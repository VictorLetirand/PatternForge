//! US-07.5 : campagne adverse de la découpe d'une pièce selon une ligne, 200 séquences à graine
//! fixe. Chaque séquence crée un polygone convexe aléatoire (3 à 7 sommets sur une ellipse)
//! extrait en pièce, puis tente des découpes : lignes qui traversent, qui s'arrêtent dedans, qui
//! passent dehors, qui passent par un sommet, nombres et pas de crans valides ou non, valeurs non
//! finies, puis une seconde découpe sur un morceau obtenu, des annulations / rétablissements, un
//! recollage.
//!
//! **Oracle indépendant** : il ne lit rien du code testé. Il tient son polygone (sommets
//! anti-horaires), classe les sommets par rapport à la droite (distances signées), décide seul si la
//! découpe est acceptée (deux points sur le contour, tous deux dans le segment), construit les deux
//! morceaux par découpage d'un polygone convexe par un demi-plan (Sutherland-Hodgman), puis
//! compare : ensemble des sommets (colinéaires fusionnés), aire (formule du lacet), périmètre,
//! somme des aires, distances et points des crans sur les deux bords (mêmes distances depuis le
//! point de départ de la ligne, calculées à part), côtés des crans, aperçu, recollage = polygone
//! d'origine. Les cas à moins de 1e-3 mm d'une frontière de décision sont **écartés** (et comptés).
//! Des compteurs prouvent que les contrôles forts s'exécutent. Mutations vérifiées : voir
//! `docs/JOURNAL.md` (tranche 65).

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
    second_generation: usize,
    refused_geometry: usize,
    refused_invalid: usize,
    ambiguous_skipped: usize,
    refusals_without_trace: usize,
    vertex_sets_checked: usize,
    areas_checked: usize,
    perimeters_checked: usize,
    notch_pairs_checked: usize,
    notch_sides_checked: usize,
    previews_checked: usize,
    joins_checked: usize,
    undo_redo_checks: usize,
    json_roundtrips: usize,
    step_modes: usize,
}

// ---------------------------------------------------------------- oracle

fn dist(a: PointMm, b: PointMm) -> f64 {
    (a.x - b.x).hypot(a.y - b.y)
}

fn shoelace(poly: &[PointMm]) -> f64 {
    let n = poly.len();
    (0..n)
        .map(|i| poly[i].x * poly[(i + 1) % n].y - poly[(i + 1) % n].x * poly[i].y)
        .sum::<f64>()
        / 2.0
}

fn perim(poly: &[PointMm]) -> f64 {
    let n = poly.len();
    (0..n).map(|i| dist(poly[i], poly[(i + 1) % n])).sum()
}

fn side_of(u: (f64, f64), a: PointMm, p: PointMm) -> f64 {
    u.0 * (p.y - a.y) - u.1 * (p.x - a.x)
}

/// Polygone sans doublons consécutifs ni sommets colinéaires.
fn simplified(poly: &[PointMm]) -> Vec<PointMm> {
    let mut v: Vec<PointMm> = Vec::new();
    for p in poly {
        if v.last().is_none_or(|l| dist(*l, *p) > 1e-6) {
            v.push(*p);
        }
    }
    while v.len() > 1 && dist(v[0], v[v.len() - 1]) <= 1e-6 {
        v.pop();
    }
    loop {
        let n = v.len();
        if n < 3 {
            return v;
        }
        let k = (0..n).find(|&i| {
            let (a, b, c) = (v[(i + n - 1) % n], v[i], v[(i + 1) % n]);
            let cr = (b.x - a.x) * (c.y - b.y) - (b.y - a.y) * (c.x - b.x);
            cr.abs() <= 1e-9 * dist(a, b).max(1.0) * dist(b, c).max(1.0)
        });
        match k {
            Some(i) => {
                v.remove(i);
            }
            None => return v,
        }
    }
}

fn same_set(got: &[PointMm], want: &[PointMm], tol: f64) -> bool {
    got.len() == want.len()
        && want.iter().all(|w| got.iter().any(|g| dist(*g, *w) <= tol))
        && got.iter().all(|g| want.iter().any(|w| dist(*g, *w) <= tol))
}

