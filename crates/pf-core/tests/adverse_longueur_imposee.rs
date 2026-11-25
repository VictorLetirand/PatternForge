//! US-03.3 — campagne adverse « longueur imposée » : 160 séquences à graine fixe de 60 pas
//! (poser / modifier / retirer une longueur imposée, déplacer des points et des poignées,
//! translations, suppressions de courbes, copie de feuille, annuler, rétablir, relecture), sur des
//! courbes de Bézier qui partagent des extrémités et dont une extrémité est un point construit.
//!
//! **Oracle indépendant** : le monde de l'oracle (points libres, un point décalé, courbes avec
//! poignées, contraintes attendues) est tenu ici sans appeler `pf_core::length_constraint` ni
//! `pf_geometry::solve`. La longueur développée est ré-écrite dans ce fichier (Simpson sur la
//! norme de la dérivée, 20 000 pas) ; la faisabilité d'une pose est jugée par un balayage de
//! l'inconnue avec marge. Après chaque commande : l'oracle calcule le monde **avant résolution**
//! (effet direct de la commande), puis exige que
//! - tout point libre qui n'est pas l'extrémité glissante d'une contrainte soit exactement là,
//! - toute extrémité glissante ait bougé **le long de sa direction** (produit vectoriel nul),
//! - toute poignée ajustée garde sa direction (produit vectoriel nul, produit scalaire positif),
//! - la longueur de chaque courbe contrainte vaille la cible à 1e-6 mm.
//!
//! Autres invariants : projet valide, refus = octets identiques (et attribuable aux contraintes :
//! la même commande sans contrainte aboutit), étape si et seulement si l'état change, annuler /
//! rétablir exacts à l'octet, relecture identique, contraintes du projet = contraintes attendues,
//! objets d'un calque verrouillé inchangés. Compteurs de couverture minimaux.
//!
//! Mutation vérifiée à la main : voir `docs/JOURNAL.md` (tranche 90).

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
enum OPt {
    Free(f64, f64),
    /// Point décalé : parent (indice) + (dx ; dy), toujours recalculé.
    Off(usize, f64, f64),
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
enum OAdj {
    Handle { at: usize },
    Slide { at: usize, ang: f64 },
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct OCon {
    curve: EntityId,
    target: f64,
    adj: OAdj,
}

#[derive(Clone, Debug, PartialEq)]
struct World {
    pts: Vec<OPt>,
    curves: Vec<OCurve>,
    cons: Vec<OCon>,
}

impl World {
    fn pos(&self, i: usize) -> Xy {
        match self.pts[i] {
            OPt::Free(x, y) => (x, y),
            OPt::Off(f, dx, dy) => {
                let p = self.pos(f);
                (p.0 + dx, p.1 + dy)
            }
        }
    }
    fn curve(&self, id: EntityId) -> Option<&OCurve> {
        self.curves.iter().find(|c| c.id == id)
    }
    fn geom(&self, c: &OCurve) -> [Xy; 4] {
        let (p0, p3) = (self.pos(c.a), self.pos(c.b));
        [
            p0,
            (p0.0 + c.ha.0, p0.1 + c.ha.1),
            (p3.0 + c.hb.0, p3.1 + c.hb.1),
            p3,
        ]
    }
    fn length(&self, c: &OCurve, panels: usize) -> f64 {
        simpson(&self.geom(c), panels)
    }
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

/// Compteurs de couverture.
#[derive(Default, Debug)]
struct Cov {
    slide_posed: u32,
    handle_posed: u32,
    released: u32,
    resolved_by_move: u32,
    chained: u32,
    pose_infeasible: u32,
    move_infeasible: u32,
    structural: u32,
    removed_with_note: u32,
    cycles: u32,
    undone: u32,
    redone: u32,
    copies: u32,
    lock_refused: u32,
    translations: u32,
    handle_moves: u32,
    noops: u32,
    reloads: u32,
}

struct Ctx {
    p: Project,
    doc: Document,
    sheet: SheetId,
    ids: Vec<PointId>,
    world: World,
    hist: Vec<World>,
    redo: Vec<World>,
    locked: Vec<usize>,
    copied: bool,
}

fn bytes(p: &Project) -> String {
    persist::to_json(p).unwrap()
}

fn curve_kind(
    p: &Project,
    s: SheetId,
    id: EntityId,
) -> Option<(PointId, OffsetMm, OffsetMm, PointId)> {
    match p.sheet(s).unwrap().entity(id).ok()?.kind {
        EntityKind::Curve {
            start,
            handle_start,
            handle_end,
            end,
        } => Some((start, handle_start, handle_end, end)),
        _ => None,
    }
}

fn build(rng: &mut Rng) -> Ctx {
    let mut project = Project::new("Campagne");
    let sheet = project.sheets[0].id;
    let mut ids = Vec::new();
    let mut pts = Vec::new();
    for _ in 0..6 {
        let (x, y) = (rng.val(0.0, 300.0), rng.val(0.0, 300.0));
        let o = project
            .apply(Command::AddPoint {
                sheet,
                at: PointMm::new(x, y),
            })
            .unwrap();
        ids.push(o.points[0]);
        pts.push(OPt::Free(x, y));
    }
    // Un point décalé du premier point libre.
    let (dx, dy) = (rng.val(60.0, 160.0), rng.val(-60.0, 60.0));
    let o = project
        .apply(Command::AddPointAt {
            sheet,
            point: PointRef::Derived(PointDefinition::Offset {
                from: ids[0],
                dx_mm: dx,
                dy_mm: dy,
            }),
        })
        .unwrap();
    ids.push(o.points[0]);
    pts.push(OPt::Off(0, dx, dy));
    let mut world = World {
        pts,
        curves: Vec::new(),
        cons: Vec::new(),
    };
    let mut guard = 0;
    while world.curves.len() < 3 && guard < 50 {
        guard += 1;
        let (a, b) = (rng.pick(7), rng.pick(7));
        let (pa, pb) = (world.pos(a), world.pos(b));
        if a == b || ((pa.0 - pb.0).powi(2) + (pa.1 - pb.1).powi(2)).sqrt() < 30.0 {
            continue;
        }
        if world
            .curves
            .iter()
            .any(|c| (c.a, c.b) == (a, b) || (c.a, c.b) == (b, a))
        {
            continue;
        }
        let ha = if rng.chance(30) {
            (0.0, 0.0)
        } else {
            (rng.val(-80.0, 80.0), rng.val(-80.0, 80.0))
        };
        let hb = if rng.chance(30) {
            (0.0, 0.0)
        } else {
            (rng.val(-80.0, 80.0), rng.val(-80.0, 80.0))
        };
        let o = project
            .apply(Command::AddCurve {
                sheet,
                start: PointRef::Existing(ids[a]),
                c1: PointMm::new(pa.0 + ha.0, pa.1 + ha.1),
                c2: PointMm::new(pb.0 + hb.0, pb.1 + hb.1),
                end: PointRef::Existing(ids[b]),
            })
            .unwrap();
        world.curves.push(OCurve {
            id: o.entities[0],
            a,
            b,
            ha,
            hb,
        });
    }
    // Un fil de segments garde tous les points en vie quand une courbe est supprimée.
    for i in 0..6 {
        project
            .apply(Command::AddSegment {
                sheet,
                from: PointRef::Existing(ids[i]),
                to: PointRef::Existing(ids[i + 1]),
            })
            .unwrap();
    }
    let mut ctx = Ctx {
        doc: Document::new(project.clone()),
        p: project,
        sheet,
        ids,
        world,
        hist: Vec::new(),
        redo: Vec::new(),
        locked: Vec::new(),
        copied: false,
    };
    // Calque verrouillé (une séquence sur six) : un point libre y est rangé.
    if rng.chance(17) {
        let i = rng.pick(6);
        let layer = ctx
            .doc
            .execute(Command::AddLayer {
                sheet,
                name: "Fixe".into(),
            })
            .unwrap()
            .layers[0];
        ctx.doc
            .execute(Command::SetObjectsLayer {
                sheet,
                points: vec![ctx.ids[i]],
                entities: vec![],
                layer: Some(layer),
            })
            .unwrap();
        ctx.doc
            .execute(Command::SetLayerLocked {
                sheet,
                layer,
                locked: true,
            })
            .unwrap();
        ctx.locked.push(i);
    }
    ctx.p = ctx.doc.project().clone();
    ctx.doc = Document::new(ctx.p.clone());
    ctx
}

// ---------------------------------------------------------------------------------------------
// Commandes de la campagne
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug)]
enum Op {
    Pose {
        curve: EntityId,
        target: f64,
        adj: OAdj,
    },
    Release(EntityId),
    Move(usize, Xy),
    MoveHandle {
        curve: EntityId,
        at_start: bool,
        to: Xy,
    },
    Translate(Vec<usize>, Xy),
    DeleteCurve(EntityId),
    Copy,
    Undo,
    Redo,
}

fn to_command(ctx: &Ctx, op: &Op) -> Option<Command> {
    let s = ctx.sheet;
    Some(match op {
        Op::Pose { curve, target, adj } => Command::SetCurveLengthConstraint {
            sheet: s,
            curve: *curve,
            spec: Some(LengthSpec {
                length_mm: *target,
                adjust: match *adj {
                    OAdj::Handle { at } => LengthAdjust::Handle { point: ctx.ids[at] },
                    OAdj::Slide { at, ang } => LengthAdjust::Slide {
                        point: ctx.ids[at],
                        angle_deg: ang,
                    },
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
        Op::MoveHandle {
            curve,
            at_start,
            to,
        } => Command::MoveCurveHandle {
            sheet: s,
            curve: *curve,
            handle: if *at_start {
                CurveHandle::Start
            } else {
                CurveHandle::End
            },
            to: PointMm::new(to.0, to.1),
        },
        Op::Translate(sel, d) => Command::Transform {
            sheet: s,
            points: sel.iter().map(|i| ctx.ids[*i]).collect(),
            transform: Transform::Translate {
                dx_mm: d.0,
                dy_mm: d.1,
            },
        },
        Op::DeleteCurve(c) => Command::Delete {
            sheet: s,
            points: vec![],
            entities: vec![*c],
        },
        Op::Copy => Command::DuplicateSheet { sheet: s },
        Op::Undo | Op::Redo => return None,
    })
}

/// Monde **avant résolution** : l'effet direct de la commande, sans contrainte.
fn direct_effect(w: &World, op: &Op) -> World {
    let mut n = w.clone();
    match op {
        Op::Pose { curve, target, adj } => {
            n.cons.retain(|c| c.curve != *curve);
            n.cons.push(OCon {
                curve: *curve,
                target: *target,
                adj: *adj,
            });
        }
        Op::Release(c) => n.cons.retain(|k| k.curve != *c),
        Op::Move(i, to) => n.pts[*i] = OPt::Free(to.0, to.1),
        Op::MoveHandle {
            curve,
            at_start,
            to,
        } => {
            let c = n.curve(*curve).unwrap().clone();
            let end = if *at_start { n.pos(c.a) } else { n.pos(c.b) };
            let h = (to.0 - end.0, to.1 - end.1);
            for k in n.curves.iter_mut().filter(|k| k.id == *curve) {
                if *at_start {
                    k.ha = h;
                } else {
                    k.hb = h;
                }
            }
        }
        Op::Translate(sel, d) => {
            for i in sel {
                if let OPt::Free(x, y) = n.pts[*i] {
                    n.pts[*i] = OPt::Free(x + d.0, y + d.1);
                }
            }
        }
        Op::DeleteCurve(c) => {
            n.curves.retain(|k| k.id != *c);
            n.cons.retain(|k| k.curve != *c);
        }
        Op::Copy | Op::Undo | Op::Redo => {}
    }
    n
}

fn con_matches(p: &Project, s: SheetId, ctx: &Ctx, cons: &[OCon]) -> bool {
    let sheet = p.sheet(s).unwrap();
    if sheet.length_constraints.len() != cons.len() {
        return false;
    }
    cons.iter().all(|c| {
        sheet.length_constraint(c.curve).is_some_and(|k| {
            k.length_mm == c.target
                && match (c.adj, k.adjust) {
                    (OAdj::Handle { at }, LengthAdjust::Handle { point }) => point == ctx.ids[at],
                    (OAdj::Slide { at, ang }, LengthAdjust::Slide { point, angle_deg }) => {
                        point == ctx.ids[at] && angle_deg == ang
                    }
                    _ => false,
                }
        })
    })
}

/// Vérifie l'état du projet contre le monde « avant résolution » et renvoie le monde résolu.
fn settle(ctx: &Ctx, pre: &World, p: &Project, cov: &mut Cov, from_move: bool) -> World {
    let sheet = p.sheet(ctx.sheet).unwrap();
    let mut w = pre.clone();
    let slid: Vec<(usize, f64)> = pre
        .cons
        .iter()
        .filter_map(|c| match c.adj {
            OAdj::Slide { at, ang } => Some((at, ang)),
            _ => None,
        })
        .collect();
    let mut moved = 0;
    for i in 0..pre.pts.len() {
        let q = sheet.position(ctx.ids[i]).unwrap();
        let before = pre.pos(i);
        match pre.pts[i] {
            OPt::Free(..) => {
                if let Some((_, ang)) = slid.iter().find(|(at, _)| *at == i) {
                    let (sn, cs) = ang.to_radians().sin_cos();
                    let d = (q.x - before.0, q.y - before.1);
                    let cross = d.0 * sn - d.1 * cs;
                    assert!(
                        cross.abs() <= 1e-7,
                        "point {i} sorti de sa droite : {cross}"
                    );
                    if d.0.hypot(d.1) > 1e-9 {
                        moved += 1;
                    }
                    w.pts[i] = OPt::Free(q.x, q.y);
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
            OPt::Off(..) => {
                let r = w.pos(i);
                assert!(
                    (q.x - r.0).abs() <= 1e-9 && (q.y - r.1).abs() <= 1e-9,
                    "point décalé {i} : ({} ; {}) ≠ ({} ; {})",
                    q.x,
                    q.y,
                    r.0,
                    r.1
                );
            }
        }
    }
    for k in 0..w.curves.len() {
        let c = pre.curves[k].clone();
        let (_, ha, hb, _) = curve_kind(p, ctx.sheet, c.id).expect("courbe");
        for (is_a, new, old) in [(true, (ha.dx, ha.dy), c.ha), (false, (hb.dx, hb.dy), c.hb)] {
            let at = if is_a { c.a } else { c.b };
            let unknown = pre
                .cons
                .iter()
                .any(|q| q.curve == c.id && matches!(q.adj, OAdj::Handle { at: a } if a == at));
            if unknown {
                let cross = new.0 * old.1 - new.1 * old.0;
                let dot = new.0 * old.0 + new.1 * old.1;
                let scale = new.0.hypot(new.1) * old.0.hypot(old.1);
                assert!(
                    cross.abs() <= 1e-7 * (1.0 + scale),
                    "poignée tournée : {cross}"
                );
                assert!(dot >= 0.0, "poignée retournée");
                assert!(new.0.hypot(new.1) >= 1e-3 - 1e-9, "poignée nulle acceptée");
                if (new.0 - old.0).abs() > 1e-9 || (new.1 - old.1).abs() > 1e-9 {
                    moved += 1;
                }
                if is_a {
                    w.curves[k].ha = new;
                } else {
                    w.curves[k].hb = new;
                }
            } else {
                assert!(
                    (new.0 - old.0).abs() <= 1e-9 && (new.1 - old.1).abs() <= 1e-9,
                    "poignée modifiée sans raison"
                );
            }
        }
    }
    if from_move && moved > 0 {
        cov.resolved_by_move += 1;
        if moved >= 2 {
            cov.chained += 1;
        }
    }
    for c in &w.cons {
        let curve = w.curve(c.curve).expect("courbe contrainte");
        let len = w.length(curve, 20_000);
        assert!(
            (len - c.target).abs() <= 1e-6,
            "longueur {len} ≠ cible {} ({:?})",
            c.target,
            c.adj
        );
    }
    w
}

/// Balayage de l'inconnue d'une pose : `(minimum, valeur à la borne de la branche, départ)`.
fn scan(w: &World, curve: &OCurve, adj: OAdj) -> (f64, f64) {
    let natural = w.length(curve, 400);
    let chord = {
        let (a, b) = (w.pos(curve.a), w.pos(curve.b));
        (a.0 - b.0).hypot(a.1 - b.1)
    };
    let span = 3.0 * (natural + chord + 1.0);
    let n = 240usize;
    let eval = |s: f64| -> f64 {
        let mut t = w.clone();
        match adj {
            OAdj::Slide { at, ang } => {
                let (sn, cs) = ang.to_radians().sin_cos();
                let p = t.pos(at);
                t.pts[at] = OPt::Free(p.0 + s * cs, p.1 + s * sn);
                let c = t.curve(curve.id).unwrap().clone();
                simpson(&t.geom(&c), 100)
            }
            OAdj::Handle { at } => {
                let c = t.curve(curve.id).unwrap().clone();
                let v = if at == c.a { c.ha } else { c.hb };
                let n0 = v.0.hypot(v.1);
                let (ux, uy) = (v.0 / n0, v.1 / n0);
                let mut g = t.geom(&c);
                if at == c.a {
                    g[1] = (g[0].0 + ux * s, g[0].1 + uy * s);
                } else {
                    g[2] = (g[3].0 + ux * s, g[3].1 + uy * s);
                }
                simpson(&g, 100)
            }
        }
    };
    let (lo, hi, start) = match adj {
        OAdj::Slide { .. } => (-span, span, 0.0),
        OAdj::Handle { at } => {
            let v = if at == curve.a { curve.ha } else { curve.hb };
            (1e-3, span, v.0.hypot(v.1))
        }
    };
    let xs: Vec<f64> = (0..=n)
        .map(|i| lo + (hi - lo) * i as f64 / n as f64)
        .collect();
    let fs: Vec<f64> = xs.iter().map(|x| eval(*x)).collect();
    let (imin, fmin) =
        fs.iter().copied().enumerate().fold(
            (0, f64::INFINITY),
            |a, (i, f)| if f < a.1 { (i, f) } else { a },
        );
    let end = if start >= xs[imin] { fs[n] } else { fs[0] };
    (fmin, end)
}

/// Pose refusée pour une raison de structure que l'oracle connaît d'avance.
fn structural_refusal(w: &World, curve: &OCurve, target: f64, adj: OAdj) -> bool {
    if !(target.is_finite() && target > 0.0) {
        return true;
    }
    match adj {
        OAdj::Slide { at, .. } => {
            matches!(w.pts[at], OPt::Off(..))
                || w.cons.iter().any(|c| {
                    c.curve != curve.id && matches!(c.adj, OAdj::Slide { at: a, .. } if a == at)
                })
        }
        OAdj::Handle { at } => {
            let v = if at == curve.a { curve.ha } else { curve.hb };
            v.0.hypot(v.1) < 1e-3
        }
    }
}

fn random_op(rng: &mut Rng, ctx: &Ctx) -> Op {
    let w = &ctx.world;
    let any_curve = |rng: &mut Rng| w.curves[rng.pick(w.curves.len())].clone();
    let roll = rng.below(100);
    if w.curves.is_empty() {
        return Op::Undo;
    }
    match roll {
        0..=27 => {
            let c = any_curve(rng);
            let natural = w.length(&c, 400);
            let at = if rng.chance(50) { c.a } else { c.b };
            let adj = if rng.chance(55) {
                OAdj::Slide {
                    at,
                    ang: (rng.val(0.0, 360.0) * 10.0).round() / 10.0,
                }
            } else {
                OAdj::Handle { at }
            };
            let target = match rng.below(100) {
                0..=64 => natural * (1.0 + rng.val(-0.25, 0.5)),
                65..=77 => natural,
                78..=87 => natural * rng.val(0.05, 0.6),
                88..=95 => natural * rng.val(2.0, 6.0),
                _ => *[0.0, -3.0].get(rng.pick(2)).unwrap(),
            };
            Op::Pose {
                curve: c.id,
                target: (target * 1000.0).round() / 1000.0,
                adj,
            }
        }
        28..=36 => {
            let k = if !w.cons.is_empty() && rng.chance(85) {
                w.cons[rng.pick(w.cons.len())].curve
            } else {
                any_curve(rng).id
            };
            Op::Release(k)
        }
        37..=66 => {
            // Une fois sur trois, l'autre extrémité d'une courbe à extrémité glissante est tirée
            // trop loin de la droite de glissement : la plus courte courbe possible dépasse la cible.
            let sliding: Vec<OCon> = w
                .cons
                .iter()
                .copied()
                .filter(|c| matches!(c.adj, OAdj::Slide { .. }))
                .collect();
            if !sliding.is_empty() && rng.chance(35) {
                let con = sliding[rng.pick(sliding.len())];
                if let (OAdj::Slide { at, ang }, Some(c)) = (con.adj, w.curve(con.curve)) {
                    let other = if c.a == at { c.b } else { c.a };
                    if matches!(w.pts[other], OPt::Free(..)) {
                        let (sn, cs) = ang.to_radians().sin_cos();
                        let o = w.pos(at);
                        let k = con.target
                            * rng.val(1.05, 1.6)
                            * if rng.chance(50) { 1.0 } else { -1.0 };
                        return Op::Move(other, (o.0 - sn * k, o.1 + cs * k));
                    }
                }
            }
            let free: Vec<usize> = (0..7)
                .filter(|i| matches!(w.pts[*i], OPt::Free(..)))
                .collect();
            let i = free[rng.pick(free.len())];
            let (x, y) = w.pos(i);
            let to = if rng.chance(70) {
                (x + rng.val(-60.0, 60.0), y + rng.val(-60.0, 60.0))
            } else {
                (rng.val(-100.0, 400.0), rng.val(-100.0, 400.0))
            };
            Op::Move(i, to)
        }
        67..=74 => {
            let c = any_curve(rng);
            let at_start = rng.chance(50);
            let end = if at_start { w.pos(c.a) } else { w.pos(c.b) };
            Op::MoveHandle {
                curve: c.id,
                at_start,
                to: (end.0 + rng.val(-90.0, 90.0), end.1 + rng.val(-90.0, 90.0)),
            }
        }
        75..=81 => {
            let n = 1 + rng.pick(3);
            let mut sel: Vec<usize> = Vec::new();
            for _ in 0..n {
                let i = rng.pick(6);
                if !sel.contains(&i) {
                    sel.push(i);
                }
            }
            Op::Translate(sel, (rng.val(-40.0, 40.0), rng.val(-40.0, 40.0)))
        }
        82..=85 => Op::DeleteCurve(any_curve(rng).id),
        86..=87 if !ctx.copied => Op::Copy,
        88..=92 => Op::Undo,
        93..=98 => Op::Redo,
        _ => Op::Move(0, w.pos(0)),
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

fn run(seed: u64, cov: &mut Cov) {
    let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let mut ctx = build(&mut rng);
    for step in 0..STEPS {
        let op = random_op(&mut rng, &ctx);
        let label = format!("séquence {seed}, pas {step}, {op:?}");
        let before_bytes = bytes(ctx.doc.project());
        let before_world = ctx.world.clone();
        match op {
            Op::Undo => {
                let ok = ctx.doc.undo();
                assert_eq!(ok, !ctx.hist.is_empty(), "{label}");
                if ok {
                    ctx.redo.push(ctx.world.clone());
                    ctx.world = ctx.hist.pop().unwrap();
                    cov.undone += 1;
                }
                ctx.p = ctx.doc.project().clone();
            }
            Op::Redo => {
                let ok = ctx.doc.redo();
                assert_eq!(ok, !ctx.redo.is_empty(), "{label}");
                if ok {
                    ctx.hist.push(ctx.world.clone());
                    ctx.world = ctx.redo.pop().unwrap();
                    cov.redone += 1;
                }
                ctx.p = ctx.doc.project().clone();
            }
            _ => {
                let cmd = to_command(&ctx, &op).unwrap();
                let pre = direct_effect(&ctx.world, &op);
                let mut probe = ctx.doc.clone();
                match probe.execute(cmd.clone()) {
                    Err(e) => {
                        let msg = e.to_string();
                        assert_eq!(bytes(ctx.doc.project()), before_bytes, "{label}");
                        // Un refus de la validation finale (longueur écrite ≠ cible) trahirait une
                        // résolution incohérente : ce n'est jamais un refus légitime.
                        assert!(
                            !msg.contains("ne correspond pas"),
                            "{label} : incohérence interne : {msg}"
                        );
                        // Refus attribuable.
                        let sheet_id = ctx.sheet;
                        let attributed =
                            works_without_constraints(ctx.doc.project(), sheet_id, &cmd);
                        match &op {
                            Op::Pose { curve, target, adj } => {
                                let c = before_world.curve(*curve).unwrap().clone();
                                if structural_refusal(&before_world, &c, *target, *adj) {
                                    cov.structural += 1;
                                } else if msg.contains("circulaire") {
                                    cov.cycles += 1;
                                } else if msg.contains("n'a pas de solution") {
                                    cov.pose_infeasible += 1;
                                } else {
                                    // Une autre contrainte est devenue irrésoluble par ce glissement.
                                    assert!(
                                        msg.contains("longueur imposée")
                                            || msg.contains("verrouill"),
                                        "{label} : refus inattendu : {msg}"
                                    );
                                }
                                // Faux refus : seule contrainte, cible faisable avec marge.
                                if before_world.cons.is_empty()
                                    && !structural_refusal(&before_world, &c, *target, *adj)
                                    && ctx.locked.is_empty()
                                {
                                    let (fmin, fend) = scan(&before_world, &c, *adj);
                                    let margin = 3.0 + 0.01 * target;
                                    assert!(
                                        !(*target >= fmin + margin && *target <= fend - margin),
                                        "{label} : faux refus, plage [{fmin} ; {fend}] : {msg}"
                                    );
                                }
                            }
                            _ => {
                                if attributed {
                                    assert!(
                                        msg.contains("longueur imposée")
                                            || msg.contains("circulaire")
                                            || msg.contains("verrouill"),
                                        "{label} : refus attribué aux contraintes sans l'indiquer : {msg}"
                                    );
                                    if msg.contains("verrouill") {
                                        cov.lock_refused += 1;
                                    } else {
                                        cov.move_infeasible += 1;
                                    }
                                } else if msg.contains("verrouill") {
                                    cov.lock_refused += 1;
                                }
                            }
                        }
                        ctx.p = ctx.doc.project().clone();
                    }
                    Ok(outcome) => {
                        let p = probe.project().clone();
                        let changed = p != *ctx.doc.project();
                        // Pose acceptée : jamais si l'oracle sait la structure invalide.
                        if let Op::Pose { curve, target, adj } = &op {
                            let c = before_world.curve(*curve).unwrap().clone();
                            assert!(
                                !structural_refusal(&before_world, &c, *target, *adj),
                                "{label} : structure invalide acceptée"
                            );
                        }
                        // Contraintes attendues = contraintes du projet ; retraits avec remarque.
                        assert!(
                            con_matches(&p, ctx.sheet, &ctx, &pre.cons),
                            "{label} : contraintes du projet ≠ attendues"
                        );
                        if matches!(op, Op::DeleteCurve(_))
                            && before_world.cons.len() > pre.cons.len()
                        {
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
                            let from_move = matches!(
                                op,
                                Op::Move(..) | Op::MoveHandle { .. } | Op::Translate(..)
                            );
                            let w = settle(&ctx, &pre, &p, cov, from_move);
                            // Calque verrouillé : points inchangés.
                            for i in &ctx.locked {
                                let (a, b) = (before_world.pos(*i), w.pos(*i));
                                assert!(
                                    (a.0 - b.0).abs() <= 1e-12 && (a.1 - b.1).abs() <= 1e-12,
                                    "{label} : point verrouillé déplacé"
                                );
                            }
                            w
                        };
                        match &op {
                            Op::Pose { adj, .. } => match adj {
                                OAdj::Slide { .. } => cov.slide_posed += 1,
                                OAdj::Handle { .. } => cov.handle_posed += 1,
                            },
                            Op::Release(_) => cov.released += 1,
                            Op::Translate(..) => cov.translations += 1,
                            Op::MoveHandle { .. } => cov.handle_moves += 1,
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
                                    let g = copy.curve_geometry(k.curve).unwrap();
                                    let len = simpson(
                                        &[
                                            (g.p0.x, g.p0.y),
                                            (g.c1.x, g.c1.y),
                                            (g.c2.x, g.c2.y),
                                            (g.p3.x, g.p3.y),
                                        ],
                                        20_000,
                                    );
                                    assert!((len - k.length_mm).abs() <= 1e-6, "{label} : copie");
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
                        ctx.p = p;
                    }
                }
            }
        }
        // Invariants à chaque pas : validité, relecture identique, contraintes attendues.
        ctx.p.validate().unwrap_or_else(|e| panic!("{label} : {e}"));
        let text = bytes(&ctx.p);
        let back =
            persist::from_json(&text).unwrap_or_else(|e| panic!("{label} : relecture : {e}"));
        assert_eq!(back, ctx.p, "{label} : relecture");
        assert_eq!(bytes(&back), text, "{label} : relecture (octets)");
        cov.reloads += 1;
        assert!(
            con_matches(&ctx.p, ctx.sheet, &ctx, &ctx.world.cons),
            "{label} : contraintes ≠ oracle"
        );
        for c in &ctx.world.cons {
            let curve = ctx.world.curve(c.curve).unwrap();
            let len = ctx.world.length(curve, 20_000);
            assert!(
                (len - c.target).abs() <= 1e-6,
                "{label} : longueur {len} ≠ {}",
                c.target
            );
        }
    }
}

#[test]
fn campagne_adverse_longueur_imposee() {
    let mut cov = Cov::default();
    for seed in 1..=SEQUENCES {
        run(seed, &mut cov);
    }
    eprintln!("{cov:#?}");
    assert!(cov.slide_posed >= 100, "{cov:?}");
    assert!(cov.handle_posed >= 60, "{cov:?}");
    assert!(cov.released >= 40, "{cov:?}");
    assert!(cov.resolved_by_move >= 150, "{cov:?}");
    assert!(cov.chained >= 5, "{cov:?}");
    assert!(cov.pose_infeasible >= 20, "{cov:?}");
    assert!(cov.move_infeasible >= 20, "{cov:?}");
    assert!(cov.structural >= 10, "{cov:?}");
    assert!(cov.removed_with_note >= 5, "{cov:?}");
    assert!(cov.undone >= 100, "{cov:?}");
    assert!(cov.redone >= 40, "{cov:?}");
    assert!(cov.copies >= 5, "{cov:?}");
    assert!(cov.lock_refused >= 2, "{cov:?}");
    assert!(cov.translations >= 50, "{cov:?}");
    assert!(cov.handle_moves >= 50, "{cov:?}");
    assert!(cov.noops >= 20, "{cov:?}");
}
