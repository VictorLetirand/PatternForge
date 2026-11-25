//! Barre d'outils, barre d'état, liste des feuilles et inspecteur.

use crate::a11y::ComboExt;
use crate::app::{App, TextBuf};
use crate::editor::{Tool, ToolState};
use crate::palette::pal;
use crate::text::{
    core_error, entity_name, internal_kind_label, internal_target_name, marking_shape_label,
    point_role_label, role_label, skip_reason_label, Localized,
};
use eframe::egui::{self, RichText};
use pf_core::input::{format_deg, format_mm, format_number};
use pf_core::{
    contour, measure, Command, Construction, EntityId, EntityKind, LayerId, OffsetMm, ParamKind,
    PathRole, PointDefinition, PointId, PointRef, PointRole, SegmentEnd, SplineNode,
};
use pf_export::pdf;
use pf_geometry::{CornerStyle, Side};
use pf_i18n::{tr, tr_args};

/// Largeur sous laquelle la barre d'outils passe sur plusieurs lignes (US-16.8, interface agrandie).
const NARROW_TOOLBAR_WIDTH: f32 = 900.0;

/// Champ texte validé à la perte du focus ou sur Entrée. Renvoie le nouveau
/// texte quand il a été validé et qu'il diffère de la valeur actuelle.
pub(crate) fn buffered_edit(
    ui: &mut egui::Ui,
    buf: &mut TextBuf,
    key: u64,
    current: &str,
    width: f32,
) -> Option<String> {
    let mut text = match buf {
        Some((k, t)) if *k == key => t.clone(),
        _ => current.to_string(),
    };
    let resp = crate::a11y::add_text(
        ui,
        egui::TextEdit::singleline(&mut text).desired_width(width),
    );
    if resp.has_focus() || resp.changed() {
        *buf = Some((key, text.clone()));
    }
    if resp.lost_focus() {
        *buf = None;
        if text != current {
            return Some(text);
        }
    }
    None
}

/// Comme [`buffered_edit`], pour un texte sur plusieurs lignes (Entrée = saut de ligne ;
/// la validation se fait à la perte du focus).
fn buffered_multiline(
    ui: &mut egui::Ui,
    buf: &mut TextBuf,
    key: u64,
    current: &str,
    width: f32,
) -> Option<String> {
    let mut text = match buf {
        Some((k, t)) if *k == key => t.clone(),
        _ => current.to_string(),
    };
    let resp = crate::a11y::add_text(
        ui,
        egui::TextEdit::multiline(&mut text)
            .desired_width(width)
            .desired_rows(3),
    );
    if resp.has_focus() || resp.changed() {
        *buf = Some((key, text.clone()));
    }
    if resp.lost_focus() {
        *buf = None;
        if text != current {
            return Some(text);
        }
    }
    None
}

/// Commande qui réécrit la fiche d'une pièce avec les valeurs de `p`.
fn card_command(p: &pf_core::Piece) -> Command {
    Command::SetPieceCard {
        piece: p.id,
        name: p.name.clone(),
        code: p.code.clone(),
        quantity: p.quantity,
        mirrored_pair: p.mirrored_pair,
        material: p.material.clone(),
        category: p.category,
        comment: p.comment.clone(),
    }
}

pub(crate) fn category_label(category: Option<pf_core::PieceCategory>) -> String {
    use pf_core::PieceCategory as C;
    match category {
        None => tr("piece.category.none"),
        Some(C::Fabric) => tr("piece.category.fabric"),
        Some(C::Lining) => tr("piece.category.lining"),
        Some(C::Interfacing) => tr("piece.category.interfacing"),
        Some(C::Leather) => tr("piece.category.leather"),
        Some(C::Reinforcement) => tr("piece.category.reinforcement"),
    }
}

pub(crate) fn issue_text(issue: &pf_core::piece::PieceIssue) -> String {
    use pf_core::piece::PieceIssue as I;
    match issue {
        I::NoName => tr("piece.issue.no_name"),
        I::MissingGrainline => tr("piece.issue.missing_grainline"),
        I::GrainlineMissingEntity => tr("piece.issue.grainline_missing_entity"),
        I::EmptyOutline => tr("piece.issue.empty_outline"),
        I::OutlineOpen => tr("piece.issue.outline_open"),
        I::OutlineUnreadable => tr("piece.issue.outline_unreadable"),
        I::OutlineSelfIntersecting => tr("piece.issue.outline_self_intersecting"),
        I::InternalOutsideOutline { count } => tr_args(
            "piece.issue.internal_outside_outline",
            &[("count", &count.to_string())],
        ),
        I::DartInvalid { count } => {
            tr_args("piece.issue.dart_invalid", &[("count", &count.to_string())])
        }
        I::MarkingOutsideOutline { count } => tr_args(
            "piece.issue.marking_outside_outline",
            &[("count", &count.to_string())],
        ),
        I::ButtonsTooCloseToEdge { count } => tr_args(
            "piece.issue.buttons_too_close_to_edge",
            &[("count", &count.to_string())],
        ),
        I::GrainRotationExceeded => tr("piece.issue.grain_rotation_exceeded"),
        I::DuplicateCode { code } => tr_args("piece.issue.duplicate_code", &[("code", code)]),
        I::DerivedStale { .. } => tr("piece.issue.derived_stale"),
        I::CutContourFailed { reason } => {
            tr_args("piece.issue.cut_contour_failed", &[("reason", reason)])
        }
        I::FoldNotchesFailed { reason } => {
            tr_args("piece.issue.fold_notches_failed", &[("reason", reason)])
        }
        I::CutNotchesFailed { reason } => {
            tr_args("piece.issue.cut_notches_failed", &[("reason", reason)])
        }
        I::CornerNotchesFailed { reason } => {
            tr_args("piece.issue.corner_notches_failed", &[("reason", reason)])
        }
        I::CutDartFailed { reason } => {
            tr_args("piece.issue.cut_dart_failed", &[("reason", reason)])
        }
        I::FoldOutsideContour { count } => tr_args(
            "piece.issue.fold_outside_contour",
            &[("count", &count.to_string())],
        ),
        I::FoldMarginIgnored { count } => tr_args(
            "piece.issue.fold_margin_ignored",
            &[("count", &count.to_string())],
        ),
        I::NetCutWithSeam { hems, folds } => tr_args(
            "piece.issue.net_cut_with_seam",
            &[("hems", &hems.to_string()), ("folds", &folds.to_string())],
        ),
    }
}

/// Forme choisie dans le formulaire « Ajouter un marquage… » (US-06.3).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MarkingKind {
    Punch,
    Drill,
    Cross,
}

/// Formulaire « Ajouter un marquage… » (US-06.3) : état d'interface, jamais enregistré ;
/// l'ajout passe par `Command::AddPieceMarking`.
pub struct MarkingForm {
    pub piece: pf_core::PieceId,
    pub kind: MarkingKind,
    /// Diamètre (perçage) ou côté (croix) saisi, en mm.
    pub size: String,
    /// Bout de pince reculé (deux points sélectionnés) plutôt que sur un point.
    pub dart: bool,
    /// Recul saisi, en mm.
    pub setback: String,
}

/// Formulaire « Créer une pièce dérivée… » (US-04.4) : état d'interface, jamais
/// enregistré ; la création passe par `Command::CreateDerivedPiece`.
pub struct DerivedForm {
    pub mother: pf_core::PieceId,
    pub kind: pf_core::DerivedKind,
    /// Décalage saisi en mm (positif = extérieur, négatif = intérieur).
    pub distance: String,
    pub name: String,
}

fn derived_kind_label(kind: pf_core::DerivedKind) -> String {
    use pf_core::DerivedKind as K;
    match kind {
        K::Facing => tr("piece.derived.kind.facing"),
        K::Lining => tr("piece.derived.kind.lining"),
        K::Interfacing => tr("piece.derived.kind.interfacing"),
        K::Reinforcement => tr("piece.derived.kind.reinforcement"),
        K::Unfolded => tr("piece.derived.kind.unfolded"),
    }
}

/// Paragraphe d'aide d'une fenêtre d'export (PH-2b) : affiché tel quel s'il tient sur deux lignes,
/// sinon replié sous « Notes » (fermé par défaut ; le texte reste intégral, jamais abrégé).
pub(crate) fn note_paragraph(ui: &mut egui::Ui, salt: &str, text: &str) {
    let rich = RichText::new(text).small().color(pal().muted);
    let rows = egui::WidgetText::from(rich.clone())
        .into_galley(
            ui,
            Some(egui::TextWrapMode::Wrap),
            ui.available_width(),
            egui::TextStyle::Body,
        )
        .rows
        .len();
    if rows <= 2 {
        crate::a11y::label(ui, rich);
    } else {
        egui::CollapsingHeader::new(
            RichText::new(tr("export.notes"))
                .small()
                .strong()
                .color(pal().muted),
        )
        .id_salt(("notes", salt))
        .default_open(false)
        .show(ui, |ui| {
            crate::a11y::label(ui, rich);
        });
    }
}

pub(crate) fn section(ui: &mut egui::Ui, title: &str) {
    pf_ui::components::section_header(ui, title);
}

/// Amène l'en-tête d'un bloc replié ouvert par une étape dans la partie visible du panneau, sous le
/// chevron de repli (PH-6, défaut 3) : viser le corps du bloc (`scroll_to_cursor`) sortait son titre
/// de la vue et laissait le chevron recouvrir la première ligne. La demande dure
/// [`BLOCK_VIEW_FRAMES`] images (`start` la lance) : la section parente et le bloc s'ouvrent en
/// s'animant, une seule demande viserait une position qui bouge encore.
pub(crate) fn scroll_block_into_view(
    ui: &mut egui::Ui,
    salt: impl std::hash::Hash,
    header: egui::Rect,
    start: bool,
) {
    let key = egui::Id::new(("block-into-view", salt));
    if start {
        ui.data_mut(|d| d.insert_temp(key, BLOCK_VIEW_FRAMES));
    }
    let left: u8 = ui.data(|d| d.get_temp(key)).unwrap_or(0);
    if left == 0 {
        return;
    }
    ui.data_mut(|d| d.insert_temp(key, left - 1));
    let rect = egui::Rect::from_min_max(
        header.min - egui::vec2(0.0, BLOCK_TOP_CLEARANCE),
        header.max,
    );
    ui.scroll_to_rect(rect, Some(egui::Align::TOP));
    ui.ctx().request_repaint();
}

/// Place laissée au-dessus de l'en-tête d'un bloc amené en vue (hauteur du chevron de repli).
const BLOCK_TOP_CLEARANCE: f32 = 48.0;
/// Nombre d'images pendant lesquelles un bloc ouvert par une étape reste visé.
const BLOCK_VIEW_FRAMES: u8 = 12;

/// Chevron de repli du panneau des feuilles (PH-2c) : vrai au clic. Dessiné (la police n'a pas
/// toutes les flèches) ; nom accessible et info-bulle = l'action (« Réduire » / « Déployer »).
fn fold_chevron(ui: &mut egui::Ui, rect: egui::Rect, folded: bool) -> bool {
    let name = tr(if folded {
        "panel.sheets.unfold"
    } else {
        "panel.sheets.fold"
    });
    let response = ui.interact(rect, ui.id().with("sheets_fold"), egui::Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), name.clone())
    });
    let visuals = ui.style().interact(&response);
    let painter = ui.painter();
    painter.rect_filled(rect, 4.0, visuals.weak_bg_fill);
    painter.rect_stroke(rect, 4.0, visuals.bg_stroke, egui::StrokeKind::Inside);
    // « < » pour réduire, « > » pour déployer.
    let c = rect.center();
    let (tip, tail) = if folded { (3.0, -3.0) } else { (-3.0, 3.0) };
    painter.add(egui::Shape::line(
        vec![
            egui::pos2(c.x + tail, c.y - 5.0),
            egui::pos2(c.x + tip, c.y),
            egui::pos2(c.x + tail, c.y + 5.0),
        ],
        egui::Stroke::new(1.8_f32, visuals.fg_stroke.color),
    ));
    response.on_hover_text(name).clicked()
}

/// Haut des grandes fenêtres centrées (aide, mise en page) : sous le menu et la barre d'outils.
pub(crate) const WINDOW_TOP: f32 = 78.0;

/// Hauteur disponible pour une grande fenêtre centrée, marge basse comprise : la barre d'état
/// reste découverte (PH-6, défaut 2).
pub(crate) fn window_room(ctx: &egui::Context) -> f32 {
    let status = egui::containers::panel::PanelState::load(ctx, egui::Id::new("status"))
        .map_or(28.0, |s| s.rect.height());
    (ctx.screen_rect().height() - WINDOW_TOP - status - 16.0).max(240.0)
}

/// Section repliable : repliée par défaut ; elle s'ouvre quand `linked` (sélection ou état
/// courant) passe à vrai, puis l'utilisateur la replie ou la déplie à volonté. L'état est une
/// préférence d'interface (mémoire egui), jamais écrite dans le projet.
pub(crate) fn fold<R>(
    ui: &mut egui::Ui,
    salt: impl std::hash::Hash,
    title: &str,
    linked: bool,
    body: impl FnOnce(&mut egui::Ui) -> R,
) {
    let id = ui.make_persistent_id(salt);
    let key = id.with("linked");
    let was = ui.data_mut(|d| d.get_temp::<bool>(key).unwrap_or(false));
    ui.data_mut(|d| d.insert_temp(key, linked));
    let mut header =
        egui::CollapsingHeader::new(RichText::new(title).small().strong().color(pal().muted))
            .id_salt(id)
            .default_open(false);
    if linked && !was {
        header = header.open(Some(true));
    }
    header.show(ui, body);
}

#[allow(dead_code)]
fn section_plain(ui: &mut egui::Ui, title: &str) {
    ui.add_space(10.0);
    crate::a11y::label(ui, RichText::new(title).small().strong().color(pal().muted));
}

pub(crate) fn row(ui: &mut egui::Ui, label: &str, value: &str) {
    pf_ui::components::property_row(ui, label, value);
}

#[allow(dead_code)]
fn row_plain(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.horizontal(|ui| {
        crate::a11y::label(ui, RichText::new(label).color(pal().muted));
        crate::a11y::label(ui, RichText::new(value).strong().monospace());
    });
}

/// Modification d'une tangente demandée dans l'inspecteur d'une courbe lisse.
enum TangentEdit {
    Set(OffsetMm),
    Reset,
}

/// Ligne « module + direction » d'une tangente. Renvoie la modification
/// demandée (s'il y en a une) et si la saisie est terminée.
fn tangent_row(
    ui: &mut egui::Ui,
    title: &str,
    t: pf_geometry::VectorMm,
    explicit: bool,
) -> (Option<TangentEdit>, bool) {
    let (mut module, mut angle) = pf_geometry::tangent_to_polar(t);
    let (mut changed, mut done, mut reset) = (false, false, false);
    ui.horizontal(|ui| {
        crate::a11y::label(ui, title);
        let m = ui
            .add(crate::views::length_drag(
                egui::DragValue::new(&mut module)
                    .localized()
                    .speed(0.5)
                    .suffix(" mm")
                    .max_decimals(3)
                    .range(0.0..=1.0e6),
            ))
            .on_hover_text(tr("inspector.spline.module.hover"));
        let a = crate::a11y::add_drag(
            ui,
            egui::DragValue::new(&mut angle)
                .localized()
                .speed(0.5)
                .suffix("°")
                .max_decimals(2),
        );
        changed = m.changed() || a.changed();
        done = m.drag_stopped() || m.lost_focus() || a.drag_stopped() || a.lost_focus();
        if explicit {
            reset = ui
                .small_button(tr("inspector.spline.reset"))
                .on_hover_text(tr("inspector.spline.reset.hover"))
                .clicked();
        } else {
            crate::a11y::label(
                ui,
                RichText::new(tr("inspector.spline.computed"))
                    .small()
                    .color(pal().muted),
            );
        }
    });
    let edit = if reset {
        Some(TangentEdit::Reset)
    } else if changed {
        pf_geometry::tangent_from_polar(module, angle)
            .ok()
            .map(|v| TangentEdit::Set(OffsetMm::from_vector(v)))
    } else {
        None
    };
    (edit, done || reset)
}

/// Lignes d'inspecteur d'un contour fermé : périmètre, aire, alertes.
fn contour_rows(ui: &mut egui::Ui, title: &str, c: &contour::Contour, m: &contour::ContourMeasure) {
    let role = c.role.map(role_label).unwrap_or_else(|| tr("role.mixed"));
    let line = tr_args(
        "inspector.contour.line",
        &[
            ("title", title),
            ("count", &c.edges.len().to_string()),
            ("role", &role),
        ],
    );
    crate::a11y::label(ui, RichText::new(line).strong());
    row(
        ui,
        &tr("inspector.contour.perimeter"),
        &format_mm(m.perimeter_mm),
    );
    row(
        ui,
        &tr("inspector.contour.area"),
        &format!("{} cm²", format_number(m.area_mm2 / 100.0, 2)),
    );
    if m.self_intersecting {
        crate::a11y::label(
            ui,
            RichText::new(tr("inspector.contour.self_intersecting"))
                .small()
                .color(pal().error),
        );
    }
}

/// Famille d'outils de la barre d'outils (présentation seulement : lettres et outils inchangés).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolFamily {
    Draw,
    Construct,
    Modify,
    Piece,
    Measure,
}

impl ToolFamily {
    pub const ALL: [ToolFamily; 5] = [
        ToolFamily::Draw,
        ToolFamily::Construct,
        ToolFamily::Modify,
        ToolFamily::Piece,
        ToolFamily::Measure,
    ];

