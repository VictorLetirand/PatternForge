//! US-12.5 — export HPGL / PLT : un test au moins par critère d'acceptation. Le fichier est relu
//! par le lecteur indépendant de `common/hpgl.rs` (il ne partage rien avec l'écrivain).
//!
//! Rien de ceci ne remplace un essai sur un vrai traceur : la fidélité de l'échelle sur machine
//! n'est pas prouvée.

mod common;

use common::hpgl::*;
use common::{pt, rect_at, scene_of, Rng};
use pf_core::PathRole;
use pf_export::hpgl::{export_hpgl, export_hpgl_with, HpglError, HpglOptions, HpglPen};
use pf_export::{
    Margins, Orientation, OutputCategory, OutputContent, PageFormat, PageSetup, PageSetupError,
    PathGeom, PrintScale, ScenePath,
};
use pf_geometry::{Arc, CircleTrace};

fn with_role(role: PathRole, geom: PathGeom) -> ScenePath {
    ScenePath {
        geom,
        role,
        sense: pf_core::GrainSense::Unmarked,
    }
}

fn a4() -> PageSetup {
    PageSetup::default()
}

fn export_text(scene: &pf_export::Scene, page: &PageSetup) -> String {
    export_hpgl(scene, page).expect("export").text
}

// ----- CA1 : écrivain -----------------------------------------------------------------------

#[test]
fn ca1_texte_exact_ascii_unites_de_0_025_mm() {
    let mut s = scene_of(vec![PathGeom::Line(pt(10.0, 20.0), pt(110.012, 20.0))]);
    s.paths[0].role = PathRole::Construction;
    let text = export_text(&s, &a4());
    // 100,012 mm = 4000,48 unités → 4000 ; origine = coin bas-gauche de l'emprise.
    assert_eq!(text, "IN;\nPA;\nSP3;\nPU0,0;\nPD4000,0;\nPU;\nSP0;\n");
    assert!(text.is_ascii());
    // Déterministe à l'octet.
    assert_eq!(text, export_text(&s, &a4()));
}

#[test]
fn ca1_coordonnees_entieres_erreur_au_plus_une_demi_unite() {
    let mut rng = Rng(7);
    for _ in 0..30 {
        let (ax, ay) = (rng.range(-80.0, 80.0), rng.range(-80.0, 80.0));
        let (bx, by) = (ax + rng.range(5.0, 90.0), ay + rng.range(5.0, 90.0));
        let s = scene_of(vec![PathGeom::Line(pt(ax, ay), pt(bx, by))]);
        let r = read_hpgl(&export_text(&s, &a4())).unwrap();
        let p = &r.strokes[0].pts;
        let (w, h) = (p[1].0.max(p[0].0), p[1].1.max(p[0].1));
        // Le coin bas-gauche est en (0 ; 0) ; la largeur écrite diffère de la vraie de ≤ 0,0125 mm.
        assert!((w as f64 / UNITS - (bx - ax)).abs() <= 0.0125 + 1e-9);
        assert!((h as f64 / UNITS - (by - ay)).abs() <= 0.0125 + 1e-9);
    }
}

#[test]
fn ca1_courbe_discretisee_fleche_au_plus_0_01_mm() {
    let c = CircleTrace::new(pt(100.0, 100.0), 50.0, 0.0, true).unwrap();
    let s = scene_of(vec![PathGeom::Circle(c)]);
    let r = read_hpgl(&export_text(&s, &a4())).unwrap();
    let pts: Vec<(f64, f64)> = r.strokes[0]
        .pts
        .iter()
        .map(|p| (p.0 as f64 / UNITS, p.1 as f64 / UNITS))
        .collect();
    assert!(pts.len() > 50);
    let centre = (50.0, 50.0);
    for w in pts.windows(2) {
        let mid = ((w[0].0 + w[1].0) / 2.0, (w[0].1 + w[1].1) / 2.0);
        let sagitta = 50.0 - (mid.0 - centre.0).hypot(mid.1 - centre.1);
        // Flèche vraie ≤ 0,01 mm ; l'arrondi des deux sommets peut y ajouter 0,0125 mm.
        assert!(sagitta <= 0.01 + 0.0125, "flèche {sagitta}");
    }
    for p in &pts {
        let rad = (p.0 - centre.0).hypot(p.1 - centre.1);
        // Arrondi du sommet, plus le décalage de l'origine (le minimum du tracé discrétisé peut
        // manquer le vrai minimum du cercle d'une flèche).
        assert!(
            (rad - 50.0).abs() <= 0.0125 * 2f64.sqrt() + 0.01 + 1e-9,
            "rayon {rad}"
        );
    }
}

