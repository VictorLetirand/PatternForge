//! Aligner, répartir, mettre à l'horizontale / à la verticale (US-02.9).
//! Ne calcule rien : le cœur (`Project::plan_*`) planifie un `Batch` de
//! commandes existantes, vérifié avant application ; ici on l'exécute en une
//! seule étape d'historique ou on affiche le refus / le « rien à faire ».

use crate::app::App;
use crate::editor::{Editor, Status};
use eframe::egui;
use pf_core::align::{AlignMode, DistributeMode};
use pf_core::{Command, EntityKind, PointId};
use pf_geometry::PointMm;
use pf_i18n::tr;
use pf_ui::components::{button, ButtonKind, NumericField, Unit};

/// Options d'alignement du menu : six alignements relatifs à la sélection, ou
/// sur la référence (ligne d'aide ou segment sélectionné).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlignChoice {
    Left,
    Right,
    Top,
    Bottom,
    CenterHorizontal,
    CenterVertical,
    OnReference,
}

#[derive(Debug, Clone)]
pub struct LevelDialog {
    pub horizontal: bool,
    pub custom_pivot: bool,
    pub cx: f64,
    pub cy: f64,
}

impl Editor {
    fn say(&mut self, text: String, error: bool) {
        self.status = Status { text, error };
    }

    /// Points visés : points sélectionnés plus extrémités des tracés sélectionnés.
    fn align_points(&self, with_entities: bool) -> Vec<PointId> {
        let sheet = self.sheet();
        let mut pts = self.selection.points.clone();
        if with_entities {
            for &e in &self.selection.entities {
                if let Ok(all) = sheet.entity_all_points(e) {
                    pts.extend(all);
                }
            }
        }
        pts.sort();
        pts.dedup();
        pts
    }

    fn run_plan(
        &mut self,
        plan: Result<Option<Command>, pf_core::CoreError>,
        noop_key: &str,
        done_key: &str,
    ) {
        match plan {
            Err(e) => {
                let text = crate::text::core_error(&e);
                self.say(text, true);
            }
            Ok(None) => self.say(tr(noop_key), false),
            Ok(Some(cmd)) => {
                if self.exec(cmd).is_some() {
                    self.say(tr(done_key), false);
                }
            }
        }
    }

    pub fn align_selection(&mut self, choice: AlignChoice) {
        let sheet = self.sheet;
        let mode = match choice {
            AlignChoice::Left => AlignMode::Left,
            AlignChoice::Right => AlignMode::Right,
            AlignChoice::Top => AlignMode::Top,
            AlignChoice::Bottom => AlignMode::Bottom,
            AlignChoice::CenterHorizontal => AlignMode::CenterHorizontal,
            AlignChoice::CenterVertical => AlignMode::CenterVertical,
            AlignChoice::OnReference => {
                let s = self.sheet();
                let line =
                    if let Some(g) = self.selection.guides.iter().find_map(|g| s.guide(*g).ok()) {
                        Some((g.origin(), g.angle_deg))
                    } else if let [e] = self.selection.entities[..] {
                        match s.entity(e).map(|e| &e.kind) {
                            Ok(EntityKind::Segment { from, to, .. }) => {
                                match (s.position(*from), s.position(*to)) {
                                    (Ok(a), Ok(b)) if pf_geometry::distance_mm(a, b) > 1e-9 => {
                                        Some((a, (b.y - a.y).atan2(b.x - a.x).to_degrees()))
                                    }
                                    _ => None,
                                }
                            }
                            _ => None,
                        }
                    } else {
                        None
                    };
                let Some((origin, angle_deg)) = line else {
                    self.say(tr("status.align.need_reference"), true);
                    return;
                };
                AlignMode::OnLine { origin, angle_deg }
            }
        };
        // Avec une référence, seuls les points sélectionnés sont alignés.
        let pts = self.align_points(choice != AlignChoice::OnReference);
        let plan = self.doc.project().plan_align(sheet, &pts, mode);
        self.run_plan(plan, "status.align.noop", "status.align.done");
    }

