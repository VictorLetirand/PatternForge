//! US-05.3 — ourlets et rempliés. Un groupe de tests par critère d'acceptation :
//! CA1 ourlet à forme symétrisée (coordonnées calculées à la main), CA2 ourlet sur rectangle et
//! hauteur nulle, CA3 sommet non saillant (pas de pli) et type de coin choisi qui l'emporte,
//! CA4 rempli (marge de coupe = valeur, bord droit ou courbe), CA5 crans de rempli calculés sur
//! un bord courbe (pas, nombre, défaut, sens du tracé, cercle), CA6 contrôles (`PieceIssue`),
//! CA7 refus sans trace, CA8 une étape annulable et no-op, CA9 cohérence (suppression, copie de
//! feuille, séparer), CA10 fichier (schéma 3, octets inchangés, chargeur strict).

use pf_core::persist;
use pf_core::piece::PieceIssue;
use pf_core::*;
use pf_geometry::{PointMm, Trace};

const PI: f64 = std::f64::consts::PI;

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

fn point_at(p: &Project, sheet: SheetId, at: PointMm) -> PointId {
    let s = p.sheet(sheet).unwrap();
    s.points
        .iter()
        .find(|q| pf_geometry::distance_mm(s.position(q.id).unwrap(), at) < 1e-9)
        .unwrap_or_else(|| panic!("aucun point en {at:?}"))
        .id
}

/// Polygone fermé (sommets anti-horaires) : renvoie la pièce et ses tracés (`edges[i]` va de
/// `pts[i]` à `pts[i + 1]`).
fn poly(pts: &[(f64, f64)]) -> (Project, PieceId, Vec<EntityId>) {
    let mut p = Project::new("Ourlets");
    let s = p.sheets[0].id;
    let n = pts.len();
    let edges: Vec<EntityId> = (0..n)
        .map(|i| {
            segment(
                &mut p,
                s,
                pt(pts[i].0, pts[i].1),
                pt(pts[(i + 1) % n].0, pts[(i + 1) % n].1),
            )
        })
        .collect();
    let piece = extract_paths(&mut p, s, edges.clone(), "Pièce");
    (p, piece, edges)
}

fn rect() -> (Project, PieceId, Vec<EntityId>) {
    poly(&[(0.0, 0.0), (100.0, 0.0), (100.0, 60.0), (0.0, 60.0)])
}

fn trapeze() -> (Project, PieceId, Vec<EntityId>) {
    poly(&[(0.0, 0.0), (100.0, 0.0), (80.0, 60.0), (20.0, 60.0)])
}

/// Quartier de disque de rayon `r` : bas (0,0)→(r,0), arc de centre (0,0) de 0° à 90°, gauche
/// (0,r)→(0,0). `cw` dessine l'arc dans le sens horaire (de (0,r) à (r,0)).
fn quarter(r: f64, cw: bool) -> (Project, PieceId, EntityId, EntityId) {
    let mut p = Project::new("Quartier");
    let s = p.sheets[0].id;
    let bottom = segment(&mut p, s, pt(0.0, 0.0), pt(r, 0.0));
    let left = segment(&mut p, s, pt(0.0, r), pt(0.0, 0.0));
    let mid = pt(r * 0.5f64.sqrt(), r * 0.5f64.sqrt());
    let (a, b) = (point_at(&p, s, pt(r, 0.0)), point_at(&p, s, pt(0.0, r)));
    let (start, end) = if cw { (b, a) } else { (a, b) };
    let arc = p
        .apply(Command::AddArcThreePoints {
            sheet: s,
            start: PointRef::Existing(start),
            through: PointRef::New(mid),
            end: PointRef::Existing(end),
        })
        .unwrap()
        .entities[0];
    let piece = extract_paths(&mut p, s, vec![bottom, arc, left], "Quartier");
    (p, piece, arc, left)
}

fn hem(entity: EntityId, height_mm: f64) -> HemAllowance {
    HemAllowance { entity, height_mm }
}

fn fold(entity: EntityId, value_mm: f64) -> FoldAllowance {
    FoldAllowance {
        entity,
        value_mm,
        notch_step_mm: None,
        notch_count: None,
    }
}

fn seam(default_mm: f64, hems: Vec<HemAllowance>, folds: Vec<FoldAllowance>) -> SeamAllowance {
    SeamAllowance {
        default_mm,
        hems,
        folds,
        ..Default::default()
    }
}

fn set(p: &mut Project, piece: PieceId, s: SeamAllowance) -> Result<Outcome, CoreError> {
    p.apply(Command::SetSeamAllowance {
        piece,
        seam: Some(s),
    })
}

/// Refus : message d'erreur ; le projet est identique (valeur et octets).
fn refused(p: &mut Project, piece: PieceId, s: SeamAllowance) -> String {
    let (before, text) = (p.clone(), bytes(p));
    let err = p
        .apply(Command::SetSeamAllowance {
            piece,
            seam: Some(s),
        })
        .expect_err("la commande aurait dû être refusée");
    assert_eq!(*p, before, "refus sans trace (valeur)");
    assert_eq!(text, bytes(p), "refus sans trace (octets)");
    err.to_string()
}

