//! Contrôle des raccords (US-08.3) dans la fenêtre « Coutures… » : section repliée « Raccords »
//! (une ligne par couple et par extrémité) et fenêtre « Vue assemblée ». Elle ne calcule aucune
//! géométrie : angles, somme, écart, statut et vue assemblée viennent de
//! `Project::seam_junction_report` et `Project::seam_assembled_view` (lectures **observées**,
//! recalculées à chaque image, jamais stockées, aucune `Command`). Le statut porte toujours une
//! icône dessinée **et** un texte ; les couleurs ne sont qu'un renfort. Seul l'ajustement de la
//! vue au cadre (pixels) est fait ici.

use crate::app::App;
use crate::canvas::{label, polyline, stroke};
use crate::palette::pal;
use crate::seams::SeamUi;
use eframe::egui::{self, Align2, Color32, Pos2, RichText, Sense, Shape};
use pf_core::input::format_number;
use pf_core::{
    AssembledView, JunctionEnd, JunctionIssue, JunctionReading, JunctionStatus, Project, SeamIssue,
    SeamPairId, SeamWhich,
};
use pf_geometry::PointMm;
use pf_i18n::{tr, tr_args};

/// Hauteur maximale de la fenêtre de vue assemblée (points).
const VIEW_MAX_HEIGHT: f32 = 600.0;
/// Cadre de dessin de la vue assemblée (points).
const VIEW_SIZE: egui::Vec2 = egui::vec2(440.0, 300.0);

fn side_letter(side: SeamWhich) -> &'static str {
    match side {
        SeamWhich::A => "A",
        SeamWhich::B => "B",
    }
}

fn end_text(end: JunctionEnd) -> String {
    tr(match end {
        JunctionEnd::Start => "seams.join.end.start",
        JunctionEnd::End => "seams.join.end.end",
    })
}

fn degrees(v: Option<f64>) -> String {
    v.map_or_else(|| "-".into(), |v| format!("{}°", format_number(v, 1)))
}

fn signed_degrees(v: Option<f64>) -> String {
    v.map_or_else(
        || "-".into(),
        |v| {
            let sign = if v > 0.0 && format_number(v, 1) != "0" {
                "+"
            } else {
                ""
            };
            format!("{sign}{}°", format_number(v, 1))
        },
    )
}

fn reason_text(reason: JunctionIssue) -> String {
    match reason {
        JunctionIssue::Seam(SeamIssue::PieceMissing) => tr("seams.reason.piece_missing"),
        JunctionIssue::Seam(SeamIssue::OutlineOpen) => tr("seams.reason.outline_open"),
        JunctionIssue::Seam(SeamIssue::EdgeNotInOutline) => tr("seams.reason.edge_not_in_outline"),
        JunctionIssue::Seam(SeamIssue::Degenerate) => tr("seams.reason.degenerate"),
        JunctionIssue::TangentUndefined => tr("seams.join.reason.tangent"),
        JunctionIssue::Cusp => tr("seams.join.reason.cusp"),
        JunctionIssue::ChordDegenerate => tr("seams.join.reason.chord"),
    }
}

/// Texte du statut : lisse, cassé (bosse ou creux selon le signe de l'écart), non calculable.
fn status_text(r: &JunctionReading) -> String {
    match r.status {
        JunctionStatus::Smooth => tr("seams.join.status.smooth"),
        JunctionStatus::Broken if r.deviation_deg.is_some_and(|d| d > 0.0) => {
            tr("seams.join.status.bump")
        }
        JunctionStatus::Broken => tr("seams.join.status.hollow"),
        JunctionStatus::NotComputable { .. } => tr("seams.join.status.na"),
    }
}

fn status_color(r: &JunctionReading) -> Color32 {
    match r.status {
        JunctionStatus::Smooth => pal().internal,
        JunctionStatus::Broken => pal().error,
        JunctionStatus::NotComputable { .. } => pal().muted,
    }
}

