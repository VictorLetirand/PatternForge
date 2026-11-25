//! US-03.2 — constructions maintenues (segment longueur/angle, perpendiculaire, parallèle) :
//! calculs faits à la main, refus sans trace (octets identiques), une seule étape annulable,
//! coordination avec les formules (03.1b), copies, ouverture d'un fichier altéré.
//!
//! Le rectangle n'est pas repris (US-03.2b) : voir `docs/DECISIONS.md` 319.

use pf_core::persist;
use pf_core::*;
use pf_geometry::{PointMm, Side};
use serde_json::Value;

const TOL: f64 = 1e-9;

fn bytes(p: &Project) -> String {
    persist::to_json(p).unwrap()
}

fn pos(p: &Project, s: SheetId, id: PointId) -> PointMm {
    p.sheet(s).unwrap().position(id).unwrap()
}

fn at(p: &Project, s: SheetId, id: PointId, x: f64, y: f64) {
    let q = pos(p, s, id);
    assert!(
        (q.x - x).abs() < TOL && (q.y - y).abs() < TOL,
        "position {q:?}, attendu ({x} ; {y})"
    );
}

/// Commande refusée : projet **identique à l'octet** ; renvoie le message.
fn refused(p: &mut Project, cmd: Command) -> String {
    let before = bytes(p);
    let before_p = p.clone();
    let e = p.apply(cmd).expect_err("devrait être refusée").to_string();
    assert_eq!(bytes(p), before, "trace laissée par un refus : {e}");
    assert_eq!(*p, before_p);
    e
}

fn keep(s: SheetId, segment: EntityId, kept: bool) -> Command {
    Command::SetConstructionKept {
        sheet: s,
        segment,
        kept,
    }
}

fn mv(s: SheetId, point: PointId, x: f64, y: f64) -> Command {
    Command::MovePoint {
        sheet: s,
        point,
        to: PointMm::new(x, y),
    }
}

fn is_kept(p: &Project, s: SheetId, e: EntityId) -> bool {
    p.sheet(s).unwrap().is_kept_segment(e)
}

fn free(p: &mut Project, s: SheetId, x: f64, y: f64) -> PointId {
    p.apply(Command::AddPoint {
        sheet: s,
        at: PointMm::new(x, y),
    })
    .unwrap()
    .points[0]
}

/// A (0 ; 0), segment de 100 mm à 30° : renvoie (projet, feuille, A, B, segment).
fn polar_project() -> (Project, SheetId, PointId, PointId, EntityId) {
    let mut p = Project::new("Maintenu");
    let s = p.sheets[0].id;
    let a = free(&mut p, s, 0.0, 0.0);
    let o = p
        .apply(Command::AddSegmentPolar {
            sheet: s,
            from: PointRef::Existing(a),
            length_mm: 100.0,
            angle_deg: 30.0,
        })
        .unwrap();
    (p, s, a, o.points[0], o.entities[0])
}

/// Référence P0 (0 ; 0) → P1 (100 ; 0), puis une perpendiculaire de 40 mm à gauche, partant de
/// F (30 ; 0) : renvoie (projet, feuille, P0, P1, F, E, référence, perpendiculaire).
#[allow(clippy::type_complexity)]
fn perp_project() -> (
    Project,
    SheetId,
    PointId,
    PointId,
    PointId,
    PointId,
    EntityId,
    EntityId,
) {
    let mut p = Project::new("Perpendiculaire");
    let s = p.sheets[0].id;
    let r = p
        .apply(Command::AddSegment {
            sheet: s,
            from: PointRef::New(PointMm::new(0.0, 0.0)),
            to: PointRef::New(PointMm::new(100.0, 0.0)),
        })
        .unwrap();
    let (p0, p1, reference) = (r.points[0], r.points[1], r.entities[0]);
    let f = free(&mut p, s, 30.0, 0.0);
    let o = p
        .apply(Command::AddPerpendicular {
            sheet: s,
            reference,
            from: PointRef::Existing(f),
            length_mm: 40.0,
            side: Side::Left,
        })
        .unwrap();
    (p, s, p0, p1, f, o.points[0], reference, o.entities[0])
}

