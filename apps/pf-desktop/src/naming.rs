//! US-11.5 — fenêtre « Conventions de nommage… » du dock (menu Fichier, sans raccourci, aucun outil).
//!
//! Une règle (préfixe, séparateur, suffixe, compteur) vise une cible : pièces, codes de pièces,
//! feuilles ou variantes. Rien n'est appliqué par défaut : la fenêtre n'a que deux effets sur des
//! noms existants, le renommage en lot (aperçu, puis un seul pas d'annulation) ; la création
//! d'une pièce ou d'une feuille n'utilise la règle que si la case « Utiliser la règle de
//! nommage » est cochée (voir `Editor::rule_name`). Le prochain nom est calculé par le cœur
//! (`Project::next_name`) : aucun état de compteur n'est stocké.

use crate::a11y::ComboExt;
use crate::app::App;
use crate::dock::ToolWin;
use crate::grading::section_header;
use eframe::egui::{self, RichText};
use pf_core::{
    Command, NamingCounter, NamingRule, NamingRuleId, NamingRuleSpec, NamingTarget, Project,
};
use pf_i18n::{tr, tr_args};
use pf_ui::components::{button, ButtonKind};

const W_FIELD: f32 = 130.0;
const W_NUM: f32 = 60.0;
const PREVIEW_ROWS: usize = 12;

pub(crate) const TARGETS: [NamingTarget; 4] = [
    NamingTarget::Piece,
    NamingTarget::PieceCode,
    NamingTarget::Sheet,
    NamingTarget::Variant,
];

pub(crate) fn target_label(t: NamingTarget) -> String {
    tr(match t {
        NamingTarget::Piece => "naming.target.piece",
        NamingTarget::PieceCode => "naming.target.piece_code",
        NamingTarget::Sheet => "naming.target.sheet",
        NamingTarget::Variant => "naming.target.variant",
    })
}

/// État de la fenêtre (interface seulement, jamais enregistré).
#[derive(Debug, Clone)]
pub struct NamingDialog {
    pub selected: Option<NamingRuleId>,
    pub list_open: bool,
    pub form_open: bool,
    pub batch_open: bool,
    pub preview: bool,
    /// Règle telle qu'elle a été chargée dans le formulaire (détecte un changement externe).
    loaded: Option<Option<NamingRule>>,
    name: String,
    target: NamingTarget,
    prefix: String,
    separator: String,
    suffix: String,
    use_counter: bool,
    start: String,
    step: String,
    width: String,
    error: Option<String>,
}

impl Default for NamingDialog {
    fn default() -> Self {
        NamingDialog {
            selected: None,
            list_open: true,
            form_open: true,
            batch_open: true,
            preview: false,
            loaded: None,
            name: String::new(),
            target: NamingTarget::Piece,
            prefix: String::new(),
            separator: "-".into(),
            suffix: String::new(),
            use_counter: true,
            start: "1".into(),
            step: "1".into(),
            width: "2".into(),
            error: None,
        }
    }
}

impl NamingDialog {
    fn load(&mut self, rule: Option<&NamingRule>) {
        let blank = NamingDialog::default();
        match rule {
            Some(r) => {
                self.name = r.name.clone();
                self.target = r.target;
                self.prefix = r.prefix.clone();
                self.separator = r.separator.clone();
                self.suffix = r.suffix.clone();
                self.use_counter = r.counter.is_some();
                let c = r.counter.unwrap_or(NamingCounter {
                    start: 1,
                    step: 1,
                    width: 2,
                });
                self.start = c.start.to_string();
                self.step = c.step.to_string();
                self.width = c.width.to_string();
            }
            None => {
                self.name = String::new();
                self.target = blank.target;
                self.prefix = blank.prefix;
                self.separator = blank.separator;
                self.suffix = blank.suffix;
                self.use_counter = blank.use_counter;
                self.start = blank.start;
                self.step = blank.step;
                self.width = blank.width;
            }
        }
        self.loaded = Some(rule.cloned());
    }

    fn spec(&self) -> Result<NamingRuleSpec, String> {
        let num = |text: &str| {
            text.trim()
                .parse::<u32>()
                .map_err(|_| tr_args("naming.err.number", &[("text", text)]))
        };
        let counter = if self.use_counter {
            Some(NamingCounter {
                start: num(&self.start)?,
                step: num(&self.step)?,
                width: u8::try_from(num(&self.width)?)
                    .map_err(|_| tr_args("naming.err.number", &[("text", &self.width)]))?,
            })
        } else {
            None
        };
        Ok(NamingRuleSpec {
            name: self.name.clone(),
            target: self.target,
            prefix: self.prefix.clone(),
            separator: self.separator.clone(),
            suffix: self.suffix.clone(),
            counter,
        })
    }
}

enum NAction {
    New,
    Add,
    Update(NamingRuleId),
    Remove(NamingRuleId),
    Apply(NamingRuleId),
}

