//! Plans de coupe, placement automatique en mode « compléter » (US-15.4, tranche 133,
//! décisions 437 à 439) : les placements existants sont conservés tels quels, seules les
//! instances manquantes sont posées. Campagne adverse : `adverse_cutting_auto_completer.rs`.
//!
//! **Toutes les valeurs sont calculées à la main** (jamais relues de la sortie du code).

use pf_core::cutting_auto::{
    AutoError, AutoItem, AutoMode, AutoOptions, ExistingProblems, UnplaceableReason,
};
use pf_core::cutting_plan::{CuttingPlanSpec, FabricFold};
use pf_core::*;
use pf_geometry::PointMm;
use std::sync::atomic::AtomicBool;

fn rect(p: &mut Project, w: f64, h: f64, ox: f64, name: &str) -> PieceId {
    let s = p.sheets[0].id;
    let vertices = [(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)]
        .iter()
        .map(|v| PointRef::New(PointMm::new(ox + v.0, 7.0 + v.1)))
        .collect();
    let e = p
        .apply(Command::AddPolygon {
            sheet: s,
            vertices,
            closed: true,
            role: PathRole::default(),
            layer: None,
        })
        .unwrap();
    p.apply(Command::ExtractPiece {
        sheet: s,
        source: PieceSource::Paths {
            entities: e.entities,
        },
        name: name.into(),
        detached: false,
    })
    .unwrap()
    .pieces[0]
}

fn plan(p: &mut Project, width: f64, gap: f64) -> CuttingPlanId {
    p.apply(Command::AddCuttingPlan {
        spec: CuttingPlanSpec {
            name: "Plan".into(),
            material: None,
            fabric_width_mm: width,
            fold: FabricFold::Flat,
            selvedge_mm: 0.0,
            min_gap_mm: gap,
            grain_direction: None,
        },
    })
    .unwrap()
    .cutting_plans[0]
}

fn put(p: &mut Project, pl: CuttingPlanId, piece: PieceId, x: f64, y: f64, mirrored: bool) {
    p.apply(Command::AddPlacement {
        plan: pl,
        piece,
        size: None,
        x_mm: x,
        y_mm: y,
        rotation_deg: 0.0,
        mirrored,
    })
    .unwrap();
}

fn item(piece: PieceId, quantity: u32, mirrored_quantity: u32) -> AutoItem {
    AutoItem {
        piece,
        size: None,
        quantity,
        mirrored_quantity,
    }
}

fn opts() -> AutoOptions {
    AutoOptions {
        allow_rotation: true,
        time_limit_ms: 10_000,
    }
}

fn complete(
    p: &Project,
    pl: CuttingPlanId,
    items: &[AutoItem],
) -> Result<pf_core::cutting_auto::AutoProposal, AutoError> {
    p.propose_cutting_layout_mode(
        pl,
        items,
        &opts(),
        AutoMode::CompleteMissing,
        &AtomicBool::new(false),
    )
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-6
}

#[test]
fn un_carre_pose_deux_a_completer_laize_200_metrage_200() {
    let mut p = Project::new("Compl");
    let q = rect(&mut p, 100.0, 100.0, 300.0, "Q");
    let pl = plan(&mut p, 200.0, 0.0);
    put(&mut p, pl, q, 0.0, 0.0, false);
    let before = p.cutting_plan(pl).unwrap().placements.clone();
    let prop = complete(&p, pl, &[item(q, 3, 0)]).unwrap();
    // Deux poses nouvelles, en (100 ; 0) et (0 ; 100) (coins libres, boîtes de 100 mm).
    assert_eq!(prop.poses.len(), 2);
    let mut at: Vec<(i64, i64)> = prop
        .poses
        .iter()
        .map(|q| (q.x_mm.round() as i64, q.y_mm.round() as i64))
        .collect();
    at.sort();
    assert_eq!(at, vec![(0, 100), (100, 0)]);
    // 3 x 10 000 mm2 sur 200 x 200 : longueur 200, rendement 75 %.
    assert!(near(prop.proposed.length_mm, 200.0), "{prop:?}");
    assert!(near(prop.proposed.yield_percent, 75.0), "{prop:?}");
    // Le projet n'a pas bougé ; appliquée, la proposition garde l'existant intact, en une étape.
    assert_eq!(p.cutting_plan(pl).unwrap().placements, before);
    let mut doc = Document::new(p.clone());
    doc.execute(prop.command.clone()).unwrap();
    let after = &doc.project().cutting_plan(pl).unwrap().placements;
    assert_eq!(after.len(), 3);
    assert_eq!(after[0], before[0]);
    assert!(doc.project().analyze_cutting_plan(pl).unwrap().is_valid());
    assert!(doc.undo());
    assert_eq!(doc.project(), &p);
    assert!(!doc.undo(), "une seule étape annulable");
}

