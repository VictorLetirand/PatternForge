//! US-11.4 — fenêtre « Bibliothèque… » du dock (menu Fichier, sans raccourci, aucun outil).
//!
//! Gabarits réutilisables (pièce, bloc ou modèle) enregistrés dans un **dossier séparé du projet**
//! (réglage `library_dir` de `settings.json`, jamais une donnée du projet). Enregistrer et gérer les
//! entrées passent par `pf_core::Library` (fichiers versionnés, écriture atomique) ; **insérer** est
//! une seule `Command::InsertLibrary` = un pas d'annulation. Une entrée insérée est une copie
//! indépendante : rien n'est suivi automatiquement. Aucun calcul ici ; les conflits de variables
//! sont annoncés avant l'envoi (le cœur refuse de toute façon) et le renommage est une case explicite.

use crate::app::App;
use crate::dock::ToolWin;
use crate::grading::section_header;
use eframe::egui::{self, RichText};
use pf_core::{
    Command, Library, LibraryContent, LibraryError, LibraryKind, LibraryListing, PieceId,
};
use pf_i18n::{settings, tr, tr_args};
use pf_ui::components::{button, ButtonKind};
use std::path::PathBuf;

/// Clé du réglage dans `settings.json`.
pub const LIBRARY_DIR_SETTING: &str = "library_dir";

/// État de la fenêtre (interface seulement, jamais enregistré).
#[derive(Debug, Clone)]
pub struct LibraryDialog {
    pub dir_text: String,
    pub name: String,
    pub kind: LibraryKind,
    pub piece: Option<PieceId>,
    pub selected: Option<u64>,
    pub rename_text: String,
    /// Case explicite : renommer les variables en conflit à l'insertion.
    pub rename_vars: bool,
    /// Dernier message : `(erreur ?, texte)`.
    pub message: Option<(bool, String)>,
    pub listing: LibraryListing,
    pub save_open: bool,
    pub entries_open: bool,
    pub problems_open: bool,
}

impl LibraryDialog {
    fn new(dir: &std::path::Path) -> LibraryDialog {
        LibraryDialog {
            dir_text: dir.display().to_string(),
            name: String::new(),
            kind: LibraryKind::Piece,
            piece: None,
            selected: None,
            rename_text: String::new(),
            rename_vars: false,
            message: None,
            listing: LibraryListing::default(),
            save_open: true,
            entries_open: true,
            problems_open: true,
        }
    }
}

enum LAction {
    UseDir,
    Save,
    Insert,
    Rename,
    Delete,
}

/// Texte d'interface d'un refus de la bibliothèque (clés en littéral).
pub fn error_text(e: &LibraryError) -> String {
    match e {
        LibraryError::NameEmpty => tr("library.err.name_empty"),
        LibraryError::NameTooLong { max } => {
            tr_args("library.err.name_too_long", &[("max", &max.to_string())])
        }
        LibraryError::NameControl => tr("library.err.name_control"),
        LibraryError::NameDuplicate { name } => {
            tr_args("library.err.name_duplicate", &[("name", name)])
        }
        LibraryError::NotFound { id } => {
            tr_args("library.err.not_found", &[("id", &id.to_string())])
        }
        LibraryError::Corrupt { file, message } => tr_args(
            "library.err.corrupt",
            &[("file", file), ("detail", message)],
        ),
        LibraryError::Invalid { detail } => tr_args("library.err.invalid", &[("detail", detail)]),
        LibraryError::Io { detail } => tr_args("library.err.io", &[("detail", detail)]),
    }
}

fn kind_label(k: LibraryKind) -> String {
    tr(match k {
        LibraryKind::Piece => "library.kind.piece",
        LibraryKind::Block => "library.kind.block",
        LibraryKind::Model => "library.kind.model",
    })
}

impl App {
    /// Dossier de bibliothèque : réglage, sinon `library` à côté de `settings.json`.
    pub fn library_dir(&self) -> PathBuf {
        self.library_dir.clone().unwrap_or_else(|| {
            self.settings_path
                .as_deref()
                .and_then(std::path::Path::parent)
                .map(|p| p.join("library"))
                .unwrap_or_else(|| std::env::temp_dir().join("patternforge-library"))
        })
    }

