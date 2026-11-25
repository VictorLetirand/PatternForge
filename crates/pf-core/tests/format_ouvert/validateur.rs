//! Mini-validateur JSON Schema (sous-ensemble de la draft 2020-12), **écrit pour les tests** et
//! indépendant du code de production : il ne lit ni `pf_core` ni `persist`, seulement le schéma
//! `docs/pforge-v3.schema.json` et un document JSON.
//!
//! Mots-clés gérés : `$ref` (local, `#/...`), `type`, `const`, `enum`, `required`, `properties`,
//! `additionalProperties` (booléen), `items`, `minItems`, `maxItems`, `minimum`, `maximum`,
//! `oneOf`. Les annotations (`$schema`, `$id`, `title`, `description`, `$comment`, `$defs`) sont
//! ignorées. **Tout autre mot-clé fait paniquer** : un schéma qui utilise un mot-clé non géré ne
//! peut pas être validé en silence.
//!
//! Sémantique des nombres (spec JSON Schema) : égalité mathématique (`3.0` = `3`), `integer` =
//! nombre sans partie fractionnaire. Le chargeur serde, lui, refuse `3.0` pour un entier : c'est
//! un écart connu (voir `docs/FORMAT_PFORGE_V3.md`).

use serde_json::Value;

const ANNOTATIONS: [&str; 6] = [
    "$schema",
    "$id",
    "title",
    "description",
    "$comment",
    "$defs",
];

/// Erreurs de `doc` contre `schema` (vide = valide). Chaque erreur commence par le pointeur JSON.
pub fn validate(schema: &Value, doc: &Value) -> Vec<String> {
    let mut errors = Vec::new();
    check(schema, schema, doc, "", &mut errors);
    errors
}

fn resolve<'a>(root: &'a Value, reference: &str) -> &'a Value {
    let path = reference
        .strip_prefix("#/")
        .unwrap_or_else(|| panic!("$ref non local : {reference}"));
    path.split('/').fold(root, |v, key| {
        v.get(key)
            .unwrap_or_else(|| panic!("$ref introuvable : {reference}"))
    })
}

fn num_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.as_f64() == y.as_f64(),
        _ => a == b,
    }
}

fn type_ok(name: &str, doc: &Value) -> bool {
    match name {
        "object" => doc.is_object(),
        "array" => doc.is_array(),
        "string" => doc.is_string(),
        "boolean" => doc.is_boolean(),
        "null" => doc.is_null(),
        "number" => doc.is_number(),
        "integer" => doc
            .as_f64()
            .is_some_and(|f| f.is_finite() && f.fract() == 0.0),
        other => panic!("type non géré : {other}"),
    }
}

