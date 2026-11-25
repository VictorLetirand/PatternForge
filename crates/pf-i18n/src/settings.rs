//! Réglages de l'application, hors projet : aujourd'hui la seule langue.
//!
//! Fichier `settings.json` (`{"language":"fr"}`) dans `%APPDATA%\PatternCraft\`
//! (Windows) ou `$XDG_CONFIG_HOME/patterncraft` / `$HOME/.config/patterncraft`.
//! Toutes les fonctions prennent le chemin en paramètre : les tests utilisent
//! un dossier temporaire et n'écrivent jamais dans le dossier utilisateur.

use crate::Lang;
use serde_json::{json, Map, Value};
use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};

/// Nom du fichier de réglages.
pub const FILE_NAME: &str = "settings.json";

/// Dossier de configuration d'après des variables d'environnement fournies
/// par l'appelant (testable sans toucher à l'environnement réel).
pub fn config_dir_from(windows: bool, var: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    let non_empty = |name: &str| var(name).filter(|v| !v.is_empty());
    if windows {
        non_empty("APPDATA").map(|a| PathBuf::from(a).join("PatternCraft"))
    } else if let Some(xdg) = non_empty("XDG_CONFIG_HOME") {
        Some(PathBuf::from(xdg).join("patterncraft"))
    } else {
        non_empty("HOME").map(|h| PathBuf::from(h).join(".config").join("patterncraft"))
    }
}

/// Chemin par défaut de `settings.json` pour l'utilisateur courant.
pub fn default_settings_path() -> Option<PathBuf> {
    config_dir_from(cfg!(windows), |name| std::env::var_os(name)).map(|d| d.join(FILE_NAME))
}

/// Lit la langue enregistrée. Fichier absent, illisible, corrompu ou valeur
/// inconnue : français. Ne modifie jamais le fichier.
pub fn load_language(path: &Path) -> Lang {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Lang::Fr;
    };
    let Ok(value) = serde_json::from_str::<Value>(&text) else {
        return Lang::Fr;
    };
    value
        .get("language")
        .and_then(Value::as_str)
        .and_then(Lang::from_code)
        .unwrap_or(Lang::Fr)
}

/// Enregistre la langue, en créant le dossier au besoin. Les autres clés d'un
/// fichier valide existant sont conservées ; un fichier illisible est remplacé.
/// L'écriture passe par un fichier temporaire puis un renommage.
pub fn save_language(path: &Path, lang: Lang) -> io::Result<()> {
    save_value(path, "language", json!(lang.code()))
}

/// Lit une clé quelconque du fichier de réglages (`None` si fichier absent,
/// illisible, non objet ou clé absente). Ne modifie jamais le fichier.
pub fn load_value(path: &Path, key: &str) -> Option<Value> {
    let text = std::fs::read_to_string(path).ok()?;
    match serde_json::from_str::<Value>(&text).ok()? {
        Value::Object(mut map) => map.remove(key),
        _ => None,
    }
}

