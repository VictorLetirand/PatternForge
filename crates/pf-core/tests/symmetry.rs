//! US-02.11 — symétrie liée : points miroir maintenus (réflexion exacte), tracés
//! miroir de sens inversé, refus du côté miroir, dépliage annulable, suppression
//! des sources/axes, verrou de calque, fichiers, copies de feuille et presse-papiers.

use pf_core::*;
use pf_geometry::{PointMm, Side, Transform};

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

/// Axe vertical x = 0.
fn reflect(p: PointMm) -> PointMm {
    Transform::Mirror {
        axis_point: pt(0.0, 0.0),
        axis_angle_deg: 90.0,
    }
    .point(p)
    .unwrap()
}

fn close(a: PointMm, b: PointMm, tol: f64) -> bool {
    (a.x - b.x).abs() <= tol && (a.y - b.y).abs() <= tol
}

struct Half {
    p: Project,
    sheet: SheetId,
    guide: GuideId,
    /// A (sur l'axe), B, C, D (sur l'axe)
    pts: [PointId; 4],
    ents: Vec<EntityId>,
}

/// Demi-contour droit : A(0,0) B(40,0) C(40,60) D(0,60), segments AB et CD,
/// courbe BC bombée ; A et D sont sur l'axe x = 0 (guide marqué axe).
fn half() -> Half {
    let mut p = Project::new("Symétrie");
    let sheet = p.sheets[0].id;
    let guide = p
        .apply(Command::AddGuide {
            sheet,
            x_mm: 0.0,
            y_mm: 0.0,
            angle_deg: 90.0,
            axis: true,
            layer: None,
        })
        .unwrap()
        .guides[0];
    let o = p
        .apply(Command::AddSegment {
            sheet,
            from: PointRef::New(pt(0.0, 0.0)),
            to: PointRef::New(pt(40.0, 0.0)),
        })
        .unwrap();
    let (a, b, ab) = (o.points[0], o.points[1], o.entities[0]);
    let o = p
        .apply(Command::AddCurve {
            sheet,
            start: PointRef::Existing(b),
            c1: pt(55.0, 10.0),
            c2: pt(52.0, 50.0),
            end: PointRef::New(pt(40.0, 60.0)),
        })
        .unwrap();
    let (c, bc) = (o.points[0], o.entities[0]);
    let o = p
        .apply(Command::AddSegment {
            sheet,
            from: PointRef::Existing(c),
            to: PointRef::New(pt(0.0, 60.0)),
        })
        .unwrap();
    let (d, cd) = (o.points[0], o.entities[0]);
    p.apply(Command::SetPathRole {
        sheet,
        entity: ab,
        role: PathRole::CuttingContour,
    })
    .unwrap();
    p.apply(Command::AddNotch {
        sheet,
        entity: ab,
        distance_mm: 10.0,
        side: Side::Left,
        length_mm: 5.0,
        style: NotchStyle::Single,
    })
    .unwrap();
    Half {
        p,
        sheet,
        guide,
        pts: [a, b, c, d],
        ents: vec![ab, bc, cd],
    }
}

fn link(h: &mut Half) -> Outcome {
    h.p.apply(Command::LinkMirror {
        sheet: h.sheet,
        points: Vec::new(),
        entities: h.ents.clone(),
        axis: MirrorAxis::Guide { guide: h.guide },
    })
    .unwrap()
}

fn refused(p: &mut Project, command: Command) -> CoreError {
    let before = p.clone();
    let err = p.apply(command).expect_err("doit être refusée");
    assert!(!err.to_string().is_empty());
    assert_eq!(*p, before, "une commande refusée ne laisse aucune trace");
    err
}

/// Compare chaque tracé à son miroir : même point à la fraction f du tracé
/// miroir que le réfléchi du point à la fraction 1 − f de la source (sens inversé).
fn assert_mirrored(s: &Sheet, src: EntityId, mir: EntityId, tol: f64) {
    let (ls, lm) = (
        s.path_length_mm(src).unwrap(),
        s.path_length_mm(mir).unwrap(),
    );
    assert!((ls - lm).abs() <= tol, "longueurs {ls} {lm}");
    for i in 0..=10 {
        let f = f64::from(i) / 10.0;
        let (m, _) = s.path_point_at(mir, f * lm).unwrap();
        let (q, _) = s.path_point_at(src, (1.0 - f) * ls).unwrap();
        assert!(
            close(m, reflect(q), tol),
            "f={f} : {m:?} vs {:?}",
            reflect(q)
        );
    }
}

