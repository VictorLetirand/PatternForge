//! US-09.5c (cran proportionnel) et US-09.7 (valeurs de variables par taille). Valeurs calculées à
//! la main dans chaque test.
//!
//! Fixture : segment AB de (0 ; 0) à (100 ; 0), gamme S M L (base M), écarts `B(S) = (−20 ; 0)`,
//! `B(L) = (+50 ; 0)`, `A(L) = (+10 ; 0)`. Variables `W = 100` (S : 60, L : 130) et `H = W / 2`.
//! C = polaire depuis A, 90°, distance liée à `W` ; D = A décalé de B, `dy` lié à `H`.
//!
//! | taille | A        | B          | W   | H  | C          | D           | longueur AB |
//! |--------|----------|------------|-----|----|------------|-------------|-------------|
//! | S      | (0 ; 0)  | (80 ; 0)   | 60  | 30 | (0 ; 60)   | (80 ; 30)   | 80          |
//! | M      | (0 ; 0)  | (100 ; 0)  | 100 | 50 | (0 ; 100)  | (100 ; 50)  | 100         |
//! | L      | (10 ; 0) | (150 ; 0)  | 130 | 65 | (10 ; 130) | (150 ; 65)  | 140         |
//!
//! Cran proportionnel écrit à 20 mm (fraction 0,2 de AB en base) : S 16, M 20, L 28 ; cran fixe : 20
//! partout.

use pf_core::persist;
use pf_core::*;
use pf_geometry::{PointMm, Side};

const TOL: f64 = 1e-9;

fn bytes(p: &Project) -> String {
    persist::to_json(p).expect("projet valide")
}

struct F {
    p: Project,
    sheet: SheetId,
    ab: EntityId,
    c: PointId,
    d: PointId,
    n_fixed: NotchId,
    n_prop: NotchId,
    s: SizeId,
    m: SizeId,
    l: SizeId,
}

fn size(p: &Project, label: &str) -> SizeId {
    p.sizes
        .iter()
        .flat_map(|r| r.sizes.iter())
        .find(|s| s.label == label)
        .unwrap_or_else(|| panic!("taille {label}"))
        .id
}

fn point_at(p: &Project, sheet: SheetId, x: f64, y: f64) -> PointId {
    let s = p.sheet(sheet).unwrap();
    s.points
        .iter()
        .find(|q| {
            let pos = s.position(q.id).unwrap();
            (pos.x - x).abs() < 1e-9 && (pos.y - y).abs() < 1e-9
        })
        .unwrap_or_else(|| panic!("aucun point en ({x} ; {y})"))
        .id
}

fn derived(p: &mut Project, sheet: SheetId, d: PointDefinition) -> PointId {
    p.apply(Command::AddPointAt {
        sheet,
        point: PointRef::Derived(d),
    })
    .unwrap()
    .points[0]
}

fn bind(p: &mut Project, sheet: SheetId, point: PointId, param: ParamKind, expr: &str) {
    p.apply(Command::SetParameterExpr {
        target: ParamTarget {
            sheet,
            point,
            param,
        },
        expr: Some(expr.into()),
    })
    .unwrap();
}

fn grade(p: &mut Project, sheet: SheetId, point: PointId, size: SizeId, dx: f64) {
    p.apply(Command::SetPointGrading {
        sheet,
        point,
        size,
        dx_mm: dx,
        dy_mm: 0.0,
    })
    .unwrap();
}

fn per_size(p: &mut Project, name: &str, size: SizeId, expr: &str) {
    p.apply(Command::SetVariableSizeExpr {
        name: name.into(),
        size,
        expr: expr.into(),
    })
    .unwrap();
}

