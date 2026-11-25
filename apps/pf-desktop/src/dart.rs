//! Outil « Pince » (US-07.1) : sommet, bord de la base, ouverture (et longueur facultative).
//!
//! Aucun calcul géométrique ici : le point d'axe (pied de la perpendiculaire au bord) vient de
//! `Project::dart_base_point`, la pince, sa fermeture et son capuchon de `Project::piece_darts`
//! (`pf_core::piece_dart`), la pose est la commande `Command::AddPieceDart` (une étape annulable,
//! refus sans trace). L'aperçu rejoue la commande sur un clone du projet.
//!
//! Flux : clic 1 = sommet (dans une pièce) ; clic 2 = bord droit qui porte la base ; l'ouverture se
//! saisit (ou suit la souris) ; une longueur saisie déplace le sommet le long de l'axe ; Entrée ou
//! clic pose la pince. Clic sur le sommet ou une jambe d'une pince existante = aperçu fermé et
//! mesures (fenêtre « Pince »). La pince se dessine en **trait mixte** avec le libellé « Pince » :
//! jamais la couleur seule. Elle n'est pas exportée (limite documentée).

use crate::app::App;
use crate::canvas::{label, polyline, stroke, Xf};
use crate::editor::{Editor, Tool, ToolState};
use crate::palette::pal;
use eframe::egui::{self, Align2, Pos2, RichText};
use pf_core::input::{format_mm, format_number, parse_length_mm};
use pf_core::{Command, DartCalc, DartId, PieceId, PointRef};
use pf_geometry::split::edge_sample;
use pf_geometry::{distance_mm, PointMm, Side};
use pf_i18n::{tr, tr_args};

/// Paramètres lus de l'état de l'outil (saisie, souris, bascule de côté).
struct DartInput {
    apex_ref: PointRef,
    base_pos: PointMm,
    apex_pos: PointMm,
    length_mm: f64,
    opening_mm: f64,
    fixed: Side,
}

/// Transfert de la pince sélectionnée (US-07.2) : état de l'interface (aucune donnée du projet).
#[derive(Debug, Clone, PartialEq)]
pub struct DartTransferUi {
    /// En attente du clic sur le bord qui reçoit la pince.
    pub armed: bool,
    /// Transfert partiel : la part saisie s'applique ; sinon transfert total.
    pub partial: bool,
    /// Part de l'ouverture transférée, en pourcentage (mode partiel).
    pub percent: String,
}

impl Default for DartTransferUi {
    fn default() -> Self {
        DartTransferUi {
            armed: false,
            partial: false,
            percent: "50".into(),
        }
    }
}

/// Pince en cours de pose : paramètres et résultat du calcul (ou la raison du refus).
pub struct DartDraft {
    pub apex: PointMm,
    pub base: PointMm,
    pub length_mm: f64,
    pub opening_mm: f64,
    pub result: Result<DartCalc, String>,
}

impl Editor {
    /// Clic de l'outil Pince.
    pub fn dart_click(&mut self, world: PointMm, tol_mm: f64) {
        if self.dart_transfer.armed {
            self.transfer_dart_at(world);
            return;
        }
        match self.state {
            ToolState::DartBase { .. } => self.confirm(),
            ToolState::DartApex { piece, apex } => {
                let Some(apex_pos) = self.dart_ref_pos(apex) else {
                    return;
                };
                match self.project().dart_base_point(piece, apex_pos, world) {
                    Some(base) => {
                        self.state = ToolState::DartBase {
                            piece,
                            apex,
                            base: PointRef::New(base),
                        };
                        self.length_text.clear();
                        self.angle_text.clear();
                        self.info(tr("status.dart.opening"));
                    }
                    None => self.error(tr("status.dart.base_edge")),
                }
            }
            _ => {
                if self.dart_select_at(world, tol_mm) {
                    return;
                }
                self.dart_selected = None;
                let apex = self.snap_at(world, tol_mm).target;
                let Some(pos) = self.dart_ref_pos(apex) else {
                    return;
                };
                match self.project().piece_containing(self.sheet, pos) {
                    Some(piece) => {
                        self.state = ToolState::DartApex { piece, apex };
                        self.info(tr("status.dart.base"));
                    }
                    None => self.error(tr("status.dart.apex_outside")),
                }
            }
        }
    }

