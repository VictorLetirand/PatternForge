//! `pf-validate` : valide un fichier `.pforge.json` en lecture seule (US-12.7).
//!
//! ```text
//! pf-validate FICHIER [--json]
//! ```
//!
//! Le verdict vient du **même chargeur que l'application** (`pf_core::persist::load`) :
//! mêmes règles de version, de champs, de références et de validation métier. Le fichier
//! n'est jamais modifié (aucune écriture, aucune sauvegarde, aucune migration sur disque).
//!
//! Codes de sortie : 0 = valide, 1 = invalide, 2 = fichier illisible ou mauvais usage.
//! Les textes sont écrits ici en français (le binaire n'est pas dans l'interface egui).

use pf_core::background_image::{status, ImageStatus};
use pf_core::diagnose::classify;
use pf_core::{persist, SCHEMA_VERSION};
use serde_json::{json, Value};
use std::path::Path;
use std::process::ExitCode;

const USAGE: &str = "Usage : pf-validate FICHIER.pforge.json [--json]\n\
\n\
Valide un projet PatternForge avec le chargeur de l'application, sans jamais modifier le fichier.\n\
  --json    écrit un seul objet JSON (clés triées) sur la sortie standard\n\
  --aide    affiche ce texte\n\
Codes de sortie : 0 valide, 1 invalide, 2 fichier illisible ou usage incorrect.";

fn main() -> ExitCode {
    let mut path: Option<String> = None;
    let mut as_json = false;
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--json" => as_json = true,
            "--aide" | "--help" | "-h" => {
                println!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            a if a.starts_with("--") => return usage(&format!("option inconnue : {a}")),
            _ if path.is_none() => path = Some(arg),
            _ => return usage("un seul fichier à la fois"),
        }
    }
    let Some(path) = path else {
        return usage("fichier manquant");
    };
    ExitCode::from(validate(&path, as_json))
}

fn usage(why: &str) -> ExitCode {
    eprintln!("pf-validate : {why}\n{USAGE}");
    ExitCode::from(2)
}

/// Valide `path`, écrit le rapport, renvoie le code de sortie.
fn validate(path: &str, as_json: bool) -> u8 {
    let result = persist::load(Path::new(path));
    // Relecture du texte pour la version d'origine et la localisation (lecture seule).
    let text = std::fs::read_to_string(path).ok();
    let text = text.as_deref().map(|t| t.trim_start_matches('\u{feff}'));
    let file_version = text
        .and_then(|t| serde_json::from_str::<Value>(t).ok())
        .and_then(|v| v.get("schema_version").and_then(Value::as_u64));

    match result {
        Ok(project) => {
            // Image de fond (E14) : la validation du fichier ne dépend pas de l'image, mais son
            // état (introuvable / modifiée) est signalé sans changer le code de sortie.
            let backgrounds = background_findings(Path::new(path), &project);
            let migrated = file_version.is_some_and(|v| v != u64::from(SCHEMA_VERSION));
            if as_json {
                let mut report = json!({
                    "file": path,
                    "valid": true,
                    "file_schema_version": file_version,
                    "app_schema_version": SCHEMA_VERSION,
                    "migrated_in_memory": migrated,
                    "project": {
                        "name": project.name,
                        "sheets": project.sheets.len(),
                        "pieces": project.pieces.len(),
                    },
                    "migration_notes": project.migration_notes,
                    "errors": [],
                });
                if !backgrounds.is_empty() {
                    report["backgrounds"] = Value::Array(
                        backgrounds
                            .iter()
                            .map(|b| {
                                json!({
                                    "sheet": b.sheet,
                                    "image": b.image,
                                    "status": b.status,
                                    "residual_mm": b.residual_mm,
                                })
                            })
                            .collect(),
                    );
                }
                print_json(&report);
            } else {
                println!(
                    "{path} : valide — projet « {} », {} feuille(s), {} pièce(s).",
                    project.name,
                    project.sheets.len(),
                    project.pieces.len()
                );
                if migrated {
                    println!(
                        "  Remarque : fichier en version {}, relu comme version {SCHEMA_VERSION} \
                         en mémoire ; le fichier n'a pas été modifié.",
                        file_version.unwrap_or(0)
                    );
                }
                for note in &project.migration_notes {
                    println!("  Note de migration : {note}");
                }
                for b in &backgrounds {
                    println!(
                        "  Image de fond (feuille « {} », « {} ») : {}, résidu de calibration {:.6} mm.",
                        b.sheet,
                        b.image,
                        match b.status {
                            "ok" => "image présente, empreinte conforme",
                            "missing" => "image introuvable",
                            "modified" => "image modifiée (empreinte différente)",
                            _ => "image illisible",
                        },
                        b.residual_mm
                    );
                }
            }
            0
        }
        Err(error) => {
            let finding = classify(&error, text, file_version);
            let code = if finding.kind == "lecture" { 2 } else { 1 };
            if as_json {
                print_json(&json!({
                    "file": path,
                    "valid": false,
                    "file_schema_version": file_version,
                    "app_schema_version": SCHEMA_VERSION,
                    "errors": [{
                        "kind": finding.kind,
                        "message": finding.message,
                        "line": finding.line,
                        "column": finding.column,
                    }],
                }));
            } else {
                let place = match (finding.line, finding.column) {
                    (Some(l), Some(c)) => format!(" (ligne {l}, colonne {c})"),
                    _ => String::new(),
                };
                println!("{path} : invalide (1 erreur)");
                println!("  [{}] {}{place}", finding.kind, finding.message);
            }
            code
        }
    }
}

struct BackgroundFinding {
    sheet: String,
    image: String,
    status: &'static str,
    residual_mm: f64,
}

/// État du fichier de chaque image de fond, cherché à côté du projet (lecture seule).
fn background_findings(path: &Path, project: &pf_core::Project) -> Vec<BackgroundFinding> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    project
        .sheets
        .iter()
        .filter_map(|s| {
            let b = s.background.as_ref()?;
            let status = match status(dir, b) {
                ImageStatus::Ok => "ok",
                ImageStatus::Missing => "missing",
                ImageStatus::Modified => "modified",
                ImageStatus::Unreadable(_) => "unreadable",
            };
            let residual_mm = b.residuals().map(|r| r.rms_mm).unwrap_or(f64::NAN);
            Some(BackgroundFinding {
                sheet: s.name.clone(),
                image: b.image.clone(),
                status,
                residual_mm,
            })
        })
        .collect()
}

fn print_json(value: &Value) {
    // `Value` écrit ses clés triées (BTreeMap) : sortie déterministe.
    println!("{}", serde_json::to_string(value).unwrap_or_default());
}
