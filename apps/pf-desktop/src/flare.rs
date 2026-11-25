//! Fenêtre « Ampleur » (US-07.4) : choisir une pièce et son bord fixe (un côté droit), le nombre de
//! lignes, leurs positions et l'ouverture en haut de chacune. Elle ne calcule rien : l'aperçu
//! (longueur du bord fixe avant / après, gain du bord ouvert, aires, raison d'un refus) vient de
//! `Project::flare_preview`, la pose est `Command::AddPieceFlare` (une seule étape annulable, refus
//! sans trace). Les lignes posées viennent de `Project::piece_flares` ; chacune porte un libellé,
//! jamais la couleur seule.
//!
//! Limites : bord fixe choisi dans la liste des côtés droits (pas au clic) ; lignes perpendiculaires
//! au bord ; ouverture « bas » nulle ; pas de tracé des lignes avant la pose (seuls les pivots
//! sont marqués) ; les lignes ne sont pas exportées.

use crate::a11y::ComboExt;
use crate::app::App;
use crate::canvas::{label, polyline, stroke, Xf};
use crate::editor::Editor;
use crate::palette::pal;
use eframe::egui::{self, Align2, RichText};
use pf_core::input::{format_mm, format_number};
use pf_core::units::parse_length_display;
use pf_core::{
    flare_even_positions, Command, FlareLineSpec, FlarePreview, PieceId, Project, FLARE_MAX_LINES,
};
use pf_i18n::{tr, tr_args};

/// État de la fenêtre « Ampleur » (interface seulement, jamais enregistré).
#[derive(Debug, Clone, PartialEq)]
pub struct FlareUi {
    pub open: bool,
    pub piece: Option<PieceId>,
    /// Indice dans `Project::piece_flare_sides`.
    pub side: Option<usize>,
    pub count: String,
    /// Vide = lignes réparties à pas égal ; sinon `N` positions séparées par `;`.
    pub positions: String,
    /// Une valeur pour toutes les lignes, ou `N` valeurs séparées par `;`.
    pub opening: String,
}

impl Default for FlareUi {
    fn default() -> Self {
        FlareUi {
            open: false,
            piece: None,
            side: None,
            count: "3".into(),
            positions: String::new(),
            opening: "30".into(),
        }
    }
}

/// Pièces proposées : celles qui ont un contour et ne sont pas dérivées d'une autre.
fn candidates(project: &Project) -> Vec<(PieceId, String)> {
    project
        .pieces
        .iter()
        .filter(|p| p.derived.is_none() && !p.outline.is_empty())
        .map(|p| (p.id, p.name.clone()))
        .collect()
}

fn cm2(area_mm2: f64) -> String {
    format!("{} cm²", format_number(area_mm2 / 100.0, 2))
}

/// Liste de nombres séparés par `;`.
fn parse_list(text: &str) -> Result<Vec<f64>, String> {
    text.split(';')
        .map(|t| parse_length_display(t.trim()))
        .collect()
}

impl Editor {
    /// Commande décrite par la fenêtre.
    fn flare_command(&self) -> Result<Command, String> {
        let ui = &self.flare;
        let (Some(piece), Some(side)) = (ui.piece, ui.side) else {
            return Err(tr("status.flare.no_side"));
        };
        let sides = self.project().piece_flare_sides(piece);
        let Some(edge) = sides.get(side) else {
            return Err(tr("status.flare.no_side"));
        };
        let bad = || tr("status.flare.bad_values");
        let count: usize = ui.count.trim().parse().map_err(|_| bad())?;
        if count == 0 || count > FLARE_MAX_LINES {
            return Err(bad());
        }
        let positions = if ui.positions.trim().is_empty() {
            flare_even_positions(edge.length_mm, count)
        } else {
            parse_list(&ui.positions).map_err(|_| bad())?
        };
        let openings = parse_list(&ui.opening).map_err(|_| bad())?;
        let openings = match openings.len() {
            1 => vec![openings[0]; count],
            n if n == count => openings,
            _ => return Err(bad()),
        };
        if positions.len() != count {
            return Err(bad());
        }
        Ok(Command::AddPieceFlare {
            piece,
            fixed_from: edge.from,
            fixed_to: edge.to,
            lines: positions
                .into_iter()
                .zip(openings)
                .map(|(at_mm, opening_mm)| FlareLineSpec { at_mm, opening_mm })
                .collect(),
        })
    }

