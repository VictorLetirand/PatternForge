//! US-12.3 — `Command::ImportGeometry` : une étape annulable, refus sans trace, géométrie de
//! construction ordinaire, fusion des points, calques par nom, remarques de ce qui n'est pas créé.

use pf_core::persist;
use pf_core::*;
use pf_geometry::PointMm;

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

fn obj(item: ImportItem, layer: Option<&str>) -> ImportObject {
    ImportObject {
        item,
        layer: layer.map(str::to_string),
    }
}

fn seg(a: (f64, f64), b: (f64, f64), layer: Option<&str>) -> ImportObject {
    obj(
        ImportItem::Segment {
            from: pt(a.0, a.1),
            to: pt(b.0, b.1),
        },
        layer,
    )
}

fn setup() -> (Project, SheetId) {
    let p = Project::new("import");
    let s = p.sheets[0].id;
    (p, s)
}

fn import(
    p: &mut Project,
    sheet: SheetId,
    objects: Vec<ImportObject>,
) -> Result<Outcome, CoreError> {
    p.apply(Command::ImportGeometry { sheet, objects })
}

fn refused(p: &mut Project, sheet: SheetId, objects: Vec<ImportObject>) -> CoreError {
    let before = p.clone();
    let json_before = persist::to_json(p).unwrap();
    let err = import(p, sheet, objects).expect_err("doit être refusé");
    assert_eq!(*p, before, "refus sans trace : projet identique");
    assert_eq!(
        persist::to_json(p).unwrap(),
        json_before,
        "octets identiques"
    );
    err
}

// ---------------------------------------------------------------- CA2 : une étape annulable

#[test]
fn ca2_un_seul_pas_d_annulation_octets_identiques() {
    let (p, s) = setup();
    let mut doc = Document::new(p);
    let before = persist::to_json(doc.project()).unwrap();
    let out = doc
        .execute(Command::ImportGeometry {
            sheet: s,
            objects: vec![
                seg((0.0, 0.0), (100.0, 0.0), None),
                seg((100.0, 0.0), (100.0, 50.0), Some("Devant")),
                obj(
                    ImportItem::Circle {
                        center: pt(50.0, 25.0),
                        radius_mm: 10.0,
                    },
                    Some("Devant"),
                ),
                obj(
                    ImportItem::Arc {
                        center: pt(0.0, 0.0),
                        start: pt(30.0, 0.0),
                        end: pt(0.0, 30.0),
                        ccw: true,
                    },
                    Some("Notes"),
                ),
                obj(
                    ImportItem::Spline {
                        points: vec![pt(0.0, 100.0), pt(20.0, 120.0), pt(50.0, 100.0)],
                    },
                    None,
                ),
                obj(ImportItem::Point { at: pt(7.0, 7.0) }, None),
            ],
        })
        .unwrap();
    assert_eq!(out.entities.len(), 5, "5 tracés");
    assert_eq!(out.layers.len(), 2, "calques Devant et Notes créés");
    assert_eq!(out.sheet, Some(s));
    assert!(out.notes.is_empty(), "{:?}", out.notes);
    assert_ne!(persist::to_json(doc.project()).unwrap(), before);
    assert!(doc.undo(), "un pas d'annulation");
    assert_eq!(persist::to_json(doc.project()).unwrap(), before);
    assert!(!doc.can_undo(), "un seul pas : l'historique est vide");
    assert!(doc.redo());
    assert!(doc.project().sheet(s).unwrap().entities.len() == 5);
    assert!(doc.undo() && !doc.can_undo());
}

#[test]
fn ca2_objets_ordinaires_sans_piece_ni_contrainte_ni_role() {
    let (mut p, s) = setup();
    import(
        &mut p,
        s,
        vec![
            seg((0.0, 0.0), (10.0, 0.0), None),
            seg((10.0, 0.0), (10.0, 10.0), None),
        ],
    )
    .unwrap();
    assert!(p.pieces.is_empty());
    let sheet = p.sheet(s).unwrap();
    assert_eq!(sheet.points.len(), 3, "le point commun est fusionné");
    for e in &sheet.entities {
        assert_eq!(e.role, PathRole::Construction);
        assert!(matches!(
            e.kind,
            EntityKind::Segment {
                construction: None,
                ..
            }
        ));
        assert!(e.layer.is_none() && e.name.is_none());
    }
    for pt in &sheet.points {
        assert!(matches!(pt.definition, PointDefinition::Free { .. }));
        assert_eq!(pt.role, PointRole::Construction);
    }
    assert!(sheet.notches.is_empty() && sheet.dimensions.is_empty() && sheet.groups.is_empty());
    // Schéma inchangé, fichier relu à l'identique.
    assert_eq!(p.schema_version, 6);
    let json = persist::to_json(&p).unwrap();
    assert_eq!(persist::from_json(&json).unwrap(), p);
}

