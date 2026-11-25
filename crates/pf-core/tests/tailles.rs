//! US-09.1 — tailles du modèle : gammes, taille de base, tailles de rupture, commandes, API
//! `evaluate_sheet`, format. Valeurs écrites à la main (gamme XS S M L XL, base M…).

use pf_core::{
    persist, Command, CoreError, Document, PointRef, Project, SizeError, SizeId, SizeKind,
    SizeRangeId, TextProblem,
};
use pf_geometry::PointMm;

fn labels(l: &[&str]) -> Vec<String> {
    l.iter().map(|s| s.to_string()).collect()
}

/// Projet avec un segment, un point construit à distance et une pièce : de la vraie géométrie.
fn geometry() -> Project {
    let mut p = Project::new("Tailles");
    let sheet = p.sheets[0].id;
    p.apply(Command::AddSegment {
        sheet,
        from: PointRef::New(PointMm::new(10.0, 20.0)),
        to: PointRef::New(PointMm::new(110.0, 20.0)),
    })
    .unwrap();
    p.apply(Command::AddSegmentPolar {
        sheet,
        from: PointRef::New(PointMm::new(0.0, 0.0)),
        length_mm: 80.0,
        angle_deg: 30.0,
    })
    .unwrap();
    p
}

fn alpha(p: &mut Project) -> SizeRangeId {
    p.apply(Command::AddSizeRange {
        name: "Femme".into(),
        kind: SizeKind::Alphanumeric,
        labels: labels(&["XS", "S", "M", "L", "XL"]),
        base: 2,
    })
    .unwrap()
    .size_ranges[0]
}

fn size_of(p: &Project, label: &str) -> SizeId {
    p.sizes
        .iter()
        .flat_map(|r| r.sizes.iter())
        .find(|s| s.label == label)
        .unwrap_or_else(|| panic!("taille {label}"))
        .id
}

fn labels_of(p: &Project, range: usize) -> Vec<String> {
    p.sizes[range]
        .sizes
        .iter()
        .map(|s| s.label.clone())
        .collect()
}

fn base_label(p: &Project, range: usize) -> String {
    p.sizes[range].base_size().unwrap().label.clone()
}

fn json(p: &Project) -> String {
    persist::to_json(p).unwrap()
}

/// Le projet sans ses gammes (pour comparer « tout le reste »).
fn without_sizes(p: &Project) -> Project {
    let mut q = p.clone();
    q.sizes.clear();
    q
}

#[test]
fn gamme_alphanumerique_xs_xl_base_m() {
    let mut p = geometry();
    let before = p.next_id;
    let out = p
        .apply(Command::AddSizeRange {
            name: "Femme".into(),
            kind: SizeKind::Alphanumeric,
            labels: labels(&["XS", "S", "M", "L", "XL"]),
            base: 2,
        })
        .unwrap();
    assert_eq!(p.sizes.len(), 1);
    let r = &p.sizes[0];
    assert_eq!(r.name, "Femme");
    assert_eq!(r.kind, SizeKind::Alphanumeric);
    assert_eq!(labels_of(&p, 0), ["XS", "S", "M", "L", "XL"]);
    assert_eq!(base_label(&p, 0), "M");
    assert!(r.sizes.iter().all(|s| !s.break_size));
    // 1 gamme + 5 tailles = 6 identifiants consécutifs, jamais réutilisés.
    assert_eq!(p.next_id, before + 6);
    assert_eq!(out.size_ranges, vec![r.id]);
    assert_eq!(out.sizes, r.sizes.iter().map(|s| s.id).collect::<Vec<_>>());
    let mut ids: Vec<u64> = std::iter::once(r.id.0)
        .chain(r.sizes.iter().map(|s| s.id.0))
        .collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), 6);
}

