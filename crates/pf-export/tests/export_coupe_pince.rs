//! PH-5a (CA6) — la coupe « pince fermée » est exportée : PDF, SVG, DXF (couche 1) et HPGL (plume 1)
//! la relisent avec les lecteurs indépendants de `common/`. Valeurs calculées **à la main**
//! (voir `pf-core/tests/coupe_pince_fermee.rs`) : rectangle 100 × 100 mm, marge 10 mm, pince de
//! sommet (50 ; 40), axe vers (50 ; 0), longueur 40, ouverture 10 (tan α = 1/8) : coupe rouverte
//! (−10 ; −10) → J (43,75 ; −10) → J' (56,25 ; −10) → (110 ; −10) → (110 ; 110) → (−10 ; 110).
//!
//! Sans réglage « coupe pince fermée », les octets de chaque export sont ceux de la pièce sans le
//! réglage (réglage posé puis retiré = mêmes octets).
//!
//! Honnêteté : rien ici ne prouve l'ouverture des fichiers dans un logiciel de patronage ou de
//! découpe, ni une impression réelle (étapes humaines).

mod common;

use common::coupe::*;
use common::dxf::{self as rd, Ent};
use common::dxf_check::placed;
use common::dxf_models::rect_piece;
use common::hpgl::read_hpgl;
use pf_core::{Command, DartId, PathRole, PieceId, PointRef, Project, SeamAllowance, SheetId};
use pf_export::dxf::export_dxf_with;
use pf_export::hpgl::export_hpgl;
use pf_export::pdf::export_pdf;
use pf_export::svg::export_svg;
use pf_export::{PageSetup, PieceExportMode};
use pf_geometry::Side;

const REF_CUT: [(f64, f64); 6] = [
    (-10.0, -10.0),
    (43.75, -10.0),
    (56.25, -10.0),
    (110.0, -10.0),
    (110.0, 110.0),
    (-10.0, 110.0),
];

fn project(closed: bool) -> (Project, SheetId, PieceId, DartId) {
    let mut p = Project::new("Coupe pince fermée");
    let s = p.sheets[0].id;
    let (piece, _) = rect_piece(
        &mut p,
        s,
        (0.0, 0.0),
        (100.0, 100.0),
        "Dos",
        PathRole::SewingContour,
        false,
    );
    p.apply(Command::SetSeamAllowance {
        piece,
        seam: Some(SeamAllowance {
            default_mm: 10.0,
            ..Default::default()
        }),
    })
    .unwrap();
    let dart = p
        .apply(Command::AddPieceDart {
            piece,
            apex: PointRef::New(pt(50.0, 40.0)),
            toward: PointRef::New(pt(50.0, 0.0)),
            length_mm: 40.0,
            opening_mm: 10.0,
            fixed: Side::Right,
        })
        .unwrap()
        .darts[0];
    if closed {
        p.apply(Command::SetPieceDartCutClosed {
            piece,
            dart,
            closed: true,
        })
        .unwrap();
    }
    (p, s, piece, dart)
}

fn polys(dxf: &rd::Dxf, layer: &str) -> Vec<Vec<(f64, f64)>> {
    dxf.blocks
        .iter()
        .flat_map(|(_, ents)| ents)
        .filter_map(|e| match e {
            Ent::Polyline {
                layer: l,
                pts,
                closed,
                ..
            } if l == layer => {
                assert!(*closed);
                Some(pts.clone())
            }
            _ => None,
        })
        .collect()
}

#[test]
fn la_scene_porte_la_coupe_pince_fermee() {
    let (p, s, _, _) = project(true);
    let scene = scene_with(&p, s, PieceExportMode::Both);
    assert_eq!(scene.cut_contours.len(), 1);
}

#[test]
fn svg_la_coupe_a_les_six_sommets_de_la_pince_fermee() {
    let (p, s, _, _) = project(true);
    let doc = read_svg(&export_svg(&scene_with(&p, s, PieceExportMode::Both)).unwrap());
    let cut = doc.with_role("coupe-calculee");
    assert_eq!(cut.len(), 1);
    // Repère SVG : x' = x + 20 (coupe de x = −10, plus 10 mm de blanc), y' = 120 − y.
    let expected: Vec<(f64, f64)> = REF_CUT
        .iter()
        .map(|&(x, y)| (x + 20.0, 120.0 - y))
        .collect();
    // Six sommets, le dernier trait revient au premier (chemin fermé).
    assert_eq!(cut[0].pts.len(), 7, "{:?}", cut[0].pts);
    assert_eq!(cut[0].pts[0], cut[0].pts[6]);
    assert!(
        same_set(&cut[0].pts[..6], &expected, 1e-6),
        "{:?}",
        cut[0].pts
    );
}

