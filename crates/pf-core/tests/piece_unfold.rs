//! US-04.7 — déplier : une pièce construite sur une pliure devient une pièce entière
//! symétrique, liée à la pièce source. Un groupe de tests par critère d'acceptation :
//! CA1 dépliage exact (valeurs calculées à la main), CA2 lien avec la source (même étape
//! annulable, obsolescence, lien cassable, copie sans lien), CA3 refus sans trace,
//! CA4 une étape annulable, CA5 octets inchangés, CA6 sérialisation et cohérence avec
//! placement, plan, éléments internes, séparer et assembler.

use pf_core::contour::{contour_polyline, ContourEdge};
use pf_core::persist;
use pf_core::piece::PieceIssue;
use pf_core::*;
use pf_geometry::{distance_mm, PointMm};
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

fn bytes(p: &Project) -> String {
    persist::to_json(p).unwrap()
}

fn setup() -> (Project, SheetId) {
    let p = Project::new("déplier");
    let s = p.sheets[0].id;
    (p, s)
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

fn segment(p: &mut Project, sheet: SheetId, a: PointMm, b: PointMm) -> EntityId {
    p.apply(Command::AddSegment {
        sheet,
        from: PointRef::New(a),
        to: PointRef::New(b),
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

fn rect_piece(p: &mut Project, sheet: SheetId, w: f64, h: f64, name: &str) -> PieceId {
    let o = p
        .apply(Command::AddRectangle {
            sheet,
            corner: PointRef::New(pt(0.0, 0.0)),
            width_mm: w,
            height_mm: h,
        })
        .unwrap();
    extract_paths(p, sheet, o.entities, name)
}

fn ends_of(shape: &Shape) -> (PointMm, PointMm) {
    match shape {
        Shape::Line(a, b) => (*a, *b),
        Shape::Cubic(c) => (c.p0, c.p3),
        Shape::Arc(a) => (a.start_point(), a.end_point()),
        other => panic!("forme inattendue {other:?}"),
    }
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

fn area(p: &Project, id: PieceId) -> f64 {
    p.piece_contour(id).unwrap().area_mm2
}

fn perimeter(p: &Project, id: PieceId) -> f64 {
    p.piece_contour(id).unwrap().perimeter_mm
}

fn unfold(p: &mut Project, piece: PieceId, fold: EntityId) -> Result<Outcome, CoreError> {
    p.apply(Command::UnfoldPiece {
        piece,
        fold,
        name: None,
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

fn refuse_unfold(p: &mut Project, piece: PieceId, fold: EntityId) -> String {
    refused(
        p,
        Command::UnfoldPiece {
            piece,
            fold,
            name: None,
        },
    )
}

fn polyline(p: &Project, piece: PieceId) -> Vec<PointMm> {
    let sheet = p.piece_sheet(piece).unwrap();
    let edges: Vec<ContourEdge> = p
        .piece(piece)
        .unwrap()
        .outline
        .iter()
        .map(|e| ContourEdge {
            entity: e.entity,
            reversed: e.reversed,
        })
        .collect();
    contour_polyline(sheet, &edges, 48).unwrap()
}

/// Le contour est égal à son image par `reflect` (chaque point échantillonné a son image).
fn assert_symmetric(p: &Project, piece: PieceId, reflect: impl Fn(PointMm) -> PointMm) {
    let line = polyline(p, piece);
    for q in &line {
        let image = reflect(*q);
        assert!(
            line.iter().any(|r| distance_mm(*r, image) < 1e-9),
            "{q:?} n'a pas d'image {image:?} dans le contour"
        );
    }
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

fn sheet_of(p: &Project, piece: PieceId) -> SheetId {
    p.piece_sheet(piece).unwrap().id
}

/// Demi-corps : (0,0) (60,0) (80,40) (70,100) (0,110), pliure sur l'axe x = 0.
fn half_body() -> (Project, SheetId, PieceId, EntityId) {
    let (mut p, s) = setup();
    let m = poly_piece(
        &mut p,
        s,
        &[
            pt(0.0, 0.0),
            pt(60.0, 0.0),
            pt(80.0, 40.0),
            pt(70.0, 100.0),
            pt(0.0, 110.0),
        ],
        "Demi-corps",
    );
    let fold = edge_between(&p, m, pt(0.0, 110.0), pt(0.0, 0.0));
    (p, s, m, fold)
}

// ======================================================================== CA1

#[test]
fn ca1_demi_corps_polygonal_valeurs_calculees_a_la_main() {
    let (mut p, s, m, fold) = half_body();
    let sheets_before = serde_json::to_string(&p.sheets).unwrap();
    let source_before = p.piece(m).unwrap().clone();
    let o = unfold(&mut p, m, fold).unwrap();
    let u = o.pieces[0];
    // Formule du lacet sur (0,0) (60,0) (80,40) (70,100) (0,110) : 15300 / 2 = 7650 mm².
    assert!(near(area(&p, m), 7650.0, 1e-9));
    // Aire = 2 × aire de la moitié ; le sens est anti-horaire.
    assert!(
        near(area(&p, u), 15_300.0, 15_300.0 * 1e-9),
        "{}",
        area(&p, u)
    );
    let c = p.piece_contour(u).unwrap();
    assert!(c.closed && !c.self_intersecting && !c.clockwise);
    // Périmètre = 2 × (périmètre de la moitié − pliure).
    let half_edges = 60.0 + 2000f64.sqrt() + 3700f64.sqrt() + 5000f64.sqrt();
    assert!(near(perimeter(&p, m), half_edges + 110.0, 1e-9));
    let want = 2.0 * half_edges;
    assert!(
        near(perimeter(&p, u), want, want * 1e-9),
        "{}",
        perimeter(&p, u)
    );
    assert!(near(
        perimeter(&p, u),
        2.0 * (perimeter(&p, m) - 110.0),
        want * 1e-9
    ));
    // Sommets : ceux de la moitié et leurs images x → −x ; la pliure a disparu.
    let cs = corners(&p, u);
    assert_eq!(cs.len(), 8, "{cs:?}");
    for q in [
        pt(0.0, 0.0),
        pt(60.0, 0.0),
        pt(80.0, 40.0),
        pt(70.0, 100.0),
        pt(0.0, 110.0),
        pt(-60.0, 0.0),
        pt(-80.0, 40.0),
        pt(-70.0, 100.0),
    ] {
        assert!(has_corner(&cs, q), "sommet manquant {q:?} dans {cs:?}");
    }
    let sheet = p.piece_sheet(u).unwrap();
    for e in &p.piece(u).unwrap().outline {
        let (a, b) = ends_of(&sheet.shape(e.entity).unwrap());
        assert!(
            !(near(a.x, 0.0, 1e-9) && near(b.x, 0.0, 1e-9)),
            "un tracé reste sur la pliure"
        );
    }
    assert_symmetric(&p, u, |q| pt(-q.x, q.y));
    // Chaque arête est raccordée à la suivante (contour fermé unique).
    let n = p.piece(u).unwrap().outline.len();
    let ends: Vec<(PointMm, PointMm)> = p
        .piece(u)
        .unwrap()
        .outline
        .iter()
        .map(|e| {
            let (a, b) = ends_of(&sheet.shape(e.entity).unwrap());
            if e.reversed {
                (b, a)
            } else {
                (a, b)
            }
        })
        .collect();
    for j in 0..n {
        assert!(distance_mm(ends[j].1, ends[(j + 1) % n].0) < 1e-9);
    }
    // La source et la construction ne sont pas touchées.
    assert_eq!(*p.piece(m).unwrap(), source_before);
    assert_eq!(
        serde_json::to_string(&p.sheets[..1]).unwrap(),
        format!("[{}]", &sheets_before[1..sheets_before.len() - 1])
    );
    assert_eq!(p.sheet(s).unwrap().entities.len(), 5);
    p.validate().unwrap();
}

#[test]
fn ca1_rectangle_pliure_sur_chacun_des_quatre_bords() {
    for (fa, fb, fold_len) in [
        (pt(0.0, 0.0), pt(100.0, 0.0), 100.0),
        (pt(100.0, 0.0), pt(100.0, 60.0), 60.0),
        (pt(100.0, 60.0), pt(0.0, 60.0), 100.0),
        (pt(0.0, 60.0), pt(0.0, 0.0), 60.0),
    ] {
        let (mut p, s) = setup();
        let m = rect_piece(&mut p, s, 100.0, 60.0, "R");
        let fold = edge_between(&p, m, fa, fb);
        let u = unfold(&mut p, m, fold).unwrap().pieces[0];
        assert!(near(area(&p, u), 12_000.0, 1e-9), "{fa:?}");
        assert!(near(perimeter(&p, u), 2.0 * (320.0 - fold_len), 1e-9));
        assert_eq!(corners(&p, u).len(), 6);
        // Axe de la pliure : l'image de chaque sommet par la symétrie est un sommet.
        let (d, n) = (fb - fa, (fb - fa).unit().unwrap());
        let _ = d;
        assert_symmetric(&p, u, |q| {
            let v = q - fa;
            let k = 2.0 * n.dot(v);
            pt(fa.x + n.x * k - v.x, fa.y + n.y * k - v.y)
        });
        // La pliure n'est plus un bord : aucune arête ne relie ses extrémités.
        let sheet = p.piece_sheet(u).unwrap();
        for e in &p.piece(u).unwrap().outline {
            let (a, b) = ends_of(&sheet.shape(e.entity).unwrap());
            assert!(
                !((distance_mm(a, fa) < 1e-9 && distance_mm(b, fb) < 1e-9)
                    || (distance_mm(a, fb) < 1e-9 && distance_mm(b, fa) < 1e-9))
            );
        }
    }
}

#[test]
fn ca1_pliure_oblique_triangle_sur_la_diagonale() {
    let (mut p, s) = setup();
    let m = poly_piece(
        &mut p,
        s,
        &[pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 10.0)],
        "Triangle",
    );
    let fold = edge_between(&p, m, pt(10.0, 10.0), pt(0.0, 0.0));
    let u = unfold(&mut p, m, fold).unwrap().pieces[0];
    // Le triangle sous la diagonale y = x plus son image : le carré de 10 mm.
    assert!(near(area(&p, u), 100.0, 1e-9));
    assert!(near(perimeter(&p, u), 40.0, 1e-9));
    let cs = corners(&p, u);
    for q in [pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 10.0), pt(0.0, 10.0)] {
        assert!(has_corner(&cs, q), "{q:?} dans {cs:?}");
    }
}

#[test]
fn ca1_contour_a_bezier_dome_pliure_sur_la_base() {
    let (mut p, s) = setup();
    let base = segment(&mut p, s, pt(0.0, 0.0), pt(100.0, 0.0));
    let curve = p
        .apply(Command::AddCurve {
            sheet: s,
            start: PointRef::New(pt(100.0, 0.0)),
            c1: pt(100.0, 100.0),
            c2: pt(0.0, 100.0),
            end: PointRef::New(pt(0.0, 0.0)),
        })
        .unwrap()
        .entities[0];
    let m = extract_paths(&mut p, s, vec![base, curve], "Dôme");
    let u = unfold(&mut p, m, base).unwrap().pieces[0];
    // Aire du dôme : 6000 mm² (∫ de la Bézier) ; l'entier en a le double.
    assert!(near(area(&p, m), 6000.0, 1e-6), "{}", area(&p, m));
    assert!(near(
        area(&p, u),
        2.0 * area(&p, m),
        2.0 * area(&p, m) * 1e-9
    ));
    // Longueur de la Bézier par la règle de Simpson, indépendamment de la bibliothèque.
    let speed = |t: f64| {
        let (dx, dy) = (-600.0 * t * (1.0 - t), 300.0 * (1.0 - 2.0 * t));
        dx.hypot(dy)
    };
    let n = 20_000;
    let h = 1.0 / f64::from(n);
    let mut len = speed(0.0) + speed(1.0);
    for i in 1..n {
        len += speed(h * f64::from(i)) * if i % 2 == 1 { 4.0 } else { 2.0 };
    }
    len *= h / 3.0;
    assert!(
        near(perimeter(&p, u), 2.0 * len, 1e-6),
        "{}",
        perimeter(&p, u)
    );
    assert!(near(
        perimeter(&p, u),
        2.0 * (perimeter(&p, m) - 100.0),
        1e-9 * perimeter(&p, u)
    ));
    // Deux Béziers : l'original et son image y → −y, points de contrôle réfléchis.
    let sheet = p.piece_sheet(u).unwrap();
    let curves: Vec<[PointMm; 4]> = p
        .piece(u)
        .unwrap()
        .outline
        .iter()
        .filter_map(|e| match sheet.shape(e.entity).unwrap() {
            Shape::Cubic(c) => Some(if e.reversed {
                [c.p3, c.c2, c.c1, c.p0]
            } else {
                [c.p0, c.c1, c.c2, c.p3]
            }),
            _ => None,
        })
        .collect();
    assert_eq!(curves.len(), 2);
    let original = [
        pt(100.0, 0.0),
        pt(100.0, 100.0),
        pt(0.0, 100.0),
        pt(0.0, 0.0),
    ];
    let mirrored = [
        pt(100.0, 0.0),
        pt(100.0, -100.0),
        pt(0.0, -100.0),
        pt(0.0, 0.0),
    ];
    let same = |a: &[PointMm; 4], b: &[PointMm; 4]| (0..4).all(|k| distance_mm(a[k], b[k]) < 1e-9);
    let reversed = |c: &[PointMm; 4]| [c[3], c[2], c[1], c[0]];
    assert!(curves
        .iter()
        .any(|c| same(c, &original) || same(&reversed(c), &original)));
    assert!(curves
        .iter()
        .any(|c| same(c, &mirrored) || same(&reversed(c), &mirrored)));
    assert_symmetric(&p, u, |q| pt(q.x, -q.y));
}

#[test]
fn ca1_demi_disque_a_arc_donne_un_disque() {
    let (mut p, s) = setup();
    let diameter = segment(&mut p, s, pt(-50.0, 0.0), pt(50.0, 0.0));
    let arc = p
        .apply(Command::AddArcThreePoints {
            sheet: s,
            start: PointRef::New(pt(50.0, 0.0)),
            through: PointRef::New(pt(0.0, 50.0)),
            end: PointRef::New(pt(-50.0, 0.0)),
        })
        .unwrap()
        .entities[0];
    let m = extract_paths(&mut p, s, vec![diameter, arc], "Demi-disque");
    let u = unfold(&mut p, m, diameter).unwrap().pieces[0];
    assert!(near(area(&p, u), PI * 2500.0, 1e-9 * PI * 2500.0));
    assert!(near(perimeter(&p, u), 2.0 * PI * 50.0, 1e-9));
    let sheet = p.piece_sheet(u).unwrap();
    let arcs: Vec<_> = p
        .piece(u)
        .unwrap()
        .outline
        .iter()
        .filter_map(|e| match sheet.shape(e.entity).unwrap() {
            Shape::Arc(a) => Some(a),
            _ => None,
        })
        .collect();
    assert_eq!(arcs.len(), 2);
    for a in &arcs {
        assert!(distance_mm(a.center, pt(0.0, 0.0)) < 1e-9);
        assert!(near(a.radius_mm, 50.0, 1e-9));
        assert!(near(a.sweep_rad.abs(), PI, 1e-9));
    }
    assert_symmetric(&p, u, |q| pt(q.x, -q.y));
}

#[test]
fn ca1_la_depliee_est_une_piece_liee_sur_sa_feuille_et_herite_de_la_fiche() {
    let (mut p, _, m, fold) = half_body();
    p.apply(Command::SetPieceCard {
        piece: m,
        name: "Demi-corps".into(),
        code: Some("DC".into()),
        quantity: 2,
        mirrored_pair: true,
        material: Some("Coton".into()),
        category: Some(PieceCategory::Fabric),
        comment: Some("couper sur la pliure".into()),
    })
    .unwrap();
    let o = unfold(&mut p, m, fold).unwrap();
    let u = p.piece(o.pieces[0]).unwrap();
    assert_eq!(u.name, "Demi-corps – dépliée");
    assert_eq!(u.code.as_deref(), Some("DC-D"));
    assert_eq!(u.quantity, 2);
    assert_eq!(u.material.as_deref(), Some("Coton"));
    assert_eq!(u.category, Some(PieceCategory::Fabric));
    assert!(u.comment.is_none(), "le commentaire décrit la moitié");
    assert!(!u.mirrored_pair, "une pièce entière n'est pas une paire");
    assert!(u.placement.is_none() && u.grainline.is_none() && u.internal.is_empty());
    let PieceLink::Linked { sheet } = u.link else {
        panic!("la pièce dépliée doit être liée à sa feuille");
    };
    assert_eq!(o.sheet, Some(sheet));
    assert_ne!(sheet, p.sheets[0].id);
    let info = p.piece_derivation(u.id).unwrap();
    assert_eq!(info.mother, m);
    assert_eq!(info.kind, DerivedKind::Unfolded);
    assert_eq!(info.operation, DerivedOp::Unfold { fold });
    assert_eq!(info.state, DerivedState::Current);
    assert_eq!(p.derived_pieces_of(m), vec![u.id]);
    // Nom donné : respecté ; nom vide : défaut.
    let (mut q, _, qm, qf) = half_body();
    let named = q
        .apply(Command::UnfoldPiece {
            piece: qm,
            fold: qf,
            name: Some("  Corps entier ".into()),
        })
        .unwrap();
    assert_eq!(q.piece(named.pieces[0]).unwrap().name, "Corps entier");
    // Nom de 120 caractères : le suffixe est raccourci pour tenir.
    let (mut r, rs) = setup();
    let long = "N".repeat(120);
    let rm = rect_piece(&mut r, rs, 100.0, 50.0, &long);
    let rf = edge_between(&r, rm, pt(0.0, 50.0), pt(0.0, 0.0));
    let ru = unfold(&mut r, rm, rf).unwrap().pieces[0];
    assert!(r.piece(ru).unwrap().name.chars().count() <= 120);
    assert!(r.piece(ru).unwrap().name.ends_with(" – dépliée"));
    r.validate().unwrap();
}

#[test]
fn ca1_droit_fil_et_elements_internes_non_repris_avec_remarque() {
    let (mut p, s, m, fold) = half_body();
    let grain = segment(&mut p, s, pt(10.0, 20.0), pt(10.0, 80.0));
    let inner = segment(&mut p, s, pt(20.0, 20.0), pt(40.0, 60.0));
    p.apply(Command::SetPieceOutline {
        piece: m,
        outline: p.piece(m).unwrap().outline.clone(),
        grainline: Some(grain),
    })
    .unwrap();
    p.apply(Command::AddPieceInternal {
        piece: m,
        target: InternalTarget::Path { entity: inner },
        kind: InternalKind::Topstitch,
    })
    .unwrap();
    let o = unfold(&mut p, m, fold).unwrap();
    let u = o.pieces[0];
    assert!(
        o.notes.iter().any(|n| n.contains("droit-fil")
            && n.contains("ne sont pas repris")
            && n.contains("1 élément")),
        "{:?}",
        o.notes
    );
    assert!(p.piece(u).unwrap().grainline.is_none());
    assert!(p.piece(u).unwrap().internal.is_empty());
    assert!(p.piece_issues(u).contains(&PieceIssue::MissingGrainline));
    // La source garde les siens.
    assert_eq!(p.piece(m).unwrap().grainline, Some(grain));
    assert_eq!(p.piece(m).unwrap().internal.len(), 1);
    // Sans droit-fil ni élément interne : pas de remarque.
    let (mut q, _, qm, qf) = half_body();
    assert!(unfold(&mut q, qm, qf).unwrap().notes.is_empty());
}

#[test]
fn ca1_apercu_lecture_pure_memes_mesures_et_memes_refus() {
    let (mut p, _, m, fold) = half_body();
    let before = bytes(&p);
    let pre = p.unfold_preview(m, fold, None).unwrap();
    assert_eq!(before, bytes(&p), "l'aperçu ne modifie rien");
    assert!(near(pre.half_area_mm2, 7650.0, 1e-9));
    assert!(near(pre.area_mm2, 15_300.0, 15_300.0 * 1e-9));
    assert!(near(pre.fold_length_mm, 110.0, 1e-9));
    assert!(distance_mm(pre.fold_start, pt(0.0, 110.0)) < 1e-9);
    assert!(distance_mm(pre.fold_end, pt(0.0, 0.0)) < 1e-9);
    assert!(near(
        pre.perimeter_mm,
        2.0 * (pre.half_perimeter_mm - pre.fold_length_mm),
        1e-9 * pre.perimeter_mm
    ));
    assert_eq!(pre.name, "Demi-corps – dépliée");
    assert!(pre.outline.len() >= 8 && pre.half_outline.len() >= 5);
    // Mêmes mesures que la commande.
    let u = unfold(&mut p, m, fold).unwrap().pieces[0];
    assert!(near(pre.area_mm2, area(&p, u), 1e-9));
    assert!(near(pre.perimeter_mm, perimeter(&p, u), 1e-9));
    // Mêmes refus : pliure déjà dépliée, pliure absente.
    assert!(p.unfold_preview(m, fold, None).is_err());
    assert!(p.unfold_preview(m, EntityId(999_999), None).is_err());
    assert!(p.unfold_preview(PieceId(999_999), fold, None).is_err());
    // Candidates : les segments du contour, le tracé de rôle « pliure » en premier.
    let (mut q, _, qm, qf) = half_body();
    q.apply(Command::SetPathRole {
        sheet: q.sheets[0].id,
        entity: qf,
        role: PathRole::Fold,
    })
    .unwrap();
    let c = q.unfold_candidates(qm);
    assert_eq!(c.len(), 5);
    assert_eq!(c[0].entity, qf);
    assert!(c[0].is_fold_role && near(c[0].length_mm, 110.0, 1e-9));
    assert!(c[1..].iter().all(|x| !x.is_fold_role));
    assert!(q.unfold_candidates(PieceId(999_999)).is_empty());
}

// ======================================================================== CA2

#[test]
fn ca2_modifier_la_moitie_met_a_jour_la_depliee_dans_la_meme_etape_annulable() {
    let (p, s, m, fold) = half_body();
    let mut d = Document::new(p);
    let u = d
        .execute(Command::UnfoldPiece {
            piece: m,
            fold,
            name: None,
        })
        .unwrap()
        .pieces[0];
    let ids_before = d.project().piece(u).unwrap().outline.clone();
    let before = d.project().clone();
    let before_bytes = bytes(&before);
    // (70, 100) → (75, 120) : lacet (0,0) (60,0) (80,40) (75,120) (0,110) = 17250 / 2.
    let point = find_point(d.project(), s, 70.0, 100.0);
    let out = d
        .execute(Command::MovePoint {
            sheet: s,
            point,
            to: pt(75.0, 120.0),
        })
        .unwrap();
    assert_eq!(out.regenerated, vec![u]);
    let after = d.project().clone();
    assert!(near(area(&after, m), 8625.0, 1e-9));
    assert!(near(area(&after, u), 17_250.0, 17_250.0 * 1e-9));
    assert_eq!(
        after.piece(u).unwrap().outline,
        ids_before,
        "identifiants stables"
    );
    let cs = corners(&after, u);
    assert!(has_corner(&cs, pt(75.0, 120.0)) && has_corner(&cs, pt(-75.0, 120.0)));
    assert!(!has_corner(&cs, pt(70.0, 100.0)) && !has_corner(&cs, pt(-70.0, 100.0)));
    assert_symmetric(&after, u, |q| pt(-q.x, q.y));
    assert_eq!(
        after.piece_derivation(u).unwrap().state,
        DerivedState::Current
    );
    // Une seule étape pour les deux changements.
    assert!(d.undo());
    assert_eq!(*d.project(), before);
    assert_eq!(bytes(d.project()), before_bytes);
    assert!(d.redo());
    assert_eq!(*d.project(), after);
    // Déplacer la pliure elle-même : la dépliée suit (l'axe s'incline).
    let top = find_point(d.project(), s, 0.0, 110.0);
    d.execute(Command::MovePoint {
        sheet: s,
        point: top,
        to: pt(10.0, 110.0),
    })
    .unwrap();
    let now = d.project();
    assert!(near(
        area(now, u),
        2.0 * area(now, m),
        2.0 * area(now, m) * 1e-9
    ));
    assert_eq!(
        now.piece_derivation(u).unwrap().state,
        DerivedState::Current
    );
}

#[test]
fn ca2_regeneration_impossible_obsolete_la_commande_de_la_source_n_est_pas_refusee() {
    // L : (0,0) (60,0) (60,100) (10,100) (10,50) (0,50), pliure (0,50)→(0,0).
    let (mut p, s) = setup();
    let m = poly_piece(
        &mut p,
        s,
        &[
            pt(0.0, 0.0),
            pt(60.0, 0.0),
            pt(60.0, 100.0),
            pt(10.0, 100.0),
            pt(10.0, 50.0),
            pt(0.0, 50.0),
        ],
        "L",
    );
    let fold = edge_between(&p, m, pt(0.0, 50.0), pt(0.0, 0.0));
    let u = unfold(&mut p, m, fold).unwrap().pieces[0];
    let us = sheet_of(&p, u);
    let geometry_before = (
        p.sheet(us).unwrap().clone(),
        p.piece(u).unwrap().outline.clone(),
    );
    let next_before = p.next_id;
    // Le sommet (10, 100) passe de l'autre côté de la droite de pliure : la source reste simple.
    let out = move_point(&mut p, s, (10.0, 100.0), (-30.0, 100.0)).unwrap();
    assert!(out.regenerated.is_empty());
    assert!(
        out.notes
            .iter()
            .any(|n| n.contains("n'a pas pu être mise à jour")),
        "{:?}",
        out.notes
    );
    let DerivedState::Stale { reason } = p.piece_derivation(u).unwrap().state else {
        panic!("la dépliée devrait être obsolète");
    };
    assert!(reason.contains("traverse"), "{reason}");
    assert!(p.piece_issues(u).contains(&PieceIssue::DerivedStale {
        reason: reason.clone()
    }));
    // Dernière géométrie valide conservée, à l'identique, sans identifiant brûlé.
    assert_eq!(*p.sheet(us).unwrap(), geometry_before.0);
    assert_eq!(p.piece(u).unwrap().outline, geometry_before.1);
    assert_eq!(p.next_id, next_before);
    p.validate().unwrap();
    // Écrit, relu à l'identique.
    let json = bytes(&p);
    assert!(json.contains("\"stale\""));
    assert_eq!(persist::from_json(&json).unwrap(), p);
    // Réparer la source : la dépliée est recalculée.
    let out = move_point(&mut p, s, (-30.0, 100.0), (10.0, 100.0)).unwrap();
    assert_eq!(out.regenerated, vec![u]);
    assert_eq!(p.piece_derivation(u).unwrap().state, DerivedState::Current);
    assert!(near(area(&p, u), 2.0 * area(&p, m), 1e-6));
}

#[test]
fn ca2_la_pliure_retiree_du_contour_ou_supprimee_rend_la_depliee_obsolete() {
    let (mut p, _, m, fold) = half_body();
    let u = unfold(&mut p, m, fold).unwrap().pieces[0];
    // Contour réécrit sans la pliure (un contour ouvert) : obsolète, pas refusé.
    let outline: Vec<PieceEdge> = p
        .piece(m)
        .unwrap()
        .outline
        .iter()
        .filter(|e| e.entity != fold)
        .copied()
        .collect();
    let out = p
        .apply(Command::SetPieceOutline {
            piece: m,
            outline,
            grainline: None,
        })
        .unwrap();
    assert!(out.regenerated.is_empty());
    assert!(matches!(
        p.piece_derivation(u).unwrap().state,
        DerivedState::Stale { .. }
    ));
    p.validate().unwrap();
    // Remettre le contour d'origine : la dépliée est de nouveau à jour.
    let sheet = p.sheets[0].id;
    let _ = sheet;
    let mut full = p.piece(m).unwrap().outline.clone();
    full.push(PieceEdge {
        entity: fold,
        reversed: false,
    });
    let out = p
        .apply(Command::SetPieceOutline {
            piece: m,
            outline: full,
            grainline: None,
        })
        .unwrap();
    assert_eq!(out.regenerated, vec![u]);
    assert_eq!(p.piece_derivation(u).unwrap().state, DerivedState::Current);
    // Pliure supprimée de la construction : le contour change, obsolète encore.
    let out = p
        .apply(Command::Delete {
            sheet: p.sheets[0].id,
            points: vec![],
            entities: vec![fold],
        })
        .unwrap();
    assert!(out.regenerated.is_empty());
    assert!(matches!(
        p.piece_derivation(u).unwrap().state,
        DerivedState::Stale { .. }
    ));
    p.validate().unwrap();
}

#[test]
fn ca2_casser_le_lien_laisse_une_piece_ordinaire() {
    let (mut p, s, m, fold) = half_body();
    let u = unfold(&mut p, m, fold).unwrap().pieces[0];
    p.apply(Command::BreakDerivedLink { piece: u }).unwrap();
    assert!(p.piece(u).unwrap().derived.is_none());
    assert!(p.piece_derivation(u).is_none());
    let geometry = p.sheet(sheet_of(&p, u)).unwrap().clone();
    // La source change : la pièce libérée ne bouge plus.
    let out = move_point(&mut p, s, (70.0, 100.0), (75.0, 120.0)).unwrap();
    assert!(out.regenerated.is_empty());
    assert_eq!(*p.sheet(sheet_of(&p, u)).unwrap(), geometry);
    // Sa géométrie est de nouveau modifiable.
    let us = sheet_of(&p, u);
    let v = find_point(&p, us, 60.0, 0.0);
    p.apply(Command::MovePoint {
        sheet: us,
        point: v,
        to: pt(61.0, 0.0),
    })
    .unwrap();
    // Casser un lien qui n'existe pas : refus.
    refused(&mut p, Command::BreakDerivedLink { piece: u });
    refused(&mut p, Command::BreakDerivedLink { piece: m });
    p.validate().unwrap();
}

#[test]
fn ca2_modifier_directement_la_depliee_liee_est_refuse() {
    let (mut p, _, m, fold) = half_body();
    let u = unfold(&mut p, m, fold).unwrap().pieces[0];
    let us = sheet_of(&p, u);
    let v = find_point(&p, us, 60.0, 0.0);
    let msg = refused(
        &mut p,
        Command::MovePoint {
            sheet: us,
            point: v,
            to: pt(61.0, 0.0),
        },
    );
    assert!(msg.contains("Cassez le lien"), "{msg}");
    let msg = refused(
        &mut p,
        Command::SetPieceOutline {
            piece: u,
            outline: vec![],
            grainline: None,
        },
    );
    assert!(msg.contains("Cassez le lien"), "{msg}");
}

#[test]
fn ca2_copie_de_feuille_sans_lien() {
    let (mut p, s, m, fold) = half_body();
    let u = unfold(&mut p, m, fold).unwrap().pieces[0];
    // Copier la feuille de la dépliée : la copie est une pièce ordinaire.
    let out = p
        .apply(Command::DuplicateSheet {
            sheet: sheet_of(&p, u),
        })
        .unwrap();
    assert!(
        out.notes.iter().any(|n| n.contains("n'est plus liée")),
        "{:?}",
        out.notes
    );
    let copy = out.pieces[0];
    assert!(p.piece(copy).unwrap().derived.is_none());
    assert!(p.piece(u).unwrap().derived.is_some());
    // Copier la feuille de la source : la copie n'a pas de dérivée, l'original garde la sienne.
    let out = p.apply(Command::DuplicateSheet { sheet: s }).unwrap();
    let src_copy = out.pieces[0];
    assert!(p.piece(src_copy).unwrap().derived.is_none());
    assert_eq!(p.derived_pieces_of(m), vec![u]);
    assert!(p.derived_pieces_of(src_copy).is_empty());
    // Exporter la feuille dépliée vers un autre projet : sans lien.
    let cmd = p.import_command(sheet_of(&p, u)).unwrap();
    let mut other = Project::new("autre");
    let out = other.apply(cmd).unwrap();
    assert!(other.pieces.iter().all(|q| q.derived.is_none()));
    assert!(out.notes.iter().any(|n| n.contains("n'est plus liée")));
    p.validate().unwrap();
    other.validate().unwrap();
}

#[test]
fn ca2_supprimer_la_source_casse_le_lien_dans_la_meme_etape() {
    let (mut p, _, m, fold) = half_body();
    let u = unfold(&mut p, m, fold).unwrap().pieces[0];
    let out = p.apply(Command::DeletePiece { piece: m }).unwrap();
    assert!(p.piece(u).unwrap().derived.is_none());
    assert!(
        out.notes.iter().any(|n| n.contains("n'est plus dérivée")),
        "{:?}",
        out.notes
    );
    p.validate().unwrap();
}

#[test]
fn ca2_deux_pliures_deux_depliees_independantes() {
    let (mut p, s) = setup();
    let m = rect_piece(&mut p, s, 100.0, 60.0, "R");
    let left = edge_between(&p, m, pt(0.0, 60.0), pt(0.0, 0.0));
    let bottom = edge_between(&p, m, pt(0.0, 0.0), pt(100.0, 0.0));
    let a = unfold(&mut p, m, left).unwrap().pieces[0];
    let b = unfold(&mut p, m, bottom).unwrap().pieces[0];
    assert_eq!(p.derived_pieces_of(m), vec![a, b]);
    assert!(near(area(&p, a), 12_000.0, 1e-9) && near(area(&p, b), 12_000.0, 1e-9));
    // Les deux suivent la source dans la même étape.
    let out = move_point(&mut p, s, (100.0, 60.0), (100.0, 80.0)).unwrap();
    assert_eq!(out.regenerated, vec![a, b]);
    assert!(near(area(&p, a), 2.0 * area(&p, m), 1e-6));
    assert!(near(area(&p, b), 2.0 * area(&p, m), 1e-6));
}

// ======================================================================== CA3

#[test]
fn ca3_refus_sans_trace_pieces_et_pliures_invalides() {
    let (mut p, s, m, fold) = half_body();
    // Pièce inexistante, pliure absente du contour, tracé de la construction hors contour.
    refuse_unfold(&mut p, PieceId(999_999), fold);
    let msg = refuse_unfold(&mut p, m, EntityId(999_999));
    assert!(msg.contains("n'est pas un tracé du contour"), "{msg}");
    let outside = segment(&mut p, s, pt(0.0, -50.0), pt(0.0, 200.0));
    let msg = refuse_unfold(&mut p, m, outside);
    assert!(msg.contains("n'est pas un tracé du contour"), "{msg}");
    // Pièce sans contour.
    let empty = p
        .apply(Command::AddPiece {
            sheet: s,
            name: "Étiquette".into(),
            quantity: 1,
            material: None,
            mirrored_pair: false,
        })
        .unwrap()
        .pieces[0];
    let msg = refuse_unfold(&mut p, empty, fold);
    assert!(msg.contains("n'a pas de contour"), "{msg}");
    // Pliure déjà dépliée.
    unfold(&mut p, m, fold).unwrap();
    let msg = refuse_unfold(&mut p, m, fold);
    assert!(msg.contains("déjà dépliée"), "{msg}");
    // Nom invalide.
    let (mut q, _, qm, qf) = half_body();
    refused(
        &mut q,
        Command::UnfoldPiece {
            piece: qm,
            fold: qf,
            name: Some("a\u{7}b".into()),
        },
    );
    refused(
        &mut q,
        Command::UnfoldPiece {
            piece: qm,
            fold: qf,
            name: Some("N".repeat(121)),
        },
    );
    // Type et opération incohérents.
    refused(
        &mut q,
        Command::CreateDerivedPiece {
            mother: qm,
            kind: DerivedKind::Unfolded,
            operation: DerivedOp::Offset { distance_mm: -3.0 },
            name: None,
        },
    );
    refused(
        &mut q,
        Command::CreateDerivedPiece {
            mother: qm,
            kind: DerivedKind::Lining,
            operation: DerivedOp::Unfold { fold: qf },
            name: None,
        },
    );
}

#[test]
fn ca3_refus_pliure_non_rectiligne_cercle_courbe_arc() {
    // Dôme : la Bézier n'est pas une pliure.
    let (mut p, s) = setup();
    let base = segment(&mut p, s, pt(0.0, 0.0), pt(100.0, 0.0));
    let curve = p
        .apply(Command::AddCurve {
            sheet: s,
            start: PointRef::New(pt(100.0, 0.0)),
            c1: pt(100.0, 100.0),
            c2: pt(0.0, 100.0),
            end: PointRef::New(pt(0.0, 0.0)),
        })
        .unwrap()
        .entities[0];
    let dome = extract_paths(&mut p, s, vec![base, curve], "Dôme");
    let msg = refuse_unfold(&mut p, dome, curve);
    assert!(msg.contains("segment rectiligne"), "{msg}");
    // Cercle.
    let circle = p
        .apply(Command::AddCircle {
            sheet: s,
            center: PointRef::New(pt(300.0, 300.0)),
            radius_mm: 40.0,
        })
        .unwrap()
        .entities[0];
    let disc = extract_paths(&mut p, s, vec![circle], "Disque");
    let msg = refuse_unfold(&mut p, disc, circle);
    assert!(msg.contains("segment rectiligne"), "{msg}");
    // Arc.
    let diameter = segment(&mut p, s, pt(500.0, 0.0), pt(600.0, 0.0));
    let arc = p
        .apply(Command::AddArcThreePoints {
            sheet: s,
            start: PointRef::New(pt(600.0, 0.0)),
            through: PointRef::New(pt(550.0, 50.0)),
            end: PointRef::New(pt(500.0, 0.0)),
        })
        .unwrap()
        .entities[0];
    let half_disc = extract_paths(&mut p, s, vec![diameter, arc], "Demi-disque");
    let msg = refuse_unfold(&mut p, half_disc, arc);
    assert!(msg.contains("segment rectiligne"), "{msg}");
}

#[test]
fn ca3_refus_contour_ouvert() {
    let (mut p, _, m, fold) = half_body();
    let mut outline = p.piece(m).unwrap().outline.clone();
    let removed = outline.iter().position(|e| e.entity != fold).unwrap();
    outline.remove(removed);
    p.apply(Command::SetPieceOutline {
        piece: m,
        outline,
        grainline: None,
    })
    .unwrap();
    let msg = refuse_unfold(&mut p, m, fold);
    assert!(msg.contains("pas fermé"), "{msg}");
}

#[test]
fn ca3_refus_contour_qui_traverse_ou_touche_la_pliure() {
    // Un sommet de l'autre côté de la droite de pliure (la pliure est plus courte que le
    // contour : le polygone reste simple).
    let (mut p, s) = setup();
    let crossing = poly_piece(
        &mut p,
        s,
        &[
            pt(0.0, 0.0),
            pt(60.0, 0.0),
            pt(60.0, 100.0),
            pt(-30.0, 100.0),
            pt(10.0, 50.0),
            pt(0.0, 50.0),
        ],
        "Traverse",
    );
    let fold = edge_between(&p, crossing, pt(0.0, 50.0), pt(0.0, 0.0));
    let msg = refuse_unfold(&mut p, crossing, fold);
    assert!(msg.contains("traverse"), "{msg}");
    // Un sommet posé sur la droite, plus loin que la pliure : ils se touchent en un point.
    let (mut q, qs) = setup();
    let touching = poly_piece(
        &mut q,
        qs,
        &[
            pt(0.0, 0.0),
            pt(60.0, 0.0),
            pt(60.0, 100.0),
            pt(0.0, 100.0),
            pt(10.0, 80.0),
            pt(10.0, 50.0),
            pt(0.0, 50.0),
        ],
        "Touche",
    );
    let fold = edge_between(&q, touching, pt(0.0, 50.0), pt(0.0, 0.0));
    let msg = refuse_unfold(&mut q, touching, fold);
    assert!(msg.contains("touche"), "{msg}");
    // Un bord aligné séparé de la pliure : deux morceaux non raccordables.
    let (mut r, rs) = setup();
    let two = poly_piece(
        &mut r,
        rs,
        &[
            pt(0.0, 0.0),
            pt(60.0, 0.0),
            pt(60.0, 100.0),
            pt(0.0, 100.0),
            pt(0.0, 80.0),
            pt(30.0, 60.0),
            pt(0.0, 40.0),
        ],
        "Deux morceaux",
    );
    let fold = edge_between(&r, two, pt(0.0, 40.0), pt(0.0, 0.0));
    let msg = refuse_unfold(&mut r, two, fold);
    assert!(msg.contains("touche"), "{msg}");
    // Contour qui se recoupe (nœud papillon) : refusé avant tout dépliage.
    let (mut t, ts) = setup();
    let bow = poly_piece(
        &mut t,
        ts,
        &[pt(0.0, 0.0), pt(60.0, 40.0), pt(60.0, 0.0), pt(0.0, 40.0)],
        "Papillon",
    );
    let fold = edge_between(&t, bow, pt(0.0, 40.0), pt(0.0, 0.0));
    let msg = refuse_unfold(&mut t, bow, fold);
    assert!(msg.contains("recoupe"), "{msg}");
}

#[test]
fn ca3_refus_pieces_deja_derivees_et_depliee_de_depliee() {
    let (mut p, _, m, fold) = half_body();
    let lining = p
        .apply(Command::CreateDerivedPiece {
            mother: m,
            kind: DerivedKind::Lining,
            operation: DerivedOp::Offset { distance_mm: -3.0 },
            name: None,
        })
        .unwrap()
        .pieces[0];
    let some_edge = p.piece(lining).unwrap().outline[0].entity;
    let msg = refuse_unfold(&mut p, lining, some_edge);
    assert!(msg.contains("pièce dérivée"), "{msg}");
    let u = unfold(&mut p, m, fold).unwrap().pieces[0];
    let edge = p.piece(u).unwrap().outline[0].entity;
    let msg = refuse_unfold(&mut p, u, edge);
    assert!(msg.contains("pièce dérivée"), "{msg}");
    // Une fois le lien cassé, la pièce se déplie comme une autre : le bord du bas, prolongé
    // par son image, forme une pliure de 120 mm (deux segments alignés).
    p.apply(Command::BreakDerivedLink { piece: u }).unwrap();
    let bottom = edge_between(&p, u, pt(0.0, 0.0), pt(60.0, 0.0));
    let again = unfold(&mut p, u, bottom).unwrap().pieces[0];
    assert!(near(area(&p, again), 2.0 * 15_300.0, 1e-6));
    assert!(near(
        perimeter(&p, again),
        2.0 * (perimeter(&p, u) - 120.0),
        1e-6
    ));
    assert_eq!(p.piece_derivation(again).unwrap().mother, u);
    p.validate().unwrap();
}

#[test]
fn ca3_les_refus_ne_consomment_pas_d_identifiant() {
    let (mut p, _, m, fold) = half_body();
    let next = p.next_id;
    refuse_unfold(&mut p, m, EntityId(424_242));
    refuse_unfold(&mut p, PieceId(1), fold);
    assert_eq!(p.next_id, next);
}

// ======================================================================== CA4

#[test]
fn ca4_une_etape_annulable_exacte() {
    let (p, s, m, fold) = half_body();
    let mut d = Document::new(p);
    let start = d.project().clone();
    let start_bytes = bytes(&start);
    d.execute(Command::UnfoldPiece {
        piece: m,
        fold,
        name: None,
    })
    .unwrap();
    let after = d.project().clone();
    assert_ne!(after, start);
    assert!(d.undo());
    assert_eq!(*d.project(), start);
    assert_eq!(bytes(d.project()), start_bytes);
    assert!(!d.can_undo(), "une seule étape pour déplier");
    assert!(d.redo());
    assert_eq!(*d.project(), after);
    assert_eq!(bytes(d.project()), bytes(&after));
    // Annuler, rétablir plusieurs fois : exact.
    for _ in 0..3 {
        assert!(d.undo());
        assert_eq!(bytes(d.project()), start_bytes);
        assert!(d.redo());
        assert_eq!(*d.project(), after);
    }
    let _ = s;
}

#[test]
fn ca4_refus_ne_cree_pas_d_etape() {
    let (p, _, m, fold) = half_body();
    let mut d = Document::new(p);
    assert!(d
        .execute(Command::UnfoldPiece {
            piece: m,
            fold: EntityId(999_999),
            name: None,
        })
        .is_err());
    assert!(!d.can_undo());
    d.execute(Command::UnfoldPiece {
        piece: m,
        fold,
        name: None,
    })
    .unwrap();
    // Un second dépliage de la même pliure est refusé : une seule étape reste à annuler.
    assert!(d
        .execute(Command::UnfoldPiece {
            piece: m,
            fold,
            name: None,
        })
        .is_err());
    assert!(d.undo());
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
        // Aucun champ nouveau : le schéma reste 3 et rien de lié à US-04.7 n'est écrit.
        assert!(
            !text.contains("unfold") && !text.contains("unfolded"),
            "{name}"
        );
        let v: serde_json::Value = serde_json::from_str(text).unwrap();
        assert!(v["schema_version"].as_u64().unwrap() <= 6, "{name}");
    }
}

#[test]
fn ca5_sur_les_fixtures_et_exemples_le_depliage_ne_touche_pas_a_la_construction() {
    let mut paths: Vec<PathBuf> = Vec::new();
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    for name in FIXTURES {
        paths.push(fixtures.join(name));
    }
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.to_string_lossy().ends_with(".pforge.json") {
            paths.push(path);
        }
    }
    let mut tried = 0;
    for path in paths {
        let p0 = persist::load(&path).unwrap();
        for piece in p0.pieces.clone() {
            if p0.piece_contour(piece.id).map_or(true, |c| !c.closed) {
                continue;
            }
            for cand in p0.unfold_candidates(piece.id) {
                let mut p = p0.clone();
                let before_sheets = serde_json::to_value(&p.sheets).unwrap();
                let before_pieces = p.pieces.len();
                let Ok(o) = unfold(&mut p, piece.id, cand.entity) else {
                    assert_eq!(
                        bytes(&p),
                        bytes(&p0),
                        "{} : refus avec trace",
                        path.display()
                    );
                    continue;
                };
                tried += 1;
                let after = serde_json::to_value(&p.sheets).unwrap();
                // Construction existante intacte : seule une feuille est ajoutée.
                assert_eq!(
                    &after.as_array().unwrap()[..before_sheets.as_array().unwrap().len()],
                    before_sheets.as_array().unwrap().as_slice(),
                    "{}",
                    path.display()
                );
                assert_eq!(p.pieces.len(), before_pieces + 1);
                let a0 = p0.piece_contour(piece.id).unwrap().area_mm2;
                let a1 = area(&p, o.pieces[0]);
                assert!(near(a1, 2.0 * a0, 2.0 * a0 * 1e-9), "{}", path.display());
                p.validate().unwrap();
            }
        }
    }
    assert!(tried >= 1, "aucune pièce d'exemple dépliée");
}

// ======================================================================== CA6

#[test]
fn ca6_serialisation_relue_identique_et_champs_coherents() {
    let (mut p, s, m, fold) = half_body();
    let grain = segment(&mut p, s, pt(10.0, 20.0), pt(10.0, 80.0));
    p.apply(Command::SetPieceOutline {
        piece: m,
        outline: p.piece(m).unwrap().outline.clone(),
        grainline: Some(grain),
    })
    .unwrap();
    p.apply(Command::SetPiecePlacement {
        piece: m,
        placement: Some(Placement {
            x_mm: 10.0,
            y_mm: 20.0,
            rotation_deg: 30.0,
        }),
    })
    .unwrap();
    let u = unfold(&mut p, m, fold).unwrap().pieces[0];
    // Placement manuel de la dépliée : enregistré, jamais appliqué à la construction.
    p.apply(Command::SetPiecePlacement {
        piece: u,
        placement: Some(Placement {
            x_mm: 300.0,
            y_mm: 0.0,
            rotation_deg: 90.0,
        }),
    })
    .unwrap();
    let layout = p.plan_layout();
    assert!(layout.iter().any(|i| i.piece == u) && layout.iter().any(|i| i.piece == m));
    let text = bytes(&p);
    let back = persist::from_json(&text).unwrap();
    assert_eq!(back, p);
    assert_eq!(bytes(&back), text);
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["schema_version"], 6);
    let derived = v["pieces"]
        .as_array()
        .unwrap()
        .iter()
        .find_map(|q| q.get("derived"))
        .expect("la dépliée porte un lien");
    assert_eq!(derived["kind"], "unfolded");
    assert_eq!(derived["operation"]["type"], "unfold");
    assert_eq!(derived["operation"]["fold"], fold.0);
    assert!(derived.get("stale").is_none());
    // Seul le champ de l'opération est nouveau : la source n'a pas de lien.
    let source = v["pieces"]
        .as_array()
        .unwrap()
        .iter()
        .find(|q| q["id"] == m.0)
        .unwrap();
    assert!(source.get("derived").is_none());
    // Les lectures de fiche restent cohérentes.
    let card = p.piece_card(u).unwrap();
    assert_eq!(card.derived, p.piece_derivation(u));
    p.validate().unwrap();
}

#[test]
fn ca6_lecture_refuse_un_lien_incoherent() {
    let (mut p, _, m, fold) = half_body();
    let u = unfold(&mut p, m, fold).unwrap().pieces[0];
    let text = bytes(&p);
    // Type « facing » avec l'opération de dépliage : fichier incohérent, refusé à l'ouverture.
    let forged = text.replace("\"kind\": \"unfolded\"", "\"kind\": \"facing\"");
    assert_ne!(forged, text);
    assert!(persist::from_json(&forged).is_err());
    // Champ inconnu dans l'opération : refusé.
    let forged = text.replace("\"type\": \"unfold\"", "\"type\": \"unfold\", \"extra\": 1");
    assert_ne!(forged, text);
    assert!(persist::from_json(&forged).is_err());
    // Opération inconnue : refusée.
    let forged = text.replace("\"type\": \"unfold\"", "\"type\": \"refold\"");
    assert!(persist::from_json(&forged).is_err());
    // Mère absente : référence morte, refusée.
    let mother = format!("\"mother\": {}", m.0);
    assert!(text.contains(&mother));
    let forged = text.replace(&mother, "\"mother\": 987654");
    assert!(persist::from_json(&forged).is_err());
    let _ = u;
}

#[test]
fn ca6_separer_et_assembler_coherents_avec_les_depliees() {
    let (mut p, _, m, fold) = half_body();
    let u = unfold(&mut p, m, fold).unwrap().pieces[0];
    // La dépliée liée ne se sépare ni ne s'assemble.
    let msg = refused(
        &mut p,
        Command::SplitPiece {
            piece: u,
            a: pt(-100.0, 50.0),
            b: pt(100.0, 50.0),
        },
    );
    assert!(msg.contains("pièce dérivée"), "{msg}");
    let edge = p.piece(u).unwrap().outline[0].entity;
    refused(
        &mut p,
        Command::JoinPieces {
            first: u,
            first_edge: edge,
            second: m,
            second_edge: fold,
            tolerance_mm: 0.5,
        },
    );
    // Séparer la source la remplace : le lien de la dépliée est cassé, avec remarque.
    let out = p
        .apply(Command::SplitPiece {
            piece: m,
            a: pt(30.0, -5.0),
            b: pt(30.0, 5.0),
        })
        .unwrap();
    assert!(p.piece(u).unwrap().derived.is_none());
    assert!(
        out.notes.iter().any(|n| n.contains("n'est plus dérivée")),
        "{:?}",
        out.notes
    );
    // Libérée, la dépliée se sépare comme une pièce ordinaire (axe de symétrie x = 0).
    let halves = p
        .apply(Command::SplitPiece {
            piece: u,
            a: pt(0.0, -5.0),
            b: pt(0.0, 5.0),
        })
        .unwrap();
    assert_eq!(halves.pieces.len(), 2);
    let sum: f64 = halves.pieces.iter().map(|i| area(&p, *i)).sum();
    assert!(near(sum, 15_300.0, 15_300.0 * 1e-9), "{sum}");
    // Les deux moitiés sont symétriques l'une de l'autre : même aire.
    assert!(near(
        area(&p, halves.pieces[0]),
        area(&p, halves.pieces[1]),
        1e-6
    ));
    p.validate().unwrap();
}

#[test]
fn ca6_plan_de_pieces_et_placement_automatique() {
    let (mut p, _, m, fold) = half_body();
    let u = unfold(&mut p, m, fold).unwrap().pieces[0];
    let layout = p.plan_layout();
    let (lm, lu) = (
        layout.iter().find(|i| i.piece == m).unwrap(),
        layout.iter().find(|i| i.piece == u).unwrap(),
    );
    assert!(!lm.manual && !lu.manual);
    assert_ne!(lm.placement, lu.placement);
    // Le placement n'est jamais appliqué à la construction : feuille de la dépliée intacte.
    let sheet = p.sheet(sheet_of(&p, u)).unwrap().clone();
    p.apply(Command::SetPiecePlacement {
        piece: u,
        placement: Some(Placement {
            x_mm: 50.0,
            y_mm: 60.0,
            rotation_deg: 45.0,
        }),
    })
    .unwrap();
    assert_eq!(*p.sheet(sheet_of(&p, u)).unwrap(), sheet);
    // Régénérer la dépliée ne perd pas son placement.
    let s0 = p.sheets[0].id;
    move_point(&mut p, s0, (70.0, 100.0), (75.0, 120.0)).unwrap();
    assert_eq!(p.piece(u).unwrap().placement.unwrap().x_mm, 50.0);
}
