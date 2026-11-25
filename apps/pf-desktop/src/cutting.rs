//! US-15.1 / US-15.2 : fenêtre « Plan de coupe » du dock (menu Fichier, sans raccourci, aucun
//! outil).
//!
//! La laize est horizontale (`x`), la longueur monte (`y` vers le haut) : le plan est dessiné à
//! l'échelle. Les contours posés, les chevauchements, écarts et débordements viennent du cœur
//! (`pf_core::cutting_layout`, recalculés à la lecture, jamais stockés). Chaque geste est **une**
//! commande, donc un pas d'annulation : un glisser continu n'en produit qu'un, à la fin du geste.
//! Les défauts sont dits par un **motif** (hachures « / » pour un chevauchement, « \ » pour un
//! débordement), par la couleur et par le texte des contrôles, jamais par la couleur seule.

use crate::a11y::ComboExt;
use crate::app::App;
use crate::dock::ToolWin;
use crate::grading::section_header;
use crate::measurements::{edit, Buffers};
use eframe::egui::{self, Color32, Pos2, Rect, RichText, Sense, Shape, Stroke};
use pf_core::cutting_auto::{
    AutoError, AutoItem, AutoMode, AutoOptions, AutoProposal, UnplaceableReason,
};
use pf_core::cutting_layout::{
    first_allowed_rotation, place_polygon, triangulate_polygon, OverflowReason, PieceShape,
    PlanAnalysis, RotationCheck, RowLayoutError, WarningKind,
};
use pf_core::cutting_yield::{plan_yield, YieldStatus};
use pf_core::{
    Command, CuttingPlan, CuttingPlanId, CuttingPlanSpec, FabricFold, PieceId, PlacementId,
    Project, SizeId,
};
use pf_export::cutting_export::{cutting_csv, cutting_file_stem, cutting_pdf, cutting_svg};
use pf_export::report::{ReportExport, ReportFormat};
use pf_geometry::PointMm;
use pf_i18n::{lang, tr, tr_args};
use pf_ui::components::{button, ButtonKind};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::sync::{Arc, Mutex};

const K_NAME: u8 = 40;
const K_MATERIAL: u8 = 41;
const K_WIDTH: u8 = 42;
const K_SELVEDGE: u8 = 43;
const K_GAP: u8 = 44;
const K_GRAIN: u8 = 45;
const K_ROT: u8 = 46;
const K_X: u8 = 47;
const K_Y: u8 = 48;
const K_AUTO_QTY: u8 = 49;
/// Hauteur maximale du canevas (points).
const CANVAS_MAX_H: f32 = 340.0;
const CANVAS_MIN_H: f32 = 150.0;
/// Pas des hachures à l'écran (points).
const HATCH_PX: f64 = 6.0;

#[derive(Debug, Clone)]
enum Drag {
    /// Pièce de la liste en cours de dépose (contour à la rotation de pose).
    New {
        shape: Box<PieceShape>,
        rotation: f64,
    },
    /// Pièce posée déplacée ; `delta` en mm depuis l'origine du geste.
    Move {
        placement: PlacementId,
        delta: (f64, f64),
    },
}

/// Géométrie du canevas à l'image précédente (conversion écran / mm).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Geo {
    pub(crate) rect: Rect,
    pub(crate) scale: f32,
    /// Coin bas-gauche du canevas dans le plan (mm) : `(0 ; 0)` sans peau (US-15.5).
    pub(crate) x0: f64,
    pub(crate) y0: f64,
}

impl Geo {
    pub(crate) fn to_screen(self, x: f64, y: f64) -> Pos2 {
        Pos2::new(
            self.rect.left() + (x - self.x0) as f32 * self.scale,
            self.rect.bottom() - (y - self.y0) as f32 * self.scale,
        )
    }

    pub(crate) fn to_mm(self, p: Pos2) -> (f64, f64) {
        (
            self.x0 + f64::from((p.x - self.rect.left()) / self.scale),
            self.y0 + f64::from((self.rect.bottom() - p.y) / self.scale),
        )
    }
}

#[derive(Debug, Clone)]
struct Cache {
    project: Project,
    plan: CuttingPlanId,
    analysis: PlanAnalysis,
}

/// État de la fenêtre (interface seulement, jamais enregistré).
#[derive(Debug, Clone)]
pub struct CuttingDialog {
    pub selected: Option<CuttingPlanId>,
    pub placement: Option<PlacementId>,
    /// Pièces cochées pour « Poser la sélection ».
    pub pick: Vec<PieceId>,
    /// Taille posée par les gestes de pose ; `None` = taille de base.
    pub size: Option<SizeId>,
    pub list_open: bool,
    pub params_open: bool,
    pub canvas_open: bool,
    pub placed_open: bool,
    pub diag_open: bool,
    pub yield_open: bool,
    /// Placement automatique (US-15.4) : section, périmètre, quantité, rotations.
    pub auto_open: bool,
    pub auto_all: bool,
    /// Mode du placement automatique (état d'interface, jamais enregistré).
    pub auto_mode: AutoMode,
    pub auto_rot: bool,
    pub auto_qty: u32,
    /// Tests : le prochain calcul s'arrête sans rendre de résultat (panne simulée).
    #[cfg(test)]
    pub(crate) auto_fail: bool,
    pub(crate) auto_job: Option<AutoJob>,
    /// Dernière proposition (ou refus) du plan `auto_plan`, en attente de validation.
    pub(crate) auto_out: Option<Result<AutoProposal, AutoError>>,
    auto_plan: Option<CuttingPlanId>,
    /// Peau (US-15.5) : saisie, exigence de qualité, sélection (interface seulement).
    pub(crate) hide: crate::cutting_hide::HideUi,
    buffers: Buffers,
    new_name: String,
    error: Option<String>,
    drag: Option<Drag>,
    geo: Option<Geo>,
    cache: Option<Cache>,
}

impl CuttingDialog {
    /// Diagnostics en cache du plan affiché (US-15.5).
    pub(crate) fn cache_analysis(&self) -> Option<&PlanAnalysis> {
        self.cache.as_ref().map(|c| &c.analysis)
    }

    /// Position écran d'un point (x, y) du plan (mm), d'après le canevas de l'image précédente.
    #[cfg(test)]
    pub(crate) fn screen_pos(&self, x_mm: f64, y_mm: f64) -> Option<Pos2> {
        self.geo.map(|g| g.to_screen(x_mm, y_mm))
    }
}

impl Default for CuttingDialog {
    fn default() -> Self {
        CuttingDialog {
            selected: None,
            placement: None,
            pick: Vec::new(),
            size: None,
            list_open: true,
            params_open: false,
            canvas_open: true,
            placed_open: true,
            diag_open: true,
            yield_open: true,
            auto_open: false,
            auto_all: false,
            auto_mode: AutoMode::ReplaceAll,
            auto_rot: true,
            auto_qty: 1,
            #[cfg(test)]
            auto_fail: false,
            auto_job: None,
            auto_out: None,
            auto_plan: None,
            hide: crate::cutting_hide::HideUi::default(),
            buffers: Buffers::new(),
            new_name: String::new(),
            error: None,
            drag: None,
            geo: None,
            cache: None,
        }
    }
}

/// Calcul de proposition en cours dans un fil séparé (annulable).
#[derive(Debug, Clone)]
pub(crate) struct AutoJob {
    rx: Arc<Mutex<Receiver<Result<AutoProposal, AutoError>>>>,
    cancel: Arc<AtomicBool>,
}

/// Sortie du plan de coupe (US-15.3 / US-13.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CutOut {
    Svg,
    Pdf,
    Csv,
}

enum CAction {
    Export(CutOut),
    Create,
    Remove(CuttingPlanId),
    Spec(CuttingPlanId, CuttingPlanSpec),
    PoseSelection,
    PoseAll,
    DropNew {
        piece: PieceId,
        x: f64,
        y: f64,
        rotation: f64,
    },
    Move {
        x: f64,
        y: f64,
    },
    Rotate(f64),
    Mirror,
    Delete,
    SetPos(Option<f64>, Option<f64>),
    Error(String),
    AutoRun,
    AutoCancel,
    AutoApply,
    AutoDiscard,
    Hide(crate::cutting_hide::HideAct),
}

fn parse_num(text: &str) -> Result<f64, String> {
    text.trim()
        .replace(',', ".")
        .parse::<f64>()
        .map_err(|_| tr_args("cutting.err.number", &[("text", text.trim())]))
}

/// Champ de longueur (mm exacts relus dans l'unité d'affichage) : saisie sans suffixe = unité
/// d'affichage, jamais renvoyé arrondi au modèle.
pub(crate) fn parse_len(text: &str) -> Result<f64, String> {
    pf_core::units::parse_length_display(text)
        .map_err(|_| tr_args("cutting.err.number", &[("text", text.trim())]))
}

/// Longueur dans l'unité d'affichage, sans suffixe (gabarit `tr_len`) ; mm : comme `mm`.
pub(crate) fn len(v: f64) -> String {
    if pf_core::units::display_units().unit == pf_core::units::LengthUnit::Mm {
        mm(v)
    } else {
        pf_core::units::len_number(v, 1)
    }
}

/// Longueur avec son unité d'affichage (lignes de rendement).
fn len_unit(v: f64) -> String {
    if pf_core::units::display_units().unit == pf_core::units::LengthUnit::Mm {
        format!("{} mm", mm(v))
    } else {
        pf_core::units::len_text(v, 1)
    }
}

