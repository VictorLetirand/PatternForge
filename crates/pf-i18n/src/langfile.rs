//! US-19.3 : langues supplémentaires ajoutées **par fichier**, sans recompiler.
//!
//! Un fichier `<code>.lang.json` (dossier `langues/` à côté de l'exécutable ou des réglages) :
//! `{ "format": "patternforge-lang", "version": 1, "code": "es", "name": "Español",
//! "messages": { "clé": "texte", … } }`. Le chargeur est **strict** : un fichier invalide est
//! refusé en entier avec un message (jamais une langue à moitié chargée, jamais de panique).
//! Les clés absentes retombent sur l'anglais puis sur le français. Aucune langue
//! supplémentaire n'est livrée : le vocabulaire métier doit être validé par un humain.

use crate::{find, Lang};
use serde_json::Value;
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// Marque du fichier.
pub const FORMAT: &str = "patternforge-lang";
/// Version du format de fichier de langue.
pub const VERSION: u64 = 1;
/// Suffixe des fichiers de langue.
pub const SUFFIX: &str = ".lang.json";
/// Nom du dossier de langues.
pub const DIR_NAME: &str = "langues";
/// Taille maximale d'un fichier de langue (octets).
pub const MAX_BYTES: u64 = 2 * 1024 * 1024;
/// Longueur maximale d'un texte (caractères).
pub const MAX_TEXT_CHARS: usize = 4000;
/// Longueur maximale du nom affiché (caractères).
pub const MAX_NAME_CHARS: usize = 60;

/// Raison de refus d'un fichier de langue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LangFileError {
    TooBig,
    Unreadable(String),
    NotJson(String),
    NotAnObject,
    WrongFormat,
    BadVersion,
    UnknownField(String),
    MissingField(&'static str),
    BadCode(String),
    ReservedCode(String),
    BadName,
    BadMessages,
    UnknownKey(String),
    BadText(String),
    PlaceholderMismatch(String),
    CodeFileMismatch { code: String, file: String },
}

impl LangFileError {
    /// Code stable de la raison.
    pub fn code(&self) -> &'static str {
        match self {
            Self::TooBig => "too_big",
            Self::Unreadable(_) => "unreadable",
            Self::NotJson(_) => "not_json",
            Self::NotAnObject => "not_an_object",
            Self::WrongFormat => "wrong_format",
            Self::BadVersion => "bad_version",
            Self::UnknownField(_) => "unknown_field",
            Self::MissingField(_) => "missing_field",
            Self::BadCode(_) => "bad_code",
            Self::ReservedCode(_) => "reserved_code",
            Self::BadName => "bad_name",
            Self::BadMessages => "bad_messages",
            Self::UnknownKey(_) => "unknown_key",
            Self::BadText(_) => "bad_text",
            Self::PlaceholderMismatch(_) => "placeholder_mismatch",
            Self::CodeFileMismatch { .. } => "code_file_mismatch",
        }
    }

    /// Message dans la langue courante (clé `lang.err.<code>`).
    pub fn message(&self) -> String {
        let detail = match self {
            Self::Unreadable(d)
            | Self::NotJson(d)
            | Self::UnknownField(d)
            | Self::BadCode(d)
            | Self::ReservedCode(d)
            | Self::UnknownKey(d)
            | Self::BadText(d)
            | Self::PlaceholderMismatch(d) => d.clone(),
            Self::MissingField(d) => (*d).to_string(),
            Self::CodeFileMismatch { code, file } => format!("{code} / {file}"),
            _ => String::new(),
        };
        crate::tr_args(&format!("lang.err.{}", self.code()), &[("detail", &detail)])
    }
}

/// Langue supplémentaire chargée.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtraLang {
    pub code: String,
    pub name: String,
    pub messages: BTreeMap<String, String>,
}

/// Code BCP 47 simplifié : sous-étiquettes alphanumériques de 1 à 8 caractères séparées par `-`,
/// la première de 2 ou 3 lettres, 35 caractères au plus.
pub fn valid_code(code: &str) -> bool {
    if code.is_empty() || code.len() > 35 || !code.is_ascii() {
        return false;
    }
    let mut parts = code.split('-');
    let first = parts.next().unwrap_or("");
    if !(2..=3).contains(&first.len()) || !first.chars().all(|c| c.is_ascii_alphabetic()) {
        return false;
    }
    parts.all(|p| (1..=8).contains(&p.len()) && p.chars().all(|c| c.is_ascii_alphanumeric()))
}

