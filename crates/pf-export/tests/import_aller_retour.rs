//! US-12.4 - Aller-retour avec l'export existant : tout SVG produit par `pf-export` se
//! reimporte avec les memes longueurs a 0,01 mm pres.
//!
//! Deux oracles independants du lecteur teste :
//! - **texte** : un mini-lecteur ecrit ici (grammaire de l'ecrivain seulement : `M L C A Z`
//!   absolus, `line`, `circle`) somme les longueurs lues dans le SVG ; l'arc est mesure par sa
//!   corde et son rayon, avec la regle d'arrondi declaree (demi-cercle a 2e-4 pres) ;
//! - **modele** : pour le groupe des traces de la `Scene`, la longueur calculee par
//!   `pf-geometry` (segments, arcs, cercles, Beziers, courbes lisses).

use pf_core::{persist, Command, EntityKind, ImportItem, PointRef, Project};
use pf_export::pdf_import::PdfDocument;
use pf_export::svg_import::{SvgDocument, SvgUnitChoice};
use pf_export::vector_import::VectorBuild;
use pf_export::{grain_arrow_marks, svg, PathGeom, Scene};
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples")
}

fn item_len(i: &ImportItem) -> f64 {
    match i {
        ImportItem::Segment { from, to } => (to.x - from.x).hypot(to.y - from.y),
        ImportItem::Circle { radius_mm, .. } => std::f64::consts::TAU * radius_mm,
        ImportItem::Arc {
            center,
            start,
            end,
            ccw,
        } => {
            let r = (start.x - center.x).hypot(start.y - center.y);
            let (a0, a1) = (
                (start.y - center.y).atan2(start.x - center.x),
                (end.y - center.y).atan2(end.x - center.x),
            );
            let t = std::f64::consts::TAU;
            r * if *ccw {
                (a1 - a0).rem_euclid(t)
            } else {
                (a0 - a1).rem_euclid(t)
            }
        }
        ImportItem::Bezier { start, c1, c2, end } => {
            let at = |t: f64| {
                let u = 1.0 - t;
                let f = |a: f64, b: f64, c: f64, d: f64| {
                    u * u * u * a + 3.0 * u * u * t * b + 3.0 * u * t * t * c + t * t * t * d
                };
                (f(start.x, c1.x, c2.x, end.x), f(start.y, c1.y, c2.y, end.y))
            };
            (0..2000)
                .map(|k| {
                    let (a, b) = (at(k as f64 / 2000.0), at((k + 1) as f64 / 2000.0));
                    (b.0 - a.0).hypot(b.1 - a.1)
                })
                .sum()
        }
        other => panic!("{other:?}"),
    }
}

fn imported_length(b: &VectorBuild) -> f64 {
    b.objects.iter().map(|o| item_len(&o.item)).sum()
}

// ------------------------------------------------------------------ oracle texte

fn attr(tag: &str, key: &str) -> Option<f64> {
    let pat = format!(" {key}=\"");
    let i = tag.find(&pat)? + pat.len();
    tag[i..][..tag[i..].find('"')?].parse().ok()
}

fn d_attr(tag: &str) -> Option<&str> {
    let i = tag.find(" d=\"")? + 4;
    Some(&tag[i..][..tag[i..].find('"')?])
}

fn arc_len(p0: (f64, f64), p1: (f64, f64), r: f64, large: bool) -> f64 {
    let c = (p1.0 - p0.0).hypot(p1.1 - p0.1);
    if (c / 2.0 - r).abs() <= 2e-4 {
        return std::f64::consts::PI * c / 2.0;
    }
    let theta = 2.0 * (c / (2.0 * r)).clamp(-1.0, 1.0).asin();
    r * if large {
        std::f64::consts::TAU - theta
    } else {
        theta
    }
}