    /// Menu Fichier › « Bibliothèque… » (sans raccourci).
    pub fn open_library(&mut self) {
        if self.library.is_none() {
            let mut d = LibraryDialog::new(&self.library_dir());
            self.library_reload(&mut d);
            self.library = Some(d);
        }
    }

    /// Relit le dossier (une fois par geste, jamais à chaque image).
    pub(crate) fn library_reload(&self, d: &mut LibraryDialog) {
        match Library::new(self.library_dir()).list() {
            Ok(l) => d.listing = l,
            Err(e) => {
                d.listing = LibraryListing::default();
                d.message = Some((true, error_text(&e)));
            }
        }
        if d.selected
            .is_some_and(|id| !d.listing.entries.iter().any(|e| e.id == id))
        {
            d.selected = None;
        }
    }

    /// Contenu à enregistrer d'après le type choisi : pièce du menu, sélection de la feuille
    /// active, ou projet entier.
    fn library_source(&self, d: &LibraryDialog) -> Result<LibraryContent, String> {
        let project = self.editor.project();
        match d.kind {
            LibraryKind::Piece => {
                let piece = d.piece.ok_or_else(|| tr("library.err.no_piece"))?;
                project.library_piece(piece).map_err(|e| e.to_string())
            }
            LibraryKind::Block => {
                let s = &self.editor.selection;
                if s.points.is_empty() && s.entities.is_empty() {
                    return Err(tr("library.err.no_selection"));
                }
                project
                    .library_block(self.editor.sheet, &s.points, &s.entities, &s.notches)
                    .map_err(|e| e.to_string())
            }
            LibraryKind::Model => project.library_model().map_err(|e| e.to_string()),
        }
    }

    fn library_act(&mut self, d: &mut LibraryDialog, action: LAction) {
        let lib = Library::new(self.library_dir());
        d.message = None;
        let result: Result<String, String> = match action {
            LAction::UseDir => {
                let dir = PathBuf::from(d.dir_text.trim());
                if d.dir_text.trim().is_empty() {
                    Err(tr("library.err.dir_empty"))
                } else {
                    self.library_dir = Some(dir.clone());
                    if let Some(p) = &self.settings_path {
                        let _ = settings::save_value(
                            p,
                            LIBRARY_DIR_SETTING,
                            serde_json::json!(dir.display().to_string()),
                        );
                    }
                    d.selected = None;
                    Ok(tr_args(
                        "library.dir.used",
                        &[("path", &dir.display().to_string())],
                    ))
                }
            }
            LAction::Save => match self.library_source(d) {
                Err(e) => Err(e),
                Ok(content) => match lib.save(&d.name, content) {
                    Ok(entry) => {
                        d.name.clear();
                        d.selected = Some(entry.id);
                        d.rename_text = entry.name.clone();
                        Ok(tr_args("library.saved", &[("name", &entry.name)]))
                    }
                    Err(e) => Err(error_text(&e)),
                },
            },
            LAction::Insert => match d.selected.map(|id| lib.load(id)) {
                None => Err(tr("library.err.no_entry")),
                Some(Err(e)) => Err(error_text(&e)),
                Some(Ok(entry)) => self.library_insert(d, &entry.name, entry.content),
            },
            LAction::Rename => match d.selected {
                None => Err(tr("library.err.no_entry")),
                Some(id) => lib
                    .rename(id, &d.rename_text)
                    .map(|e| tr_args("library.renamed", &[("name", &e.name)]))
                    .map_err(|e| error_text(&e)),
            },
            LAction::Delete => match d.selected {
                None => Err(tr("library.err.no_entry")),
                Some(id) => {
                    let name = d
                        .listing
                        .entries
                        .iter()
                        .find(|e| e.id == id)
                        .map(|e| e.name.clone())
                        .unwrap_or_default();
                    lib.delete(id)
                        .map(|_| {
                            d.selected = None;
                            tr_args("library.deleted", &[("name", &name)])
                        })
                        .map_err(|e| error_text(&e))
                }
            },
        };
        d.message = Some(match result {
            Ok(t) => (false, t),
            Err(t) => (true, t),
        });
        self.library_reload(d);
    }

