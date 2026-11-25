//! US-04.6 — séparer une pièce par une ligne, assembler deux pièces le long de deux
//! bords. Un groupe de tests par critère d'acceptation :
//! CA1 séparation exacte (aires, périmètres, sommets calculés à la main),
//! CA2 assemblage (tolérance, écart affiché, aller-retour),
//! CA3 refus sans trace, CA4 une étape annulable, CA5 octets inchangés,
//! CA6 sérialisation et cohérence avec pièces dérivées, éléments internes, placement.

use pf_core::persist;
use pf_core::piece::PieceIssue;
use pf_core::piece_internal::InternalGeometry;
use pf_core::*;
use pf_geometry::{distance_mm, PointMm};
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

fn extract_paths(p: &mut Project, sheet: SheetId, entities: Vec<EntityId>, name: &str) -> PieceId {
    p.apply(Command::ExtractPiece {
        sheet,
        source: PieceSource::Paths { entities },
        name: name.into(),
        detached: false,
    })
    .unwrap()
    .pieces[0]
}

/// Rectangle `w × h` de coin `(x, y)`, extrait en pièce liée.
fn rect_piece(
    p: &mut Project,
    sheet: SheetId,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    name: &str,
) -> PieceId {
    let o = p
        .apply(Command::AddRectangle {
            sheet,
            corner: PointRef::New(pt(x, y)),
            width_mm: w,
            height_mm: h,
        })
        .unwrap();
    extract_paths(p, sheet, o.entities, name)
}

fn segment(p: &mut Project, sheet: SheetId, a: PointMm, b: PointMm) -> EntityId {
    p.apply(Command::AddSegment {
        sheet,
        from: PointRef::New(a),
        to: PointRef::New(b),
    })
    .unwrap()
    .entities[0]
}

fn curve(
    p: &mut Project,
    sheet: SheetId,
    a: PointMm,
    c1: PointMm,
    c2: PointMm,
    b: PointMm,
) -> EntityId {
    p.apply(Command::AddCurve {
        sheet,
        start: PointRef::New(a),
        c1,
        c2,
        end: PointRef::New(b),
    })
    .unwrap()
    .entities[0]
}

/// Polygone fermé de segments, extrait en pièce liée.
fn poly_piece(p: &mut Project, sheet: SheetId, pts: &[PointMm], name: &str) -> PieceId {
    let n = pts.len();
    let ids = (0..n)
        .map(|i| segment(p, sheet, pts[i], pts[(i + 1) % n]))
        .collect();
    extract_paths(p, sheet, ids, name)
}

/// Dôme : segment (0,0)-(100,0) et Bézier (100,0) (100,100) (0,100) (0,0). Aire 6000 mm².
fn dome_piece(
    p: &mut Project,
    sheet: SheetId,
    f: impl Fn(PointMm) -> PointMm,
    name: &str,
) -> PieceId {
    let a = segment(p, sheet, f(pt(0.0, 0.0)), f(pt(100.0, 0.0)));
    let b = curve(
        p,
        sheet,
        f(pt(100.0, 0.0)),
        f(pt(100.0, 100.0)),
        f(pt(0.0, 100.0)),
        f(pt(0.0, 0.0)),
    );
    extract_paths(p, sheet, vec![a, b], name)
}

fn setup() -> (Project, SheetId) {
    let p = Project::new("séparer");
    let s = p.sheets[0].id;
    (p, s)
}

fn ends_of(shape: &Shape) -> (PointMm, PointMm) {
    match shape {
        Shape::Line(a, b) => (*a, *b),
        Shape::Cubic(c) => (c.p0, c.p3),
        Shape::Arc(a) => (a.start_point(), a.end_point()),
        other => panic!("forme inattendue {other:?}"),
    }
}

/// Début de chaque arête du contour d'une pièce, dans l'ordre du contour.
fn corners(p: &Project, id: PieceId) -> Vec<PointMm> {
    let sheet = p.piece_sheet(id).unwrap();
    p.piece(id)
        .unwrap()
        .outline
        .iter()
        .map(|e| {
            let (a, b) = ends_of(&sheet.shape(e.entity).unwrap());
            if e.reversed {
                b
            } else {
                a
            }
        })
        .collect()
}

fn has_corner(corners: &[PointMm], at: PointMm) -> bool {
    corners.iter().any(|c| distance_mm(*c, at) < 1e-9)
}

/// Tracé du contour de `piece` qui relie `a` et `b` (dans un sens ou l'autre).
fn edge_between(p: &Project, piece: PieceId, a: PointMm, b: PointMm) -> EntityId {
    let sheet = p.piece_sheet(piece).unwrap();
    p.piece(piece)
        .unwrap()
        .outline
        .iter()
        .find(|e| {
            let (s, t) = ends_of(&sheet.shape(e.entity).unwrap());
            (distance_mm(s, a) < 1e-6 && distance_mm(t, b) < 1e-6)
                || (distance_mm(s, b) < 1e-6 && distance_mm(t, a) < 1e-6)
        })
        .unwrap_or_else(|| panic!("aucun bord entre {a:?} et {b:?}"))
        .entity
}

fn split(p: &mut Project, piece: PieceId, a: PointMm, b: PointMm) -> Result<Outcome, CoreError> {
    p.apply(Command::SplitPiece { piece, a, b })
}

fn join(
    p: &mut Project,
    first: (PieceId, EntityId),
    second: (PieceId, EntityId),
    tolerance_mm: f64,
) -> Result<Outcome, CoreError> {
    p.apply(Command::JoinPieces {
        first: first.0,
        first_edge: first.1,
        second: second.0,
        second_edge: second.1,
        tolerance_mm,
    })
}

/// Refus : message d'erreur ; le projet est identique (valeur et octets).
fn refused(p: &mut Project, c: Command) -> String {
    let (before, text) = (p.clone(), bytes(p));
    let err = p.apply(c).expect_err("la commande aurait dû être refusée");
    assert_eq!(*p, before, "refus sans trace (valeur)");
    assert_eq!(text, bytes(p), "refus sans trace (octets)");
    err.to_string()
}

/// Dernier tracé du contour : l'arête de coupe d'une moitié.
fn cut_edge(p: &Project, half: PieceId) -> EntityId {
    p.piece(half).unwrap().outline.last().unwrap().entity
}

fn area(p: &Project, id: PieceId) -> f64 {
    p.piece_contour(id).unwrap().area_mm2
}

fn perimeter(p: &Project, id: PieceId) -> f64 {
    p.piece_contour(id).unwrap().perimeter_mm
}

/// Longueur d'arc de la Bézier (100,0) (100,100) (0,100) (0,0) sur `t ∈ [t0 ; t1]`, par la
/// règle de Simpson : indépendant du calcul de la bibliothèque.
fn dome_length(t0: f64, t1: f64) -> f64 {
    let speed = |t: f64| {
        let (dx, dy) = (-600.0 * t * (1.0 - t), 300.0 * (1.0 - 2.0 * t));
        dx.hypot(dy)
    };
    let n = 20_000;
    let h = (t1 - t0) / n as f64;
    let mut sum = speed(t0) + speed(t1);
    for i in 1..n {
        sum += speed(t0 + h * i as f64) * if i % 2 == 1 { 4.0 } else { 2.0 };
    }
    sum * h / 3.0
}

