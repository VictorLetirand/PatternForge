//! PH-4b — campagne adverse des perçages au pas : 160 séquences à graine fixe sur des contours
//! aléatoires (polygones étoilés, secteurs à arc, bosse de Bézier, rectangle à bord haut en
//! Bézier), avec chaîne de tracés, pas / nombre / marges / distances / côtés / formes aléatoires,
//! entrées invalides (prédites par l'oracle), annulations et rétablissements.
//!
//! Oracle **indépendant** : chaque tracé est échantillonné **densément** (6000 points avec
//! tangente analytique écrite ici) dans le sens du contour lu dans le modèle (`outline`,
//! `reversed`) ; abscisses par cumul de longueurs de la polyligne ; position = point à l'abscisse
//! décalé de la normale intérieure (gauche d'un contour anti-horaire, orientation lue au lacet) ;
//! distance au bord par distance point-polyligne. Aucune fonction de `pf_geometry` ni de
//! `piece_marking_run` n'y sert à juger. Tolérances annoncées : 1e-3 mm sur les positions,
//! 1e-3° sur l'angle des fentes, 1e-3 mm sur la distance au bord.
//!
//! Propriétés : jamais de panique ; entrée invalide = refus, octets identiques, aucune étape ;
//! entrée valide = acceptée, mêmes positions que l'aperçu ; nombre de perçages, espacement réel,
//! positions, distance au bord et orientation conformes à l'oracle ; annuler / rétablir exacts ;
//! relecture JSON identique ; compteurs prouvant que les contrôles forts se sont exécutés.

use pf_core::persist;
use pf_core::*;
use pf_geometry::PointMm;
use std::f64::consts::PI;
use std::panic::{catch_unwind, AssertUnwindSafe};

const SEQUENCES: usize = 160;
const SAMPLES: usize = 6000;
const POS_TOL: f64 = 1e-3;
const ANGLE_TOL_DEG: f64 = 1e-3;

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

#[derive(Default, Debug)]
struct Counters {
    sequences: usize,
    runs_ok: usize,
    refused_predicted: usize,
    position_checks: usize,
    tangent_checks: usize,
    distance_upper_checks: usize,
    distance_exact_checks: usize,
    junction_candidates: usize,
    curved_chains: usize,
    whole_contours: usize,
    multi_edge_chains: usize,
    slots: usize,
    drills: usize,
    step_mode: usize,
    count_mode: usize,
    margins: usize,
    outside: usize,
    previews_equal: usize,
    undo_redo: usize,
    json_roundtrips: usize,
    selection_checks: usize,
    limit_refusals: usize,
    spacing_checks: usize,
    rebuilt: usize,
}

/// Échantillon dense d'un tracé : position et tangente unitaire analytique.
#[derive(Clone, Copy)]
struct Sample {
    x: f64,
    y: f64,
    tx: f64,
    ty: f64,
}

/// Un tracé du modèle (id) et son échantillonnage dans le sens naturel (départ → arrivée).
struct EdgeDef {
    id: EntityId,
    natural: Vec<Sample>,
    curved: bool,
}

fn seg_samples(a: (f64, f64), b: (f64, f64)) -> Vec<Sample> {
    let l = (b.0 - a.0).hypot(b.1 - a.1);
    let (tx, ty) = ((b.0 - a.0) / l, (b.1 - a.1) / l);
    vec![
        Sample {
            x: a.0,
            y: a.1,
            tx,
            ty,
        },
        Sample {
            x: b.0,
            y: b.1,
            tx,
            ty,
        },
    ]
}

fn arc_samples(c: (f64, f64), r: f64, a0_deg: f64, a1_deg: f64) -> Vec<Sample> {
    (0..=SAMPLES)
        .map(|i| {
            let a = (a0_deg + (a1_deg - a0_deg) * i as f64 / SAMPLES as f64).to_radians();
            Sample {
                x: c.0 + r * a.cos(),
                y: c.1 + r * a.sin(),
                tx: -a.sin(),
                ty: a.cos(),
            }
        })
        .collect()
}