    fn dart_ref_pos(&self, r: PointRef) -> Option<PointMm> {
        match r {
            PointRef::Existing(id) => self.sheet().position(id).ok(),
            PointRef::New(p) => Some(p),
            PointRef::Derived(d) => self.sheet().evaluate(&d).ok(),
        }
    }

    /// Sélectionne la pince dont le sommet ou une jambe est sous `world` ; vrai si trouvée.
    fn dart_select_at(&mut self, world: PointMm, tol_mm: f64) -> bool {
        let mut found = None;
        for piece in self.project().pieces_on(self.sheet) {
            for r in self.project().piece_darts(piece.id) {
                let apex = self.sheet().position(r.dart.apex).ok();
                let near_apex = apex.is_some_and(|a| distance_mm(a, world) <= tol_mm);
                let near_leg = r.calc.as_ref().ok().is_some_and(|c| {
                    [c.shape.left_foot, c.shape.right_foot].iter().any(|&f| {
                        pf_geometry::project_on_segment(world, c.shape.apex, f)
                            .is_ok_and(|(_, d)| d <= tol_mm)
                    })
                });
                if near_apex || near_leg {
                    found = Some((piece.id, r.dart.id));
                }
            }
        }
        let Some((piece, dart)) = found else {
            return false;
        };
        self.dart_selected = Some((piece, dart));
        if let Some(text) = self.dart_summary(piece, dart) {
            self.info(text);
        }
        true
    }

    /// Une ligne de mesures observées de la pince (fermeture), ou la raison si elle n'est pas
    /// calculable.
    pub fn dart_summary(&self, piece: PieceId, dart: DartId) -> Option<String> {
        let r = self
            .project()
            .piece_darts(piece)
            .into_iter()
            .find(|r| r.dart.id == dart)?;
        Some(match r.calc {
            Ok(c) => tr_args(
                "status.dart.selected",
                &[
                    ("before", &format_mm(c.closed.before_mm)),
                    ("after", &format_mm(c.closed.after_mm)),
                    ("removed", &format_mm(c.closed.removed_mm)),
                ],
            ),
            Err(why) => tr_args("dart.panel.invalid", &[("reason", &why)]),
        })
    }

    fn dart_input(&self, apex: PointRef, base: PointRef) -> Result<DartInput, String> {
        let (Some(apex0), Some(base_pos)) = (self.dart_ref_pos(apex), self.dart_ref_pos(base))
        else {
            return Err(tr("status.dart.base_edge"));
        };
        let dist = distance_mm(apex0, base_pos);
        let typed_len = self.typed_height()?;
        let length_mm = typed_len.unwrap_or(dist);
        // Une longueur saisie déplace le sommet le long de l'axe (la base reste sur le bord).
        let (apex_pos, apex_ref) = if typed_len.is_some() && dist > 0.0 {
            let k = length_mm / dist;
            let p = PointMm::new(
                base_pos.x + (apex0.x - base_pos.x) * k,
                base_pos.y + (apex0.y - base_pos.y) * k,
            );
            (p, PointRef::New(p))
        } else {
            (apex0, apex)
        };
        let opening_mm = match self.typed_length()? {
            Some(w) => w,
            None => {
                // Souris : deux fois la distance du curseur à l'axe.
                let (ux, uy) = (base_pos.x - apex_pos.x, base_pos.y - apex_pos.y);
                let n = ux.hypot(uy);
                if n > 0.0 {
                    let (cx, cy) = (self.cursor.x - base_pos.x, self.cursor.y - base_pos.y);
                    2.0 * ((ux * cy - uy * cx) / n).abs()
                } else {
                    0.0
                }
            }
        };
        Ok(DartInput {
            apex_ref,
            base_pos,
            apex_pos,
            length_mm,
            opening_mm,
            fixed: if self.perp_flip {
                Side::Right
            } else {
                Side::Left
            },
        })
    }