/// Enregistre une clé en conservant les autres (écriture atomique).
pub fn save_value(path: &Path, key: &str, value: Value) -> io::Result<()> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    let mut map = match std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
    {
        Some(Value::Object(map)) => map,
        _ => Map::new(),
    };
    map.insert(key.to_string(), value);
    let text = serde_json::to_string_pretty(&Value::Object(map))
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Dossier temporaire unique, supprimé en fin de test.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> TempDir {
            let dir = std::env::temp_dir().join(format!(
                "pf-i18n-test-{}-{tag}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            TempDir(dir)
        }
        fn file(&self) -> PathBuf {
            self.0.join(FILE_NAME)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn fichier_absent_donne_le_francais() {
        let tmp = TempDir::new("absent");
        assert_eq!(load_language(&tmp.file()), Lang::Fr);
        assert!(!tmp.0.exists(), "la lecture ne crée rien");
    }

    #[test]
    fn fichier_corrompu_ou_valeur_inconnue_donne_le_francais_sans_toucher_au_fichier() {
        let tmp = TempDir::new("corrompu");
        std::fs::create_dir_all(&tmp.0).expect("dossier temporaire");
        for content in [
            "",
            "pas du json {",
            "[1,2,3]",
            "{}",
            r#"{"language":42}"#,
            r#"{"language":"klingon"}"#,
            r#"{"language":null}"#,
            "\u{0}\u{1}\u{2}",
        ] {
            std::fs::write(tmp.file(), content).expect("écriture de test");
            assert_eq!(load_language(&tmp.file()), Lang::Fr, "{content:?}");
            assert_eq!(
                std::fs::read_to_string(tmp.file()).ok().as_deref(),
                Some(content),
                "le fichier ne doit pas être modifié"
            );
        }
    }

    #[test]
    fn octets_invalides_donnent_le_francais() {
        let tmp = TempDir::new("binaire");
        std::fs::create_dir_all(&tmp.0).expect("dossier temporaire");
        std::fs::write(tmp.file(), [0xff, 0xfe, 0xfd]).expect("écriture de test");
        assert_eq!(load_language(&tmp.file()), Lang::Fr);
    }

    #[test]
    fn aller_retour() {
        let tmp = TempDir::new("aller-retour");
        for lang in [Lang::En, Lang::Fr, Lang::En] {
            save_language(&tmp.file(), lang).expect("enregistrement");
            assert_eq!(load_language(&tmp.file()), lang);
        }
        let text = std::fs::read_to_string(tmp.file()).expect("lecture");
        let value: Value = serde_json::from_str(&text).expect("json valide");
        assert_eq!(value["language"], "en");
        assert!(!tmp.0.join("settings.json.tmp").exists());
    }

    #[test]
    fn dossier_inexistant_cree() {
        let tmp = TempDir::new("cree");
        let path = tmp.0.join("a").join("b").join(FILE_NAME);
        save_language(&path, Lang::En).expect("enregistrement");
        assert_eq!(load_language(&path), Lang::En);
    }

    #[test]
    fn les_autres_cles_sont_conservees() {
        let tmp = TempDir::new("autres");
        std::fs::create_dir_all(&tmp.0).expect("dossier temporaire");
        std::fs::write(tmp.file(), r#"{"theme":"clair","language":"fr"}"#).expect("écriture");
        save_language(&tmp.file(), Lang::En).expect("enregistrement");
        let value: Value =
            serde_json::from_str(&std::fs::read_to_string(tmp.file()).expect("lecture"))
                .expect("json valide");
        assert_eq!(value["theme"], "clair");
        assert_eq!(value["language"], "en");
    }

    #[test]
    fn valeur_quelconque_aller_retour_sans_perdre_la_langue() {
        let tmp = TempDir::new("valeur");
        save_language(&tmp.file(), Lang::En).expect("langue");
        save_value(&tmp.file(), "shortcuts", json!({"notch": "K"})).expect("valeur");
        assert_eq!(load_language(&tmp.file()), Lang::En);
        assert_eq!(
            load_value(&tmp.file(), "shortcuts"),
            Some(json!({"notch": "K"}))
        );
        assert_eq!(load_value(&tmp.file(), "absente"), None);
        assert_eq!(load_value(&tmp.0.join("nope.json"), "shortcuts"), None);
    }

    #[test]
    fn fichier_corrompu_remplace_a_l_enregistrement_explicite() {
        let tmp = TempDir::new("remplace");
        std::fs::create_dir_all(&tmp.0).expect("dossier temporaire");
        std::fs::write(tmp.file(), "n'importe quoi").expect("écriture");
        save_language(&tmp.file(), Lang::En).expect("enregistrement");
        assert_eq!(load_language(&tmp.file()), Lang::En);
    }

    #[test]
    fn chemin_d_un_dossier_donne_le_francais() {
        let tmp = TempDir::new("dossier");
        std::fs::create_dir_all(&tmp.0).expect("dossier temporaire");
        assert_eq!(load_language(&tmp.0), Lang::Fr);
    }

    #[test]
    fn dossier_de_configuration_selon_la_plateforme() {
        let env = |pairs: &'static [(&'static str, &'static str)]| {
            move |name: &str| {
                pairs
                    .iter()
                    .find(|(k, _)| *k == name)
                    .map(|(_, v)| OsString::from(*v))
            }
        };
        assert_eq!(
            config_dir_from(true, env(&[("APPDATA", "C:\\Roaming")])),
            Some(PathBuf::from("C:\\Roaming").join("PatternCraft"))
        );
        assert_eq!(config_dir_from(true, env(&[("HOME", "/home/x")])), None);
        assert_eq!(
            config_dir_from(
                false,
                env(&[("XDG_CONFIG_HOME", "/cfg"), ("HOME", "/home/x")])
            ),
            Some(PathBuf::from("/cfg").join("patterncraft"))
        );
        assert_eq!(
            config_dir_from(false, env(&[("HOME", "/home/x")])),
            Some(
                PathBuf::from("/home/x")
                    .join(".config")
                    .join("patterncraft")
            )
        );
        assert_eq!(
            config_dir_from(false, env(&[("XDG_CONFIG_HOME", ""), ("HOME", "/h")])),
            Some(PathBuf::from("/h").join(".config").join("patterncraft"))
        );
        assert_eq!(config_dir_from(false, env(&[])), None);
    }
}