fn cut(p: &Project, piece: PieceId) -> CutContour {
    p.piece_cut_contour(piece).expect("contour de coupe")
}

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

/// Aire (formule du lacet) et périmètre d'un polygone donné par ses sommets.
fn area_perimeter(v: &[(f64, f64)]) -> (f64, f64) {
    let n = v.len();
    let (mut a, mut p) = (0.0, 0.0);
    for i in 0..n {
        let (s, t) = (v[i], v[(i + 1) % n]);
        a += s.0 * t.1 - t.0 * s.1;
        p += ((t.0 - s.0).powi(2) + (t.1 - s.1).powi(2)).sqrt();
    }
    (a.abs() / 2.0, p)
}

/// Contrôles propres aux marges (les autres, comme l'absence de droit-fil, sont hors sujet).
fn issues(p: &Project, piece: PieceId) -> Vec<PieceIssue> {
    p.piece_issues(piece)
        .into_iter()
        .filter(|i| {
            matches!(
                i,
                PieceIssue::CutContourFailed { .. } | PieceIssue::FoldNotchesFailed { .. }
            )
        })
        .collect()
}

// ---------------------------------------------------------------- CA1

#[test]
fn ca1_ourlet_sur_un_trapeze_forme_symetrisee() {
    // Trapèze A(0,0) B(100,0) C(80,60) D(20,60), marge 10, ourlet de 20 sur AB. Le bord DA a pour
    // droite x = y/3 ; sa marge (10 vers l'extérieur) x = y/3 − s avec s = 10·√10/3 (décalage
    // horizontal = 10 / cos, cos = 3/√10). Elle coupe le pli (y = 0) en (−s ; 0) puis son miroir
    // x = −y/3 − s descend jusqu'à la marge d'ourlet y = −20 : x = 20/3 − s. Symétrique en B.
    let (mut p, piece, edges) = trapeze();
    set(&mut p, piece, seam(10.0, vec![hem(edges[0], 20.0)], vec![])).unwrap();
    let s = 10.0 * 10f64.sqrt() / 3.0;
    let want = [
        (20.0 / 3.0 - s, -20.0),
        (100.0 - 20.0 / 3.0 + s, -20.0),
        (100.0 + s, 0.0),
        (100.0 - 70.0 / 3.0 + s, 70.0),
        (70.0 / 3.0 - s, 70.0),
        (-s, 0.0),
    ];
    let c = cut(&p, piece);
    assert_cycle(&vertices(&c), &want, 1e-9);
    let (area, perimeter) = area_perimeter(&want);
    assert!(near(c.area_mm2, area, 1e-6), "{} ≠ {area}", c.area_mm2);
    assert!(near(c.perimeter_mm, perimeter, 1e-6));
    // La couture, elle, reste observée telle quelle : (100 + 60) / 2 × 60 = 4800 mm².
    assert!(near(p.piece_contour(piece).unwrap().area_mm2, 4800.0, 1e-9));
    assert!(issues(&p, piece).is_empty());
}

// ---------------------------------------------------------------- CA2

#[test]
fn ca2_ourlet_sur_un_rectangle_et_hauteur_nulle() {
    let (mut p, piece, edges) = rect();
    set(&mut p, piece, seam(10.0, vec![hem(edges[0], 20.0)], vec![])).unwrap();
    let c = cut(&p, piece);
    // 120 × 90 : marge 10 à gauche, à droite et en haut, 20 en bas.
    assert_cycle(
        &vertices(&c),
        &[(-10.0, -20.0), (110.0, -20.0), (110.0, 70.0), (-10.0, 70.0)],
        1e-9,
    );
    assert!(near(c.area_mm2, 10_800.0, 1e-6));
    assert!(near(c.perimeter_mm, 420.0, 1e-6));
    // Hauteur nulle : le bas reste sur la couture.
    set(&mut p, piece, seam(10.0, vec![hem(edges[0], 0.0)], vec![])).unwrap();
    let c = cut(&p, piece);
    assert_cycle(
        &vertices(&c),
        &[(-10.0, 0.0), (110.0, 0.0), (110.0, 70.0), (-10.0, 70.0)],
        1e-9,
    );
    assert!(near(c.area_mm2, 8400.0, 1e-6));
    // Recalcul automatique (jamais stocké) : déplacer D en (0 ; 80) change le contour de coupe
    // sans autre commande : 120 × (80 + 10 + 20) = 13 200.
    set(&mut p, piece, seam(10.0, vec![hem(edges[0], 20.0)], vec![])).unwrap();
    let s = p.piece_sheet(piece).unwrap().id;
    let tl = point_at(&p, s, pt(0.0, 60.0));
    let tr = point_at(&p, s, pt(100.0, 60.0));
    for q in [tl, tr] {
        let x = if q == tl { 0.0 } else { 100.0 };
        p.apply(Command::MovePoint {
            sheet: s,
            point: q,
            to: pt(x, 80.0),
        })
        .unwrap();
    }
    assert!(near(cut(&p, piece).area_mm2, 13_200.0, 1e-6));
}

