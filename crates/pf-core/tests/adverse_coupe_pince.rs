//! PH-5a — campagne adverse de la coupe « pince fermée » : 170 séquences à graine fixe sur des
//! rectangles et des polygones convexes (points d'une ellipse, 4 à 7 sommets), marges par bord
//! aléatoires, 1 à 3 pinces (jambe fixe, ouverture, longueur, position aléatoires, dont des
//! pinces impossibles, des bouches sur un coin et des pinces qui se touchent), réglages, ouvertures
//! modifiées, sommets déplacés, retraits, annulations et rétablissements.
//!
//! **Oracle indépendant** : il ne lit rien du code testé. Il refait le contour de coupe ordinaire
//! (droites décalées de la marge de chaque bord, coins en onglet : intersection des droites), puis,
//! pour chaque pince réglée, la coupe pince fermée de deux façons qui doivent s'accorder :
//! (1) la **rotation de fermeture** écrite à la main (angle = différence des `atan2` des deux
//! jambes, côté mobile tourné autour du sommet, décalages des deux demi-bords fermés coupés l'un
//! par l'autre, rotation inverse du côté mobile) ; (2) la formule droite : J1 = pied avant la bouche
//! moins `m · tan α`, J2 = pied après plus `m · tan α` (`tan α = (ouverture / 2) / longueur`). Il
//! décide aussi seul des **replis** (bouche sur un coin, bord trop court, bouches qui se touchent,
//! pince devenue impossible) et du contrôle `CutDartFailed`. Des **compteurs** prouvent que les
//! contrôles forts s'exécutent. Mutations vérifiées à la main : voir `docs/JOURNAL.md` (tranche 80).

use pf_core::persist;
use pf_core::piece::PieceIssue;
use pf_core::*;
use pf_geometry::region::trace_geometry;
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
    rectangles: usize,
    polygons: usize,
    darts_added: usize,
    darts_refused: usize,
    refusals_without_trace: usize,
    ambiguous_skipped: usize,
    undo_redo_checks: usize,
    json_roundtrips: usize,
    plain_checked: usize,
    closed_ok_checked: usize,
    closed_two_on_one_edge: usize,
    closed_vertices_checked: usize,
    rotation_oracle_checked: usize,
    formula_oracle_checked: usize,
    invariant_checked: usize,
    fallback_expected: usize,
    fallback_corner: usize,
    fallback_touch: usize,
    fallback_short: usize,
    fallback_invalid: usize,
    issue_checked: usize,
    moves: usize,
    toggles: usize,
    openings: usize,
    removes: usize,
}

// ----- polygone et décalage (oracle) ----------------------------------------------------------

#[derive(Clone)]
struct Poly {
    v: Vec<PointMm>,
    /// Marge (début, fin) de chaque bord, dans le sens du contour anti-horaire.
    m: Vec<(f64, f64)>,
}

fn sub(a: PointMm, b: PointMm) -> PointMm {
    pt(a.x - b.x, a.y - b.y)
}
fn add(a: PointMm, b: PointMm) -> PointMm {
    pt(a.x + b.x, a.y + b.y)
}
fn mul(a: PointMm, k: f64) -> PointMm {
    pt(a.x * k, a.y * k)
}
fn cross(a: PointMm, b: PointMm) -> f64 {
    a.x * b.y - a.y * b.x
}
fn dot(a: PointMm, b: PointMm) -> f64 {
    a.x * b.x + a.y * b.y
}
fn dist(a: PointMm, b: PointMm) -> f64 {
    sub(a, b).x.hypot(sub(a, b).y)
}

/// Intersection de deux droites (point, direction).
fn meet(p1: PointMm, d1: PointMm, p2: PointMm, d2: PointMm) -> PointMm {
    let t = cross(sub(p2, p1), d2) / cross(d1, d2);
    add(p1, mul(d1, t))
}

/// Rotation de `p` autour de `c` d'un angle en radians.
fn rotate(p: PointMm, c: PointMm, angle: f64) -> PointMm {
    let (s, k) = angle.sin_cos();
    let (dx, dy) = (p.x - c.x, p.y - c.y);
    pt(c.x + k * dx - s * dy, c.y + s * dx + k * dy)
}

