//! PH-4a — construction rapide : polygone, rectangle orienté, rectangle à coins arrondis.
//! Un test au moins par critère d'acceptation ; toutes les valeurs de contrôle sont calculées à
//! la main (formules fermées), jamais lues dans le code testé.

use pf_core::*;
use pf_geometry::{distance_mm, PointMm, RectSpec, Transform};
use std::f64::consts::PI;

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

fn setup() -> (Project, SheetId) {
    let p = Project::new("Construction rapide");
    let s = p.sheets[0].id;
    (p, s)
}

fn snap(p: &Project) -> String {
    serde_json::to_string(p).unwrap()
}

fn rect(x: f64, y: f64, w: f64, h: f64, rot: f64, centered: bool) -> RectSpec {
    RectSpec {
        origin: pt(x, y),
        width_mm: w,
        height_mm: h,
        rotation_deg: rot,
        centered,
    }
}

fn oriented(s: SheetId, r: RectSpec) -> Command {
    Command::AddOrientedRectangle {
        sheet: s,
        rect: r,
        role: PathRole::Construction,
        layer: None,
    }
}

fn rounded(s: SheetId, r: RectSpec, radius: f64, hide: bool) -> Command {
    Command::AddRoundedRectangle {
        sheet: s,
        rect: r,
        radius_mm: radius,
        hide_centers: hide,
        role: PathRole::Construction,
        layer: None,
    }
}

fn polygon(s: SheetId, v: &[(f64, f64)], closed: bool) -> Command {
    Command::AddPolygon {
        sheet: s,
        vertices: v.iter().map(|(x, y)| PointRef::New(pt(*x, *y))).collect(),
        closed,
        role: PathRole::Construction,
        layer: None,
    }
}

fn positions(p: &Project, s: SheetId, ids: &[PointId]) -> Vec<PointMm> {
    let sh = p.sheet(s).unwrap();
    ids.iter().map(|id| sh.position(*id).unwrap()).collect()
}

fn same(a: PointMm, b: PointMm, tol: f64) -> bool {
    distance_mm(a, b) <= tol
}

fn length_sum(p: &Project, s: SheetId, ents: &[EntityId]) -> f64 {
    let sh = p.sheet(s).unwrap();
    ents.iter().map(|e| sh.path_length_mm(*e).unwrap()).sum()
}

fn extract_at(p: &mut Project, s: SheetId, at: PointMm) -> Result<PieceId, CoreError> {
    p.apply(Command::ExtractPiece {
        sheet: s,
        source: PieceSource::Click { at },
        name: "P".into(),
        detached: false,
    })
    .map(|o| o.pieces[0])
}

fn refused(p: &mut Project, c: Command) -> CoreError {
    let before = snap(p);
    let err = p.apply(c).expect_err("doit être refusée");
    assert!(!err.to_string().is_empty());
    assert_eq!(snap(p), before, "un refus ne laisse aucune trace");
    err
}

// ------------------------------------------------------------------ CA1

#[test]
fn ca1_rectangle_100_par_50_calcule_a_la_main() {
    let (mut p, s) = setup();
    let out = p
        .apply(oriented(s, rect(10.0, 20.0, 100.0, 50.0, 0.0, false)))
        .unwrap();
    assert_eq!((out.points.len(), out.entities.len()), (4, 4));
    let pos = positions(&p, s, &out.points);
    for (got, want) in pos.iter().zip([
        pt(10.0, 20.0),
        pt(110.0, 20.0),
        pt(110.0, 70.0),
        pt(10.0, 70.0),
    ]) {
        assert!(same(*got, want, 1e-12), "{got:?} != {want:?}");
    }
    assert!((length_sum(&p, s, &out.entities) - 300.0).abs() < 1e-9);
    let piece = extract_at(&mut p, s, pt(50.0, 40.0)).unwrap();
    let c = p.piece_contour(piece).unwrap();
    assert!((c.area_mm2 - 5000.0).abs() < 1e-9 && (c.perimeter_mm - 300.0).abs() < 1e-9);
}