fn fixture() -> F {
    let mut p = Project::new("Valeurs par taille");
    let sheet = p.sheets[0].id;
    p.apply(Command::AddSizeRange {
        name: "Femme".into(),
        kind: SizeKind::Alphanumeric,
        labels: vec!["S".into(), "M".into(), "L".into()],
        base: 1,
    })
    .unwrap();
    let (s, m, l) = (size(&p, "S"), size(&p, "M"), size(&p, "L"));
    let ab = p
        .apply(Command::AddSegment {
            sheet,
            from: PointRef::New(PointMm::new(0.0, 0.0)),
            to: PointRef::New(PointMm::new(100.0, 0.0)),
        })
        .unwrap()
        .entities[0];
    let a = point_at(&p, sheet, 0.0, 0.0);
    let b = point_at(&p, sheet, 100.0, 0.0);
    grade(&mut p, sheet, b, s, -20.0);
    grade(&mut p, sheet, b, l, 50.0);
    grade(&mut p, sheet, a, l, 10.0);
    for (name, expr) in [("W", "100"), ("H", "W / 2")] {
        p.apply(Command::AddVariable {
            name: name.into(),
            unit: VariableUnit::Mm,
            expr: expr.into(),
        })
        .unwrap();
    }
    let c = derived(
        &mut p,
        sheet,
        PointDefinition::Polar {
            from: a,
            distance_mm: 1.0,
            angle_deg: 90.0,
            reference: None,
        },
    );
    bind(&mut p, sheet, c, ParamKind::Distance, "W");
    let d = derived(
        &mut p,
        sheet,
        PointDefinition::Offset {
            from: b,
            dx_mm: 0.0,
            dy_mm: 1.0,
        },
    );
    bind(&mut p, sheet, d, ParamKind::Dy, "H");
    per_size(&mut p, "W", s, "60");
    per_size(&mut p, "W", l, "130");
    let notch = |p: &mut Project| {
        p.apply(Command::AddNotch {
            sheet,
            entity: ab,
            distance_mm: 20.0,
            side: Side::Left,
            length_mm: 5.0,
            style: NotchStyle::Single,
        })
        .unwrap()
        .notches[0]
    };
    let n_fixed = notch(&mut p);
    let n_prop = notch(&mut p);
    p.apply(Command::SetNotchProportional {
        sheet,
        notch: n_prop,
        proportional: true,
    })
    .unwrap();
    F {
        p,
        sheet,
        ab,
        c,
        d,
        n_fixed,
        n_prop,
        s,
        m,
        l,
    }
}

fn pos(f: &F, size: SizeId, point: PointId) -> (f64, f64) {
    let r = f.p.derived_for_size(f.sheet, Some(size)).unwrap();
    let q = r.points.iter().find(|(id, _)| *id == point).unwrap().1;
    (q.x, q.y)
}

fn at(f: &F, size: SizeId, point: PointId, x: f64, y: f64) {
    let (px, py) = pos(f, size, point);
    assert!(
        (px - x).abs() < TOL && (py - y).abs() < TOL,
        "({px} ; {py}) ≠ ({x} ; {y})"
    );
}

fn notch_mm(f: &F, size: SizeId, notch: NotchId) -> f64 {
    let r = f.p.derived_for_size(f.sheet, Some(size)).unwrap();
    r.notches
        .iter()
        .find(|n| n.notch == notch)
        .unwrap()
        .distance_mm
        .clone()
        .unwrap()
}

fn refused(p: &Project, c: Command, what: &str) -> String {
    let mut q = p.clone();
    let before = bytes(p);
    let err = q.apply(c).expect_err(what);
    assert_eq!(q, *p, "{what} : le projet a changé");
    assert_eq!(bytes(&q), before, "{what} : octets différents");
    err.to_string()
}

// ====================================================================== CA1 : valeurs par taille

#[test]
fn ca1_la_table_est_evaluee_par_taille_a_la_main() {
    let f = fixture();
    let get = |size: Option<SizeId>| -> Vec<(String, f64)> {
        f.p.variable_values_for_size(size).unwrap()
    };
    assert_eq!(
        get(Some(f.s)),
        vec![("W".to_string(), 60.0), ("H".to_string(), 30.0)]
    );
    assert_eq!(
        get(Some(f.m)),
        vec![("W".to_string(), 100.0), ("H".to_string(), 50.0)]
    );
    assert_eq!(
        get(Some(f.l)),
        vec![("W".to_string(), 130.0), ("H".to_string(), 65.0)]
    );
    // `None` = base = table ordinaire.
    assert_eq!(get(None), f.p.variable_values().unwrap());
    assert_eq!(f.p.variable_size_expr("W", f.s), Some("60"));
    assert_eq!(f.p.variable_size_expr("H", f.s), None);
    assert!(f.p.variable_values_for_size(Some(SizeId(987_654))).is_err());
}

// ====================================================================== CA2 : positions, écarts

#[test]
fn ca2_les_valeurs_par_taille_et_les_ecarts_s_ajoutent() {
    let f = fixture();
    // M (base) : géométrie écrite.
    at(&f, f.m, f.c, 0.0, 100.0);
    at(&f, f.m, f.d, 100.0, 50.0);
    // S : B écarté de −20, W = 60, H = 30.
    at(&f, f.s, f.c, 0.0, 60.0);
    at(&f, f.s, f.d, 80.0, 30.0);
    // L : A écarté de +10 (C suit son parent) ET W = 130 (distance de la taille) ; B + 50, H = 65.
    at(&f, f.l, f.c, 10.0, 130.0);
    at(&f, f.l, f.d, 150.0, 65.0);
    // La base écrite n'a pas bougé.
    at(&f, f.m, f.c, 0.0, 100.0);
    assert_eq!(f.p.derived_for_size(f.sheet, None).unwrap().points, {
        f.p.derived_for_size(f.sheet, Some(f.m)).unwrap().points
    });
}

