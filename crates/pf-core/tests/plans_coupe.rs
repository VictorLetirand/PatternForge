//! Plans de coupe (socle d'E15, schéma 5 inchangé) : stockage, commandes, validation, intégrité
//! référentielle, persistance. Aucune interface, aucun export, aucun placement automatique.
//! Campagne : `adverse_plans_coupe.rs` ; schéma et `pf-validate` : `format_ouvert.rs`.
//!
//! **Valeurs et chaînes écrites à la main**, jamais relues du code testé.
//!
//! Fixture : deux rectangles « A » (100 × 50) et « B » (100 × 20) sur une feuille, une gamme
//! « S, M, L » de base M.

use pf_core::cutting_plan::{CuttingPlanSpec, FabricFold};
use pf_core::persist;
use pf_core::*;
use pf_geometry::PointMm;
use serde_json::{json, Value};

fn bytes(p: &Project) -> String {
    persist::to_json(p).unwrap()
}

struct Sc {
    p: Project,
    s: SheetId,
    a: PieceId,
    b: PieceId,
    sizes: Vec<SizeId>,
}

fn rect_piece(p: &mut Project, s: SheetId, x: f64, h: f64, name: &str) -> PieceId {
    let r = p
        .apply(Command::AddRectangle {
            sheet: s,
            corner: PointRef::New(PointMm::new(x, 0.0)),
            width_mm: 100.0,
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

fn sc() -> Sc {
    let mut p = Project::new("Plans de coupe");
    let s = p.sheets[0].id;
    let a = rect_piece(&mut p, s, 0.0, 50.0, "A");
    let b = rect_piece(&mut p, s, 200.0, 20.0, "B");
    p.apply(Command::AddSizeRange {
        name: "Femme".into(),
        kind: SizeKind::Alphanumeric,
        labels: vec!["S".into(), "M".into(), "L".into()],
        base: 1,
    })
    .unwrap();
    let sizes = p.sizes[0].sizes.iter().map(|z| z.id).collect();
    Sc { p, s, a, b, sizes }
}

fn spec(name: &str) -> CuttingPlanSpec {
    CuttingPlanSpec {
        name: name.into(),
        material: None,
        fabric_width_mm: 1500.0,
        fold: FabricFold::Flat,
        selvedge_mm: 10.0,
        min_gap_mm: 5.0,
        grain_direction: None,
    }
}

fn add_plan(p: &mut Project, name: &str) -> CuttingPlanId {
    p.apply(Command::AddCuttingPlan { spec: spec(name) })
        .unwrap()
        .cutting_plans[0]
}

#[allow(clippy::too_many_arguments)]
fn put(
    p: &mut Project,
    plan: CuttingPlanId,
    piece: PieceId,
    size: Option<SizeId>,
    x: f64,
    y: f64,
    rot: f64,
    mirrored: bool,
) -> Result<PlacementId, CoreError> {
    p.apply(Command::AddPlacement {
        plan,
        piece,
        size,
        x_mm: x,
        y_mm: y,
        rotation_deg: rot,
        mirrored,
    })
    .map(|o| o.placements[0])
}

/// Refus : message ; le projet est identique (valeur et octets).
fn refused(p: &mut Project, c: Command) -> String {
    let (before, text) = (p.clone(), bytes(p));
    let e = p.apply(c).expect_err("la commande devait être refusée");
    assert_eq!(*p, before);
    assert_eq!(bytes(p), text, "refus avec trace");
    e.to_string()
}

// ---------------------------------------------------------------------------
// Stockage : forme exacte, omission, aller-retour
// ---------------------------------------------------------------------------

#[test]
fn stockage_omis_quand_vide_et_forme_exacte() {
    let mut f = sc();
    assert!(!bytes(&f.p).contains("cutting_plans"));
    let plan =
        f.p.apply(Command::AddCuttingPlan {
            spec: CuttingPlanSpec {
                name: "Plan A".into(),
                material: Some("Coton".into()),
                fabric_width_mm: 1500.0,
                fold: FabricFold::Folded,
                selvedge_mm: 10.0,
                min_gap_mm: 5.0,
                grain_direction: Some(90.0),
            },
        })
        .unwrap()
        .cutting_plans[0];
    // Plan sans placement : clé `placements` omise.
    let json = serde_json::to_string(&f.p.cutting_plans[0]).unwrap();
    assert_eq!(
        json,
        format!(
            "{{\"id\":{},\"name\":\"Plan A\",\"material\":\"Coton\",\"fabric_width_mm\":1500.0,\
             \"fold\":\"folded\",\"selvedge_mm\":10.0,\"min_gap_mm\":5.0,\"grain_direction\":90.0}}",
            plan.0
        )
    );
    let pl1 = put(
        &mut f.p,
        plan,
        f.a,
        Some(f.sizes[2]),
        12.5,
        -3.0,
        90.0,
        true,
    )
    .unwrap();
    let pl2 = put(&mut f.p, plan, f.b, None, 0.0, 200.0, 0.0, false).unwrap();
    let json = serde_json::to_string(&f.p.cutting_plans[0]).unwrap();
    assert_eq!(
        json,
        format!(
            "{{\"id\":{},\"name\":\"Plan A\",\"material\":\"Coton\",\"fabric_width_mm\":1500.0,\
             \"fold\":\"folded\",\"selvedge_mm\":10.0,\"min_gap_mm\":5.0,\"grain_direction\":90.0,\
             \"placements\":[{{\"id\":{},\"piece\":{},\"size\":{},\"x_mm\":12.5,\"y_mm\":-3.0,\
             \"rotation_deg\":90.0,\"mirrored\":true}},{{\"id\":{},\"piece\":{},\"x_mm\":0.0,\
             \"y_mm\":200.0,\"rotation_deg\":0.0}}]}}",
            plan.0, pl1.0, f.a.0, f.sizes[2].0, pl2.0, f.b.0
        )
    );
    // Ordre de sérialisation : après `sizes`.
    let text = bytes(&f.p);
    assert!(text.find("\"sizes\"").unwrap() < text.find("\"cutting_plans\"").unwrap());
    // Aller-retour octet à octet.
    let again = persist::from_json(&text).unwrap();
    assert_eq!(again, f.p);
    assert_eq!(bytes(&again), text);
    // Retirer le plan : la clé disparaît.
    f.p.apply(Command::RemoveCuttingPlan { plan }).unwrap();
    assert!(!bytes(&f.p).contains("cutting_plans"));
}

#[test]
fn ids_stables_sans_reutilisation() {
    let mut f = sc();
    let first = add_plan(&mut f.p, "P1");
    let pl = put(&mut f.p, first, f.a, None, 0.0, 0.0, 0.0, false).unwrap();
    f.p.apply(Command::RemoveCuttingPlan { plan: first })
        .unwrap();
    let second = add_plan(&mut f.p, "P1");
    let pl2 = put(&mut f.p, second, f.a, None, 0.0, 0.0, 0.0, false).unwrap();
    assert!(second.0 > pl.0, "id de plan réutilisé");
    assert!(pl2.0 > second.0);
    assert_ne!(first, second);
    assert_ne!(pl, pl2);
    // Un même identifiant n'est jamais partagé avec un autre objet du projet.
    let mut all = vec![second.0, pl2.0];
    all.extend(f.p.pieces.iter().map(|x| x.id.0));
    all.extend(f.p.sheets.iter().map(|x| x.id.0));
    let n = all.len();
    all.sort_unstable();
    all.dedup();
    assert_eq!(all.len(), n);
}

// ---------------------------------------------------------------------------
// Commandes : valeurs acceptées / refusées (frontières écrites à la main)
// ---------------------------------------------------------------------------

fn add_spec(f: &mut Sc, s: CuttingPlanSpec) -> Result<Outcome, CoreError> {
    f.p.apply(Command::AddCuttingPlan { spec: s })
}

#[test]
fn plan_valeurs_acceptees_aux_frontieres() {
    let mut f = sc();
    let mut ok = |name: &str, edit: &dyn Fn(&mut CuttingPlanSpec)| {
        let mut s = spec(name);
        edit(&mut s);
        add_spec(&mut f, s).unwrap();
    };
    ok("laize max", &|s| {
        s.fabric_width_mm = 10_000.0;
        s.selvedge_mm = 4_999.999;
    });
    ok("laize petite", &|s| {
        s.fabric_width_mm = 0.001;
        s.selvedge_mm = 0.0;
    });
    ok("écart max", &|s| s.min_gap_mm = 1000.0);
    ok("écart nul", &|s| s.min_gap_mm = 0.0);
    ok("droit-fil 0", &|s| s.grain_direction = Some(0.0));
    ok("droit-fil 359,999", &|s| s.grain_direction = Some(359.999));
    ok("matière", &|s| s.material = Some("Cuir de veau".into()));
    // 2 × lisière strictement sous la laize : 2 × 749,999 = 1499,998 < 1500.
    ok("lisière limite", &|s| s.selvedge_mm = 749.999);
    assert_eq!(f.p.cutting_plans.len(), 8);
}

#[test]
fn plan_valeurs_refusees_sans_trace() {
    let mut f = sc();
    add_plan(&mut f.p, "Existant");
    type Case = (
        &'static str,
        Box<dyn Fn(&mut CuttingPlanSpec)>,
        &'static str,
    );
    let cases: Vec<Case> = vec![
        ("nom vide", Box::new(|s| s.name = String::new()), "vide"),
        ("nom blanc", Box::new(|s| s.name = "   ".into()), "vide"),
        (
            "nom trop long",
            Box::new(|s| s.name = "x".repeat(121)),
            "120",
        ),
        (
            "nom de contrôle",
            Box::new(|s| s.name = "a\u{7}b".into()),
            "contrôle",
        ),
        (
            "nom pris (casse)",
            Box::new(|s| s.name = "eXISTANT".into()),
            "existe déjà",
        ),
        (
            "matière vide",
            Box::new(|s| s.material = Some(" ".into())),
            "vide",
        ),
        (
            "laize nulle",
            Box::new(|s| s.fabric_width_mm = 0.0),
            "laize",
        ),
        (
            "laize négative",
            Box::new(|s| s.fabric_width_mm = -1.0),
            "laize",
        ),
        (
            "laize NaN",
            Box::new(|s| s.fabric_width_mm = f64::NAN),
            "laize",
        ),
        (
            "laize infinie",
            Box::new(|s| s.fabric_width_mm = f64::INFINITY),
            "laize",
        ),
        (
            "laize > 10 000",
            Box::new(|s| s.fabric_width_mm = 10_000.001),
            "laize",
        ),
        (
            "lisière négative",
            Box::new(|s| s.selvedge_mm = -0.001),
            "lisière",
        ),
        (
            "lisière NaN",
            Box::new(|s| s.selvedge_mm = f64::NAN),
            "lisière",
        ),
        (
            "2 × lisière = laize",
            Box::new(|s| s.selvedge_mm = 750.0),
            "lisières",
        ),
        (
            "2 × lisière > laize",
            Box::new(|s| s.selvedge_mm = 900.0),
            "lisières",
        ),
        (
            "lisière infinie",
            Box::new(|s| s.selvedge_mm = f64::INFINITY),
            "lisière",
        ),
        (
            "écart négatif",
            Box::new(|s| s.min_gap_mm = -1.0),
            "écart minimal",
        ),
        (
            "écart > 1 000",
            Box::new(|s| s.min_gap_mm = 1000.001),
            "écart minimal",
        ),
        (
            "écart NaN",
            Box::new(|s| s.min_gap_mm = f64::NAN),
            "écart minimal",
        ),
        (
            "droit-fil 360",
            Box::new(|s| s.grain_direction = Some(360.0)),
            "droit-fil",
        ),
        (
            "droit-fil négatif",
            Box::new(|s| s.grain_direction = Some(-0.5)),
            "droit-fil",
        ),
        (
            "droit-fil NaN",
            Box::new(|s| s.grain_direction = Some(f64::NAN)),
            "droit-fil",
        ),
    ];
    for (name, edit, word) in cases {
        let mut s = spec("Nouveau");
        edit(&mut s);
        let msg = refused(&mut f.p, Command::AddCuttingPlan { spec: s.clone() });
        assert!(msg.contains(word), "{name} : {msg}");
        // Même refus pour la mise à jour d'un plan existant (le nom « Existant » est le sien).
        if name == "nom pris (casse)" {
            continue;
        }
        let id = f.p.cutting_plans[0].id;
        let msg = refused(&mut f.p, Command::UpdateCuttingPlan { plan: id, spec: s });
        assert!(msg.contains(word), "{name} (mise à jour) : {msg}");
    }
}

#[test]
fn nom_pris_par_un_autre_plan_refuse_a_la_mise_a_jour() {
    let mut f = sc();
    let a = add_plan(&mut f.p, "Alpha");
    add_plan(&mut f.p, "Beta");
    let msg = refused(
        &mut f.p,
        Command::UpdateCuttingPlan {
            plan: a,
            spec: spec("BETA"),
        },
    );
    assert!(msg.contains("existe déjà"), "{msg}");
    // Son propre nom, même en changeant la casse, reste permis.
    f.p.apply(Command::UpdateCuttingPlan {
        plan: a,
        spec: spec("ALPHA"),
    })
    .unwrap();
    assert_eq!(f.p.cutting_plans[0].name, "ALPHA");
}

#[test]
fn nom_et_matiere_sont_rognes() {
    let mut f = sc();
    let mut s = spec("  Plan  ");
    s.material = Some(" Lin ".into());
    add_spec(&mut f, s).unwrap();
    assert_eq!(f.p.cutting_plans[0].name, "Plan");
    assert_eq!(f.p.cutting_plans[0].material.as_deref(), Some("Lin"));
}

#[test]
fn placement_normalisation_et_bornes() {
    let mut f = sc();
    let plan = add_plan(&mut f.p, "P");
    // (rotation demandée, rotation stockée) calculés à la main :
    // 450 = 360 + 90 ; -90 = -360 + 270 ; 720 = 2 × 360 ; 1e6 = 2777 × 360 + 280.
    for (asked, stored) in [
        (450.0, 90.0),
        (-90.0, 270.0),
        (720.0, 0.0),
        (360.0, 0.0),
        (1.0e6, 280.0),
        (-1.0e-20, 0.0),
        (359.5, 359.5),
    ] {
        let id = put(&mut f.p, plan, f.a, None, 0.0, 0.0, asked, false).unwrap();
        let r = f.p.cutting_plans[0]
            .placements
            .iter()
            .find(|p| p.id == id)
            .unwrap()
            .rotation_deg;
        assert_eq!(r, stored, "rotation {asked}");
        assert!(r.is_sign_positive(), "zéro négatif écrit pour {asked}");
    }
    // Position aux bornes acceptée, au-delà refusée.
    put(&mut f.p, plan, f.a, None, 1.0e6, -1.0e6, 0.0, false).unwrap();
    let n = f.p.cutting_plans[0].placements.len();
    for (x, y, r) in [
        (1.0e6 + 1.0, 0.0, 0.0),
        (0.0, -1.0e6 - 1.0, 0.0),
        (f64::NAN, 0.0, 0.0),
        (0.0, f64::INFINITY, 0.0),
        (0.0, 0.0, f64::NAN),
        (0.0, 0.0, 1.0e6 + 1.0),
        (0.0, 0.0, f64::NEG_INFINITY),
    ] {
        refused(
            &mut f.p,
            Command::AddPlacement {
                plan,
                piece: f.a,
                size: None,
                x_mm: x,
                y_mm: y,
                rotation_deg: r,
                mirrored: false,
            },
        );
    }
    assert_eq!(f.p.cutting_plans[0].placements.len(), n);
}

#[test]
fn placement_references_et_doublons() {
    let mut f = sc();
    let plan = add_plan(&mut f.p, "P");
    // Une pièce posée deux fois avec les mêmes paramètres : deux placements distincts (quantité).
    let p1 = put(&mut f.p, plan, f.a, Some(f.sizes[0]), 1.0, 2.0, 30.0, false).unwrap();
    let p2 = put(&mut f.p, plan, f.a, Some(f.sizes[0]), 1.0, 2.0, 30.0, false).unwrap();
    assert_ne!(p1, p2);
    assert_eq!(f.p.cutting_plans[0].placements.len(), 2);
    // Références inconnues.
    let ghost_piece = PieceId(99_999);
    let ghost_size = SizeId(99_999);
    let ghost_plan = CuttingPlanId(99_999);
    let add = |plan, piece, size| Command::AddPlacement {
        plan,
        piece,
        size,
        x_mm: 0.0,
        y_mm: 0.0,
        rotation_deg: 0.0,
        mirrored: false,
    };
    refused(&mut f.p, add(plan, ghost_piece, None));
    let msg = refused(&mut f.p, add(plan, f.a, Some(ghost_size)));
    assert!(msg.contains("taille"), "{msg}");
    refused(&mut f.p, add(ghost_plan, f.a, None));
    refused(
        &mut f.p,
        Command::RemovePlacement {
            plan,
            placement: PlacementId(99_999),
        },
    );
    refused(&mut f.p, Command::RemoveCuttingPlan { plan: ghost_plan });
    // Un placement d'un autre plan n'est pas atteignable par ce plan.
    let other = add_plan(&mut f.p, "Q");
    refused(
        &mut f.p,
        Command::RemovePlacement {
            plan: other,
            placement: p1,
        },
    );
    refused(
        &mut f.p,
        Command::UpdatePlacement {
            plan: other,
            placement: p1,
            size: None,
            x_mm: 0.0,
            y_mm: 0.0,
            rotation_deg: 0.0,
            mirrored: false,
        },
    );
}

#[test]
fn mise_a_jour_du_placement_garde_la_piece_et_refuse_taille_inconnue() {
    let mut f = sc();
    let plan = add_plan(&mut f.p, "P");
    let pl = put(&mut f.p, plan, f.b, Some(f.sizes[0]), 1.0, 2.0, 30.0, false).unwrap();
    f.p.apply(Command::UpdatePlacement {
        plan,
        placement: pl,
        size: None,
        x_mm: -7.5,
        y_mm: 8.0,
        rotation_deg: -30.0,
        mirrored: true,
    })
    .unwrap();
    let q = f.p.cutting_plans[0].placements[0];
    assert_eq!(
        (q.piece, q.size, q.x_mm, q.y_mm, q.rotation_deg, q.mirrored),
        (f.b, None, -7.5, 8.0, 330.0, true)
    );
    let msg = refused(
        &mut f.p,
        Command::UpdatePlacement {
            plan,
            placement: pl,
            size: Some(SizeId(99_999)),
            x_mm: 0.0,
            y_mm: 0.0,
            rotation_deg: 0.0,
            mirrored: false,
        },
    );
    assert!(msg.contains("taille"), "{msg}");
    refused(
        &mut f.p,
        Command::UpdatePlacement {
            plan,
            placement: pl,
            size: None,
            x_mm: f64::NAN,
            y_mm: 0.0,
            rotation_deg: 0.0,
            mirrored: false,
        },
    );
}

// ---------------------------------------------------------------------------
// Historique : une étape, même valeur = aucune étape, annuler / rétablir exacts
// ---------------------------------------------------------------------------

#[test]
fn historique_une_etape_et_aucune_pour_la_meme_valeur() {
    let f = sc();
    let (a, sizes) = (f.a, f.sizes.clone());
    let mut doc = Document::new(f.p);
    let start = bytes(doc.project());
    let plan = doc
        .execute(Command::AddCuttingPlan { spec: spec("P") })
        .unwrap()
        .cutting_plans[0];
    let after_add = bytes(doc.project());
    // Même valeur : aucun pas.
    doc.execute(Command::UpdateCuttingPlan {
        plan,
        spec: spec("P"),
    })
    .unwrap();
    assert_eq!(bytes(doc.project()), after_add);
    let pl = doc
        .execute(Command::AddPlacement {
            plan,
            piece: a,
            size: Some(sizes[1]),
            x_mm: 10.0,
            y_mm: 20.0,
            rotation_deg: 10.0,
            mirrored: false,
        })
        .unwrap()
        .placements[0];
    let after_put = bytes(doc.project());
    // 370 ≡ 10 : même valeur après normalisation, aucun pas.
    doc.execute(Command::UpdatePlacement {
        plan,
        placement: pl,
        size: Some(sizes[1]),
        x_mm: 10.0,
        y_mm: 20.0,
        rotation_deg: 370.0,
        mirrored: false,
    })
    .unwrap();
    assert_eq!(bytes(doc.project()), after_put);
    // Un seul pas d'annulation par commande effective : 2 annulations = retour au départ.
    assert!(doc.undo());
    assert_eq!(bytes(doc.project()), after_add);
    assert!(doc.undo());
    assert_eq!(bytes(doc.project()), start);
    assert!(!doc.can_undo(), "pas supplémentaire créé par un no-op");
    assert!(doc.redo());
    assert!(doc.redo());
    assert_eq!(bytes(doc.project()), after_put);
    // Un changement réel de la rotation crée un pas et s'annule exactement.
    doc.execute(Command::UpdatePlacement {
        plan,
        placement: pl,
        size: Some(sizes[1]),
        x_mm: 10.0,
        y_mm: 20.0,
        rotation_deg: 11.0,
        mirrored: false,
    })
    .unwrap();
    assert_ne!(bytes(doc.project()), after_put);
    assert!(doc.undo());
    assert_eq!(bytes(doc.project()), after_put);
    doc.execute(Command::RemovePlacement {
        plan,
        placement: pl,
    })
    .unwrap();
    assert!(doc.undo());
    assert_eq!(bytes(doc.project()), after_put);
    doc.execute(Command::RemoveCuttingPlan { plan }).unwrap();
    assert!(doc.undo());
    assert_eq!(bytes(doc.project()), after_put);
}

// ---------------------------------------------------------------------------
// Intégrité référentielle : supprimer une pièce / une taille posée est REFUSÉ
// ---------------------------------------------------------------------------

#[test]
fn supprimer_une_piece_posee_est_refuse_en_nommant_le_plan() {
    let mut f = sc();
    let plan = add_plan(&mut f.p, "Plan manteau");
    let pl = put(&mut f.p, plan, f.a, None, 0.0, 0.0, 0.0, false).unwrap();
    let msg = refused(&mut f.p, Command::DeletePiece { piece: f.a });
    assert!(msg.contains("Plan manteau"), "{msg}");
    // Une pièce non posée se supprime.
    f.p.apply(Command::DeletePiece { piece: f.b }).unwrap();
    // Après retrait du placement, la pièce posée se supprime aussi.
    f.p.apply(Command::RemovePlacement {
        plan,
        placement: pl,
    })
    .unwrap();
    f.p.apply(Command::DeletePiece { piece: f.a }).unwrap();
    assert!(f.p.pieces.is_empty());
}

#[test]
fn supprimer_une_feuille_qui_porte_une_piece_posee_est_refuse() {
    let mut f = sc();
    // Seconde feuille pour que la suppression de la première soit possible.
    f.p.apply(Command::CreateSheet {
        name: "Autre".into(),
    })
    .unwrap();
    let plan = add_plan(&mut f.p, "Plan feuille");
    put(&mut f.p, plan, f.b, None, 0.0, 0.0, 0.0, false).unwrap();
    let msg = refused(&mut f.p, Command::DeleteSheet { sheet: f.s });
    assert!(msg.contains("Plan feuille"), "{msg}");
}

#[test]
fn supprimer_une_taille_ou_une_gamme_posee_est_refuse() {
    let mut f = sc();
    let plan = add_plan(&mut f.p, "Plan tailles");
    let pl = put(&mut f.p, plan, f.a, Some(f.sizes[2]), 0.0, 0.0, 0.0, false).unwrap();
    let msg = refused(&mut f.p, Command::RemoveSize { size: f.sizes[2] });
    assert!(msg.contains("Plan tailles"), "{msg}");
    let range = f.p.sizes[0].id;
    let msg = refused(&mut f.p, Command::RemoveSizeRange { range });
    assert!(msg.contains("Plan tailles"), "{msg}");
    // Une taille non posée se supprime ; poser en taille de base (None) n'empêche rien.
    f.p.apply(Command::RemoveSize { size: f.sizes[0] }).unwrap();
    // Après retrait du placement, la taille se supprime.
    f.p.apply(Command::RemovePlacement {
        plan,
        placement: pl,
    })
    .unwrap();
    f.p.apply(Command::RemoveSize { size: f.sizes[2] }).unwrap();
}

#[test]
fn supprimer_le_plan_libere_ses_references() {
    let mut f = sc();
    let plan = add_plan(&mut f.p, "P");
    put(&mut f.p, plan, f.a, Some(f.sizes[0]), 0.0, 0.0, 0.0, false).unwrap();
    f.p.apply(Command::RemoveCuttingPlan { plan }).unwrap();
    f.p.apply(Command::RemoveSize { size: f.sizes[0] }).unwrap();
    f.p.apply(Command::DeletePiece { piece: f.a }).unwrap();
}

#[test]
fn duplication_de_feuille_ne_copie_pas_les_plans() {
    let mut f = sc();
    let plan = add_plan(&mut f.p, "P");
    put(&mut f.p, plan, f.a, None, 5.0, 6.0, 0.0, false).unwrap();
    let before = f.p.cutting_plans.clone();
    let out = f.p.apply(Command::DuplicateSheet { sheet: f.s }).unwrap();
    // Plans inchangés : même contenu, pas de nouveau plan, pas de placement de la copie.
    assert_eq!(f.p.cutting_plans, before);
    assert_eq!(f.p.pieces.len(), 4);
    assert!(out
        .notes
        .iter()
        .any(|n| n.contains("plans de coupe") && n.contains("« A »")));
    assert!(!out.notes.iter().any(|n| n.contains("« B »")));
    // Enregistrer sous un autre nom (copie du projet) garde les plans tels quels.
    let copy = persist::from_json(&bytes(&f.p)).unwrap();
    assert_eq!(copy.cutting_plans, before);
}

// ---------------------------------------------------------------------------
// Persistance : refus au chargement
// ---------------------------------------------------------------------------

fn full_json() -> (Value, Sc) {
    let mut f = sc();
    let plan = add_plan(&mut f.p, "Plan A");
    put(&mut f.p, plan, f.a, Some(f.sizes[0]), 1.0, 2.0, 3.0, true).unwrap();
    put(&mut f.p, plan, f.b, None, 4.0, 5.0, 6.0, false).unwrap();
    add_plan(&mut f.p, "Plan B");
    (serde_json::from_str(&bytes(&f.p)).unwrap(), f)
}

fn load(v: &Value) -> Result<Project, CoreError> {
    persist::from_json(&v.to_string())
}

#[test]
fn chargement_accepte_le_plan_complet() {
    let (v, f) = full_json();
    assert_eq!(load(&v).unwrap(), f.p);
}

#[test]
fn cle_reservee_refusee_dans_un_fichier_anterieur_a_la_v5() {
    let (mut v, _) = full_json();
    // `sizes` (clé de la v4) est retirée : seule la clé des plans doit motiver le refus.
    v.as_object_mut().unwrap().remove("sizes");
    for version in 1..=4 {
        v["schema_version"] = json!(version);
        let e = load(&v).expect_err("clé v5 dans un fichier plus ancien");
        let m = e.to_string();
        assert!(
            m.contains("cutting_plans") && m.contains("version 5"),
            "v{version} : {m}"
        );
    }
}

#[test]
fn chargement_refuse_chaque_cas_invalide() {
    let (good, f) = full_json();
    let pl0 = f.p.cutting_plans[0].placements[0].id.0;
    let next = f.p.next_id;
    type Case = (&'static str, Box<dyn Fn(&mut Value)>);
    let cases: Vec<Case> = vec![
        (
            "clé inconnue dans un plan",
            Box::new(|d| d["cutting_plans"][0]["zz"] = json!(1)),
        ),
        (
            "clé inconnue dans un placement",
            Box::new(|d| d["cutting_plans"][0]["placements"][0]["zz"] = json!(1)),
        ),
        (
            "id de placement dupliqué",
            Box::new(move |d| d["cutting_plans"][0]["placements"][1]["id"] = json!(pl0)),
        ),
        (
            "id de plan égal à un placement",
            Box::new(move |d| d["cutting_plans"][1]["id"] = json!(pl0)),
        ),
        (
            "id de plan >= next_id",
            Box::new(move |d| d["cutting_plans"][1]["id"] = json!(next)),
        ),
        (
            "pièce inconnue",
            Box::new(|d| d["cutting_plans"][0]["placements"][0]["piece"] = json!(99999)),
        ),
        (
            "taille inconnue",
            Box::new(|d| d["cutting_plans"][0]["placements"][0]["size"] = json!(99999)),
        ),
        (
            "x non fini (1e999)",
            Box::new(|d| {
                *d = serde_json::from_str(&d.to_string().replacen(
                    "\"x_mm\":1.0",
                    "\"x_mm\":1e999",
                    1,
                ))
                .unwrap_or(Value::Null)
            }),
        ),
        (
            "x hors bornes",
            Box::new(|d| d["cutting_plans"][0]["placements"][0]["x_mm"] = json!(1000000.5)),
        ),
        (
            "y hors bornes",
            Box::new(|d| d["cutting_plans"][0]["placements"][0]["y_mm"] = json!(-1.0e7)),
        ),
        (
            "rotation 360",
            Box::new(|d| d["cutting_plans"][0]["placements"][0]["rotation_deg"] = json!(360.0)),
        ),
        (
            "rotation négative",
            Box::new(|d| d["cutting_plans"][0]["placements"][0]["rotation_deg"] = json!(-1.0)),
        ),
        (
            "laize nulle",
            Box::new(|d| d["cutting_plans"][0]["fabric_width_mm"] = json!(0.0)),
        ),
        (
            "laize négative",
            Box::new(|d| d["cutting_plans"][0]["fabric_width_mm"] = json!(-1500.0)),
        ),
        (
            "laize > 10 000",
            Box::new(|d| d["cutting_plans"][0]["fabric_width_mm"] = json!(10000.5)),
        ),
        (
            "lisière trop grande (2 × 750 = 1500)",
            Box::new(|d| d["cutting_plans"][0]["selvedge_mm"] = json!(750.0)),
        ),
        (
            "lisière négative",
            Box::new(|d| d["cutting_plans"][0]["selvedge_mm"] = json!(-1.0)),
        ),
        (
            "écart > 1 000",
            Box::new(|d| d["cutting_plans"][0]["min_gap_mm"] = json!(1000.5)),
        ),
        (
            "écart négatif",
            Box::new(|d| d["cutting_plans"][0]["min_gap_mm"] = json!(-0.5)),
        ),
        (
            "droit-fil 360",
            Box::new(|d| d["cutting_plans"][0]["grain_direction"] = json!(360.0)),
        ),
        (
            "droit-fil négatif",
            Box::new(|d| d["cutting_plans"][0]["grain_direction"] = json!(-10.0)),
        ),
        (
            "nom vide",
            Box::new(|d| d["cutting_plans"][0]["name"] = json!("")),
        ),
        (
            "nom blanc",
            Box::new(|d| d["cutting_plans"][0]["name"] = json!("  ")),
        ),
        (
            "nom avec blancs en tête",
            Box::new(|d| d["cutting_plans"][0]["name"] = json!(" Plan A")),
        ),
        (
            "nom dupliqué (casse)",
            Box::new(|d| d["cutting_plans"][1]["name"] = json!("PLAN a")),
        ),
        (
            "matière vide",
            Box::new(|d| d["cutting_plans"][0]["material"] = json!("")),
        ),
        (
            "pliage inconnu",
            Box::new(|d| d["cutting_plans"][0]["fold"] = json!("rolled")),
        ),
        (
            "laize texte",
            Box::new(|d| d["cutting_plans"][0]["fabric_width_mm"] = json!("1500")),
        ),
        (
            "miroir texte",
            Box::new(|d| d["cutting_plans"][0]["placements"][0]["mirrored"] = json!("oui")),
        ),
        (
            "champ obligatoire absent (laize)",
            Box::new(|d| {
                d["cutting_plans"][0]
                    .as_object_mut()
                    .unwrap()
                    .remove("fabric_width_mm");
            }),
        ),
    ];
    for (name, mutate) in cases {
        let mut doc = good.clone();
        mutate(&mut doc);
        assert_ne!(doc, good, "{name} : la mutation n'a rien changé");
        // `x non fini` ne produit pas un `Value` : on teste directement le texte.
        let refused = if doc.is_null() {
            persist::from_json(
                &good
                    .to_string()
                    .replacen("\"x_mm\":1.0", "\"x_mm\":1e999", 1),
            )
            .is_err()
        } else {
            load(&doc).is_err()
        };
        assert!(refused, "{name} : le chargeur accepte");
    }
}

#[test]
fn nombres_non_finis_refuses_par_validate_et_par_l_ecriture_sans_panique() {
    let (_, f) = full_json();
    type Edit = Box<dyn Fn(&mut Project)>;
    let edits: Vec<Edit> = vec![
        Box::new(|p| p.cutting_plans[0].fabric_width_mm = f64::NAN),
        Box::new(|p| p.cutting_plans[0].selvedge_mm = f64::INFINITY),
        Box::new(|p| p.cutting_plans[0].min_gap_mm = f64::NAN),
        Box::new(|p| p.cutting_plans[0].grain_direction = Some(f64::NAN)),
        Box::new(|p| p.cutting_plans[0].placements[0].x_mm = f64::NAN),
        Box::new(|p| p.cutting_plans[0].placements[0].y_mm = f64::NEG_INFINITY),
        Box::new(|p| p.cutting_plans[0].placements[0].rotation_deg = f64::NAN),
    ];
    for (i, edit) in edits.iter().enumerate() {
        let mut p = f.p.clone();
        edit(&mut p);
        assert!(p.validate().is_err(), "cas {i}");
        assert!(persist::to_json(&p).is_err(), "cas {i} : écriture");
    }
}

#[test]
fn pf_validate_accepte_le_plan_et_refuse_les_cas_invalides() {
    let (good, _) = full_json();
    let dir = std::env::temp_dir().join(format!("pf-plans-coupe-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let run = |name: &str, doc: &Value| -> i32 {
        let path = dir.join(name);
        std::fs::write(&path, doc.to_string()).unwrap();
        std::process::Command::new(env!("CARGO_BIN_EXE_pf-validate"))
            .arg(&path)
            .output()
            .unwrap()
            .status
            .code()
            .unwrap()
    };
    assert_eq!(run("ok.pforge.json", &good), 0);
    let mut bad = good.clone();
    bad["cutting_plans"][0]["placements"][0]["piece"] = json!(99999);
    assert_eq!(run("piece.pforge.json", &bad), 1);
    let mut bad = good.clone();
    bad["cutting_plans"][0]["fabric_width_mm"] = json!(0.0);
    assert_eq!(run("laize.pforge.json", &bad), 1);
    let mut old = good.clone();
    old["schema_version"] = json!(4);
    assert_eq!(run("v4.pforge.json", &old), 1);
    let _ = std::fs::remove_dir_all(&dir);
}
