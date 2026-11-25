//! US-07.5 — découpe d'une pièce selon une ligne interne, crans de repère des deux côtés,
//! recollage. Valeurs attendues calculées à la main dans les tests (jamais relues du code testé).
//! CA1 sommets, aires, périmètres, CA2 crans aux mêmes distances, CA3 recoller = pièce d'origine,
//! CA4 refus sans trace, CA5 une étape annulable, CA6 fichiers (aller-retour, ancien format,
//! chargeur strict, `pf-validate`), CA7 pièces dérivées et fiche.

use pf_core::persist;
use pf_core::*;
use pf_geometry::{distance_mm, PointMm, Transform};
use std::path::PathBuf;

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

fn bytes(p: &Project) -> String {
    persist::to_json(p).unwrap()
}

fn setup() -> (Project, SheetId) {
    let p = Project::new("découper");
    let s = p.sheets[0].id;
    (p, s)
}

/// Rectangle `w × h` de coin `(x, y)`, extrait en pièce liée.
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

fn segment(p: &mut Project, s: SheetId, a: PointMm, b: PointMm) -> EntityId {
    p.apply(Command::AddSegment {
        sheet: s,
        from: PointRef::New(a),
        to: PointRef::New(b),
    })
    .unwrap()
    .entities[0]
}

fn bezier(p: &mut Project, s: SheetId, a: [PointMm; 4]) -> EntityId {
    p.apply(Command::AddCurve {
        sheet: s,
        start: PointRef::New(a[0]),
        c1: a[1],
        c2: a[2],
        end: PointRef::New(a[3]),
    })
    .unwrap()
    .entities[0]
}

fn pts(a: PointMm, b: PointMm) -> CutLine {
    CutLine::Points { a, b }
}

fn two() -> CutNotches {
    CutNotches::Count { count: 2 }
}

fn cut(
    p: &mut Project,
    piece: PieceId,
    line: CutLine,
    n: CutNotches,
) -> Result<Outcome, CoreError> {
    p.apply(Command::CutPiece {
        piece,
        line,
        notches: n,
    })
}

fn refused(p: &mut Project, c: Command) -> String {
    let (before, text) = (p.clone(), bytes(p));
    let err = p.apply(c).expect_err("la commande aurait dû être refusée");
    assert_eq!(*p, before, "refus sans trace (valeur)");
    assert_eq!(text, bytes(p), "refus sans trace (octets)");
    err.to_string()
}

fn refused_cut(p: &mut Project, piece: PieceId, line: CutLine, n: CutNotches) -> String {
    refused(
        p,
        Command::CutPiece {
            piece,
            line,
            notches: n,
        },
    )
}

fn ends_of(shape: &Shape) -> (PointMm, PointMm) {
    match shape {
        Shape::Line(a, b) => (*a, *b),
        Shape::Cubic(c) => (c.p0, c.p3),
        Shape::Arc(a) => (a.start_point(), a.end_point()),
        other => panic!("forme inattendue {other:?}"),
    }
}

/// Sommets du contour (début de chaque arête dans le sens du contour).
fn corners(p: &Project, id: PieceId) -> Vec<PointMm> {
    let sheet = p.piece_sheet(id).unwrap();
    p.piece(id)
        .unwrap()
        .outline
        .iter()
        .map(|e| {
            let (a, b) = ends_of(&sheet.shape(e.entity).unwrap());
            if e.reversed {
                b
            } else {
                a
            }
        })
        .collect()
}

fn has_corner(c: &[PointMm], at: PointMm) -> bool {
    c.iter().any(|v| distance_mm(*v, at) < 1e-9)
}

fn area(p: &Project, id: PieceId) -> f64 {
    p.piece_contour(id).unwrap().area_mm2
}

fn perimeter(p: &Project, id: PieceId) -> f64 {
    p.piece_contour(id).unwrap().perimeter_mm
}

/// Dernier tracé du contour : l'arête de découpe d'un morceau.
fn cut_edge(p: &Project, half: PieceId) -> PieceEdge {
    *p.piece(half).unwrap().outline.last().unwrap()
}

/// Crans de l'arête de découpe de `half` : (distance lue, point sur le tracé, côté).
fn marks(p: &Project, half: PieceId) -> Vec<(f64, PointMm, pf_geometry::Side)> {
    let sheet = p.piece_sheet(half).unwrap();
    let edge = cut_edge(p, half).entity;
    let mut v: Vec<_> = sheet
        .notches
        .iter()
        .filter(|n| n.entity == edge)
        .map(|n| {
            let d = sheet.notch_distance_mm(n).unwrap();
            (d, sheet.path_point_at(edge, d).unwrap().0, n.side)
        })
        .collect();
    v.sort_by(|a, b| a.0.total_cmp(&b.0));
    v
}

