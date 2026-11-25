//! Section « Ancrage » de l'inspecteur d'un cran (US-06.2). Elle ne calcule rien : la position
//! effective vient de `Sheet::notch_distance_mm`, et chaque changement est une commande
//! (`SetNotchAnchor`, une étape annulable, refus sans trace).
//!
//! Limites : l'outil Cran pose toujours un cran à distance fixe ; le mode d'ancrage se choisit
//! ensuite dans l'inspecteur (une étape de plus dans l'historique). Les points et le tracé
//! source se choisissent dans des listes, pas au clic.

use crate::a11y::ComboExt;
use crate::app::App;
use crate::palette::pal;
use crate::text::{entity_name, Localized};
use eframe::egui::{self, RichText};
use pf_core::input::format_mm;
use pf_core::{Command, EntityId, NotchAnchor, NotchId, PointId};
use pf_i18n::tr;

/// Ancrage proposé quand on change de mode : la position actuelle est conservée quand c'est
/// possible (proportion), sinon le premier objet de la liste (l'utilisateur le choisit ensuite).
fn default_anchor(
    mode: &str,
    at: Option<f64>,
    len: f64,
    points: &[(PointId, String)],
    paths: &[(EntityId, String)],
) -> Option<Option<NotchAnchor>> {
    match mode {
        "fixed" => Some(None),
        "midpoint" => Some(Some(NotchAnchor::Midpoint {})),
        "ratio" => {
            let ratio = if len > 0.0 {
                (at.unwrap_or(0.0) / len).clamp(0.0, 1.0)
            } else {
                0.0
            };
            Some(Some(NotchAnchor::Ratio { ratio }))
        }
        "point" => points
            .first()
            .map(|(point, _)| Some(NotchAnchor::Point { point: *point })),
        "carried" => match (paths.first(), points.first()) {
            (Some((entity, _)), Some((point, _))) => Some(Some(NotchAnchor::Carried {
                entity: *entity,
                point: *point,
                from_end: false,
            })),
            _ => None,
        },
        _ => None,
    }
}

