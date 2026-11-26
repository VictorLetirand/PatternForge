//! US-04.1 — extraire une pièce : un test par critère d'acceptation, plus le
//! refus sans trace, les calques, l'annulation, le fichier et les copies.

use pf_core::persist;
use pf_core::*;
use pf_geometry::{PointMm, RegionError, Side};
use std::f64::consts::PI;

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

fn setup() -> (Project, SheetId) {
    let p = Project::new("pièces");
    let s = p.sheets[0].id;
    (p, s)
}

/// Rectangle de quatre tracés qui ne partagent que leurs coins.
fn rect(p: &mut Project, s: SheetId, x0: f64, y0: f64, w: f64, h: f64) -> Outcome {
    p.apply(Command::AddRectangle {
        sheet: s,
        corner: PointRef::New(pt(x0, y0)),
        width_mm: w,
        height_mm: h,
    })
    .unwrap()
}

fn extract_cmd(s: SheetId, at: PointMm, detached: bool) -> Command {
    Command::ExtractPiece {
        sheet: s,
        source: PieceSource::Click { at },
        name: "Devant".into(),
        detached,
    }
}

fn extract(p: &mut Project, s: SheetId, at: PointMm) -> Result<PieceId, CoreError> {
    p.apply(extract_cmd(s, at, false)).map(|o| o.pieces[0])
}

