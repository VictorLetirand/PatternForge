//! US-07.3 : campagne adverse des plis, 180 séquences à graine fixe. Chaque séquence crée un polygone
//! convexe aléatoire (3 à 6 sommets sur une ellipse) extrait en pièce, puis enchaîne des poses de
//! plis (plat / creux, côté, profondeur, nombre, étendue, direction aléatoire ou **la même** que
//! le pli précédent, ligne retournée), des commandes invalides, des annulations et rétablissements.
//!
//! **Oracle indépendant** : il ne lit rien du code testé. Il tient son propre polygone (liste de
//! sommets anti-horaires) et sa propre liste de plis ; il écarte le polygone par un balayage des
//! arêtes (insertion des points de coupe `I` et `I + W·d`, translation des sommets du côté du sens),
//! décide seul si la pose est acceptée (ligne qui traverse, plis parallèles, bandes disjointes) et,
//! si oui, compare : ensemble des sommets (segments alignés fusionnés), aire (formule du lacet),
//! largeur ajoutée = somme des largeurs, coordonnées des lignes de pli stockées, **pièce repliée =
//! polygone d'origine à 1e-6 mm**, repères (extrémités sur la ligne attendue ET sur le contour).
//! Les cas à moins de 1e-3 mm d'une frontière de décision sont **écartés** (et comptés). Des
//! compteurs prouvent que les contrôles forts s'exécutent. Mutations vérifiées : voir
//! `docs/JOURNAL.md` (tranche 63).

use pf_core::persist;
use pf_core::*;
use pf_geometry::{PointMm, Side};

struct Rng(u64);

impl Rng {
    fn step(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn unit(&mut self) -> f64 {
        (self.step() >> 11) as f64 / (1u64 << 53) as f64
    }
    fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.unit()
    }
    fn below(&mut self, n: u64) -> u64 {
        self.step() % n
    }
    fn chance(&mut self, percent: u64) -> bool {
        self.below(100) < percent
    }
}

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

fn bytes(p: &Project) -> String {
    persist::to_json(p).unwrap()
}

#[derive(Default, Debug)]
struct Counters {
    sequences: usize,
    attempts: usize,
    accepted: usize,
    refused_geometry: usize,
    refused_invalid: usize,
    ambiguous_skipped: usize,
    refusals_without_trace: usize,
    vertex_sets_checked: usize,
    areas_checked: usize,
    widths_checked: usize,
    stored_lines_checked: usize,
    folded_checked: usize,
    marks_checked: usize,
    undo_redo_checks: usize,
    multi_accepted: usize,
    hollow_accepted: usize,
    chained: usize,
    flipped_lines: usize,
    json_roundtrips: usize,
}

// ---------------------------------------------------------------- oracle

fn sub(a: PointMm, b: PointMm) -> (f64, f64) {
    (a.x - b.x, a.y - b.y)
}

fn dist(a: PointMm, b: PointMm) -> f64 {
    let (dx, dy) = sub(a, b);
    dx.hypot(dy)
}

fn seg_point_dist(p: PointMm, a: PointMm, b: PointMm) -> f64 {
    let (vx, vy) = sub(b, a);
    let l2 = vx * vx + vy * vy;
    let t = if l2 == 0.0 {
        0.0
    } else {
        (((p.x - a.x) * vx + (p.y - a.y) * vy) / l2).clamp(0.0, 1.0)
    };
    dist(p, pt(a.x + t * vx, a.y + t * vy))
}

fn boundary_dist(poly: &[PointMm], p: PointMm) -> f64 {
    (0..poly.len())
        .map(|i| seg_point_dist(p, poly[i], poly[(i + 1) % poly.len()]))
        .fold(f64::INFINITY, f64::min)
}

fn shoelace(poly: &[PointMm]) -> f64 {
    let n = poly.len();
    0.5 * (0..n)
        .map(|i| poly[i].x * poly[(i + 1) % n].y - poly[(i + 1) % n].x * poly[i].y)
        .sum::<f64>()
}

