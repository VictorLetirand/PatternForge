//! « Enregistrer sous » : les images de fond suivent le projet (audit du 25/11, A1).
//!
//! Le projet cite ses images par un chemin **relatif** (`<nom>.assets/<fichier>`), qui n'est pas
//! réécrit : les fichiers sont copiés sous ce même chemin à côté du nouveau projet. Dans le même
//! dossier il n'y a donc rien à faire. Règles : jamais d'écrasement (un fichier différent déjà là
//! est signalé), jamais de suppression ni de modification de l'original, et un échec ne remet pas
//! en cause le projet, déjà enregistré quand cette copie commence.

use pf_core::background_image::{assets_dir_name, resolve};
use pf_core::Project;
use std::collections::BTreeSet;
use std::fs;
use std::io::{ErrorKind, Read};
use std::path::{Path, PathBuf};

/// Fichier non copié.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CopyProblem {
    /// Un fichier différent porte déjà ce nom à destination : il n'a pas été remplacé.
    Conflict(String),
    /// Lecture ou écriture impossible (fichier, message du système).
    Failed(String, String),
}

fn parent_of(file: &Path) -> &Path {
    file.parent()
        .filter(|d| !d.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

/// Fichiers à copier (chemins relatifs) : le contenu des dossiers `.assets` cités par le projet et
/// de celui de l'ancien nom (une annulation peut rétablir un fond retiré), plus toute image citée.
fn wanted(
    project: &Project,
    old_file: &Path,
    problems: &mut Vec<CopyProblem>,
) -> BTreeSet<PathBuf> {
    let old_dir = parent_of(old_file);
    let mut dirs: BTreeSet<String> = assets_dir_name(old_file).into_iter().collect();
    let mut files = BTreeSet::new();
    for background in project.sheets.iter().filter_map(|s| s.background.as_ref()) {
        let relative = background.image.as_str();
        if resolve(old_dir, relative).is_none() {
            continue;
        }
        files.insert(relative.split('/').collect::<PathBuf>());
        if let Some((first, _)) = relative.split_once('/') {
            if first.ends_with(".assets") {
                dirs.insert(first.to_string());
            }
        }
    }
    for dir in dirs {
        let entries = match fs::read_dir(old_dir.join(&dir)) {
            Ok(entries) => entries,
            Err(e) if e.kind() == ErrorKind::NotFound => continue,
            Err(e) => {
                problems.push(CopyProblem::Failed(dir, e.to_string()));
                continue;
            }
        };
        for entry in entries {
            match entry.and_then(|e| e.file_type().map(|t| (e, t))) {
                // Fichiers ordinaires seulement : ni sous-dossier, ni lien.
                Ok((entry, kind)) if kind.is_file() => {
                    files.insert(Path::new(&dir).join(entry.file_name()));
                }
                Ok(_) => {}
                Err(e) => problems.push(CopyProblem::Failed(dir.clone(), e.to_string())),
            }
        }
    }
    files
}

fn same_content(a: &Path, b: &Path) -> std::io::Result<bool> {
    let (mut a, mut b) = (fs::File::open(a)?, fs::File::open(b)?);
    if a.metadata()?.len() != b.metadata()?.len() {
        return Ok(false);
    }
    let (mut x, mut y) = (vec![0u8; 64 * 1024], vec![0u8; 64 * 1024]);
    loop {
        let n = a.read(&mut x)?;
        if n == 0 {
            return Ok(true);
        }
        b.read_exact(&mut y[..n])?;
        if x[..n] != y[..n] {
            return Ok(false);
        }
    }
}

/// Copie un fichier sans jamais en remplacer un (`create_new`) ; une copie ratée est retirée.
fn copy_new(source: &Path, target: &Path) -> std::io::Result<()> {
    let mut from = fs::File::open(source)?;
    if let Some(dir) = target.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut to = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(target)?;
    let result = std::io::copy(&mut from, &mut to).and_then(|_| to.sync_all());
    if result.is_err() {
        drop(to);
        let _ = fs::remove_file(target);
    }
    result
}

/// Copie à côté de `new_file` les images rangées à côté de `old_file`. Renvoie ce qui a échoué.
pub(crate) fn copy_assets(project: &Project, old_file: &Path, new_file: &Path) -> Vec<CopyProblem> {
    let (old_dir, new_dir) = (parent_of(old_file), parent_of(new_file));
    if let (Ok(a), Ok(b)) = (fs::canonicalize(old_dir), fs::canonicalize(new_dir)) {
        if a == b {
            return Vec::new();
        }
    }
    let mut problems = Vec::new();
    for relative in wanted(project, old_file, &mut problems) {
        let (source, target) = (old_dir.join(&relative), new_dir.join(&relative));
        let name = relative.display().to_string().replace('\\', "/");
        match fs::metadata(&source) {
            Ok(m) if m.is_file() => {}
            // Déjà absente avant l'enregistrement : rien à copier, la fenêtre du fond le dit.
            Ok(_) => continue,
            Err(e) if e.kind() == ErrorKind::NotFound => continue,
            Err(e) => {
                problems.push(CopyProblem::Failed(name, e.to_string()));
                continue;
            }
        }
        if fs::symlink_metadata(&target).is_ok() {
            match same_content(&source, &target) {
                Ok(true) => {}
                Ok(false) => problems.push(CopyProblem::Conflict(name)),
                Err(e) => problems.push(CopyProblem::Failed(name, e.to_string())),
            }
        } else if let Err(e) = copy_new(&source, &target) {
            problems.push(CopyProblem::Failed(name, e.to_string()));
        }
    }
    problems
}