    /// Insère le contenu : conflits de variables annoncés avant l'envoi, une seule commande.
    pub(crate) fn library_insert(
        &mut self,
        d: &LibraryDialog,
        name: &str,
        content: LibraryContent,
    ) -> Result<String, String> {
        if !d.rename_vars {
            match self.editor.project().library_conflicts(&content) {
                Ok(conflicts) => {
                    if let Some(c) = conflicts.first() {
                        return Err(tr_args(
                            "library.conflict",
                            &[
                                ("name", &c.existing_name),
                                ("incoming", &c.incoming_value.to_string()),
                                ("existing", &c.existing_value.to_string()),
                            ],
                        ));
                    }
                }
                Err(e) => return Err(e.to_string()),
            }
        }
        let is_block = content.kind == LibraryKind::Block;
        let command = Command::InsertLibrary {
            content: Box::new(content),
            sheet: is_block.then_some(self.editor.sheet),
            layer: if is_block {
                self.editor.current_layer()
            } else {
                None
            },
            rename_variables: d.rename_vars,
        };
        let out = self.measure_exec(command)?;
        if !is_block {
            if let Some(sheet) = out.sheet {
                self.editor.set_sheet(sheet);
            }
        }
        Ok(tr_args("library.inserted", &[("name", name)]))
    }

    pub fn library_window(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.library.take() else {
            return;
        };
        let project = self.editor.project().clone();
        let tokens = pf_ui::theme::tokens(ctx);
        let (danger, secondary) = (tokens.danger, tokens.text_secondary);
        if d.piece.is_some_and(|p| project.piece(p).is_err()) {
            d.piece = None;
        }
        let mut open = true;
        let mut close = false;
        let mut actions: Vec<LAction> = Vec::new();
        let slot = crate::dock::slot(ctx, &self.dock, ToolWin::Library);
        let width = self.dock.content_width(ToolWin::Library);
        egui::Window::new(tr("library.title"))
            .id(ToolWin::Library.id())
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
                        crate::a11y::label(ui, RichText::new(tr("library.intro")).color(secondary));
                        if let Some((is_err, text)) = &d.message {
                            let color = if *is_err { danger } else { secondary };
                            crate::a11y::label(ui, RichText::new(text).color(color));
                        }
                        ui.add_space(4.0);
                        dir_section(ui, &mut d, &mut actions);
                        ui.add_space(4.0);
                        save_section(ui, &mut d, &project, secondary, &mut actions);
                        ui.add_space(4.0);
                        entries_section(ui, &mut d, secondary, &mut actions);
                        problems_section(ui, &mut d, secondary, danger);
                        ui.add_space(6.0);
                        close = button(ui, &tr("dialog.close"), ButtonKind::Secondary).clicked();
                    });
            });
        for a in actions {
            self.library_act(&mut d, a);
        }
        if open && !close {
            self.library = Some(d);
        }
    }
}

fn dir_section(ui: &mut egui::Ui, d: &mut LibraryDialog, actions: &mut Vec<LAction>) {
    ui.horizontal(|ui| {
        crate::a11y::label(ui, tr("library.dir"));
        crate::a11y::add_text(
            ui,
            egui::TextEdit::singleline(&mut d.dir_text)
                .id(egui::Id::new("library_dir"))
                .desired_width(200.0),
        );
        if button(ui, &tr("library.dir.use"), ButtonKind::Secondary).clicked() {
            actions.push(LAction::UseDir);
        }
    });
}

fn save_section(
    ui: &mut egui::Ui,
    d: &mut LibraryDialog,
    project: &pf_core::Project,
    secondary: egui::Color32,
    actions: &mut Vec<LAction>,
) {
    section_header(ui, &mut d.save_open, tr("library.save.title"));
    if !d.save_open {
        return;
    }
    ui.horizontal(|ui| {
        crate::a11y::label(ui, tr("library.save.kind"));
        for k in [LibraryKind::Piece, LibraryKind::Block, LibraryKind::Model] {
            if ui.selectable_label(d.kind == k, kind_label(k)).clicked() {
                d.kind = k;
            }
        }
    });
    match d.kind {
        LibraryKind::Piece => {
            let current = d
                .piece
                .and_then(|p| project.piece(p).ok())
                .map(|p| p.name.clone())
                .unwrap_or_else(|| tr("library.save.piece.choose"));
            ui.horizontal(|ui| {
                crate::a11y::label(ui, tr("library.save.piece"));
                use crate::a11y::ComboExt;
                egui::ComboBox::from_id_salt("library_piece")
                    .selected_text(current)
                    .width(160.0)
                    .show_ui_named(ui, |ui| {
                        for p in &project.pieces {
                            if ui
                                .selectable_label(d.piece == Some(p.id), &p.name)
                                .clicked()
                            {
                                d.piece = Some(p.id);
                            }
                        }
                    });
            });
        }
        LibraryKind::Block => {
            crate::a11y::label(
                ui,
                RichText::new(tr("library.save.block_hint")).color(secondary),
            );
        }
        LibraryKind::Model => {
            crate::a11y::label(
                ui,
                RichText::new(tr("library.save.model_hint")).color(secondary),
            );
        }
    }
    ui.horizontal(|ui| {
        crate::a11y::label(ui, tr("library.save.name"));
        crate::a11y::add_text(
            ui,
            egui::TextEdit::singleline(&mut d.name)
                .id(egui::Id::new("library_name"))
                .desired_width(160.0),
        );
        if button(ui, &tr("library.save.button"), ButtonKind::Primary).clicked() {
            actions.push(LAction::Save);
        }
    });
}

