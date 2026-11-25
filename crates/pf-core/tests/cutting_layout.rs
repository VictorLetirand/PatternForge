//! Plans de coupe, lecture (US-15.1 / US-15.2) : contours posés, rotation et droit-fil, miroir,
//! chevauchement, écart, débordement (plat et plié), pose en rangées, annulation en un pas.
//! Campagne adverse : `adverse_cutting_layout.rs`.
//!
//! **Toutes les valeurs sont calculées à la main** (jamais relues de la sortie du code).

use pf_core::cutting_layout::{
    intersection_area_mm2, place_point, polygon_distance_mm, OverflowReason, RotationCheck,
    WarningKind,
};
use pf_core::cutting_plan::{CuttingPlanSpec, FabricFold};
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

/// Projet d'un carré 100 × 100 « Q » (coin à (300 ; 7), hors de l'origine pour éprouver le repère
/// local) et d'un rectangle 100 × 50 « R ».
struct Sc {
    p: Project,
    s: SheetId,
    q: PieceId,
    r: PieceId,
}

fn sc() -> Sc {
    let mut p = Project::new("Pose");
    let s = p.sheets[0].id;
    let q = rect(&mut p, s, 300.0, 100.0, 100.0, "Q");
    let r = rect(&mut p, s, 600.0, 100.0, 50.0, "R");
    Sc { p, s, q, r }
}

fn spec(width: f64, sel: f64, gap: f64, fold: FabricFold, grain: Option<f64>) -> CuttingPlanSpec {
    CuttingPlanSpec {
        name: "Plan".into(),
        material: None,
        fabric_width_mm: width,
        fold,
        selvedge_mm: sel,
        min_gap_mm: gap,
        grain_direction: grain,
    }
}

fn plan(p: &mut Project, s: CuttingPlanSpec) -> CuttingPlanId {
    p.apply(Command::AddCuttingPlan { spec: s })
        .unwrap()
        .cutting_plans[0]
}

fn put(
    p: &mut Project,
    plan: CuttingPlanId,
    piece: PieceId,
    x: f64,
    y: f64,
    rot: f64,
    m: bool,
) -> PlacementId {
    p.apply(Command::AddPlacement {
        plan,
        piece,
        size: None,
        x_mm: x,
        y_mm: y,
        rotation_deg: rot,
        mirrored: m,
    })
    .unwrap()
    .placements[0]
}