#[test]
fn ca1_rotation_de_90_degres_autour_de_l_origine_du_rectangle() {
    // Convention : la rotation se fait autour du premier coin (ici (10 ; 20)).
    let (mut p, s) = setup();
    let out = p
        .apply(oriented(s, rect(10.0, 20.0, 100.0, 50.0, 90.0, false)))
        .unwrap();
    let want = [
        pt(10.0, 20.0),
        pt(10.0, 120.0),
        pt(-40.0, 120.0),
        pt(-40.0, 20.0),
    ];
    for (got, w) in positions(&p, s, &out.points).iter().zip(want) {
        assert!(same(*got, w, 1e-12), "{got:?} != {w:?}");
    }
    // Le même rectangle tourné autour de l'origine du plan (0 ; 0) : origine (-20 ; 10).
    let (mut q, t) = setup();
    let out = q
        .apply(oriented(t, rect(-20.0, 10.0, 100.0, 50.0, 90.0, false)))
        .unwrap();
    let want = [
        pt(-20.0, 10.0),
        pt(-20.0, 110.0),
        pt(-70.0, 110.0),
        pt(-70.0, 10.0),
    ];
    for (got, w) in positions(&q, t, &out.points).iter().zip(want) {
        assert!(same(*got, w, 1e-12), "{got:?} != {w:?}");
    }
}

#[test]
fn ca1_angle_normalise_et_quelconque() {
    // 450° = 90° ; -90° = 270° ; 30° : coins calculés à la main (cos 30° = √3/2).
    let (mut a, sa) = setup();
    let (mut b, sb) = setup();
    let pa = a
        .apply(oriented(sa, rect(0.0, 0.0, 10.0, 4.0, 450.0, false)))
        .unwrap();
    let pb = b
        .apply(oriented(sb, rect(0.0, 0.0, 10.0, 4.0, 90.0, false)))
        .unwrap();
    for (x, y) in positions(&a, sa, &pa.points)
        .iter()
        .zip(positions(&b, sb, &pb.points))
    {
        assert!(same(*x, y, 1e-12));
    }
    let (mut c, sc) = setup();
    let out = c
        .apply(oriented(sc, rect(0.0, 0.0, 10.0, 4.0, -90.0, false)))
        .unwrap();
    let want = [pt(0.0, 0.0), pt(0.0, -10.0), pt(4.0, -10.0), pt(4.0, 0.0)];
    for (got, w) in positions(&c, sc, &out.points).iter().zip(want) {
        assert!(same(*got, w, 1e-12), "{got:?} != {w:?}");
    }
    let (mut d, sd) = setup();
    let out = d
        .apply(oriented(sd, rect(0.0, 0.0, 10.0, 4.0, 30.0, false)))
        .unwrap();
    let (c30, s30) = (3f64.sqrt() / 2.0, 0.5);
    let want = [
        pt(0.0, 0.0),
        pt(10.0 * c30, 10.0 * s30),
        pt(10.0 * c30 - 4.0 * s30, 10.0 * s30 + 4.0 * c30),
        pt(-4.0 * s30, 4.0 * c30),
    ];
    for (got, w) in positions(&d, sd, &out.points).iter().zip(want) {
        assert!(same(*got, w, 1e-12), "{got:?} != {w:?}");
    }
}

#[test]
fn ca1_rectangle_centre() {
    let (mut p, s) = setup();
    let out = p
        .apply(oriented(s, rect(0.0, 0.0, 100.0, 50.0, 0.0, true)))
        .unwrap();
    let want = [
        pt(-50.0, -25.0),
        pt(50.0, -25.0),
        pt(50.0, 25.0),
        pt(-50.0, 25.0),
    ];
    for (got, w) in positions(&p, s, &out.points).iter().zip(want) {
        assert!(same(*got, w, 1e-12), "{got:?} != {w:?}");
    }
    // Centré et tourné de 90° : P0 = (25 ; -50), P1 = (25 ; 50), P2 = (-25 ; 50), P3 = (-25 ; -50).
    let (mut q, t) = setup();
    let out = q
        .apply(oriented(t, rect(0.0, 0.0, 100.0, 50.0, 90.0, true)))
        .unwrap();
    let want = [
        pt(25.0, -50.0),
        pt(25.0, 50.0),
        pt(-25.0, 50.0),
        pt(-25.0, -50.0),
    ];
    for (got, w) in positions(&q, t, &out.points).iter().zip(want) {
        assert!(same(*got, w, 1e-12), "{got:?} != {w:?}");
    }
}

