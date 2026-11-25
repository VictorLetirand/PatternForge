//! Fenêtre « Construction rapide » (PH-4a) : polygone, rectangle orienté, rectangle à coins
//! arrondis. Elle ne calcule rien : l'aperçu (dimensions, périmètre, aire, nombre de tracés et de
//! centres d'arcs) vient de `pf_geometry::quick`, qui sert aussi à la commande ; la pose est
//! `Command::AddPolygon` / `AddOrientedRectangle` / `AddRoundedRectangle` (une seule étape
//! annulable, refus sans trace, erreur du cœur affichée dans la fenêtre).
//!
//! Limites : les sommets d'un polygone se saisissent au clavier (« x;y », un par ligne), pas par
//! clic sur le plan ; pas d'aperçu dessiné sur le plan avant la pose.

use crate::app::App;
use crate::editor::Editor;
use crate::palette::pal;
use eframe::egui::{self, RichText};
use pf_core::input::{format_mm, format_number, parse_angle_deg};
use pf_core::units::parse_length_display;
use pf_core::{Command, PathRole};
use pf_geometry::quick::{
    measure_polygon, measure_rectangle, measure_rounded_rectangle, QuickMeasure,
};
use pf_geometry::{PointMm, RectSpec};
use pf_i18n::{tr, tr_args};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuickKind {
    Polygon,
    Rectangle,
    Rounded,
}

/// État de la fenêtre (interface seulement, jamais enregistré).
#[derive(Debug, Clone, PartialEq)]
pub struct QuickUi {
    pub open: bool,
    pub kind: QuickKind,
    /// Un sommet « x;y » par ligne.
    pub vertices: String,
    pub closed: bool,
    pub x: String,
    pub y: String,
    pub width: String,
    pub height: String,
    pub angle: String,
    pub centered: bool,
    pub radius: String,
    /// Range les centres d'arcs dans le calque masqué « Centres d'arcs ».
    pub hide_centers: bool,
    /// Dernier refus du cœur (effacé par une création réussie).
    pub error: Option<String>,
}

impl Default for QuickUi {
    fn default() -> Self {
        QuickUi {
            open: false,
            kind: QuickKind::Rounded,
            vertices: String::new(),
            closed: true,
            x: "0".into(),
            y: "0".into(),
            width: "100".into(),
            height: "60".into(),
            angle: "0".into(),
            centered: false,
            radius: "5".into(),
            hide_centers: true,
            error: None,
        }
    }
}

/// Sommets lus dans la zone de texte : une ligne « x;y » par sommet, lignes vides ignorées.
fn parse_vertices(text: &str) -> Result<Vec<PointMm>, String> {
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let bad = || {
            tr_args(
                "quick.error.line",
                &[("n", &(i + 1).to_string()), ("text", line.trim())],
            )
        };
        let (a, b) = line.split_once(';').ok_or_else(bad)?;
        let x = parse_length_display(a).map_err(|_| bad())?;
        let y = parse_length_display(b).map_err(|_| bad())?;
        out.push(PointMm::new(x, y));
    }
    Ok(out)
}

impl QuickUi {
    fn rect(&self) -> Result<RectSpec, String> {
        let bad = || tr("quick.error.values");
        let v = |s: &str| parse_length_display(s).map_err(|_| bad());
        Ok(RectSpec {
            origin: PointMm::new(v(&self.x)?, v(&self.y)?),
            width_mm: v(&self.width)?,
            height_mm: v(&self.height)?,
            rotation_deg: parse_angle_deg(&self.angle).map_err(|_| bad())?,
            centered: self.centered,
        })
    }
}

impl Editor {
    /// Commande décrite par la fenêtre (valeurs lues, rien n'est validé ici).
    pub(crate) fn quick_command(&self) -> Result<Command, String> {
        let ui = &self.quick;
        let sheet = self.sheet;
        let layer = self.current_layer();
        let role = PathRole::Construction;
        Ok(match ui.kind {
            QuickKind::Polygon => Command::AddPolygon {
                sheet,
                vertices: parse_vertices(&ui.vertices)?
                    .into_iter()
                    .map(pf_core::PointRef::New)
                    .collect(),
                closed: ui.closed,
                role,
                layer,
            },
            QuickKind::Rectangle => Command::AddOrientedRectangle {
                sheet,
                rect: ui.rect()?,
                role,
                layer,
            },
            QuickKind::Rounded => Command::AddRoundedRectangle {
                sheet,
                rect: ui.rect()?,
                radius_mm: parse_length_display(&ui.radius)
                    .map_err(|_| tr("quick.error.values"))?,
                hide_centers: ui.hide_centers,
                role,
                layer,
            },
        })
    }

