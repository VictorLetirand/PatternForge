//! US-03.5 — fenêtre « Diagnostic » du dock (menu Affichage) : les **liens** du projet, relus à
//! chaque image par `Project::link_diagnostics` (calcul dérivé, jamais stocké, aucune `Command`).
//!
//! Liste groupée par gravité (erreurs, avertissements, informations) puis par type de lien, avec
//! des compteurs. Chaque ligne porte une icône **dessinée** et un texte : jamais la couleur seule.
//! Un clic sur une ligne change de feuille si besoin, sélectionne l'objet concerné et cadre la
//! vue (une variable ouvre la fenêtre « Variables »). Aucun calcul métier ici : tout vient du
//! cœur ; seul le texte est composé avec les catalogues de langue.
//!
//! Vocabulaire : « longueur imposée » ne désigne que les courbes de Bézier concernées ; les
//! formules et les constructions maintenues ne sont jamais des « contraintes non satisfaites ».

use crate::app::App;
use crate::dock::ToolWin;
use crate::text::entity_name;
use eframe::egui::{self, RichText};
use pf_core::input::format_number;
use pf_core::{
    DiagnosticKind, DiagnosticObject, DiagnosticSeverity, EntityKind, LinkDiagnostic, LinkType,
    ParamKind, PointId, Project, Sheet,
};
use pf_geometry::Bbox;
use pf_i18n::{tr, tr_args};
use pf_ui::components::{button, ButtonKind};

/// État de la fenêtre : rien à mémoriser (la liste est relue à chaque image).
#[derive(Debug, Clone, Copy, Default)]
pub struct DiagnosticDialog;

const SEVERITIES: [DiagnosticSeverity; 3] = [
    DiagnosticSeverity::Error,
    DiagnosticSeverity::Warning,
    DiagnosticSeverity::Info,
];

const LINK_TYPES: [LinkType; 6] = [
    LinkType::Formula,
    LinkType::KeptConstruction,
    LinkType::ImposedLength,
    LinkType::LengthEquality,
    LinkType::Variable,
    LinkType::PointGrading,
];

fn group_key(s: DiagnosticSeverity) -> &'static str {
    match s {
        DiagnosticSeverity::Error => "diag.group.error",
        DiagnosticSeverity::Warning => "diag.group.warning",
        DiagnosticSeverity::Info => "diag.group.info",
    }
}

fn type_label(l: LinkType) -> String {
    tr(match l {
        LinkType::Formula => "diag.type.formula",
        LinkType::KeptConstruction => "diag.type.kept",
        LinkType::ImposedLength => "diag.type.imposed",
        LinkType::LengthEquality => "diag.type.equality",
        LinkType::Variable => "diag.type.variable",
        LinkType::PointGrading => "diag.type.grading",
    })
}

fn param_label(p: ParamKind) -> String {
    tr(match p {
        ParamKind::Distance => "diag.param.distance",
        ParamKind::Angle => "diag.param.angle",
        ParamKind::Dx => "diag.param.dx",
        ParamKind::Dy => "diag.param.dy",
        ParamKind::AlongDistance => "diag.param.along_distance",
        ParamKind::BisectorDistance => "diag.param.bisector_distance",
        ParamKind::RadiusA => "diag.param.radius_a",
        ParamKind::RadiusB => "diag.param.radius_b",
        ParamKind::SegmentDistance => "diag.param.segment_distance",
        ParamKind::SegmentRatio => "diag.param.segment_ratio",
        ParamKind::CurveRatio => "diag.param.curve_ratio",
        ParamKind::DivisionRatio => "diag.param.division_ratio",
    })
}

fn point_name(sheet: &Sheet, id: PointId) -> String {
    format!("{} {}", tr("entity.point"), sheet.label(id))
}

fn signed(x: f64) -> String {
    if x >= 0.0 {
        format!("+{}", pf_core::units::len_number(x, 3))
    } else {
        pf_core::units::len_number(x, 3)
    }
}

