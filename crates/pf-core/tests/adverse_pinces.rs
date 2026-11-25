//! US-07.1 — campagne adverse des pinces : 180 séquences à graine fixe sur des pièces
//! rectangulaires (position, taille et sens du contour aléatoires), mêlant poses valides et
//! invalides, déplacements et suppressions de points, modifications, retraits, annulations et
//! rétablissements.
//!
//! **Oracle indépendant** : il ne lit rien du code testé. Il refait le triangle de la pince
//! (axe, milieu de base, pieds), décide seul si la pose doit être acceptée (bornes, sommet
//! strictement dans le rectangle, pieds sur un même côté), puis recalcule la fermeture avec des
//! formules d'angle (`2·atan2(w/2, L)`, signe selon la jambe fixe), sans le produit vecteur /
//! scalaire du code : longueurs avant / après, extrémités de la ligne pliée, points `P1` / `P2`
//! du capuchon, et propriétés du capuchon (points sur les prolongements des jambes, égale
//! distance du sommet, `K` sur la droite lisse). Des **compteurs** prouvent que les contrôles
//! forts s'exécutent. Mutations vérifiées à la main : voir `docs/JOURNAL.md` (tranche 61).

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
    add_attempts: usize,
    add_accepted: usize,
    add_refused: usize,
    refusals_without_trace: usize,
    ambiguous_skipped: usize,
    undo_redo_checks: usize,
    moves: usize,
    deletes: usize,
    sets: usize,
    removes: usize,
    calc_checked: usize,
    calc_expected_err: usize,
    closing_checked: usize,
    chain_ends_checked: usize,
    capuchon_checked: usize,
    capuchon_none: usize,
    legs_equal_checked: usize,
    issues_checked: usize,
    json_roundtrips: usize,
}

/// Rectangle de la pièce : coin bas-gauche, largeur, hauteur.
#[derive(Clone, Copy)]
struct Rect {
    x0: f64,
    y0: f64,
    w: f64,
    h: f64,
}

impl Rect {
    fn x1(&self) -> f64 {
        self.x0 + self.w
    }
    fn y1(&self) -> f64 {
        self.y0 + self.h
    }
    /// Côtés dans le sens anti-horaire : (début, fin).
    fn sides(&self) -> [(PointMm, PointMm); 4] {
        let (a, b, c, d) = (
            pt(self.x0, self.y0),
            pt(self.x1(), self.y0),
            pt(self.x1(), self.y1()),
            pt(self.x0, self.y1()),
        );
        [(a, b), (b, c), (c, d), (d, a)]
    }
    /// Distance à chaque côté.
    fn side_distances(&self, p: PointMm) -> [f64; 4] {
        let mut out = [0.0; 4];
        for (i, (a, b)) in self.sides().iter().enumerate() {
            let (dx, dy) = (b.x - a.x, b.y - a.y);
            let len2 = dx * dx + dy * dy;
            let t = (((p.x - a.x) * dx + (p.y - a.y) * dy) / len2).clamp(0.0, 1.0);
            out[i] = ((a.x + t * dx - p.x).powi(2) + (a.y + t * dy - p.y).powi(2)).sqrt();
        }
        out
    }
    fn strictly_inside(&self, p: PointMm, margin: f64) -> Option<bool> {
        let m = [
            p.x - self.x0,
            self.x1() - p.x,
            p.y - self.y0,
            self.y1() - p.y,
        ];
        let least = m.iter().cloned().fold(f64::INFINITY, f64::min);
        if (least - margin).abs() < 1e-6 {
            return None; // frontière de tolérance : ambigu
        }
        Some(least > margin)
    }
}

/// Décision de l'oracle pour une pince : `Some(true)` calculable, `Some(false)` impossible,
/// `None` ambigu (frontière de tolérance ou coin).
struct Verdict {
    ok: bool,
    side: usize,
    apex: PointMm,
    left: PointMm,
    right: PointMm,
    length: f64,
    opening: f64,
}