/// Jupe : 5 sommets, aire par la formule des lacets (calculée à la main : somme 4100 / 2).
const SKIRT: [(f64, f64); 5] = [
    (0.0, 0.0),
    (40.0, 0.0),
    (50.0, 30.0),
    (20.0, 50.0),
    (-10.0, 25.0),
];

#[test]
fn ca1_polygone_ferme_cinq_sommets_et_ouvert() {
    let (mut p, s) = setup();
    let out = p.apply(polygon(s, &SKIRT, true)).unwrap();
    assert_eq!((out.points.len(), out.entities.len()), (5, 5));
    // Côtés : 40 ; √1000 ; √1300 ; √1525 ; √725 (différences calculées à la main).
    let per = 40.0 + 1000f64.sqrt() + 1300f64.sqrt() + 1525f64.sqrt() + 725f64.sqrt();
    assert!((length_sum(&p, s, &out.entities) - per).abs() < 1e-9);
    let piece = extract_at(&mut p, s, pt(25.0, 20.0)).unwrap();
    let c = p.piece_contour(piece).unwrap();
    assert!((c.area_mm2 - 2050.0).abs() < 1e-9, "aire {}", c.area_mm2);
    assert!((c.perimeter_mm - per).abs() < 1e-9);
    // Ouvert : N − 1 côtés, rien entre le dernier et le premier.
    let (mut q, t) = setup();
    let out = q.apply(polygon(t, &SKIRT, false)).unwrap();
    assert_eq!((out.points.len(), out.entities.len()), (5, 4));
    assert!((length_sum(&q, t, &out.entities) - (per - 725f64.sqrt())).abs() < 1e-9);
    // Deux sommets : un seul côté.
    let (mut r, u) = setup();
    let out = r
        .apply(polygon(u, &[(0.0, 0.0), (3.0, 4.0)], false))
        .unwrap();
    assert_eq!(out.entities.len(), 1);
    assert!((length_sum(&r, u, &out.entities) - 5.0).abs() < 1e-12);
}

#[test]
fn ca1_polygone_fusionne_avec_un_point_existant() {
    let (mut p, s) = setup();
    p.apply(Command::AddPoint {
        sheet: s,
        at: pt(40.0, 0.0),
    })
    .unwrap();
    let out = p.apply(polygon(s, &SKIRT, true)).unwrap();
    assert_eq!(out.points.len(), 4, "le sommet (40 ; 0) existe déjà");
    assert_eq!(out.entities.len(), 5);
}

// ------------------------------------------------------------------ CA2

/// Arcs et segments du contour dans l'ordre de création : tangentes (entrée, sortie) unitaires.
fn tangents(p: &Project, s: SheetId, e: EntityId) -> ((f64, f64), (f64, f64)) {
    let sh = p.sheet(s).unwrap();
    match &sh.entity(e).unwrap().kind {
        EntityKind::Segment { from, to, .. } => {
            let (a, b) = (sh.position(*from).unwrap(), sh.position(*to).unwrap());
            let l = distance_mm(a, b);
            let d = ((b.x - a.x) / l, (b.y - a.y) / l);
            (d, d)
        }
        EntityKind::Arc {
            center,
            start,
            end,
            ccw,
        } => {
            assert!(*ccw, "arcs anti-horaires");
            let c = sh.position(*center).unwrap();
            let t = |q: PointId| {
                let v = sh.position(q).unwrap();
                let r = distance_mm(c, v);
                (-(v.y - c.y) / r, (v.x - c.x) / r)
            };
            (t(*start), t(*end))
        }
        other => panic!("tracé inattendu {other:?}"),
    }
}

fn angle_between_deg(a: (f64, f64), b: (f64, f64)) -> f64 {
    let cross = a.0 * b.1 - a.1 * b.0;
    let dot = a.0 * b.0 + a.1 * b.1;
    cross.abs().atan2(dot).to_degrees()
}

