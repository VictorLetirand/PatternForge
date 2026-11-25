//! US-07.4 : campagne adverse de l'ampleur par ouverture en éventail, 200 séquences à graine fixe.
//! Chaque séquence crée un polygone convexe aléatoire (3 à 6 sommets sur une ellipse) extrait en
//! pièce, choisit un de ses côtés comme bord fixe (parfois donné à l'envers), puis tente des
//! ampleurs : 1 à 5 lignes à positions et ouvertures aléatoires, des commandes invalides, une
//! seconde ampleur sur la même pièce (refusée), des annulations et rétablissements.
//!
//! **Oracle indépendant** : il ne lit rien du code testé. Il tient son propre polygone (sommets
//! anti-horaires + drapeau « arête du bord fixe »), traite les lignes de la plus éloignée à la plus
//! proche en comptant lui-même les croisements de la droite perpendiculaire, décide seul si
//! l'ampleur est acceptée (une seule coupe en deux points, pivot sur le contour, rotation d'au plus
//! 90 degrés, contour qui ne se recoupe pas) puis compare : ensemble des sommets (segments alignés
//! fusionnés), aire (formule du lacet) = aire d'origine + somme de 1/2 L² sin θ, périmètre =
//! périmètre d'origine + somme des ouvertures, **longueur du bord fixe conservée à 1e-6 mm** (somme
//! des arêtes marquées), pivots et lèvres stockés, aperçu d'accord avec la commande. Les cas à moins
//! de 1e-3 mm d'une frontière de décision sont **écartés** (et comptés). Des compteurs prouvent que
//! les contrôles forts s'exécutent. Mutations vérifiées : voir `docs/JOURNAL.md` (tranche 64).

use pf_core::persist;
use pf_core::*;
use pf_geometry::PointMm;

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
    multi_line_accepted: usize,
    reversed_accepted: usize,
    refused_geometry: usize,
    refused_invalid: usize,
    refused_second: usize,
    ambiguous_skipped: usize,
    refusals_without_trace: usize,
    vertex_sets_checked: usize,
    areas_checked: usize,
    perimeters_checked: usize,
    fixed_edges_checked: usize,
    flares_checked: usize,
    lengths_checked: usize,
    previews_checked: usize,
    undo_redo_checks: usize,
    json_roundtrips: usize,
}

// ---------------------------------------------------------------- oracle

fn dist(a: PointMm, b: PointMm) -> f64 {
    (a.x - b.x).hypot(a.y - b.y)
}

fn shoelace(poly: &[PointMm]) -> f64 {
    let n = poly.len();
    0.5 * (0..n)
        .map(|i| poly[i].x * poly[(i + 1) % n].y - poly[(i + 1) % n].x * poly[i].y)
        .sum::<f64>()
}

fn perim(poly: &[PointMm]) -> f64 {
    (0..poly.len())
        .map(|i| dist(poly[i], poly[(i + 1) % poly.len()]))
        .sum()
}

fn seg_point_dist(p: PointMm, a: PointMm, b: PointMm) -> f64 {
    let (vx, vy) = (b.x - a.x, b.y - a.y);
    let l2 = vx * vx + vy * vy;
    let t = if l2 == 0.0 {
        0.0
    } else {
        (((p.x - a.x) * vx + (p.y - a.y) * vy) / l2).clamp(0.0, 1.0)
    };
    dist(p, pt(a.x + t * vx, a.y + t * vy))
}

/// Distance entre deux segments (0 s'ils se croisent proprement).
fn seg_seg_dist(a: PointMm, b: PointMm, c: PointMm, d: PointMm) -> f64 {
    let cr =
        |p: PointMm, q: PointMm, r: PointMm| (q.x - p.x) * (r.y - p.y) - (q.y - p.y) * (r.x - p.x);
    let (o1, o2, o3, o4) = (cr(a, b, c), cr(a, b, d), cr(c, d, a), cr(c, d, b));
    if o1 * o2 < 0.0 && o3 * o4 < 0.0 {
        return 0.0;
    }
    [
        seg_point_dist(c, a, b),
        seg_point_dist(d, a, b),
        seg_point_dist(a, c, d),
        seg_point_dist(b, c, d),
    ]
    .into_iter()
    .fold(f64::INFINITY, f64::min)
}

