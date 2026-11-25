//! Fenêtre « Assembler deux pièces » (US-04.6) : deux pièces, un bord de chacune, une
//! tolérance. Elle ne calcule rien : l'aperçu (longueurs, écart de longueur et de forme,
//! contour déplacé de la seconde pièce, aire du résultat, raison d'un refus éventuel) vient
//! de `Project::join_preview`, et l'assemblage est la commande `Command::JoinPieces`
//! (une seule étape annulable, refus sans trace).
//!
//! Limites : les bords se choisissent dans une liste (pas au clic), l'aperçu est un petit
//! dessin de la fenêtre (pas une fenêtre de feuille), la seconde pièce est déplacée dans le
//! repère de la première (jamais de symétrie).

use crate::a11y::ComboExt;
use crate::app::App;
use crate::canvas::{label, stroke};
use crate::palette::pal;
use crate::text::entity_name;
use eframe::egui::{self, Align2, Pos2, RichText, Sense, Shape, Vec2};
use pf_core::contour::{contour_polyline, ContourEdge};
use pf_core::input::{format_mm, format_number};
use pf_core::{Command, EntityId, PieceId, Project, DEFAULT_JOIN_TOLERANCE_MM};
use pf_geometry::{Bbox, PointMm};
use pf_i18n::{tr, tr_args};
use pf_ui::components::{NumericField, Unit};

/// Choix en cours dans la fenêtre (état d'interface, jamais enregistré).
#[derive(Debug, Clone, PartialEq)]
pub struct JoinDialog {
    pub first: Option<PieceId>,
    pub first_edge: Option<EntityId>,
    pub second: Option<PieceId>,
    pub second_edge: Option<EntityId>,
    pub tolerance_mm: f64,
}

impl JoinDialog {
    pub fn new(first: Option<PieceId>) -> JoinDialog {
        JoinDialog {
            first,
            first_edge: None,
            second: None,
            second_edge: None,
            tolerance_mm: DEFAULT_JOIN_TOLERANCE_MM,
        }
    }
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

/// Bords d'une pièce (tracés de son contour) avec un libellé et leur longueur.
fn edges_of(project: &Project, piece: PieceId) -> Vec<(EntityId, String)> {
    let (Ok(p), Ok(sheet)) = (project.piece(piece), project.piece_sheet(piece)) else {
        return Vec::new();
    };
    p.outline
        .iter()
        .map(|e| {
            let len = sheet
                .path_length_mm(e.entity)
                .map(format_mm)
                .unwrap_or_default();
            (
                e.entity,
                format!("{} — {len}", entity_name(sheet, e.entity)),
            )
        })
        .collect()
}

fn outline_polyline(project: &Project, piece: PieceId) -> Vec<PointMm> {
    let (Ok(p), Ok(sheet)) = (project.piece(piece), project.piece_sheet(piece)) else {
        return Vec::new();
    };
    let edges: Vec<ContourEdge> = p
        .outline
        .iter()
        .map(|e| ContourEdge {
            entity: e.entity,
            reversed: e.reversed,
        })
        .collect();
    contour_polyline(sheet, &edges, 48).unwrap_or_default()
}

fn edge_polyline(project: &Project, piece: PieceId, edge: EntityId) -> Vec<PointMm> {
    project
        .piece_sheet(piece)
        .ok()
        .and_then(|s| s.shape(edge).ok())
        .map(|s| s.sample(48))
        .unwrap_or_default()
}

impl App {
    /// Ouvre la fenêtre d'assemblage ; `first` pré-remplit la première pièce.
    pub fn open_assemble(&mut self, first: Option<PieceId>) {
        let mut dialog = JoinDialog::new(first);
        if let Some(p) = first {
            dialog.first_edge = edges_of(self.editor.project(), p).first().map(|e| e.0);
        }
        self.assemble = Some(dialog);
    }

