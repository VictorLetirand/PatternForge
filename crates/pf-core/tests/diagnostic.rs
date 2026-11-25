//! US-03.5 — diagnostic des liens : cas construits à la main (valeurs attendues écrites ici, pas
//! relues du code testé). La campagne adverse est dans `adverse_diagnostic.rs`.

use pf_core::persist;
use pf_core::*;
use pf_geometry::{PointMm, Side};

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

fn count(p: &Project, severity: DiagnosticSeverity, link: LinkType) -> usize {
    p.link_diagnostics()
        .iter()
        .filter(|d| d.severity == severity && d.link == link)
        .count()
}

fn total(p: &Project, severity: DiagnosticSeverity) -> usize {
    p.link_diagnostics()
        .iter()
        .filter(|d| d.severity == severity)
        .count()
}

/// Une formule (`d = largeur/2+10` sur un point en direction), une construction maintenue (segment
/// 60 mm à 0°) et une longueur imposée (Bézier, extrémité glissante) : trois liens.
struct Three {
    p: Project,
    sheet: SheetId,
    /// Point dont la distance est liée à `largeur`.
    formula_point: PointId,
    /// Départ, arrivée (suivie) et segment maintenu.
    kept_from: PointId,
    kept_to: PointId,
    kept_seg: EntityId,
    /// Courbe à longueur imposée et son extrémité glissante.
    curve: EntityId,
    curve_end: PointId,
}

fn three() -> Three {
    let mut p = Project::new("Diagnostic");
    let sheet = p.sheets[0].id;
    p.apply(Command::AddVariable {
        name: "largeur".into(),
        unit: VariableUnit::Mm,
        expr: "120".into(),
    })
    .unwrap();
    let o = p
        .apply(Command::AddPoint {
            sheet,
            at: pt(0.0, 0.0),
        })
        .unwrap()
        .points[0];
    let c = p
        .apply(Command::AddPointAt {
            sheet,
            point: PointRef::Derived(PointDefinition::Polar {
                from: o,
                distance_mm: 50.0,
                angle_deg: 30.0,
                reference: None,
            }),
        })
        .unwrap()
        .points[0];
    p.apply(Command::SetParameterExpr {
        target: ParamTarget {
            sheet,
            point: c,
            param: ParamKind::Distance,
        },
        expr: Some("largeur/2+10".into()),
    })
    .unwrap();
    let a = p
        .apply(Command::AddPoint {
            sheet,
            at: pt(0.0, 100.0),
        })
        .unwrap()
        .points[0];
    let ab = p
        .apply(Command::AddSegmentPolar {
            sheet,
            from: PointRef::Existing(a),
            length_mm: 60.0,
            angle_deg: 0.0,
        })
        .unwrap();
    let (b, seg) = (ab.points[0], ab.entities[0]);
    p.apply(Command::SetConstructionKept {
        sheet,
        segment: seg,
        kept: true,
    })
    .unwrap();
    let k = p
        .apply(Command::AddCurve {
            sheet,
            start: PointRef::New(pt(0.0, -100.0)),
            c1: pt(40.0, -10.0),
            c2: pt(150.0, -10.0),
            end: PointRef::New(pt(190.0, -100.0)),
        })
        .unwrap();
    let (end, curve) = (k.points[1], k.entities[0]);
    p.apply(Command::SetCurveLengthConstraint {
        sheet,
        curve,
        spec: Some(LengthSpec {
            length_mm: 300.0,
            adjust: LengthAdjust::Slide {
                point: end,
                angle_deg: 0.0,
            },
            reference: None,
        }),
    })
    .unwrap();
    Three {
        p,
        sheet,
        formula_point: c,
        kept_from: a,
        kept_to: b,
        kept_seg: seg,
        curve,
        curve_end: end,
    }
}

#[test]
fn projet_sans_lien_ni_variable_donne_une_liste_vide() {
    assert!(Project::new("Vide").link_diagnostics().is_empty());
    assert!(demo_project().unwrap().link_diagnostics().is_empty());
}

