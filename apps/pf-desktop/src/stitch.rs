//! Bloc « Lignes de piqûre… » de la section de marquages d'une pièce (US-06.9, maroquinerie).
//! Il ne calcule rien : trous, pas réel et refus viennent de `Project::stitch_line_preview`
//! (lecture seule) et des commandes `AddStitchLine` / `MergeStitchLines` / `RemoveStitchLine`
//! (une étape annulable, refus sans trace). Le bord se prend dans la sélection (tracés du
//! contour) ; la ligne reste **liée** à son bord après création.
//!
//! Replié par défaut. Aucune liste déroulante (les tests de l'inspecteur de pièce comptent les
//! `ComboBox`) : les choix sont des boutons à bascule.

use crate::app::App;
use crate::palette::pal;
use crate::text::core_error;
use eframe::egui::{self, RichText};
use pf_core::input::format_mm;
use pf_core::units::parse_length_display;
use pf_core::{Command, PieceId, StitchHole, StitchLineId, StitchLineSpec};
use pf_geometry::PointMm;
use pf_i18n::{tr, tr_args};

/// Forme choisie dans le formulaire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HoleKind {
    Round,
    Flat,
    French,
    Japanese,
}

/// Aperçu affiché et dessiné sur le plan : trous tels que la commande les créera.
#[derive(Debug, Clone, PartialEq)]
pub struct StitchPreview {
    pub count: usize,
    pub step_mm: f64,
    pub length_mm: f64,
    /// Centre et direction du bord de chaque trou.
    pub holes: Vec<(PointMm, f64)>,
    pub hole: StitchHole,
}

/// Formulaire « Lignes de piqûre… » : état d'interface, jamais enregistré.
pub struct StitchForm {
    pub piece: PieceId,
    pub kind: HoleKind,
    pub pitch: String,
    pub setback: String,
    /// Nombre de trous automatique (d'après le pas) plutôt que fixé.
    pub auto: bool,
    pub count: String,
    pub diameter: String,
    pub length: String,
    pub width: String,
    pub slant: String,
    /// Ligne que la prochaine création continue.
    pub continues: Option<StitchLineId>,
    pub preview: Option<StitchPreview>,
    /// Ligne en cours d'appariement (US-08.6) : le choix de l'autre ligne est affiché.
    pub pair_from: Option<StitchLineId>,
    /// Ligne en cours de report sur une autre pièce (US-08.6).
    pub carry_from: Option<StitchLineId>,
}

impl StitchForm {
    pub fn new(piece: PieceId) -> StitchForm {
        StitchForm {
            piece,
            kind: HoleKind::Round,
            pitch: "4".into(),
            setback: "3".into(),
            auto: true,
            count: "10".into(),
            diameter: "1.2".into(),
            length: "3".into(),
            width: "1".into(),
            slant: "45".into(),
            continues: None,
            preview: None,
            pair_from: None,
            carry_from: None,
        }
    }

    /// Paramètres saisis ; `Err` = clé de message d'un champ qui n'est pas un nombre.
    pub fn spec(&self, edges: Vec<pf_core::EntityId>) -> Result<StitchLineSpec, &'static str> {
        let num = |t: &str| parse_length_display(t).map_err(|_| "stitch.invalid");
        let hole = match self.kind {
            HoleKind::Round => StitchHole::Round {
                diameter_mm: num(&self.diameter)?,
            },
            HoleKind::Flat => StitchHole::Flat {
                length_mm: num(&self.length)?,
                width_mm: num(&self.width)?,
            },
            HoleKind::French => StitchHole::French {
                length_mm: num(&self.length)?,
                width_mm: num(&self.width)?,
            },
            HoleKind::Japanese => StitchHole::Japanese {
                length_mm: num(&self.length)?,
                width_mm: num(&self.width)?,
                slant_deg: self
                    .slant
                    .trim()
                    .replace(',', ".")
                    .parse::<f64>()
                    .map_err(|_| "stitch.invalid")?,
            },
        };
        let count = if self.auto {
            None
        } else {
            Some(
                self.count
                    .trim()
                    .parse::<u32>()
                    .map_err(|_| "stitch.invalid_count")?,
            )
        };
        Ok(StitchLineSpec {
            edges,
            hole,
            pitch_mm: num(&self.pitch)?,
            setback_mm: num(&self.setback)?,
            count,
            continues: self.continues,
        })
    }
}