/// Retire les sommets alignés avec leurs voisins (à 1e-7 mm).
fn simplified(poly: &[PointMm]) -> Vec<PointMm> {
    let mut v = poly.to_vec();
    loop {
        let n = v.len();
        if n <= 3 {
            return v;
        }
        let mut removed = false;
        for i in 0..n {
            let (p, q, r) = (v[(i + n - 1) % n], v[i], v[(i + 1) % n]);
            let (ux, uy) = sub(r, p);
            let len = ux.hypot(uy);
            if len < 1e-12 {
                v.remove(i);
                removed = true;
                break;
            }
            let off = (ux * (q.y - p.y) - uy * (q.x - p.x)).abs() / len;
            let (qx, qy) = sub(q, p);
            let along = (ux * qx + uy * qy) / len;
            if off < 1e-7 && along > 0.0 && along < len {
                v.remove(i);
                removed = true;
                break;
            }
        }
        if !removed {
            return v;
        }
    }
}

fn same_set(got: &[PointMm], want: &[PointMm], tol: f64) -> bool {
    got.len() == want.len()
        && want
            .iter()
            .all(|w| got.iter().filter(|g| dist(**g, *w) <= tol).count() == 1)
}

#[derive(Clone, Copy)]
struct OPleat {
    a: PointMm,
    ux: f64,
    uy: f64,
    sense: Side,
    kind: PleatKind,
    depth: f64,
}

impl OPleat {
    fn normal(&self) -> (f64, f64) {
        match self.sense {
            Side::Left => (-self.uy, self.ux),
            Side::Right => (self.uy, -self.ux),
        }
    }
    fn width(&self) -> f64 {
        match self.kind {
            PleatKind::Flat => 2.0 * self.depth,
            PleatKind::Hollow => 4.0 * self.depth,
        }
    }
}

#[derive(Clone)]
struct OState {
    poly: Vec<PointMm>,
    pleats: Vec<OPleat>,
}

enum Verdict {
    /// Nouvel état et aire gagnée = Σ largeur × corde.
    Ok(OState, f64),
    Refuse,
    Ambiguous,
}

/// Pose d'un seul pli sur l'état de l'oracle.
fn o_apply(
    st: &OState,
    a: PointMm,
    b: PointMm,
    sense: Side,
    kind: PleatKind,
    depth: f64,
) -> Verdict {
    let (ux, uy) = {
        let (dx, dy) = sub(b, a);
        let l = dx.hypot(dy);
        (dx / l, dy / l)
    };
    let new = OPleat {
        a,
        ux,
        uy,
        sense,
        kind,
        depth,
    };
    let (dx, dy) = new.normal();
    let w = new.width();
    let s_of = |p: PointMm| (p.x - a.x) * dx + (p.y - a.y) * dy;
    let ss: Vec<f64> = st.poly.iter().map(|v| s_of(*v)).collect();
    if ss.iter().any(|s| s.abs() < 1e-3) {
        return Verdict::Ambiguous;
    }
    if ss.iter().all(|s| *s < 0.0) || ss.iter().all(|s| *s > 0.0) {
        return Verdict::Refuse;
    }
    let mut pleats = Vec::new();
    for e in &st.pleats {
        let cr = ux * e.uy - uy * e.ux;
        if cr.abs() > 1e-5 {
            return Verdict::Refuse;
        }
        if cr.abs() > 1e-9 {
            return Verdict::Ambiguous;
        }
        let (edx, edy) = e.normal();
        let s_e = s_of(e.a);
        let we = e.width();
        let (lo, hi) = if dx * edx + dy * edy > 0.0 {
            (s_e, s_e + we)
        } else {
            (s_e - we, s_e)
        };
        if (lo - w).abs() < 1e-3 || hi.abs() < 1e-3 {
            return Verdict::Ambiguous;
        }
        if lo > w {
            pleats.push(OPleat {
                a: pt(e.a.x + dx * w, e.a.y + dy * w),
                ..*e
            });
        } else if hi < 0.0 {
            pleats.push(*e);
        } else {
            return Verdict::Refuse;
        }
    }
    let n = st.poly.len();
    let mut out = Vec::new();
    let mut crossings = Vec::new();
    for i in 0..n {
        let (p, q) = (st.poly[i], st.poly[(i + 1) % n]);
        let (sp, sq) = (ss[i], ss[(i + 1) % n]);
        out.push(if sp > 0.0 {
            pt(p.x + dx * w, p.y + dy * w)
        } else {
            p
        });
        if sp * sq < 0.0 {
            let t = sp / (sp - sq);
            let ip = pt(p.x + t * (q.x - p.x), p.y + t * (q.y - p.y));
            let moved = pt(ip.x + dx * w, ip.y + dy * w);
            crossings.push(ip);
            if sp < 0.0 {
                out.push(ip);
                out.push(moved);
            } else {
                out.push(moved);
                out.push(ip);
            }
        }
    }
    pleats.push(new);
    let chord = dist(crossings[0], crossings[1]);
    Verdict::Ok(OState { poly: out, pleats }, w * chord)
}