fn oracle(
    r: &Rect,
    a: PointMm,
    t: PointMm,
    length: f64,
    opening: f64,
    ambiguous: &mut bool,
) -> Verdict {
    let none = |ok| Verdict {
        ok,
        side: 0,
        apex: a,
        left: a,
        right: a,
        length,
        opening,
    };
    let finite = [a.x, a.y, t.x, t.y, length, opening]
        .iter()
        .all(|v| v.is_finite());
    if !finite
        || !(1.0..=2000.0).contains(&length)
        || !(0.1..=1000.0).contains(&opening)
        || opening > length
    {
        return none(false);
    }
    let (dx, dy) = (t.x - a.x, t.y - a.y);
    let n = dx.hypot(dy);
    if n <= 1e-9 {
        return none(false);
    }
    let (ux, uy) = (dx / n, dy / n);
    let m = (a.x + ux * length, a.y + uy * length);
    let half = opening / 2.0;
    // Normale gauche (−uy ; ux).
    let left = pt(m.0 - uy * half, m.1 + ux * half);
    let right = pt(m.0 + uy * half, m.1 - ux * half);
    match r.strictly_inside(a, 1e-3) {
        None => {
            *ambiguous = true;
            return none(false);
        }
        Some(false) => return none(false),
        Some(true) => {}
    }
    let (dl, dr) = (r.side_distances(left), r.side_distances(right));
    let nearest = |d: [f64; 4]| {
        let mut idx: Vec<usize> = (0..4).collect();
        idx.sort_by(|&i, &j| d[i].total_cmp(&d[j]));
        (idx[0], d[idx[0]], d[idx[1]])
    };
    let (sl, dl0, dl1) = nearest(dl);
    let (sr, dr0, dr1) = nearest(dr);
    for (d0, d1) in [(dl0, dl1), (dr0, dr1)] {
        // Pied proche de la tolérance, ou presque dans un coin : on ne tranche pas.
        if (d0 - 1e-3).abs() < 1e-6 || (d0 <= 1e-3 && d1 <= 1e-3 + 1e-6) {
            *ambiguous = true;
        }
    }
    if dl0 > 1e-3 || dr0 > 1e-3 || sl != sr {
        return Verdict {
            ok: false,
            ..none(false)
        };
    }
    Verdict {
        ok: true,
        side: sl,
        apex: a,
        left,
        right,
        length,
        opening,
    }
}

fn dist(a: PointMm, b: PointMm) -> f64 {
    ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt()
}

/// Rotation de `p` autour de `c` d'un angle en radians (formules trigonométriques directes).
fn rotate(p: PointMm, c: PointMm, angle: f64) -> PointMm {
    let (s, k) = angle.sin_cos();
    let (dx, dy) = (p.x - c.x, p.y - c.y);
    pt(c.x + k * dx - s * dy, c.y + s * dx + k * dy)
}

fn cross_len(a: PointMm, b: PointMm, p: PointMm) -> f64 {
    // distance de p à la droite (a, b)
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    ((p.x - a.x) * dy - (p.y - a.y) * dx).abs() / dx.hypot(dy)
}