fn field(ui: &mut egui::Ui, label_key: &str, text: &mut String) {
    crate::a11y::label(ui, tr(label_key));
    crate::a11y::add_text(ui, egui::TextEdit::singleline(text).desired_width(70.0));
    ui.end_row();
}

/// Libellé d'une forme de trou (jamais la couleur seule).
pub fn hole_label(hole: StitchHole) -> String {
    match hole {
        StitchHole::Round { diameter_mm } => {
            tr_args("stitch.shape.round", &[("size", &format_mm(diameter_mm))])
        }
        StitchHole::Flat {
            length_mm,
            width_mm,
        } => tr_args(
            "stitch.shape.flat",
            &[
                ("length", &format_mm(length_mm)),
                ("width", &format_mm(width_mm)),
            ],
        ),
        StitchHole::French {
            length_mm,
            width_mm,
        } => tr_args(
            "stitch.shape.french",
            &[
                ("length", &format_mm(length_mm)),
                ("width", &format_mm(width_mm)),
            ],
        ),
        StitchHole::Japanese {
            length_mm,
            width_mm,
            ..
        } => tr_args(
            "stitch.shape.japanese",
            &[
                ("length", &format_mm(length_mm)),
                ("width", &format_mm(width_mm)),
            ],
        ),
    }
}

impl App {
    /// Bloc replié « Lignes de piqûre… » de la section de marquages d'une pièce.
    pub(crate) fn piece_stitch_block(&mut self, ui: &mut egui::Ui, piece: &pf_core::Piece) {
        let mut header = egui::CollapsingHeader::new(
            RichText::new(tr("stitch.title"))
                .small()
                .strong()
                .color(pal().muted),
        )
        .id_salt(("stitch-lines", piece.id.0))
        .default_open(false);
        // Étape de capture : ouvert une fois, et amené dans la partie visible du panneau.
        let once = std::mem::take(&mut self.stitch_open_once);
        if once {
            header = header.open(Some(true));
        }
        let shown = header.show(ui, |ui| self.stitch_body(ui, piece));
        crate::panels::scroll_block_into_view(
            ui,
            ("stitch", piece.id.0),
            shown.header_response.rect,
            once,
        );
    }