#[test]
fn ca_segment_longueur_angle_maintenu_suit_son_point_de_depart() {
    let (mut p, s, a, b, seg) = polar_project();
    // 100 (cos 30° ; sin 30°) = (86,602540378… ; 50).
    let (bx, by) = (100.0 * 3f64.sqrt() / 2.0, 50.0);
    at(&p, s, b, bx, by);
    assert!(
        !is_kept(&p, s, seg),
        "les projets existants restent non maintenus"
    );
    let mut doc = Document::new(p.clone());
    doc.execute(keep(s, seg, true)).unwrap();
    assert!(is_kept(doc.project(), s, seg));
    at(doc.project(), s, b, bx, by);
    let kept_bytes = bytes(doc.project());
    // A (10 ; 20) : B = (96,6025… ; 70), dans la même étape.
    doc.execute(mv(s, a, 10.0, 20.0)).unwrap();
    at(doc.project(), s, b, 10.0 + bx, 20.0 + by);
    assert!(doc.undo());
    assert_eq!(bytes(doc.project()), kept_bytes, "annuler : retour exact");
    assert!(doc.undo());
    assert_eq!(bytes(doc.project()), bytes(&p), "une étape pour lier");
    assert!(!doc.undo(), "pas d'étape en trop");
    // Non maintenu : B reste en place quand A bouge.
    p.apply(mv(s, a, 10.0, 20.0)).unwrap();
    at(&p, s, b, bx, by);
}

#[test]
fn ca_perpendiculaire_maintenue_suit_sa_reference() {
    let (mut p, s, _p0, p1, f, e, _r, seg) = perp_project();
    // F (30 ; 0), normale gauche de (1 ; 0) = (0 ; 1) : E = (30 ; 40).
    at(&p, s, e, 30.0, 40.0);
    p.apply(keep(s, seg, true)).unwrap();
    // Référence tournée vers (0 ; 100) : direction (0 ; 1), normale gauche (-1 ; 0) : E = (-10 ; 0).
    p.apply(mv(s, p1, 0.0, 100.0)).unwrap();
    at(&p, s, e, -10.0, 0.0);
    at(&p, s, f, 30.0, 0.0);
    // Le point de départ bouge aussi : F (50 ; 10) : E = (10 ; 10).
    p.apply(mv(s, f, 50.0, 10.0)).unwrap();
    at(&p, s, e, 10.0, 10.0);
}

#[test]
fn ca_parallele_maintenue_suit_sa_reference() {
    let mut p = Project::new("Parallèle");
    let s = p.sheets[0].id;
    let r = p
        .apply(Command::AddSegment {
            sheet: s,
            from: PointRef::New(PointMm::new(0.0, 0.0)),
            to: PointRef::New(PointMm::new(100.0, 0.0)),
        })
        .unwrap();
    let (p1, reference) = (r.points[1], r.entities[0]);
    let o = p
        .apply(Command::AddParallel {
            sheet: s,
            reference,
            distance_mm: 20.0,
            side: Side::Left,
        })
        .unwrap();
    let (a, b, seg) = (o.points[0], o.points[1], o.entities[0]);
    at(&p, s, a, 0.0, 20.0);
    at(&p, s, b, 100.0, 20.0);
    p.apply(keep(s, seg, true)).unwrap();
    // Référence (0 ; 0)-(100 ; 50) : longueur √12500 = 111,803398875 ; normale gauche
    // (-50 ; 100) / 111,803398875 ; fois 20 = (-8,94427191 ; 17,88854382).
    p.apply(mv(s, p1, 100.0, 50.0)).unwrap();
    let (nx, ny) = (
        -50.0 / 12500f64.sqrt() * 20.0,
        100.0 / 12500f64.sqrt() * 20.0,
    );
    at(&p, s, a, nx, ny);
    at(&p, s, b, 100.0 + nx, 50.0 + ny);
    assert!((nx + 8.94427191).abs() < 1e-8 && (ny - 17.88854382).abs() < 1e-8);
}

