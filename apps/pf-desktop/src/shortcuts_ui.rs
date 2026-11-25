//! US-16.3 : personnalisation de tous les raccourcis dans les préférences (section repliable),
//! détection de conflits, réinitialisation, jeux prédéfinis, export / import d'un fichier.
//!
//! Réglages de l'application (clé `shortcuts` de `settings.json`), jamais une donnée du projet.

use crate::app::App;
use crate::shortcuts::{
    self, AssignError, BindError, Binding, Cmd, FileError, Keymap, Preset, Target,
};
use eframe::egui::{self, Key, RichText};
use pf_i18n::{tr, tr_args};
use std::path::Path;

fn target_label(target: Target) -> String {
    match target {
        Target::Tool(tool) => tool.label(),
        Target::Cmd(cmd) => cmd.label(),
    }
}

fn file_error_text(error: &FileError) -> String {
    let (key, detail, other) = match error {
        FileError::NotJson(d) => ("shortcuts.file.not_json", d.clone(), String::new()),
        FileError::NotAnObject => ("shortcuts.file.not_an_object", String::new(), String::new()),
        FileError::WrongFormat => ("shortcuts.file.wrong_format", String::new(), String::new()),
        FileError::BadVersion => ("shortcuts.file.bad_version", String::new(), String::new()),
        FileError::UnknownField(d) => ("shortcuts.file.unknown_field", d.clone(), String::new()),
        FileError::UnknownTool(d) => ("shortcuts.file.unknown_tool", d.clone(), String::new()),
        FileError::UnknownCommand(d) => {
            ("shortcuts.file.unknown_command", d.clone(), String::new())
        }
        FileError::BadValue(d) => ("shortcuts.file.bad_value", d.clone(), String::new()),
        FileError::Conflict { command, other } => {
            ("shortcuts.file.conflict", command.clone(), other.clone())
        }
    };
    tr_args(key, &[("detail", &detail), ("other", &other)])
}

impl App {
    fn bind_error_text(&self, error: BindError, binding: Option<Binding>) -> String {
        let combo = binding.map(Binding::display).unwrap_or_default();
        match error {
            BindError::Conflict(other) => tr_args(
                "shortcuts.err.conflict",
                &[("combo", &combo), ("command", &other.label())],
            ),
            BindError::NeedsCtrl => tr("shortcuts.err.needs_ctrl"),
            BindError::Fixed(cmd) => tr_args("shortcuts.err.fixed", &[("command", &cmd.label())]),
            BindError::Reserved | BindError::Unreadable => tr("shortcuts.err.reserved"),
        }
    }

    /// Affecte la combinaison pressée à la cible ; refus clair, rien n'est changé en cas d'échec.
    pub fn assign_target(&mut self, target: Target, binding: Binding) -> bool {
        let mut km = shortcuts::current_keymap();
        let result = match target {
            Target::Cmd(cmd) => km
                .cmds
                .assign(cmd, Some(binding))
                .map_err(|e| self.bind_error_text(e, Some(binding))),
            Target::Tool(tool) => {
                if binding.ctrl || binding.shift {
                    Err(tr("help.shortcut.not_letter"))
                } else {
                    km.tools.assign(tool, binding.key).map_err(|e| match e {
                        AssignError::NotALetter => tr("help.shortcut.not_letter"),
                        AssignError::Taken(other) => tr_args(
                            "help.shortcut.taken",
                            &[("letter", &km.tools.label(other)), ("tool", &other.label())],
                        ),
                    })
                }
            }
        };
        match result {
            Ok(()) => {
                self.key_error = None;
                self.key_capture = None;
                self.apply_keymap(km);
                true
            }
            Err(text) => {
                self.key_error = Some(text);
                false
            }
        }
    }

    /// Retire le raccourci de la cible (une commande fixe refuse).
    pub fn clear_target(&mut self, target: Target) {
        let mut km = shortcuts::current_keymap();
        match target {
            Target::Tool(tool) => km.tools.unbind(tool),
            Target::Cmd(cmd) => {
                if let Err(e) = km.cmds.assign(cmd, None) {
                    self.key_error = Some(self.bind_error_text(e, None));
                    return;
                }
            }
        }
        self.key_error = None;
        self.apply_keymap(km);
    }

    /// Adopte une table complète et la mémorise hors projet.
    pub fn apply_keymap(&mut self, km: Keymap) {
        shortcuts::set_current_keymap(km);
        self.save_shortcuts();
    }

    pub fn apply_preset(&mut self, preset: Preset) {
        self.key_error = None;
        self.key_capture = None;
        self.apply_keymap(preset.keymap());
        self.editor.info(tr("shortcuts.preset_applied"));
    }

    /// Écrit le fichier d'export (toutes les entrées).
    pub fn export_keymap_to(&mut self, path: &Path) -> bool {
        match std::fs::write(path, shortcuts::current_keymap().export_text()) {
            Ok(()) => {
                self.editor.info(tr("shortcuts.exported"));
                true
            }
            Err(e) => {
                self.editor.error(tr_args(
                    "shortcuts.export_error",
                    &[("error", &e.to_string())],
                ));
                false
            }
        }
    }

