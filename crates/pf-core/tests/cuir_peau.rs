//! US-15.5 « Cuir » : commandes de la peau, diagnostics (défaut touché, débordement, qualité),
//! métrage cuir et placement automatique sur peau. Campagne adverse : `adverse_cuir_peau.rs`.
//!
//! **Toutes les valeurs sont calculées à la main** (jamais relues de la sortie du code).
//! Peau de référence : rectangle 1000 × 600 mm de (0 ; 0) à (1000 ; 600), défaut carré 50 × 50 mm
//! de (200 ; 200) à (250 ; 250), carré de pièce « Q » 100 × 100.

use pf_core::cutting_auto::{AutoError, AutoItem, AutoMode, AutoOptions, UnplaceableReason};
use pf_core::cutting_plan::{CuttingPlanSpec, FabricFold};
use pf_core::hide_layout::{hide_yield, quality_violations};
use pf_core::*;
use pf_geometry::PointMm;
use std::sync::atomic::AtomicBool;

fn pp(x: f64, y: f64) -> PlanePoint {
    PlanePoint { x, y }
}

fn rect_pts(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<PlanePoint> {
    vec![pp(x0, y0), pp(x1, y0), pp(x1, y1), pp(x0, y1)]
}

fn near(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

struct Sc {
    p: Project,
    plan: CuttingPlanId,
    q: PieceId,
}

fn square_piece(p: &mut Project, name: &str, side: f64) -> PieceId {
    let s = p.sheets[0].id;
    let r = p
        .apply(Command::AddRectangle {
            sheet: s,
            corner: PointRef::New(PointMm::new(300.0, 7.0)),
            width_mm: side,
            height_mm: side,
        })
        .unwrap();
    p.apply(Command::ExtractPiece {
        sheet: s,
        source: PieceSource::Paths {
            entities: r.entities,
        },
        name: name.into(),
        detached: false,
    })
    .unwrap()
    .pieces[0]
}

fn sc() -> Sc {
    let mut p = Project::new("Cuir");
    let q = square_piece(&mut p, "Q", 100.0);
    let plan = p
        .apply(Command::AddCuttingPlan {
            spec: CuttingPlanSpec {
                name: "Peau".into(),
                material: None,
                fabric_width_mm: 1000.0,
                fold: FabricFold::Flat,
                selvedge_mm: 0.0,
                min_gap_mm: 0.0,
                grain_direction: None,
            },
        })
        .unwrap()
        .cutting_plans[0];
    Sc { p, plan, q }
}

fn with_hide(f: &mut Sc) -> HideDefectId {
    f.p.apply(Command::SetHideContour {
        plan: f.plan,
        contour: rect_pts(0.0, 0.0, 1000.0, 600.0),
    })
    .unwrap();
    f.p.apply(Command::AddHideDefect {
        plan: f.plan,
        kind: DefectKind::Hole,
        polygon: rect_pts(200.0, 200.0, 250.0, 250.0),
        note: Some("trou".into()),
    })
    .unwrap()
    .hide_defects[0]
}

fn put(f: &mut Sc, x: f64, y: f64) -> PlacementId {
    f.p.apply(Command::AddPlacement {
        plan: f.plan,
        piece: f.q,
        size: None,
        x_mm: x,
        y_mm: y,
        rotation_deg: 0.0,
        mirrored: false,
    })
    .unwrap()
    .placements[0]
}

#[test]
fn pose_a_cheval_sur_le_defaut_2500_mm2() {
    let mut f = sc();
    let d = with_hide(&mut f);
    // Q posé de (150 ; 180) à (250 ; 280) : recoupe x 200..250 × y 200..250 = 50 × 50 = 2500.
    let pl = put(&mut f, 150.0, 180.0);
    let an = f.p.analyze_cutting_plan(f.plan).unwrap();
    assert_eq!(an.defect_hits.len(), 1);
    let h = an.defect_hits[0];
    assert_eq!((h.placement, h.defect, h.kind), (pl, d, DefectKind::Hole));
    assert!(near(h.area_mm2, 2500.0, 1e-6), "{}", h.area_mm2);
    assert!(an.hide_overflows.is_empty());
    assert!(!an.is_valid());
}

#[test]
fn simple_contact_avec_le_defaut_n_est_pas_une_collision() {
    let mut f = sc();
    with_hide(&mut f);
    // Q de (250 ; 200) à (350 ; 300) : bord commun avec le défaut, aire d'intersection nulle.
    put(&mut f, 250.0, 200.0);
    let an = f.p.analyze_cutting_plan(f.plan).unwrap();
    assert!(an.defect_hits.is_empty());
    assert!(an.is_valid());
}

#[test]
fn piece_sortant_d_un_coin_deborde_de_7500_mm2() {
    let mut f = sc();
    with_hide(&mut f);
    // Q de (950 ; 550) à (1050 ; 650) : 50 × 50 = 2500 dedans, 10 000 − 2 500 = 7 500 dehors.
    let pl = put(&mut f, 950.0, 550.0);
    let an = f.p.analyze_cutting_plan(f.plan).unwrap();
    assert_eq!(an.hide_overflows.len(), 1);
    assert_eq!(an.hide_overflows[0].placement, pl);
    assert!(near(an.hide_overflows[0].area_mm2, 7500.0, 1e-6));
    // La laize ne contraint plus : aucun débordement « laize ».
    assert!(an.overflows.is_empty());
}

#[test]
fn qualite_pire_grade_et_aires_par_grade() {
    let mut f = sc();
    with_hide(&mut f);
    for (g, x0, x1) in [(1u8, 0.0, 500.0), (2, 500.0, 800.0), (3, 800.0, 1000.0)] {
        f.p.apply(Command::AddQualityZone {
            plan: f.plan,
            grade: g,
            polygon: rect_pts(x0, 0.0, x1, 600.0),
        })
        .unwrap();
    }
    // Q de (450 ; 100) à (550 ; 200) : 50 × 100 = 5000 en grade 1, 5000 en grade 2.
    let a = put(&mut f, 450.0, 100.0);
    // Q de (750 ; 100) à (850 ; 200) : 5000 en grade 2, 5000 en grade 3.
    let b = put(&mut f, 750.0, 300.0);
    // Q de (100 ; 400) à (200 ; 500) : tout en grade 1.
    let c = put(&mut f, 100.0, 400.0);
    let an = f.p.analyze_cutting_plan(f.plan).unwrap();
    assert!(an.is_valid());
    let q = |id| an.qualities.iter().find(|q| q.placement == id).unwrap();
    assert_eq!(q(a).worst_grade, Some(2));
    assert!(near(q(a).areas_mm2[0], 5000.0, 1e-6) && near(q(a).areas_mm2[1], 5000.0, 1e-6));
    assert_eq!(q(b).worst_grade, Some(3));
    assert!(near(q(b).areas_mm2[1], 5000.0, 1e-6) && near(q(b).areas_mm2[2], 5000.0, 1e-6));
    assert_eq!(q(c).worst_grade, Some(1));
    assert!(near(q(c).areas_mm2[0], 10_000.0, 1e-6));
    // Exigence de grade 2 au plus : seule la pose b la viole.
    assert_eq!(quality_violations(&an, 2), vec![b]);
    assert_eq!(quality_violations(&an, 1), vec![a, b]);
    assert!(quality_violations(&an, 3).is_empty());
}

#[test]
fn metrage_cuir_surface_posee_sur_surface_utile() {
    let mut f = sc();
    with_hide(&mut f);
    put(&mut f, 10.0, 10.0);
    let an = f.p.analyze_cutting_plan(f.plan).unwrap();
    let y = hide_yield(f.p.cutting_plan(f.plan).unwrap().hide.as_ref(), &an).unwrap();
    // 1000 × 600 = 600 000 ; défaut 2 500 ; utile 597 500 ; posé 10 000 ; 10 000 / 597 500.
    assert!(near(y.contour_area_mm2, 600_000.0, 1e-6));
    assert!(near(y.defects_area_mm2, 2_500.0, 1e-6));
    assert!(near(y.useful_area_mm2, 597_500.0, 1e-6));
    assert!(near(y.used_area_mm2, 10_000.0, 1e-6));
    assert!(near(y.percent, 100.0 * 10_000.0 / 597_500.0, 1e-9));
    assert!(!y.defects_overlap);
    // Plan invalide : aucun chiffre.
    put(&mut f, 150.0, 180.0);
    let an = f.p.analyze_cutting_plan(f.plan).unwrap();
    assert!(hide_yield(f.p.cutting_plan(f.plan).unwrap().hide.as_ref(), &an).is_none());
}

#[test]
fn plan_sans_peau_inchange() {
    let mut f = sc();
    let pl = put(&mut f, 10.0, 10.0);
    let an = f.p.analyze_cutting_plan(f.plan).unwrap();
    assert!(an.defect_hits.is_empty() && an.hide_overflows.is_empty() && an.qualities.is_empty());
    assert!(an.is_valid());
    // Hors laize : débordement de laize (comportement d'avant).
    f.p.apply(Command::UpdatePlacement {
        plan: f.plan,
        placement: pl,
        size: None,
        x_mm: 950.0,
        y_mm: 10.0,
        rotation_deg: 0.0,
        mirrored: false,
    })
    .unwrap();
    assert_eq!(f.p.analyze_cutting_plan(f.plan).unwrap().overflows.len(), 1);
}

#[test]
fn commandes_refus_nommes_et_annulation() {
    let mut f = sc();
    // Sans peau : défaut, zone, retrait refusés.
    for c in [
        Command::AddHideDefect {
            plan: f.plan,
            kind: DefectKind::Scar,
            polygon: rect_pts(0.0, 0.0, 10.0, 10.0),
            note: None,
        },
        Command::AddQualityZone {
            plan: f.plan,
            grade: 1,
            polygon: rect_pts(0.0, 0.0, 10.0, 10.0),
        },
        Command::RemoveHide { plan: f.plan },
    ] {
        let before = f.p.clone();
        let e = f.p.apply(c).unwrap_err().to_string();
        assert!(e.contains("Peau") || e.contains("peau"), "{e}");
        assert_eq!(f.p, before, "aucune trace");
    }
    let d = with_hide(&mut f);
    let before = f.p.clone();
    let bad = [
        // Contour auto-croisé.
        Command::SetHideContour {
            plan: f.plan,
            contour: vec![pp(0.0, 0.0), pp(10.0, 10.0), pp(10.0, 0.0), pp(0.0, 10.0)],
        },
        // Nouveau contour qui ne contient plus le défaut.
        Command::SetHideContour {
            plan: f.plan,
            contour: rect_pts(0.0, 0.0, 100.0, 100.0),
        },
        // Défaut hors du contour.
        Command::AddHideDefect {
            plan: f.plan,
            kind: DefectKind::Stain,
            polygon: rect_pts(990.0, 590.0, 1010.0, 610.0),
            note: None,
        },
        // Défaut qui en recoupe un autre.
        Command::AddHideDefect {
            plan: f.plan,
            kind: DefectKind::Stain,
            polygon: rect_pts(240.0, 240.0, 300.0, 300.0),
            note: None,
        },
        // Grade 4.
        Command::AddQualityZone {
            plan: f.plan,
            grade: 4,
            polygon: rect_pts(0.0, 0.0, 10.0, 10.0),
        },
        // Ids inconnus.
        Command::RemoveHideDefect {
            plan: f.plan,
            defect: HideDefectId(9999),
        },
        Command::RemoveQualityZone {
            plan: f.plan,
            zone: QualityZoneId(9999),
        },
    ];
    for c in bad {
        let e = f.p.apply(c.clone());
        assert!(e.is_err(), "{c:?}");
        assert_eq!(f.p, before, "aucune trace après {c:?}");
    }
    // Un pas d'annulation par commande, annulation exacte.
    let mut doc = Document::new(f.p.clone());
    let snap0 = doc.project().clone();
    doc.execute(Command::UpdateHideDefect {
        plan: f.plan,
        defect: d,
        kind: DefectKind::Thin,
        polygon: rect_pts(300.0, 300.0, 340.0, 330.0),
        note: None,
    })
    .unwrap();
    let h = doc
        .project()
        .cutting_plan(f.plan)
        .unwrap()
        .hide
        .clone()
        .unwrap();
    assert_eq!(h.defects[0].kind, DefectKind::Thin);
    assert_eq!(h.defects[0].note, None);
    assert!(doc.undo());
    assert_eq!(doc.project(), &snap0);
    assert!(!doc.can_undo() || doc.undo());
    // Retrait de la peau : défauts et zones disparaissent avec elle.
    let mut doc = Document::new(f.p.clone());
    doc.execute(Command::RemoveHide { plan: f.plan }).unwrap();
    assert!(doc.project().cutting_plan(f.plan).unwrap().hide.is_none());
    assert!(doc.undo());
    assert_eq!(doc.project(), &f.p);
}

#[test]
fn le_fichier_garde_la_peau_apres_aller_retour() {
    let mut f = sc();
    with_hide(&mut f);
    f.p.apply(Command::AddQualityZone {
        plan: f.plan,
        grade: 2,
        polygon: rect_pts(0.0, 0.0, 500.0, 300.0),
    })
    .unwrap();
    let json = pf_core::persist::to_json(&f.p).unwrap();
    let back = pf_core::persist::from_json(&json).unwrap();
    assert_eq!(back, f.p);
    assert!(json.contains("\"schema_version\": 6"));
}

fn items(q: PieceId, n: u32) -> Vec<AutoItem> {
    vec![AutoItem {
        piece: q,
        size: None,
        quantity: n,
        mirrored_quantity: 0,
    }]
}

#[test]
fn auto_sur_peau_evite_defaut_et_reste_dans_le_contour() {
    let mut f = sc();
    // Peau étroite 300 × 100 ; défaut au milieu : 100 × 100 de (100 ; 0) à (200 ; 100)
    // = tout le milieu. Deux carrés 100 × 100 tiennent de part et d'autre, pas un troisième.
    f.p.apply(Command::SetHideContour {
        plan: f.plan,
        contour: rect_pts(0.0, 0.0, 300.0, 100.0),
    })
    .unwrap();
    f.p.apply(Command::AddHideDefect {
        plan: f.plan,
        kind: DefectKind::Hole,
        polygon: rect_pts(100.0, 0.0, 200.0, 100.0),
        note: None,
    })
    .unwrap();
    let cancel = AtomicBool::new(false);
    let prop =
        f.p.propose_cutting_layout(f.plan, &items(f.q, 2), &AutoOptions::default(), &cancel)
            .unwrap();
    f.p.apply(prop.command.clone()).unwrap();
    let an = f.p.analyze_cutting_plan(f.plan).unwrap();
    assert!(an.is_valid());
    assert_eq!(an.outlines.len(), 2);
    assert!(an.defect_hits.is_empty() && an.hide_overflows.is_empty());
    // Trois : refus nommé listant la pièce.
    let e =
        f.p.propose_cutting_layout(f.plan, &items(f.q, 3), &AutoOptions::default(), &cancel)
            .unwrap_err();
    match e {
        AutoError::Unplaceable(v) => {
            assert_eq!(v.len(), 1);
            assert_eq!(v[0].piece, f.q);
            assert_eq!(v[0].reason, UnplaceableReason::NoRoomOnHide);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn auto_sur_peau_respecte_la_qualite_exigee() {
    let mut f = sc();
    // Peau 300 × 100 ; zone grade 3 à droite (200..300) : avec exigence 2, un seul carré
    // (0..100 ou 100..200) par rangée utile ; deux carrés tiennent dans 0..200.
    f.p.apply(Command::SetHideContour {
        plan: f.plan,
        contour: rect_pts(0.0, 0.0, 300.0, 100.0),
    })
    .unwrap();
    f.p.apply(Command::AddQualityZone {
        plan: f.plan,
        grade: 3,
        polygon: rect_pts(200.0, 0.0, 300.0, 100.0),
    })
    .unwrap();
    let cancel = AtomicBool::new(false);
    let opts = AutoOptions::default();
    let ok =
        f.p.propose_hide_layout(
            f.plan,
            &items(f.q, 2),
            &opts,
            Some(2),
            AutoMode::ReplaceAll,
            &cancel,
        )
        .unwrap();
    f.p.apply(ok.command).unwrap();
    let an = f.p.analyze_cutting_plan(f.plan).unwrap();
    assert!(quality_violations(&an, 2).is_empty());
    let e =
        f.p.propose_hide_layout(
            f.plan,
            &items(f.q, 3),
            &opts,
            Some(2),
            AutoMode::ReplaceAll,
            &cancel,
        )
        .unwrap_err();
    assert!(matches!(e, AutoError::Unplaceable(_)), "{e:?}");
}