    pub fn assemble_window(&mut self, ctx: &egui::Context) {
        let Some(mut dialog) = self.assemble.take() else {
            return;
        };
        let pieces = candidates(self.editor.project());
        // Une pièce disparue (annuler, supprimer) libère le choix.
        for (piece, edge) in [
            (&mut dialog.first, &mut dialog.first_edge),
            (&mut dialog.second, &mut dialog.second_edge),
        ] {
            if piece.is_some_and(|p| !pieces.iter().any(|c| c.0 == p)) {
                *piece = None;
                *edge = None;
            }
        }
        let (mut open, mut apply) = (true, false);
        let slot = crate::dock::slot(ctx, &self.dock, crate::dock::ToolWin::Assemble);
        let width = self.dock.content_width(crate::dock::ToolWin::Assemble);
        egui::Window::new(tr("assemble.title"))
            .id(egui::Id::new("assemble_window"))
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
                            RichText::new(tr("assemble.intro"))
                                .small()
                                .color(pal().muted),
                        );
                        egui::Grid::new("assemble-grid")
                            .num_columns(2)
                            .show(ui, |ui| {
                                for (n, (piece, edge)) in [
                                    (&mut dialog.first, &mut dialog.first_edge),
                                    (&mut dialog.second, &mut dialog.second_edge),
                                ]
                                .into_iter()
                                .enumerate()
                                {
                                    crate::a11y::label(
                                        ui,
                                        tr(if n == 0 {
                                            "assemble.first"
                                        } else {
                                            "assemble.second"
                                        }),
                                    );
                                    let current = piece
                                        .and_then(|p| pieces.iter().find(|c| c.0 == p))
                                        .map(|c| c.1.clone())
                                        .unwrap_or_else(|| tr("assemble.choose"));
                                    let before = *piece;
                                    egui::ComboBox::from_id_salt(("assemble-piece", n))
                                        .selected_text(current)
                                        .show_ui_named(ui, |ui| {
                                            for (id, name) in &pieces {
                                                ui.selectable_value(piece, Some(*id), name);
                                            }
                                        });
                                    if *piece != before {
                                        *edge = piece.and_then(|p| {
                                            edges_of(self.editor.project(), p).first().map(|e| e.0)
                                        });
                                    }
                                    ui.end_row();
                                    crate::a11y::label(ui, tr("assemble.edge"));
                                    let edges = piece
                                        .map(|p| edges_of(self.editor.project(), p))
                                        .unwrap_or_default();
                                    let text = edge
                                        .and_then(|e| edges.iter().find(|x| x.0 == e))
                                        .map(|x| x.1.clone())
                                        .unwrap_or_else(|| tr("assemble.choose"));
                                    egui::ComboBox::from_id_salt(("assemble-edge", n))
                                        .selected_text(text)
                                        .show_ui_named(ui, |ui| {
                                            for (id, name) in &edges {
                                                ui.selectable_value(edge, Some(*id), name);
                                            }
                                        });
                                    ui.end_row();
                                }
                                crate::a11y::label(ui, tr("assemble.tolerance"));
                                NumericField::new(&mut dialog.tolerance_mm, Unit::Mm)
                                    .id_salt("assemble-tol")
                                    .decimals(3)
                                    .show(ui)
                                    .response
                                    .on_hover_text(tr("assemble.tolerance.hover"));
                                ui.end_row();
                            });
                        ui.separator();
                        apply = self.assemble_preview(ui, &dialog);
                    });
            });
        if apply {
            if let (Some(f), Some(fe), Some(s), Some(se)) = (
                dialog.first,
                dialog.first_edge,
                dialog.second,
                dialog.second_edge,
            ) {
                let cmd = Command::JoinPieces {
                    first: f,
                    first_edge: fe,
                    second: s,
                    second_edge: se,
                    tolerance_mm: dialog.tolerance_mm,
                };
                if let Some(out) = self.editor.exec(cmd) {
                    let describe = out.pieces.first().and_then(|id| {
                        let p = self.editor.project().piece(*id).ok()?;
                        let area = self.editor.project().piece_contour(*id).ok()?.area_mm2;
                        let sheet = self.editor.project().piece_sheet(*id).ok()?.name.clone();
                        Some((p.name.clone(), format_number(area / 100.0, 2), sheet))
                    });
                    let (name, area, sheet) = describe.unwrap_or_default();
                    let mut text = tr_args(
                        "status.join.done",
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
            self.assemble = Some(dialog);
        }
    }

    /// Aperçu, écart et boutons ; renvoie vrai quand « Assembler » est cliqué.
    fn assemble_preview(&self, ui: &mut egui::Ui, dialog: &JoinDialog) -> bool {
        let (Some(f), Some(fe), Some(s), Some(se)) = (
            dialog.first,
            dialog.first_edge,
            dialog.second,
            dialog.second_edge,
        ) else {
            crate::a11y::label(ui, RichText::new(tr("assemble.pick")).color(pal().muted));
            return false;
        };
        let project = self.editor.project();
        let preview = match project.join_preview(f, fe, s, se, dialog.tolerance_mm) {
            Ok(p) => p,
            Err(e) => {
                crate::a11y::label(
                    ui,
                    RichText::new(crate::text::core_error(&e)).color(pal().error),
                );
                return false;
            }
        };
        // Petit dessin : pièce 1 (trait plein), pièce 2 déplacée (trait pointillé), jonction épaisse.
        let first = outline_polyline(project, f);
        let joint = edge_polyline(project, f, fe);
        let size = Vec2::new(360.0, 220.0);
        let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 4.0, pal().halo);
        let mut bb: Option<Bbox> = None;
        for p in first.iter().chain(&preview.moved_second) {
            match bb.as_mut() {
                Some(b) => b.include(*p),
                None => bb = Some(Bbox::from_point(*p)),
            }
        }
        if let Some(bb) = bb {
            let pad = 14.0_f32;
            let (w, h) = ((bb.width()).max(1e-6), (bb.height()).max(1e-6));
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
            painter.add(Shape::line(line(&first, true), stroke(2.0, pal().ink)));
            painter.extend(Shape::dashed_line(
                &line(&preview.moved_second, true),
                stroke(2.0, pal().preview),
                6.0,
                4.0,
            ));
            painter.add(Shape::line(line(&joint, false), stroke(5.0, pal().select)));
            let centre = |pts: &[PointMm]| {
                let n = pts.len().max(1) as f64;
                PointMm::new(
                    pts.iter().map(|p| p.x).sum::<f64>() / n,
                    pts.iter().map(|p| p.y).sum::<f64>() / n,
                )
            };
            label(
                &painter,
                to_screen(centre(&first)),
                Align2::CENTER_CENTER,
                "1",
                pal().ink,
            );
            label(
                &painter,
                to_screen(centre(&preview.moved_second)),
                Align2::CENTER_CENTER,
                "2",
                pal().preview,
            );
        }
        crate::a11y::label(
            ui,
            RichText::new(tr("assemble.legend"))
                .small()
                .color(pal().muted),
        );
        // Écart toujours affiché, en clair.
        crate::a11y::label(
            ui,
            tr_args(
                "assemble.lengths",
                &[
                    ("first", &format_mm(preview.first_length_mm)),
                    ("second", &format_mm(preview.second_length_mm)),
                ],
            ),
        );
        crate::a11y::label(
            ui,
            tr_args(
                "assemble.gap",
                &[
                    ("gap", &format_mm(preview.gap_mm)),
                    ("tolerance", &format_mm(preview.tolerance_mm)),
                    ("shape", &format_mm(preview.deviation_mm)),
                ],
            ),
        );
        match (&preview.refusal, &preview.result) {
            (Some(reason), _) => {
                crate::a11y::label(
                    ui,
                    RichText::new(tr_args("assemble.refused", &[("reason", reason)]))
                        .color(pal().error),
                );
            }
            (None, Some(r)) => {
                crate::a11y::label(
                    ui,
                    tr_args(
                        "assemble.result",
                        &[
                            ("area", &format_number(r.area_mm2 / 100.0, 2)),
                            ("perimeter", &format_mm(r.perimeter_mm)),
                        ],
                    ),
                );
            }
            (None, None) => {}
        }

        ui.add_enabled(
            preview.refusal.is_none(),
            egui::Button::new(tr("assemble.apply")),
        )
        .clicked()
    }
}