#[test]
fn chaine_de_constructions_maintenues_dans_l_ordre_topologique() {
    // A -> B (100 mm, 0°) -> C (50 mm, 90°) : déplacer A déplace B puis C en une étape.
    let mut p = Project::new("Chaîne");
    let s = p.sheets[0].id;
    let a = free(&mut p, s, 0.0, 0.0);
    // Créées dans l'ordre inverse du graphe : le second segment est lié avant le premier.
    let o1 = p
        .apply(Command::AddSegmentPolar {
            sheet: s,
            from: PointRef::Existing(a),
            length_mm: 100.0,
            angle_deg: 0.0,
        })
        .unwrap();
    let (b, s1) = (o1.points[0], o1.entities[0]);
    let o2 = p
        .apply(Command::AddSegmentPolar {
            sheet: s,
            from: PointRef::Existing(b),
            length_mm: 50.0,
            angle_deg: 90.0,
        })
        .unwrap();
    let (c, s2) = (o2.points[0], o2.entities[0]);
    p.apply(keep(s, s2, true)).unwrap();
    p.apply(keep(s, s1, true)).unwrap();
    at(&p, s, c, 100.0, 50.0);
    let mut doc = Document::new(p);
    doc.execute(mv(s, a, -5.0, 7.0)).unwrap();
    at(doc.project(), s, b, 95.0, 7.0);
    at(doc.project(), s, c, 95.0, 57.0);
    assert!(doc.undo());
    at(doc.project(), s, c, 100.0, 50.0);
    assert!(!doc.undo(), "une seule étape pour toute la chaîne");
}

#[test]
fn delier_garde_les_positions_et_un_non_changement_n_est_pas_une_etape() {
    let (mut p, s, a, b, seg) = polar_project();
    p.apply(keep(s, seg, true)).unwrap();
    p.apply(mv(s, a, 10.0, 20.0)).unwrap();
    let moved = pos(&p, s, b);
    let mut doc = Document::new(p);
    doc.execute(keep(s, seg, false)).unwrap();
    assert!(!is_kept(doc.project(), s, seg));
    assert_eq!(
        pos(doc.project(), s, b),
        moved,
        "délier garde les positions"
    );
    // Déjà délié, déjà lié : aucune étape.
    let before = bytes(doc.project());
    doc.execute(keep(s, seg, false)).unwrap();
    assert_eq!(bytes(doc.project()), before);
    assert!(doc.undo());
    assert!(is_kept(doc.project(), s, seg));
    assert!(!doc.undo(), "le second délier n'a pas créé d'étape");
    assert!(doc.redo());
    let mut p = doc.project().clone();
    p.apply(mv(s, a, 0.0, 0.0)).unwrap();
    assert_eq!(pos(&p, s, b), moved, "délié : B ne suit plus");
}

#[test]
fn liaison_refusee_sans_trace() {
    let (mut p, s, a, b, seg) = polar_project();
    // Segment sans construction mémorisée.
    let plain = p
        .apply(Command::AddSegment {
            sheet: s,
            from: PointRef::New(PointMm::new(500.0, 0.0)),
            to: PointRef::New(PointMm::new(600.0, 0.0)),
        })
        .unwrap()
        .entities[0];
    let msg = refused(&mut p, keep(s, plain, true));
    assert!(msg.contains("aucune construction"), "{msg}");
    // Tracé et feuille inconnus.
    refused(&mut p, keep(s, EntityId(999_999), true));
    refused(&mut p, keep(SheetId(999_999), seg, true));
    // B déplacé à la main depuis la création : la construction ne décrit plus le segment.
    p.apply(mv(s, b, 90.0, 60.0)).unwrap();
    let msg = refused(&mut p, keep(s, seg, true));
    assert!(msg.contains("ne correspond plus"), "{msg}");
    let _ = a;
}

