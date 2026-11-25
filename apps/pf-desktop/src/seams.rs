//! Fenêtre « Coutures… » (US-08.1) : déclarer que le bord A d'une pièce se coud avec le bord B
//! d'une autre, et lire le tableau des couples. Elle ne calcule rien : les longueurs, l'écart,
//! l'embu mesuré et le statut viennent de `Project::seam_pair_report` (mesures **observées**
//! recalculées à chaque image, donc toujours à jour ; jamais stockées, jamais des contraintes),
//! la déclaration est `Command::AddSeamPair` / `SetSeamPair` / `RemoveSeamPair` (une étape
//! annulable, refus sans trace). Chaque statut porte une icône **et** un texte. Les deux bords
//! appariés sont surlignés sur le plan avec le libellé « couture n ».
//!
//! Marche de pièces (US-08.2) : pour le couple choisi, « Reporter les crans » exécute
//! `Command::MarchNotches` (une étape annulable) ; l'aperçu (texte et repères sur le plan) vient de
//! `Project::march_preview`, qui ne modifie rien.
//!
//! Contrôle des crans (US-08.4) : section repliée « Contrôle des crans » ; la lecture vient de
//! `Project::notch_check_report` (observée, jamais stockée) ; « Reporter les manquants » exécute
//! `Command::MarchNotches` pour les seuls crans manquants (une étape annulable, `Batch` quand les
//! deux côtés en ont). Cran manquant : icône dessinée + texte, repère sur le plan.
//!
//! Contrôle des raccords (US-08.3, `junctions.rs`) : section repliée « Raccords » (angle de chaque
//! jonction, somme, écart, statut en texte et symbole) et fenêtre « Vue assemblée » ; lectures de
//! `Project::seam_junction_report` / `seam_assembled_view`, rien n'est stocké.
//!
//! Limites : un bord = suite continue de tracés du contour dessiné (liste ou clic) ; le rapport
//! n'est pas exporté.

use crate::a11y::ComboExt;
use crate::app::App;
use crate::canvas::{label, polyline, stroke, Xf};
use crate::editor::{Editor, Target};
use crate::palette::pal;
use eframe::egui::{self, Align2, FontId, Pos2, RichText, Sense, Shape};
use pf_core::units::parse_length_display;
use pf_core::{
    Command, EntityId, NotchCheckReading, NotchMatch, PieceId, Project, SeamIssue, SeamPairId,
    SeamPairReading, SeamSide, SeamStatus, SeamWhich, NOTCH_SHIFT_RANGE_MM,
};
use pf_i18n::{tr, tr_args};

/// Un côté du formulaire : pièce, premier tracé, nombre de tracés consécutifs.
#[derive(Debug, Clone, PartialEq)]
pub struct SeamSideUi {
    pub piece: Option<PieceId>,
    pub edge: Option<EntityId>,
    pub count: String,
}

impl Default for SeamSideUi {
    fn default() -> Self {
        SeamSideUi {
            piece: None,
            edge: None,
            count: "1".into(),
        }
    }
}

/// État de la fenêtre « Coutures… » (interface seulement, jamais enregistré).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SeamUi {
    pub open: bool,
    /// 0 = aucun clic attendu ; 1 = bord A sur le plan ; 2 = bord B sur le plan.
    pub picking: u8,
    pub a: SeamSideUi,
    pub b: SeamSideUi,
    /// Embu attendu et tolérance du nouveau couple (vide = aucun / tolérance par défaut).
    pub embu: String,
    pub tol: String,
    /// Couple choisi dans le tableau, et ses valeurs en cours de modification.
    pub selected: Option<SeamPairId>,
    pub sel_embu: String,
    pub sel_tol: String,
    /// Marche de pièces : sens B vers A (faux = A vers B).
    pub march_from_b: bool,
    /// Section « Contrôle des crans » dépliée (repliée par défaut).
    pub check_open: bool,
    /// Section « Raccords » dépliée (repliée par défaut), fenêtre « Vue assemblée » ouverte et
    /// jonction choisie dans le tableau (US-08.3).
    pub join_open: bool,
    pub join_view: bool,
    pub join_sel: Option<(SeamPairId, pf_core::JunctionEnd)>,
    loaded: Option<SeamPairId>,
}

/// Pièces proposées : celles qui ont un contour.
fn candidates(project: &Project) -> Vec<(PieceId, String)> {
    project
        .pieces
        .iter()
        .filter(|p| !p.outline.is_empty())
        .map(|p| (p.id, p.name.clone()))
        .collect()
}

fn edges_of(project: &Project, piece: PieceId) -> Vec<(EntityId, String)> {
    let (Ok(p), Ok(sheet)) = (project.piece(piece), project.piece_sheet(piece)) else {
        return Vec::new();
    };
    p.outline
        .iter()
        .map(|e| (e.entity, crate::text::entity_name(sheet, e.entity)))
        .collect()
}

fn piece_name(project: &Project, piece: PieceId) -> String {
    project
        .piece(piece)
        .map(|p| p.name.clone())
        .unwrap_or_else(|_| "?".into())
}

fn num(v: Option<f64>) -> String {
    v.map_or_else(|| "-".into(), |v| pf_core::units::len_number(v, 1))
}

/// Vide = `None` ; sinon une longueur lisible.
fn parse_optional(text: &str) -> Result<Option<f64>, String> {
    let t = text.trim();
    if t.is_empty() {
        return Ok(None);
    }
    parse_length_display(t)
        .map(Some)
        .map_err(|_| tr("status.seams.bad_values"))
}

fn march_side(from_b: bool) -> SeamWhich {
    if from_b {
        SeamWhich::B
    } else {
        SeamWhich::A
    }
}

fn side_letter(side: SeamWhich) -> &'static str {
    match side {
        SeamWhich::A => "A",
        SeamWhich::B => "B",
    }
}

