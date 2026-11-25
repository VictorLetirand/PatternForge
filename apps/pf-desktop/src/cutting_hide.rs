//! US-15.5 « Cuir » : section « Peau » de la fenêtre Plan de coupe.
//!
//! Saisie du contour de la peau, des défauts et des zones de qualité : polygone **par clics** sur
//! le canevas (ou coordonnées saisies au clavier), **depuis une polyligne fermée** de la feuille
//! active (tracés sélectionnés) ou **depuis le contour d'une pièce**. Chaque geste est une
//! commande du cœur (un pas d'annulation, refus nommé sans trace). Aucun calcul géométrique ici :
//! diagnostics, aires et qualité viennent de `pf_core::hide_layout`. L'exigence de qualité est un
//! réglage d'interface (non enregistré : le format ne la porte pas).
//!
//! Un défaut est dit par son **motif** de hachures (genre), sa couleur et son libellé, jamais par
//! la couleur seule ; une zone de qualité par ses tirets (grade), sa teinte et son libellé « Q n ».

use crate::a11y::ComboExt;
use crate::app::App;
use crate::cutting::{fill_polygon, hatch, mm, outline, parse_len, CuttingDialog, Geo};
use crate::grading::section_header;
use crate::measurements::{edit, Buffers};
use eframe::egui::{self, Pos2, RichText, Shape, Stroke};
use pf_core::cutting_layout::PlanAnalysis;
use pf_core::hide_layout::{ccw, hide_yield, polygon_area_abs, quality_violations};
use pf_core::{
    Command, CuttingPlan, DefectKind, Hide, HideDefectId, PieceId, PlanePoint, Project,
    QualityZoneId,
};
use pf_geometry::PointMm;
use pf_i18n::{tr, tr_args};
use pf_ui::components::{button, ButtonKind};

const K_NOTE: u8 = 60;
const K_EDIT_NOTE: u8 = 61;

/// Objet saisi.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Target {
    Contour,
    Defect,
    Zone,
}

/// Objet existant sélectionné dans la liste.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Chosen {
    Defect(HideDefectId),
    Zone(QualityZoneId),
}

/// État d'interface de la section (jamais enregistré).
#[derive(Debug, Clone)]
pub struct HideUi {
    pub open: bool,
    pub(crate) target: Target,
    pub(crate) kind: DefectKind,
    pub(crate) grade: u8,
    pub(crate) note: String,
    /// Exigence de qualité : pire grade accepté (1 à 3) ; `None` = aucune.
    pub(crate) required: Option<u8>,
    pub(crate) drawing: bool,
    /// Objet remplacé par le tracé en cours (« Redessiner »).
    pub(crate) replacing: Option<Chosen>,
    pub(crate) points: Vec<(f64, f64)>,
    pt_x: String,
    pt_y: String,
    pub(crate) piece: Option<PieceId>,
    pub(crate) chosen: Option<Chosen>,
    bufs: Buffers,
}

impl Default for HideUi {
    fn default() -> Self {
        HideUi {
            open: true,
            target: Target::Contour,
            kind: DefectKind::Hole,
            grade: 1,
            note: String::new(),
            required: None,
            drawing: false,
            replacing: None,
            points: Vec::new(),
            pt_x: String::new(),
            pt_y: String::new(),
            piece: None,
            chosen: None,
            bufs: Buffers::new(),
        }
    }
}

/// Gestes de la section, exécutés par `App::hide_act`.
#[derive(Debug, Clone)]
pub(crate) enum HideAct {
    Cmd(Command),
    FromSheet,
    FromPiece(PieceId),
    Finish,
    Error(String),
}

fn kind_key(kind: DefectKind) -> &'static str {
    match kind {
        DefectKind::Hole => "cutting.hide.kind.hole",
        DefectKind::Scar => "cutting.hide.kind.scar",
        DefectKind::Stain => "cutting.hide.kind.stain",
        DefectKind::Thin => "cutting.hide.kind.thin",
        DefectKind::Other => "cutting.hide.kind.other",
    }
}

