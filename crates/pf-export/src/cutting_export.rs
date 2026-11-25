//! US-15.3 / US-13.5 : sorties du **plan de coupe** (planche de pièces) en SVG, PDF et CSV.
//!
//! Le plan est exporté **à l'échelle 1:1** (laize en mm) par la mécanique d'assemblage existante :
//! la même `Scene` que les autres exports (SVG en mm, PDF paginé par `pdf::export_pdf_lang`, carré
//! étalon et recouvrement du réglage de page). La scène contient le bord de la laize, les
//! lisières hachurées, le pli (étoffe pliée) en tirets, les contours posés (traits de coupe), une
//! étiquette par pose (pièce, rang, taille), les cotes de laize et de longueur et une légende de
//! métrage. **Légendes ASCII uniquement**, dans la langue demandée (clés `cutx.*`) ; seuls les
//! noms de pièces sont ceux de l'utilisateur. Un plan sans pose est refusé (`NothingSelected`).
//!
//! Métrage : un plan invalide porte la mention « invalide » et **aucun chiffre** (longueur,
//! rendement) ; le CSV écrit alors des cellules vides. Lecture seule : le projet n'est pas modifié.

use crate::report::{csv_bytes, tx, Cell, Row, Section};
use crate::size_export::dash_polyline;
use crate::{
    num, pdf, svg, ExportError, OutputContent, PageSetup, PathGeom, PathRole, Scene,
    SceneDimension, ScenePath, ScenePieceLabel,
};
use pf_core::cutting_layout::PlanAnalysis;
use pf_core::cutting_yield::{plan_yield, PlanYield, YieldStatus};
use pf_core::hide_layout::{hide_yield, polygon_area_abs};
use pf_core::{CuttingPlan, CuttingPlanId, DefectKind, GrainSense, Hide, PlanePoint, Project};
use pf_geometry::PointMm;
use pf_i18n::{lookup, Lang};

/// Distance (mm) de la ligne de cote au bord de la laize.
const DIM_OFFSET_MM: f64 = 25.0;
/// Pas (mm) des hachures de lisière.
const HATCH_STEP_MM: f64 = 8.0;

fn t(lang: Lang, key: &str) -> String {
    lookup(lang, key)
}

fn ta(lang: Lang, key: &str, args: &[(&str, &str)]) -> String {
    let mut s = lookup(lang, key);
    for (k, v) in args {
        s = s.replace(&format!("{{{k}}}"), v);
    }
    s
}

fn line(a: PointMm, b: PointMm, role: PathRole) -> ScenePath {
    ScenePath {
        geom: PathGeom::Line(a, b),
        role,
        sense: GrainSense::Unmarked,
    }
}

fn label(center: PointMm, text: String) -> ScenePieceLabel {
    ScenePieceLabel {
        piece: String::new(),
        center,
        lines: vec![text],
    }
}

fn size_text(project: &Project, size: Option<pf_core::SizeId>, lang: Lang) -> String {
    match size {
        None => t(lang, "cutx.size.base"),
        Some(id) => project
            .find_size(id)
            .map(|(r, i)| r.sizes[i].label.clone())
            .unwrap_or_default(),
    }
}

