//! US-06.10 — les zones de parage, de collage et de découpe interne sont exportées avec le
//! contour : groupe `zones` du SVG (un groupe par zone, contour + motif + étiquette), tracés
//! vectoriels et étiquettes du PDF, polylignes et textes du DXF, plume 6 du HPGL. La largeur
//! d'une bande est relue **dans chaque format** par un lecteur écrit ici (jamais par l'écrivain) :
//! elle doit valoir la valeur saisie à 0,01 mm. Valeurs calculées à la main.

mod common;

use common::hpgl::read_hpgl;
use pf_core::{
    Command, PieceId, PieceSource, PointRef, Project, SheetId, ZoneKind, ZoneShape, ZoneSpec,
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

fn edge(p: &Project, s: SheetId, a: PointMm, b: PointMm) -> pf_core::EntityId {
    let sheet = p.sheet(s).unwrap();
    let near = |u: PointMm, v: PointMm| (u.x - v.x).abs() < 1e-9 && (u.y - v.y).abs() < 1e-9;
    sheet
        .entities
        .iter()
        .find(|e| {
            sheet
                .segment_ends(e.id)
                .is_ok_and(|(x, y)| (near(x, a) && near(y, b)) || (near(x, b) && near(y, a)))
        })
        .unwrap()
        .id
}

/// Rectangle 100 × 70 ; parage : bande de 5 mm sur le bas (y de 0 à 5, aire 500) ; collage :
/// bande de 7,5 mm sur le haut (y de 62,5 à 70, aire 750) ; découpe interne : polygone
/// (30 ; 20) (70 ; 20) (70 ; 50) (30 ; 50), aire 1200.
fn project() -> (Project, SheetId, PieceId) {
    let mut p = Project::new("Zones export");
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
    let bottom = edge(&p, s, pt(0.0, 0.0), pt(100.0, 0.0));
    let top = edge(&p, s, pt(100.0, 70.0), pt(0.0, 70.0));
    for (kind, e, w) in [(ZoneKind::Skive, bottom, 5.0), (ZoneKind::Glue, top, 7.5)] {
        p.apply(Command::AddPieceZone {
            piece,
            zone: ZoneSpec {
                kind,
                shape: ZoneShape::Band {
                    edges: vec![e],
                    width_mm: w,
                },
            },
        })
        .unwrap();
    }
    p.apply(Command::AddPieceZone {
        piece,
        zone: ZoneSpec {
            kind: ZoneKind::InnerCut,
            shape: ZoneShape::Polygon {
                points: [(30.0, 20.0), (70.0, 20.0), (70.0, 50.0), (30.0, 50.0)]
                    .map(|(x, y)| pt(x, y))
                    .to_vec(),
            },
        },
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

fn numbers(d: &str) -> Vec<f64> {
    d.split_whitespace()
        .filter_map(|v| v.parse().ok())
        .collect()
}

/// Les groupes `<g data-zone=…>` du SVG : (identifiant, texte du groupe).
fn zone_groups(svg: &str) -> Vec<(String, String)> {
    let (_, after) = svg.split_once("data-role=\"zone\"").expect("groupe zones");
    let mut out = Vec::new();
    for chunk in after.split("<g data-zone=\"").skip(1) {
        let id = chunk.split('"').next().unwrap().to_string();
        let body = chunk.split("</g>").next().unwrap();
        out.push((id, format!("<g data-zone=\"{body}")));
    }
    out
}

fn svg_to_model(svg_bb: (f64, f64), p: (f64, f64)) -> (f64, f64) {
    // x = x_svg − marge + min.x ; y = max.y − (y_svg − marge)
    let m = pf_export::svg::SVG_MARGIN_MM;
    (p.0 - m + svg_bb.0, svg_bb.1 - (p.1 - m))
}

#[test]
fn la_scene_porte_trois_zones_avec_largeur_et_aire() {
    let (p, s, _) = project();
    let sc = Scene::from_sheet(&p, s).unwrap();
    assert_eq!(sc.zones.len(), 3);
    assert_eq!(sc.zones[0].kind, ZoneKind::Skive);
    assert_eq!(sc.zones[0].width_mm, Some(5.0));
    assert_eq!(sc.zones[1].width_mm, Some(7.5));
    assert_eq!(sc.zones[2].width_mm, None);
    let area = |z: &pf_export::SceneZone| {
        let v = &z.polygon;
        (0..v.len())
            .map(|i| v[i].x * v[(i + 1) % v.len()].y - v[(i + 1) % v.len()].x * v[i].y)
            .sum::<f64>()
            .abs()
            / 2.0
    };
    for (z, want) in sc.zones.iter().zip([500.0, 750.0, 1200.0]) {
        assert!((area(z) - want).abs() < 1e-9);
    }
}

#[test]
fn svg_largeur_lue_motif_couleur_et_etiquette_par_type() {
    let (p, s, _) = project();
    let sc = Scene::from_sheet(&p, s).unwrap();
    let svg = export_svg(&sc).unwrap();
    let bb = sc.bbox().unwrap();
    let groups = zone_groups(&svg);
    assert_eq!(groups.len(), 3);
    let by = |id: &str| groups.iter().find(|g| g.0 == id).unwrap().1.clone();
    let (skive, glue, cut) = (by("parage"), by("collage"), by("decoupe-interne"));
    // Étiquette texte par type et couleurs distinctes.
    assert!(skive.contains(">PARAGE</text>"));
    assert!(glue.contains(">COLLAGE</text>"));
    assert!(cut.contains(">DECOUPE INTERNE</text>"));
    let colors: Vec<&str> = [&skive, &glue, &cut]
        .iter()
        .map(|g| attr_str(g.lines().next().unwrap(), "stroke"))
        .collect();
    assert!(colors[0] != colors[1] && colors[1] != colors[2] && colors[0] != colors[2]);
    // Largeur saisie annoncée et lue sur le contour : écart des deux côtés horizontaux.
    for (g, saisie) in [(&skive, 5.0), (&glue, 7.5)] {
        let tag = g.lines().next().unwrap();
        assert!((attr_str(tag, "data-width-mm").parse::<f64>().unwrap() - saisie).abs() < 1e-9);
        let contour = g
            .lines()
            .find(|l| l.contains("data-part=\"contour\""))
            .unwrap();
        let v = numbers(attr_str(contour, "d"));
        assert_eq!(v.len(), 8, "{contour}");
        let ys: Vec<f64> = (0..4)
            .map(|k| svg_to_model((bb.min.x, bb.max.y), (v[2 * k], v[2 * k + 1])).1)
            .collect();
        let (lo, hi) = ys
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), y| {
                (a.min(*y), b.max(*y))
            });
        assert!(
            ((hi - lo) - saisie).abs() < 0.01,
            "{} lu pour {saisie}",
            hi - lo
        );
    }
    // Parage : hachures, aucun point ; longueur ≈ aire / pas = 500 / 2 = 250 mm.
    assert!(!skive.contains("<circle"));
    let motif = skive
        .lines()
        .find(|l| l.contains("data-part=\"motif\""))
        .unwrap();
    let n = numbers(attr_str(motif, "d"));
    let mut total = 0.0;
    for seg in n.chunks(4) {
        total += (seg[2] - seg[0]).hypot(seg[3] - seg[1]);
        // Toutes les hachures sont dans la bande (y de 0 à 5) et à 45°.
        let (a, b) = (
            svg_to_model((bb.min.x, bb.max.y), (seg[0], seg[1])),
            svg_to_model((bb.min.x, bb.max.y), (seg[2], seg[3])),
        );
        assert!(a.1 >= -1e-6 && a.1 <= 5.0 + 1e-6 && b.1 >= -1e-6 && b.1 <= 5.0 + 1e-6);
        assert!(((b.0 - a.0).abs() - (b.1 - a.1).abs()).abs() < 1e-6);
    }
    assert!((total - 250.0).abs() < 8.0, "{total}");
    // Collage : points, aucune hachure ; ≈ 750 / 2,5² = 120 points.
    assert!(!glue.contains("data-part=\"motif\" stroke-width"));
    let dots = glue.matches("<circle").count();
    assert!((110..=130).contains(&dots), "{dots}");
    // Découpe interne : croisillons, les deux orientations (pentes + et −) présentes.
    let motif = cut
        .lines()
        .find(|l| l.contains("data-part=\"motif\""))
        .unwrap();
    let n = numbers(attr_str(motif, "d"));
    let slopes: Vec<f64> = n.chunks(4).map(|s| (s[3] - s[1]) * (s[2] - s[0])).collect();
    assert!(slopes.iter().any(|v| *v > 1e-6) && slopes.iter().any(|v| *v < -1e-6));
    // Contour : plein pour le parage, pointillé distinct pour les deux autres.
    assert!(!skive.contains("stroke-dasharray"));
    assert!(glue.contains("stroke-dasharray=\"2 1\""));
    assert!(cut.contains("stroke-dasharray=\"4 1 0.8 1\""));
}

#[test]
fn sans_zone_aucun_groupe() {
    let (mut p, s, piece) = project();
    let ids: Vec<_> = p.piece(piece).unwrap().zones.iter().map(|z| z.id).collect();
    for zone in ids {
        p.apply(Command::RemovePieceZone { piece, zone }).unwrap();
    }
    let sc = Scene::from_sheet(&p, s).unwrap();
    assert!(sc.zones.is_empty());
    let svg = export_svg(&sc).unwrap();
    assert!(!svg.contains("zone"));
    let pdf = export_pdf(std::slice::from_ref(&sc)).unwrap();
    assert!(!common::pdf_text::readable(&pdf.bytes).contains("PARAGE"));
}

/// Chemins `m l … S` d'un flux de page : sommets en mm.
fn paths(stream: &str) -> Vec<Vec<(f64, f64)>> {
    let mm = |v: f64| v / pf_export::MM_TO_PT;
    let mut out: Vec<Vec<(f64, f64)>> = Vec::new();
    let mut cur: Option<Vec<(f64, f64)>> = None;
    let mut stack: Vec<f64> = Vec::new();
    let mut cleaned = String::new();
    let mut chars = stream.chars();
    while let Some(c) = chars.next() {
        match c {
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
            "m" if stack.len() >= 2 => {
                let n = stack.len();
                cur = Some(vec![(mm(stack[n - 2]), mm(stack[n - 1]))]);
            }
            "l" if stack.len() >= 2 => {
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

fn pdf_paths(bytes: &[u8]) -> Vec<Vec<(f64, f64)>> {
    let text = common::pdf_text::readable(bytes);
    let mut all = Vec::new();
    let mut rest = text.as_str();
    while let Some(start) = rest.find("stream\n") {
        let body = &rest[start + 7..];
        let end = body.find("endstream").unwrap();
        all.extend(paths(&body[..end]));
        rest = &body[end + 9..];
    }
    all
}

fn shoelace(v: &[(f64, f64)]) -> f64 {
    let n = v.len();
    (0..n)
        .map(|i| v[i].0 * v[(i + 1) % n].1 - v[(i + 1) % n].0 * v[i].1)
        .sum::<f64>()
        .abs()
        / 2.0
}

#[test]
fn pdf_contours_largeur_lue_et_etiquettes() {
    let (p, s, _) = project();
    let sc = Scene::from_sheet(&p, s).unwrap();
    let pdf = export_pdf(std::slice::from_ref(&sc)).unwrap();
    let text = common::pdf_text::readable(&pdf.bytes);
    for label in ["PARAGE", "COLLAGE", "DECOUPE INTERNE"] {
        assert!(text.contains(label), "{label}");
    }
    let all = pdf_paths(&pdf.bytes);
    // Bande de 5 mm × 100 mm (aire 500) : quatre sommets, petit côté lu = 5,00 mm.
    let rects = |area: f64| -> Vec<&Vec<(f64, f64)>> {
        all.iter()
            .filter(|v| v.len() == 4 && (shoelace(v) - area).abs() < 0.01)
            .collect()
    };
    for (area, width) in [(500.0, 5.0), (750.0, 7.5)] {
        let r = rects(area);
        assert_eq!(r.len(), 1, "aire {area}");
        let v = r[0];
        let side = |i: usize| (v[(i + 1) % 4].0 - v[i].0).hypot(v[(i + 1) % 4].1 - v[i].1);
        let short = (0..4).map(side).fold(f64::INFINITY, f64::min);
        assert!((short - width).abs() < 0.01, "{short} pour {width}");
    }
    // Découpe interne : rectangle 40 × 30 (aire 1200) ; hachures : des segments à 45° et à 135°.
    assert_eq!(rects(1200.0).len(), 1);
    let diagonals = all
        .iter()
        .filter(|v| {
            v.len() == 2 && ((v[1].0 - v[0].0).abs() - (v[1].1 - v[0].1).abs()).abs() < 1e-3
        })
        .count();
    // 35 hachures du parage (5 mm de haut à 45° : ~7,07 mm chacune) + 2 × ~16 de la découpe.
    assert!((60..=80).contains(&diagonals), "{diagonals}");
}

#[test]
fn dxf_polylignes_largeur_lue_et_textes() {
    let (p, _, _) = project();
    let out = export_dxf(&p).unwrap();
    let dxf = common::dxf::parse(&out.text).unwrap();
    let inserted = dxf.inserted().unwrap();
    let ents = inserted[0].1;
    let closed = |layer: &str| -> Vec<Vec<common::dxf::P>> {
        ents.iter()
            .filter(|e| e.layer() == layer)
            .filter_map(|e| match e {
                common::dxf::Ent::Polyline {
                    pts, closed: true, ..
                } => Some(pts.clone()),
                _ => None,
            })
            .collect()
    };
    // Parage et collage : couche 13 ; découpe interne : couche 8.
    let marks = closed("13");
    assert_eq!(marks.len(), 2);
    for (pts, (area, width)) in marks.iter().zip([(500.0, 5.0), (750.0, 7.5)]) {
        assert!((common::dxf::area(pts).abs() - area).abs() < 0.01);
        let (lo, hi) = common::dxf::bbox(pts);
        let short = (hi.0 - lo.0).min(hi.1 - lo.1);
        assert!((short - width).abs() < 0.01, "{short} pour {width}");
    }
    let cuts = closed("8");
    assert_eq!(cuts.len(), 1);
    assert!((common::dxf::area(&cuts[0]).abs() - 1200.0).abs() < 0.01);
    // Étiquettes sur la couche du texte ; hachures (lignes) et points sur la couche 13.
    let texts: Vec<String> = ents
        .iter()
        .filter_map(|e| match e {
            common::dxf::Ent::Text { layer, text, .. } if layer == "84" => Some(text.clone()),
            _ => None,
        })
        .collect();
    for label in ["PARAGE", "COLLAGE", "DECOUPE INTERNE"] {
        assert!(texts.iter().any(|t| t == label), "{label} dans {texts:?}");
    }
    let lines = ents
        .iter()
        .filter(|e| matches!(e, common::dxf::Ent::Line { layer, .. } if layer == "13"))
        .count();
    let points = ents
        .iter()
        .filter(|e| matches!(e, common::dxf::Ent::Point { layer, .. } if layer == "13"))
        .count();
    assert!((60..=80).contains(&lines), "{lines}");
    assert!((110..=130).contains(&points), "{points}");
}

#[test]
fn hpgl_plume_6_contours_largeur_lue() {
    let (p, s, _) = project();
    let sc = Scene::from_sheet(&p, s).unwrap();
    let plot = export_hpgl(&sc, &PageSetup::default()).unwrap();
    let r = read_hpgl(&plot.text).unwrap();
    let pen6: Vec<_> = r.strokes.iter().filter(|k| k.pen == 6).collect();
    // Contours fermés (5 points) : largeur lue au petit côté, arrondi de 1/40 mm.
    let closed: Vec<_> = pen6.iter().filter(|k| k.pts.len() == 5).collect();
    assert_eq!(closed.len(), 3);
    let mut shorts: Vec<f64> = closed
        .iter()
        .map(|k| {
            let xs: Vec<i64> = k.pts.iter().map(|q| q.0).collect();
            let ys: Vec<i64> = k.pts.iter().map(|q| q.1).collect();
            let w = (xs.iter().max().unwrap() - xs.iter().min().unwrap()) as f64 / 40.0;
            let h = (ys.iter().max().unwrap() - ys.iter().min().unwrap()) as f64 / 40.0;
            w.min(h)
        })
        .collect();
    shorts.sort_by(f64::total_cmp);
    for (got, width) in shorts.iter().zip([5.0, 7.5, 30.0]) {
        assert!((got - width).abs() <= 0.03, "{got} pour {width}");
    }
    // Hachures et points : de nombreux traits courts sur la même plume.
    // 3 contours + ~70 hachures + 120 points.
    assert!((185..=200).contains(&pen6.len()), "{}", pen6.len());
}
