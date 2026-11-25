//! US-09.2a — règle de gradation par point : écarts cumulés par (point libre, taille), évaluation
//! par taille, changement de base, retrait de taille, liens, copie, format. Valeurs écrites à la
//! main (gamme XS S M L XL, base M).

use pf_core::{
    persist, Command, CoreError, DependencyNature, DependencyObject, DiagnosticKind, Document,
    GradingError, LinkType, PointDefinition, PointId, PointRef, Project, SheetId, SizeError,
    SizeId, SizeKind,
};
use pf_geometry::PointMm;

const TOL: f64 = 1e-9;

fn labels(l: &[&str]) -> Vec<String> {
    l.iter().map(|s| s.to_string()).collect()
}

fn bytes(p: &Project) -> String {
    persist::to_json(p).expect("projet valide")
}

/// Projet avec une gamme XS S M L XL (base M) et un point libre P (100 ; 200).
struct Fixture {
    p: Project,
    sheet: SheetId,
    point: PointId,
}

fn fixture() -> Fixture {
    let mut p = Project::new("Gradation");
    let sheet = p.sheets[0].id;
    let point = p
        .apply(Command::AddPoint {
            sheet,
            at: PointMm::new(100.0, 200.0),
        })
        .unwrap()
        .points[0];
    p.apply(Command::AddSizeRange {
        name: "Femme".into(),
        kind: SizeKind::Alphanumeric,
        labels: labels(&["XS", "S", "M", "L", "XL"]),
        base: 2,
    })
    .unwrap();
    Fixture { p, sheet, point }
}

fn size(p: &Project, label: &str) -> SizeId {
    p.sizes
        .iter()
        .flat_map(|r| r.sizes.iter())
        .find(|s| s.label == label)
        .unwrap_or_else(|| panic!("taille {label}"))
        .id
}

fn set(f: &mut Fixture, label: &str, dx: f64, dy: f64) {
    let size = size(&f.p, label);
    f.p.apply(Command::SetPointGrading {
        sheet: f.sheet,
        point: f.point,
        size,
        dx_mm: dx,
        dy_mm: dy,
    })
    .unwrap_or_else(|e| panic!("{label} : {e}"));
}

fn at(p: &Project, sheet: SheetId, point: PointId, size: Option<SizeId>) -> (f64, f64) {
    let e = p.evaluate_sheet(sheet, size).unwrap();
    let pos = e
        .points
        .iter()
        .find(|(id, _)| *id == point)
        .unwrap_or_else(|| panic!("point {} absent : {:?}", point.0, e.issues))
        .1;
    (pos.x, pos.y)
}

fn close(a: (f64, f64), b: (f64, f64), ctx: &str) {
    assert!(
        (a.0 - b.0).abs() < TOL && (a.1 - b.1).abs() < TOL,
        "{ctx} : {a:?} != {b:?}"
    );
}

/// Les cinq écarts du cahier : S (−5 ; −10), L (+5 ; +10), XL (+10 ; +22), XS (−9 ; −20).
fn five_sizes() -> Fixture {
    let mut f = fixture();
    set(&mut f, "S", -5.0, -10.0);
    set(&mut f, "L", 5.0, 10.0);
    set(&mut f, "XL", 10.0, 22.0);
    set(&mut f, "XS", -9.0, -20.0);
    f
}

fn labelled(_f: &Fixture) -> Vec<(&'static str, (f64, f64))> {
    vec![
        ("XS", (91.0, 180.0)),
        ("S", (95.0, 190.0)),
        ("M", (100.0, 200.0)),
        ("L", (105.0, 210.0)),
        ("XL", (110.0, 222.0)),
    ]
}

#[test]
fn ca_position_en_taille_t_est_base_plus_ecart_sur_cinq_tailles() {
    let f = five_sizes();
    for (label, expected) in labelled(&f) {
        let s = size(&f.p, label);
        close(at(&f.p, f.sheet, f.point, Some(s)), expected, label);
    }
    // `None` = taille de base = géométrie écrite.
    close(at(&f.p, f.sheet, f.point, None), (100.0, 200.0), "None");
    // La géométrie écrite n'a pas bougé : aucune géométrie graduée n'est stockée.
    assert_eq!(
        f.p.sheet(f.sheet)
            .unwrap()
            .point(f.point)
            .unwrap()
            .definition,
        PointDefinition::Free { x: 100.0, y: 200.0 }
    );
    // Les écarts sont rangés dans l'ordre (point, taille) : 4 entrées.
    assert_eq!(f.p.sheet(f.sheet).unwrap().gradings.len(), 4);
}