    /// Aperçu de la pose (lecture seule : même calcul que la commande).
    fn flare_preview(&self) -> Option<Result<FlarePreview, String>> {
        let ui = &self.flare;
        ui.piece?;
        let cmd = match self.flare_command() {
            Ok(c) => c,
            Err(e) => return Some(Err(e)),
        };
        let Command::AddPieceFlare {
            piece,
            fixed_from,
            fixed_to,
            lines,
        } = cmd
        else {
            return None;
        };
        Some(
            self.project()
                .flare_preview(piece, fixed_from, fixed_to, &lines)
                .map_err(|e| crate::text::core_error(&e)),
        )
    }

    /// Pose l'ampleur : une étape annulable ; l'affichage suit la pièce reconstruite.
    pub fn flare_apply(&mut self) {
        let cmd = match self.flare_command() {
            Ok(c) => c,
            Err(e) => {
                self.error(e);
                return;
            }
        };
        let Some(piece) = self.flare.piece else {
            return;
        };
        let name = self
            .project()
            .piece(piece)
            .map(|p| p.name.clone())
            .unwrap_or_default();
        let Some(out) = self.exec(cmd) else {
            return;
        };
        if let Some(sheet) = out.sheet {
            self.set_sheet(sheet);
        }
        self.info(tr_args(
            "status.flare.done",
            &[("count", &out.flares.len().to_string()), ("name", &name)],
        ));
    }

    /// Choisit le côté le plus long de la pièce comme bord fixe proposé.
    fn flare_default_side(&mut self) {
        let Some(piece) = self.flare.piece else {
            self.flare.side = None;
            return;
        };
        let sides = self.project().piece_flare_sides(piece);
        let mut best: Option<(usize, f64)> = None;
        for (i, s) in sides.iter().enumerate() {
            if best.is_none_or(|(_, l)| s.length_mm > l + 1e-9) {
                best = Some((i, s.length_mm));
            }
        }
        self.flare.side = best.map(|b| b.0);
    }
}

/// Projet de démonstration (captures `--etape ampleur`) : jupe de 200 x 100 mm, bord fixe = le bas.
pub fn demo_flare_project(
) -> Result<(pf_core::Project, PieceId, pf_core::SheetId), pf_core::CoreError> {
    use pf_core::{PieceSource, PointRef};
    use pf_geometry::PointMm;
    let mut p = pf_core::Project::new("Ampleur");
    let sheet = p.sheets[0].id;
    p.apply(Command::AddRectangle {
        sheet,
        corner: PointRef::New(PointMm::new(0.0, 0.0)),
        width_mm: 200.0,
        height_mm: 100.0,
    })?;
    let piece = p
        .apply(Command::ExtractPiece {
            sheet,
            source: PieceSource::Click {
                at: PointMm::new(100.0, 50.0),
            },
            name: "Jupe".into(),
            detached: false,
        })?
        .pieces[0];
    Ok((p, piece, sheet))
}

impl App {
    /// Ouvre la fenêtre « Ampleur » ; la pièce proposée est la première de la feuille affichée.
    pub fn open_flare(&mut self) {
        let project = self.editor.project();
        let list = candidates(project);
        let on_sheet = project
            .pieces_on(self.editor.sheet)
            .into_iter()
            .map(|p| p.id)
            .find(|id| list.iter().any(|c| c.0 == *id));
        let piece = on_sheet.or(list.first().map(|c| c.0));
        self.editor.flare.open = true;
        self.editor.flare.piece = piece;
        self.editor.flare_default_side();
    }

