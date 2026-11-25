//! US-03.2 — constructions maintenues, dans l'inspecteur d'un segment.
//!
//! Un segment créé par longueur / angle, perpendiculaire ou parallèle peut être **maintenu** :
//! la case « Conserver la construction » envoie `Command::SetConstructionKept`. L'état se lit par
//! une icône **et** un texte (« Construction maintenue » / « Construction non maintenue »),
//! jamais par la couleur seule ; il se distingue de « ƒ Maintenu » des formules. Aucun mot de
//! « contrainte » ni de « longueur imposée » : réservés à US-03.3 (solveur).

use crate::app::App;
use crate::palette::pal;
use eframe::egui::{self, RichText};
use pf_core::{Command, EntityId};
use pf_i18n::tr;

/// Icône « maillons » de l'état « construction maintenue », dessinée (la police n'a pas de glyphe
/// de chaîne fiable) ; toujours accompagnée du texte.
fn link_icon(ui: &mut egui::Ui) {
    let color = ui.visuals().strong_text_color();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(22.0, 16.0), egui::Sense::hover());
    let stroke = egui::Stroke::new(1.8_f32, color);
    let size = egui::vec2(12.0, 9.0);
    for dx in [1.0, 9.0] {
        let r = egui::Rect::from_min_size(rect.min + egui::vec2(dx, 3.5), size);
        ui.painter()
            .rect_stroke(r, 4.5, stroke, egui::StrokeKind::Middle);
    }
}

impl App {
    /// État, case et bouton de liaison d'un segment qui a une construction mémorisée.
    pub(crate) fn kept_controls(&mut self, ui: &mut egui::Ui, segment: EntityId, kept: bool) {
        let muted = pal().muted;
        let sheet = self.editor.sheet;
        let mut unlink = false;
        if kept {
            ui.horizontal(|ui| {
                link_icon(ui);
                crate::a11y::label(
                    ui,
                    RichText::new(tr("inspector.kept.state"))
                        .strong()
                        .size(15.0),
                );
                if ui
                    .button(tr("inspector.kept.unlink"))
                    .on_hover_text(tr("inspector.kept.unlink.hover"))
                    .clicked()
                {
                    unlink = true;
                }
            });
            crate::a11y::label(
                ui,
                RichText::new(tr("inspector.kept.note"))
                    .small()
                    .color(muted),
            );
        } else {
            crate::a11y::label(ui, RichText::new(tr("inspector.kept.state_off")).strong());
        }
        let mut value = kept;
        let changed = ui
            .checkbox(&mut value, tr("inspector.kept.checkbox"))
            .on_hover_text(tr("inspector.kept.checkbox.hover"))
            .changed();
        if unlink || changed {
            self.editor.exec(Command::SetConstructionKept {
                sheet,
                segment,
                kept: if unlink { false } else { value },
            });
        }
    }

    /// Mention d'un point libre dont la position est écrite par une construction maintenue.
    pub(crate) fn kept_point_note(&self, ui: &mut egui::Ui, point: pf_core::PointId) {
        let sheet = self.editor.sheet();
        let Some(segment) = sheet.kept_segment_of_point(point) else {
            return;
        };
        let name = crate::text::entity_name(sheet, segment);
        ui.horizontal(|ui| {
            link_icon(ui);
            crate::a11y::label(
                ui,
                RichText::new(pf_i18n::tr_args("inspector.kept.point", &[("name", &name)]))
                    .small()
                    .color(pal().muted),
            );
        });
    }
}
