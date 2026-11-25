//! US-09.4a — nid de tailles : toutes les tailles d'une gamme superposées sur le canevas, point
//! d'empilement commun, lues par `Project::size_nest` (calcul dérivé, jamais stocké, aucune
//! `Command`). Option de vue (Affichage › « Afficher le nid de tailles ») et fenêtre du dock
//! « Nid de tailles… » : gamme, point d'empilement, tailles affichées, légende. Lecture seule.
//!
//! Une taille n'est **jamais** identifiée par la couleur seule : chaque taille a un motif de trait
//! (plein épais pour la base, puis six motifs en rotation) **et** une étiquette textuelle posée au
//! point le plus haut de son contour ; la légende écrit le nom du motif. Au-delà de la palette
//! (10 couleurs), les couleurs se répètent mais le motif et l'étiquette restent.
//!
//! Aucun calcul métier ici : tout vient du cœur. Le nid n'est recalculé que si le projet, la
//! feuille, la gamme ou le point d'empilement change. L'édition reste celle de la taille de base.

use crate::a11y::ComboExt;
use crate::app::App;
use crate::canvas::{polyline, stroke, Xf};
use crate::dock::ToolWin;
use crate::palette::{is_dark, pal};
use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, RichText, Sense};
use pf_core::{NestAnchor, NestError, PointId, Project, SheetId, SizeId, SizeNest, SizeRangeId};
use pf_export::size_export::SizeFormat;
use pf_i18n::{tr, tr_args};
use pf_ui::components::{button, ButtonKind};
use std::collections::BTreeSet;

/// Motif de trait d'une taille.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Pattern {
    Solid,
    Dashed,
    Dotted,
    DashDot,
    LongDash,
    DashDotDot,
    Fine,
}

/// Motifs des tailles autres que la base, dans l'ordre de rotation.
const ROTATION: [Pattern; 6] = [
    Pattern::Dashed,
    Pattern::Dotted,
    Pattern::DashDot,
    Pattern::LongDash,
    Pattern::DashDotDot,
    Pattern::Fine,
];

impl Pattern {
    /// Longueurs des traits et des vides (pixels), cycliques ; vide = trait plein.
    fn dashes(self) -> (&'static [f32], &'static [f32]) {
        match self {
            Pattern::Solid => (&[], &[]),
            Pattern::Dashed => (&[8.0], &[5.0]),
            Pattern::Dotted => (&[2.4], &[4.2]),
            Pattern::DashDot => (&[12.0, 1.5], &[4.0, 4.0]),
            Pattern::LongDash => (&[18.0], &[6.0]),
            Pattern::DashDotDot => (&[12.0, 1.5, 1.5], &[4.0, 4.0, 4.0]),
            Pattern::Fine => (&[3.0], &[2.5]),
        }
    }

    /// Nom du motif, écrit dans la légende.
    pub(crate) fn name(self) -> String {
        match self {
            Pattern::Solid => tr("nest.pattern.solid"),
            Pattern::Dashed => tr("nest.pattern.dashed"),
            Pattern::Dotted => tr("nest.pattern.dotted"),
            Pattern::DashDot => tr("nest.pattern.dashdot"),
            Pattern::LongDash => tr("nest.pattern.longdash"),
            Pattern::DashDotDot => tr("nest.pattern.dashdotdot"),
            Pattern::Fine => tr("nest.pattern.fine"),
        }
    }
}

/// Couleurs des tailles sur fond clair : contraste d'au moins 3:1 avec le fond du canevas.
pub(crate) const COLORS_LIGHT: [(u8, u8, u8); 10] = [
    (0x00, 0x72, 0xB2),
    (0xC2, 0x4E, 0x00),
    (0x00, 0x7A, 0x5E),
    (0xA2, 0x3B, 0x8C),
    (0x8A, 0x6D, 0x00),
    (0x00, 0x83, 0x8F),
    (0xB3, 0x26, 0x1E),
    (0x5E, 0x4F, 0xA2),
    (0x6B, 0x4F, 0x3A),
    (0x4D, 0x7C, 0x0F),
];

