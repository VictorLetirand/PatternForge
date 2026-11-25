//! US-06.1 — export des crans avancés : un cran en V de 4 mm de large et 5 mm de profondeur, posé
//! « sur la coupe » d'un rectangle 100 × 50 mm à 30 mm du coin, avec une marge de 10 mm (puis
//! 15 mm). Valeurs attendues **calculées à la main** : pied sur la coupe en (30 ; -10), pointe en
//! (30 ; -5), pieds du V en (28 ; -10) et (32 ; -10) ; le V se compose de deux segments de
//! √(2² + 5²) = √29 ≈ 5,385 mm. Lecteurs SVG, PDF, DXF et HPGL indépendants des écrivains
//! (`common/`). Rien ici ne prouve l'ouverture des fichiers dans un logiciel tiers (étape humaine).

mod common;

use common::coupe::*;
use common::dxf::{self as rd, Ent};
use common::dxf_check::placed;
use common::dxf_models::rect_piece;
use common::hpgl::read_hpgl;
use pf_core::{
    Command, NotchForm, NotchId, NotchStyle, PathRole, PieceId, Project, SeamAllowance, SheetId,
};
use pf_export::dxf::{export_dxf_with, DxfIssueKind};
use pf_export::hpgl::export_hpgl;
use pf_export::pdf::export_pdf;
use pf_export::svg::export_svg;
use pf_export::{OutputContent, PageSetup, PieceExportMode};
use pf_geometry::Side;

const SQRT29: f64 = 5.385_164_807_134_504;

/// Rectangle 100 × 50 avec une marge optionnelle et un cran en V sur la coupe (ou sur la
/// couture) : le projet, la feuille, la pièce et le cran.
fn scene(margin: Option<f64>, on_cut: bool) -> (Project, SheetId, PieceId, NotchId) {
    let mut p = Project::new("Crans avancés");
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
        set_margin(&mut p, piece, m);
    }
    // Le bord du bas va de (0,0) à (100,0) ou l'inverse : on pose le cran à 30 mm du coin (0,0).
    let sheet = p.sheet(s).unwrap();
    let (edge, at) = edges
        .iter()
        .find_map(|&e| {
            let (a, b) = sheet.segment_ends(e).unwrap();
            if a.y == 0.0 && b.y == 0.0 && a.x == 0.0 && b.x == 100.0 {
                Some((e, 30.0))
            } else if a.y == 0.0 && b.y == 0.0 && a.x == 100.0 && b.x == 0.0 {
                Some((e, 70.0))
            } else {
                None
            }
        })
        .expect("bord du bas");
    let n = p
        .apply(Command::AddNotch {
            sheet: s,
            entity: edge,
            distance_mm: at,
            side: Side::Right,
            length_mm: 5.0,
            style: NotchStyle::Single,
        })
        .unwrap()
        .notches[0];
    p.apply(Command::SetNotchShape {
        sheet: s,
        notch: n,
        form: NotchForm::V,
        width_mm: Some(4.0),
        on_cut,
    })
    .unwrap();
    (p, s, piece, n)
}

fn set_margin(p: &mut Project, piece: PieceId, mm: f64) {
    p.apply(Command::SetSeamAllowance {
        piece,
        seam: Some(SeamAllowance {
            default_mm: mm,
            ..Default::default()
        }),
    })
    .unwrap();
}

/// Les deux segments du V sur la coupe (marge `m`), dans le repère du patron.
fn v_on_cut(m: f64) -> [Seg; 2] {
    [
        ((28.0, -m), (30.0, -m + 5.0)),
        ((30.0, -m + 5.0), (32.0, -m)),
    ]
}

type Seg = ((f64, f64), (f64, f64));

fn ends(segs: &[Seg]) -> Vec<(f64, f64)> {
    segs.iter().flat_map(|s| [s.0, s.1]).collect()
}

// ---------------------------------------------------------------------------
// SVG
// ---------------------------------------------------------------------------

