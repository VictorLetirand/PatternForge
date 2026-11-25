//! US-09.5a — campagne adverse « géométrie dérivée gradée » : 160 séquences à graine fixe de
//! 14 pas sur une pièce à quatre côtés (rectangle légèrement déformé, cinq tailles XS S M L XL,
//! base variable) : écarts posés, remplacés, effacés ; marge de couture ; crans à distance fixe et
//! ancrés en proportion ; changements de base.
//!
//! **Oracle indépendant** : le modèle (`Model`) est réécrit ici. Il stocke la position de base des
//! quatre coins et leurs écarts, et recalcule **à la main** (sans appeler le cœur) : les sommets de
//! chaque taille, les longueurs de côté, le périmètre, l'aire (formule du lacet), les sommets du
//! contour de coupe par la formule du biseau `P + m (n₁ + n₂) / (1 + n₁·n₂)` (normales sortantes
//! unitaires), son périmètre et son aire, la distance coupe ↔ couture, la position de chaque cran
//! (distance fixe ou proportion de la longueur du côté). Le changement de base y est formulé par
//! l'invariant « les positions évaluées ne changent pas ».
//!
//! Invariants vérifiés à chaque pas : projet valide ; refus = octets identiques ; la lecture par
//! taille n'écrit rien (valeur et octets) ; positions, longueurs, périmètre, aire, fermeture,
//! sommets de coupe, périmètre et aire de coupe, distance coupe ↔ couture = marge (1e-6), crans ;
//! `None` = base ; un changement de base laisse **toutes** les données dérivées de toutes les
//! tailles identiques ; une taille inconnue est refusée.
//!
//! Mutations vérifiées à la main : voir `docs/JOURNAL.md` (tranche 97).

use pf_core::persist;
use pf_core::*;
use pf_geometry::{distance_mm, PointMm, Side, Trace};
use std::collections::BTreeMap;

const SEQUENCES: u64 = 160;
const STEPS: usize = 14;
const GHOST: u64 = 987_654;
const TOL: f64 = 1e-6;
const LABELS: [&str; 5] = ["XS", "S", "M", "L", "XL"];

struct Rng(u64);

impl Rng {
    fn step(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.step() % n
    }
    fn range(&mut self, lo: i64, hi: i64) -> i64 {
        lo + self.below((hi - lo + 1) as u64) as i64
    }
}

// ---------------------------------------------------------------------------------------------
// Oracle
// ---------------------------------------------------------------------------------------------

type V = (f64, f64);

#[derive(Clone, Copy, Debug, PartialEq)]
enum NotchKind {
    /// Distance fixe depuis le départ du côté.
    Fixed(f64),
    /// Proportion de la longueur du côté.
    Ratio(f64),
}

#[derive(Clone, Debug, PartialEq)]
struct Model {
    /// Position de base des coins A, B, C, D (sens anti-horaire).
    base: [V; 4],
    /// Écart cumulé (taille, coin) → (dx, dy), jamais nul, jamais sur la taille de base.
    delta: BTreeMap<(usize, usize), V>,
    base_size: usize,
    margin: f64,
    /// (côté, nature) ; le côté `i` va du coin `i` au coin `(i + 1) % 4`.
    notches: Vec<(usize, NotchKind)>,
}

impl Model {
    fn eval(&self, size: usize, corner: usize) -> V {
        let (dx, dy) = self
            .delta
            .get(&(size, corner))
            .copied()
            .unwrap_or((0.0, 0.0));
        (self.base[corner].0 + dx, self.base[corner].1 + dy)
    }

    fn quad(&self, size: usize) -> [V; 4] {
        [0, 1, 2, 3].map(|c| self.eval(size, c))
    }

    /// Forme acceptable pour l'oracle : anti-horaire, angles intérieurs entre 55° et 140°, côtés
    /// d'au moins 80 mm (pas de repli en biseau du moteur de marges).
    fn shape_ok(&self) -> bool {
        (0..5).all(|s| {
            let q = self.quad(s);
            (0..4).all(|i| {
                let (p, c, n) = (q[(i + 3) % 4], q[i], q[(i + 1) % 4]);
                let (a, b) = ((p.0 - c.0, p.1 - c.1), (n.0 - c.0, n.1 - c.1));
                let (la, lb) = (a.0.hypot(a.1), b.0.hypot(b.1));
                let cross = b.0 * a.1 - b.1 * a.0; // > 0 en anti-horaire (b suit a)
                let angle = (cross).atan2(a.0 * b.0 + a.1 * b.1).to_degrees();
                la >= 80.0 && lb >= 80.0 && (55.0..=140.0).contains(&angle)
            })
        })
    }

