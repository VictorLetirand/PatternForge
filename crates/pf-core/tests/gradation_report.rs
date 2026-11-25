//! US-09.3a — report de la règle de gradation (`CopyPointGrading`) et gradation proportionnelle
//! (`InterpolatePointGrading`). Valeurs calculées à la main (gamme XS S M L XL, base M).
//!
//! A (0 ; 0) : XS (−2 ; −4), S (−5 ; −10), L (+5 ; +10).
//! B (100 ; 0) : S (−9 ; −20), L (+9 ; +22), XL (+8 ; +16).
//! Pour un point à t : écart = (1−t)·écart(A) + t·écart(B), un écart absent valant 0.

use pf_core::{
    persist, Command, CoreError, Document, GradingCopyMode, GradingError, GradingFollower,
    PointDefinition, PointId, PointRef, Project, SheetId, SizeId, SizeKind,
};
use pf_geometry::PointMm;

const TOL: f64 = 1e-12;

struct Fx {
    p: Project,
    sheet: SheetId,
    a: PointId,
    b: PointId,
}

fn labels(l: &[&str]) -> Vec<String> {
    l.iter().map(|s| s.to_string()).collect()
}

fn bytes(p: &Project) -> String {
    persist::to_json(p).expect("projet valide")
}

fn size(p: &Project, label: &str) -> SizeId {
    p.sizes
        .iter()
        .flat_map(|r| r.sizes.iter())
        .find(|s| s.label == label)
        .unwrap_or_else(|| panic!("taille {label}"))
        .id
}

fn add_point(f: &mut Fx, x: f64, y: f64) -> PointId {
    f.p.apply(Command::AddPoint {
        sheet: f.sheet,
        at: PointMm::new(x, y),
    })
    .unwrap()
    .points[0]
}

