//! US-13.7 : fenêtre « Projection » (réglages dans le dock) et plein écran noir par-dessus
//! l'application. Toute la conversion mm <-> pixels est dans `projection` ; ici on ne fait que
//! dessiner. Rien n'est écrit dans le projet : tracé aplati à l'ouverture, jeté à la fermeture.
//! Précision « calculée, non mesurée sur écran ou mur » : **[HUMAIN]**.

use crate::app::App;
use crate::dock::ToolWin;
use crate::projection::{self as proj, ProjectionError, ProjectionSettings};
use eframe::egui::{self, Color32, Pos2, RichText, Sense, Stroke};
use pf_export::PathGeom;
use pf_i18n::{tr, tr_args};
use pf_ui::components::{button, ButtonKind};

const MARGIN_MM: f64 = 10.0;
const CURVE_STEPS: usize = 24;

#[derive(Default)]
pub struct ProjectionState {
    pub settings: ProjectionSettings,
    pub active: bool,
    pub error: Option<String>,
    /// Tracé de la feuille, aplati, en mm, origine en haut à gauche de son encombrement.
    pub lines: Vec<Vec<(f64, f64)>>,
}

fn range_args(value: f64, min: f64, max: f64) -> [(&'static str, String); 3] {
    [
        ("value", crate::print_setup::factor_text(value)),
        ("min", min.to_string()),
        ("max", max.to_string()),
    ]
}

pub fn projection_error(e: &ProjectionError) -> String {
    let (key, args) = match e {
        ProjectionError::ScreenWidth(v) => (
            "proj.err.width",
            range_args(*v, proj::MIN_SCREEN_MM, proj::MAX_SCREEN_MM),
        ),
        ProjectionError::GridStep(v) => (
            "proj.err.grid",
            range_args(*v, proj::MIN_GRID_MM, proj::MAX_GRID_MM),
        ),
        ProjectionError::Stroke(v) => (
            "proj.err.stroke",
            range_args(*v, proj::MIN_STROKE_MM, proj::MAX_STROKE_MM),
        ),
        ProjectionError::ScreenPixels(_) => return tr("proj.err.pixels"),
    };
    let refs: Vec<(&str, &str)> = args.iter().map(|(k, v)| (*k, v.as_str())).collect();
    tr_args(key, &refs)
}

fn flatten(geom: &PathGeom) -> Vec<(f64, f64)> {
    let beziers = match geom {
        PathGeom::Line(a, b) => return vec![(a.x, a.y), (b.x, b.y)],
        PathGeom::Cubic(c) => vec![*c],
        other => other.arc_beziers(),
    };
    let mut out = Vec::new();
    for c in &beziers {
        for i in 0..=CURVE_STEPS {
            let p = c.point_at(i as f64 / CURVE_STEPS as f64);
            out.push((p.x, p.y));
        }
    }
    out
}

impl App {
    pub fn open_projection(&mut self) {
        if self.projection.is_none() {
            self.projection = Some(ProjectionState::default());
        }
    }

    /// Valide les réglages, aplatit le tracé de la feuille et passe en plein écran.
    pub(crate) fn projection_start(&mut self, ctx: &egui::Context) {
        if self.projection_arm() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(true));
        }
    }

    /// Valide les réglages et active la projection (sans toucher à la fenêtre système).
    pub(crate) fn projection_arm(&mut self) -> bool {
        let lines = self.projection_lines();
        let Some(st) = self.projection.as_mut() else {
            return false;
        };
        match st.settings.validate() {
            Ok(()) => {
                st.error = None;
                st.lines = lines;
                st.active = true;
                true
            }
            Err(e) => {
                st.error = Some(projection_error(&e));
                false
            }
        }
    }

    fn projection_lines(&self) -> Vec<Vec<(f64, f64)>> {
        let Ok(scene) = self.export_scene(self.editor.sheet) else {
            return Vec::new();
        };
        let raw: Vec<Vec<(f64, f64)>> = scene.paths.iter().map(|p| flatten(&p.geom)).collect();
        let (min_x, max_y) = raw
            .iter()
            .flatten()
            .fold((f64::MAX, f64::MIN), |(x, y), p| (x.min(p.0), y.max(p.1)));
        raw.into_iter()
            .map(|l| l.into_iter().map(|(x, y)| (x - min_x, max_y - y)).collect())
            .collect()
    }

    pub(crate) fn projection_stop(&mut self, ctx: &egui::Context) {
        if let Some(st) = self.projection.as_mut() {
            st.active = false;
            st.lines.clear();
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
    }

    pub fn projection_window(&mut self, ctx: &egui::Context) {
        let Some(mut st) = self.projection.take() else {
            return;
        };
        if st.active {
            self.projection = Some(st);
            return;
        }
        let tokens = pf_ui::theme::tokens(ctx);
        let secondary = tokens.text_secondary;
        let (mut open, mut close, mut start) = (true, false, false);
        let slot = crate::dock::slot(ctx, &self.dock, ToolWin::Projection);
        let width = self.dock.content_width(ToolWin::Projection);
        egui::Window::new(tr("proj.title"))
            .id(ToolWin::Projection.id())
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .fixed_pos(slot.pos)
            .default_height(slot.max_height)
            .max_height(slot.max_height)
            .show(ctx, |ui| {
                ui.set_width(width);
                crate::a11y::label(ui, RichText::new(tr("proj.intro")).color(secondary));
                ui.add_space(4.0);
                egui::Grid::new("proj_grid").num_columns(2).show(ui, |ui| {
                    crate::a11y::label(ui, tr("proj.screen_width"));
                    crate::a11y::add_drag(
                        ui,
                        egui::DragValue::new(&mut st.settings.screen_width_mm)
                            .range(proj::MIN_SCREEN_MM..=proj::MAX_SCREEN_MM)
                            .speed(1.0),
                    );
                    ui.end_row();
                    crate::a11y::label(ui, tr("proj.grid"));
                    crate::a11y::add_drag(
                        ui,
                        egui::DragValue::new(&mut st.settings.grid_step_mm)
                            .range(proj::MIN_GRID_MM..=proj::MAX_GRID_MM)
                            .speed(0.5),
                    );
                    ui.end_row();
                    crate::a11y::label(ui, tr("proj.stroke"));
                    crate::a11y::add_drag(
                        ui,
                        egui::DragValue::new(&mut st.settings.stroke_mm)
                            .range(proj::MIN_STROKE_MM..=proj::MAX_STROKE_MM)
                            .speed(0.05),
                    );
                    ui.end_row();
                });
                ui.checkbox(&mut st.settings.mirror_h, tr("proj.mirror_h"));
                ui.checkbox(&mut st.settings.mirror_v, tr("proj.mirror_v"));
                crate::a11y::label(ui, RichText::new(tr("proj.human")).color(secondary));
                if let Some(e) = &st.error {
                    crate::a11y::label(ui, RichText::new(e).color(tokens.danger));
                }
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    start = button(ui, &tr("proj.start"), ButtonKind::Primary).clicked();
                    close = button(ui, &tr("dialog.close"), ButtonKind::Secondary).clicked();
                });
            });
        if open && !close {
            self.projection = Some(st);
            if start {
                self.projection_start(ctx);
            }
        }
    }

    /// Échap quitte la projection (consommé avant tout autre raccourci : il ne ferme pas aussi
    /// une fenêtre d'outil ni ne vide une sélection).
    pub(crate) fn projection_escape(&mut self, ctx: &egui::Context) {
        let active = self.projection.as_ref().is_some_and(|s| s.active);
        if active && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            self.projection_stop(ctx);
        }
    }

    /// Plein écran noir ; Échap quitte. Rien n'est dessiné hors de cette zone.
    pub fn projection_overlay(&mut self, ctx: &egui::Context) {
        let Some(st) = self.projection.as_ref().filter(|s| s.active) else {
            return;
        };
        let screen = ctx.screen_rect();
        let ppp = ctx.pixels_per_point();
        let Ok(k) = proj::pixels_per_mm(&st.settings, f64::from(screen.width() * ppp)) else {
            self.projection_stop(ctx);
            return;
        };
        let s = st.settings;
        let pt_per_mm = k / f64::from(ppp);
        let (w, h) = (f64::from(screen.width()), f64::from(screen.height()));
        let at = |x_mm: f64, y_mm: f64| -> Pos2 {
            Pos2::new(
                proj::mirror(x_mm * pt_per_mm, w, s.mirror_h) as f32 + screen.min.x,
                proj::mirror(y_mm * pt_per_mm, h, s.mirror_v) as f32 + screen.min.y,
            )
        };
        let stroke_pt = (proj::stroke_px(&s, k) / f64::from(ppp)) as f32;
        let (w_mm, h_mm) = (w / pt_per_mm, h / pt_per_mm);
        let lines = st.lines.clone();
        let hud = tr_args(
            "proj.hud",
            &[
                ("px", &format!("{:.0}", proj::mm_to_px(100.0, k))),
                (
                    "mm",
                    &format!("{:.4}", proj::px_to_mm(1.0, k))
                        .replace('.', &pf_i18n::lang().decimal_separator().to_string()),
                ),
            ],
        );
        egui::Area::new(egui::Id::new("projection_overlay"))
            .order(egui::Order::Foreground)
            .fixed_pos(screen.min)
            .show(ctx, |ui| {
                ui.allocate_rect(screen, Sense::click_and_drag());
                let p = ui.painter();
                p.rect_filled(screen, 0.0, Color32::BLACK);
                let thin = Stroke::new(1.0 / ppp, Color32::from_gray(70));
                let strong = Stroke::new(1.0 / ppp, Color32::from_gray(130));
                for (i, x) in proj::grid_lines_mm(s.grid_step_mm, w_mm).iter().enumerate() {
                    let g = if i % 5 == 0 { strong } else { thin };
                    p.line_segment([at(*x, 0.0), at(*x, h_mm)], g);
                }
                for (i, y) in proj::grid_lines_mm(s.grid_step_mm, h_mm).iter().enumerate() {
                    let g = if i % 5 == 0 { strong } else { thin };
                    p.line_segment([at(0.0, *y), at(w_mm, *y)], g);
                }
                let line = Stroke::new(stroke_pt, Color32::WHITE);
                for l in &lines {
                    let pts: Vec<Pos2> = l
                        .iter()
                        .map(|(x, y)| at(MARGIN_MM + x, MARGIN_MM + y))
                        .collect();
                    p.add(egui::Shape::line(pts, line));
                }
                let sq = proj::STANDARD_SQUARE_MM;
                let y0 = (h_mm - sq - MARGIN_MM).max(MARGIN_MM);
                let corners = vec![
                    at(MARGIN_MM, y0),
                    at(MARGIN_MM + sq, y0),
                    at(MARGIN_MM + sq, y0 + sq),
                    at(MARGIN_MM, y0 + sq),
                ];
                let gold = Color32::from_rgb(255, 220, 0);
                p.add(egui::Shape::closed_line(
                    corners,
                    Stroke::new(stroke_pt, gold),
                ));
                p.text(
                    at(MARGIN_MM + sq / 2.0, y0 + sq / 2.0),
                    egui::Align2::CENTER_CENTER,
                    tr("proj.square"),
                    egui::FontId::proportional(14.0),
                    gold,
                );
                p.text(
                    Pos2::new(screen.center().x, screen.max.y - 8.0),
                    egui::Align2::CENTER_BOTTOM,
                    hud,
                    egui::FontId::proportional(13.0),
                    Color32::from_gray(190),
                );
            });
    }
}
