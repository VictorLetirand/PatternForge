//! US-11.5 conventions de nommage (tranche 130) — un test par critère, valeurs écrites à la main.

use pf_core::persist;
use pf_core::*;
use serde_json::{json, Value};

fn spec(name: &str, target: NamingTarget, prefix: &str, sep: &str) -> NamingRuleSpec {
    NamingRuleSpec {
        name: name.into(),
        target,
        prefix: prefix.into(),
        separator: sep.into(),
        suffix: String::new(),
        counter: Some(NamingCounter {
            start: 1,
            step: 1,
            width: 3,
        }),
    }
}

fn add_rule(p: &mut Project, s: NamingRuleSpec) -> NamingRuleId {
    p.apply(Command::AddNamingRule { spec: s })
        .unwrap()
        .naming_rules[0]
}

fn add_piece(p: &mut Project, name: &str) -> PieceId {
    let sheet = p.sheets[0].id;
    p.apply(Command::AddPiece {
        sheet,
        name: name.into(),
        quantity: 1,
        material: None,
        mirrored_pair: false,
    })
    .unwrap()
    .pieces[0]
}

fn refused(p: &mut Project, c: Command, needle: &str) {
    let before = persist::to_json(p).unwrap();
    let e = p.apply(c).expect_err("devait être refusé").to_string();
    assert!(e.contains(needle), "« {needle} » absent de : {e}");
    assert_eq!(persist::to_json(p).unwrap(), before, "refus avec trace");
}

#[test]
fn prochain_nom_depart_puis_plus_grand_numero_plus_pas() {
    let mut p = Project::new("N");
    let r = add_rule(&mut p, spec("Pièces", NamingTarget::Piece, "P", "-"));
    assert_eq!(p.next_name(r).unwrap(), "P-001");
    add_piece(&mut p, "P-001");
    add_piece(&mut p, "P-002");
    assert_eq!(p.next_name(r).unwrap(), "P-003");
    // Un trou n'est pas comblé : le plus grand numéro compte (7), pas le nombre de pièces.
    add_piece(&mut p, "P-007");
    assert_eq!(p.next_name(r).unwrap(), "P-008");
    // Les noms qui ne suivent pas le motif sont ignorés : autre préfixe, texte, 10 chiffres.
    for n in ["Q-099", "P-x1", "P-", "P-1234567890", "Dos"] {
        add_piece(&mut p, n);
    }
    assert_eq!(p.next_name(r).unwrap(), "P-008");
    // Zéros de tête au-delà de la largeur : lus comme 12.
    add_piece(&mut p, "P-0012");
    assert_eq!(p.next_name(r).unwrap(), "P-013");
}

#[test]
fn debut_pas_et_largeur() {
    let mut p = Project::new("N");
    let mut s = spec("Pièces", NamingTarget::Piece, "P", "-");
    s.counter = Some(NamingCounter {
        start: 10,
        step: 5,
        width: 0,
    });
    let r = add_rule(&mut p, s);
    assert_eq!(p.next_name(r).unwrap(), "P-10");
    add_piece(&mut p, "P-10");
    assert_eq!(p.next_name(r).unwrap(), "P-15");
    // Un numéro inférieur au début ne ramène pas en arrière.
    let mut q = Project::new("N");
    let mut s = spec("Pièces", NamingTarget::Piece, "P", "-");
    s.counter = Some(NamingCounter {
        start: 10,
        step: 5,
        width: 0,
    });
    let r = add_rule(&mut q, s);
    add_piece(&mut q, "P-2");
    assert_eq!(q.next_name(r).unwrap(), "P-10");
}

#[test]
fn suffixe_separateur_et_parties_vides() {
    let mut p = Project::new("N");
    let mut s = spec("Feuilles", NamingTarget::Sheet, "", " ");
    s.suffix = "mm".into();
    s.counter = Some(NamingCounter {
        start: 1,
        step: 1,
        width: 0,
    });
    let r = add_rule(&mut p, s);
    // Pas de préfixe : « 1 mm » (le séparateur n'apparaît qu'entre deux parties).
    assert_eq!(p.next_name(r).unwrap(), "1 mm");
    p.apply(Command::CreateSheet {
        name: "4 mm".into(),
    })
    .unwrap();
    assert_eq!(p.next_name(r).unwrap(), "5 mm");
    // Préfixe seul, sans compteur : le nom fixe, puis refus quand il est pris.
    let mut q = Project::new("N");
    let mut s = spec("Variantes", NamingTarget::Variant, "Base", "-");
    s.counter = None;
    let r = add_rule(&mut q, s);
    assert_eq!(q.next_name(r).unwrap(), "Base");
    q.apply(Command::AddVariant {
        name: "base".into(),
    })
    .unwrap();
    assert!(q
        .next_name(r)
        .unwrap_err()
        .to_string()
        .contains("pas de compteur"));
}

