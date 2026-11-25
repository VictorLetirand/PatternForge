//! UI Lab : catalogue jouable des jetons et composants, une page par famille.

use crate::components::{
    button, confirm_dialog, key_hint, property_row, section_header, tool_button, ButtonKind,
    DialogAction, NumericField, ToastLevel, Toasts, Unit,
};
use crate::strings::lab as s;
use crate::theme;
use crate::tokens::{contrast_ratio, Tokens};
use egui::{
    Color32, Context, Frame, Margin, Painter, Pos2, RichText, Sense, Shape, Stroke, StrokeKind, Ui,
};
use pf_core::input::format_number;

/// Pages du catalogue.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LabPage {
    Tokens,
    Buttons,
    Numeric,
    Messages,
    Inspector,
}

impl LabPage {
    pub const ALL: [LabPage; 5] = [
        Self::Tokens,
        Self::Buttons,
        Self::Numeric,
        Self::Messages,
        Self::Inspector,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Self::Tokens => s::PAGE_TOKENS,
            Self::Buttons => s::PAGE_BUTTONS,
            Self::Numeric => s::PAGE_NUMERIC,
            Self::Messages => s::PAGE_MESSAGES,
            Self::Inspector => s::PAGE_INSPECTOR,
        }
    }
}

/// Dernier choix du dialogue de démonstration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LabChoice {
    Cancel,
    Confirm,
}

/// État du catalogue.
pub struct LabState {
    pub page: LabPage,
    pub dark: bool,
    pub tool: usize,
    pub toasts: Toasts,
    pub dialog_open: bool,
    pub last_choice: Option<LabChoice>,
    pub length_mm: f64,
    pub angle_deg: f64,
    pub fine_mm: f64,
    pub disabled_mm: f64,
    pub offset_mm: f64,
}

impl Default for LabState {
    fn default() -> Self {
        Self {
            page: LabPage::Tokens,
            dark: false,
            tool: 1,
            toasts: Toasts::default(),
            dialog_open: false,
            last_choice: None,
            length_mm: 120.0,
            angle_deg: 90.0,
            fine_mm: 12.5,
            disabled_mm: 40.0,
            offset_mm: 15.0,
        }
    }
}

const TOOLS: [(&str, &str); 5] = [
    (s::TOOL_SELECT, "Échap"),
    (s::TOOL_SEGMENT, "S"),
    (s::TOOL_POINT, "P"),
    (s::TOOL_CURVE, "C"),
    (s::TOOL_MEASURE, "M"),
];

/// Affiche le catalogue complet (barre, navigation, page courante, messages, dialogue).
pub fn ui_lab(ctx: &Context, state: &mut LabState) {
    let tokens = if state.dark {
        Tokens::precision_dark()
    } else {
        Tokens::precision_light()
    };
    theme::apply(ctx, &tokens);

    egui::TopBottomPanel::top("pf_ui_lab_top").show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(s::TITLE)
                    .size(tokens.text.heading)
                    .strong()
                    .color(tokens.text_primary),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.checkbox(&mut state.dark, s::THEME_DARK);
            });
        });
    });
    egui::SidePanel::left("pf_ui_lab_nav")
        .exact_width(200.0)
        .resizable(false)
        .show(ctx, |ui| {
            ui.add_space(tokens.space.m);
            for page in LabPage::ALL {
                if tool_button(ui, page.title(), None, state.page == page).clicked() {
                    state.page = page;
                }
            }
        });
    egui::CentralPanel::default()
        .frame(
            Frame::NONE
                .fill(tokens.surface_canvas)
                .inner_margin(Margin::same(tokens.space.xl as i8)),
        )
        .show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| match state.page {
                    LabPage::Tokens => page_tokens(ui, &tokens),
                    LabPage::Buttons => page_buttons(ui, state),
                    LabPage::Numeric => page_numeric(ui, state),
                    LabPage::Messages => page_messages(ui, state),
                    LabPage::Inspector => page_inspector(ui, state),
                });
        });

    let now = ctx.input(|i| i.time);
    state.toasts.show(ctx, now);
    if state.dialog_open {
        let actions = [
            DialogAction::cancel(s::DIALOG_CANCEL, LabChoice::Cancel),
            DialogAction::danger(s::DIALOG_CONFIRM, LabChoice::Confirm),
        ];
        if let Some(choice) = confirm_dialog(ctx, s::DIALOG_TITLE, s::DIALOG_BODY, &actions) {
            state.last_choice = Some(choice);
            state.dialog_open = false;
        }
    }
}

fn hex(c: Color32) -> String {
    format!("#{:02X}{:02X}{:02X}", c.r(), c.g(), c.b())
}

