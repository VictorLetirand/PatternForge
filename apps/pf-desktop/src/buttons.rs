//! Section « Boutons et boutonnières » de l'inspecteur d'une pièce (US-06.4). Elle ne calcule
//! rien : positions, espacement et contrôle du recul du bord viennent de
//! `Project::piece_buttons`, et chaque action est une commande (`AddButtonGroup`,
//! `SetButtonGroup`, `RemoveButtonGroup` : une étape annulable, refus sans trace).
//!
//! Limites : les deux points se prennent dans la sélection (le premier sélectionné porte le
//! premier élément) ; un groupe existant change de points par « Points = sélection » ; les
//! valeurs se valident à la perte du focus. Aucune liste déroulante (les tests de l'inspecteur
//! de pièce comptent les `ComboBox`).

use crate::app::App;
use crate::palette::pal;
use crate::panels::buffered_edit;
use eframe::egui::{self, RichText};
use pf_core::input::{format_mm, format_number};
use pf_core::{
    ButtonElement, ButtonGroup, ButtonShape, ButtonholeOrientation, Command, PieceId, PointId,
};
use pf_i18n::{tr, tr_args};

/// Orientation choisie dans le formulaire.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum OrientKind {
    Horizontal,
    Vertical,
    Perpendicular,
    Angle,
}

/// Formulaire « Ajouter des boutons… » : état d'interface, jamais enregistré ; l'ajout passe par
/// `Command::AddButtonGroup`.
pub struct ButtonForm {
    pub piece: PieceId,
    /// Boutonnières (fentes) plutôt que boutons (cercles).
    pub buttonhole: bool,
    pub count: String,
    /// Diamètre du bouton, en mm.
    pub size: String,
    pub length: String,
    pub width: String,
    pub orient: OrientKind,
    pub angle: String,
    /// Recul du bord, en mm.
    pub margin: String,
}

impl ButtonForm {
    fn new(piece: PieceId) -> ButtonForm {
        ButtonForm {
            piece,
            buttonhole: false,
            count: "5".into(),
            size: "12".into(),
            length: "20".into(),
            width: "3".into(),
            orient: OrientKind::Perpendicular,
            angle: "0".into(),
            margin: "10".into(),
        }
    }
}

/// Libellé traduit de l'orientation d'une boutonnière.
pub fn orientation_label(orientation: ButtonholeOrientation) -> String {
    match orientation {
        ButtonholeOrientation::Horizontal {} => tr("piece.buttons.orient.horizontal"),
        ButtonholeOrientation::Vertical {} => tr("piece.buttons.orient.vertical"),
        ButtonholeOrientation::Perpendicular {} => tr("piece.buttons.orient.perpendicular"),
        ButtonholeOrientation::Angle { degrees } => tr_args(
            "piece.buttons.orient.angle_value",
            &[("angle", &format_number(degrees, 2))],
        ),
    }
}

/// Libellé traduit d'un élément dessiné (étiquette sur la feuille).
pub fn shape_label(shape: ButtonShape) -> String {
    match shape {
        ButtonShape::Button { diameter_mm } => tr_args(
            "piece.buttons.label.button",
            &[("size", &format_mm(diameter_mm))],
        ),
        ButtonShape::Buttonhole {
            length_mm,
            width_mm,
            ..
        } => tr_args(
            "piece.buttons.label.buttonhole",
            &[
                ("length", &format_mm(length_mm)),
                ("width", &format_mm(width_mm)),
            ],
        ),
    }
}

fn kind_label(kind: OrientKind) -> String {
    match kind {
        OrientKind::Horizontal => tr("piece.buttons.orient.horizontal"),
        OrientKind::Vertical => tr("piece.buttons.orient.vertical"),
        OrientKind::Perpendicular => tr("piece.buttons.orient.perpendicular"),
        OrientKind::Angle => tr("piece.buttons.orient.angle"),
    }
}