fn issue_text(reason: SeamIssue) -> String {
    tr(match reason {
        SeamIssue::PieceMissing => "seams.reason.piece_missing",
        SeamIssue::OutlineOpen => "seams.reason.outline_open",
        SeamIssue::EdgeNotInOutline => "seams.reason.edge_not_in_outline",
        SeamIssue::Degenerate => "seams.reason.degenerate",
    })
}

fn status_text(status: &SeamStatus) -> String {
    tr(match status {
        SeamStatus::Within => "seams.status.within",
        SeamStatus::OutOfTolerance => "seams.status.out",
        SeamStatus::NotComputable { .. } => "seams.status.na",
    })
}

/// Icône de statut dessinée (formes différentes : cercle coché, triangle, cercle interrogé) ;
/// elle accompagne toujours le texte du statut.
fn status_icon(ui: &mut egui::Ui, status: &SeamStatus) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), Sense::hover());
    let p = ui.painter();
    let c = rect.center();
    let at = |x: f32, y: f32| c + egui::vec2(x, y);
    match status {
        SeamStatus::Within => {
            let col = pal().internal;
            p.circle_stroke(c, 7.0, stroke(1.6, col));
            p.add(Shape::line(
                vec![at(-3.5, 0.5), at(-1.0, 3.0), at(3.5, -2.5)],
                stroke(1.8, col),
            ));
        }
        SeamStatus::OutOfTolerance => {
            let col = pal().error;
            p.add(Shape::closed_line(
                vec![at(0.0, -7.0), at(7.5, 6.0), at(-7.5, 6.0)],
                stroke(1.6, col),
            ));
            p.line_segment([at(0.0, -2.0), at(0.0, 1.5)], stroke(1.6, col));
            p.circle_filled(at(0.0, 3.8), 1.0, col);
        }
        SeamStatus::NotComputable { .. } => {
            let col = pal().muted;
            p.circle_stroke(c, 7.0, stroke(1.6, col));
            p.text(
                c,
                Align2::CENTER_CENTER,
                "?",
                FontId::proportional(11.0),
                col,
            );
        }
    }
}

/// Anomalies listées au plus (les autres sont comptées).
const ISSUES_SHOWN: usize = 6;

/// Icône de contrôle des crans : cercle coché (apparié), cercle barré (manquant), cercle à deux
/// traits décalés (décalé). Elle accompagne toujours le texte.
fn check_icon(ui: &mut egui::Ui, m: NotchMatch) {
    if m == NotchMatch::Matched {
        status_icon(ui, &SeamStatus::Within);
        return;
    }
    let (rect, _) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), Sense::hover());
    let p = ui.painter();
    let c = rect.center();
    let at = |x: f32, y: f32| c + egui::vec2(x, y);
    if m == NotchMatch::Missing {
        let col = pal().error;
        p.circle_stroke(c, 7.0, stroke(1.6, col));
        p.line_segment([at(-3.0, -3.0), at(3.0, 3.0)], stroke(1.8, col));
        p.line_segment([at(-3.0, 3.0), at(3.0, -3.0)], stroke(1.8, col));
    } else {
        let col = pal().accent_text;
        p.circle_stroke(c, 7.0, stroke(1.6, col));
        p.line_segment([at(-2.5, -4.0), at(-2.5, 0.5)], stroke(1.8, col));
        p.line_segment([at(2.5, -0.5), at(2.5, 4.0)], stroke(1.8, col));
    }
}

/// Lecture du contrôle des crans d'un couple (calculée à la demande, jamais stockée).
/// Renvoie vrai quand « Reporter les manquants » est cliqué.
fn notch_check_ui(ui: &mut egui::Ui, r: &NotchCheckReading) -> bool {
    if let Some((side, reason)) = r.not_computable {
        crate::a11y::label(
            ui,
            RichText::new(tr_args(
                "seams.reason.line",
                &[
                    ("n", &r.number.to_string()),
                    ("side", side_letter(side)),
                    ("reason", &issue_text(reason)),
                ],
            ))
            .small()
            .color(pal().muted),
        );
        return false;
    }
    let missing = r.count_status(NotchMatch::Missing);
    let shifted = r.count_status(NotchMatch::Shifted);
    if r.items.is_empty() {
        crate::a11y::label(
            ui,
            RichText::new(tr("seams.check.empty"))
                .small()
                .color(pal().muted),
        );
    } else {
        crate::a11y::label(
            ui,
            RichText::new(tr_args(
                "seams.check.summary",
                &[
                    ("n", &r.items.len().to_string()),
                    ("a", &r.count(SeamWhich::A).to_string()),
                    ("b", &r.count(SeamWhich::B).to_string()),
                    ("m", &missing.to_string()),
                    ("s", &shifted.to_string()),
                ],
            ))
            .small(),
        );
        if missing + shifted == 0 {
            ui.horizontal(|ui| {
                check_icon(ui, NotchMatch::Matched);
                crate::a11y::label(ui, RichText::new(tr("seams.check.ok")).small());
            });
        }
    }
    let mut go = false;
    ui.add_enabled_ui(missing > 0, |ui| {
        if ui.button(tr("seams.check.button")).clicked() {
            go = true;
        }
    });
    let issues: Vec<_> = r
        .items
        .iter()
        .filter(|i| i.status != NotchMatch::Matched)
        .collect();
    for (k, i) in issues.iter().take(ISSUES_SHOWN).enumerate() {
        let gap = i
            .gap_mm
            .map_or_else(String::new, |g| pf_core::units::len_number(g, 2));
        let found = i
            .found_mm
            .map_or_else(String::new, |f| pf_core::units::len_number(f, 2));
        let text = tr_args(
            if i.status == NotchMatch::Missing {
                "seams.check.missing"
            } else {
                "seams.check.shifted"
            },
            &[
                ("n", &(k + 1).to_string()),
                ("side", side_letter(i.side)),
                ("pos", &pf_core::units::len_number(i.position_mm, 2)),
                ("exp", &pf_core::units::len_number(i.expected_mm, 2)),
                ("gap", &gap),
                ("found", &found),
            ],
        );
        ui.horizontal_wrapped(|ui| {
            check_icon(ui, i.status);
            crate::a11y::label(ui, RichText::new(text).small());
        });
    }
    if issues.len() > ISSUES_SHOWN {
        crate::a11y::label(
            ui,
            RichText::new(tr_args(
                "seams.check.more",
                &[("n", &(issues.len() - ISSUES_SHOWN).to_string())],
            ))
            .small()
            .color(pal().muted),
        );
    }
    crate::a11y::label(
        ui,
        RichText::new(pf_i18n::tr_len(
            "seams.check.note",
            &[
                ("tol", &pf_core::units::len_number(r.tolerance_mm, 2)),
                (
                    "range",
                    &pf_core::units::len_number(NOTCH_SHIFT_RANGE_MM.max(r.tolerance_mm), 0),
                ),
            ],
        ))
        .small()
        .color(pal().muted),
    );
    go
}