/// Découpe du polygone convexe `poly` par le demi-plan `side * cross(u, p - a) >= 0`.
fn clip(poly: &[PointMm], a: PointMm, u: (f64, f64), sign: f64) -> Vec<PointMm> {
    let n = poly.len();
    let mut out = Vec::new();
    for i in 0..n {
        let (s, e) = (poly[i], poly[(i + 1) % n]);
        let (ds, de) = (sign * side_of(u, a, s), sign * side_of(u, a, e));
        let (s_in, e_in) = (ds >= -1e-9, de >= -1e-9);
        let inter = |ds: f64, de: f64| {
            let t = ds / (ds - de);
            pt(s.x + (e.x - s.x) * t, s.y + (e.y - s.y) * t)
        };
        match (s_in, e_in) {
            (true, true) => out.push(e),
            (true, false) => out.push(inter(ds, de)),
            (false, true) => {
                out.push(inter(ds, de));
                out.push(e);
            }
            _ => {}
        }
    }
    simplified(&out)
}

enum Verdict {
    Accept {
        p: PointMm,
        q: PointMm,
        u: (f64, f64),
        left: Vec<PointMm>,
        right: Vec<PointMm>,
    },
    Refuse,
    Ambiguous,
}

fn o_cut(poly: &[PointMm], a: PointMm, b: PointMm) -> Verdict {
    let len = dist(a, b);
    if len < 1e-9 {
        return Verdict::Refuse;
    }
    let u = ((b.x - a.x) / len, (b.y - a.y) / len);
    let d: Vec<f64> = poly.iter().map(|v| side_of(u, a, *v)).collect();
    if d.iter().any(|x| x.abs() > 1e-9 && x.abs() < 1e-3) {
        return Verdict::Ambiguous;
    }
    if d.iter().all(|x| *x <= 1e-9) || d.iter().all(|x| *x >= -1e-9) {
        return Verdict::Refuse;
    }
    let n = poly.len();
    let mut hits: Vec<PointMm> = Vec::new();
    for i in 0..n {
        let (di, dj) = (d[i], d[(i + 1) % n]);
        let (vi, vj) = (poly[i], poly[(i + 1) % n]);
        let found = if di.abs() <= 1e-9 {
            Some(vi)
        } else if dj.abs() > 1e-9 && di * dj < 0.0 {
            let t = di / (di - dj);
            Some(pt(vi.x + (vj.x - vi.x) * t, vi.y + (vj.y - vi.y) * t))
        } else {
            None
        };
        if let Some(h) = found {
            if hits.iter().all(|k| dist(*k, h) > 1e-6) {
                hits.push(h);
            }
        }
    }
    if hits.len() != 2 {
        return Verdict::Ambiguous;
    }
    let t = |h: PointMm| (h.x - a.x) * u.0 + (h.y - a.y) * u.1;
    let (mut p, mut q) = (hits[0], hits[1]);
    if t(p) > t(q) {
        std::mem::swap(&mut p, &mut q);
    }
    let (tp, tq) = (t(p), t(q));
    if tp.abs() < 1e-3 || (tq - len).abs() < 1e-3 {
        return Verdict::Ambiguous;
    }
    if tp < 0.0 || tq > len {
        return Verdict::Refuse;
    }
    Verdict::Accept {
        p,
        q,
        u,
        left: clip(poly, a, u, 1.0),
        right: clip(poly, a, u, -1.0),
    }
}

/// Positions attendues des crans sur une ligne de `len` mm ; `None` = refus attendu.
fn o_notches(spec: &CutNotches, len: f64) -> Option<Vec<f64>> {
    match *spec {
        CutNotches::Count { count } => {
            if count > 50 {
                return None;
            }
            Some(
                (1..=count)
                    .map(|k| len * k as f64 / (count as f64 + 1.0))
                    .collect(),
            )
        }
        CutNotches::Step { step_mm } => {
            if !step_mm.is_finite() || !(1.0..=1000.0).contains(&step_mm) {
                return None;
            }
            let v: Vec<f64> = (1..)
                .map(|k| k as f64 * step_mm)
                .take_while(|d| *d < len - 1e-6)
                .collect();
            (v.len() <= 50).then_some(v)
        }
    }
}