/// Icône dessinée : cercle coché (lisse), triangle (cassé), cercle interrogé (non calculable).
fn status_icon(ui: &mut egui::Ui, r: &JunctionReading) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), Sense::hover());
    let p = ui.painter();
    let c = rect.center();
    let col = status_color(r);
    let at = |x: f32, y: f32| c + egui::vec2(x, y);
    match r.status {
        JunctionStatus::Smooth => {
            p.circle_stroke(c, 7.0, stroke(1.6, col));
            p.add(Shape::line(
                vec![at(-3.5, 0.5), at(-1.0, 3.0), at(3.5, -2.5)],
                stroke(1.8, col),
            ));
        }
        JunctionStatus::Broken => {
            p.add(Shape::closed_line(
                vec![at(0.0, -7.0), at(7.5, 6.0), at(-7.5, 6.0)],
                stroke(1.6, col),
            ));
            p.line_segment([at(0.0, -2.0), at(0.0, 1.5)], stroke(1.6, col));
            p.circle_filled(at(0.0, 3.8), 1.0, col);
        }
        JunctionStatus::NotComputable { .. } => {
            p.circle_stroke(c, 7.0, stroke(1.6, col));
            p.text(
                c,
                Align2::CENTER_CENTER,
                "?",
                egui::FontId::proportional(11.0),
                col,
            );
        }
    }
}

/// Section « Raccords » (repliée par défaut) : tableau des jonctions, « Vue assemblée » et note.
/// `clicked` reçoit le couple dont une ligne a été cliquée ; renvoie vrai quand la fenêtre de
/// vue assemblée doit s'ouvrir.
pub(crate) fn junction_section(
    ui: &mut egui::Ui,
    project: &Project,
    rows: &[JunctionReading],
    st: &mut SeamUi,
    clicked: &mut Option<SeamPairId>,
) -> bool {
    let mut open_view = false;
    let head = egui::CollapsingHeader::new(tr("seams.join.title"))
        .id_salt("seams-join")
        .open(Some(st.join_open))
        .show(ui, |ui| {
            crate::a11y::label(
                ui,
                RichText::new(tr("seams.join.intro"))
                    .small()
                    .color(pal().muted),
            );
            if rows.is_empty() {
                crate::a11y::label(ui, RichText::new(tr("seams.empty")).color(pal().muted));
                return;
            }
            egui::Grid::new("seams-join-grid")
                .num_columns(7)
                .spacing([8.0, 4.0])
                .striped(true)
                .show(ui, |ui| {
                    for key in [
                        "seams.col.n",
                        "seams.join.col.end",
                        "seams.join.col.a",
                        "seams.join.col.b",
                        "seams.join.col.sum",
                        "seams.join.col.dev",
                        "seams.col.status",
                    ] {
                        crate::a11y::label(ui, RichText::new(tr(key)).small().strong());
                    }
                    ui.end_row();
                    for r in rows {
                        let chosen = st.join_sel == Some((r.pair, r.end));
                        if ui.selectable_label(chosen, r.number.to_string()).clicked() {
                            *clicked = Some(r.pair);
                            st.join_sel = Some((r.pair, r.end));
                        }
                        crate::a11y::label(ui, RichText::new(end_text(r.end)).small());
                        crate::a11y::label(ui, RichText::new(degrees(r.angle_a_deg)).small());
                        crate::a11y::label(ui, RichText::new(degrees(r.angle_b_deg)).small());
                        crate::a11y::label(ui, RichText::new(degrees(r.sum_deg)).small());
                        crate::a11y::label(
                            ui,
                            RichText::new(signed_degrees(r.deviation_deg)).small(),
                        );
                        ui.horizontal(|ui| {
                            status_icon(ui, r);
                            crate::a11y::label(ui, RichText::new(status_text(r)).small());
                        });
                        ui.end_row();
                    }
                });
            for r in rows {
                if let (JunctionStatus::NotComputable { side, reason }, JunctionEnd::Start) =
                    (r.status, r.end)
                {
                    crate::a11y::label(
                        ui,
                        RichText::new(tr_args(
                            "seams.join.reason.line",
                            &[
                                ("n", &r.number.to_string()),
                                ("side", side_letter(side)),
                                ("reason", &reason_text(reason)),
                            ],
                        ))
                        .small()
                        .color(pal().muted),
                    );
                }
            }
            let ready = st
                .selected
                .is_some_and(|id| project.seam_assembled_view(id).is_ok());
            ui.add_enabled_ui(ready, |ui| {
                if ui.button(tr("seams.join.view.button")).clicked() {
                    open_view = true;
                }
            });
            let tol = rows.first().map_or(2.0, |r| r.tolerance_deg);
            crate::a11y::label(
                ui,
                RichText::new(tr_args(
                    "seams.join.note",
                    &[("tol", &format_number(tol, 1))],
                ))
                .small()
                .color(pal().muted),
            );
        });
    if head.header_response.clicked() {
        st.join_open = !st.join_open;
    }
    open_view
}

