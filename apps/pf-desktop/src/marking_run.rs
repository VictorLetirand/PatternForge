//! Bloc « Perçages au pas… » du formulaire de marquages de l'inspecteur d'une pièce (PH-4b).
//! Il ne calcule rien : positions, espacement réel et refus viennent de
//! `Project::marking_run_preview` (lecture seule) et de `Command::AddPieceMarkingRun` (une étape
//! annulable, refus sans trace). Le bord se prend dans la sélection (tracés du contour) ; la
//! rangée n'est pas liée au contour après création (positions figées).
//!
//! Replié par défaut. Aucune liste déroulante (les tests de l'inspecteur de pièce comptent les
//! `ComboBox`) : les choix sont des boutons à bascule.

use crate::app::App;
use crate::palette::pal;
use crate::text::core_error;
use eframe::egui::{self, RichText};
use pf_core::input::format_mm;
use pf_core::units::parse_length_display;
use pf_core::{
    Command, MarkingRunSpec, PieceId, RunForm, RunSide, RunSpacing, MARKING_RUN_MAX_COUNT,
};
use pf_i18n::{tr, tr_args};

/// Lignes de positions montrées dans l'aperçu avant le « … ».
const PREVIEW_LINES: usize = 12;

/// Aperçu affiché : nombre, espacement réel, longueur du bord et positions (texte).
#[derive(Debug, Clone, PartialEq)]
pub struct PitchPreview {
    pub count: usize,
    pub step_mm: f64,
    pub length_mm: f64,
    /// Positions (centres) dans l'ordre des abscisses, telles que la commande les créera.
    pub positions: Vec<(f64, f64)>,
}

/// Formulaire « Perçages au pas… » : état d'interface, jamais enregistré ; la création passe par
/// `Command::AddPieceMarkingRun`.
pub struct PitchForm {
    pub piece: PieceId,
    /// Nombre de perçages plutôt que pas demandé.
    pub by_count: bool,
    pub step: String,
    pub count: String,
    pub distance: String,
    pub start: String,
    pub end: String,
    pub outside: bool,
    /// Fente de poinçon plutôt que perçage rond.
    pub slot: bool,
    pub diameter: String,
    pub length: String,
    pub width: String,
    pub preview: Option<PitchPreview>,
}

impl PitchForm {
    pub fn new(piece: PieceId) -> PitchForm {
        PitchForm {
            piece,
            by_count: false,
            step: "4".into(),
            count: "10".into(),
            distance: "3".into(),
            start: "0".into(),
            end: "0".into(),
            outside: false,
            slot: false,
            diameter: "1.2".into(),
            length: "4".into(),
            width: "1".into(),
            preview: None,
        }
    }

    /// Paramètres saisis ; `Err` = clé de message d'un champ qui n'est pas un nombre.
    pub fn spec(&self, edges: Vec<pf_core::EntityId>) -> Result<MarkingRunSpec, &'static str> {
        let length =
            |text: &str| parse_length_display(text).map_err(|_| "piece.marking.run.invalid");
        let spacing = if self.by_count {
            RunSpacing::Count {
                count: self
                    .count
                    .trim()
                    .parse::<u32>()
                    .map_err(|_| "piece.marking.run.invalid_count")?,
            }
        } else {
            RunSpacing::Step {
                step_mm: length(&self.step)?,
            }
        };
        let form = if self.slot {
            RunForm::Slot {
                length_mm: length(&self.length)?,
                width_mm: length(&self.width)?,
            }
        } else {
            RunForm::Drill {
                diameter_mm: length(&self.diameter)?,
            }
        };
        Ok(MarkingRunSpec {
            edges,
            spacing,
            start_margin_mm: length(&self.start)?,
            end_margin_mm: length(&self.end)?,
            distance_mm: length(&self.distance)?,
            side: if self.outside {
                RunSide::Outside
            } else {
                RunSide::Inside
            },
            form,
        })
    }
}

fn field(ui: &mut egui::Ui, label_key: &str, text: &mut String) {
    crate::a11y::label(ui, tr(label_key));
    crate::a11y::add_text(ui, egui::TextEdit::singleline(text).desired_width(70.0));
    ui.end_row();
}

impl App {
    /// Bloc replié « Perçages au pas… » de la section de marquages d'une pièce.
    pub(crate) fn piece_pitch_block(&mut self, ui: &mut egui::Ui, piece: &pf_core::Piece) {
        let mut header = egui::CollapsingHeader::new(
            RichText::new(tr("piece.marking.run.title"))
                .small()
                .strong()
                .color(pal().muted),
        )
        .id_salt(("marking-run", piece.id.0))
        .default_open(false);
        // Étape de capture : ouvert une fois, et amené dans la partie visible du panneau.
        let once = std::mem::take(&mut self.pitch_open_once);
        if once {
            header = header.open(Some(true));
        }
        let shown = header.show(ui, |ui| self.pitch_body(ui, piece.id));
        crate::panels::scroll_block_into_view(
            ui,
            ("pitch", piece.id.0),
            shown.header_response.rect,
            once,
        );
    }