    /// Lignes d'ampleur des pièces de la feuille : deux lèvres en pointillés depuis le pivot, chaque
    /// ligne numérotée ; bord fixe et pivots prévus de la fenêtre ouverte.
    pub(crate) fn paint_flares(&self, painter: &egui::Painter, xf: &Xf) {
        let project = self.editor.project();
        let color = pal().measure;
        for piece in project.pieces_on(self.editor.sheet) {
            for (i, f) in project.piece_flares(piece.id).iter().enumerate() {
                for end in [f.stay_end(), f.moved_end()] {
                    polyline(
                        painter,
                        vec![xf.s(f.hinge()), xf.s(end)],
                        color,
                        1.8,
                        &[8.0, 4.0],
                        &[8.0, 4.0],
                    );
                }
                painter.circle_stroke(xf.s(f.hinge()), 4.0, stroke(2.0, color));
                let mid = f.stay_end().lerp(f.moved_end(), 0.5);
                label(
                    painter,
                    xf.s(mid) + egui::vec2(0.0, -4.0),
                    Align2::CENTER_BOTTOM,
                    &tr_args("flare.mark", &[("n", &(i + 1).to_string())]),
                    color,
                );
            }
        }
        let ui = &self.editor.flare;
        if !ui.open {
            return;
        }
        let (Some(piece), Some(side)) = (ui.piece, ui.side) else {
            return;
        };
        let sides = project.piece_flare_sides(piece);
        let Some(edge) = sides.get(side) else {
            return;
        };
        if project.piece(piece).is_ok_and(|p| !p.flares.is_empty()) {
            return;
        }
        painter.extend(egui::Shape::dashed_line(
            &[xf.s(edge.from), xf.s(edge.to)],
            stroke(3.0, pal().preview),
            10.0,
            5.0,
        ));
        label(
            painter,
            xf.s(edge.from.lerp(edge.to, 0.5)) + egui::vec2(0.0, 10.0),
            Align2::CENTER_TOP,
            &tr("flare.mark.fixed"),
            pal().preview,
        );
        if let Ok(Command::AddPieceFlare { lines, .. }) = self.editor.flare_command() {
            for l in lines {
                let t = l.at_mm / edge.length_mm;
                if (0.0..=1.0).contains(&t) {
                    painter.circle_stroke(
                        xf.s(edge.from.lerp(edge.to, t)),
                        5.0,
                        stroke(2.0, pal().preview),
                    );
                }
            }
        }
    }