/// Plus petite distance entre deux arêtes non voisines.
fn min_nonadjacent_dist(poly: &[PointMm]) -> f64 {
    let n = poly.len();
    let mut best = f64::INFINITY;
    for i in 0..n {
        for j in (i + 2)..n {
            if i == 0 && j == n - 1 {
                continue;
            }
            best = best.min(seg_seg_dist(
                poly[i],
                poly[(i + 1) % n],
                poly[j],
                poly[(j + 1) % n],
            ));
        }
    }
    best
}

/// Retire les sommets alignés avec leurs voisins (à 1e-7 mm) et les doublons.
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
            let (ux, uy) = (r.x - p.x, r.y - p.y);
            let len = ux.hypot(uy);
            if len < 1e-12 {
                v.remove(i);
                removed = true;
                break;
            }
            let off = (ux * (q.y - p.y) - uy * (q.x - p.x)).abs() / len;
            let along = (ux * (q.x - p.x) + uy * (q.y - p.y)) / len;
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
struct OFlare {
    h: PointMm,
    t: PointMm,
    t2: PointMm,
    length: f64,
    opening: f64,
}

struct OResult {
    poly: Vec<PointMm>,
    fixed: Vec<bool>,
    flares: Vec<OFlare>,
    gain_area: f64,
    gain_perimeter: f64,
}

enum Verdict {
    Ok(Box<OResult>),
    Refuse,
    Ambiguous,
}

fn rotate(q: PointMm, c: PointMm, phi: f64) -> PointMm {
    let (s, co) = phi.sin_cos();
    let (dx, dy) = (q.x - c.x, q.y - c.y);
    pt(c.x + dx * co - dy * s, c.y + dx * s + dy * co)
}

