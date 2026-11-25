//! US-06.10 — zones de parage, de collage et de découpe interne : `AddPieceZone`,
//! `SetPieceZone`, `RemovePieceZone`. Valeurs attendues **calculées à la main** (jamais relues du
//! code testé) ; la campagne adverse est dans `adverse_zones.rs`.

use pf_core::persist;
use pf_core::*;
use pf_geometry::PointMm;

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

fn bytes(p: &Project) -> String {
    persist::to_json(p).unwrap()
}

fn close(a: PointMm, x: f64, y: f64, tol: f64) -> bool {
    (a.x - x).abs() <= tol && (a.y - y).abs() <= tol
}

fn refused(p: &mut Project, c: Command) -> String {
    let (before, text) = (p.clone(), bytes(p));
    let err = p.apply(c).expect_err("la commande aurait dû être refusée");
    assert_eq!(*p, before, "refus sans trace (valeur)");
    assert_eq!(text, bytes(p), "refus sans trace (octets)");
    err.to_string()
}

struct Scene {
    p: Project,
    s: SheetId,
    piece: PieceId,
}

/// Rectangle 100 × 70 mm, pièce liée anti-horaire.
fn rect() -> Scene {
    let mut p = Project::new("Zones");
    let s = p.sheets[0].id;
    p.apply(Command::AddRectangle {
        sheet: s,
        corner: PointRef::New(pt(0.0, 0.0)),
        width_mm: 100.0,
        height_mm: 70.0,
    })
    .unwrap();
    let piece = p
        .apply(Command::ExtractPiece {
            sheet: s,
            source: PieceSource::Click { at: pt(50.0, 35.0) },
            name: "Fond".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    Scene { p, s, piece }
}

fn seg_between(p: &Project, s: SheetId, a: PointMm, b: PointMm) -> EntityId {
    let sheet = p.sheet(s).unwrap();
    sheet
        .entities
        .iter()
        .find(|e| {
            sheet.segment_ends(e.id).is_ok_and(|(x, y)| {
                (close(x, a.x, a.y, 1e-9) && close(y, b.x, b.y, 1e-9))
                    || (close(x, b.x, b.y, 1e-9) && close(y, a.x, a.y, 1e-9))
            })
        })
        .unwrap_or_else(|| panic!("segment {a:?} {b:?}"))
        .id
}

fn bottom(sc: &Scene) -> EntityId {
    seg_between(&sc.p, sc.s, pt(0.0, 0.0), pt(100.0, 0.0))
}

fn right(sc: &Scene) -> EntityId {
    seg_between(&sc.p, sc.s, pt(100.0, 0.0), pt(100.0, 70.0))
}

fn top(sc: &Scene) -> EntityId {
    seg_between(&sc.p, sc.s, pt(0.0, 70.0), pt(100.0, 70.0))
}

fn point_at(p: &Project, s: SheetId, x: f64, y: f64) -> PointId {
    let sheet = p.sheet(s).unwrap();
    sheet
        .points
        .iter()
        .find(|q| sheet.position(q.id).is_ok_and(|a| close(a, x, y, 1e-9)))
        .expect("point")
        .id
}

fn band(kind: ZoneKind, edges: &[EntityId], width: f64) -> ZoneSpec {
    ZoneSpec {
        kind,
        shape: ZoneShape::Band {
            edges: edges.to_vec(),
            width_mm: width,
        },
    }
}

fn poly(kind: ZoneKind, pts: &[(f64, f64)]) -> ZoneSpec {
    ZoneSpec {
        kind,
        shape: ZoneShape::Polygon {
            points: pts.iter().map(|&(x, y)| pt(x, y)).collect(),
        },
    }
}

fn add(p: &mut Project, piece: PieceId, zone: ZoneSpec) -> ZoneId {
    p.apply(Command::AddPieceZone { piece, zone })
        .unwrap()
        .zones[0]
}

/// Aire par la formule des lacets, écrite ici (indépendante du code testé).
fn shoelace(v: &[PointMm]) -> f64 {
    let n = v.len();
    (0..n)
        .map(|i| v[i].x * v[(i + 1) % n].y - v[(i + 1) % n].x * v[i].y)
        .sum::<f64>()
        .abs()
        / 2.0
}

fn has(v: &[PointMm], x: f64, y: f64) -> bool {
    v.iter().any(|p| close(*p, x, y, 1e-9))
}

// ----- CA1 : la largeur lue sur le plan est la valeur saisie ----------------------------------

#[test]
fn ca1_bande_sur_le_bas_a_largeur_saisie() {
    let mut sc = rect();
    let e = bottom(&sc);
    let id = add(&mut sc.p, sc.piece, band(ZoneKind::Skive, &[e], 5.0));
    let z = sc.p.zone_layout(sc.piece, id).unwrap();
    // Rectangle (0 ; 0) (100 ; 0) (100 ; 5) (0 ; 5) : 4 sommets, aire 100 × 5.
    assert_eq!(z.polygon.len(), 4);
    for (x, y) in [(0.0, 0.0), (100.0, 0.0), (100.0, 5.0), (0.0, 5.0)] {
        assert!(has(&z.polygon, x, y), "{:?}", z.polygon);
    }
    assert!((z.area_mm2 - 500.0).abs() < 1e-9);
    assert_eq!(z.width_mm, Some(5.0));
    assert_eq!(z.kind, ZoneKind::Skive);
}

#[test]
fn ca1_largeur_a_deux_decimales_lue_a_0_01_pres() {
    let mut sc = rect();
    let e = bottom(&sc);
    let id = add(&mut sc.p, sc.piece, band(ZoneKind::Glue, &[e], 12.34));
    let z = sc.p.zone_layout(sc.piece, id).unwrap();
    // Largeur lue = distance des sommets du décalage à la droite du bord (y = 0).
    let widths: Vec<f64> = z
        .polygon
        .iter()
        .map(|p| p.y)
        .filter(|y| *y > 1e-9)
        .collect();
    assert_eq!(widths.len(), 2);
    for w in widths {
        assert!((w - 12.34).abs() < 0.005, "{w}");
    }
    assert!((z.area_mm2 - 100.0 * 12.34).abs() < 1e-6);
}

#[test]
fn ca1_bande_en_l_sur_deux_bords_consecutifs() {
    let mut sc = rect();
    let (b, r) = (bottom(&sc), right(&sc));
    let id = add(&mut sc.p, sc.piece, band(ZoneKind::InnerCut, &[b, r], 5.0));
    let z = sc.p.zone_layout(sc.piece, id).unwrap();
    // L : [0 ; 100] × [0 ; 5] ∪ [95 ; 100] × [0 ; 70] = 500 + 350 − 25 = 825 mm².
    assert!((z.area_mm2 - 825.0).abs() < 1e-9, "{}", z.area_mm2);
    for (x, y) in [
        (0.0, 0.0),
        (100.0, 0.0),
        (100.0, 70.0),
        (95.0, 70.0),
        (95.0, 5.0),
        (0.0, 5.0),
    ] {
        assert!(has(&z.polygon, x, y), "({x}, {y}) dans {:?}", z.polygon);
    }
}

/// Pentagone (0 ; 0) (100 ; 0) (110 ; 35) (100 ; 70) (0 ; 70) : le bord (100 ; 0)-(110 ; 35) est
/// incliné et ses deux angles sont obtus (106° et 148°).
fn pentagon() -> Scene {
    let mut p = Project::new("Zones");
    let s = p.sheets[0].id;
    p.apply(Command::AddPolygon {
        sheet: s,
        vertices: [
            (0.0, 0.0),
            (100.0, 0.0),
            (110.0, 35.0),
            (100.0, 70.0),
            (0.0, 70.0),
        ]
        .map(|(x, y)| PointRef::New(pt(x, y)))
        .to_vec(),
        closed: true,
        role: PathRole::default(),
        layer: None,
    })
    .unwrap();
    let piece = p
        .apply(Command::ExtractPiece {
            sheet: s,
            source: PieceSource::Click { at: pt(50.0, 35.0) },
            name: "Penta".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    Scene { p, s, piece }
}

#[test]
fn ca1_bande_sur_un_bord_incline() {
    // Bord (100 ; 0)-(110 ; 35) de longueur √1325, normale intérieure (−35 ; 10) / √1325,
    // bande de 5 mm : aire 5 √1325, sommets décalés (100 − 175/L ; 50/L) et (110 − 175/L ; 35 + 50/L).
    let mut sc = pentagon();
    let e = seg_between(&sc.p, sc.s, pt(100.0, 0.0), pt(110.0, 35.0));
    let id = add(&mut sc.p, sc.piece, band(ZoneKind::Skive, &[e], 5.0));
    let z = sc.p.zone_layout(sc.piece, id).unwrap();
    let len = 1325f64.sqrt();
    assert!((z.area_mm2 - 5.0 * len).abs() < 1e-9);
    assert!(has(&z.polygon, 100.0 - 175.0 / len, 50.0 / len));
    assert!(has(&z.polygon, 110.0 - 175.0 / len, 35.0 + 50.0 / len));
    // Largeur lue : distance de chaque sommet décalé à la droite du bord, 5 mm à 0,01 près.
    for v in &z.polygon {
        let d = ((v.x - 100.0) * 35.0 - v.y * 10.0).abs() / len;
        assert!(d < 1e-9 || (d - 5.0).abs() < 0.005, "{d}");
    }
}

#[test]
fn ca3_un_coin_aigu_au_bout_de_la_bande_refuse() {
    // Rectangle 100 × 70 avec le coin (100 ; 0) amené à (100 ; 10) : le bord du bas devient
    // oblique et fait 84° avec le bord gauche : le bout de bande sortirait de la pièce.
    let mut sc = rect();
    let e = bottom(&sc);
    let corner = point_at(&sc.p, sc.s, 100.0, 0.0);
    sc.p.apply(Command::MovePoint {
        sheet: sc.s,
        point: corner,
        to: pt(100.0, 10.0),
    })
    .unwrap();
    let why = refused(
        &mut sc.p,
        Command::AddPieceZone {
            piece: sc.piece,
            zone: band(ZoneKind::Skive, &[e], 5.0),
        },
    );
    assert!(why.contains("sortirait"), "{why}");
}

// ----- CA2 : la zone suit le bord source, dans la même étape ---------------------------------

#[test]
fn ca2_deplacer_un_coin_recalcule_la_bande_dans_la_meme_etape() {
    let sc = pentagon();
    let e = seg_between(&sc.p, sc.s, pt(100.0, 0.0), pt(110.0, 35.0));
    let corner = point_at(&sc.p, sc.s, 110.0, 35.0);
    let mut doc = Document::new(sc.p.clone());
    let original = bytes(doc.project());
    let id = doc
        .execute(Command::AddPieceZone {
            piece: sc.piece,
            zone: band(ZoneKind::Skive, &[e], 5.0),
        })
        .unwrap()
        .zones[0];
    let after_add = bytes(doc.project());
    doc.execute(Command::MovePoint {
        sheet: sc.s,
        point: corner,
        to: pt(120.0, 35.0),
    })
    .unwrap();
    // Bord (100 ; 0)-(120 ; 35) : longueur √1625, aire 5 √1625, sommet décalé
    // (120 − 175/L ; 35 + 100/L).
    let z = doc.project().zone_layout(sc.piece, id).unwrap();
    let len = 1625f64.sqrt();
    assert!((z.area_mm2 - 5.0 * len).abs() < 1e-9);
    assert!(has(&z.polygon, 120.0 - 175.0 / len, 35.0 + 100.0 / len));
    // Une seule étape annulable pour le déplacement, puis une pour la création.
    assert!(doc.undo());
    assert_eq!(bytes(doc.project()), after_add);
    assert!(doc.undo());
    assert_eq!(bytes(doc.project()), original);
    assert!(!doc.can_undo());
}

#[test]
fn ca2_un_deplacement_qui_invalide_la_zone_refuse_la_commande_de_la_mere() {
    let mut sc = rect();
    let e = bottom(&sc);
    add(&mut sc.p, sc.piece, band(ZoneKind::Skive, &[e], 60.0));
    // Le coin (0 ; 70) descend à 50 : le décalage (y = 60) sort de la pièce.
    let corner = point_at(&sc.p, sc.s, 0.0, 70.0);
    let why = refused(
        &mut sc.p,
        Command::MovePoint {
            sheet: sc.s,
            point: corner,
            to: pt(0.0, 50.0),
        },
    );
    assert!(why.contains("zone"), "{why}");
    // Un déplacement qui garde la zone valide est accepté.
    sc.p.apply(Command::MovePoint {
        sheet: sc.s,
        point: corner,
        to: pt(0.0, 65.0),
    })
    .unwrap();
}

#[test]
fn ca2_bord_supprime_retire_la_bande_avec_remarque() {
    // Un tracé qui sort du contour retire la bande qui le portait, avec remarque (ou la
    // suppression est refusée : jamais une bande qui référence un tracé absent).
    let mut sc = rect();
    let b = bottom(&sc);
    add(&mut sc.p, sc.piece, band(ZoneKind::Glue, &[b], 5.0));
    match sc.p.apply(Command::Delete {
        sheet: sc.s,
        points: vec![],
        entities: vec![b],
    }) {
        Ok(o) => {
            assert!(o.notes.iter().any(|n| n.contains("zone")), "{:?}", o.notes);
            assert!(sc.p.piece(sc.piece).unwrap().zones.is_empty());
        }
        Err(_) => assert_eq!(sc.p.piece(sc.piece).unwrap().zones.len(), 1),
    }
}

// ----- CA3 : hors de la pièce et cas dégénérés = refus sans trace ----------------------------

#[test]
fn ca3_largeur_hors_bornes_ou_trop_grande_refusee_sans_trace() {
    let mut sc = rect();
    let (b, t) = (bottom(&sc), top(&sc));
    for w in [0.49, 0.0, -5.0, 100.01, f64::NAN, f64::INFINITY] {
        refused(
            &mut sc.p,
            Command::AddPieceZone {
                piece: sc.piece,
                zone: band(ZoneKind::Skive, &[b], w),
            },
        );
    }
    // 80 mm sur une pièce de 70 mm : le décalage sort de la pièce.
    let why = refused(
        &mut sc.p,
        Command::AddPieceZone {
            piece: sc.piece,
            zone: band(ZoneKind::Skive, &[b], 80.0),
        },
    );
    assert!(why.contains("sortirait"), "{why}");
    // 69,9 mm : accepté (décalage à y = 69,9).
    add(&mut sc.p, sc.piece, band(ZoneKind::Skive, &[b], 69.9));
    // Bords qui ne se suivent pas, aucun bord, bord inconnu.
    refused(
        &mut sc.p,
        Command::AddPieceZone {
            piece: sc.piece,
            zone: band(ZoneKind::Glue, &[b, t], 5.0),
        },
    );
    refused(
        &mut sc.p,
        Command::AddPieceZone {
            piece: sc.piece,
            zone: band(ZoneKind::Glue, &[], 5.0),
        },
    );
    refused(
        &mut sc.p,
        Command::AddPieceZone {
            piece: sc.piece,
            zone: band(ZoneKind::Glue, &[EntityId(999_999)], 5.0),
        },
    );
    refused(
        &mut sc.p,
        Command::AddPieceZone {
            piece: PieceId(999_999),
            zone: band(ZoneKind::Glue, &[b], 5.0),
        },
    );
}

#[test]
fn ca3_contour_entier_refuse_pour_une_bande() {
    let mut sc = rect();
    let edges: Vec<EntityId> =
        sc.p.piece(sc.piece)
            .unwrap()
            .outline
            .iter()
            .map(|e| e.entity)
            .collect();
    let why = refused(
        &mut sc.p,
        Command::AddPieceZone {
            piece: sc.piece,
            zone: band(ZoneKind::Skive, &edges, 5.0),
        },
    );
    assert!(why.contains("contour entier"), "{why}");
}

#[test]
fn ca3_polygone_libre_valide_et_refus() {
    let mut sc = rect();
    // Triangle (10 ; 10) (60 ; 10) (10 ; 40) : aire ½ × 50 × 30 = 750.
    let id = add(
        &mut sc.p,
        sc.piece,
        poly(ZoneKind::Glue, &[(10.0, 10.0), (60.0, 10.0), (10.0, 40.0)]),
    );
    let z = sc.p.zone_layout(sc.piece, id).unwrap();
    assert!((z.area_mm2 - 750.0).abs() < 1e-9);
    assert!((shoelace(&z.polygon) - 750.0).abs() < 1e-9);
    assert_eq!(z.width_mm, None);
    let bad: [&[(f64, f64)]; 8] = [
        // Pincement : le sommet (35 ; 10) touche le côté (10 ; 10)-(60 ; 10) sans le croiser.
        &[
            (10.0, 10.0),
            (60.0, 10.0),
            (60.0, 50.0),
            (35.0, 10.0),
            (10.0, 50.0),
        ],
        &[(10.0, 10.0), (60.0, 10.0)],
        &[(10.0, 10.0), (110.0, 10.0), (10.0, 40.0)],
        &[(10.0, 10.0), (60.0, 40.0), (60.0, 10.0), (10.0, 40.0)],
        &[(10.0, 10.0), (20.0, 10.0), (30.0, 10.0)],
        &[(10.0, 10.0), (10.0, 10.0), (10.0, 10.0)],
        &[(10.0, 10.0), (60.0, 10.0), (60.0, 80.0), (10.0, 40.0)],
        &[(10.0, 10.0), (60.0, 10.0), (f64::NAN, 40.0)],
    ];
    for pts in bad {
        refused(
            &mut sc.p,
            Command::AddPieceZone {
                piece: sc.piece,
                zone: poly(ZoneKind::Skive, pts),
            },
        );
    }
}

// ----- Remplacer, retirer, plafonds ------------------------------------------------------------

#[test]
fn set_et_remove_une_etape_chacun() {
    let mut sc = rect();
    let b = bottom(&sc);
    let id = add(&mut sc.p, sc.piece, band(ZoneKind::Skive, &[b], 5.0));
    sc.p.apply(Command::SetPieceZone {
        piece: sc.piece,
        zone: id,
        spec: band(ZoneKind::Glue, &[b], 8.0),
    })
    .unwrap();
    let z = sc.p.zone_layout(sc.piece, id).unwrap();
    assert_eq!(z.kind, ZoneKind::Glue);
    assert!((z.area_mm2 - 800.0).abs() < 1e-9);
    // Remplacement invalide : refus sans trace.
    refused(
        &mut sc.p,
        Command::SetPieceZone {
            piece: sc.piece,
            zone: id,
            spec: band(ZoneKind::Glue, &[b], 90.0),
        },
    );
    refused(
        &mut sc.p,
        Command::SetPieceZone {
            piece: sc.piece,
            zone: ZoneId(999_999),
            spec: band(ZoneKind::Glue, &[b], 8.0),
        },
    );
    sc.p.apply(Command::RemovePieceZone {
        piece: sc.piece,
        zone: id,
    })
    .unwrap();
    assert!(sc.p.piece(sc.piece).unwrap().zones.is_empty());
    refused(
        &mut sc.p,
        Command::RemovePieceZone {
            piece: sc.piece,
            zone: id,
        },
    );
}

#[test]
fn au_plus_cinquante_zones_par_piece() {
    let mut sc = rect();
    for k in 0..ZONE_MAX_PER_PIECE {
        let x = 1.0 + k as f64;
        add(
            &mut sc.p,
            sc.piece,
            poly(
                ZoneKind::Glue,
                &[(x, 10.0), (x + 0.9, 10.0), (x + 0.9, 20.0), (x, 20.0)],
            ),
        );
    }
    refused(
        &mut sc.p,
        Command::AddPieceZone {
            piece: sc.piece,
            zone: poly(ZoneKind::Glue, &[(10.0, 30.0), (20.0, 30.0), (10.0, 40.0)]),
        },
    );
}

#[test]
fn apercu_en_lecture_seule_et_memes_refus() {
    let sc = rect();
    let b = bottom(&sc);
    let before = bytes(&sc.p);
    let z =
        sc.p.zone_preview(sc.piece, &band(ZoneKind::Skive, &[b], 5.0))
            .unwrap();
    assert!((z.area_mm2 - 500.0).abs() < 1e-9);
    assert!(sc
        .p
        .zone_preview(sc.piece, &band(ZoneKind::Skive, &[b], 80.0))
        .is_err());
    assert_eq!(bytes(&sc.p), before);
}

#[test]
fn copie_de_feuille_donne_de_nouveaux_identifiants_et_garde_la_surface() {
    let mut sc = rect();
    let b = bottom(&sc);
    add(&mut sc.p, sc.piece, band(ZoneKind::Skive, &[b], 5.0));
    let out = sc.p.apply(Command::DuplicateSheet { sheet: sc.s }).unwrap();
    let copy = out.pieces[0];
    let np = sc.p.piece(copy).unwrap();
    assert_eq!(np.zones.len(), 1);
    assert_ne!(np.zones[0].id, sc.p.piece(sc.piece).unwrap().zones[0].id);
    let z = sc.p.zone_layout(copy, np.zones[0].id).unwrap();
    assert!((z.area_mm2 - 500.0).abs() < 1e-9);
}

// ----- Fichier ---------------------------------------------------------------------------------

#[test]
fn fichier_aller_retour_omis_quand_vide_et_refuse_avant_la_v5() {
    let mut sc = rect();
    let plain = bytes(&sc.p);
    assert!(!plain.contains("\"zones\""), "omis quand vide");
    let b = bottom(&sc);
    let id = add(&mut sc.p, sc.piece, band(ZoneKind::InnerCut, &[b], 5.0));
    add(
        &mut sc.p,
        sc.piece,
        poly(ZoneKind::Glue, &[(10.0, 10.0), (60.0, 10.0), (10.0, 40.0)]),
    );
    let json = bytes(&sc.p);
    assert!(json.contains("\"zones\""));
    assert!(json.contains("\"inner_cut\""));
    let back = persist::from_json(&json).unwrap();
    assert_eq!(back, sc.p);
    assert_eq!(bytes(&back), json);
    assert_eq!(
        back.zone_layout(sc.piece, id).unwrap(),
        sc.p.zone_layout(sc.piece, id).unwrap()
    );
    // Clé inconnue refusée ; clé réservée à la v5 refusée dans un fichier v4.
    let bad = json.replacen("\"width_mm\"", "\"width_mm_x\"", 1);
    assert!(persist::from_json(&bad).is_err());
    let v4 = json.replacen("\"schema_version\": 6", "\"schema_version\": 4", 1);
    assert_ne!(v4, json);
    let err = persist::from_json(&v4).expect_err("clé réservée à la v5");
    assert!(err.to_string().contains("zones"), "{err}");
    // Un fichier dont la bande sortirait de la pièce est refusé à l'ouverture.
    let wide = json.replacen("\"width_mm\": 5.0", "\"width_mm\": 90.0", 1);
    assert_ne!(wide, json);
    assert!(persist::from_json(&wide).is_err());
}

#[path = "format_ouvert/validateur.rs"]
mod validateur;

#[test]
fn le_schema_json_valide_les_zones_et_refuse_une_cle_inconnue() {
    let mut sc = rect();
    let b = bottom(&sc);
    add(&mut sc.p, sc.piece, band(ZoneKind::Skive, &[b], 5.0));
    add(
        &mut sc.p,
        sc.piece,
        poly(ZoneKind::Glue, &[(10.0, 10.0), (60.0, 10.0), (10.0, 40.0)]),
    );
    let schema: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../docs/pforge-v6.schema.json"),
        )
        .unwrap(),
    )
    .unwrap();
    let mut doc: serde_json::Value = serde_json::from_str(&bytes(&sc.p)).unwrap();
    let errors = validateur::validate(&schema, &doc);
    assert!(errors.is_empty(), "{errors:?}");
    doc["pieces"][0]["zones"][0]["extra"] = serde_json::json!(1);
    assert!(!validateur::validate(&schema, &doc).is_empty());
    doc["pieces"][0]["zones"][0]
        .as_object_mut()
        .unwrap()
        .remove("extra");
    doc["pieces"][0]["zones"][0]["kind"] = serde_json::json!("peinture");
    assert!(!validateur::validate(&schema, &doc).is_empty());
    doc["pieces"][0]["zones"][0]["kind"] = serde_json::json!("skive");
    doc["pieces"][0]["zones"][0]["shape"]["width_mm"] = serde_json::json!(0.2);
    assert!(!validateur::validate(&schema, &doc).is_empty());
}