#[test]
fn ca2b_sans_ecart_la_valeur_par_taille_agit_seule_et_sans_valeur_l_ecart_agit_seul() {
    let mut f = fixture();
    // XL : aucune valeur, aucun écart → projet évalué identique à la base, octet pour octet.
    f.p.apply(Command::AddSize {
        range: f.p.sizes[0].id,
        label: "XL".into(),
        at: None,
    })
    .unwrap();
    let xl = size(&f.p, "XL");
    assert_eq!(f.p.graded_project(Some(xl)).unwrap(), f.p);
    // Valeur seule (pas d'écart en XL) : W = 200, H = 100 ; A et B n'ont pas bougé.
    per_size(&mut f.p, "W", xl, "200");
    at(&f, xl, f.c, 0.0, 200.0);
    at(&f, xl, f.d, 100.0, 100.0);
    // Écart seul : retirer la valeur de XL puis graduer B de +7 → D suit B, H reste celui de la base.
    f.p.apply(Command::ClearVariableSizeExpr {
        name: "W".into(),
        size: xl,
    })
    .unwrap();
    let b = point_at(&f.p, f.sheet, 100.0, 0.0);
    grade(&mut f.p, f.sheet, b, xl, 7.0);
    at(&f, xl, f.c, 0.0, 100.0);
    at(&f, xl, f.d, 107.0, 50.0);
}

// ====================================================================== CA3 : cran proportionnel

#[test]
fn ca3_un_cran_proportionnel_garde_sa_fraction_un_cran_fixe_sa_distance() {
    let f = fixture();
    // Fraction 20 / 100 = 0,2 : S (80) → 16, M → 20, L (150 − 10 = 140) → 28.
    for (size, prop) in [(f.s, 16.0), (f.m, 20.0), (f.l, 28.0)] {
        assert!(
            (notch_mm(&f, size, f.n_prop) - prop).abs() < TOL,
            "{size:?}"
        );
        assert!(
            (notch_mm(&f, size, f.n_fixed) - 20.0).abs() < TOL,
            "{size:?}"
        );
    }
    // Longueurs de AB par taille : 80, 100, 140.
    for (size, len) in [(f.s, 80.0), (f.m, 100.0), (f.l, 140.0)] {
        let r = f.p.derived_for_size(f.sheet, Some(size)).unwrap();
        let got = r
            .path_lengths
            .iter()
            .find(|(e, _)| *e == f.ab)
            .unwrap()
            .1
            .clone()
            .unwrap();
        assert!((got - len).abs() < TOL);
    }
    // Les traits dessinés suivent : le pied du cran proportionnel de L est en (10 + 28 ; 0).
    let r = f.p.derived_for_size(f.sheet, Some(f.l)).unwrap();
    let marks = r
        .notches
        .iter()
        .find(|n| n.notch == f.n_prop)
        .unwrap()
        .marks
        .clone()
        .unwrap()
        .1;
    assert!((marks[0].0.x - 38.0).abs() < TOL && marks[0].0.y.abs() < TOL);
    // Rien n'est stocké : la distance écrite reste 20.
    assert_eq!(
        f.p.sheet(f.sheet)
            .unwrap()
            .notch(f.n_prop)
            .unwrap()
            .distance_mm,
        20.0
    );
}

#[test]
fn ca3b_la_base_et_les_exports_en_base_ne_changent_pas() {
    let f = fixture();
    let sheet = f.p.sheet(f.sheet).unwrap();
    let n = sheet.notch(f.n_prop).unwrap();
    assert_eq!(sheet.notch_distance_mm(n).unwrap(), 20.0);
    assert_eq!(f.p.graded_project(None).unwrap(), f.p);
    // La taille de base de la gamme (M) n'a ni écart ni valeur : le projet évalué est identique.
    assert_eq!(f.p.graded_project(Some(f.m)).unwrap(), f.p);
}