impl SeamSideUi {
    /// Côté décrit par le formulaire (suite de tracés consécutifs à partir du premier).
    fn side(&self, project: &Project) -> Result<SeamSide, String> {
        let (Some(piece), Some(edge)) = (self.piece, self.edge) else {
            return Err(tr("status.seams.no_piece"));
        };
        let count: usize = self
            .count
            .trim()
            .parse()
            .map_err(|_| tr("status.seams.bad_values"))?;
        project
            .seam_side_range(piece, edge, count)
            .map_err(|e| crate::text::core_error(&e))
    }

    /// Pièce supprimée ou tracé sorti du contour : le choix est remis à un état valide.
    fn fix(&mut self, project: &Project) {
        if self.piece.is_some_and(|p| project.piece(p).is_err()) {
            *self = SeamSideUi::default();
        }
        let Some(piece) = self.piece else {
            return;
        };
        let edges = edges_of(project, piece);
        if !self.edge.is_some_and(|e| edges.iter().any(|x| x.0 == e)) {
            self.edge = edges.first().map(|x| x.0);
        }
    }
}

impl Editor {
    fn seam_fix(&mut self) {
        let project = self.doc.project().clone();
        self.seam.a.fix(&project);
        self.seam.b.fix(&project);
        if self
            .seam
            .selected
            .is_some_and(|id| !project.seams.iter().any(|s| s.id == id))
        {
            self.seam.selected = None;
            self.seam.loaded = None;
        }
        if self
            .seam
            .join_sel
            .is_some_and(|(id, _)| !project.seams.iter().any(|s| s.id == id))
        {
            self.seam.join_sel = None;
        }
    }

    /// Clic sur un bord de pièce pendant « Choisir sur le plan » : A puis B.
    pub fn seam_click(&mut self, world: pf_geometry::PointMm, tol_mm: f64) {
        let Some(Target::Entity(entity)) = self.hit_test(world, tol_mm) else {
            self.error(tr("status.seams.not_edge"));
            return;
        };
        let owner = self
            .doc
            .project()
            .pieces_on(self.sheet)
            .into_iter()
            .find(|p| p.outline.iter().any(|e| e.entity == entity))
            .map(|p| p.id);
        let Some(piece) = owner else {
            self.error(tr("status.seams.not_edge"));
            return;
        };
        let side = SeamSideUi {
            piece: Some(piece),
            edge: Some(entity),
            count: "1".into(),
        };
        if self.seam.picking == 1 {
            self.seam.a = side;
            self.seam.picking = 2;
            self.info(tr("status.seams.pick_b"));
        } else {
            self.seam.b = side;
            self.seam.picking = 0;
            self.info(tr("status.seams.picked"));
        }
    }

    pub fn cancel_seam_pick(&mut self) -> bool {
        if self.seam.picking == 0 {
            return false;
        }
        self.seam.picking = 0;
        self.info(tr("status.cancelled"));
        true
    }

    /// Déclare le couple décrit par le formulaire : une étape annulable, refus sans trace.
    pub fn seam_declare(&mut self) {
        let project = self.doc.project();
        let built = self.seam.a.side(project).and_then(|a| {
            let b = self.seam.b.side(project)?;
            Ok(Command::AddSeamPair {
                a,
                b,
                embu_mm: parse_optional(&self.seam.embu)?,
                tolerance_mm: parse_optional(&self.seam.tol)?,
            })
        });
        let cmd = match built {
            Ok(c) => c,
            Err(e) => {
                self.error(e);
                return;
            }
        };
        let Some(out) = self.exec(cmd) else {
            return;
        };
        let n = self
            .doc
            .project()
            .seams
            .iter()
            .position(|s| out.seam_pairs.contains(&s.id))
            .map_or(0, |i| i + 1);
        self.seam.selected = out.seam_pairs.first().copied();
        self.seam.loaded = None;
        self.info(tr_args("status.seams.done", &[("n", &n.to_string())]));
    }

    /// Remplace l'embu attendu et la tolérance du couple choisi (les côtés ne changent pas).
    pub fn seam_update(&mut self) {
        let Some(id) = self.seam.selected else {
            return;
        };
        let Some(pair) = self
            .doc
            .project()
            .seams
            .iter()
            .find(|s| s.id == id)
            .cloned()
        else {
            return;
        };
        let values = parse_optional(&self.seam.sel_embu)
            .and_then(|e| parse_optional(&self.seam.sel_tol).map(|t| (e, t)));
        let (embu_mm, tolerance_mm) = match values {
            Ok(v) => v,
            Err(e) => {
                self.error(e);
                return;
            }
        };
        if self
            .exec(Command::SetSeamPair {
                id,
                a: pair.a,
                b: pair.b,
                embu_mm,
                tolerance_mm,
            })
            .is_some()
        {
            self.info(tr("status.seams.updated"));
        }
    }