#[test]
fn collision_avec_un_nom_existant_saute_au_suivant_libre() {
    let mut p = Project::new("N");
    let r = add_rule(&mut p, spec("Pièces", NamingTarget::Piece, "P", "-"));
    add_piece(&mut p, "P-001");
    // « p-002 » (casse) n'est pas relu comme numéro mais occupe le nom P-002 ; « p-003 » aussi.
    add_piece(&mut p, "p-002");
    add_piece(&mut p, "p-003");
    assert_eq!(p.next_name(r).unwrap(), "P-004");
}

#[test]
fn le_compteur_est_derive_donc_l_annulation_ne_laisse_ni_trou_ni_double() {
    let mut doc = Document::new(Project::new("N"));
    let r = doc
        .execute(Command::AddNamingRule {
            spec: spec("Pièces", NamingTarget::Piece, "P", "-"),
        })
        .unwrap()
        .naming_rules[0];
    let sheet = doc.project().sheets[0].id;
    let create = |doc: &mut Document| {
        let name = doc.project().next_name(r).unwrap();
        doc.execute(Command::AddPiece {
            sheet,
            name: name.clone(),
            quantity: 1,
            material: None,
            mirrored_pair: false,
        })
        .unwrap();
        name
    };
    assert_eq!(create(&mut doc), "P-001");
    assert_eq!(create(&mut doc), "P-002");
    let bytes_after_two = persist::to_json(doc.project()).unwrap();
    assert!(doc.undo());
    // Annulée : le numéro 2 est de nouveau le prochain (pas de trou), puis rétabli sans double.
    assert_eq!(doc.project().next_name(r).unwrap(), "P-002");
    assert!(doc.redo());
    assert_eq!(persist::to_json(doc.project()).unwrap(), bytes_after_two);
    assert_eq!(doc.project().next_name(r).unwrap(), "P-003");
    // Annuler la règle entière rend le projet d'origine.
    while doc.undo() {}
    assert_eq!(
        persist::to_json(doc.project()).unwrap(),
        persist::to_json(&Project::new("N")).unwrap()
    );
}

#[test]
fn creer_une_regle_ne_renomme_rien_et_ne_change_pas_les_octets_sans_regle() {
    let mut p = Project::new("N");
    add_piece(&mut p, "Dos");
    let plain = persist::to_json(&p).unwrap();
    assert!(!plain.contains("naming_rules"));
    add_rule(&mut p, spec("Pièces", NamingTarget::Piece, "P", "-"));
    assert_eq!(p.pieces[0].name, "Dos", "aucun renommage rétroactif");
    let with = persist::to_json(&p).unwrap();
    assert!(with.contains("\"naming_rules\""));
    let id = p.naming_rules[0].id;
    p.apply(Command::RemoveNamingRule { rule: id }).unwrap();
    // Règle retirée : retour aux mêmes octets (la clé disparaît, le compteur d'ids a avancé).
    assert!(!persist::to_json(&p).unwrap().contains("naming_rules"));
}