/// Couleurs des tailles sur fond sombre.
pub(crate) const COLORS_DARK: [(u8, u8, u8); 10] = [
    (0x56, 0xB4, 0xE9),
    (0xFF, 0x8A, 0x4C),
    (0x34, 0xD3, 0x99),
    (0xF4, 0x72, 0xB6),
    (0xFA, 0xCC, 0x15),
    (0x22, 0xD3, 0xEE),
    (0xFB, 0x71, 0x85),
    (0xA7, 0x8B, 0xFA),
    (0xD6, 0xA7, 0x7A),
    (0xA3, 0xE6, 0x35),
];

/// Trait d'une taille.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct NestStyle {
    pub color: Color32,
    pub pattern: Pattern,
    pub width: f32,
}

impl NestStyle {
    pub(crate) fn dashes(&self) -> (&'static [f32], &'static [f32]) {
        self.pattern.dashes()
    }
}

/// Trait de la taille de rang `rank` parmi les tailles **autres que la base** (dans l'ordre de la
/// gamme) ; la base est toujours en plein, plus épais, couleur d'encre.
pub(crate) fn style_for(rank: Option<usize>, dark: bool) -> NestStyle {
    match rank {
        None => NestStyle {
            color: if dark {
                Color32::from_rgb(230, 237, 246)
            } else {
                Color32::from_rgb(17, 24, 39)
            },
            pattern: Pattern::Solid,
            width: 2.5,
        },
        Some(i) => {
            let (r, g, b) = if dark {
                COLORS_DARK[i % COLORS_DARK.len()]
            } else {
                COLORS_LIGHT[i % COLORS_LIGHT.len()]
            };
            NestStyle {
                color: Color32::from_rgb(r, g, b),
                pattern: ROTATION[i % ROTATION.len()],
                width: 1.6,
            }
        }
    }
}

/// Styles des tailles d'une gamme, dans l'ordre de ses tailles (`base` = taille de base).
pub(crate) fn styles(sizes: &[SizeId], base: SizeId, dark: bool) -> Vec<(SizeId, NestStyle)> {
    let mut rank = 0;
    sizes
        .iter()
        .map(|id| {
            if *id == base {
                (*id, style_for(None, dark))
            } else {
                rank += 1;
                (*id, style_for(Some(rank - 1), dark))
            }
        })
        .collect()
}

/// Dernier nid calculé et ce dont il vient.
#[derive(Debug, Clone)]
struct Cache {
    project: Project,
    sheet: SheetId,
    range: SizeRangeId,
    anchor: NestAnchor,
    result: Result<SizeNest, NestError>,
}

/// Réglages du nid sur le canevas (interface seulement, jamais enregistré).
#[derive(Debug, Clone, Default)]
pub struct NestView {
    /// Le nid est dessiné sur le canevas.
    pub enabled: bool,
    /// Gamme affichée ; `None` = la première du projet.
    pub range: Option<SizeRangeId>,
    /// Point d'empilement ; `None` = origine de la feuille.
    pub anchor: Option<PointId>,
    /// Tailles masquées.
    pub hidden: BTreeSet<SizeId>,
    cache: Option<Cache>,
}

impl NestView {
    /// Nid calculé pour la feuille active, s'il est affiché et lisible.
    pub(crate) fn nest(&self) -> Option<&SizeNest> {
        self.cache.as_ref().and_then(|c| c.result.as_ref().ok())
    }

    fn error(&self) -> Option<&NestError> {
        self.cache.as_ref().and_then(|c| c.result.as_ref().err())
    }
}

/// Fenêtre « Nid de tailles » : les réglages vivent dans [`NestView`] (ils survivent à sa
/// fermeture, le nid reste affiché).
#[derive(Debug, Clone, Default)]
pub struct SizeNestDialog;

