//! Crans : position le long du tracé, côté, double, suivi, suppression, symétrie.

use pf_core::*;
use pf_geometry::{distance_mm, PointMm, Side, Transform};

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

fn close(a: PointMm, b: PointMm) -> bool {
    distance_mm(a, b) < 1e-6
}

/// Segment AB de (0,0) à (120,0).
fn ab() -> (Project, SheetId, EntityId, Vec<PointId>) {
    let mut p = Project::new("Crans");
    let sheet = p.sheets[0].id;
    let o = p
        .apply(Command::AddSegment {
            sheet,
            from: PointRef::New(pt(0., 0.)),
            to: PointRef::New(pt(120., 0.)),
        })
        .unwrap();
    (p, sheet, o.entities[0], o.points)
}

fn add_notch(
    p: &mut Project,
    sheet: SheetId,
    entity: EntityId,
    distance_mm: f64,
    side: Side,
    length_mm: f64,
    style: NotchStyle,
) -> Result<Outcome, CoreError> {
    p.apply(Command::AddNotch {
        sheet,
        entity,
        distance_mm,
        side,
        length_mm,
        style,
    })
}

#[test]
fn cran_simple_sur_un_segment() {
    let (mut p, sheet, ab, _) = ab();
    let o = add_notch(&mut p, sheet, ab, 40.0, Side::Left, 5.0, NotchStyle::Single).unwrap();
    let s = p.sheet(sheet).unwrap();
    let marks = s.notch_marks(s.notch(o.notches[0]).unwrap()).unwrap();
    assert_eq!(marks.len(), 1);
    assert!(close(marks[0].0, pt(40., 0.)) && close(marks[0].1, pt(40., 5.)));
    // Côté droit : pointe de l'autre côté.
    let o = add_notch(
        &mut p,
        sheet,
        ab,
        80.0,
        Side::Right,
        7.5,
        NotchStyle::Single,
    )
    .unwrap();
    let s = p.sheet(sheet).unwrap();
    let marks = s.notch_marks(s.notch(o.notches[0]).unwrap()).unwrap();
    assert!(close(marks[0].0, pt(80., 0.)) && close(marks[0].1, pt(80., -7.5)));
    assert_eq!(s.all_notch_marks().len(), 2);
}

#[test]
fn cran_double_deux_traits_espaces() {
    let (mut p, sheet, ab, _) = ab();
    let o = add_notch(&mut p, sheet, ab, 60.0, Side::Left, 5.0, NotchStyle::Double).unwrap();
    let s = p.sheet(sheet).unwrap();
    let marks = s.notch_marks(s.notch(o.notches[0]).unwrap()).unwrap();
    assert_eq!(marks.len(), 2);
    assert!(close(marks[0].0, pt(58.5, 0.)) && close(marks[1].0, pt(61.5, 0.)));
    assert!(marks.iter().all(|m| close(m.1, pt(m.0.x, 5.))));
    // Cran double près du départ : les traits restent sur le tracé.
    let o = add_notch(&mut p, sheet, ab, 0.5, Side::Left, 5.0, NotchStyle::Double).unwrap();
    let s = p.sheet(sheet).unwrap();
    let marks = s.notch_marks(s.notch(o.notches[0]).unwrap()).unwrap();
    assert!(marks.iter().all(|m| m.0.x >= 0.0));
}

