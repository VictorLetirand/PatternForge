//! Validation de feuille indexée : performance bornée et équivalence de verdict
//! (mêmes erreurs, mêmes messages que la recherche linéaire d'origine).

use pf_core::*;
use std::time::Instant;

fn seg(id: u64, from: u64, to: u64) -> Entity {
    Entity {
        id: EntityId(id),
        role: PathRole::Construction,
        kind: EntityKind::Segment {
            from: PointId(from),
            to: PointId(to),
            construction: None,
        },
        layer: None,
        name: None,
    }
}

fn free(id: u64, x: f64, y: f64) -> Point {
    Point {
        id: PointId(id),
        label: format!("P{id}"),
        definition: PointDefinition::Free { x, y },
        role: PointRole::default(),
        layer: None,
    }
}

/// Feuille de `n` segments chaînés (n + 1 points), avec calque, crans, cotations, groupes.
fn big_project(n: u64) -> Project {
    let mut p = Project::new("Grand");
    let layer = LayerId(2);
    let sh = &mut p.sheets[0];
    sh.layers.push(Layer {
        id: layer,
        name: "Calque".into(),
        visible: true,
        locked: false,
    });
    let mut next = 3;
    let mut alloc = || {
        next += 1;
        next - 1
    };
    let pts: Vec<u64> = (0..=n).map(|_| alloc()).collect();
    for (i, &id) in pts.iter().enumerate() {
        let mut pt = free(id, i as f64, (i % 7) as f64);
        if i % 3 == 0 {
            pt.layer = Some(layer);
        }
        sh.points.push(pt);
    }
    let mut ents = Vec::new();
    for i in 0..n as usize {
        let id = alloc();
        let mut e = seg(id, pts[i], pts[i + 1]);
        if i % 5 == 0 {
            e.layer = Some(layer);
        }
        ents.push(id);
        sh.entities.push(e);
    }
    for (k, &e) in ents.iter().enumerate().step_by(10) {
        sh.notches.push(Notch {
            id: NotchId(alloc()),
            entity: EntityId(e),
            distance_mm: 0.5,
            side: pf_geometry::Side::Left,
            length_mm: 5.0,
            style: NotchStyle::Single,
            anchor: None,
            form: NotchForm::Line,
            width_mm: None,
            on_cut: false,
            corner: false,
            proportional: false,
        });
        sh.dimensions.push(Dimension {
            id: DimensionId(alloc()),
            from: PointId(pts[k]),
            to: PointId(pts[k + 1]),
            offset_mm: 10.0,
        });
    }
    for chunk in ents.chunks(100) {
        sh.groups.push(Group {
            id: GroupId(alloc()),
            members: chunk.iter().map(|&e| EntityId(e)).collect(),
        });
    }
    p.next_id = next;
    p
}

fn time_validate(n: u64) -> f64 {
    let p = big_project(n);
    let t = Instant::now();
    p.validate().unwrap();
    let s = t.elapsed().as_secs_f64();
    eprintln!("validate {n} segments ({} points) : {s:.3} s", n + 1);
    s
}

#[test]
fn performance_validation_20000_puis_100000() {
    let a = time_validate(20_000);
    let b = time_validate(100_000);
    assert!(a < 5.0, "20 000 objets : {a} s");
    assert!(b < 10.0, "100 000 objets : {b} s");
}

/// Projet de base valide, petit : 4 points, 3 segments, un calque.
fn small() -> Project {
    let mut p = Project::new("Petit");
    let sh = &mut p.sheets[0];
    sh.layers.push(Layer {
        id: LayerId(10),
        name: "L".into(),
        visible: true,
        locked: false,
    });
    for (i, (x, y)) in [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)]
        .into_iter()
        .enumerate()
    {
        sh.points.push(free(20 + i as u64, x, y));
    }
    sh.entities.push(seg(30, 20, 21));
    sh.entities.push(seg(31, 21, 22));
    sh.entities.push(seg(32, 22, 23));
    p.next_id = 100;
    p.validate().unwrap();
    p
}

type Mutation = fn(&mut Project);

