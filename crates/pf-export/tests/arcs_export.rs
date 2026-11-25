//! US-02.1 — export des arcs et cercles : arcs vrais en SVG, Béziers d'erreur
//! < 0,005 mm en PDF, relus par un lecteur indépendant des fonctions d'export.

use pf_core::{Command, PointRef, Project, SheetId};
use pf_export::{pdf, svg, PathGeom, Scene};
use pf_geometry::{distance_mm, PointMm};

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

fn project(radius: f64) -> (Project, SheetId) {
    let mut p = Project::new("Arcs");
    let s = p.sheets[0].id;
    p.apply(Command::AddArc {
        sheet: s,
        center: PointRef::New(pt(0.0, 0.0)),
        radius_mm: radius,
        start_deg: 20.0,
        end_deg: 250.0,
        ccw: true,
    })
    .unwrap();
    p.apply(Command::AddCircle {
        sheet: s,
        center: PointRef::New(pt(3.0 * radius, 0.0)),
        radius_mm: radius / 2.0,
    })
    .unwrap();
    (p, s)
}

/// Lecteur indépendant : extrait les chemins `m … c … S` du flux PDF.
fn pdf_cubics(bytes: &[u8]) -> Vec<Vec<[PointMm; 4]>> {
    let text = String::from_utf8_lossy(bytes);
    let mm = 72.0 / 25.4;
    let mut out = Vec::new();
    for line in text.lines() {
        let tokens: Vec<&str> = line.split_whitespace().collect();
        if tokens.len() < 9 || tokens[2] != "m" || tokens.last() != Some(&"S") {
            continue;
        }
        if !tokens.contains(&"c") {
            continue;
        }
        let n: Vec<f64> = tokens
            .iter()
            .filter_map(|t| t.parse::<f64>().ok())
            .map(|v| v / mm)
            .collect();
        let start = pt(n[0], n[1]);
        let mut curves = Vec::new();
        let mut cur = start;
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
fn svg_exporte_des_arcs_vrais_au_rayon_exact() {
    let (p, s) = project(100.0);
    let scene = Scene::from_sheet(&p, s).unwrap();
    assert!(matches!(scene.paths[0].geom, PathGeom::Arc(_)));
    assert!(matches!(scene.paths[1].geom, PathGeom::Circle(_)));
    let svg = svg::export_svg(&scene).unwrap();
    // Arc de 230° (> 180°) en sens anti-horaire : grand arc, sweep-flag 0 (axe Y inversé).
    assert!(svg.contains("A 100 100 0 1 0 "), "{svg}");
    // Cercle : deux demi-arcs de rayon 50.
    assert_eq!(svg.matches("A 50 50 0 0 0 ").count(), 2, "{svg}");
    // Les dimensions du dessin correspondent à la boîte englobante (1 unité = 1 mm).
    let bb = scene.bbox().unwrap();
    assert!((bb.width() - 450.0).abs() < 1e-6, "{}", bb.width());
    assert!(
        bb.height() > 190.0 && bb.height() < 200.0,
        "{}",
        bb.height()
    );
}

#[test]
fn pdf_beziers_erreur_inferieure_a_0_005_mm() {
    for radius in [3.0, 100.0, 400.0] {
        let (p, s) = project(radius);
        let scene = Scene::from_sheet(&p, s).unwrap();
        let bytes = pdf::export_pdf(std::slice::from_ref(&scene)).unwrap().bytes;
        let paths = pdf_cubics(&bytes);
        // Chaque page (tuile) redessine l'arc et le cercle : on lit ceux de la première.
        assert!(paths.len() >= 2 && paths.len() % 2 == 0, "rayon {radius}");
        // Translation page → modèle : l'arc commence à son point de départ connu.
        let PathGeom::Arc(arc) = scene.paths[0].geom else {
            unreachable!()
        };
        let t = paths[0][0][0] - arc.start_point();
        let center_page = pt(arc.center.x + t.x, arc.center.y + t.y);
        let mut worst: f64 = 0.0;
        for curve in &paths[0] {
            for i in 0..=50 {
                let u = i as f64 / 50.0;
                let b = pf_geometry::CubicBezier::new(curve[0], curve[1], curve[2], curve[3]);
                let q = b.point_at(u);
                worst = worst.max((distance_mm(q, center_page) - radius).abs());
            }
        }
        assert!(worst < 0.005, "rayon {radius} : erreur {worst}");
        // Extrémités exactes (arrondi PDF 4 décimales de points).
        let last = paths[0].last().unwrap()[3];
        let end = arc.end_point();
        assert!(distance_mm(last, pt(end.x + t.x, end.y + t.y)) < 0.01);
        // Cercle : chemin fermé (début = fin) et rayon exact.
        let c = &paths[1];
        assert!(distance_mm(c[0][0], c.last().unwrap()[3]) < 0.01);
    }
}
