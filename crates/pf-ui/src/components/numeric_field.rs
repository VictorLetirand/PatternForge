use super::Interaction;
use crate::strings::NumericStrings;
use crate::theme;
use egui::{
    Align, CornerRadius, CursorIcon, FontId, Id, Key, Modifiers, Response, RichText, Sense, Stroke,
    TextEdit, Ui, WidgetInfo, WidgetType,
};
use pf_core::input::{format_number, parse_angle_deg};
use pf_core::units::{display_units, field_text, parse_length_display, LengthUnit};
use std::ops::RangeInclusive;

/// Unité d'un champ numérique. La valeur interne est en mm ou en degrés ; `Mm` s'affiche et se
/// saisit dans l'unité d'affichage courante (mm, cm, pouces : `pf_core::units`, US-16.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit {
    Mm,
    Deg,
}

impl Unit {
    /// Symbole affiché (aussi poignée de réglage par glisser).
    pub fn symbol(self) -> &'static str {
        match self {
            Self::Mm => display_units().unit.suffix(),
            Self::Deg => "°",
        }
    }

    /// Analyse une saisie (`120`, `120,5`, `12 cm`, `1,2 m` ; `90`, `90°`) ; sans unité, une
    /// longueur est dans l'unité d'affichage courante.
    pub fn parse(self, text: &str) -> Result<f64, String> {
        match self {
            Self::Mm => parse_length_display(text),
            Self::Deg => parse_angle_deg(text),
        }
    }
}

/// Résultat d'un affichage de champ numérique.
pub struct NumericResponse {
    /// La valeur a changé pendant cette image (validation, flèches, glisser).
    pub changed: bool,
    /// Une valeur valide vient d'être confirmée (Entrée, perte de focus,
    /// flèche, fin de glisser).
    pub committed: bool,
    /// Erreur de saisie en cours (la valeur n'a alors pas été modifiée).
    pub error: Option<String>,
    /// Réponse du champ de saisie.
    pub response: Response,
}

#[derive(Clone, Default)]
struct FieldState {
    text: String,
    error: Option<String>,
    dirty: bool,
    /// Le champ avait le focus à la fin de l'image précédente.
    was_focused: bool,
    /// Valeur de départ et déplacement cumulé (px) d'un glisser en cours.
    scrub: Option<(f64, f64)>,
}

/// Champ numérique avec unité, erreur en ligne, incrément et glisser.
pub struct NumericField<'a> {
    value: &'a mut f64,
    unit: Unit,
    step: f64,
    range: Option<RangeInclusive<f64>>,
    decimals: usize,
    width: f32,
    id_salt: Option<Id>,
    strings: NumericStrings,
}

impl<'a> NumericField<'a> {
    pub fn new(value: &'a mut f64, unit: Unit) -> Self {
        Self {
            value,
            unit,
            step: 1.0,
            range: None,
            decimals: 2,
            width: 84.0,
            id_salt: None,
            strings: NumericStrings::default(),
        }
    }

    /// Pas des flèches haut/bas ; le glisser avance de `pas / 4` par pixel.
    pub fn step(mut self, step: f64) -> Self {
        self.step = step;
        self
    }

    /// Bornes incluses : une saisie hors bornes est refusée, flèches et glisser sont bornés.
    pub fn range(mut self, range: RangeInclusive<f64>) -> Self {
        self.range = Some(range);
        self
    }

    /// Nombre maximal de décimales affichées (et d'arrondi des incréments).
    pub fn decimals(mut self, decimals: usize) -> Self {
        self.decimals = decimals;
        self
    }

    pub fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    /// Identité stable (obligatoire si plusieurs champs peuvent changer d'ordre).
    pub fn id_salt(mut self, salt: impl std::hash::Hash) -> Self {
        self.id_salt = Some(Id::new(salt));
        self
    }

    pub fn strings(mut self, strings: NumericStrings) -> Self {
        self.strings = strings;
        self
    }

    fn round(&self, v: f64) -> f64 {
        let p = 10f64.powi(i32::try_from(self.decimals).unwrap_or(6));
        (v * p).round() / p
    }

    fn clamp(&self, v: f64) -> f64 {
        match &self.range {
            Some(r) => v.clamp(*r.start(), *r.end()),
            None => v,
        }
    }

    fn validate(&self, text: &str) -> Result<f64, String> {
        let v = self.unit.parse(text)?;
        if let Some(r) = &self.range {
            if !r.contains(&v) {
                return Err(self
                    .strings
                    .out_of_range
                    .replace("{min}", &format_number(*r.start(), self.decimals))
                    .replace("{max}", &format_number(*r.end(), self.decimals)));
            }
        }
        Ok(v)
    }

