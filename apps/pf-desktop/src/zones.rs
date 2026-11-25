//! Bloc « Zones… » de la section de marquages d'une pièce (US-06.10, maroquinerie) : zones de
//! parage, de collage et de découpe interne. Il ne calcule rien : surface, aire et refus viennent
//! de `Project::zone_preview` / `zone_layout` (lecture seule) et des commandes `AddPieceZone` /
//! `RemovePieceZone` (une étape annulable, refus sans trace). Une bande se prend sur les tracés
//! sélectionnés du contour et reste **liée** à son bord ; un polygone libre prend les points
//! sélectionnés, dans l'ordre de sélection.
//!
//! Replié par défaut. Aucune liste déroulante (les tests de l'inspecteur de pièce comptent les
//! `ComboBox`) : les choix sont des boutons à bascule. Sur le plan, le type se lit au **motif**
//! (hachures, points, croisillons), au tracé du contour (plein, tirets, tiret-point) et à
//! l'étiquette, jamais à la couleur seule.

use crate::app::App;
use crate::palette::{pal, zone_color};
use crate::text::core_error;
use eframe::egui::{self, RichText};
use pf_core::input::{format_mm, format_number};
use pf_core::units::parse_length_display;
use pf_core::{Command, PieceId, ZoneKind, ZoneShape, ZoneSpec};
use pf_geometry::PointMm;
use pf_i18n::{tr, tr_args};

/// Forme choisie dans le formulaire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZoneMode {
    Band,
    Polygon,
}

/// Aperçu affiché et dessiné sur le plan : surface telle que la commande la créera.
#[derive(Debug, Clone, PartialEq)]
pub struct ZonePreview {
    pub kind: ZoneKind,
    pub polygon: Vec<PointMm>,
    pub area_mm2: f64,
}

/// Formulaire « Zones… » : état d'interface, jamais enregistré.
pub struct ZoneForm {
    pub piece: PieceId,
    pub kind: ZoneKind,
    pub mode: ZoneMode,
    pub width: String,
    pub preview: Option<ZonePreview>,
}

impl ZoneForm {
    pub fn new(piece: PieceId) -> ZoneForm {
        ZoneForm {
            piece,
            kind: ZoneKind::Skive,
            mode: ZoneMode::Band,
            width: "5".into(),
            preview: None,
        }
    }
}

/// Libellé d'un type de zone (jamais la couleur seule).
pub fn kind_label(kind: ZoneKind) -> String {
    tr(match kind {
        ZoneKind::Skive => "zone.kind.skive",
        ZoneKind::Glue => "zone.kind.glue",
        ZoneKind::InnerCut => "zone.kind.inner_cut",
    })
}

impl App {
    /// Bloc replié « Zones… » de la section de marquages d'une pièce.
    pub(crate) fn piece_zone_block(&mut self, ui: &mut egui::Ui, piece: &pf_core::Piece) {
        let mut header = egui::CollapsingHeader::new(
            RichText::new(tr("zone.title"))
                .small()
                .strong()
                .color(pal().muted),
        )
        .id_salt(("zones", piece.id.0))
        .default_open(false);
        // Étape de capture : ouvert une fois, et amené dans la partie visible du panneau.
        let once = std::mem::take(&mut self.zone_open_once);
        if once {
            header = header.open(Some(true));
        }
        let shown = header.show(ui, |ui| self.zone_body(ui, piece));
        crate::panels::scroll_block_into_view(
            ui,
            ("zones", piece.id.0),
            shown.header_response.rect,
            once,
        );
    }

