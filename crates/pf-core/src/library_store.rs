//! Dossier de bibliothèque (US-11.4) : un fichier `lib-NNNNNN.pforge-lib.json` par entrée, séparé
//! de tout projet. Format versionné (`deny_unknown_fields`), identifiants d'entrée stables (le
//! numéro du fichier ; renommer ne le change pas), écriture atomique, validation complète à la
//! lecture. Un fichier illisible est **listé comme problème**, jamais ignoré ni supprimé ni
//! écrasé : aucune opération ne le touche.

use crate::error::CoreError;
use crate::library::{LibraryContent, LIBRARY_FORMAT, LIBRARY_VERSION};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// Extension d'un fichier de bibliothèque.
pub const LIBRARY_EXT: &str = ".pforge-lib.json";
/// Longueur maximale du nom d'une entrée (caractères).
pub const MAX_LIBRARY_NAME_CHARS: usize = 80;
/// Taille maximale lue pour un fichier de bibliothèque (octets).
const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;

/// Une entrée : nom choisi par la modéliste, contenu autonome.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LibraryEntry {
    pub format: String,
    pub version: u32,
    /// Identifiant stable : égal au numéro du fichier.
    pub id: u64,
    pub name: String,
    pub content: LibraryContent,
}

/// Fichier du dossier qui n'a pas pu être lu comme entrée.
#[derive(Debug, Clone, PartialEq)]
pub struct LibraryProblem {
    pub file: String,
    pub message: String,
}

/// Contenu du dossier : entrées valides (par identifiant) et fichiers en problème.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LibraryListing {
    pub entries: Vec<LibraryEntry>,
    pub problems: Vec<LibraryProblem>,
}

/// Refus et échecs de la bibliothèque.
#[derive(Debug, Clone, PartialEq)]
pub enum LibraryError {
    NameEmpty,
    NameTooLong {
        max: usize,
    },
    NameControl,
    NameDuplicate {
        name: String,
    },
    NotFound {
        id: u64,
    },
    /// Fichier d'entrée illisible ou invalide (n'est jamais modifié).
    Corrupt {
        file: String,
        message: String,
    },
    /// Contenu refusé par le cœur.
    Invalid {
        detail: String,
    },
    Io {
        detail: String,
    },
}

impl LibraryError {
    /// Code stable (clé `library.err.<code>` du catalogue).
    pub fn code(&self) -> &'static str {
        match self {
            Self::NameEmpty => "name_empty",
            Self::NameTooLong { .. } => "name_too_long",
            Self::NameControl => "name_control",
            Self::NameDuplicate { .. } => "name_duplicate",
            Self::NotFound { .. } => "not_found",
            Self::Corrupt { .. } => "corrupt",
            Self::Invalid { .. } => "invalid",
            Self::Io { .. } => "io",
        }
    }

    /// Paramètres du message : `(nom, valeur)`.
    pub fn params(&self) -> Vec<(&'static str, String)> {
        match self {
            Self::NameEmpty | Self::NameControl => Vec::new(),
            Self::NameTooLong { max } => vec![("max", max.to_string())],
            Self::NameDuplicate { name } => vec![("name", name.clone())],
            Self::NotFound { id } => vec![("id", id.to_string())],
            Self::Corrupt { file, message } => {
                vec![("file", file.clone()), ("detail", message.clone())]
            }
            Self::Invalid { detail } | Self::Io { detail } => vec![("detail", detail.clone())],
        }
    }
}

impl std::fmt::Display for LibraryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NameEmpty => write!(f, "le nom de l'entrée est vide"),
            Self::NameTooLong { max } => write!(f, "le nom dépasse {max} caractères"),
            Self::NameControl => write!(f, "le nom contient un caractère de contrôle"),
            Self::NameDuplicate { name } => write!(f, "une entrée « {name} » existe déjà"),
            Self::NotFound { id } => write!(f, "entrée {id} introuvable"),
            Self::Corrupt { file, message } => {
                write!(f, "fichier « {file} » illisible : {message}")
            }
            Self::Invalid { detail } => write!(f, "{detail}"),
            Self::Io { detail } => write!(f, "erreur de fichier : {detail}"),
        }
    }
}

impl std::error::Error for LibraryError {}

impl From<CoreError> for LibraryError {
    fn from(e: CoreError) -> LibraryError {
        LibraryError::Invalid {
            detail: e.to_string(),
        }
    }
}