pub(crate) fn mm(v: f64) -> String {
    let s = format!("{v:.1}");
    s.strip_suffix(".0").map_or(s.clone(), str::to_string)
}

fn spec_of(plan: &CuttingPlan) -> CuttingPlanSpec {
    CuttingPlanSpec {
        name: plan.name.clone(),
        material: plan.material.clone(),
        fabric_width_mm: plan.fabric_width_mm,
        fold: plan.fold,
        selvedge_mm: plan.selvedge_mm,
        min_gap_mm: plan.min_gap_mm,
        grain_direction: plan.grain_direction,
    }
}

fn default_spec(name: String) -> CuttingPlanSpec {
    CuttingPlanSpec {
        name,
        material: None,
        fabric_width_mm: 1500.0,
        fold: FabricFold::Flat,
        selvedge_mm: 10.0,
        min_gap_mm: 5.0,
        grain_direction: None,
    }
}

fn layout_error(e: &RowLayoutError) -> String {
    match e {
        RowLayoutError::NothingToPlace => tr("cutting.pose_nothing"),
        RowLayoutError::TooWide {
            name,
            width_mm,
            span_mm,
        } => pf_i18n::tr_len(
            "cutting.err.too_wide",
            &[
                ("name", name),
                ("width", &len(*width_mm)),
                ("span", &len(*span_mm)),
            ],
        ),
        RowLayoutError::NoRotation { name } => {
            tr_args("cutting.err.no_rotation", &[("name", name)])
        }
        RowLayoutError::Core(e) => e.to_string(),
    }
}

/// Nom d'une pose : pièce, taille éventuelle et rang parmi les poses du plan.
fn pose_name(project: &Project, plan: &CuttingPlan, id: PlacementId) -> String {
    let Some((rank, pl)) = plan.placements.iter().enumerate().find(|(_, p)| p.id == id) else {
        return String::new();
    };
    let name = project
        .piece(pl.piece)
        .map(|p| p.name.clone())
        .unwrap_or_default();
    format!("{name} #{}", rank + 1)
}

impl App {
    /// Menu Fichier › « Plan de coupe… » (sans raccourci).
    pub fn open_cutting(&mut self) {
        if self.cutting.is_none() {
            self.cutting = Some(CuttingDialog::default());
        }
    }

    fn cutting_act(&mut self, d: &mut CuttingDialog, action: CAction) {
        let project = self.editor.project().clone();
        let size = d.size;
        let result: Result<(), String> = match action {
            CAction::Create => {
                let name = d.new_name.trim().to_string();
                self.measure_exec(Command::AddCuttingPlan {
                    spec: default_spec(name),
                })
                .map(|out| {
                    d.new_name.clear();
                    d.selected = out.cutting_plans.last().copied();
                    d.placement = None;
                })
            }
            CAction::Remove(plan) => {
                self.measure_exec(Command::RemoveCuttingPlan { plan })
                    .map(|_| {
                        d.selected = None;
                        d.placement = None;
                    })
            }
            CAction::Spec(plan, spec) => self
                .measure_exec(Command::UpdateCuttingPlan { plan, spec })
                .map(|_| ()),
            CAction::PoseSelection | CAction::PoseAll => {
                let wanted: Vec<(PieceId, Option<SizeId>)> = match (d.selected, &action) {
                    (Some(plan), CAction::PoseAll) => project
                        .analyze_cutting_plan(plan)
                        .map(|a| a.unplaced.into_iter().map(|p| (p, size)).collect())
                        .unwrap_or_default(),
                    _ => project
                        .pieces
                        .iter()
                        .filter(|p| d.pick.contains(&p.id))
                        .map(|p| (p.id, size))
                        .collect(),
                };
                match d
                    .selected
                    .map(|plan| project.plan_row_layout(plan, &wanted))
                {
                    None => Ok(()),
                    Some(Err(e)) => Err(layout_error(&e)),
                    Some(Ok(cmd)) => self.measure_exec(cmd).map(|_| d.pick.clear()),
                }
            }
            CAction::DropNew {
                piece,
                x,
                y,
                rotation,
            } => match d.selected {
                Some(plan) => self
                    .measure_exec(Command::AddPlacement {
                        plan,
                        piece,
                        size,
                        x_mm: x,
                        y_mm: y,
                        rotation_deg: rotation,
                        mirrored: false,
                    })
                    .map(|_| ()),
                None => Ok(()),
            },
            CAction::Move { x, y, .. } => {
                self.update_pose(d, &project, |pl| (x, y, pl.rotation_deg, pl.mirrored))
            }
            CAction::Rotate(deg) => {
                let r = self.checked_rotation(d, &project, deg);
                match r {
                    Err(e) => Err(e),
                    Ok(rot) => {
                        self.update_pose(d, &project, |pl| (pl.x_mm, pl.y_mm, rot, pl.mirrored))
                    }
                }
            }
            CAction::Mirror => self.update_pose(d, &project, |pl| {
                (pl.x_mm, pl.y_mm, pl.rotation_deg, !pl.mirrored)
            }),
            CAction::Delete => match (d.selected, d.placement) {
                (Some(plan), Some(placement)) => self
                    .measure_exec(Command::RemovePlacement { plan, placement })
                    .map(|_| d.placement = None),
                _ => Ok(()),
            },
            CAction::SetPos(x, y) => self.update_pose(d, &project, |pl| {
                (
                    x.unwrap_or(pl.x_mm),
                    y.unwrap_or(pl.y_mm),
                    pl.rotation_deg,
                    pl.mirrored,
                )
            }),
            CAction::Error(e) => Err(e),
            CAction::Hide(act) => self.hide_act(d, act),
            CAction::AutoRun => {
                if let Some(plan) = d.selected {
                    self.auto_start(d, &project, plan);
                }
                Ok(())
            }
            CAction::AutoCancel => {
                if let Some(job) = d.auto_job.take() {
                    job.cancel.store(true, Ordering::Relaxed);
                }
                Ok(())
            }
            CAction::AutoDiscard => {
                d.auto_out = None;
                Ok(())
            }
            CAction::AutoApply => match d.auto_out.take() {
                Some(Ok(p)) => self.measure_exec(p.command).map(|_| d.placement = None),
                other => {
                    d.auto_out = other;
                    Ok(())
                }
            },
            CAction::Export(what) => {
                if let Some(plan) = d.selected {
                    self.save_cutting_dialog(plan, what);
                }
                Ok(())
            }
        };
        d.error = result.err();
    }

    /// Lance le calcul de la proposition dans un fil séparé (annulable depuis la fenêtre).
    fn auto_start(&mut self, d: &mut CuttingDialog, project: &Project, plan: CuttingPlanId) {
        d.auto_out = None;
        d.auto_plan = Some(plan);
        let items = auto_items(d, project, plan);
        if items.is_empty() {
            d.auto_out = Some(Err(AutoError::NothingToPlace));
            return;
        }
        let (tx, rx) = channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let (snapshot, c) = (project.clone(), cancel.clone());
        let options = AutoOptions {
            allow_rotation: d.auto_rot,
            ..AutoOptions::default()
        };
        let mode = d.auto_mode;
        let required = d.hide.required;
        #[cfg(test)]
        let fail = d.auto_fail;
        std::thread::spawn(move || {
            // Même effet qu'une panique (le fil meurt, le canal se ferme), sans trace en console.
            #[cfg(test)]
            if fail {
                std::panic::resume_unwind(Box::new("simulated failure"));
            }
            let r = propose(&snapshot, plan, &items, &options, mode, required, &c);
            let _ = tx.send(r);
        });
        d.auto_job = Some(AutoJob {
            rx: Arc::new(Mutex::new(rx)),
            cancel,
        });
    }

    /// Rotation saisie : refusée avec un message clair si le droit-fil l'interdit.
    fn checked_rotation(
        &self,
        d: &CuttingDialog,
        project: &Project,
        deg: f64,
    ) -> Result<f64, String> {
        let (Some(plan), Some(id)) = (d.selected, d.placement) else {
            return Ok(deg);
        };
        let plan = project.cutting_plan(plan).map_err(|e| e.to_string())?;
        let Some(pl) = plan.placements.iter().find(|p| p.id == id) else {
            return Ok(deg);
        };
        if let Ok(shape) = project.piece_shape(pl.piece, pl.size) {
            if let RotationCheck::Refused { deviation_deg } =
                Project::rotation_check(plan, &shape, deg, pl.mirrored)
            {
                return Err(tr_args(
                    "cutting.err.rotation_grain",
                    &[("dev", &mm(deviation_deg))],
                ));
            }
        }
        Ok(deg)
    }

    /// Remplace position, rotation et miroir de la pose sélectionnée (une commande).
    fn update_pose(
        &mut self,
        d: &CuttingDialog,
        project: &Project,
        f: impl FnOnce(&pf_core::CuttingPlacement) -> (f64, f64, f64, bool),
    ) -> Result<(), String> {
        let (Some(plan), Some(id)) = (d.selected, d.placement) else {
            return Ok(());
        };
        let found = project
            .cutting_plan(plan)
            .ok()
            .and_then(|p| p.placements.iter().find(|q| q.id == id).copied());
        let Some(pl) = found else { return Ok(()) };
        let (x, y, rot, mirrored) = f(&pl);
        self.measure_exec(Command::UpdatePlacement {
            plan,
            placement: id,
            size: pl.size,
            x_mm: (x * 10.0).round() / 10.0,
            y_mm: (y * 10.0).round() / 10.0,
            rotation_deg: rot,
            mirrored,
        })
        .map(|_| ())
    }

