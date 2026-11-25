//! US-09.8 — export gradé : taille à exporter, sélection de tailles, nid.
//!
//! **Lecture seule** : aucune `Command`, rien n'est enregistré dans le projet (schéma 4 inchangé).
//! Le choix de la taille exportée (`App.export_size`) est un état d'interface, jamais enregistré ;
//! par défaut c'est la taille de base de la première gamme, dont la sortie est identique à l'octet
//! à celle d'avant. Toute la logique (projet évalué, noms de fichiers, découpe du nid) est dans
//! `pf_export::size_export` ; ce module ne fait que l'interface :
//! - le sélecteur « Taille à exporter » des fenêtres de mise en page du PDF, DXF et HPGL ;
//! - la fenêtre « Exporter des tailles » du dock : un fichier par taille cochée (PDF ou SVG) dans
//!   un dossier choisi, les échecs listés ;
//! - l'export du nid (SVG ou PDF) depuis la fenêtre « Nid de tailles ».

use crate::app::App;
use crate::dock::ToolWin;
use crate::palette::pal;
use eframe::egui::{self, RichText};
use pf_core::{Project, SizeId};
use pf_export::size_export::{
    default_export_size, export_size, export_sizes, nest_file_stem, nest_pdf_lang, nest_scene,
    nest_svg_lang, size_file_stem, size_files, size_project, write_size_files, ExportSize,
    SizeFormat,
};
use pf_export::{ExportError, SceneOptions};
use pf_i18n::{tr, tr_args};
use pf_ui::components::{button, ButtonKind};
use std::borrow::Cow;
use std::collections::BTreeSet;
use std::path::Path;

/// Résultat affiché de la dernière sélection exportée (texte déjà localisé).
#[derive(Debug, Clone, Default)]
pub struct SizesReport {
    pub done: String,
    pub failed: Vec<String>,
}

/// Fenêtre « Exporter des tailles » : état d'interface, jamais enregistré.
#[derive(Debug, Clone)]
pub struct ExportSizesDialog {
    pub selected: BTreeSet<SizeId>,
    pub format: SizeFormat,
    pub report: Option<SizesReport>,
}

impl Default for ExportSizesDialog {
    fn default() -> Self {
        ExportSizesDialog {
            selected: BTreeSet::new(),
            format: SizeFormat::Pdf,
            report: None,
        }
    }
}

/// Texte d'une taille dans une liste : la base est marquée.
fn size_text(s: &ExportSize) -> String {
    if s.is_base {
        tr_args("nest.tag.base", &[("label", &s.label)])
    } else {
        s.label.clone()
    }
}

/// Tailles d'un projet groupées par gamme : (nom de la gamme, tailles).
fn by_range(project: &Project) -> Vec<(String, Vec<ExportSize>)> {
    project
        .sizes
        .iter()
        .map(|r| {
            let sizes = export_sizes(project)
                .into_iter()
                .filter(|s| r.size(s.id).is_some())
                .collect();
            (r.name.clone(), sizes)
        })
        .collect()
}

impl App {
    /// Taille réellement exportée : le choix de l'utilisateur s'il existe encore, sinon la base de
    /// la première gamme ; `None` sans gamme (le projet est exporté tel quel).
    pub fn effective_export_size(&self) -> Option<SizeId> {
        let project = self.editor.project();
        self.export_size
            .filter(|id| export_size(project, *id).is_some())
            .or_else(|| default_export_size(project))
    }

    /// La taille exportée, avec son libellé et sa gamme.
    pub fn export_size_info(&self) -> Option<ExportSize> {
        self.effective_export_size()
            .and_then(|id| export_size(self.editor.project(), id))
    }

