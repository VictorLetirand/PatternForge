//! US-03.1a — table de variables : graphe, commandes atomiques, annuler / rétablir, ouverture
//! d'un fichier (valide et chaque invalidité), `pf-validate`.

use pf_core::expr::ExprErrorKind;
use pf_core::persist;
use pf_core::*;
use serde_json::{json, Value};

fn bytes(p: &Project) -> String {
    persist::to_json(p).unwrap()
}

fn add(name: &str, expr: &str) -> Command {
    Command::AddVariable {
        name: name.into(),
        unit: VariableUnit::Mm,
        expr: expr.into(),
    }
}

fn set(name: &str, expr: &str) -> Command {
    Command::SetVariableExpr {
        name: name.into(),
        expr: expr.into(),
    }
}

fn rename(from: &str, to: &str) -> Command {
    Command::RenameVariable {
        from: from.into(),
        to: to.into(),
    }
}

fn remove(name: &str) -> Command {
    Command::RemoveVariable { name: name.into() }
}

fn values(p: &Project) -> Vec<(String, f64)> {
    p.variable_values().unwrap()
}

fn expr_of<'a>(p: &'a Project, name: &str) -> &'a str {
    &p.variables.iter().find(|v| v.name == name).unwrap().expr
}

/// Commande refusée : message attendu, projet **identique à l'octet**.
fn refused(p: &mut Project, cmd: Command, needle: &str) {
    let before = bytes(p);
    let before_p = p.clone();
    let e = p.apply(cmd).expect_err("devrait être refusée").to_string();
    assert!(e.contains(needle), "message « {e} » sans « {needle} »");
    assert_eq!(bytes(p), before, "trace laissée par un refus");
    assert_eq!(*p, before_p);
}

/// A (premier de la table) = B + 1, B = C * 2, C = 5 : ordre de table inverse des dépendances.
fn chain() -> Project {
    let mut p = Project::new("Chaîne");
    for (n, e) in [("A", "0"), ("B", "0"), ("C", "5")] {
        p.apply(add(n, e)).unwrap();
    }
    p.apply(set("A", "B+1")).unwrap();
    p.apply(set("B", "C*2")).unwrap();
    p
}

#[test]
fn chaine_evaluee_dans_l_ordre_des_dependances() {
    let p = chain();
    assert_eq!(
        values(&p),
        vec![("A".into(), 11.0), ("B".into(), 10.0), ("C".into(), 5.0)]
    );
    let mut p = p;
    p.apply(set("C", "6,5")).unwrap();
    assert_eq!(values(&p)[0], ("A".into(), 14.0));
}

#[test]
fn ajout_et_mesures_de_patron() {
    let mut p = Project::new("Mesures");
    p.apply(add("poitrine", "920")).unwrap();
    p.apply(add("aisance", "40")).unwrap();
    p.apply(add("largeur", "(poitrine+aisance)/4")).unwrap();
    p.apply(Command::SetVariableUnit {
        name: "largeur".into(),
        unit: VariableUnit::Unitless,
    })
    .unwrap();
    assert_eq!(values(&p)[2], ("largeur".into(), 240.0));
    assert_eq!(p.variables[2].unit, VariableUnit::Unitless);
    // L'unité est une étiquette : la valeur ne change pas.
    p.apply(Command::SetVariableUnit {
        name: "largeur".into(),
        unit: VariableUnit::Deg,
    })
    .unwrap();
    assert_eq!(values(&p)[2].1, 240.0);
}

#[test]
fn cycles_refuses_avec_leur_chemin() {
    // Cycle à 2.
    let mut p = Project::new("Cycles");
    p.apply(add("A", "1")).unwrap();
    p.apply(add("B", "A+1")).unwrap();
    refused(&mut p, set("A", "B+1"), "A → B → A");
    // Cycle à 3.
    p.apply(add("C", "B+1")).unwrap();
    refused(&mut p, set("A", "C+1"), "A → C → B → A");
    // Auto-référence.
    refused(&mut p, set("A", "A+1"), "A → A");
    refused(&mut p, set("C", "C"), "C → C");
    // Le message est localisable : catégorie Cycle, position de la référence fautive.
    let mut q = p.clone();
    q.variables[0].expr = "C+1".into();
    match q.variable_values().unwrap_err() {
        VariableError::Expr { error, .. } => {
            assert_eq!(error.kind, ExprErrorKind::Cycle);
            assert_eq!(error.name.as_deref(), Some("A → C → B → A"));
        }
        e => panic!("{e}"),
    }
}