impl App {
    /// Section « Ancrage » : mode (distance fixe, point, milieu, proportion, report), réglages
    /// du mode, position effective calculée.
    pub(crate) fn notch_anchor_section(&mut self, ui: &mut egui::Ui, id: NotchId) {
        let sheet_id = self.editor.sheet;
        let s = self.editor.sheet();
        let Ok(notch) = s.notch(id).cloned() else {
            return;
        };
        let points: Vec<(PointId, String)> =
            s.points.iter().map(|p| (p.id, p.label.clone())).collect();
        let paths: Vec<(EntityId, String)> = s
            .entities
            .iter()
            .map(|e| (e.id, entity_name(s, e.id)))
            .collect();
        let len = s.path_length_mm(notch.entity).unwrap_or(0.0);
        let at = s.notch_distance_mm(&notch).ok();
        let label_of = |p: PointId| {
            points
                .iter()
                .find(|(q, _)| *q == p)
                .map_or_else(|| "?".to_string(), |(_, l)| l.clone())
        };
        let path_of = |e: EntityId| {
            paths
                .iter()
                .find(|(q, _)| *q == e)
                .map_or_else(|| "?".to_string(), |(_, l)| l.clone())
        };

        crate::panels::section(ui, &tr("inspector.notch.anchor.section"));
        let mode = notch.anchor.map_or("fixed", |a| a.key());
        let mut wanted: Option<Option<NotchAnchor>> = None;
        ui.horizontal_wrapped(|ui| {
            let modes = [
                ("fixed", tr("inspector.notch.anchor.fixed")),
                ("point", tr("inspector.notch.anchor.point")),
                ("midpoint", tr("inspector.notch.anchor.midpoint")),
                ("ratio", tr("inspector.notch.anchor.ratio")),
                ("carried", tr("inspector.notch.anchor.carried")),
            ];
            for (key, text) in modes {
                let usable = match key {
                    "point" => !points.is_empty(),
                    "carried" => !points.is_empty() && !paths.is_empty(),
                    _ => true,
                };
                let r = ui.add_enabled(usable, egui::Button::selectable(mode == key, text));
                if r.clicked() && mode != key {
                    wanted = default_anchor(key, at, len, &points, &paths);
                }
            }
        });

        let mut edited = None;
        let mut grouped = false;
        match notch.anchor {
            None => {
                crate::a11y::label(
                    ui,
                    RichText::new(tr("inspector.notch.anchor.note.fixed"))
                        .small()
                        .color(pal().muted),
                );
            }
            Some(NotchAnchor::Midpoint {}) => {
                crate::a11y::label(
                    ui,
                    RichText::new(tr("inspector.notch.anchor.note.midpoint"))
                        .small()
                        .color(pal().muted),
                );
            }
            Some(NotchAnchor::Point { point }) => {
                let mut choice = point;
                ui.horizontal(|ui| {
                    crate::a11y::label(ui, tr("inspector.notch.anchor.point_field"));
                    egui::ComboBox::from_id_salt(("notch-anchor-point", id.0))
                        .selected_text(label_of(point))
                        .show_ui_named(ui, |ui| {
                            for (p, l) in &points {
                                ui.selectable_value(&mut choice, *p, l);
                            }
                        });
                });
                if choice != point {
                    edited = Some(NotchAnchor::Point { point: choice });
                }
                crate::a11y::label(
                    ui,
                    RichText::new(tr("inspector.notch.anchor.note.point"))
                        .small()
                        .color(pal().muted),
                );
            }
            Some(NotchAnchor::Ratio { ratio }) => {
                let mut percent = ratio * 100.0;
                let mut stopped = false;
                ui.horizontal(|ui| {
                    crate::a11y::label(ui, tr("inspector.notch.anchor.ratio_field"));
                    let r = crate::a11y::add_drag(
                        ui,
                        egui::DragValue::new(&mut percent)
                            .localized()
                            .speed(0.2)
                            .suffix(" %")
                            .max_decimals(2)
                            .range(0.0..=100.0),
                    );
                    if r.changed() {
                        edited = Some(NotchAnchor::Ratio {
                            ratio: (percent / 100.0).clamp(0.0, 1.0),
                        });
                        // Un glisser = une seule étape d'historique.
                        grouped = true;
                    }
                    stopped = r.drag_stopped() || r.lost_focus();
                });
                if stopped {
                    self.editor.doc.end_group();
                }
                crate::a11y::label(
                    ui,
                    RichText::new(tr("inspector.notch.anchor.note.ratio"))
                        .small()
                        .color(pal().muted),
                );
            }
            Some(NotchAnchor::Carried {
                entity,
                point,
                from_end,
            }) => {
                let (mut e_choice, mut p_choice, mut end) = (entity, point, from_end);
                ui.horizontal(|ui| {
                    crate::a11y::label(ui, tr("inspector.notch.anchor.source_path"));
                    egui::ComboBox::from_id_salt(("notch-anchor-source", id.0))
                        .selected_text(path_of(entity))
                        .show_ui_named(ui, |ui| {
                            for (e, l) in &paths {
                                ui.selectable_value(&mut e_choice, *e, l);
                            }
                        });
                });
                ui.horizontal(|ui| {
                    crate::a11y::label(ui, tr("inspector.notch.anchor.point_field"));
                    egui::ComboBox::from_id_salt(("notch-anchor-carried-point", id.0))
                        .selected_text(label_of(point))
                        .show_ui_named(ui, |ui| {
                            for (p, l) in &points {
                                ui.selectable_value(&mut p_choice, *p, l);
                            }
                        });
                });
                ui.checkbox(&mut end, tr("inspector.notch.anchor.from_end"));
                if (e_choice, p_choice, end) != (entity, point, from_end) {
                    edited = Some(NotchAnchor::Carried {
                        entity: e_choice,
                        point: p_choice,
                        from_end: end,
                    });
                }
                crate::a11y::label(
                    ui,
                    RichText::new(tr("inspector.notch.anchor.note.carried"))
                        .small()
                        .color(pal().muted),
                );
            }
        }
        if let Some(a) = edited {
            wanted = Some(Some(a));
        }
        if notch.anchor.is_some() {
            match at {
                Some(d) => crate::panels::row(ui, &tr("inspector.notch.position"), &format_mm(d)),
                None => crate::panels::row(ui, &tr("inspector.notch.position"), "—"),
            }
        }
        if let Some(anchor) = wanted {
            let command = Command::SetNotchAnchor {
                sheet: sheet_id,
                notch: id,
                anchor,
            };
            if grouped {
                self.editor.doc.begin_group();
                if let Err(e) = self.editor.doc.execute(command) {
                    self.editor.error(crate::text::core_error(&e));
                }
            } else {
                self.editor.exec(command);
            }
        }
    }
}