fn bezier_samples(p: [(f64, f64); 4]) -> Vec<Sample> {
    (0..=SAMPLES)
        .map(|i| {
            let t = i as f64 / SAMPLES as f64;
            let u = 1.0 - t;
            let (b0, b1, b2, b3) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
            let (d0, d1, d2) = (3.0 * u * u, 6.0 * u * t, 3.0 * t * t);
            let x = b0 * p[0].0 + b1 * p[1].0 + b2 * p[2].0 + b3 * p[3].0;
            let y = b0 * p[0].1 + b1 * p[1].1 + b2 * p[2].1 + b3 * p[3].1;
            let dx = d0 * (p[1].0 - p[0].0) + d1 * (p[2].0 - p[1].0) + d2 * (p[3].0 - p[2].0);
            let dy = d0 * (p[1].1 - p[0].1) + d1 * (p[2].1 - p[1].1) + d2 * (p[3].1 - p[2].1);
            let l = dx.hypot(dy);
            Sample {
                x,
                y,
                tx: dx / l,
                ty: dy / l,
            }
        })
        .collect()
}

fn pt(p: (f64, f64)) -> PointMm {
    PointMm::new(p.0, p.1)
}

struct Case {
    p: Project,
    piece: PieceId,
    edges: Vec<EdgeDef>,
}

fn add_seg(p: &mut Project, s: SheetId, a: (f64, f64), b: (f64, f64)) -> EntityId {
    p.apply(Command::AddSegment {
        sheet: s,
        from: PointRef::New(pt(a)),
        to: PointRef::New(pt(b)),
    })
    .unwrap()
    .entities[0]
}

fn extract(p: &mut Project, s: SheetId, at: (f64, f64)) -> Option<PieceId> {
    p.apply(Command::ExtractPiece {
        sheet: s,
        source: PieceSource::Click { at: pt(at) },
        name: "Pièce".into(),
        detached: false,
    })
    .ok()
    .map(|o| o.pieces[0])
}