#[test]
fn ca_retirer_une_taille_ne_detruit_rien() {
    let mut p = geometry();
    alpha(&mut p);
    let s = size_of(&p, "S");
    let before = p.clone();
    assert!(p.size_dependents(s).is_empty());
    p.apply(Command::RemoveSize { size: s }).unwrap();
    assert_eq!(labels_of(&p, 0), ["XS", "M", "L", "XL"]);
    assert_eq!(base_label(&p, 0), "M");
    // Rien d'autre n'a bougé : même projet à la liste de tailles près, même compteur d'ids.
    let mut expected = before.clone();
    expected.sizes[0].sizes.retain(|x| x.id != s);
    assert_eq!(p, expected);
    assert_eq!(p.next_id, before.next_id);
    // Les tailles restantes gardent leurs identifiants.
    assert_eq!(
        p.sizes[0].sizes.iter().map(|x| x.id).collect::<Vec<_>>(),
        before.sizes[0]
            .sizes
            .iter()
            .filter(|x| x.id != s)
            .map(|x| x.id)
            .collect::<Vec<_>>()
    );
}

#[test]
fn ca_ajouter_une_taille_ne_detruit_rien() {
    let mut p = geometry();
    alpha(&mut p);
    let before = p.clone();
    let range = p.sizes[0].id;
    let out = p
        .apply(Command::AddSize {
            range,
            label: "2XL".into(),
            at: None,
        })
        .unwrap();
    assert_eq!(labels_of(&p, 0), ["XS", "S", "M", "L", "XL", "2XL"]);
    assert_eq!(base_label(&p, 0), "M");
    assert_eq!(out.sizes.len(), 1);
    assert_eq!(p.sizes[0].sizes[5].id, out.sizes[0]);
    // Le reste est identique.
    let mut back = p.clone();
    back.sizes[0].sizes.pop();
    back.next_id = before.next_id;
    assert_eq!(back, before);
    // Insertion à un rang précis.
    p.apply(Command::AddSize {
        range,
        label: "XXS".into(),
        at: Some(0),
    })
    .unwrap();
    assert_eq!(labels_of(&p, 0), ["XXS", "XS", "S", "M", "L", "XL", "2XL"]);
}

#[test]
fn ca_changer_la_base_recalcul_equivalent() {
    let mut p = geometry();
    alpha(&mut p);
    let sheet = p.sheets[0].id;
    let l = size_of(&p, "L");
    let base_eval = p.evaluate_sheet(sheet, None).unwrap();
    assert!(!base_eval.points.is_empty());
    p.apply(Command::SetBaseSize { size: l }).unwrap();
    assert_eq!(base_label(&p, 0), "L");
    assert_eq!(p.sizes[0].base, l);
    // Identité des positions évaluées avant / après, pour la taille de base et pour toute taille.
    assert_eq!(p.evaluate_sheet(sheet, None).unwrap(), base_eval);
    for s in p.sizes[0].sizes.iter().map(|s| s.id) {
        let e = p.evaluate_sheet(sheet, Some(s)).unwrap();
        assert_eq!(e.points, base_eval.points);
    }
    // Le projet n'a changé que par la désignation de la base.
    let mut q = p.clone();
    q.sizes[0].base = size_of(&q, "M");
    assert_eq!(base_label(&q, 0), "M");
    assert_eq!(q.sheets, geometry().sheets);
    assert_eq!(without_sizes(&q).sheets, without_sizes(&p).sheets);
}

#[test]
fn ordre_des_tailles_conserve_deplacement_et_renommage() {
    let mut p = geometry();
    alpha(&mut p);
    let xs = size_of(&p, "XS");
    p.apply(Command::MoveSize { size: xs, to: 3 }).unwrap();
    assert_eq!(labels_of(&p, 0), ["S", "M", "L", "XS", "XL"]);
    assert_eq!(base_label(&p, 0), "M");
    p.apply(Command::MoveSize { size: xs, to: 0 }).unwrap();
    assert_eq!(labels_of(&p, 0), ["XS", "S", "M", "L", "XL"]);
    // Aucun tri alphabétique : un libellé en tête reste en tête.
    let m = size_of(&p, "M");
    p.apply(Command::RenameSize {
        size: m,
        label: "Zéro".into(),
    })
    .unwrap();
    assert_eq!(labels_of(&p, 0), ["XS", "S", "Zéro", "L", "XL"]);
    assert_eq!(p.sizes[0].base, m, "renommer ne change pas l'identité");
    // Un changement de casse seul est un renommage permis.
    let l = size_of(&p, "L");
    p.apply(Command::RenameSize {
        size: l,
        label: "l".into(),
    })
    .unwrap();
    assert_eq!(labels_of(&p, 0)[3], "l");
}

