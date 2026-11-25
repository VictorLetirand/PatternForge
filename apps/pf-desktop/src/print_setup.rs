//! Réglages de sortie du PDF (US-13.1) : format de page, orientation, marges imprimante,
//! rouleau. Ils sont enregistrés dans les **préférences de l'application** (clé
//! `print_setup` de `settings.json`), jamais dans le projet : le papier et l'imprimante
//! dépendent de la machine, pas du patron. Le fichier `.pforge.json` n'en garde aucune trace.
//!
//! La fenêtre ne calcule rien : validation, bornes et mise en page viennent de
//! `pf_export::page` (`PageSetup::validate`, `plan_tiles_with`). L'échelle reste 1:1 ; un
//! contenu trop grand est découpé en pages, jamais réduit. Aucun essai sur papier n'a été
//! fait : l'échelle réelle reste à valider avec l'étalon (AT-09).
//!
//! Assemblage de pages (US-13.2) : recouvrement et options (repères, plan, économie de papier)
//! sont dans le même réglage `print_setup`. Un ancien fichier sans ces champs se charge avec les
//! valeurs par défaut (10 mm, toutes les options désactivées).
//!
//! Étalonnage de l'imprimante (US-13.3) : les profils nommés (facteurs X / Y issus d'une mesure
//! du carré étalon, 16 au plus) et le profil actif sont dans la même clé `print_setup`
//! (`profiles`, `active_profile`, `control_marks`) ; un ancien fichier sans ces champs se charge
//! sans correction. Rien n'est écrit dans le projet. Aucun texte ne dit que l'imprimante est
//! calibrée : la mesure sur papier reste humaine (AT-09).

use crate::app::App;
use crate::palette::pal;
use eframe::egui::{self, RichText};
use pf_core::input::format_number;
use pf_core::units::parse_length_display;

/// Millimètres sans décimale inutile : « 210 », « 215,9 ».
pub(crate) fn mm(v: f64) -> String {
    mm_dec(v, 2)
}

/// Longueur de la mise en page dans l'unité d'affichage, sans suffixe (gabarits `tr_len`) ;
/// mm : comme `mm`.
fn ml(v: f64) -> String {
    if pf_core::units::display_units().unit == pf_core::units::LengthUnit::Mm {
        mm(v)
    } else {
        pf_core::units::len_number(v, 2)
    }
}

/// Texte exact d'un champ de longueur de la mise en page (mm : sans décimale inutile).
fn field(v: f64) -> String {
    if pf_core::units::display_units().unit == pf_core::units::LengthUnit::Mm {
        mm_dec(v, 3)
    } else {
        pf_core::units::field_text(v)
    }
}

fn mm_dec(v: f64, decimals: usize) -> String {
    if v.is_finite() && (v - v.round()).abs() < 1e-9 && v.abs() < 1e12 {
        format!("{}", v.round() as i64)
    } else {
        format_number(v, decimals)
    }
}

use pf_export::calibration::{
    Axis, ProfileError, ScaleError, MAX_PROFILES, PROFILE_NAME_MAX_CHARS, REFERENCE_MM, SCALE_MAX,
    SCALE_MIN,
};
use pf_export::content::count as category_count;
use pf_export::page::{DEFAULT_OVERLAP_MM, OVERLAP_MAX_MM, OVERLAP_MIN_STEP_MM};
use pf_export::pdf;
use pf_export::{
    AssemblyOptions, ContentNote, ExportError, Margins, Orientation, OutputCategory, OutputContent,
    OutputSize, PageField, PageFormat, PageSetup, PageSetupError, PieceExportMode, PrintScale,
    PrinterProfiles, Scene, SceneOptions,
};
use pf_i18n::{tr, tr_args};
use serde_json::{json, Value};

/// Clé du réglage dans `settings.json`.
pub const SETTINGS_KEY: &str = "print_setup";

/// Clé du contenu de la sortie (US-13.4) dans `settings.json` : préférence de l'application,
/// jamais dans le projet. Absente (ancien fichier) : tout est inclus.
pub const CONTENT_KEY: &str = "output_content";

/// Contenu de sortie lu d'une valeur enregistrée. Absent, mal formé ou contenant une valeur
/// inconnue : tout est inclus (on n'écarte jamais du contenu sur la foi d'une valeur douteuse).
pub fn content_from_json(value: &Value) -> OutputContent {
    parse_content(value).unwrap_or_default()
}

fn parse_content(value: &Value) -> Option<OutputContent> {
    let obj = value.as_object()?;
    let mut content = OutputContent::default();
    if let Some(size) = obj.get("size") {
        content.size = OutputSize::from_key(size.as_str()?)?;
    }
    if let Some(mode) = obj.get("piece_mode") {
        content.piece_mode = PieceExportMode::from_key(mode.as_str()?)?;
    }
    if let Some(list) = obj.get("excluded") {
        for item in list.as_array()? {
            content.set(OutputCategory::from_key(item.as_str()?)?, false);
        }
    }
    Some(content)
}

pub fn content_to_json(content: &OutputContent) -> Value {
    let mut value = json!({
        "size": content.size.key(),
        "excluded": content.excluded().iter().map(|c| c.key()).collect::<Vec<_>>(),
    });
    // Écrit seulement quand il change quelque chose : un fichier d'avant US-05.4 est inchangé.
    if content.piece_mode != PieceExportMode::default() {
        value["piece_mode"] = json!(content.piece_mode.key());
    }
    value
}

/// Contenu enregistré dans le fichier de réglages : absent ou illisible = tout inclus.
/// Ne modifie jamais le fichier.
pub fn load_output_content(path: &std::path::Path) -> OutputContent {
    pf_i18n::settings::load_value(path, CONTENT_KEY)
        .map(|v| content_from_json(&v))
        .unwrap_or_default()
}

/// Réglage enregistré. Toute valeur absente, mal formée ou hors bornes redonne le réglage
/// par défaut (A4 portrait, marges de 10 mm) : un réglage invalide n'est jamais appliqué.
pub fn from_json(value: &Value) -> PageSetup {
    parse_json(value).unwrap_or_default()
}

