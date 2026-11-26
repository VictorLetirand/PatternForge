//! US-17.2 - versions nommées d'un projet, stockées **hors du fichier projet** (format
//! `.pforge.json` inchangé, schéma 5).
//!
//! Dossier voisin `<projet>.pforge.versions/` : un fichier par version
//! `<AAAAMMJJTHHMMSSZ>-<nom assaini>.pforge.json` (le contenu est exactement ce qu'écrirait
//! `persist::to_json`), plus `index.json` (nom, date, empreinte, taille). **L'index n'est qu'un
//! cache reconstructible** : à chaque lecture il est confronté aux fichiers (entrée sans fichier =
//! retirée ; fichier sans entrée = ajouté avec le nom tiré du fichier ; entrée dont le nom ne
//! correspond plus au fichier = nom tiré du fichier ; taille différente = version **abîmée**).
//! Une version abîmée est signalée, jamais supprimée ni réécrite par l'application, et refusée à
//! la comparaison et à la restauration.
//!
//! Écritures (création, suppression) sous un verrou `index.json.lock` (`file_guard::FileLock`) ;
//! fichiers de version et index écrits de façon atomique (`persist::write_atomic`). Aucune version
//! n'est jamais écrasée : un nom de fichier déjà pris décale l'horodatage d'une seconde.

use crate::error::CoreError;
use crate::file_guard::{self, FileLock, LockError};
use crate::model::Project;
use crate::persist;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

pub const INDEX_FILE: &str = "index.json";
pub const INDEX_FORMAT: &str = "patternforge-versions";
pub const INDEX_VERSION: u32 = 1;
pub const MAX_NAME_CHARS: usize = 60;
const VERSION_SUFFIX: &str = ".pforge";
/// Suffixe des versions écrites avant le changement d'extension, toujours lues.
const LEGACY_SUFFIX: &str = ".pforge.json";
const DIR_SUFFIX: &str = ".pforge.versions";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NameError {
    Empty,
    TooLong,
    ForbiddenChar(char),
    Reserved,
    TrailingDot,
}

impl std::fmt::Display for NameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NameError::Empty => write!(f, "le nom de la version est vide"),
            NameError::TooLong => write!(f, "le nom dépasse {MAX_NAME_CHARS} caractères"),
            NameError::ForbiddenChar(c) => write!(f, "caractère interdit « {c} »"),
            NameError::Reserved => write!(f, "nom réservé par Windows"),
            NameError::TrailingDot => write!(f, "le nom ne peut pas finir par un point"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum VersionError {
    Name(NameError),
    /// Dossier de versions utilisé par une autre instance.
    Locked,
    Io(String),
    /// Fichier de version absent du dossier (ou nom de fichier non conforme).
    Unknown(String),
    /// Version tronquée, modifiée ou illisible.
    Damaged(String),
    Core(CoreError),
}

impl std::fmt::Display for VersionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VersionError::Name(e) => write!(f, "{e}"),
            VersionError::Locked => write!(f, "dossier de versions utilisé par une autre fenêtre"),
            VersionError::Io(e) => write!(f, "{e}"),
            VersionError::Unknown(n) => write!(f, "version introuvable : {n}"),
            VersionError::Damaged(n) => write!(f, "version abîmée : {n}"),
            VersionError::Core(e) => write!(f, "{e}"),
        }
    }
}

impl From<CoreError> for VersionError {
    fn from(e: CoreError) -> Self {
        VersionError::Core(e)
    }
}

impl From<NameError> for VersionError {
    fn from(e: NameError) -> Self {
        VersionError::Name(e)
    }
}

