//! Lecteur CSV **indépendant** (RFC 4180, séparateur `;`, BOM UTF-8 facultatif) pour les tests du
//! rapport de contrôle (US-08.5). Il ne partage aucun code avec l'écrivain de `pf-export` : c'est
//! un petit automate à états qui ne connaît que la grammaire du format.
#![allow(dead_code)]

use std::collections::BTreeMap;

/// Contenu d'un fichier CSV en lignes de champs. Panique si le fichier n'est pas du CSV bien
/// formé (guillemet jamais fermé, caractère après un guillemet fermant, fin de ligne non CR LF).
pub fn parse(bytes: &[u8]) -> Vec<Vec<String>> {
    let text = std::str::from_utf8(bytes).expect("UTF-8");
    let text = text.strip_prefix('\u{FEFF}').unwrap_or(text);
    let chars: Vec<char> = text.chars().collect();
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut i = 0;
    let mut in_quotes = false;
    let mut after_quote = false;
    let mut at_line_start = true;
    while i < chars.len() {
        let c = chars[i];
        if in_quotes {
            if c == '"' {
                if chars.get(i + 1) == Some(&'"') {
                    field.push('"');
                    i += 2;
                    continue;
                }
                in_quotes = false;
                after_quote = true;
            } else {
                field.push(c);
            }
            i += 1;
            continue;
        }
        match c {
            '"' => {
                assert!(
                    field.is_empty() && !after_quote,
                    "guillemet au milieu d'un champ non protégé (ligne {})",
                    rows.len() + 1
                );
                in_quotes = true;
                at_line_start = false;
            }
            ';' => {
                row.push(std::mem::take(&mut field));
                after_quote = false;
                at_line_start = false;
            }
            '\r' => {
                assert_eq!(
                    chars.get(i + 1),
                    Some(&'\n'),
                    "fin de ligne : CR LF attendu (ligne {})",
                    rows.len() + 1
                );
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
                after_quote = false;
                at_line_start = true;
                i += 1;
            }
            '\n' => panic!("LF sans CR (ligne {})", rows.len() + 1),
            _ => {
                assert!(!after_quote, "caractère après un guillemet fermant");
                field.push(c);
                at_line_start = false;
            }
        }
        i += 1;
    }
    assert!(!in_quotes, "guillemet jamais fermé");
    assert!(at_line_start, "le fichier doit finir par CR LF");
    rows
}

/// Une section du rapport : code, ligne d'en-tête (sans le `#code`) et lignes de données
/// (sans le code de section).
#[derive(Debug, Clone, PartialEq)]
pub struct Section {
    pub header: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

/// Regroupe les lignes par section : un en-tête commence par `#code`, une ligne de données par le
/// code. Panique sur une ligne de donnée sans en-tête, ou dont la largeur diffère de l'en-tête.
pub fn sections(rows: &[Vec<String>]) -> BTreeMap<String, Section> {
    let mut out: BTreeMap<String, Section> = BTreeMap::new();
    let mut current: Option<String> = None;
    for r in rows {
        let first = r.first().expect("ligne non vide");
        if let Some(code) = first.strip_prefix('#') {
            assert!(
                out.insert(
                    code.to_string(),
                    Section {
                        header: r[1..].to_vec(),
                        rows: Vec::new()
                    }
                )
                .is_none(),
                "section {code} deux fois"
            );
            current = Some(code.to_string());
        } else {
            let code = current.as_ref().expect("ligne avant tout en-tête");
            assert_eq!(first, code, "ligne de donnée dans la mauvaise section");
            let s = out.get_mut(code).unwrap();
            assert_eq!(
                r.len() - 1,
                s.header.len(),
                "largeur de ligne différente de l'en-tête ({code})"
            );
            s.rows.push(r[1..].to_vec());
        }
    }
    out
}

/// Nombre lu dans un champ numérique (vide = `None`) ; panique sur `NaN`, `inf` ou texte.
pub fn number(field: &str) -> Option<f64> {
    if field.is_empty() {
        return None;
    }
    assert!(
        field
            .chars()
            .all(|c| c.is_ascii_digit() || c == '.' || c == '-'),
        "champ numérique illisible : {field:?}"
    );
    let v: f64 = field.parse().expect("nombre");
    assert!(v.is_finite());
    Some(v)
}
