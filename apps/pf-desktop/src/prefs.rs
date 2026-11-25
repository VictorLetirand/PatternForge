//! US-16.7 - préférences de l'application : tolérance de sélection, valeurs par défaut (marge,
//! cran), couleur et motif de trait par rôle, export / import des réglages.
//!
//! Ce sont des **réglages de l'application** (clé `preferences` de `settings.json`), jamais une
//! donnée du projet : aucune `Command`, rien dans le fichier `.pforge.json`. Ils ne modifient
//! jamais un projet déjà enregistré : la tolérance règle seulement la prise à l'écran, la marge
//! et la longueur de cran par défaut ne servent qu'à une **nouvelle** action explicite, les
//! couleurs et motifs ne changent que l'affichage. Un rôle ne se distingue jamais par la couleur
//! seule : chaque rôle a un motif de trait propre (les six motifs sont toujours tous différents)
//! et toute couleur doit offrir un contraste d'au moins 3:1 avec le fond du plan.

use crate::a11y::ComboExt;
use crate::app::App;
use crate::palette::pal;
use eframe::egui::{self, Color32, RichText};
use pf_core::notch::{DEFAULT_NOTCH_LENGTH_MM, MAX_NOTCH_LENGTH_MM};
use pf_core::MAX_SEAM_MM;
use pf_i18n::{settings, tr, tr_args};
use pf_ui::tokens::{contrast_ratio, Tokens};
use serde_json::{json, Map, Value};
use std::cell::Cell;
use std::path::Path;

/// Clé du réglage dans `settings.json`.
pub const SETTINGS_KEY: &str = "preferences";
/// Marque et version du fichier d'export.
pub const FORMAT: &str = "patternforge-preferences";
pub const VERSION: u64 = 1;
/// Contraste minimal d'un trait de rôle avec le fond du plan (objets graphiques, WCAG 1.4.11).
pub const MIN_CONTRAST: f64 = 3.0;
/// Échelle de l'interface (US-16.8) : de 100 à 200 %, `pixels_per_point` = natif × échelle.
pub const UI_SCALE_MIN_PCT: u32 = 100;
pub const UI_SCALE_MAX_PCT: u32 = 200;
pub const PICK_RADIUS_MIN_PX: f64 = 3.0;
pub const PICK_RADIUS_MAX_PX: f64 = 24.0;
pub const NOTCH_MIN_MM: f64 = 0.5;
/// Marge proposée par défaut (10 mm).
pub const DEFAULT_MARGIN_MM: f64 = 10.0;

/// Rôles de tracé dont la couleur et le motif se règlent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Construction,
    Cutting,
    Sewing,
    Grainline,
    Fold,
    Axis,
}

impl Role {
    pub const ALL: [Role; 6] = [
        Role::Construction,
        Role::Cutting,
        Role::Sewing,
        Role::Grainline,
        Role::Fold,
        Role::Axis,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Role::Construction => "construction",
            Role::Cutting => "cutting",
            Role::Sewing => "sewing",
            Role::Grainline => "grainline",
            Role::Fold => "fold",
            Role::Axis => "axis",
        }
    }

    fn index(self) -> usize {
        Role::ALL.iter().position(|r| *r == self).unwrap_or(0)
    }

    pub fn label(self) -> String {
        match self {
            Role::Construction => tr("prefs.role.construction"),
            Role::Cutting => tr("prefs.role.cutting"),
            Role::Sewing => tr("prefs.role.sewing"),
            Role::Grainline => tr("prefs.role.grainline"),
            Role::Fold => tr("prefs.role.fold"),
            Role::Axis => tr("prefs.role.axis"),
        }
    }

    fn token(self, t: &Tokens) -> Color32 {
        match self {
            Role::Construction => t.role_construction,
            Role::Cutting => t.role_cutting,
            Role::Sewing => t.role_sewing,
            Role::Grainline => t.role_grainline,
            Role::Fold => t.role_fold,
            Role::Axis => t.role_axis,
        }
    }

    fn set_token(self, t: &mut Tokens, color: Color32) {
        match self {
            Role::Construction => t.role_construction = color,
            Role::Cutting => t.role_cutting = color,
            Role::Sewing => t.role_sewing = color,
            Role::Grainline => t.role_grainline = color,
            Role::Fold => t.role_fold = color,
            Role::Axis => t.role_axis = color,
        }
    }
}