fn cases() -> Vec<(&'static str, Mutation)> {
    vec![
        ("segment_point_absent", |p| {
            p.sheets[0].entities[1].kind = EntityKind::Segment {
                from: PointId(21),
                to: PointId(77),
                construction: None,
            }
        }),
        ("segment_depart_absent", |p| {
            p.sheets[0].entities[0].kind = EntityKind::Segment {
                from: PointId(78),
                to: PointId(21),
                construction: None,
            }
        }),
        ("segment_degenere", |p| {
            p.sheets[0].entities[0].kind = EntityKind::Segment {
                from: PointId(20),
                to: PointId(20),
                construction: None,
            }
        }),
        ("cercle_centre_absent", |p| {
            p.sheets[0].entities.push(Entity {
                id: EntityId(40),
                role: PathRole::Construction,
                kind: EntityKind::Circle {
                    center: PointId(79),
                    radius_mm: 5.0,
                    start_deg: 0.0,
                    ccw: true,
                },
                layer: None,
                name: None,
            })
        }),
        ("point_sur_segment_absent", |p| {
            p.sheets[0].points.push(Point {
                id: PointId(50),
                label: "M".into(),
                definition: PointDefinition::OnSegmentRatio {
                    segment: EntityId(66),
                    ratio: 0.5,
                },
                role: PointRole::default(),
                layer: None,
            })
        }),
        ("point_sur_segment_valide", |p| {
            p.sheets[0].points.push(Point {
                id: PointId(50),
                label: "M".into(),
                definition: PointDefinition::OnSegmentRatio {
                    segment: EntityId(32),
                    ratio: 0.5,
                },
                role: PointRole::default(),
                layer: None,
            })
        }),
        ("cran_sur_trace_absent", |p| {
            p.sheets[0].notches.push(Notch {
                id: NotchId(51),
                entity: EntityId(88),
                distance_mm: 1.0,
                side: pf_geometry::Side::Left,
                length_mm: 5.0,
                style: NotchStyle::Single,
                anchor: None,
                form: NotchForm::Line,
                width_mm: None,
                on_cut: false,
                corner: false,
                proportional: false,
            })
        }),
        ("cran_hors_trace", |p| {
            p.sheets[0].notches.push(Notch {
                id: NotchId(51),
                entity: EntityId(30),
                distance_mm: 500.0,
                side: pf_geometry::Side::Left,
                length_mm: 5.0,
                style: NotchStyle::Single,
                anchor: None,
                form: NotchForm::Line,
                width_mm: None,
                on_cut: false,
                corner: false,
                proportional: false,
            })
        }),
        ("cotation_point_absent", |p| {
            p.sheets[0].dimensions.push(Dimension {
                id: DimensionId(52),
                from: PointId(20),
                to: PointId(99),
                offset_mm: 5.0,
            })
        }),
        ("cotation_depart_absent", |p| {
            p.sheets[0].dimensions.push(Dimension {
                id: DimensionId(52),
                from: PointId(98),
                to: PointId(20),
                offset_mm: 5.0,
            })
        }),
        ("cotation_memes_points", |p| {
            p.sheets[0].dimensions.push(Dimension {
                id: DimensionId(52),
                from: PointId(20),
                to: PointId(20),
                offset_mm: 5.0,
            })
        }),
        ("groupe_membre_absent", |p| {
            p.sheets[0].groups.push(Group {
                id: GroupId(53),
                members: vec![EntityId(30), EntityId(97)],
            })
        }),
        ("groupe_vide", |p| {
            p.sheets[0].groups.push(Group {
                id: GroupId(53),
                members: vec![],
            })
        }),
        ("groupes_qui_se_recoupent", |p| {
            p.sheets[0].groups.push(Group {
                id: GroupId(53),
                members: vec![EntityId(30)],
            });
            p.sheets[0].groups.push(Group {
                id: GroupId(54),
                members: vec![EntityId(30), EntityId(31)],
            });
        }),
        ("point_sur_calque_absent", |p| {
            p.sheets[0].points[2].layer = Some(LayerId(96))
        }),
        ("trace_sur_calque_absent", |p| {
            p.sheets[0].entities[2].layer = Some(LayerId(95))
        }),
        ("point_sur_calque_valide", |p| {
            p.sheets[0].points[2].layer = Some(LayerId(10))
        }),
        ("guide_sur_calque_absent", |p| {
            p.sheets[0].guides.push(Guide {
                id: GuideId(55),
                x_mm: 0.0,
                y_mm: 0.0,
                angle_deg: 0.0,
                axis: false,
                layer: Some(LayerId(94)),
            })
        }),
        ("id_point_en_double", |p| {
            p.sheets[0].points[3].id = PointId(20)
        }),
        ("id_trace_en_double", |p| {
            p.sheets[0].entities[2].id = EntityId(30)
        }),
        ("id_point_egal_a_un_trace", |p| {
            p.sheets[0].points[3].id = PointId(31)
        }),
        ("nom_de_trace_en_double", |p| {
            p.sheets[0].entities[0].name = Some("A".into());
            p.sheets[0].entities[1].name = Some("A".into());
        }),
        ("nom_de_calque_en_double", |p| {
            p.sheets[0].layers.push(Layer {
                id: LayerId(11),
                name: "L".into(),
                visible: true,
                locked: false,
            })
        }),
        ("arc_points_confondus", |p| {
            p.sheets[0].entities.push(Entity {
                id: EntityId(41),
                role: PathRole::Construction,
                kind: EntityKind::Arc {
                    center: PointId(20),
                    start: PointId(21),
                    end: PointId(21),
                    ccw: true,
                },
                layer: None,
                name: None,
            })
        }),
        ("arc_point_absent", |p| {
            p.sheets[0].entities.push(Entity {
                id: EntityId(41),
                role: PathRole::Construction,
                kind: EntityKind::Arc {
                    center: PointId(20),
                    start: PointId(21),
                    end: PointId(93),
                    ccw: true,
                },
                layer: None,
                name: None,
            })
        }),
        ("identifiant_au_dela_du_compteur", |p| {
            p.sheets[0].points[0].id = PointId(5000)
        }),
    ]
}