    /// Marche de pièces : reporte les crans du côté source du couple choisi (une étape).
    pub fn seam_march(&mut self) {
        let Some(pair) = self.seam.selected else {
            return;
        };
        let from = march_side(self.seam.march_from_b);
        if let Some(o) = self.exec(Command::MarchNotches {
            pair,
            from,
            which: None,
        }) {
            let mut text = tr_args(
                "status.seams.marched",
                &[("n", &o.notches.len().to_string())],
            );
            if !o.notes.is_empty() {
                text.push(' ');
                text.push_str(&o.notes.join(" "));
            }
            self.info(text);
        }
    }

    /// Reporte les crans manquants du couple choisi (des deux côtés s'il y en a) : une étape.
    pub fn seam_march_missing(&mut self) {
        let Some(pair) = self.seam.selected else {
            return;
        };
        let report = self.doc.project().notch_check_report();
        let mut cmds: Vec<Command> = Vec::new();
        if let Some(r) = report.iter().find(|r| r.id == pair) {
            for from in [SeamWhich::A, SeamWhich::B] {
                let ids = r.missing(from);
                if !ids.is_empty() {
                    cmds.push(Command::MarchNotches {
                        pair,
                        from,
                        which: Some(ids),
                    });
                }
            }
        }
        let cmd = match cmds.len() {
            0 => {
                self.error(tr("status.seams.nothing_missing"));
                return;
            }
            1 => cmds.remove(0),
            _ => Command::Batch { commands: cmds },
        };
        if let Some(o) = self.exec(cmd) {
            self.info(tr_args(
                "status.seams.marched",
                &[("n", &o.notches.len().to_string())],
            ));
        }
    }

    pub fn seam_remove(&mut self) {
        let Some(id) = self.seam.selected else {
            return;
        };
        if self.exec(Command::RemoveSeamPair { id }).is_some() {
            self.seam.selected = None;
            self.seam.loaded = None;
            self.info(tr("status.seams.removed"));
        }
    }
}

/// Couple de démonstration : pièce et tracé de chaque côté, embu attendu, tolérance.
type Couple = (
    PieceId,
    Option<EntityId>,
    PieceId,
    Option<EntityId>,
    Option<f64>,
    Option<f64>,
);

/// Projet de démonstration (captures `--etape coutures`) : quatre pièces rectangulaires et trois
/// couples (dans la tolérance, hors tolérance, non calculable car le contour d'une pièce est ouvert).
pub fn demo_seam_project() -> Result<Project, pf_core::CoreError> {
    use pf_core::{PieceEdge, PieceSource, PointRef};
    use pf_geometry::PointMm;
    let mut p = Project::new("Coutures");
    let sheet = p.sheets[0].id;
    let piece = |p: &mut Project, name: &str, x: f64, w: f64, h: f64| {
        let o = p.apply(Command::AddRectangle {
            sheet,
            corner: PointRef::New(PointMm::new(x, 0.0)),
            width_mm: w,
            height_mm: h,
        })?;
        Ok::<_, pf_core::CoreError>(
            p.apply(Command::ExtractPiece {
                sheet,
                source: PieceSource::Paths {
                    entities: o.entities,
                },
                name: name.into(),
                detached: false,
            })?
            .pieces[0],
        )
    };
    let devant = piece(&mut p, "Devant", 0.0, 205.0, 100.0)?;
    let dos = piece(&mut p, "Dos", 260.0, 200.0, 100.0)?;
    let manche = piece(&mut p, "Manche", 520.0, 150.0, 80.0)?;
    let poche = piece(&mut p, "Poche", 720.0, 60.0, 40.0)?;
    // Tracé de la pièce dont les deux extrémités vérifient `f`.
    let find = |p: &Project, piece: PieceId, f: &dyn Fn(PointMm, PointMm) -> bool| {
        let sh = p.piece_sheet(piece).ok()?;
        p.piece(piece)
            .ok()?
            .outline
            .iter()
            .map(|e| e.entity)
            .find(|e| sh.segment_ends(*e).is_ok_and(|(a, b)| f(a, b)))
    };
    let flat = |y: f64| move |a: PointMm, b: PointMm| a.y == y && b.y == y;
    let upright = |x: f64| move |a: PointMm, b: PointMm| a.x == x && b.x == x;
    let side = |p: &Project, piece: PieceId, e: Option<EntityId>| {
        p.seam_side_range(
            piece,
            e.ok_or_else(|| pf_core::CoreError::Invalid("edge not found".into()))?,
            1,
        )
    };
    let couples: [Couple; 3] = [
        (
            devant,
            find(&p, devant, &flat(0.0)),
            dos,
            find(&p, dos, &flat(0.0)),
            Some(5.0),
            None,
        ),
        (
            dos,
            find(&p, dos, &upright(460.0)),
            manche,
            find(&p, manche, &upright(520.0)),
            None,
            Some(2.0),
        ),
        (
            dos,
            find(&p, dos, &flat(100.0)),
            poche,
            find(&p, poche, &flat(40.0)),
            None,
            None,
        ),
    ];
    for (pa, ea, pb, eb, embu_mm, tolerance_mm) in couples {
        let (a, b) = (side(&p, pa, ea)?, side(&p, pb, eb)?);
        p.apply(Command::AddSeamPair {
            a,
            b,
            embu_mm,
            tolerance_mm,
        })?;
    }
    // Contour de la poche ouvert (un côté non cité retiré) : son couple n'est plus calculable.
    let drop = find(&p, poche, &upright(720.0));
    let keep: Vec<PieceEdge> = p
        .piece(poche)?
        .outline
        .iter()
        .copied()
        .filter(|e| Some(e.entity) != drop)
        .collect();
    p.apply(Command::SetPieceOutline {
        piece: poche,
        outline: keep,
        grainline: None,
    })?;
    Ok(p)
}

