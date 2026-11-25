//! US-14.1 / 14.2 — fenêtre « Image de fond » du dock et affichage du fond sous le dessin.
//!
//! La fenêtre importe une photo ou un scan (PNG, JPEG) : copie à côté du projet, calibration par
//! deux points (distance connue) ou quatre points (rectangle connu, correction de perspective) sur
//! un aperçu cliquable, résidu affiché, réglages d'affichage, reliage, retrait ; puis le bloc du
//! décalque (accrochage aux contours, lissage en Béziers). **Aucun calcul géométrique ici** :
//! `pf_core::background_ops` (calibration), `pf_geometry` (contours, ajustement). Rien n'est
//! exporté : le fond ne passe jamais par `Scene` (PDF, SVG, DXF, HPGL inchangés).
//!
//! Le fichier image est lu et haché **une fois** par couple (chemin, empreinte) : le résultat est
//! gardé dans `BgView` et recalculé seulement après un changement de fond, d'enregistrement ou à la
//! demande (« Revérifier »).

use crate::app::App;
use crate::canvas::Xf;
use crate::dock::ToolWin;
use eframe::egui::{self, Color32, Pos2, Rect, RichText, Sense, Stroke, TextureHandle, Vec2};
use pf_core::background_image::{self as bi, ImageStatus};
use pf_core::{Background, CalibrationSpec, Command, PlanePoint, SheetId};
use pf_geometry::PointMm;
use pf_i18n::{tr, tr_args, tr_len};
use pf_ui::components::{button, ButtonKind};

/// Plus grand côté (pixels) de la texture envoyée au processeur graphique.
const MAX_TEXTURE_SIDE: u32 = 4096;
/// Découpage du maillage de l'image (la perspective n'est pas affine : une grille, pas deux
/// triangles).
const MESH_DIVISIONS: usize = 16;
/// Hauteur maximale de l'aperçu de calibration (points d'écran).
const PREVIEW_MAX_HEIGHT: f32 = 300.0;

/// Mode de calibration choisi dans la fenêtre.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum CalMode {
    #[default]
    TwoPoints,
    FourPoints,
}

impl CalMode {
    fn needed(self) -> usize {
        match self {
            CalMode::TwoPoints => 2,
            CalMode::FourPoints => 4,
        }
    }
}

/// Image choisie, pas encore posée (ou à recalibrer) : aperçu et points cliqués.
pub struct Pending {
    pub image: bi::ImportedImage,
    pub tex: TextureHandle,
    pub points: Vec<PlanePoint>,
    /// Vrai : la calibration d'un fond existant est remplacée ; faux : un fond est posé.
    pub recalibrate: bool,
}

/// État de la fenêtre (jamais enregistré).
#[derive(Default)]
pub struct BackgroundDialog {
    pub path_text: String,
    pub relink_text: String,
    pub pending: Option<Pending>,
    pub mode: CalMode,
    pub dist_text: String,
    pub width_text: String,
    pub height_text: String,
    pub error: Option<String>,
}

/// Fichier et texture du fond de la feuille active (cache d'affichage).
#[derive(Default)]
pub struct BgView {
    key: Option<(std::path::PathBuf, SheetId, String, String)>,
    pub status: Option<ImageStatus>,
    tex: Option<TextureHandle>,
}

impl BgView {
    /// Force une nouvelle lecture du fichier à la prochaine image.
    pub fn invalidate(&mut self) {
        self.key = None;
    }
}

/// Réduction entière par moyenne de blocs (`k` ≥ 1).
fn downscale(img: &bi::DecodedImage, k: u32) -> (usize, usize, Vec<u8>) {
    if k <= 1 {
        return (img.width as usize, img.height as usize, img.rgba.clone());
    }
    let (w, h) = (img.width as usize, img.height as usize);
    let k = k as usize;
    let (nw, nh) = (w.div_ceil(k), h.div_ceil(k));
    let mut out = vec![0u8; nw * nh * 4];
    for y in 0..nh {
        for x in 0..nw {
            let mut acc = [0u32; 4];
            let mut n = 0u32;
            for yy in (y * k)..((y + 1) * k).min(h) {
                for xx in (x * k)..((x + 1) * k).min(w) {
                    let p = &img.rgba[(yy * w + xx) * 4..(yy * w + xx) * 4 + 4];
                    for c in 0..4 {
                        acc[c] += u32::from(p[c]);
                    }
                    n += 1;
                }
            }
            for c in 0..4 {
                out[(y * nw + x) * 4 + c] = (acc[c] / n) as u8;
            }
        }
    }
    (nw, nh, out)
}

