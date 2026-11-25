//! Tests des types de coins (US-05.2). Les coordonnées attendues sont calculées à la main
//! (commentaires) ; aucune n'est relue du code testé.
//!
//! Parallélogramme (0,0) (100,0) (160,80) (60,80), anti-horaire, marge 10 : les directions de
//! couture sont (1,0), (0,6 ; 0,8), (-1,0), (-0,6 ; -0,8), donc cos = 0,6 et sin = 0,8
//! (rationnels). Le sommet 1 = (100,0) est un virage de 53,13° (coin obtus), le sommet 0 = (0,0)
//! un virage de 126,87° (coin aigu). Onglets calculés à la main (V + (n1 + n2) · d / (1 + n1·n2)) :
//! X0 = (-20,-10), X1 = (105,-10), X2 = (180,90), X3 = (55,90).
//! Au sommet 1 : n1 = (0,-1), n2 = (0,8 ; -0,6), E = (100,-10), S = (108,-6), L2 = S + μ (0,6 ; 0,8).

use super::*;

fn p(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

fn poly(pts: &[(f64, f64)]) -> Vec<EdgeGeom> {
    (0..pts.len())
        .map(|i| {
            let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
            EdgeGeom::Line(p(a.0, a.1), p(b.0, b.1))
        })
        .collect()
}

fn rect(w: f64, h: f64) -> Vec<EdgeGeom> {
    poly(&[(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)])
}

fn para() -> Vec<EdgeGeom> {
    poly(&[(0.0, 0.0), (100.0, 0.0), (160.0, 80.0), (60.0, 80.0)])
}

fn uniform(n: usize, mm: f64) -> Vec<EdgeMargin> {
    vec![EdgeMargin::uniform(mm); n]
}

fn vertices(traces: &[Trace]) -> Vec<PointMm> {
    traces
        .iter()
        .map(|t| match t {
            Trace::Line(a, _) => *a,
            other => panic!("segment attendu, reçu {other:?}"),
        })
        .collect()
}

fn assert_cycle(got: &[PointMm], want: &[(f64, f64)], tol: f64) {
    assert_eq!(got.len(), want.len(), "nombre de sommets : {got:?}");
    let n = want.len();
    let ok = (0..n).any(|shift| {
        (0..n).all(|i| {
            let (g, w) = (got[(i + shift) % n], want[i]);
            (g.x - w.0).abs() <= tol && (g.y - w.1).abs() <= tol
        })
    });
    assert!(ok, "sommets {got:?} ≠ attendus {want:?}");
}

fn has(v: &[PointMm], w: (f64, f64)) -> bool {
    v.iter()
        .any(|q| (q.x - w.0).abs() < 1e-9 && (q.y - w.1).abs() < 1e-9)
}

fn assert_adjacent(v: &[PointMm], a: (f64, f64), b: (f64, f64)) {
    let n = v.len();
    let near = |q: PointMm, w: (f64, f64)| (q.x - w.0).abs() < 1e-9 && (q.y - w.1).abs() < 1e-9;
    assert!(
        (0..n).any(|i| near(v[i], a) && near(v[(i + 1) % n], b)),
        "{a:?} puis {b:?} attendus dans {v:?}"
    );
}

fn with(
    chain: &[EdgeGeom],
    margins: &[EdgeMargin],
    corners: &[(usize, CornerKind)],
    max_tip_mm: Option<f64>,
) -> Result<Vec<Trace>> {
    let options = SeamOptions {
        corners: corners
            .iter()
            .map(|&(vertex, kind)| CornerSpec { vertex, kind })
            .collect(),
        max_tip_mm,
    };
    seam_contour_with(chain, margins, &options)
}

fn at_vertex_1(kind: CornerKind) -> Vec<PointMm> {
    vertices(&with(&para(), &uniform(4, 10.0), &[(1, kind)], None).unwrap())
}

#[test]
fn coin_onglet_par_defaut_sur_le_parallelogramme() {
    let cut = seam_contour(&para(), &uniform(4, 10.0)).unwrap();
    assert_cycle(
        &vertices(&cut),
        &[(-20.0, -10.0), (105.0, -10.0), (180.0, 90.0), (55.0, 90.0)],
        1e-9,
    );
}

#[test]
fn coin_perpendiculaire_a_la_couture_precedente() {
    // λ = (S - E)·(1,0) = 8 : L1 est prolongée jusqu'à (108,-10), puis la perpendiculaire monte à
    // S = (108,-6), puis L2 jusqu'à X2.
    assert_cycle(
        &at_vertex_1(CornerKind::PerpendicularPrevious),
        &[
            (-20.0, -10.0),
            (108.0, -10.0),
            (108.0, -6.0),
            (180.0, 90.0),
            (55.0, 90.0),
        ],
        1e-9,
    );
}

#[test]
fn coin_perpendiculaire_a_la_couture_suivante() {
    // μ = (E - S)·(0,6 ; 0,8) = (-8 ; -4)·(0,6 ; 0,8) = -8 : pied en S - 8 (0,6 ; 0,8) =
    // (103,2 ; -12,4) ; le segment E → pied (3,2 ; -2,4) est perpendiculaire à (0,6 ; 0,8).
    assert_cycle(
        &at_vertex_1(CornerKind::PerpendicularNext),
        &[
            (-20.0, -10.0),
            (100.0, -10.0),
            (103.2, -12.4),
            (180.0, 90.0),
            (55.0, 90.0),
        ],
        1e-9,
    );
}

#[test]
fn coin_symetrique_a_marges_egales_relie_les_deux_bouts() {
    // E et S sont à la même hauteur sur la bissectrice (1,-2)/√5 : coupe E → S.
    assert_cycle(
        &at_vertex_1(CornerKind::Symmetric),
        &[
            (-20.0, -10.0),
            (100.0, -10.0),
            (108.0, -6.0),
            (180.0, 90.0),
            (55.0, 90.0),
        ],
        1e-9,
    );
}

#[test]
fn coin_symetrique_a_marges_differentes_passe_par_le_bout_le_plus_haut() {
    // Marges 10 (arête 0) et 15 (arête 1) : E = (100,-10), S = (100,0) + 15 (0,8 ; -0,6) =
    // (112,-9). Hauteurs sur (1,-2)/√5 : E 20/√5, S 30/√5 ; la coupe passe par S et coupe L1
    // (y = -10) à x = 110 : (110,-10) → (112,-9), perpendiculaire à (1,-2) car (2,1)·(1,-2) = 0.
    // (La pointe de l'onglet, (111,25 ; -10), est au-delà de la coupe.)
    let m = [
        EdgeMargin::uniform(10.0),
        EdgeMargin::uniform(15.0),
        EdgeMargin::uniform(10.0),
        EdgeMargin::uniform(10.0),
    ];
    let v = vertices(&with(&para(), &m, &[(1, CornerKind::Symmetric)], None).unwrap());
    assert_adjacent(&v, (110.0, -10.0), (112.0, -9.0));
}

#[test]
fn coin_tronque_a_une_profondeur_donnee() {
    // Profondeur √5 / 2 sous la pointe (105,-10) : la coupe est la droite (p - V)·(1,-2) = 22,5.
    // Sur L1 (y = -10) : x = 102,5 ; sur L2 : μ = -2,5, soit (106,5 ; -8).
    assert_cycle(
        &at_vertex_1(CornerKind::Truncated {
            depth_mm: 0.5 * 5f64.sqrt(),
        }),
        &[
            (-20.0, -10.0),
            (102.5, -10.0),
            (106.5, -8.0),
            (180.0, 90.0),
            (55.0, 90.0),
        ],
        1e-9,
    );
}

#[test]
fn coin_tronque_de_profondeur_nulle_est_l_onglet_sans_limite_de_pointe() {
    // Le sommet 0 (aigu) a une pointe à 22,36 mm de V ; avec une limite de 20 mm l'onglet serait
    // coupé en biseau, pas le coin tronqué de profondeur 0 (le sommet 2, lui, est bien coupé :
    // E2 = (168,74), S2 = (160,90)).
    let cut = with(
        &para(),
        &uniform(4, 10.0),
        &[(0, CornerKind::Truncated { depth_mm: 0.0 })],
        Some(20.0),
    )
    .unwrap();
    assert_cycle(
        &vertices(&cut),
        &[
            (-20.0, -10.0),
            (105.0, -10.0),
            (168.0, 74.0),
            (160.0, 90.0),
            (55.0, 90.0),
        ],
        1e-9,
    );
}

#[test]
fn coin_en_marche_passe_par_le_sommet_du_parallelogramme_des_marges() {
    // M = V + 10 n1 + 10 n2 = (100,0) + (0,-10) + (8,-6) = (108,-16) ; E → M ∥ n2, M → S ∥ n1.
    assert_cycle(
        &at_vertex_1(CornerKind::Stepped),
        &[
            (-20.0, -10.0),
            (100.0, -10.0),
            (108.0, -16.0),
            (108.0, -6.0),
            (180.0, 90.0),
            (55.0, 90.0),
        ],
        1e-9,
    );
}

#[test]
fn coin_ourlet_rabattu_pli_sur_la_couture_suivante() {
    // Pli = couture suivante (100 + 0,6 k ; 0,8 k). L1 (y = -10) la coupe en k = -12,5 :
    // F = (92,5 ; -10). Le miroir de (1,0) par rapport à (0,6 ; 0,8) est (0,28 ; -0,96) ; à 10 mm
    // du pli côté extérieur (n2 = (0,8 ; -0,6)) : F + (0,28 ; -0,96) · 12,5 = (96 ; -22),
    // contrôle : (-4 ; -22)·n2 = -3,2 + 13,2 = 10. Puis L2 : (180 ; 90) - (96 ; -22) = 140 (0,6 ; 0,8).
    assert_cycle(
        &at_vertex_1(CornerKind::HemFoldNext),
        &[
            (-20.0, -10.0),
            (92.5, -10.0),
            (96.0, -22.0),
            (180.0, 90.0),
            (55.0, 90.0),
        ],
        1e-9,
    );
}

#[test]
fn coin_ourlet_rabattu_pli_sur_la_couture_precedente() {
    // Pli = couture précédente (y = 0). L2 le coupe en μ = 7,5 : (112,5 ; 0). Le miroir de
    // (0,6 ; 0,8) est (0,6 ; -0,8) ; à 10 mm du pli (côté y < 0) :
    // (112,5 ; 0) + (0,6 ; -0,8) · 12,5 = (120 ; -10), qui est bien sur L1.
    assert_cycle(
        &at_vertex_1(CornerKind::HemFoldPrevious),
        &[
            (-20.0, -10.0),
            (120.0, -10.0),
            (112.5, 0.0),
            (180.0, 90.0),
            (55.0, 90.0),
        ],
        1e-9,
    );
}

#[test]
fn les_types_de_coins_se_confondent_avec_l_onglet_a_angle_droit() {
    // Rectangle : tous les coins valent l'onglet (110,-10).
    for kind in [
        CornerKind::PerpendicularPrevious,
        CornerKind::PerpendicularNext,
        CornerKind::Stepped,
        CornerKind::HemFoldNext,
        CornerKind::HemFoldPrevious,
        CornerKind::Truncated { depth_mm: 0.0 },
    ] {
        let cut = with(&rect(100.0, 60.0), &uniform(4, 10.0), &[(1, kind)], None).unwrap();
        assert_cycle(
            &vertices(&cut),
            &[(-10.0, -10.0), (110.0, -10.0), (110.0, 70.0), (-10.0, 70.0)],
            1e-9,
        );
    }
}

#[test]
fn coin_choisi_par_sommet_les_autres_restent_en_onglet() {
    // Sommet 1 en perpendiculaire, sommet 2 en marche. Sommet 2 : V = (160,80), n1 = (0,8 ; -0,6),
    // n2 = (0,1) : M = V + 10 n1 + 10 n2 = (168 ; 84). Les sommets 0 et 3 gardent leur onglet.
    let cut = with(
        &para(),
        &uniform(4, 10.0),
        &[
            (1, CornerKind::PerpendicularPrevious),
            (2, CornerKind::Stepped),
        ],
        None,
    )
    .unwrap();
    let v = vertices(&cut);
    assert_adjacent(&v, (108.0, -10.0), (108.0, -6.0));
    assert!(has(&v, (168.0, 84.0)));
    assert!(has(&v, (-20.0, -10.0)));
    assert!(has(&v, (55.0, 90.0)));
}

#[test]
fn pointe_tres_aigue_bornee_par_la_longueur_maximale() {
    // Sommets 0 et 2 (aigus) : pointe à √(20² + 10²) = 22,36 mm de V. Limite 20 : biseaux
    // E0 = (-8,6) → S0 = (0,-10) (E0 = V + 10 (-0,8 ; 0,6), S0 = V + 10 (0,-1)) et
    // E2 = (160,80) + 10 (0,8 ; -0,6) = (168,74) → S2 = (160,80) + 10 (0,1) = (160,90).
    let cut = with(&para(), &uniform(4, 10.0), &[], Some(20.0)).unwrap();
    assert_cycle(
        &vertices(&cut),
        &[
            (55.0, 90.0),
            (-8.0, 6.0),
            (0.0, -10.0),
            (105.0, -10.0),
            (168.0, 74.0),
            (160.0, 90.0),
        ],
        1e-9,
    );
    // Limite 30 : onglet conservé.
    let cut = with(&para(), &uniform(4, 10.0), &[], Some(30.0)).unwrap();
    assert_cycle(
        &vertices(&cut),
        &[(-20.0, -10.0), (105.0, -10.0), (180.0, 90.0), (55.0, 90.0)],
        1e-9,
    );
}

#[test]
fn longueur_maximale_aussi_avec_un_type_de_coin_sur_un_autre_sommet() {
    // Chemin « arête par arête » (un coin choisi) : même biseau au sommet 0 ; sommet 1 en marche.
    let cut = with(
        &para(),
        &uniform(4, 10.0),
        &[(1, CornerKind::Stepped)],
        Some(20.0),
    )
    .unwrap();
    let v = vertices(&cut);
    assert_adjacent(&v, (-8.0, 6.0), (0.0, -10.0));
    assert_adjacent(&v, (100.0, -10.0), (108.0, -16.0));
}

#[test]
fn limite_par_defaut_quatre_fois_la_marge_inchangee() {
    // Pointe à 22,36 < 4 × 10 : onglet sans limite explicite, y compris par le chemin par arête.
    assert_eq!(SEAM_MITER_LIMIT, 4.0);
    let cut = with(
        &para(),
        &uniform(4, 10.0),
        &[(1, CornerKind::Stepped)],
        None,
    )
    .unwrap();
    assert!(has(&vertices(&cut), (-20.0, -10.0)));
}

fn l_shape() -> Vec<EdgeGeom> {
    poly(&[
        (0.0, 0.0),
        (100.0, 0.0),
        (100.0, 50.0),
        (50.0, 50.0),
        (50.0, 100.0),
        (0.0, 100.0),
    ])
}

fn split_rect() -> Vec<EdgeGeom> {
    poly(&[
        (0.0, 0.0),
        (50.0, 0.0),
        (100.0, 0.0),
        (100.0, 60.0),
        (0.0, 60.0),
    ])
}

#[test]
fn nature_des_sommets() {
    assert_eq!(vertex_turn(&l_shape(), 0).unwrap(), VertexTurn::Convex);
    assert_eq!(vertex_turn(&l_shape(), 3).unwrap(), VertexTurn::Reentrant);
    assert!(vertex_turn(&l_shape(), 6).is_err());
    assert_eq!(vertex_turn(&split_rect(), 1).unwrap(), VertexTurn::Smooth);
}

#[test]
fn coins_impossibles_expliques() {
    let m = uniform(4, 10.0);
    let r = rect(100.0, 60.0);
    for (corners, max) in [
        (vec![(4, CornerKind::Stepped)], None),
        (
            vec![(1, CornerKind::Stepped), (1, CornerKind::Symmetric)],
            None,
        ),
        (vec![(1, CornerKind::Truncated { depth_mm: -1.0 })], None),
        (
            vec![(1, CornerKind::Truncated { depth_mm: f64::NAN })],
            None,
        ),
        (vec![], Some(-1.0)),
        (vec![], Some(f64::INFINITY)),
        (vec![], Some(MAX_CORNER_MM + 1.0)),
    ] {
        assert!(with(&r, &m, &corners, max).is_err(), "{corners:?} {max:?}");
    }
    let e = with(
        &split_rect(),
        &uniform(5, 10.0),
        &[(1, CornerKind::Stepped)],
        None,
    )
    .unwrap_err()
    .to_string();
    assert!(e.contains("lisse"), "{e}");
    let e = with(
        &l_shape(),
        &uniform(6, 5.0),
        &[(3, CornerKind::Stepped)],
        None,
    )
    .unwrap_err()
    .to_string();
    assert!(e.contains("rentrant"), "{e}");
    // Tronqué beaucoup trop profond : la coupe dépasse les bords, erreur et non un contour faux.
    let deep = with(
        &r,
        &m,
        &[(1, CornerKind::Truncated { depth_mm: 5000.0 })],
        None,
    );
    assert!(deep.is_err());
}

#[test]
fn coin_choisi_sur_marge_nulle_est_sans_effet() {
    let cut = with(
        &rect(100.0, 60.0),
        &uniform(4, 0.0),
        &[(1, CornerKind::Stepped)],
        None,
    )
    .unwrap();
    assert_cycle(
        &vertices(&cut),
        &[(0.0, 0.0), (100.0, 0.0), (100.0, 60.0), (0.0, 60.0)],
        1e-12,
    );
}
