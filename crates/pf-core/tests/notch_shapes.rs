//! US-06.1 — crans avancés : formes (trait, T, V, U, fente), profondeur / largeur, cran **sur la
//! coupe** projeté depuis la couture. Valeurs attendues calculées à la main (jamais relues du code
//! testé). Un groupe de tests par critère :
//! CA1 formes sur la couture, CA2 projection sur la coupe (rectangle, bord incliné, marge
//! progressive, sans marge), CA3 formes sur la coupe, CA4 la position suit la marge sans rien
//! stocker, CA5 cran double, CA6 refus sans trace, CA7 une étape annulable et no-op, CA8 fichier
//! (omission, aller-retour, chargeur strict, `pf-validate`), CA9 échec de projection = contrôle.

use pf_core::persist;
use pf_core::piece::PieceIssue;
use pf_core::*;
use pf_geometry::{PointMm, Side};
use serde_json::Value;
use std::process::Command as Proc;

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

fn bytes(p: &Project) -> String {
    persist::to_json(p).unwrap()
}

fn close(a: PointMm, x: f64, y: f64, tol: f64) -> bool {
    (a.x - x).abs() <= tol && (a.y - y).abs() <= tol
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

/// Polygone fermé anti-horaire : projet, feuille, pièce, tracés (`edges[i]` : `pts[i]` → `pts[i+1]`).
fn poly(pts: &[(f64, f64)]) -> (Project, SheetId, PieceId, Vec<EntityId>) {
    let mut p = Project::new("Crans");
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
    let piece = p
        .apply(Command::ExtractPiece {
            sheet: s,
            source: PieceSource::Paths {
                entities: edges.clone(),
            },
            name: "Pièce".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    (p, s, piece, edges)
}

fn rect() -> (Project, SheetId, PieceId, Vec<EntityId>) {
    poly(&[(0.0, 0.0), (100.0, 0.0), (100.0, 60.0), (0.0, 60.0)])
}

fn margin(p: &mut Project, piece: PieceId, mm: f64) {
    p.apply(Command::SetSeamAllowance {
        piece,
        seam: Some(SeamAllowance {
            default_mm: mm,
            ..Default::default()
        }),
    })
    .unwrap();
}

fn notch(
    p: &mut Project,
    s: SheetId,
    entity: EntityId,
    at: f64,
    side: Side,
    depth: f64,
    style: NotchStyle,
) -> NotchId {
    p.apply(Command::AddNotch {
        sheet: s,
        entity,
        distance_mm: at,
        side,
        length_mm: depth,
        style,
    })
    .unwrap()
    .notches[0]
}

fn shape(
    p: &mut Project,
    s: SheetId,
    n: NotchId,
    form: NotchForm,
    width: Option<f64>,
    on_cut: bool,
) -> Result<Outcome, CoreError> {
    p.apply(Command::SetNotchShape {
        sheet: s,
        notch: n,
        form,
        width_mm: width,
        on_cut,
    })
}

fn seam_marks(p: &Project, s: SheetId, n: NotchId) -> Vec<(PointMm, PointMm)> {
    let sh = p.sheet(s).unwrap();
    sh.notch_marks(sh.notch(n).unwrap()).unwrap()
}

type Seg = ((f64, f64), (f64, f64));

fn same_segments(got: &[(PointMm, PointMm)], want: &[Seg], what: &str) {
    assert_eq!(got.len(), want.len(), "{what} : {got:?}");
    for (g, w) in got.iter().zip(want) {
        assert!(
            close(g.0, w.0 .0, w.0 .1, 1e-9) && close(g.1, w.1 .0, w.1 .1, 1e-9),
            "{what} : obtenu {g:?}, attendu {w:?}"
        );
    }
}

// ---------------------------------------------------------------- CA1

#[test]
fn ca1_formes_sur_la_couture_a_la_main() {
    let (mut p, s, _, e) = rect();
    // Bord bas (0,0) → (100,0) : tangente (1,0). À gauche = vers l'intérieur (0,+1).
    let n = notch(&mut p, s, e[0], 30.0, Side::Left, 5.0, NotchStyle::Single);
    same_segments(
        &seam_marks(&p, s, n),
        &[((30.0, 0.0), (30.0, 5.0))],
        "trait gauche",
    );
    shape(&mut p, s, n, NotchForm::T, Some(4.0), false).unwrap();
    same_segments(
        &seam_marks(&p, s, n),
        &[((30.0, 0.0), (30.0, 5.0)), ((28.0, 5.0), (32.0, 5.0))],
        "T",
    );
    shape(&mut p, s, n, NotchForm::V, Some(4.0), false).unwrap();
    same_segments(
        &seam_marks(&p, s, n),
        &[((28.0, 0.0), (30.0, 5.0)), ((30.0, 5.0), (32.0, 0.0))],
        "V",
    );
    shape(&mut p, s, n, NotchForm::Slot, Some(4.0), false).unwrap();
    same_segments(
        &seam_marks(&p, s, n),
        &[
            ((28.0, 0.0), (28.0, 5.0)),
            ((28.0, 5.0), (32.0, 5.0)),
            ((32.0, 5.0), (32.0, 0.0)),
        ],
        "fente",
    );
    // Largeur par défaut (3 mm) quand elle n'est pas précisée.
    shape(&mut p, s, n, NotchForm::V, None, false).unwrap();
    same_segments(
        &seam_marks(&p, s, n),
        &[((28.5, 0.0), (30.0, 5.0)), ((30.0, 5.0), (31.5, 0.0))],
        "V largeur par défaut",
    );
    // À droite : la forme est de l'autre côté du bord, vers (0,-1).
    let r = notch(&mut p, s, e[0], 60.0, Side::Right, 5.0, NotchStyle::Single);
    shape(&mut p, s, r, NotchForm::V, Some(4.0), false).unwrap();
    same_segments(
        &seam_marks(&p, s, r),
        &[((58.0, 0.0), (60.0, -5.0)), ((60.0, -5.0), (62.0, 0.0))],
        "V droite",
    );
}

#[test]
fn ca1_cran_u_demi_cercle_a_la_main() {
    let (mut p, s, _, e) = rect();
    let n = notch(&mut p, s, e[0], 30.0, Side::Left, 5.0, NotchStyle::Single);
    shape(&mut p, s, n, NotchForm::U, Some(4.0), false).unwrap();
    // Rayon 2, profondeur 5 : côtés droits de (28,0) à (28,3), demi-cercle de centre (30,3), côtés
    // de (32,3) à (32,0). 12 segments d'arc + 2 côtés = 14 segments.
    let m = seam_marks(&p, s, n);
    assert_eq!(m.len(), 14, "{m:?}");
    assert!(close(m[0].0, 28.0, 0.0, 1e-9) && close(m[0].1, 28.0, 3.0, 1e-9));
    assert!(close(m[13].0, 32.0, 3.0, 1e-9) && close(m[13].1, 32.0, 0.0, 1e-9));
    // Le sommet de l'arc est à (30,5) ; chaque sommet de l'arc est à 2 mm du centre (30,3), au-dessus.
    assert!(close(m[6].1, 30.0, 5.0, 1e-9), "sommet {:?}", m[6].1);
    for seg in &m[1..13] {
        for v in [seg.0, seg.1] {
            let r = ((v.x - 30.0).powi(2) + (v.y - 3.0).powi(2)).sqrt();
            assert!((r - 2.0).abs() < 1e-9 && v.y >= 3.0 - 1e-9, "{v:?}");
        }
    }
    // Largeur par défaut 3 mm (rayon 1,5) : côtés de (28,5 ; 0) à (28,5 ; 3,5).
    shape(&mut p, s, n, NotchForm::U, None, false).unwrap();
    let m = seam_marks(&p, s, n);
    assert!(close(m[0].0, 28.5, 0.0, 1e-9) && close(m[0].1, 28.5, 3.5, 1e-9));
}

#[test]
fn ca1_un_cran_sans_forme_reste_identique_a_l_ancien() {
    // Trait simple à gauche / à droite, simple et double : mêmes segments que le calcul d'origine.
    let (mut p, s, _, e) = rect();
    let n = notch(&mut p, s, e[0], 30.0, Side::Right, 5.0, NotchStyle::Double);
    same_segments(
        &seam_marks(&p, s, n),
        &[((28.5, 0.0), (28.5, -5.0)), ((31.5, 0.0), (31.5, -5.0))],
        "double à droite",
    );
}

// ---------------------------------------------------------------- CA2

#[test]
fn ca2_rectangle_marge_10_puis_15_position_sur_la_coupe() {
    let (mut p, s, piece, e) = rect();
    let n = notch(&mut p, s, e[0], 30.0, Side::Right, 5.0, NotchStyle::Single);
    shape(&mut p, s, n, NotchForm::Line, None, true).unwrap();
    margin(&mut p, piece, 10.0);
    let c = p.notch_on_cut(s, n).unwrap();
    assert!(close(c.foot, 30.0, -10.0, 0.01), "{:?}", c.foot);
    assert!(close(c.seam_point, 30.0, 0.0, 1e-9));
    assert!((c.reach_mm - 10.0).abs() < 0.01);
    assert!(close(pt(c.outward.x, c.outward.y), 0.0, -1.0, 1e-9));
    margin(&mut p, piece, 15.0);
    let c = p.notch_on_cut(s, n).unwrap();
    assert!(close(c.foot, 30.0, -15.0, 0.01), "{:?}", c.foot);
    assert!((c.reach_mm - 15.0).abs() < 0.01);
    // Le côté du cran est sans effet : la projection va toujours vers l'extérieur.
    p.apply(Command::SetNotch {
        sheet: s,
        notch: n,
        distance_mm: 30.0,
        side: Side::Left,
        length_mm: 5.0,
        style: NotchStyle::Single,
    })
    .unwrap();
    let c = p.notch_on_cut(s, n).unwrap();
    assert!(close(c.foot, 30.0, -15.0, 0.01), "{:?}", c.foot);
    // Les autres bords : droit (100,*) → x = 115 ; haut y = 75 ; gauche x = -15.
    let r = notch(&mut p, s, e[1], 20.0, Side::Left, 5.0, NotchStyle::Single);
    let t = notch(&mut p, s, e[2], 40.0, Side::Left, 5.0, NotchStyle::Single);
    let l = notch(&mut p, s, e[3], 10.0, Side::Left, 5.0, NotchStyle::Single);
    for id in [r, t, l] {
        shape(&mut p, s, id, NotchForm::Line, None, true).unwrap();
    }
    // Bord droit (100,0) → (100,60), à 20 mm : couture (100,20) → coupe (115,20).
    assert!(close(p.notch_on_cut(s, r).unwrap().foot, 115.0, 20.0, 0.01));
    // Bord haut (100,60) → (0,60), à 40 mm : couture (60,60) → coupe (60,75).
    assert!(close(p.notch_on_cut(s, t).unwrap().foot, 60.0, 75.0, 0.01));
    // Bord gauche (0,60) → (0,0), à 10 mm : couture (0,50) → coupe (-15,50).
    assert!(close(p.notch_on_cut(s, l).unwrap().foot, -15.0, 50.0, 0.01));
}

#[test]
fn ca2_bord_incline_perpendiculaire_a_la_couture() {
    // Trapèze : le bord droit (100,0) → (80,60) est incliné. Direction (-20,60)/√4000.
    let (mut p, s, piece, e) = poly(&[(0.0, 0.0), (100.0, 0.0), (80.0, 60.0), (20.0, 60.0)]);
    let len = (20.0f64 * 20.0 + 60.0 * 60.0).sqrt();
    let n = notch(
        &mut p,
        s,
        e[1],
        len / 2.0,
        Side::Left,
        5.0,
        NotchStyle::Single,
    );
    shape(&mut p, s, n, NotchForm::Line, None, true).unwrap();
    margin(&mut p, piece, 10.0);
    // Normale extérieure unitaire (60, 20)/√4000 ; couture au milieu (90,30).
    let (nx, ny) = (60.0 / len, 20.0 / len);
    let c = p.notch_on_cut(s, n).unwrap();
    assert!(
        close(c.foot, 90.0 + 10.0 * nx, 30.0 + 10.0 * ny, 0.01),
        "{:?}",
        c.foot
    );
    // Perpendiculaire à la couture : (foot - couture) · (−20, 60) = 0 et distance 10.
    let d = c.foot - c.seam_point;
    assert!((d.x * -20.0 + d.y * 60.0).abs() < 0.01 * len);
    assert!(((d.x.powi(2) + d.y.powi(2)).sqrt() - 10.0).abs() < 0.01);
    margin(&mut p, piece, 17.5);
    let c = p.notch_on_cut(s, n).unwrap();
    assert!(
        close(c.foot, 90.0 + 17.5 * nx, 30.0 + 17.5 * ny, 0.01),
        "{:?}",
        c.foot
    );
}

#[test]
fn ca2_marge_progressive_rayon_perpendiculaire_a_la_couture() {
    let (mut p, s, piece, e) = rect();
    let n = notch(&mut p, s, e[0], 30.0, Side::Right, 5.0, NotchStyle::Single);
    shape(&mut p, s, n, NotchForm::Line, None, true).unwrap();
    // Marge du bord bas de 5 (en (0,0)) à 15 (en (100,0)) : la coupe est la droite
    // y = -5 - 0,1 x ; le rayon vertical de x = 30 la rencontre en y = -8.
    p.apply(Command::SetSeamAllowance {
        piece,
        seam: Some(SeamAllowance {
            default_mm: 10.0,
            edges: vec![EdgeAllowance {
                entity: e[0],
                start_mm: 5.0,
                end_mm: 15.0,
            }],
            ..Default::default()
        }),
    })
    .unwrap();
    let c = p.notch_on_cut(s, n).unwrap();
    assert!(close(c.foot, 30.0, -8.0, 0.01), "{:?}", c.foot);
}

#[test]
fn ca2_sans_marge_ni_coupe_nette_la_coupe_est_la_couture() {
    let (mut p, s, piece, e) = rect();
    let n = notch(&mut p, s, e[0], 30.0, Side::Right, 5.0, NotchStyle::Single);
    shape(&mut p, s, n, NotchForm::Line, None, true).unwrap();
    let c = p.notch_on_cut(s, n).unwrap();
    assert!(close(c.foot, 30.0, 0.0, 0.01), "{:?}", c.foot);
    assert!(c.reach_mm.abs() < 0.01);
    // Coupe nette (déclarée) : même résultat.
    p.apply(Command::SetNetCut {
        piece,
        net_cut: true,
    })
    .unwrap();
    assert!(close(p.notch_on_cut(s, n).unwrap().foot, 30.0, 0.0, 0.01));
    // Le trait va de la coupe (30,0) vers l'intérieur : (30,5).
    let (place, marks) = p
        .notch_placed_marks(s, p.sheet(s).unwrap().notch(n).unwrap())
        .unwrap();
    assert_eq!(place, notch::NotchPlace::Seam);
    same_segments(&marks, &[((30.0, 0.0), (30.0, 5.0))], "sans marge");
}

// ---------------------------------------------------------------- CA3

#[test]
fn ca3_formes_sur_la_coupe_a_la_main() {
    let (mut p, s, piece, e) = rect();
    margin(&mut p, piece, 10.0);
    let n = notch(&mut p, s, e[0], 30.0, Side::Right, 5.0, NotchStyle::Single);
    let marks = |p: &Project| {
        let (place, m) = p
            .notch_placed_marks(s, p.sheet(s).unwrap().notch(n).unwrap())
            .unwrap();
        assert_eq!(place, notch::NotchPlace::Cut);
        m
    };
    // Pied (30,-10) ; la forme va de la coupe vers la couture (+y), largeur le long de x.
    shape(&mut p, s, n, NotchForm::Line, None, true).unwrap();
    same_segments(&marks(&p), &[((30.0, -10.0), (30.0, -5.0))], "trait");
    shape(&mut p, s, n, NotchForm::T, Some(4.0), true).unwrap();
    same_segments(
        &marks(&p),
        &[((30.0, -10.0), (30.0, -5.0)), ((28.0, -5.0), (32.0, -5.0))],
        "T",
    );
    shape(&mut p, s, n, NotchForm::V, Some(4.0), true).unwrap();
    same_segments(
        &marks(&p),
        &[((28.0, -10.0), (30.0, -5.0)), ((30.0, -5.0), (32.0, -10.0))],
        "V",
    );
    shape(&mut p, s, n, NotchForm::Slot, Some(4.0), true).unwrap();
    same_segments(
        &marks(&p),
        &[
            ((28.0, -10.0), (28.0, -5.0)),
            ((28.0, -5.0), (32.0, -5.0)),
            ((32.0, -5.0), (32.0, -10.0)),
        ],
        "fente",
    );
    shape(&mut p, s, n, NotchForm::U, Some(4.0), true).unwrap();
    let m = marks(&p);
    assert_eq!(m.len(), 14);
    assert!(close(m[0].0, 28.0, -10.0, 1e-9) && close(m[0].1, 28.0, -7.0, 1e-9));
    assert!(close(m[6].1, 30.0, -5.0, 1e-9), "sommet {:?}", m[6].1);
    assert!(close(m[13].1, 32.0, -10.0, 1e-9));
    // Aucun trait sur la couture : le cran « sur la coupe » n'y a pas de marque.
    assert!(seam_marks(&p, s, n).is_empty());
}

// ---------------------------------------------------------------- CA4

#[test]
fn ca4_la_position_sur_la_coupe_suit_la_marge_sans_rien_stocker() {
    let (mut p, s, piece, e) = rect();
    let n = notch(&mut p, s, e[0], 30.0, Side::Right, 5.0, NotchStyle::Single);
    shape(&mut p, s, n, NotchForm::V, Some(4.0), true).unwrap();
    margin(&mut p, piece, 10.0);
    let stored =
        |p: &Project| serde_json::to_string(p.sheet(s).unwrap().notch(n).unwrap()).unwrap();
    let before = stored(&p);
    let (_, m10) = p
        .notch_placed_marks(s, p.sheet(s).unwrap().notch(n).unwrap())
        .unwrap();
    for (mm, y) in [(15.0, -15.0), (4.0, -4.0), (12.5, -12.5)] {
        margin(&mut p, piece, mm);
        assert_eq!(
            stored(&p),
            before,
            "le cran stocké ne change pas avec la marge"
        );
        let (_, m) = p
            .notch_placed_marks(s, p.sheet(s).unwrap().notch(n).unwrap())
            .unwrap();
        // V de largeur 4 : pieds en (28,y) et (32,y), pointe en (30, y + 5).
        same_segments(
            &m,
            &[((28.0, y), (30.0, y + 5.0)), ((30.0, y + 5.0), (32.0, y))],
            "V suivant la marge",
        );
        assert_ne!(m, m10);
    }
    // Un cran à la position ancrée (milieu) suit aussi la marge : (50, -marge).
    p.apply(Command::SetNotchAnchor {
        sheet: s,
        notch: n,
        anchor: Some(NotchAnchor::Midpoint {}),
    })
    .unwrap();
    margin(&mut p, piece, 9.0);
    assert!(close(p.notch_on_cut(s, n).unwrap().foot, 50.0, -9.0, 0.01));
}

// ---------------------------------------------------------------- CA5

#[test]
fn ca5_cran_double_sur_la_coupe_chaque_trait_projete() {
    let (mut p, s, piece, e) = rect();
    margin(&mut p, piece, 10.0);
    let n = notch(&mut p, s, e[0], 30.0, Side::Right, 5.0, NotchStyle::Double);
    shape(&mut p, s, n, NotchForm::Line, None, true).unwrap();
    let (_, m) = p
        .notch_placed_marks(s, p.sheet(s).unwrap().notch(n).unwrap())
        .unwrap();
    same_segments(
        &m,
        &[((28.5, -10.0), (28.5, -5.0)), ((31.5, -10.0), (31.5, -5.0))],
        "double sur la coupe",
    );
}

// ---------------------------------------------------------------- CA6

#[test]
fn ca6_refus_sans_trace() {
    let (mut p, s, piece, e) = rect();
    margin(&mut p, piece, 10.0);
    let n = notch(&mut p, s, e[0], 30.0, Side::Right, 5.0, NotchStyle::Single);
    let free = segment(&mut p, s, pt(200.0, 0.0), pt(260.0, 0.0));
    let alone = notch(&mut p, s, free, 10.0, Side::Left, 5.0, NotchStyle::Single);
    let cases: Vec<(&str, Command)> = vec![
        (
            "largeur sur un trait",
            Command::SetNotchShape {
                sheet: s,
                notch: n,
                form: NotchForm::Line,
                width_mm: Some(3.0),
                on_cut: false,
            },
        ),
        (
            "largeur trop petite",
            Command::SetNotchShape {
                sheet: s,
                notch: n,
                form: NotchForm::V,
                width_mm: Some(0.05),
                on_cut: false,
            },
        ),
        (
            "largeur trop grande",
            Command::SetNotchShape {
                sheet: s,
                notch: n,
                form: NotchForm::V,
                width_mm: Some(100.5),
                on_cut: false,
            },
        ),
        (
            "largeur NaN",
            Command::SetNotchShape {
                sheet: s,
                notch: n,
                form: NotchForm::Slot,
                width_mm: Some(f64::NAN),
                on_cut: false,
            },
        ),
        (
            "largeur infinie",
            Command::SetNotchShape {
                sheet: s,
                notch: n,
                form: NotchForm::T,
                width_mm: Some(f64::INFINITY),
                on_cut: false,
            },
        ),
        (
            "U plus large que le double de la profondeur",
            Command::SetNotchShape {
                sheet: s,
                notch: n,
                form: NotchForm::U,
                width_mm: Some(10.5),
                on_cut: false,
            },
        ),
        (
            "U par défaut trop large pour une profondeur de 1 mm",
            Command::SetNotch {
                sheet: s,
                notch: n,
                distance_mm: 30.0,
                side: Side::Right,
                length_mm: 1.0,
                style: NotchStyle::Single,
            },
        ),
        (
            "sur la coupe hors pièce",
            Command::SetNotchShape {
                sheet: s,
                notch: alone,
                form: NotchForm::Line,
                width_mm: None,
                on_cut: true,
            },
        ),
        (
            "cran inconnu",
            Command::SetNotchShape {
                sheet: s,
                notch: NotchId(987_654),
                form: NotchForm::V,
                width_mm: None,
                on_cut: false,
            },
        ),
        (
            "feuille inconnue",
            Command::SetNotchShape {
                sheet: SheetId(987_654),
                notch: n,
                form: NotchForm::V,
                width_mm: None,
                on_cut: false,
            },
        ),
    ];
    for (name, command) in cases {
        if name.starts_with("U par défaut") {
            // Cas valide pour le trait : il faut d'abord un U de 3 mm de large, puis réduire.
            shape(&mut p, s, n, NotchForm::U, None, false).unwrap();
        }
        let (before, text) = (p.clone(), bytes(&p));
        assert!(p.apply(command).is_err(), "{name} aurait dû être refusé");
        assert_eq!(p, before, "{name} : refus sans trace (valeur)");
        assert_eq!(bytes(&p), text, "{name} : refus sans trace (octets)");
    }
}

// ---------------------------------------------------------------- CA7

#[test]
fn ca7_une_etape_annulable_et_no_op() {
    let (p, s, piece, e) = rect();
    let mut doc = Document::new(p);
    doc.execute(Command::SetSeamAllowance {
        piece,
        seam: Some(SeamAllowance {
            default_mm: 10.0,
            ..Default::default()
        }),
    })
    .unwrap();
    let n = doc
        .execute(Command::AddNotch {
            sheet: s,
            entity: e[0],
            distance_mm: 30.0,
            side: Side::Right,
            length_mm: 5.0,
            style: NotchStyle::Single,
        })
        .unwrap()
        .notches[0];
    let before = bytes(doc.project());
    let command = Command::SetNotchShape {
        sheet: s,
        notch: n,
        form: NotchForm::V,
        width_mm: Some(4.0),
        on_cut: true,
    };
    doc.execute(command.clone()).unwrap();
    let after = bytes(doc.project());
    assert_ne!(before, after);
    assert!(doc.undo());
    assert_eq!(bytes(doc.project()), before, "une seule annulation");
    assert!(doc.redo());
    assert_eq!(bytes(doc.project()), after);
    // Le même réglage ne crée aucune étape : une annulation revient à l'état d'avant.
    doc.execute(command).unwrap();
    assert!(doc.undo());
    assert_eq!(bytes(doc.project()), before);
    // Sur la couture sans forme : no-op aussi.
    let plain = Command::SetNotchShape {
        sheet: s,
        notch: n,
        form: NotchForm::Line,
        width_mm: None,
        on_cut: false,
    };
    doc.execute(plain).unwrap();
    assert_eq!(bytes(doc.project()), before);
    assert!(doc.undo());
    assert!(
        doc.project().sheet(s).unwrap().notch(n).is_err(),
        "l'annulation suivante retire le cran : le no-op n'a pas créé d'étape"
    );
}

// ---------------------------------------------------------------- CA8

fn cli(path: &str) -> i32 {
    Proc::new(env!("CARGO_BIN_EXE_pf-validate"))
        .arg(path)
        .output()
        .expect("lancement de pf-validate")
        .status
        .code()
        .unwrap_or(-1)
}

fn tmp_file(name: &str, text: &str) -> String {
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("notch_shapes");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, text).unwrap();
    path.to_string_lossy().into_owned()
}

#[test]
fn ca8_omission_aller_retour_et_ancien_fichier() {
    let (mut p, s, piece, e) = rect();
    margin(&mut p, piece, 10.0);
    let plain = notch(&mut p, s, e[0], 30.0, Side::Right, 5.0, NotchStyle::Single);
    let old = bytes(&p);
    // Un cran ordinaire n'écrit aucun des nouveaux champs ; relu et réécrit à l'identique.
    for key in ["\"form\"", "\"width_mm\"", "\"on_cut\""] {
        assert!(!old.contains(key), "{key} écrit sans nécessité");
    }
    assert_eq!(bytes(&persist::from_json(&old).unwrap()), old);
    // Cran avancé : champs écrits, aller-retour exact (valeur et octets).
    let fancy = notch(&mut p, s, e[1], 20.0, Side::Left, 6.0, NotchStyle::Double);
    shape(&mut p, s, fancy, NotchForm::U, Some(4.5), true).unwrap();
    let json = bytes(&p);
    let doc: Value = serde_json::from_str(&json).unwrap();
    let notches = &doc["sheets"][0]["notches"];
    assert!(notches[0].get("form").is_none() && notches[0].get("on_cut").is_none());
    assert_eq!(notches[1]["form"], "u");
    assert_eq!(notches[1]["width_mm"], 4.5);
    assert_eq!(notches[1]["on_cut"], true);
    // La position sur la coupe n'est jamais écrite.
    assert!(!json.contains("cut_foot") && !json.contains("on_cut_mm"));
    let back = persist::from_json(&json).unwrap();
    assert_eq!(back, p);
    assert_eq!(bytes(&back), json);
    // Retour au trait : les champs disparaissent et le fichier est celui d'avant.
    shape(&mut p, s, fancy, NotchForm::Line, None, false).unwrap();
    assert!(!bytes(&p).contains("\"form\""));
    assert_eq!(p.schema_version, 6);
    let _ = (plain, old);
}

#[test]
fn ca8_chargeur_strict_et_pf_validate() {
    let (mut p, s, piece, e) = rect();
    margin(&mut p, piece, 10.0);
    let n = notch(&mut p, s, e[0], 30.0, Side::Right, 5.0, NotchStyle::Single);
    shape(&mut p, s, n, NotchForm::V, Some(4.0), true).unwrap();
    let good = bytes(&p);
    assert_eq!(cli(&tmp_file("bon.pforge.json", &good)), 0);
    let bad = |from: &str, to: &str| {
        assert!(good.contains(from), "{from}");
        good.replacen(from, to, 1)
    };
    let mutants = [
        ("forme inconnue", bad("\"form\": \"v\"", "\"form\": \"w\"")),
        (
            "largeur négative",
            bad("\"width_mm\": 4.0", "\"width_mm\": -4.0"),
        ),
        (
            "largeur nulle",
            bad("\"width_mm\": 4.0", "\"width_mm\": 0.0"),
        ),
        (
            "largeur trop grande",
            bad("\"width_mm\": 4.0", "\"width_mm\": 500.0"),
        ),
        (
            "largeur textuelle",
            bad("\"width_mm\": 4.0", "\"width_mm\": \"4\""),
        ),
        (
            "on_cut textuel",
            bad("\"on_cut\": true", "\"on_cut\": \"oui\""),
        ),
        (
            "clé inconnue",
            bad("\"on_cut\": true", "\"on_cut\": true,\n \"coupe\": 1"),
        ),
        (
            "U trop large",
            bad("\"form\": \"v\"", "\"form\": \"u\"").replacen("4.0", "11.0", 1),
        ),
        (
            "largeur sur un trait",
            bad("\"form\": \"v\"", "\"form\": \"line\""),
        ),
    ];
    for (name, text) in mutants {
        assert!(persist::from_json(&text).is_err(), "{name} : chargeur");
        assert_eq!(
            cli(&tmp_file("mauvais.pforge.json", &text)),
            1,
            "{name} : pf-validate"
        );
    }
}

// ---------------------------------------------------------------- CA9

#[test]
fn ca9_projection_impossible_est_un_controle_jamais_un_refus() {
    let (mut p, s, piece, e) = rect();
    margin(&mut p, piece, 10.0);
    let n = notch(&mut p, s, e[0], 30.0, Side::Right, 5.0, NotchStyle::Single);
    shape(&mut p, s, n, NotchForm::Line, None, true).unwrap();
    assert!(!p
        .piece_issues(piece)
        .iter()
        .any(|i| matches!(i, PieceIssue::CutNotchesFailed { .. })));
    // Un sommet est déplacé : le contour se recoupe, la coupe n'est plus calculable. La commande
    // de la mère passe ; le cran signale qu'il ne se projette plus, sans panique.
    let corner = p
        .sheet(s)
        .unwrap()
        .points
        .iter()
        .find(|q| {
            close(
                p.sheet(s).unwrap().position(q.id).unwrap(),
                100.0,
                0.0,
                1e-9,
            )
        })
        .unwrap()
        .id;
    p.apply(Command::MovePoint {
        sheet: s,
        point: corner,
        to: pt(100.0, 100.0),
    })
    .unwrap();
    assert!(p.notch_on_cut(s, n).is_err());
    assert!(p.notch_on_cut_marks(s, n).is_err());
    let issues = p.piece_issues(piece);
    let issue = issues
        .iter()
        .find(|i| matches!(i, PieceIssue::CutNotchesFailed { .. }))
        .expect("contrôle CutNotchesFailed");
    assert_eq!(issue.key(), "cut_notches_failed");
    // Le cran est toujours là et reste lisible sur la couture (position de couture).
    assert!(p.sheet(s).unwrap().notch(n).is_ok());
}

#[test]
fn ca9_cran_dont_la_piece_disparait_est_signale_sans_panique() {
    let (mut p, s, piece, e) = rect();
    let n = notch(&mut p, s, e[0], 30.0, Side::Right, 5.0, NotchStyle::Single);
    shape(&mut p, s, n, NotchForm::Line, None, true).unwrap();
    p.apply(Command::DeletePiece { piece }).unwrap();
    // Plus aucune pièce : projection impossible (erreur expliquée) ; le cran est dessiné sur sa
    // couture à la place (repli), la commande de suppression n'a pas été refusée.
    let err = p.notch_on_cut(s, n).unwrap_err().to_string();
    assert!(err.contains("aucune pièce"), "{err}");
    let (place, marks) = p
        .notch_placed_marks(s, p.sheet(s).unwrap().notch(n).unwrap())
        .unwrap();
    assert_eq!(place, notch::NotchPlace::Seam);
    same_segments(
        &marks,
        &[((30.0, 0.0), (30.0, -5.0))],
        "repli sur la couture",
    );
    // Le projet reste valide et se sauvegarde.
    p.validate().unwrap();
    assert_eq!(persist::from_json(&bytes(&p)).unwrap(), p);
}

#[test]
fn ca9_copie_d_un_tracé_ne_copie_pas_la_projection() {
    let (mut p, s, _, e) = rect();
    let n = notch(&mut p, s, e[0], 30.0, Side::Right, 5.0, NotchStyle::Single);
    shape(&mut p, s, n, NotchForm::V, Some(4.0), true).unwrap();
    let out = p
        .apply(Command::Duplicate {
            sheet: s,
            entities: vec![e[0]],
            transform: pf_geometry::Transform::Translate {
                dx_mm: 0.0,
                dy_mm: 200.0,
            },
        })
        .unwrap();
    let copy = p
        .sheet(s)
        .unwrap()
        .notches
        .iter()
        .find(|c| out.notches.contains(&c.id))
        .expect("le cran est copié avec son tracé");
    assert_eq!(copy.form, NotchForm::V);
    assert_eq!(copy.width_mm, Some(4.0));
    assert!(!copy.on_cut, "la copie n'est dans aucune pièce");
}
