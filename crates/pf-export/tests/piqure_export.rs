//! US-06.9 — les trous des lignes de piqûre sont exportés comme marquage distinct : groupe
//! `trous-de-piqure` du SVG, tracés vectoriels du PDF (et légende seulement quand il y en a),
//! couche 13 du DXF (cercle exact pour un trou rond, polyligne fermée sinon), plume 6 du HPGL.
//! Valeurs calculées à la main ; lecteurs DXF et HPGL indépendants de l'écrivain.

mod common;

use common::hpgl::read_hpgl;
use pf_core::{
    Command, PieceId, PieceSource, PointRef, Project, SheetId, StitchHole, StitchLineSpec,
};
use pf_export::dxf::export_dxf;
use pf_export::hpgl::export_hpgl;
use pf_export::pdf::export_pdf;
use pf_export::svg::export_svg;
use pf_export::{PageSetup, Scene};
use pf_geometry::PointMm;

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

/// Rectangle 100 × 70 ; bas : 3 trous ronds Ø 1,2 à y = 3 (x = 0, 50, 100) ; haut : 2 losanges
/// 3 × 2 à y = 67 (x = 100 et 0), bord orienté vers −X (tangente 180°).
fn project() -> (Project, SheetId, PieceId) {
    let mut p = Project::new("Piqure export");
    let s = p.sheets[0].id;
    p.apply(Command::AddRectangle {
        sheet: s,
        corner: PointRef::New(pt(0.0, 0.0)),
        width_mm: 100.0,
        height_mm: 70.0,
    })
    .unwrap();
    let piece = p
        .apply(Command::ExtractPiece {
            sheet: s,
            source: PieceSource::Click { at: pt(50.0, 35.0) },
            name: "Fond".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    let sheet = p.sheet(s).unwrap();
    let edge = |a: PointMm, b: PointMm| {
        sheet
            .entities
            .iter()
            .find(|e| {
                sheet.segment_ends(e.id).is_ok_and(|(x, y)| {
                    ((x.x - a.x).abs() < 1e-9
                        && (x.y - a.y).abs() < 1e-9
                        && (y.x - b.x).abs() < 1e-9
                        && (y.y - b.y).abs() < 1e-9)
                        || ((x.x - b.x).abs() < 1e-9
                            && (x.y - b.y).abs() < 1e-9
                            && (y.x - a.x).abs() < 1e-9
                            && (y.y - a.y).abs() < 1e-9)
                })
            })
            .unwrap()
            .id
    };
    let (bottom, top) = (
        edge(pt(0.0, 0.0), pt(100.0, 0.0)),
        edge(pt(100.0, 70.0), pt(0.0, 70.0)),
    );
    let spec = |e, hole, n| StitchLineSpec {
        edges: vec![e],
        hole,
        pitch_mm: 4.0,
        setback_mm: 3.0,
        count: Some(n),
        continues: None,
    };
    p.apply(Command::AddStitchLine {
        piece,
        line: spec(bottom, StitchHole::Round { diameter_mm: 1.2 }, 3),
    })
    .unwrap();
    p.apply(Command::AddStitchLine {
        piece,
        line: spec(
            top,
            StitchHole::French {
                length_mm: 3.0,
                width_mm: 2.0,
            },
            2,
        ),
    })
    .unwrap();
    (p, s, piece)
}

fn attr_str<'a>(tag: &'a str, name: &str) -> &'a str {
    let key = format!(" {name}=\"");
    let start = tag
        .find(&key)
        .unwrap_or_else(|| panic!("{name} absent de {tag}"))
        + key.len();
    &tag[start..start + tag[start..].find('"').unwrap()]
}

fn attr(tag: &str, name: &str) -> f64 {
    attr_str(tag, name).parse().unwrap()
}

#[test]
fn la_scene_porte_cinq_trous() {
    let (p, s, _) = project();
    let sc = Scene::from_sheet(&p, s).unwrap();
    assert_eq!(sc.stitch_holes.len(), 5);
    assert!(sc.markings.is_empty(), "distinct des marquages");
    let want = [
        (0.0, 3.0),
        (50.0, 3.0),
        (100.0, 3.0),
        (100.0, 67.0),
        (0.0, 67.0),
    ];
    for (h, (x, y)) in sc.stitch_holes.iter().zip(want) {
        assert!(
            (h.pos.x - x).abs() < 1e-9 && (h.pos.y - y).abs() < 1e-9,
            "{h:?}"
        );
    }
    assert!((sc.stitch_holes[3].tangent_deg - 180.0).abs() < 1e-9);
    assert_eq!(sc.stitch_holes[0].form_id(), "rond");
    assert_eq!(sc.stitch_holes[3].form_id(), "francais");
}

