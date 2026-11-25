//! US-03.3 — longueur développée **imposée**, dans l'inspecteur d'une courbe de Bézier.
//!
//! Section repliée par défaut : case « Imposer la longueur développée », cible (une **valeur**, ou
//! **égale à la longueur d'un autre tracé + décalage**, US-03.4 : tracé choisi par un clic dans le
//! plan, ou, US-03.4b, **somme de plusieurs tracés** dont certains d'**autres feuilles**, avec un
//! décalage en nombre ou **maintenu par une formule**), choix de ce qui s'ajuste (la poignée d'une extrémité, ou une extrémité qui glisse le long
//! d'une direction), état « résolue / non résolue » (icône dessinée **et** texte, jamais la couleur
//! seule) et bouton « Désactiver ». Tout passe par `Command::SetCurveLengthConstraint` ; un refus
//! (plage sans solution, cycle…) s'affiche sous le champ et ne change rien. Seules les courbes de Bézier ont cette
//! section : c'est le seul endroit de l'interface qui dit « longueur imposée ». À ne pas confondre
//! avec la mesure observée (lignes au-dessus) ni avec « ƒ Maintenu » (formule d'un paramètre).

use crate::a11y::ComboExt;
use crate::app::App;
use crate::editor::{Editor, Target};
use crate::palette::pal;
use eframe::egui::{self, RichText};
use pf_core::input::{format_number, parse_angle_deg};
use pf_core::units::parse_length_display;
use pf_core::{
    Command, CurveLength, EntityId, EntityKind, LengthAdjust, LengthEquality, LengthRef,
    LengthSpec, LengthTerm, PointId, Project, Sheet, SheetId, LENGTH_TOLERANCE_MM,
};
use pf_geometry::PointMm;
use pf_i18n::{tr, tr_args};

/// Ce qui s'ajuste, dans l'ordre d'affichage.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Choice {
    HandleStart,
    HandleEnd,
    SlideStart,
    SlideEnd,
}

/// Cible de la contrainte : une valeur, ou la longueur d'un autre tracé + décalage (US-03.4a).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Mode {
    Value,
    Reference,
    /// Cible maintenue par une formule sur les variables (US-03.3b).
    Formula,
}

/// Choix par clic du tracé de référence (état d'interface, jamais enregistré) : `waiting` = courbe
/// dont on attend la référence, `chosen` = `(courbe, tracé cliqué)` à reprendre par l'inspecteur.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LengthPick {
    pub waiting: Option<EntityId>,
    pub chosen: Option<(EntityId, EntityId)>,
}

impl Editor {
    /// Attend un clic sur le tracé de référence de `curve` (interception avant tout outil).
    pub fn start_length_pick(&mut self, curve: EntityId) {
        self.length_pick = LengthPick {
            waiting: Some(curve),
            chosen: None,
        };
        self.info(tr("status.imposed.pick"));
    }

    /// Clic pendant le choix : un tracé autre que la courbe elle-même est retenu.
    pub fn length_pick_click(&mut self, world: PointMm, tol_mm: f64) {
        let Some(curve) = self.length_pick.waiting else {
            return;
        };
        match self.hit_test(world, tol_mm) {
            Some(Target::Entity(e)) if e == curve => self.error(tr("status.imposed.self")),
            Some(Target::Entity(e)) => {
                self.length_pick = LengthPick {
                    waiting: None,
                    chosen: Some((curve, e)),
                };
                self.info(tr("status.imposed.picked"));
            }
            _ => self.error(tr("status.imposed.not_trace")),
        }
    }

    /// Échap : abandonne le choix en cours.
    pub fn cancel_length_pick(&mut self) -> bool {
        if self.length_pick.waiting.is_none() {
            return false;
        }
        self.length_pick = LengthPick::default();
        self.info(tr("status.cancelled"));
        true
    }
}

/// Brouillon de la section pour une courbe (état d'interface, jamais enregistré).
impl ImposedForm {
    /// Réécrit les champs de longueur quand l'unité d'affichage change (une formule est laissée).
    pub(crate) fn convert_lengths(&mut self, f: &dyn Fn(&str) -> String) {
        self.target = f(&self.target);
        self.offset = f(&self.offset);
    }
}

