//! Internationalisation de PatternForge : catalogues FR/EN, langue courante
//! et réglage persistant (hors projet).
//!
//! La langue courante est propre à chaque fil d'exécution (`thread_local!`) :
//! l'interface egui tourne sur un seul fil, et les tests (un fil par test) ne
//! s'influencent pas. Français par défaut.
//!
//! Résolution d'une clé : langue courante, puis français, puis la clé elle-même.
//! Aucune panique possible.

mod en;
mod fr;
pub mod langfile;
pub mod settings;

use std::cell::Cell;

/// Langues de l'interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Lang {
    #[default]
    Fr,
    En,
}

impl Lang {
    pub const ALL: [Lang; 2] = [Lang::Fr, Lang::En];

    /// Code stable utilisé dans `settings.json` et `--lang`.
    pub fn code(self) -> &'static str {
        match self {
            Lang::Fr => "fr",
            Lang::En => "en",
        }
    }

    /// Lit un code de langue (insensible à la casse) ; `None` si inconnu.
    pub fn from_code(code: &str) -> Option<Lang> {
        match code.trim().to_ascii_lowercase().as_str() {
            "fr" => Some(Lang::Fr),
            "en" => Some(Lang::En),
            _ => None,
        }
    }

    /// Nom de la langue dans sa propre langue (jamais traduit).
    pub fn native_name(self) -> &'static str {
        match self {
            Lang::Fr => "Français",
            Lang::En => "English",
        }
    }

    /// Séparateur décimal d'affichage usuel.
    pub fn decimal_separator(self) -> char {
        match self {
            Lang::Fr => ',',
            Lang::En => '.',
        }
    }

    /// Catalogue brut (clé, texte) de la langue.
    pub fn catalog(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Lang::Fr => fr::CATALOG,
            Lang::En => en::CATALOG,
        }
    }
}

thread_local! {
    static LANG: Cell<Lang> = const { Cell::new(Lang::Fr) };
}

/// Change la langue courante (fil d'exécution courant).
pub fn set_lang(lang: Lang) {
    langfile::deactivate();
    LANG.with(|l| l.set(lang));
}

/// Langue courante (français par défaut).
pub fn lang() -> Lang {
    LANG.with(Cell::get)
}

type Catalog = &'static [(&'static str, &'static str)];

fn find(catalog: Catalog, key: &str) -> Option<&'static str> {
    catalog.iter().find(|(k, _)| *k == key).map(|(_, v)| *v)
}

/// Catalogue principal, puis repli, puis la clé elle-même.
fn resolve(primary: Catalog, fallback: Catalog, key: &str) -> String {
    find(primary, key)
        .or_else(|| find(fallback, key))
        .unwrap_or(key)
        .to_string()
}

/// Texte d'une clé dans une langue donnée, avec repli sur le français puis
/// sur la clé elle-même.
pub fn lookup(lang: Lang, key: &str) -> String {
    resolve(lang.catalog(), fr::CATALOG, key)
}

thread_local! {
    static LENGTH_UNIT: Cell<Option<&'static str>> = const { Cell::new(None) };
}

/// Unité d'affichage des longueurs de l'interface (fil courant) : `Some("cm")` remplace le
/// repère « (mm) » des libellés de champ par « (cm) » ; `None` (défaut) ou `Some("mm")` ne
/// change rien. Les exports lisent `lookup` et ne sont jamais concernés.
pub fn set_length_unit_word(word: Option<&'static str>) {
    LENGTH_UNIT.with(|c| c.set(word.filter(|w| *w != "mm")));
}

/// Texte d'une clé dans la langue courante.
pub fn tr(key: &str) -> String {
    let text = langfile::active_text(key).unwrap_or_else(|| lookup(lang(), key));
    match LENGTH_UNIT.with(Cell::get) {
        Some(word) if text.contains("(mm)") => text.replace("(mm)", &format!("({word})")),
        _ => text,
    }
}