/// Nom d'entrée nettoyé (espaces de bord retirés) ou refus nommé.
pub fn check_entry_name(name: &str) -> Result<String, LibraryError> {
    let t = name.trim();
    if t.is_empty() {
        return Err(LibraryError::NameEmpty);
    }
    if t.chars().any(char::is_control) {
        return Err(LibraryError::NameControl);
    }
    if t.chars().count() > MAX_LIBRARY_NAME_CHARS {
        return Err(LibraryError::NameTooLong {
            max: MAX_LIBRARY_NAME_CHARS,
        });
    }
    Ok(t.to_string())
}

fn file_name(id: u64) -> String {
    format!("lib-{id:06}{LIBRARY_EXT}")
}

/// Numéro d'un nom de fichier canonique `lib-NNNNNN.pforge-lib.json`.
fn id_of_file(name: &str) -> Option<u64> {
    let digits = name.strip_prefix("lib-")?.strip_suffix(LIBRARY_EXT)?;
    if digits.len() < 6 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let id: u64 = digits.parse().ok()?;
    (file_name(id) == name && id > 0).then_some(id)
}

impl LibraryEntry {
    /// JSON lisible (une entrée par fichier).
    pub fn to_json(&self) -> Result<String, LibraryError> {
        serde_json::to_string_pretty(self)
            .map(|mut s| {
                s.push('\n');
                s
            })
            .map_err(|e| LibraryError::Invalid {
                detail: e.to_string(),
            })
    }

    /// Lit et valide une entrée : clés en double, champs inconnus, format, version, nom,
    /// identifiants et références du contenu.
    pub fn from_json(text: &str) -> Result<LibraryEntry, LibraryError> {
        let text = text.trim_start_matches('\u{feff}');
        crate::persist::reject_duplicate_keys(text).map_err(|e| LibraryError::Invalid {
            detail: e.to_string(),
        })?;
        let entry: LibraryEntry =
            serde_json::from_str(text).map_err(|e| LibraryError::Invalid {
                detail: e.to_string(),
            })?;
        if entry.format != LIBRARY_FORMAT {
            return Err(LibraryError::Invalid {
                detail: "ce n'est pas un fichier de bibliothèque PatternForge".into(),
            });
        }
        if entry.version != LIBRARY_VERSION {
            return Err(LibraryError::Invalid {
                detail: format!(
                    "version {} non prise en charge (attendue : {LIBRARY_VERSION})",
                    entry.version
                ),
            });
        }
        if entry.id == 0 {
            return Err(LibraryError::Invalid {
                detail: "identifiant d'entrée nul".into(),
            });
        }
        if check_entry_name(&entry.name)? != entry.name {
            return Err(LibraryError::Invalid {
                detail: "nom d'entrée non normalisé".into(),
            });
        }
        entry.content.validate()?;
        Ok(entry)
    }
}

/// Dossier de bibliothèque.
#[derive(Debug, Clone, PartialEq)]
pub struct Library {
    dir: PathBuf,
}

impl Library {
    pub fn new(dir: impl Into<PathBuf>) -> Library {
        Library { dir: dir.into() }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn io(&self, e: std::io::Error) -> LibraryError {
        LibraryError::Io {
            detail: format!("{} : {e}", self.dir.display()),
        }
    }

    fn read_file(&self, name: &str, id: u64) -> Result<LibraryEntry, LibraryError> {
        let path = self.dir.join(name);
        let corrupt = |message: String| LibraryError::Corrupt {
            file: name.to_string(),
            message,
        };
        let meta = fs::metadata(&path).map_err(|e| corrupt(e.to_string()))?;
        if meta.len() > MAX_FILE_BYTES {
            return Err(corrupt("fichier trop volumineux".into()));
        }
        let bytes = fs::read(&path).map_err(|e| corrupt(e.to_string()))?;
        let text = String::from_utf8(bytes).map_err(|_| corrupt("texte UTF-8 invalide".into()))?;
        let entry = LibraryEntry::from_json(&text).map_err(|e| corrupt(e.to_string()))?;
        if entry.id != id {
            return Err(corrupt(format!(
                "l'identifiant {} ne correspond pas au nom du fichier",
                entry.id
            )));
        }
        Ok(entry)
    }

    /// Entrées valides (triées par identifiant) et fichiers en problème. Un dossier absent est
    /// une bibliothèque vide ; les fichiers temporaires d'écriture et les fichiers étrangers sont
    /// ignorés, tout fichier `*.pforge-lib.json` illisible est listé.
    pub fn list(&self) -> Result<LibraryListing, LibraryError> {
        let mut listing = LibraryListing::default();
        let rd = match fs::read_dir(&self.dir) {
            Ok(rd) => rd,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(listing),
            Err(e) => return Err(self.io(e)),
        };
        let mut names: Vec<String> = rd
            .filter_map(|e| e.ok())
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|n| n.ends_with(LIBRARY_EXT))
            .collect();
        names.sort();
        for name in names {
            match id_of_file(&name) {
                None => listing.problems.push(LibraryProblem {
                    file: name,
                    message: "nom de fichier inattendu".into(),
                }),
                Some(id) => match self.read_file(&name, id) {
                    Ok(entry) => listing.entries.push(entry),
                    Err(LibraryError::Corrupt { file, message }) => {
                        listing.problems.push(LibraryProblem { file, message })
                    }
                    Err(e) => return Err(e),
                },
            }
        }
        listing.entries.sort_by_key(|e| e.id);
        Ok(listing)
    }

