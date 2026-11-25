//! US-01.6 — presse-papiers : copier / coller de fragments.

use pf_core::clipboard::PASTE_OFFSET_MM;
use pf_core::*;
use pf_geometry::{PointMm, Side};
use std::collections::HashSet;

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

fn all_ids(p: &Project) -> HashSet<u64> {
    let mut ids = HashSet::new();
    for s in &p.sheets {
        ids.insert(s.id.0);
        ids.extend(s.points.iter().map(|x| x.id.0));
        ids.extend(s.entities.iter().map(|x| x.id.0));
        ids.extend(s.notches.iter().map(|x| x.id.0));
        ids.extend(s.dimensions.iter().map(|x| x.id.0));
        ids.extend(s.layers.iter().map(|x| x.id.0));
        ids.extend(s.groups.iter().map(|x| x.id.0));
    }
    ids
}

/// Segment AB (0,0)-(100,0), point P à 30 mm sur AB, BC perpendiculaire à AB
/// issue de P, un cran sur AB, une cotation A-B.
struct Scene {
    p: Project,
    sheet: SheetId,
    ab: EntityId,
    perp: EntityId,
    a: PointId,
    b: PointId,
    on: PointId,
}

fn scene() -> Scene {
    let mut p = Project::new("A");
    let sheet = p.sheets[0].id;
    let ab = p
        .apply(Command::AddSegment {
            sheet,
            from: PointRef::New(pt(0.0, 0.0)),
            to: PointRef::New(pt(100.0, 0.0)),
        })
        .unwrap();
    let (a, b) = (ab.points[0], ab.points[1]);
    let ab = ab.entities[0];
    let on = p
        .apply(Command::AddPointOnSegment {
            sheet,
            segment: ab,
            distance_mm: 30.0,
            origin: SegmentEnd::Start,
        })
        .unwrap()
        .points[0];
    let perp = p
        .apply(Command::AddPerpendicular {
            sheet,
            reference: ab,
            from: PointRef::Existing(on),
            length_mm: 20.0,
            side: Side::Left,
        })
        .unwrap()
        .entities[0];
    p.apply(Command::AddNotch {
        sheet,
        entity: ab,
        distance_mm: 50.0,
        side: Side::Left,
        length_mm: 5.0,
        style: NotchStyle::Single,
    })
    .unwrap();
    p.apply(Command::AddDimension {
        sheet,
        from: a,
        to: b,
        offset_mm: 10.0,
    })
    .unwrap();
    Scene {
        p,
        sheet,
        ab,
        perp,
        a,
        b,
        on,
    }
}

fn paste(p: &mut Project, sheet: SheetId, f: &Fragment, layer: Option<LayerId>) -> Outcome {
    p.apply(Command::PasteFragment {
        sheet,
        fragment: Box::new(f.clone()),
        layer,
    })
    .unwrap()
}

#[test]
fn ids_regeneres_validate_et_undo_exact() {
    let s = scene();
    let sheet = s.p.sheet(s.sheet).unwrap();
    let ents: Vec<EntityId> = sheet.entities.iter().map(|e| e.id).collect();
    let frag = s.p.copy_fragment(s.sheet, &[], &ents, &[], &[]).unwrap();
    let before = s.p.clone();
    let known = all_ids(&before);
    let mut doc = Document::new(s.p.clone());
    let out = doc
        .execute(Command::PasteFragment {
            sheet: s.sheet,
            fragment: Box::new(frag),
            layer: None,
        })
        .unwrap();
    doc.project().validate().unwrap();
    let new_ids: Vec<u64> = out
        .points
        .iter()
        .map(|x| x.0)
        .chain(out.entities.iter().map(|x| x.0))
        .chain(out.notches.iter().map(|x| x.0))
        .chain(out.dimensions.iter().map(|x| x.0))
        .collect();
    assert!(!new_ids.is_empty());
    assert!(new_ids.iter().all(|id| !known.contains(id)));
    assert_eq!(out.entities.len(), 2);
    assert_eq!(out.notches.len(), 1);
    assert_eq!(out.dimensions.len(), 1);
    // Une seule étape d'historique, restaurée à l'identique.
    assert!(doc.undo());
    assert_eq!(doc.project(), &before);
    assert!(!doc.can_undo());
}

