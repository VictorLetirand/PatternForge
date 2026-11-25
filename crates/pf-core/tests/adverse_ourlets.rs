//! US-05.3 — campagne adverse des ourlets et rempliés : 180 séquences à graine fixe.
//!
//! - 108 polygones étoilés à 3 à 8 sommets (coordonnées paires, sommets lisses ajoutés, coins
//!   rentrants) avec, par bord, une surcharge, un ourlet, un rempli ou la marge par défaut, des
//!   ourlets voisins, des types de coin choisis à la main sur un sommet d'ourlet ;
//! - 36 quartiers de disque (arc dessiné dans les deux sens) avec un rempli sur l'arc : pas,
//!   nombre ou défaut, rayons assez grands pour dépasser les 1000 crans ;
//! - 36 disques entiers avec un rempli et des crans.
//!
//! Oracle **indépendant** : rien de `pf_geometry::seam`, `pf_geometry::hem` ni de
//! `piece_cut_contour` / `piece_fold_notches` n'y sert à juger. Polygones : l'oracle reconstruit
//! chaque coin par intersections de droites et réflexion de points (l'implémentation raisonne par
//! paramètres le long des tangentes), puis compare sommets, aire (formule du lacet) et périmètre.
//! Quartiers et disques : aire, périmètre, rayon de l'arc décalé et crans (nombre, rang, distance,
//! point, normale) calculés par la trigonométrie. Propriétés :
//!
//! 1. jamais de panique ; résultat identique à graine égale ;
//! 2. toute commande invalide est refusée sans trace (octets identiques, aucune étape) ;
//! 3. un contour de coupe que l'oracle juge valide est reproduit à 1e-6 mm ; sinon l'échec est
//!    un contrôle `CutContourFailed`, jamais un refus de commande ;
//! 4. crans de rempli : exactement ceux de l'oracle, ou `FoldNotchesFailed` quand l'oracle compte
//!    plus de 1000 crans ;
//! 5. annuler rend les octets d'avant, rétablir rend le même contour ; JSON : aller-retour exact.

use pf_core::persist;
use pf_core::piece::PieceIssue;
use pf_core::*;
use pf_geometry::{PointMm, Trace};
use std::f64::consts::PI;
use std::panic::{catch_unwind, AssertUnwindSafe};

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
    fn chance(&mut self, percent: u64) -> bool {
        self.next() % 100 < percent
    }
    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
    fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.unit()
    }
    /// Multiple de 0,25 mm dans `[lo ; hi]`.
    fn quarter(&mut self, lo: f64, hi: f64) -> f64 {
        (self.range(lo, hi) * 4.0).round() / 4.0
    }
}

// ------------------------------------------------------------------ vecteurs de l'oracle

type V2 = (f64, f64);

fn sub(a: V2, b: V2) -> V2 {
    (a.0 - b.0, a.1 - b.1)
}
fn add(a: V2, b: V2) -> V2 {
    (a.0 + b.0, a.1 + b.1)
}
fn mul(a: V2, k: f64) -> V2 {
    (a.0 * k, a.1 * k)
}
fn dot(a: V2, b: V2) -> f64 {
    a.0 * b.0 + a.1 * b.1
}
fn cross(a: V2, b: V2) -> f64 {
    a.0 * b.1 - a.1 * b.0
}
fn dist(a: V2, b: V2) -> f64 {
    let d = sub(a, b);
    (d.0 * d.0 + d.1 * d.1).sqrt()
}
fn unit(a: V2) -> V2 {
    let n = (a.0 * a.0 + a.1 * a.1).sqrt();
    (a.0 / n, a.1 / n)
}

/// Point commun de deux droites `p + t d` et `q + s e` (`None` si parallèles).
fn isect(p: V2, d: V2, q: V2, e: V2) -> Option<V2> {
    let den = cross(d, e);
    if den.abs() < 1e-12 {
        return None;
    }
    let t = cross(sub(q, p), e) / den;
    Some(add(p, mul(d, t)))
}

/// Symétrique de `p` par rapport à la droite `v + t d`.
fn reflect(p: V2, v: V2, d: V2) -> V2 {
    let d = unit(d);
    let w = sub(p, v);
    let along = mul(d, dot(w, d));
    add(v, sub(mul(along, 2.0), w))
}

// ------------------------------------------------------------------ oracle des coins

#[derive(Clone, Copy, Debug, PartialEq)]
enum K {
    Mitre,
    Stepped,
    HemNext,
    HemPrev,
}

/// Points du contour de coupe au sommet `v` (entre `vp` et `vn`), de la fin de la droite décalée
/// précédente au début de la suivante. `None` : construction impossible pour l'oracle.
fn corner_pts(vp: V2, v: V2, vn: V2, a: f64, b: f64, kind: K) -> Option<Vec<V2>> {
    let (t1, t2) = (unit(sub(v, vp)), unit(sub(vn, v)));
    let (n1, n2) = ((t1.1, -t1.0), (t2.1, -t2.0));
    let e = add(v, mul(n1, a));
    let s = add(v, mul(n2, b));
    let turn = cross(t1, t2);
    if dist(e, s) < 1e-12 {
        return Some(vec![e]);
    }
    if turn.abs() < 1e-12 {
        return if kind == K::Mitre {
            Some(vec![e, s])
        } else {
            None
        };
    }
    let x = isect(e, t1, s, t2)?;
    if kind == K::Mitre || turn < 0.0 {
        let reach = 4.0 * a.max(b);
        return Some(if turn > 0.0 && dist(x, v) > reach {
            vec![e, s]
        } else {
            vec![x]
        });
    }
    match kind {
        K::Mitre => unreachable!(),
        K::Stepped => Some(vec![e, add(add(v, mul(n1, a)), mul(n2, b)), s]),
        K::HemNext => {
            // Pli = bord suivant : miroir de la droite décalée précédente par rapport au pli.
            let f = isect(e, t1, v, t2)?;
            let (m0, m1) = (reflect(e, v, t2), reflect(add(e, t1), v, t2));
            Some(vec![f, isect(m0, sub(m1, m0), s, t2)?])
        }
        K::HemPrev => {
            let f = isect(s, t2, v, t1)?;
            let (m0, m1) = (reflect(s, v, t1), reflect(add(s, t2), v, t1));
            Some(vec![isect(m0, sub(m1, m0), e, t1)?, f])
        }
    }
}