#[test]
fn tailles_de_rupture() {
    let mut p = geometry();
    alpha(&mut p);
    let s = size_of(&p, "S");
    let xl = size_of(&p, "XL");
    p.apply(Command::SetSizeBreak {
        size: s,
        break_size: true,
    })
    .unwrap();
    p.apply(Command::SetSizeBreak {
        size: xl,
        break_size: true,
    })
    .unwrap();
    let marked: Vec<_> = p.sizes[0]
        .sizes
        .iter()
        .filter(|x| x.break_size)
        .map(|x| x.label.clone())
        .collect();
    assert_eq!(marked, ["S", "XL"]);
    p.apply(Command::SetSizeBreak {
        size: s,
        break_size: false,
    })
    .unwrap();
    assert_eq!(p.sizes[0].sizes.iter().filter(|x| x.break_size).count(), 1);
    // Le marqueur est écrit seulement quand il est vrai.
    let text = json(&p);
    assert_eq!(text.matches("\"break_size\"").count(), 1);
}

#[test]
fn gamme_numerique_34_a_44() {
    let mut p = geometry();
    p.apply(Command::AddSizeRange {
        name: "Numérique".into(),
        kind: SizeKind::Numeric,
        labels: labels(&["34", "36", "38", "40", "42", "44"]),
        base: 2,
    })
    .unwrap();
    assert_eq!(p.sizes[0].kind, SizeKind::Numeric);
    assert_eq!(base_label(&p, 0), "38");
    let t = size_of(&p, "34");
    p.apply(Command::RemoveSize { size: t }).unwrap();
    assert_eq!(labels_of(&p, 0), ["36", "38", "40", "42", "44"]);
    // « 10 » après « 9 » : aucun ordre numérique ni alphabétique n'est imposé.
    let range = p.sizes[0].id;
    p.apply(Command::AddSize {
        range,
        label: "9".into(),
        at: Some(0),
    })
    .unwrap();
    assert_eq!(labels_of(&p, 0)[0], "9");
    let text = json(&p);
    assert!(text.contains("\"kind\": \"numeric\""));
    assert_eq!(persist::from_json(&text).unwrap(), p);
}

#[test]
fn deux_gammes_independantes_stature_et_tour() {
    let mut p = geometry();
    let stature = p
        .apply(Command::AddSizeRange {
            name: "Stature".into(),
            kind: SizeKind::Numeric,
            labels: labels(&["160", "166", "172"]),
            base: 1,
        })
        .unwrap()
        .size_ranges[0];
    let tour = p
        .apply(Command::AddSizeRange {
            name: "Tour".into(),
            kind: SizeKind::Alphanumeric,
            labels: labels(&["S", "M", "L"]),
            base: 0,
        })
        .unwrap()
        .size_ranges[0];
    assert_eq!(p.sizes.len(), 2);
    assert_eq!(base_label(&p, 0), "166");
    assert_eq!(base_label(&p, 1), "S");
    let tour_before = p.sizes[1].clone();
    // Toucher la stature ne touche pas le tour.
    let t166 = size_of(&p, "166");
    let t160 = size_of(&p, "160");
    p.apply(Command::SetBaseSize { size: t160 }).unwrap();
    p.apply(Command::RemoveSize { size: t166 }).unwrap();
    p.apply(Command::AddSize {
        range: stature,
        label: "178".into(),
        at: None,
    })
    .unwrap();
    assert_eq!(p.sizes[1], tour_before);
    assert_eq!(labels_of(&p, 0), ["160", "172", "178"]);
    // Un même libellé dans deux gammes est permis (« S » existe dans le tour seulement ici,
    // mais « 160 » pourrait l'être ailleurs) ; les identifiants, eux, sont distincts.
    p.apply(Command::AddSize {
        range: tour,
        label: "160".into(),
        at: None,
    })
    .unwrap();
    let mut ids: Vec<u64> = p
        .sizes
        .iter()
        .flat_map(|r| std::iter::once(r.id.0).chain(r.sizes.iter().map(|s| s.id.0)))
        .collect();
    let n = ids.len();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), n);
    // Supprimer une gamme n'emporte pas l'autre.
    p.apply(Command::RemoveSizeRange { range: stature })
        .unwrap();
    assert_eq!(p.sizes.len(), 1);
    assert_eq!(p.sizes[0].id, tour);
}