    fn stitch_body(&mut self, ui: &mut egui::Ui, piece: &pf_core::Piece) {
        let id = piece.id;
        let mut form = match self.stitch_form.take() {
            Some(f) if f.piece == id => f,
            _ => StitchForm::new(id),
        };
        crate::a11y::label(
            ui,
            RichText::new(tr("stitch.hint")).small().color(pal().muted),
        );
        // Lignes existantes : résumé, continuer, fusionner avec la suivante, supprimer.
        let mut commands: Vec<Command> = Vec::new();
        for (i, line) in piece.stitch_lines.iter().enumerate() {
            let project = self.editor.view_project();
            let summary = match project.stitch_line_layout(id, line.id) {
                Ok(l) => tr_args(
                    "stitch.line.row",
                    &[
                        ("id", &line.id.0.to_string()),
                        ("count", &l.holes.len().to_string()),
                        ("step", &format_mm(l.step_mm)),
                        ("shape", &hole_label(line.hole)),
                    ],
                ),
                Err(e) => core_error(&e),
            };
            crate::a11y::label(ui, RichText::new(summary).small());
            if let Some(c) = line.continues {
                crate::a11y::label(
                    ui,
                    RichText::new(tr_args(
                        "stitch.line.continues",
                        &[("id", &c.0.to_string())],
                    ))
                    .small()
                    .color(pal().muted),
                );
            }
            ui.horizontal_wrapped(|ui| {
                if ui.small_button(tr("stitch.line.continue")).clicked() {
                    form.continues = Some(line.id);
                }
                let next = piece.stitch_lines.get(i + 1).map(|n| n.id);
                if ui
                    .add_enabled(
                        next.is_some(),
                        egui::Button::new(tr("stitch.line.merge")).small(),
                    )
                    .clicked()
                {
                    if let Some(second) = next {
                        commands.push(Command::MergeStitchLines {
                            piece: id,
                            first: line.id,
                            second,
                        });
                    }
                }
                if ui.small_button(tr("stitch.line.remove")).clicked() {
                    commands.push(Command::RemoveStitchLine {
                        piece: id,
                        line: line.id,
                    });
                }
            });
            // US-08.6 : appariement, report ou rupture du couple.
            let paired = self.editor.view_project().stitch_pair_of(id, line.id);
            ui.horizontal_wrapped(|ui| match paired {
                None => {
                    if ui.small_button(tr("stitch.pair.start")).clicked() {
                        form.pair_from = Some(line.id);
                        form.carry_from = None;
                    }
                    if ui.small_button(tr("stitch.carry.start")).clicked() {
                        form.carry_from = Some(line.id);
                        form.pair_from = None;
                    }
                }
                Some((_, pair)) => {
                    if ui.small_button(tr("stitch.pair.break")).clicked() {
                        commands.push(Command::UnpairStitchLines { pair: pair.id });
                    }
                }
            });
        }
        // Choix de l'autre ligne (appariement) ou de la pièce voisine (report).
        let selection = self.editor.selection.entities.clone();
        if let Some(from) = form.pair_from {
            crate::a11y::label(ui, RichText::new(tr("stitch.pair.choose")).small());
            let project = self.editor.view_project();
            for other in project.pieces.iter().filter(|p| p.id != id) {
                for l in &other.stitch_lines {
                    if project.stitch_pair_of(other.id, l.id).is_some() {
                        continue;
                    }
                    let label = tr_args(
                        "stitch.pair.with",
                        &[("id", &l.id.0.to_string()), ("piece", &other.name)],
                    );
                    if ui.small_button(label).clicked() {
                        commands.push(Command::PairStitchLines {
                            a: pf_core::StitchRef {
                                piece: id,
                                line: from,
                            },
                            b: pf_core::StitchRef {
                                piece: other.id,
                                line: l.id,
                            },
                        });
                        form.pair_from = None;
                    }
                }
            }
            if ui.small_button(tr("stitch.pair.cancel")).clicked() {
                form.pair_from = None;
            }
        }
        if let Some(from) = form.carry_from {
            crate::a11y::label(ui, RichText::new(tr("stitch.carry.choose")).small());
            let project = self.editor.view_project();
            for other in project.pieces.iter().filter(|p| p.id != id) {
                let edges = project.marking_run_edges(other.id, &selection);
                let label = tr_args("stitch.carry.to", &[("piece", &other.name)]);
                if ui
                    .add_enabled(edges.is_ok(), egui::Button::new(label).small())
                    .clicked()
                {
                    if let Ok(to_edges) = edges {
                        commands.push(Command::CarryStitchLine {
                            from: pf_core::StitchRef {
                                piece: id,
                                line: from,
                            },
                            to_piece: other.id,
                            to_edges,
                        });
                        form.carry_from = None;
                    }
                }
            }
            if ui.small_button(tr("stitch.pair.cancel")).clicked() {
                form.carry_from = None;
            }
        }
        for c in commands {
            if self.editor.exec(c).is_some() {
                form.preview = None;
            }
        }
        // Lecture du contrôle (US-08.6) des couples qui touchent cette pièce.
        let reports: Vec<pf_core::StitchCheckReading> = self
            .editor
            .view_project()
            .stitch_check_report()
            .into_iter()
            .filter(|r| r.a.piece == id || r.b.piece == id)
            .collect();
        for r in &reports {
            crate::a11y::label(
                ui,
                RichText::new(match r.not_computable {
                    Some(_) => tr_args("stitch.pair.na", &[("n", &r.number.to_string())]),
                    None => tr_args(
                        "stitch.pair.summary",
                        &[
                            ("n", &r.number.to_string()),
                            ("a", &r.count_a.to_string()),
                            ("b", &r.count_b.to_string()),
                            ("sa", &format_mm(r.mean_step_a)),
                            ("sb", &format_mm(r.mean_step_b)),
                            ("gap", &format_mm(r.step_gap_mm)),
                        ],
                    ),
                })
                .small(),
            );
            let flagged: Vec<&pf_core::StitchCheckItem> = r
                .items
                .iter()
                .filter(|i| i.status != pf_core::StitchMatch::Matched)
                .collect();
            for (shown, it) in flagged.iter().enumerate() {
                if shown == 8 {
                    crate::a11y::label(
                        ui,
                        RichText::new(tr_args(
                            "stitch.pair.more",
                            &[("count", &(flagged.len() - shown).to_string())],
                        ))
                        .small()
                        .color(pal().muted),
                    );
                    break;
                }
                let (key, hole) = match it.status {
                    pf_core::StitchMatch::Shifted => ("stitch.pair.item.shifted", it.a),
                    pf_core::StitchMatch::Missing(pf_core::seam_pair::SeamWhich::B) => {
                        ("stitch.pair.item.missing", it.a)
                    }
                    _ => ("stitch.pair.item.extra", it.b),
                };
                let (x, y) = hole.map_or((0.0, 0.0), |h| (h.position.x, h.position.y));
                crate::a11y::label(
                    ui,
                    RichText::new(tr_args(
                        key,
                        &[
                            ("n", &it.number.to_string()),
                            ("x", &format_mm(x)),
                            ("y", &format_mm(y)),
                            ("gap", &format_mm(it.gap_mm.unwrap_or(0.0))),
                        ],
                    ))
                    .small()
                    .color(ui.visuals().warn_fg_color),
                );
            }
        }
        if piece.stitch_lines.is_empty() {
            crate::a11y::label(
                ui,
                RichText::new(tr("stitch.none")).small().color(pal().muted),
            );
        }
        // Bord : tracés du contour pris dans la sélection (ordre du contour calculé par le cœur).
        let edges = self
            .editor
            .project()
            .marking_run_edges(id, &self.editor.selection.entities);
        match &edges {
            Ok(e) => {
                crate::a11y::label(
                    ui,
                    tr_args("stitch.edge", &[("count", &e.len().to_string())]),
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
            ui.selectable_value(&mut form.kind, HoleKind::Round, tr("stitch.form.round"));
            ui.selectable_value(&mut form.kind, HoleKind::Flat, tr("stitch.form.flat"));
            ui.selectable_value(&mut form.kind, HoleKind::French, tr("stitch.form.french"));
            ui.selectable_value(
                &mut form.kind,
                HoleKind::Japanese,
                tr("stitch.form.japanese"),
            );
        });
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(&mut form.auto, true, tr("stitch.mode.auto"));
            ui.selectable_value(&mut form.auto, false, tr("stitch.mode.count"));
        });
        egui::Grid::new(("stitch-form", id.0))
            .num_columns(2)
            .show(ui, |ui| {
                field(ui, "stitch.pitch", &mut form.pitch);
                field(ui, "stitch.setback", &mut form.setback);
                if !form.auto {
                    field(ui, "stitch.count", &mut form.count);
                }
                match form.kind {
                    HoleKind::Round => field(ui, "stitch.diameter", &mut form.diameter),
                    _ => {
                        field(ui, "stitch.length", &mut form.length);
                        field(ui, "stitch.width", &mut form.width);
                        if form.kind == HoleKind::Japanese {
                            field(ui, "stitch.slant", &mut form.slant);
                        }
                    }
                }
            });
        if let Some(c) = form.continues {
            ui.horizontal_wrapped(|ui| {
                crate::a11y::label(
                    ui,
                    RichText::new(tr_args("stitch.continues", &[("id", &c.0.to_string())])).small(),
                );
                if ui.small_button(tr("stitch.continues.clear")).clicked() {
                    form.continues = None;
                }
            });
        }
        let ready = edges.is_ok();
        let (mut preview, mut create) = (false, false);
        ui.horizontal_wrapped(|ui| {
            preview = ui
                .add_enabled(ready, egui::Button::new(tr("stitch.preview")))
                .clicked();
            create = ui
                .add_enabled(ready, egui::Button::new(tr("stitch.create")))
                .clicked();
        });
        if let (true, Ok(edges)) = (preview || create, edges) {
            match form.spec(edges) {
                Err(key) => {
                    form.preview = None;
                    self.editor.error(tr(key));
                }
                Ok(spec) if preview => match self.editor.project().stitch_line_preview(id, &spec) {
                    Ok(l) => {
                        form.preview = Some(StitchPreview {
                            count: l.holes.len(),
                            step_mm: l.step_mm,
                            length_mm: l.length_mm,
                            holes: l
                                .holes
                                .iter()
                                .map(|h| (h.position, h.tangent_deg))
                                .collect(),
                            hole: spec.hole,
                        });
                    }
                    Err(e) => {
                        form.preview = None;
                        self.editor.error(core_error(&e));
                    }
                },
                Ok(spec) => {
                    if let Some(o) = self.editor.exec(Command::AddStitchLine {
                        piece: id,
                        line: spec,
                    }) {
                        form.preview = None;
                        form.continues = None;
                        self.editor.info(tr_args(
                            "stitch.added",
                            &[("id", &o.stitch_lines[0].0.to_string())],
                        ));
                    }
                }
            }
        }
        if let Some(p) = &form.preview {
            crate::a11y::label(
                ui,
                RichText::new(tr_args(
                    "stitch.result",
                    &[
                        ("count", &p.count.to_string()),
                        ("step", &format_mm(p.step_mm)),
                        ("length", &format_mm(p.length_mm)),
                    ],
                ))
                .small(),
            );
        }
        crate::a11y::label(
            ui,
            RichText::new(tr("stitch.note")).small().color(pal().muted),
        );
        self.stitch_form = Some(form);
    }

    /// Trous des lignes de piqûre de la feuille et aperçu du formulaire, sur le plan : la
    /// **forme** (cercle, rectangle, losange, rectangle incliné) et le libellé les distinguent,
    /// jamais la couleur seule ; le numéro 1 marque le premier trou. Non sélectionnable.
    pub(crate) fn paint_stitch_lines(&self, painter: &egui::Painter, xf: &crate::canvas::Xf) {
        let project = self.editor.view_project();
        let zoom = self.view.zoom as f32;
        let color = pal().internal;
        let draw = |hole: StitchHole, at: PointMm, tangent: f64, color: egui::Color32| {
            let c = xf.s(at);
            let st = egui::Stroke::new(1.5_f32, color);
            match hole.polygon(at, tangent) {
                None => {
                    let r = match hole {
                        StitchHole::Round { diameter_mm } => diameter_mm as f32 / 2.0,
                        _ => 0.5,
                    };
                    painter.circle_stroke(c, (r * zoom).max(2.5), st);
                }
                Some(v) => {
                    let mut pts: Vec<egui::Pos2> = v.iter().map(|p| xf.s(*p)).collect();
                    pts.push(pts[0]);
                    painter.add(egui::Shape::line(pts, st));
                }
            }
            c
        };
        // Numéros appariés (US-08.6) : un trou d'une ligne en couple porte le numéro de son
        // vis-à-vis (le même sur les deux pièces).
        let numbers = project.stitch_all_paired_numbers();
        for piece in project.pieces_on(self.editor.sheet) {
            for line in project.piece_stitch_lines(piece.id) {
                let paired = numbers.get(&(piece.id, line.id));
                for h in &line.layout.holes {
                    let c = draw(line.hole, h.position, h.tangent_deg, color);
                    if let Some(n) = paired.and_then(|v| v.get(h.number as usize - 1)) {
                        crate::canvas::label(
                            painter,
                            c + egui::vec2(0.0, -8.0),
                            egui::Align2::CENTER_BOTTOM,
                            &n.to_string(),
                            color,
                        );
                    }
                    if h.number == 1 {
                        crate::canvas::label(
                            painter,
                            c + egui::vec2(6.0, 4.0),
                            egui::Align2::LEFT_TOP,
                            &tr_args(
                                "stitch.canvas.label",
                                &[
                                    ("id", &line.id.0.to_string()),
                                    ("shape", &hole_label(line.hole)),
                                    ("count", &line.layout.holes.len().to_string()),
                                ],
                            ),
                            color,
                        );
                    }
                }
            }
        }
        if let Some(p) = self.stitch_form.as_ref().and_then(|f| f.preview.as_ref()) {
            for (at, tangent) in &p.holes {
                draw(p.hole, *at, *tangent, pal().select);
            }
        }
    }
}

