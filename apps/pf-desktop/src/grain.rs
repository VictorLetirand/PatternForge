//! Section « Droit-fil avancé » de l'inspecteur d'une pièce (US-06.5). Elle ne calcule rien :
//! la flèche, son angle et le contrôle de rotation viennent de `Project::piece_grain_lines` et
//! `Project::piece_grain_rotation`, et chaque modification est **une** commande
//! `SetPieceGrain` (une étape annulable, refus sans trace).
//!
//! Limites : la flèche calculée n'est pas cliquable sur la feuille (le tracé porteur, dessiné
//! en filigrane, se sélectionne) ; une ligne supplémentaire se prend dans la sélection ; aucune
//! liste déroulante (les tests de l'inspecteur de pièce comptent les `ComboBox`), le tracé de
//! référence se choisit dans une liste de tracés.

use crate::app::App;
use crate::palette::pal;
use crate::panels::buffered_edit;
use eframe::egui::{self, RichText};
use pf_core::input::{format_deg, format_mm, format_number};
use pf_core::{
    Command, EntityId, ExtraGrainLine, GrainDirection, GrainLineSpec, GrainSense, PathRole,
    PieceGrain, Shape, GRAIN_MAX_EXTRA_LINES,
};
use pf_i18n::{tr, tr_args};

/// Mode de direction choisi dans l'inspecteur.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Drawn,
    Parallel,
    Perpendicular,
    BiasPlus,
    BiasMinus,
}

const MODES: [Mode; 5] = [
    Mode::Drawn,
    Mode::Parallel,
    Mode::Perpendicular,
    Mode::BiasPlus,
    Mode::BiasMinus,
];

fn mode_of(d: GrainDirection) -> Mode {
    match d {
        GrainDirection::Drawn {} => Mode::Drawn,
        GrainDirection::Parallel { .. } => Mode::Parallel,
        GrainDirection::Perpendicular { .. } => Mode::Perpendicular,
        GrainDirection::Bias { angle_deg } if angle_deg < 0.0 => Mode::BiasMinus,
        GrainDirection::Bias { .. } => Mode::BiasPlus,
    }
}

fn mode_label(m: Mode) -> String {
    match m {
        Mode::Drawn => tr("piece.grain.mode.drawn"),
        Mode::Parallel => tr("piece.grain.mode.parallel"),
        Mode::Perpendicular => tr("piece.grain.mode.perpendicular"),
        Mode::BiasPlus => tr("piece.grain.mode.bias_plus"),
        Mode::BiasMinus => tr("piece.grain.mode.bias_minus"),
    }
}

/// Libellé traduit du sens du tissu.
pub fn sense_label(s: GrainSense) -> String {
    match s {
        GrainSense::Unmarked => tr("piece.grain.sense.none"),
        GrainSense::Nap => tr("piece.grain.sense.nap"),
        GrainSense::Pattern => tr("piece.grain.sense.pattern"),
    }
}