/// Contrôles forts d'une pince calculable.
fn check_calc(r: &Rect, v: &Verdict, fixed: Side, calc: &DartCalc, c: &mut Counters) {
    let tol = 1e-6;
    // Jambes égales, et égales à √(L² + (w/2)²).
    let leg = (v.length.powi(2) + (v.opening / 2.0).powi(2)).sqrt();
    let (ll, lr) = (
        dist(v.apex, calc.shape.left_foot),
        dist(v.apex, calc.shape.right_foot),
    );
    assert!((ll - lr).abs() < tol, "jambes {ll} / {lr}");
    assert!(
        (ll - leg).abs() < 1e-7 && (lr - leg).abs() < 1e-7,
        "{ll} {lr} {leg}"
    );
    assert!(
        dist(calc.shape.left_foot, v.left) < 1e-7 && dist(calc.shape.right_foot, v.right) < 1e-7
    );
    c.legs_equal_checked += 1;

    // Ligne portée : le côté du rectangle.
    let (start, end) = r.sides()[v.side];
    let (dx, dy) = (end.x - start.x, end.y - start.y);
    let side_len = dx.hypot(dy);
    let (ux, uy) = (dx / side_len, dy / side_len);
    let s_of = |p: PointMm| (p.x - start.x) * ux + (p.y - start.y) * uy;
    let (sl, sr) = (s_of(v.left), s_of(v.right));
    let (s_first, s_second) = (sl.min(sr), sl.max(sr));
    let left_is_first = sl < sr;
    let cl = &calc.closed;
    assert!(
        (cl.before_mm - side_len).abs() < tol,
        "avant {} / {side_len}",
        cl.before_mm
    );
    assert!((cl.removed_mm - (s_second - s_first)).abs() < tol);
    assert!(
        (cl.removed_mm - v.opening).abs() < tol,
        "base droite = ouverture"
    );
    assert!((cl.after_mm - (side_len - v.opening)).abs() < tol);
    assert!(((cl.before_mm - cl.after_mm) - v.opening).abs() < tol);
    c.closing_checked += 1;

    // Ligne pliée : angle par formules directes, signe selon la jambe fixe.
    let theta = 2.0 * (v.opening / 2.0).atan2(v.length);
    let angle = match fixed {
        Side::Left => theta,
        Side::Right => -theta,
    };
    assert!((cl.rotation_deg.to_radians() - angle).abs() < 1e-9);
    let moving_is_first = match fixed {
        Side::Left => !left_is_first, // la jambe gauche reste : le pied droit bouge
        Side::Right => left_is_first,
    };
    let (e1_len, e2_len) = (s_first, side_len - s_second);
    let fixed_foot = if fixed == Side::Left { v.left } else { v.right };
    if moving_is_first {
        // Le côté avant tourne ; la ligne finit à l'extrémité de la ligne d'origine.
        let first = cl.chain.first().map(|e| e.start());
        let last = cl.chain.last().map(|e| e.end());
        if e1_len > 1e-9 {
            assert!(dist(first.unwrap(), rotate(start, v.apex, angle)) < 1e-6);
        }
        assert!(dist(last.unwrap(), end) < 1e-6);
    } else {
        let first = cl.chain.first().map(|e| e.start());
        let last = cl.chain.last().map(|e| e.end());
        assert!(dist(first.unwrap(), start) < 1e-6);
        if e2_len > 1e-9 {
            assert!(dist(last.unwrap(), rotate(end, v.apex, angle)) < 1e-6);
        }
    }
    // Le pied qui bouge tombe sur le pied fixe : le joint de la chaîne est le pied fixe.
    if e1_len > 1e-9 && e2_len > 1e-9 {
        let ends: Vec<PointMm> = cl.chain.iter().map(|e| e.end()).collect();
        assert!(
            ends.iter().any(|&p| dist(p, fixed_foot) < 1e-6),
            "le pied mobile doit tomber sur le pied fixe"
        );
    }
    c.chain_ends_checked += 1;

    // Capuchon.
    let full = e1_len > 1e-9 && e2_len > 1e-9;
    match (cl.capuchon, full) {
        (_, false) => assert!(cl.capuchon.is_none(), "pas de capuchon sans deux côtés"),
        (None, true) => c.capuchon_none += 1,
        (Some(cap), true) => {
            let span = 2.0 * v.opening;
            let (d1, d2) = (e1_len.min(span), e2_len.min(span));
            let p1 = pt(start.x + ux * (s_first - d1), start.y + uy * (s_first - d1));
            let p2 = pt(
                start.x + ux * (s_second + d2),
                start.y + uy * (s_second + d2),
            );
            assert!(
                dist(cap[0], p1) < 1e-6 && dist(cap[3], p2) < 1e-6,
                "P1 / P2 du capuchon"
            );
            // Ka sur la jambe du pied « gauche », Kb sur celle du pied « droit » (ordre le long de la ligne).
            let (first_foot, second_foot) = if left_is_first {
                (v.left, v.right)
            } else {
                (v.right, v.left)
            };
            assert!(
                cross_len(v.apex, first_foot, cap[1]) < 1e-6,
                "Ka hors de sa jambe"
            );
            assert!(
                cross_len(v.apex, second_foot, cap[2]) < 1e-6,
                "Kb hors de sa jambe"
            );
            let (da, db) = (dist(v.apex, cap[1]), dist(v.apex, cap[2]));
            assert!((da - db).abs() < 1e-6, "égale distance du sommet");
            assert!((cl.capuchon_leg_mm.unwrap() - da).abs() < 1e-6);
            // K fixe est sur la droite lisse entre les deux points voisins du pied (côté fixe
            // inchangé, côté mobile tourné).
            let (q1, q2) = if moving_is_first {
                (rotate(p1, v.apex, angle), p2)
            } else {
                (p1, rotate(p2, v.apex, angle))
            };
            let k_fixed = if moving_is_first { cap[2] } else { cap[1] };
            assert!(
                cross_len(q1, q2, k_fixed) < 1e-6,
                "K hors de la droite lisse"
            );
            let on_leg_side = (k_fixed.x - v.apex.x) * (fixed_foot.x - v.apex.x)
                + (k_fixed.y - v.apex.y) * (fixed_foot.y - v.apex.y);
            assert!(
                on_leg_side > 0.0,
                "K du côté du pied, pas au-delà du sommet"
            );
            c.capuchon_checked += 1;
        }
    }
}