#[test]
fn les_points_construits_se_recalculent_depuis_les_parents_graduees() {
    let mut f = five_sizes();
    // C : décalé de (30 ; −10) depuis P ; D : milieu de P et C.
    let c =
        f.p.apply(Command::AddPointAt {
            sheet: f.sheet,
            point: PointRef::Derived(PointDefinition::Offset {
                from: f.point,
                dx_mm: 30.0,
                dy_mm: -10.0,
            }),
        })
        .unwrap()
        .points[0];
    let d =
        f.p.apply(Command::AddPointAt {
            sheet: f.sheet,
            point: PointRef::Derived(PointDefinition::Midpoint {
                first: f.point,
                second: c,
            }),
        })
        .unwrap()
        .points[0];
    for (label, (px, py)) in labelled(&f) {
        let s = Some(size(&f.p, label));
        close(at(&f.p, f.sheet, c, s), (px + 30.0, py - 10.0), label);
        close(at(&f.p, f.sheet, d, s), (px + 15.0, py - 5.0), label);
    }
    // Un tracé suit ses points : le segment P–C a la même longueur dans toutes les tailles.
    let seg =
        f.p.apply(Command::AddSegment {
            sheet: f.sheet,
            from: PointRef::Existing(f.point),
            to: PointRef::Existing(c),
        })
        .unwrap()
        .entities[0];
    for (label, (px, py)) in labelled(&f) {
        let g = f.p.graded_sheet(f.sheet, Some(size(&f.p, label))).unwrap();
        let (a, b) = g.segment_ends(seg).unwrap();
        close((a.x, a.y), (px, py), label);
        close((b.x, b.y), (px + 30.0, py - 10.0), label);
    }
    // Un point construit ne se gradue pas (graduer ses parents).
    let err =
        f.p.check_grading_command(&Command::SetPointGrading {
            sheet: f.sheet,
            point: c,
            size: size(&f.p, "S"),
            dx_mm: 1.0,
            dy_mm: 0.0,
        })
        .unwrap_err();
    assert!(matches!(err, GradingError::NotFree { .. }), "{err}");
}

#[test]
fn changer_la_base_m_vers_l_laisse_toutes_les_positions_identiques() {
    let mut f = five_sizes();
    let before: Vec<_> = ["XS", "S", "M", "L", "XL"]
        .iter()
        .map(|l| at(&f.p, f.sheet, f.point, Some(size(&f.p, l))))
        .collect();
    let l = size(&f.p, "L");
    f.p.apply(Command::SetBaseSize { size: l }).unwrap();
    for (i, label) in ["XS", "S", "M", "L", "XL"].iter().enumerate() {
        close(
            at(&f.p, f.sheet, f.point, Some(size(&f.p, label))),
            before[i],
            label,
        );
    }
    // La géométrie écrite est celle de L ; `None` = nouvelle base.
    assert_eq!(
        f.p.sheet(f.sheet)
            .unwrap()
            .point(f.point)
            .unwrap()
            .definition,
        PointDefinition::Free { x: 105.0, y: 210.0 }
    );
    close(at(&f.p, f.sheet, f.point, None), (105.0, 210.0), "None");
    // Écarts re-exprimés à la main : écart'(s) = écart(s) − écart(L) = écart(s) − (5 ; 10).
    let g = &f.p.sheet(f.sheet).unwrap().gradings;
    let got = |label: &str| {
        let id = size(&f.p, label);
        g.iter().find(|e| e.size == id).map(|e| (e.dx_mm, e.dy_mm))
    };
    assert_eq!(got("XS"), Some((-14.0, -30.0)));
    assert_eq!(got("S"), Some((-10.0, -20.0)));
    assert_eq!(
        got("M"),
        Some((-5.0, -10.0)),
        "l'ancienne base reçoit un écart"
    );
    assert_eq!(got("L"), None, "la nouvelle base n'a plus d'écart");
    assert_eq!(got("XL"), Some((5.0, 12.0)));
    // Retour à M : positions encore identiques.
    let m = size(&f.p, "M");
    f.p.apply(Command::SetBaseSize { size: m }).unwrap();
    for (i, label) in ["XS", "S", "M", "L", "XL"].iter().enumerate() {
        close(
            at(&f.p, f.sheet, f.point, Some(size(&f.p, label))),
            before[i],
            label,
        );
    }
}

