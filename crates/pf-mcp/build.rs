//! Catalogue des variantes de `pf_core::Command`, extrait du source du type.
//!
//! Aucune dépendance : le fichier `command.rs` est lu ligne à ligne (variantes à champs nommés,
//! un champ par ligne). Toute ligne inattendue fait échouer la compilation, jamais de catalogue
//! silencieusement incomplet. Un test (`tests/command_coverage.rs`) recoupe ce catalogue avec la
//! liste que `serde` donne lui-même pour `Command`.

use std::fmt::Write as _;
use std::path::PathBuf;

struct Field {
    name: String,
    ty: String,
    required: bool,
}

struct Variant {
    name: String,
    doc: String,
    fields: Vec<Field>,
}

fn snake(name: &str) -> String {
    let mut out = String::new();
    for (i, c) in name.chars().enumerate() {
        if c.is_ascii_uppercase() {
            if i > 0 {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("manifeste"));
    let source = manifest.join("../pf-core/src/command.rs");
    println!("cargo:rerun-if-changed={}", source.display());
    let text = std::fs::read_to_string(&source).expect("lecture de command.rs");
    let mut variants: Vec<Variant> = Vec::new();
    let mut in_enum = false;
    let mut docs: Vec<String> = Vec::new();
    let mut current: Option<Variant> = None;
    let mut default_attr = false;
    for (n, line) in text.lines().enumerate() {
        let line = line.trim_end();
        if !in_enum {
            in_enum = line == "pub enum Command {";
            continue;
        }
        if line == "}" {
            break;
        }
        let at = |msg: &str| -> String { format!("command.rs:{} : {msg} : {line}", n + 1) };
        let indent = line.len() - line.trim_start().len();
        let body = line.trim_start();
        match (indent, current.is_some()) {
            (_, _) if body.is_empty() => {}
            (4, false) if body.starts_with("///") => {
                docs.push(body.trim_start_matches('/').trim().to_string());
            }
            (4, false) if body.starts_with("#[") => {}
            (4, false) => {
                let name = body
                    .strip_suffix(" {")
                    .unwrap_or_else(|| panic!("{}", at("variante à champs nommés attendue")));
                current = Some(Variant {
                    name: name.to_string(),
                    doc: docs.join(" "),
                    fields: Vec::new(),
                });
                docs.clear();
            }
            (4, true) => {
                assert!(body == "},", "{}", at("fin de variante attendue"));
                variants.extend(current.take());
                default_attr = false;
            }
            (8, true) if body.starts_with("///") => {}
            (8, true) if body.starts_with("#[serde(") => {
                default_attr = default_attr || body.contains("default");
            }
            (8, true) => {
                let rest = body
                    .strip_suffix(',')
                    .unwrap_or_else(|| panic!("{}", at("champ sur une seule ligne attendu")));
                let (name, ty) = rest
                    .split_once(": ")
                    .unwrap_or_else(|| panic!("{}", at("champ « nom: type » attendu")));
                let required = !default_attr && !ty.starts_with("Option<");
                if let Some(v) = current.as_mut() {
                    v.fields.push(Field {
                        name: name.to_string(),
                        ty: ty.to_string(),
                        required,
                    });
                }
                default_attr = false;
            }
            _ => panic!("{}", at("ligne inattendue")),
        }
    }
    assert!(
        current.is_none() && !variants.is_empty(),
        "catalogue vide ou tronqué"
    );

    let mut out = String::from("pub(crate) const CATALOG: &[CommandSpec] = &[\n");
    for v in &variants {
        let _ = writeln!(
            out,
            "    CommandSpec {{ name: {:?}, doc: {:?}, fields: &[",
            snake(&v.name),
            v.doc
        );
        for f in &v.fields {
            let _ = writeln!(
                out,
                "        FieldSpec {{ name: {:?}, ty: {:?}, required: {} }},",
                f.name, f.ty, f.required
            );
        }
        out.push_str("    ] },\n");
    }
    out.push_str("];\n");
    let dest = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR")).join("command_catalog.rs");
    std::fs::write(dest, out).expect("écriture du catalogue");
}
