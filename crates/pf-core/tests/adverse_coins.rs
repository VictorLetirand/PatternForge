//! US-05.2 — campagne adverse des types de coins : 170 séquences à graine fixe (polygones étoilés
//! à 3 à 8 sommets à coordonnées paires, sommets lisses ajoutés, coins rentrants, marges par défaut
//! et par bord, types de coins aléatoires par sommet, longueur maximale de pointe, commandes
//! invalides, annuler / rétablir, aller-retour JSON).
//!
//! Oracle **indépendant** : rien de `pf_geometry::seam` ni de `piece_cut_contour` n'y sert à juger.
//! Pour chaque sommet, l'oracle reconstruit le coin par **intersections de droites** (droites
//! décalées, perpendiculaires, droite de coupe perpendiculaire à la bissectrice calculée comme somme
//! des vecteurs unitaires des deux bords, miroir par réflexion de points), puis assemble le polygone
//! attendu : points de coin de chaque sommet reliés par les droites décalées. L'implémentation, elle,
//! raisonne par paramètres le long des tangentes. Propriétés :
//!
//! 1. jamais de panique ; résultat identique à graine égale ;
//! 2. nature des sommets (`seam_vertices`) = signe du produit vectoriel, sommets lisses exclus ;
//! 3. un type de coin sur un sommet rentrant ou lisse est refusé sans trace (octets identiques) ;
//! 4. quand le polygone attendu est valide (progression avant sur chaque bord, sans croisement),
//!    le contour de coupe calculé le **reproduit** (sommets à 1e-6 mm) ; sinon un échec est
//!    signalé par `CutContourFailed`, jamais un refus de commande ;
//! 5. annuler rend les octets d'avant, rétablir rend le même contour ; JSON : aller-retour exact.

use pf_core::persist;
use pf_core::piece::PieceIssue;
use pf_core::*;
use pf_geometry::{PointMm, Trace};
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
    PerpPrev,
    PerpNext,
    Symmetric,
    Truncated(f64),
    Stepped,
    HemNext,
    HemPrev,
}

fn corner_type(k: K) -> Option<CornerType> {
    Some(match k {
        K::Mitre => return None,
        K::PerpPrev => CornerType::PerpendicularPrevious {},
        K::PerpNext => CornerType::PerpendicularNext {},
        K::Symmetric => CornerType::Symmetric {},
        K::Truncated(depth_mm) => CornerType::Truncated { depth_mm },
        K::Stepped => CornerType::Stepped {},
        K::HemNext => CornerType::HemFoldNext {},
        K::HemPrev => CornerType::HemFoldPrevious {},
    })
}

fn kind_index(k: K) -> usize {
    match k {
        K::Mitre => 0,
        K::PerpPrev => 1,
        K::PerpNext => 2,
        K::Symmetric => 3,
        K::Truncated(_) => 4,
        K::Stepped => 5,
        K::HemNext => 6,
        K::HemPrev => 7,
    }
}

