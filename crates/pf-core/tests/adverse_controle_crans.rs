//! US-08.4 : campagne adverse du contrôle des crans, 160 séquences à graine fixe. Chaque séquence
//! crée deux pièces (rectangles entiers ou polygones étoilés, en segments, parfois parcourues dans
//! le sens horaire), déclare un couple (suites de tracés, parfois le contour entier, parfois la même
//! pièce des deux côtés), pose des crans aléatoires et des crans placés **près de la position
//! attendue** (écarts de 0 à 15 mm, quasi doublons), puis lit le contrôle avec une tolérance tirée.
//!
//! **Oracle indépendant** : il tient ses propres sommets, longueurs et sens de rotation, recalcule
//! seul les abscisses, les doublons, la position attendue du vis-à-vis (embu proportionnel, sens de
//! couture) et l'appariement un-pour-un (sélection répétée du plus petit écart, autre algorithme que
//! le tri de l'implémentation). Il compare : crans lus, abscisse, attendue, statut, vis-à-vis,
//! écart, listes de manquants ; lecture sans modification (octets identiques), aller-retour JSON ;
//! et la propriété de cohérence : la marche des manquants ne laisse aucun manquant. Les cas
//! d'égalité numérique (écarts à moins de 1e-7 l'un de l'autre ou de la tolérance) sont écartés et
//! comptés. Des compteurs prouvent que les contrôles forts s'exécutent. Mutations vérifiées : voir
//! `docs/JOURNAL.md` (tranche 73).

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
    items: usize,
    matched: usize,
    shifted: usize,
    missing: usize,
    pairings_compared: usize,
    ambiguous: usize,
    dups_dropped: usize,
    same_piece: usize,
    whole_side: usize,
    same_sense_sewing: usize,
    clockwise_piece: usize,
    tol_variants: usize,
    readonly_checks: usize,
    json_checks: usize,
    property_checks: usize,
    property_skipped: usize,
    marched: usize,
    both_directions: usize,
}

struct OPiece {
    piece: PieceId,
    verts: Vec<(f64, f64)>,
    ents: Vec<EntityId>,
    cw: bool,
}

impl OPiece {
    fn n(&self) -> usize {
        self.verts.len()
    }
    fn len(&self, i: usize) -> f64 {
        let (a, b) = (self.verts[i], self.verts[(i + 1) % self.n()]);
        ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt()
    }
}

#[derive(Clone, Copy)]
struct OSide {
    piece: usize,
    start: usize,
    count: usize,
}

#[derive(Clone, Copy)]
struct ONotch {
    id: NotchId,
    piece: usize,
    edge: usize,
    /// Distance depuis le sommet de départ de l'arête, dans le sens du contour.
    d: f64,
}