#[test]
fn renommer_et_supprimer_une_gamme() {
    let mut p = geometry();
    let r = alpha(&mut p);
    p.apply(Command::RenameSizeRange {
        range: r,
        name: "  Homme ".into(),
    })
    .unwrap();
    assert_eq!(p.sizes[0].name, "Homme");
    p.apply(Command::RemoveSizeRange { range: r }).unwrap();
    assert!(p.sizes.is_empty());
    assert!(!json(&p).contains("\"sizes\""));
}

/// Commandes refusées : message explicite, projet identique à l'octet, aucune étape d'historique.
#[test]
fn refus_sans_trace() {
    let mut doc = Document::new(geometry());
    let range = doc
        .execute(Command::AddSizeRange {
            name: "Femme".into(),
            kind: SizeKind::Alphanumeric,
            labels: labels(&["XS", "S", "M"]),
            base: 1,
        })
        .unwrap()
        .size_ranges[0];
    let p = doc.project().clone();
    let (xs, s, m) = (size_of(&p, "XS"), size_of(&p, "S"), size_of(&p, "M"));
    let ghost_size = SizeId(9999);
    let ghost_range = SizeRangeId(9999);
    let cases: Vec<(&str, Command)> = vec![
        ("retirer la base", Command::RemoveSize { size: s }),
        (
            "nom de gamme vide",
            Command::RenameSizeRange {
                range,
                name: "  ".into(),
            },
        ),
        (
            "gamme inconnue",
            Command::RemoveSizeRange { range: ghost_range },
        ),
        ("taille inconnue", Command::SetBaseSize { size: ghost_size }),
        (
            "libellé vide",
            Command::AddSize {
                range,
                label: "".into(),
                at: None,
            },
        ),
        (
            "libellé en double (casse ignorée)",
            Command::AddSize {
                range,
                label: "xs".into(),
                at: None,
            },
        ),
        (
            "rang hors gamme",
            Command::AddSize {
                range,
                label: "XL".into(),
                at: Some(9),
            },
        ),
        (
            "déplacement hors gamme",
            Command::MoveSize { size: xs, to: 3 },
        ),
        (
            "renommage en double",
            Command::RenameSize {
                size: m,
                label: "XS".into(),
            },
        ),
        (
            "création sans taille",
            Command::AddSizeRange {
                name: "Vide".into(),
                kind: SizeKind::Numeric,
                labels: vec![],
                base: 0,
            },
        ),
        (
            "base hors liste",
            Command::AddSizeRange {
                name: "Autre".into(),
                kind: SizeKind::Numeric,
                labels: labels(&["1", "2"]),
                base: 2,
            },
        ),
        (
            "gamme en double (casse ignorée)",
            Command::AddSizeRange {
                name: "FEMME".into(),
                kind: SizeKind::Numeric,
                labels: labels(&["1"]),
                base: 0,
            },
        ),
        (
            "libellés en double à la création",
            Command::AddSizeRange {
                name: "Autre".into(),
                kind: SizeKind::Numeric,
                labels: labels(&["1", " 1"]),
                base: 0,
            },
        ),
        (
            "libellé trop long",
            Command::AddSize {
                range,
                label: "x".repeat(21),
                at: None,
            },
        ),
        (
            "libellé avec caractère de contrôle",
            Command::AddSize {
                range,
                label: "a\tb".into(),
                at: None,
            },
        ),
        (
            "marquer une taille inconnue",
            Command::SetSizeBreak {
                size: ghost_size,
                break_size: true,
            },
        ),
    ];
    let bytes = json(doc.project());
    for (name, cmd) in cases {
        let err = doc.execute(cmd.clone()).expect_err(name);
        assert!(!err.to_string().is_empty(), "{name}");
        assert_eq!(json(doc.project()), bytes, "{name} : octets modifiés");
        assert!(doc.can_undo(), "l'ajout initial reste annulable");
        // Le contrôle typé de l'interface voit le même refus.
        assert!(doc.project().check_size_command(&cmd).is_err(), "{name}");
    }
    // Une seule étape d'historique : celle de la création de la gamme.
    assert!(doc.undo());
    assert!(!doc.can_undo(), "aucune trace des refus");
}