/// Nom lisible de l'objet principal d'un constat (point, tracé ou variable).
fn primary_name(project: &Project, d: &LinkDiagnostic) -> String {
    let sheet = d.sheet.and_then(|s| project.sheet(s).ok());
    match (d.primary(), sheet) {
        (Some(DiagnosticObject::Point(p)), Some(s)) => point_name(s, *p),
        (Some(DiagnosticObject::Entity(e)), Some(s)) => entity_name(s, *e),
        (Some(DiagnosticObject::Variable(n)), _) => n.clone(),
        _ => type_label(d.link),
    }
}

/// Texte d'une ligne, selon la nature du constat et le type de lien.
fn row_text(project: &Project, d: &LinkDiagnostic) -> String {
    let sheet = d.sheet.and_then(|s| project.sheet(s).ok());
    let name = primary_name(project, d);
    match d.kind {
        DiagnosticKind::Broken => {
            let key = if d.link == LinkType::Variable {
                "diag.row.broken.variables"
            } else {
                "diag.row.broken"
            };
            let name = match (d.link, d.objects.is_empty()) {
                (LinkType::Formula, false) => format_param(&name, d),
                _ => name,
            };
            tr_args(key, &[("name", &name), ("detail", &d.detail)])
        }
        DiagnosticKind::LockedLayer => tr_args("diag.row.locked", &[("name", &name)]),
        DiagnosticKind::UnusedVariable => tr_args("diag.row.unused", &[("name", &d.text)]),
        DiagnosticKind::GradedAndFollowed => tr_args(
            match d.follower {
                Some(pf_core::GradingFollower::Length) => "diag.row.graded_length",
                _ => "diag.row.graded_kept",
            },
            &[("name", &name)],
        ),
        DiagnosticKind::Active => match d.link {
            LinkType::Formula => {
                let angle = d.param == Some(ParamKind::Angle);
                let ratio = d.param.is_some_and(crate::formulas::is_ratio);
                let value = d.value.map_or_else(
                    || "?".into(),
                    |v| {
                        if ratio {
                            format!("{} %", format_number(v * 100.0, 2))
                        } else if angle {
                            format!("{}°", format_number(v, 3))
                        } else {
                            pf_core::units::len_text(v, 3)
                        }
                    },
                );
                tr_args(
                    "diag.row.formula",
                    &[
                        ("name", &format_param(&name, d)),
                        ("expr", &d.text),
                        ("value", &value),
                    ],
                )
            }
            LinkType::KeptConstruction => {
                let points: Vec<String> = sheet
                    .map(|s| d.followed_points().map(|p| point_name(s, p)).collect())
                    .unwrap_or_default();
                tr_args(
                    "diag.row.kept",
                    &[("name", &name), ("points", &points.join(", "))],
                )
            }
            LinkType::ImposedLength if !d.text.is_empty() => pf_i18n::tr_len(
                "diag.row.imposed_formula",
                &[
                    ("name", &name),
                    ("expr", &d.text),
                    (
                        "value",
                        &pf_core::units::len_number(d.value.unwrap_or(f64::NAN), 3),
                    ),
                ],
            ),
            LinkType::ImposedLength => pf_i18n::tr_len(
                "diag.row.imposed",
                &[
                    ("name", &name),
                    (
                        "value",
                        &pf_core::units::len_number(d.value.unwrap_or(f64::NAN), 3),
                    ),
                ],
            ),
            LinkType::LengthEquality => {
                let reference = match (d.objects.get(1), sheet) {
                    (Some(DiagnosticObject::Entity(e)), Some(s)) => entity_name(s, *e),
                    _ => "?".into(),
                };
                pf_i18n::tr_len(
                    "diag.row.equality",
                    &[
                        ("name", &name),
                        ("reference", &reference),
                        ("offset", &signed(d.offset_mm.unwrap_or(0.0))),
                        (
                            "value",
                            &pf_core::units::len_number(d.value.unwrap_or(f64::NAN), 3),
                        ),
                    ],
                )
            }
            LinkType::Variable | LinkType::PointGrading => name,
        },
    }
}

/// « point C, distance » : le point et le paramètre d'un lien de formule.
fn format_param(point: &str, d: &LinkDiagnostic) -> String {
    match d.param {
        Some(p) => format!("{point}, {}", param_label(p)),
        None => point.to_string(),
    }
}