// ------------------------------------------------------------ CA 1

#[test]
fn ca1_points_et_traces_miroir_exacts_a_1e9() {
    let mut h = half();
    let out = link(&mut h);
    let s = h.p.sheet(h.sheet).unwrap();
    // A et D sont sur l'axe : partagés, seuls B et C ont une image.
    assert_eq!(out.points.len(), 2);
    assert_eq!(out.entities.len(), 3);
    for (&src, &mir) in [h.pts[1], h.pts[2]].iter().zip(&out.points) {
        let def = s.point(mir).unwrap().definition;
        assert!(matches!(
            def,
            PointDefinition::Mirror { source, axis: MirrorAxis::Guide { guide } }
                if source == src && guide == h.guide
        ));
        let (ps, pm) = (s.position(src).unwrap(), s.position(mir).unwrap());
        assert!(close(pm, reflect(ps), 1e-9), "{pm:?}");
    }
    // Segment : extrémités échangées (sens inversé), rôle conservé.
    let (ab, bc, cd) = (h.ents[0], h.ents[1], h.ents[2]);
    let m_ab = s.entity(out.entities[0]).unwrap();
    assert_eq!(m_ab.role, PathRole::CuttingContour);
    assert!(matches!(m_ab.kind, EntityKind::Segment { from, to, .. }
        if from == out.points[0] && to == h.pts[0]));
    assert!(matches!(
        s.entity(out.entities[1]).unwrap().kind,
        EntityKind::Curve { .. }
    ));
    assert_mirrored(s, ab, out.entities[0], 1e-9);
    assert_mirrored(s, bc, out.entities[1], 1e-6);
    assert_mirrored(s, cd, out.entities[2], 1e-9);
    // Cran : reflété (pied et pointe), côté conservé par l'inversion du sens.
    assert_eq!(out.notches.len(), 1);
    let src_marks = s.notch_marks(&s.notches[0]).unwrap();
    let mir_marks = s.notch_marks(s.notch(out.notches[0]).unwrap()).unwrap();
    assert!(close(mir_marks[0].0, reflect(src_marks[0].0), 1e-9));
    assert!(close(mir_marks[0].1, reflect(src_marks[0].1), 1e-9));
    h.p.validate().unwrap();
}

#[test]
fn ca1_arcs_cercles_et_courbes_lisses_refletes() {
    let mut p = Project::new("Formes");
    let sheet = p.sheets[0].id;
    let guide = p
        .apply(Command::AddGuide {
            sheet,
            x_mm: 0.0,
            y_mm: 0.0,
            angle_deg: 90.0,
            axis: true,
            layer: None,
        })
        .unwrap()
        .guides[0];
    let mut ents = Vec::new();
    for cmd in [
        Command::AddArc {
            sheet,
            center: PointRef::New(pt(30.0, 10.0)),
            radius_mm: 12.0,
            start_deg: 20.0,
            end_deg: 130.0,
            ccw: true,
        },
        Command::AddArc {
            sheet,
            center: PointRef::New(pt(60.0, 10.0)),
            radius_mm: 8.0,
            start_deg: 200.0,
            end_deg: 300.0,
            ccw: false,
        },
        Command::AddCircle {
            sheet,
            center: PointRef::New(pt(25.0, 50.0)),
            radius_mm: 7.0,
        },
        Command::AddSpline {
            sheet,
            points: vec![
                PointRef::New(pt(10.0, 80.0)),
                PointRef::New(pt(25.0, 95.0)),
                PointRef::New(pt(45.0, 85.0)),
                PointRef::New(pt(60.0, 100.0)),
            ],
        },
    ] {
        ents.push(p.apply(cmd).unwrap().entities[0]);
    }
    p.apply(Command::SetSplineNode {
        sheet,
        spline: ents[3],
        index: 1,
        corner: true,
        tangent: Some(OffsetMm { dx: 9.0, dy: 4.0 }),
        tangent_in: Some(OffsetMm { dx: 3.0, dy: -2.0 }),
    })
    .unwrap();
    let out = p
        .apply(Command::LinkMirror {
            sheet,
            points: Vec::new(),
            entities: ents.clone(),
            axis: MirrorAxis::Guide { guide },
        })
        .unwrap();
    let s = p.sheet(sheet).unwrap();
    assert_eq!(out.entities.len(), 4);
    for (src, mir) in ents.iter().zip(&out.entities) {
        assert_mirrored(s, *src, *mir, 1e-6);
    }
    // Les sens de parcours sont cohérents avec la symétrie (aire de l'arc inchangée).
    assert!(matches!(
        s.entity(out.entities[0]).unwrap().kind,
        EntityKind::Arc { ccw: true, .. }
    ));
    assert!(matches!(
        s.entity(out.entities[1]).unwrap().kind,
        EntityKind::Arc { ccw: false, .. }
    ));
}

