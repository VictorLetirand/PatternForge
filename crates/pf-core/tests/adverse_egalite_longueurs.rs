//! US-03.4 — campagne adverse « égalité de longueurs » : 160 séquences à graine fixe de 60 pas
//! (poser une égalité « tracé = autre tracé + décalage » ou une valeur, retirer, déplacer des points,
//! réduire un segment de référence, translations, suppressions, copie de feuille, annuler,
//! rétablir, relecture), sur 3 segments et 4 courbes de Bézier (poignées nulles ou non) qui
//! peuvent se référencer en chaîne.
//!
//! **Oracle indépendant** : le monde de l'oracle (points libres, segments, courbes avec poignées,
//! contraintes attendues) est tenu ici sans appeler `pf_core::length_constraint` ni
//! `pf_geometry::solve`. La longueur d'un segment est une distance, celle d'une Bézier une
//! intégration fine ré-écrite dans ce fichier (Simpson raffiné sur la norme de la dérivée, jusqu'à 1e-9 mm).
//! Après chaque commande acceptée : l'oracle calcule le monde **avant résolution** (effet direct de
//! la commande), puis exige que
//! - tout point qui n'est pas l'extrémité glissante d'une contrainte soit exactement là,
//! - toute extrémité glissante ait bougé **le long de sa direction** (produit vectoriel nul),
//! - la longueur de chaque courbe contrainte vaille **longueur du tracé de référence + décalage**
//!   (ou la valeur) à 1e-6 mm, et que la cible écrite dans le projet soit la même à 1e-6 mm.
//!
//! Autres invariants : projet valide, refus = octets identiques (attribuable aux contraintes, jamais
//! une incohérence interne), **cycle prédit par l'oracle <=> refus « circulaire »**, pas de faux
//! refus quand la cible est atteignable avec marge, étape si et seulement si l'état change, annuler
//! / rétablir exacts à l'octet, relecture identique, contraintes du projet = contraintes attendues,
//! retrait avec remarque quand un tracé de référence ou la courbe disparaît. Compteurs de
//! couverture minimaux. Mutation vérifiée à la main : voir `docs/JOURNAL.md` (tranche 91).

use pf_core::persist;
use pf_core::*;
use pf_geometry::{PointMm, Transform};

const SEQUENCES: u64 = 160;
const STEPS: usize = 60;

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
    fn chance(&mut self, percent: u64) -> bool {
        self.below(100) < percent
    }
    fn val(&mut self, lo: f64, hi: f64) -> f64 {
        let n = ((hi - lo) * 100.0) as u64;
        lo + self.below(n + 1) as f64 / 100.0
    }
    fn pick(&mut self, n: usize) -> usize {
        self.below(n as u64) as usize
    }
}

// ---------------------------------------------------------------------------------------------
// Oracle
// ---------------------------------------------------------------------------------------------

type Xy = (f64, f64);

#[derive(Clone, Debug, PartialEq)]
struct OSeg {
    id: EntityId,
    a: usize,
    b: usize,
}