    /// Réglage d'une taille : le modèle après la commande (`None` = refus prévu).
    fn set(&self, size: usize, corner: usize, d: V) -> Option<Model> {
        if size == self.base_size {
            return None;
        }
        let mut m = self.clone();
        if d == (0.0, 0.0) {
            m.delta.remove(&(size, corner));
        } else {
            m.delta.insert((size, corner), d);
        }
        Some(m)
    }

    /// Changement de base formulé par l'invariant : positions évaluées inchangées. Refusé (`None`)
    /// quand un cran à distance fixe sortirait du côté de la nouvelle base : la géométrie écrite
    /// serait invalide (constat de cette campagne).
    fn rebase(&self, new_base: usize) -> Option<Model> {
        let q = self.quad(new_base);
        if self.notches.iter().any(
            |(e, k)| matches!(k, NotchKind::Fixed(d) if *d > len(q[*e], q[(*e + 1) % 4]) + 1e-9),
        ) {
            return None;
        }
        let mut m = self.clone();
        m.base = [0, 1, 2, 3].map(|c| self.eval(new_base, c));
        m.delta.clear();
        for s in (0..5).filter(|s| *s != new_base) {
            for c in 0..4 {
                let (a, b) = (self.eval(s, c), self.eval(new_base, c));
                let d = (a.0 - b.0, a.1 - b.1);
                if d != (0.0, 0.0) {
                    m.delta.insert((s, c), d);
                }
            }
        }
        m.base_size = new_base;
        Some(m)
    }
}

fn len(a: V, b: V) -> f64 {
    (b.0 - a.0).hypot(b.1 - a.1)
}

fn shoelace(q: &[V]) -> f64 {
    let n = q.len();
    (0..n)
        .map(|i| q[i].0 * q[(i + 1) % n].1 - q[(i + 1) % n].0 * q[i].1)
        .sum::<f64>()
        / 2.0
}

fn perimeter(q: &[V]) -> f64 {
    (0..q.len()).map(|i| len(q[i], q[(i + 1) % q.len()])).sum()
}

/// Sommets du contour de coupe d'un polygone convexe anti-horaire (biseau, marge `m`).
fn offset(q: &[V; 4], m: f64) -> [V; 4] {
    let normal = |a: V, b: V| {
        let l = len(a, b);
        ((b.1 - a.1) / l, -(b.0 - a.0) / l)
    };
    [0, 1, 2, 3].map(|i| {
        let (n1, n2) = (normal(q[(i + 3) % 4], q[i]), normal(q[i], q[(i + 1) % 4]));
        let k = m / (1.0 + n1.0 * n2.0 + n1.1 * n2.1);
        (q[i].0 + k * (n1.0 + n2.0), q[i].1 + k * (n1.1 + n2.1))
    })
}

// ---------------------------------------------------------------------------------------------
// Projet réel
// ---------------------------------------------------------------------------------------------

struct World {
    p: Project,
    sheet: SheetId,
    piece: PieceId,
    sizes: [SizeId; 5],
    corners: [PointId; 4],
    edges: [EntityId; 4],
    notch_ids: Vec<NotchId>,
}

fn bytes(p: &Project) -> String {
    persist::to_json(p).expect("projet valide")
}

