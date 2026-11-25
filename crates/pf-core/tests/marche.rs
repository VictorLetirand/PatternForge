//! US-08.2 — marche de pièces. Valeurs attendues calculées à la main (coordonnées du plan), jamais
//! relues du code testé. CA1 abscisse proportionnelle (embu réparti), CA2 sens de couture, CA3 côté
//! multi-tracés, CA4 courbe (oracle par subdivision fine), CA5 vrais crans copiés, CA6 sélection et
//! refus sans trace, CA7 pas de doublon, CA8 une étape annulable, CA9 aperçu sans modification,
//! CA10 fichiers.

use pf_core::persist;
use pf_core::*;
use pf_geometry::{PointMm, Side};
use serde_json::Value;

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

fn bytes(p: &Project) -> String {
    persist::to_json(p).unwrap()
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

fn rect_piece(p: &mut Project, s: SheetId, x: f64, y: f64, w: f64, h: f64, name: &str) -> PieceId {
    let o = p
        .apply(Command::AddRectangle {
            sheet: s,
            corner: PointRef::New(pt(x, y)),
            width_mm: w,
            height_mm: h,
        })
        .unwrap();
    p.apply(Command::ExtractPiece {
        sheet: s,
        source: PieceSource::Paths {
            entities: o.entities,
        },
        name: name.into(),
        detached: false,
    })
    .unwrap()
    .pieces[0]
}

fn edge(p: &Project, s: SheetId, a: (f64, f64), b: (f64, f64)) -> EntityId {
    let sheet = p.sheet(s).unwrap();
    let near = |u: PointMm, v: (f64, f64)| (u.x - v.0).abs() < 1e-9 && (u.y - v.1).abs() < 1e-9;
    sheet
        .entities
        .iter()
        .map(|e| e.id)
        .find(|id| {
            sheet
                .segment_ends(*id)
                .is_ok_and(|(u, v)| (near(u, a) && near(v, b)) || (near(u, b) && near(v, a)))
        })
        .expect("tracé introuvable")
}

fn side(p: &Project, piece: PieceId, edges: &[EntityId]) -> SeamSide {
    for start in edges {
        let s = p.seam_side_range(piece, *start, edges.len()).unwrap();
        let mut a = s.edges.clone();
        let mut b = edges.to_vec();
        a.sort();
        b.sort();
        if a == b {
            return s;
        }
    }
    panic!("tracés non consécutifs");
}

fn pair(p: &mut Project, a: SeamSide, b: SeamSide) -> SeamPairId {
    p.apply(Command::AddSeamPair {
        a,
        b,
        embu_mm: None,
        tolerance_mm: None,
    })
    .unwrap()
    .seam_pairs[0]
}

/// Cran posé à `d` mm du point `from` (coordonnées du plan) le long d'un segment, quel que soit
/// le sens d'enregistrement du tracé ; `side` est lu en allant de `from` vers l'autre extrémité.
fn put(p: &mut Project, s: SheetId, e: EntityId, from: (f64, f64), d: f64, side: Side) -> NotchId {
    let sh = p.sheet(s).unwrap();
    let (u, _) = sh.segment_ends(e).unwrap();
    let len = sh.path_length_mm(e).unwrap();
    let from_start = (u.x - from.0).abs() < 1e-9 && (u.y - from.1).abs() < 1e-9;
    let side = match (side, from_start) {
        (x, true) => x,
        (Side::Left, false) => Side::Right,
        (Side::Right, false) => Side::Left,
    };
    p.apply(Command::AddNotch {
        sheet: s,
        entity: e,
        distance_mm: if from_start { d } else { len - d },
        side,
        length_mm: 5.0,
        style: NotchStyle::Single,
    })
    .unwrap()
    .notches[0]
}

/// (pied, pointe) d'un cran, dans le plan.
fn foot_tip(p: &Project, s: SheetId, n: NotchId) -> ((f64, f64), (f64, f64)) {
    let sh = p.sheet(s).unwrap();
    let m = sh.notch_marks(sh.notch(n).unwrap()).unwrap()[0];
    ((m.0.x, m.0.y), (m.1.x, m.1.y))
}

/// Crans de la feuille portés par ces tracés, hors `known`, triés par abscisse x du pied.
fn fresh_on(p: &Project, s: SheetId, edges: &[EntityId], known: &[NotchId]) -> Vec<NotchId> {
    let sh = p.sheet(s).unwrap();
    let mut v: Vec<(f64, NotchId)> = sh
        .notches
        .iter()
        .filter(|n| edges.contains(&n.entity) && !known.contains(&n.id))
        .map(|n| (foot_tip(p, s, n.id).0 .0, n.id))
        .collect();
    v.sort_by(|a, b| a.0.total_cmp(&b.0));
    v.into_iter().map(|x| x.1).collect()
}

fn refused(p: &mut Project, c: Command) -> String {
    let before = bytes(p);
    let e = p.apply(c).unwrap_err().to_string();
    assert_eq!(before, bytes(p), "un refus ne laisse aucune trace");
    e
}

fn march(pair: SeamPairId, from: SeamWhich) -> Command {
    Command::MarchNotches {
        pair,
        from,
        which: None,
    }
}

/// R1 200 × 50 en (0, 0), R2 195 × 40 en (500, 0) (embu 5) ; A = bas de R1, B = bas de R2 ;
/// contours anti-horaires : bas parcouru de gauche à droite.
struct Rig {
    p: Project,
    s: SheetId,
    r2: PieceId,
    a_edge: EntityId,
    b_edge: EntityId,
    pair: SeamPairId,
}

fn rig() -> Rig {
    let mut p = Project::new("marche");
    let s = p.sheets[0].id;
    let r1 = rect_piece(&mut p, s, 0.0, 0.0, 200.0, 50.0, "Devant");
    let r2 = rect_piece(&mut p, s, 500.0, 0.0, 195.0, 40.0, "Dos");
    let a_edge = edge(&p, s, (0.0, 0.0), (200.0, 0.0));
    let b_edge = edge(&p, s, (500.0, 0.0), (695.0, 0.0));
    let a = side(&p, r1, &[a_edge]);
    let b = side(&p, r2, &[b_edge]);
    let pair = pair(&mut p, a, b);
    Rig {
        p,
        s,
        r2,
        a_edge,
        b_edge,
        pair,
    }
}

// ======================================================================== CA1

#[test]
fn ca1_cran_reporte_a_l_abscisse_proportionnelle_a_la_main() {
    let mut r = rig();
    let n50 = put(&mut r.p, r.s, r.a_edge, (0.0, 0.0), 50.0, Side::Left);
    let n100 = put(&mut r.p, r.s, r.a_edge, (0.0, 0.0), 100.0, Side::Left);
    let n150 = put(&mut r.p, r.s, r.a_edge, (0.0, 0.0), 150.0, Side::Left);
    let known = [n50, n100, n150];
    let o = r.p.apply(march(r.pair, SeamWhich::A)).unwrap();
    assert_eq!(o.notches.len(), 3);
    // A = 200, B = 195 : abscisse cible = abscisse × 195 / 200 = 48,75 / 97,5 / 146,25 depuis le
    // début de B ; contours de même sens (anti-horaire) : début de A contre FIN de B (x = 695) :
    // x = 695 − 48,75 = 646,25 ; 695 − 97,5 = 597,5 ; 695 − 146,25 = 548,75.
    let made = fresh_on(&r.p, r.s, &[r.b_edge], &[]);
    assert_eq!(made.len(), 3);
    let want = [548.75, 597.5, 646.25];
    for (n, x) in made.iter().zip(want) {
        let (foot, tip) = foot_tip(&r.p, r.s, *n);
        assert!(close(foot.0, x) && close(foot.1, 0.0), "{foot:?} ≠ {x}");
        // Cran de A à gauche d'un contour anti-horaire = vers l'intérieur : la pointe de B est
        // aussi à l'intérieur de B (y > 0), à 5 mm.
        assert!(close(tip.0, x) && close(tip.1, 5.0), "{tip:?}");
    }
    // Les crans sources sont intacts.
    for (n, x) in known.iter().zip([50.0, 100.0, 150.0]) {
        assert!(close(foot_tip(&r.p, r.s, *n).0 .0, x));
    }
    // Lus par `notch_distance_mm` : distance depuis le départ du tracé de B (sens d'enregistrement
    // quelconque), recomposée à la main.
    let sh = r.p.sheet(r.s).unwrap();
    let (u, _) = sh.segment_ends(r.b_edge).unwrap();
    for (n, x) in made.iter().zip(want) {
        let d = sh.notch_distance_mm(sh.notch(*n).unwrap()).unwrap();
        let expect = if (u.x - 500.0).abs() < 1e-9 {
            x - 500.0
        } else {
            695.0 - x
        };
        assert!(close(d, expect), "{d} ≠ {expect}");
    }
}

#[test]
fn ca1_ecart_d_abscisse_et_aucun_ecart_sans_embu() {
    let mut r = rig();
    for d in [50.0, 100.0, 150.0] {
        put(&mut r.p, r.s, r.a_edge, (0.0, 0.0), d, Side::Left);
    }
    let v = r.p.march_preview(r.pair, SeamWhich::A, None).unwrap();
    assert!(v.opposite && close(v.from_length_mm, 200.0) && close(v.to_length_mm, 195.0));
    // Écart = abscisse reportée − abscisse source = s × (195 / 200 − 1) = −0,025 × s.
    let gaps: Vec<f64> = v.items.iter().map(|i| i.gap_mm).collect();
    for (g, e) in gaps.iter().zip([-1.25, -2.5, -3.75]) {
        assert!(close(*g, e), "{gaps:?}");
    }
    let src: Vec<f64> = v.items.iter().map(|i| i.source_mm).collect();
    assert!(src
        .iter()
        .zip([50.0, 100.0, 150.0])
        .all(|(a, b)| close(*a, b)));
    // Sans embu : R2 de 200 mm (même longueur) → écart nul, mêmes abscisses depuis le bord opposé.
    let mut q = Project::new("sans embu");
    let s = q.sheets[0].id;
    let r1 = rect_piece(&mut q, s, 0.0, 0.0, 200.0, 50.0, "A");
    let r2 = rect_piece(&mut q, s, 500.0, 0.0, 200.0, 50.0, "B");
    let ae = edge(&q, s, (0.0, 0.0), (200.0, 0.0));
    let be = edge(&q, s, (500.0, 0.0), (700.0, 0.0));
    let (sa, sb) = (side(&q, r1, &[ae]), side(&q, r2, &[be]));
    let id = pair(&mut q, sa, sb);
    put(&mut q, s, ae, (0.0, 0.0), 30.0, Side::Right);
    let v = q.march_preview(id, SeamWhich::A, None).unwrap();
    assert!(close(v.items[0].gap_mm, 0.0));
    q.apply(march(id, SeamWhich::A)).unwrap();
    let n = fresh_on(&q, s, &[be], &[])[0];
    let (foot, tip) = foot_tip(&q, s, n);
    // 700 − 30 = 670 ; côté droit d'un contour anti-horaire = extérieur : y = −5.
    assert!(
        close(foot.0, 670.0) && close(tip.1, -5.0),
        "{foot:?} {tip:?}"
    );
}

// ======================================================================== CA2

/// Même contour, parcouru dans l'autre sens (sens horaire), mêmes tracés.
fn make_clockwise(p: &mut Project, piece: PieceId) {
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

#[test]
fn ca2_sens_de_couture_independant_du_sens_d_enregistrement() {
    let mut r = rig();
    for d in [50.0, 100.0, 150.0] {
        put(&mut r.p, r.s, r.a_edge, (0.0, 0.0), d, Side::Left);
    }
    // B enregistrée dans le sens horaire : sens différents → mêmes sens de couture, début de A
    // contre début de B, mais le début de B est maintenant à x = 695 : mêmes positions du plan.
    make_clockwise(&mut r.p, r.r2);
    assert!(r.p.piece_contour(r.r2).unwrap().clockwise);
    let v = r.p.march_preview(r.pair, SeamWhich::A, None).unwrap();
    assert!(!v.opposite);
    r.p.apply(march(r.pair, SeamWhich::A)).unwrap();
    let made = fresh_on(&r.p, r.s, &[r.b_edge], &[]);
    for (n, x) in made.iter().zip([548.75, 597.5, 646.25]) {
        let (foot, tip) = foot_tip(&r.p, r.s, *n);
        assert!(close(foot.0, x), "{foot:?} ≠ {x}");
        // Intérieur de B (y > 0) quel que soit le sens d'enregistrement.
        assert!(close(tip.1, 5.0), "{tip:?}");
    }
}

#[test]
fn ca2_meme_piece_des_deux_cotes_bord_gauche_contre_bord_droit() {
    // Tube 100 × 50 : bord gauche et bord droit cousus ; contour anti-horaire : le gauche est
    // parcouru de haut en bas, le droit de bas en haut → début de A (haut) contre fin de B (haut).
    let mut p = Project::new("tube");
    let s = p.sheets[0].id;
    let r = rect_piece(&mut p, s, 0.0, 0.0, 100.0, 50.0, "Tube");
    let left = edge(&p, s, (0.0, 0.0), (0.0, 50.0));
    let right = edge(&p, s, (100.0, 0.0), (100.0, 50.0));
    let (a, b) = (side(&p, r, &[left]), side(&p, r, &[right]));
    let id = pair(&mut p, a, b);
    // Cran à 10 mm du haut du bord gauche (y = 40), côté intérieur (x > 0).
    let n = put(&mut p, s, left, (0.0, 50.0), 10.0, Side::Left);
    let (_, tip) = foot_tip(&p, s, n);
    assert!(
        tip.0 > 0.0,
        "{tip:?}: l'intérieur du tube est à droite du bord gauche"
    );
    p.apply(march(id, SeamWhich::A)).unwrap();
    let m = fresh_on(&p, s, &[right], &[])[0];
    let (foot, tip) = foot_tip(&p, s, m);
    assert!(close(foot.0, 100.0) && close(foot.1, 40.0), "{foot:?}");
    assert!(
        close(tip.0, 95.0),
        "intérieur = vers x plus petit : {tip:?}"
    );
}

// ======================================================================== CA3

#[test]
fn ca3_cote_a_plusieurs_traces_le_cran_tombe_sur_le_bon_trace() {
    let mut p = Project::new("multi");
    let s = p.sheets[0].id;
    let r1 = rect_piece(&mut p, s, 0.0, 0.0, 210.0, 50.0, "Dessus");
    let r2 = rect_piece(&mut p, s, 500.0, 0.0, 120.0, 80.0, "Dessous");
    let ae = edge(&p, s, (0.0, 0.0), (210.0, 0.0));
    let b1 = edge(&p, s, (500.0, 0.0), (620.0, 0.0));
    let b2 = edge(&p, s, (620.0, 0.0), (620.0, 80.0));
    let a = side(&p, r1, &[ae]);
    let b = side(&p, r2, &[b1, b2]);
    let id = pair(&mut p, a, b);
    // B suit le contour anti-horaire : bas (120) puis droite (80) = 200 ; A = 210.
    // Cran source à s ; abscisse cible = s × 200 / 210 ; position depuis le début de B =
    // 200 − s × 200 / 210 (sens opposés).
    // s = 10  : 200 − 9,5238095… = 190,4761905 → 2e tracé, y = 70,4761905 ;
    // s = 50  : 200 − 47,6190476 = 152,3809524 → 2e tracé, y = 32,3809524 ;
    // s = 84  : 200 − 80 = 120 exactement → jonction (620 ; 0) ;
    // s = 100 : 200 − 95,2380952 = 104,7619048 → 1er tracé, x = 604,7619048 ;
    // s = 150 : 200 − 142,8571429 = 57,1428571 → 1er tracé, x = 557,1428571.
    for d in [10.0, 50.0, 84.0, 100.0, 150.0] {
        put(&mut p, s, ae, (0.0, 0.0), d, Side::Right);
    }
    p.apply(march(id, SeamWhich::A)).unwrap();
    let sh = p.sheet(s).unwrap();
    let on = |e: EntityId| -> Vec<(f64, f64)> {
        let mut v: Vec<(f64, f64)> = sh
            .notches
            .iter()
            .filter(|n| n.entity == e)
            .map(|n| foot_tip(&p, s, n.id).0)
            .collect();
        v.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
        v
    };
    let f1 = on(b1);
    let f2 = on(b2);
    // Le cran de la jonction peut être sur l'un ou l'autre tracé : même point.
    let all: Vec<(f64, f64)> = f1.iter().chain(f2.iter()).copied().collect();
    assert_eq!(all.len(), 5);
    let want = [
        (557.142_857_142_857, 0.0),
        (604.761_904_761_904_8, 0.0),
        (620.0, 0.0),
        (620.0, 32.380_952_380_952_4),
        (620.0, 70.476_190_476_190_5),
    ];
    let mut got = all.clone();
    got.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
    for (g, w) in got.iter().zip(want) {
        assert!(
            (g.0 - w.0).abs() < 1e-6 && (g.1 - w.1).abs() < 1e-6,
            "{g:?} ≠ {w:?}"
        );
    }
    assert!(f2.iter().any(|f| (f.1 - 32.380_952_380_952_4).abs() < 1e-6));
    assert!(f1
        .iter()
        .any(|f| (f.0 - 604.761_904_761_904_8).abs() < 1e-6));
    // Côté droit du contour anti-horaire = extérieur : sous le bas (y = −5), à droite du côté
    // droit (x = 625).
    let tips: Vec<(f64, f64)> = sh
        .notches
        .iter()
        .filter(|n| n.entity == b2 && foot_tip(&p, s, n.id).0 .1 > 1.0)
        .map(|n| foot_tip(&p, s, n.id).1)
        .collect();
    assert!(tips.iter().all(|t| close(t.0, 625.0)), "{tips:?}");
    let tips: Vec<(f64, f64)> = sh
        .notches
        .iter()
        .filter(|n| n.entity == b1 && foot_tip(&p, s, n.id).0 .0 < 619.0)
        .map(|n| foot_tip(&p, s, n.id).1)
        .collect();
    assert!(tips.iter().all(|t| close(t.1, -5.0)), "{tips:?}");
}

#[test]
fn ca3_sens_b_vers_a() {
    let mut r = rig();
    // Cran sur B à 97,5 mm du début de B (contour anti-horaire : x = 597,5) → abscisse de couture
    // 97,5 × 200 / 195 = 100 ; sens opposés : 200 − 100 = 100 depuis le début de A → x = 100.
    put(&mut r.p, r.s, r.b_edge, (500.0, 0.0), 97.5, Side::Left);
    r.p.apply(march(r.pair, SeamWhich::B)).unwrap();
    let n = fresh_on(&r.p, r.s, &[r.a_edge], &[])[0];
    let (foot, tip) = foot_tip(&r.p, r.s, n);
    assert!(close(foot.0, 100.0) && close(foot.1, 0.0), "{foot:?}");
    assert!(close(tip.1, 5.0), "intérieur de A : {tip:?}");
}

// ======================================================================== CA4

/// Bézier cubique, longueur par subdivision fine (oracle indépendant du code de longueur).
struct Bez([(f64, f64); 4]);

impl Bez {
    fn at(&self, t: f64) -> (f64, f64) {
        let u = 1.0 - t;
        let c = |f: fn(&(f64, f64)) -> f64| {
            u * u * u * f(&self.0[0])
                + 3.0 * u * u * t * f(&self.0[1])
                + 3.0 * u * t * t * f(&self.0[2])
                + t * t * t * f(&self.0[3])
        };
        (c(|p| p.0), c(|p| p.1))
    }
    /// Polyligne cumulée (longueur, point) sur 200 000 pas.
    fn table(&self) -> Vec<(f64, (f64, f64))> {
        let n = 200_000;
        let mut out = vec![(0.0, self.at(0.0))];
        for i in 1..=n {
            let q = self.at(i as f64 / n as f64);
            let (l, p) = *out.last().unwrap();
            out.push((l + ((q.0 - p.0).powi(2) + (q.1 - p.1).powi(2)).sqrt(), q));
        }
        out
    }
}

fn point_at_length(table: &[(f64, (f64, f64))], d: f64) -> (f64, f64) {
    let i = table
        .partition_point(|(l, _)| *l < d)
        .clamp(1, table.len() - 1);
    let ((l0, p0), (l1, p1)) = (table[i - 1], table[i]);
    let f = if l1 > l0 { (d - l0) / (l1 - l0) } else { 0.0 };
    (p0.0 + (p1.0 - p0.0) * f, p0.1 + (p1.1 - p0.1) * f)
}

/// Pièce « D » : courbe de Bézier de (x0, 0) à (x0 + 100, 0) fermée par sa corde.
fn d_piece(
    p: &mut Project,
    s: SheetId,
    x0: f64,
    c1: (f64, f64),
    c2: (f64, f64),
    name: &str,
) -> (PieceId, EntityId, Bez) {
    let curve = p
        .apply(Command::AddCurve {
            sheet: s,
            start: PointRef::New(pt(x0, 0.0)),
            c1: pt(x0 + c1.0, c1.1),
            c2: pt(x0 + c2.0, c2.1),
            end: PointRef::New(pt(x0 + 100.0, 0.0)),
        })
        .unwrap()
        .entities[0];
    let chord = p
        .apply(Command::AddSegment {
            sheet: s,
            from: PointRef::New(pt(x0 + 100.0, 0.0)),
            to: PointRef::New(pt(x0, 0.0)),
        })
        .unwrap()
        .entities[0];
    let piece = p
        .apply(Command::ExtractPiece {
            sheet: s,
            source: PieceSource::Paths {
                entities: vec![curve, chord],
            },
            name: name.into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    let bez = Bez([
        (x0, 0.0),
        (x0 + c1.0, c1.1),
        (x0 + c2.0, c2.1),
        (x0 + 100.0, 0.0),
    ]);
    (piece, curve, bez)
}

#[test]
fn ca4_abscisse_curviligne_sur_courbes_de_bezier_oracle_numerique() {
    let mut p = Project::new("courbes");
    let s = p.sheets[0].id;
    let (pa, ca, ba) = d_piece(&mut p, s, 0.0, (30.0, 40.0), (70.0, 40.0), "A");
    let (pb, cb, bb) = d_piece(&mut p, s, 400.0, (20.0, 70.0), (80.0, 70.0), "B");
    let (ta, tb) = (ba.table(), bb.table());
    let (la, lb) = (ta.last().unwrap().0, tb.last().unwrap().0);
    assert!(lb > la + 5.0, "B plus longue : {la} / {lb}");
    let (sa, sb) = (side(&p, pa, &[ca]), side(&p, pb, &[cb]));
    let id = pair(&mut p, sa, sb);
    // Crans de A aux longueurs développées 0,2 / 0,5 / 0,9 × LA depuis le départ du tracé.
    let fr = [0.2, 0.5, 0.9];
    for f in fr {
        p.apply(Command::AddNotch {
            sheet: s,
            entity: ca,
            distance_mm: f * la,
            side: Side::Left,
            length_mm: 4.0,
            style: NotchStyle::Single,
        })
        .unwrap();
    }
    p.apply(march(id, SeamWhich::A)).unwrap();
    let sh = p.sheet(s).unwrap();
    let mut got: Vec<(f64, NotchId)> = sh
        .notches
        .iter()
        .filter(|n| n.entity == cb)
        .map(|n| (sh.notch_distance_mm(n).unwrap(), n.id))
        .collect();
    got.sort_by(|a, b| a.0.total_cmp(&b.0));
    assert_eq!(got.len(), 3);
    // Les deux courbes sont parcourues à rebours dans le contour anti-horaire (courbe vers le haut
    // puis corde de retour) : abscisse de couture s = LA − u (u depuis le départ du tracé) ;
    // cible depuis le début de B : s × LB / LA ; sens opposés : position = LB − s × LB / LA ;
    // tracé de B à rebours : u_B = LB − position = s × LB / LA = (1 − f) × LB pour u = f × LA.
    for ((d, id), f) in got.iter().zip(fr.iter().rev()) {
        let expect_u = (1.0 - f) * lb;
        assert!((d - expect_u).abs() < 1e-4, "u_B {d} ≠ {expect_u}");
        // Point du plan : sur la courbe de B à cette longueur développée (oracle par subdivision).
        let want = point_at_length(&tb, expect_u);
        let foot = foot_tip(&p, s, *id).0;
        assert!(
            (foot.0 - want.0).abs() < 1e-3 && (foot.1 - want.1).abs() < 1e-3,
            "{foot:?} ≠ {want:?}"
        );
    }
}

// ======================================================================== CA5

#[test]
fn ca5_vrais_crans_forme_style_longueur_et_coupe_copies_non_ancres() {
    let mut r = rig();
    let n = put(&mut r.p, r.s, r.a_edge, (0.0, 0.0), 80.0, Side::Left);
    r.p.apply(Command::SetNotchShape {
        sheet: r.s,
        notch: n,
        form: NotchForm::T,
        width_mm: Some(4.0),
        on_cut: true,
    })
    .unwrap();
    // Cran ancré (milieu) : reporté en position fixe.
    let m =
        r.p.apply(Command::AddAnchoredNotch {
            sheet: r.s,
            entity: r.a_edge,
            anchor: NotchAnchor::Midpoint {},
            side: if r.p.sheet(r.s).unwrap().segment_ends(r.a_edge).unwrap().0.x < 100.0 {
                Side::Right
            } else {
                Side::Left
            },
            length_mm: 6.0,
            style: NotchStyle::Double,
        })
        .unwrap()
        .notches[0];
    let o = r.p.apply(march(r.pair, SeamWhich::A)).unwrap();
    assert_eq!(o.notches.len(), 2);
    let sh = r.p.sheet(r.s).unwrap();
    let src_t = sh.notch(n).unwrap().clone();
    let src_m = sh.notch(m).unwrap().clone();
    for id in &o.notches {
        let t = sh.notch(*id).unwrap();
        assert_eq!(t.entity, r.b_edge);
        assert!(t.anchor.is_none(), "position fixe, jamais ancrée");
        let model = if t.form == NotchForm::T {
            &src_t
        } else {
            &src_m
        };
        let same = Notch {
            id: t.id,
            entity: t.entity,
            distance_mm: t.distance_mm,
            side: t.side,
            anchor: None,
            ..model.clone()
        };
        assert_eq!(*t, same, "forme, largeur, style, longueur, coupe copiés");
    }
    let tee = sh.notch(o.notches[0]).unwrap();
    assert!(tee.form == NotchForm::T && tee.width_mm == Some(4.0) && tee.on_cut);
    // Milieu de A (100) → 97,5 de B (sens opposés : 195 − 97,5 = 97,5) ; côté droit (extérieur).
    let mid = sh.notch(o.notches[1]).unwrap();
    assert_eq!(mid.style, NotchStyle::Double);
    let foot = foot_tip(&r.p, r.s, mid.id);
    // Cran double : premier trait à 597,5 - 1,5 (écart de 3 mm entre les deux traits).
    assert!(close(foot.0 .0, 596.0), "{foot:?}");
    assert!(foot.1 .1 < 0.0, "extérieur : {foot:?}");
    // Non liés : supprimer le cran source ne touche pas le cran reporté ni le couple.
    let seams = r.p.seams.clone();
    r.p.apply(Command::DeleteNotch {
        sheet: r.s,
        notch: n,
    })
    .unwrap();
    assert!(r.p.sheet(r.s).unwrap().notch(o.notches[0]).is_ok());
    assert_eq!(r.p.seams, seams, "aucune correspondance mémorisée");
}

// ======================================================================== CA6

#[test]
fn ca6_selection_par_liste_et_refus_sans_trace() {
    let mut r = rig();
    let n1 = put(&mut r.p, r.s, r.a_edge, (0.0, 0.0), 50.0, Side::Left);
    let n2 = put(&mut r.p, r.s, r.a_edge, (0.0, 0.0), 100.0, Side::Left);
    // Cran du haut de R1 : hors du côté source.
    let top = edge(&r.p, r.s, (0.0, 50.0), (200.0, 50.0));
    let foreign = put(&mut r.p, r.s, top, (0.0, 50.0), 20.0, Side::Left);
    // « Tous » ignore le cran du haut ; la liste le refuse.
    let c = |which: Option<Vec<NotchId>>| Command::MarchNotches {
        pair: r.pair,
        from: SeamWhich::A,
        which,
    };
    let e = refused(&mut r.p, c(Some(vec![n1, foreign])));
    assert!(e.contains("hors du côté source"), "{e}");
    let e = refused(&mut r.p, c(Some(vec![NotchId(999_999)])));
    assert!(e.contains("hors du côté source"), "{e}");
    let e = refused(&mut r.p, c(Some(vec![])));
    assert!(e.contains("aucun cran"), "{e}");
    // Couple inconnu.
    refused(&mut r.p, march(SeamPairId(999_999), SeamWhich::A));
    // Aucun cran sur B (source) : refus.
    let e = refused(&mut r.p, march(r.pair, SeamWhich::B));
    assert!(e.contains("aucun cran"), "{e}");
    // Liste : seul n2 est reporté (x = 695 − 97,5 = 597,5), un identifiant répété compte une fois.
    let o = r.p.apply(c(Some(vec![n2, n2]))).unwrap();
    assert_eq!(o.notches.len(), 1);
    let foot = foot_tip(&r.p, r.s, o.notches[0]).0;
    assert!(close(foot.0, 597.5), "{foot:?}");
    // « Tous » : n1 est reporté ; n2 déjà présent (remarque) ; le cran du haut reste ignoré.
    let o = r.p.apply(c(None)).unwrap();
    assert_eq!(o.notches.len(), 1);
    assert!(
        o.notes.iter().any(|t| t.contains("déjà présent")),
        "{:?}",
        o.notes
    );
    let sh = r.p.sheet(r.s).unwrap();
    assert_eq!(
        sh.notches.iter().filter(|n| n.entity == r.b_edge).count(),
        2
    );
    let _ = n1;
}

#[test]
fn ca6_cote_non_calculable_refuse_sans_trace() {
    let mut r = rig();
    put(&mut r.p, r.s, r.a_edge, (0.0, 0.0), 50.0, Side::Left);
    // On ouvre le contour de B (retrait du côté gauche, non cité) : le couple reste déclaré mais
    // le côté B n'est plus calculable.
    let left = edge(&r.p, r.s, (500.0, 0.0), (500.0, 40.0));
    let outline: Vec<PieceEdge> =
        r.p.piece(r.r2)
            .unwrap()
            .outline
            .iter()
            .copied()
            .filter(|e| e.entity != left)
            .collect();
    r.p.apply(Command::SetPieceOutline {
        piece: r.r2,
        outline,
        grainline: None,
    })
    .unwrap();
    assert_eq!(r.p.seams.len(), 1);
    let e = refused(&mut r.p, march(r.pair, SeamWhich::A));
    assert!(
        e.contains("côté cible") && e.contains("non calculable"),
        "{e}"
    );
    assert!(r.p.march_preview(r.pair, SeamWhich::A, None).is_err());
}

// ======================================================================== CA7

#[test]
fn ca7_pas_de_doublon_tolerance_0_01_et_rejeu_refuse() {
    let mut r = rig();
    for d in [50.0, 100.0, 150.0] {
        put(&mut r.p, r.s, r.a_edge, (0.0, 0.0), d, Side::Left);
    }
    // Cibles attendues : x = 646,25 / 597,5 / 548,75. Un cran déjà posé à 0,005 mm de la première
    // (doublon) et un à 0,02 mm de la deuxième (hors tolérance : le report s'ajoute).
    put(
        &mut r.p,
        r.s,
        r.b_edge,
        (500.0, 0.0),
        146.25 + 0.005,
        Side::Right,
    );
    put(
        &mut r.p,
        r.s,
        r.b_edge,
        (500.0, 0.0),
        97.5 + 0.02,
        Side::Left,
    );
    let o = r.p.apply(march(r.pair, SeamWhich::A)).unwrap();
    assert_eq!(o.notches.len(), 2, "un doublon écarté");
    assert!(
        o.notes.iter().any(|t| t.contains("1 cran(s) déjà présent")),
        "{:?}",
        o.notes
    );
    let xs: Vec<f64> = o
        .notches
        .iter()
        .map(|n| foot_tip(&r.p, r.s, *n).0 .0)
        .collect();
    // 646,25 existait déjà à 0,005 mm : non recréé ; 597,5 (0,02 mm) et 548,75 créés.
    assert!(xs.iter().any(|x| close(*x, 597.5)) && xs.iter().any(|x| close(*x, 548.75)));
    assert!(!xs.iter().any(|x| close(*x, 646.25)));
    // Rejeu : tout est présent → refus sans trace (aucune étape).
    let e = refused(&mut r.p, march(r.pair, SeamWhich::A));
    assert!(e.contains("existent déjà"), "{e}");
}

#[test]
fn ca7_deux_crans_sources_confondus_ne_donnent_qu_un_cran() {
    let mut r = rig();
    put(&mut r.p, r.s, r.a_edge, (0.0, 0.0), 50.0, Side::Left);
    put(&mut r.p, r.s, r.a_edge, (0.0, 0.0), 50.004, Side::Right);
    let v = r.p.march_preview(r.pair, SeamWhich::A, None).unwrap();
    assert_eq!(v.items.iter().filter(|i| i.duplicate).count(), 1);
    let o = r.p.apply(march(r.pair, SeamWhich::A)).unwrap();
    assert_eq!(o.notches.len(), 1);
}

// ======================================================================== CA8

#[test]
fn ca8_une_seule_etape_annulable_et_aucune_pour_un_refus() {
    let mut r = rig();
    for d in [50.0, 100.0, 150.0] {
        put(&mut r.p, r.s, r.a_edge, (0.0, 0.0), d, Side::Left);
    }
    let mut d = Document::new(r.p.clone());
    let start = bytes(d.project());
    let o = d.execute(march(r.pair, SeamWhich::A)).unwrap();
    assert_eq!(o.notches.len(), 3);
    let after = bytes(d.project());
    assert_ne!(after, start);
    assert!(d.undo());
    assert_eq!(bytes(d.project()), start, "trois crans retirés d'un coup");
    assert!(!d.can_undo(), "une seule étape");
    assert!(d.redo());
    assert_eq!(bytes(d.project()), after);
    // Refus (rejeu) : aucune étape de plus.
    assert!(d.execute(march(r.pair, SeamWhich::A)).is_err());
    assert!(d.undo());
    assert!(!d.can_undo());
}

// ======================================================================== CA9

#[test]
fn ca9_apercu_sans_modifier_le_projet() {
    let mut r = rig();
    for d in [50.0, 100.0, 150.0] {
        put(&mut r.p, r.s, r.a_edge, (0.0, 0.0), d, Side::Left);
    }
    let before = bytes(&r.p);
    let v = r.p.march_preview(r.pair, SeamWhich::A, None).unwrap();
    assert_eq!(bytes(&r.p), before);
    assert_eq!(v.items.len(), 3);
    let xs: Vec<f64> = v.items.iter().map(|i| i.point.x).collect();
    for (x, e) in xs.iter().zip([646.25, 597.5, 548.75]) {
        assert!(close(*x, e), "{xs:?}");
    }
    let t: Vec<f64> = v.items.iter().map(|i| i.target_mm).collect();
    for (x, e) in t.iter().zip([146.25, 97.5, 48.75]) {
        assert!(close(*x, e), "{t:?}");
    }
    assert!(v.items.iter().all(|i| !i.duplicate && i.entity == r.b_edge));
}

// ======================================================================== CA10

#[test]
fn ca10_aller_retour_json_aucune_donnee_nouvelle_et_pf_validate() {
    let mut r = rig();
    for d in [50.0, 100.0, 150.0] {
        put(&mut r.p, r.s, r.a_edge, (0.0, 0.0), d, Side::Left);
    }
    let before: Value = serde_json::from_str(&bytes(&r.p)).unwrap();
    r.p.apply(march(r.pair, SeamWhich::A)).unwrap();
    let text = bytes(&r.p);
    let after: Value = serde_json::from_str(&text).unwrap();
    // Aucun champ nouveau : mêmes clés de projet, mêmes couples ; seuls des crans ordinaires.
    let keys = |v: &Value| -> Vec<String> { v.as_object().unwrap().keys().cloned().collect() };
    assert_eq!(keys(&before), keys(&after));
    assert_eq!(before["seams"], after["seams"]);
    let nk = |v: &Value| -> Vec<String> {
        let mut k: Vec<String> = v["sheets"][0]["notches"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|n| n.as_object().unwrap().keys().cloned())
            .collect();
        k.sort();
        k.dedup();
        k
    };
    assert_eq!(nk(&before), nk(&after));
    assert_eq!(after["sheets"][0]["notches"].as_array().unwrap().len(), 6);
    let back = persist::from_json(&text).unwrap();
    assert_eq!(back, r.p);
    assert_eq!(bytes(&back), text);
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "pf-marche-validate-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("marche.pforge.json");
    persist::save(&path, &r.p).unwrap();
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_pf-validate"))
        .arg(&path)
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn ca10_la_commande_se_serialise_et_se_relit() {
    let c = Command::MarchNotches {
        pair: SeamPairId(7),
        from: SeamWhich::B,
        which: Some(vec![NotchId(3)]),
    };
    let t = serde_json::to_string(&c).unwrap();
    assert!(t.contains("march_notches") && t.contains("\"b\""), "{t}");
    assert_eq!(serde_json::from_str::<Command>(&t).unwrap(), c);
}