impl App {
    /// Menu Affichage › « Nid de tailles… » (sans raccourci).
    pub fn open_size_nest(&mut self) {
        if self.size_nest.is_none() {
            self.size_nest = Some(SizeNestDialog);
        }
    }

    /// Recalcule le nid seulement quand il est utile (affiché ou fenêtre ouverte) et que le
    /// projet, la feuille, la gamme ou le point d'empilement a changé.
    pub fn refresh_nest(&mut self) {
        if !self.nest.enabled && self.size_nest.is_none() {
            self.nest.cache = None;
            return;
        }
        let project = self.editor.project();
        let ranges: Vec<SizeRangeId> = project.sizes.iter().map(|r| r.id).collect();
        let range = self
            .nest
            .range
            .filter(|id| ranges.contains(id))
            .or(ranges.first().copied());
        self.nest.range = range;
        let sheet = self.editor.sheet;
        // Un point supprimé (ou d'une autre feuille) : retour à l'origine de la feuille.
        if let Some(id) = self.nest.anchor {
            if project.sheet(sheet).map_or(true, |s| s.point(id).is_err()) {
                self.nest.anchor = None;
            }
        }
        let anchor = self
            .nest
            .anchor
            .map_or(NestAnchor::Origin, NestAnchor::Point);
        let Some(range) = range else {
            self.nest.cache = None;
            return;
        };
        let stale = self.nest.cache.as_ref().is_none_or(|c| {
            c.sheet != sheet || c.range != range || c.anchor != anchor || &c.project != project
        });
        if stale {
            let result = project.size_nest(sheet, range, anchor);
            self.nest.cache = Some(Cache {
                project: project.clone(),
                sheet,
                range,
                anchor,
                result,
            });
        }
    }

    /// Emprise du nid affiché (pour cadrer la vue), en mm.
    pub(crate) fn nest_bbox(&self) -> Option<pf_geometry::Bbox> {
        if !self.nest.enabled {
            return None;
        }
        let nest = self.nest.nest()?;
        let mut out: Option<pf_geometry::Bbox> = None;
        for s in nest
            .sizes
            .iter()
            .filter(|s| !self.nest.hidden.contains(&s.size))
        {
            for p in s.outlines.iter().flat_map(|o| o.points.iter().copied()) {
                match out.as_mut() {
                    Some(b) => b.include(p),
                    None => out = Some(pf_geometry::Bbox::from_point(p)),
                }
            }
        }
        out
    }

    /// Dessine le nid sous les tracés de la feuille : un trait par taille (couleur + motif) et
    /// l'étiquette de la taille au point le plus haut de son contour ; le point d'empilement est
    /// marqué d'une croix cerclée.
    pub(crate) fn paint_nest(&self, painter: &egui::Painter, xf: &Xf) {
        if !self.nest.enabled {
            return;
        }
        let Some(nest) = self.nest.nest() else {
            return;
        };
        if nest.sheet != self.editor.sheet {
            return;
        }
        let ids: Vec<SizeId> = nest.sizes.iter().map(|s| s.size).collect();
        let styles = styles(&ids, nest.base, is_dark());
        let mut tags: Vec<(Pos2, String, Color32)> = Vec::new();
        for (ns, (_, style)) in nest.sizes.iter().zip(&styles) {
            if self.nest.hidden.contains(&ns.size) {
                continue;
            }
            let (dash, gap) = style.dashes();
            for o in &ns.outlines {
                let mut pts: Vec<Pos2> = o.points.iter().map(|p| xf.s(*p)).collect();
                if o.closed {
                    if let Some(first) = pts.first().copied() {
                        pts.push(first);
                    }
                }
                if pts.len() >= 2 {
                    polyline(painter, pts, style.color, style.width, dash, gap);
                }
            }
            if let Some(top) = ns.top_point() {
                let name = self.size_label(ns.size);
                let text = if ns.is_base {
                    tr_args("nest.tag.base", &[("label", &name)])
                } else {
                    name
                };
                tags.push((xf.s(top), text, style.color));
            }
        }
        place_tags(painter, tags);
        // Point d'empilement : croix dans un cercle (forme, pas seulement la couleur).
        let at = xf.s(nest.anchor_position);
        let ink = pal().ink;
        painter.circle_stroke(at, 7.0, stroke(1.5, ink));
        painter.line_segment(
            [at + egui::vec2(-10.0, 0.0), at + egui::vec2(10.0, 0.0)],
            stroke(1.5, ink),
        );
        painter.line_segment(
            [at + egui::vec2(0.0, -10.0), at + egui::vec2(0.0, 10.0)],
            stroke(1.5, ink),
        );
        let text = tr("nest.anchor.canvas");
        let width = painter
            .layout_no_wrap(text.clone(), FontId::proportional(13.0), ink)
            .size()
            .x;
        let (pos, side) = if at.x + 10.0 + width + 8.0 > painter.clip_rect().right() {
            (at + egui::vec2(-10.0, 10.0), Align2::RIGHT_TOP)
        } else {
            (at + egui::vec2(10.0, 10.0), Align2::LEFT_TOP)
        };
        crate::canvas::label_now(painter, pos, side, &text, ink);
    }