const KINDS: [DefectKind; 5] = [
    DefectKind::Hole,
    DefectKind::Scar,
    DefectKind::Stain,
    DefectKind::Thin,
    DefectKind::Other,
];

/// Angles de hachure par genre : le motif distingue le genre sans la couleur.
fn kind_angles(kind: DefectKind) -> &'static [f64] {
    match kind {
        DefectKind::Hole => &[45.0, 135.0],
        DefectKind::Scar => &[45.0],
        DefectKind::Stain => &[135.0],
        DefectKind::Thin => &[90.0],
        DefectKind::Other => &[0.0],
    }
}

fn plane(points: &[(f64, f64)]) -> Vec<PlanePoint> {
    points
        .iter()
        .map(|p| PlanePoint { x: p.0, y: p.1 })
        .collect()
}

fn mm_pts(poly: &[PlanePoint]) -> Vec<PointMm> {
    ccw(poly)
}

/// Lignes de diagnostic de la peau (défaut touché, débordement), pour la liste des contrôles.
pub(crate) fn diag_lines(
    name: &dyn Fn(pf_core::PlacementId) -> String,
    a: &PlanAnalysis,
) -> Vec<String> {
    let mut v = Vec::new();
    for h in &a.defect_hits {
        v.push(tr_args(
            "cutting.hide.diag.hit",
            &[
                ("name", &name(h.placement)),
                ("id", &h.defect.0.to_string()),
                ("kind", &tr(kind_key(h.kind))),
                ("area", &mm(h.area_mm2)),
            ],
        ));
    }
    for o in &a.hide_overflows {
        v.push(tr_args(
            "cutting.hide.diag.overflow",
            &[("name", &name(o.placement)), ("area", &mm(o.area_mm2))],
        ));
    }
    v
}

/// Ajoute un point cliqué sur le canevas (arrondi au dixième de mm) au tracé en cours.
pub(crate) fn canvas_click(d: &mut CuttingDialog, x: f64, y: f64) {
    if d.hide.drawing {
        d.hide
            .points
            .push(((x * 10.0).round() / 10.0, (y * 10.0).round() / 10.0));
    }
}

/// Peau, défauts, zones et tracé en cours sur le canevas.
pub(crate) fn paint_hide(
    painter: &egui::Painter,
    geo: Geo,
    plan: &CuttingPlan,
    ui_state: &HideUi,
    tokens: &pf_ui::tokens::Tokens,
) {
    if let Some(h) = &plan.hide {
        let contour = mm_pts(&h.contour);
        outline(
            painter,
            geo,
            &contour,
            Stroke::new(2.0_f32, tokens.border_strong),
        );
        for z in &h.quality_zones {
            let poly = mm_pts(&z.polygon);
            let alpha = match z.grade {
                1 => 0.10,
                2 => 0.20,
                _ => 0.32,
            };
            fill_polygon(painter, geo, &poly, tokens.warning.gamma_multiply(alpha));
            let pts: Vec<Pos2> = poly.iter().map(|p| geo.to_screen(p.x, p.y)).collect();
            let (dash, gap) = match z.grade {
                1 => (12.0, 2.0),
                2 => (7.0, 4.0),
                _ => (3.0, 4.0),
            };
            let mut closed = pts.clone();
            if let Some(first) = pts.first() {
                closed.push(*first);
            }
            painter.add(Shape::dashed_line(
                &closed,
                Stroke::new(1.2_f32, tokens.text_secondary),
                dash,
                gap,
            ));
            let c = bbox_center(&poly);
            painter.text(
                geo.to_screen(c.x, c.y),
                egui::Align2::CENTER_CENTER,
                tr_args("cutting.hide.grade.n", &[("g", &z.grade.to_string())]),
                egui::FontId::proportional(10.0),
                tokens.text_secondary,
            );
        }
        for d in &h.defects {
            let poly = mm_pts(&d.polygon);
            fill_polygon(painter, geo, &poly, tokens.danger.gamma_multiply(0.15));
            for a in kind_angles(d.kind) {
                hatch(painter, geo, &poly, *a, Stroke::new(1.0_f32, tokens.danger));
            }
            let selected = ui_state.chosen == Some(Chosen::Defect(d.id));
            outline(
                painter,
                geo,
                &poly,
                Stroke::new(
                    if selected { 3.0_f32 } else { 1.5_f32 },
                    if selected {
                        tokens.focus_ring
                    } else {
                        tokens.danger
                    },
                ),
            );
            let c = bbox_center(&poly);
            painter.text(
                geo.to_screen(c.x, c.y),
                egui::Align2::CENTER_CENTER,
                format!("D{} {}", d.id.0, tr(kind_key(d.kind))),
                egui::FontId::proportional(10.0),
                tokens.text_primary,
            );
        }
    }
    if ui_state.drawing && !ui_state.points.is_empty() {
        let pts: Vec<Pos2> = ui_state
            .points
            .iter()
            .map(|p| geo.to_screen(p.0, p.1))
            .collect();
        painter.add(Shape::line(
            pts.clone(),
            Stroke::new(1.5_f32, tokens.action),
        ));
        for p in &pts {
            painter.circle_filled(*p, 3.0, tokens.action);
        }
    }
}

