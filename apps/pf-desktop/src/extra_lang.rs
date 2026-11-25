//! US-19.3 : langues supplémentaires par fichier (dossiers `langues/`), choix mémorisé hors projet.

use crate::app::App;
use pf_i18n::{langfile, settings, tr, tr_args};
use std::path::{Path, PathBuf};

/// Clé du réglage (code de la langue supplémentaire choisie, `null` si aucune).
pub const SETTINGS_KEY: &str = "extra_language";

/// Dossiers explorés : `langues/` à côté de l'exécutable puis à côté des réglages.
pub fn dirs(settings_path: Option<&Path>) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
    {
        out.push(dir.join(langfile::DIR_NAME));
    }
    if let Some(dir) = settings_path.and_then(Path::parent) {
        out.push(dir.join(langfile::DIR_NAME));
    }
    out
}

/// Charge les langues des dossiers donnés (un code déjà chargé est refusé) ; renvoie les
/// refus `(fichier, message)`. La liste des langues disponibles est remplacée.
pub fn load_dirs(dirs: &[PathBuf]) -> Vec<(String, String)> {
    let mut loaded: Vec<langfile::ExtraLang> = Vec::new();
    let mut refused = Vec::new();
    for dir in dirs {
        let report = langfile::load_dir(dir);
        for lang in report.loaded {
            if loaded.iter().any(|l| l.code == lang.code) {
                let name = format!("{}{}", lang.code, langfile::SUFFIX);
                let err = langfile::LangFileError::ReservedCode(lang.code);
                refused.push((name, err.message()));
            } else {
                loaded.push(lang);
            }
        }
        refused.extend(report.refused.into_iter().map(|(f, e)| (f, e.message())));
    }
    langfile::set_available(loaded);
    refused
}

impl App {
    /// Au lancement : charge les fichiers de langue, signale le premier refus, réactive la langue
    /// supplémentaire mémorisée (sauf si `--lang` l'a imposée).
    pub(crate) fn init_extra_languages(&mut self, forced: bool) {
        let settings_path = self.settings_path.clone();
        let refused = load_dirs(&dirs(settings_path.as_deref()));
        if let Some((file, reason)) = refused.first() {
            self.editor.error(tr_args(
                "lang.extra.refused",
                &[("file", file), ("reason", reason)],
            ));
        }
        if forced {
            return;
        }
        if let Some(code) = settings_path
            .as_deref()
            .and_then(|p| settings::load_value(p, SETTINGS_KEY))
            .and_then(|v| v.as_str().map(str::to_string))
        {
            langfile::activate(&code);
        }
    }

    /// Choisit une langue supplémentaire disponible et mémorise le choix.
    pub fn set_extra_language(&mut self, code: &str) {
        if !langfile::activate(code) {
            return;
        }
        pf_core::input::set_decimal_separator(pf_i18n::Lang::En.decimal_separator());
        if let Some(path) = &self.settings_path {
            let _ = settings::save_value(path, SETTINGS_KEY, serde_json::json!(code));
        }
        self.editor.info(tr("lang.extra.activated"));
    }

    /// Oublie la langue supplémentaire mémorisée (choix du français ou de l'anglais).
    pub(crate) fn clear_extra_language_setting(&self) {
        if let Some(path) = &self.settings_path {
            let _ = settings::save_value(path, SETTINGS_KEY, serde_json::Value::Null);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Launch;
    use eframe::egui;
    use pf_i18n::{set_lang, Lang};

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("pf_extra_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    const ES: &str = r#"{"format":"patternforge-lang","version":1,"code":"es","name":"Español","messages":{"menu.file.title":"Archivo"}}"#;

    #[test]
    fn la_langue_choisie_est_memorisee_hors_projet_et_reactivee() {
        let dir = temp("choix");
        let lang_dir = dir.join("langues");
        std::fs::create_dir_all(&lang_dir).unwrap();
        std::fs::write(lang_dir.join("es.lang.json"), ES).unwrap();
        std::fs::write(lang_dir.join("it.lang.json"), "pas du json").unwrap();
        let settings_file = dir.join("settings.json");
        let mut app = App::with_context(&egui::Context::default(), Launch::default());
        app.settings_path = Some(settings_file.clone());
        app.init_extra_languages(false);
        assert!(app.editor.status.error, "le fichier it refusé est signalé");
        assert_eq!(langfile::available(), vec![("es".into(), "Español".into())]);
        app.set_extra_language("es");
        assert_eq!(tr("menu.file.title"), "Archivo");
        assert_eq!(
            settings::load_value(&settings_file, SETTINGS_KEY),
            Some(serde_json::json!("es"))
        );
        // Nouvelle session : la langue est réactivée.
        set_lang(Lang::Fr);
        let mut again = App::with_context(&egui::Context::default(), Launch::default());
        again.settings_path = Some(settings_file.clone());
        again.init_extra_languages(false);
        assert_eq!(tr("menu.file.title"), "Archivo");
        // Choisir le français l'oublie.
        again.set_language(Lang::Fr);
        assert_eq!(tr("menu.file.title"), "Fichier");
        assert_eq!(
            settings::load_value(&settings_file, SETTINGS_KEY),
            Some(serde_json::Value::Null)
        );
        // `--lang` l'emporte sur le réglage.
        let mut forced = App::with_context(&egui::Context::default(), Launch::default());
        forced.settings_path = Some(settings_file);
        forced.init_extra_languages(true);
        assert_eq!(tr("menu.file.title"), "Fichier");
        set_lang(Lang::Fr);
    }
}