#[test]
fn ca1_axe_segment_de_reference_oblique() {
    let mut p = Project::new("Axe segment");
    let sheet = p.sheets[0].id;
    let o = p
        .apply(Command::AddSegment {
            sheet,
            from: PointRef::New(pt(0.0, 0.0)),
            to: PointRef::New(pt(30.0, 30.0)),
        })
        .unwrap();
    let axis_seg = o.entities[0];
    let t = p
        .apply(Command::AddPoint {
            sheet,
            at: pt(20.0, 5.0),
        })
        .unwrap()
        .points[0];
    let out = p
        .apply(Command::LinkMirror {
            sheet,
            points: vec![t],
            entities: Vec::new(),
            axis: MirrorAxis::Segment { segment: axis_seg },
        })
        .unwrap();
    let s = p.sheet(sheet).unwrap();
    assert!(close(
        s.position(out.points[0]).unwrap(),
        pt(5.0, 20.0),
        1e-9
    ));
    // L'axe bouge : le miroir suit.
    let (a, b) = s.segment_point_ids(axis_seg).unwrap();
    let _ = a;
    p.apply(Command::MovePoint {
        sheet,
        point: b,
        to: pt(30.0, 0.0),
    })
    .unwrap();
    let s = p.sheet(sheet).unwrap();
    assert!(close(
        s.position(out.points[0]).unwrap(),
        pt(20.0, -5.0),
        1e-9
    ));
}

// ------------------------------------------------------------ CA 2

#[test]
fn ca2_cote_miroir_non_editable_et_source_modifiee_met_a_jour_le_miroir() {
    let mut h = half();
    let out = link(&mut h);
    let mirror_b = out.points[0];
    let err = refused(
        &mut h.p,
        Command::MovePoint {
            sheet: h.sheet,
            point: mirror_b,
            to: pt(-50.0, 3.0),
        },
    );
    assert!(err.to_string().contains("maintenu par symétrie"), "{err}");
    // Transformer le miroir seul : aucun point libre, refus.
    refused(
        &mut h.p,
        Command::Transform {
            sheet: h.sheet,
            points: vec![mirror_b],
            transform: Transform::Translate {
                dx_mm: 1.0,
                dy_mm: 1.0,
            },
        },
    );
    // La source bouge : le miroir suit (réflexion exacte).
    h.p.apply(Command::MovePoint {
        sheet: h.sheet,
        point: h.pts[1],
        to: pt(47.5, -3.25),
    })
    .unwrap();
    let s = h.p.sheet(h.sheet).unwrap();
    assert!(close(s.position(mirror_b).unwrap(), pt(-47.5, -3.25), 1e-9));
    // L'axe glisse le long de sa direction : l'image ne change pas.
    h.p.apply(Command::MoveGuide {
        sheet: h.sheet,
        guide: h.guide,
        x_mm: 0.0,
        y_mm: 25.0,
        angle_deg: 90.0,
    })
    .unwrap();
    let s = h.p.sheet(h.sheet).unwrap();
    assert!(close(s.position(mirror_b).unwrap(), pt(-47.5, -3.25), 1e-9));
    h.p.validate().unwrap();
}

// ------------------------------------------------------------ CA 3

