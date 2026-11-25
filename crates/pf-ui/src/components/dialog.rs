use super::{button, ButtonKind};
use crate::theme;
use egui::{Align, Context, Id, Layout, RichText};

/// Un bouton de dialogue et le choix qu'il renvoie.
#[derive(Clone, Debug)]
pub struct DialogAction<T> {
    pub label: String,
    pub value: T,
    pub kind: ButtonKind,
    /// Choix renvoyé sur Échap ou clic hors du dialogue ; reçoit aussi le focus initial.
    pub cancel: bool,
}

impl<T> DialogAction<T> {
    pub fn primary(label: impl Into<String>, value: T) -> Self {
        Self::new(label, value, ButtonKind::Primary, false)
    }

    pub fn danger(label: impl Into<String>, value: T) -> Self {
        Self::new(label, value, ButtonKind::Danger, false)
    }

    /// Annulation : choix par défaut sur Échap, focus initial (action sûre).
    pub fn cancel(label: impl Into<String>, value: T) -> Self {
        Self::new(label, value, ButtonKind::Secondary, true)
    }

    fn new(label: impl Into<String>, value: T, kind: ButtonKind, cancel: bool) -> Self {
        Self {
            label: label.into(),
            value,
            kind,
            cancel,
        }
    }
}

/// Dialogue modal de confirmation. À appeler à chaque image tant qu'il est ouvert ;
/// renvoie `Some(choix)` quand l'utilisateur a choisi (l'appelant le ferme alors).
/// Échap ou un clic sur le fond renvoie l'action marquée `cancel`, s'il y en a une.
pub fn confirm_dialog<T: Clone>(
    ctx: &Context,
    title: &str,
    body: &str,
    actions: &[DialogAction<T>],
) -> Option<T> {
    let t = theme::tokens(ctx);
    let focus_flag = Id::new(("pf_ui::dialog_focus", title));
    let first_frame = ctx.data(|d| d.get_temp::<bool>(focus_flag)).is_none();
    let mut choice: Option<T> = None;
    let modal = egui::Modal::new(Id::new(("pf_ui::dialog", title))).show(ctx, |ui| {
        ui.set_max_width(380.0);
        ui.label(
            RichText::new(title)
                .size(t.text.heading)
                .strong()
                .color(t.text_primary),
        );
        ui.add_space(t.space.m);
        ui.label(RichText::new(body).color(t.text_secondary));
        ui.add_space(t.space.l);
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), 28.0),
            Layout::right_to_left(Align::Center),
            |ui| {
                for action in actions.iter().rev() {
                    let response = button(ui, &action.label, action.kind);
                    if first_frame && action.cancel {
                        response.request_focus();
                    }
                    if response.clicked() {
                        choice = Some(action.value.clone());
                    }
                }
            },
        );
    });
    if first_frame {
        ctx.data_mut(|d| d.insert_temp(focus_flag, true));
    }
    if choice.is_none() && modal.should_close() {
        choice = actions.iter().find(|a| a.cancel).map(|a| a.value.clone());
    }
    if choice.is_some() {
        ctx.data_mut(|d| d.remove::<bool>(focus_flag));
    }
    choice
}
