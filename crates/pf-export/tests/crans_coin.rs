//! US-06.1b — export des crans **en coin** : rectangle 100 × 50 mm, deux crans de 5 mm au sommet
//! (100 ; 0), un de chaque côté. La bissectrice est (1 ; −1)/√2 : le trait extérieur va de
//! (100 ; 0) à (100 + 5/√2 ; −5/√2) = (103,5355… ; −3,5355…), le trait intérieur à
//! (100 − 5/√2 ; 5/√2) (valeurs **calculées à la main**). Lecteurs SVG, PDF, DXF et HPGL
//! indépendants des écrivains (`common/`). Rien ici ne prouve l'ouverture des fichiers dans un
//! logiciel tiers (étape humaine).

mod common;

use common::coupe::*;
use common::dxf::{self as rd, Ent};
use common::dxf_check::placed;
use common::dxf_models::rect_piece;
use common::hpgl::read_hpgl;
use pf_core::{
    Command, EntityId, NotchId, NotchStyle, PathRole, PieceId, Project, SeamAllowance, SheetId,
};
use pf_export::dxf::export_dxf_with;
use pf_export::hpgl::export_hpgl;
use pf_export::pdf::export_pdf;
use pf_export::svg::export_svg;
use pf_export::{PageSetup, PieceExportMode};
use pf_geometry::Side;

const H: f64 = 3.535_533_905_932_738; // 5 / √2