#[test]
fn ca1_pas_de_nan_ni_hors_plage() {
    let nan = f64::NAN;
    let bad = [
        PathGeom::Line(pt(0.0, 0.0), pt(nan, 5.0)),
        PathGeom::Line(pt(0.0, 0.0), pt(f64::INFINITY, 5.0)),
        PathGeom::Cubic(pf_geometry::CubicBezier::new(
            pt(0.0, 0.0),
            pt(nan, 1.0),
            pt(5.0, 5.0),
            pt(9.0, 9.0),
        )),
        PathGeom::Arc(Arc {
            center: pt(0.0, 0.0),
            radius_mm: nan,
            start_rad: 0.0,
            sweep_rad: 1.0,
        }),
    ];
    for g in bad {
        let s = scene_of(vec![g.clone(), PathGeom::Line(pt(0.0, 0.0), pt(9.0, 9.0))]);
        assert_eq!(
            export_hpgl(&s, &a4()).unwrap_err(),
            HpglError::NotFinite,
            "{g:?}"
        );
    }
    // 1e6 mm = 4e7 unités : au-delà de la plage écrite.
    let s = scene_of(vec![PathGeom::Line(pt(0.0, 0.0), pt(1.0e6, 0.0))]);
    assert!(matches!(
        export_hpgl(&s, &a4()).unwrap_err(),
        HpglError::OutOfRange { .. }
    ));
    // Une valeur inattendue après le facteur (hors plage de coordonnées) est refusée de même.
    let s = scene_of(vec![PathGeom::Line(pt(0.0, 0.0), pt(1.0e308, 0.0))]);
    assert!(export_hpgl(&s, &a4()).is_err());
}

#[test]
fn ca1_pd_decoupes_et_instructions_limitees() {
    let c = CircleTrace::new(pt(0.0, 0.0), 80.0, 0.0, true).unwrap();
    let text = export_text(&scene_of(vec![PathGeom::Circle(c)]), &a4());
    for line in text.lines() {
        let name = &line[..2];
        assert!(["IN", "PA", "SP", "PU", "PD"].contains(&name), "{line}");
        if name == "PD" {
            let n = line.trim_end_matches(';')[2..].split(',').count();
            assert!(n <= 64, "au plus 32 points par PD : {line}");
        }
    }
    assert!(text.lines().filter(|l| l.starts_with("PD")).count() > 1);
}

// ----- CA2 : plumes -------------------------------------------------------------------------

#[test]
fn ca2_table_des_plumes_unique() {
    for role in PathRole::ALL {
        assert_eq!(
            HpglPen::of_role(role).number(),
            pen_of_role(role),
            "{role:?}"
        );
    }
    let numbers: Vec<u8> = HpglPen::ALL.iter().map(|p| p.number()).collect();
    assert_eq!(numbers, (1..=9).collect::<Vec<u8>>());
}

fn three_roles() -> pf_export::Scene {
    let mut s = scene_of(Vec::new());
    for (role, y) in [
        (PathRole::CuttingContour, 0.0),
        (PathRole::SewingContour, 10.0),
        (PathRole::Construction, 20.0),
        (PathRole::Fold, 30.0),
    ] {
        s.paths
            .push(with_role(role, PathGeom::Line(pt(0.0, y), pt(50.0, y))));
    }
    s
}

#[test]
fn ca2_seules_les_plumes_presentes_une_seule_selection_chacune() {
    let plot = export_hpgl(&three_roles(), &a4()).unwrap();
    let r = read_hpgl(&plot.text).unwrap();
    assert_eq!(r.selections, vec![1, 2, 3, 8]);
    let pens: Vec<u8> = plot.report.pens.iter().map(|p| p.pen.number()).collect();
    assert_eq!(pens, vec![1, 2, 3, 8]);
    assert_eq!(plot.text.matches("SP").count(), 5, "4 plumes + SP0");
    assert!(!plot.text.contains("SP4;") && !plot.text.contains("SP5;"));
    // Rien d'autre que SP : pas de pause inventée.
    assert_eq!(r.commands, plot.text.lines().count());
}