/// Une version telle que listée.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionInfo {
    /// Nom du fichier dans le dossier (identifiant stable de la version).
    pub file: String,
    pub name: String,
    /// `AAAA-MM-JJ HH:MM:SS` (UTC), tiré du nom du fichier.
    pub created: String,
    pub revision: String,
    pub size: u64,
    /// Taille ou empreinte différente de ce que l'index attend, ou fichier illisible.
    pub damaged: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listing {
    pub versions: Vec<VersionInfo>,
    /// L'index était absent, illisible ou en désaccord avec les fichiers (et a été récrit si possible).
    pub index_rebuilt: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct IndexEntry {
    file: String,
    name: String,
    created: String,
    revision: String,
    size: u64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct IndexFile {
    format: String,
    version: u32,
    entries: Vec<IndexEntry>,
}

/// Dossier des versions d'un projet : `jupe.pforge.json` -> `jupe.pforge.versions`.
pub fn versions_dir(project: &Path) -> PathBuf {
    let name = project
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let stem = name
        .strip_suffix(LEGACY_SUFFIX)
        .map(str::to_string)
        .or_else(|| {
            Path::new(&name)
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
        })
        .unwrap_or(name);
    project.with_file_name(format!("{stem}{DIR_SUFFIX}"))
}

/// Valide un nom de version (rognage des espaces de bord) et renvoie le nom rogné.
pub fn validate_name(raw: &str) -> Result<String, NameError> {
    let name = raw.trim();
    if name.is_empty() {
        return Err(NameError::Empty);
    }
    if name.chars().count() > MAX_NAME_CHARS {
        return Err(NameError::TooLong);
    }
    if let Some(c) = name.chars().find(|c| {
        matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') || (*c as u32) < 0x20
    }) {
        return Err(NameError::ForbiddenChar(c));
    }
    if name.ends_with('.') {
        return Err(NameError::TrailingDot);
    }
    let base = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    let reserved = matches!(base.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ["COM", "LPT"].iter().any(|p| {
            base.strip_prefix(p)
                .is_some_and(|d| d.len() == 1 && matches!(d.as_bytes()[0], b'1'..=b'9'))
        });
    if reserved {
        return Err(NameError::Reserved);
    }
    Ok(name.to_string())
}

/// Nom de fichier assaini : lettres et chiffres gardés, le reste devient `-` (sans doublon).
pub fn sanitize_for_file(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_matches('-');
    if out.is_empty() {
        "version".to_string()
    } else {
        out.to_string()
    }
}

/// `AAAAMMJJTHHMMSSZ` pour des secondes Unix (UTC, calendrier grégorien).
pub fn format_timestamp(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}{month:02}{day:02}T{:02}{:02}{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

fn display_date(stamp: &str) -> String {
    format!(
        "{}-{}-{} {}:{}:{}",
        &stamp[0..4],
        &stamp[4..6],
        &stamp[6..8],
        &stamp[9..11],
        &stamp[11..13],
        &stamp[13..15]
    )
}

/// `(horodatage, nom assaini)` d'un nom de fichier de version, `None` si non conforme.
fn parse_file_name(file: &str) -> Option<(&str, &str)> {
    let stem = file
        .strip_suffix(LEGACY_SUFFIX)
        .or_else(|| file.strip_suffix(VERSION_SUFFIX))?;
    let b = stem.as_bytes();
    if b.len() < 18 || b[8] != b'T' || b[15] != b'Z' || b[16] != b'-' {
        return None;
    }
    let digits = |r: std::ops::Range<usize>| b[r].iter().all(u8::is_ascii_digit);
    (digits(0..8) && digits(9..15) && stem.is_char_boundary(17)).then(|| (&stem[..16], &stem[17..]))
}

fn io_err(path: &Path, e: &std::io::Error) -> VersionError {
    VersionError::Io(format!("{} : {e}", path.display()))
}

fn read_index(dir: &Path) -> Option<Vec<IndexEntry>> {
    let text = fs::read_to_string(dir.join(INDEX_FILE)).ok()?;
    let idx: IndexFile = serde_json::from_str(&text).ok()?;
    (idx.format == INDEX_FORMAT && idx.version == INDEX_VERSION).then_some(idx.entries)
}

fn write_index(dir: &Path, versions: &[VersionInfo]) -> Result<(), VersionError> {
    let idx = IndexFile {
        format: INDEX_FORMAT.into(),
        version: INDEX_VERSION,
        entries: versions
            .iter()
            .filter(|v| !v.damaged)
            .map(|v| IndexEntry {
                file: v.file.clone(),
                name: v.name.clone(),
                created: v.created.clone(),
                revision: v.revision.clone(),
                size: v.size,
            })
            .collect(),
    };
    let text =
        serde_json::to_string_pretty(&idx).map_err(|e| VersionError::Io(e.to_string()))? + "\n";
    persist::write_atomic(&dir.join(INDEX_FILE), &text).map_err(VersionError::Core)
}

/// Confronte les fichiers du dossier à `known` (entrées d'index). Sans E/S d'écriture.
fn scan(dir: &Path, known: &[IndexEntry]) -> Result<Vec<VersionInfo>, VersionError> {
    let by_file: BTreeMap<&str, &IndexEntry> = known.iter().map(|e| (e.file.as_str(), e)).collect();
    let rd = match fs::read_dir(dir) {
        Ok(r) => r,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(io_err(dir, &e)),
    };
    let mut files: Vec<String> = rd
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| parse_file_name(n).is_some())
        .collect();
    files.sort();
    let mut out = Vec::new();
    for file in files {
        let Some((stamp, stem)) = parse_file_name(&file) else {
            continue;
        };
        let path = dir.join(&file);
        let len = fs::metadata(&path).map_err(|e| io_err(&path, &e))?.len();
        let (name, revision, size, damaged) = match by_file.get(file.as_str()) {
            Some(e) => {
                let name = if sanitize_for_file(&e.name) == stem {
                    e.name.clone()
                } else {
                    stem.to_string()
                };
                (name, e.revision.clone(), e.size, e.size != len)
            }
            None => {
                let bytes = fs::read(&path).map_err(|e| io_err(&path, &e))?;
                let revision = file_guard::revision_of(&bytes);
                let ok = persist::from_bytes(bytes, &file).is_ok();
                (stem.to_string(), revision, len, !ok)
            }
        };
        out.push(VersionInfo {
            created: display_date(stamp),
            file,
            name,
            revision,
            size,
            damaged,
        });
    }
    Ok(out)
}

fn same_as_index(versions: &[VersionInfo], known: &[IndexEntry]) -> bool {
    // Une version abîmée n'est jamais indexée : elle est réexaminée à chaque lecture.
    let healthy: Vec<&VersionInfo> = versions.iter().filter(|v| !v.damaged).collect();
    healthy.len() == known.len()
        && healthy.iter().zip(known).all(|(v, e)| {
            v.file == e.file
                && v.name == e.name
                && v.created == e.created
                && v.revision == e.revision
                && v.size == e.size
        })
}

fn lock(dir: &Path, now: u64) -> Result<FileLock, VersionError> {
    match FileLock::acquire(&dir.join(INDEX_FILE), now) {
        Ok(l) => Ok(l),
        Err(LockError::Held(_)) => Err(VersionError::Locked),
        Err(LockError::Io(e)) => Err(VersionError::Io(e)),
    }
}

/// Versions du projet `project`, index confronté aux fichiers et récrit s'il diffère (échec
/// d'écriture sans conséquence : la liste reste juste, l'index reste un cache).
pub fn list(project: &Path) -> Result<Listing, VersionError> {
    let dir = versions_dir(project);
    let index = read_index(&dir);
    let known = index.clone().unwrap_or_default();
    let versions = scan(&dir, &known)?;
    let rebuilt = index.is_none() && !versions.is_empty()
        || index.is_some_and(|k| !same_as_index(&versions, &k));
    if rebuilt {
        let _ = write_index(&dir, &versions);
    }
    Ok(Listing {
        versions,
        index_rebuilt: rebuilt,
    })
}

/// Crée la version nommée `name` de `current` à l'instant `now` (secondes Unix).
pub fn create(
    project: &Path,
    current: &Project,
    name: &str,
    now: u64,
) -> Result<VersionInfo, VersionError> {
    let name = validate_name(name)?;
    let json = persist::to_json(current)?;
    let dir = versions_dir(project);
    fs::create_dir_all(&dir).map_err(|e| io_err(&dir, &e))?;
    let _lock = lock(&dir, now)?;
    let known = read_index(&dir).unwrap_or_default();
    let stem = sanitize_for_file(&name);
    let mut secs = now;
    let (stamp, file) = loop {
        let stamp = format_timestamp(secs);
        let file = format!("{stamp}-{stem}{VERSION_SUFFIX}");
        if !dir.join(&file).exists() {
            break (stamp, file);
        }
        secs += 1;
    };
    persist::write_atomic(&dir.join(&file), &json)?;
    let info = VersionInfo {
        created: display_date(&stamp),
        file,
        name,
        revision: file_guard::revision_of(json.as_bytes()),
        size: json.len() as u64,
        damaged: false,
    };
    let mut entries = known;
    entries.push(IndexEntry {
        file: info.file.clone(),
        name: info.name.clone(),
        created: info.created.clone(),
        revision: info.revision.clone(),
        size: info.size,
    });
    let versions = scan(&dir, &entries)?;
    write_index(&dir, &versions)?;
    Ok(info)
}

fn checked_path(project: &Path, file: &str) -> Result<PathBuf, VersionError> {
    if parse_file_name(file).is_none() {
        return Err(VersionError::Unknown(file.to_string()));
    }
    let path = versions_dir(project).join(file);
    if path.is_file() {
        Ok(path)
    } else {
        Err(VersionError::Unknown(file.to_string()))
    }
}

/// Lit une version. Refus (`Damaged`) si la taille ou l'empreinte attendues par l'index ne
/// correspondent pas, ou si le contenu n'est plus un projet valide.
pub fn read(project: &Path, file: &str) -> Result<Project, VersionError> {
    let path = checked_path(project, file)?;
    let bytes = fs::read(&path).map_err(|e| io_err(&path, &e))?;
    let revision = file_guard::revision_of(&bytes);
    if let Some(e) =
        read_index(&versions_dir(project)).and_then(|k| k.into_iter().find(|e| e.file == file))
    {
        if e.revision != revision || e.size != bytes.len() as u64 {
            return Err(VersionError::Damaged(file.to_string()));
        }
    }
    persist::from_bytes(bytes, file)
        .map(|(p, _)| p)
        .map_err(|_| VersionError::Damaged(file.to_string()))
}

/// Supprime une version (la confirmation est l'affaire de l'interface).
pub fn delete(project: &Path, file: &str, now: u64) -> Result<(), VersionError> {
    let path = checked_path(project, file)?;
    let dir = versions_dir(project);
    let _lock = lock(&dir, now)?;
    fs::remove_file(&path).map_err(|e| io_err(&path, &e))?;
    let known = read_index(&dir).unwrap_or_default();
    let versions = scan(&dir, &known)?;
    write_index(&dir, &versions)
}