/// Contour aléatoire ; `None` quand le tirage ne donne pas de région fermée (tirage refait).
#[allow(clippy::needless_late_init)]
fn build(rng: &mut Rng, kind: usize) -> Option<Case> {
    let mut p = Project::new("Perçages adverses");
    let s = p.sheets[0].id;
    let origin = (rng.range(-300.0, 300.0), rng.range(-300.0, 300.0));
    let mut edges = Vec::new();
    let piece;
    match kind {
        // Polygone étoilé autour de l'origine (toujours simple).
        0 => {
            let n = 3 + rng.below(5);
            let step = 2.0 * PI / n as f64;
            let v: Vec<(f64, f64)> = (0..n)
                .map(|k| {
                    let a = step * (k as f64 + rng.range(-0.3, 0.3));
                    let r = rng.range(25.0, 90.0);
                    (origin.0 + r * a.cos(), origin.1 + r * a.sin())
                })
                .collect();
            let o = p
                .apply(Command::AddPolygon {
                    sheet: s,
                    vertices: v.iter().map(|q| PointRef::New(pt(*q))).collect(),
                    closed: true,
                    role: PathRole::default(),
                    layer: None,
                })
                .unwrap();
            for k in 0..n {
                edges.push(EdgeDef {
                    id: o.entities[k],
                    natural: seg_samples(v[k], v[(k + 1) % n]),
                    curved: false,
                });
            }
            piece = extract(&mut p, s, origin)?;
        }
        // Secteur : rayon, arc, rayon.
        1 => {
            let r = rng.range(30.0, 90.0);
            let a0 = rng.range(0.0, 360.0);
            let sweep = rng.range(40.0, 200.0);
            let at = |deg: f64| {
                (
                    origin.0 + r * deg.to_radians().cos(),
                    origin.1 + r * deg.to_radians().sin(),
                )
            };
            let first = add_seg(&mut p, s, origin, at(a0));
            let arc = p
                .apply(Command::AddArc {
                    sheet: s,
                    center: PointRef::New(pt(origin)),
                    radius_mm: r,
                    start_deg: a0,
                    end_deg: a0 + sweep,
                    ccw: true,
                })
                .unwrap()
                .entities[0];
            let last = add_seg(&mut p, s, at(a0 + sweep), origin);
            edges.push(EdgeDef {
                id: first,
                natural: seg_samples(origin, at(a0)),
                curved: false,
            });
            edges.push(EdgeDef {
                id: arc,
                natural: arc_samples(origin, r, a0, a0 + sweep),
                curved: true,
            });
            edges.push(EdgeDef {
                id: last,
                natural: seg_samples(at(a0 + sweep), origin),
                curved: false,
            });
            let mid = (a0 + sweep / 2.0).to_radians();
            piece = extract(
                &mut p,
                s,
                (
                    origin.0 + 0.3 * r * mid.cos(),
                    origin.1 + 0.3 * r * mid.sin(),
                ),
            )?;
        }
        // Corde et bosse de Bézier.
        2 => {
            let w = rng.range(60.0, 160.0);
            let (a, b) = (rng.range(0.1, 0.4), rng.range(0.6, 0.9));
            let (h1, h2) = (rng.range(20.0, 80.0), rng.range(20.0, 80.0));
            let (p0, p3) = (origin, (origin.0 + w, origin.1));
            let (c1, c2) = (
                (origin.0 + w * a, origin.1 + h1),
                (origin.0 + w * b, origin.1 + h2),
            );
            let chord = add_seg(&mut p, s, p0, p3);
            let curve = p
                .apply(Command::AddCurve {
                    sheet: s,
                    start: PointRef::New(pt(p0)),
                    c1: pt(c1),
                    c2: pt(c2),
                    end: PointRef::New(pt(p3)),
                })
                .unwrap()
                .entities[0];
            edges.push(EdgeDef {
                id: chord,
                natural: seg_samples(p0, p3),
                curved: false,
            });
            edges.push(EdgeDef {
                id: curve,
                natural: bezier_samples([p0, c1, c2, p3]),
                curved: true,
            });
            piece = extract(&mut p, s, (origin.0 + w / 2.0, origin.1 + 3.0))?;
        }
        // Rectangle dont le bord haut est une Bézier.
        _ => {
            let (w, h) = (rng.range(60.0, 150.0), rng.range(60.0, 120.0));
            let (x0, y0) = origin;
            let bottom = add_seg(&mut p, s, (x0, y0), (x0 + w, y0));
            let right = add_seg(&mut p, s, (x0 + w, y0), (x0 + w, y0 + h));
            let (c1, c2) = (
                (
                    x0 + w * rng.range(0.6, 0.9),
                    y0 + h + rng.range(-25.0, 30.0),
                ),
                (
                    x0 + w * rng.range(0.1, 0.4),
                    y0 + h + rng.range(-25.0, 30.0),
                ),
            );
            let top = p
                .apply(Command::AddCurve {
                    sheet: s,
                    start: PointRef::New(pt((x0 + w, y0 + h))),
                    c1: pt(c1),
                    c2: pt(c2),
                    end: PointRef::New(pt((x0, y0 + h))),
                })
                .unwrap()
                .entities[0];
            let left = add_seg(&mut p, s, (x0, y0 + h), (x0, y0));
            edges.push(EdgeDef {
                id: bottom,
                natural: seg_samples((x0, y0), (x0 + w, y0)),
                curved: false,
            });
            edges.push(EdgeDef {
                id: right,
                natural: seg_samples((x0 + w, y0), (x0 + w, y0 + h)),
                curved: false,
            });
            edges.push(EdgeDef {
                id: top,
                natural: bezier_samples([(x0 + w, y0 + h), c1, c2, (x0, y0 + h)]),
                curved: true,
            });
            edges.push(EdgeDef {
                id: left,
                natural: seg_samples((x0, y0 + h), (x0, y0)),
                curved: false,
            });
            piece = extract(&mut p, s, (x0 + w / 2.0, y0 + h / 2.0))?;
        }
    }
    Some(Case { p, piece, edges })
}

/// Jonction vive : abscisse, tangente d'avant, tangente d'après.
type Junction = (f64, (f64, f64), (f64, f64));

/// Bord parcouru dans le sens du contour : polyligne dense, abscisses cumulées, jonctions.
struct Chain {
    pts: Vec<Sample>,
    cum: Vec<f64>,
    /// Jonctions vives : abscisse, tangente d'avant, tangente d'après.
    junctions: Vec<Junction>,
    total: f64,
    whole: bool,
    curved: bool,
    edges: usize,
}

fn travel(edge: &EdgeDef, reversed: bool) -> Vec<Sample> {
    if reversed {
        edge.natural
            .iter()
            .rev()
            .map(|s| Sample {
                tx: -s.tx,
                ty: -s.ty,
                ..*s
            })
            .collect()
    } else {
        edge.natural.clone()
    }
}