struct Seq {
    p: Project,
    doc: Document,
    sheet: SheetId,
    piece: PieceId,
    rect: Rect,
}

fn new_seq(rng: &mut Rng) -> Seq {
    let mut p = Project::new("Campagne pinces");
    let sheet = p.sheets[0].id;
    let rect = Rect {
        x0: (rng.range(-300.0, 300.0) * 4.0).round() / 4.0,
        y0: (rng.range(-300.0, 300.0) * 4.0).round() / 4.0,
        w: (rng.range(120.0, 420.0) * 4.0).round() / 4.0,
        h: (rng.range(90.0, 320.0) * 4.0).round() / 4.0,
    };
    p.apply(Command::AddRectangle {
        sheet,
        corner: PointRef::New(pt(rect.x0, rect.y0)),
        width_mm: rect.w,
        height_mm: rect.h,
    })
    .unwrap();
    let piece = p
        .apply(Command::ExtractPiece {
            sheet,
            source: PieceSource::Click {
                at: pt(rect.x0 + rect.w / 2.0, rect.y0 + rect.h / 2.0),
            },
            name: "Pièce".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    Seq {
        doc: Document::new(p.clone()),
        p,
        sheet,
        piece,
        rect,
    }
}

/// Paramètres d'une pose : sommet, point d'axe, longueur, ouverture, jambe fixe.
fn random_dart(rng: &mut Rng, r: &Rect) -> (PointMm, PointMm, f64, f64, Side) {
    let fixed = if rng.chance(50) {
        Side::Left
    } else {
        Side::Right
    };
    // Côté visé (0 bas, 1 droite, 2 haut, 3 gauche) ; axe perpendiculaire au côté.
    let side = rng.below(4) as usize;
    let (start, end) = r.sides()[side];
    let (dx, dy) = (end.x - start.x, end.y - start.y);
    let len = dx.hypot(dy);
    let (ux, uy) = (dx / len, dy / len);
    // Normale intérieure (gauche de la direction, le contour étant anti-horaire).
    let (nx, ny) = (-uy, ux);
    let depth_max = if side % 2 == 0 { r.h } else { r.w };
    let s = rng.range(len * 0.1, len * 0.9);
    let base = pt(start.x + ux * s, start.y + uy * s);
    let depth = rng.range(depth_max * 0.2, depth_max * 0.85);
    let apex = pt(base.x + nx * depth, base.y + ny * depth);
    let opening = rng.range(4.0, depth.min(60.0));
    let mode = rng.below(100);
    if mode < 55 {
        // Pose propre : longueur = distance au bord.
        (apex, base, depth, opening, fixed)
    } else if mode < 70 {
        // Longueur erronée : les pieds sortent du contour.
        (apex, base, depth + rng.range(0.5, 20.0), opening, fixed)
    } else if mode < 80 {
        // Axe oblique : pieds hors du contour.
        let off = pt(
            base.x + ux * rng.range(2.0, 30.0),
            base.y + uy * rng.range(2.0, 30.0),
        );
        (apex, off, depth, opening, fixed)
    } else if mode < 88 {
        // Valeurs hors bornes ou non finies.
        let weird = [0.0, -3.0, 0.05, 1e9, f64::NAN, f64::INFINITY];
        let w = weird[rng.below(6) as usize];
        if rng.chance(50) {
            (apex, base, depth, w, fixed)
        } else {
            (apex, base, w, opening, fixed)
        }
    } else if mode < 94 {
        // Sommet hors de la pièce.
        (
            pt(
                apex.x + nx * (depth_max + 50.0),
                apex.y + ny * (depth_max + 50.0),
            ),
            base,
            depth,
            opening,
            fixed,
        )
    } else {
        // Sommet confondu avec le point d'axe, ou sur le contour.
        if rng.chance(50) {
            (apex, apex, depth, opening, fixed)
        } else {
            (
                base,
                pt(base.x + nx, base.y + ny),
                depth_max,
                opening,
                fixed,
            )
        }
    }
}

fn positions(s: &Seq, d: &PieceDart) -> Option<(PointMm, PointMm)> {
    let sh = s.p.sheet(s.sheet).ok()?;
    Some((sh.position(d.apex).ok()?, sh.position(d.toward).ok()?))
}

fn verify_all(s: &Seq, c: &mut Counters) {
    let darts = &s.p.piece(s.piece).unwrap().darts;
    let resolved = s.p.piece_darts(s.piece);
    assert_eq!(darts.len(), resolved.len());
    let mut expected_bad = 0usize;
    let mut checkable = true;
    for (d, r) in darts.iter().zip(&resolved) {
        let (a, t) = positions(s, d).expect("positions lisibles");
        let mut amb = false;
        let v = oracle(&s.rect, a, t, d.length_mm, d.opening_mm, &mut amb);
        if amb {
            c.ambiguous_skipped += 1;
            checkable = false;
            continue;
        }
        c.calc_checked += 1;
        match (&r.calc, v.ok) {
            (Ok(calc), true) => check_calc(&s.rect, &v, d.fixed, calc, c),
            (Err(_), false) => {
                c.calc_expected_err += 1;
                expected_bad += 1;
            }
            (Ok(_), false) => panic!("pince calculée alors que l'oracle la refuse : {d:?}"),
            (Err(e), true) => panic!("pince refusée ({e}) alors que l'oracle l'accepte : {d:?}"),
        }
    }
    if checkable {
        let issues = s.p.piece_issues(s.piece);
        let got = issues
            .iter()
            .find_map(|i| match i {
                piece::PieceIssue::DartInvalid { count } => Some(*count),
                _ => None,
            })
            .unwrap_or(0);
        assert_eq!(got, expected_bad, "PieceIssue::DartInvalid");
        c.issues_checked += 1;
    }
}

fn run(rng: &mut Rng, c: &mut Counters) {
    let mut s = new_seq(rng);
    c.sequences += 1;
    for _ in 0..14 {
        let op = rng.below(12);
        let darts: Vec<PieceDart> = s.p.piece(s.piece).unwrap().darts.clone();
        match op {
            0..=5 => {
                let (a, t, l, w, fixed) = random_dart(rng, &s.rect);
                let cmd = Command::AddPieceDart {
                    piece: s.piece,
                    apex: PointRef::New(a),
                    toward: PointRef::New(t),
                    length_mm: l,
                    opening_mm: w,
                    fixed,
                };
                c.add_attempts += 1;
                let mut amb = false;
                let v = oracle(&s.rect, a, t, l, w, &mut amb);
                let (before, text) = (s.doc.project().clone(), bytes(s.doc.project()));
                match s.doc.execute(cmd) {
                    Ok(out) => {
                        assert!(amb || v.ok, "pose acceptée alors que l'oracle la refuse");
                        c.add_accepted += 1;
                        let now = s.doc.project().piece(s.piece).unwrap();
                        assert_eq!(now.darts.len(), darts.len() + 1);
                        assert_eq!(out.darts.len(), 1);
                        let added = now.darts.last().unwrap();
                        assert_eq!(added.id, out.darts[0]);
                        assert!(
                            added.length_mm == l && added.opening_mm == w && added.fixed == fixed
                        );
                        // Une seule annulation défait tout (dart et points créés).
                        let after = bytes(s.doc.project());
                        assert!(s.doc.undo());
                        assert_eq!(bytes(s.doc.project()), text, "annulation en une étape");
                        assert!(s.doc.redo());
                        assert_eq!(bytes(s.doc.project()), after, "rétablissement");
                        c.undo_redo_checks += 1;
                    }
                    Err(_) => {
                        assert!(
                            amb || !v.ok,
                            "pose refusée alors que l'oracle l'accepte : {a:?} {t:?} {l} {w}"
                        );
                        c.add_refused += 1;
                        assert_eq!(*s.doc.project(), before, "refus sans trace (valeur)");
                        assert_eq!(bytes(s.doc.project()), text, "refus sans trace (octets)");
                        c.refusals_without_trace += 1;
                    }
                }
                s.p = s.doc.project().clone();
            }
            6 | 7 if !darts.is_empty() => {
                // Déplace le sommet ou le point d'axe d'une pince, vers un point quelconque.
                let d = darts[rng.below(darts.len() as u64) as usize];
                let point = if op == 6 { d.apex } else { d.toward };
                let to = if rng.chance(70) {
                    pt(
                        s.rect.x0 + rng.range(-0.2, 1.2) * s.rect.w,
                        s.rect.y0 + rng.range(-0.2, 1.2) * s.rect.h,
                    )
                } else if let Some((a, t)) = positions(&s, &d) {
                    // Retour sur le point d'axe du même côté : confond sommet et axe.
                    if op == 6 {
                        t
                    } else {
                        a
                    }
                } else {
                    pt(0.0, 0.0)
                };
                let (before, text) = (s.doc.project().clone(), bytes(s.doc.project()));
                match s.doc.execute(Command::MovePoint {
                    sheet: s.sheet,
                    point,
                    to,
                }) {
                    Ok(_) => c.moves += 1,
                    Err(_) => {
                        assert_eq!(*s.doc.project(), before);
                        assert_eq!(bytes(s.doc.project()), text);
                        c.refusals_without_trace += 1;
                    }
                }
                s.p = s.doc.project().clone();
            }
            8 if !darts.is_empty() => {
                // Modification : mêmes contrôles que la pose.
                let d = darts[rng.below(darts.len() as u64) as usize];
                let (a, t) = positions(&s, &d).unwrap();
                let l = if rng.chance(50) {
                    d.length_mm
                } else {
                    rng.range(1.0, 120.0)
                };
                let w = if rng.chance(50) {
                    d.opening_mm
                } else {
                    rng.range(0.1, 60.0)
                };
                let fixed = if rng.chance(50) {
                    d.fixed
                } else {
                    d.fixed.flipped()
                };
                let mut amb = false;
                let v = oracle(&s.rect, a, t, l, w, &mut amb);
                let (before, text) = (s.doc.project().clone(), bytes(s.doc.project()));
                let steps = s.doc.can_undo();
                let _ = steps;
                match s.doc.execute(Command::SetPieceDart {
                    piece: s.piece,
                    dart: d.id,
                    apex: d.apex,
                    toward: d.toward,
                    length_mm: l,
                    opening_mm: w,
                    fixed,
                }) {
                    Ok(_) => {
                        assert!(
                            amb || v.ok,
                            "modification acceptée alors que l'oracle la refuse"
                        );
                        c.sets += 1;
                        let now = s.doc.project().piece(s.piece).unwrap();
                        let same = now.darts.iter().find(|x| x.id == d.id).unwrap();
                        assert_eq!((same.length_mm, same.opening_mm, same.fixed), (l, w, fixed));
                        assert_eq!(now.darts.len(), darts.len());
                    }
                    Err(_) => {
                        assert!(
                            amb || !v.ok,
                            "modification refusée alors que l'oracle l'accepte"
                        );
                        assert_eq!(*s.doc.project(), before);
                        assert_eq!(bytes(s.doc.project()), text);
                        c.refusals_without_trace += 1;
                    }
                }
                s.p = s.doc.project().clone();
            }
            9 if !darts.is_empty() => {
                let d = darts[rng.below(darts.len() as u64) as usize];
                s.doc
                    .execute(Command::RemovePieceDart {
                        piece: s.piece,
                        dart: d.id,
                    })
                    .unwrap();
                assert!(s
                    .doc
                    .project()
                    .piece(s.piece)
                    .unwrap()
                    .darts
                    .iter()
                    .all(|x| x.id != d.id));
                c.removes += 1;
                s.p = s.doc.project().clone();
            }
            10 if !darts.is_empty() => {
                // Supprime un point d'ancrage : les pinces qui en dépendent disparaissent, avec
                // remarque ; les autres restent.
                let d = darts[rng.below(darts.len() as u64) as usize];
                let victim = if rng.chance(50) { d.apex } else { d.toward };
                let out = s.doc.execute(Command::Delete {
                    sheet: s.sheet,
                    points: vec![victim],
                    entities: vec![],
                });
                let out = out.unwrap();
                let now = s.doc.project().piece(s.piece).unwrap();
                let expected: Vec<DartId> = darts
                    .iter()
                    .filter(|x| x.apex != victim && x.toward != victim)
                    .map(|x| x.id)
                    .collect();
                let got: Vec<DartId> = now.darts.iter().map(|x| x.id).collect();
                assert_eq!(got, expected);
                assert_eq!(
                    out.notes.iter().filter(|n| n.contains("pince")).count(),
                    darts.len() - expected.len(),
                    "une remarque par pince retirée"
                );
                c.deletes += 1;
                s.p = s.doc.project().clone();
            }
            _ => {
                // Annuler puis rétablir une étape quelconque : identique à l'octet.
                if s.doc.can_undo() {
                    let now = bytes(s.doc.project());
                    s.doc.undo();
                    if s.doc.can_redo() {
                        s.doc.redo();
                    }
                    assert_eq!(bytes(s.doc.project()), now);
                    c.undo_redo_checks += 1;
                    s.p = s.doc.project().clone();
                }
            }
        }
        // Après chaque pas : projet valide, positions relues, contrôles forts.
        s.p.validate().expect("projet valide après chaque pas");
        verify_all(&s, c);
    }
    // Aller-retour du fichier.
    let text = bytes(&s.p);
    let back = persist::from_json(&text).expect("relecture");
    assert_eq!(bytes(&back), text, "octets stables");
    assert_eq!(back, s.p);
    c.json_roundtrips += 1;
}

#[test]
fn campagne_adverse_des_pinces_graine_fixe() {
    let mut rng = Rng(0x7A11_5EED_0DA2_7001);
    let mut c = Counters::default();
    for _ in 0..180 {
        run(&mut rng, &mut c);
    }
    println!("{c:?}");
    // Les contrôles forts se sont réellement exécutés.
    assert_eq!(c.sequences, 180);
    assert!(c.add_attempts >= 600, "{c:?}");
    assert!(c.add_accepted >= 150, "{c:?}");
    assert!(c.add_refused >= 150, "{c:?}");
    assert!(c.refusals_without_trace >= c.add_refused, "{c:?}");
    assert!(c.undo_redo_checks >= 150, "{c:?}");
    assert!(c.moves >= 100, "{c:?}");
    assert!(c.sets >= 20, "{c:?}");
    assert!(c.removes >= 20, "{c:?}");
    assert!(c.deletes >= 20, "{c:?}");
    assert!(c.calc_checked >= 1500, "{c:?}");
    assert!(c.calc_expected_err >= 100, "{c:?}");
    assert!(c.legs_equal_checked >= 300, "{c:?}");
    assert!(c.closing_checked == c.legs_equal_checked, "{c:?}");
    assert!(c.chain_ends_checked == c.legs_equal_checked, "{c:?}");
    assert!(c.capuchon_checked >= 150, "{c:?}");
    assert!(c.issues_checked >= 1000, "{c:?}");
    assert_eq!(c.json_roundtrips, 180);
}
