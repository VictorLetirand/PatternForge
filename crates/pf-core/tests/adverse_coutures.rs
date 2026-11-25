//! US-08.1 : campagne adverse des coutures appariées, 200 séquences à graine fixe. Chaque séquence
//! crée trois pièces (rectangles à dimensions entières, ou polygones convexes aléatoires, ou
//! « D » = corde + arc), puis tente des déclarations valides et invalides (suites de tracés
//! continues, non continues, doublons, tracés d'une autre pièce, pièce inconnue, tracé déjà cousu,
//! valeurs hors bornes ou non finies), des modifications, des retraits, des déplacements de
//! sommets, des sorties / retours de tracés du contour, des suppressions de pièces, des annulations
//! et rétablissements, des aller-retours JSON.
//!
//! **Oracle indépendant** : il tient ses propres sommets et longueurs d'arc, calcule seul chaque
//! longueur de couture (somme de distances de sommets, ou rayon × angle pour un arc), l'écart,
//! l'embu et le statut, décide seul si une déclaration est acceptée (continuité cyclique du
//! contour qu'il tient, arêtes jamais cousues deux fois, bornes), et ce que devient la liste après
//! chaque commande (retrait des couples dont un tracé a quitté le contour). Les séquences « entier »
//! (dimensions et valeurs entières) comparent le statut **à l'égalité exacte** (limite incluse) ;
//! les autres écartent (et comptent) les cas à moins de 1e-6 mm de la limite. Des compteurs
//! prouvent que les contrôles forts s'exécutent. Mutations vérifiées : voir `docs/JOURNAL.md`
//! (tranche 71).

use pf_core::persist;
use pf_core::*;
use pf_geometry::PointMm;
use std::collections::HashSet;

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
    ops: usize,
    adds_ok: usize,
    adds_refused: usize,
    refused_not_consecutive: usize,
    refused_overlap: usize,
    refused_bounds: usize,
    refused_unknown_piece: usize,
    sets_ok: usize,
    removes_ok: usize,
    moves_ok: usize,
    outline_changes: usize,
    pieces_deleted: usize,
    pruned: usize,
    undo_roundtrips: usize,
    json_roundtrips: usize,
    reports_checked: usize,
    within: usize,
    out_of_tolerance: usize,
    not_computable: usize,
    exact_boundary: usize,
    boundary_skipped: usize,
    arc_sides: usize,
    multi_edge_sides: usize,
    wrap_sides: usize,
}

enum Len {
    Seg(usize, usize),
    Fixed(f64),
}

struct OPiece {
    id: PieceId,
    sheet: SheetId,
    verts: Vec<(PointId, (f64, f64))>,
    kinds: Vec<(EntityId, Len)>,
    outline: Vec<EntityId>,
    full: Vec<PieceEdge>,
    movable: bool,
}

#[derive(Clone, PartialEq)]
struct OSide {
    piece: PieceId,
    edges: Vec<EntityId>,
}

#[derive(Clone)]
struct OPair {
    id: SeamPairId,
    a: OSide,
    b: OSide,
    embu: Option<f64>,
    tol: Option<f64>,
}

struct Oracle {
    pieces: Vec<OPiece>,
    pairs: Vec<OPair>,
    integer: bool,
}

impl Oracle {
    fn piece(&self, id: PieceId) -> Option<&OPiece> {
        self.pieces.iter().find(|p| p.id == id)
    }

    fn edge_len(p: &OPiece, e: EntityId) -> f64 {
        match &p.kinds.iter().find(|(id, _)| *id == e).unwrap().1 {
            Len::Seg(i, j) => {
                let (a, b) = (p.verts[*i].1, p.verts[*j].1);
                ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt()
            }
            Len::Fixed(l) => *l,
        }
    }

    /// Suite continue (cyclique) de tracés du contour tenu par l'oracle.
    fn side_valid(&self, s: &OSide) -> bool {
        let Some(p) = self.piece(s.piece) else {
            return false;
        };
        let n = p.outline.len();
        if s.edges.is_empty() || s.edges.len() > n {
            return false;
        }
        let Some(start) = p.outline.iter().position(|e| *e == s.edges[0]) else {
            return false;
        };
        s.edges
            .iter()
            .enumerate()
            .all(|(i, e)| p.outline[(start + i) % n] == *e)
    }

