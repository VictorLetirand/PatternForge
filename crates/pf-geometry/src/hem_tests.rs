use super::*;

fn near(a: &[f64], b: &[f64]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-9)
}

#[test]
fn distances_par_pas_nombre_et_bornes() {
    use FoldNotchPlan::{Count, Step};
    // L = 100 : pas 25 → 4 intervalles ; pas 30 → ⌈3,33⌉ = 4 intervalles ; pas 100 → 1.
    assert!(near(
        &fold_notch_distances(100.0, Step(25.0)).unwrap(),
        &[25.0, 50.0, 75.0]
    ));
    assert!(near(
        &fold_notch_distances(100.0, Step(30.0)).unwrap(),
        &[25.0, 50.0, 75.0]
    ));
    assert!(fold_notch_distances(100.0, Step(100.0)).unwrap().is_empty());
    assert!(fold_notch_distances(100.0, Step(1000.0))
        .unwrap()
        .is_empty());
    // Pas de 40 : ⌈2,5⌉ = 3 intervalles de 33,33.
    assert!(near(
        &fold_notch_distances(100.0, Step(40.0)).unwrap(),
        &[100.0 / 3.0, 200.0 / 3.0]
    ));
    assert!(near(
        &fold_notch_distances(100.0, Count(1)).unwrap(),
        &[50.0]
    ));
    assert!(near(
        &fold_notch_distances(100.0, Count(3)).unwrap(),
        &[25.0, 50.0, 75.0]
    ));
    // 1000 crans acceptés, 1001 refusé ; pas hors bornes ; longueur nulle.
    assert_eq!(
        fold_notch_distances(100.0, Count(1000)).unwrap().len(),
        1000
    );
    assert!(fold_notch_distances(100.0, Count(1001)).is_err());
    assert!(fold_notch_distances(100.0, Count(0)).is_err());
    assert!(fold_notch_distances(100.0, Step(0.99)).is_err());
    assert!(fold_notch_distances(100.0, Step(1000.1)).is_err());
    assert!(fold_notch_distances(100.0, Step(f64::NAN)).is_err());
    assert!(fold_notch_distances(0.0, Count(1)).is_err());
    assert!(fold_notch_distances(f64::INFINITY, Count(1)).is_err());
    // 1001 intervalles = 1000 crans : accepté ; 1002 intervalles : refusé.
    assert_eq!(fold_notch_distances(1001.0, Step(1.0)).unwrap().len(), 1000);
    assert!(fold_notch_distances(1002.0, Step(1.0)).is_err());
}

#[test]
fn crans_sur_deux_segments_consecutifs() {
    // (0,0)→(60,0) puis (60,0)→(60,40) : L = 100, 3 crans à 25, 50, 75 :
    // (25 ; 0), (50 ; 0), (60 ; 15) ; normales (0 ; −1), (0 ; −1), (1 ; 0).
    let edges = [
        EdgeGeom::Line(PointMm::new(0.0, 0.0), PointMm::new(60.0, 0.0)),
        EdgeGeom::Line(PointMm::new(60.0, 0.0), PointMm::new(60.0, 40.0)),
    ];
    let n = fold_notches(&edges, FoldNotchPlan::Count(3)).unwrap();
    let want = [
        (25.0, 0.0, 0.0, -1.0),
        (50.0, 0.0, 0.0, -1.0),
        (60.0, 15.0, 1.0, 0.0),
    ];
    assert_eq!(n.len(), 3);
    for (k, w) in n.iter().zip(want) {
        assert!((k.point.x - w.0).abs() < 1e-9 && (k.point.y - w.1).abs() < 1e-9);
        assert!((k.normal.x - w.2).abs() < 1e-9 && (k.normal.y - w.3).abs() < 1e-9);
    }
}