    pub fn cutting_window(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.cutting.take() else {
            return;
        };
        let project = self.editor.project().clone();
        let tokens = pf_ui::theme::tokens(ctx);
        d.selected = d
            .selected
            .filter(|id| project.cutting_plan(*id).is_ok())
            .or(project.cutting_plans.first().map(|p| p.id));
        let plan_ref = d.selected.and_then(|id| project.cutting_plan(id).ok());
        if d.placement
            .is_some_and(|id| plan_ref.is_none_or(|p| !p.placements.iter().any(|q| q.id == id)))
        {
            d.placement = None;
        }
        d.pick.retain(|p| project.piece(*p).is_ok());
        if d.auto_plan != d.selected {
            d.auto_out = None;
            if let Some(job) = d.auto_job.take() {
                job.cancel.store(true, Ordering::Relaxed);
            }
            d.auto_plan = d.selected;
        }
        let arrived = d.auto_job.as_ref().map(|job| {
            job.rx
                .lock()
                .map_or(Err(TryRecvError::Disconnected), |rx| rx.try_recv())
        });
        match arrived {
            Some(Ok(result)) => {
                d.auto_out = Some(result);
                d.auto_job = None;
            }
            Some(Err(TryRecvError::Empty)) => {
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
            }
            // Fil mort sans résultat (panique) : le dire, comme à l'ouverture et pour les contours.
            Some(Err(TryRecvError::Disconnected)) => {
                d.auto_job = None;
                self.error_dialog = Some(tr("cutting.auto.err.thread"));
                ctx.request_repaint();
            }
            None => {}
        }
        if d.size.is_some_and(|s| project.find_size(s).is_none()) {
            d.size = None;
        }
        // Diagnostics : recalculés seulement quand le projet ou le plan affiché change.
        if let Some(id) = d.selected {
            let fresh = d
                .cache
                .as_ref()
                .is_some_and(|c| c.plan == id && c.project == project);
            if !fresh {
                if d.cache.is_some() {
                    d.auto_out = None;
                }
                d.cache = project.analyze_cutting_plan(id).ok().map(|analysis| Cache {
                    project: project.clone(),
                    plan: id,
                    analysis,
                });
            }
        } else {
            d.cache = None;
        }
        let mut open = true;
        let mut close = false;
        let mut actions: Vec<CAction> = Vec::new();
        let slot = crate::dock::slot(ctx, &self.dock, ToolWin::Cutting);
        let width = self.dock.content_width(ToolWin::Cutting);
        egui::Window::new(tr("cutting.title"))
            .id(ToolWin::Cutting.id())
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
                            RichText::new(tr("cutting.intro")).color(tokens.text_secondary),
                        );
                        if let Some(text) = &d.error {
                            crate::a11y::label(ui, RichText::new(text).color(tokens.danger));
                        }
                        ui.add_space(4.0);
                        plans_section(ui, &mut d, &project, &mut actions);
                        if let Some(plan) = plan_ref {
                            ui.add_space(4.0);
                            canvas_section(ui, &mut d, &project, plan, width, &mut actions);
                            ui.add_space(4.0);
                            ui.add_space(4.0);
                            let mut hide_acts = Vec::new();
                            crate::cutting_hide::hide_section(
                                ui,
                                &mut d,
                                &project,
                                plan,
                                &mut hide_acts,
                            );
                            actions.extend(hide_acts.into_iter().map(CAction::Hide));
                            ui.add_space(4.0);
                            placed_section(ui, &mut d, &project, plan);
                            selection_section(ui, &mut d, &project, plan, &mut actions);
                            ui.add_space(4.0);
                            diag_section(ui, &mut d, &project, plan);
                            ui.add_space(4.0);
                            yield_section(ui, &mut d, &project, plan, &mut actions);
                            ui.add_space(4.0);
                            auto_section(ui, &mut d, &project, &mut actions);
                            ui.add_space(4.0);
                            params_section(ui, &mut d, &project, plan, &mut actions);
                        }
                        ui.add_space(6.0);
                        close = button(ui, &tr("dialog.close"), ButtonKind::Secondary).clicked();
                    });
            });
        for a in actions {
            self.cutting_act(&mut d, a);
        }
        if open && !close {
            self.cutting = Some(d);
        }
    }
}

fn plans_section(
    ui: &mut egui::Ui,
    d: &mut CuttingDialog,
    project: &Project,
    actions: &mut Vec<CAction>,
) {
    section_header(
        ui,
        &mut d.list_open,
        tr_args(
            "cutting.plans",
            &[("count", &project.cutting_plans.len().to_string())],
        ),
    );
    if !d.list_open {
        return;
    }
    if project.cutting_plans.is_empty() {
        crate::a11y::label(ui, tr("cutting.none"));
    }
    for p in &project.cutting_plans {
        let on = d.selected == Some(p.id);
        if ui.selectable_label(on, &p.name).clicked() && !on {
            d.selected = Some(p.id);
            d.placement = None;
            d.pick.clear();
        }
    }
    ui.horizontal(|ui| {
        crate::a11y::label(ui, tr("cutting.new"));
        crate::a11y::add_text(
            ui,
            egui::TextEdit::singleline(&mut d.new_name)
                .id(egui::Id::new("cutting_new_name"))
                .desired_width(150.0),
        );
        if button(ui, &tr("cutting.add"), ButtonKind::Primary).clicked() {
            actions.push(CAction::Create);
        }
        if let Some(id) = d.selected {
            if button(ui, &tr("cutting.remove"), ButtonKind::Danger).clicked() {
                actions.push(CAction::Remove(id));
            }
        }
    });
}

fn params_section(
    ui: &mut egui::Ui,
    d: &mut CuttingDialog,
    project: &Project,
    plan: &CuttingPlan,
    actions: &mut Vec<CAction>,
) {
    section_header(ui, &mut d.params_open, tr("cutting.params"));
    if !d.params_open {
        return;
    }
    let id = plan.id;
    let base = spec_of(plan);
    let key = |k: u8| (k, id.0, 0);
    let commit = |actions: &mut Vec<CAction>, s: Result<CuttingPlanSpec, String>| {
        actions.push(match s {
            Ok(s) => CAction::Spec(id, s),
            Err(e) => CAction::Error(e),
        });
    };
    egui::Grid::new("cutting_params")
        .num_columns(2)
        .show(ui, |ui| {
            crate::a11y::label(ui, tr("cutting.name"));
            if let Some(t) = edit(ui, &mut d.buffers, key(K_NAME), &plan.name, 180.0) {
                commit(
                    actions,
                    Ok(CuttingPlanSpec {
                        name: t,
                        ..base.clone()
                    }),
                );
            }
            ui.end_row();
            crate::a11y::label(ui, tr("cutting.material"));
            ui.horizontal(|ui| {
                let current = plan.material.clone().unwrap_or_default();
                if let Some(t) = edit(ui, &mut d.buffers, key(K_MATERIAL), &current, 120.0) {
                    let material = (!t.trim().is_empty()).then(|| t.trim().to_string());
                    commit(
                        actions,
                        Ok(CuttingPlanSpec {
                            material,
                            ..base.clone()
                        }),
                    );
                }
                let mut materials: Vec<String> = project
                    .pieces
                    .iter()
                    .filter_map(|p| p.material.clone())
                    .collect();
                materials.sort();
                materials.dedup();
                crate::a11y::label(ui, tr("cutting.material.pick"));
                egui::ComboBox::from_id_salt("cutting_material")
                    .selected_text(
                        plan.material
                            .clone()
                            .unwrap_or_else(|| tr("cutting.material.free")),
                    )
                    .show_ui_named(ui, |ui| {
                        if ui
                            .selectable_label(plan.material.is_none(), tr("cutting.material.free"))
                            .clicked()
                        {
                            commit(
                                actions,
                                Ok(CuttingPlanSpec {
                                    material: None,
                                    ..base.clone()
                                }),
                            );
                        }
                        for m in &materials {
                            if ui
                                .selectable_label(plan.material.as_deref() == Some(m.as_str()), m)
                                .clicked()
                            {
                                commit(
                                    actions,
                                    Ok(CuttingPlanSpec {
                                        material: Some(m.clone()),
                                        ..base.clone()
                                    }),
                                );
                            }
                        }
                    });
            });
            ui.end_row();
            for (k, label, value) in [
                (K_WIDTH, "cutting.width", plan.fabric_width_mm),
                (K_SELVEDGE, "cutting.selvedge", plan.selvedge_mm),
                (K_GAP, "cutting.gap", plan.min_gap_mm),
            ] {
                crate::a11y::label(ui, tr(label));
                ui.horizontal(|ui| {
                    if let Some(t) = edit(
                        ui,
                        &mut d.buffers,
                        key(k),
                        &pf_core::units::field_text(value),
                        80.0,
                    ) {
                        let spec = parse_len(&t).map(|v| match k {
                            K_WIDTH => CuttingPlanSpec {
                                fabric_width_mm: v,
                                ..base.clone()
                            },
                            K_SELVEDGE => CuttingPlanSpec {
                                selvedge_mm: v,
                                ..base.clone()
                            },
                            _ => CuttingPlanSpec {
                                min_gap_mm: v,
                                ..base.clone()
                            },
                        });
                        commit(actions, spec);
                    }
                    ui.label(pf_core::units::display_units().unit.suffix());
                });
                ui.end_row();
            }
            crate::a11y::label(ui, tr("cutting.grain"));
            ui.horizontal(|ui| {
                let current = plan.grain_direction.map(mm).unwrap_or_default();
                if let Some(t) = edit(ui, &mut d.buffers, key(K_GRAIN), &current, 80.0) {
                    let spec = if t.trim().is_empty() {
                        Ok(CuttingPlanSpec {
                            grain_direction: None,
                            ..base.clone()
                        })
                    } else {
                        parse_num(&t).map(|v| CuttingPlanSpec {
                            grain_direction: Some(v),
                            ..base.clone()
                        })
                    };
                    commit(actions, spec);
                }
                ui.label("°");
            });
            ui.end_row();
        });
    crate::a11y::label(
        ui,
        RichText::new(tr("cutting.grain.hint"))
            .small()
            .color(pf_ui::theme::tokens(ui.ctx()).text_secondary),
    );
    let mut folded = plan.fold == FabricFold::Folded;
    if ui.checkbox(&mut folded, tr("cutting.fold")).changed() {
        let fold = if folded {
            FabricFold::Folded
        } else {
            FabricFold::Flat
        };
        commit(
            actions,
            Ok(CuttingPlanSpec {
                fold,
                ..base.clone()
            }),
        );
    }
}

