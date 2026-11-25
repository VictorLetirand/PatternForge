use super::Interaction;
use crate::theme;
use egui::{
    Color32, CornerRadius, FontId, Response, Sense, Stroke, StrokeKind, Ui, WidgetInfo, WidgetType,
};

/// Variante visuelle d'un bouton.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonKind {
    /// Action principale (plein, couleur d'action).
    Primary,
    /// Action secondaire / annulation (contour).
    Secondary,
    /// Action destructive (contour et texte danger ; plein au survol).
    Danger,
}

/// Bouton textuel à états complets (normal, survol, pressé, focus, désactivé).
pub fn button(ui: &mut Ui, label: &str, kind: ButtonKind) -> Response {
    let t = theme::tokens(ui.ctx());
    let enabled = ui.is_enabled();
    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        FontId::proportional(t.text.body),
        Color32::PLACEHOLDER,
    );
    let size = egui::vec2(
        galley.size().x + 2.0 * t.space.l,
        galley.size().y + 2.0 * t.space.s + 2.0,
    );
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, enabled, label));
    if ui.is_rect_visible(rect) {
        let state = Interaction::of(&response, enabled);
        let (fill, stroke, text) = match (kind, state) {
            (_, Interaction::Disabled) => (t.surface_panel, t.border, t.text_disabled),
            (ButtonKind::Primary, Interaction::Normal) => {
                (t.action_strong, t.action_strong, t.action_text)
            }
            (ButtonKind::Primary, Interaction::Hover) => {
                (t.action_hover, t.action_hover, t.action_text)
            }
            (ButtonKind::Primary, Interaction::Pressed) => {
                (t.action_pressed, t.action_pressed, t.action_text)
            }
            (ButtonKind::Secondary, Interaction::Normal) => {
                (t.surface_raised, t.border_strong, t.text_primary)
            }
            (ButtonKind::Secondary, Interaction::Hover) => {
                (t.selection_bg, t.action, t.text_primary)
            }
            (ButtonKind::Secondary, Interaction::Pressed) => {
                (t.selection_bg, t.action_pressed, t.text_primary)
            }
            (ButtonKind::Danger, Interaction::Normal) => (t.surface_raised, t.danger, t.danger),
            (ButtonKind::Danger, _) => (t.danger, t.danger, t.surface_raised),
        };
        let radius = CornerRadius::same(t.radius.m);
        let painter = ui.painter();
        painter.rect(
            rect,
            radius,
            fill,
            Stroke::new(1.0_f32, stroke),
            StrokeKind::Inside,
        );
        if response.has_focus() {
            painter.rect_stroke(
                rect.expand(2.0),
                CornerRadius::same(t.radius.m + 2),
                Stroke::new(2.0_f32, t.focus_ring),
                StrokeKind::Outside,
            );
        }
        painter.galley(rect.center() - galley.size() / 2.0, galley, text);
    }
    if enabled {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    }
}