    fn pitch_body(&mut self, ui: &mut egui::Ui, piece: PieceId) {
        let mut form = match self.pitch_form.take() {
            Some(f) if f.piece == piece => f,
            _ => PitchForm::new(piece),
        };
        crate::a11y::label(
            ui,
            RichText::new(tr("piece.marking.run.hint"))
                .small()
                .color(pal().muted),
        );
        // Bord : tracés du contour pris dans la sélection (ordre du contour calculé par le cœur).
        let edges = self
            .editor
            .project()
            .marking_run_edges(piece, &self.editor.selection.entities);
        match &edges {
            Ok(e) => {
                crate::a11y::label(
                    ui,
                    tr_args("piece.marking.run.edge", &[("count", &e.len().to_string())]),
                );
            }
            Err(e) => {
                crate::a11y::label(
                    ui,
                    RichText::new(core_error(e))
                        .small()
                        .color(ui.visuals().warn_fg_color),
                );
            }
        }
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(&mut form.by_count, false, tr("piece.marking.run.mode.step"));
            ui.selectable_value(&mut form.by_count, true, tr("piece.marking.run.mode.count"));
        });
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(
                &mut form.outside,
                false,
                tr("piece.marking.run.side.inside"),
            );
            ui.selectable_value(
                &mut form.outside,
                true,
                tr("piece.marking.run.side.outside"),
            );
        });
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(&mut form.slot, false, tr("piece.marking.run.form.drill"));
            ui.selectable_value(&mut form.slot, true, tr("piece.marking.run.form.slot"));
        });
        egui::Grid::new(("pitch-form", piece.0))
            .num_columns(2)
            .show(ui, |ui| {
                if form.by_count {
                    field(ui, "piece.marking.run.count", &mut form.count);
                } else {
                    field(ui, "piece.marking.run.step", &mut form.step);
                }
                field(ui, "piece.marking.run.distance", &mut form.distance);
                field(ui, "piece.marking.run.start", &mut form.start);
                field(ui, "piece.marking.run.end", &mut form.end);
                if form.slot {
                    field(ui, "piece.marking.run.length", &mut form.length);
                    field(ui, "piece.marking.run.width", &mut form.width);
                } else {
                    field(ui, "piece.marking.run.diameter", &mut form.diameter);
                }
            });
        let ready = edges.is_ok();
        let (mut preview, mut create) = (false, false);
        ui.horizontal_wrapped(|ui| {
            preview = ui
                .add_enabled(ready, egui::Button::new(tr("piece.marking.run.preview")))
                .clicked();
            create = ui
                .add_enabled(ready, egui::Button::new(tr("piece.marking.run.create")))
                .clicked();
        });
        if let (true, Ok(edges)) = (preview || create, edges) {
            match form.spec(edges) {
                Err(key) => {
                    form.preview = None;
                    self.editor.error(tr(key));
                }
                Ok(spec) if preview => {
                    match self.editor.project().marking_run_preview(piece, &spec) {
                        Ok(l) => {
                            form.preview = Some(PitchPreview {
                                count: l.holes.len(),
                                step_mm: l.step_mm,
                                length_mm: l.length_mm,
                                positions: l
                                    .holes
                                    .iter()
                                    .map(|h| (h.position.x, h.position.y))
                                    .collect(),
                            });
                        }
                        Err(e) => {
                            form.preview = None;
                            self.editor.error(core_error(&e));
                        }
                    }
                }
                Ok(spec) => {
                    let command = Command::AddPieceMarkingRun { piece, run: spec };
                    if let Some(o) = self.editor.exec(command) {
                        form.preview = None;
                        self.editor.info(tr_args(
                            "piece.marking.run.added",
                            &[("count", &o.markings.len().to_string())],
                        ));
                    }
                }
            }
        }
        if let Some(p) = &form.preview {
            let requested = if form.by_count {
                String::new()
            } else {
                pf_i18n::tr_len("piece.marking.run.requested", &[("step", form.step.trim())])
            };
            crate::a11y::label(
                ui,
                RichText::new(tr_args(
                    "piece.marking.run.result",
                    &[
                        ("count", &p.count.to_string()),
                        ("step", &format_mm(p.step_mm)),
                        ("length", &format_mm(p.length_mm)),
                        ("requested", &requested),
                    ],
                ))
                .small(),
            );
            for (i, (x, y)) in p.positions.iter().take(PREVIEW_LINES).enumerate() {
                crate::a11y::label(
                    ui,
                    RichText::new(tr_args(
                        "piece.marking.run.position",
                        &[
                            ("n", &(i + 1).to_string()),
                            ("x", &format_mm(*x)),
                            ("y", &format_mm(*y)),
                        ],
                    ))
                    .small()
                    .color(pal().muted),
                );
            }
            if p.positions.len() > PREVIEW_LINES {
                crate::a11y::label(
                    ui,
                    RichText::new(tr_args(
                        "piece.marking.run.more",
                        &[("count", &(p.positions.len() - PREVIEW_LINES).to_string())],
                    ))
                    .small()
                    .color(pal().muted),
                );
            }
        }
        crate::a11y::label(
            ui,
            RichText::new(tr_args(
                "piece.marking.run.note",
                &[("max", &MARKING_RUN_MAX_COUNT.to_string())],
            ))
            .small()
            .color(pal().muted),
        );
        self.pitch_form = Some(form);
    }
}

