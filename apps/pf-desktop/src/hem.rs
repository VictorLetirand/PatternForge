//! Ourlets et rempliés (US-05.3), dans la section « Marge de couture » de l'inspecteur (repliée par
//! défaut, aucune section de plus). Ne calcule rien : le contour de coupe et les crans de rempli
//! viennent de `Project::piece_cut_contour` / `piece_fold_notches` (recalculés, jamais stockés) ;
//! chaque modification est **une** commande `SetSeamAllowance`. Convention non validée par une
//! modéliste ; aucun export (US-05.4).

use crate::app::App;
use crate::palette::pal;
use crate::panels::buffered_edit;
use eframe::egui::{self, RichText};
use pf_core::units::{field_text, parse_length_display};
use pf_core::{
    Command, EntityId, EntityKind, FoldAllowance, HemAllowance, SeamAllowance, MAX_FOLD_NOTCHES,
    MAX_FOLD_NOTCH_STEP_MM, MAX_SEAM_MM, MIN_FOLD_NOTCH_STEP_MM,
};
use pf_i18n::{tr, tr_args};

/// Hauteur d'ourlet proposée à l'ajout (mm).
const NEW_HEM_MM: f64 = 20.0;
/// Valeur de rempli proposée à l'ajout (mm).
const NEW_FOLD_MM: f64 = 6.0;

fn parse_value(text: &str) -> Option<f64> {
    parse_length_display(text)
        .ok()
        .filter(|v| v.is_finite() && (0.0..=MAX_SEAM_MM).contains(v))
}

fn parse_step(text: &str) -> Option<f64> {
    parse_length_display(text)
        .ok()
        .filter(|v| (MIN_FOLD_NOTCH_STEP_MM..=MAX_FOLD_NOTCH_STEP_MM).contains(v))
}

fn parse_count(text: &str) -> Option<u32> {
    text.trim()
        .parse::<u32>()
        .ok()
        .filter(|n| (1..=MAX_FOLD_NOTCHES as u32).contains(n))
}

/// Projet de capture `--etape ourlet` : trapèze, marge de 10 mm, ourlet de 20 mm sur le bord du
/// bas (le bord est sélectionné) ; `--etape rempli` : quartier de disque, rempli de 12 mm sur
/// l'arc avec un cran tous les 25 mm au plus. Renvoie aussi le tracé à sélectionner.
pub(crate) fn demo_hem_project(
    fold: bool,
) -> Result<(pf_core::Project, EntityId), pf_core::CoreError> {
    use pf_core::{PieceSource, PointRef};
    use pf_geometry::PointMm;
    let mut p = pf_core::Project::new(if fold { "Rempli" } else { "Ourlet" });
    let sheet = p.sheets[0].id;
    let seg = |p: &mut pf_core::Project, a: (f64, f64), b: (f64, f64)| {
        Ok::<_, pf_core::CoreError>(
            p.apply(Command::AddSegment {
                sheet,
                from: PointRef::New(PointMm::new(a.0, a.1)),
                to: PointRef::New(PointMm::new(b.0, b.1)),
            })?
            .entities[0],
        )
    };
    if fold {
        let (r, h) = (100.0, 100.0 * 0.5f64.sqrt());
        let bottom = seg(&mut p, (0.0, 0.0), (r, 0.0))?;
        let left = seg(&mut p, (0.0, r), (0.0, 0.0))?;
        let at = |p: &pf_core::Project, x: f64, y: f64| {
            let s = &p.sheets[0];
            s.points
                .iter()
                .find(|k| {
                    s.position(k.id)
                        .is_ok_and(|q| (q.x - x).abs() < 1e-9 && (q.y - y).abs() < 1e-9)
                })
                .map(|k| k.id)
        };
        let (Some(a), Some(b)) = (at(&p, r, 0.0), at(&p, 0.0, r)) else {
            return Err(pf_core::CoreError::Invalid("quartier de disque".into()));
        };
        let arc = p
            .apply(Command::AddArcThreePoints {
                sheet,
                start: PointRef::Existing(a),
                through: PointRef::New(PointMm::new(h, h)),
                end: PointRef::Existing(b),
            })?
            .entities[0];
        let piece = p
            .apply(Command::ExtractPiece {
                sheet,
                source: PieceSource::Paths {
                    entities: vec![bottom, arc, left],
                },
                name: "Quartier".into(),
                detached: false,
            })?
            .pieces[0];
        p.apply(Command::SetSeamAllowance {
            piece,
            seam: Some(SeamAllowance {
                default_mm: 6.0,
                folds: vec![FoldAllowance {
                    entity: arc,
                    value_mm: 12.0,
                    notch_step_mm: None,
                    notch_count: None,
                }],
                ..Default::default()
            }),
        })?;
        return Ok((p, arc));
    }
    let pts = [(0.0, 0.0), (100.0, 0.0), (80.0, 60.0), (20.0, 60.0)];
    let mut ids = Vec::new();
    for i in 0..pts.len() {
        ids.push(seg(&mut p, pts[i], pts[(i + 1) % pts.len()])?);
    }
    let piece = p
        .apply(Command::ExtractPiece {
            sheet,
            source: PieceSource::Paths {
                entities: ids.clone(),
            },
            name: "Jupe".into(),
            detached: false,
        })?
        .pieces[0];
    p.apply(Command::SetSeamAllowance {
        piece,
        seam: Some(SeamAllowance {
            default_mm: 10.0,
            hems: vec![HemAllowance {
                entity: ids[0],
                height_mm: 20.0,
            }],
            ..Default::default()
        }),
    })?;
    Ok((p, ids[0]))
}