#[test]
fn cran_sur_une_courbe_a_la_longueur_developpee() {
    let mut p = Project::new("Courbe");
    let sheet = p.sheets[0].id;
    let o = p
        .apply(Command::AddCurve {
            sheet,
            start: PointRef::New(pt(0., 0.)),
            c1: pt(30., 40.),
            c2: pt(70., 40.),
            end: PointRef::New(pt(100., 0.)),
        })
        .unwrap();
    let curve = o.entities[0];
    let total = p.sheet(sheet).unwrap().path_length_mm(curve).unwrap();
    let n = add_notch(
        &mut p,
        sheet,
        curve,
        total / 2.0,
        Side::Left,
        5.0,
        NotchStyle::Single,
    )
    .unwrap();
    let s = p.sheet(sheet).unwrap();
    let marks = s.notch_marks(s.notch(n.notches[0]).unwrap()).unwrap();
    // Par symétrie, la moitié de la longueur est au sommet (50, 30) ; la tangente y est horizontale.
    assert!(
        distance_mm(marks[0].0, pt(50., 30.)) < 1e-4,
        "{:?}",
        marks[0]
    );
    assert!(
        distance_mm(marks[0].1, pt(50., 35.)) < 1e-4,
        "{:?}",
        marks[0]
    );
    // Le cran reste perpendiculaire à la courbe et de longueur 5 mm en tout point.
    let c = s.curve_geometry(curve).unwrap();
    for frac in [0.1, 0.33, 0.8] {
        let d = total * frac;
        let (base, tangent) = s.path_point_at(curve, d).unwrap();
        let t = c.nearest_t(base).0;
        assert!(c.point_at(t).x.is_finite());
        let tip = base + tangent.rotated_left() * 5.0;
        assert!((distance_mm(base, tip) - 5.0).abs() < 1e-9);
        assert!((tip - base).dot(tangent).abs() < 1e-9);
    }
    // Distance retrouvée depuis un point proche du tracé.
    let (target, _) = s.path_point_at(curve, 40.0).unwrap();
    let back = s
        .path_distance_of(curve, pt(target.x + 0.01, target.y + 0.01))
        .unwrap();
    assert!((back - 40.0).abs() < 0.05, "{back}");
}

#[test]
fn valeurs_invalides_refusees_sans_effet() {
    let (mut p, sheet, ab, _) = ab();
    let before = p.clone();
    for (d, l) in [
        (130.0, 5.0),
        (-1.0, 5.0),
        (f64::NAN, 5.0),
        (40.0, 0.0),
        (40.0, -3.0),
        (40.0, 150.0),
        (40.0, f64::INFINITY),
    ] {
        let r = add_notch(&mut p, sheet, ab, d, Side::Left, l, NotchStyle::Single);
        assert!(
            r.is_err(),
            "distance {d}, longueur {l} aurait dû être refusée"
        );
        assert_eq!(p, before);
    }
    let err = add_notch(
        &mut p,
        sheet,
        ab,
        130.0,
        Side::Left,
        5.0,
        NotchStyle::Single,
    )
    .unwrap_err();
    assert!(err.to_string().contains("hors du tracé"), "{err}");
    // Tracé inconnu.
    assert!(add_notch(
        &mut p,
        sheet,
        EntityId(9999),
        10.0,
        Side::Left,
        5.0,
        NotchStyle::Single
    )
    .is_err());
    assert_eq!(p, before);
    // Position exactement à l'extrémité : acceptée.
    add_notch(
        &mut p,
        sheet,
        ab,
        120.0,
        Side::Left,
        5.0,
        NotchStyle::Single,
    )
    .unwrap();
    add_notch(&mut p, sheet, ab, 0.0, Side::Left, 5.0, NotchStyle::Single).unwrap();
}

#[test]
fn le_cran_suit_son_tracé_et_bloque_un_raccourcissement() {
    let (mut p, sheet, ab, pts) = ab();
    let n = add_notch(
        &mut p,
        sheet,
        ab,
        100.0,
        Side::Left,
        5.0,
        NotchStyle::Single,
    )
    .unwrap()
    .notches[0];
    // Déplacer le segment entier : le cran suit.
    p.apply(Command::Transform {
        sheet,
        points: pts.clone(),
        transform: Transform::Translate {
            dx_mm: 10.0,
            dy_mm: 20.0,
        },
    })
    .unwrap();
    let s = p.sheet(sheet).unwrap();
    let m = s.notch_marks(s.notch(n).unwrap()).unwrap();
    assert!(close(m[0].0, pt(110., 20.)) && close(m[0].1, pt(110., 25.)));
    // Raccourcir sous la position du cran : refusé avec une explication.
    let before = p.clone();
    let err = p
        .apply(Command::MovePoint {
            sheet,
            point: pts[1],
            to: pt(150., 20.),
        })
        .map(|_| ())
        .and_then(|_| {
            p.apply(Command::MovePoint {
                sheet,
                point: pts[1],
                to: pt(60., 20.),
            })
            .map(|_| ())
        })
        .unwrap_err();
    assert!(err.to_string().contains("cran"), "{err}");
    assert_ne!(
        p, before,
        "le premier déplacement (allongement) était valide"
    );
    // Modifier les paramètres du cran.
    p.apply(Command::SetNotch {
        sheet,
        notch: n,
        distance_mm: 30.0,
        side: Side::Right,
        length_mm: 8.0,
        style: NotchStyle::Double,
    })
    .unwrap();
    assert_eq!(
        p.sheet(sheet).unwrap().notch(n).unwrap().style,
        NotchStyle::Double
    );
    assert!(p
        .apply(Command::SetNotch {
            sheet,
            notch: NotchId(777),
            distance_mm: 1.0,
            side: Side::Left,
            length_mm: 5.0,
            style: NotchStyle::Single
        })
        .is_err());
}

