//! Forme d'un cran, position « sur la coupe » (US-06.1) et cran en coin (US-06.1b) dans
//! l'inspecteur du cran. Rien n'est calculé ici : la forme, la projection et le sommet viennent de
//! `pf_core::notch` ; chaque changement est une commande (`SetNotchShape` / `SetNotchCorner`, une
//! étape annulable, refus sans trace).

use crate::app::App;
use crate::palette::pal;
use eframe::egui::{self, RichText};
use pf_core::input::format_mm;
use pf_core::notch::{MAX_NOTCH_WIDTH_MM, MIN_NOTCH_WIDTH_MM};
use pf_core::{Command, NotchForm, NotchId};
use pf_i18n::{tr, tr_args};

/// Libellé traduit d'une forme (clés littérales : le test des clés utilisées ne voit pas les
/// clés calculées).
pub(crate) fn notch_form_label(form: NotchForm) -> String {
    match form {
        NotchForm::Line => tr("inspector.notch.form.line"),
        NotchForm::T => tr("inspector.notch.form.t"),
        NotchForm::V => tr("inspector.notch.form.v"),
        NotchForm::U => tr("inspector.notch.form.u"),
        NotchForm::Slot => tr("inspector.notch.form.slot"),
    }
}

impl App {
    /// Forme, largeur et « sur la coupe » du cran `id`, sous ses réglages de style.
    pub(crate) fn notch_shape_block(&mut self, ui: &mut egui::Ui, id: NotchId) {
        use pf_ui::components::{NumericField, Unit};
        let sheet_id = self.editor.sheet;
        let project = self.editor.project();
        let Ok(notch) = project.sheet(sheet_id).and_then(|s| s.notch(id)).cloned() else {
            return;
        };
        let in_piece = project.notch_piece(sheet_id, &notch).is_some();
        let projection = notch.on_cut.then(|| {
            project
                .notch_on_cut(sheet_id, id)
                .map_err(|e| e.to_string())
        });

        // Sommet lu d'après le contour (même pour un cran pas encore en coin : sert à savoir si
        // la case peut être cochée).
        let vertex = project
            .notch_corner(sheet_id, id)
            .map_err(|e| e.to_string());
        let (mut form, mut width, mut on_cut) =
            (notch.form, notch.effective_width_mm(), notch.on_cut);
        let mut corner = notch.corner;
        let mut changed = false;
        let mut width_edited = false;
        ui.horizontal_wrapped(|ui| {
            crate::a11y::label(ui, tr("inspector.notch.form"));
            for f in NotchForm::ALL {
                changed |= ui
                    .selectable_value(&mut form, f, notch_form_label(f))
                    .changed();
            }
        });
        if !form.is_line() {
            egui::Grid::new("notch-width-grid")
                .num_columns(2)
                .show(ui, |ui| {
                    crate::a11y::label(ui, tr("inspector.notch.width"));
                    let committed = NumericField::new(&mut width, Unit::Mm)
                        .range(MIN_NOTCH_WIDTH_MM..=MAX_NOTCH_WIDTH_MM)
                        .decimals(2)
                        .id_salt(("notch-width", id.0))
                        .show(ui)
                        .committed;
                    changed |= committed;
                    width_edited |= committed;
                    ui.end_row();
                });
        }
        changed |= ui
            .add_enabled(
                (in_piece && !notch.corner) || notch.on_cut,
                egui::Checkbox::new(&mut on_cut, tr("inspector.notch.on_cut")),
            )
            .changed();
        match projection {
            Some(Ok(c)) => {
                crate::panels::row(
                    ui,
                    &tr("inspector.notch.on_cut.reach"),
                    &format_mm(c.reach_mm),
                );
            }
            Some(Err(reason)) => {
                crate::a11y::label(
                    ui,
                    RichText::new(tr_args(
                        "inspector.notch.on_cut.failed",
                        &[("reason", &reason)],
                    ))
                    .small()
                    .color(pal().error),
                );
            }
            None => {}
        }
        if notch.on_cut {
            crate::a11y::label(
                ui,
                RichText::new(tr("inspector.notch.on_cut.note"))
                    .small()
                    .color(pal().muted),
            );
        }
        // US-09.5c : cran proportionnel (un cran ancré suit déjà son bord : case grisée).
        let mut proportional = notch.proportional;
        let proportional_ok = notch.anchor.is_none() || notch.proportional;
        let proportional_changed = ui
            .add_enabled(
                proportional_ok,
                egui::Checkbox::new(&mut proportional, tr("inspector.notch.proportional")),
            )
            .changed();
        if !proportional_ok {
            crate::a11y::label(
                ui,
                RichText::new(tr("inspector.notch.proportional.anchored"))
                    .small()
                    .color(pal().muted),
            );
        } else if notch.proportional {
            crate::a11y::label(
                ui,
                RichText::new(tr("inspector.notch.proportional.note"))
                    .small()
                    .color(pal().muted),
            );
        }
        if proportional_changed {
            self.editor.exec(Command::SetNotchProportional {
                sheet: sheet_id,
                notch: id,
                proportional,
            });
        }
        let corner_ok = notch.corner || (!notch.on_cut && vertex.is_ok());
        let corner_changed = ui
            .add_enabled(
                corner_ok,
                egui::Checkbox::new(&mut corner, tr("inspector.notch.corner")),
            )
            .changed();
        if !corner_ok {
            let reason = match &vertex {
                _ if notch.on_cut => tr("inspector.notch.corner.exclusive"),
                Err(reason) => reason.clone(),
                Ok(_) => String::new(),
            };
            crate::a11y::label(
                ui,
                RichText::new(tr_args(
                    "inspector.notch.corner.unavailable",
                    &[("reason", &reason)],
                ))
                .small()
                .color(pal().muted),
            );
        }
        if notch.corner {
            match &vertex {
                Ok(c) => crate::panels::row(
                    ui,
                    &tr("inspector.notch.corner.angle"),
                    &pf_core::input::format_deg(c.interior_angle_deg),
                ),
                Err(reason) => {
                    crate::a11y::label(
                        ui,
                        RichText::new(tr_args(
                            "inspector.notch.corner.failed",
                            &[("reason", reason)],
                        ))
                        .small()
                        .color(pal().error),
                    );
                }
            }
            crate::a11y::label(
                ui,
                RichText::new(tr("inspector.notch.corner.note"))
                    .small()
                    .color(pal().muted),
            );
        }
        if corner_changed {
            self.editor.exec(Command::SetNotchCorner {
                sheet: sheet_id,
                notch: id,
                corner,
            });
        }
        if changed {
            let width_mm = if form.is_line() {
                None
            } else if width_edited || notch.width_mm.is_some() {
                Some(width)
            } else {
                None
            };
            self.editor.exec(Command::SetNotchShape {
                sheet: sheet_id,
                notch: id,
                form,
                width_mm,
                on_cut,
            });
        }
    }
}