fn endpoints(p: &Project, s: SheetId, e: EntityId) -> (PointId, PointId) {
    match &p.sheet(s).unwrap().entity(e).unwrap().kind {
        EntityKind::Segment { from, to, .. } => (*from, *to),
        EntityKind::Arc { start, end, .. } => (*start, *end),
        other => panic!("tracé inattendu {other:?}"),
    }
}

/// Chaque raccord : même point, tangentes (sortie du précédent, entrée du suivant) à < 1e-6°.
fn assert_smooth_closed_chain(p: &Project, s: SheetId, ents: &[EntityId]) -> usize {
    let n = ents.len();
    for i in 0..n {
        let (a, b) = (ents[i], ents[(i + 1) % n]);
        let (_, a_end) = endpoints(p, s, a);
        let (b_start, _) = endpoints(p, s, b);
        assert_eq!(a_end, b_start, "raccord {i} : points différents");
        let ang = angle_between_deg(tangents(p, s, a).1, tangents(p, s, b).0);
        assert!(ang < 1e-6, "raccord {i} : angle {ang}°");
    }
    n
}

#[test]
fn ca2_rectangle_arrondi_100_par_70_rayon_5() {
    let (mut p, s) = setup();
    let out = p
        .apply(rounded(
            s,
            rect(0.0, 0.0, 100.0, 70.0, 0.0, false),
            5.0,
            false,
        ))
        .unwrap();
    assert_eq!(out.entities.len(), 8, "4 segments + 4 arcs");
    // 2·(100 − 10) + 2·(70 − 10) + 2π·5 = 300 + 10π = 331,4159265…
    let per = 300.0 + 10.0 * PI;
    assert!((per - 331.415_926_535_897_9).abs() < 1e-12);
    assert!((length_sum(&p, s, &out.entities) - per).abs() < 1e-9);
    assert_eq!(assert_smooth_closed_chain(&p, s, &out.entities), 8);
    // Quatre arcs de 90° (r = 5) et quatre segments (90, 60, 90, 60).
    let sh = p.sheet(s).unwrap();
    let mut lens: Vec<f64> = out
        .entities
        .iter()
        .map(|e| sh.path_length_mm(*e).unwrap())
        .collect();
    lens.sort_by(f64::total_cmp);
    for l in &lens[..4] {
        assert!((l - 2.5 * PI).abs() < 1e-9, "quart d'arc {l}");
    }
    for (l, w) in lens[4..].iter().zip([60.0, 60.0, 90.0, 90.0]) {
        assert!((l - w).abs() < 1e-9);
    }
    // Aire : 7000 − (4 − π)·25 = 6978,5398163…
    let piece = extract_at(&mut p, s, pt(50.0, 35.0)).unwrap();
    let c = p.piece_contour(piece).unwrap();
    let area = 7000.0 - (4.0 - PI) * 25.0;
    assert!((area - 6_978.539_816_339_745).abs() < 1e-9);
    assert!((c.area_mm2 - area).abs() <= 1e-6 * area);
    assert!((c.perimeter_mm - per).abs() <= 1e-6 * per);
}

#[test]
fn ca2_rectangle_arrondi_tourne_et_centre_reste_tangent() {
    let (mut p, s) = setup();
    let out = p
        .apply(rounded(
            s,
            rect(30.0, -40.0, 100.0, 70.0, 33.0, true),
            12.5,
            true,
        ))
        .unwrap();
    assert_eq!(out.entities.len(), 8);
    assert_smooth_closed_chain(&p, s, &out.entities);
    let per = 2.0 * (100.0 - 25.0) + 2.0 * (70.0 - 25.0) + 2.0 * PI * 12.5;
    assert!((length_sum(&p, s, &out.entities) - per).abs() < 1e-9);
}

