//! US-14.2 — bloc « Décalque » de la fenêtre « Image de fond » et aperçu sur le plan.
//! Aucun calcul géométrique ici : `Editor::trace_*` (`trace.rs`) et `pf_geometry`.

use crate::app::App;
use crate::background::BackgroundDialog;
use crate::canvas::{stroke, Xf};
use crate::editor::Tool;
use crate::palette::pal;
use eframe::egui::{self, RichText, Shape};
use pf_i18n::{tr, tr_args, tr_len};
use pf_ui::components::{button, ButtonKind};

/// Points par Bézier dessinés pour l'aperçu du lissage.
const PREVIEW_STEPS: usize = 24;

impl App {
    /// Arrête le calcul de contours en cours (fermeture de la fenêtre).
    pub(crate) fn trace_cancel_job(&mut self) {
        self.editor.trace.cancel_edges();
    }

    /// Relit le résultat du calcul de contours ; redemande une image tant qu'il tourne.
    pub(crate) fn trace_poll(&mut self, ctx: &egui::Context) {
        self.editor.trace.poll_edges();
        if self.editor.trace.computing() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }

    /// Bloc de la fenêtre : accrochage aux contours, lissage, application.
    pub(crate) fn trace_section(&mut self, ui: &mut egui::Ui, _d: &mut BackgroundDialog) {
        let weak = ui.visuals().weak_text_color();
        let danger = ui.visuals().error_fg_color;
        crate::a11y::label(ui, RichText::new(tr("trace.title")).strong());
        crate::a11y::label(ui, RichText::new(tr("trace.intro")).color(weak));
        if button(ui, &tr("trace.use_tool"), ButtonKind::Secondary).clicked() {
            self.editor.set_tool(Tool::Trace);
        }
        ui.checkbox(&mut self.editor.trace.snap_edges, tr("trace.snap"));
        egui::Grid::new("trace_edges")
            .num_columns(2)
            .show(ui, |ui| {
                crate::a11y::label(ui, tr("trace.threshold"));
                crate::a11y::add_drag(
                    ui,
                    egui::DragValue::new(&mut self.editor.trace.threshold)
                        .range(1..=255)
                        .speed(1.0),
                );
                ui.end_row();
            });
        if self.editor.trace.computing() {
            crate::a11y::label(ui, tr("trace.computing"));
            if button(ui, &tr("trace.cancel"), ButtonKind::Secondary).clicked() {
                self.editor.trace.cancel_edges();
            }
        } else if button(ui, &tr("trace.compute"), ButtonKind::Secondary).clicked() {
            let bg = self
                .editor
                .project()
                .sheet(self.editor.sheet)
                .ok()
                .and_then(|s| s.background.clone());
            match (bg, self.project_dir()) {
                (Some(bg), Some(dir)) => self.editor.trace.start_edges(dir, bg),
                _ => self.editor.trace.error = Some(tr("bg.err.unsaved")),
            }
        }
        if let Some(c) = &self.editor.trace.edges {
            let n = c.map.count_edges(f32::from(self.editor.trace.threshold));
            crate::a11y::label(ui, tr_args("trace.edges.ready", &[("n", &n.to_string())]));
        }
        if let Some(e) = &self.editor.trace.error {
            crate::a11y::label(ui, RichText::new(e).color(danger));
        }
        ui.separator();
        let n = self.editor.trace.points.len();
        crate::a11y::label(ui, tr_args("trace.points", &[("n", &n.to_string())]));
        egui::Grid::new("trace_fit").num_columns(2).show(ui, |ui| {
            crate::a11y::label(ui, tr("trace.tolerance"));
            crate::a11y::add_text(
                ui,
                egui::TextEdit::singleline(&mut self.editor.trace.tolerance_text),
            );
            ui.end_row();
            crate::a11y::label(ui, tr("trace.corner"));
            crate::a11y::add_text(
                ui,
                egui::TextEdit::singleline(&mut self.editor.trace.corner_text),
            );
            ui.end_row();
        });
        if n >= 2 {
            match self.editor.trace_fit() {
                Ok(fit) => {
                    crate::a11y::label(
                        ui,
                        tr_len(
                            "trace.fit.summary",
                            &[
                                ("n", &fit.curves.len().to_string()),
                                ("err", &pf_core::units::len_number(fit.max_error_mm, 3)),
                            ],
                        ),
                    );
                }
                Err(e) => {
                    crate::a11y::label(ui, RichText::new(e).color(danger));
                }
            }
        }
        ui.horizontal(|ui| {
            if button(ui, &tr("trace.apply_smooth"), ButtonKind::Primary).clicked() {
                let _ = self.editor.trace_apply(true);
            }
            if button(ui, &tr("trace.apply_segments"), ButtonKind::Secondary).clicked() {
                let _ = self.editor.trace_apply(false);
            }
        });
        ui.horizontal(|ui| {
            if button(ui, &tr("trace.remove_last"), ButtonKind::Secondary).clicked() {
                self.editor.trace_remove_last();
            }
            if button(ui, &tr("trace.clear"), ButtonKind::Secondary).clicked() {
                self.editor.trace_clear();
            }
        });
    }

    /// Points posés (anneaux numérotés par la forme, pas par la couleur seule) et aperçu du lissage.
    pub(crate) fn paint_trace(&self, painter: &egui::Painter, xf: &Xf) {
        let pts = &self.editor.trace.points;
        if pts.is_empty() {
            return;
        }
        let raw = stroke(1.0, pal().preview);
        for w in pts.windows(2) {
            painter.extend(Shape::dashed_line(&[xf.s(w[0]), xf.s(w[1])], raw, 4.0, 4.0));
        }
        for p in pts {
            painter.circle_stroke(xf.s(*p), 4.0, stroke(1.5, pal().preview));
        }
        if pts.len() >= 2 {
            if let Ok(fit) = self.editor.trace_fit() {
                let line = stroke(2.0, pal().select);
                for c in &fit.curves {
                    let poly: Vec<egui::Pos2> = (0..=PREVIEW_STEPS)
                        .map(|i| xf.s(c.point_at(i as f64 / PREVIEW_STEPS as f64)))
                        .collect();
                    painter.add(Shape::line(poly, line));
                }
            }
        }
    }
}
