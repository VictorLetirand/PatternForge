//! US-08.4 — contrôle des crans. Valeurs attendues calculées à la main dans chaque test (abscisse
//! attendue = abscisse × L cible / L source, comptée depuis la fin de B pour des contours de même
//! sens). C1 manquant, C2 décalé, C3 tolérance (limite incluse), C4 sens de couture, C5 un-pour-un,
//! C6 non calculable, C7 doublons et crans doubles, C8 lecture sans modification et en direct,
//! C9 cohérence avec la marche, C10 contour entier.

use pf_core::persist;
use pf_core::*;
use pf_geometry::{PointMm, Side};

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

fn side(p: &Project, piece: PieceId, e: EntityId) -> SeamSide {
    p.seam_side_range(piece, e, 1).unwrap()
}

/// Cran posé à `d` mm du point `from` (plan) le long d'un segment, quel que soit son sens
/// d'enregistrement.
fn put(p: &mut Project, s: SheetId, e: EntityId, from: (f64, f64), d: f64) -> NotchId {
    let sh = p.sheet(s).unwrap();
    let (u, _) = sh.segment_ends(e).unwrap();
    let len = sh.path_length_mm(e).unwrap();
    let from_start = (u.x - from.0).abs() < 1e-9 && (u.y - from.1).abs() < 1e-9;
    p.apply(Command::AddNotch {
        sheet: s,
        entity: e,
        distance_mm: if from_start { d } else { len - d },
        side: if from_start { Side::Left } else { Side::Right },
        length_mm: 5.0,
        style: NotchStyle::Single,
    })
    .unwrap()
    .notches[0]
}

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

/// A = bas de R1 (`la` mm), B = bas de R2 (`lb` mm) ; contours anti-horaires : abscisse sur A = x,
/// abscisse sur B = x − 500.
struct Rig {
    p: Project,
    s: SheetId,
    r2: PieceId,
    a: EntityId,
    b: EntityId,
    pair: SeamPairId,
}

fn rig(la: f64, lb: f64) -> Rig {
    let mut p = Project::new("crans");
    let s = p.sheets[0].id;
    let r1 = rect_piece(&mut p, s, 0.0, 0.0, la, 50.0, "Devant");
    let r2 = rect_piece(&mut p, s, 500.0, 0.0, lb, 40.0, "Dos");
    let a = edge(&p, s, (0.0, 0.0), (la, 0.0));
    let b = edge(&p, s, (500.0, 0.0), (500.0 + lb, 0.0));
    let (sa, sb) = (side(&p, r1, a), side(&p, r2, b));
    let pair = p
        .apply(Command::AddSeamPair {
            a: sa,
            b: sb,
            embu_mm: None,
            tolerance_mm: None,
        })
        .unwrap()
        .seam_pairs[0];
    Rig {
        p,
        s,
        r2,
        a,
        b,
        pair,
    }
}

impl Rig {
    fn na(&mut self, d: f64) -> NotchId {
        put(&mut self.p, self.s, self.a, (0.0, 0.0), d)
    }
    fn nb(&mut self, d: f64) -> NotchId {
        put(&mut self.p, self.s, self.b, (500.0, 0.0), d)
    }
    fn read(&self) -> NotchCheckReading {
        self.p.notch_check_report().remove(0)
    }
}

fn item(r: &NotchCheckReading, n: NotchId) -> &NotchCheckItem {
    r.items.iter().find(|i| i.notch == n).expect("cran lu")
}

// ====================================================================== C1