    /// Plus grand numéro de fichier présent (valide ou non) : un identifiant n'est jamais repris
    /// tant que son fichier existe.
    fn max_id(&self) -> u64 {
        fs::read_dir(&self.dir)
            .map(|rd| {
                rd.filter_map(|e| e.ok())
                    .filter_map(|e| e.file_name().into_string().ok())
                    .filter_map(|n| id_of_file(&n))
                    .max()
                    .unwrap_or(0)
            })
            .unwrap_or(0)
    }

    /// Lit une entrée par identifiant.
    pub fn load(&self, id: u64) -> Result<LibraryEntry, LibraryError> {
        let name = file_name(id);
        if !self.dir.join(&name).is_file() {
            return Err(LibraryError::NotFound { id });
        }
        self.read_file(&name, id)
    }

    fn duplicate(listing: &LibraryListing, name: &str, except: Option<u64>) -> bool {
        let key = name.to_lowercase();
        listing
            .entries
            .iter()
            .any(|e| Some(e.id) != except && e.name.to_lowercase() == key)
    }

    /// Enregistre une entrée sous `name` : refus nommés (vide, trop long, caractères de contrôle,
    /// doublon casse ignorée), contenu validé, écriture atomique. Rien n'est écrit en cas de refus.
    pub fn save(&self, name: &str, content: LibraryContent) -> Result<LibraryEntry, LibraryError> {
        let name = check_entry_name(name)?;
        content.validate()?;
        let listing = self.list()?;
        if Self::duplicate(&listing, &name, None) {
            return Err(LibraryError::NameDuplicate { name });
        }
        fs::create_dir_all(&self.dir).map_err(|e| self.io(e))?;
        let entry = LibraryEntry {
            format: LIBRARY_FORMAT.to_string(),
            version: LIBRARY_VERSION,
            id: self.max_id() + 1,
            name,
            content,
        };
        self.write(&entry)?;
        Ok(entry)
    }

    fn write(&self, entry: &LibraryEntry) -> Result<(), LibraryError> {
        let path = self.dir.join(file_name(entry.id));
        crate::persist::write_atomic(&path, &entry.to_json()?).map_err(|e| LibraryError::Io {
            detail: e.to_string(),
        })
    }

    /// Renomme une entrée (identifiant et contenu inchangés).
    pub fn rename(&self, id: u64, new_name: &str) -> Result<LibraryEntry, LibraryError> {
        let name = check_entry_name(new_name)?;
        let mut entry = self.load(id)?;
        if Self::duplicate(&self.list()?, &name, Some(id)) {
            return Err(LibraryError::NameDuplicate { name });
        }
        if entry.name == name {
            return Ok(entry);
        }
        entry.name = name;
        self.write(&entry)?;
        Ok(entry)
    }

    /// Supprime une entrée **valide** (un fichier en problème n'est jamais supprimé par le
    /// programme).
    pub fn delete(&self, id: u64) -> Result<(), LibraryError> {
        self.load(id)?;
        fs::remove_file(self.dir.join(file_name(id))).map_err(|e| self.io(e))
    }
}
