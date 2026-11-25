//! Section « Cotes » de l'inspecteur (US-06.7). Elle ne calcule rien : valeurs, texte, angle du
//! texte et géométrie viennent de `Sheet::full_dimension_geometry`, et chaque action est une
//! commande (`AddFullDimension`, `SetFullDimension*`, `DeleteFullDimension` : une étape
//! annulable, refus sans trace).
//!
//! Une cote est une **mesure observée** : sa valeur suit la construction, elle n'impose rien.
//! Création depuis la sélection (aucun raccourci clavier : boutons seulement) : un tracé
//! (longueur développée, rayon, diamètre), deux points (horizontale, verticale), trois points
//! (sommet d'abord) ou deux segments qui se touchent (angle anti-horaire du premier côté au
//! second). Les cotes ne se sélectionnent pas à la souris : on les gère dans cette section.
//! Aucune liste déroulante ; les valeurs se valident à la perte du focus.

use crate::app::App;
use crate::palette::pal;
use crate::panels::buffered_edit;
use eframe::egui::{self, RichText};
use pf_core::full_dimension::MAX_FULL_DIMENSION_LABEL_CHARS;
use pf_core::input::{decimal_separator, format_number};
use pf_core::{Command, EntityId, FullDimension, FullDimensionKind, PointId};
use pf_i18n::{tr, tr_args};

/// Libellé traduit du type d'une cote.
pub fn kind_label(kind: &FullDimensionKind) -> String {
    match kind {
        FullDimensionKind::PathLength { .. } => tr("full_dim.kind.path_length"),
        FullDimensionKind::Angle { .. } => tr("full_dim.kind.angle"),
        FullDimensionKind::Radius { .. } => tr("full_dim.kind.radius"),
        FullDimensionKind::Diameter { .. } => tr("full_dim.kind.diameter"),
        FullDimensionKind::Horizontal { .. } => tr("full_dim.kind.horizontal"),
        FullDimensionKind::Vertical { .. } => tr("full_dim.kind.vertical"),
    }
}

/// Étiquette et clé du champ de mise en page d'une cote selon son type.
fn placement_label(kind: &FullDimensionKind) -> String {
    match kind {
        FullDimensionKind::Angle { .. } => tr("full_dim.edit.arc_radius"),
        FullDimensionKind::Radius { .. } | FullDimensionKind::Diameter { .. } => {
            tr("full_dim.edit.direction")
        }
        _ => tr("full_dim.edit.offset"),
    }
}

/// Texte de la ligne d'une cote : type et valeur mesurée (séparateur décimal de la langue),
/// ou le motif « non calculable » expliqué par le cœur.
fn row_text(sheet: &pf_core::Sheet, d: &FullDimension) -> String {
    match sheet.full_dimension_geometry(d.id) {
        Ok(g) => {
            let mut text = tr_args(
                "full_dim.row",
                &[
                    ("kind", &kind_label(&d.kind)),
                    ("value", &g.text(decimal_separator())),
                ],
            );
            // Projections : la valeur signée est lisible (ΔX = x(arrivée) − x(départ)).
            if matches!(
                d.kind,
                FullDimensionKind::Horizontal { .. } | FullDimensionKind::Vertical { .. }
            ) {
                text.push_str(&tr_args(
                    "full_dim.signed",
                    &[("value", &format_number(g.value, 2))],
                ));
            }
            text
        }
        Err(e) => tr_args(
            "full_dim.row.bad",
            &[("kind", &kind_label(&d.kind)), ("reason", &e.to_string())],
        ),
    }
}

/// Cote créable depuis la sélection courante, ou `None` (le bouton est alors grisé).
struct Creatable {
    path_length: Option<FullDimensionKind>,
    angle: Option<FullDimensionKind>,
    radius: Option<FullDimensionKind>,
    diameter: Option<FullDimensionKind>,
    horizontal: Option<FullDimensionKind>,
    vertical: Option<FullDimensionKind>,
}