fn text_length(svg: &str) -> f64 {
    let mut total = 0.0;
    for tag in svg.split('<').skip(1) {
        let tag = tag.split('>').next().unwrap_or("");
        if tag.starts_with("line ") {
            let g = |k| attr(tag, k).unwrap_or(0.0);
            total += (g("x2") - g("x1")).hypot(g("y2") - g("y1"));
        } else if tag.starts_with("circle ") {
            total += std::f64::consts::TAU * attr(tag, "r").unwrap();
        } else if tag.starts_with("path ") {
            let toks: Vec<&str> = d_attr(tag).unwrap().split_whitespace().collect();
            let (mut i, mut cur, mut start) = (0, (0.0, 0.0), (0.0, 0.0));
            let n = |s: &str| s.parse::<f64>().unwrap();
            while i < toks.len() {
                match toks[i] {
                    "M" => {
                        cur = (n(toks[i + 1]), n(toks[i + 2]));
                        start = cur;
                        i += 3;
                    }
                    "L" => {
                        let p = (n(toks[i + 1]), n(toks[i + 2]));
                        total += (p.0 - cur.0).hypot(p.1 - cur.1);
                        cur = p;
                        i += 3;
                    }
                    "C" => {
                        let v: Vec<f64> = toks[i + 1..i + 7].iter().map(|s| n(s)).collect();
                        let imp = ImportItem::Bezier {
                            start: pf_geometry::PointMm::new(cur.0, cur.1),
                            c1: pf_geometry::PointMm::new(v[0], v[1]),
                            c2: pf_geometry::PointMm::new(v[2], v[3]),
                            end: pf_geometry::PointMm::new(v[4], v[5]),
                        };
                        total += item_len(&imp);
                        cur = (v[4], v[5]);
                        i += 7;
                    }
                    "A" => {
                        let p = (n(toks[i + 6]), n(toks[i + 7]));
                        total += arc_len(cur, p, n(toks[i + 1]), toks[i + 4] == "1");
                        cur = p;
                        i += 8;
                    }
                    "Z" => {
                        total += (start.0 - cur.0).hypot(start.1 - cur.1);
                        cur = start;
                        i += 1;
                    }
                    other => panic!("commande inattendue {other}"),
                }
            }
        }
    }
    total
}

fn import_text(svg: &str) -> VectorBuild {
    SvgDocument::parse(svg.as_bytes())
        .unwrap()
        .build_choice(SvgUnitChoice::Declared, 1.0)
        .unwrap()
}

// ------------------------------------------------------------------ oracle modele

fn path_len(p: &pf_export::ScenePath) -> f64 {
    let mut l = match &p.geom {
        PathGeom::Line(a, b) => pf_geometry::distance_mm(*a, *b),
        PathGeom::Cubic(c) => c.length_mm(1e-6).unwrap(),
        PathGeom::Arc(a) => a.length_mm(),
        PathGeom::Circle(c) => std::f64::consts::TAU * c.circle.radius_mm,
        PathGeom::Spline(s) => s.iter().map(|c| c.length_mm(1e-6).unwrap()).sum(),
    };
    if let (pf_core::PathRole::Grainline, PathGeom::Line(a, b)) = (p.role, &p.geom) {
        l += grain_arrow_marks(*a, *b, p.sense)
            .iter()
            .map(|(p, q)| pf_geometry::distance_mm(*p, *q))
            .sum::<f64>();
    }
    l
}

/// Le SVG reduit a son premier groupe (les traces de la scene) : en-tete + groupe + fermeture.
fn first_group(svg: &str) -> String {
    let g = svg.find("  <g ").unwrap();
    let end = svg[g..].find("  </g>\n").unwrap() + g + 7;
    format!("{}{}</svg>\n", &svg[..g], &svg[g..end])
}

fn example_projects() -> Vec<(String, Project)> {
    let mut out = Vec::new();
    for rel in [
        "geometrie-exercice.pforge.json",
        "grande-piece-600x450.pforge.json",
        "jupe-droite.pforge.json",
        "piece-crans-droit-fil.pforge.json",
        "porte-cartes.pforge.json",
        "aide/sac-cabas.pforge.json",
    ] {
        out.push((rel.to_string(), persist::load(&root().join(rel)).unwrap()));
    }
    out
}