impl Poly {
    fn n(&self) -> usize {
        self.v.len()
    }
    /// Début, direction unitaire, longueur, normale intérieure (gauche) du bord `k`.
    fn edge(&self, k: usize) -> (PointMm, PointMm, f64, PointMm) {
        let (a, b) = (self.v[k], self.v[(k + 1) % self.n()]);
        let len = dist(a, b);
        let d = mul(sub(b, a), 1.0 / len);
        (a, d, len, pt(-d.y, d.x))
    }
    /// Un point et la direction de la droite décalée du bord `k` (vers l'extérieur).
    fn offset_line(&self, k: usize) -> (PointMm, PointMm) {
        let (a, d, len, n_in) = self.edge(k);
        let (start, end) = (
            add(a, mul(n_in, -self.m[k].0)),
            add(add(a, mul(d, len)), mul(n_in, -self.m[k].1)),
        );
        (start, sub(end, start))
    }
    /// Coin de coupe au début du bord `k` (onglet).
    fn corner(&self, k: usize) -> PointMm {
        let p = (k + self.n() - 1) % self.n();
        let (p1, d1) = self.offset_line(p);
        let (p2, d2) = self.offset_line(k);
        meet(p1, d1, p2, d2)
    }
    fn has_margin(&self) -> bool {
        self.m.iter().any(|m| m.0 > 0.0 || m.1 > 0.0)
    }
    /// Distance de `p` à chaque droite de bord, vers l'intérieur (positive = dedans).
    fn inward_distances(&self, p: PointMm) -> Vec<f64> {
        (0..self.n())
            .map(|k| {
                let (a, _, _, n_in) = self.edge(k);
                dot(sub(p, a), n_in)
            })
            .collect()
    }
    fn min_interior_angle_deg(&self) -> f64 {
        let n = self.n();
        let mut least = 180.0_f64;
        for k in 0..n {
            let (_, d0, _, _) = self.edge((k + n - 1) % n);
            let (_, d1, _, _) = self.edge(k);
            let turn = cross(d0, d1).atan2(dot(d0, d1)).to_degrees();
            least = least.min(180.0 - turn);
        }
        least
    }
}

#[derive(Clone)]
struct DartSpec {
    id: DartId,
    apex: PointId,
    edge: usize,
    s_mid: f64,
    w: f64,
    l: f64,
    fixed: Side,
    closed: bool,
    /// Déplacement cumulé du sommet le long de l'axe (mm) : ≠ 0 = pince impossible.
    shift: f64,
}

impl DartSpec {
    /// Pieds dans l'ordre du parcours : (avant, après) en abscisse le long du bord.
    fn feet_s(&self) -> (f64, f64) {
        (self.s_mid - self.w / 2.0, self.s_mid + self.w / 2.0)
    }
    fn invalid_now(&self) -> bool {
        self.shift.abs() > 2e-3
    }
}

