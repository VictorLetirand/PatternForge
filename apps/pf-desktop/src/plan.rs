//! Vue « Plan de pièces » (US-04.5) : toutes les pièces du projet à plat, déplaçables et
//! orientables sans changer leur géométrie. C'est une vue de **mise en page** : le
//! placement est une donnée du projet (`Piece.placement`), écrite uniquement par
//! `Command::SetPiecePlacement` ; la construction n'est jamais touchée.
//!
//! Aucun calcul géométrique ici : le contour transformé, la boîte, l'aire, le périmètre
//! et les chevauchements viennent de `pf-core` (`Project::plan_layout`).
//! Limites : pas d'aimantation aux bords, pas d'optimisation de placement (imbrication),
//! pas d'export du plan.

use crate::app::App;
use crate::canvas::{label, stroke, Xf};
use crate::editor::Status;
use crate::palette::pal;
use crate::panels::{buffered_edit, category_label, issue_text, row, section};
use crate::text::core_error;
use crate::view::View;
use eframe::egui::{self, Align2, PointerButton, RichText, Sense, Shape};
use pf_core::input::{format_mm, format_number, parse_angle_deg};
use pf_core::piece::PieceIssue;
use pf_core::piece_plan::overlapping_boxes;
use pf_core::units::parse_length_display;
use pf_core::{Command, PieceCategory, PieceId, Placement, PlanItem, PlanSkip, Project};
use pf_geometry::{point_in_polygon, Bbox, PointMm};
use pf_i18n::{tr, tr_args};
use std::cmp::Ordering;

/// Colonne de tri de la liste des pièces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanSort {
    Name,
    Code,
    Category,
    Quantity,
    Area,
    Perimeter,
    Manual,
}

impl PlanSort {
    pub const ALL: [PlanSort; 7] = [
        PlanSort::Name,
        PlanSort::Code,
        PlanSort::Category,
        PlanSort::Quantity,
        PlanSort::Area,
        PlanSort::Perimeter,
        PlanSort::Manual,
    ];

    fn label(self) -> String {
        tr(match self {
            PlanSort::Name => "plan.col.name",
            PlanSort::Code => "plan.col.code",
            PlanSort::Category => "plan.col.category",
            PlanSort::Quantity => "plan.col.quantity",
            PlanSort::Area => "plan.col.area",
            PlanSort::Perimeter => "plan.col.perimeter",
            PlanSort::Manual => "plan.col.manual",
        })
    }
}

/// Une ligne de la liste des pièces (valeurs lues dans `piece_card`, jamais stockées).
#[derive(Debug, Clone, PartialEq)]
pub struct PlanRow {
    pub id: PieceId,
    pub name: String,
    pub code: Option<String>,
    pub category: Option<PieceCategory>,
    pub quantity: u32,
    pub area_mm2: Option<f64>,
    pub perimeter_mm: Option<f64>,
    /// Placée à la main (placement enregistré), sinon placement automatique.
    pub manual: bool,
    pub issues: Vec<PieceIssue>,
    pub skipped: Option<PlanSkip>,
}

