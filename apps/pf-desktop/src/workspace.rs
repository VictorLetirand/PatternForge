//! Personnalisation de l'interface (US-16.4) : barres d'outils configurables, espaces de travail
//! (construction, pièces, gradation, impression) et côté d'ancrage des palettes.
//!
//! Ces réglages sont des **préférences de l'application** (clé `layout` de `settings.json`),
//! jamais une donnée du projet : aucune `Command`, rien dans le fichier `.pforge.json`. Ils ne
//! changent que l'affichage de la barre d'outils : les raccourcis clavier des outils masqués
//! restent actifs, et un outil actif n'est jamais désactivé par le masquage.

use crate::app::App;
use crate::dock::ToolWin;
use crate::editor::Tool;
use crate::panels::ToolFamily;
use crate::shortcuts::tool_id;
use eframe::egui;
use pf_i18n::{settings, tr};
use serde_json::{json, Value};
use std::collections::BTreeSet;

/// Clé du réglage dans `settings.json`.
pub const SETTINGS_KEY: &str = "layout";

/// Espace de travail : jeu de familles d'outils visibles et de palettes ouvertes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Workspace {
    Construction,
    Pieces,
    Grading,
    Printing,
}

impl Workspace {
    pub const ALL: [Workspace; 4] = [
        Workspace::Construction,
        Workspace::Pieces,
        Workspace::Grading,
        Workspace::Printing,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Workspace::Construction => "construction",
            Workspace::Pieces => "pieces",
            Workspace::Grading => "grading",
            Workspace::Printing => "printing",
        }
    }

    pub fn from_key(key: &str) -> Option<Workspace> {
        Workspace::ALL.into_iter().find(|w| w.key() == key)
    }

    pub fn label(self) -> String {
        match self {
            Workspace::Construction => tr("workspace.construction"),
            Workspace::Pieces => tr("workspace.pieces"),
            Workspace::Grading => tr("workspace.grading"),
            Workspace::Printing => tr("workspace.printing"),
        }
    }

    /// Familles d'outils affichées dans la barre d'outils.
    pub fn families(self) -> &'static [ToolFamily] {
        match self {
            Workspace::Construction => &[
                ToolFamily::Draw,
                ToolFamily::Construct,
                ToolFamily::Modify,
                ToolFamily::Measure,
            ],
            Workspace::Pieces => &[ToolFamily::Modify, ToolFamily::Piece, ToolFamily::Measure],
            Workspace::Grading => &[ToolFamily::Modify, ToolFamily::Measure],
            Workspace::Printing => &[ToolFamily::Piece, ToolFamily::Measure],
        }
    }

    /// Palettes ouvertes en entrant dans l'espace.
    pub fn windows(self) -> &'static [ToolWin] {
        match self {
            Workspace::Construction => &[ToolWin::Variables],
            Workspace::Pieces => &[ToolWin::Seams],
            Workspace::Grading => &[ToolWin::Sizes, ToolWin::Grading],
            Workspace::Printing => &[ToolWin::ExportSizes],
        }
    }
}

/// Côté du dock des palettes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum DockSide {
    #[default]
    Right,
    Left,
}

/// Réglages d'interface de l'utilisateur.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Layout {
    /// `None` : toutes les familles.
    pub workspace: Option<Workspace>,
    /// Identifiants (`shortcuts::tool_id`) des outils masqués de la barre d'outils.
    hidden: BTreeSet<String>,
    pub dock_side: DockSide,
}

impl Layout {
    /// Un outil est affiché sauf s'il a été masqué ; Sélection l'est toujours.
    pub fn tool_visible(&self, tool: Tool) -> bool {
        tool == Tool::Select || !self.hidden.contains(tool_id(tool))
    }

    pub fn set_tool_visible(&mut self, tool: Tool, visible: bool) {
        if tool == Tool::Select {
            return;
        }
        if visible {
            self.hidden.remove(tool_id(tool));
        } else {
            self.hidden.insert(tool_id(tool).to_string());
        }
    }

    /// Outils d'une famille à afficher (vide si la famille est écartée par l'espace de travail).
    pub fn visible_tools(&self, family: ToolFamily) -> Vec<Tool> {
        if self
            .workspace
            .is_some_and(|w| !w.families().contains(&family))
        {
            return Vec::new();
        }
        family
            .tools()
            .iter()
            .copied()
            .filter(|t| self.tool_visible(*t))
            .collect()
    }

    pub fn reset_toolbars(&mut self) {
        self.hidden.clear();
    }

    pub fn to_json(&self) -> Value {
        let mut value = json!({
            "hidden_tools": self.hidden.iter().collect::<Vec<_>>(),
            "dock_side": match self.dock_side {
                DockSide::Right => "right",
                DockSide::Left => "left",
            },
        });
        if let Some(w) = self.workspace {
            value["workspace"] = json!(w.key());
        }
        value
    }

