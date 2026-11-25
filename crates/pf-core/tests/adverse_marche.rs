//! US-08.2 : campagne adverse de la marche de pièces, 160 séquences à graine fixe. Chaque séquence
//! crée deux ou trois pièces (rectangles à dimensions entières ou polygones étoilés aléatoires,
//! tous en segments), déclare un couple (suites de 1 à n tracés, parfois le contour entier, parfois
//! la même pièce des deux côtés), pose des crans aléatoires sur les deux côtés (à gauche ou à
//! droite du sens du contour, parfois quasi confondus avec la position attendue), puis marche
//! dans un sens ou l'autre, en tous les crans, en liste, ou avec une liste invalide.
//!
//! **Oracle indépendant** : il tient ses propres sommets et longueurs, calcule seul chaque
//! abscisse de couture, la position proportionnelle (embu réparti), le sens de couture (contours
//! anti-horaires : début contre fin), le tracé d'accueil, le point du plan du pied et de la pointe
//! du cran reporté, les doublons (±0,01 mm, tour de fermeture compris) et les refus. Il compare au
//! projet : crans créés (pied et pointe), crans existants intacts, refus sans trace (octets
//! identiques), une seule étape annulable, rétablissement, aller-retour JSON, aperçu = commande.
//! Des compteurs prouvent que les contrôles forts s'exécutent. Mutations vérifiées : voir
//! `docs/JOURNAL.md` (tranche 72).

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
    marches_ok: usize,
    created: usize,
    refused_no_source: usize,
    refused_all_present: usize,
    refused_bad_list: usize,
    refused_unknown_pair: usize,
    duplicates_skipped: usize,
    near_dup_kept: usize,
    list_selection: usize,
    b_to_a: usize,
    same_piece: usize,
    whole_side: usize,
    multi_edge_target: usize,
    landed_on_later_edge: usize,
    undo_checks: usize,
    json_checks: usize,
    preview_checks: usize,
    replay_refused: usize,
    positions_checked: usize,
}

struct OPiece {
    piece: PieceId,
    verts: Vec<(f64, f64)>,
    ents: Vec<EntityId>,
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
    left: bool,
    len: f64,
}

struct Exp {
    source: NotchId,
    foot: (f64, f64),
    tip: (f64, f64),
    dup: bool,
    edge_rank: usize,
}

enum Refusal {
    NoSource,
    BadList,
    AllPresent,
}

fn build_piece(rng: &mut Rng, p: &mut Project, s: SheetId, index: usize) -> OPiece {
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
    // Un triangle dont l'origine est dehors peut être horaire : l'oracle tient un contour
    // anti-horaire (aire signée positive).
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
    OPiece { piece, verts, ents }
}

fn edges_of(ps: &[OPiece], s: &OSide) -> Vec<usize> {
    (0..s.count)
        .map(|t| (s.start + t) % ps[s.piece].n())
        .collect()
}

fn to_side(p: &Project, ps: &[OPiece], s: &OSide) -> SeamSide {
    p.seam_side_range(ps[s.piece].piece, ps[s.piece].ents[s.start], s.count)
        .unwrap()
}

fn side_len(ps: &[OPiece], s: &OSide) -> f64 {
    edges_of(ps, s).iter().map(|i| ps[s.piece].len(*i)).sum()
}

/// Abscisse du cran sur la suite (depuis son début, sens du contour).
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

#[allow(clippy::too_many_arguments)]
fn place(
    p: &mut Project,
    s: SheetId,
    ps: &[OPiece],
    piece: usize,
    edge: usize,
    d: f64,
    left: bool,
    len: f64,
) -> ONotch {
    let op = &ps[piece];
    let ent = op.ents[edge];
    let sh = p.sheet(s).unwrap();
    let (u, _) = sh.segment_ends(ent).unwrap();
    let total = sh.path_length_mm(ent).unwrap();
    let v = op.verts[edge];
    let from_start = (u.x - v.0).abs() < 1e-9 && (u.y - v.1).abs() < 1e-9;
    let geometric_left = if from_start { left } else { !left };
    let id = p
        .apply(Command::AddNotch {
            sheet: s,
            entity: ent,
            distance_mm: if from_start {
                d.min(total)
            } else {
                (total - d).max(0.0)
            },
            side: if geometric_left {
                Side::Left
            } else {
                Side::Right
            },
            length_mm: len,
            style: NotchStyle::Single,
        })
        .unwrap()
        .notches[0];
    ONotch {
        id,
        piece,
        edge,
        d,
        left,
        len,
    }
}

