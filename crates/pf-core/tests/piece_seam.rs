//! US-05.1 — valeur de marge par bord. Un groupe de tests par critère d'acceptation :
//! CA1 défaut de pièce + valeur par tracé, CA2 marge progressive (début ≠ fin), CA3 contour de
//! coupe = décalage exact du contour de couture (valeurs calculées à la main), CA4 valeurs
//! nulles, CA5 recalcul automatique (jamais stocké), CA6 refus sans trace, CA7 échec de calcul =
//! contrôle, CA8 une étape annulable, CA9 cohérence (suppression, copie de feuille, séparer,
//! assembler, dérivée liée, calque verrouillé), CA10 fichier (schéma 3, octets inchangés).

use pf_core::persist;
use pf_core::piece::PieceIssue;
use pf_core::*;
use pf_geometry::{distance_mm, PointMm, Trace};

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

fn segment(p: &mut Project, sheet: SheetId, a: PointMm, b: PointMm) -> EntityId {
    p.apply(Command::AddSegment {
        sheet,
        from: PointRef::New(a),
        to: PointRef::New(b),
    })
    .unwrap()
    .entities[0]
}

/// Polygone fermé de segments (sommets donnés dans l'ordre), extrait en pièce liée.
fn poly_piece(p: &mut Project, sheet: SheetId, pts: &[PointMm], name: &str) -> PieceId {
    let n = pts.len();
    let ids = (0..n)
        .map(|i| segment(p, sheet, pts[i], pts[(i + 1) % n]))
        .collect();
    extract_paths(p, sheet, ids, name)
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

fn point_at(p: &Project, sheet: SheetId, at: PointMm) -> PointId {
    let s = p.sheet(sheet).unwrap();
    s.points
        .iter()
        .find(|q| distance_mm(s.position(q.id).unwrap(), at) < 1e-9)
        .unwrap_or_else(|| panic!("aucun point en {at:?}"))
        .id
}

/// Rectangle 100 × 60 en (0,0), pièce « Devant ».
fn rect() -> (Project, SheetId, PieceId) {
    let mut p = Project::new("Marges");
    let s = p.sheets[0].id;
    let piece = poly_piece(
        &mut p,
        s,
        &[pt(0.0, 0.0), pt(100.0, 0.0), pt(100.0, 60.0), pt(0.0, 60.0)],
        "Devant",
    );
    (p, s, piece)
}

fn seam(default_mm: f64, edges: Vec<EdgeAllowance>) -> Option<SeamAllowance> {
    Some(SeamAllowance {
        default_mm,
        edges,
        ..Default::default()
    })
}

fn set(p: &mut Project, piece: PieceId, s: Option<SeamAllowance>) -> Result<Outcome, CoreError> {
    p.apply(Command::SetSeamAllowance { piece, seam: s })
}

/// Refus : message d'erreur ; le projet est identique (valeur et octets).
fn refused(p: &mut Project, c: Command) -> String {
    let (before, text) = (p.clone(), bytes(p));
    let err = p.apply(c).expect_err("la commande aurait dû être refusée");
    assert_eq!(*p, before, "refus sans trace (valeur)");
    assert_eq!(text, bytes(p), "refus sans trace (octets)");
    err.to_string()
}

/// Sommets (début de chaque segment) du contour de coupe.
fn vertices(c: &CutContour) -> Vec<PointMm> {
    c.traces
        .iter()
        .map(|t| match t {
            Trace::Line(a, _) => *a,
            other => panic!("segment attendu, reçu {other:?}"),
        })
        .collect()
}

fn assert_cycle(got: &[PointMm], want: &[(f64, f64)], tol: f64) {
    assert_eq!(got.len(), want.len(), "nombre de sommets : {got:?}");
    let n = want.len();
    let ok = (0..n).any(|shift| {
        (0..n).all(|i| {
            let (g, w) = (got[(i + shift) % n], want[i]);
            near(g.x, w.0, tol) && near(g.y, w.1, tol)
        })
    });
    assert!(ok, "sommets {got:?} ≠ attendus {want:?}");
}

fn cut(p: &Project, piece: PieceId) -> CutContour {
    p.piece_cut_contour(piece).expect("contour de coupe")
}

// ---------------------------------------------------------------- CA1 / CA3

#[test]
fn ca1_marge_par_defaut_donne_120_par_80() {
    let (mut p, _, piece) = rect();
    set(&mut p, piece, seam(10.0, vec![])).unwrap();
    let c = cut(&p, piece);
    // Rectangle 100 × 60, marge 10 : 120 × 80, aire 9600, périmètre 400.
    assert_cycle(
        &vertices(&c),
        &[(-10.0, -10.0), (110.0, -10.0), (110.0, 70.0), (-10.0, 70.0)],
        1e-9,
    );
    assert!(near(c.area_mm2, 9600.0, 1e-6), "{}", c.area_mm2);
    assert!(near(c.perimeter_mm, 400.0, 1e-6), "{}", c.perimeter_mm);
    // La couture, elle, reste observée telle quelle : 6000 mm², 320 mm.
    let seam_contour = p.piece_contour(piece).unwrap();
    assert!(near(seam_contour.area_mm2, 6000.0, 1e-9));
    assert!(near(seam_contour.perimeter_mm, 320.0, 1e-9));
}

#[test]
fn ca1_surcharge_par_tracee_du_contour() {
    let (mut p, _, piece) = rect();
    let bottom = edge_between(&p, piece, pt(0.0, 0.0), pt(100.0, 0.0));
    let edges = vec![EdgeAllowance {
        entity: bottom,
        start_mm: 20.0,
        end_mm: 20.0,
    }];
    set(&mut p, piece, seam(10.0, edges)).unwrap();
    // Bas à 20 (y = -20), les autres à 10 : x de -10 à 110, y de -20 à 70.
    let c = cut(&p, piece);
    assert_cycle(
        &vertices(&c),
        &[(-10.0, -20.0), (110.0, -20.0), (110.0, 70.0), (-10.0, 70.0)],
        1e-9,
    );
    assert!(near(c.area_mm2, 120.0 * 90.0, 1e-6));
    assert!(near(c.perimeter_mm, 2.0 * (120.0 + 90.0), 1e-6));
}

#[test]
fn ca1_marge_differente_par_bord_coordonnees_calculees_a_la_main() {
    let (mut p, _, piece) = rect();
    let e = |a: PointMm, b: PointMm, m: f64| EdgeAllowance {
        entity: edge_between(&p, piece, a, b),
        start_mm: m,
        end_mm: m,
    };
    let edges = vec![
        e(pt(0.0, 0.0), pt(100.0, 0.0), 5.0),
        e(pt(100.0, 0.0), pt(100.0, 60.0), 10.0),
        e(pt(100.0, 60.0), pt(0.0, 60.0), 15.0),
        e(pt(0.0, 60.0), pt(0.0, 0.0), 20.0),
    ];
    set(&mut p, piece, seam(0.0, edges)).unwrap();
    // Bas y = -5, droite x = 110, haut y = 75, gauche x = -20.
    assert_cycle(
        &vertices(&cut(&p, piece)),
        &[(-20.0, -5.0), (110.0, -5.0), (110.0, 75.0), (-20.0, 75.0)],
        1e-9,
    );
}

#[test]
fn ca1_la_surcharge_est_lue_dans_le_sens_du_contour_pas_dans_celui_du_tracee() {
    // Même rectangle, mais le bas est dessiné de droite à gauche : le début reste (0,0) → (100,0)
    // dans le sens anti-horaire du contour.
    let mut p = Project::new("Sens");
    let s = p.sheets[0].id;
    let bottom = segment(&mut p, s, pt(100.0, 0.0), pt(0.0, 0.0)); // dessiné à l'envers
    let right = segment(&mut p, s, pt(100.0, 0.0), pt(100.0, 60.0));
    let top = segment(&mut p, s, pt(100.0, 60.0), pt(0.0, 60.0));
    let left = segment(&mut p, s, pt(0.0, 60.0), pt(0.0, 0.0));
    let piece = extract_paths(&mut p, s, vec![bottom, right, top, left], "Inverse");
    let edges = vec![EdgeAllowance {
        entity: bottom,
        start_mm: 0.0,
        end_mm: 20.0,
    }];
    set(&mut p, piece, seam(0.0, edges)).unwrap();
    // Bas : 0 en (0,0), 20 en (100,0) : y = -20·x/100. Côtés à 0 : x = 0 et x = 100, haut y = 60.
    // Sommets : (0,0) ; (100,-20) ; (100,60) ; (0,60).
    let c = cut(&p, piece);
    assert_cycle(
        &vertices(&c),
        &[(0.0, 0.0), (100.0, -20.0), (100.0, 60.0), (0.0, 60.0)],
        1e-9,
    );
}

// ---------------------------------------------------------------- CA2

#[test]
fn ca2_marge_progressive_sur_un_segment() {
    let (mut p, _, piece) = rect();
    let right = edge_between(&p, piece, pt(100.0, 0.0), pt(100.0, 60.0));
    let edges = vec![EdgeAllowance {
        entity: right,
        start_mm: 0.0,
        end_mm: 20.0,
    }];
    set(&mut p, piece, seam(10.0, edges)).unwrap();
    // Droite décalée : de (100, 0) à (120, 60), soit x = 100 + y / 3. Elle coupe y = -10 en
    // x = 96,666667 et y = 70 en x = 123,333333. Aire du trapèze : 120 × 80 = 9600.
    let c = cut(&p, piece);
    assert_cycle(
        &vertices(&c),
        &[
            (-10.0, -10.0),
            (96.666667, -10.0),
            (123.333333, 70.0),
            (-10.0, 70.0),
        ],
        1e-6,
    );
    assert!(near(c.area_mm2, 9600.0, 1e-4), "{}", c.area_mm2);
}

#[test]
fn ca2_progressive_inversee_fin_plus_petite_que_debut() {
    let (mut p, _, piece) = rect();
    let right = edge_between(&p, piece, pt(100.0, 0.0), pt(100.0, 60.0));
    let edges = vec![EdgeAllowance {
        entity: right,
        start_mm: 20.0,
        end_mm: 0.0,
    }];
    set(&mut p, piece, seam(10.0, edges)).unwrap();
    // De (120, 0) à (100, 60) : x = 120 − y / 3 ; y = -10 → 123,333333 ; y = 70 → 96,666667.
    assert_cycle(
        &vertices(&cut(&p, piece)),
        &[
            (-10.0, -10.0),
            (123.333333, -10.0),
            (96.666667, 70.0),
            (-10.0, 70.0),
        ],
        1e-6,
    );
}

// ---------------------------------------------------------------- CA3 : courbes

#[test]
fn ca3_cercle_rayon_plus_marge() {
    let mut p = Project::new("Cercle");
    let s = p.sheets[0].id;
    let circle = p
        .apply(Command::AddCircle {
            sheet: s,
            center: PointRef::New(pt(50.0, 50.0)),
            radius_mm: 30.0,
        })
        .unwrap()
        .entities[0];
    let piece = extract_paths(&mut p, s, vec![circle], "Rond");
    set(&mut p, piece, seam(10.0, vec![])).unwrap();
    let c = cut(&p, piece);
    // Rayon 40 : aire π·1600, périmètre 80π.
    assert!(
        near(c.area_mm2, std::f64::consts::PI * 1600.0, 1e-6),
        "{}",
        c.area_mm2
    );
    assert!(near(c.perimeter_mm, 80.0 * std::f64::consts::PI, 1e-6));
    for t in &c.traces {
        match t {
            Trace::Arc(a) => {
                assert!(near(a.radius_mm, 40.0, 1e-9));
                assert!(distance_mm(a.center, pt(50.0, 50.0)) < 1e-9);
            }
            other => panic!("arc attendu {other:?}"),
        }
    }
}

#[test]
fn ca3_arc_rayon_plus_marge_et_dome_a_bezier_echantillonne() {
    // Demi-disque : segment (-50,0)→(50,0) et arc par trois points (50,0) (0,50) (-50,0).
    let mut p = Project::new("Demi-disque");
    let s = p.sheets[0].id;
    let base = segment(&mut p, s, pt(-50.0, 0.0), pt(50.0, 0.0));
    let arc = p
        .apply(Command::AddArcThreePoints {
            sheet: s,
            start: PointRef::Existing(point_at(&p, s, pt(50.0, 0.0))),
            through: PointRef::New(pt(0.0, 50.0)),
            end: PointRef::Existing(point_at(&p, s, pt(-50.0, 0.0))),
        })
        .unwrap()
        .entities[0];
    let piece = extract_paths(&mut p, s, vec![base, arc], "Demi-disque");
    set(&mut p, piece, seam(10.0, vec![])).unwrap();
    let c = cut(&p, piece);
    let arcs: Vec<_> = c
        .traces
        .iter()
        .filter_map(|t| {
            if let Trace::Arc(a) = t {
                Some(*a)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(arcs.len(), 1);
    assert!(near(arcs[0].radius_mm, 60.0, 1e-9), "{}", arcs[0].radius_mm);
    // Aire : 120 × 10 + π·60²/2 ; périmètre : 120 + 20 + π·60.
    assert!(near(
        c.area_mm2,
        1200.0 + std::f64::consts::PI * 1800.0,
        1e-6
    ));
    assert!(near(
        c.perimeter_mm,
        140.0 + std::f64::consts::PI * 60.0,
        1e-6
    ));

    // Dôme : segment (0,0)-(100,0) et Bézier (100,0) (100,100) (0,100) (0,0), marge 5.
    let mut p = Project::new("Dôme");
    let s = p.sheets[0].id;
    let a = segment(&mut p, s, pt(0.0, 0.0), pt(100.0, 0.0));
    let curve = p
        .apply(Command::AddCurve {
            sheet: s,
            start: PointRef::Existing(point_at(&p, s, pt(100.0, 0.0))),
            c1: pt(100.0, 100.0),
            c2: pt(0.0, 100.0),
            end: PointRef::Existing(point_at(&p, s, pt(0.0, 0.0))),
        })
        .unwrap()
        .entities[0];
    let piece = extract_paths(&mut p, s, vec![a, curve], "Dôme");
    set(&mut p, piece, seam(5.0, vec![])).unwrap();
    let c = cut(&p, piece);
    // Distance indépendante : échantillonnage dense de la Bézier d'origine (formule de Bernstein).
    let bez = |t: f64| {
        let u = 1.0 - t;
        let (b0, b1, b2, b3) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
        pt(
            b0 * 100.0 + b1 * 100.0 + b2 * 0.0 + b3 * 0.0,
            b0 * 0.0 + b1 * 100.0 + b2 * 100.0 + b3 * 0.0,
        )
    };
    let dense: Vec<PointMm> = (0..=20000).map(|i| bez(f64::from(i) / 20000.0)).collect();
    let mut checked = 0;
    for t in &c.traces {
        if let Trace::Cubic(o) = t {
            for k in 1..40 {
                let q = o.point_at(f64::from(k) / 40.0);
                let d = dense
                    .iter()
                    .map(|d| distance_mm(*d, q))
                    .fold(f64::MAX, f64::min);
                assert!((d - 5.0).abs() <= 0.01, "distance {d} au lieu de 5");
                checked += 1;
            }
        }
    }
    assert!(checked >= 39);
}

#[test]
fn ca3_piece_en_l_coin_rentrant_sans_boucle() {
    let mut p = Project::new("L");
    let s = p.sheets[0].id;
    let l = [
        (0.0, 0.0),
        (100.0, 0.0),
        (100.0, 40.0),
        (40.0, 40.0),
        (40.0, 100.0),
        (0.0, 100.0),
    ];
    let pts: Vec<PointMm> = l.iter().map(|q| pt(q.0, q.1)).collect();
    let piece = poly_piece(&mut p, s, &pts, "L");
    set(&mut p, piece, seam(10.0, vec![])).unwrap();
    // Coin rentrant (40,40) : y = 50 coupe x = 50 en (50,50) ; aire 7200 + 3600 = 10800.
    let c = cut(&p, piece);
    assert_cycle(
        &vertices(&c),
        &[
            (-10.0, -10.0),
            (110.0, -10.0),
            (110.0, 50.0),
            (50.0, 50.0),
            (50.0, 110.0),
            (-10.0, 110.0),
        ],
        1e-9,
    );
    assert!(near(c.area_mm2, 10800.0, 1e-6));
    // Périmètre : 120 + 60 + 60 + 60 + 60 + 120 = 480.
    assert!(near(c.perimeter_mm, 480.0, 1e-6));
}

#[test]
fn ca3_pointe_tres_aigue_biseau_et_pointe_moderee_onglet() {
    // Sommet (0,0), demi-angle 5° : pointe 11,5 × la marge > 4 × : biseau vertical x = −m·sin 5°.
    let h = 100.0 * 5f64.to_radians().tan();
    let mut p = Project::new("Pointe");
    let s = p.sheets[0].id;
    let piece = poly_piece(
        &mut p,
        s,
        &[pt(0.0, 0.0), pt(100.0, -h), pt(100.0, h)],
        "Aiguë",
    );
    set(&mut p, piece, seam(2.0, vec![])).unwrap();
    let c = cut(&p, piece);
    let min_x = vertices(&c).iter().map(|q| q.x).fold(f64::MAX, f64::min);
    assert!(near(min_x, -2.0 * 5f64.to_radians().sin(), 1e-9), "{min_x}");
    assert!(c.traces.len() == 4, "trois côtés et un biseau");
    // Demi-angle 20° : pointe 2,92 × la marge : onglet, sommet à x = −m / sin 20°.
    let h = 100.0 * 20f64.to_radians().tan();
    let mut p = Project::new("Pointe");
    let s = p.sheets[0].id;
    let piece = poly_piece(
        &mut p,
        s,
        &[pt(0.0, 0.0), pt(100.0, -h), pt(100.0, h)],
        "Moyenne",
    );
    set(&mut p, piece, seam(3.0, vec![])).unwrap();
    let min_x = vertices(&cut(&p, piece))
        .iter()
        .map(|q| q.x)
        .fold(f64::MAX, f64::min);
    assert!(
        near(min_x, -3.0 / 20f64.to_radians().sin(), 1e-9),
        "{min_x}"
    );
}

// ---------------------------------------------------------------- CA4

#[test]
fn ca4_valeur_nulle_partout_contour_de_coupe_copie_du_contour_de_couture() {
    let (mut p, _, piece) = rect();
    // Une marge vide ne stocke rien et ne crée aucune étape.
    let before = bytes(&p);
    set(&mut p, piece, seam(0.0, vec![])).unwrap();
    assert!(p.piece(piece).unwrap().seam.is_none());
    assert_eq!(bytes(&p), before);
    let c = cut(&p, piece);
    assert_cycle(
        &vertices(&c),
        &[(0.0, 0.0), (100.0, 0.0), (100.0, 60.0), (0.0, 60.0)],
        0.0,
    );
    assert!(near(c.area_mm2, 6000.0, 1e-9) && near(c.perimeter_mm, 320.0, 1e-9));
    // Surcharges toutes à zéro avec défaut 0 : stockées (l'utilisateur les a écrites), copie exacte.
    let bottom = edge_between(&p, piece, pt(0.0, 0.0), pt(100.0, 0.0));
    let edges = vec![EdgeAllowance {
        entity: bottom,
        start_mm: 0.0,
        end_mm: 0.0,
    }];
    set(&mut p, piece, seam(0.0, edges)).unwrap();
    assert_eq!(cut(&p, piece).traces, c.traces);
}

#[test]
fn ca4_marge_nulle_sur_un_bord_il_reste_sur_la_couture() {
    let (mut p, _, piece) = rect();
    let bottom = edge_between(&p, piece, pt(0.0, 0.0), pt(100.0, 0.0));
    let edges = vec![EdgeAllowance {
        entity: bottom,
        start_mm: 0.0,
        end_mm: 0.0,
    }];
    set(&mut p, piece, seam(10.0, edges)).unwrap();
    // Le bas reste en y = 0 ; les côtés le prolongent : (-10, 0) et (110, 0). Aire 120 × 70.
    let c = cut(&p, piece);
    assert_cycle(
        &vertices(&c),
        &[(-10.0, 0.0), (110.0, 0.0), (110.0, 70.0), (-10.0, 70.0)],
        1e-9,
    );
    assert!(near(c.area_mm2, 8400.0, 1e-6));
}

// ---------------------------------------------------------------- CA5

#[test]
fn ca5_deplacer_un_point_change_le_contour_de_coupe_sans_autre_commande() {
    let (mut p, s, piece) = rect();
    set(&mut p, piece, seam(10.0, vec![])).unwrap();
    assert!(near(cut(&p, piece).area_mm2, 9600.0, 1e-6));
    let seam_before = p.piece(piece).unwrap().seam.clone();
    let corner = point_at(&p, s, pt(100.0, 60.0));
    p.apply(Command::MovePoint {
        sheet: s,
        point: corner,
        to: pt(120.0, 60.0),
    })
    .unwrap();
    // Seul le déplacement a été commandé : la marge stockée n'a pas bougé, le contour de coupe
    // suit. Côté droit (100,0)→(120,60), marge 10 : x = 109,486833 + (y + 3,162278) / 3 ;
    // y = -10 → 107,207592 ; y = 70 → 133,874259 ; aire 10443,274043 ; périmètre 425,409255.
    assert_eq!(p.piece(piece).unwrap().seam, seam_before);
    let c = cut(&p, piece);
    assert_cycle(
        &vertices(&c),
        &[
            (-10.0, -10.0),
            (107.207592, -10.0),
            (133.874259, 70.0),
            (-10.0, 70.0),
        ],
        1e-6,
    );
    assert!(near(c.area_mm2, 10443.274043, 1e-4), "{}", c.area_mm2);
    assert!(near(c.perimeter_mm, 425.409255, 1e-4), "{}", c.perimeter_mm);
    // Jamais stocké : le fichier ne contient aucune géométrie de coupe.
    let text = bytes(&p);
    assert!(text.contains("\"seam\""), "la marge est enregistrée");
    assert!(!text.contains("cut"), "aucun contour de coupe enregistré");
}

// ---------------------------------------------------------------- CA6

#[test]
fn ca6_refus_sans_trace() {
    let (mut p, s, piece) = rect();
    let bottom = edge_between(&p, piece, pt(0.0, 0.0), pt(100.0, 0.0));
    let right = edge_between(&p, piece, pt(100.0, 0.0), pt(100.0, 60.0));
    let ov = |entity, a: f64, b: f64| EdgeAllowance {
        entity,
        start_mm: a,
        end_mm: b,
    };
    set(&mut p, piece, seam(4.0, vec![ov(bottom, 6.0, 6.0)])).unwrap();
    for (what, c) in [
        ("négatif", seam(-0.001, vec![])),
        ("NaN", seam(f64::NAN, vec![])),
        ("infini", seam(f64::INFINITY, vec![])),
        ("trop grand", seam(1000.01, vec![])),
        ("surcharge négative", seam(1.0, vec![ov(bottom, -1.0, 2.0)])),
        ("surcharge NaN", seam(1.0, vec![ov(bottom, 1.0, f64::NAN)])),
        (
            "surcharge trop grande",
            seam(1.0, vec![ov(bottom, 1.0, 1001.0)]),
        ),
        (
            "tracé hors du contour",
            seam(1.0, vec![ov(EntityId(987_654), 1.0, 1.0)]),
        ),
        (
            "doublon",
            seam(1.0, vec![ov(bottom, 1.0, 1.0), ov(bottom, 2.0, 2.0)]),
        ),
    ] {
        let msg = refused(&mut p, Command::SetSeamAllowance { piece, seam: c });
        assert!(!msg.is_empty(), "{what}");
    }
    // Pièce inconnue.
    refused(
        &mut p,
        Command::SetSeamAllowance {
            piece: PieceId(987_654),
            seam: seam(1.0, vec![]),
        },
    );
    // Tracé d'une autre pièce / feuille.
    let other = segment(&mut p, s, pt(500.0, 0.0), pt(600.0, 0.0));
    let msg = refused(
        &mut p,
        Command::SetSeamAllowance {
            piece,
            seam: seam(1.0, vec![ov(other, 1.0, 1.0)]),
        },
    );
    assert!(msg.contains("contour"), "{msg}");
    // Les valeurs limites sont acceptées : 0 et 1000.
    set(&mut p, piece, seam(1000.0, vec![ov(right, 0.0, 1000.0)])).unwrap();
}

#[test]
fn ca6_contour_ouvert_refuse() {
    let mut p = Project::new("Ouvert");
    let s = p.sheets[0].id;
    let a = segment(&mut p, s, pt(0.0, 0.0), pt(100.0, 0.0));
    let b = segment(&mut p, s, pt(100.0, 0.0), pt(100.0, 60.0));
    let piece = p
        .apply(Command::AddPiece {
            sheet: s,
            name: "Ouverte".into(),
            quantity: 1,
            material: None,
            mirrored_pair: false,
        })
        .unwrap()
        .pieces[0];
    p.apply(Command::SetPieceOutline {
        piece,
        outline: vec![
            PieceEdge {
                entity: a,
                reversed: false,
            },
            PieceEdge {
                entity: b,
                reversed: false,
            },
        ],
        grainline: None,
    })
    .unwrap();
    let msg = refused(
        &mut p,
        Command::SetSeamAllowance {
            piece,
            seam: seam(5.0, vec![]),
        },
    );
    assert!(msg.contains("fermé"), "{msg}");
    // Retirer la marge d'une pièce qui n'en a pas : sans effet, accepté.
    set(&mut p, piece, None).unwrap();
}

#[test]
fn ca6_marge_progressive_refusee_sur_une_courbe() {
    let mut p = Project::new("Courbe");
    let s = p.sheets[0].id;
    let a = segment(&mut p, s, pt(0.0, 0.0), pt(100.0, 0.0));
    let curve = p
        .apply(Command::AddCurve {
            sheet: s,
            start: PointRef::Existing(point_at(&p, s, pt(100.0, 0.0))),
            c1: pt(100.0, 100.0),
            c2: pt(0.0, 100.0),
            end: PointRef::Existing(point_at(&p, s, pt(0.0, 0.0))),
        })
        .unwrap()
        .entities[0];
    let piece = extract_paths(&mut p, s, vec![a, curve], "Dôme");
    let msg = refused(
        &mut p,
        Command::SetSeamAllowance {
            piece,
            seam: seam(
                1.0,
                vec![EdgeAllowance {
                    entity: curve,
                    start_mm: 1.0,
                    end_mm: 2.0,
                }],
            ),
        },
    );
    assert!(
        msg.contains("marge progressive : segments seulement"),
        "{msg}"
    );
    // Constante sur la courbe : acceptée.
    set(
        &mut p,
        piece,
        seam(
            1.0,
            vec![EdgeAllowance {
                entity: curve,
                start_mm: 3.0,
                end_mm: 3.0,
            }],
        ),
    )
    .unwrap();
}

#[test]
fn ca6_piece_derivee_liee_refusee_cassez_le_lien() {
    let (mut p, _, mother) = rect();
    let d = p
        .apply(Command::CreateDerivedPiece {
            mother,
            kind: DerivedKind::Lining,
            operation: DerivedOp::Offset { distance_mm: -5.0 },
            name: None,
        })
        .unwrap()
        .pieces[0];
    let msg = refused(
        &mut p,
        Command::SetSeamAllowance {
            piece: d,
            seam: seam(5.0, vec![]),
        },
    );
    assert!(msg.contains("cassez le lien"), "{msg}");
    // La lecture reste possible : copie exacte de son contour.
    assert!(p.piece_cut_contour(d).is_ok());
    // Lien cassé : la marge est permise.
    p.apply(Command::BreakDerivedLink { piece: d }).unwrap();
    set(&mut p, d, seam(5.0, vec![])).unwrap();
}

#[test]
fn ca6_calque_verrouille_refuse() {
    let (mut p, s, piece) = rect();
    let layer = p
        .apply(Command::AddLayer {
            sheet: s,
            name: "Verrou".into(),
        })
        .unwrap()
        .layers[0];
    let bottom = edge_between(&p, piece, pt(0.0, 0.0), pt(100.0, 0.0));
    p.apply(Command::SetObjectsLayer {
        sheet: s,
        points: vec![],
        entities: vec![bottom],
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
        Command::SetSeamAllowance {
            piece,
            seam: seam(5.0, vec![]),
        },
    );
    assert!(msg.contains("verrouillé"), "{msg}");
    p.apply(Command::SetLayerLocked {
        sheet: s,
        layer,
        locked: false,
    })
    .unwrap();
    set(&mut p, piece, seam(5.0, vec![])).unwrap();
}

// ---------------------------------------------------------------- CA7

#[test]
fn ca7_echec_de_calcul_est_un_controle_jamais_un_refus() {
    let (mut p, _, piece) = rect();
    set(&mut p, piece, seam(10.0, vec![])).unwrap();
    assert!(!p
        .piece_issues(piece)
        .iter()
        .any(|i| matches!(i, PieceIssue::CutContourFailed { .. })));
    // Le contour devient ouvert (un tracé retiré du contour) : la commande de la mère est
    // acceptée, le contour de coupe échoue, la pièce porte le contrôle.
    let outline: Vec<PieceEdge> = p
        .piece(piece)
        .unwrap()
        .outline
        .iter()
        .skip(1)
        .copied()
        .collect();
    p.apply(Command::SetPieceOutline {
        piece,
        outline,
        grainline: None,
    })
    .unwrap();
    let err = p.piece_cut_contour(piece).unwrap_err().to_string();
    assert!(err.contains("contour de coupe"), "{err}");
    let issues = p.piece_issues(piece);
    let reason = issues
        .iter()
        .find_map(|i| match i {
            PieceIssue::CutContourFailed { reason } => Some(reason.clone()),
            _ => None,
        })
        .expect("contrôle CutContourFailed");
    assert!(!reason.is_empty());
    assert_eq!(
        PieceIssue::CutContourFailed { reason }.key(),
        "cut_contour_failed"
    );
    // La marge est conservée (rien n'est perdu), la commande de la mère n'a pas été refusée.
    assert!(p.piece(piece).unwrap().seam.is_some());
    // Une marge trop grande pour un coin rentrant est aussi un contrôle : encoche étroite.
    let mut q = Project::new("Encoche");
    let sq = q.sheets[0].id;
    let pts: Vec<PointMm> = [
        (0.0, 0.0),
        (100.0, 0.0),
        (100.0, 50.0),
        (52.0, 50.0),
        (52.0, 40.0),
        (48.0, 40.0),
        (48.0, 50.0),
        (0.0, 50.0),
    ]
    .iter()
    .map(|c| pt(c.0, c.1))
    .collect();
    let notched = poly_piece(&mut q, sq, &pts, "Encochée");
    set(&mut q, notched, seam(8.0, vec![])).unwrap();
    assert!(q.piece_cut_contour(notched).is_err());
    assert!(q
        .piece_issues(notched)
        .iter()
        .any(|i| matches!(i, PieceIssue::CutContourFailed { .. })));
    // Déplacer un point qui rend la marge impossible n'est pas refusé non plus.
    let top_left = point_at(&q, sq, pt(0.0, 50.0));
    q.apply(Command::MovePoint {
        sheet: sq,
        point: top_left,
        to: pt(0.0, 55.0),
    })
    .unwrap();
}

// ---------------------------------------------------------------- CA8

#[test]
fn ca8_une_seule_annulation_et_retablissement_exact() {
    let (p, _, piece) = rect();
    let mut doc = Document::new(p);
    let before = bytes(doc.project());
    doc.execute(Command::SetSeamAllowance {
        piece,
        seam: seam(10.0, vec![]),
    })
    .unwrap();
    let after = bytes(doc.project());
    assert_ne!(before, after);
    assert!(doc.undo());
    assert_eq!(bytes(doc.project()), before);
    assert!(!doc.can_undo(), "une seule étape");
    assert!(doc.redo());
    assert_eq!(bytes(doc.project()), after);
    // Une marge identique ne crée aucune étape.
    doc.execute(Command::SetSeamAllowance {
        piece,
        seam: seam(10.0, vec![]),
    })
    .unwrap();
    assert!(doc.undo());
    assert!(!doc.can_undo());
}

// ---------------------------------------------------------------- CA9

#[test]
fn ca9_surcharge_retiree_avec_remarque_quand_le_tracee_quitte_le_contour() {
    let (mut p, _, piece) = rect();
    let bottom = edge_between(&p, piece, pt(0.0, 0.0), pt(100.0, 0.0));
    let right = edge_between(&p, piece, pt(100.0, 0.0), pt(100.0, 60.0));
    let ov = |entity, m: f64| EdgeAllowance {
        entity,
        start_mm: m,
        end_mm: m,
    };
    set(
        &mut p,
        piece,
        seam(10.0, vec![ov(bottom, 20.0), ov(right, 15.0)]),
    )
    .unwrap();
    // Le bas quitte le contour (contour remplacé) : sa surcharge est retirée, la droite reste.
    let outline: Vec<PieceEdge> = p
        .piece(piece)
        .unwrap()
        .outline
        .iter()
        .filter(|e| e.entity != bottom)
        .copied()
        .collect();
    let out = p
        .apply(Command::SetPieceOutline {
            piece,
            outline,
            grainline: None,
        })
        .unwrap();
    assert!(
        out.notes.iter().any(|n| n.contains("marge")),
        "{:?}",
        out.notes
    );
    let s = p.piece(piece).unwrap().seam.clone().unwrap();
    assert_eq!(s.edges, vec![ov(right, 15.0)]);
    assert_eq!(s.default_mm, 10.0);
    // Supprimer un tracé du contour vide le contour : toutes les surcharges sont retirées, le
    // défaut reste, et le contour de coupe est un contrôle.
    let (mut p, s0, piece) = rect();
    let bottom = edge_between(&p, piece, pt(0.0, 0.0), pt(100.0, 0.0));
    set(&mut p, piece, seam(10.0, vec![ov(bottom, 20.0)])).unwrap();
    let out = p
        .apply(Command::Delete {
            sheet: s0,
            points: vec![],
            entities: vec![bottom],
        })
        .unwrap();
    assert!(
        out.notes.iter().any(|n| n.contains("marge")),
        "{:?}",
        out.notes
    );
    let s = p.piece(piece).unwrap().seam.clone().unwrap();
    assert!(s.edges.is_empty() && s.default_mm == 10.0);
    assert!(p.validate().is_ok());
}

#[test]
fn ca9_copie_de_feuille_reecrit_les_identifiants() {
    let (mut p, s, piece) = rect();
    let right = edge_between(&p, piece, pt(100.0, 0.0), pt(100.0, 60.0));
    set(
        &mut p,
        piece,
        seam(
            10.0,
            vec![EdgeAllowance {
                entity: right,
                start_mm: 0.0,
                end_mm: 20.0,
            }],
        ),
    )
    .unwrap();
    let out = p.apply(Command::DuplicateSheet { sheet: s }).unwrap();
    let copy = out.pieces[0];
    let cp = p.piece(copy).unwrap();
    let cs = cp.seam.clone().expect("marge copiée");
    assert_eq!(cs.default_mm, 10.0);
    assert_eq!(cs.edges.len(), 1);
    let new_edge = cs.edges[0].entity;
    assert_ne!(new_edge, right, "identifiant réécrit");
    assert!(cp.outline.iter().any(|e| e.entity == new_edge));
    assert_eq!((cs.edges[0].start_mm, cs.edges[0].end_mm), (0.0, 20.0));
    // Le contour de coupe de la copie est celui de l'original (même géométrie).
    assert_eq!(cut(&p, copy).traces, cut(&p, piece).traces);
    assert!(p.validate().is_ok());
}

#[test]
fn ca9_separer_reporte_le_defaut_avec_remarque_pour_les_surcharges() {
    let (mut p, _, piece) = rect();
    let right = edge_between(&p, piece, pt(100.0, 0.0), pt(100.0, 60.0));
    set(
        &mut p,
        piece,
        seam(
            10.0,
            vec![EdgeAllowance {
                entity: right,
                start_mm: 5.0,
                end_mm: 5.0,
            }],
        ),
    )
    .unwrap();
    let out = p
        .apply(Command::SplitPiece {
            piece,
            a: pt(50.0, -5.0),
            b: pt(50.0, 65.0),
        })
        .unwrap();
    assert_eq!(out.pieces.len(), 2);
    for id in &out.pieces {
        let s = p.piece(*id).unwrap().seam.clone().expect("défaut reporté");
        assert_eq!(s.default_mm, 10.0);
        assert!(s.edges.is_empty());
        // Moitié 50 × 60 avec 10 partout : 70 × 80.
        assert!(near(cut(&p, *id).area_mm2, 70.0 * 80.0, 1e-6));
    }
    assert!(
        out.notes
            .iter()
            .any(|n| n.contains("marge") && n.contains("pas reprises")),
        "{:?}",
        out.notes
    );
}

#[test]
fn ca9_assembler_garde_le_defaut_de_la_premiere_pièce() {
    let mut p = Project::new("Assembler");
    let s = p.sheets[0].id;
    let a = poly_piece(
        &mut p,
        s,
        &[pt(0.0, 0.0), pt(100.0, 0.0), pt(100.0, 60.0), pt(0.0, 60.0)],
        "A",
    );
    let b = poly_piece(
        &mut p,
        s,
        &[
            pt(200.0, 0.0),
            pt(300.0, 0.0),
            pt(300.0, 60.0),
            pt(200.0, 60.0),
        ],
        "B",
    );
    set(&mut p, a, seam(10.0, vec![])).unwrap();
    set(&mut p, b, seam(7.0, vec![])).unwrap();
    let ea = edge_between(&p, a, pt(100.0, 0.0), pt(100.0, 60.0));
    let eb = edge_between(&p, b, pt(200.0, 0.0), pt(200.0, 60.0));
    let out = p
        .apply(Command::JoinPieces {
            first: a,
            first_edge: ea,
            second: b,
            second_edge: eb,
            tolerance_mm: 0.5,
        })
        .unwrap();
    let joined = out.pieces[0];
    assert_eq!(
        p.piece(joined).unwrap().seam.as_ref().unwrap().default_mm,
        10.0
    );
    assert!(
        out.notes.iter().any(|n| n.contains("différait")),
        "{:?}",
        out.notes
    );
    // 200 × 60 avec 10 partout : 220 × 80.
    assert!(near(cut(&p, joined).area_mm2, 220.0 * 80.0, 1e-6));
}

// ---------------------------------------------------------------- CA10

#[test]
fn ca10_fichier_schema_3_octets_inchanges_sans_marge_et_relecture_exacte() {
    let (mut p, _, piece) = rect();
    let without = bytes(&p);
    assert!(!without.contains("seam"), "aucune clé sans marge");
    let right = edge_between(&p, piece, pt(100.0, 0.0), pt(100.0, 60.0));
    set(
        &mut p,
        piece,
        seam(
            10.0,
            vec![EdgeAllowance {
                entity: right,
                start_mm: 0.0,
                end_mm: 20.0,
            }],
        ),
    )
    .unwrap();
    let text = bytes(&p);
    assert!(text.contains("\"seam\""));
    assert!(text.contains("\"schema_version\": 6") || text.contains("\"schema_version\": 6"));
    let back = persist::from_json(&text).unwrap();
    assert_eq!(back, p);
    assert_eq!(bytes(&back), text);
    // Retirer la marge ramène les octets d'origine.
    set(&mut p, piece, None).unwrap();
    assert_eq!(bytes(&p), without);
}

#[test]
fn ca10_chargeur_refuse_les_marges_invalides() {
    let (mut p, _, piece) = rect();
    let right = edge_between(&p, piece, pt(100.0, 0.0), pt(100.0, 60.0));
    set(
        &mut p,
        piece,
        seam(
            10.0,
            vec![EdgeAllowance {
                entity: right,
                start_mm: 1.0,
                end_mm: 2.0,
            }],
        ),
    )
    .unwrap();
    let base: serde_json::Value = serde_json::from_str(&bytes(&p)).unwrap();
    let load = |v: &serde_json::Value| persist::from_json(&v.to_string());
    assert!(load(&base).is_ok());
    let id = right.0;
    let mutated = |f: &dyn Fn(&mut serde_json::Value)| {
        let mut v = base.clone();
        f(&mut v["pieces"][0]["seam"]);
        v
    };
    for (what, bad) in [
        ("clé inconnue", mutated(&|s| s["zz"] = 1.into())),
        (
            "défaut manquant",
            mutated(&|s| {
                s.as_object_mut().unwrap().remove("default_mm");
            }),
        ),
        (
            "défaut négatif",
            mutated(&|s| s["default_mm"] = (-10.0).into()),
        ),
        (
            "défaut trop grand",
            mutated(&|s| s["default_mm"] = 1001.0.into()),
        ),
        (
            "début négatif",
            mutated(&|s| s["edges"][0]["start_mm"] = (-1.0).into()),
        ),
        (
            "fin trop grande",
            mutated(&|s| s["edges"][0]["end_mm"] = 1001.0.into()),
        ),
        (
            "tracé inconnu",
            mutated(&|s| s["edges"][0]["entity"] = 999_999.into()),
        ),
        (
            "clé en trop sur une surcharge",
            mutated(&|s| s["edges"][0]["zz"] = 1.into()),
        ),
        (
            "fin manquante",
            mutated(&|s| {
                s["edges"][0].as_object_mut().unwrap().remove("end_mm");
            }),
        ),
        (
            "non canonique",
            mutated(&|s| {
                s["default_mm"] = 0.0.into();
                s.as_object_mut().unwrap().remove("edges");
            }),
        ),
        (
            "doublon",
            mutated(&|s| {
                let e = s["edges"][0].clone();
                s["edges"].as_array_mut().unwrap().push(e);
            }),
        ),
    ] {
        assert!(load(&bad).is_err(), "{what}");
    }
    assert_eq!(
        base["pieces"][0]["seam"]["edges"][0]["entity"],
        serde_json::json!(id)
    );
    // Marge progressive sur une courbe, dans un fichier : refusée par le chargeur.
    let mut q = Project::new("Courbe");
    let sq = q.sheets[0].id;
    let a = segment(&mut q, sq, pt(0.0, 0.0), pt(100.0, 0.0));
    let curve = q
        .apply(Command::AddCurve {
            sheet: sq,
            start: PointRef::Existing(point_at(&q, sq, pt(100.0, 0.0))),
            c1: pt(100.0, 100.0),
            c2: pt(0.0, 100.0),
            end: PointRef::Existing(point_at(&q, sq, pt(0.0, 0.0))),
        })
        .unwrap()
        .entities[0];
    let dome = extract_paths(&mut q, sq, vec![a, curve], "Dôme");
    set(
        &mut q,
        dome,
        seam(
            1.0,
            vec![EdgeAllowance {
                entity: curve,
                start_mm: 2.0,
                end_mm: 2.0,
            }],
        ),
    )
    .unwrap();
    let mut v: serde_json::Value = serde_json::from_str(&bytes(&q)).unwrap();
    assert!(load(&v).is_ok());
    v["pieces"][0]["seam"]["edges"][0]["end_mm"] = 3.0.into();
    assert!(load(&v).is_err(), "progressive sur une courbe");
}