/// Rectangle 200 × 100 coupé par la ligne (40, -20) → (140, 120) : pente 1,4, coupe y = 0 en
/// x0 = 40 + 20 / 1,4 et y = 100 en x1 = 40 + 120 / 1,4.
fn oblique() -> (Project, PieceId, Outcome) {
    let (mut p, s) = setup();
    let m = rect_piece(&mut p, s, 0.0, 0.0, 200.0, 100.0, "Devant");
    let o = cut(&mut p, m, pts(pt(40.0, -20.0), pt(140.0, 120.0)), two()).unwrap();
    (p, m, o)
}

const X0: f64 = 40.0 + 20.0 / 1.4;
const X1: f64 = 40.0 + 120.0 / 1.4;

fn cut_len_oblique() -> f64 {
    (X1 - X0).hypot(100.0)
}

// ======================================================================== CA1

#[test]
fn ca1_rectangle_ligne_oblique_sommets_aires_perimetres() {
    let (p, _, o) = oblique();
    assert_eq!(o.pieces.len(), 2);
    let (l, r) = (o.pieces[0], o.pieces[1]);
    // Gauche de P -> Q (P = (X0, 0), vers le haut) : x petit, quadrilatère (0,0) P Q (0,100).
    let cl = corners(&p, l);
    assert_eq!(cl.len(), 4);
    for c in [pt(0.0, 0.0), pt(X0, 0.0), pt(X1, 100.0), pt(0.0, 100.0)] {
        assert!(has_corner(&cl, c), "{c:?} absent de {cl:?}");
    }
    let cr = corners(&p, r);
    assert_eq!(cr.len(), 4);
    for c in [pt(X0, 0.0), pt(200.0, 0.0), pt(200.0, 100.0), pt(X1, 100.0)] {
        assert!(has_corner(&cr, c), "{c:?} absent de {cr:?}");
    }
    // Aires : trapèze 100 * (X0 + X1) / 2 = 9000 et le reste ; somme = 20000 à 1e-6.
    assert!((area(&p, l) - 9000.0).abs() < 1e-6);
    assert!((area(&p, r) - 11000.0).abs() < 1e-6);
    assert!((area(&p, l) + area(&p, r) - 20000.0).abs() < 1e-6);
    // Périmètres : bords du rectangle + la ligne de découpe, comptée dans chaque morceau.
    let len = cut_len_oblique();
    assert!((perimeter(&p, l) - (X0 + X1 + 100.0 + len)).abs() < 1e-6);
    assert!((perimeter(&p, r) - ((200.0 - X0) + (200.0 - X1) + 100.0 + len)).abs() < 1e-6);
    // Les deux bords de découpe ont la même longueur.
    let sheet_l = p.piece_sheet(l).unwrap();
    let sheet_r = p.piece_sheet(r).unwrap();
    let (el, er) = (cut_edge(&p, l).entity, cut_edge(&p, r).entity);
    let (ll, lr) = (
        sheet_l.path_length_mm(el).unwrap(),
        sheet_r.path_length_mm(er).unwrap(),
    );
    assert!((ll - len).abs() < 1e-9 && (lr - len).abs() < 1e-9);
    // Noms, codes, pièces détachées sur des feuilles dédiées, construction d'origine intacte.
    assert_eq!(p.piece(l).unwrap().name, "Devant (1/2)");
    assert_eq!(p.piece(r).unwrap().name, "Devant (2/2)");
    assert_ne!(sheet_l.id, sheet_r.id);
    assert_ne!(sheet_l.id, p.sheets[0].id);
}

#[test]
fn ca1_ligne_qui_depasse_ou_qui_s_arrete_sur_le_contour() {
    // Mêmes morceaux que la ligne longue : une ligne dont les extrémités sont exactement sur le
    // contour (X0, 0) -> (X1, 100) donne le même résultat.
    let (mut p, s) = setup();
    let m = rect_piece(&mut p, s, 0.0, 0.0, 200.0, 100.0, "Devant");
    let o = cut(&mut p, m, pts(pt(X0, 0.0), pt(X1, 100.0)), two()).unwrap();
    assert!((area(&p, o.pieces[0]) - 9000.0).abs() < 1e-6);
    assert!((area(&p, o.pieces[1]) - 11000.0).abs() < 1e-6);
}

#[test]
fn ca1_tracé_existant_segment_ligne_interne() {
    // Segment (-20, 10) -> (220, 70) de pente 1/4 : traverse x = 0 en y = 15 et x = 200 en y = 65.
    let (mut p, s) = setup();
    let m = rect_piece(&mut p, s, 0.0, 0.0, 200.0, 100.0, "Devant");
    let line = segment(&mut p, s, pt(-20.0, 10.0), pt(220.0, 70.0));
    let o = cut(&mut p, m, CutLine::Path { entity: line }, two()).unwrap();
    // Gauche de P -> Q (vers la droite) = au-dessus : 200 * (100 - (15 + 65) / 2) = 12000.
    assert!((area(&p, o.pieces[0]) - 12000.0).abs() < 1e-6);
    assert!((area(&p, o.pieces[1]) - 8000.0).abs() < 1e-6);
    let len = 200.0_f64.hypot(50.0);
    for id in &o.pieces {
        let cl = corners(&p, *id);
        assert!(has_corner(&cl, pt(0.0, 15.0)) && has_corner(&cl, pt(200.0, 65.0)));
    }
    // Le tracé d'origine reste sur la feuille de départ.
    assert!(p.sheets[0].entity(line).is_ok());
    let (m0, _) = (marks(&p, o.pieces[0]), len);
    assert_eq!(m0.len(), 2);
}

