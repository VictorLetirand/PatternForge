//! Fenêtre « Plis » (US-07.3) : choisir une pièce, poser la ligne de pli (deux clics), puis la
//! profondeur, le genre (plat ou creux), le sens, le nombre et l'étendue. Elle ne calcule rien :
//! l'aperçu (largeur ajoutée, aires, raison d'un refus) rejoue `Command::AddPiecePleats` sur un clone
//! du projet, la pose est cette même commande (une seule étape annulable, refus sans trace) et la
//! pièce repliée vient de `Project::piece_folded`. Les repères (pli, contre-pli, flèche de sens)
//! viennent de `Project::piece_pleat_marks` ; chacun porte un libellé, jamais la couleur seule.
//!
//! Limites : la ligne se pose par deux clics (accrochage actif), pas par saisie de coordonnées ;
//! les repères ne sont pas exportés ; pas de retrait d'un pli isolé (annuler seulement).

use crate::a11y::ComboExt;
use crate::app::App;
use crate::canvas::{label, polyline, stroke, Xf};
use crate::editor::Editor;
use crate::palette::pal;
use eframe::egui::{self, Align2, Pos2, RichText, Vec2};
use pf_core::input::{format_mm, format_number};
use pf_core::units::parse_length_display;
use pf_core::{Command, PieceId, PleatKind, PleatLineKind, Project};
use pf_geometry::split::edge_sample;
use pf_geometry::{distance_mm, PointMm, Side, EPSILON_MM};
use pf_i18n::{tr, tr_args};

/// État de la fenêtre « Plis » (interface seulement, jamais enregistré).
#[derive(Debug, Clone, PartialEq)]
pub struct PleatUi {
    pub open: bool,
    pub piece: Option<PieceId>,
    /// 0 = aucun clic attendu ; 1 = premier point ; 2 = second point.
    pub picking: u8,
    pub from: Option<PointMm>,
    pub to: Option<PointMm>,
    pub kind: PleatKind,
    pub sense: Side,
    pub depth: String,
    pub count: String,
    pub span: String,
    pub show_folded: bool,
}

impl Default for PleatUi {
    fn default() -> Self {
        PleatUi {
            open: false,
            piece: None,
            picking: 0,
            from: None,
            to: None,
            kind: PleatKind::Flat,
            sense: Side::Right,
            depth: "10".into(),
            count: "1".into(),
            span: "100".into(),
            show_folded: false,
        }
    }
}

/// Résultat de l'aperçu d'une pose.
struct PleatPreview {
    width_mm: f64,
    area_before: f64,
    area_after: f64,
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
    /// Clic de pose de la ligne de pli (deux points, accrochage actif).
    pub fn pleat_click(&mut self, world: PointMm, tol_mm: f64) {
        let at = self.snap_at(world, tol_mm).pos;
        match self.pleat.picking {
            1 => {
                self.pleat.from = Some(at);
                self.pleat.to = None;
                self.pleat.picking = 2;
                self.info(tr("status.pleat.second"));
            }
            2 => {
                let Some(first) = self.pleat.from else {
                    self.pleat.picking = 1;
                    return;
                };
                if distance_mm(first, at) <= EPSILON_MM {
                    self.error(tr("status.pleat.same_point"));
                    return;
                }
                self.pleat.to = Some(at);
                self.pleat.picking = 0;
            }
            _ => {}
        }
    }

    /// Commande décrite par la fenêtre.
    fn pleat_command(&self) -> Result<Command, String> {
        let ui = &self.pleat;
        let (Some(piece), Some(from), Some(to)) = (ui.piece, ui.from, ui.to) else {
            return Err(tr("status.pleat.no_line"));
        };
        let bad = || tr("status.pleat.bad_values");
        let depth_mm = parse_length_display(&ui.depth).map_err(|_| bad())?;
        let count: u32 = ui.count.trim().parse().map_err(|_| bad())?;
        let span_mm = if count > 1 {
            parse_length_display(&ui.span).map_err(|_| bad())?
        } else {
            0.0
        };
        Ok(Command::AddPiecePleats {
            piece,
            from,
            to,
            depth_mm,
            kind: ui.kind,
            sense: ui.sense,
            count,
            span_mm,
        })
    }