    /// Lecture tolérante : champ absent, mal formé ou inconnu = valeur par défaut (jamais de
    /// refus d'ouvrir l'application pour un réglage d'interface).
    pub fn from_json(value: &Value) -> Layout {
        let mut layout = Layout::default();
        let Some(obj) = value.as_object() else {
            return layout;
        };
        layout.workspace = obj
            .get("workspace")
            .and_then(Value::as_str)
            .and_then(Workspace::from_key);
        layout.dock_side = match obj.get("dock_side").and_then(Value::as_str) {
            Some("left") => DockSide::Left,
            _ => DockSide::Right,
        };
        if let Some(list) = obj.get("hidden_tools").and_then(Value::as_array) {
            for tool in list.iter().filter_map(Value::as_str) {
                if let Some(t) = Tool::ALL.into_iter().find(|t| tool_id(*t) == tool) {
                    layout.set_tool_visible(t, false);
                }
            }
        }
        layout
    }
}

/// Réglages enregistrés (absents ou illisibles : valeurs par défaut). Ne modifie jamais le fichier.
pub fn load_layout(path: &std::path::Path) -> Layout {
    settings::load_value(path, SETTINGS_KEY)
        .map(|v| Layout::from_json(&v))
        .unwrap_or_default()
}

impl App {
    /// Enregistre les réglages d'interface ; un échec est signalé, l'état en mémoire reste.
    fn save_layout(&mut self) {
        let Some(path) = self.settings_path.clone() else {
            return;
        };
        if let Err(e) = settings::save_value(&path, SETTINGS_KEY, self.layout.to_json()) {
            self.editor.error(pf_i18n::tr_args(
                "customize.save_error",
                &[("error", &e.to_string())],
            ));
        }
    }

    /// Change d'espace de travail : familles affichées et palettes. Les palettes des autres
    /// espaces sont fermées, jamais celles ouvertes à la main hors d'un espace.
    pub fn set_workspace(&mut self, workspace: Option<Workspace>) {
        for other in Workspace::ALL {
            for win in other.windows() {
                self.close_tool_window(*win);
            }
        }
        self.layout.workspace = workspace;
        match workspace {
            Some(Workspace::Construction) => self.open_variables(),
            Some(Workspace::Pieces) => self.open_seams(),
            Some(Workspace::Grading) => {
                self.open_sizes();
                self.open_grading();
            }
            Some(Workspace::Printing) => self.open_export_sizes(),
            None => {}
        }
        self.save_layout();
    }

    /// Fenêtre « Personnaliser » : outils visibles par famille et côté du dock.
    pub fn customize_window(&mut self, ctx: &egui::Context) {
        if !self.show_customize {
            return;
        }
        let mut open = true;
        let mut layout = self.layout.clone();
        egui::Window::new(tr("customize.title"))
            .open(&mut open)
            .default_width(360.0)
            .show(ctx, |ui| {
                crate::a11y::label(ui, tr("customize.note"));
                ui.separator();
                for family in ToolFamily::ALL {
                    crate::a11y::label(ui, egui::RichText::new(family.label()).strong());
                    for tool in family.tools() {
                        let mut visible = layout.tool_visible(*tool);
                        if ui.checkbox(&mut visible, tool.label()).changed() {
                            layout.set_tool_visible(*tool, visible);
                        }
                    }
                }
                ui.separator();
                crate::a11y::label(ui, egui::RichText::new(tr("customize.dock")).strong());
                ui.horizontal(|ui| {
                    ui.selectable_value(
                        &mut layout.dock_side,
                        DockSide::Left,
                        tr("customize.dock.left"),
                    );
                    ui.selectable_value(
                        &mut layout.dock_side,
                        DockSide::Right,
                        tr("customize.dock.right"),
                    );
                });
                ui.separator();
                if ui.button(tr("customize.reset")).clicked() {
                    layout.reset_toolbars();
                }
            });
        if layout != self.layout {
            self.layout = layout;
            self.save_layout();
        }
        self.show_customize = open;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Launch;
    use egui_kittest::kittest::Queryable;
    use egui_kittest::Harness;

    fn harness() -> Harness<'static, App> {
        let app = App::with_context(&egui::Context::default(), Launch::default());
        let mut h = Harness::builder()
            .with_size(egui::vec2(1280.0, 800.0))
            .build_state(|ctx, app: &mut App| app.ui(ctx), app);
        h.run();
        h
    }