#[test]
fn refus_typés_pour_l_interface() {
    let mut p = geometry();
    let r = alpha(&mut p);
    let m = size_of(&p, "M");
    let e = p.check_size_command(&Command::RemoveSize { size: m });
    assert_eq!(e, Err(SizeError::CannotRemoveBase { label: "M".into() }));
    let e = p.check_size_command(&Command::AddSize {
        range: r,
        label: "".into(),
        at: None,
    });
    assert_eq!(
        e,
        Err(SizeError::SizeLabel {
            label: "".into(),
            problem: TextProblem::Empty
        })
    );
    // Dernière taille : on supprime la gamme à la place.
    let mut q = geometry();
    q.apply(Command::AddSizeRange {
        name: "Unique".into(),
        kind: SizeKind::Numeric,
        labels: labels(&["1"]),
        base: 0,
    })
    .unwrap();
    let only = q.sizes[0].sizes[0].id;
    assert_eq!(
        q.check_size_command(&Command::RemoveSize { size: only }),
        Err(SizeError::CannotRemoveLastSize)
    );
    // Une commande étrangère n'est pas un refus de tailles.
    assert_eq!(
        p.check_size_command(&Command::RenameProject { name: "x".into() }),
        Err(SizeError::NotASizeCommand)
    );
}

#[test]
fn retirer_la_base_apres_avoir_designe_une_autre() {
    let mut p = geometry();
    alpha(&mut p);
    let m = size_of(&p, "M");
    let l = size_of(&p, "L");
    assert!(p.apply(Command::RemoveSize { size: m }).is_err());
    p.apply(Command::SetBaseSize { size: l }).unwrap();
    p.apply(Command::RemoveSize { size: m }).unwrap();
    assert_eq!(labels_of(&p, 0), ["XS", "S", "L", "XL"]);
    assert_eq!(base_label(&p, 0), "L");
}

#[test]
fn sans_effet_donc_sans_etape() {
    let mut doc = Document::new(geometry());
    doc.execute(Command::AddSizeRange {
        name: "Femme".into(),
        kind: SizeKind::Alphanumeric,
        labels: labels(&["XS", "S", "M"]),
        base: 1,
    })
    .unwrap();
    // Neutraliser la pile : une seule étape (la création).
    let (xs, s) = (size_of(doc.project(), "XS"), size_of(doc.project(), "S"));
    let range = doc.project().sizes[0].id;
    let noops = vec![
        Command::RenameSizeRange {
            range,
            name: "Femme".into(),
        },
        Command::RenameSize {
            size: xs,
            label: "XS".into(),
        },
        Command::MoveSize { size: xs, to: 0 },
        Command::SetSizeBreak {
            size: xs,
            break_size: false,
        },
        Command::SetBaseSize { size: s },
    ];
    let bytes = json(doc.project());
    for c in noops {
        doc.execute(c.clone())
            .unwrap_or_else(|e| panic!("{c:?} : {e}"));
        assert_eq!(json(doc.project()), bytes);
    }
    assert!(doc.undo());
    assert!(
        !doc.can_undo(),
        "les commandes sans effet n'ont pas d'étape"
    );
}

#[test]
fn annuler_et_refaire_exacts_a_l_octet() {
    let mut doc = Document::new(geometry());
    let mut states = vec![json(doc.project())];
    let cmds = |p: &Project| -> Vec<Command> {
        let r = p.sizes[0].id;
        let (xs, m, l) = (size_of(p, "XS"), size_of(p, "M"), size_of(p, "L"));
        vec![
            Command::AddSize {
                range: r,
                label: "2XL".into(),
                at: None,
            },
            Command::SetBaseSize { size: l },
            Command::RemoveSize { size: m },
            Command::SetSizeBreak {
                size: xs,
                break_size: true,
            },
            Command::MoveSize { size: xs, to: 2 },
            Command::RenameSizeRange {
                range: r,
                name: "Dames".into(),
            },
        ]
    };
    doc.execute(Command::AddSizeRange {
        name: "Femme".into(),
        kind: SizeKind::Alphanumeric,
        labels: labels(&["XS", "S", "M", "L", "XL"]),
        base: 2,
    })
    .unwrap();
    states.push(json(doc.project()));
    for c in cmds(&doc.project().clone()) {
        doc.execute(c).unwrap();
        states.push(json(doc.project()));
    }
    for i in (0..states.len() - 1).rev() {
        assert!(doc.undo());
        assert_eq!(json(doc.project()), states[i], "annuler jusqu'à {i}");
    }
    assert!(!doc.can_undo());
    for state in states.iter().skip(1) {
        assert!(doc.redo());
        assert_eq!(&json(doc.project()), state);
    }
    assert!(!doc.can_redo());
}

