//! US-08.3 : campagne adverse du contrôle des raccords, 160 séquences à graine fixe. Chaque séquence
//! crée deux pièces (rectangles ou polygones étoilés convexes / concaves, parfois avec des arêtes en
//! Bézier, parfois parcourues dans le sens horaire), déclare un ou deux couples (suites de tracés,
//! parfois le contour entier, parfois la même pièce des deux côtés), puis enchaîne : lectures avec
//! des tolérances variées, déplacement d'un sommet, inversion du sens d'une pièce (le couple est
//! retiré par la règle d'US-08.1 quand un côté a plus d'un tracé, puis redéclaré), suppression d'un
//! couple, suppression d'un tracé de couture.
//!
//! **Oracle indépendant** : il tient ses propres sommets et poignées de Bézier, relit seulement
//! l'ordre et le sens des tracés du contour (la structure, pas les angles), et recalcule seul : le
//! sens de rotation (formule des lacets sur une polyligne), les tangentes (vecteur d'arête ; pour une
//! Bézier, différence finie de pas 1e-7 sur sa propre évaluation de Bernstein, jamais la dérivée du
//! code testé), l'angle intérieur par `atan2` du virage (180° − virage en sens anti-horaire,
//! 180° + virage en sens horaire : autre formule que l'implémentation), l'appariement des
//! extrémités (sens opposés ⇔ même sens de rotation), la somme, l'écart et le statut. Tolérances
//! annoncées : 1e-7° pour les arêtes droites, 1e-3° avec une Bézier (différence finie) ; les cas
//! d'égalité numérique (écart à moins de 1e-5° de la tolérance) et les pointes (angle 0° / 360°)
//! sont écartés et comptés. Vue assemblée : coïncidence du début à 1e-6 mm, cordes alignées à
//! 1e-9 rad, écart de fin = différence des cordes de l'oracle, périmètres conservés. Lecture seule
//! vérifiée à l'octet, déterminisme, aller-retour JSON, aucune référence morte. Des compteurs
//! prouvent que les contrôles forts s'exécutent. Mutations vérifiées : voir `docs/JOURNAL.md`
//! (tranche 76).

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

type V = (f64, f64);

fn pt(v: V) -> PointMm {
    PointMm::new(v.0, v.1)
}

fn bytes(p: &Project) -> String {
    persist::to_json(p).unwrap()
}

#[derive(Default, Debug)]
struct Counters {
    sequences: usize,
    rejected: usize,
    rows: usize,
    angles: usize,
    curved_rows: usize,
    smooth: usize,
    broken: usize,
    cusp_skipped: usize,
    ambiguous: usize,
    opposite: usize,
    same_sense: usize,
    clockwise_pieces: usize,
    same_piece: usize,
    whole_side: usize,
    two_pairs: usize,
    tol_variants: usize,
    moves: usize,
    move_refused: usize,
    inversions: usize,
    pruned_on_inversion: usize,
    kept_on_inversion: usize,
    readded: usize,
    pairs_removed: usize,
    entities_deleted: usize,
    views: usize,
    views_refused: usize,
    readonly_checks: usize,
    json_checks: usize,
    dead_ref_checks: usize,
}

// ---------------------------------------------------------------- oracle : géométrie propre

#[derive(Clone, Copy)]
enum G {
    Line(V, V),
    Cubic([V; 4]),
}

fn rev(g: G) -> G {
    match g {
        G::Line(a, b) => G::Line(b, a),
        G::Cubic(c) => G::Cubic([c[3], c[2], c[1], c[0]]),
    }
}

fn bez(c: &[V; 4], t: f64) -> V {
    let u = 1.0 - t;
    let w = [u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t];
    (
        (0..4).map(|i| w[i] * c[i].0).sum(),
        (0..4).map(|i| w[i] * c[i].1).sum(),
    )
}

fn sub(a: V, b: V) -> V {
    (a.0 - b.0, a.1 - b.1)
}

fn tan_start(g: G) -> V {
    match g {
        G::Line(a, b) => sub(b, a),
        G::Cubic(c) => sub(bez(&c, 1e-7), c[0]),
    }
}

fn tan_end(g: G) -> V {
    match g {
        G::Line(a, b) => sub(b, a),
        G::Cubic(c) => sub(c[3], bez(&c, 1.0 - 1e-7)),
    }
}