#[test]
fn refus_point_deja_suivi_point_d_arc_et_point_construit() {
    // Deux segments qui finissent au même point B : un seul peut le suivre.
    let mut p = Project::new("Doublon");
    let s = p.sheets[0].id;
    let a1 = free(&mut p, s, 0.0, 0.0);
    let a2 = free(&mut p, s, 100.0, 0.0);
    let o1 = p
        .apply(Command::AddSegmentPolar {
            sheet: s,
            from: PointRef::Existing(a1),
            length_mm: 50.0,
            angle_deg: 0.0,
        })
        .unwrap();
    let (b, s1) = (o1.points[0], o1.entities[0]);
    let o2 = p
        .apply(Command::AddSegmentPolar {
            sheet: s,
            from: PointRef::Existing(a2),
            length_mm: 50.0,
            angle_deg: 180.0,
        })
        .unwrap();
    assert_eq!(o2.points.len(), 0, "B réutilisé : même position");
    let s2 = o2.entities[0];
    p.apply(keep(s, s1, true)).unwrap();
    let msg = refused(&mut p, keep(s, s2, true));
    assert!(msg.contains("déjà suivi"), "{msg}");
    // B ne peut pas devenir l'extrémité d'un arc tant qu'il est suivi.
    let msg = refused(
        &mut p,
        Command::AddArcThreePoints {
            sheet: s,
            start: PointRef::Existing(b),
            through: PointRef::New(PointMm::new(75.0, 30.0)),
            end: PointRef::New(PointMm::new(100.0, 0.0)),
        },
    );
    assert!(msg.contains("arc") || msg.contains("Arc"), "{msg}");
}

#[test]
fn deplacement_refuse_si_le_resultat_est_degenere_sans_trace() {
    let (mut p, s, _p0, p1, _f, _e, _r, seg) = perp_project();
    p.apply(keep(s, seg, true)).unwrap();
    // La référence devient un point : la perpendiculaire n'a plus de direction, tout est refusé,
    // et le message vient de la construction maintenue (pas d'une validation générique).
    let msg = refused(&mut p, mv(s, p1, 0.0, 0.0));
    assert!(msg.contains("construction maintenue"), "{msg}");
    // Même chose pour une parallèle.
    let mut q = Project::new("Parallèle dégénérée");
    let qs = q.sheets[0].id;
    let r = q
        .apply(Command::AddSegment {
            sheet: qs,
            from: PointRef::New(PointMm::new(0.0, 0.0)),
            to: PointRef::New(PointMm::new(100.0, 0.0)),
        })
        .unwrap();
    let o = q
        .apply(Command::AddParallel {
            sheet: qs,
            reference: r.entities[0],
            distance_mm: 20.0,
            side: Side::Right,
        })
        .unwrap();
    q.apply(keep(qs, o.entities[0], true)).unwrap();
    let msg = refused(&mut q, mv(qs, r.points[1], 0.0, 0.0));
    assert!(msg.contains("construction maintenue"), "{msg}");
}

#[test]
fn calque_verrouille_refuse_la_commande_mere_sans_trace() {
    let (mut p, s, a, b, seg) = polar_project();
    p.apply(keep(s, seg, true)).unwrap();
    let layer = p
        .apply(Command::AddLayer {
            sheet: s,
            name: "Verrou".into(),
        })
        .unwrap()
        .layers[0];
    p.apply(Command::SetObjectsLayer {
        sheet: s,
        points: vec![b],
        entities: vec![],
        layer: Some(layer),
    })
    .unwrap();
    p.apply(Command::SetLayerLocked {
        sheet: s,
        layer,
        locked: true,
    })
    .unwrap();
    let msg = refused(&mut p, mv(s, a, 10.0, 20.0));
    assert!(msg.contains("verrouillé"), "{msg}");
    assert!(msg.contains("construction maintenue"), "{msg}");
}