fn build_piece(rng: &mut Rng, p: &mut Project, s: SheetId, index: usize, want_cw: bool) -> OPiece {
    let ox = 1500.0 * index as f64;
    let mut verts: Vec<(f64, f64)> = if rng.chance(40) {
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
    let area2: f64 = (0..verts.len())
        .map(|i| {
            let (a, b) = (verts[i], verts[(i + 1) % verts.len()]);
            a.0 * b.1 - b.0 * a.1
        })
        .sum();
    if area2 < 0.0 {
        verts.reverse();
    }
    let n = verts.len();
    let mut ents = Vec::new();
    for k in 0..n {
        let (a, b) = (verts[k], verts[(k + 1) % n]);
        ents.push(
            p.apply(Command::AddSegment {
                sheet: s,
                from: PointRef::New(pt(a.0, a.1)),
                to: PointRef::New(pt(b.0, b.1)),
            })
            .unwrap()
            .entities[0],
        );
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
        .unwrap()
        .pieces[0];
    if !want_cw {
        return OPiece {
            piece,
            verts,
            ents,
            cw: false,
        };
    }
    // Contour parcouru dans l'autre sens : sommets inversés, l'arête k va de v'k à v'k+1, soit
    // l'arête (n − 2 − k) d'origine, parcourue à l'envers.
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
    let n = verts.len();
    OPiece {
        piece,
        verts: verts.iter().rev().copied().collect(),
        ents: (0..n).map(|k| ents[(2 * n - 2 - k) % n]).collect(),
        cw: true,
    }
}

fn edges_of(ps: &[OPiece], s: &OSide) -> Vec<usize> {
    (0..s.count)
        .map(|t| (s.start + t) % ps[s.piece].n())
        .collect()
}

fn side_len(ps: &[OPiece], s: &OSide) -> f64 {
    edges_of(ps, s).iter().map(|i| ps[s.piece].len(*i)).sum()
}

fn is_whole(ps: &[OPiece], s: &OSide) -> bool {
    s.count == ps[s.piece].n()
}

fn abscissa(ps: &[OPiece], s: &OSide, n: &ONotch) -> Option<f64> {
    if n.piece != s.piece {
        return None;
    }
    let mut at = 0.0;
    for i in edges_of(ps, s) {
        if i == n.edge {
            return Some(at + n.d);
        }
        at += ps[s.piece].len(i);
    }
    None
}

/// (rang d'arête dans la suite, distance) d'une abscisse.
fn locate(ps: &[OPiece], s: &OSide, pos: f64) -> (usize, f64) {
    let es = edges_of(ps, s);
    let mut at = 0.0;
    for (r, i) in es.iter().enumerate() {
        let l = ps[s.piece].len(*i);
        if pos <= at + l + 1e-12 || r == es.len() - 1 {
            return (*i, (pos - at).clamp(0.0, l));
        }
        at += l;
    }
    unreachable!()
}

fn place(p: &mut Project, s: SheetId, ps: &[OPiece], piece: usize, edge: usize, d: f64) -> ONotch {
    let op = &ps[piece];
    let ent = op.ents[edge];
    let sh = p.sheet(s).unwrap();
    let (u, _) = sh.segment_ends(ent).unwrap();
    let total = sh.path_length_mm(ent).unwrap();
    let v = op.verts[edge];
    let from_start = (u.x - v.0).abs() < 1e-9 && (u.y - v.1).abs() < 1e-9;
    let id = p
        .apply(Command::AddNotch {
            sheet: s,
            entity: ent,
            distance_mm: if from_start {
                d.min(total)
            } else {
                (total - d).max(0.0)
            },
            side: Side::Left,
            length_mm: 5.0,
            style: NotchStyle::Single,
        })
        .unwrap()
        .notches[0];
    ONotch { id, piece, edge, d }
}

fn circ(d: f64, total: f64, whole: bool) -> f64 {
    let d = d.abs();
    if whole {
        d.min((total - d).max(0.0))
    } else {
        d
    }
}

/// Crans d'un côté (abscisse), doublons retirés ; `dropped` compte les retirés.
fn kept(
    ps: &[OPiece],
    s: &OSide,
    other_total: f64,
    notches: &[ONotch],
    dropped: &mut usize,
) -> Vec<(ONotch, f64)> {
    let mut all: Vec<(ONotch, f64)> = notches
        .iter()
        .filter_map(|n| abscissa(ps, s, n).map(|a| (*n, a)))
        .collect();
    all.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.id.0.cmp(&b.0.id.0)));
    let own = side_len(ps, s);
    let whole = is_whole(ps, s);
    let tol = 0.01 / (other_total / own).min(1.0);
    let mut out: Vec<(ONotch, f64)> = Vec::new();
    for c in all {
        if out.iter().any(|k| circ(k.1 - c.1, own, whole) <= tol) {
            *dropped += 1;
        } else {
            out.push(c);
        }
    }
    out
}

/// Position attendue sur la cible (embu proportionnel, puis sens de couture).
fn expected(s: f64, ls: f64, lt: f64, opposite: bool) -> f64 {
    let sewn = s * lt / ls;
    (if opposite { lt - sewn } else { sewn }).clamp(0.0, lt)
}

struct OItem {
    id: NotchId,
    is_a: bool,
    pos: f64,
    expected: f64,
    partner: Option<NotchId>,
    gap: Option<f64>,
    matched: Option<bool>,
}

struct Verdict {
    items: Vec<OItem>,
    ambiguous: bool,
    dropped: usize,
    opposite: bool,
}