#[test]
fn ca4_commande_proportionnelle_un_pas_noop_et_refus_avec_ancrage_conserve() {
    let mut f = fixture();
    let mut doc = Document::new(f.p.clone());
    let set = |p: bool| Command::SetNotchProportional {
        sheet: f.sheet,
        notch: f.n_fixed,
        proportional: p,
    };
    assert!(doc.execute(set(true)).is_ok());
    assert!(
        doc.project()
            .sheet(f.sheet)
            .unwrap()
            .notch(f.n_fixed)
            .unwrap()
            .proportional
    );
    // Même réglage : aucune étape.
    let steps = |d: &mut Document| {
        let mut n = 0;
        while d.undo() {
            n += 1;
        }
        n
    };
    let before = doc.project().clone();
    doc.execute(set(true)).unwrap();
    assert_eq!(*doc.project(), before);
    // Une seule étape pour la pose (`doc` repart de la fixture).
    assert_eq!(steps(&mut doc), 1);
    assert_eq!(*doc.project(), f.p);
    // Retrait.
    f.p.apply(set(true)).unwrap();
    f.p.apply(set(false)).unwrap();
    assert!(
        !f.p.sheet(f.sheet)
            .unwrap()
            .notch(f.n_fixed)
            .unwrap()
            .proportional
    );
    // Cran ancré : refus sans trace, ancrage conservé.
    let anchored =
        f.p.apply(Command::AddAnchoredNotch {
            sheet: f.sheet,
            entity: f.ab,
            anchor: NotchAnchor::Ratio { ratio: 0.5 },
            side: Side::Left,
            length_mm: 5.0,
            style: NotchStyle::Single,
        })
        .unwrap()
        .notches[0];
    let msg = refused(
        &f.p,
        Command::SetNotchProportional {
            sheet: f.sheet,
            notch: anchored,
            proportional: true,
        },
        "cran ancré",
    );
    assert!(msg.contains("ancr"), "{msg}");
    let n = f.p.sheet(f.sheet).unwrap().notch(anchored).unwrap();
    assert_eq!(n.anchor, Some(NotchAnchor::Ratio { ratio: 0.5 }));
    assert!(!n.proportional);
    // « Retirer » sur un cran ancré : aucun changement, pas d'erreur.
    let before = f.p.clone();
    f.p.apply(Command::SetNotchProportional {
        sheet: f.sheet,
        notch: anchored,
        proportional: false,
    })
    .unwrap();
    assert_eq!(f.p, before);
    // Cran ou feuille inconnus.
    refused(
        &f.p,
        Command::SetNotchProportional {
            sheet: f.sheet,
            notch: NotchId(987_654),
            proportional: true,
        },
        "cran inconnu",
    );
    refused(
        &f.p,
        Command::SetNotchProportional {
            sheet: SheetId(987_654),
            notch: f.n_prop,
            proportional: true,
        },
        "feuille inconnue",
    );
}

#[test]
fn ca4b_ancrer_un_cran_proportionnel_le_rend_ordinaire_avec_remarque() {
    let mut f = fixture();
    let out =
        f.p.apply(Command::SetNotchAnchor {
            sheet: f.sheet,
            notch: f.n_prop,
            anchor: Some(NotchAnchor::Midpoint {}),
        })
        .unwrap();
    assert!(
        out.notes.iter().any(|n| n.contains("proportionnel")),
        "{:?}",
        out.notes
    );
    let n = f.p.sheet(f.sheet).unwrap().notch(f.n_prop).unwrap();
    assert!(!n.proportional && n.anchor.is_some());
    // Il suit désormais son bord : milieu = 8 (S), 10 (M)... : 40 / 50 / 70.
    for (size, mid) in [(f.s, 40.0), (f.m, 50.0), (f.l, 70.0)] {
        assert!((notch_mm(&f, size, f.n_prop) - mid).abs() < TOL);
    }
}

#[test]
fn ca4c_la_relecture_garde_le_cran_proportionnel_et_les_valeurs_par_taille() {
    let f = fixture();
    let text = bytes(&f.p);
    assert!(text.contains("\"proportional\": true"));
    assert!(text.contains("\"per_size\""));
    let back = persist::from_json(&text).unwrap();
    assert_eq!(back, f.p);
    assert_eq!(bytes(&back), text);
}

// ====================================================================== CA5 : contrôle de crans (08.4)

fn rect_piece(p: &mut Project, s: SheetId, x: f64, w: f64, h: f64, name: &str) -> PieceId {
    let o = p
        .apply(Command::AddRectangle {
            sheet: s,
            corner: PointRef::New(PointMm::new(x, 0.0)),
            width_mm: w,
            height_mm: h,
        })
        .unwrap();
    p.apply(Command::ExtractPiece {
        sheet: s,
        source: PieceSource::Paths {
            entities: o.entities,
        },
        name: name.into(),
        detached: false,
    })
    .unwrap()
    .pieces[0]
}