    /// Pince en cours de pose, calculée sur un clone du projet (aucune trace).
    pub fn dart_draft(&self) -> Option<DartDraft> {
        let ToolState::DartBase { piece, apex, base } = self.state else {
            return None;
        };
        let i = self.dart_input(apex, base).ok()?;
        let mut p = self.project().clone();
        let result = p
            .apply(Command::AddPieceDart {
                piece,
                apex: i.apex_ref,
                toward: PointRef::New(i.base_pos),
                length_mm: i.length_mm,
                opening_mm: i.opening_mm,
                fixed: i.fixed,
            })
            .map_err(|e| crate::text::core_error(&e))
            .and_then(|out| {
                p.piece_dart_close(piece, out.darts[0])
                    .map_err(|e| crate::text::core_error(&e))
            });
        Some(DartDraft {
            apex: i.apex_pos,
            base: i.base_pos,
            length_mm: i.length_mm,
            opening_mm: i.opening_mm,
            result,
        })
    }

    /// Pose la pince (Entrée ou clic de l'étape d'ouverture).
    pub(crate) fn commit_dart(&mut self) {
        let ToolState::DartBase { piece, apex, base } = self.state else {
            return;
        };
        let i = match self.dart_input(apex, base) {
            Ok(i) => i,
            Err(e) => {
                self.error(e);
                return;
            }
        };
        let cmd = Command::AddPieceDart {
            piece,
            apex: i.apex_ref,
            toward: PointRef::New(i.base_pos),
            length_mm: i.length_mm,
            opening_mm: i.opening_mm,
            fixed: i.fixed,
        };
        if let Some(out) = self.exec(cmd) {
            self.state = ToolState::Idle;
            self.reset_inputs();
            if let Some(&d) = out.darts.first() {
                self.dart_selected = Some((piece, d));
                self.info(tr_args(
                    "status.dart.done",
                    &[("opening", &format_mm(i.opening_mm))],
                ));
            }
        }
    }

    /// Part (]0 ; 1]) du transfert : 1 en mode total, sinon le pourcentage saisi.
    fn transfer_part(&self) -> Result<f64, String> {
        if !self.dart_transfer.partial {
            return Ok(1.0);
        }
        match parse_length_mm(&self.dart_transfer.percent) {
            Ok(p) if p > 0.0 && p <= 100.0 => Ok(p / 100.0),
            _ => Err(tr("status.dart.transfer_percent")),
        }
    }

    /// Clic sur le bord cible : transfère la pince sélectionnée (une étape annulable, refus
    /// expliqué dans la barre d'état, rien ne change). La pièce est reconstruite sur une nouvelle
    /// feuille : l'affichage la suit.
    fn transfer_dart_at(&mut self, world: PointMm) {
        let Some((piece, dart)) = self.dart_selected else {
            self.dart_transfer.armed = false;
            return;
        };
        let part = match self.transfer_part() {
            Ok(p) => p,
            Err(e) => {
                self.error(e);
                return;
            }
        };
        let Some((edge, distance_mm)) = self.project().dart_target_at(piece, world) else {
            self.error(tr("status.dart.transfer_pick"));
            return;
        };
        let Some(out) = self.exec(Command::TransferPieceDart {
            piece,
            dart,
            edge,
            distance_mm,
            part,
        }) else {
            return;
        };
        self.dart_transfer.armed = false;
        if let Some(sheet) = out.sheet {
            self.set_sheet(sheet);
        }
        self.dart_selected = out.darts.first().map(|&d| (piece, d));
        self.info(tr_args(
            "status.dart.transfer_done",
            &[("count", &out.darts.len().to_string())],
        ));
    }