/// Texture d'une image décodée (réduite si elle dépasse `MAX_TEXTURE_SIDE`).
fn make_texture(ctx: &egui::Context, name: &str, img: &bi::DecodedImage) -> TextureHandle {
    let k = img.width.max(img.height).div_ceil(MAX_TEXTURE_SIDE).max(1);
    let (w, h, rgba) = downscale(img, k);
    let color = egui::ColorImage::from_rgba_unmultiplied([w, h], &rgba);
    ctx.load_texture(name, color, egui::TextureOptions::LINEAR)
}

impl App {
    /// Ouvre la fenêtre « Image de fond ».
    pub fn open_background(&mut self) {
        if self.background.is_none() {
            self.background = Some(BackgroundDialog::default());
        }
    }

    /// Dossier du projet enregistré (`None` : projet jamais enregistré).
    pub(crate) fn project_dir(&self) -> Option<std::path::PathBuf> {
        let path = self.editor.doc.path.as_ref()?;
        Some(path.parent().map_or_else(
            || std::path::PathBuf::from("."),
            std::path::Path::to_path_buf,
        ))
    }

    fn active_background(&self) -> Option<Background> {
        self.editor
            .project()
            .sheet(self.editor.sheet)
            .ok()?
            .background
            .clone()
    }

    /// Relit le fichier du fond de la feuille active si son couple (chemin, empreinte) a changé.
    pub(crate) fn bg_refresh(&mut self, ctx: &egui::Context) {
        let bg = self.active_background();
        let dir = self.project_dir();
        let key = match (&bg, &dir) {
            (Some(b), Some(d)) => Some((
                d.clone(),
                self.editor.sheet,
                b.image.clone(),
                b.sha256.clone(),
            )),
            _ => None,
        };
        if key == self.bg_view.key && (key.is_some() || self.bg_view.status.is_none()) {
            return;
        }
        self.bg_view.key = key;
        self.bg_view.tex = None;
        self.bg_view.status = None;
        let (Some(b), Some(d)) = (bg, dir) else {
            return;
        };
        let status = bi::status(&d, &b);
        if status == ImageStatus::Ok {
            match bi::load(&d, &b) {
                Ok(img) => self.bg_view.tex = Some(make_texture(ctx, "background", &img)),
                Err(e) => {
                    self.bg_view.status = Some(ImageStatus::Unreadable(e));
                    return;
                }
            }
        }
        self.bg_view.status = Some(status);
    }

    /// Dessine le fond sous la grille et les tracés, à l'échelle du modèle (maillage de
    /// `MESH_DIVISIONS` côtés, les sommets passent par la calibration). Un fond introuvable ou
    /// modifié n'est pas dessiné : une bannière le dit.
    pub(crate) fn paint_background(
        &mut self,
        ctx: &egui::Context,
        painter: &egui::Painter,
        xf: &Xf,
    ) {
        self.bg_refresh(ctx);
        let Some(bg) = self.active_background() else {
            return;
        };
        if !bg.visible {
            return;
        }
        match &self.bg_view.status {
            Some(ImageStatus::Ok) => {}
            Some(other) => {
                let key = match other {
                    ImageStatus::Missing => "bg.status.missing",
                    ImageStatus::Modified => "bg.status.modified",
                    _ => "bg.status.unreadable",
                };
                let rect = painter.clip_rect();
                painter.text(
                    rect.left_bottom() + Vec2::new(10.0, -44.0),
                    egui::Align2::LEFT_BOTTOM,
                    tr(key),
                    egui::FontId::proportional(12.0),
                    Color32::from_rgb(200, 60, 40),
                );
                return;
            }
            None => return,
        }
        let (Some(tex), Ok(map)) = (&self.bg_view.tex, bg.map()) else {
            return;
        };
        let alpha = (bg.opacity.clamp(0.0, 1.0) * 255.0).round() as u8;
        let tint = Color32::from_white_alpha(alpha);
        let (w, h) = (f64::from(bg.width_px), f64::from(bg.height_px));
        let n = MESH_DIVISIONS;
        let mut mesh = egui::Mesh::with_texture(tex.id());
        for j in 0..=n {
            for i in 0..=n {
                let (u, v) = (i as f64 / n as f64, j as f64 / n as f64);
                let Some(mm) = map.to_mm(PointMm::new(u * w, v * h)) else {
                    return;
                };
                mesh.vertices.push(egui::epaint::Vertex {
                    pos: xf.s(mm),
                    uv: Pos2::new(u as f32, v as f32),
                    color: tint,
                });
            }
        }
        for j in 0..n {
            for i in 0..n {
                let a = (j * (n + 1) + i) as u32;
                let (b, c, d) = (a + 1, a + (n as u32 + 1), a + (n as u32 + 2));
                mesh.indices.extend_from_slice(&[a, b, d, a, d, c]);
            }
        }
        painter.add(egui::Shape::mesh(mesh));
    }