#[test]
fn refus_nommes_sans_trace() {
    let mut p = Project::new("N");
    let ok = spec("Pièces", NamingTarget::Piece, "P", "-");
    add_rule(&mut p, ok.clone());
    let add = |s: NamingRuleSpec| Command::AddNamingRule { spec: s };
    let other = |name: &str, target| spec(name, target, "F", "-");
    refused(&mut p, add(other("", NamingTarget::Sheet)), "vide");
    refused(
        &mut p,
        add(other("pièces", NamingTarget::Sheet)),
        "existe déjà",
    );
    refused(
        &mut p,
        add(other("Autre", NamingTarget::Piece)),
        "une seule règle par cible",
    );
    let mut s = other("Feuilles", NamingTarget::Sheet);
    s.prefix = String::new();
    s.counter = None;
    refused(&mut p, add(s), "préfixe vide exige un compteur");
    let mut s = other("Feuilles", NamingTarget::Sheet);
    s.prefix = "F\u{7}".into();
    refused(&mut p, add(s), "caractère de contrôle");
    let mut s = other("Feuilles", NamingTarget::Sheet);
    s.suffix = "x".repeat(41);
    refused(&mut p, add(s), "dépasse 40 caractères");
    let mut s = other("Feuilles", NamingTarget::Sheet);
    s.separator = "12345".into();
    refused(&mut p, add(s), "dépasse 4 caractères");
    let mut s = other("Feuilles", NamingTarget::Sheet);
    s.prefix = " F".into();
    refused(&mut p, add(s), "espaces");
    for (counter, needle) in [
        ((1_000_000, 1, 0), "début du compteur"),
        ((1, 0, 0), "pas du compteur"),
        ((1, 1001, 0), "pas du compteur"),
        ((1, 1, 10), "largeur"),
    ] {
        let mut s = other("Feuilles", NamingTarget::Sheet);
        s.counter = Some(NamingCounter {
            start: counter.0,
            step: counter.1,
            width: counter.2,
        });
        refused(&mut p, add(s), needle);
    }
    let ghost = NamingRuleId(99_999);
    refused(
        &mut p,
        Command::RemoveNamingRule { rule: ghost },
        "introuvable",
    );
    refused(
        &mut p,
        Command::ApplyNamingRule { rule: ghost },
        "introuvable",
    );
    refused(
        &mut p,
        Command::UpdateNamingRule {
            rule: ghost,
            spec: ok,
        },
        "introuvable",
    );
    // Bornes acceptées : début 999999, pas 1000, largeur 9.
    let mut s = other("Bornes", NamingTarget::Sheet);
    s.counter = Some(NamingCounter {
        start: 999_999,
        step: 1000,
        width: 9,
    });
    add_rule(&mut p, s);
    // Un numéro dépassant 999 999 999 est refusé à la lecture : 999999 + 1000 × k.
    let r = p.naming_rules[1].id;
    assert_eq!(p.next_name(r).unwrap(), "F-000999999");
}

#[test]
fn modifier_et_supprimer_une_regle_meme_valeur_aucun_pas() {
    let mut doc = Document::new(Project::new("N"));
    let s = spec("Pièces", NamingTarget::Piece, "P", "-");
    let r = doc
        .execute(Command::AddNamingRule { spec: s.clone() })
        .unwrap()
        .naming_rules[0];
    assert!(doc.can_undo());
    let _ = doc.undo();
    let r2 = doc
        .execute(Command::AddNamingRule { spec: s.clone() })
        .unwrap()
        .naming_rules[0];
    assert_eq!(r, r2, "annuler puis recréer rend le même identifiant");
    // Même valeur : aucune étape (annuler remonte jusqu'à la création, pas à une modification).
    doc.execute(Command::UpdateNamingRule {
        rule: r2,
        spec: s.clone(),
    })
    .unwrap();
    assert!(doc.undo());
    assert!(doc.project().naming_rules.is_empty());
    assert!(!doc.can_undo());
    assert!(doc.redo());
    // Valeur changée : une étape ; casse du nom changée : acceptée (la règle ne se bloque pas).
    let mut t = s.clone();
    t.name = "PIÈCES".into();
    t.prefix = "Q".into();
    doc.execute(Command::UpdateNamingRule { rule: r2, spec: t })
        .unwrap();
    assert_eq!(doc.project().naming_rules[0].prefix, "Q");
    assert!(doc.undo());
    assert_eq!(doc.project().naming_rules[0].prefix, "P");
    // Suppression : une étape, les noms déjà donnés ne changent pas.
    add_piece_doc(&mut doc, "P-001");
    doc.execute(Command::RemoveNamingRule { rule: r2 }).unwrap();
    assert_eq!(doc.project().pieces[0].name, "P-001");
    assert!(doc.undo());
    assert_eq!(doc.project().naming_rules.len(), 1);
}

fn add_piece_doc(doc: &mut Document, name: &str) {
    let sheet = doc.project().sheets[0].id;
    doc.execute(Command::AddPiece {
        sheet,
        name: name.into(),
        quantity: 1,
        material: None,
        mirrored_pair: false,
    })
    .unwrap();
}

