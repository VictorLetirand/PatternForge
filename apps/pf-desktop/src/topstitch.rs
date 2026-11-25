//! Surpiqûre liée à un bord (US-06.8) dans l'inspecteur : création depuis la sélection de bord
//! (un ou plusieurs tracés de la feuille) ou depuis le contour d'une pièce, réglage de la
//! distance et du côté, indicateur d'obsolescence, « Casser le lien ». Rien ne se calcule ici :
//! le côté proposé (intérieur / extérieur ou gauche / droite) vient de
//! `Project::topstitch_chain_kind`, et chaque action est une commande (`AddTopstitch`,
//! `SetTopstitch`, `BreakTopstitchLink` : une étape annulable, refus sans trace).
//!
//! Limites : la distance se saisit en mm (validée à la perte du focus pour une surpiqûre
//! existante) ; aucune liste déroulante (les tests de l'inspecteur comptent les `ComboBox`) ;
//! aucun raccourci.

use crate::app::App;
use crate::palette::pal;
use crate::panels::buffered_edit;
use crate::text::core_error;
use eframe::egui::{self, RichText};
use pf_core::input::format_mm;
use pf_core::{
    ChainKind, Command, EntityId, Topstitch, TopstitchSide, TopstitchSource, TopstitchState,
};
use pf_i18n::{tr, tr_args};

/// Libellé traduit d'un côté de surpiqûre.
pub fn side_label(side: TopstitchSide) -> String {
    match side {
        TopstitchSide::Inside => tr("topstitch.side.inside"),
        TopstitchSide::Outside => tr("topstitch.side.outside"),
        TopstitchSide::Left => tr("topstitch.side.left"),
        TopstitchSide::Right => tr("topstitch.side.right"),
    }
}

/// Texte d'état d'une surpiqûre : « À jour » ou « ⚠ Obsolète : raison ».
pub fn state_text(t: &Topstitch) -> String {
    match t.state() {
        TopstitchState::Current => tr("topstitch.state.current"),
        TopstitchState::Stale { reason } => {
            format!(
                "⚠ {}",
                tr_args("topstitch.state.stale", &[("reason", &reason)])
            )
        }
    }
}

impl App {
    /// Distance saisie dans le formulaire de création, en mm.
    fn topstitch_distance_mm(&mut self) -> Option<f64> {
        match pf_core::units::parse_length_display(&self.topstitch_distance) {
            Ok(v) => Some(v),
            Err(_) => {
                self.editor.error(tr("topstitch.invalid_distance"));
                None
            }
        }
    }

    /// Champ de distance du formulaire de création (état d'interface, jamais enregistré). La
    /// fiche de la pièce (panneau de gauche) a ses propres libellés : la sélection et la fiche
    /// peuvent être affichées ensemble.
    fn topstitch_distance_field(&mut self, ui: &mut egui::Ui, piece: bool) {
        ui.horizontal(|ui| {
            let key = if piece {
                "topstitch.piece.distance"
            } else {
                "topstitch.distance"
            };
            crate::a11y::label(ui, RichText::new(tr(key)).small());
            crate::a11y::add_text(
                ui,
                egui::TextEdit::singleline(&mut self.topstitch_distance).desired_width(60.0),
            );
        });
    }

    /// Une surpiqûre liée existante : état, distance, côté, « Casser le lien ».
    fn topstitch_link_editor(&mut self, ui: &mut egui::Ui, t: &Topstitch) {
        let sheet = self.editor.sheet;
        let project = self.editor.project();
        let source_text = match &t.source {
            TopstitchSource::Piece { piece } => tr_args(
                "topstitch.source.piece",
                &[(
                    "name",
                    &project
                        .piece(*piece)
                        .map(|p| p.name.clone())
                        .unwrap_or_default(),
                )],
            ),
            TopstitchSource::Paths { paths, .. } => {
                tr_args("topstitch.source.paths", &[("n", &paths.len().to_string())])
            }
        };
        let kind = project.topstitch_chain_kind(sheet, &t.source).ok();
        let sides: Vec<TopstitchSide> = match kind {
            Some(ChainKind::Closed) => vec![TopstitchSide::Inside, TopstitchSide::Outside],
            Some(ChainKind::Open { .. }) => vec![TopstitchSide::Left, TopstitchSide::Right],
            None => vec![t.side],
        };
        crate::a11y::label(ui, RichText::new(source_text).small());
        let stale = t.stale.is_some();
        crate::a11y::label(
            ui,
            RichText::new(state_text(t)).small().color(if stale {
                pal().error
            } else {
                pal().muted
            }),
        );
        if stale {
            crate::a11y::label(
                ui,
                RichText::new(tr("topstitch.stale_hint"))
                    .small()
                    .color(pal().muted),
            );
        }
        let mut commands: Vec<Command> = Vec::new();
        ui.horizontal_wrapped(|ui| {
            crate::a11y::label(ui, RichText::new(tr("topstitch.distance")).small());
            let current = pf_core::units::field_text(t.distance_mm);
            let key = (t.id.0 << 4) | 0x6;
            if let Some(text) = buffered_edit(ui, &mut self.text_buf, key, &current, 60.0) {
                match pf_core::units::parse_length_display(&text) {
                    Ok(distance_mm) => commands.push(Command::SetTopstitch {
                        sheet,
                        topstitch: t.id,
                        distance_mm,
                        side: t.side,
                    }),
                    Err(_) => self.editor.error(tr("topstitch.invalid_distance")),
                }
            }
            for side in &sides {
                if ui
                    .add(egui::Button::selectable(t.side == *side, side_label(*side)))
                    .clicked()
                    && t.side != *side
                {
                    commands.push(Command::SetTopstitch {
                        sheet,
                        topstitch: t.id,
                        distance_mm: t.distance_mm,
                        side: *side,
                    });
                }
            }
        });
        if ui
            .button(tr("topstitch.break"))
            .on_hover_text(tr("topstitch.break.hover"))
            .clicked()
        {
            commands.push(Command::BreakTopstitchLink {
                sheet,
                topstitch: t.id,
            });
        }
        for c in commands {
            self.editor.exec(c);
        }
    }