fn opt_cmp<T>(
    a: Option<T>,
    b: Option<T>,
    ascending: bool,
    f: impl Fn(&T, &T) -> Ordering,
) -> Ordering {
    match (a, b) {
        (Some(x), Some(y)) if ascending => f(&x, &y),
        (Some(x), Some(y)) => f(&y, &x),
        // Une valeur absente est toujours rangée en dernier, quel que soit le sens.
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

/// Trie les lignes selon la colonne ; à valeurs égales, ordre des identifiants.
pub fn sort_rows(rows: &mut [PlanRow], key: PlanSort, ascending: bool) {
    let dir = |o: Ordering| if ascending { o } else { o.reverse() };
    rows.sort_by(|a, b| {
        let by_key = match key {
            PlanSort::Name => dir(a.name.to_lowercase().cmp(&b.name.to_lowercase())),
            PlanSort::Code => opt_cmp(
                a.code.as_ref().map(|c| c.to_lowercase()),
                b.code.as_ref().map(|c| c.to_lowercase()),
                ascending,
                Ord::cmp,
            ),
            PlanSort::Category => {
                let index = |r: &PlanRow| {
                    r.category
                        .and_then(|c| PieceCategory::ALL.iter().position(|x| *x == c))
                };
                opt_cmp(index(a), index(b), ascending, Ord::cmp)
            }
            PlanSort::Quantity => dir(a.quantity.cmp(&b.quantity)),
            PlanSort::Area => opt_cmp(a.area_mm2, b.area_mm2, ascending, f64::total_cmp),
            PlanSort::Perimeter => {
                opt_cmp(a.perimeter_mm, b.perimeter_mm, ascending, f64::total_cmp)
            }
            PlanSort::Manual => dir(a.manual.cmp(&b.manual)),
        };
        by_key.then(a.id.cmp(&b.id))
    });
}

/// Lecture du plan prise une fois par image (lecture pure du projet).
#[derive(Debug, Clone, Default)]
pub struct PlanSnapshot {
    pub items: Vec<PlanItem>,
    /// Paires de pièces dont les boîtes englobantes se recouvrent (information).
    pub overlaps: Vec<(PieceId, PieceId)>,
    pub rows: Vec<PlanRow>,
}

impl PlanSnapshot {
    fn build(project: &Project) -> PlanSnapshot {
        let items = project.plan_layout();
        let overlaps = overlapping_boxes(&items);
        let rows = items
            .iter()
            .filter_map(|it| {
                let card = project.piece_card(it.piece).ok()?;
                Some(PlanRow {
                    id: it.piece,
                    name: card.name,
                    code: card.code,
                    category: card.category,
                    quantity: card.quantity,
                    area_mm2: card.area_mm2,
                    perimeter_mm: card.perimeter_mm,
                    manual: it.manual,
                    issues: card.issues,
                    skipped: it.skipped.clone(),
                })
            })
            .collect();
        PlanSnapshot {
            items,
            overlaps,
            rows,
        }
    }

    fn item(&self, id: PieceId) -> Option<&PlanItem> {
        self.items.iter().find(|i| i.piece == id)
    }

    /// Boîte englobante de tout le plan (ou d'une seule pièce).
    fn bounds(&self, only: Option<PieceId>) -> Option<Bbox> {
        let mut out: Option<Bbox> = None;
        for bb in self
            .items
            .iter()
            .filter(|i| only.is_none_or(|id| id == i.piece))
            .filter_map(|i| i.bbox)
        {
            match &mut out {
                Some(total) => total.union(bb),
                None => out = Some(bb),
            }
        }
        out
    }
}

/// Glisser d'une pièce en cours : une seule étape d'annulation (groupe du document).
#[derive(Debug, Clone, Copy)]
pub struct PlanDrag {
    piece: PieceId,
    /// Point du plan saisi au début du glisser (mm).
    grab: PointMm,
    /// Placement effectif au début du glisser.
    start: Placement,
}

/// État de la vue « Plan de pièces » : jamais enregistré dans le projet.
pub struct PlanState {
    /// Vue de la feuille, rendue en quittant le plan.
    saved_view: View,
    fit_pending: bool,
    /// Pièce à cadrer au prochain affichage (clic sur une ligne de la liste).
    frame_piece: Option<PieceId>,
    pub selected: Option<PieceId>,
    pub sort: PlanSort,
    pub ascending: bool,
    pub drag: Option<PlanDrag>,
    pub snapshot: PlanSnapshot,
}

/// Pas des boutons de rotation (degrés).
const ROTATE_STEPS: [f64; 4] = [-90.0, -15.0, 15.0, 90.0];

impl App {
    /// Ouvre ou ferme la vue « Plan de pièces ».
    pub fn toggle_plan(&mut self) {
        if self.plan.is_some() {
            self.leave_plan();
            return;
        }
        // Un tracé à moitié fait sur la feuille ne doit pas rester en suspens.
        self.editor.set_tool(crate::editor::Tool::Select);
        self.plan = Some(PlanState {
            saved_view: self.view,
            fit_pending: true,
            frame_piece: None,
            selected: None,
            sort: PlanSort::Name,
            ascending: true,
            drag: None,
            snapshot: PlanSnapshot::build(self.editor.project()),
        });
    }

    /// Quitte le plan (sans effet s'il n'est pas ouvert) : un glisser en cours est annulé.
    pub fn leave_plan(&mut self) {
        if let Some(plan) = self.plan.take() {
            if plan.drag.is_some() {
                self.editor.doc.cancel_group();
            }
            self.view = plan.saved_view;
        }
    }

    /// Relit le plan dans le projet (une fois par image, et après chaque commande).
    pub fn plan_refresh(&mut self) {
        let Some(plan) = self.plan.as_mut() else {
            return;
        };
        plan.snapshot = PlanSnapshot::build(self.editor.doc.project());
        if plan
            .selected
            .is_some_and(|id| plan.snapshot.item(id).is_none())
        {
            plan.selected = None;
        }
        if plan
            .drag
            .is_some_and(|d| plan.snapshot.item(d.piece).is_none())
        {
            plan.drag = None;
            self.editor.doc.cancel_group();
        }
    }

    /// Recadre tout le plan au prochain affichage.
    pub fn plan_fit(&mut self) {
        if let Some(plan) = self.plan.as_mut() {
            plan.fit_pending = true;
        }
    }

    /// Échap dans le plan : annule un glisser en cours sans trace, sinon désélectionne.
    pub fn plan_escape(&mut self) {
        let Some(plan) = self.plan.as_mut() else {
            return;
        };
        if plan.drag.take().is_some() {
            self.editor.doc.cancel_group();
            self.editor.info(tr("status.cancelled"));
            self.plan_refresh();
        } else {
            plan.selected = None;
        }
    }

    /// Lignes de la liste, triées selon la colonne et le sens courants.
    pub fn plan_sorted_rows(&self) -> Vec<PlanRow> {
        let Some(plan) = &self.plan else {
            return Vec::new();
        };
        let mut rows = plan.snapshot.rows.clone();
        sort_rows(&mut rows, plan.sort, plan.ascending);
        rows
    }

    /// Exécute une commande de placement (une étape d'annulation) et relit le plan.
    fn plan_exec(&mut self, ctx: &egui::Context, command: Command) {
        if self.editor.exec(command).is_some() {
            self.plan_refresh();
        }
        ctx.request_repaint();
    }

    /// Pièce sous un point du plan (la sélectionnée est dessinée au-dessus des autres).
    fn plan_hit(&self, p: PointMm) -> Option<PieceId> {
        let plan = self.plan.as_ref()?;
        let mut order: Vec<&PlanItem> = plan.snapshot.items.iter().rev().collect();
        if let Some(sel) = plan.selected {
            order.sort_by_key(|i| i.piece != sel);
        }
        order
            .into_iter()
            .find(|i| {
                if i.outline.len() >= 3 {
                    point_in_polygon(p, &i.outline)
                } else {
                    i.bbox.is_some_and(|b| b.contains(p))
                }
            })
            .map(|i| i.piece)
    }

    fn plan_begin_drag(&mut self, origin: PointMm) {
        let Some(piece) = self.plan_hit(origin) else {
            if let Some(plan) = self.plan.as_mut() {
                plan.selected = None;
            }
            return;
        };
        let Some(plan) = self.plan.as_mut() else {
            return;
        };
        plan.selected = Some(piece);
        let Some(start) = plan.snapshot.item(piece).and_then(|i| i.placement) else {
            return;
        };
        self.editor.doc.begin_group();
        plan.drag = Some(PlanDrag {
            piece,
            grab: origin,
            start,
        });
    }

    fn plan_drag_to(&mut self, world: PointMm) {
        self.editor.cursor = world;
        let Some(d) = self.plan.as_ref().and_then(|p| p.drag) else {
            return;
        };
        let (dx, dy) = (world.x - d.grab.x, world.y - d.grab.y);
        if dx == 0.0 && dy == 0.0 {
            return;
        }
        let command = Command::SetPiecePlacement {
            piece: d.piece,
            placement: Some(Placement {
                x_mm: d.start.x_mm + dx,
                y_mm: d.start.y_mm + dy,
                rotation_deg: d.start.rotation_deg,
            }),
        };
        match self.editor.doc.execute(command) {
            Ok(_) => self.editor.status = Status::default(),
            Err(e) => self.editor.error(core_error(&e)),
        }
        self.plan_refresh();
    }

    fn plan_end_drag(&mut self) {
        if let Some(plan) = self.plan.as_mut() {
            if plan.drag.take().is_some() {
                self.editor.doc.end_group();
            }
        }
    }

    fn plan_apply_fit(&mut self, rect: egui::Rect) {
        let size = (f64::from(rect.width()), f64::from(rect.height()));
        let Some(plan) = self.plan.as_mut() else {
            return;
        };
        if let Some(id) = plan.frame_piece.take() {
            if let Some(bb) = plan.snapshot.bounds(Some(id)) {
                self.view.fit(Some(bb), size);
                return;
            }
        }
        if plan.fit_pending {
            plan.fit_pending = false;
            let bb = plan.snapshot.bounds(None);
            self.view.fit(bb, size);
        }
    }

    // ----- barre d'outils -------------------------------------------------------

    pub fn plan_toolbar(&mut self, ctx: &egui::Context) {
        let (mut back, mut fit) = (false, false);
        let count = self
            .plan
            .as_ref()
            .map(|p| {
                let manual = p.snapshot.rows.iter().filter(|r| r.manual).count();
                (p.snapshot.rows.len(), manual)
            })
            .unwrap_or_default();
        egui::TopBottomPanel::top("tools").show(ctx, |ui| {
            ui.add_space(3.0);
            ui.horizontal_wrapped(|ui| {
                crate::a11y::label(ui, RichText::new(tr("plan.title")).strong());
                ui.separator();
                back = ui.button(tr("plan.back")).clicked();
                fit = ui.button(tr("plan.fit")).clicked();
                ui.separator();
                crate::a11y::label(
                    ui,
                    tr_args(
                        "plan.count",
                        &[
                            ("count", &count.0.to_string()),
                            ("manual", &count.1.to_string()),
                        ],
                    ),
                );
            });
            crate::a11y::label(
                ui,
                RichText::new(tr("plan.note")).small().color(pal().muted),
            );
            ui.add_space(2.0);
        });
        if fit {
            self.plan_fit();
        }
        if back {
            self.leave_plan();
        }
    }

    // ----- panneau de droite : pièce sélectionnée --------------------------------

    pub fn plan_inspector(&mut self, ctx: &egui::Context) {
        let mut command: Option<Command> = None;
        egui::SidePanel::right("inspector")
            .resizable(true)
            .default_width(270.0)
            .min_width(230.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    section(ui, &tr("plan.title"));
                    crate::a11y::label(
                        ui,
                        RichText::new(tr("plan.help")).small().color(pal().muted),
                    );
                    let selected = self.plan.as_ref().and_then(|p| {
                        let id = p.selected?;
                        Some((
                            p.snapshot.item(id)?.clone(),
                            p.snapshot.rows.iter().find(|r| r.id == id)?.clone(),
                        ))
                    });
                    let Some((item, row_data)) = selected else {
                        ui.add_space(8.0);
                        crate::a11y::label(ui, tr("plan.no_selection"));
                        return;
                    };
                    ui.add_space(6.0);
                    ui.heading(tr_args("plan.selected", &[("name", &item.name)]));
                    if let Some(skip) = &item.skipped {
                        crate::a11y::label(
                            ui,
                            RichText::new(match skip {
                                PlanSkip::EmptyOutline => tr("plan.skip.empty_outline"),
                                PlanSkip::Unreadable => tr("plan.skip.outline_unreadable"),
                            })
                            .color(pal().muted),
                        );
                    }
                    if let Some(current) = item.placement.filter(|_| item.skipped.is_none()) {
                        section(ui, &tr("plan.placement.title"));
                        crate::a11y::label(
                            ui,
                            if item.manual {
                                tr("plan.placement.manual")
                            } else {
                                tr("plan.placement.auto")
                            },
                        );
                        let key = item.piece.0 << 8;
                        let mut next: Option<Placement> = None;
                        egui::Grid::new("plan-placement")
                            .num_columns(2)
                            .show(ui, |ui| {
                                crate::a11y::label(ui, tr("plan.x"));
                                let text = pf_core::units::field_text(current.x_mm);
                                if let Some(t) =
                                    buffered_edit(ui, &mut self.text_buf, key | 0x91, &text, 90.0)
                                {
                                    match parse_length_display(&t) {
                                        Ok(x) => next = Some(Placement { x_mm: x, ..current }),
                                        Err(_) => self.editor.error(tr("plan.invalid_number")),
                                    }
                                }
                                ui.end_row();
                                crate::a11y::label(ui, tr("plan.y"));
                                let text = pf_core::units::field_text(current.y_mm);
                                if let Some(t) =
                                    buffered_edit(ui, &mut self.text_buf, key | 0x92, &text, 90.0)
                                {
                                    match parse_length_display(&t) {
                                        Ok(y) => next = Some(Placement { y_mm: y, ..current }),
                                        Err(_) => self.editor.error(tr("plan.invalid_number")),
                                    }
                                }
                                ui.end_row();
                                crate::a11y::label(ui, tr("plan.rotation"));
                                let text = format_number(current.rotation_deg, 2);
                                if let Some(t) =
                                    buffered_edit(ui, &mut self.text_buf, key | 0x93, &text, 90.0)
                                {
                                    match parse_angle_deg(&t) {
                                        Ok(r) => {
                                            next = Some(Placement {
                                                rotation_deg: r,
                                                ..current
                                            })
                                        }
                                        Err(_) => self.editor.error(tr("plan.invalid_number")),
                                    }
                                }
                                ui.end_row();
                            });
                        ui.horizontal(|ui| {
                            for step in ROTATE_STEPS {
                                let text = format!(
                                    "{}{}°",
                                    if step < 0.0 { "−" } else { "+" },
                                    step.abs()
                                );
                                if ui
                                    .button(text)
                                    .on_hover_text(tr("plan.rotate.hover"))
                                    .clicked()
                                {
                                    next = Some(Placement {
                                        rotation_deg: current.rotation_deg + step,
                                        ..current
                                    });
                                }
                            }
                        });
                        if let Some(placement) = next {
                            command = Some(Command::SetPiecePlacement {
                                piece: item.piece,
                                placement: Some(placement),
                            });
                        }
                    }
                    if ui
                        .add_enabled(item.manual, egui::Button::new(tr("plan.reset_auto")))
                        .on_hover_text(tr("plan.reset_auto.hover"))
                        .clicked()
                    {
                        command = Some(Command::SetPiecePlacement {
                            piece: item.piece,
                            placement: None,
                        });
                    }
                    if let (Some(area), Some(perimeter)) = (item.area_mm2, item.perimeter_mm) {
                        section(ui, &tr("piece.measure.title"));
                        row(
                            ui,
                            &tr("piece.measure.area"),
                            &format!("{} cm²", format_number(area / 100.0, 2)),
                        );
                        row(ui, &tr("piece.measure.perimeter"), &format_mm(perimeter));
                        crate::a11y::label(
                            ui,
                            RichText::new(tr("plan.measure.note"))
                                .small()
                                .color(pal().muted),
                        );
                    }
                    // Avertissements : texte et symbole, jamais la couleur seule.
                    for issue in &row_data.issues {
                        crate::a11y::label(
                            ui,
                            RichText::new(tr_args(
                                "piece.warning.line",
                                &[("text", &issue_text(issue))],
                            ))
                            .small()
                            .color(ui.visuals().warn_fg_color),
                        );
                    }
                    // Rotation et tolérance du droit-fil (US-06.5) : valeurs observées.
                    if let Some(r) = self.editor.project().piece_grain_rotation(item.piece) {
                        use pf_core::input::format_deg;
                        let text = tr_args(
                            "piece.grain.rotation",
                            &[
                                ("rotation", &format_deg(r.rotation_deg)),
                                ("deviation", &format_deg(r.deviation_deg)),
                                ("tolerance", &format_deg(r.tolerance_deg)),
                            ],
                        );
                        let color = if r.exceeded() {
                            ui.visuals().warn_fg_color
                        } else {
                            pal().muted
                        };
                        crate::a11y::label(ui, RichText::new(text).small().color(color));
                    }
                    // Chevauchements de boîtes avec cette pièce (information seulement).
                    let names: Vec<String> = self
                        .plan
                        .as_ref()
                        .map(|p| {
                            p.snapshot
                                .overlaps
                                .iter()
                                .filter_map(|&(a, b)| {
                                    let other = if a == item.piece {
                                        b
                                    } else if b == item.piece {
                                        a
                                    } else {
                                        return None;
                                    };
                                    p.snapshot.item(other).map(|i| i.name.clone())
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    if !names.is_empty() {
                        crate::a11y::label(
                            ui,
                            RichText::new(tr_args(
                                "plan.overlap.with",
                                &[("names", &names.join(", "))],
                            ))
                            .small(),
                        );
                    }
                });
            });
        if let Some(command) = command {
            self.plan_exec(ctx, command);
        }
    }

    // ----- liste triable ---------------------------------------------------------

    fn plan_list_panel(&mut self, ctx: &egui::Context) {
        let rows = self.plan_sorted_rows();
        let Some((sort, ascending, selected)) = self
            .plan
            .as_ref()
            .map(|p| (p.sort, p.ascending, p.selected))
        else {
            return;
        };
        let overlaps: Vec<(String, String)> = self
            .plan
            .as_ref()
            .map(|p| {
                p.snapshot
                    .overlaps
                    .iter()
                    .filter_map(|&(a, b)| {
                        Some((
                            p.snapshot.item(a)?.name.clone(),
                            p.snapshot.item(b)?.name.clone(),
                        ))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let mut clicked_sort: Option<PlanSort> = None;
        let mut clicked_row: Option<PieceId> = None;
        egui::TopBottomPanel::bottom("plan_list")
            .resizable(true)
            .default_height(190.0)
            .show(ctx, |ui| {
                section(ui, &tr("plan.list.title"));
                if !overlaps.is_empty() {
                    let shown: Vec<String> = overlaps
                        .iter()
                        .take(5)
                        .map(|(a, b)| tr_args("plan.overlap.pair", &[("a", a), ("b", b)]))
                        .collect();
                    crate::a11y::label(
                        ui,
                        RichText::new(tr_args(
                            "plan.overlap.title",
                            &[
                                ("count", &overlaps.len().to_string()),
                                ("pairs", &shown.join(" ; ")),
                            ],
                        ))
                        .small(),
                    );
                }
                if rows.is_empty() {
                    crate::a11y::label(ui, tr("plan.empty"));
                    return;
                }
                egui::ScrollArea::both()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        egui::Grid::new("plan-rows")
                            .num_columns(PlanSort::ALL.len() + 1)
                            .striped(true)
                            .show(ui, |ui| {
                                for col in PlanSort::ALL {
                                    let arrow = if col == sort {
                                        if ascending {
                                            " ^"
                                        } else {
                                            " v"
                                        }
                                    } else {
                                        ""
                                    };
                                    let text =
                                        RichText::new(format!("{}{arrow}", col.label())).strong();
                                    if ui.add(egui::Button::new(text).frame(false)).clicked() {
                                        clicked_sort = Some(col);
                                    }
                                }
                                crate::a11y::label(
                                    ui,
                                    RichText::new(tr("plan.col.warnings")).strong(),
                                );
                                ui.end_row();
                                for r in &rows {
                                    if ui
                                        .selectable_label(selected == Some(r.id), &r.name)
                                        .clicked()
                                    {
                                        clicked_row = Some(r.id);
                                    }
                                    crate::a11y::label(
                                        ui,
                                        r.code.clone().unwrap_or_else(|| "—".into()),
                                    );
                                    crate::a11y::label(
                                        ui,
                                        match r.category {
                                            Some(c) => category_label(Some(c)),
                                            None => "—".into(),
                                        },
                                    );
                                    crate::a11y::label(ui, r.quantity.to_string());
                                    crate::a11y::label(
                                        ui,
                                        r.area_mm2
                                            .map_or("—".into(), |a| format_number(a / 100.0, 2)),
                                    );
                                    crate::a11y::label(
                                        ui,
                                        r.perimeter_mm.map_or("—".into(), format_mm),
                                    );
                                    crate::a11y::label(
                                        ui,
                                        if r.manual {
                                            tr("plan.yes")
                                        } else {
                                            tr("plan.no")
                                        },
                                    );
                                    if r.issues.is_empty() {
                                        crate::a11y::label(ui, "");
                                    } else {
                                        let all: Vec<String> =
                                            r.issues.iter().map(issue_text).collect();
                                        crate::a11y::label(
                                            ui,
                                            RichText::new(format!("⚠ {}", r.issues.len()))
                                                .color(ui.visuals().warn_fg_color),
                                        )
                                        .on_hover_text(all.join("\n"));
                                    }
                                    ui.end_row();
                                }
                            });
                    });
            });
        if let Some(plan) = self.plan.as_mut() {
            if let Some(col) = clicked_sort {
                if plan.sort == col {
                    plan.ascending = !plan.ascending;
                } else {
                    plan.sort = col;
                    plan.ascending = true;
                }
            }
            if let Some(id) = clicked_row {
                plan.selected = Some(id);
                plan.frame_piece = Some(id);
            }
        }
    }

    // ----- canevas ------------------------------------------------------------------

    pub fn plan_canvas(&mut self, ctx: &egui::Context) {
        self.plan_list_panel(ctx);
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(pf_ui::theme::tokens(ctx).surface_canvas))
            .show(ctx, |ui| {
                let (response, painter) =
                    ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
                let rect = response.rect;
                self.canvas_rect = rect;
                let center = (f64::from(rect.center().x), f64::from(rect.center().y));
                self.plan_apply_fit(rect);
                self.plan_input(ctx, &response, center);
                let xf = Xf {
                    view: self.view,
                    center,
                };
                self.paint_grid(&painter, rect, &xf);
                self.paint_plan(ctx, &painter, &xf);
            });
    }

    fn plan_input(&mut self, ctx: &egui::Context, response: &egui::Response, center: (f64, f64)) {
        let xf = Xf {
            view: self.view,
            center,
        };
        self.canvas_hovered = false;
        let space = ctx.input(|i| i.key_down(egui::Key::Space));
        if let Some(pos) = response.hover_pos() {
            self.editor.cursor = xf.w(pos);
            let (scroll, pinch) = ctx.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
            let factor = f64::from(pinch) * (f64::from(scroll) * 0.0025).exp();
            if (factor - 1.0).abs() > 1e-6 {
                self.view
                    .zoom_about((f64::from(pos.x), f64::from(pos.y)), center, factor);
            }
        }
        // Déplacement de la vue : bouton du milieu, ou Espace + glisser.
        if response.dragged_by(PointerButton::Middle)
            || (space && response.dragged_by(PointerButton::Primary))
        {
            let d = response.drag_delta();
            self.view.pan_pixels(f64::from(d.x), f64::from(d.y));
            return;
        }
        if response.drag_started_by(PointerButton::Primary) {
            if let Some(origin) = ctx.input(|i| i.pointer.press_origin()).map(|p| xf.w(p)) {
                self.plan_begin_drag(origin);
            }
        }
        if response.dragged_by(PointerButton::Primary) {
            if let Some(pos) = response.interact_pointer_pos() {
                self.plan_drag_to(xf.w(pos));
            }
        }
        if response.drag_stopped_by(PointerButton::Primary) {
            self.plan_end_drag();
        }
        if response.clicked_by(PointerButton::Primary) {
            if let Some(pos) = response.interact_pointer_pos() {
                let hit = self.plan_hit(xf.w(pos));
                if let Some(plan) = self.plan.as_mut() {
                    plan.selected = hit;
                }
            }
        }
        // Clic droit : annule un glisser en cours sans trace, sinon désélectionne.
        if response.clicked_by(PointerButton::Secondary) {
            self.plan_escape();
        }
    }

    fn paint_plan(&self, ctx: &egui::Context, painter: &egui::Painter, xf: &Xf) {
        let Some(plan) = &self.plan else {
            return;
        };
        let tokens = pf_ui::theme::tokens(ctx);
        let snapshot = &plan.snapshot;
        if snapshot.rows.is_empty() {
            painter.text(
                painter.clip_rect().center(),
                Align2::CENTER_CENTER,
                tr("plan.empty"),
                egui::FontId::proportional(14.0),
                pal().muted,
            );
            return;
        }
        let overlapped = |id: PieceId| snapshot.overlaps.iter().any(|&(a, b)| a == id || b == id);
        // La pièce sélectionnée est dessinée en dernier (au-dessus).
        let mut order: Vec<&PlanItem> =
            snapshot.items.iter().filter(|i| i.bbox.is_some()).collect();
        order.sort_by_key(|i| Some(i.piece) == plan.selected);
        for it in order {
            let pts: Vec<egui::Pos2> = it.outline.iter().map(|p| xf.s(*p)).collect();
            if pts.len() < 2 {
                continue;
            }
            let is_selected = Some(it.piece) == plan.selected;
            if is_selected {
                painter.add(Shape::closed_line(
                    pts.clone(),
                    stroke(7.0, pal().select.gamma_multiply(0.45)),
                ));
            }
            painter.add(Shape::closed_line(
                pts,
                stroke(if is_selected { 3.0 } else { 2.0 }, tokens.role_cutting),
            ));
            let Some(bb) = it.bbox else { continue };
            // Boîtes qui se recouvrent : cadre pointillé (motif, pas seulement la couleur).
            if overlapped(it.piece) {
                let corners = [
                    xf.s(bb.min),
                    xf.s(PointMm::new(bb.max.x, bb.min.y)),
                    xf.s(bb.max),
                    xf.s(PointMm::new(bb.min.x, bb.max.y)),
                    xf.s(bb.min),
                ];
                painter.extend(Shape::dashed_line(
                    &corners,
                    stroke(1.5, pal().error),
                    6.0,
                    4.0,
                ));
            }
            let flagged = plan
                .snapshot
                .rows
                .iter()
                .any(|r| r.id == it.piece && !r.issues.is_empty());
            let text = if flagged {
                format!("⚠ {}", it.name)
            } else {
                it.name.clone()
            };
            let mid = PointMm::new((bb.min.x + bb.max.x) / 2.0, (bb.min.y + bb.max.y) / 2.0);
            label(painter, xf.s(mid), Align2::CENTER_CENTER, &text, pal().ink);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: u64, name: &str, code: Option<&str>, qty: u32, area: Option<f64>) -> PlanRow {
        PlanRow {
            id: PieceId(id),
            name: name.into(),
            code: code.map(Into::into),
            category: None,
            quantity: qty,
            area_mm2: area,
            perimeter_mm: area.map(|a| a / 10.0),
            manual: id % 2 == 0,
            issues: Vec::new(),
            skipped: None,
        }
    }

    fn names(rows: &[PlanRow]) -> Vec<&str> {
        rows.iter().map(|r| r.name.as_str()).collect()
    }

    #[test]
    fn tri_croissant_decroissant_valeurs_absentes_en_dernier() {
        let mut rows = vec![
            row(1, "b", Some("Z"), 2, Some(30.0)),
            row(2, "A", None, 1, None),
            row(3, "c", Some("a"), 3, Some(10.0)),
        ];
        sort_rows(&mut rows, PlanSort::Name, true);
        assert_eq!(names(&rows), ["A", "b", "c"], "insensible à la casse");
        sort_rows(&mut rows, PlanSort::Name, false);
        assert_eq!(names(&rows), ["c", "b", "A"]);
        sort_rows(&mut rows, PlanSort::Code, true);
        assert_eq!(names(&rows), ["c", "b", "A"], "a < Z, sans code en dernier");
        sort_rows(&mut rows, PlanSort::Code, false);
        assert_eq!(
            names(&rows),
            ["b", "c", "A"],
            "sans code toujours en dernier"
        );
        sort_rows(&mut rows, PlanSort::Area, true);
        assert_eq!(names(&rows), ["c", "b", "A"]);
        sort_rows(&mut rows, PlanSort::Area, false);
        assert_eq!(names(&rows), ["b", "c", "A"]);
        sort_rows(&mut rows, PlanSort::Quantity, false);
        assert_eq!(names(&rows), ["c", "b", "A"]);
        sort_rows(&mut rows, PlanSort::Manual, true);
        assert_eq!(
            names(&rows),
            ["b", "c", "A"],
            "non placées d'abord, puis par id"
        );
    }
}