#[test]
fn trois_liens_donnent_trois_lignes_d_information_et_rien_d_autre() {
    let t = three();
    let d = t.p.link_diagnostics();
    assert_eq!(d.len(), 3, "{d:#?}");
    assert!(d.iter().all(|x| x.severity == DiagnosticSeverity::Info));
    assert!(d.iter().all(|x| x.kind == DiagnosticKind::Active));
    let links: Vec<LinkType> = d.iter().map(|x| x.link).collect();
    assert_eq!(
        links,
        vec![
            LinkType::Formula,
            LinkType::KeptConstruction,
            LinkType::ImposedLength
        ]
    );
    // Formule : point en direction, paramètre distance, texte et valeur écrite (120/2+10 = 70).
    assert_eq!(d[0].objects, vec![DiagnosticObject::Point(t.formula_point)]);
    assert_eq!(d[0].param, Some(ParamKind::Distance));
    assert_eq!(d[0].text, "largeur/2+10");
    assert_eq!(d[0].value, Some(70.0));
    assert_eq!(d[0].sheet, Some(t.sheet));
    // Construction maintenue : segment d'abord, puis le point d'arrivée suivi (pas le départ).
    assert_eq!(
        d[1].objects,
        vec![
            DiagnosticObject::Entity(t.kept_seg),
            DiagnosticObject::Point(t.kept_to)
        ]
    );
    assert!(!d[1].followed_points().any(|p| p == t.kept_from));
    // Longueur imposée : la courbe, puis l'extrémité qui glisse ; 300 mm ; pas d'égalité.
    assert_eq!(
        d[2].objects,
        vec![
            DiagnosticObject::Entity(t.curve),
            DiagnosticObject::Point(t.curve_end)
        ]
    );
    assert_eq!(d[2].value, Some(300.0));
    assert_eq!(d[2].offset_mm, None);
    // La variable est citée par la formule : pas d'avertissement.
    assert_eq!(total(&t.p, DiagnosticSeverity::Warning), 0);
    assert_eq!(total(&t.p, DiagnosticSeverity::Error), 0);
}

#[test]
fn egalite_de_longueurs_est_un_type_distinct_de_la_longueur_imposee() {
    let mut t = three();
    let seg =
        t.p.apply(Command::AddSegment {
            sheet: t.sheet,
            from: PointRef::New(pt(0.0, 300.0)),
            to: PointRef::New(pt(300.0, 300.0)),
        })
        .unwrap()
        .entities[0];
    t.p.apply(Command::SetCurveLengthConstraint {
        sheet: t.sheet,
        curve: t.curve,
        spec: Some(LengthSpec {
            length_mm: 1.0,
            adjust: LengthAdjust::Slide {
                point: t.curve_end,
                angle_deg: 0.0,
            },
            reference: Some(LengthRef {
                entity: seg,
                offset_mm: 12.0,
            }),
        }),
    })
    .unwrap();
    assert_eq!(
        count(&t.p, DiagnosticSeverity::Info, LinkType::ImposedLength),
        0
    );
    assert_eq!(
        count(&t.p, DiagnosticSeverity::Info, LinkType::LengthEquality),
        1
    );
    let eq =
        t.p.link_diagnostics()
            .into_iter()
            .find(|d| d.link == LinkType::LengthEquality)
            .unwrap();
    assert_eq!(eq.offset_mm, Some(12.0));
    assert!((eq.value.unwrap() - 312.0).abs() < 1e-6);
    assert!(eq.objects.contains(&DiagnosticObject::Entity(seg)));
}

