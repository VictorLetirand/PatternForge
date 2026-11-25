//! Canevas : dessin de la feuille et traduction souris → éditeur.
//!
//! Aucun calcul métier ici : les positions viennent du modèle (mm), passent
//! par [`View`] pour l'affichage, et les mesures affichées viennent de
//! `pf-core` / `pf-geometry`.

use crate::app::App;
use crate::editor::{Preview, SnapKind, Target, Tool, ToolState};
use crate::labels;
use crate::palette::pal;
use crate::prefs::Role;
use crate::view::View;
use eframe::egui::{
    self, Align2, Color32, FontId, PointerButton, Pos2, Rect, Sense, Shape, Stroke, StrokeKind,
};
use pf_core::input::{format_deg, format_mm, format_number};
use pf_core::{measure, EntityKind, Guide, GuideId, PathRole, PointRole};
use pf_geometry::{Bbox, PointMm};
use pf_i18n::{tr, tr_args};

/// Rayon de visée autour du curseur, en pixels.
pub(crate) const PICK_RADIUS_PX: f64 = 8.0;
/// Écart minimal entre deux sommets du lasso (pixels) et nombre maximal de sommets.
const LASSO_STEP_PX: f64 = 3.0;
const LASSO_MAX_POINTS: usize = 5000;
/// Épaisseur des règles graduées (pixels) : les étiquettes ne s'y posent pas.
const RULER_BAND: f32 = 18.0;

/// Apparence écran d'un rôle : couleur **et** motif de trait (longueurs de traits puis
/// d'intervalles, en pixels). Les lignes spéciales (US-06.8) réutilisent la couleur d'un rôle
/// voisin : seul le motif et l'épaisseur les distinguent (tous différents, voir le test).
pub(crate) fn role_style(
    role: PathRole,
    t: &pf_ui::tokens::Tokens,
) -> (Color32, f32, &'static [f32], &'static [f32]) {
    match role {
        PathRole::Construction => base_role(Role::Construction, t.role_construction),
        PathRole::SewingContour => base_role(Role::Sewing, t.role_sewing),
        PathRole::CuttingContour => base_role(Role::Cutting, t.role_cutting),
        PathRole::Grainline => base_role(Role::Grainline, t.role_grainline),
        PathRole::Fold => base_role(Role::Fold, t.role_fold),
        PathRole::AnnotationAxis => base_role(Role::Axis, t.role_axis),
        PathRole::Topstitch => (t.role_sewing, 1.8, &[3.0], &[2.5]),
        PathRole::CenterFront => (t.role_fold, 2.4, &[20.0, 6.0], &[5.0, 5.0]),
        PathRole::CenterBack => (t.role_fold, 2.4, &[20.0, 6.0, 6.0], &[5.0, 5.0, 5.0]),
        PathRole::WaistLine => (t.role_axis, 1.2, &[14.0], &[5.0]),
        PathRole::HipLine => (t.role_axis, 1.2, &[6.0], &[4.0]),
        PathRole::BustLine => (t.role_axis, 1.2, &[14.0, 3.0], &[4.0, 4.0]),
        PathRole::SkivingLine => (t.role_grainline, 2.2, &[8.0, 2.0, 2.0], &[4.0, 4.0, 4.0]),
        PathRole::GlueLine => (t.role_grainline, 3.0, &[2.0], &[3.5]),
    }
}

/// Couleur du jeton et motif choisi (préférences, US-16.7) d'un des six rôles de base.
fn base_role(role: Role, color: Color32) -> (Color32, f32, &'static [f32], &'static [f32]) {
    let (width, dash, gap) = crate::prefs::role_pattern(role).style();
    (color, width, dash, gap)
}

/// Trait d'épaisseur donnée en pixels (type explicite : pas d'inférence de littéral).
pub(crate) fn stroke(width: f32, color: Color32) -> Stroke {
    Stroke::new(width, color)
}

pub(crate) struct Xf {
    pub(crate) view: View,
    pub(crate) center: (f64, f64),
}

impl Xf {
    pub(crate) fn s(&self, p: PointMm) -> Pos2 {
        let (x, y) = self.view.to_screen(p, self.center);
        Pos2::new(x as f32, y as f32)
    }
    pub(crate) fn w(&self, p: Pos2) -> PointMm {
        self.view
            .to_world((f64::from(p.x), f64::from(p.y)), self.center)
    }
}

pub(crate) fn polyline(
    painter: &egui::Painter,
    pts: Vec<Pos2>,
    color: Color32,
    width: f32,
    dash: &[f32],
    gap: &[f32],
) {
    let stroke = stroke(width, color);
    if dash.is_empty() {
        painter.add(Shape::line(pts, stroke));
    } else {
        painter.extend(Shape::dashed_line_with_offset(&pts, stroke, dash, gap, 0.0));
    }
}

/// Partie d'une ligne d'aide qui traverse la zone visible, en pixels.
fn guide_segment(painter: &egui::Painter, xf: &Xf, g: &Guide) -> Option<(Pos2, Pos2)> {
    let clip = painter.clip_rect();
    let mut visible = Bbox::from_point(xf.w(clip.left_bottom()));
    visible.include(xf.w(clip.right_top()));
    g.clip_to(&visible).map(|(a, b)| (xf.s(a), xf.s(b)))
}

/// Libellé de tracé, de raccord ou de couture : posé par l'agencement des étiquettes (PH-2c,
/// `labels.rs`) pendant le dessin du canevas, tout de suite ailleurs.
pub(crate) fn label(
    painter: &egui::Painter,
    pos: Pos2,
    anchor: Align2,
    text: &str,
    color: Color32,
) {
    label_kind(painter, pos, anchor, text, color, labels::Kind::Path);
}

/// Comme [`label`], avec la nature de l'étiquette (sa priorité dans l'agencement).
pub(crate) fn label_kind(
    painter: &egui::Painter,
    pos: Pos2,
    anchor: Align2,
    text: &str,
    color: Color32,
    kind: labels::Kind,
) {
    if !labels::boxed(painter, pos, anchor, text, color, kind) {
        label_now(painter, pos, anchor, text, color);
    }
}

/// Étiquette dessinée sur place, jamais déplacée : aperçus d'outil et bulles qui suivent le
/// curseur (ils ne participent pas à l'agencement, sinon les autres bougeraient à chaque image).
pub(crate) fn label_now(
    painter: &egui::Painter,
    pos: Pos2,
    anchor: Align2,
    text: &str,
    color: Color32,
) {
    let font = FontId::proportional(13.0);
    let galley = painter.layout_no_wrap(text.to_string(), font, color);
    let rect = anchor.anchor_size(pos, galley.size()).expand(3.0);
    painter.rect_filled(rect, 3.0, pal().label_bg);
    painter.galley(rect.min + egui::vec2(3.0, 3.0), galley, color);
}

impl App {
    pub fn canvas_panel(&mut self, ctx: &egui::Context) {
        if self.plan.is_some() {
            self.plan_canvas(ctx);
            return;
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(pf_ui::theme::tokens(ctx).surface_canvas))
            .show(ctx, |ui| {
                let (response, painter) =
                    ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
                let rect = response.rect;
                self.canvas_rect = rect;
                let center = (f64::from(rect.center().x), f64::from(rect.center().y));
                if self.fit_pending {
                    let size = (f64::from(rect.width()), f64::from(rect.height()));
                    self.view.fit(self.fit_bbox(), size);
                    self.fit_pending = false;
                }
                if let Some(bb) = self.zoom_target.take() {
                    let size = (f64::from(rect.width()), f64::from(rect.height()));
                    self.view.fit(Some(bb), size);
                }
                self.canvas_input(ctx, &response, center);
                let xf = Xf {
                    view: self.view,
                    center,
                };
                self.paint_grid(&painter, rect, &xf);
                self.paint_background(ctx, &painter, &xf);
                // Étiquettes : collectées pendant le dessin, posées d'un coup (PH-2c).
                labels::begin(&painter);
                self.paint_sheet(&painter, &xf);
                self.paint_overlays(ctx, &painter, &xf);
                self.paint_trace(&painter, &xf);
                let mut free = rect;
                if self.editor.show_rulers {
                    free.min += egui::vec2(RULER_BAND, RULER_BAND);
                }
                let tip = labels::flush(&painter, free, response.hover_pos());
                if let Some(text) = tip {
                    response.clone().on_hover_text_at_pointer(text);
                }
                let skipped = labels::skipped(ctx);
                if skipped > 0 {
                    painter.text(
                        rect.left_bottom() + egui::vec2(10.0, -24.0),
                        Align2::LEFT_BOTTOM,
                        tr_args(
                            "canvas.labels.limit",
                            &[
                                ("count", &skipped.to_string()),
                                ("max", &labels::MAX_LABELS.to_string()),
                            ],
                        ),
                        FontId::proportional(11.0),
                        pal().grid_text,
                    );
                }
                if self.editor.show_rulers {
                    self.paint_rulers(&painter, rect, &xf);
                }
                // Plan en lecture seule : une taille autre que la base est affichée (US-09.2a).
                if let Some(size) = self.editor.size_view() {
                    let top = if self.editor.show_rulers {
                        RULER_BAND + 8.0
                    } else {
                        8.0
                    };
                    let galley = painter.layout_no_wrap(
                        tr_args("size.banner", &[("name", &size)]),
                        FontId::proportional(13.0),
                        pal().ink,
                    );
                    let at = rect.left_top() + egui::vec2(RULER_BAND + 8.0, top);
                    let bg = egui::Rect::from_min_size(at, galley.size()).expand(5.0);
                    painter.rect_filled(bg, 4.0, pal().label_bg);
                    painter.rect_stroke(
                        bg,
                        4.0,
                        stroke(1.0, pal().select),
                        egui::StrokeKind::Inside,
                    );
                    painter.galley(at, galley, pal().ink);
                }
                self.palette(ctx, rect);
                self.entry_bubble(ctx, rect);
                self.dart_window(ctx, rect);
            });
    }

