//! Composants réutilisables. Chacun lit les jetons du contexte et gère les
//! états normal / survol / pressé / sélectionné / focus / désactivé.

mod button;
mod dialog;
mod numeric_field;
mod text;
mod toast;
mod tool_button;

pub use button::{button, ButtonKind};
pub use dialog::{confirm_dialog, DialogAction};
pub use numeric_field::{NumericField, NumericResponse, Unit};
pub use text::{key_hint, property_row, section_header};
pub use toast::{Toast, ToastLevel, Toasts};
pub use tool_button::{tool_button, tool_button_tip};

use egui::Response;

/// État d'interaction d'un widget peint à la main.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Interaction {
    Disabled,
    Pressed,
    Hover,
    Normal,
}

impl Interaction {
    pub(crate) fn of(response: &Response, enabled: bool) -> Self {
        if !enabled {
            Self::Disabled
        } else if response.is_pointer_button_down_on() {
            Self::Pressed
        } else if response.hovered() {
            Self::Hover
        } else {
            Self::Normal
        }
    }
}
