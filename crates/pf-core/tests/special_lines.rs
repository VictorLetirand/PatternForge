//! US-06.8 — lignes spéciales : nouveaux rôles de tracé (surpiqûre, milieu devant / dos, lignes de
//! taille, de hanche, de poitrine, de parage, de collage) et **surpiqûre liée à un bord**, un
//! groupe de tests par critère d'acceptation :
//! CA1 rôles (attribution, fichier, relecture identique, rôle inconnu refusé),
//! CA2 surpiqûre liée (régénération dans la même étape, obsolescence, lien cassable),
//! CA3 mesure exacte (valeurs calculées à la main), CA4 refus sans trace, CA5 une étape annulable,
//! CA6 fichier (schéma 3, octets inchangés, relecture identique, fichiers forgés refusés),
//! CA7 cohérence (copie de feuille, séparer, déplier, dérivées, plan, calques, éléments internes).
//! Le rendu écran / export des rôles est mesuré dans `crates/pf-export/tests/lignes_speciales.rs`.

use pf_core::persist;
use pf_core::*;
use pf_geometry::{project_on_segment, PointMm, Transform};
use serde_json::{json, Value};
use std::path::PathBuf;

const FIXTURES: [&str; 4] = [
    "v1-exercice.pforge.json",
    "v1-grande-piece.pforge.json",
    "v2-piece-contour-droit-fil.pforge.json",
    "v2-piece-sans-contour.pforge.json",
];

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

fn near(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

fn bytes(p: &Project) -> String {
    persist::to_json(p).unwrap()
}

fn fixture(name: &str) -> Project {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let text = std::fs::read_to_string(dir.join(name)).unwrap();
    persist::from_json(&text).unwrap_or_else(|e| panic!("{name} : {e}"))
}

/// Refus : message d'erreur ; le projet est identique (valeur et octets).
fn refused(p: &mut Project, c: Command) -> String {
    let (before, text) = (p.clone(), bytes(p));
    let err = p.apply(c).expect_err("la commande aurait dû être refusée");
    assert_eq!(*p, before, "refus sans trace (valeur)");
    assert_eq!(text, bytes(p), "refus sans trace (octets)");
    assert!(!err.to_string().is_empty());
    err.to_string()
}

fn rect(p: &mut Project, s: SheetId, x: f64, y: f64, w: f64, h: f64) -> Outcome {
    p.apply(Command::AddRectangle {
        sheet: s,
        corner: PointRef::New(pt(x, y)),
        width_mm: w,
        height_mm: h,
    })
    .unwrap()
}

fn seg(p: &mut Project, s: SheetId, a: (f64, f64), b: (f64, f64)) -> Outcome {
    p.apply(Command::AddSegment {
        sheet: s,
        from: PointRef::New(pt(a.0, a.1)),
        to: PointRef::New(pt(b.0, b.1)),
    })
    .unwrap()
}

fn paths(entities: &[EntityId]) -> TopstitchSource {
    TopstitchSource::Paths {
        paths: entities.to_vec(),
        from: None,
    }
}

fn add_topstitch(
    p: &mut Project,
    s: SheetId,
    source: TopstitchSource,
    d: f64,
    side: TopstitchSide,
) -> Result<Outcome, CoreError> {
    p.apply(Command::AddTopstitch {
        sheet: s,
        source,
        distance_mm: d,
        side,
    })
}

/// Projet : une feuille, un rectangle `w × h` en (0, 0), sa surpiqûre liée `d` vers l'intérieur.
fn rect_with_topstitch(w: f64, h: f64, d: f64) -> (Project, SheetId, Vec<EntityId>, TopstitchId) {
    let mut p = Project::new("surpiqûre");
    let s = p.sheets[0].id;
    let r = rect(&mut p, s, 0.0, 0.0, w, h);
    let o = add_topstitch(&mut p, s, paths(&r.entities), d, TopstitchSide::Inside).unwrap();
    (p, s, r.entities, o.topstitches[0])
}

fn point_at(p: &Project, s: SheetId, x: f64, y: f64) -> PointId {
    let sh = p.sheet(s).unwrap();
    sh.points
        .iter()
        .find(|q| {
            let v = sh.position(q.id).unwrap();
            near(v.x, x, 1e-9) && near(v.y, y, 1e-9)
        })
        .unwrap_or_else(|| panic!("aucun point en ({x} ; {y})"))
        .id
}

fn move_point(
    p: &mut Project,
    s: SheetId,
    from: (f64, f64),
    to: (f64, f64),
) -> Result<Outcome, CoreError> {
    let point = point_at(p, s, from.0, from.1);
    p.apply(Command::MovePoint {
        sheet: s,
        point,
        to: pt(to.0, to.1),
    })
}

fn link(p: &Project, s: SheetId, t: TopstitchId) -> Topstitch {
    p.sheet(s).unwrap().topstitch(t).unwrap().clone()
}

/// Segments générés d'une surpiqûre (extrémités en mm), dans l'ordre de la chaîne.
fn generated_segments(p: &Project, s: SheetId, t: TopstitchId) -> Vec<(PointMm, PointMm)> {
    let sh = p.sheet(s).unwrap();
    link(p, s, t)
        .paths
        .iter()
        .map(|&e| sh.segment_ends(e).expect("segment"))
        .collect()
}

/// Boîte (xmin, ymin, xmax, ymax) des extrémités des segments.
fn bbox(segs: &[(PointMm, PointMm)]) -> (f64, f64, f64, f64) {
    let pts: Vec<PointMm> = segs.iter().flat_map(|(a, b)| [*a, *b]).collect();
    (
        pts.iter().map(|q| q.x).fold(f64::MAX, f64::min),
        pts.iter().map(|q| q.y).fold(f64::MAX, f64::min),
        pts.iter().map(|q| q.x).fold(f64::MIN, f64::max),
        pts.iter().map(|q| q.y).fold(f64::MIN, f64::max),
    )
}

/// Distance d'un point à une polyligne fermée de segments.
fn dist_to_segments(q: PointMm, segs: &[(PointMm, PointMm)]) -> f64 {
    segs.iter()
        .map(|(a, b)| project_on_segment(q, *a, *b).unwrap().1)
        .fold(f64::MAX, f64::min)
}

fn corners(segs: &[(PointMm, PointMm)]) -> Vec<(i64, i64)> {
    let mut c: Vec<(i64, i64)> = segs
        .iter()
        .map(|(a, _)| ((a.x * 1e6).round() as i64, (a.y * 1e6).round() as i64))
        .collect();
    c.sort();
    c
}

fn m(v: f64) -> i64 {
    (v * 1e6).round() as i64
}

const SPECIAL: [(PathRole, &str); 8] = [
    (PathRole::Topstitch, "topstitch"),
    (PathRole::CenterFront, "center_front"),
    (PathRole::CenterBack, "center_back"),
    (PathRole::WaistLine, "waist_line"),
    (PathRole::HipLine, "hip_line"),
    (PathRole::BustLine, "bust_line"),
    (PathRole::SkivingLine, "skiving_line"),
    (PathRole::GlueLine, "glue_line"),
];

// ----- CA1 : rôles ------------------------------------------------------------------------

#[test]
fn ca1_les_huit_roles_sont_attribuables_et_relus_a_l_identique() {
    for (role, key) in SPECIAL {
        let mut p = Project::new("rôles");
        let s = p.sheets[0].id;
        let e = seg(&mut p, s, (0.0, 0.0), (50.0, 0.0)).entities[0];
        p.apply(Command::SetPathRole {
            sheet: s,
            entity: e,
            role,
        })
        .unwrap();
        assert_eq!(p.sheet(s).unwrap().entity(e).unwrap().role, role);
        let text = bytes(&p);
        let v: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["schema_version"], 6, "schéma 4 (US-09.1)");
        assert_eq!(v["sheets"][0]["entities"][0]["role"], json!(key));
        let back = persist::from_json(&text).unwrap();
        assert_eq!(back, p, "{key}");
        assert_eq!(bytes(&back), text, "{key} : octets");
        assert!(role.is_special());
    }
}

#[test]
fn ca1_catalogue_des_roles_distincts_et_complets() {
    assert_eq!(PathRole::ALL.len(), 14);
    assert_eq!(PathRole::BASE.len() + PathRole::SPECIAL.len(), 14);
    let mut labels: Vec<&str> = PathRole::ALL.iter().map(|r| r.label_fr()).collect();
    labels.sort();
    labels.dedup();
    assert_eq!(labels.len(), 14, "libellés distincts");
    let mut keys: Vec<String> = PathRole::ALL
        .iter()
        .map(|r| serde_json::to_string(r).unwrap())
        .collect();
    keys.sort();
    keys.dedup();
    assert_eq!(keys.len(), 14, "valeurs écrites distinctes");
    for r in PathRole::BASE {
        assert!(!r.is_special());
        assert!(PathRole::ALL.contains(&r));
    }
    // Les six rôles historiques s'écrivent comme avant.
    assert_eq!(serde_json::to_string(&PathRole::Fold).unwrap(), "\"fold\"");
    assert_eq!(
        serde_json::to_string(&PathRole::CuttingContour).unwrap(),
        "\"cutting_contour\""
    );
    // Une ligne de surpiqûre est proposée comme élément interne « surpiqûre ».
    assert_eq!(
        InternalKind::default_for_role(PathRole::Topstitch),
        InternalKind::Topstitch
    );
    assert_eq!(
        InternalKind::default_for_role(PathRole::GlueLine),
        InternalKind::Other
    );
}