// ======================================================================== CA1

#[test]
fn ca1_rectangle_coupe_verticale_aires_perimetres_sommets() {
    let (mut p, s) = setup();
    let m = rect_piece(&mut p, s, 0.0, 0.0, 100.0, 60.0, "Devant");
    let o = split(&mut p, m, pt(30.0, -5.0), pt(30.0, 5.0)).unwrap();
    assert_eq!(o.pieces.len(), 2);
    let (left, right) = (o.pieces[0], o.pieces[1]);
    // Gauche de la droite orientée vers le haut : x < 30.
    assert!(near(area(&p, left), 1800.0, 1e-9));
    assert!(near(perimeter(&p, left), 180.0, 1e-9));
    assert!(near(area(&p, right), 4200.0, 1e-9));
    assert!(near(perimeter(&p, right), 260.0, 1e-9));
    let (cl, cr) = (corners(&p, left), corners(&p, right));
    assert_eq!(cl.len(), 4);
    for c in [pt(0.0, 0.0), pt(30.0, 0.0), pt(30.0, 60.0), pt(0.0, 60.0)] {
        assert!(has_corner(&cl, c), "{c:?} absent de {cl:?}");
    }
    assert_eq!(cr.len(), 4);
    for c in [
        pt(30.0, 0.0),
        pt(100.0, 0.0),
        pt(100.0, 60.0),
        pt(30.0, 60.0),
    ] {
        assert!(has_corner(&cr, c), "{c:?} absent de {cr:?}");
    }
    for h in [left, right] {
        let c = p.piece_contour(h).unwrap();
        assert!(c.closed && !c.self_intersecting && !c.clockwise);
    }
    // Somme des aires = aire d'origine (1e-9 relatif).
    assert!(near(
        area(&p, left) + area(&p, right),
        6000.0,
        6000.0 * 1e-9
    ));
    // La pièce d'origine est remplacée.
    assert!(p.piece(m).is_err());
    assert_eq!(p.pieces.len(), 2);
    p.validate().unwrap();
}

#[test]
fn ca1_coupe_par_sommets_diagonale_et_droite_inclinee() {
    let (mut p, s) = setup();
    let m = rect_piece(&mut p, s, 0.0, 0.0, 100.0, 60.0, "Devant");
    // Diagonale par deux coins opposés : deux triangles de 3000 mm².
    let o = split(&mut p, m, pt(0.0, 0.0), pt(100.0, 60.0)).unwrap();
    for id in &o.pieces {
        assert!(near(area(&p, *id), 3000.0, 1e-9));
        assert_eq!(corners(&p, *id).len(), 3);
    }
    assert!(near(
        perimeter(&p, o.pieces[0]),
        100.0 + 60.0 + 100.0f64.hypot(60.0),
        1e-9
    ));
    // Droite à 45° qui coupe les côtés bas et gauche : triangle de 8 × 8 / 2.
    let (mut p, s) = setup();
    let m = rect_piece(&mut p, s, 0.0, 0.0, 100.0, 60.0, "Devant");
    let o = split(&mut p, m, pt(8.0, 0.0), pt(0.0, 8.0)).unwrap();
    let mut areas: Vec<f64> = o.pieces.iter().map(|i| area(&p, *i)).collect();
    areas.sort_by(f64::total_cmp);
    assert!(near(areas[0], 32.0, 1e-9) && near(areas[1], 6000.0 - 32.0, 1e-9));
}

#[test]
fn ca1_contour_a_bezier_dome_coupe_en_son_milieu() {
    let (mut p, s) = setup();
    let m = dome_piece(&mut p, s, |q| q, "Dôme");
    assert!(near(area(&p, m), 6000.0, 1e-6));
    // Droite verticale x = 50 : par symétrie, 3000 + 3000 ; la Bézier est coupée en t = 0,5.
    let o = split(&mut p, m, pt(50.0, 0.0), pt(50.0, 1.0)).unwrap();
    let (left, right) = (o.pieces[0], o.pieces[1]);
    assert!(near(area(&p, left), 3000.0, 1e-6));
    assert!(near(area(&p, right), 3000.0, 1e-6));
    // Périmètre : 50 (bord) + 75 (coupe, jusqu'à y = 75) + longueur de la demi-Bézier.
    let half = dome_length(0.0, 0.5);
    assert!(near(dome_length(0.5, 1.0), half, 1e-6), "symétrie du dôme");
    for h in [left, right] {
        assert!(
            near(perimeter(&p, h), 50.0 + 75.0 + half, 1e-4),
            "{}",
            perimeter(&p, h)
        );
        assert!(has_corner(&corners(&p, h), pt(50.0, 75.0)));
        assert!(has_corner(&corners(&p, h), pt(50.0, 0.0)));
    }
    // De Casteljau en t = 0,5 : moitié gauche (50,75) (25,75) (0,50) (0,0).
    let sheet = p.piece_sheet(left).unwrap();
    let c = p
        .piece(left)
        .unwrap()
        .outline
        .iter()
        .find_map(|e| match sheet.shape(e.entity).unwrap() {
            Shape::Cubic(c) => Some(c),
            _ => None,
        })
        .expect("une Bézier dans la moitié gauche");
    for (got, want) in [
        (c.p0, pt(50.0, 75.0)),
        (c.c1, pt(25.0, 75.0)),
        (c.c2, pt(0.0, 50.0)),
        (c.p3, pt(0.0, 0.0)),
    ] {
        assert!(distance_mm(got, want) < 1e-9, "{got:?} ≠ {want:?}");
    }
    p.validate().unwrap();
}

#[test]
fn ca1_somme_des_aires_conservee_pour_beaucoup_de_droites() {
    let (mut p, s) = setup();
    let m = dome_piece(&mut p, s, |q| q, "Dôme");
    let total = area(&p, m);
    let mut cuts = 0;
    for k in 0..40 {
        // Droites de pente et de position variées, certaines hors de la pièce.
        let ang = (k as f64) * 0.157;
        let c = pt(10.0 + (k as f64) * 2.3, 5.0 + (k % 7) as f64 * 9.0);
        let (a, b) = (c, pt(c.x + ang.cos(), c.y + ang.sin()));
        let mut q = p.clone();
        if let Ok(o) = split(&mut q, m, a, b) {
            cuts += 1;
            let sum: f64 = o.pieces.iter().map(|i| area(&q, *i)).sum();
            assert!(near(sum, total, total * 1e-9), "k={k} : {sum} ≠ {total}");
            q.validate().unwrap();
        }
    }
    assert!(cuts >= 15, "trop peu de coupes réussies : {cuts}");
}

