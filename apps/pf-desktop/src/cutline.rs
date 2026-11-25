//! Fenêtre « Découper… » (US-07.5) : choisir une pièce et une ligne (un tracé de la feuille qui
//! traverse la pièce de bord à bord, ou une droite saisie par deux points), puis le nombre ou le
//! pas des crans de repère. Elle ne calcule rien : l'aperçu (aire, périmètre et bord de découpe de
//! chaque morceau, positions des crans, raison d'un refus) vient de `Project::cut_preview`, la liste
//! des lignes utilisables de `Project::cut_candidates`, la pose est `Command::CutPiece` (une seule
//! étape annulable, refus sans trace). Les points de départ et d'arrivée sont marqués sur le plan
//! avec un libellé, jamais la couleur seule.
//!
//! Limites : la ligne est un seul tracé (segment, courbe de Bézier, arc) ou une droite ; pas de
//! polyligne ; coordonnées saisies au clavier (pas de clic sur le plan).

use crate::a11y::ComboExt;
use crate::app::App;
use crate::canvas::{label, stroke, Xf};
use crate::editor::Editor;
use crate::palette::pal;
use eframe::egui::{self, Align2, RichText};
use pf_core::input::{format_mm, format_number};
use pf_core::units::parse_length_display;
use pf_core::{Command, CutLine, CutNotches, CutPreview, EntityId, PieceId, Project};
use pf_i18n::{tr, tr_args};

/// État de la fenêtre « Découper… » (interface seulement, jamais enregistré).
#[derive(Debug, Clone, PartialEq)]
pub struct CutUi {
    pub open: bool,
    pub piece: Option<PieceId>,
    /// `None` = droite saisie par deux points.
    pub line: Option<EntityId>,
    pub ax: String,
    pub ay: String,
    pub bx: String,
    pub by: String,
    /// Vrai : un cran tous les `step` mm ; faux : `count` crans à intervalles égaux.
    pub by_step: bool,
    pub count: String,
    pub step: String,
}

impl Default for CutUi {
    fn default() -> Self {
        CutUi {
            open: false,
            piece: None,
            line: None,
            ax: String::new(),
            ay: String::new(),
            bx: String::new(),
            by: String::new(),
            by_step: false,
            count: pf_core::CUT_DEFAULT_NOTCH_COUNT.to_string(),
            step: "50".into(),
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

impl Editor {
    /// Commande décrite par la fenêtre.
    pub(crate) fn cutline_command(&self) -> Result<Command, String> {
        let ui = &self.cutline;
        let Some(piece) = ui.piece else {
            return Err(tr("status.cutline.no_piece"));
        };
        let bad = || tr("status.cutline.bad_values");
        let line = match ui.line {
            Some(entity) => CutLine::Path { entity },
            None => CutLine::Points {
                a: pf_geometry::PointMm::new(
                    parse_length_display(&ui.ax).map_err(|_| bad())?,
                    parse_length_display(&ui.ay).map_err(|_| bad())?,
                ),
                b: pf_geometry::PointMm::new(
                    parse_length_display(&ui.bx).map_err(|_| bad())?,
                    parse_length_display(&ui.by).map_err(|_| bad())?,
                ),
            },
        };
        let notches = if ui.by_step {
            CutNotches::Step {
                step_mm: parse_length_display(&ui.step).map_err(|_| bad())?,
            }
        } else {
            CutNotches::Count {
                count: ui.count.trim().parse().map_err(|_| bad())?,
            }
        };
        Ok(Command::CutPiece {
            piece,
            line,
            notches,
        })
    }

    /// Aperçu de la découpe (lecture seule : même calcul que la commande).
    fn cutline_preview(&self) -> Option<Result<CutPreview, String>> {
        self.cutline.piece?;
        let cmd = match self.cutline_command() {
            Ok(c) => c,
            Err(e) => return Some(Err(e)),
        };
        let Command::CutPiece {
            piece,
            line,
            notches,
        } = cmd
        else {
            return None;
        };
        Some(
            self.project()
                .cut_preview(piece, &line, &notches)
                .map_err(|e| crate::text::core_error(&e)),
        )
    }

    /// Découpe la pièce : une étape annulable ; l'affichage suit le premier morceau.
    pub fn cutline_apply(&mut self) {
        let cmd = match self.cutline_command() {
            Ok(c) => c,
            Err(e) => {
                self.error(e);
                return;
            }
        };
        let Some(piece) = self.cutline.piece else {
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
            "status.cutline.done",
            &[
                ("name", &name),
                ("count", &(out.notches.len() / 2).to_string()),
            ],
        ));
        // Les deux morceaux remplacent la pièce : la fenêtre repart d'un choix vide.
        self.cutline.piece = None;
        self.cutline.line = None;
    }

    /// Propose la droite par défaut de la pièce choisie et vide le choix de tracé.
    fn cutline_reset_line(&mut self) {
        self.cutline.line = None;
        let Some(piece) = self.cutline.piece else {
            return;
        };
        if let Some((a, b)) = self.project().default_cut_line(piece) {
            let ui = &mut self.cutline;
            ui.ax = pf_core::units::field_text(a.x);
            ui.ay = pf_core::units::field_text(a.y);
            ui.bx = pf_core::units::field_text(b.x);
            ui.by = pf_core::units::field_text(b.y);
        }
    }
}

/// Projet de démonstration (captures `--etape decoupe`) : rectangle 200 x 100 mm.
pub fn demo_cut_project() -> Result<(pf_core::Project, PieceId), pf_core::CoreError> {
    use pf_core::{PieceSource, PointRef};
    use pf_geometry::PointMm;
    let mut p = pf_core::Project::new("Devant");
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
            name: "Devant".into(),
            detached: false,
        })?
        .pieces[0];
    // Une ligne interne déjà tracée, oblique, qui dépasse le contour des deux côtés.
    p.apply(Command::AddSegment {
        sheet,
        from: PointRef::New(PointMm::new(40.0, -20.0)),
        to: PointRef::New(PointMm::new(140.0, 120.0)),
    })?;
    Ok((p, piece))
}

