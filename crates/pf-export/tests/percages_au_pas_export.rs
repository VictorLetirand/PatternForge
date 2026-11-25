//! PH-4b — les fentes de poinçon des perçages au pas sont exportées avec leurs dimensions et leur
//! orientation (le long du bord) en SVG et en PDF (lecteurs indépendants), en DXF (couche 13,
//! polyligne fermée relue et mesurée par `common/dxf_check.rs`) et en HPGL (plume 6). Les
//! perçages ronds de la rangée sont des perçages ordinaires : ils sont déjà couverts par
//! `marquages.rs`.

mod common;

use common::dxf_check::{verify_export, L_DRILL};
use common::hpgl::read_hpgl;
use pf_core::{
    Command, MarkingRunSpec, MarkingShape, PathRole, PieceId, PieceSource, PointRef, Project,
    RunForm, RunSide, RunSpacing, SheetId,
};
use pf_export::dxf::export_dxf;
use pf_export::hpgl::export_hpgl;
use pf_export::pdf::{export_pdf, plan_tiles};
use pf_export::svg::export_svg;
use pf_export::{PageSetup, Scene, MM_TO_PT};
use pf_geometry::PointMm;

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

/// Longueur et largeur de la fente, dimensions attendues dans tous les formats.
const LEN: f64 = 6.0;
const WID: f64 = 1.5;
/// Direction du bord oblique (30 ; 40) / 50 : atan2(4, 3) en degrés, calculé à la main.
const ANGLE: f64 = 53.13010235415598;