#[test]
fn ecart_minimal_respecte_avec_l_existant() {
    // Laize 210, écart 10 : (0;0) posé ; les deux autres en (110;0) et (0;110), longueur 210.
    let mut p = Project::new("Compl");
    let q = rect(&mut p, 100.0, 100.0, 300.0, "Q");
    let pl = plan(&mut p, 210.0, 10.0);
    put(&mut p, pl, q, 0.0, 0.0, false);
    let prop = complete(&p, pl, &[item(q, 3, 0)]).unwrap();
    let mut at: Vec<(i64, i64)> = prop
        .poses
        .iter()
        .map(|q| (q.x_mm.round() as i64, q.y_mm.round() as i64))
        .collect();
    at.sort();
    assert_eq!(at, vec![(0, 110), (110, 0)]);
    assert!(near(prop.proposed.length_mm, 210.0), "{prop:?}");
}

#[test]
fn l_existant_est_conserve_octet_pour_octet_meme_tourne_ou_en_miroir() {
    let mut p = Project::new("Compl");
    let q = rect(&mut p, 100.0, 50.0, 300.0, "R");
    let pl = plan(&mut p, 400.0, 0.0);
    p.apply(Command::SetPieceInfo {
        piece: q,
        name: "R".into(),
        quantity: 1,
        material: None,
        mirrored_pair: true,
    })
    .unwrap();
    // Une pose tournée à 90° (x -> -y) : boîte 50 x 100, x dans [30 ; 80], y dans [20 ; 120].
    p.apply(Command::AddPlacement {
        plan: pl,
        piece: q,
        size: None,
        x_mm: 80.0,
        y_mm: 20.0,
        rotation_deg: 90.0,
        mirrored: false,
    })
    .unwrap();
    let before = p.cutting_plan(pl).unwrap().placements.clone();
    // Reste à poser : 1 en miroir (paire : 1 + 1).
    let prop = complete(&p, pl, &[item(q, 1, 1)]).unwrap();
    assert_eq!(prop.poses.len(), 1);
    assert!(prop.poses[0].mirrored);
    let mut doc = Document::new(p.clone());
    doc.execute(prop.command).unwrap();
    assert_eq!(
        doc.project().cutting_plan(pl).unwrap().placements[0],
        before[0]
    );
}

#[test]
fn existant_chevauchant_ou_debordant_est_refuse_nomme() {
    let mut p = Project::new("Compl");
    let q = rect(&mut p, 100.0, 100.0, 300.0, "Q");
    let pl = plan(&mut p, 200.0, 0.0);
    put(&mut p, pl, q, 0.0, 0.0, false);
    put(&mut p, pl, q, 50.0, 0.0, false);
    // Deux carrés de 100 décalés de 50 : un seul chevauchement.
    match complete(&p, pl, &[item(q, 3, 0)]) {
        Err(AutoError::ExistingInvalid(e)) => assert_eq!(
            e,
            ExistingProblems {
                overlaps: 1,
                ..Default::default()
            }
        ),
        other => panic!("{other:?}"),
    }
    // Débordement : carré en x = 150 sur une laize de 200 (150 + 100 > 200).
    let mut p = Project::new("Compl");
    let q = rect(&mut p, 100.0, 100.0, 300.0, "Q");
    let pl = plan(&mut p, 200.0, 0.0);
    put(&mut p, pl, q, 150.0, 0.0, false);
    match complete(&p, pl, &[item(q, 2, 0)]) {
        Err(AutoError::ExistingInvalid(e)) => assert_eq!(
            e,
            ExistingProblems {
                overflows: 1,
                ..Default::default()
            }
        ),
        other => panic!("{other:?}"),
    }
    // Le mode « tout replacer » répare, lui, comme avant (décision 411).
    assert!(p
        .propose_cutting_layout(pl, &[item(q, 2, 0)], &opts(), &AtomicBool::new(false))
        .is_ok());
}