#[test]
fn ca2_crans_marquages_cotes_ont_leur_plume() {
    use pf_core::MarkingShape;
    let mut s = scene_of(vec![PathGeom::Line(pt(0.0, 0.0), pt(50.0, 0.0))]);
    s.notch_marks.push((pt(10.0, 0.0), pt(10.0, 5.0)));
    s.markings.push(pf_export::SceneMarking {
        pos: pt(20.0, 20.0),
        shape: MarkingShape::Cross { size_mm: 6.0 },
    });
    s.dimensions.push(pf_export::SceneDimension {
        from: pt(0.0, 0.0),
        to: pt(50.0, 0.0),
        from_offset: pt(0.0, -8.0),
        to_offset: pt(50.0, -8.0),
        text: "50.00 mm".into(),
    });
    let plot = export_hpgl(&s, &a4()).unwrap();
    let r = read_hpgl(&plot.text).unwrap();
    assert_eq!(r.selections, vec![1, 5, 6, 7]);
    // Textes non tracés, comptés.
    assert_eq!(plot.report.texts_omitted, 1);
}

#[test]
fn ca2_categories_exclues_ne_sont_pas_tracees() {
    let s = three_roles();
    let no_construction = OutputContent::default().without(OutputCategory::Construction);
    let r = read_hpgl(&export_text(&no_construction.apply(&s).unwrap(), &a4())).unwrap();
    assert_eq!(r.selections, vec![1, 2, 8]);
    let r = read_hpgl(&export_text(
        &OutputContent::cutting_only().apply(&s).unwrap(),
        &a4(),
    ))
    .unwrap();
    assert_eq!(r.selections, vec![1, 3, 8], "sans la couture");
    let r = read_hpgl(&export_text(
        &OutputContent::sewing_only().apply(&s).unwrap(),
        &a4(),
    ))
    .unwrap();
    assert_eq!(r.selections, vec![2, 3, 8], "sans la coupe");
    let only_cut = OutputContent::only(&[OutputCategory::CuttingContour]);
    let r = read_hpgl(&export_text(&only_cut.apply(&s).unwrap(), &a4())).unwrap();
    assert_eq!(r.selections, vec![1]);
}

// ----- CA3 : ordre de tracé -----------------------------------------------------------------

#[test]
fn ca3_ordre_optimise_jamais_plus_long_que_l_ordre_naturel_sur_30_cas() {
    let mut strictly_better = 0;
    for seed in 1..=30u64 {
        let mut rng = Rng(seed * 7919);
        let scene = common::hpgl::random_scene(&mut rng, 1.0);
        let opt = export_hpgl_with(&scene, &a4(), HpglOptions { optimize: true }).unwrap();
        let nat = export_hpgl_with(&scene, &a4(), HpglOptions { optimize: false }).unwrap();
        let (ro, rn) = (read_hpgl(&opt.text).unwrap(), read_hpgl(&nat.text).unwrap());
        let (to, tn) = (travel_of(&ro), travel_of(&rn));
        assert!(to <= tn + 1e-9, "graine {seed} : {to} > {tn}");
        // Le rapport dit la même chose que la relecture, et la somme des PU aussi.
        assert!((opt.report.travel_mm - to).abs() < 1e-6, "graine {seed}");
        assert!((ro.pen_up_mm - to).abs() < 1e-6, "graine {seed}");
        assert!((opt.report.natural_travel_mm - tn).abs() < 1e-6);
        // Même géométrie : mêmes segments (sans tenir compte du sens ni du sommet de départ).
        assert_eq!(
            segment_multiset(&ro),
            segment_multiset(&rn),
            "graine {seed}"
        );
        // Une seule sélection par plume.
        let mut sel = ro.selections.clone();
        sel.dedup();
        assert_eq!(sel, ro.selections);
        if to < tn - 1e-6 {
            strictly_better += 1;
        }
    }
    assert!(
        strictly_better >= 20,
        "l'optimisation ne sert que {strictly_better} fois"
    );
}