    fn canvas_input(&mut self, ctx: &egui::Context, response: &egui::Response, center: (f64, f64)) {
        let xf = Xf {
            view: self.view,
            center,
        };
        let tol_mm = self.prefs.pick_radius_px / self.view.zoom;
        self.editor.pick_tol_mm = tol_mm;
        let (shift, alt, space) = ctx.input(|i| {
            (
                i.modifiers.shift,
                i.modifiers.alt,
                i.key_down(egui::Key::Space),
            )
        });

        // Maj : verrouillage de l'angle des tracés obliques (US-16.2).
        self.editor.angle_lock = shift;
        self.canvas_hovered = response.hover_pos().is_some();

        if let Some(pos) = response.hover_pos() {
            self.editor.pointer_moved(xf.w(pos));
            // Zoom à la molette (ou pincement), centré sur le curseur.
            let (scroll, pinch) = ctx.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
            let factor = f64::from(pinch) * (f64::from(scroll) * 0.0025).exp();
            if (factor - 1.0).abs() > 1e-6 {
                self.view
                    .zoom_about((f64::from(pos.x), f64::from(pos.y)), center, factor);
            }
        }

        // Déplacement de la vue : bouton du milieu, ou Espace + glisser.
        let panning = response.dragged_by(PointerButton::Middle)
            || (space && response.dragged_by(PointerButton::Primary));
        if panning {
            let d = response.drag_delta();
            self.view.pan_pixels(f64::from(d.x), f64::from(d.y));
            return;
        }

        if response.drag_started_by(PointerButton::Primary) && self.editor.tool == Tool::Select {
            let origin = ctx.input(|i| i.pointer.press_origin()).map(|p| xf.w(p));
            if let Some(origin) = origin {
                let grabbed = match self.editor.hit_test(origin, tol_mm) {
                    Some(t @ (Target::Point(_) | Target::Handle(..))) => self.editor.begin_drag(t),
                    // Une ligne d'aide infinie ne se saisit qu'une fois sélectionnée
                    // (sinon elle volerait le début de toute fenêtre de sélection).
                    Some(t @ Target::Guide(g)) if self.editor.selection.guides.contains(&g) => {
                        self.editor.begin_drag(t)
                    }
                    _ => false,
                };
                if !grabbed {
                    if self.editor.lasso_mode {
                        self.lasso = Some(vec![origin]);
                    } else {
                        self.window_start = Some(origin);
                    }
                }
            }
        }
        if response.dragged_by(PointerButton::Primary) && self.editor.is_dragging() {
            if let Some(pos) = response.interact_pointer_pos() {
                self.editor.drag_to(xf.w(pos));
            }
        }
        // Lasso : un sommet tous les LASSO_STEP_PX pixels, plafonné.
        if response.dragged_by(PointerButton::Primary) {
            if let (Some(path), Some(pos)) = (self.lasso.as_mut(), response.interact_pointer_pos())
            {
                let here = xf.w(pos);
                let far_enough = path.last().is_none_or(|l| {
                    pf_geometry::distance_mm(*l, here) * self.view.zoom >= LASSO_STEP_PX
                });
                if far_enough && path.len() < LASSO_MAX_POINTS {
                    path.push(here);
                }
            }
        }
        if response.drag_stopped_by(PointerButton::Primary) {
            self.editor.end_drag();
            if let Some(start) = self.window_start.take() {
                self.editor.select_window(start, self.editor.cursor, shift);
            }
            if let Some(path) = self.lasso.take() {
                // Alt : mode « touche » (objets croisés par le lasso).
                self.editor.select_lasso(&path, shift, alt);
            }
        }
        if response.clicked_by(PointerButton::Primary) {
            if let Some(pos) = response.interact_pointer_pos() {
                self.editor.click_with(xf.w(pos), tol_mm, shift, alt);
            }
        }
        // Double-clic (après le clic, qui a déjà ciblé le tracé) : toute sa chaîne.
        if response.double_clicked_by(PointerButton::Primary) && self.editor.tool == Tool::Select {
            if let Some(pos) = response.interact_pointer_pos() {
                let _ = self.editor.select_chain_at(xf.w(pos), tol_mm, shift);
            }
        }
        if response.clicked_by(PointerButton::Secondary) {
            self.window_start = None;
            self.lasso = None;
            self.editor.escape();
        }
    }

    pub(crate) fn paint_grid(&self, painter: &egui::Painter, rect: Rect, xf: &Xf) {
        let grid = self.view_settings.grid;
        if !grid.visible {
            return;
        }
        let (min, max) = (xf.w(rect.left_bottom()), xf.w(rect.right_top()));
        // Pas fin = repère / subdivisions (10 mm par défaut) ; trop serré à l'écran, il s'élargit.
        let (step, major) = grid.levels(self.view.zoom);
        let is_major = |v: f64| (v / major - (v / major).round()).abs() < 1e-6;
        let mut x = (min.x / step).floor() * step;
        while x <= max.x {
            let sx = xf.s(PointMm::new(x, 0.0)).x;
            let color = if x.abs() < 1e-9 {
                pal().axis
            } else if is_major(x) {
                pal().grid_major
            } else {
                pal().grid_minor
            };
            painter.line_segment(
                [Pos2::new(sx, rect.top()), Pos2::new(sx, rect.bottom())],
                stroke(1.0, color),
            );
            x += step;
        }
        let mut y = (min.y / step).floor() * step;
        while y <= max.y {
            let sy = xf.s(PointMm::new(0.0, y)).y;
            let color = if y.abs() < 1e-9 {
                pal().axis
            } else if is_major(y) {
                pal().grid_major
            } else {
                pal().grid_minor
            };
            painter.line_segment(
                [Pos2::new(rect.left(), sy), Pos2::new(rect.right(), sy)],
                stroke(1.0, color),
            );
            y += step;
        }
        painter.text(
            rect.left_bottom() + egui::vec2(10.0, -8.0),
            Align2::LEFT_BOTTOM,
            tr_args(
                "canvas.grid",
                &[
                    ("step", &pf_core::input::format_mm(step)),
                    ("major", &pf_core::input::format_mm(major)),
                ],
            ),
            FontId::proportional(11.0),
            pal().grid_text,
        );
    }