fn bottom(p: &Project, s: SheetId, x: f64, w: f64) -> EntityId {
    let sheet = p.sheet(s).unwrap();
    sheet
        .entities
        .iter()
        .map(|e| e.id)
        .find(|id| {
            sheet.segment_ends(*id).is_ok_and(|(u, v)| {
                (u.y.abs() < 1e-9 && v.y.abs() < 1e-9)
                    && ((u.x - x).abs() < 1e-9 && (v.x - (x + w)).abs() < 1e-9)
            })
        })
        .expect("bord bas orienté de gauche à droite")
}

/// Deux rectangles de 200 mm cousus par leur bord bas. En L, le premier s'allonge de 20 mm (ses
/// deux coins de droite : +20) : 220 mm contre 200. Le cran A est à 50 mm du départ ; son vis-à-vis
/// attendu sur B est `200 − s × 200 / L_A` (contours de même sens) : en base 150, en L avec un cran
/// proportionnel (s = 55) `200 − 55 × 200 / 220 = 150` (apparié), avec un cran fixe (s = 50)
/// `200 − 50 × 200 / 220 = 154,545…` (écart 4,545 > 1 mm : décalé).
fn pair(proportional: bool) -> (Project, NotchId, NotchId, SizeId) {
    let mut p = Project::new("contrôle");
    let s = p.sheets[0].id;
    let r1 = rect_piece(&mut p, s, 0.0, 200.0, 50.0, "Devant");
    let r2 = rect_piece(&mut p, s, 500.0, 200.0, 40.0, "Dos");
    p.apply(Command::AddSizeRange {
        name: "Femme".into(),
        kind: SizeKind::Alphanumeric,
        labels: vec!["M".into(), "L".into()],
        base: 0,
    })
    .unwrap();
    let l = size(&p, "L");
    for (x, y) in [(200.0, 0.0), (200.0, 50.0)] {
        let pt = point_at(&p, s, x, y);
        grade(&mut p, s, pt, l, 20.0);
    }
    let (a, b) = (bottom(&p, s, 0.0, 200.0), bottom(&p, s, 500.0, 200.0));
    let side = |p: &Project, piece, e| p.seam_side_range(piece, e, 1).unwrap();
    p.apply(Command::AddSeamPair {
        a: side(&p, r1, a),
        b: side(&p, r2, b),
        embu_mm: None,
        tolerance_mm: None,
    })
    .unwrap();
    let put = |p: &mut Project, e: EntityId, d: f64| {
        p.apply(Command::AddNotch {
            sheet: s,
            entity: e,
            distance_mm: d,
            side: Side::Left,
            length_mm: 5.0,
            style: NotchStyle::Single,
        })
        .unwrap()
        .notches[0]
    };
    let na = put(&mut p, a, 50.0);
    let nb = put(&mut p, b, 150.0);
    if proportional {
        p.apply(Command::SetNotchProportional {
            sheet: s,
            notch: na,
            proportional: true,
        })
        .unwrap();
    }
    (p, na, nb, l)
}

fn status(p: &Project, size: Option<SizeId>, n: NotchId) -> (NotchMatch, f64) {
    let r = p.notch_check_for_size(size).unwrap().remove(0);
    assert!(r.not_computable.is_none());
    let i = r.items.iter().find(|i| i.notch == n).expect("cran lu");
    (i.status, i.expected_mm)
}

#[test]
fn ca5_le_controle_de_crans_est_coherent_par_taille() {
    // Proportionnel : apparié en base (attendu 150) ET en L (55 → attendu 150).
    let (p, na, _, l) = pair(true);
    let (st, exp) = status(&p, None, na);
    assert_eq!(st, NotchMatch::Matched);
    assert!((exp - 150.0).abs() < 1e-9, "{exp}");
    let (st, exp) = status(&p, Some(l), na);
    assert_eq!(st, NotchMatch::Matched, "attendu {exp}");
    assert!((exp - 150.0).abs() < 1e-9, "{exp}");
    // Fixe : apparié en base, décalé en L (attendu 200 − 50 × 200 / 220 = 154,545…).
    let (p, na, _, l) = pair(false);
    assert_eq!(status(&p, None, na).0, NotchMatch::Matched);
    let (st, exp) = status(&p, Some(l), na);
    assert_eq!(st, NotchMatch::Shifted);
    assert!((exp - (200.0 - 50.0 * 200.0 / 220.0)).abs() < 1e-9, "{exp}");
    // Le contrôle ordinaire (base) n'a pas changé.
    assert_eq!(
        p.notch_check_report(),
        p.notch_check_for_size(None).unwrap()
    );
    assert!(p.notch_check_for_size(Some(SizeId(987_654))).is_err());
}