    /// Importe le fichier désigné : copie à côté du projet, aperçu de calibration. Refus affiché.
    pub(crate) fn bg_import(&mut self, ctx: &egui::Context, recalibrate: bool) {
        let Some(d) = self.background.as_mut() else {
            return;
        };
        d.error = None;
        let source = std::path::PathBuf::from(d.path_text.trim());
        let Some(path) = self.editor.doc.path.clone() else {
            d.error = Some(tr("bg.err.unsaved"));
            return;
        };
        let imported = match bi::import_image(&path, &source) {
            Ok(i) => i,
            Err(e) => {
                d.error = Some(tr_args("bg.err.import", &[("why", &e)]));
                return;
            }
        };
        let dir = path.parent().unwrap_or_else(|| std::path::Path::new("."));
        let Some(file) = bi::resolve(dir, &imported.relative) else {
            return;
        };
        let decoded = bi::read_bounded(&file).and_then(|b| bi::decode(&b));
        match decoded {
            Ok(img) => {
                d.pending = Some(Pending {
                    image: imported,
                    tex: make_texture(ctx, "background_preview", &img),
                    points: Vec::new(),
                    recalibrate,
                });
            }
            Err(e) => d.error = Some(tr_args("bg.err.import", &[("why", &e)])),
        }
    }

    /// Calibration demandée par la fenêtre (points cliqués et longueurs saisies).
    fn bg_spec(d: &BackgroundDialog) -> Result<CalibrationSpec, String> {
        let p = d.pending.as_ref().ok_or_else(|| tr("bg.err.no_image"))?;
        if p.points.len() != d.mode.needed() {
            return Err(tr_args(
                "bg.err.points",
                &[("n", &d.mode.needed().to_string())],
            ));
        }
        let len = |t: &str| {
            pf_core::units::parse_length_display(t.trim()).map_err(|_| tr("bg.err.length"))
        };
        Ok(match d.mode {
            CalMode::TwoPoints => CalibrationSpec::TwoPoints {
                points_px: [p.points[0], p.points[1]],
                known_distance_mm: len(&d.dist_text)?,
            },
            CalMode::FourPoints => CalibrationSpec::FourPoints {
                points_px: [p.points[0], p.points[1], p.points[2], p.points[3]],
                known_width_mm: len(&d.width_text)?,
                known_height_mm: len(&d.height_text)?,
            },
        })
    }

    /// « Poser le fond » / « Recalibrer » : une commande, une étape.
    pub(crate) fn bg_confirm(&mut self) {
        let Some(d) = self.background.as_mut() else {
            return;
        };
        let spec = match Self::bg_spec(d) {
            Ok(s) => s,
            Err(e) => {
                d.error = Some(e);
                return;
            }
        };
        let Some(p) = d.pending.as_ref() else {
            return;
        };
        let sheet = self.editor.sheet;
        let command = if p.recalibrate {
            Command::CalibrateBackground {
                sheet,
                calibration: spec,
            }
        } else {
            Command::SetBackground {
                sheet,
                image: p.image.relative.clone(),
                sha256: p.image.sha256.clone(),
                width_px: p.image.width_px,
                height_px: p.image.height_px,
                calibration: spec,
            }
        };
        if self.editor.exec(command).is_some() {
            if let Some(d) = self.background.as_mut() {
                d.pending = None;
                d.error = None;
            }
            self.bg_view.invalidate();
        } else if let Some(d) = self.background.as_mut() {
            d.error = Some(self.editor.status.text.clone());
        }
    }

