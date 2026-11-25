//! US-07.4 : ampleur par ouverture en éventail. Un groupe de tests par critère : CA1 longueur du
//! bord fixe conservée (à 1e-6 mm), CA2 bord ouvert gagne la somme des ouvertures et sommets de
//! chaque morceau calculés à la main, CA3 longueurs des lignes de coupe conservées, CA4 valeurs
//! d'ouverture par ligne et formes obliques, CA5 refus sans trace, CA6 une étape annulable, CA7
//! fichier (schéma 3, relecture, chargeur strict, `pf-validate`), CA8 marge de couture, reports.
//!
//! Convention (non validée métier) : lignes perpendiculaires au bord fixe, pivot au pied de la
//! ligne, ouverture mesurée en haut (corde), valeur « bas » nulle. Les morceaux s'enchaînent : un
//! morceau entraîne ceux qui sont au-delà. Le vide ouvert est rempli (corde de l'ouverture).
//!
//! Pièce de référence : rectangle 200 × 100, anti-horaire (0;0) (200;0) (200;100) (0;100), bord fixe
//! = le bas (200 mm). Ouverture de 30 mm sur une ligne de 100 mm : θ = 2 asin(30 / 200).

use pf_core::persist;
use pf_core::*;
use pf_geometry::{distance_mm, PointMm};
use serde_json::{json, Value};

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