#[test]
fn c1_cran_manquant_97_5_a_la_main() {
    let mut r = rig(200.0, 195.0);
    let (a50, a100, a150) = (r.na(50.0), r.na(100.0), r.na(150.0));
    // B (195) : abscisses attendues = 195 − 0,975 × s = 146,25 / 97,5 / 48,75 ; seuls 48,75 et
    // 146,25 existent.
    let (b48, b146) = (r.nb(48.75), r.nb(146.25));
    let rd = r.read();
    assert!(rd.not_computable.is_none() && rd.items.len() == 5);
    assert_eq!((rd.count(SeamWhich::A), rd.count(SeamWhich::B)), (3, 2));
    let i = item(&rd, a100);
    assert_eq!(i.status, NotchMatch::Missing);
    assert!(close(i.expected_mm, 97.5) && close(i.position_mm, 100.0));
    assert!(i.partner.is_none() && i.found_mm.is_none() && i.gap_mm.is_none());
    assert_eq!(rd.missing(SeamWhich::A), vec![a100]);
    assert!(rd.missing(SeamWhich::B).is_empty());
    for (a, b, e) in [(a50, b146, 146.25), (a150, b48, 48.75)] {
        let i = item(&rd, a);
        assert_eq!(i.status, NotchMatch::Matched);
        assert_eq!(i.partner, Some(b));
        assert!(close(i.expected_mm, e) && close(i.found_mm.unwrap(), e));
        assert!(close(i.gap_mm.unwrap(), 0.0));
        let j = item(&rd, b);
        assert_eq!(j.status, NotchMatch::Matched);
        assert_eq!(j.partner, Some(a));
    }
    assert_eq!(rd.count_status(NotchMatch::Missing), 1);
}

#[test]
fn c1_cran_de_b_sans_vis_a_vis_en_a() {
    let mut r = rig(200.0, 195.0);
    let a100 = r.na(100.0);
    let (b97, b30) = (r.nb(97.5), r.nb(30.0));
    // B30 → attendu sur A : 200 − 30 × 200 / 195 = 169,23… ; aucun cran de A à moins de 10 mm.
    let rd = r.read();
    assert_eq!(item(&rd, a100).status, NotchMatch::Matched);
    assert_eq!(item(&rd, b97).status, NotchMatch::Matched);
    let j = item(&rd, b30);
    assert_eq!(j.status, NotchMatch::Missing);
    assert!(close(j.expected_mm, 200.0 - 30.0 * 200.0 / 195.0));
    assert_eq!(rd.missing(SeamWhich::B), vec![b30]);
}

// ====================================================================== C2

#[test]
fn c2_cran_decale_de_2_5_mm() {
    let mut r = rig(200.0, 195.0);
    let a100 = r.na(100.0);
    // Attendu à 97,5 ; posé à 100 : écart 2,5 mm sur B (et 2,5 × 200 / 195 sur A).
    let b100 = r.nb(100.0);
    let rd = r.read();
    let i = item(&rd, a100);
    assert_eq!(i.status, NotchMatch::Shifted);
    assert_eq!(i.partner, Some(b100));
    assert!(close(i.expected_mm, 97.5) && close(i.found_mm.unwrap(), 100.0));
    assert!(close(i.gap_mm.unwrap(), 2.5));
    let j = item(&rd, b100);
    assert_eq!(j.status, NotchMatch::Shifted);
    assert!(close(j.expected_mm, 200.0 - 100.0 * 200.0 / 195.0));
    assert!(close(j.gap_mm.unwrap(), 2.5 * 200.0 / 195.0));
    assert!(rd.missing(SeamWhich::A).is_empty() && rd.missing(SeamWhich::B).is_empty());
    assert_eq!(rd.count_status(NotchMatch::Shifted), 2);
}

#[test]
fn c2_au_dela_de_la_plage_c_est_manquant_pas_decale() {
    // Sans embu : attendu à 100 ; posé à 110,5 (écart 10,5 > 10) : deux manquants.
    let mut r = rig(200.0, 200.0);
    let a = r.na(100.0);
    let b = r.nb(110.5);
    let rd = r.read();
    // Même sens : attendu = 200 − 100 = 100.
    assert!(close(item(&rd, a).expected_mm, 100.0));
    assert_eq!(item(&rd, a).status, NotchMatch::Missing);
    assert_eq!(item(&rd, b).status, NotchMatch::Missing);
    // À 109,5 (écart 9,5 ≤ 10) : décalé.
    let mut q = rig(200.0, 200.0);
    let a = q.na(100.0);
    q.nb(109.5);
    assert_eq!(item(&q.read(), a).status, NotchMatch::Shifted);
}

// ====================================================================== C3