fn chain_of(case: &Case, ids: &[EntityId]) -> Chain {
    let outline = &case.p.piece(case.piece).unwrap().outline;
    let mut pts: Vec<Sample> = Vec::new();
    let mut cum = Vec::new();
    let mut junctions = Vec::new();
    let mut total = 0.0;
    let mut curved = false;
    for id in ids {
        let oe = outline.iter().find(|e| e.entity == *id).unwrap();
        let def = case.edges.iter().find(|e| e.id == *id).unwrap();
        curved |= def.curved;
        let t = travel(def, oe.reversed);
        for (i, q) in t.iter().enumerate() {
            if let Some(prev) = pts.last().filter(|_| i == 0) {
                // Raccord : les extrémités se touchent (même point).
                assert!((prev.x - q.x).hypot(prev.y - q.y) < 1e-6, "chaîne rompue");
                junctions.push((total, (prev.tx, prev.ty), (q.tx, q.ty)));
            }
            if let Some(prev) = pts.last() {
                total += (q.x - prev.x).hypot(q.y - prev.y);
            }
            pts.push(*q);
            cum.push(total);
        }
    }
    let whole = ids.len() == outline.len();
    if whole {
        // Contour entier : la fermeture est une jonction, au début comme à la fin.
        let (first, last) = (pts[0], pts[pts.len() - 1]);
        for at in [0.0, total] {
            junctions.push((at, (last.tx, last.ty), (first.tx, first.ty)));
        }
    }
    Chain {
        pts,
        cum,
        junctions,
        total,
        whole,
        curved,
        edges: ids.len(),
    }
}

/// Orientation du contour au lacet sur les polylignes denses, dans le sens du modèle.
fn is_ccw(case: &Case) -> bool {
    let outline = &case.p.piece(case.piece).unwrap().outline;
    let mut poly: Vec<(f64, f64)> = Vec::new();
    for oe in outline {
        let def = case.edges.iter().find(|e| e.id == oe.entity).unwrap();
        poly.extend(travel(def, oe.reversed).iter().map(|s| (s.x, s.y)));
    }
    let n = poly.len();
    let a: f64 = (0..n)
        .map(|i| poly[i].0 * poly[(i + 1) % n].1 - poly[(i + 1) % n].0 * poly[i].1)
        .sum();
    a > 0.0
}

impl Chain {
    /// Point et tangentes candidates à l'abscisse `s` (deux tangentes à une jonction vive).
    fn at(&self, s: f64) -> ((f64, f64), Vec<(f64, f64)>) {
        let s = s.clamp(0.0, self.total);
        let k = self
            .cum
            .partition_point(|c| *c < s)
            .clamp(1, self.pts.len() - 1);
        let (a, b) = (self.pts[k - 1], self.pts[k]);
        let l = self.cum[k] - self.cum[k - 1];
        let f = if l > 0.0 {
            (s - self.cum[k - 1]) / l
        } else {
            0.0
        };
        let pos = (a.x + (b.x - a.x) * f, a.y + (b.y - a.y) * f);
        let lerp = |a: &Sample, b: &Sample, f: f64| {
            let (tx, ty) = (a.tx + (b.tx - a.tx) * f, a.ty + (b.ty - a.ty) * f);
            let n = tx.hypot(ty);
            (tx / n, ty / n)
        };
        let mut tangents = vec![lerp(&a, &b, f)];
        // Jonction vive : la tangente d'avant et celle d'après sont toutes deux admises.
        for (at, before, after) in &self.junctions {
            if (at - s).abs() < 1e-7 {
                tangents.push(*before);
                tangents.push(*after);
            }
        }
        (pos, tangents)
    }