#[test]
fn evaluer_une_feuille_taille_inconnue_et_base() {
    let mut p = geometry();
    alpha(&mut p);
    let sheet = p.sheets[0].id;
    let none = p.evaluate_sheet(sheet, None).unwrap();
    assert_eq!(none.points.len(), p.sheets[0].points.len());
    for (id, pos) in &none.points {
        assert_eq!(*pos, p.sheets[0].position(*id).unwrap());
    }
    let xl = size_of(&p, "XL");
    assert_eq!(
        p.evaluate_sheet(sheet, Some(xl)).unwrap().points,
        none.points
    );
    let err = p.evaluate_sheet(sheet, Some(SizeId(424_242))).unwrap_err();
    assert!(err.to_string().contains("taille inconnue"), "{err}");
    // Feuille inconnue : l'erreur habituelle.
    assert!(matches!(
        p.evaluate_sheet(pf_core::SheetId(777), None),
        Err(CoreError::UnknownSheet(_))
    ));
    // Sans aucune gamme, `None` fonctionne et toute taille est inconnue.
    let q = geometry();
    assert!(q.evaluate_sheet(q.sheets[0].id, None).is_ok());
    assert!(q.evaluate_sheet(q.sheets[0].id, Some(SizeId(1))).is_err());
}

#[test]
fn limites() {
    let mut p = geometry();
    for i in 0..pf_core::MAX_SIZE_RANGES {
        p.apply(Command::AddSizeRange {
            name: format!("G{i}"),
            kind: SizeKind::Numeric,
            labels: labels(&["1"]),
            base: 0,
        })
        .unwrap();
    }
    let before = json(&p);
    assert!(p
        .apply(Command::AddSizeRange {
            name: "De trop".into(),
            kind: SizeKind::Numeric,
            labels: labels(&["1"]),
            base: 0,
        })
        .is_err());
    assert_eq!(json(&p), before);
    let range = p.sizes[0].id;
    for i in 1..pf_core::MAX_SIZES_PER_RANGE {
        p.apply(Command::AddSize {
            range,
            label: format!("{}", i + 1),
            at: None,
        })
        .unwrap();
    }
    assert!(p
        .apply(Command::AddSize {
            range,
            label: "trop".into(),
            at: None
        })
        .is_err());
}

// ---------------------------------------------------------------------------
// Format : écriture, relecture, validation
// ---------------------------------------------------------------------------

#[test]
fn projet_sans_gamme_identique_a_l_ancien_format_hors_version() {
    let p = geometry();
    let text = json(&p);
    assert!(!text.contains("\"sizes\""));
    assert!(text.contains("\"schema_version\": 6"));
    // Même octets que ce que l'ancienne version écrivait : seule la ligne de version change.
    let as_v3 = text.replace("\"schema_version\": 6", "\"schema_version\": 3");
    let back = persist::from_json(&as_v3).unwrap();
    assert_eq!(back, p);
    assert_eq!(json(&back), text);
}

#[test]
fn ecriture_relecture_identiques_avec_gammes() {
    let mut p = geometry();
    alpha(&mut p);
    let s = size_of(&p, "S");
    p.apply(Command::SetSizeBreak {
        size: s,
        break_size: true,
    })
    .unwrap();
    let text = json(&p);
    assert!(text.contains("\"schema_version\": 6"));
    let back = persist::from_json(&text).unwrap();
    assert_eq!(back, p);
    assert_eq!(json(&back), text);
}

fn doc_value(p: &Project) -> serde_json::Value {
    serde_json::from_str(&json(p)).unwrap()
}

fn refused(v: &serde_json::Value, expect: &str) {
    let err = persist::from_json(&v.to_string()).expect_err(expect);
    assert!(
        err.to_string().contains(expect) || expect.is_empty(),
        "attendu « {expect} », obtenu « {err} »"
    );
}