#[allow(clippy::needless_range_loop)]
fn oracle(ps: &[OPiece], a: &OSide, b: &OSide, notches: &[ONotch], tol: f64) -> Verdict {
    let (la, lb) = (side_len(ps, a), side_len(ps, b));
    let (wa, wb) = (is_whole(ps, a), is_whole(ps, b));
    let opposite = ps[a.piece].cw == ps[b.piece].cw;
    let mut dropped = 0;
    let ka = kept(ps, a, lb, notches, &mut dropped);
    let kb = kept(ps, b, la, notches, &mut dropped);
    let ea: Vec<f64> = ka.iter().map(|k| expected(k.1, la, lb, opposite)).collect();
    let eb: Vec<f64> = kb.iter().map(|k| expected(k.1, lb, la, opposite)).collect();
    let range = 10.0f64.max(tol);
    let cost = |i: usize, j: usize| -> f64 {
        circ(ea[i] - kb[j].1, lb, wb).max(circ(eb[j] - ka[i].1, la, wa))
    };
    let mut pa: Vec<Option<(usize, f64)>> = vec![None; ka.len()];
    let mut pb: Vec<Option<(usize, f64)>> = vec![None; kb.len()];
    let mut ambiguous = false;
    loop {
        let mut best: Option<(f64, usize, usize)> = None;
        for i in 0..ka.len() {
            for j in 0..kb.len() {
                if pa[i].is_some() || pb[j].is_some() {
                    continue;
                }
                let c = cost(i, j);
                if c > range {
                    continue;
                }
                let better = match best {
                    None => true,
                    Some((bc, bi, bj)) => {
                        (c.total_cmp(&bc).then(i.cmp(&bi)).then(j.cmp(&bj)))
                            == std::cmp::Ordering::Less
                    }
                };
                if better {
                    best = Some((c, i, j));
                }
            }
        }
        let Some((c, bi, bj)) = best else { break };
        // Ambiguïté : un autre couple concurrent (même cran) à moins de 1e-7 du meilleur ;
        // ou un écart proche de la tolérance / de la plage.
        for i in 0..ka.len() {
            for j in 0..kb.len() {
                if (i == bi) != (j == bj) && pa[i].is_none() && pb[j].is_none() {
                    let o = cost(i, j);
                    if o <= range && (o - c).abs() < 1e-7 {
                        ambiguous = true;
                    }
                }
            }
        }
        if (c - tol).abs() < 1e-7 || (c - range).abs() < 1e-7 {
            ambiguous = true;
        }
        pa[bi] = Some((bj, c));
        pb[bj] = Some((bi, c));
    }
    let mut items = Vec::new();
    for (i, k) in ka.iter().enumerate() {
        items.push(OItem {
            id: k.0.id,
            is_a: true,
            pos: k.1,
            expected: ea[i],
            partner: pa[i].map(|(j, _)| kb[j].0.id),
            gap: pa[i].map(|(j, _)| circ(ea[i] - kb[j].1, lb, wb)),
            matched: pa[i].map(|(_, c)| c <= tol),
        });
    }
    for (j, k) in kb.iter().enumerate() {
        items.push(OItem {
            id: k.0.id,
            is_a: false,
            pos: k.1,
            expected: eb[j],
            partner: pb[j].map(|(i, _)| ka[i].0.id),
            gap: pb[j].map(|(i, _)| circ(eb[j] - ka[i].1, la, wa)),
            matched: pb[j].map(|(_, c)| c <= tol),
        });
    }
    Verdict {
        items,
        ambiguous,
        dropped,
        opposite,
    }
}

