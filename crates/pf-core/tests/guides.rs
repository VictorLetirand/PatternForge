//! US-02.8 — lignes d'aide : objets persistants de la feuille, commandes
//! annulables et validées, accrochables (géométrie), verrouillage des calques,
//! copie de feuille et presse-papiers, axe de symétrie.

use pf_core::*;
use pf_geometry::{PointMm, Transform};

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

fn add_guide(p: &mut Project, sheet: SheetId, x: f64, y: f64, angle: f64) -> GuideId {
    p.apply(Command::AddGuide {
        sheet,
        x_mm: x,
        y_mm: y,
        angle_deg: angle,
        axis: false,
        layer: None,
    })
    .unwrap()
    .guides[0]
}

fn layer(p: &mut Project, sheet: SheetId, name: &str) -> LayerId {
    p.apply(Command::AddLayer {
        sheet,
        name: name.into(),
    })
    .unwrap()
    .layers[0]
}

fn refused(p: &mut Project, command: Command) -> CoreError {
    let before = p.clone();
    let err = p.apply(command).expect_err("doit être refusée");
    assert!(!err.to_string().is_empty());
    assert_eq!(*p, before, "une commande refusée ne laisse aucune trace");
    err
}

// ---------------------------------------------------- persistance (CA 1)

#[test]
fn persistance_guides_omis_quand_vide_et_fichier_sans_guide_inchange() {
    let mut p = demo_piece_project().unwrap();
    let sheet = p.sheets[0].id;
    let without = persist::to_json(&p).unwrap();
    assert!(!without.contains("guides"), "clé omise quand vide");
    assert_eq!(p.schema_version, 6);

    let n0 = p.next_id;
    let g = add_guide(&mut p, sheet, 0.0, 20.0, 0.0);
    let with = persist::to_json(&p).unwrap();
    assert!(with.contains("\"guides\""));
    assert_eq!(p.schema_version, 6, "schéma 4 (US-09.1)");
    assert!(!with.contains("\"axis\""), "axe omis quand faux");
    assert!(!with.contains("\"layer\": null"));
    let back = persist::from_json(&with).unwrap();
    assert_eq!(back, p);
    assert_eq!(persist::to_json(&back).unwrap(), with);

    // Supprimer le dernier guide ramène exactement les octets d'origine, au
    // compteur d'identifiants près (un identifiant n'est jamais réutilisé).
    p.apply(Command::DeleteGuide { sheet, guide: g }).unwrap();
    let expected = without.replace(
        &format!("\"next_id\": {n0}"),
        &format!("\"next_id\": {}", p.next_id),
    );
    assert_eq!(persist::to_json(&p).unwrap(), expected);
    // Relire puis réécrire un fichier sans guide : octets identiques.
    let reread = persist::from_json(&without).unwrap();
    assert_eq!(persist::to_json(&reread).unwrap(), without);
}