/// Icône de gravité dessinée : croix dans un disque (erreur), triangle à point d'exclamation
/// (avertissement), « i » dans un disque (information). Toujours accompagnée du texte du groupe.
pub(crate) fn severity_icon(ui: &mut egui::Ui, severity: DiagnosticSeverity) {
    let tokens = pf_ui::theme::tokens(ui.ctx());
    let color = match severity {
        DiagnosticSeverity::Error => tokens.danger,
        DiagnosticSeverity::Warning => tokens.warning,
        DiagnosticSeverity::Info => tokens.text_secondary,
    };
    let (rect, _) = ui.allocate_exact_size(egui::vec2(20.0, 18.0), egui::Sense::hover());
    let stroke = egui::Stroke::new(1.6_f32, color);
    let c = rect.center();
    let p = ui.painter();
    match severity {
        DiagnosticSeverity::Error => {
            p.circle_stroke(c, 7.0, stroke);
            p.line_segment(
                [c + egui::vec2(-3.0, -3.0), c + egui::vec2(3.0, 3.0)],
                stroke,
            );
            p.line_segment(
                [c + egui::vec2(-3.0, 3.0), c + egui::vec2(3.0, -3.0)],
                stroke,
            );
        }
        DiagnosticSeverity::Warning => {
            let top = c + egui::vec2(0.0, -7.0);
            let left = c + egui::vec2(-8.0, 6.0);
            let right = c + egui::vec2(8.0, 6.0);
            p.line_segment([top, left], stroke);
            p.line_segment([left, right], stroke);
            p.line_segment([right, top], stroke);
            p.line_segment(
                [c + egui::vec2(0.0, -2.5), c + egui::vec2(0.0, 1.5)],
                stroke,
            );
            p.circle_filled(c + egui::vec2(0.0, 4.0), 1.0, color);
        }
        DiagnosticSeverity::Info => {
            p.circle_stroke(c, 7.0, stroke);
            p.circle_filled(c + egui::vec2(0.0, -3.0), 1.0, color);
            p.line_segment(
                [c + egui::vec2(0.0, -0.5), c + egui::vec2(0.0, 3.5)],
                stroke,
            );
        }
    }
}

/// Emprise de l'objet principal, pour cadrer la vue.
pub(crate) fn object_bbox(sheet: &Sheet, object: &DiagnosticObject) -> Option<Bbox> {
    match object {
        DiagnosticObject::Point(p) => sheet.position(*p).ok().map(Bbox::from_point),
        DiagnosticObject::Entity(e) => match sheet.entity(*e).ok()?.kind {
            EntityKind::Segment { .. } => sheet.segment_ends(*e).ok().map(|(a, b)| {
                let mut bb = Bbox::from_point(a);
                bb.include(b);
                bb
            }),
            EntityKind::Curve { .. } => sheet.curve_geometry(*e).ok().map(|c| c.bbox()),
            _ => sheet.shape(*e).ok().map(|s| s.bbox()),
        },
        DiagnosticObject::Variable(_) => None,
    }
}

impl App {
    /// Menu Affichage › « Diagnostic des liens » : ouvre (ou garde ouverte) la fenêtre.
    pub fn open_diagnostic(&mut self) {
        if self.diagnostic.is_none() {
            self.diagnostic = Some(DiagnosticDialog);
        }
    }

    /// Clic sur une ligne : feuille de l'objet active, objet sélectionné, vue cadrée. Une variable
    /// ouvre la fenêtre « Variables » ; un constat sans objet (cycle…) ne fait que changer de feuille.
    pub fn activate_diagnostic(&mut self, d: &LinkDiagnostic) {
        if let Some(DiagnosticObject::Variable(_)) = d.primary() {
            self.open_variables();
            return;
        }
        if d.link == LinkType::Variable {
            self.open_variables();
            return;
        }
        let Some(sheet_id) = d.sheet else {
            return;
        };
        // Le plan de pièces remplace le canevas : on revient à la feuille pour voir l'objet.
        if self.plan.is_some() {
            self.leave_plan();
        }
        self.editor.set_sheet(sheet_id);
        self.editor.selection.clear();
        match d.primary() {
            Some(DiagnosticObject::Point(p)) => self.editor.selection.points.push(*p),
            Some(DiagnosticObject::Entity(e)) => self.editor.selection.entities.push(*e),
            _ => {}
        }
        let bbox = d
            .primary()
            .and_then(|o| object_bbox(self.editor.sheet(), o));
        if let Some(bbox) = bbox {
            self.focus_on(bbox);
        }
    }

