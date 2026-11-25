//! Aides communes des tests d'export de la couture et de la coupe calculée (US-05.4) : pièces de
//! test, lecteurs SVG et PDF indépendants des écrivains (le DXF et le HPGL utilisent
//! `common::dxf` et `common::hpgl`). Aucune constante ni fonction des écrivains n'est utilisée :
//! 72 / 25,4 est écrit en dur.
#![allow(dead_code)]

use super::dxf_models::rect_piece;
use pf_core::{
    Command, EntityId, FoldAllowance, HemAllowance, PathRole, PieceId, PieceSource, PointId,
    PointRef, Project, SeamAllowance, SheetId,
};
use pf_export::{PieceExportMode, Scene, SceneOptions};
use pf_geometry::PointMm;

pub const MODES: [PieceExportMode; 3] = [
    PieceExportMode::Both,
    PieceExportMode::SewingOnly,
    PieceExportMode::CutOnly,
];

pub fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

pub fn near(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

pub fn scene_with(p: &Project, s: SheetId, mode: PieceExportMode) -> Scene {
    Scene::from_sheet_with(
        p,
        s,
        SceneOptions {
            piece_mode: mode,
            ..SceneOptions::default()
        },
    )
    .unwrap()
}

/// Rectangle 100 × 50 mm en (0 ; 0), contour de rôle couture, marge optionnelle.
pub fn rect_project(margin: Option<f64>) -> (Project, SheetId, PieceId) {
    let mut p = Project::new("Coupe");
    let s = p.sheets[0].id;
    let (piece, _) = rect_piece(
        &mut p,
        s,
        (0.0, 0.0),
        (100.0, 50.0),
        "Dos",
        PathRole::SewingContour,
        false,
    );
    if let Some(m) = margin {
        p.apply(Command::SetSeamAllowance {
            piece,
            seam: Some(SeamAllowance {
                default_mm: m,
                ..Default::default()
            }),
        })
        .unwrap();
    }
    (p, s, piece)
}

// ---------------------------------------------------------------------------
// Pièces plus riches : quart de disque avec rempli, polygone dont la coupe est impossible
// ---------------------------------------------------------------------------

pub fn segment(p: &mut Project, s: SheetId, a: PointMm, b: PointMm) -> EntityId {
    p.apply(Command::AddSegment {
        sheet: s,
        from: PointRef::New(a),
        to: PointRef::New(b),
    })
    .unwrap()
    .entities[0]
}

pub fn point_at(p: &Project, s: SheetId, at: PointMm) -> PointId {
    let sheet = p.sheet(s).unwrap();
    sheet
        .points
        .iter()
        .find(|q| pf_geometry::distance_mm(sheet.position(q.id).unwrap(), at) < 1e-9)
        .unwrap()
        .id
}

pub fn extract(p: &mut Project, s: SheetId, entities: Vec<EntityId>, name: &str) -> PieceId {
    p.apply(Command::ExtractPiece {
        sheet: s,
        source: PieceSource::Paths { entities },
        name: name.into(),
        detached: false,
    })
    .unwrap()
    .pieces[0]
}

/// Quart de disque de rayon 60 (centre à l'origine), rempli de 12 mm sur l'arc avec 3 crans.
pub fn quarter_fold() -> (Project, SheetId, PieceId) {
    quarter_fold_with(60.0, 12.0, 3)
}

/// Quart de disque de rayon `r`, rempli de `value` mm sur l'arc, `count` crans.
pub fn quarter_fold_with(r: f64, value: f64, count: u32) -> (Project, SheetId, PieceId) {
    let mut p = Project::new("Quartier");
    let s = p.sheets[0].id;
    let bottom = segment(&mut p, s, pt(0.0, 0.0), pt(r, 0.0));
    let left = segment(&mut p, s, pt(0.0, r), pt(0.0, 0.0));
    let (a, b) = (point_at(&p, s, pt(r, 0.0)), point_at(&p, s, pt(0.0, r)));
    let k = r * 0.5f64.sqrt();
    let arc = p
        .apply(Command::AddArcThreePoints {
            sheet: s,
            start: PointRef::Existing(a),
            through: PointRef::New(pt(k, k)),
            end: PointRef::Existing(b),
        })
        .unwrap()
        .entities[0];
    for e in [bottom, arc, left] {
        p.apply(Command::SetPathRole {
            sheet: s,
            entity: e,
            role: PathRole::SewingContour,
        })
        .unwrap();
    }
    let piece = extract(&mut p, s, vec![bottom, arc, left], "Quartier");
    p.apply(Command::SetSeamAllowance {
        piece,
        seam: Some(SeamAllowance {
            default_mm: 0.0,
            folds: vec![FoldAllowance {
                entity: arc,
                value_mm: value,
                notch_step_mm: None,
                notch_count: Some(count),
            }],
            ..Default::default()
        }),
    })
    .unwrap();
    (p, s, piece)
}

/// Polygone à encoche étroite dont l'ourlet de 8 mm rend la coupe impossible (cas du CA6 d'US-05.3).
pub fn impossible_cut() -> (Project, SheetId, PieceId) {
    let pts = [
        (0.0, 0.0),
        (100.0, 0.0),
        (100.0, 50.0),
        (52.0, 50.0),
        (52.0, 40.0),
        (48.0, 40.0),
        (48.0, 50.0),
        (0.0, 50.0),
    ];
    let mut p = Project::new("Impossible");
    let s = p.sheets[0].id;
    let n = pts.len();
    let edges: Vec<EntityId> = (0..n)
        .map(|i| {
            segment(
                &mut p,
                s,
                pt(pts[i].0, pts[i].1),
                pt(pts[(i + 1) % n].0, pts[(i + 1) % n].1),
            )
        })
        .collect();
    for &e in &edges {
        p.apply(Command::SetPathRole {
            sheet: s,
            entity: e,
            role: PathRole::SewingContour,
        })
        .unwrap();
    }
    let piece = extract(&mut p, s, edges.clone(), "Encoche");
    p.apply(Command::SetSeamAllowance {
        piece,
        seam: Some(SeamAllowance {
            default_mm: 8.0,
            hems: vec![HemAllowance {
                entity: edges[4],
                height_mm: 8.0,
            }],
            ..Default::default()
        }),
    })
    .unwrap();
    assert!(
        p.piece_cut_contour(piece).is_err(),
        "le cas doit être impossible"
    );
    (p, s, piece)
}

// ---------------------------------------------------------------------------
// Lecteurs indépendants
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct SvgItem {
    pub role: Option<String>,
    pub dash: Option<String>,
    pub width: Option<String>,
    /// Commandes de `d` ou extrémités d'une ligne.
    pub d: Option<String>,
    pub pts: Vec<(f64, f64)>,
}

pub struct SvgDoc {
    pub width: f64,
    pub height: f64,
    pub items: Vec<SvgItem>,
}

pub fn attr(tag: &str, name: &str) -> Option<String> {
    let key = format!(" {name}=\"");
    let start = tag.find(&key)? + key.len();
    let end = start + tag[start..].find('"')?;
    Some(tag[start..end].to_string())
}

pub fn read_svg(svg: &str) -> SvgDoc {
    let mut doc = SvgDoc {
        width: 0.0,
        height: 0.0,
        items: Vec::new(),
    };
    let (mut g_dash, mut g_width): (Option<String>, Option<String>) = (None, None);
    for raw in svg.lines() {
        let l = raw.trim();
        if l.starts_with("<svg") {
            let dim = |n: &str| attr(l, n).unwrap().trim_end_matches("mm").parse().unwrap();
            doc.width = dim("width");
            doc.height = dim("height");
        } else if l.starts_with("<g ") {
            g_dash = attr(l, "stroke-dasharray");
            g_width = attr(l, "stroke-width");
        } else if l.starts_with("<path") || l.starts_with("<line") {
            let tag = if l.starts_with("<path") {
                "path"
            } else {
                "line"
            };
            let num = |n: &str| attr(l, n).unwrap().parse::<f64>().unwrap();
            let (d, pts) = if tag == "path" {
                let d = attr(l, "d").unwrap();
                let toks: Vec<&str> = d.split_whitespace().collect();
                let mut pts = Vec::new();
                let mut i = 0;
                while i < toks.len() {
                    match toks[i] {
                        "M" | "L" => {
                            pts.push((toks[i + 1].parse().unwrap(), toks[i + 2].parse().unwrap()));
                            i += 3;
                        }
                        "A" => {
                            pts.push((toks[i + 6].parse().unwrap(), toks[i + 7].parse().unwrap()));
                            i += 8;
                        }
                        "C" => {
                            pts.push((toks[i + 5].parse().unwrap(), toks[i + 6].parse().unwrap()));
                            i += 7;
                        }
                        _ => i += 1,
                    }
                }
                (Some(d), pts)
            } else {
                (None, vec![(num("x1"), num("y1")), (num("x2"), num("y2"))])
            };
            doc.items.push(SvgItem {
                role: attr(l, "data-role"),
                dash: attr(l, "stroke-dasharray").or_else(|| g_dash.clone()),
                width: attr(l, "stroke-width").or_else(|| g_width.clone()),
                d,
                pts,
            });
        }
    }
    doc
}

impl SvgDoc {
    pub fn with_role(&self, role: &str) -> Vec<&SvgItem> {
        self.items
            .iter()
            .filter(|i| i.role.as_deref() == Some(role))
            .collect()
    }
}

pub fn same_set(found: &[(f64, f64)], expected: &[(f64, f64)], tol: f64) -> bool {
    let has = |list: &[(f64, f64)], p: (f64, f64)| {
        list.iter()
            .any(|q| near(q.0, p.0, tol) && near(q.1, p.1, tol))
    };
    found.iter().all(|p| has(expected, *p)) && expected.iter().all(|p| has(found, *p))
}

#[derive(Debug, Clone)]
pub struct PdfStroke {
    /// Pointillé en points (vide = trait plein), épaisseur en points.
    pub dash_pt: Vec<f64>,
    pub width_pt: f64,
    /// Sommets (x, y) en points ; pour une Bézier, seulement le point d'arrivée.
    pub pts: Vec<(f64, f64)>,
    pub curves: usize,
    pub closed: bool,
}

/// Traits dessinés dans la fenêtre de patron de la première page de patron (flux de contenu en
/// lignes : `m`, `l`, `c`, `h`, `S` ; pointillé lu dans `[…] 0 d`).
pub fn read_pdf(bytes: &[u8]) -> Vec<PdfStroke> {
    let readable = super::pdf_text::readable(bytes);
    let text = readable.as_str();
    let mut rest = text;
    let mut out = Vec::new();
    while let Some(start) = rest.find("stream\n") {
        let body = &rest[start + 7..];
        let end = body.find("endstream").unwrap();
        let stream = &body[..end];
        rest = &body[end + 9..];
        if !stream.contains(" re W n") {
            continue;
        }
        let lines: Vec<&str> = stream.lines().collect();
        let from = lines.iter().position(|l| l.ends_with(" re W n")).unwrap();
        let to = lines[from..].iter().position(|l| *l == "Q").unwrap() + from;
        let (mut dash, mut width) = (Vec::new(), 0.0);
        for l in &lines[from + 1..to] {
            if l.contains(" w [") && l.ends_with(" 0 d") {
                let w: Vec<&str> = l.split_whitespace().collect();
                width = w[2].parse().unwrap();
                let a = l.find('[').unwrap() + 1;
                let b = l.find(']').unwrap();
                dash = l[a..b]
                    .split_whitespace()
                    .map(|v| v.parse().unwrap())
                    .collect();
            } else if l.ends_with(" S") && l.contains(" m ") {
                let t: Vec<&str> = l.split_whitespace().collect();
                let (mut pts, mut curves, mut i) = (Vec::new(), 0, 0);
                while i < t.len() {
                    match t[i] {
                        "m" | "l" => {
                            pts.push((t[i - 2].parse().unwrap(), t[i - 1].parse().unwrap()));
                        }
                        "c" => {
                            pts.push((t[i - 2].parse().unwrap(), t[i - 1].parse().unwrap()));
                            curves += 1;
                        }
                        _ => {}
                    }
                    i += 1;
                }
                out.push(PdfStroke {
                    dash_pt: dash.clone(),
                    width_pt: width,
                    pts,
                    curves,
                    closed: l.ends_with(" h S"),
                });
            }
        }
        break;
    }
    out
}

/// Point en mm → points PDF, formule écrite en dur : 72 / 25,4.
pub fn mm_pt(v: f64) -> f64 {
    v * 72.0 / 25.4
}

pub fn stroke_len_pt(s: &PdfStroke) -> f64 {
    let mut t: f64 = s
        .pts
        .windows(2)
        .map(|w| (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1))
        .sum();
    if s.closed {
        t += (s.pts[0].0 - s.pts[s.pts.len() - 1].0).hypot(s.pts[0].1 - s.pts[s.pts.len() - 1].1);
    }
    t
}
