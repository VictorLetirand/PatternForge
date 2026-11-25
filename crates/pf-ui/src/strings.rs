//! Textes d'interface du design system, regroupés pour être traduits plus tard.
//! Les composants ne contiennent aucun texte : ils reçoivent ces structures
//! (français par défaut) ou des `&str` en paramètre.

/// Libellés des niveaux de message (en plus de la couleur).
#[derive(Clone, Debug, PartialEq)]
pub struct ToastLabels {
    pub info: String,
    pub success: String,
    pub warning: String,
    pub error: String,
    /// Libellé du bouton de fermeture d'un message.
    pub dismiss: String,
}

impl Default for ToastLabels {
    fn default() -> Self {
        Self {
            info: "Information".into(),
            success: "Succès".into(),
            warning: "Avertissement".into(),
            error: "Erreur".into(),
            dismiss: "Fermer".into(),
        }
    }
}

/// Textes du champ numérique.
#[derive(Clone, Debug, PartialEq)]
pub struct NumericStrings {
    /// Préfixe de l'erreur en ligne (l'erreur reste lisible sans la couleur).
    pub error_prefix: String,
    /// Modèle d'erreur hors bornes ; `{min}` et `{max}` sont remplacés.
    pub out_of_range: String,
    /// Infobulle du libellé d'unité glissable.
    pub scrub_hint: String,
}

impl Default for NumericStrings {
    fn default() -> Self {
        Self {
            error_prefix: "Erreur : ".into(),
            out_of_range: "valeur hors bornes (entre {min} et {max})".into(),
            scrub_hint: "Glisser horizontalement pour ajuster".into(),
        }
    }
}

/// Textes propres au catalogue (UI Lab).
pub mod lab {
    pub const TITLE: &str = "UI Lab — Precision Light";
    pub const THEME_DARK: &str = "Thème sombre (expérimental)";
    pub const PAGE_TOKENS: &str = "Couleurs et jetons";
    pub const PAGE_BUTTONS: &str = "Boutons et outils";
    pub const PAGE_NUMERIC: &str = "Champs numériques";
    pub const PAGE_MESSAGES: &str = "Messages et dialogues";
    pub const PAGE_INSPECTOR: &str = "Inspecteur type";

    pub const SECTION_SURFACES: &str = "Jetons de couleur";
    pub const SECTION_ROLES: &str = "Rôles de tracé (couleur et style de trait)";
    pub const SECTION_SPACING: &str = "Espacements (échelle fixe)";
    pub const ROLE_CONSTRUCTION: &str = "Construction (tirets longs)";
    pub const ROLE_CUTTING: &str = "Coupe (trait plein épais)";
    pub const ROLE_SEWING: &str = "Couture (pointillés)";
    pub const ROLE_GRAINLINE: &str = "Droit-fil (flèche)";
    pub const ROLE_FOLD: &str = "Pli (tiret-point)";
    pub const ROLE_AXIS: &str = "Axe (tirets courts)";

    pub const SECTION_TOOLS: &str = "Boutons d'outil";
    pub const TOOL_SELECT: &str = "Sélection";
    pub const TOOL_SEGMENT: &str = "Segment";
    pub const TOOL_POINT: &str = "Point";
    pub const TOOL_CURVE: &str = "Courbe";
    pub const TOOL_MEASURE: &str = "Mesure";
    pub const STATE_NORMAL: &str = "Normal";
    pub const STATE_SELECTED: &str = "Sélectionné";
    pub const STATE_DISABLED: &str = "Désactivé";
    pub const STATE_HINT: &str = "Survol, appui et focus clavier (Tab) : à essayer ci-dessus.";
    pub const SECTION_BUTTONS: &str = "Boutons";
    pub const BTN_PRIMARY: &str = "Valider";
    pub const BTN_SECONDARY: &str = "Annuler";
    pub const BTN_DANGER: &str = "Supprimer";
    pub const SECTION_HINTS: &str = "Pastilles de raccourci";

    pub const SECTION_NUMERIC: &str = "Champ numérique";
    pub const NUMERIC_HELP: &str = "Essayer : 120 · 120,5 · 12 cm · 1,2 m · abc (erreur). \
        Entrée ou perte de focus valide, Échap annule, flèches haut/bas incrémentent, \
        glisser sur l'unité ajuste.";
    pub const FIELD_LENGTH: &str = "Longueur";
    pub const FIELD_ANGLE: &str = "Angle (0 à 360)";
    pub const FIELD_STEP: &str = "Pas fin 0,1 mm (0 à 50)";
    pub const FIELD_DISABLED: &str = "Désactivé";
    pub const VALUE_PREFIX: &str = "Valeur interne : ";

    pub const SECTION_TOASTS: &str = "Messages temporaires";
    pub const TOAST_INFO: &str = "Projet rechargé depuis le disque.";
    pub const TOAST_SUCCESS: &str = "Export PDF enregistré.";
    pub const TOAST_WARNING: &str = "Courbe sans longueur imposée.";
    pub const TOAST_ERROR: &str = "Référence manquante : point P3.";
    pub const SHOW_INFO: &str = "Afficher une information";
    pub const SHOW_SUCCESS: &str = "Afficher un succès";
    pub const SHOW_WARNING: &str = "Afficher un avertissement";
    pub const SHOW_ERROR: &str = "Afficher une erreur";
    pub const SECTION_DIALOG: &str = "Dialogue de confirmation";
    pub const OPEN_DIALOG: &str = "Ouvrir le dialogue";
    pub const DIALOG_TITLE: &str = "Supprimer la feuille ?";
    pub const DIALOG_BODY: &str =
        "La feuille « Devant » et ses 12 entités seront supprimées. Action annulable avec Ctrl+Z.";
    pub const DIALOG_CANCEL: &str = "Annuler";
    pub const DIALOG_CONFIRM: &str = "Supprimer la feuille";
    pub const DIALOG_RESULT_PREFIX: &str = "Dernier choix : ";
    pub const CHOICE_NONE: &str = "aucun";
    pub const CHOICE_CANCELLED: &str = "annulé";
    pub const CHOICE_CONFIRMED: &str = "suppression confirmée";

    pub const SECTION_GEOMETRY: &str = "Géométrie";
    pub const SECTION_SEGMENT: &str = "Segment AB";
    pub const PROP_ID: &str = "Identifiant";
    pub const PROP_SHEET: &str = "Feuille";
    pub const PROP_LENGTH_MEASURED: &str = "Longueur mesurée";
    pub const PROP_ROLE: &str = "Rôle";
    pub const PROP_ROLE_VALUE: &str = "Construction";
    pub const SECTION_PARAMS: &str = "Paramètres de construction";
    pub const PARAM_LENGTH: &str = "Longueur";
    pub const PARAM_ANGLE: &str = "Angle";
    pub const PARAM_OFFSET: &str = "Décalage";
}