/// Repère de la jonction choisie sur le plan : un cercle à chaque sommet (A et B cousus) et un
/// libellé numéroté, dans la couleur du statut.
pub(crate) fn paint_junction(
    painter: &egui::Painter,
    xf: &crate::canvas::Xf,
    r: &JunctionReading,
    on_sheet_a: bool,
    on_sheet_b: bool,
) {
    let col = status_color(r);
    for (p, shown, name) in [(r.point_a, on_sheet_a, "A"), (r.point_b, on_sheet_b, "B")] {
        let (Some(p), true) = (p, shown) else {
            continue;
        };
        let c = xf.s(p);
        polyline(
            painter,
            (0..=24)
                .map(|t| {
                    let a = std::f32::consts::TAU * t as f32 / 24.0;
                    c + egui::vec2(a.cos(), a.sin()) * 11.0
                })
                .collect(),
            col,
            2.0,
            &[],
            &[],
        );
        label(
            painter,
            c + egui::vec2(0.0, 26.0),
            Align2::CENTER_TOP,
            &tr_args(
                "seams.join.mark",
                &[
                    ("n", &r.number.to_string()),
                    ("side", name),
                    ("sum", &degrees(r.sum_deg)),
                ],
            ),
            col,
        );
    }
}

/// Ajuste les points au cadre : (échelle, décalage) pixels, axe Y vers le haut.
struct Fit {
    min: (f64, f64),
    scale: f64,
    origin: Pos2,
}

impl Fit {
    fn new(view: &AssembledView, rect: egui::Rect) -> Fit {
        let pts = view.contour_a.iter().chain(&view.contour_b);
        let (mut lo, mut hi) = ((f64::MAX, f64::MAX), (f64::MIN, f64::MIN));
        for p in pts {
            lo = (lo.0.min(p.x), lo.1.min(p.y));
            hi = (hi.0.max(p.x), hi.1.max(p.y));
        }
        let margin = 34.0_f64;
        let (w, h) = (
            f64::from(rect.width()) - 2.0 * margin,
            f64::from(rect.height()) - 2.0 * margin,
        );
        let scale = (w / (hi.0 - lo.0).max(1e-6)).min(h / (hi.1 - lo.1).max(1e-6));
        let used = ((hi.0 - lo.0) * scale, (hi.1 - lo.1) * scale);
        Fit {
            min: lo,
            scale,
            origin: Pos2::new(
                rect.left() + ((f64::from(rect.width()) - used.0) / 2.0) as f32,
                rect.bottom() - ((f64::from(rect.height()) - used.1) / 2.0) as f32,
            ),
        }
    }

    fn s(&self, p: PointMm) -> Pos2 {
        Pos2::new(
            self.origin.x + ((p.x - self.min.0) * self.scale) as f32,
            self.origin.y - ((p.y - self.min.1) * self.scale) as f32,
        )
    }
}

