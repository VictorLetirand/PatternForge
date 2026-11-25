//! US-06.4 — les boutons et boutonnières sont exportés comme **marquage distinct** (groupe
//! nommé « boutons », forme propre : cercle pour un bouton, fente à bouts arrondis pour une
//! boutonnière), jamais reconnaissables par la couleur seule. Un bouton de diamètre D est
//! exporté avec un diamètre de D mm ; une fente de L × l est exportée avec L × l mm. Les tests
//! relisent les fichiers produits avec un lecteur indépendant du code d'export.

#[path = "common/pdf_text.rs"]
mod pdf_text;

use pf_core::{
    ButtonElement, ButtonholeOrientation, Command, PieceId, PieceSource, PointRef, Project, SheetId,
};
use pf_export::pdf::{export_pdf, plan_tiles};
use pf_export::svg::export_svg;
use pf_export::{Scene, MM_TO_PT};
use pf_geometry::PointMm;

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

/// Pièce 100 × 60 mm, trois groupes :
///  – 3 boutons Ø 12 de (20 ; 45) à (80 ; 45) : centres (20 ; 45), (50 ; 45), (80 ; 45) ;
///  – 2 boutonnières horizontales 24 × 4 de (20 ; 15) à (80 ; 15) ;
///  – 1 boutonnière 20 × 3 à 30° en (50 ; 30).
fn build(with_groups: bool) -> (Project, SheetId, PieceId) {
    let mut p = Project::new("Boutons export");
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
    let (a, b) = (point(20.0, 45.0), point(80.0, 45.0));
    let (c, d) = (point(20.0, 15.0), point(80.0, 15.0));
    let (e, f) = (point(50.0, 30.0), point(60.0, 30.0));
    let mut group = |from, to, count, element| {
        p.apply(Command::AddButtonGroup {
            piece,
            from,
            to,
            count,
            element,
            edge_margin_mm: 0.0,
        })
        .unwrap();
    };
    if !with_groups {
        return (p, s, piece);
    }
    group(a, b, 3, ButtonElement::Button { diameter_mm: 12.0 });
    group(
        c,
        d,
        2,
        ButtonElement::Buttonhole {
            length_mm: 24.0,
            width_mm: 4.0,
            orientation: ButtonholeOrientation::Horizontal {},
        },
    );
    group(
        e,
        f,
        1,
        ButtonElement::Buttonhole {
            length_mm: 20.0,
            width_mm: 3.0,
            orientation: ButtonholeOrientation::Angle { degrees: 30.0 },
        },
    );
    (p, s, piece)
}

fn project() -> (Project, SheetId, PieceId) {
    build(true)
}

fn scene() -> Scene {
    let (p, s, _) = project();
    Scene::from_sheet(&p, s).unwrap()
}

