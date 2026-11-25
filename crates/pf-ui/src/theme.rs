//! Application des jetons à `egui::Style` et relecture depuis les composants.

use crate::tokens::Tokens;
use egui::{
    Context, CornerRadius, FontFamily, FontId, Id, Margin, Stroke, TextStyle, Theme,
    ThemePreference, Vec2,
};
use std::collections::BTreeMap;

fn store_id() -> Id {
    Id::new("pf_ui::tokens")
}

/// Jetons courants du contexte (Precision Light si [`apply`] n'a jamais été appelé).
pub fn tokens(ctx: &Context) -> Tokens {
    ctx.data(|d| d.get_temp::<Tokens>(store_id()))
        .unwrap_or_else(Tokens::precision_light)
}

/// Configure le style egui depuis les jetons et les mémorise pour [`tokens`].
/// Sans effet si ces mêmes jetons sont déjà appliqués (appel possible à chaque image).
pub fn apply(ctx: &Context, tokens: &Tokens) {
    if ctx.data(|d| d.get_temp::<Tokens>(store_id())).as_ref() == Some(tokens) {
        return;
    }
    let theme = if tokens.dark {
        Theme::Dark
    } else {
        Theme::Light
    };
    ctx.set_theme(if tokens.dark {
        ThemePreference::Dark
    } else {
        ThemePreference::Light
    });
    ctx.set_style_of(theme, build_style(tokens, theme));
    ctx.data_mut(|d| d.insert_temp(store_id(), tokens.clone()));
}

/// Épaisseur de l'anneau de focus clavier (points logiques, 2 au moins : US-16.8).
pub const FOCUS_RING_WIDTH: f32 = 2.0;

/// Anneau de focus du widget qui a le focus clavier : rectangle (agrandi de 1 point) et trait.
/// `None` si rien n'a le focus ou s'il est hors de vue. Calculé depuis la réponse de l'image
/// précédente.
pub fn focus_ring(ctx: &Context) -> Option<(egui::Rect, Stroke)> {
    let id = ctx.memory(|m| m.focused())?;
    // Partie visible du widget : un widget masqué par un défilement n'a pas d'anneau.
    let rect = ctx.read_response(id)?.interact_rect;
    if !rect.is_finite() || rect.width() <= 0.0 || rect.height() <= 0.0 {
        return None;
    }
    let stroke = Stroke::new(FOCUS_RING_WIDTH, tokens(ctx).focus_ring);
    Some((rect.expand(1.0), stroke))
}

/// Dessine l'anneau de focus au premier plan (à appeler en fin d'image).
pub fn paint_focus_ring(ctx: &Context) {
    let Some((rect, stroke)) = focus_ring(ctx) else {
        return;
    };
    let layer = egui::LayerId::new(egui::Order::Foreground, Id::new("pf_ui::focus_ring"));
    ctx.layer_painter(layer).rect_stroke(
        rect,
        CornerRadius::same(3),
        stroke,
        egui::StrokeKind::Outside,
    );
}

fn build_style(t: &Tokens, theme: Theme) -> egui::Style {
    let mut style = egui::Style {
        visuals: match theme {
            Theme::Dark => egui::Visuals::dark(),
            Theme::Light => egui::Visuals::light(),
        },
        ..Default::default()
    };

    let text_styles: BTreeMap<TextStyle, FontId> = [
        (
            TextStyle::Small,
            FontId::new(t.text.caption, FontFamily::Proportional),
        ),
        (
            TextStyle::Body,
            FontId::new(t.text.body, FontFamily::Proportional),
        ),
        (
            TextStyle::Button,
            FontId::new(t.text.body, FontFamily::Proportional),
        ),
        (
            TextStyle::Heading,
            FontId::new(t.text.heading, FontFamily::Proportional),
        ),
        (
            TextStyle::Monospace,
            FontId::new(t.text.mono, FontFamily::Monospace),
        ),
    ]
    .into();
    style.text_styles = text_styles;

    let sp = &mut style.spacing;
    sp.item_spacing = Vec2::new(t.space.m, t.space.s);
    sp.button_padding = Vec2::new(t.space.m, t.space.s - 1.0);
    sp.interact_size = Vec2::new(28.0, 24.0);
    sp.window_margin = Margin::same(t.space.l as i8);
    sp.menu_margin = Margin::same(t.space.s as i8);
    sp.indent = t.space.xl;

    let v = &mut style.visuals;
    let r_m = CornerRadius::same(t.radius.m);
    v.panel_fill = t.surface_panel;
    v.window_fill = t.surface_raised;
    v.extreme_bg_color = t.surface_raised;
    v.faint_bg_color = t.surface_panel;
    v.code_bg_color = t.surface_panel;
    v.window_stroke = Stroke::new(1.0_f32, t.border_strong);
    v.window_corner_radius = CornerRadius::same(t.radius.l);
    v.menu_corner_radius = r_m;
    // Texte de lien : 4,5:1 au moins sur les fonds de fenêtre et de panneau (US-16.8).
    v.hyperlink_color = if t.dark { t.action } else { t.action_strong };
    v.error_fg_color = t.danger;
    v.warn_fg_color = t.warning;
    // Texte sélectionné : fond de sélection ; le trait du curseur fait 2 px. L'anneau de focus
    // (`focus_ring`) est dessiné par `paint_focus_ring` : egui n'en dessine pas lui-même.
    v.selection.bg_fill = t.selection_bg;
    v.selection.stroke = Stroke::new(2.0_f32, t.focus_ring);
    v.text_cursor.stroke = Stroke::new(2.0_f32, t.focus_ring);

    let w = &mut v.widgets;
    w.noninteractive.bg_fill = t.surface_panel;
    w.noninteractive.weak_bg_fill = t.surface_panel;
    w.noninteractive.bg_stroke = Stroke::new(1.0_f32, t.border);
    w.noninteractive.fg_stroke = Stroke::new(1.0_f32, t.text_primary);
    w.inactive.bg_fill = t.surface_raised;
    w.inactive.weak_bg_fill = t.surface_raised;
    w.inactive.bg_stroke = Stroke::new(1.0_f32, t.border_strong);
    w.inactive.fg_stroke = Stroke::new(1.0_f32, t.text_primary);
    w.hovered.bg_fill = t.selection_bg;
    w.hovered.weak_bg_fill = t.selection_bg;
    w.hovered.bg_stroke = Stroke::new(1.0_f32, t.action);
    w.hovered.fg_stroke = Stroke::new(1.0_f32, t.text_primary);
    w.active.bg_fill = t.selection_bg;
    w.active.weak_bg_fill = t.selection_bg;
    w.active.bg_stroke = Stroke::new(1.0_f32, t.action_pressed);
    w.active.fg_stroke = Stroke::new(1.0_f32, t.text_primary);
    w.open = w.active;
    for s in [
        &mut w.noninteractive,
        &mut w.inactive,
        &mut w.hovered,
        &mut w.active,
        &mut w.open,
    ] {
        s.corner_radius = r_m;
        s.expansion = 0.0;
    }
    style
}