    /// Relie le fond à l'image désignée (mêmes dimensions) : copie puis `RelinkBackground`.
    fn bg_relink(&mut self) {
        let Some(d) = self.background.as_mut() else {
            return;
        };
        d.error = None;
        let source = std::path::PathBuf::from(d.relink_text.trim());
        let Some(path) = self.editor.doc.path.clone() else {
            d.error = Some(tr("bg.err.unsaved"));
            return;
        };
        match bi::import_image(&path, &source) {
            Ok(i) => {
                let sheet = self.editor.sheet;
                let ok = self
                    .editor
                    .exec(Command::RelinkBackground {
                        sheet,
                        image: i.relative,
                        sha256: i.sha256,
                        width_px: i.width_px,
                        height_px: i.height_px,
                    })
                    .is_some();
                if ok {
                    self.bg_view.invalidate();
                } else if let Some(d) = self.background.as_mut() {
                    d.error = Some(self.editor.status.text.clone());
                }
            }
            Err(e) => d.error = Some(tr_args("bg.err.import", &[("why", &e)])),
        }
    }

    fn bg_display(&mut self, opacity: f64, visible: bool, locked: bool) {
        let sheet = self.editor.sheet;
        let _ = self.editor.exec(Command::SetBackgroundDisplay {
            sheet,
            opacity,
            visible,
            locked,
        });
    }

    /// Fenêtre « Image de fond » du dock.
    pub fn background_window(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.background.take() else {
            return;
        };
        let tokens = pf_ui::theme::tokens(ctx);
        let (secondary, danger) = (tokens.text_secondary, tokens.danger);
        let slot = crate::dock::slot(ctx, &self.dock, ToolWin::Background);
        let width = self.dock.content_width(ToolWin::Background);
        let bg = self.active_background();
        let status = self.bg_view.status.clone();
        let (mut open, mut close) = (true, false);
        let (mut do_import, mut do_confirm, mut do_relink, mut do_remove) =
            (false, false, false, false);
        let mut recalibrate = false;
        let mut recheck = false;
        let mut display: Option<(f64, bool, bool)> = None;
        egui::Window::new(tr("bg.title"))
            .id(ToolWin::Background.id())
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .fixed_pos(slot.pos)
            .default_height(slot.max_height)
            .max_height(slot.max_height)
            .show(ctx, |ui| {
                ui.set_width(width);
                egui::ScrollArea::vertical().show(ui, |ui| {
                    crate::a11y::label(ui, RichText::new(tr("bg.intro")).color(secondary));
                    ui.add_space(4.0);
                    if let Some(e) = &d.error {
                        crate::a11y::label(ui, RichText::new(e).color(danger));
                    }
                    match (&bg, d.pending.is_some()) {
                        (_, true) => Self::bg_calibration_ui(ui, &mut d, &mut do_confirm),
                        (Some(b), false) => {
                            Self::bg_existing_ui(
                                ui,
                                b,
                                status.as_ref(),
                                &mut d,
                                &mut display,
                                &mut recalibrate,
                                &mut do_relink,
                                &mut do_remove,
                                &mut recheck,
                            );
                        }
                        (None, false) => {
                            crate::a11y::label(ui, tr("bg.path"));
                            crate::a11y::add_text(
                                ui,
                                egui::TextEdit::singleline(&mut d.path_text).desired_width(width),
                            );
                            ui.horizontal(|ui| {
                                do_import =
                                    button(ui, &tr("bg.import"), ButtonKind::Primary).clicked();
                                if button(ui, &tr("bg.browse"), ButtonKind::Secondary).clicked() {
                                    if let Some(p) = rfd::FileDialog::new()
                                        .add_filter(tr("bg.filter"), &["png", "jpg", "jpeg"])
                                        .pick_file()
                                    {
                                        d.path_text = p.display().to_string();
                                    }
                                }
                            });
                        }
                    }
                    if bg.is_some() && d.pending.is_none() {
                        ui.separator();
                        self.trace_section(ui, &mut d);
                    }
                    ui.add_space(6.0);
                    close = button(ui, &tr("dialog.close"), ButtonKind::Secondary).clicked();
                });
            });
        if open && !close {
            self.background = Some(d);
        } else {
            self.trace_cancel_job();
        }
        if do_import {
            self.bg_import(ctx, false);
        }
        if recalibrate {
            // Recalibrer : on repart de l'image déjà posée (aperçu relu depuis la copie).
            if let Some(b) = self.active_background() {
                if let Some(dir) = self.project_dir() {
                    if let Some(p) = bi::resolve(&dir, &b.image) {
                        if let Some(dd) = self.background.as_mut() {
                            dd.path_text = p.display().to_string();
                        }
                        self.bg_import(ctx, true);
                    }
                }
            }
        }
        if do_confirm {
            self.bg_confirm();
        }
        if do_relink {
            self.bg_relink();
        }
        if do_remove {
            let sheet = self.editor.sheet;
            let _ = self.editor.exec(Command::RemoveBackground { sheet });
            self.bg_view.invalidate();
        }
        if recheck {
            self.bg_view.invalidate();
        }
        if let Some((o, v, l)) = display {
            self.bg_display(o, v, l);
        }
    }