#[derive(Clone, Debug, PartialEq)]
struct OCurve {
    id: EntityId,
    a: usize,
    b: usize,
    ha: Xy,
    hb: Xy,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Tgt {
    Val(f64),
    Ref { ent: EntityId, off: f64 },
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct OCon {
    curve: EntityId,
    /// Extrémité qui glisse.
    at: usize,
    ang: f64,
    tgt: Tgt,
}

#[derive(Clone, Debug, PartialEq)]
struct World {
    pts: Vec<Xy>,
    segs: Vec<OSeg>,
    curves: Vec<OCurve>,
    cons: Vec<OCon>,
}

fn dist(a: Xy, b: Xy) -> f64 {
    (a.0 - b.0).hypot(a.1 - b.1)
}

/// Longueur d'arc d'une Bézier : Simpson sur la norme de la dérivée.
fn simpson(g: &[Xy; 4], n: usize) -> f64 {
    let speed = |t: f64| {
        let u = 1.0 - t;
        let dx = 3.0
            * (u * u * (g[1].0 - g[0].0)
                + 2.0 * u * t * (g[2].0 - g[1].0)
                + t * t * (g[3].0 - g[2].0));
        let dy = 3.0
            * (u * u * (g[1].1 - g[0].1)
                + 2.0 * u * t * (g[2].1 - g[1].1)
                + t * t * (g[3].1 - g[2].1));
        (dx * dx + dy * dy).sqrt()
    };
    let h = 1.0 / n as f64;
    let mut sum = speed(0.0) + speed(1.0);
    for i in 1..n {
        sum += speed(i as f64 * h) * if i % 2 == 1 { 4.0 } else { 2.0 };
    }
    sum * h / 3.0
}

/// Longueur d'arc à ~1e-9 mm : Simpson raffiné (panneaux x4) jusqu'à ce que deux niveaux
/// consécutifs s'accordent (les courbes lisses convergent vite, un point de rebroussement non).
fn fine(g: &[Xy; 4]) -> f64 {
    let mut prev = simpson(g, 1000);
    let mut n = 1000;
    loop {
        n *= 4;
        let cur = simpson(g, n);
        if (cur - prev).abs() <= 2e-9 || n >= 256_000 {
            return cur;
        }
        prev = cur;
    }
}

impl World {
    fn curve(&self, id: EntityId) -> Option<&OCurve> {
        self.curves.iter().find(|c| c.id == id)
    }
    fn seg(&self, id: EntityId) -> Option<&OSeg> {
        self.segs.iter().find(|s| s.id == id)
    }
    fn geom(&self, c: &OCurve) -> [Xy; 4] {
        let (p0, p3) = (self.pts[c.a], self.pts[c.b]);
        [
            p0,
            (p0.0 + c.ha.0, p0.1 + c.ha.1),
            (p3.0 + c.hb.0, p3.1 + c.hb.1),
            p3,
        ]
    }
    /// Longueur d'un tracé ; `quick` = Simpson à 1000 panneaux (contrôle rapide), sinon `fine`.
    fn ent_len_with(&self, id: EntityId, quick: bool) -> Option<f64> {
        if let Some(c) = self.curve(id) {
            let g = self.geom(c);
            return Some(if quick { simpson(&g, 1000) } else { fine(&g) });
        }
        self.seg(id).map(|s| dist(self.pts[s.a], self.pts[s.b]))
    }
    fn con_of(&self, curve: EntityId) -> Option<&OCon> {
        self.cons.iter().find(|c| c.curve == curve)
    }
    fn ent_len(&self, id: EntityId) -> Option<f64> {
        self.ent_len_with(id, false)
    }
    fn target_with(&self, c: &OCon, quick: bool) -> f64 {
        match c.tgt {
            Tgt::Val(v) => v,
            Tgt::Ref { ent, off } => self.ent_len_with(ent, quick).expect("référence") + off,
        }
    }
    /// En suivant les références, la courbe `from` atteint-elle `to` ?
    fn reaches(&self, from: EntityId, to: EntityId) -> bool {
        let mut cur = from;
        for _ in 0..16 {
            if cur == to {
                return true;
            }
            match self.con_of(cur).map(|c| c.tgt) {
                Some(Tgt::Ref { ent, .. }) => cur = ent,
                _ => return false,
            }
        }
        true
    }
}

#[derive(Default, Debug)]
struct Cov {
    eq_posed: u32,
    val_posed: u32,
    released: u32,
    resolved_by_move: u32,
    chained: u32,
    cycles_refused: u32,
    pose_infeasible: u32,
    move_infeasible: u32,
    structural: u32,
    removed_with_note: u32,
    undone: u32,
    redone: u32,
    copies: u32,
    translations: u32,
    shrinks: u32,
    noops: u32,
    reloads: u32,
}

struct Ctx {
    sheet: SheetId,
    ids: Vec<PointId>,
    world: World,
    hist: Vec<World>,
    redo: Vec<World>,
    doc: Document,
    copied: bool,
}

fn bytes(p: &Project) -> String {
    persist::to_json(p).unwrap()
}

fn build(rng: &mut Rng) -> Ctx {
    let mut project = Project::new("Campagne égalité");
    let sheet = project.sheets[0].id;
    let mut ids: Vec<PointId> = Vec::new();
    let mut pts: Vec<Xy> = Vec::new();
    let fresh = |rng: &mut Rng, pts: &[Xy], near: Option<Xy>| -> Xy {
        loop {
            let q = (rng.val(0.0, 300.0), rng.val(0.0, 300.0));
            let far_from_all = pts.iter().all(|p| dist(*p, q) > 2.0);
            let far_from_mate = near.is_none_or(|n| dist(n, q) >= 40.0);
            if far_from_all && far_from_mate {
                return q;
            }
        }
    };
    let mut world = World {
        pts: Vec::new(),
        segs: Vec::new(),
        curves: Vec::new(),
        cons: Vec::new(),
    };
    for _ in 0..3 {
        let a = fresh(rng, &pts, None);
        pts.push(a);
        let b = fresh(rng, &pts, Some(a));
        pts.push(b);
        let o = project
            .apply(Command::AddSegment {
                sheet,
                from: PointRef::New(PointMm::new(a.0, a.1)),
                to: PointRef::New(PointMm::new(b.0, b.1)),
            })
            .unwrap();
        ids.extend(o.points.iter().copied());
        world.segs.push(OSeg {
            id: o.entities[0],
            a: pts.len() - 2,
            b: pts.len() - 1,
        });
    }
    for _ in 0..4 {
        let a = fresh(rng, &pts, None);
        pts.push(a);
        let b = fresh(rng, &pts, Some(a));
        pts.push(b);
        let zero = rng.chance(50);
        let ha = if zero {
            (0.0, 0.0)
        } else {
            (rng.val(-80.0, 80.0), rng.val(-80.0, 80.0))
        };
        let hb = if zero {
            (0.0, 0.0)
        } else {
            (rng.val(-80.0, 80.0), rng.val(-80.0, 80.0))
        };
        let o = project
            .apply(Command::AddCurve {
                sheet,
                start: PointRef::New(PointMm::new(a.0, a.1)),
                c1: PointMm::new(a.0 + ha.0, a.1 + ha.1),
                c2: PointMm::new(b.0 + hb.0, b.1 + hb.1),
                end: PointRef::New(PointMm::new(b.0, b.1)),
            })
            .unwrap();
        ids.extend(o.points.iter().copied());
        world.curves.push(OCurve {
            id: o.entities[0],
            a: pts.len() - 2,
            b: pts.len() - 1,
            ha,
            hb,
        });
    }
    world.pts = pts;
    assert_eq!(ids.len(), world.pts.len());
    // Des segments d'appui (hors oracle) gardent tous les points en vie quand un tracé est supprimé.
    for i in 0..ids.len() - 2 {
        project
            .apply(Command::AddSegment {
                sheet,
                from: PointRef::Existing(ids[i]),
                to: PointRef::Existing(ids[i + 2]),
            })
            .unwrap();
    }
    Ctx {
        doc: Document::new(project),
        sheet,
        ids,
        world,
        hist: Vec::new(),
        redo: Vec::new(),
        copied: false,
    }
}

// ---------------------------------------------------------------------------------------------
// Commandes de la campagne
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug)]
enum Op {
    PoseEq {
        curve: EntityId,
        at: usize,
        ang: f64,
        ent: EntityId,
        off: f64,
    },
    PoseVal {
        curve: EntityId,
        at: usize,
        ang: f64,
        target: f64,
    },
    Release(EntityId),
    Move(usize, Xy),
    /// Déplace une extrémité d'un segment pour qu'il mesure `len` (ré-échelonne autour de l'autre).
    Shrink {
        seg: EntityId,
        end_b: bool,
        to: Xy,
    },
    Translate(Vec<usize>, Xy),
    Delete(EntityId),
    Copy,
    Undo,
    Redo,
}

