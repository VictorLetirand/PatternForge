//! Document ouvert : projet + historique annuler/rétablir + état « modifié ».

use crate::command::{Command, Outcome};
use crate::error::CoreError;
use crate::file_guard;
use crate::model::Project;
use crate::persist;
use std::path::{Path, PathBuf};

const MAX_HISTORY: usize = 200;

#[derive(Debug, Clone)]
pub struct Document {
    project: Project,
    undo: Vec<Project>,
    redo: Vec<Project>,
    /// État du projet au début d'un geste continu (glisser), s'il y en a un.
    group_start: Option<Project>,
    saved: Project,
    pub path: Option<PathBuf>,
    /// Révision (empreinte) du fichier tel que lu ou écrit en dernier (US-17.3).
    disk_revision: Option<String>,
}

/// Le fichier du document a-t-il changé sur le disque depuis qu'il a été lu ou écrit ?
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExternalChange {
    Unchanged,
    /// Le contenu diffère de celui que ce document a lu ou écrit.
    Modified,
    /// Le fichier n'existe plus.
    Removed,
}

/// Refus d'un enregistrement protégé, ou erreur d'écriture.
#[derive(Debug, Clone, PartialEq)]
pub enum SaveError {
    /// Fichier modifié ou supprimé hors de ce document : rien n'a été écrit.
    External(ExternalChange),
    Core(CoreError),
}

impl From<CoreError> for SaveError {
    fn from(e: CoreError) -> SaveError {
        SaveError::Core(e)
    }
}

impl std::fmt::Display for SaveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SaveError::External(_) => write!(f, "le fichier a été modifié par ailleurs"),
            SaveError::Core(e) => write!(f, "{e}"),
        }
    }
}

impl Document {
    pub fn new(project: Project) -> Document {
        Document {
            saved: project.clone(),
            project,
            undo: Vec::new(),
            redo: Vec::new(),
            group_start: None,
            path: None,
            disk_revision: None,
        }
    }

    pub fn open(path: &Path) -> Result<Document, CoreError> {
        let (project, revision) = persist::load_with_revision(path)?;
        let mut doc = Document::new(project);
        doc.path = Some(path.to_path_buf());
        doc.disk_revision = Some(revision);
        Ok(doc)
    }

    /// Document reconstitué depuis une copie de secours : `saved` est l'état
    /// du fichier principal (ou un projet vide), de sorte que le document reste
    /// « modifié » jusqu'au prochain enregistrement. Historique vide.
    pub fn recovered(project: Project, path: Option<PathBuf>, saved: Project) -> Document {
        let disk_revision = path
            .as_deref()
            .and_then(|p| file_guard::file_revision(p).ok().flatten());
        Document {
            disk_revision,
            saved,
            project,
            undo: Vec::new(),
            redo: Vec::new(),
            group_start: None,
            path,
        }
    }

    /// Document déjà lu par ailleurs (ouverture en fil séparé, US-17.4) : mêmes champs que `open`.
    pub fn opened(path: &Path, project: Project, revision: String) -> Document {
        let mut doc = Document::new(project);
        doc.path = Some(path.to_path_buf());
        doc.disk_revision = Some(revision);
        doc
    }

    /// Remplace tout le projet par `project` (restauration d'une version, US-17.2) : **une seule**
    /// étape annulable, jamais une perte (l'état d'avant reste dans l'historique). Projet invalide :
    /// refus sans trace ; projet identique : aucune étape.
    pub fn restore_project(&mut self, project: Project) -> Result<(), CoreError> {
        self.end_group();
        project.validate()?;
        if project == self.project {
            return Ok(());
        }
        let before = std::mem::replace(&mut self.project, project);
        self.push_undo(before);
        Ok(())
    }

    pub fn project(&self) -> &Project {
        &self.project
    }

    /// Vrai si le projet diffère de sa dernière version enregistrée.
    pub fn is_dirty(&self) -> bool {
        self.project != self.saved
    }

    /// Exécute une commande. Hors geste continu, elle devient une étape
    /// d'historique ; pendant un geste, tout le geste forme une seule étape.
    pub fn execute(&mut self, command: Command) -> Result<Outcome, CoreError> {
        // `before` est l'ancien projet lui-même (remplacé, pas recopié).
        let (outcome, before) = self.project.apply_returning_previous(command)?;
        if self.group_start.is_none() && self.project != before {
            self.push_undo(before);
        }
        Ok(outcome)
    }

    fn push_undo(&mut self, state: Project) {
        self.undo.push(state);
        if self.undo.len() > MAX_HISTORY {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    /// Début d'un geste continu (ex. glisser un point).
    pub fn begin_group(&mut self) {
        if self.group_start.is_none() {
            self.group_start = Some(self.project.clone());
        }
    }

    /// Fin du geste : une seule étape d'historique si quelque chose a changé.
    pub fn end_group(&mut self) {
        if let Some(start) = self.group_start.take() {
            if start != self.project {
                self.push_undo(start);
            }
        }
    }

    /// Abandonne le geste en cours et restaure l'état initial.
    pub fn cancel_group(&mut self) {
        if let Some(start) = self.group_start.take() {
            self.project = start;
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo(&mut self) -> bool {
        self.end_group();
        match self.undo.pop() {
            Some(prev) => {
                self.redo.push(std::mem::replace(&mut self.project, prev));
                true
            }
            None => false,
        }
    }

    pub fn redo(&mut self) -> bool {
        self.end_group();
        match self.redo.pop() {
            Some(next) => {
                self.undo.push(std::mem::replace(&mut self.project, next));
                true
            }
            None => false,
        }
    }

    /// Enregistre vers `path` (écriture atomique) et mémorise ce chemin.
    pub fn save_as(&mut self, path: &Path) -> Result<(), CoreError> {
        self.end_group();
        let json = persist::to_json(&self.project)?;
        persist::write_atomic(path, &json)?;
        self.disk_revision = Some(file_guard::revision_of(json.as_bytes()));
        self.saved = self.project.clone();
        self.path = Some(path.to_path_buf());
        Ok(())
    }

    /// Le fichier du document a-t-il changé sur le disque depuis sa lecture ou
    /// son dernier enregistrement ? Document sans chemin : `Unchanged`.
    pub fn external_change(&self) -> Result<ExternalChange, CoreError> {
        let Some(path) = &self.path else {
            return Ok(ExternalChange::Unchanged);
        };
        Ok(match file_guard::file_revision(path)? {
            None => ExternalChange::Removed,
            Some(rev) if self.disk_revision.as_ref() == Some(&rev) => ExternalChange::Unchanged,
            Some(_) => ExternalChange::Modified,
        })
    }

    /// Enregistre, mais **refuse** (sans rien écrire) si le fichier visé est celui
    /// du document et qu'il a été modifié ou supprimé hors de lui. Enregistrer
    /// sous un autre chemin n'est pas contrôlé ici (le système demande déjà
    /// confirmation d'un remplacement). `save_as` reste l'écrasement explicite.
    pub fn save_checked(&mut self, path: &Path) -> Result<(), SaveError> {
        if self.path.as_deref() == Some(path) {
            match self.external_change()? {
                ExternalChange::Unchanged => {}
                other => return Err(SaveError::External(other)),
            }
        }
        Ok(self.save_as(path)?)
    }
}
