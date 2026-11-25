//! US-06.9 — lignes de piqûre : `AddStitchLine`, `SetStitchLine`, `RemoveStitchLine`,
//! `MergeStitchLines`. Valeurs attendues **calculées à la main** (jamais relues du code testé) ;
//! la campagne adverse est dans `adverse_piqure.rs`.

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
    let mut p = Project::new("Piqure");
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

fn point_at(p: &Project, s: SheetId, x: f64, y: f64) -> PointId {
    let sheet = p.sheet(s).unwrap();
    sheet
        .points
        .iter()
        .find(|q| sheet.position(q.id).is_ok_and(|a| close(a, x, y, 1e-9)))
        .expect("point")
        .id
}

fn spec(edges: &[EntityId], pitch: f64, setback: f64) -> StitchLineSpec {
    StitchLineSpec {
        edges: edges.to_vec(),
        hole: StitchHole::Round { diameter_mm: 1.2 },
        pitch_mm: pitch,
        setback_mm: setback,
        count: None,
        continues: None,
    }
}

fn add(p: &mut Project, piece: PieceId, s: StitchLineSpec) -> StitchLineId {
    p.apply(Command::AddStitchLine { piece, line: s })
        .unwrap()
        .stitch_lines[0]
}

fn xy(l: &StitchLayout) -> Vec<(f64, f64)> {
    l.holes
        .iter()
        .map(|h| (h.position.x, h.position.y))
        .collect()
}

fn same(got: &[(f64, f64)], want: &[(f64, f64)], tol: f64) {
    assert_eq!(got.len(), want.len(), "{got:?}\n{want:?}");
    for (g, w) in got.iter().zip(want) {
        assert!(
            (g.0 - w.0).abs() <= tol && (g.1 - w.1).abs() <= tol,
            "{g:?} != {w:?}"
        );
    }
}

// ----- CA1 : nombre et position des trous (bord droit) -------------------------------------

#[test]
fn ca1_pas_4_sur_100_mm_donne_26_trous_a_4_mm_retrait_3() {
    let mut sc = rect();
    let e = bottom(&sc);
    let id = add(&mut sc.p, sc.piece, spec(&[e], 4.0, 3.0));
    let l = sc.p.stitch_line_layout(sc.piece, id).unwrap();
    // ⌈100 / 4⌉ = 25 intervalles → 26 trous, x = 4k, y = retrait 3.
    let want: Vec<(f64, f64)> = (0..26).map(|k| (4.0 * k as f64, 3.0)).collect();
    same(&xy(&l), &want, 1e-9);
    assert_eq!(l.step_mm, 4.0);
    assert_eq!(l.length_mm, 100.0);
    assert_eq!(l.holes[0].number, 1);
    assert_eq!(l.holes[25].number, 26);
    // Direction du bord : +X (0 degré) sur le bas d'un contour anti-horaire.
    assert!(l.holes.iter().all(|h| h.tangent_deg.abs() < 1e-9));
}

#[test]
fn ca1_pas_4_5_donne_24_trous_espaces_de_100_sur_23() {
    let mut sc = rect();
    let e = bottom(&sc);
    let id = add(&mut sc.p, sc.piece, spec(&[e], 4.5, 3.0));
    let l = sc.p.stitch_line_layout(sc.piece, id).unwrap();
    // ⌈100 / 4,5⌉ = ⌈22,2…⌉ = 23 intervalles égaux de 100 / 23 mm, 24 trous.
    let want: Vec<(f64, f64)> = (0..24).map(|k| (100.0 * k as f64 / 23.0, 3.0)).collect();
    same(&xy(&l), &want, 1e-9);
    assert!((l.step_mm - 100.0 / 23.0).abs() < 1e-12);
}

#[test]
fn ca1_nombre_fixe_5_et_1() {
    let mut sc = rect();
    let e = bottom(&sc);
    let mut s = spec(&[e], 4.0, 3.0);
    s.count = Some(5);
    let id = add(&mut sc.p, sc.piece, s.clone());
    let l = sc.p.stitch_line_layout(sc.piece, id).unwrap();
    same(
        &xy(&l),
        &[
            (0.0, 3.0),
            (25.0, 3.0),
            (50.0, 3.0),
            (75.0, 3.0),
            (100.0, 3.0),
        ],
        1e-9,
    );
    s.count = Some(1);
    sc.p.apply(Command::SetStitchLine {
        piece: sc.piece,
        line: id,
        spec: s,
    })
    .unwrap();
    let l = sc.p.stitch_line_layout(sc.piece, id).unwrap();
    same(&xy(&l), &[(50.0, 3.0)], 1e-9);
    assert_eq!(l.step_mm, 0.0);
}

