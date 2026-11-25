//! US-02.10 — décalage de contour : `Command::OffsetPaths`, chaînes, côté, refus.

use pf_core::contour::{contour_from_entities, measure_contour};
use pf_core::offset::{detect_chain, offset_preview, offset_side, ordered_chain};
use pf_core::persist;
use pf_core::*;
use pf_geometry::{CornerStyle, PointMm};
use std::f64::consts::{PI, TAU};

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

fn setup() -> (Project, SheetId) {
    let p = Project::new("Décalage");
    let s = p.sheets[0].id;
    (p, s)
}

fn snapshot(p: &Project) -> String {
    serde_json::to_string(p).unwrap()
}

fn polygon(p: &mut Project, sheet: SheetId, v: &[(f64, f64)]) -> Vec<EntityId> {
    let n = v.len();
    (0..n)
        .map(|i| {
            p.apply(Command::AddSegment {
                sheet,
                from: PointRef::New(pt(v[i].0, v[i].1)),
                to: PointRef::New(pt(v[(i + 1) % n].0, v[(i + 1) % n].1)),
            })
            .unwrap()
            .entities[0]
        })
        .collect()
}

fn rectangle(p: &mut Project, sheet: SheetId, w: f64, h: f64) -> Vec<EntityId> {
    polygon(p, sheet, &[(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)])
}

fn offset(sheet: SheetId, paths: &[EntityId], distance_mm: f64, corner: CornerStyle) -> Command {
    Command::OffsetPaths {
        sheet,
        paths: paths.to_vec(),
        distance_mm,
        corner,
        role: PathRole::Construction,
    }
}

fn perimeter_and_area(p: &Project, sheet: SheetId, entities: &[EntityId]) -> (f64, f64) {
    let s = p.sheet(sheet).unwrap();
    let c = contour_from_entities(s, entities).expect("contour fermé");
    let m = measure_contour(s, &c).unwrap();
    (m.perimeter_mm, m.area_mm2)
}

fn dense(p: &Project, sheet: SheetId, id: EntityId, n: usize) -> Vec<PointMm> {
    p.sheet(sheet).unwrap().shape(id).unwrap().sample(n)
}