    /// Outils de la famille, dans l'ordre de `Tool::ALL`. `Tool::Select` reste hors famille.
    pub fn tools(self) -> &'static [Tool] {
        match self {
            ToolFamily::Draw => &[
                Tool::Point,
                Tool::Segment,
                Tool::Rectangle,
                Tool::Curve,
                Tool::Spline,
                Tool::Circle,
                Tool::Arc,
                Tool::Trace,
            ],
            ToolFamily::Construct => &[
                Tool::Perpendicular,
                Tool::Parallel,
                Tool::PointOnSegment,
                Tool::Tangent,
                Tool::Guide,
            ],
            ToolFamily::Modify => &[
                Tool::Fillet,
                Tool::Chamfer,
                Tool::Extend,
                Tool::Split,
                Tool::Merge,
                Tool::Offset,
            ],
            ToolFamily::Piece => &[Tool::Notch, Tool::Grainline, Tool::ExtractPiece, Tool::Dart],
            ToolFamily::Measure => &[Tool::Measure, Tool::Dimension],
        }
    }

    pub fn label(self) -> String {
        tr(match self {
            ToolFamily::Draw => "toolbar.family.draw",
            ToolFamily::Construct => "toolbar.family.construct",
            ToolFamily::Modify => "toolbar.family.modify",
            ToolFamily::Piece => "toolbar.family.piece",
            ToolFamily::Measure => "toolbar.family.measure",
        })
    }
}

