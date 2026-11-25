//! Plans de coupe, proposition de placement automatique (US-15.4) : rendement, rotation et
//! droit-fil, miroir, refus, niveau 2 (glisser au contact), déterminisme, annulation en un pas.
//! Campagne adverse : `adverse_cutting_auto.rs`.
//!
//! **Toutes les valeurs sont calculées à la main** (jamais relues de la sortie du code).

use pf_core::cutting_auto::{AutoError, AutoItem, AutoOptions, UnplaceableReason};
use pf_core::cutting_plan::{CuttingPlanSpec, FabricFold};
use pf_core::*;
use pf_geometry::PointMm;
use std::sync::atomic::AtomicBool;

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

fn poly(p: &mut Project, verts: &[(f64, f64)], ox: f64, name: &str) -> PieceId {
    let s = p.sheets[0].id;
    let vertices = verts
        .iter()
        .map(|v| PointRef::New(pt(ox + v.0, 7.0 + v.1)))
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

fn rect(p: &mut Project, w: f64, h: f64, ox: f64, name: &str) -> PieceId {
    poly(p, &[(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)], ox, name)
}

fn vertical_grain(p: &mut Project, piece: PieceId, x: f64, y0: f64, y1: f64) {
    let s = p.sheets[0].id;
    let g = p
        .apply(Command::AddSegment {
            sheet: s,
            from: PointRef::New(pt(x, 7.0 + y0)),
            to: PointRef::New(pt(x, 7.0 + y1)),
        })
        .unwrap();
    p.apply(Command::SetPathRole {
        sheet: s,
        entity: g.entities[0],
        role: PathRole::Grainline,
    })
    .unwrap();
    let outline = p.piece(piece).unwrap().outline.clone();
    p.apply(Command::SetPieceOutline {
        piece,
        outline,
        grainline: Some(g.entities[0]),
    })
    .unwrap();
}

fn plan(
    p: &mut Project,
    width: f64,
    sel: f64,
    gap: f64,
    fold: FabricFold,
    grain: Option<f64>,
) -> CuttingPlanId {
    let name = format!("Plan {}", p.cutting_plans.len());
    p.apply(Command::AddCuttingPlan {
        spec: CuttingPlanSpec {
            name,
            material: None,
            fabric_width_mm: width,
            fold,
            selvedge_mm: sel,
            min_gap_mm: gap,
            grain_direction: grain,
        },
    })
    .unwrap()
    .cutting_plans[0]
}

fn item(piece: PieceId, quantity: u32) -> AutoItem {
    AutoItem {
        piece,
        size: None,
        quantity,
        mirrored_quantity: 0,
    }
}

fn opts(rot: bool) -> AutoOptions {
    AutoOptions {
        allow_rotation: rot,
        time_limit_ms: 10_000,
    }
}

fn no_cancel() -> AtomicBool {
    AtomicBool::new(false)
}

fn near(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

#[test]
fn quatre_carres_sur_laize_200_longueur_200_rendement_100() {
    let mut p = Project::new("Auto");
    let q = rect(&mut p, 100.0, 100.0, 300.0, "Q");
    let pl = plan(&mut p, 200.0, 0.0, 0.0, FabricFold::Flat, None);
    let prop = p
        .propose_cutting_layout(pl, &[item(q, 4)], &opts(true), &no_cancel())
        .unwrap();
    // 4 x 10 000 mm2 / (200 x 200) = 100 %.
    assert_eq!(prop.poses.len(), 4);
    assert!(near(prop.proposed.length_mm, 200.0, 1e-6), "{prop:?}");
    assert!(near(prop.proposed.yield_percent, 100.0, 1e-6));
    assert!(prop.current.is_none());
    assert!(prop.proposed.yield_percent >= prop.level1.yield_percent - 1e-9);
    // Le projet n'a pas bougé (lecture pure).
    assert!(p.cutting_plan(pl).unwrap().placements.is_empty());
}

#[test]
fn proposition_appliquee_en_une_etape_annulable_et_editable() {
    let mut p = Project::new("Auto");
    let q = rect(&mut p, 100.0, 100.0, 300.0, "Q");
    let pl = plan(&mut p, 200.0, 0.0, 0.0, FabricFold::Flat, None);
    let prop = p
        .propose_cutting_layout(pl, &[item(q, 4)], &opts(false), &no_cancel())
        .unwrap();
    let mut doc = Document::new(p);
    let before = doc.project().clone();
    doc.execute(prop.command.clone()).unwrap();
    let placed = doc.project().cutting_plan(pl).unwrap().placements.clone();
    assert_eq!(placed.len(), 4);
    // Placements ordinaires, éditables.
    doc.execute(Command::UpdatePlacement {
        plan: pl,
        placement: placed[0].id,
        size: None,
        x_mm: 1000.0,
        y_mm: 0.0,
        rotation_deg: 0.0,
        mirrored: false,
    })
    .unwrap();
    assert!(doc.undo());
    assert!(doc.undo());
    assert_eq!(doc.project(), &before);
}

#[test]
fn proposition_remplace_les_placements_existants() {
    let mut p = Project::new("Auto");
    let q = rect(&mut p, 100.0, 100.0, 300.0, "Q");
    let pl = plan(&mut p, 200.0, 0.0, 0.0, FabricFold::Flat, None);
    p.apply(Command::AddPlacement {
        plan: pl,
        piece: q,
        size: None,
        x_mm: 0.0,
        y_mm: 500.0,
        rotation_deg: 0.0,
        mirrored: false,
    })
    .unwrap();
    let prop = p
        .propose_cutting_layout(pl, &[item(q, 2)], &opts(false), &no_cancel())
        .unwrap();
    // Actuel : un carré à y = 500, longueur 600, rendement 10 000 / (200 x 600) = 8,33 %.
    let cur = prop.current.unwrap();
    assert!(near(cur.length_mm, 600.0, 1e-6) && near(cur.yield_percent, 100.0 / 12.0, 1e-6));
    p.apply(prop.command).unwrap();
    assert_eq!(p.cutting_plan(pl).unwrap().placements.len(), 2);
}

#[test]
fn rotation_gagne_du_metrage_sur_des_rectangles_120_x_60() {
    let mut p = Project::new("Auto");
    let r = rect(&mut p, 120.0, 60.0, 300.0, "R");
    let pl = plan(&mut p, 200.0, 0.0, 0.0, FabricFold::Flat, None);
    // Sans rotation : empilés, longueur 3 x 60 = 180, rendement 21 600 / 36 000 = 60 %.
    let a = p
        .propose_cutting_layout(pl, &[item(r, 3)], &opts(false), &no_cancel())
        .unwrap();
    assert!(near(a.proposed.length_mm, 180.0, 1e-6), "{a:?}");
    assert!(near(a.proposed.yield_percent, 60.0, 1e-6));
    // Avec rotation : deux à plat (120 x 60 empilés) et un debout (60 x 120) à côté :
    // longueur 120, rendement 21 600 / 24 000 = 90 %.
    let b = p
        .propose_cutting_layout(pl, &[item(r, 3)], &opts(true), &no_cancel())
        .unwrap();
    assert!(near(b.proposed.length_mm, 120.0, 1e-6), "{b:?}");
    assert!(near(b.proposed.yield_percent, 90.0, 1e-6));
    assert!(b.poses.iter().any(|q| q.rotation_deg != 0.0));
}

#[test]
fn droit_fil_interdit_la_rotation_sans_droit_fil_elle_est_permise() {
    // Rectangle 300 x 100 sur une laize utile de 100 : il ne tient que tourné de 90 / 270.
    let mut p = Project::new("Auto");
    let r = rect(&mut p, 300.0, 100.0, 300.0, "R");
    let libre = plan(&mut p, 100.0, 0.0, 0.0, FabricFold::Flat, None);
    let ok = p
        .propose_cutting_layout(libre, &[item(r, 1)], &opts(true), &no_cancel())
        .unwrap();
    assert!(near(ok.proposed.length_mm, 300.0, 1e-6));
    assert!(near(ok.proposed.yield_percent, 100.0, 1e-6));
    assert!(ok.poses[0].rotation_deg == 90.0 || ok.poses[0].rotation_deg == 270.0);
    // Droit-fil vertical local, sens du plan 90° : seules 0° / 180° sont permises -> refus.
    vertical_grain(&mut p, r, 150.0, 10.0, 90.0);
    let fil = plan(&mut p, 100.0, 0.0, 0.0, FabricFold::Flat, Some(90.0));
    match p.propose_cutting_layout(fil, &[item(r, 1)], &opts(true), &no_cancel()) {
        Err(AutoError::Unplaceable(v)) => {
            assert_eq!(v.len(), 1);
            assert_eq!(v[0].piece, r);
            assert!(matches!(
                v[0].reason,
                UnplaceableReason::TooWide { width_mm, span_mm }
                    if near(width_mm, 300.0, 1e-6) && near(span_mm, 100.0, 1e-9)
            ));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn refus_clair_liste_toutes_les_pieces_non_posables_sans_trace() {
    let mut p = Project::new("Auto");
    let a = rect(&mut p, 500.0, 10.0, 300.0, "A");
    let b = rect(&mut p, 400.0, 10.0, 900.0, "B");
    let c = rect(&mut p, 50.0, 10.0, 1500.0, "C");
    let pl = plan(&mut p, 300.0, 0.0, 0.0, FabricFold::Flat, None);
    let before = p.clone();
    // A et B ne tiennent ni à plat ni debout (10 de large mais 500 > ... debout : 10 x 500 tient :
    // on interdit donc la rotation).
    let err = p
        .propose_cutting_layout(
            pl,
            &[item(a, 1), item(b, 1), item(c, 1)],
            &opts(false),
            &no_cancel(),
        )
        .unwrap_err();
    match err {
        AutoError::Unplaceable(v) => {
            let ids: Vec<PieceId> = v.iter().map(|u| u.piece).collect();
            assert_eq!(ids, vec![a, b]);
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(p, before);
    assert_eq!(
        p.propose_cutting_layout(pl, &[item(a, 0)], &opts(true), &no_cancel()),
        Err(AutoError::NothingToPlace)
    );
}

#[test]
fn miroir_seulement_pour_une_paire_et_quantite_exacte() {
    let mut p = Project::new("Auto");
    let r = rect(&mut p, 100.0, 50.0, 300.0, "R");
    let pl = plan(&mut p, 400.0, 0.0, 0.0, FabricFold::Flat, None);
    let it = AutoItem {
        piece: r,
        size: None,
        quantity: 1,
        mirrored_quantity: 1,
    };
    assert_eq!(
        p.propose_cutting_layout(pl, std::slice::from_ref(&it), &opts(true), &no_cancel()),
        Err(AutoError::MirrorNotAllowed(r))
    );
    p.apply(Command::SetPieceInfo {
        piece: r,
        name: "R".into(),
        quantity: 1,
        material: None,
        mirrored_pair: true,
    })
    .unwrap();
    let prop = p
        .propose_cutting_layout(pl, &[it], &opts(true), &no_cancel())
        .unwrap();
    assert_eq!(prop.poses.len(), 2);
    assert_eq!(prop.poses.iter().filter(|q| q.mirrored).count(), 1);
    // 2 x 5 000 / (400 x 50) = 50 % (côte à côte : 200 de large, longueur 50).
    assert!(near(prop.proposed.length_mm, 50.0, 1e-6), "{prop:?}");
    p.apply(prop.command).unwrap();
    let an = p.analyze_cutting_plan(pl).unwrap();
    assert!(an.is_valid() && an.over_placed.is_empty());
}

#[test]
fn ecart_mini_et_lisieres_respectes_calcul_a_la_main() {
    // Laize 230, lisières 15 : utile 200 ; écart 20 : deux carrés 90 + 20 + 90 = 200 côte à côte.
    let mut p = Project::new("Auto");
    let q = rect(&mut p, 90.0, 90.0, 300.0, "Q");
    let pl = plan(&mut p, 230.0, 15.0, 20.0, FabricFold::Flat, None);
    let prop = p
        .propose_cutting_layout(pl, &[item(q, 4)], &opts(false), &no_cancel())
        .unwrap();
    // Deux rangées : longueur 90 + 20 + 90 = 200 ; aire 4 x 8 100 = 32 400 ; / (200 x 200) = 81 %.
    assert!(near(prop.proposed.length_mm, 200.0, 1e-6), "{prop:?}");
    assert!(near(prop.proposed.yield_percent, 81.0, 1e-6));
    for q in &prop.poses {
        assert!(q.x_mm >= 15.0 - 1e-6 && q.x_mm + 90.0 <= 215.0 + 1e-6);
    }
}

#[test]
fn pliee_pose_d_un_seul_cote_du_pli() {
    // Laize 400, lisières 20, pliée : pli à 200 ; zone [20 ; 200] = 180. Carrés 80 + 20 écart + 80.
    let mut p = Project::new("Auto");
    let q = rect(&mut p, 80.0, 80.0, 300.0, "Q");
    let pl = plan(&mut p, 400.0, 20.0, 20.0, FabricFold::Folded, None);
    let prop = p
        .propose_cutting_layout(pl, &[item(q, 3)], &opts(false), &no_cancel())
        .unwrap();
    // 2 par rangée (80 + 20 + 80 = 180), la 3e au-dessus : longueur 80 + 20 + 80 = 180.
    assert!(near(prop.proposed.length_mm, 180.0, 1e-6), "{prop:?}");
    for q in &prop.poses {
        assert!(
            q.x_mm >= 20.0 - 1e-6 && q.x_mm + 80.0 <= 200.0 + 1e-6,
            "{q:?}"
        );
    }
    p.apply(prop.command).unwrap();
    assert!(p.analyze_cutting_plan(pl).unwrap().is_valid());
}

#[test]
fn niveau_2_glisse_le_rectangle_sur_l_hypotenuse() {
    // S : triangle (0,0) (100,0) (100,100), hypoténuse y = x. R : carré 50 x 50. Laize utile 100 :
    // niveau 1 = S en (0 ; 0), R au-dessus en (0 ; 100) : longueur 150, rendement
    // (5 000 + 2 500) / (100 x 150) = 50 %. Niveau 2 : R descend jusqu'à ce que son coin bas droit
    // (50 ; y) touche la droite y = x, soit y = 50 : longueur 100, rendement 7 500 / 10 000 = 75 %.
    let mut p = Project::new("Auto");
    let s = poly(
        &mut p,
        &[(0.0, 0.0), (100.0, 0.0), (100.0, 100.0)],
        300.0,
        "S",
    );
    let r = rect(&mut p, 50.0, 50.0, 600.0, "R");
    let pl = plan(&mut p, 100.0, 0.0, 0.0, FabricFold::Flat, None);
    let prop = p
        .propose_cutting_layout(pl, &[item(s, 1), item(r, 1)], &opts(false), &no_cancel())
        .unwrap();
    assert!(near(prop.level1.length_mm, 150.0, 1e-6), "{prop:?}");
    assert!(near(prop.level1.yield_percent, 50.0, 1e-6));
    assert!(prop.refined);
    assert!(prop.proposed.length_mm >= 100.0 && prop.proposed.length_mm <= 100.05);
    assert!(prop.proposed.yield_percent >= 74.97 && prop.proposed.yield_percent <= 75.0 + 1e-9);
    let pr = prop.poses.iter().find(|q| q.piece == r).unwrap();
    assert!(pr.y_mm >= 50.0 - 1e-9 && pr.y_mm <= 50.03, "{pr:?}");
    assert!(near(pr.x_mm, 0.0, 1e-9));
}

#[test]
fn deterministe_au_caractere_pres_et_annulable_et_borne_de_temps() {
    let mut p = Project::new("Auto");
    let a = poly(&mut p, &[(0.0, 0.0), (80.0, 0.0), (0.0, 40.0)], 300.0, "T");
    let b = rect(&mut p, 60.0, 60.0, 600.0, "R");
    let pl = plan(&mut p, 250.0, 5.0, 3.0, FabricFold::Flat, None);
    let items = [item(a, 5), item(b, 4)];
    let one = p
        .propose_cutting_layout(pl, &items, &opts(true), &no_cancel())
        .unwrap();
    let two = p
        .propose_cutting_layout(pl, &items, &opts(true), &no_cancel())
        .unwrap();
    assert_eq!(format!("{one:?}"), format!("{two:?}"));
    // Annulation.
    let stop = AtomicBool::new(true);
    assert_eq!(
        p.propose_cutting_layout(pl, &items, &opts(true), &stop),
        Err(AutoError::Cancelled)
    );
    // Borne de temps nulle : le premier essai est toujours rendu, valide, signalé.
    let tight = p
        .propose_cutting_layout(
            pl,
            &items,
            &AutoOptions {
                allow_rotation: true,
                time_limit_ms: 0,
            },
            &no_cancel(),
        )
        .unwrap();
    assert!(tight.time_limited && tight.trials == 1);
    let mut q = p.clone();
    q.apply(tight.command).unwrap();
    assert!(q.analyze_cutting_plan(pl).unwrap().is_valid());
}

#[test]
fn niveau_2_respecte_l_ecart_mini_sur_l_hypotenuse() {
    // Même scène, écart 10 : le coin (50 ; y) doit rester à 10 mm de la droite y = x, soit
    // (y - 50) / racine(2) >= 10, donc y >= 50 + 14,142 = 64,142 (niveau 1 : R en y = 110).
    let mut p = Project::new("Auto");
    let s = poly(
        &mut p,
        &[(0.0, 0.0), (100.0, 0.0), (100.0, 100.0)],
        300.0,
        "S",
    );
    let r = rect(&mut p, 50.0, 50.0, 600.0, "R");
    let pl = plan(&mut p, 100.0, 0.0, 10.0, FabricFold::Flat, None);
    let prop = p
        .propose_cutting_layout(pl, &[item(s, 1), item(r, 1)], &opts(false), &no_cancel())
        .unwrap();
    let pr = prop.poses.iter().find(|q| q.piece == r).unwrap();
    assert!(prop.refined);
    assert!(pr.y_mm >= 64.142 - 1e-3 && pr.y_mm <= 64.2, "{pr:?}");
    assert!(near(prop.level1.length_mm, 160.0, 1e-6));
}