pub(crate) struct ImposedForm {
    curve: EntityId,
    target: String,
    choice: Choice,
    angle: String,
    mode: Mode,
    /// Tracés de la somme (US-03.4b) ; un seul = l'égalité simple de 03.4a.
    terms: Vec<LengthTerm>,
    /// Le prochain tracé choisi dans le plan remplace la somme au lieu de s'y ajouter.
    replace: bool,
    /// Décalage : un nombre (mm), ou une formule sur les variables.
    offset: String,
    formula: String,
    error: Option<String>,
    /// Contrainte du projet dont ce brouillon est issu : si elle change (annuler, rétablir, autre
    /// commande), le brouillon est relu du projet.
    synced: Option<CurveLength>,
}

/// Extrémités d'une courbe de Bézier ou d'une courbe lisse (les seuls tracés qui ont cette section).
fn ends(sheet: &Sheet, curve: EntityId) -> Option<(PointId, PointId)> {
    match sheet.entity(curve).ok()?.kind {
        EntityKind::Curve { start, end, .. } => Some((start, end)),
        EntityKind::Spline { .. } => sheet.entity_points(curve).ok(),
        _ => None,
    }
}

/// Courbe lisse : pas de poignée à ajuster, seule une extrémité peut glisser.
fn is_smooth(sheet: &Sheet, curve: EntityId) -> bool {
    matches!(
        sheet.entity(curve).map(|e| &e.kind),
        Ok(EntityKind::Spline { .. })
    )
}

impl ImposedForm {
    fn new(sheet: &Sheet, curve: EntityId, constraint: Option<&CurveLength>) -> Self {
        let (start, end) = ends(sheet, curve).unwrap_or((PointId(0), PointId(0)));
        let measured = sheet.curve_developed_mm(curve).unwrap_or(0.0);
        let chord = sheet
            .position(start)
            .ok()
            .zip(sheet.position(end).ok())
            .and_then(|(a, b)| pf_geometry::direction_angle_deg(a, b).ok())
            .unwrap_or(0.0);
        let (target, choice, angle) = match constraint {
            Some(c) => {
                let (choice, angle) = match c.adjust {
                    LengthAdjust::Handle { point } if point == start => {
                        (Choice::HandleStart, chord)
                    }
                    LengthAdjust::Handle { .. } => (Choice::HandleEnd, chord),
                    LengthAdjust::Slide { point, angle_deg } if point == start => {
                        (Choice::SlideStart, angle_deg)
                    }
                    LengthAdjust::Slide { angle_deg, .. } => (Choice::SlideEnd, angle_deg),
                };
                (c.length_mm, choice, angle)
            }
            None => (measured, Choice::SlideEnd, chord),
        };
        let (mut mode, terms, offset) = match constraint.and_then(|c| c.reference.map(|r| (c, r))) {
            Some((c, r)) => (
                Mode::Reference,
                c.terms(),
                c.offset_expr()
                    .map_or_else(|| pf_core::units::field_text(r.offset_mm), str::to_string),
            ),
            None => (Mode::Value, Vec::new(), "0".to_string()),
        };
        let formula = constraint.and_then(|c| c.expr.clone()).unwrap_or_default();
        if !formula.is_empty() {
            mode = Mode::Formula;
        }
        ImposedForm {
            curve,
            target: pf_core::units::field_text(target),
            choice,
            angle: format_number(angle, 1),
            mode,
            terms,
            replace: false,
            offset,
            formula,
            error: None,
            synced: constraint.cloned(),
        }
    }

    /// Égalité décrite par le brouillon : somme des tracés + décalage (nombre ou formule).
    fn equality(&self) -> Result<LengthEquality, String> {
        if self.terms.is_empty() {
            return Err(tr("inspector.imposed.ref.need"));
        }
        let text = self.offset.trim();
        let (offset_mm, offset_expr) = match parse_length_display(text) {
            Ok(v) => (v, None),
            Err(_) if !text.is_empty() => (0.0, Some(text.to_string())),
            Err(e) => return Err(e),
        };
        Ok(LengthEquality {
            terms: self.terms.clone(),
            offset_mm,
            offset_expr,
        })
    }