impl App {
    /// Ouvre la fenêtre « Découper… » ; la pièce proposée est la première de la feuille affichée.
    pub fn open_cutline(&mut self) {
        let project = self.editor.project();
        let list = candidates(project);
        let on_sheet = project
            .pieces_on(self.editor.sheet)
            .into_iter()
            .map(|p| p.id)
            .find(|id| list.iter().any(|c| c.0 == *id));
        self.editor.cutline.open = true;
        self.editor.cutline.piece = on_sheet.or(list.first().map(|c| c.0));
        self.editor.cutline_reset_line();
    }

    /// Départ et arrivée de la ligne de découpe sur le plan (fenêtre ouverte).
    pub(crate) fn paint_cutline(&self, painter: &egui::Painter, xf: &Xf) {
        if !self.editor.cutline.open {
            return;
        }
        let Some(Ok(p)) = self.editor.cutline_preview() else {
            return;
        };
        let color = pal().preview;
        painter.extend(egui::Shape::dashed_line(
            &[xf.s(p.points[0]), xf.s(p.points[1])],
            stroke(2.0, color),
            10.0,
            5.0,
        ));
        for (at, key) in [
            (p.points[0], "cutline.mark.start"),
            (p.points[1], "cutline.mark.end"),
        ] {
            painter.circle_stroke(xf.s(at), 5.0, stroke(2.0, color));
            label(
                painter,
                xf.s(at) + egui::vec2(8.0, -6.0),
                Align2::LEFT_BOTTOM,
                &tr(key),
                color,
            );
        }
    }