fn start_pt(g: G) -> V {
    match g {
        G::Line(a, _) => a,
        G::Cubic(c) => c[0],
    }
}

fn end_pt(g: G) -> V {
    match g {
        G::Line(_, b) => b,
        G::Cubic(c) => c[3],
    }
}

fn pts_of(g: G) -> Vec<V> {
    match g {
        G::Line(a, b) => vec![a, b],
        G::Cubic(c) => (0..=48).map(|i| bez(&c, i as f64 / 48.0)).collect(),
    }
}

fn dist(a: V, b: V) -> f64 {
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
}

fn perimeter(dir: &[G]) -> f64 {
    dir.iter()
        .map(|g| pts_of(*g).windows(2).map(|w| dist(w[0], w[1])).sum::<f64>())
        .sum()
}

fn clockwise(dir: &[G]) -> bool {
    let pts: Vec<V> = dir.iter().flat_map(|g| pts_of(*g)).collect();
    let n = pts.len();
    let a2: f64 = (0..n)
        .map(|i| {
            let (a, b) = (pts[i], pts[(i + 1) % n]);
            a.0 * b.1 - b.0 * a.1
        })
        .sum();
    a2 < 0.0
}

fn area2(dir: &[G]) -> f64 {
    let pts: Vec<V> = dir.iter().flat_map(|g| pts_of(*g)).collect();
    let n = pts.len();
    (0..n)
        .map(|i| {
            let (a, b) = (pts[i], pts[(i + 1) % n]);
            a.0 * b.1 - b.0 * a.1
        })
        .sum::<f64>()
        .abs()
}

/// Angle intérieur (degrés) au sommet où arrive `tin` et d'où part `tout` ; `None` pour une pointe.
fn interior(tin: V, tout: V, cw: bool) -> Option<f64> {
    let turn = (tin.0 * tout.1 - tin.1 * tout.0)
        .atan2(tin.0 * tout.0 + tin.1 * tout.1)
        .to_degrees();
    let a = if cw { 180.0 + turn } else { 180.0 - turn };
    (a > 1e-6 && a < 360.0 - 1e-6).then_some(a)
}

// ---------------------------------------------------------------- pièces

struct OPiece {
    piece: PieceId,
    verts: Vec<V>,
    /// Poignées de la Bézier de l'arête k (de verts[k] à verts[k+1]), ou arête droite.
    ctrl: Vec<Option<(V, V)>>,
    ents: Vec<EntityId>,
}

impl OPiece {
    fn geom(&self, k: usize) -> G {
        let (a, b) = (self.verts[k], self.verts[(k + 1) % self.verts.len()]);
        match self.ctrl[k] {
            None => G::Line(a, b),
            Some((c1, c2)) => G::Cubic([a, c1, c2, b]),
        }
    }
    fn curved(&self) -> bool {
        self.ctrl.iter().any(Option::is_some)
    }
    /// Arêtes dans l'ordre et le sens du contour actuel de la pièce (structure relue du projet).
    fn directed(&self, p: &Project) -> Vec<G> {
        p.piece(self.piece)
            .unwrap()
            .outline
            .iter()
            .map(|e| {
                let k = self.ents.iter().position(|x| *x == e.entity).unwrap();
                if e.reversed {
                    rev(self.geom(k))
                } else {
                    self.geom(k)
                }
            })
            .collect()
    }
}