#[test]
fn ca3_deplier_rend_independant_ferme_le_contour_et_s_annule_en_une_etape() {
    let mut h = half();
    link(&mut h);
    let mut doc = Document::new(h.p.clone());
    let before = doc.project().clone();
    let out = doc
        .execute(Command::UnfoldMirror {
            sheet: h.sheet,
            axis: MirrorAxis::Guide { guide: h.guide },
        })
        .unwrap();
    let s = doc.project().sheet(h.sheet).unwrap();
    assert!(s.points.iter().all(|p| !p.definition.is_mirror()));
    assert!(
        out.notes.iter().any(|n| n.contains("contour(s) fermé(s)")),
        "{:?}",
        out.notes
    );
    let contours = contour::closed_contours(s);
    assert_eq!(contours.len(), 1);
    assert_eq!(contours[0].edges.len(), 6);
    let m = contour::measure_contour(s, &contours[0]).unwrap();
    assert!(m.area_mm2 > 80.0 * 60.0, "aire {}", m.area_mm2);
    // Indépendant : bouger l'ancien miroir est maintenant permis.
    let mirror_b =
        h.p.sheet(h.sheet)
            .unwrap()
            .points
            .iter()
            .find(|p| p.definition.is_mirror())
            .unwrap()
            .id;
    doc.execute(Command::MovePoint {
        sheet: h.sheet,
        point: mirror_b,
        to: pt(-45.0, 0.0),
    })
    .unwrap();
    assert!(doc.undo());
    assert!(doc.undo());
    assert_eq!(*doc.project(), before, "annulation exacte");
}

#[test]
fn ca3_deplier_sans_jonction_sur_l_axe_laisse_les_traces_avec_remarque() {
    let mut p = Project::new("Sans jonction");
    let sheet = p.sheets[0].id;
    let guide = p
        .apply(Command::AddGuide {
            sheet,
            x_mm: 0.0,
            y_mm: 0.0,
            angle_deg: 90.0,
            axis: true,
            layer: None,
        })
        .unwrap()
        .guides[0];
    let e = p
        .apply(Command::AddSegment {
            sheet,
            from: PointRef::New(pt(10.0, 0.0)),
            to: PointRef::New(pt(40.0, 5.0)),
        })
        .unwrap()
        .entities[0];
    p.apply(Command::LinkMirror {
        sheet,
        points: Vec::new(),
        entities: vec![e],
        axis: MirrorAxis::Guide { guide },
    })
    .unwrap();
    let out = p
        .apply(Command::UnfoldMirror {
            sheet,
            axis: MirrorAxis::Guide { guide },
        })
        .unwrap();
    assert!(
        out.notes.iter().any(|n| n.contains("pas de contour fermé")),
        "{:?}",
        out.notes
    );
    assert_eq!(p.sheet(sheet).unwrap().entities.len(), 2);
    // Plus aucune symétrie à déplier : refus sans trace.
    refused(
        &mut p,
        Command::UnfoldMirror {
            sheet,
            axis: MirrorAxis::Guide { guide },
        },
    );
}

// ------------------------------------------------------------ CA 4

#[test]
fn ca4_suppression_de_l_axe_fige_et_de_la_source_est_refusee_avec_message() {
    // Axe = guide supprimé : miroirs figés avec remarque.
    let mut h = half();
    let out = link(&mut h);
    let before_pos = h.p.sheet(h.sheet).unwrap().position(out.points[0]).unwrap();
    let del =
        h.p.apply(Command::DeleteGuide {
            sheet: h.sheet,
            guide: h.guide,
        })
        .unwrap();
    assert!(
        del.notes.iter().any(|n| n.contains("maintenant libres")),
        "{:?}",
        del.notes
    );
    let s = h.p.sheet(h.sheet).unwrap();
    assert!(s.point(out.points[0]).unwrap().definition.is_free());
    assert_eq!(s.position(out.points[0]).unwrap(), before_pos);

    // Axe = segment supprimé : figé aussi (réutilise le gel des points construits).
    let mut p = Project::new("Axe segment");
    let sheet = p.sheets[0].id;
    let axis = p
        .apply(Command::AddSegment {
            sheet,
            from: PointRef::New(pt(0.0, 0.0)),
            to: PointRef::New(pt(0.0, 50.0)),
        })
        .unwrap()
        .entities[0];
    let t = p
        .apply(Command::AddPoint {
            sheet,
            at: pt(12.0, 7.0),
        })
        .unwrap()
        .points[0];
    let m = p
        .apply(Command::LinkMirror {
            sheet,
            points: vec![t],
            entities: Vec::new(),
            axis: MirrorAxis::Segment { segment: axis },
        })
        .unwrap()
        .points[0];
    let del = p
        .apply(Command::Delete {
            sheet,
            points: Vec::new(),
            entities: vec![axis],
        })
        .unwrap();
    assert!(!del.notes.is_empty());
    assert!(p
        .sheet(sheet)
        .unwrap()
        .point(m)
        .unwrap()
        .definition
        .is_free());

    // Supprimer la source seule : refus clair, rien ne change.
    let mut h = half();
    link(&mut h);
    let err = refused(
        &mut h.p,
        Command::Delete {
            sheet: h.sheet,
            points: vec![h.pts[1]],
            entities: Vec::new(),
        },
    );
    assert!(err.to_string().contains("parent"), "{err}");
}