#[test]
fn ca1_contour_entier_ferme_sans_doublon() {
    let mut sc = rect();
    let edges: Vec<EntityId> =
        sc.p.piece(sc.piece)
            .unwrap()
            .outline
            .iter()
            .map(|e| e.entity)
            .collect();
    let id = add(&mut sc.p, sc.piece, spec(&edges, 5.0, 3.0));
    let l = sc.p.stitch_line_layout(sc.piece, id).unwrap();
    // Trajet = rectangle 94 × 64 de périmètre 316 (coins en pointe) : ⌈316 / 5⌉ = 64 trous
    // espacés de 316 / 64 = 4,9375 mm, le dernier n'est pas sur le premier.
    assert!(l.closed);
    assert_eq!(l.holes.len(), 64);
    assert!((l.length_mm - 316.0).abs() < 1e-9);
    assert!((l.step_mm - 4.9375).abs() < 1e-12);
    let (a, b) = (l.holes[0].position, l.holes[63].position);
    assert!(pf_geometry::distance_mm(a, b) > 4.0);
}

// ----- CA2 : formes de trou ------------------------------------------------------------------

#[test]
fn ca2_sommets_des_quatre_formes_calcules_a_la_main() {
    let o = pt(10.0, 20.0);
    let round = StitchHole::Round { diameter_mm: 2.0 };
    assert!(round.polygon(o, 0.0).is_none());
    // Plat 4 × 2, tangente 0 : rectangle ±2 × ±1.
    let flat = StitchHole::Flat {
        length_mm: 4.0,
        width_mm: 2.0,
    };
    let v = flat.polygon(o, 0.0).unwrap();
    for (got, want) in v
        .iter()
        .zip([(12.0, 19.0), (12.0, 21.0), (8.0, 21.0), (8.0, 19.0)])
    {
        assert!(close(*got, want.0, want.1, 1e-12), "{got:?} {want:?}");
    }
    // Losange français 3 × 2, tangente 90 : sommets tournés d'un quart de tour.
    let fr = StitchHole::French {
        length_mm: 3.0,
        width_mm: 2.0,
    };
    let v = fr.polygon(o, 90.0).unwrap();
    for (got, want) in v
        .iter()
        .zip([(10.0, 21.5), (9.0, 20.0), (10.0, 18.5), (11.0, 20.0)])
    {
        assert!(close(*got, want.0, want.1, 1e-12), "{got:?} {want:?}");
    }
    // Oblique japonais 3 × 1 incliné de 90 degrés, tangente 0.
    let jp = StitchHole::Japanese {
        length_mm: 3.0,
        width_mm: 1.0,
        slant_deg: 90.0,
    };
    let v = jp.polygon(o, 0.0).unwrap();
    for (got, want) in v
        .iter()
        .zip([(10.5, 21.5), (9.5, 21.5), (9.5, 18.5), (10.5, 18.5)])
    {
        assert!(close(*got, want.0, want.1, 1e-12), "{got:?} {want:?}");
    }
}

#[test]
fn ca2_dimensions_hors_bornes_refusees_sans_trace() {
    let mut sc = rect();
    let e = bottom(&sc);
    for hole in [
        StitchHole::Round { diameter_mm: 0.0 },
        StitchHole::Round {
            diameter_mm: f64::NAN,
        },
        StitchHole::Flat {
            length_mm: 21.0,
            width_mm: 1.0,
        },
        StitchHole::Japanese {
            length_mm: 3.0,
            width_mm: 1.0,
            slant_deg: 180.0,
        },
    ] {
        let mut s = spec(&[e], 4.0, 3.0);
        s.hole = hole;
        refused(
            &mut sc.p,
            Command::AddStitchLine {
                piece: sc.piece,
                line: s,
            },
        );
    }
}

// ----- CA3 : coins -------------------------------------------------------------------------

#[test]
fn ca3_pas_continu_autour_d_un_angle() {
    let mut sc = rect();
    let (b, r) = (bottom(&sc), right(&sc));
    let chain = sc.p.marking_run_edges(sc.piece, &[b, r]).unwrap();
    let id = add(&mut sc.p, sc.piece, spec(&chain, 4.0, 3.0));
    let l = sc.p.stitch_line_layout(sc.piece, id).unwrap();
    // Trajet (0;3) → (97;3) → (97;70) : 97 + 67 = 164 mm = 41 pas de 4 mm → 42 trous.
    assert_eq!(l.holes.len(), 42);
    assert!((l.length_mm - 164.0).abs() < 1e-9);
    let h = xy(&l);
    // s = 96 : (96 ; 3) ; s = 100, 3 mm après le coin : (97 ; 6) — 1 mm + 3 mm = un pas de 4.
    same(&h[24..26], &[(96.0, 3.0), (97.0, 6.0)], 1e-9);
    same(&h[41..], &[(97.0, 70.0)], 1e-9);
    // Un pas mesuré le long du trajet est toujours 4 mm.
    assert!((l.holes[25].abscissa_mm - l.holes[24].abscissa_mm - 4.0).abs() < 1e-9);
}