#[test]
fn ca2_stade_deux_demi_cercles_et_deux_segments() {
    let (mut p, s) = setup();
    let out = p
        .apply(rounded(
            s,
            rect(0.0, 0.0, 100.0, 70.0, 0.0, false),
            35.0,
            false,
        ))
        .unwrap();
    assert_eq!(out.entities.len(), 4, "2 demi-cercles + 2 segments");
    let sh = p.sheet(s).unwrap();
    let arcs: Vec<f64> = out
        .entities
        .iter()
        .filter(|e| matches!(sh.entity(**e).unwrap().kind, EntityKind::Arc { .. }))
        .map(|e| sh.path_length_mm(*e).unwrap())
        .collect();
    assert_eq!(arcs.len(), 2);
    for l in arcs {
        assert!((l - 35.0 * PI).abs() < 1e-9, "demi-cercle {l}");
    }
    // 2·(100 − 70) + 2π·35 = 60 + 70π ; aire = 100·70 − (4 − π)·35².
    let per = 60.0 + 70.0 * PI;
    assert!((length_sum(&p, s, &out.entities) - per).abs() < 1e-9);
    assert_smooth_closed_chain(&p, s, &out.entities);
    let piece = extract_at(&mut p, s, pt(50.0, 35.0)).unwrap();
    let c = p.piece_contour(piece).unwrap();
    let area = 7000.0 - (4.0 - PI) * 1225.0;
    assert!((c.area_mm2 - area).abs() <= 1e-6 * area);
    assert!((c.perimeter_mm - per).abs() <= 1e-6 * per);
    // Deux centres seulement.
    assert_eq!(out.points.len(), 6);
}

#[test]
fn ca2_stade_vertical_et_cercle() {
    // Stade vertical : w = 2r < h.
    let (mut p, s) = setup();
    let out = p
        .apply(rounded(
            s,
            rect(0.0, 0.0, 40.0, 100.0, 0.0, false),
            20.0,
            false,
        ))
        .unwrap();
    assert_eq!(out.entities.len(), 4);
    assert_smooth_closed_chain(&p, s, &out.entities);
    assert!((length_sum(&p, s, &out.entities) - (120.0 + 40.0 * PI)).abs() < 1e-9);
    // Carré de 40 et r = 20 : un cercle (deux demi-cercles de même centre).
    let (mut q, t) = setup();
    let out = q
        .apply(rounded(
            t,
            rect(0.0, 0.0, 40.0, 40.0, 0.0, false),
            20.0,
            false,
        ))
        .unwrap();
    assert_eq!(out.entities.len(), 2);
    assert_eq!(out.points.len(), 3, "un centre et deux extrémités");
    assert!((length_sum(&q, t, &out.entities) - 40.0 * PI).abs() < 1e-9);
    assert_smooth_closed_chain(&q, t, &out.entities);
}

#[test]
fn ca2_refus_du_rayon() {
    let (mut p, s) = setup();
    let r = rect(0.0, 0.0, 100.0, 70.0, 0.0, false);
    for bad in [35.1, 36.0, 0.0, -1.0, f64::NAN, f64::INFINITY, 1e-7] {
        refused(&mut p, rounded(s, r, bad, false));
    }
    // r = 0 propose le rectangle simple.
    let msg = refused(&mut p, rounded(s, r, 0.0, false)).to_string();
    assert!(msg.contains("rectangle simple"), "{msg}");
    // Côté droit restant de 5e-7 mm : indiscernable d'un point, refusé avec explication.
    refused(&mut p, rounded(s, r, 35.0 - 2.5e-7, false));
}

// ------------------------------------------------------------------ CA3

#[test]
fn ca3_extract_piece_trouve_la_region_sans_reglage() {
    let (mut p, s) = setup();
    p.apply(rounded(
        s,
        rect(-20.0, 5.0, 60.0, 30.0, 17.0, true),
        8.0,
        true,
    ))
    .unwrap();
    // Le centre du rectangle est le point (−20 ; 5) (centré).
    let id = extract_at(&mut p, s, pt(-20.0, 5.0)).unwrap();
    let c = p.piece_contour(id).unwrap();
    assert!(c.closed && !c.self_intersecting);
    let area = 60.0 * 30.0 - (4.0 - PI) * 64.0;
    let per = 2.0 * (60.0 - 16.0) + 2.0 * (30.0 - 16.0) + 2.0 * PI * 8.0;
    assert!((c.area_mm2 - area).abs() <= 1e-6 * area);
    assert!((c.perimeter_mm - per).abs() <= 1e-6 * per);
    assert_eq!(p.piece(id).unwrap().outline.len(), 8);
}

// ------------------------------------------------------------------ CA4

