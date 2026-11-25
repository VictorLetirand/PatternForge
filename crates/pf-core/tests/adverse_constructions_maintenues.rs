//! US-03.2 — campagne adverse « constructions maintenues » : 160 séquences à graine fixe de 60 pas
//! (segments plain / longueur-angle / perpendiculaire / parallèle, liage et déliage, déplacements
//! de points, transformations, suppressions, inversions de sens, annuler, rétablir,
//! sérialiser-relire).
//!
//! **Oracle indépendant** : le monde de l'oracle (points libres, segments, constructions) est
//! tenu ici, sans appeler `pf_core::kept` ; les positions attendues sont reconstruites
//! géométriquement (cos / sin en radians, normale `(-dy ; dx) / |d|`) et la propagation est
//! recalculée dans l'ordre de création (un parent est toujours créé avant son dépendant). Pour
//! chaque commande l'oracle prédit d'abord accepté / refusé et l'état résultant, le code doit être
//! d'accord.
//!
//! Invariants après chaque pas : projet valide, positions = oracle (1e-6 mm), états « maintenu »
//! = oracle, refus = octets identiques, étape créée si et seulement si l'état change, annuler /
//! rétablir exacts à l'octet, relecture identique (octets et valeur). Des compteurs de couverture
//! minimaux garantissent que la campagne exerce réellement les cas difficiles.
//!
//! Mutation vérifiée à la main : voir `docs/JOURNAL.md` (tranche 89).

use pf_core::persist;
use pf_core::*;
use pf_geometry::{PointMm, Side, Transform};

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
    /// Nombre à deux décimales dans [lo ; hi].
    fn val(&mut self, lo: f64, hi: f64) -> f64 {
        let n = ((hi - lo) * 100.0) as u64;
        lo + self.below(n + 1) as f64 / 100.0
    }
}

// ---------------------------------------------------------------------------------------------
// Oracle
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
enum Kind {
    Plain,
    Polar {
        len: f64,
        ang: f64,
    },
    Perp {
        reference: EntityId,
        len: f64,
        left: bool,
    },
    Par {
        reference: EntityId,
        dist: f64,
        left: bool,
    },
}

#[derive(Clone, Debug, PartialEq)]
struct OSeg {
    id: EntityId,
    a: PointId,
    b: PointId,
    kind: Kind,
    kept: bool,
}

#[derive(Clone, Debug, PartialEq, Default)]
struct World {
    pts: Vec<(PointId, f64, f64)>,
    segs: Vec<OSeg>,
}

type Xy = (f64, f64);

impl World {
    fn pos(&self, id: PointId) -> Option<Xy> {
        self.pts.iter().find(|p| p.0 == id).map(|p| (p.1, p.2))
    }
    fn set(&mut self, id: PointId, at: Xy) {
        if let Some(p) = self.pts.iter_mut().find(|p| p.0 == id) {
            p.1 = at.0;
            p.2 = at.1;
        }
    }
    fn seg(&self, id: EntityId) -> Option<&OSeg> {
        self.segs.iter().find(|s| s.id == id)
    }
    fn dist(&self, a: PointId, b: PointId) -> f64 {
        let (p, q) = (self.pos(a).unwrap(), self.pos(b).unwrap());
        ((q.0 - p.0).powi(2) + (q.1 - p.1).powi(2)).sqrt()
    }
    /// Un segment nul : le code refuse (≤ 1e-9 mm).
    fn has_degenerate(&self) -> bool {
        self.segs.iter().any(|s| self.dist(s.a, s.b) <= 1e-9)
    }