    /// Aperçu de la pose (lecture seule : la commande est rejouée sur un clone).
    fn pleat_preview(&self) -> Option<Result<PleatPreview, String>> {
        let ui = &self.pleat;
        ui.piece?;
        ui.from?;
        ui.to?;
        let cmd = match self.pleat_command() {
            Ok(c) => c,
            Err(e) => return Some(Err(e)),
        };
        let piece = ui.piece?;
        let before = self.project().piece_contour(piece).ok()?.area_mm2;
        let mut clone = self.project().clone();
        Some(match clone.apply(cmd) {
            Ok(_) => {
                let after = clone
                    .piece_contour(piece)
                    .map(|c| c.area_mm2)
                    .unwrap_or(0.0);
                let width: f64 = clone
                    .piece(piece)
                    .map(|p| p.pleats.iter().map(|l| l.width_mm()).sum())
                    .unwrap_or(0.0)
                    - self
                        .project()
                        .piece(piece)
                        .map(|p| p.pleats.iter().map(|l| l.width_mm()).sum())
                        .unwrap_or(0.0);
                Ok(PleatPreview {
                    width_mm: width,
                    area_before: before,
                    area_after: after,
                })
            }
            Err(e) => Err(crate::text::core_error(&e)),
        })
    }

    /// Pose les plis : une étape annulable ; l'affichage suit la pièce reconstruite.
    pub fn pleat_apply(&mut self) {
        let cmd = match self.pleat_command() {
            Ok(c) => c,
            Err(e) => {
                self.error(e);
                return;
            }
        };
        let Some(piece) = self.pleat.piece else {
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
        self.pleat.from = None;
        self.pleat.to = None;
        self.info(tr_args(
            "status.pleat.done",
            &[("count", &out.pleats.len().to_string()), ("name", &name)],
        ));
    }

    /// Abandonne la pose de la ligne en cours.
    pub fn cancel_pleat_pick(&mut self) -> bool {
        if self.pleat.picking == 0 {
            return false;
        }
        self.pleat.picking = 0;
        self.info(tr("status.cancelled"));
        true
    }
}

/// Projet de démonstration (captures `--etape pli`) : jupe de 200 x 120 mm, trois plis plats de
/// 8 mm de profondeur répartis entre x = 50 et x = 130, bande vers la droite.
pub fn demo_pleat_project(
) -> Result<(pf_core::Project, PieceId, pf_core::SheetId), pf_core::CoreError> {
    use pf_core::{PieceSource, PointRef};
    let mut p = pf_core::Project::new("Plis");
    let sheet = p.sheets[0].id;
    p.apply(Command::AddRectangle {
        sheet,
        corner: PointRef::New(PointMm::new(0.0, 0.0)),
        width_mm: 200.0,
        height_mm: 120.0,
    })?;
    let piece = p
        .apply(Command::ExtractPiece {
            sheet,
            source: PieceSource::Click {
                at: PointMm::new(100.0, 60.0),
            },
            name: "Jupe".into(),
            detached: false,
        })?
        .pieces[0];
    let out = p.apply(Command::AddPiecePleats {
        piece,
        from: PointMm::new(50.0, 0.0),
        to: PointMm::new(50.0, 120.0),
        depth_mm: 8.0,
        kind: PleatKind::Flat,
        sense: Side::Right,
        count: 3,
        span_mm: 80.0,
    })?;
    let sheet = out
        .sheet
        .ok_or_else(|| pf_core::CoreError::Invalid("result sheet missing".into()))?;
    Ok((p, piece, sheet))
}

fn rot(v: Vec2, a: f32) -> Vec2 {
    let (s, c) = a.sin_cos();
    egui::vec2(v.x * c - v.y * s, v.x * s + v.y * c)
}

impl App {
    /// Ouvre la fenêtre « Plis » ; la pièce proposée est la première de la feuille affichée.
    pub fn open_pleats(&mut self) {
        let project = self.editor.project();
        let list = candidates(project);
        let on_sheet = project
            .pieces_on(self.editor.sheet)
            .into_iter()
            .map(|p| p.id)
            .find(|id| list.iter().any(|c| c.0 == *id));
        let piece = on_sheet.or(list.first().map(|c| c.0));
        let ui = &mut self.editor.pleat;
        ui.open = true;
        ui.piece = piece;
        ui.picking = 0;
        ui.from = None;
        ui.to = None;
    }