// ---------------------------------------------------------------- CA3

#[test]
fn ca3_sommet_rentrant_pas_de_pli_et_coin_choisi_prioritaire() {
    // Pièce en L : (0,0) (100,0) (100,40) (40,40) (40,100) (0,100). Ourlet de 10 sur le bord
    // (100,40)→(40,40), marge 5 ailleurs. Début (100,40) saillant : le bord précédent (x = 105)
    // monte jusqu'à la marge d'ourlet (y = 50). Fin (40,40) rentrant : pas de pli, les marges se
    // coupent en (45 ; 50).
    let (mut p, piece, edges) = poly(&[
        (0.0, 0.0),
        (100.0, 0.0),
        (100.0, 40.0),
        (40.0, 40.0),
        (40.0, 100.0),
        (0.0, 100.0),
    ]);
    set(&mut p, piece, seam(5.0, vec![hem(edges[2], 10.0)], vec![])).unwrap();
    let want = [
        (-5.0, -5.0),
        (105.0, -5.0),
        (105.0, 50.0),
        (45.0, 50.0),
        (45.0, 105.0),
        (-5.0, 105.0),
    ];
    let c = cut(&p, piece);
    assert_cycle(&vertices(&c), &want, 1e-9);
    let (area, perimeter) = area_perimeter(&want);
    assert!(near(c.area_mm2, area, 1e-6) && near(c.perimeter_mm, perimeter, 1e-6));

    // Un type de coin choisi à la main au sommet A du trapèze l'emporte sur le pli de l'ourlet :
    // « en marche » avec a = 10 (marge précédente) et b = 20 (ourlet) donne E → M → S avec
    // E = A + 10·n (n = (−3 ; 1)/√10), S = (0 ; −20), M = E + (S − A) = (E.x ; E.y − 20).
    let (mut p, piece, edges) = trapeze();
    let mut s = seam(10.0, vec![hem(edges[0], 20.0)], vec![]);
    s.corners = vec![CornerAllowance {
        entity: edges[0],
        corner: CornerType::Stepped {},
    }];
    set(&mut p, piece, s).unwrap();
    let e = (-30.0 / 10f64.sqrt(), 10.0 / 10f64.sqrt());
    let c = cut(&p, piece);
    let v = vertices(&c);
    let has = |x: f64, y: f64| v.iter().any(|q| near(q.x, x, 1e-9) && near(q.y, y, 1e-9));
    assert!(has(e.0, e.1 - 20.0), "marche attendue en M : {v:?}");
    assert!(has(0.0, -20.0), "{v:?}");
    let fold_point = 20.0 / 3.0 - 10.0 * 10f64.sqrt() / 3.0;
    assert!(!has(fold_point, -20.0), "le pli ne doit plus s'appliquer");
}

#[test]
fn ca3_ourlet_voisin_d_une_courbe_repli_sur_la_marge_simple() {
    // Quartier de disque (rayon 100), marges nulles, ourlet de 20 sur le bas. Au début (0 ; 0) le
    // bord précédent est un segment : le pli s'applique, le bord gauche descend jusqu'à y = −20.
    // À la fin (100 ; 0) le bord suivant est un arc : pas de pli (le miroir d'une courbe n'est pas
    // un arc), le bas (y = −20) remonte à (100 ; 0) puis l'arc continue. Aire : 100 × 20 + π·100² / 4,
    // périmètre : 100 + 20 + 50π + 120.
    let (mut p, piece, arc, left) = quarter(100.0, false);
    let bottom = p
        .piece(piece)
        .unwrap()
        .outline
        .iter()
        .map(|o| o.entity)
        .find(|e| *e != arc && *e != left)
        .unwrap();
    set(&mut p, piece, seam(0.0, vec![hem(bottom, 20.0)], vec![])).unwrap();
    let c = cut(&p, piece);
    assert!(
        near(c.area_mm2, 2000.0 + PI * 2500.0, 1e-6),
        "{}",
        c.area_mm2
    );
    assert!(
        near(c.perimeter_mm, 240.0 + 50.0 * PI, 1e-6),
        "{}",
        c.perimeter_mm
    );
    let v: Vec<PointMm> = c
        .traces
        .iter()
        .map(|t| match t {
            Trace::Line(a, _) => *a,
            Trace::Arc(a) => a.start_point(),
            other => panic!("{other:?}"),
        })
        .collect();
    let has = |x: f64, y: f64| v.iter().any(|q| near(q.x, x, 1e-9) && near(q.y, y, 1e-9));
    assert!(
        has(0.0, -20.0) && has(100.0, -20.0) && has(100.0, 0.0),
        "{v:?}"
    );
    assert!(issues(&p, piece).is_empty());
}

// ---------------------------------------------------------------- CA4

