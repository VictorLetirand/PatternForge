//! US-06.7 — cotations complètes : longueur développée d'un tracé, angle, rayon, diamètre,
//! projections horizontale et verticale, texte orienté et légende. Une cote est une **mesure
//! observée** (jamais une contrainte). Un groupe de tests par critère d'acceptation :
//! CA1 valeurs calculées à la main, CA2 suit la construction et retrait avec remarque,
//! CA3 texte orienté dans ]-90 ; 90] et légende, CA4 refus sans trace, CA5 une étape annulable,
//! CA6 octets inchangés, CA7 sérialisation, CA8 cohérence (copie de feuille, presse-papiers,
//! symétrie, séparer / assembler / déplier, calques verrouillés, plan de pièces).

use pf_core::full_dimension::{
    DimensionLine, MAX_FULL_DIMENSIONS_PER_SHEET, MAX_FULL_DIMENSION_LABEL_CHARS,
};
use pf_core::persist;
use pf_core::*;
use pf_geometry::{distance_mm, PointMm};
use std::f64::consts::PI;
use std::path::PathBuf;

const FIXTURES: [&str; 4] = [
    "v1-exercice.pforge.json",
    "v1-grande-piece.pforge.json",
    "v2-piece-contour-droit-fil.pforge.json",
    "v2-piece-sans-contour.pforge.json",
];

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