    /// Relit un fichier de raccourcis : invalide = refus entier, rien n'est changé.
    pub fn import_keymap_from(&mut self, path: &Path) -> bool {
        let result = std::fs::read_to_string(path)
            .map_err(|e| FileError::NotJson(e.to_string()))
            .and_then(|text| Keymap::import_text(&text));
        match result {
            Ok(km) => {
                self.key_error = None;
                self.apply_keymap(km);
                self.editor.info(tr("shortcuts.imported"));
                true
            }
            Err(e) => {
                self.editor.error(tr_args(
                    "shortcuts.import_error",
                    &[("reason", &file_error_text(&e))],
                ));
                false
            }
        }
    }

    /// Capture de la combinaison pour `key_capture` : renvoie `true` si une capture est en cours
    /// (la touche n'agit alors nulle part ailleurs).
    pub(crate) fn capture_key(&mut self, ctx: &egui::Context) -> bool {
        let Some(target) = self.key_capture else {
            return false;
        };
        let pressed = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Key {
                    key,
                    pressed: true,
                    repeat: false,
                    modifiers,
                    ..
                } => Some((*key, *modifiers)),
                _ => None,
            })
        });
        if let Some((key, modifiers)) = pressed {
            if key == Key::Escape {
                self.key_capture = None;
            } else {
                let binding = Binding {
                    ctrl: modifiers.command,
                    shift: modifiers.shift,
                    key,
                };
                self.assign_target(target, binding);
            }
            ctx.input_mut(|i| i.events.clear());
        }
        true
    }

    /// Section « Raccourcis clavier » de la fenêtre Préférences.
    pub(crate) fn shortcuts_section(&mut self, ui: &mut egui::Ui) {
        let muted = crate::palette::pal().muted;
        crate::a11y::label(ui, RichText::new(tr("shortcuts.note")).small().color(muted));
        let km = shortcuts::current_keymap();
        let mut capture = None;
        let mut clear = None;
        egui::ScrollArea::vertical()
            .id_salt("shortcuts_scroll")
            .max_height(220.0)
            .show(ui, |ui| {
                egui::Grid::new("shortcuts_grid")
                    .num_columns(3)
                    .striped(true)
                    .show(ui, |ui| {
                        let mut targets: Vec<Target> =
                            Cmd::ALL.iter().map(|c| Target::Cmd(*c)).collect();
                        targets.extend(crate::editor::Tool::ALL.iter().map(|t| Target::Tool(*t)));
                        for target in targets {
                            let label = target_label(target);
                            let combo = match target {
                                Target::Tool(t) => km.tools.label(t),
                                Target::Cmd(c) => {
                                    km.cmds.get(c).map_or(String::new(), Binding::display)
                                }
                            };
                            crate::a11y::label(ui, &label);
                            ui.strong(&combo);
                            let fixed = matches!(target, Target::Cmd(c) if c.fixed());
                            if fixed {
                                crate::a11y::label(
                                    ui,
                                    RichText::new(tr("shortcuts.fixed")).small().color(muted),
                                );
                            } else if self.key_capture == Some(target) {
                                crate::a11y::label(ui, tr("shortcuts.press"));
                            } else {
                                ui.horizontal(|ui| {
                                    let change = ui.button(tr("shortcuts.change"));
                                    let name = format!("{} : {label}", tr("shortcuts.change"));
                                    change.widget_info(|| {
                                        egui::WidgetInfo::labeled(
                                            egui::WidgetType::Button,
                                            true,
                                            &name,
                                        )
                                    });
                                    if change.clicked() {
                                        capture = Some(target);
                                    }
                                    let remove = ui.button(tr("shortcuts.clear"));
                                    let name = format!("{} : {label}", tr("shortcuts.clear"));
                                    remove.widget_info(|| {
                                        egui::WidgetInfo::labeled(
                                            egui::WidgetType::Button,
                                            true,
                                            &name,
                                        )
                                    });
                                    if remove.clicked() {
                                        clear = Some(target);
                                    }
                                });
                            }
                            ui.end_row();
                        }
                    });
            });
        if let Some(err) = &self.key_error {
            ui.colored_label(crate::palette::pal().error, err);
        }
        let (mut preset, mut reset, mut export, mut import) = (None, false, false, false);
        ui.horizontal_wrapped(|ui| {
            crate::a11y::label(ui, tr("shortcuts.preset"));
            for p in Preset::ALL {
                if ui.button(p.label()).clicked() {
                    preset = Some(p);
                }
            }
            reset = ui.button(tr("shortcuts.reset")).clicked();
            export = ui.button(tr("shortcuts.export")).clicked();
            import = ui.button(tr("shortcuts.import")).clicked();
        });
        if let Some(target) = capture {
            self.key_capture = Some(target);
            self.key_error = None;
        }
        if let Some(target) = clear {
            self.clear_target(target);
        }
        if let Some(p) = preset {
            self.apply_preset(p);
        }
        if reset {
            self.apply_preset(Preset::PatternForge);
        }
        if export {
            if let Some(path) = rfd::FileDialog::new()
                .add_filter(tr("shortcuts.filter"), &["json"])
                .set_file_name("patternforge-raccourcis.json")
                .save_file()
            {
                self.export_keymap_to(&path);
            }
        }
        if import {
            if let Some(path) = rfd::FileDialog::new()
                .add_filter(tr("shortcuts.filter"), &["json"])
                .pick_file()
            {
                self.import_keymap_from(&path);
            }
        }
    }
}