fn parse_json(value: &Value) -> Option<PageSetup> {
    let num = |v: &Value| v.as_f64().filter(|x| x.is_finite());
    let format = match value.get("format")?.as_str()? {
        "a4" => PageFormat::A4,
        "a3" => PageFormat::A3,
        "a0" => PageFormat::A0,
        "letter" => PageFormat::Letter,
        "custom" => PageFormat::Custom {
            width_mm: num(value.get("width_mm")?)?,
            height_mm: num(value.get("height_mm")?)?,
        },
        "roll" => PageFormat::Roll {
            width_mm: num(value.get("width_mm")?)?,
        },
        _ => return None,
    };
    let orientation = match value.get("orientation")?.as_str()? {
        "portrait" => Orientation::Portrait,
        "landscape" => Orientation::Landscape,
        _ => return None,
    };
    let m = value.get("margins_mm")?;
    let margins = Margins {
        top_mm: num(m.get("top")?)?,
        bottom_mm: num(m.get("bottom")?)?,
        left_mm: num(m.get("left")?)?,
        right_mm: num(m.get("right")?)?,
    };
    // Champs d'US-13.2 : absents d'un ancien fichier = valeurs par défaut ; présents mais mal
    // formés = tout le réglage est ignoré (même règle que les autres champs).
    let overlap_mm = match value.get("overlap_mm") {
        None => DEFAULT_OVERLAP_MM,
        Some(v) => num(v)?,
    };
    let assembly = match value.get("assembly") {
        None => AssemblyOptions::default(),
        Some(a) => {
            let a = a.as_object()?;
            let flag = |key: &str| match a.get(key) {
                None => Some(false),
                Some(v) => v.as_bool(),
            };
            AssemblyOptions {
                marks: flag("marks")?,
                plan: flag("plan")?,
                save_paper: flag("save_paper")?,
            }
        }
    };
    let control_marks = match value.get("control_marks") {
        None => false,
        Some(v) => v.as_bool()?,
    };
    let setup = PageSetup {
        format,
        orientation,
        margins,
        overlap_mm,
        assembly,
        // La correction vient du profil actif (jamais d'un facteur isolé dans le fichier).
        scale: profiles_from_json(value).active_scale(),
        control_marks,
    };
    setup.validate().ok()?;
    Some(setup)
}

/// Profils d'imprimante lus de la valeur `print_setup` : un profil mal formé, invalide (nom,
/// facteurs hors bornes) ou en double est écarté — jamais appliqué ; au plus 16 sont gardés.
pub fn profiles_from_json(value: &Value) -> PrinterProfiles {
    let items: Vec<(String, f64, f64)> = value
        .get("profiles")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(|p| {
                    Some((
                        p.get("name")?.as_str()?.to_string(),
                        p.get("x")?.as_f64()?,
                        p.get("y")?.as_f64()?,
                    ))
                })
                .collect()
        })
        .unwrap_or_default();
    PrinterProfiles::from_parts(items, value.get("active_profile").and_then(Value::as_str))
}

/// Profils enregistrés dans le fichier de réglages : absents ou illisibles = aucun profil.
/// Ne modifie jamais le fichier.
pub fn load_profiles(path: &std::path::Path) -> PrinterProfiles {
    pf_i18n::settings::load_value(path, SETTINGS_KEY)
        .map(|v| profiles_from_json(&v))
        .unwrap_or_default()
}

/// Valeur complète de la clé `print_setup` : réglage de page et profils d'imprimante.
pub fn settings_value(setup: &PageSetup, profiles: &PrinterProfiles) -> Value {
    let mut v = to_json(setup);
    if !profiles.list().is_empty() {
        v["profiles"] = Value::Array(
            profiles
                .list()
                .iter()
                .map(|p| json!({"name": p.name, "x": p.scale.x, "y": p.scale.y}))
                .collect(),
        );
    }
    if let Some(active) = profiles.active() {
        v["active_profile"] = json!(active.name);
    }
    v
}

pub fn to_json(setup: &PageSetup) -> Value {
    let mut v = match setup.format {
        PageFormat::A4 => json!({"format": "a4"}),
        PageFormat::A3 => json!({"format": "a3"}),
        PageFormat::A0 => json!({"format": "a0"}),
        PageFormat::Letter => json!({"format": "letter"}),
        PageFormat::Custom {
            width_mm,
            height_mm,
        } => json!({"format": "custom", "width_mm": width_mm, "height_mm": height_mm}),
        PageFormat::Roll { width_mm } => json!({"format": "roll", "width_mm": width_mm}),
    };
    v["orientation"] = json!(match setup.orientation {
        Orientation::Portrait => "portrait",
        Orientation::Landscape => "landscape",
    });
    v["margins_mm"] = json!({
        "top": setup.margins.top_mm,
        "bottom": setup.margins.bottom_mm,
        "left": setup.margins.left_mm,
        "right": setup.margins.right_mm,
    });
    v["overlap_mm"] = json!(setup.overlap_mm);
    v["assembly"] = json!({
        "marks": setup.assembly.marks,
        "plan": setup.assembly.plan,
        "save_paper": setup.assembly.save_paper,
    });
    v["control_marks"] = json!(setup.control_marks);
    v
}

/// Libellé du réglage dans la langue de l'interface : « A4 », « A3 paysage », « rouleau 914 mm ».
pub fn setup_label(setup: &PageSetup) -> String {
    let base = match setup.format {
        PageFormat::A4 => "A4".to_string(),
        PageFormat::A3 => "A3".to_string(),
        PageFormat::A0 => "A0".to_string(),
        PageFormat::Letter => "Letter".to_string(),
        PageFormat::Custom {
            width_mm,
            height_mm,
        } => pf_i18n::tr_len(
            "print.label.custom",
            &[("w", &ml(width_mm)), ("h", &ml(height_mm))],
        ),
        PageFormat::Roll { width_mm } => {
            pf_i18n::tr_len("print.label.roll", &[("w", &ml(width_mm))])
        }
    };
    if setup.orientation == Orientation::Landscape && !setup.format.is_roll() {
        tr_args("print.label.landscape", &[("label", &base)])
    } else {
        base
    }
}

fn field_name(field: PageField) -> String {
    match field {
        PageField::Width => tr("print.field.width"),
        PageField::Height => tr("print.field.height"),
        PageField::RollWidth => tr("print.field.roll_width"),
        PageField::MarginTop => tr("print.margin.top"),
        PageField::MarginBottom => tr("print.margin.bottom"),
        PageField::MarginLeft => tr("print.margin.left"),
        PageField::MarginRight => tr("print.margin.right"),
        PageField::Overlap => tr("print.overlap"),
    }
}

