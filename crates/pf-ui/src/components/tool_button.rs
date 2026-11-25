use super::Interaction;
use crate::theme;
use egui::{
    Color32, CornerRadius, FontId, Rect, Response, Sense, Stroke, StrokeKind, Ui, WidgetInfo,
    WidgetType,
};

/// Bouton d'outil compact : libellé, raccourci affiché à droite, infobulle.
///
/// L'état sélectionné est signalé par le fond, le contour et une barre
/// d'accent à gauche (jamais par la couleur seule).
pub fn tool_button(ui: &mut Ui, label: &str, shortcut: Option<&str>, selected: bool) -> Response {
    tool_button_tip(ui, label, shortcut, selected, None)
}

/// Comme `tool_button`, avec une aide sous le libellé de l'infobulle (geste attendu, US-16.9).
pub fn tool_button_tip(
    ui: &mut Ui,
    label: &str,
    shortcut: Option<&str>,
    selected: bool,
    help: Option<&str>,
) -> Response {
    let t = theme::tokens(ui.ctx());
    let enabled = ui.is_enabled();
    let label_galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        FontId::proportional(t.text.body),
        Color32::PLACEHOLDER,
    );
    let shortcut_galley = shortcut.map(|s| {
        ui.painter().layout_no_wrap(
            s.to_owned(),
            FontId::monospace(t.text.caption),
            Color32::PLACEHOLDER,
        )
    });
    let gap = t.space.l;
    let pad = t.space.m;
    let content_w =
        label_galley.size().x + shortcut_galley.as_ref().map_or(0.0, |g| gap + g.size().x);
    let size = egui::vec2((content_w + 2.0 * pad + t.space.s).max(72.0), 28.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    response.widget_info(|| WidgetInfo::selected(WidgetType::Button, enabled, selected, label));
    if ui.is_rect_visible(rect) {
        let state = Interaction::of(&response, enabled);
        let (fill, stroke, text, hint) = match (state, selected) {
            (Interaction::Disabled, _) => {
                (t.surface_panel, t.border, t.text_disabled, t.text_disabled)
            }
            (Interaction::Pressed, _) => (
                t.selection_bg,
                t.action_pressed,
                t.text_primary,
                t.text_secondary,
            ),
            (_, true) => (t.selection_bg, t.action, t.text_primary, t.text_secondary),
            (Interaction::Hover, false) => (
                t.selection_bg,
                t.border_strong,
                t.text_primary,
                t.text_secondary,
            ),
            (Interaction::Normal, false) => {
                (t.surface_raised, t.border, t.text_primary, t.text_secondary)
            }
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
        if selected && enabled {
            let bar = Rect::from_min_max(
                rect.min + egui::vec2(1.0, 4.0),
                egui::pos2(rect.min.x + 4.0, rect.max.y - 4.0),
            );
            painter.rect_filled(bar, CornerRadius::same(t.radius.s), t.action);
        }
        if response.has_focus() {
            painter.rect_stroke(
                rect.expand(2.0),
                CornerRadius::same(t.radius.m + 2),
                Stroke::new(2.0_f32, t.focus_ring),
                StrokeKind::Outside,
            );
        }
        let left = rect.min.x + pad + t.space.s;
        painter.galley(
            egui::pos2(left, rect.center().y - label_galley.size().y / 2.0),
            label_galley,
            text,
        );
        if let Some(g) = shortcut_galley {
            painter.galley(
                egui::pos2(
                    rect.max.x - pad - g.size().x,
                    rect.center().y - g.size().y / 2.0,
                ),
                g,
                hint,
            );
        }
    }
    let mut tip = match shortcut {
        Some(s) => format!("{label} ({s})"),
        None => label.to_owned(),
    };
    if let Some(help) = help {
        tip.push('\n');
        tip.push_str(help);
    }
    response.on_hover_text(tip)
}