/// Rectangle 100 × 50 (marge optionnelle) et, au sommet (100 ; 0), un cran en coin de chaque côté.
fn scene(margin: Option<f64>) -> (Project, SheetId, PieceId, [NotchId; 2]) {
    let mut p = Project::new("Crans en coin");
    let s = p.sheets[0].id;
    let (piece, edges) = rect_piece(
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
    // Le tracé qui touche (100 ; 0) et la position du cran (début ou fin selon son sens).
    let sheet = p.sheet(s).unwrap();
    let (edge, at): (EntityId, f64) = edges
        .iter()
        .find_map(|&e| {
            let (a, b) = sheet.segment_ends(e).unwrap();
            let len = sheet.path_length_mm(e).unwrap();
            if (a.x - 100.0).abs() < 1e-9 && a.y.abs() < 1e-9 {
                Some((e, 0.0))
            } else if (b.x - 100.0).abs() < 1e-9 && b.y.abs() < 1e-9 {
                Some((e, len))
            } else {
                None
            }
        })
        .expect("tracé au sommet (100 ; 0)");
    let mut put = |side| {
        let n = p
            .apply(Command::AddNotch {
                sheet: s,
                entity: edge,
                distance_mm: at,
                side,
                length_mm: 5.0,
                style: NotchStyle::Single,
            })
            .unwrap()
            .notches[0];
        p.apply(Command::SetNotchCorner {
            sheet: s,
            notch: n,
            corner: true,
        })
        .unwrap();
        n
    };
    let a = put(Side::Left);
    let b = put(Side::Right);
    (p, s, piece, [a, b])
}

/// Les deux traits attendus dans le repère du patron (ordre et sens indifférents).
fn want_marks() -> [((f64, f64), (f64, f64)); 2] {
    [
        ((100.0, 0.0), (100.0 + H, -H)),
        ((100.0, 0.0), (100.0 - H, H)),
    ]
}

// ---------------------------------------------------------------------------
// Scène, SVG
// ---------------------------------------------------------------------------

#[test]
fn scene_les_traits_suivent_la_bissectrice_sur_la_couture() {
    for margin in [None, Some(10.0)] {
        let (p, s, _, _) = scene(margin);
        let scn = scene_with(&p, s, PieceExportMode::Both);
        assert_eq!(scn.notch_marks.len(), 2, "{margin:?}");
        let got: Vec<(f64, f64)> = scn
            .notch_marks
            .iter()
            .flat_map(|m| [(m.0.x, m.0.y), (m.1.x, m.1.y)])
            .collect();
        let want: Vec<(f64, f64)> = want_marks().iter().flat_map(|m| [m.0, m.1]).collect();
        assert!(same_set(&got, &want, 1e-9), "{margin:?} : {got:?}");
        // Jamais sur le contour de coupe (combinaison « sur la coupe » refusée) : aucun trait ici.
        assert!(scn
            .cut_contours
            .iter()
            .all(|c| c.cut_notch_marks.is_empty()));
        assert!(scn.cut_warnings.is_empty());
    }
}

#[test]
fn svg_deux_traits_de_5_mm_a_45_degres_au_sommet_du_contour() {
    let (p, s, _, _) = scene(None);
    let doc = read_svg(&export_svg(&scene_with(&p, s, PieceExportMode::Both)).unwrap());
    // Traits de cran : `<line>` de 5 mm de long (le repère SVG a l'axe y vers le bas).
    let lines: Vec<&SvgItem> = doc
        .items
        .iter()
        .filter(|i| i.d.is_none() && i.pts.len() == 2)
        .filter(|i| {
            let (a, b) = (i.pts[0], i.pts[1]);
            near((b.0 - a.0).hypot(b.1 - a.1), 5.0, 1e-3)
        })
        .collect();
    assert_eq!(lines.len(), 2, "{:?}", doc.items);
    // Le pied est commun, sur le sommet bas-droit du contour (plus grand x et plus grand y).
    let contour: Vec<(f64, f64)> = doc
        .items
        .iter()
        .filter(|i| {
            i.pts
                .windows(2)
                .any(|w| (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1) > 6.0)
        })
        .flat_map(|i| i.pts.clone())
        .collect();
    let vx = contour.iter().map(|q| q.0).fold(f64::MIN, f64::max);
    let vy = contour.iter().map(|q| q.1).fold(f64::MIN, f64::max);
    let mut tips = Vec::new();
    for l in &lines {
        let foot = [l.pts[0], l.pts[1]]
            .into_iter()
            .find(|q| near(q.0, vx, 1e-3) && near(q.1, vy, 1e-3))
            .unwrap_or_else(|| panic!("pied hors du sommet ({vx} ; {vy}) : {:?}", l.pts));
        let tip = if foot == l.pts[0] { l.pts[1] } else { l.pts[0] };
        tips.push((tip.0 - foot.0, tip.1 - foot.1));
    }
    // y inversé : extérieur (1 ; −1) → (+H ; +H), intérieur (−1 ; 1) → (−H ; −H).
    assert!(same_set(&tips, &[(H, H), (-H, -H)], 1e-3), "{tips:?}");
}

// ---------------------------------------------------------------------------
// PDF
// ---------------------------------------------------------------------------

#[test]
fn pdf_deux_traits_pleins_de_5_mm_opposes() {
    let (p, s, _, _) = scene(None);
    let pdf = export_pdf(&[scene_with(&p, s, PieceExportMode::Both)]).unwrap();
    let strokes = read_pdf(&pdf.bytes);
    let marks: Vec<&PdfStroke> = strokes
        .iter()
        .filter(|t| !t.closed && t.pts.len() == 2 && t.dash_pt.is_empty())
        .filter(|t| near(stroke_len_pt(t), mm_pt(5.0), 5e-3))
        .collect();
    assert_eq!(marks.len(), 2, "{strokes:?}");
    // Pied commun = sommet du contour fermé ; les deux traits partent dans des sens opposés.
    let contour: Vec<(f64, f64)> = strokes
        .iter()
        .filter(|t| stroke_len_pt(t) > mm_pt(6.0))
        .flat_map(|t| t.pts.clone())
        .collect();
    assert!(contour.len() >= 4, "contour lu : {contour:?}");
    let (a, b) = (&marks[0].pts, &marks[1].pts);
    let foot = a
        .iter()
        .find(|p| {
            b.iter()
                .any(|q| near(p.0, q.0, 1e-6) && near(p.1, q.1, 1e-6))
        })
        .expect("pied commun");
    assert!(
        contour
            .iter()
            .any(|c| near(c.0, foot.0, 5e-3) && near(c.1, foot.1, 5e-3)),
        "pied sur un sommet du contour"
    );
    let tip = |m: &Vec<(f64, f64)>| {
        let t = if near(m[0].0, foot.0, 1e-6) && near(m[0].1, foot.1, 1e-6) {
            m[1]
        } else {
            m[0]
        };
        (t.0 - foot.0, t.1 - foot.1)
    };
    let (ta, tb) = (tip(a), tip(b));
    assert!(
        near(ta.0, -tb.0, 1e-3) && near(ta.1, -tb.1, 1e-3),
        "{ta:?} {tb:?}"
    );
    // 45° : |dx| = |dy| = 5/√2 mm.
    assert!(near(ta.0.abs(), mm_pt(H), 5e-3) && near(ta.1.abs(), mm_pt(H), 5e-3));
}

// ---------------------------------------------------------------------------
// DXF
// ---------------------------------------------------------------------------

#[test]
fn dxf_deux_traits_sur_la_couche_des_crans_positions_a_la_main() {
    let (p, _, piece, _) = scene(None);
    let out = export_dxf_with(&p, PieceExportMode::Both).unwrap();
    let dxf = rd::parse(&out.text).unwrap();
    let place = placed(&p, piece);
    let marks: Vec<((f64, f64), (f64, f64))> = dxf
        .blocks
        .iter()
        .flat_map(|(_, ents)| ents)
        .filter_map(|e| match e {
            Ent::Line { layer, a, b } if layer == "4" => Some((*a, *b)),
            _ => None,
        })
        .collect();
    assert_eq!(marks.len(), 2, "{marks:?}");
    let got: Vec<(f64, f64)> = marks.iter().flat_map(|m| [m.0, m.1]).collect();
    let want: Vec<(f64, f64)> = want_marks()
        .iter()
        .flat_map(|m| [m.0, m.1])
        .map(|(x, y)| place(pt(x, y)))
        .collect();
    assert!(same_set(&got, &want, 1e-4), "{got:?} / {want:?}");
    assert_eq!(out.report.omissions().count(), 0);
}

// ---------------------------------------------------------------------------
// HPGL
// ---------------------------------------------------------------------------

#[test]
fn hpgl_deux_traits_de_5_mm_plume_des_crans() {
    let (p, s, _, _) = scene(None);
    let text = export_hpgl(
        &scene_with(&p, s, PieceExportMode::Both),
        &PageSetup::default(),
    )
    .unwrap()
    .text;
    let r = read_hpgl(&text).unwrap();
    let notches: Vec<_> = r.strokes.iter().filter(|s| s.pen == 5).collect();
    assert_eq!(notches.len(), 2, "deux traits, plume 5 (crans)");
    for n in &notches {
        assert!(near(n.length_mm(), 5.0, 0.05), "{}", n.length_mm());
        // 40 unités / mm : (±141,42 ; ±141,42) à l'arrondi près.
        let (a, b) = (n.pts[0], n.pts[1]);
        let (dx, dy) = ((b.0 - a.0).abs() as f64, (b.1 - a.1).abs() as f64);
        assert!(
            near(dx, 40.0 * H, 1.5) && near(dy, 40.0 * H, 1.5),
            "{dx} {dy}"
        );
    }
    // Pied commun, traits opposés.
    let (a, b) = (&notches[0].pts, &notches[1].pts);
    let foot = a
        .iter()
        .find(|p| b.contains(p))
        .expect("pied commun aux deux traits");
    let tip = |m: &Vec<(i64, i64)>| {
        let t = if m[0] == *foot { m[1] } else { m[0] };
        (t.0 - foot.0, t.1 - foot.1)
    };
    let (ta, tb) = (tip(a), tip(b));
    assert!(
        (ta.0 + tb.0).abs() <= 1 && (ta.1 + tb.1).abs() <= 1,
        "{ta:?} {tb:?}"
    );
}

// ---------------------------------------------------------------------------
// Repli : pas de refus, jamais de perte
// ---------------------------------------------------------------------------

#[test]
fn cran_qui_quitte_son_sommet_est_exporte_comme_cran_ordinaire() {
    let (mut p, s, _, [a, _]) = scene(None);
    // Le cran quitte le sommet (ancrage à 30 % du tracé) : la commande passe, l'export se replie
    // sur la normale du tracé, sans avertissement de coupe ni refus.
    p.apply(Command::SetNotchAnchor {
        sheet: s,
        notch: a,
        anchor: Some(pf_core::NotchAnchor::Ratio { ratio: 0.3 }),
    })
    .unwrap();
    assert!(p.notch_corner(s, a).is_err());
    let scn = scene_with(&p, s, PieceExportMode::Both);
    assert_eq!(scn.notch_marks.len(), 2);
    assert!(scn.cut_warnings.is_empty());
    // Le cran de repli est perpendiculaire à son tracé (axes x ou y), pas à 45°.
    let ordinary: Vec<_> = scn
        .notch_marks
        .iter()
        .filter(|m| !((m.0.x - 100.0).abs() < 1e-9 && m.0.y.abs() < 1e-9))
        .collect();
    assert_eq!(ordinary.len(), 1, "{:?}", scn.notch_marks);
    let m = ordinary[0];
    let (dx, dy) = ((m.1.x - m.0.x).abs(), (m.1.y - m.0.y).abs());
    assert!(
        (dx < 1e-9 && (dy - 5.0).abs() < 1e-9) || (dy < 1e-9 && (dx - 5.0).abs() < 1e-9),
        "{m:?}"
    );
}