#[test]
fn ca1_un_role_inconnu_est_toujours_refuse() {
    let mut p = Project::new("rôles");
    let s = p.sheets[0].id;
    let e = seg(&mut p, s, (0.0, 0.0), (50.0, 0.0)).entities[0];
    let text = bytes(&p);
    assert!(text.contains("\"role\": \"construction\""));
    for bad in [
        "centre_devant",
        "TOPSTITCH",
        "center-front",
        "",
        "waist",
        "stitch_line",
    ] {
        let forged = text.replace(
            "\"role\": \"construction\"",
            &format!("\"role\": \"{bad}\""),
        );
        assert!(persist::from_json(&forged).is_err(), "rôle {bad:?} accepté");
    }
    // Un rôle d'un type inattendu aussi.
    let forged = text.replace("\"role\": \"construction\"", "\"role\": 3");
    assert!(persist::from_json(&forged).is_err());
    let _ = e;
}

#[test]
fn ca1_fichiers_sans_ligne_speciale_octets_inchanges() {
    for name in FIXTURES {
        let first = bytes(&fixture(name));
        assert_eq!(first, bytes(&persist::from_json(&first).unwrap()), "{name}");
        assert!(!first.contains("topstitch"), "{name}");
        for (_, key) in SPECIAL {
            assert!(!first.contains(key), "{name} : {key}");
        }
    }
    let examples = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let mut seen = 0;
    for entry in std::fs::read_dir(examples).unwrap() {
        let path = entry.unwrap().path();
        if !path.to_string_lossy().ends_with(".pforge.json") {
            continue;
        }
        let shipped = std::fs::read_to_string(&path)
            .unwrap()
            .replace("\r\n", "\n");
        assert_eq!(
            shipped,
            persist::to_json(&persist::from_json(&shipped).unwrap()).unwrap(),
            "{path:?}"
        );
        assert!(!shipped.contains("topstitch"), "{path:?}");
        seen += 1;
    }
    assert!(seen >= 1);
    // Un projet sans surpiqûre liée n'écrit pas la clé.
    let (mut p, s, _, t) = rect_with_topstitch(100.0, 60.0, 5.0);
    assert!(bytes(&p).contains("\"topstitches\""));
    p.apply(Command::BreakTopstitchLink {
        sheet: s,
        topstitch: t,
    })
    .unwrap();
    assert!(!bytes(&p).contains("\"topstitches\""));
}

#[test]
fn ca1_changement_de_role_une_etape_annulable_et_refus_sur_calque_verrouille() {
    let mut p = Project::new("rôles");
    let s = p.sheets[0].id;
    let e = seg(&mut p, s, (0.0, 0.0), (50.0, 0.0)).entities[0];
    let mut doc = Document::new(p);
    for (role, _) in SPECIAL {
        doc.execute(Command::SetPathRole {
            sheet: s,
            entity: e,
            role,
        })
        .unwrap();
    }
    // Une étape par changement, annulables une à une.
    for (role, _) in SPECIAL.iter().rev().skip(1) {
        assert!(doc.undo());
        assert_eq!(
            doc.project().sheet(s).unwrap().entity(e).unwrap().role,
            *role
        );
    }
    assert!(doc.undo());
    assert_eq!(
        doc.project().sheet(s).unwrap().entity(e).unwrap().role,
        PathRole::Construction
    );
    assert!(!doc.can_undo());
    // Même rôle : aucune étape.
    let steps = |d: &Document| {
        let mut d = d.clone();
        let mut n = 0;
        while d.undo() {
            n += 1;
        }
        n
    };
    let before = steps(&doc);
    doc.execute(Command::SetPathRole {
        sheet: s,
        entity: e,
        role: PathRole::Construction,
    })
    .unwrap();
    assert_eq!(steps(&doc), before);
    // Calque verrouillé : refus sans trace.
    let mut p = doc.project().clone();
    let layer = p
        .apply(Command::AddLayer {
            sheet: s,
            name: "Verrou".into(),
        })
        .unwrap()
        .layers[0];
    p.apply(Command::SetObjectsLayer {
        sheet: s,
        points: vec![],
        entities: vec![e],
        layer: Some(layer),
    })
    .unwrap();
    p.apply(Command::SetLayerLocked {
        sheet: s,
        layer,
        locked: true,
    })
    .unwrap();
    let msg = refused(
        &mut p,
        Command::SetPathRole {
            sheet: s,
            entity: e,
            role: PathRole::BustLine,
        },
    );
    assert!(msg.contains("verrouillé"), "{msg}");
    // Entité ou feuille inconnue : refus sans trace.
    refused(
        &mut p,
        Command::SetPathRole {
            sheet: s,
            entity: EntityId(99_999),
            role: PathRole::BustLine,
        },
    );
}

// ----- CA3 : mesure exacte ---------------------------------------------------------------

#[test]
fn ca3_rectangle_vers_l_interieur_est_un_rectangle_reduit_de_2d() {
    for (w, h, d) in [(100.0, 60.0, 5.0), (240.0, 150.0, 12.5), (80.0, 80.0, 0.5)] {
        let (p, s, _, t) = rect_with_topstitch(w, h, d);
        let segs = generated_segments(&p, s, t);
        assert_eq!(segs.len(), 4, "{w}×{h}");
        let (x0, y0, x1, y1) = bbox(&segs);
        assert!(near(x0, d, 1e-9) && near(y0, d, 1e-9), "{x0} {y0}");
        assert!(near(x1 - x0, w - 2.0 * d, 1e-9), "largeur {}", x1 - x0);
        assert!(near(y1 - y0, h - 2.0 * d, 1e-9), "hauteur {}", y1 - y0);
        assert_eq!(
            corners(&segs),
            {
                let mut c = vec![
                    (m(d), m(d)),
                    (m(w - d), m(d)),
                    (m(w - d), m(h - d)),
                    (m(d), m(h - d)),
                ];
                c.sort();
                c
            },
            "{w}×{h}"
        );
        // Aire et périmètre calculés à la main, par le contour des lignes générées.
        let sh = p.sheet(s).unwrap();
        let ids = link(&p, s, t).paths;
        let contour = contour::contour_from_entities(sh, &ids).unwrap();
        let ms = contour::measure_contour(sh, &contour).unwrap();
        assert!(near(ms.area_mm2, (w - 2.0 * d) * (h - 2.0 * d), 1e-6));
        assert!(near(
            ms.perimeter_mm,
            2.0 * ((w - 2.0 * d) + (h - 2.0 * d)),
            1e-6
        ));
        // Chaque ligne est parallèle à un côté, à la distance d de lui.
        let original = [
            (pt(0.0, 0.0), pt(w, 0.0)),
            (pt(w, 0.0), pt(w, h)),
            (pt(w, h), pt(0.0, h)),
            (pt(0.0, h), pt(0.0, 0.0)),
        ];
        for (a, b) in &segs {
            for q in [*a, *b, pt((a.x + b.x) / 2.0, (a.y + b.y) / 2.0)] {
                let dd = dist_to_segments(q, &original);
                assert!(near(dd, d, 1e-6), "distance {dd} au lieu de {d}");
            }
        }
        // Rôle et lien.
        let l = link(&p, s, t);
        assert_eq!(l.state(), TopstitchState::Current);
        assert!(l
            .paths
            .iter()
            .all(|&e| sh.entity(e).unwrap().role == PathRole::Topstitch));
    }
}

#[test]
fn ca3_rectangle_vers_l_exterieur_coins_en_pointe() {
    let mut p = Project::new("extérieur");
    let s = p.sheets[0].id;
    let r = rect(&mut p, s, 10.0, 20.0, 100.0, 60.0);
    let t = add_topstitch(&mut p, s, paths(&r.entities), 5.0, TopstitchSide::Outside)
        .unwrap()
        .topstitches[0];
    let segs = generated_segments(&p, s, t);
    assert_eq!(segs.len(), 4);
    let (x0, y0, x1, y1) = bbox(&segs);
    assert!(near(x0, 5.0, 1e-9) && near(y0, 15.0, 1e-9));
    assert!(near(x1, 115.0, 1e-9) && near(y1, 85.0, 1e-9));
    // Le résultat ne dépend pas du sens d'enregistrement des tracés : même rectangle, sens horaire.
    let mut q = Project::new("extérieur");
    let sq = q.sheets[0].id;
    let a = q
        .apply(Command::AddSegment {
            sheet: sq,
            from: PointRef::New(pt(10.0, 20.0)),
            to: PointRef::New(pt(10.0, 80.0)),
        })
        .unwrap();
    let b = q
        .apply(Command::AddSegment {
            sheet: sq,
            from: PointRef::Existing(a.points[1]),
            to: PointRef::New(pt(110.0, 80.0)),
        })
        .unwrap();
    let c = q
        .apply(Command::AddSegment {
            sheet: sq,
            from: PointRef::Existing(b.points[0]),
            to: PointRef::New(pt(110.0, 20.0)),
        })
        .unwrap();
    let d = q
        .apply(Command::AddSegment {
            sheet: sq,
            from: PointRef::Existing(c.points[0]),
            to: PointRef::Existing(a.points[0]),
        })
        .unwrap();
    let edges = [a.entities[0], b.entities[0], c.entities[0], d.entities[0]];
    let tq = add_topstitch(&mut q, sq, paths(&edges), 5.0, TopstitchSide::Outside)
        .unwrap()
        .topstitches[0];
    assert_eq!(corners(&generated_segments(&q, sq, tq)), corners(&segs));
    let ti = add_topstitch(&mut q, sq, paths(&edges), 5.0, TopstitchSide::Inside)
        .unwrap()
        .topstitches[0];
    let (x0, y0, x1, y1) = bbox(&generated_segments(&q, sq, ti));
    assert!(near(x0, 15.0, 1e-9) && near(y0, 25.0, 1e-9));
    assert!(near(x1, 105.0, 1e-9) && near(y1, 75.0, 1e-9));
}