/// Projet de démonstration de la marche (`--etape marche`) : celui des coutures, avec trois crans
/// sur le bord du bas de « Devant » (couple 1 : 205 mm contre 200 mm).
pub fn demo_march_project() -> Result<Project, pf_core::CoreError> {
    let mut p = demo_seam_project()?;
    let pair = p.seams[0].clone();
    let sheet = p.piece_sheet(pair.a.piece)?.id;
    for d in [50.0, 100.0, 150.0] {
        p.apply(Command::AddNotch {
            sheet,
            entity: pair.a.edges[0],
            distance_mm: d,
            side: pf_geometry::Side::Left,
            length_mm: 6.0,
            style: pf_core::NotchStyle::Single,
        })?;
    }
    Ok(p)
}

/// Projet de démonstration du contrôle des crans (`--etape crans-couture`) : celui de la marche
/// (trois crans sur le bord de « Devant »), avec sur « Dos » un vis-à-vis exact (cran 1), un
/// vis-à-vis décalé de 2,5 mm (cran 2) et aucun vis-à-vis pour le cran 3.
pub fn demo_notch_check_project() -> Result<Project, pf_core::CoreError> {
    let mut p = demo_march_project()?;
    let pair = p.seams[0].id;
    let sheet = p.piece_sheet(p.seams[0].b.piece)?.id;
    let v = p.march_preview(pair, SeamWhich::A, None)?;
    for (it, shift) in v.items.iter().zip([0.0, 2.5]) {
        p.apply(Command::AddNotch {
            sheet,
            entity: it.entity,
            distance_mm: it.distance_mm + shift,
            side: it.side,
            length_mm: 6.0,
            style: pf_core::NotchStyle::Single,
        })?;
    }
    Ok(p)
}

/// Projet de démonstration des raccords (`--etape raccords`) : couple 1 = « Devant » (angle de
/// 100° au début de la couture) contre « Dos » (70°) : somme 170°, écart −10°, cassé (creux) ;
/// couple 2 = « Manche » (90°) contre « Poche » (110°) : somme 200°, écart +20°, cassé (bosse).
/// Les fins de couture sont à 90° + 90° (lisses).
pub fn demo_junction_project() -> Result<Project, pf_core::CoreError> {
    use pf_core::{PieceSource, PointRef};
    use pf_geometry::PointMm;
    let mut p = Project::new("Raccords");
    let sheet = p.sheets[0].id;
    let (c10, s10) = (10f64.to_radians().cos(), 10f64.to_radians().sin());
    let (c20, s20) = (20f64.to_radians().cos(), 20f64.to_radians().sin());
    // (nom, sommets anti-horaires, indice du tracé de couture : sommet k vers k + 1)
    let shapes: [(&str, [(f64, f64); 4]); 4] = [
        (
            "Devant",
            [
                (100.0 - 100.0 * c10, -100.0 * s10),
                (100.0, 0.0),
                (100.0, 50.0),
                (0.0, 50.0),
            ],
        ),
        (
            "Dos",
            [
                (300.0, 50.0),
                (300.0, 0.0),
                (300.0 + 100.0 * c20, 100.0 * s20),
                (300.0 + 100.0 * c20, 50.0),
            ],
        ),
        (
            "Manche",
            [(520.0, 0.0), (620.0, 0.0), (620.0, 50.0), (520.0, 50.0)],
        ),
        (
            "Poche",
            [
                (720.0, 50.0),
                (720.0, 0.0),
                (720.0 + 100.0 * c20, -100.0 * s20),
                (720.0 + 100.0 * c20, 50.0),
            ],
        ),
    ];
    let mut made = Vec::new();
    for (name, v) in shapes {
        let mut ents = Vec::new();
        for k in 0..4 {
            let (a, b) = (v[k], v[(k + 1) % 4]);
            ents.push(
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
                    entities: ents.clone(),
                },
                name: name.into(),
                detached: false,
            })?
            .pieces[0];
        made.push((piece, ents));
    }
    // Côté A : tracé du sommet 1 au sommet 2 (montant) ; côté B : tracé du sommet 0 au sommet 1.
    for ((pa, ea), (pb, eb)) in [(&made[0], &made[1]), (&made[2], &made[3])] {
        let (a, b) = (
            p.seam_side_range(*pa, ea[1], 1)?,
            p.seam_side_range(*pb, eb[0], 1)?,
        );
        p.apply(Command::AddSeamPair {
            a,
            b,
            embu_mm: None,
            tolerance_mm: None,
        })?;
    }
    Ok(p)
}

impl App {
    /// Ouvre la fenêtre « Coutures… » ; les deux côtés partent des premières pièces de la feuille.
    pub fn open_seams(&mut self) {
        self.editor.seam.open = true;
        self.editor.seam.picking = 0;
        let project = self.editor.project();
        let list = candidates(project);
        let on_sheet: Vec<PieceId> = project
            .pieces_on(self.editor.sheet)
            .into_iter()
            .map(|p| p.id)
            .filter(|id| list.iter().any(|c| c.0 == *id))
            .collect();
        let pick = |i: usize| on_sheet.get(i).copied().or(list.get(i).map(|c| c.0));
        let (pa, pb) = (pick(0), pick(1).or(pick(0)));
        let ui = &mut self.editor.seam;
        if ui.a.piece.is_none() {
            ui.a.piece = pa;
        }
        if ui.b.piece.is_none() {
            ui.b.piece = pb;
        }
        self.editor.seam_fix();
    }

