//! Fenêtre « Transformer la sélection » : déplacer, pivoter, symétrie, avec
//! ou sans copie. Elle ne calcule rien : elle assemble une commande
//! `Transform` ou `Duplicate` du cœur, qui valide et applique de façon atomique.

use crate::app::App;
use crate::editor::Status;
use eframe::egui;
use pf_core::{Command, EntityId, PointId};
use pf_geometry::{PointMm, Transform};
use pf_i18n::{tr, tr_args};
use pf_ui::components::{button, ButtonKind, NumericField, Unit};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Translate,
    Rotate,
    Mirror,
}

#[derive(Debug, Clone)]
pub struct TransformDialog {
    pub kind: Kind,
    pub dx: f64,
    pub dy: f64,
    /// Centre de rotation, ou point de l'axe de symétrie.
    pub cx: f64,
    pub cy: f64,
    /// Angle de rotation, ou direction de l'axe de symétrie.
    pub angle: f64,
    pub copy: bool,
}

impl TransformDialog {
    pub fn transform(&self) -> Transform {
        let at = PointMm::new(self.cx, self.cy);
        match self.kind {
            Kind::Translate => Transform::Translate {
                dx_mm: self.dx,
                dy_mm: self.dy,
            },
            Kind::Rotate => Transform::Rotate {
                center: at,
                angle_deg: self.angle,
            },
            Kind::Mirror => Transform::Mirror {
                axis_point: at,
                axis_angle_deg: self.angle,
            },
        }
    }
}

impl App {
    /// Points et tracés visés : la sélection, plus les extrémités des tracés sélectionnés.
    fn selection_targets(&self) -> (Vec<PointId>, Vec<EntityId>) {
        let sheet = self.editor.sheet();
        let sel = &self.editor.selection;
        let mut points = sel.points.clone();
        for &e in &sel.entities {
            if let Ok(all) = sheet.entity_all_points(e) {
                points.extend(all);
            }
        }
        points.sort();
        points.dedup();
        (points, sel.entities.clone())
    }

    /// Ouvre la fenêtre, centrée par défaut sur le milieu de la sélection.
    pub fn open_transform(&mut self, copy: bool) {
        let (points, _) = self.selection_targets();
        let guide = {
            let sheet = self.editor.sheet();
            self.editor
                .selection
                .guides
                .iter()
                .find_map(|g| sheet.guide(*g).ok())
                .map(|g| (g.x_mm, g.y_mm, g.angle_deg))
        };
        let sheet = self.editor.sheet();
        let positions: Vec<PointMm> = points
            .iter()
            .filter_map(|p| sheet.position(*p).ok())
            .collect();
        let Some(first) = positions.first().copied() else {
            self.editor.error(tr("transform.error.select_first"));
            return;
        };
        let mut bb = pf_geometry::Bbox::from_point(first);
        for p in &positions {
            bb.include(*p);
        }
        let mut dialog = TransformDialog {
            kind: Kind::Translate,
            dx: 10.0,
            dy: 0.0,
            cx: (bb.min.x + bb.max.x) / 2.0,
            cy: (bb.min.y + bb.max.y) / 2.0,
            angle: 90.0,
            copy,
        };
        // Une ligne d'aide sélectionnée avec les objets : la symétrie propose son axe.
        if let Some((x, y, angle)) = guide {
            dialog.kind = Kind::Mirror;
            (dialog.cx, dialog.cy, dialog.angle) = (x, y, angle);
        }
        self.transform = Some(dialog);
    }

    fn apply_transform(&mut self, dialog: &TransformDialog) -> bool {
        let (points, entities) = self.selection_targets();
        let sheet = self.editor.sheet;
        let transform = dialog.transform();
        if dialog.copy {
            if entities.is_empty() {
                self.editor.error(tr("transform.error.select_paths"));
                return false;
            }
            let cmd = Command::Duplicate {
                sheet,
                entities,
                transform,
            };
            match self.editor.exec(cmd) {
                Some(out) => {
                    let n = out.entities.len();
                    self.editor.selection.clear();
                    self.editor.selection.entities = out.entities;
                    self.editor.status = Status {
                        text: tr_args("transform.status.copied", &[("count", &n.to_string())]),
                        error: false,
                    };
                    true
                }
                None => false,
            }
        } else {
            let cmd = Command::Transform {
                sheet,
                points,
                transform,
            };
            if self.editor.exec(cmd).is_some() {
                self.editor.status = Status {
                    text: tr("transform.status.done"),
                    error: false,
                };
                true
            } else {
                false
            }
        }
    }