/// Scène du plan (voir le module).
pub fn cutting_scene(
    project: &Project,
    plan_id: CuttingPlanId,
    lang: Lang,
) -> Result<Scene, ExportError> {
    let plan = project.cutting_plan(plan_id)?;
    let analysis = project.analyze_cutting_plan(plan_id)?;
    if analysis.outlines.is_empty() {
        return Err(ExportError::EmptySheet(plan.name.clone()));
    }
    let yield_ = plan_yield(project, plan, &analysis);
    let (w, sel) = (plan.fabric_width_mm, plan.selvedge_mm);
    let top = analysis
        .outlines
        .iter()
        .map(|o| o.bbox.max.y)
        .fold(0.0_f64, f64::max)
        .max(1.0);
    let mut paths: Vec<ScenePath> = Vec::new();
    let mut labels: Vec<ScenePieceLabel> = Vec::new();
    // Peau (US-15.5) : son contour remplace la laize, les lisières et le pli.
    if let Some(h) = &plan.hide {
        draw_hide(h, lang, &mut paths, &mut labels);
    }
    // Bord de la laize.
    let corners = [
        PointMm::new(0.0, 0.0),
        PointMm::new(w, 0.0),
        PointMm::new(w, top),
        PointMm::new(0.0, top),
    ];
    for i in 0..(if plan.hide.is_some() { 0 } else { 4 }) {
        paths.push(line(
            corners[i],
            corners[(i + 1) % 4],
            PathRole::Construction,
        ));
    }
    // Lisières hachurées.
    if sel > 0.0 && plan.hide.is_none() {
        for (x0, x1) in [(0.0, sel), (w - sel, w)] {
            let band = [
                PointMm::new(x0, 0.0),
                PointMm::new(x1, 0.0),
                PointMm::new(x1, top),
                PointMm::new(x0, top),
            ];
            for (a, b) in pf_geometry::hatch::hatch_segments(&band, 45.0, HATCH_STEP_MM) {
                paths.push(line(a, b, PathRole::Construction));
            }
            labels.push(label(
                PointMm::new((x0 + x1) / 2.0, top + 6.0),
                t(lang, "cutx.selvedge"),
            ));
        }
    }
    // Pli.
    if plan.fold == pf_core::FabricFold::Folded && plan.hide.is_none() {
        for (a, b) in dash_polyline(
            &[PointMm::new(w / 2.0, 0.0), PointMm::new(w / 2.0, top)],
            false,
            &[8.0, 4.0],
        ) {
            paths.push(line(a, b, PathRole::Construction));
        }
        labels.push(label(
            PointMm::new(w / 2.0, top + 6.0),
            t(lang, "cutx.fold"),
        ));
    }
    // Contours posés et étiquettes.
    for (rank, pl) in plan.placements.iter().enumerate() {
        let Some(o) = analysis.outlines.iter().find(|o| o.placement == pl.id) else {
            continue;
        };
        let n = o.polygon.len();
        for i in 0..n {
            paths.push(line(
                o.polygon[i],
                o.polygon[(i + 1) % n],
                PathRole::CuttingContour,
            ));
        }
        let name = project
            .piece(pl.piece)
            .map(|p| p.name.clone())
            .unwrap_or_default();
        let mut text = format!("{name} #{}", rank + 1);
        if pl.size.is_some() {
            text.push_str(&format!(" ({})", size_text(project, pl.size, lang)));
        }
        labels.push(label(
            PointMm::new(
                (o.bbox.min.x + o.bbox.max.x) / 2.0,
                (o.bbox.min.y + o.bbox.max.y) / 2.0,
            ),
            text,
        ));
    }
    let (length_text, yield_text) = legend(&yield_, lang);
    if let Some(h) = &plan.hide {
        let hy = hide_yield(Some(h), &analysis);
        return Ok(hide_finish(
            project, plan, h, hy, &analysis, lang, paths, labels, top,
        ));
    }
    // Cotes : laize sous le plan, longueur à droite.
    let dims = vec![
        SceneDimension {
            from: PointMm::new(0.0, 0.0),
            to: PointMm::new(w, 0.0),
            from_offset: PointMm::new(0.0, -DIM_OFFSET_MM),
            to_offset: PointMm::new(w, -DIM_OFFSET_MM),
            text: ta(
                lang,
                "cutx.width",
                &[("w", &num(w)), ("u", &num(w - 2.0 * sel))],
            ),
        },
        SceneDimension {
            from: PointMm::new(w, 0.0),
            to: PointMm::new(w, top),
            from_offset: PointMm::new(w + DIM_OFFSET_MM, 0.0),
            to_offset: PointMm::new(w + DIM_OFFSET_MM, top),
            text: length_text,
        },
    ];
    labels.push(label(PointMm::new(0.0, -2.0 * DIM_OFFSET_MM), yield_text));
    let title = ta(lang, "cutx.title", &[("name", &plan.name)]);
    let mut scene = Scene::calibration_square_lang(lang);
    scene.project_name = project.name.clone();
    scene.sheet_name = title;
    scene.paths = paths;
    scene.piece_labels = labels;
    scene.dimensions = dims;
    Ok(scene)
}

/// Motifs de hachure d'un défaut : angles (degrés), distincts par genre (jamais la couleur seule).
fn defect_angles(kind: DefectKind) -> &'static [f64] {
    match kind {
        DefectKind::Hole => &[45.0, 135.0],
        DefectKind::Scar => &[45.0],
        DefectKind::Stain => &[135.0],
        DefectKind::Thin => &[90.0],
        DefectKind::Other => &[0.0],
    }
}

fn kind_key(kind: DefectKind) -> &'static str {
    match kind {
        DefectKind::Hole => "cutx.hide.kind.hole",
        DefectKind::Scar => "cutx.hide.kind.scar",
        DefectKind::Stain => "cutx.hide.kind.stain",
        DefectKind::Thin => "cutx.hide.kind.thin",
        DefectKind::Other => "cutx.hide.kind.other",
    }
}