    /// Positions des points suivis d'après les parents actuels ; `None` = géométrie dégénérée
    /// ou référence disparue.
    fn expected(&self, s: &OSeg) -> Option<Vec<(PointId, Xy)>> {
        match &s.kind {
            Kind::Plain => None,
            Kind::Polar { len, ang } => {
                let a = self.pos(s.a)?;
                let r = ang.to_radians();
                Some(vec![(s.b, (a.0 + len * r.cos(), a.1 + len * r.sin()))])
            }
            Kind::Perp {
                reference,
                len,
                left,
            } => {
                let r = self.seg(*reference)?;
                let (ra, rb) = (self.pos(r.a)?, self.pos(r.b)?);
                let (dx, dy) = (rb.0 - ra.0, rb.1 - ra.1);
                let l = (dx * dx + dy * dy).sqrt();
                if l <= 1e-9 {
                    return None;
                }
                let k = if *left { *len } else { -*len };
                let f = self.pos(s.a)?;
                Some(vec![(s.b, (f.0 - dy / l * k, f.1 + dx / l * k))])
            }
            Kind::Par {
                reference,
                dist,
                left,
            } => {
                let r = self.seg(*reference)?;
                let (ra, rb) = (self.pos(r.a)?, self.pos(r.b)?);
                let (dx, dy) = (rb.0 - ra.0, rb.1 - ra.1);
                let l = (dx * dx + dy * dy).sqrt();
                if l <= 1e-9 {
                    return None;
                }
                let k = if *left { *dist } else { -*dist };
                let (nx, ny) = (-dy / l * k, dx / l * k);
                Some(vec![
                    (s.a, (ra.0 + nx, ra.1 + ny)),
                    (s.b, (rb.0 + nx, rb.1 + ny)),
                ])
            }
        }
    }

    fn reference_of(&self, s: &OSeg) -> Option<EntityId> {
        match s.kind {
            Kind::Perp { reference, .. } | Kind::Par { reference, .. } => Some(reference),
            _ => None,
        }
    }
}

/// Compteurs de couverture.
#[derive(Default, Debug)]
struct Cov {
    polar: u32,
    perp: u32,
    par: u32,
    linked: u32,
    unlinked_cmd: u32,
    propagated: u32,
    chained: u32,
    direct_unlink: u32,
    stale_refused: u32,
    degenerate_refused: u32,
    transforms: u32,
    deleted_refs: u32,
    reversed: u32,
    undone: u32,
    redone: u32,
    reloads: u32,
    dead_ref_unlink: u32,
}

/// Règle de l'oracle après un changement direct (`new` = monde modifié, `old` = avant) : les
/// liens sont réévalués dans l'ordre de création. `Err` = refus (géométrie dégénérée).
fn settle(old: &World, mut new: World, cov: &mut Cov) -> Result<World, ()> {
    // Une référence supprimée ou changée de sens délie.
    for i in 0..new.segs.len() {
        let s = new.segs[i].clone();
        if !s.kept {
            continue;
        }
        let dead = new.reference_of(&s).is_some_and(|r| new.seg(r).is_none());
        let changed = new.reference_of(&s).is_some_and(|r| {
            let o = old.seg(r).map(|x| (x.a, x.b));
            let n = new.seg(r).map(|x| (x.a, x.b));
            o != n
        });
        if dead || changed {
            new.segs[i].kept = false;
            cov.dead_ref_unlink += 1;
        }
    }
    let mut moved_links = 0;
    let mut written: Vec<PointId> = Vec::new();
    for i in 0..new.segs.len() {
        let s = new.segs[i].clone();
        if !s.kept {
            continue;
        }
        let Some(exp) = new.expected(&s) else {
            return Err(());
        };
        let mut mismatch = false;
        let mut direct = false;
        for (d, e) in &exp {
            let cur = new.pos(*d).unwrap();
            if ((cur.0 - e.0).powi(2) + (cur.1 - e.1).powi(2)).sqrt() > 1e-9 {
                mismatch = true;
            }
            if let Some(was) = old.pos(*d) {
                if was != cur {
                    direct = true;
                }
            }
        }
        if !mismatch {
            continue;
        }
        if direct {
            new.segs[i].kept = false;
            cov.direct_unlink += 1;
            continue;
        }
        for (d, e) in exp {
            new.set(d, e);
            written.push(d);
        }
        moved_links += 1;
        // Chaîne : ce lien dépend d'un point écrit par un lien précédent.
        let parents = match s.kind {
            Kind::Polar { .. } => vec![s.a],
            Kind::Perp { reference, .. } => {
                let r = new.seg(reference).unwrap();
                vec![s.a, r.a, r.b]
            }
            Kind::Par { reference, .. } => {
                let r = new.seg(reference).unwrap();
                vec![r.a, r.b]
            }
            Kind::Plain => vec![],
        };
        if parents
            .iter()
            .any(|p| written.contains(p) && !new.segs[i].kept_dependents().contains(p))
        {
            cov.chained += 1;
        }
    }
    if moved_links > 0 {
        cov.propagated += 1;
    }
    if new.has_degenerate() {
        return Err(());
    }
    Ok(new)
}

