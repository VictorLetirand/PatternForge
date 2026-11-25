//! US-01.4 — duplication et import de feuille.

use pf_core::persist;
use pf_core::*;
use pf_geometry::{PointMm, Side};
use std::collections::HashSet;
use std::path::PathBuf;

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

/// Tous les identifiants d'une feuille et de ses pièces liées.
fn ids_of(p: &Project, sheet: SheetId) -> HashSet<u64> {
    let s = p.sheet(sheet).unwrap();
    let mut ids = HashSet::new();
    ids.insert(s.id.0);
    ids.extend(s.points.iter().map(|x| x.id.0));
    ids.extend(s.entities.iter().map(|x| x.id.0));
    ids.extend(s.notches.iter().map(|x| x.id.0));
    ids.extend(s.dimensions.iter().map(|x| x.id.0));
    ids.extend(s.layers.iter().map(|x| x.id.0));
    ids.extend(s.groups.iter().map(|x| x.id.0));
    ids.extend(p.pieces_of(sheet).map(|x| x.id.0));
    ids
}

/// Feuille « Rabat » (rectangle, crans, cotations, droit-fil, pièce) enrichie :
/// points construits, perpendiculaire, courbe, groupe, deux calques dont un verrouillé.
fn rich_project() -> (Project, SheetId) {
    let mut p = demo_piece_project().unwrap();
    let s = p.sheets[0].id;
    let (a, b) = {
        let sh = p.sheet(s).unwrap();
        (sh.points[0].id, sh.points[1].id)
    };
    let base = p.sheet(s).unwrap().entities[0].id;
    let mid = p
        .apply(Command::AddPointOnSegment {
            sheet: s,
            segment: base,
            distance_mm: 30.0,
            origin: SegmentEnd::Start,
        })
        .unwrap();
    let perp = p
        .apply(Command::AddPerpendicular {
            sheet: s,
            reference: base,
            from: PointRef::Existing(mid.points[0]),
            length_mm: 25.0,
            side: Side::Left,
        })
        .unwrap();
    let curve = p
        .apply(Command::AddCurve {
            sheet: s,
            start: PointRef::Existing(a),
            c1: pt(10.0, -40.0),
            c2: pt(150.0, -40.0),
            end: PointRef::Existing(b),
        })
        .unwrap();
    p.apply(Command::AddPointAt {
        sheet: s,
        point: PointRef::Derived(PointDefinition::OnCurve {
            curve: curve.entities[0],
            t: 0.5,
        }),
    })
    .unwrap();
    p.apply(Command::CreateGroup {
        sheet: s,
        entities: vec![perp.entities[0], curve.entities[0]],
    })
    .unwrap();
    let l1 = p
        .apply(Command::AddLayer {
            sheet: s,
            name: "Base".into(),
        })
        .unwrap()
        .layers[0];
    let l2 = p
        .apply(Command::AddLayer {
            sheet: s,
            name: "Gelé".into(),
        })
        .unwrap()
        .layers[0];
    p.apply(Command::SetObjectsLayer {
        sheet: s,
        points: vec![],
        entities: vec![perp.entities[0], curve.entities[0]],
        layer: Some(l1),
    })
    .unwrap();
    p.apply(Command::SetObjectsLayer {
        sheet: s,
        points: vec![],
        entities: vec![base],
        layer: Some(l2),
    })
    .unwrap();
    p.apply(Command::SetLayerLocked {
        sheet: s,
        layer: l2,
        locked: true,
    })
    .unwrap();
    p.validate().unwrap();
    (p, s)
}

// ---------------------------------------------------------------- CA 1