fn build_piece(
    rng: &mut Rng,
    p: &mut Project,
    s: SheetId,
    index: usize,
    curved: bool,
) -> Option<OPiece> {
    let ox = 1500.0 * index as f64;
    let mut verts: Vec<V> = if !curved && rng.chance(30) {
        let w = rng.range(30.0, 300.0).floor();
        let h = rng.range(30.0, 300.0).floor();
        vec![(ox, 0.0), (ox + w, 0.0), (ox + w, h), (ox, h)]
    } else {
        let n = 3 + rng.below(5) as usize;
        (0..n)
            .map(|i| {
                let a = std::f64::consts::TAU * (i as f64 + rng.range(0.1, 0.9)) / n as f64;
                let (rx, ry) = (rng.range(60.0, 200.0), rng.range(60.0, 200.0));
                (ox + rx * a.cos(), ry * a.sin())
            })
            .collect()
    };
    let n = verts.len();
    let a2: f64 = (0..n)
        .map(|i| {
            let (a, b) = (verts[i], verts[(i + 1) % n]);
            a.0 * b.1 - b.0 * a.1
        })
        .sum();
    if a2 < 0.0 {
        verts.reverse();
    }
    let ctrl: Vec<Option<(V, V)>> = (0..n)
        .map(|k| {
            if curved && rng.chance(45) {
                let (a, b) = (verts[k], verts[(k + 1) % n]);
                let d = sub(b, a);
                let perp = (-d.1, d.0);
                let (f1, f2) = (rng.range(-0.3, 0.3), rng.range(-0.3, 0.3));
                Some((
                    (a.0 + d.0 / 3.0 + perp.0 * f1, a.1 + d.1 / 3.0 + perp.1 * f1),
                    (
                        a.0 + 2.0 * d.0 / 3.0 + perp.0 * f2,
                        a.1 + 2.0 * d.1 / 3.0 + perp.1 * f2,
                    ),
                ))
            } else {
                None
            }
        })
        .collect();
    let mut ents = Vec::new();
    for k in 0..n {
        let (a, b) = (verts[k], verts[(k + 1) % n]);
        let cmd = match ctrl[k] {
            None => Command::AddSegment {
                sheet: s,
                from: PointRef::New(pt(a)),
                to: PointRef::New(pt(b)),
            },
            Some((c1, c2)) => Command::AddCurve {
                sheet: s,
                start: PointRef::New(pt(a)),
                c1: pt(c1),
                c2: pt(c2),
                end: PointRef::New(pt(b)),
            },
        };
        ents.push(p.apply(cmd).ok()?.entities[0]);
    }
    let piece = p
        .apply(Command::ExtractPiece {
            sheet: s,
            source: PieceSource::Paths {
                entities: ents.clone(),
            },
            name: format!("Pièce {index}"),
            detached: false,
        })
        .ok()?
        .pieces[0];
    let op = OPiece {
        piece,
        verts,
        ctrl,
        ents,
    };
    (area2(&op.directed(p)) > 2000.0).then_some(op)
}

fn reverse_outline(p: &mut Project, piece: PieceId) {
    let outline: Vec<PieceEdge> = p
        .piece(piece)
        .unwrap()
        .outline
        .iter()
        .rev()
        .map(|e| PieceEdge {
            entity: e.entity,
            reversed: !e.reversed,
        })
        .collect();
    p.apply(Command::SetPieceOutline {
        piece,
        outline,
        grainline: None,
    })
    .unwrap();
}

fn point_at(p: &Project, s: SheetId, v: V) -> PointId {
    let sheet = p.sheet(s).unwrap();
    sheet
        .points
        .iter()
        .map(|q| q.id)
        .find(|id| {
            let q = sheet.position(*id).unwrap();
            (q.x - v.0).abs() < 1e-9 && (q.y - v.1).abs() < 1e-9
        })
        .expect("point introuvable")
}

// ---------------------------------------------------------------- oracle : lecture des raccords

struct End {
    point: V,
    angle: Option<f64>,
}

fn end_info(dir: &[G], cw: bool, s: usize, c: usize, at_end: bool) -> End {
    let n = dir.len();
    if at_end {
        let last = dir[(s + c - 1) % n];
        End {
            point: end_pt(last),
            angle: interior(tan_end(last), tan_start(dir[(s + c) % n]), cw),
        }
    } else {
        End {
            point: start_pt(dir[s]),
            angle: interior(tan_end(dir[(s + n - 1) % n]), tan_start(dir[s]), cw),
        }
    }
}

struct OSide {
    dir: Vec<G>,
    cw: bool,
    start: usize,
    count: usize,
    curved: bool,
}

fn oside(p: &Project, ps: &[OPiece], side: &SeamSide) -> OSide {
    let op = ps.iter().find(|o| o.piece == side.piece).unwrap();
    let outline = &p.piece(side.piece).unwrap().outline;
    let start = outline
        .iter()
        .position(|e| e.entity == side.edges[0])
        .expect("tracé de couture dans le contour");
    let dir = op.directed(p);
    let cw = clockwise(&dir);
    // Une Bézier dans la suite ou dans les deux arêtes voisines : tolérance de différence finie.
    let n = dir.len();
    let touched = [(start + n - 1) % n, (start + side.edges.len()) % n]
        .into_iter()
        .chain((0..side.edges.len()).map(|k| (start + k) % n))
        .any(|i| matches!(dir[i], G::Cubic(_)));
    OSide {
        dir,
        cw,
        start,
        count: side.edges.len(),
        curved: touched,
    }
}