#[test]
fn changer_la_base_cree_les_ecarts_des_tailles_qui_n_en_avaient_pas() {
    // Seule L porte un écart : après la bascule, S, XS, XL et M doivent en recevoir un (−écart L).
    let mut f = fixture();
    set(&mut f, "L", 5.0, 10.0);
    let l = size(&f.p, "L");
    f.p.apply(Command::SetBaseSize { size: l }).unwrap();
    assert_eq!(f.p.sheet(f.sheet).unwrap().gradings.len(), 4);
    for label in ["XS", "S", "M", "XL"] {
        close(
            at(&f.p, f.sheet, f.point, Some(size(&f.p, label))),
            (100.0, 200.0),
            label,
        );
    }
    close(
        at(&f.p, f.sheet, f.point, Some(l)),
        (105.0, 210.0),
        "L reste en place",
    );
}

#[test]
fn retirer_une_taille_ne_retire_que_son_ecart_et_s_annule_a_l_octet() {
    let mut f = five_sizes();
    let mut doc = Document::new(f.p.clone());
    let s = size(&f.p, "S");
    let out = doc.execute(Command::RemoveSize { size: s }).unwrap();
    assert!(
        out.notes.iter().any(|n| n.contains("écart")),
        "remarque : {:?}",
        out.notes
    );
    let after = doc.project().clone();
    assert!(after
        .sheet(f.sheet)
        .unwrap()
        .gradings
        .iter()
        .all(|g| g.size != s));
    assert_eq!(after.sheet(f.sheet).unwrap().gradings.len(), 3);
    for (label, expected) in labelled(&f) {
        if label != "S" {
            close(
                at(&after, f.sheet, f.point, Some(size(&after, label))),
                expected,
                label,
            );
        }
    }
    // Une seule étape annulable ; annuler / rétablir exacts à l'octet.
    let redone = bytes(&after);
    assert!(doc.undo());
    assert_eq!(bytes(doc.project()), bytes(&f.p));
    assert!(!doc.can_undo());
    assert!(doc.redo());
    assert_eq!(bytes(doc.project()), redone);
    // `size_dependents` annonce les écarts d'une taille.
    assert_eq!(f.p.size_dependents(s).len(), 1);
    f.p.apply(Command::RemoveSize { size: s }).unwrap();
}

#[test]
fn supprimer_une_gamme_retire_ses_ecarts() {
    let mut f = five_sizes();
    let range = f.p.sizes[0].id;
    f.p.apply(Command::RemoveSizeRange { range }).unwrap();
    assert!(f.p.sheet(f.sheet).unwrap().gradings.is_empty());
    assert!(!bytes(&f.p).contains("gradings"));
}

