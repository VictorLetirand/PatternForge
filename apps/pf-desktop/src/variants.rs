//! US-11.1 — fenêtre « Variantes » du dock (menu Fichier, sans raccourci, aucun outil).
//!
//! Une variante **référence** des pièces du projet (jamais de copie) avec quantité, matière et
//! option ; la fenêtre liste les variantes, les options (case « active » par option) et le tableau
//! des entrées. Chaque geste = une `Command` = un pas d'annulation ; refus = message sous l'intro,
//! projet inchangé. Aucun calcul métier ici : la nomenclature par variante est celle du cœur
//! (`Project::bill_of_materials_variant`, fenêtre « Nomenclature »). Rien n'est suivi
//! automatiquement : le choix de la variante est toujours explicite.

use crate::a11y::ComboExt;
use crate::app::App;
use crate::dock::ToolWin;
use crate::grading::section_header;
use crate::measurements::{edit, Buffers};
use crate::sizes::cell;
use eframe::egui::{self, RichText};
use pf_core::{Command, PieceId, Project, Variant, VariantId};
use pf_i18n::{tr, tr_args};
use pf_ui::components::{button, ButtonKind};

const W_NAME: f32 = 130.0;
const W_QTY: f32 = 40.0;
const W_MAT: f32 = 90.0;
const K_NAME: u8 = 30;
const K_QTY: u8 = 31;
const K_MAT: u8 = 32;

/// État de la fenêtre (interface seulement, jamais enregistré).
#[derive(Debug, Clone)]
pub struct VariantsDialog {
    /// Variante affichée ; `None` = la première du projet.
    pub selected: Option<VariantId>,
    pub list_open: bool,
    pub options_open: bool,
    pub entries_open: bool,
    buffers: Buffers,
    new_name: String,
    new_option: String,
    add_piece: Option<PieceId>,
    add_qty: String,
    error: Option<String>,
}

impl Default for VariantsDialog {
    fn default() -> Self {
        VariantsDialog {
            selected: None,
            list_open: true,
            options_open: true,
            entries_open: true,
            buffers: Buffers::new(),
            new_name: String::new(),
            new_option: String::new(),
            add_piece: None,
            add_qty: "1".to_string(),
            error: None,
        }
    }
}

enum VAction {
    Add,
    Rename(VariantId, String),
    Remove(VariantId),
    Duplicate(VariantId),
    OptionAdd(VariantId),
    OptionRemove(VariantId, String),
    OptionActive(VariantId, String, bool),
    EntryAdd(VariantId),
    EntryEdit {
        variant: VariantId,
        piece: PieceId,
        option: Option<String>,
        quantity: Option<String>,
        material: Option<Option<String>>,
        new_option: Option<Option<String>>,
    },
    EntryRemove(VariantId, PieceId, Option<String>),
}

/// Nom proposé pour une copie : « X (copie) », puis « X (copie) 2 »… jusqu'à un nom libre.
fn copy_name(project: &Project, base: &str) -> String {
    let first = tr_args("variants.copy_name", &[("name", base)]);
    let taken = |n: &str| {
        project
            .variants
            .iter()
            .any(|v| v.name.to_lowercase() == n.to_lowercase())
    };
    if !taken(&first) {
        return first;
    }
    (2..)
        .map(|i| format!("{first} {i}"))
        .find(|n| !taken(n))
        .unwrap_or(first)
}

impl App {
    /// Menu Fichier › « Variantes… » (sans raccourci).
    pub fn open_variants(&mut self) {
        if self.variants.is_none() {
            self.variants = Some(VariantsDialog::default());
        }
    }