#[test]
fn reference_inconnue_et_expression_invalide_refusees() {
    let mut p = Project::new("Refus");
    p.apply(add("A", "1")).unwrap();
    refused(&mut p, add("B", "Z+1"), "« Z » inconnue");
    refused(&mut p, add("B", "a+1"), "la casse compte");
    refused(&mut p, set("A", "2+"), "caractère 3");
    refused(&mut p, set("A", "1/0"), "division par zéro");
    refused(&mut p, set("A", "sqrt(-4)"), "négatif");
    refused(&mut p, set("A", ""), "vide");
    refused(&mut p, set("A", &"9".repeat(400)), "trop grand");
    refused(&mut p, set("A", &"1+".repeat(300)), "trop longue");
    refused(&mut p, set("Z", "1"), "introuvable");
    // Débordement à l'évaluation, par un autre chemin que le littéral.
    let d = "9".repeat(150);
    refused(&mut p, set("A", &format!("{d}*{d}*{d}")), "débordement");
    assert_eq!(values(&p), vec![("A".into(), 1.0)]);
}

#[test]
fn noms_valides_uniques_non_reserves() {
    let mut p = Project::new("Noms");
    p.apply(add("Tour", "1")).unwrap();
    refused(&mut p, add("Tour", "2"), "existe déjà");
    refused(&mut p, add("tour", "2"), "que par la casse");
    refused(&mut p, add("TOUR", "2"), "que par la casse");
    for bad in ["", " x", "x y", "1x", "é", "tour_é", "a-b", "a.b", "x\u{0}"] {
        refused(&mut p, add(bad, "1"), "nom de variable");
    }
    for reserved in ["min", "max", "abs", "sqrt", "pi", "PI", "Min", "SQRT"] {
        refused(&mut p, add(reserved, "1"), "réservé");
    }
    let forty = "a".repeat(40);
    p.apply(add(&forty, "1")).unwrap();
    refused(&mut p, add(&"a".repeat(41), "1"), "40");
    p.apply(add("_x9", "1")).unwrap();
    p.apply(add("pi2", "pi")).unwrap();
    p.apply(add("minimum", "1")).unwrap();
}

#[test]
fn limite_de_500_variables() {
    let mut p = Project::new("Limite");
    for i in 0..MAX_VARIABLES {
        p.variables.push(Variable {
            name: format!("v{i}"),
            unit: VariableUnit::Unitless,
            expr: "1".into(),
            per_size: Vec::new(),
        });
    }
    assert_eq!(p.variable_values().unwrap().len(), 500);
    refused(&mut p, add("trop", "1"), "trop de variables");
}

#[test]
fn renommage_reecrit_les_references_aux_frontieres_de_mot() {
    let mut p = Project::new("Renommage");
    p.apply(add("B", "1")).unwrap();
    p.apply(add("A", "B+1")).unwrap();
    p.apply(add("B22", "2")).unwrap();
    p.apply(add("AB", "B22+A*B")).unwrap();
    p.apply(add("B_", "min(B;B22)")).unwrap();
    let before = values(&p);
    p.apply(rename("B", "B2")).unwrap();
    assert_eq!(expr_of(&p, "A"), "B2+1");
    assert_eq!(expr_of(&p, "AB"), "B22+A*B2");
    assert_eq!(expr_of(&p, "B_"), "min(B2;B22)");
    assert_eq!(expr_of(&p, "B22"), "2");
    assert_eq!(p.variables[0].name, "B2");
    // Les valeurs ne changent pas (même ordre de table).
    let after: Vec<f64> = values(&p).into_iter().map(|v| v.1).collect();
    assert_eq!(after, before.iter().map(|v| v.1).collect::<Vec<_>>());
    // Conflits : nom pris (casse comprise), réservé, invalide, inconnu.
    refused(&mut p, rename("B2", "A"), "existe déjà");
    refused(&mut p, rename("B2", "a"), "que par la casse");
    refused(&mut p, rename("B2", "min"), "réservé");
    refused(&mut p, rename("B2", "é"), "nom de variable");
    refused(&mut p, rename("Zz", "Yy"), "introuvable");
    // Seule la casse change : permis, références réécrites.
    p.apply(rename("B2", "b2")).unwrap();
    assert_eq!(expr_of(&p, "A"), "b2+1");
    // Même nom : sans effet.
    let n = bytes(&p);
    p.apply(rename("b2", "b2")).unwrap();
    assert_eq!(bytes(&p), n);
}

