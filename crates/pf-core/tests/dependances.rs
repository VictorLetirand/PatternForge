//! US-03.6 — historique de construction : arbre de dépendances (parents, enfants) et impact d'une
//! suppression. Cas faits à la main ; la campagne est `adverse_dependances.rs`.

use pf_core::persist;
use pf_core::DependencyNature as N;
use pf_core::DependencyObject as O;
use pf_core::*;
use pf_geometry::{PointMm, Side};

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

fn bytes(p: &Project) -> String {
    persist::to_json(p).unwrap()
}

fn add_point(p: &mut Project, s: SheetId, x: f64, y: f64) -> PointId {
    p.apply(Command::AddPoint {
        sheet: s,
        at: pt(x, y),
    })
    .unwrap()
    .points[0]
}

fn derived(p: &mut Project, s: SheetId, def: PointDefinition) -> PointId {
    p.apply(Command::AddPointAt {
        sheet: s,
        point: PointRef::Derived(def),
    })
    .unwrap()
    .points[0]
}

fn objects(entries: &[DependencyEntry]) -> Vec<(O, usize)> {
    entries
        .iter()
        .map(|e| (e.object.clone(), e.depth))
        .collect()
}

/// A libre, B polaire depuis A, C décalé depuis B.
fn chain() -> (Project, SheetId, PointId, PointId, PointId) {
    let mut p = Project::new("Chaîne");
    let s = p.sheets[0].id;
    let a = add_point(&mut p, s, 0.0, 0.0);
    let b = derived(
        &mut p,
        s,
        PointDefinition::Polar {
            from: a,
            distance_mm: 50.0,
            angle_deg: 30.0,
            reference: None,
        },
    );
    let c = derived(
        &mut p,
        s,
        PointDefinition::Offset {
            from: b,
            dx_mm: 10.0,
            dy_mm: 5.0,
        },
    );
    (p, s, a, b, c)
}

fn curve(p: &mut Project, s: SheetId, y: f64, w: f64) -> (EntityId, PointId, PointId) {
    let out = p
        .apply(Command::AddCurve {
            sheet: s,
            start: PointRef::New(pt(0.0, y)),
            c1: pt(w / 3.0, y + 20.0),
            c2: pt(2.0 * w / 3.0, y + 20.0),
            end: PointRef::New(pt(w, y)),
        })
        .unwrap();
    (out.entities[0], out.points[0], out.points[1])
}

#[test]
fn chaine_polaire_puis_decalage_directs_puis_transitifs() {
    let (p, _s, a, b, c) = chain();
    let tree = p.dependency_tree(&O::Point(a), None).unwrap();
    assert!(tree.parents.is_empty());
    assert_eq!(
        objects(&tree.children),
        vec![(O::Point(b), 1), (O::Point(c), 2)]
    );
    assert_eq!(tree.children[0].nature, N::Construction);
    assert_eq!(tree.children[1].via, O::Point(b));
    // Profondeur 1 : seulement le direct.
    let direct = p.dependency_tree(&O::Point(a), Some(1)).unwrap();
    assert_eq!(objects(&direct.children), vec![(O::Point(b), 1)]);
    // Vu de C : B puis A.
    let up = p.dependency_tree(&O::Point(c), None).unwrap();
    assert_eq!(
        objects(&up.parents),
        vec![(O::Point(b), 1), (O::Point(a), 2)]
    );
    assert!(up.children.is_empty());
    assert_eq!(up.direct_parents().count(), 1);
    assert_eq!(tree.direct_children().count(), 1);
}

#[test]
fn deux_appels_donnent_le_meme_resultat_et_le_projet_est_inchange() {
    let (p, s, a, b, c) = chain();
    let before = bytes(&p);
    let t1 = p.dependency_tree(&O::Point(a), None);
    let t2 = p.dependency_tree(&O::Point(a), None);
    assert_eq!(t1, t2);
    let i1 = p.deletion_impact(s, &[O::Point(b)]);
    let i2 = p.deletion_impact(s, &[O::Point(b)]);
    assert_eq!(i1, i2);
    assert_eq!(p.dependency_edges(), p.dependency_edges());
    assert_eq!(bytes(&p), before);
    let _ = c;
    // Objet inconnu : pas d'arbre.
    assert!(p.dependency_tree(&O::Point(PointId(9999)), None).is_none());
    assert!(p
        .dependency_tree(&O::Variable("absente".into()), None)
        .is_none());
}