fn near(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

fn total_length(p: &Project, s: SheetId) -> f64 {
    let sh = p.sheet(s).unwrap();
    sh.entities
        .iter()
        .map(|e| sh.path_length_mm(e.id).unwrap())
        .sum()
}

/// Deux rectangles qui se croisent sans aucun point commun.
fn crossing() -> (Project, SheetId) {
    let (mut p, s) = setup();
    rect(&mut p, s, 0.0, 0.0, 100.0, 100.0);
    rect(&mut p, s, 50.0, 50.0, 100.0, 100.0);
    (p, s)
}

// ---------------------------------------------------------------- CA 1

#[test]
fn ca1_region_d_un_carre_sans_coupe() {
    let (mut p, s) = setup();
    rect(&mut p, s, 0.0, 0.0, 100.0, 100.0);
    let id = extract(&mut p, s, pt(40.0, 40.0)).unwrap();
    let piece = p.piece(id).unwrap();
    assert_eq!(piece.outline.len(), 4);
    assert_eq!(piece.link, PieceLink::Linked { sheet: s });
    assert_eq!(p.sheet(s).unwrap().entities.len(), 4, "rien n'est coupé");
    let c = p.piece_contour(id).unwrap();
    assert!(near(c.area_mm2, 10_000.0, 1e-9) && near(c.perimeter_mm, 400.0, 1e-9));
    assert!(c.closed && !c.self_intersecting);
}

#[test]
fn ca1_rectangles_qui_se_croisent_sans_point_commun() {
    let (mut p, s) = crossing();
    let before_len = total_length(&p, s);
    // Zone commune : 50 × 50. Les quatre côtés concernés sont coupés (8 -> 12 tracés).
    let common = extract(&mut p, s, pt(75.0, 75.0)).unwrap();
    let c = p.piece_contour(common).unwrap();
    assert!(near(c.area_mm2, 2_500.0, 1e-9) && near(c.perimeter_mm, 200.0, 1e-9));
    assert!(c.closed, "tronçons réels raccordés par des points communs");
    assert_eq!(p.sheet(s).unwrap().entities.len(), 12);
    assert!(
        near(total_length(&p, s), before_len, 1e-9),
        "rien n'est perdu"
    );
    // Zone non commune : la plus petite région qui contient le clic (en L, 7 500).
    let l = extract(&mut p, s, pt(20.0, 20.0)).unwrap();
    let c = p.piece_contour(l).unwrap();
    assert!(near(c.area_mm2, 7_500.0, 1e-9) && near(c.perimeter_mm, 400.0, 1e-9));
    assert_eq!(p.piece(l).unwrap().outline.len(), 6);
    assert_eq!(
        p.sheet(s).unwrap().entities.len(),
        12,
        "les tronçons déjà coupés sont réutilisés"
    );
    // La pièce en L partage des tracés avec la zone commune : les deux restent valides.
    p.validate().unwrap();
}

#[test]
fn ca1_cercle_coupe_par_une_corde_devient_des_arcs() {
    let (mut p, s) = setup();
    p.apply(Command::AddCircle {
        sheet: s,
        center: PointRef::New(pt(0.0, 0.0)),
        radius_mm: 10.0,
    })
    .unwrap();
    p.apply(Command::AddSegment {
        sheet: s,
        from: PointRef::New(pt(-8.0, 6.0)),
        to: PointRef::New(pt(8.0, 6.0)),
    })
    .unwrap();
    let id = extract(&mut p, s, pt(0.0, 0.0)).unwrap();
    let alpha = (0.6_f64).acos();
    let small = 100.0 * (alpha - alpha.sin() * alpha.cos());
    let c = p.piece_contour(id).unwrap();
    assert!(near(c.area_mm2, PI * 100.0 - small, 1e-9), "{}", c.area_mm2);
    assert!(near(c.perimeter_mm, 16.0 + 20.0 * PI - 20.0 * alpha, 1e-9));
    assert!(c.closed);
    let sh = p.sheet(s).unwrap();
    // Plus de cercle : deux arcs à sa place, plus la corde.
    assert_eq!(sh.entities.len(), 3);
    assert_eq!(
        sh.entities
            .iter()
            .filter(|e| matches!(e.kind, EntityKind::Arc { .. }))
            .count(),
        2
    );
    // L'autre côté de la corde est une région distincte.
    let top = extract(&mut p, s, pt(0.0, 8.0)).unwrap();
    assert!(near(p.piece_contour(top).unwrap().area_mm2, small, 1e-9));
    assert_eq!(p.sheet(s).unwrap().entities.len(), 3);
}

#[test]
fn ca1_cercle_entier_et_courbe_de_bezier() {
    let (mut p, s) = setup();
    p.apply(Command::AddCircle {
        sheet: s,
        center: PointRef::New(pt(5.0, 5.0)),
        radius_mm: 20.0,
    })
    .unwrap();
    let id = extract(&mut p, s, pt(5.0, 5.0)).unwrap();
    assert_eq!(p.piece(id).unwrap().outline.len(), 1);
    assert!(near(
        p.piece_contour(id).unwrap().area_mm2,
        PI * 400.0,
        1e-9
    ));
    assert_eq!(p.sheet(s).unwrap().entities.len(), 1);

    // Arche de Bézier fermée par sa corde : aire 6 000, puis coupée par une verticale.
    let (mut p, s) = setup();
    let a = p
        .apply(Command::AddCurve {
            sheet: s,
            start: PointRef::New(pt(0.0, 0.0)),
            c1: pt(0.0, 100.0),
            c2: pt(100.0, 100.0),
            end: PointRef::New(pt(100.0, 0.0)),
        })
        .unwrap();
    p.apply(Command::AddSegment {
        sheet: s,
        from: PointRef::Existing(a.points[1]),
        to: PointRef::Existing(a.points[0]),
    })
    .unwrap();
    let whole = extract(&mut p, s, pt(50.0, 30.0)).unwrap();
    assert!(near(
        p.piece_contour(whole).unwrap().area_mm2,
        6_000.0,
        1e-8
    ));
    p.apply(Command::AddSegment {
        sheet: s,
        from: PointRef::New(pt(50.0, -10.0)),
        to: PointRef::New(pt(50.0, 120.0)),
    })
    .unwrap();
    let left = extract(&mut p, s, pt(20.0, 20.0)).unwrap();
    let c = p.piece_contour(left).unwrap();
    assert!(near(c.area_mm2, 3_000.0, 1e-6), "{}", c.area_mm2);
    assert!(
        c.closed,
        "la Bézier coupée reste raccordée aux autres tronçons"
    );
}

#[test]
fn ca1_courbe_lisse_entiere_ou_coupee_a_un_point_de_passage() {
    let (mut p, s) = setup();
    let sp = p
        .apply(Command::AddSpline {
            sheet: s,
            points: vec![
                PointRef::New(pt(0.0, 0.0)),
                PointRef::New(pt(50.0, 60.0)),
                PointRef::New(pt(100.0, 0.0)),
            ],
        })
        .unwrap();
    p.apply(Command::AddSegment {
        sheet: s,
        from: PointRef::Existing(sp.points[2]),
        to: PointRef::Existing(sp.points[0]),
    })
    .unwrap();
    let id = extract(&mut p, s, pt(50.0, 20.0)).unwrap();
    let sh = p.sheet(s).unwrap();
    let mut poly = sh.spline_geometry(sp.entities[0]).unwrap().sample(4000);
    poly.push(pt(0.0, 0.0));
    let dense = pf_geometry::polygon_signed_area_mm2(&poly).abs();
    assert!(near(p.piece_contour(id).unwrap().area_mm2, dense, 1e-3));
    // Coupée exactement au point de passage (50 ; 60) par une verticale : acceptée.
    p.apply(Command::AddSegment {
        sheet: s,
        from: PointRef::New(pt(50.0, -5.0)),
        to: PointRef::New(pt(50.0, 80.0)),
    })
    .unwrap();
    let half = extract(&mut p, s, pt(20.0, 10.0)).unwrap();
    assert!(p.piece_contour(half).unwrap().closed);

    // Verticale qui coupe la courbe entre deux points de passage : refus expliqué, rien ne change.
    let (mut p, s) = setup();
    let sp = p
        .apply(Command::AddSpline {
            sheet: s,
            points: vec![
                PointRef::New(pt(0.0, 0.0)),
                PointRef::New(pt(50.0, 60.0)),
                PointRef::New(pt(100.0, 0.0)),
            ],
        })
        .unwrap();
    p.apply(Command::AddSegment {
        sheet: s,
        from: PointRef::Existing(sp.points[2]),
        to: PointRef::Existing(sp.points[0]),
    })
    .unwrap();
    p.apply(Command::AddSegment {
        sheet: s,
        from: PointRef::New(pt(25.0, -5.0)),
        to: PointRef::New(pt(25.0, 80.0)),
    })
    .unwrap();
    let before = p.clone();
    let err = extract(&mut p, s, pt(10.0, 5.0)).unwrap_err();
    assert!(err.to_string().contains("point de passage"), "{err}");
    assert_eq!(p, before);
}

// ---------------------------------------------------------------- CA 5

#[test]
fn ca5_region_non_fermee_montre_l_ouverture() {
    let (mut p, s) = setup();
    let seg = |p: &mut Project, a: PointMm, b: PointMm| {
        p.apply(Command::AddSegment {
            sheet: s,
            from: PointRef::New(a),
            to: PointRef::New(b),
        })
        .unwrap();
    };
    seg(&mut p, pt(0.0, 0.0), pt(100.0, 0.0));
    seg(&mut p, pt(100.0, 0.0), pt(100.0, 100.0));
    seg(&mut p, pt(100.0, 100.0), pt(0.0, 100.0));
    seg(&mut p, pt(0.0, 100.0), pt(0.0, 50.25));
    seg(&mut p, pt(0.0, 49.75), pt(0.0, 0.0));
    let before = p.clone();
    let err = extract(&mut p, s, pt(50.0, 50.0)).unwrap_err();
    assert!(err.to_string().contains("ouverture de 0.500 mm"), "{err}");
    assert_eq!(p, before, "refus sans trace");
    // La même ouverture, en données : les deux extrémités à surligner.
    let RegionError::Open(gap) = p
        .sheet(s)
        .unwrap()
        .find_region_at(pt(50.0, 50.0))
        .unwrap_err()
    else {
        panic!("ouverture attendue");
    };
    assert!(near(gap.gap_mm, 0.5, 1e-9));
    let mut ys = [gap.from.y, gap.to.y];
    ys.sort_by(f64::total_cmp);
    assert!(near(ys[0], 49.75, 1e-9) && near(ys[1], 50.25, 1e-9));
    // Hors de toute région : message distinct, sans ouverture.
    let err = extract(&mut p, s, pt(500.0, 500.0)).unwrap_err();
    assert!(err.to_string().contains("aucune région fermée"), "{err}");
    assert_eq!(p, before);
}

// ---------------------------------------------------------------- CA 2

#[test]
fn ca2_piece_liee_suit_la_construction() {
    let (mut p, s) = setup();
    let r = rect(&mut p, s, 0.0, 0.0, 100.0, 100.0);
    let id = extract(&mut p, s, pt(50.0, 50.0)).unwrap();
    assert!(near(p.piece_contour(id).unwrap().area_mm2, 10_000.0, 1e-9));
    // On tire le coin (100 ; 100) vers (120 ; 100) : la pièce suit, trapèze de 11 000.
    p.apply(Command::MovePoint {
        sheet: s,
        point: r.points[2],
        to: pt(120.0, 100.0),
    })
    .unwrap();
    let c = p.piece_contour(id).unwrap();
    assert!(near(c.area_mm2, 11_000.0, 1e-9), "{}", c.area_mm2);
    assert!(c.closed);
    p.validate().unwrap();
    // Supprimer un tracé du contour : la pièce survit, contour vidé et signalé (décision 36).
    let out = p
        .apply(Command::Delete {
            sheet: s,
            points: vec![],
            entities: vec![r.entities[0]],
        })
        .unwrap();
    assert!(
        out.notes.iter().any(|n| n.contains("contour")),
        "{:?}",
        out.notes
    );
    assert!(p.piece(id).unwrap().outline.is_empty());
    let err = p.piece_contour(id).unwrap_err();
    assert!(err.to_string().contains("pas de contour"), "{err}");
}

// ---------------------------------------------------------------- CA 3

#[test]
fn ca3_piece_detachee_copie_dans_une_feuille_dediee() {
    let (mut p, s) = crossing();
    let source_before = p.sheet(s).unwrap().clone();
    let out = p.apply(extract_cmd(s, pt(75.0, 75.0), true)).unwrap();
    let id = out.pieces[0];
    let new_sheet = out.sheet.expect("feuille dédiée");
    assert_ne!(new_sheet, s);
    assert_eq!(p.sheets.len(), 2);
    assert_eq!(p.sheet(new_sheet).unwrap().name, "Devant");
    assert_eq!(
        p.sheet(s).unwrap(),
        &source_before,
        "la construction n'est pas touchée"
    );
    let piece = p.piece(id).unwrap();
    assert_eq!(piece.link, PieceLink::Detached);
    assert!(piece
        .outline
        .iter()
        .all(|e| p.sheet(new_sheet).unwrap().entity(e.entity).is_ok()));
    let c = p.piece_contour(id).unwrap();
    assert!(near(c.area_mm2, 2_500.0, 1e-9) && c.closed && !c.clockwise);
    // Une copie ne suit pas : tirer un coin de la construction ne change pas la pièce.
    let corner = p.sheet(s).unwrap().points[2].id;
    p.apply(Command::MovePoint {
        sheet: s,
        point: corner,
        to: pt(300.0, 300.0),
    })
    .unwrap();
    assert!(near(p.piece_contour(id).unwrap().area_mm2, 2_500.0, 1e-9));
    // La feuille dédiée ne se supprime pas tant que la pièce détachée existe (refus sans trace).
    let before = p.clone();
    let err = p
        .apply(Command::DeleteSheet { sheet: new_sheet })
        .unwrap_err();
    assert!(matches!(err, CoreError::DeadReference(_)), "{err}");
    assert_eq!(p, before);
    // Un nom de feuille déjà pris reçoit un suffixe.
    let out = p
        .apply(extract_cmd(new_sheet, pt(75.0, 75.0), true))
        .unwrap();
    assert_eq!(p.sheet(out.sheet.unwrap()).unwrap().name, "Devant (2)");
    // Cercle coupé : copie en arcs.
    let (mut q, s2) = setup();
    q.apply(Command::AddCircle {
        sheet: s2,
        center: PointRef::New(pt(0.0, 0.0)),
        radius_mm: 10.0,
    })
    .unwrap();
    q.apply(Command::AddSegment {
        sheet: s2,
        from: PointRef::New(pt(-8.0, 6.0)),
        to: PointRef::New(pt(8.0, 6.0)),
    })
    .unwrap();
    let o = q.apply(extract_cmd(s2, pt(0.0, 8.0), true)).unwrap();
    let alpha = (0.6_f64).acos();
    let small = 100.0 * (alpha - alpha.sin() * alpha.cos());
    assert!(near(
        q.piece_contour(o.pieces[0]).unwrap().area_mm2,
        small,
        1e-9
    ));
    assert_eq!(
        q.sheet(s2).unwrap().entities.len(),
        2,
        "la source garde son cercle entier"
    );
}

// ---------------------------------------------------------------- CA 4

#[test]
fn ca4_contour_oriente_anti_horaire_aire_et_perimetre() {
    // Carré tracé dans le sens horaire : le contour de la pièce est tout de même anti-horaire.
    let (mut p, s) = setup();
    let seg = |p: &mut Project, a: PointMm, b: PointMm| {
        p.apply(Command::AddSegment {
            sheet: s,
            from: PointRef::New(a),
            to: PointRef::New(b),
        })
        .unwrap()
        .entities[0]
    };
    let ids = [
        seg(&mut p, pt(0.0, 0.0), pt(0.0, 80.0)),
        seg(&mut p, pt(0.0, 80.0), pt(60.0, 80.0)),
        seg(&mut p, pt(60.0, 80.0), pt(60.0, 0.0)),
        seg(&mut p, pt(60.0, 0.0), pt(0.0, 0.0)),
    ];
    let id = extract(&mut p, s, pt(10.0, 10.0)).unwrap();
    let c = p.piece_contour(id).unwrap();
    assert!(!c.clockwise);
    assert!(near(c.area_mm2, 4_800.0, 1e-9) && near(c.perimeter_mm, 280.0, 1e-9));
    assert!(p.piece(id).unwrap().outline.iter().all(|e| e.reversed));
    // Même chose depuis les tracés choisis (dans le désordre), et le même contour en miroir.
    let from_paths = p
        .apply(Command::ExtractPiece {
            sheet: s,
            source: PieceSource::Paths {
                entities: vec![ids[2], ids[0], ids[3], ids[1]],
            },
            name: "Choisie".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    let c2 = p.piece_contour(from_paths).unwrap();
    assert!(!c2.clockwise && near(c2.area_mm2, 4_800.0, 1e-9));
    assert_eq!(
        p.sheet(s).unwrap().entities.len(),
        4,
        "aucune coupe depuis les tracés"
    );
    // Les tracés choisis ne forment pas un contour fermé : refus expliqué.
    let before = p.clone();
    let err = p
        .apply(Command::ExtractPiece {
            sheet: s,
            source: PieceSource::Paths {
                entities: vec![ids[0], ids[1]],
            },
            name: "Ouverte".into(),
            detached: false,
        })
        .unwrap_err();
    assert!(err.to_string().contains("contour ouvert"), "{err}");
    assert_eq!(p, before);
}

#[test]
fn ca4_valeurs_observees_non_stockees() {
    let (mut p, s) = crossing();
    extract(&mut p, s, pt(75.0, 75.0)).unwrap();
    let json = persist::to_json(&p).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["schema_version"], 6);
    let piece = v["pieces"][0].as_object().unwrap();
    let allowed = [
        "id",
        "name",
        "quantity",
        "material",
        "mirrored_pair",
        "outline",
        "grainline",
        "link",
    ];
    for k in piece.keys() {
        assert!(allowed.contains(&k.as_str()), "clé inattendue : {k}");
    }
    assert!(!json.contains("area") && !json.contains("perimeter"));
}

// ---------------------------------------------------------------- refus, verrou, calques

#[test]
fn refus_sans_trace() {
    let (mut p, s) = crossing();
    let before = p.clone();
    let cases = [
        // Nom vide, clic non fini, hors de l'espace de travail, feuille inconnue.
        Command::ExtractPiece {
            sheet: s,
            source: PieceSource::Click { at: pt(75.0, 75.0) },
            name: "   ".into(),
            detached: false,
        },
        extract_cmd(s, pt(f64::NAN, 1.0), false),
        extract_cmd(s, pt(1e300, 0.0), false),
        extract_cmd(SheetId(9999), pt(75.0, 75.0), false),
        extract_cmd(s, pt(-30.0, -30.0), false),
        extract_cmd(s, pt(-30.0, -30.0), true),
        Command::ExtractPiece {
            sheet: s,
            source: PieceSource::Paths { entities: vec![] },
            name: "x".into(),
            detached: false,
        },
        Command::ExtractPiece {
            sheet: s,
            source: PieceSource::Paths {
                entities: vec![EntityId(9999)],
            },
            name: "x".into(),
            detached: true,
        },
    ];
    for c in cases {
        assert!(p.apply(c.clone()).is_err(), "{c:?}");
        assert_eq!(p, before);
    }
}

#[test]
fn verrou_un_calque_verrouille_ne_se_coupe_pas() {
    let (mut p, s) = setup();
    let a = rect(&mut p, s, 0.0, 0.0, 100.0, 100.0);
    rect(&mut p, s, 50.0, 50.0, 100.0, 100.0);
    let layer = p
        .apply(Command::AddLayer {
            sheet: s,
            name: "Base".into(),
        })
        .unwrap()
        .layers[0];
    p.apply(Command::SetObjectsLayer {
        sheet: s,
        points: a.points.clone(),
        entities: a.entities.clone(),
        layer: Some(layer),
    })
    .unwrap();
    p.apply(Command::SetLayerLocked {
        sheet: s,
        layer,
        locked: true,
    })
    .unwrap();
    let before = p.clone();
    let err = extract(&mut p, s, pt(75.0, 75.0)).unwrap_err();
    assert!(matches!(err, CoreError::LayerLocked(_)), "{err}");
    assert_eq!(p, before);
    // Une pièce détachée ne modifie rien : elle reste possible.
    let out = p.apply(extract_cmd(s, pt(75.0, 75.0), true)).unwrap();
    assert!(near(
        p.piece_contour(out.pieces[0]).unwrap().area_mm2,
        2_500.0,
        1e-9
    ));
}

#[test]
fn calque_masque_ne_delimite_aucune_region() {
    let (mut p, s) = setup();
    rect(&mut p, s, 0.0, 0.0, 100.0, 100.0);
    let b = rect(&mut p, s, 50.0, 50.0, 100.0, 100.0);
    let layer = p
        .apply(Command::AddLayer {
            sheet: s,
            name: "Aide".into(),
        })
        .unwrap()
        .layers[0];
    p.apply(Command::SetObjectsLayer {
        sheet: s,
        points: b.points.clone(),
        entities: b.entities.clone(),
        layer: Some(layer),
    })
    .unwrap();
    p.apply(Command::SetLayerVisible {
        sheet: s,
        layer,
        visible: false,
    })
    .unwrap();
    // Le second rectangle est masqué : la zone « commune » n'existe plus, c'est tout le premier.
    let id = extract(&mut p, s, pt(75.0, 75.0)).unwrap();
    assert!(near(p.piece_contour(id).unwrap().area_mm2, 10_000.0, 1e-9));
    assert_eq!(p.sheet(s).unwrap().entities.len(), 8, "rien n'est coupé");
}

#[test]
fn cercle_a_decouper_avec_crans_ou_deja_contour_est_refuse() {
    let (mut p, s) = setup();
    let c = p
        .apply(Command::AddCircle {
            sheet: s,
            center: PointRef::New(pt(0.0, 0.0)),
            radius_mm: 10.0,
        })
        .unwrap();
    p.apply(Command::AddSegment {
        sheet: s,
        from: PointRef::New(pt(-8.0, 6.0)),
        to: PointRef::New(pt(8.0, 6.0)),
    })
    .unwrap();
    p.apply(Command::AddNotch {
        sheet: s,
        entity: c.entities[0],
        distance_mm: 5.0,
        side: Side::Left,
        length_mm: 3.0,
        style: NotchStyle::Single,
    })
    .unwrap();
    let before = p.clone();
    let err = extract(&mut p, s, pt(0.0, 0.0)).unwrap_err();
    assert!(err.to_string().contains("crans"), "{err}");
    assert_eq!(p, before);
}

// ---------------------------------------------------------------- une étape annulable

#[test]
fn une_seule_etape_annulable() {
    let (p, s) = crossing();
    let before = p.clone();
    let mut doc = Document::new(p);
    doc.execute(extract_cmd(s, pt(75.0, 75.0), false)).unwrap();
    let after = doc.project().clone();
    assert_eq!(after.pieces.len(), 1);
    assert_eq!(after.sheet(s).unwrap().entities.len(), 12);
    assert!(doc.undo());
    assert_eq!(doc.project(), &before, "coupes et pièce annulées ensemble");
    assert!(!doc.can_undo());
    assert!(doc.redo());
    assert_eq!(doc.project(), &after);
    // Détachée : la feuille dédiée disparaît aussi à l'annulation.
    assert!(doc.undo());
    doc.execute(extract_cmd(s, pt(75.0, 75.0), true)).unwrap();
    assert_eq!(doc.project().sheets.len(), 2);
    assert!(doc.undo());
    assert_eq!(doc.project(), &before);
}

#[test]
fn commande_serialisable() {
    for c in [
        extract_cmd(SheetId(1), pt(1.0, 2.0), false),
        Command::ExtractPiece {
            sheet: SheetId(1),
            source: PieceSource::Paths {
                entities: vec![EntityId(3), EntityId(4)],
            },
            name: "x".into(),
            detached: true,
        },
    ] {
        let text = serde_json::to_string(&c).unwrap();
        assert_eq!(serde_json::from_str::<Command>(&text).unwrap(), c);
    }
    // `detached` est facultatif.
    let c: Command = serde_json::from_str(
        r#"{"type":"extract_piece","sheet":1,"source":{"type":"click","at":{"x":1.0,"y":2.0}},"name":"n"}"#,
    )
    .unwrap();
    assert!(matches!(
        c,
        Command::ExtractPiece {
            detached: false,
            ..
        }
    ));
}

// ---------------------------------------------------------------- fichier

#[test]
fn fichier_v3_sans_pieces_extraites_identique_a_l_octet() {
    let dir = env!("CARGO_MANIFEST_DIR");
    let path = std::path::Path::new(dir).join("../../examples/piece-crans-droit-fil.pforge");
    let text = std::fs::read_to_string(path).unwrap();
    let project = persist::from_json(&text).unwrap();
    assert_eq!(persist::to_json(&project).unwrap(), text);

    // Un refus d'extraction ne change pas l'enregistrement.
    let (mut p, s) = crossing();
    let json = persist::to_json(&p).unwrap();
    let _ = extract(&mut p, s, pt(-30.0, -30.0)).unwrap_err();
    assert_eq!(persist::to_json(&p).unwrap(), json);
}

#[test]
fn fichier_avec_pieces_extraites_aller_retour_exact() {
    let (mut p, s) = crossing();
    extract(&mut p, s, pt(75.0, 75.0)).unwrap();
    extract(&mut p, s, pt(20.0, 20.0)).unwrap();
    p.apply(extract_cmd(s, pt(120.0, 120.0), true)).unwrap();
    let json = persist::to_json(&p).unwrap();
    let back = persist::from_json(&json).unwrap();
    assert_eq!(back, p);
    assert_eq!(persist::to_json(&back).unwrap(), json);
}

// ---------------------------------------------------------------- copies

#[test]
fn copie_de_feuille_garde_les_pieces_liees() {
    let (mut p, s) = crossing();
    let id = extract(&mut p, s, pt(75.0, 75.0)).unwrap();
    let detached = p
        .apply(extract_cmd(s, pt(20.0, 20.0), true))
        .unwrap()
        .pieces[0];
    let out = p.apply(Command::DuplicateSheet { sheet: s }).unwrap();
    let copy = out.sheet.unwrap();
    let copies: Vec<&Piece> = p.pieces_of(copy).collect();
    assert_eq!(
        copies.len(),
        1,
        "la pièce détachée n'est pas copiée avec la feuille"
    );
    assert_ne!(copies[0].id, id);
    assert!(copies[0]
        .outline
        .iter()
        .all(|e| p.sheet(copy).unwrap().entity(e.entity).is_ok()));
    let (a, b) = (
        p.piece_contour(id).unwrap(),
        p.piece_contour(copies[0].id).unwrap(),
    );
    assert!(near(a.area_mm2, b.area_mm2, 1e-9) && b.closed);
    assert!(p.piece(detached).is_ok());
    p.validate().unwrap();
}

#[test]
fn presse_papiers_ne_duplique_pas_la_piece() {
    let (mut p, s) = crossing();
    let id = extract(&mut p, s, pt(75.0, 75.0)).unwrap();
    let outline: Vec<EntityId> = p
        .piece(id)
        .unwrap()
        .outline
        .iter()
        .map(|e| e.entity)
        .collect();
    let fragment = p.copy_fragment(s, &[], &outline, &[], &[]).unwrap();
    p.apply(Command::PasteFragment {
        sheet: s,
        fragment: Box::new(fragment),
        layer: None,
    })
    .unwrap();
    // Les tracés sont dupliqués, la pièce non (les pièces ne passent pas par le presse-papiers).
    assert_eq!(p.pieces.len(), 1);
    assert_eq!(p.sheet(s).unwrap().entities.len(), 16);
    assert!(near(p.piece_contour(id).unwrap().area_mm2, 2_500.0, 1e-9));
    p.validate().unwrap();
}

#[test]
fn extraire_deux_fois_la_meme_region_ne_coupe_plus() {
    let (mut p, s) = crossing();
    let first = extract(&mut p, s, pt(75.0, 75.0)).unwrap();
    let count = p.sheet(s).unwrap().entities.len();
    let second = extract(&mut p, s, pt(60.0, 90.0)).unwrap();
    assert_eq!(p.sheet(s).unwrap().entities.len(), count);
    assert_eq!(
        p.piece(first).unwrap().outline.len(),
        p.piece(second).unwrap().outline.len()
    );
    assert!(near(
        p.piece_contour(first).unwrap().area_mm2,
        p.piece_contour(second).unwrap().area_mm2,
        1e-9
    ));
}