#[test]
fn ca4_une_seule_etape_annulable_octets_identiques() {
    let (p0, s) = setup();
    let cmds = [
        polygon(s, &SKIRT, true),
        oriented(s, rect(0.0, 0.0, 10.0, 5.0, 12.0, true)),
        rounded(s, rect(0.0, 0.0, 100.0, 70.0, 0.0, false), 5.0, true),
    ];
    for c in cmds {
        let mut doc = Document::new(p0.clone());
        let before = snap(doc.project());
        let out = doc.execute(c).unwrap();
        assert!(!out.entities.is_empty(), "jamais un no-op");
        let after = snap(doc.project());
        assert_ne!(before, after);
        assert!(doc.undo());
        assert_eq!(snap(doc.project()), before, "annuler = octets identiques");
        assert!(!doc.can_undo(), "une seule étape");
        assert!(doc.redo());
        assert_eq!(snap(doc.project()), after, "rétablir = octets identiques");
    }
}

#[test]
fn ca4_refus_sans_trace() {
    let (mut p, s) = setup();
    p.apply(Command::AddLayer {
        sheet: s,
        name: "Verrou".into(),
    })
    .unwrap();
    let locked = p.sheet(s).unwrap().layers[0].id;
    let r = rect(0.0, 0.0, 100.0, 70.0, 0.0, false);
    let nan = f64::NAN;
    let many: Vec<(f64, f64)> = (0..201)
        .map(|i| (i as f64 * 3.0, (i % 2) as f64 * 7.0))
        .collect();
    let cases = vec![
        polygon(s, &[(0.0, nan), (1.0, 1.0), (5.0, 0.0)], true),
        polygon(s, &[(0.0, 0.0), (f64::INFINITY, 1.0), (5.0, 0.0)], true),
        polygon(s, &[(0.0, 0.0), (2e6, 1.0), (5.0, 0.0)], true),
        polygon(s, &[(0.0, 0.0), (0.0, 0.0), (5.0, 0.0)], true),
        polygon(s, &[(0.0, 0.0), (5e-7, 0.0), (5.0, 0.0)], true),
        polygon(s, &[(0.0, 0.0), (5.0, 0.0), (5.0, 5.0), (5.0, 5.0)], false),
        polygon(s, &[(0.0, 0.0), (5.0, 0.0), (5.0, 5.0), (5e-7, 0.0)], true),
        polygon(s, &[(0.0, 0.0), (5.0, 0.0)], true),
        polygon(s, &[(0.0, 0.0)], false),
        polygon(s, &[], false),
        polygon(s, &many, false),
        oriented(s, rect(0.0, 0.0, 0.0, 5.0, 0.0, false)),
        oriented(s, rect(0.0, 0.0, 5.0, -2.0, 0.0, false)),
        oriented(s, rect(0.0, 0.0, 5.0, 1e-7, 0.0, false)),
        oriented(s, rect(0.0, 0.0, nan, 5.0, 0.0, false)),
        oriented(s, rect(0.0, 0.0, 5.0, 5.0, nan, false)),
        oriented(s, rect(nan, 0.0, 5.0, 5.0, 0.0, false)),
        oriented(s, rect(9e5, 0.0, 2e5, 5.0, 0.0, false)),
        rounded(s, r, 36.0, false),
        rounded(s, r, 0.0, false),
        rounded(s, r, nan, true),
        rounded(s, rect(0.0, 0.0, 0.0, 5.0, 0.0, false), 1.0, false),
        Command::AddOrientedRectangle {
            sheet: s,
            rect: r,
            role: PathRole::Construction,
            layer: Some(LayerId(987_654)),
        },
        oriented(SheetId(987_654), r),
    ];
    for c in cases {
        refused(&mut p, c);
    }
    // Calque verrouillé : refus du verrou, sans trace.
    p.apply(Command::SetLayerLocked {
        sheet: s,
        layer: locked,
        locked: true,
    })
    .unwrap();
    for c in [
        Command::AddPolygon {
            sheet: s,
            vertices: vec![PointRef::New(pt(0.0, 0.0)), PointRef::New(pt(5.0, 0.0))],
            closed: false,
            role: PathRole::Construction,
            layer: Some(locked),
        },
        Command::AddOrientedRectangle {
            sheet: s,
            rect: r,
            role: PathRole::Construction,
            layer: Some(locked),
        },
        Command::AddRoundedRectangle {
            sheet: s,
            rect: r,
            radius_mm: 5.0,
            hide_centers: false,
            role: PathRole::Construction,
            layer: Some(locked),
        },
    ] {
        let err = refused(&mut p, c);
        assert!(matches!(err, CoreError::LayerLocked(_)), "{err}");
    }
    // Deuxième exécution identique : segments déjà tracés, refus (jamais un no-op).
    p.apply(oriented(s, r)).unwrap();
    refused(&mut p, oriented(s, r));
}