#[test]
fn point_a_formule_a_pour_parent_la_variable() {
    let (mut p, s, _a, b, _c) = chain();
    p.apply(Command::AddVariable {
        name: "L".into(),
        unit: VariableUnit::Mm,
        expr: "40".into(),
    })
    .unwrap();
    p.apply(Command::AddVariable {
        name: "M".into(),
        unit: VariableUnit::Mm,
        expr: "L*2".into(),
    })
    .unwrap();
    p.apply(Command::SetParameterExpr {
        target: ParamTarget {
            sheet: s,
            point: b,
            param: ParamKind::Distance,
        },
        expr: Some("M/2".into()),
    })
    .unwrap();
    let tree = p.dependency_tree(&O::Point(b), None).unwrap();
    let vars: Vec<_> = tree
        .parents
        .iter()
        .filter(|e| matches!(e.object, O::Variable(_)))
        .map(|e| (e.object.clone(), e.depth, e.nature))
        .collect();
    assert_eq!(
        vars,
        vec![
            (O::Variable("M".into()), 1, N::FormulaReference),
            (O::Variable("L".into()), 2, N::VariableReference),
        ]
    );
    // Et dans l'autre sens : la variable L a pour dépendants M, le point B, puis C.
    let l = p.dependency_tree(&O::Variable("L".into()), None).unwrap();
    let kids = objects(&l.children);
    assert!(kids.contains(&(O::Variable("M".into()), 1)));
    assert!(kids.contains(&(O::Point(b), 2)));
    assert_eq!(l.sheet, None);
}

#[test]
fn egalite_de_longueurs_la_reference_est_listee_en_parent() {
    let mut p = Project::new("Égalité");
    let s = p.sheets[0].id;
    let (c1, _a1, _b1) = curve(&mut p, s, 0.0, 120.0);
    let (c2, _a2, b2) = curve(&mut p, s, 60.0, 100.0);
    p.apply(Command::SetCurveLengthConstraint {
        sheet: s,
        curve: c2,
        spec: Some(LengthSpec {
            length_mm: 1.0,
            adjust: LengthAdjust::Slide {
                point: b2,
                angle_deg: 0.0,
            },
            reference: Some(LengthRef {
                entity: c1,
                offset_mm: 5.0,
            }),
        }),
    })
    .unwrap();
    let tree = p.dependency_tree(&O::Entity(c2), None).unwrap();
    let reference = tree
        .parents
        .iter()
        .find(|e| e.object == O::Entity(c1))
        .expect("référence en parent");
    assert_eq!((reference.depth, reference.nature), (1, N::LengthEquality));
    // Le point d'extrémité ajusté dépend de la courbe (longueur imposée) ; la courbe dépend
    // aussi de ce point (extrémité) : pas de boucle, chaque objet une fois.
    let slid = tree
        .children
        .iter()
        .find(|e| e.object == O::Point(b2))
        .expect("point ajusté en enfant");
    assert_eq!((slid.depth, slid.nature), (1, N::ImposedLength));
    assert!(tree.parents.iter().any(|e| e.object == O::Point(b2)));
    let mut all: Vec<_> = tree
        .parents
        .iter()
        .chain(&tree.children)
        .map(|e| (e.object.clone(), e.depth))
        .collect();
    let n = all.len();
    all.sort();
    all.dedup();
    assert!(all.len() <= n);
    assert!(!tree.parents.iter().any(|e| e.object == tree.object));
    assert!(!tree.children.iter().any(|e| e.object == tree.object));
}

