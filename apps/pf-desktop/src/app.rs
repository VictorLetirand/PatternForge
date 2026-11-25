//! Fenêtre principale : menus, raccourcis, fichiers, dialogues.

use crate::editor::{Editor, Status, Tool};
use crate::shortcuts;
use crate::text::core_error;
use crate::view::View;
use crate::Launch;
use eframe::egui::{self, Key, Modifiers, RichText};
use pf_core::recovery::{self, Recovery, RecoveryCheck};
use pf_core::{Document, Project, SheetId, FILE_EXTENSION};
use pf_export::{pdf, svg, Scene};
use pf_geometry::PointMm;
use pf_i18n::{settings, tr, tr_args, Lang};
use pf_ui::components::{button, ButtonKind};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Intervalle par défaut de la copie de secours (`autosave_seconds`).
pub const DEFAULT_AUTOSAVE_SECONDS: u64 = 60;

/// Action en attente d'une réponse « enregistrer les modifications ? ».
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pending {
    New,
    Open,
    Quit,
}

/// Boîtes modales de l'application (PH-2b), de la plus basse à la plus haute.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ModalKind {
    Recovery,
    Unsaved,
    Import,
    Error,
    /// Confirmation d'une suppression qui touche d'autres objets (US-03.6).
    Delete,
    /// Fichier modifié ailleurs avant l'enregistrement (US-17.3).
    Conflict,
    /// Fichier ouvert dans une autre fenêtre (US-17.3).
    Locked,
}

/// Largeur commune des boîtes modales (stable d'une boîte à l'autre).
pub(crate) const MODAL_WIDTH: f32 = 440.0;

/// Titre fort d'une boîte modale : taille supérieure, texte accentué.
pub(crate) fn modal_title(ui: &mut egui::Ui, text: &str) {
    crate::a11y::label(
        ui,
        RichText::new(text)
            .size(20.0)
            .strong()
            .color(crate::palette::pal().accent_text),
    );
    ui.add_space(6.0);
}

/// Champ texte en cours d'édition : (clé de l'objet édité, texte).
pub type TextBuf = Option<(u64, String)>;

/// Projet source ouvert pour copier une feuille (US-01.4).
pub struct ImportDialog {
    pub path: PathBuf,
    pub source: Project,
}

/// Reprise proposée : copie de secours et chemin du projet concerné.
pub struct RecoveryPrompt {
    pub recovery: Recovery,
    pub path: Option<PathBuf>,
}

pub struct App {
    pub editor: Editor,
    pub import: Option<ImportDialog>,
    pub view: View,
    /// Recadrer la vue sur le contenu au prochain affichage.
    pub fit_pending: bool,
    /// Coin de départ d'une fenêtre de sélection (monde, mm).
    pub window_start: Option<PointMm>,
    /// Tracé du lasso en cours (monde, mm) ; `None` hors glisser (US-16.1).
    pub lasso: Option<Vec<PointMm>>,
    pub renaming: Option<(SheetId, String)>,
    pub rename_focus: bool,
    pub text_buf: TextBuf,
    /// Erreur d'une formule saisie dans l'inspecteur (US-03.1b) : clé du champ et message ;
    /// rien n'a été modifié.
    pub formula_error: Option<(u64, String)>,
    /// Rapport du dernier import d'éléments internes (US-04.3), avec sa pièce.
    pub internal_report: Option<(
        pf_core::PieceId,
        pf_core::piece_internal::InternalImportReport,
    )>,
    /// Formulaire « Créer une pièce dérivée… » ouvert (US-04.4) ; état d'interface,
    /// jamais enregistré.
    pub derived_form: Option<crate::panels::DerivedForm>,
    /// Formulaire « Ajouter un marquage… » ouvert (US-06.3) ; état d'interface,
    /// jamais enregistré.
    pub marking_form: Option<crate::panels::MarkingForm>,
    /// Bloc « Perçages au pas… » (PH-4b) : état d'interface, jamais enregistré.
    pub pitch_form: Option<crate::marking_run::PitchForm>,
    /// Étape de capture : ouvre une seule fois le bloc « Perçages au pas… » (jamais forcé ensuite).
    pub pitch_open_once: bool,
    /// Bloc « Lignes de piqûre… » (US-06.9) : état d'interface, jamais enregistré.
    pub stitch_form: Option<crate::stitch::StitchForm>,
    /// Étape de capture : ouvre une seule fois le bloc « Lignes de piqûre… ».
    pub stitch_open_once: bool,
    /// Bloc « Zones… » (US-06.10) : état d'interface, jamais enregistré.
    pub zone_form: Option<crate::zones::ZoneForm>,
    /// Étape de capture : ouvre une seule fois le bloc « Zones… ».
    pub zone_open_once: bool,
    /// Brouillon de la section « Longueur imposée » (US-03.3) : état d'interface, jamais enregistré.
    pub imposed_form: Option<crate::imposed::ImposedForm>,
    /// Étape de capture : ouvre une seule fois la section « Longueur imposée ».
    pub imposed_open_once: bool,
    /// Formulaire « Ajouter des boutons… » ouvert (US-06.4) ; état d'interface, jamais
    /// enregistré.
    pub button_form: Option<crate::buttons::ButtonForm>,
    /// Distance saisie pour creer une surpiqure liee (US-06.8, mm) ; etat d'interface, jamais
    /// enregistre.
    pub topstitch_distance: String,
    /// Vue « Plan de pièces » (US-04.5) ouverte à la place de la feuille ; état
    /// d'interface, jamais enregistré (les placements, eux, sont dans le projet).
    pub plan: Option<crate::plan::PlanState>,
    pub palette_open: bool,
    /// Le pointeur est sur le canevas (la bulle de saisie directe le suit, US-16.2).
    pub canvas_hovered: bool,
    /// Fenêtre « Transformer la sélection » (déplacer, pivoter, symétrie, copier).
    pub transform: Option<crate::transform::TransformDialog>,
    /// Fenêtre « Assembler deux pièces » (US-04.6) ; état d'interface, jamais enregistré.
    pub assemble: Option<crate::assemble::JoinDialog>,
    /// Fenêtre « Déplier une pièce » (US-04.7) ; état d'interface, jamais enregistré.
    pub unfold: Option<crate::unfold::UnfoldDialog>,
    pub level: Option<crate::align::LevelDialog>,
    /// Fenêtre « Variables » (US-03.1a) ; état d'interface, jamais enregistré.
    pub variables: Option<crate::variables::VariablesDialog>,
    /// Fenêtre « Diagnostic » des liens (US-03.5) ; état d'interface, jamais enregistré.
    pub diagnostic: Option<crate::diagnostic::DiagnosticDialog>,
    /// Fenêtre « Dépendances » (US-03.6) ; état d'interface, jamais enregistré.
    pub dependencies: Option<crate::dependencies::DependenciesDialog>,
    /// Fenêtre « Tailles » (US-09.1) ; état d'interface, jamais enregistré.
    pub sizes: Option<crate::sizes::SizesDialog>,
    /// Fenêtre « Gradation » (US-09.2a), état d'interface seulement.
    pub grading: Option<crate::grading::GradingDialog>,
    pub size_measures: Option<crate::size_measures::SizeMeasuresDialog>,
    /// Fenêtre « Barème de mesures » (US-10.1) ; état d'interface, jamais enregistré.
    pub bareme: Option<crate::measurements::BaremeDialog>,
    /// Fenêtre « Fiches client » (US-10.2) ; état d'interface, jamais enregistré.
    pub clients: Option<crate::measurements::ClientsDialog>,
    /// Fenêtre « Retouches et mesures finies » (US-10.3 / 10.4) ; état d'interface, jamais enregistré.
    pub finished: Option<crate::finished::FinishedDialog>,
    /// Fenêtre « Nid de tailles » (US-09.4a) ; les réglages du nid sont dans `nest`.
    pub size_nest: Option<crate::size_nest::SizeNestDialog>,
    /// Fenêtre « Contrôle de forme » (US-09.9) ; état d'interface, jamais enregistré.
    pub shape_control: Option<crate::shape_control::ShapeControlDialog>,
    /// Nid de tailles affiché sur le canevas (US-09.4a) : état d'interface, jamais enregistré.
    pub nest: crate::size_nest::NestView,
    /// Taille exportée (US-09.8) : choix d'interface, jamais enregistré ; `None` = base.
    pub export_size: Option<pf_core::SizeId>,
    /// Fenêtre « Exporter des tailles » (US-09.8) ; état d'interface, jamais enregistré.
    pub export_sizes: Option<crate::export_size::ExportSizesDialog>,
    pub bom: Option<crate::bom::BomDialog>,
    /// Fenêtre « Variantes » (US-11.1) ; état d'interface, jamais enregistré.
    pub variants: Option<crate::variants::VariantsDialog>,
    /// Fenêtre « Conventions de nommage » (US-11.5) ; état d'interface, jamais enregistré.
    pub naming: Option<crate::naming::NamingDialog>,
    pub cutting: Option<crate::cutting::CuttingDialog>,
    pub versions: Option<crate::versions::VersionsDialog>,
    pub direct_print: Option<crate::direct_print_ui::DirectPrintDialog>,
    pub projection: Option<crate::projection_ui::ProjectionState>,
    /// Fenetre Image de fond (US-14.1) : etat d'interface, jamais enregistre.
    pub background: Option<crate::background::BackgroundDialog>,
    /// Fichier et texture du fond de la feuille active (cache d'affichage).
    pub bg_view: crate::background::BgView,
    /// Spouleur d'impression directe (US-13.6) : le systÃ¨me, ou un faux dans les tests.
    pub spooler: Box<dyn crate::direct_print::Spooler>,
    pub print_temp_dir: std::path::PathBuf,
    /// Fenêtre « Bibliothèque » (US-11.4) ; état d'interface, jamais enregistré.
    pub library: Option<crate::library::LibraryDialog>,
    /// Dossier de la bibliothèque (réglage `library_dir`, hors projet) ; absent = `library` à côté
    /// de `settings.json`.
    pub library_dir: Option<PathBuf>,
    pub(crate) open_job: Option<crate::open_job::OpenJob>,
    pub(crate) open_async_bytes: u64,
    /// Suppression en attente de confirmation (US-03.6) : impact annoncé par le cœur.
    pub pending_delete: Option<pf_core::DeletionImpact>,
    /// Réglage de page du PDF (US-13.1) : préférence de l'application, hors projet.
    pub page_setup: pf_export::PageSetup,
    /// Fenêtre « Mise en page du PDF » (US-13.1) ; état d'interface, jamais enregistré.
    pub print_dialog: Option<crate::print_setup::PrintDialog>,
    /// Profils d'imprimante (US-13.3) : préférence de l'application, hors projet.
    pub printer_profiles: pf_export::PrinterProfiles,
    /// Contenu de la sortie PDF et SVG (US-13.4) : préférence de l'application, hors projet.
    pub output_content: pf_export::OutputContent,
    /// Assistant « Étalonner l'imprimante » (US-13.3) ; état d'interface, jamais enregistré.
    pub calib_dialog: Option<crate::print_setup::CalibDialog>,
    /// Fenêtre « Exporter en DXF » (US-12.1) : rapport avant écriture ; état d'interface.
    pub dxf_dialog: Option<crate::dxf::DxfDialog>,
    /// Fenêtre « Importer un DXF » (US-12.3) : aperçu et rapport avant import ; état d'interface.
    pub hpgl_dialog: Option<crate::hpgl::HpglDialog>,
    /// Fenêtre « Exporter le rapport de contrôle » (US-08.5) : instantané en lecture seule ;
    /// état d'interface, jamais enregistré.
    pub report_dialog: Option<crate::report::ReportDialog>,
    pub dxf_import: Option<crate::dxf_generic::ImportDialog>,
    /// Fenêtre « Importer un SVG / PDF » (US-12.4), état d'interface.
    pub vector_import: Option<crate::vector_import::VectorDialog>,
    pub aama_import: Option<crate::dxf_aama::AamaDialog>,
    /// Fenêtre « Exporter la construction en DXF » (US-12.3) ; état d'interface.
    pub dxf_construction: Option<crate::dxf_generic::ConstructionDialog>,
    /// Fenêtre « Rechercher » (US-01.5).
    pub search: Option<crate::search::SearchDialog>,
    pub pending: Option<Pending>,
    pub error_dialog: Option<String>,
    pub show_help: bool,
    /// Étape affichée du guide du premier patron (US-16.9) ; `None` = fenêtre fermée.
    pub guide_step: Option<usize>,
    /// Espace de travail, outils masqués et côté du dock (US-16.4) : préférences hors projet.
    pub layout: crate::workspace::Layout,
    /// Fenêtre « Personnaliser ».
    pub show_customize: bool,
    /// Fenêtre « Historique » (US-16.6) et son journal d'étiquettes.
    pub show_history: bool,
    /// Préférences de l'application (US-16.7) : hors projet, clé `preferences` de `settings.json`.
    pub prefs: crate::prefs::Prefs,
    pub show_prefs: bool,
    /// Unité d'affichage, grille, règles et vues nommées (US-16.5) : hors projet, clé `views`.
    pub view_settings: crate::views::ViewSettings,
    pub show_views: bool,
    pub view_name_draft: String,
    pub view_rename: Option<(usize, String)>,
    /// Dernier problème de lecture ou d'action sur les vues, affiché dans la fenêtre.
    pub view_issue: Option<String>,
    /// Boîte à cadrer à la prochaine image (zoom sur la sélection).
    pub zoom_target: Option<pf_geometry::Bbox>,
    pub history: crate::history::HistoryLog,
    /// Thème sombre (réglage mémorisé hors projet).
    pub dark: bool,
    /// Outil dont on est en train de choisir la lettre (aide F1).
    pub shortcut_capture: Option<Tool>,
    /// Capture d'une combinaison depuis Préférences > Raccourcis clavier (US-16.3).
    pub key_capture: Option<shortcuts::Target>,
    pub key_error: Option<String>,
    /// Section « Raccourcis clavier » des préférences ouverte d'emblée (étape de capture).
    pub shortcuts_open: bool,
    /// Donne le focus clavier au bouton « Réinitialiser » des préférences (étape de capture).
    pub focus_demo: bool,
    /// Commandes exécutées par la table des raccourcis (tests seulement).
    #[cfg(test)]
    pub cmd_log: Vec<shortcuts::Cmd>,
    /// Dernier refus d'affectation, affiché dans l'aide.
    pub shortcut_error: Option<String>,
    allow_close: bool,
    screenshot: Option<PathBuf>,
    /// Fichier de réglages (langue). `None` : rien n'est jamais écrit (tests).
    pub settings_path: Option<PathBuf>,
    /// Dossier de données utilisateur (copie de secours d'un projet sans
    /// chemin). `None` : aucune copie de ce genre (tests).
    pub recovery_dir: Option<PathBuf>,
    /// Intervalle de copie de secours ; `None` : désactivée.
    pub autosave_interval: Option<Duration>,
    last_autosave: Instant,
    /// État du projet à la dernière copie de secours.
    autosaved: Option<Project>,
    /// Boîte « Récupérer / Ignorer ».
    pub recovery_prompt: Option<RecoveryPrompt>,
    /// Verrou du fichier ouvert et boîtes « modifié ailleurs » / « ouvert ailleurs » (US-17.3).
    pub(crate) file_lock: Option<pf_core::file_guard::FileLock>,
    pub(crate) last_lock_refresh: Instant,
    pub(crate) conflict_prompt: Option<crate::safeguard::ConflictPrompt>,
    pub(crate) lock_prompt: Option<crate::safeguard::LockPrompt>,
    /// Journal d'erreurs local, assaini et borné (US-17.5) : clé `error_log` de `settings.json`.
    pub(crate) error_log: pf_core::error_log::ErrorLog,
    pub(crate) show_error_log: bool,
    pub(crate) show_glossary: bool,
    pub(crate) glossary_filter: String,
    pub(crate) logged_dialog: Option<String>,
    pub(crate) logged_status: Option<String>,
    pub(crate) frame: u64,
    last_title: String,
    /// Rectangle du canevas à la dernière image (px).
    pub canvas_rect: egui::Rect,
    /// Section « Marge de couture » dépliée de force (captures `--etape marge…`) ; état
    /// d'interface, jamais enregistré.
    pub seam_open: bool,
    /// Sous-bloc « Marges par matière » déplié de force (capture `--etape matieres`).
    pub materials_open: bool,
    /// Capture `--etape coins` : fait défiler la fiche jusqu'au bloc « Coins » (état d'interface).
    pub corners_focus: bool,
    /// Captures `--defile PX` : décalage imposé aux panneaux défilants (état d'interface).
    pub capture_scroll: Option<f32>,
    /// Dock des fenêtres d'outil (PH-2b) : état d'interface, jamais enregistré.
    pub dock: crate::dock::ToolDock,
    /// Panneau des feuilles replié ou non (PH-2c) : préférence d'affichage, hors projet.
    pub(crate) sheets_fold: crate::dock::SheetsFold,
    /// Boîte modale du dessus et image où elle est apparue (Entrée / Échap ignorés ce jour-là).
    modal_seen: Option<(ModalKind, u64)>,
    /// Un champ texte avait le focus à la fin de l'image précédente (egui le retire dès qu'Échap
    /// est pressé : sans cette mémoire, Échap fermerait la fenêtre pendant la saisie).
    typing_last_frame: bool,
}