    pub(crate) fn flare_window(&mut self, ctx: &egui::Context) {
        if !self.editor.flare.open {
            return;
        }
        let pieces = candidates(self.editor.project());
        if self
            .editor
            .flare
            .piece
            .is_some_and(|p| !pieces.iter().any(|c| c.0 == p))
        {
            self.editor.flare.piece = None;
            self.editor.flare.side = None;
        }
        let sides = self
            .editor
            .flare
            .piece
            .map(|p| self.editor.project().piece_flare_sides(p))
            .unwrap_or_default();
        let existing = self.editor.flare.piece.and_then(|piece| {
            let p = self.editor.project().piece(piece).ok()?;
            (!p.flares.is_empty()).then(|| {
                (
                    p.flares.len(),
                    p.flares.iter().map(|f| f.opening_mm()).sum::<f64>(),
                )
            })
        });
        let preview = if existing.is_some() {
            None
        } else {
            self.editor.flare_preview()
        };
        let (mut open, mut action) = (true, 0u8);
        let mut piece_changed = false;
        let slot = crate::dock::slot(ctx, &self.dock, crate::dock::ToolWin::Flare);
        let width = self.dock.content_width(crate::dock::ToolWin::Flare);
        egui::Window::new(tr("flare.title"))
            .id(egui::Id::new("flare_window"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .fixed_pos(slot.pos)
            .vscroll(true)
            .max_height(slot.max_height)
            .show(ctx, |ui| {
                ui.set_width(width);
                crate::a11y::label(
                    ui,
                    RichText::new(tr("flare.intro")).small().color(pal().muted),
                );
                let st = &mut self.editor.flare;
                egui::Grid::new("flare-grid").num_columns(2).show(ui, |ui| {
                    crate::a11y::label(ui, tr("flare.piece"));
                    let current = st
                        .piece
                        .and_then(|p| pieces.iter().find(|c| c.0 == p))
                        .map(|c| c.1.clone())
                        .unwrap_or_else(|| tr("flare.choose"));
                    let before = st.piece;
                    egui::ComboBox::from_id_salt("flare-piece")
                        .selected_text(current)
                        .show_ui_named(ui, |ui| {
                            for (id, name) in &pieces {
                                ui.selectable_value(&mut st.piece, Some(*id), name);
                            }
                        });
                    piece_changed = before != st.piece;
                    ui.end_row();
                    crate::a11y::label(ui, tr("flare.side"));
                    let current = st
                        .side
                        .and_then(|i| sides.get(i).map(|s| (i, s)))
                        .map(|(i, s)| {
                            tr_args(
                                "flare.side.item",
                                &[
                                    ("n", &(i + 1).to_string()),
                                    ("length", &format_mm(s.length_mm)),
                                ],
                            )
                        })
                        .unwrap_or_else(|| tr("flare.side.none"));
                    egui::ComboBox::from_id_salt("flare-side")
                        .selected_text(current)
                        .show_ui_named(ui, |ui| {
                            for (i, s) in sides.iter().enumerate() {
                                ui.selectable_value(
                                    &mut st.side,
                                    Some(i),
                                    tr_args(
                                        "flare.side.item",
                                        &[
                                            ("n", &(i + 1).to_string()),
                                            ("length", &format_mm(s.length_mm)),
                                        ],
                                    ),
                                );
                            }
                        });
                    ui.end_row();
                    crate::a11y::label(ui, tr("flare.count"));
                    crate::a11y::add_text(
                        ui,
                        egui::TextEdit::singleline(&mut st.count).desired_width(70.0),
                    );
                    ui.end_row();
                    crate::a11y::label(ui, tr("flare.positions"));
                    crate::a11y::add_text(
                        ui,
                        egui::TextEdit::singleline(&mut st.positions).desired_width(150.0),
                    );
                    ui.end_row();
                    crate::a11y::label(ui, tr("flare.opening"));
                    crate::a11y::add_text(
                        ui,
                        egui::TextEdit::singleline(&mut st.opening).desired_width(150.0),
                    );
                    ui.end_row();
                });
                crate::a11y::label(
                    ui,
                    RichText::new(tr("flare.hint")).small().color(pal().muted),
                );
                crate::a11y::label(
                    ui,
                    RichText::new(tr("flare.bottom")).small().color(pal().muted),
                );
                ui.separator();
                match &preview {
                    Some(Ok(p)) => {
                        crate::a11y::label(
                            ui,
                            tr_args(
                                "flare.result.fixed",
                                &[
                                    ("before", &format_mm(p.fixed_before_mm)),
                                    ("after", &format_mm(p.fixed_after_mm)),
                                ],
                            ),
                        );
                        crate::a11y::label(
                            ui,
                            tr_args("flare.result.open", &[("gain", &format_mm(p.open_gain_mm))]),
                        );
                        crate::a11y::label(
                            ui,
                            tr_args(
                                "flare.result.area",
                                &[
                                    ("before", &cm2(p.area_before_mm2)),
                                    ("after", &cm2(p.area_after_mm2)),
                                    ("pbefore", &format_mm(p.perimeter_before_mm)),
                                    ("pafter", &format_mm(p.perimeter_after_mm)),
                                ],
                            ),
                        );
                        let angles: Vec<String> = p
                            .lines
                            .iter()
                            .map(|l| format!("{}°", format_number(l.angle_deg, 1)))
                            .collect();
                        crate::a11y::label(
                            ui,
                            tr_args("flare.result.angles", &[("angles", &angles.join(" ; "))]),
                        );
                    }
                    Some(Err(why)) => {
                        crate::a11y::label(
                            ui,
                            RichText::new(tr_args("flare.refused", &[("reason", why)]))
                                .color(pal().error),
                        );
                    }
                    None => {}
                }
                if let Some((count, gain)) = existing {
                    crate::a11y::label(
                        ui,
                        tr_args(
                            "flare.existing",
                            &[("count", &count.to_string()), ("gain", &format_mm(gain))],
                        ),
                    );
                }
                ui.separator();
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(
                            matches!(preview, Some(Ok(_))),
                            egui::Button::new(tr("flare.apply")),
                        )
                        .clicked()
                    {
                        action = 2;
                    }
                    if ui.button(tr("flare.close")).clicked() {
                        action = 3;
                    }
                });
            });
        if piece_changed {
            self.editor.flare_default_side();
        }
        match action {
            2 => self.editor.flare_apply(),
            3 => open = false,
            _ => {}
        }
        if !open {
            self.editor.flare.open = false;
        }
    }
}