/// Pièces encore à poser : `(pièce, posées, attendues)`.
fn todo_pieces(project: &Project, plan: &CuttingPlan) -> Vec<(PieceId, usize, usize)> {
    project
        .plan_candidate_pieces(plan)
        .into_iter()
        .filter_map(|id| {
            let p = project.piece(id).ok()?;
            let expected = p.quantity as usize * if p.mirrored_pair { 2 } else { 1 };
            let placed = plan.placements.iter().filter(|q| q.piece == id).count();
            (placed < expected).then_some((id, placed, expected))
        })
        .collect()
}

fn canvas_section(
    ui: &mut egui::Ui,
    d: &mut CuttingDialog,
    project: &Project,
    plan: &CuttingPlan,
    width: f32,
    actions: &mut Vec<CAction>,
) {
    section_header(ui, &mut d.canvas_open, tr("cutting.canvas"));
    if !d.canvas_open {
        return;
    }
    let tokens = pf_ui::theme::tokens(ui.ctx());
    crate::a11y::label(
        ui,
        RichText::new(tr("cutting.canvas.hint"))
            .small()
            .color(tokens.text_secondary),
    );
    if !project.sizes.is_empty() {
        ui.horizontal(|ui| {
            crate::a11y::label(ui, tr("cutting.size"));
            let text = d
                .size
                .and_then(|s| project.find_size(s).map(|(r, i)| r.sizes[i].label.clone()))
                .unwrap_or_else(|| tr("cutting.size.base"));
            egui::ComboBox::from_id_salt("cutting_size")
                .selected_text(text)
                .show_ui_named(ui, |ui| {
                    if ui
                        .selectable_label(d.size.is_none(), tr("cutting.size.base"))
                        .clicked()
                    {
                        d.size = None;
                    }
                    for range in &project.sizes {
                        for z in &range.sizes {
                            if ui
                                .selectable_label(d.size == Some(z.id), &z.label)
                                .clicked()
                            {
                                d.size = Some(z.id);
                            }
                        }
                    }
                });
        });
    }
    let todo = todo_pieces(project, plan);
    let list_w = 140.0_f32;
    let canvas_w = (width - list_w - 16.0).max(200.0);
    let analysis = d
        .cache
        .as_ref()
        .map(|c| c.analysis.clone())
        .unwrap_or_default();
    let max_y = analysis
        .outlines
        .iter()
        .map(|o| o.bbox.max.y)
        .fold(0.0_f64, f64::max);
    let (mut w, mut length) = (
        plan.fabric_width_mm,
        (max_y * 1.1 + 50.0).max(plan.fabric_width_mm * 0.6),
    );
    let (mut x0, mut y0) = (0.0_f64, 0.0_f64);
    if let Some(h) = &plan.hide {
        // Peau : le canevas montre le contour, les poses et le tracé en cours (US-15.5).
        let mut pts: Vec<(f64, f64)> = h.contour.iter().map(|p| (p.x, p.y)).collect();
        for o in &analysis.outlines {
            pts.push((o.bbox.min.x, o.bbox.min.y));
            pts.push((o.bbox.max.x, o.bbox.max.y));
        }
        pts.extend(d.hide.points.iter().copied());
        let lo = pts
            .iter()
            .fold((f64::MAX, f64::MAX), |a, p| (a.0.min(p.0), a.1.min(p.1)));
        let hi = pts
            .iter()
            .fold((f64::MIN, f64::MIN), |a, p| (a.0.max(p.0), a.1.max(p.1)));
        (x0, y0) = lo;
        w = (hi.0 - lo.0).max(1.0);
        length = ((hi.1 - lo.1) * 1.05).max(1.0);
    }
    let scale = ((f64::from(canvas_w) / w).min(f64::from(CANVAS_MAX_H) / length)) as f32;
    let height = ((length as f32 * scale).clamp(CANVAS_MIN_H, CANVAS_MAX_H)) + 22.0;
    let mut dropped: Option<(PieceId, Pos2, f64)> = None;
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(list_w);
            crate::a11y::label(ui, RichText::new(tr("cutting.todo")).strong());
            if todo.is_empty() {
                crate::a11y::label(ui, tr("cutting.todo.none"));
            }
            for (id, placed, expected) in &todo {
                let name = project
                    .piece(*id)
                    .map(|p| p.name.clone())
                    .unwrap_or_default();
                let text = tr_args(
                    "cutting.todo.row",
                    &[
                        ("name", &name),
                        ("placed", &placed.to_string()),
                        ("expected", &expected.to_string()),
                    ],
                );
                let on = d.pick.contains(id);
                let r = ui.add(egui::Button::selectable(on, text).sense(Sense::click_and_drag()));
                if r.clicked() {
                    if on {
                        d.pick.retain(|p| p != id);
                    } else {
                        d.pick.push(*id);
                    }
                }
                if r.drag_started() {
                    if let Ok(shape) = project.piece_shape(*id, d.size) {
                        let rotation = first_allowed_rotation(plan, &shape).unwrap_or(0.0);
                        d.drag = Some(Drag::New {
                            shape: Box::new(shape),
                            rotation,
                        });
                    }
                }
                if r.drag_stopped() {
                    if let (Some(Drag::New { rotation, .. }), Some(p)) =
                        (&d.drag, ui.input(|i| i.pointer.interact_pos()))
                    {
                        dropped = Some((*id, p, *rotation));
                    }
                }
            }
            ui.add_space(4.0);
            if button(ui, &tr("cutting.pose_sel"), ButtonKind::Primary).clicked() {
                actions.push(CAction::PoseSelection);
            }
            if button(ui, &tr("cutting.pose_all"), ButtonKind::Secondary).clicked() {
                actions.push(CAction::PoseAll);
            }
            crate::a11y::label(
                ui,
                RichText::new(tr("cutting.pose_hint"))
                    .small()
                    .color(tokens.text_secondary),
            );
        });
        let (resp, painter) =
            ui.allocate_painter(egui::vec2(canvas_w, height), Sense::click_and_drag());
        let fabric = Rect::from_min_max(
            Pos2::new(resp.rect.left(), resp.rect.top() + 18.0),
            Pos2::new(resp.rect.left() + w as f32 * scale, resp.rect.bottom()),
        );
        let geo = Geo {
            rect: fabric,
            scale,
            x0,
            y0,
        };
        d.geo = Some(geo);
        paint_plan(&painter, geo, plan, &analysis, project, d, &tokens);
        // Interaction : sélection, déplacement d'une pièce posée.
        let hit = |p: Pos2| {
            let (x, y) = geo.to_mm(p);
            analysis
                .outlines
                .iter()
                .rev()
                .find(|o| pf_geometry::point_in_polygon(PointMm::new(x, y), &o.polygon))
                .map(|o| o.placement)
        };
        let drawing = d.hide.drawing;
        if drawing && resp.clicked() {
            if let Some(p) = resp.interact_pointer_pos() {
                let (x, y) = geo.to_mm(p);
                crate::cutting_hide::canvas_click(d, x, y);
            }
        }
        if resp.drag_started() && !drawing {
            if let Some(p) = ui.input(|i| i.pointer.press_origin()) {
                if let Some(id) = hit(p) {
                    d.placement = Some(id);
                    d.drag = Some(Drag::Move {
                        placement: id,
                        delta: (0.0, 0.0),
                    });
                }
            }
        }
        if resp.dragged() {
            if let Some(Drag::Move { delta, .. }) = &mut d.drag {
                let dv = ui.input(|i| i.pointer.delta());
                delta.0 += f64::from(dv.x / scale);
                delta.1 -= f64::from(dv.y / scale);
            }
        }
        if resp.drag_stopped() {
            if let Some(Drag::Move { placement, delta }) = d.drag.take() {
                if let Some(pl) = plan.placements.iter().find(|q| q.id == placement) {
                    if delta.0.abs() > 1e-6 || delta.1.abs() > 1e-6 {
                        d.placement = Some(placement);
                        actions.push(CAction::Move {
                            x: pl.x_mm + delta.0,
                            y: pl.y_mm + delta.1,
                        });
                    }
                }
            }
        } else if resp.clicked() && !drawing {
            d.placement = resp.interact_pointer_pos().and_then(hit);
        }
    });
    if let Some((piece, pos, rotation)) = dropped.take() {
        if let (Some(geo), Some(Drag::New { shape, .. })) = (d.geo, d.drag.take()) {
            if geo.rect.expand(2.0).contains(pos) {
                let (px, py) = geo.to_mm(pos);
                let poly = place_polygon(&shape.polygon, 0.0, 0.0, rotation, false);
                let (mut min, mut max) = (
                    PointMm::new(f64::MAX, f64::MAX),
                    PointMm::new(f64::MIN, f64::MIN),
                );
                for p in &poly {
                    min = PointMm::new(min.x.min(p.x), min.y.min(p.y));
                    max = PointMm::new(max.x.max(p.x), max.y.max(p.y));
                }
                actions.push(CAction::DropNew {
                    piece,
                    x: ((px - (max.x - min.x) / 2.0 - min.x) * 10.0).round() / 10.0,
                    y: ((py - (max.y - min.y) / 2.0 - min.y) * 10.0).round() / 10.0,
                    rotation,
                });
            }
        }
    }
    if !ui.input(|i| i.pointer.any_down()) && !matches!(d.drag, Some(Drag::Move { .. })) {
        d.drag = None;
    }
}