/// Motif de trait : épaisseur (px) puis longueurs de traits et d'intervalles (px).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pattern {
    Thin,
    Thick,
    Dashed,
    Dotted,
    DashDot,
    DashDotDot,
}

impl Pattern {
    pub const ALL: [Pattern; 6] = [
        Pattern::Thin,
        Pattern::Thick,
        Pattern::Dashed,
        Pattern::Dotted,
        Pattern::DashDot,
        Pattern::DashDotDot,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Pattern::Thin => "thin",
            Pattern::Thick => "thick",
            Pattern::Dashed => "dashed",
            Pattern::Dotted => "dotted",
            Pattern::DashDot => "dash_dot",
            Pattern::DashDotDot => "dash_dot_dot",
        }
    }

    pub fn from_key(key: &str) -> Option<Pattern> {
        Pattern::ALL.into_iter().find(|p| p.key() == key)
    }

    pub fn label(self) -> String {
        match self {
            Pattern::Thin => tr("prefs.pattern.thin"),
            Pattern::Thick => tr("prefs.pattern.thick"),
            Pattern::Dashed => tr("prefs.pattern.dashed"),
            Pattern::Dotted => tr("prefs.pattern.dotted"),
            Pattern::DashDot => tr("prefs.pattern.dash_dot"),
            Pattern::DashDotDot => tr("prefs.pattern.dash_dot_dot"),
        }
    }

    pub fn style(self) -> (f32, &'static [f32], &'static [f32]) {
        match self {
            Pattern::Thin => (1.8, &[], &[]),
            Pattern::Thick => (3.0, &[], &[]),
            Pattern::Dashed => (1.8, &[8.0], &[5.0]),
            Pattern::Dotted => (1.5, &[2.0], &[4.0]),
            Pattern::DashDot => (1.8, &[12.0, 2.0], &[4.0, 4.0]),
            Pattern::DashDotDot => (1.8, &[18.0, 3.0, 3.0], &[4.0, 4.0, 4.0]),
        }
    }
}

const DEFAULT_PATTERNS: [Pattern; 6] = [
    Pattern::Thin,
    Pattern::Thick,
    Pattern::Dashed,
    Pattern::DashDotDot,
    Pattern::DashDot,
    Pattern::Dotted,
];

thread_local! {
    static PATTERNS: Cell<[Pattern; 6]> = const { Cell::new(DEFAULT_PATTERNS) };
}

/// Motif d'un rôle pour le dessin du plan (publié une fois par image par [`Prefs::publish`]).
pub fn role_pattern(role: Role) -> Pattern {
    PATTERNS.with(|p| p.get()[role.index()])
}

fn canvas_color(dark: bool) -> Color32 {
    if dark {
        Tokens::precision_dark().surface_canvas
    } else {
        Tokens::precision_light().surface_canvas
    }
}

fn default_token(dark: bool, role: Role) -> Color32 {
    role.token(&if dark {
        Tokens::precision_dark()
    } else {
        Tokens::precision_light()
    })
}

fn rgb_of(c: Color32) -> [u8; 3] {
    [c.r(), c.g(), c.b()]
}

fn color_of(rgb: [u8; 3]) -> Color32 {
    Color32::from_rgb(rgb[0], rgb[1], rgb[2])
}