#[test]
fn les_svg_des_exemples_se_reimportent_aux_memes_longueurs() {
    let mut files = 0;
    for name in [
        "etalon-100mm.svg",
        "geometrie-exercice.svg",
        "jupe-droite.svg",
        "piece-crans-droit-fil.svg",
        "porte-cartes.svg",
    ] {
        let text = std::fs::read_to_string(root().join(name)).unwrap();
        let b = import_text(&text);
        let (got, want) = (imported_length(&b), text_length(&text));
        assert!((got - want).abs() <= 0.01, "{name}: {got} contre {want}");
        // `<text>` (legendes) : ignore et liste, jamais en silence.
        assert_eq!(
            b.report.read,
            b.report.imported_total() + b.report.ignored_total(),
            "{name}"
        );
        files += 1;
    }
    assert_eq!(files, 5);
}

#[test]
fn toutes_les_feuilles_des_projets_d_exemple_aller_retour_scene_svg_import() {
    let (mut sheets, mut arcs, mut circles, mut curves, mut splines) = (0, 0, 0, 0, 0);
    for (name, project) in example_projects() {
        for s in &project.sheets {
            let Ok(scene) = Scene::from_sheet(&project, s.id) else {
                continue;
            };
            let Ok(text) = svg::export_svg(&scene) else {
                continue;
            };
            // Texte : tout le fichier.
            let b = import_text(&text);
            assert!(
                (imported_length(&b) - text_length(&text)).abs() <= 0.01,
                "{name}/{}",
                s.name
            );
            // Modele : le groupe des traces.
            let g = import_text(&first_group(&text));
            let truth: f64 = scene.paths.iter().map(path_len).sum();
            assert!(
                (imported_length(&g) - truth).abs() <= 0.01,
                "{name}/{} : {} contre {truth}",
                s.name,
                imported_length(&g)
            );
            for p in &scene.paths {
                match p.geom {
                    PathGeom::Arc(_) => arcs += 1,
                    PathGeom::Circle(_) => circles += 1,
                    PathGeom::Cubic(_) => curves += 1,
                    PathGeom::Spline(_) => splines += 1,
                    PathGeom::Line(..) => {}
                }
            }
            sheets += 1;
        }
    }
    // Couverture : ces exemples contiennent des arcs et des Beziers ; cercles et courbes lisses
    // sont couverts par le projet fabrique ci-dessous.
    assert!(sheets >= 6, "{sheets} feuilles");
    assert!(
        arcs > 0 && curves > 0,
        "{arcs} {circles} {curves} {splines}"
    );
}

#[test]
fn les_pdf_des_exemples_s_importent_comme_chemins() {
    for name in [
        "etalon-100mm.pdf",
        "geometrie-exercice.pdf",
        "jupe-droite.pdf",
        "piece-crans-droit-fil.pdf",
        "porte-cartes.pdf",
        "grande-piece-600x450.pdf",
    ] {
        let bytes = std::fs::read(root().join(name)).unwrap();
        let d = PdfDocument::parse(&bytes).unwrap();
        let b = d.build(1, 1.0).unwrap();
        assert!(b.report.objects > 0, "{name}");
        // Le texte de la legende est liste, pas importe.
        assert!(b.report.ignored_of("text") > 0, "{name}");
        assert_eq!(
            b.report.read,
            b.report.imported_total() + b.report.ignored_total()
        );
    }
}

#[test]
fn le_carre_etalon_du_pdf_fait_100_mm() {
    let bytes = std::fs::read(root().join("etalon-100mm.pdf")).unwrap();
    let b = PdfDocument::parse(&bytes).unwrap().build(1, 1.0).unwrap();
    let longest = b
        .objects
        .iter()
        .filter_map(|o| match &o.item {
            ImportItem::Segment { from, to } => Some((to.x - from.x).hypot(to.y - from.y)),
            _ => None,
        })
        .fold(0.0, f64::max);
    assert!((longest - 100.0).abs() <= 0.01, "{longest}");
}