#[test]
fn ca1_cercle_coupe_par_un_diametre() {
    let (mut p, s) = setup();
    let o = p
        .apply(Command::AddCircle {
            sheet: s,
            center: PointRef::New(pt(0.0, 0.0)),
            radius_mm: 50.0,
        })
        .unwrap();
    let m = extract_paths(&mut p, s, o.entities, "Rond");
    let o = split(&mut p, m, pt(-1.0, 0.0), pt(1.0, 0.0)).unwrap();
    let half = std::f64::consts::PI * 50.0 * 50.0 / 2.0;
    for id in &o.pieces {
        assert!(near(area(&p, *id), half, half * 1e-9));
        assert!(near(
            perimeter(&p, *id),
            std::f64::consts::PI * 50.0 + 100.0,
            1e-6
        ));
    }
}

#[test]
fn ca1_apercu_donne_les_memes_mesures_que_la_commande() {
    let (mut p, s) = setup();
    let m = rect_piece(&mut p, s, 0.0, 0.0, 100.0, 60.0, "Devant");
    let before = bytes(&p);
    let pre = p.split_preview(m, pt(30.0, -5.0), pt(30.0, 5.0)).unwrap();
    assert_eq!(before, bytes(&p), "l'aperçu ne modifie rien");
    assert!(distance_mm(pre.points[0], pt(30.0, 0.0)) < 1e-9);
    assert!(distance_mm(pre.points[1], pt(30.0, 60.0)) < 1e-9);
    assert!(near(pre.left.area_mm2, 1800.0, 1e-9) && near(pre.right.area_mm2, 4200.0, 1e-9));
    assert!(near(pre.left.perimeter_mm, 180.0, 1e-9));
    assert!(p.split_preview(m, pt(0.0, 100.0), pt(1.0, 100.0)).is_err());
}

#[test]
fn ca1_fiche_heritee_noms_et_codes_deterministes() {
    let (mut p, s) = setup();
    let m = rect_piece(&mut p, s, 0.0, 0.0, 100.0, 60.0, "Devant");
    p.apply(Command::SetPieceCard {
        piece: m,
        name: "Devant".into(),
        code: Some("DV".into()),
        quantity: 3,
        mirrored_pair: true,
        material: Some("Coton".into()),
        category: Some(PieceCategory::Fabric),
        comment: Some("à plat".into()),
    })
    .unwrap();
    let o = split(&mut p, m, pt(30.0, -5.0), pt(30.0, 5.0)).unwrap();
    let names = ["Devant (1/2)", "Devant (2/2)"];
    let codes = ["DV-1", "DV-2"];
    for (k, id) in o.pieces.iter().enumerate() {
        let h = p.piece(*id).unwrap();
        assert_eq!(h.name, names[k]);
        assert_eq!(h.code.as_deref(), Some(codes[k]));
        assert_eq!(h.quantity, 3);
        assert!(h.mirrored_pair);
        assert_eq!(h.material.as_deref(), Some("Coton"));
        assert_eq!(h.category, Some(PieceCategory::Fabric));
        assert_eq!(h.comment.as_deref(), Some("à plat"));
        assert!(h.derived.is_none() && h.placement.is_none());
        assert_eq!(h.link, PieceLink::Detached);
    }
    // Déterminisme : la même commande sur le même projet donne le même projet.
    let (mut q, s2) = setup();
    let m2 = rect_piece(&mut q, s2, 0.0, 0.0, 100.0, 60.0, "Devant");
    q.apply(Command::SetPieceCard {
        piece: m2,
        name: "Devant".into(),
        code: Some("DV".into()),
        quantity: 3,
        mirrored_pair: true,
        material: Some("Coton".into()),
        category: Some(PieceCategory::Fabric),
        comment: Some("à plat".into()),
    })
    .unwrap();
    split(&mut q, m2, pt(30.0, -5.0), pt(30.0, 5.0)).unwrap();
    assert_eq!(bytes(&p), bytes(&q));
    // Un nom de 120 caractères reste valide.
    let (mut r, s3) = setup();
    let long = "N".repeat(120);
    let m3 = rect_piece(&mut r, s3, 0.0, 0.0, 100.0, 60.0, &long);
    let o = split(&mut r, m3, pt(30.0, -5.0), pt(30.0, 5.0)).unwrap();
    assert!(r.piece(o.pieces[0]).unwrap().name.ends_with(" (1/2)"));
    assert!(r.piece(o.pieces[0]).unwrap().name.chars().count() <= 120);
    r.validate().unwrap();
}

// ======================================================================== CA2

#[test]
fn ca2_assemblage_de_deux_rectangles_deplaces_et_tournes() {
    for angle_deg in [0.0_f64, 90.0, 37.0, -123.0] {
        let (mut p, s) = setup();
        let a = rect_piece(&mut p, s, 0.0, 0.0, 100.0, 50.0, "A");
        // B : 100 × 20, posé ailleurs et tourné autour de (300, 300).
        let (sn, cs) = angle_deg.to_radians().sin_cos();
        let f = |q: PointMm| {
            let (dx, dy) = (q.x - 300.0, q.y - 300.0);
            pt(300.0 + cs * dx - sn * dy, 300.0 + sn * dx + cs * dy)
        };
        let b = poly_piece(
            &mut p,
            s,
            &[
                f(pt(500.0, 500.0)),
                f(pt(600.0, 500.0)),
                f(pt(600.0, 520.0)),
                f(pt(500.0, 520.0)),
            ],
            "B",
        );
        let ea = edge_between(&p, a, pt(100.0, 50.0), pt(0.0, 50.0));
        let eb = edge_between(&p, b, f(pt(500.0, 500.0)), f(pt(600.0, 500.0)));
        let sheet0 = serde_json::to_string(&p.sheets[0]).unwrap();
        let o = join(&mut p, (a, ea), (b, eb), 0.5).unwrap();
        // Construction intacte : même feuille, plus une feuille pour la pièce assemblée.
        assert_eq!(sheet0, serde_json::to_string(&p.sheets[0]).unwrap());
        assert_eq!(p.sheets.len(), 2);
        let merged = o.pieces[0];
        assert_eq!(p.pieces.len(), 1);
        assert!(
            near(area(&p, merged), 7000.0, 7000.0 * 1e-9),
            "{angle_deg}°"
        );
        assert!(near(perimeter(&p, merged), 340.0, 1e-6));
        // Sommets : la jonction a disparu mais ses extrémités (0,50) et (100,50) demeurent
        // (le résultat est dans le repère de A).
        let cs = corners(&p, merged);
        for c in [pt(0.0, 0.0), pt(100.0, 0.0), pt(100.0, 70.0), pt(0.0, 70.0)] {
            assert!(
                cs.iter().any(|q| distance_mm(*q, c) < 1e-9),
                "{angle_deg}° : {c:?} dans {cs:?}"
            );
        }
        assert!(cs.iter().any(|q| distance_mm(*q, pt(0.0, 50.0)) < 1e-9));
        assert!(cs.iter().any(|q| distance_mm(*q, pt(100.0, 50.0)) < 1e-9));
        // Écart rapporté même nul.
        assert!(
            o.notes
                .iter()
                .any(|n| n.contains("Écart de longueur") && n.contains("0.000 mm")),
            "{:?}",
            o.notes
        );
        p.validate().unwrap();
    }
}