#[test]
fn pdf_la_coupe_a_les_six_sommets_de_la_pince_fermee() {
    let (p, s, _, _) = project(true);
    let pdf = export_pdf(&[scene_with(&p, s, PieceExportMode::Both)]).unwrap();
    let strokes = read_pdf(&pdf.bytes);
    let cut = strokes.iter().find(|t| t.closed).expect("coupe fermée");
    assert_eq!(cut.pts.len(), 7, "six sommets et le retour au premier");
    assert_eq!(cut.pts[0], cut.pts[6]);
    // Repère du PDF relatif au coin bas-gauche de la coupe : x − (−10), y − (−10), en points.
    let min_x = cut.pts.iter().map(|q| q.0).fold(f64::MAX, f64::min);
    let min_y = cut.pts.iter().map(|q| q.1).fold(f64::MAX, f64::min);
    let got: Vec<(f64, f64)> = cut.pts[..6]
        .iter()
        .map(|q| (q.0 - min_x, q.1 - min_y))
        .collect();
    let expected: Vec<(f64, f64)> = REF_CUT
        .iter()
        .map(|&(x, y)| (mm_pt(x + 10.0), mm_pt(y + 10.0)))
        .collect();
    assert!(same_set(&got, &expected, 3e-3), "{got:?} ≠ {expected:?}");
}

#[test]
fn dxf_couche_1_a_les_six_sommets_de_la_pince_fermee() {
    let (p, _, piece, _) = project(true);
    let out = export_dxf_with(&p, PieceExportMode::Both).unwrap();
    let dxf = rd::parse(&out.text).unwrap();
    let cut = polys(&dxf, "1");
    assert_eq!(cut.len(), 1);
    let place = placed(&p, piece);
    let expected: Vec<(f64, f64)> = REF_CUT.iter().map(|&(x, y)| place(pt(x, y))).collect();
    assert_eq!(cut[0].len(), 6, "{:?}", cut[0]);
    assert!(same_set(&cut[0], &expected, 1e-4), "{:?}", cut[0]);
    // Les jambes de la pince (couche 90) sont toujours écrites ; la couture (couche 15) inchangée.
    assert_eq!(polys(&dxf, "15").len(), 1);
}

#[test]
fn hpgl_plume_1_passe_par_les_six_sommets_de_la_pince_fermee() {
    let (p, s, _, _) = project(true);
    let text = export_hpgl(
        &scene_with(&p, s, PieceExportMode::Both),
        &PageSetup::default(),
    )
    .unwrap()
    .text;
    let r = read_hpgl(&text).unwrap();
    let cut: Vec<&common::hpgl::Stroke> = r.strokes.iter().filter(|s| s.pen == 1).collect();
    assert_eq!(cut.len(), 1);
    // Périmètre 480 mm (coupe 120 × 120) ; sommets relatifs au coin bas-gauche (40 unités / mm).
    assert!(
        near(cut[0].length_mm(), 480.0, 0.1),
        "{}",
        cut[0].length_mm()
    );
    let min_x = cut[0].pts.iter().map(|q| q.0).min().unwrap();
    let min_y = cut[0].pts.iter().map(|q| q.1).min().unwrap();
    let got: Vec<(f64, f64)> = cut[0]
        .pts
        .iter()
        .map(|q| ((q.0 - min_x) as f64 / 40.0, (q.1 - min_y) as f64 / 40.0))
        .collect();
    for &(x, y) in &REF_CUT {
        let want = (x + 10.0, y + 10.0);
        assert!(
            got.iter()
                .any(|g| near(g.0, want.0, 0.03) && near(g.1, want.1, 0.03)),
            "sommet {want:?} absent de {got:?}"
        );
    }
}

#[test]
fn sans_le_reglage_les_octets_sont_ceux_de_la_pince_ordinaire() {
    let (base, s, piece, dart) = project(false);
    let (mut toggled, _, _, _) = project(false);
    toggled
        .apply(Command::SetPieceDartCutClosed {
            piece,
            dart,
            closed: true,
        })
        .unwrap();
    toggled
        .apply(Command::SetPieceDartCutClosed {
            piece,
            dart,
            closed: false,
        })
        .unwrap();
    for mode in MODES {
        assert_eq!(
            export_svg(&scene_with(&base, s, mode)).unwrap(),
            export_svg(&scene_with(&toggled, s, mode)).unwrap(),
            "SVG {mode:?}"
        );
        assert_eq!(
            export_pdf(&[scene_with(&base, s, mode)]).unwrap().bytes,
            export_pdf(&[scene_with(&toggled, s, mode)]).unwrap().bytes,
            "PDF {mode:?}"
        );
        assert_eq!(
            export_dxf_with(&base, mode).unwrap().text,
            export_dxf_with(&toggled, mode).unwrap().text,
            "DXF {mode:?}"
        );
    }
    // Réglé : au moins le SVG et le DXF diffèrent (la pointe de marge est écrite).
    let (closed, _, _, _) = project(true);
    assert_ne!(
        export_svg(&scene_with(&base, s, PieceExportMode::Both)).unwrap(),
        export_svg(&scene_with(&closed, s, PieceExportMode::Both)).unwrap()
    );
    assert_ne!(
        export_dxf_with(&base, PieceExportMode::Both).unwrap().text,
        export_dxf_with(&closed, PieceExportMode::Both)
            .unwrap()
            .text
    );
}