    /// Retire la pince sélectionnée ; vrai si une pince l'était.
    pub fn delete_dart(&mut self) -> bool {
        let Some((piece, dart)) = self.dart_selected else {
            return false;
        };
        self.dart_selected = None;
        self.exec(Command::RemovePieceDart { piece, dart });
        true
    }

    /// Change la jambe qui reste fixe (sens de pliage) de la pince sélectionnée.
    pub fn flip_dart(&mut self) {
        let Some((piece, dart)) = self.dart_selected else {
            return;
        };
        let Some(d) = self
            .project()
            .piece(piece)
            .ok()
            .and_then(|p| p.darts.iter().find(|d| d.id == dart).copied())
        else {
            return;
        };
        self.exec(Command::SetPieceDart {
            piece,
            dart,
            apex: d.apex,
            toward: d.toward,
            length_mm: d.length_mm,
            opening_mm: d.opening_mm,
            fixed: d.fixed.flipped(),
        });
    }
}

/// Projet de démonstration (capture `--etape pince`) : rectangle de 200 x 150 mm devenu pièce,
/// pince de 24 mm d'ouverture à 110 mm du bord bas, jambe droite fixe.
pub fn demo_dart_project() -> Result<(pf_core::Project, PieceId, DartId), pf_core::CoreError> {
    use pf_core::PieceSource;
    let mut p = pf_core::Project::new("Pince");
    let sheet = p.sheets[0].id;
    p.apply(Command::AddRectangle {
        sheet,
        corner: PointRef::New(PointMm::new(0.0, 0.0)),
        width_mm: 200.0,
        height_mm: 150.0,
    })?;
    let piece = p
        .apply(Command::ExtractPiece {
            sheet,
            source: PieceSource::Click {
                at: PointMm::new(100.0, 75.0),
            },
            name: "Devant".into(),
            detached: false,
        })?
        .pieces[0];
    let dart = p
        .apply(Command::AddPieceDart {
            piece,
            apex: PointRef::New(PointMm::new(70.0, 110.0)),
            toward: PointRef::New(PointMm::new(70.0, 0.0)),
            length_mm: 110.0,
            opening_mm: 24.0,
            fixed: Side::Right,
        })?
        .darts[0];
    Ok((p, piece, dart))
}

/// Projet de démonstration (capture `--etape pince-coupe-fermee`, PH-5a) : disque de rayon 100 mm
/// devenu pièce, marge 30 mm, pince de 50 mm d'ouverture et 60 mm de longueur au bas du disque
/// (pieds sur le cercle), réglée « couper pince fermée » : la coupe calculée (cercle de rayon
/// 130 mm) porte la pointe de marge de la pince (une corde qui remplace l'arc entre J et J').
pub fn demo_dart_cut_project() -> Result<(pf_core::Project, PieceId, DartId), pf_core::CoreError> {
    use pf_core::PieceSource;
    let mut p = pf_core::Project::new("Pince coupe");
    let sheet = p.sheets[0].id;
    p.apply(Command::AddCircle {
        sheet,
        center: PointRef::New(PointMm::new(0.0, 0.0)),
        radius_mm: 100.0,
    })?;
    let piece = p
        .apply(Command::ExtractPiece {
            sheet,
            source: PieceSource::Click {
                at: PointMm::new(0.0, 0.0),
            },
            name: "Disque".into(),
            detached: false,
        })?
        .pieces[0];
    p.apply(Command::SetSeamAllowance {
        piece,
        seam: Some(pf_core::SeamAllowance {
            default_mm: 30.0,
            ..Default::default()
        }),
    })?;
    let y0 = -(100.0_f64 * 100.0 - 25.0 * 25.0).sqrt();
    let dart = p
        .apply(Command::AddPieceDart {
            piece,
            apex: PointRef::New(PointMm::new(0.0, y0 + 60.0)),
            toward: PointRef::New(PointMm::new(0.0, y0)),
            length_mm: 60.0,
            opening_mm: 50.0,
            fixed: Side::Right,
        })?
        .darts[0];
    p.apply(Command::SetPieceDartCutClosed {
        piece,
        dart,
        closed: true,
    })?;
    Ok((p, piece, dart))
}

