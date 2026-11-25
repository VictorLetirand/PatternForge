//! US-16.8 : noms accessibles des contrôles (lecteurs d'écran, AccessKit).
//!
//! Convention de l'interface : une étiquette précède son contrôle (grille « étiquette, champ »).
//! `label` retient le dernier texte posé ; `add_text`, `add_drag` et `ComboExt::show_ui_named`
//! donnent ce texte comme **nom accessible** au champ qui suit. Une unité ou un texte sans lettres (« mm »,
//! « ° ») n'est jamais retenu comme nom ; une lettre seule (« X ») l'est. Rien ne change à l'écran. Ce qui ne peut être vérifié
//! sans lecteur d'écran réel (annonce, ordre de lecture) reste **[HUMAIN]**.

use eframe::egui::{self, Response, WidgetText, WidgetType};
use std::cell::RefCell;

struct Last {
    text: String,
    uses: u8,
}

thread_local! {
    static LAST: RefCell<Option<Last>> = const { RefCell::new(None) };
}

/// Nombre de contrôles qu'une même étiquette peut nommer (une ligne de tableau : une cellule
/// par taille).
const MAX_USES: u8 = 8;

/// Unités et symboles qui suivent un champ : jamais un nom.
const UNITS: [&str; 9] = ["mm", "cm", "px", "pt", "m", "in", "deg", "kg", "s"];

/// Longueur maximale d'un nom de champ (caractères) : au-delà, c'est une phrase d'état ou d'aide.
const MAX_NAME_CHARS: usize = 48;

fn usable(text: &str) -> bool {
    let t = text.trim().trim_end_matches(':').trim();
    t.chars().any(char::is_alphabetic)
        && !UNITS.contains(&t.to_lowercase().as_str())
        && t.chars().count() <= MAX_NAME_CHARS
        && !t.ends_with(['.', '!', '?'])
        && t.chars().next().is_some_and(char::is_alphanumeric)
}

/// Comme `ui.label`, en retenant le texte comme nom du prochain contrôle.
pub fn label(ui: &mut egui::Ui, text: impl Into<WidgetText>) -> Response {
    let text = text.into();
    let plain = text.text().to_string();
    if usable(&plain) {
        LAST.with(|l| {
            *l.borrow_mut() = Some(Last {
                text: plain,
                uses: 0,
            })
        });
    }
    ui.label(text)
}

/// Oublie l'étiquette retenue (début d'une autre fenêtre, d'un autre panneau).
pub fn forget() {
    LAST.with(|l| *l.borrow_mut() = None);
}

fn next_name() -> Option<String> {
    LAST.with(|l| {
        let mut slot = l.borrow_mut();
        let last = slot.as_mut()?;
        last.uses += 1;
        let name = last.text.clone();
        if last.uses >= MAX_USES {
            *slot = None;
        }
        Some(name)
    })
}

/// Donne un nom accessible à la réponse d'un contrôle, sans toucher à son rôle ni à sa valeur.
pub fn named(response: Response, _typ: WidgetType, name: &str) -> Response {
    response
        .ctx
        .accesskit_node_builder(response.id, |builder| builder.set_label(name));
    response
}

/// Le nom du champ est l'étiquette suivie de « (champ) » : il diffère du texte de l'étiquette
/// elle-même, que le lecteur d'écran lit juste avant, et reste traduit avec la langue.
fn field_name(label: &str) -> String {
    format!(
        "{} ({})",
        label.trim().trim_end_matches(':').trim_end(),
        pf_i18n::tr("a11y.field_suffix")
    )
}

fn named_by_last(response: Response, typ: WidgetType) -> Response {
    match next_name() {
        Some(name) => named(response, typ, &field_name(&name)),
        None => response,
    }
}

/// `ui.add(TextEdit…)`, nommé par l'étiquette qui précède.
pub fn add_text(ui: &mut egui::Ui, edit: egui::TextEdit<'_>) -> Response {
    let response = ui.add(edit);
    named_by_last(response, WidgetType::TextEdit)
}

/// `ui.add(DragValue…)`, nommé par l'étiquette qui précède.
pub fn add_drag(ui: &mut egui::Ui, drag: egui::DragValue<'_>) -> Response {
    let response = ui.add(drag);
    named_by_last(response, WidgetType::DragValue)
}

/// `ui.add_enabled(enabled, TextEdit…)`, nommé par l'étiquette qui précède.
pub fn add_text_enabled(ui: &mut egui::Ui, enabled: bool, edit: egui::TextEdit<'_>) -> Response {
    let response = ui.add_enabled(enabled, edit);
    named_by_last(response, WidgetType::TextEdit)
}

/// `ComboBox::show_ui`, nommé par l'étiquette qui précède.
pub trait ComboExt {
    fn show_ui_named<R>(
        self,
        ui: &mut egui::Ui,
        add_contents: impl FnOnce(&mut egui::Ui) -> R,
    ) -> egui::InnerResponse<Option<R>>;
}

impl ComboExt for egui::ComboBox {
    fn show_ui_named<R>(
        self,
        ui: &mut egui::Ui,
        add_contents: impl FnOnce(&mut egui::Ui) -> R,
    ) -> egui::InnerResponse<Option<R>> {
        let mut inner = self.show_ui(ui, add_contents);
        inner.response = named_by_last(inner.response, WidgetType::ComboBox);
        inner
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn le_nom_d_un_champ_est_l_etiquette_suivie_du_mot_champ() {
        assert_eq!(field_name("Longueur"), "Longueur (champ)");
        assert_eq!(field_name("Longueur :"), "Longueur (champ)");
        assert_eq!(field_name("  Marge (mm) "), "Marge (mm) (champ)");
        pf_i18n::set_lang(pf_i18n::Lang::En);
        assert_eq!(field_name("Length"), "Length (field)");
        pf_i18n::set_lang(pf_i18n::Lang::Fr);
    }

    #[test]
    fn un_texte_sans_lettres_n_est_jamais_un_nom() {
        assert!(!usable("mm"));
        assert!(!usable("MM"));
        assert!(usable("X"));
        assert!(usable("Y :"));
        assert!(!usable("°"));
        assert!(!usable("%"));
        assert!(!usable(""));
        assert!(!usable("12"));
        assert!(!usable("26 perçage(s) posé(s) en une étape."));
        assert!(!usable(&"x".repeat(49)));
        assert!(usable("Distance au bord"));
        assert!(!usable("⚠ hors du contour"));
        assert!(usable("Longueur"));
        assert!(usable("Âge"));
    }

    #[test]
    fn une_etiquette_nomme_au_plus_huit_controles_puis_s_efface() {
        forget();
        assert_eq!(next_name(), None);
        LAST.with(|l| {
            *l.borrow_mut() = Some(Last {
                text: "Marge".into(),
                uses: 0,
            })
        });
        for _ in 0..MAX_USES {
            assert_eq!(next_name().as_deref(), Some("Marge"));
        }
        assert_eq!(next_name(), None);
    }
}