pub(crate) fn hatch(
    painter: &egui::Painter,
    geo: Geo,
    polygon: &[PointMm],
    angle_deg: f64,
    stroke: Stroke,
) {
    let step = HATCH_PX / f64::from(geo.scale);
    for (a, b) in pf_geometry::hatch::hatch_segments(polygon, angle_deg, step) {
        painter.line_segment([geo.to_screen(a.x, a.y), geo.to_screen(b.x, b.y)], stroke);
    }
}

pub(crate) fn fill_polygon(painter: &egui::Painter, geo: Geo, polygon: &[PointMm], color: Color32) {
    for t in triangulate_polygon(polygon) {
        painter.add(Shape::convex_polygon(
            t.iter().map(|p| geo.to_screen(p.x, p.y)).collect(),
            color,
            Stroke::NONE,
        ));
    }
}

pub(crate) fn outline(painter: &egui::Painter, geo: Geo, polygon: &[PointMm], stroke: Stroke) {
    let pts: Vec<Pos2> = polygon.iter().map(|p| geo.to_screen(p.x, p.y)).collect();
    painter.add(Shape::closed_line(pts, stroke));
}

fn paint_plan(
    painter: &egui::Painter,
    geo: Geo,
    plan: &CuttingPlan,
    analysis: &PlanAnalysis,
    project: &Project,
    d: &CuttingDialog,
    tokens: &pf_ui::tokens::Tokens,
) {
    let (w, sel) = (plan.fabric_width_mm, plan.selvedge_mm);
    let length = f64::from(geo.rect.height() / geo.scale);
    painter.rect_filled(geo.rect, 0.0, tokens.surface_canvas);
    // Lisières hachurées, zone utile, pli.
    let band = |x0: f64, x1: f64| {
        vec![
            PointMm::new(x0, 0.0),
            PointMm::new(x1, 0.0),
            PointMm::new(x1, length),
            PointMm::new(x0, length),
        ]
    };
    let has_hide = plan.hide.is_some();
    if sel > 0.0 && !has_hide {
        let stroke = Stroke::new(1.0_f32, tokens.text_secondary);
        hatch(painter, geo, &band(0.0, sel), 45.0, stroke);
        hatch(painter, geo, &band(w - sel, w), 45.0, stroke);
        let lab = |x: f64| {
            painter.text(
                geo.to_screen(x, length),
                egui::Align2::CENTER_TOP,
                tr("cutting.zone.selvedge"),
                egui::FontId::proportional(9.0),
                tokens.text_secondary,
            );
        };
        if sel * f64::from(geo.scale) > 24.0 {
            lab(sel / 2.0);
            lab(w - sel / 2.0);
        }
    }
    painter.rect_stroke(
        geo.rect,
        0.0,
        Stroke::new(1.0_f32, tokens.border_strong),
        egui::StrokeKind::Inside,
    );
    if plan.fold == FabricFold::Folded && !has_hide {
        let (a, b) = (geo.to_screen(w / 2.0, 0.0), geo.to_screen(w / 2.0, length));
        painter.add(Shape::dashed_line(
            &[a, b],
            Stroke::new(1.5_f32, tokens.text_primary),
            6.0,
            4.0,
        ));
        painter.text(
            b + egui::vec2(3.0, 2.0),
            egui::Align2::LEFT_TOP,
            tr("cutting.zone.fold"),
            egui::FontId::proportional(10.0),
            tokens.text_primary,
        );
    }
    if has_hide {
        crate::cutting_hide::paint_hide(painter, geo, plan, &d.hide, tokens);
    } else {
        painter.text(
            Pos2::new(geo.rect.left(), geo.rect.top() - 3.0),
            egui::Align2::LEFT_BOTTOM,
            pf_i18n::tr_len("cutting.dim.width", &[("mm", &len(w))]),
            egui::FontId::proportional(11.0),
            tokens.text_primary,
        );
    }
    let overlapping = |id: PlacementId| analysis.overlaps.iter().any(|o| o.a == id || o.b == id);
    let overflowing = |id: PlacementId| {
        analysis.overflows.iter().any(|o| o.placement == id)
            || analysis.hide_overflows.iter().any(|o| o.placement == id)
    };
    let on_defect = |id: PlacementId| analysis.defect_hits.iter().any(|o| o.placement == id);
    let moving = match &d.drag {
        Some(Drag::Move { placement, delta }) => Some((*placement, *delta)),
        _ => None,
    };
    for o in &analysis.outlines {
        let shifted: Vec<PointMm>;
        let polygon: &[PointMm] = match moving {
            Some((id, (dx, dy))) if id == o.placement => {
                shifted = o
                    .polygon
                    .iter()
                    .map(|p| PointMm::new(p.x + dx, p.y + dy))
                    .collect();
                &shifted
            }
            _ => &o.polygon,
        };
        let (over, flow) = (overlapping(o.placement), overflowing(o.placement));
        let defect = on_defect(o.placement);
        let selected = d.placement == Some(o.placement);
        fill_polygon(painter, geo, polygon, tokens.action.gamma_multiply(0.18));
        if defect {
            // Défaut touché : hachures verticales (jamais la couleur seule).
            hatch(
                painter,
                geo,
                polygon,
                90.0,
                Stroke::new(1.4_f32, tokens.danger),
            );
        }
        if over {
            hatch(
                painter,
                geo,
                polygon,
                45.0,
                Stroke::new(1.4_f32, tokens.danger),
            );
        }
        if flow {
            hatch(
                painter,
                geo,
                polygon,
                135.0,
                Stroke::new(1.4_f32, tokens.warning),
            );
        }
        let color = if over || defect {
            tokens.danger
        } else if flow {
            tokens.warning
        } else {
            tokens.action
        };
        outline(
            painter,
            geo,
            polygon,
            Stroke::new(
                if selected { 3.0_f32 } else { 1.5_f32 },
                if selected { tokens.focus_ring } else { color },
            ),
        );
        let c = geo.to_screen(
            (o.bbox.min.x + o.bbox.max.x) / 2.0
                + moving.map_or(0.0, |(id, dl)| if id == o.placement { dl.0 } else { 0.0 }),
            (o.bbox.min.y + o.bbox.max.y) / 2.0
                + moving.map_or(0.0, |(id, dl)| if id == o.placement { dl.1 } else { 0.0 }),
        );
        painter.text(
            c,
            egui::Align2::CENTER_CENTER,
            pose_name(project, plan, o.placement),
            egui::FontId::proportional(10.0),
            tokens.text_primary,
        );
    }
    if let Some(Drag::New {
        shape, rotation, ..
    }) = &d.drag
    {
        if let Some(p) = painter.ctx().input(|i| i.pointer.hover_pos()) {
            let (px, py) = geo.to_mm(p);
            let poly = place_polygon(&shape.polygon, 0.0, 0.0, *rotation, false);
            let (mut lo, mut hi) = (
                PointMm::new(f64::MAX, f64::MAX),
                PointMm::new(f64::MIN, f64::MIN),
            );
            for q in &poly {
                lo = PointMm::new(lo.x.min(q.x), lo.y.min(q.y));
                hi = PointMm::new(hi.x.max(q.x), hi.y.max(q.y));
            }
            let (ox, oy) = (
                px - (hi.x - lo.x) / 2.0 - lo.x,
                py - (hi.y - lo.y) / 2.0 - lo.y,
            );
            let ghost: Vec<PointMm> = poly
                .iter()
                .map(|q| PointMm::new(q.x + ox, q.y + oy))
                .collect();
            fill_polygon(painter, geo, &ghost, tokens.action.gamma_multiply(0.3));
            outline(painter, geo, &ghost, Stroke::new(1.5_f32, tokens.action));
        }
    }
}