    pub fn show(self, ui: &mut Ui) -> NumericResponse {
        let t = theme::tokens(ui.ctx());
        let enabled = ui.is_enabled();
        let base = match self.id_salt {
            Some(salt) => ui.make_persistent_id(salt),
            None => {
                let id = ui.next_auto_id();
                ui.skip_ahead_auto_ids(1);
                id
            }
        };
        let edit_id = base.with("edit");
        let mut st = ui
            .data_mut(|d| d.get_temp::<FieldState>(base))
            .unwrap_or_default();
        let fmt = |v: f64| {
            if self.unit == Unit::Mm && display_units().unit != LengthUnit::Mm {
                field_text(v)
            } else {
                format_number(v, self.decimals)
            }
        };
        let had_focus = ui.memory(|m| m.has_focus(edit_id));
        // Focus retiré avant l'affichage (clic ailleurs, Tab) : valider quand même.
        let focus_taken = st.was_focused && !had_focus;
        let mut changed = false;
        let mut committed = false;
        let mut cancelled = false;
        let mut value = *self.value;

        if !had_focus && !focus_taken && st.error.is_none() && st.scrub.is_none() {
            st.text = fmt(value);
            st.dirty = false;
        }

        // egui retire lui-même le focus sur Échap : `focus_taken` couvre aussi ce cas.
        if (had_focus || focus_taken)
            && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape))
        {
            cancelled = true;
            st.text = fmt(value);
            st.error = None;
            st.dirty = false;
            ui.memory_mut(|m| m.surrender_focus(edit_id));
        }
        if had_focus {
            let up = ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::ArrowUp));
            let down = ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::ArrowDown));
            if up != down {
                let from = self.validate(&st.text).unwrap_or(value);
                let delta = if up { self.step } else { -self.step };
                value = self.clamp(self.round(from + delta));
                st.text = fmt(value);
                st.error = None;
                st.dirty = false;
                changed = value != *self.value;
                committed = true;
            }
        }

        let has_error = st.error.is_some();
        let (edit, unit_resp) = ui
            .vertical(|ui| {
                let row = ui.horizontal(|ui| {
                    let edit = ui
                        .scope(|ui| {
                            if has_error {
                                let w = &mut ui.visuals_mut().widgets;
                                for s in [
                                    &mut w.noninteractive,
                                    &mut w.inactive,
                                    &mut w.hovered,
                                    &mut w.active,
                                ] {
                                    s.bg_stroke = Stroke::new(1.5_f32, t.danger);
                                }
                            }
                            ui.add(
                                TextEdit::singleline(&mut st.text)
                                    .id(edit_id)
                                    .font(FontId::monospace(t.text.mono))
                                    .horizontal_align(Align::Max)
                                    .desired_width(self.width),
                            )
                        })
                        .inner;
                    let unit_resp = unit_handle(ui, &self, enabled);
                    (edit, unit_resp)
                });
                if let Some(err) = &st.error {
                    ui.label(
                        RichText::new(format!("{}{}", self.strings.error_prefix, err))
                            .size(t.text.caption)
                            .color(t.danger),
                    );
                }
                row.inner
            })
            .inner;

        // Glisser horizontal sur l'unité.
        if unit_resp.drag_started() {
            st.scrub = Some((value, 0.0));
            ui.memory_mut(|m| m.surrender_focus(edit_id));
        }
        if unit_resp.dragged() {
            let fast = ui.input(|i| i.modifiers.shift);
            if let Some((origin, acc)) = st.scrub.as_mut() {
                let mult = if fast { 10.0 } else { 1.0 };
                *acc += f64::from(unit_resp.drag_delta().x) * mult;
                let target = self.clamp(self.round(*origin + *acc * self.step * 0.25));
                if target != value {
                    value = target;
                    changed = true;
                }
                st.text = fmt(value);
                st.error = None;
                st.dirty = false;
            }
        }
        if unit_resp.drag_stopped() && st.scrub.take().is_some() {
            committed = true;
        }

        // Saisie : validation sur Entrée ou perte de focus.
        if edit.changed() {
            st.dirty = true;
            st.error = None;
        }
        if (edit.lost_focus() || focus_taken) && !cancelled && st.dirty {
            let enter = ui.input(|i| i.key_pressed(Key::Enter));
            match self.validate(&st.text) {
                Ok(v) => {
                    changed |= v != value;
                    value = v;
                    committed = true;
                    st.text = fmt(v);
                    st.dirty = false;
                    st.error = None;
                }
                Err(e) => {
                    st.error = Some(e);
                    if enter {
                        edit.request_focus();
                    }
                }
            }
        }

        *self.value = value;
        let error = st.error.clone();
        st.was_focused = ui.memory(|m| m.has_focus(edit_id));
        ui.data_mut(|d| d.insert_temp(base, st));
        NumericResponse {
            changed,
            committed,
            error,
            response: edit,
        }
    }
}

/// Libellé d'unité, poignée de réglage par glisser.
fn unit_handle(ui: &mut Ui, field: &NumericField<'_>, enabled: bool) -> Response {
    let t = theme::tokens(ui.ctx());
    let symbol = field.unit.symbol();
    let galley = ui.painter().layout_no_wrap(
        symbol.to_owned(),
        FontId::monospace(t.text.mono),
        egui::Color32::PLACEHOLDER,
    );
    let size = egui::vec2(
        galley.size().x + 2.0 * t.space.m,
        ui.spacing().interact_size.y,
    );
    let (rect, response) = ui.allocate_exact_size(size, Sense::drag());
    response.widget_info(|| WidgetInfo::labeled(WidgetType::DragValue, enabled, symbol));
    if ui.is_rect_visible(rect) {
        let state = Interaction::of(&response, enabled);
        let color = if enabled {
            t.text_secondary
        } else {
            t.text_disabled
        };
        if matches!(state, Interaction::Hover | Interaction::Pressed) {
            ui.painter()
                .rect_filled(rect, CornerRadius::same(t.radius.m), t.selection_bg);
        }
        ui.painter()
            .galley(rect.center() - galley.size() / 2.0, galley, color);
    }
    if enabled {
        response
            .on_hover_cursor(CursorIcon::ResizeHorizontal)
            .on_hover_text(field.strings.scrub_hint.clone())
    } else {
        response
    }
}