fn bytes(p: &Project) -> String {
    persist::to_json(p).unwrap()
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

fn poly_piece(pts: &[(f64, f64)]) -> (Project, SheetId, PieceId) {
    let mut p = Project::new("Ampleur");
    let s = p.sheets[0].id;
    let n = pts.len();
    let ids = (0..n)
        .map(|i| {
            segment(
                &mut p,
                s,
                pt(pts[i].0, pts[i].1),
                pt(pts[(i + 1) % n].0, pts[(i + 1) % n].1),
            )
        })
        .collect();
    let piece = p
        .apply(Command::ExtractPiece {
            sheet: s,
            source: PieceSource::Paths { entities: ids },
            name: "Jupe".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    (p, s, piece)
}

fn rect() -> (Project, SheetId, PieceId) {
    poly_piece(&[(0.0, 0.0), (200.0, 0.0), (200.0, 100.0), (0.0, 100.0)])
}

/// Sommets du contour (début de chaque tracé, dans le sens du contour).
fn vertices(p: &Project, piece: PieceId) -> Vec<PointMm> {
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
            other => panic!("segment attendu, reçu {other:?}"),
        })
        .collect()
}

fn same_vertices(got: &[PointMm], want: &[(f64, f64)], tol: f64) -> bool {
    got.len() == want.len()
        && want
            .iter()
            .all(|&(x, y)| got.iter().any(|g| distance_mm(*g, pt(x, y)) <= tol))
}

fn perimeter(v: &[PointMm]) -> f64 {
    (0..v.len())
        .map(|i| distance_mm(v[i], v[(i + 1) % v.len()]))
        .sum()
}

/// Aire par la formule du lacet (valeur absolue).
fn area(v: &[PointMm]) -> f64 {
    let n = v.len();
    (0..n)
        .map(|i| v[i].x * v[(i + 1) % n].y - v[(i + 1) % n].x * v[i].y)
        .sum::<f64>()
        .abs()
        / 2.0
}

fn line(at: f64, opening: f64) -> FlareLineSpec {
    FlareLineSpec {
        at_mm: at,
        opening_mm: opening,
    }
}

fn flare_cmd(piece: PieceId, lines: Vec<FlareLineSpec>) -> Command {
    Command::AddPieceFlare {
        piece,
        fixed_from: pt(0.0, 0.0),
        fixed_to: pt(200.0, 0.0),
        lines,
    }
}

fn refused(p: &mut Project, c: Command) -> String {
    let (before, text) = (p.clone(), bytes(p));
    let err = p.apply(c).expect_err("la commande aurait dû être refusée");
    assert_eq!(*p, before, "refus sans trace (valeur)");
    assert_eq!(text, bytes(p), "refus sans trace (octets)");
    assert!(!err.to_string().is_empty());
    err.to_string()
}

/// Rotation horaire de `th` radians autour de `c` (formule écrite ici, indépendante du code testé).
fn rot_cw(q: (f64, f64), c: (f64, f64), th: f64) -> (f64, f64) {
    let (s, co) = th.sin_cos();
    let (dx, dy) = (q.0 - c.0, q.1 - c.1);
    (c.0 + dx * co + dy * s, c.1 - dx * s + dy * co)
}

fn theta(opening: f64, length: f64) -> f64 {
    2.0 * (opening / (2.0 * length)).asin()
}

/// Image d'un point du morceau `i` (au-delà de `i` lignes) : rotation autour du pivot le plus
/// éloigné d'abord, puis des plus proches. `hinges[k]` = pivot d'origine de la ligne `k + 1`.
fn image(i: usize, q: (f64, f64), hinges: &[(f64, f64)], th: &[f64]) -> (f64, f64) {
    let mut q = q;
    for k in (0..i).rev() {
        q = rot_cw(q, hinges[k], th[k]);
    }
    q
}

/// Trois lignes en x = 50, 100, 150, ouvertes de 30 : les sommets du contour, calculés à la main.
fn three_lines_vertices() -> Vec<(f64, f64)> {
    let h = [(50.0, 0.0), (100.0, 0.0), (150.0, 0.0)];
    let th = [theta(30.0, 100.0); 3];
    let im = |i: usize, x: f64, y: f64| image(i, (x, y), &h, &th);
    vec![
        (0.0, 0.0),
        (50.0, 0.0),
        im(1, 100.0, 0.0),
        im(2, 150.0, 0.0),
        im(3, 200.0, 0.0),
        im(3, 200.0, 100.0),
        im(3, 150.0, 100.0),
        im(2, 150.0, 100.0),
        im(2, 100.0, 100.0),
        im(1, 100.0, 100.0),
        im(1, 50.0, 100.0),
        (50.0, 100.0),
        (0.0, 100.0),
    ]
}

// ----- CA1 : longueur du bord fixe conservée ----------------------------------------------------

/// Longueur du bord fixe lue sur le contour : arêtes consécutives du sommet (0 ; 0) au sommet
/// `end` (coin bas-droit tourné).
fn fixed_edge_length(v: &[PointMm], end: (f64, f64)) -> f64 {
    let i0 = v
        .iter()
        .position(|q| distance_mm(*q, pt(0.0, 0.0)) < 1e-9)
        .expect("coin bas-gauche");
    let i1 = v
        .iter()
        .position(|q| distance_mm(*q, pt(end.0, end.1)) < 1e-9)
        .expect("coin bas-droit");
    let n = v.len();
    let mut sum = 0.0;
    let mut k = i0;
    while k != i1 {
        sum += distance_mm(v[k], v[(k + 1) % n]);
        k = (k + 1) % n;
    }
    sum
}

#[test]
fn ca1_une_ligne_de_30_a_x_100_le_bord_fixe_garde_200_mm() {
    let (mut p, _s, piece) = rect();
    p.apply(flare_cmd(piece, vec![line(100.0, 30.0)])).unwrap();
    let v = vertices(&p, piece);
    let end = image(1, (200.0, 0.0), &[(100.0, 0.0)], &[theta(30.0, 100.0)]);
    let len = fixed_edge_length(&v, end);
    assert!((len - 200.0).abs() < 1e-6, "bord fixe : {len}");
}

#[test]
fn ca1_trois_lignes_de_30_le_bord_fixe_garde_200_mm_a_1e_6_pres() {
    let (mut p, _s, piece) = rect();
    p.apply(flare_cmd(
        piece,
        vec![line(50.0, 30.0), line(100.0, 30.0), line(150.0, 30.0)],
    ))
    .unwrap();
    let v = vertices(&p, piece);
    let want = three_lines_vertices();
    let len = fixed_edge_length(&v, want[4]);
    assert!((len - 200.0).abs() < 1e-6, "bord fixe : {len}");
    // L'aperçu dit la même chose (lecture sur la commande rejouée).
    let (q, _qs, qpiece) = rect();
    let pre = q
        .flare_preview(
            qpiece,
            pt(0.0, 0.0),
            pt(200.0, 0.0),
            &[line(50.0, 30.0), line(100.0, 30.0), line(150.0, 30.0)],
        )
        .unwrap();
    assert!((pre.fixed_before_mm - 200.0).abs() < 1e-9);
    assert!((pre.fixed_after_mm - 200.0).abs() < 1e-6);
}

#[test]
fn ca1_bord_fixe_en_haut_ou_donne_a_l_envers_le_bord_fixe_est_conserve() {
    let (mut p, _s, piece) = rect();
    // Bord fixe = le haut, de (200 ; 100) vers (0 ; 100) : positions mesurées depuis (200 ; 100).
    p.apply(Command::AddPieceFlare {
        piece,
        fixed_from: pt(200.0, 100.0),
        fixed_to: pt(0.0, 100.0),
        lines: vec![line(100.0, 30.0)],
    })
    .unwrap();
    let v = vertices(&p, piece);
    // Résultat du cas « bas fixe » retourné d'un demi-tour autour de (100 ; 50).
    let h = [(100.0, 0.0)];
    let th = [theta(30.0, 100.0)];
    let flip = |q: (f64, f64)| (200.0 - q.0, 100.0 - q.1);
    let base = [
        (100.0, 100.0),
        (0.0, 100.0),
        (0.0, 0.0),
        (100.0, 0.0),
        image(1, (200.0, 0.0), &h, &th),
        image(1, (200.0, 100.0), &h, &th),
        image(1, (100.0, 100.0), &h, &th),
    ];
    let want: Vec<(f64, f64)> = base.iter().map(|q| flip(*q)).collect();
    assert!(same_vertices(&v, &want, 1e-9), "{v:?}");
    assert!((perimeter(&v) - 630.0).abs() < 1e-6);
}

// ----- CA2 : bord ouvert, sommets de chaque morceau ---------------------------------------------

#[test]
fn ca2_une_ligne_sept_sommets_calcules_a_la_main() {
    let (mut p, _s, piece) = rect();
    let out = p.apply(flare_cmd(piece, vec![line(100.0, 30.0)])).unwrap();
    assert_eq!(out.pieces, vec![piece]);
    assert_eq!(out.flares.len(), 1);
    let h = [(100.0, 0.0)];
    let th = [theta(30.0, 100.0)];
    let want = [
        (100.0, 100.0),
        (0.0, 100.0),
        (0.0, 0.0),
        (100.0, 0.0),
        image(1, (200.0, 0.0), &h, &th),
        image(1, (200.0, 100.0), &h, &th),
        image(1, (100.0, 100.0), &h, &th),
    ];
    // Valeurs numériques à la main : c = 0,955 ; s = 0,3 x racine(0,9775) = 0,296604.
    let s = 0.3 * 0.9775_f64.sqrt();
    assert!((want[4].0 - 195.5).abs() < 1e-9 && (want[4].1 + 100.0 * s).abs() < 1e-9);
    assert!((want[5].0 - (195.5 + 100.0 * s)).abs() < 1e-9);
    assert!((want[6].1 - 95.5).abs() < 1e-9);
    let v = vertices(&p, piece);
    assert!(same_vertices(&v, &want, 1e-9), "{v:?}");
}

#[test]
fn ca2_trois_lignes_de_30_treize_sommets_et_bord_haut_gagne_90() {
    let (mut p, _s, piece) = rect();
    p.apply(flare_cmd(
        piece,
        vec![line(50.0, 30.0), line(100.0, 30.0), line(150.0, 30.0)],
    ))
    .unwrap();
    let v = vertices(&p, piece);
    let want = three_lines_vertices();
    assert!(same_vertices(&v, &want, 1e-9), "{v:?}");
    // Le haut est la chaîne de (50 ; 100)... : quatre morceaux de 50 et trois cordes de 30. Le bord
    // ouvert gagne 3 x 30 = 90 ; le bas (bord fixe) et les côtés ne changent pas : périmètre 690.
    let chords: Vec<f64> = (0..v.len())
        .map(|i| distance_mm(v[i], v[(i + 1) % v.len()]))
        .filter(|d| (d - 30.0).abs() < 1e-9)
        .collect();
    assert_eq!(chords.len(), 3, "trois cordes d'ouverture");
    assert!((perimeter(&v) - 690.0).abs() < 1e-6);
    // Aire : 20 000 + 3 x 1/2 x 100^2 x sin θ.
    let th = theta(30.0, 100.0);
    let expected = 20_000.0 + 3.0 * 0.5 * 100.0 * 100.0 * th.sin();
    assert!(
        (area(&v) - expected).abs() < 1e-6,
        "{} / {expected}",
        area(&v)
    );
    // Mesures observées par le cœur.
    let c = p.piece_contour(piece).unwrap();
    assert!((c.area_mm2 - expected).abs() < 1e-6);
    assert!((c.perimeter_mm - 690.0).abs() < 1e-6);
}

#[test]
fn ca2_l_aperçu_donne_le_gain_du_bord_ouvert_egal_a_la_somme_des_ouvertures() {
    let (p, _s, piece) = rect();
    let lines = [line(40.0, 12.0), line(110.0, 25.0), line(170.0, 7.5)];
    let pre = p
        .flare_preview(piece, pt(0.0, 0.0), pt(200.0, 0.0), &lines)
        .unwrap();
    assert!((pre.open_gain_mm - 44.5).abs() < 1e-12);
    assert!((pre.perimeter_after_mm - pre.perimeter_before_mm - 44.5).abs() < 1e-6);
    assert!((pre.perimeter_before_mm - 600.0).abs() < 1e-9);
    assert!((pre.area_before_mm2 - 20_000.0).abs() < 1e-9);
    let gain: f64 = lines
        .iter()
        .map(|l| 0.5 * 100.0 * 100.0 * theta(l.opening_mm, 100.0).sin())
        .sum();
    assert!((pre.area_after_mm2 - 20_000.0 - gain).abs() < 1e-6);
    assert_eq!(pre.lines.len(), 3);
    assert!((pre.lines[0].at_mm - 40.0).abs() < 1e-12);
    assert!((pre.lines[1].angle_deg - theta(25.0, 100.0).to_degrees()).abs() < 1e-9);
    assert!((pre.lines[2].length_mm - 100.0).abs() < 1e-9);
}

// ----- CA3 : longueurs des lignes de coupe conservées -------------------------------------------

#[test]
fn ca3_les_deux_levres_de_chaque_ligne_gardent_100_mm_et_l_ouverture_est_celle_demandee() {
    let (mut p, _s, piece) = rect();
    let lines = [line(50.0, 30.0), line(100.0, 20.0), line(150.0, 45.0)];
    p.apply(flare_cmd(piece, lines.to_vec())).unwrap();
    let flares = p.piece_flares(piece);
    assert_eq!(flares.len(), 3);
    // Positions finales calculées à la main : lèvre fixe = image de (x ; 100) par les rotations des
    // lignes plus proches ; lèvre tournée = image par la rotation de la ligne elle-même en plus.
    let h = [(50.0, 0.0), (100.0, 0.0), (150.0, 0.0)];
    let th = [theta(30.0, 100.0), theta(20.0, 100.0), theta(45.0, 100.0)];
    for (k, f) in flares.iter().enumerate() {
        let top = (h[k].0, 100.0);
        let hinge = image(k, h[k], &h, &th);
        let stay = image(k, top, &h, &th);
        let moved = image(k + 1, top, &h, &th);
        assert!(
            distance_mm(f.hinge(), pt(hinge.0, hinge.1)) < 1e-9,
            "pivot {k}"
        );
        assert!(
            distance_mm(f.stay_end(), pt(stay.0, stay.1)) < 1e-9,
            "lèvre fixe {k}"
        );
        assert!(
            distance_mm(f.moved_end(), pt(moved.0, moved.1)) < 1e-9,
            "lèvre tournée {k}"
        );
        assert!((f.length_mm() - 100.0).abs() < 1e-9);
        assert!((distance_mm(f.hinge(), f.moved_end()) - 100.0).abs() < 1e-9);
        assert!((f.opening_mm() - lines[k].opening_mm).abs() < 1e-9);
        assert!((f.angle_deg() - th[k].to_degrees()).abs() < 1e-9);
    }
    // Chaque morceau garde sa forme : distance entre deux coins d'un même morceau inchangée.
    let v = vertices(&p, piece);
    let tl3 = image(3, (150.0, 100.0), &h, &th);
    let tr3 = image(3, (200.0, 100.0), &h, &th);
    let (a, b) = (pt(tl3.0, tl3.1), pt(tr3.0, tr3.1));
    assert!(v.iter().any(|q| distance_mm(*q, a) < 1e-9));
    assert!(v.iter().any(|q| distance_mm(*q, b) < 1e-9));
    assert!((distance_mm(a, b) - 50.0).abs() < 1e-9);
}

// ----- CA4 : valeurs différentes par ligne, formes obliques --------------------------------------

#[test]
fn ca4_ouvertures_differentes_aire_et_perimetre_a_la_main() {
    let (mut p, _s, piece) = rect();
    p.apply(flare_cmd(piece, vec![line(60.0, 20.0), line(140.0, 50.0)]))
        .unwrap();
    let v = vertices(&p, piece);
    // Périmètre 600 + 20 + 50 ; aire 20 000 + 1/2 x 100^2 x (sin θ1 + sin θ2).
    assert!((perimeter(&v) - 670.0).abs() < 1e-6);
    let expected = 20_000.0 + 5_000.0 * (theta(20.0, 100.0).sin() + theta(50.0, 100.0).sin());
    assert!((area(&v) - expected).abs() < 1e-6);
    let h = [(60.0, 0.0), (140.0, 0.0)];
    let th = [theta(20.0, 100.0), theta(50.0, 100.0)];
    let end = image(2, (200.0, 0.0), &h, &th);
    assert!((fixed_edge_length(&v, end) - 200.0).abs() < 1e-6);
}

#[test]
fn ca4_trapeze_ligne_oblique_au_bord_longueur_de_ligne_variable() {
    // Trapèze (0;0) (200;0) (160;100) (40;100), bord fixe = le bas. Ligne à x = 100 : de (100 ; 0)
    // à (100 ; 100), L = 100. Ouverture 20 : aire gagnée 1/2 x 100^2 x sin θ.
    let (mut p, _s, piece) = poly_piece(&[(0.0, 0.0), (200.0, 0.0), (160.0, 100.0), (40.0, 100.0)]);
    p.apply(flare_cmd(piece, vec![line(100.0, 20.0)])).unwrap();
    let v = vertices(&p, piece);
    let base = 200.0 + 2.0 * (40.0_f64.hypot(100.0)) + 120.0;
    assert!((perimeter(&v) - (base + 20.0)).abs() < 1e-6);
    let a0 = (200.0 + 120.0) / 2.0 * 100.0;
    assert!((area(&v) - (a0 + 5_000.0 * theta(20.0, 100.0).sin())).abs() < 1e-6);
    // Ligne à x = 60 : le haut est à y = 100 aussi (côté gauche oblique de (0;0) à (40;100)), L = 100.
    let (mut q, _qs, qpiece) =
        poly_piece(&[(0.0, 0.0), (200.0, 0.0), (160.0, 100.0), (40.0, 100.0)]);
    q.apply(flare_cmd(qpiece, vec![line(60.0, 20.0)])).unwrap();
    let w = vertices(&q, qpiece);
    assert!((perimeter(&w) - (base + 20.0)).abs() < 1e-6);
    // Ligne à x = 20 : coupe le côté gauche oblique à y = 50 (x = 20 sur la droite de (0;0) à (40;100)),
    // L = 50.
    let (mut r, _rs, rpiece) =
        poly_piece(&[(0.0, 0.0), (200.0, 0.0), (160.0, 100.0), (40.0, 100.0)]);
    r.apply(flare_cmd(rpiece, vec![line(20.0, 10.0)])).unwrap();
    let z = vertices(&r, rpiece);
    assert!((perimeter(&z) - (base + 10.0)).abs() < 1e-6);
    let f = &r.piece_flares(rpiece)[0];
    assert!((f.length_mm() - 50.0).abs() < 1e-9);
    // Aire : trapèze 32 000 + 1/2 x 50^2 x sin(2 asin(10 / 100)).
    let expected = 16_000.0 + 0.5 * 2_500.0 * theta(10.0, 50.0).sin();
    let _ = expected;
    let a0 = (200.0 + 120.0) / 2.0 * 100.0;
    assert!((area(&z) - (a0 + 0.5 * 2_500.0 * theta(10.0, 50.0).sin())).abs() < 1e-6);
}

#[test]
fn ca4_bord_fixe_oblique_rectangle_tourne_de_30_degres() {
    // Rectangle 200 x 100 tourné de 30° autour de l'origine : sommets calculés à la main.
    let (c, s) = (30.0_f64.to_radians().cos(), 30.0_f64.to_radians().sin());
    let pts = [
        (0.0, 0.0),
        (200.0 * c, 200.0 * s),
        (200.0 * c - 100.0 * s, 200.0 * s + 100.0 * c),
        (-100.0 * s, 100.0 * c),
    ];
    let (mut p, _s, piece) = poly_piece(&pts);
    p.apply(Command::AddPieceFlare {
        piece,
        fixed_from: pt(pts[0].0, pts[0].1),
        fixed_to: pt(pts[1].0, pts[1].1),
        lines: vec![line(80.0, 30.0), line(150.0, 30.0)],
    })
    .unwrap();
    let v = vertices(&p, piece);
    assert!((perimeter(&v) - 660.0).abs() < 1e-6);
    let expected = 20_000.0 + 2.0 * 5_000.0 * theta(30.0, 100.0).sin();
    assert!((area(&v) - expected).abs() < 1e-6);
    // Même résultat que le cas droit tourné de 30° : bas, premier morceau.
    let flares = p.piece_flares(piece);
    assert!((flares[0].length_mm() - 100.0).abs() < 1e-9);
    assert!((flares[1].opening_mm() - 30.0).abs() < 1e-9);
}

// ----- CA5 : refus sans trace --------------------------------------------------------------------

#[test]
fn ca5_refus_sans_trace_valeurs_et_nombre_de_lignes() {
    let (mut p, _s, piece) = rect();
    // Nombre de lignes hors de 1 à 50.
    refused(&mut p, flare_cmd(piece, vec![]));
    let many: Vec<FlareLineSpec> = (1..=51).map(|k| line(3.0 * k as f64, 1.0)).collect();
    refused(&mut p, flare_cmd(piece, many));
    // Ouvertures négatives, nulles, non finies, hors bornes, trop grandes pour la ligne.
    for o in [-1.0, 0.0, f64::NAN, f64::INFINITY, 0.005, 2001.0, 150.0] {
        refused(&mut p, flare_cmd(piece, vec![line(100.0, o)]));
    }
    // Positions : sur une extrémité, hors du bord, non finie, doublée.
    for at in [0.0, -5.0, 200.0, 250.0, f64::NAN, f64::INFINITY] {
        refused(&mut p, flare_cmd(piece, vec![line(at, 10.0)]));
    }
    let msg = refused(
        &mut p,
        flare_cmd(piece, vec![line(100.0, 10.0), line(100.0, 10.0)]),
    );
    assert!(msg.contains("même position"), "{msg}");
    // Cinquante lignes : accepté si elles sont assez espacées et peu ouvertes.
    let ok: Vec<FlareLineSpec> = (1..=50).map(|k| line(3.9 * k as f64, 0.2)).collect();
    p.apply(flare_cmd(piece, ok)).unwrap();
    assert_eq!(p.piece_flares(piece).len(), 50);
}

#[test]
fn ca5_refus_sans_trace_bord_fixe_piece_et_contour() {
    let (mut p, _s, piece) = rect();
    // Pièce inconnue ; bord qui n'est pas un côté du contour (diagonale, portion de côté, hors pièce).
    refused(&mut p, flare_cmd(PieceId(999_999), vec![line(100.0, 10.0)]));
    for (a, b) in [
        (pt(0.0, 0.0), pt(200.0, 100.0)),
        (pt(0.0, 0.0), pt(100.0, 0.0)),
        (pt(0.0, 10.0), pt(200.0, 10.0)),
        (pt(0.0, 0.0), pt(0.0, 0.0)),
        (pt(f64::NAN, 0.0), pt(200.0, 0.0)),
    ] {
        let msg = refused(
            &mut p,
            Command::AddPieceFlare {
                piece,
                fixed_from: a,
                fixed_to: b,
                lines: vec![line(50.0, 10.0)],
            },
        );
        assert!(!msg.is_empty());
    }
    // Forme en C : la perpendiculaire à x = 100 traverse le contour en quatre points.
    let (mut c, _cs, cpiece) = poly_piece(&[
        (0.0, 0.0),
        (200.0, 0.0),
        (200.0, 30.0),
        (50.0, 30.0),
        (50.0, 70.0),
        (200.0, 70.0),
        (200.0, 100.0),
        (0.0, 100.0),
    ]);
    refused(&mut c, flare_cmd(cpiece, vec![line(100.0, 10.0)]));
    // Ligne qui ne traverse le contour qu'en son pied (tangente) : la droite x = 100 d'un triangle
    // dont le sommet est à x = 100 ne coupe qu'en un point.
    let (mut t, _ts, tpiece) = poly_piece(&[(0.0, 0.0), (100.0, 0.0), (100.0, 100.0)]);
    let msg = refused(
        &mut t,
        Command::AddPieceFlare {
            piece: tpiece,
            fixed_from: pt(0.0, 0.0),
            fixed_to: pt(100.0, 0.0),
            lines: vec![line(99.9995, 5.0)],
        },
    );
    assert!(!msg.is_empty());
}

#[test]
fn ca5_refus_ouverture_cumulee_qui_ramene_les_morceaux_les_uns_sur_les_autres() {
    let (mut p, _s, piece) = rect();
    // Cinq lignes de 90 degrés : 450 degrés cumulés ; la droite de la ligne suivante traverse alors
    // les morceaux déjà tournés (quatre points) : refus, la pièce n'est pas touchée.
    let lines: Vec<FlareLineSpec> = [30.0, 60.0, 90.0, 120.0, 150.0]
        .iter()
        .map(|x| line(*x, 141.0))
        .collect();
    let msg = refused(&mut p, flare_cmd(piece, lines));
    assert!(
        msg.contains("chevauch") || msg.contains("coupe") || msg.contains("traverse"),
        "{msg}"
    );
}

#[test]
fn ca5_refus_piece_derivee_pince_pli_ou_deja_ouverte() {
    let (mut p, _s, piece) = rect();
    let derived = p
        .apply(Command::CreateDerivedPiece {
            mother: piece,
            kind: DerivedKind::Lining,
            operation: DerivedOp::Offset { distance_mm: -5.0 },
            name: None,
        })
        .unwrap()
        .pieces[0];
    let msg = refused(
        &mut p,
        Command::AddPieceFlare {
            piece: derived,
            fixed_from: pt(5.0, 5.0),
            fixed_to: pt(195.0, 5.0),
            lines: vec![line(50.0, 10.0)],
        },
    );
    assert!(msg.contains("dérivée"), "{msg}");
    // Pièce avec pince.
    let (mut q, qs, qpiece) = rect();
    let apex = q
        .apply(Command::AddPoint {
            sheet: qs,
            at: pt(50.0, 80.0),
        })
        .unwrap()
        .points[0];
    let axis = q
        .apply(Command::AddPoint {
            sheet: qs,
            at: pt(50.0, 0.0),
        })
        .unwrap()
        .points[0];
    q.apply(Command::AddPieceDart {
        piece: qpiece,
        apex: PointRef::Existing(apex),
        toward: PointRef::Existing(axis),
        length_mm: 80.0,
        opening_mm: 10.0,
        fixed: pf_geometry::Side::Left,
    })
    .unwrap();
    let msg = refused(&mut q, flare_cmd(qpiece, vec![line(100.0, 10.0)]));
    assert!(msg.contains("pince"), "{msg}");
    // Pièce avec pli.
    let (mut r, _rs, rpiece) = rect();
    r.apply(Command::AddPiecePleats {
        piece: rpiece,
        from: pt(100.0, 0.0),
        to: pt(100.0, 100.0),
        depth_mm: 10.0,
        kind: PleatKind::Flat,
        sense: pf_geometry::Side::Right,
        count: 1,
        span_mm: 0.0,
    })
    .unwrap();
    let msg = refused(
        &mut r,
        Command::AddPieceFlare {
            piece: rpiece,
            fixed_from: pt(0.0, 0.0),
            fixed_to: pt(220.0, 0.0),
            lines: vec![line(50.0, 10.0)],
        },
    );
    assert!(msg.contains("plis"), "{msg}");
    // Déjà une ampleur : une seule opération par pièce.
    let (mut u, _us, upiece) = rect();
    u.apply(flare_cmd(upiece, vec![line(100.0, 10.0)])).unwrap();
    let sides = u.piece_flare_sides(upiece);
    let s0 = sides[0];
    let msg = refused(
        &mut u,
        Command::AddPieceFlare {
            piece: upiece,
            fixed_from: s0.from,
            fixed_to: s0.to,
            lines: vec![line(30.0, 10.0)],
        },
    );
    assert!(msg.contains("déjà"), "{msg}");
}

#[test]
fn ca5_l_apercu_refuse_comme_la_commande_et_ne_modifie_rien() {
    let (p, _s, piece) = rect();
    let before = bytes(&p);
    assert!(p
        .flare_preview(piece, pt(0.0, 0.0), pt(200.0, 0.0), &[line(100.0, -3.0)])
        .is_err());
    assert!(p
        .flare_preview(piece, pt(0.0, 0.0), pt(200.0, 0.0), &[])
        .is_err());
    assert_eq!(bytes(&p), before);
}

#[test]
fn ca5_cotes_droits_proposes_pour_le_bord_fixe_et_positions_egales() {
    let (p, _s, piece) = rect();
    let sides = p.piece_flare_sides(piece);
    assert_eq!(sides.len(), 4);
    let lens: Vec<f64> = sides.iter().map(|s| s.length_mm).collect();
    assert!(lens.iter().filter(|l| (**l - 200.0).abs() < 1e-9).count() == 2);
    assert!(lens.iter().filter(|l| (**l - 100.0).abs() < 1e-9).count() == 2);
    assert_eq!(flare_even_positions(200.0, 3), vec![50.0, 100.0, 150.0]);
    assert!(flare_even_positions(200.0, 0).is_empty());
    assert!(p.piece_flare_sides(PieceId(77)).is_empty());
}

// ----- CA6 : une étape annulable -----------------------------------------------------------------

#[test]
fn ca6_une_seule_etape_annulable_et_un_refus_ne_cree_aucune_etape() {
    let (p, _s, piece) = rect();
    let mut doc = Document::new(p);
    let base = bytes(doc.project());
    assert!(doc
        .execute(flare_cmd(piece, vec![line(300.0, 10.0)]))
        .is_err());
    assert_eq!(bytes(doc.project()), base);
    assert!(!doc.can_undo());
    doc.execute(flare_cmd(
        piece,
        vec![line(50.0, 30.0), line(100.0, 30.0), line(150.0, 30.0)],
    ))
    .unwrap();
    let after = bytes(doc.project());
    assert_ne!(after, base);
    assert!(doc.undo(), "une annulation");
    assert_eq!(bytes(doc.project()), base);
    assert!(!doc.can_undo(), "une seule étape pour trois lignes");
    assert!(doc.redo());
    assert_eq!(bytes(doc.project()), after);
}

#[test]
fn ca6_la_construction_d_origine_est_intacte_la_piece_est_sur_une_feuille_dediee() {
    let (mut p, s, piece) = rect();
    let sheet_before = p.sheet(s).unwrap().clone();
    let n_sheets = p.sheets.len();
    let out = p.apply(flare_cmd(piece, vec![line(100.0, 30.0)])).unwrap();
    assert_eq!(*p.sheet(s).unwrap(), sheet_before);
    assert_eq!(p.sheets.len(), n_sheets + 1);
    assert_eq!(out.sheet, Some(p.piece_sheet(piece).unwrap().id));
    assert_ne!(p.piece_sheet(piece).unwrap().id, s);
    assert_eq!(p.piece(piece).unwrap().link, PieceLink::Detached);
    assert!(out.notes[0].contains("ampleur"), "{:?}", out.notes);
    assert!(out.notes[0].contains("200.000"), "{:?}", out.notes);
}

// ----- CA7 : fichier -----------------------------------------------------------------------------

#[test]
fn ca7_fichier_schema_3_relu_a_l_identique_chargeur_strict_et_pf_validate() {
    let (mut p, _s, piece) = rect();
    p.apply(flare_cmd(piece, vec![line(70.0, 20.0), line(130.0, 25.0)]))
        .unwrap();
    let text = bytes(&p);
    let v: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["schema_version"], 6);
    let back = persist::from_json(&text).unwrap();
    assert_eq!(back, p);
    assert_eq!(text, bytes(&back));
    assert_eq!(v["pieces"][0]["flares"].as_array().unwrap().len(), 2);
    let mutate = |f: &dyn Fn(&mut Value)| {
        let mut w = v.clone();
        f(&mut w["pieces"][0]["flares"][0]);
        persist::from_json(&serde_json::to_string(&w).unwrap())
    };
    assert!(mutate(&|_| {}).is_ok());
    assert!(mutate(&|d| d["z"] = json!(1)).is_err(), "clé inconnue");
    assert!(mutate(&|d| d["stay_x_mm"] = json!("a")).is_err());
    assert!(
        mutate(&|d| d["hinge_x_mm"] = json!(1.0e12)).is_err(),
        "hors espace"
    );
    assert!(
        mutate(&|d| {
            d["moved_x_mm"] = d["stay_x_mm"].clone();
            d["moved_y_mm"] = d["stay_y_mm"].clone();
        })
        .is_err(),
        "lèvres confondues"
    );
    assert!(
        mutate(&|d| {
            d["stay_x_mm"] = d["hinge_x_mm"].clone();
            d["stay_y_mm"] = d["hinge_y_mm"].clone();
        })
        .is_err(),
        "ligne nulle"
    );
    assert!(
        mutate(&|d| d["moved_y_mm"] = json!(d["moved_y_mm"].as_f64().unwrap() + 1.0)).is_err(),
        "lèvres de longueurs différentes"
    );
    assert!(
        mutate(&|d| d["id"] = json!(1)).is_err(),
        "identifiant dupliqué"
    );
    let mut missing = v.clone();
    missing["pieces"][0]["flares"][0]
        .as_object_mut()
        .unwrap()
        .remove("moved_y_mm");
    assert!(persist::from_json(&serde_json::to_string(&missing).unwrap()).is_err());
    // Plus de 50 lignes stockées : refusé.
    let mut many = v.clone();
    let one = many["pieces"][0]["flares"][0].clone();
    let list = many["pieces"][0]["flares"].as_array_mut().unwrap();
    for k in 0..51 {
        let mut f = one.clone();
        f["id"] = json!(900_000 + k);
        list.push(f);
    }
    assert!(persist::from_json(&serde_json::to_string(&many).unwrap()).is_err());
    // Un fichier sans ampleur n'a pas la clé.
    let (q, _qs, _qp) = rect();
    assert!(!bytes(&q).contains("flares"));
    // pf-validate : 0 pour le fichier valide, 1 pour une ligne dégénérée.
    let dir = std::env::temp_dir().join(format!("pf-ampleur-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let ok = dir.join("ok.pforge.json");
    std::fs::write(&ok, &text).unwrap();
    let run = |path: &std::path::Path| {
        std::process::Command::new(env!("CARGO_BIN_EXE_pf-validate"))
            .arg(path)
            .output()
            .unwrap()
            .status
            .code()
    };
    assert_eq!(run(&ok), Some(0));
    let mut bad = v.clone();
    bad["pieces"][0]["flares"][0]["moved_x_mm"] =
        bad["pieces"][0]["flares"][0]["stay_x_mm"].clone();
    bad["pieces"][0]["flares"][0]["moved_y_mm"] =
        bad["pieces"][0]["flares"][0]["stay_y_mm"].clone();
    let badp = dir.join("bad.pforge.json");
    std::fs::write(&badp, serde_json::to_string(&bad).unwrap()).unwrap();
    assert_eq!(run(&badp), Some(1));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn ca7_un_ancien_fichier_sans_ampleur_est_relu_et_reecrit_a_l_identique() {
    let (p, _s, _piece) = rect();
    let text = bytes(&p);
    assert_eq!(text, bytes(&persist::from_json(&text).unwrap()));
    // Fichier du dépôt qui ne connaît pas l'ampleur.
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/piece-crans-droit-fil.pforge.json");
    let raw = std::fs::read_to_string(&path).unwrap();
    let loaded = persist::from_json(&raw).unwrap();
    assert!(!raw.contains("flares"));
    assert_eq!(persist::to_json(&loaded).unwrap(), raw);
}

#[test]
fn ca7_le_fichier_nomme_la_cle_flares_et_pas_de_cle_en_trop() {
    let (mut p, _s, piece) = rect();
    p.apply(flare_cmd(piece, vec![line(100.0, 30.0)])).unwrap();
    let v: Value = serde_json::from_str(&bytes(&p)).unwrap();
    let f = v["pieces"][0]["flares"][0].as_object().unwrap();
    let mut keys: Vec<&str> = f.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "hinge_x_mm",
            "hinge_y_mm",
            "id",
            "moved_x_mm",
            "moved_y_mm",
            "stay_x_mm",
            "stay_y_mm"
        ]
    );
}

// ----- CA8 : marge de couture, reports -----------------------------------------------------------

#[test]
fn ca8_la_marge_de_couture_se_calcule_sur_le_contour_ouvert() {
    let (mut p, _s, piece) = rect();
    p.apply(Command::SetSeamAllowance {
        piece,
        seam: Some(SeamAllowance {
            default_mm: 10.0,
            ..SeamAllowance::default()
        }),
    })
    .unwrap();
    p.apply(flare_cmd(piece, vec![line(100.0, 30.0)])).unwrap();
    let cut = p.piece_cut_contour(piece).unwrap();
    // Le contour de coupe est plus grand que le contour de couture et ne le recoupe pas.
    let seam_area = p.piece_contour(piece).unwrap().area_mm2;
    assert!(
        cut.area_mm2 > seam_area + 2_000.0,
        "{} / {seam_area}",
        cut.area_mm2
    );
    assert_eq!(
        p.piece(piece).unwrap().seam.as_ref().unwrap().default_mm,
        10.0
    );
}

#[test]
fn ca8_droit_fil_et_elements_suivent_leur_morceau_ou_sont_signales() {
    let (mut p, s, piece) = rect();
    // Droit-fil vertical à x = 150 (morceau tourné).
    let g = segment(&mut p, s, pt(150.0, 20.0), pt(150.0, 80.0));
    let outline = p.piece(piece).unwrap().outline.clone();
    p.apply(Command::SetPieceOutline {
        piece,
        outline,
        grainline: Some(g),
    })
    .unwrap();
    p.apply(flare_cmd(piece, vec![line(100.0, 30.0)])).unwrap();
    let gl = p.piece(piece).unwrap().grainline.expect("droit-fil repris");
    let th = theta(30.0, 100.0);
    let a = rot_cw((150.0, 20.0), (100.0, 0.0), th);
    let b = rot_cw((150.0, 80.0), (100.0, 0.0), th);
    match p.piece_sheet(piece).unwrap().shape(gl).unwrap() {
        Shape::Line(x, y) => {
            assert!(distance_mm(x, pt(a.0, a.1)) < 1e-9, "{x:?}");
            assert!(distance_mm(y, pt(b.0, b.1)) < 1e-9, "{y:?}");
        }
        other => panic!("{other:?}"),
    }
    // Droit-fil qui traverse la ligne : non repris, avec remarque.
    let (mut q, qs, qpiece) = rect();
    let g = segment(&mut q, qs, pt(20.0, 50.0), pt(180.0, 50.0));
    let outline = q.piece(qpiece).unwrap().outline.clone();
    q.apply(Command::SetPieceOutline {
        piece: qpiece,
        outline,
        grainline: Some(g),
    })
    .unwrap();
    let out = q.apply(flare_cmd(qpiece, vec![line(100.0, 30.0)])).unwrap();
    assert!(q.piece(qpiece).unwrap().grainline.is_none());
    assert!(
        out.notes.iter().any(|n| n.contains("ligne d'ampleur")),
        "{:?}",
        out.notes
    );
}

#[test]
fn ca8_une_copie_de_feuille_garde_des_identifiants_d_ampleur_uniques() {
    let (mut p, _s, piece) = rect();
    p.apply(flare_cmd(piece, vec![line(70.0, 20.0), line(130.0, 25.0)]))
        .unwrap();
    let sheet = p.piece_sheet(piece).unwrap().id;
    p.apply(Command::DuplicateSheet { sheet }).unwrap();
    let mut ids: Vec<u64> = p
        .pieces
        .iter()
        .flat_map(|q| q.flares.iter().map(|l| l.id.0))
        .collect();
    let n = ids.len();
    assert!(n >= 2);
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), n, "identifiants d'ampleur uniques");
    persist::from_json(&bytes(&p)).unwrap();
}
