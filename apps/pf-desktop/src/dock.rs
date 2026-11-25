//! Ancrage commun des fenêtres d'outil (PH-2b).
//!
//! Règle unique : tant qu'une fenêtre d'outil est ouverte, une colonne (le « dock ») est
//! réservée entre le canevas et l'inspecteur ; les fenêtres s'y empilent de haut en bas, dans
//! l'ordre d'ouverture, et ne recouvrent donc ni le panneau des feuilles, ni l'inspecteur, ni
//! le canevas (qui se rétrécit de la largeur du dock). Aucune fenêtre n'a d'ancrage propre :
//! toutes appellent [`slot`]. Échap ferme la plus récente. Pur état d'interface : rien n'est
//! écrit dans le projet.

use crate::app::App;
use eframe::egui::{self, Pos2, Rect};

/// Marge entre le bord du dock et une fenêtre, et entre deux fenêtres empilées.
pub(crate) const DOCK_GAP: f32 = 8.0;
/// Largeur ajoutée à la plus large fenêtre ouverte : marges du dock et cadre de la fenêtre.
const DOCK_PAD: f32 = 32.0;
/// Hauteur minimale d'une fenêtre empilée (cadre compris).
const MIN_HEIGHT: f32 = 160.0;
/// Hauteur ajoutée par une fenêtre à défilement à sa hauteur maximale de contenu (titre, cadre,
/// marges : mesurée à 75 points, 76 par prudence).
const WINDOW_CHROME: f32 = 76.0;
/// Part de l'écran qu'un dock ne dépasse jamais (le canevas garde le reste).
const MAX_SHARE: f32 = 0.5;

/// Largeur de la bande du panneau des feuilles replié (PH-2c).
pub(crate) const FOLDED_WIDTH: f32 = 34.0;
/// Largeur estimée du panneau des feuilles déployé (son maximum) pour décider du repli.
const SHEETS_ESTIMATE: f32 = 260.0;
/// Largeur de canevas que le repli automatique cherche à préserver (points).
pub(crate) const MIN_CANVAS: f32 = 400.0;

/// État du panneau Projet / Feuilles / Pièce (préférence d'affichage, jamais dans le projet).
/// `Auto` : replié en bande étroite tant qu'un dock ouvert laisserait moins de [`MIN_CANVAS`]
/// au canevas ; `Folded` : replié par le chevron (mémorisé dans les réglages) ; `Open` :
/// déployé par le chevron pour la session (aucun repli automatique).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum SheetsFold {
    #[default]
    Auto,
    Folded,
    Open,
}

/// Clé du réglage mémorisé (valeur « folded » ; toute autre valeur = automatique).
pub(crate) const SHEETS_SETTING: &str = "sheets_panel";

/// Les fenêtres d'outil rangées dans le dock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ToolWin {
    Pleat,
    Flare,
    Cutline,
    Quick,
    Seams,
    JoinView,
    Dart,
    Report,
    Unfold,
    Assemble,
    Transform,
    Level,
    Variables,
    Diagnostic,
    Dependencies,
    Sizes,
    Grading,
    SizeMeasures,
    Bareme,
    Clients,
    Finished,
    SizeNest,
    ShapeControl,
    ExportSizes,
    Bom,
    Variants,
    Versions,
    Cutting,
    Library,
    Naming,
    Print,
    Projection,
    Background,
}

impl ToolWin {
    pub(crate) const ALL: [ToolWin; 33] = [
        ToolWin::Pleat,
        ToolWin::Flare,
        ToolWin::Cutline,
        ToolWin::Quick,
        ToolWin::Seams,
        ToolWin::JoinView,
        ToolWin::Dart,
        ToolWin::Report,
        ToolWin::Unfold,
        ToolWin::Assemble,
        ToolWin::Transform,
        ToolWin::Level,
        ToolWin::Variables,
        ToolWin::Diagnostic,
        ToolWin::Dependencies,
        ToolWin::Sizes,
        ToolWin::Grading,
        ToolWin::SizeMeasures,
        ToolWin::Bareme,
        ToolWin::Clients,
        ToolWin::Finished,
        ToolWin::SizeNest,
        ToolWin::ShapeControl,
        ToolWin::ExportSizes,
        ToolWin::Bom,
        ToolWin::Variants,
        ToolWin::Versions,
        ToolWin::Cutting,
        ToolWin::Library,
        ToolWin::Naming,
        ToolWin::Print,
        ToolWin::Projection,
        ToolWin::Background,
    ];