#[test]
fn variable_inutilisee_donne_un_avertissement_et_une_variable_citee_aucun() {
    let mut t = three();
    t.p.apply(Command::AddVariable {
        name: "inutile".into(),
        unit: VariableUnit::Mm,
        expr: "5".into(),
    })
    .unwrap();
    let d = t.p.link_diagnostics();
    let warnings: Vec<_> = d
        .iter()
        .filter(|x| x.severity == DiagnosticSeverity::Warning)
        .collect();
    assert_eq!(warnings.len(), 1, "{d:#?}");
    assert_eq!(warnings[0].kind, DiagnosticKind::UnusedVariable);
    assert_eq!(warnings[0].link, LinkType::Variable);
    assert_eq!(warnings[0].text, "inutile");
    assert_eq!(warnings[0].sheet, None);
    // Citée par une autre variable : « largeur » reste utilisée, « inutile » devient utilisée.
    t.p.apply(Command::AddVariable {
        name: "double".into(),
        unit: VariableUnit::Mm,
        expr: "inutile*2".into(),
    })
    .unwrap();
    let names: Vec<String> =
        t.p.link_diagnostics()
            .into_iter()
            .filter(|x| x.kind == DiagnosticKind::UnusedVariable)
            .map(|x| x.text)
            .collect();
    assert_eq!(names, vec!["double".to_string()]);
}

#[test]
fn variables_seules_sans_aucun_lien_sont_signalees_pas_ignorees() {
    let mut p = Project::new("Variables");
    p.apply(Command::AddVariable {
        name: "a".into(),
        unit: VariableUnit::Mm,
        expr: "1".into(),
    })
    .unwrap();
    let d = p.link_diagnostics();
    assert_eq!(d.len(), 1);
    assert_eq!(d[0].kind, DiagnosticKind::UnusedVariable);
}

#[test]
fn calque_verrouille_portant_un_point_suivi_donne_un_avertissement_et_la_propagation_est_refusee() {
    let mut t = three();
    t.p.apply(Command::AddLayer {
        sheet: t.sheet,
        name: "Verrou".into(),
    })
    .unwrap();
    let layer = t.p.sheets[0].layers.last().unwrap().id;
    t.p.apply(Command::SetObjectsLayer {
        sheet: t.sheet,
        points: vec![t.kept_to],
        entities: vec![],
        layer: Some(layer),
    })
    .unwrap();
    assert_eq!(
        total(&t.p, DiagnosticSeverity::Warning),
        0,
        "pas verrouillé"
    );
    t.p.apply(Command::SetLayerLocked {
        sheet: t.sheet,
        layer,
        locked: true,
    })
    .unwrap();
    let d = t.p.link_diagnostics();
    let warnings: Vec<_> = d
        .iter()
        .filter(|x| x.severity == DiagnosticSeverity::Warning)
        .collect();
    assert_eq!(warnings.len(), 1, "{d:#?}");
    assert_eq!(warnings[0].kind, DiagnosticKind::LockedLayer);
    assert_eq!(warnings[0].link, LinkType::KeptConstruction);
    assert!(warnings[0]
        .objects
        .contains(&DiagnosticObject::Point(t.kept_to)));
    // Le lien reste actif (information) : l'avertissement s'ajoute, il ne le remplace pas.
    assert_eq!(
        count(&t.p, DiagnosticSeverity::Info, LinkType::KeptConstruction),
        1
    );
    // L'avertissement est vrai : déplacer le parent est refusé, sans trace.
    let before = persist::to_json(&t.p).unwrap();
    let err =
        t.p.apply(Command::MovePoint {
            sheet: t.sheet,
            point: t.kept_from,
            to: pt(10.0, 100.0),
        })
        .unwrap_err();
    assert!(err.to_string().contains("verrouill"), "{err}");
    assert_eq!(persist::to_json(&t.p).unwrap(), before);
}

#[test]
fn le_diagnostic_n_altere_pas_le_projet_et_deux_appels_donnent_la_meme_liste() {
    let t = three();
    let before = persist::to_json(&t.p).unwrap();
    let a = t.p.link_diagnostics();
    let b = t.p.link_diagnostics();
    assert_eq!(a, b);
    assert_eq!(persist::to_json(&t.p).unwrap(), before);
    // Relecture du fichier : même liste, même ordre.
    let again = persist::from_json(&before).unwrap();
    assert_eq!(again.link_diagnostics(), a);
}