fn check(root: &Value, schema: &Value, doc: &Value, at: &str, errors: &mut Vec<String>) {
    let obj = match schema {
        Value::Bool(true) => return,
        Value::Bool(false) => {
            errors.push(format!("{at} : refusé par un schéma « false »"));
            return;
        }
        Value::Object(o) => o,
        other => panic!("schéma invalide : {other}"),
    };
    for (key, arg) in obj {
        let key = key.as_str();
        if ANNOTATIONS.contains(&key) {
            continue;
        }
        match key {
            "$ref" => {
                let target = resolve(root, arg.as_str().expect("$ref texte"));
                check(root, target, doc, at, errors);
            }
            "type" => {
                let ok = match arg {
                    Value::String(t) => type_ok(t, doc),
                    Value::Array(ts) => ts.iter().any(|t| type_ok(t.as_str().unwrap(), doc)),
                    _ => panic!("type invalide"),
                };
                if !ok {
                    errors.push(format!("{at} : type attendu {arg}"));
                }
            }
            "const" => {
                if !num_eq(arg, doc) {
                    errors.push(format!("{at} : valeur attendue {arg}"));
                }
            }
            "enum" => {
                if !arg.as_array().unwrap().iter().any(|v| num_eq(v, doc)) {
                    errors.push(format!("{at} : valeur hors énumération"));
                }
            }
            "required" => {
                if let Some(o) = doc.as_object() {
                    for name in arg.as_array().unwrap() {
                        let name = name.as_str().unwrap();
                        if !o.contains_key(name) {
                            errors.push(format!("{at} : champ requis « {name} » absent"));
                        }
                    }
                }
            }
            "properties" => {
                if let Some(o) = doc.as_object() {
                    for (name, sub) in arg.as_object().unwrap() {
                        if let Some(v) = o.get(name) {
                            check(root, sub, v, &format!("{at}/{name}"), errors);
                        }
                    }
                }
            }
            "additionalProperties" => {
                let allowed = match arg {
                    Value::Bool(b) => *b,
                    _ => panic!("additionalProperties : seul un booléen est géré"),
                };
                if let (false, Some(o)) = (allowed, doc.as_object()) {
                    let known = obj.get("properties").and_then(Value::as_object);
                    for name in o.keys() {
                        if !known.is_some_and(|k| k.contains_key(name)) {
                            errors.push(format!("{at} : champ inconnu « {name} »"));
                        }
                    }
                }
            }
            "items" => {
                if let Some(a) = doc.as_array() {
                    for (i, v) in a.iter().enumerate() {
                        check(root, arg, v, &format!("{at}/{i}"), errors);
                    }
                }
            }
            "minItems" | "maxItems" => {
                if let Some(a) = doc.as_array() {
                    let n = arg.as_u64().unwrap() as usize;
                    let bad = if key == "minItems" {
                        a.len() < n
                    } else {
                        a.len() > n
                    };
                    if bad {
                        errors.push(format!("{at} : {key} {n} non respecté"));
                    }
                }
            }
            "minLength" | "maxLength" => {
                if let Some(s) = doc.as_str() {
                    let n = arg.as_u64().unwrap() as usize;
                    let len = s.chars().count();
                    let bad = if key == "minLength" { len < n } else { len > n };
                    if bad {
                        errors.push(format!("{at} : {key} {n} non respecté"));
                    }
                }
            }
            "pattern" => {
                if let Some(s) = doc.as_str() {
                    if !pattern_matches(arg.as_str().unwrap(), s) {
                        errors.push(format!("{at} : ne respecte pas le motif {arg}"));
                    }
                }
            }
            "minimum" | "maximum" => {
                if let (Some(x), Some(limit)) = (doc.as_f64(), arg.as_f64()) {
                    let bad = if key == "minimum" {
                        x < limit
                    } else {
                        x > limit
                    };
                    if bad {
                        errors.push(format!("{at} : {key} {limit} non respecté"));
                    }
                }
            }
            "oneOf" => {
                let matching = arg
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|sub| {
                        let mut e = Vec::new();
                        check(root, sub, doc, at, &mut e);
                        e.is_empty()
                    })
                    .count();
                if matching != 1 {
                    errors.push(format!(
                        "{at} : {matching} variante(s) valide(s) au lieu d'une"
                    ));
                }
            }
            other => panic!("mot-clé de schéma non géré par le mini-validateur : {other}"),
        }
    }
}

/// Motif minimal pour `pattern` : `^` puis une suite de classes `[a-zA-Z_0-9]` (plages et
/// caractères isolés) chacune suivie ou non de `*`, puis `$`. Tout autre motif est refusé
/// (panique) plutôt que mal compris.
fn pattern_matches(pattern: &str, text: &str) -> bool {
    let body = pattern
        .strip_prefix('^')
        .and_then(|p| p.strip_suffix('$'))
        .unwrap_or_else(|| panic!("motif non géré (^…$ attendu) : {pattern}"));
    let mut atoms: Vec<(Vec<(char, char)>, bool)> = Vec::new();
    let chars: Vec<char> = body.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        assert_eq!(chars[i], '[', "motif non géré : {pattern}");
        i += 1;
        let mut ranges = Vec::new();
        while chars[i] != ']' {
            if chars.get(i + 1) == Some(&'-') && chars[i + 2] != ']' {
                ranges.push((chars[i], chars[i + 2]));
                i += 3;
            } else {
                ranges.push((chars[i], chars[i]));
                i += 1;
            }
        }
        i += 1;
        let star = chars.get(i) == Some(&'*');
        if star {
            i += 1;
        }
        atoms.push((ranges, star));
    }
    fn go(atoms: &[(Vec<(char, char)>, bool)], text: &[char]) -> bool {
        let Some(((ranges, star), rest)) = atoms.split_first() else {
            return text.is_empty();
        };
        let ok = |c: char| ranges.iter().any(|(a, b)| *a <= c && c <= *b);
        if *star {
            let mut n = 0;
            loop {
                if go(rest, &text[n..]) {
                    return true;
                }
                if n < text.len() && ok(text[n]) {
                    n += 1;
                } else {
                    return false;
                }
            }
        }
        !text.is_empty() && ok(text[0]) && go(rest, &text[1..])
    }
    go(&atoms, &text.chars().collect::<Vec<_>>())
}