fn kind_of(o: ButtonholeOrientation) -> OrientKind {
    match o {
        ButtonholeOrientation::Horizontal {} => OrientKind::Horizontal,
        ButtonholeOrientation::Vertical {} => OrientKind::Vertical,
        ButtonholeOrientation::Perpendicular {} => OrientKind::Perpendicular,
        ButtonholeOrientation::Angle { .. } => OrientKind::Angle,
    }
}

fn orientation_of(kind: OrientKind, degrees: f64) -> ButtonholeOrientation {
    match kind {
        OrientKind::Horizontal => ButtonholeOrientation::Horizontal {},
        OrientKind::Vertical => ButtonholeOrientation::Vertical {},
        OrientKind::Perpendicular => ButtonholeOrientation::Perpendicular {},
        OrientKind::Angle => ButtonholeOrientation::Angle { degrees },
    }
}

fn set_command(piece: PieceId, g: &ButtonGroup) -> Command {
    Command::SetButtonGroup {
        piece,
        group: g.id,
        from: g.from,
        to: g.to,
        count: g.count,
        element: g.element,
        edge_margin_mm: g.edge_margin_mm,
    }
}

/// Entier de 1 à 100 saisi (refus de tout autre texte).
fn parse_count(text: &str) -> Option<u32> {
    text.trim().parse::<u32>().ok()
}

/// Ligne d'un groupe : texte, orientation actuelle (degrés) et nombre d'éléments trop proches.
struct Row {
    group: ButtonGroup,
    text: String,
    angle_deg: f64,
    too_close: usize,
}