#[test]
fn ca2_un_projet_sans_import_reste_identique_a_l_octet() {
    // Rien de nouveau n'est écrit tant qu'on n'importe pas : les exemples du dépôt le prouvent
    // aussi (`generer_exemples` ne change aucun fichier). Ici : un projet relu et réécrit.
    let (p, _) = setup();
    let json = persist::to_json(&p).unwrap();
    assert_eq!(
        persist::to_json(&persist::from_json(&json).unwrap()).unwrap(),
        json
    );
    assert!(!json.contains("\"objects\"") && !json.contains("\"kind\""));
}

// ---------------------------------------------------------------- CA2 : refus sans trace

#[test]
fn ca2_refus_sans_trace() {
    let (mut p, s) = setup();
    // Un peu de contenu d'abord : le refus doit laisser exactement cet état.
    import(&mut p, s, vec![seg((0.0, 0.0), (5.0, 0.0), Some("Base"))]).unwrap();
    let good = seg((1.0, 1.0), (2.0, 2.0), None);

    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 2.0e6, -1.0e30] {
        let e = refused(
            &mut p,
            s,
            vec![good.clone(), seg((0.0, 0.0), (bad, 1.0), None)],
        );
        assert!(matches!(e, CoreError::Geometry(_)), "{bad} : {e}");
    }
    // Rayon invalide.
    refused(
        &mut p,
        s,
        vec![obj(
            ImportItem::Circle {
                center: pt(0.0, 0.0),
                radius_mm: f64::NAN,
            },
            None,
        )],
    );
    // Liste vide, trop longue.
    let e = refused(&mut p, s, vec![]);
    assert!(e.to_string().contains("rien à importer"));
    let many: Vec<ImportObject> = (0..MAX_IMPORT_OBJECTS + 1)
        .map(|i| {
            obj(
                ImportItem::Point {
                    at: pt(i as f64, 0.0),
                },
                None,
            )
        })
        .collect();
    let e = refused(&mut p, s, many);
    assert!(e.to_string().contains("limite"), "{e}");
    // Courbe à un seul point, nom de calque invalide.
    refused(
        &mut p,
        s,
        vec![obj(
            ImportItem::Spline {
                points: vec![pt(0.0, 0.0)],
            },
            None,
        )],
    );
    for name in ["", "   ", "a\u{7}b", &"x".repeat(121)] {
        if name.trim().is_empty() {
            // Un nom vide désigne le calque par défaut : accepté, pas un refus.
            continue;
        }
        refused(&mut p, s, vec![seg((0.0, 0.0), (1.0, 0.0), Some(name))]);
    }
    // Feuille inconnue.
    let e = refused(&mut p, SheetId(999_999), vec![good]);
    assert!(matches!(e, CoreError::UnknownSheet(_)));
}

#[test]
fn ca2_collision_d_identifiants_refusee_sans_trace() {
    let (mut p, s) = setup();
    // Compteur forgé au maximum : les nouveaux identifiants débordent, validate() refuse.
    p.next_id = u64::MAX - 1;
    let before = p.clone();
    let e = import(
        &mut p,
        s,
        vec![
            seg((0.0, 0.0), (10.0, 0.0), None),
            seg((10.0, 0.0), (10.0, 10.0), None),
        ],
    )
    .expect_err("collision d'identifiants");
    assert!(
        e.to_string().contains("identifiant") || matches!(e, CoreError::DuplicateId(_)),
        "{e}"
    );
    assert_eq!(p, before);
}

