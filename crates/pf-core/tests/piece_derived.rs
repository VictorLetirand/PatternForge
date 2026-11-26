//! US-04.4 — pièces dérivées : un groupe de tests par critère d'acceptation
//! (CA1 création, CA2 mise à jour automatique, CA3 lien cassable, CA4 intégrité du
//! fichier, CA5 lecture, CA7 copie de feuille). Les valeurs attendues sont
//! recalculées **indépendamment** (polygones décalés à la main, formules du
//! stade et du cercle, distance à une polyligne dense).

use pf_core::persist;
use pf_core::piece::PieceIssue;
use pf_core::*;
use pf_geometry::{PointMm, Transform};
use serde_json::{json, Value};
use std::f64::consts::PI;
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

fn fixture(name: &str) -> Project {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let text = std::fs::read_to_string(dir.join(name)).unwrap();
    persist::from_json(&text).unwrap_or_else(|e| panic!("{name} : {e}"))
}

fn to_value(p: &Project) -> Value {
    serde_json::from_str(&persist::to_json(p).unwrap()).unwrap()
}

fn offset(d: f64) -> DerivedOp {
    DerivedOp::Offset { distance_mm: d }
}

/// Rectangle `w × h` en (0, 0), extrait en pièce « Mère » (liée, quatre tracés).
fn rect_mother(w: f64, h: f64) -> (Project, SheetId, PieceId) {
    let mut p = Project::new("dérivées");
    let s = p.sheets[0].id;
    let o = p
        .apply(Command::AddRectangle {
            sheet: s,
            corner: PointRef::New(pt(0.0, 0.0)),
            width_mm: w,
            height_mm: h,
        })
        .unwrap();
    let m = p
        .apply(Command::ExtractPiece {
            sheet: s,
            source: PieceSource::Paths {
                entities: o.entities,
            },
            name: "Mère".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    (p, s, m)
}

fn derive(
    p: &mut Project,
    mother: PieceId,
    kind: DerivedKind,
    d: f64,
) -> Result<PieceId, CoreError> {
    p.apply(Command::CreateDerivedPiece {
        mother,
        kind,
        operation: offset(d),
        name: None,
    })
    .map(|o| o.pieces[0])
}

fn find_point(p: &Project, s: SheetId, x: f64, y: f64) -> PointId {
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
    let point = find_point(p, s, from.0, from.1);
    p.apply(Command::MovePoint {
        sheet: s,
        point,
        to: pt(to.0, to.1),
    })
}

/// Sommets d'une pièce dont le contour n'a que des segments, dans l'ordre du contour.
fn polygon(p: &Project, piece: PieceId) -> Vec<(f64, f64)> {
    let pc = p.piece(piece).unwrap();
    let sh = p.piece_sheet(piece).unwrap();
    pc.outline
        .iter()
        .map(|e| {
            let (a, b) = sh.segment_point_ids(e.entity).unwrap();
            let v = sh.position(if e.reversed { b } else { a }).unwrap();
            (v.x, v.y)
        })
        .collect()
}

fn signed_area(poly: &[(f64, f64)]) -> f64 {
    let n = poly.len();
    (0..n)
        .map(|i| {
            let (a, b) = (poly[i], poly[(i + 1) % n]);
            0.5 * (a.0 * b.1 - b.0 * a.1)
        })
        .sum()
}

fn perimeter(poly: &[(f64, f64)]) -> f64 {
    let n = poly.len();
    (0..n)
        .map(|i| {
            let (a, b) = (poly[i], poly[(i + 1) % n]);
            (b.0 - a.0).hypot(b.1 - a.1)
        })
        .sum()
}

/// Polygone simple décalé de `d` (positif = vers l'extérieur), coins en pointe :
/// chaque arête est déplacée le long de sa normale, les sommets sont les
/// intersections des droites consécutives. Calcul indépendant de la bibliothèque.
fn offset_poly(poly: &[(f64, f64)], d: f64) -> Vec<(f64, f64)> {
    let mut v = poly.to_vec();
    if signed_area(&v) < 0.0 {
        v.reverse();
    }
    let n = v.len();
    let line = |i: usize| {
        let (a, b) = (v[i], v[(i + 1) % n]);
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let l = dx.hypot(dy);
        // Anti-horaire : l'extérieur est à droite de la direction.
        ((a.0 + d * dy / l, a.1 - d * dx / l), (dx, dy))
    };
    (0..n)
        .map(|i| {
            let ((p0, d0), (p1, d1)) = (line((i + n - 1) % n), line(i));
            let t = ((p1.0 - p0.0) * d1.1 - (p1.1 - p0.1) * d1.0) / (d0.0 * d1.1 - d0.1 * d1.0);
            (p0.0 + t * d0.0, p0.1 + t * d0.1)
        })
        .collect()
}

/// L'aire et le périmètre mesurés de la dérivée valent ceux du décalage calculé à part.
fn assert_offset_of(p: &Project, mother: PieceId, derived: PieceId, d: f64) {
    let expected = offset_poly(&polygon(p, mother), d);
    let c = p.piece_contour(derived).unwrap();
    assert!(c.closed && !c.self_intersecting && !c.clockwise);
    assert!(
        near(c.area_mm2, signed_area(&expected).abs(), 1e-6),
        "aire {} ≠ attendu {}",
        c.area_mm2,
        signed_area(&expected).abs()
    );
    assert!(
        near(c.perimeter_mm, perimeter(&expected), 1e-6),
        "périmètre {} ≠ attendu {}",
        c.perimeter_mm,
        perimeter(&expected)
    );
    // Les sommets eux-mêmes (ordre cyclique libre) : chaque sommet attendu existe.
    let got = polygon(p, derived);
    assert_eq!(got.len(), expected.len());
    for e in &expected {
        assert!(
            got.iter()
                .any(|g| near(g.0, e.0, 1e-6) && near(g.1, e.1, 1e-6)),
            "sommet attendu {e:?} absent de {got:?}"
        );
    }
}

fn assert_refused(p: &mut Project, cmd: Command) -> CoreError {
    let before = p.clone();
    let err = p
        .apply(cmd)
        .expect_err("la commande aurait dû être refusée");
    assert_eq!(*p, before, "refus sans trace : {err}");
    err
}

fn sheet_of(p: &Project, piece: PieceId) -> SheetId {
    match p.piece(piece).unwrap().link {
        PieceLink::Linked { sheet } => sheet,
        PieceLink::Detached => panic!("pièce détachée"),
    }
}

/// Dernier recours pour comparer deux états : leur JSON complet.
fn text(p: &Project) -> String {
    persist::to_json(p).unwrap()
}

// ============================================================ CA1 : création

#[test]
fn ca1_rectangle_decale_vers_l_interieur() {
    let (mut p, s, m) = rect_mother(200.0, 100.0);
    let sheets = p.sheets.len();
    let d = derive(&mut p, m, DerivedKind::Lining, -10.0).unwrap();
    // 200×100 décalé de −10 : 180×80.
    let c = p.piece_contour(d).unwrap();
    assert!(near(c.area_mm2, 14_400.0, 1e-6), "{}", c.area_mm2);
    assert!(near(c.perimeter_mm, 520.0, 1e-6), "{}", c.perimeter_mm);
    assert_offset_of(&p, m, d, -10.0);
    // Feuille dédiée, pièce liée, nom par défaut, catégorie et quantité.
    assert_eq!(p.sheets.len(), sheets + 1);
    let piece = p.piece(d).unwrap();
    assert_ne!(sheet_of(&p, d), s);
    assert_eq!(piece.name, "Mère – doublure");
    assert_eq!(p.sheet(sheet_of(&p, d)).unwrap().name, "Mère – doublure");
    assert_eq!(piece.category, Some(PieceCategory::Lining));
    assert_eq!(piece.quantity, 1);
    assert_eq!(
        piece.derived,
        Some(DerivedSpec {
            mother: m,
            kind: DerivedKind::Lining,
            operation: offset(-10.0),
            stale: None
        })
    );
    // La mère n'est pas touchée.
    let mc = p.piece_contour(m).unwrap();
    assert!(near(mc.area_mm2, 20_000.0, 1e-9));
}

#[test]
fn ca1_rectangle_decale_vers_l_exterieur() {
    let (mut p, _, m) = rect_mother(200.0, 100.0);
    let d = derive(&mut p, m, DerivedKind::Interfacing, 10.0).unwrap();
    let c = p.piece_contour(d).unwrap();
    // 220×120.
    assert!(near(c.area_mm2, 26_400.0, 1e-6), "{}", c.area_mm2);
    assert!(near(c.perimeter_mm, 680.0, 1e-6), "{}", c.perimeter_mm);
    assert_offset_of(&p, m, d, 10.0);
    assert_eq!(
        p.piece(d).unwrap().category,
        Some(PieceCategory::Interfacing)
    );
}

#[test]
fn ca1_types_categories_et_noms() {
    let (mut p, _, m) = rect_mother(100.0, 60.0);
    let cases = [
        (DerivedKind::Facing, None, "Mère – parementure"),
        (
            DerivedKind::Lining,
            Some(PieceCategory::Lining),
            "Mère – doublure",
        ),
        (
            DerivedKind::Interfacing,
            Some(PieceCategory::Interfacing),
            "Mère – entoilage",
        ),
        (
            DerivedKind::Reinforcement,
            Some(PieceCategory::Reinforcement),
            "Mère – renfort",
        ),
    ];
    for (kind, cat, name) in cases {
        let d = derive(&mut p, m, kind, -2.0).unwrap();
        let piece = p.piece(d).unwrap();
        assert_eq!(piece.category, cat, "{kind:?}");
        assert_eq!(piece.name, name);
    }
    assert_eq!(p.derived_pieces_of(m).len(), 4);
    // Nom choisi, rogné ; la catégorie reste éditable par la fiche.
    let o = p
        .apply(Command::CreateDerivedPiece {
            mother: m,
            kind: DerivedKind::Lining,
            operation: offset(-2.0),
            name: Some("  Doublure poche  ".into()),
        })
        .unwrap();
    let d = o.pieces[0];
    assert_eq!(p.piece(d).unwrap().name, "Doublure poche");
    p.apply(Command::SetPieceCard {
        piece: d,
        name: "Doublure poche".into(),
        code: Some("DB1".into()),
        quantity: 2,
        mirrored_pair: true,
        material: None,
        category: Some(PieceCategory::Fabric),
        comment: None,
    })
    .unwrap();
    let card = p.piece(d).unwrap();
    assert_eq!(card.category, Some(PieceCategory::Fabric));
    assert!(card.derived.is_some(), "la fiche ne casse pas le lien");
}

#[test]
fn ca1_cercle() {
    let mut p = Project::new("cercle");
    let s = p.sheets[0].id;
    let c = p
        .apply(Command::AddCircle {
            sheet: s,
            center: PointRef::New(pt(10.0, 20.0)),
            radius_mm: 50.0,
        })
        .unwrap()
        .entities[0];
    let m = p
        .apply(Command::ExtractPiece {
            sheet: s,
            source: PieceSource::Paths { entities: vec![c] },
            name: "Rond".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    for (dist, r) in [(-10.0, 40.0), (7.5, 57.5)] {
        let d = derive(&mut p, m, DerivedKind::Lining, dist).unwrap();
        let q = p.piece_contour(d).unwrap();
        assert!(near(q.area_mm2, PI * r * r, 1e-6), "{} {r}", q.area_mm2);
        assert!(near(q.perimeter_mm, 2.0 * PI * r, 1e-6));
        let sh = p.sheet(sheet_of(&p, d)).unwrap();
        let e = &p.piece(d).unwrap().outline;
        assert_eq!(e.len(), 1);
        assert!(matches!(
            sh.entity(e[0].entity).unwrap().kind,
            EntityKind::Circle { radius_mm, .. } if near(radius_mm, r, 1e-9)
        ));
        let centre = sh.circle_geometry(e[0].entity).unwrap().circle.center;
        assert!(near(centre.x, 10.0, 1e-9) && near(centre.y, 20.0, 1e-9));
    }
    // Un rayon qui s'annule est refusé.
    assert_refused(
        &mut p,
        Command::CreateDerivedPiece {
            mother: m,
            kind: DerivedKind::Lining,
            operation: offset(-50.0),
            name: None,
        },
    );
}

/// Stade : deux segments de 100 mm et deux demi-cercles de rayon 20 (coins arrondis).
fn stadium() -> (Project, SheetId, PieceId) {
    let mut p = Project::new("stade");
    let s = p.sheets[0].id;
    let mut ids = Vec::new();
    for cmd in [
        Command::AddSegment {
            sheet: s,
            from: PointRef::New(pt(0.0, 0.0)),
            to: PointRef::New(pt(100.0, 0.0)),
        },
        Command::AddArcThreePoints {
            sheet: s,
            start: PointRef::New(pt(100.0, 0.0)),
            through: PointRef::New(pt(120.0, 20.0)),
            end: PointRef::New(pt(100.0, 40.0)),
        },
        Command::AddSegment {
            sheet: s,
            from: PointRef::New(pt(100.0, 40.0)),
            to: PointRef::New(pt(0.0, 40.0)),
        },
        Command::AddArcThreePoints {
            sheet: s,
            start: PointRef::New(pt(0.0, 40.0)),
            through: PointRef::New(pt(-20.0, 20.0)),
            end: PointRef::New(pt(0.0, 0.0)),
        },
    ] {
        ids.extend(p.apply(cmd).unwrap().entities);
    }
    let m = p
        .apply(Command::ExtractPiece {
            sheet: s,
            source: PieceSource::Paths { entities: ids },
            name: "Stade".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    (p, s, m)
}

/// Aire et périmètre d'un stade (côtés droits de 100 mm, rayon `r`, hauteur 2r).
fn stadium_measures(r: f64) -> (f64, f64) {
    (100.0 * 2.0 * r + PI * r * r, 2.0 * 100.0 + 2.0 * PI * r)
}

#[test]
fn ca1_contour_mixte_segments_et_arcs() {
    let (mut p, _, m) = stadium();
    let c = p.piece_contour(m).unwrap();
    let (a, per) = stadium_measures(20.0);
    assert!(near(c.area_mm2, a, 1e-6) && near(c.perimeter_mm, per, 1e-6));
    for (dist, r) in [(-5.0, 15.0), (5.0, 25.0)] {
        let d = derive(&mut p, m, DerivedKind::Lining, dist).unwrap();
        let q = p.piece_contour(d).unwrap();
        let (a, per) = stadium_measures(r);
        assert!(near(q.area_mm2, a, 1e-6), "{} ≠ {a}", q.area_mm2);
        assert!(
            near(q.perimeter_mm, per, 1e-6),
            "{} ≠ {per}",
            q.perimeter_mm
        );
        let sh = p.sheet(sheet_of(&p, d)).unwrap();
        let kinds: Vec<&str> = p
            .piece(d)
            .unwrap()
            .outline
            .iter()
            .map(|e| match sh.entity(e.entity).unwrap().kind {
                EntityKind::Segment { .. } => "segment",
                EntityKind::Arc { .. } => "arc",
                _ => "autre",
            })
            .collect();
        assert_eq!(kinds.iter().filter(|k| **k == "segment").count(), 2);
        assert_eq!(kinds.iter().filter(|k| **k == "arc").count(), 2);
    }
}

/// Distance de `q` à une polyligne (projection sur chaque segment).
fn dist_to_polyline(q: (f64, f64), line: &[(f64, f64)]) -> f64 {
    line.windows(2)
        .map(|w| {
            let (a, b) = (w[0], w[1]);
            let (dx, dy) = (b.0 - a.0, b.1 - a.1);
            let l2 = dx * dx + dy * dy;
            let t = if l2 == 0.0 {
                0.0
            } else {
                (((q.0 - a.0) * dx + (q.1 - a.1) * dy) / l2).clamp(0.0, 1.0)
            };
            (q.0 - (a.0 + t * dx)).hypot(q.1 - (a.1 + t * dy))
        })
        .fold(f64::INFINITY, f64::min)
}

/// De Casteljau écrit à la main (indépendant de la bibliothèque).
fn bezier(c: [(f64, f64); 4], t: f64) -> (f64, f64) {
    let lerp = |a: (f64, f64), b: (f64, f64)| (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
    let (ab, bc, cd) = (lerp(c[0], c[1]), lerp(c[1], c[2]), lerp(c[2], c[3]));
    lerp(lerp(ab, bc), lerp(bc, cd))
}

/// Points d'une pièce échantillonnés avec des formules indépendantes
/// (segments et Béziers) : `per_edge` points par tracé.
fn sample_piece(p: &Project, piece: PieceId, per_edge: usize) -> Vec<(f64, f64)> {
    sample_edges(p, piece, per_edge)
        .into_iter()
        .flat_map(|(_, pts)| pts)
        .collect()
}

/// Comme `sample_piece`, par tracé : `true` pour une Bézier.
fn sample_edges(p: &Project, piece: PieceId, per_edge: usize) -> Vec<(bool, Vec<(f64, f64)>)> {
    let sh = p.piece_sheet(piece).unwrap();
    let mut out = Vec::new();
    for e in &p.piece(piece).unwrap().outline {
        let ctrl: [(f64, f64); 4] = match sh.entity(e.entity).unwrap().kind {
            EntityKind::Segment { from, to, .. } => {
                let (a, b) = (sh.position(from).unwrap(), sh.position(to).unwrap());
                [(a.x, a.y), (a.x, a.y), (b.x, b.y), (b.x, b.y)]
            }
            EntityKind::Curve { .. } => {
                let c = sh.curve_geometry(e.entity).unwrap();
                [
                    (c.p0.x, c.p0.y),
                    (c.c1.x, c.c1.y),
                    (c.c2.x, c.c2.y),
                    (c.p3.x, c.p3.y),
                ]
            }
            ref k => panic!("tracé non prévu par ce test : {k:?}"),
        };
        let mut pts: Vec<(f64, f64)> = (0..=per_edge)
            .map(|i| {
                let t = i as f64 / per_edge as f64;
                if matches!(
                    sh.entity(e.entity).unwrap().kind,
                    EntityKind::Segment { .. }
                ) {
                    (
                        ctrl[0].0 + (ctrl[3].0 - ctrl[0].0) * t,
                        ctrl[0].1 + (ctrl[3].1 - ctrl[0].1) * t,
                    )
                } else {
                    bezier(ctrl, t)
                }
            })
            .collect();
        if e.reversed {
            pts.reverse();
        }
        out.push((
            matches!(sh.entity(e.entity).unwrap().kind, EntityKind::Curve { .. }),
            pts,
        ));
    }
    out
}

fn shoelace_closed(pts: &[(f64, f64)]) -> f64 {
    signed_area(pts).abs()
}

#[test]
fn ca1_contour_avec_bezier_a_un_centieme_de_mm() {
    // Lentille : un segment AB et une Bézier de B à A.
    let mut p = Project::new("lentille");
    let s = p.sheets[0].id;
    let seg = p
        .apply(Command::AddSegment {
            sheet: s,
            from: PointRef::New(pt(0.0, 0.0)),
            to: PointRef::New(pt(100.0, 0.0)),
        })
        .unwrap()
        .entities[0];
    let curve = p
        .apply(Command::AddCurve {
            sheet: s,
            start: PointRef::New(pt(100.0, 0.0)),
            c1: pt(100.0, 60.0),
            c2: pt(0.0, 60.0),
            end: PointRef::New(pt(0.0, 0.0)),
        })
        .unwrap()
        .entities[0];
    let m = p
        .apply(Command::ExtractPiece {
            sheet: s,
            source: PieceSource::Paths {
                entities: vec![seg, curve],
            },
            name: "Lentille".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    let mother_line = sample_piece(&p, m, 3000);
    for dist in [-3.0, 4.0] {
        let d = derive(&mut p, m, DerivedKind::Lining, dist).unwrap();
        let derived = sample_piece(&p, d, 400);
        // Chaque point des courbes de la dérivée est à |d| de la mère, à 0,01 mm
        // près ; sur les segments aussi, sauf le prolongement d'un coin extérieur en
        // pointe (plus loin que |d|, jamais plus près).
        let mut curve_points = 0;
        for (is_curve, pts) in sample_edges(&p, d, 400) {
            for q in &pts {
                let got = dist_to_polyline(*q, &mother_line);
                if is_curve || dist < 0.0 {
                    curve_points += 1;
                    assert!(
                        near(got, dist.abs(), 0.01),
                        "distance {got} ≠ {} en {q:?}",
                        dist.abs()
                    );
                } else {
                    assert!(got >= dist.abs() - 0.01, "{got} en {q:?}");
                }
            }
        }
        assert!(curve_points > 400);
        // Aire mesurée (exacte) = aire de l'échantillonnage indépendant (±0,01 mm²
        // par mm de périmètre : l'échantillon de 400 points est une polyligne
        // inscrite, d'écart ≤ sagitta).
        let c = p.piece_contour(d).unwrap();
        let sampled = shoelace_closed(&derived);
        assert!(
            near(c.area_mm2, sampled, 0.01 * c.perimeter_mm / 10.0),
            "aire {} ≠ échantillon {sampled}",
            c.area_mm2
        );
        // Le signe : vers l'intérieur plus petit, vers l'extérieur plus grand.
        let am = p.piece_contour(m).unwrap().area_mm2;
        assert_eq!(c.area_mm2 < am, dist < 0.0);
    }
}

#[test]
fn ca1_refus_sans_trace() {
    let (mut p, s, m) = rect_mother(100.0, 50.0);
    let derive_cmd = |mother, d| Command::CreateDerivedPiece {
        mother,
        kind: DerivedKind::Lining,
        operation: offset(d),
        name: None,
    };
    // Mère inexistante.
    assert!(matches!(
        assert_refused(&mut p, derive_cmd(PieceId(999_999), -5.0)),
        CoreError::UnknownPiece(_)
    ));
    // Distances inutilisables.
    for d in [
        0.0,
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        0.001,
        1e9,
        -1e9,
    ] {
        assert_refused(&mut p, derive_cmd(m, d));
    }
    // Décalage qui annule la forme (demi-largeur 25 mm) ou qui la dégénère.
    let e = assert_refused(&mut p, derive_cmd(m, -30.0));
    assert!(e.to_string().contains("annule") || e.to_string().contains("exploitable"));
    assert_refused(&mut p, derive_cmd(m, -25.0));
    // Nom trop long, nom à caractère de contrôle.
    assert_refused(
        &mut p,
        Command::CreateDerivedPiece {
            mother: m,
            kind: DerivedKind::Lining,
            operation: offset(-5.0),
            name: Some("x".repeat(121)),
        },
    );
    assert_refused(
        &mut p,
        Command::CreateDerivedPiece {
            mother: m,
            kind: DerivedKind::Lining,
            operation: offset(-5.0),
            name: Some("a\u{0}b".into()),
        },
    );
    // Mère sans contour (pièce-étiquette).
    let tag = p
        .apply(Command::AddPiece {
            sheet: s,
            name: "Étiquette".into(),
            quantity: 1,
            material: None,
            mirrored_pair: false,
        })
        .unwrap()
        .pieces[0];
    let e = assert_refused(&mut p, derive_cmd(tag, -5.0));
    assert!(e.to_string().contains("contour"), "{e}");
    // Contour ouvert : un seul tracé d'un rectangle.
    let first = p.piece(m).unwrap().outline[0];
    p.apply(Command::SetPieceOutline {
        piece: tag,
        outline: vec![first],
        grainline: None,
    })
    .unwrap();
    let e = assert_refused(&mut p, derive_cmd(tag, -5.0));
    assert!(e.to_string().contains("fermé") || e.to_string().contains("ouvert"));
    // Contour qui se recoupe (nœud papillon).
    let mut q = Project::new("nœud");
    let qs = q.sheets[0].id;
    let mut ids = Vec::new();
    let pts = [(0.0, 0.0), (100.0, 100.0), (100.0, 0.0), (0.0, 100.0)];
    for i in 0..4 {
        let (a, b) = (pts[i], pts[(i + 1) % 4]);
        ids.extend(
            q.apply(Command::AddSegment {
                sheet: qs,
                from: PointRef::New(pt(a.0, a.1)),
                to: PointRef::New(pt(b.0, b.1)),
            })
            .unwrap()
            .entities,
        );
    }
    let bow = q
        .apply(Command::ExtractPiece {
            sheet: qs,
            source: PieceSource::Paths { entities: ids },
            name: "Nœud".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    let e = assert_refused(&mut q, derive_cmd(bow, -3.0));
    assert!(e.to_string().contains("recoupe"), "{e}");
}

#[test]
fn ca1_creation_une_etape_annulable() {
    let (p, _, m) = rect_mother(200.0, 100.0);
    let initial = p.clone();
    let mut doc = Document::new(p);
    let out = doc
        .execute(Command::CreateDerivedPiece {
            mother: m,
            kind: DerivedKind::Reinforcement,
            operation: offset(-10.0),
            name: None,
        })
        .unwrap();
    let d = out.pieces[0];
    assert_eq!(out.entities.len(), 4);
    assert!(out.sheet.is_some());
    assert!(doc.project().piece(d).is_ok());
    let created = doc.project().clone();
    assert!(doc.undo());
    assert_eq!(*doc.project(), initial, "une seule annulation défait tout");
    assert!(!doc.can_undo());
    assert!(doc.redo());
    assert_eq!(*doc.project(), created);
}

#[test]
fn ca1_profondeur_bornee() {
    let (mut p, _, m) = rect_mother(400.0, 300.0);
    let mut cur = m;
    for i in 0..8 {
        cur = derive(&mut p, cur, DerivedKind::Lining, -1.0)
            .unwrap_or_else(|e| panic!("niveau {} : {e}", i + 1));
    }
    let e = assert_refused(
        &mut p,
        Command::CreateDerivedPiece {
            mother: cur,
            kind: DerivedKind::Lining,
            operation: offset(-1.0),
            name: None,
        },
    );
    assert!(e.to_string().contains("niveaux"), "{e}");
}

// ===================================================== CA2 : mise à jour

#[test]
fn ca2_deplacer_un_point_de_la_mere_met_a_jour_la_derivee() {
    let (mut p, s, m) = rect_mother(200.0, 100.0);
    let d = derive(&mut p, m, DerivedKind::Lining, -10.0).unwrap();
    // Trapèze : le coin (200 ; 100) passe en (150 ; 100).
    let out = move_point(&mut p, s, (200.0, 100.0), (150.0, 100.0)).unwrap();
    assert_eq!(out.regenerated, vec![d]);
    assert_offset_of(&p, m, d, -10.0);
    // Un autre coin, puis vers l'extérieur.
    move_point(&mut p, s, (0.0, 0.0), (-20.0, -5.0)).unwrap();
    assert_offset_of(&p, m, d, -10.0);
    let e = derive(&mut p, m, DerivedKind::Interfacing, 6.0).unwrap();
    assert_offset_of(&p, m, e, 6.0);
    move_point(&mut p, s, (150.0, 100.0), (170.0, 120.0)).unwrap();
    assert_offset_of(&p, m, d, -10.0);
    assert_offset_of(&p, m, e, 6.0);
    assert_eq!(p.piece_derivation(d).unwrap().state, DerivedState::Current);
}

#[test]
fn ca2_transformations_de_la_mere() {
    let (mut p, s, m) = rect_mother(200.0, 100.0);
    let d = derive(&mut p, m, DerivedKind::Lining, -10.0).unwrap();
    let all: Vec<PointId> = p.sheet(s).unwrap().points.iter().map(|q| q.id).collect();
    for transform in [
        Transform::Translate {
            dx_mm: 33.0,
            dy_mm: -12.5,
        },
        Transform::Rotate {
            center: pt(10.0, 10.0),
            angle_deg: 37.0,
        },
        Transform::Mirror {
            axis_point: pt(0.0, 0.0),
            axis_angle_deg: 20.0,
        },
    ] {
        let out = p
            .apply(Command::Transform {
                sheet: s,
                points: all.clone(),
                transform,
            })
            .unwrap();
        assert_eq!(out.regenerated, vec![d], "{transform:?}");
        // Isométrie : l'aire et le périmètre de la dérivée ne changent pas.
        let c = p.piece_contour(d).unwrap();
        assert!(near(c.area_mm2, 14_400.0, 1e-6), "{}", c.area_mm2);
        assert!(near(c.perimeter_mm, 520.0, 1e-6));
        assert_offset_of(&p, m, d, -10.0);
    }
}

#[test]
fn ca2_cercle_rayon_modifie() {
    let mut p = Project::new("cercle");
    let s = p.sheets[0].id;
    let c = p
        .apply(Command::AddCircle {
            sheet: s,
            center: PointRef::New(pt(0.0, 0.0)),
            radius_mm: 50.0,
        })
        .unwrap()
        .entities[0];
    let m = p
        .apply(Command::ExtractPiece {
            sheet: s,
            source: PieceSource::Paths { entities: vec![c] },
            name: "Rond".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    let d = derive(&mut p, m, DerivedKind::Lining, -10.0).unwrap();
    let ids = p.piece(d).unwrap().outline.clone();
    p.apply(Command::SetCircleRadius {
        sheet: s,
        entity: c,
        radius_mm: 80.0,
    })
    .unwrap();
    let q = p.piece_contour(d).unwrap();
    assert!(near(q.area_mm2, PI * 70.0 * 70.0, 1e-6));
    assert_eq!(p.piece(d).unwrap().outline, ids, "mêmes tracés");
}

#[test]
fn ca2_ajout_et_remplacement_du_contour_de_la_mere() {
    let (mut p, s, m) = rect_mother(200.0, 100.0);
    let d = derive(&mut p, m, DerivedKind::Lining, -10.0).unwrap();
    // Un second rectangle, plus grand ; la mère change de contour (SetPieceOutline).
    let o = p
        .apply(Command::AddRectangle {
            sheet: s,
            corner: PointRef::New(pt(500.0, 0.0)),
            width_mm: 300.0,
            height_mm: 200.0,
        })
        .unwrap();
    let out = p
        .apply(Command::SetPieceOutline {
            piece: m,
            outline: o
                .entities
                .iter()
                .map(|&e| PieceEdge {
                    entity: e,
                    reversed: false,
                })
                .collect(),
            grainline: None,
        })
        .unwrap();
    assert_eq!(out.regenerated, vec![d]);
    let c = p.piece_contour(d).unwrap();
    assert!(near(c.area_mm2, 280.0 * 180.0, 1e-6), "{}", c.area_mm2);
    // Dans un Batch : deux déplacements, une seule étape, une seule régénération.
    let pa = find_point(&p, s, 500.0, 0.0);
    let pb = find_point(&p, s, 800.0, 200.0);
    let out = p
        .apply(Command::Batch {
            commands: vec![
                Command::MovePoint {
                    sheet: s,
                    point: pa,
                    to: pt(500.0, 20.0),
                },
                Command::MovePoint {
                    sheet: s,
                    point: pb,
                    to: pt(790.0, 200.0),
                },
            ],
        })
        .unwrap();
    assert_eq!(out.regenerated, vec![d]);
    assert_offset_of(&p, m, d, -10.0);
}

#[test]
fn ca2_batch_creation_puis_modification_de_la_mere() {
    // Créer la dérivée puis déplacer la mère dans le même lot : la dérivée suit l'état final.
    let (mut p, s, m) = rect_mother(200.0, 100.0);
    let corner = find_point(&p, s, 200.0, 100.0);
    let out = p
        .apply(Command::Batch {
            commands: vec![
                Command::CreateDerivedPiece {
                    mother: m,
                    kind: DerivedKind::Lining,
                    operation: offset(-10.0),
                    name: None,
                },
                Command::MovePoint {
                    sheet: s,
                    point: corner,
                    to: pt(150.0, 100.0),
                },
            ],
        })
        .unwrap();
    let d = out.pieces[0];
    assert_eq!(out.regenerated, vec![d]);
    assert_eq!(p.piece_derivation(d).unwrap().state, DerivedState::Current);
    assert_offset_of(&p, m, d, -10.0);
    // Lot dont la dérivée ne peut pas suivre : elle est créée puis obsolète, sans refus.
    let (mut q, qs, qm) = rect_mother(200.0, 100.0);
    let qc = find_point(&q, qs, 200.0, 100.0);
    let out = q
        .apply(Command::Batch {
            commands: vec![
                Command::CreateDerivedPiece {
                    mother: qm,
                    kind: DerivedKind::Lining,
                    operation: offset(-10.0),
                    name: None,
                },
                Command::MovePoint {
                    sheet: qs,
                    point: qc,
                    to: pt(-50.0, 100.0),
                },
            ],
        })
        .unwrap();
    assert!(matches!(
        q.piece_derivation(out.pieces[0]).unwrap().state,
        DerivedState::Stale { .. }
    ));
    assert!(q.validate().is_ok());
}

#[test]
fn ca2_annuler_retablir_en_une_etape() {
    let (mut p, s, m) = rect_mother(200.0, 100.0);
    let d = derive(&mut p, m, DerivedKind::Lining, -10.0).unwrap();
    let before = p.clone();
    let mut doc = Document::new(p);
    doc.execute(Command::MovePoint {
        sheet: s,
        point: find_point(&before, s, 200.0, 100.0),
        to: pt(160.0, 90.0),
    })
    .unwrap();
    let after = doc.project().clone();
    assert_ne!(
        after.piece_contour(d).unwrap().area_mm2,
        before.piece_contour(d).unwrap().area_mm2
    );
    assert!(doc.undo());
    assert_eq!(
        *doc.project(),
        before,
        "mère et dérivée reviennent ensemble"
    );
    assert!(!doc.can_undo(), "une seule étape");
    assert!(doc.redo());
    assert_eq!(*doc.project(), after);
    assert!(!doc.can_redo());
}

#[test]
fn ca2_identifiants_stables_tant_que_la_topologie_est_la_meme() {
    let (mut p, s, m) = rect_mother(200.0, 100.0);
    let d = derive(&mut p, m, DerivedKind::Lining, -10.0).unwrap();
    let ds = sheet_of(&p, d);
    let ids_of = |p: &Project| {
        let sh = p.sheet(ds).unwrap();
        (
            p.piece(d).unwrap().outline.clone(),
            sh.entities.iter().map(|e| e.id).collect::<Vec<_>>(),
            sh.points.iter().map(|q| q.id).collect::<Vec<_>>(),
            p.next_id,
        )
    };
    let first = ids_of(&p);
    for to in [(150.0, 100.0), (170.0, 130.0), (190.0, 95.0)] {
        let from = {
            let sh = p.sheet(s).unwrap();
            let q = sh
                .points
                .iter()
                .find(|q| {
                    let v = sh.position(q.id).unwrap();
                    v.x > 140.0 && v.y > 80.0
                })
                .unwrap();
            let v = sh.position(q.id).unwrap();
            (v.x, v.y)
        };
        move_point(&mut p, s, from, to).unwrap();
        assert_eq!(ids_of(&p), first, "identifiants stables après {to:?}");
    }
}

/// Triangle dont la pointe peut devenir très aiguë : le coin extérieur passe
/// de la pointe au biseau (le décalage gagne un tracé).
fn triangle() -> (Project, SheetId, PieceId) {
    let mut p = Project::new("triangle");
    let s = p.sheets[0].id;
    let pts = [(0.0, 0.0), (100.0, 0.0), (50.0, 50.0)];
    let mut ids = Vec::new();
    for i in 0..3 {
        let (a, b) = (pts[i], pts[(i + 1) % 3]);
        ids.extend(
            p.apply(Command::AddSegment {
                sheet: s,
                from: PointRef::New(pt(a.0, a.1)),
                to: PointRef::New(pt(b.0, b.1)),
            })
            .unwrap()
            .entities,
        );
    }
    let m = p
        .apply(Command::ExtractPiece {
            sheet: s,
            source: PieceSource::Paths { entities: ids },
            name: "Triangle".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    (p, s, m)
}

#[test]
fn ca2_la_topologie_change_les_traces_sont_recrees_avec_remarque() {
    let (mut p, s, m) = triangle();
    let d = derive(&mut p, m, DerivedKind::Reinforcement, 5.0).unwrap();
    assert_eq!(p.piece(d).unwrap().outline.len(), 3);
    assert_offset_of(&p, m, d, 5.0);
    let old: Vec<EntityId> = p
        .piece(d)
        .unwrap()
        .outline
        .iter()
        .map(|e| e.entity)
        .collect();
    // Pointe très aiguë (~11°) : coin en biseau, donc un tracé de plus.
    let out = move_point(&mut p, s, (50.0, 50.0), (50.0, 500.0)).unwrap();
    assert_eq!(out.regenerated, vec![d]);
    assert!(
        out.notes.iter().any(|n| n.contains("recréés")),
        "{:?}",
        out.notes
    );
    let piece = p.piece(d).unwrap();
    assert_eq!(piece.outline.len(), 4);
    assert!(piece.outline.iter().all(|e| !old.contains(&e.entity)));
    // Plus aucune trace des anciens tracés dans la feuille dédiée, ni de point orphelin.
    let sh = p.sheet(sheet_of(&p, d)).unwrap();
    assert_eq!(sh.entities.len(), 4);
    assert!(sh.points.iter().all(|q| sh.point_is_used(q.id)));
    let c = p.piece_contour(d).unwrap();
    assert!(c.closed && !c.self_intersecting);
    // Les distances : chaque sommet du décalage est au moins à 5 mm de la mère.
    let mother = polygon(&p, m);
    let mut closed = mother.clone();
    closed.push(mother[0]);
    for q in polygon(&p, d) {
        assert!(dist_to_polyline(q, &closed) >= 5.0 - 1e-6, "{q:?}");
    }
    // Retour à la pointe : topologie à 3 tracés de nouveau.
    move_point(&mut p, s, (50.0, 500.0), (50.0, 50.0)).unwrap();
    assert_eq!(p.piece(d).unwrap().outline.len(), 3);
    assert_offset_of(&p, m, d, 5.0);
}

#[test]
fn ca2_regeneration_impossible_derivee_obsolete_mere_non_refusee() {
    let (mut p, s, m) = rect_mother(200.0, 100.0);
    let d = derive(&mut p, m, DerivedKind::Lining, -10.0).unwrap();
    let ds = sheet_of(&p, d);
    let geometry_before = (
        p.sheet(ds).unwrap().clone(),
        p.piece(d).unwrap().outline.clone(),
    );
    let next_before = p.next_id;
    // Nœud papillon : le contour de la mère se recoupe.
    let out = move_point(&mut p, s, (200.0, 100.0), (-50.0, 100.0)).unwrap();
    assert!(out.regenerated.is_empty());
    assert!(
        out.notes
            .iter()
            .any(|n| n.contains("n'a pas pu être mise à jour")),
        "{:?}",
        out.notes
    );
    // Dernière géométrie valide conservée, à l'identique.
    assert_eq!(*p.sheet(ds).unwrap(), geometry_before.0);
    assert_eq!(p.piece(d).unwrap().outline, geometry_before.1);
    assert_eq!(p.next_id, next_before, "aucun identifiant brûlé");
    let info = p.piece_derivation(d).unwrap();
    let DerivedState::Stale { reason } = &info.state else {
        panic!("la dérivée devrait être obsolète");
    };
    assert!(reason.contains("recoupe"), "{reason}");
    // Avertissement, pas une erreur de validation.
    assert!(p.piece_issues(d).contains(&PieceIssue::DerivedStale {
        reason: reason.clone()
    }));
    assert!(p.validate().is_ok());
    // Le drapeau est écrit et relu à l'identique.
    let json = persist::to_json(&p).unwrap();
    assert!(json.contains("\"stale\""));
    assert_eq!(persist::from_json(&json).unwrap(), p);
    // Une commande sans effet sur la mère laisse l'état obsolète tel quel.
    p.apply(Command::AddPoint {
        sheet: s,
        at: pt(1.0, 1.0),
    })
    .unwrap();
    assert!(matches!(
        p.piece_derivation(d).unwrap().state,
        DerivedState::Stale { .. }
    ));
    // Réparer la mère : la dérivée est recalculée et n'est plus obsolète.
    let out = move_point(&mut p, s, (-50.0, 100.0), (200.0, 100.0)).unwrap();
    assert_eq!(out.regenerated, vec![d]);
    assert_eq!(p.piece_derivation(d).unwrap().state, DerivedState::Current);
    assert_offset_of(&p, m, d, -10.0);
    assert!(!persist::to_json(&p).unwrap().contains("\"stale\""));
}

#[test]
fn ca2_obsolescence_annulable_et_mere_sans_contour() {
    let (mut p, s, m) = rect_mother(200.0, 100.0);
    let d = derive(&mut p, m, DerivedKind::Lining, -10.0).unwrap();
    let before = p.clone();
    let mut doc = Document::new(p);
    // Supprimer un tracé du contour de la mère vide ce contour (décision 36).
    let edge = before.piece(m).unwrap().outline[0].entity;
    doc.execute(Command::Delete {
        sheet: s,
        points: vec![],
        entities: vec![edge],
    })
    .unwrap();
    let info = doc.project().piece_derivation(d).unwrap();
    assert!(
        matches!(&info.state, DerivedState::Stale { reason } if reason.contains("n'a pas de contour")),
        "{info:?}"
    );
    // Même géométrie qu'avant.
    assert_eq!(
        doc.project().sheet(sheet_of(&before, d)).unwrap(),
        before.sheet(sheet_of(&before, d)).unwrap()
    );
    assert!(doc.undo());
    assert_eq!(*doc.project(), before);
}

#[test]
fn ca2_derivee_de_derivee_suit_toute_la_chaine() {
    let (mut p, s, m) = rect_mother(200.0, 100.0);
    let b = derive(&mut p, m, DerivedKind::Lining, -10.0).unwrap();
    let c = derive(&mut p, b, DerivedKind::Interfacing, -5.0).unwrap();
    assert_eq!(p.piece_derivation(c).unwrap().mother, b);
    let cc = p.piece_contour(c).unwrap();
    assert!(near(cc.area_mm2, 170.0 * 70.0, 1e-6));
    let out = move_point(&mut p, s, (200.0, 100.0), (170.0, 90.0)).unwrap();
    assert_eq!(out.regenerated, vec![b, c], "mère avant fille");
    assert_offset_of(&p, m, b, -10.0);
    assert_offset_of(&p, b, c, -5.0);
    // Annuler d'un coup.
    let mut doc = Document::new(p.clone());
    move_point(&mut p, s, (170.0, 90.0), (180.0, 95.0)).unwrap();
    assert_offset_of(&p, b, c, -5.0);
    doc.execute(Command::MovePoint {
        sheet: s,
        point: find_point(doc.project(), s, 170.0, 90.0),
        to: pt(160.0, 95.0),
    })
    .unwrap();
    assert!(doc.undo());
    assert_offset_of(doc.project(), b, c, -5.0);
    // Supprimer la mère directe casse seulement son propre lien.
    p.apply(Command::DeletePiece { piece: b }).unwrap();
    assert!(p.piece(c).unwrap().derived.is_none());
    assert!(p.piece(b).is_err());
}

#[test]
fn ca2_modifier_directement_une_derivee_liee_est_refuse() {
    let (mut p, _, m) = rect_mother(200.0, 100.0);
    let d = derive(&mut p, m, DerivedKind::Lining, -10.0).unwrap();
    let ds = sheet_of(&p, d);
    let corner = {
        let sh = p.sheet(ds).unwrap();
        sh.points[0].id
    };
    let e = assert_refused(
        &mut p,
        Command::MovePoint {
            sheet: ds,
            point: corner,
            to: pt(50.0, 50.0),
        },
    );
    assert!(e.to_string().contains("Cassez le lien"), "{e}");
    let edge = p.piece(d).unwrap().outline[0].entity;
    assert_refused(
        &mut p,
        Command::Delete {
            sheet: ds,
            points: vec![],
            entities: vec![edge],
        },
    );
    let all: Vec<PointId> = p.sheet(ds).unwrap().points.iter().map(|q| q.id).collect();
    assert_refused(
        &mut p,
        Command::Transform {
            sheet: ds,
            points: all,
            transform: Transform::Translate {
                dx_mm: 5.0,
                dy_mm: 0.0,
            },
        },
    );
    // Ce qui n'est pas la géométrie du contour reste possible.
    p.apply(Command::SetPieceCard {
        piece: d,
        name: "Doublure A".into(),
        code: Some("D1".into()),
        quantity: 2,
        mirrored_pair: false,
        material: None,
        category: Some(PieceCategory::Lining),
        comment: None,
    })
    .unwrap();
    p.apply(Command::AddNotch {
        sheet: ds,
        entity: edge,
        distance_mm: 20.0,
        side: pf_geometry::Side::Left,
        length_mm: 5.0,
        style: NotchStyle::Single,
    })
    .unwrap();
    p.apply(Command::AddPoint {
        sheet: ds,
        at: pt(1.0, 1.0),
    })
    .unwrap();
}

#[test]
fn ca2_la_mere_supprimee_casse_les_liens_dans_la_meme_etape() {
    let (mut p, s, m) = rect_mother(200.0, 100.0);
    let d = derive(&mut p, m, DerivedKind::Lining, -10.0).unwrap();
    let before = p.clone();
    let out = p.apply(Command::DeletePiece { piece: m }).unwrap();
    assert!(p.piece(d).unwrap().derived.is_none());
    assert!(out.notes.iter().any(|n| n.contains("n'est plus dérivée")));
    assert!(p.validate().is_ok());
    // La feuille de la mère supprimée : même résultat.
    let mut q = before.clone();
    let out = q.apply(Command::DeleteSheet { sheet: s }).unwrap();
    assert!(q.piece(d).unwrap().derived.is_none());
    assert!(out.notes.iter().any(|n| n.contains("n'est plus dérivée")));
    // Annuler d'un coup : le lien revient.
    let mut doc = Document::new(before.clone());
    doc.execute(Command::DeletePiece { piece: m }).unwrap();
    assert!(doc.undo());
    assert_eq!(*doc.project(), before);
    // Supprimer la feuille dédiée de la dérivée supprime la pièce (décision 36).
    let ds = sheet_of(&before, d);
    let mut r = before.clone();
    r.apply(Command::DeleteSheet { sheet: ds }).unwrap();
    assert!(r.piece(d).is_err());
    assert!(r.piece(m).is_ok());
}

#[test]
fn ca2_seules_les_dependantes_sont_recalculees() {
    // 50 pièces ; 3 dérivées (deux de la pièce 0, une de la pièce 1).
    let mut p = Project::new("cinquante");
    let s = p.sheets[0].id;
    let mut pieces = Vec::new();
    for i in 0..50 {
        let o = p
            .apply(Command::AddRectangle {
                sheet: s,
                corner: PointRef::New(pt(i as f64 * 150.0, 0.0)),
                width_mm: 100.0,
                height_mm: 60.0,
            })
            .unwrap();
        pieces.push(
            p.apply(Command::ExtractPiece {
                sheet: s,
                source: PieceSource::Paths {
                    entities: o.entities,
                },
                name: format!("P{i}"),
                detached: false,
            })
            .unwrap()
            .pieces[0],
        );
    }
    let d0a = derive(&mut p, pieces[0], DerivedKind::Lining, -5.0).unwrap();
    let d0b = derive(&mut p, pieces[0], DerivedKind::Interfacing, 3.0).unwrap();
    let d1 = derive(&mut p, pieces[1], DerivedKind::Lining, -5.0).unwrap();
    // Pièce 30 sans dérivée : rien n'est recalculé.
    let out = move_point(
        &mut p,
        s,
        (30.0 * 150.0 + 100.0, 60.0),
        (30.0 * 150.0 + 90.0, 60.0),
    )
    .unwrap();
    assert!(out.regenerated.is_empty(), "{:?}", out.regenerated);
    // Pièce 0 : ses deux dérivées seulement.
    let out = move_point(&mut p, s, (100.0, 60.0), (90.0, 70.0)).unwrap();
    assert_eq!(out.regenerated, vec![d0a, d0b]);
    // Pièce 1 : sa seule dérivée.
    let out = move_point(&mut p, s, (250.0, 60.0), (240.0, 60.0)).unwrap();
    assert_eq!(out.regenerated, vec![d1]);
    // Une commande sans lien avec un contour ne recalcule rien.
    let out = p
        .apply(Command::AddPoint {
            sheet: s,
            at: pt(5.0, 500.0),
        })
        .unwrap();
    assert!(out.regenerated.is_empty());
    let out = p
        .apply(Command::RenameProject {
            name: "autre".into(),
        })
        .unwrap();
    assert!(out.regenerated.is_empty());
    // Le coût ne dépend pas du nombre de pièces non liées : mêmes 3 dérivées
    // recalculées au plus, quel que soit le déplacement.
    let mut x = 0.0;
    for k in 1..=10 {
        let to = -(k as f64);
        let out = move_point(&mut p, s, (x, 0.0), (to, 0.0)).unwrap();
        x = to;
        assert_eq!(out.regenerated, vec![d0a, d0b]);
    }
    let _ = d1;
}

// ============================================================ CA3 : lien

#[test]
fn ca3_casser_le_lien() {
    let (mut p, s, m) = rect_mother(200.0, 100.0);
    let d = derive(&mut p, m, DerivedKind::Lining, -10.0).unwrap();
    let ds = sheet_of(&p, d);
    let linked = p.clone();
    let out = p.apply(Command::BreakDerivedLink { piece: d }).unwrap();
    assert!(out.regenerated.is_empty());
    assert!(p.piece(d).unwrap().derived.is_none());
    assert!(p.piece_derivation(d).is_none());
    assert!(p.validate().is_ok());
    // La mère bouge : la ex-dérivée ne bouge plus.
    let geometry = p.sheet(ds).unwrap().clone();
    let out = move_point(&mut p, s, (200.0, 100.0), (150.0, 100.0)).unwrap();
    assert!(out.regenerated.is_empty());
    assert_eq!(*p.sheet(ds).unwrap(), geometry);
    assert!(near(p.piece_contour(d).unwrap().area_mm2, 14_400.0, 1e-6));
    // Sa géométrie est maintenant modifiable comme toute pièce ordinaire.
    let corner = p.sheet(ds).unwrap().points[0].id;
    p.apply(Command::MovePoint {
        sheet: ds,
        point: corner,
        to: pt(0.0, 0.0),
    })
    .unwrap();
    // Annulation : le lien revient.
    let mut doc = Document::new(linked.clone());
    doc.execute(Command::BreakDerivedLink { piece: d }).unwrap();
    assert!(doc.project().piece(d).unwrap().derived.is_none());
    assert!(doc.undo());
    assert_eq!(*doc.project(), linked);
    assert!(doc.project().piece(d).unwrap().derived.is_some());
    // Et le fichier d'une pièce au lien cassé n'a plus de clé « derived ».
    let mut q = linked.clone();
    q.apply(Command::BreakDerivedLink { piece: d }).unwrap();
    assert!(!persist::to_json(&q).unwrap().contains("\"derived\""));
}

#[test]
fn ca3_refus_sans_trace() {
    let (mut p, _, m) = rect_mother(200.0, 100.0);
    // La mère n'est pas dérivée.
    let e = assert_refused(&mut p, Command::BreakDerivedLink { piece: m });
    assert!(e.to_string().contains("pas une pièce dérivée"), "{e}");
    assert!(matches!(
        assert_refused(
            &mut p,
            Command::BreakDerivedLink {
                piece: PieceId(4242)
            }
        ),
        CoreError::UnknownPiece(_)
    ));
    // Casser deux fois : la seconde est refusée.
    let d = derive(&mut p, m, DerivedKind::Lining, -10.0).unwrap();
    p.apply(Command::BreakDerivedLink { piece: d }).unwrap();
    assert_refused(&mut p, Command::BreakDerivedLink { piece: d });
}

// ================================================== CA4 : intégrité du fichier

#[test]
fn ca4_fichiers_sans_derivee_octets_inchanges() {
    for name in FIXTURES {
        let first = persist::to_json(&fixture(name)).unwrap();
        assert!(!first.contains("\"derived\""), "{name}");
        assert_eq!(
            first,
            persist::to_json(&persist::from_json(&first).unwrap()).unwrap(),
            "{name}"
        );
    }
    let (p, _, _) = rect_mother(100.0, 50.0);
    let t = text(&p);
    assert!(!t.contains("derived") && !t.contains("stale"));
    assert_eq!(t, text(&persist::from_json(&t).unwrap()));
    for file in [
        "piece-crans-droit-fil.pforge",
        "geometrie-exercice.pforge",
        "grande-piece-600x450.pforge",
    ] {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples")
            .join(file);
        let shipped = std::fs::read_to_string(path).unwrap().replace("\r\n", "\n");
        let again = persist::to_json(&persist::from_json(&shipped).unwrap()).unwrap();
        assert_eq!(shipped, again, "{file}");
    }
}

#[test]
fn ca4_relecture_a_l_identique_avec_derivee() {
    let (mut p, s, m) = rect_mother(200.0, 100.0);
    let d = derive(&mut p, m, DerivedKind::Facing, -12.5).unwrap();
    let e = derive(&mut p, d, DerivedKind::Reinforcement, 3.25).unwrap();
    let t = text(&p);
    assert!(t.contains("\"derived\"") && t.contains("\"facing\"") && t.contains("\"offset\""));
    let back = persist::from_json(&t).unwrap();
    assert_eq!(back, p);
    assert_eq!(text(&back), t, "octets identiques");
    // Avec une obsolescence.
    move_point(&mut p, s, (200.0, 100.0), (-50.0, 100.0)).unwrap();
    assert!(matches!(
        p.piece_derivation(d).unwrap().state,
        DerivedState::Stale { .. }
    ));
    let t = text(&p);
    assert!(t.contains("\"stale\""));
    assert_eq!(text(&persist::from_json(&t).unwrap()), t);
    let _ = e;
}

/// Projet avec deux dérivées D1 (de la mère) et D2 (de D1), et leur index JSON.
fn two_derived() -> (Value, PieceId, PieceId, PieceId) {
    let (mut p, _, m) = rect_mother(200.0, 100.0);
    let d1 = derive(&mut p, m, DerivedKind::Lining, -10.0).unwrap();
    let d2 = derive(&mut p, d1, DerivedKind::Interfacing, -5.0).unwrap();
    (to_value(&p), m, d1, d2)
}

fn piece_json(v: &mut Value, id: PieceId) -> &mut Value {
    v["pieces"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|p| p["id"] == json!(id.0))
        .unwrap()
}

fn open_err(v: &Value) -> CoreError {
    persist::from_json(&v.to_string()).expect_err("l'ouverture aurait dû être refusée")
}

#[test]
fn ca4_refus_a_l_ouverture() {
    let (base, m, d1, d2) = two_derived();
    assert!(persist::from_json(&base.to_string()).is_ok());
    let mut cases: Vec<(&str, Value, &str)> = Vec::new();
    let mut with = |label: &'static str, expect: &'static str, f: &dyn Fn(&mut Value)| {
        let mut v = base.clone();
        f(&mut v);
        cases.push((label, v, expect));
    };
    with("mère inexistante", "mère absente", &|v| {
        piece_json(v, d1)["derived"]["mother"] = json!(987_654);
    });
    with("auto-référence", "elle-même", &|v| {
        piece_json(v, d1)["derived"]["mother"] = json!(d1.0);
    });
    with("cycle D1 ↔ D2", "cycle", &|v| {
        piece_json(v, d1)["derived"]["mother"] = json!(d2.0);
    });
    with("distance nulle", "distance", &|v| {
        piece_json(v, d1)["derived"]["operation"]["distance_mm"] = json!(0.0);
    });
    with("distance trop grande", "distance", &|v| {
        piece_json(v, d1)["derived"]["operation"]["distance_mm"] = json!(1e12);
    });
    with("distance trop petite", "distance", &|v| {
        piece_json(v, d1)["derived"]["operation"]["distance_mm"] = json!(1e-6);
    });
    with("distance en texte", "", &|v| {
        piece_json(v, d1)["derived"]["operation"]["distance_mm"] = json!("dix");
    });
    with("clé inconnue dans derived", "unknown field", &|v| {
        piece_json(v, d1)["derived"]["coin"] = json!("rond");
    });
    with("clé inconnue dans operation", "unknown field", &|v| {
        piece_json(v, d1)["derived"]["operation"]["corner"] = json!("round");
    });
    with("opération inconnue", "", &|v| {
        piece_json(v, d1)["derived"]["operation"] = json!({"type": "band", "width_mm": 20.0});
    });
    with("type de dérivée inconnu", "", &|v| {
        piece_json(v, d1)["derived"]["kind"] = json!("bretelle");
    });
    with("mère absente du fichier (champ manquant)", "", &|v| {
        piece_json(v, d1)["derived"]
            .as_object_mut()
            .unwrap()
            .remove("mother");
    });
    with("raison vide", "obsolescence", &|v| {
        piece_json(v, d1)["derived"]["stale"] = json!("   ");
    });
    with(
        "raison avec caractère de contrôle",
        "obsolescence",
        &|v| {
            piece_json(v, d1)["derived"]["stale"] = json!("a\u{1}b");
        },
    );
    with("raison trop longue", "obsolescence", &|v| {
        piece_json(v, d1)["derived"]["stale"] = json!("x".repeat(501));
    });
    with("dérivée détachée", "liée", &|v| {
        piece_json(v, d1)["link"] = json!({"type": "detached"});
    });
    assert_eq!(cases.len(), 16);
    for (label, v, expect) in &cases {
        let err = open_err(v);
        let msg = err.to_string();
        assert!(
            msg.to_lowercase().contains(&expect.to_lowercase()),
            "{label} : message « {msg} » sans « {expect} »"
        );
    }
    // Profondeur : D2 → D1 → mère, plus 8 niveaux au-dessus est refusé.
    let (mut p, _, m0) = rect_mother(400.0, 300.0);
    let mut cur = m0;
    let mut chain = Vec::new();
    for _ in 0..8 {
        cur = derive(&mut p, cur, DerivedKind::Lining, -1.0).unwrap();
        chain.push(cur);
    }
    let mut v = to_value(&p);
    persist::from_json(&v.to_string()).expect("8 niveaux acceptés");
    // Raccorde la mère à la dernière : cycle de longueur 9.
    piece_json(&mut v, m0)["derived"] = json!({
        "mother": chain[7].0, "kind": "lining", "operation": {"type": "offset", "distance_mm": -1.0}
    });
    let err = open_err(&v);
    assert!(err.to_string().contains("cycle") || err.to_string().contains("niveaux"));
    let _ = m;
}

#[test]
fn ca4_champ_derived_dans_extra_refuse() {
    let (mut p, _, m) = rect_mother(100.0, 50.0);
    p.pieces
        .iter_mut()
        .find(|q| q.id == m)
        .unwrap()
        .extra
        .insert("derived".into(), json!({"mother": 1}));
    let err = p.validate().expect_err("clé homonyme dans extra");
    assert!(err.to_string().contains("derived"), "{err}");
    assert!(persist::to_json(&p).is_err());
}

#[test]
fn ca4_ancienne_version_garde_derived_dans_extra_sans_perte() {
    // Une version antérieure (sans le champ typé) lit `derived` dans `extra` et le
    // réécrit tel quel : on simule en lisant le fichier avec un champ inconnu
    // voisin, qui doit survivre à l'aller-retour.
    let (mut p, _, m) = rect_mother(100.0, 50.0);
    derive(&mut p, m, DerivedKind::Lining, -5.0).unwrap();
    let mut v = to_value(&p);
    v["pieces"][0]["futur"] = json!({"x": 1});
    let q = persist::from_json(&v.to_string()).unwrap();
    assert_eq!(to_value(&q)["pieces"][0]["futur"], json!({"x": 1}));
    assert!(to_value(&q)["pieces"][1]["derived"].is_object());
}

// ============================================================ CA5 : lecture

#[test]
fn ca5_lecture_fiche_et_avertissement() {
    let (mut p, s, m) = rect_mother(200.0, 100.0);
    assert!(p.piece_derivation(m).is_none());
    assert!(p.piece_card(m).unwrap().derived.is_none());
    assert!(p.derived_pieces_of(m).is_empty());
    let d = derive(&mut p, m, DerivedKind::Interfacing, -10.0).unwrap();
    let info = p.piece_derivation(d).unwrap();
    assert_eq!(info.mother, m);
    assert_eq!(info.mother_name, "Mère");
    assert_eq!(info.kind, DerivedKind::Interfacing);
    assert_eq!(info.operation, offset(-10.0));
    assert_eq!(info.state, DerivedState::Current);
    assert_eq!(p.derived_pieces_of(m), vec![d]);
    let card = p.piece_card(d).unwrap();
    assert_eq!(card.derived, Some(info));
    assert_eq!(card.category, Some(PieceCategory::Interfacing));
    assert!(near(card.area_mm2.unwrap(), 14_400.0, 1e-6));
    assert!(!card
        .issues
        .iter()
        .any(|i| matches!(i, PieceIssue::DerivedStale { .. })));
    // Pièce inconnue.
    assert!(p.piece_derivation(PieceId(31_337)).is_none());
    // Obsolète : l'avertissement et la clé stable.
    move_point(&mut p, s, (200.0, 100.0), (-50.0, 100.0)).unwrap();
    let card = p.piece_card(d).unwrap();
    let stale = card
        .issues
        .iter()
        .find(|i| matches!(i, PieceIssue::DerivedStale { .. }))
        .expect("DerivedStale");
    assert_eq!(stale.key(), "derived_stale");
    assert!(matches!(
        card.derived.as_ref().unwrap().state,
        DerivedState::Stale { .. }
    ));
    // Casser le lien : plus d'avertissement.
    p.apply(Command::BreakDerivedLink { piece: d }).unwrap();
    assert!(!p
        .piece_issues(d)
        .iter()
        .any(|i| matches!(i, PieceIssue::DerivedStale { .. })));
    assert!(p.piece_card(d).unwrap().derived.is_none());
}

// ===================================================== CA7 : copie de feuille

#[test]
fn ca7_dupliquer_la_feuille_de_la_mere_copie_la_mere_sans_derivee() {
    let (mut p, s, m) = rect_mother(200.0, 100.0);
    let d = derive(&mut p, m, DerivedKind::Lining, -10.0).unwrap();
    let out = p.apply(Command::DuplicateSheet { sheet: s }).unwrap();
    let copy = out.pieces[0];
    assert!(p.piece(copy).unwrap().derived.is_none());
    assert_eq!(
        p.derived_pieces_of(m),
        vec![d],
        "la dérivée suit l'original"
    );
    assert!(p.derived_pieces_of(copy).is_empty());
    assert!(p.validate().is_ok());
    // Déplacer l'original met à jour la dérivée ; la copie ne bouge pas.
    let copy_sheet = out.sheet.unwrap();
    let copy_before = p.sheet(copy_sheet).unwrap().clone();
    let out = move_point(&mut p, s, (200.0, 100.0), (150.0, 100.0)).unwrap();
    assert_eq!(out.regenerated, vec![d]);
    assert_eq!(*p.sheet(copy_sheet).unwrap(), copy_before);
}

#[test]
fn ca7_dupliquer_la_feuille_de_la_derivee_casse_le_lien_de_la_copie() {
    let (mut p, _, m) = rect_mother(200.0, 100.0);
    let d = derive(&mut p, m, DerivedKind::Lining, -10.0).unwrap();
    let ds = sheet_of(&p, d);
    let out = p.apply(Command::DuplicateSheet { sheet: ds }).unwrap();
    assert!(
        out.notes.iter().any(|n| n.contains("n'est plus liée")),
        "{:?}",
        out.notes
    );
    let copy = out.pieces[0];
    assert!(p.piece(copy).unwrap().derived.is_none());
    assert!(p.piece(d).unwrap().derived.is_some());
    assert!(p.validate().is_ok());
    // La copie est une pièce ordinaire : sa géométrie est modifiable.
    let cs = sheet_of(&p, copy);
    let corner = p.sheet(cs).unwrap().points[0].id;
    p.apply(Command::MovePoint {
        sheet: cs,
        point: corner,
        to: pt(0.0, 0.0),
    })
    .unwrap();
    // Annuler la duplication : une étape.
    let (q, _, qm) = rect_mother(200.0, 100.0);
    let mut q = q;
    let qd = derive(&mut q, qm, DerivedKind::Lining, -10.0).unwrap();
    let initial = q.clone();
    let mut doc = Document::new(q);
    doc.execute(Command::DuplicateSheet {
        sheet: sheet_of(doc.project(), qd),
    })
    .unwrap();
    assert!(doc.undo());
    assert_eq!(*doc.project(), initial);
}

#[test]
fn ca7_importer_une_feuille_d_un_autre_projet_casse_le_lien() {
    let (mut a, _, m) = rect_mother(200.0, 100.0);
    let d = derive(&mut a, m, DerivedKind::Lining, -10.0).unwrap();
    let cmd = a.import_command(sheet_of(&a, d)).unwrap();
    let mut b = Project::new("autre");
    let out = b.apply(cmd).unwrap();
    assert!(b.pieces.iter().all(|q| q.derived.is_none()));
    assert!(out.notes.iter().any(|n| n.contains("n'est plus liée")));
    assert!(b.validate().is_ok());
    // Un ImportSheet forgé qui porte une dérivée vers une mère étrangère : même règle.
    let mut forged = a.piece(d).unwrap().clone();
    forged.derived = Some(DerivedSpec {
        mother: PieceId(12_345),
        kind: DerivedKind::Facing,
        operation: offset(-1.0),
        stale: None,
    });
    let sheet = a.sheet(sheet_of(&a, d)).unwrap().clone();
    let mut c = Project::new("forgé");
    c.apply(Command::ImportSheet {
        sheet: Box::new(sheet),
        pieces: vec![forged],
    })
    .unwrap();
    assert!(c.pieces[0].derived.is_none());
    // Le presse-papiers ne transporte pas les pièces (rien à changer).
    let dsheet = sheet_of(&a, d);
    let ents: Vec<EntityId> = a
        .sheet(dsheet)
        .unwrap()
        .entities
        .iter()
        .map(|e| e.id)
        .collect();
    let fragment = a.copy_fragment(dsheet, &[], &ents, &[], &[]).unwrap();
    let mut e = Project::new("coller");
    let es = e.sheets[0].id;
    e.apply(Command::PasteFragment {
        sheet: es,
        fragment: Box::new(fragment),
        layer: None,
    })
    .unwrap();
    assert!(e.pieces.is_empty());
}