fn entries_section(
    ui: &mut egui::Ui,
    d: &mut LibraryDialog,
    secondary: egui::Color32,
    actions: &mut Vec<LAction>,
) {
    section_header(
        ui,
        &mut d.entries_open,
        tr_args(
            "library.entries",
            &[("count", &d.listing.entries.len().to_string())],
        ),
    );
    if !d.entries_open {
        return;
    }
    if d.listing.entries.is_empty() {
        crate::a11y::label(
            ui,
            RichText::new(tr("library.entries.none")).color(secondary),
        );
        return;
    }
    let mut picked = None;
    for e in &d.listing.entries {
        // Le genre est écrit en toutes lettres (jamais par la seule couleur).
        let text = tr_args(
            "library.entry.row",
            &[("name", &e.name), ("kind", &kind_label(e.content.kind))],
        );
        if ui
            .selectable_label(d.selected == Some(e.id), text)
            .clicked()
        {
            picked = Some((e.id, e.name.clone()));
        }
    }
    if let Some((id, name)) = picked {
        d.selected = Some(id);
        d.rename_text = name;
    }
    let Some(entry) = d
        .selected
        .and_then(|id| d.listing.entries.iter().find(|e| e.id == id))
        .cloned()
    else {
        return;
    };
    let vars = if entry.content.variables.is_empty() {
        tr("library.detail.no_vars")
    } else {
        tr_args(
            "library.detail.vars",
            &[("list", &entry.content.variable_names().join(", "))],
        )
    };
    crate::a11y::label(ui, RichText::new(vars).color(secondary));
    for n in &entry.content.notes {
        crate::a11y::label(
            ui,
            RichText::new(tr_args("library.detail.note", &[("text", n)])).color(secondary),
        );
    }
    ui.horizontal(|ui| {
        ui.checkbox(&mut d.rename_vars, tr("library.insert.rename_vars"));
    });
    ui.horizontal(|ui| {
        if button(ui, &tr("library.insert"), ButtonKind::Primary).clicked() {
            actions.push(LAction::Insert);
        }
        if button(ui, &tr("library.delete"), ButtonKind::Danger).clicked() {
            actions.push(LAction::Delete);
        }
    });
    ui.horizontal(|ui| {
        crate::a11y::label(ui, tr("library.rename.label"));
        crate::a11y::add_text(
            ui,
            egui::TextEdit::singleline(&mut d.rename_text)
                .id(egui::Id::new("library_rename"))
                .desired_width(160.0),
        );
        if button(ui, &tr("library.rename"), ButtonKind::Secondary).clicked() {
            actions.push(LAction::Rename);
        }
    });
}

fn problems_section(
    ui: &mut egui::Ui,
    d: &mut LibraryDialog,
    secondary: egui::Color32,
    danger: egui::Color32,
) {
    if d.listing.problems.is_empty() {
        return;
    }
    section_header(
        ui,
        &mut d.problems_open,
        tr_args(
            "library.problems",
            &[("count", &d.listing.problems.len().to_string())],
        ),
    );
    if !d.problems_open {
        return;
    }
    crate::a11y::label(
        ui,
        RichText::new(tr("library.problems.note")).color(secondary),
    );
    for p in &d.listing.problems {
        crate::a11y::label(
            ui,
            RichText::new(tr_args(
                "library.problem.row",
                &[("file", &p.file), ("detail", &p.message)],
            ))
            .color(danger),
        );
    }
}