impl App {
    /// Menu Fichier › « Conventions de nommage… » (sans raccourci).
    pub fn open_naming(&mut self) {
        if self.naming.is_none() {
            self.naming = Some(NamingDialog::default());
        }
    }

    fn naming_act(&mut self, d: &mut NamingDialog, action: NAction) {
        let result: Result<(), String> = match action {
            NAction::New => {
                d.selected = None;
                d.loaded = None;
                Ok(())
            }
            NAction::Add => match d.spec() {
                Ok(spec) => self
                    .measure_exec(Command::AddNamingRule { spec })
                    .map(|out| d.selected = out.naming_rules.last().copied()),
                Err(e) => Err(e),
            },
            NAction::Update(rule) => match d.spec() {
                Ok(spec) => self
                    .measure_exec(Command::UpdateNamingRule { rule, spec })
                    .map(|_| ()),
                Err(e) => Err(e),
            },
            NAction::Remove(rule) => {
                let r = self.measure_exec(Command::RemoveNamingRule { rule });
                if r.is_ok() {
                    d.selected = None;
                    d.preview = false;
                }
                r.map(|_| ())
            }
            NAction::Apply(rule) => self
                .measure_exec(Command::ApplyNamingRule { rule })
                .map(|_| d.preview = false),
        };
        d.error = result.err();
    }

    pub fn naming_window(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.naming.take() else {
            return;
        };
        let project = self.editor.project().clone();
        let tokens = pf_ui::theme::tokens(ctx);
        let (danger, secondary) = (tokens.danger, tokens.text_secondary);
        // Une règle disparue (annulation) est oubliée ; le formulaire suit la règle choisie.
        d.selected = d.selected.filter(|id| project.naming_rule(*id).is_ok());
        let current = d.selected.and_then(|id| project.naming_rule(id).ok());
        if d.loaded.as_ref() != Some(&current.cloned()) {
            d.load(current);
        }
        let mut open = true;
        let mut close = false;
        let mut actions: Vec<NAction> = Vec::new();
        let slot = crate::dock::slot(ctx, &self.dock, ToolWin::Naming);
        let width = self.dock.content_width(ToolWin::Naming);
        egui::Window::new(tr("naming.title"))
            .id(ToolWin::Naming.id())
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .fixed_pos(slot.pos)
            .default_height(slot.max_height)
            .max_height(slot.max_height)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .max_height(slot.max_height)
                    .show(ui, |ui| {
                        ui.set_width(width);
                        crate::a11y::label(ui, RichText::new(tr("naming.intro")).color(secondary));
                        if let Some(text) = &d.error {
                            crate::a11y::label(ui, RichText::new(text).color(danger));
                        }
                        ui.add_space(4.0);
                        list_section(ui, &mut d, &project, secondary, &mut actions);
                        ui.add_space(4.0);
                        form_section(ui, &mut d, &project, secondary, &mut actions);
                        if let Some(r) = d.selected.and_then(|id| project.naming_rule(id).ok()) {
                            ui.add_space(4.0);
                            batch_section(ui, &mut d, &project, r, secondary, &mut actions);
                        }
                        ui.add_space(6.0);
                        close = button(ui, &tr("dialog.close"), ButtonKind::Secondary).clicked();
                    });
            });
        for a in actions {
            self.naming_act(&mut d, a);
        }
        if open && !close {
            self.naming = Some(d);
        }
    }
}

fn list_section(
    ui: &mut egui::Ui,
    d: &mut NamingDialog,
    project: &Project,
    secondary: egui::Color32,
    _actions: &mut Vec<NAction>,
) {
    section_header(
        ui,
        &mut d.list_open,
        tr_args(
            "naming.list",
            &[("count", &project.naming_rules.len().to_string())],
        ),
    );
    if !d.list_open {
        return;
    }
    if project.naming_rules.is_empty() {
        crate::a11y::label(ui, RichText::new(tr("naming.none")).color(secondary));
    }
    ui.horizontal_wrapped(|ui| {
        for r in &project.naming_rules {
            let text = format!("{} — {}", r.name, target_label(r.target));
            if ui
                .selectable_label(d.selected == Some(r.id), text)
                .clicked()
            {
                d.selected = Some(r.id);
            }
        }
    });
}

