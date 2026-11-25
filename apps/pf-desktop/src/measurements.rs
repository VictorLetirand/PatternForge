//! US-10.1 et US-10.2 — fenêtres « Barème de mesures » et « Fiches client » du dock (menu
//! Affichage, sans raccourci).
//!
//! Barème : le tableau **mesures × tailles** d'une gamme, éditable cellule par cellule (une saisie
//! validée = une commande `SetMeasurementValue` / `ClearMeasurementValue`, une case vidée = valeur
//! effacée), ajout / renommage / suppression d'une mesure, import et export CSV (réglages
//! explicites ; lecture tout ou rien, erreurs avec ligne et colonne ; rien n'est modifié si une
//! ligne est en erreur).
//!
//! Fiches client : liste des fiches, valeurs par mesure du barème, mesures **manquantes** (texte
//! « manquante », jamais devinées) et section repliée « Taille la plus proche » = lecture du cœur
//! (`Project::client_size_proposal`) avec l'écart par mesure : **information seulement**, jamais
//! appliquée. Aucun calcul métier ici.

use crate::app::App;
use crate::dock::ToolWin;
use crate::grading::section_header;
use crate::sizes::{cell, marker_icon};
use eframe::egui::{self, RichText};
use pf_core::input::{format_number, parse_angle_deg};
use pf_core::units::parse_length_display;
use pf_core::{
    export_measurement_csv, read_measurement_csv, ClientId, Command, CsvEncoding, CsvFormat,
    MeasurementId, MeasurementUnit, Outcome, SizeId, SizeRangeId,
};
use pf_i18n::{tr, tr_args};
use pf_ui::components::{button, ButtonKind};
use std::collections::HashMap;
use std::path::Path;

const W_NAME: f32 = 92.0;
const W_UNIT: f32 = 34.0;
const W_CELL: f32 = 48.0;
const W_DEL: f32 = 50.0;
/// Colonnes de la fiche client (le nom de la mesure y est plus large que dans le tableau).
const W_CLIENT_NAME: f32 = 150.0;
const W_CLIENT_UNIT: f32 = 44.0;
/// Nombre de lignes d'erreur CSV affichées.
const MAX_ERRORS_SHOWN: usize = 12;

/// Tampons de champs : (genre, id, id secondaire).
pub(crate) type Buffers = HashMap<(u8, u64, u64), String>;

const K_NAME: u8 = 0;
const K_CELL: u8 = 1;
const K_CLIENT_NAME: u8 = 2;
const K_CLIENT_VALUE: u8 = 3;

/// État de la fenêtre « Barème de mesures » (interface seulement, jamais enregistré).
#[derive(Debug, Clone, Default)]
pub struct BaremeDialog {
    /// Gamme affichée ; `None` = la première du projet.
    pub range: Option<SizeRangeId>,
    buffers: Buffers,
    pub new_name: String,
    pub new_deg: bool,
    error: Option<String>,
    notice: Option<String>,
    /// Section « Fichier CSV » dépliée (repliée au départ).
    pub csv_open: bool,
    pub format: CsvFormat,
    /// Une cellule vide du fichier efface la valeur existante.
    pub clear_empty: bool,
    /// Erreurs de la dernière lecture refusée, localisées.
    import_errors: Vec<String>,
}

/// État de la fenêtre « Fiches client » (interface seulement, jamais enregistré).
#[derive(Debug, Clone, Default)]
pub struct ClientsDialog {
    pub selected: Option<ClientId>,
    buffers: Buffers,
    pub new_name: String,
    error: Option<String>,
    notice: Option<String>,
    /// Section « Taille la plus proche » dépliée (repliée au départ).
    pub proposal_open: bool,
    /// Gamme comparée ; `None` = la première du projet.
    pub range: Option<SizeRangeId>,
    /// Nom de la taille à générer depuis la fiche (US-10.2).
    pub gen_label: String,
    /// Libellé dont le remplacement attend une confirmation explicite.
    pub(crate) gen_pending: Option<String>,
    /// Rapport de la dernière génération : (libellé, mesures renseignées, mesures manquantes).
    pub(crate) gen_report: Option<(String, usize, Vec<String>)>,
}

/// Champ de texte à tampon resynchronisé sur le projet tant qu'il n'a pas le focus ; renvoie le
/// texte validé (focus perdu) quand il diffère du projet.
pub(crate) fn edit(
    ui: &mut egui::Ui,
    bufs: &mut Buffers,
    key: (u8, u64, u64),
    current: &str,
    width: f32,
) -> Option<String> {
    let id = egui::Id::new(("bareme_field", key));
    let focused = ui.ctx().memory(|m| m.has_focus(id));
    let buf = bufs.entry(key).or_default();
    if !focused {
        *buf = current.to_string();
    }
    let response = crate::a11y::add_text(
        ui,
        egui::TextEdit::singleline(buf).id(id).desired_width(width),
    );
    (response.lost_focus() && buf.as_str() != current).then(|| buf.clone())
}

/// Valeur saisie dans l'unité de la mesure (millimètres ou degrés).
pub(crate) fn parse_value(unit: MeasurementUnit, text: &str) -> Result<f64, String> {
    match unit {
        MeasurementUnit::Mm => parse_length_display(text),
        MeasurementUnit::Deg => parse_angle_deg(text),
    }
}