/// Points du contour de coupe au sommet `v` (entre `vp` et `vn`), de la fin de la droite décalée
/// précédente au début de la suivante. `None` : construction impossible pour l'oracle.
fn corner_pts(
    vp: V2,
    v: V2,
    vn: V2,
    a: f64,
    b: f64,
    kind: K,
    limit: Option<f64>,
) -> Option<Vec<V2>> {
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
        let reach = limit.unwrap_or(4.0 * a.max(b));
        return Some(if turn > 0.0 && dist(x, v) > reach {
            vec![e, s]
        } else {
            vec![x]
        });
    }
    match kind {
        K::Mitre => unreachable!(),
        K::PerpPrev => Some(vec![isect(e, t1, s, n1)?, s]),
        K::PerpNext => Some(vec![e, isect(s, t2, e, n2)?]),
        K::Symmetric | K::Truncated(_) => {
            // Bissectrice vers l'intérieur : somme des vecteurs unitaires issus de v le long des
            // deux bords ; la direction extérieure est son opposée.
            let inward = unit(add(mul(t1, -1.0), t2));
            let w = mul(inward, -1.0);
            let k0 = match kind {
                K::Symmetric => {
                    if dot(sub(e, v), w) >= dot(sub(s, v), w) {
                        e
                    } else {
                        s
                    }
                }
                K::Truncated(depth) => sub(x, mul(w, depth)),
                _ => unreachable!(),
            };
            let dir_k = (w.1, -w.0);
            Some(vec![isect(e, t1, k0, dir_k)?, isect(s, t2, k0, dir_k)?])
        }
        K::Stepped => Some(vec![e, add(add(v, mul(n1, a)), mul(n2, b)), s]),
        K::HemNext => {
            // Pli = couture suivante : miroir de la droite décalée précédente par rapport au pli.
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
fn expected(v: &[V2], m: &[f64], kinds: &[K], limit: Option<f64>) -> Option<Vec<V2>> {
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
                limit,
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

// ------------------------------------------------------------------ scène

struct Scene {
    p: Project,
    piece: PieceId,
    /// Sommets anti-horaires (coordonnées paires) ; le bord `i` va de `v[i]` à `v[i + 1]`.
    v: Vec<V2>,
    edges: Vec<EntityId>,
    /// Triangle à pointe aiguë : 1 = demi-angle de 3° à 11° (biseau net), 2 = demi-angle de
    /// 12,3° à 13,7° (pointe entre 4 et 5 × la marge : fenêtre de la limite par défaut).
    spike: u8,
}

fn build(rng: &mut Rng, index: usize) -> Scene {
    let spike = match index % 12 {
        5 => 2,
        11 => 1,
        _ => 0,
    };
    let n = 3 + rng.below(6);
    let mut v: Vec<V2> = if spike > 0 {
        let half = if spike == 2 {
            rng.range(12.3, 13.7)
        } else {
            rng.range(3.0, 11.0)
        }
        .to_radians();
        let len = rng.range(80.0, 120.0);
        let (cx, h) = (len * half.cos(), len * half.sin());
        let ev = |x: f64| 2.0 * (x / 2.0).round();
        vec![(0.0, 0.0), (ev(cx), -ev(h)), (ev(cx), ev(h))]
    } else {
        (0..n)
            .map(|i| {
                let a = std::f64::consts::TAU * (i as f64 + rng.range(-0.25, 0.25)) / n as f64;
                let r = rng.range(40.0, 100.0);
                (
                    2.0 * (r * a.cos() / 2.0).round(),
                    2.0 * (r * a.sin() / 2.0).round(),
                )
            })
            .collect()
    };
    // Un sommet lisse : milieu exact (coordonnées paires) d'un bord, une séquence sur trois.
    if index % 3 == 0 {
        let k = rng.below(n);
        let (a, b) = (v[k], v[(k + 1) % n]);
        v.insert(k + 1, (((a.0 + b.0) / 2.0), ((a.1 + b.1) / 2.0)));
    }
    let mut p = Project::new("Adverse coins");
    let sheet = p.sheets[0].id;
    let n = v.len();
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
    Scene {
        p,
        piece,
        v,
        edges,
        spike,
    }
}

#[derive(Default)]
struct Counters {
    sequences: usize,
    by_kind: [usize; 8],
    bevel: usize,
    limit_set: usize,
    oracle_valid: usize,
    oracle_invalid_failed: usize,
    oracle_invalid_ok: usize,
    refused: usize,
    refused_bad_vertex: usize,
    vertices_checked: usize,
    undo_redo: usize,
    json: usize,
    window: usize,
    uniform_path: usize,
    per_edge_path: usize,
}

fn random_kind(rng: &mut Rng) -> K {
    match rng.below(7) {
        0 => K::PerpPrev,
        1 => K::PerpNext,
        2 => K::Symmetric,
        3 => K::Truncated(rng.range(0.0, 6.0)),
        4 => K::Stepped,
        5 => K::HemNext,
        _ => K::HemPrev,
    }
}

fn lib_vertices(project: &Project, piece: PieceId) -> Result<Vec<V2>, CoreError> {
    Ok(project
        .piece_cut_contour(piece)?
        .traces
        .iter()
        .map(|t| match t {
            Trace::Line(a, _) => (a.x, a.y),
            other => panic!("segment attendu, reçu {other:?}"),
        })
        .collect())
}

fn refuse_check(doc: &mut Document, piece: PieceId, seam: SeamAllowance, what: &str) -> String {
    let before = persist::to_json(doc.project()).unwrap();
    let r = doc.execute(Command::SetSeamAllowance {
        piece,
        seam: Some(seam),
    });
    let msg = r
        .err()
        .unwrap_or_else(|| panic!("{what} : commande invalide acceptée"))
        .to_string();
    assert_eq!(
        before,
        persist::to_json(doc.project()).unwrap(),
        "{what} : trace d'un refus"
    );
    msg
}

fn run(seed: u64, index: usize, c: &mut Counters) -> String {
    let mut rng = Rng(seed);
    let s = build(&mut rng, index);
    let what = format!("séquence {index} (graine {seed})");
    let n = s.v.len();
    let mut doc = Document::new(s.p.clone());
    let base = persist::to_json(doc.project()).unwrap();
    let mut fingerprint = String::new();

    // Nature des sommets : signe du produit vectoriel des deux bords.
    let turn = |j: usize| {
        cross(
            sub(s.v[j], s.v[(j + n - 1) % n]),
            sub(s.v[(j + 1) % n], s.v[j]),
        )
    };
    let listed = doc.project().seam_vertices(s.piece).unwrap();
    let want: Vec<(EntityId, bool)> = (0..n)
        .filter(|&j| turn(j) != 0.0)
        .map(|j| (s.edges[j], turn(j) > 0.0))
        .collect();
    let mut got: Vec<(EntityId, bool)> = listed.iter().map(|x| (x.entity, x.convex)).collect();
    let mut want_sorted = want.clone();
    got.sort_by_key(|x| x.0);
    want_sorted.sort_by_key(|x| x.0);
    assert_eq!(got, want_sorted, "{what} : sommets proposés");
    c.vertices_checked += n;

    // Commandes invalides : refus sans trace.
    let seam_of = |corners: Vec<CornerAllowance>, max: Option<f64>| SeamAllowance {
        default_mm: 5.0,
        corners,
        max_corner_mm: max,
        ..Default::default()
    };
    let corner = |entity, corner| CornerAllowance { entity, corner };
    for bad in [
        seam_of(
            vec![corner(
                EntityId(9_000_000 + index as u64),
                CornerType::Stepped {},
            )],
            None,
        ),
        seam_of(
            vec![
                corner(s.edges[0], CornerType::Stepped {}),
                corner(s.edges[0], CornerType::Symmetric {}),
            ],
            None,
        ),
        seam_of(
            vec![corner(
                s.edges[0],
                CornerType::Truncated {
                    depth_mm: -rng.range(0.001, 5.0),
                },
            )],
            None,
        ),
        seam_of(
            vec![corner(
                s.edges[0],
                CornerType::Truncated { depth_mm: f64::NAN },
            )],
            None,
        ),
        seam_of(vec![], Some(-rng.range(0.001, 5.0))),
        seam_of(vec![], Some(f64::NAN)),
        seam_of(vec![], Some(10_000.0 + rng.range(0.001, 5.0))),
    ] {
        refuse_check(&mut doc, s.piece, bad, &what);
        c.refused += 1;
    }
    // Sommet rentrant ou lisse : refus expliqué.
    for j in 0..n {
        if turn(j) <= 0.0 {
            let msg = refuse_check(
                &mut doc,
                s.piece,
                seam_of(vec![corner(s.edges[j], CornerType::Symmetric {})], None),
                &what,
            );
            let key = if turn(j) == 0.0 { "lisse" } else { "rentrant" };
            assert!(msg.contains(key), "{what} : message « {msg} »");
            c.refused_bad_vertex += 1;
        }
    }
    assert!(!doc.can_undo(), "{what} : un refus crée une étape");

    // Configuration valide : marge par défaut + surcharges uniformes par bord, types de coins sur
    // des sommets saillants, longueur maximale éventuelle.
    let default_mm = (rng.range(3.0, 12.0) * 4.0).round() / 4.0;
    let mut m = vec![default_mm; n];
    let mut edges = Vec::new();
    if rng.chance(40) {
        for (i, mi) in m.iter_mut().enumerate() {
            if rng.chance(45) {
                *mi = (rng.range(3.0, 12.0) * 4.0).round() / 4.0;
                edges.push(EdgeAllowance {
                    entity: s.edges[i],
                    start_mm: *mi,
                    end_mm: *mi,
                });
            }
        }
    }
    let mut kinds = vec![K::Mitre; n];
    let mut corners = Vec::new();
    if rng.chance(60) {
        for (j, slot) in kinds.iter_mut().enumerate() {
            if turn(j) > 0.0 && rng.chance(55) {
                let k = random_kind(&mut rng);
                *slot = k;
                corners.push(corner(s.edges[j], corner_type(k).unwrap()));
            }
        }
    }
    if s.spike == 2 {
        // Pointe entre 4 et 5 × la marge : marge uniforme, onglet partout, limite par défaut.
        edges.clear();
        m = vec![default_mm; n];
        corners.clear();
        kinds = vec![K::Mitre; n];
        if index % 24 == 17 {
            // Une séquence sur deux force le chemin « arête par arête » (un coin choisi ailleurs).
            kinds[1] = K::Stepped;
            corners.push(corner(s.edges[1], CornerType::Stepped {}));
        }
    }
    let mut limit = rng
        .chance(40)
        .then(|| (rng.range(0.0, 60.0) * 4.0).round() / 4.0);
    if s.spike == 2 {
        limit = None; // la limite par défaut (4 × la marge) est ce qui est éprouvé
    }
    let seam = SeamAllowance {
        default_mm,
        edges,
        corners: corners.clone(),
        max_corner_mm: limit,
        ..Default::default()
    };
    doc.execute(Command::SetSeamAllowance {
        piece: s.piece,
        seam: Some(seam),
    })
    .unwrap_or_else(|e| panic!("{what} : configuration valide refusée : {e}"));
    if corners.is_empty() {
        c.uniform_path += usize::from(m.iter().all(|x| *x == m[0]));
        c.per_edge_path += usize::from(!m.iter().all(|x| *x == m[0]));
    } else {
        c.per_edge_path += 1;
    }
    if limit.is_some() {
        c.limit_set += 1;
    }

    // Jugement par l'oracle.
    let project = doc.project();
    let lib = lib_vertices(project, s.piece);
    let want_poly = expected(&s.v, &m, &kinds, limit);
    let has_issue = project
        .piece_issues(s.piece)
        .iter()
        .any(|i| matches!(i, PieceIssue::CutContourFailed { .. }));
    match (&lib, &want_poly) {
        (Ok(got), Some(want)) => {
            assert!(!has_issue, "{what} : contrôle alors que le calcul réussit");
            let got = clean(got);
            assert!(
                same_cycle(&got, want, 1e-6),
                "{what} : contour calculé {got:?} ≠ attendu {want:?}\nsommets {:?}, marges {m:?}, \
                 types {kinds:?}, limite {limit:?}",
                s.v
            );
            c.oracle_valid += 1;
            c.window += usize::from(s.spike == 2);
            for k in &kinds {
                c.by_kind[kind_index(*k)] += 1;
            }
            // Biseau : le polygone attendu contient les deux bouts (E, S) d'un sommet en onglet.
            c.bevel += (0..n)
                .filter(|&j| kinds[j] == K::Mitre && turn(j) > 0.0)
                .filter(|&j| {
                    let pts = corner_pts(
                        s.v[(j + n - 1) % n],
                        s.v[j],
                        s.v[(j + 1) % n],
                        m[(j + n - 1) % n],
                        m[j],
                        K::Mitre,
                        limit,
                    );
                    pts.is_some_and(|p| p.len() == 2)
                })
                .count();
            fingerprint.push_str(&format!("{got:?}|"));
        }
        (Err(e), None) => {
            assert!(has_issue, "{what} : échec sans contrôle : {e}");
            assert!(!e.to_string().is_empty());
            c.oracle_invalid_failed += 1;
            fingerprint.push_str(&format!("{e}|"));
        }
        (Err(e), Some(want)) => panic!(
            "{what} : calcul impossible ({e}) alors que l'oracle attend {want:?}\nsommets {:?}, \
             marges {m:?}, types {kinds:?}, limite {limit:?}",
            s.v
        ),
        (Ok(got), None) => {
            // L'oracle ne sait pas juger (bord qui recule, etc.) : le contour rendu doit au moins
            // être un polygone simple, sans contrôle signalé.
            assert!(!has_issue, "{what}");
            assert!(is_simple(&clean(got)), "{what} : contour croisé {got:?}");
            c.oracle_invalid_ok += 1;
            fingerprint.push_str(&format!("{got:?}|"));
        }
    }

    // Annuler : octets d'avant ; rétablir : même contour de coupe (bit à bit).
    let after = persist::to_json(doc.project()).unwrap();
    let cut_after = doc.project().piece_cut_contour(s.piece).ok();
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
        doc.project().piece_cut_contour(s.piece).ok(),
        cut_after,
        "{what} : rétablir donne un autre contour"
    );
    c.undo_redo += 1;

    // Aller-retour JSON exact.
    let back = persist::from_json(&after).unwrap();
    assert_eq!(&back, doc.project(), "{what} : JSON");
    assert_eq!(
        persist::to_json(&back).unwrap(),
        after,
        "{what} : JSON octets"
    );
    c.json += 1;
    fingerprint
}

#[test]
fn campagne_adverse_types_de_coins_170_sequences() {
    let mut c = Counters::default();
    let mut prints = Vec::new();
    for index in 0..170usize {
        let seed = 0xC0_1A5E_0052_0000u64 ^ (index as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
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
    for (seed, index, f) in prints.iter().step_by(11) {
        let mut sink = Counters::default();
        assert_eq!(
            &run(*seed, *index, &mut sink),
            f,
            "séquence {index} non déterministe"
        );
    }
    println!(
        "séquences {} ; sommets {} ; oracle valide {} ; échecs signalés {} ; oracle muet {} ; \
         par type {:?} ; biseaux {} ; limites {} ; refus {} ; sommets refusés {} ; annuler {} ; \
         json {} ; chemin uniforme {} ; chemin par arête {}",
        c.sequences,
        c.vertices_checked,
        c.oracle_valid,
        c.oracle_invalid_failed,
        c.oracle_invalid_ok,
        c.by_kind,
        c.bevel,
        c.limit_set,
        c.refused,
        c.refused_bad_vertex,
        c.undo_redo,
        c.json,
        c.uniform_path,
        c.per_edge_path
    );
    // Les contrôles forts ont bien été exécutés (l'oracle ne passe pas à vide).
    assert_eq!(c.sequences, 170);
    assert!(c.oracle_valid >= 100, "contours jugés : {}", c.oracle_valid);
    for (i, name) in [
        "onglet",
        "perp. précédente",
        "perp. suivante",
        "symétrique",
        "tronqué",
        "en marche",
        "ourlet suivant",
        "ourlet précédent",
    ]
    .iter()
    .enumerate()
    {
        assert!(
            c.by_kind[i] >= 15,
            "type « {name} » jugé {} fois",
            c.by_kind[i]
        );
    }
    assert!(c.bevel >= 20, "biseaux jugés : {}", c.bevel);
    assert!(c.limit_set >= 40, "limites : {}", c.limit_set);
    assert!(
        c.window >= 8,
        "pointes de la fenêtre 4 à 5 × : {}",
        c.window
    );
    assert!(c.refused >= 7 * 170, "refus : {}", c.refused);
    assert!(
        c.refused_bad_vertex >= 60,
        "sommets rentrants / lisses refusés : {}",
        c.refused_bad_vertex
    );
    assert!(c.uniform_path >= 20, "chemin uniforme : {}", c.uniform_path);
    assert!(
        c.per_edge_path >= 60,
        "chemin par arête : {}",
        c.per_edge_path
    );
    assert!(c.undo_redo == 170 && c.json == 170);
}