fn verdict(p: &Project) -> String {
    match p.validate() {
        Ok(()) => "OK".to_string(),
        Err(e) => format!("{e:?} | {e}"),
    }
}

#[test]
fn equivalence_des_verdicts() {
    let expected: &[(&str, &str)] = EXPECTED;
    let got: Vec<(String, String)> = cases()
        .into_iter()
        .map(|(name, f)| {
            let mut p = small();
            f(&mut p);
            (name.to_string(), verdict(&p))
        })
        .collect();
    if expected.is_empty() {
        for (n, v) in &got {
            println!("        (\"{n}\", {v:?}),");
        }
        panic!("table EXPECTED vide");
    }
    assert_eq!(got.len(), expected.len());
    for ((n, v), (en, ev)) in got.iter().zip(expected) {
        assert_eq!(n, en);
        assert_eq!(v, ev, "verdict différent pour {n}");
    }
}

const EXPECTED: &[(&str, &str)] = &[
        ("segment_point_absent", "DeadReference(\"le tracé 31 référence un point absent (id 77)\") | référence manquante : le tracé 31 référence un point absent (id 77)"),
        ("segment_depart_absent", "DeadReference(\"le tracé 30 référence un point absent (id 78)\") | référence manquante : le tracé 30 référence un point absent (id 78)"),
        ("segment_degenere", "Invalid(\"Segment P20P20 : segment de longueur nulle : direction indéfinie\") | Segment P20P20 : segment de longueur nulle : direction indéfinie"),
        ("cercle_centre_absent", "DeadReference(\"le tracé 40 référence un point absent (id 79)\") | référence manquante : le tracé 40 référence un point absent (id 79)"),
        ("point_sur_segment_absent", "DeadReference(\"le point M dépend d'un segment absent (id 66)\") | référence manquante : le point M dépend d'un segment absent (id 66)"),
        ("point_sur_segment_valide", "OK"),
        ("cran_sur_trace_absent", "DeadReference(\"un cran référence un tracé absent (id 88)\") | référence manquante : un cran référence un tracé absent (id 88)"),
        ("cran_hors_trace", "Invalid(\"cran sur Segment P20P21 : la position 500,0 mm est hors du tracé (longueur 10,0 mm)\") | cran sur Segment P20P21 : la position 500,0 mm est hors du tracé (longueur 10,0 mm)"),
        ("cotation_point_absent", "UnknownPoint(PointId(99)) | point introuvable (id 99)"),
        ("cotation_depart_absent", "UnknownPoint(PointId(98)) | point introuvable (id 98)"),
        ("cotation_memes_points", "Invalid(\"cotation : les deux points doivent être différents\") | cotation : les deux points doivent être différents"),
        ("groupe_membre_absent", "DeadReference(\"un groupe référence un tracé absent (id 97)\") | référence manquante : un groupe référence un tracé absent (id 97)"),
        ("groupe_vide", "Invalid(\"un groupe ne peut pas être vide\") | un groupe ne peut pas être vide"),
        ("groupes_qui_se_recoupent", "Invalid(\"le tracé 30 est dans deux groupes à la fois\") | le tracé 30 est dans deux groupes à la fois"),
        ("point_sur_calque_absent", "DeadReference(\"le point P22 référence un calque absent (id 96)\") | référence manquante : le point P22 référence un calque absent (id 96)"),
        ("trace_sur_calque_absent", "DeadReference(\"le tracé 32 référence un calque absent (id 95)\") | référence manquante : le tracé 32 référence un calque absent (id 95)"),
        ("point_sur_calque_valide", "OK"),
        ("guide_sur_calque_absent", "DeadReference(\"une ligne d'aide référence un calque absent (id 94)\") | référence manquante : une ligne d'aide référence un calque absent (id 94)"),
        ("id_point_en_double", "DuplicateId(20) | identifiant en double dans le projet (20)"),
        ("id_trace_en_double", "DuplicateId(30) | identifiant en double dans le projet (30)"),
        ("id_point_egal_a_un_trace", "DuplicateId(31) | identifiant en double dans le projet (31)"),
        ("nom_de_trace_en_double", "Invalid(\"le nom de tracé « A » est utilisé deux fois\") | le nom de tracé « A » est utilisé deux fois"),
        ("nom_de_calque_en_double", "Invalid(\"le nom de calque « L » est utilisé deux fois\") | le nom de calque « L » est utilisé deux fois"),
        ("arc_points_confondus", "Invalid(\"Arc P21P21 (centre P20) : le centre et les extrémités doivent être trois points distincts\") | Arc P21P21 (centre P20) : le centre et les extrémités doivent être trois points distincts"),
        ("arc_point_absent", "DeadReference(\"le tracé 41 référence un point absent (id 93)\") | référence manquante : le tracé 41 référence un point absent (id 93)"),
        ("identifiant_au_dela_du_compteur", "Invalid(\"identifiant 5000 incohérent avec le compteur du projet (100)\") | identifiant 5000 incohérent avec le compteur du projet (100)"),
];