#[test]
fn ca2_tolerance_ecart_affiche_jamais_masque() {
    let (mut p, s) = setup();
    let a = rect_piece(&mut p, s, 0.0, 0.0, 100.0, 50.0, "A");
    let b = rect_piece(&mut p, s, 500.0, 0.0, 100.3, 20.0, "B");
    let ea = edge_between(&p, a, pt(100.0, 50.0), pt(0.0, 50.0));
    let eb = edge_between(&p, b, pt(500.0, 0.0), pt(600.3, 0.0));
    // Aperçu : écart 0,3 mm affiché, avec ou sans refus.
    let pre = p.join_preview(a, ea, b, eb, 0.5).unwrap();
    assert!(near(pre.first_length_mm, 100.0, 1e-9) && near(pre.second_length_mm, 100.3, 1e-9));
    assert!(near(pre.gap_mm, 0.3, 1e-9) && near(pre.tolerance_mm, 0.5, 0.0));
    assert!(pre.refusal.is_none() && pre.result.is_some());
    assert!(!pre.moved_second.is_empty());
    let pre = p.join_preview(a, ea, b, eb, 0.2).unwrap();
    assert!(near(pre.gap_mm, 0.3, 1e-9));
    assert!(
        pre.refusal.as_deref().is_some_and(|r| r.contains("0.300")),
        "{:?}",
        pre.refusal
    );
    // Refus au-delà de la tolérance, avec l'écart dans le message.
    let msg = refused(
        &mut p,
        Command::JoinPieces {
            first: a,
            first_edge: ea,
            second: b,
            second_edge: eb,
            tolerance_mm: 0.2,
        },
    );
    assert!(
        msg.contains("0.300") && msg.contains("100.300") && msg.contains("0.200"),
        "{msg}"
    );
    // Accepté en dessous, et l'écart est dans les remarques.
    let o = join(&mut p, (a, ea), (b, eb), 0.5).unwrap();
    assert!(
        o.notes
            .iter()
            .any(|n| n.contains("0.300 mm") && n.contains("tolérance 0.500")),
        "{:?}",
        o.notes
    );
    // Limite : un écart égal à la tolérance est accepté.
    let (mut q, s) = setup();
    let a = rect_piece(&mut q, s, 0.0, 0.0, 100.0, 50.0, "A");
    let b = rect_piece(&mut q, s, 500.0, 0.0, 100.5, 20.0, "B");
    let ea = edge_between(&q, a, pt(100.0, 50.0), pt(0.0, 50.0));
    let eb = edge_between(&q, b, pt(500.0, 0.0), pt(600.5, 0.0));
    join(&mut q, (a, ea), (b, eb), 0.5).unwrap();
}

#[test]
fn ca2_aller_retour_separer_puis_assembler_retrouve_l_aire() {
    // Rectangle, dôme (Bézier) et polygone concave à deux traversées.
    let cases: Vec<(&str, PointMm, PointMm)> = vec![
        ("rect", pt(30.0, -5.0), pt(30.0, 5.0)),
        ("rect-oblique", pt(10.0, -5.0), pt(60.0, 40.0)),
        ("dome", pt(50.0, 0.0), pt(50.0, 1.0)),
        ("dome-oblique", pt(20.0, 0.0), pt(70.0, 50.0)),
    ];
    for (kind, a, b) in cases {
        let (mut p, s) = setup();
        let m = if kind.starts_with("rect") {
            rect_piece(&mut p, s, 0.0, 0.0, 100.0, 60.0, "Pièce")
        } else {
            dome_piece(&mut p, s, |q| q, "Pièce")
        };
        let (area0, perim0) = (area(&p, m), perimeter(&p, m));
        let o = split(&mut p, m, a, b).unwrap();
        let (l, r) = (o.pieces[0], o.pieces[1]);
        let (el, er) = (cut_edge(&p, l), cut_edge(&p, r));
        let j = join(&mut p, (l, el), (r, er), 0.001).unwrap();
        assert_eq!(p.pieces.len(), 1, "{kind}");
        let back = j.pieces[0];
        assert!(
            near(area(&p, back), area0, area0 * 1e-9),
            "{kind} : {} ≠ {area0}",
            area(&p, back)
        );
        assert!(near(perimeter(&p, back), perim0, perim0 * 1e-6), "{kind}");
        let c = p.piece_contour(back).unwrap();
        assert!(c.closed && !c.self_intersecting);
        p.validate().unwrap();
    }
}

#[test]
fn ca2_assemblage_de_bords_courbes_identiques() {
    // A : dôme. B : la région au-dessus de la courbe, déplacée et tournée dans le plan.
    let (mut p, s) = setup();
    let a = dome_piece(&mut p, s, |q| q, "Dôme");
    let f = |q: PointMm| {
        let (sn, cs) = 0.8f64.sin_cos();
        pt(500.0 + cs * q.x - sn * q.y, 300.0 + sn * q.x + cs * q.y)
    };
    let c = curve(
        &mut p,
        s,
        f(pt(0.0, 0.0)),
        f(pt(0.0, 100.0)),
        f(pt(100.0, 100.0)),
        f(pt(100.0, 0.0)),
    );
    let l1 = segment(&mut p, s, f(pt(100.0, 0.0)), f(pt(100.0, 150.0)));
    let l2 = segment(&mut p, s, f(pt(100.0, 150.0)), f(pt(0.0, 150.0)));
    let l3 = segment(&mut p, s, f(pt(0.0, 150.0)), f(pt(0.0, 0.0)));
    let b = extract_paths(&mut p, s, vec![c, l1, l2, l3], "Dessus");
    assert!(near(area(&p, b), 15000.0 - 6000.0, 1e-6));
    let ea = p
        .piece(a)
        .unwrap()
        .outline
        .iter()
        .map(|e| e.entity)
        .find(|e| {
            matches!(
                p.piece_sheet(a).unwrap().shape(*e).unwrap(),
                Shape::Cubic(_)
            )
        });
    let ea = ea.unwrap();
    let o = join(&mut p, (a, ea), (b, c), 0.01).unwrap();
    // Dôme + dessus = rectangle 100 × 150.
    assert!(near(area(&p, o.pieces[0]), 15000.0, 1e-6));
    assert!(near(perimeter(&p, o.pieces[0]), 500.0, 1e-6));
    // Écart de forme observé quasi nul.
    assert!(
        o.notes.iter().any(|n| n.contains("écart de forme")),
        "{:?}",
        o.notes
    );
    p.validate().unwrap();
}

