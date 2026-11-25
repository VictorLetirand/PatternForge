//! US-05.4 — affichage et export de la couture et de la coupe calculée : un test par critère et
//! par format (SVG, PDF, DXF, HPGL). Les valeurs attendues sont calculées **à la main** dans les
//! tests (rectangle 100 × 50 mm avec une marge de 10 mm : coupe 120 × 70 mm, coins en onglet,
//! périmètre 380 mm, aire 8 400 mm² ; quart de disque de rayon 60 avec un rempli de 12 mm : arc de
//! coupe de rayon 72 mm) ; les lecteurs SVG, PDF, DXF et HPGL sont écrits dans les tests (ou dans
//! `common/`) et ne partagent rien avec les écrivains.
//!
//! Honnêteté : rien ici ne prouve l'ouverture des fichiers dans un logiciel de patronage ou de
//! découpe, ni une impression réelle (étapes humaines).

mod common;

use common::dxf::{self as rd, Ent};
use common::dxf_check::placed;
use common::hpgl::read_hpgl;
use pf_core::PathRole;
use pf_export::dxf::{export_dxf, export_dxf_with, DxfIssueKind};
use pf_export::hpgl::export_hpgl;
use pf_export::pdf::export_pdf;
use pf_export::svg::export_svg;
use pf_export::{OutputCategory, OutputContent, PageSetup, PieceExportMode, Scene, SceneOptions};

use common::coupe::*;

// ---------------------------------------------------------------------------
// Mode d'export
// ---------------------------------------------------------------------------

#[test]
fn le_mode_par_defaut_est_les_deux_et_les_cles_sont_stables() {
    assert_eq!(PieceExportMode::default(), PieceExportMode::Both);
    assert_eq!(SceneOptions::default().piece_mode, PieceExportMode::Both);
    let keys: Vec<&str> = MODES.iter().map(|m| m.key()).collect();
    assert_eq!(keys, ["both", "sewing", "cut"]);
    for m in MODES {
        assert_eq!(PieceExportMode::from_key(m.key()), Some(m));
    }
    assert_eq!(PieceExportMode::from_key("x"), None);
    assert!(PieceExportMode::Both.includes_sewing() && PieceExportMode::Both.includes_cut());
    assert!(PieceExportMode::SewingOnly.includes_sewing());
    assert!(!PieceExportMode::SewingOnly.includes_cut());
    assert!(!PieceExportMode::CutOnly.includes_sewing());
    assert!(PieceExportMode::CutOnly.includes_cut());
}