fn hex(rgb: [u8; 3]) -> String {
    format!("#{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2])
}

fn parse_hex(text: &str) -> Option<[u8; 3]> {
    let digits = text.strip_prefix('#')?;
    if digits.len() != 6 || !digits.is_ascii() {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&digits[i..i + 2], 16).ok();
    Some([byte(0)?, byte(2)?, byte(4)?])
}

/// Contraste d'une couleur de trait avec le fond du plan du thème.
pub fn contrast_on_canvas(dark: bool, rgb: [u8; 3]) -> f64 {
    contrast_ratio(color_of(rgb), canvas_color(dark))
}

/// Préférences de l'utilisateur.
#[derive(Debug, Clone, PartialEq)]
pub struct Prefs {
    /// Rayon de prise (sélection, accrochage) en pixels.
    pub pick_radius_px: f64,
    /// Échelle de l'interface en pourcent (US-16.8), de 100 à 200.
    pub ui_scale_pct: u32,
    /// Marge de couture proposée par le bouton « marge par défaut » d'une pièce sans marge.
    pub default_margin_mm: f64,
    /// Longueur de cran proposée à la pose d'un cran (cran neuf seulement).
    pub default_notch_mm: f64,
    /// Couleurs modifiées, par thème (0 clair, 1 sombre) et par rôle ; `None` = couleur d'origine.
    colors: [[Option<[u8; 3]>; 6]; 2],
    /// Motif de chaque rôle : toujours six motifs tous différents.
    patterns: [Pattern; 6],
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs {
            pick_radius_px: crate::canvas::PICK_RADIUS_PX,
            ui_scale_pct: UI_SCALE_MIN_PCT,
            default_margin_mm: DEFAULT_MARGIN_MM,
            default_notch_mm: DEFAULT_NOTCH_LENGTH_MM,
            colors: [[None; 6]; 2],
            patterns: DEFAULT_PATTERNS,
        }
    }
}

impl Prefs {
    pub fn pattern(&self, role: Role) -> Pattern {
        self.patterns[role.index()]
    }

    /// Couleur effective d'un rôle dans un thème.
    pub fn color(&self, dark: bool, role: Role) -> [u8; 3] {
        self.colors[usize::from(dark)][role.index()]
            .unwrap_or_else(|| rgb_of(default_token(dark, role)))
    }

    /// Change une couleur ; refusée (rapport de contraste renvoyé) si elle se confond avec le fond.
    pub fn set_color(&mut self, dark: bool, role: Role, rgb: [u8; 3]) -> Result<(), f64> {
        let ratio = contrast_on_canvas(dark, rgb);
        if ratio < MIN_CONTRAST {
            return Err(ratio);
        }
        let default = rgb_of(default_token(dark, role));
        self.colors[usize::from(dark)][role.index()] = (rgb != default).then_some(rgb);
        Ok(())
    }

    /// Donne un motif à un rôle ; le rôle qui l'avait reçoit l'ancien (échange) : les six motifs
    /// restent tous différents, donc aucun rôle ne se lit par la couleur seule.
    pub fn set_pattern(&mut self, role: Role, pattern: Pattern) {
        let old = self.patterns[role.index()];
        if let Some(other) = self.patterns.iter().position(|p| *p == pattern) {
            self.patterns[other] = old;
        }
        self.patterns[role.index()] = pattern;
    }

    /// Applique les couleurs choisies aux jetons du thème courant.
    pub fn apply_roles(&self, tokens: &mut Tokens) {
        for role in Role::ALL {
            if let Some(rgb) = self.colors[usize::from(tokens.dark)][role.index()] {
                role.set_token(tokens, color_of(rgb));
            }
        }
    }

    /// Publie les motifs pour le dessin du plan.
    pub fn publish(&self) {
        PATTERNS.with(|p| p.set(self.patterns));
    }

    pub fn to_json(&self) -> Value {
        let mut colors = Map::new();
        for (name, dark) in [("light", false), ("dark", true)] {
            let mut theme = Map::new();
            for role in Role::ALL {
                if let Some(rgb) = self.colors[usize::from(dark)][role.index()] {
                    theme.insert(role.key().into(), json!(hex(rgb)));
                }
            }
            colors.insert(name.into(), Value::Object(theme));
        }
        let mut patterns = Map::new();
        for role in Role::ALL {
            patterns.insert(role.key().into(), json!(self.pattern(role).key()));
        }
        json!({
            "version": VERSION,
            "pick_radius_px": self.pick_radius_px,
            "ui_scale_pct": self.ui_scale_pct,
            "default_margin_mm": self.default_margin_mm,
            "default_notch_mm": self.default_notch_mm,
            "colors": colors,
            "patterns": patterns,
        })
    }