impl App {
    /// Ourlets et rempliés de la pièce : une ligne par réglage (nom du tracé, valeur, pas ou nombre
    /// de crans), les crans calculés comptés, deux boutons d'ajout pour le tracé sélectionné.
    pub(crate) fn hem_fold_block(
        &mut self,
        ui: &mut egui::Ui,
        piece: &pf_core::Piece,
        current: &SeamAllowance,
        next: &mut SeamAllowance,
        name_of: &dyn Fn(EntityId) -> String,
        key: u64,
    ) {
        // Lectures seules : tracés droits du contour et nombre de crans calculés par rempli.
        let (straight, counts): (Vec<EntityId>, Vec<(EntityId, usize)>) = {
            let project = self.editor.project();
            let straight = project
                .piece_sheet(piece.id)
                .map(|s| {
                    piece
                        .outline
                        .iter()
                        .filter(|o| {
                            s.entity(o.entity)
                                .is_ok_and(|e| matches!(e.kind, EntityKind::Segment { .. }))
                        })
                        .map(|o| o.entity)
                        .collect()
                })
                .unwrap_or_default();
            let mut counts = Vec::new();
            for n in project.piece_fold_notches(piece.id).unwrap_or_default() {
                match counts
                    .iter_mut()
                    .find(|c: &&mut (EntityId, usize)| c.0 == n.entity)
                {
                    Some(c) => c.1 += 1,
                    None => counts.push((n.entity, 1)),
                }
            }
            (straight, counts)
        };
        let muted = |t: String| RichText::new(t).small().color(pal().muted);

        // --- ourlets ---
        crate::a11y::label(
            ui,
            RichText::new(tr_args(
                "piece.hem.title",
                &[("count", &current.hems.len().to_string())],
            ))
            .small()
            .strong()
            .color(pal().muted),
        );
        if current.hems.is_empty() {
            crate::a11y::label(ui, muted(tr("piece.hem.none")));
        }
        let mut remove = None;
        for (i, h) in current.hems.iter().enumerate() {
            ui.horizontal_wrapped(|ui| {
                crate::a11y::label(ui, RichText::new(name_of(h.entity)).small());
                crate::a11y::label(ui, RichText::new(tr("piece.hem.height")).small())
                    .on_hover_text(tr("piece.hem.height.hover"));
                let k = key | (3 << 40) | ((i as u64 + 1) << 20);
                let shown = field_text(h.height_mm);
                if let Some(text) = buffered_edit(ui, &mut self.text_buf, k, &shown, 48.0) {
                    match parse_value(&text) {
                        Some(v) => next.hems[i].height_mm = v,
                        None => self.editor.error(tr("piece.hem.invalid")),
                    }
                }
                if ui.small_button(tr("piece.hem.remove")).clicked() {
                    remove = Some(i);
                }
            });
        }
        if let Some(i) = remove {
            next.hems.remove(i);
        }

        // --- rempliés ---
        crate::a11y::label(
            ui,
            RichText::new(tr_args(
                "piece.fold.title",
                &[("count", &current.folds.len().to_string())],
            ))
            .small()
            .strong()
            .color(pal().muted),
        );
        if current.folds.is_empty() {
            crate::a11y::label(ui, muted(tr("piece.fold.none")));
        }
        let mut remove = None;
        for (i, f) in current.folds.iter().enumerate() {
            ui.horizontal_wrapped(|ui| {
                crate::a11y::label(ui, RichText::new(name_of(f.entity)).small());
                crate::a11y::label(ui, RichText::new(tr("piece.fold.value")).small())
                    .on_hover_text(tr("piece.fold.value.hover"));
                let k = key | (4 << 40) | ((i as u64 + 1) << 20);
                let shown = field_text(f.value_mm);
                if let Some(text) = buffered_edit(ui, &mut self.text_buf, k, &shown, 48.0) {
                    match parse_value(&text) {
                        Some(v) => next.folds[i].value_mm = v,
                        None => self.editor.error(tr("piece.fold.invalid")),
                    }
                }
                if ui.small_button(tr("piece.fold.remove")).clicked() {
                    remove = Some(i);
                }
            });
            if straight.contains(&f.entity) {
                crate::a11y::label(ui, muted(tr("piece.fold.straight")));
                continue;
            }
            ui.horizontal_wrapped(|ui| {
                crate::a11y::label(ui, RichText::new(tr("piece.fold.step")).small())
                    .on_hover_text(tr("piece.fold.step.hover"));
                let k = key | (5 << 40) | ((i as u64 + 1) << 20);
                let shown = f.notch_step_mm.map(field_text).unwrap_or_default();
                if let Some(text) = buffered_edit(ui, &mut self.text_buf, k, &shown, 48.0) {
                    if text.trim().is_empty() {
                        next.folds[i].notch_step_mm = None;
                    } else {
                        match parse_step(&text) {
                            Some(v) => {
                                next.folds[i].notch_step_mm = Some(v);
                                next.folds[i].notch_count = None;
                            }
                            None => self.editor.error(tr("piece.fold.step.invalid")),
                        }
                    }
                }
                crate::a11y::label(ui, RichText::new(tr("piece.fold.count")).small())
                    .on_hover_text(tr("piece.fold.count.hover"));
                let k = key | (6 << 40) | ((i as u64 + 1) << 20);
                let shown = f.notch_count.map(|v| v.to_string()).unwrap_or_default();
                if let Some(text) = buffered_edit(ui, &mut self.text_buf, k, &shown, 40.0) {
                    if text.trim().is_empty() {
                        next.folds[i].notch_count = None;
                    } else {
                        match parse_count(&text) {
                            Some(v) => {
                                next.folds[i].notch_count = Some(v);
                                next.folds[i].notch_step_mm = None;
                            }
                            None => self.editor.error(tr("piece.fold.count.invalid")),
                        }
                    }
                }
            });
            let n = counts.iter().find(|c| c.0 == f.entity).map_or(0, |c| c.1);
            crate::a11y::label(
                ui,
                muted(tr_args("piece.fold.notches", &[("count", &n.to_string())])),
            );
        }
        if let Some(i) = remove {
            next.folds.remove(i);
        }

        // --- ajout pour le tracé sélectionné ---
        let candidate = match self.editor.selection.entities.as_slice() {
            [e] if piece.outline.iter().any(|o| o.entity == *e)
                && !current.edges.iter().any(|x| x.entity == *e)
                && !current.hems.iter().any(|x| x.entity == *e)
                && !current.folds.iter().any(|x| x.entity == *e) =>
            {
                Some(*e)
            }
            _ => None,
        };
        ui.horizontal_wrapped(|ui| {
            let hem_ok = candidate.is_some_and(|e| straight.contains(&e));
            if ui
                .add_enabled(hem_ok, egui::Button::new(tr("piece.hem.add")))
                .on_hover_text(tr("piece.hem.add.hover"))
                .clicked()
            {
                if let Some(entity) = candidate {
                    next.hems.push(HemAllowance {
                        entity,
                        height_mm: NEW_HEM_MM,
                    });
                }
            }
            if ui
                .add_enabled(candidate.is_some(), egui::Button::new(tr("piece.fold.add")))
                .on_hover_text(tr("piece.fold.add.hover"))
                .clicked()
            {
                if let Some(entity) = candidate {
                    next.folds.push(FoldAllowance {
                        entity,
                        value_mm: NEW_FOLD_MM,
                        notch_step_mm: None,
                        notch_count: None,
                    });
                }
            }
        });
    }
}
