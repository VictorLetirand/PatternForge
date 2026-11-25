//! US-05.5 — marges par matière : jeu de marges de couture par défaut selon la catégorie de la
//! pièce. Un groupe de tests par critère : CA1 valeurs d'atelier par catégorie (calculées à la
//! main), CA2 jeu du projet modifiable, CA3 jamais de réécriture silencieuse, CA4 pièce sans
//! catégorie, CA5 refus sans trace, CA6 une étape annulable / no-op, CA7 fichier (octets, schéma 3,
//! `pf-validate`, chargeur strict).

use pf_core::persist;
use pf_core::*;
use pf_geometry::PointMm;

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

fn bytes(p: &Project) -> String {
    persist::to_json(p).unwrap()
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

/// Rectangle 100 × 60 (pièce « Devant »), sans catégorie.
fn rect() -> (Project, PieceId) {
    let mut p = Project::new("Matières");
    let s = p.sheets[0].id;
    let v = [pt(0.0, 0.0), pt(100.0, 0.0), pt(100.0, 60.0), pt(0.0, 60.0)];
    let ids: Vec<EntityId> = (0..4)
        .map(|i| {
            p.apply(Command::AddSegment {
                sheet: s,
                from: PointRef::New(v[i]),
                to: PointRef::New(v[(i + 1) % 4]),
            })
            .unwrap()
            .entities[0]
        })
        .collect();
    let piece = p
        .apply(Command::ExtractPiece {
            sheet: s,
            source: PieceSource::Paths { entities: ids },
            name: "Devant".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    (p, piece)
}

fn set_category(p: &mut Project, piece: PieceId, category: Option<PieceCategory>) {
    let c = p.piece(piece).unwrap().clone();
    p.apply(Command::SetPieceCard {
        piece,
        name: c.name,
        code: c.code,
        quantity: c.quantity,
        mirrored_pair: c.mirrored_pair,
        material: c.material,
        category,
        comment: c.comment,
    })
    .unwrap();
}

fn put_seam(p: &mut Project, piece: PieceId, mm: f64) {
    p.apply(Command::SetSeamAllowance {
        piece,
        seam: Some(SeamAllowance {
            default_mm: mm,
            ..Default::default()
        }),
    })
    .unwrap();
}

fn refused(p: &mut Project, c: Command) -> String {
    let (before, text) = (p.clone(), bytes(p));
    let err = p.apply(c).expect_err("la commande aurait dû être refusée");
    assert_eq!(*p, before, "refus sans trace (valeur)");
    assert_eq!(text, bytes(p), "refus sans trace (octets)");
    err.to_string()
}

// ---------------------------------------------------------------- CA1

/// Valeur d'atelier de la catégorie (écrite à la main) : proposée à la création de la marge, et
/// contour de coupe d'un rectangle 100 × 60 calculé à la main (coins droits : (100+2m)(60+2m),
/// périmètre 2(160+4m)).
fn ca1(category: PieceCategory, hand_mm: f64, key: &str) {
    assert_eq!(category.key(), key);
    let (mut p, piece) = rect();
    set_category(&mut p, piece, Some(category));
    assert!(p.seam_defaults.is_none(), "rien n'est écrit par défaut");
    assert_eq!(p.seam_default_for(Some(category)), hand_mm);
    assert_eq!(p.category_seam_mm(piece).unwrap(), hand_mm);
    let mm = p.category_seam_mm(piece).unwrap();
    put_seam(&mut p, piece, mm);
    let seam = p.piece(piece).unwrap().seam.clone().unwrap();
    assert_eq!(seam.default_mm, hand_mm);
    let cut = p.piece_cut_contour(piece).unwrap();
    assert!(
        near(
            cut.area_mm2,
            (100.0 + 2.0 * hand_mm) * (60.0 + 2.0 * hand_mm)
        ),
        "aire {}",
        cut.area_mm2
    );
    assert!(near(cut.perimeter_mm, 2.0 * (160.0 + 4.0 * hand_mm)));
}

#[test]
fn ca1_tissu_10_mm() {
    ca1(PieceCategory::Fabric, 10.0, "fabric");
}

#[test]
fn ca1_doublure_10_mm() {
    ca1(PieceCategory::Lining, 10.0, "lining");
}

#[test]
fn ca1_entoilage_5_mm() {
    ca1(PieceCategory::Interfacing, 5.0, "interfacing");
}

#[test]
fn ca1_cuir_6_mm() {
    ca1(PieceCategory::Leather, 6.0, "leather");
}

#[test]
fn ca1_renfort_5_mm() {
    ca1(PieceCategory::Reinforcement, 5.0, "reinforcement");
}

// ---------------------------------------------------------------- CA2

#[test]
fn ca2_le_jeu_du_projet_remplace_la_valeur_d_atelier_pour_chaque_categorie() {
    // Une valeur distincte par catégorie, écrite à la main.
    let want = [
        (PieceCategory::Fabric, 12.5),
        (PieceCategory::Lining, 8.0),
        (PieceCategory::Interfacing, 3.0),
        (PieceCategory::Leather, 0.0),
        (PieceCategory::Reinforcement, 1000.0),
    ];
    let (mut p, piece) = rect();
    p.apply(Command::SetSeamDefaults {
        defaults: Some(SeamDefaults {
            fabric_mm: 12.5,
            lining_mm: 8.0,
            interfacing_mm: 3.0,
            leather_mm: 0.0,
            reinforcement_mm: 1000.0,
        }),
    })
    .unwrap();
    for (c, mm) in want {
        assert_eq!(p.seam_default_for(Some(c)), mm, "{}", c.key());
        set_category(&mut p, piece, Some(c));
        assert_eq!(p.category_seam_mm(piece).unwrap(), mm, "{}", c.key());
    }
}

#[test]
fn ca2_modifier_une_seule_categorie_laisse_les_autres_aux_valeurs_d_atelier() {
    let (mut p, _) = rect();
    let mut d = p.seam_defaults_effective();
    d.set(PieceCategory::Leather, 7.0);
    p.apply(Command::SetSeamDefaults { defaults: Some(d) })
        .unwrap();
    assert_eq!(p.seam_default_for(Some(PieceCategory::Leather)), 7.0);
    assert_eq!(p.seam_default_for(Some(PieceCategory::Fabric)), 10.0);
    assert_eq!(p.seam_default_for(Some(PieceCategory::Lining)), 10.0);
    assert_eq!(p.seam_default_for(Some(PieceCategory::Interfacing)), 5.0);
    assert_eq!(p.seam_default_for(Some(PieceCategory::Reinforcement)), 5.0);
}

#[test]
fn ca2_remettre_les_valeurs_d_atelier_retire_le_jeu() {
    let (mut p, _) = rect();
    let original = bytes(&p);
    let mut d = SeamDefaults::builtin();
    d.fabric_mm = 11.0;
    p.apply(Command::SetSeamDefaults { defaults: Some(d) })
        .unwrap();
    assert_ne!(bytes(&p), original);
    p.apply(Command::SetSeamDefaults {
        defaults: Some(SeamDefaults::builtin()),
    })
    .unwrap();
    assert!(p.seam_defaults.is_none());
    assert_eq!(bytes(&p), original);
    p.apply(Command::SetSeamDefaults { defaults: Some(d) })
        .unwrap();
    p.apply(Command::SetSeamDefaults { defaults: None })
        .unwrap();
    assert_eq!(bytes(&p), original);
}

// ---------------------------------------------------------------- CA3

#[test]
fn ca3_modifier_le_jeu_ne_reecrit_pas_les_marges_posees() {
    let (mut p, piece) = rect();
    set_category(&mut p, piece, Some(PieceCategory::Leather));
    put_seam(&mut p, piece, 6.0);
    let (seam, area) = (
        p.piece(piece).unwrap().seam.clone(),
        p.piece_cut_contour(piece).unwrap().area_mm2,
    );
    p.apply(Command::SetSeamDefaults {
        defaults: Some(SeamDefaults {
            leather_mm: 20.0,
            ..SeamDefaults::builtin()
        }),
    })
    .unwrap();
    assert_eq!(p.piece(piece).unwrap().seam, seam, "marge posée inchangée");
    assert!(near(p.piece_cut_contour(piece).unwrap().area_mm2, area));
    assert!(near(area, (100.0 + 12.0) * (60.0 + 12.0)));
}

#[test]
fn ca3_changer_la_categorie_ne_modifie_pas_la_marge() {
    let (mut p, piece) = rect();
    set_category(&mut p, piece, Some(PieceCategory::Fabric));
    put_seam(&mut p, piece, 10.0);
    let seam = p.piece(piece).unwrap().seam.clone();
    for c in [
        Some(PieceCategory::Interfacing),
        None,
        Some(PieceCategory::Leather),
    ] {
        set_category(&mut p, piece, c);
        assert_eq!(p.piece(piece).unwrap().seam, seam);
    }
}

#[test]
fn ca3_appliquer_la_valeur_de_la_categorie_garde_les_surcharges_par_bord() {
    let (mut p, piece) = rect();
    let first = p.piece(piece).unwrap().outline[0].entity;
    p.apply(Command::SetSeamAllowance {
        piece,
        seam: Some(SeamAllowance {
            default_mm: 10.0,
            edges: vec![EdgeAllowance {
                entity: first,
                start_mm: 2.0,
                end_mm: 4.0,
            }],
            ..Default::default()
        }),
    })
    .unwrap();
    set_category(&mut p, piece, Some(PieceCategory::Interfacing));
    let mut next = p.piece(piece).unwrap().seam.clone().unwrap();
    next.default_mm = p.category_seam_mm(piece).unwrap();
    p.apply(Command::SetSeamAllowance {
        piece,
        seam: Some(next),
    })
    .unwrap();
    let s = p.piece(piece).unwrap().seam.clone().unwrap();
    assert_eq!(s.default_mm, 5.0);
    assert_eq!(s.edges.len(), 1);
    assert_eq!((s.edges[0].start_mm, s.edges[0].end_mm), (2.0, 4.0));
}

// ---------------------------------------------------------------- CA4

#[test]
fn ca4_sans_categorie_valeur_de_repli_et_refus_expliques() {
    let (mut p, piece) = rect();
    assert_eq!(FALLBACK_SEAM_MM, 10.0);
    assert_eq!(p.seam_default_for(None), 10.0);
    // Le repli ne dépend pas du jeu.
    p.apply(Command::SetSeamDefaults {
        defaults: Some(SeamDefaults {
            fabric_mm: 3.0,
            ..SeamDefaults::builtin()
        }),
    })
    .unwrap();
    assert_eq!(p.seam_default_for(None), 10.0);
    let before = bytes(&p);
    let err = p.category_seam_mm(piece).unwrap_err().to_string();
    assert!(err.contains("catégorie") && err.contains("Devant"), "{err}");
    assert_eq!(bytes(&p), before);
    assert!(p.category_seam_mm(PieceId(999_999)).is_err());
}

// ---------------------------------------------------------------- CA5

#[test]
fn ca5_valeurs_invalides_refusees_sans_trace() {
    let (mut p, _) = rect();
    for bad in [-0.01, 1000.01, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        for c in PieceCategory::ALL {
            let mut d = SeamDefaults::builtin();
            d.set(c, bad);
            let msg = refused(&mut p, Command::SetSeamDefaults { defaults: Some(d) });
            assert!(msg.contains(c.key()) && msg.contains("1000"), "{msg}");
        }
    }
    // Un état déjà personnalisé reste intact après un refus.
    p.apply(Command::SetSeamDefaults {
        defaults: Some(SeamDefaults {
            lining_mm: 9.0,
            ..SeamDefaults::builtin()
        }),
    })
    .unwrap();
    let d = SeamDefaults {
        leather_mm: -1.0,
        ..SeamDefaults::builtin()
    };
    refused(&mut p, Command::SetSeamDefaults { defaults: Some(d) });
    assert_eq!(p.seam_default_for(Some(PieceCategory::Lining)), 9.0);
    // Les bornes sont permises.
    let ok = SeamDefaults {
        fabric_mm: 0.0,
        lining_mm: 1000.0,
        ..SeamDefaults::builtin()
    };
    p.apply(Command::SetSeamDefaults { defaults: Some(ok) })
        .unwrap();
}

#[test]
fn ca5_zero_negatif_ecrase() {
    let (mut p, _) = rect();
    p.apply(Command::SetSeamDefaults {
        defaults: Some(SeamDefaults {
            fabric_mm: -0.0,
            ..SeamDefaults::builtin()
        }),
    })
    .unwrap();
    assert!(p.seam_defaults.unwrap().fabric_mm.is_sign_positive());
    assert!(bytes(&p).contains("\"fabric_mm\": 0.0"));
}

// ---------------------------------------------------------------- CA6

#[test]
fn ca6_une_seule_annulation_et_no_op_sans_etape() {
    let (p, _) = rect();
    let mut doc = Document::new(p);
    let before = bytes(doc.project());
    let d = SeamDefaults {
        leather_mm: 8.0,
        ..SeamDefaults::builtin()
    };
    doc.execute(Command::SetSeamDefaults { defaults: Some(d) })
        .unwrap();
    let after = bytes(doc.project());
    assert_ne!(before, after);
    assert!(doc.undo());
    assert_eq!(bytes(doc.project()), before);
    assert!(!doc.can_undo(), "une seule étape");
    assert!(doc.redo());
    assert_eq!(bytes(doc.project()), after);
    // Les mêmes valeurs ne créent aucune étape.
    doc.execute(Command::SetSeamDefaults { defaults: Some(d) })
        .unwrap();
    assert!(doc.undo());
    assert!(!doc.can_undo());
    // Les valeurs d'atelier sur un projet sans jeu : no-op aussi.
    doc.execute(Command::SetSeamDefaults {
        defaults: Some(SeamDefaults::builtin()),
    })
    .unwrap();
    assert!(!doc.can_undo());
}

// ---------------------------------------------------------------- CA7

#[test]
fn ca7_fichier_sans_jeu_identique_et_aller_retour_avec_jeu() {
    let (mut p, piece) = rect();
    let without = bytes(&p);
    assert!(!without.contains("seam_defaults"));
    set_category(&mut p, piece, Some(PieceCategory::Leather));
    let d = SeamDefaults {
        fabric_mm: 12.0,
        lining_mm: 9.5,
        interfacing_mm: 4.0,
        leather_mm: 7.25,
        reinforcement_mm: 0.0,
    };
    p.apply(Command::SetSeamDefaults { defaults: Some(d) })
        .unwrap();
    let text = bytes(&p);
    assert!(text.contains("\"seam_defaults\""));
    assert!(text.contains("\"leather_mm\": 7.25"));
    assert!(text.contains("\"schema_version\": 6"));
    let back = persist::from_json(&text).unwrap();
    assert_eq!(back, p);
    assert_eq!(bytes(&back), text);
    assert_eq!(back.seam_default_for(Some(PieceCategory::Leather)), 7.25);
    // pf-validate : 0 = valide.
    let dir = std::env::temp_dir().join(format!("pf-matieres-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("matieres.pforge.json");
    std::fs::write(&file, &text).unwrap();
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_pf-validate"))
        .arg(&file)
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
    // Un jeu invalide dans le fichier : code 1.
    let bad = text.replace("\"leather_mm\": 7.25", "\"leather_mm\": 1001.0");
    std::fs::write(&file, bad).unwrap();
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_pf-validate"))
        .arg(&file)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    std::fs::remove_dir_all(&dir).ok();
    // Retirer le jeu retire la clé.
    p.apply(Command::SetSeamDefaults { defaults: None })
        .unwrap();
    assert!(!bytes(&p).contains("seam_defaults"));
    assert_eq!(p.seam_defaults, None);
}

#[test]
fn ca7_ancien_fichier_lu_et_reecrit_a_l_identique() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples");
    let mut seen = 0;
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if !path.to_string_lossy().ends_with(".pforge.json") {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        let p = persist::from_json(&text).unwrap();
        assert!(p.seam_defaults.is_none(), "{path:?}");
        assert!(!text.contains("seam_defaults"));
        assert_eq!(bytes(&p), text, "{path:?}");
        seen += 1;
    }
    assert!(seen >= 3, "fichiers d'exemple lus : {seen}");
}

#[test]
fn ca7_chargeur_strict() {
    let (mut p, _) = rect();
    p.apply(Command::SetSeamDefaults {
        defaults: Some(SeamDefaults {
            fabric_mm: 12.0,
            ..SeamDefaults::builtin()
        }),
    })
    .unwrap();
    let base: serde_json::Value = serde_json::from_str(&bytes(&p)).unwrap();
    let load = |v: &serde_json::Value| persist::from_json(&v.to_string());
    assert!(load(&base).is_ok());
    let mutated = |f: &dyn Fn(&mut serde_json::Value)| {
        let mut v = base.clone();
        f(&mut v["seam_defaults"]);
        v
    };
    for (what, bad) in [
        ("clé inconnue", mutated(&|s| s["zz"] = 1.into())),
        (
            "valeur manquante",
            mutated(&|s| {
                s.as_object_mut().unwrap().remove("leather_mm");
            }),
        ),
        ("négatif", mutated(&|s| s["fabric_mm"] = (-1.0).into())),
        ("trop grand", mutated(&|s| s["lining_mm"] = 1000.5.into())),
        ("texte", mutated(&|s| s["leather_mm"] = "10".into())),
        (
            "null",
            mutated(&|s| s["leather_mm"] = serde_json::Value::Null),
        ),
        (
            "non canonique (valeurs d'atelier)",
            mutated(&|s| {
                *s = serde_json::to_value(SeamDefaults::builtin()).unwrap();
            }),
        ),
        ("pas un objet", mutated(&|s| *s = 10.into())),
    ] {
        assert!(load(&bad).is_err(), "{what}");
    }
}