impl App {
    pub fn toolbar(&mut self, ctx: &egui::Context) {
        if self.plan.is_some() {
            self.plan_toolbar(ctx);
            return;
        }
        egui::TopBottomPanel::top("tools").show(ctx, |ui| {
            ui.add_space(3.0);
            // Interface agrandie (US-16.8) : trop étroite pour tout sur une ligne, la barre passe
            // à la ligne au lieu de sortir de la fenêtre.
            if ui.available_width() < NARROW_TOOLBAR_WIDTH {
                ui.horizontal_wrapped(|ui| {
                    self.tool_groups(ui);
                    ui.separator();
                    self.history_buttons(ui, false);
                });
                ui.add_space(3.0);
                return;
            }
            ui.horizontal(|ui| {
                // Annuler / Rétablir / Cadrer : à droite, toujours au même endroit.
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    self.history_buttons(ui, true);
                    ui.separator();
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        self.tool_groups(ui);
                    });
                });
            });
            ui.add_space(3.0);
        });
    }

    /// Annuler, rétablir, cadrer ; `right_to_left` : ordre de pose d'une rangée alignée à droite.
    fn history_buttons(&mut self, ui: &mut egui::Ui, right_to_left: bool) {
        let fit = |app: &mut App, ui: &mut egui::Ui| {
            if ui
                .button(tr("toolbar.fit"))
                .on_hover_text(tr("toolbar.fit.hover"))
                .clicked()
            {
                app.fit_pending = true;
            }
        };
        let redo = |app: &mut App, ui: &mut egui::Ui| {
            if ui
                .add_enabled(
                    app.editor.doc.can_redo(),
                    egui::Button::new(tr("menu.edit.redo")),
                )
                .clicked()
            {
                app.editor.redo();
            }
        };
        let undo = |app: &mut App, ui: &mut egui::Ui| {
            if ui
                .add_enabled(
                    app.editor.doc.can_undo(),
                    egui::Button::new(tr("menu.edit.undo")),
                )
                .clicked()
            {
                app.editor.undo();
            }
        };
        if right_to_left {
            fit(self, ui);
            redo(self, ui);
            undo(self, ui);
        } else {
            undo(self, ui);
            redo(self, ui);
            fit(self, ui);
        }
    }

    /// Outils : Sélection seule, puis un menu par famille, puis les modes de l'outil actif.
    fn tool_groups(&mut self, ui: &mut egui::Ui) {
        let tool_btn = |ui: &mut egui::Ui, ed: &mut crate::editor::Editor, tool: Tool| {
            let key = tool.shortcut();
            if pf_ui::components::tool_button_tip(
                ui,
                &tool.label(),
                Some(key.as_str()).filter(|k| !k.is_empty()),
                ed.tool == tool,
                Some(crate::help::tool_gesture(tool).as_str()),
            )
            .clicked()
            {
                ed.set_tool(tool);
                return true;
            }
            false
        };
        tool_btn(ui, &mut self.editor, Tool::Select);
        // Plan en lecture seule (taille active ≠ base, US-09.2a) : outils de dessin grisés.
        let read_only = self.editor.size_view().is_some();
        for family in ToolFamily::ALL {
            let tools = self.layout.visible_tools(family);
            if tools.is_empty() {
                continue;
            }
            let active = family.tools().contains(&self.editor.tool);
            let name = family.label();
            let button = egui::Button::new(RichText::new(&name).strong())
                .selected(active)
                .min_size(egui::vec2(0.0, 28.0));
            let (response, _) = ui
                .add_enabled_ui(!read_only, |ui| {
                    egui::containers::menu::MenuButton::from_button(button).ui(ui, |ui| {
                        for tool in &tools {
                            if tool_btn(ui, &mut self.editor, *tool) {
                                ui.close();
                            }
                        }
                    })
                })
                .inner;
            if read_only {
                response.on_disabled_hover_text(tr("size.readonly"));
            } else {
                response.on_hover_text(tr_args("toolbar.family.hover", &[("family", &name)]));
            }
        }
        // Modes de l'outil actif (cercle, arc, aide, extraction, point).
        if matches!(self.editor.tool, Tool::Circle | Tool::Arc) {
            ui.separator();
            let tool = self.editor.tool;
            for mode in crate::editor::RoundMode::ALL {
                if ui
                    .selectable_label(self.editor.round_mode == mode, mode.label(tool))
                    .clicked()
                {
                    self.editor.set_round_mode(mode);
                }
            }
        }
        if self.editor.tool == Tool::Guide {
            ui.separator();
            for mode in crate::editor::GuideMode::ALL {
                if ui
                    .selectable_label(self.editor.guide_mode == mode, mode.label())
                    .clicked()
                {
                    self.editor.set_guide_mode(mode);
                }
            }
        }
        if self.editor.tool == Tool::ExtractPiece {
            ui.separator();
            ui.checkbox(&mut self.editor.piece_detached, tr("piece.detached"))
                .on_hover_text(tr("piece.detached.hover"));
            if let Some(r) = self
                .editor
                .project()
                .naming_rule_for(pf_core::NamingTarget::Piece)
            {
                let text = tr_args("naming.use", &[("name", &r.name)]);
                ui.checkbox(&mut self.editor.piece_use_rule, text)
                    .on_hover_text(tr("naming.use.hover"));
            }
        }
        if self.editor.tool == Tool::Point {
            ui.separator();
            for mode in crate::editor::PointMode::ALL {
                if ui
                    .selectable_label(self.editor.point_mode == mode, mode.label())
                    .clicked()
                {
                    self.editor.set_point_mode(mode);
                }
            }
        }
    }

    pub fn status_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                let tool = tr_args("status.tool", &[("tool", &self.editor.tool.label())]);
                crate::a11y::label(ui, RichText::new(tool).strong());
                ui.separator();
                let c = self.editor.cursor;
                crate::a11y::label(
                    ui,
                    RichText::new(format!(
                        "X {:>8}   Y {:>8} {}",
                        crate::views::cursor_text(c.x),
                        crate::views::cursor_text(c.y),
                        pf_core::units::display_units().unit.suffix()
                    ))
                    .monospace(),
                );
                ui.separator();
                let zoom = format_number(self.view.zoom_percent(), 0);
                let zero = format!("{}0", pf_core::input::decimal_separator());
                let zoom = zoom
                    .strip_suffix(zero.as_str())
                    .unwrap_or(&zoom)
                    .to_string();
                crate::a11y::label(ui, tr_args("status.zoom", &[("value", &zoom)]));
                // Taille active (US-09.2a) : seulement quand le projet a des gammes.
                self.size_selector(ui);
                ui.separator();
                let status = &self.editor.status;
                if status.error {
                    crate::a11y::label(
                        ui,
                        RichText::new(format!("⚠ {}", status.text))
                            .color(pal().error)
                            .strong(),
                    );
                } else {
                    crate::a11y::label(ui, &status.text);
                }
            });
        });
    }

    pub fn sheets_panel(&mut self, ctx: &egui::Context) {
        // Plan de pièces (PH-2b) : le panneau n'a pas d'objet, il n'est pas affiché. Sa largeur,
        // son défilement et ses sections ouvertes restent en mémoire egui : retour = même état.
        if self.plan.is_some() {
            return;
        }
        // Bande étroite (PH-2c) : repli par le chevron, ou automatique quand un dock ouvert
        // laisserait moins de 400 points au canevas. Autre identifiant que le panneau déployé :
        // sa largeur (réglée à la main) est retrouvée au déploiement.
        let mut toggle = false;
        if self.sheets_folded(ctx) {
            egui::SidePanel::left("sheets_folded")
                .resizable(false)
                .exact_width(crate::dock::FOLDED_WIDTH)
                .show(ctx, |ui| {
                    let rect =
                        egui::Rect::from_min_size(ui.max_rect().left_top(), egui::vec2(22.0, 22.0));
                    toggle = fold_chevron(ui, rect, true);
                });
            if toggle {
                self.toggle_sheets_fold(ctx);
            }
            return;
        }
        egui::SidePanel::left("sheets")
            .resizable(true)
            .default_width(210.0)
            .min_width(170.0)
            .max_width(260.0)
            .show(ctx, |ui| {
                // Un seul défilement pour tout le panneau (projet, feuilles, fiches, calques) :
                // rien ne sort de l'écran, et la gouttière ne recouvre pas le texte.
                ui.spacing_mut().scroll = egui::style::ScrollStyle::solid();
                let mut scroll = egui::ScrollArea::vertical()
                    .id_salt("pieces_list")
                    .auto_shrink([false, false]);
                if let Some(offset) = self.capture_scroll {
                    scroll = scroll.vertical_scroll_offset(offset);
                }
                // Défilement automatique (étapes « Coins », « Ourlet »…) : posé d'un coup, pas
                // animé, sinon la capture (8e image) tombe au milieu du mouvement et rogne la
                // ligne du haut (défaut 14).
                if self.corners_focus
                    || self.stitch_open_once
                    || self.pitch_open_once
                    || self.zone_open_once
                {
                    scroll = scroll.animated(false);
                }
                scroll.show(ui, |ui| self.sheets_content(ui));
                // Chevron dans l'angle haut droit (au-dessus du défilement, hors de la gouttière).
                let area = ui.max_rect();
                let rect = egui::Rect::from_min_size(
                    egui::pos2(area.right() - 32.0, area.top()),
                    egui::vec2(22.0, 22.0),
                );
                toggle = fold_chevron(ui, rect, false);
            });
        if toggle {
            self.toggle_sheets_fold(ctx);
        }
    }

    fn sheets_content(&mut self, ui: &mut egui::Ui) {
        {
            {
                section(ui, &tr("panel.project.title"));
                let name = self.editor.project().name.clone();
                if let Some(new) = buffered_edit(ui, &mut self.text_buf, 1, &name, f32::INFINITY) {
                    self.editor.exec(Command::RenameProject { name: new });
                }

                section(ui, &tr("panel.sheets.title"));
                let sheets: Vec<_> = self
                    .editor
                    .project()
                    .sheets
                    .iter()
                    .map(|s| {
                        let has_piece = !self.editor.project().pieces_on(s.id).is_empty();
                        (s.id, s.name.clone(), has_piece, s.entities.len())
                    })
                    .collect();
                let mut commit_rename = None;
                egui::ScrollArea::vertical()
                    .max_height(ui.available_height() - 400.0)
                    .show(ui, |ui| {
                        for (id, name, is_piece, count) in &sheets {
                            if let Some((rid, text)) = &mut self.renaming {
                                if rid == id {
                                    let resp = crate::a11y::add_text(
                                        ui,
                                        egui::TextEdit::singleline(text)
                                            .desired_width(f32::INFINITY),
                                    );
                                    if self.rename_focus {
                                        resp.request_focus();
                                        self.rename_focus = false;
                                    }
                                    if resp.lost_focus() {
                                        commit_rename = Some((*id, text.clone()));
                                    }
                                    continue;
                                }
                            }
                            let label = if *is_piece {
                                tr_args("sheet.piece_suffix", &[("name", name)])
                            } else {
                                name.clone()
                            };
                            let count = count.to_string();
                            let hover = if *is_piece {
                                tr_args("sheet.hover_piece", &[("count", &count)])
                            } else {
                                tr_args("sheet.hover", &[("count", &count)])
                            };
                            let resp = ui
                                .selectable_label(self.editor.sheet == *id, label)
                                .on_hover_text(hover);
                            if resp.clicked() {
                                self.leave_plan();
                                self.editor.set_sheet(*id);
                                self.fit_pending = true;
                            }
                            if resp.double_clicked() {
                                self.renaming = Some((*id, name.clone()));
                                self.rename_focus = true;
                            }
                        }
                    });
                if let Some((id, name)) = commit_rename {
                    // Échap a déjà vidé `renaming` : dans ce cas on ne valide pas.
                    if self.renaming.take().is_some() {
                        let current = sheets.iter().find(|s| s.0 == id).map(|s| s.1.as_str());
                        if current != Some(name.as_str()) {
                            self.editor.exec(Command::RenameSheet { sheet: id, name });
                        }
                    }
                }

                ui.add_space(6.0);
                if let Some(r) = self
                    .editor
                    .project()
                    .naming_rule_for(pf_core::NamingTarget::Sheet)
                {
                    let text = tr_args("naming.use", &[("name", &r.name)]);
                    ui.checkbox(&mut self.editor.sheet_use_rule, text)
                        .on_hover_text(tr("naming.use.hover"));
                }
                if ui.button(tr("sheet.new")).clicked() {
                    self.editor.add_sheet();
                    self.fit_pending = true;
                    let s = self.editor.sheet();
                    self.renaming = Some((s.id, s.name.clone()));
                    self.rename_focus = true;
                }
                let current = self.editor.sheet;
                let index = sheets.iter().position(|s| s.0 == current).unwrap_or(0);
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .add_enabled(index > 0, egui::Button::new("⏶"))
                        .on_hover_text(tr("sheet.move_up"))
                        .clicked()
                    {
                        self.editor.exec(Command::MoveSheet {
                            sheet: current,
                            new_index: index - 1,
                        });
                    }
                    if ui
                        .add_enabled(index + 1 < sheets.len(), egui::Button::new("⏷"))
                        .on_hover_text(tr("sheet.move_down"))
                        .clicked()
                    {
                        self.editor.exec(Command::MoveSheet {
                            sheet: current,
                            new_index: index + 1,
                        });
                    }
                    if ui.button(tr("sheet.rename")).clicked() {
                        self.renaming = Some((current, sheets[index].1.clone()));
                        self.rename_focus = true;
                    }
                    if ui
                        .button(tr("sheet.duplicate"))
                        .on_hover_text(tr("sheet.duplicate.hover"))
                        .clicked()
                    {
                        self.editor.duplicate_sheet(current);
                        self.fit_pending = true;
                    }
                    if ui
                        .add_enabled(sheets.len() > 1, egui::Button::new(tr("sheet.delete")))
                        .on_hover_text(tr("sheet.delete.hover"))
                        .clicked()
                    {
                        self.editor.delete_sheet(current);
                        self.fit_pending = true;
                    }
                });

                self.piece_section(ui);
                self.layers_section(ui);
                ui.add_space(16.0);
            }
        }
    }

    /// Panneau « Calques » : liste (visibilité, verrou, calque actif), ajout,
    /// renommage, ordre, suppression, déplacement de la sélection.
    fn layers_section(&mut self, ui: &mut egui::Ui) {
        section(ui, &tr("layers.title"));
        let sheet = self.editor.sheet();
        let rows: Vec<(LayerId, String, bool, bool, usize, usize)> = sheet
            .layers
            .iter()
            .map(|l| {
                let points = sheet
                    .points
                    .iter()
                    .filter(|p| p.layer == Some(l.id))
                    .count();
                let paths = sheet
                    .entities
                    .iter()
                    .filter(|e| e.layer == Some(l.id))
                    .count();
                (l.id, l.name.clone(), l.visible, l.locked, points, paths)
            })
            .collect();
        let active = self.editor.current_layer();
        let mut new_active: Option<Option<LayerId>> = None;
        let mut commands: Vec<Command> = Vec::new();
        let sheet_id = self.editor.sheet;
        egui::ScrollArea::vertical()
            .id_salt("layers_list")
            .max_height(110.0)
            .show(ui, |ui| {
                if ui
                    .selectable_label(active.is_none(), tr("layers.default"))
                    .clicked()
                {
                    new_active = Some(None);
                }
                for (id, name, visible, locked, points, paths) in &rows {
                    ui.horizontal(|ui| {
                        if ui
                            .selectable_label(*visible, tr("layers.show"))
                            .on_hover_text(tr("layers.show.hover"))
                            .clicked()
                        {
                            commands.push(Command::SetLayerVisible {
                                sheet: sheet_id,
                                layer: *id,
                                visible: !*visible,
                            });
                        }
                        if ui
                            .selectable_label(*locked, tr("layers.lock"))
                            .on_hover_text(tr("layers.lock.hover"))
                            .clicked()
                        {
                            commands.push(Command::SetLayerLocked {
                                sheet: sheet_id,
                                layer: *id,
                                locked: !*locked,
                            });
                        }
                        let hover = tr_args(
                            "layers.row.hover",
                            &[
                                ("points", &points.to_string()),
                                ("paths", &paths.to_string()),
                            ],
                        );
                        if ui
                            .selectable_label(active == Some(*id), name)
                            .on_hover_text(hover)
                            .clicked()
                        {
                            new_active = Some(Some(*id));
                        }
                    });
                }
            });
        ui.horizontal(|ui| {
            if ui.button(tr("layers.new")).clicked() {
                let layers = &self.editor.sheet().layers;
                let name = (layers.len() + 1..)
                    .map(|n| tr_args("layers.default_name", &[("n", &n.to_string())]))
                    .find(|n| !layers.iter().any(|l| &l.name == n))
                    .unwrap_or_default();
                if let Some(out) = self.editor.exec(Command::AddLayer {
                    sheet: sheet_id,
                    name,
                }) {
                    new_active = Some(out.layers.first().copied());
                }
            }
            if let Some(layer) = active {
                let index = rows.iter().position(|r| r.0 == layer).unwrap_or(0);
                if ui
                    .add_enabled(index > 0, egui::Button::new("⏶"))
                    .on_hover_text(tr("layers.move_up"))
                    .clicked()
                {
                    commands.push(Command::MoveLayer {
                        sheet: sheet_id,
                        layer,
                        new_index: index - 1,
                    });
                }
                if ui
                    .add_enabled(index + 1 < rows.len(), egui::Button::new("⏷"))
                    .on_hover_text(tr("layers.move_down"))
                    .clicked()
                {
                    commands.push(Command::MoveLayer {
                        sheet: sheet_id,
                        layer,
                        new_index: index + 1,
                    });
                }
                if ui
                    .button(tr("layers.delete"))
                    .on_hover_text(tr("layers.delete.hover"))
                    .clicked()
                {
                    commands.push(Command::DeleteLayer {
                        sheet: sheet_id,
                        layer,
                        move_to: None,
                    });
                }
            }
        });
        if let Some(layer) = active {
            let name = rows
                .iter()
                .find(|r| r.0 == layer)
                .map(|r| r.1.clone())
                .unwrap_or_default();
            ui.horizontal(|ui| {
                crate::a11y::label(ui, tr("layers.name"));
                let key = (layer.0 << 8) | 7;
                if let Some(new) = buffered_edit(ui, &mut self.text_buf, key, &name, 110.0) {
                    commands.push(Command::RenameLayer {
                        sheet: sheet_id,
                        layer,
                        name: new,
                    });
                }
            });
        }
        let has_selection =
            !self.editor.selection.points.is_empty() || !self.editor.selection.entities.is_empty();
        if ui
            .add_enabled(
                has_selection,
                egui::Button::new(tr("layers.move_selection")),
            )
            .on_hover_text(tr("layers.move_selection.hover"))
            .clicked()
        {
            self.editor.move_selection_to_active_layer();
        }
        if let Some(a) = new_active {
            self.editor.active_layer = a;
        }
        let changed = !commands.is_empty();
        for c in commands {
            self.editor.exec(c);
        }
        if changed {
            self.editor.prune_selection();
        }
    }

    fn piece_section(&mut self, ui: &mut egui::Ui) {
        section(ui, &tr("piece.title"));
        let sheet = self.editor.sheet;
        // Pièces dont le contour est sur cette feuille (liées, ou copies détachées).
        let pieces: Vec<_> = self
            .editor
            .project()
            .pieces_on(sheet)
            .into_iter()
            .cloned()
            .collect();
        if pieces.is_empty() {
            let mut is_piece = false;
            if ui.checkbox(&mut is_piece, tr("piece.enable")).changed() {
                self.editor.exec(Command::AddPiece {
                    sheet,
                    name: self.editor.sheet().name.clone(),
                    quantity: 1,
                    material: None,
                    mirrored_pair: false,
                });
            }
            return;
        }
        let flagged = pieces
            .iter()
            .filter(|p| !self.editor.view_project().piece_issues(p.id).is_empty())
            .count();
        crate::a11y::label(
            ui,
            RichText::new(tr_args(
                "piece.summary",
                &[
                    ("count", &pieces.len().to_string()),
                    ("warnings", &flagged.to_string()),
                ],
            ))
            .small()
            .color(pal().muted),
        );
        for (i, piece) in pieces.into_iter().enumerate() {
            if i > 0 {
                ui.separator();
            }
            self.piece_block(ui, piece);
        }
    }

    /// Fiche d'une pièce : nom, quantité, matière, lien, et mesures **observées**
    /// de son contour (recalculées d'après la construction, jamais enregistrées).
    fn piece_block(&mut self, ui: &mut egui::Ui, piece: pf_core::Piece) {
        {
            let key = piece.id.0 << 8;
            let issues = self.editor.view_project().piece_issues(piece.id);
            let material = piece.material.clone().unwrap_or_default();
            let code = piece.code.clone().unwrap_or_default();
            let comment = piece.comment.clone().unwrap_or_default();
            // Chaque champ validé = une commande = une étape d'annulation.
            let mut next = piece.clone();
            let mut changed = false;
            egui::Grid::new(("piece", piece.id.0))
                .num_columns(2)
                .show(ui, |ui| {
                    crate::a11y::label(ui, tr("piece.name"));
                    if let Some(name) =
                        buffered_edit(ui, &mut self.text_buf, key | 2, &piece.name, 110.0)
                    {
                        next.name = name;
                        changed = true;
                    }
                    ui.end_row();
                    crate::a11y::label(ui, tr("piece.code"));
                    if let Some(code) = buffered_edit(ui, &mut self.text_buf, key | 4, &code, 110.0)
                    {
                        next.code = Some(code);
                        changed = true;
                    }
                    ui.end_row();
                    crate::a11y::label(ui, tr("piece.quantity"));
                    if let Some(text) = buffered_edit(
                        ui,
                        &mut self.text_buf,
                        key | 5,
                        &piece.quantity.to_string(),
                        50.0,
                    ) {
                        match text.trim().parse::<u32>() {
                            Ok(q) if q >= 1 => {
                                next.quantity = q;
                                changed = true;
                            }
                            _ => self.editor.error(tr("piece.quantity.invalid")),
                        }
                    }
                    ui.end_row();
                    crate::a11y::label(ui, "");
                    let mut pair = piece.mirrored_pair;
                    if ui
                        .checkbox(&mut pair, tr("piece.pair"))
                        .on_hover_text(tr("piece.pair.hover"))
                        .changed()
                    {
                        next.mirrored_pair = pair;
                        changed = true;
                    }
                    ui.end_row();
                    crate::a11y::label(ui, tr("piece.material"));
                    if let Some(material) =
                        buffered_edit(ui, &mut self.text_buf, key | 3, &material, 110.0)
                    {
                        next.material = Some(material);
                        changed = true;
                    }
                    ui.end_row();
                    crate::a11y::label(ui, tr("piece.category"));
                    let mut choice = piece.category;
                    egui::ComboBox::from_id_salt(("piece-category", piece.id.0))
                        .selected_text(category_label(piece.category))
                        .show_ui_named(ui, |ui| {
                            ui.selectable_value(&mut choice, None, category_label(None));
                            for c in pf_core::PieceCategory::ALL {
                                ui.selectable_value(&mut choice, Some(c), category_label(Some(c)));
                            }
                        });
                    if choice != piece.category {
                        next.category = choice;
                        changed = true;
                    }
                    ui.end_row();
                    crate::a11y::label(ui, tr("piece.comment"));
                    if let Some(text) =
                        buffered_multiline(ui, &mut self.text_buf, key | 6, &comment, 160.0)
                    {
                        next.comment = Some(text);
                        changed = true;
                    }
                    ui.end_row();
                });
            if changed {
                self.editor.exec(card_command(&next));
            }
            // Avertissements : texte et symbole, jamais la couleur seule.
            for issue in &issues {
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
            if issues.contains(&pf_core::piece::PieceIssue::MissingGrainline) {
                let candidate = match self.editor.selection.entities.as_slice() {
                    [e] if self
                        .editor
                        .sheet()
                        .entity(*e)
                        .is_ok_and(|x| x.role == PathRole::Grainline) =>
                    {
                        Some(*e)
                    }
                    _ => None,
                };
                if ui
                    .add_enabled(
                        candidate.is_some(),
                        egui::Button::new(tr("piece.grainline.assign")),
                    )
                    .on_hover_text(tr("piece.grainline.assign.hover"))
                    .clicked()
                {
                    self.editor.exec(Command::SetPieceOutline {
                        piece: piece.id,
                        outline: piece.outline.clone(),
                        grainline: candidate,
                    });
                }
            }
            // « Sans droit-fil » (PH-5d, cuir) : près de l'état du droit-fil ; grisée tant que la
            // pièce a un droit-fil. L'état est une case cochée (texte), jamais une couleur.
            let mut no_grain = piece.no_grain;
            if ui
                .add_enabled(
                    piece.grainline.is_none(),
                    egui::Checkbox::new(&mut no_grain, tr("piece.nograin.toggle")),
                )
                .on_hover_text(tr("piece.nograin.hover"))
                .changed()
            {
                self.editor.exec(Command::SetPieceNoGrain {
                    piece: piece.id,
                    no_grain,
                });
            }
            // Marge de couture : repliée par défaut, avant les sections longues.
            self.piece_seam_section(ui, &piece);
            // Sections repliables, repliées par défaut ; une section s'ouvre quand la sélection
            // (ou l'état courant) la concerne. L'état replié est une préférence d'interface.
            let sel = &self.editor.selection;
            let (has_paths, has_points) = (!sel.entities.is_empty(), !sel.points.is_empty());
            let grain_selected = piece.grainline.is_some()
                && sel.entities.iter().any(|e| {
                    self.editor
                        .sheet()
                        .entity(*e)
                        .is_ok_and(|x| x.role == PathRole::Grainline)
                });
            let derived_open = piece.derived.is_some()
                || self
                    .derived_form
                    .as_ref()
                    .is_some_and(|f| f.mother == piece.id);
            let salt = piece.id.0;
            if piece.grainline.is_some() {
                fold(
                    ui,
                    ("fold-grain", salt),
                    &tr("piece.grain.title"),
                    grain_selected,
                    |ui| self.piece_grain_section(ui, &piece),
                );
            }
            fold(
                ui,
                ("fold-internal", salt),
                &tr("piece.internal.title"),
                has_paths,
                |ui| self.piece_internal_section(ui, &piece),
            );
            fold(
                ui,
                ("fold-marking", salt),
                &tr("piece.marking.title"),
                // PH-6 : un bloc ouvert par une étape (zones : bord sélectionné, pas de point)
                // ouvre aussi sa section, sinon il n'est jamais dessiné.
                has_points || self.stitch_open_once || self.pitch_open_once || self.zone_open_once,
                |ui| self.piece_marking_section(ui, &piece),
            );
            fold(
                ui,
                ("fold-buttons", salt),
                &tr("piece.buttons.title"),
                has_points,
                |ui| self.piece_buttons_section(ui, &piece),
            );
            if !piece.outline.is_empty() {
                fold(
                    ui,
                    ("fold-topstitch", salt),
                    &tr("topstitch.piece.title"),
                    has_paths,
                    |ui| self.piece_topstitch_section(ui, &piece),
                );
            }
            fold(
                ui,
                ("fold-derived", salt),
                &tr("piece.derived.title"),
                derived_open,
                |ui| self.piece_derived_section(ui, &piece),
            );
            // Lien avec la construction.
            let link = match piece.link {
                pf_core::PieceLink::Linked { .. } => tr("piece.link.linked"),
                pf_core::PieceLink::Detached => tr("piece.link.detached"),
            };
            crate::a11y::label(ui, RichText::new(link).small().color(pal().muted));
            // Mesures observées, recalculées à chaque affichage.
            match self.editor.view_project().piece_contour(piece.id) {
                Ok(c) => {
                    crate::a11y::label(
                        ui,
                        RichText::new(tr("piece.measure.title"))
                            .small()
                            .strong()
                            .color(pal().muted),
                    );
                    row(
                        ui,
                        &tr("piece.measure.area"),
                        &format!("{} cm²", format_number(c.area_mm2 / 100.0, 2)),
                    );
                    row(
                        ui,
                        &tr("piece.measure.perimeter"),
                        &format_mm(c.perimeter_mm),
                    );
                    row(
                        ui,
                        &tr("piece.measure.orientation"),
                        &if c.clockwise {
                            tr("piece.orientation.cw")
                        } else {
                            tr("piece.orientation.ccw")
                        },
                    );
                    row(ui, &tr("piece.measure.edges"), &c.edges.len().to_string());
                    if !c.closed {
                        crate::a11y::label(
                            ui,
                            RichText::new(tr("piece.contour.not_closed"))
                                .small()
                                .color(pal().error),
                        );
                    }
                    if c.self_intersecting {
                        crate::a11y::label(
                            ui,
                            RichText::new(tr("inspector.contour.self_intersecting"))
                                .small()
                                .color(pal().error),
                        );
                    }
                }
                Err(_) => {
                    crate::a11y::label(
                        ui,
                        RichText::new(tr("piece.contour.empty"))
                            .small()
                            .color(pal().muted),
                    );
                }
            }
            // Séparer / assembler (US-04.6) : pièces dérivées liées refusées (cassez le lien).
            let usable = piece.derived.is_none() && !piece.outline.is_empty();
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add_enabled(usable, egui::Button::new(tr("piece.split")))
                    .on_hover_text(tr("piece.split.hover"))
                    .clicked()
                {
                    self.editor.begin_split(piece.id);
                }
                if ui
                    .add_enabled(usable, egui::Button::new(tr("piece.join")))
                    .on_hover_text(tr("piece.join.hover"))
                    .clicked()
                {
                    self.open_assemble(Some(piece.id));
                }
                if ui
                    .add_enabled(usable, egui::Button::new(tr("piece.unfold_piece")))
                    .on_hover_text(tr("piece.unfold_piece.hover"))
                    .clicked()
                {
                    self.open_unfold(Some(piece.id));
                }
            });
            if ui.button(tr("piece.delete")).clicked() {
                self.editor.exec(Command::DeletePiece { piece: piece.id });
            }
        }
    }

    /// Pièces dérivées (US-04.4) : lien de cette pièce avec sa mère (état, casser le
    /// lien), liste des pièces qui en dérivent et formulaire de création. Chaque
    /// action est une commande, donc une étape d'annulation.
    fn piece_derived_section(&mut self, ui: &mut egui::Ui, piece: &pf_core::Piece) {
        use pf_core::DerivedKind;
        let project = self.editor.project();
        let info = project.piece_derivation(piece.id);
        let rows: Vec<String> = project
            .derived_pieces_of(piece.id)
            .into_iter()
            .filter_map(|id| {
                let d = project.piece(id).ok()?;
                let spec = d.derived.as_ref()?;
                let area = project.piece_contour(id).map_or_else(
                    |_| "—".to_string(),
                    |c| format_number(c.area_mm2 / 100.0, 2),
                );
                let row = match spec.operation.distance_mm() {
                    Some(distance) => pf_i18n::tr_len(
                        "piece.derived.row",
                        &[
                            ("name", &d.name),
                            ("kind", &derived_kind_label(spec.kind).to_lowercase()),
                            ("distance", &pf_core::units::len_number(distance, 2)),
                            ("area", &area),
                        ],
                    ),
                    None => tr_args(
                        "piece.derived.row_unfold",
                        &[("name", &d.name), ("area", &area)],
                    ),
                };
                Some(if spec.stale.is_some() {
                    tr_args("piece.derived.row.stale", &[("row", &row)])
                } else {
                    row
                })
            })
            .collect();
        ui.add_space(4.0);
        if let Some(info) = &info {
            let header = match info.operation.distance_mm() {
                Some(distance) => pf_i18n::tr_len(
                    "piece.derived.of",
                    &[
                        ("mother", &info.mother_name),
                        ("kind", &derived_kind_label(info.kind).to_lowercase()),
                        ("distance", &pf_core::units::len_number(distance, 2)),
                    ],
                ),
                None => tr_args("piece.derived.of_unfold", &[("mother", &info.mother_name)]),
            };
            crate::a11y::label(ui, RichText::new(header).strong());
            match &info.state {
                pf_core::DerivedState::Current => {
                    crate::a11y::label(
                        ui,
                        RichText::new(tr("piece.derived.state.current"))
                            .small()
                            .color(pal().muted),
                    );
                }
                pf_core::DerivedState::Stale { reason } => {
                    crate::a11y::label(
                        ui,
                        RichText::new(tr_args("piece.derived.state.stale", &[("reason", reason)]))
                            .small()
                            .color(ui.visuals().warn_fg_color),
                    );
                }
            }
            crate::a11y::label(
                ui,
                RichText::new(tr("piece.derived.locked"))
                    .small()
                    .color(pal().muted),
            );
            if ui
                .button(tr("piece.derived.break"))
                .on_hover_text(tr("piece.derived.break.hover"))
                .clicked()
            {
                self.editor
                    .exec(Command::BreakDerivedLink { piece: piece.id });
            }
        }
        if !rows.is_empty() {
            for row in rows {
                crate::a11y::label(ui, row);
            }
        }
        let open = self
            .derived_form
            .as_ref()
            .is_some_and(|f| f.mother == piece.id);
        if !open {
            if ui
                .add_enabled(
                    !piece.outline.is_empty(),
                    egui::Button::new(tr("piece.derived.create")),
                )
                .on_hover_text(tr("piece.derived.create.hover"))
                .clicked()
            {
                self.derived_form = Some(DerivedForm {
                    mother: piece.id,
                    kind: DerivedKind::Lining,
                    distance: "-10".into(),
                    name: String::new(),
                });
            }
            return;
        }
        let Some(mut form) = self.derived_form.take() else {
            return;
        };
        let default_name = format!(
            "{} – {}",
            piece.name,
            derived_kind_label(form.kind).to_lowercase()
        );
        egui::Grid::new(("derived-form", piece.id.0))
            .num_columns(2)
            .show(ui, |ui| {
                crate::a11y::label(ui, tr("piece.derived.form.kind"));
                egui::ComboBox::from_id_salt(("derived-kind", piece.id.0))
                    .selected_text(derived_kind_label(form.kind))
                    .show_ui_named(ui, |ui| {
                        for k in DerivedKind::ALL {
                            ui.selectable_value(&mut form.kind, k, derived_kind_label(k));
                        }
                    });
                ui.end_row();
                crate::a11y::label(ui, tr("piece.derived.form.distance"));
                crate::a11y::add_text(
                    ui,
                    egui::TextEdit::singleline(&mut form.distance).desired_width(80.0),
                )
                .on_hover_text(tr("piece.derived.form.distance.hover"));
                ui.end_row();
                crate::a11y::label(ui, tr("piece.derived.form.name"));
                crate::a11y::add_text(
                    ui,
                    egui::TextEdit::singleline(&mut form.name)
                        .hint_text(default_name.clone())
                        .desired_width(140.0),
                );
                ui.end_row();
            });
        crate::a11y::label(
            ui,
            RichText::new(tr("piece.derived.form.note"))
                .small()
                .color(pal().muted),
        );
        let (mut create, mut cancel) = (false, false);
        ui.horizontal(|ui| {
            create = ui.button(tr("piece.derived.form.ok")).clicked();
            cancel = ui.button(tr("piece.derived.form.cancel")).clicked();
        });
        if cancel {
            return;
        }
        if create {
            match pf_core::units::parse_length_display(&form.distance) {
                Err(_) => self.editor.error(tr("piece.derived.invalid_distance")),
                Ok(distance_mm) => {
                    let name = match form.name.trim() {
                        "" => default_name,
                        n => n.to_string(),
                    };
                    let done = self.editor.exec(Command::CreateDerivedPiece {
                        mother: piece.id,
                        kind: form.kind,
                        operation: pf_core::DerivedOp::Offset { distance_mm },
                        name: Some(name.clone()),
                    });
                    if let Some(out) = done {
                        let sheet = out
                            .sheet
                            .and_then(|s| self.editor.project().sheet(s).ok())
                            .map(|s| s.name.clone())
                            .unwrap_or_default();
                        if out.notes.is_empty() {
                            self.editor.info(tr_args(
                                "piece.derived.created",
                                &[("name", &name), ("sheet", &sheet)],
                            ));
                        }
                        return;
                    }
                }
            }
        }
        self.derived_form = Some(form);
    }

    /// Éléments internes d'une pièce (US-04.3) : liste (libellé, type, longueur ou
    /// position, retrait), import depuis la construction avec son rapport, ajout de
    /// la sélection. Chaque action est une commande, donc une étape d'annulation.
    fn piece_internal_section(&mut self, ui: &mut egui::Ui, piece: &pf_core::Piece) {
        use pf_core::piece_internal::InternalGeometry as G;
        use pf_core::{InternalElement, InternalKind, InternalTarget};
        struct Row {
            element: InternalElement,
            text: String,
            outside: bool,
        }
        let (rows, counts, report_lines, selection_ok) = {
            let project = self.editor.project();
            let sheet = project.piece_sheet(piece.id).ok();
            let rows: Vec<Row> = project
                .piece_internals(piece.id)
                .into_iter()
                .map(|r| {
                    let name = match (sheet, r.element.target) {
                        (Some(s), InternalTarget::Notch { notch }) => s
                            .notch(notch)
                            .map(|n| entity_name(s, n.entity))
                            .unwrap_or_default(),
                        (Some(s), t) => internal_target_name(s, t),
                        (None, _) => r.name.clone(),
                    };
                    let text = match r.geometry {
                        G::Path { length_mm } => tr_args(
                            "piece.internal.row.path",
                            &[("name", &name), ("length", &format_mm(length_mm))],
                        ),
                        G::Position { at } => tr_args(
                            if matches!(r.element.target, InternalTarget::Notch { .. }) {
                                "piece.internal.row.notch"
                            } else {
                                "piece.internal.row.point"
                            },
                            &[
                                ("name", &name),
                                ("x", &format_mm(at.x)),
                                ("y", &format_mm(at.y)),
                            ],
                        ),
                        G::Unreadable => {
                            tr_args("piece.internal.row.unreadable", &[("name", &name)])
                        }
                    };
                    Row {
                        element: r.element,
                        text,
                        outside: r.inside_outline == Some(false),
                    }
                })
                .collect();
            let report_lines: Vec<String> = match (&self.internal_report, sheet) {
                (Some((id, rep)), Some(s)) if *id == piece.id => {
                    const MAX_LINES: usize = 12;
                    let mut lines = vec![tr_args(
                        "piece.internal.report",
                        &[
                            ("added", &rep.added.len().to_string()),
                            ("skipped", &rep.skipped.len().to_string()),
                            ("already", &rep.already.to_string()),
                        ],
                    )];
                    let all: Vec<String> = rep
                        .added
                        .iter()
                        .map(|e| {
                            tr_args(
                                "piece.internal.report.added",
                                &[
                                    ("name", &internal_target_name(s, e.target)),
                                    ("kind", &internal_kind_label(e.kind)),
                                ],
                            )
                        })
                        .chain(rep.skipped.iter().map(|(t, why)| {
                            tr_args(
                                "piece.internal.report.skipped",
                                &[
                                    ("name", &internal_target_name(s, *t)),
                                    ("reason", &skip_reason_label(*why)),
                                ],
                            )
                        }))
                        .collect();
                    let hidden = all.len().saturating_sub(MAX_LINES);
                    lines.extend(all.into_iter().take(MAX_LINES));
                    if hidden > 0 {
                        lines.push(tr_args(
                            "piece.internal.report.more",
                            &[("count", &hidden.to_string())],
                        ));
                    }
                    lines
                }
                _ => Vec::new(),
            };
            // La sélection devient des éléments internes si chaque objet est admissible.
            let sel = &self.editor.selection;
            let taken = |t: InternalTarget| piece.internal.iter().any(|e| e.target == t);
            let mut commands = Vec::new();
            let mut valid = true;
            let add = |target, kind| Command::AddPieceInternal {
                piece: piece.id,
                target,
                kind,
            };
            for &e in &sel.entities {
                let t = InternalTarget::Path { entity: e };
                valid &= !taken(t)
                    && !piece.outline.iter().any(|x| x.entity == e)
                    && piece.grainline != Some(e);
                let role = project
                    .sheet(self.editor.sheet)
                    .ok()
                    .and_then(|s| s.entity(e).ok().map(|x| x.role))
                    .unwrap_or_default();
                commands.push(add(t, InternalKind::default_for_role(role)));
            }
            for &p in &sel.points {
                let t = InternalTarget::Point { point: p };
                valid &= !taken(t);
                commands.push(add(t, InternalKind::Mark));
            }
            for &n in &sel.notches {
                let t = InternalTarget::Notch { notch: n };
                valid &= !taken(t);
                commands.push(add(t, InternalKind::Mark));
            }
            let selection_ok = (valid && !commands.is_empty()).then_some(commands);
            (
                rows,
                project.piece_internal_counts(piece.id),
                report_lines,
                selection_ok,
            )
        };
        if rows.is_empty() {
            crate::a11y::label(
                ui,
                RichText::new(tr("piece.internal.none"))
                    .small()
                    .color(pal().muted),
            );
        } else {
            crate::a11y::label(
                ui,
                RichText::new(tr_args(
                    "piece.internal.summary",
                    &[
                        ("lines", &counts.lines().to_string()),
                        ("marks", &counts.marks().to_string()),
                    ],
                ))
                .small()
                .color(pal().muted),
            );
        }
        let mut commands: Vec<Command> = Vec::new();
        for row in &rows {
            let target = row.element.target;
            ui.horizontal_wrapped(|ui| {
                crate::a11y::label(ui, &row.text);
                if row.outside {
                    crate::a11y::label(
                        ui,
                        RichText::new(tr("piece.internal.outside"))
                            .small()
                            .color(ui.visuals().warn_fg_color),
                    );
                }
            });
            ui.horizontal(|ui| {
                let kinds: &[InternalKind] = match target {
                    InternalTarget::Path { .. } => &InternalKind::LINES,
                    _ => &InternalKind::MARKS,
                };
                let mut choice = row.element.kind;
                egui::ComboBox::from_id_salt(("piece-internal-kind", piece.id.0, target))
                    .selected_text(internal_kind_label(row.element.kind))
                    .show_ui_named(ui, |ui| {
                        for &k in kinds {
                            ui.selectable_value(&mut choice, k, internal_kind_label(k));
                        }
                    });
                if choice != row.element.kind {
                    commands.push(Command::SetPieceInternalKind {
                        piece: piece.id,
                        target,
                        kind: choice,
                    });
                }
                if ui
                    .button(tr("piece.internal.remove"))
                    .on_hover_text(tr("piece.internal.remove.hover"))
                    .clicked()
                {
                    commands.push(Command::RemovePieceInternal {
                        piece: piece.id,
                        target,
                    });
                }
            });
        }
        if ui
            .add_enabled(
                !piece.outline.is_empty(),
                egui::Button::new(tr("piece.internal.import")),
            )
            .on_hover_text(tr("piece.internal.import.hover"))
            .clicked()
        {
            match self.editor.project().piece_internal_import(piece.id) {
                Ok(report) => {
                    let done = self
                        .editor
                        .exec(Command::ImportPieceInternals { piece: piece.id });
                    if done.is_some() {
                        self.internal_report = Some((piece.id, report));
                    }
                }
                Err(e) => self.editor.error(core_error(&e)),
            }
        }
        if ui
            .add_enabled(
                selection_ok.is_some(),
                egui::Button::new(tr("piece.internal.add")),
            )
            .on_hover_text(tr("piece.internal.add.hover"))
            .clicked()
        {
            if let Some(mut cmds) = selection_ok {
                commands.push(if cmds.len() == 1 {
                    cmds.remove(0)
                } else {
                    Command::Batch { commands: cmds }
                });
            }
        }
        for line in &report_lines {
            crate::a11y::label(ui, RichText::new(line).small().color(pal().muted));
        }
        for c in commands {
            self.editor.exec(c);
        }
    }

    /// Perçages et pointages d'une pièce (US-06.3) : liste (forme, ancrage, position,
    /// dimension modifiable, retrait) et formulaire d'ajout depuis la sélection. Chaque
    /// action est une commande, donc une étape d'annulation. Ces marquages sont exportés
    /// (PDF, SVG) ; ils ne sont pas des éléments internes (US-04.3).
    fn piece_marking_section(&mut self, ui: &mut egui::Ui, piece: &pf_core::Piece) {
        use pf_core::{MarkingAnchor, MarkingShape};
        struct Row {
            marking: pf_core::PieceMarking,
            text: String,
            outside: bool,
        }
        let rows: Vec<Row> = {
            let project = self.editor.project();
            let sheet = project.piece_sheet(piece.id).ok();
            let name = |p: PointId| sheet.map(|s| s.label(p).to_string()).unwrap_or_default();
            project
                .piece_markings(piece.id)
                .into_iter()
                .map(|r| {
                    let anchor = match r.marking.at {
                        MarkingAnchor::Point { point } => {
                            tr_args("piece.marking.anchor.point", &[("name", &name(point))])
                        }
                        MarkingAnchor::DartTip {
                            tip,
                            toward,
                            setback_mm,
                        } => tr_args(
                            "piece.marking.anchor.dart",
                            &[
                                ("tip", &name(tip)),
                                ("toward", &name(toward)),
                                ("setback", &format_mm(setback_mm)),
                            ],
                        ),
                    };
                    let text = tr_args(
                        "piece.marking.row",
                        &[
                            ("shape", &marking_shape_label(r.marking.shape)),
                            ("anchor", &anchor),
                            ("x", &format_mm(r.at.x)),
                            ("y", &format_mm(r.at.y)),
                        ],
                    );
                    Row {
                        marking: r.marking,
                        text,
                        outside: r.inside_outline == Some(false),
                    }
                })
                .collect()
        };
        if rows.is_empty() {
            crate::a11y::label(
                ui,
                RichText::new(tr("piece.marking.none"))
                    .small()
                    .color(pal().muted),
            );
        } else {
            crate::a11y::label(
                ui,
                RichText::new(tr_args(
                    "piece.marking.summary",
                    &[("count", &rows.len().to_string())],
                ))
                .small()
                .color(pal().muted),
            );
        }
        let mut commands: Vec<Command> = Vec::new();
        for row in &rows {
            let m = row.marking;
            ui.horizontal_wrapped(|ui| {
                crate::a11y::label(ui, &row.text);
                if row.outside {
                    crate::a11y::label(
                        ui,
                        RichText::new(tr("piece.marking.outside"))
                            .small()
                            .color(ui.visuals().warn_fg_color),
                    );
                }
            });
            ui.horizontal_wrapped(|ui| {
                let key = (m.id.0 << 8) | 0x70;
                // Dimension d'un perçage ou d'une croix, validée à la perte du focus.
                if let Some(size) = m.shape.size_mm() {
                    crate::a11y::label(ui, RichText::new(tr("piece.marking.edit.size")).small());
                    let current = pf_core::units::field_text(size);
                    if let Some(text) = buffered_edit(ui, &mut self.text_buf, key, &current, 56.0) {
                        match pf_core::units::parse_length_display(&text) {
                            Ok(v) => commands.push(Command::SetPieceMarking {
                                piece: piece.id,
                                marking: m.id,
                                at: m.at,
                                shape: match m.shape {
                                    MarkingShape::Drill { .. } => {
                                        MarkingShape::Drill { diameter_mm: v }
                                    }
                                    _ => MarkingShape::Cross { size_mm: v },
                                },
                            }),
                            Err(_) => self.editor.error(tr("piece.marking.invalid_size")),
                        }
                    }
                }
                // Recul d'un bout de pince, validé à la perte du focus.
                if let MarkingAnchor::DartTip {
                    tip,
                    toward,
                    setback_mm,
                } = m.at
                {
                    crate::a11y::label(ui, RichText::new(tr("piece.marking.edit.setback")).small());
                    let current = pf_core::units::field_text(setback_mm);
                    if let Some(text) =
                        buffered_edit(ui, &mut self.text_buf, key | 1, &current, 56.0)
                    {
                        match pf_core::units::parse_length_display(&text) {
                            Ok(v) => commands.push(Command::SetPieceMarking {
                                piece: piece.id,
                                marking: m.id,
                                at: MarkingAnchor::DartTip {
                                    tip,
                                    toward,
                                    setback_mm: v,
                                },
                                shape: m.shape,
                            }),
                            Err(_) => self.editor.error(tr("piece.marking.invalid_setback")),
                        }
                    }
                }
                if ui
                    .button(tr("piece.marking.remove"))
                    .on_hover_text(tr("piece.marking.remove.hover"))
                    .clicked()
                {
                    commands.push(Command::RemovePieceMarking {
                        piece: piece.id,
                        marking: m.id,
                    });
                }
            });
        }
        let open = self
            .marking_form
            .as_ref()
            .is_some_and(|f| f.piece == piece.id);
        if !open {
            if ui
                .button(tr("piece.marking.add"))
                .on_hover_text(tr("piece.marking.add.hover"))
                .clicked()
            {
                self.marking_form = Some(MarkingForm {
                    piece: piece.id,
                    kind: MarkingKind::Drill,
                    size: "6".into(),
                    dart: false,
                    setback: "10".into(),
                });
            }
        } else if let Some(mut form) = self.marking_form.take() {
            ui.horizontal_wrapped(|ui| {
                ui.selectable_value(
                    &mut form.kind,
                    MarkingKind::Punch,
                    tr("piece.marking.kind.punch"),
                );
                ui.selectable_value(
                    &mut form.kind,
                    MarkingKind::Drill,
                    tr("piece.marking.kind.drill"),
                );
                ui.selectable_value(
                    &mut form.kind,
                    MarkingKind::Cross,
                    tr("piece.marking.kind.cross"),
                );
            });
            egui::Grid::new(("marking-form", piece.id.0))
                .num_columns(2)
                .show(ui, |ui| {
                    match form.kind {
                        MarkingKind::Punch => {}
                        MarkingKind::Drill => {
                            crate::a11y::label(ui, tr("piece.marking.form.size_drill"));
                            crate::a11y::add_text(
                                ui,
                                egui::TextEdit::singleline(&mut form.size).desired_width(70.0),
                            );
                            ui.end_row();
                        }
                        MarkingKind::Cross => {
                            crate::a11y::label(ui, tr("piece.marking.form.size_cross"));
                            crate::a11y::add_text(
                                ui,
                                egui::TextEdit::singleline(&mut form.size).desired_width(70.0),
                            );
                            ui.end_row();
                        }
                    }
                    crate::a11y::label(ui, "");
                    ui.checkbox(&mut form.dart, tr("piece.marking.form.dart"));
                    ui.end_row();
                    if form.dart {
                        crate::a11y::label(ui, tr("piece.marking.form.setback"));
                        crate::a11y::add_text(
                            ui,
                            egui::TextEdit::singleline(&mut form.setback).desired_width(70.0),
                        );
                        ui.end_row();
                    }
                });
            let hint = if form.dart {
                tr("piece.marking.form.hint.dart")
            } else {
                tr("piece.marking.form.hint.point")
            };
            crate::a11y::label(ui, RichText::new(hint).small().color(pal().muted));
            let selected = self.editor.selection.points.clone();
            let ready = selected.len() == if form.dart { 2 } else { 1 };
            let (mut add, mut close) = (false, false);
            ui.horizontal(|ui| {
                add = ui
                    .add_enabled(ready, egui::Button::new(tr("piece.marking.form.ok")))
                    .clicked();
                close = ui.button(tr("piece.marking.form.cancel")).clicked();
            });
            if !close {
                if add && ready {
                    let shape = match form.kind {
                        MarkingKind::Punch => Ok(MarkingShape::Punch),
                        MarkingKind::Drill => pf_core::units::parse_length_display(&form.size)
                            .map(|v| MarkingShape::Drill { diameter_mm: v }),
                        MarkingKind::Cross => pf_core::units::parse_length_display(&form.size)
                            .map(|v| MarkingShape::Cross { size_mm: v }),
                    };
                    let at = if form.dart {
                        pf_core::units::parse_length_display(&form.setback)
                            .map(|v| MarkingAnchor::DartTip {
                                tip: selected[0],
                                toward: selected[1],
                                setback_mm: v,
                            })
                            .map_err(|_| true)
                    } else {
                        Ok(MarkingAnchor::Point { point: selected[0] })
                    };
                    match (shape, at) {
                        (Err(_), _) => self.editor.error(tr("piece.marking.invalid_size")),
                        (_, Err(_)) => self.editor.error(tr("piece.marking.invalid_setback")),
                        (Ok(shape), Ok(at)) => {
                            if self
                                .editor
                                .exec(Command::AddPieceMarking {
                                    piece: piece.id,
                                    at,
                                    shape,
                                })
                                .is_some()
                            {
                                self.editor.info(tr("piece.marking.added"));
                            }
                        }
                    }
                }
                self.marking_form = Some(form);
            }
        }
        for c in commands {
            self.editor.exec(c);
        }
        self.piece_pitch_block(ui, piece);
        self.piece_stitch_block(ui, piece);
        self.piece_zone_block(ui, piece);
    }

    // ----- palette de saisie ----------------------------------------------

    /// Palette non modale affichée pendant un tracé : longueur / angle.
    pub fn palette(&mut self, ctx: &egui::Context, canvas: egui::Rect) {
        let open = self.editor.wants_numeric_input();
        let just_opened = open && !self.palette_open;
        self.palette_open = open;
        if !open {
            return;
        }
        let preview = self.editor.preview();
        let live = preview.as_ref().ok().copied().flatten();
        egui::Area::new(egui::Id::new("palette"))
            .order(egui::Order::Foreground)
            .fixed_pos(canvas.left_top() + egui::vec2(14.0, 14.0))
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_width(250.0);
                    crate::a11y::label(
                        ui,
                        RichText::new(tr("palette.title"))
                            .small()
                            .strong()
                            .color(pal().accent_text),
                    );
                    let dart_draft = matches!(self.editor.state, ToolState::DartBase { .. })
                        .then(|| self.editor.dart_draft())
                        .flatten();
                    let (length_label, live_len, live_angle) = match (self.editor.state, live) {
                        (ToolState::DartBase { .. }, _) => (
                            tr("palette.dart_opening"),
                            dart_draft.as_ref().map(|d| d.opening_mm),
                            dart_draft.as_ref().map(|d| d.length_mm),
                        ),
                        (ToolState::Idle, _) if self.editor.tool == Tool::Guide => {
                            let c = self.editor.cursor;
                            match self.editor.guide_mode {
                                crate::editor::GuideMode::Horizontal => {
                                    (tr("palette.guide_y"), Some(c.y), None)
                                }
                                _ => (tr("palette.guide_x"), Some(c.x), None),
                            }
                        }
                        (
                            ToolState::GuideFrom { .. },
                            Some(crate::editor::Preview::Guide { angle_deg, .. }),
                        ) => (tr("palette.guide_angle"), None, Some(angle_deg)),
                        (ToolState::RelativePick, _) => (
                            match self.editor.point_mode {
                                crate::editor::PointMode::Offset => tr("palette.dx"),
                                crate::editor::PointMode::Compass => tr("palette.radius_a"),
                                crate::editor::PointMode::AlongCurve => {
                                    tr("palette.distance_along")
                                }
                                crate::editor::PointMode::Division => tr("palette.division"),
                                _ => tr("palette.distance"),
                            },
                            None,
                            None,
                        ),
                        (ToolState::ExtendFrom { .. }, _) => (tr("palette.extend_by"), None, None),
                        (ToolState::OffsetReady, _) => (
                            tr("palette.offset_distance"),
                            self.editor.offset_distance().ok().map(f64::abs),
                            None,
                        ),
                        (ToolState::JoinReady { .. }, _) => (
                            if self.editor.tool == Tool::Fillet {
                                tr("palette.radius")
                            } else {
                                tr("palette.distance")
                            },
                            None,
                            None,
                        ),
                        (
                            ToolState::ArcCenter { .. },
                            Some(crate::editor::Preview::Segment {
                                length_mm,
                                angle_deg,
                                ..
                            }),
                        ) => (tr("palette.radius"), Some(length_mm), Some(angle_deg)),
                        (ToolState::ArcCenter { .. }, _) => (tr("palette.radius"), None, None),
                        (ToolState::ArcSweep { radius_mm, .. }, p) => {
                            let end = match p {
                                Some(crate::editor::Preview::Arc { end_deg, .. }) => Some(end_deg),
                                _ => None,
                            };
                            (tr("palette.radius"), Some(radius_mm), end)
                        }
                        (_, Some(crate::editor::Preview::Circle { radius_mm, .. }))
                        | (_, Some(crate::editor::Preview::Arc { radius_mm, .. })) => {
                            (tr("palette.radius"), Some(radius_mm), None)
                        }
                        (ToolState::CircleCenter { .. } | ToolState::RoundSecond { .. }, _) => {
                            (tr("palette.radius"), None, None)
                        }
                        (ToolState::PointOn { origin, segment }, p) => {
                            let ends = self.editor.sheet().segment_point_ids(segment).ok();
                            let label = ends
                                .map(|(a, b)| match origin {
                                    SegmentEnd::Start => self.editor.sheet().label(a).to_string(),
                                    SegmentEnd::End => self.editor.sheet().label(b).to_string(),
                                })
                                .unwrap_or_default();
                            let d = match p {
                                Some(crate::editor::Preview::PointOn { distance_mm, .. }) => {
                                    Some(distance_mm)
                                }
                                _ => None,
                            };
                            (
                                tr_args("palette.distance_from", &[("label", &label)]),
                                d,
                                None,
                            )
                        }
                        (
                            _,
                            Some(crate::editor::Preview::Segment {
                                length_mm,
                                angle_deg,
                                ..
                            }),
                        ) => (tr("palette.length"), Some(length_mm), Some(angle_deg)),
                        (_, Some(crate::editor::Preview::Perpendicular { length_mm, .. })) => {
                            (tr("palette.length"), Some(length_mm), None)
                        }
                        (_, Some(crate::editor::Preview::Parallel { distance_mm, .. })) => {
                            (tr("palette.distance"), Some(distance_mm), None)
                        }
                        (ToolState::ParallelOf { .. }, _) => (tr("palette.distance"), None, None),
                        (
                            _,
                            Some(crate::editor::Preview::Rectangle {
                                width_mm,
                                height_mm,
                                ..
                            }),
                        ) => (
                            tr("palette.width"),
                            Some(width_mm.abs()),
                            Some(height_mm.abs()),
                        ),
                        (ToolState::RectFrom { .. }, _) => (tr("palette.width"), None, None),
                        (
                            _,
                            Some(crate::editor::Preview::Notch {
                                distance_mm,
                                length_mm,
                                ..
                            }),
                        ) => (
                            tr("palette.distance_start"),
                            Some(distance_mm),
                            Some(length_mm),
                        ),
                        (ToolState::NotchOn { .. }, _) => {
                            (tr("palette.distance_start"), None, Some(5.0))
                        }
                        (_, Some(crate::editor::Preview::Dimension { offset_mm, .. })) => {
                            (tr("palette.dimension_offset"), Some(offset_mm.abs()), None)
                        }
                        (ToolState::DimensionFrom { .. }, _) => {
                            (tr("palette.dimension_offset"), None, None)
                        }
                        (ToolState::SegmentFrom { .. }, _) => {
                            (tr("palette.length"), None, Some(0.0))
                        }
                        _ => (tr("palette.length"), None, None),
                    };
                    let hint = |v: Option<f64>| v.map(|v| format_number(v, 2)).unwrap_or_default();
                    let hint_len = |v: Option<f64>| {
                        v.map(|v| pf_core::units::len_number(v, 2))
                            .unwrap_or_default()
                    };
                    egui::Grid::new("palette-grid")
                        .num_columns(3)
                        .show(ui, |ui| {
                            let sweeping = matches!(self.editor.state, ToolState::ArcSweep { .. });
                            let guide_from =
                                matches!(self.editor.state, ToolState::GuideFrom { .. });
                            if !sweeping && !guide_from {
                                crate::a11y::label(ui, length_label);
                                let resp = crate::a11y::add_text(
                                    ui,
                                    egui::TextEdit::singleline(&mut self.editor.length_text)
                                        .hint_text(
                                            if self.editor.state == ToolState::RelativePick
                                                && self.editor.point_mode
                                                    == crate::editor::PointMode::Division
                                            {
                                                hint(live_len)
                                            } else {
                                                hint_len(live_len)
                                            },
                                        )
                                        .desired_width(80.0)
                                        .min_size(egui::vec2(80.0, 0.0)),
                                );
                                if just_opened && self.editor.state != ToolState::Idle {
                                    resp.request_focus();
                                }
                                if resp.has_focus() {
                                    self.editor.entry_field = 0;
                                }
                                let is_division = self.editor.state == ToolState::RelativePick
                                    && self.editor.point_mode == crate::editor::PointMode::Division;
                                crate::a11y::label(
                                    ui,
                                    if is_division {
                                        ""
                                    } else {
                                        pf_core::units::display_units().unit.suffix()
                                    },
                                );
                                ui.end_row();
                            }
                            if let ToolState::ArcCenter { .. } | ToolState::ArcSweep { .. } =
                                self.editor.state
                            {
                                crate::a11y::label(
                                    ui,
                                    if sweeping {
                                        tr("palette.end_angle")
                                    } else {
                                        tr("palette.start_angle")
                                    },
                                );
                                let resp = crate::a11y::add_text(
                                    ui,
                                    egui::TextEdit::singleline(&mut self.editor.angle_text)
                                        .hint_text(hint(live_angle))
                                        .desired_width(80.0)
                                        .min_size(egui::vec2(80.0, 0.0)),
                                );
                                if sweeping {
                                    resp.request_focus();
                                }
                                crate::a11y::label(ui, "°");
                                ui.end_row();
                            }
                            if guide_from {
                                crate::a11y::label(ui, tr("palette.guide_angle"));
                                crate::a11y::add_text(
                                    ui,
                                    egui::TextEdit::singleline(&mut self.editor.angle_text)
                                        .hint_text(hint(live_angle))
                                        .desired_width(80.0)
                                        .min_size(egui::vec2(80.0, 0.0)),
                                );
                                crate::a11y::label(ui, "°");
                                ui.end_row();
                            }
                            if self.editor.tool == Tool::Guide
                                && self.editor.state == ToolState::Idle
                                && self.editor.guide_mode == crate::editor::GuideMode::Oblique
                            {
                                crate::a11y::label(ui, tr("palette.guide_y"));
                                crate::a11y::add_text(
                                    ui,
                                    egui::TextEdit::singleline(&mut self.editor.angle_text)
                                        .hint_text(hint_len(Some(self.editor.cursor.y)))
                                        .desired_width(80.0)
                                        .min_size(egui::vec2(80.0, 0.0)),
                                );
                                crate::a11y::label(
                                    ui,
                                    pf_core::units::display_units().unit.suffix(),
                                );
                                ui.end_row();
                            }
                            if matches!(self.editor.state, ToolState::SegmentFrom { .. }) {
                                crate::a11y::label(ui, tr("palette.angle"));
                                let resp = crate::a11y::add_text(
                                    ui,
                                    egui::TextEdit::singleline(&mut self.editor.angle_text)
                                        .hint_text(hint(live_angle))
                                        .desired_width(80.0)
                                        .min_size(egui::vec2(80.0, 0.0)),
                                );
                                if resp.has_focus() {
                                    self.editor.entry_field = 1;
                                }
                                crate::a11y::label(ui, "°");
                                ui.end_row();
                            }
                            if matches!(self.editor.state, ToolState::DartBase { .. }) {
                                crate::a11y::label(ui, tr("palette.dart_length"));
                                let resp = crate::a11y::add_text(
                                    ui,
                                    egui::TextEdit::singleline(&mut self.editor.angle_text)
                                        .hint_text(hint_len(live_angle))
                                        .desired_width(80.0)
                                        .min_size(egui::vec2(80.0, 0.0)),
                                );
                                if resp.has_focus() {
                                    self.editor.entry_field = 1;
                                }
                                crate::a11y::label(
                                    ui,
                                    pf_core::units::display_units().unit.suffix(),
                                );
                                ui.end_row();
                            }
                            if matches!(self.editor.state, ToolState::NotchOn { .. }) {
                                crate::a11y::label(ui, tr("palette.notch_length"));
                                crate::a11y::add_text(
                                    ui,
                                    egui::TextEdit::singleline(&mut self.editor.angle_text)
                                        .hint_text(hint_len(live_angle))
                                        .desired_width(80.0)
                                        .min_size(egui::vec2(80.0, 0.0)),
                                );
                                crate::a11y::label(
                                    ui,
                                    pf_core::units::display_units().unit.suffix(),
                                );
                                ui.end_row();
                            }
                            if matches!(self.editor.state, ToolState::JoinReady { .. })
                                && self.editor.tool == Tool::Chamfer
                            {
                                crate::a11y::label(ui, tr("palette.distance_second"));
                                crate::a11y::add_text(
                                    ui,
                                    egui::TextEdit::singleline(&mut self.editor.angle_text)
                                        .hint_text(hint_len(live_angle))
                                        .desired_width(80.0)
                                        .min_size(egui::vec2(80.0, 0.0)),
                                );
                                crate::a11y::label(
                                    ui,
                                    pf_core::units::display_units().unit.suffix(),
                                );
                                ui.end_row();
                            }
                            if self.editor.state == ToolState::RelativePick {
                                use crate::editor::PointMode;
                                let unit_word = pf_core::units::display_units().unit.suffix();
                                let second = match self.editor.point_mode {
                                    PointMode::Offset => Some((tr("palette.dy"), unit_word)),
                                    PointMode::Compass => Some((tr("palette.radius_b"), unit_word)),
                                    PointMode::Direction | PointMode::DirectionRelative => {
                                        Some((tr("palette.angle"), "°"))
                                    }
                                    _ => None,
                                };
                                if let Some((label, unit)) = second {
                                    crate::a11y::label(ui, label);
                                    crate::a11y::add_text(
                                        ui,
                                        egui::TextEdit::singleline(&mut self.editor.angle_text)
                                            .desired_width(80.0)
                                            .min_size(egui::vec2(80.0, 0.0)),
                                    );
                                    crate::a11y::label(ui, unit);
                                    ui.end_row();
                                }
                            }
                            if matches!(self.editor.state, ToolState::RectFrom { .. }) {
                                crate::a11y::label(ui, tr("palette.height"));
                                let resp = crate::a11y::add_text(
                                    ui,
                                    egui::TextEdit::singleline(&mut self.editor.angle_text)
                                        .hint_text(hint_len(live_angle))
                                        .desired_width(80.0)
                                        .min_size(egui::vec2(80.0, 0.0)),
                                );
                                if resp.has_focus() {
                                    self.editor.entry_field = 1;
                                }
                                crate::a11y::label(
                                    ui,
                                    pf_core::units::display_units().unit.suffix(),
                                );
                                ui.end_row();
                            }
                        });
                    if matches!(self.editor.state, ToolState::RectFrom { .. }) {
                        ui.checkbox(&mut self.editor.rect_kept, tr("palette.rect_kept"))
                            .on_hover_text(tr("palette.rect_kept.hover"));
                    }
                    // Formule à la création : une expression saisie sera maintenue (ƒ + texte).
                    let kept_now = self.editor.typed_creation_formulas();
                    for (expr, value, angle) in &kept_now {
                        let shown = if *angle {
                            format!("{} °", format_number(*value, 3))
                        } else {
                            App::kept_value_text(*value, "mm")
                        };
                        crate::a11y::label(
                            ui,
                            RichText::new(tr_args(
                                "palette.formula.kept",
                                &[("expr", expr), ("value", &shown)],
                            ))
                            .strong(),
                        );
                    }
                    if kept_now.is_empty()
                        && (matches!(self.editor.state, ToolState::PointOn { .. })
                            || (self.editor.state == ToolState::RelativePick
                                && self.editor.point_mode != crate::editor::PointMode::Division))
                    {
                        crate::a11y::label(
                            ui,
                            RichText::new(tr("palette.formula.hint"))
                                .small()
                                .color(pal().muted),
                        );
                    }
                    match self.editor.state {
                        ToolState::DartBase { .. } => {
                            let side = if self.editor.perp_flip {
                                tr("palette.side.right")
                            } else {
                                tr("palette.side.left")
                            };
                            ui.horizontal(|ui| {
                                crate::a11y::label(
                                    ui,
                                    tr_args("palette.dart_fixed", &[("side", &side)]),
                                );
                                if ui.small_button(tr("palette.flip")).clicked() {
                                    self.editor.perp_flip = !self.editor.perp_flip;
                                }
                            });
                            match dart_draft.as_ref().map(|d| &d.result) {
                                Some(Ok(c)) => {
                                    crate::a11y::label(
                                        ui,
                                        tr_args(
                                            "dart.panel.line",
                                            &[
                                                ("before", &format_mm(c.closed.before_mm)),
                                                ("after", &format_mm(c.closed.after_mm)),
                                                ("removed", &format_mm(c.closed.removed_mm)),
                                            ],
                                        ),
                                    );
                                }
                                Some(Err(e)) => {
                                    crate::a11y::label(ui, RichText::new(e).color(pal().error));
                                }
                                None => {}
                            }
                        }
                        ToolState::ParallelOf { .. } => {
                            let side = match live {
                                Some(crate::editor::Preview::Parallel {
                                    side: Side::Left, ..
                                }) => tr("palette.side.left"),
                                Some(crate::editor::Preview::Parallel {
                                    side: Side::Right,
                                    ..
                                }) => tr("palette.side.right"),
                                _ => "—".to_string(),
                            };
                            ui.horizontal(|ui| {
                                crate::a11y::label(
                                    ui,
                                    tr_args("palette.side_segment", &[("side", &side)]),
                                );
                                if ui.small_button(tr("palette.flip")).clicked() {
                                    self.editor.perp_flip = !self.editor.perp_flip;
                                }
                            });
                        }
                        ToolState::NotchOn { .. } => {
                            let side = match live {
                                Some(crate::editor::Preview::Notch {
                                    side: Side::Left, ..
                                }) => tr("palette.side.left"),
                                Some(crate::editor::Preview::Notch {
                                    side: Side::Right, ..
                                }) => tr("palette.side.right"),
                                _ => "—".to_string(),
                            };
                            ui.horizontal(|ui| {
                                crate::a11y::label(
                                    ui,
                                    tr_args("palette.side_path", &[("side", &side)]),
                                );
                                if ui.small_button(tr("palette.flip")).clicked() {
                                    self.editor.perp_flip = !self.editor.perp_flip;
                                }
                            });
                            ui.checkbox(&mut self.editor.notch_double, tr("palette.notch_double"));
                        }
                        ToolState::OffsetReady => {
                            let side = match self.editor.offset_left() {
                                Some(true) => tr("palette.side.left"),
                                Some(false) => tr("palette.side.right"),
                                None => "—".to_string(),
                            };
                            ui.horizontal(|ui| {
                                crate::a11y::label(
                                    ui,
                                    tr_args("palette.offset_side", &[("side", &side)]),
                                );
                                if ui.small_button(tr("palette.flip")).clicked() {
                                    self.editor.flip_offset();
                                }
                            });
                            ui.horizontal(|ui| {
                                crate::a11y::label(ui, tr("palette.offset_corner"));
                                for (corner, key) in [
                                    (CornerStyle::Miter, "palette.offset_miter"),
                                    (CornerStyle::Round, "palette.offset_round"),
                                ] {
                                    if ui
                                        .selectable_label(
                                            self.editor.offset_corner == corner,
                                            tr(key),
                                        )
                                        .clicked()
                                    {
                                        self.editor.set_offset_corner(corner);
                                    }
                                }
                            });
                            ui.horizontal(|ui| {
                                crate::a11y::label(ui, tr("palette.offset_role"));
                                egui::ComboBox::from_id_salt("offset-role")
                                    .selected_text(role_label(self.editor.offset_role))
                                    .show_ui_named(ui, |ui| {
                                        for r in PathRole::ALL {
                                            ui.selectable_value(
                                                &mut self.editor.offset_role,
                                                r,
                                                role_label(r),
                                            );
                                        }
                                    });
                            });
                        }
                        ToolState::DimensionFrom { .. } => {
                            let side = match live {
                                Some(crate::editor::Preview::Dimension { offset_mm, .. }) => {
                                    if offset_mm >= 0.0 {
                                        tr("palette.side.left")
                                    } else {
                                        tr("palette.side.right")
                                    }
                                }
                                _ => "—".to_string(),
                            };
                            ui.horizontal(|ui| {
                                crate::a11y::label(
                                    ui,
                                    tr_args("palette.dimension_side", &[("side", &side)]),
                                );
                                if ui.small_button(tr("palette.flip")).clicked() {
                                    self.editor.perp_flip = !self.editor.perp_flip;
                                }
                            });
                        }
                        ToolState::PerpFrom { .. } => {
                            let side = match live {
                                Some(crate::editor::Preview::Perpendicular {
                                    side: Side::Left,
                                    ..
                                }) => tr("palette.side.left"),
                                Some(crate::editor::Preview::Perpendicular {
                                    side: Side::Right,
                                    ..
                                }) => tr("palette.side.right"),
                                _ => "—".to_string(),
                            };
                            ui.horizontal(|ui| {
                                crate::a11y::label(ui, tr_args("palette.side", &[("side", &side)]));
                                if ui.small_button(tr("palette.flip")).clicked() {
                                    self.editor.perp_flip = !self.editor.perp_flip;
                                }
                            });
                        }
                        ToolState::ArcSweep { .. } | ToolState::RoundSecond { .. }
                            if self.editor.tool == Tool::Arc =>
                        {
                            let text = if self.editor.perp_flip {
                                tr("palette.arc_cw")
                            } else {
                                tr("palette.arc_ccw")
                            };
                            ui.horizontal(|ui| {
                                crate::a11y::label(ui, text);
                                if ui.small_button(tr("palette.flip")).clicked() {
                                    self.editor.perp_flip = !self.editor.perp_flip;
                                }
                            });
                        }
                        ToolState::RelativePick
                            if self.editor.point_mode == crate::editor::PointMode::AlongCurve =>
                        {
                            ui.checkbox(&mut self.editor.rel_from_end, tr("palette.from_end"));
                        }
                        ToolState::PointOn { .. }
                            if ui.small_button(tr("palette.other_end")).clicked() =>
                        {
                            self.editor.flip_origin();
                        }
                        _ => {}
                    }
                    if let Err(e) = &preview {
                        crate::a11y::label(ui, RichText::new(e).color(pal().error));
                    }
                    crate::a11y::label(
                        ui,
                        RichText::new(tr("palette.hint")).small().color(pal().muted),
                    );
                });
            });
    }

    // ----- bulle de saisie directe (US-16.2) --------------------------------

    /// Bulle près du curseur : les mêmes champs que la palette (un seul état,
    /// `length_text` / `angle_text`), en lecture seule. Tant qu'un champ est vide
    /// elle montre la mesure observée (italique, « mesuré ») ; une valeur tapée
    /// est surlignée et soulignée (« saisi »). Sans effet sur le projet.
    pub fn entry_bubble(&mut self, ctx: &egui::Context, canvas: egui::Rect) {
        let rows = self.editor.entry_rows();
        if !self.canvas_hovered || rows.is_empty() {
            return;
        }
        let center = canvas.center();
        let (x, y) = self.view.to_screen(
            self.editor.cursor,
            (f64::from(center.x), f64::from(center.y)),
        );
        let size = egui::vec2(300.0, 28.0 * rows.len() as f32 + 60.0);
        let pos = egui::pos2(
            (x as f32 + 18.0).clamp(canvas.left(), (canvas.right() - size.x).max(canvas.left())),
            (y as f32 + 38.0).clamp(canvas.top(), (canvas.bottom() - size.y).max(canvas.top())),
        );
        let problem = self.editor.preview().err();
        let lock = self.editor.angle_lock
            && matches!(
                self.editor.state,
                ToolState::SegmentFrom { .. } | ToolState::GuideFrom { .. }
            );
        let position_row = self.editor.state == ToolState::Idle;
        egui::Area::new(egui::Id::new("entry-bubble"))
            .order(egui::Order::Tooltip)
            .interactable(false)
            .fixed_pos(pos)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_max_width(size.x);
                    let base = egui::TextFormat {
                        font_id: egui::TextStyle::Body.resolve(ui.style()),
                        color: pal().ink,
                        ..Default::default()
                    };
                    for row in &rows {
                        let mut job = egui::text::LayoutJob::default();
                        job.append(if row.active { "› " } else { "  " }, 0.0, base.clone());
                        job.append(&format!("{} : ", row.label), 0.0, base.clone());
                        let unit = if row.unit.is_empty() {
                            String::new()
                        } else {
                            format!(" {}", row.unit)
                        };
                        if row.typed.is_empty() {
                            let observed = row.observed.clone().unwrap_or_else(|| "—".into());
                            let weak = egui::TextFormat {
                                italics: true,
                                color: pal().muted,
                                ..base.clone()
                            };
                            job.append(
                                &format!("{observed}{unit} · {}", tr("bubble.observed")),
                                0.0,
                                weak,
                            );
                        } else {
                            let typed = egui::TextFormat {
                                background: pal().select.gamma_multiply(0.45),
                                underline: egui::Stroke::new(1.0_f32, pal().ink),
                                ..base.clone()
                            };
                            job.append(&format!(" {} ", row.typed), 0.0, typed);
                            job.append(
                                &format!("{unit} · {}", tr("bubble.typed")),
                                0.0,
                                base.clone(),
                            );
                        }
                        crate::a11y::label(ui, job);
                    }
                    if lock {
                        crate::a11y::label(
                            ui,
                            RichText::new(tr_args(
                                "bubble.angle_lock",
                                &[(
                                    "step",
                                    &(pf_geometry::ANGLE_LOCK_STEP_DEG.round() as i64).to_string(),
                                )],
                            ))
                            .small()
                            .strong(),
                        );
                    }
                    if let Some(e) = &problem {
                        crate::a11y::label(
                            ui,
                            RichText::new(format!("! {e}")).small().color(pal().error),
                        );
                    }
                    let hint = if position_row {
                        tr("bubble.coord_hint")
                    } else {
                        tr("bubble.hint")
                    };
                    crate::a11y::label(ui, RichText::new(hint).small().color(pal().muted));
                });
            });
    }

    // ----- inspecteur ------------------------------------------------------

    pub fn inspector_panel(&mut self, ctx: &egui::Context) {
        if self.plan.is_some() {
            self.plan_inspector(ctx);
            return;
        }
        egui::SidePanel::right("inspector")
            .resizable(true)
            .default_width(270.0)
            .min_width(230.0)
            .show(ctx, |ui| {
                // Gouttière réservée : la barre de défilement ne recouvre pas les valeurs.
                ui.spacing_mut().scroll = egui::style::ScrollStyle::solid();
                let mut scroll = egui::ScrollArea::vertical().auto_shrink([false, false]);
                if let Some(offset) = self.capture_scroll {
                    scroll = scroll.vertical_scroll_offset(offset);
                }
                scroll.show(ui, |ui| {
                    // Plan en lecture seule (taille active ≠ base) : l'inspecteur ne modifie rien.
                    if self.editor.size_view().is_some() {
                        ui.disable();
                    }
                    section(ui, &tr("inspector.title"));
                    // US-09.5a : ce que l'inspecteur grisé montre est évalué dans cette taille.
                    if let Some(size) = self.editor.size_view() {
                        crate::a11y::label(
                            ui,
                            RichText::new(tr_args("inspector.size.evaluated", &[("name", &size)]))
                                .small()
                                .color(pal().muted),
                        );
                    }
                    let sel = self.editor.selection.clone();
                    match (sel.points.as_slice(), sel.entities.as_slice()) {
                        _ if sel.notches.len() == 1
                            && sel.points.is_empty()
                            && sel.entities.is_empty() =>
                        {
                            self.inspect_notch(ui, sel.notches[0]);
                        }
                        _ if sel.dimensions.len() == 1
                            && sel.points.is_empty()
                            && sel.entities.is_empty()
                            && sel.notches.is_empty() =>
                        {
                            self.inspect_dimension(ui, sel.dimensions[0]);
                        }
                        _ if sel.guides.len() == 1
                            && sel.points.is_empty()
                            && sel.entities.is_empty()
                            && sel.notches.is_empty()
                            && sel.dimensions.is_empty() =>
                        {
                            self.inspect_guide(ui, sel.guides[0]);
                        }
                        ([], []) => self.inspect_sheet(ui),
                        ([p], []) => self.inspect_point(ui, *p),
                        ([], [e]) => self.inspect_entity(ui, *e),
                        ([a, b], []) => {
                            let s = self.editor.sheet();
                            ui.heading(tr_args(
                                "inspector.points.title",
                                &[("a", s.label(*a)), ("b", s.label(*b))],
                            ));
                            section(ui, &tr("inspector.section.measure"));
                            if let Ok(m) = measure::point_distance(s, *a, *b) {
                                row(ui, &tr("inspector.points.distance"), &format_mm(m.value));
                            }
                        }
                        ([], [a, b]) => self.inspect_pair(ui, *a, *b),
                        _ => self.inspect_many(ui),
                    }
                    self.topstitch_section(ui);
                    self.full_dimension_section(ui);
                    if !sel.is_empty() {
                        ui.add_space(12.0);
                        if ui
                            .button(tr_args(
                                "inspector.delete_selection",
                                &[("key", &tr("key.delete"))],
                            ))
                            .clicked()
                        {
                            self.request_delete();
                        }
                    }
                });
            });
    }

    fn inspect_sheet(&mut self, ui: &mut egui::Ui) {
        let s = self.editor.sheet();
        ui.heading(tr_args("inspector.sheet.title", &[("name", &s.name)]));
        row(
            ui,
            &tr("inspector.sheet.points"),
            &s.points.len().to_string(),
        );
        row(
            ui,
            &tr("inspector.sheet.paths"),
            &s.entities.len().to_string(),
        );
        row(
            ui,
            &tr("inspector.sheet.notches"),
            &s.notches.len().to_string(),
        );
        row(
            ui,
            &tr("inspector.sheet.dimensions"),
            &s.dimensions.len().to_string(),
        );
        if !s.topstitches.is_empty() {
            let stale = s.topstitches.iter().filter(|t| t.stale.is_some()).count();
            row(
                ui,
                &tr("inspector.sheet.topstitches"),
                &tr_args(
                    "inspector.sheet.topstitches_value",
                    &[
                        ("n", &s.topstitches.len().to_string()),
                        ("stale", &stale.to_string()),
                    ],
                ),
            );
        }
        let grain = match s.grainline_count() {
            0 => tr("inspector.sheet.grainline_none"),
            n => tr_args("inspector.sheet.grainline_count", &[("n", &n.to_string())]),
        };
        row(ui, &tr("inspector.sheet.grainline"), &grain);
        if s.grainline_count() > 1 {
            crate::a11y::label(
                ui,
                RichText::new(tr("inspector.sheet.grainline_many"))
                    .small()
                    .color(pal().muted),
            );
        }
        if let Some(bb) = s.bbox() {
            row(
                ui,
                &tr("inspector.sheet.bbox"),
                &format!(
                    "{} × {}",
                    pf_core::units::len_number(bb.width(), 1),
                    pf_core::units::len_text(bb.height(), 1)
                ),
            );
        }
        let contours = contour::closed_contours(s);
        if !contours.is_empty() {
            fold(
                ui,
                "fold-contours",
                &tr("inspector.contours.title"),
                true,
                |ui| {
                    for (i, c) in contours.iter().enumerate() {
                        if let Ok(m) = contour::measure_contour(s, c) {
                            let title =
                                tr_args("inspector.contour.n", &[("n", &(i + 1).to_string())]);
                            contour_rows(ui, &title, c, &m);
                        }
                    }
                },
            );
        }
        // Ouverte quand la mise en page ou le contenu de sortie ne sont plus ceux par défaut.
        let print_linked = self.page_setup != pf_export::page::PageSetup::default()
            || !self.output_content.is_default();
        fold(
            ui,
            "fold-print",
            &tr("inspector.print.title"),
            print_linked,
            |ui| match self.export_scene(self.editor.sheet) {
                Ok(scene) => {
                    match pdf::page_count_content(
                        std::slice::from_ref(&scene),
                        &self.page_setup,
                        &self.output_content,
                    ) {
                        Ok(n) => row(
                            ui,
                            &tr_args(
                                "inspector.print.format",
                                &[("format", &crate::print_setup::setup_label(&self.page_setup))],
                            ),
                            &tr_args("inspector.print.pages", &[("n", &n.to_string())]),
                        ),
                        Err(e) => {
                            crate::a11y::label(
                                ui,
                                RichText::new(crate::print_setup::export_error(&e))
                                    .color(pal().error),
                            );
                        }
                    }
                    if scene.is_construction_only() {
                        crate::a11y::label(
                            ui,
                            RichText::new(tr("inspector.print.construction_only"))
                                .small()
                                .color(pal().muted),
                        );
                    }
                }
                Err(pf_export::ExportError::NothingSelected(_)) => {
                    crate::a11y::label(
                        ui,
                        RichText::new(tr("inspector.print.nothing_selected")).color(pal().error),
                    );
                }
                Err(_) => {
                    crate::a11y::label(
                        ui,
                        RichText::new(tr("inspector.print.empty")).color(pal().muted),
                    );
                }
            },
        );
        fold(
            ui,
            "fold-about",
            &tr("inspector.about.title"),
            false,
            |ui| {
                crate::a11y::label(
                    ui,
                    RichText::new(tr("inspector.about.text"))
                        .small()
                        .color(pal().muted),
                );
            },
        );
    }

    fn role_combo(&mut self, ui: &mut egui::Ui, entity: EntityId, current: PathRole) {
        section(ui, &tr("inspector.role.title"));
        let mut role = current;
        egui::ComboBox::from_id_salt("role")
            .selected_text(role_label(role))
            .show_ui_named(ui, |ui| {
                for r in PathRole::ALL {
                    ui.selectable_value(&mut role, r, role_label(r));
                }
            });
        if role != current {
            let sheet = self.editor.sheet;
            self.editor.exec(Command::SetPathRole {
                sheet,
                entity,
                role,
            });
        }
    }

    fn inspect_entity(&mut self, ui: &mut egui::Ui, id: EntityId) {
        let s = self.editor.sheet();
        let Ok(entity) = s.entity(id).cloned() else {
            return;
        };
        ui.heading(entity_name(s, id));
        let sheet_id = self.editor.sheet;
        ui.horizontal(|ui| {
            crate::a11y::label(ui, tr("inspector.entity.name"));
            if let Some(text) = buffered_edit(
                ui,
                &mut self.text_buf,
                (id.0 << 8) | 5,
                entity.name.as_deref().unwrap_or(""),
                150.0,
            ) {
                self.editor.exec(Command::SetEntityName {
                    sheet: sheet_id,
                    entity: id,
                    name: Some(text),
                });
            }
        });
        if let Some(g) = self.editor.sheet().group_of(id) {
            row(
                ui,
                &tr("inspector.entity.group"),
                &tr_args(
                    "inspector.entity.group_size",
                    &[("n", &g.members.len().to_string())],
                ),
            );
        }
        let s = self.editor.sheet();
        section(ui, &tr("inspector.section.measures"));
        match entity.kind {
            EntityKind::Segment { construction, .. } => {
                if let Ok(m) = measure::entity_length(s, id) {
                    row(ui, &tr("inspector.length"), &format_mm(m.value));
                }
                if let Ok(d) = measure::segment_direction_deg(s, id) {
                    row(ui, &tr("inspector.direction"), &format_deg(d));
                }
                if let Some(c) = construction {
                    section(ui, &tr("inspector.creation.title"));
                    let text = match c {
                        Construction::Polar {
                            length_mm,
                            angle_deg,
                            ..
                        } => tr_args(
                            "inspector.creation.polar",
                            &[
                                ("length", &format_mm(length_mm)),
                                ("angle", &format_deg(angle_deg)),
                            ],
                        ),
                        Construction::Parallel {
                            distance_mm, side, ..
                        } => tr_args(
                            "inspector.creation.parallel",
                            &[
                                ("distance", &format_mm(distance_mm)),
                                (
                                    "side",
                                    &if side == Side::Left {
                                        tr("inspector.creation.left")
                                    } else {
                                        tr("inspector.creation.right")
                                    },
                                ),
                            ],
                        ),
                        Construction::Perpendicular {
                            length_mm, side, ..
                        } => tr_args(
                            "inspector.creation.perpendicular",
                            &[
                                ("length", &format_mm(length_mm)),
                                (
                                    "side",
                                    &if side == Side::Left {
                                        tr("inspector.creation.left")
                                    } else {
                                        tr("inspector.creation.right")
                                    },
                                ),
                            ],
                        ),
                    };
                    crate::a11y::label(ui, RichText::new(text).monospace());
                    if !c.is_kept() {
                        crate::a11y::label(
                            ui,
                            RichText::new(tr("inspector.creation.note"))
                                .small()
                                .color(pal().muted),
                        );
                    }
                    self.kept_controls(ui, id, c.is_kept());
                }
            }
            EntityKind::Curve { .. } => {
                let len = measure::entity_length(s, id).map(|m| m.value);
                let chord = measure::curve_chord(s, id).map(|m| m.value);
                if let (Ok(len), Ok(chord)) = (len, chord) {
                    row(ui, &tr("inspector.curve.length"), &format_mm(len));
                    row(ui, &tr("inspector.curve.chord"), &format_mm(chord));
                    row(ui, &tr("inspector.curve.gap"), &format_mm(len - chord));
                }
                self.imposed_section(ui, id);
                crate::a11y::label(
                    ui,
                    RichText::new(tr("inspector.curve.note"))
                        .small()
                        .color(pal().muted),
                );
                if ui
                    .button(tr("inspector.curve.to_spline"))
                    .on_hover_text(tr("inspector.curve.to_spline.hover"))
                    .clicked()
                {
                    self.editor.exec(Command::CurveToSpline {
                        sheet: sheet_id,
                        curve: id,
                    });
                }
            }
            EntityKind::Arc { ccw, .. } => {
                if let Ok(arc) = s.arc_geometry(id) {
                    row(ui, &tr("inspector.arc.length"), &format_mm(arc.length_mm()));
                    row(ui, &tr("inspector.arc.radius"), &format_mm(arc.radius_mm));
                    row(
                        ui,
                        &tr("inspector.arc.sweep"),
                        &format_deg(arc.sweep_deg().abs()),
                    );
                    row(
                        ui,
                        &tr("inspector.arc.direction"),
                        &if ccw {
                            tr("inspector.arc.ccw")
                        } else {
                            tr("inspector.arc.cw")
                        },
                    );
                }
                crate::a11y::label(
                    ui,
                    RichText::new(tr("inspector.arc.note"))
                        .small()
                        .color(pal().muted),
                );
            }
            EntityKind::Spline { .. } => self.inspect_spline(ui, id),
            EntityKind::Circle { radius_mm, .. } => {
                if let Ok(c) = s.circle_geometry(id) {
                    row(
                        ui,
                        &tr("inspector.circle.circumference"),
                        &format_mm(c.length_mm()),
                    );
                    row(
                        ui,
                        &tr("inspector.circle.area"),
                        &format!("{} mm²", format_number(c.circle.area_mm2(), 2)),
                    );
                }
                section(ui, &tr("inspector.circle.param"));
                let mut r_mm = radius_mm;
                ui.horizontal(|ui| {
                    crate::a11y::label(ui, tr("inspector.circle.radius"));
                    let r = crate::a11y::add_drag(
                        ui,
                        crate::views::length_drag(
                            egui::DragValue::new(&mut r_mm)
                                .localized()
                                .speed(0.1)
                                .suffix(" mm")
                                .max_decimals(3)
                                .range(0.001..=1.0e6),
                        ),
                    );
                    if r.changed() {
                        self.editor.doc.begin_group();
                        let cmd = Command::SetCircleRadius {
                            sheet: sheet_id,
                            entity: id,
                            radius_mm: r_mm,
                        };
                        if let Err(e) = self.editor.doc.execute(cmd) {
                            self.editor.error(core_error(&e));
                        }
                    }
                    if r.drag_stopped() || r.lost_focus() {
                        self.editor.doc.end_group();
                    }
                });
                crate::a11y::label(
                    ui,
                    RichText::new(tr("inspector.circle.note"))
                        .small()
                        .color(pal().muted),
                );
            }
        }
        self.role_combo(ui, id, entity.role);
    }

    /// Inspecteur d'une courbe par points de passage : mesures observées
    /// (totale et par tronçon), points lisses/anguleux, tangentes.
    fn inspect_spline(&mut self, ui: &mut egui::Ui, id: EntityId) {
        let sheet_id = self.editor.sheet;
        let s = self.editor.sheet();
        let (Ok(resolved), Ok(geom), Ok(entity)) =
            (s.spline_nodes(id), s.spline_geometry(id), s.entity(id))
        else {
            return;
        };
        let EntityKind::Spline { nodes: raw } = entity.kind.clone() else {
            return;
        };
        let lengths = measure::spline_segment_lengths(s, id).unwrap_or_default();
        let total = measure::entity_length(s, id)
            .map(|m| m.value)
            .unwrap_or(f64::NAN);
        let chord = pf_geometry::distance_mm(geom.start_point(), geom.end_point());
        let labels: Vec<String> = resolved
            .iter()
            .map(|n| s.label(n.point).to_string())
            .collect();
        let mids: Vec<pf_geometry::PointMm> =
            geom.segments().iter().map(|c| c.point_at(0.5)).collect();
        row(ui, &tr("inspector.spline.length"), &format_mm(total));
        row(ui, &tr("inspector.spline.chord"), &format_mm(chord));
        row(
            ui,
            &tr("inspector.spline.count"),
            &resolved.len().to_string(),
        );
        self.imposed_section(ui, id);
        section(ui, &tr("inspector.spline.segments"));
        for (i, len) in lengths.iter().enumerate() {
            let title = tr_args(
                "inspector.spline.segment",
                &[("from", &labels[i]), ("to", &labels[i + 1])],
            );
            row(ui, &title, &format_mm(*len));
        }
        section(ui, &tr("inspector.spline.points"));
        // Une seule action par image, appliquée après le parcours.
        let mut action: Option<Command> = None;
        let mut dragging = false;
        let mut group_done = false;
        let last = resolved.len() - 1;
        for (i, n) in resolved.iter().enumerate() {
            ui.add_space(4.0);
            crate::a11y::label(
                ui,
                RichText::new(tr_args("inspector.spline.node", &[("label", &labels[i])])).strong(),
            );
            let current: SplineNode = raw[i];
            let set = |node: SplineNode| Command::SetSplineNode {
                sheet: sheet_id,
                spline: id,
                index: i,
                corner: node.corner,
                tangent: node.tangent,
                tangent_in: node.tangent_in,
            };
            let interior = i != 0 && i != last;
            if interior {
                let mut corner = n.corner;
                if ui
                    .checkbox(&mut corner, tr("inspector.spline.corner"))
                    .on_hover_text(tr("inspector.spline.corner.hover"))
                    .changed()
                {
                    action = Some(set(SplineNode {
                        corner,
                        tangent_in: if corner { current.tangent_in } else { None },
                        ..current
                    }));
                }
            }
            // (titre, valeur, saisie ?, tangente entrante ?)
            let rows: Vec<(String, pf_geometry::VectorMm, bool, bool)> = if n.corner && interior {
                vec![
                    (
                        tr("inspector.spline.tangent_in"),
                        n.tangent_in,
                        n.tangent_in_explicit,
                        true,
                    ),
                    (
                        tr("inspector.spline.tangent_out"),
                        n.tangent_out,
                        n.tangent_out_explicit,
                        false,
                    ),
                ]
            } else if i == last {
                vec![(
                    tr("inspector.spline.tangent"),
                    n.tangent_in,
                    n.tangent_in_explicit,
                    false,
                )]
            } else {
                vec![(
                    tr("inspector.spline.tangent"),
                    n.tangent_out,
                    n.tangent_out_explicit,
                    false,
                )]
            };
            for (title, vector, explicit, incoming) in rows {
                let (edit, done) = tangent_row(ui, &title, vector, explicit);
                group_done |= done;
                if let (Some(edit), None) = (edit, &action) {
                    let value = match edit {
                        TangentEdit::Set(v) => {
                            dragging = true;
                            Some(v)
                        }
                        TangentEdit::Reset => None,
                    };
                    let mut node = current;
                    if incoming {
                        node.tangent_in = value;
                    } else {
                        node.tangent = value;
                    }
                    action = Some(set(node));
                }
            }
            ui.horizontal(|ui| {
                if i != last
                    && ui
                        .small_button(tr("inspector.spline.insert"))
                        .on_hover_text(tr("inspector.spline.insert.hover"))
                        .clicked()
                {
                    action = Some(Command::AddSplinePoint {
                        sheet: sheet_id,
                        spline: id,
                        index: i + 1,
                        point: PointRef::New(mids[i]),
                    });
                }
                if resolved.len() > 2
                    && ui
                        .small_button(tr("inspector.spline.remove"))
                        .on_hover_text(tr("inspector.spline.remove.hover"))
                        .clicked()
                {
                    action = Some(Command::RemoveSplinePoint {
                        sheet: sheet_id,
                        spline: id,
                        index: i,
                    });
                }
            });
        }
        if resolved.len() == 2
            && ui
                .button(tr("inspector.spline.to_curve"))
                .on_hover_text(tr("inspector.spline.to_curve.hover"))
                .clicked()
        {
            action = Some(Command::SplineToCurve {
                sheet: sheet_id,
                spline: id,
            });
        }
        crate::a11y::label(
            ui,
            RichText::new(tr("inspector.spline.note"))
                .small()
                .color(pal().muted),
        );
        if let Some(cmd) = action {
            if dragging {
                // Faire glisser une valeur : une seule étape d'historique.
                self.editor.doc.begin_group();
                if let Err(e) = self.editor.doc.execute(cmd) {
                    self.editor.error(core_error(&e));
                }
            } else {
                self.editor.exec(cmd);
            }
        }
        if group_done {
            self.editor.doc.end_group();
        }
    }

    /// Inspecteur d'un point relatif (US-02.5) : parents nommés, paramètres
    /// modifiables par `SetPointValues` (un seul pas d'historique par geste).
    fn inspect_relative_point(
        &mut self,
        ui: &mut egui::Ui,
        id: PointId,
        definition: PointDefinition,
        pos: pf_geometry::PointMm,
    ) {
        let sheet_id = self.editor.sheet;
        let s = self.editor.sheet();
        let lbl = |p: PointId| s.label(p).to_string();
        let (title, desc, params): (String, String, Vec<(String, f64, &str)>) = match definition {
            PointDefinition::AlongCurve {
                curve,
                distance_mm,
                origin,
            } => (
                tr("inspector.point.along_curve.title"),
                tr_args(
                    "inspector.point.along_curve.desc",
                    &[
                        ("name", &entity_name(s, curve)),
                        (
                            "from",
                            &if origin == SegmentEnd::Start {
                                tr("inspector.point.start")
                            } else {
                                tr("inspector.point.end")
                            },
                        ),
                    ],
                ),
                vec![(tr("palette.distance_along"), distance_mm, "mm")],
            ),
            PointDefinition::Offset { from, dx_mm, dy_mm } => (
                tr("inspector.point.offset.title"),
                tr_args("inspector.point.offset.desc", &[("from", &lbl(from))]),
                vec![
                    (tr("palette.dx"), dx_mm, "mm"),
                    (tr("palette.dy"), dy_mm, "mm"),
                ],
            ),
            PointDefinition::Midpoint { first, second } => (
                tr("inspector.point.midpoint.title"),
                tr_args(
                    "inspector.point.midpoint.desc",
                    &[("first", &lbl(first)), ("second", &lbl(second))],
                ),
                Vec::new(),
            ),
            PointDefinition::Projection { point, trace } => (
                tr("inspector.point.projection.title"),
                tr_args(
                    "inspector.point.projection.desc",
                    &[("point", &lbl(point)), ("name", &entity_name(s, trace))],
                ),
                Vec::new(),
            ),
            PointDefinition::Bisector {
                vertex,
                first,
                second,
                distance_mm,
            } => (
                tr("inspector.point.bisector.title"),
                tr_args(
                    "inspector.point.bisector.desc",
                    &[
                        ("first", &lbl(first)),
                        ("vertex", &lbl(vertex)),
                        ("second", &lbl(second)),
                    ],
                ),
                vec![(tr("palette.distance"), distance_mm, "mm")],
            ),
            PointDefinition::Polar {
                from,
                distance_mm,
                angle_deg,
                reference,
            } => (
                tr("inspector.point.direction.title"),
                match reference {
                    None => tr_args(
                        "inspector.point.direction.desc_abs",
                        &[("from", &lbl(from))],
                    ),
                    Some(r) => tr_args(
                        "inspector.point.direction.desc_rel",
                        &[("from", &lbl(from)), ("name", &entity_name(s, r))],
                    ),
                },
                vec![
                    (tr("palette.distance"), distance_mm, "mm"),
                    (tr("palette.angle"), angle_deg, "°"),
                ],
            ),
            PointDefinition::Compass {
                center_a,
                center_b,
                radius_a_mm,
                radius_b_mm,
                ..
            } => (
                tr("inspector.point.compass.title"),
                tr_args(
                    "inspector.point.compass.desc",
                    &[("a", &lbl(center_a)), ("b", &lbl(center_b))],
                ),
                vec![
                    (tr("palette.radius_a"), radius_a_mm, "mm"),
                    (tr("palette.radius_b"), radius_b_mm, "mm"),
                ],
            ),
            _ => return,
        };
        section(ui, &title);
        crate::a11y::label(ui, desc);
        // Formules maintenues (US-03.1b) : paramètres qui en acceptent une, et formule liée.
        let kinds = crate::formulas::kinds_of(&definition);
        let bound: Vec<Option<String>> = (0..params.len())
            .map(|i| {
                kinds
                    .get(i)
                    .copied()
                    .flatten()
                    .and_then(|k| self.editor.sheet().formula(id, k).map(|f| f.expr.clone()))
            })
            .collect();
        let mut values: Vec<f64> = params.iter().map(|p| p.1).collect();
        let (mut changed, mut done) = (false, false);
        egui::Grid::new("relative-params")
            .num_columns(2)
            .show(ui, |ui| {
                for (i, ((label, _, unit), v)) in params.iter().zip(values.iter_mut()).enumerate() {
                    crate::a11y::label(ui, label);
                    if let Some(expr) = &bound[i] {
                        // Valeur calculée, en lecture seule, à côté de sa formule.
                        ui.horizontal(|ui| {
                            crate::a11y::label(
                                ui,
                                RichText::new(App::kept_value_text(*v, unit)).monospace(),
                            );
                            crate::a11y::label(
                                ui,
                                RichText::new(format!("= {expr}"))
                                    .monospace()
                                    .color(pal().muted),
                            );
                        });
                        ui.end_row();
                        continue;
                    }
                    let drag = egui::DragValue::new(v)
                        .localized()
                        .speed(0.1)
                        .suffix(format!(" {unit}"))
                        .max_decimals(3);
                    let drag = if *unit == "mm" {
                        crate::views::length_drag(drag)
                    } else {
                        drag
                    };
                    let r = crate::a11y::add_drag(ui, drag);
                    changed |= r.changed();
                    done |= r.drag_stopped() || r.lost_focus();
                    ui.end_row();
                }
            });
        if changed {
            self.editor.doc.begin_group();
            let cmd = Command::SetPointValues {
                sheet: sheet_id,
                point: id,
                values,
            };
            if let Err(e) = self.editor.doc.execute(cmd) {
                self.editor.error(core_error(&e));
            }
        }
        if done {
            self.editor.doc.end_group();
        }
        let rows: Vec<crate::formulas::ParamRow> = params
            .iter()
            .enumerate()
            .filter_map(|(i, (label, v, _))| {
                kinds
                    .get(i)
                    .copied()
                    .flatten()
                    .map(|kind| crate::formulas::ParamRow {
                        kind,
                        label: label.clone(),
                        value: *v,
                    })
            })
            .collect();
        if !rows.is_empty() {
            self.formula_rows(ui, id, &rows);
        }
        crate::a11y::label(
            ui,
            RichText::new(tr("inspector.point.relative.note"))
                .small()
                .color(pal().muted),
        );
        row(
            ui,
            &tr("inspector.position"),
            &pf_core::units::len_pair(pos.x, pos.y, 2),
        );
    }

    fn inspect_point(&mut self, ui: &mut egui::Ui, id: PointId) {
        let sheet_id = self.editor.sheet;
        let s = self.editor.sheet();
        let Ok(point) = s.point(id).cloned() else {
            return;
        };
        let Ok(pos) = s.position(id) else { return };
        ui.heading(tr_args("inspector.point.title", &[("label", &point.label)]));
        ui.horizontal(|ui| {
            crate::a11y::label(ui, tr("inspector.point.label"));
            if let Some(label) =
                buffered_edit(ui, &mut self.text_buf, (id.0 << 8) | 4, &point.label, 90.0)
            {
                self.editor.exec(Command::SetPointLabel {
                    sheet: sheet_id,
                    point: id,
                    label,
                });
            }
        });
        match point.definition {
            PointDefinition::Free { .. } => {
                section(ui, &tr("inspector.point.free.title"));
                self.kept_point_note(ui, id);
                self.imposed_point_note(ui, id);
                let (mut x, mut y) = (pos.x, pos.y);
                let mut changed = false;
                let mut done = false;
                egui::Grid::new("pos").num_columns(2).show(ui, |ui| {
                    for (label, v) in [("X", &mut x), ("Y", &mut y)] {
                        crate::a11y::label(ui, label);
                        let r = crate::a11y::add_drag(
                            ui,
                            crate::views::length_drag(
                                egui::DragValue::new(v)
                                    .localized()
                                    .speed(0.1)
                                    .suffix(" mm")
                                    .max_decimals(3),
                            ),
                        );
                        changed |= r.changed();
                        done |= r.drag_stopped() || r.lost_focus();
                        ui.end_row();
                    }
                });
                if changed {
                    self.editor.doc.begin_group();
                    let to = pf_geometry::PointMm::new(x, y);
                    if let Err(e) = self.editor.doc.execute(Command::MovePoint {
                        sheet: sheet_id,
                        point: id,
                        to,
                    }) {
                        self.editor.error(core_error(&e));
                    }
                }
                if done {
                    self.editor.doc.end_group();
                }
            }
            PointDefinition::OnSegment {
                segment,
                distance_mm,
                origin,
            } => {
                section(ui, &tr("inspector.point.on_segment.title"));
                let name = entity_name(self.editor.sheet(), segment);
                let from = self
                    .editor
                    .sheet()
                    .segment_point_ids(segment)
                    .map(|(a, b)| if origin == SegmentEnd::Start { a } else { b })
                    .map(|p| self.editor.sheet().label(p).to_string())
                    .unwrap_or_default();
                crate::a11y::label(
                    ui,
                    tr_args(
                        "inspector.point.on_segment.desc",
                        &[("name", &name), ("from", &from)],
                    ),
                );
                let mut d = distance_mm;
                if let Some(expr) = self.kept_formula_text(id, ParamKind::SegmentDistance) {
                    self.kept_value_line(ui, ParamKind::SegmentDistance, d, &expr);
                } else {
                    let r = crate::a11y::add_drag(
                        ui,
                        crate::views::length_drag(
                            egui::DragValue::new(&mut d)
                                .localized()
                                .speed(0.1)
                                .suffix(" mm")
                                .max_decimals(3)
                                .range(0.0..=1.0e6),
                        ),
                    );
                    if r.changed() {
                        self.editor.doc.begin_group();
                        let cmd = Command::SetPointDistance {
                            sheet: sheet_id,
                            point: id,
                            distance_mm: d,
                        };
                        if let Err(e) = self.editor.doc.execute(cmd) {
                            self.editor.error(core_error(&e));
                        }
                    }
                    if r.drag_stopped() || r.lost_focus() {
                        self.editor.doc.end_group();
                    }
                }
                self.formula_rows(
                    ui,
                    id,
                    &[crate::formulas::ParamRow {
                        kind: ParamKind::SegmentDistance,
                        label: tr("inspector.formula.param.segment_distance"),
                        value: distance_mm,
                    }],
                );
                crate::a11y::label(
                    ui,
                    RichText::new(tr("inspector.point.on_segment.note"))
                        .small()
                        .color(pal().muted),
                );
                row(
                    ui,
                    &tr("inspector.position"),
                    &pf_core::units::len_pair(pos.x, pos.y, 2),
                );
            }
            PointDefinition::OnSegmentRatio { segment, ratio }
            | PointDefinition::OnCurve {
                curve: segment,
                t: ratio,
            } => {
                let on_curve = matches!(point.definition, PointDefinition::OnCurve { .. });
                section(
                    ui,
                    &if on_curve {
                        tr("inspector.point.on_curve.title")
                    } else {
                        tr("inspector.point.ratio.title")
                    },
                );
                let name = entity_name(self.editor.sheet(), segment);
                crate::a11y::label(
                    ui,
                    tr_args("inspector.point.ratio.desc", &[("name", &name)]),
                );
                let ratio_kind = if on_curve {
                    ParamKind::CurveRatio
                } else {
                    ParamKind::SegmentRatio
                };
                let mut percent = ratio * 100.0;
                if let Some(expr) = self.kept_formula_text(id, ratio_kind) {
                    self.kept_value_line(ui, ratio_kind, ratio, &expr);
                } else {
                    let r = crate::a11y::add_drag(
                        ui,
                        egui::DragValue::new(&mut percent)
                            .localized()
                            .speed(0.2)
                            .suffix(" %")
                            .max_decimals(2)
                            .range(0.0..=100.0),
                    );
                    if r.changed() {
                        self.editor.doc.begin_group();
                        let cmd = Command::SetPointParameter {
                            sheet: sheet_id,
                            point: id,
                            value: percent / 100.0,
                        };
                        if let Err(e) = self.editor.doc.execute(cmd) {
                            self.editor.error(core_error(&e));
                        }
                    }
                    if r.drag_stopped() || r.lost_focus() {
                        self.editor.doc.end_group();
                    }
                }
                self.formula_rows(
                    ui,
                    id,
                    &[crate::formulas::ParamRow {
                        kind: ratio_kind,
                        label: tr(if on_curve {
                            "inspector.formula.param.curve_ratio"
                        } else {
                            "inspector.formula.param.segment_ratio"
                        }),
                        value: ratio,
                    }],
                );
                crate::a11y::label(
                    ui,
                    RichText::new(if on_curve {
                        tr("inspector.point.on_curve.note")
                    } else {
                        tr("inspector.point.ratio.note")
                    })
                    .small()
                    .color(pal().muted),
                );
                row(
                    ui,
                    &tr("inspector.position"),
                    &pf_core::units::len_pair(pos.x, pos.y, 2),
                );
            }
            PointDefinition::CircleIntersection { first, second, .. } => {
                section(ui, &tr("inspector.point.round_intersection.title"));
                let s = self.editor.sheet();
                crate::a11y::label(
                    ui,
                    tr_args(
                        "inspector.point.intersection.desc",
                        &[
                            ("first", &entity_name(s, first)),
                            ("second", &entity_name(s, second)),
                        ],
                    ),
                );
                crate::a11y::label(
                    ui,
                    RichText::new(tr("inspector.point.round_intersection.note"))
                        .small()
                        .color(pal().muted),
                );
                row(
                    ui,
                    &tr("inspector.position"),
                    &pf_core::units::len_pair(pos.x, pos.y, 2),
                );
            }
            PointDefinition::TraceIntersection { first, second, .. } => {
                section(ui, &tr("inspector.point.trace_intersection.title"));
                let s = self.editor.sheet();
                crate::a11y::label(
                    ui,
                    tr_args(
                        "inspector.point.intersection.desc",
                        &[
                            ("first", &entity_name(s, first)),
                            ("second", &entity_name(s, second)),
                        ],
                    ),
                );
                crate::a11y::label(
                    ui,
                    RichText::new(tr("inspector.point.trace_intersection.note"))
                        .small()
                        .color(pal().muted),
                );
                row(
                    ui,
                    &tr("inspector.position"),
                    &pf_core::units::len_pair(pos.x, pos.y, 2),
                );
            }
            PointDefinition::Mirror { source, .. } => {
                section(ui, &tr("inspector.point.mirror.title"));
                let name = self.editor.sheet().label(source).to_string();
                crate::a11y::label(
                    ui,
                    tr_args("inspector.point.mirror.desc", &[("name", &name)]),
                );
                crate::a11y::label(
                    ui,
                    RichText::new(tr("inspector.point.mirror.note"))
                        .small()
                        .color(pal().muted),
                );
                row(
                    ui,
                    &tr("inspector.position"),
                    &pf_core::units::len_pair(pos.x, pos.y, 2),
                );
            }
            PointDefinition::Division { trace, ratio, .. } => {
                section(ui, &tr("inspector.point.division.title"));
                let name = entity_name(self.editor.sheet(), trace);
                crate::a11y::label(
                    ui,
                    tr_args(
                        "inspector.point.division.desc",
                        &[
                            ("name", &name),
                            ("percent", &format_number(ratio * 100.0, 2)),
                        ],
                    ),
                );
                if let Some(expr) = self.kept_formula_text(id, ParamKind::DivisionRatio) {
                    self.kept_value_line(ui, ParamKind::DivisionRatio, ratio, &expr);
                }
                self.formula_rows(
                    ui,
                    id,
                    &[crate::formulas::ParamRow {
                        kind: ParamKind::DivisionRatio,
                        label: tr("inspector.formula.param.division_ratio"),
                        value: ratio,
                    }],
                );
                crate::a11y::label(
                    ui,
                    RichText::new(tr("inspector.point.division.note"))
                        .small()
                        .color(pal().muted),
                );
                row(
                    ui,
                    &tr("inspector.position"),
                    &pf_core::units::len_pair(pos.x, pos.y, 2),
                );
            }
            PointDefinition::AlongCurve { .. }
            | PointDefinition::Offset { .. }
            | PointDefinition::Midpoint { .. }
            | PointDefinition::Projection { .. }
            | PointDefinition::Bisector { .. }
            | PointDefinition::Polar { .. }
            | PointDefinition::Compass { .. } => {
                self.inspect_relative_point(ui, id, point.definition, pos);
            }
            PointDefinition::Intersection { first, second } => {
                section(ui, &tr("inspector.point.intersection.title"));
                let s = self.editor.sheet();
                crate::a11y::label(
                    ui,
                    tr_args(
                        "inspector.point.intersection.desc",
                        &[
                            ("first", &entity_name(s, first)),
                            ("second", &entity_name(s, second)),
                        ],
                    ),
                );
                crate::a11y::label(
                    ui,
                    RichText::new(tr("inspector.point.intersection.note"))
                        .small()
                        .color(pal().muted),
                );
                row(
                    ui,
                    &tr("inspector.position"),
                    &pf_core::units::len_pair(pos.x, pos.y, 2),
                );
            }
        }
        section(ui, &tr("inspector.point.role.title"));
        let mut role = point.role;
        ui.horizontal(|ui| {
            ui.selectable_value(
                &mut role,
                PointRole::Construction,
                point_role_label(PointRole::Construction),
            );
            ui.selectable_value(
                &mut role,
                PointRole::Reference,
                point_role_label(PointRole::Reference),
            );
        });
        if role != point.role {
            self.editor.exec(Command::SetPointRole {
                sheet: sheet_id,
                point: id,
                role,
            });
        }
    }

    fn inspect_notch(&mut self, ui: &mut egui::Ui, id: pf_core::NotchId) {
        use pf_ui::components::{NumericField, Unit};
        let sheet_id = self.editor.sheet;
        let s = self.editor.sheet();
        let Ok(notch) = s.notch(id).cloned() else {
            return;
        };
        let path_len = s.path_length_mm(notch.entity).unwrap_or(0.0);
        let carrier = entity_name(s, notch.entity);
        ui.heading(tr("inspector.notch.title"));
        row(ui, &tr("inspector.notch.on"), &carrier);
        row(ui, &tr("inspector.notch.path_length"), &format_mm(path_len));
        section(ui, &tr("inspector.notch.section"));
        // Position effective : celle du cran ancré est calculée (US-06.2), non saisie.
        let anchored = notch.anchor.is_some();
        let (mut distance, mut length) = (
            s.notch_distance_mm(&notch).unwrap_or(notch.distance_mm),
            notch.length_mm,
        );
        let (mut side, mut style) = (notch.side, notch.style);
        let mut changed = false;
        egui::Grid::new("notch-grid").num_columns(2).show(ui, |ui| {
            if !anchored {
                crate::a11y::label(ui, tr("palette.distance_start"));
                changed |= NumericField::new(&mut distance, Unit::Mm)
                    .range(0.0..=path_len)
                    .decimals(3)
                    .id_salt(("notch-distance", id.0))
                    .show(ui)
                    .committed;
                ui.end_row();
            }
            crate::a11y::label(ui, tr("palette.notch_length"));
            changed |= NumericField::new(&mut length, Unit::Mm)
                .range(0.5..=pf_core::notch::MAX_NOTCH_LENGTH_MM)
                .decimals(2)
                .id_salt(("notch-length", id.0))
                .show(ui)
                .committed;
            ui.end_row();
        });
        ui.horizontal(|ui| {
            crate::a11y::label(ui, tr("inspector.notch.side"));
            changed |= ui
                .selectable_value(&mut side, Side::Left, tr("inspector.notch.left"))
                .changed();
            changed |= ui
                .selectable_value(&mut side, Side::Right, tr("inspector.notch.right"))
                .changed();
        });
        ui.horizontal(|ui| {
            crate::a11y::label(ui, tr("inspector.notch.style"));
            changed |= ui
                .selectable_value(
                    &mut style,
                    pf_core::NotchStyle::Single,
                    tr("inspector.notch.single"),
                )
                .changed();
            changed |= ui
                .selectable_value(
                    &mut style,
                    pf_core::NotchStyle::Double,
                    tr("inspector.notch.double"),
                )
                .changed();
        });
        crate::a11y::label(
            ui,
            RichText::new(tr("inspector.notch.note"))
                .small()
                .color(pal().muted),
        );
        if changed {
            self.editor.exec(Command::SetNotch {
                sheet: sheet_id,
                notch: id,
                distance_mm: distance,
                side,
                length_mm: length,
                style,
            });
        }
        self.notch_shape_block(ui, id);
        self.notch_anchor_section(ui, id);
    }

    /// Inspecteur d'une ligne d'aide : point de passage, angle, axe de symétrie.
    fn inspect_guide(&mut self, ui: &mut egui::Ui, id: pf_core::GuideId) {
        use pf_ui::components::{NumericField, Unit};
        let sheet_id = self.editor.sheet;
        let s = self.editor.sheet();
        let Ok(g) = s.guide(id).cloned() else {
            return;
        };
        let layer = match g.layer.and_then(|l| s.layer(l).ok()) {
            Some(l) => l.name.clone(),
            None => tr("inspector.guide.layer_default"),
        };
        ui.heading(tr("inspector.guide.title"));
        row(
            ui,
            &tr("inspector.guide.kind"),
            &if g.angle_deg == 0.0 {
                tr("inspector.guide.kind.horizontal")
            } else if g.angle_deg == 90.0 {
                tr("inspector.guide.kind.vertical")
            } else {
                tr("inspector.guide.kind.oblique")
            },
        );
        row(ui, &tr("inspector.guide.layer"), &layer);
        crate::a11y::label(
            ui,
            RichText::new(tr("inspector.guide.note"))
                .small()
                .color(pal().muted),
        );
        section(ui, &tr("inspector.guide.section"));
        let (mut x, mut y, mut angle, mut axis) = (g.x_mm, g.y_mm, g.angle_deg, g.axis);
        let mut moved = false;
        egui::Grid::new("guide-grid").num_columns(2).show(ui, |ui| {
            for (label, value, unit, salt) in [
                ("inspector.guide.x", &mut x, Unit::Mm, "guide-x"),
                ("inspector.guide.y", &mut y, Unit::Mm, "guide-y"),
                (
                    "inspector.guide.angle",
                    &mut angle,
                    Unit::Deg,
                    "guide-angle",
                ),
            ] {
                crate::a11y::label(ui, tr(label));
                moved |= NumericField::new(value, unit)
                    .decimals(2)
                    .id_salt((salt, id.0))
                    .show(ui)
                    .committed;
                ui.end_row();
            }
        });
        if moved {
            self.editor.exec(Command::MoveGuide {
                sheet: sheet_id,
                guide: id,
                x_mm: x,
                y_mm: y,
                angle_deg: angle,
            });
        }
        if ui.checkbox(&mut axis, tr("inspector.guide.axis")).changed() {
            self.editor.exec(Command::SetGuideAxis {
                sheet: sheet_id,
                guide: id,
                axis,
            });
        }
    }

    fn inspect_dimension(&mut self, ui: &mut egui::Ui, id: pf_core::DimensionId) {
        use pf_ui::components::{NumericField, Unit};
        let sheet_id = self.editor.sheet;
        let s = self.editor.sheet();
        let Ok(dim) = s.dimension(id).cloned() else {
            return;
        };
        let value = s.dimension_geometry(id).map(|g| g.value_mm);
        ui.heading(tr("inspector.dimension.title"));
        row(
            ui,
            &tr("inspector.dimension.between"),
            &tr_args(
                "inspector.dimension.points",
                &[("a", s.label(dim.from)), ("b", s.label(dim.to))],
            ),
        );
        match value {
            Ok(v) => row(ui, &tr("inspector.dimension.value"), &format_mm(v)),
            Err(e) => {
                crate::a11y::label(
                    ui,
                    RichText::new(crate::text::core_error(&e)).color(pal().error),
                );
            }
        }
        crate::a11y::label(
            ui,
            RichText::new(tr("inspector.dimension.note"))
                .small()
                .color(pal().muted),
        );
        section(ui, &tr("inspector.dimension.section"));
        let mut offset = dim.offset_mm;
        let mut changed = false;
        egui::Grid::new("dimension-grid")
            .num_columns(2)
            .show(ui, |ui| {
                crate::a11y::label(ui, tr("inspector.dimension.offset"));
                changed |= NumericField::new(&mut offset, Unit::Mm)
                    .range(
                        -pf_core::dimension::MAX_DIMENSION_OFFSET_MM
                            ..=pf_core::dimension::MAX_DIMENSION_OFFSET_MM,
                    )
                    .decimals(2)
                    .id_salt(("dimension-offset", id.0))
                    .show(ui)
                    .committed;
                ui.end_row();
            });
        if changed {
            self.editor.exec(Command::SetDimensionOffset {
                sheet: sheet_id,
                dimension: id,
                offset_mm: offset,
            });
        }
    }

    fn inspect_pair(&mut self, ui: &mut egui::Ui, a: EntityId, b: EntityId) {
        let s = self.editor.sheet();
        ui.heading(tr("inspector.pair.title"));
        section(ui, &tr("inspector.section.measures"));
        for id in [a, b] {
            if let Ok(m) = measure::entity_length(s, id) {
                row(ui, &entity_name(s, id), &format_mm(m.value));
            }
        }
        match measure::angle_between_segments(s, a, b) {
            Ok(m) => {
                row(ui, &tr("inspector.pair.angle"), &format_deg(m.value));
                crate::a11y::label(
                    ui,
                    RichText::new(tr("inspector.pair.angle_note"))
                        .small()
                        .color(pal().muted),
                );
            }
            Err(_) => {
                crate::a11y::label(
                    ui,
                    RichText::new(tr("inspector.pair.angle_unavailable"))
                        .small()
                        .color(pal().muted),
                );
            }
        }
    }

    fn inspect_many(&mut self, ui: &mut egui::Ui) {
        let mut create_piece = false;
        self.inspect_many_rows(ui, &mut create_piece);
        if create_piece {
            self.editor.piece_from_selection();
        }
    }

    fn inspect_many_rows(&self, ui: &mut egui::Ui, create_piece: &mut bool) {
        let s = self.editor.sheet();
        let sel = &self.editor.selection;
        ui.heading(tr_args(
            "inspector.many.title",
            &[("count", &sel.len().to_string())],
        ));
        row(
            ui,
            &tr("inspector.sheet.points"),
            &sel.points.len().to_string(),
        );
        row(
            ui,
            &tr("inspector.sheet.paths"),
            &sel.entities.len().to_string(),
        );
        let total: f64 = sel
            .entities
            .iter()
            .filter_map(|id| measure::entity_length(s, *id).ok())
            .map(|m| m.value)
            .sum();
        if !sel.entities.is_empty() {
            section(ui, &tr("inspector.section.measure"));
            row(ui, &tr("inspector.many.total"), &format_mm(total));
            if sel.entities.len() >= 2 {
                match contour::contour_from_entities(s, &sel.entities)
                    .and_then(|c| contour::measure_contour(s, &c).map(|m| (c, m)))
                {
                    Ok((c, m)) => {
                        contour_rows(ui, &tr("inspector.contour.single"), &c, &m);
                        if ui
                            .button(tr("piece.from_selection"))
                            .on_hover_text(tr("piece.from_selection.hover"))
                            .clicked()
                        {
                            *create_piece = true;
                        }
                    }
                    Err(e) => {
                        crate::a11y::label(
                            ui,
                            RichText::new(tr_args(
                                "inspector.contour.none",
                                &[("detail", e.to_string().trim_end_matches('.'))],
                            ))
                            .small()
                            .color(pal().muted),
                        );
                    }
                }
            }
        }
    }
}