fn build(m: &Model) -> World {
    let mut p = Project::new("Adverse dérivée");
    let sheet = p.sheets[0].id;
    p.apply(Command::AddSizeRange {
        name: "Femme".into(),
        kind: SizeKind::Alphanumeric,
        labels: LABELS.iter().map(|s| s.to_string()).collect(),
        base: 2,
    })
    .unwrap();
    let sizes = LABELS.map(|l| p.sizes[0].sizes.iter().find(|s| s.label == l).unwrap().id);
    let c: [PointMm; 4] = m.base.map(|(x, y)| PointMm::new(x, y));
    let edges: Vec<EntityId> = (0..4)
        .map(|i| {
            p.apply(Command::AddSegment {
                sheet,
                from: PointRef::New(c[i]),
                to: PointRef::New(c[(i + 1) % 4]),
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
            name: "Pièce".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    let corners = [0, 1, 2, 3].map(|i| {
        let s = p.sheet(sheet).unwrap();
        s.points
            .iter()
            .find(|q| distance_mm(s.position(q.id).unwrap(), c[i]) < 1e-9)
            .unwrap()
            .id
    });
    World {
        p,
        sheet,
        piece,
        sizes,
        corners,
        edges: [edges[0], edges[1], edges[2], edges[3]],
        notch_ids: Vec::new(),
    }
}

// ---------------------------------------------------------------------------------------------
// Vérification d'un état
// ---------------------------------------------------------------------------------------------

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() <= TOL
}

fn nearp(a: PointMm, b: V) -> bool {
    near(a.x, b.0) && near(a.y, b.1)
}

fn check(w: &World, m: &Model, ctx: &str) -> usize {
    let mut beyond_count = 0;
    let (value, text) = (w.p.clone(), bytes(&w.p));
    let mut all = Vec::new();
    for s in 0..5 {
        let d =
            w.p.derived_for_size(w.sheet, Some(w.sizes[s]))
                .unwrap_or_else(|e| panic!("{ctx} taille {s} : {e}"));
        let q = m.quad(s);
        // Positions évaluées.
        for (i, c) in w.corners.iter().enumerate() {
            let got = d.points.iter().find(|(id, _)| id == c).expect("coin").1;
            assert!(
                nearp(got, q[i]),
                "{ctx} taille {s} coin {i} : {got:?} ≠ {:?}",
                q[i]
            );
        }
        // Longueurs de côté.
        for i in 0..4 {
            let got = d
                .path_lengths
                .iter()
                .find(|(e, _)| *e == w.edges[i])
                .unwrap();
            let got = got.1.clone().unwrap();
            assert!(
                near(got, len(q[i], q[(i + 1) % 4])),
                "{ctx} taille {s} côté {i}"
            );
        }
        let pd = d.pieces.iter().find(|x| x.piece == w.piece).unwrap();
        // Périmètre, aire, fermeture.
        let c = pd.contour.clone().unwrap();
        assert!(
            near(c.perimeter_mm, perimeter(&q)),
            "{ctx} taille {s} périmètre"
        );
        assert!(
            near(c.area_mm2, shoelace(&q).abs()),
            "{ctx} taille {s} aire"
        );
        assert!(c.closed && !c.self_intersecting, "{ctx} taille {s}");
        // Contour de coupe : sommets du biseau, périmètre, aire, distance coupe ↔ couture.
        let cut = pd
            .cut
            .clone()
            .unwrap_or_else(|e| panic!("{ctx} taille {s} coupe : {e}"));
        let want = offset(&q, m.margin);
        assert!(
            near(cut.perimeter_mm, perimeter(&want)),
            "{ctx} taille {s} périmètre de coupe"
        );
        assert!(
            near(cut.area_mm2, shoelace(&want).abs()),
            "{ctx} taille {s} aire de coupe"
        );
        assert_eq!(cut.traces.len(), 4, "{ctx} taille {s}");
        let starts: Vec<PointMm> = cut
            .traces
            .iter()
            .map(|t| match t {
                Trace::Line(a, _) => *a,
                other => panic!("{ctx} : segment attendu, reçu {other:?}"),
            })
            .collect();
        assert!(
            (0..4).any(|sh| (0..4).all(|i| nearp(starts[(i + sh) % 4], want[i]))),
            "{ctx} taille {s} sommets de coupe {starts:?} ≠ {want:?}"
        );
        for t in &cut.traces {
            let Trace::Line(a, b) = t else { unreachable!() };
            // Le bord de couture parallèle de même sens : distance de la droite = marge.
            let k = (0..4)
                .find(|&k| {
                    let (u, v) = (
                        (b.x - a.x, b.y - a.y),
                        (q[(k + 1) % 4].0 - q[k].0, q[(k + 1) % 4].1 - q[k].1),
                    );
                    (u.0 * v.1 - u.1 * v.0).abs() <= 1e-9 * u.0.hypot(u.1) * v.0.hypot(v.1)
                        && u.0 * v.0 + u.1 * v.1 > 0.0
                })
                .unwrap_or_else(|| panic!("{ctx} taille {s} : aucun bord parallèle"));
            let (p0, p1) = (q[k], q[(k + 1) % 4]);
            let l = len(p0, p1);
            // Distance signée à droite du sens (extérieur d'un contour anti-horaire).
            let dist =
                |pt: PointMm| ((pt.x - p0.0) * (p1.1 - p0.1) - (pt.y - p0.1) * (p1.0 - p0.0)) / l;
            assert!(
                near(dist(*a), m.margin) && near(dist(*b), m.margin),
                "{ctx} taille {s} marge"
            );
        }
        // Crans.
        assert_eq!(d.notches.len(), m.notches.len(), "{ctx} taille {s}");
        for (nd, (edge, kind)) in d.notches.iter().zip(&m.notches) {
            let (a, b) = (q[*edge], q[(edge + 1) % 4]);
            let l = len(a, b);
            let dist = match kind {
                NotchKind::Fixed(x) => *x,
                NotchKind::Ratio(r) => r * l,
            };
            assert!(
                near(nd.distance_mm.clone().unwrap(), dist),
                "{ctx} taille {s} distance de cran"
            );
            let at = dist.min(l);
            let foot = (a.0 + (b.0 - a.0) / l * at, a.1 + (b.1 - a.1) / l * at);
            let (_, marks) = nd.marks.clone().unwrap();
            assert!(nearp(marks[0].0, foot), "{ctx} taille {s} pied de cran");
        }
        // Seule limite attendue : un cran à distance fixe plus loin que la fin de son côté.
        let beyond: Vec<NotchId> = m
            .notches
            .iter()
            .zip(&w.notch_ids)
            .filter(|((e, k), _)| {
                matches!(k, NotchKind::Fixed(x) if *x > len(q[*e], q[(*e + 1) % 4]) + 1e-9)
            })
            .map(|(_, id)| *id)
            .collect();
        let got: Vec<NotchId> = d
            .limits
            .iter()
            .map(|l| match l {
                DerivedLimit::NotchBeyondEdge { notch } => *notch,
                other => panic!("{ctx} taille {s} : limite inattendue {other:?}"),
            })
            .collect();
        assert_eq!(got, beyond, "{ctx} taille {s} : crans hors du côté");
        beyond_count += got.len();
        assert!(d.issues.is_empty(), "{ctx} taille {s}");
        all.push(d);
    }
    // `None` = base ; lecture sans effet ; taille inconnue refusée.
    let none = w.p.derived_for_size(w.sheet, None).unwrap();
    let mut as_base = all[m.base_size].clone();
    as_base.size = None;
    assert_eq!(none, as_base, "{ctx} : None ≠ base");
    assert_eq!(w.p, value, "{ctx} : la lecture a modifié le projet");
    assert_eq!(bytes(&w.p), text, "{ctx}");
    assert_eq!(
        w.p.derived_for_size(w.sheet, Some(SizeId(GHOST))),
        Err(DerivedError::UnknownSize(SizeId(GHOST)))
    );
    assert!(w.p.validate().is_ok(), "{ctx}");
    beyond_count
}

fn all_derived(w: &World) -> Vec<SizeDerived> {
    (0..5)
        .map(|s| w.p.derived_for_size(w.sheet, Some(w.sizes[s])).unwrap())
        .collect()
}

// ---------------------------------------------------------------------------------------------
// Campagne
// ---------------------------------------------------------------------------------------------

fn initial(rng: &mut Rng) -> Model {
    let (w, h) = (
        rng.range(30, 50) as f64 * 10.0,
        rng.range(40, 70) as f64 * 10.0,
    );
    let j = |rng: &mut Rng| rng.range(-4, 4) as f64 * 5.0;
    let base = [
        (j(rng), j(rng)),
        (w + j(rng), j(rng)),
        (w + j(rng), h + j(rng)),
        (j(rng), h + j(rng)),
    ];
    Model {
        base,
        delta: BTreeMap::new(),
        base_size: 2,
        margin: 0.0,
        notches: Vec::new(),
    }
}

/// Pas appliqués, refus prévus, pas ignorés (forme refusée par l oracle), changements de base,
/// crans posés, états avec au moins un écart.
#[derive(Default, Debug)]
struct Stats {
    applied: usize,
    refused: usize,
    skipped: usize,
    rebases: usize,
    notches: usize,
    graded: usize,
    beyond: usize,
}

fn run(seed: u64, stats: &mut Stats) {
    let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let mut m = initial(&mut rng);
    assert!(m.shape_ok(), "seed {seed} : forme initiale");
    let mut w = build(&m);
    let _ = check(&w, &m, &format!("seed {seed} initial"));
    for step in 0..STEPS {
        let ctx = format!("seed {seed} pas {step}");
        let before = (w.p.clone(), bytes(&w.p));
        let choice = rng.below(100);
        let (command, next): (Option<Command>, Option<Model>) = if choice < 50 {
            // Écart (multiple de 0,5 mm), éventuellement nul.
            let (s, c) = (rng.below(5) as usize, rng.below(4) as usize);
            let d = (
                rng.range(-120, 120) as f64 / 2.0,
                rng.range(-120, 120) as f64 / 2.0,
            );
            let cmd = Command::SetPointGrading {
                sheet: w.sheet,
                point: w.corners[c],
                size: w.sizes[s],
                dx_mm: d.0,
                dy_mm: d.1,
            };
            match m.set(s, c, d) {
                None => (Some(cmd), None),
                Some(n) if n.shape_ok() => (Some(cmd), Some(n)),
                Some(_) => (None, None),
            }
        } else if choice < 58 {
            let c = rng.below(4) as usize;
            let cmd = Command::ClearPointGrading {
                sheet: w.sheet,
                point: w.corners[c],
            };
            let mut n = m.clone();
            n.delta.retain(|(_, cc), _| *cc != c);
            (Some(cmd), Some(n))
        } else if choice < 70 {
            let mm = [0.0, 6.0, 10.0, 12.5, 20.0][rng.below(5) as usize];
            let cmd = Command::SetSeamAllowance {
                piece: w.piece,
                seam: (mm > 0.0).then(|| SeamAllowance {
                    default_mm: mm,
                    ..Default::default()
                }),
            };
            let mut n = m.clone();
            n.margin = mm;
            (Some(cmd), Some(n))
        } else if choice < 80 && m.notches.len() < 6 {
            let e = rng.below(4) as usize;
            let (a, b) = (m.eval(m.base_size, e), m.eval(m.base_size, (e + 1) % 4));
            let dist = (rng.range(1, (len(a, b) / 5.0) as i64 - 1) * 5) as f64;
            let cmd = Command::AddNotch {
                sheet: w.sheet,
                entity: w.edges[e],
                distance_mm: dist,
                side: Side::Left,
                length_mm: 6.0,
                style: NotchStyle::Single,
            };
            let mut n = m.clone();
            n.notches.push((e, NotchKind::Fixed(dist)));
            (Some(cmd), Some(n))
        } else if choice < 88 && m.notches.len() < 6 {
            let e = rng.below(4) as usize;
            let r = rng.range(1, 19) as f64 / 20.0;
            let cmd = Command::AddAnchoredNotch {
                sheet: w.sheet,
                entity: w.edges[e],
                anchor: NotchAnchor::Ratio { ratio: r },
                side: Side::Left,
                length_mm: 6.0,
                style: NotchStyle::Single,
            };
            let mut n = m.clone();
            n.notches.push((e, NotchKind::Ratio(r)));
            (Some(cmd), Some(n))
        } else {
            let nb = rng.below(5) as usize;
            (
                Some(Command::SetBaseSize { size: w.sizes[nb] }),
                m.rebase(nb),
            )
        };
        let Some(command) = command else {
            stats.skipped += 1;
            continue;
        };
        let base_before = all_derived(&w);
        let is_rebase = matches!(command, Command::SetBaseSize { .. });
        let is_notch = matches!(
            command,
            Command::AddNotch { .. } | Command::AddAnchoredNotch { .. }
        );
        match (w.p.apply(command), next) {
            (Ok(out), Some(n)) => {
                if is_notch {
                    w.notch_ids.push(out.notches[0]);
                }
                m = n;
                stats.applied += 1;
                stats.notches += usize::from(is_notch);
                stats.rebases += usize::from(is_rebase);
                stats.graded += usize::from(!m.delta.is_empty());
                stats.beyond += check(&w, &m, &ctx);
                if is_rebase {
                    // Toutes les données dérivées de toutes les tailles restent identiques.
                    assert_eq!(base_before, all_derived(&w), "{ctx} : changement de base");
                }
            }
            (Err(e), None) => {
                stats.refused += 1;
                assert_eq!(w.p, before.0, "{ctx} : refus avec trace ({e})");
                assert_eq!(bytes(&w.p), before.1, "{ctx}");
            }
            (Ok(_), None) => panic!("{ctx} : commande acceptée, refus prévu"),
            (Err(e), Some(_)) => panic!("{ctx} : refus inattendu : {e}"),
        }
    }
}

#[test]
fn campagne_adverse_geometrie_derivee_gradee() {
    let mut stats = Stats::default();
    for seed in 1..=SEQUENCES {
        run(seed, &mut stats);
    }
    // La campagne exerce vraiment ce qu annonce son en-tête.
    assert!(stats.applied >= 1200, "{stats:?}");
    assert!(
        stats.refused >= 40 && stats.rebases >= 100 && stats.notches >= 200,
        "{stats:?}"
    );
    assert!(stats.graded >= 800 && stats.beyond >= 50, "{stats:?}");
    eprintln!("{stats:?}");
}