// ---------------------------------------------------------------- bibliothèque

fn outline_vertices(p: &Project, piece: PieceId) -> Vec<PointMm> {
    let sheet = p.piece_sheet(piece).unwrap();
    p.piece(piece)
        .unwrap()
        .outline
        .iter()
        .map(|e| {
            let (a, b) = match sheet.shape(e.entity).unwrap() {
                Shape::Line(a, b) => (a, b),
                other => panic!("tracé inattendu {other:?}"),
            };
            if e.reversed {
                b
            } else {
                a
            }
        })
        .collect()
}

fn random_polygon(r: &mut Rng) -> Vec<PointMm> {
    let n = 3 + r.below(5) as usize;
    let (cx, cy) = (r.range(-300.0, 300.0), r.range(-300.0, 300.0));
    let (rx, ry) = (r.range(40.0, 160.0), r.range(40.0, 160.0));
    let phase = r.range(0.0, std::f64::consts::TAU);
    (0..n)
        .map(|k| {
            let ang = phase + std::f64::consts::TAU * (k as f64 + r.range(0.1, 0.9)) / n as f64;
            pt(cx + rx * ang.cos(), cy + ry * ang.sin())
        })
        .collect()
}

fn build(poly: &[PointMm]) -> (Project, PieceId) {
    let mut p = Project::new("adverse découpes");
    let s = p.sheets[0].id;
    let n = poly.len();
    let ids: Vec<EntityId> = (0..n)
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

/// Une ligne aléatoire par rapport au polygone `poly`.
fn random_line(r: &mut Rng, poly: &[PointMm]) -> (PointMm, PointMm) {
    let n = poly.len() as f64;
    let c = pt(
        poly.iter().map(|v| v.x).sum::<f64>() / n,
        poly.iter().map(|v| v.y).sum::<f64>() / n,
    );
    let rad = poly.iter().map(|v| dist(*v, c)).fold(0.0, f64::max);
    let ang = r.range(0.0, std::f64::consts::TAU);
    let u = (ang.cos(), ang.sin());
    let nrm = (-u.1, u.0);
    let off = r.range(-0.9, 0.9) * rad;
    let mid = pt(c.x + nrm.0 * off, c.y + nrm.1 * off);
    let mk = |t: f64| pt(mid.x + u.0 * t, mid.y + u.1 * t);
    match r.below(100) {
        0..=59 => (mk(-r.range(1.2, 3.0) * rad), mk(r.range(1.2, 3.0) * rad)),
        60..=74 => (mk(-r.range(1.2, 3.0) * rad), mk(r.range(-0.3, 0.3) * rad)),
        75..=82 => {
            let far = pt(c.x + 5.0 * rad, c.y + 5.0 * rad);
            (far, pt(far.x + 50.0, far.y - 30.0))
        }
        83..=90 => (mk(-0.2 * rad), mk(0.2 * rad)),
        _ => {
            // Par un sommet.
            let v = poly[r.below(poly.len() as u64) as usize];
            (
                pt(v.x - u.0 * 3.0 * rad, v.y - u.1 * 3.0 * rad),
                pt(v.x + u.0 * 3.0 * rad, v.y + u.1 * 3.0 * rad),
            )
        }
    }
}

fn random_spec(r: &mut Rng, c: &mut Counters) -> CutNotches {
    match r.below(10) {
        0..=4 => CutNotches::default(),
        5 => CutNotches::Count {
            count: r.below(8) as u32,
        },
        6 => CutNotches::Count {
            count: 40 + r.below(20) as u32,
        },
        7 => {
            c.step_modes += 1;
            CutNotches::Step {
                step_mm: r.range(8.0, 60.0),
            }
        }
        8 => {
            c.step_modes += 1;
            CutNotches::Step {
                step_mm: [0.0, -3.0, 0.5, 1e9, f64::NAN, 1.0, 2000.0][r.below(7) as usize],
            }
        }
        _ => CutNotches::Count { count: 1 },
    }
}

/// Distance, point et côté de chaque cran de l'arête de découpe de `half`.
fn marks(p: &Project, half: PieceId) -> Vec<(f64, PointMm, Side)> {
    let sheet = p.piece_sheet(half).unwrap();
    let edge = p.piece(half).unwrap().outline.last().unwrap().entity;
    let mut v: Vec<_> = sheet
        .notches
        .iter()
        .filter(|n| n.entity == edge)
        .map(|n| {
            let d = sheet.notch_distance_mm(n).unwrap();
            (d, sheet.path_point_at(edge, d).unwrap().0, n.side)
        })
        .collect();
    v.sort_by(|a, b| a.0.total_cmp(&b.0));
    v
}

fn check_poly(p: &Project, id: PieceId, want: &[PointMm], c: &mut Counters, what: &str) {
    let got = simplified(&outline_vertices(p, id));
    assert!(
        same_set(&got, want, 1e-6),
        "{what} : sommets {got:?} ≠ {want:?}"
    );
    c.vertex_sets_checked += 1;
    let ct = p.piece_contour(id).unwrap();
    let wa = shoelace(want).abs();
    assert!(
        (ct.area_mm2 - wa).abs() <= 1e-6 * wa.max(1.0),
        "{what} : aire {} ≠ {wa}",
        ct.area_mm2
    );
    c.areas_checked += 1;
    let wp = perim(want);
    assert!(
        (ct.perimeter_mm - wp).abs() <= 1e-6 * wp.max(1.0),
        "{what} : périmètre {} ≠ {wp}",
        ct.perimeter_mm
    );
    c.perimeters_checked += 1;
}

/// Une tentative de découpe de `piece` (polygone `poly` tenu par l'oracle). Renvoie les deux
/// morceaux (identifiant, polygone) si la découpe a été acceptée.
fn attempt(
    d: &mut Document,
    piece: PieceId,
    poly: &[PointMm],
    r: &mut Rng,
    c: &mut Counters,
) -> Option<Vec<(PieceId, Vec<PointMm>)>> {
    let (a, b) = random_line(r, poly);
    let spec = random_spec(r, c);
    c.attempts += 1;
    let verdict = o_cut(poly, a, b);
    let cmd = Command::CutPiece {
        piece,
        line: CutLine::Points { a, b },
        notches: spec,
    };
    let (before, before_bytes) = (d.project().clone(), bytes(d.project()));
    match verdict {
        Verdict::Ambiguous => {
            c.ambiguous_skipped += 1;
            None
        }
        Verdict::Refuse => {
            assert!(
                d.execute(cmd).is_err(),
                "acceptée à tort : {a:?} {b:?} sur {poly:?}"
            );
            assert_eq!(*d.project(), before);
            assert_eq!(bytes(d.project()), before_bytes);
            c.refused_geometry += 1;
            c.refusals_without_trace += 1;
            None
        }
        Verdict::Accept {
            p,
            q,
            u,
            left,
            right,
        } => {
            let len = dist(p, q);
            let expected = o_notches(&spec, len);
            let Some(expected) = expected else {
                assert!(d.execute(cmd).is_err(), "spec invalide acceptée : {spec:?}");
                assert_eq!(*d.project(), before);
                assert_eq!(bytes(d.project()), before_bytes);
                c.refused_invalid += 1;
                c.refusals_without_trace += 1;
                return None;
            };
            // Aperçu d'accord avec l'oracle, avant la commande.
            let prev = d
                .project()
                .cut_preview(piece, &CutLine::Points { a, b }, &spec)
                .unwrap();
            assert!(prev.refusal.is_none(), "{:?}", prev.refusal);
            assert!(dist(prev.points[0], p) < 1e-6 && dist(prev.points[1], q) < 1e-6);
            assert!((prev.cut_length_mm - len).abs() < 1e-6);
            assert!((prev.left.area_mm2 - shoelace(&left).abs()).abs() < 1e-6 * 1e3);
            assert!((prev.right.area_mm2 - shoelace(&right).abs()).abs() < 1e-6 * 1e3);
            assert!((prev.left.edge_mm - prev.right.edge_mm).abs() < 1e-9);
            assert_eq!(prev.notch_distances_mm.len(), expected.len());
            c.previews_checked += 1;

            let o = d
                .execute(cmd)
                .unwrap_or_else(|e| panic!("refusée à tort : {e} ; {a:?} {b:?} sur {poly:?}"));
            c.accepted += 1;
            let (l, rr) = (o.pieces[0], o.pieces[1]);
            check_poly(d.project(), l, &left, c, "gauche");
            check_poly(d.project(), rr, &right, c, "droite");
            let total = shoelace(poly).abs();
            let sum = d.project().piece_contour(l).unwrap().area_mm2
                + d.project().piece_contour(rr).unwrap().area_mm2;
            assert!(
                (sum - total).abs() <= 1e-6 * total.max(1.0),
                "somme des aires"
            );
            // Crans : mêmes distances depuis P des deux côtés, aux points attendus.
            let (ml, mr) = (marks(d.project(), l), marks(d.project(), rr));
            assert_eq!((ml.len(), mr.len()), (expected.len(), expected.len()));
            assert_eq!(o.notches.len(), 2 * expected.len());
            for (k, want) in expected.iter().enumerate() {
                assert!((ml[k].0 - want).abs() < 1e-6, "gauche {} ≠ {want}", ml[k].0);
                assert!((mr[k].0 - want).abs() < 1e-6, "droite {} ≠ {want}", mr[k].0);
                assert!((ml[k].0 - mr[k].0).abs() < 1e-9, "distances inégales");
                let at = pt(p.x + want * u.0, p.y + want * u.1);
                assert!(dist(ml[k].1, at) < 1e-6 && dist(mr[k].1, at) < 1e-6);
                assert!(ml[k].2 == Side::Right && mr[k].2 == Side::Left);
                c.notch_pairs_checked += 1;
                c.notch_sides_checked += 2;
            }
            // Une seule étape : annuler redonne l'état d'avant, rétablir l'état d'après.
            let after = d.project().clone();
            assert!(d.undo());
            assert_eq!(*d.project(), before);
            assert_eq!(bytes(d.project()), before_bytes);
            assert!(d.redo());
            assert_eq!(*d.project(), after);
            c.undo_redo_checks += 1;
            // Aller-retour JSON.
            let text = bytes(d.project());
            let back = persist::from_json(&text).unwrap();
            assert_eq!(bytes(&back), text);
            c.json_roundtrips += 1;
            d.project().validate().unwrap();
            Some(vec![(l, left), (rr, right)])
        }
    }
}

/// Recollage des deux morceaux : polygone d'origine (aire, périmètre, sommets sur son contour).
fn check_join(
    d: &mut Document,
    halves: &[(PieceId, Vec<PointMm>)],
    poly: &[PointMm],
    c: &mut Counters,
) {
    let p = d.project();
    let edge = |id: PieceId| p.piece(id).unwrap().outline.last().unwrap().entity;
    let cmd = Command::JoinPieces {
        first: halves[0].0,
        first_edge: edge(halves[0].0),
        second: halves[1].0,
        second_edge: edge(halves[1].0),
        tolerance_mm: 1e-6,
    };
    let mut q = p.clone();
    let o = q
        .apply(cmd)
        .unwrap_or_else(|e| panic!("recollage refusé : {e}"));
    let id = o.pieces[0];
    let ct = q.piece_contour(id).unwrap();
    let total = shoelace(poly).abs();
    assert!(
        (ct.area_mm2 - total).abs() <= 1e-6 * total.max(1.0),
        "aire recollée"
    );
    assert!((ct.perimeter_mm - perim(poly)).abs() <= 1e-6 * perim(poly).max(1.0));
    let v = outline_vertices(&q, id);
    // Chaque sommet d'origine est présent ; chaque sommet recollé est sur un bord d'origine.
    for w in poly {
        assert!(v.iter().any(|g| dist(*g, *w) < 1e-6), "sommet {w:?} perdu");
    }
    let n = poly.len();
    for g in &v {
        let on_edge = (0..n).any(|i| {
            let (s, e) = (poly[i], poly[(i + 1) % n]);
            let l = dist(s, e);
            let t =
                (((g.x - s.x) * (e.x - s.x) + (g.y - s.y) * (e.y - s.y)) / (l * l)).clamp(0.0, 1.0);
            dist(*g, pt(s.x + (e.x - s.x) * t, s.y + (e.y - s.y) * t)) < 1e-6
        });
        assert!(on_edge, "sommet recollé {g:?} hors du contour d'origine");
    }
    c.joins_checked += 1;
}

fn invalid_commands(d: &mut Document, piece: PieceId, poly: &[PointMm], c: &mut Counters) {
    let (a, b) = (poly[0], poly[1]);
    let cmds = [
        CutLine::Points {
            a: pt(f64::NAN, 0.0),
            b,
        },
        CutLine::Points {
            a,
            b: pt(f64::INFINITY, 0.0),
        },
        CutLine::Points {
            a: pt(1e300, 0.0),
            b,
        },
        CutLine::Points { a, b: a },
        CutLine::Path {
            entity: EntityId(987_654),
        },
    ];
    for line in cmds {
        let (before, text) = (d.project().clone(), bytes(d.project()));
        assert!(d
            .execute(Command::CutPiece {
                piece,
                line,
                notches: CutNotches::default(),
            })
            .is_err());
        assert_eq!(*d.project(), before);
        assert_eq!(bytes(d.project()), text);
        c.refused_invalid += 1;
        c.refusals_without_trace += 1;
    }
    let (before, text) = (d.project().clone(), bytes(d.project()));
    assert!(d
        .execute(Command::CutPiece {
            piece: PieceId(987_654),
            line: CutLine::Points { a, b },
            notches: CutNotches::default(),
        })
        .is_err());
    assert_eq!(*d.project(), before);
    assert_eq!(bytes(d.project()), text);
    c.refusals_without_trace += 1;
}

fn run_sequence(seed: u64, c: &mut Counters) {
    let mut r = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let poly = random_polygon(&mut r);
    let (project, piece) = build(&poly);
    let mut d = Document::new(project);
    c.sequences += 1;
    invalid_commands(&mut d, piece, &poly, c);
    let mut tries = 0;
    while tries < 6 {
        tries += 1;
        if let Some(halves) = attempt(&mut d, piece, &poly, &mut r, c) {
            if r.chance(35) {
                check_join(&mut d, &halves, &poly, c);
            }
            // Seconde génération : on découpe un des morceaux (pièce détachée).
            if r.chance(60) {
                let (id, hp) = halves[r.below(2) as usize].clone();
                for _ in 0..4 {
                    if attempt(&mut d, id, &hp, &mut r, c).is_some() {
                        c.second_generation += 1;
                        break;
                    }
                }
            }
            break;
        }
    }
}

#[test]
fn campagne_adverse_decoupes_200_sequences() {
    let mut c = Counters::default();
    for seed in 1..=200u64 {
        run_sequence(seed, &mut c);
    }
    eprintln!("{c:#?}");
    assert_eq!(c.sequences, 200);
    assert!(c.accepted >= 120, "{c:?}");
    assert!(c.refused_geometry >= 40, "{c:?}");
    assert!(c.refused_invalid >= 100, "{c:?}");
    assert!(c.second_generation >= 30, "{c:?}");
    assert!(c.vertex_sets_checked >= 2 * c.accepted, "{c:?}");
    assert!(c.areas_checked >= 2 * c.accepted, "{c:?}");
    assert!(c.perimeters_checked >= 2 * c.accepted, "{c:?}");
    assert!(c.notch_pairs_checked >= 150, "{c:?}");
    assert!(c.notch_sides_checked >= 300, "{c:?}");
    assert!(c.previews_checked >= c.accepted, "{c:?}");
    assert!(c.joins_checked >= 20, "{c:?}");
    assert!(c.undo_redo_checks >= c.accepted, "{c:?}");
    assert!(c.json_roundtrips >= c.accepted, "{c:?}");
    assert!(c.step_modes >= 20, "{c:?}");
    assert!(c.refusals_without_trace >= 140, "{c:?}");
}