    /// Lecture tolérante : toute valeur absente, mal formée, hors bornes, de contraste
    /// insuffisant ou incohérente (motifs en double) redonne sa valeur par défaut. Renvoie aussi le
    /// nombre de valeurs présentes mais écartées.
    pub fn from_json_report(value: &Value) -> (Prefs, usize) {
        let mut prefs = Prefs::default();
        let mut ignored = 0;
        let Some(obj) = value.as_object() else {
            return (prefs, usize::from(!value.is_null()));
        };
        let mut number = |key: &str, min: f64, max: f64, slot: &mut f64| {
            if let Some(v) = obj.get(key) {
                match v
                    .as_f64()
                    .filter(|n| n.is_finite() && (min..=max).contains(n))
                {
                    Some(n) => *slot = n,
                    None => ignored += 1,
                }
            }
        };
        number(
            "pick_radius_px",
            PICK_RADIUS_MIN_PX,
            PICK_RADIUS_MAX_PX,
            &mut prefs.pick_radius_px,
        );
        number(
            "default_margin_mm",
            0.0,
            MAX_SEAM_MM,
            &mut prefs.default_margin_mm,
        );
        number(
            "default_notch_mm",
            NOTCH_MIN_MM,
            MAX_NOTCH_LENGTH_MM,
            &mut prefs.default_notch_mm,
        );
        if let Some(v) = obj.get("ui_scale_pct") {
            match v
                .as_u64()
                .filter(|n| (u64::from(UI_SCALE_MIN_PCT)..=u64::from(UI_SCALE_MAX_PCT)).contains(n))
            {
                Some(n) => prefs.ui_scale_pct = n as u32,
                None => ignored += 1,
            }
        }
        if let Some(colors) = obj.get("colors") {
            for (name, dark) in [("light", false), ("dark", true)] {
                let Some(theme) = colors.get(name) else {
                    continue;
                };
                let Some(theme) = theme.as_object() else {
                    ignored += 1;
                    continue;
                };
                for role in Role::ALL {
                    let Some(text) = theme.get(role.key()) else {
                        continue;
                    };
                    let ok = text
                        .as_str()
                        .and_then(parse_hex)
                        .is_some_and(|rgb| prefs.set_color(dark, role, rgb).is_ok());
                    if !ok {
                        ignored += 1;
                    }
                }
            }
        }
        if let Some(patterns) = obj.get("patterns") {
            let read: Option<Vec<Pattern>> = patterns.as_object().and_then(|o| {
                Role::ALL
                    .iter()
                    .map(|r| o.get(r.key())?.as_str().and_then(Pattern::from_key))
                    .collect()
            });
            match read {
                Some(list) if (0..6).all(|i| (0..i).all(|j| list[i] != list[j])) => {
                    prefs.patterns.copy_from_slice(&list);
                }
                _ => ignored += 1,
            }
        }
        (prefs, ignored)
    }

    pub fn from_json(value: &Value) -> Prefs {
        Prefs::from_json_report(value).0
    }
}

/// Préférences enregistrées (absentes ou illisibles : valeurs par défaut). Ne modifie jamais le fichier.
pub fn load_prefs(path: &Path) -> Prefs {
    settings::load_value(path, SETTINGS_KEY)
        .map(|v| Prefs::from_json(&v))
        .unwrap_or_default()
}

impl App {
    /// Enregistre les préférences ; un échec est signalé, l'état en mémoire reste.
    pub(crate) fn save_prefs(&mut self) {
        let Some(path) = self.settings_path.clone() else {
            return;
        };
        if let Err(e) = settings::save_value(&path, SETTINGS_KEY, self.prefs.to_json()) {
            self.editor.error(pf_i18n::tr_args(
                "prefs.save_error",
                &[("error", &e.to_string())],
            ));
        }
    }

    /// Écrit les réglages dans un fichier d'export.
    pub fn export_prefs_to(&mut self, path: &Path) -> bool {
        let mut value = self.prefs.to_json();
        value["format"] = json!(FORMAT);
        let text = serde_json::to_string_pretty(&value).unwrap_or_default();
        match std::fs::write(path, text) {
            Ok(()) => {
                self.editor.info(tr("prefs.exported"));
                true
            }
            Err(e) => {
                self.editor
                    .error(tr_args("prefs.export_error", &[("error", &e.to_string())]));
                false
            }
        }
    }