#[test]
fn ca2_assemblage_un_bord_courbe_contre_un_bord_droit_ou_different_est_refuse() {
    let (mut p, s) = setup();
    let a = dome_piece(&mut p, s, |q| q, "Dôme");
    let b = dome_piece(&mut p, s, |q| pt(q.x + 400.0, q.y), "Autre dôme");
    let (ca, cb) = (
        p.piece(a)
            .unwrap()
            .outline
            .iter()
            .map(|e| e.entity)
            .find(|e| {
                matches!(
                    p.piece_sheet(a).unwrap().shape(*e).unwrap(),
                    Shape::Cubic(_)
                )
            })
            .unwrap(),
        p.piece(b)
            .unwrap()
            .outline
            .iter()
            .map(|e| e.entity)
            .find(|e| {
                matches!(
                    p.piece_sheet(b).unwrap().shape(*e).unwrap(),
                    Shape::Cubic(_)
                )
            })
            .unwrap(),
    );
    // Deux domes bombés du même côté : une fois la jonction faite ils ne coïncident pas.
    let msg = refused(
        &mut p,
        Command::JoinPieces {
            first: a,
            first_edge: ca,
            second: b,
            second_edge: cb,
            tolerance_mm: 1.0,
        },
    );
    assert!(msg.contains("même forme"), "{msg}");
    // Courbe contre droite de même corde.
    let straight = edge_between(&p, b, pt(400.0, 0.0), pt(500.0, 0.0));
    let msg = refused(
        &mut p,
        Command::JoinPieces {
            first: a,
            first_edge: ca,
            second: b,
            second_edge: straight,
            tolerance_mm: 10.0,
        },
    );
    assert!(
        msg.contains("même forme") || msg.contains("même longueur"),
        "{msg}"
    );
}

// ======================================================================== CA3

#[test]
fn ca3_separer_refus_sans_trace() {
    let (mut p, s) = setup();
    let m = rect_piece(&mut p, s, 0.0, 0.0, 100.0, 60.0, "Devant");
    let split_cmd = |a: PointMm, b: PointMm| Command::SplitPiece { piece: m, a, b };
    // Ne coupe pas.
    let msg = refused(&mut p, split_cmd(pt(0.0, 100.0), pt(1.0, 100.0)));
    assert!(msg.contains("ne traverse pas"), "{msg}");
    // Tangente : ne touche qu'un coin.
    let msg = refused(&mut p, split_cmd(pt(0.0, 0.0), pt(-1.0, 1.0)));
    assert!(msg.contains("toucher") || msg.contains("tangente"), "{msg}");
    // Longe un bord.
    let msg = refused(&mut p, split_cmd(pt(0.0, 60.0), pt(10.0, 60.0)));
    assert!(msg.contains("longe"), "{msg}");
    // Droite dégénérée.
    let msg = refused(&mut p, split_cmd(pt(10.0, 10.0), pt(10.0, 10.0)));
    assert!(msg.contains("dégénérée"), "{msg}");
    // Valeurs non finies ou hors espace de travail.
    refused(&mut p, split_cmd(pt(f64::NAN, 0.0), pt(1.0, 1.0)));
    refused(&mut p, split_cmd(pt(0.0, 0.0), pt(f64::INFINITY, 1.0)));
    refused(&mut p, split_cmd(pt(0.0, 0.0), pt(2.0e6, 1.0)));
    // Pièce inexistante.
    refused(
        &mut p,
        Command::SplitPiece {
            piece: PieceId(999_999),
            a: pt(30.0, -5.0),
            b: pt(30.0, 5.0),
        },
    );
}

#[test]
fn ca3_separer_refuse_plus_de_deux_traversees() {
    let (mut p, s) = setup();
    // U renversé : une droite basse le traverse en quatre points.
    let u = poly_piece(
        &mut p,
        s,
        &[
            pt(0.0, 0.0),
            pt(30.0, 0.0),
            pt(30.0, 40.0),
            pt(60.0, 40.0),
            pt(60.0, 0.0),
            pt(90.0, 0.0),
            pt(90.0, 100.0),
            pt(0.0, 100.0),
        ],
        "U",
    );
    let msg = refused(
        &mut p,
        Command::SplitPiece {
            piece: u,
            a: pt(-10.0, 20.0),
            b: pt(100.0, 20.0),
        },
    );
    assert!(msg.contains("4 points"), "{msg}");
    // Une droite haute ne la traverse qu'en deux points : acceptée (pièce concave).
    let o = split(&mut p, u, pt(-10.0, 70.0), pt(100.0, 70.0)).unwrap();
    let sum: f64 = o.pieces.iter().map(|i| area(&p, *i)).sum();
    assert!(near(sum, 90.0 * 100.0 - 30.0 * 40.0, 1e-9));
}

#[test]
fn ca3_separer_pieces_inutilisables() {
    let (mut p, s) = setup();
    let m = rect_piece(&mut p, s, 0.0, 0.0, 100.0, 60.0, "Devant");
    // Dérivée liée : refusée.
    let d = p
        .apply(Command::CreateDerivedPiece {
            mother: m,
            kind: DerivedKind::Lining,
            operation: DerivedOp::Offset { distance_mm: -5.0 },
            name: None,
        })
        .unwrap()
        .pieces[0];
    let msg = refused(
        &mut p,
        Command::SplitPiece {
            piece: d,
            a: pt(50.0, -5.0),
            b: pt(50.0, 5.0),
        },
    );
    assert!(msg.contains("dérivée"), "{msg}");
    // Contour ouvert.
    let mut open = p.piece(m).unwrap().outline.clone();
    open.pop();
    p.apply(Command::SetPieceOutline {
        piece: m,
        outline: open,
        grainline: None,
    })
    .unwrap();
    refused(
        &mut p,
        Command::SplitPiece {
            piece: m,
            a: pt(30.0, -5.0),
            b: pt(30.0, 5.0),
        },
    );
    // Pièce-étiquette sans contour.
    let e = p
        .apply(Command::AddPiece {
            sheet: s,
            name: "Étiquette".into(),
            quantity: 1,
            material: None,
            mirrored_pair: false,
        })
        .unwrap()
        .pieces[0];
    refused(
        &mut p,
        Command::SplitPiece {
            piece: e,
            a: pt(30.0, -5.0),
            b: pt(30.0, 5.0),
        },
    );
}