#[test]
fn le_style_du_contour_calcule_differe_de_tous_les_roles() {
    let ours = (
        pf_export::COMPUTED_CUT_STROKE_MM,
        pf_export::COMPUTED_CUT_DASH_MM,
    );
    // Trait long / trait court, comme à l'écran.
    assert_eq!(ours.1, &[10.0, 2.5, 2.5, 2.5]);
    for role in PathRole::ALL {
        let st = pf_export::stroke_style(role);
        assert!(
            st.dash_mm != ours.1,
            "le pointillé du contour calculé ne se distingue pas de {role:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// SVG
// ---------------------------------------------------------------------------

#[test]
fn svg_les_deux_coupe_120_sur_70_et_couture_100_sur_50() {
    let (p, s, _) = rect_project(Some(10.0));
    let svg = export_svg(&scene_with(&p, s, PieceExportMode::Both)).unwrap();
    let doc = read_svg(&svg);
    // Emprise = coupe : -10..110 × -10..60, plus 10 mm de marge blanche de chaque côté.
    assert_eq!((doc.width, doc.height), (140.0, 90.0));
    // Repère SVG : x' = x + 20, y' = 70 - y + 10 + ... calculé à la main par coin (voir plus bas).
    let cut = doc.with_role("coupe-calculee");
    assert_eq!(cut.len(), 1);
    let expected_cut = [(10.0, 80.0), (130.0, 80.0), (130.0, 10.0), (10.0, 10.0)];
    assert!(
        same_set(&cut[0].pts, &expected_cut, 1e-9),
        "{:?}",
        cut[0].pts
    );
    assert!(cut[0].d.as_ref().unwrap().ends_with(" Z"), "chemin fermé");
    let sewing = doc.with_role("couture");
    assert_eq!(sewing.len(), 4);
    // Les quatre côtés de couture : sommets (0,0) (100,0) (100,50) (0,50) -> (20,70) (120,70)
    // (120,20) (20,20).
    let mut ends: Vec<(f64, f64)> = sewing.iter().flat_map(|i| i.pts.clone()).collect();
    ends.dedup();
    assert!(same_set(
        &ends,
        &[(20.0, 70.0), (120.0, 70.0), (120.0, 20.0), (20.0, 20.0)],
        1e-9
    ));
}

#[test]
fn svg_motif_du_trait_distinct_couture_et_coupe() {
    let (p, s, _) = rect_project(Some(10.0));
    let doc = read_svg(&export_svg(&scene_with(&p, s, PieceExportMode::Both)).unwrap());
    let cut = doc.with_role("coupe-calculee")[0].clone();
    let sewing = doc.with_role("couture")[0].clone();
    assert_eq!(cut.dash.as_deref(), Some("10 2.5 2.5 2.5"));
    assert_eq!(cut.width.as_deref(), Some("0.6"));
    assert_eq!(sewing.dash.as_deref(), Some("3 2"));
    assert_ne!(cut.dash, sewing.dash, "jamais la couleur seule");
}

#[test]
fn svg_couture_seule_est_identique_a_la_piece_sans_marge() {
    let (with_seam, s, _) = rect_project(Some(10.0));
    let (without, s2, _) = rect_project(None);
    let a = export_svg(&scene_with(&with_seam, s, PieceExportMode::SewingOnly)).unwrap();
    let b = export_svg(&scene_with(&without, s2, PieceExportMode::SewingOnly)).unwrap();
    assert_eq!(a, b, "couture seule = export d'avant, à l'octet");
    let doc = read_svg(&a);
    assert!(doc.with_role("coupe-calculee").is_empty());
    assert_eq!((doc.width, doc.height), (120.0, 70.0));
}

#[test]
fn svg_coupe_seule_n_ecrit_pas_la_couture() {
    let (p, s, _) = rect_project(Some(10.0));
    let doc = read_svg(&export_svg(&scene_with(&p, s, PieceExportMode::CutOnly)).unwrap());
    assert!(doc.with_role("couture").is_empty());
    assert_eq!(doc.with_role("coupe-calculee").len(), 1);
    assert_eq!((doc.width, doc.height), (140.0, 90.0));
}

#[test]
fn svg_le_defaut_est_les_deux() {
    let (p, s, _) = rect_project(Some(10.0));
    let default = export_svg(&Scene::from_sheet(&p, s).unwrap()).unwrap();
    let both = export_svg(&scene_with(&p, s, PieceExportMode::Both)).unwrap();
    assert_eq!(default, both);
}

#[test]
fn svg_crans_de_rempli_sur_l_arc_de_coupe_de_rayon_72() {
    let (p, s, piece) = quarter_fold();
    let notches = p.piece_fold_notches(piece).unwrap();
    assert_eq!(notches.len(), 3, "trois crans demandés");
    let scene = scene_with(&p, s, PieceExportMode::Both);
    assert_eq!(scene.cut_contours.len(), 1);
    let svg = export_svg(&scene).unwrap();
    let doc = read_svg(&svg);
    // Emprise : de x = 0 à 72 (arc de coupe), y de 0 à 72 ; coin du contour (0,0).
    // Le segment du bas va de (0,0) à (60,0) ; l'arc de coupe a pour rayon 60 + 12 = 72.
    let cut = doc.with_role("coupe-calculee");
    assert_eq!(cut.len(), 1);
    assert!(
        cut[0].d.as_ref().unwrap().contains("A 72 72"),
        "arc de rayon 72 : {:?}",
        cut[0].d
    );
    let marks = doc.with_role("cran-rempli");
    assert_eq!(marks.len(), 3);
    // Emprise de la scène : x de 0 à 72 et y de 0 à 72 (arc de coupe de rayon 72, marges nulles
    // sur les deux segments). Le centre (0 ; 0) est donc en (0 + 10 ; 72 - 0 + 10) = (10 ; 82)
    // dans le repère SVG (marge blanche de 10 mm, axe Y retourné).
    let centre = (10.0, 82.0);
    // Chaque cran : pied à 72 mm du centre, pointe à 67 mm, sur le même rayon.
    for m in marks {
        let (foot, tip) = (m.pts[0], m.pts[1]);
        let r_foot = (foot.0 - centre.0).hypot(foot.1 - centre.1);
        let r_tip = (tip.0 - centre.0).hypot(tip.1 - centre.1);
        assert!(near(r_foot, 72.0, 1e-3), "pied à {r_foot}");
        assert!(near(r_tip, 67.0, 1e-3), "pointe à {r_tip}");
        // Radial : pied, pointe et centre alignés.
        let cross =
            (foot.0 - centre.0) * (tip.1 - centre.1) - (foot.1 - centre.1) * (tip.0 - centre.0);
        assert!(cross.abs() / (r_foot * r_tip) < 1e-4, "trait non radial");
    }
}

// ---------------------------------------------------------------------------
// PDF
// ---------------------------------------------------------------------------

#[test]
fn pdf_coupe_120_sur_70_en_points_1_pour_1() {
    let (p, s, _) = rect_project(Some(10.0));
    let pdf = export_pdf(&[scene_with(&p, s, PieceExportMode::Both)]).unwrap();
    let strokes = read_pdf(&pdf.bytes);
    let cut: Vec<&PdfStroke> = strokes.iter().filter(|t| t.closed).collect();
    assert_eq!(cut.len(), 1, "un seul chemin fermé : la coupe");
    let cut = cut[0];
    // `m` puis quatre `l` : le dernier retourne au premier sommet, puis `h`.
    assert_eq!(cut.pts.len(), 5);
    assert_eq!(cut.pts[0], cut.pts[4]);
    // Côtés 120 et 70 mm : 340,1575 pt et 198,4252 pt (72 / 25,4 écrit en dur).
    let (w, h) = (mm_pt(120.0), mm_pt(70.0));
    let xs: Vec<f64> = cut.pts.iter().map(|q| q.0).collect();
    let ys: Vec<f64> = cut.pts.iter().map(|q| q.1).collect();
    let span = |v: &[f64]| {
        v.iter().cloned().fold(f64::MIN, f64::max) - v.iter().cloned().fold(f64::MAX, f64::min)
    };
    assert!(near(span(&xs), w, 2e-3), "largeur {}", span(&xs));
    assert!(near(span(&ys), h, 2e-3), "hauteur {}", span(&ys));
    let left = xs.iter().cloned().fold(f64::MAX, f64::min);
    // Périmètre 380 mm.
    assert!(near(stroke_len_pt(cut), mm_pt(380.0), 5e-3));
    // La couture (traits en pointillé 3 / 2) est décalée de 10 mm vers l'intérieur de la coupe,
    // sur chacun des quatre bords : x de 0 à 100, y de 0 à 50 contre -10..110 et -10..60.
    let sewing: Vec<&PdfStroke> = strokes.iter().filter(|t| t.dash_pt.len() == 2).collect();
    assert_eq!(sewing.len(), 4);
    let sx: Vec<f64> = sewing
        .iter()
        .flat_map(|t| t.pts.iter().map(|q| q.0))
        .collect();
    let sy: Vec<f64> = sewing
        .iter()
        .flat_map(|t| t.pts.iter().map(|q| q.1))
        .collect();
    let min = |v: &[f64]| v.iter().cloned().fold(f64::MAX, f64::min);
    assert!(near(min(&sx) - left, mm_pt(10.0), 2e-3));
    assert!(near(min(&sy) - min(&ys), mm_pt(10.0), 2e-3));
    assert!(near(span(&sx), mm_pt(100.0), 2e-3));
    assert!(near(span(&sy), mm_pt(50.0), 2e-3));
}

#[test]
fn pdf_motif_de_la_coupe_en_points_distinct_de_la_couture() {
    let (p, s, _) = rect_project(Some(10.0));
    let pdf = export_pdf(&[scene_with(&p, s, PieceExportMode::Both)]).unwrap();
    let strokes = read_pdf(&pdf.bytes);
    let cut = strokes.iter().find(|t| t.closed).unwrap();
    let expected: Vec<f64> = [10.0, 2.5, 2.5, 2.5].iter().map(|v| mm_pt(*v)).collect();
    assert_eq!(cut.dash_pt.len(), 4);
    for (a, b) in cut.dash_pt.iter().zip(&expected) {
        assert!(near(*a, *b, 2e-4), "{a} / {b}");
    }
    assert!(near(cut.width_pt, mm_pt(0.6), 2e-4));
    let sewing = strokes
        .iter()
        .find(|t| !t.closed && t.dash_pt.len() == 2)
        .expect("couture en pointillé 3 / 2");
    assert!(near(sewing.dash_pt[0], mm_pt(3.0), 2e-4));
    assert!(near(sewing.dash_pt[1], mm_pt(2.0), 2e-4));
}

#[test]
fn pdf_couture_seule_identique_a_la_piece_sans_marge_et_coupe_seule_sans_couture() {
    let (with_seam, s, _) = rect_project(Some(10.0));
    let (without, s2, _) = rect_project(None);
    let a = export_pdf(&[scene_with(&with_seam, s, PieceExportMode::SewingOnly)]).unwrap();
    let b = export_pdf(&[scene_with(&without, s2, PieceExportMode::SewingOnly)]).unwrap();
    assert_eq!(a.bytes, b.bytes);
    let cut_only = read_pdf(
        &export_pdf(&[scene_with(&with_seam, s, PieceExportMode::CutOnly)])
            .unwrap()
            .bytes,
    );
    assert!(cut_only.iter().any(|t| t.closed));
    assert!(
        !cut_only.iter().any(|t| t.dash_pt.len() == 2),
        "aucun trait de couture (pointillé 3 / 2)"
    );
}

#[test]
fn pdf_crans_de_rempli_traits_pleins_sur_le_contour_de_coupe() {
    let (p, s, _) = quarter_fold();
    let pdf = export_pdf(&[scene_with(&p, s, PieceExportMode::Both)]).unwrap();
    let strokes = read_pdf(&pdf.bytes);
    let cut = strokes.iter().find(|t| t.closed).unwrap();
    assert!(cut.curves >= 1, "l'arc de coupe est écrit en Béziers");
    // Les crans : traits pleins de 5 mm (pied à 72, pointe à 67).
    let notches: Vec<&PdfStroke> = strokes
        .iter()
        .filter(|t| !t.closed && t.pts.len() == 2 && t.dash_pt.is_empty())
        .filter(|t| near(stroke_len_pt(t), mm_pt(5.0), 5e-3))
        .collect();
    assert_eq!(notches.len(), 3);
}

// ---------------------------------------------------------------------------
// DXF
// ---------------------------------------------------------------------------

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
fn dxf_couche_15_couture_et_couche_1_coupe() {
    let (p, _, piece) = rect_project(Some(10.0));
    let out = export_dxf_with(&p, PieceExportMode::Both).unwrap();
    let dxf = rd::parse(&out.text).unwrap();
    let cut = polys(&dxf, "1");
    let sewing = polys(&dxf, "15");
    assert_eq!((cut.len(), sewing.len()), (1, 1));
    // Aires : 120 × 70 = 8 400 ; 100 × 50 = 5 000.
    assert!(near(rd::area(&cut[0]).abs(), 8400.0, 1e-6));
    assert!(near(rd::area(&sewing[0]).abs(), 5000.0, 1e-6));
    assert!(near(rd::perimeter(&cut[0], true), 380.0, 1e-6));
    // Sommets de coupe = coins calculés à la main, placés comme la pièce.
    let place = placed(&p, piece);
    let expected: Vec<(f64, f64)> = [(-10.0, -10.0), (110.0, -10.0), (110.0, 60.0), (-10.0, 60.0)]
        .iter()
        .map(|&(x, y)| place(pt(x, y)))
        .collect();
    assert_eq!(cut[0].len(), 4);
    assert!(same_set(&cut[0], &expected, 1e-4), "{:?}", cut[0]);
    // Chaque sommet de couture est à 10 mm du contour de coupe.
    for v in &sewing[0] {
        assert!(near(rd::dist_to_polyline(*v, &cut[0], true), 10.0, 1e-6));
    }
    // Rapport : couches 1 et 15 écrites, aucune omission.
    assert!(out.report.layers.iter().any(|l| l.layer.number == 1));
    assert!(out.report.layers.iter().any(|l| l.layer.number == 15));
    assert_eq!(out.report.omissions().count(), 0);
    assert!(!out
        .report
        .issues
        .iter()
        .any(|i| i.kind == DxfIssueKind::NoCutContour));
}

#[test]
fn dxf_couture_seule_et_coupe_seule() {
    let (p, _, _) = rect_project(Some(10.0));
    let sewing_only = rd::parse(
        &export_dxf_with(&p, PieceExportMode::SewingOnly)
            .unwrap()
            .text,
    )
    .unwrap();
    assert!(polys(&sewing_only, "1").is_empty());
    assert_eq!(polys(&sewing_only, "15").len(), 1);
    let report = export_dxf_with(&p, PieceExportMode::SewingOnly)
        .unwrap()
        .report;
    assert!(
        report
            .issues
            .iter()
            .any(|i| i.kind == DxfIssueKind::NoCutContour),
        "remarque : aucun contour de coupe"
    );
    let cut_only = rd::parse(&export_dxf_with(&p, PieceExportMode::CutOnly).unwrap().text).unwrap();
    assert_eq!(polys(&cut_only, "1").len(), 1);
    assert!(polys(&cut_only, "15").is_empty());
}

#[test]
fn dxf_le_defaut_est_les_deux_et_la_piece_sans_marge_ne_change_pas() {
    let (p, _, _) = rect_project(Some(10.0));
    assert_eq!(
        export_dxf(&p).unwrap().text,
        export_dxf_with(&p, PieceExportMode::Both).unwrap().text
    );
    // Sans marge : même fichier quel que soit le mode, une seule polyligne (couche 15).
    let (q, _, _) = rect_project(None);
    let base = export_dxf(&q).unwrap().text;
    for m in MODES {
        assert_eq!(export_dxf_with(&q, m).unwrap().text, base, "{m:?}");
    }
    let dxf = rd::parse(&base).unwrap();
    assert_eq!(polys(&dxf, "15").len(), 1);
    assert!(polys(&dxf, "1").is_empty());
}

#[test]
fn dxf_crans_de_rempli_sur_la_couche_4() {
    let (p, _, piece) = quarter_fold();
    let dxf = rd::parse(&export_dxf_with(&p, PieceExportMode::Both).unwrap().text).unwrap();
    let marks = lines_on(&dxf, "4");
    assert_eq!(marks.len(), 3);
    // Longueur 5 mm, pied à 72 mm du coin du quartier (la couture est en (0,0)).
    let place = placed(&p, piece);
    let centre = place(pt(0.0, 0.0));
    for (a, b) in marks {
        assert!(near(rd::dist(a, b), 5.0, 1e-4));
        let (ra, rb) = (rd::dist(a, centre), rd::dist(b, centre));
        let (far, near_) = if ra > rb { (ra, rb) } else { (rb, ra) };
        assert!(
            near(far, 72.0, 1e-3) && near(near_, 67.0, 1e-3),
            "{far} {near_}"
        );
    }
}

// ---------------------------------------------------------------------------
// HPGL
// ---------------------------------------------------------------------------

#[test]
fn hpgl_coupe_plume_1_couture_plume_2() {
    let (p, s, _) = rect_project(Some(10.0));
    let text = export_hpgl(
        &scene_with(&p, s, PieceExportMode::Both),
        &PageSetup::default(),
    )
    .unwrap()
    .text;
    let r = read_hpgl(&text).unwrap();
    let cut: f64 = r
        .strokes
        .iter()
        .filter(|s| s.pen == 1)
        .map(|s| s.length_mm())
        .sum();
    let sewing: f64 = r
        .strokes
        .iter()
        .filter(|s| s.pen == 2)
        .map(|s| s.length_mm())
        .sum();
    assert!(near(cut, 380.0, 0.05), "coupe {cut}");
    assert!(near(sewing, 300.0, 0.05), "couture {sewing}");
    // Les modes : couture seule sans plume 1, coupe seule sans plume 2.
    let only = |m| {
        read_hpgl(
            &export_hpgl(&scene_with(&p, s, m), &PageSetup::default())
                .unwrap()
                .text,
        )
        .unwrap()
    };
    assert!(!only(PieceExportMode::SewingOnly)
        .strokes
        .iter()
        .any(|s| s.pen == 1));
    assert!(!only(PieceExportMode::CutOnly)
        .strokes
        .iter()
        .any(|s| s.pen == 2));
}

#[test]
fn hpgl_crans_de_rempli_plume_5() {
    let (p, s, _) = quarter_fold();
    let text = export_hpgl(
        &scene_with(&p, s, PieceExportMode::Both),
        &PageSetup::default(),
    )
    .unwrap()
    .text;
    let r = read_hpgl(&text).unwrap();
    let notches: Vec<_> = r.strokes.iter().filter(|s| s.pen == 5).collect();
    assert_eq!(notches.len(), 3);
    for n in notches {
        assert!(near(n.length_mm(), 5.0, 0.05));
    }
}

// ---------------------------------------------------------------------------
// Pièce sans marge : aucun doublon
// ---------------------------------------------------------------------------

#[test]
fn sans_marge_les_trois_modes_donnent_le_meme_export_sans_doublon() {
    let (p, s, _) = rect_project(None);
    let base = scene_with(&p, s, PieceExportMode::Both);
    assert!(base.cut_contours.is_empty());
    let svg = export_svg(&base).unwrap();
    let pdf = export_pdf(std::slice::from_ref(&base)).unwrap().bytes;
    let plt = export_hpgl(&base, &PageSetup::default()).unwrap().text;
    for m in MODES {
        let sc = scene_with(&p, s, m);
        assert_eq!(sc, base, "{m:?}");
        assert_eq!(export_svg(&sc).unwrap(), svg);
        assert_eq!(export_pdf(std::slice::from_ref(&sc)).unwrap().bytes, pdf);
        assert_eq!(export_hpgl(&sc, &PageSetup::default()).unwrap().text, plt);
    }
    // Quatre côtés de couture, pas huit.
    assert_eq!(read_svg(&svg).with_role("couture").len(), 4);
}

// ---------------------------------------------------------------------------
// Coupe impossible : avertissement lisible, jamais de panique, fichier non silencieux
// ---------------------------------------------------------------------------

#[test]
fn coupe_impossible_avertissement_et_couture_conservee() {
    let (p, s, _) = impossible_cut();
    for m in [PieceExportMode::Both, PieceExportMode::CutOnly] {
        let scene = scene_with(&p, s, m);
        assert!(scene.cut_contours.is_empty());
        assert_eq!(scene.cut_warnings.len(), 1, "{m:?}");
        let w = &scene.cut_warnings[0];
        assert_eq!(w.piece, "Encoche");
        assert!(!w.reason.is_empty() && !w.notches_only);
        assert_eq!(w.sewing_kept, m == PieceExportMode::CutOnly);
        let msg = w.to_string();
        assert!(
            msg.contains("Encoche") && msg.contains("contour de coupe"),
            "{msg}"
        );
        // La couture est exportée (8 côtés).
        let doc = read_svg(&export_svg(&scene).unwrap());
        assert_eq!(doc.with_role("couture").len(), 8);
        assert!(doc.with_role("coupe-calculee").is_empty());
    }
    // Couture seule : aucun calcul, donc aucun avertissement.
    assert!(scene_with(&p, s, PieceExportMode::SewingOnly)
        .cut_warnings
        .is_empty());
}

#[test]
fn coupe_impossible_dxf_signale_l_omission_et_ecrit_la_couture() {
    let (p, _, _) = impossible_cut();
    let out = export_dxf_with(&p, PieceExportMode::Both).unwrap();
    assert!(out
        .report
        .omissions()
        .any(|i| i.kind == DxfIssueKind::CutContourFailed && i.piece == "Encoche"));
    let dxf = rd::parse(&out.text).unwrap();
    assert_eq!(polys(&dxf, "15").len(), 1);
    assert!(polys(&dxf, "1").is_empty());
    // En coupe seule, la couture est écrite à la place (omission signalée).
    let cut_only = export_dxf_with(&p, PieceExportMode::CutOnly).unwrap();
    assert_eq!(polys(&rd::parse(&cut_only.text).unwrap(), "15").len(), 1);
    assert!(cut_only
        .report
        .omissions()
        .any(|i| i.kind == DxfIssueKind::CutContourFailed));
}

#[test]
fn coupe_impossible_hpgl_et_pdf_sans_panique() {
    let (p, s, _) = impossible_cut();
    let scene = scene_with(&p, s, PieceExportMode::Both);
    assert!(export_hpgl(&scene, &PageSetup::default()).is_ok());
    assert!(export_pdf(&[scene]).is_ok());
}

// ---------------------------------------------------------------------------
// Filtre de contenu (US-13.4) : la coupe calculée appartient à la catégorie coupe
// ---------------------------------------------------------------------------

#[test]
fn le_filtre_de_contenu_retire_la_coupe_calculee_avec_la_categorie_coupe() {
    let (p, s, _) = rect_project(Some(10.0));
    let full = scene_with(&p, s, PieceExportMode::Both);
    assert_eq!(
        pf_export::content::count(&full, OutputCategory::CuttingContour),
        1
    );
    let filtered = OutputContent::sewing_only().apply(&full).unwrap();
    assert!(filtered.cut_contours.is_empty());
    let kept = OutputContent::cutting_only().apply(&full).unwrap();
    assert_eq!(kept.cut_contours.len(), 1);
    assert!(OutputContent::default().apply(&full).unwrap() == full);
}