#[test]
fn tri_par_gravite_puis_par_type_de_lien() {
    let mut t = three();
    t.p.apply(Command::AddVariable {
        name: "inutile".into(),
        unit: VariableUnit::Mm,
        expr: "5".into(),
    })
    .unwrap();
    // Une erreur : la formule cite une variable disparue (fichier altéré).
    t.p.sheets[0].formulas[0].expr = "absente".into();
    let d = t.p.link_diagnostics();
    let keys: Vec<(DiagnosticSeverity, LinkType)> =
        d.iter().map(|x| (x.severity, x.link)).collect();
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(keys, sorted);
    assert_eq!(d[0].severity, DiagnosticSeverity::Error);
    assert_eq!(d.last().unwrap().severity, DiagnosticSeverity::Info);
}

// --- fichiers altérés à la main : le diagnostic dit ce que l'ouverture refuserait -----------------

#[test]
fn formule_dont_la_variable_a_disparu_est_une_erreur_et_n_est_plus_un_lien_actif() {
    let mut t = three();
    t.p.sheets[0].formulas[0].expr = "absente*2".into();
    assert!(t.p.validate().is_err());
    assert_eq!(count(&t.p, DiagnosticSeverity::Error, LinkType::Formula), 1);
    assert_eq!(count(&t.p, DiagnosticSeverity::Info, LinkType::Formula), 0);
    let e =
        t.p.link_diagnostics()
            .into_iter()
            .find(|d| d.severity == DiagnosticSeverity::Error)
            .unwrap();
    assert!(e.detail.contains("absente"), "{}", e.detail);
    assert_eq!(e.objects, vec![DiagnosticObject::Point(t.formula_point)]);
}

#[test]
fn formule_dont_la_valeur_ecrite_a_derive_est_une_erreur() {
    let mut t = three();
    let sheet = &mut t.p.sheets[0];
    let point = sheet.points.iter_mut().find(|p| p.id == t.formula_point);
    if let Some(Point {
        definition: PointDefinition::Polar { distance_mm, .. },
        ..
    }) = point
    {
        *distance_mm += 1.0;
    } else {
        panic!("point en direction attendu");
    }
    assert!(t.p.validate().is_err());
    assert_eq!(count(&t.p, DiagnosticSeverity::Error, LinkType::Formula), 1);
}

#[test]
fn point_suivi_deplace_de_plus_de_la_tolerance_est_une_erreur_de_construction_maintenue() {
    let mut t = three();
    let sheet = &mut t.p.sheets[0];
    let moved = sheet.points.iter_mut().find(|p| p.id == t.kept_to).unwrap();
    let PointDefinition::Free { x, .. } = &mut moved.definition else {
        panic!("point libre attendu");
    };
    *x += 0.01;
    assert!(t.p.validate().is_err());
    let d = t.p.link_diagnostics();
    let errors: Vec<_> = d
        .iter()
        .filter(|x| x.severity == DiagnosticSeverity::Error)
        .collect();
    assert_eq!(errors.len(), 1, "{d:#?}");
    assert_eq!(errors[0].link, LinkType::KeptConstruction);
    assert_eq!(errors[0].objects[0], DiagnosticObject::Entity(t.kept_seg));
    assert!(errors[0].detail.contains("position écrite"));
    assert_eq!(
        count(&t.p, DiagnosticSeverity::Info, LinkType::KeptConstruction),
        0
    );
}

#[test]
fn ecart_sous_la_tolerance_n_est_pas_une_erreur() {
    let mut t = three();
    let sheet = &mut t.p.sheets[0];
    let moved = sheet.points.iter_mut().find(|p| p.id == t.kept_to).unwrap();
    let PointDefinition::Free { x, .. } = &mut moved.definition else {
        panic!("point libre attendu");
    };
    *x += 1e-11;
    assert!(t.p.validate().is_ok());
    assert_eq!(total(&t.p, DiagnosticSeverity::Error), 0);
}