    fn paint_sheet(&self, painter: &egui::Painter, xf: &Xf) {
        let tokens = pf_ui::theme::tokens(painter.ctx());
        let sheet = self.editor.sheet();
        let sel = &self.editor.selection;

        // Lignes d'aide : trait fin pointillé (point et tiret pour un axe de
        // symétrie), sous les tracés, sur toute la zone visible.
        if self.editor.show_guides {
            for g in sheet.guides.iter().filter(|g| sheet.guide_visible(g)) {
                let Some((p, q)) = guide_segment(painter, xf, g) else {
                    continue;
                };
                if sel.guides.contains(&g.id) {
                    painter.line_segment([p, q], stroke(6.0, pal().select.gamma_multiply(0.45)));
                }
                let (dash, gap): (&[f32], &[f32]) = if g.axis {
                    (&[14.0, 2.0], &[4.0, 4.0])
                } else {
                    (&[2.0], &[5.0])
                };
                polyline(painter, vec![p, q], pal().guide, 1.0, dash, gap);
            }
        }

        // Nid de tailles (US-09.4a) : sous les tracés, jamais sélectionnable ni modifiable.
        self.paint_nest(painter, xf);

        // Droit-fil avancé (US-06.5) : la flèche calculée remplace le tracé dessiné.
        let grain_lines = self.editor.view_project().sheet_grain_lines(sheet.id);
        for e in sheet
            .entities
            .iter()
            .filter(|e| sheet.layer_visible(e.layer))
        {
            let grain = grain_lines.iter().find(|g| g.entity == e.id);
            let pts: Vec<Pos2> = match &e.kind {
                EntityKind::Segment { .. } => match sheet.segment_ends(e.id) {
                    Ok((a, b)) => {
                        if let Some(g) = grain {
                            // Le tracé porteur reste visible, en filigrane (c'est lui qui se
                            // sélectionne) ; la flèche calculée est dessinée par-dessus.
                            polyline(
                                painter,
                                vec![xf.s(a), xf.s(b)],
                                pal().muted,
                                1.0,
                                &[2.0],
                                &[3.0],
                            );
                            vec![xf.s(g.from), xf.s(g.to)]
                        } else {
                            vec![xf.s(a), xf.s(b)]
                        }
                    }
                    Err(_) => continue,
                },
                EntityKind::Curve { .. } => match sheet.curve_geometry(e.id) {
                    Ok(c) => c.sample(96).into_iter().map(|p| xf.s(p)).collect(),
                    Err(_) => continue,
                },
                EntityKind::Arc { .. } | EntityKind::Circle { .. } => match sheet.shape(e.id) {
                    Ok(s) => s.sample(96).into_iter().map(|p| xf.s(p)).collect(),
                    Err(_) => continue,
                },
                EntityKind::Spline { .. } => match sheet.spline_geometry(e.id) {
                    Ok(s) => s.sample(32).into_iter().map(|p| xf.s(p)).collect(),
                    Err(_) => continue,
                },
            };
            let selected = sel.entities.contains(&e.id);
            if selected {
                painter.add(Shape::line(
                    pts.clone(),
                    stroke(7.0, pal().select.gamma_multiply(0.45)),
                ));
            }
            let (color, width, dash, gap) = role_style(e.role, &tokens);
            let mid = pts[pts.len() / 2];
            let (first, last) = (pts[0], pts[pts.len() - 1]);
            polyline(painter, pts, color, width, dash, gap);
            // Droit-fil : flèche à deux pointes (le rôle ne se reconnaît pas qu'à sa couleur).
            if e.role == PathRole::Grainline {
                let ends = match grain {
                    Some(g) => Some((g.from, g.to, g.spec.sense)),
                    None => sheet
                        .segment_ends(e.id)
                        .ok()
                        .map(|(a, b)| (a, b, pf_core::GrainSense::Unmarked)),
                };
                if let Some((a, b, sense)) = ends {
                    for (p, q) in pf_export::grain_arrow_marks(a, b, sense) {
                        painter.line_segment([xf.s(p), xf.s(q)], stroke(width, color));
                    }
                }
            }

            if selected {
                // Cote de lecture sur les tracés sélectionnés.
                if let Ok(m) = measure::entity_length(sheet, e.id) {
                    let pos = if matches!(e.kind, EntityKind::Segment { .. }) {
                        first + (last - first) * 0.5
                    } else {
                        mid
                    };
                    label_kind(
                        painter,
                        pos + egui::vec2(0.0, 14.0),
                        Align2::CENTER_TOP,
                        &format_mm(m.value),
                        pal().ink,
                        labels::Kind::Selected,
                    );
                }
                // Poignées d'une courbe sélectionnée.
                if let Ok(c) = sheet.curve_geometry(e.id) {
                    for (anchor, handle) in [(c.p0, c.c1), (c.p3, c.c2)] {
                        painter
                            .line_segment([xf.s(anchor), xf.s(handle)], stroke(1.0, pal().handle));
                        painter.circle_filled(xf.s(handle), 5.0, pal().halo);
                        painter.circle_stroke(xf.s(handle), 5.0, stroke(2.0, pal().handle));
                    }
                }
            }
        }

        // Crans : traits perpendiculaires au bord, épais, en noir.
        // Un cran « sur la coupe » (US-06.1) est dessiné sur le contour de coupe calculé, avec un
        // fin repère pointillé depuis son point de couture (jamais la couleur seule : forme, tiret).
        let project = self.editor.view_project();
        for n in sheet.notches.iter().filter(|n| sheet.notch_visible(n)) {
            let Ok((_, marks)) = project.notch_placed_marks(self.editor.sheet, n) else {
                continue;
            };
            let selected = sel.notches.contains(&n.id);
            if n.on_cut {
                if let Ok(c) = project.notch_on_cut(self.editor.sheet, n.id) {
                    painter.line_segment(
                        [xf.s(c.seam_point), xf.s(c.foot)],
                        stroke(1.0, tokens.role_cutting.gamma_multiply(0.6)),
                    );
                    painter.circle_stroke(
                        xf.s(c.seam_point),
                        3.0,
                        stroke(1.2, tokens.role_cutting),
                    );
                }
            }
            for (p, q) in marks {
                if selected {
                    painter.line_segment(
                        [xf.s(p), xf.s(q)],
                        stroke(7.0, pal().select.gamma_multiply(0.5)),
                    );
                }
                painter.line_segment([xf.s(p), xf.s(q)], stroke(2.5, tokens.role_cutting));
            }
        }

        // Cotations : mesures affichées (valeur recalculée), traits fins et flèches.
        for d in sheet
            .dimensions
            .iter()
            .filter(|d| sheet.dimension_visible(d))
        {
            if let Ok(g) = sheet.dimension_geometry(d.id) {
                let selected = sel.dimensions.contains(&d.id);
                let color = if selected { pal().select } else { pal().ink };
                self.paint_dimension(painter, xf, &g, color, selected);
            }
        }

        // Cotes complètes (US-06.7) : mesures observées, texte orienté le long de la ligne.
        for d in sheet
            .full_dimensions
            .iter()
            .filter(|d| sheet.full_dimension_visible(d))
        {
            if let Ok(g) = sheet.full_dimension_geometry(d.id) {
                self.paint_full_dimension(painter, xf, &g, pal().ink);
            }
        }

        // Marque d'angle droit entre deux segments sélectionnés qui se touchent.
        if let [a, b] = sel.entities.as_slice() {
            if let (Ok(m), Ok((a1, a2)), Ok((b1, b2))) = (
                measure::angle_between_segments(sheet, *a, *b),
                sheet.segment_point_ids(*a),
                sheet.segment_point_ids(*b),
            ) {
                let shared = [a1, a2].into_iter().find(|p| *p == b1 || *p == b2);
                if let (Some(v), true) = (shared, (m.value - 90.0).abs() < 1e-9) {
                    let other = |x, y| if x == v { y } else { x };
                    if let (Ok(pv), Ok(pa), Ok(pb)) = (
                        sheet.position(v),
                        sheet.position(other(a1, a2)),
                        sheet.position(other(b1, b2)),
                    ) {
                        self.paint_right_angle(painter, xf, pv, pa, pb);
                    }
                }
            }
        }

        for p in sheet.points.iter().filter(|p| sheet.layer_visible(p.layer)) {
            let Ok(pos) = sheet.position(p.id) else {
                continue;
            };
            let s = xf.s(pos);
            let selected = sel.points.contains(&p.id);
            if selected {
                painter.circle_filled(s, 9.0, pal().select.gamma_multiply(0.5));
            }
            let constructed = !p.definition.is_free();
            let fill = if constructed {
                pal().point_constructed
            } else {
                pal().point_free
            };
            match p.role {
                PointRole::Construction if constructed => {
                    // Losange : point construit sur un segment.
                    let r = 5.5;
                    painter.add(Shape::convex_polygon(
                        vec![
                            s + egui::vec2(0.0, -r),
                            s + egui::vec2(r, 0.0),
                            s + egui::vec2(0.0, r),
                            s + egui::vec2(-r, 0.0),
                        ],
                        fill,
                        stroke(1.0, pal().halo),
                    ));
                }
                PointRole::Construction => {
                    painter.circle_filled(s, 4.0, fill);
                    painter.circle_stroke(s, 4.0, stroke(1.0, pal().halo));
                }
                PointRole::Reference => {
                    // Carré avec croix : point de repère.
                    let r = Rect::from_center_size(s, egui::vec2(9.0, 9.0));
                    painter.rect_filled(r, 0.0, pal().halo);
                    painter.rect_stroke(r, 0.0, stroke(1.5, fill), StrokeKind::Inside);
                    painter.line_segment([r.left_top(), r.right_bottom()], stroke(1.0, fill));
                    painter.line_segment([r.right_top(), r.left_bottom()], stroke(1.0, fill));
                }
            }
            // Marqueur réservé : les étiquettes évitent de le masquer (PH-2c).
            labels::reserve(painter, Rect::from_center_size(s, egui::vec2(12.0, 12.0)));
            let kind = if selected {
                labels::Kind::Selected
            } else {
                labels::Kind::Point
            };
            let pos = s + egui::vec2(8.0, -8.0);
            if !labels::plain(
                painter,
                pos,
                Align2::LEFT_BOTTOM,
                &p.label,
                14.0,
                pal().ink,
                kind,
            ) {
                painter.text(
                    pos,
                    Align2::LEFT_BOTTOM,
                    &p.label,
                    FontId::proportional(14.0),
                    pal().ink,
                );
            }
        }
        self.paint_piece_internals(painter, xf);
        self.paint_piece_markings(painter, xf);
        self.paint_stitch_lines(painter, xf);
        self.paint_zones(painter, xf);
        self.paint_darts(painter, xf);
        self.paint_pleats(painter, xf);
        self.paint_flares(painter, xf);
        self.paint_cutline(painter, xf);
        self.paint_seams(painter, xf);
        self.paint_piece_buttons(painter, xf);
        self.paint_cut_contours(painter, xf);
        self.paint_fold_edges(painter, xf);
        self.paint_hem_marks(painter, xf);
        self.paint_topstitch_stale(painter, xf);
    }