// ------------------------------------------------------- refus sans trace

#[test]
fn refus_sans_trace() {
    let mut h = half();
    let (sheet, guide) = (h.sheet, h.guide);
    let ents = h.ents.clone();
    // Guide non marqué comme axe.
    h.p.apply(Command::SetGuideAxis {
        sheet,
        guide,
        axis: false,
    })
    .unwrap();
    let e = refused(
        &mut h.p,
        Command::LinkMirror {
            sheet,
            points: Vec::new(),
            entities: ents.clone(),
            axis: MirrorAxis::Guide { guide },
        },
    );
    assert!(e.to_string().contains("axe"), "{e}");
    h.p.apply(Command::SetGuideAxis {
        sheet,
        guide,
        axis: true,
    })
    .unwrap();
    let axis = MirrorAxis::Guide { guide };
    // Sélection vide, axe absent, tracé absent.
    refused(
        &mut h.p,
        Command::LinkMirror {
            sheet,
            points: vec![],
            entities: vec![],
            axis,
        },
    );
    refused(
        &mut h.p,
        Command::LinkMirror {
            sheet,
            points: vec![],
            entities: ents.clone(),
            axis: MirrorAxis::Guide {
                guide: GuideId(987_654),
            },
        },
    );
    refused(
        &mut h.p,
        Command::LinkMirror {
            sheet,
            points: vec![],
            entities: vec![EntityId(987_654)],
            axis,
        },
    );
    // Axe segment faisant partie de la sélection.
    refused(
        &mut h.p,
        Command::LinkMirror {
            sheet,
            points: vec![],
            entities: ents.clone(),
            axis: MirrorAxis::Segment { segment: ents[0] },
        },
    );
    // Tout est sur l'axe : rien à refléter.
    h.p.apply(Command::AddSegment {
        sheet,
        from: PointRef::Existing(h.pts[0]),
        to: PointRef::Existing(h.pts[3]),
    })
    .unwrap();
    let on_axis = h.p.sheet(sheet).unwrap().entities.last().unwrap().id;
    refused(
        &mut h.p,
        Command::LinkMirror {
            sheet,
            points: vec![h.pts[0], h.pts[3]],
            entities: vec![on_axis],
            axis,
        },
    );
    // Objet déjà miroir d'un autre.
    let out = link(&mut h);
    let e = refused(
        &mut h.p,
        Command::LinkMirror {
            sheet,
            points: vec![out.points[0]],
            entities: Vec::new(),
            axis,
        },
    );
    assert!(e.to_string().contains("déjà"), "{e}");
    refused(
        &mut h.p,
        Command::LinkMirror {
            sheet,
            points: vec![],
            entities: vec![out.entities[0]],
            axis,
        },
    );
}