// ------------------------------------------------------------------ CA5

#[test]
fn ca5_calque_role_groupe_et_fichier() {
    let (mut p, s) = setup();
    let layer = p
        .apply(Command::AddLayer {
            sheet: s,
            name: "Coupe".into(),
        })
        .unwrap()
        .layers[0];
    let out = p
        .apply(Command::AddRoundedRectangle {
            sheet: s,
            rect: rect(0.0, 0.0, 100.0, 70.0, 0.0, false),
            radius_mm: 5.0,
            hide_centers: true,
            role: PathRole::CuttingContour,
            layer: Some(layer),
        })
        .unwrap();
    let sh = p.sheet(s).unwrap();
    for e in &out.entities {
        let e = sh.entity(*e).unwrap();
        assert_eq!((e.role, e.layer), (PathRole::CuttingContour, Some(layer)));
    }
    // Centres : calque masqué des centres ; les autres points dans le calque demandé.
    let centers = sh
        .layers
        .iter()
        .find(|l| l.name == CENTERS_LAYER_NAME)
        .unwrap();
    assert!(!centers.visible);
    for id in &out.points {
        let pt_ = sh.point(*id).unwrap();
        assert!(pt_.layer == Some(layer) || pt_.layer == Some(centers.id));
    }
    // Un groupe sur le résultat.
    let g = p
        .apply(Command::CreateGroup {
            sheet: s,
            entities: out.entities.clone(),
        })
        .unwrap();
    assert_eq!(g.groups.len(), 1);
    // Sauvegarde puis relecture identique.
    let json = persist::to_json(&p).unwrap();
    let back = persist::from_json(&json).unwrap();
    assert_eq!(snap(&back), snap(&p));
    assert_eq!(persist::to_json(&back).unwrap(), json);
}

#[test]
fn ca5_copie_de_feuille_presse_papiers_et_symetrie() {
    let (mut p, s) = setup();
    let out = p
        .apply(rounded(
            s,
            rect(0.0, 0.0, 100.0, 70.0, 0.0, false),
            5.0,
            true,
        ))
        .unwrap();
    let per = 300.0 + 10.0 * PI;
    // Copie de feuille : la copie garde ses 8 tracés, ses centres et son calque masqué.
    p.apply(Command::DuplicateSheet { sheet: s }).unwrap();
    assert_eq!(p.sheets.len(), 2);
    let copy = &p.sheets[1];
    assert_eq!(copy.entities.len(), 8);
    let hidden: Vec<_> = copy.layers.iter().filter(|l| !l.visible).collect();
    assert_eq!(hidden.len(), 1);
    let hidden_points = copy
        .points
        .iter()
        .filter(|q| q.layer == Some(hidden[0].id))
        .count();
    assert_eq!(
        hidden_points, 4,
        "les centres restent masqués dans la copie"
    );
    p.validate().unwrap();
    // Presse-papiers.
    let (mut q, t) = setup();
    let out2 = q
        .apply(rounded(
            t,
            rect(0.0, 0.0, 100.0, 70.0, 0.0, false),
            5.0,
            false,
        ))
        .unwrap();
    let frag = q.copy_fragment(t, &[], &out2.entities, &[], &[]).unwrap();
    let pasted = q
        .apply(Command::PasteFragment {
            sheet: t,
            fragment: Box::new(frag),
            layer: None,
        })
        .unwrap();
    assert_eq!(pasted.entities.len(), 8);
    assert!((length_sum(&q, t, &pasted.entities) - per).abs() < 1e-9);
    q.validate().unwrap();
    // Symétrie (copie transformée) : aire et longueurs conservées.
    let dup = p
        .apply(Command::Duplicate {
            sheet: s,
            entities: out.entities.clone(),
            transform: Transform::Mirror {
                axis_point: pt(-10.0, 0.0),
                axis_angle_deg: 90.0,
            },
        })
        .unwrap();
    assert_eq!(dup.entities.len(), 8);
    assert!((length_sum(&p, s, &dup.entities) - per).abs() < 1e-9);
    p.validate().unwrap();
    let id = extract_at_nth(&mut p, s, pt(-70.0, 35.0));
    let c = p.piece_contour(id).unwrap();
    let area = 7000.0 - (4.0 - PI) * 25.0;
    assert!((c.area_mm2 - area).abs() <= 1e-6 * area);
}

