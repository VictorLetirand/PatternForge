//! Fenêtre « Déplier une pièce sur sa pliure » (US-04.7) : une pièce, un segment de son
//! contour pour pliure, un nom facultatif. Elle ne calcule rien : l'aperçu (mesures de la
//! moitié et de la pièce entière, contour dépliée, raison d'un refus éventuel) vient de
//! `Project::unfold_preview`, et le dépliage est la commande `Command::UnfoldPiece` (une
//! seule étape annulable, refus sans trace, résultat lié à la pièce source).
//!
//! Limites : la pliure se choisit dans une liste (pas au clic), l'aperçu est un petit
//! dessin de la fenêtre (pas une fenêtre de feuille), et seul un segment du contour peut
//! servir de pliure.

use crate::a11y::ComboExt;
use crate::app::App;
use crate::canvas::{label, stroke};
use crate::palette::pal;
use crate::text::entity_name;
use eframe::egui::{self, Align2, Pos2, RichText, Sense, Shape, Vec2};
use pf_core::input::{format_mm, format_number};
use pf_core::{Command, EntityId, PieceId, Project};
use pf_geometry::{Bbox, PointMm};
use pf_i18n::{tr, tr_args};

/// Choix en cours dans la fenêtre (état d'interface, jamais enregistré).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct UnfoldDialog {
    pub piece: Option<PieceId>,
    pub fold: Option<EntityId>,
    pub name: String,
}

/// Pièces proposées : celles qui ont un contour et ne sont pas dérivées d'une autre.
fn candidates(project: &Project) -> Vec<(PieceId, String)> {
    project
        .pieces
        .iter()
        .filter(|p| !p.outline.is_empty() && p.derived.is_none())
        .map(|p| (p.id, p.name.clone()))
        .collect()
}

/// Segments du contour d'une pièce, libellés avec leur longueur (pliure d'abord).
fn folds_of(project: &Project, piece: PieceId) -> Vec<(EntityId, String)> {
    let Ok(sheet) = project.piece_sheet(piece) else {
        return Vec::new();
    };
    project
        .unfold_candidates(piece)
        .into_iter()
        .map(|c| {
            (
                c.entity,
                format!(
                    "{} — {}",
                    entity_name(sheet, c.entity),
                    format_mm(c.length_mm)
                ),
            )
        })
        .collect()
}

impl App {
    /// Ouvre la fenêtre de dépliage ; `piece` pré-remplit la pièce et sa première pliure.
    pub fn open_unfold(&mut self, piece: Option<PieceId>) {
        let mut dialog = UnfoldDialog {
            piece,
            ..UnfoldDialog::default()
        };
        if let Some(p) = piece {
            dialog.fold = folds_of(self.editor.project(), p).first().map(|f| f.0);
        }
        self.unfold = Some(dialog);
    }