fn placeholders(text: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    let mut rest = text;
    while let Some(start) = rest.find('{') {
        let after = &rest[start + 1..];
        match after.find('}') {
            Some(end) => {
                found.insert(after[..end].to_string());
                rest = &after[end + 1..];
            }
            None => break,
        }
    }
    found
}

fn reserved(code: &str) -> bool {
    Lang::ALL
        .iter()
        .any(|l| l.code().eq_ignore_ascii_case(code))
}

/// Analyse stricte du texte d'un fichier de langue.
pub fn parse(text: &str) -> Result<ExtraLang, LangFileError> {
    if text.len() as u64 > MAX_BYTES {
        return Err(LangFileError::TooBig);
    }
    let value: Value =
        serde_json::from_str(text).map_err(|e| LangFileError::NotJson(e.to_string()))?;
    let Value::Object(map) = value else {
        return Err(LangFileError::NotAnObject);
    };
    if let Some(extra) = map
        .keys()
        .find(|k| !["format", "version", "code", "name", "messages"].contains(&k.as_str()))
    {
        return Err(LangFileError::UnknownField(extra.clone()));
    }
    if map.get("format").and_then(Value::as_str) != Some(FORMAT) {
        return Err(LangFileError::WrongFormat);
    }
    if map.get("version").and_then(Value::as_u64) != Some(VERSION) {
        return Err(LangFileError::BadVersion);
    }
    let code = map
        .get("code")
        .ok_or(LangFileError::MissingField("code"))?
        .as_str()
        .ok_or_else(|| LangFileError::BadCode(String::new()))?;
    if !valid_code(code) {
        return Err(LangFileError::BadCode(code.to_string()));
    }
    if reserved(code) {
        return Err(LangFileError::ReservedCode(code.to_string()));
    }
    let name = map
        .get("name")
        .ok_or(LangFileError::MissingField("name"))?
        .as_str()
        .ok_or(LangFileError::BadName)?;
    if name.trim().is_empty()
        || name.chars().count() > MAX_NAME_CHARS
        || name.chars().any(char::is_control)
    {
        return Err(LangFileError::BadName);
    }
    let table = map
        .get("messages")
        .ok_or(LangFileError::MissingField("messages"))?
        .as_object()
        .ok_or(LangFileError::BadMessages)?;
    let mut messages = BTreeMap::new();
    for (key, value) in table {
        let Some(reference) = find(Lang::Fr.catalog(), key) else {
            return Err(LangFileError::UnknownKey(key.clone()));
        };
        let text = value
            .as_str()
            .ok_or_else(|| LangFileError::BadText(key.clone()))?;
        if text.trim().is_empty()
            || text.chars().count() > MAX_TEXT_CHARS
            || text.chars().any(|c| c.is_control() && c != '\n')
        {
            return Err(LangFileError::BadText(key.clone()));
        }
        if placeholders(text) != placeholders(reference) {
            return Err(LangFileError::PlaceholderMismatch(key.clone()));
        }
        messages.insert(key.clone(), text.to_string());
    }
    Ok(ExtraLang {
        code: code.to_string(),
        name: name.to_string(),
        messages,
    })
}

/// Lit un fichier (taille bornée avant lecture) ; le code doit correspondre au nom du fichier.
pub fn load_file(path: &Path) -> Result<ExtraLang, LangFileError> {
    let meta = std::fs::metadata(path).map_err(|e| LangFileError::Unreadable(e.to_string()))?;
    if meta.len() > MAX_BYTES {
        return Err(LangFileError::TooBig);
    }
    let text =
        std::fs::read_to_string(path).map_err(|e| LangFileError::Unreadable(e.to_string()))?;
    let lang = parse(&text)?;
    let file = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string();
    if file != format!("{}{SUFFIX}", lang.code) {
        return Err(LangFileError::CodeFileMismatch {
            code: lang.code,
            file,
        });
    }
    Ok(lang)
}