    fn zone_body(&mut self, ui: &mut egui::Ui, piece: &pf_core::Piece) {
        let id = piece.id;
        let mut form = match self.zone_form.take() {
            Some(f) if f.piece == id => f,
            _ => ZoneForm::new(id),
        };
        crate::a11y::label(
            ui,
            RichText::new(tr("zone.hint")).small().color(pal().muted),
        );
        // Zones existantes : résumé et suppression.
        let mut commands: Vec<Command> = Vec::new();
        for z in &piece.zones {
            let project = self.editor.view_project();
            let summary = match project.zone_layout(id, z.id) {
                Ok(l) => match l.width_mm {
                    Some(w) => tr_args(
                        "zone.row.band",
                        &[
                            ("id", &z.id.0.to_string()),
                            ("kind", &kind_label(z.kind)),
                            ("width", &format_mm(w)),
                            ("area", &format_number(l.area_mm2, 2)),
                        ],
                    ),
                    None => tr_args(
                        "zone.row.polygon",
                        &[
                            ("id", &z.id.0.to_string()),
                            ("kind", &kind_label(z.kind)),
                            ("count", &l.polygon.len().to_string()),
                            ("area", &format_number(l.area_mm2, 2)),
                        ],
                    ),
                },
                Err(e) => core_error(&e),
            };
            crate::a11y::label(ui, RichText::new(summary).small());
            if ui.small_button(tr("zone.remove")).clicked() {
                commands.push(Command::RemovePieceZone {
                    piece: id,
                    zone: z.id,
                });
            }
        }
        if piece.zones.is_empty() {
            crate::a11y::label(
                ui,
                RichText::new(tr("zone.none")).small().color(pal().muted),
            );
        }
        for c in commands {
            if self.editor.exec(c).is_some() {
                form.preview = None;
            }
        }
        ui.horizontal_wrapped(|ui| {
            for kind in ZoneKind::ALL {
                ui.selectable_value(&mut form.kind, kind, kind_label(kind));
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(&mut form.mode, ZoneMode::Band, tr("zone.mode.band"));
            ui.selectable_value(&mut form.mode, ZoneMode::Polygon, tr("zone.mode.polygon"));
        });
        // Forme choisie : bord (tracés du contour sélectionnés) ou sommets (points sélectionnés).
        let sheet = self.editor.sheet().id;
        let shape: Result<ZoneShape, String> = match form.mode {
            ZoneMode::Band => {
                ui.horizontal(|ui| {
                    crate::a11y::label(ui, tr("zone.width"));
                    crate::a11y::add_text(
                        ui,
                        egui::TextEdit::singleline(&mut form.width).desired_width(70.0),
                    );
                });
                let edges = self
                    .editor
                    .project()
                    .marking_run_edges(id, &self.editor.selection.entities);
                match edges {
                    Ok(e) => {
                        crate::a11y::label(
                            ui,
                            tr_args("zone.edge", &[("count", &e.len().to_string())]),
                        );
                        parse_length_display(&form.width)
                            .map_err(|_| tr("zone.invalid"))
                            .map(|w| ZoneShape::Band {
                                edges: e,
                                width_mm: w,
                            })
                    }
                    Err(e) => {
                        let text = core_error(&e);
                        crate::a11y::label(
                            ui,
                            RichText::new(text.clone())
                                .small()
                                .color(ui.visuals().warn_fg_color),
                        );
                        Err(text)
                    }
                }
            }
            ZoneMode::Polygon => {
                let pts: Vec<PointMm> = self
                    .editor
                    .selection
                    .points
                    .iter()
                    .filter_map(|p| self.editor.project().sheet(sheet).ok()?.position(*p).ok())
                    .collect();
                crate::a11y::label(
                    ui,
                    tr_args("zone.points", &[("count", &pts.len().to_string())]),
                );
                if pts.len() >= 3 {
                    Ok(ZoneShape::Polygon { points: pts })
                } else {
                    Err(tr("zone.points.need"))
                }
            }
        };
        let (mut preview, mut create) = (false, false);
        ui.horizontal_wrapped(|ui| {
            preview = ui
                .add_enabled(shape.is_ok(), egui::Button::new(tr("zone.preview")))
                .clicked();
            create = ui
                .add_enabled(shape.is_ok(), egui::Button::new(tr("zone.create")))
                .clicked();
        });
        if let (true, Ok(shape)) = (preview || create, shape) {
            let spec = ZoneSpec {
                kind: form.kind,
                shape,
            };
            if preview {
                match self.editor.project().zone_preview(id, &spec) {
                    Ok(z) => {
                        form.preview = Some(ZonePreview {
                            kind: z.kind,
                            polygon: z.polygon,
                            area_mm2: z.area_mm2,
                        });
                    }
                    Err(e) => {
                        form.preview = None;
                        self.editor.error(core_error(&e));
                    }
                }
            } else if let Some(o) = self.editor.exec(Command::AddPieceZone {
                piece: id,
                zone: spec,
            }) {
                form.preview = None;
                self.editor
                    .info(tr_args("zone.added", &[("id", &o.zones[0].0.to_string())]));
            }
        }
        if let Some(p) = &form.preview {
            crate::a11y::label(
                ui,
                RichText::new(tr_args(
                    "zone.result",
                    &[
                        ("kind", &kind_label(p.kind)),
                        ("area", &format_number(p.area_mm2, 2)),
                    ],
                ))
                .small(),
            );
        }
        crate::a11y::label(
            ui,
            RichText::new(tr("zone.note")).small().color(pal().muted),
        );
        self.zone_form = Some(form);
    }

    /// Zones des pièces de la feuille et aperçu du formulaire, sur le plan : le **motif**
    /// (hachures, points, croisillons), le tracé du contour (plein, tirets, tiret-point) et
    /// l'étiquette distinguent les types, jamais la couleur seule. Non sélectionnable.
    pub(crate) fn paint_zones(&self, painter: &egui::Painter, xf: &crate::canvas::Xf) {
        let project = self.editor.view_project();
        for z in project.sheet_zones(self.editor.sheet) {
            let scene = pf_export::SceneZone {
                kind: z.kind,
                polygon: z.polygon,
                width_mm: z.width_mm,
            };
            let color = zone_color(scene.kind);
            let pts: Vec<egui::Pos2> = scene.polygon.iter().map(|p| xf.s(*p)).collect();
            let mut ring = pts.clone();
            if let Some(first) = pts.first() {
                ring.push(*first);
            }
            let stroke = egui::Stroke::new(1.5_f32, color);
            match scene.kind {
                ZoneKind::Skive => {
                    painter.add(egui::Shape::line(ring, stroke));
                }
                ZoneKind::Glue => {
                    painter.extend(egui::Shape::dashed_line(&ring, stroke, 8.0, 4.0));
                }
                ZoneKind::InnerCut => {
                    painter.extend(egui::Shape::dashed_line(&ring, stroke, 14.0, 3.0));
                }
            }
            let pat = scene.pattern();
            let thin = egui::Stroke::new(1.0_f32, color);
            for (a, b) in &pat.hatch {
                painter.line_segment([xf.s(*a), xf.s(*b)], thin);
            }
            for d in &pat.dots {
                painter.circle_filled(xf.s(*d), 1.3, color);
            }
            if let Some(at) = pat.label_at {
                let text = match scene.width_mm {
                    Some(w) => format!("{} {}", kind_label(scene.kind), format_mm(w)),
                    None => kind_label(scene.kind),
                };
                crate::canvas::label(painter, xf.s(at), egui::Align2::CENTER_CENTER, &text, color);
            }
        }
        if let Some(p) = self.zone_form.as_ref().and_then(|f| f.preview.as_ref()) {
            let mut ring: Vec<egui::Pos2> = p.polygon.iter().map(|q| xf.s(*q)).collect();
            if let Some(first) = ring.first().copied() {
                ring.push(first);
            }
            painter.add(egui::Shape::line(
                ring,
                egui::Stroke::new(2.0_f32, pal().select),
            ));
        }
    }
}

/// Étape de capture : une bande de chaque nature sur la pièce, un bord sélectionné, bloc ouvert.
pub(crate) fn demo_zones(app: &mut App) {
    if let Some(edge) = app
        .editor
        .project()
        .pieces
        .first()
        .and_then(|p| p.outline.first())
        .map(|e| e.entity)
    {
        app.editor.selection.entities = vec![edge];
    }
    app.zone_open_once = true;
}

/// Projet de démonstration (étape de capture) : fond de porte-cartes 100 × 70 mm à congés de
/// 5 mm, une bande de parage (4 mm) sur le bas, une bande de collage (6 mm) sur le haut et une
/// découpe interne polygonale au centre.
pub(crate) fn demo_zone_project() -> Result<pf_core::Project, pf_core::CoreError> {
    let mut p = crate::marking_run::demo_pitch_project()?;
    let piece = p.pieces[0].id;
    let ids: Vec<_> = p.pieces[0].markings.iter().map(|m| m.id).collect();
    for marking in ids {
        p.apply(Command::RemovePieceMarking { piece, marking })?;
    }
    let sheet = p.sheet(match p.pieces[0].link {
        pf_core::PieceLink::Linked { sheet } => sheet,
        pf_core::PieceLink::Detached => p.sheets[0].id,
    })?;
    let straight = |a: PointMm, b: PointMm| {
        sheet
            .entities
            .iter()
            .find(|e| {
                sheet.segment_ends(e.id).is_ok_and(|(x, y)| {
                    let near = |u: PointMm, v: PointMm| {
                        (u.x - v.x).abs() < 1e-9 && (u.y - v.y).abs() < 1e-9
                    };
                    (near(x, a) && near(y, b)) || (near(x, b) && near(y, a))
                })
            })
            .map(|e| e.id)
            .ok_or_else(|| pf_core::CoreError::Invalid("demo edge not found".into()))
    };
    let bottom = straight(PointMm::new(5.0, 0.0), PointMm::new(95.0, 0.0))?;
    let top = straight(PointMm::new(95.0, 70.0), PointMm::new(5.0, 70.0))?;
    for (kind, edge, width) in [(ZoneKind::Skive, bottom, 4.0), (ZoneKind::Glue, top, 6.0)] {
        p.apply(Command::AddPieceZone {
            piece,
            zone: ZoneSpec {
                kind,
                shape: ZoneShape::Band {
                    edges: vec![edge],
                    width_mm: width,
                },
            },
        })?;
    }
    p.apply(Command::AddPieceZone {
        piece,
        zone: ZoneSpec {
            kind: ZoneKind::InnerCut,
            shape: ZoneShape::Polygon {
                points: [(35.0, 25.0), (65.0, 25.0), (65.0, 45.0), (35.0, 45.0)]
                    .map(|(x, y)| PointMm::new(x, y))
                    .to_vec(),
            },
        },
    })?;
    Ok(p)
}