    /// Les deux bords de chaque couple de la feuille affichée, surlignés avec « couture n ».
    pub(crate) fn paint_seams(&self, painter: &egui::Painter, xf: &Xf) {
        if !self.editor.seam.open {
            return;
        }
        let project = self.editor.project();
        let sheet = self.editor.sheet();
        let on_sheet: Vec<PieceId> = project
            .pieces_on(self.editor.sheet)
            .into_iter()
            .map(|p| p.id)
            .collect();
        let color = pal().preview;
        for (i, pair) in project.seams.iter().enumerate() {
            let chosen = self.editor.seam.selected == Some(pair.id);
            for (side, is_a) in [(&pair.a, true), (&pair.b, false)] {
                if !on_sheet.contains(&side.piece) {
                    continue;
                }
                let mut first_mid: Option<Pos2> = None;
                for e in &side.edges {
                    let Ok(shape) = sheet.shape(*e) else {
                        continue;
                    };
                    let pts: Vec<Pos2> = shape.sample(64).into_iter().map(|p| xf.s(p)).collect();
                    if pts.len() < 2 {
                        continue;
                    }
                    first_mid.get_or_insert(pts[pts.len() / 2]);
                    painter.add(Shape::line(
                        pts.clone(),
                        stroke(if chosen { 9.0 } else { 6.0 }, color.gamma_multiply(0.30)),
                    ));
                    // A en trait plein, B en tirets : jamais la couleur seule.
                    let (dash, gap): (&[f32], &[f32]) =
                        if is_a { (&[], &[]) } else { (&[9.0], &[5.0]) };
                    polyline(painter, pts, color, 2.4, dash, gap);
                }
                if let Some(mid) = first_mid {
                    let (off, anchor) = if is_a {
                        (-14.0, Align2::CENTER_BOTTOM)
                    } else {
                        (14.0, Align2::CENTER_TOP)
                    };
                    label(
                        painter,
                        mid + egui::vec2(0.0, off),
                        anchor,
                        &tr_args(
                            if is_a { "seams.mark.a" } else { "seams.mark.b" },
                            &[("n", &(i + 1).to_string())],
                        ),
                        color,
                    );
                }
            }
        }
        // Contrôle des crans : chaque cran manquant est entouré (tirets) là où son vis-à-vis est
        // attendu, avec un libellé numéroté comme la liste.
        if self.editor.seam.check_open {
            if let Some(pair) = self
                .editor
                .seam
                .selected
                .and_then(|id| project.seams.iter().find(|s| s.id == id))
            {
                let reading = project
                    .notch_check_report()
                    .into_iter()
                    .find(|r| r.id == pair.id);
                let issues: Vec<_> = reading
                    .iter()
                    .flat_map(|r| r.items.iter())
                    .filter(|i| i.status != NotchMatch::Matched)
                    .take(ISSUES_SHOWN)
                    .collect();
                for (k, i) in issues.iter().enumerate() {
                    if i.status != NotchMatch::Missing {
                        continue;
                    }
                    let target = if i.side == SeamWhich::A {
                        pair.b.piece
                    } else {
                        pair.a.piece
                    };
                    let Ok(v) = project.march_preview(pair.id, i.side, Some(&[i.notch])) else {
                        continue;
                    };
                    let Some(it) = v.items.first().filter(|_| on_sheet.contains(&target)) else {
                        continue;
                    };
                    let c = xf.s(it.point);
                    let col = pal().error;
                    polyline(
                        painter,
                        (0..=24)
                            .map(|t| {
                                let a = std::f32::consts::TAU * t as f32 / 24.0;
                                c + egui::vec2(a.cos(), a.sin()) * 9.0
                            })
                            .collect(),
                        col,
                        1.8,
                        &[4.0],
                        &[3.0],
                    );
                    label(
                        painter,
                        c + egui::vec2(0.0, 24.0),
                        Align2::CENTER_TOP,
                        &tr_args("seams.check.mark", &[("n", &(k + 1).to_string())]),
                        col,
                    );
                }
            }
        }
        // Raccord choisi dans le tableau (US-08.3) : cercle à chaque sommet cousu.
        if self.editor.seam.join_open {
            if let Some((id, end)) = self.editor.seam.join_sel {
                let row = project
                    .seam_junction_report()
                    .into_iter()
                    .find(|r| r.pair == id && r.end == end);
                if let (Some(r), Some(pair)) = (row, project.seams.iter().find(|s| s.id == id)) {
                    crate::junctions::paint_junction(
                        painter,
                        xf,
                        &r,
                        on_sheet.contains(&pair.a.piece),
                        on_sheet.contains(&pair.b.piece),
                    );
                }
            }
        }
        // Repères de l'aperçu de la marche : un par cran qui serait posé sur la pièce cible.
        let from_b = self.editor.seam.march_from_b;
        if let Some(pair) = self
            .editor
            .seam
            .selected
            .and_then(|id| project.seams.iter().find(|s| s.id == id))
        {
            let target = if from_b { pair.a.piece } else { pair.b.piece };
            if let (true, Ok(v)) = (
                on_sheet.contains(&target),
                project.march_preview(pair.id, march_side(from_b), None),
            ) {
                for (k, it) in v.items.iter().enumerate() {
                    let c = xf.s(it.point);
                    let col = if it.duplicate { pal().muted } else { color };
                    painter.circle_stroke(c, 5.0, stroke(1.6, col));
                    painter.line_segment(
                        [c + egui::vec2(-8.0, 0.0), c + egui::vec2(8.0, 0.0)],
                        stroke(1.2, col),
                    );
                    label(
                        painter,
                        c + egui::vec2(0.0, 10.0),
                        Align2::CENTER_TOP,
                        &(k + 1).to_string(),
                        col,
                    );
                }
            }
        }
    }