fn mm_points(poly: &[PlanePoint]) -> Vec<PointMm> {
    poly.iter().map(|p| PointMm::new(p.x, p.y)).collect()
}

fn bbox_center(poly: &[PointMm]) -> PointMm {
    let (mut lo, mut hi) = (poly[0], poly[0]);
    for p in poly {
        lo = PointMm::new(lo.x.min(p.x), lo.y.min(p.y));
        hi = PointMm::new(hi.x.max(p.x), hi.y.max(p.y));
    }
    PointMm::new((lo.x + hi.x) / 2.0, (lo.y + hi.y) / 2.0)
}

fn closed_lines(poly: &[PointMm], role: PathRole, paths: &mut Vec<ScenePath>) {
    let n = poly.len();
    for i in 0..n {
        paths.push(line(poly[i], poly[(i + 1) % n], role));
    }
}

/// Contour de peau, défauts (contour tireté + hachures propres au genre + libellé) et zones de
/// qualité (contour en tirets propres au grade + libellé).
fn draw_hide(h: &Hide, lang: Lang, paths: &mut Vec<ScenePath>, labels: &mut Vec<ScenePieceLabel>) {
    let contour = mm_points(&h.contour);
    closed_lines(&contour, PathRole::Construction, paths);
    labels.push(label(
        PointMm::new(contour[0].x, contour[0].y - 6.0),
        t(lang, "cutx.hide.contour"),
    ));
    for z in &h.quality_zones {
        let poly = mm_points(&z.polygon);
        let pattern: &[f64] = match z.grade {
            1 => &[14.0, 2.0],
            2 => &[8.0, 4.0],
            _ => &[3.0, 4.0],
        };
        for (a, b) in dash_polyline(&poly, true, pattern) {
            paths.push(line(a, b, PathRole::Construction));
        }
        labels.push(label(
            bbox_center(&poly),
            ta(lang, "cutx.hide.zone", &[("g", &z.grade.to_string())]),
        ));
    }
    for d in &h.defects {
        let poly = mm_points(&d.polygon);
        closed_lines(&poly, PathRole::SewingContour, paths);
        let mut ccw = poly.clone();
        if pf_core::cutting_layout::polygon_signed_area(&ccw) < 0.0 {
            ccw.reverse();
        }
        for angle in defect_angles(d.kind) {
            for (a, b) in pf_geometry::hatch::hatch_segments(&ccw, *angle, 3.0) {
                paths.push(line(a, b, PathRole::Construction));
            }
        }
        labels.push(label(
            bbox_center(&poly),
            ta(
                lang,
                "cutx.hide.defect",
                &[
                    ("id", &d.id.0.to_string()),
                    ("kind", &t(lang, kind_key(d.kind))),
                ],
            ),
        ));
    }
}

/// Fin de la scène d'un plan sur peau : cote de longueur et légende de métrage cuir.
#[allow(clippy::too_many_arguments)]
fn hide_finish(
    project: &Project,
    plan: &CuttingPlan,
    h: &Hide,
    hy: Option<pf_core::hide_layout::HideYield>,
    analysis: &PlanAnalysis,
    lang: Lang,
    paths: Vec<ScenePath>,
    mut labels: Vec<ScenePieceLabel>,
    top: f64,
) -> Scene {
    let contour = mm_points(&h.contour);
    let (xmax, ymin, ymax) = contour.iter().fold(
        (f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY),
        |(x, lo, hi), p| (x.max(p.x), lo.min(p.y), hi.max(p.y)),
    );
    let top = top.max(ymax);
    let (length_text, yield_text) = match hy {
        Some(y) => {
            let length = analysis
                .outlines
                .iter()
                .map(|o| o.bbox.max.y)
                .fold(0.0_f64, f64::max);
            (
                ta(lang, "cutx.length", &[("l", &num(length))]),
                ta(
                    lang,
                    "cutx.hide.yield",
                    &[
                        ("used", &format!("{:.2}", y.used_area_mm2)),
                        ("useful", &format!("{:.2}", y.useful_area_mm2)),
                        ("y", &format!("{:.2}", y.percent)),
                    ],
                ),
            )
        }
        None => (t(lang, "cutx.length_invalid"), t(lang, "cutx.hide.invalid")),
    };
    labels.push(label(
        PointMm::new(xmax / 2.0, ymin - 2.0 * DIM_OFFSET_MM),
        yield_text,
    ));
    let dims = vec![SceneDimension {
        from: PointMm::new(xmax, ymin),
        to: PointMm::new(xmax, top),
        from_offset: PointMm::new(xmax + DIM_OFFSET_MM, ymin),
        to_offset: PointMm::new(xmax + DIM_OFFSET_MM, top),
        text: length_text,
    }];
    let mut scene = Scene::calibration_square_lang(lang);
    scene.project_name = project.name.clone();
    scene.sheet_name = ta(lang, "cutx.title", &[("name", &plan.name)]);
    scene.paths = paths;
    scene.piece_labels = labels;
    scene.dimensions = dims;
    scene
}