    /// La demande décrite par le brouillon, ou le message de ce qui est invalide. Le second
    /// élément est l'égalité **étendue** (autre feuille, somme, formule), posée par une seconde
    /// commande dans la même étape ; `None` pour une valeur, une formule de cible ou l'égalité
    /// simple à un tracé de la feuille (03.4a, inchangée).
    fn spec(
        &self,
        project: &Project,
        sheet_id: SheetId,
    ) -> Result<(LengthSpec, Option<LengthEquality>), String> {
        let sheet = project.sheet(sheet_id).map_err(|e| e.to_string())?;
        let (start, end) = ends(sheet, self.curve).ok_or_else(|| tr("inspector.imposed.gone"))?;
        let measured = || {
            sheet
                .curve_developed_mm(self.curve)
                .map_err(|_| tr("inspector.imposed.ref.unreadable"))
        };
        let (length_mm, reference, extended) = match self.mode {
            Mode::Value => (parse_length_display(&self.target)?, None, None),
            // Valeur de départ seulement : la formule la remplace (même étape, `SetCurveLengthExpr`).
            Mode::Formula => (measured()?, None, None),
            Mode::Reference => {
                let eq = self.equality()?;
                let (_, _, target) = project
                    .equality_preview(sheet_id, self.curve, &eq)
                    .map_err(|_| tr("inspector.imposed.ref.unreadable"))?;
                match (eq.terms.as_slice(), eq.offset_expr.is_some()) {
                    // Égalité simple : la cible est recalculée par le noyau ; ceci n'en est que l'écho.
                    ([t], false) if t.sheet.is_none() => (
                        target,
                        Some(LengthRef {
                            entity: t.entity,
                            offset_mm: eq.offset_mm,
                        }),
                        None,
                    ),
                    _ => (measured()?, None, Some(eq)),
                }
            }
        };
        let adjust = match self.choice {
            Choice::HandleStart => LengthAdjust::Handle { point: start },
            Choice::HandleEnd => LengthAdjust::Handle { point: end },
            Choice::SlideStart => LengthAdjust::Slide {
                point: start,
                angle_deg: parse_angle_deg(&self.angle)?,
            },
            Choice::SlideEnd => LengthAdjust::Slide {
                point: end,
                angle_deg: parse_angle_deg(&self.angle)?,
            },
        };
        Ok((
            LengthSpec {
                length_mm,
                adjust,
                reference,
            },
            extended,
        ))
    }

    /// Désignation d'un tracé de la somme ; la feuille est nommée quand ce n'est pas celle de la courbe.
    fn term_name(&self, project: &Project, sheet_id: SheetId, t: &LengthTerm) -> String {
        match t.sheet.and_then(|id| project.sheet(id).ok()) {
            Some(s) if s.id != sheet_id => tr_args(
                "inspector.imposed.ref.sheet",
                &[
                    ("name", &crate::text::entity_name(s, t.entity)),
                    ("sheet", &s.name),
                ],
            ),
            _ => project
                .sheet(sheet_id)
                .map(|s| crate::text::entity_name(s, t.entity))
                .unwrap_or_default(),
        }
    }
}

/// Nombre signé (`+12`, `-3`) pour l'affichage d'un décalage.
fn signed(x: f64) -> String {
    if x >= 0.0 {
        format!("+{}", pf_core::units::len_number(x, 3))
    } else {
        pf_core::units::len_number(x, 3)
    }
}

/// Icône d'état dessinée : coche (résolue) ou croix (non résolue) ; toujours avec son texte.
fn state_icon(ui: &mut egui::Ui, ok: bool) {
    let tokens = pf_ui::theme::tokens(ui.ctx());
    let color = if ok {
        ui.visuals().strong_text_color()
    } else {
        tokens.danger
    };
    let (rect, _) = ui.allocate_exact_size(egui::vec2(18.0, 16.0), egui::Sense::hover());
    let stroke = egui::Stroke::new(2.0_f32, color);
    let c = rect.center();
    if ok {
        let pts = [
            c + egui::vec2(-6.0, 0.0),
            c + egui::vec2(-2.0, 4.5),
            c + egui::vec2(6.0, -5.0),
        ];
        ui.painter().line_segment([pts[0], pts[1]], stroke);
        ui.painter().line_segment([pts[1], pts[2]], stroke);
    } else {
        ui.painter().line_segment(
            [c + egui::vec2(-5.0, -5.0), c + egui::vec2(5.0, 5.0)],
            stroke,
        );
        ui.painter().line_segment(
            [c + egui::vec2(-5.0, 5.0), c + egui::vec2(5.0, -5.0)],
            stroke,
        );
    }
}