    /// Projet lu par les exports : celui de la taille choisie (le projet lui-même pour une taille
    /// de base : aucune copie, sortie identique à l'octet).
    pub fn export_project(&self) -> Result<Cow<'_, Project>, ExportError> {
        size_project(self.editor.project(), self.effective_export_size())
    }

    /// Nom de fichier proposé : `stem` pour la base, `stem-<taille>` sinon.
    pub fn export_stem(&self, stem: &str) -> String {
        size_file_stem(stem, self.export_size_info().as_ref())
    }

    /// Fin de la note de l'état après un export : « — taille L » hors taille de base.
    pub fn export_size_note(&self) -> String {
        match self.export_size_info() {
            Some(s) if !s.is_base => tr_args("export.note.size", &[("size", &s.label)]),
            _ => String::new(),
        }
    }

    /// Mention « Export : taille M » et sélecteur de la taille exportée, au sommet des fenêtres
    /// d'export. Rien sans gamme. Renvoie le nouveau choix quand l'utilisateur en fait un.
    /// `name_only` : le fichier ne porte pas d'en-tête de taille (DXF, HPGL), seul son nom.
    pub(crate) fn export_size_section(&self, ui: &mut egui::Ui, name_only: bool) -> Option<SizeId> {
        let info = self.export_size_info()?;
        let project = self.editor.project();
        let mut chosen = None;
        crate::a11y::label(
            ui,
            RichText::new(tr_args("print.export_size", &[("name", &info.label)])).strong(),
        );
        crate::a11y::label(ui, RichText::new(tr("exsize.title")).small().strong());
        let groups = by_range(project);
        for (name, sizes) in &groups {
            ui.horizontal_wrapped(|ui| {
                if groups.len() > 1 {
                    crate::a11y::label(ui, RichText::new(name).small().color(pal().muted));
                }
                for s in sizes {
                    if ui.selectable_label(s.id == info.id, size_text(s)).clicked() {
                        chosen = Some(s.id);
                    }
                }
            });
        }
        let key = if name_only {
            "exsize.hint.name_only"
        } else {
            "exsize.hint"
        };
        crate::a11y::label(ui, RichText::new(tr(key)).small().color(pal().muted));
        chosen
    }

    // ----- sélection de tailles ------------------------------------------------------------

    /// Menu Fichier › « Exporter des tailles… ».
    pub fn open_export_sizes(&mut self) {
        if self.export_sizes.is_none() {
            self.export_sizes = Some(ExportSizesDialog::default());
        }
    }

    /// Exporte les tailles cochées dans `dir` : un fichier par taille (`<projet>-<taille>.<ext>`,
    /// feuille courante, réglages de page et contenu de sortie en vigueur). Un échec n'arrête pas
    /// les autres tailles ; tous les échecs sont listés. Renvoie `true` si tout est écrit.
    pub fn export_sizes_to(&mut self, dir: &Path) -> bool {
        let Some(d) = &self.export_sizes else {
            return false;
        };
        let project = self.editor.project();
        let ids: Vec<SizeId> = export_sizes(project)
            .into_iter()
            .map(|s| s.id)
            .filter(|id| d.selected.contains(id))
            .collect();
        if ids.is_empty() {
            self.editor.error(tr("exsz.none_selected"));
            return false;
        }
        let stem = Self::safe_file_stem(&project.name);
        let files = size_files(
            project,
            self.editor.sheet,
            &ids,
            &stem,
            d.format,
            &self.page_setup,
            &self.output_content,
            SceneOptions {
                lang: pf_i18n::lang(),
                ..SceneOptions::default()
            },
        );
        let written = write_size_files(dir, &files);
        let report = SizesReport {
            done: tr_args(
                "exsz.done",
                &[
                    ("n", &written.written.len().to_string()),
                    ("dir", &dir.display().to_string()),
                ],
            ),
            failed: written
                .failed
                .iter()
                .map(|f| {
                    tr_args(
                        "exsz.failed.line",
                        &[
                            ("label", &f.label),
                            ("file", &f.file_name),
                            ("detail", &crate::print_setup::export_error(&f.error)),
                        ],
                    )
                })
                .collect(),
        };
        let ok = report.failed.is_empty();
        self.editor.status.text = report.done.clone();
        self.editor.status.error = !ok;
        if let Some(d) = self.export_sizes.as_mut() {
            d.report = Some(report);
        }
        ok
    }

    fn export_sizes_dialog_run(&mut self) {
        if let Some(dir) = rfd::FileDialog::new().pick_folder() {
            self.export_sizes_to(&dir);
        }
    }

    pub fn export_sizes_window(&mut self, ctx: &egui::Context) {
        if self.export_sizes.is_none() {
            return;
        }
        let secondary = pf_ui::theme::tokens(ctx).text_secondary;
        let groups = by_range(self.editor.project());
        let mut dialog = self.export_sizes.take().unwrap_or_default();
        // Les tailles supprimées depuis la dernière image ne restent pas cochées.
        let known: BTreeSet<SizeId> = groups
            .iter()
            .flat_map(|(_, s)| s.iter().map(|s| s.id))
            .collect();
        dialog.selected.retain(|id| known.contains(id));
        let (mut run, mut close, mut open) = (false, false, true);
        let slot = crate::dock::slot(ctx, &self.dock, ToolWin::ExportSizes);
        let width = self.dock.content_width(ToolWin::ExportSizes);
        egui::Window::new(tr("exsz.title"))
            .id(ToolWin::ExportSizes.id())
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
                        crate::a11y::label(ui, RichText::new(tr("exsz.intro")).color(secondary));
                        ui.add_space(4.0);
                        if known.is_empty() {
                            crate::a11y::label(
                                ui,
                                RichText::new(tr("sizemeas.no_sizes")).color(secondary),
                            );
                            ui.add_space(6.0);
                            close =
                                button(ui, &tr("dialog.close"), ButtonKind::Secondary).clicked();
                            return;
                        }
                        ui.horizontal(|ui| {
                            crate::a11y::label(ui, tr("exsz.format"));
                            ui.selectable_value(&mut dialog.format, SizeFormat::Pdf, "PDF");
                            ui.selectable_value(&mut dialog.format, SizeFormat::Svg, "SVG");
                        });
                        ui.add_space(4.0);
                        crate::a11y::label(ui, RichText::new(tr("exsz.sizes")).strong());
                        ui.horizontal(|ui| {
                            if button(ui, &tr("exsz.all"), ButtonKind::Secondary).clicked() {
                                dialog.selected = known.clone();
                            }
                            if button(ui, &tr("exsz.none"), ButtonKind::Secondary).clicked() {
                                dialog.selected.clear();
                            }
                        });
                        for (name, sizes) in &groups {
                            if groups.len() > 1 {
                                crate::a11y::label(
                                    ui,
                                    RichText::new(name).small().color(secondary),
                                );
                            }
                            ui.horizontal_wrapped(|ui| {
                                for s in sizes {
                                    let mut on = dialog.selected.contains(&s.id);
                                    if ui.checkbox(&mut on, size_text(s)).changed() {
                                        if on {
                                            dialog.selected.insert(s.id);
                                        } else {
                                            dialog.selected.remove(&s.id);
                                        }
                                    }
                                }
                            });
                        }
                        crate::a11y::label(
                            ui,
                            tr_args("exsz.count", &[("n", &dialog.selected.len().to_string())]),
                        );
                        crate::a11y::label(
                            ui,
                            RichText::new(tr("exsz.note")).small().color(secondary),
                        );
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            let enabled = !dialog.selected.is_empty();
                            ui.add_enabled_ui(enabled, |ui| {
                                run = button(ui, &tr("exsz.run"), ButtonKind::Primary).clicked();
                            });
                            close =
                                button(ui, &tr("dialog.close"), ButtonKind::Secondary).clicked();
                        });
                        if let Some(r) = &dialog.report {
                            ui.add_space(4.0);
                            crate::a11y::label(ui, RichText::new(&r.done).strong());
                            if !r.failed.is_empty() {
                                crate::a11y::label(
                                    ui,
                                    RichText::new(tr_args(
                                        "exsz.failed",
                                        &[("n", &r.failed.len().to_string())],
                                    ))
                                    .color(pal().error)
                                    .strong(),
                                );
                                for line in &r.failed {
                                    crate::a11y::label(ui, RichText::new(line).color(pal().error));
                                }
                            }
                        }
                    });
            });
        self.export_sizes = Some(dialog);
        if run {
            self.export_sizes_dialog_run();
        }
        if close || !open {
            self.export_sizes = None;
        }
    }

    // ----- nid -------------------------------------------------------------------------------

    /// Nom de fichier proposé pour le nid : `<projet>-<nid>-<gamme>.<ext>`.
    pub(crate) fn nest_file_name(&self, format: SizeFormat) -> String {
        let project = self.editor.project();
        let range = self
            .nest
            .range
            .and_then(|id| project.size_range(id).ok())
            .map(|r| r.name.clone())
            .unwrap_or_default();
        let stem = Self::safe_file_stem(&project.name);
        format!(
            "{}.{}",
            nest_file_stem(&stem, &range, pf_i18n::lang()),
            format.extension()
        )
    }

    /// Écrit le nid de la gamme et du point d'empilement choisis dans la fenêtre « Nid de
    /// tailles », tailles cochées seulement, en SVG ou en PDF, dans `path`. Renvoie `true` si le
    /// fichier est écrit ; un refus ou un échec est dit (barre d'état, boîte d'erreur) sans
    /// fichier partiel.
    pub fn export_nest_to(&mut self, format: SizeFormat, path: &Path) -> bool {
        let project = self.editor.project();
        let Some(range) = self.nest.range else {
            self.editor.error(tr("sizemeas.no_sizes"));
            return false;
        };
        let anchor = self
            .nest
            .anchor
            .map_or(pf_core::NestAnchor::Origin, pf_core::NestAnchor::Point);
        let nest = match project.size_nest(self.editor.sheet, range, anchor) {
            Ok(n) => n,
            Err(e) => {
                self.editor.error(e.to_string());
                return false;
            }
        };
        let shown: Vec<SizeId> = nest
            .sizes
            .iter()
            .map(|s| s.size)
            .filter(|id| !self.nest.hidden.contains(id))
            .collect();
        if shown.is_empty() {
            self.editor.error(tr("nest.export.nothing"));
            return false;
        }
        let export = match nest_scene(project, &nest, &shown, pf_i18n::lang()) {
            Ok(e) => e,
            Err(e) => {
                self.editor.error(crate::print_setup::export_error(&e));
                return false;
            }
        };
        let bytes = match format {
            SizeFormat::Svg => nest_svg_lang(&export, pf_i18n::lang()),
            SizeFormat::Pdf => nest_pdf_lang(&export, &self.page_setup, pf_i18n::lang()),
        };
        let bytes = match bytes {
            Ok(b) => b,
            Err(e) => {
                self.editor.error(crate::print_setup::export_error(&e));
                return false;
            }
        };
        if let Err(e) = std::fs::write(path, bytes) {
            self.error_dialog = Some(tr_args("export.failed", &[("detail", &e.to_string())]));
            return false;
        }
        let mut note = tr_args("export.note.nest", &[("n", &shown.len().to_string())]);
        for (size, message) in &export.issues {
            note.push_str(&format!(
                " ; {}",
                tr_args("nest.export.issue", &[("size", size), ("message", message)])
            ));
        }
        let what = match format {
            SizeFormat::Svg => "SVG",
            SizeFormat::Pdf => "PDF",
        };
        self.editor.status.text = tr_args(
            "export.done",
            &[
                ("what", what),
                ("note", &note),
                ("path", &path.display().to_string()),
            ],
        );
        self.editor.status.error = !export.issues.is_empty();
        true
    }

    /// Boîte « Enregistrer sous » puis [`App::export_nest_to`].
    pub(crate) fn export_nest_dialog(&mut self, format: SizeFormat) {
        let name = self.nest_file_name(format);
        let what = match format {
            SizeFormat::Svg => "SVG",
            SizeFormat::Pdf => "PDF",
        };
        if let Some(path) = rfd::FileDialog::new()
            .add_filter(what, &[format.extension()])
            .set_file_name(name)
            .save_file()
        {
            self.export_nest_to(format, &path);
        }
    }
}