#[test]
fn deplacer_directement_le_point_suivi_delie_avec_remarque() {
    let (mut p, s, a, b, seg) = polar_project();
    p.apply(keep(s, seg, true)).unwrap();
    let o = p.apply(mv(s, b, 70.0, 80.0)).unwrap();
    assert!(!is_kept(&p, s, seg));
    assert!(
        o.notes.iter().any(|n| n.contains("n'est plus maintenu")),
        "{:?}",
        o.notes
    );
    at(&p, s, b, 70.0, 80.0);
    // Délié : A ne tire plus B.
    p.apply(mv(s, a, 5.0, 5.0)).unwrap();
    at(&p, s, b, 70.0, 80.0);
}

#[test]
fn translation_de_tout_le_segment_reste_maintenue_rotation_delie() {
    let (mut p, s, a, b, seg) = polar_project();
    p.apply(keep(s, seg, true)).unwrap();
    p.apply(Command::Transform {
        sheet: s,
        points: vec![a, b],
        transform: pf_geometry::Transform::Translate {
            dx_mm: 10.0,
            dy_mm: -5.0,
        },
    })
    .unwrap();
    assert!(is_kept(&p, s, seg), "translation : toujours cohérent");
    let o = p
        .apply(Command::Transform {
            sheet: s,
            points: vec![a, b],
            transform: pf_geometry::Transform::Rotate {
                center: PointMm::new(0.0, 0.0),
                angle_deg: 90.0,
            },
        })
        .unwrap();
    // L'angle absolu (30°) ne décrit plus le segment tourné : délié, avec remarque.
    assert!(!is_kept(&p, s, seg));
    assert!(o.notes.iter().any(|n| n.contains("n'est plus maintenu")));
}

#[test]
fn reference_supprimee_delie_avec_remarque() {
    let (mut p, s, _p0, _p1, _f, e, reference, seg) = perp_project();
    p.apply(keep(s, seg, true)).unwrap();
    let before = pos(&p, s, e);
    let o = p
        .apply(Command::Delete {
            sheet: s,
            points: vec![],
            entities: vec![reference],
        })
        .unwrap();
    assert!(!is_kept(&p, s, seg));
    assert!(
        o.notes.iter().any(|n| n.contains("n'est plus maintenu")),
        "{:?}",
        o.notes
    );
    assert_eq!(pos(&p, s, e), before);
}

#[test]
fn inverser_un_segment_maintenu_delie_avec_remarque() {
    let (mut p, s, _a, b, seg) = polar_project();
    p.apply(keep(s, seg, true)).unwrap();
    let o = p
        .apply(Command::ReversePath {
            sheet: s,
            entity: seg,
        })
        .unwrap();
    assert!(!is_kept(&p, s, seg));
    assert!(o.notes.iter().any(|n| n.contains("n'est plus maintenu")));
    let _ = b;
}

#[test]
fn coordination_avec_les_formules_un_point_en_direction_suit_le_point_maintenu() {
    // B (suivi) -> P = B + (L ; 0°) avec formule sur la distance : déplacer A déplace B puis P ;
    // changer L déplace P sans toucher B.
    let (mut p, s, a, b, seg) = polar_project();
    p.apply(Command::AddVariable {
        name: "L".into(),
        unit: VariableUnit::Mm,
        expr: "40".into(),
    })
    .unwrap();
    let q = p
        .apply(Command::AddPointAt {
            sheet: s,
            point: PointRef::Derived(PointDefinition::Polar {
                from: b,
                distance_mm: 1.0,
                angle_deg: 0.0,
                reference: None,
            }),
        })
        .unwrap()
        .points[0];
    p.apply(Command::SetParameterExpr {
        target: ParamTarget {
            sheet: s,
            point: q,
            param: ParamKind::Distance,
        },
        expr: Some("L*2".into()),
    })
    .unwrap();
    p.apply(keep(s, seg, true)).unwrap();
    let bx = 100.0 * 3f64.sqrt() / 2.0;
    at(&p, s, q, bx + 80.0, 50.0);
    p.apply(mv(s, a, 10.0, 20.0)).unwrap();
    at(&p, s, b, 10.0 + bx, 70.0);
    at(&p, s, q, 10.0 + bx + 80.0, 70.0);
    assert!(p
        .sheet(s)
        .unwrap()
        .formula(q, ParamKind::Distance)
        .is_some());
    p.apply(Command::SetVariableExpr {
        name: "L".into(),
        expr: "10".into(),
    })
    .unwrap();
    at(&p, s, b, 10.0 + bx, 70.0);
    at(&p, s, q, 10.0 + bx + 20.0, 70.0);
}