/// Textes de la cote de longueur et de la légende de rendement (ASCII).
fn legend(y: &PlanYield, lang: Lang) -> (String, String) {
    match (&y.status, &y.figures) {
        (YieldStatus::Valid, Some(g)) => (
            ta(lang, "cutx.length", &[("l", &num(g.length_mm))]),
            ta(
                lang,
                "cutx.yield",
                &[("y", &format!("{:.2}", g.yield_percent))],
            ),
        ),
        _ => (t(lang, "cutx.length_invalid"), t(lang, "cutx.invalid")),
    }
}

/// Octets du plan en SVG (mm, échelle 1:1).
pub fn cutting_svg(
    project: &Project,
    plan: CuttingPlanId,
    lang: Lang,
) -> Result<Vec<u8>, ExportError> {
    let scene = cutting_scene(project, plan, lang)?;
    svg::export_svg_lang(&scene, lang).map(String::into_bytes)
}

/// Octets du plan en PDF, échelle 1:1, paginé selon `setup` (carré étalon, recouvrement).
pub fn cutting_pdf(
    project: &Project,
    plan: CuttingPlanId,
    setup: &PageSetup,
    lang: Lang,
) -> Result<Vec<u8>, ExportError> {
    let scene = cutting_scene(project, plan, lang)?;
    pdf::export_pdf_lang(
        std::slice::from_ref(&scene),
        setup,
        &OutputContent::default(),
        lang,
    )
    .map(|out| out.bytes)
}

fn status_text(s: YieldStatus, lang: Lang) -> String {
    t(
        lang,
        match s {
            YieldStatus::Valid => "cutx.status.valid",
            YieldStatus::Invalid => "cutx.status.invalid",
            YieldStatus::Empty => "cutx.status.empty",
        },
    )
}

fn n2(v: f64) -> Cell {
    Cell::Num(format!("{v:.2}"))
}

fn rows_of(project: &Project, plan: &CuttingPlan, analysis: &PlanAnalysis, lang: Lang) -> Vec<Row> {
    let y = plan_yield(project, plan, analysis);
    let material = plan
        .material
        .clone()
        .unwrap_or_else(|| t(lang, "cutx.unset"));
    let head = |status: YieldStatus| {
        vec![
            tx(plan.name.clone()),
            tx(material.clone()),
            tx(status_text(status, lang)),
        ]
    };
    let row = |cells: Vec<Cell>| Row { cells, note: None };
    match y.figures {
        Some(g) => g
            .by_size
            .iter()
            .map(|r| {
                let mut c = head(y.status);
                c.extend([
                    tx(size_text(project, r.size, lang)),
                    Cell::Num(r.pieces.to_string()),
                    n2(r.used_area_mm2),
                    n2(g.length_mm),
                    n2(g.useful_width_mm),
                    n2(r.share_percent),
                    n2(g.yield_percent),
                ]);
                row(c)
            })
            .collect(),
        None => {
            let mut c = head(y.status);
            c.extend((0..7).map(|_| tx("")));
            vec![row(c)]
        }
    }
}

/// CSV du métrage de **tous** les plans du projet : une ligne par plan et par taille posée
/// (séparateur `;`, BOM, CRLF, première colonne `metrage`). Un plan invalide ou vide : une ligne
/// sans chiffre.
pub fn cutting_csv(project: &Project, lang: Lang) -> Result<Vec<u8>, ExportError> {
    let mut rows = Vec::new();
    for plan in &project.cutting_plans {
        let analysis = project.analyze_cutting_plan(plan.id)?;
        rows.extend(rows_of(project, plan, &analysis, lang));
    }
    let header = [
        "plan",
        "material",
        "status",
        "size",
        "pieces",
        "used_area",
        "length",
        "useful_width",
        "size_share",
        "plan_yield",
    ]
    .map(|k| t(lang, &format!("cutx.csv.{k}")))
    .to_vec();
    let section = Section {
        code: "metrage",
        title: String::new(),
        header,
        rows,
        pdf: Vec::new(),
        empty: String::new(),
    };
    let mut sections = vec![section];
    sections.extend(hide_sections(project, lang)?);
    Ok(csv_bytes(&sections))
}