/// Ampleur complète : `fi` = indice de l'arête fixe (de `poly[fi]` à `poly[fi + 1]`, anti-horaire),
/// `rev` = bord fixe donné de `poly[fi + 1]` vers `poly[fi]`, `lines` triées par position croissante.
fn o_run(poly: &[PointMm], fi: usize, rev: bool, lines: &[(f64, f64)]) -> Verdict {
    let n0 = poly.len();
    let (e0, e1) = (poly[fi], poly[(fi + 1) % n0]);
    let (a, b) = if rev { (e1, e0) } else { (e0, e1) };
    let len = dist(a, b);
    let (ux, uy) = ((b.x - a.x) / len, (b.y - a.y) / len);
    let mut cur: Vec<PointMm> = poly.to_vec();
    let mut fixed: Vec<bool> = (0..n0).map(|i| i == fi).collect();
    let mut flares: Vec<OFlare> = Vec::new();
    let (mut gain_area, mut gain_perimeter) = (0.0, 0.0);
    for &(at, o) in lines.iter().rev() {
        let h = pt(a.x + ux * at, a.y + uy * at);
        let sd: Vec<f64> = cur
            .iter()
            .map(|v| (v.x - h.x) * ux + (v.y - h.y) * uy)
            .collect();
        if sd.iter().any(|s| s.abs() < 1e-3) {
            return Verdict::Ambiguous;
        }
        let n = cur.len();
        let mut crossings: Vec<PointMm> = Vec::new();
        for i in 0..n {
            let (p, q) = (cur[i], cur[(i + 1) % n]);
            let (sp, sq) = (sd[i], sd[(i + 1) % n]);
            if sp * sq < 0.0 {
                let t = sp / (sp - sq);
                crossings.push(pt(p.x + t * (q.x - p.x), p.y + t * (q.y - p.y)));
            }
        }
        if crossings.len() != 2 {
            return Verdict::Refuse;
        }
        let (d0, d1) = (dist(crossings[0], h), dist(crossings[1], h));
        let top = if d0 <= d1 { crossings[1] } else { crossings[0] };
        if d0.min(d1) > 1e-6 {
            return Verdict::Refuse;
        }
        let l = dist(h, top);
        let s = (top.x - h.x) * (-uy) + (top.y - h.y) * ux;
        let ratio = o / (2.0 * l);
        if (ratio - std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-6 {
            return Verdict::Ambiguous;
        }
        if ratio > std::f64::consts::FRAC_1_SQRT_2 {
            return Verdict::Refuse;
        }
        let theta = 2.0 * ratio.asin();
        let phi = if s > 0.0 { -theta } else { theta };
        let tf = |p: PointMm, far: bool| if far { rotate(p, h, phi) } else { p };
        let mut out: Vec<PointMm> = Vec::new();
        let mut flags: Vec<bool> = Vec::new();
        for i in 0..n {
            let (p, q) = (cur[i], cur[(i + 1) % n]);
            let (sp, sq) = (sd[i], sd[(i + 1) % n]);
            out.push(tf(p, sp > 0.0));
            flags.push(fixed[i]);
            if sp * sq < 0.0 {
                let t = sp / (sp - sq);
                let x = pt(p.x + t * (q.x - p.x), p.y + t * (q.y - p.y));
                out.push(tf(x, sp > 0.0));
                flags.push(false);
                out.push(tf(x, sq > 0.0));
                flags.push(fixed[i]);
            }
        }
        // Retire les arêtes de longueur nulle (la corde au pivot) en gardant l'arête suivante.
        let mut k = 0;
        while k < out.len() && out.len() > 3 {
            let nx = (k + 1) % out.len();
            if dist(out[k], out[nx]) < 1e-9 {
                out.remove(k);
                flags.remove(k);
            } else {
                k += 1;
            }
        }
        for f in &mut flares {
            f.h = rotate(f.h, h, phi);
            f.t = rotate(f.t, h, phi);
            f.t2 = rotate(f.t2, h, phi);
        }
        flares.push(OFlare {
            h,
            t: top,
            t2: rotate(top, h, phi),
            length: l,
            opening: o,
        });
        gain_area += 0.5 * l * l * theta.sin();
        gain_perimeter += o;
        cur = out;
        fixed = flags;
    }
    flares.reverse();
    let dmin = min_nonadjacent_dist(&cur);
    if dmin < 1e-7 {
        return Verdict::Refuse;
    }
    if dmin < 1e-3 {
        return Verdict::Ambiguous;
    }
    Verdict::Ok(Box::new(OResult {
        poly: cur,
        fixed,
        flares,
        gain_area,
        gain_perimeter,
    }))
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
    let mut p = Project::new("Adverse ampleur");
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

fn command(piece: PieceId, from: PointMm, to: PointMm, lines: &[(f64, f64)]) -> Command {
    Command::AddPieceFlare {
        piece,
        fixed_from: from,
        fixed_to: to,
        lines: lines
            .iter()
            .map(|&(at, o)| FlareLineSpec {
                at_mm: at,
                opening_mm: o,
            })
            .collect(),
    }
}

/// Contrôles forts d'un état accepté.
fn check_state(
    seed: u64,
    c: &mut Counters,
    proj: &Project,
    piece: PieceId,
    res: &OResult,
    original: &[PointMm],
    fixed_len: f64,
) {
    // 1. Ensemble des sommets (segments alignés fusionnés).
    let want = simplified(&res.poly);
    let got = outline_vertices(proj, piece);
    assert!(
        same_set(&got, &want, 1e-6),
        "seed {seed} : sommets {got:?} contre {want:?}"
    );
    c.vertex_sets_checked += 1;
    // 2. Aire : lacet de l'oracle, et gain = somme des 1/2 L² sin θ.
    let contour = proj.piece_contour(piece).unwrap();
    assert!(
        (contour.area_mm2 - shoelace(&res.poly)).abs() < 1e-5,
        "seed {seed} : aire {} contre {}",
        contour.area_mm2,
        shoelace(&res.poly)
    );
    assert!(
        (contour.area_mm2 - shoelace(original) - res.gain_area).abs() < 1e-5,
        "seed {seed} : gain d'aire"
    );
    c.areas_checked += 1;
    // 3. Périmètre : + somme des ouvertures (le bord ouvert gagne la somme des ouvertures).
    assert!(
        (contour.perimeter_mm - perim(original) - res.gain_perimeter).abs() < 1e-5,
        "seed {seed} : périmètre"
    );
    assert!(
        (perim(&res.poly) - perim(original) - res.gain_perimeter).abs() < 1e-5,
        "seed {seed} : périmètre de l'oracle"
    );
    c.perimeters_checked += 1;
    // 4. Longueur du bord fixe conservée : somme des arêtes marquées de l'oracle ET lecture des
    //    sommets observés qui sont, chacun, un sommet de la chaîne marquée.
    let n = res.poly.len();
    let marked: f64 = (0..n)
        .filter(|&i| res.fixed[i])
        .map(|i| dist(res.poly[i], res.poly[(i + 1) % n]))
        .sum();
    assert!(
        (marked - fixed_len).abs() < 1e-6,
        "seed {seed} : bord fixe {marked} contre {fixed_len}"
    );
    let marked_vertices: Vec<PointMm> = (0..n)
        .filter(|&i| res.fixed[i])
        .flat_map(|i| [res.poly[i], res.poly[(i + 1) % n]])
        .collect();
    for q in &marked_vertices {
        assert!(
            got.iter().any(|g| dist(*g, *q) < 1e-6),
            "seed {seed} : sommet du bord fixe absent"
        );
    }
    c.fixed_edges_checked += 1;
    // 5. Lignes stockées : pivot, lèvres, longueur, ouverture.
    let stored = proj.piece_flares(piece);
    assert_eq!(stored.len(), res.flares.len(), "seed {seed}");
    for (sf, of) in stored.iter().zip(&res.flares) {
        assert!(dist(sf.hinge(), of.h) < 1e-6, "seed {seed} : pivot");
        assert!(dist(sf.stay_end(), of.t) < 1e-6, "seed {seed} : lèvre fixe");
        assert!(
            dist(sf.moved_end(), of.t2) < 1e-6,
            "seed {seed} : lèvre tournée"
        );
        c.flares_checked += 1;
        assert!((sf.length_mm() - of.length).abs() < 1e-6, "seed {seed}");
        assert!(
            (dist(sf.hinge(), sf.moved_end()) - of.length).abs() < 1e-6,
            "seed {seed} : longueur de ligne conservée"
        );
        assert!((sf.opening_mm() - of.opening).abs() < 1e-6, "seed {seed}");
        c.lengths_checked += 1;
    }
}

fn invalid_command(
    r: &mut Rng,
    piece: PieceId,
    a: PointMm,
    b: PointMm,
    poly: &[PointMm],
    len: f64,
) -> Command {
    let ok = vec![(0.4 * len, 10.0)];
    match r.below(12) {
        0 => command(piece, a, b, &[]),
        1 => {
            let many: Vec<(f64, f64)> = (1..=51).map(|k| (len * k as f64 / 53.0, 0.5)).collect();
            command(piece, a, b, &many)
        }
        2 => command(piece, a, b, &[(0.4 * len, 0.0)]),
        3 => command(piece, a, b, &[(0.4 * len, -r.range(0.1, 50.0))]),
        4 => command(piece, a, b, &[(0.4 * len, f64::NAN)]),
        5 => command(piece, a, b, &[(0.4 * len, 2001.0 + r.range(0.0, 1e4))]),
        6 => command(piece, a, b, &[(if r.chance(50) { 0.0 } else { len }, 10.0)]),
        7 => command(piece, a, b, &[(-r.range(0.1, 50.0), 10.0)]),
        8 => command(piece, a, b, &[(f64::INFINITY, 10.0)]),
        9 => command(piece, a, b, &[(0.4 * len, 10.0), (0.4 * len, 10.0)]),
        10 => command(PieceId(999_999), a, b, &ok),
        _ => {
            // Bord qui n'est pas un côté : milieu d'un côté vers le sommet suivant, ou diagonale.
            let m = pt((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
            let far = poly
                .iter()
                .copied()
                .find(|v| dist(*v, a) > 1e-6 && dist(*v, b) > 1e-6)
                .unwrap_or(pt(a.x + 1.0, a.y + 7.0));
            command(piece, m, far, &ok)
        }
    }
}

fn run_sequence(seed: u64, c: &mut Counters) {
    let mut r = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let mut original = random_polygon(&mut r);
    let (proj0, piece) = build(&original);
    if shoelace(&original) < 0.0 {
        original.reverse();
    }
    let mut doc = Document::new(proj0);
    c.sequences += 1;
    let n = original.len();
    let ops = 3 + r.below(4);
    let mut accepted_state = false;
    let mut history: Vec<String> = Vec::new();
    for _ in 0..ops {
        c.attempts += 1;
        let fi = r.below(n as u64) as usize;
        let (e0, e1) = (original[fi], original[(fi + 1) % n]);
        let rev = r.chance(35);
        let (a, b) = if rev { (e1, e0) } else { (e0, e1) };
        let len = dist(a, b);
        if accepted_state {
            // Une seconde ampleur est refusée (une seule opération par pièce).
            let before = bytes(doc.project());
            assert!(
                doc.execute(command(piece, a, b, &[(0.5 * len, 5.0)]))
                    .is_err(),
                "seed {seed} : seconde ampleur acceptée"
            );
            assert_eq!(
                before,
                bytes(doc.project()),
                "seed {seed} : refus sans trace"
            );
            c.refused_second += 1;
            c.refusals_without_trace += 1;
            break;
        }
        if r.chance(22) {
            let cmd = invalid_command(&mut r, piece, a, b, &original, len);
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
        let count = 1 + r.below(5) as usize;
        let mut ats: Vec<f64> = (0..count)
            .map(|_| r.range(0.06 * len, 0.94 * len))
            .collect();
        ats.sort_by(f64::total_cmp);
        let big = r.chance(15);
        let lines: Vec<(f64, f64)> = ats
            .iter()
            .map(|&at| {
                (
                    at,
                    if big {
                        r.range(1.0, 400.0)
                    } else {
                        r.range(0.5, 70.0)
                    },
                )
            })
            .collect();
        let verdict = o_run(&original, fi, rev, &lines);
        let before = bytes(doc.project());
        let cmd = command(piece, a, b, &lines);
        // L'aperçu est d'accord avec la commande (hors cas ambigus).
        let preview = doc.project().flare_preview(
            piece,
            a,
            b,
            &lines
                .iter()
                .map(|&(at, o)| FlareLineSpec {
                    at_mm: at,
                    opening_mm: o,
                })
                .collect::<Vec<_>>(),
        );
        match verdict {
            Verdict::Ambiguous => {
                c.ambiguous_skipped += 1;
            }
            Verdict::Refuse => {
                assert!(
                    doc.execute(cmd).is_err(),
                    "seed {seed} : l'ampleur aurait dû être refusée"
                );
                assert!(preview.is_err(), "seed {seed} : aperçu accepté à tort");
                assert_eq!(
                    before,
                    bytes(doc.project()),
                    "seed {seed} : refus sans trace"
                );
                c.refused_geometry += 1;
                c.refusals_without_trace += 1;
                c.previews_checked += 1;
            }
            Verdict::Ok(res) => {
                let pre = preview.unwrap_or_else(|e| panic!("seed {seed} : aperçu refusé : {e}"));
                assert!((pre.fixed_after_mm - len).abs() < 1e-6, "seed {seed}");
                assert!(
                    (pre.open_gain_mm - lines.iter().map(|l| l.1).sum::<f64>()).abs() < 1e-9,
                    "seed {seed}"
                );
                c.previews_checked += 1;
                let out = doc
                    .execute(cmd)
                    .unwrap_or_else(|e| panic!("seed {seed} : ampleur refusée à tort : {e}"));
                assert_eq!(out.flares.len(), lines.len());
                c.accepted += 1;
                if lines.len() > 1 {
                    c.multi_line_accepted += 1;
                }
                if rev {
                    c.reversed_accepted += 1;
                }
                check_state(seed, c, doc.project(), piece, &res, &original, len);
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
                history.push(before);
                accepted_state = true;
            }
        }
    }
    while let Some(before) = history.pop() {
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
fn campagne_adverse_ampleur_200_sequences() {
    let mut c = Counters::default();
    for seed in 1..=200u64 {
        run_sequence(seed, &mut c);
    }
    eprintln!("{c:#?}");
    assert!(c.sequences >= 150, "{c:?}");
    assert!(c.accepted >= 100, "{c:?}");
    assert!(c.multi_line_accepted >= 50, "{c:?}");
    assert!(c.reversed_accepted >= 25, "{c:?}");
    assert!(c.refused_geometry >= 25, "{c:?}");
    assert!(c.refused_invalid >= 60, "{c:?}");
    assert!(c.refused_second >= 50, "{c:?}");
    assert_eq!(c.vertex_sets_checked, c.accepted, "{c:?}");
    assert_eq!(c.areas_checked, c.accepted, "{c:?}");
    assert_eq!(c.perimeters_checked, c.accepted, "{c:?}");
    assert_eq!(c.fixed_edges_checked, c.accepted, "{c:?}");
    assert!(c.flares_checked >= c.accepted, "{c:?}");
    assert_eq!(c.lengths_checked, c.flares_checked, "{c:?}");
    assert_eq!(c.previews_checked, c.accepted + c.refused_geometry, "{c:?}");
    assert!(c.undo_redo_checks >= 40, "{c:?}");
    assert_eq!(c.json_roundtrips, c.accepted, "{c:?}");
    assert_eq!(
        c.refusals_without_trace,
        c.refused_geometry + c.refused_invalid + c.refused_second,
        "{c:?}"
    );
    assert!(c.ambiguous_skipped * 10 <= c.attempts, "{c:?}");
}