fn shoelace(v: &[V2]) -> f64 {
    let n = v.len();
    (0..n).map(|i| cross(v[i], v[(i + 1) % n])).sum::<f64>() / 2.0
}

fn perimeter(v: &[V2]) -> f64 {
    let n = v.len();
    (0..n).map(|i| dist(v[i], v[(i + 1) % n])).sum()
}

fn crossing(a: V2, b: V2, c: V2, d: V2) -> bool {
    let (u, w) = (sub(b, a), sub(d, c));
    let den = cross(u, w);
    if den.abs() < 1e-12 {
        return false;
    }
    let t = cross(sub(c, a), w) / den;
    let s = cross(sub(c, a), u) / den;
    let e = 1e-7;
    t > e && t < 1.0 - e && s > e && s < 1.0 - e
}

fn is_simple(v: &[V2]) -> bool {
    let n = v.len();
    for i in 0..n {
        for j in (i + 2)..n {
            if i == 0 && j == n - 1 {
                continue;
            }
            if crossing(v[i], v[(i + 1) % n], v[j], v[(j + 1) % n]) {
                return false;
            }
        }
    }
    true
}

/// Retire doublons (1e-5 mm) et points alignés d'une liste cyclique.
fn clean(pts: &[V2]) -> Vec<V2> {
    let mut v: Vec<V2> = Vec::new();
    for p in pts {
        if v.last().is_none_or(|l| dist(*l, *p) > 1e-5) {
            v.push(*p);
        }
    }
    while v.len() > 1 && dist(v[0], v[v.len() - 1]) <= 1e-5 {
        v.pop();
    }
    loop {
        let n = v.len();
        let drop = (0..n).find(|&i| {
            let (a, b, c) = (v[(i + n - 1) % n], v[i], v[(i + 1) % n]);
            let (u, w) = (sub(b, a), sub(c, b));
            cross(u, w).abs() <= 1e-9 * dist(a, b) * dist(b, c) && dot(u, w) > 0.0
        });
        match drop {
            Some(i) if n > 3 => {
                v.remove(i);
            }
            _ => break,
        }
    }
    v
}

fn same_cycle(a: &[V2], b: &[V2], tol: f64) -> bool {
    a.len() == b.len()
        && (0..a.len()).any(|s| (0..a.len()).all(|i| dist(a[(i + s) % a.len()], b[i]) <= tol))
}

/// Polygone attendu, ou `None` s'il n'est pas valide (bord qui ne progresse plus, croisement,
/// aire plus petite que celle de la couture).
fn expected(v: &[V2], m: &[f64], kinds: &[K]) -> Option<Vec<V2>> {
    let n = v.len();
    let corners: Vec<Vec<V2>> = (0..n)
        .map(|j| {
            corner_pts(
                v[(j + n - 1) % n],
                v[j],
                v[(j + 1) % n],
                m[(j + n - 1) % n],
                m[j],
                kinds[j],
            )
        })
        .collect::<Option<_>>()?;
    for i in 0..n {
        let t = unit(sub(v[(i + 1) % n], v[i]));
        let last = *corners[i].last()?;
        let first = corners[(i + 1) % n][0];
        if dot(sub(first, last), t) <= 1e-6 {
            return None;
        }
    }
    let poly = clean(&corners.concat());
    if poly.len() < 3 || !is_simple(&poly) || shoelace(&poly) <= shoelace(v) {
        return None;
    }
    Some(poly)
}

// ------------------------------------------------------------------ compteurs

#[derive(Default)]
struct Counters {
    sequences: usize,
    polygons: usize,
    quarters: usize,
    circles: usize,
    hems: usize,
    folds: usize,
    hem_next_judged: usize,
    hem_prev_judged: usize,
    adjacent_hems_judged: usize,
    hem_at_reentrant: usize,
    explicit_over_hem_judged: usize,
    oracle_valid: usize,
    oracle_area_checked: usize,
    oracle_invalid_failed: usize,
    oracle_invalid_ok: usize,
    engine_limit: usize,
    notches_checked: usize,
    plan_step: usize,
    plan_count: usize,
    plan_default: usize,
    no_notch_cases: usize,
    notch_fail_cases: usize,
    max_notches: usize,
    refused: usize,
    undo_redo: usize,
    json: usize,
}