fn bbox_center(poly: &[PointMm]) -> PointMm {
    let (mut lo, mut hi) = (poly[0], poly[0]);
    for p in poly {
        lo = PointMm::new(lo.x.min(p.x), lo.y.min(p.y));
        hi = PointMm::new(hi.x.max(p.x), hi.y.max(p.y));
    }
    PointMm::new((lo.x + hi.x) / 2.0, (lo.y + hi.y) / 2.0)
}

/// Section « Peau (cuir) » : saisie, liste, exigence de qualité, métrage et qualité des poses.
pub(crate) fn hide_section(
    ui: &mut egui::Ui,
    d: &mut CuttingDialog,
    project: &Project,
    plan: &CuttingPlan,
    acts: &mut Vec<HideAct>,
) {
    let tokens = pf_ui::theme::tokens(ui.ctx());
    let state = match &plan.hide {
        None => tr("cutting.hide.state.none"),
        Some(h) => tr_args(
            "cutting.hide.state.some",
            &[
                ("defects", &h.defects.len().to_string()),
                ("zones", &h.quality_zones.len().to_string()),
            ],
        ),
    };
    let mut open = d.hide.open;
    section_header(
        ui,
        &mut open,
        tr_args("cutting.hide.title", &[("state", &state)]),
    );
    d.hide.open = open;
    if !open {
        return;
    }
    let secondary = |t: &str| {
        RichText::new(t.to_string())
            .small()
            .color(tokens.text_secondary)
    };
    crate::a11y::label(ui, secondary(&tr("cutting.hide.intro")));
    if plan.hide.is_none() {
        crate::a11y::label(ui, tr("cutting.hide.none"));
    }
    entry_block(ui, d, project, plan, acts);
    if let Some(h) = &plan.hide {
        list_block(ui, d, plan, h, acts);
        results_block(ui, d, project, plan, h);
    }
    crate::a11y::label(ui, secondary(&tr("cutting.hide.limit")));
}

