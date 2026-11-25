//! US-01.3 — noms d'objets et groupes : commandes, validation, fichier, annulation.

use pf_core::persist;
use pf_core::*;
use pf_geometry::{PointMm, Transform};

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

fn setup() -> (Project, SheetId) {
    let p = Project::new("groupes");
    let s = p.sheets[0].id;
    (p, s)
}

/// Segment horizontal de 60 mm à la hauteur `y`.
fn segment(p: &mut Project, sheet: SheetId, y: f64) -> EntityId {
    p.apply(Command::AddSegment {
        sheet,
        from: PointRef::New(pt(0.0, y)),
        to: PointRef::New(pt(60.0, y)),
    })
    .unwrap()
    .entities[0]
}

fn name(
    p: &mut Project,
    sheet: SheetId,
    entity: EntityId,
    n: Option<&str>,
) -> Result<(), CoreError> {
    p.apply(Command::SetEntityName {
        sheet,
        entity,
        name: n.map(str::to_string),
    })
    .map(|_| ())
}

fn group(p: &mut Project, sheet: SheetId, entities: &[EntityId]) -> Result<GroupId, CoreError> {
    p.apply(Command::CreateGroup {
        sheet,
        entities: entities.to_vec(),
    })
    .map(|o| o.groups[0])
}

// ---------------------------------------------------------------- CA 1

#[test]
fn ca1_nom_facultatif_unique_par_feuille() {
    let (mut p, s) = setup();
    let (a, b) = (segment(&mut p, s, 0.0), segment(&mut p, s, 10.0));
    assert!(p.sheets[0].entities.iter().all(|e| e.name.is_none()));

    name(&mut p, s, a, Some("  Ourlet ")).unwrap();
    assert_eq!(
        p.sheets[0].entity(a).unwrap().name.as_deref(),
        Some("Ourlet")
    );
    // Même nom sur un autre tracé : refusé, rien ne change.
    let before = p.clone();
    let err = name(&mut p, s, b, Some("Ourlet")).unwrap_err();
    assert!(matches!(err, CoreError::Invalid(_)), "{err}");
    assert!(err.to_string().contains("déjà utilisé"), "{err}");
    assert_eq!(p, before);
    // Renommer un tracé avec son propre nom reste permis.
    name(&mut p, s, a, Some("Ourlet")).unwrap();
    // Nom vide ou None : retire le nom.
    name(&mut p, s, a, Some("   ")).unwrap();
    assert_eq!(p.sheets[0].entity(a).unwrap().name, None);
    name(&mut p, s, a, Some("Ourlet")).unwrap();
    name(&mut p, s, a, None).unwrap();
    assert_eq!(p.sheets[0].entity(a).unwrap().name, None);
    // Tracé inconnu et nom à caractère de contrôle : refusés.
    assert!(name(&mut p, s, EntityId(9999), Some("x")).is_err());
    assert!(name(&mut p, s, a, Some("a\nb")).is_err());

    // L'unicité vaut par feuille : le même nom sert sur une autre feuille.
    let s2 = p
        .apply(Command::CreateSheet {
            name: "Feuille 2".into(),
        })
        .unwrap()
        .sheet
        .unwrap();
    let c = segment(&mut p, s2, 0.0);
    name(&mut p, s, a, Some("Ourlet")).unwrap();
    name(&mut p, s2, c, Some("Ourlet")).unwrap();
}

#[test]
fn ca1_fichier_sans_nom_ni_groupe_inchange_et_champs_valides() {
    let (mut p, s) = setup();
    let (a, b) = (segment(&mut p, s, 0.0), segment(&mut p, s, 10.0));
    let json = persist::to_json(&p).unwrap();
    assert!(
        !json.contains("\"name\": null") && !json.contains("groups"),
        "{json}"
    );
    assert_eq!(p.schema_version, 6, "schéma 4 (US-09.1)");
    assert_eq!(
        json.matches("\"name\"").count(),
        2,
        "projet + feuille seulement"
    );
    // Relecture puis réécriture : octet pour octet.
    assert_eq!(
        persist::to_json(&persist::from_json(&json).unwrap()).unwrap(),
        json
    );

    // Avec noms et groupes : aller-retour sans perte.
    name(&mut p, s, a, Some("Ourlet")).unwrap();
    group(&mut p, s, &[a, b]).unwrap();
    let json = persist::to_json(&p).unwrap();
    assert!(json.contains("Ourlet") && json.contains("groups"));
    assert_eq!(persist::from_json(&json).unwrap(), p);

    // Un fichier incohérent est refusé à l'ouverture.
    let mut bad = p.clone();
    bad.sheets[0].entities[1].name = Some("Ourlet2".into());
    let text = persist::to_json(&bad).unwrap().replace("Ourlet2", "Ourlet");
    let err = persist::from_json(&text).unwrap_err();
    assert!(err.to_string().contains("deux fois"), "{err}");
    let mut bad = p.clone();
    bad.sheets[0].groups[0].members.push(EntityId(9999));
    assert!(matches!(bad.validate(), Err(CoreError::DeadReference(_))));
    let mut bad = p.clone();
    bad.sheets[0].groups.push(Group {
        id: GroupId(bad.next_id),
        members: vec![a],
    });
    bad.next_id += 1;
    assert!(bad.validate().is_err(), "un tracé dans deux groupes");
}

// ---------------------------------------------------------------- CA 2 (modèle)