    fn variants_act(&mut self, d: &mut VariantsDialog, action: VAction) {
        let project = self.editor.project().clone();
        let result: Result<(), String> = match action {
            VAction::Add => {
                let r = self.measure_exec(Command::AddVariant {
                    name: d.new_name.clone(),
                });
                match r {
                    Ok(out) => {
                        d.new_name.clear();
                        d.selected = out.variants.last().copied();
                        Ok(())
                    }
                    Err(e) => Err(e),
                }
            }
            VAction::Rename(v, name) => self
                .measure_exec(Command::RenameVariant { variant: v, name })
                .map(|_| ()),
            VAction::Remove(v) => {
                let r = self.measure_exec(Command::RemoveVariant { variant: v });
                if r.is_ok() && d.selected == Some(v) {
                    d.selected = None;
                }
                r.map(|_| ())
            }
            VAction::Duplicate(v) => match project.variant(v) {
                Ok(src) => {
                    let name = copy_name(&project, &src.name);
                    match self.measure_exec(Command::DuplicateVariant { variant: v, name }) {
                        Ok(out) => {
                            d.selected = out.variants.last().copied();
                            Ok(())
                        }
                        Err(e) => Err(e),
                    }
                }
                Err(e) => Err(e.to_string()),
            },
            VAction::OptionAdd(v) => {
                let r = self.measure_exec(Command::AddVariantOption {
                    variant: v,
                    option: d.new_option.clone(),
                });
                if r.is_ok() {
                    d.new_option.clear();
                }
                r.map(|_| ())
            }
            VAction::OptionRemove(v, option) => self
                .measure_exec(Command::RemoveVariantOption { variant: v, option })
                .map(|_| ()),
            VAction::OptionActive(v, option, active) => self
                .measure_exec(Command::SetVariantOptionActive {
                    variant: v,
                    option,
                    active,
                })
                .map(|_| ()),
            VAction::EntryAdd(v) => match (d.add_piece, d.add_qty.trim().parse::<u32>()) {
                (None, _) => Err(tr("variants.err.no_piece")),
                (_, Err(_)) => Err(tr_args("variants.err.qty", &[("text", &d.add_qty)])),
                (Some(piece), Ok(quantity)) => self
                    .measure_exec(Command::AddVariantEntry {
                        variant: v,
                        piece,
                        quantity,
                        material: None,
                        option: None,
                    })
                    .map(|_| ()),
            },
            VAction::EntryEdit {
                variant,
                piece,
                option,
                quantity,
                material,
                new_option,
            } => match project.variant(variant) {
                Ok(var) => match var
                    .entries
                    .iter()
                    .find(|e| e.piece == piece && e.option == option)
                {
                    Some(e) => {
                        let q = match quantity {
                            Some(text) => text
                                .trim()
                                .parse::<u32>()
                                .map_err(|_| tr_args("variants.err.qty", &[("text", &text)])),
                            None => Ok(e.quantity),
                        };
                        match q {
                            Ok(quantity) => self
                                .measure_exec(Command::UpdateVariantEntry {
                                    variant,
                                    piece,
                                    option: option.clone(),
                                    quantity,
                                    material: material.unwrap_or_else(|| e.material.clone()),
                                    new_option: new_option.unwrap_or_else(|| e.option.clone()),
                                })
                                .map(|_| ()),
                            Err(e) => Err(e),
                        }
                    }
                    None => Ok(()),
                },
                Err(e) => Err(e.to_string()),
            },
            VAction::EntryRemove(v, piece, option) => self
                .measure_exec(Command::RemoveVariantEntry {
                    variant: v,
                    piece,
                    option,
                })
                .map(|_| ()),
        };
        d.error = result.err();
    }

    pub fn variants_window(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.variants.take() else {
            return;
        };
        let project = self.editor.project().clone();
        let tokens = pf_ui::theme::tokens(ctx);
        let (danger, secondary) = (tokens.danger, tokens.text_secondary);
        // Une variante disparue (annulation) est oubliée : on retombe sur la première.
        d.selected = d
            .selected
            .filter(|id| project.variant(*id).is_ok())
            .or(project.variants.first().map(|v| v.id));
        if d.add_piece.is_some_and(|p| project.piece(p).is_err()) {
            d.add_piece = None;
        }
        let mut open = true;
        let mut close = false;
        let mut actions: Vec<VAction> = Vec::new();
        let slot = crate::dock::slot(ctx, &self.dock, ToolWin::Variants);
        let width = self.dock.content_width(ToolWin::Variants);
        egui::Window::new(tr("variants.title"))
            .id(ToolWin::Variants.id())
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
                        crate::a11y::label(
                            ui,
                            RichText::new(tr("variants.intro")).color(secondary),
                        );
                        crate::a11y::label(
                            ui,
                            RichText::new(tr("variants.plans_note")).color(secondary),
                        );
                        if let Some(text) = &d.error {
                            crate::a11y::label(ui, RichText::new(text).color(danger));
                        }
                        ui.add_space(4.0);
                        list_section(ui, &mut d, &project, secondary, &mut actions);
                        if let Some(v) = d.selected.and_then(|id| project.variant(id).ok()) {
                            ui.add_space(4.0);
                            options_section(ui, &mut d, v, secondary, &mut actions);
                            ui.add_space(4.0);
                            entries_section(ui, &mut d, &project, v, secondary, &mut actions);
                        }
                        ui.add_space(6.0);
                        close = button(ui, &tr("dialog.close"), ButtonKind::Secondary).clicked();
                    });
            });
        for a in actions {
            self.variants_act(&mut d, a);
        }
        if open && !close {
            self.variants = Some(d);
        }
    }
}