/// Éditeur d'une ligne (principale ou supplémentaire) : direction, tracé de référence, sens et
/// inversion. Renvoie les nouveaux réglages s'ils changent. `salt` distingue les lignes.
fn line_editor(
    ui: &mut egui::Ui,
    salt: u64,
    spec: GrainLineSpec,
    refs: &[(EntityId, String)],
    suggested: Option<EntityId>,
) -> Option<GrainLineSpec> {
    let mut next = spec;
    ui.horizontal_wrapped(|ui| {
        crate::a11y::label(ui, RichText::new(tr("piece.grain.direction")).small());
        let now = mode_of(spec.direction);
        for m in MODES {
            let needs_ref = matches!(m, Mode::Parallel | Mode::Perpendicular);
            let enabled = !needs_ref || !refs.is_empty();
            let mut button =
                ui.add_enabled(enabled, egui::Button::selectable(now == m, mode_label(m)));
            if matches!(m, Mode::BiasPlus | Mode::BiasMinus) {
                button = button.on_hover_text(tr("piece.grain.bias.hover"));
            }
            if button.clicked() && now != m {
                let reference = spec
                    .direction
                    .reference()
                    .or(suggested)
                    .or_else(|| refs.first().map(|r| r.0));
                next.direction = match (m, reference) {
                    (Mode::Drawn, _) => GrainDirection::Drawn {},
                    (Mode::Parallel, Some(reference)) => GrainDirection::Parallel { reference },
                    (Mode::Perpendicular, Some(reference)) => {
                        GrainDirection::Perpendicular { reference }
                    }
                    (Mode::BiasPlus, _) => GrainDirection::Bias { angle_deg: 45.0 },
                    (Mode::BiasMinus, _) => GrainDirection::Bias { angle_deg: -45.0 },
                    (_, None) => spec.direction,
                };
            }
        }
    });
    if let Some(current) = spec.direction.reference() {
        crate::a11y::label(ui, RichText::new(tr("piece.grain.reference")).small());
        egui::ScrollArea::vertical()
            .id_salt(("grain-refs", salt))
            .max_height(96.0)
            .show(ui, |ui| {
                for (id, name) in refs {
                    if ui
                        .add(egui::Button::selectable(*id == current, name))
                        .clicked()
                        && *id != current
                    {
                        next.direction = match spec.direction {
                            GrainDirection::Perpendicular { .. } => {
                                GrainDirection::Perpendicular { reference: *id }
                            }
                            _ => GrainDirection::Parallel { reference: *id },
                        };
                    }
                }
            });
    } else if refs.is_empty() {
        crate::a11y::label(
            ui,
            RichText::new(tr("piece.grain.reference.none"))
                .small()
                .color(pal().muted),
        );
    }
    ui.horizontal_wrapped(|ui| {
        crate::a11y::label(ui, RichText::new(tr("piece.grain.sense")).small());
        for s in GrainSense::ALL {
            if ui
                .add(egui::Button::selectable(spec.sense == s, sense_label(s)))
                .on_hover_text(tr("piece.grain.sense.hover"))
                .clicked()
                && spec.sense != s
            {
                next.sense = s;
            }
        }
        let mut reversed = spec.reversed;
        if ui
            .checkbox(&mut reversed, tr("piece.grain.reversed"))
            .changed()
        {
            next.reversed = reversed;
        }
    });
    (next != spec).then_some(next)
}