#[test]
fn constructions_internes_conservees_et_decalage() {
    let mut s = scene();
    let frag =
        s.p.copy_fragment(s.sheet, &[s.on], &[s.ab, s.perp], &[], &[])
            .unwrap();
    assert!(frag.notes.is_empty(), "rien d'externe : aucune remarque");
    let out = paste(&mut s.p, s.sheet, &frag, None);
    let sheet = s.p.sheet(s.sheet).unwrap();
    // Le point construit suit le nouveau segment, pas l'ancien.
    let new_on = sheet
        .points
        .iter()
        .find(|p| {
            out.points.contains(&p.id) && matches!(p.definition, PointDefinition::OnSegment { .. })
        })
        .unwrap();
    let PointDefinition::OnSegment { segment, .. } = new_on.definition else {
        unreachable!()
    };
    assert!(out.entities.contains(&segment));
    // Collé sur les mêmes positions dans la même feuille : décalé.
    let pos = sheet.position(new_on.id).unwrap();
    let orig = sheet.position(s.on).unwrap();
    assert!((pos.x - orig.x - PASTE_OFFSET_MM).abs() < 1e-9);
    assert!((pos.y - orig.y - PASTE_OFFSET_MM).abs() < 1e-9);
    // Cran et cotation (A et B copiés avec AB) conservés ; perpendiculaire liée à la copie.
    assert_eq!(out.notches.len(), 1);
    assert_eq!(out.dimensions.len(), 1);
    let new_perp = sheet
        .entities
        .iter()
        .filter(|e| out.entities.contains(&e.id))
        .find_map(|e| match e.kind {
            EntityKind::Segment {
                construction: Some(Construction::Perpendicular { reference, .. }),
                ..
            } => Some(reference),
            _ => None,
        })
        .unwrap();
    assert!(out.entities.contains(&new_perp));
    // Deuxième collage : décalé encore, jamais empilé sur le premier.
    let out2 = paste(&mut s.p, s.sheet, &frag, None);
    let sheet = s.p.sheet(s.sheet).unwrap();
    let p2 = sheet
        .points
        .iter()
        .find(|p| out2.points.contains(&p.id) && p.definition.is_free())
        .unwrap();
    let q = sheet.position(p2.id).unwrap();
    let all: Vec<_> = sheet
        .points
        .iter()
        .filter(|p| p.id != p2.id)
        .map(|p| sheet.position(p.id).unwrap())
        .collect();
    assert!(all
        .iter()
        .all(|o| (o.x - q.x).abs() > 1e-6 || (o.y - q.y).abs() > 1e-6));
    s.p.validate().unwrap();
}

#[test]
fn groupe_conserve_seulement_si_entierement_copie() {
    let mut s = scene();
    s.p.apply(Command::CreateGroup {
        sheet: s.sheet,
        entities: vec![s.ab, s.perp],
    })
    .unwrap();
    let full =
        s.p.copy_fragment(s.sheet, &[], &[s.ab, s.perp], &[], &[])
            .unwrap();
    assert_eq!(full.groups.len(), 1);
    let out = paste(&mut s.p, s.sheet, &full, None);
    assert_eq!(out.groups.len(), 1);
    let partial = s.p.copy_fragment(s.sheet, &[], &[s.ab], &[], &[]).unwrap();
    assert!(partial.groups.is_empty());
    s.p.validate().unwrap();
}

#[test]
fn parent_externe_fige_avec_remarque() {
    let mut s = scene();
    // Le point P seul : son segment AB n'est pas copié.
    let frag = s.p.copy_fragment(s.sheet, &[s.on], &[], &[], &[]).unwrap();
    assert!(frag.points[0].definition.is_free());
    assert_eq!(frag.notes.len(), 1);
    let out = paste(&mut s.p, s.sheet, &frag, None);
    assert!(out.notes.iter().any(|n| n.contains("libre")));
    // Position figée sur celle de l'original, décalée (collage sur place).
    let sheet = s.p.sheet(s.sheet).unwrap();
    let np = sheet.position(out.points[0]).unwrap();
    assert!((np.x - 30.0 - PASTE_OFFSET_MM).abs() < 1e-9);
    // Perpendiculaire sans sa référence : provenance retirée, avec remarque.
    let frag =
        s.p.copy_fragment(s.sheet, &[], &[s.perp], &[], &[])
            .unwrap();
    assert!(frag.notes.iter().any(|n| n.contains("construction")));
    assert!(frag.points.iter().all(|p| p.definition.is_free()));
    let out = paste(&mut s.p, s.sheet, &frag, None);
    assert!(!out.notes.is_empty());
    s.p.validate().unwrap();
}