    pub(crate) fn seams_window(&mut self, ctx: &egui::Context) {
        if !self.editor.seam.open {
            return;
        }
        self.editor.seam_fix();
        let project = self.editor.project().clone();
        let report = project.seam_pair_report();
        let check = if self.editor.seam.check_open {
            project.notch_check_report()
        } else {
            Vec::new()
        };
        let joins = if self.editor.seam.join_open {
            project.seam_junction_report()
        } else {
            Vec::new()
        };
        let pieces = candidates(&project);
        let (mut open, mut action) = (true, 0u8);
        let mut clicked: Option<SeamPairId> = None;
        let picking = self.editor.seam.picking;
        let selected = self.editor.seam.selected;
        let slot = crate::dock::slot(ctx, &self.dock, crate::dock::ToolWin::Seams);
        let width = self.dock.content_width(crate::dock::ToolWin::Seams);
        egui::Window::new(tr("seams.title"))
            .id(egui::Id::new("seams_window"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .fixed_pos(slot.pos)
            .vscroll(true)
            .max_height(slot.max_height)
            .show(ctx, |ui| {
                ui.set_width(width);
                crate::a11y::label(
                    ui,
                    RichText::new(tr("seams.intro")).small().color(pal().muted),
                );
                ui.separator();
                seams_table(ui, &project, &report, selected, &mut clicked);
                for r in &report {
                    if let SeamStatus::NotComputable { side, reason } = r.status {
                        crate::a11y::label(
                            ui,
                            RichText::new(tr_args(
                                "seams.reason.line",
                                &[
                                    ("n", &r.number.to_string()),
                                    ("side", side_letter(side)),
                                    ("reason", &issue_text(reason)),
                                ],
                            ))
                            .small()
                            .color(pal().muted),
                        );
                    }
                }
                let st = &mut self.editor.seam;
                if let Some(id) = clicked {
                    st.selected = Some(id);
                }
                // Raccords (US-08.3) : section repliée, tout de suite sous le tableau des couples.
                if crate::junctions::junction_section(ui, &project, &joins, st, &mut clicked) {
                    st.join_view = true;
                }
                if let Some(id) = clicked {
                    st.selected = Some(id);
                }
                // Couple choisi : modifier l'embu attendu et la tolérance, ou le retirer.
                let chosen = st
                    .selected
                    .and_then(|id| project.seams.iter().position(|s| s.id == id));
                if let Some(index) = chosen {
                    let pair = &project.seams[index];
                    if st.loaded != Some(pair.id) {
                        st.loaded = Some(pair.id);
                        st.sel_embu = pair
                            .embu_mm
                            .map(pf_core::units::field_text)
                            .unwrap_or_default();
                        st.sel_tol = pair
                            .tolerance_mm
                            .map(pf_core::units::field_text)
                            .unwrap_or_default();
                    }
                    ui.separator();
                    ui.strong(tr_args(
                        "seams.selected",
                        &[("n", &(index + 1).to_string())],
                    ));
                    ui.horizontal(|ui| {
                        crate::a11y::label(ui, tr("seams.embu"));
                        crate::a11y::add_text(
                            ui,
                            egui::TextEdit::singleline(&mut st.sel_embu).desired_width(60.0),
                        );
                        crate::a11y::label(ui, tr("seams.tol"));
                        crate::a11y::add_text(
                            ui,
                            egui::TextEdit::singleline(&mut st.sel_tol)
                                .hint_text(tr("seams.tol.default"))
                                .desired_width(80.0),
                        );
                    });
                    ui.horizontal(|ui| {
                        if ui.button(tr("seams.update")).clicked() {
                            action = 2;
                        }
                        if ui.button(tr("seams.remove")).clicked() {
                            action = 3;
                        }
                    });
                    let this = check.iter().find(|r| r.id == pair.id);
                    let head = egui::CollapsingHeader::new(tr("seams.check.title"))
                        .id_salt("seams-check")
                        .open(Some(st.check_open))
                        .show(ui, |ui| {
                            if this.is_some_and(|r| notch_check_ui(ui, r)) {
                                action = 7;
                            }
                        });
                    if head.header_response.clicked() {
                        st.check_open = !st.check_open;
                    }
                    ui.separator();
                    ui.strong(tr("seams.march.title"));
                    let preview = project.march_preview(pair.id, march_side(st.march_from_b), None);
                    let ready = preview
                        .as_ref()
                        .is_ok_and(|v| v.items.iter().any(|i| !i.duplicate));
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut st.march_from_b, false, tr("seams.march.ab"));
                        ui.selectable_value(&mut st.march_from_b, true, tr("seams.march.ba"));
                        ui.add_enabled_ui(ready, |ui| {
                            if ui.button(tr("seams.march.button")).clicked() {
                                action = 6;
                            }
                        });
                    });
                    match preview {
                        Ok(v) => {
                            let sense = tr(if v.opposite {
                                "seams.march.opposite"
                            } else {
                                "seams.march.same"
                            });
                            crate::a11y::label(
                                ui,
                                RichText::new(pf_i18n::tr_len(
                                    "seams.march.summary",
                                    &[
                                        ("count", &v.items.len().to_string()),
                                        ("from", &pf_core::units::len_number(v.from_length_mm, 2)),
                                        ("to", &pf_core::units::len_number(v.to_length_mm, 2)),
                                        ("sense", &sense),
                                    ],
                                ))
                                .small(),
                            );
                            for (k, it) in v.items.iter().take(4).enumerate() {
                                let dup = if it.duplicate {
                                    tr("seams.march.dup")
                                } else {
                                    String::new()
                                };
                                crate::a11y::label(
                                    ui,
                                    RichText::new(pf_i18n::tr_len(
                                        "seams.march.line",
                                        &[
                                            ("n", &(k + 1).to_string()),
                                            ("src", &pf_core::units::len_number(it.source_mm, 2)),
                                            ("dst", &pf_core::units::len_number(it.target_mm, 2)),
                                            ("gap", &pf_core::units::len_number(it.gap_mm, 2)),
                                            ("dup", &dup),
                                        ],
                                    ))
                                    .small(),
                                );
                            }
                            if v.items.len() > 4 {
                                crate::a11y::label(
                                    ui,
                                    RichText::new(tr_args(
                                        "seams.march.more",
                                        &[("n", &(v.items.len() - 4).to_string())],
                                    ))
                                    .small()
                                    .color(pal().muted),
                                );
                            }
                        }
                        Err(_) => {
                            crate::a11y::label(
                                ui,
                                RichText::new(tr("seams.march.none"))
                                    .small()
                                    .color(pal().muted),
                            );
                        }
                    }
                    crate::a11y::label(
                        ui,
                        RichText::new(tr("seams.march.note"))
                            .small()
                            .color(pal().muted),
                    );
                }
                ui.separator();
                ui.strong(tr("seams.new"));
                for (which, is_a) in [("seams.side.a", true), ("seams.side.b", false)] {
                    let s = if is_a { &mut st.a } else { &mut st.b };
                    let salt = if is_a { "a" } else { "b" };
                    ui.horizontal(|ui| {
                        crate::a11y::label(ui, RichText::new(tr(which)).strong());
                        let current = s
                            .piece
                            .and_then(|p| pieces.iter().find(|c| c.0 == p))
                            .map(|c| c.1.clone())
                            .unwrap_or_else(|| tr("seams.choose"));
                        let before = s.piece;
                        egui::ComboBox::from_id_salt(format!("seams-piece-{salt}"))
                            .selected_text(current)
                            .width(110.0)
                            .show_ui_named(ui, |ui| {
                                for (id, name) in &pieces {
                                    ui.selectable_value(&mut s.piece, Some(*id), name);
                                }
                            });
                        if before != s.piece {
                            s.edge = None;
                            s.count = "1".into();
                        }
                        let edges = s.piece.map(|p| edges_of(&project, p)).unwrap_or_default();
                        let current = s
                            .edge
                            .and_then(|e| edges.iter().find(|x| x.0 == e))
                            .map(|x| x.1.clone())
                            .unwrap_or_else(|| tr("seams.choose"));
                        egui::ComboBox::from_id_salt(format!("seams-edge-{salt}"))
                            .selected_text(current)
                            .width(120.0)
                            .show_ui_named(ui, |ui| {
                                for (id, name) in &edges {
                                    ui.selectable_value(&mut s.edge, Some(*id), name);
                                }
                            });
                        crate::a11y::label(ui, tr("seams.count"));
                        crate::a11y::add_text(
                            ui,
                            egui::TextEdit::singleline(&mut s.count).desired_width(28.0),
                        );
                    });
                }
                ui.horizontal(|ui| {
                    crate::a11y::label(ui, tr("seams.embu"));
                    crate::a11y::add_text(
                        ui,
                        egui::TextEdit::singleline(&mut st.embu).desired_width(60.0),
                    );
                    crate::a11y::label(ui, tr("seams.tol"));
                    crate::a11y::add_text(
                        ui,
                        egui::TextEdit::singleline(&mut st.tol)
                            .hint_text(tr("seams.tol.default"))
                            .desired_width(80.0),
                    );
                });
                if picking > 0 {
                    crate::a11y::label(
                        ui,
                        RichText::new(tr(if picking == 1 {
                            "status.seams.pick_a"
                        } else {
                            "status.seams.pick_b"
                        }))
                        .color(pal().accent_text),
                    );
                }
                ui.horizontal(|ui| {
                    if ui.button(tr("seams.pick")).clicked() {
                        action = 4;
                    }
                    if ui.button(tr("seams.declare")).clicked() {
                        action = 1;
                    }
                    if ui.button(tr("seams.close")).clicked() {
                        action = 5;
                    }
                });
            });
        match action {
            1 => self.editor.seam_declare(),
            2 => self.editor.seam_update(),
            3 => self.editor.seam_remove(),
            6 => self.editor.seam_march(),
            7 => self.editor.seam_march_missing(),
            4 => {
                self.editor.seam.picking = 1;
                self.editor.info(tr("status.seams.pick_a"));
            }
            5 => open = false,
            _ => {}
        }
        if !open {
            self.editor.seam.open = false;
            self.editor.seam.picking = 0;
            self.editor.seam.join_view = false;
        }
        self.junction_view_window(ctx);
    }
}