/// Étape de capture : un point est sélectionné (la section de marquages s'affiche) et le bloc
/// s'ouvre une fois.
pub(crate) fn demo_stitch(app: &mut App) {
    if let Some(first) = app.editor.sheet().points.first().map(|p| p.id) {
        app.editor.selection.points = vec![first];
    }
    // Un tracé du contour est sélectionné : le bord est valide, les boutons sont actifs.
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
    app.stitch_open_once = true;
}

/// Projet de démonstration d'US-08.6 (étape de capture) : deux rectangles 100 × 70 mm, une ligne
/// de 9 trous sur le bas du premier, une de 8 trous sur le bas du second, déclarées appariées :
/// le contrôle liste les trous décalés et sans vis-à-vis.
pub(crate) fn demo_stitch_pair_project() -> Result<pf_core::Project, pf_core::CoreError> {
    use pf_core::{PathRole, PieceSource, PointRef, Project, StitchRef};
    let pt = PointMm::new;
    let mut p = Project::new("Controle des trous de piqure");
    let sheet = p.sheets[0].id;
    let piece =
        |p: &mut Project, x0: f64, name: &str, count: u32| -> Result<_, pf_core::CoreError> {
            p.apply(Command::AddPolygon {
                sheet,
                vertices: [(x0, 0.0), (x0 + 100.0, 0.0), (x0 + 100.0, 70.0), (x0, 70.0)]
                    .map(|(x, y)| PointRef::New(pt(x, y)))
                    .to_vec(),
                closed: true,
                role: PathRole::default(),
                layer: None,
            })?;
            let id = p
                .apply(Command::ExtractPiece {
                    sheet,
                    source: PieceSource::Click {
                        at: pt(x0 + 50.0, 35.0),
                    },
                    name: name.into(),
                    detached: false,
                })?
                .pieces[0];
            let sh = p.sheet(sheet)?;
            let bottom = sh
                .entities
                .iter()
                .find(|e| {
                    sh.segment_ends(e.id).is_ok_and(|(a, b)| {
                        a.y == 0.0 && b.y == 0.0 && (a.x - x0).abs().min((b.x - x0).abs()) < 1e-9
                    })
                })
                .map(|e| e.id)
                .ok_or_else(|| pf_core::CoreError::Invalid("demo edge not found".into()))?;
            let line = p
                .apply(Command::AddStitchLine {
                    piece: id,
                    line: StitchLineSpec {
                        edges: vec![bottom],
                        hole: StitchHole::Round { diameter_mm: 1.2 },
                        pitch_mm: 4.0,
                        setback_mm: 3.0,
                        count: Some(count),
                        continues: None,
                    },
                })?
                .stitch_lines[0];
            Ok((id, line))
        };
    let (a, la) = piece(&mut p, 0.0, "Devant", 9)?;
    let (b, lb) = piece(&mut p, 150.0, "Dos", 8)?;
    p.apply(Command::PairStitchLines {
        a: StitchRef { piece: a, line: la },
        b: StitchRef { piece: b, line: lb },
    })?;
    Ok(p)
}

/// Projet de démonstration (étape de capture) : fond de porte-cartes 100 × 70 mm à congés de
/// 5 mm, une ligne de piqûre autour du contour entier (losanges français, pas 4 mm, retrait
/// 3 mm).
pub(crate) fn demo_stitch_project() -> Result<pf_core::Project, pf_core::CoreError> {
    let mut p = crate::marking_run::demo_pitch_project()?;
    let piece = p.pieces[0].id;
    let edges: Vec<pf_core::EntityId> = p.pieces[0].outline.iter().map(|e| e.entity).collect();
    // Retire la rangée de perçages de la démonstration des perçages au pas.
    let ids: Vec<_> = p.pieces[0].markings.iter().map(|m| m.id).collect();
    for marking in ids {
        p.apply(Command::RemovePieceMarking { piece, marking })?;
    }
    p.apply(Command::AddStitchLine {
        piece,
        line: StitchLineSpec {
            edges,
            hole: StitchHole::French {
                length_mm: 3.0,
                width_mm: 1.5,
            },
            pitch_mm: 4.0,
            setback_mm: 3.0,
            count: None,
            continues: None,
        },
    })?;
    Ok(p)
}