fn page_tokens(ui: &mut Ui, t: &Tokens) {
    section_header(ui, s::SECTION_SURFACES);
    egui::Grid::new("lab_colors")
        .num_columns(3)
        .spacing([t.space.l, t.space.xs])
        .show(ui, |ui| {
            for (name, color) in t.colors() {
                let (rect, _) = ui.allocate_exact_size(egui::vec2(36.0, 16.0), Sense::hover());
                ui.painter().rect(
                    rect,
                    t.radius.s,
                    color,
                    Stroke::new(1.0_f32, t.border_strong),
                    StrokeKind::Inside,
                );
                ui.label(RichText::new(name).monospace().size(t.text.mono));
                ui.label(
                    RichText::new(hex(color))
                        .monospace()
                        .size(t.text.mono)
                        .color(t.text_secondary),
                );
                ui.end_row();
            }
        });
    ui.add_space(t.space.m);
    for (label, fg, bg) in [
        (
            "text_primary / surface_panel",
            t.text_primary,
            t.surface_panel,
        ),
        (
            "text_secondary / surface_panel",
            t.text_secondary,
            t.surface_panel,
        ),
        (
            "action_text / action_strong",
            t.action_text,
            t.action_strong,
        ),
    ] {
        property_row(
            ui,
            label,
            &format!("{}:1", format_number(contrast_ratio(fg, bg), 1)),
        );
    }

    section_header(ui, s::SECTION_ROLES);
    let roles = [
        (s::ROLE_CONSTRUCTION, t.role_construction),
        (s::ROLE_CUTTING, t.role_cutting),
        (s::ROLE_SEWING, t.role_sewing),
        (s::ROLE_GRAINLINE, t.role_grainline),
        (s::ROLE_FOLD, t.role_fold),
        (s::ROLE_AXIS, t.role_axis),
    ];
    for (i, (label, color)) in roles.into_iter().enumerate() {
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(140.0, 16.0), Sense::hover());
            ui.painter().rect_filled(rect, 0.0, t.surface_canvas);
            role_line(ui.painter(), rect, i, color);
            ui.label(label);
        });
    }

    section_header(ui, s::SECTION_SPACING);
    for (name, v) in [
        ("xs", t.space.xs),
        ("s", t.space.s),
        ("m", t.space.m),
        ("l", t.space.l),
        ("xl", t.space.xl),
    ] {
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(v * 4.0, 10.0), Sense::hover());
            ui.painter().rect_filled(rect, 0.0, t.action);
            ui.label(RichText::new(format!("{name} = {v}")).monospace());
        });
    }
}

/// Trait d'un rôle : le style (pas seulement la couleur) distingue chaque rôle.
fn role_line(painter: &Painter, rect: egui::Rect, index: usize, color: Color32) {
    let y = rect.center().y;
    let (a, b) = (
        Pos2::new(rect.min.x + 2.0, y),
        Pos2::new(rect.max.x - 2.0, y),
    );
    match index {
        0 => painter.extend(Shape::dashed_line(
            &[a, b],
            Stroke::new(1.5_f32, color),
            12.0,
            5.0,
        )),
        1 => {
            painter.line_segment([a, b], Stroke::new(2.5_f32, color));
        }
        2 => painter.extend(Shape::dashed_line(
            &[a, b],
            Stroke::new(2.0_f32, color),
            1.5,
            3.5,
        )),
        3 => {
            let stroke = Stroke::new(1.5_f32, color);
            painter.line_segment([a, b], stroke);
            painter.line_segment([b, Pos2::new(b.x - 6.0, y - 4.0)], stroke);
            painter.line_segment([b, Pos2::new(b.x - 6.0, y + 4.0)], stroke);
        }
        4 => {
            let mut x = a.x;
            while x + 14.0 < b.x {
                painter.line_segment(
                    [Pos2::new(x, y), Pos2::new(x + 10.0, y)],
                    Stroke::new(1.5_f32, color),
                );
                painter.circle_filled(Pos2::new(x + 14.0, y), 1.2, color);
                x += 20.0;
            }
        }
        _ => painter.extend(Shape::dashed_line(
            &[a, b],
            Stroke::new(1.0_f32, color),
            4.0,
            3.0,
        )),
    }
}

fn page_buttons(ui: &mut Ui, state: &mut LabState) {
    section_header(ui, s::SECTION_TOOLS);
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.label(RichText::new(s::STATE_NORMAL).small());
            for (i, (label, key)) in TOOLS.iter().enumerate() {
                if tool_button(ui, label, Some(key), state.tool == i).clicked() {
                    state.tool = i;
                }
            }
        });
        ui.vertical(|ui| {
            ui.label(RichText::new(s::STATE_SELECTED).small());
            tool_button(ui, s::TOOL_SEGMENT, Some("S"), true);
            ui.label(RichText::new(s::STATE_DISABLED).small());
            ui.add_enabled_ui(false, |ui| {
                tool_button(ui, s::TOOL_CURVE, Some("C"), false);
                tool_button(ui, s::TOOL_SEGMENT, Some("S"), true);
            });
        });
    });
    ui.label(RichText::new(s::STATE_HINT).small());

    section_header(ui, s::SECTION_BUTTONS);
    for enabled in [true, false] {
        ui.add_enabled_ui(enabled, |ui| {
            ui.horizontal(|ui| {
                button(ui, s::BTN_PRIMARY, ButtonKind::Primary);
                button(ui, s::BTN_SECONDARY, ButtonKind::Secondary);
                button(ui, s::BTN_DANGER, ButtonKind::Danger);
            });
        });
    }

    section_header(ui, s::SECTION_HINTS);
    ui.horizontal(|ui| {
        for keys in ["Ctrl+S", "Ctrl+Z", "Maj+clic", "F1", "Échap"] {
            key_hint(ui, keys);
        }
    });
}