#[test]
fn ca3_segment_oblique_est_a_la_distance_demandee_des_deux_cotes() {
    // Segment 3-4-5 : direction (0,6 ; 0,8), normale gauche (−0,8 ; 0,6).
    let mut p = Project::new("oblique");
    let s = p.sheets[0].id;
    let e = seg(&mut p, s, (10.0, 20.0), (40.0, 60.0)).entities[0];
    let left = add_topstitch(&mut p, s, paths(&[e]), 7.0, TopstitchSide::Left)
        .unwrap()
        .topstitches[0];
    let right = add_topstitch(&mut p, s, paths(&[e]), 7.0, TopstitchSide::Right)
        .unwrap()
        .topstitches[0];
    let l = generated_segments(&p, s, left);
    let r = generated_segments(&p, s, right);
    assert_eq!(l.len(), 1);
    assert!(near(l[0].0.x, 10.0 - 7.0 * 0.8, 1e-9) && near(l[0].0.y, 20.0 + 7.0 * 0.6, 1e-9));
    assert!(near(l[0].1.x, 40.0 - 7.0 * 0.8, 1e-9) && near(l[0].1.y, 60.0 + 7.0 * 0.6, 1e-9));
    assert!(near(r[0].0.x, 10.0 + 7.0 * 0.8, 1e-9) && near(r[0].0.y, 20.0 - 7.0 * 0.6, 1e-9));
    // Distance mesurée, indépendamment de la formule : 7 mm à 1e-6 près.
    let orig = [(pt(10.0, 20.0), pt(40.0, 60.0))];
    for (a, b) in l.iter().chain(r.iter()) {
        for q in [*a, *b, pt((a.x + b.x) / 2.0, (a.y + b.y) / 2.0)] {
            assert!(near(dist_to_segments(q, &orig), 7.0, 1e-6));
        }
    }
    // Le point de départ du bord est mémorisé : l'inversion d'un tracé ne déplace pas la ligne.
    let TopstitchSource::Paths { from, .. } = link(&p, s, left).source else {
        panic!("source de tracés");
    };
    assert!(from.is_some());
    let before = generated_segments(&p, s, left);
    p.apply(Command::ReversePath {
        sheet: s,
        entity: e,
    })
    .unwrap();
    let after = generated_segments(&p, s, left);
    assert!(near(before[0].0.x, after[0].0.x, 1e-9) && near(before[0].0.y, after[0].0.y, 1e-9));
    assert!(near(before[0].1.x, after[0].1.x, 1e-9) && near(before[0].1.y, after[0].1.y, 1e-9));
}

#[test]
fn ca3_arc_et_cercle_sont_concentriques_exacts() {
    let mut p = Project::new("arcs");
    let s = p.sheets[0].id;
    let c = p
        .apply(Command::AddCircle {
            sheet: s,
            center: PointRef::New(pt(50.0, 40.0)),
            radius_mm: 30.0,
        })
        .unwrap();
    let t = add_topstitch(&mut p, s, paths(&c.entities), 4.0, TopstitchSide::Inside)
        .unwrap()
        .topstitches[0];
    let sh = p.sheet(s).unwrap();
    let ids = link(&p, s, t).paths;
    assert_eq!(ids.len(), 1);
    let g = sh.circle_geometry(ids[0]).unwrap();
    assert!(near(g.circle.radius_mm, 26.0, 1e-9));
    assert!(near(g.circle.center.x, 50.0, 1e-9) && near(g.circle.center.y, 40.0, 1e-9));
    let out = add_topstitch(&mut p, s, paths(&c.entities), 4.0, TopstitchSide::Outside)
        .unwrap()
        .topstitches[0];
    let g = p
        .sheet(s)
        .unwrap()
        .circle_geometry(link(&p, s, out).paths[0])
        .unwrap();
    assert!(near(g.circle.radius_mm, 34.0, 1e-9));
    // Le cercle change de rayon : la surpiqûre suit.
    p.apply(Command::SetCircleRadius {
        sheet: s,
        entity: c.entities[0],
        radius_mm: 20.0,
    })
    .unwrap();
    let r = |t| {
        p.sheet(s)
            .unwrap()
            .circle_geometry(link(&p, s, t).paths[0])
            .unwrap()
            .circle
            .radius_mm
    };
    assert!(near(r(t), 16.0, 1e-9));
    assert!(near(r(out), 24.0, 1e-9));
    // Un arc ouvert : rayon r ± d, même centre.
    let mut q = Project::new("arc");
    let sq = q.sheets[0].id;
    let a = q
        .apply(Command::AddArc {
            sheet: sq,
            center: PointRef::New(pt(0.0, 0.0)),
            radius_mm: 50.0,
            start_deg: 0.0,
            end_deg: 90.0,
            ccw: true,
        })
        .unwrap();
    // Sens enregistré : de (50 ; 0) à (0 ; 50), anti-horaire ; la gauche est vers le centre.
    let ts = add_topstitch(&mut q, sq, paths(&a.entities), 6.0, TopstitchSide::Left)
        .unwrap()
        .topstitches[0];
    let arc = q
        .sheet(sq)
        .unwrap()
        .arc_geometry(link(&q, sq, ts).paths[0])
        .unwrap();
    assert!(near(arc.radius_mm, 44.0, 1e-9));
    assert!(near(arc.center.x, 0.0, 1e-9) && near(arc.center.y, 0.0, 1e-9));
    assert!(near(
        arc.length_mm(),
        44.0 * std::f64::consts::FRAC_PI_2,
        1e-9
    ));
}

#[test]
fn ca3_courbe_de_bezier_a_la_tolerance_du_projet() {
    let mut p = Project::new("bézier");
    let s = p.sheets[0].id;
    let c = p
        .apply(Command::AddCurve {
            sheet: s,
            start: PointRef::New(pt(0.0, 0.0)),
            c1: pt(20.0, 60.0),
            c2: pt(80.0, 60.0),
            end: PointRef::New(pt(100.0, 0.0)),
        })
        .unwrap();
    let source = c.entities[0];
    for (side, d) in [
        (TopstitchSide::Left, 6.0),
        (TopstitchSide::Right, 6.0),
        (TopstitchSide::Left, 2.5),
    ] {
        let t = add_topstitch(&mut p, s, paths(&[source]), d, side)
            .unwrap()
            .topstitches[0];
        let sh = p.sheet(s).unwrap();
        let orig = sh.shape(source).unwrap();
        let ids = link(&p, s, t).paths;
        assert!(!ids.is_empty());
        let mut worst = 0.0_f64;
        for id in ids {
            for q in sh.shape(id).unwrap().sample(200) {
                worst = worst.max((orig.distance_to_point(q) - d).abs());
            }
        }
        // Tolérance du décalage : 2e-5 mm d'ajustement, très en dessous des 0,01 mm du cahier.
        assert!(worst < 5e-3, "écart {worst} mm");
    }
}

#[test]
fn ca3_refus_propres_quand_le_decalage_degenere() {
    let mut p = Project::new("dégénéré");
    let s = p.sheets[0].id;
    let r = rect(&mut p, s, 0.0, 0.0, 100.0, 60.0);
    // Distance ≥ demi-largeur vers l'intérieur : la forme s'annule ou se réduit à un trait.
    for d in [30.0, 30.5, 45.0, 50.0, 120.0] {
        let msg = refused(
            &mut p,
            Command::AddTopstitch {
                sheet: s,
                source: paths(&r.entities),
                distance_mm: d,
                side: TopstitchSide::Inside,
            },
        );
        assert!(!msg.is_empty(), "d = {d}");
    }
    // Juste en dessous de la demi-hauteur : un rectangle étroit, accepté.
    let t = add_topstitch(&mut p, s, paths(&r.entities), 29.5, TopstitchSide::Inside)
        .unwrap()
        .topstitches[0];
    let (x0, y0, x1, y1) = bbox(&generated_segments(&p, s, t));
    assert!(near(x1 - x0, 100.0 - 59.0, 1e-9) && near(y1 - y0, 1.0, 1e-9));
    // Changer la distance vers une valeur dégénérée : refus, rien ne change.
    let msg = refused(
        &mut p,
        Command::SetTopstitch {
            sheet: s,
            topstitch: t,
            distance_mm: 31.0,
            side: TopstitchSide::Inside,
        },
    );
    assert!(!msg.is_empty());
    // Un segment seul n'a pas d'intérieur : « intérieur » est refusé.
    let e = seg(&mut p, s, (200.0, 0.0), (260.0, 0.0)).entities[0];
    refused(
        &mut p,
        Command::AddTopstitch {
            sheet: s,
            source: paths(&[e]),
            distance_mm: 3.0,
            side: TopstitchSide::Inside,
        },
    );
    // Un bord fermé n'a ni gauche ni droite.
    refused(
        &mut p,
        Command::AddTopstitch {
            sheet: s,
            source: paths(&r.entities),
            distance_mm: 3.0,
            side: TopstitchSide::Left,
        },
    );
}

// ----- CA2 : régénération ----------------------------------------------------------------

