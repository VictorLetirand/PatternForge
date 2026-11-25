//! US-17.3 - verrou et accès concurrent : ouverture d'un fichier déjà ouvert ailleurs, refus
//! d'écraser une modification externe, avec un choix proposé à l'utilisateur.
//!
//! La détection est dans `pf-core` (`file_guard`, `Document::save_checked`) ; ici : les boîtes,
//! la prise du verrou à l'ouverture, son rafraîchissement et la suite d'une action en attente
//! (quitter, ouvrir…) une fois le conflit résolu. Rien n'est jamais écrit sans que la personne
//! l'ait choisi.

use crate::app::{modal_title, App, MODAL_WIDTH};
use eframe::egui;
use pf_core::file_guard::{FileLock, LockError, LockHolder, LOCK_REFRESH_SECS};
use pf_core::ExternalChange;
use pf_i18n::{tr, tr_args};
use pf_ui::components::{button, ButtonKind};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Enregistrement refusé : le fichier a changé hors de ce document.
pub struct ConflictPrompt {
    pub path: PathBuf,
    pub change: ExternalChange,
}

/// Ouverture proposée d'un fichier dont le verrou est tenu par une autre fenêtre.
pub struct LockPrompt {
    pub path: PathBuf,
    pub holder: LockHolder,
}

pub(crate) fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

impl App {
    /// Prend le verrou de `path` puis ouvre le fichier. Déjà tenu ailleurs : boîte de choix.
    pub(crate) fn open_with_lock(&mut self, path: &Path) {
        let lock = if self.editor.doc.path.as_deref() == Some(path) {
            self.file_lock.take()
        } else {
            match FileLock::acquire(path, unix_now()) {
                Ok(lock) => Some(lock),
                Err(LockError::Held(holder)) => {
                    self.log_error("lock", &tr("errlog.locked"));
                    self.lock_prompt = Some(LockPrompt {
                        path: path.to_path_buf(),
                        holder,
                    });
                    return;
                }
                // Dossier en lecture seule, etc. : pas de verrou, la révision protège quand même.
                Err(LockError::Io(_)) => None,
            }
        };
        self.open_loaded(path, lock);
    }

    /// Rafraîchit le verrou tenu (toutes les `LOCK_REFRESH_SECS` s) et demande l'image suivante.
    pub(crate) fn lock_tick(&mut self, ctx: &egui::Context) {
        let Some(lock) = &self.file_lock else { return };
        let period = Duration::from_secs(LOCK_REFRESH_SECS);
        if self.last_lock_refresh.elapsed() >= period {
            let _ = lock.refresh(unix_now());
            self.last_lock_refresh = Instant::now();
        }
        ctx.request_repaint_after(period);
    }

    pub(crate) fn safeguard_dialogs(
        &mut self,
        ctx: &egui::Context,
        keys_conflict: (bool, bool),
        keys_locked: (bool, bool),
    ) {
        if let Some(prompt) = &self.conflict_prompt {
            // Entrée = Enregistrer sous (le choix qui ne détruit rien) ; Échap = Annuler.
            let (mut save_as, mut overwrite, mut cancel) =
                (keys_conflict.0, false, keys_conflict.1);
            let name = file_name(&prompt.path);
            let removed = prompt.change == ExternalChange::Removed;
            egui::Modal::new(egui::Id::new("file_conflict")).show(ctx, |ui| {
                ui.set_width(MODAL_WIDTH);
                modal_title(ui, &tr("file.conflict.title"));
                let body = if removed {
                    tr_args("file.conflict.removed", &[("name", &name)])
                } else {
                    tr_args("file.conflict.modified", &[("name", &name)])
                };
                crate::a11y::label(ui, body);
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    save_as |=
                        button(ui, &tr("file.conflict.save_as"), ButtonKind::Primary).clicked();
                    let label = if removed {
                        tr("file.conflict.recreate")
                    } else {
                        tr("file.conflict.overwrite")
                    };
                    overwrite = button(ui, &label, ButtonKind::Secondary).clicked();
                    cancel |= button(ui, &tr("dialog.cancel"), ButtonKind::Secondary).clicked();
                });
            });
            if cancel {
                self.conflict_prompt = None;
            } else if overwrite || save_as {
                if let Some(prompt) = self.conflict_prompt.take() {
                    let saved = if overwrite {
                        self.commit_save(&prompt.path, true)
                    } else {
                        self.save(true)
                    };
                    if saved {
                        if let Some(action) = self.pending.take() {
                            self.perform(action, ctx);
                        }
                    }
                }
            }
        }
        if let Some(prompt) = &self.lock_prompt {
            // Entrée ou Échap = Annuler : ouvrir à deux demande un geste volontaire.
            let (mut cancel, mut open_anyway) = (keys_locked.0 || keys_locked.1, false);
            let name = file_name(&prompt.path);
            let secs = unix_now().saturating_sub(prompt.holder.since);
            egui::Modal::new(egui::Id::new("file_locked")).show(ctx, |ui| {
                ui.set_width(MODAL_WIDTH);
                modal_title(ui, &tr("file.locked.title"));
                crate::a11y::label(
                    ui,
                    tr_args(
                        "file.locked.body",
                        &[
                            ("name", &name),
                            ("pid", &prompt.holder.pid.to_string()),
                            ("secs", &secs.to_string()),
                        ],
                    ),
                );
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    cancel |= button(ui, &tr("dialog.cancel"), ButtonKind::Primary).clicked();
                    open_anyway =
                        button(ui, &tr("file.locked.open"), ButtonKind::Secondary).clicked();
                });
            });
            if cancel {
                self.lock_prompt = None;
            } else if open_anyway {
                if let Some(prompt) = self.lock_prompt.take() {
                    self.open_loaded(&prompt.path, None);
                }
            }
        }
    }
}