/// Gabarit de longueurs : chaque « {x} mm » (« mm » suivi d'autre chose qu'une lettre, un chiffre
/// ou `²`) devient « {x} cm » ou « {x}" » selon l'unité d'affichage.
fn lengths_in_unit(text: &str, word: &str) -> String {
    let suffix = if word == "in" { "\"" } else { " cm" };
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find("} mm") {
        let after = &rest[i + 4..];
        let unit_follows = after
            .chars()
            .next()
            .is_some_and(|c| c.is_alphanumeric() || c == '²');
        out.push_str(&rest[..=i]);
        if unit_follows {
            out.push_str(" mm");
        } else {
            out.push_str(suffix);
        }
        rest = after;
    }
    out.push_str(rest);
    out
}

/// Comme [`tr_args`] pour un gabarit dont les valeurs sont des **longueurs déjà converties** dans
/// l'unité d'affichage (sans suffixe) : « {x} mm » prend l'unité courante. En mm : identique à
/// `tr_args`.
pub fn tr_len(key: &str, args: &[(&str, &str)]) -> String {
    let mut text = tr(key);
    if let Some(word) = LENGTH_UNIT.with(Cell::get) {
        text = lengths_in_unit(&text, word);
    }
    for (name, value) in args {
        text = text.replace(&format!("{{{name}}}"), value);
    }
    text
}