pub(crate) fn unit_label(unit: MeasurementUnit) -> String {
    use pf_core::units::{display_units, LengthUnit};
    match unit {
        MeasurementUnit::Mm if display_units().unit != LengthUnit::Mm => {
            display_units().unit.suffix().to_string()
        }
        MeasurementUnit::Mm => tr("bareme.unit.mm"),
        MeasurementUnit::Deg => tr("bareme.unit.deg"),
    }
}

/// Valeur de mesure affichée : une longueur suit l'unité d'affichage (US-16.5), un angle non.
pub(crate) fn show_u(unit: MeasurementUnit, v: f64) -> String {
    match unit {
        MeasurementUnit::Mm => pf_core::units::len_number(v, 3),
        MeasurementUnit::Deg => show(v),
    }
}

/// Texte exact d'un champ de mesure (relu sans arrondi) : même règle que [`show_u`].
pub(crate) fn field_u(unit: MeasurementUnit, v: f64) -> String {
    match unit {
        MeasurementUnit::Mm => pf_core::units::field_text(v),
        MeasurementUnit::Deg => show(v),
    }
}

/// Unité d'une mesure du barème (mm si elle a disparu).
pub(crate) fn unit_of(project: &pf_core::Project, id: MeasurementId) -> MeasurementUnit {
    project
        .measurement(id)
        .map_or(MeasurementUnit::Mm, |m| m.unit)
}

/// Valeur affichée (3 décimales au plus, sans zéros inutiles).
pub(crate) fn show(v: f64) -> String {
    format_number(v, 3)
}

/// Gamme affichée : celle choisie si elle existe encore, sinon la première.
pub(crate) fn pick_range(
    ranges: &[(SizeRangeId, String)],
    wanted: Option<SizeRangeId>,
) -> Option<SizeRangeId> {
    wanted
        .filter(|id| ranges.iter().any(|(r, _)| r == id))
        .or(ranges.first().map(|(r, _)| *r))
}

pub(crate) fn range_selector(
    ui: &mut egui::Ui,
    ranges: &[(SizeRangeId, String)],
    current: Option<SizeRangeId>,
) -> Option<SizeRangeId> {
    let mut chosen = None;
    if ranges.len() > 1 {
        ui.horizontal_wrapped(|ui| {
            crate::a11y::label(ui, tr("bareme.range"));
            for (id, name) in ranges {
                if ui.selectable_label(Some(*id) == current, name).clicked() {
                    chosen = Some(*id);
                }
            }
        });
    }
    chosen
}

enum BAction {
    Add,
    Rename(MeasurementId, String),
    Remove(MeasurementId),
    SetCell(MeasurementId, SizeId, f64),
    ClearCell(MeasurementId, SizeId),
    Export(SizeRangeId),
    Import(SizeRangeId),
}

impl App {
    /// Menu Affichage › « Barème de mesures… » (sans raccourci).
    pub fn open_bareme(&mut self) {
        if self.bareme.is_none() {
            self.bareme = Some(BaremeDialog::default());
        }
    }

    /// Menu Affichage › « Fiches client… » (sans raccourci).
    pub fn open_clients(&mut self) {
        if self.clients.is_none() {
            self.clients = Some(ClientsDialog::default());
        }
    }

    /// Exécute une commande ; `Err` = le message (déjà localisé) du refus.
    pub(crate) fn measure_exec(&mut self, cmd: Command) -> Result<Outcome, String> {
        match self.editor.exec(cmd) {
            Some(out) => Ok(out),
            None => Err(self.editor.status.text.clone()),
        }
    }

    /// Écrit le barème de la gamme `range` dans `path` (CSV). Le projet n'est pas modifié ; un
    /// échec ouvre la boîte d'erreur, sans fichier. Renvoie le message de confirmation.
    pub fn bareme_export_to(
        &mut self,
        range: SizeRangeId,
        format: &CsvFormat,
        path: &Path,
    ) -> Option<String> {
        let project = self.editor.project();
        let range_name = project
            .size_range(range)
            .map(|r| r.name.clone())
            .unwrap_or_default();
        let out = match export_measurement_csv(project, range, format) {
            Ok(o) => o,
            Err(e) => {
                self.error_dialog = Some(tr_args("export.failed", &[("detail", &e)]));
                return None;
            }
        };
        if let Err(e) = std::fs::write(path, &out.bytes) {
            self.error_dialog = Some(tr_args("export.failed", &[("detail", &e.to_string())]));
            return None;
        }
        let mut note = tr_args(
            "bareme.csv.export_note",
            &[
                ("rows", &out.measurements.to_string()),
                ("range", &range_name),
            ],
        );
        if out.omitted_values > 0 {
            note.push_str(&tr_args(
                "bareme.csv.omitted",
                &[("count", &out.omitted_values.to_string())],
            ));
        }
        let text = tr_args(
            "export.done",
            &[
                ("what", "CSV"),
                ("note", &note),
                ("path", &path.display().to_string()),
            ],
        );
        self.editor.status.text = text.clone();
        self.editor.status.error = false;
        Some(text)
    }

