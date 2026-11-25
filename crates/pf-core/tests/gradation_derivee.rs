//! US-09.5a — géométrie dérivée gradée : toute donnée dérivée suit la taille par **réévaluation**
//! (aucune règle dupliquée, rien de stocké). Valeurs calculées à la main sur un rectangle 400 × 600
//! en base M, gamme XS S M L XL ; écarts cumulés `S (−5 ; −10)`, `L (+5 ; +10)`, `XL (+10 ; +22)`,
//! `XS (−9 ; −20)` : A reste fixe, B reçoit `(dx ; 0)`, C `(dx ; dy)`, D `(0 ; dy)`, donc la taille
//! `T` est un rectangle `(400 + dx) × (600 + dy)` :
//!
//! | taille | largeur | hauteur | périmètre | aire    |
//! |--------|---------|---------|-----------|---------|
//! | XS     | 391     | 580     | 1942      | 226 780 |
//! | S      | 395     | 590     | 1970      | 233 050 |
//! | M      | 400     | 600     | 2000      | 240 000 |
//! | L      | 405     | 610     | 2030      | 247 050 |
//! | XL     | 410     | 622     | 2064      | 255 020 |

use pf_core::notch::NotchPlace;
use pf_core::persist;
use pf_core::piece::PieceIssue;
use pf_core::*;
use pf_geometry::{distance_mm, PointMm, Side, Trace};

const TOL: f64 = 1e-6;

/// (libellé, dx, dy) des quatre tailles qui ont un écart ; M = base = (0 ; 0).
const DELTAS: [(&str, f64, f64); 4] = [
    ("XS", -9.0, -20.0),
    ("S", -5.0, -10.0),
    ("L", 5.0, 10.0),
    ("XL", 10.0, 22.0),
];
const SIZES: [&str; 5] = ["XS", "S", "M", "L", "XL"];

fn delta(label: &str) -> (f64, f64) {
    DELTAS
        .iter()
        .find(|d| d.0 == label)
        .map_or((0.0, 0.0), |d| (d.1, d.2))
}

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() <= TOL
}

fn close(a: PointMm, b: PointMm, ctx: &str) {
    assert!(
        distance_mm(a, b) <= TOL,
        "{ctx} : ({} ; {}) ≠ ({} ; {})",
        a.x,
        a.y,
        b.x,
        b.y
    );
}

fn num(a: f64, b: f64, ctx: &str) {
    assert!(near(a, b), "{ctx} : {a} ≠ {b}");
}

fn bytes(p: &Project) -> String {
    persist::to_json(p).expect("projet valide")
}

struct Fx {
    p: Project,
    sheet: SheetId,
    piece: PieceId,
    ab: EntityId,
    bc: EntityId,
    cd: EntityId,
    da: EntityId,
    a: PointId,
    b: PointId,
    c: PointId,
    d: PointId,
}

fn size(p: &Project, label: &str) -> SizeId {
    p.sizes
        .iter()
        .flat_map(|r| r.sizes.iter())
        .find(|s| s.label == label)
        .unwrap_or_else(|| panic!("taille {label}"))
        .id
}

fn point_at(p: &Project, sheet: SheetId, at: PointMm) -> PointId {
    let s = p.sheet(sheet).unwrap();
    s.points
        .iter()
        .find(|q| distance_mm(s.position(q.id).unwrap(), at) < 1e-9)
        .unwrap_or_else(|| panic!("aucun point en {at:?}"))
        .id
}

fn seg(p: &mut Project, sheet: SheetId, a: PointMm, b: PointMm) -> EntityId {
    p.apply(Command::AddSegment {
        sheet,
        from: PointRef::New(a),
        to: PointRef::New(b),
    })
    .unwrap()
    .entities[0]
}

fn grade(p: &mut Project, sheet: SheetId, point: PointId, label: &str, dx: f64, dy: f64) {
    let size = size(p, label);
    p.apply(Command::SetPointGrading {
        sheet,
        point,
        size,
        dx_mm: dx,
        dy_mm: dy,
    })
    .unwrap_or_else(|e| panic!("{label} : {e}"));
}

/// Gamme XS S M L XL (base M) seule.
fn with_sizes() -> (Project, SheetId) {
    let mut p = Project::new("Gradation dérivée");
    let sheet = p.sheets[0].id;
    p.apply(Command::AddSizeRange {
        name: "Femme".into(),
        kind: SizeKind::Alphanumeric,
        labels: SIZES.iter().map(|s| s.to_string()).collect(),
        base: 2,
    })
    .unwrap();
    (p, sheet)
}

