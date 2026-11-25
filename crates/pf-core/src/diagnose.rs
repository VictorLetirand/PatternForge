//! Diagnostic d'un fichier projet refusé (US-12.7, US-18.2) : catégorie stable et position.
//! Partagé par `pf-validate` et `patternforge-cli valider` : une seule logique.

use crate::{persist, CoreError};

/// Une erreur trouvée dans le fichier.
pub struct Finding {
    /// Catégorie stable (lisible par une machine) : `json`, `format`, `version`,
    /// `structure`, `reference`, `identifiant`, `validation`, `lecture`.
    pub kind: &'static str,
    pub message: String,
    pub line: Option<usize>,
    pub column: Option<usize>,
}

/// Catégorie et localisation d'une erreur du chargeur. La localisation (ligne, colonne) n'existe
/// que si l'analyseur JSON la fournit ; pour les erreurs de validation (références,
/// identifiants), le message cite l'identifiant ou le nom de l'objet fautif.
pub fn classify(error: &CoreError, text: Option<&str>, file_version: Option<u64>) -> Finding {
    let message = error.to_string();
    let (kind, line, column) = match error {
        CoreError::Io(_) => ("lecture", None, None),
        CoreError::NotAPatternForgeFile => ("format", None, None),
        CoreError::UnsupportedVersion { .. } => ("version", None, None),
        CoreError::Malformed(_) => match text.map(persist::reject_duplicate_keys) {
            Some(Err(e)) => ("json", Some(e.line()), Some(e.column())),
            _ => {
                // JSON syntaxiquement correct : seconde lecture typée (version courante
                // seulement) pour obtenir la position quand le type serde la connaît.
                let located = match (text, file_version) {
                    (Some(t), Some(3)) => serde_json::from_str::<crate::Project>(t).err(),
                    _ => None,
                };
                match located {
                    Some(e) if e.line() > 0 => ("structure", Some(e.line()), Some(e.column())),
                    _ => ("structure", None, None),
                }
            }
        },
        CoreError::DuplicateId(_) => ("identifiant", None, None),
        CoreError::UnknownSheet(_)
        | CoreError::UnknownPoint(_)
        | CoreError::UnknownEntity(_)
        | CoreError::UnknownNotch(_)
        | CoreError::UnknownDimension(_)
        | CoreError::UnknownFullDimension(_)
        | CoreError::UnknownPiece(_)
        | CoreError::UnknownLayer(_)
        | CoreError::UnknownGroup(_)
        | CoreError::UnknownGuide(_)
        | CoreError::DeadReference(_)
        | CoreError::CyclicReference(_) => ("reference", None, None),
        _ => ("validation", None, None),
    };
    Finding {
        kind,
        message,
        line: line.filter(|&l| l > 0),
        column: column.filter(|&c| c > 0),
    }
}

/// Version de schéma annoncée par le texte d'un fichier (lecture seule, BOM ignoré).
pub fn file_schema_version(text: &str) -> Option<u64> {
    serde_json::from_str::<serde_json::Value>(text.trim_start_matches('\u{feff}'))
        .ok()
        .and_then(|v| v.get("schema_version").and_then(serde_json::Value::as_u64))
}