#[test]
fn ca3_un_seul_trace_deja_dans_le_bon_sens_egal_et_sens_inverse_choisi() {
    // Un seul tracé dont le départ est déjà l'extrémité la plus proche : mêmes déplacements.
    let one = scene_of(vec![PathGeom::Line(pt(5.0, 5.0), pt(80.0, 40.0))]);
    let (o, n) = (
        export_hpgl_with(&one, &a4(), HpglOptions { optimize: true }).unwrap(),
        export_hpgl_with(&one, &a4(), HpglOptions { optimize: false }).unwrap(),
    );
    assert_eq!(o.text, n.text);
    assert_eq!(o.report.travel_mm, n.report.travel_mm);
    // Départ à l'autre bout : l'ordre optimisé parcourt le tracé dans le sens le plus court.
    let rev = scene_of(vec![PathGeom::Line(pt(80.0, 40.0), pt(5.0, 5.0))]);
    let o = export_hpgl_with(&rev, &a4(), HpglOptions { optimize: true }).unwrap();
    let n = export_hpgl_with(&rev, &a4(), HpglOptions { optimize: false }).unwrap();
    assert_eq!(travel_of(&read_hpgl(&o.text).unwrap()), 0.0);
    assert!(travel_of(&read_hpgl(&n.text).unwrap()) > 80.0);
    assert_eq!(
        segment_multiset(&read_hpgl(&o.text).unwrap()),
        segment_multiset(&read_hpgl(&n.text).unwrap())
    );
}

#[test]
fn ca3_contour_ferme_commence_au_sommet_le_plus_proche() {
    let c = CircleTrace::new(pt(100.0, 100.0), 50.0, 0.0, true).unwrap();
    let s = scene_of(vec![PathGeom::Circle(c)]);
    let r = read_hpgl(&export_text(&s, &a4())).unwrap();
    let start = r.strokes[0].pts[0];
    let d = (start.0 as f64 / UNITS).hypot(start.1 as f64 / UNITS);
    // Centre à (50 ; 50) du coin bas-gauche : le sommet le plus proche est à 70,71 − 50 mm.
    assert!(
        (d - (50.0 * 2f64.sqrt() - 50.0)).abs() < 0.2,
        "départ à {d} mm"
    );
    let n = export_hpgl_with(&s, &a4(), HpglOptions { optimize: false }).unwrap();
    assert!(travel_of(&r) < travel_of(&read_hpgl(&n.text).unwrap()) - 10.0);
    // Même contour : même nombre de segments, fermé.
    let p = &r.strokes[0].pts;
    assert_eq!(p[0], *p.last().unwrap());
}

#[test]
fn ca3_ordre_stable_a_distances_egales() {
    // Deux départs à 10 mm de l'origine : le premier tracé de la scène passe d'abord.
    let (a, b) = (
        PathGeom::Line(pt(10.0, 0.0), pt(10.0, 30.0)),
        PathGeom::Line(pt(0.0, 10.0), pt(30.0, 10.0)),
    );
    let first_start = |s: &pf_export::Scene| {
        let text = export_text(s, &a4());
        assert_eq!(text, export_text(s, &a4()));
        read_hpgl(&text).unwrap().strokes[0].pts[0]
    };
    assert_eq!(first_start(&scene_of(vec![a.clone(), b.clone()])), (400, 0));
    assert_eq!(first_start(&scene_of(vec![b, a])), (0, 400));
}

// ----- CA4 : format de page, rouleau, échelle ----------------------------------------------

fn sized(w: f64, h: f64) -> pf_export::Scene {
    scene_of(rect_at(500.0, 600.0, w, h))
}

#[test]
fn ca4_feuille_fixe_zone_imprimable_et_origine_bas_gauche() {
    let plot = export_hpgl(&sized(150.0, 200.0), &a4()).unwrap();
    let rep = &plot.report;
    assert_eq!(
        (rep.zone_width_mm, rep.zone_height_mm),
        (190.0, Some(277.0))
    );
    assert_eq!((rep.width_mm, rep.height_mm), (150.0, 200.0));
    let r = read_hpgl(&plot.text).unwrap();
    let pts: Vec<(i64, i64)> = r.strokes.iter().flat_map(|s| s.pts.clone()).collect();
    assert_eq!(pts.iter().map(|p| p.0).min(), Some(0));
    assert_eq!(pts.iter().map(|p| p.1).min(), Some(0));
    assert_eq!(pts.iter().map(|p| p.0).max(), Some(6000));
    assert_eq!(pts.iter().map(|p| p.1).max(), Some(8000));
    // Pile à la limite : accepté.
    assert!(export_hpgl(&sized(190.0, 277.0), &a4()).is_ok());
}