/// Pose de `count` plis répartis : le plus éloigné d'abord.
#[allow(clippy::too_many_arguments)]
fn o_batch(
    st: &OState,
    a: PointMm,
    b: PointMm,
    sense: Side,
    kind: PleatKind,
    depth: f64,
    count: u32,
    span: f64,
) -> Verdict {
    let (ux, uy) = {
        let (dx, dy) = sub(b, a);
        let l = dx.hypot(dy);
        (dx / l, dy / l)
    };
    let probe = OPleat {
        a,
        ux,
        uy,
        sense,
        kind,
        depth,
    };
    let (dx, dy) = probe.normal();
    let w = probe.width();
    let step = if count > 1 {
        span / f64::from(count - 1)
    } else {
        0.0
    };
    if count > 1 {
        if (step - w).abs() < 1e-3 {
            return Verdict::Ambiguous;
        }
        if step < w {
            return Verdict::Refuse;
        }
    }
    let mut cur = st.clone();
    let mut gain = 0.0;
    for k in (0..count).rev() {
        let off = step * f64::from(k);
        let (ka, kb) = (
            pt(a.x + dx * off, a.y + dy * off),
            pt(b.x + dx * off, b.y + dy * off),
        );
        match o_apply(&cur, ka, kb, sense, kind, depth) {
            Verdict::Ok(next, g) => {
                cur = next;
                gain += g;
            }
            other => return other,
        }
    }
    Verdict::Ok(cur, gain)
}

// ---------------------------------------------------------------- implémentation observée

fn outline_vertices(p: &Project, piece: PieceId) -> Vec<PointMm> {
    let sheet = p.piece_sheet(piece).unwrap();
    p.piece(piece)
        .unwrap()
        .outline
        .iter()
        .map(|e| match sheet.shape(e.entity).unwrap() {
            Shape::Line(a, b) => {
                if e.reversed {
                    b
                } else {
                    a
                }
            }
            other => panic!("segment attendu : {other:?}"),
        })
        .collect()
}

fn random_polygon(r: &mut Rng) -> Vec<PointMm> {
    loop {
        let k = 3 + r.below(4) as usize;
        let (cx, cy) = (r.range(-300.0, 300.0), r.range(-300.0, 300.0));
        let (rx, ry) = (r.range(80.0, 220.0), r.range(60.0, 160.0));
        let mut angles: Vec<f64> = (0..k)
            .map(|_| r.range(0.0, std::f64::consts::TAU))
            .collect();
        angles.sort_by(f64::total_cmp);
        let gaps_ok = (0..k).all(|i| {
            let gap = if i + 1 < k {
                angles[i + 1] - angles[i]
            } else {
                angles[0] + std::f64::consts::TAU - angles[i]
            };
            (0.45..std::f64::consts::PI - 0.1).contains(&gap)
        });
        if gaps_ok {
            return angles
                .iter()
                .map(|t| pt(cx + rx * t.cos(), cy + ry * t.sin()))
                .collect();
        }
    }
}