#[test]
fn svg_groupe_distinct_cercles_et_losanges() {
    let (p, s, _) = project();
    let sc = Scene::from_sheet(&p, s).unwrap();
    let svg = export_svg(&sc).unwrap();
    let bb = sc.bbox().unwrap();
    let (m, ymax) = (pf_export::svg::SVG_MARGIN_MM, bb.max.y);
    let x = |v: f64| v - bb.min.x + m;
    let y = |v: f64| ymax - v + m;
    let (_, after) = svg.split_once("data-role=\"piqure\"").expect("groupe");
    let group = after.split("</g>").next().unwrap();
    let circles: Vec<&str> = group.lines().filter(|l| l.contains("<circle")).collect();
    assert_eq!(circles.len(), 3, "{group}");
    for (tag, cx) in circles.iter().zip([0.0, 50.0, 100.0]) {
        assert!((attr(tag, "data-diameter-mm") - 1.2).abs() < 1e-9);
        assert!((attr(tag, "r") - 0.6).abs() < 1e-9);
        assert!((attr(tag, "cx") - x(cx)).abs() < 1e-2, "{tag}");
        assert!((attr(tag, "cy") - y(3.0)).abs() < 1e-2, "{tag}");
    }
    let paths: Vec<&str> = group
        .lines()
        .filter(|l| l.contains("data-form=\"francais\""))
        .collect();
    assert_eq!(paths.len(), 2);
    // Losange centré en (100 ; 67), tangente 180° : (98,5 ; 67) (100 ; 66) (101,5 ; 67) (100 ; 68).
    let t: Vec<f64> = attr_str(paths[0], "d")
        .split_whitespace()
        .filter_map(|v| v.parse().ok())
        .collect();
    let want = [(98.5, 67.0), (100.0, 66.0), (101.5, 67.0), (100.0, 68.0)];
    assert_eq!(t.len(), 8);
    for (k, (wx, wy)) in want.iter().enumerate() {
        assert!((t[2 * k] - x(*wx)).abs() < 1e-2 && (t[2 * k + 1] - y(*wy)).abs() < 1e-2);
    }
    assert!((attr(paths[0], "data-angle-deg") - 180.0).abs() < 1e-9);
}

#[test]
fn sans_ligne_aucun_groupe_ni_legende() {
    let (mut p, s, piece) = project();
    let ids: Vec<_> = p
        .piece(piece)
        .unwrap()
        .stitch_lines
        .iter()
        .map(|l| l.id)
        .collect();
    for line in ids {
        p.apply(Command::RemoveStitchLine { piece, line }).unwrap();
    }
    let sc = Scene::from_sheet(&p, s).unwrap();
    assert!(sc.stitch_holes.is_empty());
    assert!(!export_svg(&sc).unwrap().contains("piqure"));
    let pdf = export_pdf(std::slice::from_ref(&sc)).unwrap();
    assert!(!common::pdf_text::readable(&pdf.bytes).contains("piqure"));
}

/// Chemins `m l ...` d'un flux de page : sommets en mm.
fn closed_paths(stream: &str) -> Vec<Vec<(f64, f64)>> {
    let mm = |v: f64| v / pf_export::MM_TO_PT;
    let mut out: Vec<Vec<(f64, f64)>> = Vec::new();
    let mut cur: Option<Vec<(f64, f64)>> = None;
    let mut stack: Vec<f64> = Vec::new();
    let mut cleaned = String::new();
    let mut chars = stream.chars();
    while let Some(c) = chars.next() {
        match c {
            '(' => {
                for d in chars.by_ref() {
                    if d == ')' {
                        break;
                    }
                }
            }
            '[' => {
                for d in chars.by_ref() {
                    if d == ']' {
                        break;
                    }
                }
            }
            _ => cleaned.push(c),
        }
    }
    for tok in cleaned.split_whitespace() {
        if let Ok(v) = tok.parse::<f64>() {
            stack.push(v);
            continue;
        }
        match tok {
            "m" => {
                let n = stack.len();
                cur = Some(vec![(mm(stack[n - 2]), mm(stack[n - 1]))]);
            }
            "l" => {
                if let Some(path) = cur.as_mut() {
                    let n = stack.len();
                    path.push((mm(stack[n - 2]), mm(stack[n - 1])));
                }
            }
            "S" | "s" | "f" => {
                if let Some(path) = cur.take() {
                    out.push(path);
                }
            }
            _ => {}
        }
        stack.clear();
    }
    out
}