/// Projet de démonstration (capture `--etape pince-transfert`) : la pince de `demo_dart_project`
/// transférée pour moitié de son angle vers le bord gauche, à 90 mm du bord bas.
pub fn demo_dart_transfer_project(
) -> Result<(pf_core::Project, PieceId, DartId, pf_core::SheetId), pf_core::CoreError> {
    let (mut p, piece, dart) = demo_dart_project()?;
    let (edge, distance_mm) = p
        .dart_target_at(piece, PointMm::new(0.0, 90.0))
        .ok_or_else(|| pf_core::CoreError::Invalid("target edge not found".into()))?;
    let out = p.apply(Command::TransferPieceDart {
        piece,
        dart,
        edge,
        distance_mm,
        part: 0.5,
    })?;
    let sheet = out
        .sheet
        .ok_or_else(|| pf_core::CoreError::Invalid("result sheet missing".into()))?;
    Ok((p, piece, dart, sheet))
}

fn pts_of(edges: &[pf_geometry::region::EdgeGeom], xf: &Xf) -> Vec<Pos2> {
    let mut out: Vec<Pos2> = Vec::new();
    for e in edges {
        for p in edge_sample(e, 32) {
            out.push(xf.s(p));
        }
    }
    out
}

impl App {
    /// Pinces des pièces de la feuille : jambes en trait mixte et libellé « Pince » ; si la pince
    /// est sélectionnée (outil Pince) ou en cours de pose : ligne fermée et capuchon.
    pub(crate) fn paint_darts(&self, painter: &egui::Painter, xf: &Xf) {
        let project = self.editor.project();
        let color = pal().measure;
        let dash: (&[f32], &[f32]) = (&[10.0, 3.0, 2.0, 3.0], &[3.0, 3.0, 3.0, 3.0]);
        let selected = self.editor.dart_selected;
        for piece in project.pieces_on(self.editor.sheet) {
            for (rank, r) in project.piece_darts(piece.id).into_iter().enumerate() {
                let chosen = selected == Some((piece.id, r.dart.id));
                match &r.calc {
                    Ok(c) => {
                        for foot in [c.shape.left_foot, c.shape.right_foot] {
                            polyline(
                                painter,
                                vec![xf.s(c.shape.apex), xf.s(foot)],
                                color,
                                if chosen { 3.0 } else { 2.0 },
                                dash.0,
                                dash.1,
                            );
                        }
                        label(
                            painter,
                            xf.s(c.shape.apex) + egui::vec2(8.0, -6.0 - 15.0 * rank as f32),
                            Align2::LEFT_BOTTOM,
                            &tr_args("dart.label", &[("opening", &format_mm(r.dart.opening_mm))]),
                            color,
                        );
                        if chosen && self.editor.tool == Tool::Dart {
                            self.paint_dart_closing(painter, xf, &c.closed);
                        }
                    }
                    Err(_) => {
                        if let Ok(a) = self.editor.sheet().position(r.dart.apex) {
                            painter.circle_stroke(xf.s(a), 6.0, stroke(2.0, color));
                            label(
                                painter,
                                xf.s(a) + egui::vec2(8.0, -6.0),
                                Align2::LEFT_BOTTOM,
                                &tr("dart.label.invalid"),
                                pal().error,
                            );
                        }
                    }
                }
            }
        }
        if let Some(d) = self.editor.dart_draft() {
            painter.extend(egui::Shape::dashed_line(
                &[xf.s(d.apex), xf.s(d.base)],
                stroke(1.5, pal().preview),
                6.0,
                4.0,
            ));
            match &d.result {
                Ok(c) => {
                    for foot in [c.shape.left_foot, c.shape.right_foot] {
                        polyline(
                            painter,
                            vec![xf.s(c.shape.apex), xf.s(foot)],
                            pal().preview,
                            2.4,
                            dash.0,
                            dash.1,
                        );
                    }
                    self.paint_dart_closing(painter, xf, &c.closed);
                }
                Err(_) => {
                    label(
                        painter,
                        xf.s(d.apex) + egui::vec2(8.0, 14.0),
                        Align2::LEFT_TOP,
                        &format!(
                            "{} / {}",
                            pf_core::units::len_number(d.opening_mm, 1),
                            pf_core::units::len_number(d.length_mm, 1)
                        ),
                        pal().preview,
                    );
                }
            }
        }
    }