/// Propositions de cotes pour la sélection : mise en page par défaut, jamais écrite avant
/// l'envoi de la commande (le cœur refuse ce qui n'a pas de sens).
fn creatable(sheet: &pf_core::Sheet, points: &[PointId], entities: &[EntityId]) -> Creatable {
    use pf_core::full_dimension::DEFAULT_ANGLE_ARC_RADIUS_MM as ARC;
    let offset = pf_core::dimension::DEFAULT_DIMENSION_OFFSET_MM;
    let mut c = Creatable {
        path_length: None,
        angle: None,
        radius: None,
        diameter: None,
        horizontal: None,
        vertical: None,
    };
    if let ([], [e]) = (points, entities) {
        c.path_length = Some(FullDimensionKind::PathLength {
            entity: *e,
            offset_mm: offset,
        });
        if let Ok(angle_deg) = sheet.default_leader_angle_deg(*e) {
            c.radius = Some(FullDimensionKind::Radius {
                entity: *e,
                angle_deg,
            });
            c.diameter = Some(FullDimensionKind::Diameter {
                entity: *e,
                angle_deg,
            });
        }
    }
    if let ([a, b], []) = (points, entities) {
        c.horizontal = Some(FullDimensionKind::Horizontal {
            from: *a,
            to: *b,
            offset_mm: offset,
        });
        c.vertical = Some(FullDimensionKind::Vertical {
            from: *a,
            to: *b,
            offset_mm: offset,
        });
    }
    match (points, entities) {
        // Trois points : le premier sélectionné est le sommet, le deuxième porte le premier
        // côté, le troisième le second (sens anti-horaire du premier au second).
        ([v, a, b], []) => {
            c.angle = Some(FullDimensionKind::Angle {
                vertex: *v,
                first: *a,
                second: *b,
                radius_mm: ARC,
            });
        }
        // Deux segments qui partagent une extrémité : sommet = extrémité commune.
        ([], [s1, s2]) => {
            if let (Ok((a1, b1)), Ok((a2, b2))) =
                (sheet.segment_point_ids(*s1), sheet.segment_point_ids(*s2))
            {
                let shared = [
                    (a1, b1, a2, b2),
                    (a1, b1, b2, a2),
                    (b1, a1, a2, b2),
                    (b1, a1, b2, a2),
                ]
                .into_iter()
                .find(|(v1, _, v2, _)| v1 == v2);
                if let Some((vertex, first, _, second)) = shared {
                    c.angle = Some(FullDimensionKind::Angle {
                        vertex,
                        first,
                        second,
                        radius_mm: ARC,
                    });
                }
            }
        }
        _ => {}
    }
    c
}