#[test]
fn ca2_deplacer_un_point_du_bord_regenere_dans_la_meme_etape() {
    let (p, s, _, t) = rect_with_topstitch(100.0, 60.0, 5.0);
    let ids_before = link(&p, s, t).paths;
    let mut doc = Document::new(p);
    let out = {
        let point = point_at(doc.project(), s, 100.0, 60.0);
        doc.execute(Command::MovePoint {
            sheet: s,
            point,
            to: pt(120.0, 70.0),
        })
        .unwrap()
    };
    assert_eq!(out.regenerated_topstitches, vec![t]);
    let p = doc.project();
    // Mêmes identifiants (topologie identique), géométrie recalculée.
    assert_eq!(link(p, s, t).paths, ids_before);
    let segs = generated_segments(p, s, t);
    // Calculé à la main : le côté droit va de (100 ; 0) à (120 ; 70) ; les autres côtés
    // restent des droites décalées de 5 mm vers l'intérieur.
    let original = [
        (pt(0.0, 0.0), pt(100.0, 0.0)),
        (pt(100.0, 0.0), pt(120.0, 70.0)),
        (pt(120.0, 70.0), pt(0.0, 60.0)),
        (pt(0.0, 60.0), pt(0.0, 0.0)),
    ];
    for (a, b) in &segs {
        for q in [*a, *b, pt((a.x + b.x) / 2.0, (a.y + b.y) / 2.0)] {
            assert!(
                near(dist_to_segments(q, &original), 5.0, 1e-6),
                "{q:?} à {}",
                dist_to_segments(q, &original)
            );
        }
    }
    // Une seule étape annule le déplacement ET la régénération.
    assert!(doc.undo());
    let back = generated_segments(doc.project(), s, t);
    assert_eq!(corners(&back), {
        let mut c = vec![
            (m(5.0), m(5.0)),
            (m(95.0), m(5.0)),
            (m(95.0), m(55.0)),
            (m(5.0), m(55.0)),
        ];
        c.sort();
        c
    });
    assert!(
        !doc.can_undo(),
        "une seule étape pour le déplacement et la régénération"
    );
    assert!(doc.redo());
    assert_eq!(link(doc.project(), s, t).paths, ids_before);
    assert_eq!(generated_segments(doc.project(), s, t), segs);
}

#[test]
fn ca5_la_creation_est_une_seule_etape_annulable() {
    let mut p = Project::new("création");
    let s = p.sheets[0].id;
    let r = rect(&mut p, s, 0.0, 0.0, 100.0, 60.0);
    let mut doc = Document::new(p);
    let out = doc
        .execute(Command::AddTopstitch {
            sheet: s,
            source: paths(&r.entities),
            distance_mm: 5.0,
            side: TopstitchSide::Inside,
        })
        .unwrap();
    let t = out.topstitches[0];
    assert_eq!(out.regenerated_topstitches, vec![t]);
    assert_eq!(out.entities.len(), 4);
    assert_eq!(doc.project().sheet(s).unwrap().entities.len(), 8);
    assert!(doc.undo());
    assert!(!doc.can_undo());
    let sh = doc.project().sheet(s).unwrap();
    assert!(sh.topstitches.is_empty());
    assert_eq!(sh.entities.len(), 4);
    assert_eq!(sh.points.len(), 4, "aucun point orphelin");
    assert!(doc.redo());
    assert_eq!(doc.project().sheet(s).unwrap().entities.len(), 8);
    // Changer la distance : une étape, mêmes identifiants, géométrie recalculée.
    let ids = link(doc.project(), s, t).paths;
    doc.execute(Command::SetTopstitch {
        sheet: s,
        topstitch: t,
        distance_mm: 10.0,
        side: TopstitchSide::Inside,
    })
    .unwrap();
    assert_eq!(link(doc.project(), s, t).paths, ids);
    let (x0, y0, x1, y1) = bbox(&generated_segments(doc.project(), s, t));
    assert!(near(x0, 10.0, 1e-9) && near(y0, 10.0, 1e-9));
    assert!(near(x1, 90.0, 1e-9) && near(y1, 50.0, 1e-9));
    assert!(doc.undo());
    let (x0, _, x1, _) = bbox(&generated_segments(doc.project(), s, t));
    assert!(near(x0, 5.0, 1e-9) && near(x1, 95.0, 1e-9));
    // Côté changé : intérieur → extérieur.
    doc.execute(Command::SetTopstitch {
        sheet: s,
        topstitch: t,
        distance_mm: 5.0,
        side: TopstitchSide::Outside,
    })
    .unwrap();
    let (x0, _, x1, _) = bbox(&generated_segments(doc.project(), s, t));
    assert!(near(x0, -5.0, 1e-9) && near(x1, 105.0, 1e-9));
    // Sans changement : aucune étape.
    let steps = |d: &Document| {
        let mut d = d.clone();
        let mut n = 0;
        while d.undo() {
            n += 1;
        }
        n
    };
    let n = steps(&doc);
    doc.execute(Command::SetTopstitch {
        sheet: s,
        topstitch: t,
        distance_mm: 5.0,
        side: TopstitchSide::Outside,
    })
    .unwrap();
    assert_eq!(steps(&doc), n);
}

#[test]
fn ca2_la_surpiqure_suit_le_meme_bord_que_si_elle_etait_creee_apres() {
    // Régénérer puis recréer donne la même géométrie, à 1e-9 près.
    let (mut p, s, _, t) = rect_with_topstitch(100.0, 60.0, 5.0);
    move_point(&mut p, s, (100.0, 60.0), (130.0, 80.0)).unwrap();
    move_point(&mut p, s, (0.0, 0.0), (-10.0, -5.0)).unwrap();
    let regenerated = generated_segments(&p, s, t);
    let edges: Vec<EntityId> = p
        .sheet(s)
        .unwrap()
        .entities
        .iter()
        .filter(|e| e.role == PathRole::Construction)
        .map(|e| e.id)
        .collect();
    p.apply(Command::BreakTopstitchLink {
        sheet: s,
        topstitch: t,
    })
    .unwrap();
    let fresh = add_topstitch(&mut p, s, paths(&edges), 5.0, TopstitchSide::Inside)
        .unwrap()
        .topstitches[0];
    let created = generated_segments(&p, s, fresh);
    assert_eq!(corners(&regenerated).len(), 4);
    let key = |segs: &[(PointMm, PointMm)]| {
        let mut v: Vec<(i64, i64)> = segs
            .iter()
            .map(|(a, _)| ((a.x * 1e5).round() as i64, (a.y * 1e5).round() as i64))
            .collect();
        v.sort();
        v
    };
    assert_eq!(key(&regenerated), key(&created));
}

#[test]
fn ca2_obsolete_plutot_que_commande_refusee_puis_retour_a_jour() {
    let (mut p, s, _, t) = rect_with_topstitch(100.0, 60.0, 20.0);
    let ok = generated_segments(&p, s, t);
    let (top, bottom) = (point_at(&p, s, 100.0, 60.0), point_at(&p, s, 100.0, 0.0));
    let narrow = Command::Batch {
        commands: vec![
            Command::MovePoint {
                sheet: s,
                point: bottom,
                to: pt(30.0, 0.0),
            },
            Command::MovePoint {
                sheet: s,
                point: top,
                to: pt(30.0, 60.0),
            },
        ],
    };
    // On rétrécit la largeur à 30 mm (< 2 × 20) : le décalage dégénère. La commande de la mère
    // **n'est pas refusée** ; la surpiqûre garde sa dernière géométrie et est signalée obsolète.
    let out = p.apply(narrow).unwrap();
    assert!(out.regenerated_topstitches.is_empty());
    match link(&p, s, t).state() {
        TopstitchState::Stale { reason } => assert!(!reason.is_empty()),
        TopstitchState::Current => panic!("la surpiqûre aurait dû être obsolète"),
    }
    assert!(
        out.notes.iter().any(|n| n.contains("surpiqûre")),
        "remarque attendue : {:?}",
        out.notes
    );
    assert_eq!(
        generated_segments(&p, s, t),
        ok,
        "dernière géométrie valide conservée"
    );
    // Le fichier garde l'état obsolète et le relit à l'identique.
    let text = bytes(&p);
    assert!(text.contains("\"stale\""));
    assert_eq!(persist::from_json(&text).unwrap(), p);
    assert_eq!(bytes(&persist::from_json(&text).unwrap()), text);
    // Un autre changement qui ne touche pas le bord ne change rien à l'état.
    let other = seg(&mut p, s, (300.0, 0.0), (320.0, 0.0));
    assert!(other.regenerated_topstitches.is_empty());
    assert!(link(&p, s, t).stale.is_some());
    // On rend la largeur : régénérée dans la même étape, l'obsolescence disparaît.
    let back = p
        .apply(Command::Batch {
            commands: vec![
                Command::MovePoint {
                    sheet: s,
                    point: bottom,
                    to: pt(100.0, 0.0),
                },
                Command::MovePoint {
                    sheet: s,
                    point: top,
                    to: pt(100.0, 60.0),
                },
            ],
        })
        .unwrap();
    assert_eq!(back.regenerated_topstitches, vec![t]);
    assert_eq!(link(&p, s, t).state(), TopstitchState::Current);
    assert_eq!(generated_segments(&p, s, t), ok);
}

