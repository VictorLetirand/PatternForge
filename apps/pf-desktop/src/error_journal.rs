//! US-17.5 - journal d'erreurs local : les erreurs affichées sont gardées, **assainies**, dans la
//! clé `error_log` de `settings.json` (hors projet), consultables et exportables à la main.
//! Rien n'est envoyé. L'assainissement et la rotation sont dans `pf_core::error_log` (tests
//! adverses sur les chemins, noms de projet et d'utilisateur) ; ici : la collecte, la
//! persistance et la fenêtre.

use crate::app::App;
use crate::palette::pal;
use crate::safeguard::unix_now;
use eframe::egui::{self, RichText};
use pf_core::error_log::{format_utc, ErrorLog, MAX_ENTRIES};
use pf_i18n::{settings, tr, tr_args};
use std::path::Path;

/// Clé du fichier de réglages.
pub const SETTINGS_KEY: &str = "error_log";

pub fn load(path: &Path) -> ErrorLog {
    settings::load_value(path, SETTINGS_KEY)
        .map(|v| ErrorLog::from_value(&v))
        .unwrap_or_default()
}

/// Valeurs à masquer partout : trop courtes, elles masqueraient du texte ordinaire.
fn usable(values: impl IntoIterator<Item = String>) -> Vec<String> {
    values
        .into_iter()
        .filter(|v| v.trim().chars().count() >= 3)
        .collect()
}

/// Type d'une entrée du journal, en toutes lettres (PH-6) ; un type inconnu s'affiche tel quel.
fn kind_label(kind: &str) -> String {
    match kind {
        "dialog" => tr("errlog.kind.dialog"),
        "conflict" => tr("errlog.kind.conflict"),
        "status" => tr("errlog.kind.status"),
        "lock" => tr("errlog.kind.lock"),
        other => other.to_string(),
    }
}

impl App {
    /// Noms et chemins qui ne doivent jamais entrer dans le journal.
    fn error_secrets(&self) -> Vec<String> {
        let mut secrets = vec![self.editor.project().name.clone()];
        secrets.extend(self.editor.project().sheets.iter().map(|s| s.name.clone()));
        if let Some(path) = &self.editor.doc.path {
            secrets.push(path.display().to_string());
            if let Some(parent) = path.parent() {
                secrets.push(parent.display().to_string());
            }
            for part in [path.file_name(), path.file_stem()].into_iter().flatten() {
                secrets.push(part.to_string_lossy().into_owned());
            }
        }
        for var in [
            "USERNAME",
            "USER",
            "LOGNAME",
            "USERPROFILE",
            "HOME",
            "COMPUTERNAME",
        ] {
            if let Ok(value) = std::env::var(var) {
                secrets.push(value);
            }
        }
        usable(secrets)
    }

    /// Enregistre une erreur (assainie) et réécrit le réglage si le journal a changé.
    pub(crate) fn log_error(&mut self, kind: &str, raw: &str) {
        let secrets = self.error_secrets();
        if self.error_log.record(unix_now(), kind, raw, &secrets) {
            self.save_error_log();
        }
    }

    fn save_error_log(&self) {
        if let Some(path) = &self.settings_path {
            let _ = settings::save_value(path, SETTINGS_KEY, self.error_log.to_value());
        }
    }

    /// Une fois par erreur affichée (boîte d'erreur ou message d'état rouge).
    pub(crate) fn log_new_errors(&mut self) {
        match self.error_dialog.clone() {
            Some(text) if self.logged_dialog.as_ref() != Some(&text) => {
                self.log_error("dialog", &text);
                self.logged_dialog = Some(text);
            }
            None => self.logged_dialog = None,
            _ => {}
        }
        if self.editor.status.error {
            let text = self.editor.status.text.clone();
            if self.logged_status.as_ref() != Some(&text) {
                self.log_error("status", &text);
                self.logged_status = Some(text);
            }
        } else {
            self.logged_status = None;
        }
    }

    /// Texte exporté : en-tête, puis une ligne par erreur (la plus ancienne d'abord).
    pub(crate) fn error_log_text(&self) -> String {
        let mut text = tr("errlog.export_header");
        text.push_str("\n\n");
        for line in self.error_log.export_lines() {
            text.push_str(&line);
            text.push('\n');
        }
        text
    }

    pub(crate) fn export_error_log_to(&mut self, path: &Path) {
        match std::fs::write(path, self.error_log_text()) {
            Ok(()) => {
                self.editor.status.text = tr("errlog.exported");
                self.editor.status.error = false;
            }
            Err(e) => {
                self.error_dialog = Some(tr_args(
                    "errlog.export_error",
                    &[("detail", &crate::editor::capitalize(&e.kind().to_string()))],
                ));
            }
        }
    }

    /// Fenêtre « Journal d'erreurs » : consulter, exporter, effacer.
    pub fn error_log_window(&mut self, ctx: &egui::Context) {
        if !self.show_error_log {
            return;
        }
        let mut open = true;
        let (mut export, mut clear) = (false, false);
        let muted = pal().muted;
        egui::Window::new(tr("errlog.title"))
            .open(&mut open)
            .default_width(460.0)
            .show(ctx, |ui| {
                crate::a11y::label(
                    ui,
                    RichText::new(tr_args("errlog.note", &[("max", &MAX_ENTRIES.to_string())]))
                        .small()
                        .color(muted),
                );
                ui.horizontal(|ui| {
                    let filled = !self.error_log.is_empty();
                    export = ui
                        .add_enabled(filled, egui::Button::new(tr("errlog.export")))
                        .clicked();
                    clear = ui
                        .add_enabled(filled, egui::Button::new(tr("errlog.clear")))
                        .clicked();
                });
                ui.separator();
                if self.error_log.is_empty() {
                    crate::a11y::label(ui, tr("errlog.empty"));
                }
                egui::ScrollArea::vertical()
                    .max_height(360.0)
                    .show(ui, |ui| {
                        for e in self.error_log.entries().iter().rev() {
                            crate::a11y::label(
                                ui,
                                RichText::new(format!(
                                    "{} [{}]",
                                    format_utc(e.time),
                                    kind_label(&e.kind)
                                ))
                                .small()
                                .color(muted),
                            );
                            crate::a11y::label(ui, &e.message);
                            ui.add_space(4.0);
                        }
                    });
            });
        if export {
            let file = rfd::FileDialog::new()
                .set_file_name("journal-erreurs.txt")
                .add_filter("txt", &["txt"])
                .save_file();
            if let Some(path) = file {
                self.export_error_log_to(&path);
            }
        }
        if clear {
            self.error_log.clear();
            self.save_error_log();
        }
        self.show_error_log = open;
    }
}