#[test]
fn ca4_rempli_marge_de_coupe_egale_a_la_valeur() {
    // Rectangle 100 × 60, marge 3, rempli de 8 sur le bas : 106 × 71.
    let (mut p, piece, edges) = rect();
    set(&mut p, piece, seam(3.0, vec![], vec![fold(edges[0], 8.0)])).unwrap();
    let c = cut(&p, piece);
    assert_cycle(
        &vertices(&c),
        &[(-3.0, -8.0), (103.0, -8.0), (103.0, 63.0), (-3.0, 63.0)],
        1e-9,
    );
    assert!(near(c.area_mm2, 106.0 * 71.0, 1e-6));
    assert!(near(c.perimeter_mm, 2.0 * (106.0 + 71.0), 1e-6));
    // Un bord droit n'a aucun cran de rempli.
    assert!(p.piece_fold_notches(piece).unwrap().is_empty());

    // Rempli de 12 sur l'arc du quartier de disque (rayon 100), marge nulle ailleurs : l'arc
    // devient de rayon 112 ; aire π·112² / 4, périmètre 2 × 112 + π·112 / 2.
    let (mut p, piece, arc, _) = quarter(100.0, false);
    set(&mut p, piece, seam(0.0, vec![], vec![fold(arc, 12.0)])).unwrap();
    let c = cut(&p, piece);
    assert!(
        near(c.area_mm2, PI * 112.0 * 112.0 / 4.0, 1e-6),
        "{}",
        c.area_mm2
    );
    assert!(
        near(c.perimeter_mm, 224.0 + PI * 112.0 / 2.0, 1e-6),
        "{}",
        c.perimeter_mm
    );
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
    assert!(near(arcs[0].radius_mm, 112.0, 1e-9));
}

// ---------------------------------------------------------------- CA5

fn angle_deg(n: &FoldNotch) -> f64 {
    n.point.y.atan2(n.point.x).to_degrees()
}

fn check_notches(notches: &[FoldNotch], radius: f64, angles: &[f64]) {
    assert_eq!(notches.len(), angles.len(), "{notches:?}");
    let length = radius * PI / 2.0;
    for (i, (n, a)) in notches.iter().zip(angles).enumerate() {
        assert_eq!(n.index, i + 1);
        assert!(
            near(angle_deg(n), *a, 1e-6),
            "cran {i} : {} ≠ {a}",
            angle_deg(n)
        );
        assert!(near(n.point.x, radius * a.to_radians().cos(), 1e-6));
        assert!(near(n.point.y, radius * a.to_radians().sin(), 1e-6));
        assert!(near(n.distance_mm, length * a / 90.0, 1e-6));
        // Normale extérieure = direction radiale.
        assert!(near(n.normal.x, a.to_radians().cos(), 1e-6));
        assert!(near(n.normal.y, a.to_radians().sin(), 1e-6));
    }
}

#[test]
fn ca5_crans_de_rempli_defaut_pas_et_nombre() {
    let (mut p, piece, arc, _) = quarter(100.0, false);
    // Défaut (pas de 25 mm) : L = 50π ≈ 157,08 ; ⌈L / 25⌉ = 7 intervalles, donc 6 crans aux
    // angles k × 90° / 7.
    set(&mut p, piece, seam(0.0, vec![], vec![fold(arc, 12.0)])).unwrap();
    let want: Vec<f64> = (1..7).map(|k| f64::from(k) * 90.0 / 7.0).collect();
    check_notches(&p.piece_fold_notches(piece).unwrap(), 100.0, &want);
    // Nombre : 3 crans aux quarts… 22,5° ; 45° ; 67,5°.
    let mut f = fold(arc, 12.0);
    f.notch_count = Some(3);
    set(&mut p, piece, seam(0.0, vec![], vec![f])).unwrap();
    check_notches(
        &p.piece_fold_notches(piece).unwrap(),
        100.0,
        &[22.5, 45.0, 67.5],
    );
    // Pas de 100 : ⌈1,57⌉ = 2 intervalles, un cran à 45° ; pas de 200 : un seul intervalle, aucun.
    for (step, want) in [(100.0, vec![45.0]), (200.0, vec![])] {
        let mut f = fold(arc, 12.0);
        f.notch_step_mm = Some(step);
        set(&mut p, piece, seam(0.0, vec![], vec![f])).unwrap();
        check_notches(&p.piece_fold_notches(piece).unwrap(), 100.0, &want);
    }
    // Pas exact : L = 50π, pas = L / 4 → exactement 4 intervalles, 3 crans (pas de 5e).
    let mut f = fold(arc, 12.0);
    f.notch_step_mm = Some(12.5 * PI);
    set(&mut p, piece, seam(0.0, vec![], vec![f])).unwrap();
    assert_eq!(p.piece_fold_notches(piece).unwrap().len(), 3);
    // Aucun cran lu n'est stocké : le fichier ne porte que les réglages.
    assert!(!bytes(&p).contains("distance"), "crans jamais stockés");
}

#[test]
fn ca5_arc_dessine_dans_le_sens_horaire_crans_dans_le_sens_du_contour() {
    // Même quartier, arc enregistré de (0,100) à (100,0) : le rang 1 reste le cran le plus proche
    // du début du bord dans le contour anti-horaire, c'est-à-dire de (100 ; 0).
    let (mut p, piece, arc, _) = quarter(100.0, true);
    set(&mut p, piece, seam(0.0, vec![], vec![fold(arc, 12.0)])).unwrap();
    let want: Vec<f64> = (1..7).map(|k| f64::from(k) * 90.0 / 7.0).collect();
    check_notches(&p.piece_fold_notches(piece).unwrap(), 100.0, &want);
}