    /// Ligne pliée (trait plein) et capuchon (trait épais, libellé).
    fn paint_dart_closing(
        &self,
        painter: &egui::Painter,
        xf: &Xf,
        closed: &pf_geometry::dart::ClosedLine,
    ) {
        let pts = pts_of(&closed.chain, xf);
        if pts.len() >= 2 {
            painter.add(egui::Shape::line(pts, stroke(2.0, pal().preview)));
        }
        if let Some(cap) = closed.capuchon {
            let line: Vec<Pos2> = cap.iter().map(|&p| xf.s(p)).collect();
            painter.add(egui::Shape::line(line.clone(), stroke(3.5, pal().measure)));
            label(
                painter,
                line[1].lerp(line[2], 0.5) + egui::vec2(0.0, 12.0),
                Align2::CENTER_TOP,
                &tr("dart.cap"),
                pal().measure,
            );
        }
    }

    /// Fenêtre « Pince » : mesures de la pince sélectionnée et actions (côté fixe, suppression).
    pub(crate) fn dart_window(&mut self, ctx: &egui::Context, _canvas: egui::Rect) {
        if self.editor.tool != Tool::Dart {
            return;
        }
        let Some((piece, dart)) = self.editor.dart_selected else {
            return;
        };
        let Some(r) = self
            .editor
            .project()
            .piece_darts(piece)
            .into_iter()
            .find(|r| r.dart.id == dart)
        else {
            self.editor.dart_selected = None;
            return;
        };
        let mut action = 0u8;
        // Coupe pince fermée (PH-5a) : le motif du repli vient du modèle (aucun calcul ici).
        let cut_failure = r
            .dart
            .cut_closed
            .then(|| self.editor.project().piece_cut_dart_failure(piece))
            .flatten();
        let slot = crate::dock::slot(ctx, &self.dock, crate::dock::ToolWin::Dart);
        let width = self.dock.content_width(crate::dock::ToolWin::Dart);
        egui::Area::new(egui::Id::new("dart-window"))
            .order(egui::Order::Foreground)
            .fixed_pos(slot.pos)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_width(width);
                    crate::a11y::label(
                        ui,
                        RichText::new(tr("dart.panel.title"))
                            .small()
                            .strong()
                            .color(pal().accent_text),
                    );
                    crate::a11y::label(
                        ui,
                        tr_args(
                            "dart.panel.params",
                            &[
                                ("opening", &format_mm(r.dart.opening_mm)),
                                ("length", &format_mm(r.dart.length_mm)),
                            ],
                        ),
                    );
                    let side = match r.dart.fixed {
                        Side::Left => tr("palette.side.left"),
                        Side::Right => tr("palette.side.right"),
                    };
                    crate::a11y::label(ui, tr_args("dart.panel.fixed", &[("side", &side)]));
                    match &r.calc {
                        Ok(c) => {
                            crate::a11y::label(
                                ui,
                                tr_args(
                                    "dart.panel.legs",
                                    &[
                                        ("leg", &format_mm(c.shape.leg_left_mm)),
                                        ("angle", &format_number(c.shape.angle_deg, 2)),
                                    ],
                                ),
                            );
                            crate::a11y::label(
                                ui,
                                tr_args(
                                    "dart.panel.line",
                                    &[
                                        ("before", &format_mm(c.closed.before_mm)),
                                        ("after", &format_mm(c.closed.after_mm)),
                                        ("removed", &format_mm(c.closed.removed_mm)),
                                    ],
                                ),
                            );
                            if let Some(corner) = c.closed.corner_deg {
                                crate::a11y::label(
                                    ui,
                                    tr_args(
                                        "dart.panel.corner",
                                        &[("angle", &format_number(corner, 2))],
                                    ),
                                );
                            }
                            match c.closed.capuchon_leg_mm {
                                Some(leg) => crate::a11y::label(
                                    ui,
                                    tr_args("dart.panel.cap", &[("leg", &format_mm(leg))]),
                                ),
                                None => crate::a11y::label(ui, tr("dart.panel.cap_none")),
                            };
                        }
                        Err(why) => {
                            crate::a11y::label(
                                ui,
                                RichText::new(tr_args("dart.panel.invalid", &[("reason", why)]))
                                    .color(pal().error),
                            );
                        }
                    }
                    let mut cut_closed = r.dart.cut_closed;
                    if ui
                        .checkbox(&mut cut_closed, tr("dart.panel.cut_closed"))
                        .changed()
                    {
                        action = 5;
                    }
                    if let Some(why) = &cut_failure {
                        crate::a11y::label(
                            ui,
                            RichText::new(format!(
                                "⚠ {}",
                                tr_args("dart.panel.cut_closed_failed", &[("reason", why)])
                            ))
                            .color(pal().error),
                        );
                    }
                    // Transfert (US-07.2) : mode total / partiel, part, puis clic sur le bord cible.
                    ui.separator();
                    let ui_state = &mut self.editor.dart_transfer;
                    ui.horizontal(|ui| {
                        if ui
                            .selectable_label(!ui_state.partial, tr("dart.transfer.total"))
                            .clicked()
                        {
                            ui_state.partial = false;
                        }
                        if ui
                            .selectable_label(ui_state.partial, tr("dart.transfer.partial"))
                            .clicked()
                        {
                            ui_state.partial = true;
                        }
                        if ui_state.partial {
                            crate::a11y::add_text(
                                ui,
                                egui::TextEdit::singleline(&mut ui_state.percent)
                                    .desired_width(44.0),
                            );
                            crate::a11y::label(ui, "%");
                        }
                    });
                    let label = if ui_state.armed {
                        tr("dart.transfer.cancel")
                    } else {
                        tr("dart.transfer.button")
                    };
                    if ui
                        .add_enabled(r.calc.is_ok(), egui::Button::new(label))
                        .clicked()
                    {
                        action = 4;
                    }
                    ui.horizontal(|ui| {
                        if ui.button(tr("dart.panel.flip")).clicked() {
                            action = 1;
                        }
                        if ui.button(tr("dart.panel.delete")).clicked() {
                            action = 2;
                        }
                        if ui.button(tr("dart.panel.close")).clicked() {
                            action = 3;
                        }
                    });
                });
            });
        match action {
            1 => self.editor.flip_dart(),
            2 => {
                self.editor.delete_dart();
            }
            3 => {
                self.editor.dart_selected = None;
                self.editor.dart_transfer.armed = false;
            }
            5 => {
                self.editor.exec(Command::SetPieceDartCutClosed {
                    piece,
                    dart,
                    closed: !r.dart.cut_closed,
                });
            }
            4 => {
                let armed = !self.editor.dart_transfer.armed;
                self.editor.dart_transfer.armed = armed;
                self.editor.info(if armed {
                    tr("status.dart.transfer_pick")
                } else {
                    tr("status.cancelled")
                });
            }
            _ => {}
        }
    }
}