    fn size_label(&self, size: SizeId) -> String {
        self.editor
            .project()
            .find_size(size)
            .map(|(r, i)| r.sizes[i].label.clone())
            .unwrap_or_default()
    }

    pub fn size_nest_window(&mut self, ctx: &egui::Context) {
        if self.size_nest.is_none() {
            return;
        }
        let secondary = pf_ui::theme::tokens(ctx).text_secondary;
        let project = self.editor.project();
        let sheet_id = self.editor.sheet;
        let ranges: Vec<(SizeRangeId, String)> = project
            .sizes
            .iter()
            .map(|r| (r.id, r.name.clone()))
            .collect();
        let range = self.nest.range;
        // Tailles de la gamme choisie : (id, libellé), taille de base.
        let (sizes, base): (Vec<(SizeId, String)>, Option<SizeId>) = range
            .and_then(|id| project.size_range(id).ok())
            .map(|r| {
                (
                    r.sizes.iter().map(|s| (s.id, s.label.clone())).collect(),
                    Some(r.base),
                )
            })
            .unwrap_or_default();
        let points: Vec<(PointId, String)> = project
            .sheet(sheet_id)
            .map(|s| {
                s.points
                    .iter()
                    .map(|p| (p.id, s.label(p.id).to_string()))
                    .collect()
            })
            .unwrap_or_default();
        let styles = base
            .map(|b| {
                let ids: Vec<SizeId> = sizes.iter().map(|s| s.0).collect();
                styles(&ids, b, is_dark())
            })
            .unwrap_or_default();
        let issues: Vec<String> = self
            .nest
            .nest()
            .map(|n| {
                n.sizes
                    .iter()
                    .flat_map(|s| {
                        let label = sizes
                            .iter()
                            .find(|(id, _)| *id == s.size)
                            .map(|(_, l)| l.clone())
                            .unwrap_or_default();
                        s.issues.iter().map(move |m| {
                            tr_args("nest.issue", &[("size", &label), ("message", m)])
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        let error = self.nest.error().map(|e| match e {
            NestError::AnchorUnreadable { size, reason } => tr_args(
                "nest.error.anchor",
                &[
                    (
                        "size",
                        &sizes
                            .iter()
                            .find(|(id, _)| id == size)
                            .map(|(_, l)| l.clone())
                            .unwrap_or_default(),
                    ),
                    ("reason", reason),
                ],
            ),
            other => other.to_string(),
        });
        let anchor_text = match self.nest.anchor {
            Some(id) => points
                .iter()
                .find(|(p, _)| *p == id)
                .map(|(_, l)| l.clone())
                .unwrap_or_default(),
            None => tr("nest.anchor.origin"),
        };
        let mut enabled = self.nest.enabled;
        let mut hidden = self.nest.hidden.clone();
        let mut chosen_range: Option<SizeRangeId> = None;
        let mut chosen_anchor: Option<Option<PointId>> = None;
        let mut open = true;
        let mut close = false;
        let mut export_req: Option<SizeFormat> = None;
        let slot = crate::dock::slot(ctx, &self.dock, ToolWin::SizeNest);
        let width = self.dock.content_width(ToolWin::SizeNest);
        egui::Window::new(tr("nest.title"))
            .id(ToolWin::SizeNest.id())
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
                        crate::a11y::label(ui, RichText::new(tr("nest.intro")).color(secondary));
                        crate::a11y::label(ui, RichText::new(tr("nest.readonly")).strong());
                        ui.add_space(4.0);
                        let Some(range) = range else {
                            crate::a11y::label(
                                ui,
                                RichText::new(tr("sizemeas.no_sizes")).color(secondary),
                            );
                            ui.add_space(6.0);
                            close =
                                button(ui, &tr("dialog.close"), ButtonKind::Secondary).clicked();
                            return;
                        };
                        ui.checkbox(&mut enabled, tr("nest.show"));
                        if ranges.len() > 1 {
                            ui.horizontal_wrapped(|ui| {
                                crate::a11y::label(ui, tr("nest.range"));
                                for (id, name) in &ranges {
                                    if ui.selectable_label(*id == range, name).clicked() {
                                        chosen_range = Some(*id);
                                    }
                                }
                            });
                        }
                        ui.horizontal(|ui| {
                            crate::a11y::label(ui, tr("nest.anchor"));
                            egui::ComboBox::from_id_salt("nest_anchor")
                                .selected_text(anchor_text.clone())
                                .show_ui_named(ui, |ui| {
                                    if ui
                                        .selectable_label(
                                            self.nest.anchor.is_none(),
                                            tr("nest.anchor.origin"),
                                        )
                                        .clicked()
                                    {
                                        chosen_anchor = Some(None);
                                    }
                                    for (id, label) in &points {
                                        if ui
                                            .selectable_label(self.nest.anchor == Some(*id), label)
                                            .clicked()
                                        {
                                            chosen_anchor = Some(Some(*id));
                                        }
                                    }
                                });
                        });
                        if let Some(e) = &error {
                            crate::a11y::label(ui, RichText::new(e).color(pal().error).strong());
                        }
                        ui.add_space(6.0);
                        crate::a11y::label(ui, RichText::new(tr("nest.legend")).strong());
                        ui.horizontal(|ui| {
                            if button(ui, &tr("nest.show_all"), ButtonKind::Secondary).clicked() {
                                hidden.clear();
                            }
                            if button(ui, &tr("nest.hide_all"), ButtonKind::Secondary).clicked() {
                                hidden = sizes.iter().map(|s| s.0).collect();
                            }
                        });
                        for ((id, label), (_, style)) in sizes.iter().zip(&styles) {
                            ui.horizontal(|ui| {
                                let mut shown = !hidden.contains(id);
                                if ui.checkbox(&mut shown, label).changed() {
                                    if shown {
                                        hidden.remove(id);
                                    } else {
                                        hidden.insert(*id);
                                    }
                                }
                                legend_sample(ui, style);
                                let mut text = style.pattern.name();
                                if Some(*id) == base {
                                    text = format!("{text} · {}", tr("nest.base_mark"));
                                }
                                crate::a11y::label(ui, RichText::new(text).color(secondary));
                            });
                        }
                        for m in &issues {
                            crate::a11y::label(ui, RichText::new(m).color(pal().error));
                        }
                        // US-09.8 : export du nid (tailles cochées, point d'empilement choisi).
                        ui.add_space(6.0);
                        crate::a11y::label(ui, RichText::new(tr("nest.export.title")).strong());
                        crate::a11y::label(
                            ui,
                            RichText::new(tr("nest.export.hint")).color(secondary),
                        );
                        ui.horizontal(|ui| {
                            if button(ui, &tr("nest.export.svg"), ButtonKind::Secondary).clicked() {
                                export_req = Some(SizeFormat::Svg);
                            }
                            if button(ui, &tr("nest.export.pdf"), ButtonKind::Secondary).clicked() {
                                export_req = Some(SizeFormat::Pdf);
                            }
                        });
                        ui.add_space(6.0);
                        crate::a11y::label(ui, RichText::new(tr("nest.note")).color(secondary));
                        ui.add_space(6.0);
                        close = button(ui, &tr("dialog.close"), ButtonKind::Secondary).clicked();
                    });
            });
        self.nest.enabled = enabled;
        self.nest.hidden = hidden;
        if let Some(id) = chosen_range {
            self.nest.range = Some(id);
            self.nest.hidden.clear();
        }
        if let Some(a) = chosen_anchor {
            self.nest.anchor = a;
        }
        if let Some(format) = export_req {
            self.export_nest_dialog(format);
        }
        if !open || close {
            self.size_nest = None;
        }
    }
}

/// Échantillon de trait de la légende : le motif réel, dessiné.
fn legend_sample(ui: &mut egui::Ui, style: &NestStyle) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(64.0, 16.0), Sense::hover());
    let y = rect.center().y;
    let (dash, gap) = style.dashes();
    polyline(
        ui.painter(),
        vec![
            Pos2::new(rect.left() + 2.0, y),
            Pos2::new(rect.right() - 2.0, y),
        ],
        style.color,
        style.width,
        dash,
        gap,
    );
}