#[test]
fn ca3_assembler_refus_sans_trace() {
    let (mut p, s) = setup();
    let a = rect_piece(&mut p, s, 0.0, 0.0, 100.0, 50.0, "A");
    let b = rect_piece(&mut p, s, 500.0, 0.0, 100.0, 20.0, "B");
    let c = rect_piece(&mut p, s, 900.0, 0.0, 40.0, 20.0, "C");
    let ea = edge_between(&p, a, pt(100.0, 50.0), pt(0.0, 50.0));
    let eb = edge_between(&p, b, pt(500.0, 0.0), pt(600.0, 0.0));
    let ec = edge_between(&p, c, pt(900.0, 0.0), pt(940.0, 0.0));
    let j = |f: (PieceId, EntityId), s2: (PieceId, EntityId), t: f64| Command::JoinPieces {
        first: f.0,
        first_edge: f.1,
        second: s2.0,
        second_edge: s2.1,
        tolerance_mm: t,
    };
    // Longueurs différentes au-delà de la tolérance.
    let msg = refused(&mut p, j((a, ea), (c, ec), 0.5));
    assert!(
        msg.contains("même longueur") && msg.contains("60.000"),
        "{msg}"
    );
    // Même pièce deux fois.
    let msg = refused(&mut p, j((a, ea), (a, ea), 0.5));
    assert!(msg.contains("différentes"), "{msg}");
    // Bord qui n'est pas celui de la pièce.
    let msg = refused(&mut p, j((a, eb), (b, eb), 0.5));
    assert!(msg.contains("pas un bord"), "{msg}");
    refused(&mut p, j((a, ea), (b, EntityId(999_999)), 0.5));
    // Pièce inexistante.
    refused(&mut p, j((PieceId(999_999), ea), (b, eb), 0.5));
    // Tolérance invalide.
    for t in [-0.1, f64::NAN, f64::INFINITY, 10.5] {
        refused(&mut p, j((a, ea), (b, eb), t));
    }
    // Dérivée liée.
    let d = p
        .apply(Command::CreateDerivedPiece {
            mother: b,
            kind: DerivedKind::Lining,
            operation: DerivedOp::Offset { distance_mm: -2.0 },
            name: None,
        })
        .unwrap()
        .pieces[0];
    let ed = p.piece(d).unwrap().outline[0].entity;
    let msg = refused(&mut p, j((a, ea), (d, ed), 10.0));
    assert!(msg.contains("dérivée"), "{msg}");
    // Cercle : un seul bord, sans jonction possible.
    let o = p
        .apply(Command::AddCircle {
            sheet: s,
            center: PointRef::New(pt(2000.0, 0.0)),
            radius_mm: 20.0,
        })
        .unwrap();
    let circle_entity = o.entities[0];
    let disc = extract_paths(&mut p, s, o.entities, "Rond");
    let msg = refused(&mut p, j((a, ea), (disc, circle_entity), 10.0));
    assert!(msg.contains("un seul bord"), "{msg}");
    // Aperçu : erreur pour un bord inutilisable, pas de panique.
    assert!(p.join_preview(a, eb, b, eb, 0.5).is_err());
}

#[test]
fn ca3_assembler_refuse_le_recouvrement() {
    let (mut p, s) = setup();
    // A en L ; B une bande qui, collée sur le coin intérieur, entre dans le bras haut de A.
    let a = poly_piece(
        &mut p,
        s,
        &[
            pt(0.0, 0.0),
            pt(100.0, 0.0),
            pt(100.0, 50.0),
            pt(50.0, 50.0),
            pt(50.0, 100.0),
            pt(0.0, 100.0),
        ],
        "A",
    );
    let b = poly_piece(
        &mut p,
        s,
        &[
            pt(1000.0, 0.0),
            pt(1050.0, 0.0),
            pt(1050.0, 100.0),
            pt(900.0, 100.0),
            pt(900.0, 10.0),
            pt(1000.0, 10.0),
        ],
        "B",
    );
    let ea = edge_between(&p, a, pt(100.0, 50.0), pt(50.0, 50.0));
    let eb = edge_between(&p, b, pt(1000.0, 0.0), pt(1050.0, 0.0));
    let msg = refused(
        &mut p,
        Command::JoinPieces {
            first: a,
            first_edge: ea,
            second: b,
            second_edge: eb,
            tolerance_mm: 0.5,
        },
    );
    assert!(msg.contains("recouvrent"), "{msg}");
    let pre = p.join_preview(a, ea, b, eb, 0.5).unwrap();
    assert!(pre
        .refusal
        .as_deref()
        .is_some_and(|r| r.contains("recouvrent")));
    assert!(pre.result.is_none());
}

#[test]
fn ca2_demi_disques_arcs_a_la_jonction_et_arc_mal_raccorde() {
    let (mut p, s) = setup();
    let half = |p: &mut Project, cx: f64, r: f64, name: &str| {
        let seg = segment(p, s, pt(cx - r, 0.0), pt(cx + r, 0.0));
        let arc = p
            .apply(Command::AddArcThreePoints {
                sheet: s,
                start: PointRef::New(pt(cx + r, 0.0)),
                through: PointRef::New(pt(cx, r)),
                end: PointRef::New(pt(cx - r, 0.0)),
            })
            .unwrap()
            .entities[0];
        (extract_paths(p, s, vec![seg, arc], name), seg)
    };
    let (a, sa) = half(&mut p, 0.0, 50.0, "Haut");
    let (b, sb) = half(&mut p, 500.0, 50.0, "Haut 2");
    let (c, sc) = half(&mut p, 1000.0, 50.002, "Haut 3");
    // Deux demi-disques identiques collés par le diamètre : un disque.
    let want = std::f64::consts::PI * 50.0 * 50.0;
    let mut q = p.clone();
    let o = join(&mut q, (a, sa), (b, sb), 0.5).unwrap();
    assert!(
        near(area(&q, o.pieces[0]), want, want * 1e-9),
        "{}",
        area(&q, o.pieces[0])
    );
    assert!(near(
        perimeter(&q, o.pieces[0]),
        2.0 * std::f64::consts::PI * 50.0,
        1e-6
    ));
    // Rayons différant de 0,002 mm : dans la tolérance de longueur, mais un arc ne se
    // raccorde pas avec un tel écart : refus expliqué, sans trace.
    let msg = refused(
        &mut p,
        Command::JoinPieces {
            first: a,
            first_edge: sa,
            second: c,
            second_edge: sc,
            tolerance_mm: 0.5,
        },
    );
    assert!(msg.contains("arc"), "{msg}");
}

// ======================================================================== CA4

#[test]
fn ca4_une_etape_annulable_exacte() {
    let (mut p, s) = setup();
    let m = rect_piece(&mut p, s, 0.0, 0.0, 100.0, 60.0, "Devant");
    let mut d = Document::new(p);
    let start = d.project().clone();
    let start_bytes = bytes(&start);
    // Séparer : une étape.
    let o = d
        .execute(Command::SplitPiece {
            piece: m,
            a: pt(30.0, -5.0),
            b: pt(30.0, 5.0),
        })
        .unwrap();
    let after_split = d.project().clone();
    assert_ne!(after_split, start);
    assert!(d.undo());
    assert_eq!(*d.project(), start);
    assert_eq!(bytes(d.project()), start_bytes);
    assert!(!d.can_undo(), "une seule étape pour séparer");
    assert!(d.redo());
    assert_eq!(
        *d.project(),
        after_split,
        "rétablir redonne exactement le même état"
    );
    // Assembler : une étape de plus ; annuler ramène à la séparation.
    let (l, r) = (o.pieces[0], o.pieces[1]);
    let (el, er) = (cut_edge(d.project(), l), cut_edge(d.project(), r));
    d.execute(Command::JoinPieces {
        first: l,
        first_edge: el,
        second: r,
        second_edge: er,
        tolerance_mm: 0.01,
    })
    .unwrap();
    let after_join = d.project().clone();
    assert!(d.undo());
    assert_eq!(*d.project(), after_split);
    assert_eq!(bytes(d.project()), bytes(&after_split));
    assert!(d.redo());
    assert_eq!(*d.project(), after_join);
    assert!(d.undo() && d.undo());
    assert_eq!(bytes(d.project()), start_bytes);
    assert!(!d.can_undo());
}