    /// Repères des plis des pièces de la feuille : pli (trait mixte épais), contre-pli (trait
    /// court), flèche de sens, chacun avec son libellé ; ligne en cours de pose ; pièce repliée
    /// (pointillés) si demandée.
    pub(crate) fn paint_pleats(&self, painter: &egui::Painter, xf: &Xf) {
        let project = self.editor.project();
        let color = pal().measure;
        for piece in project.pieces_on(self.editor.sheet) {
            for r in project.piece_pleat_marks(piece.id) {
                match &r.marks {
                    Ok(m) => {
                        for l in &m.lines {
                            let (dash, gap, width, name) = match l.kind {
                                PleatLineKind::Fold => (
                                    &[12.0, 3.0, 3.0, 3.0][..],
                                    &[3.0, 3.0, 3.0, 3.0][..],
                                    2.4,
                                    tr("pleat.mark.fold"),
                                ),
                                PleatLineKind::Counter => (
                                    &[4.0, 4.0][..],
                                    &[4.0, 4.0][..],
                                    1.6,
                                    tr("pleat.mark.counter"),
                                ),
                            };
                            polyline(
                                painter,
                                vec![xf.s(l.from), xf.s(l.to)],
                                color,
                                width,
                                dash,
                                gap,
                            );
                            // Le pli est nommé à un bout de la ligne, le contre-pli à l'autre.
                            let (at, anchor, dy) = match l.kind {
                                PleatLineKind::Fold => (l.from, Align2::CENTER_TOP, 4.0),
                                PleatLineKind::Counter => (l.to, Align2::CENTER_BOTTOM, -4.0),
                            };
                            label(
                                painter,
                                xf.s(at) + egui::vec2(0.0, dy),
                                anchor,
                                &name,
                                color,
                            );
                        }
                        for a in &m.arrows {
                            let (s, e) = (xf.s(a.from), xf.s(a.to));
                            painter.line_segment([s, e], stroke(2.0, color));
                            let v = e - s;
                            if v.length() > 1.0 {
                                let back = -v.normalized() * 9.0;
                                for ang in [0.45_f32, -0.45] {
                                    painter
                                        .line_segment([e, e + rot(back, ang)], stroke(2.0, color));
                                }
                            }
                            label(
                                painter,
                                s.lerp(e, 0.5) + egui::vec2(0.0, 6.0),
                                Align2::CENTER_TOP,
                                &tr("pleat.mark.sense"),
                                color,
                            );
                        }
                    }
                    Err(_) => {
                        label(
                            painter,
                            xf.s(r.pleat.start()) + egui::vec2(8.0, -6.0),
                            Align2::LEFT_BOTTOM,
                            &tr("pleat.mark.invalid"),
                            pal().error,
                        );
                    }
                }
            }
        }
        let ui = &self.editor.pleat;
        if let Some(from) = ui.from {
            painter.circle_stroke(xf.s(from), 5.0, stroke(2.0, pal().preview));
            if let Some(to) = ui.to {
                painter.circle_stroke(xf.s(to), 5.0, stroke(2.0, pal().preview));
                painter.extend(egui::Shape::dashed_line(
                    &[xf.s(from), xf.s(to)],
                    stroke(1.5, pal().preview),
                    6.0,
                    4.0,
                ));
            }
        }
        if ui.open && ui.show_folded {
            if let Some(piece) = ui.piece {
                if let Ok(f) = project.piece_folded(piece) {
                    let mut pts: Vec<Pos2> = Vec::new();
                    for e in &f.edges {
                        pts.extend(edge_sample(e, 32).into_iter().map(|p| xf.s(p)));
                    }
                    if let Some(first) = pts.first().copied() {
                        pts.push(first);
                    }
                    polyline(painter, pts, pal().preview, 2.0, &[8.0, 4.0], &[8.0, 4.0]);
                }
            }
        }
    }

