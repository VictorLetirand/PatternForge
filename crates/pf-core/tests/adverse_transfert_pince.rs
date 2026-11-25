//! US-07.2 — campagne adverse du transfert de pince : 180 séquences à graine fixe. Chaque
//! séquence pose une pince sur un rectangle aléatoire (côté, position, longueur, ouverture, jambe
//! fixe) puis enchaîne des transferts (cible aléatoire, part totale / 50 % / aléatoire, valeurs
//! invalides, cible sur la base ou sur un pied), des retraits de la pince restante, des annulations
//! et des rétablissements.
//!
//! **Oracle indépendant** : il ne lit rien du code testé. Il tient son **propre polygone** (liste de
//! sommets anti-horaires), place lui-même les pieds (`M ± s·w/2`), insère la cible et les pieds
//! dans le polygone, tourne la partie mobile avec `cos` / `sin` d'un angle obtenu par différence
//! d'`atan2` (le code utilise produit vectoriel / scalaire), construit le nouveau polygone, décide
//! seul si le transfert doit être accepté (polygone simple, sommet strictement dedans, jambes qui
//! ne traversent rien, bornes de la pince) et, s'il l'est, compare : ensemble des sommets du
//! contour, ouverture / longueur / côté fixe de chaque pince, somme des angles, longueurs des
//! bords hors pince. Les cas à moins de 1e-3 mm d'une frontière de décision sont **écartés** (et
//! comptés). Des **compteurs** prouvent que les contrôles forts s'exécutent. Mutations vérifiées
//! à la main : voir `docs/JOURNAL.md` (tranche 62).

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
    refused: usize,
    refused_justified: usize,
    ambiguous_skipped: usize,
    vertex_sets_checked: usize,
    angle_sums_checked: usize,
    kept_lengths_checked: usize,
    darts_checked: usize,
    fixed_side_checked: usize,
    undo_redo_checks: usize,
    refusals_without_trace: usize,
    chained: usize,
    two_darts_refused: usize,
    partials: usize,
    totals: usize,
}

// ---------------------------------------------------------------- géométrie de l'oracle

fn sub(a: PointMm, b: PointMm) -> (f64, f64) {
    (a.x - b.x, a.y - b.y)
}

fn dist(a: PointMm, b: PointMm) -> f64 {
    let (dx, dy) = sub(a, b);
    dx.hypot(dy)
}

fn rot(p: PointMm, c: PointMm, ang: f64) -> PointMm {
    let (dx, dy) = sub(p, c);
    let (s, co) = ang.sin_cos();
    pt(c.x + dx * co - dy * s, c.y + dx * s + dy * co)
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

fn cross3(o: PointMm, a: PointMm, b: PointMm) -> f64 {
    (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x)
}

/// Les segments `ab` et `cd` se croisent ou se touchent-ils ?
fn seg_touch(a: PointMm, b: PointMm, c: PointMm, d: PointMm) -> bool {
    let (d1, d2, d3, d4) = (
        cross3(a, b, c),
        cross3(a, b, d),
        cross3(c, d, a),
        cross3(c, d, b),
    );
    if ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0))
        && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0))
    {
        return true;
    }
    seg_point_dist(c, a, b) < 1e-9
        || seg_point_dist(d, a, b) < 1e-9
        || seg_point_dist(a, c, d) < 1e-9
        || seg_point_dist(b, c, d) < 1e-9
}

/// Distance minimale entre deux segments (0 s'ils se touchent).
fn seg_seg_dist(a: PointMm, b: PointMm, c: PointMm, d: PointMm) -> f64 {
    if seg_touch(a, b, c, d) {
        return 0.0;
    }
    [
        seg_point_dist(a, c, d),
        seg_point_dist(b, c, d),
        seg_point_dist(c, a, b),
        seg_point_dist(d, a, b),
    ]
    .into_iter()
    .fold(f64::INFINITY, f64::min)
}