/// Résultat attendu de la marche d'après le seul oracle.
fn oracle(
    ps: &[OPiece],
    src: &OSide,
    dst: &OSide,
    notches: &[ONotch],
    which: &Option<Vec<NotchId>>,
) -> Result<Vec<Exp>, Refusal> {
    let (ls, lt) = (side_len(ps, src), side_len(ps, dst));
    let mut picked: Vec<(f64, ONotch)> = Vec::new();
    match which {
        None => {
            for n in notches {
                if let Some(a) = abscissa(ps, src, n) {
                    picked.push((a, *n));
                }
            }
        }
        Some(ids) => {
            for id in ids {
                let Some(n) = notches.iter().find(|n| n.id == *id) else {
                    return Err(Refusal::BadList);
                };
                let Some(a) = abscissa(ps, src, n) else {
                    return Err(Refusal::BadList);
                };
                if !picked.iter().any(|(_, m)| m.id == n.id) {
                    picked.push((a, *n));
                }
            }
        }
    }
    if picked.is_empty() {
        return Err(Refusal::NoSource);
    }
    picked.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.id.0.cmp(&b.1.id.0)));
    let existing: Vec<f64> = notches
        .iter()
        .filter_map(|n| abscissa(ps, dst, n))
        .collect();
    let whole = dst.count == ps[dst.piece].n();
    let mut taken: Vec<f64> = Vec::new();
    let mut out = Vec::new();
    let dedges = edges_of(ps, dst);
    for (s, n) in picked {
        // Position depuis le début de la cible : abscisse × (cible / source), puis sens opposé
        // (deux contours anti-horaires : début de la source contre fin de la cible).
        let pos = (lt - s * lt / ls).clamp(0.0, lt);
        let mut at = 0.0;
        let mut rank = dedges.len() - 1;
        let mut d = 0.0;
        for (r, i) in dedges.iter().enumerate() {
            let l = ps[dst.piece].len(*i);
            if pos <= at + l + 1e-12 || r == dedges.len() - 1 {
                rank = r;
                d = (pos - at).clamp(0.0, l);
                break;
            }
            at += l;
        }
        let i = dedges[rank];
        let o = &ps[dst.piece];
        let (a, b) = (o.verts[i], o.verts[(i + 1) % o.n()]);
        let l = o.len(i);
        let (ux, uy) = ((b.0 - a.0) / l, (b.1 - a.1) / l);
        let foot = (a.0 + ux * d, a.1 + uy * d);
        let k = if n.left { 1.0 } else { -1.0 };
        let tip = (foot.0 - uy * n.len * k, foot.1 + ux * n.len * k);
        let near = |e: f64| (e - pos).abs() <= 0.01 || (whole && lt - (e - pos).abs() <= 0.01);
        let dup = existing.iter().chain(taken.iter()).any(|e| near(*e));
        if !dup {
            taken.push(pos);
        }
        out.push(Exp {
            source: n.id,
            foot,
            tip,
            dup,
            edge_rank: rank,
        });
    }
    if out.iter().all(|e| e.dup) {
        return Err(Refusal::AllPresent);
    }
    Ok(out)
}

fn foot_tip(p: &Project, s: SheetId, n: NotchId) -> ((f64, f64), (f64, f64)) {
    let sh = p.sheet(s).unwrap();
    let m = sh.notch_marks(sh.notch(n).unwrap()).unwrap()[0];
    ((m.0.x, m.0.y), (m.1.x, m.1.y))
}