    /// Aperçu cliquable, mode, longueurs connues, résidu prévisionnel et boutons.
    fn bg_calibration_ui(ui: &mut egui::Ui, d: &mut BackgroundDialog, confirm: &mut bool) {
        crate::a11y::label(ui, tr("bg.mode"));
        ui.horizontal(|ui| {
            for (mode, key) in [
                (CalMode::TwoPoints, "bg.mode.two"),
                (CalMode::FourPoints, "bg.mode.four"),
            ] {
                if ui.selectable_label(d.mode == mode, tr(key)).clicked() {
                    d.mode = mode;
                    if let Some(p) = d.pending.as_mut() {
                        p.points.truncate(mode.needed());
                    }
                }
            }
        });
        let needed = d.mode.needed();
        crate::a11y::label(
            ui,
            RichText::new(tr(if d.mode == CalMode::TwoPoints {
                "bg.click.two"
            } else {
                "bg.click.four"
            }))
            .color(ui.visuals().weak_text_color()),
        );
        if let Some(p) = d.pending.as_mut() {
            let (iw, ih) = (p.image.width_px as f32, p.image.height_px as f32);
            let scale = (ui.available_width().min(440.0) / iw).min(PREVIEW_MAX_HEIGHT / ih);
            let (rect, resp) =
                ui.allocate_exact_size(Vec2::new(iw * scale, ih * scale), Sense::click());
            resp.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Button, true, tr("bg.preview"))
            });
            let painter = ui.painter_at(rect);
            painter.image(
                p.tex.id(),
                rect,
                Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                Color32::WHITE,
            );
            if resp.clicked() && p.points.len() < needed {
                if let Some(pos) = resp.interact_pointer_pos() {
                    let x = f64::from(((pos.x - rect.left()) / scale).clamp(0.0, iw));
                    let y = f64::from(((pos.y - rect.top()) / scale).clamp(0.0, ih));
                    if !p.points.iter().any(|q| q.x == x && q.y == y) {
                        p.points.push(PlanePoint { x, y });
                    }
                }
            }
            if resp.secondary_clicked() {
                p.points.pop();
            }
            for (i, q) in p.points.iter().enumerate() {
                let at = rect.min + Vec2::new(q.x as f32 * scale, q.y as f32 * scale);
                painter.circle_stroke(
                    at,
                    5.0,
                    Stroke::new(2.0_f32, Color32::from_rgb(220, 40, 40)),
                );
                painter.text(
                    at + Vec2::new(7.0, -7.0),
                    egui::Align2::LEFT_BOTTOM,
                    (i + 1).to_string(),
                    egui::FontId::proportional(12.0),
                    Color32::from_rgb(220, 40, 40),
                );
            }
            // Réglage fin : coordonnées en pixels de l'image (l'aperçu est réduit).
            let (w, h) = (f64::from(p.image.width_px), f64::from(p.image.height_px));
            egui::Grid::new("bg_points").num_columns(4).show(ui, |ui| {
                for (i, q) in p.points.iter_mut().enumerate() {
                    crate::a11y::label(ui, tr_args("bg.point.x", &[("n", &(i + 1).to_string())]));
                    crate::a11y::add_drag(
                        ui,
                        egui::DragValue::new(&mut q.x)
                            .range(0.0..=w)
                            .speed(0.5)
                            .max_decimals(2),
                    );
                    crate::a11y::label(ui, tr_args("bg.point.y", &[("n", &(i + 1).to_string())]));
                    crate::a11y::add_drag(
                        ui,
                        egui::DragValue::new(&mut q.y)
                            .range(0.0..=h)
                            .speed(0.5)
                            .max_decimals(2),
                    );
                    ui.end_row();
                }
            });
            ui.horizontal(|ui| {
                if button(ui, &tr("bg.point.remove_last"), ButtonKind::Secondary).clicked() {
                    p.points.pop();
                }
            });
        }
        egui::Grid::new("bg_lengths")
            .num_columns(2)
            .show(ui, |ui| match d.mode {
                CalMode::TwoPoints => {
                    crate::a11y::label(ui, tr("bg.known.distance"));
                    crate::a11y::add_text(ui, egui::TextEdit::singleline(&mut d.dist_text));
                    ui.end_row();
                }
                CalMode::FourPoints => {
                    crate::a11y::label(ui, tr("bg.known.width"));
                    crate::a11y::add_text(ui, egui::TextEdit::singleline(&mut d.width_text));
                    ui.end_row();
                    crate::a11y::label(ui, tr("bg.known.height"));
                    crate::a11y::add_text(ui, egui::TextEdit::singleline(&mut d.height_text));
                    ui.end_row();
                }
            });
        let recalibrate = d.pending.as_ref().is_some_and(|p| p.recalibrate);
        ui.horizontal(|ui| {
            let label = if recalibrate {
                "bg.recalibrate.apply"
            } else {
                "bg.apply"
            };
            *confirm = button(ui, &tr(label), ButtonKind::Primary).clicked();
            if button(ui, &tr("dialog.cancel"), ButtonKind::Secondary).clicked() {
                d.pending = None;
                d.error = None;
            }
        });
    }

    /// Fond posé : état du fichier, calibration et résidu, affichage, reliage, retrait.
    #[allow(clippy::too_many_arguments)]
    fn bg_existing_ui(
        ui: &mut egui::Ui,
        b: &Background,
        status: Option<&ImageStatus>,
        d: &mut BackgroundDialog,
        display: &mut Option<(f64, bool, bool)>,
        recalibrate: &mut bool,
        relink: &mut bool,
        remove: &mut bool,
        recheck: &mut bool,
    ) {
        let (status_key, bad) = match status {
            Some(ImageStatus::Ok) => ("bg.status.ok", false),
            Some(ImageStatus::Missing) => ("bg.status.missing", true),
            Some(ImageStatus::Modified) => ("bg.status.modified", true),
            Some(ImageStatus::Unreadable(_)) => ("bg.status.unreadable", true),
            None => ("bg.status.unknown", true),
        };
        let color = if bad {
            Color32::from_rgb(200, 60, 40)
        } else {
            ui.visuals().text_color()
        };
        crate::a11y::label(ui, RichText::new(tr(status_key)).color(color));
        crate::a11y::label(
            ui,
            RichText::new(tr_args("bg.file", &[("path", &b.image)]))
                .color(ui.visuals().weak_text_color()),
        );
        if button(ui, &tr("bg.recheck"), ButtonKind::Secondary).clicked() {
            *recheck = true;
        }
        // Calibration et résidu (recalculés depuis les points, jamais lus du champ stocké).
        match b.residuals() {
            Ok(r) => {
                let scale = match &b.calibration {
                    pf_core::Calibration::TwoPoints { .. } => b.map().ok().and_then(|m| {
                        let a = m.to_mm(PointMm::new(0.0, 0.0))?;
                        let c = m.to_mm(PointMm::new(1.0, 0.0))?;
                        Some(pf_geometry::distance_mm(a, c))
                    }),
                    _ => None,
                };
                let kind = match &b.calibration {
                    pf_core::Calibration::TwoPoints {
                        known_distance_mm, ..
                    } => tr_len(
                        "bg.cal.two",
                        &[
                            ("d", &pf_core::units::len_number(*known_distance_mm, 3)),
                            (
                                "scale",
                                &scale.map_or("—".into(), |s| pf_core::units::len_number(s, 4)),
                            ),
                        ],
                    ),
                    pf_core::Calibration::FourPoints {
                        known_width_mm,
                        known_height_mm,
                        ..
                    } => tr_len(
                        "bg.cal.four",
                        &[
                            ("w", &pf_core::units::len_number(*known_width_mm, 3)),
                            ("h", &pf_core::units::len_number(*known_height_mm, 3)),
                        ],
                    ),
                };
                crate::a11y::label(ui, kind);
                crate::a11y::label(
                    ui,
                    tr_len(
                        "bg.residual.rms",
                        &[("rms", &pf_core::units::len_number(r.rms_mm, 6))],
                    ),
                );
                let per_point: Vec<String> = r
                    .per_point_mm
                    .iter()
                    .enumerate()
                    .map(|(i, e)| format!("{} : {}", i + 1, pf_core::units::len_number(*e, 6)))
                    .collect();
                crate::a11y::label(
                    ui,
                    tr_len("bg.residual.points", &[("list", &per_point.join(" ; "))]),
                );
                crate::a11y::label(
                    ui,
                    RichText::new(tr("bg.residual.note")).color(ui.visuals().weak_text_color()),
                );
            }
            Err(e) => {
                crate::a11y::label(
                    ui,
                    RichText::new(e.to_string()).color(Color32::from_rgb(200, 60, 40)),
                );
            }
        }
        // Affichage : opacité, visibilité, verrou (une commande chacun).
        let (mut opacity, mut visible, mut locked) = (b.opacity, b.visible, b.locked);
        egui::Grid::new("bg_display").num_columns(2).show(ui, |ui| {
            crate::a11y::label(ui, tr("bg.opacity"));
            let r = crate::a11y::add_drag(
                ui,
                egui::DragValue::new(&mut opacity)
                    .range(0.0..=1.0)
                    .speed(0.01)
                    .fixed_decimals(2),
            );
            if r.drag_stopped() || (r.changed() && !r.dragged()) {
                *display = Some((opacity, visible, locked));
            }
            ui.end_row();
        });
        if ui.checkbox(&mut visible, tr("bg.visible")).changed()
            || ui.checkbox(&mut locked, tr("bg.locked")).changed()
        {
            *display = Some((opacity, visible, locked));
        }
        ui.horizontal(|ui| {
            ui.add_enabled_ui(!b.locked, |ui| {
                if button(ui, &tr("bg.recalibrate"), ButtonKind::Secondary).clicked() {
                    *recalibrate = true;
                }
                if button(ui, &tr("bg.remove"), ButtonKind::Secondary).clicked() {
                    *remove = true;
                }
            });
        });
        // Reliage explicite (image introuvable ou modifiée, ou autre fichier de mêmes dimensions).
        ui.add_enabled_ui(!b.locked, |ui| {
            crate::a11y::label(ui, tr("bg.relink.path"));
            crate::a11y::add_text(ui, egui::TextEdit::singleline(&mut d.relink_text));
            if button(ui, &tr("bg.relink"), ButtonKind::Secondary).clicked() {
                *relink = true;
            }
        });
    }
}