/// Sections « peau » et « peau_poses » : seulement si un plan a une peau (sinon aucun octet de
/// plus : le CSV d'un projet sans cuir est inchangé).
fn hide_sections(project: &Project, lang: Lang) -> Result<Vec<Section>, ExportError> {
    let mut head_rows: Vec<Row> = Vec::new();
    let mut pose_rows: Vec<Row> = Vec::new();
    let row = |cells: Vec<Cell>| Row { cells, note: None };
    for plan in project.cutting_plans.iter().filter(|p| p.hide.is_some()) {
        let Some(h) = &plan.hide else { continue };
        let an = project.analyze_cutting_plan(plan.id)?;
        let y = plan_yield(project, plan, &an);
        let hy = hide_yield(Some(h), &an);
        let contour = polygon_area_abs(&h.contour);
        let defects: f64 = h
            .defects
            .iter()
            .map(|d| polygon_area_abs(&d.polygon))
            .sum::<f64>()
            + 0.0;
        head_rows.push(row(vec![
            tx(plan.name.clone()),
            tx(status_text(y.status, lang)),
            n2(contour),
            n2(defects),
            n2((contour - defects).max(0.0)),
            hy.map_or_else(|| tx(""), |v| n2(v.used_area_mm2)),
            hy.map_or_else(|| tx(""), |v| n2(v.percent)),
            Cell::Num(an.defect_hits.len().to_string()),
            Cell::Num(an.hide_overflows.len().to_string()),
        ]));
        for (rank, pl) in plan.placements.iter().enumerate() {
            let name = project
                .piece(pl.piece)
                .map(|p| p.name.clone())
                .unwrap_or_default();
            let hit: f64 = an
                .defect_hits
                .iter()
                .filter(|d| d.placement == pl.id)
                .map(|d| d.area_mm2)
                .sum::<f64>()
                + 0.0;
            let out: f64 = an
                .hide_overflows
                .iter()
                .filter(|d| d.placement == pl.id)
                .map(|d| d.area_mm2)
                .sum::<f64>()
                + 0.0;
            let q = an.qualities.iter().find(|q| q.placement == pl.id);
            pose_rows.push(row(vec![
                tx(plan.name.clone()),
                tx(name),
                Cell::Num((rank + 1).to_string()),
                n2(hit),
                n2(out),
                q.and_then(|q| q.worst_grade)
                    .map_or_else(|| tx(""), |g| Cell::Num(g.to_string())),
                q.map_or_else(|| tx(""), |q| n2(q.areas_mm2[0])),
                q.map_or_else(|| tx(""), |q| n2(q.areas_mm2[1])),
                q.map_or_else(|| tx(""), |q| n2(q.areas_mm2[2])),
            ]));
        }
    }
    if head_rows.is_empty() {
        return Ok(Vec::new());
    }
    let header = |keys: &[&str]| -> Vec<String> {
        keys.iter()
            .map(|k| t(lang, &format!("cutx.csv.hide.{k}")))
            .collect()
    };
    let mk = |code: &'static str, header: Vec<String>, rows: Vec<Row>| Section {
        code,
        title: String::new(),
        header,
        rows,
        pdf: Vec::new(),
        empty: String::new(),
    };
    Ok(vec![
        mk(
            "peau",
            header(&[
                "plan",
                "status",
                "contour",
                "defects",
                "useful",
                "used",
                "percent",
                "hits",
                "overflows",
            ]),
            head_rows,
        ),
        mk(
            "peau_poses",
            header(&[
                "plan",
                "piece",
                "rank",
                "defect_area",
                "overflow_area",
                "worst_grade",
                "g1",
                "g2",
                "g3",
            ]),
            pose_rows,
        ),
    ])
}

/// Nom (sans extension) du fichier : `<projet>-<plan>` assaini.
pub fn cutting_file_stem(project: &str, plan: &str) -> String {
    let clean = |s: &str| {
        let c: String = s
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        if c.is_empty() {
            "plan".to_string()
        } else {
            c
        }
    };
    format!("{}-{}", clean(project), clean(plan))
}