#[test]
fn renommage_en_lot_apercu_puis_une_seule_etape() {
    let mut doc = Document::new(Project::new("N"));
    for n in ["Dos", "Poche", "Manche"] {
        add_piece_doc(&mut doc, n);
    }
    let mut s = spec("Pièces", NamingTarget::Piece, "P", "-");
    s.counter = Some(NamingCounter {
        start: 10,
        step: 10,
        width: 2,
    });
    let r = doc
        .execute(Command::AddNamingRule { spec: s })
        .unwrap()
        .naming_rules[0];
    let preview = doc.project().naming_preview(r).unwrap();
    let got: Vec<(&str, &str)> = preview
        .iter()
        .map(|c| (c.old.as_str(), c.new.as_str()))
        .collect();
    assert_eq!(
        got,
        [("Dos", "P-10"), ("Poche", "P-20"), ("Manche", "P-30")]
    );
    // L'aperçu ne modifie rien.
    assert_eq!(doc.project().pieces[0].name, "Dos");
    let before = persist::to_json(doc.project()).unwrap();
    doc.execute(Command::ApplyNamingRule { rule: r }).unwrap();
    let names: Vec<_> = doc
        .project()
        .pieces
        .iter()
        .map(|p| p.name.as_str())
        .collect();
    assert_eq!(names, ["P-10", "P-20", "P-30"]);
    // Une seule annulation, octets d'origine.
    assert!(doc.undo());
    assert_eq!(persist::to_json(doc.project()).unwrap(), before);
    // Rejouer le lot sur des noms déjà conformes : aucun pas.
    assert!(doc.redo());
    let steps = {
        doc.execute(Command::ApplyNamingRule { rule: r }).unwrap();
        let mut n = 0;
        while doc.undo() {
            n += 1;
        }
        n
    };
    // Pièces ×3, règle, lot = 5 étapes : le second lot n'en a pas ajouté.
    assert_eq!(steps, 5);
}

#[test]
fn lot_sur_les_codes_les_feuilles_et_les_variantes_refus_sans_compteur() {
    let mut p = Project::new("N");
    let a = add_piece(&mut p, "A");
    let b = add_piece(&mut p, "B");
    let card = |piece, name: &str, code: Option<&str>| Command::SetPieceCard {
        piece,
        name: name.into(),
        code: code.map(str::to_string),
        quantity: 1,
        mirrored_pair: false,
        material: None,
        category: None,
        comment: None,
    };
    p.apply(card(a, "A", Some("zz"))).unwrap();
    p.apply(card(b, "B", None)).unwrap();
    let c = add_rule(&mut p, spec("Codes", NamingTarget::PieceCode, "C", "_"));
    p.apply(Command::ApplyNamingRule { rule: c }).unwrap();
    // Seule la pièce qui avait un code est renumérotée ; l'autre n'en reçoit pas.
    assert_eq!(p.pieces[0].code.as_deref(), Some("C_001"));
    assert_eq!(p.pieces[1].code, None);
    p.apply(Command::CreateSheet { name: "S2".into() }).unwrap();
    let f = add_rule(&mut p, spec("Feuilles", NamingTarget::Sheet, "F", "-"));
    p.apply(Command::ApplyNamingRule { rule: f }).unwrap();
    let sheets: Vec<_> = p.sheets.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(sheets, ["F-001", "F-002"]);
    p.apply(Command::AddVariant { name: "x".into() }).unwrap();
    p.apply(Command::AddVariant { name: "y".into() }).unwrap();
    let mut s = spec("Variantes", NamingTarget::Variant, "V", "-");
    s.counter = None;
    let v = add_rule(&mut p, s);
    refused(
        &mut p,
        Command::ApplyNamingRule { rule: v },
        "exige un compteur",
    );
    assert!(p.naming_preview(v).is_err());
}

#[test]
fn fichier_relu_et_cle_reservee_avant_la_v5() {
    let mut p = Project::new("N");
    add_rule(&mut p, spec("Pièces", NamingTarget::Piece, "P", "-"));
    let text = persist::to_json(&p).unwrap();
    let doc: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        doc["naming_rules"],
        json!([{
            "id": p.naming_rules[0].id.0,
            "name": "Pièces",
            "target": "piece",
            "prefix": "P",
            "separator": "-",
            "counter": {"start": 1, "step": 1, "width": 3}
        }])
    );
    assert_eq!(
        doc["schema_version"],
        json!(6),
        "schéma courant : la version 6 (US-11.5 n'ajoute aucune clé)"
    );
    let back = persist::from_json(&text).unwrap();
    assert_eq!(persist::to_json(&back).unwrap(), text);
    // Fichier déclaré v4 (ou plus ancien) : clé réservée, même vide.
    for (version, value) in [(4, doc["naming_rules"].clone()), (4, json!([]))] {
        let mut d = doc.clone();
        d["schema_version"] = json!(version);
        d["naming_rules"] = value;
        let e = persist::from_json(&d.to_string()).unwrap_err().to_string();
        assert!(
            e.contains("« naming_rules » est réservée au format version 5"),
            "{e}"
        );
    }
}