#[test]
fn cycle_et_axe_degenere_dans_un_fichier_forge_refuses() {
    let mut h = half();
    let out = link(&mut h);
    let json = persist::to_json(&h.p).unwrap();
    // Le miroir devient sa propre source : cycle.
    let m = out.points[0];
    let mut forged = h.p.clone();
    let s = forged.sheets.iter_mut().find(|s| s.id == h.sheet).unwrap();
    s.points.iter_mut().find(|p| p.id == m).unwrap().definition = PointDefinition::Mirror {
        source: m,
        axis: MirrorAxis::Guide { guide: h.guide },
    };
    assert!(matches!(
        forged.validate(),
        Err(CoreError::CyclicReference(_))
    ));
    assert!(persist::to_json(&forged).is_err());
    // Axe segment dégénéré (points confondus) : refusé à la validation.
    let mut forged = persist::from_json(&json).unwrap();
    let s = forged.sheets.iter_mut().find(|s| s.id == h.sheet).unwrap();
    s.guides.clear();
    assert!(forged.validate().is_err(), "axe absent");
}

#[test]
fn calque_verrouille_respecte() {
    let mut h = half();
    let layer =
        h.p.apply(Command::AddLayer {
            sheet: h.sheet,
            name: "Verrou".into(),
        })
        .unwrap()
        .layers[0];
    h.p.apply(Command::SetObjectsLayer {
        sheet: h.sheet,
        points: vec![h.pts[1]],
        entities: vec![h.ents[0]],
        layer: Some(layer),
    })
    .unwrap();
    h.p.apply(Command::SetLayerLocked {
        sheet: h.sheet,
        layer,
        locked: true,
    })
    .unwrap();
    // Le miroir irait dans le calque verrouillé : refusé.
    let e = refused(
        &mut h.p,
        Command::LinkMirror {
            sheet: h.sheet,
            points: Vec::new(),
            entities: vec![h.ents[0]],
            axis: MirrorAxis::Guide { guide: h.guide },
        },
    );
    assert!(matches!(e, CoreError::LayerLocked(_)), "{e}");
    // Un calque déverrouillé : accepté, calque conservé ; reverrouillé, déplier est refusé.
    h.p.apply(Command::SetLayerLocked {
        sheet: h.sheet,
        layer,
        locked: false,
    })
    .unwrap();
    let out = link(&mut h);
    let s = h.p.sheet(h.sheet).unwrap();
    assert_eq!(s.entity(out.entities[0]).unwrap().layer, Some(layer));
    h.p.apply(Command::SetLayerLocked {
        sheet: h.sheet,
        layer,
        locked: true,
    })
    .unwrap();
    let e = refused(
        &mut h.p,
        Command::UnfoldMirror {
            sheet: h.sheet,
            axis: MirrorAxis::Guide { guide: h.guide },
        },
    );
    assert!(matches!(e, CoreError::LayerLocked(_)), "{e}");
}

// ------------------------------------------------------ fichiers, copies

#[test]
fn fichier_schema_3_sans_montee_et_sans_symetrie_inchange() {
    let demo = demo_piece_project().unwrap();
    let json = persist::to_json(&demo).unwrap();
    assert!(!json.contains("\"mirror\""));
    assert_eq!(
        persist::to_json(&persist::from_json(&json).unwrap()).unwrap(),
        json
    );

    let mut h = half();
    link(&mut h);
    assert_eq!(h.p.schema_version, 6);
    let with = persist::to_json(&h.p).unwrap();
    assert!(with.contains("\"mirror\""));
    let back = persist::from_json(&with).unwrap();
    assert_eq!(back, h.p);
    assert_eq!(persist::to_json(&back).unwrap(), with);
}

#[test]
fn copie_de_feuille_reecrit_source_et_axe() {
    let mut h = half();
    let out = link(&mut h);
    let o =
        h.p.apply(Command::DuplicateSheet { sheet: h.sheet })
            .unwrap();
    let copy = o.sheet.unwrap();
    let cs = h.p.sheet(copy).unwrap().clone();
    let mirrors: Vec<&Point> = cs
        .points
        .iter()
        .filter(|p| p.definition.is_mirror())
        .collect();
    assert_eq!(mirrors.len(), 2);
    for m in &mirrors {
        let PointDefinition::Mirror { source, axis } = m.definition else {
            unreachable!()
        };
        assert!(cs.point(source).is_ok(), "source dans la copie");
        let MirrorAxis::Guide { guide } = axis else {
            panic!()
        };
        assert!(cs.guide(guide).is_ok(), "axe dans la copie");
        assert_ne!(guide, h.guide);
    }
    // Indépendance : bouger la source de l'original ne touche pas la copie.
    let before = cs.position(mirrors[0].id).unwrap();
    h.p.apply(Command::MovePoint {
        sheet: h.sheet,
        point: h.pts[1],
        to: pt(70.0, 0.0),
    })
    .unwrap();
    assert_eq!(
        h.p.sheet(copy).unwrap().position(mirrors[0].id).unwrap(),
        before
    );
    assert!(close(
        h.p.sheet(h.sheet).unwrap().position(out.points[0]).unwrap(),
        pt(-70.0, 0.0),
        1e-9
    ));
}