/// Refus de réglage ou de mise en page, dans la langue de l'interface.
pub fn page_error(e: &PageSetupError) -> String {
    let n = ml;
    match e {
        PageSetupError::NotFinite(field) => {
            tr_args("print.err.not_finite", &[("field", &field_name(*field))])
        }
        PageSetupError::SideOutOfRange { field, value_mm } => pf_i18n::tr_len(
            "print.err.side",
            &[
                ("field", &field_name(*field)),
                ("value", &n(*value_mm)),
                ("min", &n(pf_export::page::PAGE_SIDE_MIN_MM)),
                ("max", &n(pf_export::page::PAGE_SIDE_MAX_MM)),
            ],
        ),
        PageSetupError::MarginOutOfRange { field, value_mm } => pf_i18n::tr_len(
            "print.err.margin",
            &[
                ("field", &field_name(*field)),
                ("value", &n(*value_mm)),
                ("max", &n(pf_export::page::MARGIN_MAX_MM)),
            ],
        ),
        PageSetupError::PrintableTooSmall {
            width_mm,
            height_mm,
        } => pf_i18n::tr_len(
            "print.err.printable",
            &[
                ("w", &n(*width_mm)),
                (
                    "h",
                    &if height_mm.is_finite() {
                        n(*height_mm)
                    } else {
                        "—".into()
                    },
                ),
                ("min_w", &n(pf_export::page::PRINTABLE_MIN_WIDTH_MM)),
                ("min_h", &n(pf_export::page::PRINTABLE_MIN_HEIGHT_MM)),
            ],
        ),
        PageSetupError::RollTooLong { length_mm } => pf_i18n::tr_len(
            "print.err.roll_long",
            &[
                ("len", &n(*length_mm)),
                ("max", &n(pf_export::page::ROLL_LENGTH_MAX_MM)),
            ],
        ),
        PageSetupError::ContentTooWide {
            content_mm,
            printable_mm,
        } => pf_i18n::tr_len(
            "print.err.too_wide",
            &[
                ("content", &n(*content_mm)),
                ("printable", &n(*printable_mm)),
            ],
        ),
        PageSetupError::OverlapOutOfRange { value_mm } => pf_i18n::tr_len(
            "print.err.overlap",
            &[
                ("field", &field_name(PageField::Overlap)),
                ("value", &n(*value_mm)),
                ("max", &n(OVERLAP_MAX_MM)),
            ],
        ),
        PageSetupError::OverlapTooLarge {
            overlap_mm,
            window_mm,
        } => pf_i18n::tr_len(
            "print.err.overlap_large",
            &[
                ("overlap", &n(*overlap_mm)),
                ("window", &n(*window_mm)),
                ("min", &n(OVERLAP_MIN_STEP_MM)),
            ],
        ),
        PageSetupError::Scale(e) => scale_error(e),
    }
}

/// Facteur écrit sans zéros inutiles (6 décimales au plus).
pub fn factor_text(v: f64) -> String {
    format_number(v, 6)
}

fn axis_name(axis: Axis) -> &'static str {
    axis.label()
}

/// Refus d'un facteur ou d'une mesure, dans la langue de l'interface.
pub fn scale_error(e: &ScaleError) -> String {
    match e {
        ScaleError::NotFinite(axis) => {
            tr_args("calib.err.not_finite", &[("axis", axis_name(*axis))])
        }
        ScaleError::OutOfRange { axis, value } => tr_args(
            "calib.err.range",
            &[
                ("axis", axis_name(*axis)),
                ("value", &factor_text(*value)),
                ("min", &factor_text(SCALE_MIN)),
                ("max", &factor_text(SCALE_MAX)),
                ("mmin", &mm(REFERENCE_MM / SCALE_MAX)),
                ("mmax", &mm(REFERENCE_MM / SCALE_MIN)),
            ],
        ),
        ScaleError::BadMeasure { axis, value_mm } => tr_args(
            "calib.err.measure",
            &[("axis", axis_name(*axis)), ("value", &mm(*value_mm))],
        ),
    }
}

/// Refus d'un nom ou d'une liste de profils, dans la langue de l'interface.
pub fn profile_error(e: &ProfileError) -> String {
    match e {
        ProfileError::NameEmpty => tr("calib.err.name_empty"),
        ProfileError::NameTooLong { max } => {
            tr_args("calib.err.name_long", &[("max", &max.to_string())])
        }
        ProfileError::NameControlChar => tr("calib.err.name_control"),
        ProfileError::Duplicate => tr("calib.err.duplicate"),
        ProfileError::TooMany { max } => {
            tr_args("calib.err.too_many", &[("max", &max.to_string())])
        }
        ProfileError::Unknown => tr("calib.err.unknown"),
        ProfileError::Scale(e) => scale_error(e),
    }
}

/// Réglage lu dans le fichier de réglages : absent, illisible ou invalide = réglage par défaut.
/// Ne modifie jamais le fichier.
pub fn load_page_setup(path: &std::path::Path) -> PageSetup {
    pf_i18n::settings::load_value(path, SETTINGS_KEY)
        .map(|v| from_json(&v))
        .unwrap_or_default()
}