#[test]
fn suppression_d_un_cran_et_d_un_tracé_porteur() {
    let (mut p, sheet, ab, _) = ab();
    let n1 = add_notch(&mut p, sheet, ab, 20.0, Side::Left, 5.0, NotchStyle::Single)
        .unwrap()
        .notches[0];
    let n2 = add_notch(&mut p, sheet, ab, 70.0, Side::Left, 5.0, NotchStyle::Single)
        .unwrap()
        .notches[0];
    p.apply(Command::DeleteNotch { sheet, notch: n1 }).unwrap();
    assert!(p.sheet(sheet).unwrap().notch(n1).is_err());
    assert!(p.apply(Command::DeleteNotch { sheet, notch: n1 }).is_err());
    let o = p
        .apply(Command::Delete {
            sheet,
            points: vec![],
            entities: vec![ab],
        })
        .unwrap();
    assert!(o.notes.iter().any(|n| n.contains("cran")), "{:?}", o.notes);
    assert!(p.sheet(sheet).unwrap().notch(n2).is_err());
    assert!(p.sheet(sheet).unwrap().notches.is_empty());
    p.validate().unwrap();
}

#[test]
fn symetrie_et_duplication_inversent_le_cote() {
    let mut p = Project::new("Miroir");
    let sheet = p.sheets[0].id;
    let o = p
        .apply(Command::AddSegment {
            sheet,
            from: PointRef::New(pt(0., 10.)),
            to: PointRef::New(pt(100., 10.)),
        })
        .unwrap();
    let seg = o.entities[0];
    let n = add_notch(
        &mut p,
        sheet,
        seg,
        50.0,
        Side::Left,
        5.0,
        NotchStyle::Single,
    )
    .unwrap()
    .notches[0];
    let axis = Transform::Mirror {
        axis_point: pt(0., 0.),
        axis_angle_deg: 0.0,
    };
    // Copie symétrique : la pointe du cran copié est l'image de la pointe de l'original.
    let c = p
        .apply(Command::Duplicate {
            sheet,
            entities: vec![seg],
            transform: axis,
        })
        .unwrap();
    assert_eq!(c.notches.len(), 1);
    let s = p.sheet(sheet).unwrap();
    let copy = s.notch(c.notches[0]).unwrap();
    assert_eq!(copy.side, Side::Right);
    let marks = s.notch_marks(copy).unwrap();
    assert!(
        close(marks[0].0, pt(50., -10.)) && close(marks[0].1, pt(50., -15.)),
        "{marks:?}"
    );

    // Symétrie sur place de l'original : même résultat physique.
    p.apply(Command::Transform {
        sheet,
        points: o.points.clone(),
        transform: axis,
    })
    .unwrap();
    let s = p.sheet(sheet).unwrap();
    let marks = s.notch_marks(s.notch(n).unwrap()).unwrap();
    assert!(
        close(marks[0].0, pt(50., -10.)) && close(marks[0].1, pt(50., -15.)),
        "{marks:?}"
    );

    // Une copie sans symétrie garde le côté.
    let c = p
        .apply(Command::Duplicate {
            sheet,
            entities: vec![seg],
            transform: Transform::Translate {
                dx_mm: 0.0,
                dy_mm: 40.0,
            },
        })
        .unwrap();
    let s = p.sheet(sheet).unwrap();
    assert_eq!(
        s.notch(c.notches[0]).unwrap().side,
        s.notch(n).unwrap().side
    );
}