    /// Identifiant egui de la zone de la fenêtre (le même que celui de sa fenêtre).
    pub(crate) fn id(self) -> egui::Id {
        egui::Id::new(match self {
            ToolWin::Pleat => "pleat_window",
            ToolWin::Flare => "flare_window",
            ToolWin::Cutline => "cutline_window",
            ToolWin::Quick => "quick_window",
            ToolWin::Seams => "seams_window",
            ToolWin::JoinView => "seams_join_view",
            ToolWin::Dart => "dart-window",
            ToolWin::Report => "report_window",
            ToolWin::Unfold => "unfold_window",
            ToolWin::Assemble => "assemble_window",
            ToolWin::Transform => "transform_window",
            ToolWin::Level => "level_window",
            ToolWin::Variables => "variables_window",
            ToolWin::Diagnostic => "diagnostic_window",
            ToolWin::Dependencies => "dependencies_window",
            ToolWin::Sizes => "sizes_window",
            ToolWin::Grading => "grading_window",
            ToolWin::SizeMeasures => "size_measures_window",
            ToolWin::Bareme => "bareme_window",
            ToolWin::Clients => "clients_window",
            ToolWin::Finished => "finished_window",
            ToolWin::SizeNest => "size_nest_window",
            ToolWin::ShapeControl => "shape_control_window",
            ToolWin::ExportSizes => "export_sizes_window",
            ToolWin::Bom => "bom_window",
            ToolWin::Variants => "variants_window",
            ToolWin::Versions => "versions_window",
            ToolWin::Cutting => "cutting_window",
            ToolWin::Library => "library_window",
            ToolWin::Naming => "naming_window",
            ToolWin::Print => "direct_print_window",
            ToolWin::Projection => "projection_window",
            ToolWin::Background => "background_window",
        })
    }

    /// Largeur du contenu de la fenêtre (points) : la plus large décide de celle du dock.
    pub(crate) fn width(self) -> f32 {
        match self {
            ToolWin::Pleat | ToolWin::Flare | ToolWin::Cutline | ToolWin::Quick => 330.0,
            ToolWin::Seams => 470.0,
            ToolWin::JoinView => 440.0,
            ToolWin::Dart => 270.0,
            ToolWin::Report => 420.0,
            ToolWin::Unfold => 380.0,
            ToolWin::Assemble => 380.0,
            ToolWin::Transform | ToolWin::Level => 300.0,
            ToolWin::Variables => 480.0,
            ToolWin::Diagnostic | ToolWin::Dependencies => 420.0,
            ToolWin::Sizes => 460.0,
            ToolWin::Grading => 460.0,
            ToolWin::SizeMeasures => 460.0,
            ToolWin::Bareme => 460.0,
            ToolWin::Clients => 460.0,
            ToolWin::Finished => 460.0,
            ToolWin::SizeNest => 460.0,
            ToolWin::ShapeControl => 460.0,
            ToolWin::ExportSizes => 460.0,
            ToolWin::Bom => 460.0,
            ToolWin::Variants => 460.0,
            ToolWin::Versions => 460.0,
            ToolWin::Cutting => 560.0,
            ToolWin::Library => 460.0,
            ToolWin::Naming => 460.0,
            ToolWin::Print | ToolWin::Projection => 420.0,
            ToolWin::Background => 460.0,
        }
    }
}

/// État du dock : fenêtres ouvertes (la plus récente en dernier) et rectangle réservé.
#[derive(Debug)]
pub struct ToolDock {
    order: Vec<ToolWin>,
    pub(crate) rect: Rect,
}

impl Default for ToolDock {
    fn default() -> Self {
        ToolDock {
            order: Vec::new(),
            rect: Rect::NOTHING,
        }
    }
}

impl ToolDock {
    /// Fenêtres ouvertes, de la plus ancienne à la plus récente.
    #[cfg(test)]
    pub(crate) fn open(&self) -> &[ToolWin] {
        &self.order
    }

    /// Fenêtre ouverte la plus récente.
    pub(crate) fn latest(&self) -> Option<ToolWin> {
        self.order.last().copied()
    }

    /// Largeur de contenu **commune** à toutes les fenêtres ouvertes du dock (PH-2c) : celle de
    /// la plus large, pour que le bord du dock soit droit (plus d'escalier 470 / 440).
    pub(crate) fn content_width(&self, win: ToolWin) -> f32 {
        self.order
            .iter()
            .map(|w| w.width())
            .fold(win.width(), f32::max)
    }

