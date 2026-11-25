//! US-11.1 variantes (tranche 119, schéma 5 inchangé) : stockage, commandes, validation,
//! intégrité référentielle, nomenclature par variante, persistance. Campagne :
//! `adverse_variantes.rs` ; schéma et `pf-validate` : `format_ouvert.rs`.
//!
//! **Valeurs et chaînes écrites à la main**, jamais relues du code testé.
//!
//! Fixture : trois rectangles sur une feuille, « Dos » 100 × 50 (aire 5 000 mm²), « Poche »
//! 100 × 20 (2 000 mm²) et « Manche » 100 × 30 (3 000 mm², paire en miroir), tous en « Coton ».

use pf_core::persist;
use pf_core::*;
use pf_geometry::PointMm;
use serde_json::{json, Value};

fn bytes(p: &Project) -> String {
    persist::to_json(p).unwrap()
}

struct Sc {
    p: Project,
    s: SheetId,
    dos: PieceId,
    poche: PieceId,
    manche: PieceId,
}

fn rect_piece(p: &mut Project, s: SheetId, x: f64, h: f64, name: &str) -> PieceId {
    let r = p
        .apply(Command::AddRectangle {
            sheet: s,
            corner: PointRef::New(PointMm::new(x, 0.0)),
            width_mm: 100.0,
            height_mm: h,
        })
        .unwrap();
    let id = p
        .apply(Command::ExtractPiece {
            sheet: s,
            source: PieceSource::Paths {
                entities: r.entities,
            },
            name: name.into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    card(p, id, name, 1, false, Some("Coton"));
    id
}

fn card(p: &mut Project, piece: PieceId, name: &str, qty: u32, pair: bool, mat: Option<&str>) {
    p.apply(Command::SetPieceCard {
        piece,
        name: name.into(),
        code: None,
        quantity: qty,
        mirrored_pair: pair,
        material: mat.map(str::to_string),
        category: None,
        comment: None,
    })
    .unwrap();
}

fn sc() -> Sc {
    let mut p = Project::new("Variantes");
    let s = p.sheets[0].id;
    let dos = rect_piece(&mut p, s, 0.0, 50.0, "Dos");
    let poche = rect_piece(&mut p, s, 200.0, 20.0, "Poche");
    let manche = rect_piece(&mut p, s, 400.0, 30.0, "Manche");
    card(&mut p, manche, "Manche", 1, true, Some("Coton"));
    Sc {
        p,
        s,
        dos,
        poche,
        manche,
    }
}

fn add_variant(p: &mut Project, name: &str) -> VariantId {
    p.apply(Command::AddVariant { name: name.into() })
        .unwrap()
        .variants[0]
}

fn entry(
    p: &mut Project,
    v: VariantId,
    piece: PieceId,
    qty: u32,
    material: Option<&str>,
    option: Option<&str>,
) -> Result<(), CoreError> {
    p.apply(Command::AddVariantEntry {
        variant: v,
        piece,
        quantity: qty,
        material: material.map(str::to_string),
        option: option.map(str::to_string),
    })
    .map(|_| ())
}

fn option(p: &mut Project, v: VariantId, name: &str) -> Result<(), CoreError> {
    p.apply(Command::AddVariantOption {
        variant: v,
        option: name.into(),
    })
    .map(|_| ())
}

fn set_active(p: &mut Project, v: VariantId, name: &str, active: bool) {
    p.apply(Command::SetVariantOptionActive {
        variant: v,
        option: name.into(),
        active,
    })
    .unwrap();
}

/// La variante A de l'énoncé : Dos × 1, Poche × 2 (option « avec poche »), Manche × 1 en « Lin ».
fn variant_a(f: &mut Sc) -> VariantId {
    let a = add_variant(&mut f.p, "A");
    option(&mut f.p, a, "avec poche").unwrap();
    entry(&mut f.p, a, f.dos, 1, None, None).unwrap();
    entry(&mut f.p, a, f.poche, 2, None, Some("avec poche")).unwrap();
    entry(&mut f.p, a, f.manche, 1, Some("Lin"), None).unwrap();
    set_active(&mut f.p, a, "avec poche", true);
    a
}

/// Refus : message ; le projet est identique (valeur et octets).
fn refused(p: &mut Project, c: Command) -> String {
    let (before, text) = (p.clone(), bytes(p));
    let e = p.apply(c).expect_err("la commande devait être refusée");
    assert_eq!(*p, before);
    assert_eq!(bytes(p), text, "refus avec trace");
    e.to_string()
}

fn close(a: Option<f64>, b: f64) -> bool {
    a.is_some_and(|a| (a - b).abs() < 1e-6)
}

// ---------------------------------------------------------------------------
// Stockage : forme exacte, omission, aller-retour
// ---------------------------------------------------------------------------

#[test]
fn stockage_omis_quand_vide_et_forme_exacte() {
    let mut f = sc();
    assert!(!bytes(&f.p).contains("variants"));
    let a = variant_a(&mut f);
    // Variante vide : seules `id` et `name`.
    let vide = add_variant(&mut f.p, "Vide");
    let json = serde_json::to_string(&f.p.variants[1]).unwrap();
    assert_eq!(json, format!("{{\"id\":{},\"name\":\"Vide\"}}", vide.0));
    let json = serde_json::to_string(&f.p.variants[0]).unwrap();
    assert_eq!(
        json,
        format!(
            "{{\"id\":{},\"name\":\"A\",\"options\":[\"avec poche\"],\
             \"options_active\":[\"avec poche\"],\"entries\":[\
             {{\"id\":30,\"piece\":{},\"quantity\":1}},\
             {{\"id\":31,\"piece\":{},\"quantity\":2,\"option\":\"avec poche\"}},\
             {{\"id\":32,\"piece\":{},\"quantity\":1,\"material\":\"Lin\"}}]}}",
            a.0, f.dos.0, f.poche.0, f.manche.0
        )
    );
    // Ordre de sérialisation : après `sizes`... et avant les remarques de migration.
    let text = bytes(&f.p);
    assert!(text.contains("\"variants\""));
    // Aller-retour octet à octet.
    let again = persist::from_json(&text).unwrap();
    assert_eq!(again, f.p);
    assert_eq!(bytes(&again), text);
    // Retirer les variantes : la clé disparaît (le compteur d'identifiants, lui, ne recule pas).
    f.p.apply(Command::RemoveVariant { variant: a }).unwrap();
    f.p.apply(Command::RemoveVariant { variant: vide }).unwrap();
    assert!(!bytes(&f.p).contains("variants"));
    // Schéma 6 (identifiant d'entrée) : la ligne de version suit.
    assert!(bytes(&f.p).contains("\"schema_version\": 6"));
}

#[test]
fn ids_stables_sans_reutilisation() {
    let mut f = sc();
    let first = add_variant(&mut f.p, "V1");
    f.p.apply(Command::RemoveVariant { variant: first })
        .unwrap();
    let second = add_variant(&mut f.p, "V1");
    assert!(second.0 > first.0, "id de variante réutilisé");
    let copy =
        f.p.apply(Command::DuplicateVariant {
            variant: second,
            name: "V2".into(),
        })
        .unwrap()
        .variants[0];
    assert!(copy.0 > second.0);
    let mut all = vec![second.0, copy.0];
    all.extend(f.p.pieces.iter().map(|x| x.id.0));
    all.extend(f.p.sheets.iter().map(|x| x.id.0));
    let n = all.len();
    all.sort_unstable();
    all.dedup();
    assert_eq!(all.len(), n, "identifiant partagé avec un autre objet");
    assert!(all.iter().all(|i| *i < f.p.next_id));
}

// ---------------------------------------------------------------------------
// Commandes : cas valides
// ---------------------------------------------------------------------------

#[test]
fn nom_rogne_renommage_et_duplication_independante() {
    let mut f = sc();
    let a = variant_a(&mut f);
    f.p.apply(Command::RenameVariant {
        variant: a,
        name: "  Chemise  ".into(),
    })
    .unwrap();
    assert_eq!(f.p.variants[0].name, "Chemise");
    // Renommer avec la même graphie ou une autre casse de soi-même : accepté (pas un doublon).
    f.p.apply(Command::RenameVariant {
        variant: a,
        name: "CHEMISE".into(),
    })
    .unwrap();
    assert_eq!(f.p.variants[0].name, "CHEMISE");
    let copy =
        f.p.apply(Command::DuplicateVariant {
            variant: a,
            name: "Copie".into(),
        })
        .unwrap()
        .variants[0];
    assert_ne!(copy, a);
    let (src, dup) = (f.p.variant(a).unwrap().clone(), f.p.variant(copy).unwrap());
    assert_eq!(dup.name, "Copie");
    assert_eq!(dup.options, src.options);
    assert_eq!(dup.options_active, src.options_active);
    // Mêmes entrées, mais des identifiants **neufs** (schéma 6) : 34 à 36 après la copie (33).
    let strip = |v: &[VariantEntry]| {
        v.iter()
            .map(|e| (e.piece, e.quantity, e.material.clone(), e.option.clone()))
            .collect::<Vec<_>>()
    };
    assert_eq!(strip(&dup.entries), strip(&src.entries));
    assert_eq!(copy.0, 33);
    assert_eq!(
        dup.entries.iter().map(|e| e.id.0).collect::<Vec<_>>(),
        vec![34, 35, 36]
    );
    assert_eq!(
        src.entries.iter().map(|e| e.id.0).collect::<Vec<_>>(),
        vec![30, 31, 32]
    );
    // Indépendante : modifier la copie ne touche pas la source.
    set_active(&mut f.p, copy, "avec poche", false);
    f.p.apply(Command::RemoveVariantEntry {
        variant: copy,
        piece: f.dos,
        option: None,
    })
    .unwrap();
    assert_eq!(f.p.variant(a).unwrap(), &src);
    assert_eq!(f.p.variant(copy).unwrap().entries.len(), 2);
}

#[test]
fn entrees_modifier_quantite_matiere_option_et_retirer() {
    let mut f = sc();
    let a = variant_a(&mut f);
    // Quantité.
    f.p.apply(Command::UpdateVariantEntry {
        variant: a,
        piece: f.dos,
        option: None,
        quantity: 999,
        material: None,
        new_option: None,
    })
    .unwrap();
    assert_eq!(f.p.variants[0].entries[0].quantity, 999);
    // Matière : texte libre rogné, puis retirée.
    f.p.apply(Command::UpdateVariantEntry {
        variant: a,
        piece: f.dos,
        option: None,
        quantity: 1,
        material: Some("  Soie ".into()),
        new_option: None,
    })
    .unwrap();
    assert_eq!(f.p.variants[0].entries[0].material.as_deref(), Some("Soie"));
    // Option : l'entrée de la Poche passe de « avec poche » à aucune ; la graphie d'entrée peut
    // différer par la casse (la forme stockée est celle de la variante).
    f.p.apply(Command::UpdateVariantEntry {
        variant: a,
        piece: f.poche,
        option: Some("AVEC POCHE".into()),
        quantity: 2,
        material: None,
        new_option: None,
    })
    .unwrap();
    assert_eq!(f.p.variants[0].entries[1].option, None);
    f.p.apply(Command::UpdateVariantEntry {
        variant: a,
        piece: f.poche,
        option: None,
        quantity: 2,
        material: None,
        new_option: Some("Avec Poche".into()),
    })
    .unwrap();
    assert_eq!(
        f.p.variants[0].entries[1].option.as_deref(),
        Some("avec poche"),
        "l'option est stockée sous la graphie de la variante"
    );
    // Même pièce, deux options distinctes : deux entrées (pas un doublon).
    option(&mut f.p, a, "sans poche").unwrap();
    entry(&mut f.p, a, f.poche, 1, None, Some("sans poche")).unwrap();
    entry(&mut f.p, a, f.poche, 1, None, None).unwrap();
    assert_eq!(f.p.variants[0].entries.len(), 5);
    // Retrait.
    f.p.apply(Command::RemoveVariantEntry {
        variant: a,
        piece: f.poche,
        option: Some("sans poche".into()),
    })
    .unwrap();
    assert_eq!(f.p.variants[0].entries.len(), 4);
    f.p.validate().unwrap();
}

#[test]
fn options_ajout_inactive_activation_et_retrait() {
    let mut f = sc();
    let v = add_variant(&mut f.p, "V");
    option(&mut f.p, v, "  avec poche ").unwrap();
    let var = f.p.variant(v).unwrap();
    assert_eq!(var.options, ["avec poche"]);
    assert!(var.options_active.is_empty(), "une option naît inactive");
    set_active(&mut f.p, v, "AVEC POCHE", true);
    assert_eq!(f.p.variant(v).unwrap().options_active, ["avec poche"]);
    set_active(&mut f.p, v, "avec poche", false);
    assert!(f.p.variant(v).unwrap().options_active.is_empty());
    set_active(&mut f.p, v, "avec poche", true);
    f.p.apply(Command::RemoveVariantOption {
        variant: v,
        option: "avec poche".into(),
    })
    .unwrap();
    let var = f.p.variant(v).unwrap();
    assert!(var.options.is_empty() && var.options_active.is_empty());
}

// ---------------------------------------------------------------------------
// Refus : chaque cas invalide, sans trace
// ---------------------------------------------------------------------------

#[test]
fn refus_de_chaque_cas_invalide_sans_trace() {
    let mut f = sc();
    let a = variant_a(&mut f);
    let b = add_variant(&mut f.p, "B");
    let long = "x".repeat(121);
    let cases: Vec<(&str, Command, &str)> = vec![
        ("nom vide", Command::AddVariant { name: "".into() }, "vide"),
        (
            "nom d'espaces",
            Command::AddVariant { name: "   ".into() },
            "vide",
        ),
        (
            "nom trop long",
            Command::AddVariant { name: long.clone() },
            "120",
        ),
        (
            "nom dupliqué (casse)",
            Command::AddVariant { name: "a".into() },
            "existe déjà",
        ),
        (
            "renommer en nom pris",
            Command::RenameVariant {
                variant: b,
                name: "a".into(),
            },
            "existe déjà",
        ),
        (
            "renommer en nom vide",
            Command::RenameVariant {
                variant: b,
                name: " ".into(),
            },
            "vide",
        ),
        (
            "renommer une inconnue",
            Command::RenameVariant {
                variant: VariantId(99999),
                name: "Z".into(),
            },
            "introuvable",
        ),
        (
            "supprimer une inconnue",
            Command::RemoveVariant {
                variant: VariantId(99999),
            },
            "introuvable",
        ),
        (
            "dupliquer sous un nom pris",
            Command::DuplicateVariant {
                variant: a,
                name: "B".into(),
            },
            "existe déjà",
        ),
        (
            "dupliquer une inconnue",
            Command::DuplicateVariant {
                variant: VariantId(99999),
                name: "Z".into(),
            },
            "introuvable",
        ),
        (
            "quantité 0",
            Command::AddVariantEntry {
                variant: b,
                piece: f.dos,
                quantity: 0,
                material: None,
                option: None,
            },
            "entre 1 et 999",
        ),
        (
            "quantité 1000",
            Command::AddVariantEntry {
                variant: b,
                piece: f.dos,
                quantity: 1000,
                material: None,
                option: None,
            },
            "entre 1 et 999",
        ),
        (
            "pièce inconnue",
            Command::AddVariantEntry {
                variant: b,
                piece: PieceId(99999),
                quantity: 1,
                material: None,
                option: None,
            },
            "",
        ),
        (
            "option inconnue",
            Command::AddVariantEntry {
                variant: b,
                piece: f.dos,
                quantity: 1,
                material: None,
                option: Some("fantôme".into()),
            },
            "inconnue",
        ),
        (
            "matière vide",
            Command::AddVariantEntry {
                variant: b,
                piece: f.dos,
                quantity: 1,
                material: Some("  ".into()),
                option: None,
            },
            "vide",
        ),
        (
            "variante inconnue (entrée)",
            Command::AddVariantEntry {
                variant: VariantId(99999),
                piece: f.dos,
                quantity: 1,
                material: None,
                option: None,
            },
            "introuvable",
        ),
        (
            "entrée dupliquée (même pièce, sans option)",
            Command::AddVariantEntry {
                variant: a,
                piece: f.dos,
                quantity: 3,
                material: None,
                option: None,
            },
            "déjà",
        ),
        (
            "entrée dupliquée (même pièce, même option, casse)",
            Command::AddVariantEntry {
                variant: a,
                piece: f.poche,
                quantity: 3,
                material: None,
                option: Some("AVEC POCHE".into()),
            },
            "déjà",
        ),
        (
            "modifier une entrée absente",
            Command::UpdateVariantEntry {
                variant: a,
                piece: f.dos,
                option: Some("avec poche".into()),
                quantity: 1,
                material: None,
                new_option: Some("avec poche".into()),
            },
            "introuvable",
        ),
        (
            "modifier : quantité 0",
            Command::UpdateVariantEntry {
                variant: a,
                piece: f.dos,
                option: None,
                quantity: 0,
                material: None,
                new_option: None,
            },
            "entre 1 et 999",
        ),
        (
            "modifier : quantité 1000",
            Command::UpdateVariantEntry {
                variant: a,
                piece: f.dos,
                option: None,
                quantity: 1000,
                material: None,
                new_option: None,
            },
            "entre 1 et 999",
        ),
        (
            "modifier : option inconnue",
            Command::UpdateVariantEntry {
                variant: a,
                piece: f.dos,
                option: None,
                quantity: 1,
                material: None,
                new_option: Some("fantôme".into()),
            },
            "inconnue",
        ),
        (
            "retirer une entrée absente",
            Command::RemoveVariantEntry {
                variant: a,
                piece: f.dos,
                option: Some("avec poche".into()),
            },
            "introuvable",
        ),
        (
            "option vide",
            Command::AddVariantOption {
                variant: a,
                option: "  ".into(),
            },
            "vide",
        ),
        (
            "option dupliquée (casse)",
            Command::AddVariantOption {
                variant: a,
                option: "Avec Poche".into(),
            },
            "existe déjà",
        ),
        (
            "retirer une option inconnue",
            Command::RemoveVariantOption {
                variant: a,
                option: "fantôme".into(),
            },
            "inconnue",
        ),
        (
            "retirer une option utilisée",
            Command::RemoveVariantOption {
                variant: a,
                option: "avec poche".into(),
            },
            "Poche",
        ),
        (
            "activer une option inconnue",
            Command::SetVariantOptionActive {
                variant: a,
                option: "fantôme".into(),
                active: true,
            },
            "inconnue",
        ),
    ];
    for (name, command, needle) in cases {
        let m = refused(&mut f.p, command);
        assert!(m.contains(needle), "{name} : message « {m} »");
    }
}

#[test]
fn limites_de_variantes_d_options_et_d_entrees() {
    let mut f = sc();
    for i in 0..MAX_VARIANTS {
        add_variant(&mut f.p, &format!("V{i}"));
    }
    let m = refused(
        &mut f.p,
        Command::AddVariant {
            name: "de trop".into(),
        },
    );
    assert!(m.contains("50"), "{m}");
    let first = f.p.variants[0].id;
    let m = refused(
        &mut f.p,
        Command::DuplicateVariant {
            variant: first,
            name: "de trop".into(),
        },
    );
    assert!(m.contains("50"), "{m}");
    for i in 0..MAX_VARIANT_OPTIONS {
        option(&mut f.p, first, &format!("o{i}")).unwrap();
    }
    let m = refused(
        &mut f.p,
        Command::AddVariantOption {
            variant: first,
            option: "de trop".into(),
        },
    );
    assert!(m.contains("20"), "{m}");
}

// ---------------------------------------------------------------------------
// Nomenclature par variante : valeurs calculées à la main
// ---------------------------------------------------------------------------

#[test]
fn nomenclature_de_la_variante_a_la_main() {
    let mut f = sc();
    let a = variant_a(&mut f);
    let bom = f.p.bill_of_materials_variant(None, Some(a)).unwrap();
    assert_eq!(bom.variant, Some(a));
    // Trois lignes : Dos × 1 ; Poche × 2 ; Manche × 1 doublée (paire en miroir) = 2 découpes.
    let l: Vec<(&str, u32, u32, Option<&str>)> = bom
        .lines
        .iter()
        .map(|x| (x.name.as_str(), x.quantity, x.cuts, x.option.as_deref()))
        .collect();
    assert_eq!(
        l,
        [
            ("Dos", 1, 1, None),
            ("Poche", 2, 2, Some("avec poche")),
            ("Manche", 1, 2, None)
        ]
    );
    // Matières : Coton (Dos + Poche) = 3 découpes, 5000 + 2 × 2000 = 9000 ; Lin (surcharge de la
    // matière de la pièce) = 2 découpes, 2 × 3000 = 6000.
    let m: Vec<(Option<&str>, usize, u32)> = bom
        .materials
        .iter()
        .map(|x| (x.material.as_deref(), x.pieces, x.cuts))
        .collect();
    assert_eq!(m, [(Some("Coton"), 2, 3), (Some("Lin"), 1, 2)]);
    assert!(close(bom.materials[0].net_area_mm2[0], 9000.0));
    assert!(close(bom.materials[1].net_area_mm2[0], 6000.0));
    // Option désactivée : la Poche (2 instances, 4 000 mm²) disparaît.
    set_active(&mut f.p, a, "avec poche", false);
    let bom = f.p.bill_of_materials_variant(None, Some(a)).unwrap();
    assert_eq!(bom.lines.len(), 2);
    assert!(bom.lines.iter().all(|x| x.name != "Poche"));
    assert_eq!(bom.materials[0].cuts, 1);
    assert!(close(bom.materials[0].net_area_mm2[0], 5000.0));
    assert!(close(bom.materials[1].net_area_mm2[0], 6000.0));
    // Réactivée : retour exact.
    set_active(&mut f.p, a, "avec poche", true);
    let bom = f.p.bill_of_materials_variant(None, Some(a)).unwrap();
    assert!(close(bom.materials[0].net_area_mm2[0], 9000.0));
}

#[test]
fn sans_variante_la_nomenclature_est_celle_du_projet() {
    let mut f = sc();
    variant_a(&mut f);
    // Tout le projet : quantité de chaque pièce (1), Manche en paire = 2 découpes ; Coton =
    // 5000 + 2000 + 2 × 3000 = 13 000, 3 pièces, 4 découpes. Les variantes n'y changent rien.
    let bom = f.p.bill_of_materials(None).unwrap();
    assert_eq!(bom.variant, None);
    assert_eq!(bom.lines.len(), 3);
    assert!(bom.lines.iter().all(|l| l.option.is_none()));
    assert_eq!(bom.materials.len(), 1);
    assert_eq!((bom.materials[0].pieces, bom.materials[0].cuts), (3, 4));
    assert!(close(bom.materials[0].net_area_mm2[0], 13000.0));
    assert_eq!(bom, f.p.bill_of_materials_variant(None, None).unwrap());
    // Variante inconnue : erreur typée.
    assert!(f
        .p
        .bill_of_materials_variant(None, Some(VariantId(99999)))
        .is_err());
}

#[test]
fn nomenclature_par_variante_et_par_taille() {
    let mut f = sc();
    let a = variant_a(&mut f);
    f.p.apply(Command::AddSizeRange {
        name: "G".into(),
        kind: SizeKind::Alphanumeric,
        labels: vec!["S".into(), "M".into()],
        base: 0,
    })
    .unwrap();
    let range = f.p.sizes[0].id;
    let bom = f.p.bill_of_materials_variant(Some(range), Some(a)).unwrap();
    assert_eq!(bom.labels, ["S", "M"]);
    // Sans écart de gradation : les deux colonnes valent la base.
    for col in 0..2 {
        assert!(close(bom.materials[0].net_area_mm2[col], 9000.0));
        assert!(close(bom.materials[1].net_area_mm2[col], 6000.0));
    }
}

// ---------------------------------------------------------------------------
// CA : une pièce partagée modifiée l'est dans toutes les variantes
// ---------------------------------------------------------------------------

#[test]
fn une_piece_partagee_modifiee_l_est_dans_toutes_les_variantes() {
    let mut f = sc();
    let a = variant_a(&mut f);
    let b = add_variant(&mut f.p, "B");
    entry(&mut f.p, b, f.dos, 3, None, None).unwrap();
    let find = |p: &Project, v: VariantId, piece: PieceId| {
        p.bill_of_materials_variant(None, Some(v))
            .unwrap()
            .lines
            .into_iter()
            .find(|l| l.piece == piece)
            .unwrap()
    };
    // Avant : même nom, même aire dans les deux variantes ; quantités propres à chacune.
    let (da, db) = (find(&f.p, a, f.dos), find(&f.p, b, f.dos));
    assert_eq!((da.name.as_str(), db.name.as_str()), ("Dos", "Dos"));
    assert!(close(da.areas_mm2[0], 5000.0) && close(db.areas_mm2[0], 5000.0));
    assert_eq!((da.quantity, db.quantity), (1, 3));
    // 1. Nom + matière de la pièce (la variante B n'a pas de surcharge).
    card(&mut f.p, f.dos, "Dos long", 1, false, Some("Popeline"));
    let (da, db) = (find(&f.p, a, f.dos), find(&f.p, b, f.dos));
    assert_eq!(
        (da.name.as_str(), db.name.as_str()),
        ("Dos long", "Dos long")
    );
    assert_eq!(db.material.as_deref(), Some("Popeline"));
    // 2. Géométrie : le coin (100 ; 50) monte à (100 ; 60). Polygone (0,0) (100,0) (100,60)
    // (0,50) : formule du lacet = (0 + 6000 + 5000 + 0) / 2 = 5500.
    let corner = f.p.sheets[0]
        .points
        .iter()
        .find(|pt| {
            let at = f.p.sheets[0].position(pt.id).unwrap();
            (at.x - 100.0).abs() < 1e-9 && (at.y - 50.0).abs() < 1e-9
        })
        .unwrap()
        .id;
    f.p.apply(Command::MovePoint {
        sheet: f.s,
        point: corner,
        to: PointMm::new(100.0, 60.0),
    })
    .unwrap();
    let (da, db) = (find(&f.p, a, f.dos), find(&f.p, b, f.dos));
    assert!(close(da.areas_mm2[0], 5500.0), "{:?}", da.areas_mm2);
    assert!(close(db.areas_mm2[0], 5500.0), "{:?}", db.areas_mm2);
    // 3. Marge de couture de la pièce : la pièce reste UNE seule et les variantes ne la copient pas.
    assert_eq!(f.p.pieces.len(), 3);
    assert_eq!(f.p.variant(a).unwrap().entries[0].piece, f.dos);
    assert_eq!(f.p.variant(b).unwrap().entries[0].piece, f.dos);
    // Surcharge de matière : seule la variante qui la porte est concernée (propre à l'entrée).
    f.p.apply(Command::UpdateVariantEntry {
        variant: b,
        piece: f.dos,
        option: None,
        quantity: 3,
        material: Some("Lin".into()),
        new_option: None,
    })
    .unwrap();
    assert_eq!(find(&f.p, a, f.dos).material.as_deref(), Some("Popeline"));
    assert_eq!(find(&f.p, b, f.dos).material.as_deref(), Some("Lin"));
}

// ---------------------------------------------------------------------------
// Intégrité référentielle : supprimer une pièce citée est REFUSÉ
// ---------------------------------------------------------------------------

#[test]
fn supprimer_une_piece_citee_est_refuse_en_nommant_la_variante() {
    let mut f = sc();
    variant_a(&mut f);
    let m = refused(&mut f.p, Command::DeletePiece { piece: f.dos });
    assert!(m.contains("« A »") && m.contains("retirez"), "{m}");
    // La feuille qui porte la pièce citée : même refus.
    f.p.apply(Command::CreateSheet {
        name: "Autre".into(),
    })
    .unwrap();
    let m = refused(&mut f.p, Command::DeleteSheet { sheet: f.s });
    assert!(m.contains("« A »") && m.contains("retirez"), "{m}");
    // Après le retrait de l'entrée, la suppression passe.
    f.p.apply(Command::RemoveVariantEntry {
        variant: f.p.variants[0].id,
        piece: f.dos,
        option: None,
    })
    .unwrap();
    f.p.apply(Command::DeletePiece { piece: f.dos }).unwrap();
    assert_eq!(f.p.pieces.len(), 2);
    // Supprimer la variante libère aussi la pièce.
    let v = f.p.variants[0].id;
    f.p.apply(Command::RemoveVariant { variant: v }).unwrap();
    f.p.apply(Command::DeletePiece { piece: f.poche }).unwrap();
}

#[test]
fn supprimer_une_option_utilisee_est_refuse_en_nommant_l_entree() {
    let mut f = sc();
    let a = variant_a(&mut f);
    let m = refused(
        &mut f.p,
        Command::RemoveVariantOption {
            variant: a,
            option: "avec poche".into(),
        },
    );
    assert!(m.contains("Poche") && m.contains("avec poche"), "{m}");
    // Une fois l'entrée détachée de l'option, le retrait passe.
    f.p.apply(Command::UpdateVariantEntry {
        variant: a,
        piece: f.poche,
        option: Some("avec poche".into()),
        quantity: 2,
        material: None,
        new_option: None,
    })
    .unwrap();
    f.p.apply(Command::RemoveVariantOption {
        variant: a,
        option: "avec poche".into(),
    })
    .unwrap();
}

#[test]
fn supprimer_la_variante_ne_touche_pas_aux_pieces() {
    let mut f = sc();
    let a = variant_a(&mut f);
    let before: Vec<_> = f.p.pieces.clone();
    f.p.apply(Command::RemoveVariant { variant: a }).unwrap();
    assert_eq!(f.p.pieces, before);
    assert!(f.p.variants.is_empty());
}

// ---------------------------------------------------------------------------
// Historique : un pas par geste, aucun pas pour la même valeur
// ---------------------------------------------------------------------------

#[test]
fn historique_une_etape_et_aucune_pour_la_meme_valeur() {
    let f = sc();
    let (dos, poche) = (f.dos, f.poche);
    let mut doc = Document::new(f.p);
    let mut states = vec![bytes(doc.project())];
    let mut run = |doc: &mut Document, c: Command| -> Outcome {
        let o = doc.execute(c).unwrap();
        states.push(bytes(doc.project()));
        o
    };
    let v = run(&mut doc, Command::AddVariant { name: "V".into() }).variants[0];
    run(
        &mut doc,
        Command::AddVariantOption {
            variant: v,
            option: "o".into(),
        },
    );
    run(
        &mut doc,
        Command::AddVariantEntry {
            variant: v,
            piece: dos,
            quantity: 2,
            material: None,
            option: None,
        },
    );
    run(
        &mut doc,
        Command::AddVariantEntry {
            variant: v,
            piece: poche,
            quantity: 1,
            material: None,
            option: Some("o".into()),
        },
    );
    run(
        &mut doc,
        Command::SetVariantOptionActive {
            variant: v,
            option: "o".into(),
            active: true,
        },
    );
    run(
        &mut doc,
        Command::UpdateVariantEntry {
            variant: v,
            piece: dos,
            option: None,
            quantity: 5,
            material: Some("Lin".into()),
            new_option: None,
        },
    );
    let w = run(
        &mut doc,
        Command::DuplicateVariant {
            variant: v,
            name: "W".into(),
        },
    )
    .variants[0];
    run(
        &mut doc,
        Command::RenameVariant {
            variant: w,
            name: "W2".into(),
        },
    );
    run(
        &mut doc,
        Command::RemoveVariantEntry {
            variant: w,
            piece: dos,
            option: None,
        },
    );
    run(&mut doc, Command::RemoveVariant { variant: w });
    let steps = states.len() - 1;
    // Mêmes valeurs : aucun pas, aucun octet changé.
    let last = states.last().unwrap().clone();
    for c in [
        Command::RenameVariant {
            variant: v,
            name: "V".into(),
        },
        Command::SetVariantOptionActive {
            variant: v,
            option: "o".into(),
            active: true,
        },
        Command::UpdateVariantEntry {
            variant: v,
            piece: dos,
            option: None,
            quantity: 5,
            material: Some("Lin".into()),
            new_option: None,
        },
    ] {
        doc.execute(c).unwrap();
        assert_eq!(bytes(doc.project()), last);
    }
    // Exactement `steps` annulations, chacune rétablit l'état précédent à l'octet.
    for i in (0..steps).rev() {
        assert!(doc.undo(), "annulation {i}");
        assert_eq!(bytes(doc.project()), states[i], "annulation {i}");
    }
    assert!(!doc.can_undo(), "pas supplémentaire créé par un no-op");
    for (i, state) in states.iter().enumerate().skip(1) {
        assert!(doc.redo(), "rétablissement {i}");
        assert_eq!(&bytes(doc.project()), state);
    }
}

// ---------------------------------------------------------------------------
// Persistance : refus au chargement
// ---------------------------------------------------------------------------

fn full_json() -> (Value, Sc) {
    let mut f = sc();
    variant_a(&mut f);
    let b = add_variant(&mut f.p, "B");
    entry(&mut f.p, b, f.dos, 3, None, None).unwrap();
    (serde_json::from_str(&bytes(&f.p)).unwrap(), f)
}

fn load(v: &Value) -> Result<Project, CoreError> {
    persist::from_json(&v.to_string())
}

#[test]
fn chargement_accepte_les_variantes_completes() {
    let (v, f) = full_json();
    assert_eq!(load(&v).unwrap(), f.p);
}

#[test]
fn cle_reservee_refusee_dans_un_fichier_anterieur_a_la_v5() {
    let (mut v, _) = full_json();
    for version in 1..=4 {
        v["schema_version"] = json!(version);
        let e = load(&v).expect_err("clé v5 dans un fichier plus ancien");
        let m = e.to_string();
        assert!(
            m.contains("variants") && m.contains("version 5"),
            "v{version} : {m}"
        );
    }
}

#[test]
fn chargement_refuse_chaque_cas_invalide() {
    let (good, f) = full_json();
    let next = f.p.next_id;
    let dos = f.dos.0;
    type Case = (&'static str, Box<dyn Fn(&mut Value)>);
    let cases: Vec<Case> = vec![
        (
            "clé inconnue dans une variante",
            Box::new(|d| d["variants"][0]["zz"] = json!(1)),
        ),
        (
            "clé inconnue dans une entrée",
            Box::new(|d| d["variants"][0]["entries"][0]["zz"] = json!(1)),
        ),
        (
            "id de variante dupliqué",
            Box::new(|d| {
                let id = d["variants"][0]["id"].clone();
                d["variants"][1]["id"] = id;
            }),
        ),
        (
            "id de variante >= next_id",
            Box::new(move |d| d["variants"][1]["id"] = json!(next)),
        ),
        (
            "id de variante égal à une pièce",
            Box::new(move |d| d["variants"][1]["id"] = json!(dos)),
        ),
        (
            "nom vide",
            Box::new(|d| d["variants"][0]["name"] = json!("")),
        ),
        (
            "nom avec espaces de bord",
            Box::new(|d| d["variants"][0]["name"] = json!(" A")),
        ),
        (
            "nom dupliqué (casse)",
            Box::new(|d| d["variants"][1]["name"] = json!("a")),
        ),
        (
            "pièce inconnue",
            Box::new(|d| d["variants"][0]["entries"][0]["piece"] = json!(99999)),
        ),
        (
            "quantité 0",
            Box::new(|d| d["variants"][0]["entries"][0]["quantity"] = json!(0)),
        ),
        (
            "quantité 1000",
            Box::new(|d| d["variants"][0]["entries"][0]["quantity"] = json!(1000)),
        ),
        (
            "quantité négative",
            Box::new(|d| d["variants"][0]["entries"][0]["quantity"] = json!(-1)),
        ),
        (
            "quantité décimale",
            Box::new(|d| d["variants"][0]["entries"][0]["quantity"] = json!(1.5)),
        ),
        (
            "matière vide",
            Box::new(|d| d["variants"][0]["entries"][0]["material"] = json!("")),
        ),
        (
            "option inconnue",
            Box::new(|d| d["variants"][0]["entries"][0]["option"] = json!("fantôme")),
        ),
        (
            "option active inconnue",
            Box::new(|d| d["variants"][0]["options_active"] = json!(["fantôme"])),
        ),
        (
            "option active dupliquée",
            Box::new(|d| d["variants"][0]["options_active"] = json!(["avec poche", "avec poche"])),
        ),
        (
            "option dupliquée (casse)",
            Box::new(|d| d["variants"][0]["options"] = json!(["avec poche", "AVEC POCHE"])),
        ),
        (
            "option vide",
            Box::new(|d| d["variants"][0]["options"] = json!(["avec poche", ""])),
        ),
        (
            "entrée dupliquée",
            Box::new(|d| {
                let e = d["variants"][0]["entries"][0].clone();
                d["variants"][0]["entries"].as_array_mut().unwrap().push(e);
            }),
        ),
        (
            "variants n'est pas une liste",
            Box::new(|d| d["variants"] = json!({})),
        ),
    ];
    for (name, mutate) in cases {
        let mut doc = good.clone();
        mutate(&mut doc);
        assert_ne!(doc, good, "{name} : la mutation n'a rien changé");
        assert!(load(&doc).is_err(), "{name} : le chargeur accepte");
    }
}

#[test]
fn nombres_extremes_ne_paniquent_pas() {
    let (mut v, _) = full_json();
    for q in [
        json!(u64::MAX),
        json!(4294967296u64),
        json!(1e30),
        json!(null),
    ] {
        v["variants"][0]["entries"][0]["quantity"] = q;
        assert!(load(&v).is_err());
    }
}