fn to_command(ctx: &Ctx, op: &Op) -> Option<Command> {
    let s = ctx.sheet;
    Some(match op {
        Op::PoseEq {
            curve,
            at,
            ang,
            ent,
            off,
        } => Command::SetCurveLengthConstraint {
            sheet: s,
            curve: *curve,
            spec: Some(LengthSpec {
                length_mm: 1.0,
                adjust: LengthAdjust::Slide {
                    point: ctx.ids[*at],
                    angle_deg: *ang,
                },
                reference: Some(LengthRef {
                    entity: *ent,
                    offset_mm: *off,
                }),
            }),
        },
        Op::PoseVal {
            curve,
            at,
            ang,
            target,
        } => Command::SetCurveLengthConstraint {
            sheet: s,
            curve: *curve,
            spec: Some(LengthSpec {
                length_mm: *target,
                adjust: LengthAdjust::Slide {
                    point: ctx.ids[*at],
                    angle_deg: *ang,
                },
                reference: None,
            }),
        },
        Op::Release(c) => Command::SetCurveLengthConstraint {
            sheet: s,
            curve: *c,
            spec: None,
        },
        Op::Move(i, to) => Command::MovePoint {
            sheet: s,
            point: ctx.ids[*i],
            to: PointMm::new(to.0, to.1),
        },
        Op::Shrink { seg, end_b, to } => {
            let o = ctx.world.seg(*seg).unwrap();
            Command::MovePoint {
                sheet: s,
                point: ctx.ids[if *end_b { o.b } else { o.a }],
                to: PointMm::new(to.0, to.1),
            }
        }
        Op::Translate(sel, d) => Command::Transform {
            sheet: s,
            points: sel.iter().map(|i| ctx.ids[*i]).collect(),
            transform: Transform::Translate {
                dx_mm: d.0,
                dy_mm: d.1,
            },
        },
        Op::Delete(e) => Command::Delete {
            sheet: s,
            points: vec![],
            entities: vec![*e],
        },
        Op::Copy => Command::DuplicateSheet { sheet: s },
        Op::Undo | Op::Redo => return None,
    })
}

