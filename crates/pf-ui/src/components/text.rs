use crate::theme;
use egui::{Align, Layout, Margin, Response, RichText, Stroke, Ui};

/// Titre de section (texte renforcé + filet).
pub fn section_header(ui: &mut Ui, text: &str) -> Response {
    let t = theme::tokens(ui.ctx());
    ui.add_space(t.space.m);
    let response = ui.label(
        RichText::new(text)
            .strong()
            .size(t.text.body)
            .color(t.text_secondary),
    );
    ui.separator();
    response
}

/// Ligne « libellé : valeur » en lecture seule, valeur en police monospace. Quand libellé et
/// valeur ne tiennent pas côte à côte, la valeur passe à la ligne sous le libellé (jamais
/// tronquée en silence) et porte sa valeur complète en info-bulle.
pub fn property_row(ui: &mut Ui, label: &str, value: &str) -> Response {
    let t = theme::tokens(ui.ctx());
    let label_text = RichText::new(label).color(t.text_secondary);
    let value_text = RichText::new(value)
        .monospace()
        .size(t.text.mono)
        .color(t.text_primary);
    let width = |text: &RichText| {
        egui::WidgetText::from(text.clone())
            .into_galley(
                ui,
                Some(egui::TextWrapMode::Extend),
                f32::INFINITY,
                egui::TextStyle::Body,
            )
            .size()
            .x
    };
    let needed = width(&label_text) + width(&value_text) + ui.spacing().item_spacing.x;
    if needed <= ui.available_width() {
        return ui
            .horizontal(|ui| {
                ui.label(label_text);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(value_text);
                });
            })
            .response;
    }
    ui.vertical(|ui| {
        ui.label(label_text);
        ui.add(egui::Label::new(value_text).wrap())
            .on_hover_text(value);
    })
    .response
}

/// Pastille de raccourci clavier (« Ctrl+S »).
pub fn key_hint(ui: &mut Ui, keys: &str) -> Response {
    let t = theme::tokens(ui.ctx());
    egui::Frame::NONE
        .fill(t.surface_raised)
        .stroke(Stroke::new(1.0_f32, t.border_strong))
        .corner_radius(t.radius.s)
        .inner_margin(Margin::symmetric(5, 1))
        .show(ui, |ui| {
            ui.label(
                RichText::new(keys)
                    .monospace()
                    .size(t.text.caption)
                    .color(t.text_secondary),
            );
        })
        .response
}