    /// Emprise à cadrer : la feuille et les contours de coupe calculés (US-05.1) qui la dépassent.
    fn fit_bbox(&self) -> Option<Bbox> {
        let mut bb = self.editor.sheet().bbox();
        if let Some(nb) = self.nest_bbox() {
            match bb.as_mut() {
                Some(b) => b.union(nb),
                None => bb = Some(nb),
            }
        }
        let project = self.editor.view_project();
        for piece in project
            .pieces_on(self.editor.sheet)
            .into_iter()
            .filter(|p| p.seam.is_some())
        {
            let Ok(cut) = project.piece_cut_contour(piece.id) else {
                continue;
            };
            for g in cut
                .traces
                .iter()
                .flat_map(pf_geometry::region::trace_geometry)
            {
                for p in pf_geometry::split::edge_sample(&g, 16) {
                    match bb.as_mut() {
                        Some(b) => b.include(p),
                        None => bb = Some(Bbox::from_point(p)),
                    }
                }
            }
        }
        bb
    }

    /// Contour de coupe **calculé** des pièces qui ont une marge de couture (US-05.1), en filigrane :
    /// trait mince à motif trait long / trait court (jamais confondu avec un tracé de rôle coupe,
    /// plein et épais) et libellé « coupe calculée ». Recalculé à chaque image, jamais stocké ;
    /// non sélectionnable.
    fn paint_cut_contours(&self, painter: &egui::Painter, xf: &Xf) {
        let project = self.editor.view_project();
        let color = pf_ui::theme::tokens(painter.ctx()).role_cutting;
        for piece in project.pieces_on(self.editor.sheet) {
            // Coupe nette (US-05.6) : contour de coupe = contour dessiné, rien à dédoubler ;
            // seulement le libellé écrit (jamais la couleur seule), au coin haut-gauche.
            if piece.net_cut {
                let Ok(cut) = project.piece_cut_contour(piece.id) else {
                    continue;
                };
                let mut top: Option<Pos2> = None;
                for g in cut
                    .traces
                    .iter()
                    .flat_map(pf_geometry::region::trace_geometry)
                {
                    for p in pf_geometry::split::edge_sample(&g, 16) {
                        let s = xf.s(p);
                        let higher = top.is_none_or(|t| {
                            s.y < t.y - 0.5 || ((s.y - t.y).abs() <= 0.5 && s.x < t.x)
                        });
                        if higher {
                            top = Some(s);
                        }
                    }
                }
                if let Some(top) = top {
                    label(
                        painter,
                        top + egui::vec2(0.0, -6.0),
                        Align2::LEFT_BOTTOM,
                        &tr("piece.netcut.canvas"),
                        color,
                    );
                }
                continue;
            }
            if piece.seam.is_none() {
                continue;
            }
            let Ok(cut) = project.piece_cut_contour(piece.id) else {
                continue;
            };
            // La portion qui suit un bord sur le pli (PH-5b) n'est pas un bord de coupe : la coupe
            // est dessinée en suites ouvertes, la pliure l'est à part (`paint_fold_edges`).
            let mut runs: Vec<Vec<Pos2>> = vec![Vec::new()];
            for (i, t) in cut.traces.iter().enumerate() {
                if cut.fold_cuts.iter().any(|f| f.trace == i) {
                    if runs.last().is_some_and(|r| !r.is_empty()) {
                        runs.push(Vec::new());
                    }
                    continue;
                }
                for g in pf_geometry::region::trace_geometry(t) {
                    for p in pf_geometry::split::edge_sample(&g, 64) {
                        let s = xf.s(p);
                        let run = runs.last_mut().expect("une suite");
                        if run.last().is_none_or(|l| l.distance(s) > 0.01) {
                            run.push(s);
                        }
                    }
                }
            }
            runs.retain(|r| !r.is_empty());
            let closed = cut.fold_cuts.is_empty();
            let Some(first) = runs.first().and_then(|r| r.first()).copied() else {
                continue;
            };
            if closed {
                if let Some(run) = runs.first_mut() {
                    run.push(first);
                }
            }
            let pts: Vec<Pos2> = runs.iter().flatten().copied().collect();
            // Libellé au coin haut-gauche du contour (jamais coupé par le panneau de droite).
            let top = pts.iter().copied().fold(first, |a, b| {
                if b.y < a.y - 0.5 || ((b.y - a.y).abs() <= 0.5 && b.x < a.x) {
                    b
                } else {
                    a
                }
            });
            for run in runs {
                polyline(
                    painter,
                    run,
                    color,
                    1.6,
                    &[10.0, 2.5, 2.5, 2.5],
                    &[3.5, 3.5, 3.5, 3.5],
                );
            }
            label(
                painter,
                top + egui::vec2(0.0, -6.0),
                Align2::LEFT_BOTTOM,
                &tr("piece.seam.canvas"),
                color,
            );
        }
    }

    /// Bords sur le pli (PH-5b) : ligne de pliure d'un motif propre (long trait, point), prolongée
    /// jusqu'aux marges voisines quand la coupe est calculée, et libellé « SUR LE PLI » côté
    /// intérieur au milieu du bord (jamais la couleur seule). Recalculé à chaque image.
    fn paint_fold_edges(&self, painter: &egui::Painter, xf: &Xf) {
        let project = self.editor.view_project();
        let color = pf_ui::theme::tokens(painter.ctx()).role_fold;
        for piece in project.pieces_on(self.editor.sheet) {
            if piece.on_fold.is_empty() {
                continue;
            }
            for line in project.piece_fold_lines(piece.id, true) {
                let (a, b) = (xf.s(line.from), xf.s(line.to));
                polyline(
                    painter,
                    vec![a, b],
                    color,
                    2.6,
                    &[18.0, 3.0, 2.0, 3.0],
                    &[4.0, 4.0, 4.0, 4.0],
                );
                // Intérieur = à gauche de `from → to` (contour anti-horaire, Y vers le haut) ; à
                // l'écran Y est inversé : le point de dessin est décalé de 14 px vers l'intérieur.
                let mid = Pos2::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
                let (dx, dy) = (b.x - a.x, b.y - a.y);
                let len = dx.hypot(dy).max(1e-3);
                let inward = egui::vec2(dy / len, -dx / len);
                // Le libellé est horizontal : le décalage tient compte de sa taille pour qu'il
                // longe la ligne sans la recouvrir, quelle que soit l'orientation du bord.
                let text = tr("onfold.label");
                let size = painter
                    .layout_no_wrap(text.clone(), FontId::proportional(13.0), color)
                    .size();
                let off = 6.0
                    + inward.x.abs() * (size.x / 2.0 + 3.0)
                    + inward.y.abs() * (size.y / 2.0 + 3.0);
                label(
                    painter,
                    mid + inward * off,
                    Align2::CENTER_CENTER,
                    &text,
                    color,
                );
            }
        }
    }