fn placed_section(ui: &mut egui::Ui, d: &mut CuttingDialog, project: &Project, plan: &CuttingPlan) {
    section_header(
        ui,
        &mut d.placed_open,
        tr_args(
            "cutting.placed",
            &[("count", &plan.placements.len().to_string())],
        ),
    );
    if !d.placed_open {
        return;
    }
    let analysis = d
        .cache
        .as_ref()
        .map(|c| c.analysis.clone())
        .unwrap_or_default();
    for pl in &plan.placements {
        let mut text = pose_name(project, plan, pl.id);
        if analysis
            .overlaps
            .iter()
            .any(|o| o.a == pl.id || o.b == pl.id)
        {
            text.push_str(&format!(" — {}", tr("cutting.pattern.overlap")));
        }
        if analysis.overflows.iter().any(|o| o.placement == pl.id) {
            text.push_str(&format!(" — {}", tr("cutting.pattern.overflow")));
        }
        let on = d.placement == Some(pl.id);
        if ui.selectable_label(on, text).clicked() {
            d.placement = Some(pl.id);
        }
    }
}

fn selection_section(
    ui: &mut egui::Ui,
    d: &mut CuttingDialog,
    project: &Project,
    plan: &CuttingPlan,
    actions: &mut Vec<CAction>,
) {
    let Some(pl) = d
        .placement
        .and_then(|id| plan.placements.iter().find(|q| q.id == id))
    else {
        let tokens = pf_ui::theme::tokens(ui.ctx());
        crate::a11y::label(
            ui,
            RichText::new(tr("cutting.sel.none")).color(tokens.text_secondary),
        );
        return;
    };
    crate::a11y::label(
        ui,
        RichText::new(tr_args(
            "cutting.sel.title",
            &[("name", &pose_name(project, plan, pl.id))],
        ))
        .strong(),
    );
    let constrained = plan.grain_direction.is_some()
        && project
            .piece_shape(pl.piece, pl.size)
            .is_ok_and(|s| s.grain.is_some());
    let step = if constrained { 180.0 } else { 90.0 };
    let key = |k: u8| (k, pl.id.0, 0);
    egui::Grid::new("cutting_sel")
        .num_columns(2)
        .show(ui, |ui| {
            crate::a11y::label(ui, tr("cutting.rot"));
            ui.horizontal(|ui| {
                if let Some(t) = edit(ui, &mut d.buffers, key(K_ROT), &mm(pl.rotation_deg), 70.0) {
                    actions.push(match parse_num(&t) {
                        Ok(v) => CAction::Rotate(v),
                        Err(e) => CAction::Error(e),
                    });
                }
                ui.label("°");
            });
            ui.end_row();
            for (k, label, value) in [
                (K_X, "cutting.pos_x", pl.x_mm),
                (K_Y, "cutting.pos_y", pl.y_mm),
            ] {
                crate::a11y::label(ui, tr(label));
                ui.horizontal(|ui| {
                    if let Some(t) = edit(
                        ui,
                        &mut d.buffers,
                        key(k),
                        &pf_core::units::field_text(value),
                        80.0,
                    ) {
                        actions.push(match parse_len(&t) {
                            Ok(v) if k == K_X => CAction::SetPos(Some(v), None),
                            Ok(v) => CAction::SetPos(None, Some(v)),
                            Err(e) => CAction::Error(e),
                        });
                    }
                    ui.label(pf_core::units::display_units().unit.suffix());
                });
                ui.end_row();
            }
        });
    ui.horizontal(|ui| {
        if button(
            ui,
            &tr_args("cutting.rot_step", &[("deg", &mm(step))]),
            ButtonKind::Secondary,
        )
        .clicked()
        {
            actions.push(CAction::Rotate((pl.rotation_deg + step).rem_euclid(360.0)));
        }
        let label = if pl.mirrored {
            tr("cutting.mirror.on")
        } else {
            tr("cutting.mirror")
        };
        if button(ui, &label, ButtonKind::Secondary).clicked() {
            actions.push(CAction::Mirror);
        }
        if button(ui, &tr("cutting.delete"), ButtonKind::Danger).clicked() {
            actions.push(CAction::Delete);
        }
    });
}

fn diag_lines(project: &Project, plan: &CuttingPlan, a: &PlanAnalysis) -> Vec<String> {
    let nm = |id: PlacementId| pose_name(project, plan, id);
    let piece = |id: PieceId| {
        project
            .piece(id)
            .map(|p| p.name.clone())
            .unwrap_or_default()
    };
    let mut v = Vec::new();
    for o in &a.overlaps {
        v.push(tr_args(
            "cutting.diag.overlap",
            &[("a", &nm(o.a)), ("b", &nm(o.b)), ("area", &mm(o.area_mm2))],
        ));
    }
    for g in &a.gaps {
        v.push(pf_i18n::tr_len(
            "cutting.diag.gap",
            &[
                ("a", &nm(g.a)),
                ("b", &nm(g.b)),
                ("dist", &len(g.distance_mm)),
                ("req", &len(g.required_mm)),
            ],
        ));
    }
    for o in &a.overflows {
        for r in &o.reasons {
            let key = match r {
                OverflowReason::OutsideUseful => "cutting.diag.outside_useful",
                OverflowReason::BeforeStart => "cutting.diag.before_start",
                OverflowReason::CrossesFold => "cutting.diag.crosses_fold",
            };
            v.push(tr_args(key, &[("name", &nm(o.placement))]));
        }
    }
    v.extend(crate::cutting_hide::diag_lines(&nm, a));
    for r in &a.rotation_refused {
        v.push(tr_args(
            "cutting.diag.rotation",
            &[("name", &nm(r.placement)), ("dev", &mm(r.deviation_deg))],
        ));
    }
    for (id, detail) in &a.unreadable {
        v.push(tr_args(
            "cutting.diag.unreadable",
            &[("name", &nm(*id)), ("detail", detail)],
        ));
    }
    for o in &a.over_placed {
        v.push(tr_args(
            "cutting.diag.over_placed",
            &[
                ("name", &piece(o.piece)),
                ("placed", &o.placed.to_string()),
                ("expected", &o.expected.to_string()),
            ],
        ));
    }
    v
}

fn warn_lines(project: &Project, plan: &CuttingPlan, a: &PlanAnalysis) -> Vec<String> {
    let piece = |id: PieceId| {
        project
            .piece(id)
            .map(|p| p.name.clone())
            .unwrap_or_default()
    };
    let mut v = Vec::new();
    for w in &a.warnings {
        let key = match w.kind {
            WarningKind::PieceWithoutGrain => "cutting.diag.piece_without_grain",
            WarningKind::PlanWithoutGrain => "cutting.diag.plan_without_grain",
            WarningKind::MirrorWithoutPair => "cutting.diag.mirror_without_pair",
        };
        v.push(tr_args(
            key,
            &[("name", &pose_name(project, plan, w.placement))],
        ));
    }
    for id in &a.unplaced {
        v.push(tr_args("cutting.diag.unplaced", &[("name", &piece(*id))]));
    }
    v
}

fn diag_section(ui: &mut egui::Ui, d: &mut CuttingDialog, project: &Project, plan: &CuttingPlan) {
    let tokens = pf_ui::theme::tokens(ui.ctx());
    let Some(a) = d.cache.as_ref().map(|c| c.analysis.clone()) else {
        return;
    };
    let defects = diag_lines(project, plan, &a);
    let notes = warn_lines(project, plan, &a);
    section_header(
        ui,
        &mut d.diag_open,
        tr_args(
            "cutting.diag",
            &[("count", &(defects.len() + notes.len()).to_string())],
        ),
    );
    if !d.diag_open {
        return;
    }
    if a.is_valid() {
        crate::a11y::label(
            ui,
            RichText::new(tr("cutting.diag.ok")).color(tokens.success),
        );
    } else {
        crate::a11y::label(
            ui,
            RichText::new(tr("cutting.diag.invalid")).color(tokens.danger),
        );
    }
    for l in &defects {
        crate::a11y::label(ui, RichText::new(format!("✗ {l}")).color(tokens.danger));
    }
    for l in &notes {
        crate::a11y::label(
            ui,
            RichText::new(format!("• {l}")).color(tokens.text_secondary),
        );
    }
}