fn entry_block(
    ui: &mut egui::Ui,
    d: &mut CuttingDialog,
    project: &Project,
    plan: &CuttingPlan,
    acts: &mut Vec<HideAct>,
) {
    crate::a11y::label(ui, RichText::new(tr("cutting.hide.target")).strong());
    ui.horizontal_wrapped(|ui| {
        for (t, key) in [
            (Target::Contour, "cutting.hide.target.contour"),
            (Target::Defect, "cutting.hide.target.defect"),
            (Target::Zone, "cutting.hide.target.zone"),
        ] {
            // Défaut et zone n'ont de sens qu'avec un contour.
            if t != Target::Contour && plan.hide.is_none() {
                continue;
            }
            if ui.selectable_label(d.hide.target == t, tr(key)).clicked() && !d.hide.drawing {
                d.hide.target = t;
            }
        }
    });
    if plan.hide.is_none() {
        d.hide.target = Target::Contour;
    }
    match d.hide.target {
        Target::Defect => {
            crate::a11y::label(ui, tr("cutting.hide.kind.label"));
            ui.horizontal_wrapped(|ui| {
                for k in KINDS {
                    if ui
                        .selectable_label(d.hide.kind == k, tr(kind_key(k)))
                        .clicked()
                    {
                        d.hide.kind = k;
                    }
                }
            });
            crate::a11y::label(ui, tr("cutting.hide.note"));
            if let Some(t) = edit(ui, &mut d.hide.bufs, (K_NOTE, 0, 0), &d.hide.note, 200.0) {
                d.hide.note = t;
            }
        }
        Target::Zone => {
            crate::a11y::label(ui, tr("cutting.hide.grade.label"));
            ui.horizontal(|ui| {
                for g in 1..=3u8 {
                    let text = tr_args("cutting.hide.grade.n", &[("g", &g.to_string())]);
                    if ui.selectable_label(d.hide.grade == g, text).clicked() {
                        d.hide.grade = g;
                    }
                }
            });
        }
        Target::Contour => {}
    }
    if d.hide.drawing {
        crate::a11y::label(
            ui,
            RichText::new(tr_args(
                "cutting.hide.drawing",
                &[("n", &d.hide.points.len().to_string())],
            ))
            .color(tokens_action(ui)),
        );
        ui.horizontal(|ui| {
            crate::a11y::label(ui, tr("cutting.hide.pt_x"));
            crate::a11y::add_text(
                ui,
                egui::TextEdit::singleline(&mut d.hide.pt_x).desired_width(70.0),
            );
            crate::a11y::label(ui, tr("cutting.hide.pt_y"));
            crate::a11y::add_text(
                ui,
                egui::TextEdit::singleline(&mut d.hide.pt_y).desired_width(70.0),
            );
        });
        ui.horizontal_wrapped(|ui| {
            if button(ui, &tr("cutting.hide.pt_add"), ButtonKind::Secondary).clicked() {
                match (parse_len(&d.hide.pt_x), parse_len(&d.hide.pt_y)) {
                    (Ok(x), Ok(y)) => {
                        d.hide.points.push((x, y));
                        d.hide.pt_x.clear();
                        d.hide.pt_y.clear();
                    }
                    (Err(e), _) | (_, Err(e)) => acts.push(HideAct::Error(e)),
                }
            }
            if button(ui, &tr("cutting.hide.pt_undo"), ButtonKind::Secondary).clicked() {
                d.hide.points.pop();
            }
            if button(ui, &tr("cutting.hide.finish"), ButtonKind::Primary).clicked() {
                acts.push(HideAct::Finish);
            }
            if button(ui, &tr("cutting.hide.cancel"), ButtonKind::Secondary).clicked() {
                d.hide.drawing = false;
                d.hide.points.clear();
                d.hide.replacing = None;
            }
        });
        return;
    }
    ui.horizontal_wrapped(|ui| {
        if button(ui, &tr("cutting.hide.src.click"), ButtonKind::Secondary).clicked() {
            d.hide.drawing = true;
            d.hide.points.clear();
            d.hide.replacing = None;
        }
        if button(ui, &tr("cutting.hide.src.sheet"), ButtonKind::Secondary).clicked() {
            acts.push(HideAct::FromSheet);
        }
    });
    ui.horizontal_wrapped(|ui| {
        crate::a11y::label(ui, tr("cutting.hide.src.piece.pick"));
        let text = d
            .hide
            .piece
            .and_then(|id| project.piece(id).ok().map(|p| p.name.clone()))
            .unwrap_or_default();
        egui::ComboBox::from_id_salt("cutting_hide_piece")
            .selected_text(text)
            .show_ui_named(ui, |ui| {
                for p in &project.pieces {
                    if ui
                        .selectable_label(d.hide.piece == Some(p.id), &p.name)
                        .clicked()
                    {
                        d.hide.piece = Some(p.id);
                    }
                }
            });
        if button(ui, &tr("cutting.hide.src.piece"), ButtonKind::Secondary).clicked() {
            acts.push(match d.hide.piece {
                Some(id) => HideAct::FromPiece(id),
                None => HideAct::Error(tr("cutting.hide.err.piece")),
            });
        }
    });
}