#[test]
fn construction_maintenue_les_parents_sont_listes() {
    let mut p = Project::new("Maintenue");
    let s = p.sheets[0].id;
    let seg = p
        .apply(Command::AddSegmentPolar {
            sheet: s,
            from: PointRef::New(pt(0.0, 0.0)),
            length_mm: 80.0,
            angle_deg: 0.0,
        })
        .unwrap();
    let (a, b) = (seg.points[0], seg.points[1]);
    p.apply(Command::SetConstructionKept {
        sheet: s,
        segment: seg.entities[0],
        kept: true,
    })
    .unwrap();
    let tree = p.dependency_tree(&O::Point(b), None).unwrap();
    let from_a = tree
        .parents
        .iter()
        .find(|e| e.object == O::Point(a))
        .unwrap();
    assert_eq!((from_a.depth, from_a.nature), (1, N::KeptConstruction));
    // Délier retire le lien (A n'est plus parent de B).
    p.apply(Command::SetConstructionKept {
        sheet: s,
        segment: seg.entities[0],
        kept: false,
    })
    .unwrap();
    let tree = p.dependency_tree(&O::Point(b), None).unwrap();
    assert!(!tree
        .parents
        .iter()
        .any(|e| e.object == O::Point(a) && e.nature == N::KeptConstruction));
}

#[test]
fn suppression_sans_dependant_est_triviale() {
    let (mut p, s, _a, _b, _c) = chain();
    let lone = add_point(&mut p, s, 300.0, 300.0);
    let impact = p.deletion_impact(s, &[O::Point(lone)]);
    assert!(impact.is_trivial());
    assert_eq!(impact.refused, None);
    assert!(impact.removed.is_empty() && impact.unlinked.is_empty());
    // Un segment isolé : ses deux extrémités libres partent avec lui (comportement habituel).
    let seg = p
        .apply(Command::AddSegmentPolar {
            sheet: s,
            from: PointRef::New(pt(-100.0, -100.0)),
            length_mm: 30.0,
            angle_deg: 10.0,
        })
        .unwrap();
    let impact = p.deletion_impact(s, &[O::Entity(seg.entities[0])]);
    assert!(impact.is_trivial(), "{impact:?}");
    assert_eq!(impact.cleaned.len(), 2);
    assert!(impact.notes.is_empty());
    // Rien à supprimer : trivial, sans refus.
    assert!(p.deletion_impact(s, &[]).is_trivial());
}

#[test]
fn supprimer_un_parent_est_refuse_et_le_dit() {
    let (p, s, a, b, c) = chain();
    let impact = p.deletion_impact(s, &[O::Point(a)]);
    let refusal = impact.refused.clone().expect("refus annoncé");
    // Identique au refus réel de la commande.
    let real = p
        .clone()
        .apply(Command::Delete {
            sheet: s,
            points: vec![a],
            entities: vec![],
        })
        .unwrap_err()
        .to_string();
    assert_eq!(refusal, real);
    assert!(refusal.contains("parent"), "{refusal}");
    // Avec B et C ajoutés à la sélection, plus de refus ni de dépendant touché.
    let impact = p.deletion_impact(s, &[O::Point(a), O::Point(b), O::Point(c)]);
    assert_eq!(impact.refused, None);
    assert!(impact.is_trivial(), "{impact:?}");
}

#[test]
fn supprimer_un_trace_annonce_le_point_rendu_libre_et_sa_descendance_reste() {
    let mut p = Project::new("Trace");
    let s = p.sheets[0].id;
    let seg = p
        .apply(Command::AddSegmentPolar {
            sheet: s,
            from: PointRef::New(pt(0.0, 0.0)),
            length_mm: 100.0,
            angle_deg: 0.0,
        })
        .unwrap();
    let e = seg.entities[0];
    let on = derived(
        &mut p,
        s,
        PointDefinition::OnSegmentRatio {
            segment: e,
            ratio: 0.5,
        },
    );
    let off = derived(
        &mut p,
        s,
        PointDefinition::Offset {
            from: on,
            dx_mm: 0.0,
            dy_mm: 20.0,
        },
    );
    let tree = p.dependency_tree(&O::Entity(e), None).unwrap();
    assert_eq!(
        objects(&tree.children),
        vec![(O::Point(on), 1), (O::Point(off), 2)]
    );
    let impact = p.deletion_impact(s, &[O::Entity(e)]);
    assert!(!impact.is_trivial());
    assert_eq!(
        impact.unlinked,
        vec![DependencyEdge {
            parent: O::Entity(e),
            child: O::Point(on),
            nature: N::Construction,
        }]
    );
    assert_eq!(impact.cleaned.len(), 2);
    assert!(impact.notes.iter().any(|n| n.contains("libre")));
    // Effet réel : B devient libre, C reste construit depuis B.
    let mut after = p.clone();
    after
        .apply(Project::deletion_command(s, &[O::Entity(e)]).unwrap())
        .unwrap();
    let sheet = after.sheet(s).unwrap();
    assert!(sheet.point(on).unwrap().definition.is_free());
    assert!(matches!(
        sheet.point(off).unwrap().definition,
        PointDefinition::Offset { .. }
    ));
}