#[test]
fn renommage_refuse_si_une_expression_devient_trop_longue() {
    let mut p = Project::new("Long");
    p.apply(add("B", "1")).unwrap();
    // 124 références de 1 caractère séparées par « + » : 247 caractères.
    let e = format!("B{}", "+B".repeat(123));
    p.apply(add("A", &e)).unwrap();
    // 40 caractères au lieu de 1 : 124 × 40 + 123 > 500, refusé en entier.
    refused(&mut p, rename("B", &"b".repeat(40)), "trop longue");
}

#[test]
fn suppression_refusee_tant_qu_une_variable_s_y_refere() {
    let mut p = chain();
    refused(&mut p, remove("C"), "« B »");
    refused(&mut p, remove("B"), "« A »");
    refused(&mut p, remove("Q"), "introuvable");
    assert_eq!(p.variable_dependents("C"), vec!["B".to_string()]);
    p.apply(remove("A")).unwrap();
    p.apply(remove("B")).unwrap();
    p.apply(remove("C")).unwrap();
    assert!(p.variables.is_empty());
    // Plusieurs dépendants : tous listés.
    let mut q = Project::new("Dep");
    q.apply(add("X", "1")).unwrap();
    q.apply(add("Y", "X")).unwrap();
    q.apply(add("Z", "X+Y")).unwrap();
    let e = q.apply(remove("X")).unwrap_err().to_string();
    assert!(e.contains("« Y »") && e.contains("« Z »"), "{e}");
}

#[test]
fn historique_annuler_retablir_et_sans_effet() {
    let mut doc = Document::new(Project::new("Histoire"));
    let mut states = vec![bytes(doc.project())];
    let cmds = [
        add("A", "10"),
        add("B", "A*2"),
        set("A", "12,5"),
        rename("A", "Base"),
        Command::SetVariableUnit {
            name: "Base".into(),
            unit: VariableUnit::Deg,
        },
        remove("B"),
    ];
    for c in cmds {
        doc.execute(c).unwrap();
        states.push(bytes(doc.project()));
    }
    // Chaque commande a ouvert exactement une étape.
    for i in (0..states.len() - 1).rev() {
        assert!(doc.undo());
        assert_eq!(bytes(doc.project()), states[i], "annuler → état {i}");
    }
    assert!(!doc.can_undo());
    for (i, s) in states.iter().enumerate().skip(1) {
        assert!(doc.redo());
        assert_eq!(bytes(doc.project()), *s, "rétablir → état {i}");
    }
    // Sans effet : aucune étape d'annulation, octets identiques.
    let n = bytes(doc.project());
    for c in [
        set("Base", "12,5"),
        set("Base", "  12,5  "),
        rename("Base", "Base"),
        Command::SetVariableUnit {
            name: "Base".into(),
            unit: VariableUnit::Deg,
        },
    ] {
        doc.execute(c).unwrap();
    }
    assert_eq!(bytes(doc.project()), n);
    assert!(!doc.can_redo());
    assert!(doc.undo()); // la dernière étape est bien celle de `remove("B")`
    assert_eq!(bytes(doc.project()), states[5]);
}

#[test]
fn projet_sans_variables_octets_inchanges() {
    let p = Project::new("Sans");
    let text = bytes(&p);
    assert!(!text.contains("variables"));
    let again = persist::from_json(&text).unwrap();
    assert_eq!(bytes(&again), text);
    // Ajouter puis tout retirer ramène aux octets d'origine.
    let mut q = p.clone();
    q.apply(add("A", "1")).unwrap();
    assert!(bytes(&q).contains("\"variables\""));
    q.apply(remove("A")).unwrap();
    assert_eq!(bytes(&q), text);
}

#[test]
fn aller_retour_json_et_valeurs() {
    let mut p = chain();
    p.apply(add("Angle", "45,5")).unwrap();
    p.apply(Command::SetVariableUnit {
        name: "Angle".into(),
        unit: VariableUnit::Deg,
    })
    .unwrap();
    let text = bytes(&p);
    assert!(text.contains("\"unit\": \"deg\""));
    assert!(text.contains("\"unit\": \"mm\""));
    let q = persist::from_json(&text).unwrap();
    assert_eq!(q, p);
    assert_eq!(bytes(&q), text);
    assert_eq!(values(&q), values(&p));
    let none = json!({"name":"n","unit":"none","expr":"1"});
    assert_eq!(
        serde_json::from_value::<Variable>(none).unwrap().unit,
        VariableUnit::Unitless
    );
}