fn tokens_action(ui: &egui::Ui) -> egui::Color32 {
    pf_ui::theme::tokens(ui.ctx()).action
}

fn list_block(
    ui: &mut egui::Ui,
    d: &mut CuttingDialog,
    plan: &CuttingPlan,
    h: &Hide,
    acts: &mut Vec<HideAct>,
) {
    ui.add_space(4.0);
    crate::a11y::label(
        ui,
        tr_args(
            "cutting.hide.row.contour",
            &[
                ("n", &h.contour.len().to_string()),
                ("area", &mm(polygon_area_abs(&h.contour) / 100.0)),
            ],
        ),
    );
    for df in &h.defects {
        let note = df
            .note
            .as_ref()
            .map_or(String::new(), |n| format!(" : {n}"));
        let text = tr_args(
            "cutting.hide.row.defect",
            &[
                ("id", &df.id.0.to_string()),
                ("kind", &tr(kind_key(df.kind))),
                ("n", &df.polygon.len().to_string()),
                ("note", &note),
            ],
        );
        let on = d.hide.chosen == Some(Chosen::Defect(df.id));
        if ui.selectable_label(on, text).clicked() {
            d.hide.chosen = Some(Chosen::Defect(df.id));
        }
    }
    for z in &h.quality_zones {
        let text = tr_args(
            "cutting.hide.row.zone",
            &[
                ("id", &z.id.0.to_string()),
                ("g", &z.grade.to_string()),
                ("n", &z.polygon.len().to_string()),
            ],
        );
        let on = d.hide.chosen == Some(Chosen::Zone(z.id));
        if ui.selectable_label(on, text).clicked() {
            d.hide.chosen = Some(Chosen::Zone(z.id));
        }
    }
    let plan_id = plan.id;
    match d.hide.chosen {
        Some(Chosen::Defect(id)) => {
            if let Some(df) = h.defects.iter().find(|x| x.id == id) {
                ui.horizontal_wrapped(|ui| {
                    for k in KINDS {
                        if ui.selectable_label(df.kind == k, tr(kind_key(k))).clicked()
                            && df.kind != k
                        {
                            acts.push(HideAct::Cmd(Command::UpdateHideDefect {
                                plan: plan_id,
                                defect: id,
                                kind: k,
                                polygon: df.polygon.clone(),
                                note: df.note.clone(),
                            }));
                        }
                    }
                });
                crate::a11y::label(ui, tr("cutting.hide.note"));
                let current = df.note.clone().unwrap_or_default();
                if let Some(t) = edit(
                    ui,
                    &mut d.hide.bufs,
                    (K_EDIT_NOTE, id.0, 0),
                    &current,
                    200.0,
                ) {
                    acts.push(HideAct::Cmd(Command::UpdateHideDefect {
                        plan: plan_id,
                        defect: id,
                        kind: df.kind,
                        polygon: df.polygon.clone(),
                        note: Some(t),
                    }));
                }
                ui.horizontal(|ui| {
                    if button(ui, &tr("cutting.hide.redraw"), ButtonKind::Secondary).clicked() {
                        d.hide.target = Target::Defect;
                        d.hide.kind = df.kind;
                        d.hide.note = df.note.clone().unwrap_or_default();
                        d.hide.drawing = true;
                        d.hide.points.clear();
                        d.hide.replacing = Some(Chosen::Defect(id));
                    }
                    if button(ui, &tr("cutting.hide.remove"), ButtonKind::Danger).clicked() {
                        d.hide.chosen = None;
                        acts.push(HideAct::Cmd(Command::RemoveHideDefect {
                            plan: plan_id,
                            defect: id,
                        }));
                    }
                });
            }
        }
        Some(Chosen::Zone(id)) => {
            if let Some(z) = h.quality_zones.iter().find(|x| x.id == id) {
                ui.horizontal(|ui| {
                    for g in 1..=3u8 {
                        let text = tr_args("cutting.hide.grade.n", &[("g", &g.to_string())]);
                        if ui.selectable_label(z.grade == g, text).clicked() && z.grade != g {
                            acts.push(HideAct::Cmd(Command::UpdateQualityZone {
                                plan: plan_id,
                                zone: id,
                                grade: g,
                                polygon: z.polygon.clone(),
                            }));
                        }
                    }
                });
                ui.horizontal(|ui| {
                    if button(ui, &tr("cutting.hide.redraw"), ButtonKind::Secondary).clicked() {
                        d.hide.target = Target::Zone;
                        d.hide.grade = z.grade;
                        d.hide.drawing = true;
                        d.hide.points.clear();
                        d.hide.replacing = Some(Chosen::Zone(id));
                    }
                    if button(ui, &tr("cutting.hide.remove"), ButtonKind::Danger).clicked() {
                        d.hide.chosen = None;
                        acts.push(HideAct::Cmd(Command::RemoveQualityZone {
                            plan: plan_id,
                            zone: id,
                        }));
                    }
                });
            }
        }
        None => {}
    }
    if button(ui, &tr("cutting.hide.remove_hide"), ButtonKind::Danger).clicked() {
        d.hide.chosen = None;
        acts.push(HideAct::Cmd(Command::RemoveHide { plan: plan_id }));
    }
}