impl App {
    /// Section « Cotes » : liste des cotes de la feuille active (valeur, légende, position du
    /// texte, mise en page, retrait) et boutons de création depuis la sélection.
    pub(crate) fn full_dimension_section(&mut self, ui: &mut egui::Ui) {
        let sheet_id = self.editor.sheet;
        let (dims, rows, offered): (Vec<FullDimension>, Vec<String>, Creatable) = {
            let s = self.editor.sheet();
            let sel = &self.editor.selection;
            (
                s.full_dimensions.clone(),
                s.full_dimensions.iter().map(|d| row_text(s, d)).collect(),
                creatable(s, &sel.points, &sel.entities),
            )
        };
        // Repliée par défaut ; ouverte quand la feuille a des cotes ou qu'une sélection existe.
        let linked = !dims.is_empty() || !self.editor.selection.is_empty();
        crate::panels::fold(ui, "fold-fulldim", &tr("full_dim.title"), linked, |ui| {
            crate::a11y::label(
                ui,
                RichText::new(tr("full_dim.note"))
                    .small()
                    .color(pal().muted),
            );
            let mut commands: Vec<Command> = Vec::new();
            if dims.is_empty() {
                crate::a11y::label(
                    ui,
                    RichText::new(tr("full_dim.none"))
                        .small()
                        .color(pal().muted),
                );
            }
            for (d, text) in dims.iter().zip(&rows) {
                let key = 0x6700_0000_0000 | (d.id.0 << 4);
                crate::a11y::label(ui, text);
                ui.horizontal_wrapped(|ui| {
                    // Légende : texte libre court, accents autorisés.
                    crate::a11y::label(ui, RichText::new(tr("full_dim.edit.label")).small());
                    let current = d.label.clone().unwrap_or_default();
                    if let Some(text) =
                        buffered_edit(ui, &mut self.text_buf, key | 1, &current, 110.0)
                    {
                        commands.push(Command::SetFullDimensionLabel {
                            sheet: sheet_id,
                            dimension: d.id,
                            label: Some(text),
                        });
                    }
                    // Position du texte le long de la ligne de cote (mm).
                    crate::a11y::label(ui, RichText::new(tr("full_dim.edit.shift")).small());
                    let current = pf_core::units::field_text(d.text_shift_mm);
                    if let Some(text) =
                        buffered_edit(ui, &mut self.text_buf, key | 2, &current, 56.0)
                    {
                        match pf_core::units::parse_length_display(&text) {
                            Ok(v) => commands.push(Command::SetFullDimensionTextShift {
                                sheet: sheet_id,
                                dimension: d.id,
                                text_shift_mm: v,
                            }),
                            Err(_) => self.editor.error(tr("full_dim.invalid_number")),
                        }
                    }
                    // Mise en page : décalage de la ligne, rayon de l'arc ou direction du trait.
                    crate::a11y::label(ui, RichText::new(placement_label(&d.kind)).small());
                    let current = match d.kind {
                        // Direction du trait (degrés) : pas une longueur.
                        FullDimensionKind::Radius { .. } | FullDimensionKind::Diameter { .. } => {
                            format_number(d.kind.placement(), 3)
                        }
                        _ => pf_core::units::field_text(d.kind.placement()),
                    };
                    if let Some(text) =
                        buffered_edit(ui, &mut self.text_buf, key | 3, &current, 56.0)
                    {
                        let parsed = match d.kind {
                            FullDimensionKind::Radius { .. }
                            | FullDimensionKind::Diameter { .. } => {
                                pf_core::input::parse_angle_deg(&text)
                            }
                            _ => pf_core::units::parse_length_display(&text),
                        };
                        match parsed {
                            Ok(v) => commands.push(Command::SetFullDimensionPlacement {
                                sheet: sheet_id,
                                dimension: d.id,
                                placement: v,
                            }),
                            Err(_) => self.editor.error(tr("full_dim.invalid_number")),
                        }
                    }
                    if ui
                        .button(tr("full_dim.remove"))
                        .on_hover_text(tr("full_dim.remove.hover"))
                        .clicked()
                    {
                        commands.push(Command::DeleteFullDimension {
                            sheet: sheet_id,
                            dimension: d.id,
                        });
                    }
                });
            }
            // Création depuis la sélection : un bouton par type, grisé quand elle ne convient pas.
            crate::a11y::label(
                ui,
                RichText::new(tr("full_dim.add.hint"))
                    .small()
                    .color(pal().muted),
            );
            let mut add: Option<FullDimensionKind> = None;
            ui.horizontal_wrapped(|ui| {
                for (offer, label, hover) in [
                    (
                        offered.path_length,
                        "full_dim.add.path_length",
                        "full_dim.add.path_length.hover",
                    ),
                    (
                        offered.angle,
                        "full_dim.add.angle",
                        "full_dim.add.angle.hover",
                    ),
                    (
                        offered.radius,
                        "full_dim.add.radius",
                        "full_dim.add.radius.hover",
                    ),
                    (
                        offered.diameter,
                        "full_dim.add.diameter",
                        "full_dim.add.diameter.hover",
                    ),
                    (
                        offered.horizontal,
                        "full_dim.add.horizontal",
                        "full_dim.add.horizontal.hover",
                    ),
                    (
                        offered.vertical,
                        "full_dim.add.vertical",
                        "full_dim.add.vertical.hover",
                    ),
                ] {
                    let resp = ui.add_enabled(offer.is_some(), egui::Button::new(tr(label)));
                    if resp.on_hover_text(tr(hover)).clicked() {
                        add = offer;
                    }
                }
            });
            if let Some(kind) = add {
                commands.push(Command::AddFullDimension {
                    sheet: sheet_id,
                    kind,
                    label: None,
                    text_shift_mm: 0.0,
                });
            }
            for c in commands {
                let is_add = matches!(c, Command::AddFullDimension { .. });
                if self.editor.exec(c).is_some() && is_add {
                    self.editor.info(tr("full_dim.added"));
                }
            }
            // La borne de la légende est annoncée à qui la saisit (une fois, discret).
            if !dims.is_empty() {
                crate::a11y::label(
                    ui,
                    RichText::new(tr_args(
                        "full_dim.label.limit",
                        &[("max", &MAX_FULL_DIMENSION_LABEL_CHARS.to_string())],
                    ))
                    .small()
                    .color(pal().muted),
                );
            }
        });
    }
}
