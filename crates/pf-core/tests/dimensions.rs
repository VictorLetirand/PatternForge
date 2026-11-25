//! Cotations : mesure affichée vivante (non contrainte), validation, suppression, fichier.

use pf_core::*;
use pf_geometry::{distance_mm, PointMm, Transform};

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

/// Segment de (0,0) à (3,4), longueur 5.
fn base() -> (Project, SheetId, EntityId, Vec<PointId>) {
    let mut p = Project::new("Cotations");
    let sheet = p.sheets[0].id;
    let o = p
        .apply(Command::AddSegment {
            sheet,
            from: PointRef::New(pt(0., 0.)),
            to: PointRef::New(pt(3., 4.)),
        })
        .unwrap();
    (p, sheet, o.entities[0], o.points)
}

fn add(
    p: &mut Project,
    sheet: SheetId,
    from: PointId,
    to: PointId,
    off: f64,
) -> Result<Outcome, CoreError> {
    p.apply(Command::AddDimension {
        sheet,
        from,
        to,
        offset_mm: off,
    })
}

#[test]
fn valeur_exacte_et_points_decales() {
    let (mut p, sheet, _, pts) = base();
    let o = add(&mut p, sheet, pts[0], pts[1], 10.0).unwrap();
    let g = p
        .sheet(sheet)
        .unwrap()
        .dimension_geometry(o.dimensions[0])
        .unwrap();
    assert_eq!(g.value_mm, 5.0);
    // Gauche de (3,4)/5 = (-0.8, 0.6) ; décalage 10 -> (-8, 6).
    assert!(distance_mm(g.from_offset, pt(-8., 6.)) < 1e-9);
    assert!(distance_mm(g.to_offset, pt(-5., 10.)) < 1e-9);
    assert!(distance_mm(g.from, pt(0., 0.)) < 1e-9);
}

#[test]
fn decalage_negatif_va_a_droite_et_se_modifie() {
    let (mut p, sheet, _, pts) = base();
    let id = add(&mut p, sheet, pts[0], pts[1], -5.0).unwrap().dimensions[0];
    let g = p.sheet(sheet).unwrap().dimension_geometry(id).unwrap();
    assert!(distance_mm(g.from_offset, pt(4., -3.)) < 1e-9);
    p.apply(Command::SetDimensionOffset {
        sheet,
        dimension: id,
        offset_mm: 0.0,
    })
    .unwrap();
    let g = p.sheet(sheet).unwrap().dimension_geometry(id).unwrap();
    assert!(distance_mm(g.from_offset, g.from) < 1e-9);
}

#[test]
fn la_valeur_suit_le_deplacement_d_un_point() {
    let (mut p, sheet, _, pts) = base();
    let id = add(&mut p, sheet, pts[0], pts[1], 10.0).unwrap().dimensions[0];
    p.apply(Command::MovePoint {
        sheet,
        point: pts[1],
        to: pt(6., 8.),
    })
    .unwrap();
    assert_eq!(
        p.sheet(sheet)
            .unwrap()
            .dimension_geometry(id)
            .unwrap()
            .value_mm,
        10.0
    );
    // Transform : les deux points se déplacent, la valeur est conservée.
    p.apply(Command::Transform {
        sheet,
        points: pts.clone(),
        transform: Transform::Translate {
            dx_mm: 100.,
            dy_mm: 0.0,
        },
    })
    .unwrap();
    let g = p.sheet(sheet).unwrap().dimension_geometry(id).unwrap();
    assert_eq!(g.value_mm, 10.0);
    assert!(distance_mm(g.from, pt(100., 0.)) < 1e-9);
}

#[test]
fn refus_sans_effet() {
    let (mut p, sheet, _, pts) = base();
    let before = p.clone();
    assert!(add(&mut p, sheet, pts[0], pts[0], 10.0).is_err());
    assert!(add(&mut p, sheet, pts[0], PointId(9999), 10.0).is_err());
    assert!(add(&mut p, sheet, pts[0], pts[1], f64::NAN).is_err());
    assert!(add(&mut p, sheet, pts[0], pts[1], f64::INFINITY).is_err());
    assert!(add(&mut p, sheet, pts[0], pts[1], 10_001.0).is_err());
    assert_eq!(p, before);
    // Points confondus : refusé à la création.
    let o = p
        .apply(Command::AddSegment {
            sheet,
            from: PointRef::New(pt(50., 50.)),
            to: PointRef::New(pt(60., 50.)),
        })
        .unwrap();
    let id = add(&mut p, sheet, o.points[0], o.points[1], 1.0)
        .unwrap()
        .dimensions[0];
    let before = p.clone();
    assert!(p
        .apply(Command::SetDimensionOffset {
            sheet,
            dimension: id,
            offset_mm: f64::NAN
        })
        .is_err());
    assert!(p
        .apply(Command::DeleteDimension {
            sheet,
            dimension: DimensionId(9999)
        })
        .is_err());
    assert_eq!(p, before);
}

#[test]
fn supprimer_un_point_supprime_la_cotation() {
    let (mut p, sheet, ab, pts) = base();
    add(&mut p, sheet, pts[0], pts[1], 10.0).unwrap();
    let o = p
        .apply(Command::Delete {
            sheet,
            points: vec![],
            entities: vec![ab],
        })
        .unwrap();
    assert!(p.sheet(sheet).unwrap().dimensions.is_empty());
    assert!(o.notes.iter().any(|n| n.contains("cotation")));
}

#[test]
fn duplicate_ne_copie_pas_les_cotations() {
    let (mut p, sheet, ab, pts) = base();
    add(&mut p, sheet, pts[0], pts[1], 10.0).unwrap();
    p.apply(Command::Duplicate {
        sheet,
        entities: vec![ab],
        transform: Transform::Translate {
            dx_mm: 50.,
            dy_mm: 0.0,
        },
    })
    .unwrap();
    assert_eq!(p.sheet(sheet).unwrap().dimensions.len(), 1);
}

#[test]
fn fichier_aller_retour_et_champ_absent() {
    let (mut p, sheet, _, pts) = base();
    let clean = persist::to_json(&p).unwrap();
    assert!(!clean.contains("dimensions"));
    add(&mut p, sheet, pts[0], pts[1], 12.5).unwrap();
    let json = persist::to_json(&p).unwrap();
    assert!(json.contains("\"dimensions\""));
    let back = persist::from_json(&json).unwrap();
    assert_eq!(back, p);
    assert_eq!(persist::to_json(&back).unwrap(), json);
    // Référence morte, from = to, décalage invalide : refus à l'ouverture.
    let id = p.sheets[0].dimensions[0].id.0;
    let dead = json.replacen(&format!("\"to\": {}", pts[1].0), "\"to\": 4242", 1);
    assert!(persist::from_json(&dead).is_err());
    let same = json.replacen(
        &format!("\"to\": {}", pts[1].0),
        &format!("\"to\": {}", pts[0].0),
        1,
    );
    assert!(persist::from_json(&same).is_err());
    let far = json.replacen("\"offset_mm\": 12.5", "\"offset_mm\": 99999.0", 1);
    assert!(persist::from_json(&far).is_err());
    let _ = id;
}

#[test]
fn annulation() {
    let (p, sheet, _, pts) = base();
    let mut doc = Document::new(p);
    doc.execute(Command::AddDimension {
        sheet,
        from: pts[0],
        to: pts[1],
        offset_mm: 10.0,
    })
    .unwrap();
    assert_eq!(doc.project().sheet(sheet).unwrap().dimensions.len(), 1);
    assert!(doc.undo());
    assert!(doc.project().sheet(sheet).unwrap().dimensions.is_empty());
}