fn results_block(
    ui: &mut egui::Ui,
    d: &mut CuttingDialog,
    project: &Project,
    plan: &CuttingPlan,
    h: &Hide,
) {
    let tokens = pf_ui::theme::tokens(ui.ctx());
    ui.add_space(4.0);
    crate::a11y::label(ui, tr("cutting.hide.required"));
    ui.horizontal(|ui| {
        if ui
            .selectable_label(d.hide.required.is_none(), tr("cutting.hide.required.none"))
            .clicked()
        {
            d.hide.required = None;
        }
        for g in 1..=3u8 {
            let text = tr_args("cutting.hide.required.n", &[("g", &g.to_string())]);
            if ui
                .selectable_label(d.hide.required == Some(g), text)
                .clicked()
            {
                d.hide.required = Some(g);
            }
        }
    });
    let Some(a) = d.cache_analysis() else { return };
    let name = |id: pf_core::PlacementId| {
        let rank = plan.placements.iter().position(|p| p.id == id).unwrap_or(0);
        let piece = plan
            .placements
            .iter()
            .find(|p| p.id == id)
            .and_then(|p| project.piece(p.piece).ok().map(|p| p.name.clone()))
            .unwrap_or_default();
        format!("{piece} #{}", rank + 1)
    };
    match hide_yield(Some(h), a) {
        Some(y) => {
            crate::a11y::label(
                ui,
                RichText::new(tr_args(
                    "cutting.hide.yield",
                    &[
                        ("used", &mm(y.used_area_mm2 / 100.0)),
                        ("useful", &mm(y.useful_area_mm2 / 100.0)),
                        ("contour", &mm(y.contour_area_mm2 / 100.0)),
                        ("defects", &mm(y.defects_area_mm2 / 100.0)),
                        ("y", &format!("{:.2}", y.percent)),
                    ],
                ))
                .strong(),
            );
            if y.defects_overlap {
                crate::a11y::label(
                    ui,
                    RichText::new(tr("cutting.hide.yield.overlap")).color(tokens.warning),
                );
            }
        }
        None => {
            crate::a11y::label(
                ui,
                RichText::new(tr("cutting.hide.yield.invalid")).color(tokens.danger),
            );
        }
    }
    for q in &a.qualities {
        let text = match q.worst_grade {
            Some(g) => tr_args(
                "cutting.hide.quality.row",
                &[
                    ("name", &name(q.placement)),
                    ("worst", &g.to_string()),
                    ("a1", &mm(q.areas_mm2[0] / 100.0)),
                    ("a2", &mm(q.areas_mm2[1] / 100.0)),
                    ("a3", &mm(q.areas_mm2[2] / 100.0)),
                ],
            ),
            None => tr_args("cutting.hide.quality.none", &[("name", &name(q.placement))]),
        };
        crate::a11y::label(ui, RichText::new(text).color(tokens.text_secondary));
    }
    if let Some(req) = d.hide.required {
        for id in quality_violations(a, req) {
            let worst = a
                .qualities
                .iter()
                .find(|q| q.placement == id)
                .and_then(|q| q.worst_grade)
                .unwrap_or(0);
            crate::a11y::label(
                ui,
                RichText::new(format!(
                    "✗ {}",
                    tr_args(
                        "cutting.hide.diag.quality",
                        &[
                            ("name", &name(id)),
                            ("worst", &worst.to_string()),
                            ("req", &req.to_string()),
                        ],
                    )
                ))
                .color(tokens.danger),
            );
        }
    }
}