#[test]
fn ca2_calque_verrouille_refuse_l_import_sans_trace() {
    let (mut p, s) = setup();
    import(&mut p, s, vec![seg((0.0, 0.0), (5.0, 0.0), Some("Fixe"))]).unwrap();
    let layer = p.sheet(s).unwrap().layers[0].id;
    p.apply(Command::SetLayerLocked {
        sheet: s,
        layer,
        locked: true,
    })
    .unwrap();
    let e = refused(&mut p, s, vec![seg((0.0, 10.0), (5.0, 10.0), Some("Fixe"))]);
    assert!(matches!(e, CoreError::LayerLocked(_)), "{e}");
    // Un autre calque reste possible.
    import(
        &mut p,
        s,
        vec![seg((0.0, 10.0), (5.0, 10.0), Some("Libre"))],
    )
    .unwrap();
}

// ---------------------------------------------------------------- fusion, calques, remarques

#[test]
fn les_points_confondus_fusionnent_entre_eux_et_avec_la_feuille() {
    let (mut p, s) = setup();
    p.apply(Command::AddPoint {
        sheet: s,
        at: pt(0.0, 0.0),
    })
    .unwrap();
    let out = import(
        &mut p,
        s,
        vec![
            seg((0.0, 0.0), (10.0, 0.0), None),
            seg((10.0 + 5e-7, 0.0), (10.0, 10.0), None),
        ],
    )
    .unwrap();
    let sheet = p.sheet(s).unwrap();
    assert_eq!(
        sheet.points.len(),
        3,
        "A existant, (10;0) fusionné, (10;10)"
    );
    assert_eq!(out.points.len(), 2, "deux points créés seulement");
    assert_eq!(sheet.entities.len(), 2);
}

#[test]
fn doublons_et_objets_nuls_sont_signales_jamais_silencieux() {
    let (mut p, s) = setup();
    let out = import(
        &mut p,
        s,
        vec![
            seg((0.0, 0.0), (10.0, 0.0), None),
            seg((10.0, 0.0), (0.0, 0.0), None), // même segment, sens inverse
            seg((0.0, 0.0), (5e-7, 0.0), None), // nul une fois les points fusionnés
            obj(ImportItem::Point { at: pt(0.0, 0.0) }, None), // déjà là
            seg((0.0, 5.0), (10.0, 5.0), None),
        ],
    )
    .unwrap();
    assert_eq!(out.entities.len(), 2);
    let notes = out.notes.join(" | ");
    assert!(notes.contains("déjà présent"), "{notes}");
    assert!(notes.contains("longueur nulle"), "{notes}");
    assert_eq!(p.sheet(s).unwrap().points.len(), 4);
}

#[test]
fn arc_ou_courbe_incoherents_apres_fusion_sont_signales() {
    let (mut p, s) = setup();
    let out = import(
        &mut p,
        s,
        vec![
            // Extrémités à des rayons différents : arc impossible.
            obj(
                ImportItem::Arc {
                    center: pt(0.0, 0.0),
                    start: pt(10.0, 0.0),
                    end: pt(0.0, 12.0),
                    ccw: true,
                },
                None,
            ),
            // Un point de passage qui revient.
            obj(
                ImportItem::Spline {
                    points: vec![pt(0.0, 50.0), pt(10.0, 60.0), pt(0.0, 50.0)],
                },
                None,
            ),
            obj(
                ImportItem::Arc {
                    center: pt(100.0, 0.0),
                    start: pt(110.0, 0.0),
                    end: pt(100.0, 10.0),
                    ccw: true,
                },
                None,
            ),
        ],
    )
    .unwrap();
    assert_eq!(out.entities.len(), 1);
    assert!(
        out.notes.join(" ").contains("incohérent"),
        "{:?}",
        out.notes
    );
    // Pas de point orphelin laissé par les objets refusés : 3 points (arc valide) seulement.
    assert_eq!(p.sheet(s).unwrap().points.len(), 3);
}