#[test]
fn ca1_duplication_sans_collision_et_liens_conserves() {
    let (mut p, s) = rich_project();
    let before = p.clone();
    let old_ids = ids_of(&p, s);
    let out = p.apply(Command::DuplicateSheet { sheet: s }).unwrap();
    let n = out.sheet.unwrap();
    assert_eq!(p.schema_version, 6);
    // Placée juste après l'originale, renommée.
    assert_eq!(p.sheets[1].id, n);
    assert_eq!(p.sheets[1].name, "Rabat (copie)");
    // Aucun identifiant partagé ; la source est inchangée.
    let new_ids = ids_of(&p, n);
    assert!(new_ids.is_disjoint(&old_ids));
    assert_eq!(new_ids.len(), old_ids.len());
    assert_eq!(ids_of(&p, s), old_ids);
    assert_eq!(p.sheet(s).unwrap(), before.sheet(s).unwrap());
    p.validate().unwrap();

    let (o, c) = (p.sheet(s).unwrap(), p.sheet(n).unwrap());
    // Même géométrie, point par point.
    assert_eq!(o.points.len(), c.points.len());
    for (po, pc) in o.points.iter().zip(&c.points) {
        assert_eq!(po.label, pc.label);
        assert_eq!(o.position(po.id).unwrap(), c.position(pc.id).unwrap());
        // Les parents des points construits sont dans la copie.
        for d in pc.definition.dependencies() {
            assert!(c.entity(d).is_ok());
        }
    }
    // Calques : état de verrouillage et nom conservés, références recréées.
    assert_eq!(o.layers.len(), c.layers.len());
    for (lo, lc) in o.layers.iter().zip(&c.layers) {
        assert_eq!(
            (&lo.name, lo.locked, lo.visible),
            (&lc.name, lc.locked, lc.visible)
        );
    }
    assert!(c
        .entities
        .iter()
        .all(|e| e.layer.is_none_or(|l| c.layer(l).is_ok())));
    assert!(
        c.layer_locked(c.entities[0].layer),
        "le calque verrouillé le reste"
    );
    // Groupes, crans, cotations pointent dans la copie.
    assert_eq!(c.groups.len(), 1);
    assert!(c.groups[0].members.iter().all(|m| c.entity(*m).is_ok()));
    assert_eq!(c.notches.len(), o.notches.len());
    assert!(c.notches.iter().all(|k| c.entity(k.entity).is_ok()));
    assert_eq!(c.dimensions.len(), 2);
    assert!(c
        .dimensions
        .iter()
        .all(|d| c.point(d.from).is_ok() && c.point(d.to).is_ok()));
    // Construction perpendiculaire : la référence suit la copie.
    let perp_ref = c.entities.iter().find_map(|e| match e.kind {
        EntityKind::Segment {
            construction: Some(Construction::Perpendicular { reference, .. }),
            ..
        } => Some(reference),
        _ => None,
    });
    assert!(c.entity(perp_ref.unwrap()).is_ok());
    // La pièce liée est dupliquée avec un nouvel ID, contour et droit-fil dans la copie.
    let pieces: Vec<_> = p.pieces_of(n).collect();
    assert_eq!(pieces.len(), 1);
    assert_eq!(p.pieces_of(s).count(), 1);
    assert_eq!(pieces[0].outline.len(), 4);
    assert!(pieces[0].outline.iter().all(|e| c.entity(e.entity).is_ok()));
    assert!(c.entity(pieces[0].grainline.unwrap()).is_ok());
    // Les crans de la copie sont mesurables (validate l'a vérifié) et une
    // seconde duplication donne un nom encore unique.
    let out2 = p.apply(Command::DuplicateSheet { sheet: s }).unwrap();
    assert_eq!(
        p.sheet(out2.sheet.unwrap()).unwrap().name,
        "Rabat (copie) (2)"
    );
    // Annuler = un seul état précédent (au niveau Document).
    let mut doc = Document::new(before.clone());
    doc.execute(Command::DuplicateSheet { sheet: s }).unwrap();
    assert!(doc.undo());
    assert_eq!(*doc.project(), before);
    assert!(!doc.can_undo(), "une seule étape d'historique");
}

#[test]
fn propriete_copie_disjointe_valide_et_annulable() {
    let (rich, _) = rich_project();
    let projects = [demo_project().unwrap(), demo_piece_project().unwrap(), rich];
    for project in projects {
        for sheet in project.sheets.iter().map(|s| s.id).collect::<Vec<_>>() {
            let mut doc = Document::new(project.clone());
            let old_ids = ids_of(doc.project(), sheet);
            let out = doc.execute(Command::DuplicateSheet { sheet }).unwrap();
            let n = out.sheet.unwrap();
            let new_ids = ids_of(doc.project(), n);
            assert!(new_ids.is_disjoint(&old_ids));
            assert_eq!(new_ids.len(), old_ids.len());
            assert!(new_ids.iter().all(|i| *i < doc.project().next_id));
            doc.project().validate().unwrap();
            assert_eq!(doc.project().sheets.len(), project.sheets.len() + 1);
            assert!(doc.undo());
            assert_eq!(*doc.project(), project, "undo = état précédent");
            assert_eq!(
                persist::to_json(doc.project()).unwrap(),
                persist::to_json(&project).unwrap(),
                "bit à bit"
            );
        }
    }
}