fn random_side(rng: &mut Rng, ps: &[OPiece], piece: usize, max_count: usize) -> OSide {
    let n = ps[piece].n();
    let count = if rng.chance(15) {
        n.min(max_count)
    } else {
        1 + rng.below(max_count.min(n) as u64) as usize
    };
    OSide {
        piece,
        start: rng.below(n as u64) as usize,
        count,
    }
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

fn run_sequence(seq: usize, c: &mut Counters) {
    let mut rng =
        Rng(0xC2B2_AE3D_27D4_EB4F ^ ((seq as u64 + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15)));
    for _ in 0..4 {
        rng.step();
    }
    let mut proj = Project::new("adverse crans");
    let sheet = proj.sheets[0].id;
    let same_piece_case = rng.chance(20);
    let cw1 = !same_piece_case && rng.chance(35);
    let ps = vec![
        build_piece(&mut rng, &mut proj, sheet, 0, false),
        build_piece(&mut rng, &mut proj, sheet, 1, cw1),
    ];
    for op in &ps {
        let real = proj.piece_contour(op.piece).unwrap().clockwise;
        assert_eq!(real, op.cw, "sens de rotation du contour (séquence {seq})");
    }
    if cw1 {
        c.clockwise_piece += 1;
    }
    let (a, b) = if same_piece_case && ps[0].n() >= 4 {
        c.same_piece += 1;
        let n = ps[0].n();
        let ca = 1 + rng.below((n / 2) as u64) as usize;
        (
            OSide {
                piece: 0,
                start: 0,
                count: ca,
            },
            OSide {
                piece: 0,
                start: ca,
                count: 1 + rng.below((n - ca) as u64) as usize,
            },
        )
    } else {
        (
            random_side(&mut rng, &ps, 0, 8),
            random_side(&mut rng, &ps, 1, 8),
        )
    };
    let to_side = |s: &OSide| {
        proj.seam_side_range(ps[s.piece].piece, ps[s.piece].ents[s.start], s.count)
            .unwrap()
    };
    let (sa, sb) = (to_side(&a), to_side(&b));
    proj.apply(Command::AddSeamPair {
        a: sa,
        b: sb,
        embu_mm: None,
        tolerance_mm: None,
    })
    .unwrap();
    let pair = proj.seams[0].id;
    if is_whole(&ps, &a) || is_whole(&ps, &b) {
        c.whole_side += 1;
    }

    // Crans aléatoires, puis crans proches des positions attendues, puis quasi doublons.
    let mut notches: Vec<ONotch> = Vec::new();
    for side in [&a, &b] {
        for _ in 0..rng.below(5) {
            let es = edges_of(&ps, side);
            let e = es[rng.below(es.len() as u64) as usize];
            let l = ps[side.piece].len(e);
            let d = if rng.chance(15) {
                if rng.chance(50) {
                    0.0
                } else {
                    l
                }
            } else {
                rng.range(0.0, l)
            };
            notches.push(place(&mut proj, sheet, &ps, side.piece, e, d));
        }
    }
    let (la, lb) = (side_len(&ps, &a), side_len(&ps, &b));
    let opposite = ps[a.piece].cw == ps[b.piece].cw;
    if !opposite {
        c.same_sense_sewing += 1;
    }
    let offsets = [
        0.0, 0.0, 0.0, 0.3, -0.3, 0.95, -0.95, 1.05, -1.05, 4.0, -4.0, 9.5, 15.0, -15.0,
    ];
    for _ in 0..rng.below(6) {
        let from_a = rng.chance(60);
        let (src, dst, ls, lt) = if from_a {
            (a, b, la, lb)
        } else {
            (b, a, lb, la)
        };
        let mine: Vec<(ONotch, f64)> = notches
            .iter()
            .filter_map(|n| abscissa(&ps, &src, n).map(|x| (*n, x)))
            .collect();
        if mine.is_empty() {
            continue;
        }
        let (_, s) = mine[rng.below(mine.len() as u64) as usize];
        let off = offsets[rng.below(offsets.len() as u64) as usize];
        let pos = (expected(s, ls, lt, opposite) + off).clamp(0.0, lt);
        let (e, d) = locate(&ps, &dst, pos);
        notches.push(place(&mut proj, sheet, &ps, dst.piece, e, d));
    }
    for _ in 0..rng.below(3) {
        if notches.is_empty() {
            break;
        }
        let base = notches[rng.below(notches.len() as u64) as usize];
        let side = if abscissa(&ps, &a, &base).is_some() {
            &a
        } else {
            &b
        };
        let l = ps[base.piece].len(base.edge);
        let off = [0.003, -0.003, 0.02, -0.02][rng.below(4) as usize];
        let d = (base.d + off).clamp(0.0, l);
        if abscissa(&ps, side, &base).is_some() {
            notches.push(place(&mut proj, sheet, &ps, base.piece, base.edge, d));
        }
    }

    // Lecture avec une tolérance tirée ; la lecture ne modifie rien.
    let tol = [1.0, 1.0, 0.5, 2.5, 0.0][rng.below(5) as usize];
    if tol != 1.0 {
        c.tol_variants += 1;
    }
    let before = bytes(&proj);
    let rd = proj.notch_check_report_with(tol).remove(0);
    assert_eq!(
        before,
        bytes(&proj),
        "lecture sans modification (séquence {seq})"
    );
    c.readonly_checks += 1;
    let v = oracle(&ps, &a, &b, &notches, tol);
    assert_eq!(rd.id, pair);
    assert!(rd.not_computable.is_none(), "séquence {seq}");
    assert_eq!(
        rd.items.len(),
        v.items.len(),
        "nombre de crans lus (séquence {seq})"
    );
    c.dups_dropped += v.dropped;
    if v.ambiguous {
        c.ambiguous += 1;
    }
    for o in &v.items {
        let side = if o.is_a { SeamWhich::A } else { SeamWhich::B };
        let i = rd
            .items
            .iter()
            .find(|i| i.notch == o.id)
            .unwrap_or_else(|| panic!("cran {:?} absent du rapport (séquence {seq})", o.id));
        assert_eq!(i.side, side, "séquence {seq}");
        assert!(near(i.position_mm, o.pos), "abscisse (séquence {seq})");
        assert!(near(i.expected_mm, o.expected), "attendue (séquence {seq})");
        c.items += 1;
        if v.ambiguous {
            continue;
        }
        assert_eq!(i.partner, o.partner, "vis-à-vis (séquence {seq})");
        let want = match o.matched {
            None => NotchMatch::Missing,
            Some(true) => NotchMatch::Matched,
            Some(false) => NotchMatch::Shifted,
        };
        assert_eq!(i.status, want, "statut (séquence {seq}) {i:?}");
        match (i.gap_mm, o.gap) {
            (Some(x), Some(y)) => assert!(near(x, y), "écart {x} ≠ {y} (séquence {seq})"),
            (None, None) => {}
            other => panic!("écart incohérent {other:?} (séquence {seq})"),
        }
        c.pairings_compared += 1;
        match want {
            NotchMatch::Matched => c.matched += 1,
            NotchMatch::Shifted => c.shifted += 1,
            NotchMatch::Missing => c.missing += 1,
        }
    }
    let _ = v.opposite;

    // Aller-retour JSON : même rapport.
    let back = persist::from_json(&before).unwrap();
    assert_eq!(back.notch_check_report_with(tol).remove(0), rd);
    c.json_checks += 1;

    // Propriété de cohérence avec la marche : après le report des manquants, plus de manquant.
    // Écartée si deux crans d'un côté sont à moins de 0,05 mm (doublons) : le report les fusionne.
    let crowded = |s: &OSide, other: f64| {
        let own = side_len(&ps, s);
        let w = is_whole(&ps, s);
        let mut d = 0;
        let ks = kept(&ps, s, other, &notches, &mut d);
        ks.iter().enumerate().any(|(i, x)| {
            ks.iter()
                .skip(i + 1)
                .any(|y| circ(x.1 - y.1, own, w) * other.min(own) / own < 0.05)
        })
    };
    if v.ambiguous || crowded(&a, lb) || crowded(&b, la) {
        c.property_skipped += 1;
        return;
    }
    let (ma, mb) = (rd.missing(SeamWhich::A), rd.missing(SeamWhich::B));
    let mut cmds = Vec::new();
    for (from, ids) in [(SeamWhich::A, ma), (SeamWhich::B, mb)] {
        if !ids.is_empty() {
            cmds.push(Command::MarchNotches {
                pair,
                from,
                which: Some(ids),
            });
        }
    }
    if cmds.len() == 2 {
        c.both_directions += 1;
    }
    if cmds.is_empty() {
        c.property_skipped += 1;
        return;
    }
    proj.apply(Command::Batch { commands: cmds })
        .unwrap_or_else(|e| panic!("marche des manquants refusée (séquence {seq}) : {e}"));
    c.marched += 1;
    let after = proj.notch_check_report_with(tol).remove(0);
    assert_eq!(
        after.count_status(NotchMatch::Missing),
        0,
        "plus aucun manquant après la marche (séquence {seq}) : {:?}",
        after
            .items
            .iter()
            .filter(|i| i.status == NotchMatch::Missing)
            .collect::<Vec<_>>()
    );
    c.property_checks += 1;
}

#[test]
fn adverse_controle_des_crans_160_sequences() {
    let mut c = Counters::default();
    for seq in 0..160 {
        run_sequence(seq, &mut c);
        c.sequences += 1;
    }
    eprintln!("{c:?}");
    assert_eq!(c.sequences, 160);
    assert!(c.items > 400, "{c:?}");
    assert!(
        c.matched > 100 && c.shifted > 20 && c.missing > 100,
        "{c:?}"
    );
    assert!(c.pairings_compared > 300, "{c:?}");
    assert!(c.ambiguous < 20, "trop de cas écartés : {c:?}");
    assert!(c.dups_dropped > 10, "{c:?}");
    assert!(c.same_piece > 10 && c.whole_side > 10, "{c:?}");
    assert!(c.same_sense_sewing > 10 && c.clockwise_piece > 10, "{c:?}");
    assert!(c.tol_variants > 30, "{c:?}");
    assert_eq!(c.readonly_checks, 160);
    assert_eq!(c.json_checks, 160);
    assert!(
        c.property_checks > 40 && c.marched == c.property_checks,
        "{c:?}"
    );
    assert!(c.both_directions > 5, "{c:?}");
}
