//! Plans de coupe, métrage (US-15.3) : longueur consommée, surface utilisée, rendement, par taille.
//! Campagne adverse : `adverse_cutting_yield.rs`. **Valeurs calculées à la main.**

use pf_core::cutting_plan::{CuttingPlanSpec, FabricFold};
use pf_core::cutting_yield::YieldStatus;
use pf_core::*;
use pf_geometry::PointMm;

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

fn rect(p: &mut Project, s: SheetId, x: f64, w: f64, h: f64, name: &str) -> PieceId {
    let r = p
        .apply(Command::AddRectangle {
            sheet: s,
            corner: PointRef::New(pt(x, 7.0)),
            width_mm: w,
            height_mm: h,
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

struct Sc {
    p: Project,
    s: SheetId,
    sq: PieceId,
}

fn sc() -> Sc {
    let mut p = Project::new("Metrage");
    let s = p.sheets[0].id;
    let sq = rect(&mut p, s, 300.0, 100.0, 100.0, "Carre");
    p.apply(Command::SetPieceInfo {
        piece: sq,
        name: "Carre".into(),
        quantity: 9,
        material: None,
        mirrored_pair: false,
    })
    .unwrap();
    Sc { p, s, sq }
}

fn plan(p: &mut Project, width: f64, sel: f64, fold: FabricFold) -> CuttingPlanId {
    p.apply(Command::AddCuttingPlan {
        spec: CuttingPlanSpec {
            name: "Plan".into(),
            material: None,
            fabric_width_mm: width,
            fold,
            selvedge_mm: sel,
            min_gap_mm: 0.0,
            grain_direction: None,
        },
    })
    .unwrap()
    .cutting_plans[0]
}

fn put(p: &mut Project, plan: CuttingPlanId, piece: PieceId, size: Option<SizeId>, x: f64, y: f64) {
    p.apply(Command::AddPlacement {
        plan,
        piece,
        size,
        x_mm: x,
        y_mm: y,
        rotation_deg: 0.0,
        mirrored: false,
    })
    .unwrap();
}

fn near(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

#[test]
fn deux_carres_cote_a_cote_rendement_66_67_pour_cent() {
    // Laize 320, lisières 10 : utile 300. Deux carrés 100 x 100 en (10 ; 0) et (110 ; 0) :
    // longueur 100 ; 20 000 / (300 x 100) = 66,666... %.
    let mut f = sc();
    let pl = plan(&mut f.p, 320.0, 10.0, FabricFold::Flat);
    put(&mut f.p, pl, f.sq, None, 10.0, 0.0);
    put(&mut f.p, pl, f.sq, None, 110.0, 0.0);
    let y = f.p.cutting_plan_yield(pl).unwrap();
    assert_eq!(y.status, YieldStatus::Valid);
    let g = y.figures.unwrap();
    assert!(near(g.useful_width_mm, 300.0, 1e-9));
    assert!(near(g.length_mm, 100.0, 1e-9));
    assert!(near(g.used_area_mm2, 20_000.0, 1e-6));
    assert!(
        near(g.yield_percent, 66.666_667, 1e-5),
        "{}",
        g.yield_percent
    );
    assert_eq!(g.by_size.len(), 1);
    assert_eq!((g.by_size[0].size, g.by_size[0].pieces), (None, 2));
    assert!(near(g.by_size[0].share_percent, 66.666_667, 1e-5));
}

#[test]
fn la_longueur_va_de_zero_au_bord_le_plus_eloigne() {
    // Un seul carré posé en y = 250 : longueur 350 (pas 100) ; 10 000 / (300 x 350) = 9,5238 %.
    let mut f = sc();
    let pl = plan(&mut f.p, 320.0, 10.0, FabricFold::Flat);
    put(&mut f.p, pl, f.sq, None, 10.0, 250.0);
    let g = f.p.cutting_plan_yield(pl).unwrap().figures.unwrap();
    assert!(near(g.length_mm, 350.0, 1e-9));
    assert!(
        near(g.yield_percent, 9.523_810, 1e-5),
        "{}",
        g.yield_percent
    );
}

#[test]
fn par_taille_une_ligne_par_taille_posee_et_la_somme_des_parts_est_le_rendement() {
    let mut f = sc();
    f.p.apply(Command::AddSizeRange {
        name: "Femme".into(),
        kind: SizeKind::Alphanumeric,
        labels: vec!["S".into(), "M".into(), "L".into()],
        base: 1,
    })
    .unwrap();
    let sizes: Vec<SizeId> = f.p.sizes[0].sizes.iter().map(|z| z.id).collect();
    let pl = plan(&mut f.p, 320.0, 10.0, FabricFold::Flat);
    // Carrés (sans gradation : 10 000 mm² chacun) : L, base, S dans cet ordre de pose.
    put(&mut f.p, pl, f.sq, Some(sizes[2]), 10.0, 0.0);
    put(&mut f.p, pl, f.sq, None, 110.0, 0.0);
    put(&mut f.p, pl, f.sq, Some(sizes[0]), 210.0, 0.0);
    put(&mut f.p, pl, f.sq, Some(sizes[0]), 10.0, 100.0);
    let g = f.p.cutting_plan_yield(pl).unwrap().figures.unwrap();
    // Longueur 200 : référence 300 x 200 = 60 000 ; 4 carrés = 40 000 = 66,67 %.
    assert!(near(g.length_mm, 200.0, 1e-9));
    assert!(near(g.yield_percent, 66.666_667, 1e-5));
    // Lignes : base (1 carré), S (2), L (1) ; parts 10 000 / 60 000 = 16,6667 % et 33,3333 %.
    let rows: Vec<_> = g.by_size.iter().map(|r| (r.size, r.pieces)).collect();
    assert_eq!(
        rows,
        vec![(None, 1), (Some(sizes[0]), 2), (Some(sizes[2]), 1)]
    );
    assert!(near(g.by_size[0].share_percent, 16.666_667, 1e-5));
    assert!(near(g.by_size[1].share_percent, 33.333_333, 1e-5));
    let sum: f64 = g.by_size.iter().map(|r| r.share_percent).sum();
    assert!(near(sum, g.yield_percent, 1e-9));
}

#[test]
fn un_plan_invalide_ne_porte_aucun_chiffre() {
    let mut f = sc();
    let pl = plan(&mut f.p, 320.0, 10.0, FabricFold::Flat);
    put(&mut f.p, pl, f.sq, None, 10.0, 0.0);
    put(&mut f.p, pl, f.sq, None, 60.0, 0.0); // chevauchement de 50 x 100
    let y = f.p.cutting_plan_yield(pl).unwrap();
    assert_eq!(y.status, YieldStatus::Invalid);
    assert!(y.figures.is_none());
    // Débordement : carré posé à y < 0.
    let mut g = sc();
    let pl = plan(&mut g.p, 320.0, 10.0, FabricFold::Flat);
    put(&mut g.p, pl, g.sq, None, 10.0, -5.0);
    let y = g.p.cutting_plan_yield(pl).unwrap();
    assert_eq!((y.status, y.figures), (YieldStatus::Invalid, None));
    // Corrigé : le chiffre revient.
    let mut h = sc();
    let pl = plan(&mut h.p, 320.0, 10.0, FabricFold::Flat);
    put(&mut h.p, pl, h.sq, None, 10.0, 0.0);
    assert_eq!(
        h.p.cutting_plan_yield(pl).unwrap().status,
        YieldStatus::Valid
    );
}

#[test]
fn un_plan_vide_est_non_renseigne() {
    let mut f = sc();
    let pl = plan(&mut f.p, 320.0, 10.0, FabricFold::Flat);
    let y = f.p.cutting_plan_yield(pl).unwrap();
    assert_eq!((y.status, y.figures), (YieldStatus::Empty, None));
}

#[test]
fn pliee_la_laize_utile_reste_laize_moins_deux_lisieres() {
    // Laize 320, lisières 10, pliée : carré dans la bande de gauche [10 ; 160] ; utile 300 ;
    // 10 000 / (300 x 100) = 33,33 %.
    let mut f = sc();
    let pl = plan(&mut f.p, 320.0, 10.0, FabricFold::Folded);
    put(&mut f.p, pl, f.sq, None, 10.0, 0.0);
    let g = f.p.cutting_plan_yield(pl).unwrap().figures.unwrap();
    assert!(near(g.useful_width_mm, 300.0, 1e-9));
    assert!(near(g.yield_percent, 33.333_333, 1e-5));
}

#[test]
fn surface_utilisee_est_l_aire_de_coupe_exacte_avec_marge() {
    // Marge 10 mm : coupe 120 x 120 = 14 400 mm² ; laize 320, lisières 10 : utile 300 ;
    // longueur 120 ; 14 400 / (300 x 120) = 40 %.
    let mut f = sc();
    f.p.apply(Command::SetSeamAllowance {
        piece: f.sq,
        seam: Some(pf_core::piece_seam::SeamAllowance {
            default_mm: 10.0,
            ..Default::default()
        }),
    })
    .unwrap();
    let pl = plan(&mut f.p, 320.0, 10.0, FabricFold::Flat);
    put(&mut f.p, pl, f.sq, None, 10.0, 0.0);
    let g = f.p.cutting_plan_yield(pl).unwrap().figures.unwrap();
    assert!(near(g.used_area_mm2, 14_400.0, 1e-3), "{}", g.used_area_mm2);
    assert!(near(g.length_mm, 120.0, 1e-6));
    assert!(near(g.yield_percent, 40.0, 1e-4), "{}", g.yield_percent);
    let _ = f.s;
}