#[test]
fn ca5_cercle_entier_crans_regulierement_espaces() {
    // Cercle de rayon 50 : L = 100π ≈ 314,16 ; ⌈L / 25⌉ = 13 intervalles, 12 crans espacés de
    // 2π / 13 ; contour de coupe = cercle de rayon 55.
    let mut p = Project::new("Cercle");
    let s = p.sheets[0].id;
    let circle = p
        .apply(Command::AddCircle {
            sheet: s,
            center: PointRef::New(pt(0.0, 0.0)),
            radius_mm: 50.0,
        })
        .unwrap()
        .entities[0];
    let piece = extract_paths(&mut p, s, vec![circle], "Rond");
    set(&mut p, piece, seam(0.0, vec![], vec![fold(circle, 5.0)])).unwrap();
    let n = p.piece_fold_notches(piece).unwrap();
    assert_eq!(n.len(), 12);
    for k in &n {
        assert!(near(k.point.x.hypot(k.point.y), 50.0, 1e-6));
    }
    let step = 2.0 * PI / 13.0;
    for w in n.windows(2) {
        let (a, b) = (
            w[0].point.y.atan2(w[0].point.x),
            w[1].point.y.atan2(w[1].point.x),
        );
        let d = (b - a).rem_euclid(2.0 * PI);
        assert!(near(d, step, 1e-9), "écart angulaire {d} ≠ {step}");
        assert!(near(
            w[1].distance_mm - w[0].distance_mm,
            100.0 * PI / 13.0,
            1e-6
        ));
    }
    assert!(near(cut(&p, piece).area_mm2, PI * 55.0 * 55.0, 1e-6));
}

// ---------------------------------------------------------------- CA6

#[test]
fn ca6_trop_de_crans_est_un_controle_pas_un_refus() {
    // Quartier de rayon 5000 : L ≈ 7854 mm ; un pas de 1 mm donnerait 7853 crans (> 1000).
    let (mut p, piece, arc, _) = quarter(5000.0, false);
    let mut f = fold(arc, 12.0);
    f.notch_step_mm = Some(1.0);
    set(&mut p, piece, seam(0.0, vec![], vec![f])).unwrap();
    let err = p.piece_fold_notches(piece).unwrap_err().to_string();
    assert!(err.contains("1000"), "{err}");
    let reason = issues(&p, piece)
        .into_iter()
        .find_map(|i| match i {
            PieceIssue::FoldNotchesFailed { reason } => Some(reason),
            _ => None,
        })
        .expect("contrôle FoldNotchesFailed");
    assert!(!reason.is_empty());
    assert_eq!(
        PieceIssue::FoldNotchesFailed { reason }.key(),
        "fold_notches_failed"
    );
    // Le contour de coupe, lui, reste calculable.
    assert!(p.piece_cut_contour(piece).is_ok());
    // Un pas plus grand redonne un calcul valide et supprime le contrôle.
    let mut f = fold(arc, 12.0);
    f.notch_step_mm = Some(10.0);
    set(&mut p, piece, seam(0.0, vec![], vec![f])).unwrap();
    assert_eq!(p.piece_fold_notches(piece).unwrap().len(), 785);
    assert!(issues(&p, piece).is_empty());
}

#[test]
fn ca6_ourlet_impossible_est_un_controle_la_commande_de_la_mere_reste_acceptee() {
    // Ourlet de 100 sur le bas d'un rectangle 100 × 60 : accepté, le contour de coupe est calculable
    // (le rectangle reste simple). Une encoche étroite avec un ourlet dont la marge ne tient pas
    // dans la forme donne un contrôle expliqué.
    let (mut q, piece, edges) = poly(&[
        (0.0, 0.0),
        (100.0, 0.0),
        (100.0, 50.0),
        (52.0, 50.0),
        (52.0, 40.0),
        (48.0, 40.0),
        (48.0, 50.0),
        (0.0, 50.0),
    ]);
    // Ourlet de 8 sur le fond de l'encoche (bord (52,40)→(48,40)) : sa marge de 8 recouvre toute
    // la pièce autour de l'encoche étroite.
    set(&mut q, piece, seam(8.0, vec![hem(edges[4], 8.0)], vec![])).unwrap();
    assert!(q.piece_cut_contour(piece).is_err());
    assert!(issues(&q, piece)
        .iter()
        .any(|i| matches!(i, PieceIssue::CutContourFailed { .. })));
    // Déplacer un point n'est pas refusé non plus.
    let s = q.piece_sheet(piece).unwrap().id;
    let q0 = point_at(&q, s, pt(0.0, 50.0));
    q.apply(Command::MovePoint {
        sheet: s,
        point: q0,
        to: pt(0.0, 55.0),
    })
    .unwrap();
}

// ---------------------------------------------------------------- CA7