#[test]
fn duplication_feuille_inconnue_refusee_sans_trace() {
    let (mut p, _) = rich_project();
    let before = p.clone();
    assert!(p
        .apply(Command::DuplicateSheet {
            sheet: SheetId(9999)
        })
        .is_err());
    assert_eq!(p, before);
}

// ---------------------------------------------------------------- CA 2

fn fixture(name: &str) -> Project {
    persist::load(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name),
    )
    .unwrap()
}

#[test]
fn ca2_import_depuis_fixtures_v1_et_v2() {
    for name in [
        "v1-exercice.pforge.json",
        "v1-grande-piece.pforge.json",
        "v2-piece-contour-droit-fil.pforge.json",
        "v2-piece-sans-contour.pforge.json",
    ] {
        // La migration est celle de l'ouverture : la source est déjà en v3.
        let source = fixture(name);
        assert_eq!(source.schema_version, 6, "{name}");
        for sheet in source.sheets.iter().map(|s| s.id) {
            let mut target = Project::new("cible");
            // Un nom identique existe déjà : l'import doit le rendre unique.
            let same = source.sheet(sheet).unwrap().name.clone();
            target
                .apply(Command::RenameSheet {
                    sheet: target.sheets[0].id,
                    name: same.clone(),
                })
                .unwrap();
            let cmd = source.import_command(sheet).unwrap();
            let src_before = source.clone();
            let out = target.apply(cmd).unwrap();
            let n = out.sheet.unwrap();
            assert_eq!(source, src_before);
            assert_eq!(target.schema_version, 6);
            target.validate().unwrap();
            assert_eq!(target.sheets.len(), 2);
            assert_eq!(target.sheets[1].id, n);
            assert_eq!(target.sheets[1].name, format!("{same} (2)"), "{name}");
            // Identifiants régénérés dans la plage du projet cible.
            let ids = ids_of(&target, n);
            assert!(ids.iter().all(|i| *i >= 2 && *i < target.next_id));
            assert_eq!(ids.len(), ids_of(&source, sheet).len(), "{name}");
            // Pièces liées migrées : copiées avec contour valide.
            assert_eq!(target.pieces_of(n).count(), source.pieces_of(sheet).count());
        }
    }
    let src = fixture("v2-piece-contour-droit-fil.pforge.json");
    let mut target = Project::new("cible");
    let sheet = src.sheets[0].id;
    target.apply(src.import_command(sheet).unwrap()).unwrap();
    let piece = &target.pieces[0];
    assert!(!piece.outline.is_empty());
    assert!(piece.grainline.is_some());
}

#[test]
fn import_pieces_et_calques_verrouilles_conserves() {
    let (src, s) = rich_project();
    let mut target = Project::new("cible");
    let out = target.apply(src.import_command(s).unwrap()).unwrap();
    let n = out.sheet.unwrap();
    let c = target.sheet(n).unwrap();
    assert_eq!(target.sheets[1].name, "Rabat");
    assert!(c.layers.iter().any(|l| l.locked));
    assert_eq!(target.pieces_of(n).count(), 1);
    assert_eq!(out.pieces.len(), 1);
}

#[test]
fn import_incoherent_refuse_sans_trace() {
    let (src, s) = rich_project();
    let Command::ImportSheet { mut sheet, pieces } = src.import_command(s).unwrap() else {
        unreachable!()
    };
    sheet.notches[0].entity = EntityId(424242);
    let mut target = Project::new("cible");
    let before = target.clone();
    assert!(target
        .apply(Command::ImportSheet { sheet, pieces })
        .is_err());
    assert_eq!(target, before);
}