#[test]
fn persistance_sur_disque_avec_axe_et_calque() {
    let mut p = Project::new("guides");
    let sheet = p.sheets[0].id;
    let l = layer(&mut p, sheet, "Aides");
    p.apply(Command::AddGuide {
        sheet,
        x_mm: 12.5,
        y_mm: -3.25,
        angle_deg: 30.0,
        axis: true,
        layer: Some(l),
    })
    .unwrap();
    let dir = std::env::temp_dir().join(format!("pf-guides-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("g.pforge.json");
    persist::save(&file, &p).unwrap();
    let back = persist::load(&file).unwrap();
    assert_eq!(back, p);
    let g = &back.sheets[0].guides[0];
    assert!(g.axis);
    assert_eq!(g.layer, Some(l));
    assert_eq!((g.x_mm, g.y_mm, g.angle_deg), (12.5, -3.25, 30.0));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn fichier_avec_guide_invalide_refuse_a_l_ouverture() {
    let mut p = Project::new("g");
    let sheet = p.sheets[0].id;
    add_guide(&mut p, sheet, 0.0, 0.0, 0.0);
    let text = persist::to_json(&p).unwrap();
    for (from, to) in [
        ("\"angle_deg\": 0.0", "\"angle_deg\": null"),
        ("\"x_mm\": 0.0", "\"x_mm\": 1e300"),
        ("\"x_mm\": 0.0", "\"x_mm\": \"a\""),
        ("\"angle_deg\": 0.0", "\"angle_deg\": 0.0, \"inconnu\": 1"),
        (
            "\"angle_deg\": 0.0",
            "\"angle_deg\": 0.0, \"layer\": 424242",
        ),
    ] {
        let bad = text.replacen(from, to, 1);
        assert_ne!(bad, text, "{from}");
        assert!(persist::from_json(&bad).is_err(), "{to} doit être refusé");
    }
    // Identifiant en double avec un autre objet : refusé.
    let id = p.sheets[0].guides[0].id.0;
    let dup = text.replacen(
        &format!("\"id\": {id}"),
        &format!("\"id\": {}", p.sheets[0].id.0),
        1,
    );
    assert!(persist::from_json(&dup).is_err());
}

// ------------------------------------------- commandes, undo, validation

#[test]
fn ajouter_deplacer_supprimer_est_annulable() {
    let mut doc = Document::new(Project::new("g"));
    let sheet = doc.project().sheets[0].id;
    let before = doc.project().clone();
    let out = doc
        .execute(Command::AddGuide {
            sheet,
            x_mm: 10.0,
            y_mm: 20.0,
            angle_deg: 200.0,
            axis: false,
            layer: None,
        })
        .unwrap();
    let g = out.guides[0];
    // Angle ramené dans [0 ; 180[.
    assert_eq!(doc.project().sheets[0].guide(g).unwrap().angle_deg, 20.0);
    doc.execute(Command::MoveGuide {
        sheet,
        guide: g,
        x_mm: 5.0,
        y_mm: 6.0,
        angle_deg: -90.0,
    })
    .unwrap();
    let moved = doc.project().sheets[0].guide(g).unwrap().clone();
    assert_eq!((moved.x_mm, moved.y_mm, moved.angle_deg), (5.0, 6.0, 90.0));
    doc.execute(Command::SetGuideAxis {
        sheet,
        guide: g,
        axis: true,
    })
    .unwrap();
    assert!(doc.project().sheets[0].guide(g).unwrap().axis);
    doc.execute(Command::DeleteGuide { sheet, guide: g })
        .unwrap();
    assert!(doc.project().sheets[0].guides.is_empty());
    for _ in 0..4 {
        assert!(doc.undo());
    }
    assert_eq!(*doc.project(), before);
    assert!(doc.redo());
    assert_eq!(doc.project().sheets[0].guides.len(), 1);
    // Identifiant jamais réutilisé après suppression.
    let mut p = Project::new("ids");
    let s = p.sheets[0].id;
    let a = add_guide(&mut p, s, 0.0, 0.0, 0.0);
    p.apply(Command::DeleteGuide { sheet: s, guide: a })
        .unwrap();
    let b = add_guide(&mut p, s, 0.0, 0.0, 0.0);
    assert_ne!(a, b);
}

/// Défaut D-03 (trouvé par la campagne adverse) : `rem_euclid(180)` d'un petit
/// angle négatif rend exactement 180, hors de `[0 ; 180[`.
#[test]
fn angle_normalise_dans_zero_cent_quatre_vingts_exclu() {
    let mut p = Project::new("g");
    let s = p.sheets[0].id;
    for (given, expected) in [
        (-1e-300, 0.0),
        (-0.0, 0.0),
        (180.0, 0.0),
        (-180.0, 0.0),
        (-1.0, 179.0),
        (359.5, 179.5),
        (1e15, 100.0),
    ] {
        let g = add_guide(&mut p, s, 0.0, 0.0, given);
        let angle = p.sheets[0].guide(g).unwrap().angle_deg;
        assert_eq!(angle, expected, "{given}");
        assert!(angle.is_sign_positive(), "{given} : -0 évité");
    }
}

#[test]
fn refus_sans_trace_valeurs_invalides_et_references_absentes() {
    let mut p = Project::new("g");
    let sheet = p.sheets[0].id;
    let g = add_guide(&mut p, sheet, 1.0, 2.0, 45.0);
    let add = |x: f64, y: f64, a: f64, layer: Option<LayerId>, sheet: SheetId| Command::AddGuide {
        sheet,
        x_mm: x,
        y_mm: y,
        angle_deg: a,
        axis: false,
        layer,
    };
    for (x, y, a) in [
        (f64::NAN, 0.0, 0.0),
        (0.0, f64::INFINITY, 0.0),
        (0.0, 0.0, f64::NAN),
        (0.0, 0.0, f64::NEG_INFINITY),
        (1e9, 0.0, 0.0),
        (0.0, -1e9, 0.0),
    ] {
        refused(&mut p, add(x, y, a, None, sheet));
        refused(
            &mut p,
            Command::MoveGuide {
                sheet,
                guide: g,
                x_mm: x,
                y_mm: y,
                angle_deg: a,
            },
        );
    }
    refused(&mut p, add(0.0, 0.0, 0.0, Some(LayerId(777)), sheet));
    refused(&mut p, add(0.0, 0.0, 0.0, None, SheetId(777)));
    let missing = GuideId(777);
    assert!(matches!(
        refused(
            &mut p,
            Command::DeleteGuide {
                sheet,
                guide: missing
            }
        ),
        CoreError::UnknownGuide(_)
    ));
    refused(
        &mut p,
        Command::SetGuideAxis {
            sheet,
            guide: missing,
            axis: true,
        },
    );
    refused(
        &mut p,
        Command::MoveGuide {
            sheet,
            guide: missing,
            x_mm: 0.0,
            y_mm: 0.0,
            angle_deg: 0.0,
        },
    );
    // Un lot dont une commande échoue ne laisse rien (tout ou rien).
    refused(
        &mut p,
        Command::Batch {
            commands: vec![
                add(5.0, 5.0, 0.0, None, sheet),
                Command::DeleteGuide {
                    sheet,
                    guide: missing,
                },
            ],
        },
    );
}

// ------------------------------------------------------- géométrie (CA 3)

#[test]
fn geometrie_distance_projection_et_intersection_de_guides() {
    let h = Guide {
        id: GuideId(1),
        x_mm: 0.0,
        y_mm: 20.0,
        angle_deg: 0.0,
        axis: false,
        layer: None,
    };
    let v = Guide {
        angle_deg: 90.0,
        x_mm: 50.0,
        y_mm: 0.0,
        ..h.clone()
    };
    assert!((h.distance_to(pt(7.0, 25.0)) - 5.0).abs() < 1e-12);
    assert_eq!(h.project(pt(7.0, 25.0)), pt(7.0, 20.0));
    assert_eq!(h.intersection(&v), Some(pt(50.0, 20.0)));
    let oblique = Guide {
        angle_deg: 45.0,
        x_mm: 0.0,
        y_mm: 0.0,
        ..h.clone()
    };
    let x = oblique.intersection(&h).unwrap();
    assert!((x.x - 20.0).abs() < 1e-9 && (x.y - 20.0).abs() < 1e-9);
    // Parallèles ou confondus : pas d'intersection unique.
    assert_eq!(h.intersection(&h.clone()), None);
    let h2 = Guide {
        y_mm: 30.0,
        ..h.clone()
    };
    assert_eq!(h.intersection(&h2), None);
}

#[test]
fn accrochage_intersection_guide_trace_segment_cercle_et_courbe() {
    let mut p = Project::new("g");
    let s = p.sheets[0].id;
    let seg = p
        .apply(Command::AddSegment {
            sheet: s,
            from: PointRef::New(pt(0.0, 0.0)),
            to: PointRef::New(pt(100.0, 0.0)),
        })
        .unwrap()
        .entities[0];
    let circle = p
        .apply(Command::AddCircle {
            sheet: s,
            center: PointRef::New(pt(50.0, 50.0)),
            radius_mm: 10.0,
        })
        .unwrap()
        .entities[0];
    let curve = p
        .apply(Command::AddCurve {
            sheet: s,
            start: PointRef::New(pt(0.0, 10.0)),
            c1: PointMm::new(30.0, 60.0),
            c2: PointMm::new(70.0, 60.0),
            end: PointRef::New(pt(100.0, 10.0)),
        })
        .unwrap()
        .entities[0];
    let v = add_guide(&mut p, s, 30.0, 0.0, 90.0);
    let h = add_guide(&mut p, s, 0.0, 50.0, 0.0);
    let sheet = &p.sheets[0];
    let (gv, gh) = (sheet.guide(v).unwrap(), sheet.guide(h).unwrap());
    // Verticale x = 30 coupe le segment en (30 ; 0).
    let hits = sheet.guide_trace_hits(gv, seg);
    assert_eq!(hits.len(), 1);
    assert!((hits[0].x - 30.0).abs() < 1e-6 && hits[0].y.abs() < 1e-6);
    // L'intersection est en dehors du segment borné : aucune (le guide est
    // infini, le tracé non).
    let far = Guide {
        x_mm: 500.0,
        ..gv.clone()
    };
    assert!(sheet.guide_trace_hits(&far, seg).is_empty());
    // Horizontale y = 50 : deux points sur le cercle.
    let hits = sheet.guide_trace_hits(gh, circle);
    assert_eq!(hits.len(), 2);
    let mut xs: Vec<f64> = hits.iter().map(|h| h.x).collect();
    xs.sort_by(f64::total_cmp);
    assert!((xs[0] - 40.0).abs() < 1e-6 && (xs[1] - 60.0).abs() < 1e-6);
    // Courbe de Bézier : la verticale la coupe une fois.
    let hits = sheet.guide_trace_hits(gv, curve);
    assert_eq!(hits.len(), 1);
    assert!((hits[0].x - 30.0).abs() < 1e-5);
    // Guide superposé à un segment : pas de point, pas de panique.
    let on_seg = Guide {
        y_mm: 0.0,
        ..gh.clone()
    };
    assert!(sheet.guide_trace_hits(&on_seg, seg).is_empty());
}

#[test]
fn rogner_une_ligne_a_une_boite() {
    let g = Guide {
        id: GuideId(1),
        x_mm: 0.0,
        y_mm: 5.0,
        angle_deg: 0.0,
        axis: false,
        layer: None,
    };
    let bb = pf_geometry::Bbox {
        min: pt(-10.0, 0.0),
        max: pt(30.0, 10.0),
    };
    assert_eq!(g.clip_to(&bb), Some((pt(-10.0, 5.0), pt(30.0, 5.0))));
    let outside = Guide { y_mm: 50.0, ..g };
    assert_eq!(outside.clip_to(&bb), None);
}

// ----------------------------------------- visibilité et verrou (CA 5-6)

#[test]
fn visibilite_par_calque() {
    let mut p = Project::new("g");
    let s = p.sheets[0].id;
    let l = layer(&mut p, s, "Aides");
    let g = p
        .apply(Command::InLayer {
            sheet: s,
            layer: l,
            command: Box::new(Command::AddGuide {
                sheet: s,
                x_mm: 0.0,
                y_mm: 0.0,
                angle_deg: 0.0,
                axis: false,
                layer: None,
            }),
        })
        .unwrap()
        .guides[0];
    assert_eq!(p.sheets[0].guide(g).unwrap().layer, Some(l));
    assert!(p.sheets[0].guide_visible(p.sheets[0].guide(g).unwrap()));
    p.apply(Command::SetLayerVisible {
        sheet: s,
        layer: l,
        visible: false,
    })
    .unwrap();
    assert!(!p.sheets[0].guide_visible(p.sheets[0].guide(g).unwrap()));
    // Un guide sans calque est toujours visible.
    let free = add_guide(&mut p, s, 0.0, 9.0, 0.0);
    assert!(p.sheets[0].guide_visible(p.sheets[0].guide(free).unwrap()));
}

#[test]
fn verrou_de_calque_respecte() {
    let mut p = Project::new("g");
    let s = p.sheets[0].id;
    let l = layer(&mut p, s, "Gelé");
    p.apply(Command::CreateSheet { name: "B".into() }).unwrap();
    let g = p
        .apply(Command::AddGuide {
            sheet: s,
            x_mm: 1.0,
            y_mm: 2.0,
            angle_deg: 10.0,
            axis: false,
            layer: Some(l),
        })
        .unwrap()
        .guides[0];
    p.apply(Command::SetLayerLocked {
        sheet: s,
        layer: l,
        locked: true,
    })
    .unwrap();
    for command in [
        Command::MoveGuide {
            sheet: s,
            guide: g,
            x_mm: 9.0,
            y_mm: 9.0,
            angle_deg: 10.0,
        },
        Command::SetGuideAxis {
            sheet: s,
            guide: g,
            axis: true,
        },
        Command::DeleteGuide { sheet: s, guide: g },
        Command::AddGuide {
            sheet: s,
            x_mm: 0.0,
            y_mm: 0.0,
            angle_deg: 0.0,
            axis: false,
            layer: Some(l),
        },
        Command::DeleteSheet { sheet: s },
    ] {
        let before = p.clone();
        let err = p.apply(command).expect_err("calque verrouillé");
        assert!(matches!(err, CoreError::LayerLocked(_)), "{err}");
        assert_eq!(p, before);
    }
    // Déverrouillé : la même commande passe.
    p.apply(Command::SetLayerLocked {
        sheet: s,
        layer: l,
        locked: false,
    })
    .unwrap();
    p.apply(Command::DeleteGuide { sheet: s, guide: g })
        .unwrap();
}

#[test]
fn supprimer_un_calque_exige_de_deplacer_ses_guides() {
    let mut p = Project::new("g");
    let s = p.sheets[0].id;
    let (a, b) = (layer(&mut p, s, "A"), layer(&mut p, s, "B"));
    let g = p
        .apply(Command::AddGuide {
            sheet: s,
            x_mm: 0.0,
            y_mm: 0.0,
            angle_deg: 0.0,
            axis: false,
            layer: Some(a),
        })
        .unwrap()
        .guides[0];
    let err = refused(
        &mut p,
        Command::DeleteLayer {
            sheet: s,
            layer: a,
            move_to: None,
        },
    );
    assert!(err.to_string().contains("contient"), "{err}");
    p.apply(Command::DeleteLayer {
        sheet: s,
        layer: a,
        move_to: Some(b),
    })
    .unwrap();
    assert_eq!(p.sheets[0].guide(g).unwrap().layer, Some(b));
}

// --------------------------------------------------- copies (CA 7)

fn project_with_guides() -> (Project, SheetId, Vec<GuideId>, LayerId) {
    let mut p = Project::new("copie");
    let s = p.sheets[0].id;
    p.apply(Command::AddSegment {
        sheet: s,
        from: PointRef::New(pt(0.0, 0.0)),
        to: PointRef::New(pt(50.0, 0.0)),
    })
    .unwrap();
    let l = layer(&mut p, s, "Aides");
    let g1 = p
        .apply(Command::AddGuide {
            sheet: s,
            x_mm: 0.0,
            y_mm: 30.0,
            angle_deg: 0.0,
            axis: false,
            layer: Some(l),
        })
        .unwrap()
        .guides[0];
    let g2 = p
        .apply(Command::AddGuide {
            sheet: s,
            x_mm: 25.0,
            y_mm: 0.0,
            angle_deg: 90.0,
            axis: true,
            layer: None,
        })
        .unwrap()
        .guides[0];
    (p, s, vec![g1, g2], l)
}

#[test]
fn copie_de_feuille_reecrit_les_identifiants_des_guides() {
    let (mut p, s, guides, l) = project_with_guides();
    let out = p.apply(Command::DuplicateSheet { sheet: s }).unwrap();
    let n = out.sheet.unwrap();
    assert_eq!(out.guides.len(), 2);
    let (src, copy) = (p.sheet(s).unwrap(), p.sheet(n).unwrap());
    assert_eq!(copy.guides.len(), 2);
    for (a, b) in src.guides.iter().zip(&copy.guides) {
        assert_ne!(a.id, b.id, "identifiants régénérés");
        assert!(!guides.contains(&b.id));
        assert_eq!(
            (a.x_mm, a.y_mm, a.angle_deg, a.axis),
            (b.x_mm, b.y_mm, b.angle_deg, b.axis)
        );
    }
    // Le calque référencé est celui de la copie, pas celui de la source.
    let new_layer = copy.guides[0].layer.unwrap();
    assert_ne!(new_layer, l);
    assert!(copy.layer(new_layer).is_ok());
    assert_eq!(copy.guides[1].layer, None);
    p.validate().unwrap();
    // Import depuis un autre projet : même réécriture.
    let command = p.import_command(s).unwrap();
    let mut other = Project::new("autre");
    let out = other.apply(command).unwrap();
    assert_eq!(out.guides.len(), 2);
    other.validate().unwrap();
}

#[test]
fn presse_papiers_copie_colle_les_guides_avec_nouveaux_identifiants() {
    let (mut p, s, guides, _) = project_with_guides();
    let fragment = p
        .copy_fragment_with_guides(s, &[], &[], &[], &[], &guides)
        .unwrap();
    assert_eq!(fragment.guides.len(), 2);
    assert!(fragment.guides.iter().all(|g| g.layer.is_none()));
    // Aller-retour JSON.
    let again = Fragment::from_json(&fragment.to_json()).unwrap();
    assert_eq!(again, fragment);

    let l2 = layer(&mut p, s, "Cible");
    let out = p
        .apply(Command::PasteFragment {
            sheet: s,
            fragment: Box::new(fragment.clone()),
            layer: Some(l2),
        })
        .unwrap();
    assert_eq!(out.guides.len(), 2);
    let sheet = p.sheet(s).unwrap();
    assert_eq!(sheet.guides.len(), 4);
    for id in &out.guides {
        assert!(!guides.contains(id));
        assert_eq!(sheet.guide(*id).unwrap().layer, Some(l2));
    }
    p.validate().unwrap();

    // Collage dans une autre feuille d'un autre projet.
    let mut other = Project::new("autre");
    let target = other.sheets[0].id;
    let out = other
        .apply(Command::PasteFragment {
            sheet: target,
            fragment: Box::new(fragment),
            layer: None,
        })
        .unwrap();
    assert_eq!(out.guides.len(), 2);
    assert_eq!(other.sheets[0].guides.len(), 2);

    // Copie sans guide : fragment inchangé (compatibilité), et un fragment
    // sans aucun objet reste refusé.
    let plain = p.copy_fragment(
        s,
        &[],
        &p.sheets[0]
            .entities
            .iter()
            .map(|e| e.id)
            .collect::<Vec<_>>(),
        &[],
        &[],
    );
    assert!(plain.unwrap().guides.is_empty());
    assert!(p.copy_fragment(s, &[], &[], &[], &[]).is_err());
    // Un guide inconnu est refusé.
    assert!(p
        .copy_fragment_with_guides(s, &[], &[], &[], &[], &[GuideId(777)])
        .is_err());
}

#[test]
fn fragment_avec_guide_invalide_refuse() {
    let (p, s, guides, _) = project_with_guides();
    let mut f = p
        .copy_fragment_with_guides(s, &[], &[], &[], &[], &guides)
        .unwrap();
    f.guides[0].angle_deg = f64::NAN;
    assert!(f.validate().is_err());
    let mut f = p
        .copy_fragment_with_guides(s, &[], &[], &[], &[], &guides)
        .unwrap();
    f.guides[1].id = f.guides[0].id;
    assert!(f.validate().is_err(), "identifiant en double");
}

// ------------------------------------------- axe de symétrie (CA 9)

#[test]
fn symetrie_par_l_axe_d_un_guide() {
    let mut p = Project::new("miroir");
    let s = p.sheets[0].id;
    let seg = p
        .apply(Command::AddSegment {
            sheet: s,
            from: PointRef::New(pt(10.0, 0.0)),
            to: PointRef::New(pt(30.0, 10.0)),
        })
        .unwrap();
    // Axe vertical x = 50 (guide marqué « axe »).
    let g = p
        .apply(Command::AddGuide {
            sheet: s,
            x_mm: 50.0,
            y_mm: 0.0,
            angle_deg: 90.0,
            axis: true,
            layer: None,
        })
        .unwrap()
        .guides[0];
    let guide = p.sheets[0].guide(g).unwrap().clone();
    let mirror = Transform::Mirror {
        axis_point: guide.origin(),
        axis_angle_deg: guide.angle_deg,
    };
    p.apply(Command::Duplicate {
        sheet: s,
        entities: seg.entities.clone(),
        transform: mirror,
    })
    .unwrap();
    let sheet = &p.sheets[0];
    let copy = sheet.entities.last().unwrap().id;
    let (a, b) = sheet.segment_ends(copy).unwrap();
    assert_eq!((a, b), (pt(90.0, 0.0), pt(70.0, 10.0)));
    // Axe oblique à 45° par l'origine : (10 ; 0) devient (0 ; 10).
    let oblique = Transform::Mirror {
        axis_point: pt(0.0, 0.0),
        axis_angle_deg: 45.0,
    };
    let q = oblique.point(pt(10.0, 0.0)).unwrap();
    assert!(q.x.abs() < 1e-9 && (q.y - 10.0).abs() < 1e-9);
    // Les guides ne sont pas des tracés : la symétrie ne les touche pas.
    assert_eq!(p.sheets[0].guide(g).unwrap(), &guide);
}