fn near2(a: (f64, f64), b: (f64, f64)) -> bool {
    (a.0 - b.0).abs() < 1e-6 && (a.1 - b.1).abs() < 1e-6
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

fn run_sequence(seq: usize, c: &mut Counters) {
    let mut rng =
        Rng(0x9E37_79B9_7F4A_7C15 ^ ((seq as u64 + 1).wrapping_mul(0xD1B5_4A32_D192_ED03)));
    for _ in 0..4 {
        rng.step();
    }
    let mut proj = Project::new("adverse marche");
    let sheet = proj.sheets[0].id;
    let np = 2 + rng.below(2) as usize;
    let ps: Vec<OPiece> = (0..np)
        .map(|i| build_piece(&mut rng, &mut proj, sheet, i))
        .collect();

    // Couple : deux pièces distinctes, ou la même pièce avec deux suites disjointes.
    let (a, b) = if rng.chance(20) && ps[0].n() >= 4 {
        c.same_piece += 1;
        let n = ps[0].n();
        let ca = 1 + rng.below((n / 2) as u64) as usize;
        let sa = OSide {
            piece: 0,
            start: 0,
            count: ca,
        };
        let sb = OSide {
            piece: 0,
            start: ca,
            count: 1 + rng.below((n - ca) as u64) as usize,
        };
        (sa, sb)
    } else {
        (
            random_side(&mut rng, &ps, 0, 8),
            random_side(&mut rng, &ps, 1, 8),
        )
    };
    let (sa, sb) = (to_side(&proj, &ps, &a), to_side(&proj, &ps, &b));
    let pair = proj
        .apply(Command::AddSeamPair {
            a: sa,
            b: sb,
            embu_mm: None,
            tolerance_mm: None,
        })
        .unwrap()
        .seam_pairs[0];
    if a.count == ps[a.piece].n() || b.count == ps[b.piece].n() {
        c.whole_side += 1;
    }

    // Crans : sur chaque côté, plus un cran « étranger » (arête hors de la suite de A).
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
            let left = rng.chance(50);
            let len = rng.range(2.0, 9.0);
            notches.push(place(&mut proj, sheet, &ps, side.piece, e, d, left, len));
        }
    }
    let ea = edges_of(&ps, &a);
    let outside: Option<usize> = (0..ps[a.piece].n())
        .find(|i| !ea.contains(i) && !(a.piece == b.piece && edges_of(&ps, &b).contains(i)));
    let foreign = outside.map(|e| {
        let l = ps[a.piece].len(e);
        place(&mut proj, sheet, &ps, a.piece, e, l / 2.0, true, 5.0)
    });
    if let Some(f) = foreign {
        notches.push(f);
    }

    // Direction et sélection.
    let reverse = rng.chance(35);
    let (src, dst, from) = if reverse {
        c.b_to_a += 1;
        (b, a, SeamWhich::B)
    } else {
        (a, b, SeamWhich::A)
    };
    let mut which: Option<Vec<NotchId>> = None;
    match rng.below(10) {
        0..=2 => {
            let on_src: Vec<NotchId> = notches
                .iter()
                .filter(|n| abscissa(&ps, &src, n).is_some())
                .map(|n| n.id)
                .collect();
            if !on_src.is_empty() {
                let mut l: Vec<NotchId> =
                    on_src.iter().copied().filter(|_| rng.chance(60)).collect();
                if rng.chance(30) {
                    if let Some(x) = l.first().copied() {
                        l.push(x);
                    }
                }
                which = Some(l);
                c.list_selection += 1;
            }
        }
        3 => {
            // Liste invalide : cran étranger, ou identifiant inconnu.
            let bad = foreign.map(|f| f.id).filter(|_| rng.chance(50));
            let bad = bad.unwrap_or(NotchId(9_999_999));
            let mut l: Vec<NotchId> = notches
                .iter()
                .filter(|n| abscissa(&ps, &src, n).is_some())
                .map(|n| n.id)
                .take(1)
                .collect();
            l.push(bad);
            which = Some(l);
        }
        _ => {}
    }

    // Doublons semés : des crans déjà présents près des positions attendues sur la cible.
    if let Ok(exp) = oracle(&ps, &src, &dst, &notches, &which) {
        for e in exp.iter().take(2) {
            if !rng.chance(45) {
                continue;
            }
            let off = [0.0, 0.004, -0.004, 0.02, -0.02][rng.below(5) as usize];
            // Cran de la cible à (pied attendu + off le long du tracé d'accueil).
            let o = &ps[dst.piece];
            let de = edges_of(&ps, &dst)[e.edge_rank];
            let (va, vb) = (o.verts[de], o.verts[(de + 1) % o.n()]);
            let l = o.len(de);
            let dist = ((e.foot.0 - va.0).powi(2) + (e.foot.1 - va.1).powi(2)).sqrt() + off;
            if dist < 0.0 || dist > l {
                continue;
            }
            let _ = vb;
            notches.push(place(&mut proj, sheet, &ps, dst.piece, de, dist, true, 4.0));
        }
    }

    // Marche.
    let expected = oracle(&ps, &src, &dst, &notches, &which);
    let before_bytes = bytes(&proj);
    let before_ids: Vec<NotchId> = proj
        .sheet(sheet)
        .unwrap()
        .notches
        .iter()
        .map(|n| n.id)
        .collect();
    let cmd = Command::MarchNotches {
        pair,
        from,
        which: which.clone(),
    };
    let preview = proj.march_preview(pair, from, which.as_deref());
    assert_eq!(
        bytes(&proj),
        before_bytes,
        "l'aperçu ne modifie rien (séq. {seq})"
    );
    let mut doc = Document::new(proj.clone());
    match expected {
        Err(r) => {
            let e = doc.execute(cmd.clone()).unwrap_err().to_string();
            assert_eq!(
                bytes(doc.project()),
                before_bytes,
                "refus sans trace (séq. {seq}) : {e}"
            );
            assert!(!doc.can_undo(), "refus sans étape (séq. {seq})");
            match r {
                Refusal::NoSource => c.refused_no_source += 1,
                Refusal::BadList => c.refused_bad_list += 1,
                Refusal::AllPresent => c.refused_all_present += 1,
            }
            if let Ok(v) = &preview {
                // Aperçu lisible même quand tout existe : tous marqués doublons.
                assert!(v.items.iter().all(|i| i.duplicate), "séq. {seq}");
            } else {
                assert!(matches!(r, Refusal::NoSource | Refusal::BadList));
            }
        }
        Ok(exp) => {
            let o = doc.execute(cmd.clone()).unwrap_or_else(|e| {
                panic!("séq. {seq} : refus inattendu : {e}");
            });
            c.marches_ok += 1;
            let fresh: Vec<&Exp> = exp.iter().filter(|e| !e.dup).collect();
            c.duplicates_skipped += exp.len() - fresh.len();
            assert_eq!(
                o.notches.len(),
                fresh.len(),
                "séq. {seq} : nombre de crans créés"
            );
            c.created += o.notches.len();
            let after = doc.project();
            let sh = after.sheet(sheet).unwrap();
            // Anciens crans intacts, nouveaux = ceux de l'outcome et rien d'autre.
            assert_eq!(
                sh.notches.len(),
                before_ids.len() + fresh.len(),
                "séq. {seq}"
            );
            for id in &before_ids {
                assert_eq!(
                    sh.notch(*id).unwrap(),
                    proj.sheet(sheet).unwrap().notch(*id).unwrap(),
                    "séq. {seq} : cran existant modifié"
                );
            }
            let mut used = vec![false; o.notches.len()];
            for e in &fresh {
                let k = o
                    .notches
                    .iter()
                    .enumerate()
                    .position(|(i, n)| {
                        !used[i] && {
                            let (f, t) = foot_tip(after, sheet, *n);
                            near2(f, e.foot) && near2(t, e.tip)
                        }
                    })
                    .unwrap_or_else(|| {
                        let got: Vec<_> = o
                            .notches
                            .iter()
                            .map(|n| foot_tip(after, sheet, *n))
                            .collect();
                        let ex: Vec<_> = exp.iter().map(|e| (e.source, e.dup, e.foot)).collect();
                        let pv: Vec<_> = preview.as_ref().unwrap().items.iter().map(|i| (i.source, i.duplicate, i.target_mm, i.point)).collect();
                        let nn: Vec<_> = notches.iter().map(|n| (n.id, n.piece, n.edge, n.d)).collect();
                        panic!(
                            "séq. {seq} : cran attendu {:?} {:?} absent ; créés {got:?} ; EXP {ex:?} PREV {pv:?} NOTCHES {nn:?}",
                            e.foot, e.tip
                        )
                    });
                used[k] = true;
                c.positions_checked += 1;
                if e.edge_rank > 0 {
                    c.landed_on_later_edge += 1;
                }
                let n = sh.notch(o.notches[k]).unwrap();
                assert!(n.anchor.is_none() && !n.on_cut, "séq. {seq}");
            }
            if dst.count > 1 {
                c.multi_edge_target += 1;
            }
            // Doublons « presque » : un cran voisin à 0,02 mm n'empêche pas le report.
            c.near_dup_kept += fresh
                .iter()
                .filter(|f| {
                    notches.iter().any(|n| {
                        n.piece == dst.piece
                            && abscissa(&ps, &dst, n).is_some()
                            && foot_tip(&proj, sheet, n.id).0 != f.foot
                            && ((foot_tip(&proj, sheet, n.id).0 .0 - f.foot.0).powi(2)
                                + (foot_tip(&proj, sheet, n.id).0 .1 - f.foot.1).powi(2))
                            .sqrt()
                                < 0.03
                    })
                })
                .count();
            // Aperçu = commande.
            let v = preview
                .as_ref()
                .unwrap_or_else(|e| panic!("séq. {seq} : {e}"));
            assert_eq!(v.items.len(), exp.len(), "séq. {seq}");
            for (it, e) in v.items.iter().zip(&exp) {
                assert_eq!(it.source, e.source, "séq. {seq}");
                assert_eq!(it.duplicate, e.dup, "séq. {seq}");
                assert!(near2((it.point.x, it.point.y), e.foot), "séq. {seq}");
            }
            c.preview_checks += 1;
            // Une seule étape annulable, rétablissement, JSON.
            let done = bytes(after);
            assert!(doc.undo(), "séq. {seq}");
            assert_eq!(
                bytes(doc.project()),
                before_bytes,
                "séq. {seq} : annulation"
            );
            assert!(!doc.can_undo(), "séq. {seq} : une seule étape");
            assert!(doc.redo());
            assert_eq!(bytes(doc.project()), done, "séq. {seq} : rétablir");
            c.undo_checks += 1;
            let back = persist::from_json(&done).unwrap();
            assert_eq!(&back, doc.project(), "séq. {seq} : JSON");
            c.json_checks += 1;
            // Rejeu : tout est présent → refus sans trace.
            let mut again = doc.project().clone();
            let e = again.apply(cmd.clone());
            // Tout est maintenant présent sur la cible : le rejeu est refusé, sans trace.
            assert!(e.is_err(), "séq. {seq} : rejeu");
            assert_eq!(bytes(&again), done, "séq. {seq} : rejeu sans trace");
            c.replay_refused += 1;
        }
    }
    // Couple inconnu : refus sans trace.
    if seq % 8 == 0 {
        let mut q = proj.clone();
        let b0 = bytes(&q);
        assert!(q
            .apply(Command::MarchNotches {
                pair: SeamPairId(8_888_888),
                from,
                which: None
            })
            .is_err());
        assert_eq!(bytes(&q), b0);
        c.refused_unknown_pair += 1;
    }
    c.sequences += 1;
}