#[test]
fn reference_de_construction_maintenue_disparue_est_une_erreur() {
    let mut p = Project::new("Perpendiculaire");
    let sheet = p.sheets[0].id;
    let a = p
        .apply(Command::AddSegment {
            sheet,
            from: PointRef::New(pt(0.0, 0.0)),
            to: PointRef::New(pt(100.0, 0.0)),
        })
        .unwrap();
    let (a0, reference) = (a.points[0], a.entities[0]);
    let perp = p
        .apply(Command::AddPerpendicular {
            sheet,
            reference,
            from: PointRef::Existing(a0),
            length_mm: 40.0,
            side: Side::Left,
        })
        .unwrap()
        .entities[0];
    p.apply(Command::SetConstructionKept {
        sheet,
        segment: perp,
        kept: true,
    })
    .unwrap();
    assert_eq!(total(&p, DiagnosticSeverity::Error), 0);
    // Fichier altéré : le segment de référence est retiré de la feuille.
    p.sheets[0].entities.retain(|e| e.id != reference);
    let d = p.link_diagnostics();
    let errors: Vec<_> = d
        .iter()
        .filter(|x| x.severity == DiagnosticSeverity::Error)
        .collect();
    assert_eq!(errors.len(), 1, "{d:#?}");
    assert_eq!(errors[0].link, LinkType::KeptConstruction);
    assert_eq!(errors[0].objects[0], DiagnosticObject::Entity(perp));
    assert!(
        errors[0].detail.contains("n'existe plus"),
        "{}",
        errors[0].detail
    );
}

#[test]
fn longueur_ecrite_differente_de_la_cible_est_une_erreur() {
    let mut t = three();
    t.p.sheets[0].length_constraints[0].length_mm += 5.0;
    assert!(t.p.validate().is_err());
    let d = t.p.link_diagnostics();
    let errors: Vec<_> = d
        .iter()
        .filter(|x| x.severity == DiagnosticSeverity::Error)
        .collect();
    assert_eq!(errors.len(), 1, "{d:#?}");
    assert_eq!(errors[0].link, LinkType::ImposedLength);
    assert_eq!(errors[0].objects[0], DiagnosticObject::Entity(t.curve));
    assert_eq!(
        count(&t.p, DiagnosticSeverity::Info, LinkType::ImposedLength),
        0
    );
}

#[test]
fn reference_d_egalite_disparue_est_une_erreur_d_egalite() {
    let mut t = three();
    let seg =
        t.p.apply(Command::AddSegment {
            sheet: t.sheet,
            from: PointRef::New(pt(0.0, 300.0)),
            to: PointRef::New(pt(300.0, 300.0)),
        })
        .unwrap()
        .entities[0];
    t.p.apply(Command::SetCurveLengthConstraint {
        sheet: t.sheet,
        curve: t.curve,
        spec: Some(LengthSpec {
            length_mm: 1.0,
            adjust: LengthAdjust::Slide {
                point: t.curve_end,
                angle_deg: 0.0,
            },
            reference: Some(LengthRef {
                entity: seg,
                offset_mm: 0.0,
            }),
        }),
    })
    .unwrap();
    t.p.sheets[0].entities.retain(|e| e.id != seg);
    assert!(t.p.validate().is_err());
    let d = t.p.link_diagnostics();
    let errors: Vec<_> = d
        .iter()
        .filter(|x| x.severity == DiagnosticSeverity::Error)
        .collect();
    assert_eq!(errors.len(), 1, "{d:#?}");
    assert_eq!(errors[0].link, LinkType::LengthEquality);
    assert!(
        errors[0].detail.contains("n'existe pas"),
        "{}",
        errors[0].detail
    );
}