fn lozenges_in(bytes: &[u8]) -> usize {
    let text = common::pdf_text::readable(bytes);
    let mut found = 0;
    let mut rest = text.as_str();
    while let Some(start) = rest.find("stream\n") {
        let body = &rest[start + 7..];
        let end = body.find("endstream").unwrap();
        for path in closed_paths(&body[..end]) {
            // Losange 3 x 2 : quatre sommets, aire d1 * d2 / 2 = 3 mm2 (formule des lacets).
            if path.len() >= 4 {
                let v = &path[..4];
                let a: f64 = (0..4)
                    .map(|i| v[i].0 * v[(i + 1) % 4].1 - v[(i + 1) % 4].0 * v[i].1)
                    .sum::<f64>()
                    / 2.0;
                if (a.abs() - 3.0).abs() < 1e-2 {
                    found += 1;
                }
            }
        }
        rest = &body[end + 9..];
    }
    found
}

#[test]
fn pdf_legende_et_traces_vectoriels() {
    let (p, s, _) = project();
    let sc = Scene::from_sheet(&p, s).unwrap();
    let with = export_pdf(std::slice::from_ref(&sc)).unwrap();
    let text = common::pdf_text::readable(&with.bytes);
    assert!(text.contains("Trou de piqure"), "legende");
    assert_eq!(lozenges_in(&with.bytes), 2, "deux losanges de 3 mm2");
    let mut none = sc.clone();
    none.stitch_holes.clear();
    let without = export_pdf(std::slice::from_ref(&none)).unwrap();
    assert_eq!(lozenges_in(&without.bytes), 0);
    assert!(!common::pdf_text::readable(&without.bytes).contains("piqure"));
}

#[test]
fn dxf_couche_des_percages_cercles_et_polylignes_fermees() {
    let (p, _, _) = project();
    let out = export_dxf(&p).unwrap();
    let dxf = common::dxf::parse(&out.text).unwrap();
    let inserted = dxf.inserted().unwrap();
    let ents = inserted[0].1;
    let layer13: Vec<&common::dxf::Ent> = ents.iter().filter(|e| e.layer() == "13").collect();
    let circles: Vec<_> = layer13
        .iter()
        .filter_map(|e| match e {
            common::dxf::Ent::Circle { c, r, .. } => Some((*c, *r)),
            _ => None,
        })
        .collect();
    let polys: Vec<_> = layer13
        .iter()
        .filter_map(|e| match e {
            common::dxf::Ent::Polyline {
                pts, closed: true, ..
            } => Some(pts.clone()),
            _ => None,
        })
        .collect();
    assert_eq!((circles.len(), polys.len()), (3, 2));
    // Origine du contour = coin (0 ; 0) : plus petit x / y des polylignes du contour (couche 1).
    let boundary: Vec<_> = ents
        .iter()
        .filter(|e| e.layer() == "1")
        .flat_map(|e| match e {
            common::dxf::Ent::Polyline { pts, .. } => pts.clone(),
            common::dxf::Ent::Line { a, b, .. } => vec![*a, *b],
            _ => vec![],
        })
        .collect();
    let (ox, oy) = common::dxf::bbox(&boundary).0;
    for ((c, r), x) in circles.iter().zip([0.0, 50.0, 100.0]) {
        assert!((r - 0.6).abs() < 1e-9);
        assert!(
            (c.0 - ox - x).abs() < 1e-3 && (c.1 - oy - 3.0).abs() < 1e-3,
            "{c:?}"
        );
    }
    // Losange : aire = d1 · d2 / 2 = 3 · 2 / 2 = 3 mm², centroïde (100 ; 67).
    for poly in &polys {
        assert!((common::dxf::area(poly).abs() - 3.0).abs() < 1e-3);
    }
    let cx: f64 = polys[0].iter().map(|q| q.0).sum::<f64>() / 4.0 - ox;
    assert!((cx - 100.0).abs() < 1e-3, "{cx}");
}

#[test]
fn hpgl_plume_6_cercles_et_losanges() {
    let (p, s, _) = project();
    let sc = Scene::from_sheet(&p, s).unwrap();
    let plot = export_hpgl(&sc, &PageSetup::default()).unwrap();
    let r = read_hpgl(&plot.text).unwrap();
    let strokes: Vec<_> = r.strokes.iter().filter(|k| k.pen == 6).collect();
    assert_eq!(strokes.len(), 5);
    // Périmètre d'un losange 3 × 2 : 4 · √(1,5² + 1²) = 7,2111… mm ; cercle Ø 1,2 : π · 1,2.
    let lozenge = 4.0 * (1.5f64 * 1.5 + 1.0).sqrt();
    let circle = std::f64::consts::PI * 1.2;
    let near = |a: f64, b: f64| (a - b).abs() < 0.05;
    assert_eq!(
        strokes
            .iter()
            .filter(|k| near(k.length_mm(), circle))
            .count(),
        3
    );
    assert_eq!(
        strokes
            .iter()
            .filter(|k| near(k.length_mm(), lozenge))
            .count(),
        2
    );
}
