//! PH-5a — contour de coupe « pince fermée » : on ferme la pince, on trace la marge, on rouvre.
//! Un groupe de tests par critère d'acceptation (CA1 à CA5 ; exports CA6 dans `pf-export`, interface
//! CA7 dans `pf-desktop`, campagne dans `adverse_coupe_pince.rs`).
//!
//! **Valeurs calculées à la main**, jamais relues du code testé.
//!
//! # Cas de référence (CA1)
//!
//! Rectangle 100 × 100 (bord bas y = 0), marge uniforme 10 mm, pince de sommet A = (50 ; 40), axe
//! vertical vers (50 ; 0), longueur 40, ouverture 10 : pieds (45 ; 0) et (55 ; 0), jambes
//! √(40² + 5²) = √1625. Demi-angle α = atan(5/40) = atan(1/8), tan α = 1/8 ; angle de fermeture
//! 2α : cos 2α = (1 − 1/64) / (1 + 1/64) = 63/65, sin 2α = (2/8) / (1 + 1/64) = 16/65.
//!
//! Pince fermée (jambe de droite en place, celle de gauche tourne autour de A de 2α pour la rejoindre
//! ; le côté mobile est le bord qui suit la base) : le bord bas devient deux demi-bords qui se
//! rejoignent au pied fixe F = (45 ; 0) en un coin rentrant ; la figure est symétrique par rapport à
//! la ligne de la jambe (droite A–F, qui passe par A = (50 ; 40) et F = (45 ; 0) : pente 40/5 = 8).
//! Les deux décalages (à 10 mm sous chaque demi-bord) se coupent donc sur cette droite, en J où
//! y = −10 : x = 45 + (−10 − 0) / 8 = 43,75, soit J = (43,75 ; −10) (= F + 10·tan α sur le côté
//! fixe). On rouvre : le côté mobile (la partie droite) subit la rotation inverse (−2α autour de A),
//! J devient J' = symétrique de J par rapport à l'axe x = 50, soit J' = (56,25 ; −10). La coupe
//! rouverte est donc : (−10 ; −10) → J → J' → (110 ; −10) → (110 ; 110) → (−10 ; 110).
//! Les trois points du bas sont alignés (bord droit, marge uniforme) : la pointe de marge n'ajoute
//! que deux sommets à la bouche ; périmètre 480, aire 14 400 comme la coupe ordinaire.

use pf_core::persist;
use pf_core::piece::PieceIssue;
use pf_core::*;
use pf_geometry::region::trace_geometry;
use pf_geometry::{distance_mm, PointMm, Side};
use std::path::PathBuf;

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

fn bytes(p: &Project) -> String {
    persist::to_json(p).unwrap()
}

struct Scene {
    p: Project,
    s: SheetId,
    piece: PieceId,
}

