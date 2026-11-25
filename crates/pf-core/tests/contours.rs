//! Contours fermés, périmètre et aire.

use pf_core::contour::{closed_contours, contour_from_entities, measure_contour};
use pf_core::*;
use pf_geometry::PointMm;

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

/// Polygone fermé passant par ces sommets ; renvoie les tracés créés.
fn polygon(p: &mut Project, sheet: SheetId, vertices: &[(f64, f64)]) -> Vec<EntityId> {
    let n = vertices.len();
    (0..n)
        .map(|i| {
            let (a, b) = (vertices[i], vertices[(i + 1) % n]);
            p.apply(Command::AddSegment {
                sheet,
                from: PointRef::New(pt(a.0, a.1)),
                to: PointRef::New(pt(b.0, b.1)),
            })
            .unwrap()
            .entities[0]
        })
        .collect()
}

#[test]
fn rectangle_600_par_450() {
    let mut p = Project::new("Contours");
    let sheet = p.sheets[0].id;
    polygon(
        &mut p,
        sheet,
        &[(0., 0.), (600., 0.), (600., 450.), (0., 450.)],
    );
    let s = p.sheet(sheet).unwrap();
    let contours = closed_contours(s);
    assert_eq!(contours.len(), 1);
    assert_eq!(contours[0].edges.len(), 4);
    assert_eq!(contours[0].role, Some(PathRole::Construction));
    let m = measure_contour(s, &contours[0]).unwrap();
    assert_eq!(m.perimeter_mm, 2100.0);
    assert_eq!(m.area_mm2, 270_000.0);
    assert!(!m.clockwise && !m.self_intersecting);
}

#[test]
fn contour_avec_courbe_de_l_exercice() {
    // AB, BC et la courbe A→C de l'exercice de référence forment un contour.
    let p = demo_project().unwrap();
    let s = &p.sheets[0];
    let contours = closed_contours(s);
    assert_eq!(contours.len(), 1);
    assert_eq!(contours[0].edges.len(), 3);
    let m = measure_contour(s, &contours[0]).unwrap();
    let curve = s.entities[2].id;
    let curve_len = measure::entity_length(s, curve).unwrap().value;
    assert!((m.perimeter_mm - (200.0 + curve_len)).abs() < 1e-9);
    // Aire recoupée par un polygone très fin, indépendamment de la formule exacte.
    let mut poly = vec![pt(0., 0.), pt(120., 0.), pt(120., 80.)];
    let geom = s.curve_geometry(curve).unwrap();
    poly.extend(geom.sample(100_000).into_iter().rev().skip(1));
    let reference = pf_geometry::polygon_signed_area_mm2(&poly).abs();
    assert!(
        (m.area_mm2 - reference).abs() < 1e-4,
        "{} vs {reference}",
        m.area_mm2
    );
    assert!(m.area_mm2 > 4800.0, "plus grand que le triangle ABC seul");
    assert!(!m.self_intersecting);
}

#[test]
fn chaines_ouvertes_embranchements_et_contours_multiples() {
    let mut p = Project::new("Cas");
    let sheet = p.sheets[0].id;
    // Chaîne ouverte en L.
    let open = p
        .apply(Command::AddSegment {
            sheet,
            from: PointRef::New(pt(0., 0.)),
            to: PointRef::New(pt(50., 0.)),
        })
        .unwrap();
    let l2 = p
        .apply(Command::AddSegment {
            sheet,
            from: PointRef::Existing(open.points[1]),
            to: PointRef::New(pt(50., 30.)),
        })
        .unwrap();
    assert!(closed_contours(p.sheet(sheet).unwrap()).is_empty());
    let err = contour_from_entities(p.sheet(sheet).unwrap(), &[open.entities[0], l2.entities[0]])
        .unwrap_err();
    assert!(err.to_string().contains("ouvert"), "{err}");

    // Deux triangles disjoints, plus loin.
    polygon(&mut p, sheet, &[(100., 0.), (140., 0.), (100., 30.)]);
    let t2 = polygon(&mut p, sheet, &[(200., 0.), (260., 0.), (200., 80.)]);
    let s = p.sheet(sheet).unwrap();
    let contours = closed_contours(s);
    assert_eq!(contours.len(), 2);
    let areas: Vec<f64> = contours
        .iter()
        .map(|c| measure_contour(s, c).unwrap().area_mm2)
        .collect();
    assert_eq!(areas, vec![600.0, 2400.0]);

    // Une diagonale ajoutée dans le second triangle crée un embranchement :
    // ce groupe n'est plus un contour simple, l'autre le reste.
    let (a, _) = s.entity_points(t2[0]).unwrap();
    p.apply(Command::AddSegment {
        sheet,
        from: PointRef::Existing(a),
        to: PointRef::New(pt(230., 40.)),
    })
    .unwrap();
    let s = p.sheet(sheet).unwrap();
    assert_eq!(closed_contours(s).len(), 1);
    // Mais la sélection des trois côtés seuls reste mesurable.
    let m = measure_contour(s, &contour_from_entities(s, &t2).unwrap()).unwrap();
    assert_eq!(m.area_mm2, 2400.0);
    assert!(contour_from_entities(s, &[t2[0], EntityId(424242)]).is_err());
    assert!(contour_from_entities(s, &[]).is_err());
}

#[test]
fn sens_horaire_et_auto_intersection() {
    let mut p = Project::new("Sens");
    let sheet = p.sheets[0].id;
    polygon(
        &mut p,
        sheet,
        &[(0., 0.), (0., 100.), (100., 100.), (100., 0.)],
    );
    let s = p.sheet(sheet).unwrap();
    let m = measure_contour(s, &closed_contours(s)[0]).unwrap();
    assert!(m.clockwise);
    assert_eq!(m.area_mm2, 10_000.0);

    // Nœud papillon : les côtés se croisent.
    let mut p = Project::new("Papillon");
    let sheet = p.sheets[0].id;
    polygon(
        &mut p,
        sheet,
        &[(0., 0.), (100., 100.), (100., 0.), (0., 100.)],
    );
    let s = p.sheet(sheet).unwrap();
    let m = measure_contour(s, &closed_contours(s)[0]).unwrap();
    assert!(m.self_intersecting);
}

#[test]
fn boucle_d_une_seule_courbe_et_role_commun() {
    let mut p = Project::new("Goutte");
    let sheet = p.sheets[0].id;
    let o = p
        .apply(Command::AddCurve {
            sheet,
            start: PointRef::New(pt(0., 0.)),
            c1: pt(80., 60.),
            c2: pt(-80., 60.),
            end: PointRef::New(pt(0., 0.)),
        })
        .unwrap();
    assert_eq!(
        o.points.len(),
        1,
        "départ et arrivée partagent le même point"
    );
    p.apply(Command::SetPathRole {
        sheet,
        entity: o.entities[0],
        role: PathRole::CuttingContour,
    })
    .unwrap();
    let s = p.sheet(sheet).unwrap();
    let contours = closed_contours(s);
    assert_eq!(contours.len(), 1);
    assert_eq!(contours[0].role, Some(PathRole::CuttingContour));
    let m = measure_contour(s, &contours[0]).unwrap();
    let geom = s.curve_geometry(o.entities[0]).unwrap();
    let reference = pf_geometry::polygon_signed_area_mm2(&geom.sample(100_000)).abs();
    assert!((m.area_mm2 - reference).abs() < 1e-4);
    assert!(m.area_mm2 > 100.0);
}