/// Emplacements des étiquettes de taille : une **colonne** à droite du plus à droite des points
/// d'attache, de haut en bas dans l'ordre des points, chaque étiquette aussi près que possible
/// au-dessus de son point sans recouvrir la précédente, le tout dans `clip`. Fonction pure.
/// `tags` : point d'attache (pixels) et taille de l'étiquette ; même ordre en sortie.
fn column_layout(tags: &[(Pos2, egui::Vec2)], clip: Rect) -> Vec<Rect> {
    let mut order: Vec<usize> = (0..tags.len()).collect();
    order.sort_by(|a, b| {
        tags[*a]
            .0
            .y
            .total_cmp(&tags[*b].0.y)
            .then(tags[*a].0.x.total_cmp(&tags[*b].0.x))
    });
    let column = tags.iter().map(|t| t.0.x).fold(f32::MIN, f32::max) + 16.0;
    let mut out = vec![Rect::NOTHING; tags.len()];
    let mut bottom = clip.top() + 2.0;
    for i in order {
        let (at, size) = tags[i];
        let left = column
            .min(clip.right() - size.x - 4.0)
            .max(clip.left() + 2.0);
        let top = (at.y - size.y - 4.0).max(bottom + 2.0);
        out[i] = Rect::from_min_size(Pos2::new(left, top), size);
        bottom = top + size.y;
    }
    out
}