#[test]
fn refus_sans_trace_et_sans_effet_sans_etape() {
    let mut f = five_sizes();
    let s = size(&f.p, "S");
    let m = size(&f.p, "M");
    let before = bytes(&f.p);
    let ghost = |p: u64| PointId(p);
    let cmd = |point: PointId, size: SizeId, dx: f64, dy: f64| Command::SetPointGrading {
        sheet: f.sheet,
        point,
        size,
        dx_mm: dx,
        dy_mm: dy,
    };
    let cases: Vec<(&str, Command)> = vec![
        ("taille inconnue", cmd(f.point, SizeId(987_654), 1.0, 1.0)),
        ("point inconnu", cmd(ghost(987_654), s, 1.0, 1.0)),
        ("NaN", cmd(f.point, s, f64::NAN, 0.0)),
        ("infini", cmd(f.point, s, 0.0, f64::INFINITY)),
        ("hors bornes", cmd(f.point, s, 2.0e6, 0.0)),
        ("hors bornes négatif", cmd(f.point, s, 0.0, -1_000_000.5)),
        ("écart sur la base", cmd(f.point, m, 1.0, 1.0)),
        (
            "hors espace de travail",
            cmd(f.point, s, 999_999.0, 999_999.0),
        ),
        (
            "feuille inconnue",
            Command::SetPointGrading {
                sheet: SheetId(987_654),
                point: f.point,
                size: s,
                dx_mm: 1.0,
                dy_mm: 1.0,
            },
        ),
        (
            "effacer : point inconnu",
            Command::ClearPointGrading {
                sheet: f.sheet,
                point: ghost(987_654),
            },
        ),
    ];
    for (name, c) in cases {
        let err = f.p.apply(c.clone()).expect_err(name);
        assert!(!err.to_string().is_empty(), "{name}");
        assert_eq!(bytes(&f.p), before, "{name} : refus avec trace");
        assert!(
            f.p.check_grading_command(&c).is_err(),
            "{name} : contrôle typé"
        );
    }
    // Sans effet : même valeur, ou écart nul sur une taille qui n'en a pas : aucune étape.
    let mut doc = Document::new(f.p.clone());
    doc.execute(cmd(f.point, s, -5.0, -10.0)).unwrap();
    assert!(!doc.can_undo(), "même valeur = aucune étape");
    doc.execute(cmd(f.point, size(&f.p, "XS"), -9.0, -20.0))
        .unwrap();
    assert!(!doc.can_undo());
    let mut q = Project::new("vide");
    let qs = q.sheets[0].id;
    let qp = q
        .apply(Command::AddPoint {
            sheet: qs,
            at: PointMm::new(1.0, 1.0),
        })
        .unwrap()
        .points[0];
    let mut doc = Document::new(q);
    doc.execute(Command::ClearPointGrading {
        sheet: qs,
        point: qp,
    })
    .unwrap();
    assert!(!doc.can_undo(), "rien à effacer = aucune étape");
}

#[test]
fn poser_un_ecart_nul_retire_l_entree_et_effacer_retire_tout() {
    let mut f = five_sizes();
    set(&mut f, "S", 0.0, 0.0);
    assert_eq!(f.p.sheet(f.sheet).unwrap().gradings.len(), 3);
    close(
        at(&f.p, f.sheet, f.point, Some(size(&f.p, "S"))),
        (100.0, 200.0),
        "S retirée = base",
    );
    // Un seul axe nul garde l'entrée.
    set(&mut f, "L", 0.0, 10.0);
    assert_eq!(f.p.sheet(f.sheet).unwrap().gradings.len(), 3);
    f.p.apply(Command::ClearPointGrading {
        sheet: f.sheet,
        point: f.point,
    })
    .unwrap();
    assert!(f.p.sheet(f.sheet).unwrap().gradings.is_empty());
    assert!(!bytes(&f.p).contains("gradings"));
}

#[test]
fn un_projet_sans_gradation_est_strictement_identique() {
    let f = fixture();
    let text = bytes(&f.p);
    assert!(!text.contains("gradings"));
    let back = persist::from_json(&text).unwrap();
    assert_eq!(bytes(&back), text);
    // L'évaluation d'une taille sans écart = la géométrie de base.
    for s in &f.p.sizes[0].sizes {
        close(
            at(&f.p, f.sheet, f.point, Some(s.id)),
            (100.0, 200.0),
            &s.label,
        );
    }
}

#[test]
fn un_point_supprime_ou_devenu_construit_perd_ses_ecarts_avec_remarque() {
    let mut f = five_sizes();
    let out =
        f.p.apply(Command::Delete {
            sheet: f.sheet,
            points: vec![f.point],
            entities: vec![],
        })
        .unwrap();
    assert!(
        out.notes.iter().any(|n| n.contains("gradation")),
        "{:?}",
        out.notes
    );
    assert!(f.p.sheet(f.sheet).unwrap().gradings.is_empty());
}