#[test]
fn presse_papiers_garde_la_symetrie_avec_son_axe_sinon_fige() {
    let mut h = half();
    let out = link(&mut h);
    let all_ents: Vec<EntityId> = h.ents.iter().chain(&out.entities).copied().collect();
    // Avec l'axe : maintenue après collage.
    let frag =
        h.p.copy_fragment_with_guides(h.sheet, &[], &all_ents, &[], &[], &[h.guide])
            .unwrap();
    let frag = Fragment::from_json(&frag.to_json()).unwrap();
    let pasted =
        h.p.apply(Command::PasteFragment {
            sheet: h.sheet,
            fragment: Box::new(frag),
            layer: None,
        })
        .unwrap();
    let s = h.p.sheet(h.sheet).unwrap();
    let kept: Vec<&PointId> = pasted
        .points
        .iter()
        .filter(|&&id| s.point(id).unwrap().definition.is_mirror())
        .collect();
    assert_eq!(kept.len(), 2, "symétrie maintenue dans la copie");
    for id in kept {
        let PointDefinition::Mirror {
            axis: MirrorAxis::Guide { guide },
            ..
        } = s.point(*id).unwrap().definition
        else {
            panic!()
        };
        assert!(pasted.guides.contains(&guide), "axe collé, pas l'original");
    }
    // Sans l'axe : les miroirs deviennent libres, avec remarque.
    let frag =
        h.p.copy_fragment(h.sheet, &[], &all_ents, &[], &[])
            .unwrap();
    assert!(frag.points.iter().all(|p| !p.definition.is_mirror()));
    assert!(!frag.notes.is_empty());
}

#[test]
fn point_isole_avec_source_derivee_et_annulation_exacte() {
    let mut h = half();
    let mid =
        h.p.apply(Command::AddPointAt {
            sheet: h.sheet,
            point: PointRef::Derived(PointDefinition::Midpoint {
                first: h.pts[1],
                second: h.pts[2],
            }),
        })
        .unwrap()
        .points[0];
    let mut doc = Document::new(h.p.clone());
    let before = doc.project().clone();
    let out = doc
        .execute(Command::LinkMirror {
            sheet: h.sheet,
            points: vec![mid],
            entities: Vec::new(),
            axis: MirrorAxis::Guide { guide: h.guide },
        })
        .unwrap();
    let s = doc.project().sheet(h.sheet).unwrap();
    assert!(close(
        s.position(out.points[0]).unwrap(),
        pt(-40.0, 30.0),
        1e-9
    ));
    let n = s.points.len();
    assert!(doc.undo());
    assert_eq!(*doc.project(), before);
    assert!(doc.redo());
    assert_eq!(doc.project().sheet(h.sheet).unwrap().points.len(), n);
}

#[test]
fn l_axe_deplace_le_miroir_d_un_point_isole() {
    let mut h = half();
    let t = h.pts[1];
    let out =
        h.p.apply(Command::LinkMirror {
            sheet: h.sheet,
            points: vec![t],
            entities: Vec::new(),
            axis: MirrorAxis::Guide { guide: h.guide },
        })
        .unwrap();
    h.p.apply(Command::MoveGuide {
        sheet: h.sheet,
        guide: h.guide,
        x_mm: 10.0,
        y_mm: 0.0,
        angle_deg: 90.0,
    })
    .unwrap();
    let s = h.p.sheet(h.sheet).unwrap();
    assert!(close(
        s.position(out.points[0]).unwrap(),
        pt(-20.0, 0.0),
        1e-9
    ));
}