/// Avertissements d'export du contour de coupe (US-05.4) : une phrase lisible par contour (ou
/// jeu de crans) non calculable, à la suite de la note de fin d'export ; vide quand tout est
/// exporté. Jamais de fichier silencieusement incomplet.
pub fn cut_warnings_note(warnings: &[pf_export::CutWarning]) -> String {
    let mut out = String::new();
    for w in warnings {
        let key = if w.cut_notches {
            "export.cut_warning.cut_notches"
        } else if w.notches_only {
            "export.cut_warning.notches"
        } else {
            "export.cut_warning"
        };
        out.push_str(&tr_args(
            key,
            &[("piece", w.piece.as_str()), ("reason", w.reason.as_str())],
        ));
        if w.sewing_kept {
            out.push_str(&tr("export.cut_warning.sewing_kept"));
        }
    }
    out
}

/// Avertissements d'export d'une scène : contours de coupe puis pinces non calculables (omises,
/// jamais en silence) ; vide quand tout est exporté.
pub fn scene_warnings_note(scene: &pf_export::Scene) -> String {
    let mut out = cut_warnings_note(&scene.cut_warnings);
    for w in &scene.dart_warnings {
        out.push_str(&tr_args(
            "export.dart_warning",
            &[("piece", w.piece.as_str()), ("reason", w.reason.as_str())],
        ));
    }
    out
}

impl App {
    /// Lancement réel : la langue vient de `--lang`, sinon du réglage enregistré.
    pub fn new(cc: &eframe::CreationContext<'_>, mut launch: Launch) -> App {
        let settings_path = settings::default_settings_path();
        if launch.lang.is_none() {
            launch.lang = settings_path.as_deref().map(settings::load_language);
        }
        let launch_dark = launch.dark;
        let lang_forced = launch.lang.is_some();
        // Une étape de capture garde ses préférences propres (jamais celles de l'utilisateur).
        let has_stage = launch.stage.is_some();
        let at_startup = launch.file.is_none() && !launch.demo;
        let mut app = Self::with_context(&cc.egui_ctx, launch);
        app.init_extra_languages(lang_forced);
        if let Some(value) = settings_path
            .as_deref()
            .and_then(|p| settings::load_value(p, shortcuts::SETTINGS_KEY))
        {
            shortcuts::set_current_keymap(shortcuts::Keymap::from_settings(&value));
        }
        if let Some(p) = settings_path.as_deref() {
            app.page_setup = crate::print_setup::load_page_setup(p);
            app.printer_profiles = crate::print_setup::load_profiles(p);
            app.page_setup.scale = app.printer_profiles.active_scale();
            app.output_content = crate::print_setup::load_output_content(p);
            app.layout = crate::workspace::load_layout(p);
            if !has_stage {
                app.prefs = crate::prefs::load_prefs(p);
                let (views, issues) = crate::views::load(p);
                app.view_settings = views;
                app.editor.show_rulers = app.view_settings.rulers;
                if let Some(issue) = issues.first() {
                    let text = issue.text();
                    app.view_issue = Some(text.clone());
                    app.editor.error(text);
                }
            }
            if app.error_log.is_empty() {
                app.error_log = crate::error_journal::load(p);
            }
        }
        if let Some(p) = settings_path.as_deref() {
            app.dark = settings::load_value(p, "theme")
                .and_then(|v| v.as_str().map(|t| t == "dark"))
                == Some(true);
        }
        if settings_path
            .as_deref()
            .and_then(|p| settings::load_value(p, crate::dock::SHEETS_SETTING))
            .and_then(|v| v.as_str().map(|t| t == "folded"))
            == Some(true)
        {
            app.sheets_fold = crate::dock::SheetsFold::Folded;
        }
        if let Some(dark) = launch_dark {
            app.dark = dark;
        }
        app.autosave_interval = autosave_interval(
            settings_path
                .as_deref()
                .and_then(|p| settings::load_value(p, "autosave_seconds"))
                .as_ref(),
        );
        app.recovery_dir = settings_path
            .as_deref()
            .and_then(Path::parent)
            .map(Path::to_path_buf);
        app.library_dir = settings_path
            .as_deref()
            .and_then(|p| settings::load_value(p, crate::library::LIBRARY_DIR_SETTING))
            .and_then(|v| v.as_str().map(PathBuf::from))
            .filter(|p| !p.as_os_str().is_empty());
        app.settings_path = settings_path;
        if at_startup {
            app.check_recovery();
        }
        app
    }