    pub fn distribute_selection(&mut self, mode: DistributeMode) {
        let sheet = self.sheet;
        let pts = self.align_points(true);
        let plan = self.doc.project().plan_distribute(sheet, &pts, mode);
        self.run_plan(plan, "status.distribute.noop", "status.distribute.done");
    }

    /// Segments seuls sélectionnés : chacun pivote avec ce qui lui est attaché.
    /// Sinon (plusieurs objets) : la sélection pivote en bloc, le premier
    /// segment étant la référence.
    pub fn level_selection(&mut self, horizontal: bool, pivot: Option<PointMm>) {
        let sheet = self.sheet;
        let sel = &self.selection;
        let segments: Vec<_> = sel
            .entities
            .iter()
            .copied()
            .filter(|&e| {
                matches!(
                    self.sheet().entity(e).map(|e| &e.kind),
                    Ok(EntityKind::Segment { .. })
                )
            })
            .collect();
        if segments.is_empty() {
            self.say(tr("status.level.select"), true);
            return;
        }
        let only_segments = sel.points.is_empty() && segments.len() == sel.entities.len();
        let scope = (!only_segments).then(|| self.align_points(true));
        let plan =
            self.doc
                .project()
                .plan_level(sheet, &segments, horizontal, pivot, scope.as_deref());
        self.run_plan(plan, "status.level.noop", "status.level.done");
    }
}

impl App {
    pub fn open_level(&mut self, horizontal: bool) {
        let sheet = self.editor.sheet();
        let start = self.editor.selection.entities.iter().find_map(|&e| {
            match sheet.entity(e).map(|e| &e.kind) {
                Ok(EntityKind::Segment { from, .. }) => sheet.position(*from).ok(),
                _ => None,
            }
        });
        let Some(start) = start else {
            self.editor.error(tr("status.level.select"));
            return;
        };
        self.level = Some(LevelDialog {
            horizontal,
            custom_pivot: false,
            cx: start.x,
            cy: start.y,
        });
    }

    pub fn level_window(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.level.take() else {
            return;
        };
        let (mut open, mut apply, mut cancel) = (true, false, false);
        let slot = crate::dock::slot(ctx, &self.dock, crate::dock::ToolWin::Level);
        let width = self.dock.content_width(crate::dock::ToolWin::Level);
        egui::Window::new(tr(if d.horizontal {
            "level.title.horizontal"
        } else {
            "level.title.vertical"
        }))
        .id(egui::Id::new("level_window"))
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
                    crate::a11y::label(ui, tr("level.hint"));
                    ui.radio_value(&mut d.custom_pivot, false, tr("level.pivot.start"));
                    ui.radio_value(&mut d.custom_pivot, true, tr("level.pivot.custom"));
                    if d.custom_pivot {
                        egui::Grid::new("level-grid").num_columns(2).show(ui, |ui| {
                            crate::a11y::label(ui, tr("transform.center_x"));
                            NumericField::new(&mut d.cx, Unit::Mm)
                                .id_salt("lv-cx")
                                .decimals(3)
                                .show(ui);
                            ui.end_row();
                            crate::a11y::label(ui, tr("transform.center_y"));
                            NumericField::new(&mut d.cy, Unit::Mm)
                                .id_salt("lv-cy")
                                .decimals(3)
                                .show(ui);
                            ui.end_row();
                        });
                    }
                    ui.horizontal(|ui| {
                        apply = button(ui, &tr("transform.apply"), ButtonKind::Primary).clicked();
                        cancel = button(ui, &tr("dialog.close"), ButtonKind::Secondary).clicked();
                    });
                });
        });
        if apply {
            let pivot = d.custom_pivot.then(|| PointMm::new(d.cx, d.cy));
            self.editor.level_selection(d.horizontal, pivot);
            if !self.editor.status.error {
                return;
            }
        }
        if open && !cancel {
            self.level = Some(d);
        }
    }
}