    /// Relit des réglages exportés : fichier d'une autre nature ou de version plus récente
    /// refusé sans rien changer ; valeurs douteuses écartées et signalées.
    pub fn import_prefs_from(&mut self, path: &Path) -> bool {
        let fail = |app: &mut App, reason: String| {
            app.editor
                .error(tr_args("prefs.import_error", &[("reason", &reason)]));
            false
        };
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) => return fail(self, e.to_string()),
        };
        let value: Value = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(e) => return fail(self, e.to_string()),
        };
        if value.get("format").and_then(Value::as_str) != Some(FORMAT) {
            return fail(self, tr("prefs.import_not_prefs"));
        }
        if value
            .get("version")
            .and_then(Value::as_u64)
            .is_none_or(|v| v == 0 || v > VERSION)
        {
            return fail(self, tr("prefs.import_version"));
        }
        let (prefs, ignored) = Prefs::from_json_report(&value);
        self.prefs = prefs;
        self.save_prefs();
        self.editor.info(if ignored == 0 {
            tr("prefs.imported")
        } else {
            tr_args("prefs.imported_partial", &[("n", &ignored.to_string())])
        });
        true
    }

    /// Fenêtre « Préférences ».
    pub fn prefs_window(&mut self, ctx: &egui::Context) {
        if !self.show_prefs {
            return;
        }
        let mut open = true;
        let mut prefs = self.prefs.clone();
        let dark = self.dark;
        let muted = pal().muted;
        let (mut export, mut import) = (false, false);
        let mut refused = None;
        egui::Window::new(tr("prefs.title"))
            .open(&mut open)
            .default_width(420.0)
            .show(ctx, |ui| {
                // Défile : à 200 % le contenu dépasse la fenêtre (US-16.8).
                let room = crate::panels::window_room(ctx) - 70.0;
                egui::ScrollArea::vertical()
                    .id_salt("prefs_scroll")
                    .max_height(room)
                    .auto_shrink([true, true])
                    .show(ui, |ui| {
                        crate::a11y::label(
                            ui,
                            RichText::new(tr("prefs.note")).small().color(muted),
                        );
                        ui.separator();
                        egui::Grid::new("prefs-values")
                            .num_columns(3)
                            .show(ui, |ui| {
                                crate::a11y::label(ui, tr("prefs.scale"));
                                crate::a11y::add_drag(
                                    ui,
                                    egui::DragValue::new(&mut prefs.ui_scale_pct)
                                        .range(UI_SCALE_MIN_PCT..=UI_SCALE_MAX_PCT)
                                        .speed(1.0)
                                        .suffix(" %"),
                                );
                                crate::a11y::label(ui, "");
                                ui.end_row();
                                crate::a11y::label(ui, tr("prefs.pick"));
                                crate::a11y::add_drag(
                                    ui,
                                    egui::DragValue::new(&mut prefs.pick_radius_px)
                                        .range(PICK_RADIUS_MIN_PX..=PICK_RADIUS_MAX_PX)
                                        .speed(0.2)
                                        .max_decimals(1),
                                );
                                crate::a11y::label(ui, "px");
                                ui.end_row();
                                crate::a11y::label(ui, tr("prefs.margin"));
                                crate::a11y::add_drag(
                                    ui,
                                    crate::views::length_drag(
                                        egui::DragValue::new(&mut prefs.default_margin_mm)
                                            .range(0.0..=MAX_SEAM_MM)
                                            .speed(0.5)
                                            .max_decimals(2),
                                    ),
                                );
                                crate::a11y::label(
                                    ui,
                                    pf_core::units::display_units().unit.suffix(),
                                );
                                ui.end_row();
                                crate::a11y::label(ui, tr("prefs.notch"));
                                crate::a11y::add_drag(
                                    ui,
                                    crate::views::length_drag(
                                        egui::DragValue::new(&mut prefs.default_notch_mm)
                                            .range(NOTCH_MIN_MM..=MAX_NOTCH_LENGTH_MM)
                                            .speed(0.1)
                                            .max_decimals(2),
                                    ),
                                );
                                crate::a11y::label(
                                    ui,
                                    pf_core::units::display_units().unit.suffix(),
                                );
                                ui.end_row();
                            });
                        ui.separator();
                        crate::a11y::label(ui, RichText::new(tr("prefs.roles")).strong());
                        crate::a11y::label(
                            ui,
                            RichText::new(tr("prefs.roles.note")).small().color(muted),
                        );
                        egui::Grid::new("prefs-roles")
                            .num_columns(4)
                            .show(ui, |ui| {
                                for role in Role::ALL {
                                    crate::a11y::label(ui, role.label());
                                    let mut rgb = prefs.color(dark, role);
                                    if ui.color_edit_button_srgb(&mut rgb).changed()
                                        && prefs.set_color(dark, role, rgb).is_err()
                                    {
                                        refused = Some((role, contrast_on_canvas(dark, rgb)));
                                    }
                                    let mut pattern = prefs.pattern(role);
                                    egui::ComboBox::from_id_salt(("prefs-pattern", role.key()))
                                        .selected_text(pattern.label())
                                        .show_ui_named(ui, |ui| {
                                            for p in Pattern::ALL {
                                                ui.selectable_value(&mut pattern, p, p.label());
                                            }
                                        });
                                    if pattern != prefs.pattern(role) {
                                        prefs.set_pattern(role, pattern);
                                    }
                                    pattern_sample(
                                        ui,
                                        color_of(prefs.color(dark, role)),
                                        prefs.pattern(role),
                                    );
                                    ui.end_row();
                                }
                            });
                        crate::a11y::label(
                            ui,
                            RichText::new(tr_args(
                                "prefs.contrast.rule",
                                &[("min", &format!("{MIN_CONTRAST}"))],
                            ))
                            .small()
                            .color(muted),
                        );
                        ui.separator();
                        let shortcuts = egui::CollapsingHeader::new(tr("shortcuts.title"))
                            .id_salt("prefs-shortcuts")
                            .default_open(self.shortcuts_open)
                            .show(ui, |ui| self.shortcuts_section(ui));
                        if self.shortcuts_open && self.frame < 24 {
                            // Étape de capture : la section est amenée dans la vue.
                            shortcuts
                                .header_response
                                .scroll_to_me(Some(egui::Align::TOP));
                        }
                        ui.separator();
                        ui.horizontal(|ui| {
                            let reset_button = ui.button(tr("prefs.reset"));
                            if std::mem::take(&mut self.focus_demo) {
                                reset_button.request_focus();
                                reset_button.scroll_to_me(Some(egui::Align::Center));
                            }
                            if reset_button.clicked() {
                                prefs = Prefs::default();
                            }
                            export = ui.button(tr("prefs.export")).clicked();
                            import = ui.button(tr("prefs.import")).clicked();
                        });
                    });
            });
        if let Some((role, ratio)) = refused {
            self.editor.error(tr_args(
                "prefs.contrast.refused",
                &[
                    ("role", &role.label()),
                    ("ratio", &pf_core::input::format_number(ratio, 1)),
                    ("min", &format!("{MIN_CONTRAST}")),
                ],
            ));
        }
        if prefs != self.prefs {
            self.prefs = prefs;
            self.save_prefs();
        }
        if export {
            if let Some(path) = rfd::FileDialog::new()
                .add_filter(tr("prefs.filter"), &["json"])
                .set_file_name("patternforge-preferences.json")
                .save_file()
            {
                self.export_prefs_to(&path);
            }
        }
        if import {
            if let Some(path) = rfd::FileDialog::new()
                .add_filter(tr("prefs.filter"), &["json"])
                .pick_file()
            {
                self.import_prefs_from(&path);
            }
        }
        self.show_prefs = open;
    }
}