#[test]
fn coordination_avec_les_formules_un_parent_deplace_par_formule_entraine_le_point_suivi() {
    // X = A + (L ; 0°) (formule), segment de 100 mm à 0° depuis X, maintenu : L -> X -> B.
    let mut p = Project::new("Formule parent");
    let s = p.sheets[0].id;
    p.apply(Command::AddVariable {
        name: "L".into(),
        unit: VariableUnit::Mm,
        expr: "30".into(),
    })
    .unwrap();
    let a = free(&mut p, s, 0.0, 0.0);
    let x = p
        .apply(Command::AddPointAt {
            sheet: s,
            point: PointRef::Derived(PointDefinition::Polar {
                from: a,
                distance_mm: 30.0,
                angle_deg: 0.0,
                reference: None,
            }),
        })
        .unwrap()
        .points[0];
    p.apply(Command::SetParameterExpr {
        target: ParamTarget {
            sheet: s,
            point: x,
            param: ParamKind::Distance,
        },
        expr: Some("L".into()),
    })
    .unwrap();
    let o = p
        .apply(Command::AddSegmentPolar {
            sheet: s,
            from: PointRef::Existing(x),
            length_mm: 100.0,
            angle_deg: 0.0,
        })
        .unwrap();
    let (b, seg) = (o.points[0], o.entities[0]);
    p.apply(keep(s, seg, true)).unwrap();
    let mut doc = Document::new(p);
    doc.execute(Command::SetVariableExpr {
        name: "L".into(),
        expr: "50".into(),
    })
    .unwrap();
    at(doc.project(), s, x, 50.0, 0.0);
    at(doc.project(), s, b, 150.0, 0.0);
    assert!(doc.undo());
    at(doc.project(), s, b, 130.0, 0.0);
    assert!(!doc.undo(), "variable, formule et construction : une étape");
}

#[test]
fn copie_de_feuille_garde_le_lien_sur_la_copie_seulement() {
    let (mut p, s, a, b, seg) = polar_project();
    p.apply(keep(s, seg, true)).unwrap();
    let o = p.apply(Command::DuplicateSheet { sheet: s }).unwrap();
    let s2 = o.sheet.unwrap();
    let copy = p.sheet(s2).unwrap();
    let kept: Vec<_> = copy.kept_segment_ids();
    assert_eq!(kept.len(), 1);
    // Les identifiants de la copie sont neufs ; bouger A d'origine ne bouge pas la copie.
    let (ca, cb) = copy.segment_point_ids(kept[0]).unwrap();
    let cb_before = pos(&p, s2, cb);
    p.apply(mv(s, a, 3.0, 4.0)).unwrap();
    assert_eq!(pos(&p, s2, cb), cb_before);
    at(&p, s, b, 3.0 + 100.0 * 3f64.sqrt() / 2.0, 54.0);
    p.apply(mv(s2, ca, 3.0, 4.0)).unwrap();
    assert!((pos(&p, s2, cb).y - 54.0).abs() < TOL);
}