    /// Ourlets et rempliés (US-05.3) : étiquette « ourlet H mm » / « rempli V mm » au milieu du
    /// bord (jamais la couleur seule) et petit trait extérieur à chaque cran de rempli **calculé**
    /// (recalculé à chaque image, jamais stocké ; la pose réelle des crans est E06).
    fn paint_hem_marks(&self, painter: &egui::Painter, xf: &Xf) {
        let project = self.editor.view_project();
        let color = pf_ui::theme::tokens(painter.ctx()).role_cutting;
        for piece in project.pieces_on(self.editor.sheet) {
            let Some(seam) = piece.seam.as_ref() else {
                continue;
            };
            if seam.hems.is_empty() && seam.folds.is_empty() {
                continue;
            }
            let Ok(sheet) = project.piece_sheet(piece.id) else {
                continue;
            };
            let mid = |entity| {
                let pts = sheet.shape(entity).ok()?.sample(8);
                // Segment : deux extrémités seulement, le milieu est leur moyenne.
                let (a, b) = (*pts.first()?, *pts.last()?);
                Some(if pts.len() == 2 {
                    xf.s(PointMm::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0))
                } else {
                    xf.s(pts[pts.len() / 2])
                })
            };
            for h in &seam.hems {
                if let Some(at) = mid(h.entity) {
                    label(
                        painter,
                        at + egui::vec2(0.0, 34.0),
                        Align2::CENTER_TOP,
                        &pf_i18n::tr_len(
                            "piece.hem.canvas",
                            &[("height", &pf_core::units::len_number(h.height_mm, 2))],
                        ),
                        color,
                    );
                }
            }
            for f in &seam.folds {
                if let Some(at) = mid(f.entity) {
                    label(
                        painter,
                        at + egui::vec2(0.0, 34.0),
                        Align2::CENTER_TOP,
                        &pf_i18n::tr_len(
                            "piece.fold.canvas",
                            &[("value", &pf_core::units::len_number(f.value_mm, 2))],
                        ),
                        color,
                    );
                }
            }
            let notches = project.piece_fold_notches(piece.id).unwrap_or_default();
            for n in &notches {
                let a = xf.s(n.point);
                let b = xf.s(PointMm::new(n.point.x + n.normal.x, n.point.y + n.normal.y));
                let d = b - a;
                if d.length() < 1e-6 {
                    continue;
                }
                let tip = a + d / d.length() * 8.0;
                painter.line_segment([a, tip], stroke(2.0, color));
            }
            if let Some(n) = notches.first() {
                label(
                    painter,
                    xf.s(n.point) + egui::vec2(10.0, 10.0),
                    Align2::LEFT_TOP,
                    &tr("piece.fold.canvas.notch"),
                    color,
                );
            }
        }
    }

    /// Surpiqûre liée obsolète (US-06.8) : « ⚠ » et texte au milieu de sa première ligne, en plus
    /// de la couleur d'erreur (jamais la couleur seule). Aucun indicateur quand tout est à jour.
    fn paint_topstitch_stale(&self, painter: &egui::Painter, xf: &Xf) {
        let sheet = self.editor.sheet();
        for t in sheet.topstitches.iter().filter(|t| t.stale.is_some()) {
            let Some(shape) = t.paths.first().and_then(|e| sheet.shape(*e).ok()) else {
                continue;
            };
            let pts = shape.sample(8);
            let mid = xf.s(pts[pts.len() / 2]);
            label(
                painter,
                mid + egui::vec2(0.0, -6.0),
                Align2::CENTER_BOTTOM,
                &format!("⚠ {}", tr("topstitch.canvas.stale")),
                pal().error,
            );
        }
    }

    /// Boutons (cercle du diamètre réel) et boutonnières (fente à bouts arrondis à l'échelle)
    /// des pièces de la feuille (US-06.4) : la **forme** et l'étiquette les distinguent, jamais
    /// la couleur seule ; un élément trop proche du bord porte un « ⚠ » en plus de sa couleur.
    /// Non sélectionnable, absent du plan de pièces.
    fn paint_piece_buttons(&self, painter: &egui::Painter, xf: &Xf) {
        use pf_core::ButtonShape as B;
        let project = self.editor.view_project();
        let zoom = self.view.zoom as f32;
        for piece in project.pieces_on(self.editor.sheet) {
            for group in project.piece_buttons(piece.id) {
                for el in &group.elements {
                    let c = xf.s(el.center);
                    let color = if el.too_close {
                        pal().error
                    } else {
                        pal().internal
                    };
                    let st = stroke(2.0, color);
                    let reach = match el.shape {
                        B::Button { diameter_mm } => {
                            let radius = (diameter_mm as f32 / 2.0 * zoom).max(3.0);
                            painter.circle_stroke(c, radius, st);
                            // Point central : le centre exact du bouton.
                            painter.circle_filled(c, 1.5, color);
                            radius
                        }
                        B::Buttonhole { .. } => {
                            let Some(slot) = el.shape.slot(el.center) else {
                                continue;
                            };
                            let mut pts: Vec<Pos2> =
                                slot.polyline(12).into_iter().map(|p| xf.s(p)).collect();
                            if let Some(first) = pts.first().copied() {
                                pts.push(first);
                            }
                            let (hx, hy) = slot.half_extent();
                            painter.add(Shape::line(pts, st));
                            (hx.max(hy) as f32 * zoom).max(4.0)
                        }
                    };
                    if el.index == 0 {
                        label(
                            painter,
                            c + egui::vec2(reach + 4.0, reach + 2.0),
                            Align2::LEFT_TOP,
                            &crate::buttons::shape_label(el.shape),
                            color,
                        );
                    }
                    if el.too_close {
                        label(
                            painter,
                            c + egui::vec2(0.0, -(reach + 4.0)),
                            Align2::CENTER_BOTTOM,
                            "⚠",
                            color,
                        );
                    }
                }
            }
        }
    }

    /// Perçages, pointages et croix des pièces de la feuille (US-06.3) : la **forme**
    /// (disque plein, cercle du diamètre réel, croix) et son libellé les distinguent,
    /// jamais la couleur seule. Un cercle est à l'échelle de la vue (rayon minimal de
    /// 3 px pour rester visible) ; non sélectionnable et absent du plan de pièces.
    fn paint_piece_markings(&self, painter: &egui::Painter, xf: &Xf) {
        use pf_core::MarkingShape as M;
        let project = self.editor.view_project();
        let color = pal().internal;
        let zoom = self.view.zoom as f32;
        let mut last_label: Option<(String, Pos2)> = None;
        for piece in project.pieces_on(self.editor.sheet) {
            for r in project.piece_markings(piece.id) {
                let c = xf.s(r.at);
                let st = stroke(2.0, color);
                let reach = match r.marking.shape {
                    M::Punch => {
                        painter.circle_filled(c, 3.5, color);
                        painter.circle_stroke(c, 3.5, stroke(1.0, pal().halo));
                        3.5
                    }
                    M::Drill { diameter_mm } => {
                        let radius = (diameter_mm as f32 / 2.0 * zoom).max(3.0);
                        painter.circle_stroke(c, radius, st);
                        radius
                    }
                    M::Cross { size_mm } => {
                        let half = (size_mm as f32 / 2.0 * zoom).max(4.0);
                        painter.line_segment(
                            [c - egui::vec2(half, 0.0), c + egui::vec2(half, 0.0)],
                            st,
                        );
                        painter.line_segment(
                            [c - egui::vec2(0.0, half), c + egui::vec2(0.0, half)],
                            st,
                        );
                        half
                    }
                    M::Slot { .. } => {
                        let Some(slot) = r.marking.shape.slot(r.at) else {
                            continue;
                        };
                        let mut pts: Vec<Pos2> =
                            slot.polyline(12).into_iter().map(|p| xf.s(p)).collect();
                        if let Some(first) = pts.first().copied() {
                            pts.push(first);
                        }
                        let (hx, hy) = slot.half_extent();
                        painter.add(Shape::line(pts, st));
                        (hx.max(hy) as f32 * zoom).max(4.0)
                    }
                };
                // Une rangée de perçages identiques (PH-4b) n'écrit pas son libellé à chaque
                // perçage : il est répété seulement quand le précédent est assez loin.
                let text = crate::text::marking_shape_label(r.marking.shape);
                if last_label
                    .as_ref()
                    .is_some_and(|(t, p)| *t == text && p.distance(c) < 140.0)
                {
                    continue;
                }
                label(
                    painter,
                    c + egui::vec2(reach + 4.0, reach + 2.0),
                    Align2::LEFT_TOP,
                    &text,
                    color,
                );
                last_label = Some((text, c));
            }
        }
    }

    /// Éléments internes des pièces de la feuille (US-04.3), par-dessus la construction :
    /// motif de trait **et** libellé du type (jamais la couleur seule). Lignes : trait
    /// tireté propre au type ; repères : cercle (marque), rectangle (boutonnière) ou
    /// losange (autre).
    fn paint_piece_internals(&self, painter: &egui::Painter, xf: &Xf) {
        use pf_core::piece_internal::InternalGeometry as G;
        use pf_core::{InternalKind as K, InternalTarget as T};
        let project = self.editor.view_project();
        let sheet = self.editor.sheet();
        let color = pal().internal;
        for piece in project.pieces_on(self.editor.sheet) {
            for r in project.piece_internals(piece.id) {
                let kind = r.element.kind;
                let text = crate::text::internal_kind_label(kind);
                match (r.element.target, r.geometry) {
                    (T::Path { entity }, G::Path { .. }) => {
                        let Ok(shape) = sheet.shape(entity) else {
                            continue;
                        };
                        let pts: Vec<Pos2> =
                            shape.sample(96).into_iter().map(|p| xf.s(p)).collect();
                        if pts.len() < 2 {
                            continue;
                        }
                        let (dash, gap): (&[f32], &[f32]) = match kind {
                            K::Fold => (&[10.0, 3.0, 2.0, 3.0], &[3.0, 3.0, 3.0, 3.0]),
                            K::Topstitch => (&[3.0], &[3.0]),
                            K::PocketPosition => (&[8.0, 2.0], &[3.0, 3.0]),
                            _ => (&[1.5], &[4.0]),
                        };
                        let mid = pts[pts.len() / 2];
                        polyline(painter, pts, color, 2.4, dash, gap);
                        label(
                            painter,
                            mid + egui::vec2(0.0, -12.0),
                            Align2::CENTER_BOTTOM,
                            &text,
                            color,
                        );
                    }
                    (_, G::Position { at }) => {
                        let s = xf.s(at);
                        let st = stroke(2.0, color);
                        match kind {
                            K::Buttonhole => {
                                let rect = Rect::from_center_size(s, egui::vec2(16.0, 7.0));
                                painter.rect_stroke(rect, 2.0, st, StrokeKind::Middle);
                            }
                            K::Other => {
                                let r = 7.0;
                                painter.add(Shape::closed_line(
                                    vec![
                                        s + egui::vec2(0.0, -r),
                                        s + egui::vec2(r, 0.0),
                                        s + egui::vec2(0.0, r),
                                        s + egui::vec2(-r, 0.0),
                                    ],
                                    st,
                                ));
                            }
                            _ => {
                                painter.circle_stroke(s, 7.0, st);
                                painter.line_segment(
                                    [s - egui::vec2(4.0, 0.0), s + egui::vec2(4.0, 0.0)],
                                    st,
                                );
                                painter.line_segment(
                                    [s - egui::vec2(0.0, 4.0), s + egui::vec2(0.0, 4.0)],
                                    st,
                                );
                            }
                        }
                        label(
                            painter,
                            s + egui::vec2(10.0, 8.0),
                            Align2::LEFT_TOP,
                            &text,
                            color,
                        );
                    }
                    _ => {}
                }
            }
        }
    }

    fn paint_right_angle(
        &self,
        painter: &egui::Painter,
        xf: &Xf,
        v: PointMm,
        a: PointMm,
        b: PointMm,
    ) {
        let (Ok(ua), Ok(ub)) = ((a - v).unit(), (b - v).unit()) else {
            return;
        };
        let k = 12.0 / self.view.zoom;
        let (p1, p2) = (v + ua * k, v + ub * k);
        let corner = p1 + ub * k;
        painter.add(Shape::line(
            vec![xf.s(p1), xf.s(corner), xf.s(p2)],
            stroke(1.5, pal().angle),
        ));
    }

    /// Cotation : traits d'attache, ligne de cote, flèches, valeur au milieu.
    fn paint_dimension(
        &self,
        painter: &egui::Painter,
        xf: &Xf,
        g: &pf_core::dimension::DimensionGeometry,
        color: Color32,
        selected: bool,
    ) {
        let scene = pf_export::SceneDimension {
            from: g.from,
            to: g.to,
            from_offset: g.from_offset,
            to_offset: g.to_offset,
            text: String::new(),
        };
        for (p, q) in scene.marks() {
            if selected {
                painter.line_segment(
                    [xf.s(p), xf.s(q)],
                    stroke(5.0, pal().select.gamma_multiply(0.4)),
                );
            }
            painter.line_segment([xf.s(p), xf.s(q)], stroke(1.2, color));
        }
        let mid = xf.s(g.from_offset.lerp(g.to_offset, 0.5));
        label_kind(
            painter,
            mid + egui::vec2(0.0, -4.0),
            Align2::CENTER_BOTTOM,
            &format_mm(g.value_mm),
            color,
            if selected {
                labels::Kind::Selected
            } else {
                labels::Kind::Dimension
            },
        );
    }

    /// Cote complète : mêmes traits que l'export (attaches, ligne ou arc, flèches en mm, donc
    /// à l'échelle) et texte réel orienté le long de la ligne (jamais à l'envers), posé au-dessus
    /// de celle-ci.
    fn paint_full_dimension(
        &self,
        painter: &egui::Painter,
        xf: &Xf,
        g: &pf_core::full_dimension::FullDimensionGeometry,
        color: Color32,
    ) {
        let scene = pf_export::SceneFullDimension::from_geometry(g);
        for (p, q) in &scene.marks {
            painter.line_segment([xf.s(*p), xf.s(*q)], stroke(1.2, color));
        }
        let text = g.text(pf_core::input::decimal_separator());
        let galley = painter.layout_no_wrap(text, FontId::proportional(11.0), color);
        // Angle d'écran : le sens trigonométrique du modèle devient un angle horaire (Y vers
        // le bas). Le centre du texte est remonté, en pixels, de la moitié de sa hauteur + 3 px.
        let theta = g.text_angle_deg.to_radians() as f32;
        let (sin_t, cos_t) = theta.sin_cos();
        let lift = galley.size().y / 2.0 + 3.0;
        let center = xf.s(g.text_anchor) + egui::vec2(-sin_t, -cos_t) * lift;
        let a = -theta;
        let (sa, ca) = a.sin_cos();
        let half = galley.size() / 2.0;
        let off = egui::vec2(half.x * ca - half.y * sa, half.x * sa + half.y * ca);
        let anchor = xf.s(g.text_anchor);
        if !labels::rotated(
            painter,
            galley.clone(),
            center - off,
            a,
            anchor,
            color,
            labels::Kind::Dimension,
        ) {
            painter.add(egui::epaint::TextShape::new(center - off, galley, color).with_angle(a));
        }
    }

    /// Règles graduées en mm sur les bords haut et gauche (affichage seul).
    fn paint_rulers(&self, painter: &egui::Painter, rect: Rect, xf: &Xf) {
        const BAND: f32 = RULER_BAND;
        let ink = pal().muted;
        let font = FontId::proportional(10.0);
        painter.rect_filled(
            Rect::from_min_max(rect.min, Pos2::new(rect.max.x, rect.min.y + BAND)),
            0.0,
            pal().label_bg,
        );
        painter.rect_filled(
            Rect::from_min_max(rect.min, Pos2::new(rect.min.x + BAND, rect.max.y)),
            0.0,
            pal().label_bg,
        );
        // Graduation principale : premier pas 1-2-5 (1/16 po et fractions en pouces) dont l'écart
        // à l'écran dépasse 60 px ; subdivisions de la grille.
        let units = self.view_settings.units;
        let step = crate::views::ruler_major_mm(units.unit, self.view.zoom);
        let sub = step / f64::from(self.view_settings.grid.subdivisions);
        let (min, max) = (xf.w(rect.left_bottom()), xf.w(rect.right_top()));
        let is_major = |v: f64| (v / step - (v / step).round()).abs() < 1e-6;
        let mut x = (min.x / sub).floor() * sub;
        while x <= max.x {
            let sx = xf.s(PointMm::new(x, 0.0)).x;
            if sx >= rect.left() + BAND {
                let len = if is_major(x) { 9.0 } else { 4.0 };
                painter.line_segment(
                    [
                        Pos2::new(sx, rect.top() + BAND),
                        Pos2::new(sx, rect.top() + BAND - len),
                    ],
                    stroke(1.0, ink),
                );
                if is_major(x) {
                    painter.text(
                        Pos2::new(sx + 2.0, rect.top() + 1.0),
                        Align2::LEFT_TOP,
                        crate::views::ruler_label(x, step, units),
                        font.clone(),
                        ink,
                    );
                }
            }
            x += sub;
        }
        let mut y = (min.y / sub).floor() * sub;
        while y <= max.y {
            let sy = xf.s(PointMm::new(0.0, y)).y;
            if sy >= rect.top() + BAND {
                let len = if is_major(y) { 9.0 } else { 4.0 };
                painter.line_segment(
                    [
                        Pos2::new(rect.left() + BAND, sy),
                        Pos2::new(rect.left() + BAND - len, sy),
                    ],
                    stroke(1.0, ink),
                );
                if is_major(y) {
                    painter.text(
                        Pos2::new(rect.left() + 1.0, sy + 1.0),
                        Align2::LEFT_TOP,
                        crate::views::ruler_label(y, step, units),
                        font.clone(),
                        ink,
                    );
                }
            }
            y += sub;
        }
    }

    fn paint_overlays(&self, ctx: &egui::Context, painter: &egui::Painter, xf: &Xf) {
        let ghost = stroke(1.5, pal().preview);
        let dashed = |a: PointMm, b: PointMm| {
            painter.extend(Shape::dashed_line(&[xf.s(a), xf.s(b)], ghost, 6.0, 4.0));
        };
        let dot = |p: PointMm| {
            painter.circle_filled(xf.s(p), 4.0, pal().preview);
        };

        // Référence choisie pour une perpendiculaire ou un point à distance.
        let reference = match self.editor.state {
            ToolState::PerpPickStart { reference }
            | ToolState::PerpFrom { reference, .. }
            | ToolState::NotchOn { reference }
            | ToolState::ParallelOf { reference } => Some(reference),
            ToolState::PointOn { segment, .. } => Some(segment),
            _ => None,
        };
        if let Some(Ok((a, b))) = reference.map(|r| self.editor.sheet().segment_ends(r)) {
            painter.line_segment(
                [xf.s(a), xf.s(b)],
                stroke(6.0, pal().preview.gamma_multiply(0.3)),
            );
        }

        // Marqueur d'accrochage : forme différente selon la nature, plus un libellé.
        if let Some(snap) = self.editor.hover_snap() {
            let c = xf.s(snap.pos);
            let mark = stroke(2.0, pal().snap);
            let r = 7.0_f32;
            match snap.kind {
                SnapKind::Point => {
                    painter.rect_stroke(
                        Rect::from_center_size(c, egui::vec2(2.0 * r, 2.0 * r)),
                        0.0_f32,
                        mark,
                        StrokeKind::Middle,
                    );
                }
                SnapKind::Intersection => {
                    painter.line_segment([c + egui::vec2(-r, -r), c + egui::vec2(r, r)], mark);
                    painter.line_segment([c + egui::vec2(-r, r), c + egui::vec2(r, -r)], mark);
                }
                SnapKind::Midpoint => {
                    painter.add(Shape::closed_line(
                        vec![
                            c + egui::vec2(0.0, -r),
                            c + egui::vec2(r, r * 0.8),
                            c + egui::vec2(-r, r * 0.8),
                        ],
                        mark,
                    ));
                }
                SnapKind::OnPath => {
                    painter.circle_stroke(c, r, mark);
                }
                SnapKind::OnGuide => {
                    painter.line_segment([c + egui::vec2(-r, 0.0), c + egui::vec2(r, 0.0)], mark);
                    painter.line_segment([c + egui::vec2(0.0, -r), c + egui::vec2(0.0, r)], mark);
                }
                SnapKind::Free => {}
            }
            label_now(
                painter,
                c + egui::vec2(12.0, -12.0),
                Align2::LEFT_BOTTOM,
                &snap.kind.label(),
                pal().snap,
            );
        }

        // Ouverture trouvée par « Extraire une pièce » : deux anneaux sur les extrémités,
        // un trait pointillé entre elles et l'écart écrit (jamais la couleur seule).
        if let Some(gap) = self.editor.piece_gap {
            let mark = stroke(2.5, pal().error);
            for p in [gap.from, gap.to] {
                painter.circle_stroke(xf.s(p), 9.0, mark);
            }
            painter.extend(Shape::dashed_line(
                &[xf.s(gap.from), xf.s(gap.to)],
                mark,
                4.0,
                3.0,
            ));
            label_now(
                painter,
                xf.s(gap.from.lerp(gap.to, 0.5)) + egui::vec2(0.0, 16.0),
                Align2::CENTER_TOP,
                &format_mm(gap.gap_mm),
                pal().error,
            );
        }

        // Séparation d'une pièce (US-04.6) : droite de coupe, points de coupe, aires des deux
        // moitiés (ou raison du refus). Le texte double toujours la couleur.
        if let Some(draft) = self.editor.split_draft() {
            let clip = painter.clip_rect();
            let diag = pf_geometry::distance_mm(xf.w(clip.left_bottom()), xf.w(clip.right_top()));
            let (dx, dy) = (draft.to.x - draft.from.x, draft.to.y - draft.from.y);
            let len = dx.hypot(dy).max(1e-9);
            let reach = 2.0 * diag + len;
            let far =
                |s: f64| PointMm::new(draft.from.x + dx / len * s, draft.from.y + dy / len * s);
            let color = if draft.preview.is_ok() {
                pal().preview
            } else {
                pal().error
            };
            painter.line_segment(
                [xf.s(far(-reach)), xf.s(far(reach))],
                stroke(1.5, color.gamma_multiply(0.6)),
            );
            match &draft.preview {
                Ok(pre) => {
                    painter.extend(Shape::dashed_line(
                        &[xf.s(pre.points[0]), xf.s(pre.points[1])],
                        stroke(3.0, color),
                        6.0,
                        4.0,
                    ));
                    for p in pre.points {
                        painter.circle_stroke(xf.s(p), 6.0, stroke(2.0, color));
                    }
                    label_now(
                        painter,
                        xf.s(pre.points[0].lerp(pre.points[1], 0.5)) + egui::vec2(0.0, 16.0),
                        Align2::CENTER_TOP,
                        &tr_args(
                            "status.split.preview",
                            &[
                                ("first", &format_number(pre.left.area_mm2 / 100.0, 2)),
                                ("second", &format_number(pre.right.area_mm2 / 100.0, 2)),
                            ],
                        ),
                        color,
                    );
                }
                Err(reason) => label_now(
                    painter,
                    xf.s(draft.to) + egui::vec2(12.0, 16.0),
                    Align2::LEFT_TOP,
                    reason,
                    color,
                ),
            }
        }

        // Courbe lisse en cours de pose : courbe d'aperçu et points de passage posés.
        if let Some((curve, placed)) = self.editor.spline_draft() {
            let line: Vec<Pos2> = curve.into_iter().map(|p| xf.s(p)).collect();
            if line.len() >= 2 {
                painter.add(Shape::line(line, ghost));
            }
            for p in placed {
                dot(p);
            }
        }

        // Décalage : chaîne choisie en surbrillance, résultat en aperçu.
        if let Some((chain, result)) = self.editor.offset_draft() {
            for line in chain {
                let pts: Vec<Pos2> = line.into_iter().map(|p| xf.s(p)).collect();
                if pts.len() >= 2 {
                    painter.add(Shape::line(
                        pts,
                        stroke(6.0, pal().preview.gamma_multiply(0.3)),
                    ));
                }
            }
            for line in result {
                let pts: Vec<Pos2> = line.into_iter().map(|p| xf.s(p)).collect();
                if pts.len() >= 2 {
                    painter.add(Shape::line(pts, ghost));
                }
            }
        }

        if let Ok(Some(preview)) = self.editor.preview() {
            match preview {
                Preview::Segment {
                    from,
                    to,
                    length_mm,
                    angle_deg,
                } => {
                    dashed(from, to);
                    dot(from);
                    dot(to);
                    let text = format!("{} · {}", format_mm(length_mm), format_deg(angle_deg));
                    label_now(
                        painter,
                        xf.s(to) + egui::vec2(12.0, 12.0),
                        Align2::LEFT_TOP,
                        &text,
                        pal().preview,
                    );
                }
                Preview::Perpendicular {
                    from,
                    to,
                    length_mm,
                    ..
                } => {
                    dashed(from, to);
                    dot(from);
                    dot(to);
                    if let ToolState::PerpFrom { reference, .. } = self.editor.state {
                        if let Ok((ra, rb)) = self.editor.sheet().segment_ends(reference) {
                            let along = if pf_geometry::distance_mm(from, ra)
                                > pf_geometry::distance_mm(from, rb)
                            {
                                ra
                            } else {
                                rb
                            };
                            self.paint_right_angle(painter, xf, from, along, to);
                        }
                    }
                    let text = format!("{} · {}", format_mm(length_mm), format_deg(90.0));
                    label_now(
                        painter,
                        xf.s(to) + egui::vec2(12.0, 12.0),
                        Align2::LEFT_TOP,
                        &text,
                        pal().preview,
                    );
                }
                Preview::PointOn {
                    origin,
                    at,
                    distance_mm,
                } => {
                    painter.line_segment([xf.s(origin), xf.s(at)], stroke(3.0, pal().preview));
                    dot(at);
                    label_now(
                        painter,
                        xf.s(at) + egui::vec2(0.0, 14.0),
                        Align2::CENTER_TOP,
                        &format_mm(distance_mm),
                        pal().preview,
                    );
                }
                Preview::Rectangle {
                    from,
                    to,
                    width_mm,
                    height_mm,
                } => {
                    let corners = [
                        from,
                        PointMm::new(to.x, from.y),
                        to,
                        PointMm::new(from.x, to.y),
                        from,
                    ];
                    for w in corners.windows(2) {
                        dashed(w[0], w[1]);
                    }
                    dot(from);
                    dot(to);
                    let text = format!(
                        "{} × {}",
                        format_mm(width_mm.abs()),
                        format_mm(height_mm.abs())
                    );
                    label_now(
                        painter,
                        xf.s(to) + egui::vec2(12.0, 12.0),
                        Align2::LEFT_TOP,
                        &text,
                        pal().preview,
                    );
                }
                Preview::Parallel {
                    from,
                    to,
                    distance_mm,
                    ..
                } => {
                    dashed(from, to);
                    dot(from);
                    dot(to);
                    label_now(
                        painter,
                        xf.s(from.lerp(to, 0.5)) + egui::vec2(0.0, 14.0),
                        Align2::CENTER_TOP,
                        &format_mm(distance_mm),
                        pal().preview,
                    );
                }
                Preview::Notch {
                    base,
                    distance_mm,
                    side,
                    length_mm,
                    style,
                } => {
                    if let ToolState::NotchOn { reference } = self.editor.state {
                        let n = pf_core::Notch {
                            id: pf_core::NotchId(0),
                            entity: reference,
                            distance_mm,
                            side,
                            length_mm,
                            style,
                            anchor: None,
                            form: pf_core::NotchForm::Line,
                            width_mm: None,
                            on_cut: false,
                            corner: false,
                            proportional: false,
                        };
                        if let Ok(marks) = self.editor.sheet().notch_marks(&n) {
                            for (p, q) in marks {
                                painter
                                    .line_segment([xf.s(p), xf.s(q)], stroke(3.0, pal().preview));
                            }
                        }
                    }
                    dot(base);
                    label_now(
                        painter,
                        xf.s(base) + egui::vec2(10.0, 14.0),
                        Align2::LEFT_TOP,
                        &format_mm(distance_mm),
                        pal().preview,
                    );
                }
                Preview::Curve { from, to } => {
                    dashed(from, to);
                    dot(from);
                }
                Preview::Circle { center, radius_mm } => {
                    if let Ok(c) = pf_geometry::CircleTrace::new(center, radius_mm, 0.0, true) {
                        let pts: Vec<Pos2> = c.sample(96).into_iter().map(|p| xf.s(p)).collect();
                        painter.extend(Shape::dashed_line(&pts, ghost, 6.0, 4.0));
                    }
                    dot(center);
                    label_now(
                        painter,
                        xf.s(center) + egui::vec2(12.0, 12.0),
                        Align2::LEFT_TOP,
                        &format!("R {}", format_mm(radius_mm)),
                        pal().preview,
                    );
                }
                Preview::Arc {
                    center,
                    radius_mm,
                    start_deg,
                    end_deg,
                    ccw,
                } => {
                    if let Ok(a) = pf_geometry::Arc::from_center_angles(
                        center, radius_mm, start_deg, end_deg, ccw,
                    ) {
                        let pts: Vec<Pos2> = a.sample(96).into_iter().map(|p| xf.s(p)).collect();
                        painter.extend(Shape::dashed_line(&pts, ghost, 6.0, 4.0));
                        dot(a.start_point());
                        dot(a.end_point());
                        label_now(
                            painter,
                            xf.s(a.end_point()) + egui::vec2(12.0, 12.0),
                            Align2::LEFT_TOP,
                            &format!(
                                "R {} · {} · {}",
                                format_mm(radius_mm),
                                format_deg(a.sweep_deg().abs()),
                                format_mm(a.length_mm())
                            ),
                            pal().preview,
                        );
                    }
                    dashed(
                        center,
                        pf_geometry::point_from_polar(center, radius_mm, start_deg)
                            .unwrap_or(center),
                    );
                    dot(center);
                }
                Preview::Guide { through, angle_deg } => {
                    let g = Guide {
                        id: GuideId(0),
                        x_mm: through.x,
                        y_mm: through.y,
                        angle_deg,
                        axis: false,
                        layer: None,
                    };
                    if let Some((p, q)) = guide_segment(painter, xf, &g) {
                        painter.extend(Shape::dashed_line(&[p, q], ghost, 6.0, 4.0));
                    }
                    dot(through);
                    label_now(
                        painter,
                        xf.s(through) + egui::vec2(12.0, 12.0),
                        Align2::LEFT_TOP,
                        &format_deg(angle_deg),
                        pal().preview,
                    );
                }
                Preview::Dimension { geometry, .. } => {
                    self.paint_dimension(painter, xf, &geometry, pal().preview, false);
                    dot(geometry.from);
                    dot(geometry.to);
                }
                Preview::Measure {
                    from,
                    to,
                    distance_mm,
                } => {
                    dashed(from, to);
                    label_now(
                        painter,
                        xf.s(to) + egui::vec2(12.0, 12.0),
                        Align2::LEFT_TOP,
                        &format_mm(distance_mm),
                        pal().preview,
                    );
                }
            }
        }

        // Dernière mesure de distance (lecture seule).
        if let Some((a, b)) = self.editor.last_measure {
            let color = pal().measure;
            painter.extend(Shape::dashed_line(
                &[xf.s(a), xf.s(b)],
                stroke(1.5, color),
                4.0,
                3.0,
            ));
            for p in [a, b] {
                painter.circle_stroke(xf.s(p), 5.0, stroke(1.5, color));
            }
            let mid = xf.s(a.lerp(b, 0.5));
            let text = tr_args(
                "canvas.measure",
                &[("value", &format_mm(pf_geometry::distance_mm(a, b)))],
            );
            label_kind(
                painter,
                mid + egui::vec2(0.0, -10.0),
                Align2::CENTER_BOTTOM,
                &text,
                color,
                labels::Kind::Dimension,
            );
        }

        // Fenêtre de sélection en cours.
        if let Some(start) = self.window_start {
            if ctx.input(|i| i.pointer.primary_down()) {
                let r = Rect::from_two_pos(xf.s(start), xf.s(self.editor.cursor));
                painter.rect_filled(r, 0.0, pal().select.gamma_multiply(0.12));
                painter.rect_stroke(r, 0.0, stroke(1.0, pal().select), StrokeKind::Inside);
            }
        }

        // Lasso en cours : contour libre, refermé sur son départ.
        if let Some(path) = &self.lasso {
            if path.len() >= 2 && ctx.input(|i| i.pointer.primary_down()) {
                let mut pts: Vec<Pos2> = path.iter().map(|p| xf.s(*p)).collect();
                pts.push(xf.s(self.editor.cursor));
                painter.add(Shape::closed_line(pts, stroke(1.5, pal().select)));
            }
        }
    }
}