#[test]
fn ca2_groupe_validation_creation_et_dissolution() {
    let (mut p, s) = setup();
    let (a, b, c) = (
        segment(&mut p, s, 0.0),
        segment(&mut p, s, 10.0),
        segment(&mut p, s, 20.0),
    );
    // Moins de deux tracés, tracé inconnu : refusés.
    assert!(group(&mut p, s, &[a]).is_err());
    assert!(group(&mut p, s, &[a, a]).is_err());
    assert!(group(&mut p, s, &[a, EntityId(9999)]).is_err());
    assert!(p.sheets[0].groups.is_empty());

    let g = group(&mut p, s, &[a, b]).unwrap();
    assert_eq!(p.sheets[0].group_of(a).unwrap().id, g);
    assert!(p.sheets[0].group_of(c).is_none());
    assert_eq!(p.sheets[0].expand_groups(&[a]), vec![a, b]);
    assert_eq!(p.sheets[0].expand_groups(&[c]), vec![c]);

    // Un tracé dans au plus un groupe.
    let before = p.clone();
    let err = group(&mut p, s, &[b, c]).unwrap_err();
    assert!(err.to_string().contains("déjà partie d'un groupe"), "{err}");
    assert_eq!(p, before);

    p.apply(Command::Ungroup { sheet: s, group: g }).unwrap();
    assert!(p.sheets[0].groups.is_empty());
    assert!(p.apply(Command::Ungroup { sheet: s, group: g }).is_err());
    group(&mut p, s, &[b, c]).unwrap();
}

#[test]
fn ca2_groupe_sur_objet_verrouille_refuse() {
    let (mut p, s) = setup();
    let layer = p
        .apply(Command::AddLayer {
            sheet: s,
            name: "Fixe".into(),
        })
        .unwrap()
        .layers[0];
    let a = segment(&mut p, s, 0.0);
    let b = segment(&mut p, s, 10.0);
    p.apply(Command::SetObjectsLayer {
        sheet: s,
        points: vec![],
        entities: vec![a],
        layer: Some(layer),
    })
    .unwrap();
    // Calque déverrouillé : on groupe, puis on verrouille.
    let g = group(&mut p, s, &[a, b]).unwrap();
    p.apply(Command::SetLayerLocked {
        sheet: s,
        layer,
        locked: true,
    })
    .unwrap();
    for command in [
        Command::Ungroup { sheet: s, group: g },
        Command::SetEntityName {
            sheet: s,
            entity: a,
            name: Some("X".into()),
        },
    ] {
        let before = p.clone();
        let err = p.apply(command).unwrap_err();
        assert!(matches!(err, CoreError::LayerLocked(_)), "{err}");
        assert_eq!(p, before);
    }
    // Sur un tracé verrouillé non encore groupé : refusé aussi.
    let (mut p, s) = setup();
    let layer = p
        .apply(Command::AddLayer {
            sheet: s,
            name: "Fixe".into(),
        })
        .unwrap()
        .layers[0];
    let (a, b) = (segment(&mut p, s, 0.0), segment(&mut p, s, 10.0));
    p.apply(Command::SetObjectsLayer {
        sheet: s,
        points: vec![],
        entities: vec![a],
        layer: Some(layer),
    })
    .unwrap();
    p.apply(Command::SetLayerLocked {
        sheet: s,
        layer,
        locked: true,
    })
    .unwrap();
    let before = p.clone();
    let err = group(&mut p, s, &[a, b]).unwrap_err();
    assert!(matches!(err, CoreError::LayerLocked(_)), "{err}");
    assert!(err.to_string().contains("verrouillé"), "{err}");
    assert_eq!(p, before);
}

#[test]
fn ca2_suppression_d_un_membre_le_retire_et_groupe_vide_supprime() {
    let (mut p, s) = setup();
    let (a, b, c) = (
        segment(&mut p, s, 0.0),
        segment(&mut p, s, 10.0),
        segment(&mut p, s, 20.0),
    );
    group(&mut p, s, &[a, b, c]).unwrap();
    p.apply(Command::Delete {
        sheet: s,
        points: vec![],
        entities: vec![a],
    })
    .unwrap();
    assert_eq!(p.sheets[0].groups[0].members, vec![b, c]);
    p.apply(Command::Delete {
        sheet: s,
        points: vec![],
        entities: vec![b, c],
    })
    .unwrap();
    assert!(p.sheets[0].groups.is_empty(), "groupe vide supprimé");
    p.validate().unwrap();
}

// ---------------------------------------------------------------- CA 3 (modèle)

#[test]
fn ca3_transformation_du_groupe_en_une_etape_annulable() {
    let (mut p, s) = setup();
    let (a, b) = (segment(&mut p, s, 0.0), segment(&mut p, s, 10.0));
    let c = segment(&mut p, s, 20.0);
    group(&mut p, s, &[a, b]).unwrap();
    let mut doc = Document::new(p);
    let before = doc.project().clone();

    // Ce que fait l'éditeur : la sélection d'un membre est étendue au groupe,
    // puis une seule commande transforme tous les points des tracés.
    let members = doc.project().sheets[0].expand_groups(&[a]);
    assert_eq!(members, vec![a, b]);
    let sheet = &doc.project().sheets[0];
    let mut points = Vec::new();
    for &e in &members {
        let (p0, p1) = sheet.entity_points(e).unwrap();
        points.extend([p0, p1]);
    }
    doc.execute(Command::Transform {
        sheet: s,
        points,
        transform: Transform::Translate {
            dx_mm: 5.0,
            dy_mm: 7.0,
        },
    })
    .unwrap();
    let sheet = &doc.project().sheets[0];
    assert_eq!(sheet.segment_ends(a).unwrap().0, pt(5.0, 7.0));
    assert_eq!(sheet.segment_ends(b).unwrap().0, pt(5.0, 17.0));
    assert_eq!(
        sheet.segment_ends(c).unwrap().0,
        pt(0.0, 20.0),
        "hors groupe : immobile"
    );

    assert!(doc.undo());
    assert_eq!(
        *doc.project(),
        before,
        "une seule annulation rétablit tout le groupe"
    );
}