/// Monde **avant résolution** : l'effet direct de la commande, sans résoudre.
fn direct_effect(w: &World, op: &Op) -> World {
    let mut n = w.clone();
    match op {
        Op::PoseEq {
            curve,
            at,
            ang,
            ent,
            off,
        } => {
            n.cons.retain(|c| c.curve != *curve);
            n.cons.push(OCon {
                curve: *curve,
                at: *at,
                ang: *ang,
                tgt: Tgt::Ref {
                    ent: *ent,
                    off: *off,
                },
            });
        }
        Op::PoseVal {
            curve,
            at,
            ang,
            target,
        } => {
            n.cons.retain(|c| c.curve != *curve);
            n.cons.push(OCon {
                curve: *curve,
                at: *at,
                ang: *ang,
                tgt: Tgt::Val(*target),
            });
        }
        Op::Release(c) => n.cons.retain(|k| k.curve != *c),
        Op::Move(i, to) => n.pts[*i] = *to,
        Op::Shrink { seg, end_b, to } => {
            let s = n.seg(*seg).unwrap().clone();
            n.pts[if *end_b { s.b } else { s.a }] = *to;
        }
        Op::Translate(sel, d) => {
            for i in sel {
                n.pts[*i] = (n.pts[*i].0 + d.0, n.pts[*i].1 + d.1);
            }
        }
        Op::Delete(e) => {
            n.segs.retain(|k| k.id != *e);
            n.curves.retain(|k| k.id != *e);
            n.cons
                .retain(|k| k.curve != *e && !matches!(k.tgt, Tgt::Ref { ent, .. } if ent == *e));
        }
        Op::Copy | Op::Undo | Op::Redo => {}
    }
    n
}

fn con_matches(p: &Project, ctx: &Ctx, cons: &[OCon]) -> bool {
    let sheet = p.sheet(ctx.sheet).unwrap();
    if sheet.length_constraints.len() != cons.len() {
        return false;
    }
    cons.iter().all(|c| {
        sheet.length_constraint(c.curve).is_some_and(|k| {
            let adjust_ok = matches!(k.adjust, LengthAdjust::Slide { point, angle_deg }
                if point == ctx.ids[c.at] && angle_deg == c.ang);
            let target_ok = match (c.tgt, k.reference) {
                (Tgt::Val(v), None) => k.length_mm == v,
                (Tgt::Ref { ent, off }, Some(r)) => r.entity == ent && r.offset_mm == off,
                _ => false,
            };
            adjust_ok && target_ok
        })
    })
}

/// Vérifie l'état du projet contre le monde « avant résolution » et renvoie le monde résolu.
fn settle(ctx: &Ctx, pre: &World, p: &Project, cov: &mut Cov, from_move: bool) -> World {
    let sheet = p.sheet(ctx.sheet).unwrap();
    let mut w = pre.clone();
    let mut moved = 0;
    for i in 0..pre.pts.len() {
        let q = sheet.position(ctx.ids[i]).unwrap();
        let before = pre.pts[i];
        if let Some(c) = pre.cons.iter().find(|c| c.at == i) {
            let (sn, cs) = c.ang.to_radians().sin_cos();
            let d = (q.x - before.0, q.y - before.1);
            let cross = d.0 * sn - d.1 * cs;
            assert!(
                cross.abs() <= 1e-7,
                "point {i} sorti de sa droite : {cross}"
            );
            if d.0.hypot(d.1) > 1e-9 {
                moved += 1;
            }
            w.pts[i] = (q.x, q.y);
        } else {
            assert!(
                (q.x - before.0).abs() <= 1e-9 && (q.y - before.1).abs() <= 1e-9,
                "point {i} déplacé sans raison : ({} ; {}) → ({} ; {})",
                before.0,
                before.1,
                q.x,
                q.y
            );
        }
    }
    if from_move && moved > 0 {
        cov.resolved_by_move += 1;
        if moved >= 2 {
            cov.chained += 1;
        }
    }
    w
}