/// Projet de l'étape « cuir-peau » : les trois pièces de « plan-coupe » et un plan « Peau »
/// (contour de 5 sommets (0 ; 0), (1400 ; 0), (1400 ; 700), (700 ; 900), (0 ; 700) ; trou
/// 40 x 40 en (300 ; 300) ; cicatrice 200 x 20 en (600 ; 100) ; zone de qualité 1 en
/// (0 ; 0)-(700 ; 700), zone de qualité 3 en (700 ; 0)-(1400 ; 700)). Poses (sauf `empty`) : le
/// Devant en (0 ; 0) recouvre le trou (40 x 40 = 1 600 mm2) ; la Poche 120 x 150 en (1350 ; 650)
/// dépasse du coin : 18 000 - 2 857,142857 = 15 142,857143 mm2 dehors.
pub(crate) fn cuir_stage_project(
    empty: bool,
) -> Result<(Project, pf_core::CuttingPlanId), pf_core::CoreError> {
    use pf_core::cutting_plan::{CuttingPlanSpec, FabricFold};
    let (mut p, _, _) = crate::cutting::cutting_stage_project(true)?;
    let pl = |x: f64, y: f64| PlanePoint { x, y };
    let rect =
        |x0: f64, y0: f64, x1: f64, y1: f64| vec![pl(x0, y0), pl(x1, y0), pl(x1, y1), pl(x0, y1)];
    let plan = p
        .apply(Command::AddCuttingPlan {
            spec: CuttingPlanSpec {
                name: "Peau".into(),
                material: None,
                fabric_width_mm: 1400.0,
                fold: FabricFold::Flat,
                selvedge_mm: 0.0,
                min_gap_mm: 5.0,
                grain_direction: None,
            },
        })?
        .cutting_plans[0];
    p.apply(Command::SetHideContour {
        plan,
        contour: vec![
            pl(0.0, 0.0),
            pl(1400.0, 0.0),
            pl(1400.0, 700.0),
            pl(700.0, 900.0),
            pl(0.0, 700.0),
        ],
    })?;
    p.apply(Command::AddHideDefect {
        plan,
        kind: DefectKind::Hole,
        polygon: rect(300.0, 300.0, 340.0, 340.0),
        note: Some("trou".into()),
    })?;
    p.apply(Command::AddHideDefect {
        plan,
        kind: DefectKind::Scar,
        polygon: rect(600.0, 100.0, 800.0, 120.0),
        note: None,
    })?;
    p.apply(Command::AddQualityZone {
        plan,
        grade: 1,
        polygon: rect(0.0, 0.0, 700.0, 700.0),
    })?;
    p.apply(Command::AddQualityZone {
        plan,
        grade: 3,
        polygon: rect(700.0, 0.0, 1400.0, 700.0),
    })?;
    if !empty {
        let (devant, poche) = (p.pieces[0].id, p.pieces[2].id);
        for (piece, x, y) in [(devant, 0.0, 0.0), (poche, 1350.0, 650.0)] {
            p.apply(Command::AddPlacement {
                plan,
                piece,
                size: None,
                x_mm: x,
                y_mm: y,
                rotation_deg: 0.0,
                mirrored: false,
            })?;
        }
    }
    Ok((p, plan))
}