/// Projet de capture (`--etape cran-coupe`) : trapèze avec 12 mm de marge et quatre crans : U,
/// V (sélectionné, sur le bord incliné) et T sur la coupe, fente sur la couture.
pub(crate) fn demo_notch_project() -> Result<(pf_core::Project, NotchId), pf_core::CoreError> {
    use pf_core::{NotchStyle, PieceSource, PointRef, SeamAllowance};
    use pf_geometry::{PointMm, Side};
    let mut p = pf_core::Project::new("Notch demo");
    let sheet = p.sheets[0].id;
    let pts = [(0.0, 0.0), (120.0, 0.0), (100.0, 70.0), (20.0, 70.0)];
    let mut edges = Vec::new();
    for i in 0..pts.len() {
        let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
        edges.push(
            p.apply(Command::AddSegment {
                sheet,
                from: PointRef::New(PointMm::new(a.0, a.1)),
                to: PointRef::New(PointMm::new(b.0, b.1)),
            })?
            .entities[0],
        );
    }
    let piece = p
        .apply(Command::ExtractPiece {
            sheet,
            source: PieceSource::Paths {
                entities: edges.clone(),
            },
            name: "Piece 1".into(),
            detached: false,
        })?
        .pieces[0];
    p.apply(Command::SetSeamAllowance {
        piece,
        seam: Some(SeamAllowance {
            default_mm: 12.0,
            ..Default::default()
        }),
    })?;
    let mut put = |edge: usize, at: f64, form: NotchForm, width: f64, on_cut: bool| {
        let id = p
            .apply(Command::AddNotch {
                sheet,
                entity: edges[edge],
                distance_mm: at,
                side: Side::Right,
                length_mm: 8.0,
                style: NotchStyle::Single,
            })?
            .notches[0];
        p.apply(Command::SetNotchShape {
            sheet,
            notch: id,
            form,
            width_mm: Some(width),
            on_cut,
        })?;
        Ok::<NotchId, pf_core::CoreError>(id)
    };
    put(0, 40.0, NotchForm::U, 6.0, true)?;
    let selected = put(1, 36.0, NotchForm::V, 6.0, true)?;
    put(3, 30.0, NotchForm::T, 6.0, true)?;
    put(2, 40.0, NotchForm::Slot, 4.0, false)?;
    Ok((p, selected))
}

