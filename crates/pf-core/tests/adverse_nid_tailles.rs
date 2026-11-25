//! US-09.4a — campagne adverse « nid de tailles » : 160 séquences à graine fixe de 14 pas sur un
//! quadrilatère à coins entiers (4 segments, 3 à 6 tailles, base variable) : écarts posés,
//! remplacés, effacés, changements de base, lectures du nid avec un point d'empilement tiré au
//! hasard (coin, point construit sur un côté, origine de la feuille), point et gamme inconnus.
//!
//! **Oracle indépendant** : le modèle (`Model`) ne garde que la **position absolue de chaque coin
//! dans chaque taille** (base + écart, jamais relue du cœur). Il en déduit à la main la position
//! du point d'empilement dans chaque taille (coin : lui-même ; point construit : coin de départ +
//! 50 mm le long du côté, par `hypot` ; origine : (0 ; 0)), la translation
//! `position(base) − position(taille)` (écrite ici, nulle pour la base et pour l'origine) et les
//! deux extrémités translatées de chaque côté.
//!
//! Invariants à chaque pas : lecture sans effet (octets identiques) ; tailles dans l'ordre de la
//! gamme ; chaque contour = oracle ; la taille de base est la géométrie écrite ; le point
//! d'empilement coïncide dans toutes les tailles ; un changement de base ne change aucune position
//! absolue ; point et gamme inconnus refusés.
//!
//! Mutations vérifiées à la main : voir `docs/JOURNAL.md` (tranche 99).

use pf_core::persist;
use pf_core::*;
use pf_geometry::PointMm;

const SEQUENCES: u64 = 160;
const STEPS: usize = 14;
const GHOST: u64 = 987_654;
const POOL: [&str; 6] = ["XXS", "XS", "S", "M", "L", "XL"];
const ALONG: f64 = 50.0;
const MIN_SIDE: f64 = 60.0;

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

type V = (f64, f64);

fn bytes(p: &Project) -> String {
    persist::to_json(p).expect("projet valide")
}

#[derive(Clone, Copy, Debug)]
enum Anch {
    Origin,
    Corner(usize),
    /// Point construit sur le côté `k`, à 50 mm du coin `k`.
    OnSide(usize),
}

#[derive(Clone, Debug)]
struct Model {
    /// Position absolue du coin `c` dans la taille `s` : `pos[s][c]`.
    pos: Vec<[V; 4]>,
    base: usize,
}

fn dist(a: V, b: V) -> f64 {
    (b.0 - a.0).hypot(b.1 - a.1)
}

impl Model {
    fn shape_ok(&self) -> bool {
        self.pos
            .iter()
            .all(|q| (0..4).all(|i| dist(q[i], q[(i + 1) % 4]) >= MIN_SIDE))
    }

    fn set(&self, s: usize, c: usize, d: V) -> Option<Model> {
        if s == self.base {
            return None;
        }
        let mut m = self.clone();
        let b = m.pos[m.base][c];
        m.pos[s][c] = (b.0 + d.0, b.1 + d.1);
        Some(m)
    }

    /// Position du point d'empilement dans la taille `s`.
    fn anchor_at(&self, s: usize, a: Anch) -> V {
        match a {
            Anch::Origin => (0.0, 0.0),
            Anch::Corner(c) => self.pos[s][c],
            Anch::OnSide(k) => {
                let (p, q) = (self.pos[s][k], self.pos[s][(k + 1) % 4]);
                let len = dist(p, q);
                (
                    p.0 + ALONG * (q.0 - p.0) / len,
                    p.1 + ALONG * (q.1 - p.1) / len,
                )
            }
        }
    }

    /// Translation de la taille `s` : base − taille (nulle pour l'origine).
    fn shift(&self, s: usize, a: Anch) -> V {
        if matches!(a, Anch::Origin) || s == self.base {
            return (0.0, 0.0);
        }
        let (b, t) = (self.anchor_at(self.base, a), self.anchor_at(s, a));
        (b.0 - t.0, b.1 - t.1)
    }
}

struct Ids {
    sheet: SheetId,
    range: SizeRangeId,
    sizes: Vec<SizeId>,
    corners: [PointId; 4],
    sides: [EntityId; 4],
    on_side: [PointId; 4],
}