    fn distance_to(&self, p: (f64, f64)) -> f64 {
        self.pts
            .windows(2)
            .map(|w| {
                let (ax, ay, bx, by) = (w[0].x, w[0].y, w[1].x, w[1].y);
                let (vx, vy) = (bx - ax, by - ay);
                let l2 = vx * vx + vy * vy;
                let t = if l2 > 0.0 {
                    (((p.0 - ax) * vx + (p.1 - ay) * vy) / l2).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                (p.0 - ax - vx * t).hypot(p.1 - ay - vy * t)
            })
            .fold(f64::INFINITY, f64::min)
    }
}

/// Prédiction de l'oracle : nombre de perçages si la rangée est valide, sinon `None`.
fn predict(sp: &MarkingRunSpec, chain: &Chain, existing: usize) -> Option<usize> {
    let ok_dist = sp.distance_mm.is_finite() && (0.0..=100.0).contains(&sp.distance_mm);
    let ok_margin = |m: f64| m.is_finite() && m >= 0.0;
    let ok_form = match sp.form {
        RunForm::Drill { diameter_mm } => {
            diameter_mm.is_finite() && (0.1..=100.0).contains(&diameter_mm)
        }
        RunForm::Slot {
            length_mm,
            width_mm,
        } => {
            width_mm.is_finite()
                && (0.1..=100.0).contains(&width_mm)
                && length_mm.is_finite()
                && length_mm >= width_mm
                && length_mm <= 100.0
        }
    };
    if !(ok_dist && ok_margin(sp.start_margin_mm) && ok_margin(sp.end_margin_mm) && ok_form) {
        return None;
    }
    let span = chain.total - sp.start_margin_mm - sp.end_margin_mm;
    let count = match sp.spacing {
        RunSpacing::Step { step_mm } => {
            if !(step_mm.is_finite() && (0.5..=1000.0).contains(&step_mm)) || span <= 0.001 {
                return None;
            }
            let q = span / step_mm;
            let n = (q - 1e-9 * q.max(1.0)).ceil() as usize + 1;
            if n > 500 {
                return None;
            }
            n
        }
        RunSpacing::Count { count } => {
            if !(1..=500).contains(&count) || span < 0.0 {
                return None;
            }
            if count > 1 && span / (count - 1) as f64 <= 0.0011 {
                return None;
            }
            count as usize
        }
    };
    if chain.whole && count > 1 && sp.start_margin_mm + sp.end_margin_mm < 0.001 {
        return None;
    }
    (existing + count <= 1000).then_some(count)
}

fn random_form(rng: &mut Rng) -> RunForm {
    if rng.chance(40) {
        let w = rng.range(0.5, 5.0);
        RunForm::Slot {
            length_mm: w + rng.range(0.0, 10.0),
            width_mm: w,
        }
    } else {
        RunForm::Drill {
            diameter_mm: rng.range(0.1, 10.0),
        }
    }
}

fn random_valid(rng: &mut Rng, ids: &[EntityId], total: f64) -> MarkingRunSpec {
    let margin = |rng: &mut Rng| {
        if rng.chance(50) {
            0.0
        } else {
            total * rng.range(0.0, 0.3)
        }
    };
    let (d0, f) = (margin(rng), margin(rng));
    let span = total - d0 - f;
    let spacing = if rng.chance(50) {
        let k = 1 + rng.below(80);
        RunSpacing::Step {
            step_mm: (span / k as f64 * rng.range(0.9, 1.1)).max(0.5),
        }
    } else {
        RunSpacing::Count {
            count: 1 + rng.below(60) as u32,
        }
    };
    MarkingRunSpec {
        edges: ids.to_vec(),
        spacing,
        start_margin_mm: d0,
        end_margin_mm: f,
        distance_mm: if rng.chance(10) {
            0.0
        } else {
            rng.range(0.0, 30.0)
        },
        side: if rng.chance(25) {
            RunSide::Outside
        } else {
            RunSide::Inside
        },
        form: random_form(rng),
    }
}

/// Une entrée invalide (l'oracle la prédit invalide) à partir d'une valide.
fn corrupt(rng: &mut Rng, sp: &mut MarkingRunSpec, total: f64, outline: &[PieceEdge]) -> bool {
    let mut edges_bad = false;
    match rng.below(12) {
        0 => sp.distance_mm = [-1.0, 100.5, f64::NAN, f64::INFINITY][rng.below(4)],
        1 => {
            sp.spacing = RunSpacing::Step {
                step_mm: [0.0, 0.3, 1000.5, f64::NAN, -2.0][rng.below(5)],
            }
        }
        2 => {
            sp.spacing = RunSpacing::Count {
                count: [0, 501, 100_000][rng.below(3)],
            }
        }
        3 => sp.start_margin_mm = [-1.0, f64::NAN, f64::INFINITY][rng.below(3)],
        4 => sp.end_margin_mm = [-0.5, f64::NAN][rng.below(2)],
        5 => {
            sp.start_margin_mm = total * rng.range(0.6, 0.9);
            sp.end_margin_mm = total * rng.range(0.6, 0.9);
        }
        6 => {
            sp.form = RunForm::Drill {
                diameter_mm: [0.0, 0.05, 100.5, f64::NAN][rng.below(4)],
            }
        }
        7 => {
            sp.form = RunForm::Slot {
                length_mm: 1.0,
                width_mm: 2.0,
            }
        }
        8 => {
            sp.form = RunForm::Slot {
                length_mm: [100.5, f64::NAN][rng.below(2)],
                width_mm: 2.0,
            }
        }
        9 => {
            sp.edges = vec![EntityId(987_654_321)];
            edges_bad = true;
        }
        10 => {
            sp.edges = vec![];
            edges_bad = true;
        }
        _ => {
            // Deux tracés qui ne se suivent pas dans le contour (s'il y en a quatre ou plus).
            if outline.len() >= 4 {
                sp.edges = vec![outline[0].entity, outline[2].entity];
            } else {
                sp.edges = vec![EntityId(987_654_322)];
            }
            edges_bad = true;
        }
    }
    edges_bad
}

fn snap(p: &Project) -> String {
    persist::to_json(p).unwrap()
}

#[test]
fn campagne_adverse_percages_au_pas() {
    let mut c = Counters::default();
    for seq in 0..SEQUENCES {
        let mut attempt = 0u64;
        let (mut rng, case) = loop {
            let mut rng = Rng(0xFEED_0000 + seq as u64 * 7_919 + attempt * 1_000_003);
            if let Some(case) = build(&mut rng, seq % 4) {
                break (rng, case);
            }
            attempt += 1;
            c.rebuilt += 1;
            assert!(attempt < 20, "séq {seq} : aucun contour valide");
        };
        c.sequences += 1;
        let ccw = is_ccw(&case);
        assert!(ccw, "séq {seq} : une pièce extraite est anti-horaire");
        let outline = case.p.piece(case.piece).unwrap().outline.clone();
        let n = outline.len();
        let mut doc = Document::new(case.p.clone());
        let mut stack = vec![snap(doc.project())];
        let runs = if seq % 20 == 7 { 3 } else { 1 + rng.below(3) };
        let mut existing = 0usize;
        for run in 0..runs {
            // Chaîne : une suite continue du contour (le tour de la fermeture permis).
            let (start, count) = match rng.below(4) {
                0 => (0, n),
                _ => (rng.below(n), 1 + rng.below(n)),
            };
            let ids: Vec<EntityId> = (0..count)
                .map(|k| outline[(start + k) % n].entity)
                .collect();
            let chain = chain_of(&case, &ids);
            // Sélection → bord : même suite, quel que soit l'ordre de la sélection.
            let mut shuffled = ids.clone();
            for i in (1..shuffled.len()).rev() {
                shuffled.swap(i, rng.below(i + 1));
            }
            let from_selection = doc
                .project()
                .marking_run_edges(case.piece, &shuffled)
                .unwrap();
            let expected_ids: Vec<EntityId> = if count == n {
                outline.iter().map(|e| e.entity).collect()
            } else {
                ids.clone()
            };
            assert_eq!(from_selection, expected_ids, "séq {seq} sélection");
            c.selection_checks += 1;
            let chain = if count == n {
                chain_of(&case, &expected_ids)
            } else {
                chain
            };

            let mut sp = random_valid(&mut rng, &expected_ids, chain.total);
            let invalid = rng.chance(25);
            let edges_bad = invalid && corrupt(&mut rng, &mut sp, chain.total, &outline);
            // Cas limite : trois rangées de 500 sur la même pièce (la troisième dépasse 1000).
            if seq % 20 == 7 {
                sp.spacing = RunSpacing::Count { count: 500 };
                sp.start_margin_mm = 0.0;
                sp.end_margin_mm = if chain.whole { 1.0 } else { 0.0 };
            }
            let predicted = if edges_bad {
                None
            } else {
                predict(&sp, &chain, existing)
            };
            let before = snap(doc.project());
            let undo_before = stack.len();
            let cmd = Command::AddPieceMarkingRun {
                piece: case.piece,
                run: sp.clone(),
            };
            let preview = catch_unwind(AssertUnwindSafe(|| {
                doc.project().marking_run_preview(case.piece, &sp)
            }))
            .unwrap_or_else(|_| panic!("séq {seq} run {run} : panique à l'aperçu"));
            let result = catch_unwind(AssertUnwindSafe(|| doc.execute(cmd)))
                .unwrap_or_else(|_| panic!("séq {seq} run {run} : panique à la création {sp:?}"));
            match (predicted, result) {
                (None, Err(_)) => {
                    assert!(
                        preview.is_err(),
                        "séq {seq} : l'aperçu devait refuser {sp:?}"
                    );
                    assert_eq!(snap(doc.project()), before, "séq {seq} : refus sans trace");
                    c.refused_predicted += 1;
                    if existing >= 600 && matches!(sp.spacing, RunSpacing::Count { count: 500 }) {
                        c.limit_refusals += 1;
                    }
                }
                (None, Ok(_)) => panic!("séq {seq} run {run} : refus attendu, accepté {sp:?}"),
                (Some(want), Err(e)) => {
                    panic!("séq {seq} run {run} : {want} perçages attendus, refus « {e} » {sp:?}")
                }
                (Some(want), Ok(o)) => {
                    c.runs_ok += 1;
                    assert_eq!(o.markings.len(), want, "séq {seq} run {run} : nombre");
                    let preview =
                        preview.unwrap_or_else(|e| panic!("séq {seq} : aperçu refusé : {e}"));
                    assert_eq!(preview.holes.len(), want);
                    let all = doc.project().piece_markings(case.piece);
                    let new = &all[all.len() - want..];
                    assert_eq!(all.len(), existing + want);
                    // Espacement réel : (longueur − marges) / (n − 1), jamais plus que le pas demandé.
                    let span = chain.total - sp.start_margin_mm - sp.end_margin_mm;
                    if want > 1 {
                        let real = span / (want - 1) as f64;
                        assert!(
                            (preview.step_mm - real).abs() < 1e-4,
                            "séq {seq} espacement"
                        );
                        if let RunSpacing::Step { step_mm } = sp.spacing {
                            assert!(real <= step_mm + 1e-9, "séq {seq} : {real} > {step_mm}");
                            c.step_mode += 1;
                        } else {
                            c.count_mode += 1;
                        }
                        c.spacing_checks += 1;
                    } else {
                        c.count_mode += 1;
                    }
                    if chain.curved {
                        c.curved_chains += 1;
                    }
                    if chain.whole {
                        c.whole_contours += 1;
                    }
                    if chain.edges > 1 {
                        c.multi_edge_chains += 1;
                    }
                    if sp.start_margin_mm > 0.0 || sp.end_margin_mm > 0.0 {
                        c.margins += 1;
                    }
                    if sp.side == RunSide::Outside {
                        c.outside += 1;
                    }
                    for (i, r) in new.iter().enumerate() {
                        let s = if want == 1 {
                            sp.start_margin_mm + span / 2.0
                        } else {
                            sp.start_margin_mm + span * i as f64 / (want - 1) as f64
                        };
                        let (pos, tangents) = chain.at(s);
                        if tangents.len() > 1 {
                            c.junction_candidates += 1;
                        }
                        // Positions admises : une par tangente candidate (normale à gauche).
                        let sign = if sp.side == RunSide::Inside {
                            1.0
                        } else {
                            -1.0
                        };
                        let found = tangents.iter().any(|(tx, ty)| {
                            let (ex, ey) = (
                                pos.0 - ty * sp.distance_mm * sign,
                                pos.1 + tx * sp.distance_mm * sign,
                            );
                            (r.at.x - ex).hypot(r.at.y - ey) <= POS_TOL
                        });
                        assert!(
                            found,
                            "séq {seq} run {run} perçage {i}/{want} : {:?} loin de l'oracle {pos:?} {tangents:?} s={s} total={} chain edges={} whole={} outline={:?} sp={sp:?}",
                            r.at, chain.total, chain.edges, chain.whole, outline
                        );
                        c.position_checks += 1;
                        assert!(
                            (preview.holes[i].position.x - r.at.x)
                                .hypot(preview.holes[i].position.y - r.at.y)
                                < 1e-12
                        );
                        // Distance au bord : jamais plus que demandée (le pied de la normale est
                        // sur le bord), exactement la distance sur un tracé unique rectiligne.
                        if i % 7 == 0 {
                            let dist = chain.distance_to((r.at.x, r.at.y));
                            assert!(
                                dist <= sp.distance_mm + POS_TOL,
                                "séq {seq} : distance {dist} > {}",
                                sp.distance_mm
                            );
                            c.distance_upper_checks += 1;
                            if chain.edges == 1 && !chain.curved {
                                assert!(
                                    (dist - sp.distance_mm).abs() <= POS_TOL,
                                    "séq {seq} : distance {dist} ≠ {}",
                                    sp.distance_mm
                                );
                                c.distance_exact_checks += 1;
                            }
                        }
                        match (r.marking.shape, sp.form) {
                            (
                                MarkingShape::Drill { diameter_mm },
                                RunForm::Drill { diameter_mm: d },
                            ) => {
                                assert_eq!(diameter_mm, d);
                                c.drills += 1;
                            }
                            (
                                MarkingShape::Slot {
                                    length_mm,
                                    width_mm,
                                    angle_deg,
                                },
                                RunForm::Slot {
                                    length_mm: l,
                                    width_mm: w,
                                },
                            ) => {
                                assert_eq!((length_mm, width_mm), (l, w));
                                assert!((0.0..360.0).contains(&angle_deg));
                                let ok = tangents.iter().any(|(tx, ty)| {
                                    let mut a = ty.atan2(*tx).to_degrees();
                                    if a < 0.0 {
                                        a += 360.0;
                                    }
                                    let d = (a - angle_deg).abs();
                                    d.min(360.0 - d) <= ANGLE_TOL_DEG
                                });
                                assert!(ok, "séq {seq} : angle {angle_deg} {tangents:?}");
                                c.slots += 1;
                                c.tangent_checks += 1;
                            }
                            other => panic!("forme inattendue {other:?}"),
                        }
                    }
                    c.previews_equal += 1;
                    existing += want;
                    stack.push(snap(doc.project()));
                    assert_eq!(stack.len(), undo_before + 1, "une étape par rangée");
                    // Relecture JSON identique.
                    let text = snap(doc.project());
                    let back = persist::from_json(&text).unwrap();
                    assert_eq!(snap(&back), text, "séq {seq} : relecture JSON");
                    assert_eq!(back, *doc.project());
                    c.json_roundtrips += 1;
                }
            }
        }
        // Annuler tout, niveau par niveau, puis tout rétablir : octets identiques.
        for level in (0..stack.len() - 1).rev() {
            assert!(doc.undo(), "séq {seq} : annuler");
            assert_eq!(
                snap(doc.project()),
                stack[level],
                "séq {seq} : annuler niveau {level}"
            );
        }
        assert!(!doc.can_undo(), "séq {seq} : trop d'étapes");
        for (level, expected) in stack.iter().enumerate().skip(1) {
            assert!(doc.redo(), "séq {seq} : rétablir");
            assert_eq!(
                snap(doc.project()),
                *expected,
                "séq {seq} : rétablir niveau {level}"
            );
        }
        c.undo_redo += 1;
    }
    eprintln!("{c:#?}");
    // Les contrôles forts se sont exécutés.
    assert_eq!(c.sequences, SEQUENCES);
    assert!(c.runs_ok >= 150, "{c:?}");
    assert!(c.refused_predicted >= 40, "{c:?}");
    assert!(c.position_checks >= 2000, "{c:?}");
    assert!(c.tangent_checks >= 500, "{c:?}");
    assert!(
        c.distance_upper_checks >= 200 && c.distance_exact_checks >= 30,
        "{c:?}"
    );
    assert!(
        c.curved_chains >= 60 && c.whole_contours >= 10 && c.multi_edge_chains >= 60,
        "{c:?}"
    );
    assert!(
        c.slots > 0 && c.drills > 0 && c.step_mode >= 40 && c.count_mode >= 40,
        "{c:?}"
    );
    assert!(c.margins >= 40 && c.outside >= 20, "{c:?}");
    assert!(c.junction_candidates >= 10, "{c:?}");
    assert!(
        c.undo_redo == SEQUENCES && c.json_roundtrips >= 150,
        "{c:?}"
    );
    assert!(
        c.selection_checks >= 150 && c.spacing_checks >= 100,
        "{c:?}"
    );
    assert!(c.limit_refusals >= 1, "{c:?}");
    assert!(
        c.rebuilt <= SEQUENCES / 5,
        "trop de contours refaits : {c:?}"
    );
}