    /// Largeur réservée pour les fenêtres ouvertes (0 si aucune) : la plus large, mesurée à
    /// l'image précédente (largeur annoncée à la première), plus les marges ; bornée à la moitié
    /// de l'écran.
    pub(crate) fn reserved_width(&self, ctx: &egui::Context) -> f32 {
        let widest = self
            .order
            .iter()
            .map(|w| {
                let measured = ctx
                    .memory(|m| m.area_rect(w.id()))
                    .map_or(0.0, |r| r.width() + 2.0 * DOCK_GAP);
                (w.width() + DOCK_PAD).max(measured)
            })
            .fold(0.0_f32, f32::max);
        widest.min(ctx.screen_rect().width() * MAX_SHARE)
    }
}

/// Position et hauteur maximale d'une fenêtre dans le dock.
#[derive(Clone, Copy, Debug)]
pub(crate) struct DockSlot {
    pub pos: Pos2,
    pub max_height: f32,
}

/// **La** règle d'ancrage : place `win` dans le dock, sous les fenêtres plus anciennes
/// (hauteurs mesurées à l'image précédente) ; la hauteur disponible est partagée à parts égales.
pub(crate) fn slot(ctx: &egui::Context, dock: &ToolDock, win: ToolWin) -> DockSlot {
    let rect = if dock.rect.is_positive() {
        dock.rect
    } else {
        ctx.screen_rect()
    };
    let n = dock.order.len().max(1) as f32;
    let share = ((rect.height() - DOCK_GAP * (n + 1.0)) / n).max(MIN_HEIGHT);
    let mut y = rect.top() + DOCK_GAP;
    for earlier in dock.order.iter().take_while(|w| **w != win) {
        let height = ctx
            .memory(|m| m.area_rect(earlier.id()))
            .map_or(share, |r| r.height());
        y += height + DOCK_GAP;
    }
    DockSlot {
        pos: Pos2::new(rect.left() + DOCK_GAP, y),
        max_height: (share - WINDOW_CHROME).max(MIN_HEIGHT - WINDOW_CHROME),
    }
}

impl App {
    /// Le panneau des feuilles est-il affiché en bande étroite ? En mode automatique : quand un
    /// dock est ouvert et que, panneau déployé (260 pt au plus), il resterait moins de
    /// [`MIN_CANVAS`] au canevas. Décision sans hystérésis (estimation constante du panneau).
    pub(crate) fn sheets_folded(&self, ctx: &egui::Context) -> bool {
        match self.sheets_fold {
            SheetsFold::Folded => true,
            SheetsFold::Open => false,
            SheetsFold::Auto => {
                let dock = self.dock.reserved_width(ctx);
                if dock <= 0.0 {
                    return false;
                }
                let inspector =
                    egui::containers::panel::PanelState::load(ctx, egui::Id::new("inspector"))
                        .map_or(270.0, |s| s.rect.width());
                ctx.screen_rect().width() - SHEETS_ESTIMATE - inspector - dock < MIN_CANVAS
            }
        }
    }

    /// Chevron du panneau des feuilles : replie (mémorisé) ou déploie (pour la session).
    pub(crate) fn toggle_sheets_fold(&mut self, ctx: &egui::Context) {
        let next = if self.sheets_folded(ctx) {
            SheetsFold::Open
        } else {
            SheetsFold::Folded
        };
        self.sheets_fold = next;
        if let Some(path) = &self.settings_path {
            let value = if next == SheetsFold::Folded {
                "folded"
            } else {
                "auto"
            };
            let _ = pf_i18n::settings::save_value(path, SHEETS_SETTING, serde_json::json!(value));
        }
    }

    fn tool_window_open(&self, win: ToolWin) -> bool {
        match win {
            ToolWin::Pleat => self.editor.pleat.open,
            ToolWin::Flare => self.editor.flare.open,
            ToolWin::Cutline => self.editor.cutline.open,
            ToolWin::Quick => self.editor.quick.open,
            ToolWin::Seams => self.editor.seam.open,
            ToolWin::JoinView => self.editor.seam.open && self.editor.seam.join_view,
            ToolWin::Dart => self.editor.dart_selected.is_some(),
            ToolWin::Report => self.report_dialog.is_some(),
            ToolWin::Unfold => self.unfold.is_some(),
            ToolWin::Assemble => self.assemble.is_some(),
            ToolWin::Transform => self.transform.is_some(),
            ToolWin::Level => self.level.is_some(),
            ToolWin::Variables => self.variables.is_some(),
            ToolWin::Diagnostic => self.diagnostic.is_some(),
            ToolWin::Dependencies => self.dependencies.is_some(),
            ToolWin::Sizes => self.sizes.is_some(),
            ToolWin::Grading => self.grading.is_some(),
            ToolWin::SizeMeasures => self.size_measures.is_some(),
            ToolWin::Bareme => self.bareme.is_some(),
            ToolWin::Clients => self.clients.is_some(),
            ToolWin::Finished => self.finished.is_some(),
            ToolWin::SizeNest => self.size_nest.is_some(),
            ToolWin::ShapeControl => self.shape_control.is_some(),
            ToolWin::ExportSizes => self.export_sizes.is_some(),
            ToolWin::Bom => self.bom.is_some(),
            ToolWin::Variants => self.variants.is_some(),
            ToolWin::Versions => self.versions.is_some(),
            ToolWin::Cutting => self.cutting.is_some(),
            ToolWin::Library => self.library.is_some(),
            ToolWin::Naming => self.naming.is_some(),
            ToolWin::Print => self.direct_print.is_some(),
            ToolWin::Projection => self.projection.is_some(),
            ToolWin::Background => self.background.is_some(),
        }
    }