fn list_section(
    ui: &mut egui::Ui,
    d: &mut VariantsDialog,
    project: &Project,
    secondary: egui::Color32,
    actions: &mut Vec<VAction>,
) {
    section_header(
        ui,
        &mut d.list_open,
        tr_args(
            "variants.list",
            &[("count", &project.variants.len().to_string())],
        ),
    );
    if !d.list_open {
        return;
    }
    if project.variants.is_empty() {
        crate::a11y::label(ui, RichText::new(tr("variants.none")).color(secondary));
    }
    ui.horizontal_wrapped(|ui| {
        for v in &project.variants {
            if ui
                .selectable_label(d.selected == Some(v.id), &v.name)
                .clicked()
            {
                d.selected = Some(v.id);
            }
        }
    });
    if let Some(v) = d.selected.and_then(|id| project.variant(id).ok()) {
        ui.horizontal(|ui| {
            crate::a11y::label(ui, tr("variants.name"));
            cell(ui, W_NAME + 10.0, |ui| {
                if let Some(t) = edit(ui, &mut d.buffers, (K_NAME, v.id.0, 0), &v.name, W_NAME) {
                    actions.push(VAction::Rename(v.id, t));
                }
            });
            if button(ui, &tr("variants.duplicate"), ButtonKind::Secondary).clicked() {
                actions.push(VAction::Duplicate(v.id));
            }
            if button(ui, &tr("variants.remove"), ButtonKind::Danger).clicked() {
                actions.push(VAction::Remove(v.id));
            }
        });
    }
    ui.horizontal(|ui| {
        crate::a11y::label(ui, tr("variants.new"));
        crate::a11y::add_text(
            ui,
            egui::TextEdit::singleline(&mut d.new_name)
                .id(egui::Id::new("variants_new_name"))
                .desired_width(W_NAME),
        );
        if button(ui, &tr("variants.add"), ButtonKind::Primary).clicked() {
            actions.push(VAction::Add);
        }
    });
}

fn options_section(
    ui: &mut egui::Ui,
    d: &mut VariantsDialog,
    v: &Variant,
    secondary: egui::Color32,
    actions: &mut Vec<VAction>,
) {
    section_header(
        ui,
        &mut d.options_open,
        tr_args(
            "variants.options",
            &[("name", &v.name), ("count", &v.options.len().to_string())],
        ),
    );
    if !d.options_open {
        return;
    }
    if v.options.is_empty() {
        crate::a11y::label(
            ui,
            RichText::new(tr("variants.option.none")).color(secondary),
        );
    }
    for o in &v.options {
        let active = v.option_active(o);
        ui.horizontal(|ui| {
            let mut on = active;
            // L'état est dit en toutes lettres (jamais par la seule case ou la couleur).
            let state = tr(if active {
                "variants.option.on"
            } else {
                "variants.option.off"
            });
            if ui.checkbox(&mut on, format!("{o} : {state}")).changed() {
                actions.push(VAction::OptionActive(v.id, o.clone(), on));
            }
            if button(ui, &tr("variants.remove"), ButtonKind::Danger).clicked() {
                actions.push(VAction::OptionRemove(v.id, o.clone()));
            }
        });
    }
    ui.horizontal(|ui| {
        crate::a11y::label(ui, tr("variants.option.new"));
        crate::a11y::add_text(
            ui,
            egui::TextEdit::singleline(&mut d.new_option)
                .id(egui::Id::new("variants_new_option"))
                .desired_width(W_NAME),
        );
        if button(ui, &tr("variants.add"), ButtonKind::Primary).clicked() {
            actions.push(VAction::OptionAdd(v.id));
        }
    });
}