    /// Mesures annoncées (lecture seule : mêmes contrôles et même disposition que la commande).
    fn quick_preview(&self) -> Result<QuickMeasure, String> {
        let to_text =
            |e: pf_geometry::GeometryError| crate::text::core_error(&pf_core::CoreError::from(e));
        match self.quick_command()? {
            Command::AddPolygon {
                vertices, closed, ..
            } => {
                let pts: Vec<PointMm> = vertices
                    .iter()
                    .filter_map(|v| match v {
                        pf_core::PointRef::New(p) => Some(*p),
                        _ => None,
                    })
                    .collect();
                measure_polygon(&pts, closed).map_err(to_text)
            }
            Command::AddOrientedRectangle { rect, .. } => measure_rectangle(&rect).map_err(to_text),
            Command::AddRoundedRectangle {
                rect, radius_mm, ..
            } => measure_rounded_rectangle(&rect, radius_mm).map_err(to_text),
            _ => Err(tr("quick.error.values")),
        }
    }

    /// Crée la forme : une étape annulable ; refus = message dans la fenêtre, rien n'est posé.
    pub fn quick_apply(&mut self) {
        self.quick.error = None;
        let cmd = match self.quick_command() {
            Ok(c) => c,
            Err(e) => {
                self.quick.error = Some(e.clone());
                self.error(e);
                return;
            }
        };
        let Some(out) = self.exec(cmd) else {
            self.quick.error = Some(self.status.text.clone());
            return;
        };
        self.selection.clear();
        self.selection.entities = out.entities.clone();
        self.prune_selection();
        let mut text = tr_args(
            "status.quick.done",
            &[("count", &out.entities.len().to_string())],
        );
        if !out.notes.is_empty() {
            text = format!(
                "{text} {}",
                tr_args("note.core", &[("text", &out.notes.join(" "))])
            );
        }
        self.info(text);
    }
}

fn cm2(area_mm2: f64) -> String {
    format!("{} cm²", format_number(area_mm2 / 100.0, 2))
}

impl App {
    /// Ouvre la fenêtre « Construction rapide » (menu Édition, aucun raccourci).
    pub fn open_quick(&mut self) {
        self.editor.quick.open = true;
        self.editor.quick.error = None;
    }