#[test]
fn ca2_lien_cassable_les_lignes_deviennent_des_traces_libres() {
    let (p, s, edges, t) = rect_with_topstitch(100.0, 60.0, 5.0);
    let lines = link(&p, s, t).paths;
    let mut doc = Document::new(p);
    let out = doc
        .execute(Command::BreakTopstitchLink {
            sheet: s,
            topstitch: t,
        })
        .unwrap();
    assert!(
        out.notes.iter().any(|n| n.contains("libres")),
        "{:?}",
        out.notes
    );
    assert!(doc.project().sheet(s).unwrap().topstitches.is_empty());
    let sh = doc.project().sheet(s).unwrap();
    for &l in &lines {
        assert_eq!(sh.entity(l).unwrap().role, PathRole::Topstitch);
    }
    assert!(!bytes(doc.project()).contains("topstitches"));
    // Le bord bouge : la ligne libre ne suit plus.
    let before: Vec<_> = lines.iter().map(|&l| sh.segment_ends(l).unwrap()).collect();
    let point = point_at(doc.project(), s, 100.0, 60.0);
    doc.execute(Command::MovePoint {
        sheet: s,
        point,
        to: pt(150.0, 90.0),
    })
    .unwrap();
    let sh = doc.project().sheet(s).unwrap();
    let after: Vec<_> = lines.iter().map(|&l| sh.segment_ends(l).unwrap()).collect();
    assert_eq!(before, after);
    // Une ligne libre se modifie directement.
    let a = point_at(doc.project(), s, 5.0, 5.0);
    doc.execute(Command::MovePoint {
        sheet: s,
        point: a,
        to: pt(6.0, 6.0),
    })
    .unwrap();
    // Annuler remet le lien.
    assert!(doc.undo());
    assert!(doc.undo());
    assert!(doc.undo());
    assert_eq!(doc.project().sheet(s).unwrap().topstitches.len(), 1);
    let _ = edges;
    // Lien inconnu : refus sans trace.
    let mut q = doc.project().clone();
    refused(
        &mut q,
        Command::BreakTopstitchLink {
            sheet: s,
            topstitch: TopstitchId(99_999),
        },
    );
}

#[test]
fn ca2_une_ligne_generee_ne_se_modifie_pas_directement() {
    let (mut p, s, _, t) = rect_with_topstitch(100.0, 60.0, 5.0);
    let l = link(&p, s, t);
    let first = l.paths[0];
    let corner = point_at(&p, s, 5.0, 5.0);
    let msg = refused(
        &mut p,
        Command::MovePoint {
            sheet: s,
            point: corner,
            to: pt(8.0, 9.0),
        },
    );
    assert!(msg.contains("Cassez le lien"), "{msg}");
    let msg = refused(
        &mut p,
        Command::SetPathRole {
            sheet: s,
            entity: first,
            role: PathRole::Construction,
        },
    );
    assert!(!msg.is_empty());
    refused(
        &mut p,
        Command::SplitPath {
            sheet: s,
            entity: first,
            at: pt(50.0, 5.0),
        },
    );
    refused(
        &mut p,
        Command::ReversePath {
            sheet: s,
            entity: first,
        },
    );
    // Ni dessiner un bord sur une ligne générée.
    refused(
        &mut p,
        Command::AddTopstitch {
            sheet: s,
            source: paths(&[first]),
            distance_mm: 2.0,
            side: TopstitchSide::Left,
        },
    );
    // Ce qui n'est pas la géométrie reste modifiable : nom, calque.
    p.apply(Command::SetEntityName {
        sheet: s,
        entity: first,
        name: Some("piqûre du haut".into()),
    })
    .unwrap();
    // Tout déplacer d'un bloc (bord et ligne) est permis : la régénération écrit seule.
    let all: Vec<PointId> = p.sheet(s).unwrap().points.iter().map(|q| q.id).collect();
    let out = p
        .apply(Command::Transform {
            sheet: s,
            points: all,
            transform: Transform::Translate {
                dx_mm: 40.0,
                dy_mm: -10.0,
            },
        })
        .unwrap();
    assert_eq!(out.regenerated_topstitches, vec![t]);
    let (x0, y0, x1, y1) = bbox(&generated_segments(&p, s, t));
    assert!(near(x0, 45.0, 1e-9) && near(y0, -5.0, 1e-9));
    assert!(near(x1, 135.0, 1e-9) && near(y1, 45.0, 1e-9));
}

#[test]
fn ca2_supprimer_un_tracé_du_bord_ou_une_ligne_casse_le_lien_avec_remarque() {
    // Bord supprimé.
    let (mut p, s, edges, t) = rect_with_topstitch(100.0, 60.0, 5.0);
    let lines = link(&p, s, t).paths;
    let out = p
        .apply(Command::Delete {
            sheet: s,
            points: vec![],
            entities: vec![edges[0]],
        })
        .unwrap();
    assert!(
        out.notes.iter().any(|n| n.contains("surpiqûre")),
        "{:?}",
        out.notes
    );
    assert!(p.sheet(s).unwrap().topstitches.is_empty());
    for l in &lines {
        assert!(p.sheet(s).unwrap().entity(*l).is_ok(), "lignes conservées");
    }
    // Ligne générée supprimée.
    let (mut p, s, _, t) = rect_with_topstitch(100.0, 60.0, 5.0);
    let lines = link(&p, s, t).paths;
    let out = p
        .apply(Command::Delete {
            sheet: s,
            points: vec![],
            entities: vec![lines[0]],
        })
        .unwrap();
    assert!(out.notes.iter().any(|n| n.contains("surpiqûre")));
    assert!(p.sheet(s).unwrap().topstitches.is_empty());
    assert!(p.sheet(s).unwrap().entity(lines[1]).is_ok());
    p.validate().unwrap();
}

#[test]
fn ca2_couper_ou_fusionner_un_tracé_du_bord_garde_la_ligne_complete() {
    let mut p = Project::new("coupe");
    let s = p.sheets[0].id;
    let e = seg(&mut p, s, (0.0, 0.0), (100.0, 0.0)).entities[0];
    let t = add_topstitch(&mut p, s, paths(&[e]), 4.0, TopstitchSide::Left)
        .unwrap()
        .topstitches[0];
    let before = generated_segments(&p, s, t);
    p.apply(Command::SplitPath {
        sheet: s,
        entity: e,
        at: pt(40.0, 0.0),
    })
    .unwrap();
    let TopstitchSource::Paths { paths: src, .. } = link(&p, s, t).source else {
        panic!()
    };
    assert_eq!(src.len(), 2, "le second tronçon prolonge le bord");
    let (x0, y0, x1, y1) = bbox(&generated_segments(&p, s, t));
    assert!(near(x0, 0.0, 1e-9) && near(x1, 100.0, 1e-9));
    assert!(near(y0, 4.0, 1e-9) && near(y1, 4.0, 1e-9));
    assert!(near(before[0].0.y, 4.0, 1e-9));
    // Fusion : le bord revient à un seul tracé.
    let second = *src.iter().find(|&&x| x != e).unwrap();
    p.apply(Command::MergePaths {
        sheet: s,
        first: e,
        second,
    })
    .unwrap();
    let TopstitchSource::Paths { paths: src, .. } = link(&p, s, t).source else {
        panic!()
    };
    assert_eq!(src, vec![e]);
    let (x0, _, x1, y1) = bbox(&generated_segments(&p, s, t));
    assert!(near(x0, 0.0, 1e-9) && near(x1, 100.0, 1e-9) && near(y1, 4.0, 1e-9));
    p.validate().unwrap();
}