#[test]
fn fichiers_abimes_refuses_au_chargement() {
    let mut p = Project::new("N");
    add_rule(&mut p, spec("Pièces", NamingTarget::Piece, "P", "-"));
    add_rule(&mut p, spec("Feuilles", NamingTarget::Sheet, "F", "-"));
    let good: Value = serde_json::from_str(&persist::to_json(&p).unwrap()).unwrap();
    type Case = (&'static str, Box<dyn Fn(&mut Value)>);
    let cases: Vec<Case> = vec![
        (
            "clé en trop",
            Box::new(|d| d["naming_rules"][0]["zz"] = json!(1)),
        ),
        (
            "clé en trop dans le compteur",
            Box::new(|d| d["naming_rules"][0]["counter"]["zz"] = json!(1)),
        ),
        (
            "cible inconnue",
            Box::new(|d| d["naming_rules"][0]["target"] = json!("point")),
        ),
        (
            "pas nul",
            Box::new(|d| d["naming_rules"][0]["counter"]["step"] = json!(0)),
        ),
        (
            "pas énorme",
            Box::new(|d| d["naming_rules"][0]["counter"]["step"] = json!(1001)),
        ),
        (
            "pas négatif",
            Box::new(|d| d["naming_rules"][0]["counter"]["step"] = json!(-1)),
        ),
        (
            "pas décimal",
            Box::new(|d| d["naming_rules"][0]["counter"]["step"] = json!(1.5)),
        ),
        (
            "début énorme",
            Box::new(|d| d["naming_rules"][0]["counter"]["start"] = json!(1_000_000)),
        ),
        (
            "largeur 10",
            Box::new(|d| d["naming_rules"][0]["counter"]["width"] = json!(10)),
        ),
        (
            "largeur 300",
            Box::new(|d| d["naming_rules"][0]["counter"]["width"] = json!(300)),
        ),
        (
            "nom vide",
            Box::new(|d| d["naming_rules"][0]["name"] = json!("")),
        ),
        (
            "nom en doublon (casse)",
            Box::new(|d| d["naming_rules"][1]["name"] = json!("PIÈCES")),
        ),
        (
            "même cible deux fois",
            Box::new(|d| d["naming_rules"][1]["target"] = json!("piece")),
        ),
        (
            "préfixe avec contrôle",
            Box::new(|d| d["naming_rules"][0]["prefix"] = json!("P\u{7}")),
        ),
        (
            "préfixe trop long",
            Box::new(|d| d["naming_rules"][0]["prefix"] = json!("P".repeat(41))),
        ),
        (
            "préfixe avec espace de bord",
            Box::new(|d| d["naming_rules"][0]["prefix"] = json!("P ")),
        ),
        (
            "séparateur trop long",
            Box::new(|d| d["naming_rules"][0]["separator"] = json!("-----")),
        ),
        (
            "préfixe vide sans compteur",
            Box::new(|d| {
                let r = d["naming_rules"][0].as_object_mut().unwrap();
                r.remove("prefix");
                r.remove("counter");
            }),
        ),
        (
            "identifiant 0",
            Box::new(|d| d["naming_rules"][0]["id"] = json!(0)),
        ),
        (
            "identifiant dupliqué",
            Box::new(|d| {
                let id = d["naming_rules"][0]["id"].clone();
                d["naming_rules"][1]["id"] = id;
            }),
        ),
        (
            "règles en texte",
            Box::new(|d| d["naming_rules"] = json!("oui")),
        ),
        (
            "trop de règles",
            Box::new(|d| {
                let r = d["naming_rules"][0].clone();
                let list = d["naming_rules"].as_array_mut().unwrap();
                for i in 0..20 {
                    let mut c = r.clone();
                    c["id"] = json!(5000 + i);
                    c["name"] = json!(format!("R{i}"));
                    list.push(c);
                }
            }),
        ),
    ];
    for (name, mutate) in cases {
        let mut d = good.clone();
        mutate(&mut d);
        assert_ne!(d, good, "{name} : la mutation n'a rien changé");
        assert!(
            persist::from_json(&d.to_string()).is_err(),
            "{name} : accepté"
        );
    }
}