fn build(rng: &mut Rng) -> (Project, Ids, Model) {
    let n = 3 + rng.below(4) as usize;
    let base_idx = rng.below(n as u64) as usize;
    let corners_pos: [V; 4] = loop {
        let c: Vec<V> = (0..4)
            .map(|_| (rng.range(0, 500) as f64, rng.range(0, 500) as f64))
            .collect();
        let q = [c[0], c[1], c[2], c[3]];
        if (0..4).all(|i| dist(q[i], q[(i + 1) % 4]) >= 100.0) {
            break q;
        }
    };
    let mut p = Project::new("Nid adverse");
    let sheet = p.sheets[0].id;
    p.apply(Command::AddSizeRange {
        name: "Gamme".into(),
        kind: SizeKind::Alphanumeric,
        labels: POOL[..n].iter().map(|s| s.to_string()).collect(),
        base: base_idx,
    })
    .unwrap();
    let range = p.sizes[0].id;
    let sizes: Vec<SizeId> = p.sizes[0].sizes.iter().map(|s| s.id).collect();
    let pm = |v: V| PointMm::new(v.0, v.1);
    let mut sides = Vec::new();
    for i in 0..4 {
        let e = p
            .apply(Command::AddSegment {
                sheet,
                from: PointRef::New(pm(corners_pos[i])),
                to: PointRef::New(pm(corners_pos[(i + 1) % 4])),
            })
            .unwrap()
            .entities[0];
        sides.push(e);
    }
    let sh = p.sheet(sheet).unwrap();
    let corner_ids: Vec<PointId> = corners_pos
        .iter()
        .map(|c| {
            sh.points
                .iter()
                .find(|q| {
                    let pos = sh.position(q.id).unwrap();
                    (pos.x - c.0).abs() < 1e-9 && (pos.y - c.1).abs() < 1e-9
                })
                .expect("coin")
                .id
        })
        .collect();
    let mut on_side = Vec::new();
    for e in &sides {
        let id = p
            .apply(Command::AddPointOnSegment {
                sheet,
                segment: *e,
                distance_mm: ALONG,
                origin: SegmentEnd::Start,
            })
            .unwrap()
            .points[0];
        on_side.push(id);
    }
    let model = Model {
        pos: vec![corners_pos; n],
        base: base_idx,
    };
    assert!(model.shape_ok());
    let ids = Ids {
        sheet,
        range,
        sizes,
        corners: [corner_ids[0], corner_ids[1], corner_ids[2], corner_ids[3]],
        sides: [sides[0], sides[1], sides[2], sides[3]],
        on_side: [on_side[0], on_side[1], on_side[2], on_side[3]],
    };
    (p, ids, model)
}

fn to_anchor(ids: &Ids, a: Anch) -> NestAnchor {
    match a {
        Anch::Origin => NestAnchor::Origin,
        Anch::Corner(c) => NestAnchor::Point(ids.corners[c]),
        Anch::OnSide(k) => NestAnchor::Point(ids.on_side[k]),
    }
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-6 + 1e-9 * a.abs().max(b.abs())
}

fn check_nest(p: &Project, ids: &Ids, m: &Model, a: Anch, ctx: &str) {
    let before = bytes(p);
    let nest = p
        .size_nest(ids.sheet, ids.range, to_anchor(ids, a))
        .unwrap_or_else(|e| panic!("{ctx} : {e}"));
    assert_eq!(bytes(p), before, "{ctx} : la lecture a écrit");
    assert_eq!(nest.sizes.len(), ids.sizes.len(), "{ctx}");
    assert_eq!(nest.base, ids.sizes[m.base], "{ctx} : base");
    let base_anchor = m.anchor_at(m.base, a);
    assert!(
        close(nest.anchor_position.x, base_anchor.0)
            && close(nest.anchor_position.y, base_anchor.1),
        "{ctx} : position du point d'empilement en base"
    );
    for (s, ns) in nest.sizes.iter().enumerate() {
        assert_eq!(ns.size, ids.sizes[s], "{ctx} : ordre des tailles");
        assert_eq!(ns.is_base, s == m.base, "{ctx} : base");
        let sh = m.shift(s, a);
        assert!(
            close(ns.shift.0, sh.0) && close(ns.shift.1, sh.1),
            "{ctx} : taille {s} décalage {:?} ≠ {sh:?}",
            ns.shift
        );
        assert!(ns.issues.is_empty(), "{ctx} : {:?}", ns.issues);
        assert_eq!(ns.outlines.len(), 4, "{ctx} : taille {s}");
        for k in 0..4 {
            let o = ns
                .outlines
                .iter()
                .find(|o| matches!(o.kind, NestOutlineKind::Path { entity, .. } if entity == ids.sides[k]))
                .unwrap_or_else(|| panic!("{ctx} : côté {k} absent"));
            let (u, v) = (m.pos[s][k], m.pos[s][(k + 1) % 4]);
            let want = [(u.0 + sh.0, u.1 + sh.1), (v.0 + sh.0, v.1 + sh.1)];
            assert_eq!(o.points.len(), 2, "{ctx}");
            for (g, w) in o.points.iter().zip(want) {
                assert!(
                    close(g.x, w.0) && close(g.y, w.1),
                    "{ctx} : taille {s} côté {k} ({} ; {}) ≠ {w:?}",
                    g.x,
                    g.y
                );
            }
            // La taille de base est la géométrie écrite, à l'octet près des flottants.
            if s == m.base {
                assert_eq!((o.points[0].x, o.points[0].y), u, "{ctx} : base écrite");
                assert_eq!((o.points[1].x, o.points[1].y), v, "{ctx} : base écrite");
            }
        }
        // Le point d'empilement coïncide dans toutes les tailles (après translation).
        if !matches!(a, Anch::Origin) {
            let at = m.anchor_at(s, a);
            assert!(
                close(at.0 + sh.0, base_anchor.0) && close(at.1 + sh.1, base_anchor.1),
                "{ctx} : le point ne coïncide pas dans la taille {s}"
            );
        }
    }
}