#[test]
fn c3_tolerance_de_cran_limite_incluse() {
    // Sans embu : A à 100 → B attendu à 100 ; B à 101 : écart 1,0 exactement.
    let mut r = rig(200.0, 200.0);
    let a = r.na(100.0);
    r.nb(101.0);
    let rd = r.read();
    assert!(close(rd.tolerance_mm, 1.0) && close(NOTCH_CHECK_TOLERANCE_MM, 1.0));
    assert_eq!(item(&rd, a).status, NotchMatch::Matched, "limite incluse");
    let tight = r.p.notch_check_report_with(0.99).remove(0);
    assert_eq!(item(&tight, a).status, NotchMatch::Shifted);
    let wide = r.p.notch_check_report_with(5.0).remove(0);
    assert_eq!(item(&wide, a).status, NotchMatch::Matched);
    for bad in [f64::NAN, -1.0, f64::INFINITY] {
        let d = r.p.notch_check_report_with(bad).remove(0);
        assert!(close(d.tolerance_mm, 1.0));
    }
    let zero = r.p.notch_check_report_with(0.0).remove(0);
    assert_eq!(item(&zero, a).status, NotchMatch::Shifted);
}

// ====================================================================== C4

#[test]
fn c4_sens_de_couture_oppose_pour_contours_de_meme_sens() {
    let mut r = rig(200.0, 200.0);
    let a = r.na(30.0);
    // Même sens (anti-horaire) : début de A contre FIN de B → attendu à 170 depuis le début de B.
    let b_same = r.nb(30.0);
    let rd = r.read();
    assert!(close(item(&rd, a).expected_mm, 170.0));
    // 30 contre 170 : écart 140, hors plage → manquants des deux côtés.
    assert_eq!(item(&rd, a).status, NotchMatch::Missing);
    assert_eq!(item(&rd, b_same).status, NotchMatch::Missing);
    let b_ok = r.nb(170.0);
    let rd = r.read();
    assert_eq!(item(&rd, a).partner, Some(b_ok));
    assert_eq!(item(&rd, a).status, NotchMatch::Matched);
    assert_eq!(item(&rd, b_same).status, NotchMatch::Missing);
    // B dans le sens horaire (mêmes tracés) : sens différents → même sens de couture : le début
    // de B est à x = 700 ; attendu = 30 depuis ce début, soit x = 670, abscisse 30 : le cran posé à
    // 170 mm de x = 500 est à x = 670, donc à 30 du début.
    make_clockwise(&mut r.p, r.r2);
    let rd = r.read();
    assert!(close(item(&rd, a).expected_mm, 30.0));
    assert_eq!(item(&rd, a).partner, Some(b_ok));
    assert_eq!(item(&rd, a).status, NotchMatch::Matched);
    assert!(close(item(&rd, b_ok).position_mm, 30.0));
}

// ====================================================================== C5

#[test]
fn c5_appariement_un_pour_un_par_ecart_croissant() {
    let mut r = rig(200.0, 200.0);
    let (a100, a100_5) = (r.na(100.0), r.na(100.5));
    // Un seul cran de B, à 100 (attendu pour a100 ; a100_5 le voit à 0,5).
    let b = r.nb(100.0);
    let rd = r.read();
    assert_eq!(item(&rd, a100).partner, Some(b));
    assert_eq!(item(&rd, a100).status, NotchMatch::Matched);
    assert_eq!(
        item(&rd, a100_5).status,
        NotchMatch::Missing,
        "B ne sert qu'une fois"
    );
    assert_eq!(item(&rd, b).partner, Some(a100));
    assert_eq!(rd.missing(SeamWhich::A), vec![a100_5]);
    // Un cran de B à 99,7 : A 100 attend 100 (écart 0,3), A 100,5 attend 99,5 (écart 0,2) →
    // c'est ce dernier qui gagne ; A 100 reste sans vis-à-vis.
    let mut q = rig(200.0, 200.0);
    let (x, y) = (q.na(100.0), q.na(100.5));
    let z = q.nb(99.7);
    let d = q.read();
    assert_eq!(item(&d, y).partner, Some(z));
    assert_eq!(item(&d, x).status, NotchMatch::Missing);
    let count = |s: SeamWhich| {
        d.items
            .iter()
            .filter(|i| i.side == s && i.partner.is_some())
            .count()
    };
    assert_eq!((count(SeamWhich::A), count(SeamWhich::B)), (1, 1));
}