/// Projet de démonstration (étape de capture) : fond de porte-cartes 100 × 70 mm à congés de
/// 5 mm, rangée de 23 perçages ronds sur le bord bas et rangée de 8 fentes sur le bord droit.
pub(crate) fn demo_pitch_project() -> Result<pf_core::Project, pf_core::CoreError> {
    use pf_core::{PieceSource, PointRef, Project};
    use pf_geometry::PointMm;
    let pt = PointMm::new;
    let mut p = Project::new("Percages au pas");
    let sheet = p.sheets[0].id;
    let seg = |p: &mut Project, a: PointMm, b: PointMm| -> Result<_, pf_core::CoreError> {
        Ok(p.apply(Command::AddSegment {
            sheet,
            from: PointRef::New(a),
            to: PointRef::New(b),
        })?
        .entities[0])
    };
    let bottom = seg(&mut p, pt(0.0, 0.0), pt(100.0, 0.0))?;
    let right = seg(&mut p, pt(100.0, 0.0), pt(100.0, 70.0))?;
    let top = seg(&mut p, pt(100.0, 70.0), pt(0.0, 70.0))?;
    let left = seg(&mut p, pt(0.0, 70.0), pt(0.0, 0.0))?;
    let fillet = |p: &mut Project, first, second, a, b| -> Result<_, pf_core::CoreError> {
        Ok(*p
            .apply(Command::FilletPaths {
                sheet,
                first,
                second,
                radius_mm: 5.0,
                pick_first: a,
                pick_second: b,
            })?
            .entities
            .last()
            .expect("fillet arc"))
    };
    let a01 = fillet(&mut p, bottom, right, pt(50.0, 0.0), pt(100.0, 35.0))?;
    let a12 = fillet(&mut p, right, top, pt(100.0, 35.0), pt(50.0, 70.0))?;
    let a23 = fillet(&mut p, top, left, pt(50.0, 70.0), pt(0.0, 35.0))?;
    let a30 = fillet(&mut p, left, bottom, pt(0.0, 35.0), pt(50.0, 0.0))?;
    let piece = p
        .apply(Command::ExtractPiece {
            sheet,
            source: PieceSource::Paths {
                entities: vec![bottom, a01, right, a12, top, a23, left, a30],
            },
            name: "Fond".into(),
            detached: false,
        })?
        .pieces[0];
    let run = |edge, spacing, margins: (f64, f64), form| MarkingRunSpec {
        edges: vec![edge],
        spacing,
        start_margin_mm: margins.0,
        end_margin_mm: margins.1,
        distance_mm: 3.0,
        side: RunSide::Inside,
        form,
    };
    p.apply(Command::AddPieceMarkingRun {
        piece,
        run: run(
            bottom,
            RunSpacing::Step { step_mm: 4.0 },
            (1.0, 1.0),
            RunForm::Drill { diameter_mm: 1.2 },
        ),
    })?;
    p.apply(Command::AddPieceMarkingRun {
        piece,
        run: run(
            right,
            RunSpacing::Count { count: 8 },
            (6.0, 6.0),
            RunForm::Slot {
                length_mm: 4.0,
                width_mm: 1.0,
            },
        ),
    })?;
    Ok(p)
}

/// Étape de capture : le bord haut est sélectionné, le bloc « Perçages au pas… » est ouvert avec
/// l'aperçu d'une rangée de perçages (la création n'est pas faite).
pub(crate) fn demo_pitch(app: &mut App) {
    let Some(piece) = app.editor.project().pieces.first().map(|p| p.id) else {
        return;
    };
    let project = app.editor.project();
    let sheet = &project.sheets[0];
    let top = sheet.entities.iter().map(|e| e.id).find(|id| {
        sheet
            .segment_ends(*id)
            .is_ok_and(|(a, b)| a.y == 70.0 && b.y == 70.0)
    });
    let mut form = PitchForm::new(piece);
    form.start = "1".into();
    form.end = "1".into();
    if let Some(top) = top {
        if let Ok(edges) = project.marking_run_edges(piece, &[top]) {
            if let Ok(spec) = form.spec(edges) {
                if let Ok(l) = project.marking_run_preview(piece, &spec) {
                    form.preview = Some(PitchPreview {
                        count: l.holes.len(),
                        step_mm: l.step_mm,
                        length_mm: l.length_mm,
                        positions: l
                            .holes
                            .iter()
                            .map(|h| (h.position.x, h.position.y))
                            .collect(),
                    });
                }
            }
        }
        app.editor.selection.entities = vec![top];
    }
    // Un point sélectionné ouvre la section de marquages (liée à la sélection de points).
    if let Some(first) = app.editor.sheet().points.first().map(|p| p.id) {
        app.editor.selection.points = vec![first];
    }
    app.pitch_form = Some(form);
    app.pitch_open_once = true;
}