fn near(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

fn nearv(a: PointMm, b: V, tol: f64) -> bool {
    near(a.x, b.0, tol) && near(a.y, b.1, tol)
}

/// Aucune référence morte : le projet est valide et chaque couple cite des tracés de contour.
fn no_dead_refs(p: &Project, tag: &str) {
    p.validate().unwrap_or_else(|e| panic!("{tag} : {e}"));
    for pair in &p.seams {
        for side in [&pair.a, &pair.b] {
            let piece = p
                .piece(side.piece)
                .unwrap_or_else(|_| panic!("{tag} : pièce morte"));
            for e in &side.edges {
                assert!(
                    piece.outline.iter().any(|o| o.entity == *e),
                    "{tag} : tracé mort"
                );
            }
        }
    }
}

fn check_all(seq: u64, p: &Project, ps: &[OPiece], tol: f64, c: &mut Counters, what: &str) {
    let tag = format!("séquence {seq}, {what}");
    no_dead_refs(p, &tag);
    c.dead_ref_checks += 1;

    let before = bytes(p);
    let rep = p.seam_junction_report_with(tol).unwrap();
    let rep2 = p.seam_junction_report_with(tol).unwrap();
    assert_eq!(bytes(p), before, "{tag} : lecture sans modification");
    assert_eq!(rep, rep2, "{tag} : déterminisme");
    assert_eq!(
        p.seam_junction_report(),
        p.seam_junction_report_with(2.0).unwrap(),
        "{tag} : tolérance par défaut"
    );
    c.readonly_checks += 1;
    assert_eq!(
        rep.len(),
        2 * p.seams.len(),
        "{tag} : deux lignes par couple"
    );

    for (i, pair) in p.seams.iter().enumerate() {
        let (a, b) = (oside(p, ps, &pair.a), oside(p, ps, &pair.b));
        let opposite = a.cw == b.cw;
        if opposite {
            c.opposite += 1;
        } else {
            c.same_sense += 1;
        }
        if pair.a.edges.len() == a.dir.len() || pair.b.edges.len() == b.dir.len() {
            c.whole_side += 1;
        }
        for (k, at_end) in [false, true].into_iter().enumerate() {
            let r = &rep[2 * i + k];
            assert_eq!((r.pair, r.number), (pair.id, i + 1), "{tag}");
            assert_eq!(
                r.end,
                if at_end {
                    JunctionEnd::End
                } else {
                    JunctionEnd::Start
                },
                "{tag}"
            );
            // Extrémité de B cousue : début contre fin quand les sens de rotation sont égaux.
            let partner_end = at_end != opposite;
            assert_eq!(
                r.partner_end,
                if partner_end {
                    JunctionEnd::End
                } else {
                    JunctionEnd::Start
                },
                "{tag} : appariement des extrémités"
            );
            let ea = end_info(&a.dir, a.cw, a.start, a.count, at_end);
            let eb = end_info(&b.dir, b.cw, b.start, b.count, partner_end);
            assert!(
                nearv(r.point_a.unwrap(), ea.point, 1e-9),
                "{tag} : sommet A"
            );
            assert!(
                nearv(r.point_b.unwrap(), eb.point, 1e-9),
                "{tag} : sommet B"
            );
            c.rows += 1;
            let curved = a.curved || b.curved;
            if curved {
                c.curved_rows += 1;
            }
            let (Some(x), Some(y)) = (ea.angle, eb.angle) else {
                c.cusp_skipped += 1;
                continue;
            };
            let atol = if curved { 1e-3 } else { 1e-7 };
            let (ra, rb) = (r.angle_a_deg.unwrap(), r.angle_b_deg.unwrap());
            assert!(near(ra, x, atol), "{tag} : angle A {ra} ≠ {x}");
            assert!(near(rb, y, atol), "{tag} : angle B {rb} ≠ {y}");
            let sum = x + y;
            assert!(near(r.sum_deg.unwrap(), sum, 2.0 * atol), "{tag} : somme");
            assert!(
                near(r.deviation_deg.unwrap(), sum - 180.0, 2.0 * atol),
                "{tag} : écart"
            );
            c.angles += 2;
            let dev = (sum - 180.0).abs();
            if (dev - tol).abs() < 1e-5 {
                c.ambiguous += 1;
                continue;
            }
            let want = if dev <= tol {
                c.smooth += 1;
                JunctionStatus::Smooth
            } else {
                c.broken += 1;
                JunctionStatus::Broken
            };
            assert_eq!(
                r.status, want,
                "{tag} : statut (écart {dev}, tolérance {tol})"
            );
        }
        // Vue assemblée.
        let (sa, ea1) = (
            end_info(&a.dir, a.cw, a.start, a.count, false),
            end_info(&a.dir, a.cw, a.start, a.count, true),
        );
        let (sb0, sb1) = (
            end_info(&b.dir, b.cw, b.start, b.count, opposite),
            end_info(&b.dir, b.cw, b.start, b.count, !opposite),
        );
        let (chord_a, chord_b) = (dist(sa.point, ea1.point), dist(sb0.point, sb1.point));
        let view = p.seam_assembled_view(pair.id);
        assert_eq!(bytes(p), before, "{tag} : la vue ne modifie rien");
        if chord_a <= 1e-6 || chord_b <= 1e-6 {
            if chord_a <= 1e-9 || chord_b <= 1e-9 {
                assert!(view.is_err(), "{tag} : corde nulle refusée");
                c.views_refused += 1;
            }
            continue;
        }
        let v = view.unwrap_or_else(|e| panic!("{tag} : vue refusée : {e}"));
        c.views += 1;
        assert!(v.junctions[0].gap_mm < 1e-6, "{tag} : début coïncident");
        assert!(nearv(v.junctions[0].point_a, sa.point, 1e-9), "{tag}");
        let (j0, j1) = (&v.junctions[0], &v.junctions[1]);
        let ua = (j1.point_a.x - j0.point_a.x, j1.point_a.y - j0.point_a.y);
        let ub = (j1.point_b.x - j0.point_b.x, j1.point_b.y - j0.point_b.y);
        let ang = (ua.0 * ub.1 - ua.1 * ub.0).atan2(ua.0 * ub.0 + ua.1 * ub.1);
        assert!(ang.abs() < 1e-9, "{tag} : cordes alignées ({ang} rad)");
        assert!(
            near(v.end_gap_mm, (chord_a - chord_b).abs(), 1e-6),
            "{tag} : écart de fin {} ≠ {}",
            v.end_gap_mm,
            (chord_a - chord_b).abs()
        );
        let perim = |poly: &[PointMm]| -> f64 {
            (0..poly.len())
                .map(|i| {
                    let (u, w) = (poly[i], poly[(i + 1) % poly.len()]);
                    ((w.x - u.x).powi(2) + (w.y - u.y).powi(2)).sqrt()
                })
                .sum()
        };
        let (pa, pb) = (perimeter(&a.dir), perimeter(&b.dir));
        assert!(
            near(perim(&v.contour_a), pa, 1e-6 * pa.max(1.0)),
            "{tag} : périmètre A"
        );
        assert!(
            near(perim(&v.contour_b), pb, 1e-6 * pb.max(1.0)),
            "{tag} : périmètre B"
        );
        assert_eq!(v.junctions.len(), 2);
    }

    // Aller-retour JSON : mêmes lectures.
    let back = persist::from_json(&before).unwrap();
    assert_eq!(
        back.seam_junction_report_with(tol).unwrap(),
        rep,
        "{tag} : JSON"
    );
    c.json_checks += 1;
}

// ---------------------------------------------------------------- séquence

fn pick_side(rng: &mut Rng, p: &Project, piece: PieceId, whole: bool) -> SeamSide {
    let outline = &p.piece(piece).unwrap().outline;
    let n = outline.len();
    let start = rng.below(n as u64) as usize;
    let count = if whole && rng.chance(10) {
        n
    } else {
        1 + rng.below(n.min(4) as u64) as usize
    };
    p.seam_side_range(piece, outline[start].entity, count)
        .unwrap()
}

fn tolerance(rng: &mut Rng, c: &mut Counters) -> f64 {
    c.tol_variants += 1;
    match rng.below(6) {
        0 => 0.0,
        1 => 2.0,
        2 => 10.0,
        3 => 90.0,
        _ => rng.range(0.0, 90.0),
    }
}

fn run_sequence(seq: u64, c: &mut Counters) -> bool {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15 ^ seq.wrapping_mul(0xD1B5_4A32_D192_ED03) | 1);
    let mut p = Project::new("raccords adverses");
    let s = p.sheets[0].id;
    let curved = rng.chance(35);
    let (Some(a), Some(b)) = (
        build_piece(&mut rng, &mut p, s, 0, curved),
        build_piece(&mut rng, &mut p, s, 1, curved),
    ) else {
        c.rejected += 1;
        return false;
    };
    let mut ps = vec![a, b];
    for op in &ps {
        if rng.chance(35) {
            reverse_outline(&mut p, op.piece);
            c.clockwise_pieces += 1;
        }
    }
    // Couples.
    let same_piece = ps[0].verts.len() >= 4 && rng.chance(15);
    let declare = |rng: &mut Rng, p: &mut Project, ps: &[OPiece]| -> bool {
        let (sa, sb) = if same_piece {
            let outline = &p.piece(ps[0].piece).unwrap().outline;
            let n = outline.len();
            let start = rng.below(n as u64) as usize;
            let ca = 1 + rng.below((n as u64 - 2).min(3)) as usize;
            let room = n - ca;
            let gap = rng.below(room as u64) as usize;
            let cb = 1 + rng.below((room - gap) as u64) as usize;
            let sb_start = (start + ca + gap) % n;
            (
                p.seam_side_range(ps[0].piece, outline[start].entity, ca)
                    .unwrap(),
                p.seam_side_range(ps[0].piece, outline[sb_start].entity, cb)
                    .unwrap(),
            )
        } else {
            (
                pick_side(rng, p, ps[0].piece, true),
                pick_side(rng, p, ps[1].piece, true),
            )
        };
        p.apply(Command::AddSeamPair {
            a: sa,
            b: sb,
            embu_mm: None,
            tolerance_mm: None,
        })
        .is_ok()
    };
    if same_piece {
        c.same_piece += 1;
    }
    if !declare(&mut rng, &mut p, &ps) {
        c.rejected += 1;
        return false;
    }
    if rng.chance(40) && (0..8).any(|_| declare(&mut rng, &mut p, &ps)) {
        c.two_pairs += 1;
    }
    let tol = tolerance(&mut rng, c);
    check_all(seq, &p, &ps, tol, c, "déclaration");
    let tol = tolerance(&mut rng, c);
    check_all(seq, &p, &ps, tol, c, "autre tolérance");

    // Déplacement d'un sommet (pièces sans courbe : l'oracle tient les sommets).
    if !ps.iter().any(OPiece::curved) && rng.chance(60) {
        for _ in 0..1 + rng.below(2) {
            let i = rng.below(2) as usize;
            let j = rng.below(ps[i].verts.len() as u64) as usize;
            let v = ps[i].verts[j];
            let to = (v.0 + rng.range(-25.0, 25.0), v.1 + rng.range(-25.0, 25.0));
            let id = point_at(&p, s, v);
            let before = bytes(&p);
            match p.apply(Command::MovePoint {
                sheet: s,
                point: id,
                to: pt(to),
            }) {
                Ok(_) => {
                    ps[i].verts[j] = to;
                    c.moves += 1;
                    if area2(&ps[i].directed(&p)) < 100.0 {
                        // Pièce quasi plate : orientation sans valeur, on arrête la séquence ici.
                        return true;
                    }
                }
                Err(_) => {
                    assert_eq!(bytes(&p), before, "refus sans trace (séquence {seq})");
                    c.move_refused += 1;
                }
            }
            let tol = tolerance(&mut rng, c);
            check_all(seq, &p, &ps, tol, c, "après déplacement");
        }
    }

    // Inversion du sens d'une pièce.
    if rng.chance(55) {
        let i = rng.below(2) as usize;
        let piece = ps[i].piece;
        let old: Vec<SeamPair> = p.seams.clone();
        reverse_outline(&mut p, piece);
        c.inversions += 1;
        for o in &old {
            let on_piece = |side: &SeamSide| side.piece == piece;
            let multi =
                (on_piece(&o.a) && o.a.edges.len() > 1) || (on_piece(&o.b) && o.b.edges.len() > 1);
            let kept = p.seams.iter().any(|q| q.id == o.id);
            assert_eq!(
                kept,
                !multi,
                "séquence {seq} : un couple survit à l'inversion ssi aucun côté sur la pièce n'a plus d'un tracé"
            );
            if kept {
                c.kept_on_inversion += 1;
            } else {
                c.pruned_on_inversion += 1;
                let flip = |side: &SeamSide| {
                    let mut s2 = side.clone();
                    if on_piece(side) {
                        s2.edges.reverse();
                    }
                    s2
                };
                p.apply(Command::AddSeamPair {
                    a: flip(&o.a),
                    b: flip(&o.b),
                    embu_mm: None,
                    tolerance_mm: None,
                })
                .unwrap_or_else(|e| panic!("séquence {seq} : redéclaration refusée : {e}"));
                c.readded += 1;
            }
        }
        let tol = tolerance(&mut rng, c);
        check_all(seq, &p, &ps, tol, c, "après inversion");
    }

    // Suppression d'un couple, puis d'un tracé de couture.
    if rng.chance(25) && !p.seams.is_empty() {
        let id = p.seams[0].id;
        p.apply(Command::RemoveSeamPair { id }).unwrap();
        c.pairs_removed += 1;
        let tol = tolerance(&mut rng, c);
        check_all(seq, &p, &ps, tol, c, "après suppression du couple");
    }
    if rng.chance(20) && !p.seams.is_empty() {
        let e = p.seams[0].a.edges[0];
        let before = p.seams.len();
        let sheet = p.piece_sheet(p.seams[0].a.piece).unwrap().id;
        if p.apply(Command::Delete {
            sheet,
            points: vec![],
            entities: vec![e],
        })
        .is_ok()
        {
            c.entities_deleted += 1;
            assert!(p.seams.len() < before, "séquence {seq} : couple retiré");
        }
        // Le contour de la pièce a changé : l'oracle ne le relit plus, seuls restent les contrôles
        // de références, de lecture seule et de déterminisme.
        no_dead_refs(&p, &format!("séquence {seq}, après suppression d'un tracé"));
        let before = bytes(&p);
        let rep = p.seam_junction_report();
        assert_eq!(bytes(&p), before);
        assert_eq!(rep.len(), 2 * p.seams.len());
    }
    true
}