#[test]
fn ca7_refus_sans_trace() {
    let (mut p, piece, edges) = rect();
    // Tracé de la feuille, hors du contour de la pièce.
    let sheet = p.sheets[0].id;
    let stray = segment(&mut p, sheet, pt(300.0, 0.0), pt(400.0, 0.0));
    let step_and_count = FoldAllowance {
        notch_step_mm: Some(10.0),
        notch_count: Some(3),
        ..fold(edges[1], 5.0)
    };
    let cases: Vec<(&str, SeamAllowance, &str)> = vec![
        (
            "hauteur négative",
            seam(1.0, vec![hem(edges[0], -1.0)], vec![]),
            "hauteur",
        ),
        (
            "hauteur trop grande",
            seam(1.0, vec![hem(edges[0], 1000.001)], vec![]),
            "hauteur",
        ),
        (
            "hauteur NaN",
            seam(1.0, vec![hem(edges[0], f64::NAN)], vec![]),
            "hauteur",
        ),
        (
            "hauteur infinie",
            seam(1.0, vec![hem(edges[0], f64::INFINITY)], vec![]),
            "hauteur",
        ),
        (
            "rempli négatif",
            seam(1.0, vec![], vec![fold(edges[0], -0.5)]),
            "rempli",
        ),
        (
            "rempli trop grand",
            seam(1.0, vec![], vec![fold(edges[0], 1000.5)]),
            "rempli",
        ),
        (
            "rempli NaN",
            seam(1.0, vec![], vec![fold(edges[0], f64::NAN)]),
            "rempli",
        ),
        (
            "ourlet en double",
            seam(1.0, vec![hem(edges[0], 5.0), hem(edges[0], 6.0)], vec![]),
            "un seul",
        ),
        (
            "rempli en double",
            seam(1.0, vec![], vec![fold(edges[0], 5.0), fold(edges[0], 6.0)]),
            "un seul",
        ),
        (
            "ourlet et rempli",
            seam(1.0, vec![hem(edges[0], 5.0)], vec![fold(edges[0], 6.0)]),
            "s'excluent",
        ),
        (
            "ourlet et surcharge",
            SeamAllowance {
                edges: vec![EdgeAllowance {
                    entity: edges[0],
                    start_mm: 1.0,
                    end_mm: 1.0,
                }],
                ..seam(1.0, vec![hem(edges[0], 5.0)], vec![])
            },
            "s'excluent",
        ),
        (
            "ourlet hors contour",
            seam(1.0, vec![hem(stray, 5.0)], vec![]),
            "contour",
        ),
        (
            "rempli hors contour",
            seam(1.0, vec![], vec![fold(stray, 5.0)]),
            "contour",
        ),
        (
            "ourlet inconnu",
            seam(1.0, vec![hem(EntityId(9_999_999), 5.0)], vec![]),
            "contour",
        ),
        (
            "pas et nombre",
            seam(1.0, vec![], vec![step_and_count]),
            "un seul",
        ),
        (
            "nombre nul",
            seam(
                1.0,
                vec![],
                vec![FoldAllowance {
                    notch_count: Some(0),
                    ..fold(edges[1], 5.0)
                }],
            ),
            "nombre de crans",
        ),
        (
            "trop de crans",
            seam(
                1.0,
                vec![],
                vec![FoldAllowance {
                    notch_count: Some(1001),
                    ..fold(edges[1], 5.0)
                }],
            ),
            "nombre de crans",
        ),
        (
            "pas trop petit",
            seam(
                1.0,
                vec![],
                vec![FoldAllowance {
                    notch_step_mm: Some(0.5),
                    ..fold(edges[1], 5.0)
                }],
            ),
            "pas des crans",
        ),
        (
            "pas NaN",
            seam(
                1.0,
                vec![],
                vec![FoldAllowance {
                    notch_step_mm: Some(f64::NAN),
                    ..fold(edges[1], 5.0)
                }],
            ),
            "pas des crans",
        ),
        (
            "crans sur un bord droit",
            seam(
                1.0,
                vec![],
                vec![FoldAllowance {
                    notch_count: Some(3),
                    ..fold(edges[1], 5.0)
                }],
            ),
            "bord courbe seulement",
        ),
    ];
    for (what, s, needle) in cases {
        let msg = refused(&mut p, piece, s);
        assert!(msg.contains(needle), "{what} : « {msg} »");
    }
    // Ourlet sur une courbe : refusé.
    let (mut c, piece, arc, _) = quarter(100.0, false);
    let msg = refused(&mut c, piece, seam(1.0, vec![hem(arc, 5.0)], vec![]));
    assert!(msg.contains("bord droit seulement"), "{msg}");
}

// ---------------------------------------------------------------- CA8