/// Toutes les contraintes du monde sont satisfaites : longueur de la courbe = cible à 1e-6 mm, et
/// cible écrite dans le projet = longueur du tracé de référence + décalage.
fn check_lengths(w: &World, p: &Project, ctx: &Ctx, label: &str) {
    let sheet = p.sheet(ctx.sheet).unwrap();
    for c in &w.cons {
        let curve = w.curve(c.curve).expect("courbe contrainte");
        // Contrôle rapide d'abord (Simpson 1000 panneaux) ; à moins de 3e-7 d'écart il est
        // concluant, au-delà (point de rebroussement, écart réel) l'oracle raffiné tranche.
        let (len, target) = {
            let (l, t) = (simpson(&w.geom(curve), 1000), w.target_with(c, true));
            if (l - t).abs() <= 3e-7 {
                (l, t)
            } else {
                (fine(&w.geom(curve)), w.target_with(c, false))
            }
        };
        assert!(
            (len - target).abs() <= 1e-6,
            "{label} : longueur {len} ≠ cible {target} ({c:?})"
        );
        let written = sheet.length_constraint(c.curve).unwrap().length_mm;
        assert!(
            (written - target).abs() <= 1e-6,
            "{label} : cible écrite {written} ≠ {target} ({c:?})"
        );
    }
}

fn random_op(rng: &mut Rng, ctx: &Ctx) -> Op {
    let w = &ctx.world;
    let roll = rng.below(100);
    if w.curves.is_empty() {
        return Op::Undo;
    }
    let any_curve = |rng: &mut Rng| w.curves[rng.pick(w.curves.len())].clone();
    match roll {
        0..=33 => {
            let c = any_curve(rng);
            let at = if rng.chance(50) { c.a } else { c.b };
            let ang = (rng.val(0.0, 360.0) * 10.0).round() / 10.0;
            let natural = simpson(&w.geom(&c), 400);
            if roll >= 28 {
                let target = match rng.below(100) {
                    0..=69 => natural * (1.0 + rng.val(-0.25, 0.5)),
                    70..=89 => natural * rng.val(0.05, 0.6),
                    _ => natural * rng.val(2.0, 6.0),
                };
                return Op::PoseVal {
                    curve: c.id,
                    at,
                    ang,
                    target: (target * 1000.0).round() / 1000.0,
                };
            }
            // Référence : un segment (60 %), une autre courbe (33 %), soi-même (7 %).
            let segs: Vec<EntityId> = w.segs.iter().map(|k| k.id).collect();
            let others: Vec<EntityId> = w
                .curves
                .iter()
                .filter(|k| k.id != c.id)
                .map(|k| k.id)
                .collect();
            let ent = match rng.below(100) {
                0..=59 if !segs.is_empty() => segs[rng.pick(segs.len())],
                60..=92 if !others.is_empty() => others[rng.pick(others.len())],
                _ => c.id,
            };
            let base = w.ent_len(ent).unwrap();
            let wanted = match rng.below(100) {
                0..=69 => natural * (1.0 + rng.val(-0.25, 0.5)),
                70..=84 => natural * rng.val(0.05, 0.6),
                85..=92 => natural * rng.val(2.0, 6.0),
                _ => 0.0 - rng.val(0.0, 5.0),
            };
            Op::PoseEq {
                curve: c.id,
                at,
                ang,
                ent,
                off: ((wanted - base) * 1000.0).round() / 1000.0,
            }
        }
        34..=41 => {
            let k = if !w.cons.is_empty() && rng.chance(85) {
                w.cons[rng.pick(w.cons.len())].curve
            } else {
                any_curve(rng).id
            };
            Op::Release(k)
        }
        42..=60 => {
            let i = rng.pick(w.pts.len());
            let (x, y) = w.pts[i];
            let to = if rng.chance(70) {
                (x + rng.val(-60.0, 60.0), y + rng.val(-60.0, 60.0))
            } else {
                (rng.val(-100.0, 400.0), rng.val(-100.0, 400.0))
            };
            Op::Move(i, to)
        }
        61..=72 => {
            // Un segment cité par une égalité est ramené à une longueur choisie (parfois courte).
            let cited: Vec<EntityId> = w
                .cons
                .iter()
                .filter_map(|c| match c.tgt {
                    Tgt::Ref { ent, .. } if w.seg(ent).is_some() => Some(ent),
                    _ => None,
                })
                .collect();
            if w.segs.is_empty() {
                return Op::Move(0, w.pts[0]);
            }
            let id = if !cited.is_empty() && rng.chance(80) {
                cited[rng.pick(cited.len())]
            } else {
                w.segs[rng.pick(w.segs.len())].id
            };
            let s = w.seg(id).unwrap();
            let end_b = rng.chance(50);
            let (fixed, moved) = if end_b { (s.a, s.b) } else { (s.b, s.a) };
            let (f, m) = (w.pts[fixed], w.pts[moved]);
            let cur = dist(f, m).max(1.0);
            let len = cur * rng.val(0.05, 2.0);
            let k = len / cur;
            Op::Shrink {
                seg: id,
                end_b,
                to: (f.0 + (m.0 - f.0) * k, f.1 + (m.1 - f.1) * k),
            }
        }
        73..=79 => {
            let n = 1 + rng.pick(3);
            let mut sel: Vec<usize> = Vec::new();
            for _ in 0..n {
                let i = rng.pick(w.pts.len());
                if !sel.contains(&i) {
                    sel.push(i);
                }
            }
            Op::Translate(sel, (rng.val(-40.0, 40.0), rng.val(-40.0, 40.0)))
        }
        80..=84 => {
            let all: Vec<EntityId> = w
                .curves
                .iter()
                .map(|k| k.id)
                .chain(w.segs.iter().map(|k| k.id))
                .collect();
            Op::Delete(all[rng.pick(all.len())])
        }
        85..=86 if !ctx.copied => Op::Copy,
        87..=92 => Op::Undo,
        93..=98 => Op::Redo,
        _ => Op::Move(0, w.pts[0]),
    }
}