    /// Lit `path` et importe le barème dans la gamme `range` : tout ou rien. `Err` = lignes
    /// d'erreur localisées (aucun changement) ; `Ok` = message de confirmation.
    pub fn bareme_import_from(
        &mut self,
        range: SizeRangeId,
        format: &CsvFormat,
        clear_empty: bool,
        path: &Path,
    ) -> Result<String, Vec<String>> {
        let bytes = std::fs::read(path).map_err(|e| {
            vec![tr_args(
                "bareme.csv.read_failed",
                &[("detail", &e.to_string())],
            )]
        })?;
        let csv = read_measurement_csv(self.editor.project(), range, &bytes, format).map_err(
            |errors| {
                let mut lines: Vec<String> = errors
                    .iter()
                    .take(MAX_ERRORS_SHOWN)
                    .map(|e| match (e.line, e.column) {
                        (0, _) => e.message.clone(),
                        (l, None) => tr_args(
                            "bareme.csv.err_line",
                            &[("line", &l.to_string()), ("message", &e.message)],
                        ),
                        (l, Some(c)) => tr_args(
                            "bareme.csv.err_cell",
                            &[
                                ("line", &l.to_string()),
                                ("column", &c.to_string()),
                                ("message", &e.message),
                            ],
                        ),
                    })
                    .collect();
                if errors.len() > MAX_ERRORS_SHOWN {
                    lines.push(tr_args(
                        "bareme.csv.errors_more",
                        &[("count", &(errors.len() - MAX_ERRORS_SHOWN).to_string())],
                    ));
                }
                lines
            },
        )?;
        let total = csv.rows.len();
        match self.measure_exec(Command::ImportMeasurements {
            rows: csv.rows,
            clear_empty,
        }) {
            Ok(out) => {
                let created = out.measurements.len();
                let text = tr_args(
                    "bareme.csv.imported",
                    &[
                        ("created", &created.to_string()),
                        ("updated", &(total - created).to_string()),
                    ],
                );
                self.editor.status.text = text.clone();
                self.editor.status.error = false;
                Ok(text)
            }
            Err(m) => Err(vec![m]),
        }
    }

    fn bareme_act(&mut self, d: &mut BaremeDialog, action: BAction) {
        d.notice = None;
        d.import_errors.clear();
        let result: Result<Option<String>, String> = match action {
            BAction::Add => {
                let unit = if d.new_deg {
                    MeasurementUnit::Deg
                } else {
                    MeasurementUnit::Mm
                };
                let r = self.measure_exec(Command::AddMeasurement {
                    name: d.new_name.clone(),
                    unit,
                });
                if r.is_ok() {
                    d.new_name.clear();
                }
                r.map(|_| None)
            }
            BAction::Rename(m, name) => self
                .measure_exec(Command::RenameMeasurement {
                    measurement: m,
                    name,
                })
                .map(|_| None),
            BAction::Remove(m) => self
                .measure_exec(Command::RemoveMeasurement { measurement: m })
                .map(|out| (!out.notes.is_empty()).then(|| tr("bareme.removed_refs"))),
            BAction::SetCell(m, s, v) => self
                .measure_exec(Command::SetMeasurementValue {
                    measurement: m,
                    size: s,
                    value: v,
                })
                .map(|_| None),
            BAction::ClearCell(m, s) => self
                .measure_exec(Command::ClearMeasurementValue {
                    measurement: m,
                    size: s,
                })
                .map(|_| None),
            BAction::Export(range) => {
                let stem = Self::safe_file_stem(&self.editor.project().name);
                match rfd::FileDialog::new()
                    .add_filter("CSV", &["csv"])
                    .set_file_name(format!("{stem}-bareme.csv"))
                    .save_file()
                {
                    Some(path) => Ok(self.bareme_export_to(range, &d.format, &path)),
                    None => Ok(None),
                }
            }
            BAction::Import(range) => match rfd::FileDialog::new()
                .add_filter("CSV", &["csv", "txt"])
                .pick_file()
            {
                Some(path) => match self.bareme_import_from(range, &d.format, d.clear_empty, &path)
                {
                    Ok(t) => Ok(Some(t)),
                    Err(lines) => {
                        d.import_errors = lines;
                        Ok(None)
                    }
                },
                None => Ok(None),
            },
        };
        match result {
            Ok(n) => {
                d.error = None;
                d.notice = n;
            }
            Err(m) => d.error = Some(m),
        }
    }