/// Pose les étiquettes de taille (toutes, toujours) : cadre à la couleur de la taille, trait de
/// rappel vers le point d'attache quand l'étiquette en est éloignée.
fn place_tags(painter: &egui::Painter, tags: Vec<(Pos2, String, Color32)>) {
    let galleys: Vec<_> = tags
        .iter()
        .map(|(_, text, color)| {
            painter.layout_no_wrap(text.clone(), FontId::proportional(12.0), *color)
        })
        .collect();
    let layout: Vec<(Pos2, egui::Vec2)> = tags
        .iter()
        .zip(&galleys)
        .map(|((at, _, _), g)| (*at, g.size() + egui::vec2(6.0, 4.0)))
        .collect();
    let rects = column_layout(&layout, painter.clip_rect());
    for (((at, _, color), galley), rect) in tags.iter().zip(galleys).zip(rects) {
        if rect.distance_to_pos(*at) > 6.0 {
            painter.line_segment([*at, rect.left_center()], stroke(1.0, *color));
        }
        painter.rect_filled(rect, 3.0, pal().label_bg);
        painter.rect_stroke(rect, 3.0, stroke(1.0, *color), egui::StrokeKind::Inside);
        painter.galley(rect.min + egui::vec2(3.0, 2.0), galley, *color);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pf_ui::tokens::contrast_ratio;
    use pf_ui::Tokens;

    #[test]
    fn palette_contrasted_on_both_canvas_backgrounds() {
        for (colors, bg) in [
            (COLORS_LIGHT, Tokens::precision_light().surface_canvas),
            (COLORS_DARK, Tokens::precision_dark().surface_canvas),
        ] {
            for (r, g, b) in colors {
                let c = Color32::from_rgb(r, g, b);
                let ratio = contrast_ratio(c, bg);
                assert!(ratio >= 3.0, "{c:?} sur {bg:?} : contraste {ratio:.2}");
            }
        }
        // Le trait de la base (encre) contraste aussi fortement.
        for (dark, bg) in [
            (false, Tokens::precision_light().surface_canvas),
            (true, Tokens::precision_dark().surface_canvas),
        ] {
            assert!(contrast_ratio(style_for(None, dark).color, bg) >= 7.0);
        }
    }

    #[test]
    fn colors_are_pairwise_distinct() {
        for colors in [COLORS_LIGHT, COLORS_DARK] {
            let set: BTreeSet<_> = colors.iter().collect();
            assert_eq!(set.len(), colors.len());
        }
    }

    #[test]
    fn base_is_solid_and_thicker_and_others_never_solid() {
        let b = style_for(None, false);
        assert_eq!(b.pattern, Pattern::Solid);
        for i in 0..40 {
            let s = style_for(Some(i), false);
            assert_ne!(s.pattern, Pattern::Solid);
            assert!(s.width < b.width);
        }
    }

    #[test]
    fn twenty_sizes_all_distinguished_by_color_and_pattern_pair() {
        let ids: Vec<SizeId> = (0..20).map(SizeId).collect();
        for dark in [false, true] {
            let st = styles(&ids, SizeId(7), dark);
            assert_eq!(st.len(), 20);
            let pairs: BTreeSet<(u8, u8, u8, u8)> = st
                .iter()
                .map(|(_, s)| {
                    (
                        s.color.r(),
                        s.color.g(),
                        s.color.b(),
                        ROTATION
                            .iter()
                            .position(|p| *p == s.pattern)
                            .map_or(99, |i| i as u8),
                    )
                })
                .collect();
            // Couleur + motif : vingt tailles, vingt couples distincts (10 couleurs × 6 motifs).
            assert_eq!(pairs.len(), 20);
            // Le motif seul alterne (six motifs) : l'étiquette textuelle porte le reste.
            let patterns: BTreeSet<String> =
                st.iter().map(|(_, s)| format!("{:?}", s.pattern)).collect();
            assert_eq!(patterns.len(), 7);
        }
    }

    #[test]
    fn labels_stay_inside_the_canvas_and_never_overlap_with_twenty_sizes() {
        let clip = Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(600.0, 800.0));
        // Vingt points d'attache presque confondus, tout en haut de la zone.
        let tags: Vec<(Pos2, egui::Vec2)> = (0..20)
            .map(|i| {
                (
                    Pos2::new(300.0 + i as f32, 3.0 + 0.5 * i as f32),
                    egui::vec2(40.0, 18.0),
                )
            })
            .collect();
        let rects = column_layout(&tags, clip);
        assert_eq!(rects.len(), 20);
        for (i, r) in rects.iter().enumerate() {
            assert!(clip.contains_rect(*r), "{i} : {r:?}");
            for (j, o) in rects.iter().enumerate().filter(|(j, _)| *j > i) {
                assert!(!r.intersects(*o), "{i} recouvre {j}");
            }
        }
        // Un point d'attache en bas à droite : l'étiquette reste dans la zone.
        let far = [(Pos2::new(599.0, 799.0), egui::vec2(60.0, 18.0))];
        assert!(clip.contains_rect(column_layout(&far, clip)[0]));
    }
}