// ====================================================================== CA6 : commandes de valeurs

#[test]
fn ca6_poser_remplacer_retirer_une_valeur_par_taille_une_etape_chacune() {
    let f = fixture();
    let mut doc = Document::new(f.p.clone());
    let set = |expr: &str| Command::SetVariableSizeExpr {
        name: "W".into(),
        size: f.s,
        expr: expr.into(),
    };
    // Remplacement : « 60 » → « 70 + 5 » (texte rogné) = une étape.
    doc.execute(set("  70 + 5  ")).unwrap();
    assert_eq!(doc.project().variable_size_expr("W", f.s), Some("70 + 5"));
    let v = doc.project().variable_values_for_size(Some(f.s)).unwrap();
    assert_eq!(v, vec![("W".to_string(), 75.0), ("H".to_string(), 37.5)]);
    // Même texte : aucune étape.
    let before = doc.project().clone();
    doc.execute(set("70 + 5")).unwrap();
    assert_eq!(*doc.project(), before);
    // Retrait : la taille reprend l'expression de base (W = 100) ; une étape.
    doc.execute(Command::ClearVariableSizeExpr {
        name: "W".into(),
        size: f.s,
    })
    .unwrap();
    assert_eq!(doc.project().variable_size_expr("W", f.s), None);
    assert_eq!(
        doc.project().variable_values_for_size(Some(f.s)).unwrap()[0].1,
        100.0
    );
    // Retirer ce qui n'existe pas : aucune étape.
    let before = doc.project().clone();
    doc.execute(Command::ClearVariableSizeExpr {
        name: "H".into(),
        size: f.s,
    })
    .unwrap();
    assert_eq!(*doc.project(), before);
    // Exactement deux étapes (remplacer, retirer) au-dessus de la fixture.
    assert!(doc.undo() && doc.undo());
    assert!(!doc.undo());
    assert_eq!(*doc.project(), f.p);
}

#[test]
fn ca6b_refus_sans_trace() {
    let f = fixture();
    let set = |name: &str, size: SizeId, expr: &str| Command::SetVariableSizeExpr {
        name: name.into(),
        size,
        expr: expr.into(),
    };
    for (cmd, what, part) in [
        (set("W", f.m, "5"), "taille de base", "base"),
        (
            set("W", SizeId(987_654), "5"),
            "taille inconnue",
            "inconnue",
        ),
        (set("Zz", f.s, "5"), "variable inconnue", "Zz"),
        (set("W", f.s, "   "), "expression vide", "vide"),
        (set("W", f.s, "1 +"), "syntaxe", "W"),
        (set("W", f.s, "1 / 0"), "division par zéro", "W"),
        (
            set("W", f.s, "Inconnue + 1"),
            "variable citée inconnue",
            "W",
        ),
        (set("W", f.s, "H"), "cycle dans la taille (W → H → W)", "S"),
        (set("W", f.s, &"1+".repeat(300)), "trop longue", "W"),
        (
            Command::ClearVariableSizeExpr {
                name: "Zz".into(),
                size: f.s,
            },
            "retrait variable inconnue",
            "Zz",
        ),
        (
            Command::ClearVariableSizeExpr {
                name: "W".into(),
                size: SizeId(987_654),
            },
            "retrait taille inconnue",
            "inconnue",
        ),
    ] {
        let msg = refused(&f.p, cmd, what);
        assert!(msg.contains(part), "{what} : {msg}");
    }
}

#[test]
fn ca6c_une_valeur_qui_rend_une_formule_inevaluable_dans_une_taille_est_refusee() {
    let mut f = fixture();
    let sheet = f.sheet;
    let a = point_at(&f.p, sheet, 0.0, 0.0);
    let e = derived(
        &mut f.p,
        sheet,
        PointDefinition::Polar {
            from: a,
            distance_mm: 1.0,
            angle_deg: 0.0,
            reference: None,
        },
    );
    // 10 / (W − 60) : 0,25 en base (W = 100) ; en S, W vaut 60 → division par zéro. Poser la
    // formule alors que S a déjà W = 60 est refusé, avec la taille nommée.
    let bind_cmd = Command::SetParameterExpr {
        target: ParamTarget {
            sheet,
            point: e,
            param: ParamKind::Distance,
        },
        expr: Some("10 / (W - 60)".into()),
    };
    let first = refused(
        &f.p,
        bind_cmd,
        "formule posée alors que S la rend inévaluable",
    );
    assert!(first.contains("« S »"), "{first}");
    // Inversement : la formule existe d'abord (S sans valeur propre), puis la valeur de S est
    // refusée.
    f.p.apply(Command::ClearVariableSizeExpr {
        name: "W".into(),
        size: f.s,
    })
    .unwrap();
    bind(&mut f.p, sheet, e, ParamKind::Distance, "10 / (W - 60)");
    let msg = refused(
        &f.p,
        Command::SetVariableSizeExpr {
            name: "W".into(),
            size: f.s,
            expr: "60".into(),
        },
        "formule inévaluable en S",
    );
    assert!(
        msg.contains("« S »") && msg.contains("10 / (W - 60)"),
        "{msg}"
    );
}