#[test]
fn ca4_trop_grand_pour_la_feuille_refus_qui_propose_le_rouleau() {
    let err = export_hpgl(&sized(200.0, 100.0), &a4()).unwrap_err();
    assert!(matches!(err, HpglError::SheetTooSmall { .. }));
    let msg = err.to_string();
    assert!(
        msg.contains("rouleau") && msg.contains("200") && msg.contains("190"),
        "{msg}"
    );
    assert!(export_hpgl(&sized(100.0, 280.0), &a4()).is_err());
    // Aucune rotation automatique : 200 × 100 mm n'entre pas en portrait, entre en paysage.
    let landscape = PageSetup {
        orientation: Orientation::Landscape,
        ..a4()
    };
    let plot = export_hpgl(&sized(200.0, 100.0), &landscape).unwrap();
    assert_eq!(plot.report.zone_height_mm, Some(190.0));
    assert_eq!(plot.report.zone_width_mm, 277.0);
    assert!(export_hpgl(&sized(100.0, 200.0), &landscape).is_err());
}

#[test]
fn ca4_rouleau_largeur_imprimable_longueur_du_contenu() {
    let roll = PageSetup {
        format: PageFormat::Roll { width_mm: 914.0 },
        ..a4()
    };
    let plot = export_hpgl(&sized(800.0, 2500.0), &roll).unwrap();
    assert_eq!(plot.report.zone_height_mm, None);
    assert_eq!(plot.report.zone_width_mm, 894.0);
    assert_eq!(plot.report.height_mm, 2500.0);
    // Plus large que la zone : refus, comme le PDF.
    let err = export_hpgl(&sized(900.0, 100.0), &roll).unwrap_err();
    assert!(matches!(
        err,
        HpglError::Page(PageSetupError::ContentTooWide { .. })
    ));
    // Longueur du rouleau bornée.
    let err = export_hpgl(&sized(100.0, 20_001.0), &roll).unwrap_err();
    assert!(matches!(
        err,
        HpglError::Page(PageSetupError::RollTooLong { .. })
    ));
}

#[test]
fn ca4_facteurs_x_y_de_calibration_multiplient_le_trace() {
    let page = PageSetup {
        format: PageFormat::A3,
        scale: PrintScale { x: 1.05, y: 0.95 },
        ..a4()
    };
    let s = scene_of(vec![
        PathGeom::Line(pt(0.0, 0.0), pt(100.0, 0.0)),
        PathGeom::Line(pt(0.0, 0.0), pt(0.0, 100.0)),
    ]);
    let plot = export_hpgl(&s, &page).unwrap();
    let r = read_hpgl(&plot.text).unwrap();
    let mut spans: Vec<(i64, i64)> = r
        .strokes
        .iter()
        .map(|s| {
            let (a, b) = (s.pts[0], s.pts[1]);
            ((a.0 - b.0).abs(), (a.1 - b.1).abs())
        })
        .collect();
    spans.sort();
    assert_eq!(spans, vec![(0, 3800), (4200, 0)]);
    assert_eq!(plot.report.scale, PrintScale { x: 1.05, y: 0.95 });
    // Le contrôle de zone utilise la taille corrigée : 190 mm × 1,1 > 190 mm.
    let big = PageSetup {
        scale: PrintScale { x: 1.1, y: 1.0 },
        ..a4()
    };
    assert!(matches!(
        export_hpgl(&sized(190.0, 100.0), &big).unwrap_err(),
        HpglError::SheetTooSmall { .. }
    ));
    assert!(export_hpgl(&sized(190.0, 100.0), &a4()).is_ok());
}

// ----- CA5 : relecture indépendante ---------------------------------------------------------

#[test]
fn ca5_relecture_egale_a_la_scene_sur_30_scenes() {
    for seed in 1..=30u64 {
        let mut rng = Rng(seed * 104_729);
        let scene = common::hpgl::random_scene(&mut rng, 1.0);
        let plot = export_hpgl(&scene, &a4()).unwrap_or_else(|e| panic!("graine {seed} : {e}"));
        let r = check_against_scene(&scene, &plot.text, 1.0, 1.0)
            .unwrap_or_else(|e| panic!("graine {seed} : {e}"));
        // Chemin à vide : somme des PU = rapport = recalcul sur les tracés relus.
        assert!(
            (r.pen_up_mm - plot.report.travel_mm).abs() < 1e-6,
            "graine {seed}"
        );
        assert!((travel_of(&r) - r.pen_up_mm).abs() < 1e-6, "graine {seed}");
        assert_eq!(r.strokes.len(), plot.report.strokes);
        let down: f64 = r.strokes.iter().map(|s| s.length_mm()).sum();
        assert!((down - plot.report.pen_down_mm).abs() < 1e-6);
    }
}