impl App {
    /// Exécute un geste de la section Peau.
    pub(crate) fn hide_act(&mut self, d: &mut CuttingDialog, act: HideAct) -> Result<(), String> {
        let project = self.editor.project().clone();
        let Some(plan) = d.selected else {
            return Ok(());
        };
        match act {
            HideAct::Error(e) => Err(e),
            HideAct::Cmd(c) => self.measure_exec(c).map(|_| ()),
            HideAct::FromSheet => {
                let sheet = self.editor.sheet().id;
                let entities = self.editor.selection.entities.clone();
                if entities.is_empty() {
                    return Err(tr("cutting.hide.err.sheet"));
                }
                let poly = project
                    .sheet_closed_polygon(sheet, &entities)
                    .map_err(|e| e.to_string())?;
                self.hide_submit(d, plan, poly)
            }
            HideAct::FromPiece(piece) => {
                let poly = project.piece_polygon(piece).map_err(|e| e.to_string())?;
                self.hide_submit(d, plan, poly)
            }
            HideAct::Finish => {
                if d.hide.points.len() < 3 {
                    return Err(tr("cutting.hide.err.points"));
                }
                let poly = plane(&d.hide.points);
                self.hide_submit(d, plan, poly)
            }
        }
    }

    /// Envoie le polygone à la commande qui correspond à l'objet saisi.
    fn hide_submit(
        &mut self,
        d: &mut CuttingDialog,
        plan: pf_core::CuttingPlanId,
        polygon: Vec<PlanePoint>,
    ) -> Result<(), String> {
        let note = (!d.hide.note.trim().is_empty()).then(|| d.hide.note.trim().to_string());
        let command = match (d.hide.target, d.hide.replacing) {
            (Target::Contour, _) => Command::SetHideContour {
                plan,
                contour: polygon,
            },
            (Target::Defect, Some(Chosen::Defect(defect))) => Command::UpdateHideDefect {
                plan,
                defect,
                kind: d.hide.kind,
                polygon,
                note,
            },
            (Target::Defect, _) => Command::AddHideDefect {
                plan,
                kind: d.hide.kind,
                polygon,
                note,
            },
            (Target::Zone, Some(Chosen::Zone(zone))) => Command::UpdateQualityZone {
                plan,
                zone,
                grade: d.hide.grade,
                polygon,
            },
            (Target::Zone, _) => Command::AddQualityZone {
                plan,
                grade: d.hide.grade,
                polygon,
            },
        };
        let out = self.measure_exec(command)?;
        d.hide.drawing = false;
        d.hide.points.clear();
        d.hide.replacing = None;
        d.hide.chosen = out
            .hide_defects
            .first()
            .map(|i| Chosen::Defect(*i))
            .or(out.quality_zones.first().map(|i| Chosen::Zone(*i)))
            .or(d.hide.chosen);
        Ok(())
    }
}