    pub(crate) fn quick_window(&mut self, ctx: &egui::Context) {
        if !self.editor.quick.open {
            return;
        }
        let preview = self.editor.quick_preview();
        let (mut open, mut action) = (true, 0u8);
        let slot = crate::dock::slot(ctx, &self.dock, crate::dock::ToolWin::Quick);
        let width = self.dock.content_width(crate::dock::ToolWin::Quick);
        egui::Window::new(tr("quick.title"))
            .id(egui::Id::new("quick_window"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .fixed_pos(slot.pos)
            .vscroll(true)
            .max_height(slot.max_height.min(600.0))
            .show(ctx, |ui| {
                ui.set_width(width);
                crate::a11y::label(
                    ui,
                    RichText::new(tr("quick.intro")).small().color(pal().muted),
                );
                let st = &mut self.editor.quick;
                ui.horizontal(|ui| {
                    for (kind, key) in [
                        (QuickKind::Polygon, "quick.kind.polygon"),
                        (QuickKind::Rectangle, "quick.kind.rectangle"),
                        (QuickKind::Rounded, "quick.kind.rounded"),
                    ] {
                        ui.selectable_value(&mut st.kind, kind, tr(key));
                    }
                });
                ui.separator();
                if st.kind == QuickKind::Polygon {
                    crate::a11y::label(ui, tr("quick.vertices"));
                    crate::a11y::add_text(
                        ui,
                        egui::TextEdit::multiline(&mut st.vertices)
                            .desired_rows(5)
                            .desired_width(f32::INFINITY)
                            .hint_text(tr("quick.vertices.hint")),
                    );
                    ui.checkbox(&mut st.closed, tr("quick.closed"));
                } else {
                    egui::Grid::new("quick-grid").num_columns(2).show(ui, |ui| {
                        crate::a11y::label(ui, tr("quick.origin"));
                        ui.horizontal(|ui| {
                            crate::a11y::add_text(
                                ui,
                                egui::TextEdit::singleline(&mut st.x).desired_width(70.0),
                            );
                            crate::a11y::add_text(
                                ui,
                                egui::TextEdit::singleline(&mut st.y).desired_width(70.0),
                            );
                        });
                        ui.end_row();
                        for (key, text) in [
                            ("quick.width", &mut st.width),
                            ("quick.height", &mut st.height),
                            ("quick.angle", &mut st.angle),
                        ] {
                            let label = crate::a11y::label(ui, tr(key));
                            if key == "quick.angle" {
                                label.on_hover_text(tr("quick.rotation_hint"));
                            }
                            crate::a11y::add_text(
                                ui,
                                egui::TextEdit::singleline(text).desired_width(70.0),
                            );
                            ui.end_row();
                        }
                        if st.kind == QuickKind::Rounded {
                            crate::a11y::label(ui, tr("quick.radius"));
                            crate::a11y::add_text(
                                ui,
                                egui::TextEdit::singleline(&mut st.radius).desired_width(70.0),
                            );
                            ui.end_row();
                        }
                    });
                    ui.checkbox(&mut st.centered, tr("quick.centered"));
                    if st.kind == QuickKind::Rounded {
                        ui.checkbox(&mut st.hide_centers, tr("quick.hide_centers"));
                    }
                }
                ui.separator();
                crate::a11y::label(ui, RichText::new(tr("quick.preview")).strong());
                match &preview {
                    Ok(m) => {
                        crate::a11y::label(
                            ui,
                            tr_args(
                                "quick.result.size",
                                &[
                                    ("w", &format_mm(m.width_mm)),
                                    ("h", &format_mm(m.height_mm)),
                                ],
                            ),
                        );
                        crate::a11y::label(
                            ui,
                            tr_args(
                                "quick.result.perimeter",
                                &[("perimeter", &format_mm(m.perimeter_mm))],
                            ),
                        );
                        crate::a11y::label(
                            ui,
                            match m.area_mm2 {
                                Some(a) => tr_args("quick.result.area", &[("area", &cm2(a))]),
                                None => tr("quick.result.open"),
                            },
                        );
                        crate::a11y::label(
                            ui,
                            tr_args(
                                "quick.result.counts",
                                &[
                                    ("segments", &m.segments.to_string()),
                                    ("arcs", &m.arcs.to_string()),
                                    ("centers", &m.centers.to_string()),
                                ],
                            ),
                        );
                    }
                    Err(why) => {
                        crate::a11y::label(
                            ui,
                            RichText::new(tr_args("quick.refused", &[("reason", why)]))
                                .color(pal().error),
                        );
                    }
                }
                if let Some(why) = &st.error {
                    crate::a11y::label(
                        ui,
                        RichText::new(tr_args("quick.refused", &[("reason", why)]))
                            .color(pal().error),
                    );
                }
                ui.separator();
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(preview.is_ok(), egui::Button::new(tr("quick.apply")))
                        .clicked()
                    {
                        action = 2;
                    }
                    if ui.button(tr("quick.close")).clicked() {
                        action = 3;
                    }
                });
            });
        match action {
            2 => self.editor.quick_apply(),
            3 => open = false,
            _ => {}
        }
        if !open {
            self.editor.quick.open = false;
        }
    }
}

/// Fenêtre ouverte sur un rectangle arrondi à créer (captures `--etape construction-rapide`).
pub fn demo_quick(app: &mut App) {
    app.open_quick();
    let ui = &mut app.editor.quick;
    ui.kind = QuickKind::Rounded;
    ui.x = "20".into();
    ui.y = "20".into();
    ui.width = "100".into();
    ui.height = "70".into();
    ui.radius = "12".into();
    app.editor.quick_apply();
    app.fit_pending = true;
}