/// Résultat du chargement d'un dossier.
#[derive(Debug, Default)]
pub struct LoadReport {
    pub loaded: Vec<ExtraLang>,
    /// Nom de fichier et raison du refus.
    pub refused: Vec<(String, LangFileError)>,
}

/// Charge tous les `*.lang.json` d'un dossier, triés par nom ; chaque fichier invalide est
/// refusé seul. Un dossier absent n'est pas une erreur. Deux fichiers de même code : le second
/// est refusé.
pub fn load_dir(dir: &Path) -> LoadReport {
    let mut report = LoadReport::default();
    let Ok(read) = std::fs::read_dir(dir) else {
        return report;
    };
    let mut files: Vec<PathBuf> = read
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.ends_with(SUFFIX))
        })
        .collect();
    files.sort();
    for path in files {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        match load_file(&path) {
            Ok(lang) if report.loaded.iter().any(|l| l.code == lang.code) => {
                report
                    .refused
                    .push((name, LangFileError::ReservedCode(lang.code)));
            }
            Ok(lang) => report.loaded.push(lang),
            Err(e) => report.refused.push((name, e)),
        }
    }
    report
}

/// Gabarit complet généré depuis le catalogue : toutes les clés, texte de référence anglais.
/// Le code `xx` et le nom `Template` sont à remplacer.
pub fn template_json() -> String {
    let mut messages = serde_json::Map::new();
    for (key, _) in Lang::Fr.catalog() {
        let text = find(Lang::En.catalog(), key)
            .or_else(|| find(Lang::Fr.catalog(), key))
            .unwrap_or("");
        messages.insert((*key).to_string(), Value::String(text.to_string()));
    }
    let doc = serde_json::json!({
        "format": FORMAT,
        "version": VERSION,
        "code": "xx",
        "name": "Template",
        "messages": Value::Object(messages),
    });
    let mut text = serde_json::to_string_pretty(&doc).unwrap_or_default();
    text.push('\n');
    text
}

thread_local! {
    static AVAILABLE: RefCell<Vec<ExtraLang>> = const { RefCell::new(Vec::new()) };
    static ACTIVE: RefCell<Option<ExtraLang>> = const { RefCell::new(None) };
}

/// Remplace la liste des langues supplémentaires disponibles (fil courant). Si la langue active
/// n'y figure plus, elle est désactivée.
pub fn set_available(list: Vec<ExtraLang>) {
    let active = ACTIVE.with(|a| a.borrow().as_ref().map(|l| l.code.clone()));
    if let Some(code) = active {
        if !list.iter().any(|l| l.code == code) {
            deactivate();
        }
    }
    AVAILABLE.with(|a| *a.borrow_mut() = list);
}

/// Langues supplémentaires disponibles : (code, nom).
pub fn available() -> Vec<(String, String)> {
    AVAILABLE.with(|a| {
        a.borrow()
            .iter()
            .map(|l| (l.code.clone(), l.name.clone()))
            .collect()
    })
}

/// Active une langue supplémentaire disponible ; `false` si le code est inconnu (rien ne change).
/// La langue de base devient l'anglais (séparateur décimal compris).
pub fn activate(code: &str) -> bool {
    let found = AVAILABLE.with(|a| a.borrow().iter().find(|l| l.code == code).cloned());
    match found {
        Some(lang) => {
            crate::LANG.with(|l| l.set(Lang::En));
            ACTIVE.with(|a| *a.borrow_mut() = Some(lang));
            true
        }
        None => false,
    }
}

/// Désactive la langue supplémentaire.
pub fn deactivate() {
    ACTIVE.with(|a| *a.borrow_mut() = None);
}

/// Code de la langue supplémentaire active.
pub fn active_code() -> Option<String> {
    ACTIVE.with(|a| a.borrow().as_ref().map(|l| l.code.clone()))
}

/// Texte de la langue supplémentaire active pour une clé.
pub(crate) fn active_text(key: &str) -> Option<String> {
    ACTIVE.with(|a| {
        a.borrow()
            .as_ref()
            .and_then(|l| l.messages.get(key).cloned())
    })
}
