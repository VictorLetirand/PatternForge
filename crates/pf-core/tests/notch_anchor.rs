//! US-06.2 — cran lié à un point : ancrage à un point construit (pied de la perpendiculaire),
//! au milieu, à une proportion du bord, ou longueur reportée depuis un point d'une autre pièce
//! de la feuille. Un groupe de tests par critère d'acceptation :
//! CA1 modèle et fichier (schéma 3, octets inchangés sans ancrage), CA2 positions calculées
//! à la main, CA3 la position suit la construction, CA4 refus sans trace, CA5 ancrage
//! invalide (retrait avec remarque, jamais de référence morte), CA6 une étape annulable,
//! CA7 opérations sur le tracé (inverser, couper, fusionner, raccourcir, symétrie, dupliquer),
//! CA8 cohérence (copie de feuille, import, presse-papiers, séparer, assembler, déplier,
//! dérivées, verrou de calque). L'export est comparé dans `crates/pf-export/tests/`.

use pf_core::persist;
use pf_core::piece_internal::InternalGeometry;
use pf_core::*;
use pf_geometry::{distance_mm, PointMm, Side};
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

fn at_near(a: PointMm, b: PointMm, tol: f64) -> bool {
    distance_mm(a, b) <= tol
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

fn free(p: &mut Project, s: SheetId, x: f64, y: f64) -> PointId {
    p.apply(Command::AddPoint {
        sheet: s,
        at: pt(x, y),
    })
    .unwrap()
    .points[0]
}

fn add_cmd(sheet: SheetId, entity: EntityId, anchor: NotchAnchor) -> Command {
    Command::AddAnchoredNotch {
        sheet,
        entity,
        anchor,
        side: Side::Left,
        length_mm: 5.0,
        style: NotchStyle::Single,
    }
}

fn anchored(p: &mut Project, s: SheetId, e: EntityId, anchor: NotchAnchor) -> NotchId {
    p.apply(add_cmd(s, e, anchor)).unwrap().notches[0]
}

fn dist(p: &Project, s: SheetId, n: NotchId) -> f64 {
    let sh = p.sheet(s).unwrap();
    sh.notch_distance_mm(sh.notch(n).unwrap()).unwrap()
}

/// Pied (sur le bord) du premier trait du cran.
fn base(p: &Project, s: SheetId, n: NotchId) -> PointMm {
    let sh = p.sheet(s).unwrap();
    sh.notch_marks(sh.notch(n).unwrap()).unwrap()[0].0
}

fn notch(p: &Project, s: SheetId, n: NotchId) -> Notch {
    p.sheet(s).unwrap().notch(n).unwrap().clone()
}

/// Segment AB de (0 ; 0) à (120 ; 0).
struct Line {
    p: Project,
    s: SheetId,
    ab: EntityId,
    b: PointId,
}

fn line() -> Line {
    let mut p = Project::new("Crans ancrés");
    let s = p.sheets[0].id;
    let o = p
        .apply(Command::AddSegment {
            sheet: s,
            from: PointRef::New(pt(0., 0.)),
            to: PointRef::New(pt(120., 0.)),
        })
        .unwrap();
    Line {
        p,
        s,
        ab: o.entities[0],
        b: o.points[1],
    }
}

/// Courbe de Bézier symétrique (0 ; 0), (30 ; 60), (90 ; 60), (120 ; 0).
const CURVE: [(f64, f64); 4] = [(0., 0.), (30., 60.), (90., 60.), (120., 0.)];

fn curve(p: &mut Project, s: SheetId) -> Outcome {
    p.apply(Command::AddCurve {
        sheet: s,
        start: PointRef::New(pt(CURVE[0].0, CURVE[0].1)),
        c1: pt(CURVE[1].0, CURVE[1].1),
        c2: pt(CURVE[2].0, CURVE[2].1),
        end: PointRef::New(pt(CURVE[3].0, CURVE[3].1)),
    })
    .unwrap()
}

fn bez(c: &[PointMm; 4], t: f64) -> PointMm {
    let u = 1.0 - t;
    let w = [u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t];
    PointMm::new(
        (0..4).map(|i| w[i] * c[i].x).sum(),
        (0..4).map(|i| w[i] * c[i].y).sum(),
    )
}

/// Longueur de la Bézier entre deux paramètres, par une polyligne très fine (indépendante du cœur).
fn arc_len(c: &[PointMm; 4], t0: f64, t1: f64) -> f64 {
    let n = 200_000;
    let mut prev = bez(c, t0);
    let mut sum = 0.0;
    for i in 1..=n {
        let q = bez(c, t0 + (t1 - t0) * i as f64 / n as f64);
        sum += distance_mm(prev, q);
        prev = q;
    }
    sum
}

/// Paramètre `t` situé à la longueur `target` du départ (dichotomie sur `arc_len`).
fn t_at(c: &[PointMm; 4], target: f64) -> f64 {
    let (mut lo, mut hi) = (0.0, 1.0);
    for _ in 0..40 {
        let mid = 0.5 * (lo + hi);
        if arc_len(c, 0.0, mid) < target {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    0.5 * (lo + hi)
}

fn curve_pts() -> [PointMm; 4] {
    CURVE.map(|(x, y)| pt(x, y))
}

// ----- CA1 : modèle et fichier ----------------------------------------------------

#[test]
fn ca1_sans_ancrage_octets_inchanges() {
    for name in FIXTURES {
        let first = bytes(&fixture(name));
        let second = bytes(&persist::from_json(&first).unwrap());
        assert_eq!(first, second, "{name}");
        assert!(!first.contains("\"anchor\""), "{name}");
        assert!(fixture(name)
            .sheets
            .iter()
            .all(|s| s.notches.iter().all(|n| n.anchor.is_none())));
    }
    // Un cran à distance fixe n'écrit aucune clé « anchor ».
    let mut l = line();
    l.p.apply(Command::AddNotch {
        sheet: l.s,
        entity: l.ab,
        distance_mm: 40.0,
        side: Side::Left,
        length_mm: 5.0,
        style: NotchStyle::Single,
    })
    .unwrap();
    assert!(!bytes(&l.p).contains("\"anchor\""));
    // Fichier livré : relu puis réécrit, identique à l'octet.
    let file = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/piece-crans-droit-fil.pforge.json");
    let shipped = std::fs::read_to_string(file).unwrap().replace("\r\n", "\n");
    assert_eq!(
        shipped,
        persist::to_json(&persist::from_json(&shipped).unwrap()).unwrap()
    );
    assert!(!shipped.contains("\"anchor\""));
    // Détacher le dernier cran ancré : plus aucune clé « anchor ».
    let n = anchored(&mut l.p, l.s, l.ab, NotchAnchor::Midpoint {});
    assert!(bytes(&l.p).contains("\"anchor\""));
    l.p.apply(Command::SetNotchAnchor {
        sheet: l.s,
        notch: n,
        anchor: None,
    })
    .unwrap();
    assert!(!bytes(&l.p).contains("\"anchor\""));
}

#[test]
fn ca1_ancrage_relu_a_l_identique_schema_3() {
    let mut l = line();
    let q = free(&mut l.p, l.s, 30.0, 17.0);
    let src =
        l.p.apply(Command::AddSegment {
            sheet: l.s,
            from: PointRef::New(pt(0., 50.)),
            to: PointRef::New(pt(200., 50.)),
        })
        .unwrap()
        .entities[0];
    let n1 = anchored(&mut l.p, l.s, l.ab, NotchAnchor::Point { point: q });
    let n2 = anchored(&mut l.p, l.s, l.ab, NotchAnchor::Midpoint {});
    let n3 = anchored(&mut l.p, l.s, l.ab, NotchAnchor::Ratio { ratio: 0.25 });
    let n4 = anchored(
        &mut l.p,
        l.s,
        l.ab,
        NotchAnchor::Carried {
            entity: src,
            point: q,
            from_end: false,
        },
    );
    let n5 = anchored(
        &mut l.p,
        l.s,
        l.ab,
        NotchAnchor::Carried {
            entity: src,
            point: q,
            from_end: true,
        },
    );
    let text = bytes(&l.p);
    let v: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["schema_version"], 6, "schéma 4 (US-09.1)");
    let list = v["sheets"][0]["notches"].as_array().unwrap();
    assert_eq!(list.len(), 5);
    assert_eq!(
        list[0]["anchor"],
        json!({"type": "point", "point": q.0}),
        "{n1:?}"
    );
    assert_eq!(list[1]["anchor"], json!({"type": "midpoint"}), "{n2:?}");
    assert_eq!(
        list[2]["anchor"],
        json!({"type": "ratio", "ratio": 0.25}),
        "{n3:?}"
    );
    // `from_end` est omis quand il est faux.
    assert_eq!(
        list[3]["anchor"],
        json!({"type": "carried", "entity": src.0, "point": q.0}),
        "{n4:?}"
    );
    assert_eq!(
        list[4]["anchor"],
        json!({"type": "carried", "entity": src.0, "point": q.0, "from_end": true}),
        "{n5:?}"
    );
    // La distance stockée d'un cran ancré est 0 (sans effet) ; la position se calcule.
    assert!(list.iter().all(|n| n["distance_mm"] == json!(0.0)));
    let back = persist::from_json(&text).unwrap();
    assert_eq!(back, l.p);
    assert_eq!(bytes(&back), text);
    assert_eq!(
        [
            NotchAnchor::Point { point: q },
            NotchAnchor::Midpoint {},
            NotchAnchor::Ratio { ratio: 0.5 },
            NotchAnchor::Carried {
                entity: src,
                point: q,
                from_end: false
            }
        ]
        .map(|a| a.key()),
        ["point", "midpoint", "ratio", "carried"]
    );
}

#[test]
fn ca1_fichier_invalide_refuse_a_l_ouverture_sans_perte_silencieuse() {
    let mut l = line();
    let q = free(&mut l.p, l.s, 30.0, 17.0);
    anchored(&mut l.p, l.s, l.ab, NotchAnchor::Point { point: q });
    let good = bytes(&l.p);
    let v: Value = serde_json::from_str(&good).unwrap();
    let with = |anchor: Value| {
        let mut w = v.clone();
        w["sheets"][0]["notches"][0]["anchor"] = anchor;
        serde_json::to_string(&w).unwrap()
    };
    assert!(persist::from_json(&with(json!({"type": "midpoint"}))).is_ok());
    for (anchor, what) in [
        (json!({"type": "ratio", "ratio": 1.5}), "proportion > 1"),
        (json!({"type": "ratio", "ratio": -0.1}), "proportion < 0"),
        (json!({"type": "ratio"}), "proportion absente"),
        (json!({"type": "point", "point": 424242}), "point absent"),
        (json!({"type": "point"}), "point manquant"),
        (
            json!({"type": "carried", "entity": 424242, "point": q.0}),
            "tracé source absent",
        ),
        (json!({"type": "midpoint", "extra": 1}), "clé inconnue"),
        (json!({"type": "autre"}), "type inconnu"),
    ] {
        assert!(persist::from_json(&with(anchor)).is_err(), "{what}");
    }
    // Un cran ancré à un point d'une autre feuille est une référence morte pour cette feuille.
    let mut w = v.clone();
    w["sheets"].as_array_mut().unwrap().push(json!({
        "id": 777777, "name": "Autre", "points": [], "entities": []
    }));
    assert!(persist::from_json(&serde_json::to_string(&w).unwrap()).is_err());
}

// ----- CA2 : positions calculées à la main --------------------------------------------

#[test]
fn ca2_milieu_d_un_segment_exact() {
    let mut l = line();
    let n = anchored(&mut l.p, l.s, l.ab, NotchAnchor::Midpoint {});
    assert!(near(dist(&l.p, l.s, n), 60.0, 1e-12));
    assert!(at_near(base(&l.p, l.s, n), pt(60.0, 0.0), 1e-12));
    // Le cran pointe vers le côté demandé, perpendiculairement au bord.
    let sh = l.p.sheet(l.s).unwrap();
    let mark = sh.notch_marks(sh.notch(n).unwrap()).unwrap()[0];
    assert!(at_near(mark.1, pt(60.0, 5.0), 1e-12));
}

#[test]
fn ca2_milieu_d_une_courbe_par_longueur_developpee() {
    let mut p = Project::new("Courbe");
    let s = p.sheets[0].id;
    let c = curve(&mut p, s).entities[0];
    let n = anchored(&mut p, s, c, NotchAnchor::Midpoint {});
    // Courbe symétrique : le milieu en longueur est le point t = 0,5, soit (60 ; 45) à la main.
    assert!(at_near(base(&p, s, n), pt(60.0, 45.0), 1e-4));
    let total = arc_len(&curve_pts(), 0.0, 1.0);
    assert!(
        near(dist(&p, s, n), total / 2.0, 1e-4),
        "{}",
        dist(&p, s, n)
    );
    assert!(near(
        p.sheet(s).unwrap().path_length_mm(c).unwrap(),
        total,
        1e-4
    ));
}

#[test]
fn ca2_milieu_arc_cercle_et_courbe_lisse() {
    let mut p = Project::new("Autres tracés");
    let s = p.sheets[0].id;
    // Demi-cercle de rayon 50 de 0° à 180° : son milieu est à 90°, soit (0 ; 50).
    let arc = p
        .apply(Command::AddArc {
            sheet: s,
            center: PointRef::New(pt(0.0, 0.0)),
            radius_mm: 50.0,
            start_deg: 0.0,
            end_deg: 180.0,
            ccw: true,
        })
        .unwrap()
        .entities[0];
    let n = anchored(&mut p, s, arc, NotchAnchor::Midpoint {});
    assert!(at_near(base(&p, s, n), pt(0.0, 50.0), 1e-9));
    assert!(near(
        dist(&p, s, n),
        std::f64::consts::PI * 50.0 / 2.0,
        1e-9
    ));
    // Cercle de rayon 10 : le milieu est diamétralement opposé au départ.
    let circle = p
        .apply(Command::AddCircle {
            sheet: s,
            center: PointRef::New(pt(200.0, 0.0)),
            radius_mm: 10.0,
        })
        .unwrap()
        .entities[0];
    let n = anchored(&mut p, s, circle, NotchAnchor::Midpoint {});
    let start = p.sheet(s).unwrap().path_point_at(circle, 0.0).unwrap().0;
    assert!(near(distance_mm(base(&p, s, n), start), 20.0, 1e-9));
    assert!(near(dist(&p, s, n), std::f64::consts::PI * 10.0, 1e-9));
    // Courbe lisse symétrique par trois points : le milieu est son sommet (50 ; 30).
    let spline = p
        .apply(Command::AddSpline {
            sheet: s,
            points: vec![
                PointRef::New(pt(0.0, 100.0)),
                PointRef::New(pt(50.0, 130.0)),
                PointRef::New(pt(100.0, 100.0)),
            ],
        })
        .unwrap()
        .entities[0];
    let n = anchored(&mut p, s, spline, NotchAnchor::Midpoint {});
    let len = p.sheet(s).unwrap().path_length_mm(spline).unwrap();
    assert!(near(dist(&p, s, n), len / 2.0, 1e-9));
    assert!(at_near(base(&p, s, n), pt(50.0, 130.0), 1e-3));
}

#[test]
fn ca2_proportion_d_un_segment_exacte_bornes_comprises() {
    let mut l = line();
    for (ratio, want) in [
        (0.0, 0.0),
        (0.25, 30.0),
        (0.5, 60.0),
        (0.8, 96.0),
        (1.0, 120.0),
    ] {
        let n = anchored(&mut l.p, l.s, l.ab, NotchAnchor::Ratio { ratio });
        assert!(near(dist(&l.p, l.s, n), want, 1e-12), "t = {ratio}");
        assert!(at_near(base(&l.p, l.s, n), pt(want, 0.0), 1e-12));
    }
}

#[test]
fn ca2_proportion_d_une_courbe_par_longueur_developpee() {
    let mut p = Project::new("Courbe");
    let s = p.sheets[0].id;
    let c = curve(&mut p, s).entities[0];
    let pts = curve_pts();
    let total = arc_len(&pts, 0.0, 1.0);
    for ratio in [0.1, 0.25, 0.7] {
        let n = anchored(&mut p, s, c, NotchAnchor::Ratio { ratio });
        let t = t_at(&pts, ratio * total);
        assert!(
            at_near(base(&p, s, n), bez(&pts, t), 1e-3),
            "t = {ratio} : {:?} au lieu de {:?}",
            base(&p, s, n),
            bez(&pts, t)
        );
        assert!(near(dist(&p, s, n), ratio * total, 1e-3));
        // La courbe n'est pas parcourue à vitesse constante : ce n'est pas le paramètre t.
        assert!(!at_near(base(&p, s, n), bez(&pts, ratio), 0.5));
    }
}

#[test]
fn ca2_point_sur_un_segment_pied_de_la_perpendiculaire() {
    let mut l = line();
    // Point hors du bord : le cran est au pied de la perpendiculaire, pas sur le point.
    let q = free(&mut l.p, l.s, 30.0, 17.0);
    let n = anchored(&mut l.p, l.s, l.ab, NotchAnchor::Point { point: q });
    assert!(near(dist(&l.p, l.s, n), 30.0, 1e-12));
    assert!(at_near(base(&l.p, l.s, n), pt(30.0, 0.0), 1e-12));
    // Point construit exactement sur le bord : le cran est exactement sur le point.
    let on =
        l.p.apply(Command::AddPointOnSegment {
            sheet: l.s,
            segment: l.ab,
            distance_mm: 45.0,
            origin: SegmentEnd::Start,
        })
        .unwrap()
        .points[0];
    let n = anchored(&mut l.p, l.s, l.ab, NotchAnchor::Point { point: on });
    assert!(near(dist(&l.p, l.s, n), 45.0, 1e-12));
    let at = l.p.sheet(l.s).unwrap().position(on).unwrap();
    assert!(at_near(base(&l.p, l.s, n), at, 1e-12));
    // Un point au-delà de l'extrémité est ramené à l'extrémité (pied clampé sur le bord).
    let far = free(&mut l.p, l.s, 500.0, 3.0);
    let n = anchored(&mut l.p, l.s, l.ab, NotchAnchor::Point { point: far });
    assert!(near(dist(&l.p, l.s, n), 120.0, 1e-12));
}

#[test]
fn ca2_point_construit_sur_une_courbe_de_bezier() {
    let mut p = Project::new("Courbe");
    let s = p.sheets[0].id;
    let c = curve(&mut p, s).entities[0];
    let pts = curve_pts();
    for t in [0.15, 0.3, 0.5, 0.82] {
        let q = p
            .apply(Command::AddPointAt {
                sheet: s,
                point: PointRef::Derived(PointDefinition::OnCurve { curve: c, t }),
            })
            .unwrap()
            .points[0];
        let n = anchored(&mut p, s, c, NotchAnchor::Point { point: q });
        let want = arc_len(&pts, 0.0, t);
        let got = dist(&p, s, n);
        assert!(near(got, want, 1e-4), "t = {t} : {got} au lieu de {want}");
        let at = p.sheet(s).unwrap().position(q).unwrap();
        assert!(at_near(base(&p, s, n), at, 1e-4), "t = {t}");
        assert!(at_near(at, bez(&pts, t), 1e-9));
    }
}

#[test]
fn ca2_report_de_longueur_depuis_un_point_d_un_autre_tracé() {
    let mut l = line();
    let src =
        l.p.apply(Command::AddSegment {
            sheet: l.s,
            from: PointRef::New(pt(0., 50.)),
            to: PointRef::New(pt(200., 50.)),
        })
        .unwrap()
        .entities[0];
    // Le point se projette à 75 mm du départ du tracé source (sa distance au bord n'y change rien).
    let q = free(&mut l.p, l.s, 75.0, 80.0);
    let carried = |from_end| NotchAnchor::Carried {
        entity: src,
        point: q,
        from_end,
    };
    let n = anchored(&mut l.p, l.s, l.ab, carried(false));
    assert!(near(dist(&l.p, l.s, n), 75.0, 1e-12));
    assert!(at_near(base(&l.p, l.s, n), pt(75.0, 0.0), 1e-12));
    // Compté depuis l'arrivée du tracé du cran (120 mm) : 120 − 75 = 45.
    let n = anchored(&mut l.p, l.s, l.ab, carried(true));
    assert!(near(dist(&l.p, l.s, n), 45.0, 1e-12));
    assert!(at_near(base(&l.p, l.s, n), pt(45.0, 0.0), 1e-12));
}

#[test]
fn ca2_report_d_un_point_d_une_autre_piece_de_la_feuille() {
    let mut p = Project::new("Deux pièces");
    let s = p.sheets[0].id;
    let rect = |p: &mut Project, x: f64| {
        let o = p
            .apply(Command::AddRectangle {
                sheet: s,
                corner: PointRef::New(pt(x, 0.0)),
                width_mm: 100.0,
                height_mm: 60.0,
            })
            .unwrap();
        let piece = p
            .apply(Command::ExtractPiece {
                sheet: s,
                source: PieceSource::Paths {
                    entities: o.entities.clone(),
                },
                name: format!("Pièce {x}"),
                detached: false,
            })
            .unwrap()
            .pieces[0];
        (piece, o.entities)
    };
    let (_pa, ea) = rect(&mut p, 0.0);
    let (pb, eb) = rect(&mut p, 300.0);
    // Point de la pièce B à 40 % de son bord eb[0] (division : point construit, maintenu).
    let div = p
        .apply(Command::DivideTrace {
            sheet: s,
            trace: eb[0],
            spec: DivisionSpec::Proportions {
                values: vec![0.4, 0.6],
            },
            start: None,
        })
        .unwrap()
        .points[0];
    let sheet = p.sheet(s).unwrap();
    assert!(sheet.point(div).is_ok());
    assert!(p
        .piece(pb)
        .unwrap()
        .outline
        .iter()
        .any(|e| e.entity == eb[0]));
    let len_a = sheet.path_length_mm(ea[0]).unwrap();
    let len_b = sheet.path_length_mm(eb[0]).unwrap();
    let n = anchored(
        &mut p,
        s,
        ea[0],
        NotchAnchor::Carried {
            entity: eb[0],
            point: div,
            from_end: false,
        },
    );
    // Même longueur développée depuis le départ du bord source : 40 % de 100 mm = 40 mm.
    assert!(near(len_a, 100.0, 1e-9) && near(len_b, 100.0, 1e-9));
    assert!(near(dist(&p, s, n), 40.0, 1e-9), "{}", dist(&p, s, n));
    // Le point de la pièce B est maintenu : allonger son bord à 200 mm déplace le report (80 mm).
    let moved_end = p.sheet(s).unwrap().entity(eb[0]).unwrap().kind.all_points()[1];
    p.apply(Command::MovePoint {
        sheet: s,
        point: moved_end,
        to: pt(500.0, 0.0),
    })
    .unwrap();
    assert!(near(dist(&p, s, n), 80.0, 1e-9), "{}", dist(&p, s, n));
}

// ----- CA3 : la position suit la construction ----------------------------------------------

#[test]
fn ca3_le_cran_suit_le_deplacement_de_son_point() {
    let mut l = line();
    let q = free(&mut l.p, l.s, 30.0, 17.0);
    let n = anchored(&mut l.p, l.s, l.ab, NotchAnchor::Point { point: q });
    l.p.apply(Command::MovePoint {
        sheet: l.s,
        point: q,
        to: pt(80.0, 40.0),
    })
    .unwrap();
    assert!(near(dist(&l.p, l.s, n), 80.0, 1e-12));
    assert!(at_near(base(&l.p, l.s, n), pt(80.0, 0.0), 1e-12));
    // Un point construit (milieu de deux autres) entraîne le cran avec ses parents.
    let (p1, p2) = (
        free(&mut l.p, l.s, 10.0, 5.0),
        free(&mut l.p, l.s, 50.0, 5.0),
    );
    let mid =
        l.p.apply(Command::AddPointAt {
            sheet: l.s,
            point: PointRef::Derived(PointDefinition::Midpoint {
                first: p1,
                second: p2,
            }),
        })
        .unwrap()
        .points[0];
    let m = anchored(&mut l.p, l.s, l.ab, NotchAnchor::Point { point: mid });
    assert!(near(dist(&l.p, l.s, m), 30.0, 1e-12));
    l.p.apply(Command::MovePoint {
        sheet: l.s,
        point: p2,
        to: pt(90.0, 5.0),
    })
    .unwrap();
    assert!(near(dist(&l.p, l.s, m), 50.0, 1e-12));
}

#[test]
fn ca3_milieu_et_proportion_suivent_la_longueur_du_bord() {
    let mut l = line();
    let mid = anchored(&mut l.p, l.s, l.ab, NotchAnchor::Midpoint {});
    let quarter = anchored(&mut l.p, l.s, l.ab, NotchAnchor::Ratio { ratio: 0.25 });
    l.p.apply(Command::MovePoint {
        sheet: l.s,
        point: l.b,
        to: pt(200.0, 0.0),
    })
    .unwrap();
    assert!(near(dist(&l.p, l.s, mid), 100.0, 1e-12));
    assert!(near(dist(&l.p, l.s, quarter), 50.0, 1e-12));
    assert!(at_near(base(&l.p, l.s, mid), pt(100.0, 0.0), 1e-12));
    // Le bord pivote : la position suit le bord (milieu du nouveau segment).
    l.p.apply(Command::MovePoint {
        sheet: l.s,
        point: l.b,
        to: pt(0.0, 80.0),
    })
    .unwrap();
    assert!(at_near(base(&l.p, l.s, mid), pt(0.0, 40.0), 1e-9));
    // Un cran à distance fixe, lui, ne bouge pas de distance (comportement existant).
    let fixed =
        l.p.apply(Command::AddNotch {
            sheet: l.s,
            entity: l.ab,
            distance_mm: 10.0,
            side: Side::Left,
            length_mm: 5.0,
            style: NotchStyle::Single,
        })
        .unwrap()
        .notches[0];
    l.p.apply(Command::MovePoint {
        sheet: l.s,
        point: l.b,
        to: pt(300.0, 0.0),
    })
    .unwrap();
    assert!(near(dist(&l.p, l.s, fixed), 10.0, 1e-12));
    assert!(near(dist(&l.p, l.s, mid), 150.0, 1e-12));
}

#[test]
fn ca3_milieu_d_une_courbe_suit_sa_deformation() {
    let mut p = Project::new("Courbe");
    let s = p.sheets[0].id;
    let o = curve(&mut p, s);
    let c = o.entities[0];
    let n = anchored(&mut p, s, c, NotchAnchor::Midpoint {});
    // On étire la courbe : la nouvelle extrémité est en (200 ; 0) et sa poignée (relative à
    // l'extrémité) la suit, de (90 ; 60) à (170 ; 60).
    p.apply(Command::MovePoint {
        sheet: s,
        point: o.points[1],
        to: pt(200.0, 0.0),
    })
    .unwrap();
    let pts = {
        let mut a = curve_pts();
        a[2] = pt(170.0, 60.0);
        a[3] = pt(200.0, 0.0);
        a
    };
    let total = arc_len(&pts, 0.0, 1.0);
    assert!(
        near(dist(&p, s, n), total / 2.0, 1e-3),
        "{}",
        dist(&p, s, n)
    );
    let t = t_at(&pts, total / 2.0);
    assert!(at_near(base(&p, s, n), bez(&pts, t), 1e-3));
}

#[test]
fn ca3_report_suit_le_point_et_le_tracé_source() {
    let mut l = line();
    let o =
        l.p.apply(Command::AddSegment {
            sheet: l.s,
            from: PointRef::New(pt(0., 50.)),
            to: PointRef::New(pt(200., 50.)),
        })
        .unwrap();
    let (src, src_end) = (o.entities[0], o.points[1]);
    let q = free(&mut l.p, l.s, 75.0, 80.0);
    let n = anchored(
        &mut l.p,
        l.s,
        l.ab,
        NotchAnchor::Carried {
            entity: src,
            point: q,
            from_end: false,
        },
    );
    l.p.apply(Command::MovePoint {
        sheet: l.s,
        point: q,
        to: pt(100.0, 90.0),
    })
    .unwrap();
    assert!(near(dist(&l.p, l.s, n), 100.0, 1e-12));
    // Le tracé source change de sens de pente : la longueur reportée suit la projection.
    l.p.apply(Command::MovePoint {
        sheet: l.s,
        point: src_end,
        to: pt(0.0, 250.0),
    })
    .unwrap();
    // Source verticale de (0 ; 50) à (0 ; 250), point en (100 ; 90) : projeté à 40 mm.
    assert!(near(dist(&l.p, l.s, n), 40.0, 1e-9));
}

// ----- CA4 : refus sans trace -----------------------------------------------------------

#[test]
fn ca4_ajout_invalide_refuse_sans_trace() {
    let mut l = line();
    let q = free(&mut l.p, l.s, 30.0, 17.0);
    let other =
        l.p.apply(Command::CreateSheet {
            name: "Autre".into(),
        })
        .unwrap();
    let other_sheet = other.sheet.unwrap();
    let foreign = free(&mut l.p, other_sheet, 5.0, 5.0);
    let foreign_path =
        l.p.apply(Command::AddSegment {
            sheet: other_sheet,
            from: PointRef::New(pt(0., 0.)),
            to: PointRef::New(pt(10., 0.)),
        })
        .unwrap()
        .entities[0];
    let long =
        l.p.apply(Command::AddSegment {
            sheet: l.s,
            from: PointRef::New(pt(0., 50.)),
            to: PointRef::New(pt(500., 50.)),
        })
        .unwrap()
        .entities[0];
    let far = free(&mut l.p, l.s, 300.0, 70.0);
    for ratio in [
        -0.1,
        1.0000001,
        2.0,
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        1e300,
    ] {
        let msg = refused(&mut l.p, add_cmd(l.s, l.ab, NotchAnchor::Ratio { ratio }));
        assert!(msg.contains("proportion"), "{msg}");
    }
    let bad = [
        // point absent
        add_cmd(
            l.s,
            l.ab,
            NotchAnchor::Point {
                point: PointId(424242),
            },
        ),
        // point d'une autre feuille
        add_cmd(l.s, l.ab, NotchAnchor::Point { point: foreign }),
        // tracé du cran absent, ou d'une autre feuille
        add_cmd(l.s, EntityId(424242), NotchAnchor::Midpoint {}),
        add_cmd(l.s, foreign_path, NotchAnchor::Midpoint {}),
        // feuille absente
        add_cmd(SheetId(424242), l.ab, NotchAnchor::Midpoint {}),
        // report : tracé source absent / d'une autre feuille / point absent / d'une autre feuille
        add_cmd(
            l.s,
            l.ab,
            NotchAnchor::Carried {
                entity: EntityId(424242),
                point: q,
                from_end: false,
            },
        ),
        add_cmd(
            l.s,
            l.ab,
            NotchAnchor::Carried {
                entity: foreign_path,
                point: q,
                from_end: false,
            },
        ),
        add_cmd(
            l.s,
            l.ab,
            NotchAnchor::Carried {
                entity: long,
                point: PointId(424242),
                from_end: false,
            },
        ),
        add_cmd(
            l.s,
            l.ab,
            NotchAnchor::Carried {
                entity: long,
                point: foreign,
                from_end: true,
            },
        ),
        // report qui sort du bord : 300 mm sur un bord de 120 mm, des deux sens de comptage
        add_cmd(
            l.s,
            l.ab,
            NotchAnchor::Carried {
                entity: long,
                point: far,
                from_end: false,
            },
        ),
        add_cmd(
            l.s,
            l.ab,
            NotchAnchor::Carried {
                entity: long,
                point: far,
                from_end: true,
            },
        ),
    ];
    for c in bad {
        refused(&mut l.p, c);
    }
    // Longueur de cran hors bornes ou non finie.
    for length_mm in [0.0, -1.0, 100.01, f64::NAN, f64::INFINITY] {
        refused(
            &mut l.p,
            Command::AddAnchoredNotch {
                sheet: l.s,
                entity: l.ab,
                anchor: NotchAnchor::Midpoint {},
                side: Side::Left,
                length_mm,
                style: NotchStyle::Single,
            },
        );
    }
    assert!(l.p.sheet(l.s).unwrap().notches.is_empty());
}

#[test]
fn ca4_modification_invalide_refusee_sans_trace() {
    let mut l = line();
    let q = free(&mut l.p, l.s, 30.0, 17.0);
    let n = anchored(&mut l.p, l.s, l.ab, NotchAnchor::Ratio { ratio: 0.5 });
    let set = |anchor| Command::SetNotchAnchor {
        sheet: l.s,
        notch: n,
        anchor,
    };
    for ratio in [-1.0, 1.5, f64::NAN, f64::INFINITY] {
        refused(&mut l.p, set(Some(NotchAnchor::Ratio { ratio })));
    }
    refused(
        &mut l.p,
        set(Some(NotchAnchor::Point {
            point: PointId(424242),
        })),
    );
    refused(
        &mut l.p,
        Command::SetNotchAnchor {
            sheet: l.s,
            notch: NotchId(424242),
            anchor: None,
        },
    );
    refused(
        &mut l.p,
        Command::SetNotchAnchor {
            sheet: SheetId(424242),
            notch: n,
            anchor: None,
        },
    );
    // SetNotch ne peut pas changer la position calculée d'un cran ancré : on le dit.
    let msg = refused(
        &mut l.p,
        Command::SetNotch {
            sheet: l.s,
            notch: n,
            distance_mm: 10.0,
            side: Side::Left,
            length_mm: 5.0,
            style: NotchStyle::Single,
        },
    );
    assert!(msg.contains("ancré"), "{msg}");
    refused(
        &mut l.p,
        Command::SetNotch {
            sheet: l.s,
            notch: n,
            distance_mm: f64::NAN,
            side: Side::Left,
            length_mm: 5.0,
            style: NotchStyle::Single,
        },
    );
    // En revanche, le côté, la longueur et le style se changent, la position calculée reste.
    l.p.apply(Command::SetNotch {
        sheet: l.s,
        notch: n,
        distance_mm: 60.0,
        side: Side::Right,
        length_mm: 8.0,
        style: NotchStyle::Double,
    })
    .unwrap();
    let after = notch(&l.p, l.s, n);
    assert_eq!(after.anchor, Some(NotchAnchor::Ratio { ratio: 0.5 }));
    assert_eq!((after.side, after.length_mm), (Side::Right, 8.0));
    assert!(near(dist(&l.p, l.s, n), 60.0, 1e-12));
    let _ = q;
}

#[test]
fn ca4_un_deplacement_qui_sortirait_un_report_du_bord_est_refuse() {
    let mut l = line();
    let o =
        l.p.apply(Command::AddSegment {
            sheet: l.s,
            from: PointRef::New(pt(0., 50.)),
            to: PointRef::New(pt(200., 50.)),
        })
        .unwrap();
    let q = free(&mut l.p, l.s, 100.0, 80.0);
    anchored(
        &mut l.p,
        l.s,
        l.ab,
        NotchAnchor::Carried {
            entity: o.entities[0],
            point: q,
            from_end: false,
        },
    );
    // 150 mm reportés sur un bord de 120 mm : même règle que pour un cran à distance fixe.
    let msg = refused(
        &mut l.p,
        Command::MovePoint {
            sheet: l.s,
            point: q,
            to: pt(150.0, 80.0),
        },
    );
    assert!(msg.contains("sort du bord"), "{msg}");
    // Raccourcir le bord porteur en dessous du report : refusé aussi.
    refused(
        &mut l.p,
        Command::MovePoint {
            sheet: l.s,
            point: l.b,
            to: pt(60.0, 0.0),
        },
    );
    // Dans les limites : accepté.
    l.p.apply(Command::MovePoint {
        sheet: l.s,
        point: q,
        to: pt(110.0, 80.0),
    })
    .unwrap();
}

// ----- CA5 : ancrage devenu invalide -------------------------------------------------------

#[test]
fn ca5_point_supprime_retire_le_cran_avec_remarque() {
    let mut l = line();
    let q = free(&mut l.p, l.s, 30.0, 17.0);
    let keep = free(&mut l.p, l.s, 70.0, 17.0);
    let dead = anchored(&mut l.p, l.s, l.ab, NotchAnchor::Point { point: q });
    let alive = anchored(&mut l.p, l.s, l.ab, NotchAnchor::Point { point: keep });
    let mid = anchored(&mut l.p, l.s, l.ab, NotchAnchor::Midpoint {});
    let o =
        l.p.apply(Command::Delete {
            sheet: l.s,
            points: vec![q],
            entities: vec![],
        })
        .unwrap();
    let sh = l.p.sheet(l.s).unwrap();
    assert!(sh.notch(dead).is_err(), "cran retiré");
    assert!(sh.notch(alive).is_ok() && sh.notch(mid).is_ok());
    assert!(
        o.notes
            .iter()
            .any(|n| n.contains("ancré") && n.contains("retiré")),
        "{:?}",
        o.notes
    );
    l.p.validate().unwrap();
}

#[test]
fn ca5_tracé_source_supprime_retire_le_cran_qui_le_cite() {
    let mut l = line();
    let o =
        l.p.apply(Command::AddSegment {
            sheet: l.s,
            from: PointRef::New(pt(0., 50.)),
            to: PointRef::New(pt(200., 50.)),
        })
        .unwrap();
    let q = free(&mut l.p, l.s, 75.0, 80.0);
    let n = anchored(
        &mut l.p,
        l.s,
        l.ab,
        NotchAnchor::Carried {
            entity: o.entities[0],
            point: q,
            from_end: false,
        },
    );
    let res =
        l.p.apply(Command::Delete {
            sheet: l.s,
            points: vec![],
            entities: vec![o.entities[0]],
        })
        .unwrap();
    assert!(l.p.sheet(l.s).unwrap().notch(n).is_err());
    assert!(
        res.notes.iter().any(|t| t.contains("ancré")),
        "{:?}",
        res.notes
    );
    l.p.validate().unwrap();
}

#[test]
fn ca5_tracé_du_cran_supprime_retire_le_cran_ancre_comme_les_autres() {
    let mut l = line();
    let n = anchored(&mut l.p, l.s, l.ab, NotchAnchor::Midpoint {});
    let o =
        l.p.apply(Command::Delete {
            sheet: l.s,
            points: vec![],
            entities: vec![l.ab],
        })
        .unwrap();
    assert!(l.p.sheet(l.s).unwrap().notch(n).is_err());
    assert!(o.notes.iter().any(|t| t.contains("cran")), "{:?}", o.notes);
}

#[test]
fn ca5_le_retrait_emporte_l_element_interne_qui_citait_le_cran() {
    let mut p = Project::new("Interne");
    let s = p.sheets[0].id;
    let o = p
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
            source: PieceSource::Paths {
                entities: o.entities.clone(),
            },
            name: "Devant".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    let inner = p
        .apply(Command::AddSegment {
            sheet: s,
            from: PointRef::New(pt(20.0, 20.0)),
            to: PointRef::New(pt(80.0, 20.0)),
        })
        .unwrap();
    let q = free(&mut p, s, 50.0, 40.0);
    let n = anchored(
        &mut p,
        s,
        inner.entities[0],
        NotchAnchor::Point { point: q },
    );
    p.apply(Command::AddPieceInternal {
        piece,
        target: InternalTarget::Notch { notch: n },
        kind: InternalKind::Mark,
    })
    .unwrap();
    assert_eq!(p.piece(piece).unwrap().internal.len(), 1);
    // Le cran interne se lit à sa position calculée (50 ; 20), dans le contour.
    let r = p.piece_internals(piece);
    assert!(matches!(
        r[0].geometry,
        InternalGeometry::Position { at } if at_near(at, pt(50.0, 20.0), 1e-9)
    ));
    assert_eq!(r[0].inside_outline, Some(true));
    p.apply(Command::Delete {
        sheet: s,
        points: vec![q],
        entities: vec![],
    })
    .unwrap();
    assert!(p.piece(piece).unwrap().internal.is_empty());
    p.validate().unwrap();
}

#[test]
fn ca5_reference_morte_introduite_par_la_commande_reste_refusee() {
    let mut l = line();
    let q = free(&mut l.p, l.s, 30.0, 17.0);
    // Ajouter puis supprimer le point dans la même commande : le cran n'existait pas
    // avant, il n'est donc pas « retiré en silence » : la commande est refusée.
    let msg = refused(
        &mut l.p,
        Command::Batch {
            commands: vec![
                add_cmd(l.s, l.ab, NotchAnchor::Point { point: q }),
                Command::Delete {
                    sheet: l.s,
                    points: vec![q],
                    entities: vec![],
                },
            ],
        },
    );
    assert!(!msg.is_empty());
}

// ----- CA6 : une étape annulable ---------------------------------------------------------------

#[test]
fn ca6_chaque_commande_est_une_etape_annulable_et_le_sans_effet_n_en_fait_pas() {
    let l = line();
    let mut doc = Document::new(l.p.clone());
    let start = bytes(doc.project());
    let n = doc
        .execute(add_cmd(l.s, l.ab, NotchAnchor::Midpoint {}))
        .unwrap()
        .notches[0];
    let with_notch = bytes(doc.project());
    assert_ne!(start, with_notch);
    // Même ancrage : aucune étape.
    doc.execute(Command::SetNotchAnchor {
        sheet: l.s,
        notch: n,
        anchor: Some(NotchAnchor::Midpoint {}),
    })
    .unwrap();
    // Détacher un cran déjà à distance fixe : aucune étape non plus.
    let fixed = doc
        .execute(Command::AddNotch {
            sheet: l.s,
            entity: l.ab,
            distance_mm: 10.0,
            side: Side::Left,
            length_mm: 5.0,
            style: NotchStyle::Single,
        })
        .unwrap()
        .notches[0];
    let before_noop = bytes(doc.project());
    doc.execute(Command::SetNotchAnchor {
        sheet: l.s,
        notch: fixed,
        anchor: None,
    })
    .unwrap();
    assert_eq!(bytes(doc.project()), before_noop);
    assert!(doc.undo(), "annule l'ajout du cran fixe");
    assert_eq!(
        bytes(doc.project()),
        with_notch,
        "le sans-effet n'a pas fait d'étape"
    );
    // Changer d'ancrage = une étape ; annuler redonne les octets d'avant.
    doc.execute(Command::SetNotchAnchor {
        sheet: l.s,
        notch: n,
        anchor: Some(NotchAnchor::Ratio { ratio: 0.1 }),
    })
    .unwrap();
    assert!(near(dist(doc.project(), l.s, n), 12.0, 1e-12));
    assert!(doc.undo());
    assert_eq!(bytes(doc.project()), with_notch);
    // Détacher : une étape ; le cran garde sa position, annuler remet l'ancrage.
    doc.execute(Command::SetNotchAnchor {
        sheet: l.s,
        notch: n,
        anchor: None,
    })
    .unwrap();
    let detached = notch(doc.project(), l.s, n);
    assert_eq!((detached.anchor, detached.distance_mm), (None, 60.0));
    assert!(doc.undo());
    assert_eq!(bytes(doc.project()), with_notch);
    // Le retrait avec remarque d'une suppression de point est annulable en une étape.
    let q = free_in(&mut doc, l.s, 30.0, 17.0);
    let pn = doc
        .execute(add_cmd(l.s, l.ab, NotchAnchor::Point { point: q }))
        .unwrap()
        .notches[0];
    let before_delete = bytes(doc.project());
    doc.execute(Command::Delete {
        sheet: l.s,
        points: vec![q],
        entities: vec![],
    })
    .unwrap();
    assert!(doc.project().sheet(l.s).unwrap().notch(pn).is_err());
    assert!(doc.undo());
    assert_eq!(bytes(doc.project()), before_delete);
    assert!(doc.redo());
    assert!(doc.project().sheet(l.s).unwrap().notch(pn).is_err());
    while doc.undo() {}
    assert_eq!(bytes(doc.project()), start);
}

fn free_in(doc: &mut Document, s: SheetId, x: f64, y: f64) -> PointId {
    doc.execute(Command::AddPoint {
        sheet: s,
        at: pt(x, y),
    })
    .unwrap()
    .points[0]
}

// ----- CA7 : opérations sur le tracé ------------------------------------------------------------

#[test]
fn ca7_inverser_le_sens_garde_la_position_et_le_cote() {
    let mut l = line();
    let q = free(&mut l.p, l.s, 30.0, 17.0);
    let ns = [
        anchored(&mut l.p, l.s, l.ab, NotchAnchor::Midpoint {}),
        anchored(&mut l.p, l.s, l.ab, NotchAnchor::Ratio { ratio: 0.25 }),
        anchored(&mut l.p, l.s, l.ab, NotchAnchor::Point { point: q }),
    ];
    let before: Vec<(PointMm, PointMm)> = ns
        .iter()
        .map(|&n| {
            let sh = l.p.sheet(l.s).unwrap();
            sh.notch_marks(sh.notch(n).unwrap()).unwrap()[0]
        })
        .collect();
    let o =
        l.p.apply(Command::ReversePath {
            sheet: l.s,
            entity: l.ab,
        })
        .unwrap();
    assert!(o.notes.iter().all(|n| !n.contains("distance fixe")));
    for (n, b) in ns.iter().zip(&before) {
        let sh = l.p.sheet(l.s).unwrap();
        let now = sh.notch_marks(sh.notch(*n).unwrap()).unwrap()[0];
        assert!(at_near(now.0, b.0, 1e-9), "même place sur le bord");
        assert!(at_near(now.1, b.1, 1e-9), "même côté de la pièce");
        assert!(notch(&l.p, l.s, *n).anchor.is_some(), "l'ancrage est gardé");
    }
    assert_eq!(
        notch(&l.p, l.s, ns[1]).anchor,
        Some(NotchAnchor::Ratio { ratio: 0.75 })
    );
    // Report : le comptage depuis l'arrivée est inversé avec le tracé du cran.
    let o2 =
        l.p.apply(Command::AddSegment {
            sheet: l.s,
            from: PointRef::New(pt(0., 50.)),
            to: PointRef::New(pt(200., 50.)),
        })
        .unwrap();
    let r = free(&mut l.p, l.s, 75.0, 80.0);
    let c = anchored(
        &mut l.p,
        l.s,
        l.ab,
        NotchAnchor::Carried {
            entity: o2.entities[0],
            point: r,
            from_end: false,
        },
    );
    let at = base(&l.p, l.s, c);
    l.p.apply(Command::ReversePath {
        sheet: l.s,
        entity: l.ab,
    })
    .unwrap();
    assert!(at_near(base(&l.p, l.s, c), at, 1e-9));
    assert!(matches!(
        notch(&l.p, l.s, c).anchor,
        Some(NotchAnchor::Carried { from_end: true, .. })
    ));
    // Inverser le tracé SOURCE d'un report : le cran est détaché, à sa place, avec remarque.
    let o =
        l.p.apply(Command::ReversePath {
            sheet: l.s,
            entity: o2.entities[0],
        })
        .unwrap();
    assert!(at_near(base(&l.p, l.s, c), at, 1e-9));
    assert_eq!(notch(&l.p, l.s, c).anchor, None);
    assert!(o.notes.iter().any(|n| n.contains("distance fixe")));
}

#[test]
fn ca7_couper_un_tracé_detache_les_crans_ancres_sans_les_deplacer() {
    let mut l = line();
    let mid = anchored(&mut l.p, l.s, l.ab, NotchAnchor::Midpoint {});
    let q = free(&mut l.p, l.s, 100.0, 9.0);
    let on_q = anchored(&mut l.p, l.s, l.ab, NotchAnchor::Point { point: q });
    let (b1, b2) = (base(&l.p, l.s, mid), base(&l.p, l.s, on_q));
    let o =
        l.p.apply(Command::SplitPath {
            sheet: l.s,
            entity: l.ab,
            at: pt(80.0, 0.0),
        })
        .unwrap();
    l.p.validate().unwrap();
    assert_eq!(
        o.notes
            .iter()
            .filter(|n| n.contains("distance fixe"))
            .count(),
        2,
        "{:?}",
        o.notes
    );
    for (n, b) in [(mid, b1), (on_q, b2)] {
        assert_eq!(notch(&l.p, l.s, n).anchor, None);
        assert!(at_near(base(&l.p, l.s, n), b, 1e-9), "ni saut ni perte");
    }
    // Le cran à 100 mm est passé sur le second tronçon, à 20 mm de son départ.
    assert_eq!(notch(&l.p, l.s, on_q).entity, o.entities[0]);
    assert!(near(notch(&l.p, l.s, on_q).distance_mm, 20.0, 1e-9));
}

#[test]
fn ca7_fusionner_raccourcir_prolonger_detachent_les_crans_ancres() {
    // Fusion de deux segments bout à bout.
    let mut p = Project::new("Fusion");
    let s = p.sheets[0].id;
    let a = p
        .apply(Command::AddSegment {
            sheet: s,
            from: PointRef::New(pt(0., 0.)),
            to: PointRef::New(pt(50., 0.)),
        })
        .unwrap();
    let b = p
        .apply(Command::AddSegment {
            sheet: s,
            from: PointRef::Existing(a.points[1]),
            to: PointRef::New(pt(150., 0.)),
        })
        .unwrap();
    let (e1, e2) = (a.entities[0], b.entities[0]);
    let n1 = anchored(&mut p, s, e1, NotchAnchor::Midpoint {});
    let n2 = anchored(&mut p, s, e2, NotchAnchor::Ratio { ratio: 0.5 });
    let (p1, p2) = (base(&p, s, n1), base(&p, s, n2));
    let o = p
        .apply(Command::MergePaths {
            sheet: s,
            first: e1,
            second: e2,
        })
        .unwrap();
    assert_eq!(
        o.notes
            .iter()
            .filter(|n| n.contains("distance fixe"))
            .count(),
        2
    );
    assert!(at_near(base(&p, s, n1), p1, 1e-9) && at_near(base(&p, s, n2), p2, 1e-9));
    assert!(near(dist(&p, s, n2), 100.0, 1e-9));
    // Raccourcir : le cran du milieu garde sa place (ou est retiré s'il tombe dans la partie coupée).
    let mut l = line();
    let m = anchored(&mut l.p, l.s, l.ab, NotchAnchor::Midpoint {});
    let o =
        l.p.apply(Command::ExtendPath {
            sheet: l.s,
            entity: l.ab,
            end: SegmentEnd::End,
            delta_mm: -20.0,
        })
        .unwrap();
    assert!(o.notes.iter().any(|n| n.contains("distance fixe")));
    assert_eq!(notch(&l.p, l.s, m).anchor, None);
    assert!(near(dist(&l.p, l.s, m), 60.0, 1e-9));
    // Prolonger par le départ : le cran garde sa place géométrique.
    let m2 = anchored(&mut l.p, l.s, l.ab, NotchAnchor::Midpoint {});
    let at = base(&l.p, l.s, m2);
    l.p.apply(Command::ExtendPath {
        sheet: l.s,
        entity: l.ab,
        end: SegmentEnd::Start,
        delta_mm: 30.0,
    })
    .unwrap();
    assert!(at_near(base(&l.p, l.s, m2), at, 1e-9));
    assert_eq!(notch(&l.p, l.s, m2).anchor, None);
}

#[test]
fn ca7_symetrie_liee_et_duplication_recopient_l_ancrage_quand_c_est_possible() {
    let mut p = Project::new("Symétrie");
    let s = p.sheets[0].id;
    let axis = p
        .apply(Command::AddSegment {
            sheet: s,
            from: PointRef::New(pt(0., -50.)),
            to: PointRef::New(pt(0., 50.)),
        })
        .unwrap()
        .entities[0];
    let o = p
        .apply(Command::AddSegment {
            sheet: s,
            from: PointRef::New(pt(10., 0.)),
            to: PointRef::New(pt(110., 0.)),
        })
        .unwrap();
    let e = o.entities[0];
    let q = free(&mut p, s, 40.0, 12.0);
    let far = free(&mut p, s, 99.0, 30.0);
    let mid = anchored(&mut p, s, e, NotchAnchor::Midpoint {});
    let ratio = anchored(&mut p, s, e, NotchAnchor::Ratio { ratio: 0.25 });
    let on_q = anchored(&mut p, s, e, NotchAnchor::Point { point: q });
    let carried = anchored(
        &mut p,
        s,
        e,
        NotchAnchor::Carried {
            entity: axis,
            point: far,
            from_end: false,
        },
    );
    let _ = carried;
    // Symétrie liée par rapport à l'axe : points et tracé reflétés, crans recopiés.
    let out = p
        .apply(Command::LinkMirror {
            sheet: s,
            points: vec![q],
            entities: vec![e],
            axis: MirrorAxis::Segment { segment: axis },
        })
        .unwrap();
    p.validate().unwrap();
    assert_eq!(out.notches.len(), 4);
    let copies: Vec<Notch> = out.notches.iter().map(|&id| notch(&p, s, id)).collect();
    assert_eq!(copies[0].anchor, Some(NotchAnchor::Midpoint {}));
    assert_eq!(copies[1].anchor, Some(NotchAnchor::Ratio { ratio: 0.75 }));
    assert!(matches!(copies[2].anchor, Some(NotchAnchor::Point { point }) if point != q));
    // Le report cite un tracé qui n'est pas reflété : recopié à distance fixe, avec remarque.
    assert_eq!(copies[3].anchor, None);
    assert!(
        out.notes.iter().any(|n| n.contains("distance fixe")),
        "{:?}",
        out.notes
    );
    // Chaque copie est au miroir de son original (même place, de l'autre côté de l'axe).
    for (orig, copy) in [mid, ratio, on_q, carried].iter().zip(&out.notches) {
        let (a, b) = (base(&p, s, *orig), base(&p, s, *copy));
        assert!(at_near(pt(-a.x, a.y), b, 1e-9), "{a:?} / {b:?}");
    }
    // Duplication par translation : mêmes ancrages recopiés (le point copié est suivi).
    let dup = p
        .apply(Command::Duplicate {
            sheet: s,
            entities: vec![e],
            transform: pf_geometry::Transform::Translate {
                dx_mm: 0.0,
                dy_mm: 200.0,
            },
        })
        .unwrap();
    p.validate().unwrap();
    let kinds: Vec<Option<&'static str>> = dup
        .notches
        .iter()
        .map(|&id| notch(&p, s, id).anchor.map(|a| a.key()))
        .collect();
    assert_eq!(kinds[0], Some("midpoint"));
    assert_eq!(kinds[1], Some("ratio"));
    // Le point `q` n'est copié que s'il est une extrémité : ici non, donc distance fixe.
    assert_eq!(kinds[2], None);
    assert_eq!(kinds[3], None);
    assert!(dup.notes.iter().any(|n| n.contains("distance fixe")));
    for (orig, copy) in [mid, ratio, on_q, carried].iter().zip(&dup.notches) {
        let (a, b) = (base(&p, s, *orig), base(&p, s, *copy));
        assert!(at_near(pt(a.x, a.y + 200.0), b, 1e-9), "{a:?} / {b:?}");
    }
}

// ----- CA8 : cohérence --------------------------------------------------------------------------

/// Pièce 100 × 60 liée sur une feuille, un point libre `q` et un cran ancré au point `q` sur le
/// bord du bas, un autre au milieu du bord de droite.
struct Piece {
    p: Project,
    s: SheetId,
    piece: PieceId,
    edges: Vec<EntityId>,
    q: PointId,
    on_q: NotchId,
    mid: NotchId,
}

fn piece_scene() -> Piece {
    let mut p = Project::new("Pièce");
    let s = p.sheets[0].id;
    let o = p
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
            source: PieceSource::Paths {
                entities: o.entities.clone(),
            },
            name: "Devant".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    let q = free(&mut p, s, 25.0, 30.0);
    let on_q = anchored(&mut p, s, o.entities[0], NotchAnchor::Point { point: q });
    let mid = anchored(&mut p, s, o.entities[1], NotchAnchor::Midpoint {});
    Piece {
        p,
        s,
        piece,
        edges: o.entities,
        q,
        on_q,
        mid,
    }
}

#[test]
fn ca8_copie_de_feuille_remappe_les_ancrages_et_reste_independante() {
    let mut sc = piece_scene();
    let l =
        sc.p.sheet(sc.s)
            .unwrap()
            .notch_marks(&notch(&sc.p, sc.s, sc.on_q))
            .unwrap();
    let before = sc.p.clone();
    let o = sc.p.apply(Command::DuplicateSheet { sheet: sc.s }).unwrap();
    sc.p.validate().unwrap();
    let new_sheet = o.sheet.unwrap();
    // L'original n'a pas changé.
    assert_eq!(sc.p.sheet(sc.s).unwrap(), before.sheet(sc.s).unwrap());
    let sh = sc.p.sheet(new_sheet).unwrap();
    assert_eq!(sh.notches.len(), 2);
    for n in &sh.notches {
        // Identifiants neufs ; l'ancrage cite des objets de la copie seulement.
        assert!(n.id != sc.on_q && n.id != sc.mid);
        assert!(sh.entity(n.entity).is_ok());
        if let Some(NotchAnchor::Point { point }) = n.anchor {
            assert!(sh.point(point).is_ok() && point != sc.q);
        }
    }
    let copied_q = sh
        .notches
        .iter()
        .find_map(|n| match n.anchor {
            Some(NotchAnchor::Point { point }) => Some(point),
            _ => None,
        })
        .unwrap();
    // Même position qu'à l'original ; déplacer le point de la copie ne bouge pas l'original.
    let copy_marks = sh.notch_marks(&sh.notches[0]).unwrap();
    assert!(at_near(copy_marks[0].0, l[0].0, 1e-9));
    let orig_base = base(&sc.p, sc.s, sc.on_q);
    sc.p.apply(Command::MovePoint {
        sheet: new_sheet,
        point: copied_q,
        to: pt(70.0, 30.0),
    })
    .unwrap();
    assert!(at_near(base(&sc.p, sc.s, sc.on_q), orig_base, 1e-12));
    assert!(at_near(
        base(
            &sc.p,
            new_sheet,
            sc.p.sheet(new_sheet).unwrap().notches[0].id
        ),
        pt(70.0, 0.0),
        1e-9
    ));
    // Import dans un autre projet : mêmes remappages, ancrages vivants.
    let mut other = Project::new("Autre projet");
    let imported = other.apply(before.import_command(sc.s).unwrap()).unwrap();
    other.validate().unwrap();
    let sh = other.sheet(imported.sheet.unwrap()).unwrap();
    assert_eq!(sh.notches.len(), 2);
    assert!(sh
        .notches
        .iter()
        .any(|n| matches!(n.anchor, Some(NotchAnchor::Point { .. }))));
    assert!(sh
        .notches
        .iter()
        .any(|n| n.anchor == Some(NotchAnchor::Midpoint {})));
}

#[test]
fn ca8_presse_papiers_garde_les_ancrages_autonomes_et_detache_les_autres() {
    let mut sc = piece_scene();
    // Tout est copié (tracé, point cité, cran) : l'ancrage est gardé.
    let all_points: Vec<PointId> =
        sc.p.sheet(sc.s)
            .unwrap()
            .points
            .iter()
            .map(|p| p.id)
            .collect();
    let frag =
        sc.p.copy_fragment(sc.s, &all_points, &sc.edges, &[], &[])
            .unwrap();
    assert!(frag
        .notches
        .iter()
        .any(|n| matches!(n.anchor, Some(NotchAnchor::Point { .. }))));
    assert!(frag.notes.iter().all(|n| !n.contains("distance fixe")));
    sc.p.apply(Command::PasteFragment {
        sheet: sc.s,
        fragment: Box::new(frag),
        layer: None,
    })
    .unwrap();
    sc.p.validate().unwrap();
    let sh = sc.p.sheet(sc.s).unwrap();
    assert_eq!(sh.notches.len(), 4);
    assert_eq!(
        sh.notches
            .iter()
            .filter(|n| matches!(n.anchor, Some(NotchAnchor::Point { point }) if point != sc.q))
            .count(),
        1,
        "le cran collé est ancré au point collé"
    );
    // Le point cité n'est pas copié : le cran est copié à distance fixe, avec remarque.
    let frag = sc.p.copy_fragment(sc.s, &[], &sc.edges, &[], &[]).unwrap();
    assert!(
        frag.notes.iter().any(|n| n.contains("distance fixe")),
        "{:?}",
        frag.notes
    );
    assert!(frag
        .notches
        .iter()
        .all(|n| !matches!(n.anchor, Some(NotchAnchor::Point { .. }))));
    // Un fragment forgé dont l'ancrage cite un objet absent est refusé.
    let mut forged = frag.clone();
    forged.notches[0].anchor = Some(NotchAnchor::Point {
        point: PointId(424242),
    });
    let before = bytes(&sc.p);
    assert!(sc
        .p
        .apply(Command::PasteFragment {
            sheet: sc.s,
            fragment: Box::new(forged),
            layer: None,
        })
        .is_err());
    assert_eq!(bytes(&sc.p), before);
}

#[test]
fn ca8_separer_ne_reprend_pas_les_crans_le_dit_et_laisse_la_construction() {
    let mut sc = piece_scene();
    let before = notch(&sc.p, sc.s, sc.on_q);
    let o =
        sc.p.apply(Command::SplitPiece {
            piece: sc.piece,
            a: pt(50.0, -10.0),
            b: pt(50.0, 70.0),
        })
        .unwrap();
    sc.p.validate().unwrap();
    assert!(
        o.notes
            .iter()
            .any(|n| n.contains("cran(s)") && n.contains("ne sont pas repris")),
        "{:?}",
        o.notes
    );
    // La construction n'est pas touchée : le cran ancré est toujours là, ancré, à sa place.
    assert_eq!(notch(&sc.p, sc.s, sc.on_q), before);
    assert!(near(dist(&sc.p, sc.s, sc.on_q), 25.0, 1e-9));
    // Les moitiés n'ont aucun cran ancré (leurs feuilles sont dédiées).
    for piece in &o.pieces {
        let sheet = sc.p.piece_sheet(*piece).unwrap();
        assert!(sheet.notches.is_empty());
    }
}

#[test]
fn ca8_assembler_et_deplier_ne_touchent_pas_les_crans_ancres() {
    let mut sc = piece_scene();
    let o =
        sc.p.apply(Command::SplitPiece {
            piece: sc.piece,
            a: pt(50.0, -10.0),
            b: pt(50.0, 70.0),
        })
        .unwrap();
    let (l, r) = (o.pieces[0], o.pieces[1]);
    let edge = |p: &Project, half: PieceId| p.piece(half).unwrap().outline.last().unwrap().entity;
    let j =
        sc.p.apply(Command::JoinPieces {
            first: l,
            first_edge: edge(&sc.p, l),
            second: r,
            second_edge: edge(&sc.p, r),
            tolerance_mm: 0.5,
        })
        .unwrap();
    sc.p.validate().unwrap();
    // Le résultat est sur sa feuille ; la construction (et ses crans ancrés) est intacte.
    assert!(near(dist(&sc.p, sc.s, sc.on_q), 25.0, 1e-9));
    assert!(sc.p.sheet(sc.s).unwrap().notch(sc.mid).is_ok());
    let merged = j.pieces[0];
    assert!(sc.p.piece_sheet(merged).unwrap().notches.is_empty());
    // Déplier : la pièce source est un demi-dos ; la dépliée ne reprend aucun cran.
    let mut p = Project::new("déplier");
    let s = p.sheets[0].id;
    let rect = p
        .apply(Command::AddRectangle {
            sheet: s,
            corner: PointRef::New(pt(0.0, 0.0)),
            width_mm: 60.0,
            height_mm: 40.0,
        })
        .unwrap();
    let piece = p
        .apply(Command::ExtractPiece {
            sheet: s,
            source: PieceSource::Paths {
                entities: rect.entities.clone(),
            },
            name: "Demi-dos".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    let n = anchored(&mut p, s, rect.entities[0], NotchAnchor::Midpoint {});
    let fold = {
        let sheet = p.piece_sheet(piece).unwrap();
        p.piece(piece)
            .unwrap()
            .outline
            .iter()
            .map(|e| e.entity)
            .find(|&e| {
                let (a, b) = sheet.segment_ends(e).unwrap();
                near(a.x, 0.0, 1e-9) && near(b.x, 0.0, 1e-9)
            })
            .unwrap()
    };
    let u = p
        .apply(Command::UnfoldPiece {
            piece,
            fold,
            name: None,
        })
        .unwrap();
    p.validate().unwrap();
    assert!(
        u.notes.iter().any(|t| t.contains("cran")),
        "le dépliage dit qu'il ne reprend pas les crans : {:?}",
        u.notes
    );
    assert_eq!(notch(&p, s, n).anchor, Some(NotchAnchor::Midpoint {}));
    assert!(p.piece_sheet(u.pieces[0]).unwrap().notches.is_empty());
}

#[test]
fn ca8_piece_derivee_ne_reprend_pas_les_crans_et_la_mere_garde_les_siens() {
    let mut sc = piece_scene();
    let d =
        sc.p.apply(Command::CreateDerivedPiece {
            mother: sc.piece,
            kind: DerivedKind::Lining,
            operation: DerivedOp::Offset { distance_mm: -3.0 },
            name: None,
        })
        .unwrap();
    sc.p.validate().unwrap();
    let derived = d.pieces[0];
    assert!(sc.p.piece_sheet(derived).unwrap().notches.is_empty());
    // Déplacer le point d'un cran ancré de la mère : le cran suit, la dérivée reste cohérente.
    let o =
        sc.p.apply(Command::MovePoint {
            sheet: sc.s,
            point: sc.q,
            to: pt(60.0, 30.0),
        })
        .unwrap();
    sc.p.validate().unwrap();
    assert!(near(dist(&sc.p, sc.s, sc.on_q), 60.0, 1e-9));
    assert!(
        o.regenerated.is_empty(),
        "le contour de la mère n'a pas changé"
    );
    // Déformer le contour de la mère : la dérivée est régénérée, le cran milieu suit le bord.
    let corner =
        sc.p.sheet(sc.s)
            .unwrap()
            .entity(sc.edges[1])
            .unwrap()
            .kind
            .all_points();
    let before = base(&sc.p, sc.s, sc.mid);
    let moved =
        sc.p.apply(Command::MovePoint {
            sheet: sc.s,
            point: corner[1],
            to: pt(100.0, 90.0),
        })
        .unwrap();
    sc.p.validate().unwrap();
    assert!(!moved.regenerated.is_empty());
    assert!(!at_near(base(&sc.p, sc.s, sc.mid), before, 1e-6));
    assert!(sc.p.piece_sheet(derived).unwrap().notches.is_empty());
}

#[test]
fn ca8_cran_d_un_calque_verrouille_ne_bouge_pas_avec_son_point() {
    let mut l = line();
    let q = free(&mut l.p, l.s, 30.0, 17.0);
    let n = anchored(&mut l.p, l.s, l.ab, NotchAnchor::Point { point: q });
    let layer =
        l.p.apply(Command::AddLayer {
            sheet: l.s,
            name: "Coupe".into(),
        })
        .unwrap()
        .layers[0];
    l.p.apply(Command::SetObjectsLayer {
        sheet: l.s,
        points: vec![],
        entities: vec![l.ab],
        layer: Some(layer),
    })
    .unwrap();
    l.p.apply(Command::SetLayerLocked {
        sheet: l.s,
        layer,
        locked: true,
    })
    .unwrap();
    // Le point est sur un calque libre, mais le cran appartient au tracé verrouillé.
    let msg = refused(
        &mut l.p,
        Command::MovePoint {
            sheet: l.s,
            point: q,
            to: pt(80.0, 17.0),
        },
    );
    assert!(msg.contains("verrouillé"), "{msg}");
    assert!(near(dist(&l.p, l.s, n), 30.0, 1e-12));
}

#[test]
fn ca8_import_de_cran_ancre_visible_et_masque_comme_son_tracé() {
    // Un cran suit le calque de son tracé : ancré ou non, il est masqué avec lui.
    let mut l = line();
    let n = anchored(&mut l.p, l.s, l.ab, NotchAnchor::Midpoint {});
    let layer =
        l.p.apply(Command::AddLayer {
            sheet: l.s,
            name: "Coupe".into(),
        })
        .unwrap()
        .layers[0];
    l.p.apply(Command::SetObjectsLayer {
        sheet: l.s,
        points: vec![],
        entities: vec![l.ab],
        layer: Some(layer),
    })
    .unwrap();
    l.p.apply(Command::SetLayerVisible {
        sheet: l.s,
        layer,
        visible: false,
    })
    .unwrap();
    let sh = l.p.sheet(l.s).unwrap();
    assert!(!sh.notch_visible(sh.notch(n).unwrap()));
}

/// D-04 (corrigé) : serde ignorerait une clé en trop sur une variante unitaire d'un enum à étiquette
/// interne ; `MarkingShape` est lu via un miroir à variante `Punch {}`.
#[test]
fn d04_une_cle_en_trop_sur_punch_est_refusee() {
    let r: Result<MarkingShape, _> = serde_json::from_str(r#"{"type": "punch", "extra": 1}"#);
    assert!(r.is_err(), "clé inconnue acceptée : {r:?}");
}