#[test]
fn ca8_une_etape_annulable_et_no_op() {
    let (p, piece, edges) = trapeze();
    let mut doc = Document::new(p);
    let before = bytes(doc.project());
    let s = seam(10.0, vec![hem(edges[0], 20.0)], vec![fold(edges[2], 4.0)]);
    doc.execute(Command::SetSeamAllowance {
        piece,
        seam: Some(s.clone()),
    })
    .unwrap();
    let after = bytes(doc.project());
    assert_ne!(before, after);
    assert!(doc.undo());
    assert_eq!(bytes(doc.project()), before);
    assert!(!doc.can_undo(), "une seule étape");
    assert!(doc.redo());
    assert_eq!(bytes(doc.project()), after);
    // Le même réglage (autre ordre des listes) ne crée aucune étape.
    doc.execute(Command::SetSeamAllowance {
        piece,
        seam: Some(s),
    })
    .unwrap();
    assert!(doc.undo());
    assert!(!doc.can_undo());
}

// ---------------------------------------------------------------- CA9

#[test]
fn ca9_suppression_copie_de_feuille_et_separer() {
    // Un tracé quitte le contour : son ourlet (puis son rempli) est retiré avec remarque, le
    // défaut reste.
    let (mut p, piece, edges) = rect();
    set(
        &mut p,
        piece,
        seam(10.0, vec![hem(edges[0], 20.0)], vec![fold(edges[1], 6.0)]),
    )
    .unwrap();
    let without = |p: &Project, gone: EntityId| -> Vec<PieceEdge> {
        p.piece(piece)
            .unwrap()
            .outline
            .iter()
            .filter(|e| e.entity != gone)
            .copied()
            .collect()
    };
    let out = p
        .apply(Command::SetPieceOutline {
            piece,
            outline: without(&p, edges[0]),
            grainline: None,
        })
        .unwrap();
    assert!(
        out.notes.iter().any(|n| n.contains("ourlet")),
        "{:?}",
        out.notes
    );
    let s = p.piece(piece).unwrap().seam.clone().unwrap();
    assert!(s.hems.is_empty() && s.folds == vec![fold(edges[1], 6.0)]);
    assert!(p.validate().is_ok());
    let out = p
        .apply(Command::SetPieceOutline {
            piece,
            outline: without(&p, edges[1]),
            grainline: None,
        })
        .unwrap();
    assert!(
        out.notes.iter().any(|n| n.contains("rempli")),
        "{:?}",
        out.notes
    );
    let s = p.piece(piece).unwrap().seam.clone().unwrap();
    assert!(s.folds.is_empty() && s.default_mm == 10.0);

    // Copie de feuille : les identifiants sont réécrits et le contour de coupe est identique.
    let (mut p, piece, edges) = rect();
    set(
        &mut p,
        piece,
        seam(10.0, vec![hem(edges[0], 20.0)], vec![fold(edges[2], 6.0)]),
    )
    .unwrap();
    let sheet = p.sheets[0].id;
    let out = p.apply(Command::DuplicateSheet { sheet }).unwrap();
    let copy = out.pieces[0];
    let cs = p.piece(copy).unwrap().seam.clone().expect("réglage copié");
    assert_eq!((cs.hems.len(), cs.folds.len()), (1, 1));
    assert_ne!(cs.hems[0].entity, edges[0]);
    assert_ne!(cs.folds[0].entity, edges[2]);
    assert!(p
        .piece(copy)
        .unwrap()
        .outline
        .iter()
        .any(|e| e.entity == cs.hems[0].entity));
    assert_eq!(cut(&p, copy).traces, cut(&p, piece).traces);
    assert!(p.validate().is_ok());

    // Séparer : ourlets et rempliés ne sont pas repris, la remarque le dit.
    let (mut p, piece, edges) = rect();
    set(&mut p, piece, seam(10.0, vec![hem(edges[0], 20.0)], vec![])).unwrap();
    let out = p
        .apply(Command::SplitPiece {
            piece,
            a: pt(50.0, -5.0),
            b: pt(50.0, 65.0),
        })
        .unwrap();
    for id in &out.pieces {
        let s = p.piece(*id).unwrap().seam.clone().expect("défaut reporté");
        assert!(s.hems.is_empty() && s.folds.is_empty());
    }
    assert!(
        out.notes
            .iter()
            .any(|n| n.contains("ourlet") && n.contains("pas repris")),
        "{:?}",
        out.notes
    );
}

// ---------------------------------------------------------------- CA10

#[test]
fn ca10_fichier_octets_inchanges_sans_ourlet_et_relecture_exacte() {
    let (mut p, piece, edges) = rect();
    let without = bytes(&p);
    set(&mut p, piece, seam(10.0, vec![], vec![])).unwrap();
    let plain = bytes(&p);
    assert!(
        !plain.contains("hems") && !plain.contains("folds"),
        "{plain}"
    );
    set(
        &mut p,
        piece,
        seam(10.0, vec![hem(edges[0], 20.0)], vec![fold(edges[2], 4.5)]),
    )
    .unwrap();
    let text = bytes(&p);
    assert!(text.contains("\"hems\"") && text.contains("\"height_mm\""));
    assert!(text.contains("\"folds\"") && text.contains("\"value_mm\""));
    assert!(!text.contains("notch_step_mm") && !text.contains("notch_count"));
    assert!(text.contains("\"schema_version\": 6") || text.contains("\"schema_version\": 6"));
    let back = persist::from_json(&text).unwrap();
    assert_eq!(back, p);
    assert_eq!(bytes(&back), text);
    // Retirer le réglage ramène les octets d'origine.
    p.apply(Command::SetSeamAllowance { piece, seam: None })
        .unwrap();
    assert_eq!(bytes(&p), without);
}