#[test]
fn calque_actif_et_calque_verrouille() {
    let mut s = scene();
    let frag = s.p.copy_fragment(s.sheet, &[], &[s.ab], &[], &[]).unwrap();
    let layer =
        s.p.apply(Command::AddLayer {
            sheet: s.sheet,
            name: "Coupe".into(),
        })
        .unwrap()
        .layers[0];
    let out = paste(&mut s.p, s.sheet, &frag, Some(layer));
    let sheet = s.p.sheet(s.sheet).unwrap();
    assert!(out
        .entities
        .iter()
        .all(|e| sheet.entity(*e).unwrap().layer == Some(layer)));
    assert!(out
        .points
        .iter()
        .all(|p| sheet.point(*p).unwrap().layer == Some(layer)));
    // Calque cible verrouillé : refus, rien ne change.
    s.p.apply(Command::SetLayerLocked {
        sheet: s.sheet,
        layer,
        locked: true,
    })
    .unwrap();
    let before = s.p.clone();
    let err =
        s.p.apply(Command::PasteFragment {
            sheet: s.sheet,
            fragment: Box::new(frag),
            layer: Some(layer),
        })
        .unwrap_err();
    assert!(matches!(err, CoreError::LayerLocked(_)), "{err}");
    assert_eq!(s.p, before);
}

#[test]
fn collage_entre_projets_par_json() {
    let s = scene();
    let frag =
        s.p.copy_fragment(s.sheet, &[], &[s.ab, s.perp], &[], &[])
            .unwrap();
    let json = frag.to_json();
    assert!(Fragment::looks_like(&json));
    let read = Fragment::from_json(&json).unwrap();
    assert_eq!(read, frag);
    let mut other = Project::new("B");
    other
        .apply(Command::CreateSheet {
            name: "Autre".into(),
        })
        .unwrap();
    let target = other.sheets[1].id;
    let out = paste(&mut other, target, &read, None);
    other.validate().unwrap();
    assert_eq!(out.entities.len(), 2);
    // Autre projet : collé à la position d'origine (rien ne coïncide).
    let sheet = other.sheet(target).unwrap();
    let pa = sheet.position(out.points[0]).unwrap();
    let orig = s.p.sheet(s.sheet).unwrap().position(s.a).unwrap();
    assert_eq!((pa.x, pa.y), (orig.x, orig.y));
    assert_eq!(other.sheets[0].points.len(), 0);
    let _ = s.b;
}

#[test]
fn fragment_corrompu_refuse_sans_rien_modifier() {
    let s = scene();
    let good =
        s.p.copy_fragment(s.sheet, &[], &[s.ab, s.perp], &[], &[])
            .unwrap();
    let mut cases: Vec<(&str, Fragment)> = Vec::new();
    let mut f = good.clone();
    f.version = 99;
    cases.push(("version", f));
    let mut f = good.clone();
    f.format = "autre".into();
    cases.push(("format", f));
    let mut f = good.clone();
    f.points.pop();
    cases.push(("point manquant", f));
    let mut f = good.clone();
    f.entities[1].id = f.entities[0].id;
    cases.push(("identifiant en double", f));
    let mut f = good.clone();
    f.notches[0].entity = EntityId(987_654);
    cases.push(("cran orphelin", f));
    let mut f = good.clone();
    f.dimensions[0].to = PointId(987_654);
    cases.push(("cotation orpheline", f));
    let mut f = good.clone();
    f.points[0].layer = Some(LayerId(3));
    cases.push(("calque embarqué", f));
    let mut f = good.clone();
    f.points.clear();
    f.entities.clear();
    cases.push(("vide", f));
    for (name, f) in cases {
        let mut p = s.p.clone();
        let before = p.clone();
        let r = p.apply(Command::PasteFragment {
            sheet: s.sheet,
            fragment: Box::new(f.clone()),
            layer: None,
        });
        assert!(r.is_err(), "{name} doit être refusé");
        assert_eq!(p, before, "{name} : rien ne doit changer");
        assert!(Fragment::from_json(&f.to_json()).is_err(), "{name}");
    }
    // Champ inconnu et JSON tronqué : refusés, pas ignorés.
    let json = good.to_json();
    assert!(Fragment::from_json(&json.replacen('{', "{\"extra\":1,", 1)).is_err());
    assert!(Fragment::from_json(&json[..json.len() / 2]).is_err());
    assert!(!Fragment::looks_like("du texte quelconque"));
    assert!(!Fragment::looks_like("{\"autre\":1}"));
}

#[test]
fn copie_vide_refusee() {
    let s = scene();
    assert!(s.p.copy_fragment(s.sheet, &[], &[], &[], &[]).is_err());
}