fn near(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

fn bytes(p: &Project) -> String {
    persist::to_json(p).unwrap()
}

fn setup() -> (Project, SheetId) {
    let p = Project::new("Cotes complètes");
    let s = p.sheets[0].id;
    (p, s)
}

fn point(p: &mut Project, s: SheetId, x: f64, y: f64) -> PointId {
    p.apply(Command::AddPoint {
        sheet: s,
        at: pt(x, y),
    })
    .unwrap()
    .points[0]
}

fn segment(p: &mut Project, s: SheetId, a: PointMm, b: PointMm) -> (EntityId, PointId, PointId) {
    let o = p
        .apply(Command::AddSegment {
            sheet: s,
            from: PointRef::New(a),
            to: PointRef::New(b),
        })
        .unwrap();
    (o.entities[0], o.points[0], o.points[1])
}

fn add(p: &mut Project, s: SheetId, kind: FullDimensionKind) -> Result<FullDimensionId, CoreError> {
    p.apply(Command::AddFullDimension {
        sheet: s,
        kind,
        label: None,
        text_shift_mm: 0.0,
    })
    .map(|o| o.full_dimensions[0])
}

fn geom(p: &Project, s: SheetId, id: FullDimensionId) -> full_dimension::FullDimensionGeometry {
    p.sheet(s).unwrap().full_dimension_geometry(id).unwrap()
}

fn value(p: &Project, s: SheetId, id: FullDimensionId) -> f64 {
    geom(p, s, id).value
}

fn angle_kind(v: PointId, a: PointId, b: PointId) -> FullDimensionKind {
    FullDimensionKind::Angle {
        vertex: v,
        first: a,
        second: b,
        radius_mm: 20.0,
    }
}

/// Longueur d'une Bézier cubique par Simpson sur la vitesse |B'(t)| : oracle indépendant des
/// méthodes de `pf-geometry`.
fn bezier_length_oracle(p0: PointMm, c1: PointMm, c2: PointMm, p3: PointMm) -> f64 {
    let speed = |t: f64| {
        let u = 1.0 - t;
        let dx =
            3.0 * (u * u * (c1.x - p0.x) + 2.0 * u * t * (c2.x - c1.x) + t * t * (p3.x - c2.x));
        let dy =
            3.0 * (u * u * (c1.y - p0.y) + 2.0 * u * t * (c2.y - c1.y) + t * t * (p3.y - c2.y));
        dx.hypot(dy)
    };
    let n = 20_000;
    let h = 1.0 / n as f64;
    let mut sum = speed(0.0) + speed(1.0);
    for i in 1..n {
        sum += speed(i as f64 * h) * if i % 2 == 1 { 4.0 } else { 2.0 };
    }
    sum * h / 3.0
}

// ---------------------------------------------------------------------------
// CA1 — valeurs calculées à la main
// ---------------------------------------------------------------------------

#[test]
fn ca1_longueur_d_un_segment() {
    let (mut p, s) = setup();
    let (e, ..) = segment(&mut p, s, pt(0., 0.), pt(3., 4.));
    let id = add(
        &mut p,
        s,
        FullDimensionKind::PathLength {
            entity: e,
            offset_mm: 10.0,
        },
    )
    .unwrap();
    let g = geom(&p, s, id);
    assert!(near(g.value, 5.0, 1e-12));
    assert_eq!(g.value_text, "5.00 mm");
    // Ligne décalée de 10 mm à gauche de (3,4)/5 : (-0,8 ; 0,6) × 10.
    match g.line {
        DimensionLine::Straight { a, b, .. } => {
            assert!(distance_mm(a, pt(-8., 6.)) < 1e-9 && distance_mm(b, pt(-5., 10.)) < 1e-9);
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(g.extension_lines.len(), 2);
}

#[test]
fn ca1_longueur_developpee_d_un_arc_r_theta() {
    let (mut p, s) = setup();
    let o = p
        .apply(Command::AddArc {
            sheet: s,
            center: PointRef::New(pt(0., 0.)),
            radius_mm: 10.0,
            start_deg: 0.0,
            end_deg: 90.0,
            ccw: true,
        })
        .unwrap();
    let id = add(
        &mut p,
        s,
        FullDimensionKind::PathLength {
            entity: o.entities[0],
            offset_mm: 5.0,
        },
    )
    .unwrap();
    let g = geom(&p, s, id);
    assert!(near(g.value, 10.0 * PI / 2.0, 1e-9), "{}", g.value);
    // Arc (pas un segment) : la longueur est annoncée comme développée.
    assert_eq!(g.value_text, format!("dév. {:.2} mm", 5.0 * PI));
    // Le rayon et le diamètre du même arc.
    let r = add(
        &mut p,
        s,
        FullDimensionKind::Radius {
            entity: o.entities[0],
            angle_deg: 45.0,
        },
    )
    .unwrap();
    assert!(near(value(&p, s, r), 10.0, 1e-12));
    let d = add(
        &mut p,
        s,
        FullDimensionKind::Diameter {
            entity: o.entities[0],
            angle_deg: 45.0,
        },
    )
    .unwrap();
    assert!(near(value(&p, s, d), 20.0, 1e-12));
}

#[test]
fn ca1_longueur_developpee_d_une_bezier_integration_independante() {
    let (mut p, s) = setup();
    let (p0, c1, c2, p3) = (pt(0., 0.), pt(30., 80.), pt(70., -40.), pt(100., 10.));
    let o = p
        .apply(Command::AddCurve {
            sheet: s,
            start: PointRef::New(p0),
            c1,
            c2,
            end: PointRef::New(p3),
        })
        .unwrap();
    let id = add(
        &mut p,
        s,
        FullDimensionKind::PathLength {
            entity: o.entities[0],
            offset_mm: -12.0,
        },
    )
    .unwrap();
    let g = geom(&p, s, id);
    let oracle = bezier_length_oracle(p0, c1, c2, p3);
    assert!(near(g.value, oracle, 1e-6), "{} contre {oracle}", g.value);
    // Plus longue que la corde : la cote annonce la longueur développée, pas la corde.
    assert!(g.value > distance_mm(p0, p3) + 1.0);
    assert!(g.value_text.starts_with("dév. "));
}

#[test]
fn ca1_longueur_developpee_d_une_courbe_par_points_de_passage() {
    let (mut p, s) = setup();
    let o = p
        .apply(Command::AddSpline {
            sheet: s,
            points: vec![
                PointRef::New(pt(0., 0.)),
                PointRef::New(pt(40., 30.)),
                PointRef::New(pt(80., 0.)),
            ],
        })
        .unwrap();
    let e = o.entities[0];
    let id = add(
        &mut p,
        s,
        FullDimensionKind::PathLength {
            entity: e,
            offset_mm: 4.0,
        },
    )
    .unwrap();
    let expected = pf_core::measure::entity_length(p.sheet(s).unwrap(), e)
        .unwrap()
        .value;
    assert!(near(value(&p, s, id), expected, 1e-9));
}

#[test]
fn ca1_angles_droit_aigu_obtus_plat_et_rentrant() {
    let (mut p, s) = setup();
    let v = point(&mut p, s, 0., 0.);
    let east = point(&mut p, s, 10., 0.);
    let north = point(&mut p, s, 0., 10.);
    let tri = point(&mut p, s, 4., 3.);
    let nw = point(&mut p, s, -1., 1.);
    let west = point(&mut p, s, -5., 0.);
    let south = point(&mut p, s, 0., -3.);
    let check = |p: &mut Project, a, b, expected: f64| {
        let id = add(p, s, angle_kind(v, a, b)).unwrap();
        let got = value(p, s, id);
        assert!(near(got, expected, 1e-9), "{got} contre {expected}");
    };
    check(&mut p, east, north, 90.0);
    // Aigu : atan(3/4) = 36,8698976…°
    check(&mut p, east, tri, (3.0f64 / 4.0).atan().to_degrees());
    // Obtus : de l'est au nord-ouest = 135°.
    check(&mut p, east, nw, 135.0);
    // Plat : sens anti-horaire de l'est à l'ouest = 180° (accepté, pas dégénéré).
    check(&mut p, east, west, 180.0);
    // Rentrant : de l'est au sud, **anti-horaire**, = 270°, et du sud à l'est = 90°.
    check(&mut p, east, south, 270.0);
    check(&mut p, south, east, 90.0);
    // Le nord vers l'est (sens inverse du premier) = 270°.
    check(&mut p, north, east, 270.0);
}

#[test]
fn ca1_arc_de_cote_d_angle_depart_et_balayage() {
    let (mut p, s) = setup();
    let v = point(&mut p, s, 5., 5.);
    let a = point(&mut p, s, 5., 25.);
    let b = point(&mut p, s, -15., 5.);
    let id = add(&mut p, s, angle_kind(v, a, b)).unwrap();
    let g = geom(&p, s, id);
    // Nord (90°) → ouest (180°) : départ 90°, balayage 90°.
    match g.line {
        DimensionLine::Arc {
            center,
            radius_mm,
            start_deg,
            sweep_deg,
        } => {
            assert!(distance_mm(center, pt(5., 5.)) < 1e-12 && radius_mm == 20.0);
            assert!(near(start_deg, 90.0, 1e-9) && near(sweep_deg, 90.0, 1e-9));
        }
        other => panic!("{other:?}"),
    }
    // Texte au milieu de l'arc (135°), orienté le long de la tangente (225° → 45°).
    assert!(
        distance_mm(
            g.text_anchor,
            pt(5. - 20. * 0.5f64.sqrt(), 5. + 20. * 0.5f64.sqrt())
        ) < 1e-9
    );
    assert!(near(g.text_angle_deg, 45.0, 1e-9), "{}", g.text_angle_deg);
    assert_eq!(g.value_text, "90.00°");
}

#[test]
fn ca1_rayon_et_diametre_d_un_cercle() {
    let (mut p, s) = setup();
    let o = p
        .apply(Command::AddCircle {
            sheet: s,
            center: PointRef::New(pt(10., 20.)),
            radius_mm: 12.5,
        })
        .unwrap();
    let e = o.entities[0];
    let r = add(
        &mut p,
        s,
        FullDimensionKind::Radius {
            entity: e,
            angle_deg: 30.0,
        },
    )
    .unwrap();
    let d = add(
        &mut p,
        s,
        FullDimensionKind::Diameter {
            entity: e,
            angle_deg: 30.0,
        },
    )
    .unwrap();
    let (gr, gd) = (geom(&p, s, r), geom(&p, s, d));
    assert!(near(gr.value, 12.5, 1e-12) && gr.value_text == "R 12.50 mm");
    assert!(near(gd.value, 25.0, 1e-12) && gd.value_text == "Ø 25.00 mm");
    // Le trait de rayon part du centre vers le cercle, dans la direction demandée.
    match gr.line {
        DimensionLine::Straight {
            a,
            b,
            arrow_a,
            arrow_b,
        } => {
            assert!(distance_mm(a, pt(10., 20.)) < 1e-12);
            let tip = pt(
                10. + 12.5 * 30f64.to_radians().cos(),
                20. + 12.5 * 30f64.to_radians().sin(),
            );
            assert!(distance_mm(b, tip) < 1e-9);
            assert!(!arrow_a && arrow_b);
        }
        other => panic!("{other:?}"),
    }
    // Le trait de diamètre traverse le centre : ses extrémités sont à 12,5 mm.
    match gd.line {
        DimensionLine::Straight { a, b, .. } => {
            assert!(near(distance_mm(a, b), 25.0, 1e-9));
            assert!(
                distance_mm(
                    PointMm::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0),
                    pt(10., 20.)
                ) < 1e-9
            );
        }
        other => panic!("{other:?}"),
    }
    // Direction par défaut proposée à l'interface.
    assert_eq!(
        p.sheet(s).unwrap().default_leader_angle_deg(e).unwrap(),
        45.0
    );
}

#[test]
fn ca1_projections_horizontale_et_verticale_signees() {
    let (mut p, s) = setup();
    let a = point(&mut p, s, 10., 20.);
    let b = point(&mut p, s, 40., 5.);
    let h = add(
        &mut p,
        s,
        FullDimensionKind::Horizontal {
            from: a,
            to: b,
            offset_mm: 8.0,
        },
    )
    .unwrap();
    let v = add(
        &mut p,
        s,
        FullDimensionKind::Vertical {
            from: a,
            to: b,
            offset_mm: 8.0,
        },
    )
    .unwrap();
    assert!(near(value(&p, s, h), 30.0, 1e-12), "ΔX = 40 − 10");
    assert!(near(value(&p, s, v), -15.0, 1e-12), "ΔY = 5 − 20");
    // De b vers a : les signes s'inversent.
    let h2 = add(
        &mut p,
        s,
        FullDimensionKind::Horizontal {
            from: b,
            to: a,
            offset_mm: 0.0,
        },
    )
    .unwrap();
    let v2 = add(
        &mut p,
        s,
        FullDimensionKind::Vertical {
            from: b,
            to: a,
            offset_mm: 0.0,
        },
    )
    .unwrap();
    assert!(near(value(&p, s, h2), -30.0, 1e-12) && near(value(&p, s, v2), 15.0, 1e-12));
    // L'affichage donne la longueur de la projection (valeur absolue, 0,01 mm).
    assert_eq!(geom(&p, s, h2).value_text, "30.00 mm");
    assert_eq!(geom(&p, s, v).value_text, "15.00 mm");
    // Ligne horizontale à l'ordonnée de `from` + décalage ; verticale à son abscisse.
    match geom(&p, s, h).line {
        DimensionLine::Straight { a: pa, b: pb, .. } => {
            assert!(distance_mm(pa, pt(10., 28.)) < 1e-12 && distance_mm(pb, pt(40., 28.)) < 1e-12);
        }
        other => panic!("{other:?}"),
    }
    match geom(&p, s, v).line {
        DimensionLine::Straight { a: pa, b: pb, .. } => {
            assert!(distance_mm(pa, pt(18., 20.)) < 1e-12 && distance_mm(pb, pt(18., 5.)) < 1e-12);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn ca1_precision_au_pas_de_un_centieme() {
    let (mut p, s) = setup();
    let a = point(&mut p, s, 0., 0.);
    let b = point(&mut p, s, 10.004, 0.);
    let c = point(&mut p, s, 10.006, 0.);
    let h1 = add(
        &mut p,
        s,
        FullDimensionKind::Horizontal {
            from: a,
            to: b,
            offset_mm: 1.0,
        },
    )
    .unwrap();
    let h2 = add(
        &mut p,
        s,
        FullDimensionKind::Horizontal {
            from: a,
            to: c,
            offset_mm: 1.0,
        },
    )
    .unwrap();
    assert_eq!(geom(&p, s, h1).value_text, "10.00 mm");
    assert_eq!(geom(&p, s, h2).value_text, "10.01 mm");
    // La valeur stockée n'est pas arrondie : seule la lecture l'est.
    assert!(near(value(&p, s, h2), 10.006, 1e-12));
    assert_eq!(geom(&p, s, h2).text(','), "10,01 mm");
}

// ---------------------------------------------------------------------------
// CA2 — la cote suit la construction ; cible supprimée = retrait avec remarque
// ---------------------------------------------------------------------------

#[test]
fn ca2_la_cote_suit_le_deplacement_de_sa_cible() {
    let (mut p, s) = setup();
    let (e, a, b) = segment(&mut p, s, pt(0., 0.), pt(30., 40.));
    let len = add(
        &mut p,
        s,
        FullDimensionKind::PathLength {
            entity: e,
            offset_mm: 3.0,
        },
    )
    .unwrap();
    let h = add(
        &mut p,
        s,
        FullDimensionKind::Horizontal {
            from: a,
            to: b,
            offset_mm: 3.0,
        },
    )
    .unwrap();
    let c = point(&mut p, s, 50., 0.);
    let ang = add(&mut p, s, angle_kind(a, c, b)).unwrap();
    assert!(near(value(&p, s, len), 50.0, 1e-9));
    assert!(near(value(&p, s, h), 30.0, 1e-9));
    p.apply(Command::MovePoint {
        sheet: s,
        point: b,
        to: pt(60., 80.),
    })
    .unwrap();
    assert!(near(value(&p, s, len), 100.0, 1e-9));
    assert!(near(value(&p, s, h), 60.0, 1e-9));
    // Angle de (50,0) vers (60,80) autour de l'origine : atan2(80,60) = 53,13°.
    assert!(near(
        value(&p, s, ang),
        (80.0f64).atan2(60.0).to_degrees(),
        1e-9
    ));
    // La cote ne contraint rien : on peut déplacer la cible sans que la valeur résiste.
    p.apply(Command::Transform {
        sheet: s,
        points: vec![a, b, c],
        transform: pf_geometry::Transform::Rotate {
            center: pt(0., 0.),
            angle_deg: 30.0,
        },
    })
    .unwrap();
    assert!(
        near(value(&p, s, len), 100.0, 1e-9),
        "la rotation conserve les longueurs"
    );
    assert!(near(
        value(&p, s, ang),
        (80.0f64).atan2(60.0).to_degrees(),
        1e-9
    ));
}

#[test]
fn ca2_cible_supprimee_retire_la_cote_avec_remarque() {
    let (mut p, s) = setup();
    let (e, _a, b) = segment(&mut p, s, pt(0., 0.), pt(30., 40.));
    let c = point(&mut p, s, 50., 0.);
    let len = add(
        &mut p,
        s,
        FullDimensionKind::PathLength {
            entity: e,
            offset_mm: 3.0,
        },
    )
    .unwrap();
    let d = point(&mut p, s, 0., 70.);
    let h = add(
        &mut p,
        s,
        FullDimensionKind::Horizontal {
            from: d,
            to: c,
            offset_mm: 3.0,
        },
    )
    .unwrap();
    let before = p.sheet(s).unwrap().full_dimensions.len();
    assert_eq!(before, 2);
    // Supprimer le tracé retire la longueur, pas la projection entre a et c.
    let o = p
        .apply(Command::Delete {
            sheet: s,
            points: vec![],
            entities: vec![e],
        })
        .unwrap();
    assert!(
        o.notes
            .iter()
            .any(|n| n.contains("cote") && n.contains("retirée")),
        "{:?}",
        o.notes
    );
    let sh = p.sheet(s).unwrap();
    assert!(sh.full_dimension(len).is_err());
    assert!(
        sh.full_dimension(h).is_ok(),
        "la projection entre d et c ne cite pas le tracé"
    );
    // Supprimer le point c retire la projection, sans référence morte.
    let o = p
        .apply(Command::Delete {
            sheet: s,
            points: vec![c],
            entities: vec![],
        })
        .unwrap();
    assert!(o.notes.iter().any(|n| n.contains("cote")), "{:?}", o.notes);
    assert!(p.sheet(s).unwrap().full_dimensions.is_empty());
    p.validate().unwrap();
    let _ = b;
}

#[test]
fn ca2_geometrie_devenue_indefinie_n_est_ni_refusee_ni_supprimee() {
    let (mut p, s) = setup();
    let v = point(&mut p, s, 0., 0.);
    let a = point(&mut p, s, 10., 0.);
    let b = point(&mut p, s, 0., 10.);
    let id = add(&mut p, s, angle_kind(v, a, b)).unwrap();
    // Le côté b vient se superposer à a : angle indéfini (0°/360°) ; la cote reste, non calculable.
    p.apply(Command::MovePoint {
        sheet: s,
        point: b,
        to: pt(25., 0.),
    })
    .unwrap();
    let sh = p.sheet(s).unwrap();
    assert!(sh.full_dimension(id).is_ok());
    let err = sh.full_dimension_geometry(id).unwrap_err().to_string();
    assert!(err.contains("superposés"), "{err}");
    assert!(sh.all_full_dimension_geometries().is_empty());
    // Elle redevient calculable dès que la construction la redéfinit.
    p.apply(Command::MovePoint {
        sheet: s,
        point: b,
        to: pt(0., 7.),
    })
    .unwrap();
    assert!(near(value(&p, s, id), 90.0, 1e-9));
}

// ---------------------------------------------------------------------------
// CA3 — texte orienté, jamais à l'envers ; légende
// ---------------------------------------------------------------------------

#[test]
fn ca3_orientation_du_texte_dans_les_quatre_quadrants() {
    let (mut p, s) = setup();
    // (direction du segment en degrés, angle attendu du texte dans ]-90 ; 90])
    let cases: [(f64, f64); 8] = [
        (30.0, 30.0),
        (120.0, -60.0),
        (210.0, 30.0),
        (300.0, -60.0),
        (0.0, 0.0),
        (90.0, 90.0),
        (180.0, 0.0),
        (270.0, 90.0),
    ];
    for (dir, expected) in cases {
        let (sin, cos) = dir.to_radians().sin_cos();
        let (e, ..) = segment(
            &mut p,
            s,
            pt(100.0 + 7.0 * dir, 50.0),
            pt(100.0 + 7.0 * dir + 40.0 * cos, 50.0 + 40.0 * sin),
        );
        let id = add(
            &mut p,
            s,
            FullDimensionKind::PathLength {
                entity: e,
                offset_mm: 6.0,
            },
        )
        .unwrap();
        let a = geom(&p, s, id).text_angle_deg;
        assert!(
            near(a, expected, 1e-9),
            "direction {dir} : texte à {a}, attendu {expected}"
        );
        assert!(a > -90.0 && a <= 90.0);
        // Le texte est remonté à gauche du sens de lecture : au-dessus de la ligne.
        let g = geom(&p, s, id);
        let up = pt(
            g.text_center().x - g.text_anchor.x,
            g.text_center().y - g.text_anchor.y,
        );
        assert!(near(
            up.x.hypot(up.y),
            pf_core::full_dimension::TEXT_LIFT_MM,
            1e-9
        ));
        assert!(up.y > -1e-9, "le texte n'est jamais sous la ligne ({up:?})");
    }
    // Horizontale : 0° ; verticale : 90° (le texte monte).
    let a = point(&mut p, s, 0., 300.);
    let b = point(&mut p, s, 50., 330.);
    let h = add(
        &mut p,
        s,
        FullDimensionKind::Horizontal {
            from: b,
            to: a,
            offset_mm: 2.0,
        },
    )
    .unwrap();
    let v = add(
        &mut p,
        s,
        FullDimensionKind::Vertical {
            from: b,
            to: a,
            offset_mm: 2.0,
        },
    )
    .unwrap();
    assert_eq!(geom(&p, s, h).text_angle_deg, 0.0);
    assert_eq!(geom(&p, s, v).text_angle_deg, 90.0);
}

#[test]
fn ca3_decalage_du_texte_le_long_de_la_ligne() {
    let (mut p, s) = setup();
    let (e, ..) = segment(&mut p, s, pt(0., 0.), pt(100., 0.));
    let id = add(
        &mut p,
        s,
        FullDimensionKind::PathLength {
            entity: e,
            offset_mm: 10.0,
        },
    )
    .unwrap();
    assert!(distance_mm(geom(&p, s, id).text_anchor, pt(50., 10.)) < 1e-12);
    p.apply(Command::SetFullDimensionTextShift {
        sheet: s,
        dimension: id,
        text_shift_mm: -20.0,
    })
    .unwrap();
    assert!(distance_mm(geom(&p, s, id).text_anchor, pt(30., 10.)) < 1e-12);
    // Segment descendant (direction 180°) : le sens de lecture reste vers la droite (0°).
    let (e2, ..) = segment(&mut p, s, pt(100., 50.), pt(0., 50.));
    let id2 = add(
        &mut p,
        s,
        FullDimensionKind::PathLength {
            entity: e2,
            offset_mm: 0.0,
        },
    )
    .unwrap();
    p.apply(Command::SetFullDimensionTextShift {
        sheet: s,
        dimension: id2,
        text_shift_mm: 10.0,
    })
    .unwrap();
    assert!(distance_mm(geom(&p, s, id2).text_anchor, pt(60., 50.)) < 1e-12);
}

#[test]
fn ca3_legende_accents_remplacement_et_retrait() {
    let (mut p, s) = setup();
    let a = point(&mut p, s, 0., 0.);
    let b = point(&mut p, s, 80., 0.);
    let id = p
        .apply(Command::AddFullDimension {
            sheet: s,
            kind: FullDimensionKind::Horizontal {
                from: a,
                to: b,
                offset_mm: 5.0,
            },
            label: Some("  Épaule — côté œil  ".into()),
            text_shift_mm: 0.0,
        })
        .unwrap()
        .full_dimensions[0];
    let g = geom(&p, s, id);
    assert_eq!(
        g.label.as_deref(),
        Some("Épaule — côté œil"),
        "espaces de bord retirés"
    );
    assert_eq!(g.text('.'), "Épaule — côté œil : 80.00 mm");
    assert_eq!(g.text(','), "Épaule — côté œil : 80,00 mm");
    // Remplacement, puis retrait par `None` et par texte vide.
    p.apply(Command::SetFullDimensionLabel {
        sheet: s,
        dimension: id,
        label: Some("Tour".into()),
    })
    .unwrap();
    assert_eq!(geom(&p, s, id).text('.'), "Tour : 80.00 mm");
    p.apply(Command::SetFullDimensionLabel {
        sheet: s,
        dimension: id,
        label: Some("   ".into()),
    })
    .unwrap();
    assert_eq!(geom(&p, s, id).label, None);
    assert_eq!(geom(&p, s, id).text('.'), "80.00 mm");
    // Borne : exactement MAX caractères accentués passe, un de plus est refusé.
    let max = "é".repeat(MAX_FULL_DIMENSION_LABEL_CHARS);
    p.apply(Command::SetFullDimensionLabel {
        sheet: s,
        dimension: id,
        label: Some(max.clone()),
    })
    .unwrap();
    assert_eq!(
        p.sheet(s)
            .unwrap()
            .full_dimension(id)
            .unwrap()
            .label
            .as_deref(),
        Some(max.as_str())
    );
    let before = bytes(&p);
    let too_long = "é".repeat(MAX_FULL_DIMENSION_LABEL_CHARS + 1);
    assert!(p
        .apply(Command::SetFullDimensionLabel {
            sheet: s,
            dimension: id,
            label: Some(too_long)
        })
        .is_err());
    assert_eq!(bytes(&p), before);
}

// ---------------------------------------------------------------------------
// CA4 — refus sans trace
// ---------------------------------------------------------------------------

fn refused(p: &mut Project, cmd: Command, needle: &str) {
    let before = bytes(p);
    let snapshot = p.clone();
    let err = p
        .apply(cmd)
        .expect_err("la commande devait être refusée")
        .to_string();
    assert!(err.contains(needle), "message {err:?} sans {needle:?}");
    assert_eq!(bytes(p), before, "refus avec trace");
    assert_eq!(*p, snapshot);
}

#[test]
fn ca4_refus_sans_trace_cible_et_geometrie() {
    let (mut p, s) = setup();
    let (seg, a, b) = segment(&mut p, s, pt(0., 0.), pt(10., 0.));
    let curve = p
        .apply(Command::AddCurve {
            sheet: s,
            start: PointRef::New(pt(0., 20.)),
            c1: pt(10., 40.),
            c2: pt(20., 40.),
            end: PointRef::New(pt(30., 20.)),
        })
        .unwrap()
        .entities[0];
    let circle = p
        .apply(Command::AddCircle {
            sheet: s,
            center: PointRef::New(pt(50., 50.)),
            radius_mm: 5.0,
        })
        .unwrap()
        .entities[0];
    let c = point(&mut p, s, 5., 5.);
    let same_x = point(&mut p, s, 0., 9.);
    let same_y = point(&mut p, s, 9., 0.);
    let far = point(&mut p, s, 100., 0.);
    let add_cmd = |kind| Command::AddFullDimension {
        sheet: s,
        kind,
        label: None,
        text_shift_mm: 0.0,
    };
    let known = FullDimensionKind::PathLength {
        entity: seg,
        offset_mm: 1.0,
    };
    // Rayon d'un segment ou d'une courbe de Bézier : pas de rayon.
    refused(
        &mut p,
        add_cmd(FullDimensionKind::Radius {
            entity: seg,
            angle_deg: 0.0,
        }),
        "n'a pas de rayon",
    );
    refused(
        &mut p,
        add_cmd(FullDimensionKind::Diameter {
            entity: curve,
            angle_deg: 0.0,
        }),
        "n'a pas de rayon",
    );
    // Un cercle n'a pas d'extrémités pour la longueur.
    refused(
        &mut p,
        add_cmd(FullDimensionKind::PathLength {
            entity: circle,
            offset_mm: 0.0,
        }),
        "cercle",
    );
    // Cible absente.
    refused(
        &mut p,
        add_cmd(FullDimensionKind::PathLength {
            entity: EntityId(9_999),
            offset_mm: 0.0,
        }),
        "introuvable",
    );
    refused(
        &mut p,
        add_cmd(FullDimensionKind::Horizontal {
            from: a,
            to: PointId(9_999),
            offset_mm: 0.0,
        }),
        "introuvable",
    );
    // Angle : points confondus par identifiant, côté de longueur nulle, côtés superposés.
    refused(&mut p, add_cmd(angle_kind(a, a, b)), "différents");
    let twin = point(&mut p, s, 7.0, 3.0);
    p.apply(Command::MovePoint {
        sheet: s,
        point: twin,
        to: pt(0.0000001, 0.0),
    })
    .unwrap();
    refused(&mut p, add_cmd(angle_kind(a, twin, b)), "longueur nulle");
    refused(&mut p, add_cmd(angle_kind(a, b, far)), "superposés");
    // Projections nulles ou points identiques.
    refused(
        &mut p,
        add_cmd(FullDimensionKind::Horizontal {
            from: a,
            to: same_x,
            offset_mm: 0.0,
        }),
        "même abscisse",
    );
    refused(
        &mut p,
        add_cmd(FullDimensionKind::Vertical {
            from: a,
            to: same_y,
            offset_mm: 0.0,
        }),
        "même ordonnée",
    );
    refused(
        &mut p,
        add_cmd(FullDimensionKind::Vertical {
            from: a,
            to: a,
            offset_mm: 0.0,
        }),
        "différents",
    );
    // Valeurs non finies ou hors bornes.
    for bad in [f64::NAN, f64::INFINITY, -f64::INFINITY, 1.0e9] {
        refused(
            &mut p,
            add_cmd(FullDimensionKind::PathLength {
                entity: seg,
                offset_mm: bad,
            }),
            "décalage",
        );
        if !bad.is_finite() {
            refused(
                &mut p,
                add_cmd(FullDimensionKind::Radius {
                    entity: circle,
                    angle_deg: bad,
                }),
                "direction",
            );
        }
        refused(
            &mut p,
            Command::AddFullDimension {
                sheet: s,
                kind: known,
                label: None,
                text_shift_mm: bad,
            },
            "décalage du texte",
        );
    }
    for bad in [0.0, 0.99, f64::NAN, 10_001.0] {
        refused(
            &mut p,
            add_cmd(FullDimensionKind::Angle {
                vertex: a,
                first: b,
                second: c,
                radius_mm: bad,
            }),
            "rayon de l'arc",
        );
    }
    // Légende : contrôle, trop longue.
    for (label, needle) in [
        ("a\nb", "contrôle"),
        ("a\tb", "contrôle"),
        ("\u{0}", "contrôle"),
        (&"x".repeat(41), "dépasse"),
    ] {
        refused(
            &mut p,
            Command::AddFullDimension {
                sheet: s,
                kind: known,
                label: Some(label.to_string()),
                text_shift_mm: 0.0,
            },
            needle,
        );
    }
    // Feuille inconnue, et cible d'une autre feuille.
    let other = p
        .apply(Command::CreateSheet { name: "B".into() })
        .unwrap()
        .sheet
        .unwrap();
    refused(
        &mut p,
        Command::AddFullDimension {
            sheet: SheetId(9_999),
            kind: known,
            label: None,
            text_shift_mm: 0.0,
        },
        "introuvable",
    );
    refused(
        &mut p,
        Command::AddFullDimension {
            sheet: other,
            kind: known,
            label: None,
            text_shift_mm: 0.0,
        },
        "introuvable",
    );
    // Commandes de modification d'une cote inexistante ou d'une autre feuille.
    let ok = add(&mut p, s, known).unwrap();
    refused(
        &mut p,
        Command::SetFullDimensionLabel {
            sheet: s,
            dimension: FullDimensionId(9_999),
            label: None,
        },
        "introuvable",
    );
    refused(
        &mut p,
        Command::SetFullDimensionLabel {
            sheet: other,
            dimension: ok,
            label: None,
        },
        "introuvable",
    );
    refused(
        &mut p,
        Command::SetFullDimensionPlacement {
            sheet: s,
            dimension: ok,
            placement: f64::NAN,
        },
        "décalage",
    );
    refused(
        &mut p,
        Command::SetFullDimensionPlacement {
            sheet: s,
            dimension: ok,
            placement: 20_000.0,
        },
        "décalage",
    );
    refused(
        &mut p,
        Command::SetFullDimensionTextShift {
            sheet: s,
            dimension: ok,
            text_shift_mm: f64::INFINITY,
        },
        "décalage du texte",
    );
    refused(
        &mut p,
        Command::DeleteFullDimension {
            sheet: s,
            dimension: FullDimensionId(9_999),
        },
        "introuvable",
    );
    refused(
        &mut p,
        Command::DeleteFullDimension {
            sheet: other,
            dimension: ok,
        },
        "introuvable",
    );
}

#[test]
fn ca4_plafond_par_feuille() {
    let (mut p, s) = setup();
    let (e, ..) = segment(&mut p, s, pt(0., 0.), pt(10., 0.));
    for i in 0..MAX_FULL_DIMENSIONS_PER_SHEET {
        add(
            &mut p,
            s,
            FullDimensionKind::PathLength {
                entity: e,
                offset_mm: i as f64,
            },
        )
        .unwrap();
    }
    refused(
        &mut p,
        Command::AddFullDimension {
            sheet: s,
            kind: FullDimensionKind::PathLength {
                entity: e,
                offset_mm: -1.0,
            },
            label: None,
            text_shift_mm: 0.0,
        },
        "au plus",
    );
}

// ---------------------------------------------------------------------------
// CA5 — une étape annulable, no-op sans étape
// ---------------------------------------------------------------------------

#[test]
fn ca5_une_etape_annulable_et_no_op_sans_etape() {
    let (mut p, s) = setup();
    let (e, ..) = segment(&mut p, s, pt(0., 0.), pt(10., 0.));
    let mut doc = Document::new(p);
    let base = bytes(doc.project());
    let id = doc
        .execute(Command::AddFullDimension {
            sheet: s,
            kind: FullDimensionKind::PathLength {
                entity: e,
                offset_mm: 4.0,
            },
            label: Some("Bas".into()),
            text_shift_mm: 2.0,
        })
        .unwrap()
        .full_dimensions[0];
    assert!(doc.can_undo());
    let with = bytes(doc.project());
    assert_ne!(with, base);
    assert!(doc.undo());
    assert_eq!(
        bytes(doc.project()),
        base,
        "une seule annulation défait la cote"
    );
    assert!(!doc.can_undo());
    assert!(doc.redo());
    assert_eq!(bytes(doc.project()), with);
    // Mêmes valeurs : aucune étape d'historique.
    let steps = |d: &mut Document| {
        let mut n = 0;
        while d.undo() {
            n += 1;
        }
        while d.redo() {}
        n
    };
    assert_eq!(steps(&mut doc), 1);
    doc.execute(Command::SetFullDimensionLabel {
        sheet: s,
        dimension: id,
        label: Some("Bas".into()),
    })
    .unwrap();
    doc.execute(Command::SetFullDimensionPlacement {
        sheet: s,
        dimension: id,
        placement: 4.0,
    })
    .unwrap();
    doc.execute(Command::SetFullDimensionTextShift {
        sheet: s,
        dimension: id,
        text_shift_mm: 2.0,
    })
    .unwrap();
    assert_eq!(
        steps(&mut doc),
        1,
        "les commandes sans effet n'ajoutent aucune étape"
    );
    // Chaque modification est une étape.
    doc.execute(Command::SetFullDimensionLabel {
        sheet: s,
        dimension: id,
        label: None,
    })
    .unwrap();
    doc.execute(Command::SetFullDimensionPlacement {
        sheet: s,
        dimension: id,
        placement: -4.0,
    })
    .unwrap();
    doc.execute(Command::SetFullDimensionTextShift {
        sheet: s,
        dimension: id,
        text_shift_mm: 0.0,
    })
    .unwrap();
    doc.execute(Command::DeleteFullDimension {
        sheet: s,
        dimension: id,
    })
    .unwrap();
    assert_eq!(steps(&mut doc), 5);
    assert!(
        doc.project().sheets[0].full_dimensions.is_empty(),
        "dernière étape : suppression"
    );
    while doc.undo() {}
    assert_eq!(bytes(doc.project()), base);
}

#[test]
fn ca5_direction_du_trait_normalisee() {
    let (mut p, s) = setup();
    let o = p
        .apply(Command::AddCircle {
            sheet: s,
            center: PointRef::New(pt(0., 0.)),
            radius_mm: 5.0,
        })
        .unwrap();
    let id = add(
        &mut p,
        s,
        FullDimensionKind::Radius {
            entity: o.entities[0],
            angle_deg: -90.0,
        },
    )
    .unwrap();
    let k = p.sheet(s).unwrap().full_dimension(id).unwrap().kind;
    assert_eq!(k.placement(), 270.0);
    p.apply(Command::SetFullDimensionPlacement {
        sheet: s,
        dimension: id,
        placement: 720.0 + 15.0,
    })
    .unwrap();
    assert_eq!(
        p.sheet(s)
            .unwrap()
            .full_dimension(id)
            .unwrap()
            .kind
            .placement(),
        15.0
    );
    p.apply(Command::SetFullDimensionPlacement {
        sheet: s,
        dimension: id,
        placement: 360.0,
    })
    .unwrap();
    assert_eq!(
        p.sheet(s)
            .unwrap()
            .full_dimension(id)
            .unwrap()
            .kind
            .placement(),
        0.0
    );
}

// ---------------------------------------------------------------------------
// CA6 — octets inchangés
// ---------------------------------------------------------------------------

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

#[test]
fn ca6_fixtures_et_projets_sans_cote_inchanges_a_l_octet() {
    for name in FIXTURES {
        let text = std::fs::read_to_string(fixtures_dir().join(name)).unwrap();
        let p = persist::from_json(&text).unwrap();
        let out = persist::to_json(&p).unwrap();
        assert!(!out.contains("full_dimensions"), "{name}");
        assert!(p.sheets.iter().all(|s| s.full_dimensions.is_empty()));
        // Relecture de la sortie : identique (pas de dérive).
        assert_eq!(
            persist::to_json(&persist::from_json(&out).unwrap()).unwrap(),
            out
        );
    }
    let demo = pf_core::demo_piece_project().unwrap();
    assert!(!persist::to_json(&demo).unwrap().contains("full_dimensions"));
    // Une cote ajoutée puis supprimée ne laisse aucun octet (ni clé vide).
    let (mut p, s) = setup();
    let (e, ..) = segment(&mut p, s, pt(0., 0.), pt(10., 0.));
    let before = bytes(&p);
    let id = add(
        &mut p,
        s,
        FullDimensionKind::PathLength {
            entity: e,
            offset_mm: 1.0,
        },
    )
    .unwrap();
    assert!(bytes(&p).contains("full_dimensions"));
    p.apply(Command::DeleteFullDimension {
        sheet: s,
        dimension: id,
    })
    .unwrap();
    // `next_id` a avancé : on compare le reste du fichier, clé de feuille comprise.
    let after = bytes(&p);
    assert!(!after.contains("full_dimensions"));
    assert_eq!(
        before
            .replace(&format!("\"next_id\":{}", p.next_id - 1), "")
            .len(),
        after
            .replace(&format!("\"next_id\":{}", p.next_id), "")
            .len()
    );
}

// ---------------------------------------------------------------------------
// CA7 — sérialisation
// ---------------------------------------------------------------------------

#[test]
fn ca7_serialisation_relue_identique_et_champs_inconnus_refuses() {
    let (mut p, s) = setup();
    let (e, a, b) = segment(&mut p, s, pt(0., 0.), pt(30., 40.));
    let c = point(&mut p, s, 50., 0.);
    let circle = p
        .apply(Command::AddCircle {
            sheet: s,
            center: PointRef::New(pt(80., 80.)),
            radius_mm: 7.5,
        })
        .unwrap()
        .entities[0];
    for (kind, label, shift) in [
        (
            FullDimensionKind::PathLength {
                entity: e,
                offset_mm: 3.5,
            },
            Some("Longueur"),
            0.0,
        ),
        (angle_kind(a, c, b), None, 1.25),
        (
            FullDimensionKind::Radius {
                entity: circle,
                angle_deg: 33.0,
            },
            Some("Œil"),
            0.0,
        ),
        (
            FullDimensionKind::Diameter {
                entity: circle,
                angle_deg: 120.5,
            },
            None,
            0.0,
        ),
        (
            FullDimensionKind::Horizontal {
                from: a,
                to: c,
                offset_mm: -2.0,
            },
            Some("H"),
            -3.0,
        ),
        (
            FullDimensionKind::Vertical {
                from: a,
                to: b,
                offset_mm: 0.1,
            },
            None,
            0.0,
        ),
    ] {
        p.apply(Command::AddFullDimension {
            sheet: s,
            kind,
            label: label.map(String::from),
            text_shift_mm: shift,
        })
        .unwrap();
    }
    let json = persist::to_json(&p).unwrap();
    let back = persist::from_json(&json).unwrap();
    assert_eq!(back, p);
    assert_eq!(
        persist::to_json(&back).unwrap(),
        json,
        "octets identiques à la relecture"
    );
    // Une clé inconnue dans une cote est refusée, comme une cible absente ou une légende invalide.
    let mut v: serde_json::Value = serde_json::from_str(&json).unwrap();
    let first = &mut v["sheets"][0]["full_dimensions"][0];
    first["surprise"] = serde_json::json!(1);
    assert!(persist::from_json(&v.to_string()).is_err());
    let mut v: serde_json::Value = serde_json::from_str(&json).unwrap();
    v["sheets"][0]["full_dimensions"][0]["kind"]["entity"] = serde_json::json!(424_242);
    assert!(persist::from_json(&v.to_string()).is_err());
    let mut v: serde_json::Value = serde_json::from_str(&json).unwrap();
    v["sheets"][0]["full_dimensions"][0]["label"] = serde_json::json!("a\nb");
    assert!(persist::from_json(&v.to_string()).is_err());
    let mut v: serde_json::Value = serde_json::from_str(&json).unwrap();
    v["sheets"][0]["full_dimensions"][0]["label"] = serde_json::json!(" espace ");
    assert!(persist::from_json(&v.to_string()).is_err());
    let mut v: serde_json::Value = serde_json::from_str(&json).unwrap();
    v["sheets"][0]["full_dimensions"][3]["kind"]["angle_deg"] = serde_json::json!(400.0);
    assert!(persist::from_json(&v.to_string()).is_err());
    let mut v: serde_json::Value = serde_json::from_str(&json).unwrap();
    v["sheets"][0]["full_dimensions"][0]["kind"]["type"] = serde_json::json!("inconnu");
    assert!(persist::from_json(&v.to_string()).is_err());
    // Identifiant en double.
    let mut v: serde_json::Value = serde_json::from_str(&json).unwrap();
    let dup = v["sheets"][0]["full_dimensions"][0]["id"].clone();
    v["sheets"][0]["full_dimensions"][1]["id"] = dup;
    assert!(persist::from_json(&v.to_string()).is_err());
    assert_eq!(p.schema_version, 6);
}

// ---------------------------------------------------------------------------
// CA8 — cohérence avec le reste du modèle
// ---------------------------------------------------------------------------

#[test]
fn ca8_copie_de_feuille_remappe_les_cotes() {
    let (mut p, s) = setup();
    let (e, a, b) = segment(&mut p, s, pt(0., 0.), pt(30., 40.));
    let id = add(
        &mut p,
        s,
        FullDimensionKind::PathLength {
            entity: e,
            offset_mm: 3.0,
        },
    )
    .unwrap();
    let h = add(
        &mut p,
        s,
        FullDimensionKind::Horizontal {
            from: a,
            to: b,
            offset_mm: 3.0,
        },
    )
    .unwrap();
    let o = p.apply(Command::DuplicateSheet { sheet: s }).unwrap();
    let copy = o.sheet.unwrap();
    let cs = p.sheet(copy).unwrap();
    assert_eq!(cs.full_dimensions.len(), 2);
    assert_eq!(o.full_dimensions.len(), 2);
    for d in &cs.full_dimensions {
        assert!(d.id != id && d.id != h, "identifiants neufs");
        // Cible dans la copie, jamais dans la feuille d'origine.
        for q in d.kind.points() {
            assert!(cs.point(q).is_ok() && p.sheet(s).unwrap().point(q).is_err());
        }
        if let Some(en) = d.kind.entity() {
            assert!(cs.entity(en).is_ok() && p.sheet(s).unwrap().entity(en).is_err());
        }
    }
    let lengths: Vec<f64> = cs
        .full_dimensions
        .iter()
        .map(|d| cs.full_dimension_value(d.id).unwrap())
        .collect();
    assert!(near(lengths[0], 50.0, 1e-9) && near(lengths[1], 30.0, 1e-9));
    // La copie est indépendante : déplacer un point de la copie ne touche pas l'original.
    let copy_pt = cs.points[1].id;
    p.apply(Command::MovePoint {
        sheet: copy,
        point: copy_pt,
        to: pt(60., 80.),
    })
    .unwrap();
    assert!(near(value(&p, s, id), 50.0, 1e-9));
    p.validate().unwrap();
}

#[test]
fn ca8_import_de_feuille_dans_un_autre_projet() {
    let (mut p, s) = setup();
    let (e, ..) = segment(&mut p, s, pt(0., 0.), pt(30., 40.));
    add(
        &mut p,
        s,
        FullDimensionKind::PathLength {
            entity: e,
            offset_mm: 3.0,
        },
    )
    .unwrap();
    let cmd = p.import_command(s).unwrap();
    let mut other = Project::new("Autre");
    let o = other.apply(cmd).unwrap();
    let imported = other.sheet(o.sheet.unwrap()).unwrap();
    assert_eq!(imported.full_dimensions.len(), 1);
    assert!(near(
        imported
            .full_dimension_value(imported.full_dimensions[0].id)
            .unwrap(),
        50.0,
        1e-9
    ));
    other.validate().unwrap();
}

#[test]
fn ca8_presse_papiers_copie_les_cotes_entierement_couvertes() {
    let (mut p, s) = setup();
    let (e, a, b) = segment(&mut p, s, pt(0., 0.), pt(30., 40.));
    let (e2, ..) = segment(&mut p, s, pt(100., 0.), pt(130., 0.));
    add(
        &mut p,
        s,
        FullDimensionKind::PathLength {
            entity: e,
            offset_mm: 3.0,
        },
    )
    .unwrap();
    add(
        &mut p,
        s,
        FullDimensionKind::PathLength {
            entity: e2,
            offset_mm: 3.0,
        },
    )
    .unwrap();
    add(
        &mut p,
        s,
        FullDimensionKind::Horizontal {
            from: a,
            to: b,
            offset_mm: 3.0,
        },
    )
    .unwrap();
    // Copier seulement le premier segment : ses deux cotes viennent, pas celle du second.
    let frag = p.copy_fragment(s, &[], &[e], &[], &[]).unwrap();
    assert_eq!(frag.full_dimensions.len(), 2);
    assert!(Fragment::from_json(&frag.to_json()).is_ok());
    let o = p
        .apply(Command::PasteFragment {
            sheet: s,
            fragment: Box::new(frag),
            layer: None,
        })
        .unwrap();
    assert_eq!(o.full_dimensions.len(), 2);
    assert_eq!(p.sheet(s).unwrap().full_dimensions.len(), 5);
    let sh = p.sheet(s).unwrap();
    for id in &o.full_dimensions {
        assert!(sh.full_dimension_geometry(*id).is_ok());
    }
    // Copier un seul point de `e` : la cote horizontale (deux points) n'est pas copiée, avec remarque.
    let frag = p.copy_fragment(s, &[a], &[], &[], &[]).unwrap();
    assert!(frag.full_dimensions.is_empty());
    assert!(
        frag.notes
            .iter()
            .any(|n| n.contains("cote(s) non copiée(s)")),
        "{:?}",
        frag.notes
    );
    // Un fragment dont la cote cite un objet absent est refusé à la lecture.
    let frag = p.copy_fragment(s, &[], &[e], &[], &[]).unwrap();
    let mut v: serde_json::Value = serde_json::from_str(&frag.to_json()).unwrap();
    v["full_dimensions"][0]["kind"]["entity"] = serde_json::json!(31_337);
    assert!(Fragment::from_json(&v.to_string()).is_err());
    // Un fragment sans cote ne porte pas la clé (compatibilité des anciens presse-papiers).
    let plain = p.copy_fragment(s, &[], &[e2], &[], &[]).unwrap();
    assert!(!plain.to_json().contains("full_dimensions") || !plain.full_dimensions.is_empty());
}

#[test]
fn ca8_symetrie_liee_les_cotes_suivent_les_points_miroirs() {
    let (mut p, s) = setup();
    let (axis, ..) = segment(&mut p, s, pt(0., -50.), pt(0., 50.));
    let (e, a, b) = segment(&mut p, s, pt(10., 0.), pt(40., 30.));
    let o = p
        .apply(Command::LinkMirror {
            sheet: s,
            points: vec![],
            entities: vec![e],
            axis: MirrorAxis::Segment { segment: axis },
        })
        .unwrap();
    let mirrored = o.entities[0];
    let h = add(
        &mut p,
        s,
        FullDimensionKind::Horizontal {
            from: a,
            to: b,
            offset_mm: 1.0,
        },
    )
    .unwrap();
    let len = add(
        &mut p,
        s,
        FullDimensionKind::PathLength {
            entity: mirrored,
            offset_mm: 1.0,
        },
    )
    .unwrap();
    assert!(near(value(&p, s, len), (30.0f64).hypot(30.0), 1e-9));
    // Déplacer le point d'origine déplace son miroir : la cote du tracé miroir suit.
    p.apply(Command::MovePoint {
        sheet: s,
        point: b,
        to: pt(70., 30.),
    })
    .unwrap();
    assert!(
        near(value(&p, s, len), (60.0f64).hypot(30.0), 1e-9),
        "{}",
        value(&p, s, len)
    );
    assert!(near(value(&p, s, h), 60.0, 1e-9));
    p.validate().unwrap();
}

/// Pièces d'une feuille : deux rectangles côte à côte, chacun extrait en pièce liée.
fn two_pieces(p: &mut Project, s: SheetId) -> (PieceId, PieceId) {
    let one = |p: &mut Project, x: f64, name: &str| {
        let o = p
            .apply(Command::AddRectangle {
                sheet: s,
                corner: PointRef::New(pt(x, 0.)),
                width_mm: 100.0,
                height_mm: 50.0,
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
    };
    (one(p, 0.0, "A"), one(p, 500.0, "B"))
}

#[test]
fn ca8_pieces_plan_separer_assembler_deplier_ne_perdent_aucune_cote_en_silence() {
    let (mut p, s) = setup();
    let (a, b) = two_pieces(&mut p, s);
    // Une cote sur chaque côté haut de pièce et une projection entre deux coins.
    let top = |p: &Project, piece: PieceId| {
        let sheet = p.piece_sheet(piece).unwrap();
        p.piece(piece)
            .unwrap()
            .outline
            .iter()
            .map(|e| e.entity)
            .find(|&e| match sheet.shape(e).unwrap() {
                Shape::Line(u, v) => (u.y - 50.0).abs() < 1e-9 && (v.y - 50.0).abs() < 1e-9,
                _ => false,
            })
            .unwrap()
    };
    let (ta, tb) = (top(&p, a), top(&p, b));
    let da = add(
        &mut p,
        s,
        FullDimensionKind::PathLength {
            entity: ta,
            offset_mm: 5.0,
        },
    )
    .unwrap();
    add(
        &mut p,
        s,
        FullDimensionKind::PathLength {
            entity: tb,
            offset_mm: 5.0,
        },
    )
    .unwrap();
    // Plan de pièces : sans effet sur les cotes (et inversement).
    let layout_before = p.plan_layout();
    p.apply(Command::SetFullDimensionLabel {
        sheet: s,
        dimension: da,
        label: Some("Haut".into()),
    })
    .unwrap();
    assert_eq!(p.plan_layout(), layout_before);
    let snapshot = p.sheet(s).unwrap().full_dimensions.clone();
    // Plan : placer une pièce ne touche pas à la construction ni aux cotes.
    p.apply(Command::SetPiecePlacement {
        piece: a,
        placement: Some(Placement {
            x_mm: 300.0,
            y_mm: 200.0,
            rotation_deg: 90.0,
        }),
    })
    .unwrap();
    assert_eq!(p.sheet(s).unwrap().full_dimensions, snapshot);
    // Séparer : la construction n'est pas touchée, les cotes restent et mesurent la même chose.
    let o = p
        .apply(Command::SplitPiece {
            piece: a,
            a: pt(50.0, -10.0),
            b: pt(50.0, 80.0),
        })
        .unwrap();
    assert!(!o.pieces.is_empty());
    assert_eq!(
        p.sheet(s).unwrap().full_dimensions,
        snapshot,
        "les cotes de la feuille d'origine restent"
    );
    assert!(near(value(&p, s, da), 100.0, 1e-9));
    // Les feuilles créées par séparer ne reçoivent aucune cote (documenté), sans référence morte.
    for sh in &p.sheets {
        if sh.id != s {
            assert!(sh.full_dimensions.is_empty());
        }
    }
    p.validate().unwrap();
}

#[test]
fn ca8_deplier_une_piece_laisse_les_cotes_de_la_source() {
    let (mut p, s) = setup();
    let o = p
        .apply(Command::AddRectangle {
            sheet: s,
            corner: PointRef::New(pt(0., 0.)),
            width_mm: 60.0,
            height_mm: 40.0,
        })
        .unwrap();
    let piece = p
        .apply(Command::ExtractPiece {
            sheet: s,
            source: PieceSource::Paths {
                entities: o.entities.clone(),
            },
            name: "Demi".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    let sheet = p.sheet(s).unwrap();
    // La pliure est le côté vertical gauche (x = 0).
    let fold = o
        .entities
        .iter()
        .copied()
        .find(|&e| matches!(sheet.shape(e).unwrap(), Shape::Line(u, v) if u.x.abs() < 1e-9 && v.x.abs() < 1e-9))
        .unwrap();
    let id = add(
        &mut p,
        s,
        FullDimensionKind::PathLength {
            entity: fold,
            offset_mm: -4.0,
        },
    )
    .unwrap();
    let before = p.sheet(s).unwrap().full_dimensions.clone();
    let d = p
        .apply(Command::UnfoldPiece {
            piece,
            fold,
            name: None,
        })
        .unwrap();
    assert!(!d.pieces.is_empty());
    assert_eq!(p.sheet(s).unwrap().full_dimensions, before);
    assert!(near(value(&p, s, id), 40.0, 1e-9));
    // La dépliée vit sur sa propre feuille : elle ne porte aucune cote (remarque dans la doc).
    for sh in p.sheets.iter().filter(|sh| sh.id != s) {
        assert!(sh.full_dimensions.is_empty());
    }
    p.validate().unwrap();
}

#[test]
fn ca8_calque_verrouille_protege_la_cote_et_sa_cible() {
    let (mut p, s) = setup();
    let layer = p
        .apply(Command::AddLayer {
            sheet: s,
            name: "Gabarit".into(),
        })
        .unwrap()
        .layers[0];
    let (e, a, b) = segment(&mut p, s, pt(0., 0.), pt(30., 40.));
    let free = point(&mut p, s, 100., 0.);
    p.apply(Command::SetObjectsLayer {
        sheet: s,
        points: vec![a, b],
        entities: vec![e],
        layer: Some(layer),
    })
    .unwrap();
    let h = add(
        &mut p,
        s,
        FullDimensionKind::Horizontal {
            from: a,
            to: free,
            offset_mm: 2.0,
        },
    )
    .unwrap();
    let len = add(
        &mut p,
        s,
        FullDimensionKind::PathLength {
            entity: e,
            offset_mm: 2.0,
        },
    )
    .unwrap();
    p.apply(Command::SetLayerLocked {
        sheet: s,
        layer,
        locked: true,
    })
    .unwrap();
    for cmd in [
        Command::SetFullDimensionLabel {
            sheet: s,
            dimension: h,
            label: Some("X".into()),
        },
        Command::SetFullDimensionPlacement {
            sheet: s,
            dimension: len,
            placement: 5.0,
        },
        Command::SetFullDimensionTextShift {
            sheet: s,
            dimension: len,
            text_shift_mm: 1.0,
        },
        Command::DeleteFullDimension {
            sheet: s,
            dimension: h,
        },
    ] {
        refused(&mut p, cmd, "verrouillé");
    }
    // Créer une cote sur un objet verrouillé est refusé aussi (elle ne pourrait plus être retirée).
    refused(
        &mut p,
        Command::AddFullDimension {
            sheet: s,
            kind: FullDimensionKind::Vertical {
                from: a,
                to: b,
                offset_mm: 1.0,
            },
            label: None,
            text_shift_mm: 0.0,
        },
        "verrouillé",
    );
    // Une cote qui cite seulement des objets libres reste modifiable ; déverrouiller rend tout modifiable.
    let other = point(&mut p, s, 100., 50.);
    let free_dim = add(
        &mut p,
        s,
        FullDimensionKind::Vertical {
            from: free,
            to: other,
            offset_mm: 1.0,
        },
    )
    .unwrap();
    p.apply(Command::SetFullDimensionLabel {
        sheet: s,
        dimension: free_dim,
        label: Some("ok".into()),
    })
    .unwrap();
    p.apply(Command::SetLayerLocked {
        sheet: s,
        layer,
        locked: false,
    })
    .unwrap();
    p.apply(Command::SetFullDimensionLabel {
        sheet: s,
        dimension: h,
        label: Some("X".into()),
    })
    .unwrap();
}

#[test]
fn ca8_calque_masque_masque_la_cote() {
    let (mut p, s) = setup();
    let layer = p
        .apply(Command::AddLayer {
            sheet: s,
            name: "Aide".into(),
        })
        .unwrap()
        .layers[0];
    let (e, ..) = segment(&mut p, s, pt(0., 0.), pt(30., 40.));
    let id = add(
        &mut p,
        s,
        FullDimensionKind::PathLength {
            entity: e,
            offset_mm: 2.0,
        },
    )
    .unwrap();
    assert!(p
        .sheet(s)
        .unwrap()
        .full_dimension_visible(p.sheet(s).unwrap().full_dimension(id).unwrap()));
    p.apply(Command::SetObjectsLayer {
        sheet: s,
        points: vec![],
        entities: vec![e],
        layer: Some(layer),
    })
    .unwrap();
    p.apply(Command::SetLayerVisible {
        sheet: s,
        layer,
        visible: false,
    })
    .unwrap();
    let sh = p.sheet(s).unwrap();
    assert!(!sh.full_dimension_visible(sh.full_dimension(id).unwrap()));
}

#[test]
fn les_cotations_existantes_entre_deux_points_restent_inchangees() {
    let (mut p, s) = setup();
    let (_, a, b) = segment(&mut p, s, pt(0., 0.), pt(3., 4.));
    let o = p
        .apply(Command::AddDimension {
            sheet: s,
            from: a,
            to: b,
            offset_mm: 10.0,
        })
        .unwrap();
    let before = bytes(&p);
    let g = p
        .sheet(s)
        .unwrap()
        .dimension_geometry(o.dimensions[0])
        .unwrap();
    assert_eq!(g.value_mm, 5.0);
    // Une cote complète voisine ne change ni la cotation existante ni son octet de sortie.
    let h = add(
        &mut p,
        s,
        FullDimensionKind::Horizontal {
            from: a,
            to: b,
            offset_mm: 1.0,
        },
    )
    .unwrap();
    p.apply(Command::DeleteFullDimension {
        sheet: s,
        dimension: h,
    })
    .unwrap();
    assert_eq!(p.sheet(s).unwrap().dimensions.len(), 1);
    assert_eq!(
        p.sheet(s)
            .unwrap()
            .dimension_geometry(o.dimensions[0])
            .unwrap(),
        g
    );
    assert!(before.contains("\"dimensions\""));
}