#[test]
fn ca2_surpiqure_le_long_du_contour_d_une_piece() {
    let mut p = Project::new("pièce");
    let s = p.sheets[0].id;
    let r = rect(&mut p, s, 0.0, 0.0, 200.0, 100.0);
    let piece = p
        .apply(Command::ExtractPiece {
            sheet: s,
            source: PieceSource::Paths {
                entities: r.entities.clone(),
            },
            name: "Devant".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    let t = add_topstitch(
        &mut p,
        s,
        TopstitchSource::Piece { piece },
        8.0,
        TopstitchSide::Inside,
    )
    .unwrap()
    .topstitches[0];
    let (x0, y0, x1, y1) = bbox(&generated_segments(&p, s, t));
    assert!(near(x0, 8.0, 1e-9) && near(y0, 8.0, 1e-9));
    assert!(near(x1, 192.0, 1e-9) && near(y1, 92.0, 1e-9));
    // Le contour de la pièce bouge : la surpiqûre suit.
    move_point(&mut p, s, (200.0, 100.0), (220.0, 110.0)).unwrap();
    let original = [
        (pt(0.0, 0.0), pt(200.0, 0.0)),
        (pt(200.0, 0.0), pt(220.0, 110.0)),
        (pt(220.0, 110.0), pt(0.0, 100.0)),
        (pt(0.0, 100.0), pt(0.0, 0.0)),
    ];
    for (a, b) in generated_segments(&p, s, t) {
        assert!(near(dist_to_segments(a, &original), 8.0, 1e-6));
        assert!(near(dist_to_segments(b, &original), 8.0, 1e-6));
    }
    // Pièce supprimée : le lien est cassé avec remarque, les lignes restent.
    let out = p.apply(Command::DeletePiece { piece }).unwrap();
    assert!(
        out.notes.iter().any(|n| n.contains("surpiqûre")),
        "{:?}",
        out.notes
    );
    assert!(p.sheet(s).unwrap().topstitches.is_empty());
    p.validate().unwrap();
}

// ----- CA4 : refus sans trace ------------------------------------------------------------

#[test]
fn ca4_refus_sans_trace_pour_toute_entree_invalide() {
    let mut p = Project::new("refus");
    let s = p.sheets[0].id;
    let r = rect(&mut p, s, 0.0, 0.0, 100.0, 60.0);
    let open = seg(&mut p, s, (200.0, 0.0), (260.0, 0.0)).entities[0];
    let other = p
        .apply(Command::CreateSheet {
            name: "Autre".into(),
        })
        .unwrap()
        .sheet
        .unwrap();
    let foreign = seg(&mut p, other, (0.0, 0.0), (10.0, 0.0)).entities[0];
    let edge = |d: f64, side| Command::AddTopstitch {
        sheet: s,
        source: paths(&r.entities),
        distance_mm: d,
        side,
    };
    // Distance : nulle, négative, non finie, hors bornes.
    for d in [
        0.0,
        -3.0,
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        0.099_999,
        1_000.000_1,
        1.0e9,
    ] {
        refused(&mut p, edge(d, TopstitchSide::Inside));
    }
    // Les bornes elles-mêmes sont acceptées (0,1 mm : bord de 100 × 60 mm).
    let ok = add_topstitch(
        &mut p,
        s,
        paths(&r.entities),
        MIN_TOPSTITCH_MM,
        TopstitchSide::Inside,
    )
    .unwrap();
    assert!(near(
        bbox(&generated_segments(&p, s, ok.topstitches[0])).0,
        0.1,
        1e-12
    ));
    // Bord absent, vide, d'une autre feuille, pièce inconnue.
    for source in [
        paths(&[]),
        paths(&[EntityId(424_242)]),
        paths(&[foreign]),
        paths(&[r.entities[0], foreign]),
        TopstitchSource::Piece {
            piece: PieceId(424_242),
        },
    ] {
        refused(
            &mut p,
            Command::AddTopstitch {
                sheet: s,
                source,
                distance_mm: 3.0,
                side: TopstitchSide::Inside,
            },
        );
    }
    // Feuille inconnue.
    refused(
        &mut p,
        Command::AddTopstitch {
            sheet: SheetId(999_999),
            source: paths(&r.entities),
            distance_mm: 3.0,
            side: TopstitchSide::Inside,
        },
    );
    // Côté qui ne convient pas à la forme du bord.
    refused(&mut p, edge(3.0, TopstitchSide::Left));
    refused(&mut p, edge(3.0, TopstitchSide::Right));
    for side in [TopstitchSide::Inside, TopstitchSide::Outside] {
        refused(
            &mut p,
            Command::AddTopstitch {
                sheet: s,
                source: paths(&[open]),
                distance_mm: 3.0,
                side,
            },
        );
    }
    // Chaîne non continue : deux côtés opposés du rectangle.
    refused(
        &mut p,
        Command::AddTopstitch {
            sheet: s,
            source: paths(&[r.entities[0], r.entities[2]]),
            distance_mm: 3.0,
            side: TopstitchSide::Left,
        },
    );
    // Point de départ qui n'est pas une extrémité du bord.
    let stray = point_at(&p, s, 100.0, 60.0);
    refused(
        &mut p,
        Command::AddTopstitch {
            sheet: s,
            source: TopstitchSource::Paths {
                paths: vec![open],
                from: Some(stray),
            },
            distance_mm: 3.0,
            side: TopstitchSide::Left,
        },
    );
    // Pièce sans contour, et pièce d'une autre feuille.
    let empty = p
        .apply(Command::AddPiece {
            sheet: s,
            name: "Vide".into(),
            quantity: 1,
            material: None,
            mirrored_pair: false,
        })
        .unwrap()
        .pieces[0];
    refused(
        &mut p,
        Command::AddTopstitch {
            sheet: s,
            source: TopstitchSource::Piece { piece: empty },
            distance_mm: 3.0,
            side: TopstitchSide::Inside,
        },
    );
    let on_other = p
        .apply(Command::AddPiece {
            sheet: other,
            name: "Ailleurs".into(),
            quantity: 1,
            material: None,
            mirrored_pair: false,
        })
        .unwrap()
        .pieces[0];
    refused(
        &mut p,
        Command::AddTopstitch {
            sheet: s,
            source: TopstitchSource::Piece { piece: on_other },
            distance_mm: 3.0,
            side: TopstitchSide::Inside,
        },
    );
    // Doublon exact.
    refused(&mut p, edge(MIN_TOPSTITCH_MM, TopstitchSide::Inside));
    // Réglage d'une surpiqûre : lien inconnu, distance invalide, côté incompatible.
    let t = ok.topstitches[0];
    for d in [0.0, -1.0, f64::NAN, f64::INFINITY, 1001.0, 0.05] {
        refused(
            &mut p,
            Command::SetTopstitch {
                sheet: s,
                topstitch: t,
                distance_mm: d,
                side: TopstitchSide::Inside,
            },
        );
    }
    refused(
        &mut p,
        Command::SetTopstitch {
            sheet: s,
            topstitch: t,
            distance_mm: 2.0,
            side: TopstitchSide::Left,
        },
    );
    refused(
        &mut p,
        Command::SetTopstitch {
            sheet: s,
            topstitch: TopstitchId(777_777),
            distance_mm: 2.0,
            side: TopstitchSide::Inside,
        },
    );
    refused(
        &mut p,
        Command::SetTopstitch {
            sheet: other,
            topstitch: t,
            distance_mm: 2.0,
            side: TopstitchSide::Inside,
        },
    );
    refused(
        &mut p,
        Command::BreakTopstitchLink {
            sheet: other,
            topstitch: t,
        },
    );
    // Trop de surpiqûres sur une feuille.
    let mut q = Project::new("trop");
    let sq = q.sheets[0].id;
    let e = seg(&mut q, sq, (0.0, 0.0), (500.0, 0.0)).entities[0];
    for i in 0..MAX_TOPSTITCHES_PER_SHEET {
        add_topstitch(
            &mut q,
            sq,
            paths(&[e]),
            1.0 + i as f64 * 0.01,
            TopstitchSide::Left,
        )
        .unwrap();
    }
    refused(
        &mut q,
        Command::AddTopstitch {
            sheet: sq,
            source: paths(&[e]),
            distance_mm: 900.0,
            side: TopstitchSide::Left,
        },
    );
}

#[test]
fn ca4_calques_verrouilles() {
    let (mut p, s, _, t) = rect_with_topstitch(100.0, 60.0, 5.0);
    let lines = link(&p, s, t).paths;
    let layer = p
        .apply(Command::AddLayer {
            sheet: s,
            name: "Piqûres".into(),
        })
        .unwrap()
        .layers[0];
    p.apply(Command::SetObjectsLayer {
        sheet: s,
        points: vec![],
        entities: lines.clone(),
        layer: Some(layer),
    })
    .unwrap();
    p.apply(Command::SetLayerLocked {
        sheet: s,
        layer,
        locked: true,
    })
    .unwrap();
    // Réglage direct : refus (les lignes sont verrouillées, elles ne peuvent pas bouger).
    let msg = refused(
        &mut p,
        Command::SetTopstitch {
            sheet: s,
            topstitch: t,
            distance_mm: 8.0,
            side: TopstitchSide::Inside,
        },
    );
    assert!(msg.contains("verrouillé"), "{msg}");
    // Le bord bouge : la commande de la mère n'est pas refusée, la surpiqûre est obsolète
    // (elle ne peut pas être recalculée) et garde sa géométrie.
    let before = generated_segments(&p, s, t);
    let out = move_point(&mut p, s, (100.0, 60.0), (130.0, 60.0)).unwrap();
    assert!(out.regenerated_topstitches.is_empty());
    let stale = link(&p, s, t).stale.expect("obsolète");
    assert!(stale.contains("verrouillé"), "{stale}");
    assert_eq!(generated_segments(&p, s, t), before);
    // Supprimer une ligne verrouillée : refus.
    refused(
        &mut p,
        Command::Delete {
            sheet: s,
            points: vec![],
            entities: vec![lines[0]],
        },
    );
    // Une surpiqûre se crée sur un bord verrouillé (rien n'est modifié), mais le bord ne bouge pas.
    let mut q = Project::new("bord verrouillé");
    let sq = q.sheets[0].id;
    let r = rect(&mut q, sq, 0.0, 0.0, 100.0, 60.0);
    let l = q
        .apply(Command::AddLayer {
            sheet: sq,
            name: "Bord".into(),
        })
        .unwrap()
        .layers[0];
    q.apply(Command::SetObjectsLayer {
        sheet: sq,
        points: vec![],
        entities: r.entities.clone(),
        layer: Some(l),
    })
    .unwrap();
    q.apply(Command::SetLayerLocked {
        sheet: sq,
        layer: l,
        locked: true,
    })
    .unwrap();
    let tq = add_topstitch(&mut q, sq, paths(&r.entities), 5.0, TopstitchSide::Inside)
        .unwrap()
        .topstitches[0];
    let msg = refused(
        &mut q,
        Command::SetPathRole {
            sheet: sq,
            entity: r.entities[0],
            role: PathRole::CenterFront,
        },
    );
    assert!(msg.contains("verrouillé"), "{msg}");
    let corner = point_at(&q, sq, 100.0, 60.0);
    let msg = refused(
        &mut q,
        Command::MovePoint {
            sheet: sq,
            point: corner,
            to: pt(120.0, 60.0),
        },
    );
    assert!(msg.contains("verrouillé"), "{msg}");
    assert_eq!(link(&q, sq, tq).state(), TopstitchState::Current);
}

#[test]
fn ca4_point_de_depart_explicite_choisit_gauche_et_droite() {
    let mut p = Project::new("départ");
    let s = p.sheets[0].id;
    let o = seg(&mut p, s, (0.0, 0.0), (100.0, 0.0));
    let e = o.entities[0];
    let (a, b) = (o.points[0], o.points[1]);
    let from_a = add_topstitch(
        &mut p,
        s,
        TopstitchSource::Paths {
            paths: vec![e],
            from: Some(a),
        },
        4.0,
        TopstitchSide::Left,
    )
    .unwrap()
    .topstitches[0];
    let from_b = add_topstitch(
        &mut p,
        s,
        TopstitchSource::Paths {
            paths: vec![e],
            from: Some(b),
        },
        4.0,
        TopstitchSide::Left,
    )
    .unwrap()
    .topstitches[0];
    // De A vers B (vers +X) la gauche est +Y ; de B vers A la gauche est −Y.
    assert!(near(bbox(&generated_segments(&p, s, from_a)).1, 4.0, 1e-9));
    assert!(near(bbox(&generated_segments(&p, s, from_b)).1, -4.0, 1e-9));
    let kind = p.topstitch_chain_kind(s, &paths(&[e])).unwrap();
    assert_eq!(kind, ChainKind::Open { from: a });
    let r = rect(&mut p, s, 300.0, 0.0, 50.0, 50.0);
    assert_eq!(
        p.topstitch_chain_kind(s, &paths(&r.entities)).unwrap(),
        ChainKind::Closed
    );
    assert!(p
        .topstitch_chain_kind(s, &paths(&[EntityId(1_234_567)]))
        .is_err());
}

// ----- CA6 : fichier ----------------------------------------------------------------------

fn tamper(text: &str, edit: impl FnOnce(&mut Value)) -> String {
    let mut v: Value = serde_json::from_str(text).unwrap();
    edit(&mut v);
    serde_json::to_string_pretty(&v).unwrap() + "\n"
}

#[test]
fn ca6_serialisation_schema_3_relue_a_l_identique() {
    let (p, _, _, t) = rect_with_topstitch(100.0, 60.0, 5.0);
    let text = bytes(&p);
    let v: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["schema_version"], 6, "schéma 4 (US-09.1)");
    let l = &v["sheets"][0]["topstitches"][0];
    assert_eq!(l["id"], json!(t.0));
    assert_eq!(l["distance_mm"], json!(5.0));
    assert_eq!(l["side"], json!("inside"));
    assert_eq!(l["source"]["type"], json!("paths"));
    assert!(
        l["source"].get("from").is_none(),
        "bord fermé : pas de départ"
    );
    assert!(l.get("stale").is_none());
    assert_eq!(l["paths"].as_array().unwrap().len(), 4);
    let back = persist::from_json(&text).unwrap();
    assert_eq!(back, p);
    assert_eq!(bytes(&back), text);
    // Bord ouvert : le point de départ est écrit.
    let mut q = Project::new("ouvert");
    let sq = q.sheets[0].id;
    let e = seg(&mut q, sq, (0.0, 0.0), (10.0, 0.0)).entities[0];
    add_topstitch(&mut q, sq, paths(&[e]), 2.0, TopstitchSide::Right).unwrap();
    let text = bytes(&q);
    let v: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["sheets"][0]["topstitches"][0]["side"], json!("right"));
    assert!(v["sheets"][0]["topstitches"][0]["source"]["from"].is_u64());
    assert_eq!(bytes(&persist::from_json(&text).unwrap()), text);
    // Pièce : la source est une référence.
    let mut r = Project::new("pièce");
    let sr = r.sheets[0].id;
    let rr = rect(&mut r, sr, 0.0, 0.0, 80.0, 40.0);
    let piece = r
        .apply(Command::ExtractPiece {
            sheet: sr,
            source: PieceSource::Paths {
                entities: rr.entities,
            },
            name: "P".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    add_topstitch(
        &mut r,
        sr,
        TopstitchSource::Piece { piece },
        3.0,
        TopstitchSide::Outside,
    )
    .unwrap();
    let text = bytes(&r);
    let v: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        v["sheets"][0]["topstitches"][0]["source"],
        json!({"type": "piece", "piece": piece.0})
    );
    assert_eq!(bytes(&persist::from_json(&text).unwrap()), text);
}

#[test]
fn ca6_fichiers_forges_refuses_sans_perte_silencieuse() {
    let (p, _, _, _) = rect_with_topstitch(100.0, 60.0, 5.0);
    let good = bytes(&p);
    assert!(persist::from_json(&good).is_ok());
    // Témoin : réécrire le JSON sans le modifier reste accepté (les refus ci-dessous viennent
    // donc bien de la modification).
    assert_eq!(
        persist::from_json(&tamper(&good, |_| {})).unwrap(),
        persist::from_json(&good).unwrap()
    );
    type Forge = Box<dyn Fn(&mut Value)>;
    let forges: Vec<(&str, Forge)> = vec![
        (
            "distance nulle",
            Box::new(|v| v["sheets"][0]["topstitches"][0]["distance_mm"] = json!(0.0)),
        ),
        (
            "distance négative",
            Box::new(|v| v["sheets"][0]["topstitches"][0]["distance_mm"] = json!(-2.0)),
        ),
        (
            "distance trop grande",
            Box::new(|v| v["sheets"][0]["topstitches"][0]["distance_mm"] = json!(5000.0)),
        ),
        (
            "distance trop petite",
            Box::new(|v| v["sheets"][0]["topstitches"][0]["distance_mm"] = json!(0.01)),
        ),
        (
            "aucune ligne",
            Box::new(|v| v["sheets"][0]["topstitches"][0]["paths"] = json!([])),
        ),
        (
            "ligne absente",
            Box::new(|v| v["sheets"][0]["topstitches"][0]["paths"][0] = json!(987_654)),
        ),
        (
            "ligne en double",
            Box::new(|v| {
                let first = v["sheets"][0]["topstitches"][0]["paths"][0].clone();
                v["sheets"][0]["topstitches"][0]["paths"][1] = first;
            }),
        ),
        (
            "bord vide",
            Box::new(|v| v["sheets"][0]["topstitches"][0]["source"]["paths"] = json!([])),
        ),
        (
            "bord absent",
            Box::new(|v| v["sheets"][0]["topstitches"][0]["source"]["paths"][0] = json!(987_654)),
        ),
        (
            "bord = ligne générée",
            Box::new(|v| {
                let g = v["sheets"][0]["topstitches"][0]["paths"][0].clone();
                v["sheets"][0]["topstitches"][0]["source"]["paths"][0] = g;
            }),
        ),
        (
            "côté gauche pour un bord fermé",
            Box::new(|v| v["sheets"][0]["topstitches"][0]["side"] = json!("left")),
        ),
        (
            "côté inconnu",
            Box::new(|v| v["sheets"][0]["topstitches"][0]["side"] = json!("dessus")),
        ),
        (
            "départ sur un bord fermé",
            Box::new(|v| {
                let pid = v["sheets"][0]["points"][0]["id"].clone();
                v["sheets"][0]["topstitches"][0]["source"]["from"] = pid;
            }),
        ),
        (
            "type de source inconnu",
            Box::new(|v| v["sheets"][0]["topstitches"][0]["source"]["type"] = json!("contour")),
        ),
        (
            "clé inconnue",
            Box::new(|v| v["sheets"][0]["topstitches"][0]["extra"] = json!(1)),
        ),
        (
            "clé inconnue dans la source",
            Box::new(|v| v["sheets"][0]["topstitches"][0]["source"]["extra"] = json!(1)),
        ),
        (
            "obsolète vide",
            Box::new(|v| v["sheets"][0]["topstitches"][0]["stale"] = json!("")),
        ),
        (
            "obsolète trop long",
            Box::new(|v| v["sheets"][0]["topstitches"][0]["stale"] = json!("x".repeat(501))),
        ),
        (
            "obsolète avec contrôle",
            Box::new(|v| v["sheets"][0]["topstitches"][0]["stale"] = json!("a\u{7}b")),
        ),
        (
            "identifiant dupliqué",
            Box::new(|v| {
                let pid = v["sheets"][0]["points"][0]["id"].clone();
                v["sheets"][0]["topstitches"][0]["id"] = pid;
            }),
        ),
        (
            "rôle d'une ligne générée modifié",
            Box::new(|v| {
                let id = v["sheets"][0]["topstitches"][0]["paths"][0]
                    .as_u64()
                    .unwrap();
                for e in v["sheets"][0]["entities"].as_array_mut().unwrap() {
                    if e["id"].as_u64() == Some(id) {
                        e["role"] = json!("construction");
                    }
                }
            }),
        ),
        (
            "pièce absente",
            Box::new(|v| {
                v["sheets"][0]["topstitches"][0]["source"] =
                    json!({"type": "piece", "piece": 987_654});
                v["sheets"][0]["topstitches"][0]["side"] = json!("outside");
            }),
        ),
        (
            "deux liens sur les mêmes lignes",
            Box::new(|v| {
                let mut second = v["sheets"][0]["topstitches"][0].clone();
                second["id"] = json!(999_999);
                v["next_id"] = json!(1_000_000);
                v["sheets"][0]["topstitches"]
                    .as_array_mut()
                    .unwrap()
                    .push(second);
            }),
        ),
    ];
    for (name, forge) in &forges {
        let bad = tamper(&good, |v| forge(v));
        assert!(
            persist::from_json(&bad).is_err(),
            "fichier forgé accepté : {name}"
        );
    }
    // Un lien dont le bord est sur une autre feuille : refusé aussi.
    let mut q = p.clone();
    let other = q
        .apply(Command::CreateSheet { name: "B".into() })
        .unwrap()
        .sheet
        .unwrap();
    let foreign = seg(&mut q, other, (0.0, 0.0), (5.0, 0.0)).entities[0];
    let text = bytes(&q);
    let bad = tamper(&text, |v| {
        v["sheets"][0]["topstitches"][0]["source"]["paths"][0] = json!(foreign.0);
    });
    assert!(persist::from_json(&bad).is_err());
}

// ----- CA7 : cohérence avec le reste ------------------------------------------------------

#[test]
fn ca7_copie_de_feuille_le_lien_n_est_pas_copie() {
    let (mut p, s, _, t) = rect_with_topstitch(100.0, 60.0, 5.0);
    let out = p.apply(Command::DuplicateSheet { sheet: s }).unwrap();
    let copy = out.sheet.unwrap();
    assert!(
        out.notes.iter().any(|n| n.contains("surpiqûre")),
        "{:?}",
        out.notes
    );
    let cs = p.sheet(copy).unwrap();
    assert!(cs.topstitches.is_empty(), "le lien n'est jamais copié");
    assert_eq!(cs.entities.len(), 8);
    assert_eq!(
        cs.entities
            .iter()
            .filter(|e| e.role == PathRole::Topstitch)
            .count(),
        4,
        "les lignes copiées gardent leur rôle"
    );
    assert_eq!(p.sheet(s).unwrap().topstitches.len(), 1);
    // L'original bouge, la copie reste ; la copie se modifie librement.
    let before: Vec<_> = cs
        .entities
        .iter()
        .filter_map(|e| cs.shape(e.id).ok())
        .collect();
    move_point(&mut p, s, (100.0, 60.0), (150.0, 60.0)).unwrap();
    let cs = p.sheet(copy).unwrap();
    let after: Vec<_> = cs
        .entities
        .iter()
        .filter_map(|e| cs.shape(e.id).ok())
        .collect();
    assert_eq!(before, after);
    let q = point_at(&p, copy, 5.0, 5.0);
    p.apply(Command::MovePoint {
        sheet: copy,
        point: q,
        to: pt(7.0, 7.0),
    })
    .unwrap();
    // Import dans un autre projet : même règle, même remarque.
    let cmd = p.import_command(s).unwrap();
    let mut other = Project::new("autre");
    let out = other.apply(cmd).unwrap();
    assert!(out.notes.iter().any(|n| n.contains("surpiqûre")));
    let imported = other.sheet(out.sheet.unwrap()).unwrap();
    assert!(imported.topstitches.is_empty());
    assert_eq!(
        imported
            .entities
            .iter()
            .filter(|e| e.role == PathRole::Topstitch)
            .count(),
        4
    );
    other.validate().unwrap();
    p.validate().unwrap();
    let _ = t;
}

fn piece_with_topstitch(
    w: f64,
    h: f64,
    d: f64,
) -> (Project, SheetId, PieceId, TopstitchId, Vec<EntityId>) {
    let mut p = Project::new("pièce");
    let s = p.sheets[0].id;
    let r = rect(&mut p, s, 0.0, 0.0, w, h);
    let piece = p
        .apply(Command::ExtractPiece {
            sheet: s,
            source: PieceSource::Paths {
                entities: r.entities.clone(),
            },
            name: "Devant".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    let t = add_topstitch(
        &mut p,
        s,
        TopstitchSource::Piece { piece },
        d,
        TopstitchSide::Inside,
    )
    .unwrap()
    .topstitches[0];
    (p, s, piece, t, r.entities)
}

#[test]
fn ca7_separer_remplace_la_piece_le_lien_est_casse_avec_remarque() {
    let (mut p, s, piece, _, _) = piece_with_topstitch(100.0, 60.0, 5.0);
    let out = p
        .apply(Command::SplitPiece {
            piece,
            a: pt(50.0, -10.0),
            b: pt(50.0, 70.0),
        })
        .unwrap();
    assert!(
        out.notes.iter().any(|n| n.contains("surpiqûre")),
        "{:?}",
        out.notes
    );
    assert!(p.sheet(s).unwrap().topstitches.is_empty());
    // La construction n'est pas touchée : les lignes de surpiqûre sont toujours là.
    assert_eq!(
        p.sheet(s)
            .unwrap()
            .entities
            .iter()
            .filter(|e| e.role == PathRole::Topstitch)
            .count(),
        4
    );
    p.validate().unwrap();
}

#[test]
fn ca7_deplier_ne_reprend_pas_la_surpiqure_et_laisse_le_lien_de_la_mere() {
    let (mut p, s, piece, t, edges) = piece_with_topstitch(100.0, 60.0, 5.0);
    // Pliure : le côté gauche (x = 0) du rectangle.
    let fold = *edges
        .iter()
        .find(|&&e| {
            let (a, b) = p.sheet(s).unwrap().segment_ends(e).unwrap();
            a.x == 0.0 && b.x == 0.0
        })
        .unwrap();
    p.apply(Command::SetPathRole {
        sheet: s,
        entity: fold,
        role: PathRole::Fold,
    })
    .unwrap();
    let out = p
        .apply(Command::UnfoldPiece {
            piece,
            fold,
            name: None,
        })
        .unwrap();
    assert!(
        out.notes.iter().any(|n| n.contains("surpiqûre")),
        "{:?}",
        out.notes
    );
    assert_eq!(link(&p, s, t).state(), TopstitchState::Current);
    let unfolded = out.pieces[0];
    let usheet = p.piece_sheet(unfolded).unwrap();
    assert!(usheet.topstitches.is_empty());
    // Ni la pièce dépliée ni la mère ne sont déformées par la surpiqûre.
    assert!(near(p.piece_contour(piece).unwrap().area_mm2, 6000.0, 1e-9));
    p.validate().unwrap();
}

#[test]
fn ca7_piece_derivee_la_surpiqure_suit_la_regeneration_de_la_derivee() {
    let (mut p, s, mother, _, _) = piece_with_topstitch(200.0, 100.0, 5.0);
    let derived = p
        .apply(Command::CreateDerivedPiece {
            mother,
            kind: DerivedKind::Lining,
            operation: DerivedOp::Offset { distance_mm: -10.0 },
            name: None,
        })
        .unwrap();
    assert!(
        derived.notes.iter().any(|n| n.contains("surpiqûre")),
        "surpiqûre de la mère non reprise : {:?}",
        derived.notes
    );
    let dp = derived.pieces[0];
    let ds = p.piece_sheet(dp).unwrap().id;
    let t2 = add_topstitch(
        &mut p,
        ds,
        TopstitchSource::Piece { piece: dp },
        4.0,
        TopstitchSide::Inside,
    )
    .unwrap()
    .topstitches[0];
    let measure = |p: &Project| {
        let outline = p.piece(dp).unwrap().outline.clone();
        let sh = p.sheet(ds).unwrap();
        let poly: Vec<(PointMm, PointMm)> = outline
            .iter()
            .map(|e| sh.segment_ends(e.entity).unwrap())
            .collect();
        let lines = generated_segments(p, ds, t2);
        let mut worst = 0.0_f64;
        for (a, b) in lines {
            for q in [a, b] {
                worst = worst.max((dist_to_segments(q, &poly) - 4.0).abs());
            }
        }
        worst
    };
    assert!(measure(&p) < 1e-6);
    // La mère change : la dérivée est régénérée, puis la surpiqûre de la dérivée la suit.
    let out = move_point(&mut p, s, (200.0, 100.0), (240.0, 120.0)).unwrap();
    assert_eq!(out.regenerated, vec![dp]);
    assert!(out.regenerated_topstitches.contains(&t2));
    assert!(measure(&p) < 1e-6, "écart {}", measure(&p));
    p.validate().unwrap();
}

#[test]
fn ca7_plan_de_pieces_independant_des_lignes_generees() {
    let (mut p, s, piece, t, _) = piece_with_topstitch(100.0, 60.0, 5.0);
    let layout = format!("{:?}", p.plan_layout());
    p.apply(Command::BreakTopstitchLink {
        sheet: s,
        topstitch: t,
    })
    .unwrap();
    assert_eq!(format!("{:?}", p.plan_layout()), layout);
    // La mesure du contour de la pièce ne compte pas la surpiqûre.
    let c = p.piece_contour(piece).unwrap();
    assert!(near(c.area_mm2, 6000.0, 1e-9) && near(c.perimeter_mm, 320.0, 1e-9));
}

#[test]
fn ca7_elements_internes_la_ligne_generee_peut_etre_une_surpiqure_interne() {
    let (mut p, s, piece, t, _) = piece_with_topstitch(100.0, 60.0, 5.0);
    let line = link(&p, s, t).paths[0];
    p.apply(Command::AddPieceInternal {
        piece,
        target: InternalTarget::Path { entity: line },
        kind: InternalKind::Topstitch,
    })
    .unwrap();
    // Le bord bouge : mêmes identifiants, l'élément interne suit.
    move_point(&mut p, s, (100.0, 60.0), (120.0, 70.0)).unwrap();
    assert_eq!(link(&p, s, t).paths[0], line);
    assert_eq!(p.piece(piece).unwrap().internal.len(), 1);
    // Les deux notions ne se confondent pas : casser le lien ne retire pas l'appartenance.
    p.apply(Command::BreakTopstitchLink {
        sheet: s,
        topstitch: t,
    })
    .unwrap();
    assert_eq!(p.piece(piece).unwrap().internal.len(), 1);
    p.validate().unwrap();
}

#[test]
fn ca7_presse_papiers_les_lignes_collees_sont_libres() {
    let (mut p, s, _, t) = rect_with_topstitch(100.0, 60.0, 5.0);
    let lines = link(&p, s, t).paths;
    let fragment = p.copy_fragment(s, &[], &lines, &[], &[]).unwrap();
    p.apply(Command::PasteFragment {
        sheet: s,
        fragment: Box::new(fragment),
        layer: None,
    })
    .unwrap();
    assert_eq!(p.sheet(s).unwrap().topstitches.len(), 1);
    assert_eq!(
        p.sheet(s)
            .unwrap()
            .entities
            .iter()
            .filter(|e| e.role == PathRole::Topstitch)
            .count(),
        8
    );
    p.validate().unwrap();
}