impl App {
    /// Groupes de boutons d'une pièce : liste (texte, champs modifiables, retrait) et formulaire
    /// d'ajout depuis la sélection de deux points.
    pub(crate) fn piece_buttons_section(&mut self, ui: &mut egui::Ui, piece: &pf_core::Piece) {
        let rows: Vec<Row> = {
            let project = self.editor.project();
            let sheet = project.piece_sheet(piece.id).ok();
            let name = |p: PointId| sheet.map(|s| s.label(p).to_string()).unwrap_or_default();
            project
                .piece_buttons(piece.id)
                .into_iter()
                .map(|r| {
                    let g = r.group;
                    let angle_deg = match r.elements.first().map(|e| e.shape) {
                        Some(ButtonShape::Buttonhole { angle_deg, .. }) => angle_deg,
                        _ => 0.0,
                    };
                    let (from, to) = (name(g.from), name(g.to));
                    let spacing = format_mm(r.spacing_mm);
                    let margin = format_mm(g.edge_margin_mm);
                    let count = g.count.to_string();
                    let text = match g.element {
                        ButtonElement::Button { diameter_mm } => tr_args(
                            "piece.buttons.row.button",
                            &[
                                ("count", &count),
                                ("size", &format_mm(diameter_mm)),
                                ("from", &from),
                                ("to", &to),
                                ("spacing", &spacing),
                                ("margin", &margin),
                            ],
                        ),
                        ButtonElement::Buttonhole {
                            length_mm,
                            width_mm,
                            orientation,
                        } => tr_args(
                            "piece.buttons.row.buttonhole",
                            &[
                                ("count", &count),
                                ("length", &format_mm(length_mm)),
                                ("width", &format_mm(width_mm)),
                                ("orientation", &orientation_label(orientation)),
                                ("from", &from),
                                ("to", &to),
                                ("spacing", &spacing),
                                ("margin", &margin),
                            ],
                        ),
                    };
                    Row {
                        group: g,
                        text,
                        angle_deg,
                        too_close: r.elements.iter().filter(|e| e.too_close).count(),
                    }
                })
                .collect()
        };
        if rows.is_empty() {
            crate::a11y::label(
                ui,
                RichText::new(tr("piece.buttons.none"))
                    .small()
                    .color(pal().muted),
            );
        } else {
            crate::a11y::label(
                ui,
                RichText::new(tr_args(
                    "piece.buttons.summary",
                    &[("count", &rows.len().to_string())],
                ))
                .small()
                .color(pal().muted),
            );
        }
        let selected = self.editor.selection.points.clone();
        let mut commands: Vec<Command> = Vec::new();
        for row in &rows {
            let g = row.group;
            ui.horizontal_wrapped(|ui| {
                crate::a11y::label(ui, &row.text);
                if row.too_close > 0 {
                    crate::a11y::label(
                        ui,
                        RichText::new(tr_args(
                            "piece.buttons.too_close",
                            &[("count", &row.too_close.to_string())],
                        ))
                        .small()
                        .color(ui.visuals().warn_fg_color),
                    );
                }
            });
            let key = (g.id.0 << 8) | 0x80;
            // Paramètres en grille à deux colonnes (étiquette | champ) ; les boutons restent
            // hors de la grille (un bouton dans une cellule dépasse du panneau).
            egui::Grid::new(("button-edit", g.id.0))
                .num_columns(2)
                .show(ui, |ui| {
                    // Nombre d'éléments.
                    crate::a11y::label(ui, RichText::new(tr("piece.buttons.edit.count")).small());
                    let current = g.count.to_string();
                    if let Some(text) = buffered_edit(ui, &mut self.text_buf, key, &current, 56.0) {
                        match parse_count(&text) {
                            Some(count) => {
                                let mut next = g;
                                next.count = count;
                                commands.push(set_command(piece.id, &next));
                            }
                            None => self.editor.error(tr("piece.buttons.invalid_count")),
                        }
                    }
                    ui.end_row();
                    // Dimensions.
                    match g.element {
                        ButtonElement::Button { diameter_mm } => {
                            crate::a11y::label(
                                ui,
                                RichText::new(tr("piece.buttons.edit.size")).small(),
                            );
                            let current = pf_core::units::field_text(diameter_mm);
                            if let Some(text) =
                                buffered_edit(ui, &mut self.text_buf, key | 1, &current, 56.0)
                            {
                                match pf_core::units::parse_length_display(&text) {
                                    Ok(v) => {
                                        let mut next = g;
                                        next.element = ButtonElement::Button { diameter_mm: v };
                                        commands.push(set_command(piece.id, &next));
                                    }
                                    Err(_) => self.editor.error(tr("piece.buttons.invalid_size")),
                                }
                            }
                            ui.end_row();
                        }
                        ButtonElement::Buttonhole {
                            length_mm,
                            width_mm,
                            orientation,
                        } => {
                            for (slot, label, value) in [
                                (1u64, "piece.buttons.edit.length", length_mm),
                                (2u64, "piece.buttons.edit.width", width_mm),
                            ] {
                                crate::a11y::label(ui, RichText::new(tr(label)).small());
                                let current = pf_core::units::field_text(value);
                                if let Some(text) = buffered_edit(
                                    ui,
                                    &mut self.text_buf,
                                    key | slot,
                                    &current,
                                    56.0,
                                ) {
                                    match pf_core::units::parse_length_display(&text) {
                                        Ok(v) => {
                                            let (l, w) = if slot == 1 {
                                                (v, width_mm)
                                            } else {
                                                (length_mm, v)
                                            };
                                            let mut next = g;
                                            next.element = ButtonElement::Buttonhole {
                                                length_mm: l,
                                                width_mm: w,
                                                orientation,
                                            };
                                            commands.push(set_command(piece.id, &next));
                                        }
                                        Err(_) => {
                                            self.editor.error(tr("piece.buttons.invalid_size"))
                                        }
                                    }
                                }
                                ui.end_row();
                            }
                            if let ButtonholeOrientation::Angle { degrees } = orientation {
                                crate::a11y::label(
                                    ui,
                                    RichText::new(tr("piece.buttons.edit.angle")).small(),
                                );
                                let current = format_number(degrees, 3);
                                if let Some(text) =
                                    buffered_edit(ui, &mut self.text_buf, key | 3, &current, 56.0)
                                {
                                    match pf_core::input::parse_angle_deg(&text) {
                                        Ok(v) => {
                                            let mut next = g;
                                            next.element = ButtonElement::Buttonhole {
                                                length_mm,
                                                width_mm,
                                                orientation: ButtonholeOrientation::Angle {
                                                    degrees: v,
                                                },
                                            };
                                            commands.push(set_command(piece.id, &next));
                                        }
                                        Err(_) => {
                                            self.editor.error(tr("piece.buttons.invalid_angle"))
                                        }
                                    }
                                }
                                ui.end_row();
                            }
                        }
                    }
                    // Recul du bord (contrôle affiché, pas une contrainte).
                    crate::a11y::label(ui, RichText::new(tr("piece.buttons.edit.margin")).small());
                    let current = pf_core::units::field_text(g.edge_margin_mm);
                    if let Some(text) =
                        buffered_edit(ui, &mut self.text_buf, key | 4, &current, 56.0)
                    {
                        match pf_core::units::parse_length_display(&text) {
                            Ok(v) => {
                                let mut next = g;
                                next.edge_margin_mm = v;
                                commands.push(set_command(piece.id, &next));
                            }
                            Err(_) => self.editor.error(tr("piece.buttons.invalid_size")),
                        }
                    }
                    ui.end_row();
                });
            ui.horizontal_wrapped(|ui| {
                // Orientation d'une boutonnière : quatre choix, sans liste déroulante.
                if let ButtonElement::Buttonhole {
                    length_mm,
                    width_mm,
                    orientation,
                } = g.element
                {
                    let now = kind_of(orientation);
                    for kind in [
                        OrientKind::Horizontal,
                        OrientKind::Vertical,
                        OrientKind::Perpendicular,
                        OrientKind::Angle,
                    ] {
                        if ui
                            .add(egui::Button::selectable(now == kind, kind_label(kind)))
                            .clicked()
                            && now != kind
                        {
                            let mut next = g;
                            next.element = ButtonElement::Buttonhole {
                                length_mm,
                                width_mm,
                                orientation: orientation_of(kind, row.angle_deg),
                            };
                            commands.push(set_command(piece.id, &next));
                        }
                    }
                }
                if ui
                    .add_enabled(
                        selected.len() == 2,
                        egui::Button::new(tr("piece.buttons.points")),
                    )
                    .on_hover_text(tr("piece.buttons.points.hover"))
                    .clicked()
                {
                    let mut next = g;
                    next.from = selected[0];
                    next.to = selected[1];
                    commands.push(set_command(piece.id, &next));
                }
                if ui
                    .button(tr("piece.buttons.remove"))
                    .on_hover_text(tr("piece.buttons.remove.hover"))
                    .clicked()
                {
                    commands.push(Command::RemoveButtonGroup {
                        piece: piece.id,
                        group: g.id,
                    });
                }
            });
        }
        let open = self
            .button_form
            .as_ref()
            .is_some_and(|f| f.piece == piece.id);
        if !open {
            if ui
                .button(tr("piece.buttons.add"))
                .on_hover_text(tr("piece.buttons.add.hover"))
                .clicked()
            {
                self.button_form = Some(ButtonForm::new(piece.id));
            }
        } else if let Some(mut form) = self.button_form.take() {
            ui.horizontal_wrapped(|ui| {
                ui.selectable_value(&mut form.buttonhole, false, tr("piece.buttons.kind.button"));
                ui.selectable_value(
                    &mut form.buttonhole,
                    true,
                    tr("piece.buttons.kind.buttonhole"),
                );
            });
            // Orientation hors de la grille : la ligne passe à la ligne au lieu de dépasser du
            // panneau (un bouton hors de la zone visible ne reçoit aucun clic).
            if form.buttonhole {
                ui.horizontal_wrapped(|ui| {
                    for kind in [
                        OrientKind::Horizontal,
                        OrientKind::Vertical,
                        OrientKind::Perpendicular,
                        OrientKind::Angle,
                    ] {
                        ui.selectable_value(&mut form.orient, kind, kind_label(kind));
                    }
                });
            }
            egui::Grid::new(("button-form", piece.id.0))
                .num_columns(2)
                .show(ui, |ui| {
                    crate::a11y::label(ui, tr("piece.buttons.form.count"));
                    crate::a11y::add_text(
                        ui,
                        egui::TextEdit::singleline(&mut form.count).desired_width(70.0),
                    );
                    ui.end_row();
                    if form.buttonhole {
                        crate::a11y::label(ui, tr("piece.buttons.edit.length"));
                        crate::a11y::add_text(
                            ui,
                            egui::TextEdit::singleline(&mut form.length).desired_width(70.0),
                        );
                        ui.end_row();
                        crate::a11y::label(ui, tr("piece.buttons.edit.width"));
                        crate::a11y::add_text(
                            ui,
                            egui::TextEdit::singleline(&mut form.width).desired_width(70.0),
                        );
                        ui.end_row();
                        if form.orient == OrientKind::Angle {
                            crate::a11y::label(ui, tr("piece.buttons.edit.angle"));
                            crate::a11y::add_text(
                                ui,
                                egui::TextEdit::singleline(&mut form.angle).desired_width(70.0),
                            );
                            ui.end_row();
                        }
                    } else {
                        crate::a11y::label(ui, tr("piece.buttons.edit.size"));
                        crate::a11y::add_text(
                            ui,
                            egui::TextEdit::singleline(&mut form.size).desired_width(70.0),
                        );
                        ui.end_row();
                    }
                    crate::a11y::label(ui, tr("piece.buttons.edit.margin"));
                    crate::a11y::add_text(
                        ui,
                        egui::TextEdit::singleline(&mut form.margin).desired_width(70.0),
                    );
                    ui.end_row();
                });
            crate::a11y::label(
                ui,
                RichText::new(tr("piece.buttons.form.hint"))
                    .small()
                    .color(pal().muted),
            );
            let ready = selected.len() == 2;
            let (mut add, mut close) = (false, false);
            ui.horizontal(|ui| {
                add = ui
                    .add_enabled(ready, egui::Button::new(tr("piece.buttons.form.ok")))
                    .clicked();
                close = ui.button(tr("piece.buttons.form.cancel")).clicked();
            });
            if !close {
                if add && ready {
                    self.add_button_group_from_form(piece.id, &form, &selected);
                }
                self.button_form = Some(form);
            }
        }
        for c in commands {
            self.editor.exec(c);
        }
    }