fn inside(poly: &[PointMm], p: PointMm) -> bool {
    let mut c = false;
    let n = poly.len();
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x) {
            c = !c;
        }
    }
    c
}

fn boundary_dist(poly: &[PointMm], p: PointMm) -> f64 {
    let n = poly.len();
    (0..n)
        .map(|i| seg_point_dist(p, poly[i], poly[(i + 1) % n]))
        .fold(f64::INFINITY, f64::min)
}

fn area(poly: &[PointMm]) -> f64 {
    let n = poly.len();
    (0..n)
        .map(|i| {
            let (a, b) = (poly[i], poly[(i + 1) % n]);
            a.x * b.y - b.x * a.y
        })
        .sum::<f64>()
        / 2.0
}

fn dedupe(poly: Vec<PointMm>) -> Vec<PointMm> {
    let mut out: Vec<PointMm> = Vec::new();
    for p in poly {
        if out.last().is_none_or(|l| dist(*l, p) > 1e-7) {
            out.push(p);
        }
    }
    while out.len() > 1 && dist(out[0], out[out.len() - 1]) <= 1e-7 {
        out.pop();
    }
    out
}

/// `Some(true)` simple, `Some(false)` se recoupe, `None` ambigu (arêtes à moins de 1e-3).
fn simple(poly: &[PointMm]) -> Option<bool> {
    let n = poly.len();
    if n < 3 {
        return Some(false);
    }
    let mut ambiguous = false;
    for i in 0..n {
        for j in i + 1..n {
            let adjacent = j == i + 1 || (i == 0 && j == n - 1);
            let (a, b) = (poly[i], poly[(i + 1) % n]);
            let (c, d) = (poly[j], poly[(j + 1) % n]);
            if adjacent {
                // Repli sur lui-même : arêtes colinéaires de sens opposé.
                let (u, v) = if j == i + 1 { (b, d) } else { (a, c) };
                let shared = if j == i + 1 { b } else { a };
                let (e1, e2) = (sub(u, shared), sub(v, shared));
                let cr = e1.0 * e2.1 - e1.1 * e2.0;
                let dt = e1.0 * e2.0 + e1.1 * e2.1;
                if cr.abs() < 1e-9 * (e1.0.hypot(e1.1) * e2.0.hypot(e2.1)) && dt > 0.0 {
                    return Some(false);
                }
                continue;
            }
            let dd = seg_seg_dist(a, b, c, d);
            if dd == 0.0 {
                return Some(false);
            }
            if dd < 1e-3 {
                ambiguous = true;
            }
        }
    }
    if ambiguous {
        None
    } else {
        Some(true)
    }
}

/// Insère `p` dans la liste cyclique (sur l'arête qui le porte) ; rend son indice. `None` si aucun
/// bord ne le porte.
fn insert(poly: &mut Vec<PointMm>, p: PointMm) -> Option<usize> {
    if let Some(i) = poly.iter().position(|v| dist(*v, p) < 1e-9) {
        return Some(i);
    }
    let n = poly.len();
    let i = (0..n).find(|&i| seg_point_dist(p, poly[i], poly[(i + 1) % n]) < 1e-7)?;
    poly.insert(i + 1, p);
    Some(i + 1)
}

// ---------------------------------------------------------------- état de l'oracle

#[derive(Clone, Copy, Debug)]
struct OracleDart {
    id: DartId,
    apex: PointMm,
    fixed_foot: PointMm,
    moving_foot: PointMm,
}

impl OracleDart {
    fn mid(&self) -> PointMm {
        pt(
            (self.fixed_foot.x + self.moving_foot.x) / 2.0,
            (self.fixed_foot.y + self.moving_foot.y) / 2.0,
        )
    }
    fn opening(&self) -> f64 {
        dist(self.fixed_foot, self.moving_foot)
    }
    fn length(&self) -> f64 {
        dist(self.apex, self.mid())
    }
    fn angle_deg(&self) -> f64 {
        2.0 * (self.opening() / 2.0).atan2(self.length()).to_degrees()
    }
    /// Côté du pied fixe par rapport à l'axe sommet → milieu de base.
    fn expected_fixed_side(&self) -> Side {
        if cross3(self.apex, self.mid(), self.fixed_foot) > 0.0 {
            Side::Left
        } else {
            Side::Right
        }
    }
}