/// Tableau des couples : une ligne par couple, lue sur le rapport (jamais stockée).
fn seams_table(
    ui: &mut egui::Ui,
    project: &Project,
    report: &[SeamPairReading],
    selected: Option<SeamPairId>,
    clicked: &mut Option<SeamPairId>,
) {
    if report.is_empty() {
        crate::a11y::label(ui, RichText::new(tr("seams.empty")).color(pal().muted));
        return;
    }
    egui::Grid::new("seams-grid")
        .num_columns(6)
        .spacing([8.0, 4.0])
        .striped(true)
        .show(ui, |ui| {
            for key in [
                "seams.col.n",
                "seams.col.pair",
                "seams.col.lengths",
                "seams.col.gap",
                "seams.col.embu",
                "seams.col.status",
            ] {
                crate::a11y::label(ui, RichText::new(tr(key)).small().strong());
            }
            ui.end_row();
            for r in report {
                let Some(pair) = project.seams.iter().find(|s| s.id == r.id) else {
                    continue;
                };
                if ui
                    .selectable_label(selected == Some(r.id), r.number.to_string())
                    .clicked()
                {
                    *clicked = Some(r.id);
                }
                let names = format!(
                    "{} / {}",
                    piece_name(project, pair.a.piece),
                    piece_name(project, pair.b.piece)
                );
                crate::a11y::label(ui, RichText::new(names).small());
                crate::a11y::label(
                    ui,
                    RichText::new(format!("{} / {}", num(r.length_a_mm), num(r.length_b_mm)))
                        .small(),
                );
                crate::a11y::label(ui, RichText::new(num(r.gap_mm)).small());
                crate::a11y::label(
                    ui,
                    RichText::new(format!(
                        "{} / {}",
                        pf_core::units::len_number(r.embu_expected_mm, 1),
                        num(r.embu_measured_mm)
                    ))
                    .small(),
                );
                ui.horizontal(|ui| {
                    status_icon(ui, &r.status);
                    crate::a11y::label(ui, RichText::new(status_text(&r.status)).small());
                });
                ui.end_row();
            }
        });
}