#[test]
fn ca10_chargeur_strict() {
    let (mut p, piece, edges) = rect();
    set(
        &mut p,
        piece,
        seam(10.0, vec![hem(edges[1], 20.0)], vec![fold(edges[2], 4.5)]),
    )
    .unwrap();
    let base: serde_json::Value = serde_json::from_str(&bytes(&p)).unwrap();
    let load = |v: &serde_json::Value| persist::from_json(&v.to_string());
    assert!(load(&base).is_ok());
    let mutated = |f: &dyn Fn(&mut serde_json::Value)| {
        let mut v = base.clone();
        f(&mut v["pieces"][0]["seam"]);
        v
    };
    let (e1, e2) = (edges[1].0, edges[2].0);
    for (what, bad) in [
        (
            "clé en trop (ourlet)",
            mutated(&|s| s["hems"][0]["zz"] = 1.into()),
        ),
        (
            "clé en trop (rempli)",
            mutated(&|s| s["folds"][0]["zz"] = 1.into()),
        ),
        (
            "hauteur manquante",
            mutated(&|s| {
                s["hems"][0].as_object_mut().unwrap().remove("height_mm");
            }),
        ),
        (
            "hauteur négative",
            mutated(&|s| s["hems"][0]["height_mm"] = (-1.0).into()),
        ),
        (
            "hauteur trop grande",
            mutated(&|s| s["hems"][0]["height_mm"] = 1000.5.into()),
        ),
        (
            "valeur trop grande",
            mutated(&|s| s["folds"][0]["value_mm"] = 1001.0.into()),
        ),
        (
            "valeur négative",
            mutated(&|s| s["folds"][0]["value_mm"] = (-2.0).into()),
        ),
        (
            "ourlet : tracé absent",
            mutated(&|s| s["hems"][0]["entity"] = 999_999.into()),
        ),
        (
            "rempli : tracé absent",
            mutated(&|s| s["folds"][0]["entity"] = 999_999.into()),
        ),
        (
            "ourlet en double",
            mutated(&|s| {
                let h = s["hems"][0].clone();
                s["hems"].as_array_mut().unwrap().push(h);
            }),
        ),
        (
            "ourlet et rempli sur le même tracé",
            mutated(&|s| s["folds"][0]["entity"] = e1.into()),
        ),
        (
            "ourlet et surcharge",
            mutated(&|s| {
                s["edges"] = serde_json::json!([{ "entity": e1, "start_mm": 1.0, "end_mm": 1.0 }]);
            }),
        ),
        (
            "pas et nombre",
            mutated(&|s| {
                s["folds"][0]["notch_step_mm"] = 10.0.into();
                s["folds"][0]["notch_count"] = 3.into();
            }),
        ),
        (
            "nombre nul",
            mutated(&|s| s["folds"][0]["notch_count"] = 0.into()),
        ),
        (
            "pas trop petit",
            mutated(&|s| s["folds"][0]["notch_step_mm"] = 0.5.into()),
        ),
        (
            "crans sur un bord droit",
            mutated(&|s| s["folds"][0]["notch_count"] = 3.into()),
        ),
        ("ourlets non liste", mutated(&|s| s["hems"] = 5.into())),
        (
            "non canonique",
            mutated(&|s| {
                s["default_mm"] = 0.0.into();
                s.as_object_mut().unwrap().remove("hems");
                s.as_object_mut().unwrap().remove("folds");
            }),
        ),
    ] {
        assert!(load(&bad).is_err(), "{what}");
    }
    assert_eq!(
        base["pieces"][0]["seam"]["hems"][0]["entity"],
        serde_json::json!(e1)
    );
    assert_eq!(
        base["pieces"][0]["seam"]["folds"][0]["entity"],
        serde_json::json!(e2)
    );
    // Ourlet sur une courbe, dans un fichier : refusé par le chargeur.
    let (mut c, piece, arc, left) = quarter(100.0, false);
    set(
        &mut c,
        piece,
        seam(1.0, vec![hem(left, 5.0)], vec![fold(arc, 5.0)]),
    )
    .unwrap();
    let mut v: serde_json::Value = serde_json::from_str(&bytes(&c)).unwrap();
    assert!(load(&v).is_ok());
    v["pieces"][0]["seam"]["hems"][0]["entity"] = arc.0.into();
    v["pieces"][0]["seam"]["folds"][0]["entity"] = left.0.into();
    assert!(load(&v).is_err(), "ourlet sur une courbe");
    // Crans sur un arc : accepté dans un fichier (pas, puis nombre).
    let mut w: serde_json::Value = serde_json::from_str(&bytes(&c)).unwrap();
    w["pieces"][0]["seam"]["folds"][0]["notch_count"] = 4.into();
    assert!(load(&w).is_ok());
}
