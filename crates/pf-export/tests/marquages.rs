//! US-06.3 — les perçages, pointages et croix sont exportés comme **marquage distinct**
//! (groupe nommé et forme propre en SVG, formes vectorielles en mm → pt en PDF), jamais
//! reconnaissables par la couleur seule ; un cercle de diamètre D est exporté avec un
//! diamètre de D mm. Les tests relisent les fichiers produits avec un lecteur indépendant.

#[path = "common/pdf_text.rs"]
mod pdf_text;

use pf_core::{
    Command, MarkingAnchor, MarkingShape, PieceId, PieceSource, PointRef, Project, SheetId,
};
use pf_export::pdf::{export_pdf, plan_tiles};
use pf_export::svg::export_svg;
use pf_export::{Scene, MM_TO_PT};
use pf_geometry::PointMm;

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

/// Pièce 100 × 60 mm et quatre marquages : perçage Ø 6 en (50 ; 30), pointage en (20 ; 20),
/// croix de 10 mm en (80 ; 40), perçage Ø 3 au bout de pince reculé de 10 mm (16 ; 18).
fn project() -> (Project, SheetId, PieceId) {
    let mut p = Project::new("Marquages export");
    let s = p.sheets[0].id;
    p.apply(Command::AddRectangle {
        sheet: s,
        corner: PointRef::New(pt(0.0, 0.0)),
        width_mm: 100.0,
        height_mm: 60.0,
    })
    .unwrap();
    let piece = p
        .apply(Command::ExtractPiece {
            sheet: s,
            source: PieceSource::Click { at: pt(50.0, 30.0) },
            name: "Devant".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    let mut point = |x: f64, y: f64| {
        p.apply(Command::AddPoint {
            sheet: s,
            at: pt(x, y),
        })
        .unwrap()
        .points[0]
    };
    let (a, b, c) = (point(50.0, 30.0), point(20.0, 20.0), point(80.0, 40.0));
    let (tip, toward) = (point(10.0, 10.0), point(40.0, 50.0));
    let mut mark = |at: MarkingAnchor, shape: MarkingShape| {
        p.apply(Command::AddPieceMarking { piece, at, shape })
            .unwrap();
    };
    mark(
        MarkingAnchor::Point { point: a },
        MarkingShape::Drill { diameter_mm: 6.0 },
    );
    mark(MarkingAnchor::Point { point: b }, MarkingShape::Punch);
    mark(
        MarkingAnchor::Point { point: c },
        MarkingShape::Cross { size_mm: 10.0 },
    );
    mark(
        MarkingAnchor::DartTip {
            tip,
            toward,
            setback_mm: 10.0,
        },
        MarkingShape::Drill { diameter_mm: 3.0 },
    );
    (p, s, piece)
}

fn scene() -> Scene {
    let (p, s, _) = project();
    Scene::from_sheet(&p, s).unwrap()
}

/// Même projet sans aucun marquage.
fn scene_without() -> Scene {
    let (mut p, s, piece) = project();
    let ids: Vec<_> = p
        .piece(piece)
        .unwrap()
        .markings
        .iter()
        .map(|m| m.id)
        .collect();
    for marking in ids {
        p.apply(Command::RemovePieceMarking { piece, marking })
            .unwrap();
    }
    Scene::from_sheet(&p, s).unwrap()
}

fn attr(tag: &str, name: &str) -> f64 {
    let key = format!(" {name}=\"");
    let start = tag
        .find(&key)
        .unwrap_or_else(|| panic!("{name} absent de {tag}"))
        + key.len();
    let end = start + tag[start..].find('"').unwrap();
    tag[start..end].parse().unwrap()
}

/// Nombres d'un chemin SVG « M x y L x y M x y L x y ».
fn path_numbers(tag: &str) -> Vec<f64> {
    let key = " d=\"";
    let start = tag.find(key).unwrap() + key.len();
    let end = start + tag[start..].find('"').unwrap();
    tag[start..end]
        .split_whitespace()
        .filter_map(|t| t.parse::<f64>().ok())
        .collect()
}

#[test]
fn la_scene_porte_les_quatre_marquages_aux_positions_calculees() {
    let sc = scene();
    assert_eq!(sc.markings.len(), 4);
    let at = |i: usize| (sc.markings[i].pos.x, sc.markings[i].pos.y);
    assert_eq!(at(0), (50.0, 30.0));
    assert_eq!(at(1), (20.0, 20.0));
    assert_eq!(at(2), (80.0, 40.0));
    // Bout de pince : (10 ; 10) + 10 · (30 ; 40) / 50 = (16 ; 18).
    assert!((at(3).0 - 16.0).abs() < 1e-9 && (at(3).1 - 18.0).abs() < 1e-9);
    assert_eq!(
        sc.markings[0].shape,
        MarkingShape::Drill { diameter_mm: 6.0 }
    );
    // Dans le contour : l'emprise de la scène n'a pas changé.
    assert_eq!(sc.bbox(), scene_without().bbox());
}

#[test]
fn svg_groupe_nomme_et_formes_distinctes_diametre_mesure() {
    let sc = scene();
    let svg = export_svg(&sc).unwrap();
    let bb = sc.bbox().unwrap();
    let (m, ymax) = (pf_export::svg::SVG_MARGIN_MM, bb.max.y);
    let x = |v: f64| v - bb.min.x + m;
    let y = |v: f64| ymax - v + m;
    // Un groupe nommé, distinct de celui des crans et des cotations.
    let (_, after) = svg
        .split_once("data-role=\"marquage\"")
        .expect("groupe des marquages");
    assert!(svg.contains("<g id=\"marquages\""));
    let group = after.split("</g>").next().unwrap();
    assert!(group.contains("<title>"), "{group}");
    // Trois cercles (deux perçages, un pointage) et une croix.
    let circles: Vec<&str> = group.lines().filter(|l| l.contains("<circle")).collect();
    assert_eq!(circles.len(), 3, "{group}");
    let drills: Vec<&&str> = circles
        .iter()
        .filter(|l| l.contains("data-form=\"percage\""))
        .collect();
    assert_eq!(drills.len(), 2);
    // Perçage Ø 6 : rayon 3 mm (diamètre exporté = diamètre demandé), au bon endroit.
    let d6 = drills
        .iter()
        .find(|l| (attr(l, "data-diameter-mm") - 6.0).abs() < 1e-9)
        .expect("perçage de 6 mm");
    assert!((2.0 * attr(d6, "r") - 6.0).abs() < 1e-9);
    assert!((attr(d6, "cx") - x(50.0)).abs() < 1e-3 && (attr(d6, "cy") - y(30.0)).abs() < 1e-3);
    assert!(
        !d6.contains("fill=\"#000000\""),
        "un perçage n'est pas plein"
    );
    // Perçage Ø 3 au bout de pince reculé : centre (16 ; 18).
    let d3 = drills
        .iter()
        .find(|l| (attr(l, "data-diameter-mm") - 3.0).abs() < 1e-9)
        .expect("perçage de 3 mm");
    assert!((2.0 * attr(d3, "r") - 3.0).abs() < 1e-9);
    assert!((attr(d3, "cx") - x(16.0)).abs() < 1e-3 && (attr(d3, "cy") - y(18.0)).abs() < 1e-3);
    // Pointage : disque plein de 1 mm, forme propre.
    let punch = circles
        .iter()
        .find(|l| l.contains("data-form=\"pointage\""))
        .expect("pointage");
    assert!((2.0 * attr(punch, "r") - 1.0).abs() < 1e-9);
    assert!(punch.contains("fill=\"#000000\""));
    assert!((attr(punch, "cx") - x(20.0)).abs() < 1e-3);
    // Croix de 10 mm : deux branches de 10 mm, horizontale et verticale, centrées en (80 ; 40).
    let cross = group
        .lines()
        .find(|l| l.contains("data-form=\"croix\""))
        .expect("croix");
    assert!((attr(cross, "data-size-mm") - 10.0).abs() < 1e-9);
    let n = path_numbers(cross);
    assert_eq!(n.len(), 8, "{cross}");
    let (h, v) = (
        ((n[2] - n[0]).hypot(n[3] - n[1])),
        ((n[6] - n[4]).hypot(n[7] - n[5])),
    );
    assert!(
        (h - 10.0).abs() < 1e-3 && (v - 10.0).abs() < 1e-3,
        "{h} {v}"
    );
    assert!((n[1] - n[3]).abs() < 1e-9 && (n[4] - n[6]).abs() < 1e-9);
    assert!(((n[0] + n[2]) / 2.0 - x(80.0)).abs() < 1e-3);
    assert!(((n[5] + n[7]) / 2.0 - y(40.0)).abs() < 1e-3);
    // Le fichier garde la mention de l'échelle et reste distinct des autres groupes.
    assert!(svg.contains("1 unité = 1 mm"));
    assert_eq!(svg.matches("data-role=\"marquage\"").count(), 1);
}

#[test]
fn svg_sans_marquage_aucune_trace_du_groupe() {
    let svg = export_svg(&scene_without()).unwrap();
    assert!(!svg.contains("marquage") && !svg.contains("data-form"));
}

// ----- PDF ------------------------------------------------------------------------

/// Chemin tracé ou rempli relevé dans un flux : points sur la courbe, nombre de courbes.
#[derive(Debug)]
struct Sub {
    pts: Vec<(f64, f64)>,
    curves: usize,
    filled: bool,
}

fn page_streams(bytes: &[u8]) -> Vec<String> {
    let readable = pdf_text::readable(bytes);
    let text = readable.as_str();
    let mut pages = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("stream\n") {
        let body = &rest[start + 7..];
        let end = body.find("endstream").unwrap();
        pages.push(body[..end].to_string());
        rest = &body[end + 9..];
    }
    pages
}

/// Lecteur indépendant des opérateurs `m l c h S f` (mm), sans texte ni pointillés.
fn subpaths(stream: &str) -> Vec<Sub> {
    let mm = |v: f64| v / MM_TO_PT;
    let mut cleaned = String::new();
    let mut chars = stream.chars();
    while let Some(c) = chars.next() {
        match c {
            '(' => {
                while let Some(d) = chars.next() {
                    match d {
                        '\\' => {
                            chars.next();
                        }
                        ')' => break,
                        _ => {}
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
    let (mut out, mut cur): (Vec<Sub>, Option<Sub>) = (Vec::new(), None);
    let mut stack: Vec<f64> = Vec::new();
    for tok in cleaned.split_whitespace() {
        if let Ok(v) = tok.parse::<f64>() {
            stack.push(v);
            continue;
        }
        let n = stack.len();
        match tok {
            "m" => {
                cur = Some(Sub {
                    pts: vec![(mm(stack[n - 2]), mm(stack[n - 1]))],
                    curves: 0,
                    filled: false,
                })
            }
            "l" => {
                if let Some(s) = cur.as_mut() {
                    s.pts.push((mm(stack[n - 2]), mm(stack[n - 1])));
                }
            }
            "c" => {
                if let Some(s) = cur.as_mut() {
                    s.pts.push((mm(stack[n - 2]), mm(stack[n - 1])));
                    s.curves += 1;
                }
            }
            "S" | "f" => {
                if let Some(mut s) = cur.take() {
                    s.filled = tok == "f";
                    out.push(s);
                }
            }
            _ => {}
        }
        stack.clear();
    }
    out
}

fn width_height(pts: &[(f64, f64)]) -> (f64, f64) {
    let (mut lo, mut hi) = ((f64::MAX, f64::MAX), (f64::MIN, f64::MIN));
    for p in pts {
        lo = (lo.0.min(p.0), lo.1.min(p.1));
        hi = (hi.0.max(p.0), hi.1.max(p.1));
    }
    (hi.0 - lo.0, hi.1 - lo.1)
}

fn centre(pts: &[(f64, f64)]) -> (f64, f64) {
    let (mut lo, mut hi) = ((f64::MAX, f64::MAX), (f64::MIN, f64::MIN));
    for p in pts {
        lo = (lo.0.min(p.0), lo.1.min(p.1));
        hi = (hi.0.max(p.0), hi.1.max(p.1));
    }
    ((lo.0 + hi.0) / 2.0, (lo.1 + hi.1) / 2.0)
}

#[test]
fn pdf_formes_vectorielles_diametre_et_positions_mesures_en_mm() {
    let sc = scene();
    let tile = plan_tiles(&sc).unwrap()[0];
    let pdf = export_pdf(std::slice::from_ref(&sc)).unwrap();
    assert_eq!(pdf.page_count, 2);
    let pages = page_streams(&pdf.bytes);
    let subs = subpaths(&pages[1]);
    // Cercles : sous-chemins courbes seulement (le contour est fait de segments).
    let circles: Vec<&Sub> = subs.iter().filter(|s| s.curves > 0).collect();
    assert_eq!(circles.len(), 3, "{subs:?}");
    let want = |at: PointMm| {
        let c = tile.to_page_mm(at);
        (c.x, c.y)
    };
    let find = |diameter: f64, filled: bool| -> &Sub {
        circles
            .iter()
            .find(|s| s.filled == filled && (width_height(&s.pts).0 - diameter).abs() < 1e-3)
            .unwrap_or_else(|| panic!("aucun cercle de {diameter} mm : {circles:?}"))
    };
    // Perçage Ø 6 mm : largeur ET hauteur 6 mm, centre à la position de la page, non plein.
    let d6 = find(6.0, false);
    let (w, h) = width_height(&d6.pts);
    assert!(
        (w - 6.0).abs() < 1e-3 && (h - 6.0).abs() < 1e-3,
        "{w} × {h}"
    );
    let (c, e) = (centre(&d6.pts), want(pt(50.0, 30.0)));
    assert!(
        (c.0 - e.0).abs() < 1e-3 && (c.1 - e.1).abs() < 1e-3,
        "{c:?} {e:?}"
    );
    // Perçage Ø 3 mm au bout de pince reculé : centre (16 ; 18).
    let d3 = find(3.0, false);
    let (c, e) = (centre(&d3.pts), want(pt(16.0, 18.0)));
    assert!(
        (c.0 - e.0).abs() < 1e-3 && (c.1 - e.1).abs() < 1e-3,
        "{c:?} {e:?}"
    );
    // Pointage : disque plein de 1 mm (opérateur de remplissage), distinct d'un perçage.
    let punch = find(1.0, true);
    let (c, e) = (centre(&punch.pts), want(pt(20.0, 20.0)));
    assert!((c.0 - e.0).abs() < 1e-3 && (c.1 - e.1).abs() < 1e-3);
    assert!(circles.iter().filter(|s| s.filled).count() == 1);
    // Croix : deux traits de 10 mm, un horizontal et un vertical, qui se coupent en (80 ; 40).
    let cross: Vec<&Sub> = subs
        .iter()
        .filter(|s| s.curves == 0 && s.pts.len() == 2)
        .filter(|s| {
            let (w, h) = width_height(&s.pts);
            (w - 10.0).abs() < 1e-3 && h < 1e-3 || (h - 10.0).abs() < 1e-3 && w < 1e-3
        })
        .collect();
    assert_eq!(cross.len(), 2, "{subs:?}");
    let e = want(pt(80.0, 40.0));
    for s in &cross {
        let c = centre(&s.pts);
        assert!((c.0 - e.0).abs() < 1e-3 && (c.1 - e.1).abs() < 1e-3);
    }
    // Les côtés du rectangle de la pièce gardent leurs longueurs (aucune échelle).
    let sides: Vec<f64> = subs
        .iter()
        .filter(|s| s.curves == 0 && s.pts.len() == 2)
        .map(|s| {
            let (a, b) = (s.pts[0], s.pts[1]);
            (b.0 - a.0).hypot(b.1 - a.1)
        })
        .filter(|l| *l > 50.0)
        .collect();
    assert_eq!(sides.len(), 4);
    // Vectoriel : aucune image.
    let text = std::str::from_utf8(&pdf.bytes).unwrap();
    assert!(!text.contains("/Image") && !text.contains("/XObject"));
}

#[test]
fn pdf_legende_des_marquages_seulement_quand_il_y_en_a() {
    let with = export_pdf(&[scene()]).unwrap();
    let without = export_pdf(&[scene_without()]).unwrap();
    let cal_with = &page_streams(&with.bytes)[0];
    let cal_without = &page_streams(&without.bytes)[0];
    assert!(cal_with.contains("Marquage"), "légende absente");
    assert!(!cal_without.contains("Marquage"));
    // La page étalon seule (aucune scène) reste celle d'avant : pas de ligne de légende.
    let alone = pf_export::pdf::export_calibration_pdf();
    assert!(!page_streams(&alone.bytes)[0].contains("Marquage"));
    // Les pages de patron sans marquage n'ont aucun chemin plein.
    let subs = subpaths(&page_streams(&without.bytes)[1]);
    assert!(subs.iter().all(|s| !s.filled && s.curves == 0));
}

#[test]
fn export_deterministe_avec_marquages() {
    let sc = scene();
    assert_eq!(
        export_svg(&sc).unwrap(),
        export_svg(&scene()).unwrap(),
        "SVG déterministe"
    );
    assert_eq!(
        export_pdf(std::slice::from_ref(&sc)).unwrap(),
        export_pdf(&[scene()]).unwrap(),
        "PDF déterministe"
    );
}

#[test]
fn marquage_hors_contour_agrandit_l_emprise_au_lieu_d_etre_perdu() {
    let (mut p, s, piece) = project();
    let far = p
        .apply(Command::AddPoint {
            sheet: s,
            at: pt(300.0, 20.0),
        })
        .unwrap()
        .points[0];
    p.apply(Command::AddPieceMarking {
        piece,
        at: MarkingAnchor::Point { point: far },
        shape: MarkingShape::Drill { diameter_mm: 8.0 },
    })
    .unwrap();
    let sc = Scene::from_sheet(&p, s).unwrap();
    let bb = sc.bbox().unwrap();
    assert!(bb.max.x >= 304.0 - 1e-9, "{bb:?}");
    let svg = export_svg(&sc).unwrap();
    assert_eq!(svg.matches("data-form=\"percage\"").count(), 3);
}