    pub fn bareme_window(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.bareme.take() else {
            return;
        };
        let project = self.editor.project().clone();
        let tokens = pf_ui::theme::tokens(ctx);
        let (danger, secondary) = (tokens.danger, tokens.text_secondary);
        let ranges: Vec<(SizeRangeId, String)> = project
            .sizes
            .iter()
            .map(|r| (r.id, r.name.clone()))
            .collect();
        let range = pick_range(&ranges, d.range);
        d.range = range;
        let mut open = true;
        let mut close = false;
        let mut actions: Vec<BAction> = Vec::new();
        let slot = crate::dock::slot(ctx, &self.dock, ToolWin::Bareme);
        let width = self.dock.content_width(ToolWin::Bareme);
        egui::Window::new(tr("bareme.title"))
            .id(ToolWin::Bareme.id())
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
                        crate::a11y::label(ui, RichText::new(tr("bareme.intro")).color(secondary));
                        ui.add_space(4.0);
                        if let Some(id) = range_selector(ui, &ranges, range) {
                            d.range = Some(id);
                        }
                        let shown = project
                            .sizes
                            .iter()
                            .find(|r| Some(r.id) == d.range)
                            .cloned();
                        if shown.is_none() {
                            crate::a11y::label(
                                ui,
                                RichText::new(tr("bareme.no_sizes")).color(secondary),
                            );
                        }
                        if project.measurements.is_empty() {
                            crate::a11y::label(
                                ui,
                                RichText::new(tr("bareme.empty")).color(secondary),
                            );
                        } else {
                            egui::ScrollArea::horizontal()
                                .id_salt("bareme_table")
                                .show(ui, |ui| {
                                    ui.spacing_mut().item_spacing.x = 3.0;
                                    ui.horizontal(|ui| {
                                        cell(ui, W_NAME, |ui| {
                                            crate::a11y::label(
                                                ui,
                                                RichText::new(tr("bareme.col.name"))
                                                    .color(secondary),
                                            );
                                        });
                                        cell(ui, W_UNIT, |ui| {
                                            crate::a11y::label(
                                                ui,
                                                RichText::new(tr("bareme.col.unit"))
                                                    .color(secondary),
                                            );
                                        });
                                        if let Some(r) = &shown {
                                            for s in &r.sizes {
                                                cell(ui, W_CELL, |ui| {
                                                    if s.id == r.base {
                                                        marker_icon(ui, false, true);
                                                    }
                                                    crate::a11y::label(
                                                        ui,
                                                        RichText::new(&s.label).strong(),
                                                    );
                                                });
                                            }
                                        }
                                    });
                                    for m in &project.measurements {
                                        ui.horizontal(|ui| {
                                            cell(ui, W_NAME, |ui| {
                                                if let Some(t) = edit(
                                                    ui,
                                                    &mut d.buffers,
                                                    (K_NAME, m.id.0, 0),
                                                    &m.name,
                                                    W_NAME - 2.0,
                                                ) {
                                                    actions.push(BAction::Rename(m.id, t));
                                                }
                                            });
                                            cell(ui, W_UNIT, |ui| {
                                                crate::a11y::label(ui, unit_label(m.unit));
                                            });
                                            if let Some(r) = &shown {
                                                for s in &r.sizes {
                                                    let current = m
                                                        .values
                                                        .iter()
                                                        .find(|v| v.size == s.id)
                                                        .map(|v| field_u(m.unit, v.value));
                                                    cell(ui, W_CELL, |ui| {
                                                        let committed = edit(
                                                            ui,
                                                            &mut d.buffers,
                                                            (K_CELL, m.id.0, s.id.0),
                                                            current.as_deref().unwrap_or(""),
                                                            W_CELL - 2.0,
                                                        );
                                                        let Some(text) = committed else {
                                                            return;
                                                        };
                                                        if text.trim().is_empty() {
                                                            actions.push(BAction::ClearCell(
                                                                m.id, s.id,
                                                            ));
                                                        } else {
                                                            match parse_value(m.unit, &text) {
                                                                Ok(v) => actions.push(
                                                                    BAction::SetCell(m.id, s.id, v),
                                                                ),
                                                                Err(_) => {
                                                                    d.error = Some(tr_args(
                                                                        "bareme.err.number",
                                                                        &[("text", &text)],
                                                                    ));
                                                                }
                                                            }
                                                        }
                                                    });
                                                }
                                            }
                                            cell(ui, W_DEL, |ui| {
                                                if button(
                                                    ui,
                                                    &tr("bareme.remove"),
                                                    ButtonKind::Danger,
                                                )
                                                .on_hover_text(tr("bareme.remove_hover"))
                                                .clicked()
                                                {
                                                    actions.push(BAction::Remove(m.id));
                                                }
                                            });
                                        });
                                    }
                                });
                        }
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            crate::a11y::label(ui, tr("bareme.new"));
                            crate::a11y::add_text(
                                ui,
                                egui::TextEdit::singleline(&mut d.new_name)
                                    .id(egui::Id::new("bareme_new_name"))
                                    .desired_width(140.0),
                            );
                            if ui
                                .selectable_label(!d.new_deg, tr("bareme.unit.mm"))
                                .clicked()
                            {
                                d.new_deg = false;
                            }
                            if ui
                                .selectable_label(d.new_deg, tr("bareme.unit.deg"))
                                .clicked()
                            {
                                d.new_deg = true;
                            }
                            if button(ui, &tr("bareme.add"), ButtonKind::Primary).clicked() {
                                actions.push(BAction::Add);
                            }
                        });
                        if let Some(text) = &d.error {
                            ui.add_space(4.0);
                            crate::a11y::label(ui, RichText::new(text).color(danger));
                        } else if let Some(text) = &d.notice {
                            ui.add_space(4.0);
                            crate::a11y::label(ui, RichText::new(text).color(secondary));
                        }
                        ui.add_space(6.0);
                        ui.separator();
                        csv_section(ui, &mut d, range, secondary, danger, &mut actions);
                        ui.add_space(6.0);
                        crate::a11y::label(ui, RichText::new(tr("bareme.note")).color(secondary));
                        ui.add_space(6.0);
                        close = button(ui, &tr("dialog.close"), ButtonKind::Secondary).clicked();
                    });
            });
        for a in actions {
            self.bareme_act(&mut d, a);
        }
        if open && !close {
            self.bareme = Some(d);
        }
    }
}