/// Image de demonstration (600 x 400 px) : papier clair et contour sombre d'un patron en jupe
/// (cotes droits, ourlet courbe). Generee par le code : aucun fichier binaire n'est versionne.
pub(crate) fn demo_pattern_image() -> image::RgbImage {
    let outline = demo_outline_px();
    let mut img = image::RgbImage::from_pixel(600, 400, image::Rgb([236, 232, 222]));
    for y in 0..400u32 {
        for x in 0..600u32 {
            let p = (f64::from(x), f64::from(y));
            let d = outline
                .windows(2)
                .map(|w| dist_to_segment(p, w[0], w[1]))
                .fold(f64::MAX, f64::min);
            if d <= 2.0 {
                img.put_pixel(x, y, image::Rgb([30, 30, 40]));
            }
        }
    }
    img
}

/// Contour du patron de demonstration, en pixels de l'image (polyligne fermee).
pub(crate) fn demo_outline_px() -> Vec<(f64, f64)> {
    let mut pts = vec![(150.0, 60.0), (450.0, 60.0), (500.0, 330.0)];
    // Ourlet courbe : de (500 ; 330) a (100 ; 330), creuse de 30 px au milieu.
    for i in 1..16 {
        let t = f64::from(i) / 16.0;
        pts.push((
            500.0 - 400.0 * t,
            330.0 + 30.0 * (std::f64::consts::PI * t).sin(),
        ));
    }
    pts.push((100.0, 330.0));
    pts.push((150.0, 60.0));
    pts
}