#[test]
fn campagne_adverse_marche_de_pieces_160_sequences() {
    let mut c = Counters::default();
    for seq in 0..160 {
        run_sequence(seq, &mut c);
    }
    eprintln!("{c:#?}");
    assert_eq!(c.sequences, 160);
    // Les contrôles forts ont réellement tourné.
    assert!(c.marches_ok >= 80, "{c:?}");
    assert!(c.created >= 150, "{c:?}");
    assert!(c.positions_checked >= 150, "{c:?}");
    assert!(c.refused_no_source >= 3, "{c:?}");
    assert!(c.refused_bad_list >= 3, "{c:?}");
    assert!(c.refused_all_present >= 1, "{c:?}");
    assert!(c.duplicates_skipped >= 5, "{c:?}");
    assert!(c.near_dup_kept >= 1, "{c:?}");
    assert!(c.list_selection >= 10, "{c:?}");
    assert!(c.b_to_a >= 20, "{c:?}");
    assert!(c.same_piece >= 5, "{c:?}");
    assert!(c.whole_side >= 5, "{c:?}");
    assert!(c.multi_edge_target >= 30, "{c:?}");
    assert!(c.landed_on_later_edge >= 20, "{c:?}");
    assert!(
        c.undo_checks == c.marches_ok && c.json_checks == c.marches_ok,
        "{c:?}"
    );
    assert!(c.preview_checks == c.marches_ok, "{c:?}");
    assert!(c.replay_refused >= 40, "{c:?}");
    assert!(c.refused_unknown_pair >= 10, "{c:?}");
}