#[test]
fn ca6d_un_projet_edite_a_la_main_donne_une_erreur_propre_a_la_lecture() {
    let mut f = fixture();
    // Hors commande (fichier modifié) : la table ne s'évalue pas en S.
    f.p.variables[0].per_size[0].expr = "1 / 0".into();
    let err = f.p.graded_project(Some(f.s)).unwrap_err().to_string();
    assert!(err.contains("« S »"), "{err}");
    match f.p.derived_for_size(f.sheet, Some(f.s)) {
        Err(DerivedError::Evaluation(m)) => assert!(m.contains("« S »"), "{m}"),
        other => panic!("{other:?}"),
    }
    assert!(f.p.variable_values_for_size(Some(f.s)).is_err());
    // Les autres tailles se lisent encore.
    assert!(f.p.derived_for_size(f.sheet, Some(f.l)).is_ok());
    // Et le chargeur refuse ce fichier.
    assert!(f.p.validate().is_err());
}

// ====================================================================== CA7 : renommage, suppression

#[test]
fn ca7_renommer_une_variable_reecrit_les_valeurs_par_taille_dans_la_meme_etape() {
    let mut f = fixture();
    per_size(&mut f.p, "H", f.s, "W + 5");
    // S : W = 60, H = 65.
    let before = f.p.variable_values_for_size(Some(f.s)).unwrap();
    assert_eq!(
        before,
        vec![("W".to_string(), 60.0), ("H".to_string(), 65.0)]
    );
    let mut doc = Document::new(f.p.clone());
    doc.execute(Command::RenameVariable {
        from: "W".into(),
        to: "Larg".into(),
    })
    .unwrap();
    let p = doc.project();
    assert_eq!(p.variable_size_expr("H", f.s), Some("Larg + 5"));
    assert_eq!(p.variables[0].name, "Larg");
    assert_eq!(p.variables[0].per_size.len(), 2);
    // Mêmes valeurs dans toutes les tailles.
    for size in [f.s, f.m, f.l] {
        let a = f.p.variable_values_for_size(Some(size)).unwrap();
        let b = p.variable_values_for_size(Some(size)).unwrap();
        assert_eq!(
            a.iter().map(|x| x.1).collect::<Vec<_>>(),
            b.iter().map(|x| x.1).collect::<Vec<_>>()
        );
    }
    // Une seule étape annulable.
    assert!(doc.undo());
    assert!(!doc.undo());
    assert_eq!(*doc.project(), f.p);
}

#[test]
fn ca7b_supprimer_une_variable_citee_par_une_valeur_par_taille_est_refuse() {
    let mut f = fixture();
    per_size(&mut f.p, "H", f.s, "W + 5");
    // Variable libre K, citée seulement par la valeur de S de H.
    f.p.apply(Command::AddVariable {
        name: "K".into(),
        unit: VariableUnit::Mm,
        expr: "3".into(),
    })
    .unwrap();
    per_size(&mut f.p, "H", f.l, "K * 2");
    let msg = refused(&f.p, Command::RemoveVariable { name: "K".into() }, "K cité");
    assert!(msg.contains("« H »"), "{msg}");
    f.p.apply(Command::ClearVariableSizeExpr {
        name: "H".into(),
        size: f.l,
    })
    .unwrap();
    f.p.apply(Command::RemoveVariable { name: "K".into() })
        .unwrap();
    // Supprimer une variable qui porte des valeurs par taille les emporte (le projet reste valide).
    let _ = bytes(&f.p);
    assert!(f.p.variables.iter().all(|v| v.name != "K"));
}