    pub fn with_context(ctx: &egui::Context, launch: Launch) -> App {
        ctx.set_theme(egui::Theme::Light);
        if let Some(lang) = launch.lang {
            pf_i18n::set_lang(lang);
        }
        pf_core::input::set_decimal_separator(pf_i18n::lang().decimal_separator());
        let mut app = App {
            editor: Editor::new(Document::new(Project::new(&tr("project.default_name")))),
            import: None,
            view: View::default(),
            fit_pending: true,
            window_start: None,
            lasso: None,
            renaming: None,
            rename_focus: false,
            text_buf: None,
            formula_error: None,
            internal_report: None,
            derived_form: None,
            marking_form: None,
            pitch_form: None,
            pitch_open_once: false,
            stitch_form: None,
            stitch_open_once: false,
            zone_form: None,
            zone_open_once: false,
            imposed_form: None,
            imposed_open_once: false,
            button_form: None,
            topstitch_distance: "5".into(),
            plan: None,
            palette_open: false,
            canvas_hovered: false,
            transform: None,
            assemble: None,
            unfold: None,
            level: None,
            variables: None,
            diagnostic: None,
            dependencies: None,
            sizes: None,
            grading: None,
            size_measures: None,
            bareme: None,
            clients: None,
            finished: None,
            size_nest: None,
            export_size: None,
            export_sizes: None,
            bom: None,
            variants: None,
            naming: None,
            cutting: None,
            versions: None,
            direct_print: None,
            projection: None,
            background: None,
            bg_view: crate::background::BgView::default(),
            spooler: Box::new(crate::direct_print::SystemSpooler),
            print_temp_dir: std::env::temp_dir(),
            library: None,
            library_dir: None,
            open_job: None,
            open_async_bytes: crate::open_job::ASYNC_OPEN_BYTES,
            shape_control: None,
            nest: Default::default(),
            pending_delete: None,
            page_setup: pf_export::PageSetup::default(),
            print_dialog: None,
            printer_profiles: pf_export::PrinterProfiles::default(),
            output_content: pf_export::OutputContent::default(),
            calib_dialog: None,
            dxf_dialog: None,
            hpgl_dialog: None,
            report_dialog: None,
            dxf_import: None,
            vector_import: None,
            aama_import: None,
            dxf_construction: None,
            search: None,
            pending: None,
            error_dialog: None,
            show_help: false,
            guide_step: None,
            layout: crate::workspace::Layout::default(),
            show_customize: false,
            show_history: false,
            prefs: crate::prefs::Prefs::default(),
            show_prefs: false,
            view_settings: crate::views::ViewSettings::default(),
            show_views: false,
            view_name_draft: String::new(),
            view_rename: None,
            view_issue: None,
            zoom_target: None,
            history: crate::history::HistoryLog::default(),
            dark: false,
            shortcut_capture: None,
            key_capture: None,
            key_error: None,
            shortcuts_open: false,
            focus_demo: false,
            #[cfg(test)]
            cmd_log: Vec::new(),
            shortcut_error: None,
            allow_close: false,
            screenshot: launch.screenshot,
            settings_path: None,
            recovery_dir: None,
            autosave_interval: None,
            last_autosave: Instant::now(),
            autosaved: None,
            recovery_prompt: None,
            file_lock: None,
            last_lock_refresh: Instant::now(),
            conflict_prompt: None,
            lock_prompt: None,
            error_log: pf_core::error_log::ErrorLog::default(),
            show_error_log: false,
            show_glossary: false,
            glossary_filter: String::new(),
            logged_dialog: None,
            logged_status: None,
            frame: 0,
            last_title: String::new(),
            canvas_rect: egui::Rect::NOTHING,
            seam_open: false,
            materials_open: false,
            corners_focus: false,
            capture_scroll: launch.scroll,
            dock: crate::dock::ToolDock::default(),
            sheets_fold: crate::dock::SheetsFold::Auto,
            modal_seen: None,
            typing_last_frame: false,
        };
        if let Some(path) = launch.file {
            app.open_path(&path);
        } else if launch.demo {
            match pf_core::demo_project() {
                Ok(project) => {
                    app.editor = Editor::new(Document::new(project));
                    let ids: Vec<_> = app.editor.sheet().entities.iter().map(|e| e.id).collect();
                    let ed = &mut app.editor;
                    let p = PointMm::new;
                    match launch.stage.as_deref() {
                        Some("segment") => {
                            ed.set_tool(Tool::Segment);
                            ed.click(p(20.0, 30.0), 2.0, false);
                            ed.pointer_moved(p(90.0, 55.0));
                            ed.length_text = "60".into();
                        }
                        Some("perpendiculaire") => {
                            ed.set_tool(Tool::Perpendicular);
                            ed.click(p(60.0, 0.0), 2.0, false);
                            ed.click(p(0.0, 0.0), 2.0, false);
                            ed.pointer_moved(p(10.0, 30.0));
                            ed.length_text = "50".into();
                        }
                        Some("point") => {
                            ed.set_tool(Tool::PointOnSegment);
                            ed.click(p(60.0, 0.0), 2.0, false);
                            ed.pointer_moved(p(75.0, 4.0));
                        }
                        Some("accrochage") => {
                            ed.set_tool(Tool::Point);
                            ed.pointer_moved(p(60.5, 0.6));
                        }
                        Some("rectangle") => {
                            ed.set_tool(Tool::Rectangle);
                            ed.click(p(150.0, 10.0), 2.0, false);
                            ed.pointer_moved(p(230.0, 60.0));
                            ed.length_text = "90".into();
                        }
                        Some("transformer") => {
                            ed.selection.entities.extend(ids.iter().take(2));
                            app.open_transform(true);
                        }
                        Some("cran") => {
                            ed.set_tool(Tool::Notch);
                            ed.click(p(80.0, 0.0), 2.0, false);
                            ed.pointer_moved(p(60.0, -20.0));
                            ed.length_text = "75".into();
                        }
                        Some("angle") => ed.selection.entities.extend(ids.iter().take(2)),
                        // Marge de couture (US-05.1) : pièce avec marge uniforme ou progressive,
                        // section de l'inspecteur dépliée, rien de sélectionné.
                        Some(stage @ ("marge" | "marge-progressive")) => {
                            match crate::seam::demo_seam_project(stage == "marge-progressive") {
                                Ok(p) => {
                                    app.editor = Editor::new(Document::new(p));
                                    app.seam_open = true;
                                    app.fit_pending = true;
                                }
                                Err(e) => app.error_dialog = Some(core_error(&e)),
                            }
                        }
                        // Coupe nette (US-05.6) : pièce déclarée en coupe nette, section dépliée.
                        Some("coupe-nette") => match crate::seam::demo_net_cut_project() {
                            Ok(p) => {
                                app.editor = Editor::new(Document::new(p));
                                app.seam_open = true;
                                app.fit_pending = true;
                            }
                            Err(e) => app.error_dialog = Some(core_error(&e)),
                        },
                        // Bord sur le pli (PH-5b) : pièce d'exemple, marge 10 mm, bord gauche déclaré
                        // sur le pli et sélectionné, section « Marge de couture » dépliée.
                        Some("sur-le-pli") => match crate::seam::demo_fold_project() {
                            Ok((p, left)) => {
                                app.editor = Editor::new(Document::new(p));
                                app.editor.selection.entities = vec![left];
                                app.seam_open = true;
                                app.fit_pending = true;
                            }
                            Err(e) => app.error_dialog = Some(core_error(&e)),
                        },
                        // Marges par matière (US-05.5) : pièce en cuir sans marge, jeu du projet
                        // personnalisé, section et sous-bloc dépliés.
                        Some("matieres") => match crate::seam::demo_materials_project() {
                            Ok(p) => {
                                app.editor = Editor::new(Document::new(p));
                                app.seam_open = true;
                                app.materials_open = true;
                                app.fit_pending = true;
                            }
                            Err(e) => app.error_dialog = Some(core_error(&e)),
                        },
                        // Types de coins (US-05.2) : parallélogramme avec trois coins choisis, le
                        // tracé du sommet 1 sélectionné, section dépliée.
                        Some("coins") => match crate::seam::demo_corners_project() {
                            Ok((p, entity)) => {
                                app.editor = Editor::new(Document::new(p));
                                app.editor.selection.entities.push(entity);
                                app.seam_open = true;
                                app.corners_focus = true;
                                app.fit_pending = true;
                            }
                            Err(e) => app.error_dialog = Some(core_error(&e)),
                        },
                        // Ourlets et rempliés (US-05.3) : trapèze avec ourlet, ou quartier de disque
                        // avec rempli et crans ; section dépliée, tracé réglé sélectionné.
                        Some(stage @ ("ourlet" | "rempli")) => {
                            match crate::hem::demo_hem_project(stage == "rempli") {
                                Ok((p, entity)) => {
                                    app.editor = Editor::new(Document::new(p));
                                    app.editor.selection.entities.push(entity);
                                    app.seam_open = true;
                                    app.corners_focus = true;
                                    app.fit_pending = true;
                                }
                                Err(e) => app.error_dialog = Some(core_error(&e)),
                            }
                        }
                        // Crans avancés (US-06.1) : trapèze à 12 mm de marge, crans U, V et T sur la
                        // coupe, fente sur la couture ; le V du bord incliné est sélectionné.
                        Some("cran-coupe") => match crate::notch_shape::demo_notch_project() {
                            Ok((p, notch)) => {
                                app.editor = Editor::new(Document::new(p));
                                app.editor.selection.notches.push(notch);
                                app.fit_pending = true;
                            }
                            Err(e) => app.error_dialog = Some(core_error(&e)),
                        },
                        // Export couture / coupe (US-05.4) : quartier de disque à rempli (coupe
                        // calculée et crans), fenêtre de mise en page ouverte, section « Contenu de
                        // la sortie » dépliée, mode « coupe seule » choisi (pas encore appliqué).
                        Some("export") => match crate::hem::demo_hem_project(true) {
                            Ok((p, _)) => {
                                app.editor = Editor::new(Document::new(p));
                                app.fit_pending = true;
                                app.open_print_setup();
                                if let Some(d) = app.print_dialog.as_mut() {
                                    d.content.piece_mode = pf_export::PieceExportMode::CutOnly;
                                }
                            }
                            Err(e) => app.error_dialog = Some(core_error(&e)),
                        },
                        // Pince (US-07.1) : pièce de 200 x 150 mm, pince de 24 mm d'ouverture
                        // sélectionnée (aperçu fermé, capuchon et mesures), outil Pince actif.
                        Some("pince") => match crate::dart::demo_dart_project() {
                            Ok((p, piece, dart)) => {
                                app.editor = Editor::new(Document::new(p));
                                app.editor.set_tool(Tool::Dart);
                                app.editor.dart_selected = Some((piece, dart));
                                app.fit_pending = true;
                            }
                            Err(e) => app.error_dialog = Some(core_error(&e)),
                        },
                        // Coupe pince fermée (PH-5a) : disque, marge 30 mm, pince au bas réglée
                        // « Couper pince fermée », fenêtre « Pince » ouverte.
                        Some("pince-coupe-fermee") => match crate::dart::demo_dart_cut_project() {
                            Ok((p, piece, dart)) => {
                                app.editor = Editor::new(Document::new(p));
                                app.editor.set_tool(Tool::Dart);
                                app.editor.dart_selected = Some((piece, dart));
                                app.fit_pending = true;
                            }
                            Err(e) => app.error_dialog = Some(core_error(&e)),
                        },
                        // Plis (US-07.3) : jupe 200 x 120, trois plis plats posés, fenêtre « Plis »
                        // ouverte avec la pièce repliée affichée.
                        Some("pli") => match crate::pleat::demo_pleat_project() {
                            Ok((p, piece, sheet)) => {
                                app.editor = Editor::new(Document::new(p));
                                app.editor.set_sheet(sheet);
                                app.editor.pleat.open = true;
                                app.editor.pleat.piece = Some(piece);
                                app.editor.pleat.show_folded = true;
                                app.fit_pending = true;
                            }
                            Err(e) => app.error_dialog = Some(core_error(&e)),
                        },
                        // Plis, pose d'un pli creux : ligne posée, aperçu de la largeur ajoutée.
                        Some("pli-creux") => match crate::pleat::demo_pleat_project() {
                            Ok((p, piece, sheet)) => {
                                app.editor = Editor::new(Document::new(p));
                                app.editor.set_sheet(sheet);
                                let ui = &mut app.editor.pleat;
                                ui.open = true;
                                ui.piece = Some(piece);
                                ui.kind = pf_core::PleatKind::Hollow;
                                ui.sense = pf_geometry::Side::Right;
                                ui.depth = "6".into();
                                ui.from = Some(pf_geometry::PointMm::new(190.0, 0.0));
                                ui.to = Some(pf_geometry::PointMm::new(190.0, 120.0));
                                app.fit_pending = true;
                            }
                            Err(e) => app.error_dialog = Some(core_error(&e)),
                        },
                        // Ampleur (US-07.4), avant la pose : jupe 200 x 100, fenêtre ouverte avec
                        // l'aperçu des longueurs (trois lignes, 30 mm en haut).
                        Some("ampleur-saisie") => match crate::flare::demo_flare_project() {
                            Ok((p, piece, _sheet)) => {
                                app.editor = Editor::new(Document::new(p));
                                app.open_flare();
                                app.editor.flare.piece = Some(piece);
                                app.fit_pending = true;
                            }
                            Err(e) => app.error_dialog = Some(core_error(&e)),
                        },
                        // Ampleur (US-07.4), après la pose : trois lignes de 30 mm, fenêtre ouverte.
                        Some("ampleur") => match crate::flare::demo_flare_project() {
                            Ok((p, piece, _sheet)) => {
                                app.editor = Editor::new(Document::new(p));
                                app.open_flare();
                                app.editor.flare.piece = Some(piece);
                                app.editor.flare_apply();
                                app.fit_pending = true;
                            }
                            Err(e) => app.error_dialog = Some(core_error(&e)),
                        },
                        // Découpe (US-07.5), avant la pose : rectangle 200 x 100, ligne oblique
                        // déjà tracée choisie, aperçu des deux morceaux et des crans.
                        Some("decoupe") => match crate::cutline::demo_cut_project() {
                            Ok((p, piece)) => {
                                app.editor = Editor::new(Document::new(p));
                                app.open_cutline();
                                app.editor.cutline.piece = Some(piece);
                                app.editor.cutline.line =
                                    app.editor.project().cut_candidates(piece).first().copied();
                                app.fit_pending = true;
                            }
                            Err(e) => app.error_dialog = Some(core_error(&e)),
                        },
                        // Découpe (US-07.5), après la pose : deux morceaux avec leurs crans.
                        Some("decoupe-posee") => match crate::cutline::demo_cut_project() {
                            Ok((p, piece)) => {
                                app.editor = Editor::new(Document::new(p));
                                app.open_cutline();
                                app.editor.cutline.piece = Some(piece);
                                app.editor.cutline.line =
                                    app.editor.project().cut_candidates(piece).first().copied();
                                app.editor.cutline_apply();
                                app.fit_pending = true;
                            }
                            Err(e) => app.error_dialog = Some(core_error(&e)),
                        },
                        // Transfert de pince (US-07.2) : moitié de l'angle vers le bord gauche.
                        Some("pince-transfert") => {
                            match crate::dart::demo_dart_transfer_project() {
                                Ok((p, piece, dart, sheet)) => {
                                    app.editor = Editor::new(Document::new(p));
                                    app.editor.set_sheet(sheet);
                                    app.editor.set_tool(Tool::Dart);
                                    app.editor.dart_selected = Some((piece, dart));
                                    app.editor.dart_transfer.partial = true;
                                    app.fit_pending = true;
                                }
                                Err(e) => app.error_dialog = Some(core_error(&e)),
                            }
                        }
                        None => ed.selection.entities.extend(ids.last()),
                        // Étapes de PH-1 (`stages.rs`) ; un nom inconnu ouvre une erreur.
                        Some(other) => {
                            if !crate::stages::apply(&mut app, other) {
                                app.error_dialog = Some(format!("unknown stage: {other}"));
                            }
                        }
                    }
                }
                Err(e) => app.error_dialog = Some(core_error(&e)),
            }
        }
        app
    }

    fn title(&self) -> String {
        let star = if self.editor.doc.is_dirty() {
            " •"
        } else {
            ""
        };
        format!("{}{star} — PatternForge", self.editor.project().name)
    }

    // ----- fichiers ------------------------------------------------------

    pub(crate) fn replace_document(&mut self, doc: Document) {
        // Le filtre et le mode lasso sont des réglages d'éditeur : ils survivent au changement de document.
        let (filter, lasso_mode) = (self.editor.select_filter, self.editor.lasso_mode);
        self.editor = Editor::new(doc);
        self.editor.select_filter = filter;
        self.editor.lasso_mode = lasso_mode;
        self.fit_pending = true;
        self.window_start = None;
        self.lasso = None;
        self.renaming = None;
        self.text_buf = None;
        self.internal_report = None;
        self.derived_form = None;
        self.marking_form = None;
        self.pitch_form = None;
        self.stitch_form = None;
        self.zone_form = None;
        self.button_form = None;
        self.leave_plan();
        self.autosaved = None;
        self.file_lock = None;
        self.history = crate::history::HistoryLog::default();
    }

    /// Propose la reprise d'une copie de secours du document courant (par son
    /// chemin, ou l'emplacement « sans nom »). Une copie illisible est signalée.
    pub fn check_recovery(&mut self) {
        let path = self.editor.doc.path.clone();
        match recovery::find_recovery(path.as_deref(), self.recovery_dir.as_deref()) {
            RecoveryCheck::Available(recovery) => {
                self.recovery_prompt = Some(RecoveryPrompt { recovery, path });
            }
            RecoveryCheck::Corrupt {
                autosave,
                moved_to,
                detail,
            } => {
                self.error_dialog = Some(tr_args(
                    "recovery.corrupt",
                    &[
                        ("path", &autosave.display().to_string()),
                        ("detail", &detail),
                        ("moved", &moved_to.display().to_string()),
                    ],
                ));
            }
            RecoveryCheck::None => {}
        }
    }

    /// Supprime la copie de secours du document courant.
    fn discard_autosave(&mut self) {
        recovery::discard_recovery(
            self.editor.doc.path.as_deref(),
            self.recovery_dir.as_deref(),
        );
        self.autosaved = None;
    }