impl App {
    /// Section repliée « Longueur imposée » d'une courbe de Bézier.
    pub(crate) fn imposed_section(&mut self, ui: &mut egui::Ui, curve: EntityId) {
        let active = self.editor.sheet().length_constraint(curve).is_some();
        let title = if active {
            format!(
                "{} · {}",
                tr("inspector.imposed.title"),
                tr("inspector.imposed.active")
            )
        } else {
            tr("inspector.imposed.title")
        };
        let mut header =
            egui::CollapsingHeader::new(RichText::new(title).small().strong().color(pal().muted))
                .id_salt(("imposed", curve.0))
                .default_open(false);
        let once = std::mem::take(&mut self.imposed_open_once);
        if once {
            header = header.open(Some(true));
        }
        let shown = header.show(ui, |ui| self.imposed_body(ui, curve));
        if once {
            shown.header_response.scroll_to_me(Some(egui::Align::TOP));
        }
    }

    fn imposed_body(&mut self, ui: &mut egui::Ui, curve: EntityId) {
        let sheet_id = self.editor.sheet;
        let constraint = self.editor.sheet().length_constraint(curve).cloned();
        let smooth = is_smooth(self.editor.sheet(), curve);
        let mut form = match self.imposed_form.take() {
            Some(f) if f.curve == curve && f.synced == constraint => f,
            _ => ImposedForm::new(self.editor.sheet(), curve, constraint.as_ref()),
        };
        let Some((start, end)) = ends(self.editor.sheet(), curve) else {
            return;
        };
        let (label_a, label_b) = {
            let s = self.editor.sheet();
            (s.label(start).to_string(), s.label(end).to_string())
        };
        let muted = pal().muted;
        let danger = pf_ui::theme::tokens(ui.ctx()).danger;
        let mut apply = false;
        let mut release = false;
        // Tracé de référence cliqué dans le plan : repris ici (une contrainte déjà posée est
        // aussitôt mise à jour).
        if let Some((c, r)) = self.editor.length_pick.chosen {
            if c == curve {
                self.editor.length_pick.chosen = None;
                let term = LengthTerm {
                    entity: r,
                    sheet: None,
                };
                form.mode = Mode::Reference;
                form.error = None;
                if form.replace {
                    form.terms = vec![term];
                    form.replace = false;
                    apply = constraint.is_some();
                } else if form.terms.contains(&term) {
                    form.error = Some(tr("inspector.imposed.ref.dup"));
                } else {
                    form.terms.push(term);
                    apply = constraint.is_some();
                }
            }
        }
        crate::a11y::label(
            ui,
            RichText::new(tr("inspector.imposed.note"))
                .small()
                .color(muted),
        );
        let mut on = constraint.is_some();
        if ui
            .checkbox(&mut on, tr("inspector.imposed.checkbox"))
            .on_hover_text(tr("inspector.imposed.checkbox.hover"))
            .changed()
        {
            if on {
                apply = true;
            } else {
                release = true;
            }
        }

        match &constraint {
            Some(c) => {
                let measured = self
                    .editor
                    .sheet()
                    .curve_developed_mm(curve)
                    .unwrap_or(f64::NAN);
                let solved = (measured - c.length_mm).abs() <= LENGTH_TOLERANCE_MM;
                ui.horizontal(|ui| {
                    state_icon(ui, solved);
                    crate::a11y::label(
                        ui,
                        RichText::new(tr(if solved {
                            "inspector.imposed.state.ok"
                        } else {
                            "inspector.imposed.state.bad"
                        }))
                        .strong()
                        .size(15.0),
                    );
                });
                crate::a11y::label(
                    ui,
                    RichText::new(pf_i18n::tr_len(
                        "inspector.imposed.gap",
                        &[(
                            "gap",
                            &pf_core::units::len_number(measured - c.length_mm, 6),
                        )],
                    ))
                    .small()
                    .color(muted),
                );
            }
            None => {
                crate::a11y::label(
                    ui,
                    RichText::new(tr("inspector.imposed.state.off")).strong(),
                );
            }
        }

        // Cible : une valeur, ou la longueur d'un autre tracé + décalage.
        ui.horizontal(|ui| {
            for (mode, key) in [
                (Mode::Value, "inspector.imposed.mode.value"),
                (Mode::Reference, "inspector.imposed.mode.ref"),
                (Mode::Formula, "inspector.imposed.mode.formula"),
            ] {
                if ui.radio_value(&mut form.mode, mode, tr(key)).changed() && constraint.is_some() {
                    apply = true;
                }
            }
        });
        match form.mode {
            Mode::Value => {
                ui.horizontal(|ui| {
                    crate::a11y::label(ui, tr("inspector.imposed.target"));
                    let r = crate::a11y::add_text(
                        ui,
                        egui::TextEdit::singleline(&mut form.target)
                            .id_salt(("imposed-target", curve.0))
                            .font(egui::TextStyle::Monospace)
                            .desired_width(90.0),
                    );
                    crate::a11y::label(ui, pf_core::units::display_units().unit.suffix());
                    if r.lost_focus() && constraint.is_some() {
                        apply = true;
                    }
                });
            }
            Mode::Formula => {
                ui.horizontal(|ui| {
                    crate::a11y::label(ui, tr("inspector.imposed.formula"));
                    let r = crate::a11y::add_text(
                        ui,
                        egui::TextEdit::singleline(&mut form.formula)
                            .id_salt(("imposed-formula", curve.0))
                            .font(egui::TextStyle::Monospace)
                            .desired_width(150.0),
                    );
                    if r.lost_focus() && !form.formula.trim().is_empty() {
                        apply = true;
                    }
                });
                let line = match &constraint {
                    Some(c) if c.expr.is_some() => pf_i18n::tr_len(
                        "inspector.imposed.formula.value",
                        &[("value", &pf_core::units::len_number(c.length_mm, 3))],
                    ),
                    _ => tr("inspector.imposed.formula.hint"),
                };
                crate::a11y::label(ui, RichText::new(line).small().color(muted));
            }
            Mode::Reference => {
                let waiting = self.editor.length_pick.waiting == Some(curve);
                let names: Vec<String> = form
                    .terms
                    .iter()
                    .map(|t| form.term_name(self.editor.project(), sheet_id, t))
                    .collect();
                ui.horizontal_wrapped(|ui| {
                    crate::a11y::label(
                        ui,
                        match names.as_slice() {
                            [] => tr("inspector.imposed.ref.none"),
                            [n] => tr_args("inspector.imposed.ref.name", &[("name", n)]),
                            many => tr_args(
                                "inspector.imposed.ref.sum",
                                &[
                                    ("count", &many.len().to_string()),
                                    ("names", &many.join(" + ")),
                                ],
                            ),
                        },
                    );
                });
                // Chaque tracé d'une somme se retire (le dernier se change, il ne se retire pas).
                if names.len() >= 2 {
                    let mut remove = None;
                    for (i, n) in names.iter().enumerate() {
                        ui.horizontal(|ui| {
                            crate::a11y::label(ui, n);
                            if ui
                                .small_button("×")
                                .on_hover_text(tr_args(
                                    "inspector.imposed.ref.remove",
                                    &[("name", n)],
                                ))
                                .clicked()
                            {
                                remove = Some(i);
                            }
                        });
                    }
                    if let Some(i) = remove {
                        form.terms.remove(i);
                        if constraint.is_some() {
                            apply = true;
                        }
                    }
                }
                ui.horizontal_wrapped(|ui| {
                    if waiting {
                        if ui.button(tr("inspector.imposed.ref.cancel")).clicked() {
                            self.editor.cancel_length_pick();
                        }
                    } else if form.terms.is_empty() {
                        if ui.button(tr("inspector.imposed.ref.pick")).clicked() {
                            form.replace = true;
                            self.editor.start_length_pick(curve);
                        }
                    } else {
                        if ui.button(tr("inspector.imposed.ref.change")).clicked() {
                            form.replace = true;
                            self.editor.start_length_pick(curve);
                        }
                        if ui
                            .button(tr("inspector.imposed.ref.add"))
                            .on_hover_text(tr("inspector.imposed.ref.add.hover"))
                            .clicked()
                        {
                            form.replace = false;
                            self.editor.start_length_pick(curve);
                        }
                    }
                });
                if waiting {
                    crate::a11y::label(
                        ui,
                        RichText::new(tr("inspector.imposed.ref.waiting"))
                            .small()
                            .color(muted),
                    );
                }
                // Tracé d'une autre feuille (US-03.4b) : choisi dans une liste, par feuille.
                let others: Vec<(SheetId, String)> = self
                    .editor
                    .project()
                    .sheets
                    .iter()
                    .filter(|s| s.id != sheet_id)
                    .map(|s| (s.id, s.name.clone()))
                    .collect();
                if !others.is_empty() {
                    let mut picked: Option<LengthTerm> = None;
                    egui::ComboBox::from_id_salt(("imposed-other-sheet", curve.0))
                        .selected_text(tr("inspector.imposed.ref.other"))
                        .show_ui_named(ui, |ui| {
                            for (id, name) in &others {
                                let Ok(sheet) = self.editor.project().sheet(*id) else {
                                    continue;
                                };
                                for e in sheet.entities.iter().take(60) {
                                    let label = format!(
                                        "{name} · {}",
                                        crate::text::entity_name(sheet, e.id)
                                    );
                                    if ui.selectable_label(false, label).clicked() {
                                        picked = Some(LengthTerm {
                                            entity: e.id,
                                            sheet: Some(*id),
                                        });
                                    }
                                }
                            }
                        });
                    if let Some(t) = picked {
                        if form.terms.contains(&t) {
                            form.error = Some(tr("inspector.imposed.ref.dup"));
                        } else {
                            form.terms.push(t);
                            form.mode = Mode::Reference;
                            form.error = None;
                            if constraint.is_some() {
                                apply = true;
                            }
                        }
                    }
                }
                ui.horizontal(|ui| {
                    crate::a11y::label(ui, tr("inspector.imposed.offset"));
                    let r = crate::a11y::add_text(
                        ui,
                        egui::TextEdit::singleline(&mut form.offset)
                            .id_salt(("imposed-offset", curve.0))
                            .font(egui::TextStyle::Monospace)
                            .desired_width(110.0),
                    );
                    if parse_length_display(&form.offset).is_ok() {
                        crate::a11y::label(ui, pf_core::units::display_units().unit.suffix());
                    }
                    if r.lost_focus() && constraint.is_some() {
                        apply = true;
                    }
                });
                crate::a11y::label(
                    ui,
                    RichText::new(tr("inspector.imposed.offset.hint"))
                        .small()
                        .color(muted),
                );
                // Longueurs des tracés, décalage évalué et résultat, relus du modèle.
                if !form.terms.is_empty() {
                    if let Ok(eq) = form.equality() {
                        let line =
                            match self.editor.project().equality_preview(sheet_id, curve, &eq) {
                                Ok((base, off, target)) => match names.as_slice() {
                                    [n] if eq.offset_expr.is_none()
                                        && form.terms[0].sheet.is_none() =>
                                    {
                                        pf_i18n::tr_len(
                                            "inspector.imposed.ref.result",
                                            &[
                                                ("name", n),
                                                ("base", &pf_core::units::len_number(base, 3)),
                                                ("offset", &signed(off)),
                                                ("result", &pf_core::units::len_number(target, 3)),
                                            ],
                                        )
                                    }
                                    _ => pf_i18n::tr_len(
                                        "inspector.imposed.ref.result.sum",
                                        &[
                                            ("names", &names.join(" + ")),
                                            ("base", &pf_core::units::len_number(base, 3)),
                                            ("offset", &signed(off)),
                                            ("result", &pf_core::units::len_number(target, 3)),
                                        ],
                                    ),
                                },
                                Err(_) => tr("inspector.imposed.ref.unreadable"),
                            };
                        crate::a11y::label(ui, RichText::new(line).small().color(muted));
                    }
                }
            }
        }
        crate::a11y::label(
            ui,
            RichText::new(tr("inspector.imposed.adjust")).color(muted),
        );
        if smooth {
            // Courbe lisse : pas de poignée ; la racine la plus proche de la position courante.
            crate::a11y::label(
                ui,
                RichText::new(tr("inspector.imposed.smooth"))
                    .small()
                    .color(muted),
            );
            if matches!(form.choice, Choice::HandleStart | Choice::HandleEnd) {
                form.choice = Choice::SlideEnd;
            }
        }
        // Deux lignes (poignée / extrémité qui glisse), chacune avec ses deux extrémités.
        for (kind, start, end, key) in [
            (
                "handle",
                Choice::HandleStart,
                Choice::HandleEnd,
                "inspector.imposed.handle",
            ),
            (
                "slide",
                Choice::SlideStart,
                Choice::SlideEnd,
                "inspector.imposed.slide",
            ),
        ]
        .into_iter()
        .filter(|(kind, ..)| !(smooth && *kind == "handle"))
        {
            ui.horizontal(|ui| {
                crate::a11y::label(ui, tr(key));
                ui.push_id((kind, curve.0), |ui| {
                    for (choice, label) in [(start, &label_a), (end, &label_b)] {
                        if ui
                            .radio_value(&mut form.choice, choice, label.as_str())
                            .changed()
                            && constraint.is_some()
                        {
                            apply = true;
                        }
                    }
                });
            });
        }
        if matches!(form.choice, Choice::SlideStart | Choice::SlideEnd) {
            ui.horizontal(|ui| {
                crate::a11y::label(ui, tr("inspector.imposed.angle"));
                let r = crate::a11y::add_text(
                    ui,
                    egui::TextEdit::singleline(&mut form.angle)
                        .id_salt(("imposed-angle", curve.0))
                        .font(egui::TextStyle::Monospace)
                        .desired_width(60.0),
                );
                crate::a11y::label(ui, "°");
                if r.lost_focus() && constraint.is_some() {
                    apply = true;
                }
            });
        }
        if constraint.is_some()
            && ui
                .button(tr("inspector.imposed.off"))
                .on_hover_text(tr("inspector.imposed.off.hover"))
                .clicked()
        {
            release = true;
        }

        if release {
            form.error = None;
            self.editor.exec(Command::SetCurveLengthConstraint {
                sheet: sheet_id,
                curve,
                spec: None,
            });
        } else if apply && !(form.mode == Mode::Formula && form.formula.trim().is_empty()) {
            form.error = None;
            match form.spec(self.editor.project(), sheet_id) {
                Ok((spec, extended)) => {
                    let set = Command::SetCurveLengthConstraint {
                        sheet: sheet_id,
                        curve,
                        spec: Some(spec),
                    };
                    // Cible par formule, ou égalité étendue : contrainte puis lien dans **une**
                    // étape annulable.
                    let command = match (form.mode, extended) {
                        (Mode::Formula, _) => Command::Batch {
                            commands: vec![
                                set,
                                Command::SetCurveLengthExpr {
                                    sheet: sheet_id,
                                    curve,
                                    expr: Some(form.formula.clone()),
                                },
                            ],
                        },
                        (_, Some(equality)) => Command::Batch {
                            commands: vec![
                                set,
                                Command::SetCurveLengthEquality {
                                    sheet: sheet_id,
                                    curve,
                                    equality: Some(equality),
                                },
                            ],
                        },
                        _ => set,
                    };
                    if self.editor.exec(command).is_none() {
                        form.error = Some(self.editor.status.text.clone());
                    }
                }
                Err(m) => form.error = Some(m),
            }
        }
        if let Some(e) = &form.error {
            ui.horizontal_wrapped(|ui| {
                state_icon(ui, false);
                crate::a11y::label(
                    ui,
                    RichText::new(tr_args("inspector.imposed.refused", &[("reason", e)]))
                        .small()
                        .color(danger),
                );
            });
        }
        self.imposed_form = Some(form);
    }

    /// Mention d'une extrémité libre déplacée par la longueur imposée d'une courbe.
    pub(crate) fn imposed_point_note(&self, ui: &mut egui::Ui, point: PointId) {
        let sheet = self.editor.sheet();
        let Some(c) = sheet.length_constraint_moving_point(point) else {
            return;
        };
        let name = crate::text::entity_name(sheet, c.curve);
        crate::a11y::label(
            ui,
            RichText::new(tr_args("inspector.imposed.point", &[("name", &name)]))
                .small()
                .color(pal().muted),
        );
    }
}