fn refuse_check(doc: &mut Document, piece: PieceId, seam: SeamAllowance, what: &str) {
    let before = persist::to_json(doc.project()).unwrap();
    let r = doc.execute(Command::SetSeamAllowance {
        piece,
        seam: Some(seam),
    });
    assert!(r.is_err(), "{what} : commande invalide acceptée");
    assert_eq!(
        before,
        persist::to_json(doc.project()).unwrap(),
        "{what} : trace d'un refus"
    );
    assert!(!doc.can_undo(), "{what} : un refus crée une étape");
}

fn hem(entity: EntityId, height_mm: f64) -> HemAllowance {
    HemAllowance { entity, height_mm }
}

fn fold(entity: EntityId, value_mm: f64) -> FoldAllowance {
    FoldAllowance {
        entity,
        value_mm,
        notch_step_mm: None,
        notch_count: None,
    }
}

/// Fin de séquence commune : annuler (octets d'avant, une seule étape), rétablir (même contour),
/// aller-retour JSON exact.
fn undo_redo_json(doc: &mut Document, piece: PieceId, base: &str, what: &str, c: &mut Counters) {
    let after = persist::to_json(doc.project()).unwrap();
    let cut_after = doc.project().piece_cut_contour(piece).ok();
    assert!(doc.undo(), "{what}");
    assert_eq!(
        base,
        persist::to_json(doc.project()).unwrap(),
        "{what} : annuler"
    );
    assert!(!doc.can_undo(), "{what} : une seule étape");
    assert!(doc.redo(), "{what}");
    assert_eq!(after, persist::to_json(doc.project()).unwrap(), "{what}");
    assert_eq!(
        doc.project().piece_cut_contour(piece).ok(),
        cut_after,
        "{what} : rétablir donne un autre contour"
    );
    c.undo_redo += 1;
    let back = persist::from_json(&after).unwrap();
    assert_eq!(&back, doc.project(), "{what} : JSON");
    assert_eq!(
        persist::to_json(&back).unwrap(),
        after,
        "{what} : JSON octets"
    );
    c.json += 1;
}

// ------------------------------------------------------------------ polygones

#[derive(Clone, Copy, Debug, PartialEq)]
enum Set {
    Default,
    Over(f64),
    Hem(f64),
    Fold(f64),
}