    pub(crate) fn cutline_window(&mut self, ctx: &egui::Context) {
        if !self.editor.cutline.open {
            return;
        }
        let pieces = candidates(self.editor.project());
        if self
            .editor
            .cutline
            .piece
            .is_some_and(|p| !pieces.iter().any(|c| c.0 == p))
        {
            self.editor.cutline.piece = None;
            self.editor.cutline.line = None;
        }
        let lines: Vec<(EntityId, String)> = self
            .editor
            .cutline
            .piece
            .map(|piece| {
                let project = self.editor.project();
                match project.piece_sheet(piece) {
                    Ok(sheet) => project
                        .cut_candidates(piece)
                        .into_iter()
                        .map(|e| (e, crate::text::entity_name(sheet, e)))
                        .collect(),
                    Err(_) => Vec::new(),
                }
            })
            .unwrap_or_default();
        let preview = self.editor.cutline_preview();
        let (mut open, mut action) = (true, 0u8);
        let mut piece_changed = false;
        let slot = crate::dock::slot(ctx, &self.dock, crate::dock::ToolWin::Cutline);
        let width = self.dock.content_width(crate::dock::ToolWin::Cutline);
        egui::Window::new(tr("cutline.title"))
            .id(egui::Id::new("cutline_window"))
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
                    RichText::new(tr("cutline.intro"))
                        .small()
                        .color(pal().muted),
                );
                let st = &mut self.editor.cutline;
                egui::Grid::new("cutline-grid")
                    .num_columns(2)
                    .show(ui, |ui| {
                        crate::a11y::label(ui, tr("cutline.piece"));
                        let current = st
                            .piece
                            .and_then(|p| pieces.iter().find(|c| c.0 == p))
                            .map(|c| c.1.clone())
                            .unwrap_or_else(|| tr("cutline.choose"));
                        let before = st.piece;
                        egui::ComboBox::from_id_salt("cutline-piece")
                            .selected_text(current)
                            .show_ui_named(ui, |ui| {
                                for (id, name) in &pieces {
                                    ui.selectable_value(&mut st.piece, Some(*id), name);
                                }
                            });
                        piece_changed = before != st.piece;
                        ui.end_row();
                        crate::a11y::label(ui, tr("cutline.line"));
                        let current = st
                            .line
                            .and_then(|e| lines.iter().find(|l| l.0 == e))
                            .map(|l| l.1.clone())
                            .unwrap_or_else(|| tr("cutline.line.points"));
                        egui::ComboBox::from_id_salt("cutline-line")
                            .selected_text(current)
                            .show_ui_named(ui, |ui| {
                                ui.selectable_value(&mut st.line, None, tr("cutline.line.points"));
                                for (id, name) in &lines {
                                    ui.selectable_value(&mut st.line, Some(*id), name);
                                }
                            });
                        ui.end_row();
                        if st.line.is_none() {
                            crate::a11y::label(ui, tr("cutline.from"));
                            ui.horizontal(|ui| {
                                crate::a11y::add_text(
                                    ui,
                                    egui::TextEdit::singleline(&mut st.ax).desired_width(70.0),
                                );
                                crate::a11y::add_text(
                                    ui,
                                    egui::TextEdit::singleline(&mut st.ay).desired_width(70.0),
                                );
                            });
                            ui.end_row();
                            crate::a11y::label(ui, tr("cutline.to"));
                            ui.horizontal(|ui| {
                                crate::a11y::add_text(
                                    ui,
                                    egui::TextEdit::singleline(&mut st.bx).desired_width(70.0),
                                );
                                crate::a11y::add_text(
                                    ui,
                                    egui::TextEdit::singleline(&mut st.by).desired_width(70.0),
                                );
                            });
                            ui.end_row();
                        }
                        crate::a11y::label(ui, tr("cutline.notches"));
                        ui.horizontal(|ui| {
                            ui.radio_value(&mut st.by_step, false, tr("cutline.notches.count"));
                            ui.radio_value(&mut st.by_step, true, tr("cutline.notches.step"));
                        });
                        ui.end_row();
                        crate::a11y::label(
                            ui,
                            if st.by_step {
                                tr("cutline.step")
                            } else {
                                tr("cutline.count")
                            },
                        );
                        if st.by_step {
                            crate::a11y::add_text(
                                ui,
                                egui::TextEdit::singleline(&mut st.step).desired_width(70.0),
                            );
                        } else {
                            crate::a11y::add_text(
                                ui,
                                egui::TextEdit::singleline(&mut st.count).desired_width(70.0),
                            );
                        }
                        ui.end_row();
                    });
                crate::a11y::label(
                    ui,
                    RichText::new(tr("cutline.hint")).small().color(pal().muted),
                );
                ui.separator();
                match &preview {
                    Some(Ok(p)) => {
                        for (n, h) in [(1, &p.left), (2, &p.right)] {
                            crate::a11y::label(
                                ui,
                                tr_args(
                                    "cutline.result.half",
                                    &[
                                        ("n", &n.to_string()),
                                        ("area", &cm2(h.area_mm2)),
                                        ("perimeter", &format_mm(h.perimeter_mm)),
                                        ("edge", &format_mm(h.edge_mm)),
                                    ],
                                ),
                            );
                        }
                        let at: Vec<String> = p
                            .notch_distances_mm
                            .iter()
                            .map(|d| {
                                let s = pf_core::units::len_number(*d, 1);
                                s.strip_suffix(",0")
                                    .or_else(|| s.strip_suffix(".0"))
                                    .unwrap_or(&s)
                                    .to_string()
                            })
                            .collect();
                        crate::a11y::label(
                            ui,
                            pf_i18n::tr_len(
                                "cutline.result.notches",
                                &[
                                    ("count", &p.notch_distances_mm.len().to_string()),
                                    ("positions", &at.join(" ; ")),
                                    ("length", &format_mm(p.cut_length_mm)),
                                ],
                            ),
                        );
                        if let Some(why) = &p.refusal {
                            crate::a11y::label(
                                ui,
                                RichText::new(tr_args("cutline.refused", &[("reason", why)]))
                                    .color(pal().error),
                            );
                        }
                    }
                    Some(Err(why)) => {
                        crate::a11y::label(
                            ui,
                            RichText::new(tr_args("cutline.refused", &[("reason", why)]))
                                .color(pal().error),
                        );
                    }
                    None => {}
                }
                ui.separator();
                ui.horizontal(|ui| {
                    let ok = matches!(&preview, Some(Ok(p)) if p.refusal.is_none());
                    if ui
                        .add_enabled(ok, egui::Button::new(tr("cutline.apply")))
                        .clicked()
                    {
                        action = 2;
                    }
                    if ui.button(tr("cutline.close")).clicked() {
                        action = 3;
                    }
                });
            });
        if piece_changed {
            self.editor.cutline_reset_line();
        }
        match action {
            2 => self.editor.cutline_apply(),
            3 => open = false,
            _ => {}
        }
        if !open {
            self.editor.cutline.open = false;
        }
    }
}