#[test]
fn ca7c_supprimer_une_taille_ou_une_gamme_retire_ses_valeurs_dans_la_meme_etape() {
    let f = fixture();
    // Taille S : sa valeur de W disparaît, celle de L reste, une seule étape.
    let mut doc = Document::new(f.p.clone());
    let out = doc.execute(Command::RemoveSize { size: f.s }).unwrap();
    assert!(
        out.notes.iter().any(|n| n.contains("valeur")),
        "{:?}",
        out.notes
    );
    assert_eq!(doc.project().variables[0].per_size.len(), 1);
    assert_eq!(doc.project().variable_size_expr("W", f.l), Some("130"));
    let _ = bytes(doc.project());
    assert!(doc.undo());
    assert!(!doc.undo());
    assert_eq!(*doc.project(), f.p);
    // Gamme entière : plus aucune valeur par taille, projet valide.
    let mut g = f.p.clone();
    let range = g.sizes[0].id;
    g.apply(Command::RemoveSizeRange { range }).unwrap();
    assert!(g.variables.iter().all(|v| v.per_size.is_empty()));
    let _ = bytes(&g);
    // Renommer une taille ne touche aucune référence (identifiants stables).
    let mut h = f.p.clone();
    h.apply(Command::RenameSize {
        size: f.s,
        label: "Petit".into(),
    })
    .unwrap();
    assert_eq!(h.variable_size_expr("W", f.s), Some("60"));
}

#[test]
fn ca7d_changer_la_base_est_refuse_tant_qu_il_y_a_des_valeurs_par_taille() {
    let mut f = fixture();
    let msg = refused(
        &f.p,
        Command::SetBaseSize { size: f.l },
        "base avec valeurs",
    );
    assert!(msg.contains("valeurs"), "{msg}");
    for size in [f.s, f.l] {
        f.p.apply(Command::ClearVariableSizeExpr {
            name: "W".into(),
            size,
        })
        .unwrap();
    }
    // Sans valeurs, le changement de base est permis ET garde la position de tous les crans
    // proportionnels dans chaque taille (la distance écrite devient celle de la nouvelle base).
    let want: Vec<f64> = [f.s, f.m, f.l]
        .iter()
        .map(|&s| notch_mm(&f, s, f.n_prop))
        .collect();
    f.p.apply(Command::SetBaseSize { size: f.l }).unwrap();
    let got: Vec<f64> = [f.s, f.m, f.l]
        .iter()
        .map(|&s| notch_mm(&f, s, f.n_prop))
        .collect();
    for (w, g) in want.iter().zip(&got) {
        assert!((w - g).abs() < 1e-9, "{want:?} ≠ {got:?}");
    }
    // Nouvelle base L : la distance écrite est 28 (= 0,2 × 140).
    assert!(
        (f.p.sheet(f.sheet)
            .unwrap()
            .notch(f.n_prop)
            .unwrap()
            .distance_mm
            - 28.0)
            .abs()
            < 1e-9
    );
}

#[test]
fn ca6e_une_valeur_qui_rend_la_geometrie_d_une_taille_impossible_est_refusee() {
    let f = fixture();
    // W = −5 en S : la distance polaire de C serait négative, C ne se calculerait pas en S.
    let msg = refused(
        &f.p,
        Command::SetVariableSizeExpr {
            name: "W".into(),
            size: f.s,
            expr: "0 - 5".into(),
        },
        "distance polaire négative en S",
    );
    assert!(msg.contains("« S »") && msg.contains("géométrie"), "{msg}");
    // Une valeur qui garde la géométrie calculable passe (W = 1 : C à 1 mm de A).
    let mut g = f.p.clone();
    per_size(&mut g, "W", f.s, "1");
    assert!(g
        .derived_for_size(f.sheet, Some(f.s))
        .unwrap()
        .issues
        .is_empty());
}

#[test]
fn ca8_les_valeurs_par_taille_n_agissent_que_par_les_formules_liees() {
    let mut f = fixture();
    // Délier les deux formules : plus aucune géométrie ne dépend de W / H, donc les valeurs par
    // taille ne changent plus que la table.
    for (point, param) in [(f.c, ParamKind::Distance), (f.d, ParamKind::Dy)] {
        f.p.apply(Command::SetParameterExpr {
            target: ParamTarget {
                sheet: f.sheet,
                point,
                param,
            },
            expr: None,
        })
        .unwrap();
    }
    // C = (0 ; 100) figé en base ; en L il ne suit que A (+10) : (10 ; 100).
    at(&f, f.s, f.c, 0.0, 100.0);
    at(&f, f.l, f.c, 10.0, 100.0);
    assert_eq!(f.p.variable_values_for_size(Some(f.l)).unwrap()[0].1, 130.0);
}