fn dist_poly(q: PointMm, poly: &[PointMm]) -> f64 {
    let mut best = f64::INFINITY;
    for w in poly.windows(2) {
        let (a, b) = (w[0], w[1]);
        let (ab, aq) = (b - a, q - a);
        let l2 = ab.x * ab.x + ab.y * ab.y;
        let t = if l2 > 0.0 {
            ((aq.x * ab.x + aq.y * ab.y) / l2).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let f = pt(a.x + ab.x * t, a.y + ab.y * t);
        best = best.min(((q.x - f.x).powi(2) + (q.y - f.y).powi(2)).sqrt());
    }
    best
}

/// Écart maximal entre la distance au tracé d'origine et `|d|`, sur tous les
/// points échantillonnés du résultat (distance minimale calculée ici).
fn worst_error(
    p: &Project,
    sheet: SheetId,
    original: &[EntityId],
    result: &[EntityId],
    d: f64,
) -> f64 {
    let reference: Vec<Vec<PointMm>> = original.iter().map(|&e| dense(p, sheet, e, 3000)).collect();
    let mut worst: f64 = 0.0;
    for &r in result {
        for q in dense(p, sheet, r, 150) {
            let dist = reference
                .iter()
                .map(|poly| dist_poly(q, poly))
                .fold(f64::INFINITY, f64::min);
            worst = worst.max((dist - d.abs()).abs());
        }
    }
    worst
}

// ------------------------------------------------------------------ CA 1 : exactitude

#[test]
fn rectangle_decale_est_un_rectangle_exact_et_independant() {
    let (mut p, s) = setup();
    let rect = rectangle(&mut p, s, 100.0, 50.0);
    let before = snapshot(&p);
    let (n_points, n_entities) = (
        p.sheet(s).unwrap().points.len(),
        p.sheet(s).unwrap().entities.len(),
    );
    let o = p.apply(offset(s, &rect, -5.0, CornerStyle::Miter)).unwrap();
    assert_eq!(o.entities.len(), 4);
    assert_eq!(o.points.len(), 4, "quatre sommets partagés");
    let (per, area) = perimeter_and_area(&p, s, &o.entities);
    assert!((per - 340.0).abs() < 1e-9);
    assert!((area - 110.0 * 60.0).abs() < 1e-6);
    // Les sommets sont exacts.
    let sheet = p.sheet(s).unwrap();
    for e in [(-5.0, -5.0), (105.0, -5.0), (105.0, 55.0), (-5.0, 55.0)] {
        assert!(
            o.points.iter().any(|&id| pf_geometry::distance_mm(
                sheet.position(id).unwrap(),
                pt(e.0, e.1)
            ) < 1e-9),
            "sommet {e:?} absent"
        );
    }
    // Vers l'intérieur.
    let o2 = p.apply(offset(s, &rect, 5.0, CornerStyle::Miter)).unwrap();
    let (per, area) = perimeter_and_area(&p, s, &o2.entities);
    assert!((per - 260.0).abs() < 1e-9 && (area - 90.0 * 40.0).abs() < 1e-6);
    // L'original est intact et les nouveaux points sont distincts des anciens.
    assert_eq!(p.sheet(s).unwrap().entities.len(), n_entities + 8);
    assert_eq!(p.sheet(s).unwrap().points.len(), n_points + 8);
    let _ = before;
    for &e in &rect {
        assert!(p.sheet(s).unwrap().entity(e).is_ok());
    }
}

#[test]
fn coins_round_arcs_de_raccord_et_perimetre() {
    let (mut p, s) = setup();
    let rect = rectangle(&mut p, s, 100.0, 50.0);
    let o = p.apply(offset(s, &rect, -5.0, CornerStyle::Round)).unwrap();
    let sheet = p.sheet(s).unwrap();
    let arcs = o
        .entities
        .iter()
        .filter(|&&e| matches!(sheet.entity(e).unwrap().kind, EntityKind::Arc { .. }))
        .count();
    assert_eq!(arcs, 4);
    let (per, _) = perimeter_and_area(&p, s, &o.entities);
    assert!((per - (300.0 + TAU * 5.0)).abs() < 1e-9);
}

#[test]
fn coin_interieur_coupe_sans_boucle_et_concave_ferme() {
    let (mut p, s) = setup();
    // L anti-horaire.
    let l = polygon(
        &mut p,
        s,
        &[
            (0.0, 0.0),
            (60.0, 0.0),
            (60.0, 20.0),
            (20.0, 20.0),
            (20.0, 60.0),
            (0.0, 60.0),
        ],
    );
    for (d, expected) in [(-4.0, 240.0 + 32.0), (4.0, 240.0 - 32.0)] {
        let o = p.apply(offset(s, &l, d, CornerStyle::Miter)).unwrap();
        let (per, _) = perimeter_and_area(&p, s, &o.entities);
        assert!((per - expected).abs() < 1e-9, "d = {d} : {per}");
        let m = measure_contour(
            p.sheet(s).unwrap(),
            &contour_from_entities(p.sheet(s).unwrap(), &o.entities).unwrap(),
        )
        .unwrap();
        assert!(!m.self_intersecting, "aucune boucle parasite");
    }
}

#[test]
fn cercle_perimetre_exact_dedans_et_dehors() {
    let (mut p, s) = setup();
    let c = p
        .apply(Command::AddCircle {
            sheet: s,
            center: PointRef::New(pt(10.0, 10.0)),
            radius_mm: 30.0,
        })
        .unwrap()
        .entities[0];
    for (d, r) in [(5.0, 25.0), (-5.0, 35.0)] {
        let o = p.apply(offset(s, &[c], d, CornerStyle::Round)).unwrap();
        assert_eq!(o.entities.len(), 1);
        let (per, area) = perimeter_and_area(&p, s, &o.entities);
        assert!((per - TAU * r).abs() < 1e-9);
        assert!((area - PI * r * r).abs() < 1e-6);
    }
    // Le côté choisi à la souris : l'intérieur du cercle est à gauche.
    let sheet = p.sheet(s).unwrap();
    assert_eq!(offset_side(sheet, &[c], pt(12.0, 10.0)).unwrap(), 1.0);
    assert_eq!(offset_side(sheet, &[c], pt(60.0, 10.0)).unwrap(), -1.0);
}

/// Segment, courbe de Bézier, arc et courbe lisse bout à bout (chaîne ouverte).
fn mixed_chain(p: &mut Project, s: SheetId) -> Vec<EntityId> {
    let a = p
        .apply(Command::AddSegment {
            sheet: s,
            from: PointRef::New(pt(-30.0, 0.0)),
            to: PointRef::New(pt(0.0, 0.0)),
        })
        .unwrap();
    let b = a.points[1];
    let curve = p
        .apply(Command::AddCurve {
            sheet: s,
            start: PointRef::Existing(b),
            c1: pt(20.0, 25.0),
            c2: pt(50.0, -20.0),
            end: PointRef::New(pt(70.0, 10.0)),
        })
        .unwrap();
    let c = curve.points[0];
    let arc = p
        .apply(Command::AddArcTwoPointsRadius {
            sheet: s,
            start: PointRef::Existing(c),
            end: PointRef::New(pt(100.0, 40.0)),
            radius_mm: 40.0,
            side: pf_geometry::Side::Left,
            ccw: true,
        })
        .unwrap();
    let d = *arc
        .points
        .iter()
        .find(|&&id| {
            pf_geometry::distance_mm(p.sheet(s).unwrap().position(id).unwrap(), pt(100.0, 40.0))
                < 1e-9
        })
        .unwrap();
    let spline = p
        .apply(Command::AddSpline {
            sheet: s,
            points: vec![
                PointRef::Existing(d),
                PointRef::New(pt(115.0, 60.0)),
                PointRef::New(pt(140.0, 45.0)),
                PointRef::New(pt(160.0, 70.0)),
            ],
        })
        .unwrap();
    vec![
        a.entities[0],
        curve.entities[0],
        arc.entities[0],
        spline.entities[0],
    ]
}

#[test]
fn distance_echantillonnee_segment_courbe_arc_et_courbe_lisse() {
    let (mut p, s) = setup();
    let chain = mixed_chain(&mut p, s);
    for d in [4.0, -4.0] {
        let o = p.apply(offset(s, &chain, d, CornerStyle::Round)).unwrap();
        let worst = worst_error(&p, s, &chain, &o.entities, d);
        assert!(worst < 0.01, "d = {d} : écart {worst}");
    }
    // Une chaîne donnée dans le désordre est ordonnée seule.
    let shuffled = vec![chain[2], chain[0], chain[3], chain[1]];
    let o = p
        .apply(offset(s, &shuffled, 4.0, CornerStyle::Round))
        .unwrap();
    assert!(worst_error(&p, s, &chain, &o.entities, 4.0) < 0.01);
}

#[test]
fn auto_intersection_boucles_supprimees() {
    let (mut p, s) = setup();
    // Courbe de rayon de courbure ~ 8 mm au sommet, décalée de 12 mm côté concave.
    let c = p
        .apply(Command::AddCurve {
            sheet: s,
            start: PointRef::New(pt(0.0, 0.0)),
            c1: pt(0.0, 30.0),
            c2: pt(20.0, 30.0),
            end: PointRef::New(pt(20.0, 0.0)),
        })
        .unwrap()
        .entities[0];
    let mut ok = 0;
    for d in [12.0, -12.0, 9.0, -9.0] {
        if let Ok(o) = p.apply(offset(s, &[c], d, CornerStyle::Round)) {
            ok += 1;
            assert!(worst_error(&p, s, &[c], &o.entities, d) < 0.01, "d = {d}");
        }
    }
    assert!(ok >= 2);
}

// --------------------------------------------------------------- CA : rôle, calque, étape

#[test]
fn role_choisi_et_defaut_construction() {
    let (mut p, s) = setup();
    let rect = rectangle(&mut p, s, 40.0, 40.0);
    let o = p.apply(offset(s, &rect, -3.0, CornerStyle::Miter)).unwrap();
    let sheet = p.sheet(s).unwrap();
    assert!(o
        .entities
        .iter()
        .all(|&e| sheet.entity(e).unwrap().role == PathRole::Construction));
    let o = p
        .apply(Command::OffsetPaths {
            sheet: s,
            paths: rect.clone(),
            distance_mm: -3.0,
            corner: CornerStyle::Miter,
            role: PathRole::CuttingContour,
        })
        .unwrap();
    let sheet = p.sheet(s).unwrap();
    assert!(o
        .entities
        .iter()
        .all(|&e| sheet.entity(e).unwrap().role == PathRole::CuttingContour));
    // L'original garde son rôle.
    assert!(rect
        .iter()
        .all(|&e| sheet.entity(e).unwrap().role == PathRole::Construction));
}

#[test]
fn une_seule_etape_annulable() {
    let (p, s) = setup();
    let mut doc = Document::new(p);
    let rect: Vec<EntityId> = [(0.0, 0.0), (60.0, 0.0), (60.0, 40.0), (0.0, 40.0)]
        .iter()
        .enumerate()
        .map(|(i, _)| i)
        .map(|i| {
            let v = [(0.0, 0.0), (60.0, 0.0), (60.0, 40.0), (0.0, 40.0)];
            doc.execute(Command::AddSegment {
                sheet: s,
                from: PointRef::New(pt(v[i].0, v[i].1)),
                to: PointRef::New(pt(v[(i + 1) % 4].0, v[(i + 1) % 4].1)),
            })
            .unwrap()
            .entities[0]
        })
        .collect();
    let before = snapshot(doc.project());
    doc.execute(offset(s, &rect, -5.0, CornerStyle::Round))
        .unwrap();
    assert!(doc.project().sheet(s).unwrap().entities.len() > 4);
    assert!(doc.undo());
    assert_eq!(snapshot(doc.project()), before, "un seul Ctrl+Z suffit");
    assert!(doc.redo());
    assert!(doc.project().sheet(s).unwrap().entities.len() > 4);
}

#[test]
fn calque_verrouille_refuse_le_resultat_mais_pas_la_lecture() {
    let (mut p, s) = setup();
    let rect = rectangle(&mut p, s, 40.0, 40.0);
    let layer = p
        .apply(Command::AddLayer {
            sheet: s,
            name: "Coupe".into(),
        })
        .unwrap()
        .layers[0];
    // Résultat dans un calque verrouillé : refusé, sans trace.
    p.apply(Command::SetLayerLocked {
        sheet: s,
        layer,
        locked: true,
    })
    .unwrap();
    let before = snapshot(&p);
    let err = p
        .apply(Command::InLayer {
            sheet: s,
            layer,
            command: Box::new(offset(s, &rect, -3.0, CornerStyle::Miter)),
        })
        .expect_err("calque verrouillé");
    assert!(matches!(err, CoreError::LayerLocked(_)), "{err}");
    assert_eq!(snapshot(&p), before);
    // Déverrouillé : le résultat va dans le calque actif.
    p.apply(Command::SetLayerLocked {
        sheet: s,
        layer,
        locked: false,
    })
    .unwrap();
    let o = p
        .apply(Command::InLayer {
            sheet: s,
            layer,
            command: Box::new(offset(s, &rect, -3.0, CornerStyle::Miter)),
        })
        .unwrap();
    let sheet = p.sheet(s).unwrap();
    assert!(o
        .entities
        .iter()
        .all(|&e| sheet.entity(e).unwrap().layer == Some(layer)));
    // L'original dans un calque verrouillé peut être lu (décalé) : il n'est pas modifié.
    p.apply(Command::SetObjectsLayer {
        sheet: s,
        points: vec![],
        entities: rect.clone(),
        layer: Some(layer),
    })
    .unwrap();
    p.apply(Command::SetLayerLocked {
        sheet: s,
        layer,
        locked: true,
    })
    .unwrap();
    let o = p.apply(offset(s, &rect, -6.0, CornerStyle::Miter)).unwrap();
    assert_eq!(o.entities.len(), 4);
}

#[test]
fn le_decalage_n_est_pas_recalcule_si_l_original_change() {
    let (mut p, s) = setup();
    let rect = rectangle(&mut p, s, 40.0, 40.0);
    let o = p.apply(offset(s, &rect, -3.0, CornerStyle::Miter)).unwrap();
    let positions: Vec<PointMm> = o
        .points
        .iter()
        .map(|&id| p.sheet(s).unwrap().position(id).unwrap())
        .collect();
    // On déplace un sommet de l'original.
    let (a, _) = p.sheet(s).unwrap().entity_points(rect[0]).unwrap();
    p.apply(Command::MovePoint {
        sheet: s,
        point: a,
        to: pt(-10.0, -10.0),
    })
    .unwrap();
    let after: Vec<PointMm> = o
        .points
        .iter()
        .map(|&id| p.sheet(s).unwrap().position(id).unwrap())
        .collect();
    assert_eq!(positions, after);
    // Aucun point du résultat n'est une définition dérivée.
    let sheet = p.sheet(s).unwrap();
    assert!(o
        .points
        .iter()
        .all(|&id| sheet.point(id).unwrap().definition.is_free()));
}

#[test]
fn schema_3_sans_donnee_nouvelle_et_fichier_relu_identique() {
    let (mut p, s) = setup();
    let rect = rectangle(&mut p, s, 40.0, 40.0);
    p.apply(offset(s, &rect, -3.0, CornerStyle::Round)).unwrap();
    assert_eq!(p.schema_version, 6);
    let json = persist::to_json(&p).unwrap();
    assert!(
        !json.contains("offset"),
        "aucun champ de décalage persistant"
    );
    let back = persist::from_json(&json).unwrap();
    assert_eq!(back, p);
    assert_eq!(back.schema_version, 6);
}

// ------------------------------------------------------------------ refus sans trace

#[test]
fn refus_sans_trace() {
    let (mut p, s) = setup();
    let rect = rectangle(&mut p, s, 10.0, 10.0);
    // Deux segments non connexes.
    let far = p
        .apply(Command::AddSegment {
            sheet: s,
            from: PointRef::New(pt(100.0, 100.0)),
            to: PointRef::New(pt(120.0, 100.0)),
        })
        .unwrap()
        .entities;
    // Un T (embranchement).
    let t1 = p
        .apply(Command::AddSegment {
            sheet: s,
            from: PointRef::New(pt(300.0, 0.0)),
            to: PointRef::New(pt(310.0, 0.0)),
        })
        .unwrap();
    let mid = t1.points[1];
    let t2 = p
        .apply(Command::AddSegment {
            sheet: s,
            from: PointRef::Existing(mid),
            to: PointRef::New(pt(320.0, 0.0)),
        })
        .unwrap();
    let t3 = p
        .apply(Command::AddSegment {
            sheet: s,
            from: PointRef::Existing(mid),
            to: PointRef::New(pt(310.0, 10.0)),
        })
        .unwrap();
    let circle = p
        .apply(Command::AddCircle {
            sheet: s,
            center: PointRef::New(pt(-50.0, -50.0)),
            radius_mm: 5.0,
        })
        .unwrap()
        .entities[0];
    let cases: Vec<(&str, Command)> = vec![
        ("d = 0", offset(s, &rect, 0.0, CornerStyle::Miter)),
        ("NaN", offset(s, &rect, f64::NAN, CornerStyle::Miter)),
        (
            "infini",
            offset(s, &rect, f64::INFINITY, CornerStyle::Miter),
        ),
        ("énorme", offset(s, &rect, 1e12, CornerStyle::Miter)),
        (
            "énorme négatif",
            offset(s, &rect, -1e300, CornerStyle::Round),
        ),
        ("vide", offset(s, &[], 1.0, CornerStyle::Miter)),
        (
            "non connexe",
            offset(s, &[rect[0], far[0]], 1.0, CornerStyle::Miter),
        ),
        (
            "embranchement",
            offset(
                s,
                &[t1.entities[0], t2.entities[0], t3.entities[0]],
                1.0,
                CornerStyle::Miter,
            ),
        ),
        (
            "annule le petit contour",
            offset(s, &rect, 6.0, CornerStyle::Miter),
        ),
        (
            "annule le cercle",
            offset(s, &[circle], 5.0, CornerStyle::Round),
        ),
        (
            "cercle mélangé",
            offset(s, &[circle, rect[0]], 1.0, CornerStyle::Round),
        ),
        (
            "tracé absent",
            offset(s, &[EntityId(99_999)], 1.0, CornerStyle::Miter),
        ),
        (
            "feuille absente",
            offset(SheetId(99_999), &rect, 1.0, CornerStyle::Miter),
        ),
    ];
    for (what, cmd) in cases {
        let before = snapshot(&p);
        let err = p.apply(cmd).expect_err(what);
        assert!(!err.to_string().is_empty(), "{what}");
        assert_eq!(snapshot(&p), before, "{what} : le projet a changé");
    }
}

// ------------------------------------------------------------ chaîne, côté, aperçu

#[test]
fn chaine_detectee_par_extremites_partagees() {
    let (mut p, s) = setup();
    let chain = mixed_chain(&mut p, s);
    // Un segment isolé ailleurs, et un tracé qui se branche en B (embranchement).
    for &start in &chain {
        let mut found = detect_chain(p.sheet(s).unwrap(), start).unwrap();
        found.sort();
        let mut expected = chain.clone();
        expected.sort();
        assert_eq!(found, expected, "depuis {start:?}");
    }
    // Un embranchement à l'extrémité de la courbe arrête la détection.
    let b = p.sheet(s).unwrap().entity_points(chain[1]).unwrap().0;
    let branch = p
        .apply(Command::AddSegment {
            sheet: s,
            from: PointRef::Existing(b),
            to: PointRef::New(pt(0.0, -30.0)),
        })
        .unwrap()
        .entities[0];
    let found = detect_chain(p.sheet(s).unwrap(), chain[2]).unwrap();
    assert!(!found.contains(&chain[0]) && !found.contains(&branch));
    assert!(found.contains(&chain[1]) && found.contains(&chain[2]) && found.contains(&chain[3]));
    // Contour fermé : détecté en entier.
    let rect = rectangle(&mut p, s, 20.0, 20.0);
    let mut found = detect_chain(p.sheet(s).unwrap(), rect[2]).unwrap();
    found.sort();
    let mut e = rect.clone();
    e.sort();
    assert_eq!(found, e);
    assert!(ordered_chain(p.sheet(s).unwrap(), &rect).unwrap().closed);
}

#[test]
fn cote_suivant_la_souris() {
    let (mut p, s) = setup();
    let rect = rectangle(&mut p, s, 100.0, 50.0);
    let sheet = p.sheet(s).unwrap();
    // Intérieur : le côté gauche du sens anti-horaire.
    let inside = offset_side(sheet, &rect, pt(50.0, 25.0)).unwrap();
    let outside = offset_side(sheet, &rect, pt(150.0, 25.0)).unwrap();
    assert_eq!((inside, outside), (1.0, -1.0));
    // La distance signée mène bien du côté de la souris.
    let preview = offset_preview(sheet, &rect, 5.0 * inside, CornerStyle::Miter).unwrap();
    let total: f64 = preview
        .iter()
        .map(|t| match t {
            pf_geometry::Trace::Line(a, b) => pf_geometry::distance_mm(*a, *b),
            _ => 0.0,
        })
        .sum();
    assert!((total - 260.0).abs() < 1e-9);
    // Aperçu : le projet n'a pas changé.
    assert_eq!(p.sheet(s).unwrap().entities.len(), 4);
}

#[test]
fn chaine_ouverte_ordonnee_dans_le_sens_du_plus_petit_identifiant() {
    let (mut p, s) = setup();
    // Segments créés de droite à gauche puis relié : le sens est celui du premier.
    let a = p
        .apply(Command::AddSegment {
            sheet: s,
            from: PointRef::New(pt(0.0, 0.0)),
            to: PointRef::New(pt(10.0, 0.0)),
        })
        .unwrap();
    let b = p
        .apply(Command::AddSegment {
            sheet: s,
            from: PointRef::New(pt(20.0, 0.0)),
            to: PointRef::Existing(a.points[1]),
        })
        .unwrap();
    let chain = ordered_chain(p.sheet(s).unwrap(), &[b.entities[0], a.entities[0]]).unwrap();
    assert!(!chain.closed);
    assert_eq!(
        chain.edges,
        vec![(a.entities[0], false), (b.entities[0], true)]
    );
    // d > 0 = à gauche du sens (0,0) → (20,0) : au-dessus (y > 0).
    let o = p
        .apply(offset(
            s,
            &[a.entities[0], b.entities[0]],
            3.0,
            CornerStyle::Miter,
        ))
        .unwrap();
    let sheet = p.sheet(s).unwrap();
    for &pid in &o.points {
        assert!((sheet.position(pid).unwrap().y - 3.0).abs() < 1e-9);
    }
}