#[test]
fn ca3_fusion_de_deux_lignes_adjacentes() {
    let mut sc = rect();
    let (b, r) = (bottom(&sc), right(&sc));
    let first = add(&mut sc.p, sc.piece, spec(&[b], 4.0, 3.0));
    let second = add(&mut sc.p, sc.piece, spec(&[r], 4.0, 3.0));
    sc.p.apply(Command::MergeStitchLines {
        piece: sc.piece,
        first,
        second,
    })
    .unwrap();
    let piece = sc.p.piece(sc.piece).unwrap();
    assert_eq!(piece.stitch_lines.len(), 1);
    assert_eq!(piece.stitch_lines[0].id, first);
    let l = sc.p.stitch_line_layout(sc.piece, first).unwrap();
    assert_eq!(l.holes.len(), 42);
    same(&xy(&l)[24..26], &[(96.0, 3.0), (97.0, 6.0)], 1e-9);
}

#[test]
fn ca3_fusion_refusee_formes_ou_pas_differents_ou_non_adjacentes() {
    let mut sc = rect();
    let (b, r) = (bottom(&sc), right(&sc));
    let top = seg_between(&sc.p, sc.s, pt(100.0, 70.0), pt(0.0, 70.0));
    let first = add(&mut sc.p, sc.piece, spec(&[b], 4.0, 3.0));
    let other_pitch = add(&mut sc.p, sc.piece, spec(&[r], 5.0, 3.0));
    let far = add(&mut sc.p, sc.piece, spec(&[top], 4.0, 3.0));
    for second in [other_pitch, far] {
        refused(
            &mut sc.p,
            Command::MergeStitchLines {
                piece: sc.piece,
                first,
                second,
            },
        );
    }
}

// ----- CA3 bis : continuation ----------------------------------------------------------------

#[test]
fn ca3_continuation_poursuit_le_pas_de_la_ligne_precedente() {
    let mut sc = rect();
    let (b, r) = (bottom(&sc), right(&sc));
    let a = add(&mut sc.p, sc.piece, spec(&[b], 8.0, 3.0));
    let mut next = spec(&[r], 8.0, 3.0);
    next.continues = Some(a);
    let id = add(&mut sc.p, sc.piece, next);
    // A : ⌈100 / 8⌉ = 13 intervalles de 100/13 ; dernier trou (100 ; 3).
    // B part de (97 ; 0) : écart 3·√2 → premier trou de B à 100/13 − 3·√2 du début.
    let s0 = 100.0 / 13.0 - 3.0 * 2f64.sqrt();
    let l = sc.p.stitch_line_layout(sc.piece, id).unwrap();
    assert!((l.start_mm - s0).abs() < 1e-9);
    same(&xy(&l)[..1], &[(97.0, s0)], 1e-9);
    // ⌈(70 − s0) / 8⌉ = 9 intervalles, 10 trous.
    assert_eq!(l.holes.len(), 10);
    assert!((l.step_mm - (70.0 - s0) / 9.0).abs() < 1e-9);
    // Une continuation qui ne suit pas la ligne précédente est refusée.
    let top = seg_between(&sc.p, sc.s, pt(100.0, 70.0), pt(0.0, 70.0));
    let mut bad = spec(&[top], 8.0, 3.0);
    bad.continues = Some(a);
    refused(
        &mut sc.p,
        Command::AddStitchLine {
            piece: sc.piece,
            line: bad,
        },
    );
    // Supprimer la ligne précédente rend la suivante indépendante (avec remarque).
    let o =
        sc.p.apply(Command::RemoveStitchLine {
            piece: sc.piece,
            line: a,
        })
        .unwrap();
    assert!(!o.notes.is_empty());
    assert_eq!(
        sc.p.piece(sc.piece).unwrap().stitch_lines[0].continues,
        None
    );
}

// ----- CA4 : la ligne suit son bord, une étape annulable ------------------------------------

