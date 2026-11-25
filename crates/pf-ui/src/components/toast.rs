use super::{button, ButtonKind};
use crate::strings::ToastLabels;
use crate::theme;
use egui::{Align, Align2, Color32, Context, Frame, Layout, Margin, RichText, Stroke, Vec2};
use std::time::Duration;

/// Niveau d'un message temporaire.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToastLevel {
    Info,
    Success,
    Warning,
    Error,
}

impl ToastLevel {
    /// Durée d'affichage par défaut (secondes) : les erreurs restent plus longtemps.
    pub fn default_ttl(self) -> f64 {
        match self {
            Self::Info | Self::Success => 4.0,
            Self::Warning => 7.0,
            Self::Error => 10.0,
        }
    }
}

/// Un message temporaire.
#[derive(Clone, Debug, PartialEq)]
pub struct Toast {
    pub level: ToastLevel,
    pub message: String,
    /// Instant de création (secondes, même horloge que `now`).
    pub created: f64,
    /// Durée de vie (secondes).
    pub ttl: f64,
}

/// File de messages temporaires, affichée en bas à droite.
/// Le temps est passé en paramètre (`now`, en secondes) : testable sans horloge.
#[derive(Clone, Debug, Default)]
pub struct Toasts {
    items: Vec<Toast>,
    pub labels: ToastLabels,
}

impl Toasts {
    pub fn new(labels: ToastLabels) -> Self {
        Self {
            items: Vec::new(),
            labels,
        }
    }

    pub fn push(&mut self, level: ToastLevel, message: impl Into<String>, now: f64) {
        self.push_with_ttl(level, message, now, level.default_ttl());
    }

    pub fn push_with_ttl(
        &mut self,
        level: ToastLevel,
        message: impl Into<String>,
        now: f64,
        ttl: f64,
    ) {
        self.items.push(Toast {
            level,
            message: message.into(),
            created: now,
            ttl,
        });
    }

    /// Retire les messages expirés à l'instant `now`.
    pub fn expire(&mut self, now: f64) {
        self.items.retain(|t| now - t.created < t.ttl);
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn items(&self) -> &[Toast] {
        &self.items
    }

    fn label(&self, level: ToastLevel) -> &str {
        match level {
            ToastLevel::Info => &self.labels.info,
            ToastLevel::Success => &self.labels.success,
            ToastLevel::Warning => &self.labels.warning,
            ToastLevel::Error => &self.labels.error,
        }
    }

    /// Expire puis affiche la pile (le plus récent en bas).
    pub fn show(&mut self, ctx: &Context, now: f64) {
        self.expire(now);
        if self.items.is_empty() {
            return;
        }
        let t = theme::tokens(ctx);
        let mut dismissed = None;
        egui::Area::new(egui::Id::new("pf_ui::toasts"))
            .order(egui::Order::Foreground)
            .anchor(Align2::RIGHT_BOTTOM, Vec2::new(-t.space.xl, -t.space.xl))
            .show(ctx, |ui| {
                ui.set_max_width(340.0);
                for (i, toast) in self.items.iter().enumerate() {
                    let accent: Color32 = match toast.level {
                        ToastLevel::Info => t.action,
                        ToastLevel::Success => t.success,
                        ToastLevel::Warning => t.warning,
                        ToastLevel::Error => t.danger,
                    };
                    Frame::NONE
                        .fill(t.surface_raised)
                        .stroke(Stroke::new(1.0_f32, t.border_strong))
                        .corner_radius(t.radius.l)
                        .inner_margin(Margin::same(t.space.m as i8))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(self.label(toast.level))
                                        .strong()
                                        .color(accent),
                                );
                                ui.label(RichText::new(&toast.message).color(t.text_primary));
                            });
                            ui.allocate_ui_with_layout(
                                egui::vec2(ui.available_width(), 28.0),
                                Layout::right_to_left(Align::Center),
                                |ui| {
                                    if button(ui, &self.labels.dismiss, ButtonKind::Secondary)
                                        .clicked()
                                    {
                                        dismissed = Some(i);
                                    }
                                },
                            );
                        });
                    ui.add_space(t.space.s);
                }
            });
        if let Some(i) = dismissed {
            self.items.remove(i);
        }
        let next = self
            .items
            .iter()
            .map(|t| (t.created + t.ttl - now).max(0.0))
            .fold(f64::INFINITY, f64::min);
        if next.is_finite() {
            ctx.request_repaint_after(Duration::from_secs_f64(next + 0.05));
        }
    }
}