fn build(poly: &[PointMm]) -> (Project, PieceId) {
    let mut p = Project::new("Adverse plis");
    let s = p.sheets[0].id;
    let n = poly.len();
    let ids = (0..n)
        .map(|i| {
            p.apply(Command::AddSegment {
                sheet: s,
                from: PointRef::New(poly[i]),
                to: PointRef::New(poly[(i + 1) % n]),
            })
            .unwrap()
            .entities[0]
        })
        .collect();
    let piece = p
        .apply(Command::ExtractPiece {
            sheet: s,
            source: PieceSource::Paths { entities: ids },
            name: "Adverse".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    (p, piece)
}

struct Proposal {
    a: PointMm,
    b: PointMm,
    sense: Side,
    kind: PleatKind,
    depth: f64,
    count: u32,
    span: f64,
}

fn command(piece: PieceId, q: &Proposal) -> Command {
    Command::AddPiecePleats {
        piece,
        from: q.a,
        to: q.b,
        depth_mm: q.depth,
        kind: q.kind,
        sense: q.sense,
        count: q.count,
        span_mm: q.span,
    }
}

/// Contrôles forts d'un état accepté.
#[allow(clippy::too_many_arguments)]
fn check_state(
    seed: u64,
    c: &mut Counters,
    proj: &Project,
    piece: PieceId,
    st: &OState,
    original: &[PointMm],
    batch_added: f64,
    area_before: f64,
) {
    // 1. Ensemble des sommets (segments alignés fusionnés).
    let want = simplified(&st.poly);
    let got = outline_vertices(proj, piece);
    assert!(
        same_set(&got, &want, 1e-6),
        "seed {seed} : sommets {got:?} contre {want:?}"
    );
    c.vertex_sets_checked += 1;
    // 2. Aire (lacet) et aire gagnée = Σ largeur × corde : largeur ajoutée.
    let area = proj.piece_contour(piece).unwrap().area_mm2;
    assert!(
        (area - shoelace(&st.poly)).abs() < 1e-5,
        "seed {seed} : aire {area} contre {}",
        shoelace(&st.poly)
    );
    c.areas_checked += 1;
    assert!(
        (area - area_before - batch_added).abs() < 1e-5,
        "seed {seed} : gain d'aire {} contre {batch_added}",
        area - area_before
    );
    c.widths_checked += 1;
    // 3. Lignes de pli stockées (ordre de pose).
    let stored = &proj.piece(piece).unwrap().pleats;
    assert_eq!(stored.len(), st.pleats.len(), "seed {seed}");
    for (sp, op) in stored.iter().zip(&st.pleats) {
        assert!(dist(sp.start(), op.a) < 1e-6, "seed {seed} : ligne stockée");
        assert!((sp.depth_mm - op.depth).abs() < 1e-12 && sp.kind == op.kind);
        assert!((sp.width_mm() - op.width()).abs() < 1e-12);
        c.stored_lines_checked += 1;
    }
    // 4. Pièce repliée = polygone d'origine.
    let folded = proj.piece_folded(piece).unwrap();
    let fv: Vec<PointMm> = folded.edges.iter().map(|e| e.start()).collect();
    assert!(
        same_set(&fv, &simplified(original), 1e-6),
        "seed {seed} : repliée {fv:?} contre {original:?}"
    );
    assert!(
        (folded.area_mm2 - shoelace(original)).abs() < 1e-5,
        "seed {seed}"
    );
    let total: f64 = st.pleats.iter().map(OPleat::width).sum();
    assert!((folded.removed_mm - total).abs() < 1e-9, "seed {seed}");
    c.folded_checked += 1;
    // 5. Repères : extrémités sur la ligne attendue et sur le contour.
    let marks = proj.piece_pleat_marks(piece);
    assert_eq!(marks.len(), st.pleats.len());
    for (m, op) in marks.iter().zip(&st.pleats) {
        let m = m
            .marks
            .as_ref()
            .unwrap_or_else(|e| panic!("seed {seed} : {e}"));
        let (dx, dy) = op.normal();
        let p = op.depth;
        let offsets: Vec<f64> = match op.kind {
            PleatKind::Flat => vec![0.0, p],
            PleatKind::Hollow => vec![0.0, p, 3.0 * p, 4.0 * p],
        };
        assert_eq!(m.lines.len(), offsets.len());
        for (line, off) in m.lines.iter().zip(&offsets) {
            for e in [line.from, line.to] {
                let s = (e.x - op.a.x) * dx + (e.y - op.a.y) * dy;
                assert!((s - off).abs() < 1e-6, "seed {seed} : repère hors ligne");
                assert!(
                    boundary_dist(&st.poly, e) < 1e-6,
                    "seed {seed} : repère hors contour"
                );
            }
            assert!(dist(line.from, line.to) > 1e-3);
            c.marks_checked += 1;
        }
        assert_eq!(
            m.arrows.len(),
            if op.kind == PleatKind::Flat { 1 } else { 2 }
        );
        let first = &m.lines[0];
        let mid = pt(
            (first.from.x + first.to.x) / 2.0,
            (first.from.y + first.to.y) / 2.0,
        );
        assert!(dist(m.arrows[0].from, mid) < 1e-6);
        assert!(
            dist(m.arrows[0].to, pt(mid.x + dx * p, mid.y + dy * p)) < 1e-6,
            "seed {seed} : flèche"
        );
    }
}

fn invalid_command(r: &mut Rng, piece: PieceId, a: PointMm, b: PointMm) -> Command {
    let mut q = Proposal {
        a,
        b,
        sense: Side::Right,
        kind: PleatKind::Flat,
        depth: 5.0,
        count: 1,
        span: 0.0,
    };
    match r.below(9) {
        0 => q.depth = 0.0,
        1 => q.depth = -r.range(0.1, 50.0),
        2 => q.depth = f64::NAN,
        3 => q.depth = f64::INFINITY,
        4 => q.depth = 501.0 + r.range(0.0, 1e4),
        5 => q.count = 0,
        6 => q.count = 51 + r.below(100) as u32,
        7 => {
            q.count = 2 + r.below(4) as u32;
            q.span = if r.chance(50) { f64::NAN } else { -1.0 };
        }
        _ => q.b = q.a,
    }
    command(piece, &q)
}

fn run_sequence(seed: u64, c: &mut Counters) {
    let mut r = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let original = random_polygon(&mut r);
    let (proj0, piece) = build(&original);
    let mut doc = Document::new(proj0);
    let mut st = OState {
        poly: {
            // Le contour de l'implémentation est anti-horaire : l'oracle part du polygone trié.
            let mut v = original.clone();
            if shoelace(&v) < 0.0 {
                v.reverse();
            }
            v
        },
        pleats: Vec::new(),
    };
    let original: Vec<PointMm> = st.poly.clone();
    c.sequences += 1;
    let (cx, cy) = {
        let n = original.len() as f64;
        (
            original.iter().map(|p| p.x).sum::<f64>() / n,
            original.iter().map(|p| p.y).sum::<f64>() / n,
        )
    };
    let mut dir: f64 = r.range(0.0, std::f64::consts::PI);
    let mut history: Vec<(String, String)> = Vec::new(); // (avant, après) par étape acceptée
    let ops = 3 + r.below(4);
    for step in 0..ops {
        c.attempts += 1;
        let propose_invalid = r.chance(20);
        // Direction : aléatoire, alignée aux axes, ou celle du pli précédent.
        if step == 0 || r.chance(if st.pleats.is_empty() { 35 } else { 12 }) {
            dir = match r.below(4) {
                0 => 0.0,
                1 => std::f64::consts::FRAC_PI_2,
                _ => r.range(0.0, std::f64::consts::PI),
            };
        }
        let flipped = r.chance(30);
        let (sx, sy) = (dir.cos(), dir.sin());
        let off = r.range(-60.0, 60.0);
        let along = if st.pleats.is_empty() {
            off
        } else {
            r.range(-110.0, 110.0)
        };
        let base = pt(cx - sy * along, cy + sx * along);
        let (mut a, mut b) = (
            pt(base.x - sx * 500.0, base.y - sy * 500.0),
            pt(base.x + sx * 500.0, base.y + sy * 500.0),
        );
        if flipped {
            std::mem::swap(&mut a, &mut b);
            c.flipped_lines += 1;
        }
        if propose_invalid {
            let cmd = invalid_command(&mut r, piece, a, b);
            let before = bytes(doc.project());
            assert!(
                doc.execute(cmd).is_err(),
                "seed {seed} : valeur invalide acceptée"
            );
            assert_eq!(
                before,
                bytes(doc.project()),
                "seed {seed} : refus sans trace"
            );
            c.refused_invalid += 1;
            c.refusals_without_trace += 1;
            continue;
        }
        let multi = r.chance(45);
        let kind = if r.chance(35) {
            PleatKind::Hollow
        } else {
            PleatKind::Flat
        };
        let depth = r.range(1.0, 25.0);
        let width = if kind == PleatKind::Flat {
            2.0 * depth
        } else {
            4.0 * depth
        };
        let count = if multi { 2 + r.below(3) as u32 } else { 1 };
        // Pas ordinairement un peu plus grand que la largeur ; parfois trop petit (chevauchement).
        let factor = if r.chance(15) {
            r.range(0.3, 0.95)
        } else {
            r.range(1.1, 2.5)
        };
        let q = Proposal {
            a,
            b,
            sense: if r.chance(50) {
                Side::Left
            } else {
                Side::Right
            },
            kind,
            depth,
            count,
            span: if multi {
                width * factor * f64::from(count - 1)
            } else {
                0.0
            },
        };
        let verdict = o_batch(&st, q.a, q.b, q.sense, q.kind, q.depth, q.count, q.span);
        let before = bytes(doc.project());
        match verdict {
            Verdict::Ambiguous => {
                c.ambiguous_skipped += 1;
            }
            Verdict::Refuse => {
                assert!(
                    doc.execute(command(piece, &q)).is_err(),
                    "seed {seed} : la pose aurait dû être refusée"
                );
                assert_eq!(
                    before,
                    bytes(doc.project()),
                    "seed {seed} : refus sans trace"
                );
                c.refused_geometry += 1;
                c.refusals_without_trace += 1;
            }
            Verdict::Ok(next, gain) => {
                let area_before = shoelace(&st.poly);
                let out = doc
                    .execute(command(piece, &q))
                    .unwrap_or_else(|e| panic!("seed {seed} : pose refusée à tort : {e}"));
                assert_eq!(out.pleats.len(), q.count as usize);
                if !st.pleats.is_empty() {
                    c.chained += 1;
                }
                if q.count > 1 {
                    c.multi_accepted += 1;
                }
                if q.kind == PleatKind::Hollow {
                    c.hollow_accepted += 1;
                }
                c.accepted += 1;
                // Largeur ajoutée : gain d'aire = Σ largeur × corde, calculé par l'oracle.
                check_state(
                    seed,
                    c,
                    doc.project(),
                    piece,
                    &next,
                    &original,
                    gain,
                    area_before,
                );
                let after = bytes(doc.project());
                assert_ne!(before, after);
                // Annulation puis rétablissement : une seule étape pour toute la commande.
                if r.chance(50) {
                    assert!(doc.undo());
                    assert_eq!(before, bytes(doc.project()), "seed {seed} : annulation");
                    assert!(doc.redo());
                    assert_eq!(after, bytes(doc.project()), "seed {seed} : rétablissement");
                    c.undo_redo_checks += 1;
                }
                // Fichier : aller-retour.
                let back = persist::from_json(&after).unwrap();
                assert_eq!(after, bytes(&back), "seed {seed}");
                c.json_roundtrips += 1;
                history.push((before, after));
                st = next;
            }
        }
    }
    // Défaire toute la séquence, étape par étape : retour octet à octet.
    while let Some((before, _)) = history.pop() {
        assert!(doc.undo(), "seed {seed}");
        assert_eq!(
            before,
            bytes(doc.project()),
            "seed {seed} : retour en arrière"
        );
    }
    assert!(!doc.can_undo(), "seed {seed}");
}

#[test]
fn campagne_adverse_plis_180_sequences() {
    let mut c = Counters::default();
    for seed in 1..=180u64 {
        run_sequence(seed, &mut c);
    }
    eprintln!("{c:#?}");
    assert!(c.sequences >= 150, "{c:?}");
    assert!(c.accepted >= 250, "{c:?}");
    assert!(c.refused_geometry >= 30, "{c:?}");
    assert!(c.refused_invalid >= 40, "{c:?}");
    assert!(c.multi_accepted >= 60, "{c:?}");
    assert!(c.hollow_accepted >= 60, "{c:?}");
    assert!(c.chained >= 80, "{c:?}");
    assert!(c.flipped_lines >= 50, "{c:?}");
    assert_eq!(c.vertex_sets_checked, c.accepted, "{c:?}");
    assert_eq!(c.areas_checked, c.accepted, "{c:?}");
    assert_eq!(c.widths_checked, c.accepted, "{c:?}");
    assert_eq!(c.folded_checked, c.accepted, "{c:?}");
    assert!(c.stored_lines_checked >= c.accepted, "{c:?}");
    assert!(c.marks_checked >= 2 * c.accepted, "{c:?}");
    assert!(c.undo_redo_checks >= 80, "{c:?}");
    assert!(c.json_roundtrips == c.accepted, "{c:?}");
    assert!(
        c.refusals_without_trace == c.refused_geometry + c.refused_invalid,
        "{c:?}"
    );
    assert!(c.ambiguous_skipped * 10 <= c.attempts, "{c:?}");
}