fn extract_at_nth(p: &mut Project, s: SheetId, at: PointMm) -> PieceId {
    extract_at(p, s, at).unwrap()
}

// ------------------------------------------------------------------ CA6

#[test]
fn ca6_centres_comptes_et_calque_masque_signale() {
    let (mut p, s) = setup();
    let out = p
        .apply(rounded(
            s,
            rect(0.0, 0.0, 100.0, 70.0, 0.0, false),
            5.0,
            true,
        ))
        .unwrap();
    // 12 points : 4 débuts, 4 fins, 4 centres ; jamais plus de 4 centres.
    assert_eq!(out.points.len(), 12);
    let sh = p.sheet(s).unwrap();
    let hidden: Vec<PointId> = out
        .points
        .iter()
        .copied()
        .filter(|id| !sh.point_visible(*id))
        .collect();
    assert_eq!(hidden.len(), 4, "exactement les 4 centres sont masqués");
    for id in &hidden {
        let c = sh.position(*id).unwrap();
        let ok = [pt(5.0, 5.0), pt(95.0, 5.0), pt(95.0, 65.0), pt(5.0, 65.0)]
            .iter()
            .any(|w| same(c, *w, 1e-12));
        assert!(ok, "centre {c:?}");
    }
    assert!(
        out.notes
            .iter()
            .any(|n| n.contains(CENTERS_LAYER_NAME) && n.contains("masqué")),
        "le masquage est annoncé : {:?}",
        out.notes
    );
    // Sans option : centres visibles, aucun calque créé.
    let (mut q, t) = setup();
    let out = q
        .apply(rounded(
            t,
            rect(0.0, 0.0, 100.0, 70.0, 0.0, false),
            5.0,
            false,
        ))
        .unwrap();
    assert!(out
        .points
        .iter()
        .all(|id| q.sheet(t).unwrap().point_visible(*id)));
    assert!(q.sheet(t).unwrap().layers.is_empty() && out.notes.is_empty());
    // Un second rectangle réutilise le même calque des centres.
    p.apply(rounded(
        s,
        rect(200.0, 0.0, 100.0, 70.0, 0.0, false),
        5.0,
        true,
    ))
    .unwrap();
    let n = p
        .sheet(s)
        .unwrap()
        .layers
        .iter()
        .filter(|l| l.name == CENTERS_LAYER_NAME)
        .count();
    assert_eq!(n, 1);
    // Réafficher le calque montre les centres.
    let layer = p.sheet(s).unwrap().layers[0].id;
    p.apply(Command::SetLayerVisible {
        sheet: s,
        layer,
        visible: true,
    })
    .unwrap();
    assert!(p
        .sheet(s)
        .unwrap()
        .points
        .iter()
        .all(|q| p.sheet(s).unwrap().point_visible(q.id)));
}

#[test]
fn ca6_polygone_sans_aucun_centre_et_stade_deux_centres() {
    let (mut p, s) = setup();
    let out = p
        .apply(rounded(
            s,
            rect(0.0, 0.0, 100.0, 70.0, 0.0, false),
            35.0,
            true,
        ))
        .unwrap();
    let sh = p.sheet(s).unwrap();
    assert_eq!(
        out.points
            .iter()
            .filter(|id| !sh.point_visible(**id))
            .count(),
        2
    );
}