#[test]
fn ca4_deplacer_un_coin_recalcule_les_trous_dans_la_meme_etape() {
    let sc = rect();
    let r = right(&sc);
    let mut doc = Document::new(sc.p.clone());
    let original = bytes(doc.project());
    let id = doc
        .execute(Command::AddStitchLine {
            piece: sc.piece,
            line: spec(&[r], 4.0, 3.0),
        })
        .unwrap()
        .stitch_lines[0];
    let l = doc.project().stitch_line_layout(sc.piece, id).unwrap();
    // x = 97, y de 0 à 70 : ⌈70 / 4⌉ = 18 intervalles, 19 trous.
    assert_eq!(l.holes.len(), 19);
    let corner = point_at(doc.project(), sc.s, 100.0, 70.0);
    let before_move = bytes(doc.project());
    doc.execute(Command::MovePoint {
        sheet: sc.s,
        point: corner,
        to: pt(100.0, 60.0),
    })
    .unwrap();
    let l = doc.project().stitch_line_layout(sc.piece, id).unwrap();
    // Bord de 60 mm : 60 / 4 = 15 intervalles de 4 mm exactement, 16 trous (x = 97, y = 4k).
    assert_eq!(l.holes.len(), 16);
    for (k, h) in l.holes.iter().enumerate() {
        assert!((h.position.x - 97.0).abs() < 0.01);
        assert!((h.position.y - 4.0 * k as f64).abs() < 0.01, "{k}");
    }
    // Un déplacement qui ferait sortir un trou de la pièce est refusé (rien n'est écrit).
    let kept = bytes(doc.project());
    assert!(doc
        .execute(Command::MovePoint {
            sheet: sc.s,
            point: corner,
            to: pt(100.0, 90.0),
        })
        .is_err());
    assert_eq!(bytes(doc.project()), kept);
    // Une seule étape annulable pour le déplacement.
    assert!(doc.undo());
    assert_eq!(bytes(doc.project()), before_move);
    assert!(doc.undo());
    assert_eq!(bytes(doc.project()), original);
    assert!(!doc.can_undo());
}

#[test]
fn ca4_bord_supprime_retire_la_ligne_avec_remarque() {
    let mut sc = rect();
    let r = right(&sc);
    add(&mut sc.p, sc.piece, spec(&[r], 4.0, 3.0));
    // La pièce référence le tracé : sa suppression est refusée ou retire la ligne ; dans les deux
    // cas jamais une ligne qui référence un tracé absent.
    match sc.p.apply(Command::Delete {
        sheet: sc.s,
        points: vec![],
        entities: vec![r],
    }) {
        Ok(o) => {
            assert!(
                o.notes.iter().any(|n| n.contains("piqûre")),
                "{:?}",
                o.notes
            );
            assert!(sc
                .p
                .pieces
                .iter()
                .all(|p| p.stitch_lines.iter().all(|l| !l.edges.contains(&r))));
        }
        Err(_) => {
            assert_eq!(sc.p.piece(sc.piece).unwrap().stitch_lines.len(), 1);
        }
    }
}

// ----- CA5 : refus sans trace -------------------------------------------------------------

#[test]
fn ca5_retrait_hors_de_la_piece_refuse_sans_trace() {
    let mut sc = rect();
    let e = bottom(&sc);
    // Retrait 80 > hauteur 70 : le trajet est à y = 80, hors de la pièce.
    let msg = refused(
        &mut sc.p,
        Command::AddStitchLine {
            piece: sc.piece,
            line: spec(&[e], 4.0, 80.0),
        },
    );
    assert!(msg.contains("sortirait") || msg.contains("pièce"), "{msg}");
    // Retrait 60 (< 70) : accepté ; le passer à 75 par SetStitchLine est refusé sans trace.
    let id = add(&mut sc.p, sc.piece, spec(&[e], 4.0, 60.0));
    refused(
        &mut sc.p,
        Command::SetStitchLine {
            piece: sc.piece,
            line: id,
            spec: spec(&[e], 4.0, 75.0),
        },
    );
}

#[test]
fn ca5_valeurs_invalides_refusees_sans_trace() {
    let mut sc = rect();
    let e = bottom(&sc);
    let c = |s: StitchLineSpec| Command::AddStitchLine {
        piece: sc.piece,
        line: s,
    };
    let mut cases = vec![
        spec(&[e], 0.4, 3.0),
        spec(&[e], 101.0, 3.0),
        spec(&[e], f64::NAN, 3.0),
        spec(&[e], 4.0, 0.4),
        spec(&[e], 4.0, f64::INFINITY),
        spec(&[], 4.0, 3.0),
    ];
    let mut zero = spec(&[e], 4.0, 3.0);
    zero.count = Some(0);
    let mut many = spec(&[e], 4.0, 3.0);
    many.count = Some(501);
    cases.extend([zero, many]);
    for s in cases {
        refused(&mut sc.p, c(s));
    }
    // Tracé hors du contour, ligne inconnue.
    refused(
        &mut sc.p,
        Command::RemoveStitchLine {
            piece: sc.piece,
            line: StitchLineId(999_999),
        },
    );
}