    fn temp_file(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("pf_layout_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("settings.json")
    }

    #[test]
    fn les_reglages_font_l_aller_retour() {
        let mut layout = Layout {
            workspace: Some(Workspace::Grading),
            dock_side: DockSide::Left,
            ..Default::default()
        };
        layout.set_tool_visible(Tool::Dart, false);
        layout.set_tool_visible(Tool::Select, false);
        assert!(layout.tool_visible(Tool::Select));
        assert_eq!(layout.hidden.len(), 1);
        assert_eq!(Layout::from_json(&layout.to_json()), layout);
    }

    #[test]
    fn un_reglage_douteux_redonne_les_valeurs_par_defaut() {
        for bad in [
            json!(null),
            json!("x"),
            json!({ "workspace": 3, "dock_side": [], "hidden_tools": "dart" }),
            json!({ "workspace": "inconnu", "dock_side": "haut", "hidden_tools": [1, "nope", "select"] }),
        ] {
            assert_eq!(Layout::from_json(&bad), Layout::default(), "{bad}");
        }
    }

    #[test]
    fn un_espace_ecarte_les_familles_et_le_masquage_les_outils() {
        let mut layout = Layout::default();
        assert_eq!(layout.visible_tools(ToolFamily::Draw).len(), 8);
        layout.set_tool_visible(Tool::Spline, false);
        assert_eq!(layout.visible_tools(ToolFamily::Draw).len(), 7);
        layout.workspace = Some(Workspace::Printing);
        assert!(layout.visible_tools(ToolFamily::Draw).is_empty());
        assert!(!layout.visible_tools(ToolFamily::Piece).is_empty());
        // Chaque espace garde au moins une famille et ne montre que des familles connues.
        for w in Workspace::ALL {
            assert!(!w.families().is_empty() && !w.windows().is_empty());
        }
    }

    #[test]
    fn les_reglages_vont_dans_les_preferences_pas_dans_le_projet() {
        let file = temp_file("prefs");
        let mut h = harness();
        h.state_mut().settings_path = Some(file.clone());
        let before = pf_core::persist::to_json(h.state().editor.project()).unwrap();
        h.state_mut().set_workspace(Some(Workspace::Grading));
        h.state_mut().layout.set_tool_visible(Tool::Dart, false);
        h.state_mut().layout.dock_side = DockSide::Left;
        h.state_mut().save_layout();
        h.run_steps(3);
        let saved = load_layout(&file);
        assert_eq!(saved, h.state().layout);
        assert_eq!(saved.workspace, Some(Workspace::Grading));
        assert!(h.state().sizes.is_some() && h.state().grading.is_some());
        assert_eq!(
            pf_core::persist::to_json(h.state().editor.project()).unwrap(),
            before
        );
        assert!(!h.state().editor.doc.is_dirty());
        assert_eq!(
            load_layout(&file.with_file_name("absent.json")),
            Layout::default()
        );
    }

    #[test]
    fn changer_d_espace_ferme_les_palettes_de_l_ancien() {
        let mut h = harness();
        h.state_mut().set_workspace(Some(Workspace::Grading));
        assert!(h.state().sizes.is_some());
        h.state_mut().set_workspace(Some(Workspace::Construction));
        h.run_steps(3);
        assert!(h.state().sizes.is_none() && h.state().grading.is_none());
        assert!(h.state().variables.is_some());
        h.state_mut().set_workspace(None);
        assert!(h.state().variables.is_none());
        assert_eq!(h.state().layout.workspace, None);
    }

    #[test]
    fn la_barre_d_outils_suit_l_espace_et_le_masquage() {
        let mut h = harness();
        assert!(h.query_by_label("Dessin").is_some());
        h.state_mut().set_workspace(Some(Workspace::Printing));
        h.run_steps(3);
        assert!(h.query_by_label("Dessin").is_none());
        assert!(h.query_by_label("Pièce").is_some());
        h.state_mut().layout.set_tool_visible(Tool::Notch, false);
        h.state_mut()
            .layout
            .set_tool_visible(Tool::Grainline, false);
        h.state_mut()
            .layout
            .set_tool_visible(Tool::ExtractPiece, false);
        h.state_mut().layout.set_tool_visible(Tool::Dart, false);
        h.run_steps(3);
        assert!(h.query_by_label("Pièce").is_none());
    }

    #[test]
    fn la_palette_s_ancre_a_gauche_ou_a_droite() {
        let mut h = harness();
        h.state_mut().open_variables();
        h.run_steps(4);
        assert!(h.state().dock.rect.center().x > 640.0);
        h.state_mut().layout.dock_side = DockSide::Left;
        h.run_steps(4);
        assert!(h.state().dock.rect.center().x < 640.0);
    }

    #[test]
    fn un_echec_d_ecriture_est_signale_sans_perdre_l_etat() {
        let dir = temp_file("blocked");
        let blocker = dir.with_file_name("blocker");
        std::fs::write(&blocker, "x").unwrap();
        let mut h = harness();
        h.state_mut().settings_path = Some(blocker.join("settings.json"));
        h.state_mut().layout.dock_side = DockSide::Left;
        h.state_mut().save_layout();
        assert!(h.state().editor.status.error);
        assert_eq!(h.state().layout.dock_side, DockSide::Left);
    }
}