#[test]
fn copie_de_feuille_garde_les_ecarts_import_et_presse_papiers_les_signalent() {
    let mut f = five_sizes();
    let out =
        f.p.apply(Command::DuplicateSheet { sheet: f.sheet })
            .unwrap();
    let copy_id = out.sheet.unwrap();
    let copy = f.p.sheet(copy_id).unwrap();
    assert_eq!(copy.gradings.len(), 4);
    let new_point = copy.points[0].id;
    assert_ne!(new_point, f.point);
    assert!(copy.gradings.iter().all(|g| g.point == new_point));
    for (label, expected) in labelled(&f) {
        close(
            at(&f.p, copy_id, new_point, Some(size(&f.p, label))),
            expected,
            label,
        );
    }
    // Import dans un autre projet : les tailles n'existent pas, les écarts sont écartés (remarque).
    let mut other = Project::new("Autre");
    let import = f.p.import_command(f.sheet).unwrap();
    let out = other.apply(import).unwrap();
    assert!(
        out.notes.iter().any(|n| n.contains("gradation")),
        "{:?}",
        out.notes
    );
    assert!(other.sheets.iter().all(|s| s.gradings.is_empty()));
    // Presse-papiers : position de base copiée, remarque.
    let frag =
        f.p.copy_fragment(f.sheet, &[f.point], &[], &[], &[])
            .unwrap();
    assert!(
        frag.notes.iter().any(|n| n.contains("gradation")),
        "{:?}",
        frag.notes
    );
}

#[test]
fn plusieurs_gammes_une_taille_est_une_gamme_et_la_base_commune_est_protegee() {
    let mut f = fixture();
    f.p.apply(Command::AddSizeRange {
        name: "Tour".into(),
        kind: SizeKind::Numeric,
        labels: labels(&["34", "36", "38"]),
        base: 0,
    })
    .unwrap();
    set(&mut f, "L", 5.0, 10.0);
    set(&mut f, "38", 0.0, 8.0);
    close(
        at(&f.p, f.sheet, f.point, Some(size(&f.p, "38"))),
        (100.0, 208.0),
        "38 (autre gamme)",
    );
    close(
        at(&f.p, f.sheet, f.point, Some(size(&f.p, "L"))),
        (105.0, 210.0),
        "L",
    );
    // La nouvelle base L porte un écart et le projet a deux gammes : refusé, sans trace.
    let before = bytes(&f.p);
    let l = size(&f.p, "L");
    let err = f.p.apply(Command::SetBaseSize { size: l }).unwrap_err();
    assert!(err.to_string().contains("plusieurs"), "{err}");
    assert_eq!(bytes(&f.p), before);
    assert!(matches!(
        f.p.check_size_command(&Command::SetBaseSize { size: l }),
        Err(SizeError::BaseShiftsOtherRanges { .. })
    ));
    // Une base sans écart se change sans déplacer personne.
    let xs = size(&f.p, "XS");
    f.p.apply(Command::SetBaseSize { size: xs }).unwrap();
    close(
        at(&f.p, f.sheet, f.point, Some(size(&f.p, "L"))),
        (105.0, 210.0),
        "L inchangée",
    );
}

#[test]
fn point_suivi_par_une_construction_maintenue_refus_a_la_pose_et_conflit_diagnostique() {
    let mut f = fixture();
    let seg =
        f.p.apply(Command::AddSegmentPolar {
            sheet: f.sheet,
            from: PointRef::New(PointMm::new(0.0, 0.0)),
            length_mm: 80.0,
            angle_deg: 30.0,
        })
        .unwrap();
    let (entity, to) = (seg.entities[0], *seg.points.last().unwrap());
    // Gradué d'abord (autorisé : le point est libre et non suivi).
    f.p.apply(Command::SetPointGrading {
        sheet: f.sheet,
        point: to,
        size: size(&f.p, "L"),
        dx_mm: 4.0,
        dy_mm: 0.0,
    })
    .unwrap();
    assert!(!f
        .p
        .link_diagnostics()
        .iter()
        .any(|d| d.kind == DiagnosticKind::GradedAndFollowed));
    // Puis suivi : aucun silence, le diagnostic signale le conflit.
    f.p.apply(Command::SetConstructionKept {
        sheet: f.sheet,
        segment: entity,
        kept: true,
    })
    .unwrap();
    let conflicts: Vec<_> =
        f.p.link_diagnostics()
            .into_iter()
            .filter(|d| d.kind == DiagnosticKind::GradedAndFollowed)
            .collect();
    assert_eq!(conflicts.len(), 1);
    assert_eq!(conflicts[0].link, LinkType::PointGrading);
    assert_eq!(
        conflicts[0].primary(),
        Some(&pf_core::DiagnosticObject::Point(to))
    );
    // Une nouvelle pose est refusée ; l'écart nul (lever le conflit) passe toujours.
    let before = bytes(&f.p);
    let err =
        f.p.apply(Command::SetPointGrading {
            sheet: f.sheet,
            point: to,
            size: size(&f.p, "S"),
            dx_mm: 1.0,
            dy_mm: 1.0,
        })
        .unwrap_err();
    assert!(err.to_string().contains("construction maintenue"), "{err}");
    assert_eq!(bytes(&f.p), before);
    f.p.apply(Command::ClearPointGrading {
        sheet: f.sheet,
        point: to,
    })
    .unwrap();
    assert!(f
        .p
        .link_diagnostics()
        .iter()
        .all(|d| d.kind != DiagnosticKind::GradedAndFollowed));
}