// ----- CA6 : fichier ----------------------------------------------------------------------

#[test]
fn ca6_aller_retour_json_et_fichier_sans_ligne_identique() {
    let mut sc = rect();
    let plain = bytes(&sc.p);
    assert!(!plain.contains("stitch_lines"), "omis quand vide");
    let e = bottom(&sc);
    let mut s = spec(&[e], 4.0, 3.0);
    s.hole = StitchHole::Japanese {
        length_mm: 3.0,
        width_mm: 1.0,
        slant_deg: 45.0,
    };
    let id = add(&mut sc.p, sc.piece, s);
    let json = bytes(&sc.p);
    assert!(json.contains("\"stitch_lines\""));
    assert!(!json.contains("\"count\""), "absent quand automatique");
    let back = persist::from_json(&json).unwrap();
    assert_eq!(back, sc.p);
    assert_eq!(bytes(&back), json);
    assert_eq!(
        back.stitch_line_layout(sc.piece, id).unwrap(),
        sc.p.stitch_line_layout(sc.piece, id).unwrap()
    );
    // Clé inconnue refusée (deny_unknown_fields) ; clé réservée à la v5 refusée dans un fichier v4.
    let bad = json.replacen("\"pitch_mm\"", "\"pitch_mm_x\"", 1);
    assert!(persist::from_json(&bad).is_err());
    let v4 = json.replacen("\"schema_version\": 6", "\"schema_version\": 4", 1);
    assert_ne!(v4, json);
    let err = persist::from_json(&v4).expect_err("clé réservée à la v5");
    assert!(err.to_string().contains("stitch_lines"), "{err}");
}

#[path = "format_ouvert/validateur.rs"]
mod validateur;

#[test]
fn ca6_le_schema_json_valide_les_lignes_et_refuse_une_cle_inconnue() {
    let mut sc = rect();
    let e = bottom(&sc);
    let mut s = spec(&[e], 4.0, 3.0);
    s.hole = StitchHole::Japanese {
        length_mm: 3.0,
        width_mm: 1.0,
        slant_deg: 45.0,
    };
    let a = add(&mut sc.p, sc.piece, s);
    let mut next = spec(&[right(&sc)], 4.0, 3.0);
    next.continues = Some(a);
    next.count = Some(4);
    add(&mut sc.p, sc.piece, next);
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
    // Une clé inconnue dans une ligne et un pas hors bornes sont refusés par le schéma.
    doc["pieces"][0]["stitch_lines"][0]["extra"] = serde_json::json!(1);
    assert!(!validateur::validate(&schema, &doc).is_empty());
    doc["pieces"][0]["stitch_lines"][0]
        .as_object_mut()
        .unwrap()
        .remove("extra");
    doc["pieces"][0]["stitch_lines"][0]["pitch_mm"] = serde_json::json!(0.1);
    assert!(!validateur::validate(&schema, &doc).is_empty());
}

#[test]
fn ca6_copie_de_feuille_garde_les_lignes_sur_les_nouveaux_tracés() {
    let mut sc = rect();
    let (b, r) = (bottom(&sc), right(&sc));
    let a = add(&mut sc.p, sc.piece, spec(&[b], 8.0, 3.0));
    let mut next = spec(&[r], 8.0, 3.0);
    next.continues = Some(a);
    add(&mut sc.p, sc.piece, next);
    let o = sc.p.apply(Command::DuplicateSheet { sheet: sc.s }).unwrap();
    let copy = o.pieces[0];
    let lines = &sc.p.piece(copy).unwrap().stitch_lines;
    assert_eq!(lines.len(), 2);
    assert!(lines.iter().all(|l| l.id != a));
    assert_eq!(lines[1].continues, Some(lines[0].id));
    let (orig, dup) = (
        sc.p.stitch_line_layout(sc.piece, sc.p.piece(sc.piece).unwrap().stitch_lines[1].id)
            .unwrap(),
        sc.p.stitch_line_layout(copy, lines[1].id).unwrap(),
    );
    same(&xy(&orig), &xy(&dup), 1e-9);
}