    fn values_valid(embu: Option<f64>, tol: Option<f64>) -> bool {
        [embu, tol]
            .iter()
            .flatten()
            .all(|v| v.is_finite() && (0.0..=1000.0).contains(v))
    }

    /// Verdict complet d'une déclaration (le couple `ignore` est remplacé).
    fn accepts(
        &self,
        a: &OSide,
        b: &OSide,
        embu: Option<f64>,
        tol: Option<f64>,
        ignore: Option<SeamPairId>,
    ) -> Result<(), &'static str> {
        if self.piece(a.piece).is_none() || self.piece(b.piece).is_none() {
            return Err("unknown_piece");
        }
        if !self.side_valid(a) || !self.side_valid(b) {
            return Err("not_consecutive");
        }
        if !Self::values_valid(embu, tol) {
            return Err("bounds");
        }
        let mut used: HashSet<(PieceId, EntityId)> = HashSet::new();
        for q in self.pairs.iter().filter(|q| Some(q.id) != ignore) {
            for s in [&q.a, &q.b] {
                for e in &s.edges {
                    used.insert((s.piece, *e));
                }
            }
        }
        for s in [a, b] {
            for e in &s.edges {
                if !used.insert((s.piece, *e)) {
                    return Err("overlap");
                }
            }
        }
        Ok(())
    }

    fn side_len(&self, s: &OSide) -> Result<f64, SeamIssue> {
        let p = self.piece(s.piece).unwrap();
        if p.outline.len() != p.full.len() {
            return Err(SeamIssue::OutlineOpen);
        }
        Ok(s.edges.iter().map(|e| Self::edge_len(p, *e)).sum())
    }

    /// Retire les couples dont un côté n'est plus une suite continue du contour tenu.
    fn prune(&mut self, c: &mut Counters) {
        let keep: Vec<OPair> = self
            .pairs
            .iter()
            .filter(|q| self.side_valid(&q.a) && self.side_valid(&q.b))
            .cloned()
            .collect();
        c.pruned += self.pairs.len() - keep.len();
        self.pairs = keep;
    }
}