#[test]
fn svg_cran_sur_la_coupe_role_distinct_et_position_a_la_main() {
    let (p, s, _, _) = scene(Some(10.0), true);
    let svg = export_svg(&scene_with(&p, s, PieceExportMode::Both)).unwrap();
    let doc = read_svg(&svg);
    // Emprise inchangée : coupe -10..110 × -10..60 → 140 × 90 ; x' = x + 20, y' = 70 - y.
    assert_eq!((doc.width, doc.height), (140.0, 90.0));
    let marks = doc.with_role("cran-coupe");
    assert_eq!(marks.len(), 2, "deux segments pour un V");
    let got: Vec<(f64, f64)> = marks.iter().flat_map(|m| m.pts.clone()).collect();
    // (28,-10) → (48,80) ; (30,-5) → (50,75) ; (32,-10) → (52,80).
    let want = [(48.0, 80.0), (50.0, 75.0), (50.0, 75.0), (52.0, 80.0)];
    assert!(same_set(&got, &want, 1e-3), "{got:?}");
    // Distinct du cran de rempli et du cran sur la couture ; trait plein (jamais la couleur seule).
    assert!(doc.with_role("cran-rempli").is_empty());
    assert!(doc.with_role("cran").is_empty());
    assert_eq!(marks[0].dash.as_deref(), Some("none"));
    // Marge 15 : le V suit la coupe ; emprise agrandie de 5 mm de chaque côté.
    let (mut p, s, piece, _) = scene(Some(10.0), true);
    set_margin(&mut p, piece, 15.0);
    let doc = read_svg(&export_svg(&scene_with(&p, s, PieceExportMode::Both)).unwrap());
    assert_eq!((doc.width, doc.height), (150.0, 100.0));
    let got: Vec<(f64, f64)> = doc
        .with_role("cran-coupe")
        .iter()
        .flat_map(|m| m.pts.clone())
        .collect();
    // x' = x + 25, y' = 75 - y : (28,-15) → (53,90) ; (30,-10) → (55,85) ; (32,-15) → (57,90).
    let want = [(53.0, 90.0), (55.0, 85.0), (55.0, 85.0), (57.0, 90.0)];
    assert!(same_set(&got, &want, 1e-3), "{got:?}");
}

#[test]
fn svg_couture_seule_n_ecrit_pas_le_cran_sur_la_coupe() {
    let (p, s, _, _) = scene(Some(10.0), true);
    let doc = read_svg(&export_svg(&scene_with(&p, s, PieceExportMode::SewingOnly)).unwrap());
    assert!(doc.with_role("cran-coupe").is_empty());
    assert!(doc.with_role("cran").is_empty());
    let doc = read_svg(&export_svg(&scene_with(&p, s, PieceExportMode::CutOnly)).unwrap());
    assert_eq!(doc.with_role("cran-coupe").len(), 2);
}