fn form_section(
    ui: &mut egui::Ui,
    d: &mut NamingDialog,
    project: &Project,
    secondary: egui::Color32,
    actions: &mut Vec<NAction>,
) {
    let title = match d.selected.and_then(|id| project.naming_rule(id).ok()) {
        Some(r) => tr_args("naming.form.edit", &[("name", &r.name)]),
        None => tr("naming.form.new"),
    };
    section_header(ui, &mut d.form_open, title);
    if !d.form_open {
        return;
    }
    ui.horizontal(|ui| {
        crate::a11y::label(ui, tr("naming.name"));
        crate::a11y::add_text(
            ui,
            egui::TextEdit::singleline(&mut d.name)
                .id(egui::Id::new("naming_name"))
                .desired_width(W_FIELD),
        );
    });
    ui.horizontal(|ui| {
        crate::a11y::label(ui, tr("naming.target"));
        egui::ComboBox::from_id_salt("naming_target")
            .selected_text(target_label(d.target))
            .width(W_FIELD)
            .show_ui_named(ui, |ui| {
                for t in TARGETS {
                    if ui
                        .selectable_label(d.target == t, target_label(t))
                        .clicked()
                    {
                        d.target = t;
                    }
                }
            });
    });
    ui.horizontal(|ui| {
        crate::a11y::label(ui, tr("naming.prefix"));
        crate::a11y::add_text(
            ui,
            egui::TextEdit::singleline(&mut d.prefix)
                .id(egui::Id::new("naming_prefix"))
                .desired_width(W_FIELD),
        );
    });
    ui.horizontal(|ui| {
        crate::a11y::label(ui, tr("naming.separator"));
        crate::a11y::add_text(
            ui,
            egui::TextEdit::singleline(&mut d.separator)
                .id(egui::Id::new("naming_separator"))
                .desired_width(W_NUM),
        );
        crate::a11y::label(ui, tr("naming.suffix"));
        crate::a11y::add_text(
            ui,
            egui::TextEdit::singleline(&mut d.suffix)
                .id(egui::Id::new("naming_suffix"))
                .desired_width(W_FIELD),
        );
    });
    ui.checkbox(&mut d.use_counter, tr("naming.counter"));
    if d.use_counter {
        ui.horizontal(|ui| {
            crate::a11y::label(ui, tr("naming.start"));
            crate::a11y::add_text(
                ui,
                egui::TextEdit::singleline(&mut d.start)
                    .id(egui::Id::new("naming_start"))
                    .desired_width(W_NUM),
            );
            crate::a11y::label(ui, tr("naming.step"));
            crate::a11y::add_text(
                ui,
                egui::TextEdit::singleline(&mut d.step)
                    .id(egui::Id::new("naming_step"))
                    .desired_width(W_NUM),
            );
            crate::a11y::label(ui, tr("naming.width"));
            crate::a11y::add_text(
                ui,
                egui::TextEdit::singleline(&mut d.width)
                    .id(egui::Id::new("naming_width"))
                    .desired_width(W_NUM),
            );
        });
    }
    if let Some(id) = d.selected {
        let next = match project.next_name(id) {
            Ok(n) => tr_args("naming.next", &[("name", &n)]),
            Err(e) => tr_args("naming.next.err", &[("detail", &e.to_string())]),
        };
        crate::a11y::label(ui, RichText::new(next).color(secondary));
    }
    ui.horizontal(|ui| match d.selected {
        Some(id) => {
            if button(ui, &tr("naming.update"), ButtonKind::Primary).clicked() {
                actions.push(NAction::Update(id));
            }
            if button(ui, &tr("naming.new"), ButtonKind::Secondary).clicked() {
                actions.push(NAction::New);
            }
            if button(ui, &tr("naming.remove"), ButtonKind::Danger).clicked() {
                actions.push(NAction::Remove(id));
            }
        }
        None => {
            if button(ui, &tr("naming.add"), ButtonKind::Primary).clicked() {
                actions.push(NAction::Add);
            }
        }
    });
}

fn batch_section(
    ui: &mut egui::Ui,
    d: &mut NamingDialog,
    project: &Project,
    rule: &NamingRule,
    secondary: egui::Color32,
    actions: &mut Vec<NAction>,
) {
    section_header(ui, &mut d.batch_open, tr("naming.batch"));
    if !d.batch_open {
        return;
    }
    crate::a11y::label(ui, RichText::new(tr("naming.batch.hint")).color(secondary));
    if !d.preview {
        if button(ui, &tr("naming.batch.preview"), ButtonKind::Secondary).clicked() {
            d.preview = true;
        }
        return;
    }
    match project.naming_preview(rule.id) {
        Err(e) => {
            crate::a11y::label(ui, RichText::new(e.to_string()).color(secondary));
        }
        Ok(changes) if changes.is_empty() => {
            crate::a11y::label(ui, RichText::new(tr("naming.batch.empty")).color(secondary));
        }
        Ok(changes) => {
            for c in changes.iter().take(PREVIEW_ROWS) {
                crate::a11y::label(
                    ui,
                    tr_args("naming.batch.row", &[("old", &c.old), ("new", &c.new)]),
                );
            }
            if changes.len() > PREVIEW_ROWS {
                crate::a11y::label(
                    ui,
                    RichText::new(tr_args(
                        "naming.batch.more",
                        &[("count", &(changes.len() - PREVIEW_ROWS).to_string())],
                    ))
                    .color(secondary),
                );
            }
            ui.horizontal(|ui| {
                let label = tr_args(
                    "naming.batch.apply",
                    &[("count", &changes.len().to_string())],
                );
                if button(ui, &label, ButtonKind::Primary).clicked() {
                    actions.push(NAction::Apply(rule.id));
                }
                if button(ui, &tr("naming.batch.cancel"), ButtonKind::Secondary).clicked() {
                    d.preview = false;
                }
            });
        }
    }
}