#[test]
fn adverse_raccords_160_sequences() {
    let mut c = Counters::default();
    let mut seq = 0;
    while c.sequences < 160 {
        assert!(seq < 800, "trop de séquences refusées : {c:?}");
        if run_sequence(seq, &mut c) {
            c.sequences += 1;
        }
        seq += 1;
    }
    eprintln!("{c:?}");
    assert_eq!(c.sequences, 160);
    assert!(c.rejected < 120, "{c:?}");
    assert!(c.rows > 900 && c.angles > 1800, "{c:?}");
    assert!(c.curved_rows > 200, "{c:?}");
    assert!(c.smooth > 200 && c.broken > 400, "{c:?}");
    assert!(c.ambiguous < 40 && c.cusp_skipped < 40, "{c:?}");
    assert!(c.opposite > 300 && c.same_sense > 150, "{c:?}");
    assert!(c.clockwise_pieces > 40 && c.same_piece > 10, "{c:?}");
    assert!(c.whole_side > 20 && c.two_pairs > 10, "{c:?}");
    assert!(c.tol_variants > 400, "{c:?}");
    assert!(c.moves > 40, "{c:?}");
    assert!(
        c.inversions > 50 && c.pruned_on_inversion > 10 && c.kept_on_inversion > 10,
        "{c:?}"
    );
    assert!(c.readded == c.pruned_on_inversion, "{c:?}");
    assert!(c.pairs_removed > 10 && c.entities_deleted > 5, "{c:?}");
    assert!(c.views > 250 && c.views_refused > 20, "{c:?}");
    assert!(
        c.readonly_checks > 400 && c.json_checks == c.readonly_checks,
        "{c:?}"
    );
    assert!(c.dead_ref_checks == c.readonly_checks, "{c:?}");
}