#[test]
fn changer_la_base_est_refuse_si_la_feuille_a_des_liens_maintenus() {
    let mut f = fixture();
    let seg =
        f.p.apply(Command::AddSegmentPolar {
            sheet: f.sheet,
            from: PointRef::New(PointMm::new(0.0, 0.0)),
            length_mm: 80.0,
            angle_deg: 30.0,
        })
        .unwrap();
    f.p.apply(Command::SetConstructionKept {
        sheet: f.sheet,
        segment: seg.entities[0],
        kept: true,
    })
    .unwrap();
    set(&mut f, "L", 5.0, 10.0);
    let before = bytes(&f.p);
    let l = size(&f.p, "L");
    let err = f.p.apply(Command::SetBaseSize { size: l }).unwrap_err();
    assert!(matches!(err, CoreError::Invalid(_)));
    assert!(err.to_string().contains("liens maintenus"), "{err}");
    assert_eq!(bytes(&f.p), before);
    assert!(matches!(
        f.p.check_size_command(&Command::SetBaseSize { size: l }),
        Err(SizeError::BaseShiftsMaintained { .. })
    ));
}

#[test]
fn le_graphe_de_dependances_relie_la_taille_au_point() {
    let f = five_sizes();
    let s = size(&f.p, "S");
    let edges = f.p.dependency_edges();
    assert!(edges.iter().any(|e| e.parent == DependencyObject::Size(s)
        && e.child == DependencyObject::Point(f.point)
        && e.nature == DependencyNature::SizeGrading
        && e.nature.key() == "size_grading"));
    assert_eq!(
        edges
            .iter()
            .filter(|e| e.nature == DependencyNature::SizeGrading)
            .count(),
        4
    );
    let tree =
        f.p.dependency_tree(&DependencyObject::Point(f.point), None)
            .unwrap();
    assert_eq!(
        tree.direct_parents()
            .filter(|e| e.nature == DependencyNature::SizeGrading)
            .count(),
        4
    );
}

#[test]
fn format_ecarts_ecrits_relus_et_clef_reservee_avant_la_v4() {
    let f = five_sizes();
    let text = bytes(&f.p);
    assert!(text.contains("\"gradings\""));
    let back = persist::from_json(&text).unwrap();
    assert_eq!(back, f.p);
    assert_eq!(bytes(&back), text);
    let mut doc: serde_json::Value = serde_json::from_str(&text).unwrap();
    doc["schema_version"] = serde_json::json!(3);
    doc.as_object_mut().unwrap().remove("sizes");
    let err = persist::from_json(&doc.to_string()).unwrap_err();
    assert!(err.to_string().contains("gradings"), "{err}");
}

#[test]
fn un_ecart_nul_ou_sur_la_base_stocke_dans_un_fichier_est_refuse() {
    let f = five_sizes();
    let text = bytes(&f.p);
    let load = |edit: &dyn Fn(&mut serde_json::Value)| {
        let mut doc: serde_json::Value = serde_json::from_str(&text).unwrap();
        edit(&mut doc);
        persist::from_json(&doc.to_string())
    };
    let m = size(&f.p, "M").0;
    assert!(load(&|d| d["sheets"][0]["gradings"][0]["dx_mm"] = serde_json::json!(1.0)).is_ok());
    assert!(load(&|d| {
        d["sheets"][0]["gradings"][0]["dx_mm"] = serde_json::json!(0.0);
        d["sheets"][0]["gradings"][0]["dy_mm"] = serde_json::json!(0.0);
    })
    .is_err());
    assert!(load(&|d| d["sheets"][0]["gradings"][0]["size"] = serde_json::json!(m)).is_err());
    assert!(load(&|d| d["sheets"][0]["gradings"][0]["size"] = serde_json::json!(987_654)).is_err());
    assert!(
        load(&|d| d["sheets"][0]["gradings"][0]["point"] = serde_json::json!(987_654)).is_err()
    );
    assert!(load(&|d| d["sheets"][0]["gradings"][0]["dy_mm"] = serde_json::json!(2.0e6)).is_err());
    assert!(load(&|d| {
        let first = d["sheets"][0]["gradings"][0].clone();
        d["sheets"][0]["gradings"]
            .as_array_mut()
            .unwrap()
            .push(first);
    })
    .is_err());
    assert!(load(&|d| d["sheets"][0]["gradings"][0]["zz"] = serde_json::json!(1)).is_err());
}