// ====================================================================== C6

#[test]
fn c6_couple_non_calculable_avec_raison() {
    let mut r = rig(200.0, 200.0);
    r.na(100.0);
    r.nb(100.0);
    // Contour de R2 ouvert : un côté du rectangle, non cité par le couple, retiré.
    let b = r.b;
    let keep: Vec<PieceEdge> =
        r.p.piece(r.r2)
            .unwrap()
            .outline
            .iter()
            .copied()
            .enumerate()
            .filter(|(i, e)| e.entity == b || *i != 0 && *i != 1)
            .map(|x| x.1)
            .collect();
    r.p.apply(Command::SetPieceOutline {
        piece: r.r2,
        outline: keep,
        grainline: None,
    })
    .unwrap();
    let rd = r.read();
    let (side, reason) = rd.not_computable.expect("non calculable");
    assert_eq!(side, SeamWhich::B);
    assert!(rd.items.is_empty());
    // Même verdict que la lecture des coutures (US-08.1).
    let s = r.p.seam_pair_report().remove(0);
    assert_eq!(s.status, SeamStatus::NotComputable { side, reason });
    // Aucun cran sur les deux côtés : calculable, vide.
    let e = rig(200.0, 200.0).read();
    assert!(e.not_computable.is_none() && e.items.is_empty());
    assert!(Project::new("vide").notch_check_report().is_empty());
}

// ====================================================================== C7

#[test]
fn c7_doublons_comptes_une_fois_et_crans_doubles_comptes_chacun() {
    let mut r = rig(200.0, 200.0);
    let a1 = r.na(100.0);
    let a2 = r.na(100.005); // doublon (≤ 0,01 mm)
    let b1 = r.nb(100.0);
    let rd = r.read();
    assert_eq!(rd.count(SeamWhich::A), 1, "doublon compté une fois");
    assert!(rd.items.iter().any(|i| i.notch == a1) && !rd.items.iter().any(|i| i.notch == a2));
    assert_eq!(item(&rd, a1).partner, Some(b1));
    assert_eq!(rd.count_status(NotchMatch::Missing), 0);
    // Crans doubles (écart 5 mm, pas un doublon) : chacun compte et veut son vis-à-vis.
    let mut q = rig(200.0, 200.0);
    let (x, y) = (q.na(100.0), q.na(105.0));
    q.nb(100.0);
    let d = q.read();
    assert_eq!(d.count(SeamWhich::A), 2);
    assert_eq!(item(&d, x).status, NotchMatch::Matched);
    assert_eq!(item(&d, y).status, NotchMatch::Missing);
    // 0,02 mm d'écart : deux crans.
    let mut w = rig(200.0, 200.0);
    w.na(100.0);
    w.na(100.02);
    assert_eq!(w.read().count(SeamWhich::A), 2);
}

// ====================================================================== C8

#[test]
fn c8_lecture_sans_modifier_le_projet_et_en_direct() {
    let mut r = rig(200.0, 195.0);
    let a100 = r.na(100.0);
    let before = bytes(&r.p);
    let _ = r.p.notch_check_report();
    let _ = r.p.notch_check_report_with(3.0);
    assert_eq!(before, bytes(&r.p), "la lecture ne modifie rien");
    assert_eq!(item(&r.read(), a100).status, NotchMatch::Missing);
    let b = r.nb(97.5);
    assert_eq!(item(&r.read(), a100).status, NotchMatch::Matched);
    r.p.apply(Command::DeleteNotch {
        sheet: r.s,
        notch: b,
    })
    .unwrap();
    assert_eq!(item(&r.read(), a100).status, NotchMatch::Missing);
    assert!(!bytes(&r.p).contains("notch_check"));
}

// ====================================================================== C9