    /// Copie de secours périodique : seulement si le document est modifié
    /// depuis la dernière copie ; jamais le fichier principal ; sans effet sur
    /// l'historique. Document redevenu identique au fichier : copie supprimée.
    pub fn autosave_tick(&mut self, now: Instant) {
        let Some(interval) = self.autosave_interval else {
            return;
        };
        if self.recovery_prompt.is_some()
            || now.saturating_duration_since(self.last_autosave) < interval
        {
            return;
        }
        self.last_autosave = now;
        if !self.editor.doc.is_dirty() {
            if self.autosaved.is_some() {
                self.discard_autosave();
            }
            return;
        }
        if self.autosaved.as_ref() == Some(self.editor.project()) {
            return;
        }
        match recovery::write_autosave(
            self.editor.doc.path.as_deref(),
            self.recovery_dir.as_deref(),
            self.editor.project(),
        ) {
            Ok(_) => self.autosaved = Some(self.editor.project().clone()),
            Err(e) => self.editor.error(tr_args(
                "autosave.error",
                &[("detail", &crate::editor::capitalize(&e.to_string()))],
            )),
        }
    }

    fn open_path(&mut self, path: &Path) {
        self.open_with_lock(path);
    }

    /// Ouvre `path` ; `lock` est le verrou déjà pris (ou `None` : choix d'ouvrir sans verrou).
    pub(crate) fn open_loaded(&mut self, path: &Path, lock: Option<pf_core::file_guard::FileLock>) {
        if self.open_job.is_some() {
            return;
        }
        // US-17.4 : un gros fichier s'ouvre dans un fil séparé (progression, annulation).
        let big = std::fs::metadata(path).is_ok_and(|m| m.len() >= self.open_async_bytes);
        if big {
            self.start_open_job(path, lock, None);
            return;
        }
        match Document::open(path) {
            Ok(doc) => self.finish_open(path, doc, lock),
            Err(e) => self.open_failed(path, &e),
        }
    }

    pub(crate) fn finish_open(
        &mut self,
        path: &Path,
        doc: Document,
        lock: Option<pf_core::file_guard::FileLock>,
    ) {
        self.replace_document(doc);
        self.file_lock = lock;
        self.editor.status.text = tr_args("file.opened", &[("path", &path.display().to_string())]);
        self.check_recovery();
    }

    pub(crate) fn open_failed(&mut self, path: &Path, e: &pf_core::CoreError) {
        self.error_dialog = Some(tr_args(
            "file.open_error",
            &[
                ("path", &path.display().to_string()),
                ("detail", &crate::editor::capitalize(&e.to_string())),
            ],
        ));
    }

    /// Ouvre le projet source (migration v1/v2 comprise) et propose ses feuilles.
    pub fn begin_import(&mut self, path: &Path) {
        match pf_core::persist::load(path) {
            Ok(source) => {
                self.import = Some(ImportDialog {
                    path: path.to_path_buf(),
                    source,
                })
            }
            Err(e) => {
                self.error_dialog = Some(tr_args(
                    "file.open_error",
                    &[
                        ("path", &path.display().to_string()),
                        ("detail", &crate::editor::capitalize(&e.to_string())),
                    ],
                ));
            }
        }
    }

    fn project_filter() -> rfd::FileDialog {
        rfd::FileDialog::new().add_filter(tr("file.filter"), &["json"])
    }