/// Rectangle 400 × 600, pièce « Devant » (sans marge), coins gradués comme dans l'en-tête.
fn rect() -> Fx {
    let (mut p, sheet) = with_sizes();
    let (a, b, c, d) = (
        pt(0.0, 0.0),
        pt(400.0, 0.0),
        pt(400.0, 600.0),
        pt(0.0, 600.0),
    );
    let ab = seg(&mut p, sheet, a, b);
    let bc = seg(&mut p, sheet, b, c);
    let cd = seg(&mut p, sheet, c, d);
    let da = seg(&mut p, sheet, d, a);
    let piece = p
        .apply(Command::ExtractPiece {
            sheet,
            source: PieceSource::Paths {
                entities: vec![ab, bc, cd, da],
            },
            name: "Devant".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    let (ia, ib, ic, id) = (
        point_at(&p, sheet, a),
        point_at(&p, sheet, b),
        point_at(&p, sheet, c),
        point_at(&p, sheet, d),
    );
    for (label, dx, dy) in DELTAS {
        grade(&mut p, sheet, ib, label, dx, 0.0);
        grade(&mut p, sheet, ic, label, dx, dy);
        grade(&mut p, sheet, id, label, 0.0, dy);
    }
    Fx {
        p,
        sheet,
        piece,
        ab,
        bc,
        cd,
        da,
        a: ia,
        b: ib,
        c: ic,
        d: id,
    }
}

fn derived(p: &Project, sheet: SheetId, label: &str) -> SizeDerived {
    // `M` est la base : `Some(base)` et `None` donnent la même chose (testé plus bas).
    p.derived_for_size(sheet, Some(size(p, label)))
        .unwrap_or_else(|e| panic!("{label} : {e}"))
}

fn piece_of(d: &SizeDerived, piece: PieceId) -> &PieceDerived {
    d.pieces.iter().find(|x| x.piece == piece).expect("pièce")
}

fn pos(d: &SizeDerived, point: PointId) -> PointMm {
    d.points
        .iter()
        .find(|(id, _)| *id == point)
        .unwrap_or_else(|| panic!("point {} absent : {:?}", point.0, d.issues))
        .1
}

fn len_of(d: &SizeDerived, e: EntityId) -> f64 {
    d.path_lengths
        .iter()
        .find(|(id, _)| *id == e)
        .unwrap()
        .1
        .clone()
        .unwrap()
}

// ---------------------------------------------------------------------------------------------
// Positions, points construits, longueurs développées
// ---------------------------------------------------------------------------------------------

#[test]
fn les_points_libres_et_construits_suivent_la_taille() {
    let mut f = rect();
    // E : milieu de B–C ; F : décalé de (50 ; −30) depuis D ; G : sur le segment C–D à 100 mm de C.
    let e =
        f.p.apply(Command::AddPointAt {
            sheet: f.sheet,
            point: PointRef::Derived(PointDefinition::Midpoint {
                first: f.b,
                second: f.c,
            }),
        })
        .unwrap()
        .points[0];
    let fpt =
        f.p.apply(Command::AddPointAt {
            sheet: f.sheet,
            point: PointRef::Derived(PointDefinition::Offset {
                from: f.d,
                dx_mm: 50.0,
                dy_mm: -30.0,
            }),
        })
        .unwrap()
        .points[0];
    let g =
        f.p.apply(Command::AddPointAt {
            sheet: f.sheet,
            point: PointRef::Derived(PointDefinition::OnSegment {
                segment: f.cd,
                distance_mm: 100.0,
                origin: SegmentEnd::Start,
            }),
        })
        .unwrap()
        .points[0];
    for label in SIZES {
        let (dx, dy) = delta(label);
        let d = derived(&f.p, f.sheet, label);
        close(pos(&d, f.c), pt(400.0 + dx, 600.0 + dy), label);
        close(pos(&d, e), pt(400.0 + dx, (600.0 + dy) / 2.0), label);
        close(pos(&d, fpt), pt(50.0, 570.0 + dy), label);
        // C–D va vers la gauche : à 100 mm de C, x = 400 + dx − 100.
        close(pos(&d, g), pt(300.0 + dx, 600.0 + dy), label);
    }
}

#[test]
fn la_longueur_developpee_de_chaque_trace_suit_la_taille() {
    let f = rect();
    for label in SIZES {
        let (dx, dy) = delta(label);
        let d = derived(&f.p, f.sheet, label);
        num(len_of(&d, f.ab), 400.0 + dx, label);
        num(len_of(&d, f.bc), 600.0 + dy, label);
        num(len_of(&d, f.cd), 400.0 + dx, label);
        num(len_of(&d, f.da), 600.0 + dy, label);
    }
}

/// Aire et périmètre du contour de couture, fermeture : valeurs de l'en-tête.
#[test]
fn surface_perimetre_et_piece_fermee_dans_chaque_taille() {
    let f = rect();
    let want = [
        ("XS", 1942.0, 226_780.0),
        ("S", 1970.0, 233_050.0),
        ("M", 2000.0, 240_000.0),
        ("L", 2030.0, 247_050.0),
        ("XL", 2064.0, 255_020.0),
    ];
    for (label, perimeter, area) in want {
        let d = derived(&f.p, f.sheet, label);
        let c = piece_of(&d, f.piece).contour.clone().unwrap();
        num(c.perimeter_mm, perimeter, label);
        num(c.area_mm2, area, label);
        assert!(c.closed && !c.self_intersecting && !c.clockwise, "{label}");
        // Contour fermé de la feuille (hors pièce) : même mesure.
        assert_eq!(d.contours.len(), 1, "{label}");
        let m = d.contours[0].measure.clone().unwrap();
        num(m.perimeter_mm, perimeter, label);
        num(m.area_mm2, area, label);
        // « Pièce fermée » : aucun avertissement d'ouverture, quelle que soit la taille.
        assert!(
            !piece_of(&d, f.piece)
                .issues
                .iter()
                .any(|i| matches!(i, PieceIssue::OutlineOpen | PieceIssue::EmptyOutline)),
            "{label}"
        );
    }
}

#[test]
fn un_cercle_garde_son_rayon_et_son_centre_suit() {
    let (mut p, sheet) = with_sizes();
    let out = p
        .apply(Command::AddCircle {
            sheet,
            center: PointRef::New(pt(200.0, 300.0)),
            radius_mm: 25.0,
        })
        .unwrap();
    let (circle, center) = (out.entities[0], point_at(&p, sheet, pt(200.0, 300.0)));
    for (label, dx, dy) in DELTAS {
        grade(&mut p, sheet, center, label, dx, dy);
    }
    for label in SIZES {
        let (dx, dy) = delta(label);
        let d = derived(&p, sheet, label);
        close(pos(&d, center), pt(200.0 + dx, 300.0 + dy), label);
        num(len_of(&d, circle), 2.0 * std::f64::consts::PI * 25.0, label);
        let m = d.contours[0].measure.clone().unwrap();
        num(m.area_mm2, std::f64::consts::PI * 625.0, label);
    }
}

// ---------------------------------------------------------------------------------------------
// Courbes : Bézier (poignées relatives) et courbe lisse
// ---------------------------------------------------------------------------------------------

/// Longueur d'une Bézier par la règle de Simpson sur 20 000 intervalles : oracle écrit ici.
fn bezier_length(p: [PointMm; 4]) -> f64 {
    let speed = |t: f64| {
        let u = 1.0 - t;
        let dx = 3.0 * u * u * (p[1].x - p[0].x)
            + 6.0 * u * t * (p[2].x - p[1].x)
            + 3.0 * t * t * (p[3].x - p[2].x);
        let dy = 3.0 * u * u * (p[1].y - p[0].y)
            + 6.0 * u * t * (p[2].y - p[1].y)
            + 3.0 * t * t * (p[3].y - p[2].y);
        dx.hypot(dy)
    };
    let n = 20_000;
    let h = 1.0 / n as f64;
    let mut s = speed(0.0) + speed(1.0);
    for i in 1..n {
        s += speed(i as f64 * h) * if i % 2 == 1 { 4.0 } else { 2.0 };
    }
    s * h / 3.0
}

fn curve_project() -> (Project, SheetId, EntityId, PointId, PointId) {
    let (mut p, sheet) = with_sizes();
    let e = p
        .apply(Command::AddCurve {
            sheet,
            start: PointRef::New(pt(0.0, 0.0)),
            c1: pt(100.0, 150.0),
            c2: pt(200.0, 150.0),
            end: PointRef::New(pt(300.0, 0.0)),
        })
        .unwrap()
        .entities[0];
    let (s, t) = (
        point_at(&p, sheet, pt(0.0, 0.0)),
        point_at(&p, sheet, pt(300.0, 0.0)),
    );
    (p, sheet, e, s, t)
}

#[test]
fn les_poignees_de_bezier_suivent_leur_extremite_et_la_longueur_est_recalculee() {
    let (mut p, sheet, e, s, t) = curve_project();
    for (label, dx, dy) in DELTAS {
        grade(&mut p, sheet, t, label, dx, dy);
    }
    let _ = s;
    for label in SIZES {
        let (dx, dy) = delta(label);
        let d = derived(&p, sheet, label);
        let end = pt(300.0 + dx, dy);
        // Poignées écrites : (100 ; 150) depuis le départ, (−100 ; 150) depuis l'arrivée.
        let g = p
            .graded_sheet(sheet, Some(size(&p, label)))
            .unwrap()
            .curve_geometry(e)
            .unwrap();
        close(g.p0, pt(0.0, 0.0), label);
        close(g.c1, pt(100.0, 150.0), label);
        close(g.c2, pt(end.x - 100.0, end.y + 150.0), label);
        close(g.p3, end, label);
        // Longueur développée : intégrale de Simpson écrite dans ce test.
        let want = bezier_length([g.p0, g.c1, g.c2, g.p3]);
        assert!(
            (len_of(&d, e) - want).abs() < 1e-5,
            "{label} : {} ≠ {want}",
            len_of(&d, e)
        );
    }
    // La taille M est la géométrie écrite : longueur de la courbe de base, symétrique, par Simpson.
    let base = bezier_length([
        pt(0.0, 0.0),
        pt(100.0, 150.0),
        pt(200.0, 150.0),
        pt(300.0, 0.0),
    ]);
    let m = derived(&p, sheet, "M");
    assert!((len_of(&m, e) - base).abs() < 1e-5);
}

#[test]
fn une_translation_des_deux_extremites_garde_la_longueur_et_translate_les_poignees() {
    let (mut p, sheet, e, s, t) = curve_project();
    for (label, dx, dy) in DELTAS {
        grade(&mut p, sheet, s, label, dx, dy);
        grade(&mut p, sheet, t, label, dx, dy);
    }
    let m = derived(&p, sheet, "M");
    let base = len_of(&m, e);
    for label in SIZES {
        let (dx, dy) = delta(label);
        let d = derived(&p, sheet, label);
        num(len_of(&d, e), base, label);
        let g = p
            .graded_sheet(sheet, Some(size(&p, label)))
            .unwrap()
            .curve_geometry(e)
            .unwrap();
        close(g.c1, pt(100.0 + dx, 150.0 + dy), label);
        close(g.c2, pt(200.0 + dx, 150.0 + dy), label);
    }
}

#[test]
fn une_courbe_lisse_passe_par_ses_points_gradues() {
    let (mut p, sheet) = with_sizes();
    // Deux points de passage : tangentes = corde, donc la courbe est le segment [N0 N1].
    let two = p
        .apply(Command::AddSpline {
            sheet,
            points: vec![PointRef::New(pt(0.0, 0.0)), PointRef::New(pt(300.0, 0.0))],
        })
        .unwrap()
        .entities[0];
    let (n0, n1) = (
        point_at(&p, sheet, pt(0.0, 0.0)),
        point_at(&p, sheet, pt(300.0, 0.0)),
    );
    for (label, dx, dy) in DELTAS {
        grade(&mut p, sheet, n1, label, dx, dy);
    }
    // Trois points de passage : départ et arrivée de la courbe = premier et dernier points.
    let three = p
        .apply(Command::AddSpline {
            sheet,
            points: vec![
                PointRef::New(pt(0.0, 200.0)),
                PointRef::New(pt(150.0, 260.0)),
                PointRef::New(pt(300.0, 200.0)),
            ],
        })
        .unwrap()
        .entities[0];
    let last = point_at(&p, sheet, pt(300.0, 200.0));
    for (label, dx, dy) in DELTAS {
        grade(&mut p, sheet, last, label, dx, dy);
    }
    for label in SIZES {
        let (dx, dy) = delta(label);
        let d = derived(&p, sheet, label);
        let _ = n0;
        // Corde de (0 ; 0) à (300 + dx ; dy) : Pythagore.
        let chord = ((300.0 + dx) * (300.0 + dx) + dy * dy).sqrt();
        num(len_of(&d, two), chord, label);
        let g = p.graded_sheet(sheet, Some(size(&p, label))).unwrap();
        let (a, _) = g.path_point_at(three, 0.0).unwrap();
        close(a, pt(0.0, 200.0), label);
        let (b, _) = g.path_point_at(three, len_of(&d, three)).unwrap();
        close(b, pt(300.0 + dx, 200.0 + dy), label);
        // Plus longue que la corde de bout en bout, jamais plus courte.
        assert!(
            len_of(&d, three) >= ((300.0 + dx).hypot(dy)) - TOL,
            "{label}"
        );
    }
}

// ---------------------------------------------------------------------------------------------
// Contour de coupe, marges (CA : marge constante dans toutes les tailles)
// ---------------------------------------------------------------------------------------------

fn set_margin(f: &mut Fx, mm: f64) {
    f.p.apply(Command::SetSeamAllowance {
        piece: f.piece,
        seam: Some(SeamAllowance {
            default_mm: mm,
            ..Default::default()
        }),
    })
    .unwrap();
}

/// Distance d'un point à un segment : écrite ici (oracle indépendant du cœur).
fn dist_point_segment(p: PointMm, a: PointMm, b: PointMm) -> f64 {
    let (vx, vy) = (b.x - a.x, b.y - a.y);
    let l2 = vx * vx + vy * vy;
    let t = (((p.x - a.x) * vx + (p.y - a.y) * vy) / l2).clamp(0.0, 1.0);
    ((p.x - a.x - t * vx).hypot(p.y - a.y - t * vy)).abs()
}

#[test]
fn le_contour_de_coupe_est_le_decalage_exact_de_la_couture_dans_chaque_taille() {
    let mut f = rect();
    set_margin(&mut f, 10.0);
    for label in SIZES {
        let (dx, dy) = delta(label);
        let (w, h) = (400.0 + dx, 600.0 + dy);
        let d = derived(&f.p, f.sheet, label);
        let cut = piece_of(&d, f.piece).cut.clone().unwrap();
        // Quatre sommets à la main : (−10 ; −10), (w + 10 ; −10), (w + 10 ; h + 10), (−10 ; h + 10).
        let want = [
            pt(-10.0, -10.0),
            pt(w + 10.0, -10.0),
            pt(w + 10.0, h + 10.0),
            pt(-10.0, h + 10.0),
        ];
        assert_eq!(cut.traces.len(), 4, "{label}");
        let got: Vec<PointMm> = cut
            .traces
            .iter()
            .map(|t| match t {
                Trace::Line(a, _) => *a,
                other => panic!("segment attendu : {other:?}"),
            })
            .collect();
        let ok = (0..4).any(|s| (0..4).all(|i| distance_mm(got[(i + s) % 4], want[i]) < TOL));
        assert!(ok, "{label} : {got:?}");
        // Périmètre (w + 20 + h + 20) × 2 et aire (w + 20)(h + 20).
        num(cut.perimeter_mm, 2.0 * (w + h) + 80.0, label);
        num(cut.area_mm2, (w + 20.0) * (h + 20.0), label);
        // CA : distance contour de coupe ↔ couture = 10 mm dans chaque taille. Le milieu de chaque
        // bord de coupe est à 10 mm du contour de couture (recalculé à la main sur le rectangle).
        let seam = [pt(0.0, 0.0), pt(w, 0.0), pt(w, h), pt(0.0, h)];
        for t in &cut.traces {
            let Trace::Line(a, b) = t else { unreachable!() };
            let mid = pt((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
            let dist = (0..4)
                .map(|i| dist_point_segment(mid, seam[i], seam[(i + 1) % 4]))
                .fold(f64::INFINITY, f64::min);
            num(dist, 10.0, label);
        }
    }
}

#[test]
fn la_marge_n_est_jamais_graduee_mais_une_surcharge_par_bord_reste_celle_du_bord() {
    let mut f = rect();
    // 10 mm partout, 25 mm sur le bord B–C.
    f.p.apply(Command::SetSeamAllowance {
        piece: f.piece,
        seam: Some(SeamAllowance {
            default_mm: 10.0,
            edges: vec![EdgeAllowance {
                entity: f.bc,
                start_mm: 25.0,
                end_mm: 25.0,
            }],
            ..Default::default()
        }),
    })
    .unwrap();
    for label in SIZES {
        let (dx, dy) = delta(label);
        let (w, h) = (400.0 + dx, 600.0 + dy);
        let d = derived(&f.p, f.sheet, label);
        let cut = piece_of(&d, f.piece).cut.clone().unwrap();
        // Bord droit : x = w + 25 ; bas y = −10 ; gauche x = −10 ; haut y = h + 10.
        num(cut.area_mm2, (w + 35.0) * (h + 20.0), label);
        num(cut.perimeter_mm, 2.0 * (w + 35.0 + h + 20.0), label);
    }
}

#[test]
fn sans_bord_courbe_aucun_cran_de_rempli_dans_aucune_taille() {
    // Sans bord courbe, aucun cran de rempli : liste vide dans toutes les tailles.
    let mut f = rect();
    set_margin(&mut f, 10.0);
    for label in SIZES {
        let d = derived(&f.p, f.sheet, label);
        assert_eq!(piece_of(&d, f.piece).fold_notches.clone().unwrap(), vec![]);
    }
}

// ---------------------------------------------------------------------------------------------
// Crans : distance fixe depuis le départ du tracé ; ancrés en proportion ou sur un point
// ---------------------------------------------------------------------------------------------

fn notch_foot(d: &SizeDerived, n: NotchId) -> PointMm {
    let nd = d.notches.iter().find(|x| x.notch == n).expect("cran");
    let (_, marks) = nd.marks.clone().unwrap();
    marks[0].0
}

#[test]
fn les_crans_a_distance_fixe_restent_a_la_meme_distance_depuis_le_depart_du_trace() {
    let mut f = rect();
    // 200 mm depuis B sur B–C ; 100 mm depuis C sur C–D (vers la gauche) ; 150 mm depuis A sur A–B.
    let mut add = |entity, mm| {
        f.p.apply(Command::AddNotch {
            sheet: f.sheet,
            entity,
            distance_mm: mm,
            side: Side::Left,
            length_mm: 8.0,
            style: NotchStyle::Single,
        })
        .unwrap()
        .notches[0]
    };
    let (n_bc, n_cd, n_ab) = (add(f.bc, 200.0), add(f.cd, 100.0), add(f.ab, 150.0));
    for label in SIZES {
        let (dx, dy) = delta(label);
        let d = derived(&f.p, f.sheet, label);
        close(notch_foot(&d, n_bc), pt(400.0 + dx, 200.0), label);
        close(notch_foot(&d, n_cd), pt(300.0 + dx, 600.0 + dy), label);
        close(notch_foot(&d, n_ab), pt(150.0, 0.0), label);
        // Distance effective = distance écrite, la longueur du tracé change.
        for (n, want) in [(n_bc, 200.0), (n_cd, 100.0), (n_ab, 150.0)] {
            let nd = d.notches.iter().find(|x| x.notch == n).unwrap();
            num(nd.distance_mm.clone().unwrap(), want, label);
        }
    }
}

#[test]
fn un_cran_fixe_plus_loin_que_la_fin_du_trace_est_signale_et_bloque_le_changement_de_base() {
    let mut f = rect();
    // 590 mm depuis B sur B–C : B–C mesure 580 en XS, 590 en S, 600 en M, 610 en L, 622 en XL.
    let n =
        f.p.apply(Command::AddNotch {
            sheet: f.sheet,
            entity: f.bc,
            distance_mm: 590.0,
            side: Side::Left,
            length_mm: 8.0,
            style: NotchStyle::Single,
        })
        .unwrap()
        .notches[0];
    for label in SIZES {
        let (dx, dy) = delta(label);
        let d = derived(&f.p, f.sheet, label);
        let beyond = d
            .limits
            .contains(&DerivedLimit::NotchBeyondEdge { notch: n });
        assert_eq!(beyond, 590.0 > 600.0 + dy, "{label}");
        // Le cran est posé à l'extrémité du tracé : jamais hors de la taille, rien de caché.
        let foot = notch_foot(&d, n);
        close(foot, pt(400.0 + dx, 590.0_f64.min(600.0 + dy)), label);
    }
    // Changer la base vers XS rendrait la géométrie écrite invalide : refusé, sans trace.
    let (before, text) = (f.p.clone(), bytes(&f.p));
    let xs = size(&f.p, "XS");
    assert!(f.p.apply(Command::SetBaseSize { size: xs }).is_err());
    assert_eq!(f.p, before);
    assert_eq!(bytes(&f.p), text);
    // Vers S (590 mm) : accepté.
    let s = size(&f.p, "S");
    f.p.apply(Command::SetBaseSize { size: s }).unwrap();
}

#[test]
fn les_crans_ancres_en_proportion_ou_sur_un_point_suivent_la_longueur_du_trace() {
    let mut f = rect();
    let mut anchored = |entity, anchor| {
        f.p.apply(Command::AddAnchoredNotch {
            sheet: f.sheet,
            entity,
            anchor,
            side: Side::Left,
            length_mm: 8.0,
            style: NotchStyle::Single,
        })
        .unwrap()
        .notches[0]
    };
    let ratio = anchored(f.bc, NotchAnchor::Ratio { ratio: 0.25 });
    let mid = anchored(f.cd, NotchAnchor::Midpoint {});
    for label in SIZES {
        let (dx, dy) = delta(label);
        let (w, h) = (400.0 + dx, 600.0 + dy);
        let d = derived(&f.p, f.sheet, label);
        close(notch_foot(&d, ratio), pt(w, 0.25 * h), label);
        close(notch_foot(&d, mid), pt(w / 2.0, h), label);
        let nd = d.notches.iter().find(|x| x.notch == ratio).unwrap();
        num(nd.distance_mm.clone().unwrap(), 0.25 * h, label);
    }
}

#[test]
fn un_cran_sur_la_coupe_suit_la_couture_et_la_marge() {
    let mut f = rect();
    set_margin(&mut f, 10.0);
    let n =
        f.p.apply(Command::AddNotch {
            sheet: f.sheet,
            entity: f.bc,
            distance_mm: 200.0,
            side: Side::Left,
            length_mm: 5.0,
            style: NotchStyle::Single,
        })
        .unwrap()
        .notches[0];
    f.p.apply(Command::SetNotchShape {
        sheet: f.sheet,
        notch: n,
        form: NotchForm::Line,
        width_mm: None,
        on_cut: true,
    })
    .unwrap();
    for label in SIZES {
        let (dx, _) = delta(label);
        let d = derived(&f.p, f.sheet, label);
        let nd = d.notches.iter().find(|x| x.notch == n).unwrap();
        let (place, marks) = nd.marks.clone().unwrap();
        assert_eq!(place, NotchPlace::Cut, "{label}");
        // Pied sur la coupe : 10 mm à l'extérieur du bord B–C (x = w + 10), à 200 mm de B.
        close(marks[0].0, pt(400.0 + dx + 10.0, 200.0), label);
    }
}

// ---------------------------------------------------------------------------------------------
// Pinces, droit-fil
// ---------------------------------------------------------------------------------------------

#[test]
fn la_pince_suit_son_sommet_et_son_axe() {
    let mut f = rect();
    // Sommet (200 ; 300) et point d'axe (200 ; 0), gradués de (dx / 2 ; 0) : l'axe reste vertical,
    // le pied est sur A–B (y = 0, hors des écarts verticaux de A et B).
    let apex =
        f.p.apply(Command::AddPoint {
            sheet: f.sheet,
            at: pt(200.0, 300.0),
        })
        .unwrap()
        .points[0];
    let toward =
        f.p.apply(Command::AddPoint {
            sheet: f.sheet,
            at: pt(200.0, 0.0),
        })
        .unwrap()
        .points[0];
    for (label, dx, _) in DELTAS {
        grade(&mut f.p, f.sheet, apex, label, dx / 2.0, 0.0);
        grade(&mut f.p, f.sheet, toward, label, dx / 2.0, 0.0);
    }
    f.p.apply(Command::AddPieceDart {
        piece: f.piece,
        apex: PointRef::Existing(apex),
        toward: PointRef::Existing(toward),
        length_mm: 300.0,
        opening_mm: 40.0,
        fixed: Side::Left,
    })
    .unwrap();
    for label in SIZES {
        let (dx, _) = delta(label);
        let d = derived(&f.p, f.sheet, label);
        let r = &piece_of(&d, f.piece).darts[0];
        let shape = r
            .calc
            .clone()
            .unwrap_or_else(|e| panic!("{label} : {e}"))
            .shape;
        let x = 200.0 + dx / 2.0;
        close(shape.apex, pt(x, 300.0), label);
        // Axe vers le bas : « gauche » de l'axe est +x ; ouverture 40 mm centrée sur l'axe.
        close(shape.base_mid, pt(x, 0.0), label);
        close(shape.left_foot, pt(x + 20.0, 0.0), label);
        close(shape.right_foot, pt(x - 20.0, 0.0), label);
        num(shape.length_mm, 300.0, label);
        num(shape.opening_mm, 40.0, label);
        assert!(
            !piece_of(&d, f.piece)
                .issues
                .iter()
                .any(|i| matches!(i, PieceIssue::DartInvalid { .. })),
            "{label}"
        );
    }
}

#[test]
fn le_droit_fil_suit_ses_extremites() {
    let mut f = rect();
    // Droit-fil vertical de (200 ; 50) à (200 ; 550), extrémités graduées de (dx / 2 ; 0) et
    // (dx / 2 ; dy) : il reste vertical, plus long de dy.
    let g =
        f.p.apply(Command::AddSegment {
            sheet: f.sheet,
            from: PointRef::New(pt(200.0, 50.0)),
            to: PointRef::New(pt(200.0, 550.0)),
        })
        .unwrap()
        .entities[0];
    f.p.apply(Command::SetPathRole {
        sheet: f.sheet,
        entity: g,
        role: PathRole::Grainline,
    })
    .unwrap();
    let (lo, hi) = (
        point_at(&f.p, f.sheet, pt(200.0, 50.0)),
        point_at(&f.p, f.sheet, pt(200.0, 550.0)),
    );
    for (label, dx, dy) in DELTAS {
        grade(&mut f.p, f.sheet, lo, label, dx / 2.0, 0.0);
        grade(&mut f.p, f.sheet, hi, label, dx / 2.0, dy);
    }
    f.p.apply(Command::SetPieceOutline {
        piece: f.piece,
        outline: f.p.piece(f.piece).unwrap().outline.clone(),
        grainline: Some(g),
    })
    .unwrap();
    for label in SIZES {
        let (dx, dy) = delta(label);
        let d = derived(&f.p, f.sheet, label);
        let pd = piece_of(&d, f.piece);
        let line = pd.grain_lines[0];
        let x = 200.0 + dx / 2.0;
        close(line.from, pt(x, 50.0), label);
        close(line.to, pt(x, 550.0 + dy), label);
        num(line.length_mm, 500.0 + dy, label);
        num(line.direction_deg, 90.0, label);
        // Droit-fil présent, contour fermé : aucun avertissement dans aucune taille.
        assert!(pd.issues.is_empty(), "{label} : {:?}", pd.issues);
    }
}

// ---------------------------------------------------------------------------------------------
// Pièces dérivées et surpiqûres liées : recalculées d'après la géométrie graduée
// ---------------------------------------------------------------------------------------------

#[test]
fn une_piece_derivee_liee_suit_la_taille_de_sa_mere() {
    let mut f = rect();
    // Doublure : décalage extérieur de 10 mm du contour (coins en prolongement).
    let derived_piece =
        f.p.apply(Command::CreateDerivedPiece {
            mother: f.piece,
            kind: DerivedKind::Lining,
            operation: DerivedOp::Offset { distance_mm: 10.0 },
            name: None,
        })
        .unwrap()
        .pieces[0];
    for label in SIZES {
        let (dx, dy) = delta(label);
        let (w, h) = (400.0 + dx, 600.0 + dy);
        let size = Some(size(&f.p, label));
        let g = f.p.graded_project(size).unwrap();
        let c = g.piece_contour(derived_piece).unwrap();
        num(c.perimeter_mm, 2.0 * (w + h) + 80.0, label);
        num(c.area_mm2, (w + 20.0) * (h + 20.0), label);
        // Même résultat par la lecture publique par taille (feuille de la dérivée).
        let sheet = f.p.piece_sheet(derived_piece).unwrap().id;
        let d = f.p.derived_for_size(sheet, size).unwrap();
        let pc = piece_of(&d, derived_piece).contour.clone().unwrap();
        num(pc.area_mm2, (w + 20.0) * (h + 20.0), label);
        // `evaluate_sheet` lit le même projet évalué.
        let e = f.p.evaluate_sheet(sheet, size).unwrap();
        assert!(e.issues.is_empty(), "{label} : {:?}", e.issues);
    }
}

#[test]
fn une_surpiqure_liee_suit_la_taille_de_son_bord() {
    let mut f = rect();
    // Surpiqûre à 5 mm à l'intérieur du contour : rectangle (w − 10) × (h − 10).
    let out =
        f.p.apply(Command::AddTopstitch {
            sheet: f.sheet,
            source: TopstitchSource::Piece { piece: f.piece },
            distance_mm: 5.0,
            side: TopstitchSide::Inside,
        })
        .unwrap();
    let _ = out;
    let paths = f.p.sheet(f.sheet).unwrap().topstitches[0].paths.clone();
    for label in SIZES {
        let (dx, dy) = delta(label);
        let (w, h) = (400.0 + dx, 600.0 + dy);
        let d = derived(&f.p, f.sheet, label);
        let total: f64 = paths.iter().map(|e| len_of(&d, *e)).sum();
        num(total, 2.0 * (w - 10.0 + h - 10.0), label);
    }
}

// ---------------------------------------------------------------------------------------------
// Ce qui ne suit pas (déclaré) : plis, ampleurs, liens maintenus
// ---------------------------------------------------------------------------------------------

#[test]
fn les_plis_sont_declares_non_gradues() {
    let mut f = rect();
    f.p.apply(Command::AddPiecePleats {
        piece: f.piece,
        from: pt(100.0, -50.0),
        to: pt(100.0, 650.0),
        depth_mm: 20.0,
        kind: PleatKind::Flat,
        sense: Side::Left,
        count: 1,
        span_mm: 0.0,
    })
    .unwrap();
    // La pièce est reconstruite sur une feuille dédiée : graduer son coin haut-droit.
    let sheet = f.p.piece_sheet(f.piece).unwrap().id;
    let corner = {
        let s = f.p.sheet(sheet).unwrap();
        let right = s
            .points
            .iter()
            .filter_map(|q| s.position(q.id).ok().map(|p| p.x))
            .fold(f64::MIN, f64::max);
        s.points
            .iter()
            .filter(|q| s.position(q.id).is_ok_and(|p| near(p.x, right)))
            .map(|q| q.id)
            .collect::<Vec<_>>()
    };
    assert!(!corner.is_empty(), "points du bord droit élargi");
    for q in corner {
        grade(&mut f.p, sheet, q, "L", 5.0, 0.0);
    }
    let base = f.p.derived_for_size(sheet, None).unwrap();
    assert!(base.limits.is_empty());
    let l = f.p.derived_for_size(sheet, Some(size(&f.p, "L"))).unwrap();
    assert!(l
        .limits
        .contains(&DerivedLimit::AbsoluteLines { piece: f.piece }));
    // La ligne du pli est en coordonnées écrites : identique dans toutes les tailles.
    let line = &piece_of(&l, f.piece).pleats[0].pleat;
    close(line.start(), pt(100.0, -50.0), "début du pli");
    close(line.end(), pt(100.0, 650.0), "fin du pli");
}

#[test]
fn un_lien_maintenu_n_est_pas_reevalue_et_le_dit() {
    let mut f = rect();
    // Un point suivi par une construction maintenue (longueur / angle), parent B gradué.
    let out =
        f.p.apply(Command::AddSegmentPolar {
            sheet: f.sheet,
            from: PointRef::Existing(f.b),
            length_mm: 100.0,
            angle_deg: 0.0,
        })
        .unwrap();
    let seg = out.entities[0];
    f.p.apply(Command::SetConstructionKept {
        sheet: f.sheet,
        segment: seg,
        kept: true,
    })
    .unwrap();
    let l = derived(&f.p, f.sheet, "L");
    assert!(
        l.limits.iter().any(|x| matches!(
            x,
            DerivedLimit::Maintained {
                by: GradingFollower::Kept,
                ..
            }
        )),
        "{:?}",
        l.limits
    );
    // Base : rien à signaler.
    assert!(derived(&f.p, f.sheet, "M").limits.is_empty());
}

// ---------------------------------------------------------------------------------------------
// Invariance sous changement de base, lecture seule, refus
// ---------------------------------------------------------------------------------------------

/// Projet riche : marge, crans fixe et ancré, pince, droit-fil, dérivée, surpiqûre.
fn rich() -> Fx {
    let mut f = rect();
    set_margin(&mut f, 10.0);
    for (entity, mm) in [(f.bc, 200.0), (f.cd, 100.0)] {
        f.p.apply(Command::AddNotch {
            sheet: f.sheet,
            entity,
            distance_mm: mm,
            side: Side::Left,
            length_mm: 8.0,
            style: NotchStyle::Single,
        })
        .unwrap();
    }
    f.p.apply(Command::AddAnchoredNotch {
        sheet: f.sheet,
        entity: f.da,
        anchor: NotchAnchor::Ratio { ratio: 0.4 },
        side: Side::Left,
        length_mm: 6.0,
        style: NotchStyle::Double,
    })
    .unwrap();
    f
}

#[test]
fn changer_la_base_laisse_toutes_les_donnees_derivees_identiques() {
    let mut f = rich();
    let before: Vec<SizeDerived> = SIZES.iter().map(|l| derived(&f.p, f.sheet, l)).collect();
    let l = size(&f.p, "L");
    f.p.apply(Command::SetBaseSize { size: l }).unwrap();
    for (i, label) in SIZES.iter().enumerate() {
        let size = Some(size(&f.p, label));
        let after = f.p.derived_for_size(f.sheet, size).unwrap();
        // Valeurs entières en mm : les évaluations sont exactes, l'égalité est exacte.
        assert_eq!(before[i], after, "{label}");
    }
    // `None` = nouvelle base (L) : mêmes données que la taille L.
    let none = f.p.derived_for_size(f.sheet, None).unwrap();
    let mut as_l = before[3].clone();
    as_l.size = None;
    assert_eq!(none, as_l);
}

#[test]
fn la_lecture_par_taille_n_ecrit_rien_et_refuse_une_taille_inconnue() {
    let f = rich();
    let (value, text) = (f.p.clone(), bytes(&f.p));
    for label in SIZES {
        let _ = derived(&f.p, f.sheet, label);
    }
    assert_eq!(f.p, value);
    assert_eq!(bytes(&f.p), text);
    // Taille inconnue, feuille inconnue : erreurs typées.
    assert_eq!(
        f.p.derived_for_size(f.sheet, Some(SizeId(987_654))),
        Err(DerivedError::UnknownSize(SizeId(987_654)))
    );
    assert_eq!(
        f.p.derived_for_size(SheetId(987_654), None),
        Err(DerivedError::UnknownSheet(SheetId(987_654)))
    );
    // Une taille retirée n'est plus connue (une taille sans écart se retire).
    let (mut g, gs) = with_sizes();
    let xl = size(&g, "XL");
    g.apply(Command::RemoveSize { size: xl }).unwrap();
    assert_eq!(
        g.derived_for_size(gs, Some(xl)),
        Err(DerivedError::UnknownSize(xl))
    );
    // Aucune gradation sur la feuille ou projet sans taille : la base se lit.
    let plain = Project::new("sans taille");
    assert!(plain.derived_for_size(plain.sheets[0].id, None).is_ok());
}

#[test]
fn la_taille_de_base_et_none_donnent_la_meme_geometrie() {
    let f = rich();
    let m = size(&f.p, "M");
    let some = f.p.derived_for_size(f.sheet, Some(m)).unwrap();
    let none = f.p.derived_for_size(f.sheet, None).unwrap();
    let mut some_as_none = some;
    some_as_none.size = None;
    assert_eq!(none, some_as_none);
}

#[test]
fn les_points_de_derived_for_size_sont_ceux_d_evaluate_sheet() {
    let f = rich();
    for label in SIZES {
        let s = Some(size(&f.p, label));
        let d = f.p.derived_for_size(f.sheet, s).unwrap();
        let e = f.p.evaluate_sheet(f.sheet, s).unwrap();
        assert_eq!(d.points, e.points, "{label}");
        assert_eq!(d.issues, e.issues, "{label}");
    }
}

#[test]
fn les_donnees_de_base_ne_changent_pas_avec_la_gradation() {
    // La gradation n'écrit que des écarts : la géométrie de base (taille M) est celle d'un projet
    // sans écart.
    let f = rich();
    let mut bare = rich();
    bare.p.sheets[0].gradings.clear();
    let g = derived(&f.p, f.sheet, "M");
    let b = bare.p.derived_for_size(bare.sheet, None).unwrap();
    assert_eq!(g.pieces, b.pieces);
    assert_eq!(g.notches, b.notches);
    let _ = (f.a, f.piece);
}