fn build_piece(
    rng: &mut Rng,
    proj: &mut Project,
    sheet: SheetId,
    index: usize,
    integer: bool,
) -> OPiece {
    let ox = 1500.0 * index as f64;
    let mut verts: Vec<(PointId, (f64, f64))> = Vec::new();
    let mut kinds: Vec<(EntityId, Len)> = Vec::new();
    let mut ents: Vec<EntityId> = Vec::new();
    let mut coords: Vec<(f64, f64)> = Vec::new();
    let mut movable = true;
    let shape = if integer { 0 } else { rng.below(3) };
    let mut arc_len = None;
    match shape {
        0 => {
            let w = rng.range(20.0, 300.0).floor();
            let h = rng.range(20.0, 300.0).floor();
            coords = vec![(ox, 0.0), (ox + w, 0.0), (ox + w, h), (ox, h)];
        }
        1 => {
            let n = 3 + rng.below(5) as usize;
            for i in 0..n {
                let a = std::f64::consts::TAU * (i as f64 + rng.range(0.1, 0.9)) / n as f64;
                let (rx, ry) = (rng.range(60.0, 200.0), rng.range(60.0, 200.0));
                coords.push((ox + rx * a.cos(), ry * a.sin()));
            }
        }
        _ => {
            movable = false;
            let r = rng.range(40.0, 200.0);
            let a0: f64 = rng.range(0.0, 90.0);
            let sweep = rng.range(30.0, 170.0);
            let a1 = a0 + sweep;
            let at = |d: f64| (ox + r * d.to_radians().cos(), r * d.to_radians().sin());
            let arc = proj
                .apply(Command::AddArc {
                    sheet,
                    center: PointRef::New(pt(ox, 0.0)),
                    radius_mm: r,
                    start_deg: a0,
                    end_deg: a1,
                    ccw: true,
                })
                .unwrap()
                .entities[0];
            ents.push(arc);
            arc_len = Some(r * sweep.to_radians());
            coords = vec![at(a0), at(a1)];
        }
    }
    let n = coords.len();
    let segs = if arc_len.is_some() { 1 } else { n };
    for k in 0..segs {
        let (a, b) = (coords[k], coords[(k + 1) % n]);
        let e = proj
            .apply(Command::AddSegment {
                sheet,
                from: PointRef::New(pt(a.0, a.1)),
                to: PointRef::New(pt(b.0, b.1)),
            })
            .unwrap()
            .entities[0];
        ents.push(e);
        kinds.push((e, Len::Seg(k, (k + 1) % n)));
    }
    if let Some(l) = arc_len {
        kinds.push((ents[0], Len::Fixed(l)));
    }
    let piece = proj
        .apply(Command::ExtractPiece {
            sheet,
            source: PieceSource::Paths { entities: ents },
            name: format!("Pièce {index}"),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    let sh = proj.sheet(sheet).unwrap();
    for (x, y) in &coords {
        let id = sh
            .points
            .iter()
            .find(|q| {
                sh.position(q.id)
                    .is_ok_and(|v| (v.x - x).abs() < 1e-6 && (v.y - y).abs() < 1e-6)
            })
            .unwrap()
            .id;
        verts.push((id, (*x, *y)));
    }
    let pc = proj.piece(piece).unwrap();
    OPiece {
        id: piece,
        sheet,
        verts,
        kinds,
        outline: pc.outline.iter().map(|e| e.entity).collect(),
        full: pc.outline.clone(),
        movable,
    }
}

fn random_side(rng: &mut Rng, o: &Oracle, c: &mut Counters) -> OSide {
    let ghost = rng.chance(4);
    if ghost || o.pieces.is_empty() {
        return OSide {
            piece: PieceId(987_654),
            edges: vec![EntityId(1)],
        };
    }
    let p = &o.pieces[rng.below(o.pieces.len() as u64) as usize];
    let n = p.outline.len();
    let mode = rng.below(10);
    let edges = match mode {
        0..=6 => {
            let start = rng.below(n as u64) as usize;
            let count = 1 + rng.below(n.min(4) as u64) as usize;
            (0..count).map(|k| p.outline[(start + k) % n]).collect()
        }
        7 => vec![p.outline[0], p.outline[2 % n]],
        8 => {
            let e = p.outline[rng.below(n as u64) as usize];
            vec![e, e]
        }
        _ => {
            let other = &o.pieces[rng.below(o.pieces.len() as u64) as usize];
            vec![other.outline[0]]
        }
    };
    let s = OSide { piece: p.id, edges };
    if s.edges.len() > 1 && o.side_valid(&s) {
        c.multi_edge_sides += 1;
    }
    s
}

fn random_value(rng: &mut Rng, integer: bool) -> Option<f64> {
    match rng.below(40) {
        0..=11 => None,
        12 => Some(0.0),
        13 => Some(1000.0),
        14 => Some(-1.0),
        15 => Some(1000.5),
        16 => Some(f64::NAN),
        _ => Some(if integer {
            rng.below(21) as f64
        } else {
            rng.range(0.0, 40.0)
        }),
    }
}

type Declared = (Option<f64>, Option<f64>);

/// Embu et tolérance choisis d'après les longueurs de l'oracle pour que l'écart à l'attendu vaille
/// 0, la moitié, la tolérance exacte (limite incluse) ou le double de la tolérance.
fn calibrate(rng: &mut Rng, o: &Oracle, a: &OSide, b: &OSide) -> Option<Declared> {
    if !o.side_valid(a) || !o.side_valid(b) {
        return None;
    }
    let (Ok(la), Ok(lb)) = (o.side_len(a), o.side_len(b)) else {
        return None;
    };
    let m = la - lb;
    let tol = if o.integer {
        rng.below(6) as f64
    } else {
        rng.range(0.0, 5.0)
    };
    let dev = tol * [0.0, 0.5, 1.0, 1.0, 2.0][rng.below(5) as usize];
    let embu = if m - dev >= 0.0 { m - dev } else { m + dev };
    (0.0..=1000.0)
        .contains(&embu)
        .then_some((Some(embu), Some(tol)))
}

fn to_side(s: &OSide) -> SeamSide {
    SeamSide {
        piece: s.piece,
        edges: s.edges.clone(),
    }
}

fn check_report(proj: &Project, o: &Oracle, c: &mut Counters) {
    let r = proj.seam_pair_report();
    assert_eq!(r.len(), o.pairs.len(), "nombre de couples");
    assert_eq!(proj.seams.len(), o.pairs.len());
    for (i, (rd, q)) in r.iter().zip(&o.pairs).enumerate() {
        c.reports_checked += 1;
        assert_eq!((rd.id, rd.number), (q.id, i + 1));
        let (la, lb) = (o.side_len(&q.a), o.side_len(&q.b));
        let tol = q.tol.unwrap_or(1.0);
        let exp = q.embu.unwrap_or(0.0);
        assert_eq!(rd.tolerance_mm, tol);
        assert_eq!(rd.embu_expected_mm, exp);
        for (got, want) in [(rd.length_a_mm, &la), (rd.length_b_mm, &lb)] {
            match want {
                Ok(l) => assert!((got.unwrap() - l).abs() < 1e-6, "{got:?} ≠ {l}"),
                Err(_) => assert!(got.is_none()),
            }
        }
        if o.piece(q.a.piece).is_some_and(|p| {
            p.kinds
                .iter()
                .any(|(e, k)| q.a.edges.contains(e) && matches!(k, Len::Fixed(_)))
        }) {
            c.arc_sides += 1;
        }
        match (la, lb) {
            (Err(reason), _) => {
                c.not_computable += 1;
                assert_eq!(
                    rd.status,
                    SeamStatus::NotComputable {
                        side: SeamWhich::A,
                        reason
                    }
                );
                assert!(rd.gap_mm.is_none() && rd.embu_measured_mm.is_none());
            }
            (_, Err(reason)) => {
                c.not_computable += 1;
                assert_eq!(
                    rd.status,
                    SeamStatus::NotComputable {
                        side: SeamWhich::B,
                        reason
                    }
                );
            }
            (Ok(a), Ok(b)) => {
                let m = a - b;
                assert!((rd.embu_measured_mm.unwrap() - m).abs() < 1e-6);
                assert!((rd.gap_mm.unwrap() - m.abs()).abs() < 1e-6);
                let dev = (m - exp).abs();
                assert!((rd.deviation_mm.unwrap() - dev).abs() < 1e-6);
                if !o.integer && (dev - tol).abs() < 1e-6 {
                    c.boundary_skipped += 1;
                    continue;
                }
                if o.integer && dev == tol {
                    c.exact_boundary += 1;
                }
                if dev <= tol {
                    c.within += 1;
                    assert_eq!(rd.status, SeamStatus::Within, "{rd:?} dev {dev} tol {tol}");
                } else {
                    c.out_of_tolerance += 1;
                    assert_eq!(
                        rd.status,
                        SeamStatus::OutOfTolerance,
                        "{rd:?} dev {dev} tol {tol}"
                    );
                }
            }
        }
    }
}

fn run_sequence(seq: usize, c: &mut Counters) {
    let mut rng =
        Rng(0x9E37_79B9_7F4A_7C15 ^ ((seq as u64 + 1).wrapping_mul(0xA24B_AED4_963E_E407)));
    let integer = seq % 3 == 0;
    let mut proj = Project::new("adverse coutures");
    let sheet = proj.sheets[0].id;
    let mut o = Oracle {
        pieces: Vec::new(),
        pairs: Vec::new(),
        integer,
    };
    for i in 0..3 {
        let p = build_piece(&mut rng, &mut proj, sheet, i, integer);
        o.pieces.push(p);
    }
    let mut doc = Document::new(proj);
    let ops = 25 + rng.below(16);
    for _ in 0..ops {
        c.ops += 1;
        let before = bytes(doc.project());
        let dice = rng.below(100);
        match dice {
            0..=49 => {
                let a = random_side(&mut rng, &o, c);
                let b = random_side(&mut rng, &o, c);
                let (mut embu, mut tol) = (
                    random_value(&mut rng, integer),
                    random_value(&mut rng, integer),
                );
                if rng.chance(55) {
                    if let Some((e, t)) = calibrate(&mut rng, &o, &a, &b) {
                        (embu, tol) = (e, t);
                    }
                }
                let verdict = o.accepts(&a, &b, embu, tol, None);
                let r = doc.execute(Command::AddSeamPair {
                    a: to_side(&a),
                    b: to_side(&b),
                    embu_mm: embu,
                    tolerance_mm: tol,
                });
                match (verdict, r) {
                    (Ok(()), Ok(out)) => {
                        c.adds_ok += 1;
                        if a.edges.len() > 1
                            && a.edges[0]
                                == o.piece(a.piece).unwrap().outline.last().copied().unwrap()
                        {
                            c.wrap_sides += 1;
                        }
                        o.pairs.push(OPair {
                            id: out.seam_pairs[0],
                            a,
                            b,
                            embu,
                            tol,
                        });
                    }
                    (Err(why), Err(_)) => {
                        c.adds_refused += 1;
                        match why {
                            "not_consecutive" => c.refused_not_consecutive += 1,
                            "overlap" => c.refused_overlap += 1,
                            "bounds" => c.refused_bounds += 1,
                            _ => c.refused_unknown_piece += 1,
                        }
                        assert_eq!(before, bytes(doc.project()), "refus sans trace");
                    }
                    (v, r) => panic!("verdict {v:?} ≠ résultat {:?}", r.map(|_| ())),
                }
            }
            50..=62 => {
                if o.pairs.is_empty() {
                    continue;
                }
                let k = rng.below(o.pairs.len() as u64) as usize;
                let id = o.pairs[k].id;
                let (a, b) = if rng.chance(30) {
                    (o.pairs[k].a.clone(), o.pairs[k].b.clone())
                } else {
                    (random_side(&mut rng, &o, c), random_side(&mut rng, &o, c))
                };
                let (mut embu, mut tol) = (
                    random_value(&mut rng, integer),
                    random_value(&mut rng, integer),
                );
                if rng.chance(55) {
                    if let Some((e, t)) = calibrate(&mut rng, &o, &a, &b) {
                        (embu, tol) = (e, t);
                    }
                }
                let verdict = o.accepts(&a, &b, embu, tol, Some(id));
                let r = doc.execute(Command::SetSeamPair {
                    id,
                    a: to_side(&a),
                    b: to_side(&b),
                    embu_mm: embu,
                    tolerance_mm: tol,
                });
                match (verdict, r) {
                    (Ok(()), Ok(_)) => {
                        c.sets_ok += 1;
                        o.pairs[k] = OPair {
                            id,
                            a,
                            b,
                            embu,
                            tol,
                        };
                    }
                    (Err(_), Err(_)) => {
                        assert_eq!(before, bytes(doc.project()), "refus sans trace");
                    }
                    (v, r) => panic!("set : verdict {v:?} ≠ résultat {:?}", r.map(|_| ())),
                }
            }
            63..=68 => {
                let unknown = o.pairs.is_empty() || rng.chance(10);
                let id = if unknown {
                    SeamPairId(987_654)
                } else {
                    o.pairs[rng.below(o.pairs.len() as u64) as usize].id
                };
                let r = doc.execute(Command::RemoveSeamPair { id });
                if unknown {
                    assert!(r.is_err());
                    assert_eq!(before, bytes(doc.project()));
                } else {
                    r.unwrap();
                    c.removes_ok += 1;
                    o.pairs.retain(|q| q.id != id);
                }
            }
            69..=82 => {
                if integer {
                    continue;
                }
                let movable: Vec<usize> = (0..o.pieces.len())
                    .filter(|i| o.pieces[*i].movable)
                    .collect();
                if movable.is_empty() {
                    continue;
                }
                let pi = movable[rng.below(movable.len() as u64) as usize];
                let vi = rng.below(o.pieces[pi].verts.len() as u64) as usize;
                let (pid, (x, y)) = o.pieces[pi].verts[vi];
                let to = (x + rng.range(-25.0, 25.0), y + rng.range(-25.0, 25.0));
                let r = doc.execute(Command::MovePoint {
                    sheet: o.pieces[pi].sheet,
                    point: pid,
                    to: pt(to.0, to.1),
                });
                if r.is_ok() {
                    c.moves_ok += 1;
                    o.pieces[pi].verts[vi].1 = to;
                } else {
                    assert_eq!(
                        before,
                        bytes(doc.project()),
                        "déplacement refusé sans trace"
                    );
                }
            }
            83..=88 => {
                let pi = rng.below(o.pieces.len() as u64) as usize;
                let full = o.pieces[pi].full.clone();
                let current = o.pieces[pi].outline.len();
                let new: Vec<PieceEdge> = if current == full.len() && rng.chance(80) {
                    let drop = rng.below(full.len() as u64) as usize;
                    full.iter()
                        .enumerate()
                        .filter(|(i, _)| *i != drop)
                        .map(|(_, e)| *e)
                        .collect()
                } else {
                    full
                };
                let r = doc.execute(Command::SetPieceOutline {
                    piece: o.pieces[pi].id,
                    outline: new.clone(),
                    grainline: None,
                });
                if r.is_ok() {
                    c.outline_changes += 1;
                    o.pieces[pi].outline = new.iter().map(|e| e.entity).collect();
                    o.prune(c);
                } else {
                    assert_eq!(before, bytes(doc.project()));
                }
            }
            89..=90 => {
                if o.pieces.len() <= 1 {
                    continue;
                }
                let pi = rng.below(o.pieces.len() as u64) as usize;
                let id = o.pieces[pi].id;
                doc.execute(Command::DeletePiece { piece: id }).unwrap();
                c.pieces_deleted += 1;
                o.pieces.remove(pi);
                let n = o.pairs.len();
                o.pairs.retain(|q| q.a.piece != id && q.b.piece != id);
                c.pruned += n - o.pairs.len();
            }
            _ => {
                let after = bytes(doc.project());
                if after != before && doc.can_undo() {
                    assert!(doc.undo());
                    assert!(doc.redo());
                    assert_eq!(after, bytes(doc.project()), "annuler puis rétablir");
                    c.undo_roundtrips += 1;
                }
                let back = persist::from_json(&after).expect("fichier relu");
                assert_eq!(bytes(&back), after, "aller-retour JSON");
                assert_eq!(back.seam_pair_report(), doc.project().seam_pair_report());
                c.json_roundtrips += 1;
            }
        }
        // Après toute commande acceptée : annuler redonne l'état d'avant, octet pour octet.
        let after = bytes(doc.project());
        if after != before && rng.chance(15) && doc.can_undo() {
            assert!(doc.undo());
            assert_eq!(
                before,
                bytes(doc.project()),
                "une seule étape annule la commande"
            );
            assert!(doc.redo());
            assert_eq!(after, bytes(doc.project()));
            c.undo_roundtrips += 1;
        }
        doc.project().validate().unwrap();
        check_report(doc.project(), &o, c);
    }
    c.sequences += 1;
}

#[test]
fn campagne_adverse_coutures_appariees_200_sequences() {
    let mut c = Counters::default();
    for seq in 0..200 {
        run_sequence(seq, &mut c);
    }
    eprintln!("{c:?}");
    assert_eq!(c.sequences, 200);
    // Les contrôles forts se sont réellement exécutés.
    assert!(c.adds_ok >= 300, "{c:?}");
    assert!(c.adds_refused >= 300, "{c:?}");
    assert!(c.refused_not_consecutive >= 50, "{c:?}");
    assert!(c.refused_overlap >= 20, "{c:?}");
    assert!(c.refused_bounds >= 50, "{c:?}");
    assert!(c.refused_unknown_piece >= 10, "{c:?}");
    assert!(c.sets_ok >= 50 && c.removes_ok >= 50, "{c:?}");
    assert!(c.moves_ok >= 200, "{c:?}");
    assert!(c.outline_changes >= 100 && c.pruned >= 20, "{c:?}");
    assert!(c.pieces_deleted >= 20, "{c:?}");
    assert!(c.undo_roundtrips >= 100 && c.json_roundtrips >= 50, "{c:?}");
    assert!(c.reports_checked >= 3000, "{c:?}");
    assert!(c.within >= 500 && c.out_of_tolerance >= 500, "{c:?}");
    assert!(c.not_computable >= 100, "{c:?}");
    assert!(c.exact_boundary >= 20, "{c:?}");
    assert!(c.arc_sides >= 100, "{c:?}");
    assert!(c.multi_edge_sides >= 100 && c.wrap_sides >= 5, "{c:?}");
}