/// Projet de l'étape « plan-coupe » : trois pièces (Devant en paire, Manche à droit-fil, Poche) et
/// un plan de laize 1400 mm (lisières 20 mm, écart 10 mm, droit-fil 90°) où la Poche chevauche le
/// Devant et où une seconde Poche déborde à droite. Renvoie aussi la pose à sélectionner.
/// alid : variante sans défaut (Poche au-dessus du Devant, pas de seconde Poche) pour le métrage.
pub(crate) fn cutting_stage_project(
    valid: bool,
) -> Result<(Project, CuttingPlanId, PlacementId), pf_core::CoreError> {
    use pf_core::{PathRole, PieceSource, PointRef};
    let mut p = Project::new("Plan de coupe");
    let sheet = p.sheets[0].id;
    type Def<'a> = (
        &'a str,
        &'a [(f64, f64)],
        u32,
        bool,
        Option<((f64, f64), (f64, f64))>,
    );
    let defs: [Def; 3] = [
        (
            "Devant",
            &[(0.0, 0.0), (400.0, 0.0), (400.0, 500.0), (0.0, 500.0)],
            1,
            true,
            Some(((200.0, 50.0), (200.0, 450.0))),
        ),
        (
            "Manche",
            &[(0.0, 0.0), (300.0, 0.0), (250.0, 420.0), (50.0, 420.0)],
            2,
            false,
            Some(((150.0, 40.0), (150.0, 380.0))),
        ),
        (
            "Poche",
            &[(0.0, 0.0), (120.0, 0.0), (120.0, 150.0), (0.0, 150.0)],
            1,
            false,
            None,
        ),
    ];
    let mut ids = Vec::new();
    for (k, (name, verts, quantity, pair, grain)) in defs.iter().enumerate() {
        let ox = 1000.0 * (k as f64 + 1.0);
        let poly = p.apply(Command::AddPolygon {
            sheet,
            vertices: verts
                .iter()
                .map(|v| PointRef::New(PointMm::new(ox + v.0, v.1)))
                .collect(),
            closed: true,
            role: PathRole::default(),
            layer: None,
        })?;
        let piece = p
            .apply(Command::ExtractPiece {
                sheet,
                source: PieceSource::Paths {
                    entities: poly.entities,
                },
                name: (*name).to_string(),
                detached: false,
            })?
            .pieces[0];
        p.apply(Command::SetPieceInfo {
            piece,
            name: (*name).to_string(),
            quantity: *quantity,
            material: None,
            mirrored_pair: *pair,
        })?;
        if let Some((a, b)) = grain {
            let g = p.apply(Command::AddSegment {
                sheet,
                from: PointRef::New(PointMm::new(ox + a.0, a.1)),
                to: PointRef::New(PointMm::new(ox + b.0, b.1)),
            })?;
            p.apply(Command::SetPathRole {
                sheet,
                entity: g.entities[0],
                role: PathRole::Grainline,
            })?;
            let outline = p.piece(piece)?.outline.clone();
            p.apply(Command::SetPieceOutline {
                piece,
                outline,
                grainline: Some(g.entities[0]),
            })?;
        }
        ids.push(piece);
    }
    let plan = p
        .apply(Command::AddCuttingPlan {
            spec: CuttingPlanSpec {
                name: "Coton 140".into(),
                material: None,
                fabric_width_mm: 1400.0,
                fold: FabricFold::Flat,
                selvedge_mm: 20.0,
                min_gap_mm: 10.0,
                grain_direction: Some(90.0),
            },
        })?
        .cutting_plans[0];
    let mut overlapping = None;
    let mut poses = vec![
        (ids[0], 30.0, 0.0, 0.0, false),
        (ids[0], 870.0, 0.0, 0.0, true),
        (ids[1], 900.0, 0.0, 0.0, false),
    ];
    if valid {
        poses.push((ids[2], 30.0, 510.0, 0.0, false));
    } else {
        poses.push((ids[2], 150.0, 400.0, 0.0, false));
        poses.push((ids[2], 1300.0, 100.0, 0.0, false));
    }
    for (piece, x, y, rot, mirrored) in poses {
        let id = p
            .apply(Command::AddPlacement {
                plan,
                piece,
                size: None,
                x_mm: x,
                y_mm: y,
                rotation_deg: rot,
                mirrored,
            })?
            .placements[0];
        if overlapping.is_none() && piece == ids[2] {
            overlapping = Some(id);
        }
    }
    Ok((p, plan, overlapping.unwrap_or(PlacementId(0))))
}

impl App {
    /// Écrit le plan (SVG ou PDF) ou le métrage de tous les plans (CSV) dans `path`, dans la langue
    /// courante. Lecture seule : le projet n'est pas modifié ; un échec ouvre la boîte d'erreur et
    /// ne laisse aucun fichier (écriture par fichier temporaire voisin).
    pub fn write_cutting_to(
        &mut self,
        plan: CuttingPlanId,
        what: CutOut,
        path: &std::path::Path,
    ) -> bool {
        let project = self.editor.project();
        let bytes = match what {
            CutOut::Svg => cutting_svg(project, plan, lang()),
            CutOut::Pdf => cutting_pdf(project, plan, &self.page_setup, lang()),
            CutOut::Csv => cutting_csv(project, lang()),
        };
        let bytes = match bytes {
            Ok(b) => b,
            Err(e) => {
                self.error_dialog = Some(tr_args("export.failed", &[("detail", &e.to_string())]));
                return false;
            }
        };
        // `ReportExport` ne sert ici que d'écriture atomique : son format n'influence pas l'octet.
        let out = ReportExport {
            format: if what == CutOut::Pdf {
                ReportFormat::Pdf
            } else {
                ReportFormat::Csv
            },
            bytes,
            page_count: None,
        };
        match out.write_to(path) {
            Ok(()) => {
                let label = match what {
                    CutOut::Svg => "SVG",
                    CutOut::Pdf => "PDF",
                    CutOut::Csv => "CSV",
                };
                self.editor.status.text = tr_args(
                    "export.done",
                    &[
                        ("what", label),
                        ("note", ""),
                        ("path", &path.display().to_string()),
                    ],
                );
                self.editor.status.error = false;
                true
            }
            Err(e) => {
                self.error_dialog = Some(tr_args("export.failed", &[("detail", &e.to_string())]));
                false
            }
        }
    }

    fn save_cutting_dialog(&mut self, plan: CuttingPlanId, what: CutOut) {
        let project = self.editor.project();
        let name = project
            .cutting_plan(plan)
            .map(|p| p.name.clone())
            .unwrap_or_default();
        let stem = cutting_file_stem(&project.name, &name);
        let (ext, file) = match what {
            CutOut::Svg => ("svg", stem),
            CutOut::Pdf => ("pdf", stem),
            CutOut::Csv => (
                "csv",
                format!("{}-metrage", cutting_file_stem(&project.name, "")),
            ),
        };
        let Some(path) = rfd::FileDialog::new()
            .add_filter(ext.to_uppercase(), &[ext])
            .set_file_name(format!("{file}.{ext}"))
            .save_file()
        else {
            return;
        };
        self.write_cutting_to(plan, what, &path);
    }
}

fn yield_section(
    ui: &mut egui::Ui,
    d: &mut CuttingDialog,
    project: &Project,
    plan: &CuttingPlan,
    actions: &mut Vec<CAction>,
) {
    let tokens = pf_ui::theme::tokens(ui.ctx());
    let Some(a) = d.cache.as_ref().map(|c| c.analysis.clone()) else {
        return;
    };
    section_header(ui, &mut d.yield_open, tr("cutting.yield"));
    if !d.yield_open {
        return;
    }
    let y = plan_yield(project, plan, &a);
    match (&y.status, &y.figures) {
        (YieldStatus::Valid, Some(g)) => {
            let row = |ui: &mut egui::Ui, key: &str, value: String| {
                ui.horizontal(|ui| {
                    crate::a11y::label(ui, tr(key));
                    crate::a11y::label(ui, RichText::new(value).strong());
                });
            };
            row(ui, "cutting.yield.length", len_unit(g.length_mm));
            row(ui, "cutting.yield.useful", len_unit(g.useful_width_mm));
            row(
                ui,
                "cutting.yield.area",
                format!("{} cm²", mm(g.used_area_mm2 / 100.0)),
            );
            row(
                ui,
                "cutting.yield.percent",
                format!("{:.2} %", g.yield_percent),
            );
            crate::a11y::label(ui, RichText::new(tr("cutting.yield.by_size")).strong());
            for r in &g.by_size {
                let size = r
                    .size
                    .and_then(|s| {
                        project
                            .find_size(s)
                            .map(|(rg, i)| rg.sizes[i].label.clone())
                    })
                    .unwrap_or_else(|| tr("cutting.size.base"));
                crate::a11y::label(
                    ui,
                    tr_args(
                        "cutting.yield.row",
                        &[
                            ("size", &size),
                            ("pieces", &r.pieces.to_string()),
                            ("area", &mm(r.used_area_mm2 / 100.0)),
                            ("share", &format!("{:.2}", r.share_percent)),
                        ],
                    ),
                );
            }
        }
        (YieldStatus::Invalid, _) => {
            crate::a11y::label(
                ui,
                RichText::new(tr("cutting.yield.invalid")).color(tokens.danger),
            );
        }
        _ => {
            crate::a11y::label(ui, tr("cutting.yield.empty"));
        }
    }
    crate::a11y::label(
        ui,
        RichText::new(tr("cutting.yield.note"))
            .small()
            .color(tokens.text_secondary),
    );
    ui.horizontal_wrapped(|ui| {
        for (what, key) in [
            (CutOut::Svg, "cutting.export.svg"),
            (CutOut::Pdf, "cutting.export.pdf"),
            (CutOut::Csv, "cutting.export.csv"),
        ] {
            if button(ui, &tr(key), ButtonKind::Secondary).clicked() {
                actions.push(CAction::Export(what));
            }
        }
    });
    crate::a11y::label(
        ui,
        RichText::new(tr("cutting.export.note"))
            .small()
            .color(tokens.text_secondary),
    );
}