    pub fn unfold_window(&mut self, ctx: &egui::Context) {
        let Some(mut dialog) = self.unfold.take() else {
            return;
        };
        let pieces = candidates(self.editor.project());
        // Une pièce disparue (annuler, supprimer) libère le choix.
        if dialog
            .piece
            .is_some_and(|p| !pieces.iter().any(|c| c.0 == p))
        {
            dialog.piece = None;
            dialog.fold = None;
        }
        let (mut open, mut apply) = (true, false);
        let slot = crate::dock::slot(ctx, &self.dock, crate::dock::ToolWin::Unfold);
        let width = self.dock.content_width(crate::dock::ToolWin::Unfold);
        egui::Window::new(tr("unfoldpiece.title"))
            .id(egui::Id::new("unfold_window"))
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
                        crate::a11y::label(
                            ui,
                            RichText::new(tr("unfoldpiece.intro"))
                                .small()
                                .color(pal().muted),
                        );
                        egui::Grid::new("unfold-grid")
                            .num_columns(2)
                            .show(ui, |ui| {
                                crate::a11y::label(ui, tr("unfoldpiece.piece"));
                                let current = dialog
                                    .piece
                                    .and_then(|p| pieces.iter().find(|c| c.0 == p))
                                    .map(|c| c.1.clone())
                                    .unwrap_or_else(|| tr("unfoldpiece.choose"));
                                let before = dialog.piece;
                                egui::ComboBox::from_id_salt("unfold-piece")
                                    .selected_text(current)
                                    .show_ui_named(ui, |ui| {
                                        for (id, name) in &pieces {
                                            ui.selectable_value(&mut dialog.piece, Some(*id), name);
                                        }
                                    });
                                if dialog.piece != before {
                                    dialog.fold = dialog.piece.and_then(|p| {
                                        folds_of(self.editor.project(), p).first().map(|f| f.0)
                                    });
                                }
                                ui.end_row();
                                crate::a11y::label(ui, tr("unfoldpiece.fold"));
                                let folds = dialog
                                    .piece
                                    .map(|p| folds_of(self.editor.project(), p))
                                    .unwrap_or_default();
                                let text = dialog
                                    .fold
                                    .and_then(|e| folds.iter().find(|x| x.0 == e))
                                    .map(|x| x.1.clone())
                                    .unwrap_or_else(|| tr("unfoldpiece.choose"));
                                egui::ComboBox::from_id_salt("unfold-fold")
                                    .selected_text(text)
                                    .show_ui_named(ui, |ui| {
                                        for (id, name) in &folds {
                                            ui.selectable_value(&mut dialog.fold, Some(*id), name);
                                        }
                                    });
                                ui.end_row();
                            });
                        // Nom en pleine largeur, sous la grille : l'indication par défaut n'est pas rognée.
                        crate::a11y::label(ui, tr("unfoldpiece.name"));
                        crate::a11y::add_text(
                            ui,
                            egui::TextEdit::singleline(&mut dialog.name)
                                .id_salt("unfold-name")
                                .hint_text(tr("unfoldpiece.name.hint"))
                                .desired_width(ui.available_width()),
                        );
                        ui.separator();
                        apply = self.unfold_preview_ui(ui, &dialog);
                    });
            });
        if apply {
            if let (Some(piece), Some(fold)) = (dialog.piece, dialog.fold) {
                let name = match dialog.name.trim() {
                    "" => None,
                    n => Some(n.to_string()),
                };
                if let Some(out) = self.editor.exec(Command::UnfoldPiece { piece, fold, name }) {
                    let describe = out.pieces.first().and_then(|id| {
                        let p = self.editor.project().piece(*id).ok()?;
                        let area = self.editor.project().piece_contour(*id).ok()?.area_mm2;
                        let sheet = self.editor.project().piece_sheet(*id).ok()?.name.clone();
                        Some((p.name.clone(), format_number(area / 100.0, 2), sheet))
                    });
                    let (name, area, sheet) = describe.unwrap_or_default();
                    let mut text = tr_args(
                        "status.unfold_piece.done",
                        &[("name", &name), ("area", &area), ("sheet", &sheet)],
                    );
                    if !out.notes.is_empty() {
                        text.push(' ');
                        text.push_str(&tr_args("note.core", &[("text", &out.notes.join(" "))]));
                    }
                    self.editor.info(text);
                    self.editor.sync();
                    return;
                }
            }
        }
        if open {
            self.unfold = Some(dialog);
        }
    }

    /// Aperçu, mesures et bouton ; renvoie vrai quand « Déplier » est cliqué.
    fn unfold_preview_ui(&self, ui: &mut egui::Ui, dialog: &UnfoldDialog) -> bool {
        let (Some(piece), Some(fold)) = (dialog.piece, dialog.fold) else {
            crate::a11y::label(ui, RichText::new(tr("unfoldpiece.pick")).color(pal().muted));
            return false;
        };
        let project = self.editor.project();
        let name = match dialog.name.trim() {
            "" => None,
            n => Some(n.to_string()),
        };
        let preview = match project.unfold_preview(piece, fold, name) {
            Ok(p) => p,
            Err(e) => {
                crate::a11y::label(
                    ui,
                    RichText::new(tr_args(
                        "unfoldpiece.refused",
                        &[("reason", &crate::text::core_error(&e))],
                    ))
                    .color(pal().error),
                );
                ui.add_enabled(false, egui::Button::new(tr("unfoldpiece.apply")));
                return false;
            }
        };
        // Petit dessin : moitié (trait plein), pièce dépliée (pointillés), pliure épaisse.
        let size = Vec2::new(360.0, 220.0);
        let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 4.0, pal().halo);
        let mut bb: Option<Bbox> = None;
        for p in preview.outline.iter().chain(&preview.half_outline) {
            match bb.as_mut() {
                Some(b) => b.include(*p),
                None => bb = Some(Bbox::from_point(*p)),
            }
        }
        if let Some(bb) = bb {
            let pad = 14.0_f32;
            let (w, h) = (bb.width().max(1e-6), bb.height().max(1e-6));
            let k = (f64::from(rect.width() - 2.0 * pad) / w)
                .min(f64::from(rect.height() - 2.0 * pad) / h);
            let to_screen = |p: PointMm| {
                Pos2::new(
                    rect.left() + pad + ((p.x - bb.min.x) * k) as f32,
                    rect.bottom() - pad - ((p.y - bb.min.y) * k) as f32,
                )
            };
            let line = |pts: &[PointMm], close: bool| -> Vec<Pos2> {
                let mut v: Vec<Pos2> = pts.iter().map(|p| to_screen(*p)).collect();
                if close {
                    if let Some(f) = v.first().copied() {
                        v.push(f);
                    }
                }
                v
            };
            painter.extend(Shape::dashed_line(
                &line(&preview.outline, true),
                stroke(2.0, pal().preview),
                6.0,
                4.0,
            ));
            painter.add(Shape::line(
                line(&preview.half_outline, true),
                stroke(2.0, pal().ink),
            ));
            painter.add(Shape::line(
                vec![to_screen(preview.fold_start), to_screen(preview.fold_end)],
                stroke(5.0, pal().select),
            ));
            label(
                &painter,
                to_screen(PointMm::new(
                    0.5 * (preview.fold_start.x + preview.fold_end.x),
                    0.5 * (preview.fold_start.y + preview.fold_end.y),
                )),
                Align2::CENTER_CENTER,
                &tr("unfoldpiece.fold_label"),
                pal().select,
            );
        }
        crate::a11y::label(
            ui,
            RichText::new(tr("unfoldpiece.legend"))
                .small()
                .color(pal().muted),
        );
        crate::a11y::label(
            ui,
            tr_args(
                "unfoldpiece.half",
                &[
                    ("area", &format_number(preview.half_area_mm2 / 100.0, 2)),
                    ("perimeter", &format_mm(preview.half_perimeter_mm)),
                    ("fold", &format_mm(preview.fold_length_mm)),
                ],
            ),
        );
        crate::a11y::label(
            ui,
            tr_args(
                "unfoldpiece.result",
                &[
                    ("name", &preview.name),
                    ("area", &format_number(preview.area_mm2 / 100.0, 2)),
                    ("perimeter", &format_mm(preview.perimeter_mm)),
                ],
            ),
        );
        crate::a11y::label(
            ui,
            RichText::new(tr("unfoldpiece.linked"))
                .small()
                .color(pal().muted),
        );
        // Remarques du cœur : repliées sous « n remarques » (texte intégral au dépliage).
        if !preview.notes.is_empty() {
            let title = tr_args(
                "unfoldpiece.notes",
                &[("n", &preview.notes.len().to_string())],
            );
            egui::CollapsingHeader::new(
                RichText::new(title)
                    .small()
                    .strong()
                    .color(ui.visuals().warn_fg_color),
            )
            .id_salt("unfold-notes")
            .default_open(false)
            .show(ui, |ui| {
                for note in &preview.notes {
                    crate::a11y::label(
                        ui,
                        RichText::new(tr_args("note.core", &[("text", note)]))
                            .small()
                            .color(ui.visuals().warn_fg_color),
                    );
                }
            });
        }
        ui.button(tr("unfoldpiece.apply")).clicked()
    }
}