/// Comme [`tr`], en remplaçant chaque `{nom}` par la valeur associée.
pub fn tr_args(key: &str, args: &[(&str, &str)]) -> String {
    let mut text = tr(key);
    for (name, value) in args {
        text = text.replace(&format!("{{{name}}}"), value);
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn keys(lang: Lang) -> BTreeSet<&'static str> {
        lang.catalog().iter().map(|(k, _)| *k).collect()
    }

    /// Noms entre accolades d'un texte, dans l'ordre alphabétique.
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

    #[test]
    fn meme_ensemble_de_cles_en_fr_et_en() {
        let (fr, en) = (keys(Lang::Fr), keys(Lang::En));
        let only_fr: Vec<_> = fr.difference(&en).collect();
        let only_en: Vec<_> = en.difference(&fr).collect();
        assert!(only_fr.is_empty(), "clés absentes de EN : {only_fr:?}");
        assert!(only_en.is_empty(), "clés absentes de FR : {only_en:?}");
    }

    #[test]
    fn cles_uniques_dans_chaque_catalogue() {
        // Une clé écrite deux fois n'est pas détectée à la compilation : la seconde
        // entrée est ignorée en silence (la collision de `status.split.done`, US-04.6).
        for lang in Lang::ALL {
            let mut seen = BTreeSet::new();
            let duplicated: Vec<&str> = lang
                .catalog()
                .iter()
                .map(|(key, _)| *key)
                .filter(|key| !seen.insert(*key))
                .collect();
            assert!(
                duplicated.is_empty(),
                "clés en double dans {} : {duplicated:?}",
                lang.code()
            );
        }
    }

    #[test]
    fn memes_placeholders_dans_les_deux_langues() {
        for (key, fr) in Lang::Fr.catalog() {
            let en = lookup(Lang::En, key);
            assert_eq!(
                placeholders(fr),
                placeholders(&en),
                "placeholders différents pour « {key} »"
            );
        }
    }

    #[test]
    fn aucune_valeur_vide() {
        for lang in Lang::ALL {
            for (key, value) in lang.catalog() {
                assert!(
                    !value.trim().is_empty(),
                    "valeur vide pour « {key} » ({})",
                    lang.code()
                );
            }
        }
    }

    #[test]
    fn cles_nommees_par_zone() {
        for (key, _) in Lang::Fr.catalog() {
            assert!(
                key.contains('.')
                    && key.chars().all(|c| c.is_ascii_lowercase()
                        || c.is_ascii_digit()
                        || c == '.'
                        || c == '_'),
                "clé mal formée : {key}"
            );
        }
    }

    #[test]
    fn francais_par_defaut_et_changement_de_langue() {
        assert_eq!(lang(), Lang::Fr);
        assert_eq!(tr("menu.file.title"), "Fichier");
        set_lang(Lang::En);
        assert_eq!(lang(), Lang::En);
        assert_eq!(tr("menu.file.title"), "File");
        set_lang(Lang::Fr);
        assert_eq!(tr("menu.file.title"), "Fichier");
    }

    #[test]
    fn la_langue_est_propre_a_chaque_fil() {
        set_lang(Lang::En);
        let other = std::thread::spawn(lang).join().ok();
        assert_eq!(other, Some(Lang::Fr));
        assert_eq!(lang(), Lang::En);
    }

    #[test]
    fn tr_args_remplace_les_placeholders() {
        assert_eq!(
            tr_args("dialog.unsaved.body", &[("name", "Robe")]),
            "Le projet « Robe » a été modifié. Voulez-vous l'enregistrer ?"
        );
        set_lang(Lang::En);
        assert_eq!(
            tr_args("dialog.unsaved.body", &[("name", "Dress")]),
            "The project \"Dress\" has been modified. Do you want to save it?"
        );
        // Argument inconnu : ignoré ; placeholder sans valeur : laissé tel quel.
        assert_eq!(
            tr_args("dialog.unsaved.body", &[("autre", "x")]),
            "The project \"{name}\" has been modified. Do you want to save it?"
        );
    }

    #[test]
    fn repli_sur_le_francais_puis_sur_la_cle() {
        // Clé inconnue partout : la clé elle-même, dans les deux langues.
        assert_eq!(lookup(Lang::Fr, "zone.inconnue"), "zone.inconnue");
        assert_eq!(lookup(Lang::En, "zone.inconnue"), "zone.inconnue");
        assert_eq!(tr(""), "");
        assert_eq!(tr_args("zone.inconnue", &[("a", "b")]), "zone.inconnue");
    }

    #[test]
    fn repli_avec_catalogues_synthetiques() {
        const EN: Catalog = &[("a.un", "one")];
        const FR: Catalog = &[("a.un", "un"), ("a.deux", "deux")];
        assert_eq!(resolve(EN, FR, "a.un"), "one");
        assert_eq!(resolve(EN, FR, "a.deux"), "deux", "repli sur le français");
        assert_eq!(resolve(EN, FR, "a.trois"), "a.trois", "repli sur la clé");
    }

    #[test]
    fn codes_de_langue() {
        for lang in Lang::ALL {
            assert_eq!(Lang::from_code(lang.code()), Some(lang));
        }
        assert_eq!(Lang::from_code(" EN "), Some(Lang::En));
        assert_eq!(Lang::from_code("es"), None);
        assert_eq!(Lang::from_code(""), None);
        assert_eq!(Lang::Fr.decimal_separator(), ',');
        assert_eq!(Lang::En.decimal_separator(), '.');
    }

    #[test]
    fn tr_len_suit_l_unite_sans_toucher_aux_aires_ni_aux_mots() {
        assert_eq!(lengths_in_unit("a {x} mm b", "cm"), "a {x} cm b");
        assert_eq!(lengths_in_unit("a {x} mm.", "in"), "a {x}\".");
        assert_eq!(lengths_in_unit("{x} mm²", "in"), "{x} mm²");
        assert_eq!(lengths_in_unit("{x} mmm", "cm"), "{x} mmm");
        assert_eq!(
            lengths_in_unit("{w} × {h} mm ({a} à {b} mm)", "cm"),
            "{w} × {h} cm ({a} à {b} cm)"
        );
        // Par défaut (et en mm) : identique à `tr_args`.
        set_length_unit_word(None);
        assert_eq!(
            tr_len("topstitch.distance", &[]),
            tr_args("topstitch.distance", &[])
        );
        set_length_unit_word(Some("mm"));
        assert_eq!(
            tr("topstitch.distance"),
            lookup(lang(), "topstitch.distance")
        );
        set_length_unit_word(Some("in"));
        assert!(tr("topstitch.distance").contains("(in)"));
        set_length_unit_word(None);
        assert!(tr("topstitch.distance").contains("(mm)"));
    }
}