/// Losange (0 ; 0) (40 ; 0) (70 ; 40) (30 ; 40), trois fentes 6 × 1,5 le long du bord oblique
/// (40 ; 0) → (70 ; 40), à 3 mm du bord, aux abscisses 10, 25 et 40 mm.
fn project() -> (Project, SheetId, PieceId) {
    let mut p = Project::new("Fentes export");
    let s = p.sheets[0].id;
    p.apply(Command::AddPolygon {
        sheet: s,
        vertices: [(0.0, 0.0), (40.0, 0.0), (70.0, 40.0), (30.0, 40.0)]
            .map(|(x, y)| PointRef::New(pt(x, y)))
            .to_vec(),
        closed: true,
        role: PathRole::default(),
        layer: None,
    })
    .unwrap();
    let piece = p
        .apply(Command::ExtractPiece {
            sheet: s,
            source: PieceSource::Click { at: pt(35.0, 20.0) },
            name: "Losange".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    let sheet = p.sheet(s).unwrap();
    let edge = sheet
        .entities
        .iter()
        .find(|e| {
            sheet.segment_ends(e.id).is_ok_and(|(a, b)| {
                (a.x - 40.0).abs() < 1e-9 && (b.x - 70.0).abs() < 1e-9 && (b.y - 40.0).abs() < 1e-9
            })
        })
        .unwrap()
        .id;
    p.apply(Command::AddPieceMarkingRun {
        piece,
        run: MarkingRunSpec {
            edges: vec![edge],
            spacing: RunSpacing::Count { count: 3 },
            start_margin_mm: 10.0,
            end_margin_mm: 10.0,
            distance_mm: 3.0,
            side: RunSide::Inside,
            form: RunForm::Slot {
                length_mm: LEN,
                width_mm: WID,
            },
        },
    })
    .unwrap();
    (p, s, piece)
}

/// Centres attendus : pied (40 ; 0) + s · (0,6 ; 0,8) + 3 · (−0,8 ; 0,6).
fn centers() -> [(f64, f64); 3] {
    [10.0, 25.0, 40.0].map(|s| (40.0 + 0.6 * s - 2.4, 0.8 * s + 1.8))
}

fn scene() -> Scene {
    let (p, s, _) = project();
    Scene::from_sheet(&p, s).unwrap()
}

#[test]
fn la_scene_porte_trois_fentes_orientees() {
    let sc = scene();
    assert_eq!(sc.markings.len(), 3);
    for (m, (cx, cy)) in sc.markings.iter().zip(centers()) {
        assert!(
            (m.pos.x - cx).abs() < 1e-9 && (m.pos.y - cy).abs() < 1e-9,
            "{m:?}"
        );
        let MarkingShape::Slot {
            length_mm,
            width_mm,
            angle_deg,
        } = m.shape
        else {
            panic!("fente attendue")
        };
        assert_eq!((length_mm, width_mm), (LEN, WID));
        assert!((angle_deg - ANGLE).abs() < 1e-9);
        assert_eq!(m.form_id(), "fente");
    }
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
fn svg_fente_dimensions_orientation_et_position_relues() {
    let sc = scene();
    let svg = export_svg(&sc).unwrap();
    let bb = sc.bbox().unwrap();
    let (m, ymax) = (pf_export::svg::SVG_MARGIN_MM, bb.max.y);
    let x = |v: f64| v - bb.min.x + m;
    let y = |v: f64| ymax - v + m;
    let (_, after) = svg.split_once("data-role=\"marquage\"").expect("groupe");
    let group = after.split("</g>").next().unwrap();
    assert!(group.contains("fente"), "légende : {group}");
    let slots: Vec<&str> = group
        .lines()
        .filter(|l| l.contains("data-form=\"fente\""))
        .collect();
    assert_eq!(slots.len(), 3, "{group}");
    for (tag, (cx, cy)) in slots.iter().zip(centers()) {
        assert!((attr(tag, "data-length-mm") - LEN).abs() < 1e-9);
        assert!((attr(tag, "data-width-mm") - WID).abs() < 1e-9);
        assert!((attr(tag, "data-angle-deg") - ANGLE).abs() < 1e-3);
        // Chemin : M p0 L p1 A r r 0 g s p2 L p3 A r r 0 g s p0 Z, relu à la main.
        let d = attr_str(tag, "d");
        let t: Vec<&str> = d.split_whitespace().collect();
        let num = |i: usize| t[i].parse::<f64>().unwrap();
        assert_eq!(
            (t[0], t[3], t[6], t[14], t[17], t[25]),
            ("M", "L", "A", "L", "A", "Z"),
            "{d}"
        );
        let (p0, p1, p2, p3) = (
            (num(1), num(2)),
            (num(4), num(5)),
            (num(12), num(13)),
            (num(15), num(16)),
        );
        // Rayons des bouts = demi-largeur.
        assert!(
            (num(7) - WID / 2.0).abs() < 1e-3 && (num(8) - WID / 2.0).abs() < 1e-3,
            "{d}"
        );
        // Côté droit : longueur L − w, parallèle à l'axe (axe Y retourné en SVG).
        let side = (p1.0 - p0.0).hypot(p1.1 - p0.1);
        assert!((side - (LEN - WID)).abs() < 1e-3, "{side}");
        let axis = (-(p1.1 - p0.1)).atan2(p1.0 - p0.0).to_degrees();
        assert!((axis - ANGLE).abs() < 1e-2, "{axis}");
        // Le centre des quatre extrémités est le centre calculé (en repère SVG).
        let c = (
            (p0.0 + p1.0 + p2.0 + p3.0) / 4.0,
            (p0.1 + p1.1 + p2.1 + p3.1) / 4.0,
        );
        assert!(
            (c.0 - x(cx)).abs() < 1e-2 && (c.1 - y(cy)).abs() < 1e-2,
            "{c:?}"
        );
        // Largeur : distance entre les deux côtés droits (extrémité du premier au début du second).
        assert!(((p2.0 - p1.0).hypot(p2.1 - p1.1) - WID).abs() < 1e-2);
    }
}

/// Opérateurs `m l c` d'un flux de page : tous les points (extrémités et poignées), en mm.
fn closed_paths(stream: &str) -> Vec<Vec<(f64, f64)>> {
    let mm = |v: f64| v / MM_TO_PT;
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
            "l" | "c" => {
                if let Some(path) = cur.as_mut() {
                    let n = stack.len();
                    for pair in stack[n - if tok == "l" { 2 } else { 6 }..].chunks(2) {
                        path.push((mm(pair[0]), mm(pair[1])));
                    }
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

#[test]
fn pdf_fente_dimensions_et_orientation_mesurees_en_mm() {
    let sc = scene();
    let tile = plan_tiles(&sc).unwrap()[0];
    let pdf = export_pdf(std::slice::from_ref(&sc)).unwrap();
    let text = common::pdf_text::readable(&pdf.bytes);
    // Tous les flux : on cherche les trois chemins courbes fermés de 6 × 1,5 mm.
    let mut found = 0;
    let (ux, uy) = (ANGLE.to_radians().cos(), ANGLE.to_radians().sin());
    let mut rest = text.as_str();
    while let Some(start) = rest.find("stream\n") {
        let body = &rest[start + 7..];
        let end = body.find("endstream").unwrap();
        for path in closed_paths(&body[..end]) {
            if path.len() < 8 {
                continue;
            }
            // Étendue le long de l'axe (u) et en travers (v), poignées de Bézier comprises.
            let (mut u_lo, mut u_hi, mut v_lo, mut v_hi) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
            for (px, py) in &path {
                let (u, v) = (px * ux + py * uy, -px * uy + py * ux);
                (u_lo, u_hi) = (u_lo.min(u), u_hi.max(u));
                (v_lo, v_hi) = (v_lo.min(v), v_hi.max(v));
            }
            if ((u_hi - u_lo) - LEN).abs() < 1e-2 && ((v_hi - v_lo) - WID).abs() < 1e-2 {
                // Centre = milieu des étendues, comparé à la position attendue sur la page.
                let (uc, vc) = ((u_lo + u_hi) / 2.0, (v_lo + v_hi) / 2.0);
                let c = (uc * ux - vc * uy, uc * uy + vc * ux);
                let hit = centers().iter().any(|(cx, cy)| {
                    let e = tile.to_page_mm(pt(*cx, *cy));
                    (c.0 - e.x).abs() < 1e-2 && (c.1 - e.y).abs() < 1e-2
                });
                assert!(hit, "centre {c:?} inattendu");
                found += 1;
            }
        }
        rest = &body[end + 9..];
    }
    assert_eq!(found, 3, "trois fentes de 6 × 1,5 mm orientées de {ANGLE}°");
    // La légende cite la fente seulement quand il y en a.
    assert!(text.contains("croix, fente"));
    let (p, _, piece) = project();
    let mut q = p.clone();
    let ids: Vec<_> = q
        .piece(piece)
        .unwrap()
        .markings
        .iter()
        .map(|m| m.id)
        .collect();
    for marking in ids {
        q.apply(Command::RemovePieceMarking { piece, marking })
            .unwrap();
    }
    let s = q.sheets[0].id;
    let without = export_pdf(&[Scene::from_sheet(&q, s).unwrap()]).unwrap();
    assert!(!common::pdf_text::readable(&without.bytes).contains("fente"));
}

#[test]
fn dxf_couche_13_polylignes_fermees_mesurees_par_le_lecteur_independant() {
    let (p, _, _) = project();
    let out = export_dxf(&p).unwrap();
    // `verify_export` compare chaque fente (aire et périmètre d'une fente 6 × 1,5, centroïde).
    let (dxf, _) = verify_export(&p, &out.text, &out.report).expect("relecture");
    let inserted = dxf.inserted().unwrap();
    let polys: usize = inserted
        .iter()
        .flat_map(|(_, ents)| ents.iter())
        .filter(|e| matches!(e, common::dxf::Ent::Polyline { layer, closed: true, .. } if layer == L_DRILL))
        .count();
    assert_eq!(polys, 3);
}

#[test]
fn hpgl_plume_6_trace_le_contour_de_chaque_fente() {
    let sc = scene();
    let plot = export_hpgl(&sc, &PageSetup::default()).unwrap();
    let r = read_hpgl(&plot.text).unwrap();
    let strokes: Vec<_> = r.strokes.iter().filter(|s| s.pen == 6).collect();
    assert_eq!(strokes.len(), 3, "{:?}", r.selections);
    // Périmètre d'une fente : 2 (L − w) + π w = 9 + 4,712… mm (polyligne, à l'arrondi près).
    let want = 2.0 * (LEN - WID) + std::f64::consts::PI * WID;
    for s in strokes {
        assert!(
            (s.length_mm() - want).abs() < 0.05,
            "{} / {want}",
            s.length_mm()
        );
        assert_eq!(s.pts.first(), s.pts.last(), "contour fermé");
    }
}
