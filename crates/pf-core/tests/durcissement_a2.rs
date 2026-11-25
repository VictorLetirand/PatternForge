//! Durcissement 1, A2 : fichiers piégés pour la résolution des points. Aucune borne de profondeur
//! n'est posée : un projet à longue chaîne reste accepté, sans geler ni saturer la pile.
//!
//! Observé sur le code d'avant la correction : le losange dépasse le délai (gel), la chaîne de
//! 20 000 points arrête le processus (« thread has overflowed its stack »).

use pf_core::*;
use pf_geometry::PointMm;
use std::sync::mpsc;
use std::time::Duration;

/// Exécute `f` dans un fil séparé à pile de 2 Mo (celle d'un fil ordinaire) avec un délai.
fn bounded<T: Send + 'static>(secs: u64, f: impl FnOnce() -> T + Send + 'static) -> T {
    let (tx, rx) = mpsc::channel();
    let worker = std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(move || {
            let _ = tx.send(f());
        })
        .unwrap();
    match rx.recv_timeout(Duration::from_secs(secs)) {
        Ok(v) => {
            worker.join().unwrap();
            v
        }
        Err(mpsc::RecvTimeoutError::Timeout) => panic!("gel : rien après {secs} s"),
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            std::panic::resume_unwind(worker.join().unwrap_err())
        }
    }
}

fn push(p: &mut Project, definition: PointDefinition) -> PointId {
    let id = PointId(p.next_id);
    p.next_id += 1;
    p.sheets[0].points.push(Point {
        id,
        label: format!("P{}", id.0),
        definition,
        role: PointRole::Construction,
        layer: None,
    });
    id
}

fn segment(p: &mut Project, from: PointId, to: PointId) -> EntityId {
    let id = EntityId(p.next_id);
    p.next_id += 1;
    p.sheets[0].entities.push(Entity {
        id,
        role: PathRole::Construction,
        kind: EntityKind::Segment {
            from,
            to,
            construction: None,
        },
        layer: None,
        name: None,
    });
    id
}

/// Losange : à chaque niveau, deux points qui dépendent **chacun des deux** du niveau
/// précédent. Sans cache, le niveau n coûte 2^n évaluations. A0 = (0 ; 0), B0 = (10 ; 0) :
/// tous les points des niveaux suivants sont au milieu, (5 ; 0).
fn diamond(levels: usize) -> (Project, PointId) {
    let mut p = Project::new("Losange");
    let mut a = push(&mut p, PointDefinition::Free { x: 0.0, y: 0.0 });
    let mut b = push(&mut p, PointDefinition::Free { x: 10.0, y: 0.0 });
    // Premier niveau : les deux milieux sont confondus ; on les redécale pour garder deux
    // parents distincts aux niveaux suivants.
    for _ in 0..levels {
        let m = push(
            &mut p,
            PointDefinition::Midpoint {
                first: a,
                second: b,
            },
        );
        let n = push(
            &mut p,
            PointDefinition::Midpoint {
                first: b,
                second: a,
            },
        );
        (a, b) = (m, n);
    }
    (p, a)
}

#[test]
fn losange_de_40_niveaux_ouvert_sans_gel() {
    let (p, last) = diamond(40);
    let json = serde_json::to_string(&p).unwrap();
    let at = bounded(20, move || {
        let loaded = persist::from_json(&json).unwrap();
        loaded.sheets[0].position(last).unwrap()
    });
    assert_eq!((at.x, at.y), (5.0, 0.0));
}

/// Une seule lecture, hors validation, sur un losange profond.
#[test]
fn losange_lecture_isolee_sans_gel() {
    let (p, last) = diamond(200);
    let at = bounded(20, move || p.sheets[0].position(last).unwrap());
    assert_eq!((at.x, at.y), (5.0, 0.0));
}

/// Chaîne de décalages de 0,5 mm : le point de rang k est à x = 0,5·k (exact en binaire).
/// `reversed` : le dernier point en tête de liste, pire ordre pour la validation.
fn chain(n: usize, reversed: bool) -> (Project, PointId, PointId) {
    let mut p = Project::new("Chaîne");
    let first = push(&mut p, PointDefinition::Free { x: 0.0, y: 0.0 });
    let mut last = first;
    for _ in 1..n {
        last = push(
            &mut p,
            PointDefinition::Offset {
                from: last,
                dx_mm: 0.5,
                dy_mm: 0.0,
            },
        );
    }
    if reversed {
        p.sheets[0].points.reverse();
    }
    (p, first, last)
}