/// Projet minimal sérialisé, puis `variables` remplacé par `vars`.
fn doc_with(vars: Value) -> String {
    let mut v: Value = serde_json::from_str(&bytes(&Project::new("Ouverture"))).unwrap();
    v["variables"] = vars;
    v.to_string()
}

fn var(name: &str, expr: &str) -> Value {
    json!({"name": name, "unit": "mm", "expr": expr})
}

fn open_err(vars: Value) -> String {
    persist::from_json(&doc_with(vars))
        .expect_err("devrait être refusé à l'ouverture")
        .to_string()
}

#[test]
fn ouverture_valide() {
    let ok = persist::from_json(&doc_with(json!([var("A", "B+1"), var("B", "2*3")]))).unwrap();
    assert_eq!(values(&ok), vec![("A".into(), 7.0), ("B".into(), 6.0)]);
    // Liste vide explicite : acceptée (relue comme absente).
    assert!(persist::from_json(&doc_with(json!([])))
        .unwrap()
        .variables
        .is_empty());
}

#[test]
fn ouverture_chaque_invalidite_refusee_avec_message_clair() {
    let cases: Vec<(&str, Value, &str)> = vec![
        (
            "doublon",
            json!([var("A", "1"), var("A", "2")]),
            "existe déjà",
        ),
        (
            "doublon de casse",
            json!([var("A", "1"), var("a", "2")]),
            "que par la casse",
        ),
        (
            "cycle à 2",
            json!([var("A", "B"), var("B", "A")]),
            "A → B → A",
        ),
        ("auto-référence", json!([var("A", "A+1")]), "A → A"),
        (
            "référence inconnue",
            json!([var("A", "Q")]),
            "« Q » inconnue",
        ),
        ("syntaxe", json!([var("A", "2+")]), "caractère 3"),
        (
            "division par zéro",
            json!([var("A", "1/0")]),
            "division par zéro",
        ),
        (
            "valeur non finie",
            json!([var("A", &format!("{0}*{0}*{0}", "9".repeat(150)))]),
            "débordement",
        ),
        ("expression vide", json!([var("A", "")]), "vide"),
        ("nom accentué", json!([var("é", "1")]), "nom de variable"),
        ("nom réservé", json!([var("pi", "1")]), "réservé"),
        ("nom vide", json!([var("", "1")]), "vide"),
        (
            "expression trop longue",
            json!([var("A", &"1+".repeat(300))]),
            "trop longue",
        ),
        (
            "champ inconnu",
            json!([{"name":"A","unit":"mm","expr":"1","note":"x"}]),
            "note",
        ),
        (
            "unité inconnue",
            json!([{"name":"A","unit":"cm","expr":"1"}]),
            "cm",
        ),
        ("champ manquant", json!([{"name":"A","unit":"mm"}]), "expr"),
        ("pas une liste", json!({"A": 1}), "expected a sequence"),
        (
            "nombre en lieu d'expression",
            json!([{"name":"A","unit":"mm","expr":1}]),
            "expected a string",
        ),
    ];
    for (what, vars, needle) in cases {
        let e = open_err(vars);
        assert!(e.contains(needle), "{what} : « {e} » sans « {needle} »");
    }
    // Plus de 500 variables.
    let many: Vec<Value> = (0..501).map(|i| var(&format!("v{i}"), "1")).collect();
    let e = open_err(Value::Array(many));
    assert!(e.contains("trop de variables"), "{e}");
}

fn validate_cli(text: &str, name: &str) -> (Option<i32>, String) {
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "pf-variables-validate-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, text).unwrap();
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_pf-validate"))
        .arg(&path)
        .output()
        .unwrap();
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
    )
}

#[test]
fn pf_validate_rapporte_les_variables_invalides() {
    let (code, out) = validate_cli(
        &doc_with(json!([var("A", "B"), var("B", "A")])),
        "cycle.pforge.json",
    );
    assert_eq!(code, Some(1), "{out}");
    assert!(out.contains("A → B → A"), "{out}");
    let (code, out) = validate_cli(
        &doc_with(json!([var("A", "1"), var("A", "2")])),
        "doublon.pforge.json",
    );
    assert_eq!(code, Some(1), "{out}");
    let (code, out) = validate_cli(
        &doc_with(json!([{"name":"A","unit":"mm","expr":"1","x":1}])),
        "inconnu.pforge.json",
    );
    assert_eq!(code, Some(1), "{out}");
    let (code, out) = validate_cli(&doc_with(json!([var("A", "1+1")])), "ok.pforge.json");
    assert_eq!(code, Some(0), "{out}");
}