fn random_anchor(rng: &mut Rng) -> Anch {
    match rng.below(6) {
        0 => Anch::Origin,
        1..=3 => Anch::Corner(rng.below(4) as usize),
        _ => Anch::OnSide(rng.below(4) as usize),
    }
}

fn run(seq: u64) -> (usize, usize) {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15 ^ (seq + 1).wrapping_mul(0xD1B5_4A32_D192_ED03));
    for _ in 0..4 {
        rng.step();
    }
    let (mut p, ids, mut m) = build(&mut rng);
    let (mut applied, mut translated) = (0, 0);
    let a0 = random_anchor(&mut rng);
    check_nest(&p, &ids, &m, a0, &format!("seq {seq} départ"));
    for step in 0..STEPS {
        let ctx = format!("seq {seq} pas {step}");
        match rng.below(10) {
            // Point, gamme et feuille inconnus : refusés, octets identiques.
            0 => {
                let before = bytes(&p);
                assert_eq!(
                    p.size_nest(ids.sheet, ids.range, NestAnchor::Point(PointId(GHOST))),
                    Err(NestError::UnknownPoint(PointId(GHOST))),
                    "{ctx}"
                );
                assert_eq!(
                    p.size_nest(ids.sheet, SizeRangeId(GHOST), NestAnchor::Origin),
                    Err(NestError::UnknownRange(SizeRangeId(GHOST))),
                    "{ctx}"
                );
                assert_eq!(
                    p.size_nest(SheetId(GHOST), ids.range, NestAnchor::Origin),
                    Err(NestError::UnknownSheet(SheetId(GHOST))),
                    "{ctx}"
                );
                assert_eq!(bytes(&p), before, "{ctx}");
            }
            // Changement de base : positions absolues inchangées, nid relu sur la nouvelle base.
            1 | 2 => {
                let target = rng.below(m.pos.len() as u64) as usize;
                p.apply(Command::SetBaseSize {
                    size: ids.sizes[target],
                })
                .unwrap_or_else(|e| panic!("{ctx} : changement de base refusé : {e}"));
                m.base = target;
            }
            // Écart posé, remplacé ou effacé (0, 0).
            _ => {
                let s = rng.below(m.pos.len() as u64) as usize;
                let c = rng.below(4) as usize;
                let d: V = if rng.below(5) == 0 {
                    (0.0, 0.0)
                } else {
                    (rng.range(-25, 25) as f64, rng.range(-25, 25) as f64)
                };
                if let Some(next) = m.set(s, c, d).filter(Model::shape_ok) {
                    p.apply(Command::SetPointGrading {
                        sheet: ids.sheet,
                        point: ids.corners[c],
                        size: ids.sizes[s],
                        dx_mm: d.0,
                        dy_mm: d.1,
                    })
                    .unwrap_or_else(|e| panic!("{ctx} : {e}"));
                    m = next;
                    applied += 1;
                }
            }
        }
        // Après chaque pas, le nid est relu avec un point d'empilement tiré au hasard.
        let a = random_anchor(&mut rng);
        check_nest(&p, &ids, &m, a, &format!("{ctx} {a:?}"));
        if !matches!(a, Anch::Origin) && (0..m.pos.len()).any(|s| m.shift(s, a) != (0.0, 0.0)) {
            translated += 1;
        }
    }
    (applied, translated)
}

#[test]
fn campagne_adverse_nid_de_tailles() {
    let (mut applied, mut translated) = (0, 0);
    for seq in 0..SEQUENCES {
        let (a, b) = run(seq);
        applied += a;
        translated += b;
    }
    // La campagne exerce réellement la gradation et des translations non nulles.
    assert!(applied > 600, "écarts appliqués : {applied}");
    assert!(
        translated > 600,
        "nids réellement translatés : {translated}"
    );
}