#[test]
fn svg_sans_marge_le_cran_sur_la_coupe_est_sur_la_couture() {
    // Pas de marge : la coupe est la couture ; le V part du bord (30,0) vers l'intérieur (+y).
    let (p, s, _, _) = scene(None, true);
    let scn = scene_with(&p, s, PieceExportMode::Both);
    assert!(scn.cut_contours.is_empty());
    let doc = read_svg(&export_svg(&scn).unwrap());
    assert!(doc.with_role("cran-coupe").is_empty());
    // Les traits sont dans le groupe des crans (marge blanche de 10 : x' = x + 10, y' = 60 - y).
    let lines: Vec<&common::coupe::SvgItem> = doc
        .items
        .iter()
        .filter(|i| i.role.is_none() && i.d.is_none() && i.pts.len() == 2 && i.width.is_some())
        .collect();
    assert!(lines.len() >= 2);
    let got: Vec<(f64, f64)> = lines.iter().flat_map(|m| m.pts.clone()).collect();
    for p in [(38.0, 60.0), (40.0, 55.0), (42.0, 60.0)] {
        assert!(
            got.iter()
                .any(|q| (q.0 - p.0).abs() < 1e-3 && (q.1 - p.1).abs() < 1e-3),
            "{p:?} absent de {got:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// PDF
// ---------------------------------------------------------------------------

#[test]
fn pdf_cran_sur_la_coupe_deux_traits_pleins_de_racine_de_29() {
    let (p, s, _, _) = scene(Some(10.0), true);
    let pdf = export_pdf(&[scene_with(&p, s, PieceExportMode::Both)]).unwrap();
    let strokes = read_pdf(&pdf.bytes);
    assert!(strokes.iter().any(|t| t.closed && !t.dash_pt.is_empty()));
    let marks: Vec<&PdfStroke> = strokes
        .iter()
        .filter(|t| !t.closed && t.pts.len() == 2 && t.dash_pt.is_empty())
        .filter(|t| near(stroke_len_pt(t), mm_pt(SQRT29), 5e-3))
        .collect();
    assert_eq!(marks.len(), 2, "{strokes:?}");
    // Les deux segments se rejoignent à la pointe du V.
    let a = marks[0].pts.clone();
    let b = marks[1].pts.clone();
    let shared = a.iter().any(|p| {
        b.iter()
            .any(|q| near(p.0, q.0, 1e-6) && near(p.1, q.1, 1e-6))
    });
    assert!(shared, "pointe commune");
    // Couture seule : aucun cran.
    let pdf = export_pdf(&[scene_with(&p, s, PieceExportMode::SewingOnly)]).unwrap();
    let only: Vec<PdfStroke> = read_pdf(&pdf.bytes);
    assert!(!only
        .iter()
        .any(|t| near(stroke_len_pt(t), mm_pt(SQRT29), 5e-3)));
}

// ---------------------------------------------------------------------------
// DXF
// ---------------------------------------------------------------------------

fn lines_on(dxf: &rd::Dxf, layer: &str) -> Vec<((f64, f64), (f64, f64))> {
    dxf.blocks
        .iter()
        .flat_map(|(_, ents)| ents)
        .filter_map(|e| match e {
            Ent::Line { layer: l, a, b } if l == layer => Some((*a, *b)),
            _ => None,
        })
        .collect()
}

#[test]
fn dxf_cran_sur_la_coupe_couche_4_avec_la_coupe_couche_1() {
    let (p, _, piece, _) = scene(Some(10.0), true);
    let out = export_dxf_with(&p, PieceExportMode::Both).unwrap();
    let dxf = rd::parse(&out.text).unwrap();
    let place = placed(&p, piece);
    let marks = lines_on(&dxf, "4");
    assert_eq!(marks.len(), 2, "{marks:?}");
    let got: Vec<(f64, f64)> = marks.iter().flat_map(|m| [m.0, m.1]).collect();
    let want: Vec<(f64, f64)> = ends(&v_on_cut(10.0))
        .iter()
        .map(|&(x, y)| place(pt(x, y)))
        .collect();
    assert!(same_set(&got, &want, 1e-4), "{got:?} / {want:?}");
    assert_eq!(out.report.omissions().count(), 0);
    // Couches de la coupe (1) et de la couture (15) toujours écrites.
    let cut = dxf
        .blocks
        .iter()
        .flat_map(|(_, e)| e)
        .any(|e| matches!(e, Ent::Polyline { layer, .. } if layer == "1"));
    assert!(cut);
    // Marge 15 : le cran suit.
    let (mut p2, _, piece2, _) = scene(Some(10.0), true);
    set_margin(&mut p2, piece2, 15.0);
    let dxf = rd::parse(&export_dxf_with(&p2, PieceExportMode::Both).unwrap().text).unwrap();
    let place = placed(&p2, piece2);
    let got: Vec<(f64, f64)> = lines_on(&dxf, "4")
        .iter()
        .flat_map(|m| [m.0, m.1])
        .collect();
    let want: Vec<(f64, f64)> = ends(&v_on_cut(15.0))
        .iter()
        .map(|&(x, y)| place(pt(x, y)))
        .collect();
    assert!(same_set(&got, &want, 1e-4), "{got:?} / {want:?}");
}

#[test]
fn dxf_modes_couture_seule_et_coupe_seule() {
    let (p, _, _, _) = scene(Some(10.0), true);
    let dxf = |m| rd::parse(&export_dxf_with(&p, m).unwrap().text).unwrap();
    assert!(lines_on(&dxf(PieceExportMode::SewingOnly), "4").is_empty());
    assert_eq!(lines_on(&dxf(PieceExportMode::CutOnly), "4").len(), 2);
    assert_eq!(lines_on(&dxf(PieceExportMode::Both), "4").len(), 2);
}

#[test]
fn dxf_cran_ordinaire_inchange_sur_la_couche_4() {
    // Le même V posé sur la couture (non projeté) : écrit une fois, sur la couche des crans.
    let (p, _, piece, _) = scene(Some(10.0), false);
    let dxf = rd::parse(&export_dxf_with(&p, PieceExportMode::Both).unwrap().text).unwrap();
    let place = placed(&p, piece);
    // Côté droit du bord du bas : le V est de l'autre côté de la couture, vers (0,-1).
    let want: Vec<(f64, f64)> = [(28.0, 0.0), (30.0, -5.0), (30.0, -5.0), (32.0, 0.0)]
        .iter()
        .map(|&(x, y)| place(pt(x, y)))
        .collect();
    let got: Vec<(f64, f64)> = lines_on(&dxf, "4")
        .iter()
        .flat_map(|m| [m.0, m.1])
        .collect();
    assert_eq!(got.len(), 4);
    assert!(same_set(&got, &want, 1e-4), "{got:?} / {want:?}");
}

// ---------------------------------------------------------------------------
// HPGL
// ---------------------------------------------------------------------------

#[test]
fn hpgl_cran_sur_la_coupe_plume_des_crans() {
    let (p, s, _, _) = scene(Some(10.0), true);
    let text = export_hpgl(
        &scene_with(&p, s, PieceExportMode::Both),
        &PageSetup::default(),
    )
    .unwrap()
    .text;
    let r = read_hpgl(&text).unwrap();
    let notches: Vec<_> = r.strokes.iter().filter(|s| s.pen == 5).collect();
    assert_eq!(notches.len(), 2, "deux segments, plume 5 (crans)");
    for n in &notches {
        assert!(near(n.length_mm(), SQRT29, 0.05), "{}", n.length_mm());
    }
    // La coupe (plume 1) garde son périmètre : 2 × (120 + 70) = 380.
    let cut: f64 = r
        .strokes
        .iter()
        .filter(|s| s.pen == 1)
        .map(|s| s.length_mm())
        .sum();
    assert!(near(cut, 380.0, 0.05), "{cut}");
    let sewing = read_hpgl(
        &export_hpgl(
            &scene_with(&p, s, PieceExportMode::SewingOnly),
            &PageSetup::default(),
        )
        .unwrap()
        .text,
    )
    .unwrap();
    assert!(!sewing.strokes.iter().any(|s| s.pen == 5));
}

// ---------------------------------------------------------------------------
// Contenu de la sortie et échecs
// ---------------------------------------------------------------------------

#[test]
fn le_filtre_des_crans_retire_aussi_les_crans_sur_la_coupe() {
    let (p, s, _, _) = scene(Some(10.0), true);
    let full = scene_with(&p, s, PieceExportMode::Both);
    assert_eq!(full.cut_contours[0].cut_notch_marks.len(), 2);
    let mut content = OutputContent::default();
    content.set(pf_export::OutputCategory::Notches, false);
    let filtered = content.apply(&full).unwrap();
    assert!(filtered.cut_contours[0].cut_notch_marks.is_empty());
    assert_eq!(filtered.cut_contours.len(), 1, "la coupe reste");
}

#[test]
fn projection_impossible_signalee_partout_jamais_en_silence() {
    // Triangle équilatéral : un cran au tout début d'un bord (sommet à 60°) n'a pas de côté
    // extérieur déterminable ; la coupe se calcule, le cran n'est pas exporté, c'est dit.
    let mut p = Project::new("Impossible");
    let s = p.sheets[0].id;
    let (a, b, c) = (
        pt(0.0, 0.0),
        pt(100.0, 0.0),
        pt(50.0, 86.602_540_378_443_86),
    );
    let edges = [
        segment(&mut p, s, a, b),
        segment(&mut p, s, b, c),
        segment(&mut p, s, c, a),
    ];
    let piece = extract(&mut p, s, edges.to_vec(), "Triangle");
    set_margin(&mut p, piece, 8.0);
    let n = p
        .apply(Command::AddNotch {
            sheet: s,
            entity: edges[0],
            distance_mm: 0.0,
            side: Side::Right,
            length_mm: 5.0,
            style: NotchStyle::Single,
        })
        .unwrap()
        .notches[0];
    p.apply(Command::SetNotchShape {
        sheet: s,
        notch: n,
        form: NotchForm::Line,
        width_mm: None,
        on_cut: true,
    })
    .unwrap();
    assert!(p.notch_on_cut(s, n).is_err());
    let scn = scene_with(&p, s, PieceExportMode::Both);
    assert!(scn.cut_contours[0].cut_notch_marks.is_empty());
    let w = scn
        .cut_warnings
        .iter()
        .find(|w| w.cut_notches)
        .expect("avertissement d'export");
    assert!(w.piece == "Triangle" && w.notches_only && !w.reason.is_empty());
    assert!(w.to_string().contains("crans sur la coupe"));
    // Le contour de coupe, lui, est bien exporté.
    assert_eq!(scn.cut_contours.len(), 1);
    // DXF : le cran illisible est compté dans le rapport, la coupe est écrite.
    let out = export_dxf_with(&p, PieceExportMode::Both).unwrap();
    assert!(out
        .report
        .issues
        .iter()
        .any(|i| i.kind == DxfIssueKind::NotchUnreadable && i.count == 1));
    assert!(lines_on(&rd::parse(&out.text).unwrap(), "4").is_empty());
}