    pub fn diagnostic_window(&mut self, ctx: &egui::Context) {
        let Some(dialog) = self.diagnostic.take() else {
            return;
        };
        let diags = self.editor.project().link_diagnostics();
        let rows: Vec<(LinkDiagnostic, String)> = diags
            .into_iter()
            .map(|d| {
                let text = row_text(self.editor.project(), &d);
                (d, text)
            })
            .collect();
        let count = |s: DiagnosticSeverity| rows.iter().filter(|(d, _)| d.severity == s).count();
        let secondary = pf_ui::theme::tokens(ctx).text_secondary;
        let mut open = true;
        let mut close = false;
        let mut clicked: Option<LinkDiagnostic> = None;
        let slot = crate::dock::slot(ctx, &self.dock, ToolWin::Diagnostic);
        let width = self.dock.content_width(ToolWin::Diagnostic);
        egui::Window::new(tr("diag.title"))
            .id(ToolWin::Diagnostic.id())
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .fixed_pos(slot.pos)
            .default_height(slot.max_height)
            .max_height(slot.max_height)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .max_height(slot.max_height)
                    .show(ui, |ui| {
                        ui.set_width(width);
                        crate::a11y::label(ui, RichText::new(tr("diag.intro")).color(secondary));
                        ui.add_space(4.0);
                        if rows.is_empty() {
                            crate::a11y::label(ui, RichText::new(tr("diag.empty")).strong());
                            crate::a11y::label(
                                ui,
                                RichText::new(tr("diag.empty.hint")).color(secondary),
                            );
                        } else {
                            crate::a11y::label(
                                ui,
                                RichText::new(tr_args(
                                    "diag.counts",
                                    &[
                                        ("errors", &count(DiagnosticSeverity::Error).to_string()),
                                        (
                                            "warnings",
                                            &count(DiagnosticSeverity::Warning).to_string(),
                                        ),
                                        ("infos", &count(DiagnosticSeverity::Info).to_string()),
                                    ],
                                )),
                            );
                            for severity in SEVERITIES {
                                let n = count(severity);
                                if n == 0 {
                                    continue;
                                }
                                ui.add_space(6.0);
                                ui.horizontal(|ui| {
                                    severity_icon(ui, severity);
                                    crate::a11y::label(
                                        ui,
                                        RichText::new(tr_args(
                                            group_key(severity),
                                            &[("count", &n.to_string())],
                                        ))
                                        .strong(),
                                    );
                                });
                                for link in LINK_TYPES {
                                    let in_type: Vec<_> = rows
                                        .iter()
                                        .filter(|(d, _)| d.severity == severity && d.link == link)
                                        .collect();
                                    if in_type.is_empty() {
                                        continue;
                                    }
                                    crate::a11y::label(
                                        ui,
                                        RichText::new(format!(
                                            "{} ({})",
                                            type_label(link),
                                            in_type.len()
                                        ))
                                        .color(secondary),
                                    );
                                    for (d, text) in in_type {
                                        ui.horizontal(|ui| {
                                            severity_icon(ui, d.severity);
                                            let response = ui.add(
                                                egui::Button::new(text.as_str())
                                                    .frame(false)
                                                    .wrap(),
                                            );
                                            let response = response
                                                .on_hover_cursor(egui::CursorIcon::PointingHand);
                                            if response.clicked() {
                                                clicked = Some(d.clone());
                                            }
                                        });
                                    }
                                }
                            }
                        }
                        ui.add_space(6.0);
                        close = button(ui, &tr("dialog.close"), ButtonKind::Secondary).clicked();
                    });
            });
        self.diagnostic = Some(dialog);
        if let Some(d) = clicked {
            self.activate_diagnostic(&d);
        }
        if close || !open {
            self.diagnostic = None;
        }
    }
}