    /// Boutons de création pour une source donnée (côtés selon la forme du bord).
    fn topstitch_create_buttons(&mut self, ui: &mut egui::Ui, source: TopstitchSource) {
        let sheet = self.editor.sheet;
        let kind = self.editor.project().topstitch_chain_kind(sheet, &source);
        let sides = match kind {
            Ok(ChainKind::Closed) => [TopstitchSide::Inside, TopstitchSide::Outside],
            Ok(ChainKind::Open { from }) => {
                let label = self.editor.sheet().label(from).to_string();
                crate::a11y::label(
                    ui,
                    RichText::new(tr_args("topstitch.open_hint", &[("from", &label)]))
                        .small()
                        .color(pal().muted),
                );
                [TopstitchSide::Left, TopstitchSide::Right]
            }
            Err(e) => {
                crate::a11y::label(
                    ui,
                    RichText::new(tr_args(
                        "topstitch.bad_edge",
                        &[("reason", &core_error(&e))],
                    ))
                    .small()
                    .color(pal().muted),
                );
                return;
            }
        };
        let piece = matches!(source, TopstitchSource::Piece { .. });
        self.topstitch_distance_field(ui, piece);
        ui.horizontal_wrapped(|ui| {
            for side in sides {
                let key = if piece {
                    "topstitch.piece.create"
                } else {
                    "topstitch.create"
                };
                let label = tr_args(key, &[("side", &side_label(side))]);
                if ui.button(label).clicked() {
                    if let Some(distance_mm) = self.topstitch_distance_mm() {
                        if self
                            .editor
                            .exec(Command::AddTopstitch {
                                sheet,
                                source: source.clone(),
                                distance_mm,
                                side,
                            })
                            .is_some()
                        {
                            self.editor.info(tr("topstitch.created"));
                        }
                    }
                }
            }
        });
    }

    /// Section de l'inspecteur quand des tracés (et seulement eux) sont sélectionnés : la
    /// surpiqûre liée dont l'un est une ligne générée, sinon la création d'une surpiqûre le long
    /// de la sélection.
    pub(crate) fn topstitch_section(&mut self, ui: &mut egui::Ui) {
        let sel = &self.editor.selection;
        if sel.entities.is_empty()
            || !(sel.points.is_empty()
                && sel.notches.is_empty()
                && sel.dimensions.is_empty()
                && sel.guides.is_empty())
        {
            return;
        }
        let entities: Vec<EntityId> = sel.entities.clone();
        let existing = {
            let s = self.editor.sheet();
            entities
                .iter()
                .find_map(|e| s.topstitch_of_path(*e))
                .cloned()
        };
        // Section liée à la sélection (un tracé) : s'ouvre à la sélection, repliable ensuite.
        crate::panels::fold(
            ui,
            "fold-topstitch-selection",
            &tr("topstitch.title"),
            true,
            |ui| {
                match existing {
                    Some(t) => self.topstitch_link_editor(ui, &t),
                    None => {
                        self.topstitch_create_buttons(
                            ui,
                            TopstitchSource::Paths {
                                paths: entities,
                                from: None,
                            },
                        );
                    }
                }
                crate::a11y::label(
                    ui,
                    RichText::new(tr("topstitch.note"))
                        .small()
                        .color(pal().muted),
                );
            },
        );
    }

    /// Sous-section de la fiche d'une pièce : surpiqûres liées à son contour et création.
    pub(crate) fn piece_topstitch_section(&mut self, ui: &mut egui::Ui, piece: &pf_core::Piece) {
        if piece.outline.is_empty() {
            return;
        }
        let links: Vec<Topstitch> = self
            .editor
            .sheet()
            .topstitches
            .iter()
            .filter(|t| t.source == TopstitchSource::Piece { piece: piece.id })
            .cloned()
            .collect();
        if links.is_empty() {
            crate::a11y::label(
                ui,
                RichText::new(tr("topstitch.piece.none"))
                    .small()
                    .color(pal().muted),
            );
        }
        for t in &links {
            crate::a11y::label(
                ui,
                RichText::new(tr_args(
                    "topstitch.piece.row",
                    &[
                        ("distance", &format_mm(t.distance_mm)),
                        ("side", &side_label(t.side)),
                        ("state", &state_text(t)),
                    ],
                ))
                .small(),
            );
        }
        self.topstitch_create_buttons(ui, TopstitchSource::Piece { piece: piece.id });
    }
}