fn near(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

/// Donne un droit-fil vertical (85 ; 10..90 en local de construction) à la pièce.
fn add_grain(f: &mut Sc, which_q: bool, x: f64, y0: f64, y1: f64) {
    let piece = if which_q { f.q } else { f.r };
    let g =
        f.p.apply(Command::AddSegment {
            sheet: f.s,
            from: PointRef::New(pt(x, y0)),
            to: PointRef::New(pt(x, y1)),
        })
        .unwrap();
    f.p.apply(Command::SetPathRole {
        sheet: f.s,
        entity: g.entities[0],
        role: PathRole::Grainline,
    })
    .unwrap();
    let outline = f.p.piece(piece).unwrap().outline.clone();
    f.p.apply(Command::SetPieceOutline {
        piece,
        outline,
        grainline: Some(g.entities[0]),
    })
    .unwrap();
}

// ---------------------------------------------------------------------------
// Convention de pose
// ---------------------------------------------------------------------------

#[test]
fn convention_carre_a_10_20_rotation_90() {
    // Local (100 ; 100) -> rotation 90 : (-100 ; 100) -> + (10 ; 20) = (-90 ; 120).
    let c = place_point(pt(100.0, 100.0), 10.0, 20.0, 90.0, false);
    assert!(near(c.x, -90.0, 1e-9) && near(c.y, 120.0, 1e-9), "{c:?}");
    // Local (100 ; 0) miroir : (-100 ; 0) ; rotation 90 : (0 ; -100) ; + (10 ; 20) = (10 ; -80).
    let m = place_point(pt(100.0, 0.0), 10.0, 20.0, 90.0, true);
    assert!(near(m.x, 10.0, 1e-9) && near(m.y, -80.0, 1e-9), "{m:?}");
    // Miroir seul : (100 ; 0) -> (-100 ; 0) -> (-90 ; 20).
    let m = place_point(pt(100.0, 0.0), 10.0, 20.0, 0.0, true);
    assert!(near(m.x, -90.0, 1e-9) && near(m.y, 20.0, 1e-9));
}

#[test]
fn contour_pose_du_carre_repere_local_a_la_boite() {
    let mut f = sc();
    let pl = plan(&mut f.p, spec(1000.0, 0.0, 0.0, FabricFold::Flat, None));
    // La pièce est construite en (300 ; 7) : le repère local est sa boîte, pas la construction.
    put(&mut f.p, pl, f.q, 10.0, 20.0, 90.0, false);
    let a = f.p.analyze_cutting_plan(pl).unwrap();
    let o = &a.outlines[0];
    assert!(near(o.bbox.min.x, -90.0, 1e-9) && near(o.bbox.max.x, 10.0, 1e-9));
    assert!(near(o.bbox.min.y, 20.0, 1e-9) && near(o.bbox.max.y, 120.0, 1e-9));
    assert!(near(o.area_mm2, 10_000.0, 1e-6));
    // x < lisière (0) : déborde à gauche ; vu comme hors zone utile.
    assert_eq!(a.overflows[0].reasons, vec![OverflowReason::OutsideUseful]);
}

#[test]
fn contour_de_coupe_avec_marge_de_couture() {
    // Marge 10 mm sur les 4 bords : coupe 120 × 120 (aire 14 400), à l'origine locale.
    let mut f = sc();
    f.p.apply(Command::SetSeamAllowance {
        piece: f.q,
        seam: Some(pf_core::piece_seam::SeamAllowance {
            default_mm: 10.0,
            ..Default::default()
        }),
    })
    .unwrap();
    let pl = plan(&mut f.p, spec(1000.0, 0.0, 0.0, FabricFold::Flat, None));
    put(&mut f.p, pl, f.q, 0.0, 0.0, 0.0, false);
    let a = f.p.analyze_cutting_plan(pl).unwrap();
    let o = &a.outlines[0];
    assert!(near(o.bbox.width(), 120.0, 1e-6) && near(o.bbox.height(), 120.0, 1e-6));
    assert!(near(o.area_mm2, 14_400.0, 1e-3), "{}", o.area_mm2);
}

// ---------------------------------------------------------------------------
// Chevauchement, écart
// ---------------------------------------------------------------------------

#[test]
fn chevauchement_aire_exacte_et_contact() {
    let mut f = sc();
    let pl = plan(&mut f.p, spec(1000.0, 0.0, 0.0, FabricFold::Flat, None));
    let a = put(&mut f.p, pl, f.q, 0.0, 0.0, 0.0, false);
    let b = put(&mut f.p, pl, f.q, 50.0, 0.0, 0.0, false);
    let an = f.p.analyze_cutting_plan(pl).unwrap();
    assert_eq!(an.overlaps.len(), 1);
    assert_eq!((an.overlaps[0].a, an.overlaps[0].b), (a, b));
    assert!(near(an.overlaps[0].area_mm2, 5000.0, 1e-6)); // 50 x 100
    assert!(!an.is_valid());
    // Contact par un bord : aire 0, pas de chevauchement.
    f.p.apply(Command::UpdatePlacement {
        plan: pl,
        placement: b,
        size: None,
        x_mm: 100.0,
        y_mm: 0.0,
        rotation_deg: 0.0,
        mirrored: false,
    })
    .unwrap();
    assert!(f.p.analyze_cutting_plan(pl).unwrap().overlaps.is_empty());
    // 1 mm de recouvrement : 1 x 100 = 100 mm².
    f.p.apply(Command::UpdatePlacement {
        plan: pl,
        placement: b,
        size: None,
        x_mm: 99.0,
        y_mm: 0.0,
        rotation_deg: 0.0,
        mirrored: false,
    })
    .unwrap();
    let an = f.p.analyze_cutting_plan(pl).unwrap();
    assert!(near(an.overlaps[0].area_mm2, 100.0, 1e-6));
}

#[test]
fn ecart_mini_distance_exacte() {
    let mut f = sc();
    let pl = plan(&mut f.p, spec(1000.0, 0.0, 5.0, FabricFold::Flat, None));
    put(&mut f.p, pl, f.q, 0.0, 0.0, 0.0, false);
    let b = put(&mut f.p, pl, f.q, 103.0, 0.0, 0.0, false);
    let an = f.p.analyze_cutting_plan(pl).unwrap();
    assert_eq!(an.gaps.len(), 1);
    assert!(near(an.gaps[0].distance_mm, 3.0, 1e-9) && an.gaps[0].required_mm == 5.0);
    // Diagonale : coin à coin (3 ; 3) -> racine de 18 = 4,242640687.
    f.p.apply(Command::UpdatePlacement {
        plan: pl,
        placement: b,
        size: None,
        x_mm: 103.0,
        y_mm: 103.0,
        rotation_deg: 0.0,
        mirrored: false,
    })
    .unwrap();
    let an = f.p.analyze_cutting_plan(pl).unwrap();
    assert!(near(an.gaps[0].distance_mm, 4.242_640_687, 1e-6));
    // Exactement 5 mm : conforme.
    f.p.apply(Command::UpdatePlacement {
        plan: pl,
        placement: b,
        size: None,
        x_mm: 105.0,
        y_mm: 0.0,
        rotation_deg: 0.0,
        mirrored: false,
    })
    .unwrap();
    assert!(f.p.analyze_cutting_plan(pl).unwrap().is_valid());
}

#[test]
fn polygones_concaves_intersection_calculee_a_la_main() {
    // L : [0;100]x[0;50] u [0;50]x[50;100] ; carré [40;90]^2.
    // Part 1 : x 40..90, y 40..50 = 50 x 10 = 500 ; part 2 : x 40..50, y 50..90 = 10 x 40 = 400.
    let l = [
        pt(0.0, 0.0),
        pt(100.0, 0.0),
        pt(100.0, 50.0),
        pt(50.0, 50.0),
        pt(50.0, 100.0),
        pt(0.0, 100.0),
    ];
    let sq = [
        pt(40.0, 40.0),
        pt(90.0, 40.0),
        pt(90.0, 90.0),
        pt(40.0, 90.0),
    ];
    let v = intersection_area_mm2(&l, &sq);
    assert!(near(v, 900.0, 1e-9), "{v}");
    assert!(near(intersection_area_mm2(&sq, &l), 900.0, 1e-9));
    // Carré dans le creux du L : [60;90]x[60;90] ne le touche pas : distance au coin (50 ; 50)
    // du L = (10 ; 10) -> 14,142135624.
    let hole = [
        pt(60.0, 60.0),
        pt(90.0, 60.0),
        pt(90.0, 90.0),
        pt(60.0, 90.0),
    ];
    assert_eq!(intersection_area_mm2(&l, &hole), 0.0);
    assert!(near(polygon_distance_mm(&l, &hole), 10.0, 1e-9)); // jusqu'à y = 50 et x = 50
                                                               // Carré [110 ; 150] x [60 ; 90] : coin (110 ; 60) au coin (100 ; 50) du L = racine de 200.
    let far = [
        pt(110.0, 60.0),
        pt(150.0, 60.0),
        pt(150.0, 90.0),
        pt(110.0, 90.0),
    ];
    assert!(near(polygon_distance_mm(&l, &far), 14.142_135_624, 1e-6));
}

#[test]
fn deux_cercles_courbes_aplaties_a_la_tolerance() {
    let mut p = Project::new("Cercles");
    let s = p.sheets[0].id;
    let c = p
        .apply(Command::AddCircle {
            sheet: s,
            center: PointRef::New(pt(500.0, 500.0)),
            radius_mm: 50.0,
        })
        .unwrap();
    let piece = p
        .apply(Command::ExtractPiece {
            sheet: s,
            source: PieceSource::Paths {
                entities: c.entities,
            },
            name: "Rond".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    let pl = plan(&mut p, spec(1000.0, 0.0, 0.0, FabricFold::Flat, None));
    put(&mut p, pl, piece, 0.0, 0.0, 0.0, false);
    put(&mut p, pl, piece, 60.0, 0.0, 0.0, false);
    let an = p.analyze_cutting_plan(pl).unwrap();
    // Aire exacte du disque : pi * 2500 = 7853,98 (jamais l'aire aplatie).
    assert!(near(an.outlines[0].area_mm2, 7_853.981_634, 0.01));
    // Lentille de deux disques de rayon 50 à 60 mm : 2 r^2 acos(d / 2r) - (d / 2) sqrt(4r^2 - d^2)
    // = 5000 x 0,927295218 - 30 x 80 = 2236,476 (polygone inscrit : un peu moins, < 0,5 %).
    let lens = an.overlaps[0].area_mm2;
    assert!((2236.476 * 0.995..=2236.477).contains(&lens), "{lens}");
}

// ---------------------------------------------------------------------------
// Débordement : plat et plié
// ---------------------------------------------------------------------------

#[test]
fn debordement_plat_lisieres_et_debut() {
    let mut f = sc();
    let pl = plan(&mut f.p, spec(1500.0, 10.0, 0.0, FabricFold::Flat, None));
    let ok = put(&mut f.p, pl, f.q, 1390.0, 0.0, 0.0, false); // xmax = 1490
    let hors = put(&mut f.p, pl, f.q, 1391.0, 200.0, 0.0, false); // xmax = 1491
    let lis = put(&mut f.p, pl, f.q, 5.0, 400.0, 0.0, false); // xmin = 5 < 10
    let neg = put(&mut f.p, pl, f.q, 100.0, -1.0, 0.0, false);
    let an = f.p.analyze_cutting_plan(pl).unwrap();
    let ids: Vec<_> = an.overflows.iter().map(|o| o.placement).collect();
    assert!(!ids.contains(&ok));
    assert_eq!(ids, vec![hors, lis, neg]);
    assert_eq!(an.overflows[2].reasons, vec![OverflowReason::BeforeStart]);
}

#[test]
fn plie_demi_laize_une_piece_tient_d_un_seul_cote_du_pli() {
    // Laize 1000, lisière 10 : pli à 500, bandes [10 ; 500] et [500 ; 990].
    let mut f = sc();
    let pl = plan(&mut f.p, spec(1000.0, 10.0, 0.0, FabricFold::Folded, None));
    let g = put(&mut f.p, pl, f.q, 400.0, 0.0, 0.0, false); // 400..500 : ok
    let d = put(&mut f.p, pl, f.q, 500.0, 200.0, 0.0, false); // 500..600 : ok
    let pli = put(&mut f.p, pl, f.q, 450.0, 400.0, 0.0, false); // 450..550 : enjambe
    let hors = put(&mut f.p, pl, f.q, 905.0, 600.0, 0.0, false); // 905..1005 : hors zone
    let an = f.p.analyze_cutting_plan(pl).unwrap();
    let find = |id| an.overflows.iter().find(|o| o.placement == id);
    assert!(find(g).is_none() && find(d).is_none());
    assert_eq!(
        find(pli).unwrap().reasons,
        vec![OverflowReason::CrossesFold]
    );
    assert_eq!(
        find(hors).unwrap().reasons,
        vec![OverflowReason::OutsideUseful]
    );
    // Même position à plat : 450..550 tient dans [10 ; 990].
    f.p.apply(Command::UpdateCuttingPlan {
        plan: pl,
        spec: spec(1000.0, 10.0, 0.0, FabricFold::Flat, None),
    })
    .unwrap();
    let an = f.p.analyze_cutting_plan(pl).unwrap();
    assert!(an.overflows.iter().all(|o| o.placement == hors));
}

// ---------------------------------------------------------------------------
// Droit-fil, miroir
// ---------------------------------------------------------------------------

#[test]
fn droit_fil_0_et_180_permis_90_et_270_refuses() {
    let mut f = sc();
    add_grain(&mut f, true, 350.0, 20.0, 80.0); // vertical : 90 degrés
    let shape = f.p.piece_shape(f.q, None).unwrap();
    let pl = plan(
        &mut f.p,
        spec(1000.0, 0.0, 0.0, FabricFold::Flat, Some(90.0)),
    );
    let plan_ref = f.p.cutting_plan(pl).unwrap().clone();
    let chk = |rot, m| Project::rotation_check(&plan_ref, &shape, rot, m);
    assert_eq!(chk(0.0, false), RotationCheck::Allowed);
    assert_eq!(chk(180.0, false), RotationCheck::Allowed);
    assert_eq!(chk(0.0, true), RotationCheck::Allowed);
    for r in [90.0, 270.0] {
        match chk(r, false) {
            RotationCheck::Refused { deviation_deg } => assert!(near(deviation_deg, 90.0, 1e-9)),
            other => panic!("{r} : {other:?}"),
        }
    }
    // Tolérance 0,01 : 0,005 admis, 0,02 refusé (écart 0,02).
    assert_eq!(chk(0.005, false), RotationCheck::Allowed);
    match chk(0.02, false) {
        RotationCheck::Refused { deviation_deg } => assert!(near(deviation_deg, 0.02, 1e-6)),
        other => panic!("{other:?}"),
    }
    // Plan de sens 0 : c'est 90 qui devient permis.
    let mut p0 = plan_ref.clone();
    p0.grain_direction = Some(0.0);
    assert!(matches!(
        Project::rotation_check(&p0, &shape, 0.0, false),
        RotationCheck::Refused { .. }
    ));
    assert_eq!(
        Project::rotation_check(&p0, &shape, 90.0, false),
        RotationCheck::Allowed
    );
    assert_eq!(
        Project::rotation_check(&p0, &shape, 270.0, false),
        RotationCheck::Allowed
    );
}

#[test]
fn rotation_refusee_est_diagnostiquee_et_invalide_le_plan() {
    let mut f = sc();
    add_grain(&mut f, true, 350.0, 20.0, 80.0);
    let pl = plan(
        &mut f.p,
        spec(1000.0, 0.0, 0.0, FabricFold::Flat, Some(90.0)),
    );
    let bad = put(&mut f.p, pl, f.q, 0.0, 0.0, 90.0, false);
    let an = f.p.analyze_cutting_plan(pl).unwrap();
    assert_eq!(an.rotation_refused.len(), 1);
    assert_eq!(an.rotation_refused[0].placement, bad);
    assert!(!an.is_valid());
}

#[test]
fn sans_droit_fil_ou_sans_sens_permis_mais_signale() {
    let mut f = sc();
    let sans = plan(
        &mut f.p,
        spec(1000.0, 0.0, 0.0, FabricFold::Flat, Some(90.0)),
    );
    let a = put(&mut f.p, sans, f.r, 0.0, 0.0, 90.0, false); // R n'a pas de droit-fil
    let an = f.p.analyze_cutting_plan(sans).unwrap();
    assert!(an.rotation_refused.is_empty());
    assert_eq!(an.warnings.len(), 1);
    assert_eq!(
        (an.warnings[0].placement, an.warnings[0].kind),
        (a, WarningKind::PieceWithoutGrain)
    );
    // Plan sans sens : signalé aussi pour une pièce qui a un droit-fil.
    add_grain(&mut f, true, 350.0, 20.0, 80.0);
    let nosense = {
        let mut s = spec(1000.0, 0.0, 0.0, FabricFold::Flat, None);
        s.name = "Sans sens".into();
        plan(&mut f.p, s)
    };
    let b = put(&mut f.p, nosense, f.q, 0.0, 0.0, 90.0, false);
    let an = f.p.analyze_cutting_plan(nosense).unwrap();
    assert!(an.rotation_refused.is_empty());
    assert_eq!(
        (an.warnings[0].placement, an.warnings[0].kind),
        (b, WarningKind::PlanWithoutGrain)
    );
}

#[test]
fn miroir_signale_sans_paire_jamais_bloquant() {
    let mut f = sc();
    let pl = plan(&mut f.p, spec(1000.0, 0.0, 0.0, FabricFold::Flat, None));
    let m = put(&mut f.p, pl, f.q, 100.0, 0.0, 0.0, true);
    let an = f.p.analyze_cutting_plan(pl).unwrap();
    assert!(an
        .warnings
        .iter()
        .any(|w| w.placement == m && w.kind == WarningKind::MirrorWithoutPair));
    assert!(an.is_valid());
    // Pièce déclarée en paire : pas de signalement.
    f.p.apply(Command::SetPieceInfo {
        piece: f.q,
        name: "Q".into(),
        quantity: 1,
        material: None,
        mirrored_pair: true,
    })
    .unwrap();
    let an = f.p.analyze_cutting_plan(pl).unwrap();
    assert!(an
        .warnings
        .iter()
        .all(|w| w.kind != WarningKind::MirrorWithoutPair));
}

#[test]
fn non_posee_et_posee_trop_souvent() {
    let mut f = sc();
    let pl = plan(&mut f.p, spec(1000.0, 0.0, 0.0, FabricFold::Flat, None));
    let an = f.p.analyze_cutting_plan(pl).unwrap();
    assert_eq!(an.unplaced, vec![f.q, f.r]);
    put(&mut f.p, pl, f.q, 0.0, 0.0, 0.0, false);
    put(&mut f.p, pl, f.q, 200.0, 0.0, 0.0, false);
    let an = f.p.analyze_cutting_plan(pl).unwrap();
    assert_eq!(an.unplaced, vec![f.r]);
    // Quantité 1 : posée 2 fois = trop.
    assert_eq!(an.over_placed.len(), 1);
    assert_eq!(
        (an.over_placed[0].placed, an.over_placed[0].expected),
        (2, 1)
    );
}

// ---------------------------------------------------------------------------
// Poser la sélection
// ---------------------------------------------------------------------------

#[test]
fn pose_en_rangees_calculee_a_la_main_et_un_seul_pas() {
    let mut f = sc();
    // Utile 980 (laize 1000, lisières 10), écart 5. R = 100 x 50 : 9 par rangée
    // (9 x 100 + 8 x 5 = 940 <= 980 ; 10 -> 1045 > 980).
    let pl = plan(&mut f.p, spec(1000.0, 10.0, 5.0, FabricFold::Flat, None));
    let wanted: Vec<_> = (0..10).map(|_| (f.r, None)).collect();
    let cmd = f.p.plan_row_layout(pl, &wanted).unwrap();
    let mut doc = Document::new(f.p);
    doc.execute(cmd).unwrap();
    let plan_ref = doc.project().cutting_plan(pl).unwrap();
    assert_eq!(plan_ref.placements.len(), 10);
    for (k, p) in plan_ref.placements.iter().take(9).enumerate() {
        assert!(
            near(p.x_mm, 10.0 + 105.0 * k as f64, 1e-6),
            "{k} {}",
            p.x_mm
        );
        assert!(near(p.y_mm, 0.0, 1e-6));
    }
    // 10e : nouvelle rangée à y = 50 + 5.
    assert!(near(plan_ref.placements[9].x_mm, 10.0, 1e-6));
    assert!(near(plan_ref.placements[9].y_mm, 55.0, 1e-6));
    assert!(doc
        .project()
        .analyze_cutting_plan(pl)
        .unwrap()
        .overlaps
        .is_empty());
    assert!(doc
        .project()
        .analyze_cutting_plan(pl)
        .unwrap()
        .gaps
        .is_empty());
    assert!(doc.undo());
    assert!(doc
        .project()
        .cutting_plan(pl)
        .unwrap()
        .placements
        .is_empty());
    assert!(!doc.can_undo() || doc.project().cutting_plans.len() == 1);
}

#[test]
fn pose_pliee_demi_laize_et_empilement_sur_l_existant() {
    let mut f = sc();
    // Plié : bande [10 ; 500] = 490 : 4 par rangée (4 x 100 + 3 x 5 = 415 ; 5 -> 520 > 490).
    let pl = plan(&mut f.p, spec(1000.0, 10.0, 5.0, FabricFold::Folded, None));
    let wanted: Vec<_> = (0..5).map(|_| (f.r, None)).collect();
    let cmd = f.p.plan_row_layout(pl, &wanted).unwrap();
    f.p.apply(cmd).unwrap();
    let pls = &f.p.cutting_plan(pl).unwrap().placements;
    assert!(near(pls[3].x_mm, 325.0, 1e-6));
    assert!(near(pls[4].y_mm, 55.0, 1e-6));
    assert!(f.p.analyze_cutting_plan(pl).unwrap().is_valid());
    // Deuxième pose : au-dessus de tout (max y = 55 + 50 = 105, + écart 5 = 110).
    let cmd = f.p.plan_row_layout(pl, &[(f.q, None)]).unwrap();
    f.p.apply(cmd).unwrap();
    let last =
        f.p.cutting_plan(pl)
            .unwrap()
            .placements
            .last()
            .copied()
            .unwrap();
    assert!(near(last.y_mm, 110.0, 1e-6) && near(last.x_mm, 10.0, 1e-6));
    assert!(f.p.analyze_cutting_plan(pl).unwrap().is_valid());
}

#[test]
fn pose_refuse_une_piece_trop_large_sans_trace() {
    let mut f = sc();
    // Utile = 100 - 20 = 80 < 100.
    let pl = plan(&mut f.p, spec(100.0, 10.0, 0.0, FabricFold::Flat, None));
    let before = f.p.clone();
    let e =
        f.p.plan_row_layout(pl, &[(f.q, None)])
            .unwrap_err()
            .to_string();
    assert!(e.contains("Q") && e.contains("plus large"), "{e}");
    assert_eq!(f.p, before);
    assert!(f.p.plan_row_layout(pl, &[]).is_err());
}

#[test]
fn pose_tourne_si_le_droit_fil_l_impose() {
    let mut f = sc();
    add_grain(&mut f, false, 650.0, 10.0, 40.0); // droit-fil vertical dans R
                                                 // Plan de sens 0 (horizontal) : rotation 0 refusée, 180 refusée aussi (vertical), 90 ok.
    let pl = plan(
        &mut f.p,
        spec(1000.0, 0.0, 0.0, FabricFold::Flat, Some(0.0)),
    );
    let cmd = f.p.plan_row_layout(pl, &[(f.r, None)]).unwrap();
    f.p.apply(cmd).unwrap();
    let p0 = f.p.cutting_plan(pl).unwrap().placements[0];
    assert_eq!(p0.rotation_deg, 90.0);
    // R tourné de 90 : 50 de large, 100 de haut ; coin bas-gauche en (0 ; 0).
    let an = f.p.analyze_cutting_plan(pl).unwrap();
    assert!(
        near(an.outlines[0].bbox.min.x, 0.0, 1e-9) && near(an.outlines[0].bbox.min.y, 0.0, 1e-9)
    );
    assert!(near(an.outlines[0].bbox.width(), 50.0, 1e-9));
    assert!(an.is_valid());
}

#[test]
fn matiere_du_plan_filtre_les_pieces_a_poser() {
    let mut f = sc();
    f.p.apply(Command::SetPieceInfo {
        piece: f.q,
        name: "Q".into(),
        quantity: 1,
        material: Some("Coton".into()),
        mirrored_pair: false,
    })
    .unwrap();
    let mut s = spec(1000.0, 0.0, 0.0, FabricFold::Flat, None);
    s.material = Some("coton".into());
    let pl = plan(&mut f.p, s);
    let an = f.p.analyze_cutting_plan(pl).unwrap();
    // La casse est ignorée ; R (sans matière) n'est pas candidate.
    assert_eq!(an.unplaced, vec![f.q]);
}