#[test]
fn transformer_une_translation_laisse_les_ecarts_une_rotation_ou_une_symetrie_les_suit() {
    use pf_geometry::Transform;
    // Translation : la position de base bouge, les écarts restent (S = −5 ; −10).
    let mut f = five_sizes();
    f.p.apply(Command::Transform {
        sheet: f.sheet,
        points: vec![f.point],
        transform: Transform::Translate {
            dx_mm: 10.0,
            dy_mm: 20.0,
        },
    })
    .unwrap();
    close(
        at(&f.p, f.sheet, f.point, Some(size(&f.p, "S"))),
        (105.0, 210.0),
        "S après translation",
    );
    // Rotation de 90° autour de l'origine : P (100 ; 200) → (−200 ; 100), écart S (−5 ; −10) →
    // (10 ; −5) : S est en (−190 ; 95), XL (10 ; 22) → (−22 ; 10) donc (−222 ; 110).
    let mut f = five_sizes();
    let out =
        f.p.apply(Command::Transform {
            sheet: f.sheet,
            points: vec![f.point],
            transform: Transform::Rotate {
                center: PointMm::new(0.0, 0.0),
                angle_deg: 90.0,
            },
        })
        .unwrap();
    assert!(
        out.notes.iter().any(|n| n.contains("gradation")),
        "{:?}",
        out.notes
    );
    close(at(&f.p, f.sheet, f.point, None), (-200.0, 100.0), "base");
    close(
        at(&f.p, f.sheet, f.point, Some(size(&f.p, "S"))),
        (-190.0, 95.0),
        "S",
    );
    close(
        at(&f.p, f.sheet, f.point, Some(size(&f.p, "XL"))),
        (-222.0, 110.0),
        "XL",
    );
    // Symétrie par rapport à l'axe Y (x → −x) : P (100 ; 200) → (−100 ; 200), écart S → (5 ; −10).
    let mut f = five_sizes();
    f.p.apply(Command::Transform {
        sheet: f.sheet,
        points: vec![f.point],
        transform: Transform::Mirror {
            axis_point: PointMm::new(0.0, 0.0),
            axis_angle_deg: 90.0,
        },
    })
    .unwrap();
    close(
        at(&f.p, f.sheet, f.point, Some(size(&f.p, "S"))),
        (-95.0, 190.0),
        "S symétrisé",
    );
}

#[test]
fn dupliquer_signale_que_les_copies_sont_a_la_position_de_base() {
    use pf_geometry::Transform;
    let mut f = five_sizes();
    let seg =
        f.p.apply(Command::AddSegment {
            sheet: f.sheet,
            from: PointRef::Existing(f.point),
            to: PointRef::New(PointMm::new(300.0, 200.0)),
        })
        .unwrap()
        .entities[0];
    let out =
        f.p.apply(Command::Duplicate {
            sheet: f.sheet,
            entities: vec![seg],
            transform: Transform::Translate {
                dx_mm: 0.0,
                dy_mm: 50.0,
            },
        })
        .unwrap();
    assert!(
        out.notes.iter().any(|n| n.contains("gradu")),
        "{:?}",
        out.notes
    );
    // Les écarts du point d'origine sont intacts ; les copies n'en ont pas.
    assert_eq!(f.p.sheet(f.sheet).unwrap().gradings.len(), 4);
    assert!(f
        .p
        .sheet(f.sheet)
        .unwrap()
        .gradings
        .iter()
        .all(|g| g.point == f.point));
}