#[test]
fn c9_apres_la_marche_des_manquants_plus_aucun_manquant() {
    let mut r = rig(200.0, 195.0);
    for d in [50.0, 100.0, 150.0] {
        r.na(d);
    }
    r.nb(146.25);
    r.nb(20.0); // B sans vis-à-vis côté A
    let rd = r.read();
    let (ma, mb) = (rd.missing(SeamWhich::A), rd.missing(SeamWhich::B));
    assert_eq!((ma.len(), mb.len()), (2, 1));
    let o =
        r.p.apply(Command::Batch {
            commands: vec![
                Command::MarchNotches {
                    pair: r.pair,
                    from: SeamWhich::A,
                    which: Some(ma),
                },
                Command::MarchNotches {
                    pair: r.pair,
                    from: SeamWhich::B,
                    which: Some(mb),
                },
            ],
        })
        .unwrap();
    assert_eq!(o.notches.len(), 3);
    let after = r.read();
    assert_eq!(after.count_status(NotchMatch::Missing), 0);
    assert_eq!(after.count_status(NotchMatch::Shifted), 0);
    assert_eq!(
        (after.count(SeamWhich::A), after.count(SeamWhich::B)),
        (4, 4)
    );
    assert!(after.items.iter().all(|i| i.gap_mm.unwrap() < 1e-9));
}

#[test]
fn c9_marche_avec_b_dans_le_sens_horaire() {
    let mut r = rig(200.0, 195.0);
    make_clockwise(&mut r.p, r.r2);
    for d in [10.0, 77.0, 190.0] {
        r.na(d);
    }
    let ids = r.read().missing(SeamWhich::A);
    assert_eq!(ids.len(), 3);
    r.p.apply(Command::MarchNotches {
        pair: r.pair,
        from: SeamWhich::A,
        which: Some(ids),
    })
    .unwrap();
    assert_eq!(r.read().count_status(NotchMatch::Matched), 6);
}

// ====================================================================== C10

#[test]
fn c10_contour_entier_ecart_circulaire_a_la_jointure() {
    // R1 et R2 identiques (périmètre 300), côté = contour entier (4 tracés).
    let mut p = Project::new("entier");
    let s = p.sheets[0].id;
    let r1 = rect_piece(&mut p, s, 0.0, 0.0, 100.0, 50.0, "A");
    let r2 = rect_piece(&mut p, s, 500.0, 0.0, 100.0, 50.0, "B");
    let (fa, fb) = (
        p.piece(r1).unwrap().outline[0],
        p.piece(r2).unwrap().outline[0],
    );
    let sa = p.seam_side_range(r1, fa.entity, 4).unwrap();
    let sb = p.seam_side_range(r2, fb.entity, 4).unwrap();
    p.apply(Command::AddSeamPair {
        a: sa,
        b: sb,
        embu_mm: None,
        tolerance_mm: None,
    })
    .unwrap();
    let mut add = |e: &PieceEdge, d: f64| {
        p.apply(Command::AddNotch {
            sheet: s,
            entity: e.entity,
            distance_mm: if e.reversed { 100.0 - d } else { d },
            side: Side::Left,
            length_mm: 5.0,
            style: NotchStyle::Single,
        })
        .unwrap()
        .notches[0]
    };
    // Premier tracé de chaque contour : 100 mm (côté long), cran à 0,2 mm du début.
    let na = add(&fa, 0.2);
    let rd0 = p.notch_check_report().remove(0);
    let i = item(&rd0, na);
    assert!(close(i.position_mm, 0.2));
    // Même sens : attendu = 300 − 0,2 = 299,8 ; aucun cran de B : manquant.
    assert!(close(i.expected_mm, 299.8));
    assert_eq!(i.status, NotchMatch::Missing);
    // Cran de B à 0,6 du début : |299,8 − 0,6| = 299,2 → circulaire 300 − 299,2 = 0,8 ≤ 1.
    let nb = {
        let id = p
            .apply(Command::AddNotch {
                sheet: s,
                entity: fb.entity,
                distance_mm: if fb.reversed { 100.0 - 0.6 } else { 0.6 },
                side: Side::Left,
                length_mm: 5.0,
                style: NotchStyle::Single,
            })
            .unwrap();
        id.notches[0]
    };
    let rd = p.notch_check_report().remove(0);
    let i = item(&rd, na);
    assert_eq!(i.partner, Some(nb), "{i:?}");
    assert_eq!(i.status, NotchMatch::Matched);
    assert!(close(i.gap_mm.unwrap(), 0.8), "{:?}", i.gap_mm);
}