/// Projet de capture (`--etape cran-coin`) : pièce en L (sommets saillants et un sommet
/// rentrant) avec quatre crans en coin ; celui du sommet rentrant (60 ; 40) est sélectionné.
pub(crate) fn demo_corner_project() -> Result<(pf_core::Project, NotchId), pf_core::CoreError> {
    use pf_core::{NotchStyle, PieceSource, PointRef};
    use pf_geometry::{PointMm, Side};
    let mut p = pf_core::Project::new("Corner notch demo");
    let sheet = p.sheets[0].id;
    let pts = [
        (0.0, 0.0),
        (120.0, 0.0),
        (120.0, 40.0),
        (60.0, 40.0),
        (60.0, 80.0),
        (0.0, 80.0),
    ];
    let mut edges = Vec::new();
    for i in 0..pts.len() {
        let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
        edges.push(
            p.apply(Command::AddSegment {
                sheet,
                from: PointRef::New(PointMm::new(a.0, a.1)),
                to: PointRef::New(PointMm::new(b.0, b.1)),
            })?
            .entities[0],
        );
    }
    p.apply(Command::ExtractPiece {
        sheet,
        source: PieceSource::Paths {
            entities: edges.clone(),
        },
        name: "Piece L".into(),
        detached: false,
    })?;
    let mut put = |edge: usize, at_end: bool, form: NotchForm| {
        let at = if at_end {
            p.sheet(sheet)?.path_length_mm(edges[edge])?
        } else {
            0.0
        };
        let id = p
            .apply(Command::AddNotch {
                sheet,
                entity: edges[edge],
                distance_mm: at,
                side: Side::Right,
                length_mm: 8.0,
                style: NotchStyle::Single,
            })?
            .notches[0];
        if !form.is_line() {
            p.apply(Command::SetNotchShape {
                sheet,
                notch: id,
                form,
                width_mm: Some(6.0),
                on_cut: false,
            })?;
        }
        p.apply(Command::SetNotchCorner {
            sheet,
            notch: id,
            corner: true,
        })?;
        Ok::<NotchId, pf_core::CoreError>(id)
    };
    put(0, false, NotchForm::Line)?;
    put(1, true, NotchForm::V)?;
    let selected = put(3, false, NotchForm::T)?;
    put(5, false, NotchForm::Line)?;
    Ok((p, selected))
}