    pub fn transform_window(&mut self, ctx: &egui::Context) {
        let Some(mut dialog) = self.transform.take() else {
            return;
        };
        let (mut open, mut apply, mut cancel) = (true, false, false);
        let slot = crate::dock::slot(ctx, &self.dock, crate::dock::ToolWin::Transform);
        let width = self.dock.content_width(crate::dock::ToolWin::Transform);
        egui::Window::new(tr("transform.title"))
            .id(egui::Id::new("transform_window"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .fixed_pos(slot.pos)
            .default_height(slot.max_height)
            .max_height(slot.max_height)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .max_height(slot.max_height)
                    .show(ui, |ui| {
                        ui.set_width(width);
                        ui.horizontal(|ui| {
                            ui.selectable_value(
                                &mut dialog.kind,
                                Kind::Translate,
                                tr("transform.kind.translate"),
                            );
                            ui.selectable_value(
                                &mut dialog.kind,
                                Kind::Rotate,
                                tr("transform.kind.rotate"),
                            );
                            ui.selectable_value(
                                &mut dialog.kind,
                                Kind::Mirror,
                                tr("transform.kind.mirror"),
                            );
                        });
                        ui.checkbox(&mut dialog.copy, tr("transform.copy"));
                        ui.separator();
                        egui::Grid::new("transform-grid")
                            .num_columns(2)
                            .show(ui, |ui| {
                                let mm =
                                    |ui: &mut egui::Ui, label: &str, v: &mut f64, salt: &str| {
                                        crate::a11y::label(ui, label);
                                        NumericField::new(v, Unit::Mm)
                                            .id_salt(salt)
                                            .decimals(3)
                                            .show(ui);
                                        ui.end_row();
                                    };
                                match dialog.kind {
                                    Kind::Translate => {
                                        mm(ui, &tr("transform.move_x"), &mut dialog.dx, "tf-dx");
                                        mm(ui, &tr("transform.move_y"), &mut dialog.dy, "tf-dy");
                                    }
                                    Kind::Rotate => {
                                        mm(ui, &tr("transform.center_x"), &mut dialog.cx, "tf-cx");
                                        mm(ui, &tr("transform.center_y"), &mut dialog.cy, "tf-cy");
                                    }
                                    Kind::Mirror => {
                                        mm(ui, &tr("transform.axis_x"), &mut dialog.cx, "tf-cx");
                                        mm(ui, &tr("transform.axis_y"), &mut dialog.cy, "tf-cy");
                                    }
                                }
                                if dialog.kind != Kind::Translate {
                                    crate::a11y::label(
                                        ui,
                                        if dialog.kind == Kind::Rotate {
                                            tr("transform.angle")
                                        } else {
                                            tr("transform.axis_direction")
                                        },
                                    );
                                    NumericField::new(&mut dialog.angle, Unit::Deg)
                                        .id_salt("tf-angle")
                                        .show(ui);
                                    ui.end_row();
                                }
                            });
                        if dialog.kind == Kind::Mirror {
                            crate::a11y::label(ui, tr("transform.axis_hint"));
                            // Axes proposés : lignes d'aide marquées « axe » ou sélectionnées.
                            let sheet = self.editor.sheet();
                            let selected = &self.editor.selection.guides;
                            let axes: Vec<(f64, f64, f64)> = sheet
                                .guides
                                .iter()
                                .filter(|g| g.axis || selected.contains(&g.id))
                                .map(|g| (g.x_mm, g.y_mm, g.angle_deg))
                                .collect();
                            if !axes.is_empty() {
                                crate::a11y::label(ui, tr("transform.axis_guides"));
                                for (x, y, angle) in axes {
                                    let text = tr_args(
                                        "transform.axis_guide_button",
                                        &[
                                            ("angle", &pf_core::input::format_deg(angle)),
                                            ("x", &pf_core::input::format_mm(x)),
                                            ("y", &pf_core::input::format_mm(y)),
                                        ],
                                    );
                                    if ui.button(text).clicked() {
                                        (dialog.cx, dialog.cy, dialog.angle) = (x, y, angle);
                                    }
                                }
                            }
                        }
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            apply =
                                button(ui, &tr("transform.apply"), ButtonKind::Primary).clicked();
                            cancel =
                                button(ui, &tr("dialog.close"), ButtonKind::Secondary).clicked();
                        });
                    });
            });
        if apply && self.apply_transform(&dialog) {
            return;
        }
        if open && !cancel {
            self.transform = Some(dialog);
        }
    }
}