    pub(crate) fn pleat_window(&mut self, ctx: &egui::Context) {
        if !self.editor.pleat.open {
            return;
        }
        let pieces = candidates(self.editor.project());
        if self
            .editor
            .pleat
            .piece
            .is_some_and(|p| !pieces.iter().any(|c| c.0 == p))
        {
            self.editor.pleat.piece = None;
        }
        let preview = self.editor.pleat_preview();
        let existing = self.editor.pleat.piece.and_then(|piece| {
            let p = self.editor.project().piece(piece).ok()?;
            (!p.pleats.is_empty()).then(|| {
                (
                    p.pleats.len(),
                    p.pleats.iter().map(|l| l.width_mm()).sum::<f64>(),
                )
            })
        });
        let folded = self
            .editor
            .pleat
            .piece
            .filter(|_| existing.is_some())
            .map(|piece| self.editor.project().piece_folded(piece));
        let (mut open, mut action) = (true, 0u8);
        let slot = crate::dock::slot(ctx, &self.dock, crate::dock::ToolWin::Pleat);
        let width = self.dock.content_width(crate::dock::ToolWin::Pleat);
        egui::Window::new(tr("pleat.title"))
            .id(egui::Id::new("pleat_window"))
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
                    RichText::new(tr("pleat.intro")).small().color(pal().muted),
                );
                let st = &mut self.editor.pleat;
                egui::Grid::new("pleat-grid").num_columns(2).show(ui, |ui| {
                    crate::a11y::label(ui, tr("pleat.piece"));
                    let current = st
                        .piece
                        .and_then(|p| pieces.iter().find(|c| c.0 == p))
                        .map(|c| c.1.clone())
                        .unwrap_or_else(|| tr("pleat.choose"));
                    egui::ComboBox::from_id_salt("pleat-piece")
                        .selected_text(current)
                        .show_ui_named(ui, |ui| {
                            for (id, name) in &pieces {
                                ui.selectable_value(&mut st.piece, Some(*id), name);
                            }
                        });
                    ui.end_row();
                    crate::a11y::label(ui, tr("pleat.line"));
                    let text = match (st.from, st.to) {
                        (Some(a), Some(b)) => tr_args(
                            "pleat.line.value",
                            &[
                                ("from", &pf_core::units::len_pair(a.x, a.y, 1)),
                                ("to", &pf_core::units::len_pair(b.x, b.y, 1)),
                            ],
                        ),
                        _ => tr("pleat.line.none"),
                    };
                    crate::a11y::label(ui, text);
                    ui.end_row();
                });
                if ui
                    .button(if st.picking > 0 {
                        tr("pleat.line.cancel")
                    } else {
                        tr("pleat.line.pick")
                    })
                    .clicked()
                {
                    action = 1;
                }
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut st.kind, PleatKind::Flat, tr("pleat.kind.flat"));
                    ui.selectable_value(&mut st.kind, PleatKind::Hollow, tr("pleat.kind.hollow"));
                });
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut st.sense, Side::Left, tr("pleat.sense.left"));
                    ui.selectable_value(&mut st.sense, Side::Right, tr("pleat.sense.right"));
                });
                egui::Grid::new("pleat-values")
                    .num_columns(3)
                    .show(ui, |ui| {
                        crate::a11y::label(ui, tr("pleat.depth"));
                        crate::a11y::add_text(
                            ui,
                            egui::TextEdit::singleline(&mut st.depth).desired_width(70.0),
                        );
                        crate::a11y::label(ui, pf_core::units::display_units().unit.suffix());
                        ui.end_row();
                        crate::a11y::label(ui, tr("pleat.count"));
                        crate::a11y::add_text(
                            ui,
                            egui::TextEdit::singleline(&mut st.count).desired_width(70.0),
                        );
                        ui.end_row();
                        if st.count.trim().parse::<u32>().is_ok_and(|n| n > 1) {
                            crate::a11y::label(ui, tr("pleat.span"));
                            crate::a11y::add_text(
                                ui,
                                egui::TextEdit::singleline(&mut st.span).desired_width(70.0),
                            );
                            crate::a11y::label(ui, pf_core::units::display_units().unit.suffix());
                            ui.end_row();
                        }
                    });
                ui.separator();
                match &preview {
                    Some(Ok(p)) => {
                        crate::a11y::label(
                            ui,
                            tr_args(
                                "pleat.result",
                                &[
                                    ("width", &format_mm(p.width_mm)),
                                    ("before", &cm2(p.area_before)),
                                    ("after", &cm2(p.area_after)),
                                ],
                            ),
                        );
                    }
                    Some(Err(why)) => {
                        crate::a11y::label(
                            ui,
                            RichText::new(tr_args("pleat.refused", &[("reason", why)]))
                                .color(pal().error),
                        );
                    }
                    None => {}
                }
                if let Some((count, width)) = existing {
                    ui.separator();
                    crate::a11y::label(
                        ui,
                        tr_args(
                            "pleat.existing",
                            &[("count", &count.to_string()), ("width", &format_mm(width))],
                        ),
                    );
                    ui.checkbox(&mut st.show_folded, tr("pleat.fold.show"));
                    match &folded {
                        Some(Ok(f)) => {
                            crate::a11y::label(
                                ui,
                                tr_args(
                                    "pleat.fold.info",
                                    &[
                                        ("width", &format_mm(f.removed_mm)),
                                        ("area", &cm2(f.area_mm2)),
                                        ("perimeter", &format_mm(f.perimeter_mm)),
                                    ],
                                ),
                            );
                        }
                        Some(Err(e)) => {
                            crate::a11y::label(
                                ui,
                                RichText::new(tr_args(
                                    "pleat.fold.error",
                                    &[("reason", &crate::text::core_error(e))],
                                ))
                                .color(pal().error),
                            );
                        }
                        None => {}
                    }
                }
                ui.separator();
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(
                            matches!(preview, Some(Ok(_))),
                            egui::Button::new(tr("pleat.apply")),
                        )
                        .clicked()
                    {
                        action = 2;
                    }
                    if ui.button(tr("pleat.close")).clicked() {
                        action = 3;
                    }
                });
            });
        match action {
            1 => {
                let st = &mut self.editor.pleat;
                if st.picking > 0 {
                    self.editor.cancel_pleat_pick();
                } else {
                    st.picking = 1;
                    st.from = None;
                    st.to = None;
                    self.editor.info(tr("status.pleat.first"));
                }
            }
            2 => self.editor.pleat_apply(),
            3 => open = false,
            _ => {}
        }
        if !open {
            self.editor.pleat.open = false;
            self.editor.pleat.picking = 0;
            self.editor.pleat.show_folded = false;
        }
    }
}