#[test]
fn presse_papiers_copie_non_maintenue_avec_remarque() {
    let (mut p, s, a, b, seg) = polar_project();
    p.apply(keep(s, seg, true)).unwrap();
    let frag = p.copy_fragment(s, &[a, b], &[seg], &[], &[]).unwrap();
    assert!(
        frag.notes.iter().any(|n| n.contains("maintenue")),
        "{:?}",
        frag.notes
    );
    let o = p
        .apply(Command::PasteFragment {
            sheet: s,
            fragment: Box::new(frag),
            layer: None,
        })
        .unwrap();
    let pasted = o.entities[0];
    assert!(!is_kept(&p, s, pasted));
    assert!(is_kept(&p, s, seg), "l'original reste maintenu");
}

#[test]
fn format_option_omis_quand_faux_et_relecture_identique() {
    let (mut p, s, _a, _b, seg) = polar_project();
    let plain = bytes(&p);
    assert!(!plain.contains("kept"), "champ omis quand faux");
    p.apply(keep(s, seg, true)).unwrap();
    let json = bytes(&p);
    assert!(json.contains("\"kept\": true"));
    let back = persist::from_json(&json).unwrap();
    assert_eq!(bytes(&back), json, "relecture identique");
    p.apply(keep(s, seg, false)).unwrap();
    assert_eq!(bytes(&p), plain, "délié : octets d'avant");
}

#[test]
fn ouverture_refuse_une_position_alteree_et_un_point_deja_suivi() {
    let (mut p, s, _a, b, seg) = polar_project();
    p.apply(keep(s, seg, true)).unwrap();
    let json = bytes(&p);
    let mut v: Value = serde_json::from_str(&json).unwrap();
    // B écrit 5 mm trop loin : refus localisé.
    let pts = v["sheets"][0]["points"].as_array_mut().unwrap();
    let pb = pts
        .iter_mut()
        .find(|q| q["id"].as_u64() == Some(b.0))
        .unwrap();
    pb["definition"]["x"] = Value::from(pb["definition"]["x"].as_f64().unwrap() + 5.0);
    let err = persist::from_json(&v.to_string()).unwrap_err().to_string();
    assert!(err.contains("construction maintenue"), "{err}");
    assert!(err.contains(p.sheet(s).unwrap().label(b)), "{err}");
    // Référence morte : refus.
    let (mut q, qs, _p0, _p1, _f, _e, _r, qseg) = perp_project();
    q.apply(keep(qs, qseg, true)).unwrap();
    let mut v: Value = serde_json::from_str(&bytes(&q)).unwrap();
    let ents = v["sheets"][0]["entities"].as_array_mut().unwrap();
    let e = ents
        .iter_mut()
        .find(|e| e["kind"]["construction"]["kept"].as_bool() == Some(true))
        .unwrap();
    e["kind"]["construction"]["reference"] = Value::from(424_242u64);
    assert!(persist::from_json(&v.to_string()).is_err());
}

#[test]
fn cycle_refuse_a_la_liaison() {
    // A est un point en direction de B (B -> A), puis un segment A -> B (longueur/angle) créé
    // vers le point B existant : le lier ferait dépendre B de A, et A de B.
    let mut p = Project::new("Cycle");
    let s = p.sheets[0].id;
    let b = free(&mut p, s, 100.0, 0.0);
    let a = p
        .apply(Command::AddPointAt {
            sheet: s,
            point: PointRef::Derived(PointDefinition::Polar {
                from: b,
                distance_mm: 100.0,
                angle_deg: 180.0,
                reference: None,
            }),
        })
        .unwrap()
        .points[0];
    let o = p
        .apply(Command::AddSegmentPolar {
            sheet: s,
            from: PointRef::Existing(a),
            length_mm: 100.0,
            angle_deg: 0.0,
        })
        .unwrap();
    assert!(o.points.is_empty(), "B réutilisé");
    let msg = refused(&mut p, keep(s, o.entities[0], true));
    assert!(msg.contains("circulaire"), "{msg}");
}