#[test]
fn fichiers_alteres_refuses() {
    let mut p = geometry();
    alpha(&mut p);
    let good = doc_value(&p);
    assert!(persist::from_json(&good.to_string()).is_ok());

    // Identifiant de taille en double (deux tailles).
    let mut v = good.clone();
    let id = v["sizes"][0]["sizes"][0]["id"].clone();
    v["sizes"][0]["sizes"][1]["id"] = id;
    refused(&v, "identifiant en double");

    // Identifiant de taille égal à celui de la gamme.
    let mut v = good.clone();
    let rid = v["sizes"][0]["id"].clone();
    v["sizes"][0]["sizes"][1]["id"] = rid;
    refused(&v, "identifiant en double");

    // Identifiant au-delà du compteur.
    let mut v = good.clone();
    v["sizes"][0]["sizes"][1]["id"] = serde_json::json!(999_999);
    refused(&v, "incohérent");

    // Base absente de la gamme.
    let mut v = good.clone();
    v["sizes"][0]["base"] = serde_json::json!(p.sheets[0].points[0].id.0);
    refused(&v, "taille de base");

    // Base d'une autre gamme.
    let mut q = p.clone();
    q.apply(Command::AddSizeRange {
        name: "Tour".into(),
        kind: SizeKind::Numeric,
        labels: labels(&["1", "2"]),
        base: 0,
    })
    .unwrap();
    let mut v = doc_value(&q);
    let other = v["sizes"][1]["sizes"][0]["id"].clone();
    v["sizes"][0]["base"] = other;
    refused(&v, "taille de base");

    // Deux gammes de même identifiant.
    let mut v = doc_value(&q);
    let id = v["sizes"][0]["id"].clone();
    v["sizes"][1]["id"] = id;
    refused(&v, "identifiant en double");

    // Deux gammes de même nom.
    let mut v = doc_value(&q);
    let name = v["sizes"][0]["name"].clone();
    v["sizes"][1]["name"] = name;
    refused(&v, "existe déjà");

    // Libellé vide, libellé en double, libellé avec espaces.
    let mut v = good.clone();
    v["sizes"][0]["sizes"][1]["label"] = serde_json::json!("");
    refused(&v, "vide");
    let mut v = good.clone();
    v["sizes"][0]["sizes"][1]["label"] = serde_json::json!("xs");
    refused(&v, "existe déjà");
    let mut v = good.clone();
    v["sizes"][0]["sizes"][1]["label"] = serde_json::json!(" S");
    refused(&v, "espaces");

    // Gamme sans taille.
    let mut v = good.clone();
    v["sizes"][0]["sizes"] = serde_json::json!([]);
    refused(&v, "aucune taille");

    // Type inconnu, champ inconnu, champ manquant.
    let mut v = good.clone();
    v["sizes"][0]["kind"] = serde_json::json!("romain");
    refused(&v, "");
    let mut v = good.clone();
    v["sizes"][0]["sizes"][0]["zz"] = serde_json::json!(1);
    refused(&v, "");
    let mut v = good.clone();
    v["sizes"][0].as_object_mut().unwrap().remove("base");
    refused(&v, "");
    let mut v = good.clone();
    v["sizes"][0]["sizes"][0]["break_size"] = serde_json::json!("oui");
    refused(&v, "");
    let mut v = good.clone();
    v["sizes"] = serde_json::json!({});
    refused(&v, "");
}

#[test]
fn cle_sizes_refusee_avant_la_v4() {
    let mut p = geometry();
    alpha(&mut p);
    let mut v = doc_value(&p);
    v["schema_version"] = serde_json::json!(3);
    refused(&v, "réservée au format version 4");
    // Même un tableau vide : rien n'est jeté en silence.
    let mut v = doc_value(&geometry());
    v["schema_version"] = serde_json::json!(3);
    v["sizes"] = serde_json::json!([]);
    refused(&v, "réservée au format version 4");
}

#[test]
fn version_7_refusee_proprement() {
    let mut v = doc_value(&geometry());
    v["schema_version"] = serde_json::json!(7);
    let err = persist::from_json(&v.to_string()).unwrap_err();
    assert_eq!(
        err,
        CoreError::UnsupportedVersion {
            found: 7,
            supported: 6
        }
    );
    assert!(err.to_string().contains("version 7"));
}