/// Projet fabrique : arcs (dont quart et demi-cercle), cercle, Bezier, courbe lisse, segment.
fn crafted() -> (Project, pf_core::SheetId) {
    let pt = pf_geometry::PointMm::new;
    let mut p = Project::new("Fabrique");
    let s = p.sheets[0].id;
    for (c, r, a0, a1, ccw) in [
        ((0.0, 0.0), 40.0, 20.0, 250.0, true),
        ((120.0, 0.0), 30.0, 0.0, 90.0, true),
        ((200.0, 0.0), 25.0, 0.0, 180.0, false),
    ] {
        p.apply(Command::AddArc {
            sheet: s,
            center: PointRef::New(pt(c.0, c.1)),
            radius_mm: r,
            start_deg: a0,
            end_deg: a1,
            ccw,
        })
        .unwrap();
    }
    p.apply(Command::AddCircle {
        sheet: s,
        center: PointRef::New(pt(0.0, 100.0)),
        radius_mm: 12.5,
    })
    .unwrap();
    p.apply(Command::AddCurve {
        sheet: s,
        start: PointRef::New(pt(50.0, 100.0)),
        c1: pt(70.0, 140.0),
        c2: pt(90.0, 60.0),
        end: PointRef::New(pt(120.0, 100.0)),
    })
    .unwrap();
    p.apply(Command::AddSpline {
        sheet: s,
        points: [(0.0, 200.0), (40.0, 230.0), (90.0, 190.0), (150.0, 220.0)]
            .iter()
            .map(|&(x, y)| PointRef::New(pt(x, y)))
            .collect(),
    })
    .unwrap();
    p.apply(Command::AddSegment {
        sheet: s,
        from: PointRef::New(pt(0.0, 300.0)),
        to: PointRef::New(pt(30.0, 340.0)),
    })
    .unwrap();
    (p, s)
}

#[test]
fn projet_fabrique_arcs_cercle_bezier_spline_gardent_leurs_longueurs_et_leurs_types() {
    let (p, s) = crafted();
    let scene = Scene::from_sheet(&p, s).unwrap();
    let text = svg::export_svg(&scene).unwrap();
    let truth: f64 = scene.paths.iter().map(path_len).sum();
    let g = import_text(&first_group(&text));
    assert!(
        (imported_length(&g) - truth).abs() <= 0.01,
        "{} contre {truth}",
        imported_length(&g)
    );
    // Ce que l'import cree dans le modele garde les memes longueurs et les types du modele.
    let mut q = Project::new("Reimport");
    let qs = q.sheets[0].id;
    q.apply(Command::ImportGeometry {
        sheet: qs,
        objects: g.objects.clone(),
    })
    .unwrap();
    let kinds = |p: &Project, s| {
        let sh = p.sheet(s).unwrap();
        let n = |f: fn(&EntityKind) -> bool| sh.entities.iter().filter(|e| f(&e.kind)).count();
        (
            n(|k| matches!(k, EntityKind::Arc { .. })),
            n(|k| matches!(k, EntityKind::Circle { .. })),
            n(|k| matches!(k, EntityKind::Curve { .. })),
        )
    };
    let (arcs, circles, curves) = kinds(&q, qs);
    // Le cercle est ecrit par le SVG en deux demi-arcs : il revient en deux arcs (meme longueur).
    assert_eq!((arcs, circles), (3 + 2, 0));
    // 1 Bezier + la spline exportee en Beziers (3 troncons).
    assert_eq!(curves, 1 + 3);
    let again: f64 = Scene::from_sheet(&q, qs)
        .unwrap()
        .paths
        .iter()
        .map(path_len)
        .sum();
    assert!((again - truth).abs() <= 0.01, "{again} contre {truth}");
}