/// Pièces et quantités demandées à la proposition : cochées ou toutes celles de la matière du plan,
/// `auto_qty` copies chacune (autant en miroir pour une pièce en paire), à la taille choisie.
fn auto_items(d: &CuttingDialog, project: &Project, plan: CuttingPlanId) -> Vec<AutoItem> {
    let pieces: Vec<PieceId> = if d.auto_all {
        project
            .cutting_plan(plan)
            .map(|p| project.plan_candidate_pieces(p))
            .unwrap_or_default()
    } else {
        d.pick.clone()
    };
    pieces
        .iter()
        .filter_map(|id| project.piece(*id).ok())
        .map(|p| AutoItem {
            piece: p.id,
            size: d.size,
            quantity: d.auto_qty,
            mirrored_quantity: if p.mirrored_pair { d.auto_qty } else { 0 },
        })
        .collect()
}

impl CuttingDialog {
    /// Calcule la proposition tout de suite (étapes de capture et tests) : même appel que le fil.
    pub(crate) fn auto_blocking(&mut self, project: &Project, plan: CuttingPlanId) {
        self.auto_plan = Some(plan);
        let items = auto_items(self, project, plan);
        let options = AutoOptions {
            allow_rotation: self.auto_rot,
            ..AutoOptions::default()
        };
        self.auto_out = Some(propose(
            project,
            plan,
            &items,
            &options,
            self.auto_mode,
            self.hide.required,
            &AtomicBool::new(false),
        ));
    }
}

/// Proposition de placement : sur peau, la proposition dédiée avec l'exigence de qualité
/// (US-15.5) ; sinon celle de la laize, inchangée.
fn propose(
    project: &Project,
    plan: CuttingPlanId,
    items: &[AutoItem],
    options: &AutoOptions,
    mode: AutoMode,
    required: Option<u8>,
    cancel: &AtomicBool,
) -> Result<AutoProposal, AutoError> {
    if project.cutting_plan(plan).is_ok_and(|p| p.hide.is_some()) {
        project.propose_hide_layout(plan, items, options, required, mode, cancel)
    } else {
        project.propose_cutting_layout_mode(plan, items, options, mode, cancel)
    }
}

fn auto_error_lines(project: &Project, e: &AutoError) -> Vec<String> {
    let name = |id: PieceId| {
        project
            .piece(id)
            .map_or_else(|_| format!("#{}", id.0), |p| p.name.clone())
    };
    match e {
        AutoError::NothingToPlace => vec![tr("cutting.auto.err.nothing")],
        AutoError::TooMany(n) => vec![tr_args(
            "cutting.auto.err.too_many",
            &[
                ("n", &n.to_string()),
                ("max", &pf_core::cutting_auto::MAX_INSTANCES.to_string()),
            ],
        )],
        AutoError::MirrorNotAllowed(id) => {
            vec![tr_args("cutting.auto.err.mirror", &[("name", &name(*id))])]
        }
        AutoError::Cancelled => vec![tr("cutting.auto.err.cancelled")],
        AutoError::VerificationFailed(why) => {
            vec![tr_args("cutting.auto.err.verify", &[("why", why)])]
        }
        AutoError::Core(e) => vec![e.to_string()],
        AutoError::NothingToComplete => vec![tr("cutting.auto.err.nothing_to_complete")],
        AutoError::ExistingInvalid(e) => vec![tr_args(
            "cutting.auto.err.existing_invalid",
            &[
                ("overlaps", &e.overlaps.to_string()),
                ("gaps", &e.gaps.to_string()),
                ("overflows", &e.overflows.to_string()),
                ("rotations", &e.rotation_refused.to_string()),
                ("unreadable", &e.unreadable.to_string()),
            ],
        )],
        AutoError::ExistingExceeds(list) => {
            let mut lines = vec![tr("cutting.auto.err.existing_exceeds")];
            for o in list {
                let key = if o.mirrored {
                    "cutting.auto.err.exceeds_line_mirror"
                } else {
                    "cutting.auto.err.exceeds_line"
                };
                lines.push(tr_args(
                    key,
                    &[
                        ("name", &name(o.piece)),
                        ("placed", &o.placed.to_string()),
                        ("wanted", &o.wanted.to_string()),
                    ],
                ));
            }
            lines
        }
        AutoError::Unplaceable(list) => {
            let mut lines = vec![tr("cutting.auto.err.unplaceable")];
            for u in list {
                let n = name(u.piece);
                lines.push(match &u.reason {
                    UnplaceableReason::TooWide { width_mm, span_mm } => pf_i18n::tr_len(
                        "cutting.auto.err.too_wide",
                        &[
                            ("name", &n),
                            ("width", &len(*width_mm)),
                            ("span", &len(*span_mm)),
                        ],
                    ),
                    UnplaceableReason::NoRotation => {
                        tr_args("cutting.auto.err.no_rotation", &[("name", &n)])
                    }
                    UnplaceableReason::Unreadable(why) => {
                        tr_args("cutting.auto.err.unreadable", &[("name", &n), ("why", why)])
                    }
                    UnplaceableReason::NoRoomOnHide => {
                        tr_args("cutting.auto.err.no_room_hide", &[("name", &n)])
                    }
                });
            }
            lines
        }
    }
}

fn auto_section(
    ui: &mut egui::Ui,
    d: &mut CuttingDialog,
    project: &Project,
    actions: &mut Vec<CAction>,
) {
    let tokens = pf_ui::theme::tokens(ui.ctx());
    section_header(ui, &mut d.auto_open, tr("cutting.auto.title"));
    if !d.auto_open {
        return;
    }
    if d.auto_mode == AutoMode::ReplaceAll {
        crate::a11y::label(
            ui,
            RichText::new(tr("cutting.auto.note"))
                .small()
                .color(tokens.text_secondary),
        );
    }
    ui.horizontal(|ui| {
        for (mode, key) in [
            (AutoMode::ReplaceAll, "cutting.auto.mode.replace"),
            (AutoMode::CompleteMissing, "cutting.auto.mode.complete"),
        ] {
            if ui.selectable_label(d.auto_mode == mode, tr(key)).clicked() && d.auto_mode != mode {
                d.auto_mode = mode;
                d.auto_out = None;
            }
        }
    });
    if d.auto_mode == AutoMode::CompleteMissing {
        crate::a11y::label(
            ui,
            RichText::new(tr("cutting.auto.mode.complete.note"))
                .small()
                .color(tokens.text_secondary),
        );
    }
    ui.horizontal(|ui| {
        if ui
            .selectable_label(!d.auto_all, tr("cutting.auto.scope.sel"))
            .clicked()
        {
            d.auto_all = false;
        }
        if ui
            .selectable_label(d.auto_all, tr("cutting.auto.scope.all"))
            .clicked()
        {
            d.auto_all = true;
        }
    });
    ui.horizontal(|ui| {
        crate::a11y::label(ui, tr("cutting.auto.qty"));
        if let Some(t) = edit(
            ui,
            &mut d.buffers,
            (K_AUTO_QTY, 0, 0),
            &d.auto_qty.to_string(),
            50.0,
        ) {
            match t.trim().parse::<u32>() {
                Ok(v) if (1..=50).contains(&v) => d.auto_qty = v,
                _ => actions.push(CAction::Error(tr("cutting.auto.err.qty"))),
            }
        }
    });
    ui.checkbox(&mut d.auto_rot, tr("cutting.auto.rotation"));
    if d.auto_job.is_some() {
        crate::a11y::label(ui, tr("cutting.auto.running"));
        if button(ui, &tr("cutting.auto.cancel"), ButtonKind::Secondary).clicked() {
            actions.push(CAction::AutoCancel);
        }
    } else if button(ui, &tr("cutting.auto.run"), ButtonKind::Primary).clicked() {
        actions.push(CAction::AutoRun);
    }
    match d.auto_out.clone() {
        None => {}
        Some(Err(e)) => {
            for line in auto_error_lines(project, &e) {
                crate::a11y::label(ui, RichText::new(line).color(tokens.danger));
            }
        }
        Some(Ok(p)) => {
            crate::a11y::label(ui, RichText::new(tr("cutting.auto.label")).strong());
            let line = |key: &str, l: f64, y: f64| {
                pf_i18n::tr_len(key, &[("l", &len(l)), ("y", &format!("{y:.2}"))])
            };
            crate::a11y::label(
                ui,
                match p.current {
                    Some(c) => line("cutting.auto.current", c.length_mm, c.yield_percent),
                    None => tr("cutting.auto.current.none"),
                },
            );
            crate::a11y::label(
                ui,
                RichText::new(line(
                    "cutting.auto.proposed",
                    p.proposed.length_mm,
                    p.proposed.yield_percent,
                ))
                .strong(),
            );
            crate::a11y::label(
                ui,
                line(
                    "cutting.auto.level1",
                    p.level1.length_mm,
                    p.level1.yield_percent,
                ),
            );
            crate::a11y::label(
                ui,
                tr(if p.refined {
                    "cutting.auto.refined"
                } else {
                    "cutting.auto.not_refined"
                }),
            );
            if p.time_limited {
                crate::a11y::label(ui, tr("cutting.auto.time_limited"));
            }
            ui.horizontal(|ui| {
                if button(ui, &tr("cutting.auto.apply"), ButtonKind::Primary).clicked() {
                    actions.push(CAction::AutoApply);
                }
                if button(ui, &tr("cutting.auto.discard"), ButtonKind::Secondary).clicked() {
                    actions.push(CAction::AutoDiscard);
                }
            });
        }
    }
}