/// Une chaîne de constructions successives plausible en atelier (plusieurs centaines).
#[test]
fn chaine_d_atelier_de_600_points_acceptee() {
    let (p, _, last) = chain(600, true);
    let json = serde_json::to_string(&p).unwrap();
    let at = bounded(60, move || {
        let loaded = persist::from_json(&json).unwrap();
        loaded.sheets[0].position(last).unwrap()
    });
    assert_eq!((at.x, at.y), (299.5, 0.0));
}

#[test]
fn chaine_de_20000_points_ouverte_sans_saturer_la_pile() {
    let (p, _, last) = chain(20_000, true);
    let json = serde_json::to_string(&p).unwrap();
    let at = bounded(60, move || {
        let loaded = persist::from_json(&json).unwrap();
        loaded.sheets[0].position(last).unwrap()
    });
    assert_eq!((at.x, at.y), (9999.5, 0.0));
}

/// Chaîne qui passe par des **tracés** : chaque point est à mi-chemin du segment qui joint les
/// deux précédents. Départ (0 ; 0) et (1024 ; 0) : la suite tend vers 2/3 de 1024.
#[test]
fn chaine_de_points_sur_segments_sans_saturer_la_pile() {
    let mut p = Project::new("Chaîne mixte");
    let mut a = push(&mut p, PointDefinition::Free { x: 0.0, y: 0.0 });
    let mut b = push(&mut p, PointDefinition::Free { x: 1024.0, y: 0.0 });
    // Au-delà de ~25 rangs les segments deviendraient trop courts : on alterne avec un décalage.
    for i in 0..6_000 {
        let seg = segment(&mut p, a, b);
        let c = push(
            &mut p,
            PointDefinition::OnSegmentRatio {
                segment: seg,
                ratio: 0.5,
            },
        );
        let d = push(
            &mut p,
            PointDefinition::Offset {
                from: c,
                dx_mm: if i % 2 == 0 { 8.0 } else { -8.0 },
                dy_mm: 1.0,
            },
        );
        (a, b) = (c, d);
    }
    p.sheets[0].points.reverse();
    let json = serde_json::to_string(&p).unwrap();
    let ys = bounded(120, move || {
        let loaded = persist::from_json(&json).unwrap();
        let s = &loaded.sheets[0];
        (s.position(a).unwrap().y, s.position(b).unwrap().y)
    });
    // Chaque rang ajoute 0,5 (moitié du décalage de 1 mm) ; le dernier décalage vaut 1.
    assert!((ys.1 - ys.0 - 1.0).abs() < 1e-9, "{ys:?}");
}

/// Un cycle au bout d'une longue chaîne : refusé proprement, sans saturer la pile.
#[test]
fn cycle_au_bout_d_une_longue_chaine_refuse_proprement() {
    let (mut p, first, last) = chain(20_000, false);
    p.sheets[0].points[0].definition = PointDefinition::Offset {
        from: last,
        dx_mm: 1.0,
        dy_mm: 0.0,
    };
    let json = serde_json::to_string(&p).unwrap();
    let err = bounded(60, move || persist::from_json(&json).unwrap_err());
    // La validation lit les points dans l'ordre : le premier lu est celui qui se referme.
    assert_eq!(err, CoreError::CyclicReference(first));
}

/// Les commandes, l'annulation, l'enregistrement et la lecture d'une longue chaîne.
#[test]
fn commandes_sur_une_longue_chaine() {
    let (p, first, last) = chain(3_000, false);
    let sheet = p.sheets[0].id;
    bounded(120, move || {
        p.validate().unwrap();
        let mut doc = Document::new(p);
        doc.execute(Command::MovePoint {
            sheet,
            point: first,
            to: PointMm::new(100.0, 7.0),
        })
        .unwrap();
        let at = doc.project().sheets[0].position(last).unwrap();
        assert_eq!((at.x, at.y), (100.0 + 1499.5, 7.0));
        doc.execute(Command::AddSegment {
            sheet,
            from: PointRef::Existing(first),
            to: PointRef::Existing(last),
        })
        .unwrap();
        assert!(doc.undo() && doc.undo());
        let at = doc.project().sheets[0].position(last).unwrap();
        assert_eq!((at.x, at.y), (1499.5, 0.0));
        let json = persist::to_json(doc.project()).unwrap();
        assert_eq!(&persist::from_json(&json).unwrap(), doc.project());
    });
}