fn paint_view(ui: &mut egui::Ui, view: &AssembledView) {
    let size = egui::vec2(ui.available_width(), VIEW_SIZE.y);
    let (resp, painter) = ui.allocate_painter(size, Sense::hover());
    let rect = resp.rect;
    painter.rect_filled(rect, 4.0, ui.visuals().extreme_bg_color);
    painter.rect_stroke(
        rect,
        4.0,
        stroke(1.0, pal().grid_major),
        egui::StrokeKind::Inside,
    );
    let fit = Fit::new(view, rect);
    let pts = |poly: &[PointMm], close: bool| -> Vec<Pos2> {
        let mut v: Vec<Pos2> = poly.iter().map(|p| fit.s(*p)).collect();
        if close {
            if let Some(f) = v.first().copied() {
                v.push(f);
            }
        }
        v
    };
    let (col_a, col_b) = (pal().preview, pal().accent_text);
    // A en trait plein, B en tirets : jamais la couleur seule.
    polyline(&painter, pts(&view.contour_a, true), col_a, 1.6, &[], &[]);
    polyline(
        &painter,
        pts(&view.contour_b, true),
        col_b,
        1.6,
        &[8.0],
        &[5.0],
    );
    polyline(
        &painter,
        pts(&view.seam_a, false),
        col_a.gamma_multiply(0.45),
        5.0,
        &[],
        &[],
    );
    polyline(
        &painter,
        pts(&view.seam_b, false),
        col_b.gamma_multiply(0.45),
        5.0,
        &[],
        &[],
    );
    let center = fit.s(PointMm::new(
        view.junctions.iter().map(|j| j.point_a.x).sum::<f64>() / 2.0,
        view.junctions.iter().map(|j| j.point_a.y).sum::<f64>() / 2.0,
    ));
    let end_gap = &view.junctions[1];
    if end_gap.gap_mm > 0.01 {
        polyline(
            &painter,
            vec![fit.s(end_gap.point_a), fit.s(end_gap.point_b)],
            pal().error,
            2.0,
            &[4.0],
            &[3.0],
        );
    }
    for (k, j) in view.junctions.iter().enumerate() {
        let col = status_color(&j.reading);
        let c = fit.s(j.point_a);
        for q in [fit.s(j.point_a), fit.s(j.point_b)] {
            painter.circle_stroke(q, 5.0, stroke(1.8, col));
        }
        // Libellé écarté du milieu de la couture, du côté de la jonction.
        let away = (c - center).normalized();
        let away = if away.x.is_finite() && away.y.is_finite() {
            away
        } else {
            egui::vec2(0.0, if k == 0 { 1.0 } else { -1.0 })
        };
        let anchor = if away.y > 0.0 {
            Align2::CENTER_TOP
        } else {
            Align2::CENTER_BOTTOM
        };
        label(
            &painter,
            c + away * 16.0,
            anchor,
            &tr_args(
                "seams.join.draw.point",
                &[
                    ("end", &end_text(j.reading.end)),
                    ("sum", &degrees(j.reading.sum_deg)),
                    ("status", &status_text(&j.reading)),
                ],
            ),
            col,
        );
    }
    label(
        &painter,
        rect.left_top() + egui::vec2(8.0, 8.0),
        Align2::LEFT_TOP,
        &tr("seams.join.draw.legend"),
        pal().muted,
    );
}

impl App {
    /// Fenêtre « Vue assemblée » du couple choisi (lecture seule) ; ferme avec la fenêtre
    /// Coutures ou quand le couple disparaît.
    pub(crate) fn junction_view_window(&mut self, ctx: &egui::Context) {
        let st = &self.editor.seam;
        if !st.join_view {
            return;
        }
        let project = self.editor.project().clone();
        let view = st
            .selected
            .and_then(|id| project.seam_assembled_view(id).ok());
        let Some(view) = view else {
            self.editor.seam.join_view = false;
            return;
        };
        let mut open = true;
        let slot = crate::dock::slot(ctx, &self.dock, crate::dock::ToolWin::JoinView);
        let width = self.dock.content_width(crate::dock::ToolWin::JoinView);
        egui::Window::new(tr_args(
            "seams.join.view.title",
            &[("n", &view.number.to_string())],
        ))
        .id(egui::Id::new("seams_join_view"))
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .fixed_pos(slot.pos)
        .vscroll(true)
        .max_height(VIEW_MAX_HEIGHT.min(slot.max_height))
        .show(ctx, |ui| {
            ui.set_width(width);
            paint_view(ui, &view);
            for j in &view.junctions {
                ui.horizontal(|ui| {
                    status_icon(ui, &j.reading);
                    crate::a11y::label(
                        ui,
                        RichText::new(tr_args(
                            "seams.join.view.line",
                            &[
                                ("end", &end_text(j.reading.end)),
                                ("a", &degrees(j.reading.angle_a_deg)),
                                ("b", &degrees(j.reading.angle_b_deg)),
                                ("sum", &degrees(j.reading.sum_deg)),
                                ("dev", &signed_degrees(j.reading.deviation_deg)),
                                ("status", &status_text(&j.reading)),
                            ],
                        ))
                        .small(),
                    );
                });
            }
            crate::a11y::label(
                ui,
                RichText::new(pf_i18n::tr_len(
                    "seams.join.view.gap",
                    &[
                        ("gap", &pf_core::units::len_number(view.end_gap_mm, 2)),
                        ("ca", &pf_core::units::len_number(view.chord_a_mm, 2)),
                        ("cb", &pf_core::units::len_number(view.chord_b_mm, 2)),
                    ],
                ))
                .small(),
            );
            crate::a11y::label(
                ui,
                RichText::new(tr("seams.join.view.note"))
                    .small()
                    .color(pal().muted),
            );
        });
        if !open {
            self.editor.seam.join_view = false;
        }
    }
}