/// Rectangle `[x0 ; x0 + w] × [y0 ; y0 + h]` devenu pièce, marge uniforme `margin`.
fn rect_scene(x0: f64, y0: f64, w: f64, h: f64, margin: f64) -> Scene {
    let mut p = Project::new("Coupe pince fermée");
    let s = p.sheets[0].id;
    p.apply(Command::AddRectangle {
        sheet: s,
        corner: PointRef::New(pt(x0, y0)),
        width_mm: w,
        height_mm: h,
    })
    .unwrap();
    let piece = p
        .apply(Command::ExtractPiece {
            sheet: s,
            source: PieceSource::Click {
                at: pt(x0 + w / 2.0, y0 + h / 2.0),
            },
            name: "Pièce".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    if margin > 0.0 {
        p.apply(Command::SetSeamAllowance {
            piece,
            seam: Some(piece_seam::SeamAllowance {
                default_mm: margin,
                ..Default::default()
            }),
        })
        .unwrap();
    }
    Scene { p, s, piece }
}

fn add_dart(
    sc: &mut Scene,
    apex: PointMm,
    toward: PointMm,
    length: f64,
    opening: f64,
    fixed: Side,
) -> Result<DartId, CoreError> {
    sc.p.apply(Command::AddPieceDart {
        piece: sc.piece,
        apex: PointRef::New(apex),
        toward: PointRef::New(toward),
        length_mm: length,
        opening_mm: opening,
        fixed,
    })
    .map(|o| o.darts[0])
}

fn set_closed(sc: &mut Scene, dart: DartId, closed: bool) -> Result<Outcome, CoreError> {
    sc.p.apply(Command::SetPieceDartCutClosed {
        piece: sc.piece,
        dart,
        closed,
    })
}

/// Débuts des tracés de la coupe, dans l'ordre (chaque tracé est un segment).
fn starts(p: &Project, piece: PieceId) -> Vec<PointMm> {
    p.piece_cut_contour(piece)
        .unwrap()
        .traces
        .iter()
        .flat_map(trace_geometry)
        .map(|g| g.start())
        .collect()
}

/// Même suite de sommets à 1e-6 mm près, à une rotation circulaire près.
fn same_cycle(got: &[PointMm], want: &[(f64, f64)]) -> Result<(), String> {
    if got.len() != want.len() {
        return Err(format!(
            "{} sommets au lieu de {} : {got:?}",
            got.len(),
            want.len()
        ));
    }
    let n = want.len();
    'shift: for k in 0..n {
        for i in 0..n {
            let (g, w) = (got[(i + k) % n], want[i]);
            if distance_mm(g, pt(w.0, w.1)) > 1e-6 {
                continue 'shift;
            }
        }
        return Ok(());
    }
    Err(format!("sommets {got:?} ≠ attendus {want:?}"))
}

const REF_CUT: [(f64, f64); 6] = [
    (-10.0, -10.0),
    (43.75, -10.0),
    (56.25, -10.0),
    (110.0, -10.0),
    (110.0, 110.0),
    (-10.0, 110.0),
];

const PLAIN_CUT: [(f64, f64); 4] = [
    (-10.0, -10.0),
    (110.0, -10.0),
    (110.0, 110.0),
    (-10.0, 110.0),
];

fn ref_scene(fixed: Side) -> (Scene, DartId) {
    let mut sc = rect_scene(0.0, 0.0, 100.0, 100.0, 10.0);
    let d = add_dart(&mut sc, pt(50.0, 40.0), pt(50.0, 0.0), 40.0, 10.0, fixed).unwrap();
    (sc, d)
}

// ----- CA1 : cas de référence calculés à la main -----------------------------------------

#[test]
fn ca1_reference_rectangle_les_deux_jambes_fixes_donnent_les_memes_sommets() {
    for fixed in [Side::Left, Side::Right] {
        let (mut sc, d) = ref_scene(fixed);
        // Avant le réglage : coupe ordinaire (4 coins).
        same_cycle(&starts(&sc.p, sc.piece), &PLAIN_CUT).unwrap();
        set_closed(&mut sc, d, true).unwrap();
        same_cycle(&starts(&sc.p, sc.piece), &REF_CUT)
            .unwrap_or_else(|e| panic!("jambe fixe {fixed:?} : {e}"));
        let cut = sc.p.piece_cut_contour(sc.piece).unwrap();
        assert!(
            (cut.perimeter_mm - 480.0).abs() < 1e-6,
            "{}",
            cut.perimeter_mm
        );
        assert!((cut.area_mm2 - 14_400.0).abs() < 1e-6, "{}", cut.area_mm2);
        assert!(sc.p.piece_cut_dart_failure(sc.piece).is_none());
        assert!(!sc
            .p
            .piece_issues(sc.piece)
            .iter()
            .any(|i| matches!(i, PieceIssue::CutDartFailed { .. })));
    }
}

#[test]
fn ca1_pince_sur_le_bord_gauche() {
    // Rectangle 100 × 100, pince sur le bord gauche (x = 0) : sommet (40 ; 50), axe vers (0 ; 50),
    // longueur 40, ouverture 10 → pieds (0 ; 45) et (0 ; 55). Le contour est anti-horaire : le bord
    // gauche monte… non : il descend (de (0 ; 100) à (0 ; 0)). La figure est la précédente tournée
    // d'un quart de tour (le bas devient la gauche) : J = (−10 ; 56,25), J' = (−10 ; 43,75) dans
    // le sens de parcours (de haut en bas).
    for fixed in [Side::Left, Side::Right] {
        let mut sc = rect_scene(0.0, 0.0, 100.0, 100.0, 10.0);
        let d = add_dart(&mut sc, pt(40.0, 50.0), pt(0.0, 50.0), 40.0, 10.0, fixed).unwrap();
        set_closed(&mut sc, d, true).unwrap();
        same_cycle(
            &starts(&sc.p, sc.piece),
            &[
                (-10.0, -10.0),
                (110.0, -10.0),
                (110.0, 110.0),
                (-10.0, 110.0),
                (-10.0, 56.25),
                (-10.0, 43.75),
            ],
        )
        .unwrap_or_else(|e| panic!("jambe fixe {fixed:?} : {e}"));
    }
}

/// Cas d'une base oblique : rectangle tourné de 30° autour de l'origine ; la coupe attendue est
/// celle du cas de référence tournée de 30° (rotation d'un cas déjà calculé à la main).
#[test]
fn ca1_pince_oblique_rectangle_tourne() {
    let (c, s) = (30.0_f64.to_radians().cos(), 30.0_f64.to_radians().sin());
    let rot = |x: f64, y: f64| pt(c * x - s * y, s * x + c * y);
    let mut p = Project::new("Oblique");
    let sh = p.sheets[0].id;
    // Quatre sommets tournés, quatre segments.
    let corners = [
        rot(0.0, 0.0),
        rot(100.0, 0.0),
        rot(100.0, 100.0),
        rot(0.0, 100.0),
    ];
    for i in 0..4 {
        p.apply(Command::AddSegment {
            sheet: sh,
            from: PointRef::New(corners[i]),
            to: PointRef::New(corners[(i + 1) % 4]),
        })
        .unwrap();
    }
    let piece = p
        .apply(Command::ExtractPiece {
            sheet: sh,
            source: PieceSource::Click {
                at: rot(50.0, 50.0),
            },
            name: "Oblique".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    p.apply(Command::SetSeamAllowance {
        piece,
        seam: Some(piece_seam::SeamAllowance {
            default_mm: 10.0,
            ..Default::default()
        }),
    })
    .unwrap();
    let mut sc = Scene { p, s: sh, piece };
    let a = rot(50.0, 40.0);
    let t = rot(50.0, 0.0);
    let d = add_dart(&mut sc, a, t, 40.0, 10.0, Side::Right).unwrap();
    set_closed(&mut sc, d, true).unwrap();
    let want: Vec<(f64, f64)> = REF_CUT
        .iter()
        .map(|&(x, y)| {
            let q = rot(x, y);
            (q.x, q.y)
        })
        .collect();
    same_cycle(&starts(&sc.p, sc.piece), &want).unwrap();
}

/// Marge qui varie sous la pince : le bord bas (marge 10 au début, 20 à la fin) a pour décalage la
/// droite O : y = −10 − x/10, de (0 ; −10) à (100 ; −20) ; les autres bords gardent 10 mm. Coins :
/// (−10 ; −9) (O coupe x = −10) et (110 ; −21). Pince de référence (jambe de droite fixe, F =
/// (45 ; 0), A = (50 ; 40)) ; la fermeture est la rotation R de centre A et d'angle −2α :
/// R(u ; v) = ((63 u + 16 v) / 65 ; (−16 u + 63 v) / 65) pour (u ; v) relatifs à A.
/// J' est le point de O tel que J = R(J') soit encore sur O (le décalage du côté mobile fermé coupe
/// O en J). J' = (50 + u ; −10 − (50 + u)/10), donc (u ; v) = (u ; −55 − u/10) ; J_x = 50 + (61,4 u −
/// 880)/65 et J_y = 40 + (−22,3 u − 3465)/65 ; J_y + 10 + J_x/10 = 0 ⇒ 220 − 161,6 u = 0 ⇒ u = 275/202.
/// D'où J' = (10375/202 ; −3057,5/202) = (51,3614 ; −15,1361) et J = (7625/202 ; −2782,5/202) =
/// (37,7475 ; −13,7748). J' est avant le début (55 ; −15,5) du décalage du bord mobile : le moteur
/// le prolonge jusqu'à J' (le décalage est une droite). Tous les points du bas restent sur O.
#[test]
fn ca1_marge_progressive_sous_la_pince() {
    let mut sc = rect_scene(0.0, 0.0, 100.0, 100.0, 0.0);
    let bottom =
        sc.p.piece(sc.piece)
            .unwrap()
            .outline
            .iter()
            .map(|e| e.entity)
            .find(|id| {
                let (a, b) = sc.p.sheets[0].segment_ends(*id).unwrap();
                a.y == 0.0 && b.y == 0.0
            })
            .unwrap();
    sc.p.apply(Command::SetSeamAllowance {
        piece: sc.piece,
        seam: Some(piece_seam::SeamAllowance {
            default_mm: 10.0,
            edges: vec![piece_seam::EdgeAllowance {
                entity: bottom,
                start_mm: 10.0,
                end_mm: 20.0,
            }],
            ..Default::default()
        }),
    })
    .unwrap();
    let d = add_dart(
        &mut sc,
        pt(50.0, 40.0),
        pt(50.0, 0.0),
        40.0,
        10.0,
        Side::Right,
    )
    .unwrap();
    same_cycle(
        &starts(&sc.p, sc.piece),
        &[
            (-10.0, -9.0),
            (110.0, -21.0),
            (110.0, 110.0),
            (-10.0, 110.0),
        ],
    )
    .unwrap();
    set_closed(&mut sc, d, true).unwrap();
    assert!(sc.p.piece_cut_dart_failure(sc.piece).is_none());
    same_cycle(
        &starts(&sc.p, sc.piece),
        &[
            (-10.0, -9.0),
            (7625.0 / 202.0, -2782.5 / 202.0),
            (10375.0 / 202.0, -3057.5 / 202.0),
            (110.0, -21.0),
            (110.0, 110.0),
            (-10.0, 110.0),
        ],
    )
    .unwrap();
}

/// Bord courbe : disque de centre O = (0 ; 0), rayon 100, marge 10 (décalage = cercle de rayon
/// R' = 110). Pince au bas du disque : milieu de base M = (0 ; −√9975), ouverture 10 (pieds
/// (∓5 ; −√9975), bien sur le cercle : 25 + 9975 = 10 000), longueur 60, sommet A = M + (0 ; 60),
/// tan α = 5/60 = 1/12 : cos 2α = 143/145, sin 2α = 24/145. Jambe de droite (x = −5, premier pied
/// dans le sens du parcours) fixe : le côté mobile tourne de −2α autour de A. Dans la figure fermée,
/// le décalage fixe est le cercle (O ; R'), le décalage mobile le cercle (O2 ; R') avec O2 = R(O),
/// R = rotation de centre A et d'angle −2α ; J = intersection de ces deux cercles la plus proche du
/// pied fixe (formule des cercles égaux : milieu de OO2 ± h · normale, h = √(R'² − (d/2)²)).
/// Rouvert : J' = R⁻¹(J) est sur le cercle (O ; R') ; la coupe est ce cercle sauf la corde J → J'.
/// Aire = π R'² − R'²/2 (φ − sin φ) (φ = angle JOJ'), périmètre = R' (2π − φ) + |JJ'|.
#[test]
fn ca1_bord_courbe_disque() {
    let mut p = Project::new("Disque");
    let s = p.sheets[0].id;
    p.apply(Command::AddCircle {
        sheet: s,
        center: PointRef::New(pt(0.0, 0.0)),
        radius_mm: 100.0,
    })
    .unwrap();
    let piece = p
        .apply(Command::ExtractPiece {
            sheet: s,
            source: PieceSource::Click { at: pt(0.0, 0.0) },
            name: "Disque".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    p.apply(Command::SetSeamAllowance {
        piece,
        seam: Some(piece_seam::SeamAllowance {
            default_mm: 10.0,
            ..Default::default()
        }),
    })
    .unwrap();
    let mut sc = Scene { p, s, piece };
    let y0 = -(9975.0_f64).sqrt();
    let d = add_dart(
        &mut sc,
        pt(0.0, y0 + 60.0),
        pt(0.0, y0),
        60.0,
        10.0,
        Side::Right,
    )
    .unwrap();
    let plain = sc.p.piece_cut_contour(piece).unwrap();
    assert!((plain.area_mm2 - std::f64::consts::PI * 110.0 * 110.0).abs() < 1e-3);
    set_closed(&mut sc, d, true).unwrap();
    assert!(sc.p.piece_cut_dart_failure(piece).is_none());
    // Oracle : figure fermée.
    let (cos, sin) = (143.0 / 145.0, -24.0 / 145.0);
    let a = pt(0.0, y0 + 60.0);
    let o2 = pt(
        a.x + cos * (0.0 - a.x) - sin * (0.0 - a.y),
        a.y + sin * (0.0 - a.x) + cos * (0.0 - a.y),
    );
    let (r1, dd) = (110.0_f64, o2.x.hypot(o2.y));
    let h = (r1 * r1 - dd * dd / 4.0).sqrt();
    let mid = pt(o2.x / 2.0, o2.y / 2.0);
    let nrm = pt(-o2.y / dd, o2.x / dd);
    let cands = [
        pt(mid.x + h * nrm.x, mid.y + h * nrm.y),
        pt(mid.x - h * nrm.x, mid.y - h * nrm.y),
    ];
    let foot = pt(-5.0, y0);
    let j = if distance_mm(cands[0], foot) < distance_mm(cands[1], foot) {
        cands[0]
    } else {
        cands[1]
    };
    let jp = pt(
        a.x + cos * (j.x - a.x) + sin * (j.y - a.y),
        a.y - sin * (j.x - a.x) + cos * (j.y - a.y),
    );
    assert!((j.x.hypot(j.y) - 110.0).abs() < 1e-9 && (jp.x.hypot(jp.y) - 110.0).abs() < 1e-9);
    assert!(
        j.x < jp.x && j.y < -100.0,
        "pointe au bas du disque : {j:?} {jp:?}"
    );
    let cut = sc.p.piece_cut_contour(piece).unwrap();
    let chords: Vec<(PointMm, PointMm)> = cut
        .traces
        .iter()
        .flat_map(trace_geometry)
        .filter_map(|g| match g {
            pf_geometry::region::EdgeGeom::Line(u, v) => Some((u, v)),
            _ => None,
        })
        .collect();
    assert_eq!(chords.len(), 1, "une seule corde : la pointe de marge");
    assert!(
        distance_mm(chords[0].0, j) < 1e-6,
        "{:?} ≠ {j:?}",
        chords[0].0
    );
    assert!(
        distance_mm(chords[0].1, jp) < 1e-6,
        "{:?} ≠ {jp:?}",
        chords[0].1
    );
    let phi = (j.x * jp.y - j.y * jp.x)
        .atan2(j.x * jp.x + j.y * jp.y)
        .abs();
    let area = std::f64::consts::PI * r1 * r1 - r1 * r1 / 2.0 * (phi - phi.sin());
    let perimeter = r1 * (2.0 * std::f64::consts::PI - phi) + distance_mm(j, jp);
    assert!(
        (cut.area_mm2 - area).abs() < 1e-4,
        "{} ≠ {area}",
        cut.area_mm2
    );
    assert!(
        (cut.perimeter_mm - perimeter).abs() < 1e-4,
        "{} ≠ {perimeter}",
        cut.perimeter_mm
    );
}

// ----- CA2 : invariant d'atelier ---------------------------------------------------------

/// Pince fermée par la rotation de fermeture = la coupe du contour fermé. Rotation écrite à la
/// main (jambe de droite fixe, cos 2α = 63/65, sin 2α = −16/65, centre A = (50 ; 40)). Contour
/// fermé local : (0 ; 0) → F = (45 ; 0) → R(100 ; 0) ; son décalage de 10 mm à droite : première
/// droite y = −10 ; seconde : direction u = (63/65 ; −16/65), normale droite (−16/65 ; −63/65),
/// passant par F + 10 · normale. Leur intersection est J ; la coupe rouverte doit avoir J comme
/// sommet, R(J') = J, et le même trajet local (coin (−10 ; −10) → J → R(110 ; −10)) à 1e-6 mm.
#[test]
fn ca2_invariant_la_coupe_fermee_par_rotation_est_celle_du_contour_ferme() {
    let (c, s) = (63.0 / 65.0, -16.0 / 65.0);
    let rot = |q: PointMm| {
        let (dx, dy) = (q.x - 50.0, q.y - 40.0);
        pt(50.0 + c * dx - s * dy, 40.0 + s * dx + c * dy)
    };
    let (mut sc, d) = ref_scene(Side::Right);
    set_closed(&mut sc, d, true).unwrap();
    let got = starts(&sc.p, sc.piece);
    // Sommets de la coupe rouverte : (−10 ; −10), J, J', E = (110 ; −10), …
    let at = |x: f64, y: f64| got.iter().position(|q| distance_mm(*q, pt(x, y)) < 1e-6);
    let (j, jp, e, c0) = (
        at(43.75, -10.0),
        at(56.25, -10.0),
        at(110.0, -10.0),
        at(-10.0, -10.0),
    );
    let (j, jp, e, c0) = (
        got[j.unwrap()],
        got[jp.unwrap()],
        got[e.unwrap()],
        got[c0.unwrap()],
    );
    // Contour fermé décalé : intersection des deux droites décalées.
    let f = pt(45.0, 0.0);
    let n2 = pt(-16.0 / 65.0, -63.0 / 65.0);
    let p2 = pt(f.x + 10.0 * n2.x, f.y + 10.0 * n2.y);
    let u2 = pt(63.0 / 65.0, -16.0 / 65.0);
    // y = −10 : p2 + t u2 → t = (−10 − p2.y) / u2.y.
    let t = (-10.0 - p2.y) / u2.y;
    let j_closed = pt(p2.x + t * u2.x, -10.0);
    assert!(distance_mm(j, j_closed) < 1e-6, "{j:?} ≠ {j_closed:?}");
    assert!(distance_mm(rot(jp), j_closed) < 1e-6, "R(J') ≠ J");
    // Le point E tourné est bien à 10 mm de la seconde moitié fermée du bord, du côté extérieur.
    let re = rot(e);
    let r100 = rot(pt(100.0, 0.0));
    let v = pt(re.x - f.x, re.y - f.y);
    let side = v.x * n2.x + v.y * n2.y; // distance à la droite fermée (normale extérieure)
    assert!((side - 10.0).abs() < 1e-6, "{side}");
    assert!((r100.x - f.x) * n2.x + (r100.y - f.y) * n2.y < 1e-9);
    // Même longueur locale (1e-6 mm) : fermé = |c0 J| + |J R(E)| ; rouvert = |c0 J| + |J' E|.
    let closed_len = distance_mm(c0, j_closed) + distance_mm(j_closed, re);
    let open_len = distance_mm(c0, j) + distance_mm(jp, e);
    assert!(
        (closed_len - open_len).abs() < 1e-6,
        "{closed_len} ≠ {open_len}"
    );
}

// ----- CA3 : défaut inchangé -------------------------------------------------------------

#[test]
fn ca3_defaut_inchange_sommets_et_octets() {
    let (mut sc, d) = ref_scene(Side::Right);
    // Contour de référence figé : les 4 coins du décalage ordinaire.
    same_cycle(&starts(&sc.p, sc.piece), &PLAIN_CUT).unwrap();
    let text = bytes(&sc.p);
    assert!(!text.contains("cut_closed"), "champ omis quand faux");
    // Réglage puis retour : octets d'avant.
    set_closed(&mut sc, d, true).unwrap();
    assert!(
        bytes(&sc.p).contains("\"cut_closed\":true")
            || bytes(&sc.p).contains("\"cut_closed\": true")
    );
    set_closed(&mut sc, d, false).unwrap();
    assert_eq!(bytes(&sc.p), text, "octets identiques après retour à faux");
    same_cycle(&starts(&sc.p, sc.piece), &PLAIN_CUT).unwrap();
}

#[test]
fn ca3_fixtures_et_projet_a_pinces_sans_le_champ_identiques_a_l_octet() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    for name in [
        "v1-exercice.pforge.json",
        "v1-grande-piece.pforge.json",
        "v2-piece-contour-droit-fil.pforge.json",
        "v2-piece-sans-contour.pforge.json",
    ] {
        let text = std::fs::read_to_string(dir.join(name)).unwrap();
        let p = persist::from_json(&text).unwrap();
        let out = persist::to_json(&p).unwrap();
        assert!(!out.contains("cut_closed"), "{name}");
        // Relecture / réécriture stable.
        assert_eq!(
            out,
            persist::to_json(&persist::from_json(&out).unwrap()).unwrap(),
            "{name}"
        );
    }
    // Projet à pinces, fichier sans le champ : relu, réécrit à l'octet.
    let (sc, _) = ref_scene(Side::Left);
    let text = bytes(&sc.p);
    assert!(text.contains("\"darts\""), "le projet porte une pince");
    assert!(!text.contains("cut_closed"));
    let back = persist::from_json(&text).unwrap();
    assert_eq!(bytes(&back), text);
    assert!(!back.piece(sc.piece).unwrap().darts[0].cut_closed);
}

// ----- CA4 : échecs propres -------------------------------------------------------------

#[test]
fn ca4_pince_non_calculable_repli_et_controle() {
    let (mut sc, d) = ref_scene(Side::Right);
    set_closed(&mut sc, d, true).unwrap();
    same_cycle(&starts(&sc.p, sc.piece), &REF_CUT).unwrap();
    // On déplace le sommet de la pince hors de la pièce : la pince n'est plus calculable ; la
    // commande de déplacement n'est pas refusée, la coupe retombe sur l'ordinaire, avec un contrôle.
    let apex = sc.p.piece(sc.piece).unwrap().darts[0].apex;
    sc.p.apply(Command::MovePoint {
        sheet: sc.s,
        point: apex,
        to: pt(50.0, 400.0),
    })
    .unwrap();
    assert!(sc.p.piece_darts(sc.piece)[0].calc.is_err());
    same_cycle(&starts(&sc.p, sc.piece), &PLAIN_CUT).unwrap();
    let reason =
        sc.p.piece_cut_dart_failure(sc.piece)
            .expect("motif du repli");
    assert!(!reason.is_empty());
    assert!(sc
        .p
        .piece_issues(sc.piece)
        .iter()
        .any(|i| matches!(i, PieceIssue::CutDartFailed { .. })));
}

#[test]
fn ca4_bouche_sur_un_coin_repli() {
    // Bord bas de 100 mm : pince dont la bouche touche le coin (0 ; 0) : pieds (0 ; 0) et (10 ; 0).
    let mut sc = rect_scene(0.0, 0.0, 100.0, 100.0, 10.0);
    let d = add_dart(
        &mut sc,
        pt(5.0, 40.0),
        pt(5.0, 0.0),
        40.0,
        10.0,
        Side::Right,
    )
    .unwrap();
    set_closed(&mut sc, d, true).unwrap();
    same_cycle(&starts(&sc.p, sc.piece), &PLAIN_CUT).unwrap();
    let why =
        sc.p.piece_cut_dart_failure(sc.piece)
            .expect("repli expliqué");
    assert!(why.contains("coin"), "{why}");
}

#[test]
fn ca4_bouches_qui_se_touchent_repli() {
    // Deux pinces sur le bord bas dont les bouches se touchent : pieds 40–50 puis 50–60.
    let mut sc = rect_scene(0.0, 0.0, 100.0, 100.0, 10.0);
    let d1 = add_dart(
        &mut sc,
        pt(45.0, 40.0),
        pt(45.0, 0.0),
        40.0,
        10.0,
        Side::Right,
    )
    .unwrap();
    let d2 = add_dart(
        &mut sc,
        pt(55.0, 40.0),
        pt(55.0, 0.0),
        40.0,
        10.0,
        Side::Right,
    )
    .unwrap();
    set_closed(&mut sc, d1, true).unwrap();
    set_closed(&mut sc, d2, true).unwrap();
    same_cycle(&starts(&sc.p, sc.piece), &PLAIN_CUT).unwrap();
    let why =
        sc.p.piece_cut_dart_failure(sc.piece)
            .expect("repli expliqué");
    assert!(why.contains("touchent"), "{why}");
}

#[test]
fn ca4_deux_pinces_separees_sont_fermees_toutes_les_deux() {
    // Pinces à x = 25 et x = 75 (ouverture 10, longueur 40) : deux pointes, symétriques.
    let mut sc = rect_scene(0.0, 0.0, 100.0, 100.0, 10.0);
    let d1 = add_dart(
        &mut sc,
        pt(25.0, 40.0),
        pt(25.0, 0.0),
        40.0,
        10.0,
        Side::Right,
    )
    .unwrap();
    let d2 = add_dart(
        &mut sc,
        pt(75.0, 40.0),
        pt(75.0, 0.0),
        40.0,
        10.0,
        Side::Left,
    )
    .unwrap();
    set_closed(&mut sc, d1, true).unwrap();
    set_closed(&mut sc, d2, true).unwrap();
    assert!(sc.p.piece_cut_dart_failure(sc.piece).is_none());
    // Pointe de la pince en x = 25 : pieds 20 et 30, fixe 20 (droite) : J = (20 − 1,25 ; −10), J' =
    // (30 + 1,25 ; −10) (mêmes décalages de 10 · tan α = 1,25 de chaque côté du pied). Idem en 75.
    same_cycle(
        &starts(&sc.p, sc.piece),
        &[
            (-10.0, -10.0),
            (18.75, -10.0),
            (31.25, -10.0),
            (68.75, -10.0),
            (81.25, -10.0),
            (110.0, -10.0),
            (110.0, 110.0),
            (-10.0, 110.0),
        ],
    )
    .unwrap();
}

#[test]
fn ca4_contour_ouvert_ou_marge_nulle_pas_de_controle() {
    // Sans marge : la coupe est le contour, réglage sans effet visible ni contrôle.
    let mut sc = rect_scene(0.0, 0.0, 100.0, 100.0, 0.0);
    let d = add_dart(
        &mut sc,
        pt(50.0, 40.0),
        pt(50.0, 0.0),
        40.0,
        10.0,
        Side::Right,
    )
    .unwrap();
    set_closed(&mut sc, d, true).unwrap();
    same_cycle(
        &starts(&sc.p, sc.piece),
        &[(0.0, 0.0), (100.0, 0.0), (100.0, 100.0), (0.0, 100.0)],
    )
    .unwrap();
    assert!(sc.p.piece_cut_dart_failure(sc.piece).is_none());
}

#[test]
fn ca4_vrais_refus_de_la_commande_sans_trace() {
    let (mut sc, d) = ref_scene(Side::Right);
    let (before, text) = (sc.p.clone(), bytes(&sc.p));
    for cmd in [
        Command::SetPieceDartCutClosed {
            piece: PieceId(999_999),
            dart: d,
            closed: true,
        },
        Command::SetPieceDartCutClosed {
            piece: sc.piece,
            dart: DartId(999_999),
            closed: true,
        },
    ] {
        let err = sc.p.apply(cmd).expect_err("refus");
        assert!(!err.to_string().is_empty());
        assert_eq!(sc.p, before);
        assert_eq!(bytes(&sc.p), text);
    }
    // Pièce dérivée liée : refusée, comme la pose d'une pince.
    let derived = sc.p.apply(Command::CreateDerivedPiece {
        mother: sc.piece,
        kind: derived::DerivedKind::Lining,
        operation: derived::DerivedOp::Offset { distance_mm: -2.0 },
        name: None,
    });
    if let Ok(out) = derived {
        let dp = out.pieces[0];
        let (before, text) = (sc.p.clone(), bytes(&sc.p));
        let err =
            sc.p.apply(Command::SetPieceDartCutClosed {
                piece: dp,
                dart: d,
                closed: true,
            })
            .expect_err("pièce dérivée : refus");
        assert!(!err.to_string().is_empty());
        assert_eq!(sc.p, before);
        assert_eq!(bytes(&sc.p), text);
    }
}

// ----- CA5 : régénération, historique, copie ---------------------------------------------

fn doc_with(sc: Scene) -> (Document, PieceId, SheetId) {
    let (piece, s) = (sc.piece, sc.s);
    (Document::new(sc.p), piece, s)
}

#[test]
fn ca5_historique_une_etape_annulable_octets_exacts() {
    let (sc, d) = ref_scene(Side::Right);
    let (mut doc, piece, _) = doc_with(sc);
    let before = bytes(doc.project());
    doc.execute(Command::SetPieceDartCutClosed {
        piece,
        dart: d,
        closed: true,
    })
    .unwrap();
    let after = bytes(doc.project());
    assert_ne!(before, after);
    // Sans effet : aucune étape.
    doc.execute(Command::SetPieceDartCutClosed {
        piece,
        dart: d,
        closed: true,
    })
    .unwrap();
    assert!(doc.undo());
    assert_eq!(bytes(doc.project()), before, "annuler = octets d'avant");
    // Une seule étape (la commande répétée n'en a pas créé) : plus rien à annuler.
    assert!(!doc.can_undo());
    assert!(doc.redo());
    assert_eq!(bytes(doc.project()), after, "rétablir = octets d'après");
    // Relecture JSON identique.
    let back = persist::from_json(&after).unwrap();
    assert_eq!(bytes(&back), after);
    assert!(back.piece(piece).unwrap().darts[0].cut_closed);
}

#[test]
fn ca5_la_coupe_suit_le_deplacement_d_un_point_et_l_ouverture() {
    let (mut sc, d) = ref_scene(Side::Right);
    set_closed(&mut sc, d, true).unwrap();
    // Ouverture 20 : tan α = 10/40 = 1/4 ; J = (45 + ... ) : pieds (40 ; 0) et (60 ; 0), fixe (40 ;
    // 0) ; la jambe A–F a pour pente 40/10 = 4 : J = (40 + (−10)/4 ; −10) = (37,5 ; −10),
    // J' = symétrique par rapport à x = 50 : (62,5 ; −10).
    let dart = sc.p.piece(sc.piece).unwrap().darts[0];
    sc.p.apply(Command::SetPieceDart {
        piece: sc.piece,
        dart: d,
        apex: dart.apex,
        toward: dart.toward,
        length_mm: 40.0,
        opening_mm: 20.0,
        fixed: Side::Right,
    })
    .unwrap();
    assert!(
        sc.p.piece(sc.piece).unwrap().darts[0].cut_closed,
        "le réglage est conservé"
    );
    same_cycle(
        &starts(&sc.p, sc.piece),
        &[
            (-10.0, -10.0),
            (37.5, -10.0),
            (62.5, -10.0),
            (110.0, -10.0),
            (110.0, 110.0),
            (-10.0, 110.0),
        ],
    )
    .unwrap();
    // Un point du contour déplacé : le rectangle s'élargit (coin droit en (130 ; 0) et (130 ; 100)) ;
    // la pince est inchangée, la coupe suit (marge 10 → x = 140).
    let corner_points: Vec<PointId> = sc.p.sheets[0]
        .points
        .iter()
        .filter(|q| {
            sc.p.sheets[0]
                .position(q.id)
                .is_ok_and(|v| (v.x - 100.0).abs() < 1e-9)
        })
        .map(|q| q.id)
        .collect();
    assert_eq!(corner_points.len(), 2);
    for id in corner_points {
        let v = sc.p.sheets[0].position(id).unwrap();
        sc.p.apply(Command::MovePoint {
            sheet: sc.s,
            point: id,
            to: pt(130.0, v.y),
        })
        .unwrap();
    }
    same_cycle(
        &starts(&sc.p, sc.piece),
        &[
            (-10.0, -10.0),
            (37.5, -10.0),
            (62.5, -10.0),
            (140.0, -10.0),
            (140.0, 110.0),
            (-10.0, 110.0),
        ],
    )
    .unwrap();
}

#[test]
fn ca5_transfert_le_reglage_suit_la_pince() {
    let (mut sc, d) = ref_scene(Side::Right);
    set_closed(&mut sc, d, true).unwrap();
    let (edge, distance_mm) = sc.p.dart_target_at(sc.piece, pt(0.0, 50.0)).unwrap();
    sc.p.apply(Command::TransferPieceDart {
        piece: sc.piece,
        dart: d,
        edge,
        distance_mm,
        part: 0.5,
    })
    .unwrap();
    let darts = &sc.p.piece(sc.piece).unwrap().darts;
    assert_eq!(darts.len(), 2);
    assert!(
        darts.iter().all(|x| x.cut_closed),
        "le réglage suit la pince transférée et la pince restante"
    );
}

#[test]
fn ca5_copie_de_feuille_conserve_le_reglage() {
    let (mut sc, d) = ref_scene(Side::Right);
    set_closed(&mut sc, d, true).unwrap();
    let out = sc.p.apply(Command::DuplicateSheet { sheet: sc.s }).unwrap();
    let new_sheet = out.sheet.expect("feuille copiée");
    let copies: Vec<&Piece> = sc
        .p
        .pieces
        .iter()
        .filter(|p| {
            p.id != sc.piece && matches!(p.link, PieceLink::Linked { sheet } if sheet == new_sheet)
        })
        .collect();
    assert_eq!(copies.len(), 1);
    assert!(copies[0].darts[0].cut_closed);
    let copy_id = copies[0].id;
    same_cycle(&starts(&sc.p, copy_id), &REF_CUT).unwrap();
}