fn csv_section(
    ui: &mut egui::Ui,
    d: &mut BaremeDialog,
    range: Option<SizeRangeId>,
    secondary: egui::Color32,
    danger: egui::Color32,
    actions: &mut Vec<BAction>,
) {
    section_header(ui, &mut d.csv_open, tr("bareme.csv.section"));
    if d.csv_open {
        ui.horizontal_wrapped(|ui| {
            crate::a11y::label(ui, tr("bareme.csv.separator"));
            for (c, key) in [
                (';', "bareme.csv.sep.semicolon"),
                (',', "bareme.csv.sep.comma"),
                ('\t', "bareme.csv.sep.tab"),
            ] {
                if ui
                    .selectable_label(d.format.separator == c, tr(key))
                    .clicked()
                {
                    d.format.separator = c;
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            crate::a11y::label(ui, tr("bareme.csv.decimal"));
            for (c, key) in [('.', "bareme.csv.dec.point"), (',', "bareme.csv.dec.comma")] {
                if ui
                    .selectable_label(d.format.decimal == c, tr(key))
                    .clicked()
                {
                    d.format.decimal = c;
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            crate::a11y::label(ui, tr("bareme.csv.encoding"));
            for (e, key) in [
                (CsvEncoding::Utf8Bom, "bareme.csv.enc.utf8bom"),
                (CsvEncoding::Utf8, "bareme.csv.enc.utf8"),
                (CsvEncoding::Windows1252, "bareme.csv.enc.1252"),
            ] {
                if ui
                    .selectable_label(d.format.encoding == e, tr(key))
                    .clicked()
                {
                    d.format.encoding = e;
                }
            }
        });
        ui.checkbox(&mut d.clear_empty, tr("bareme.csv.clear_empty"));
        crate::a11y::label(ui, RichText::new(tr("bareme.csv.note")).color(secondary));
    }
    ui.add_space(4.0);
    ui.add_enabled_ui(range.is_some(), |ui| {
        ui.horizontal_wrapped(|ui| {
            if let Some(r) = range {
                if button(ui, &tr("bareme.csv.export"), ButtonKind::Secondary).clicked() {
                    actions.push(BAction::Export(r));
                }
                if button(ui, &tr("bareme.csv.import"), ButtonKind::Secondary).clicked() {
                    actions.push(BAction::Import(r));
                }
            }
        });
    });
    if !d.import_errors.is_empty() {
        ui.add_space(4.0);
        crate::a11y::label(
            ui,
            RichText::new(tr_args(
                "bareme.csv.refused",
                &[("count", &d.import_errors.len().to_string())],
            ))
            .color(danger)
            .strong(),
        );
        for line in &d.import_errors {
            crate::a11y::label(ui, RichText::new(line).color(danger));
        }
    }
}

enum CAction {
    Add,
    Select(ClientId),
    Rename(ClientId, String),
    Remove(ClientId),
    Set(ClientId, MeasurementId, f64),
    Clear(ClientId, MeasurementId),
    /// Génère (ou, confirmé, remplace) la taille sur-mesure d'une fiche.
    Generate(ClientId, SizeRangeId, String, bool),
}

impl App {
    fn clients_act(&mut self, d: &mut ClientsDialog, action: CAction) {
        d.notice = None;
        let result: Result<(), String> = match action {
            CAction::Generate(c, range, label, replace) => {
                d.gen_report = None;
                let label = label.trim().to_string();
                let exists = self.editor.project().size_range(range).is_ok_and(|r| {
                    r.sizes
                        .iter()
                        .any(|s| s.label.to_lowercase() == label.to_lowercase())
                });
                if exists && !replace {
                    // Remplacer une colonne existante demande une confirmation explicite.
                    d.gen_pending = Some(label);
                    d.error = None;
                    return;
                }
                d.gen_pending = None;
                let r = self.measure_exec(Command::GenerateClientSize {
                    client: c,
                    range,
                    label: label.clone(),
                    replace,
                });
                match r {
                    Ok(_) => {
                        let p = self.editor.project();
                        let missing: Vec<String> = p
                            .client_missing_measurements(c)
                            .unwrap_or_default()
                            .iter()
                            .filter_map(|m| p.measurement(*m).ok().map(|m| m.name.clone()))
                            .collect();
                        let filled = p.measurements.len() - missing.len();
                        d.gen_report = Some((label, filled, missing));
                        d.gen_label.clear();
                        Ok(())
                    }
                    Err(m) => Err(m),
                }
            }
            CAction::Select(c) => {
                d.selected = Some(c);
                d.error = None;
                return;
            }
            CAction::Add => {
                let r = self.measure_exec(Command::AddClient {
                    name: d.new_name.clone(),
                });
                match r {
                    Ok(out) => {
                        d.new_name.clear();
                        d.selected = out.clients.first().copied();
                        Ok(())
                    }
                    Err(m) => Err(m),
                }
            }
            CAction::Rename(c, name) => self
                .measure_exec(Command::RenameClient { client: c, name })
                .map(|_| ()),
            CAction::Remove(c) => {
                let r = self.measure_exec(Command::RemoveClient { client: c });
                if r.is_ok() && d.selected == Some(c) {
                    d.selected = None;
                }
                r.map(|_| ())
            }
            CAction::Set(c, m, v) => self
                .measure_exec(Command::SetClientValue {
                    client: c,
                    measurement: m,
                    value: v,
                })
                .map(|_| ()),
            CAction::Clear(c, m) => self
                .measure_exec(Command::ClearClientValue {
                    client: c,
                    measurement: m,
                })
                .map(|_| ()),
        };
        d.error = result.err();
    }

    pub fn clients_window(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.clients.take() else {
            return;
        };
        let project = self.editor.project().clone();
        let tokens = pf_ui::theme::tokens(ctx);
        let (danger, secondary) = (tokens.danger, tokens.text_secondary);
        // La fiche choisie doit exister encore (suppression, annulation).
        if d.selected.is_some_and(|c| project.client(c).is_err()) {
            d.selected = None;
        }
        let ranges: Vec<(SizeRangeId, String)> = project
            .sizes
            .iter()
            .map(|r| (r.id, r.name.clone()))
            .collect();
        d.range = pick_range(&ranges, d.range);
        let mut open = true;
        let mut close = false;
        let mut actions: Vec<CAction> = Vec::new();
        let slot = crate::dock::slot(ctx, &self.dock, ToolWin::Clients);
        let width = self.dock.content_width(ToolWin::Clients);
        egui::Window::new(tr("client.title"))
            .id(ToolWin::Clients.id())
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
                        crate::a11y::label(ui, RichText::new(tr("client.intro")).color(secondary));
                        ui.add_space(4.0);
                        if project.clients.is_empty() {
                            crate::a11y::label(
                                ui,
                                RichText::new(tr("client.none")).color(secondary),
                            );
                        }
                        ui.horizontal_wrapped(|ui| {
                            for c in &project.clients {
                                if ui
                                    .selectable_label(d.selected == Some(c.id), &c.name)
                                    .clicked()
                                {
                                    actions.push(CAction::Select(c.id));
                                }
                            }
                        });
                        ui.horizontal(|ui| {
                            crate::a11y::label(ui, tr("client.new"));
                            crate::a11y::add_text(
                                ui,
                                egui::TextEdit::singleline(&mut d.new_name)
                                    .id(egui::Id::new("client_new_name"))
                                    .desired_width(140.0),
                            );
                            if button(ui, &tr("client.add"), ButtonKind::Primary).clicked() {
                                actions.push(CAction::Add);
                            }
                        });
                        if let Some(text) = &d.error {
                            crate::a11y::label(ui, RichText::new(text).color(danger));
                        }
                        if let Some(client) = d.selected.and_then(|c| project.client(c).ok()) {
                            ui.add_space(6.0);
                            ui.separator();
                            client_card(
                                ui,
                                &mut d,
                                &project,
                                client,
                                &ranges,
                                secondary,
                                danger,
                                &mut actions,
                            );
                        }
                        ui.add_space(6.0);
                        close = button(ui, &tr("dialog.close"), ButtonKind::Secondary).clicked();
                    });
            });
        for a in actions {
            self.clients_act(&mut d, a);
        }
        if open && !close {
            self.clients = Some(d);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn client_card(
    ui: &mut egui::Ui,
    d: &mut ClientsDialog,
    project: &pf_core::Project,
    client: &pf_core::Client,
    ranges: &[(SizeRangeId, String)],
    secondary: egui::Color32,
    danger: egui::Color32,
    actions: &mut Vec<CAction>,
) {
    ui.horizontal(|ui| {
        crate::a11y::label(ui, tr("client.name"));
        if let Some(t) = edit(
            ui,
            &mut d.buffers,
            (K_CLIENT_NAME, client.id.0, 0),
            &client.name,
            180.0,
        ) {
            actions.push(CAction::Rename(client.id, t));
        }
        if button(ui, &tr("client.remove"), ButtonKind::Danger).clicked() {
            actions.push(CAction::Remove(client.id));
        }
    });
    ui.add_space(4.0);
    if project.measurements.is_empty() {
        crate::a11y::label(ui, RichText::new(tr("client.no_measures")).color(secondary));
        return;
    }
    let name_of = |m: MeasurementId| {
        project
            .measurement(m)
            .map(|x| x.name.clone())
            .unwrap_or_default()
    };
    ui.horizontal(|ui| {
        cell(ui, W_CLIENT_NAME, |ui| {
            crate::a11y::label(ui, RichText::new(tr("bareme.col.name")).color(secondary));
        });
        cell(ui, W_CLIENT_UNIT, |ui| {
            crate::a11y::label(ui, RichText::new(tr("bareme.col.unit")).color(secondary));
        });
        cell(ui, W_CELL + 10.0, |ui| {
            crate::a11y::label(ui, RichText::new(tr("client.col.value")).color(secondary));
        });
        crate::a11y::label(ui, RichText::new(tr("client.col.state")).color(secondary));
    });
    for m in &project.measurements {
        let current = client
            .values
            .iter()
            .find(|v| v.measurement == m.id)
            .map(|v| field_u(m.unit, v.value));
        ui.horizontal(|ui| {
            cell(ui, W_CLIENT_NAME, |ui| {
                crate::a11y::label(ui, &m.name);
            });
            cell(ui, W_CLIENT_UNIT, |ui| {
                crate::a11y::label(ui, unit_label(m.unit));
            });
            cell(ui, W_CELL + 10.0, |ui| {
                let committed = edit(
                    ui,
                    &mut d.buffers,
                    (K_CLIENT_VALUE, client.id.0, m.id.0),
                    current.as_deref().unwrap_or(""),
                    W_CELL - 2.0,
                );
                let Some(text) = committed else { return };
                if text.trim().is_empty() {
                    actions.push(CAction::Clear(client.id, m.id));
                } else {
                    match parse_value(m.unit, &text) {
                        Ok(v) => actions.push(CAction::Set(client.id, m.id, v)),
                        Err(_) => {
                            d.error = Some(tr_args("bareme.err.number", &[("text", &text)]));
                        }
                    }
                }
            });
            if current.is_none() {
                crate::a11y::label(
                    ui,
                    RichText::new(tr("client.missing")).color(danger).strong(),
                );
            } else {
                crate::a11y::label(ui, RichText::new(tr("client.set")).color(secondary));
            }
        });
    }
    ui.add_space(4.0);
    let missing = project
        .client_missing_measurements(client.id)
        .unwrap_or_default();
    if missing.is_empty() {
        crate::a11y::label(ui, tr("client.complete"));
    } else {
        let names: Vec<String> = missing.iter().map(|m| name_of(*m)).collect();
        crate::a11y::label(
            ui,
            RichText::new(tr_args(
                "client.missing_summary",
                &[
                    ("count", &missing.len().to_string()),
                    ("names", &names.join(", ")),
                ],
            ))
            .color(danger),
        );
    }
    ui.add_space(6.0);
    generate_section(ui, d, client, ranges, secondary, danger, actions);
    ui.add_space(6.0);
    section_header(ui, &mut d.proposal_open, tr("client.prop.section"));
    if !d.proposal_open {
        return;
    }
    if let Some(id) = range_selector(ui, ranges, d.range) {
        d.range = Some(id);
    }
    let Some(range) = d.range else {
        crate::a11y::label(ui, RichText::new(tr("bareme.no_sizes")).color(secondary));
        return;
    };
    let Ok(prop) = project.client_size_proposal(client.id, range) else {
        return;
    };
    let label_of = |s: SizeId| {
        project
            .find_size(s)
            .map(|(r, i)| r.sizes[i].label.clone())
            .unwrap_or_default()
    };
    match prop.nearest {
        Some(near) => {
            let score = prop
                .candidates
                .iter()
                .find(|c| c.size == near)
                .and_then(|c| c.score_mm)
                .unwrap_or(0.0);
            let mut text = pf_i18n::tr_len(
                "client.prop.nearest",
                &[
                    ("size", &label_of(near)),
                    ("score", &pf_core::units::len_number(score, 3)),
                ],
            );
            if !prop.tied.is_empty() {
                let tied: Vec<String> = prop.tied.iter().map(|s| label_of(*s)).collect();
                text.push_str(&tr_args("client.prop.tie", &[("sizes", &tied.join(", "))]));
            }
            crate::a11y::label(ui, RichText::new(text).strong());
        }
        None => {
            crate::a11y::label(ui, RichText::new(tr("client.prop.none")).color(secondary));
        }
    }
    for c in &prop.candidates {
        let text = match c.score_mm {
            Some(s) => pf_i18n::tr_len(
                "client.prop.cand",
                &[
                    ("size", &label_of(c.size)),
                    ("score", &pf_core::units::len_number(s, 3)),
                ],
            ),
            None => tr_args(
                "client.prop.cand_none",
                &[
                    ("size", &label_of(c.size)),
                    ("count", &c.missing_in_size.len().to_string()),
                ],
            ),
        };
        crate::a11y::label(ui, text);
    }
    if let Some(c) = prop
        .nearest
        .and_then(|n| prop.candidates.iter().find(|c| c.size == n))
    {
        ui.add_space(4.0);
        crate::a11y::label(
            ui,
            RichText::new(tr_args(
                "client.prop.gap_head",
                &[("size", &label_of(c.size))],
            ))
            .strong(),
        );
        for g in &c.gaps {
            let sign = if g.gap > 0.0 { "+" } else { "" };
            crate::a11y::label(
                ui,
                tr_args(
                    "client.prop.gap_row",
                    &[
                        ("name", &name_of(g.measurement)),
                        (
                            "client",
                            &show_u(unit_of(project, g.measurement), g.client_value),
                        ),
                        (
                            "size",
                            &show_u(unit_of(project, g.measurement), g.size_value),
                        ),
                        (
                            "gap",
                            &format!("{sign}{}", show_u(unit_of(project, g.measurement), g.gap)),
                        ),
                    ],
                ),
            );
        }
    }
    ui.add_space(4.0);
    crate::a11y::label(ui, RichText::new(tr("client.prop.info")).color(secondary));
}

/// Section « Générer une taille… » : crée (ou, après confirmation, remplace) une taille de la gamme
/// dont la colonne du barème est celle de la fiche. Explicite : rien ne suit la fiche ensuite.
fn generate_section(
    ui: &mut egui::Ui,
    d: &mut ClientsDialog,
    client: &pf_core::Client,
    ranges: &[(SizeRangeId, String)],
    secondary: egui::Color32,
    danger: egui::Color32,
    actions: &mut Vec<CAction>,
) {
    crate::a11y::label(ui, RichText::new(tr("client.gen.title")).strong());
    crate::a11y::label(ui, RichText::new(tr("client.gen.info")).color(secondary));
    let Some(range) = d.range else {
        crate::a11y::label(ui, RichText::new(tr("bareme.no_sizes")).color(secondary));
        return;
    };
    if let Some(id) = range_selector(ui, ranges, d.range) {
        d.range = Some(id);
    }
    ui.horizontal(|ui| {
        crate::a11y::label(ui, tr("client.gen.name"));
        crate::a11y::add_text(
            ui,
            egui::TextEdit::singleline(&mut d.gen_label)
                .id(egui::Id::new("client_gen_label"))
                .desired_width(120.0),
        );
        if button(ui, &tr("client.gen.button"), ButtonKind::Primary).clicked() {
            actions.push(CAction::Generate(
                client.id,
                range,
                d.gen_label.clone(),
                false,
            ));
        }
    });
    if let Some(label) = d.gen_pending.clone() {
        if label.to_lowercase() == d.gen_label.trim().to_lowercase() {
            crate::a11y::label(
                ui,
                RichText::new(tr_args("client.gen.confirm", &[("size", &label)])).color(danger),
            );
            if button(ui, &tr("client.gen.replace"), ButtonKind::Danger).clicked() {
                actions.push(CAction::Generate(client.id, range, label, true));
            }
        }
    }
    if let Some((label, filled, missing)) = &d.gen_report {
        crate::a11y::label(
            ui,
            tr_args(
                "client.gen.done",
                &[("size", label), ("count", &filled.to_string())],
            ),
        );
        if missing.is_empty() {
            crate::a11y::label(
                ui,
                RichText::new(tr("client.gen.complete")).color(secondary),
            );
        } else {
            crate::a11y::label(
                ui,
                RichText::new(tr_args(
                    "client.gen.missing",
                    &[
                        ("count", &missing.len().to_string()),
                        ("names", &missing.join(", ")),
                    ],
                ))
                .color(danger),
            );
        }
    }
}

/// Projet de démonstration des étapes `bareme` et `fiche-client` : gamme XS à XL (base M), trois
/// mesures (valeurs d'exemple sans valeur métier : jamais présentées comme validées), une cellule
/// laissée vide, et la fiche « Camille » dont l'angle d'épaule est manquant. Renvoie la fiche.
pub(crate) fn demo_project() -> Result<(pf_core::Project, ClientId), pf_core::CoreError> {
    use pf_core::SizeKind;
    let mut p = pf_core::Project::new("Measurement table");
    p.apply(Command::AddSizeRange {
        name: "Women".into(),
        kind: SizeKind::Alphanumeric,
        labels: ["XS", "S", "M", "L", "XL"].map(String::from).to_vec(),
        base: 2,
    })?;
    let sizes: Vec<SizeId> = p.sizes[0].sizes.iter().map(|s| s.id).collect();
    let rows: [(&str, MeasurementUnit, [Option<f64>; 5]); 3] = [
        (
            "Bust",
            MeasurementUnit::Mm,
            [
                Some(800.0),
                Some(840.0),
                Some(880.0),
                Some(920.0),
                Some(960.0),
            ],
        ),
        (
            "Back length",
            MeasurementUnit::Mm,
            [
                Some(400.0),
                Some(405.0),
                Some(410.0),
                Some(415.0),
                Some(420.0),
            ],
        ),
        (
            "Shoulder angle",
            MeasurementUnit::Deg,
            [None, Some(20.0), Some(20.5), Some(21.0), Some(21.5)],
        ),
    ];
    let mut ids = Vec::new();
    for (name, unit, values) in rows {
        let m = p
            .apply(Command::AddMeasurement {
                name: name.into(),
                unit,
            })?
            .measurements[0];
        ids.push(m);
        for (s, v) in sizes.iter().zip(values) {
            if let Some(value) = v {
                p.apply(Command::SetMeasurementValue {
                    measurement: m,
                    size: *s,
                    value,
                })?;
            }
        }
    }
    let client = p
        .apply(Command::AddClient {
            name: "Camille".into(),
        })?
        .clients[0];
    for (m, value) in [(ids[0], 905.0), (ids[1], 405.0)] {
        p.apply(Command::SetClientValue {
            client,
            measurement: m,
            value,
        })?;
    }
    Ok((p, client))
}
