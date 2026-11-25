//! US-02.2 — export des courbes par points de passage : un seul chemin de
//! Béziers cubiques en SVG et en PDF, écart < 0,005 mm avec la courbe du modèle.

use pf_core::{Command, EntityId, OffsetMm, PathRole, PointRef, Project, SheetId};
use pf_export::{pdf, svg, PathGeom, Scene};
use pf_geometry::{distance_mm, CubicBezier, PointMm};

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

fn project() -> (Project, SheetId, EntityId) {
    let mut p = Project::new("Courbes");
    let s = p.sheets[0].id;
    let e = p
        .apply(Command::AddSpline {
            sheet: s,
            points: [(0.0, 0.0), (40.0, 30.0), (90.0, -10.0), (150.0, 20.0)]
                .iter()
                .map(|&(x, y)| PointRef::New(pt(x, y)))
                .collect(),
        })
        .unwrap()
        .entities[0];
    p.apply(Command::SetSplineNode {
        sheet: s,
        spline: e,
        index: 2,
        corner: true,
        tangent: Some(OffsetMm { dx: 30.0, dy: 20.0 }),
        tangent_in: Some(OffsetMm {
            dx: 10.0,
            dy: -25.0,
        }),
    })
    .unwrap();
    p.apply(Command::SetPathRole {
        sheet: s,
        entity: e,
        role: PathRole::SewingContour,
    })
    .unwrap();
    (p, s, e)
}

/// Lecteur indépendant des fonctions d'export : chemins `m … c … S` du PDF.
fn pdf_cubics(bytes: &[u8]) -> Vec<Vec<[PointMm; 4]>> {
    let text = String::from_utf8_lossy(bytes);
    let mm = 72.0 / 25.4;
    let mut out = Vec::new();
    for line in text.lines() {
        let tokens: Vec<&str> = line.split_whitespace().collect();
        if tokens.len() < 9 || tokens[2] != "m" || tokens.last() != Some(&"S") {
            continue;
        }
        let n: Vec<f64> = tokens
            .iter()
            .filter_map(|t| t.parse::<f64>().ok())
            .map(|v| v / mm)
            .collect();
        let mut cur = pt(n[0], n[1]);
        let mut curves = Vec::new();
        for chunk in n[2..].chunks(6) {
            let next = pt(chunk[4], chunk[5]);
            curves.push([cur, pt(chunk[0], chunk[1]), pt(chunk[2], chunk[3]), next]);
            cur = next;
        }
        out.push(curves);
    }
    out
}

#[test]
fn svg_un_seul_chemin_avec_un_c_par_troncon() {
    let (p, s, _) = project();
    let scene = Scene::from_sheet(&p, s).unwrap();
    let PathGeom::Spline(segments) = &scene.paths[0].geom else {
        panic!("la scène doit porter la courbe comme un seul chemin");
    };
    assert_eq!(segments.len(), 3);
    let text = svg::export_svg(&scene).unwrap();
    // Un seul <path> de courbe (les pointillés du tracé ne repartent pas à chaque tronçon).
    assert_eq!(text.matches("<path").count(), 1, "{text}");
    assert_eq!(text.matches(" C ").count(), 3, "{text}");
    assert!(text.contains("stroke-dasharray"), "{text}");
    let bb = scene.bbox().unwrap();
    assert!(bb.min.x <= 0.0 && bb.max.x >= 150.0);
}

#[test]
fn pdf_beziers_identiques_a_la_courbe_du_modele() {
    let (p, s, e) = project();
    let scene = Scene::from_sheet(&p, s).unwrap();
    let model = p.sheet(s).unwrap().spline_geometry(e).unwrap();
    let bytes = pdf::export_pdf(std::slice::from_ref(&scene)).unwrap().bytes;
    let paths = pdf_cubics(&bytes);
    assert!(!paths.is_empty());
    let drawn = &paths[0];
    assert_eq!(drawn.len(), 3);
    // Translation page → modèle : le départ de la courbe.
    let t = drawn[0][0] - model.start_point();
    let mut worst: f64 = 0.0;
    for (curve, seg) in drawn.iter().zip(model.segments()) {
        let b = CubicBezier::new(curve[0], curve[1], curve[2], curve[3]);
        for i in 0..=50 {
            let u = i as f64 / 50.0;
            let q = seg.point_at(u);
            worst = worst.max(distance_mm(b.point_at(u), pt(q.x + t.x, q.y + t.y)));
        }
    }
    assert!(worst < 0.005, "écart {worst} mm");
}