impl OSeg {
    fn kept_dependents(&self) -> Vec<PointId> {
        match self.kind {
            Kind::Par { .. } => vec![self.a, self.b],
            _ => vec![self.b],
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Vérification du projet contre l'oracle
// ---------------------------------------------------------------------------------------------

fn bytes(p: &Project) -> String {
    persist::to_json(p).unwrap()
}

fn check(ctx: &str, p: &Project, sheet: SheetId, w: &World) {
    p.validate()
        .unwrap_or_else(|e| panic!("{ctx} : projet invalide : {e}"));
    let s = p.sheet(sheet).unwrap();
    assert_eq!(s.points.len(), w.pts.len(), "{ctx} : nombre de points");
    assert_eq!(s.entities.len(), w.segs.len(), "{ctx} : nombre de tracés");
    for (id, x, y) in &w.pts {
        let q = s
            .position(*id)
            .unwrap_or_else(|e| panic!("{ctx} : point {id:?} : {e}"));
        assert!(
            (q.x - x).abs() < 1e-6 && (q.y - y).abs() < 1e-6,
            "{ctx} : point {id:?} en {q:?}, oracle ({x} ; {y})"
        );
    }
    for o in &w.segs {
        assert_eq!(
            s.is_kept_segment(o.id),
            o.kept,
            "{ctx} : état maintenu de {:?}",
            o.id
        );
        let (a, b) = s.segment_point_ids(o.id).unwrap();
        assert_eq!((a, b), (o.a, o.b), "{ctx} : extrémités de {:?}", o.id);
    }
}

/// Points dont dépend au moins un lien maintenu (départ, extrémités de la référence).
fn parent_points(w: &World) -> Vec<PointId> {
    let mut v = Vec::new();
    for s in w.segs.iter().filter(|s| s.kept) {
        match s.kind {
            Kind::Polar { .. } => v.push(s.a),
            Kind::Perp { reference, .. } => {
                v.push(s.a);
                if let Some(r) = w.seg(reference) {
                    v.extend([r.a, r.b]);
                }
            }
            Kind::Par { reference, .. } => {
                if let Some(r) = w.seg(reference) {
                    v.extend([r.a, r.b]);
                }
            }
            Kind::Plain => {}
        }
    }
    v
}

/// Point de départ d'une construction : souvent un point déjà suivi, pour former des chaînes.
fn pick_from(rng: &mut Rng, w: &World) -> PointId {
    let dependents: Vec<PointId> = w
        .segs
        .iter()
        .filter(|s| s.kept)
        .flat_map(|s| s.kept_dependents())
        .collect();
    if !dependents.is_empty() && rng.chance(50) {
        return dependents[rng.below(dependents.len() as u64) as usize];
    }
    w.pts[rng.below(w.pts.len() as u64) as usize].0
}

// ---------------------------------------------------------------------------------------------
// Séquences
// ---------------------------------------------------------------------------------------------

fn run(seq: u64, cov: &mut Cov) {
    let mut rng = Rng(0x5EED_0320_0000 + seq * 0x9E37_79B9 + 1);
    for _ in 0..4 {
        rng.step();
    }
    let project = Project::new("Campagne");
    let sheet = project.sheets[0].id;
    let mut doc = Document::new(project);
    let mut w = World::default();
    // Piles d'annulation de l'oracle, synchrones avec celles du document.
    let mut undo: Vec<World> = Vec::new();
    let mut redo: Vec<World> = Vec::new();
    let mut pending: Option<EntityId> = None;

    for step in 0..STEPS {
        let ctx = format!("séquence {seq}, pas {step}");
        let before_bytes = bytes(doc.project());
        let before_w = w.clone();
        // Une construction fraîchement créée est souvent liée tout de suite (cas nominal).
        let forced_link = if pending.is_some() && rng.chance(75) {
            pending.take()
        } else {
            pending = None;
            None
        };
        let roll = if forced_link.is_some() {
            45
        } else {
            rng.below(100)
        };
        // Résultat prédit : `Some(nouveau monde)` accepté, `None` refusé.
        let mut cmd: Option<Command> = None;
        let mut predicted: Option<World> = None;
        let mut special = 0; // 1 annuler, 2 rétablir, 3 relire

        let pick_pt = |rng: &mut Rng, w: &World| -> Option<PointId> {
            (!w.pts.is_empty()).then(|| w.pts[rng.below(w.pts.len() as u64) as usize].0)
        };
        let pick_seg =
            |rng: &mut Rng, w: &World, pred: &dyn Fn(&OSeg) -> bool| -> Option<EntityId> {
                let c: Vec<EntityId> = w.segs.iter().filter(|s| pred(s)).map(|s| s.id).collect();
                (!c.is_empty()).then(|| c[rng.below(c.len() as u64) as usize])
            };

        if roll < 12 || w.segs.len() < 2 {
            // Segment simple (références, parents).
            let from_existing = !w.pts.is_empty() && rng.chance(40);
            let to = PointMm::new(rng.val(-400.0, 400.0), rng.val(-400.0, 400.0));
            let from_ref = if from_existing {
                PointRef::Existing(pick_pt(&mut rng, &w).unwrap())
            } else {
                PointRef::New(PointMm::new(rng.val(-400.0, 400.0), rng.val(-400.0, 400.0)))
            };
            cmd = Some(Command::AddSegment {
                sheet,
                from: from_ref,
                to: PointRef::New(to),
            });
            // Prévision : appliquée plus bas d'après la sortie (ids neufs).
            predicted = Some(w.clone());
        } else if roll < 25 {
            let from = pick_from(&mut rng, &w);
            let len = if rng.chance(8) {
                0.0
            } else {
                rng.val(5.0, 200.0)
            };
            let ang = rng.val(-360.0, 720.0);
            cmd = Some(Command::AddSegmentPolar {
                sheet,
                from: PointRef::Existing(from),
                length_mm: len,
                angle_deg: ang,
            });
            predicted = (len > 0.0).then(|| w.clone());
        } else if roll < 32 {
            let from = pick_from(&mut rng, &w);
            let reference = pick_seg(&mut rng, &w, &|_| true).unwrap();
            let len = if rng.chance(8) {
                0.0
            } else {
                rng.val(5.0, 150.0)
            };
            cmd = Some(Command::AddPerpendicular {
                sheet,
                reference,
                from: PointRef::Existing(from),
                length_mm: len,
                side: if rng.chance(50) {
                    Side::Left
                } else {
                    Side::Right
                },
            });
            predicted = (len > 0.0).then(|| w.clone());
        } else if roll < 39 {
            let reference = pick_seg(&mut rng, &w, &|_| true).unwrap();
            let dist = if rng.chance(8) {
                0.0
            } else {
                rng.val(5.0, 150.0)
            };
            cmd = Some(Command::AddParallel {
                sheet,
                reference,
                distance_mm: dist,
                side: if rng.chance(50) {
                    Side::Left
                } else {
                    Side::Right
                },
            });
            predicted = (dist > 0.0).then(|| w.clone());
        } else if roll < 53 {
            // Lier / délier.
            let any = pick_seg(&mut rng, &w, &|s| s.kind != Kind::Plain);
            let target = if forced_link.is_some() {
                forced_link
            } else if rng.chance(10) {
                pick_seg(&mut rng, &w, &|_| true)
            } else {
                any
            };
            if let Some(t) = target {
                let kept = forced_link.is_some() || rng.chance(65);
                cmd = Some(Command::SetConstructionKept {
                    sheet,
                    segment: t,
                    kept,
                });
                let s = w.seg(t).unwrap().clone();
                predicted = if s.kept == kept {
                    Some(w.clone())
                } else if !kept {
                    let mut n = w.clone();
                    n.segs.iter_mut().find(|x| x.id == t).unwrap().kept = false;
                    cov.unlinked_cmd += 1;
                    Some(n)
                } else if s.kind == Kind::Plain {
                    None
                } else {
                    // Lier : référence vivante, géométrie non dégénérée, écart ≤ 1e-6.
                    match w.expected(&s) {
                        Some(exp)
                            if exp.iter().all(|(d, e)| {
                                let cur = w.pos(*d).unwrap();
                                ((cur.0 - e.0).powi(2) + (cur.1 - e.1).powi(2)).sqrt() <= 1e-6
                            }) =>
                        {
                            let mut n = w.clone();
                            n.segs.iter_mut().find(|x| x.id == t).unwrap().kept = true;
                            match settle(&w, n, cov) {
                                Ok(n) => {
                                    cov.linked += 1;
                                    Some(n)
                                }
                                Err(()) => None,
                            }
                        }
                        _ => {
                            cov.stale_refused += 1;
                            None
                        }
                    }
                };
            }
        } else if roll < 79 {
            // Déplacer un point : libre, parent, suivi, ou écrasé sur un autre point.
            let parents = parent_points(&w);
            let chosen = if !parents.is_empty() && rng.chance(55) {
                Some(parents[rng.below(parents.len() as u64) as usize])
            } else {
                pick_pt(&mut rng, &w)
            };
            if let Some(p) = chosen {
                let other = w.pts.iter().map(|x| x.0).find(|x| *x != p);
                let to = if let (true, Some(q)) = (rng.chance(10), other) {
                    w.pos(q).unwrap()
                } else {
                    (rng.val(-400.0, 400.0), rng.val(-400.0, 400.0))
                };
                cmd = Some(Command::MovePoint {
                    sheet,
                    point: p,
                    to: PointMm::new(to.0, to.1),
                });
                let mut n = w.clone();
                n.set(p, to);
                predicted = match settle(&w, n, cov) {
                    Ok(n) => Some(n),
                    Err(()) => {
                        cov.degenerate_refused += 1;
                        None
                    }
                };
            }
        } else if roll < 83 {
            // Transformer un sous-ensemble de points : translation ou rotation.
            let mut ids: Vec<PointId> = Vec::new();
            for p in &w.pts {
                if rng.chance(40) {
                    ids.push(p.0);
                }
            }
            if !ids.is_empty() {
                let (transform, f): (Transform, Box<dyn Fn(Xy) -> Xy>) = if rng.chance(50) {
                    let (dx, dy) = (rng.val(-50.0, 50.0), rng.val(-50.0, 50.0));
                    (
                        Transform::Translate {
                            dx_mm: dx,
                            dy_mm: dy,
                        },
                        Box::new(move |p: Xy| (p.0 + dx, p.1 + dy)),
                    )
                } else {
                    let (cx, cy) = (rng.val(-100.0, 100.0), rng.val(-100.0, 100.0));
                    let ang = rng.val(1.0, 359.0);
                    let r = ang.to_radians();
                    (
                        Transform::Rotate {
                            center: PointMm::new(cx, cy),
                            angle_deg: ang,
                        },
                        Box::new(move |p: Xy| {
                            let (x, y) = (p.0 - cx, p.1 - cy);
                            (
                                cx + x * r.cos() - y * r.sin(),
                                cy + x * r.sin() + y * r.cos(),
                            )
                        }),
                    )
                };
                cmd = Some(Command::Transform {
                    sheet,
                    points: ids.clone(),
                    transform,
                });
                let mut n = w.clone();
                for id in &ids {
                    let q = f(w.pos(*id).unwrap());
                    n.set(*id, q);
                }
                predicted = settle(&w, n, cov).ok();
                cov.transforms += 1;
            }
        } else if roll < 85 {
            // Supprimer un segment (souvent une référence).
            let t = if rng.chance(60) {
                let refs: Vec<EntityId> = w
                    .segs
                    .iter()
                    .filter_map(|s| w.reference_of(s))
                    .filter(|r| w.seg(*r).is_some())
                    .collect();
                if refs.is_empty() {
                    None
                } else {
                    Some(refs[rng.below(refs.len() as u64) as usize])
                }
            } else {
                pick_seg(&mut rng, &w, &|_| true)
            };
            if let Some(t) = t {
                cmd = Some(Command::Delete {
                    sheet,
                    points: vec![],
                    entities: vec![t],
                });
                let mut n = w.clone();
                let gone = n.segs.iter().position(|s| s.id == t).unwrap();
                let gone = n.segs.remove(gone);
                for e in [gone.a, gone.b] {
                    if !n.segs.iter().any(|s| s.a == e || s.b == e) {
                        n.pts.retain(|p| p.0 != e);
                    }
                }
                if w.segs.iter().any(|s| w.reference_of(s) == Some(t)) {
                    cov.deleted_refs += 1;
                }
                predicted = settle(&w, n, cov).ok();
            }
        } else if roll < 87 {
            // Inverser le sens d'un segment.
            if let Some(t) = pick_seg(&mut rng, &w, &|_| true) {
                cmd = Some(Command::ReversePath { sheet, entity: t });
                let mut n = w.clone();
                let s = n.segs.iter_mut().find(|s| s.id == t).unwrap();
                std::mem::swap(&mut s.a, &mut s.b);
                s.kind = Kind::Plain;
                s.kept = false;
                predicted = settle(&w, n, cov).ok();
                cov.reversed += 1;
            }
        } else if roll < 93 {
            special = 1;
        } else if roll < 97 {
            special = 2;
        } else {
            special = 3;
        }

        // ----- exécution -----
        match special {
            1 => {
                let ok = doc.undo();
                assert_eq!(ok, !undo.is_empty(), "{ctx} : annuler");
                if let Some(prev) = undo.pop() {
                    redo.push(std::mem::replace(&mut w, prev));
                    cov.undone += 1;
                }
                check(&ctx, doc.project(), sheet, &w);
                continue;
            }
            2 => {
                let ok = doc.redo();
                assert_eq!(ok, !redo.is_empty(), "{ctx} : rétablir");
                if let Some(next) = redo.pop() {
                    undo.push(std::mem::replace(&mut w, next));
                    cov.redone += 1;
                }
                check(&ctx, doc.project(), sheet, &w);
                continue;
            }
            3 => {
                let text = bytes(doc.project());
                let back = persist::from_json(&text)
                    .unwrap_or_else(|e| panic!("{ctx} : relecture refusée : {e}"));
                assert_eq!(bytes(&back), text, "{ctx} : relecture identique (octets)");
                assert_eq!(&back, doc.project(), "{ctx} : relecture identique (valeur)");
                cov.reloads += 1;
                continue;
            }
            _ => {}
        }
        let Some(command) = cmd else { continue };
        let result = doc.execute(command.clone());
        match (predicted, result) {
            (Some(mut next), Ok(out)) => {
                // Les créations ajoutent leurs points et segments d'après la sortie.
                match &command {
                    Command::AddSegment { from, .. } => {
                        let (from_id, to_id) = match from {
                            PointRef::Existing(f) => (*f, out.points[0]),
                            _ => (out.points[0], out.points[1]),
                        };
                        for id in &out.points {
                            let q = doc.project().sheet(sheet).unwrap().position(*id).unwrap();
                            next.pts.push((*id, q.x, q.y));
                        }
                        next.segs.push(OSeg {
                            id: out.entities[0],
                            a: from_id,
                            b: to_id,
                            kind: Kind::Plain,
                            kept: false,
                        });
                    }
                    Command::AddSegmentPolar {
                        from,
                        length_mm,
                        angle_deg,
                        ..
                    } => {
                        let PointRef::Existing(a) = from else {
                            unreachable!()
                        };
                        assert_eq!(out.points.len(), 1, "{ctx} : point fusionné");
                        let q = doc
                            .project()
                            .sheet(sheet)
                            .unwrap()
                            .position(out.points[0])
                            .unwrap();
                        next.pts.push((out.points[0], q.x, q.y));
                        next.segs.push(OSeg {
                            id: out.entities[0],
                            a: *a,
                            b: out.points[0],
                            kind: Kind::Polar {
                                len: *length_mm,
                                ang: *angle_deg,
                            },
                            kept: false,
                        });
                        cov.polar += 1;
                        pending = Some(out.entities[0]);
                    }
                    Command::AddPerpendicular {
                        reference,
                        from,
                        length_mm,
                        side,
                        ..
                    } => {
                        let PointRef::Existing(a) = from else {
                            unreachable!()
                        };
                        assert_eq!(out.points.len(), 1, "{ctx} : point fusionné");
                        let q = doc
                            .project()
                            .sheet(sheet)
                            .unwrap()
                            .position(out.points[0])
                            .unwrap();
                        next.pts.push((out.points[0], q.x, q.y));
                        next.segs.push(OSeg {
                            id: out.entities[0],
                            a: *a,
                            b: out.points[0],
                            kind: Kind::Perp {
                                reference: *reference,
                                len: *length_mm,
                                left: *side == Side::Left,
                            },
                            kept: false,
                        });
                        cov.perp += 1;
                        pending = Some(out.entities[0]);
                    }
                    Command::AddParallel {
                        reference,
                        distance_mm,
                        side,
                        ..
                    } => {
                        assert_eq!(out.points.len(), 2, "{ctx} : points fusionnés");
                        for id in &out.points {
                            let q = doc.project().sheet(sheet).unwrap().position(*id).unwrap();
                            next.pts.push((*id, q.x, q.y));
                        }
                        next.segs.push(OSeg {
                            id: out.entities[0],
                            a: out.points[0],
                            b: out.points[1],
                            kind: Kind::Par {
                                reference: *reference,
                                dist: *distance_mm,
                                left: *side == Side::Left,
                            },
                            kept: false,
                        });
                        cov.par += 1;
                        pending = Some(out.entities[0]);
                    }
                    _ => {}
                }
                let changed = next != before_w;
                let bytes_changed = bytes(doc.project()) != before_bytes;
                assert_eq!(
                    changed, bytes_changed,
                    "{ctx} : étape créée ssi l'état change ({command:?})"
                );
                if changed {
                    undo.push(before_w.clone());
                    redo.clear();
                }
                w = next;
                check(&ctx, doc.project(), sheet, &w);
                if changed {
                    // Annuler / rétablir exacts à l'octet.
                    let after = bytes(doc.project());
                    assert!(doc.undo(), "{ctx} : annuler");
                    assert_eq!(bytes(doc.project()), before_bytes, "{ctx} : annuler exact");
                    assert!(doc.redo(), "{ctx} : rétablir");
                    assert_eq!(bytes(doc.project()), after, "{ctx} : rétablir exact");
                }
            }
            (None, Err(_)) => {
                assert_eq!(
                    bytes(doc.project()),
                    before_bytes,
                    "{ctx} : trace laissée par un refus ({command:?})"
                );
                check(&ctx, doc.project(), sheet, &w);
            }
            (Some(_), Err(e)) => panic!("{ctx} : refus inattendu ({command:?}) : {e}"),
            (None, Ok(_)) => panic!("{ctx} : accepté alors que l'oracle refuse ({command:?})"),
        }
    }
    // Fin de séquence : relecture identique.
    let text = bytes(doc.project());
    let back = persist::from_json(&text).expect("relecture finale");
    assert_eq!(bytes(&back), text);
}

#[test]
fn campagne_adverse_constructions_maintenues() {
    let mut cov = Cov::default();
    for seq in 0..SEQUENCES {
        run(seq, &mut cov);
    }
    eprintln!("couverture : {cov:?}");
    assert!(cov.polar >= 400, "{cov:?}");
    assert!(cov.perp >= 200, "{cov:?}");
    assert!(cov.par >= 200, "{cov:?}");
    assert!(cov.linked >= 600, "{cov:?}");
    assert!(cov.unlinked_cmd >= 80, "{cov:?}");
    assert!(cov.propagated >= 600, "{cov:?}");
    assert!(cov.chained >= 150, "{cov:?}");
    assert!(cov.direct_unlink >= 250, "{cov:?}");
    assert!(cov.stale_refused >= 100, "{cov:?}");
    assert!(cov.degenerate_refused >= 30, "{cov:?}");
    assert!(cov.transforms >= 120, "{cov:?}");
    assert!(cov.deleted_refs >= 40, "{cov:?}");
    assert!(cov.reversed >= 70, "{cov:?}");
    assert!(
        cov.undone >= 200 && cov.redone >= 10 && cov.reloads >= 100,
        "{cov:?}"
    );
    assert!(cov.dead_ref_unlink >= 25, "{cov:?}");
}