#[test]
fn ca5_chemin_a_vide_d_un_cas_connu() {
    // Deux traits : (0,0)-(10,0) puis (10,40)-(10,50) : déplacement à vide = 0 + 40 mm.
    let s = scene_of(vec![
        PathGeom::Line(pt(0.0, 0.0), pt(10.0, 0.0)),
        PathGeom::Line(pt(10.0, 40.0), pt(10.0, 50.0)),
    ]);
    let plot = export_hpgl(&s, &a4()).unwrap();
    let r = read_hpgl(&plot.text).unwrap();
    assert_eq!(r.strokes.len(), 2);
    assert!((r.pen_up_mm - 40.0).abs() < 1e-9, "{}", r.pen_up_mm);
    assert!((plot.report.pen_down_mm - 20.0).abs() < 1e-9);
}

#[test]
fn ca5_fente_de_boutonniere_trait_ferme_de_bon_perimetre() {
    use pf_core::ButtonShape;
    let mut s = scene_of(vec![PathGeom::Line(pt(0.0, 0.0), pt(100.0, 0.0))]);
    s.buttons.push(pf_export::SceneButton {
        pos: pt(50.0, 30.0),
        shape: ButtonShape::Buttonhole {
            length_mm: 20.0,
            width_mm: 4.0,
            angle_deg: 30.0,
        },
    });
    let r = read_hpgl(&export_text(&s, &a4())).unwrap();
    let slot = r.strokes.iter().find(|s| s.pen == 6).unwrap();
    assert_eq!(slot.pts[0], *slot.pts.last().unwrap(), "fente fermée");
    let perimeter = 2.0 * (20.0 - 4.0) + std::f64::consts::PI * 4.0;
    assert!(
        (slot.length_mm() - perimeter).abs() < 0.03,
        "{}",
        slot.length_mm()
    );
}

// ----- CA6 : refus sans trace ---------------------------------------------------------------

#[test]
fn ca6_scene_vide_selection_vide_reglage_invalide_sont_refuses() {
    let empty = scene_of(Vec::new());
    assert_eq!(export_hpgl(&empty, &a4()).unwrap_err(), HpglError::Nothing);
    // Des points seuls ne se tracent pas.
    let mut only_points = scene_of(Vec::new());
    only_points.points.push(pf_export::ScenePoint {
        pos: pt(1.0, 1.0),
        label: "A".into(),
        show_label: true,
    });
    assert_eq!(
        export_hpgl(&only_points, &a4()).unwrap_err(),
        HpglError::Nothing
    );
    // Un trait de longueur nulle n'est pas un tracé.
    let dot = scene_of(vec![PathGeom::Line(pt(3.0, 3.0), pt(3.0, 3.0))]);
    assert_eq!(export_hpgl(&dot, &a4()).unwrap_err(), HpglError::Nothing);
    // Sélection de contenu vide : refusée avant l'export.
    let s = three_roles();
    let none = OutputCategory::ALL
        .into_iter()
        .fold(OutputContent::default(), |c, cat| c.without(cat));
    assert!(none.apply(&s).is_err());
    // Réglages invalides.
    let bad_scale = PageSetup {
        scale: PrintScale { x: 1.5, y: 1.0 },
        ..a4()
    };
    assert!(matches!(
        export_hpgl(&s, &bad_scale).unwrap_err(),
        HpglError::Page(PageSetupError::Scale(_))
    ));
    let bad_page = PageSetup {
        format: PageFormat::Custom {
            width_mm: 100.0,
            height_mm: 300.0,
        },
        margins: Margins::uniform(10.0),
        ..a4()
    };
    assert!(matches!(
        export_hpgl(&s, &bad_page).unwrap_err(),
        HpglError::Page(PageSetupError::SideOutOfRange { .. })
    ));
}

#[test]
fn ca6_trait_minuscule_ecarte_et_compte() {
    let s = scene_of(vec![
        PathGeom::Line(pt(0.0, 0.0), pt(50.0, 0.0)),
        PathGeom::Line(pt(10.0, 10.0), pt(10.001, 10.0)),
    ]);
    let plot = export_hpgl(&s, &a4()).unwrap();
    assert_eq!(plot.report.degenerate_dropped, 1);
    assert_eq!(plot.report.strokes, 1);
}