fn option_combo(
    ui: &mut egui::Ui,
    salt: String,
    v: &Variant,
    current: &Option<String>,
) -> Option<Option<String>> {
    let always = tr("variants.always");
    let mut chosen = None;
    egui::ComboBox::from_id_salt(salt)
        .selected_text(current.clone().unwrap_or_else(|| always.clone()))
        .width(W_NAME)
        .show_ui_named(ui, |ui| {
            if ui.selectable_label(current.is_none(), &always).clicked() {
                chosen = Some(None);
            }
            for o in &v.options {
                if ui
                    .selectable_label(current.as_ref() == Some(o), o)
                    .clicked()
                {
                    chosen = Some(Some(o.clone()));
                }
            }
        });
    chosen
}

fn entries_section(
    ui: &mut egui::Ui,
    d: &mut VariantsDialog,
    project: &Project,
    v: &Variant,
    secondary: egui::Color32,
    actions: &mut Vec<VAction>,
) {
    section_header(
        ui,
        &mut d.entries_open,
        tr_args(
            "variants.entries",
            &[("name", &v.name), ("count", &v.entries.len().to_string())],
        ),
    );
    if !d.entries_open {
        return;
    }
    if v.entries.is_empty() {
        crate::a11y::label(
            ui,
            RichText::new(tr("variants.entries.none")).color(secondary),
        );
    } else {
        ui.horizontal(|ui| {
            for (w, k) in [
                (W_NAME, "variants.col.piece"),
                (W_QTY + 8.0, "variants.col.qty"),
                (W_MAT + 8.0, "variants.col.material"),
                (W_NAME, "variants.col.option"),
            ] {
                cell(ui, w, |ui| {
                    crate::a11y::label(ui, RichText::new(tr(k)).small().color(secondary));
                });
            }
        });
    }
    for (i, e) in v.entries.iter().enumerate() {
        let name = project
            .piece(e.piece)
            .map(|p| p.name.clone())
            .unwrap_or_default();
        ui.horizontal(|ui| {
            cell(ui, W_NAME, |ui| {
                crate::a11y::label(ui, &name);
            });
            cell(ui, W_QTY + 8.0, |ui| {
                if let Some(t) = edit(
                    ui,
                    &mut d.buffers,
                    (K_QTY, v.id.0, i as u64),
                    &e.quantity.to_string(),
                    W_QTY,
                ) {
                    actions.push(VAction::EntryEdit {
                        variant: v.id,
                        piece: e.piece,
                        option: e.option.clone(),
                        quantity: Some(t),
                        material: None,
                        new_option: None,
                    });
                }
            });
            cell(ui, W_MAT + 8.0, |ui| {
                let cur = e.material.clone().unwrap_or_default();
                if let Some(t) = edit(ui, &mut d.buffers, (K_MAT, v.id.0, i as u64), &cur, W_MAT) {
                    let t = t.trim().to_string();
                    actions.push(VAction::EntryEdit {
                        variant: v.id,
                        piece: e.piece,
                        option: e.option.clone(),
                        quantity: None,
                        material: Some((!t.is_empty()).then_some(t)),
                        new_option: None,
                    });
                }
            });
            if let Some(o) = option_combo(ui, format!("variant_opt_{}_{i}", v.id.0), v, &e.option) {
                actions.push(VAction::EntryEdit {
                    variant: v.id,
                    piece: e.piece,
                    option: e.option.clone(),
                    quantity: None,
                    material: None,
                    new_option: Some(o),
                });
            }
            if button(ui, &tr("variants.remove"), ButtonKind::Danger).clicked() {
                actions.push(VAction::EntryRemove(v.id, e.piece, e.option.clone()));
            }
        });
    }
    ui.horizontal(|ui| {
        let current = d
            .add_piece
            .and_then(|p| project.piece(p).ok())
            .map(|p| p.name.clone())
            .unwrap_or_else(|| tr("variants.piece.choose"));
        egui::ComboBox::from_id_salt("variants_add_piece")
            .selected_text(current)
            .width(W_NAME)
            .show_ui_named(ui, |ui| {
                for p in &project.pieces {
                    if ui
                        .selectable_label(d.add_piece == Some(p.id), &p.name)
                        .clicked()
                    {
                        d.add_piece = Some(p.id);
                    }
                }
            });
        crate::a11y::add_text(
            ui,
            egui::TextEdit::singleline(&mut d.add_qty)
                .id(egui::Id::new("variants_add_qty"))
                .desired_width(W_QTY),
        );
        if button(ui, &tr("variants.add"), ButtonKind::Primary).clicked() {
            actions.push(VAction::EntryAdd(v.id));
        }
    });
}