/// Échantillon du trait (couleur **et** motif) à côté de son nom.
fn pattern_sample(ui: &mut egui::Ui, color: Color32, pattern: Pattern) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(90.0, 14.0), egui::Sense::hover());
    let (width, dash, gap) = pattern.style();
    let pts = vec![
        egui::pos2(rect.left(), rect.center().y),
        egui::pos2(rect.right(), rect.center().y),
    ];
    crate::canvas::polyline(ui.painter(), pts, color, width, dash, gap);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Launch;
    use egui_kittest::Harness;

    fn harness() -> Harness<'static, App> {
        let app = App::with_context(&egui::Context::default(), Launch::default());
        let mut h = Harness::builder()
            .with_size(egui::vec2(1280.0, 800.0))
            .build_state(|ctx, app: &mut App| app.ui(ctx), app);
        h.run();
        h
    }

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("pf_prefs_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn custom() -> Prefs {
        let mut p = Prefs {
            pick_radius_px: 12.0,
            default_margin_mm: 15.0,
            default_notch_mm: 7.5,
            ..Default::default()
        };
        p.set_color(false, Role::Cutting, [0, 0, 0]).unwrap();
        p.set_color(true, Role::Sewing, [255, 200, 0]).unwrap();
        p.set_pattern(Role::Axis, Pattern::Thick);
        p
    }

    #[test]
    fn les_valeurs_par_defaut_sont_celles_de_l_application() {
        let p = Prefs::default();
        assert_eq!(p.pick_radius_px, 8.0);
        assert_eq!(p.default_notch_mm, 5.0);
        assert_eq!(p.default_margin_mm, 10.0);
        assert_eq!(p.to_json()["colors"]["light"], json!({}));
        for role in Role::ALL {
            assert_eq!(p.pattern(role), DEFAULT_PATTERNS[role.index()]);
        }
    }

    #[test]
    fn les_preferences_font_l_aller_retour() {
        let p = custom();
        assert_eq!(Prefs::from_json(&p.to_json()), p);
        assert_eq!(Prefs::from_json_report(&p.to_json()).1, 0);
    }

    #[test]
    fn une_valeur_douteuse_redonne_sa_valeur_par_defaut() {
        let d = Prefs::default();
        for bad in [json!(null), json!("x"), json!([1]), json!(42)] {
            assert_eq!(Prefs::from_json(&bad), d, "{bad}");
        }
        let (p, ignored) = Prefs::from_json_report(&json!({
            "pick_radius_px": 999, "default_margin_mm": -3, "default_notch_mm": "x",
            "colors": { "light": { "cutting": "rouge", "sewing": "#FFFFFF" }, "dark": 4 },
            "patterns": { "construction": "thin" },
        }));
        assert_eq!(p, d);
        assert_eq!(ignored, 7);
    }

    #[test]
    fn un_motif_en_double_ecarte_tous_les_motifs() {
        let mut v = Prefs::default().to_json();
        v["patterns"]["axis"] = json!("thin");
        let (p, ignored) = Prefs::from_json_report(&v);
        assert_eq!((p.patterns, ignored), (DEFAULT_PATTERNS, 1));
    }

    #[test]
    fn une_couleur_sans_contraste_est_refusee_et_l_ancienne_gardee() {
        let mut p = Prefs::default();
        let canvas = rgb_of(canvas_color(false));
        let before = p.color(false, Role::Sewing);
        let ratio = p.set_color(false, Role::Sewing, canvas).unwrap_err();
        assert!(ratio < MIN_CONTRAST);
        assert_eq!(p.color(false, Role::Sewing), before);
        // Les couleurs d'origine des deux thèmes passent le contrôle.
        for dark in [false, true] {
            for role in Role::ALL {
                assert!(contrast_on_canvas(dark, p.color(dark, role)) >= MIN_CONTRAST);
            }
        }
    }

    #[test]
    fn changer_un_motif_echange_et_garde_six_motifs_differents() {
        let mut p = Prefs::default();
        p.set_pattern(Role::Axis, Pattern::Thick);
        assert_eq!(p.pattern(Role::Axis), Pattern::Thick);
        assert_eq!(p.pattern(Role::Cutting), Pattern::Dotted);
        for a in Role::ALL {
            for b in Role::ALL {
                assert!(a == b || p.pattern(a) != p.pattern(b));
            }
        }
    }

    #[test]
    fn le_trace_suit_les_motifs_et_les_couleurs_choisis() {
        let mut h = harness();
        let mut p = Prefs::default();
        p.set_pattern(Role::Cutting, Pattern::Dashed);
        p.set_color(false, Role::Cutting, [0, 0, 0]).unwrap();
        h.state_mut().prefs = p;
        h.run_steps(2);
        let tokens = pf_ui::theme::tokens(&h.ctx);
        assert_eq!(rgb_of(tokens.role_cutting), [0, 0, 0]);
        let (_, _, dash, _) = crate::canvas::role_style(pf_core::PathRole::CuttingContour, &tokens);
        assert_eq!(dash, Pattern::Dashed.style().1);
        assert_eq!(role_pattern(Role::Sewing), Pattern::Thick);
    }

    #[test]
    fn la_longueur_de_cran_par_defaut_vient_des_preferences() {
        let mut h = harness();
        h.state_mut().prefs.default_notch_mm = 9.0;
        h.run_steps(2);
        assert_eq!(h.state().editor.default_notch_mm, 9.0);
    }

    #[test]
    fn les_preferences_vont_dans_les_reglages_pas_dans_le_projet() {
        let file = temp_dir("settings").join("settings.json");
        let mut h = harness();
        h.state_mut().settings_path = Some(file.clone());
        let before = pf_core::persist::to_json(h.state().editor.project()).unwrap();
        h.state_mut().prefs = custom();
        h.state_mut().save_prefs();
        h.run_steps(3);
        assert_eq!(load_prefs(&file), custom());
        assert_eq!(
            pf_core::persist::to_json(h.state().editor.project()).unwrap(),
            before
        );
        assert!(!h.state().editor.doc.is_dirty());
        assert_eq!(
            load_prefs(&file.with_file_name("absent.json")),
            Prefs::default()
        );
    }

    #[test]
    fn un_echec_d_ecriture_est_signale_sans_perdre_l_etat() {
        let dir = temp_dir("blocked");
        let blocker = dir.join("blocker");
        std::fs::write(&blocker, "x").unwrap();
        let mut h = harness();
        h.state_mut().settings_path = Some(blocker.join("settings.json"));
        h.state_mut().prefs = custom();
        h.state_mut().save_prefs();
        assert!(h.state().editor.status.error);
        assert_eq!(h.state().prefs, custom());
    }

    #[test]
    fn l_export_puis_l_import_redonnent_les_memes_reglages() {
        let dir = temp_dir("export");
        let file = dir.join("prefs.json");
        let mut h = harness();
        h.state_mut().prefs = custom();
        assert!(h.state_mut().export_prefs_to(&file));
        h.state_mut().prefs = Prefs::default();
        assert!(h.state_mut().import_prefs_from(&file));
        assert_eq!(h.state().prefs, custom());
        assert!(!h.state().editor.status.error);
    }

    #[test]
    fn un_import_invalide_est_refuse_sans_rien_changer() {
        let dir = temp_dir("import");
        let mut h = harness();
        h.state_mut().prefs = custom();
        for (name, body) in [
            ("illisible.json", "pas du json".to_string()),
            ("autre.json", r#"{"language":"fr"}"#.to_string()),
            (
                "futur.json",
                format!(r#"{{"format":"{FORMAT}","version":99}}"#),
            ),
            ("absent-version.json", format!(r#"{{"format":"{FORMAT}"}}"#)),
        ] {
            let file = dir.join(name);
            std::fs::write(&file, body).unwrap();
            assert!(!h.state_mut().import_prefs_from(&file), "{name}");
            assert!(h.state().editor.status.error, "{name}");
            assert_eq!(h.state().prefs, custom(), "{name}");
        }
        assert!(!h
            .state_mut()
            .import_prefs_from(&dir.join("inexistant.json")));
    }

    #[test]
    fn un_import_partiel_ecarte_les_valeurs_douteuses_et_le_dit() {
        let file = temp_dir("partial").join("p.json");
        std::fs::write(
            &file,
            format!(
                r#"{{"format":"{FORMAT}","version":1,"pick_radius_px":500,"default_notch_mm":6}}"#
            ),
        )
        .unwrap();
        let mut h = harness();
        assert!(h.state_mut().import_prefs_from(&file));
        assert_eq!(h.state().prefs.pick_radius_px, 8.0);
        assert_eq!(h.state().prefs.default_notch_mm, 6.0);
        assert!(!h.state().editor.status.error);
    }

    #[test]
    fn la_marge_par_defaut_ne_change_aucune_marge_existante() {
        let mut h = harness();
        let before = pf_core::persist::to_json(h.state().editor.project()).unwrap();
        h.state_mut().prefs.default_margin_mm = 25.0;
        h.run_steps(3);
        assert_eq!(
            pf_core::persist::to_json(h.state().editor.project()).unwrap(),
            before
        );
        assert!(!h.state().editor.doc.is_dirty());
    }
}