enum Verdict {
    Cut(Vec<PointMm>),
    Fallback(&'static str),
    Ambiguous,
}

/// Oracle de la coupe : sommets attendus (dans le sens du contour) ou repli prédit.
#[allow(clippy::needless_range_loop)]
fn expected_cut(poly: &Poly, darts: &[DartSpec], c: &mut Counters) -> Verdict {
    let plain: Vec<PointMm> = (0..poly.n()).map(|k| poly.corner(k)).collect();
    let closed: Vec<&DartSpec> = darts.iter().filter(|d| d.closed).collect();
    if closed.is_empty() || !poly.has_margin() {
        return Verdict::Cut(plain);
    }
    if closed.iter().any(|d| d.invalid_now()) {
        return Verdict::Fallback("invalid");
    }
    let band = 1e-3;
    let mut fallback: Option<&'static str> = None;
    // Par bord, dans l'ordre le long du bord.
    let mut with: Vec<Vec<(PointMm, PointMm)>> = vec![Vec::new(); poly.n()];
    for k in 0..poly.n() {
        let mut on: Vec<&DartSpec> = closed.iter().copied().filter(|d| d.edge == k).collect();
        on.sort_by(|a, b| a.feet_s().0.total_cmp(&b.feet_s().0));
        let (v0, d, len, n_in) = poly.edge(k);
        let n_out = mul(n_in, -1.0);
        let (o0, dv) = poly.offset_line(k);
        let bare = poly.m[k] == (0.0, 0.0);
        let band_u = band / len;
        // Abscisse (fraction de l'arête) d'un point de la droite décalée.
        let u_of = |p: PointMm| dot(sub(p, o0), dv) / dot(dv, dv);
        // Deux pinces qui se touchent ou se chevauchent.
        for pair in on.windows(2) {
            let (a, b) = (pair[0].feet_s(), pair[1].feet_s());
            if (a.1 - b.0).abs() < band {
                return Verdict::Ambiguous;
            }
            if a.1 > b.0 {
                fallback.get_or_insert("touch");
            }
        }
        // Sommets de la bouche (oracle 1 : rotation de fermeture écrite à la main).
        let mut js: Vec<(PointMm, PointMm, f64, f64)> = Vec::new(); // J1, J2, u1, u2
        for dt in &on {
            let (f1, f2) = dt.feet_s();
            // Bouche sur un coin : un pied au sommet du bord.
            if f1.abs() < 1e-9 || (f2 - len).abs() < 1e-9 {
                fallback.get_or_insert("corner");
            }
            let (foot1, foot2) = (add(v0, mul(d, f1)), add(v0, mul(d, f2)));
            let apex = add(add(v0, mul(d, dt.s_mid)), mul(n_in, dt.l));
            // Pied fixe / mobile : la jambe gauche (vue du sommet vers la base) est le pied « après ».
            let (fixed_foot, moving_foot) = match dt.fixed {
                Side::Left => (foot2, foot1),
                Side::Right => (foot1, foot2),
            };
            let theta = sub(fixed_foot, apex).y.atan2(sub(fixed_foot, apex).x)
                - sub(moving_foot, apex).y.atan2(sub(moving_foot, apex).x);
            let moving_second = dt.fixed == Side::Right;
            // Fermée : la droite décalée du côté mobile tourne autour du sommet, celle du côté
            // fixe reste ; elles se coupent en J (même droite d'origine, donc même point de départ).
            let o0_t = rotate(o0, apex, theta);
            let dv_t = sub(rotate(add(o0, dv), apex, theta), o0_t);
            let jc = meet(o0, dv, o0_t, dv_t);
            let back = rotate(jc, apex, -theta);
            let (j1, j2) = if moving_second {
                (jc, back)
            } else {
                (back, jc)
            };
            // Invariant d'atelier : la rotation de fermeture ramène J2 sur J1 (ou l'inverse).
            let (jf, jm) = if moving_second { (j1, j2) } else { (j2, j1) };
            assert!(
                dist(rotate(jm, apex, theta), jf) < 1e-7,
                "coupe fermée discontinue"
            );
            c.rotation_oracle_checked += 1;
            c.invariant_checked += 1;
            // Oracle 2 : formule droite (marge constante sous la pince).
            if poly.m[k].0 == poly.m[k].1 {
                let t = (dt.w / 2.0) / dt.l;
                let m = poly.m[k].0;
                let q1 = add(add(v0, mul(d, f1 - m * t)), mul(n_out, m));
                let q2 = add(add(v0, mul(d, f2 + m * t)), mul(n_out, m));
                assert!(
                    dist(j1, q1) < 1e-7 && dist(j2, q2) < 1e-7,
                    "les deux oracles divergent : {j1:?} {q1:?} / {j2:?} {q2:?}"
                );
                c.formula_oracle_checked += 1;
            }
            js.push((j1, j2, u_of(j1), u_of(j2)));
        }
        if !bare {
            for (i, (_, _, u1, u2)) in js.iter().enumerate() {
                // Chaque morceau de décalage doit contenir le point où il est coupé : J1 après le
                // début du morceau d'avant la bouche, J2 avant la fin du morceau d'après, dans le
                // bon sens (la coupe rouverte ne revient pas sur elle-même).
                let start = if i == 0 {
                    0.0
                } else {
                    on[i - 1].feet_s().1 / len
                };
                let end = on.get(i + 1).map_or(1.0, |b| b.feet_s().0 / len);
                let mut checks = vec![*u1 - start, end - *u2, *u2 - *u1];
                if let Some((_, _, n1, _)) = js.get(i + 1) {
                    checks.push(*n1 - *u2);
                }
                if checks.iter().any(|v| v.abs() < band_u) {
                    return Verdict::Ambiguous;
                }
                if checks.iter().any(|v| *v < 0.0) {
                    fallback.get_or_insert("short");
                }
            }
        }
        for (j1, j2, _, _) in &js {
            with[k].push((*j1, *j2));
        }
    }
    if let Some(why) = fallback {
        return Verdict::Fallback(why);
    }
    let mut out = Vec::new();
    for k in 0..poly.n() {
        out.push(poly.corner(k));
        for (j1, j2) in &with[k] {
            out.push(*j1);
            out.push(*j2);
        }
    }
    c.closed_vertices_checked += out.len();
    Verdict::Cut(out)
}

fn cut_starts(p: &Project, piece: PieceId) -> Vec<PointMm> {
    p.piece_cut_contour(piece)
        .expect("la coupe ordinaire existe toujours")
        .traces
        .iter()
        .flat_map(trace_geometry)
        .map(|g| g.start())
        .collect()
}

fn same_cycle(got: &[PointMm], want: &[PointMm]) -> bool {
    if got.len() != want.len() {
        return false;
    }
    let n = want.len();
    (0..n).any(|k| (0..n).all(|i| dist(got[(i + k) % n], want[i]) < 1e-6))
}

// ----- construction de la pièce ----------------------------------------------------------------

fn make_poly(rng: &mut Rng, c: &mut Counters) -> Poly {
    loop {
        let rect = rng.chance(40);
        let v: Vec<PointMm> = if rect {
            let (x0, y0) = (rng.range(-200.0, 200.0), rng.range(-200.0, 200.0));
            let (w, h) = (rng.range(80.0, 300.0), rng.range(80.0, 300.0));
            vec![
                pt(x0, y0),
                pt(x0 + w, y0),
                pt(x0 + w, y0 + h),
                pt(x0, y0 + h),
            ]
        } else {
            let n = 4 + rng.below(4) as usize;
            let (cx, cy) = (rng.range(-100.0, 100.0), rng.range(-100.0, 100.0));
            let (rx, ry) = (rng.range(80.0, 200.0), rng.range(60.0, 200.0));
            let phi = rng.range(0.0, std::f64::consts::TAU);
            (0..n)
                .map(|k| {
                    let a =
                        phi + std::f64::consts::TAU * (k as f64 + rng.range(-0.2, 0.2)) / n as f64;
                    pt(cx + rx * a.cos(), cy + ry * a.sin())
                })
                .collect()
        };
        let base = if rng.chance(8) {
            0.0
        } else {
            rng.range(2.0, 15.0)
        };
        let m: Vec<(f64, f64)> = (0..v.len())
            .map(|_| {
                if base > 0.0 && rng.chance(40) {
                    let start = base * rng.range(1.0, 2.0);
                    // Marge progressive (début ≠ fin) sur un tiers des bords modifiés.
                    let end = if rng.chance(35) {
                        base * rng.range(1.0, 2.0)
                    } else {
                        start
                    };
                    (start, end)
                } else {
                    (base, base)
                }
            })
            .collect();
        let poly = Poly { v, m };
        let shortest = (0..poly.n())
            .map(|k| poly.edge(k).2)
            .fold(f64::INFINITY, f64::min);
        if shortest < 45.0 || poly.min_interior_angle_deg() < 60.0 {
            continue;
        }
        if rect {
            c.rectangles += 1;
        } else {
            c.polygons += 1;
        }
        return poly;
    }
}

struct World {
    doc: Document,
    sheet: SheetId,
    piece: PieceId,
    poly: Poly,
    darts: Vec<DartSpec>,
}

fn build(poly: &Poly) -> World {
    let mut p = Project::new("Campagne coupe pince");
    let sheet = p.sheets[0].id;
    let n = poly.n();
    for k in 0..n {
        p.apply(Command::AddSegment {
            sheet,
            from: PointRef::New(poly.v[k]),
            to: PointRef::New(poly.v[(k + 1) % n]),
        })
        .unwrap();
    }
    let centroid = pt(
        poly.v.iter().map(|q| q.x).sum::<f64>() / n as f64,
        poly.v.iter().map(|q| q.y).sum::<f64>() / n as f64,
    );
    let piece = p
        .apply(Command::ExtractPiece {
            sheet,
            source: PieceSource::Click { at: centroid },
            name: "Pièce".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    if poly.has_margin() {
        // Un tracé par bord : les marges par bord passent par les identifiants des tracés.
        let ids: Vec<EntityId> = (0..n)
            .map(|k| {
                let (a, b) = (poly.v[k], poly.v[(k + 1) % n]);
                p.piece(piece)
                    .unwrap()
                    .outline
                    .iter()
                    .map(|e| e.entity)
                    .find(|id| {
                        let (u, w) = p.sheets[0].segment_ends(*id).unwrap();
                        (dist(u, a) < 1e-6 && dist(w, b) < 1e-6)
                            || (dist(u, b) < 1e-6 && dist(w, a) < 1e-6)
                    })
                    .expect("tracé du bord")
            })
            .collect();
        let default = poly
            .m
            .iter()
            .flat_map(|m| [m.0, m.1])
            .fold(f64::INFINITY, f64::min);
        let edges: Vec<piece_seam::EdgeAllowance> = (0..n)
            .filter(|k| poly.m[*k] != (default, default))
            .map(|k| piece_seam::EdgeAllowance {
                entity: ids[k],
                start_mm: poly.m[k].0,
                end_mm: poly.m[k].1,
            })
            .collect();
        p.apply(Command::SetSeamAllowance {
            piece,
            seam: Some(piece_seam::SeamAllowance {
                default_mm: default,
                edges,
                ..Default::default()
            }),
        })
        .unwrap();
    }
    World {
        doc: Document::new(p),
        sheet,
        piece,
        poly: poly.clone(),
        darts: Vec::new(),
    }
}

/// Le bord `k` de la pièce a la marge `poly.m[k]` quelle que soit la place de la marge par défaut.
struct Gen {
    edge: usize,
    s_mid: f64,
    w: f64,
    l: f64,
    fixed: Side,
    corner: bool,
}

fn gen_dart(rng: &mut Rng, poly: &Poly) -> Gen {
    let edge = rng.below(poly.n() as u64) as usize;
    let len = poly.edge(edge).2;
    let w = rng.range(3.0, 30.0);
    let big = rng.chance(20);
    let l = if big {
        rng.range(w, 400.0)
    } else {
        rng.range(w.max(10.0), 90.0)
    };
    let corner = rng.chance(6);
    let s_mid = if corner {
        w / 2.0
    } else {
        rng.range(w / 2.0 + 3.0, (len - w / 2.0 - 3.0).max(w / 2.0 + 3.5))
    };
    Gen {
        edge,
        s_mid,
        w,
        l,
        fixed: if rng.chance(50) {
            Side::Left
        } else {
            Side::Right
        },
        corner,
    }
}

/// Décision de l'oracle sur la pose : `Some(true)` acceptée, `Some(false)` refusée, `None` ambigu.
fn add_expected(poly: &Poly, g: &Gen) -> Option<bool> {
    let (v0, d, len, n_in) = poly.edge(g.edge);
    if g.w > g.l {
        return Some(false);
    }
    let apex = add(add(v0, mul(d, g.s_mid)), mul(n_in, g.l));
    let dists = poly.inward_distances(apex);
    let least = dists.iter().copied().fold(f64::INFINITY, f64::min);
    if (0.0005..0.02).contains(&least) || (-0.02..=0.0005).contains(&least) {
        return None;
    }
    if least <= 0.0 {
        return Some(false);
    }
    let (f1, f2) = (g.s_mid - g.w / 2.0, g.s_mid + g.w / 2.0);
    if f1 < -1e-9 || f2 > len + 1e-9 {
        // Pied hors du bord : refus, sauf zone de tolérance du contour (non générée).
        if f1.min(len - f2) > -0.02 && f1.min(len - f2) < -1e-9 {
            return None;
        }
        return Some(false);
    }
    Some(true)
}

fn add_cmd(w: &World, g: &Gen) -> Command {
    let (v0, d, _, n_in) = w.poly.edge(g.edge);
    let m = add(v0, mul(d, g.s_mid));
    Command::AddPieceDart {
        piece: w.piece,
        apex: PointRef::New(add(m, mul(n_in, g.l))),
        toward: PointRef::New(m),
        length_mm: g.l,
        opening_mm: g.w,
        fixed: g.fixed,
    }
}

fn try_add(w: &mut World, g: &Gen, c: &mut Counters) -> bool {
    let expect = add_expected(&w.poly, g);
    let before = bytes(w.doc.project());
    let cmd = add_cmd(w, g);
    match (w.doc.execute(cmd), expect) {
        (Ok(out), Some(true) | None) => {
            let id = out.darts[0];
            let apex = w
                .doc
                .project()
                .piece(w.piece)
                .unwrap()
                .darts
                .last()
                .unwrap()
                .apex;
            w.darts.push(DartSpec {
                id,
                apex,
                edge: g.edge,
                s_mid: g.s_mid,
                w: g.w,
                l: g.l,
                fixed: g.fixed,
                closed: false,
                shift: 0.0,
            });
            c.darts_added += 1;
            true
        }
        (Err(_), Some(false) | None) => {
            assert_eq!(bytes(w.doc.project()), before, "refus sans trace");
            c.darts_refused += 1;
            c.refusals_without_trace += 1;
            false
        }
        (Ok(_), Some(false)) => panic!(
            "pince acceptée alors que l'oracle la refuse : arête {} s {} w {} L {} corner {}",
            g.edge, g.s_mid, g.w, g.l, g.corner
        ),
        (Err(e), Some(true)) => panic!(
            "pince refusée alors que l'oracle l'accepte ({e}) : arête {} s {} w {} L {} corner {}",
            g.edge, g.s_mid, g.w, g.l, g.corner
        ),
    }
}

// ----- contrôles après chaque étape ------------------------------------------------------------

fn check_state(w: &World, tag: &str, c: &mut Counters) {
    let p = w.doc.project();
    let verdict = expected_cut(&w.poly, &w.darts, c);
    let got = cut_starts(p, w.piece);
    let any_closed = w.darts.iter().any(|d| d.closed);
    let issue = p
        .piece_issues(w.piece)
        .iter()
        .any(|i| matches!(i, PieceIssue::CutDartFailed { .. }));
    let reason = p.piece_cut_dart_failure(w.piece);
    match verdict {
        Verdict::Ambiguous => c.ambiguous_skipped += 1,
        Verdict::Cut(want) => {
            assert!(
                same_cycle(&got, &want),
                "{tag} : coupe {got:?} ≠ attendue {want:?} (repli du moteur : {reason:?})"
            );
            assert!(
                !issue && reason.is_none(),
                "{tag} : contrôle inattendu {reason:?}"
            );
            if any_closed && w.poly.has_margin() {
                c.closed_ok_checked += 1;
                let two = (0..w.poly.n())
                    .any(|k| w.darts.iter().filter(|d| d.closed && d.edge == k).count() >= 2);
                if two {
                    c.closed_two_on_one_edge += 1;
                }
            } else {
                c.plain_checked += 1;
            }
        }
        Verdict::Fallback(why) => {
            let plain: Vec<PointMm> = (0..w.poly.n()).map(|k| w.poly.corner(k)).collect();
            assert!(
                same_cycle(&got, &plain),
                "{tag} : repli attendu ({why}) mais la coupe {got:?} n'est pas l'ordinaire"
            );
            assert!(
                issue && reason.is_some(),
                "{tag} : contrôle CutDartFailed attendu ({why})"
            );
            c.fallback_expected += 1;
            c.issue_checked += 1;
            match why {
                "corner" => c.fallback_corner += 1,
                "touch" => c.fallback_touch += 1,
                "short" => c.fallback_short += 1,
                _ => c.fallback_invalid += 1,
            }
        }
    }
    // Sans pince réglée : aucun octet de réglage dans le fichier.
    if !any_closed {
        assert!(
            !bytes(p).contains("cut_closed"),
            "{tag} : champ écrit alors que faux"
        );
    }
}

fn check_json(w: &World, c: &mut Counters) {
    let text = bytes(w.doc.project());
    let back = persist::from_json(&text).unwrap();
    assert_eq!(bytes(&back), text, "relecture JSON identique");
    let (a, b) = (
        cut_starts(w.doc.project(), w.piece),
        cut_starts(&back, w.piece),
    );
    assert!(same_cycle(&a, &b), "coupe identique après relecture");
    c.json_roundtrips += 1;
}

// ----- une séquence ---------------------------------------------------------------------------

#[allow(clippy::needless_range_loop)]
fn sequence(seed: u64, c: &mut Counters) {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15 ^ seed.wrapping_mul(0xBF58_476D_1CE4_E5B9));
    for _ in 0..4 {
        rng.step();
    }
    let poly = make_poly(&mut rng, c);
    let mut w = build(&poly);
    let tag = |s: &str| format!("séquence {seed} : {s}");
    check_state(&w, &tag("départ"), c);
    let mut snapshots = vec![bytes(w.doc.project())];
    let snap = |w: &World, snapshots: &mut Vec<String>| {
        let now = bytes(w.doc.project());
        if snapshots.last() != Some(&now) {
            snapshots.push(now);
        }
    };

    // Pinces : 1 à 3 poses (certaines impossibles).
    let wanted = 1 + rng.below(3) as usize;
    for _ in 0..wanted {
        let g = gen_dart(&mut rng, &w.poly);
        if try_add(&mut w, &g, c) {
            snap(&w, &mut snapshots);
        }
    }
    check_state(&w, &tag("poses"), c);
    // Réglages : chaque pince posée est réglée « coupe fermée » avec 75 % de chances.
    let ids: Vec<DartId> = w.darts.iter().map(|d| d.id).collect();
    for id in ids {
        if rng.chance(75) {
            w.doc
                .execute(Command::SetPieceDartCutClosed {
                    piece: w.piece,
                    dart: id,
                    closed: true,
                })
                .unwrap();
            w.darts.iter_mut().find(|d| d.id == id).unwrap().closed = true;
            snap(&w, &mut snapshots);
        }
    }
    check_state(&w, &tag("réglages"), c);
    check_json(&w, c);

    // Mutations.
    for step in 0..(2 + rng.below(4)) {
        if w.darts.is_empty() {
            break;
        }
        let i = rng.below(w.darts.len() as u64) as usize;
        let id = w.darts[i].id;
        let label = format!("étape {step}");
        match rng.below(6) {
            0 => {
                let now = !w.darts[i].closed;
                w.doc
                    .execute(Command::SetPieceDartCutClosed {
                        piece: w.piece,
                        dart: id,
                        closed: now,
                    })
                    .unwrap();
                w.darts[i].closed = now;
                c.toggles += 1;
            }
            1 => {
                // Ouverture modifiée (même sommet, même axe, même longueur).
                let d = *w
                    .doc
                    .project()
                    .piece(w.piece)
                    .unwrap()
                    .darts
                    .iter()
                    .find(|d| d.id == id)
                    .unwrap();
                let new_w = rng.range(3.0, w.darts[i].l.clamp(3.5, 30.0));
                let len = w.poly.edge(w.darts[i].edge).2;
                let (f1, f2) = (
                    w.darts[i].s_mid - new_w / 2.0,
                    w.darts[i].s_mid + new_w / 2.0,
                );
                let inside = f1 > 0.02 && f2 < len - 0.02;
                let outside = f1 < -0.02 || f2 > len + 0.02;
                if !(inside || outside) {
                    continue;
                }
                let before = bytes(w.doc.project());
                let r = w.doc.execute(Command::SetPieceDart {
                    piece: w.piece,
                    dart: id,
                    apex: d.apex,
                    toward: d.toward,
                    length_mm: d.length_mm,
                    opening_mm: new_w,
                    fixed: d.fixed,
                });
                // Une pince déplacée (impossible) fait refuser la modification (mêmes contrôles
                // que la pose) : l'oracle ne tranche que les pinces en place.
                if w.darts[i].invalid_now() {
                    if r.is_err() {
                        assert_eq!(bytes(w.doc.project()), before, "refus sans trace");
                        c.refusals_without_trace += 1;
                    }
                    continue;
                }
                match (r, inside) {
                    (Ok(_), true) => {
                        w.darts[i].w = new_w;
                        c.openings += 1;
                    }
                    (Err(_), false) => {
                        assert_eq!(bytes(w.doc.project()), before, "refus sans trace");
                        c.refusals_without_trace += 1;
                    }
                    (Ok(_), false) => panic!("{label} : ouverture hors du bord acceptée"),
                    (Err(e), true) => panic!("{label} : ouverture valide refusée : {e}"),
                }
            }
            2 => {
                // Sommet déplacé le long de l'axe (la base ne suit pas) puis, parfois, ramené.
                let (v0, d, _, n_in) = w.poly.edge(w.darts[i].edge);
                let target = if w.darts[i].shift != 0.0 && rng.chance(60) {
                    -w.darts[i].shift
                } else {
                    rng.range(5.0, 15.0) * if rng.chance(50) { 1.0 } else { -1.0 }
                };
                let m = add(v0, mul(d, w.darts[i].s_mid));
                let new_pos = add(m, mul(n_in, w.darts[i].l + w.darts[i].shift + target));
                // Garder le sommet dans la pièce (sinon la pince serait aussi « dehors »).
                let least = w
                    .poly
                    .inward_distances(new_pos)
                    .iter()
                    .copied()
                    .fold(f64::INFINITY, f64::min);
                if least < 0.1 {
                    continue;
                }
                w.doc
                    .execute(Command::MovePoint {
                        sheet: w.sheet,
                        point: w.darts[i].apex,
                        to: new_pos,
                    })
                    .unwrap();
                w.darts[i].shift += target;
                if w.darts[i].shift.abs() < 1e-9 {
                    w.darts[i].shift = 0.0;
                }
                c.moves += 1;
            }
            3 => {
                w.doc
                    .execute(Command::RemovePieceDart {
                        piece: w.piece,
                        dart: id,
                    })
                    .unwrap();
                w.darts.remove(i);
                c.removes += 1;
            }
            4 => {
                // Refus sans trace : pince ou pièce inconnue.
                let before = bytes(w.doc.project());
                for cmd in [
                    Command::SetPieceDartCutClosed {
                        piece: w.piece,
                        dart: DartId(987_654_321),
                        closed: true,
                    },
                    Command::SetPieceDartCutClosed {
                        piece: PieceId(987_654_321),
                        dart: id,
                        closed: true,
                    },
                ] {
                    assert!(w.doc.execute(cmd).is_err());
                    assert_eq!(bytes(w.doc.project()), before, "refus sans trace (octets)");
                    c.refusals_without_trace += 1;
                }
            }
            _ => {
                let g = gen_dart(&mut rng, &w.poly);
                try_add(&mut w, &g, c);
            }
        }
        snap(&w, &mut snapshots);
        check_state(&w, &tag(&label), c);
    }
    check_json(&w, c);

    // Annuler / rétablir : octets exacts à chaque étape.
    let end = bytes(w.doc.project());
    let mut undone = 0;
    while w.doc.can_undo() {
        assert!(w.doc.undo());
        undone += 1;
        let at = snapshots.len().saturating_sub(1 + undone);
        assert_eq!(
            bytes(w.doc.project()),
            snapshots[at],
            "{} : annuler {undone}",
            tag("historique")
        );
    }
    assert_eq!(
        undone,
        snapshots.len() - 1,
        "une étape d'historique par changement"
    );
    for k in 1..=undone {
        assert!(w.doc.redo());
        assert_eq!(
            bytes(w.doc.project()),
            snapshots[k],
            "{} : rétablir {k}",
            tag("historique")
        );
    }
    assert_eq!(bytes(w.doc.project()), end);
    c.undo_redo_checks += 1;
    c.sequences += 1;
}

#[test]
fn campagne_adverse_coupe_pince_fermee() {
    let mut c = Counters::default();
    for seed in 0..170u64 {
        sequence(seed, &mut c);
    }
    println!("{c:#?}");
    assert_eq!(c.sequences, 170);
    // Les contrôles forts se sont exécutés.
    assert!(c.rectangles >= 40 && c.polygons >= 40, "{c:?}");
    assert!(c.darts_added >= 150, "{c:?}");
    assert!(c.darts_refused >= 15, "{c:?}");
    assert!(c.refusals_without_trace >= 30, "{c:?}");
    assert!(c.closed_ok_checked >= 150, "{c:?}");
    assert!(c.closed_two_on_one_edge >= 3, "{c:?}");
    assert!(c.plain_checked >= 100, "{c:?}");
    assert!(c.closed_vertices_checked >= 800, "{c:?}");
    assert!(c.rotation_oracle_checked >= 200, "{c:?}");
    assert!(c.formula_oracle_checked >= 200, "{c:?}");
    assert!(c.invariant_checked >= 200, "{c:?}");
    assert!(c.fallback_expected >= 20, "{c:?}");
    assert!(c.fallback_invalid >= 3, "{c:?}");
    assert!(c.fallback_corner >= 1, "{c:?}");
    assert!(c.fallback_touch >= 1, "{c:?}");
    assert!(c.fallback_short >= 1, "{c:?}");
    assert!(c.issue_checked >= 20, "{c:?}");
    assert!(c.json_roundtrips >= 300, "{c:?}");
    assert!(c.undo_redo_checks == 170, "{c:?}");
    assert!(
        c.moves >= 20 && c.toggles >= 20 && c.openings >= 10 && c.removes >= 10,
        "{c:?}"
    );
    assert!(
        c.ambiguous_skipped * 20 <= c.sequences * 20,
        "trop d'ambigus : {c:?}"
    );
}