/// Même projet sans aucun groupe.
fn scene_without() -> Scene {
    let (mut p, s, piece) = project();
    let ids: Vec<_> = p
        .piece(piece)
        .unwrap()
        .buttons
        .iter()
        .map(|g| g.id)
        .collect();
    for group in ids {
        p.apply(Command::RemoveButtonGroup { piece, group })
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

/// Contenu de l'attribut `d` d'un chemin SVG, découpé en jetons.
fn path_tokens(tag: &str) -> Vec<String> {
    let key = " d=\"";
    let start = tag.find(key).unwrap() + key.len();
    let end = start + tag[start..].find('"').unwrap();
    tag[start..end]
        .split_whitespace()
        .map(str::to_string)
        .collect()
}

/// Fente relue d'un chemin SVG `M x y L x y A r r 0 f s x y L x y A r r 0 f s x y Z`, sans
/// utiliser les attributs `data-*` : centre, longueur, largeur et angle (en mm, repère du modèle
/// grâce à `back`).
struct Measured {
    center: (f64, f64),
    length: f64,
    width: f64,
    angle_deg: f64,
    radius_a: f64,
    radius_b: f64,
}

fn measure_slot(tag: &str, back: &dyn Fn(f64, f64) -> (f64, f64)) -> Measured {
    let t = path_tokens(tag);
    assert_eq!(t.len(), 3 + 3 + 8 + 3 + 8 + 1, "{t:?}");
    let num = |i: usize| t[i].parse::<f64>().unwrap();
    assert_eq!((t[0].as_str(), t[3].as_str()), ("M", "L"));
    assert_eq!((t[6].as_str(), t[14].as_str()), ("A", "L"));
    assert_eq!((t[17].as_str(), t[25].as_str()), ("A", "Z"));
    let p0 = back(num(1), num(2));
    let p1 = back(num(4), num(5));
    let p2 = back(num(12), num(13));
    let p3 = back(num(15), num(16));
    let straight = (p1.0 - p0.0).hypot(p1.1 - p0.1);
    // L'arc relie les deux côtés : son diamètre est la largeur de la fente.
    let width = (p2.0 - p1.0).hypot(p2.1 - p1.1);
    assert!(
        ((p3.0 - p2.0).hypot(p3.1 - p2.1) - straight).abs() < 1e-3,
        "côtés égaux"
    );
    Measured {
        center: ((p0.0 + p2.0) / 2.0, (p0.1 + p2.1) / 2.0),
        length: straight + width,
        width,
        angle_deg: (p1.1 - p0.1)
            .atan2(p1.0 - p0.0)
            .to_degrees()
            .rem_euclid(360.0),
        radius_a: num(7),
        radius_b: num(18),
    }
}

#[test]
fn la_scene_porte_les_six_elements_aux_positions_calculees() {
    let sc = scene();
    assert_eq!(sc.buttons.len(), 6);
    let at = |i: usize| (sc.buttons[i].pos.x, sc.buttons[i].pos.y);
    assert_eq!(at(0), (20.0, 45.0));
    assert_eq!(at(1), (50.0, 45.0));
    assert_eq!(at(2), (80.0, 45.0));
    assert_eq!(at(3), (20.0, 15.0));
    assert_eq!(at(4), (80.0, 15.0));
    assert_eq!(at(5), (50.0, 30.0));
    // Les éléments sont dans le contour : l'emprise de la scène n'a pas changé.
    assert_eq!(sc.bbox(), scene_without().bbox());
    assert!(sc.markings.is_empty(), "un bouton n'est pas un marquage");
}

#[test]
fn svg_groupe_nomme_formes_distinctes_dimensions_mesurees() {
    let sc = scene();
    let svg = export_svg(&sc).unwrap();
    let bb = sc.bbox().unwrap();
    let (m, ymax) = (pf_export::svg::SVG_MARGIN_MM, bb.max.y);
    let x = |v: f64| v - bb.min.x + m;
    let y = |v: f64| ymax - v + m;
    let back = |sx: f64, sy: f64| (sx - m + bb.min.x, ymax - sy + m);
    // Un groupe nommé, distinct des marquages, des crans et des cotations.
    assert!(svg.contains("<g id=\"boutons\""));
    assert_eq!(svg.matches("data-role=\"bouton\"").count(), 1);
    assert!(!svg.contains("data-role=\"marquage\""));
    let (_, after) = svg.split_once("data-role=\"bouton\"").unwrap();
    let group = after.split("</g>").next().unwrap();
    assert!(group.contains("<title>"), "{group}");
    // Trois boutons : cercles Ø 12 mm (rayon 6), non pleins, aux centres calculés.
    let circles: Vec<&str> = group.lines().filter(|l| l.contains("<circle")).collect();
    assert_eq!(circles.len(), 3, "{group}");
    for (c, want_x) in circles.iter().zip([20.0, 50.0, 80.0]) {
        assert!(c.contains("data-form=\"bouton\""));
        assert!((2.0 * attr(c, "r") - 12.0).abs() < 1e-9, "{c}");
        assert!((attr(c, "data-diameter-mm") - 12.0).abs() < 1e-9);
        assert!((attr(c, "cx") - x(want_x)).abs() < 1e-3, "{c}");
        assert!((attr(c, "cy") - y(45.0)).abs() < 1e-3, "{c}");
        assert!(!c.contains("fill=\"#000000\""), "un bouton n'est pas plein");
    }
    // Trois fentes (deux horizontales 24 × 4, une à 30° de 20 × 3), relues sans les attributs.
    let slots: Vec<&str> = group
        .lines()
        .filter(|l| l.contains("data-form=\"boutonniere\""))
        .collect();
    assert_eq!(slots.len(), 3, "{group}");
    for (tag, cx) in slots[..2].iter().zip([20.0, 80.0]) {
        let s = measure_slot(tag, &back);
        assert!((s.length - 24.0).abs() < 1e-3, "longueur {}", s.length);
        assert!((s.width - 4.0).abs() < 1e-3);
        assert!((s.radius_a - 2.0).abs() < 1e-3 && (s.radius_b - 2.0).abs() < 1e-3);
        assert!(s.angle_deg.abs() < 1e-2 || (360.0 - s.angle_deg).abs() < 1e-2);
        assert!((s.center.0 - cx).abs() < 1e-3 && (s.center.1 - 15.0).abs() < 1e-3);
        assert!((attr(tag, "data-length-mm") - 24.0).abs() < 1e-9);
        assert!((attr(tag, "data-width-mm") - 4.0).abs() < 1e-9);
    }
    let s = measure_slot(slots[2], &back);
    assert!((s.length - 20.0).abs() < 1e-3 && (s.width - 3.0).abs() < 1e-3);
    assert!((s.angle_deg - 30.0).abs() < 1e-2, "{}", s.angle_deg);
    assert!((s.center.0 - 50.0).abs() < 1e-3 && (s.center.1 - 30.0).abs() < 1e-3);
    assert!((attr(slots[2], "data-angle-deg") - 30.0).abs() < 1e-9);
    // Le fichier garde la mention de l'échelle.
    assert!(svg.contains("1 unité = 1 mm"));
}

#[test]
fn svg_sans_groupe_aucune_trace() {
    let svg = export_svg(&scene_without()).unwrap();
    assert!(
        !svg.contains("bouton") && !svg.contains("data-form"),
        "{svg}"
    );
}

// ----- PDF ----------------------------------------------------------------------------

#[derive(Debug)]
struct Sub {
    pts: Vec<(f64, f64)>,
    curves: usize,
    lines: usize,
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
                    lines: 0,
                    filled: false,
                })
            }
            "l" => {
                if let Some(s) = cur.as_mut() {
                    s.pts.push((mm(stack[n - 2]), mm(stack[n - 1])));
                    s.lines += 1;
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

fn extents(pts: &[(f64, f64)], u: (f64, f64)) -> ((f64, f64), (f64, f64)) {
    // Étendue des points le long de `u` et de sa normale gauche.
    let n = (-u.1, u.0);
    let (mut a, mut b) = ((f64::MAX, f64::MIN), (f64::MAX, f64::MIN));
    for p in pts {
        let (s, t) = (p.0 * u.0 + p.1 * u.1, p.0 * n.0 + p.1 * n.1);
        a = (a.0.min(s), a.1.max(s));
        b = (b.0.min(t), b.1.max(t));
    }
    (a, b)
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
fn pdf_formes_vectorielles_dimensions_et_positions_mesurees_en_mm() {
    let sc = scene();
    let tile = plan_tiles(&sc).unwrap()[0];
    let pdf = export_pdf(std::slice::from_ref(&sc)).unwrap();
    assert_eq!(pdf.page_count, 2);
    let pages = page_streams(&pdf.bytes);
    let subs = subpaths(&pages[1]);
    let want = |at: PointMm| {
        let c = tile.to_page_mm(at);
        (c.x, c.y)
    };
    // Boutons : sous-chemins courbes sans segment, non pleins, Ø 12 (largeur ET hauteur).
    let circles: Vec<&Sub> = subs
        .iter()
        .filter(|s| s.curves > 0 && s.lines == 0)
        .collect();
    assert_eq!(circles.len(), 3, "{subs:?}");
    for (c, x) in circles.iter().zip([20.0, 50.0, 80.0]) {
        assert!(!c.filled);
        // Les points d'un cercle sont régulièrement répartis sur la circonférence : leur
        // moyenne est le centre, leur distance au centre est le rayon (6 mm pour Ø 12).
        let nodes = &c.pts[..c.pts.len() - 1];
        let n = nodes.len() as f64;
        let ctr = (
            nodes.iter().map(|p| p.0).sum::<f64>() / n,
            nodes.iter().map(|p| p.1).sum::<f64>() / n,
        );
        for p in nodes {
            let r = (p.0 - ctr.0).hypot(p.1 - ctr.1);
            assert!((r - 6.0).abs() < 1e-3, "rayon {r} : {c:?}");
        }
        let ((w0, w1), _) = extents(&c.pts, (1.0, 0.0));
        assert!((w1 - w0 - 12.0).abs() < 1e-3, "largeur {}", w1 - w0);
        let e = want(pt(x, 45.0));
        assert!(
            (ctr.0 - e.0).abs() < 1e-3 && (ctr.1 - e.1).abs() < 1e-3,
            "{ctr:?} {e:?}"
        );
    }
    // Fentes : sous-chemins avec deux droites et quatre courbes (deux demi-cercles).
    let slots: Vec<&Sub> = subs
        .iter()
        .filter(|s| s.curves > 0 && s.lines == 2)
        .collect();
    assert_eq!(slots.len(), 3, "{subs:?}");
    for (s, x) in slots[..2].iter().zip([20.0, 80.0]) {
        let ((l0, l1), (w0, w1)) = extents(&s.pts, (1.0, 0.0));
        assert!((l1 - l0 - 24.0).abs() < 1e-3, "longueur {}", l1 - l0);
        assert!((w1 - w0 - 4.0).abs() < 1e-3, "largeur {}", w1 - w0);
        let (ctr, e) = (centre(&s.pts), want(pt(x, 15.0)));
        assert!((ctr.0 - e.0).abs() < 1e-3 && (ctr.1 - e.1).abs() < 1e-3);
    }
    // Fente à 30° : étendue 20 mm le long de son axe, 3 mm de travers.
    let (c, s) = (30f64.to_radians().cos(), 30f64.to_radians().sin());
    let ((l0, l1), (w0, w1)) = extents(&slots[2].pts, (c, s));
    assert!((l1 - l0 - 20.0).abs() < 1e-3, "longueur {}", l1 - l0);
    assert!((w1 - w0 - 3.0).abs() < 1e-3, "largeur {}", w1 - w0);
    let (ctr, e) = (centre(&slots[2].pts), want(pt(50.0, 30.0)));
    assert!((ctr.0 - e.0).abs() < 0.2 && (ctr.1 - e.1).abs() < 0.2);
    // Les côtés du rectangle de la pièce gardent leurs longueurs (aucune échelle).
    let sides: Vec<f64> = subs
        .iter()
        .filter(|s| s.curves == 0 && s.pts.len() == 2)
        .map(|s| (s.pts[1].0 - s.pts[0].0).hypot(s.pts[1].1 - s.pts[0].1))
        .filter(|l| *l > 50.0)
        .collect();
    assert_eq!(sides.len(), 4);
    // Vectoriel : aucune image.
    let text = std::str::from_utf8(&pdf.bytes).unwrap();
    assert!(!text.contains("/Image") && !text.contains("/XObject"));
}

#[test]
fn pdf_legende_des_boutons_seulement_quand_il_y_en_a() {
    let with = export_pdf(&[scene()]).unwrap();
    let without = export_pdf(&[scene_without()]).unwrap();
    let cal_with = &page_streams(&with.bytes)[0];
    let cal_without = &page_streams(&without.bytes)[0];
    assert!(cal_with.contains("boutonni"), "légende absente");
    assert!(!cal_without.contains("boutonni"));
    let alone = pf_export::pdf::export_calibration_pdf();
    assert!(!page_streams(&alone.bytes)[0].contains("boutonni"));
    // Les pages de patron sans groupe n'ont ni courbe ni chemin plein.
    let subs = subpaths(&page_streams(&without.bytes)[1]);
    assert!(subs.iter().all(|s| !s.filled && s.curves == 0));
}

#[test]
fn export_deterministe_et_identique_a_l_avant_quand_il_n_y_a_aucun_groupe() {
    let sc = scene();
    assert_eq!(export_svg(&sc).unwrap(), export_svg(&scene()).unwrap());
    assert_eq!(
        export_pdf(std::slice::from_ref(&sc)).unwrap(),
        export_pdf(&[scene()]).unwrap()
    );
    // Retirer tous les groupes redonne exactement la scène d'un projet qui n'en a jamais eu :
    // même scène, donc mêmes octets SVG et PDF.
    let (p, s, _) = build(false);
    let never = Scene::from_sheet(&p, s).unwrap();
    assert_eq!(never, scene_without());
    assert_eq!(
        export_svg(&never).unwrap(),
        export_svg(&scene_without()).unwrap()
    );
    assert_eq!(
        export_pdf(&[never]).unwrap(),
        export_pdf(&[scene_without()]).unwrap()
    );
}

#[test]
fn element_hors_contour_agrandit_l_emprise_au_lieu_d_etre_perdu() {
    let (mut p, s, piece) = project();
    let a = p
        .apply(Command::AddPoint {
            sheet: s,
            at: pt(300.0, 20.0),
        })
        .unwrap()
        .points[0];
    let b = p
        .apply(Command::AddPoint {
            sheet: s,
            at: pt(320.0, 20.0),
        })
        .unwrap()
        .points[0];
    p.apply(Command::AddButtonGroup {
        piece,
        from: a,
        to: b,
        count: 2,
        element: ButtonElement::Button { diameter_mm: 8.0 },
        edge_margin_mm: 0.0,
    })
    .unwrap();
    let sc = Scene::from_sheet(&p, s).unwrap();
    let bb = sc.bbox().unwrap();
    assert!(bb.max.x >= 324.0 - 1e-9, "{bb:?}");
    let svg = export_svg(&sc).unwrap();
    assert_eq!(svg.matches("data-form=\"bouton\"").count(), 5);
}