/// Bézier (100,-20) (40,40) (160,60) (100,120) : x(t), y(t), dérivées, longueur et aire par la
/// règle de Simpson (oracle indépendant de la bibliothèque).
struct Curve;

impl Curve {
    const P: [(f64, f64); 4] = [(100.0, -20.0), (40.0, 40.0), (160.0, 60.0), (100.0, 120.0)];

    fn at(t: f64) -> (f64, f64) {
        let u = 1.0 - t;
        let w = [u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t];
        (
            (0..4).map(|i| w[i] * Self::P[i].0).sum(),
            (0..4).map(|i| w[i] * Self::P[i].1).sum(),
        )
    }

    fn d(t: f64) -> (f64, f64) {
        let u = 1.0 - t;
        let w = [u * u, 2.0 * u * t, t * t];
        let dx: f64 = (0..3)
            .map(|i| 3.0 * w[i] * (Self::P[i + 1].0 - Self::P[i].0))
            .sum();
        let dy: f64 = (0..3)
            .map(|i| 3.0 * w[i] * (Self::P[i + 1].1 - Self::P[i].1))
            .sum();
        (dx, dy)
    }

    /// t tel que y(t) = `y` (y est strictement croissante).
    fn t_at_y(y: f64) -> f64 {
        let (mut lo, mut hi) = (0.0, 1.0);
        for _ in 0..200 {
            let mid = 0.5 * (lo + hi);
            if Self::at(mid).1 < y {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        0.5 * (lo + hi)
    }

    fn simpson(f: impl Fn(f64) -> f64, t0: f64, t1: f64) -> f64 {
        let n = 20_000;
        let h = (t1 - t0) / n as f64;
        let mut sum = f(t0) + f(t1);
        for i in 1..n {
            sum += f(t0 + h * i as f64) * if i % 2 == 1 { 4.0 } else { 2.0 };
        }
        sum * h / 3.0
    }

    fn length(t0: f64, t1: f64) -> f64 {
        Self::simpson(|t| Self::d(t).0.hypot(Self::d(t).1), t0, t1)
    }

    /// Longueur d'arc `s` depuis `t0` -> t.
    fn t_at_length(t0: f64, s: f64) -> f64 {
        let (mut lo, mut hi) = (t0, 1.0);
        for _ in 0..60 {
            let mid = 0.5 * (lo + hi);
            if Self::length(t0, mid) < s {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        0.5 * (lo + hi)
    }
}

fn curve_points() -> [PointMm; 4] {
    [
        pt(100.0, -20.0),
        pt(40.0, 40.0),
        pt(160.0, 60.0),
        pt(100.0, 120.0),
    ]
}

#[test]
fn ca1_ligne_courbe_de_bezier_aire_longueur_et_crans() {
    let (mut p, s) = setup();
    let m = rect_piece(&mut p, s, 0.0, 0.0, 200.0, 100.0, "Devant");
    let line = bezier(&mut p, s, curve_points());
    let o = cut(&mut p, m, CutLine::Path { entity: line }, two()).unwrap();
    let (t0, t1) = (Curve::t_at_y(0.0), Curve::t_at_y(100.0));
    // Aire à gauche (x petit) entre y = 0 et y = 100 : ∫ x dy = ∫ x(t) y'(t) dt.
    let left = Curve::simpson(|t| Curve::at(t).0 * Curve::d(t).1, t0, t1);
    assert!((area(&p, o.pieces[0]) - left).abs() < 1e-4, "{left}");
    assert!((area(&p, o.pieces[1]) - (20000.0 - left)).abs() < 1e-4);
    assert!((area(&p, o.pieces[0]) + area(&p, o.pieces[1]) - 20000.0).abs() < 1e-6);
    let len = Curve::length(t0, t1);
    let ml = marks(&p, o.pieces[0]);
    let mr = marks(&p, o.pieces[1]);
    assert_eq!((ml.len(), mr.len()), (2, 2));
    for (k, (a, b)) in ml.iter().zip(&mr).enumerate() {
        let want = len * (k as f64 + 1.0) / 3.0;
        assert!((a.0 - want).abs() < 1e-4, "{} ≠ {want}", a.0);
        // Même distance, même point : des deux côtés.
        assert!((a.0 - b.0).abs() < 1e-9);
        assert!(distance_mm(a.1, b.1) < 1e-9);
        // Le point est bien sur la courbe, à la longueur d'arc attendue (calcul à part).
        let t = Curve::t_at_length(t0, want);
        let (x, y) = Curve::at(t);
        assert!(distance_mm(a.1, pt(x, y)) < 1e-3, "{:?} ≠ ({x}, {y})", a.1);
    }
}

#[test]
fn ca1_ligne_arc_de_cercle() {
    let (mut p, s) = setup();
    let m = rect_piece(&mut p, s, 0.0, 0.0, 200.0, 100.0, "Devant");
    let o = p
        .apply(Command::AddArc {
            sheet: s,
            center: PointRef::New(pt(100.0, 50.0)),
            radius_mm: 80.0,
            start_deg: -90.0,
            end_deg: 90.0,
            ccw: true,
        })
        .unwrap();
    let arc = o.entities[0];
    let o = cut(&mut p, m, CutLine::Path { entity: arc }, two()).unwrap();
    // Traversées en y = 0 et y = 100 : x = 100 + sqrt(80² - 50²), angles ± asin(50 / 80).
    let dx = (80.0f64 * 80.0 - 50.0 * 50.0).sqrt();
    let a = (50.0f64 / 80.0).asin();
    for id in &o.pieces {
        let c = corners(&p, *id);
        assert!(has_corner(&c, pt(100.0 + dx, 0.0)) && has_corner(&c, pt(100.0 + dx, 100.0)));
    }
    // Segment circulaire de corde 100 (r²/2 (2a - sin 2a)), qui bombe vers x grand (à gauche
    // de P -> Q) : la droite = rectangle à droite de la corde moins le segment.
    let seg = 80.0 * 80.0 / 2.0 * (2.0 * a - (2.0 * a).sin());
    let right = 100.0 * (100.0 - dx) - seg;
    assert!((area(&p, o.pieces[1]) - right).abs() < 1e-6);
    assert!((area(&p, o.pieces[0]) - (20000.0 - right)).abs() < 1e-6);
    // Crans au tiers et aux deux tiers de l'arc (longueur 80 * 2a), points sur le cercle.
    let (ml, mr) = (marks(&p, o.pieces[0]), marks(&p, o.pieces[1]));
    for (k, (x, y)) in ml.iter().zip(&mr).enumerate() {
        let ang = -a + 2.0 * a * (k as f64 + 1.0) / 3.0;
        let want = pt(100.0 + 80.0 * ang.cos(), 50.0 + 80.0 * ang.sin());
        assert!(distance_mm(x.1, want) < 1e-6 && distance_mm(y.1, want) < 1e-6);
        assert!((x.0 - 80.0 * 2.0 * a * (k as f64 + 1.0) / 3.0).abs() < 1e-6);
    }
}

#[test]
fn ca1_coupe_par_un_sommet_et_pieces_concaves() {
    // Diagonale entre deux sommets : deux triangles.
    let (mut p, s) = setup();
    let m = rect_piece(&mut p, s, 0.0, 0.0, 200.0, 100.0, "Devant");
    let o = cut(&mut p, m, pts(pt(0.0, 0.0), pt(200.0, 100.0)), two()).unwrap();
    assert!((area(&p, o.pieces[0]) - 10000.0).abs() < 1e-6);
    assert!((area(&p, o.pieces[1]) - 10000.0).abs() < 1e-6);
    assert_eq!(corners(&p, o.pieces[0]).len(), 3);
    // Pièce en L (concave) : aire 200*100 - 100*50 = 15000 ; la verticale x = 50 la coupe en 1500...
    let (mut p, s) = setup();
    let pts_l = [
        pt(0.0, 0.0),
        pt(200.0, 0.0),
        pt(200.0, 50.0),
        pt(100.0, 50.0),
        pt(100.0, 100.0),
        pt(0.0, 100.0),
    ];
    let ids: Vec<EntityId> = (0..6)
        .map(|i| segment(&mut p, s, pts_l[i], pts_l[(i + 1) % 6]))
        .collect();
    let l = p
        .apply(Command::ExtractPiece {
            sheet: s,
            source: PieceSource::Paths { entities: ids },
            name: "L".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    let o = cut(&mut p, l, pts(pt(50.0, -10.0), pt(50.0, 110.0)), two()).unwrap();
    // Gauche (x < 50) : 50 * 100 = 5000 ; droite : 15000 - 5000 = 10000.
    assert!((area(&p, o.pieces[0]) - 5000.0).abs() < 1e-6);
    assert!((area(&p, o.pieces[1]) - 10000.0).abs() < 1e-6);
    // Ligne horizontale y = 75 : traverse le contour en 2 points (x = 0 et x = 100) -> accepté ;
    // y = 25 : 2 points aussi ; la droite y = 50 longe un bord.
    let (mut q, _) = (p.clone(), ());
    let id = o.pieces[1];
    let r = cut(&mut q, id, pts(pt(-10.0, 25.0), pt(300.0, 25.0)), two());
    assert!(r.is_ok(), "{r:?}");
}

// ======================================================================== CA2

#[test]
fn ca2_crans_aux_memes_distances_depuis_le_meme_repere() {
    let (p, _, o) = oblique();
    let len = cut_len_oblique();
    let (l, r) = (o.pieces[0], o.pieces[1]);
    let (ml, mr) = (marks(&p, l), marks(&p, r));
    assert_eq!((ml.len(), mr.len()), (2, 2));
    let dir = ((X1 - X0) / len, 100.0 / len);
    for k in 0..2 {
        let want = len * (k as f64 + 1.0) / 3.0;
        // Distance depuis P (départ de la ligne) : tiers et deux tiers, des deux côtés.
        assert!((ml[k].0 - want).abs() < 1e-9 && (mr[k].0 - want).abs() < 1e-9);
        assert!((ml[k].0 - mr[k].0).abs() < 1e-12);
        // Même point du plan : P + want * direction.
        let at = pt(X0 + want * dir.0, want * dir.1);
        assert!(distance_mm(ml[k].1, at) < 1e-9 && distance_mm(mr[k].1, at) < 1e-9);
    }
    // Les deux tracés de découpe vont de P à Q ; celui de droite est marqué « retourné ».
    let (el, er) = (cut_edge(&p, l), cut_edge(&p, r));
    assert!(!el.reversed && er.reversed);
    for (id, e) in [(l, el), (r, er)] {
        let (a, b) = ends_of(&p.piece_sheet(id).unwrap().shape(e.entity).unwrap());
        assert!(distance_mm(a, pt(X0, 0.0)) < 1e-9 && distance_mm(b, pt(X1, 100.0)) < 1e-9);
    }
    // Côté du cran : vers l'extérieur de chaque morceau (droite de P -> Q à gauche, gauche à droite).
    assert!(ml.iter().all(|m| m.2 == pf_geometry::Side::Right));
    assert!(mr.iter().all(|m| m.2 == pf_geometry::Side::Left));
    // Aucun cran sur la feuille d'origine.
    assert!(p.sheets[0].notches.is_empty());
}

#[test]
fn ca2_les_crans_sont_de_vrais_crans_annulables_et_exportes() {
    let (p, _, o) = oblique();
    for id in &o.pieces {
        let sheet = p.piece_sheet(*id).unwrap();
        assert_eq!(sheet.notches.len(), 2);
        // Lus par `notch_marks` : un trait par cran, de longueur par défaut, hors d'un coin.
        for n in &sheet.notches {
            let m = sheet.notch_marks(n).unwrap();
            assert_eq!(m.len(), 1);
            assert!((distance_mm(m[0].0, m[0].1) - notch::DEFAULT_NOTCH_LENGTH_MM).abs() < 1e-9);
        }
        // Exportés : le trait figure dans la scène de la feuille.
        let scene = pf_export::Scene::from_sheet(&p, sheet.id).unwrap();
        assert_eq!(scene.notch_marks.len(), 2);
    }
    // Identifiants créés dans l'`Outcome`.
    assert_eq!(o.notches.len(), 4);
    p.validate().unwrap();
}

#[test]
fn ca2_nombre_et_pas_parametrables() {
    let len = cut_len_oblique();
    let (mut p, s) = setup();
    let m = rect_piece(&mut p, s, 0.0, 0.0, 200.0, 100.0, "Devant");
    let line = pts(pt(40.0, -20.0), pt(140.0, 120.0));
    // Zéro cran : aucun.
    let mut q = p.clone();
    let o = cut(&mut q, m, line, CutNotches::Count { count: 0 }).unwrap();
    assert!(o.notches.is_empty());
    // Quatre crans : aux cinquièmes.
    let mut q = p.clone();
    let o = cut(&mut q, m, line, CutNotches::Count { count: 4 }).unwrap();
    let (ml, mr) = (marks(&q, o.pieces[0]), marks(&q, o.pieces[1]));
    assert_eq!((ml.len(), mr.len()), (4, 4));
    for k in 0..4 {
        let want = len * (k as f64 + 1.0) / 5.0;
        assert!((ml[k].0 - want).abs() < 1e-9 && (mr[k].0 - want).abs() < 1e-9);
    }
    // Pas de 30 mm : 30, 60, 90 puis 120 ≥ 122,89 - ... : 4 crans (30, 60, 90, 120), L ≈ 122,89.
    let o = cut(&mut p, m, line, CutNotches::Step { step_mm: 30.0 }).unwrap();
    let (ml, mr) = (marks(&p, o.pieces[0]), marks(&p, o.pieces[1]));
    assert_eq!((ml.len(), mr.len()), (4, 4));
    for (k, (a, b)) in ml.iter().zip(&mr).enumerate() {
        let want = 30.0 * (k as f64 + 1.0);
        assert!((a.0 - want).abs() < 1e-9 && (b.0 - want).abs() < 1e-9);
    }
}

// ======================================================================== CA3

#[test]
fn ca3_recoller_les_morceaux_redonne_la_piece_d_origine() {
    let (mut p, _, o) = oblique();
    let (l, r) = (o.pieces[0], o.pieces[1]);
    let (el, er) = (cut_edge(&p, l).entity, cut_edge(&p, r).entity);
    let j = p
        .apply(Command::JoinPieces {
            first: l,
            first_edge: el,
            second: r,
            second_edge: er,
            tolerance_mm: 1e-6,
        })
        .unwrap();
    let id = j.pieces[0];
    assert!((area(&p, id) - 20000.0).abs() < 1e-6);
    assert!((perimeter(&p, id) - 600.0).abs() < 1e-6);
    let c = corners(&p, id);
    for corner in [
        pt(0.0, 0.0),
        pt(200.0, 0.0),
        pt(200.0, 100.0),
        pt(0.0, 100.0),
    ] {
        assert!(has_corner(&c, corner), "{corner:?} manque dans {c:?}");
    }
    // Chaque sommet est sur le contour du rectangle d'origine (les deux autres sont P et Q).
    for v in &c {
        let on_x = (v.x.abs() < 1e-6 || (v.x - 200.0).abs() < 1e-6)
            && (-1e-6..=100.0 + 1e-6).contains(&v.y);
        let on_y = (v.y.abs() < 1e-6 || (v.y - 100.0).abs() < 1e-6)
            && (-1e-6..=200.0 + 1e-6).contains(&v.x);
        assert!(on_x || on_y, "{v:?} hors du contour d'origine");
    }
    assert!(has_corner(&c, pt(X0, 0.0)) && has_corner(&c, pt(X1, 100.0)));
    assert_eq!(c.len(), 6);
    p.validate().unwrap();
}

#[test]
fn ca3_recoller_apres_rotation_et_pour_un_bord_courbe() {
    // Découpe par la Bézier, puis un morceau est tourné de 37 degrés autour de (500, 500) : le
    // recollage le ramène et redonne le rectangle d'origine à 1e-6 mm.
    let (mut p, s) = setup();
    let m = rect_piece(&mut p, s, 0.0, 0.0, 200.0, 100.0, "Devant");
    let line = bezier(&mut p, s, curve_points());
    let o = cut(&mut p, m, CutLine::Path { entity: line }, two()).unwrap();
    let (l, r) = (o.pieces[0], o.pieces[1]);
    let sid = p.piece_sheet(r).unwrap().id;
    let all: Vec<PointId> = p.sheet(sid).unwrap().points.iter().map(|q| q.id).collect();
    p.apply(Command::Transform {
        sheet: sid,
        points: all,
        transform: Transform::Rotate {
            center: pt(500.0, 500.0),
            angle_deg: 37.0,
        },
    })
    .unwrap();
    let (el, er) = (cut_edge(&p, l).entity, cut_edge(&p, r).entity);
    let j = p
        .apply(Command::JoinPieces {
            first: l,
            first_edge: el,
            second: r,
            second_edge: er,
            tolerance_mm: 1e-3,
        })
        .unwrap();
    let id = j.pieces[0];
    assert!((area(&p, id) - 20000.0).abs() < 1e-4);
    assert!((perimeter(&p, id) - 600.0).abs() < 1e-4);
    let c = corners(&p, id);
    for corner in [
        pt(0.0, 0.0),
        pt(200.0, 0.0),
        pt(200.0, 100.0),
        pt(0.0, 100.0),
    ] {
        assert!(
            c.iter().any(|v| distance_mm(*v, corner) < 1e-6),
            "{corner:?} manque dans {c:?}"
        );
    }
}

// ======================================================================== CA4

#[test]
fn ca4_refus_sans_trace() {
    let (mut p, s) = setup();
    let m = rect_piece(&mut p, s, 0.0, 0.0, 200.0, 100.0, "Devant");
    let inside = segment(&mut p, s, pt(50.0, 10.0), pt(50.0, 60.0));
    let short = segment(&mut p, s, pt(80.0, -10.0), pt(80.0, 60.0));
    let circle = p
        .apply(Command::AddCircle {
            sheet: s,
            center: PointRef::New(pt(100.0, 50.0)),
            radius_mm: 30.0,
        })
        .unwrap()
        .entities[0];
    let outline_edge = p.piece(m).unwrap().outline[0].entity;
    let n = two();
    // Ligne hors de la pièce.
    let t = refused_cut(&mut p, m, pts(pt(300.0, 0.0), pt(300.0, 100.0)), n);
    assert!(t.contains("ne traverse pas"), "{t}");
    // Ligne qui ne traverse pas de bord à bord : un seul contact, ou aucun.
    assert!(refused_cut(&mut p, m, CutLine::Path { entity: short }, n).contains("toucher"));
    assert!(refused_cut(&mut p, m, CutLine::Path { entity: inside }, n).contains("ne traverse pas"));
    // Ligne confondue avec un bord, dégénérée.
    assert!(refused_cut(&mut p, m, pts(pt(-10.0, 0.0), pt(210.0, 0.0)), n).contains("longe"));
    assert!(refused_cut(&mut p, m, pts(pt(50.0, 0.0), pt(50.0, 0.0)), n).contains("dégénérée"));
    // Bord du contour, cercle, tracé inconnu, pièce inconnue.
    assert!(refused_cut(
        &mut p,
        m,
        CutLine::Path {
            entity: outline_edge
        },
        n
    )
    .contains("bord du contour"));
    assert!(refused_cut(&mut p, m, CutLine::Path { entity: circle }, n).contains("segment"));
    refused_cut(
        &mut p,
        m,
        CutLine::Path {
            entity: EntityId(999_999),
        },
        n,
    );
    refused_cut(
        &mut p,
        PieceId(999_999),
        pts(pt(50.0, -1.0), pt(50.0, 101.0)),
        n,
    );
    // Valeurs non finies ou hors bornes.
    let ok_line = pts(pt(50.0, -10.0), pt(50.0, 110.0));
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 1e300, -1e300] {
        refused_cut(&mut p, m, pts(pt(bad, -10.0), pt(50.0, 110.0)), n);
        refused_cut(&mut p, m, pts(pt(50.0, -10.0), pt(50.0, bad)), n);
    }
    for bad in [f64::NAN, f64::INFINITY, 0.0, -5.0, 0.999, 1000.001, 1e300] {
        refused_cut(&mut p, m, ok_line, CutNotches::Step { step_mm: bad });
    }
    refused_cut(&mut p, m, ok_line, CutNotches::Count { count: 51 });
    refused_cut(&mut p, m, ok_line, CutNotches::Count { count: u32::MAX });
    // Pas qui poserait plus de 50 crans sur 120 mm : refus (120 / 1 = 119 crans).
    assert!(refused_cut(&mut p, m, ok_line, CutNotches::Step { step_mm: 1.0 }).contains("plus de"));
    // La ligne correcte est acceptée (sanité du jeu d'essai).
    cut(&mut p.clone(), m, ok_line, n).unwrap();
}

#[test]
fn ca4_ligne_qui_se_recoupe_et_contour_qui_traverse_plus_de_deux_fois() {
    let (mut p, s) = setup();
    let m = rect_piece(&mut p, s, 0.0, 0.0, 200.0, 100.0, "Devant");
    // Bézier à boucle : se recoupe.
    let lp = bezier(
        &mut p,
        s,
        [
            pt(40.0, -10.0),
            pt(160.0, 140.0),
            pt(-60.0, 140.0),
            pt(160.0, -10.0),
        ],
    );
    let n = two();
    refused_cut(&mut p, m, CutLine::Path { entity: lp }, n);
    // Pièce en U : une horizontale traverse en 4 points.
    let (mut q, s2) = setup();
    let u = [
        pt(0.0, 0.0),
        pt(100.0, 0.0),
        pt(100.0, 100.0),
        pt(70.0, 100.0),
        pt(70.0, 30.0),
        pt(30.0, 30.0),
        pt(30.0, 100.0),
        pt(0.0, 100.0),
    ];
    let ids: Vec<EntityId> = (0..8)
        .map(|i| segment(&mut q, s2, u[i], u[(i + 1) % 8]))
        .collect();
    let piece = q
        .apply(Command::ExtractPiece {
            sheet: s2,
            source: PieceSource::Paths { entities: ids },
            name: "U".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    let t = refused_cut(&mut q, piece, pts(pt(-10.0, 60.0), pt(110.0, 60.0)), n);
    assert!(t.contains("4 points"), "{t}");
    // Ligne entre les pointes, hors de la pièce : refus.
    refused_cut(&mut q, piece, pts(pt(30.0, 110.0), pt(70.0, 90.0)), n);
}

#[test]
fn ca4_piece_derivee_liee_refusee() {
    let (mut p, s) = setup();
    let m = rect_piece(&mut p, s, 0.0, 0.0, 200.0, 100.0, "Devant");
    let d = p
        .apply(Command::CreateDerivedPiece {
            mother: m,
            kind: DerivedKind::Lining,
            operation: DerivedOp::Offset { distance_mm: -5.0 },
            name: None,
        })
        .unwrap()
        .pieces[0];
    let t = refused_cut(&mut p, d, pts(pt(50.0, -10.0), pt(50.0, 110.0)), two());
    assert!(t.contains("dérivée"), "{t}");
}

// ======================================================================== CA5

#[test]
fn ca5_une_etape_annulable_exacte() {
    let (mut p, s) = setup();
    let m = rect_piece(&mut p, s, 0.0, 0.0, 200.0, 100.0, "Devant");
    let mut d = Document::new(p);
    let start = d.project().clone();
    let start_bytes = bytes(&start);
    let o = d
        .execute(Command::CutPiece {
            piece: m,
            line: pts(pt(40.0, -20.0), pt(140.0, 120.0)),
            notches: two(),
        })
        .unwrap();
    let after = d.project().clone();
    assert_ne!(after, start);
    assert_eq!(o.notches.len(), 4);
    assert!(d.undo());
    assert_eq!(*d.project(), start);
    assert_eq!(bytes(d.project()), start_bytes);
    assert!(
        !d.can_undo(),
        "une seule étape pour découper (crans compris)"
    );
    assert!(d.redo());
    assert_eq!(*d.project(), after);
    // Un refus ne crée pas d'étape.
    let mut d2 = Document::new(start);
    assert!(d2
        .execute(Command::CutPiece {
            piece: m,
            line: pts(pt(300.0, 0.0), pt(300.0, 100.0)),
            notches: two(),
        })
        .is_err());
    assert!(!d2.can_undo());
}

// ======================================================================== CA6

#[test]
fn ca6_aller_retour_json_et_chargeur_strict() {
    let (p, _, o) = oblique();
    let text = bytes(&p);
    let back = persist::from_json(&text).unwrap();
    assert_eq!(back, p);
    assert_eq!(bytes(&back), text, "relu puis réécrit à l'identique");
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["schema_version"], 6);
    // Chargeur strict : une clé en trop sur un cran, une distance hors du tracé, sont refusées.
    let mut bad: serde_json::Value = v.clone();
    let sheets = bad["sheets"].as_array_mut().unwrap();
    let with_notch = sheets
        .iter_mut()
        .find(|s| !s["notches"].as_array().is_none_or(Vec::is_empty))
        .unwrap();
    with_notch["notches"][0]["inconnue"] = serde_json::json!(1);
    assert!(persist::from_json(&bad.to_string()).is_err());
    let mut bad: serde_json::Value = v.clone();
    let sheets = bad["sheets"].as_array_mut().unwrap();
    let with_notch = sheets
        .iter_mut()
        .find(|s| !s["notches"].as_array().is_none_or(Vec::is_empty))
        .unwrap();
    with_notch["notches"][0]["distance_mm"] = serde_json::json!(9999.0);
    assert!(persist::from_json(&bad.to_string()).is_err());
    assert_eq!(o.pieces.len(), 2);
}

#[test]
fn ca6_pf_validate_accepte_le_fichier() {
    let (p, _, _) = oblique();
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "pf-decoupes-validate-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("decoupe.pforge.json");
    persist::save(&path, &p).unwrap();
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
    // Fichier corrompu : une clé de cran inconnue -> code 1.
    let text = std::fs::read_to_string(&path).unwrap();
    let bad = text.replacen("\"side\"", "\"inconnue\": 1, \"side\"", 1);
    assert_ne!(bad, text);
    std::fs::write(&path, bad).unwrap();
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_pf-validate"))
        .arg(&path)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn ca6_fixtures_et_exemples_inchanges_a_l_octet_schema_3() {
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut n = 0;
    for entry in std::fs::read_dir(&fixtures).unwrap() {
        let path = entry.unwrap().path();
        if !path.to_string_lossy().ends_with(".pforge.json") {
            continue;
        }
        let t = std::fs::read_to_string(&path).unwrap();
        // Fixtures anciennes (v1 / v2) : relues sans erreur ; v3 : réécrites à l'identique.
        let q = persist::from_json(&t).unwrap();
        let again = bytes(&q);
        if t.contains("\"schema_version\": 6") {
            assert_eq!(t.replace("\r\n", "\n"), again, "{}", path.display());
        }
        n += 1;
    }
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if !path.to_string_lossy().ends_with(".pforge.json") {
            continue;
        }
        let t = std::fs::read_to_string(&path)
            .unwrap()
            .replace("\r\n", "\n");
        assert_eq!(
            t,
            bytes(&persist::from_json(&t).unwrap()),
            "{}",
            path.display()
        );
        n += 1;
    }
    assert!(n >= 6);
}

// ======================================================================== CA7

#[test]
fn ca7_fiche_heritee_derivees_cassees_avec_remarque_et_droit_fil() {
    let (mut p, s) = setup();
    let m = rect_piece(&mut p, s, 0.0, 0.0, 200.0, 100.0, "Devant");
    p.apply(Command::SetPieceCard {
        piece: m,
        name: "Devant".into(),
        code: Some("D-7".into()),
        quantity: 2,
        mirrored_pair: false,
        material: None,
        category: Some(PieceCategory::Fabric),
        comment: Some("à doubler".into()),
    })
    .unwrap();
    let d = p
        .apply(Command::CreateDerivedPiece {
            mother: m,
            kind: DerivedKind::Lining,
            operation: DerivedOp::Offset { distance_mm: -5.0 },
            name: None,
        })
        .unwrap()
        .pieces[0];
    let o = cut(&mut p, m, pts(pt(40.0, -20.0), pt(140.0, 120.0)), two()).unwrap();
    // La dérivée n'est pas refusée : son lien est cassé, avec remarque ; le projet reste valide.
    assert!(p.piece(d).unwrap().derived.is_none());
    assert!(
        o.notes
            .iter()
            .any(|n| n.contains("lien") || n.contains("dérivée")),
        "{:?}",
        o.notes
    );
    assert!(o
        .notes
        .iter()
        .any(|n| n.contains("crans de repère") || n.contains("cran(s)")));
    let (l, r) = (p.piece(o.pieces[0]).unwrap(), p.piece(o.pieces[1]).unwrap());
    assert_eq!(l.code.as_deref(), Some("D-7-1"));
    assert_eq!(r.code.as_deref(), Some("D-7-2"));
    assert_eq!((l.quantity, r.quantity), (2, 2));
    assert_eq!(l.comment.as_deref(), Some("à doubler"));
    p.validate().unwrap();
}