fn dist_to_segment(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len2 = dx * dx + dy * dy;
    let t = if len2 == 0.0 {
        0.0
    } else {
        (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / len2).clamp(0.0, 1.0)
    };
    (p.0 - a.0 - t * dx).hypot(p.1 - a.1 - t * dy)
}

/// Etape de capture « fond-image » : projet enregistre dans un dossier temporaire avec l'image de
/// demonstration calibree a deux points (1 px = 0,5 mm), contours calcules, quelques points de
/// decalque et fenetre ouverte.
pub(crate) fn background_stage(app: &mut App) -> Result<(), String> {
    let dir = std::env::temp_dir().join(format!(
        "patternforge-etape-fond-{}-{}",
        std::process::id(),
        pf_i18n::lang().code()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let file = dir.join("patron.pforge.json");
    let source = dir.join("photo.png");
    demo_pattern_image()
        .save(&source)
        .map_err(|e| e.to_string())?;
    let project = pf_core::Project::new(&tr("project.default_name"));
    app.editor = crate::editor::Editor::new(pf_core::Document::new(project));
    app.editor.doc.save_as(&file).map_err(|e| e.to_string())?;
    // Vue cadree sur l'image (300 x 200 mm, de (-50 ; -35) a (250 ; 165) mm apres calibration).
    app.fit_pending = false;
    app.view = crate::view::View {
        center: PointMm::new(100.0, 65.0),
        zoom: 1.5,
    };
    let img = bi::import_image(&file, &source)?;
    let sheet = app.editor.sheet;
    app.editor
        .exec(Command::SetBackground {
            sheet,
            image: img.relative,
            sha256: img.sha256,
            width_px: img.width_px,
            height_px: img.height_px,
            calibration: CalibrationSpec::TwoPoints {
                points_px: [
                    PlanePoint { x: 100.0, y: 330.0 },
                    PlanePoint { x: 500.0, y: 330.0 },
                ],
                known_distance_mm: 200.0,
            },
        })
        .ok_or_else(|| app.editor.status.text.clone())?;
    // Contours calcules ici (le fil separe n'est pas deterministe pour une capture).
    let bg = app
        .editor
        .project()
        .sheet(sheet)
        .map_err(|e| e.to_string())?
        .background
        .clone()
        .ok_or("fond absent")?;
    let decoded = bi::load(&dir, &bg)?;
    let map = pf_geometry::edges::sobel(
        &decoded.gray(),
        decoded.width as usize,
        decoded.height as usize,
        &std::sync::atomic::AtomicBool::new(false),
    )?
    .ok_or("calcul annule")?;
    app.editor.trace.edges = Some(crate::trace::EdgeCache {
        key: bg.sha256.clone(),
        map: std::sync::Arc::new(map),
    });
    app.editor.trace.snap_edges = true;
    // Quelques points de l'ourlet (positions du modele), lisses en un apercu.
    let m = bg.map().map_err(|e| e.to_string())?;
    app.editor.trace.points = demo_outline_px()
        .iter()
        .skip(3)
        .step_by(3)
        .filter_map(|(x, y)| m.to_mm(PointMm::new(*x, *y)))
        .collect();
    app.editor.set_tool(crate::editor::Tool::Trace);
    app.open_background();
    app.bg_view.invalidate();
    Ok(())
}