#[test]
fn rien_a_completer_ne_propose_aucune_commande() {
    let mut p = Project::new("Compl");
    let q = rect(&mut p, 100.0, 100.0, 300.0, "Q");
    let pl = plan(&mut p, 200.0, 0.0);
    put(&mut p, pl, q, 0.0, 0.0, false);
    put(&mut p, pl, q, 100.0, 0.0, false);
    assert_eq!(
        complete(&p, pl, &[item(q, 2, 0)]).unwrap_err(),
        AutoError::NothingToComplete
    );
}

#[test]
fn piece_posee_au_dela_de_la_quantite_est_refusee() {
    let mut p = Project::new("Compl");
    let q = rect(&mut p, 100.0, 100.0, 300.0, "Q");
    let pl = plan(&mut p, 200.0, 0.0);
    put(&mut p, pl, q, 0.0, 0.0, false);
    put(&mut p, pl, q, 100.0, 0.0, false);
    put(&mut p, pl, q, 0.0, 100.0, false);
    match complete(&p, pl, &[item(q, 2, 0)]) {
        Err(AutoError::ExistingExceeds(v)) => {
            assert_eq!(v.len(), 1);
            assert_eq!((v[0].placed, v[0].wanted, v[0].mirrored), (3, 2, false));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn trop_large_meme_tourne_est_refuse_en_mode_completer() {
    let mut p = Project::new("Compl");
    let q = rect(&mut p, 100.0, 100.0, 300.0, "Q");
    let grand = rect(&mut p, 300.0, 250.0, 600.0, "Grand");
    let pl = plan(&mut p, 200.0, 0.0);
    put(&mut p, pl, q, 0.0, 0.0, false);
    match complete(&p, pl, &[item(q, 2, 0), item(grand, 1, 0)]) {
        Err(AutoError::Unplaceable(v)) => {
            assert_eq!(v.len(), 1);
            assert_eq!(v[0].piece, grand);
            assert!(matches!(
                v[0].reason,
                UnplaceableReason::TooWide { width_mm, span_mm } if near(width_mm, 250.0) && near(span_mm, 200.0)
            ));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn plan_vide_ou_tout_replacer_inchanges() {
    let mut p = Project::new("Compl");
    let q = rect(&mut p, 100.0, 100.0, 300.0, "Q");
    let pl = plan(&mut p, 200.0, 0.0);
    let cancel = AtomicBool::new(false);
    let items = [item(q, 4, 0)];
    // Plan vide : « compléter » = « tout replacer » (rien à conserver).
    let a = p.propose_cutting_layout(pl, &items, &opts(), &cancel);
    let b = p.propose_cutting_layout_mode(pl, &items, &opts(), AutoMode::CompleteMissing, &cancel);
    assert_eq!(format!("{a:?}"), format!("{b:?}"));
    // Avec un existant, l'alias et le mode explicite « tout replacer » sont identiques.
    put(&mut p, pl, q, 0.0, 0.0, false);
    let a = p.propose_cutting_layout(pl, &items, &opts(), &cancel);
    let b = p.propose_cutting_layout_mode(pl, &items, &opts(), AutoMode::ReplaceAll, &cancel);
    assert_eq!(format!("{a:?}"), format!("{b:?}"));
    assert_eq!(a.unwrap().poses.len(), 4, "tout est replacé, pas complété");
}

#[test]
fn calcul_annulable_en_mode_completer() {
    let mut p = Project::new("Compl");
    let q = rect(&mut p, 100.0, 100.0, 300.0, "Q");
    let pl = plan(&mut p, 200.0, 0.0);
    put(&mut p, pl, q, 0.0, 0.0, false);
    let stop = AtomicBool::new(true);
    let r = p.propose_cutting_layout_mode(
        pl,
        &[item(q, 3, 0)],
        &opts(),
        AutoMode::CompleteMissing,
        &stop,
    );
    assert_eq!(r.unwrap_err(), AutoError::Cancelled);
}