impl App {
    /// Droit-fil avancé d'une pièce qui a un droit-fil : direction, sens, tolérance de rotation
    /// et lignes supplémentaires ; mesures observées en lecture seule.
    pub(crate) fn piece_grain_section(&mut self, ui: &mut egui::Ui, piece: &pf_core::Piece) {
        let Some(main) = piece.grainline else {
            return;
        };
        let (refs, candidates, names, lines, rotation) = {
            let project = self.editor.project();
            let Some(sheet) = project.sheets.iter().find(|s| s.entity(main).is_ok()) else {
                return;
            };
            let chord_ok = |id: EntityId| {
                sheet.entity_points(id).is_ok_and(|(p, q)| {
                    match (sheet.position(p), sheet.position(q)) {
                        (Ok(a), Ok(b)) => pf_geometry::distance_mm(a, b) > 1e-6,
                        _ => false,
                    }
                })
            };
            let own: Vec<EntityId> = std::iter::once(main)
                .chain(
                    piece
                        .grain
                        .iter()
                        .flat_map(|g| g.extra.iter().map(|e| e.entity)),
                )
                .collect();
            let refs: Vec<(EntityId, String)> = sheet
                .entities
                .iter()
                .filter(|e| e.role != PathRole::Grainline && chord_ok(e.id))
                .map(|e| (e.id, sheet.entity_name(e.id)))
                .collect();
            // Lignes de rôle droit-fil libres : segments qui ne sont le droit-fil d'aucune pièce.
            let taken = |id: EntityId| {
                project.pieces.iter().any(|o| {
                    o.grainline == Some(id)
                        || o.grain
                            .as_ref()
                            .is_some_and(|g| g.extra.iter().any(|e| e.entity == id))
                })
            };
            let candidates: Vec<EntityId> = self
                .editor
                .selection
                .entities
                .iter()
                .copied()
                .filter(|id| {
                    !own.contains(id)
                        && !taken(*id)
                        && sheet
                            .entity(*id)
                            .is_ok_and(|e| e.role == PathRole::Grainline)
                        && matches!(sheet.shape(*id), Ok(Shape::Line(..)))
                })
                .collect();
            let names: Vec<(EntityId, String)> =
                own.iter().map(|id| (*id, sheet.entity_name(*id))).collect();
            (
                refs,
                candidates,
                names,
                project.piece_grain_lines(piece.id),
                project.piece_grain_rotation(piece.id),
            )
        };
        let name_of = |id: EntityId| {
            names
                .iter()
                .find(|n| n.0 == id)
                .map(|n| n.1.clone())
                .unwrap_or_default()
        };
        let current = piece.grain.clone().unwrap_or_default();
        let mut next = current.clone();
        let key = (piece.id.0 << 8) | 0x70;
        // Flèche calculée : mesure observée, jamais un réglage.
        if let Some(l) = lines.first() {
            crate::a11y::label(
                ui,
                RichText::new(tr_args(
                    "piece.grain.observed",
                    &[
                        ("angle", &format_number(l.direction_deg, 2)),
                        ("length", &format_mm(l.length_mm)),
                    ],
                ))
                .small()
                .color(pal().muted),
            );
        }
        let suggested = self.editor.selection.entities.first().copied();
        if let Some(spec) = line_editor(ui, key, current.main_spec(), &refs, suggested) {
            next.direction = spec.direction;
            next.sense = spec.sense;
            next.reversed = spec.reversed;
        }
        // Tolérance de rotation au placement (degrés, 0 = aucune).
        ui.horizontal_wrapped(|ui| {
            crate::a11y::label(ui, RichText::new(tr("piece.grain.tolerance")).small());
            let shown = format_number(current.tolerance_deg(), 3);
            if let Some(text) = buffered_edit(ui, &mut self.text_buf, key | 1, &shown, 56.0) {
                match pf_core::input::parse_angle_deg(&text) {
                    Ok(v) => next.rotation_tolerance_deg = Some(v),
                    Err(_) => self.editor.error(tr("piece.grain.tolerance.invalid")),
                }
            }
            crate::a11y::label(ui, RichText::new("°").small())
                .on_hover_text(tr("piece.grain.tolerance.hover"));
        });
        if let Some(r) = rotation {
            let text = tr_args(
                "piece.grain.rotation",
                &[
                    ("rotation", &format_deg(r.rotation_deg)),
                    ("deviation", &format_deg(r.deviation_deg)),
                    ("tolerance", &format_deg(r.tolerance_deg)),
                ],
            );
            let color = if r.exceeded() {
                ui.visuals().warn_fg_color
            } else {
                pal().muted
            };
            crate::a11y::label(ui, RichText::new(text).small().color(color));
        }
        // Lignes supplémentaires (rayures, raccords).
        crate::a11y::label(
            ui,
            RichText::new(tr_args(
                "piece.grain.extra.title",
                &[
                    ("count", &current.extra.len().to_string()),
                    ("max", &GRAIN_MAX_EXTRA_LINES.to_string()),
                ],
            ))
            .small()
            .color(pal().muted),
        );
        if current.extra.is_empty() {
            crate::a11y::label(
                ui,
                RichText::new(tr("piece.grain.extra.none"))
                    .small()
                    .color(pal().muted),
            );
        }
        let mut remove = None;
        for (i, extra) in current.extra.iter().enumerate() {
            let header = tr_args(
                "piece.grain.extra.header",
                &[
                    ("name", &name_of(extra.entity)),
                    ("sense", &sense_label(extra.sense)),
                ],
            );
            egui::CollapsingHeader::new(header)
                .id_salt(("grain-extra", piece.id.0, extra.entity.0))
                .default_open(true)
                .show(ui, |ui| {
                    if let Some(spec) = line_editor(
                        ui,
                        key | ((i as u64 + 2) << 1),
                        extra.spec(),
                        &refs,
                        suggested,
                    ) {
                        next.extra[i] = ExtraGrainLine {
                            entity: extra.entity,
                            direction: spec.direction,
                            sense: spec.sense,
                            reversed: spec.reversed,
                        };
                    }
                    if ui.button(tr("piece.grain.extra.remove")).clicked() {
                        remove = Some(i);
                    }
                });
        }
        if let Some(i) = remove {
            next.extra.remove(i);
        }
        if ui
            .add_enabled(
                candidates.len() == 1,
                egui::Button::new(tr("piece.grain.extra.add")),
            )
            .on_hover_text(tr("piece.grain.extra.add.hover"))
            .clicked()
        {
            next.extra.push(ExtraGrainLine {
                entity: candidates[0],
                direction: GrainDirection::Drawn {},
                sense: GrainSense::Unmarked,
                reversed: false,
            });
        }
        if next != current {
            self.send_grain(piece.id, next);
        }
    }

    fn send_grain(&mut self, piece: pf_core::PieceId, grain: PieceGrain) {
        self.editor.exec(Command::SetPieceGrain { piece, grain });
    }
}