    /// Ferme une fenêtre d'outil sans effet sur le document : mêmes remises à zéro que son
    /// bouton « Fermer » (un aperçu non appliqué disparaît, il n'est jamais stocké).
    pub(crate) fn close_tool_window(&mut self, win: ToolWin) {
        match win {
            ToolWin::Pleat => {
                self.editor.pleat.open = false;
                self.editor.pleat.picking = 0;
                self.editor.pleat.show_folded = false;
            }
            ToolWin::Flare => self.editor.flare.open = false,
            ToolWin::Cutline => self.editor.cutline.open = false,
            ToolWin::Quick => self.editor.quick.open = false,
            ToolWin::Seams => {
                self.editor.seam.open = false;
                self.editor.seam.picking = 0;
                self.editor.seam.join_view = false;
            }
            ToolWin::JoinView => self.editor.seam.join_view = false,
            ToolWin::Dart => {
                self.editor.dart_selected = None;
                self.editor.dart_transfer.armed = false;
            }
            ToolWin::Report => self.report_dialog = None,
            ToolWin::Unfold => self.unfold = None,
            ToolWin::Assemble => self.assemble = None,
            ToolWin::Transform => self.transform = None,
            ToolWin::Level => self.level = None,
            ToolWin::Variables => self.variables = None,
            ToolWin::Diagnostic => self.diagnostic = None,
            ToolWin::Dependencies => self.dependencies = None,
            ToolWin::Sizes => self.sizes = None,
            ToolWin::Grading => self.grading = None,
            ToolWin::SizeMeasures => self.size_measures = None,
            ToolWin::Bareme => self.bareme = None,
            ToolWin::Clients => self.clients = None,
            ToolWin::Finished => self.finished = None,
            ToolWin::SizeNest => self.size_nest = None,
            ToolWin::ShapeControl => self.shape_control = None,
            ToolWin::ExportSizes => self.export_sizes = None,
            ToolWin::Bom => self.bom = None,
            ToolWin::Variants => self.variants = None,
            ToolWin::Versions => self.versions = None,
            ToolWin::Cutting => self.cutting = None,
            ToolWin::Library => self.library = None,
            ToolWin::Naming => self.naming = None,
            ToolWin::Print => self.direct_print = None,
            ToolWin::Projection => self.projection = None,
            ToolWin::Background => {
                self.editor.trace.cancel_edges();
                self.background = None;
            }
        }
    }

    /// Met à jour la liste des fenêtres ouvertes (ordre d'ouverture conservé).
    pub(crate) fn dock_sync(&mut self) {
        let mut order = std::mem::take(&mut self.dock.order);
        order.retain(|w| self.tool_window_open(*w));
        for win in ToolWin::ALL {
            if self.tool_window_open(win) && !order.contains(&win) {
                order.push(win);
            }
        }
        self.dock.order = order;
    }

    /// Réserve la colonne du dock (après l'inspecteur, avant le canevas : le canevas se
    /// rétrécit au lieu d'être recouvert). Sans fenêtre ouverte, rien n'est réservé.
    pub(crate) fn dock_panel(&mut self, ctx: &egui::Context) {
        let width = self.dock.reserved_width(ctx);
        if width <= 0.0 {
            self.dock.rect = Rect::NOTHING;
            return;
        }
        let panel = match self.layout.dock_side {
            crate::workspace::DockSide::Right => egui::SidePanel::right("tool_dock"),
            crate::workspace::DockSide::Left => egui::SidePanel::left("tool_dock"),
        };
        let response = panel.resizable(false).exact_width(width).show(ctx, |_| {});
        self.dock.rect = response.response.rect;
    }
}
