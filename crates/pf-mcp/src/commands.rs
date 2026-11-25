//! Outil `list_commands` : noms et schéma JSON de chaque variante de `pf_core::Command`.
//!
//! Le catalogue est extrait du source de `Command` à la compilation (`build.rs`) : une variante
//! ajoutée apparaît ici sans modifier ce fichier. Les schémas décrivent les champs et leurs types
//! Rust ; le type JSON est déduit quand il est évident (nombre, chaîne, booléen, tableau,
//! identifiant), sinon seul le type Rust est indiqué (`x-rust-type`).

use serde_json::{json, Value};

pub(crate) struct FieldSpec {
    pub name: &'static str,
    pub ty: &'static str,
    pub required: bool,
}

pub(crate) struct CommandSpec {
    pub name: &'static str,
    pub doc: &'static str,
    pub fields: &'static [FieldSpec],
}

include!(concat!(env!("OUT_DIR"), "/command_catalog.rs"));

/// Variantes qui ont aussi un outil de haut niveau : (variante, outil).
pub(crate) const DEDICATED: [(&str, &str); 2] = [
    ("extract_piece", "extract_piece"),
    ("set_seam_allowance", "apply_seam_allowance"),
];

fn type_schema(ty: &str) -> Value {
    let ty = ty.trim();
    if let Some(inner) = ty.strip_prefix("Option<").and_then(|t| t.strip_suffix('>')) {
        let mut s = type_schema(inner);
        if let Some(o) = s.as_object_mut() {
            o.insert("nullable".into(), json!(true));
        }
        return s;
    }
    if let Some(inner) = ty.strip_prefix("Box<").and_then(|t| t.strip_suffix('>')) {
        return type_schema(inner);
    }
    if let Some(inner) = ty.strip_prefix("Vec<").and_then(|t| t.strip_suffix('>')) {
        return json!({"type": "array", "items": type_schema(inner), "x-rust-type": ty});
    }
    let json_type = match ty {
        "String" => Some("string"),
        "bool" => Some("boolean"),
        "f64" | "f32" => Some("number"),
        "u8" | "u16" | "u32" | "u64" | "usize" | "i8" | "i16" | "i32" | "i64" | "isize" => {
            Some("integer")
        }
        t if t.ends_with("Id") && !t.contains("::") && !t.contains('<') => Some("integer"),
        "PointMm" => Some("object"),
        _ => None,
    };
    match json_type {
        Some(t) => json!({"type": t, "x-rust-type": ty}),
        None => json!({"x-rust-type": ty}),
    }
}

fn spec_json(c: &CommandSpec) -> Value {
    let mut properties = serde_json::Map::new();
    properties.insert("type".into(), json!({"const": c.name}));
    let mut required = vec![json!("type")];
    for f in c.fields {
        properties.insert(f.name.into(), type_schema(f.ty));
        if f.required {
            required.push(json!(f.name));
        }
    }
    let dedicated = DEDICATED
        .iter()
        .find(|(v, _)| *v == c.name)
        .map(|(_, t)| *t);
    json!({
        "name": c.name,
        "description": c.doc,
        "dedicated_tool": dedicated,
        "schema": {
            "type": "object",
            "properties": properties,
            "required": required,
            "additionalProperties": false
        }
    })
}

pub(crate) fn names() -> Vec<&'static str> {
    CATALOG.iter().map(|c| c.name).collect()
}

pub(crate) fn definition() -> Value {
    json!({
        "name": "list_commands",
        "description": "Liste toutes les commandes du modèle (nom et schéma JSON des champs) acceptées par apply_command et preview_command. Le schéma est extrait du type Rust : les types non élémentaires ne sont décrits que par x-rust-type (forme serde de pf-core).",
        "inputSchema": {
            "type": "object",
            "properties": {},
            "required": [],
            "additionalProperties": false
        },
        "annotations": {"readOnlyHint": true}
    })
}

pub(crate) fn list() -> Value {
    json!({
        "count": CATALOG.len(),
        "tag": "type",
        "commands": CATALOG.iter().map(spec_json).collect::<Vec<_>>()
    })
}

/// Noms des variantes, pour le test de recoupement avec `serde`.
pub fn command_names() -> Vec<&'static str> {
    names()
}