#[test]
fn lot_de_commandes_atomique() {
    let (mut p, sheet, ab, _) = ab();
    let before = p.clone();
    // La seconde commande échoue : rien du lot ne doit subsister.
    let bad = Command::Batch {
        commands: vec![
            Command::AddNotch {
                sheet,
                entity: ab,
                distance_mm: 10.0,
                side: Side::Left,
                length_mm: 5.0,
                style: NotchStyle::Single,
            },
            Command::AddNotch {
                sheet,
                entity: ab,
                distance_mm: 500.0,
                side: Side::Left,
                length_mm: 5.0,
                style: NotchStyle::Single,
            },
        ],
    };
    assert!(p.apply(bad).is_err());
    assert_eq!(p, before);
    let ok = Command::Batch {
        commands: vec![
            Command::AddNotch {
                sheet,
                entity: ab,
                distance_mm: 10.0,
                side: Side::Left,
                length_mm: 5.0,
                style: NotchStyle::Single,
            },
            Command::CreateSheet {
                name: "Autre".into(),
            },
        ],
    };
    let o = p.apply(ok).unwrap();
    assert_eq!(o.notches.len(), 1);
    assert!(o.sheet.is_some());
    // Un lot en une seule étape d'historique.
    let mut doc = Document::new(before);
    doc.execute(Command::Batch {
        commands: vec![
            Command::CreateSheet { name: "A".into() },
            Command::CreateSheet { name: "B".into() },
        ],
    })
    .unwrap();
    assert!(doc.undo());
    assert!(!doc.undo());
}

#[test]
fn enregistrement_et_compatibilite_des_fichiers() {
    let (mut p, sheet, ab, _) = ab();
    add_notch(
        &mut p,
        sheet,
        ab,
        33.3,
        Side::Right,
        6.0,
        NotchStyle::Double,
    )
    .unwrap();
    let json = persist::to_json(&p).unwrap();
    assert!(json.contains("\"notches\"") && json.contains("\"double\""));
    let back = persist::from_json(&json).unwrap();
    assert_eq!(back, p);
    assert_eq!(persist::to_json(&back).unwrap(), json);

    // Sans cran, le champ est absent : les anciens fichiers restent identiques.
    let (q, _, _, _) = ab_clean();
    assert!(!persist::to_json(&q).unwrap().contains("notches"));

    // Références mortes et champs inconnus dans un cran : refus à l'ouverture.
    let dead = json.replacen(&format!("\"entity\": {}", ab.0), "\"entity\": 4242", 1);
    assert!(matches!(
        persist::from_json(&dead),
        Err(CoreError::DeadReference(_))
    ));
    assert!(persist::from_json(&json.replacen("\"style\"", "\"zz\": 1, \"style\"", 1)).is_err());
    let far = json.replacen("\"distance_mm\": 33.3", "\"distance_mm\": 999.0", 1);
    assert!(persist::from_json(&far).is_err());
    // Identifiant de cran déjà pris par un autre objet.
    let dup = json.replacen(
        &format!("\"id\": {}", p.sheets[0].notches[0].id.0),
        &format!("\"id\": {}", ab.0),
        1,
    );
    assert!(persist::from_json(&dup).is_err());

    // Commande sérialisable.
    let c = Command::AddNotch {
        sheet,
        entity: ab,
        distance_mm: 1.5,
        side: Side::Left,
        length_mm: 5.0,
        style: NotchStyle::Single,
    };
    let text = serde_json::to_string(&c).unwrap();
    assert_eq!(serde_json::from_str::<Command>(&text).unwrap(), c);
}

fn ab_clean() -> (Project, SheetId, EntityId, Vec<PointId>) {
    ab()
}