/// Message d'un échec d'export (réglage de page traduit, le reste tel que le cœur le dit).
pub fn export_error(e: &ExportError) -> String {
    match e {
        ExportError::Page(p) => tr_args("error.core", &[("detail", &page_error(p))]),
        ExportError::NothingSelected(name) => tr_args("export.nothing_selected", &[("name", name)]),
        ExportError::NotFinite(name) => tr_args("export.not_finite", &[("name", name)]),
        other => crate::text::core_error(other),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormatKind {
    A4,
    A3,
    A0,
    Letter,
    Custom,
    Roll,
}

/// Saisie en cours dans la fenêtre (état d'interface : rien n'est appliqué avant « Appliquer »).
#[derive(Debug, Clone, PartialEq)]
pub struct PrintDialog {
    pub kind: FormatKind,
    pub width: String,
    pub height: String,
    pub roll_width: String,
    pub orientation: Orientation,
    /// Haut, bas, gauche, droite.
    pub margins: [String; 4],
    /// Recouvrement entre pages voisines (mm), saisi.
    pub overlap: String,
    /// Options d'assemblage (US-13.2).
    pub marks: bool,
    pub plan: bool,
    pub save_paper: bool,
    /// Étalon renforcé (US-13.3).
    pub control_marks: bool,
    /// Correction du profil actif, relue des profils à chaque image : elle n'est pas saisie ici.
    pub scale: PrintScale,
    /// Contenu de la sortie (US-13.4), saisi ici et adopté avec « Appliquer ».
    pub content: OutputContent,
}

impl PrintDialog {
    /// Même saisie avec le contenu de sortie donné.
    pub fn with_content(mut self, content: OutputContent) -> PrintDialog {
        self.content = content;
        self
    }

    pub fn from_setup(setup: &PageSetup) -> PrintDialog {
        let text = field;
        let (kind, width, height, roll_width) = match setup.format {
            PageFormat::A4 => (FormatKind::A4, 300.0, 400.0, 914.0),
            PageFormat::A3 => (FormatKind::A3, 300.0, 400.0, 914.0),
            PageFormat::A0 => (FormatKind::A0, 300.0, 400.0, 914.0),
            PageFormat::Letter => (FormatKind::Letter, 300.0, 400.0, 914.0),
            PageFormat::Custom {
                width_mm,
                height_mm,
            } => (FormatKind::Custom, width_mm, height_mm, 914.0),
            PageFormat::Roll { width_mm } => (FormatKind::Roll, 300.0, 400.0, width_mm),
        };
        let m = setup.margins;
        PrintDialog {
            kind,
            width: text(width),
            height: text(height),
            roll_width: text(roll_width),
            orientation: setup.orientation,
            margins: [m.top_mm, m.bottom_mm, m.left_mm, m.right_mm].map(text),
            overlap: text(setup.overlap_mm),
            marks: setup.assembly.marks,
            plan: setup.assembly.plan,
            save_paper: setup.assembly.save_paper,
            control_marks: setup.control_marks,
            scale: setup.scale,
            content: OutputContent::default(),
        }
    }

    /// Réglage décrit par la saisie ; message traduit si un champ n'est pas un nombre ou si le
    /// réglage est hors bornes.
    pub fn to_setup(&self) -> Result<PageSetup, String> {
        let number = |text: &str, field: PageField| -> Result<f64, String> {
            parse_length_display(text)
                .map_err(|_| tr_args("print.err.number", &[("field", &field_name(field))]))
        };
        let format = match self.kind {
            FormatKind::A4 => PageFormat::A4,
            FormatKind::A3 => PageFormat::A3,
            FormatKind::A0 => PageFormat::A0,
            FormatKind::Letter => PageFormat::Letter,
            FormatKind::Custom => PageFormat::Custom {
                width_mm: number(&self.width, PageField::Width)?,
                height_mm: number(&self.height, PageField::Height)?,
            },
            FormatKind::Roll => PageFormat::Roll {
                width_mm: number(&self.roll_width, PageField::RollWidth)?,
            },
        };
        let margins = Margins {
            top_mm: number(&self.margins[0], PageField::MarginTop)?,
            bottom_mm: number(&self.margins[1], PageField::MarginBottom)?,
            left_mm: number(&self.margins[2], PageField::MarginLeft)?,
            right_mm: number(&self.margins[3], PageField::MarginRight)?,
        };
        let setup = PageSetup {
            format,
            orientation: self.orientation,
            margins,
            overlap_mm: number(&self.overlap, PageField::Overlap)?,
            assembly: AssemblyOptions {
                marks: self.marks,
                plan: self.plan,
                save_paper: self.save_paper,
            },
            scale: self.scale,
            control_marks: self.control_marks,
        };
        setup.validate().map_err(|e| page_error(&e))?;
        Ok(setup)
    }
}

impl App {
    /// Ouvre la fenêtre des réglages d'impression, pré-remplie avec le réglage courant.
    pub fn open_print_setup(&mut self) {
        self.print_dialog =
            Some(PrintDialog::from_setup(&self.page_setup).with_content(self.output_content));
    }

    /// Adopte un contenu de sortie et le mémorise hors projet. Un échec d'écriture n'adopte
    /// rien (refus sans trace en mémoire) et le dit dans la barre d'état.
    pub fn apply_output_content(&mut self, content: OutputContent) {
        if content == self.output_content {
            return;
        }
        if let Some(path) = &self.settings_path {
            if let Err(e) =
                pf_i18n::settings::save_value(path, CONTENT_KEY, content_to_json(&content))
            {
                self.editor.status = crate::editor::Status {
                    text: tr_args(
                        "status.print_setup.save_failed",
                        &[("detail", &e.to_string())],
                    ),
                    error: true,
                };
                return;
            }
        }
        self.output_content = content;
    }

    /// Applique un réglage déjà validé et le mémorise hors projet.
    pub fn apply_page_setup(&mut self, mut setup: PageSetup) {
        // La correction d'échelle vient toujours du profil actif.
        setup.scale = self.printer_profiles.active_scale();
        self.page_setup = setup;
        let label = setup_label(&setup);
        let mut text = tr_args("status.print_setup.saved", &[("label", &label)]);
        let mut error = false;
        if let Some(path) = &self.settings_path {
            let value = settings_value(&setup, &self.printer_profiles);
            if let Err(e) = pf_i18n::settings::save_value(path, SETTINGS_KEY, value) {
                text = tr_args(
                    "status.print_setup.save_failed",
                    &[("detail", &e.to_string())],
                );
                error = true;
            }
        }
        self.editor.status = crate::editor::Status { text, error };
    }

    pub fn print_setup_window(&mut self, ctx: &egui::Context) {
        let Some(mut dialog) = self.print_dialog.take() else {
            return;
        };
        let (mut open, mut apply, mut reset, mut open_calib) = (true, false, false, false);
        let (mut open_print, mut open_projection) = (false, false);
        let mut chosen_size = None;
        dialog.scale = self.printer_profiles.active_scale();
        // La fenêtre tient dans l'écran : le contenu défile, les boutons restent visibles dessous.
        let room = crate::panels::window_room(ctx);
        let mut valid = false;
        egui::Window::new(tr("print.title"))
            .id(egui::Id::new("print_setup_window"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_TOP, [0.0, crate::panels::WINDOW_TOP])
            .default_height(room)
            .show(ctx, |ui| {
                ui.spacing_mut().scroll = egui::style::ScrollStyle::solid();
                egui::ScrollArea::vertical()
                    .id_salt("print-scroll")
                    .max_height(room - 110.0)
                    .auto_shrink([true, true])
                    .show(ui, |ui| {
                        // US-09.8 : taille exportée (la base par défaut), mention et sélecteur.
                        if let Some(size) = self.export_size_section(ui, false) {
                            chosen_size = Some(size);
                        }
                        crate::a11y::label(
                            ui,
                            RichText::new(tr("print.intro")).small().color(pal().muted),
                        );
                        ui.horizontal_wrapped(|ui| {
                            crate::a11y::label(ui, tr("print.format"));
                            for (kind, label) in [
                                (FormatKind::A4, tr("print.format.a4")),
                                (FormatKind::A3, tr("print.format.a3")),
                                (FormatKind::A0, tr("print.format.a0")),
                                (FormatKind::Letter, tr("print.format.letter")),
                                (FormatKind::Custom, tr("print.format.custom")),
                                (FormatKind::Roll, tr("print.format.roll")),
                            ] {
                                ui.selectable_value(&mut dialog.kind, kind, label);
                            }
                        });
                        egui::Grid::new("print-grid").num_columns(2).show(ui, |ui| {
                            let custom = dialog.kind == FormatKind::Custom;
                            crate::a11y::label(ui, tr("print.field.width"));
                            crate::a11y::add_text_enabled(
                                ui,
                                custom,
                                egui::TextEdit::singleline(&mut dialog.width)
                                    .id_salt("print-width")
                                    .desired_width(90.0),
                            );
                            ui.end_row();
                            crate::a11y::label(ui, tr("print.field.height"));
                            crate::a11y::add_text_enabled(
                                ui,
                                custom,
                                egui::TextEdit::singleline(&mut dialog.height)
                                    .id_salt("print-height")
                                    .desired_width(90.0),
                            );
                            ui.end_row();
                            crate::a11y::label(ui, tr("print.field.roll_width"));
                            crate::a11y::add_text_enabled(
                                ui,
                                dialog.kind == FormatKind::Roll,
                                egui::TextEdit::singleline(&mut dialog.roll_width)
                                    .id_salt("print-roll")
                                    .desired_width(90.0),
                            );
                            ui.end_row();
                            crate::a11y::label(ui, tr("print.orientation"));
                            ui.add_enabled_ui(dialog.kind != FormatKind::Roll, |ui| {
                                ui.horizontal(|ui| {
                                    ui.selectable_value(
                                        &mut dialog.orientation,
                                        Orientation::Portrait,
                                        tr("print.portrait"),
                                    );
                                    ui.selectable_value(
                                        &mut dialog.orientation,
                                        Orientation::Landscape,
                                        tr("print.landscape"),
                                    );
                                });
                            });
                            ui.end_row();
                            for (i, label) in [
                                tr("print.margin.top"),
                                tr("print.margin.bottom"),
                                tr("print.margin.left"),
                                tr("print.margin.right"),
                            ]
                            .into_iter()
                            .enumerate()
                            {
                                crate::a11y::label(ui, label);
                                crate::a11y::add_text(
                                    ui,
                                    egui::TextEdit::singleline(&mut dialog.margins[i])
                                        .id_salt(("print-margin", i))
                                        .desired_width(90.0),
                                );
                                ui.end_row();
                            }
                            crate::a11y::label(ui, tr("print.overlap"));
                            crate::a11y::add_text_enabled(
                                ui,
                                dialog.kind != FormatKind::Roll,
                                egui::TextEdit::singleline(&mut dialog.overlap)
                                    .id_salt("print-overlap")
                                    .desired_width(90.0),
                            );
                            ui.end_row();
                        });
                        if dialog.kind == FormatKind::Roll {
                            crate::a11y::label(
                                ui,
                                RichText::new(tr("print.roll_note"))
                                    .small()
                                    .color(pal().muted),
                            );
                        }
                        ui.separator();
                        crate::a11y::label(ui, RichText::new(tr("print.assembly.title")).strong());
                        ui.add_enabled_ui(dialog.kind != FormatKind::Roll, |ui| {
                            ui.checkbox(&mut dialog.marks, tr("print.assembly.marks"));
                            ui.checkbox(&mut dialog.plan, tr("print.assembly.plan"));
                            ui.checkbox(&mut dialog.save_paper, tr("print.assembly.save"));
                        });
                        crate::a11y::label(
                            ui,
                            RichText::new(tr("print.assembly.note"))
                                .small()
                                .color(pal().muted),
                        );
                        ui.separator();
                        self.content_section(ui, &mut dialog.content);
                        ui.separator();
                        match self.printer_profiles.active() {
                            Some(p) => crate::a11y::label(
                                ui,
                                tr_args(
                                    "print.profile.active",
                                    &[
                                        ("name", &p.name),
                                        ("x", &factor_text(p.scale.x)),
                                        ("y", &factor_text(p.scale.y)),
                                    ],
                                ),
                            ),
                            None => crate::a11y::label(ui, tr("print.profile.none")),
                        };
                        ui.horizontal(|ui| {
                            ui.checkbox(&mut dialog.control_marks, tr("print.control"));
                            open_calib = ui.button(tr("print.profile.open")).clicked();
                        });
                        ui.separator();
                        crate::a11y::label(ui, RichText::new(tr("print.preview.title")).strong());
                        valid = match dialog.to_setup() {
                            Ok(setup) => {
                                self.print_preview_ui(ui, &setup, &dialog.content);
                                true
                            }
                            Err(msg) => {
                                crate::a11y::label(ui, RichText::new(msg).color(pal().error));
                                false
                            }
                        };
                        crate::a11y::label(
                            ui,
                            RichText::new(tr("print.scale_note"))
                                .small()
                                .color(pal().muted),
                        );
                    });
                ui.separator();
                ui.horizontal(|ui| {
                    apply = ui
                        .add_enabled(valid, egui::Button::new(tr("print.apply")))
                        .clicked();
                    reset = ui.button(tr("print.reset")).clicked();
                    open_print = ui.button(tr("menu.file.print")).clicked();
                    open_projection = ui.button(tr("menu.file.projection")).clicked();
                });
            });
        if chosen_size.is_some() {
            self.export_size = chosen_size;
        }
        if open_print {
            self.open_direct_print();
        }
        if open_projection {
            self.open_projection();
        }
        if open_calib && self.calib_dialog.is_none() {
            self.calib_dialog = Some(CalibDialog::new());
        }
        if reset {
            dialog = PrintDialog::from_setup(&PageSetup::default());
        }
        if apply {
            if let Ok(setup) = dialog.to_setup() {
                self.apply_page_setup(setup);
                self.apply_output_content(dialog.content);
                return;
            }
        }
        if open {
            self.print_dialog = Some(dialog);
        }
    }

    /// Section « Contenu de la sortie » (US-13.4) : cases à cocher groupées, avec le nombre
    /// d'éléments de la feuille courante dans chaque catégorie.
    fn content_section(&self, ui: &mut egui::Ui, content: &mut OutputContent) {
        // Replié tant que le contenu est celui par défaut : la fenêtre garde sa hauteur.
        let open = !content.is_default();
        egui::CollapsingHeader::new(RichText::new(tr("print.content.title")).strong())
            .id_salt("print-content-header")
            .default_open(open)
            .show(ui, |ui| self.content_body(ui, content));
    }

    fn content_body(&self, ui: &mut egui::Ui, content: &mut OutputContent) {
        crate::a11y::label(
            ui,
            RichText::new(tr("print.content.intro"))
                .small()
                .color(pal().muted),
        );
        crate::a11y::label(ui, RichText::new(tr("print.content.size")).small());
        let full = self.export_project().ok().and_then(|project| {
            Scene::from_sheet_with(
                &project,
                self.editor.sheet,
                SceneOptions {
                    piece_mode: content.piece_mode,
                    ..SceneOptions::default()
                },
            )
            .ok()
        });
        // Pièces à marge de couture (US-05.4) : couture, coupe calculée, ou les deux.
        crate::a11y::label(
            ui,
            RichText::new(tr("print.content.pieces")).small().strong(),
        );
        ui.horizontal_wrapped(|ui| {
            for mode in PieceExportMode::ALL {
                ui.radio_value(&mut content.piece_mode, mode, piece_mode_label(mode));
            }
        });
        crate::a11y::label(
            ui,
            RichText::new(tr("print.content.pieces_note"))
                .small()
                .color(pal().muted),
        );
        ui.horizontal_wrapped(|ui| {
            if ui.button(tr("print.content.all")).clicked() {
                *content = OutputContent::default();
            }
            if ui.button(tr("print.content.sewing_only")).clicked() {
                *content = OutputContent::sewing_only();
            }
            if ui.button(tr("print.content.cutting_only")).clicked() {
                *content = OutputContent::cutting_only();
            }
        });
        use OutputCategory as C;
        let groups: [(String, &[C]); 4] = [
            (
                tr("print.content.group.contours"),
                &[C::CuttingContour, C::SewingContour, C::Grainline],
            ),
            (
                tr("print.content.group.lines"),
                &[C::Construction, C::SpecialLines, C::Axes],
            ),
            (
                tr("print.content.group.marks"),
                &[
                    C::Notches,
                    C::Markings,
                    C::Darts,
                    C::Buttons,
                    C::Points,
                    C::PointLabels,
                    C::PieceLabels,
                ],
            ),
            (
                tr("print.content.group.dims"),
                &[C::Dimensions, C::FullDimensions],
            ),
        ];
        for (title, cats) in groups {
            crate::a11y::label(ui, RichText::new(title).small().strong());
            egui::Grid::new(("print-content-grid", cats[0].key()))
                .num_columns(2)
                .show(ui, |ui| {
                    for (i, c) in cats.iter().enumerate() {
                        let available = full.as_ref().map_or(0, |s| category_count(s, *c));
                        let label = format!("{} ({available})", category_label(*c));
                        let mut on = content.includes(*c);
                        // Les noms de points n'existent que si les points sont écrits.
                        let enabled = *c != C::PointLabels || content.includes(C::Points);
                        if ui
                            .add_enabled(enabled, egui::Checkbox::new(&mut on, label))
                            .changed()
                        {
                            content.set(*c, on);
                        }
                        if i % 2 == 1 {
                            ui.end_row();
                        }
                    }
                });
        }
    }

    /// Dimensions résultantes en mm et pages de la feuille courante pour un réglage valide.
    fn print_preview_ui(&self, ui: &mut egui::Ui, setup: &PageSetup, content: &OutputContent) {
        let n = ml;
        // Même chemin que l'export : `export_scene` applique le contenu enregistré, ici celui de
        // la fenêtre (pas encore appliqué).
        let scene = pf_export::size_export::size_scene(
            self.editor.project(),
            self.effective_export_size(),
            self.editor.sheet,
            content,
            SceneOptions {
                lang: pf_i18n::lang(),
                ..SceneOptions::default()
            },
        );
        // Récapitulatif seulement quand le contenu n'est pas celui par défaut.
        let full = (!content.is_default())
            .then(|| {
                self.export_project().ok().and_then(|project| {
                    Scene::from_sheet_with(
                        &project,
                        self.editor.sheet,
                        SceneOptions {
                            piece_mode: content.piece_mode,
                            ..SceneOptions::default()
                        },
                    )
                    .ok()
                })
            })
            .flatten();
        if let Some(full) = full {
            let summary = content.summary(&full);
            crate::a11y::label(
                ui,
                tr_args(
                    "print.content.summary",
                    &[
                        ("kept", &summary.total_kept().to_string()),
                        ("total", &summary.total_available().to_string()),
                    ],
                ),
            );
            for note in &summary.notes {
                // « Rien à écrire » est dit plus bas, en rouge, avec le refus d'export.
                let key = match note {
                    ContentNote::CuttingAbsent => "print.content.note.cutting_absent",
                    ContentNote::SewingAbsent => "print.content.note.sewing_absent",
                    ContentNote::LabelsWithoutPoints => "print.content.note.labels_without_points",
                    ContentNote::NothingKept => continue,
                };
                crate::a11y::label(ui, RichText::new(tr(key)).small().color(pal().muted));
            }
        }
        match setup.page_height_mm() {
            Some(h) => {
                crate::a11y::label(
                    ui,
                    pf_i18n::tr_len(
                        "print.preview.page",
                        &[("w", &n(setup.page_width_mm())), ("h", &n(h))],
                    ),
                );
                crate::a11y::label(
                    ui,
                    pf_i18n::tr_len(
                        "print.preview.printable",
                        &[
                            ("w", &n(setup.printable_width_mm())),
                            ("h", &n(setup.printable_height_mm().unwrap_or(0.0))),
                        ],
                    ),
                );
            }
            None => {
                crate::a11y::label(
                    ui,
                    pf_i18n::tr_len("print.preview.roll", &[("w", &n(setup.page_width_mm()))]),
                );
                crate::a11y::label(
                    ui,
                    pf_i18n::tr_len(
                        "print.preview.printable_roll",
                        &[("w", &n(setup.printable_width_mm()))],
                    ),
                );
            }
        }
        if !setup.scale.is_identity() {
            crate::a11y::label(
                ui,
                tr_args(
                    "print.preview.scale",
                    &[
                        ("x", &factor_text(setup.scale.x)),
                        ("y", &factor_text(setup.scale.y)),
                    ],
                ),
            );
        }
        match scene {
            Ok(scene) => match pdf::plan_tiles_with(&scene, setup) {
                Ok(tiles) => {
                    let pages =
                        pdf::page_count_content(std::slice::from_ref(&scene), setup, content);
                    match pages {
                        Ok(total) if setup.format.is_roll() => {
                            let length = tiles.first().map_or(0.0, |t| t.layout.page_h_mm);
                            crate::a11y::label(
                                ui,
                                pf_i18n::tr_len(
                                    "print.preview.sheet_roll",
                                    &[("len", &n(length)), ("pages", &total.to_string())],
                                ),
                            );
                        }
                        Ok(total) => {
                            crate::a11y::label(
                                ui,
                                tr_args("print.preview.sheet", &[("pages", &total.to_string())]),
                            );
                            if let Some(t) = tiles.first() {
                                let omitted = (t.cols * t.rows).saturating_sub(tiles.len());
                                if omitted > 0 {
                                    crate::a11y::label(
                                        ui,
                                        tr_args(
                                            "print.preview.omitted",
                                            &[("n", &omitted.to_string())],
                                        ),
                                    );
                                }
                                if t.turns > 0 {
                                    crate::a11y::label(
                                        ui,
                                        tr_args(
                                            "print.preview.turned",
                                            &[("deg", &(90 * u32::from(t.turns)).to_string())],
                                        ),
                                    );
                                }
                            }
                        }
                        Err(e) => {
                            crate::a11y::label(
                                ui,
                                RichText::new(export_error(&e)).color(pal().error),
                            );
                        }
                    }
                }
                Err(e) => {
                    crate::a11y::label(ui, RichText::new(export_error(&e)).color(pal().error));
                }
            },
            Err(ExportError::NothingSelected(_)) => {
                crate::a11y::label(
                    ui,
                    RichText::new(tr("print.content.nothing")).color(pal().error),
                );
            }
            Err(_) => {
                crate::a11y::label(
                    ui,
                    RichText::new(tr("inspector.print.empty")).color(pal().muted),
                );
            }
        }
        if !content.is_default() {
            crate::a11y::label(
                ui,
                RichText::new(tr("print.content.not_paper"))
                    .small()
                    .color(pal().muted),
            );
        }
    }
}

/// Libellé d'une catégorie de contenu dans la langue de l'interface.
/// Libellé du mode d'export des pièces à marge de couture (US-05.4).
pub fn piece_mode_label(m: PieceExportMode) -> String {
    match m {
        PieceExportMode::Both => tr("print.content.pieces.both"),
        PieceExportMode::SewingOnly => tr("print.content.pieces.sewing"),
        PieceExportMode::CutOnly => tr("print.content.pieces.cut"),
    }
}

pub fn category_label(c: OutputCategory) -> String {
    match c {
        OutputCategory::Construction => tr("print.content.cat.construction"),
        OutputCategory::SewingContour => tr("print.content.cat.sewing"),
        OutputCategory::CuttingContour => tr("print.content.cat.cutting"),
        OutputCategory::Grainline => tr("print.content.cat.grainline"),
        OutputCategory::SpecialLines => tr("print.content.cat.special_lines"),
        OutputCategory::Axes => tr("print.content.cat.axes"),
        OutputCategory::Notches => tr("print.content.cat.notches"),
        OutputCategory::Markings => tr("print.content.cat.markings"),
        OutputCategory::Buttons => tr("print.content.cat.buttons"),
        OutputCategory::Points => tr("print.content.cat.points"),
        OutputCategory::PointLabels => tr("print.content.cat.point_labels"),
        OutputCategory::Dimensions => tr("print.content.cat.dimensions"),
        OutputCategory::FullDimensions => tr("print.content.cat.full_dimensions"),
        OutputCategory::Darts => tr("print.content.cat.darts"),
        OutputCategory::PieceLabels => tr("print.content.cat.piece_labels"),
    }
}

// ---------------------------------------------------------------------------
// Étalonnage de l'imprimante (US-13.3)
// ---------------------------------------------------------------------------

/// Saisie de l'assistant : nom du profil et longueurs **mesurées** du carré étalon de 100 mm
/// imprimé (X horizontal, Y vertical de la page imprimée). Rien n'est enregistré avant
/// « Enregistrer » ; les facteurs sont calculés et affichés avant.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CalibDialog {
    pub name: String,
    pub measure_x: String,
    pub measure_y: String,
    /// Profil en cours de modification (`None` = nouveau profil).
    pub editing: Option<usize>,
    /// Dernier refus, affiché en rouge.
    pub error: Option<String>,
}

impl CalibDialog {
    pub fn new() -> CalibDialog {
        CalibDialog {
            measure_x: "100".into(),
            measure_y: "100".into(),
            ..CalibDialog::default()
        }
    }

    /// Facteurs déduits des mesures saisies, ou le message traduit du refus.
    pub fn factors(&self) -> Result<PrintScale, String> {
        let number = |text: &str, axis: Axis| {
            pf_core::input::parse_length_mm(text)
                .map_err(|_| tr_args("calib.err.number", &[("axis", axis_name(axis))]))
        };
        let (mx, my) = (
            number(&self.measure_x, Axis::X)?,
            number(&self.measure_y, Axis::Y)?,
        );
        PrintScale::from_measures(mx, my).map_err(|e| scale_error(&e))
    }
}

enum CalibAction {
    Save,
    New,
    Activate(Option<usize>),
    Edit(usize),
    Delete(usize),
}

impl App {
    /// Écrit les profils et le réglage dans les préférences puis les adopte ; un échec
    /// d'écriture n'adopte rien (refus sans trace dans le fichier comme en mémoire).
    fn commit_profiles(&mut self, book: PrinterProfiles, status: String) -> Result<(), String> {
        let mut setup = self.page_setup;
        setup.scale = book.active_scale();
        if let Some(path) = &self.settings_path {
            let value = settings_value(&setup, &book);
            if let Err(e) = pf_i18n::settings::save_value(path, SETTINGS_KEY, value) {
                return Err(tr_args(
                    "calib.err.save_failed",
                    &[("detail", &e.to_string())],
                ));
            }
        }
        self.printer_profiles = book;
        self.page_setup = setup;
        self.editor.status = crate::editor::Status {
            text: status,
            error: false,
        };
        Ok(())
    }

    /// Crée (`editing = None`) ou modifie un profil. Nom vide, trop long, avec caractère de
    /// contrôle, en double, facteurs hors bornes ou 17e profil : refus, rien d'écrit.
    pub fn profile_save(
        &mut self,
        name: &str,
        scale: PrintScale,
        editing: Option<usize>,
    ) -> Result<(), String> {
        let mut book = self.printer_profiles.clone();
        match editing {
            Some(i) => book.update(i, name, scale),
            None => book.add(name, scale).map(|_| ()),
        }
        .map_err(|e| profile_error(&e))?;
        let shown = name.trim().to_string();
        self.commit_profiles(book, tr_args("status.calib.saved", &[("name", &shown)]))
    }

    /// Active un profil (`None` = aucune correction).
    pub fn profile_activate(&mut self, index: Option<usize>) -> Result<(), String> {
        let mut book = self.printer_profiles.clone();
        book.set_active(index).map_err(|e| profile_error(&e))?;
        let status = match book.active() {
            Some(p) => tr_args("status.calib.activated", &[("name", &p.name)]),
            None => tr("status.calib.deactivated"),
        };
        self.commit_profiles(book, status)
    }

    pub fn profile_delete(&mut self, index: usize) -> Result<(), String> {
        let mut book = self.printer_profiles.clone();
        let removed = book.remove(index).map_err(|e| profile_error(&e))?;
        self.commit_profiles(
            book,
            tr_args("status.calib.deleted", &[("name", &removed.name)]),
        )
    }

    pub fn calib_window(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.calib_dialog.take() else {
            return;
        };
        let mut open = true;
        let mut action: Option<CalibAction> = None;
        egui::Window::new(tr("calib.title"))
            .id(egui::Id::new("calib_window"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                crate::a11y::label(
                    ui,
                    RichText::new(tr("calib.intro")).small().color(pal().muted),
                );
                egui::Grid::new("calib-grid").num_columns(2).show(ui, |ui| {
                    crate::a11y::label(ui, tr("calib.name"));
                    crate::a11y::add_text(
                        ui,
                        egui::TextEdit::singleline(&mut d.name)
                            .id_salt("calib-name")
                            .desired_width(160.0),
                    );
                    ui.end_row();
                    crate::a11y::label(ui, tr("calib.measure_x"));
                    crate::a11y::add_text(
                        ui,
                        egui::TextEdit::singleline(&mut d.measure_x)
                            .id_salt("calib-mx")
                            .desired_width(90.0),
                    );
                    ui.end_row();
                    crate::a11y::label(ui, tr("calib.measure_y"));
                    crate::a11y::add_text(
                        ui,
                        egui::TextEdit::singleline(&mut d.measure_y)
                            .id_salt("calib-my")
                            .desired_width(90.0),
                    );
                    ui.end_row();
                });
                let factors = d.factors();
                match &factors {
                    Ok(f) => {
                        let shown = |text: &str| {
                            pf_core::input::parse_length_mm(text).map_or_else(
                                |_| text.to_string(),
                                |v| mm_dec((v * 100.0).round() / 100.0, 2),
                            )
                        };
                        crate::a11y::label(
                            ui,
                            tr_args(
                                "calib.factor.x",
                                &[
                                    ("measure", &shown(&d.measure_x)),
                                    ("factor", &factor_text(f.x)),
                                ],
                            ),
                        );
                        crate::a11y::label(
                            ui,
                            tr_args(
                                "calib.factor.y",
                                &[
                                    ("measure", &shown(&d.measure_y)),
                                    ("factor", &factor_text(f.y)),
                                ],
                            ),
                        );
                    }
                    Err(msg) => {
                        crate::a11y::label(ui, RichText::new(msg).color(pal().error));
                    }
                }
                if let Some(msg) = &d.error {
                    crate::a11y::label(ui, RichText::new(msg).color(pal().error));
                }
                ui.horizontal(|ui| {
                    let label = if d.editing.is_some() {
                        tr("calib.update")
                    } else {
                        tr("calib.save")
                    };
                    if ui
                        .add_enabled(factors.is_ok(), egui::Button::new(label))
                        .clicked()
                    {
                        action = Some(CalibAction::Save);
                    }
                    if d.editing.is_some() && ui.button(tr("calib.new")).clicked() {
                        action = Some(CalibAction::New);
                    }
                });
                ui.separator();
                crate::a11y::label(
                    ui,
                    RichText::new(tr_args(
                        "calib.list.title",
                        &[
                            ("n", &self.printer_profiles.list().len().to_string()),
                            ("max", &MAX_PROFILES.to_string()),
                        ],
                    ))
                    .strong(),
                );
                if self.printer_profiles.list().is_empty() {
                    crate::a11y::label(
                        ui,
                        RichText::new(tr("calib.list.empty")).color(pal().muted),
                    );
                }
                let active = self.printer_profiles.active_index();
                for (i, p) in self.printer_profiles.list().iter().enumerate() {
                    ui.horizontal_wrapped(|ui| {
                        let mut text = tr_args(
                            "calib.row",
                            &[
                                ("name", &p.name),
                                ("x", &factor_text(p.scale.x)),
                                ("y", &factor_text(p.scale.y)),
                            ],
                        );
                        if active == Some(i) {
                            text.push(' ');
                            text.push_str(&tr("calib.active_mark"));
                        }
                        crate::a11y::label(ui, text);
                        let toggle = if active == Some(i) {
                            tr("calib.deactivate")
                        } else {
                            tr("calib.activate")
                        };
                        if ui.button(toggle).clicked() {
                            action = Some(CalibAction::Activate(if active == Some(i) {
                                None
                            } else {
                                Some(i)
                            }));
                        }
                        if ui.button(tr("calib.edit")).clicked() {
                            action = Some(CalibAction::Edit(i));
                        }
                        if ui.button(tr("calib.delete")).clicked() {
                            action = Some(CalibAction::Delete(i));
                        }
                    });
                }
                crate::a11y::label(
                    ui,
                    RichText::new(tr_args(
                        "calib.limits",
                        &[
                            ("name_max", &PROFILE_NAME_MAX_CHARS.to_string()),
                            ("min", &factor_text(SCALE_MIN)),
                            ("max", &factor_text(SCALE_MAX)),
                        ],
                    ))
                    .small()
                    .color(pal().muted),
                );
                crate::a11y::label(
                    ui,
                    RichText::new(tr("calib.honesty"))
                        .small()
                        .color(pal().muted),
                );
            });
        match action {
            Some(CalibAction::Save) => {
                d.error = None;
                match d.factors() {
                    Ok(scale) => match self.profile_save(&d.name, scale, d.editing) {
                        Ok(()) => d = CalibDialog::new(),
                        Err(msg) => d.error = Some(msg),
                    },
                    Err(msg) => d.error = Some(msg),
                }
            }
            Some(CalibAction::New) => d = CalibDialog::new(),
            Some(CalibAction::Activate(index)) => {
                d.error = self.profile_activate(index).err();
            }
            Some(CalibAction::Edit(i)) => {
                if let Some(p) = self.printer_profiles.list().get(i) {
                    let measure = |f: f64| mm_dec((REFERENCE_MM / f * 100.0).round() / 100.0, 2);
                    d = CalibDialog {
                        name: p.name.clone(),
                        measure_x: measure(p.scale.x),
                        measure_y: measure(p.scale.y),
                        editing: Some(i),
                        error: None,
                    };
                }
            }
            Some(CalibAction::Delete(i)) => match self.profile_delete(i) {
                Ok(()) => d = CalibDialog::new(),
                Err(msg) => d.error = Some(msg),
            },
            None => {}
        }
        if open {
            self.calib_dialog = Some(d);
        }
    }
}