    /// Lit le formulaire (messages de saisie propres) puis envoie `AddButtonGroup` ; le cœur
    /// refuse sans trace ce qui est hors bornes.
    fn add_button_group_from_form(
        &mut self,
        piece: PieceId,
        form: &ButtonForm,
        points: &[PointId],
    ) {
        let Some(count) = parse_count(&form.count) else {
            self.editor.error(tr("piece.buttons.invalid_count"));
            return;
        };
        let length = |text: &str| pf_core::units::parse_length_display(text);
        let element = if form.buttonhole {
            let angle = if form.orient == OrientKind::Angle {
                pf_core::input::parse_angle_deg(&form.angle)
            } else {
                Ok(0.0)
            };
            match (length(&form.length), length(&form.width), angle) {
                (Ok(length_mm), Ok(width_mm), Ok(degrees)) => ButtonElement::Buttonhole {
                    length_mm,
                    width_mm,
                    orientation: orientation_of(form.orient, degrees),
                },
                (_, _, Err(_)) if form.orient == OrientKind::Angle => {
                    self.editor.error(tr("piece.buttons.invalid_angle"));
                    return;
                }
                _ => {
                    self.editor.error(tr("piece.buttons.invalid_size"));
                    return;
                }
            }
        } else {
            match length(&form.size) {
                Ok(diameter_mm) => ButtonElement::Button { diameter_mm },
                Err(_) => {
                    self.editor.error(tr("piece.buttons.invalid_size"));
                    return;
                }
            }
        };
        let Ok(edge_margin_mm) = length(&form.margin) else {
            self.editor.error(tr("piece.buttons.invalid_size"));
            return;
        };
        if self
            .editor
            .exec(Command::AddButtonGroup {
                piece,
                from: points[0],
                to: points[1],
                count,
                element,
                edge_margin_mm,
            })
            .is_some()
        {
            self.editor.info(tr("piece.buttons.added"));
        }
    }
}