#[derive(Clone)]
struct Model {
    poly: Vec<PointMm>,
    darts: Vec<OracleDart>,
}

enum Expect {
    Refuse(&'static str),
    Ambiguous,
    /// Nouveau polygone, nouvelles pinces (la restante a l'identifiant 0 : à remplacer), longueur
    /// des bords hors pince attendue.
    Accept(Vec<PointMm>, Vec<OracleDart>, f64),
}

fn perimeter(poly: &[PointMm]) -> f64 {
    let n = poly.len();
    (0..n).map(|i| dist(poly[i], poly[(i + 1) % n])).sum()
}

fn perimeter_open(v: &[PointMm]) -> f64 {
    v.windows(2).map(|w| dist(w[0], w[1])).sum()
}

fn position(poly: &[PointMm], p: PointMm) -> Option<usize> {
    poly.iter().position(|v| dist(*v, p) < 1e-9)
}

/// Décision et résultat attendus du transfert de `dart` vers `target` avec la part `part`.
fn oracle_transfer(m: &Model, dart: OracleDart, target: PointMm, part: f64) -> Expect {
    if !part.is_finite() || part <= 0.0 || part > 1.0 {
        return Expect::Refuse("part");
    }
    if m.darts.len() != 1 {
        return Expect::Refuse("autres pinces");
    }
    let mut poly = m.poly.clone();
    if insert(&mut poly, dart.fixed_foot).is_none() || insert(&mut poly, dart.moving_foot).is_none()
    {
        return Expect::Ambiguous;
    }
    let n = poly.len();
    let (Some(ifx), Some(imv)) = (
        position(&poly, dart.fixed_foot),
        position(&poly, dart.moving_foot),
    ) else {
        return Expect::Ambiguous;
    };
    // Base = arête entre les deux pieds ; `fa` la quitte en dernier, `fb` la suit (anti-horaire).
    let (fa, fb) = if (ifx + 1) % n == imv {
        (dart.fixed_foot, dart.moving_foot)
    } else if (imv + 1) % n == ifx {
        (dart.moving_foot, dart.fixed_foot)
    } else {
        return Expect::Ambiguous;
    };
    if seg_point_dist(target, fa, fb) < 1e-6 {
        return Expect::Refuse("base");
    }
    if poly
        .iter()
        .any(|v| dist(*v, target) > 1e-9 && dist(*v, target) < 1e-5)
    {
        return Expect::Ambiguous;
    }
    if insert(&mut poly, target).is_none() {
        return Expect::Ambiguous;
    }
    let ib = position(&poly, fb).unwrap();
    let mut c: Vec<PointMm> = poly[ib..].to_vec();
    c.extend_from_slice(&poly[..ib]);
    let it = position(&c, target).unwrap();
    let (x, y) = (c[..=it].to_vec(), c[it..].to_vec());
    if perimeter_open(&x) < 1e-5 || perimeter_open(&y) < 1e-5 {
        return Expect::Ambiguous;
    }
    // Rotation : angle du pied mobile vers le pied fixe (différence d'atan2), part de cet angle.
    let a = dart.apex;
    let am = (dart.moving_foot.y - a.y).atan2(dart.moving_foot.x - a.x);
    let af = (dart.fixed_foot.y - a.y).atan2(dart.fixed_foot.x - a.x);
    let mut d = af - am;
    while d > std::f64::consts::PI {
        d -= 2.0 * std::f64::consts::PI;
    }
    while d <= -std::f64::consts::PI {
        d += 2.0 * std::f64::consts::PI;
    }
    let psi = d * part;
    let turn = |v: &[PointMm]| -> Vec<PointMm> { v.iter().map(|p| rot(*p, a, psi)).collect() };
    let x_moves = dist(dart.moving_foot, fb) < 1e-9;
    let new_poly: Vec<PointMm> = if x_moves {
        turn(&x).into_iter().chain(y.iter().copied()).collect()
    } else {
        x.iter().copied().chain(turn(&y)).collect()
    };
    let new_poly = dedupe(new_poly);
    let (t_turned, f_turned) = (rot(target, a, psi), rot(dart.moving_foot, a, psi));
    let mut darts = vec![OracleDart {
        id: dart.id,
        apex: a,
        fixed_foot: target,
        moving_foot: t_turned,
    }];
    if part < 1.0 {
        darts.push(OracleDart {
            id: DartId(0),
            apex: a,
            fixed_foot: dart.fixed_foot,
            moving_foot: f_turned,
        });
    }
    // Validité du résultat.
    match simple(&new_poly) {
        Some(false) => return Expect::Refuse("contour"),
        None => return Expect::Ambiguous,
        Some(true) => {}
    }
    if area(&new_poly) <= 1e-6 {
        return Expect::Refuse("aire");
    }
    for nd in &darts {
        let (l, w) = (nd.length(), nd.opening());
        let near = |v: f64, bound: f64| (v - bound).abs() < 1e-6;
        if near(l, 1.0) || near(l, 2000.0) || near(w, 0.1) || near(w, 1000.0) || near(w, l) {
            return Expect::Ambiguous;
        }
        if !(1.0..=2000.0).contains(&l) || !(0.1..=1000.0).contains(&w) || w > l {
            return Expect::Refuse("bornes");
        }
        let bd = boundary_dist(&new_poly, a);
        if !inside(&new_poly, a) || bd <= 0.5e-3 {
            return Expect::Refuse("sommet");
        }
        if bd < 2e-3 {
            return Expect::Ambiguous;
        }
        for foot in [nd.fixed_foot, nd.moving_foot] {
            let k = new_poly.len();
            for i in 0..k {
                let (p, q) = (new_poly[i], new_poly[(i + 1) % k]);
                if dist(p, foot) < 1e-9 || dist(q, foot) < 1e-9 {
                    continue;
                }
                let dd = seg_seg_dist(a, foot, p, q);
                if dd == 0.0 {
                    return Expect::Refuse("jambe");
                }
                if dd < 1e-4 {
                    return Expect::Ambiguous;
                }
            }
        }
    }
    let kept = perimeter(&m.poly) - dart.opening();
    Expect::Accept(new_poly, darts, kept)
}

// ---------------------------------------------------------------- lecture du projet (vérification)

/// Segments du contour de la pièce (départ, arrivée), dans l'ordre du contour.
fn outline_edges(p: &Project, piece: PieceId) -> Vec<(PointMm, PointMm)> {
    let sheet = p.piece_sheet(piece).unwrap();
    p.piece(piece)
        .unwrap()
        .outline
        .iter()
        .map(|e| match sheet.shape(e.entity).unwrap() {
            Shape::Line(a, b) => {
                if e.reversed {
                    (b, a)
                } else {
                    (a, b)
                }
            }
            other => panic!("segment attendu : {other:?}"),
        })
        .collect()
}

/// (tracé, distance depuis son début) du point `at` : adresse d'une cible dans le projet.
fn address(p: &Project, piece: PieceId, at: PointMm) -> Option<(EntityId, f64)> {
    let sheet = p.piece_sheet(piece).ok()?;
    for e in &p.piece(piece).ok()?.outline {
        if let Shape::Line(a, b) = sheet.shape(e.entity).ok()? {
            if seg_point_dist(at, a, b) < 1e-7 {
                return Some((e.entity, dist(a, at)));
            }
        }
    }
    None
}

fn same_set(a: &[PointMm], b: &[PointMm]) -> bool {
    a.iter().all(|p| b.iter().any(|q| dist(*p, *q) < 1e-6))
        && b.iter().all(|p| a.iter().any(|q| dist(*p, *q) < 1e-6))
}

// ---------------------------------------------------------------- une séquence

fn run_sequence(seed: u64, c: &mut Counters) {
    let mut r = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    for _ in 0..4 {
        r.step();
    }
    let (x0, y0) = (r.range(-200.0, 200.0), r.range(-200.0, 200.0));
    let (w, h) = (r.range(140.0, 400.0), r.range(140.0, 300.0));
    let v = [
        pt(x0, y0),
        pt(x0 + w, y0),
        pt(x0 + w, y0 + h),
        pt(x0, y0 + h),
    ];
    let mut p = Project::new("Adverse transfert");
    let s = p.sheets[0].id;
    p.apply(Command::AddRectangle {
        sheet: s,
        corner: PointRef::New(v[0]),
        width_mm: w,
        height_mm: h,
    })
    .unwrap();
    let piece = p
        .apply(Command::ExtractPiece {
            sheet: s,
            source: PieceSource::Click {
                at: pt(x0 + w / 2.0, y0 + h / 2.0),
            },
            name: "Pièce".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    // Pince : côté k, perpendiculaire au côté, pieds M ± s·w/2 (gauche = +s).
    let k = r.below(4) as usize;
    let (a, b) = (v[k], v[(k + 1) % 4]);
    let side_len = dist(a, b);
    let su = pt((b.x - a.x) / side_len, (b.y - a.y) / side_len);
    let n_in = pt(-su.y, su.x);
    let extent = if k % 2 == 0 { h } else { w };
    let length = r.range(20.0, 0.8 * extent);
    let opening = r.range(3.0, length.min(40.0));
    let t = r.range(30.0 / side_len, 1.0 - 30.0 / side_len);
    let base = pt(a.x + su.x * t * side_len, a.y + su.y * t * side_len);
    let apex = pt(base.x + n_in.x * length, base.y + n_in.y * length);
    let fixed = if r.chance(50) {
        Side::Left
    } else {
        Side::Right
    };
    let left = pt(base.x + su.x * opening / 2.0, base.y + su.y * opening / 2.0);
    let right = pt(base.x - su.x * opening / 2.0, base.y - su.y * opening / 2.0);
    let (ff, mf) = if fixed == Side::Left {
        (left, right)
    } else {
        (right, left)
    };
    let mut doc = Document::new(p);
    let dart = match doc.execute(Command::AddPieceDart {
        piece,
        apex: PointRef::New(apex),
        toward: PointRef::New(base),
        length_mm: length,
        opening_mm: opening,
        fixed,
    }) {
        Ok(o) => o.darts[0],
        Err(_) => return,
    };
    c.sequences += 1;
    let mut model = Model {
        poly: v.to_vec(),
        darts: vec![OracleDart {
            id: dart,
            apex,
            fixed_foot: ff,
            moving_foot: mf,
        }],
    };
    let steps = 1 + r.below(4);
    for _ in 0..steps {
        let before = bytes(doc.project());
        // Retrait de l'une des deux pinces (après un transfert partiel).
        if model.darts.len() == 2 && r.chance(40) {
            let victim = model.darts[r.below(2) as usize].id;
            doc.execute(Command::RemovePieceDart {
                piece,
                dart: victim,
            })
            .unwrap();
            model.darts.retain(|d| d.id != victim);
            continue;
        }
        let od = model.darts[r.below(model.darts.len() as u64) as usize];
        // Cible.
        let n = model.poly.len();
        let target = match r.below(100) {
            0..=14 => {
                // Sur la base (entre les pieds) : refus attendu.
                let u = r.unit();
                pt(
                    od.fixed_foot.x + (od.moving_foot.x - od.fixed_foot.x) * u,
                    od.fixed_foot.y + (od.moving_foot.y - od.fixed_foot.y) * u,
                )
            }
            15..=19 => model.poly[r.below(n as u64) as usize],
            _ => {
                let j = r.below(n as u64) as usize;
                let (p0, p1) = (model.poly[j], model.poly[(j + 1) % n]);
                let u = r.range(0.03, 0.97);
                pt(p0.x + (p1.x - p0.x) * u, p0.y + (p1.y - p0.y) * u)
            }
        };
        let part = match r.below(100) {
            0..=34 => 1.0,
            35..=54 => 0.5,
            55..=89 => r.range(0.02, 0.98),
            _ => [0.0, -0.3, 1.0000001, 2.0, f64::NAN][r.below(5) as usize],
        };
        let Some((edge, distance)) = address(doc.project(), piece, target) else {
            c.ambiguous_skipped += 1;
            continue;
        };
        // Distance invalide : toujours refusée, sans trace.
        if r.chance(6) {
            let bad = [-1.0, 1e7, f64::NAN][r.below(3) as usize];
            let res = doc.execute(Command::TransferPieceDart {
                piece,
                dart: od.id,
                edge,
                distance_mm: bad,
                part: 1.0,
            });
            assert!(res.is_err(), "seed {seed} : distance {bad} acceptée");
            assert_eq!(before, bytes(doc.project()), "seed {seed}");
            c.refusals_without_trace += 1;
            continue;
        }
        c.attempts += 1;
        let expect = oracle_transfer(&model, od, target, part);
        let res = doc.execute(Command::TransferPieceDart {
            piece,
            dart: od.id,
            edge,
            distance_mm: distance,
            part,
        });
        match expect {
            Expect::Refuse(why) => {
                assert!(
                    res.is_err(),
                    "seed {seed} : transfert accepté, refus attendu ({why}) ; cible {target:?}, \
                     part {part}"
                );
                assert_eq!(
                    before,
                    bytes(doc.project()),
                    "seed {seed} : trace d'un refus"
                );
                c.refused += 1;
                c.refused_justified += 1;
                c.refusals_without_trace += 1;
                if why == "autres pinces" {
                    c.two_darts_refused += 1;
                }
            }
            Expect::Ambiguous => {
                c.ambiguous_skipped += 1;
                if res.is_ok() {
                    return; // le modèle ne peut plus suivre
                }
                assert_eq!(before, bytes(doc.project()), "seed {seed}");
            }
            Expect::Accept(new_poly, mut darts, kept) => {
                let out = res.unwrap_or_else(|e| {
                    panic!(
                        "seed {seed} : transfert refusé ({e}), accepté attendu ; cible {target:?}, \
                         part {part}, pince {od:?}"
                    )
                });
                c.accepted += 1;
                if part < 1.0 {
                    c.partials += 1;
                } else {
                    c.totals += 1;
                }
                if model.poly.len() > 4 {
                    c.chained += 1;
                }
                let proj = doc.project();
                // 1. Sommets du contour.
                let got: Vec<PointMm> = outline_edges(proj, piece).iter().map(|e| e.0).collect();
                assert_eq!(
                    got.len(),
                    new_poly.len(),
                    "seed {seed} : {got:?} contre {new_poly:?}"
                );
                assert!(
                    same_set(&got, &new_poly),
                    "seed {seed} : sommets {got:?} contre {new_poly:?}"
                );
                c.vertex_sets_checked += 1;
                // 2. Pinces : identifiants, ouverture, longueur, sommet, côté fixe.
                if darts.len() == 2 {
                    darts[1].id = out.darts[1];
                }
                let stored = &proj.piece(piece).unwrap().darts;
                assert_eq!(stored.len(), darts.len(), "seed {seed}");
                let calcs = proj.piece_darts(piece);
                for od2 in &darts {
                    let sd = stored.iter().find(|d| d.id == od2.id).expect("pince");
                    assert!((sd.opening_mm - od2.opening()).abs() < 1e-6, "seed {seed}");
                    assert!((sd.length_mm - od2.length()).abs() < 1e-6, "seed {seed}");
                    assert_eq!(sd.fixed, od2.expected_fixed_side(), "seed {seed}");
                    let apex_pos = proj.piece_sheet(piece).unwrap().position(sd.apex).unwrap();
                    assert!(dist(apex_pos, od2.apex) < 1e-9, "seed {seed}");
                    let calc = calcs
                        .iter()
                        .find(|c| c.dart.id == od2.id)
                        .unwrap()
                        .calc
                        .as_ref()
                        .expect("pince calculable");
                    assert!((calc.shape.angle_deg - od2.angle_deg()).abs() < 1e-6);
                    c.darts_checked += 1;
                    c.fixed_side_checked += 1;
                }
                // 3. Somme des angles conservée.
                let sum: f64 = calcs
                    .iter()
                    .map(|c| c.calc.as_ref().unwrap().shape.angle_deg)
                    .sum();
                assert!(
                    (sum - od.angle_deg()).abs() < 1e-6,
                    "seed {seed} : somme {sum}° contre {}°",
                    od.angle_deg()
                );
                c.angle_sums_checked += 1;
                // 4. Longueurs hors pince conservées (bases des pinces actuelles exclues).
                let is_base = |e: &(PointMm, PointMm)| {
                    darts.iter().any(|d| {
                        (dist(e.0, d.fixed_foot) < 1e-6 && dist(e.1, d.moving_foot) < 1e-6)
                            || (dist(e.0, d.moving_foot) < 1e-6 && dist(e.1, d.fixed_foot) < 1e-6)
                    })
                };
                let kept_got: f64 = outline_edges(proj, piece)
                    .iter()
                    .filter(|e| !is_base(e))
                    .map(|e| dist(e.0, e.1))
                    .sum();
                assert!(
                    (kept_got - kept).abs() < 1e-6,
                    "seed {seed} : hors pince {kept_got} contre {kept}"
                );
                c.kept_lengths_checked += 1;
                // 5. Annulation puis rétablissement : une seule étape.
                if r.chance(50) {
                    let after = bytes(doc.project());
                    assert!(doc.undo());
                    assert_eq!(before, bytes(doc.project()), "seed {seed} : annulation");
                    assert!(doc.redo());
                    assert_eq!(after, bytes(doc.project()), "seed {seed} : rétablissement");
                    c.undo_redo_checks += 1;
                }
                // 6. Fichier : aller-retour.
                let text = bytes(doc.project());
                assert_eq!(
                    text,
                    bytes(&persist::from_json(&text).unwrap()),
                    "seed {seed}"
                );
                model = Model {
                    poly: new_poly,
                    darts,
                };
            }
        }
    }
}

#[test]
fn campagne_adverse_transfert_de_pince_180_sequences() {
    let mut c = Counters::default();
    for seed in 1..=240u64 {
        run_sequence(seed, &mut c);
    }
    eprintln!("{c:#?}");
    assert!(c.sequences >= 180, "{c:?}");
    assert!(c.accepted >= 150, "{c:?}");
    assert!(c.refused_justified >= 40, "{c:?}");
    assert!(c.partials >= 40 && c.totals >= 40, "{c:?}");
    assert!(c.chained >= 20, "{c:?}");
    assert!(c.two_darts_refused >= 5, "{c:?}");
    assert_eq!(c.vertex_sets_checked, c.accepted, "{c:?}");
    assert_eq!(c.angle_sums_checked, c.accepted, "{c:?}");
    assert_eq!(c.kept_lengths_checked, c.accepted, "{c:?}");
    assert!(c.darts_checked >= c.accepted, "{c:?}");
    assert!(c.undo_redo_checks >= 50, "{c:?}");
    assert!(c.refusals_without_trace >= c.refused_justified, "{c:?}");
    assert!(c.ambiguous_skipped * 10 <= c.attempts, "{c:?}");
}