/// La même commande, toutes contraintes retirées, aboutit-elle ? (attribue un refus aux contraintes)
fn works_without_constraints(p: &Project, s: SheetId, cmd: &Command) -> bool {
    let mut q = p.clone();
    for sh in q.sheets.iter_mut().filter(|x| x.id == s) {
        sh.length_constraints.clear();
    }
    q.apply(cmd.clone()).is_ok()
}

/// Longueur d'un tracé d'une feuille, indépendante du code testé (positions + Simpson).
fn sheet_len(sheet: &Sheet, id: EntityId) -> f64 {
    let e = sheet.entity(id).unwrap();
    match e.kind {
        EntityKind::Segment { from, to, .. } => {
            let (a, b) = (sheet.position(from).unwrap(), sheet.position(to).unwrap());
            (a.x - b.x).hypot(a.y - b.y)
        }
        EntityKind::Curve { .. } => {
            let g = sheet.curve_geometry(id).unwrap();
            fine(&[
                (g.p0.x, g.p0.y),
                (g.c1.x, g.c1.y),
                (g.c2.x, g.c2.y),
                (g.p3.x, g.p3.y),
            ])
        }
        _ => unreachable!(),
    }
}

fn run(seed: u64, cov: &mut Cov) {
    let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let mut ctx = build(&mut rng);
    for step in 0..STEPS {
        let op = random_op(&mut rng, &ctx);
        let label = format!("séquence {seed}, pas {step}, {op:?}");
        let before_bytes = bytes(ctx.doc.project());
        let before_world = ctx.world.clone();
        let p_now;
        match op {
            Op::Undo => {
                let ok = ctx.doc.undo();
                assert_eq!(ok, !ctx.hist.is_empty(), "{label}");
                if ok {
                    ctx.redo.push(ctx.world.clone());
                    ctx.world = ctx.hist.pop().unwrap();
                    cov.undone += 1;
                }
                p_now = ctx.doc.project().clone();
            }
            Op::Redo => {
                let ok = ctx.doc.redo();
                assert_eq!(ok, !ctx.redo.is_empty(), "{label}");
                if ok {
                    ctx.hist.push(ctx.world.clone());
                    ctx.world = ctx.redo.pop().unwrap();
                    cov.redone += 1;
                }
                p_now = ctx.doc.project().clone();
            }
            _ => {
                let cmd = to_command(&ctx, &op).unwrap();
                let pre = direct_effect(&ctx.world, &op);
                let mut probe = ctx.doc.clone();
                match probe.execute(cmd.clone()) {
                    Err(e) => {
                        let msg = e.to_string();
                        assert_eq!(bytes(ctx.doc.project()), before_bytes, "{label}");
                        assert!(
                            !msg.contains("ne correspond pas"),
                            "{label} : incohérence interne : {msg}"
                        );
                        // Refus attribuable aux contraintes : la même commande aboutit sans elles.
                        let attributed =
                            works_without_constraints(ctx.doc.project(), ctx.sheet, &cmd);
                        match &op {
                            Op::PoseEq {
                                curve,
                                at,
                                ent,
                                off,
                                ..
                            } => {
                                let self_ref = ent == curve;
                                let cycle = !self_ref && pre.reaches(*ent, *curve);
                                if self_ref {
                                    assert!(msg.contains("soi-même"), "{label} : {msg}");
                                    cov.structural += 1;
                                } else if cycle {
                                    // Une cible non positive est jugée avant le cycle : refus aussi valable.
                                    if msg.contains("circulaire") {
                                        cov.cycles_refused += 1;
                                    } else {
                                        assert!(
                                            msg.contains("n'a pas de solution"),
                                            "{label} : {msg}"
                                        );
                                        cov.pose_infeasible += 1;
                                    }
                                } else {
                                    assert!(
                                        !msg.contains("circulaire"),
                                        "{label} : faux cycle : {msg}"
                                    );
                                    assert!(
                                        msg.contains("n'a pas de solution")
                                            || msg.contains("longueur imposée"),
                                        "{label} : refus inattendu : {msg}"
                                    );
                                    cov.pose_infeasible += 1;
                                    // Faux refus : seule contrainte, courbe droite, cible atteignable.
                                    let c = before_world.curve(*curve).unwrap();
                                    let target = before_world.ent_len(*ent).unwrap() + off;
                                    if before_world.cons.is_empty()
                                        && c.ha == (0.0, 0.0)
                                        && c.hb == (0.0, 0.0)
                                    {
                                        let other = if *at == c.a { c.b } else { c.a };
                                        let s0 = before_world.pts[*at];
                                        let o = before_world.pts[other];
                                        let ang = match &op {
                                            Op::PoseEq { ang, .. } => *ang,
                                            _ => 0.0,
                                        };
                                        let (sn, cs) = ang.to_radians().sin_cos();
                                        let dmin = ((o.0 - s0.0) * sn - (o.1 - s0.1) * cs).abs();
                                        assert!(
                                            !(target >= dmin + 3.0 + 0.01 * target
                                                && target <= 1.0e5),
                                            "{label} : faux refus (cible {target}, minimum {dmin}) : {msg}"
                                        );
                                    }
                                }
                            }
                            Op::PoseVal { .. } => {
                                assert!(
                                    msg.contains("n'a pas de solution")
                                        || msg.contains("longueur imposée"),
                                    "{label} : refus inattendu : {msg}"
                                );
                                cov.pose_infeasible += 1;
                            }
                            _ => {
                                assert!(
                                    attributed,
                                    "{label} : refus sans rapport avec les contraintes : {msg}"
                                );
                                assert!(
                                    msg.contains("longueur imposée")
                                        || msg.contains("circulaire"),
                                    "{label} : refus attribué aux contraintes sans l'indiquer : {msg}"
                                );
                                cov.move_infeasible += 1;
                            }
                        }
                        p_now = ctx.doc.project().clone();
                    }
                    Ok(outcome) => {
                        let p = probe.project().clone();
                        let changed = p != *ctx.doc.project();
                        // Cycle prédit par l'oracle => jamais accepté ; auto-référence idem.
                        if let Op::PoseEq { curve, ent, .. } = &op {
                            assert!(ent != curve, "{label} : auto-référence acceptée");
                            assert!(!pre.reaches(*ent, *curve), "{label} : cycle accepté");
                        }
                        assert!(
                            con_matches(&p, &ctx, &pre.cons),
                            "{label} : contraintes du projet ≠ attendues"
                        );
                        if matches!(op, Op::Delete(_)) && before_world.cons.len() > pre.cons.len() {
                            assert!(
                                outcome.notes.iter().any(|n| n.contains("longueur imposée")),
                                "{label} : retrait sans remarque : {:?}",
                                outcome.notes
                            );
                            cov.removed_with_note += 1;
                        }
                        let world = if matches!(op, Op::Copy) {
                            before_world.clone()
                        } else {
                            let from_move =
                                matches!(op, Op::Move(..) | Op::Shrink { .. } | Op::Translate(..));
                            settle(&ctx, &pre, &p, cov, from_move)
                        };
                        match &op {
                            Op::PoseEq { .. } => cov.eq_posed += 1,
                            Op::PoseVal { .. } => cov.val_posed += 1,
                            Op::Release(_) => cov.released += 1,
                            Op::Translate(..) => cov.translations += 1,
                            Op::Shrink { .. } => cov.shrinks += 1,
                            Op::Copy => {
                                cov.copies += 1;
                                ctx.copied = true;
                                let copy = p.sheets.last().unwrap();
                                assert_eq!(
                                    copy.length_constraints.len(),
                                    p.sheets[0].length_constraints.len(),
                                    "{label}"
                                );
                                for k in &copy.length_constraints {
                                    let own = sheet_len(copy, k.curve);
                                    assert!((own - k.length_mm).abs() <= 1e-6, "{label} : copie");
                                    if let Some(r) = k.reference {
                                        // Référence remappée sur la copie, jamais sur l'original.
                                        assert!(copy.entity(r.entity).is_ok(), "{label}");
                                        assert!(
                                            p.sheets[0].entity(r.entity).is_err(),
                                            "{label} : référence non remappée"
                                        );
                                        let want = sheet_len(copy, r.entity) + r.offset_mm;
                                        assert!(
                                            (want - k.length_mm).abs() <= 1e-6,
                                            "{label} : copie, cible"
                                        );
                                    }
                                }
                            }
                            _ => {}
                        }
                        // Étape si et seulement si l'état change.
                        let mut d2 = probe.clone();
                        if changed {
                            assert!(d2.undo(), "{label} : aucune étape");
                            assert_eq!(bytes(d2.project()), before_bytes, "{label} : annuler");
                            assert!(d2.redo(), "{label}");
                            assert_eq!(bytes(d2.project()), bytes(&p), "{label} : rétablir");
                            ctx.hist.push(ctx.world.clone());
                            ctx.redo.clear();
                        } else {
                            cov.noops += 1;
                            if d2.undo() {
                                assert_ne!(
                                    bytes(d2.project()),
                                    before_bytes,
                                    "{label} : étape créée sans changement"
                                );
                            }
                        }
                        ctx.doc = probe;
                        ctx.world = world;
                        p_now = p;
                    }
                }
            }
        }
        // Invariants à chaque pas : validité, relecture identique, contraintes et longueurs.
        p_now.validate().unwrap_or_else(|e| panic!("{label} : {e}"));
        let text = bytes(&p_now);
        let back =
            persist::from_json(&text).unwrap_or_else(|e| panic!("{label} : relecture : {e}"));
        assert_eq!(back, p_now, "{label} : relecture");
        assert_eq!(bytes(&back), text, "{label} : relecture (octets)");
        cov.reloads += 1;
        assert!(
            con_matches(&p_now, &ctx, &ctx.world.cons),
            "{label} : contraintes ≠ oracle"
        );
        check_lengths(&ctx.world, &p_now, &ctx, &label);
    }
}

#[test]
fn campagne_adverse_egalite_de_longueurs() {
    let mut cov = Cov::default();
    for seed in 1..=SEQUENCES {
        run(seed, &mut cov);
    }
    eprintln!("{cov:#?}");
    assert!(cov.eq_posed >= 150, "{cov:?}");
    assert!(cov.val_posed >= 40, "{cov:?}");
    assert!(cov.released >= 40, "{cov:?}");
    assert!(cov.resolved_by_move >= 150, "{cov:?}");
    assert!(cov.chained >= 20, "{cov:?}");
    assert!(cov.cycles_refused >= 15, "{cov:?}");
    assert!(cov.pose_infeasible >= 30, "{cov:?}");
    assert!(cov.move_infeasible >= 20, "{cov:?}");
    assert!(cov.structural >= 10, "{cov:?}");
    assert!(cov.removed_with_note >= 10, "{cov:?}");
    assert!(cov.undone >= 100, "{cov:?}");
    assert!(cov.redone >= 40, "{cov:?}");
    assert!(cov.copies >= 5, "{cov:?}");
    assert!(cov.translations >= 50, "{cov:?}");
    assert!(cov.shrinks >= 50, "{cov:?}");
    assert!(cov.noops >= 20, "{cov:?}");
}