#[test]
fn calques_crees_par_nom_puis_reutilises() {
    let (mut p, s) = setup();
    let out = import(
        &mut p,
        s,
        vec![
            seg((0.0, 0.0), (1.0, 0.0), Some("Coupe")),
            seg((0.0, 1.0), (1.0, 1.0), Some(" Coupe ")),
            seg((0.0, 2.0), (1.0, 2.0), Some("Aide")),
        ],
    )
    .unwrap();
    assert_eq!(out.layers.len(), 2);
    let again = import(&mut p, s, vec![seg((0.0, 3.0), (1.0, 3.0), Some("Coupe"))]).unwrap();
    assert!(again.layers.is_empty(), "calque existant réutilisé");
    let sheet = p.sheet(s).unwrap();
    assert_eq!(sheet.layers.len(), 2);
    let coupe = sheet.layers.iter().find(|l| l.name == "Coupe").unwrap().id;
    assert_eq!(
        sheet
            .entities
            .iter()
            .filter(|e| e.layer == Some(coupe))
            .count(),
        3
    );
    // Les points créés dans un calque portent ce calque.
    assert!(sheet.points.iter().any(|q| q.layer == Some(coupe)));
}

#[test]
fn etiquettes_de_points_uniques_et_ordonnees() {
    let (mut p, s) = setup();
    let objs: Vec<ImportObject> = (0..60)
        .map(|i| {
            obj(
                ImportItem::Point {
                    at: pt(i as f64 * 10.0, 0.0),
                },
                None,
            )
        })
        .collect();
    import(&mut p, s, objs).unwrap();
    let sheet = p.sheet(s).unwrap();
    let mut labels: Vec<&str> = sheet.points.iter().map(|q| q.label.as_str()).collect();
    assert_eq!(labels[0], "A");
    assert_eq!(labels[26], "A1");
    labels.sort();
    labels.dedup();
    assert_eq!(labels.len(), 60);
}

#[test]
fn deterministe_deux_imports_identiques_donnent_les_memes_octets() {
    let objs = vec![
        seg((0.0, 0.0), (10.0, 0.0), Some("A")),
        obj(
            ImportItem::Circle {
                center: pt(3.0, 3.0),
                radius_mm: 2.0,
            },
            Some("B"),
        ),
    ];
    let (mut p1, s1) = setup();
    let (mut p2, s2) = setup();
    import(&mut p1, s1, objs.clone()).unwrap();
    import(&mut p2, s2, objs).unwrap();
    assert_eq!(
        persist::to_json(&p1).unwrap(),
        persist::to_json(&p2).unwrap()
    );
}

#[test]
fn la_commande_survit_a_la_serialisation_json() {
    let (_, s) = setup();
    let c = Command::ImportGeometry {
        sheet: s,
        objects: vec![
            seg((0.0, 0.0), (1.0, 1.0), Some("X")),
            obj(
                ImportItem::Arc {
                    center: pt(0.0, 0.0),
                    start: pt(1.0, 0.0),
                    end: pt(0.0, 1.0),
                    ccw: false,
                },
                None,
            ),
        ],
    };
    let json = serde_json::to_string(&c).unwrap();
    assert_eq!(serde_json::from_str::<Command>(&json).unwrap(), c);
}

// ---------------------------------------------------------------- borne d'import (200 000)

fn disjoint_segments(n: usize) -> Vec<ImportObject> {
    (0..n)
        .map(|i| seg((i as f64 * 3.0, 0.0), (i as f64 * 3.0 + 1.0, 0.0), None))
        .collect()
}

#[test]
fn import_a_la_borne_passe_la_validation_en_temps_borne() {
    assert_eq!(MAX_IMPORT_OBJECTS, 200_000);
    let (mut p, s) = setup();
    let t = std::time::Instant::now();
    let out = import(&mut p, s, disjoint_segments(MAX_IMPORT_OBJECTS)).unwrap();
    let secs = t.elapsed().as_secs_f64();
    eprintln!("import de {MAX_IMPORT_OBJECTS} segments (apply + validate) : {secs:.2} s");
    assert_eq!(out.entities.len(), MAX_IMPORT_OBJECTS);
    assert_eq!(out.points.len(), 2 * MAX_IMPORT_OBJECTS);
    p.validate().unwrap();
    assert!(secs < 60.0, "import à la borne : {secs} s");
}

#[test]
fn import_au_dessus_de_la_borne_refuse_sans_trace() {
    let (mut p, s) = setup();
    let e = refused(&mut p, s, disjoint_segments(MAX_IMPORT_OBJECTS + 1));
    let msg = e.to_string();
    assert!(
        msg.contains("200001") && msg.contains("200000") && msg.contains("limite"),
        "{msg}"
    );
}