    pub(crate) fn safe_file_stem(name: &str) -> String {
        let s: String = name
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || matches!(c, '-' | '_' | ' ') {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let s = s.trim().to_string();
        if s.is_empty() {
            tr("file.default_stem")
        } else {
            s
        }
    }

    /// Enregistre ; renvoie `true` si le projet est bien sur disque.
    pub(crate) fn save(&mut self, force_dialog: bool) -> bool {
        let path = match (&self.editor.doc.path, force_dialog) {
            (Some(p), false) => Some(p.clone()),
            _ => Self::project_filter()
                .set_file_name(format!(
                    "{}.{FILE_EXTENSION}",
                    Self::safe_file_stem(&self.editor.project().name)
                ))
                .save_file(),
        };
        let Some(path) = path else { return false };
        self.commit_save(&path, false)
    }

    /// Écrit le projet dans `path`. Le fichier du document est d'abord contrôlé (modifié ou
    /// supprimé ailleurs : refus sans écriture, choix proposé) sauf si `forced` (écrasement
    /// choisi) ; un autre chemin ne s'écrit que si son verrou est libre (US-17.3).
    pub(crate) fn commit_save(&mut self, path: &Path, forced: bool) -> bool {
        use pf_core::file_guard::{FileLock, LockError};
        use pf_core::SaveError;
        let old_path = self.editor.doc.path.clone();
        let same = old_path.as_deref() == Some(path);
        let mut new_lock = None;
        if !same {
            match FileLock::acquire(path, crate::safeguard::unix_now()) {
                Ok(lock) => new_lock = Some(lock),
                Err(LockError::Held(_)) => {
                    self.error_dialog = Some(tr("file.save_locked"));
                    return false;
                }
                Err(LockError::Io(_)) => {}
            }
        }
        let result = if forced {
            self.editor.doc.save_as(path).map_err(SaveError::from)
        } else {
            self.editor.doc.save_checked(path)
        };
        match result {
            Ok(()) => {
                if !same {
                    self.file_lock = new_lock;
                }
                // Enregistrement normal : les copies de secours n'ont plus lieu d'être.
                let dir = self.recovery_dir.as_deref();
                recovery::discard_recovery(old_path.as_deref(), dir);
                recovery::discard_recovery(Some(path), dir);
                recovery::discard_recovery(None, dir);
                self.autosaved = None;
                self.editor.status.text =
                    tr_args("file.saved", &[("path", &path.display().to_string())]);
                self.editor.status.error = false;
                if let Some(old) = old_path.as_deref().filter(|_| !same) {
                    self.copy_project_assets(old, path);
                }
                true
            }
            Err(SaveError::External(change)) => {
                let key = if change == pf_core::ExternalChange::Removed {
                    "errlog.conflict.removed"
                } else {
                    "errlog.conflict.modified"
                };
                self.log_error("conflict", &tr(key));
                self.conflict_prompt = Some(crate::safeguard::ConflictPrompt {
                    path: path.to_path_buf(),
                    change,
                });
                false
            }
            Err(SaveError::Core(e)) => {
                self.error_dialog = Some(tr_args(
                    "file.save_error",
                    &[("detail", &crate::editor::capitalize(&e.to_string()))],
                ));
                false
            }
        }
    }

    /// « Enregistrer sous » : copie les images de fond à côté du nouveau fichier. Le projet est
    /// déjà enregistré ; tout fichier non copié est nommé dans la boîte d'erreur.
    fn copy_project_assets(&mut self, old: &Path, new: &Path) {
        use crate::assets_copy::{copy_assets, CopyProblem};
        let problems = copy_assets(self.editor.project(), old, new);
        if problems.is_empty() {
            return;
        }
        let lines: Vec<String> = problems
            .iter()
            .map(|problem| match problem {
                CopyProblem::Conflict(file) => tr_args("file.assets.conflict", &[("file", file)]),
                CopyProblem::Failed(file, why) => {
                    tr_args("file.assets.error", &[("file", file), ("why", why)])
                }
            })
            .collect();
        self.error_dialog = Some(tr_args(
            "file.assets.partial",
            &[
                ("path", &new.display().to_string()),
                ("detail", &lines.join("\n")),
            ],
        ));
    }

    /// Lance une action qui abandonnerait des modifications non enregistrées.
    fn request(&mut self, action: Pending, ctx: &egui::Context) {
        if self.editor.doc.is_dirty() {
            self.pending = Some(action);
        } else {
            self.perform(action, ctx);
        }
    }

    pub(crate) fn perform(&mut self, action: Pending, ctx: &egui::Context) {
        match action {
            Pending::New => {
                self.replace_document(Document::new(Project::new(&tr("project.default_name"))));
            }
            Pending::Open => {
                if let Some(path) = Self::project_filter().pick_file() {
                    self.open_path(&path);
                }
            }
            Pending::Quit => {
                self.allow_close = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }

    fn write_export(
        &mut self,
        what: &str,
        default_name: &str,
        ext: &str,
        bytes: &[u8],
        note: &str,
    ) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter(what, &[ext])
            .set_file_name(default_name)
            .save_file()
        else {
            return;
        };
        match std::fs::write(&path, bytes) {
            Ok(()) => {
                self.editor.status.text = tr_args(
                    "export.done",
                    &[
                        ("what", what),
                        ("note", note),
                        ("path", &path.display().to_string()),
                    ],
                );
                self.editor.status.error = false;
            }
            Err(e) => {
                self.error_dialog = Some(tr_args("export.failed", &[("detail", &e.to_string())]));
            }
        }
    }

    /// Scène exportée d'une feuille : le seul chemin de l'export PDF, de l'export SVG, du nombre
    /// de pages estimé et de l'aperçu (US-13.4). Elle applique le contenu de sortie choisi.
    pub fn export_scene(&self, sheet: SheetId) -> Result<Scene, pf_export::ExportError> {
        // US-09.8 : la taille exportée (la base par défaut : mêmes octets qu'avant).
        pf_export::size_export::size_scene(
            self.editor.project(),
            self.effective_export_size(),
            sheet,
            &self.output_content,
            // Langue de l'interface pour le libellé « SUR LE PLI » / « ON FOLD » (PH-5b).
            pf_export::SceneOptions {
                lang: pf_i18n::lang(),
                ..pf_export::SceneOptions::default()
            },
        )
    }

    /// Scènes du PDF : feuille courante ou toutes les feuilles. Une feuille vide, ou dont le
    /// contenu choisi ne laisse rien, est omise quand on exporte tout (le second nombre les
    /// compte) ; avec une seule feuille c'est un refus.
    pub fn pdf_scenes(&self, all_sheets: bool) -> Result<(Vec<Scene>, usize), String> {
        let project = self.editor.project();
        let ids: Vec<SheetId> = if all_sheets {
            project.sheets.iter().map(|s| s.id).collect()
        } else {
            vec![self.editor.sheet]
        };
        let (mut scenes, mut skipped, mut none_selected) = (Vec::new(), 0, None);
        for id in ids {
            match self.export_scene(id) {
                Ok(scene) => scenes.push(scene),
                Err(pf_export::ExportError::EmptySheet(_)) if all_sheets => skipped += 1,
                Err(e @ pf_export::ExportError::NothingSelected(_)) if all_sheets => {
                    skipped += 1;
                    none_selected = Some(e);
                }
                Err(e) => return Err(crate::print_setup::export_error(&e)),
            }
        }
        if scenes.is_empty() {
            return Err(match none_selected {
                Some(e) => crate::print_setup::export_error(&e),
                None => tr("export.nothing"),
            });
        }
        Ok((scenes, skipped))
    }

    fn export_svg(&mut self) {
        let scene = match self.export_scene(self.editor.sheet) {
            Ok(scene) => scene,
            Err(e) => {
                self.editor.error(crate::print_setup::export_error(&e));
                return;
            }
        };
        match svg::export_svg_lang(&scene, pf_i18n::lang()) {
            Ok(text) => {
                let stem = self.export_stem(&Self::safe_file_stem(&self.editor.sheet().name));
                let name = format!("{stem}.svg");
                let note = format!(
                    "{}{}{}",
                    tr("export.note.svg"),
                    self.export_size_note(),
                    scene_warnings_note(&scene)
                );
                self.write_export("SVG", &name, "svg", text.as_bytes(), &note);
            }
            Err(e) => self.editor.error(crate::print_setup::export_error(&e)),
        }
    }

    fn export_pdf(&mut self, all_sheets: bool) {
        let (scenes, skipped) = match self.pdf_scenes(all_sheets) {
            Ok(r) => r,
            Err(msg) => {
                self.editor.error(msg);
                return;
            }
        };
        match pdf::export_pdf_lang(
            &scenes,
            &self.page_setup,
            &self.output_content,
            pf_i18n::lang(),
        ) {
            Ok(out) => {
                let stem = if all_sheets {
                    Self::safe_file_stem(&self.editor.project().name)
                } else {
                    Self::safe_file_stem(&self.editor.sheet().name)
                };
                let stem = self.export_stem(&stem);
                let mut note = tr_args(
                    "export.note.pdf",
                    &[
                        ("pages", &out.page_count.to_string()),
                        ("format", &crate::print_setup::setup_label(&self.page_setup)),
                    ],
                );
                note.push_str(&self.export_size_note());
                if skipped > 0 {
                    note.push_str(&tr_args(
                        "export.note.skipped",
                        &[("count", &skipped.to_string())],
                    ));
                }
                for scene in &scenes {
                    note.push_str(&scene_warnings_note(scene));
                }
                self.write_export("PDF", &format!("{stem}.pdf"), "pdf", &out.bytes, &note);
            }
            Err(e) => self.editor.error(crate::print_setup::export_error(&e)),
        }
    }

    fn export_calibration(&mut self, as_svg: bool) {
        if as_svg {
            match svg::export_svg_lang(
                &Scene::calibration_square_lang(pf_i18n::lang()),
                pf_i18n::lang(),
            ) {
                Ok(text) => self.write_export(
                    "SVG",
                    "etalon-100mm.svg",
                    "svg",
                    text.as_bytes(),
                    &tr("export.note.calibration_svg"),
                ),
                Err(e) => self.editor.error(core_error(&e)),
            }
        } else {
            let out = match pdf::export_calibration_pdf_lang(&self.page_setup, pf_i18n::lang()) {
                Ok(out) => out,
                Err(e) => {
                    self.editor.error(crate::print_setup::export_error(&e));
                    return;
                }
            };
            self.write_export(
                "PDF",
                "etalon-100mm.pdf",
                "pdf",
                &out.bytes,
                &tr("export.note.calibration_pdf"),
            );
        }
    }

    // ----- clavier -------------------------------------------------------

    /// Ctrl+X / Ctrl+C / Ctrl+V (appelé hors saisie de texte seulement). egui-winit
    /// traduit ces touches en événements Cut / Copy / Paste(texte système) ;
    /// les touches brutes sont aussi acceptées (autres moteurs, tests).
    fn clipboard_shortcuts(&mut self, ctx: &egui::Context) {
        let (mut cut, mut copy, mut paste_text) = (false, false, None);
        ctx.input_mut(|i| {
            i.events.retain(|e| match e {
                egui::Event::Cut => {
                    cut = true;
                    false
                }
                egui::Event::Copy => {
                    copy = true;
                    false
                }
                egui::Event::Paste(text) => {
                    paste_text = Some(text.clone());
                    false
                }
                _ => true,
            })
        });
        let keymap = shortcuts::current_cmds();
        let hit = |cmd: shortcuts::Cmd| keymap.get(cmd).is_some_and(|b| b.pressed(ctx));
        cut |= hit(shortcuts::Cmd::Cut);
        copy |= hit(shortcuts::Cmd::Copy);
        let paste_key = hit(shortcuts::Cmd::Paste);
        if cut {
            self.editor.cut_selection();
        } else if copy {
            self.editor.copy_selection();
        }
        if let Some(text) = paste_text {
            self.editor.paste(Some(&text));
        } else if paste_key {
            self.editor.paste(None);
        }
        self.flush_clipboard(ctx);
    }

    /// Confie au presse-papiers système le dernier fragment copié.
    fn flush_clipboard(&mut self, ctx: &egui::Context) {
        if let Some(json) = self.editor.clipboard_out.take() {
            ctx.copy_text(json);
        }
    }

    /// Un dialogue ou une boîte modale est ouvert : le clavier ne va pas à l'éditeur.
    fn modal_open(&self) -> bool {
        self.pending.is_some()
            || self.conflict_prompt.is_some()
            || self.lock_prompt.is_some()
            || self.error_dialog.is_some()
            || self.recovery_prompt.is_some()
            || self.import.is_some()
            || self.transform.is_some()
            || self.assemble.is_some()
            || self.unfold.is_some()
            || self.level.is_some()
            || self.print_dialog.is_some()
            || self.calib_dialog.is_some()
            || self.dxf_dialog.is_some()
            || self.hpgl_dialog.is_some()
            || self.report_dialog.is_some()
            || self.dxf_import.is_some()
            || self.vector_import.is_some()
            || self.aama_import.is_some()
            || self.dxf_construction.is_some()
            || self.search.is_some()
    }

    /// Boîte modale du dessus (la dernière affichée par `dialogs`), s'il y en a une.
    fn top_modal(&self) -> Option<ModalKind> {
        if self.conflict_prompt.is_some() {
            Some(ModalKind::Conflict)
        } else if self.lock_prompt.is_some() {
            Some(ModalKind::Locked)
        } else if self.error_dialog.is_some() {
            Some(ModalKind::Error)
        } else if self.import.is_some() {
            Some(ModalKind::Import)
        } else if self.pending_delete.is_some() {
            Some(ModalKind::Delete)
        } else if self.pending.is_some() {
            Some(ModalKind::Unsaved)
        } else if self.recovery_prompt.is_some() {
            Some(ModalKind::Recovery)
        } else {
            None
        }
    }

    /// Entrée et Échap de la boîte `kind` si elle est celle du dessus : ignorés l'image où elle
    /// apparaît (une touche pressée pour autre chose ne la valide pas) et Entrée quand un
    /// bouton a le focus (il agit lui-même). Les touches sont consommées.
    pub(crate) fn modal_keys(&mut self, ctx: &egui::Context, kind: ModalKind) -> (bool, bool) {
        if self.top_modal() != Some(kind) {
            return (false, false);
        }
        let first = match self.modal_seen {
            Some((k, frame)) if k == kind => frame,
            _ => {
                self.modal_seen = Some((kind, self.frame));
                self.frame
            }
        };
        if self.frame <= first {
            return (false, false);
        }
        let focused = ctx.memory(|m| m.focused().is_some());
        let enter = ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter)) && !focused;
        let escape = ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape));
        (enter, escape)
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        if self.capture_key(ctx) {
            return;
        }
        if let Some(tool) = self.shortcut_capture {
            // Choix d'une lettre pour un outil : cette touche n'agit pas ailleurs.
            let pressed: Option<Key> = ctx.input(|i| {
                i.events.iter().find_map(|e| match e {
                    egui::Event::Key {
                        key,
                        pressed: true,
                        repeat: false,
                        ..
                    } => Some(*key),
                    _ => None,
                })
            });
            if let Some(key) = pressed {
                if key == Key::Escape {
                    self.shortcut_capture = None;
                } else {
                    self.assign_shortcut(tool, key);
                }
                ctx.input_mut(|i| i.events.clear());
            }
            return;
        }
        // Table centrale (US-16.3) : les commandes actives en toutes circonstances d'abord.
        // `Ctrl+Maj+Z` (second raccourci de « Rétablir ») et les combinaisons avec Maj passent
        // avant celles sans Maj, car egui fait aussi correspondre `Ctrl+Z` à `Ctrl+Maj+Z`.
        if shortcuts::REDO_ALIAS.pressed(ctx) {
            self.run_cmd(shortcuts::Cmd::Redo, ctx);
        }
        self.fire_commands(ctx, shortcuts::Guard::Always);
        // Dans le plan de pièces, les raccourcis de la feuille (presse-papiers, sélection,
        // outils, Suppr) sont inactifs : ils agiraient sur une feuille qu'on ne voit pas.
        if !ctx.wants_keyboard_input() && self.pending.is_none() && self.plan.is_none() {
            self.clipboard_shortcuts(ctx);
            // Sélection avancée (US-16.1) : Maj ajoute la chaîne, donc testée avant « Tout ».
            self.fire_commands(ctx, shortcuts::Guard::Canvas);
        }

        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            // Échap ferme la fenêtre d'outil la plus récente (PH-2b), sans effet sur le document ;
            // pas pendant une saisie de texte, une liste ouverte ou une boîte modale.
            let blocked = self.top_modal().is_some()
                || self.typing_last_frame
                || ctx.wants_keyboard_input()
                || egui::Popup::is_any_open(ctx);
            let tool_window = if blocked { None } else { self.dock.latest() };
            if let Some(win) = tool_window {
                self.close_tool_window(win);
                self.dock_sync();
            } else if !blocked && self.close_floating_window() {
                // Une fenêtre flottante (préférences, aide, export…) vient d'être fermée.
            } else if self.top_modal().is_none()
                && self.renaming.take().is_none()
                && self.pending.is_none()
            {
                self.window_start = None;
                self.lasso = None;
                if self.plan.is_some() {
                    self.plan_escape();
                } else {
                    self.editor.escape();
                }
            }
            ctx.memory_mut(|m| {
                if let Some(id) = m.focused() {
                    m.surrender_focus(id);
                }
            });
        }
        // Saisie directe (US-16.2) : les chiffres et signes tapés sans champ actif
        // alimentent la bulle près du curseur (mêmes champs que la palette).
        if !ctx.wants_keyboard_input()
            && self.canvas_hovered
            && !self.modal_open()
            && self.renaming.is_none()
            && self.editor.entry_active()
        {
            ctx.input_mut(|i| {
                i.events.retain(|e| match e {
                    egui::Event::Text(text) => {
                        text.chars().for_each(|c| self.editor.type_entry(c));
                        !text.chars().all(Editor::is_entry_char)
                    }
                    egui::Event::Key {
                        key: Key::Backspace,
                        pressed: true,
                        ..
                    } => {
                        self.editor.backspace_entry();
                        false
                    }
                    _ => true,
                })
            });
        }
        if ctx.input(|i| i.key_pressed(shortcuts::REPEAT_LAST)) && self.top_modal().is_none() {
            if self.editor.wants_confirm() {
                if self.renaming.is_none() {
                    self.editor.confirm();
                }
            } else if self.editor.tool == Tool::Select
                && ctx.input(|i| i.modifiers.is_none())
                && !ctx.wants_keyboard_input()
                && !self.modal_open()
                && self.renaming.is_none()
            {
                self.editor.repeat_last();
            }
        }

        // Raccourcis à une touche : seulement hors saisie de texte.
        if ctx.wants_keyboard_input() || self.pending.is_some() {
            return;
        }
        let cmd_keys = shortcuts::current_cmds();
        let cmd_pressed = |cmd: shortcuts::Cmd| cmd_keys.get(cmd).is_some_and(|b| b.pressed(ctx));
        let pressed = |key| ctx.input(|i| i.key_pressed(key) && i.modifiers.is_none());
        if cmd_pressed(shortcuts::Cmd::Help) {
            self.run_cmd(shortcuts::Cmd::Help, ctx);
        }
        if self.plan.is_some() {
            if cmd_pressed(shortcuts::Cmd::FitView) {
                self.run_cmd(shortcuts::Cmd::FitView, ctx);
            }
            return;
        }
        let keys = shortcuts::current();
        for tool in Tool::ALL {
            let letter = keys.letter(tool);
            if let Some((_, key)) = shortcuts::LETTERS.iter().find(|(c, _)| *c == letter) {
                if pressed(*key) {
                    self.editor.set_tool(tool);
                }
            }
        }
        for cmd in [
            shortcuts::Cmd::Delete,
            shortcuts::Cmd::FitView,
            shortcuts::Cmd::ZoomSelection,
            shortcuts::Cmd::RenameSheet,
        ] {
            if cmd_pressed(cmd) {
                self.run_cmd(cmd, ctx);
            }
        }
    }

    /// Déclenche les commandes de la table dont la combinaison est pressée : celles avec Maj
    /// d'abord, pour que `Ctrl+Maj+S` ne soit pas pris pour `Ctrl+S`.
    fn fire_commands(&mut self, ctx: &egui::Context, guard: shortcuts::Guard) {
        let keymap = shortcuts::current_cmds();
        let mut todo: Vec<(shortcuts::Cmd, shortcuts::Binding)> = shortcuts::Cmd::ALL
            .iter()
            .filter(|c| c.guard() == guard && !c.fixed())
            .filter(|c| {
                !matches!(
                    c,
                    shortcuts::Cmd::Cut | shortcuts::Cmd::Copy | shortcuts::Cmd::Paste
                )
            })
            .filter_map(|c| keymap.get(*c).map(|b| (*c, b)))
            .collect();
        todo.sort_by_key(|(_, b)| !b.shift);
        for (cmd, binding) in todo {
            if binding.pressed(ctx) {
                self.run_cmd(cmd, ctx);
            }
        }
    }

    /// Exécute une commande de la table. Le `match` n'a pas de joker : une commande ajoutée à
    /// `shortcuts::Cmd` sans action casse la compilation (test d'exhaustivité, US-16.3).
    pub(crate) fn run_cmd(&mut self, cmd: shortcuts::Cmd, ctx: &egui::Context) {
        use shortcuts::Cmd;
        #[cfg(test)]
        {
            self.cmd_log.push(cmd);
            let dialog = match cmd {
                Cmd::New | Cmd::Open | Cmd::SaveAs => true,
                Cmd::Save => self.editor.doc.path.is_none(),
                _ => false,
            };
            if dialog {
                return;
            }
        }
        match cmd {
            Cmd::New => self.request(Pending::New, ctx),
            Cmd::Open => self.request(Pending::Open, ctx),
            Cmd::Save => {
                let _ = self.save(false);
            }
            Cmd::SaveAs => {
                let _ = self.save(true);
            }
            Cmd::Undo => self.editor.undo(),
            Cmd::Redo => self.editor.redo(),
            Cmd::Find => self.open_search(),
            Cmd::Group => self.editor.group_selection(),
            Cmd::Ungroup => self.editor.ungroup_selection(),
            Cmd::Cut => {
                self.editor.cut_selection();
                self.flush_clipboard(ctx);
            }
            Cmd::Copy => {
                self.editor.copy_selection();
                self.flush_clipboard(ctx);
            }
            Cmd::Paste => self.editor.paste(None),
            Cmd::SelectAll => {
                let _ = self.editor.select_all();
            }
            Cmd::SelectChain => {
                let _ = self.editor.select_chain(true);
            }
            Cmd::InvertSelection => {
                let _ = self.editor.invert_selection();
            }
            Cmd::ToggleLasso => {
                let on = !self.editor.lasso_mode;
                self.editor.set_lasso_mode(on);
            }
            Cmd::Delete => self.request_delete(),
            Cmd::FitView => {
                if self.plan.is_some() {
                    self.plan_fit();
                } else {
                    self.fit_pending = true;
                }
            }
            Cmd::ZoomSelection => {
                if self.plan.is_none() {
                    self.zoom_to_selection();
                }
            }
            Cmd::RenameSheet => {
                let sheet = self.editor.sheet();
                self.renaming = Some((sheet.id, sheet.name.clone()));
                self.rename_focus = true;
            }
            Cmd::Help => self.show_help = !self.show_help,
            Cmd::RepeatLast => self.editor.repeat_last(),
            Cmd::CancelClose => self.editor.escape(),
        }
    }

    /// Échap ferme une fenêtre flottante hors dock (US-16.8), une par appui ; renvoie `true` si
    /// l'une d'elles était ouverte. Ne touche jamais au document.
    fn close_floating_window(&mut self) -> bool {
        for flag in [
            &mut self.show_prefs,
            &mut self.show_help,
            &mut self.show_customize,
            &mut self.show_history,
            &mut self.show_error_log,
            &mut self.show_glossary,
        ] {
            if *flag {
                *flag = false;
                return true;
            }
        }
        self.print_dialog.take().is_some()
            || self.calib_dialog.take().is_some()
            || self.dxf_dialog.take().is_some()
            || self.hpgl_dialog.take().is_some()
            || self.report_dialog.take().is_some()
            || self.dxf_import.take().is_some()
            || self.vector_import.take().is_some()
            || self.aama_import.take().is_some()
            || self.dxf_construction.take().is_some()
            || self.search.take().is_some()
    }

    /// Affecte une lettre à un outil, mémorise le choix hors projet.
    pub fn assign_shortcut(&mut self, tool: Tool, key: Key) {
        let mut keys = shortcuts::current();
        match keys.assign(tool, key) {
            Ok(()) => {
                shortcuts::set_current(keys);
                self.shortcut_error = None;
                self.shortcut_capture = None;
                self.save_shortcuts();
            }
            Err(shortcuts::AssignError::NotALetter) => {
                self.shortcut_error = Some(tr("help.shortcut.not_letter"));
            }
            Err(shortcuts::AssignError::Taken(other)) => {
                self.shortcut_error = Some(tr_args(
                    "help.shortcut.taken",
                    &[("letter", &keys.label(other)), ("tool", &other.label())],
                ));
            }
        }
    }

    /// Bascule clair / sombre et mémorise le choix hors projet.
    pub fn set_dark(&mut self, dark: bool) {
        self.dark = dark;
        if let Some(path) = &self.settings_path {
            let theme = if dark { "dark" } else { "light" };
            let _ = settings::save_value(path, "theme", serde_json::json!(theme));
        }
    }

    pub fn reset_shortcuts(&mut self) {
        shortcuts::set_current_keymap(shortcuts::Keymap::default());
        self.shortcut_error = None;
        self.shortcut_capture = None;
        self.save_shortcuts();
    }

    pub(crate) fn save_shortcuts(&self) {
        if let Some(path) = &self.settings_path {
            let _ = settings::save_value(
                path,
                shortcuts::SETTINGS_KEY,
                shortcuts::current_keymap().to_settings(),
            );
        }
    }

    // ----- menus et dialogues ----------------------------------------------

    /// Change la langue de toute l'interface et mémorise le choix hors projet.
    pub fn set_language(&mut self, lang: Lang) {
        pf_i18n::set_lang(lang);
        self.clear_extra_language_setting();
        pf_core::input::set_decimal_separator(lang.decimal_separator());
        let mut status = Status {
            text: tr("status.language_changed"),
            error: false,
        };
        if let Some(path) = &self.settings_path {
            if let Err(e) = settings::save_language(path, lang) {
                status = Status {
                    text: tr_args("status.language_save_failed", &[("detail", &e.to_string())]),
                    error: true,
                };
            }
        }
        self.editor.status = status;
    }

    fn menu_bar(&mut self, ctx: &egui::Context) {
        let item = |ui: &mut egui::Ui, text: String, shortcut: &str| {
            ui.add(egui::Button::new(text).shortcut_text(shortcut))
                .clicked()
        };
        egui::TopBottomPanel::top("menu").show(ctx, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button(tr("menu.file.title"), |ui| {
                    if item(ui, tr("menu.file.new"), "Ctrl+N") {
                        self.request(Pending::New, ctx);
                        ui.close();
                    }
                    if item(ui, tr("menu.file.open"), "Ctrl+O") {
                        self.request(Pending::Open, ctx);
                        ui.close();
                    }
                    if ui.button(tr("menu.file.demo")).clicked() {
                        ui.close();
                        if self.editor.doc.is_dirty() {
                            self.editor.error(tr("error.demo_dirty"));
                        } else if let Ok(p) = pf_core::demo_project() {
                            self.replace_document(Document::new(p));
                        }
                    }
                    ui.separator();
                    if ui.button(tr("menu.file.duplicate_sheet")).clicked() {
                        ui.close();
                        let sheet = self.editor.sheet;
                        self.editor.duplicate_sheet(sheet);
                        self.fit_pending = true;
                    }
                    if ui.button(tr("menu.file.import_sheet")).clicked() {
                        ui.close();
                        if let Some(path) = Self::project_filter().pick_file() {
                            self.begin_import(&path);
                        }
                    }
                    ui.separator();
                    if item(ui, tr("menu.file.save"), "Ctrl+S") {
                        self.save(false);
                        ui.close();
                    }
                    let save_as_keys = format!("Ctrl+{}+S", tr("key.shift"));
                    if item(ui, tr("menu.file.save_as"), &save_as_keys) {
                        self.save(true);
                        ui.close();
                    }
                    ui.separator();
                    if ui.button(tr("menu.file.export_pdf")).clicked() {
                        ui.close();
                        self.export_pdf(false);
                    }
                    if ui.button(tr("menu.file.export_pdf_all")).clicked() {
                        ui.close();
                        self.export_pdf(true);
                    }
                    if ui.button(tr("menu.file.print_setup")).clicked() {
                        ui.close();
                        self.open_print_setup();
                    }
                    if ui.button(tr("menu.file.export_svg")).clicked() {
                        ui.close();
                        self.export_svg();
                    }
                    if ui.button(tr("menu.file.export_sizes")).clicked() {
                        ui.close();
                        self.open_export_sizes();
                    }
                    if ui.button(tr("menu.file.variants")).clicked() {
                        ui.close();
                        self.open_variants();
                    }
                    if ui.button(tr("menu.file.cutting")).clicked() {
                        ui.close();
                        self.open_cutting();
                    }
                    if ui.button(tr("menu.file.library")).clicked() {
                        ui.close();
                        self.open_library();
                    }
                    if ui.button(tr("menu.file.naming")).clicked() {
                        ui.close();
                        self.open_naming();
                    }
                    if ui.button(tr("menu.file.bom")).clicked() {
                        ui.close();
                        self.open_bom();
                    }
                    if ui.button(tr("menu.file.versions")).clicked() {
                        ui.close();
                        self.open_versions();
                    }
                    ui.separator();
                    if ui.button(tr("menu.file.calibration_pdf")).clicked() {
                        ui.close();
                        self.export_calibration(false);
                    }
                    if ui.button(tr("menu.file.calibration_svg")).clicked() {
                        ui.close();
                        self.export_calibration(true);
                    }
                    ui.separator();
                    if ui.button(tr("menu.file.export_dxf")).clicked() {
                        ui.close();
                        self.open_dxf_export();
                    }
                    if ui.button(tr("menu.file.import_dxf_generic")).clicked() {
                        ui.close();
                        self.open_dxf_import();
                    }
                    if ui.button(tr("menu.file.import_vector")).clicked() {
                        ui.close();
                        self.open_vector_import();
                    }
                    if ui.button(tr("menu.file.import_aama")).clicked() {
                        ui.close();
                        self.open_aama_import();
                    }
                    if ui.button(tr("menu.file.export_dxf_construction")).clicked() {
                        ui.close();
                        self.open_construction_export();
                    }
                    if ui.button(tr("menu.file.export_hpgl")).clicked() {
                        ui.close();
                        self.open_hpgl_export();
                    }
                    if ui.button(tr("menu.file.export_report")).clicked() {
                        ui.close();
                        self.open_report_export();
                    }
                    ui.separator();
                    if ui.button(tr("menu.file.quit")).clicked() {
                        ui.close();
                        self.request(Pending::Quit, ctx);
                    }
                });
                ui.menu_button(tr("menu.edit.title"), |ui| {
                    if ui
                        .add_enabled(
                            self.editor.doc.can_undo(),
                            egui::Button::new(tr("menu.edit.undo")).shortcut_text("Ctrl+Z"),
                        )
                        .clicked()
                    {
                        self.editor.undo();
                        ui.close();
                    }
                    if ui
                        .add_enabled(
                            self.editor.doc.can_redo(),
                            egui::Button::new(tr("menu.edit.redo")).shortcut_text("Ctrl+Y"),
                        )
                        .clicked()
                    {
                        self.editor.redo();
                        ui.close();
                    }
                    if ui
                        .add_enabled(
                            self.editor.repeat_memo.is_some(),
                            egui::Button::new(tr("menu.edit.repeat"))
                                .shortcut_text(tr("key.enter")),
                        )
                        .clicked()
                    {
                        self.editor.repeat_last();
                        ui.close();
                    }
                    ui.separator();
                    if item(ui, tr("menu.edit.find"), "Ctrl+F") {
                        self.open_search();
                        ui.close();
                    }
                    self.selection_menu(ui);
                    ui.separator();
                    let has_sel = !self.editor.selection.is_empty();
                    if ui
                        .add_enabled(
                            has_sel,
                            egui::Button::new(tr("menu.edit.cut")).shortcut_text("Ctrl+X"),
                        )
                        .clicked()
                    {
                        self.editor.cut_selection();
                        ui.close();
                    }
                    if ui
                        .add_enabled(
                            has_sel,
                            egui::Button::new(tr("menu.edit.copy")).shortcut_text("Ctrl+C"),
                        )
                        .clicked()
                    {
                        self.editor.copy_selection();
                        ui.close();
                    }
                    if ui
                        .add_enabled(
                            self.editor.clipboard.is_some(),
                            egui::Button::new(tr("menu.edit.paste")).shortcut_text("Ctrl+V"),
                        )
                        .clicked()
                    {
                        self.editor.paste(None);
                        ui.close();
                    }
                    self.flush_clipboard(ctx);
                    ui.separator();
                    if ui
                        .add_enabled(
                            !self.editor.selection.is_empty(),
                            egui::Button::new(tr("menu.edit.delete"))
                                .shortcut_text(tr("key.delete")),
                        )
                        .clicked()
                    {
                        self.request_delete();
                        ui.close();
                    }
                    ui.separator();
                    if ui
                        .add_enabled(
                            self.editor.can_group(),
                            egui::Button::new(tr("menu.edit.group")).shortcut_text("Ctrl+G"),
                        )
                        .clicked()
                    {
                        self.editor.group_selection();
                        ui.close();
                    }
                    if ui
                        .add_enabled(
                            self.editor.can_ungroup(),
                            egui::Button::new(tr("menu.edit.ungroup"))
                                .shortcut_text(format!("Ctrl+{}+G", tr("key.shift"))),
                        )
                        .clicked()
                    {
                        self.editor.ungroup_selection();
                        ui.close();
                    }
                    ui.separator();
                    let has_selection = !self.editor.selection.is_empty();
                    if ui
                        .add_enabled(has_selection, egui::Button::new(tr("menu.edit.transform")))
                        .clicked()
                    {
                        self.open_transform(false);
                        ui.close();
                    }
                    if ui
                        .add_enabled(has_selection, egui::Button::new(tr("menu.edit.duplicate")))
                        .clicked()
                    {
                        self.open_transform(true);
                        ui.close();
                    }
                    ui.menu_button(tr("menu.edit.align"), |ui| {
                        use crate::align::AlignChoice as A;
                        for (choice, key) in [
                            (A::Left, "menu.align.left"),
                            (A::Right, "menu.align.right"),
                            (A::Top, "menu.align.top"),
                            (A::Bottom, "menu.align.bottom"),
                            (A::CenterHorizontal, "menu.align.center_h"),
                            (A::CenterVertical, "menu.align.center_v"),
                            (A::OnReference, "menu.align.reference"),
                        ] {
                            if ui.button(tr(key)).clicked() {
                                self.editor.align_selection(choice);
                                ui.close();
                            }
                        }
                    });
                    ui.menu_button(tr("menu.edit.distribute"), |ui| {
                        use pf_core::align::DistributeMode as D;
                        for (mode, key) in [
                            (D::Horizontal, "menu.distribute.horizontal"),
                            (D::Vertical, "menu.distribute.vertical"),
                            (D::AlongLine, "menu.distribute.line"),
                        ] {
                            if ui.button(tr(key)).clicked() {
                                self.editor.distribute_selection(mode);
                                ui.close();
                            }
                        }
                    });
                    if ui.button(tr("menu.edit.level_h")).clicked() {
                        self.open_level(true);
                        ui.close();
                    }
                    if ui.button(tr("menu.edit.level_v")).clicked() {
                        self.open_level(false);
                        ui.close();
                    }
                    ui.separator();
                    if ui.button(tr("menu.edit.mirror_link")).clicked() {
                        self.editor.link_mirror_selection();
                        ui.close();
                    }
                    if ui.button(tr("menu.edit.unfold")).clicked() {
                        self.editor.unfold_selection();
                        ui.close();
                    }
                    ui.separator();
                    for (tool, key) in [
                        (Tool::Extend, "menu.edit.extend"),
                        (Tool::Split, "menu.edit.split"),
                        (Tool::Merge, "menu.edit.merge"),
                        (Tool::Offset, "menu.edit.offset"),
                        (Tool::ExtractPiece, "menu.edit.extract_piece"),
                    ] {
                        let shortcut = tool.shortcut();
                        if ui
                            .add(egui::Button::new(tr(key)).shortcut_text(shortcut))
                            .clicked()
                        {
                            self.editor.set_tool(tool);
                            ui.close();
                        }
                    }
                    if ui
                        .add_enabled(
                            self.editor.selection_is_closed_contour(),
                            egui::Button::new(tr("piece.from_selection")),
                        )
                        .on_hover_text(tr("piece.from_selection.hover"))
                        .clicked()
                    {
                        self.editor.piece_from_selection();
                        ui.close();
                    }
                    if ui
                        .add_enabled(
                            !self.editor.selection.entities.is_empty(),
                            egui::Button::new(tr("menu.edit.reverse")),
                        )
                        .clicked()
                    {
                        self.editor.reverse_selection();
                        ui.close();
                    }
                    ui.separator();
                    // Séparer / assembler des pièces (US-04.6) : menu seulement, aucun raccourci.
                    let pieces: Vec<(pf_core::PieceId, String)> = self
                        .editor
                        .project()
                        .pieces_on(self.editor.sheet)
                        .into_iter()
                        .filter(|p| p.derived.is_none() && !p.outline.is_empty())
                        .map(|p| (p.id, p.name.clone()))
                        .collect();
                    ui.add_enabled_ui(!pieces.is_empty(), |ui| {
                        ui.menu_button(tr("menu.edit.split_piece"), |ui| {
                            for (id, name) in &pieces {
                                if ui.button(name).clicked() {
                                    self.editor.begin_split(*id);
                                    ui.close();
                                }
                            }
                        });
                    });
                    if ui
                        .add_enabled(
                            self.editor
                                .project()
                                .pieces
                                .iter()
                                .filter(|p| p.derived.is_none() && !p.outline.is_empty())
                                .count()
                                >= 2,
                            egui::Button::new(tr("menu.edit.join_pieces")),
                        )
                        .clicked()
                    {
                        self.open_assemble(None);
                        ui.close();
                    }
                    // Déplier une pièce construite sur une pliure (US-04.7) : menu seulement.
                    if ui
                        .add_enabled(
                            self.editor
                                .project()
                                .pieces
                                .iter()
                                .any(|p| p.derived.is_none() && !p.outline.is_empty()),
                            egui::Button::new(tr("menu.edit.unfold_piece")),
                        )
                        .on_hover_text(tr("unfoldpiece.intro"))
                        .clicked()
                    {
                        self.open_unfold(None);
                        ui.close();
                    }
                    // Plis (US-07.3) : menu seulement, aucun raccourci.
                    if ui
                        .add_enabled(
                            self.editor
                                .project()
                                .pieces
                                .iter()
                                .any(|p| p.derived.is_none() && !p.outline.is_empty()),
                            egui::Button::new(tr("menu.edit.pleats")),
                        )
                        .clicked()
                    {
                        self.open_pleats();
                        ui.close();
                    }
                    // Ampleur (US-07.4) : menu seulement, aucun raccourci.
                    if ui
                        .add_enabled(
                            self.editor
                                .project()
                                .pieces
                                .iter()
                                .any(|p| p.derived.is_none() && !p.outline.is_empty()),
                            egui::Button::new(tr("menu.edit.flare")),
                        )
                        .clicked()
                    {
                        self.open_flare();
                        ui.close();
                    }
                    // Découper selon une ligne (US-07.5) : menu seulement, aucun raccourci.
                    if ui
                        .add_enabled(
                            self.editor
                                .project()
                                .pieces
                                .iter()
                                .any(|p| p.derived.is_none() && !p.outline.is_empty()),
                            egui::Button::new(tr("menu.edit.cutline")),
                        )
                        .clicked()
                    {
                        self.open_cutline();
                        ui.close();
                    }
                    // Construction rapide (PH-4a) : menu seulement, aucun raccourci.
                    if ui.button(tr("menu.edit.quick")).clicked() {
                        self.open_quick();
                        ui.close();
                    }
                    // Coutures appariées (US-08.1) : menu seulement, aucun raccourci.
                    if ui
                        .add_enabled(
                            self.editor
                                .project()
                                .pieces
                                .iter()
                                .any(|p| !p.outline.is_empty()),
                            egui::Button::new(tr("menu.edit.seams")),
                        )
                        .clicked()
                    {
                        self.open_seams();
                        ui.close();
                    }
                    // Table de variables (US-03.1a) : menu seulement, aucun raccourci.
                    if ui.button(tr("menu.edit.variables")).clicked() {
                        self.open_variables();
                        ui.close();
                    }
                });
                ui.menu_button(tr("menu.view.title"), |ui| {
                    if item(ui, tr("menu.view.fit"), &tr("key.home")) {
                        self.fit_pending = true;
                        ui.close();
                    }
                    if item(ui, tr("menu.view.zoom_selection"), &tr("key.end")) {
                        self.zoom_to_selection();
                        ui.close();
                    }
                    if ui.button(tr("menu.view.zoom100")).clicked() {
                        self.view.zoom = crate::view::PX_PER_MM_AT_100;
                        ui.close();
                    }
                    let mut snap = self.editor.grid_snap_mm.is_some();
                    if ui.checkbox(&mut snap, tr("menu.view.snap_grid")).changed() {
                        self.editor.grid_snap_mm = snap.then_some(1.0);
                    }
                    ui.checkbox(&mut self.editor.snap_to_paths, tr("menu.view.snap_paths"));
                    ui.checkbox(&mut self.editor.show_guides, tr("menu.view.guides"));
                    ui.checkbox(&mut self.editor.show_rulers, tr("menu.view.rulers"));
                    ui.separator();
                    if ui
                        .selectable_label(self.plan.is_some(), tr("menu.view.plan"))
                        .on_hover_text(tr("plan.note"))
                        .clicked()
                    {
                        self.toggle_plan();
                        ui.close();
                    }
                    if ui.button(tr("menu.view.background")).clicked() {
                        self.open_background();
                        ui.close();
                    }
                    ui.separator();
                    ui.menu_button(tr("menu.view.theme"), |ui| {
                        for (dark, key) in [
                            (false, "menu.view.theme_light"),
                            (true, "menu.view.theme_dark"),
                        ] {
                            if ui.selectable_label(self.dark == dark, tr(key)).clicked() {
                                self.set_dark(dark);
                                ui.close();
                            }
                        }
                    });
                    ui.menu_button(tr("menu.view.language"), |ui| {
                        for lang in Lang::ALL {
                            let selected = pf_i18n::lang() == lang
                                && pf_i18n::langfile::active_code().is_none();
                            if ui.selectable_label(selected, lang.native_name()).clicked() {
                                self.set_language(lang);
                                ui.close();
                            }
                        }
                        let active = pf_i18n::langfile::active_code();
                        for (code, name) in pf_i18n::langfile::available() {
                            let label = format!("{name} {}", tr("lang.extra.suffix"));
                            if ui
                                .selectable_label(active.as_deref() == Some(code.as_str()), label)
                                .clicked()
                            {
                                self.set_extra_language(&code);
                                ui.close();
                            }
                        }
                    });
                    // Diagnostic des liens (US-03.5) : menu seulement, aucun raccourci.
                    ui.separator();
                    if ui.button(tr("menu.view.diagnostic")).clicked() {
                        self.open_diagnostic();
                        ui.close();
                    }
                    // Historique de construction (US-03.6) : menu seulement, aucun raccourci.
                    if ui.button(tr("menu.view.dependencies")).clicked() {
                        self.open_dependencies();
                        ui.close();
                    }
                    // Tailles du modèle (US-09.1) : menu seulement, aucun raccourci.
                    if ui.button(tr("menu.view.sizes")).clicked() {
                        self.open_sizes();
                        ui.close();
                    }
                    // Gradation par point (US-09.2a) : menu seulement, aucun raccourci.
                    if ui.button(tr("menu.view.grading")).clicked() {
                        self.open_grading();
                        ui.close();
                    }
                    // Mesures par taille (US-09.6a) : menu seulement, aucun raccourci.
                    if ui.button(tr("menu.view.size_measures")).clicked() {
                        self.open_size_measures();
                        ui.close();
                    }
                    // Barème de mesures (US-10.1) et fiches client (US-10.2) : menu seulement.
                    if ui.button(tr("menu.view.bareme")).clicked() {
                        self.open_bareme();
                        ui.close();
                    }
                    if ui.button(tr("menu.view.clients")).clicked() {
                        self.open_clients();
                        ui.close();
                    }
                    if ui.button(tr("menu.view.finished")).clicked() {
                        self.open_finished();
                        ui.close();
                    }
                    // Nid de tailles (US-09.4a) : case d'affichage et fenêtre, aucun raccourci.
                    ui.separator();
                    if ui
                        .selectable_label(self.nest.enabled, tr("menu.view.size_nest_show"))
                        .clicked()
                    {
                        self.nest.enabled = !self.nest.enabled;
                        ui.close();
                    }
                    if ui.button(tr("menu.view.size_nest")).clicked() {
                        self.open_size_nest();
                        ui.close();
                    }
                    // Contrôle de forme (US-09.9) : menu seulement, aucun raccourci.
                    if ui.button(tr("menu.view.shape_control")).clicked() {
                        self.open_shape_control();
                        ui.close();
                    }
                    ui.separator();
                    // Espaces de travail et personnalisation (US-16.4) : préférences hors projet.
                    ui.menu_button(tr("menu.view.workspace"), |ui| {
                        let current = self.layout.workspace;
                        if ui
                            .selectable_label(current.is_none(), tr("workspace.all"))
                            .clicked()
                        {
                            self.set_workspace(None);
                            ui.close();
                        }
                        for w in crate::workspace::Workspace::ALL {
                            if ui.selectable_label(current == Some(w), w.label()).clicked() {
                                self.set_workspace(Some(w));
                                ui.close();
                            }
                        }
                    });
                    if ui.button(tr("menu.view.history")).clicked() {
                        self.show_history = true;
                        ui.close();
                    }
                    if ui.button(tr("menu.view.views")).clicked() {
                        self.show_views = true;
                        ui.close();
                    }
                    if ui.button(tr("menu.view.prefs")).clicked() {
                        self.show_prefs = true;
                        ui.close();
                    }
                    if ui.button(tr("menu.view.customize")).clicked() {
                        self.show_customize = true;
                        ui.close();
                    }
                });
                ui.menu_button(tr("menu.help.title"), |ui| {
                    if item(ui, tr("menu.help.shortcuts"), "F1") {
                        self.show_help = true;
                        ui.close();
                    }
                    if ui.button(tr("menu.help.guide")).clicked() {
                        self.guide_step = Some(0);
                        ui.close();
                    }
                    if ui.button(tr("menu.help.error_log")).clicked() {
                        self.show_error_log = true;
                        ui.close();
                    }
                    if ui.button(tr("menu.help.glossary")).clicked() {
                        self.show_glossary = true;
                        ui.close();
                    }
                    ui.menu_button(tr("menu.help.examples"), |ui| {
                        for example in crate::help::Example::ALL {
                            if ui.button(example.label()).clicked() {
                                self.open_example(example);
                                ui.close();
                            }
                        }
                    });
                });
            });
        });
    }

    fn dialogs(&mut self, ctx: &egui::Context) {
        if self.top_modal().is_none() {
            self.modal_seen = None;
        }
        let keys_recovery = self.modal_keys(ctx, ModalKind::Recovery);
        let keys_unsaved = self.modal_keys(ctx, ModalKind::Unsaved);
        let keys_import = self.modal_keys(ctx, ModalKind::Import);
        let keys_conflict = self.modal_keys(ctx, ModalKind::Conflict);
        let keys_locked = self.modal_keys(ctx, ModalKind::Locked);
        let keys_error = self.modal_keys(ctx, ModalKind::Error);
        let keys_delete = self.modal_keys(ctx, ModalKind::Delete);
        if let Some(impact) = self.pending_delete.clone() {
            // Entrée = Supprimer ; Échap = Annuler (rien n'est modifié).
            let (mut confirm, mut cancel) = (keys_delete.0, keys_delete.1);
            egui::Modal::new(egui::Id::new("delete_confirm")).show(ctx, |ui| {
                ui.set_width(MODAL_WIDTH);
                modal_title(ui, &tr("dep.delete.title"));
                self.delete_modal_rows(ui, &impact);
                ui.horizontal(|ui| {
                    confirm |= button(ui, &tr("dep.delete.confirm"), ButtonKind::Primary).clicked();
                    cancel |= button(ui, &tr("dialog.cancel"), ButtonKind::Secondary).clicked();
                });
            });
            if cancel {
                self.pending_delete = None;
            } else if confirm {
                self.confirm_delete();
            }
        }
        if let Some(prompt) = &self.recovery_prompt {
            let (mut recover, mut ignore) = (false, false);
            let path_text = prompt
                .path
                .as_ref()
                .map_or_else(|| tr("recovery.unsaved"), |p| p.display().to_string());
            // Entrée = Récupérer ; pas d'Échap : « Ignorer » supprime la copie de secours.
            egui::Modal::new(egui::Id::new("recovery")).show(ctx, |ui| {
                ui.set_width(MODAL_WIDTH);
                modal_title(ui, &tr("recovery.title"));
                crate::a11y::label(
                    ui,
                    tr_args(
                        "recovery.body",
                        &[
                            ("name", &prompt.recovery.project.name),
                            ("path", &path_text),
                        ],
                    ),
                );
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    recover = button(ui, &tr("recovery.recover"), ButtonKind::Primary).clicked();
                    ignore = button(ui, &tr("recovery.ignore"), ButtonKind::Secondary).clicked();
                });
            });
            recover |= keys_recovery.0;
            if recover || ignore {
                if let Some(prompt) = self.recovery_prompt.take() {
                    if recover {
                        let saved = if prompt.path.is_some() {
                            self.editor.project().clone()
                        } else {
                            Project::new(&tr("project.default_name"))
                        };
                        let project = prompt.recovery.project;
                        self.replace_document(Document::recovered(
                            project.clone(),
                            prompt.path,
                            saved,
                        ));
                        self.autosaved = Some(project);
                        self.editor.status.text = tr("recovery.restored");
                    } else {
                        recovery::discard_recovery(
                            prompt.path.as_deref(),
                            self.recovery_dir.as_deref(),
                        );
                    }
                }
            }
        }
        if let Some(action) = self.pending.filter(|_| self.conflict_prompt.is_none()) {
            // Entrée = Enregistrer ; Échap = Annuler (l'action demandée n'a pas lieu : rien n'est
            // perdu). Jamais « Ne pas enregistrer » au clavier.
            let (mut save, mut discard, mut cancel) = (keys_unsaved.0, false, keys_unsaved.1);
            egui::Modal::new(egui::Id::new("unsaved")).show(ctx, |ui| {
                ui.set_width(MODAL_WIDTH);
                modal_title(ui, &tr("dialog.unsaved.title"));
                crate::a11y::label(
                    ui,
                    tr_args(
                        "dialog.unsaved.body",
                        &[("name", &self.editor.project().name)],
                    ),
                );
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    save |= button(ui, &tr("dialog.unsaved.save"), ButtonKind::Primary).clicked();
                    discard =
                        button(ui, &tr("dialog.unsaved.discard"), ButtonKind::Secondary).clicked();
                    cancel |= button(ui, &tr("dialog.cancel"), ButtonKind::Secondary).clicked();
                });
            });
            if cancel {
                self.pending = None;
            } else if save {
                if self.save(false) {
                    self.pending = None;
                    self.perform(action, ctx);
                }
            } else if discard {
                self.discard_autosave();
                self.pending = None;
                self.perform(action, ctx);
            }
        }
        if let Some(dialog) = &self.import {
            // Entrée = la première feuille de la liste (bouton principal) ; Échap = Annuler.
            let mut cancel = keys_import.1;
            let mut chosen: Option<SheetId> = if keys_import.0 {
                dialog.source.sheets.first().map(|s| s.id)
            } else {
                None
            };
            egui::Modal::new(egui::Id::new("import_sheet")).show(ctx, |ui| {
                ui.set_width(MODAL_WIDTH);
                modal_title(ui, &tr("import.title"));
                crate::a11y::label(
                    ui,
                    tr_args(
                        "import.body",
                        &[("path", &dialog.path.display().to_string())],
                    ),
                );
                ui.add_space(6.0);
                for (i, s) in dialog.source.sheets.iter().enumerate() {
                    let kind = if i == 0 {
                        ButtonKind::Primary
                    } else {
                        ButtonKind::Secondary
                    };
                    if button(ui, &s.name, kind).clicked() {
                        chosen = Some(s.id);
                    }
                }
                ui.add_space(8.0);
                if button(ui, &tr("dialog.cancel"), ButtonKind::Secondary).clicked() {
                    cancel = true;
                }
            });
            if let Some(id) = chosen {
                let result = dialog.source.import_command(id);
                self.import = None;
                match result {
                    Ok(cmd) => {
                        self.editor.import_sheet(cmd);
                        self.fit_pending = true;
                    }
                    Err(e) => self.editor.error(core_error(&e)),
                }
            } else if cancel {
                self.import = None;
            }
        }
        if let Some(message) = self.error_dialog.clone() {
            // Entrée ou Échap = Fermer.
            let mut close = keys_error.0 || keys_error.1;
            egui::Modal::new(egui::Id::new("error")).show(ctx, |ui| {
                ui.set_width(MODAL_WIDTH);
                modal_title(ui, "PatternForge");
                crate::a11y::label(ui, message);
                ui.add_space(8.0);
                close |= button(ui, &tr("dialog.close"), ButtonKind::Primary).clicked();
            });
            if close {
                self.error_dialog = None;
            }
        }
        self.safeguard_dialogs(ctx, keys_conflict, keys_locked);
        if self.show_help {
            let mut open = true;
            let mut capture: Option<Tool> = None;
            let mut reset = false;
            let rows: Vec<(String, String)> = vec![
                (tr("key.enter"), tr("help.desc.enter")),
                ("Tab".to_string(), tr("help.desc.tab")),
                (tr("key.escape"), tr("help.desc.escape")),
                (tr("key.shift_click"), tr("help.desc.shift_click")),
                (tr("key.shift"), tr("help.desc.angle_lock")),
                (tr("key.delete"), tr("menu.edit.delete")),
                ("Ctrl+Z / Ctrl+Y".to_string(), tr("help.desc.undo_redo")),
                (
                    format!("Ctrl+G / Ctrl+{}+G", tr("key.shift")),
                    tr("help.desc.group"),
                ),
                (
                    "Ctrl+X / Ctrl+C / Ctrl+V".to_string(),
                    tr("help.desc.clipboard"),
                ),
                ("Ctrl+F".to_string(), tr("menu.edit.find")),
                (
                    "Ctrl+A / Ctrl+I".to_string(),
                    tr("help.desc.select_all_invert"),
                ),
                (
                    format!("Ctrl+{}+A", tr("key.shift")),
                    tr("help.desc.select_chain"),
                ),
                ("Ctrl+L".to_string(), tr("help.desc.lasso")),
                (tr("key.alt_click"), tr("help.desc.alt_click")),
                ("Ctrl+S".to_string(), tr("menu.file.save")),
                (tr("key.wheel"), tr("help.desc.wheel")),
                (tr("key.pan"), tr("help.desc.pan")),
                (tr("key.home"), tr("menu.view.fit")),
                (tr("key.end"), tr("menu.view.zoom_selection")),
                ("F2".to_string(), tr("help.desc.rename")),
            ];
            egui::Window::new(tr("menu.help.shortcuts"))
                .id(egui::Id::new("help_window"))
                .open(&mut open)
                .resizable(false)
                .anchor(egui::Align2::CENTER_TOP, [0.0, crate::panels::WINDOW_TOP])
                .default_height(crate::panels::window_room(ctx) - 70.0)
                .show(ctx, |ui| {
                    // Défile : tout le contenu reste atteignable, sous le menu et la barre d'outils.
                    let room = crate::panels::window_room(ctx) - 70.0;
                    ui.spacing_mut().scroll = egui::style::ScrollStyle::solid();
                    egui::ScrollArea::vertical()
                        .id_salt("help_scroll")
                        .max_height(room)
                        .auto_shrink([true, true])
                        .show(ui, |ui| {
                            ui.strong(tr("help.shortcut.title"));
                            let keys = shortcuts::current();
                            // Deux colonnes d'outils côte à côte : la table tient sans défilement long.
                            let half = Tool::ALL.len().div_ceil(2);
                            egui::Grid::new("help_tools")
                                .num_columns(6)
                                .striped(true)
                                .show(ui, |ui| {
                                    for row in 0..half {
                                        for col in 0..2 {
                                            let Some(tool) =
                                                Tool::ALL.get(row + col * half).copied()
                                            else {
                                                continue;
                                            };
                                            ui.strong(keys.label(tool));
                                            crate::a11y::label(ui, tool.label());
                                            if self.shortcut_capture == Some(tool) {
                                                crate::a11y::label(ui, tr("help.shortcut.press"));
                                            } else if ui.button(tr("help.shortcut.edit")).clicked()
                                            {
                                                capture = Some(tool);
                                            }
                                        }
                                        ui.end_row();
                                    }
                                });
                            if let Some(err) = &self.shortcut_error {
                                ui.colored_label(crate::palette::pal().error, err);
                            }
                            if ui.button(tr("help.shortcut.reset")).clicked() {
                                reset = true;
                            }
                            ui.separator();
                            egui::Grid::new("help")
                                .num_columns(2)
                                .striped(true)
                                .show(ui, |ui| {
                                    for (keys, what) in rows {
                                        ui.strong(keys);
                                        crate::a11y::label(ui, what);
                                        ui.end_row();
                                    }
                                });
                            ui.separator();
                            crate::a11y::label(ui, tr("help.limits"));
                        });
                });
            self.show_help = open;
            if capture.is_some() {
                self.shortcut_capture = capture;
                self.shortcut_error = None;
            }
            if reset {
                self.reset_shortcuts();
            }
            if !open {
                self.shortcut_capture = None;
            }
        }
    }

    fn handle_screenshot(&mut self, ctx: &egui::Context) {
        let Some(path) = self.screenshot.clone() else {
            return;
        };
        ctx.request_repaint();
        if self.frame == 8 {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
        }
        let image = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(image) = image {
            let [w, h] = image.size;
            let result = image::save_buffer(
                &path,
                image.as_raw(),
                w as u32,
                h as u32,
                image::ExtendedColorType::Rgba8,
            );
            if let Err(e) = result {
                eprintln!("capture impossible : {e}");
            }
            self.screenshot = None;
            self.allow_close = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.ui(ctx);
    }
}