fn set(p: &mut Project, sheet: SheetId, point: PointId, label: &str, dx: f64, dy: f64) {
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

fn fixture() -> Fx {
    let mut p = Project::new("Report");
    let sheet = p.sheets[0].id;
    p.apply(Command::AddSizeRange {
        name: "Femme".into(),
        kind: SizeKind::Alphanumeric,
        labels: labels(&["XS", "S", "M", "L", "XL"]),
        base: 2,
    })
    .unwrap();
    let mut f = Fx {
        p,
        sheet,
        a: PointId(0),
        b: PointId(0),
    };
    f.a = add_point(&mut f, 0.0, 0.0);
    f.b = add_point(&mut f, 100.0, 0.0);
    let (a, b, s) = (f.a, f.b, f.sheet);
    set(&mut f.p, s, a, "XS", -2.0, -4.0);
    set(&mut f.p, s, a, "S", -5.0, -10.0);
    set(&mut f.p, s, a, "L", 5.0, 10.0);
    set(&mut f.p, s, b, "S", -9.0, -20.0);
    set(&mut f.p, s, b, "L", 9.0, 22.0);
    set(&mut f.p, s, b, "XL", 8.0, 16.0);
    f
}

/// Écart stocké (`None` : aucune entrée).
fn got(f: &Fx, point: PointId, label: &str) -> Option<(f64, f64)> {
    let s = size(&f.p, label);
    f.p.sheet(f.sheet)
        .unwrap()
        .grading(point, s)
        .map(|g| (g.dx_mm, g.dy_mm))
}

fn entries(f: &Fx, point: PointId) -> usize {
    f.p.sheet(f.sheet).unwrap().gradings_of(point).count()
}

fn want(f: &Fx, point: PointId, label: &str, expected: Option<(f64, f64)>) {
    match (got(f, point, label), expected) {
        (None, None) => {}
        (Some(g), Some(e)) => assert!(
            (g.0 - e.0).abs() < TOL && (g.1 - e.1).abs() < TOL,
            "{label} : {g:?} != {e:?}"
        ),
        (g, e) => panic!("{label} : {g:?} != {e:?}"),
    }
}

fn interpolate(f: &mut Fx, points: Vec<PointId>) -> Result<(), CoreError> {
    f.p.apply(Command::InterpolatePointGrading {
        sheet: f.sheet,
        from: f.a,
        to: f.b,
        points,
    })
    .map(|_| ())
}

fn copy(
    f: &mut Fx,
    source: PointId,
    targets: Vec<PointId>,
    mode: GradingCopyMode,
) -> Result<(), CoreError> {
    f.p.apply(Command::CopyPointGrading {
        sheet: f.sheet,
        source,
        targets,
        mode,
    })
    .map(|_| ())
}

fn refused_without_trace(p: &mut Project, cmd: Command) -> GradingError {
    let before = bytes(p);
    let typed = p.check_grading_command(&cmd).expect_err("refus typé");
    let err = p.apply(cmd).unwrap_err();
    assert!(matches!(err, CoreError::Invalid(_)), "{err}");
    assert_eq!(err.to_string(), typed.to_string());
    assert_eq!(bytes(p), before, "refus avec trace");
    typed
}

// ---------------------------------------------------------------------------------------------
// Interpolation
// ---------------------------------------------------------------------------------------------

#[test]
fn ca_interpolation_a_t_025_valeurs_a_la_main() {
    let mut f = fixture();
    let p = add_point(&mut f, 25.0, 10.0); // projection sur A→B : t = 25 / 100 = 0,25
    interpolate(&mut f, vec![p]).unwrap();
    // S : 0,75·(−5 ; −10) + 0,25·(−9 ; −20) = (−3,75 − 2,25 ; −7,5 − 5) = (−6 ; −12,5)
    want(&f, p, "S", Some((-6.0, -12.5)));
    // L : 0,75·(5 ; 10) + 0,25·(9 ; 22) = (3,75 + 2,25 ; 7,5 + 5,5) = (6 ; 13)
    want(&f, p, "L", Some((6.0, 13.0)));
    // XS (B n'a pas d'écart = 0) : 0,75·(−2 ; −4) = (−1,5 ; −3)
    want(&f, p, "XS", Some((-1.5, -3.0)));
    // XL (A n'a pas d'écart = 0) : 0,25·(8 ; 16) = (2 ; 4)
    want(&f, p, "XL", Some((2.0, 4.0)));
    // La base n'a jamais d'entrée ; 4 entrées exactement.
    want(&f, p, "M", None);
    assert_eq!(entries(&f, p), 4);
    // Position évaluée en S : base (25 ; 10) + (−6 ; −12,5).
    let ev = f.p.evaluate_sheet(f.sheet, Some(size(&f.p, "S"))).unwrap();
    let pos = ev.points.iter().find(|(id, _)| *id == p).unwrap().1;
    assert!(
        (pos.x - 19.0).abs() < TOL && (pos.y - -2.5).abs() < TOL,
        "{pos:?}"
    );
    // Les extrémités n'ont pas bougé.
    want(&f, f.a, "S", Some((-5.0, -10.0)));
    want(&f, f.b, "L", Some((9.0, 22.0)));
}

#[test]
fn interpolation_a_mi_chemin_et_projection_orthogonale_du_point_hors_axe() {
    let mut f = fixture();
    // (50 ; −30) se projette en (50 ; 0) : t = 0,5 ; (80 ; 70) → t = 0,8.
    let p = add_point(&mut f, 50.0, -30.0);
    let q = add_point(&mut f, 80.0, 70.0);
    interpolate(&mut f, vec![p, q]).unwrap();
    // t = 0,5 : S (−7 ; −15), L (7 ; 16), XS (−1 ; −2), XL (4 ; 8).
    want(&f, p, "S", Some((-7.0, -15.0)));
    want(&f, p, "L", Some((7.0, 16.0)));
    want(&f, p, "XS", Some((-1.0, -2.0)));
    want(&f, p, "XL", Some((4.0, 8.0)));
    // t = 0,8 : S 0,2·(−5) + 0,8·(−9) = −8,2 ; dy 0,2·(−10) + 0,8·(−20) = −18.
    want(&f, q, "S", Some((-8.2, -18.0)));
    // L : 0,2·5 + 0,8·9 = 8,2 ; dy 0,2·10 + 0,8·22 = 19,6.
    want(&f, q, "L", Some((8.2, 19.6)));
}

#[test]
fn interpolation_t_borne_a_0_et_a_1() {
    let mut f = fixture();
    let before = add_point(&mut f, -20.0, 5.0); // projection à t = −0,2 → 0
    let after = add_point(&mut f, 160.0, 3.0); // projection à t = 1,6 → 1
    interpolate(&mut f, vec![before, after]).unwrap();
    // t = 0 : exactement A.
    want(&f, before, "XS", Some((-2.0, -4.0)));
    want(&f, before, "S", Some((-5.0, -10.0)));
    want(&f, before, "L", Some((5.0, 10.0)));
    want(&f, before, "XL", None);
    assert_eq!(entries(&f, before), 3);
    // t = 1 : exactement B.
    want(&f, after, "XS", None);
    want(&f, after, "S", Some((-9.0, -20.0)));
    want(&f, after, "L", Some((9.0, 22.0)));
    want(&f, after, "XL", Some((8.0, 16.0)));
    assert_eq!(entries(&f, after), 3);
}

#[test]
fn interpolation_ecrase_les_ecarts_de_la_cible_et_omet_les_ecarts_nuls() {
    let mut f = fixture();
    let (a, b, s) = (f.a, f.b, f.sheet);
    // Taille S : A = (5 ; 0), B = (−5 ; 0) → à t = 0,5 l'écart est nul : pas d'entrée.
    set(&mut f.p, s, a, "S", 5.0, 0.0);
    set(&mut f.p, s, b, "S", -5.0, 0.0);
    let p = add_point(&mut f, 50.0, 7.0);
    // La cible avait déjà des écarts sur S et XL : remplacés en entier.
    set(&mut f.p, s, p, "S", 99.0, 99.0);
    set(&mut f.p, s, p, "XL", 3.0, 3.0);
    interpolate(&mut f, vec![p]).unwrap();
    want(&f, p, "S", None); // 0 → entrée absente (et l'ancienne retirée)
    want(&f, p, "XL", Some((4.0, 8.0))); // remplacée
    want(&f, p, "L", Some((7.0, 16.0)));
    want(&f, p, "XS", Some((-1.0, -2.0)));
    assert_eq!(entries(&f, p), 3);
    assert!(!bytes(&f.p).contains("-0.0"));
}

#[test]
fn interpolation_segment_degenere_refuse_sans_trace() {
    let mut f = fixture();
    let p = add_point(&mut f, 25.0, 10.0);
    // Même point aux deux extrémités.
    let err = refused_without_trace(
        &mut f.p,
        Command::InterpolatePointGrading {
            sheet: f.sheet,
            from: f.a,
            to: f.a,
            points: vec![p],
        },
    );
    assert!(
        matches!(err, GradingError::DegenerateSegment { .. }),
        "{err}"
    );
    // Deux points distincts à la même position de base : la fusion d'`AddPoint` les confond, on
    // en crée un par déplacement.
    let c = add_point(&mut f, 100.0, 40.0);
    f.p.apply(Command::MovePoint {
        sheet: f.sheet,
        point: c,
        to: PointMm::new(0.0, 0.0),
    })
    .unwrap();
    let err = refused_without_trace(
        &mut f.p,
        Command::InterpolatePointGrading {
            sheet: f.sheet,
            from: f.a,
            to: c,
            points: vec![p],
        },
    );
    assert!(
        matches!(err, GradingError::DegenerateSegment { .. }),
        "{err}"
    );
}

// ---------------------------------------------------------------------------------------------
// Report
// ---------------------------------------------------------------------------------------------

#[test]
fn les_quatre_modes_de_copie_et_remplacement_des_ecarts_de_la_cible() {
    for (mode, sign) in [
        (GradingCopyMode::Same, (1.0, 1.0)),
        (GradingCopyMode::MirrorX, (-1.0, 1.0)),
        (GradingCopyMode::MirrorY, (1.0, -1.0)),
        (GradingCopyMode::Invert, (-1.0, -1.0)),
    ] {
        let mut f = fixture();
        let (s, a) = (f.sheet, f.a);
        let t1 = add_point(&mut f, 300.0, 300.0);
        let t2 = add_point(&mut f, 400.0, 300.0);
        // La cible t1 avait une règle propre (S et XL) : remplacée en entier.
        set(&mut f.p, s, t1, "S", 1.0, 1.0);
        set(&mut f.p, s, t1, "XL", 2.0, 2.0);
        copy(&mut f, a, vec![t1, t2], mode).unwrap();
        for t in [t1, t2] {
            want(&f, t, "XS", Some((-2.0 * sign.0, -4.0 * sign.1)));
            want(&f, t, "S", Some((-5.0 * sign.0, -10.0 * sign.1)));
            want(&f, t, "L", Some((5.0 * sign.0, 10.0 * sign.1)));
            want(&f, t, "XL", None);
            assert_eq!(entries(&f, t), 3, "{mode:?}");
        }
        // La source n'a pas bougé.
        want(&f, a, "S", Some((-5.0, -10.0)));
        assert_eq!(entries(&f, a), 3);
    }
}

#[test]
fn copie_ne_laisse_jamais_de_zero_negatif_ni_d_entree_nulle() {
    let mut f = fixture();
    let (s, a) = (f.sheet, f.a);
    set(&mut f.p, s, a, "S", 6.0, 0.0);
    set(&mut f.p, s, a, "L", 0.0, 3.0);
    let t = add_point(&mut f, 300.0, 300.0);
    copy(&mut f, a, vec![t], GradingCopyMode::Invert).unwrap();
    want(&f, t, "S", Some((-6.0, 0.0)));
    want(&f, t, "L", Some((0.0, -3.0)));
    for g in &f.p.sheet(s).unwrap().gradings {
        assert!(
            g.dx_mm.is_sign_positive() || g.dx_mm != 0.0,
            "dx = −0 : {g:?}"
        );
        assert!(
            g.dy_mm.is_sign_positive() || g.dy_mm != 0.0,
            "dy = −0 : {g:?}"
        );
    }
    assert!(!bytes(&f.p).contains("-0.0"));
}

#[test]
fn copie_d_une_source_sans_regle_efface_la_regle_des_cibles() {
    let mut f = fixture();
    let (s, a) = (f.sheet, f.a);
    let blank = add_point(&mut f, 300.0, 300.0);
    let t = add_point(&mut f, 400.0, 300.0);
    set(&mut f.p, s, t, "L", 4.0, 4.0);
    copy(&mut f, blank, vec![t], GradingCopyMode::Same).unwrap();
    assert_eq!(entries(&f, t), 0);
    // La source (A) reste intacte.
    assert_eq!(entries(&f, a), 3);
}

#[test]
fn source_verrouillee_ou_suivie_est_seulement_lue() {
    let mut f = fixture();
    let s = f.sheet;
    let t = add_point(&mut f, 300.0, 300.0);
    let layer =
        f.p.apply(Command::AddLayer {
            sheet: s,
            name: "Verrou".into(),
        })
        .unwrap()
        .layers[0];
    let locked =
        f.p.apply(Command::InLayer {
            sheet: s,
            layer,
            command: Box::new(Command::AddPoint {
                sheet: s,
                at: PointMm::new(700.0, 700.0),
            }),
        })
        .unwrap()
        .points[0];
    set(&mut f.p, s, locked, "S", 1.0, 2.0);
    f.p.apply(Command::SetLayerLocked {
        sheet: s,
        layer,
        locked: true,
    })
    .unwrap();
    copy(&mut f, locked, vec![t], GradingCopyMode::Same).unwrap();
    want(&f, t, "S", Some((1.0, 2.0)));

    // Source suivie par une construction maintenue (gradée avant d'être suivie) : lue aussi.
    let seg =
        f.p.apply(Command::AddSegmentPolar {
            sheet: s,
            from: PointRef::New(PointMm::new(2000.0, 2000.0)),
            length_mm: 80.0,
            angle_deg: 30.0,
        })
        .unwrap();
    let followed = *seg.points.last().unwrap();
    set(&mut f.p, s, followed, "L", 4.0, 0.0);
    f.p.apply(Command::SetConstructionKept {
        sheet: s,
        segment: seg.entities[0],
        kept: true,
    })
    .unwrap();
    let t2 = add_point(&mut f, 600.0, 300.0);
    copy(&mut f, followed, vec![t2], GradingCopyMode::MirrorX).unwrap();
    want(&f, t2, "L", Some((-4.0, 0.0)));
}

// ---------------------------------------------------------------------------------------------
// Refus sans trace
// ---------------------------------------------------------------------------------------------

#[test]
fn refus_sans_trace_pour_chaque_garde_fou_du_report() {
    let mut f = fixture();
    let (s, a) = (f.sheet, f.a);
    let ok = add_point(&mut f, 300.0, 300.0);
    let ok2 = add_point(&mut f, 500.0, 300.0);
    set(&mut f.p, s, ok2, "XL", 7.0, 7.0); // ne doit pas être effacé par un report refusé
    let cmd = |targets: Vec<PointId>, source: PointId| Command::CopyPointGrading {
        sheet: s,
        source,
        targets,
        mode: GradingCopyMode::MirrorX,
    };

    // Liste vide.
    assert_eq!(
        refused_without_trace(&mut f.p, cmd(vec![], a)),
        GradingError::NoTargets
    );
    // Feuille inconnue.
    let err = refused_without_trace(
        &mut f.p,
        Command::CopyPointGrading {
            sheet: SheetId(987_654),
            source: a,
            targets: vec![ok],
            mode: GradingCopyMode::Same,
        },
    );
    assert!(matches!(err, GradingError::UnknownSheet(_)), "{err}");
    // Source inconnue / cible inconnue (la bonne cible n'est pas modifiée).
    let err = refused_without_trace(&mut f.p, cmd(vec![ok], PointId(987_654)));
    assert!(matches!(err, GradingError::UnknownPoint(_)), "{err}");
    let err = refused_without_trace(&mut f.p, cmd(vec![ok, PointId(987_654), ok2], a));
    assert!(matches!(err, GradingError::UnknownPoint(_)), "{err}");
    // Cible = source, doublon.
    let err = refused_without_trace(&mut f.p, cmd(vec![ok, a], a));
    assert!(matches!(err, GradingError::TargetIsSource { .. }), "{err}");
    let err = refused_without_trace(&mut f.p, cmd(vec![ok, ok2, ok], a));
    assert!(matches!(err, GradingError::DuplicateTarget { .. }), "{err}");

    // Point construit : cible puis source.
    let off =
        f.p.apply(Command::AddPointAt {
            sheet: s,
            point: PointRef::Derived(PointDefinition::Offset {
                from: ok,
                dx_mm: 10.0,
                dy_mm: 5.0,
            }),
        })
        .unwrap()
        .points[0];
    let err = refused_without_trace(&mut f.p, cmd(vec![ok2, off], a));
    assert!(matches!(err, GradingError::NotFree { .. }), "{err}");
    let err = refused_without_trace(&mut f.p, cmd(vec![ok], off));
    assert!(matches!(err, GradingError::NotFree { .. }), "{err}");
}

#[test]
fn report_hors_espace_de_travail_cible_verrouillee_et_cible_suivie() {
    let mut f = fixture();
    let (s, a) = (f.sheet, f.a);
    let ok = add_point(&mut f, 300.0, 300.0);

    // Espace de travail : 500 000 + 900 000 > 1 000 000.
    let far = add_point(&mut f, 500_000.0, 0.0);
    let big = add_point(&mut f, 0.0, 600.0);
    set(&mut f.p, s, big, "S", 900_000.0, 0.0);
    let err = refused_without_trace(
        &mut f.p,
        Command::CopyPointGrading {
            sheet: s,
            source: big,
            targets: vec![ok, far],
            mode: GradingCopyMode::Same,
        },
    );
    assert!(matches!(err, GradingError::OutOfWorkspace { .. }), "{err}");
    // La symétrie X la ramène dans les bornes : accepté.
    copy(&mut f, big, vec![ok, far], GradingCopyMode::MirrorX).unwrap();
    want(&f, far, "S", Some((-900_000.0, 0.0)));

    // Cible sur un calque verrouillé.
    let layer =
        f.p.apply(Command::AddLayer {
            sheet: s,
            name: "Verrou".into(),
        })
        .unwrap()
        .layers[0];
    let locked =
        f.p.apply(Command::InLayer {
            sheet: s,
            layer,
            command: Box::new(Command::AddPoint {
                sheet: s,
                at: PointMm::new(900.0, 900.0),
            }),
        })
        .unwrap()
        .points[0];
    f.p.apply(Command::SetLayerLocked {
        sheet: s,
        layer,
        locked: true,
    })
    .unwrap();
    let err = refused_without_trace(
        &mut f.p,
        Command::CopyPointGrading {
            sheet: s,
            source: a,
            targets: vec![ok, locked],
            mode: GradingCopyMode::Same,
        },
    );
    assert!(matches!(err, GradingError::Locked { .. }), "{err}");
    let err = refused_without_trace(
        &mut f.p,
        Command::InterpolatePointGrading {
            sheet: s,
            from: a,
            to: f.b,
            points: vec![locked],
        },
    );
    assert!(matches!(err, GradingError::Locked { .. }), "{err}");

    // Cible suivie par une construction maintenue.
    let seg =
        f.p.apply(Command::AddSegmentPolar {
            sheet: s,
            from: PointRef::New(PointMm::new(2000.0, 2000.0)),
            length_mm: 80.0,
            angle_deg: 30.0,
        })
        .unwrap();
    let followed = *seg.points.last().unwrap();
    f.p.apply(Command::SetConstructionKept {
        sheet: s,
        segment: seg.entities[0],
        kept: true,
    })
    .unwrap();
    let err = refused_without_trace(
        &mut f.p,
        Command::CopyPointGrading {
            sheet: s,
            source: a,
            targets: vec![ok, followed],
            mode: GradingCopyMode::Same,
        },
    );
    assert!(
        matches!(
            err,
            GradingError::Followed {
                by: GradingFollower::Kept,
                ..
            }
        ),
        "{err}"
    );
    let err = refused_without_trace(
        &mut f.p,
        Command::InterpolatePointGrading {
            sheet: s,
            from: a,
            to: f.b,
            points: vec![ok, followed],
        },
    );
    assert!(matches!(err, GradingError::Followed { .. }), "{err}");
}

#[test]
fn refus_sans_trace_pour_chaque_garde_fou_de_l_interpolation() {
    let mut f = fixture();
    let (s, a, b) = (f.sheet, f.a, f.b);
    let p = add_point(&mut f, 25.0, 10.0);
    let q = add_point(&mut f, 60.0, 10.0);
    set(&mut f.p, s, q, "XL", 1.0, 1.0); // ne doit pas être effacé par un refus
    let cmd = |from: PointId, to: PointId, points: Vec<PointId>| Command::InterpolatePointGrading {
        sheet: s,
        from,
        to,
        points,
    };
    assert_eq!(
        refused_without_trace(&mut f.p, cmd(a, b, vec![])),
        GradingError::NoTargets
    );
    let err = refused_without_trace(&mut f.p, cmd(PointId(987_654), b, vec![p]));
    assert!(matches!(err, GradingError::UnknownPoint(_)), "{err}");
    let err = refused_without_trace(&mut f.p, cmd(a, PointId(987_654), vec![p]));
    assert!(matches!(err, GradingError::UnknownPoint(_)), "{err}");
    let err = refused_without_trace(&mut f.p, cmd(a, b, vec![p, PointId(987_654)]));
    assert!(matches!(err, GradingError::UnknownPoint(_)), "{err}");
    // Extrémité = cible, doublon.
    let err = refused_without_trace(&mut f.p, cmd(a, b, vec![p, a]));
    assert!(matches!(err, GradingError::TargetIsSource { .. }), "{err}");
    let err = refused_without_trace(&mut f.p, cmd(a, b, vec![b, q]));
    assert!(matches!(err, GradingError::TargetIsSource { .. }), "{err}");
    let err = refused_without_trace(&mut f.p, cmd(a, b, vec![p, q, p]));
    assert!(matches!(err, GradingError::DuplicateTarget { .. }), "{err}");
    // Point construit comme extrémité ou comme cible.
    let off =
        f.p.apply(Command::AddPointAt {
            sheet: s,
            point: PointRef::Derived(PointDefinition::Offset {
                from: p,
                dx_mm: 10.0,
                dy_mm: 5.0,
            }),
        })
        .unwrap()
        .points[0];
    let err = refused_without_trace(&mut f.p, cmd(a, off, vec![p]));
    assert!(matches!(err, GradingError::NotFree { .. }), "{err}");
    let err = refused_without_trace(&mut f.p, cmd(a, b, vec![q, off]));
    assert!(matches!(err, GradingError::NotFree { .. }), "{err}");
    // Feuille inconnue.
    let err = refused_without_trace(
        &mut f.p,
        Command::InterpolatePointGrading {
            sheet: SheetId(987_654),
            from: a,
            to: b,
            points: vec![p],
        },
    );
    assert!(matches!(err, GradingError::UnknownSheet(_)), "{err}");
}

#[test]
fn valeurs_non_finies_ou_hors_bornes_deja_dans_le_projet_refusees_sans_trace() {
    // Un projet valide n'en contient pas (validé à l'ouverture) : on les injecte à la main pour
    // vérifier que la commande ne les propage jamais.
    for bad in [f64::NAN, f64::INFINITY, 2.0e6] {
        let mut f = fixture();
        let (s, a) = (f.sheet, f.a);
        let t = add_point(&mut f, 300.0, 300.0);
        set(&mut f.p, s, t, "XL", 4.0, 4.0);
        f.p.sheets[0].gradings[0].dx_mm = bad;
        let kept: Vec<_> = f.p.sheets[0]
            .gradings
            .iter()
            .filter(|g| g.point == t)
            .map(|g| (g.size, g.dx_mm, g.dy_mm))
            .collect();
        let cmd = Command::CopyPointGrading {
            sheet: s,
            source: a,
            targets: vec![t],
            mode: GradingCopyMode::Same,
        };
        let err = f.p.check_grading_command(&cmd).unwrap_err();
        assert!(
            matches!(err, GradingError::NotFinite | GradingError::OutOfBounds),
            "{err}"
        );
        assert!(f.p.apply(cmd).is_err());
        let after: Vec<_> = f.p.sheets[0]
            .gradings
            .iter()
            .filter(|g| g.point == t)
            .map(|g| (g.size, g.dx_mm, g.dy_mm))
            .collect();
        assert_eq!(kept, after, "cible modifiée par un refus");
    }
}

// ---------------------------------------------------------------------------------------------
// Historique, sans effet, relecture
// ---------------------------------------------------------------------------------------------

#[test]
fn une_etape_annulable_et_retablissable_a_l_octet_pour_deux_cibles() {
    let mut f = fixture();
    let (s, a, b) = (f.sheet, f.a, f.b);
    let t1 = add_point(&mut f, 300.0, 300.0);
    let t2 = add_point(&mut f, 400.0, 300.0);
    let p = add_point(&mut f, 25.0, 10.0);
    let mut doc = Document::new(f.p.clone());
    let start = bytes(doc.project());

    doc.execute(Command::CopyPointGrading {
        sheet: s,
        source: a,
        targets: vec![t1, t2],
        mode: GradingCopyMode::MirrorY,
    })
    .unwrap();
    let after_copy = bytes(doc.project());
    assert_ne!(after_copy, start);
    doc.execute(Command::InterpolatePointGrading {
        sheet: s,
        from: a,
        to: b,
        points: vec![p],
    })
    .unwrap();
    let after_interp = bytes(doc.project());
    assert_ne!(after_interp, after_copy);

    assert!(doc.undo());
    assert_eq!(bytes(doc.project()), after_copy, "annuler l'interpolation");
    assert!(doc.undo());
    assert_eq!(bytes(doc.project()), start, "annuler le report");
    assert!(!doc.can_undo());
    assert!(doc.redo() && doc.redo());
    assert_eq!(bytes(doc.project()), after_interp, "rétablir");
    assert!(!doc.can_redo());
}

#[test]
fn commande_sans_effet_ne_cree_aucune_etape() {
    let mut f = fixture();
    let (s, a, b) = (f.sheet, f.a, f.b);
    let t = add_point(&mut f, 300.0, 300.0);
    let p = add_point(&mut f, 25.0, 10.0);
    // Cible déjà identique à la source ; point déjà interpolé ; source et cible sans règle.
    let blank1 = add_point(&mut f, 800.0, 800.0);
    let blank2 = add_point(&mut f, 900.0, 800.0);
    copy(&mut f, a, vec![t], GradingCopyMode::Same).unwrap();
    interpolate(&mut f, vec![p]).unwrap();
    let mut doc = Document::new(f.p.clone());
    let start = bytes(doc.project());
    for cmd in [
        Command::CopyPointGrading {
            sheet: s,
            source: a,
            targets: vec![t],
            mode: GradingCopyMode::Same,
        },
        Command::InterpolatePointGrading {
            sheet: s,
            from: a,
            to: b,
            points: vec![p],
        },
        Command::CopyPointGrading {
            sheet: s,
            source: blank1,
            targets: vec![blank2],
            mode: GradingCopyMode::Invert,
        },
    ] {
        doc.execute(cmd).unwrap();
        assert_eq!(bytes(doc.project()), start);
        assert!(!doc.can_undo(), "étape d'annulation créée sans effet");
    }
}

#[test]
fn relecture_apres_sauvegarde_identique_et_schema_toujours_4() {
    let mut f = fixture();
    let (s, a) = (f.sheet, f.a);
    let t = add_point(&mut f, 300.0, 300.0);
    let p = add_point(&mut f, 25.0, 10.0);
    copy(&mut f, a, vec![t], GradingCopyMode::MirrorX).unwrap();
    interpolate(&mut f, vec![p]).unwrap();
    let text = bytes(&f.p);
    let back = persist::from_json(&text).unwrap();
    assert_eq!(back, f.p);
    assert_eq!(bytes(&back), text);
    let doc: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(doc["schema_version"], 6, "schéma 5 (décision 334)");
    // Écarts rangés par (point, taille), sans doublon.
    let g = &back.sheet(s).unwrap().gradings;
    let keys: Vec<_> = g.iter().map(|g| (g.point, g.size)).collect();
    let mut sorted = keys.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(keys, sorted);
    // Les nouvelles commandes se sérialisent (modes en snake_case).
    let json = serde_json::to_string(&Command::CopyPointGrading {
        sheet: s,
        source: a,
        targets: vec![t],
        mode: GradingCopyMode::MirrorX,
    })
    .unwrap();
    assert!(json.contains("\"mirror_x\""), "{json}");
    let cmd: Command = serde_json::from_str(&json).unwrap();
    assert!(matches!(
        cmd,
        Command::CopyPointGrading {
            mode: GradingCopyMode::MirrorX,
            ..
        }
    ));
}

#[test]
fn le_report_ne_touche_pas_a_la_geometrie_ecrite() {
    let mut f = fixture();
    let a = f.a;
    let t = add_point(&mut f, 300.0, 300.0);
    let before: Vec<_> = f.p.sheets[0].points.iter().map(|p| p.definition).collect();
    copy(&mut f, a, vec![t], GradingCopyMode::Invert).unwrap();
    // (300 ; 300) se projette à t = 3 : borné à 1, donc exactement l'écart de B.
    interpolate(&mut f, vec![t]).unwrap();
    want(&f, t, "XL", Some((8.0, 16.0)));
    let after: Vec<_> = f.p.sheets[0].points.iter().map(|p| p.definition).collect();
    assert_eq!(before, after);
    assert!(matches!(after[2], PointDefinition::Free { x, y } if x == 300.0 && y == 300.0));
}