#[test]
fn table_de_variables_invalide_est_une_erreur_et_les_formules_sont_non_resolues() {
    let mut t = three();
    t.p.variables[0].expr = "inconnue+1".into();
    assert!(t.p.validate().is_err());
    assert_eq!(
        count(&t.p, DiagnosticSeverity::Error, LinkType::Variable),
        1
    );
    assert_eq!(count(&t.p, DiagnosticSeverity::Error, LinkType::Formula), 1);
    assert_eq!(count(&t.p, DiagnosticSeverity::Info, LinkType::Formula), 0);
}

#[test]
fn un_cycle_de_longueurs_imposees_est_une_erreur() {
    // Deux courbes dont chacune lit le point écrit par l'autre : le fichier altéré est refusé.
    let mut p = Project::new("Cycle");
    let sheet = p.sheets[0].id;
    let mut made = Vec::new();
    for dy in [0.0, 80.0] {
        let o = p
            .apply(Command::AddCurve {
                sheet,
                start: PointRef::New(pt(0.0, dy)),
                c1: pt(40.0, dy + 50.0),
                c2: pt(150.0, dy + 50.0),
                end: PointRef::New(pt(190.0, dy)),
            })
            .unwrap();
        made.push((o.points[1], o.entities[0]));
    }
    let (end_a, curve_a) = made[0];
    let (end_b, curve_b) = made[1];
    for (curve, end, other) in [(curve_a, end_a, curve_b), (curve_b, end_b, curve_a)] {
        // A est égale à B + 0 (sans cycle tant que B est libre) ; B sera ensuite égale à A + 0.
        let _ = other;
        p.apply(Command::SetCurveLengthConstraint {
            sheet,
            curve,
            spec: Some(LengthSpec {
                length_mm: 300.0,
                adjust: LengthAdjust::Slide {
                    point: end,
                    angle_deg: 0.0,
                },
                reference: None,
            }),
        })
        .unwrap();
    }
    assert_eq!(total(&p, DiagnosticSeverity::Error), 0);
    // Altération : chacune devient égale à l'autre (ce que `SetCurveLengthConstraint` refuse).
    for (i, other) in [curve_b, curve_a].into_iter().enumerate() {
        let c = &mut p.sheets[0].length_constraints[i];
        c.reference = Some(LengthRef {
            entity: other,
            offset_mm: 0.0,
        });
    }
    assert!(p.validate().is_err());
    let errors = total(&p, DiagnosticSeverity::Error);
    assert!(errors >= 1, "{:#?}", p.link_diagnostics());
}

#[test]
fn les_objets_designes_existent_dans_leur_feuille() {
    let t = three();
    let s = &t.p.sheets[0];
    for d in t.p.link_diagnostics() {
        assert_eq!(d.sheet, Some(s.id));
        for o in &d.objects {
            match o {
                DiagnosticObject::Point(p) => assert!(s.point(*p).is_ok()),
                DiagnosticObject::Entity(e) => assert!(s.entity(*e).is_ok()),
                DiagnosticObject::Variable(_) => unreachable!(),
            }
        }
    }
}

#[test]
fn deux_feuilles_les_liens_gardent_leur_feuille_et_l_ordre_des_feuilles() {
    let mut t = three();
    let first = t.sheet;
    let o = t.p.apply(Command::CreateSheet {
        name: "Deux".into(),
    });
    let second = o.unwrap().sheet.unwrap();
    let a =
        t.p.apply(Command::AddSegmentPolar {
            sheet: second,
            from: PointRef::New(pt(0.0, 0.0)),
            length_mm: 30.0,
            angle_deg: 45.0,
        })
        .unwrap();
    t.p.apply(Command::SetConstructionKept {
        sheet: second,
        segment: a.entities[0],
        kept: true,
    })
    .unwrap();
    let kept: Vec<Option<SheetId>> =
        t.p.link_diagnostics()
            .iter()
            .filter(|d| d.link == LinkType::KeptConstruction)
            .map(|d| d.sheet)
            .collect();
    assert_eq!(kept, vec![Some(first), Some(second)]);
}