#[test]
fn ca4_refus_ne_cree_pas_d_etape() {
    let (mut p, s) = setup();
    let m = rect_piece(&mut p, s, 0.0, 0.0, 100.0, 60.0, "Devant");
    let mut d = Document::new(p);
    assert!(d
        .execute(Command::SplitPiece {
            piece: m,
            a: pt(0.0, 100.0),
            b: pt(1.0, 100.0)
        })
        .is_err());
    assert!(!d.can_undo());
}

// ======================================================================== CA5

#[test]
fn ca5_fixtures_et_exemples_inchanges_a_l_octet_schema_3() {
    let mut texts: Vec<(String, String)> = Vec::new();
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    for name in FIXTURES {
        let t = std::fs::read_to_string(fixtures.join(name)).unwrap();
        texts.push((
            name.to_string(),
            persist::to_json(&persist::from_json(&t).unwrap()).unwrap(),
        ));
    }
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let mut shipped = 0;
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if !path.to_string_lossy().ends_with(".pforge.json") {
            continue;
        }
        let t = std::fs::read_to_string(&path)
            .unwrap()
            .replace("\r\n", "\n");
        let again = persist::to_json(&persist::from_json(&t).unwrap()).unwrap();
        assert_eq!(t, again, "{}", path.display());
        shipped += 1;
        texts.push((path.display().to_string(), again));
    }
    assert!(shipped >= 3);
    for (name, text) in &texts {
        // Aucun champ nouveau : le schéma reste 3 et rien de lié à US-04.6 n'est écrit.
        assert!(!text.contains("split") && !text.contains("join"), "{name}");
        let v: serde_json::Value = serde_json::from_str(text).unwrap();
        assert!(v["schema_version"].as_u64().unwrap() <= 6, "{name}");
    }
}

#[test]
fn ca5_sur_les_exemples_la_separation_ne_touche_pas_a_la_construction() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let mut tried = 0;
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if !path.to_string_lossy().ends_with(".pforge.json") {
            continue;
        }
        let p0 = persist::load(&path).unwrap();
        for piece in p0.pieces.clone() {
            let Ok(c) = p0.piece_contour(piece.id) else {
                continue;
            };
            if !c.closed {
                continue;
            }
            // Droite verticale au milieu de la boîte du contour.
            let sheet = p0.piece_sheet(piece.id).unwrap();
            let mut bb = pf_geometry::Bbox::from_point(pt(0.0, 0.0));
            let mut first = true;
            for e in &piece.outline {
                let b = sheet.shape(e.entity).unwrap().bbox();
                if first {
                    bb = b;
                    first = false;
                } else {
                    bb.union(b);
                }
            }
            let x = 0.5 * (bb.min.x + bb.max.x);
            let mut p = p0.clone();
            let before_sheets = serde_json::to_value(&p.sheets).unwrap();
            if let Ok(o) = split(
                &mut p,
                piece.id,
                pt(x, bb.min.y - 1.0),
                pt(x, bb.max.y + 1.0),
            ) {
                tried += 1;
                let after = serde_json::to_value(&p.sheets).unwrap();
                // Les feuilles d'origine sont identiques ; seules des feuilles sont ajoutées.
                assert_eq!(
                    &after.as_array().unwrap()[..before_sheets.as_array().unwrap().len()],
                    before_sheets.as_array().unwrap().as_slice(),
                    "{}",
                    path.display()
                );
                let sum: f64 = o.pieces.iter().map(|i| area(&p, *i)).sum();
                let a0 = c.area_mm2;
                assert!(
                    near(sum, a0, a0 * 1e-9),
                    "{} : {sum} ≠ {a0}",
                    path.display()
                );
                p.validate().unwrap();
            }
        }
    }
    assert!(tried >= 1, "aucune pièce d'exemple séparée");
}

// ======================================================================== CA6

#[test]
fn ca6_serialisation_relue_identique_et_champs_coherents() {
    let (mut p, s) = setup();
    let m = rect_piece(&mut p, s, 0.0, 0.0, 200.0, 100.0, "Devant");
    // Droit-fil à gauche, deux lignes internes (une à droite, une qui traverse), un repère.
    let grain = segment(&mut p, s, pt(20.0, 50.0), pt(80.0, 50.0));
    p.apply(Command::SetPieceOutline {
        piece: m,
        outline: p.piece(m).unwrap().outline.clone(),
        grainline: Some(grain),
    })
    .unwrap();
    let right_line = segment(&mut p, s, pt(120.0, 20.0), pt(180.0, 20.0));
    let crossing = segment(&mut p, s, pt(90.0, 80.0), pt(110.0, 80.0));
    let mark = p
        .apply(Command::AddPoint {
            sheet: s,
            at: pt(150.0, 60.0),
        })
        .unwrap()
        .points[0];
    for (target, kind) in [
        (
            InternalTarget::Path { entity: right_line },
            InternalKind::Fold,
        ),
        (
            InternalTarget::Path { entity: crossing },
            InternalKind::Topstitch,
        ),
        (InternalTarget::Point { point: mark }, InternalKind::Mark),
    ] {
        p.apply(Command::AddPieceInternal {
            piece: m,
            target,
            kind,
        })
        .unwrap();
    }
    p.apply(Command::SetPiecePlacement {
        piece: m,
        placement: Some(Placement {
            x_mm: 10.0,
            y_mm: 20.0,
            rotation_deg: 30.0,
        }),
    })
    .unwrap();
    // Une pièce dérivée de la pièce coupée : son lien est cassé avec une remarque.
    let d = p
        .apply(Command::CreateDerivedPiece {
            mother: m,
            kind: DerivedKind::Lining,
            operation: DerivedOp::Offset { distance_mm: -3.0 },
            name: None,
        })
        .unwrap()
        .pieces[0];
    let sheets_before = serde_json::to_string(&p.sheets).unwrap();

    let o = split(&mut p, m, pt(100.0, -5.0), pt(100.0, 5.0)).unwrap();
    let (left, right) = (o.pieces[0], o.pieces[1]);
    // Droit-fil : copié sur la moitié gauche, qui le contient ; la droite est signalée.
    let gl = p
        .piece(left)
        .unwrap()
        .grainline
        .expect("droit-fil de la moitié gauche");
    let sheet_l = p.piece_sheet(left).unwrap();
    assert_eq!(
        ends_of(&sheet_l.shape(gl).unwrap()),
        (pt(20.0, 50.0), pt(80.0, 50.0))
    );
    assert_eq!(
        sheet_l.entity(gl).unwrap().role,
        p.sheet(s).unwrap().entity(grain).unwrap().role
    );
    assert!(p.piece(right).unwrap().grainline.is_none());
    assert!(p
        .piece_issues(right)
        .contains(&PieceIssue::MissingGrainline));
    assert!(!p.piece_issues(left).contains(&PieceIssue::MissingGrainline));
    // Éléments internes : ligne et repère de droite sur la moitié droite ; celui qui traverse
    // est signalé et non repris.
    assert!(p.piece(left).unwrap().internal.is_empty());
    let internals = p.piece_internals(right);
    assert_eq!(internals.len(), 2);
    assert!(internals.iter().all(|r| r.inside_outline == Some(true)));
    let lens: Vec<f64> = internals
        .iter()
        .filter_map(|r| match r.geometry {
            InternalGeometry::Path { length_mm } => Some(length_mm),
            _ => None,
        })
        .collect();
    assert_eq!(lens.len(), 1);
    assert!(near(lens[0], 60.0, 1e-9));
    assert!(internals.iter().any(|r| matches!(r.geometry, InternalGeometry::Position { at } if distance_mm(at, pt(150.0, 60.0)) < 1e-9)));
    assert!(
        o.notes
            .iter()
            .any(|n| n.contains("traversent la ligne de coupe")),
        "{:?}",
        o.notes
    );
    // Placement : non repris (placement automatique), plan cohérent.
    assert!(
        p.piece(left).unwrap().placement.is_none() && p.piece(right).unwrap().placement.is_none()
    );
    assert!(o.notes.iter().any(|n| n.contains("placement")));
    let layout = p.plan_layout();
    for id in [left, right] {
        assert!(layout.iter().any(|i| i.piece == id));
    }
    // Pièce dérivée : lien cassé (remarque), la doublure reste une pièce ordinaire valide.
    assert!(p.piece(d).unwrap().derived.is_none());
    assert!(p.piece_derivation(d).is_none());
    assert!(
        o.notes.iter().any(|n| n.contains("n'est plus dérivée")),
        "{:?}",
        o.notes
    );
    assert!(p.piece_contour(d).is_ok());
    // Construction (feuilles existantes) intacte : seules des feuilles sont ajoutées.
    let after = serde_json::to_string(&p.sheets).unwrap();
    assert!(after.starts_with(&sheets_before[..sheets_before.len() - 1]));
    assert_eq!(p.sheets.len(), 4);
    // Sérialisation : relue identique, schéma 3 inchangé.
    let text = bytes(&p);
    let back = persist::from_json(&text).unwrap();
    assert_eq!(back, p);
    assert_eq!(bytes(&back), text);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&text).unwrap()["schema_version"],
        6
    );
    p.validate().unwrap();
}