fn run_polygon(rng: &mut Rng, index: usize, c: &mut Counters, what: &str) -> String {
    let n0 = 3 + rng.below(6);
    let mut v: Vec<V2> = (0..n0)
        .map(|i| {
            let a = std::f64::consts::TAU * (i as f64 + rng.range(-0.25, 0.25)) / n0 as f64;
            let r = rng.range(40.0, 100.0);
            (
                2.0 * (r * a.cos() / 2.0).round(),
                2.0 * (r * a.sin() / 2.0).round(),
            )
        })
        .collect();
    // Un sommet lisse : milieu exact (coordonnées paires) d'un bord, une séquence sur trois.
    if index % 3 == 0 {
        let k = rng.below(n0);
        let (a, b) = (v[k], v[(k + 1) % n0]);
        v.insert(k + 1, ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0));
    }
    let n = v.len();
    let mut p = Project::new("Adverse ourlets");
    let sheet = p.sheets[0].id;
    let edges: Vec<EntityId> = (0..n)
        .map(|i| {
            p.apply(Command::AddSegment {
                sheet,
                from: PointRef::New(PointMm::new(v[i].0, v[i].1)),
                to: PointRef::New(PointMm::new(v[(i + 1) % n].0, v[(i + 1) % n].1)),
            })
            .unwrap()
            .entities[0]
        })
        .collect();
    let piece = p
        .apply(Command::ExtractPiece {
            sheet,
            source: PieceSource::Paths {
                entities: edges.clone(),
            },
            name: "Étoile".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    let mut doc = Document::new(p);
    let base = persist::to_json(doc.project()).unwrap();
    let turn = |j: usize| cross(sub(v[j], v[(j + n - 1) % n]), sub(v[(j + 1) % n], v[j]));

    // Commandes invalides : refus sans trace.
    let seam_of =
        |default_mm: f64, hems: Vec<HemAllowance>, folds: Vec<FoldAllowance>| SeamAllowance {
            default_mm,
            hems,
            folds,
            ..Default::default()
        };
    let stray = EntityId(9_000_000 + index as u64);
    let bad_height = [
        -rng.range(0.001, 5.0),
        1000.0 + rng.range(0.001, 5.0),
        f64::NAN,
        f64::INFINITY,
    ];
    let e0 = edges[0];
    let mut bads = vec![
        seam_of(2.0, vec![hem(e0, bad_height[0])], vec![]),
        seam_of(2.0, vec![hem(e0, bad_height[1])], vec![]),
        seam_of(2.0, vec![hem(e0, bad_height[2])], vec![]),
        seam_of(2.0, vec![hem(e0, bad_height[3])], vec![]),
        seam_of(2.0, vec![], vec![fold(e0, bad_height[0])]),
        seam_of(2.0, vec![], vec![fold(e0, bad_height[1])]),
        seam_of(2.0, vec![hem(e0, 5.0), hem(e0, 6.0)], vec![]),
        seam_of(2.0, vec![], vec![fold(e0, 5.0), fold(e0, 6.0)]),
        seam_of(2.0, vec![hem(e0, 5.0)], vec![fold(e0, 6.0)]),
        seam_of(2.0, vec![hem(stray, 5.0)], vec![]),
        seam_of(2.0, vec![], vec![fold(stray, 5.0)]),
    ];
    bads.push(SeamAllowance {
        edges: vec![EdgeAllowance {
            entity: e0,
            start_mm: 1.0,
            end_mm: 1.0,
        }],
        ..seam_of(2.0, vec![hem(e0, 5.0)], vec![])
    });
    for bad_fold in [
        FoldAllowance {
            notch_count: Some(0),
            ..fold(e0, 5.0)
        },
        FoldAllowance {
            notch_count: Some(1001),
            ..fold(e0, 5.0)
        },
        FoldAllowance {
            notch_step_mm: Some(0.5),
            ..fold(e0, 5.0)
        },
        FoldAllowance {
            notch_step_mm: Some(f64::NAN),
            ..fold(e0, 5.0)
        },
        // Pas et nombre ensemble ; crans sur un bord droit.
        FoldAllowance {
            notch_step_mm: Some(10.0),
            notch_count: Some(3),
            ..fold(e0, 5.0)
        },
        FoldAllowance {
            notch_count: Some(3),
            ..fold(e0, 5.0)
        },
        FoldAllowance {
            notch_step_mm: Some(10.0),
            ..fold(e0, 5.0)
        },
    ] {
        bads.push(seam_of(2.0, vec![], vec![bad_fold]));
    }
    for bad in bads {
        refuse_check(&mut doc, piece, bad, what);
        c.refused += 1;
    }

    // Configuration valide : un réglage par bord.
    let default_mm = rng.quarter(3.0, 12.0);
    let mut set = vec![Set::Default; n];
    for s in set.iter_mut() {
        let roll = rng.below(100);
        *s = if roll < 32 {
            Set::Hem(rng.quarter(2.0, 30.0))
        } else if roll < 52 {
            Set::Fold(rng.quarter(2.0, 15.0))
        } else if roll < 64 {
            Set::Over(rng.quarter(3.0, 12.0))
        } else {
            Set::Default
        };
    }
    if index % 4 != 3 && !set.iter().any(|s| matches!(s, Set::Hem(_))) {
        let k = rng.below(n);
        set[k] = Set::Hem(rng.quarter(2.0, 30.0));
    }
    let m: Vec<f64> = set
        .iter()
        .map(|s| match s {
            Set::Default => default_mm,
            Set::Over(x) | Set::Hem(x) | Set::Fold(x) => *x,
        })
        .collect();
    let is_hem = |i: usize| matches!(set[i], Set::Hem(_));
    let mut kinds = vec![K::Mitre; n];
    for i in (0..n).filter(|&i| is_hem(i)) {
        if turn((i + 1) % n) > 0.0 {
            kinds[(i + 1) % n] = K::HemPrev;
        }
    }
    for i in (0..n).filter(|&i| is_hem(i)) {
        if turn(i) > 0.0 {
            kinds[i] = K::HemNext;
        }
    }
    // Type de coin choisi à la main sur un sommet saillant : il l'emporte sur le pli de l'ourlet.
    let mut corners = Vec::new();
    let mut explicit_on_hem = false;
    if rng.chance(30) {
        let j = rng.below(n);
        if turn(j) > 0.0 {
            explicit_on_hem = matches!(kinds[j], K::HemNext | K::HemPrev);
            kinds[j] = K::Stepped;
            corners.push(CornerAllowance {
                entity: edges[j],
                corner: CornerType::Stepped {},
            });
        }
    }
    let seam = SeamAllowance {
        default_mm,
        edges: (0..n)
            .filter_map(|i| match set[i] {
                Set::Over(x) => Some(EdgeAllowance {
                    entity: edges[i],
                    start_mm: x,
                    end_mm: x,
                }),
                _ => None,
            })
            .collect(),
        hems: (0..n)
            .filter_map(|i| match set[i] {
                Set::Hem(h) => Some(hem(edges[i], h)),
                _ => None,
            })
            .collect(),
        folds: (0..n)
            .filter_map(|i| match set[i] {
                Set::Fold(f) => Some(fold(edges[i], f)),
                _ => None,
            })
            .collect(),
        corners,
        max_corner_mm: None,
    };
    doc.execute(Command::SetSeamAllowance {
        piece,
        seam: Some(seam),
    })
    .unwrap_or_else(|e| panic!("{what} : configuration valide refusée : {e}"));
    c.hems += set.iter().filter(|s| matches!(s, Set::Hem(_))).count();
    c.folds += set.iter().filter(|s| matches!(s, Set::Fold(_))).count();
    // Aucun cran de rempli sur un bord droit.
    assert!(
        doc.project().piece_fold_notches(piece).unwrap().is_empty(),
        "{what} : crans sur un bord droit"
    );

    // Jugement par l'oracle.
    let project = doc.project();
    let lib = project.piece_cut_contour(piece);
    let want_poly = expected(&v, &m, &kinds);
    let has_issue = project
        .piece_issues(piece)
        .iter()
        .any(|i| matches!(i, PieceIssue::CutContourFailed { .. }));
    let mut fingerprint = String::new();
    let lib_pts = |c: &CutContour| -> Vec<V2> {
        c.traces
            .iter()
            .map(|t| match t {
                Trace::Line(a, _) => (a.x, a.y),
                other => panic!("segment attendu, reçu {other:?}"),
            })
            .collect()
    };
    match (&lib, &want_poly) {
        (Ok(cut), Some(want)) => {
            assert!(!has_issue, "{what} : contrôle alors que le calcul réussit");
            let got = clean(&lib_pts(cut));
            assert!(
                same_cycle(&got, want, 1e-6),
                "{what} : contour calculé {got:?} ≠ attendu {want:?}\nsommets {v:?}, marges {m:?}, \
                 réglages {set:?}, types {kinds:?}"
            );
            // Aire et périmètre : lacet et distances sur le polygone de l'oracle.
            assert!(
                (cut.area_mm2 - shoelace(want).abs()).abs() <= 1e-6,
                "{what} : aire {} ≠ {}",
                cut.area_mm2,
                shoelace(want)
            );
            assert!(
                (cut.perimeter_mm - perimeter(want)).abs() <= 1e-6,
                "{what} : périmètre"
            );
            c.oracle_valid += 1;
            c.oracle_area_checked += 1;
            c.hem_next_judged += kinds.iter().filter(|k| **k == K::HemNext).count();
            c.hem_prev_judged += kinds.iter().filter(|k| **k == K::HemPrev).count();
            c.adjacent_hems_judged += (0..n).filter(|&i| is_hem(i) && is_hem((i + 1) % n)).count();
            c.hem_at_reentrant += (0..n)
                .filter(|&i| (is_hem(i) && turn(i) < 0.0) || (is_hem(i) && turn((i + 1) % n) < 0.0))
                .count();
            c.explicit_over_hem_judged += usize::from(explicit_on_hem);
            fingerprint.push_str(&format!("{got:?}|"));
        }
        (Err(e), None) => {
            assert!(has_issue, "{what} : échec sans contrôle : {e}");
            assert!(!e.to_string().is_empty());
            c.oracle_invalid_failed += 1;
            fingerprint.push_str(&format!("{e}|"));
        }
        // Limite connue du moteur de coins : un pli qui raccourcirait un bord décalé de plus que
        // sa longueur est refusé avec une explication (« trop court »), alors que le polygone de
        // l'oracle, simple mais très pointu, existe. Seul ce motif est toléré, et compté.
        (Err(e), Some(_)) if e.to_string().contains("trop court") => {
            assert!(has_issue, "{what} : échec sans contrôle : {e}");
            c.engine_limit += 1;
            fingerprint.push_str(&format!("{e}|"));
        }
        (Err(e), Some(want)) => panic!(
            "{what} : calcul impossible ({e}) alors que l'oracle attend {want:?}\nsommets {v:?}, \
             marges {m:?}, réglages {set:?}, types {kinds:?}"
        ),
        (Ok(cut), None) => {
            assert!(!has_issue, "{what}");
            let got = clean(&lib_pts(cut));
            assert!(is_simple(&got), "{what} : contour croisé {got:?}");
            c.oracle_invalid_ok += 1;
            fingerprint.push_str(&format!("{got:?}|"));
        }
    }
    undo_redo_json(&mut doc, piece, &base, what, c);
    fingerprint
}

// ------------------------------------------------------------------ plan de crans (oracle)

#[derive(Clone, Copy, Debug)]
enum Plan {
    Default,
    Step(f64),
    Count(u32),
}

/// Nombre d'intervalles attendu, ou `None` si le calcul est impossible (plus de 1000 crans).
/// Écrit à part de `pf_geometry::hem` : les intervalles sont ⌈L / pas⌉ (au moins 1), ou n + 1.
fn intervals(length: f64, plan: Plan) -> Option<usize> {
    let m = match plan {
        Plan::Default => (length / 25.0).ceil().max(1.0) as usize,
        Plan::Step(s) => (length / s).ceil().max(1.0) as usize,
        Plan::Count(n) => n as usize + 1,
    };
    (m - 1 <= 1000).then_some(m)
}

fn fold_with(entity: EntityId, value_mm: f64, plan: Plan) -> FoldAllowance {
    FoldAllowance {
        entity,
        value_mm,
        notch_step_mm: match plan {
            Plan::Step(s) => Some(s),
            _ => None,
        },
        notch_count: match plan {
            Plan::Count(n) => Some(n),
            _ => None,
        },
    }
}

/// Pas dont `L / pas` n'est pas (presque) un entier : l'arrondi par excès y serait ambigu.
fn safe_step(length: f64, mut s: f64) -> f64 {
    while ((length / s) - (length / s).round()).abs() < 1e-5 {
        s += 0.013;
    }
    s
}

fn plan_counter(plan: Plan, c: &mut Counters) {
    match plan {
        Plan::Default => c.plan_default += 1,
        Plan::Step(_) => c.plan_step += 1,
        Plan::Count(_) => c.plan_count += 1,
    }
}

// ------------------------------------------------------------------ quartiers de disque

fn run_quarter(rng: &mut Rng, index: usize, c: &mut Counters, what: &str) -> String {
    let sub_kind = (index / 5) % 6;
    let big = sub_kind == 3;
    let radius = if big {
        2.0 * (rng.range(650.0, 1400.0) / 2.0).round()
    } else {
        2.0 * (rng.range(20.0, 200.0) / 2.0).round()
    };
    let cw = rng.chance(50);
    let value = rng.quarter(1.0, 20.0);
    let d = if rng.chance(50) {
        0.0
    } else {
        rng.quarter(2.0, 10.0)
    };
    let length = radius * PI / 2.0;
    let plan = match sub_kind {
        0 => Plan::Default,
        1 => Plan::Step(safe_step(length, rng.quarter(3.0, 80.0))),
        2 => Plan::Count(1 + rng.below(40) as u32),
        3 => Plan::Step(safe_step(length, rng.quarter(1.0, 1.5))),
        4 => Plan::Step(safe_step(
            length,
            (length + rng.range(1.0, 50.0)).min(1000.0),
        )),
        _ => Plan::Count([1000u32, 999, 500][rng.below(3)]),
    };
    let mut p = Project::new("Quartier");
    let sheet = p.sheets[0].id;
    let seg = |p: &mut Project, a: V2, b: V2| {
        p.apply(Command::AddSegment {
            sheet,
            from: PointRef::New(PointMm::new(a.0, a.1)),
            to: PointRef::New(PointMm::new(b.0, b.1)),
        })
        .unwrap()
        .entities[0]
    };
    let bottom = seg(&mut p, (0.0, 0.0), (radius, 0.0));
    let left = seg(&mut p, (0.0, radius), (0.0, 0.0));
    let find = |p: &Project, x: f64, y: f64| {
        let s = p.sheet(sheet).unwrap();
        s.points
            .iter()
            .find(|k| {
                let q = s.position(k.id).unwrap();
                (q.x - x).abs() < 1e-9 && (q.y - y).abs() < 1e-9
            })
            .unwrap()
            .id
    };
    let (pa, pb) = (find(&p, radius, 0.0), find(&p, 0.0, radius));
    let h = radius * 0.5f64.sqrt();
    let (start, end) = if cw { (pb, pa) } else { (pa, pb) };
    let arc = p
        .apply(Command::AddArcThreePoints {
            sheet,
            start: PointRef::Existing(start),
            through: PointRef::New(PointMm::new(h, h)),
            end: PointRef::Existing(end),
        })
        .unwrap()
        .entities[0];
    let piece = p
        .apply(Command::ExtractPiece {
            sheet,
            source: PieceSource::Paths {
                entities: vec![bottom, arc, left],
            },
            name: "Quartier".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    let mut doc = Document::new(p);
    let base = persist::to_json(doc.project()).unwrap();

    // Invalides : ourlet sur l'arc, crans sur un bord droit, pas / nombre hors bornes.
    for bad in [
        SeamAllowance {
            default_mm: 2.0,
            hems: vec![hem(arc, 5.0)],
            ..Default::default()
        },
        SeamAllowance {
            default_mm: 2.0,
            folds: vec![FoldAllowance {
                notch_count: Some(2),
                ..fold(bottom, 4.0)
            }],
            ..Default::default()
        },
        SeamAllowance {
            default_mm: 2.0,
            folds: vec![FoldAllowance {
                notch_step_mm: Some(0.9),
                ..fold(arc, 4.0)
            }],
            ..Default::default()
        },
        SeamAllowance {
            default_mm: 2.0,
            folds: vec![FoldAllowance {
                notch_count: Some(1001),
                ..fold(arc, 4.0)
            }],
            ..Default::default()
        },
    ] {
        refuse_check(&mut doc, piece, bad, what);
        c.refused += 1;
    }

    doc.execute(Command::SetSeamAllowance {
        piece,
        seam: Some(SeamAllowance {
            default_mm: d,
            folds: vec![fold_with(arc, value, plan)],
            ..Default::default()
        }),
    })
    .unwrap_or_else(|e| panic!("{what} : configuration valide refusée : {e}"));
    c.folds += 1;
    plan_counter(plan, c);
    let project = doc.project();
    let mut fingerprint = String::new();

    // Crans : ceux de l'oracle (trigonométrie), ou l'échec annoncé.
    let notch_issue = project
        .piece_issues(piece)
        .iter()
        .any(|i| matches!(i, PieceIssue::FoldNotchesFailed { .. }));
    match (project.piece_fold_notches(piece), intervals(length, plan)) {
        (Ok(got), Some(m)) => {
            assert!(
                !notch_issue,
                "{what} : contrôle alors que le calcul réussit"
            );
            assert_eq!(got.len(), m - 1, "{what} : nombre de crans ({plan:?})");
            for (i, n) in got.iter().enumerate() {
                let k = i + 1;
                let deg = 90.0 * k as f64 / m as f64;
                let (sn, cs) = deg.to_radians().sin_cos();
                assert_eq!(n.index, k, "{what}");
                assert_eq!(n.entity, arc, "{what}");
                assert!(
                    (n.distance_mm - length * k as f64 / m as f64).abs() <= 1e-6,
                    "{what} : distance du cran {k}"
                );
                assert!(
                    (n.point.x - radius * cs).abs() <= 1e-6
                        && (n.point.y - radius * sn).abs() <= 1e-6,
                    "{what} : point du cran {k} {:?} ≠ ({}, {})",
                    n.point,
                    radius * cs,
                    radius * sn
                );
                assert!(
                    (n.normal.x - cs).abs() <= 1e-6 && (n.normal.y - sn).abs() <= 1e-6,
                    "{what} : normale du cran {k}"
                );
                c.notches_checked += 1;
            }
            c.max_notches = c.max_notches.max(got.len());
            c.no_notch_cases += usize::from(got.is_empty());
            fingerprint.push_str(&format!("{}|", got.len()));
        }
        (Err(e), None) => {
            assert!(notch_issue, "{what} : échec sans contrôle : {e}");
            assert!(e.to_string().contains("1000"), "{what} : « {e} »");
            c.notch_fail_cases += 1;
            fingerprint.push_str(&format!("{e}|"));
        }
        (Ok(got), None) => panic!("{what} : {} crans alors que l'oracle refuse", got.len()),
        (Err(e), Some(m)) => panic!("{what} : calcul impossible ({e}), l'oracle attend {m}"),
    }

    // Contour de coupe : arc de rayon R + f ; aire et périmètre exacts (voir le module).
    let rf = radius + value;
    let want_area = PI * rf * rf / 4.0 + d * rf + d * (rf + d);
    let want_per = 2.0 * rf + 4.0 * d + PI * rf / 2.0;
    match project.piece_cut_contour(piece) {
        Ok(cut) => {
            assert!(
                (cut.area_mm2 - want_area).abs() <= 1e-6 * want_area.max(1.0),
                "{what} : aire {} ≠ {want_area}",
                cut.area_mm2
            );
            assert!(
                (cut.perimeter_mm - want_per).abs() <= 1e-6 * want_per.max(1.0),
                "{what} : périmètre {} ≠ {want_per}",
                cut.perimeter_mm
            );
            let arcs: Vec<_> = cut
                .traces
                .iter()
                .filter_map(|t| {
                    if let Trace::Arc(a) = t {
                        Some(*a)
                    } else {
                        None
                    }
                })
                .collect();
            assert_eq!(arcs.len(), 1, "{what}");
            assert!((arcs[0].radius_mm - rf).abs() <= 1e-9 * rf, "{what}");
            c.oracle_valid += 1;
            c.oracle_area_checked += 1;
            fingerprint.push_str(&format!("{:.6}|", cut.area_mm2));
        }
        Err(e) => {
            panic!("{what} : contour de coupe impossible ({e}) alors que l'oracle le calcule")
        }
    }
    undo_redo_json(&mut doc, piece, &base, what, c);
    fingerprint
}

// ------------------------------------------------------------------ disques entiers

fn run_circle(rng: &mut Rng, index: usize, c: &mut Counters, what: &str) -> String {
    let radius = 2.0 * (rng.range(8.0, 120.0) / 2.0).round();
    let (cx, cy) = (
        2.0 * (rng.range(-50.0, 50.0) / 2.0).round(),
        2.0 * (rng.range(-50.0, 50.0) / 2.0).round(),
    );
    let value = rng.quarter(1.0, 15.0);
    let length = 2.0 * PI * radius;
    let plan = match index % 3 {
        0 => Plan::Default,
        1 => Plan::Step(safe_step(length, rng.quarter(2.0, 60.0))),
        _ => Plan::Count(1 + rng.below(30) as u32),
    };
    let mut p = Project::new("Disque");
    let sheet = p.sheets[0].id;
    let circle = p
        .apply(Command::AddCircle {
            sheet,
            center: PointRef::New(PointMm::new(cx, cy)),
            radius_mm: radius,
        })
        .unwrap()
        .entities[0];
    let piece = p
        .apply(Command::ExtractPiece {
            sheet,
            source: PieceSource::Paths {
                entities: vec![circle],
            },
            name: "Disque".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    let mut doc = Document::new(p);
    let base = persist::to_json(doc.project()).unwrap();
    refuse_check(
        &mut doc,
        piece,
        SeamAllowance {
            default_mm: 2.0,
            hems: vec![hem(circle, 5.0)],
            ..Default::default()
        },
        what,
    );
    c.refused += 1;
    doc.execute(Command::SetSeamAllowance {
        piece,
        seam: Some(SeamAllowance {
            default_mm: 0.0,
            folds: vec![fold_with(circle, value, plan)],
            ..Default::default()
        }),
    })
    .unwrap_or_else(|e| panic!("{what} : configuration valide refusée : {e}"));
    c.folds += 1;
    plan_counter(plan, c);
    let project = doc.project();
    let m = intervals(length, plan).expect("disque : moins de 1000 crans");
    let got = project
        .piece_fold_notches(piece)
        .unwrap_or_else(|e| panic!("{what} : {e}"));
    assert_eq!(got.len(), m - 1, "{what} : nombre de crans ({plan:?})");
    let step_angle = 2.0 * PI / m as f64;
    for (i, n) in got.iter().enumerate() {
        let (dx, dy) = (n.point.x - cx, n.point.y - cy);
        assert!((dx.hypot(dy) - radius).abs() <= 1e-6, "{what} : rayon");
        assert_eq!(n.index, i + 1);
        assert!(
            (n.distance_mm - length * (i + 1) as f64 / m as f64).abs() <= 1e-6,
            "{what} : distance"
        );
        // Normale = direction radiale sortante.
        assert!(
            (n.normal.x - dx / radius).abs() <= 1e-6 && (n.normal.y - dy / radius).abs() <= 1e-6,
            "{what} : normale"
        );
        if i > 0 {
            let (px, py) = (got[i - 1].point.x - cx, got[i - 1].point.y - cy);
            let gap = (dy.atan2(dx) - py.atan2(px)).rem_euclid(2.0 * PI);
            assert!(
                (gap - step_angle).abs() <= 1e-9,
                "{what} : écart angulaire {gap} ≠ {step_angle}"
            );
        }
        c.notches_checked += 1;
    }
    c.max_notches = c.max_notches.max(got.len());
    c.no_notch_cases += usize::from(got.is_empty());
    let cut = project
        .piece_cut_contour(piece)
        .unwrap_or_else(|e| panic!("{what} : {e}"));
    let rf = radius + value;
    assert!(
        (cut.area_mm2 - PI * rf * rf).abs() <= 1e-6 * rf * rf,
        "{what}"
    );
    assert!(
        (cut.perimeter_mm - 2.0 * PI * rf).abs() <= 1e-6 * rf,
        "{what}"
    );
    c.oracle_valid += 1;
    c.oracle_area_checked += 1;
    let fingerprint = format!("{}|{:.6}|", got.len(), cut.area_mm2);
    undo_redo_json(&mut doc, piece, &base, what, c);
    fingerprint
}

// ------------------------------------------------------------------ campagne

fn run(seed: u64, index: usize, c: &mut Counters) -> String {
    let mut rng = Rng(seed);
    let what = format!("séquence {index} (graine {seed})");
    match index % 5 {
        0..=2 => {
            c.polygons += 1;
            run_polygon(&mut rng, index, c, &what)
        }
        3 => {
            c.quarters += 1;
            run_quarter(&mut rng, index, c, &what)
        }
        _ => {
            c.circles += 1;
            run_circle(&mut rng, index, c, &what)
        }
    }
}

#[test]
fn campagne_adverse_ourlets_et_rempliés_180_sequences() {
    let mut c = Counters::default();
    let mut prints = Vec::new();
    for index in 0..180usize {
        let seed = 0x000E_5CA1_0053_0000_u64 ^ (index as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        c.sequences += 1;
        match catch_unwind(AssertUnwindSafe(|| run(seed, index, &mut c))) {
            Ok(f) => prints.push((seed, index, f)),
            Err(e) => {
                let msg = e
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
                    .unwrap_or_default();
                panic!("séquence {index} (graine {seed}) : {msg}");
            }
        }
    }
    // Déterminisme : des séquences rejouées donnent la même empreinte.
    for (seed, index, f) in prints.iter().step_by(7) {
        let mut sink = Counters::default();
        assert_eq!(
            &run(*seed, *index, &mut sink),
            f,
            "séquence {index} non déterministe"
        );
    }
    println!(
        "séquences {} (polygones {}, quartiers {}, disques {}) ; ourlets {} ; rempliés {} ; \
         oracle valide {} ; aires {} ; échecs signalés {} ; oracle muet {} ; limites du moteur {} ; plis suivants {} ; \
         plis précédents {} ; ourlets voisins {} ; ourlets en coin rentrant {} ; coin choisi sur \
         un pli {} ; crans jugés {} (max {}) ; sans cran {} ; calculs impossibles {} ; pas {} \
         nombres {} défauts {} ; refus {} ; annuler {} ; json {}",
        c.sequences,
        c.polygons,
        c.quarters,
        c.circles,
        c.hems,
        c.folds,
        c.oracle_valid,
        c.oracle_area_checked,
        c.oracle_invalid_failed,
        c.oracle_invalid_ok,
        c.engine_limit,
        c.hem_next_judged,
        c.hem_prev_judged,
        c.adjacent_hems_judged,
        c.hem_at_reentrant,
        c.explicit_over_hem_judged,
        c.notches_checked,
        c.max_notches,
        c.no_notch_cases,
        c.notch_fail_cases,
        c.plan_step,
        c.plan_count,
        c.plan_default,
        c.refused,
        c.undo_redo,
        c.json
    );
    // Les contrôles forts ont bien été exécutés (l'oracle ne passe pas à vide).
    assert_eq!(c.sequences, 180);
    assert_eq!((c.polygons, c.quarters, c.circles), (108, 36, 36));
    assert!(c.oracle_valid >= 100, "contours jugés : {}", c.oracle_valid);
    assert!(
        c.oracle_area_checked >= 100,
        "aires jugées : {}",
        c.oracle_area_checked
    );
    assert!(c.hems >= 150, "ourlets : {}", c.hems);
    assert!(
        c.hem_next_judged >= 60,
        "plis suivants jugés : {}",
        c.hem_next_judged
    );
    assert!(
        c.hem_prev_judged >= 60,
        "plis précédents jugés : {}",
        c.hem_prev_judged
    );
    assert!(
        c.adjacent_hems_judged >= 10,
        "ourlets voisins jugés : {}",
        c.adjacent_hems_judged
    );
    assert!(
        c.hem_at_reentrant >= 5,
        "ourlets en coin rentrant jugés : {}",
        c.hem_at_reentrant
    );
    assert!(
        c.explicit_over_hem_judged >= 5,
        "coin choisi sur un pli jugé : {}",
        c.explicit_over_hem_judged
    );
    assert!(c.folds >= 100, "rempliés : {}", c.folds);
    assert!(
        c.notches_checked >= 500,
        "crans jugés : {}",
        c.notches_checked
    );
    assert!(
        c.max_notches >= 500,
        "plus grand nombre de crans jugés : {}",
        c.max_notches
    );
    assert!(
        c.no_notch_cases >= 3,
        "bords sans cran : {}",
        c.no_notch_cases
    );
    assert!(
        c.notch_fail_cases >= 4,
        "calculs impossibles : {}",
        c.notch_fail_cases
    );
    assert!(c.plan_step >= 20 && c.plan_count >= 20 && c.plan_default >= 10);
    assert!(c.refused >= 180 * 4, "refus : {}", c.refused);
    assert!(
        c.oracle_invalid_failed + c.oracle_invalid_ok >= 5,
        "séquences que l'oracle ne juge pas : {}",
        c.oracle_invalid_failed + c.oracle_invalid_ok
    );
    // La limite connue du moteur reste rare (jamais la règle).
    assert!(
        c.engine_limit <= 15,
        "limites du moteur : {}",
        c.engine_limit
    );
    assert!(c.undo_redo == 180 && c.json == 180);
}