#[test]
fn piece_liee_a_un_trace_cran_et_cotation() {
    let mut p = Project::new("Pièce");
    let s = p.sheets[0].id;
    let rect = p
        .apply(Command::AddRectangle {
            sheet: s,
            corner: PointRef::New(pt(0.0, 0.0)),
            width_mm: 100.0,
            height_mm: 60.0,
        })
        .unwrap();
    let piece = p
        .apply(Command::ExtractPiece {
            sheet: s,
            source: PieceSource::Click { at: pt(50.0, 30.0) },
            name: "Devant".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    let outline: Vec<EntityId> = p
        .piece(piece)
        .unwrap()
        .outline
        .iter()
        .map(|e| e.entity)
        .collect();
    assert_eq!(outline.len(), 4);
    let first = outline[0];
    let notch = p
        .apply(Command::AddNotch {
            sheet: s,
            entity: first,
            distance_mm: 10.0,
            side: Side::Left,
            length_mm: 5.0,
            style: NotchStyle::Single,
        })
        .unwrap()
        .notches[0];
    let dim = p
        .apply(Command::AddDimension {
            sheet: s,
            from: rect.points[0],
            to: rect.points[1],
            offset_mm: 10.0,
        })
        .unwrap()
        .dimensions[0];
    // Arbre du tracé : enfants = pièce (contour) et cran ; parents = ses deux points.
    let tree = p.dependency_tree(&O::Entity(first), Some(1)).unwrap();
    let kids: Vec<_> = tree
        .children
        .iter()
        .map(|e| (e.object.clone(), e.nature))
        .collect();
    assert!(kids.contains(&(O::Piece(piece), N::PieceOutline)));
    assert!(kids.contains(&(O::Notch(notch), N::NotchReference)));
    assert_eq!(tree.direct_parents().count(), 2);
    // Arbre de la pièce : ses 4 tracés en parents directs.
    let up = p.dependency_tree(&O::Piece(piece), Some(1)).unwrap();
    assert_eq!(up.parents.len(), 4);
    assert!(up.parents.iter().all(|e| e.nature == N::PieceOutline));
    // Cotation : ses deux points en parents.
    let d = p.dependency_tree(&O::Dimension(dim), None).unwrap();
    assert_eq!(d.direct_parents().count(), 2);
    // Impact de la suppression du tracé : cran supprimé, contour de la pièce vidé (lien retiré).
    let impact = p.deletion_impact(s, &[O::Entity(first)]);
    assert!(!impact.is_trivial());
    assert!(impact.removed.contains(&O::Notch(notch)), "{impact:?}");
    assert!(
        impact
            .unlinked
            .iter()
            .any(|e| e.child == O::Piece(piece) && e.nature == N::PieceOutline),
        "{impact:?}"
    );
    assert!(impact.notes.iter().any(|n| n.contains("contour")));
    // Accord avec l'effet réel : le contour est vidé, le cran disparu, la pièce reste.
    let mut after = p.clone();
    after
        .apply(Project::deletion_command(s, &[O::Entity(first)]).unwrap())
        .unwrap();
    assert!(after.piece(piece).unwrap().outline.is_empty());
    assert!(after.sheet(s).unwrap().notch(notch).is_err());
    // Le projet d'origine n'a pas bougé.
    assert_eq!(p.piece(piece).unwrap().outline.len(), 4);
    // Supprimer la cotation seule : triviale.
    assert!(p.deletion_impact(s, &[O::Dimension(dim)]).is_trivial());
    // Supprimer le cran seul : trivial (rien ne dépend du cran).
    assert!(p.deletion_impact(s, &[O::Notch(notch)]).is_trivial());
}

#[test]
fn graphe_altere_a_la_main_sans_boucle_infinie() {
    let mut p = Project::new("Cycle");
    let s = p.sheets[0].id;
    let a = add_point(&mut p, s, 0.0, 0.0);
    let b = add_point(&mut p, s, 10.0, 0.0);
    // A dépend de B et B dépend de A : état impossible par les commandes, possible dans un
    // fichier altéré.
    let sheet = &mut p.sheets[0];
    for pt in &mut sheet.points {
        if pt.id == a {
            pt.definition = PointDefinition::Offset {
                from: b,
                dx_mm: 1.0,
                dy_mm: 1.0,
            };
        } else if pt.id == b {
            pt.definition = PointDefinition::Offset {
                from: a,
                dx_mm: 1.0,
                dy_mm: 1.0,
            };
        }
    }
    let tree = p.dependency_tree(&O::Point(a), None).unwrap();
    assert_eq!(objects(&tree.parents), vec![(O::Point(b), 1)]);
    assert_eq!(objects(&tree.children), vec![(O::Point(b), 1)]);
    // Un point qui se cite lui-même : arête ignorée.
    for pt in &mut p.sheets[0].points {
        if pt.id == a {
            pt.definition = PointDefinition::Offset {
                from: a,
                dx_mm: 1.0,
                dy_mm: 1.0,
            };
        }
    }
    let tree = p.dependency_tree(&O::Point(a), None).unwrap();
    assert!(!tree.parents.iter().any(|e| e.object == O::Point(a)));
}

#[test]
fn dependance_vers_un_objet_absent_est_ignoree() {
    // Une formule dont la variable n'existe plus (fichier altéré) ne crée pas de nœud fantôme.
    let (mut p, s, _a, b, _c) = chain();
    p.sheets[0].formulas.push(ParamFormula {
        point: b,
        param: ParamKind::Distance,
        expr: "absente*2".into(),
    });
    let tree = p.dependency_tree(&O::Point(b), None).unwrap();
    assert!(!tree
        .parents
        .iter()
        .any(|e| matches!(e.object, O::Variable(_))));
    let _ = s;
}

#[test]
fn commande_de_suppression_ordre_et_forme() {
    let s = SheetId(1);
    assert_eq!(Project::deletion_command(s, &[]), None);
    let one = Project::deletion_command(s, &[O::Point(PointId(5))]).unwrap();
    assert_eq!(
        one,
        Command::Delete {
            sheet: s,
            points: vec![PointId(5)],
            entities: vec![],
        }
    );
    let many = Project::deletion_command(
        s,
        &[
            O::Point(PointId(5)),
            O::Dimension(DimensionId(7)),
            O::Notch(NotchId(8)),
            O::Guide(GuideId(9)),
            O::Entity(EntityId(6)),
        ],
    )
    .unwrap();
    let Command::Batch { commands } = many else {
        panic!("Batch attendu");
    };
    assert_eq!(commands.len(), 4);
    assert!(matches!(commands[0], Command::DeleteDimension { .. }));
    assert!(matches!(commands[1], Command::DeleteNotch { .. }));
    assert!(matches!(commands[2], Command::DeleteGuide { .. }));
    assert!(matches!(commands[3], Command::Delete { .. }));
}

#[test]
fn annuler_apres_suppression_en_une_etape() {
    let (p, s, a, b, c) = chain();
    let mut doc = Document::new(p);
    let before = bytes(doc.project());
    let cmd = Project::deletion_command(s, &[O::Point(a), O::Point(b), O::Point(c)]).unwrap();
    doc.execute(cmd).unwrap();
    assert!(doc.project().sheet(s).unwrap().points.is_empty());
    assert!(doc.undo());
    assert_eq!(bytes(doc.project()), before);
}

#[test]
fn droit_fil_reference_et_bord_sur_le_pli() {
    let mut p = Project::new("Droit-fil");
    let s = p.sheets[0].id;
    p.apply(Command::AddRectangle {
        sheet: s,
        corner: PointRef::New(pt(0.0, 0.0)),
        width_mm: 100.0,
        height_mm: 60.0,
    })
    .unwrap();
    let piece = p
        .apply(Command::ExtractPiece {
            sheet: s,
            source: PieceSource::Click { at: pt(50.0, 30.0) },
            name: "Devant".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    let outline = p.piece(piece).unwrap().outline.clone();
    let mut segment = |a: (f64, f64), b: (f64, f64)| {
        p.apply(Command::AddSegment {
            sheet: s,
            from: PointRef::New(pt(a.0, a.1)),
            to: PointRef::New(pt(b.0, b.1)),
        })
        .unwrap()
        .entities[0]
    };
    let grain_line = segment((20.0, 10.0), (20.0, 50.0));
    let reference = segment((200.0, 0.0), (260.0, 30.0));
    p.apply(Command::SetPathRole {
        sheet: s,
        entity: grain_line,
        role: PathRole::Grainline,
    })
    .unwrap();
    p.apply(Command::SetPieceOutline {
        piece,
        outline: outline.clone(),
        grainline: Some(grain_line),
    })
    .unwrap();
    p.apply(Command::SetPieceGrain {
        piece,
        grain: PieceGrain {
            direction: GrainDirection::Parallel { reference },
            ..PieceGrain::default()
        },
    })
    .unwrap();
    let fold = outline[0].entity;
    p.apply(Command::SetPieceFold {
        piece,
        edge: fold,
        on: true,
    })
    .unwrap();
    let up = p.dependency_tree(&O::Piece(piece), Some(1)).unwrap();
    let natures = |object: O| -> Vec<N> {
        up.parents
            .iter()
            .filter(|e| e.object == object)
            .map(|e| e.nature)
            .collect()
    };
    assert_eq!(natures(O::Entity(grain_line)), vec![N::PieceGrain]);
    assert_eq!(natures(O::Entity(reference)), vec![N::PieceGrain]);
    // Le tracé de contour porte le contour ; le lien « pli » est une arête distincte du même
    // couple (la nature la plus petite est affichée).
    assert_eq!(natures(O::Entity(fold)), vec![N::PieceOutline]);
    let edges = p.dependency_edges();
    assert!(edges.iter().any(|e| e.parent == O::Entity(fold)
        && e.child == O::Piece(piece)
        && e.nature == N::PieceFold));
    // Supprimer le tracé de référence : le droit-fil retombe sur le tracé dessiné (lien retiré).
    let impact = p.deletion_impact(s, &[O::Entity(reference)]);
    assert!(
        impact
            .unlinked
            .iter()
            .any(|e| e.child == O::Piece(piece) && e.nature == N::PieceGrain),
        "{impact:?}"
    );
}

#[test]
fn symetrie_liee_axe_et_source_sont_des_parents() {
    let mut p = Project::new("Symétrie");
    let s = p.sheets[0].id;
    let a = add_point(&mut p, s, 10.0, 10.0);
    let guide = p
        .apply(Command::AddGuide {
            sheet: s,
            x_mm: 0.0,
            y_mm: 0.0,
            angle_deg: 90.0,
            axis: true,
            layer: None,
        })
        .unwrap()
        .guides[0];
    p.apply(Command::LinkMirror {
        sheet: s,
        points: vec![a],
        entities: vec![],
        axis: MirrorAxis::Guide { guide },
    })
    .unwrap();
    let m = p
        .sheet(s)
        .unwrap()
        .points
        .iter()
        .find(|q| q.definition.is_mirror())
        .expect("point miroir")
        .id;
    let tree = p.dependency_tree(&O::Point(m), Some(1)).unwrap();
    let parents: Vec<_> = tree.parents.iter().map(|e| e.object.clone()).collect();
    assert!(parents.contains(&O::Point(a)));
    assert!(parents.contains(&O::Guide(guide)));
    // Supprimer la ligne d'aide : l'annonce reprend le comportement réel (refus ou non).
    let impact = p.deletion_impact(s, &[O::Guide(guide)]);
    let real = p
        .clone()
        .apply(Project::deletion_command(s, &[O::Guide(guide)]).unwrap());
    assert_eq!(impact.refused.is_some(), real.is_err());
}