#[test]
fn ca6_assemblage_reporte_droit_fil_internes_et_fiche() {
    let (mut p, s) = setup();
    let a = rect_piece(&mut p, s, 0.0, 0.0, 100.0, 50.0, "A");
    let b = rect_piece(&mut p, s, 500.0, 0.0, 100.0, 20.0, "B");
    let ga = segment(&mut p, s, pt(10.0, 25.0), pt(60.0, 25.0));
    let gb = segment(&mut p, s, pt(510.0, 10.0), pt(560.0, 10.0));
    p.apply(Command::SetPieceOutline {
        piece: a,
        outline: p.piece(a).unwrap().outline.clone(),
        grainline: Some(ga),
    })
    .unwrap();
    p.apply(Command::SetPieceOutline {
        piece: b,
        outline: p.piece(b).unwrap().outline.clone(),
        grainline: Some(gb),
    })
    .unwrap();
    let ib = segment(&mut p, s, pt(520.0, 5.0), pt(580.0, 5.0));
    p.apply(Command::AddPieceInternal {
        piece: b,
        target: InternalTarget::Path { entity: ib },
        kind: InternalKind::Fold,
    })
    .unwrap();
    p.apply(Command::SetPieceCard {
        piece: b,
        name: "B".into(),
        code: Some("B1".into()),
        quantity: 2,
        mirrored_pair: false,
        material: Some("Lin".into()),
        category: Some(PieceCategory::Lining),
        comment: None,
    })
    .unwrap();
    let ea = edge_between(&p, a, pt(100.0, 50.0), pt(0.0, 50.0));
    let eb = edge_between(&p, b, pt(500.0, 0.0), pt(600.0, 0.0));
    let o = join(&mut p, (a, ea), (b, eb), 0.5).unwrap();
    let m = p.piece(o.pieces[0]).unwrap();
    assert_eq!(m.name, "A + B");
    assert_eq!(m.quantity, 1, "fiche de la première pièce");
    assert!(
        o.notes.iter().any(|n| n.contains("différait")),
        "{:?}",
        o.notes
    );
    assert!(o
        .notes
        .iter()
        .any(|n| n.contains("droit-fil de la pièce « B » n'est pas repris")));
    // Droit-fil de A conservé ; ligne interne de B déplacée dans le repère de A (y = 55).
    let sheet = p.piece_sheet(m.id).unwrap();
    let g = m.grainline.unwrap();
    assert_eq!(
        ends_of(&sheet.shape(g).unwrap()),
        (pt(10.0, 25.0), pt(60.0, 25.0))
    );
    let internals = p.piece_internals(m.id);
    assert_eq!(internals.len(), 1);
    assert_eq!(internals[0].inside_outline, Some(true));
    let InternalTarget::Path { entity } = m.internal[0].target else {
        panic!()
    };
    let (u, v) = ends_of(&sheet.shape(entity).unwrap());
    // Le bord de B (y = 0) est devenu le bord de A (y = 50) ; B est au-dessus : y = 50 + 5.
    assert!(near(u.y, 55.0, 1e-9) && near(v.y, 55.0, 1e-9));
    assert!(near((u.x - v.x).abs(), 60.0, 1e-9));
    p.validate().unwrap();
}

// ================================================================== pièce détachée

#[test]
fn ca6_pieces_detachees_separables_et_assemblables() {
    // Une pièce détachée (copie sur sa propre feuille) se sépare aussi.
    let (mut p, s) = setup();
    let o = p
        .apply(Command::AddRectangle {
            sheet: s,
            corner: PointRef::New(pt(0.0, 0.0)),
            width_mm: 80.0,
            height_mm: 40.0,
        })
        .unwrap();
    let m = p
        .apply(Command::ExtractPiece {
            sheet: s,
            source: PieceSource::Paths {
                entities: o.entities,
            },
            name: "Copie".into(),
            detached: true,
        })
        .unwrap()
        .pieces[0];
    let o = split(&mut p, m, pt(40.0, -1.0), pt(40.0, 1.0)).unwrap();
    for id in &o.pieces {
        assert!(near(area(&p, *id), 1600.0, 1e-9));
    }
    // Séparer une moitié encore : (1/2) devient « … (1/2) (1/2) ».
    let again = split(&mut p, o.pieces[0], pt(20.0, -1.0), pt(20.0, 1.0)).unwrap();
    assert!(p
        .piece(again.pieces[0])
        .unwrap()
        .name
        .ends_with("(1/2) (1/2)"));
    p.validate().unwrap();
}