impl App {
    /// Une image de l'interface. Séparé d'`eframe` pour être pilotable en test.
    pub fn ui(&mut self, ctx: &egui::Context) {
        self.frame += 1;
        self.projection_escape(ctx);
        crate::a11y::forget();
        // La langue courante est la source unique : le séparateur décimal la suit.
        pf_core::input::set_decimal_separator(pf_i18n::lang().decimal_separator());
        crate::palette::set_dark(self.dark);
        let mut tokens = if self.dark {
            pf_ui::tokens::Tokens::precision_dark()
        } else {
            pf_ui::tokens::Tokens::precision_light()
        };
        self.prefs.apply_roles(&mut tokens);
        self.prefs.publish();
        self.apply_view_settings();
        self.editor.default_notch_mm = self.prefs.default_notch_mm;
        pf_ui::theme::apply(ctx, &tokens);
        // Échelle de l'interface (US-16.8) : facteur de `pixels_per_point`, 100 à 200 %.
        let zoom = self.prefs.ui_scale_pct as f32 / 100.0;
        if (ctx.zoom_factor() - zoom).abs() > 1e-4 {
            ctx.set_zoom_factor(zoom);
        }
        let title = self.title();
        if title != self.last_title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.last_title = title;
        }

        if ctx.input(|i| i.viewport().close_requested())
            && !self.allow_close
            && self.editor.doc.is_dirty()
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.pending = Some(Pending::Quit);
        }
        // Fichier déposé sur la fenêtre.
        let dropped = ctx.input(|i| i.raw.dropped_files.first().and_then(|f| f.path.clone()));
        if let Some(path) = dropped {
            if self.editor.doc.is_dirty() {
                self.editor.error(tr("error.drop_dirty"));
            } else {
                self.open_path(&path);
            }
        }

        self.autosave_tick(Instant::now());
        self.lock_tick(ctx);
        self.poll_open_job(ctx);
        self.log_new_errors();
        if let Some(interval) = self.autosave_interval {
            ctx.request_repaint_after(interval);
        }

        self.plan_refresh();
        self.editor.refresh_display();
        self.refresh_nest();
        self.dock_sync();
        self.shortcuts(ctx);
        self.menu_bar(ctx);
        self.toolbar(ctx);
        self.status_bar(ctx);
        self.sheets_panel(ctx);
        self.inspector_panel(ctx);
        self.dock_sync();
        self.dock_panel(ctx);
        self.canvas_panel(ctx);
        self.transform_window(ctx);
        self.assemble_window(ctx);
        self.unfold_window(ctx);
        self.pleat_window(ctx);
        self.flare_window(ctx);
        self.cutline_window(ctx);
        self.quick_window(ctx);
        self.seams_window(ctx);
        self.level_window(ctx);
        self.variables_window(ctx);
        self.diagnostic_window(ctx);
        self.dependencies_window(ctx);
        self.sizes_window(ctx);
        self.grading_window(ctx);
        self.size_measures_window(ctx);
        self.bareme_window(ctx);
        self.clients_window(ctx);
        self.finished_window(ctx);
        self.size_nest_window(ctx);
        self.export_sizes_window(ctx);
        self.bom_window(ctx);
        self.variants_window(ctx);
        self.naming_window(ctx);
        self.cutting_window(ctx);
        self.versions_window(ctx);
        self.direct_print_window(ctx);
        self.projection_window(ctx);
        self.trace_poll(ctx);
        self.background_window(ctx);
        self.library_window(ctx);
        self.shape_control_window(ctx);
        self.print_setup_window(ctx);
        self.calib_window(ctx);
        self.dxf_window(ctx);
        self.hpgl_window(ctx);
        self.report_window(ctx);
        self.dxf_import_window(ctx);
        self.vector_import_window(ctx);
        self.aama_import_window(ctx);
        self.dxf_construction_window(ctx);
        self.search_window(ctx);
        self.dialogs(ctx);
        self.guide_window(ctx);
        self.projection_overlay(ctx);
        self.customize_window(ctx);
        self.history_window(ctx);
        self.error_log_window(ctx);
        self.open_job_window(ctx);
        self.glossary_window(ctx);
        self.prefs_window(ctx);
        self.views_window(ctx);
        self.handle_screenshot(ctx);
        pf_ui::theme::paint_focus_ring(ctx);
        self.typing_last_frame = ctx.wants_keyboard_input();
    }
}

/// Intervalle lu dans `autosave_seconds` : absent ou invalide = 60 s, 0 = désactivée.
pub(crate) fn autosave_interval(value: Option<&serde_json::Value>) -> Option<Duration> {
    let secs = value
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(DEFAULT_AUTOSAVE_SECONDS);
    (secs > 0).then(|| Duration::from_secs(secs))
}