fn page_numeric(ui: &mut Ui, state: &mut LabState) {
    section_header(ui, s::SECTION_NUMERIC);
    ui.label(s::NUMERIC_HELP);
    ui.add_space(8.0);
    egui::Grid::new("lab_numeric")
        .num_columns(3)
        .spacing([12.0, 8.0])
        .show(ui, |ui| {
            ui.label(s::FIELD_LENGTH);
            NumericField::new(&mut state.length_mm, Unit::Mm)
                .id_salt("len")
                .step(1.0)
                .range(0.0..=1000.0)
                .show(ui);
            ui.label(format!(
                "{}{}",
                s::VALUE_PREFIX,
                format_number(state.length_mm, 3)
            ));
            ui.end_row();

            ui.label(s::FIELD_ANGLE);
            NumericField::new(&mut state.angle_deg, Unit::Deg)
                .id_salt("ang")
                .step(5.0)
                .range(0.0..=360.0)
                .show(ui);
            ui.label(format!(
                "{}{}",
                s::VALUE_PREFIX,
                format_number(state.angle_deg, 3)
            ));
            ui.end_row();

            ui.label(s::FIELD_STEP);
            NumericField::new(&mut state.fine_mm, Unit::Mm)
                .id_salt("fine")
                .step(0.1)
                .range(0.0..=50.0)
                .show(ui);
            ui.label(format!(
                "{}{}",
                s::VALUE_PREFIX,
                format_number(state.fine_mm, 3)
            ));
            ui.end_row();

            ui.label(s::FIELD_DISABLED);
            ui.add_enabled_ui(false, |ui| {
                NumericField::new(&mut state.disabled_mm, Unit::Mm)
                    .id_salt("dis")
                    .show(ui);
            });
            ui.end_row();
        });
}

fn page_messages(ui: &mut Ui, state: &mut LabState) {
    let now = ui.ctx().input(|i| i.time);
    section_header(ui, s::SECTION_TOASTS);
    ui.horizontal_wrapped(|ui| {
        for (label, level, text) in [
            (s::SHOW_INFO, ToastLevel::Info, s::TOAST_INFO),
            (s::SHOW_SUCCESS, ToastLevel::Success, s::TOAST_SUCCESS),
            (s::SHOW_WARNING, ToastLevel::Warning, s::TOAST_WARNING),
            (s::SHOW_ERROR, ToastLevel::Error, s::TOAST_ERROR),
        ] {
            if button(ui, label, ButtonKind::Secondary).clicked() {
                state.toasts.push(level, text, now);
            }
        }
    });
    section_header(ui, s::SECTION_DIALOG);
    if button(ui, s::OPEN_DIALOG, ButtonKind::Secondary).clicked() {
        state.dialog_open = true;
    }
    let result = match state.last_choice {
        None => s::CHOICE_NONE,
        Some(LabChoice::Cancel) => s::CHOICE_CANCELLED,
        Some(LabChoice::Confirm) => s::CHOICE_CONFIRMED,
    };
    ui.label(format!("{}{}", s::DIALOG_RESULT_PREFIX, result));
}

fn page_inspector(ui: &mut Ui, state: &mut LabState) {
    let t = theme::tokens(ui.ctx());
    Frame::NONE
        .fill(t.surface_panel)
        .stroke(Stroke::new(1.0_f32, t.border))
        .inner_margin(Margin::same(t.space.l as i8))
        .show(ui, |ui| {
            ui.set_width(300.0);
            section_header(ui, s::SECTION_SEGMENT);
            property_row(ui, s::PROP_ID, "seg-0004");
            property_row(ui, s::PROP_SHEET, "Devant");
            property_row(ui, s::PROP_ROLE, s::PROP_ROLE_VALUE);
            property_row(
                ui,
                s::PROP_LENGTH_MEASURED,
                &format!("{} mm", format_number(state.length_mm, 2)),
            );
            section_header(ui, s::SECTION_PARAMS);
            egui::Grid::new("lab_inspector")
                .num_columns(2)
                .spacing([t.space.l, t.space.m])
                .show(ui, |ui| {
                    ui.label(s::PARAM_LENGTH);
                    NumericField::new(&mut state.length_mm, Unit::Mm)
                        .id_salt("insp_len")
                        .range(0.0..=1000.0)
                        .show(ui);
                    ui.end_row();
                    ui.label(s::PARAM_ANGLE);
                    NumericField::new(&mut state.angle_deg, Unit::Deg)
                        .id_salt("insp_ang")
                        .step(5.0)
                        .range(0.0..=360.0)
                        .show(ui);
                    ui.end_row();
                    ui.label(s::PARAM_OFFSET);
                    NumericField::new(&mut state.offset_mm, Unit::Mm)
                        .id_salt("insp_off")
                        .step(0.5)
                        .show(ui);
                    ui.end_row();
                });
        });
}
