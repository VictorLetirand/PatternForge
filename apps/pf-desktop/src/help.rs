//! Aide intégrée (US-16.9) : geste attendu de chaque outil (infobulle), guide pas à pas du
//! premier patron et projets d'exemple (vêtement, sac). Rien ici ne touche au projet ouvert :
//! un exemple s'ouvre comme un nouveau document, le guide n'est qu'une fenêtre de lecture.

use crate::app::App;
use crate::editor::Tool;
use eframe::egui;
use pf_core::{persist, CoreError, Project};
use pf_i18n::{tr, tr_args};

/// Nombre d'étapes du guide du premier patron.
pub const GUIDE_STEPS: usize = 8;

/// Geste attendu d'un outil : le texte affiché dans la barre d'état quand l'outil est choisi.
pub fn tool_gesture(tool: Tool) -> String {
    match tool {
        Tool::Select => tr("status.hint.select"),
        Tool::Point => tr("status.hint.point"),
        Tool::Segment => tr("status.hint.segment"),
        Tool::Rectangle => tr("status.hint.rectangle"),
        Tool::Parallel => tr("status.hint.parallel"),
        Tool::Perpendicular => tr("status.hint.perpendicular"),
        Tool::PointOnSegment => tr("status.hint.point_on_segment"),
        Tool::Notch => tr("status.hint.notch"),
        Tool::Grainline => tr("status.hint.grainline"),
        Tool::Curve => tr("status.hint.curve"),
        Tool::Spline => tr("status.hint.spline"),
        Tool::Circle => tr("status.hint.circle"),
        Tool::Arc => tr("status.hint.arc"),
        Tool::Fillet => tr("status.hint.fillet"),
        Tool::Chamfer => tr("status.hint.chamfer"),
        Tool::Tangent => tr("status.hint.tangent"),
        Tool::Extend => tr("status.hint.extend"),
        Tool::Split => tr("status.hint.split"),
        Tool::Merge => tr("status.hint.merge"),
        Tool::Offset => tr("status.hint.offset"),
        Tool::Measure => tr("status.hint.measure"),
        Tool::Dimension => tr("status.hint.dimension"),
        Tool::Guide => tr("help.tool.guide"),
        Tool::ExtractPiece => tr("status.hint.extract_piece"),
        Tool::Dart => tr("status.hint.dart"),
        Tool::Trace => tr("status.hint.trace"),
    }
}

/// Titre et texte d'une étape du guide (0 à `GUIDE_STEPS - 1`).
pub fn guide_step(step: usize) -> (String, String) {
    let (title, body) = match step {
        0 => (tr("guide.step.1.title"), tr("guide.step.1.body")),
        1 => (tr("guide.step.2.title"), tr("guide.step.2.body")),
        2 => (tr("guide.step.3.title"), tr("guide.step.3.body")),
        3 => (tr("guide.step.4.title"), tr("guide.step.4.body")),
        4 => (tr("guide.step.5.title"), tr("guide.step.5.body")),
        5 => (tr("guide.step.6.title"), tr("guide.step.6.body")),
        6 => (tr("guide.step.7.title"), tr("guide.step.7.body")),
        _ => (tr("guide.step.8.title"), tr("guide.step.8.body")),
    };
    (title, body)
}

/// Projets d'exemple embarqués (les mêmes fichiers que `examples/`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Example {
    /// Vêtement : jupe droite de base.
    Skirt,
    /// Sac : cabas en deux pièces.
    Bag,
}

impl Example {
    pub const ALL: [Example; 2] = [Example::Skirt, Example::Bag];

    fn json(self) -> &'static str {
        match self {
            Example::Skirt => include_str!("../../../examples/jupe-droite.pforge.json"),
            Example::Bag => include_str!("../../../examples/aide/sac-cabas.pforge.json"),
        }
    }

    pub fn label(self) -> String {
        match self {
            Example::Skirt => tr("menu.help.example.skirt"),
            Example::Bag => tr("menu.help.example.bag"),
        }
    }

    /// Projet relu et validé comme n'importe quel fichier.
    pub fn load(self) -> Result<Project, CoreError> {
        persist::from_json(self.json())
    }
}

impl App {
    /// Ouvre un exemple comme nouveau document ; refusé si des modifications seraient perdues.
    pub fn open_example(&mut self, example: Example) {
        if self.editor.doc.is_dirty() {
            self.editor.error(tr("error.demo_dirty"));
            return;
        }
        match example.load() {
            Ok(project) => self.replace_document(pf_core::Document::new(project)),
            Err(e) => self
                .editor
                .error(tr_args("help.example.error", &[("error", &e.to_string())])),
        }
    }

    /// Fenêtre du guide pas à pas : lecture seule, une étape à la fois.
    pub fn guide_window(&mut self, ctx: &egui::Context) {
        let Some(step) = self.guide_step else {
            return;
        };
        let step = step.min(GUIDE_STEPS - 1);
        let mut open = true;
        let mut next = step;
        let mut example = None;
        egui::Window::new(tr("menu.help.guide"))
            .open(&mut open)
            .collapsible(false)
            .default_width(420.0)
            .show(ctx, |ui| {
                crate::a11y::label(
                    ui,
                    tr_args(
                        "guide.progress",
                        &[
                            ("step", &(step + 1).to_string()),
                            ("total", &GUIDE_STEPS.to_string()),
                        ],
                    ),
                );
                let (title, body) = guide_step(step);
                ui.heading(title);
                crate::a11y::label(ui, body);
                ui.separator();
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(step > 0, egui::Button::new(tr("guide.previous")))
                        .clicked()
                    {
                        next = step - 1;
                    }
                    if ui
                        .add_enabled(step + 1 < GUIDE_STEPS, egui::Button::new(tr("guide.next")))
                        .clicked()
                    {
                        next = step + 1;
                    }
                    if step + 1 == GUIDE_STEPS && ui.button(tr("menu.help.example.skirt")).clicked()
                    {
                        example = Some(Example::Skirt);
                    }
                });
            });
        self.guide_step = if open { Some(next) } else { None };
        if let Some(example) = example {
            self.open_example(example);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::Launch;
    use egui_kittest::kittest::Queryable;
    use egui_kittest::Harness;

    fn harness() -> Harness<'static, App> {
        let app = App::with_context(&egui::Context::default(), Launch::default());
        let mut h = Harness::builder()
            .with_size(egui::vec2(1280.0, 800.0))
            .build_state(|ctx, app: &mut App| app.ui(ctx), app);
        h.run();
        h
    }

    #[test]
    fn chaque_outil_a_un_geste_attendu() {
        for tool in Tool::ALL {
            let gesture = tool_gesture(tool);
            assert!(gesture.chars().count() > 20, "{tool:?} : {gesture}");
            assert!(!gesture.starts_with("status.") && !gesture.starts_with("help."));
        }
    }

    #[test]
    fn le_guide_a_huit_etapes_lisibles() {
        let mut titles = std::collections::HashSet::new();
        for step in 0..GUIDE_STEPS {
            let (title, body) = guide_step(step);
            assert!(!title.starts_with("guide.") && body.chars().count() > 40);
            assert!(titles.insert(title));
        }
    }

    #[test]
    fn les_exemples_se_relisent_et_sont_complets() {
        for example in Example::ALL {
            let project = example.load().expect("exemple valide");
            let mut pieces = 0;
            for sheet in &project.sheets {
                for piece in project.pieces_on(sheet.id) {
                    pieces += 1;
                    assert!(piece.grainline.is_some(), "{}", example.label());
                    assert!(piece.seam.is_some() && piece.code.is_some());
                }
            }
            assert!(pieces >= 1);
        }
    }

    #[test]
    fn ouvrir_un_exemple_remplace_le_document_sauf_modifications() {
        let mut h = harness();
        h.state_mut().open_example(Example::Bag);
        assert_eq!(h.state().editor.project().name, "Sac cabas");
        assert!(!h.state().editor.doc.is_dirty());
        let sheet = h.state().editor.sheet;
        h.state_mut().editor.exec(pf_core::Command::RenameSheet {
            sheet,
            name: "Modifiée".into(),
        });
        assert!(h.state().editor.doc.is_dirty());
        h.state_mut().open_example(Example::Skirt);
        assert_eq!(h.state().editor.project().name, "Sac cabas");
    }

    #[test]
    fn le_guide_se_parcourt_et_se_ferme() {
        let mut h = harness();
        h.state_mut().guide_step = Some(0);
        h.run_steps(3);
        h.get_by_label("Étape 1 sur 8");
        h.get_by_label("Suivant").click();
        h.run_steps(3);
        h.get_by_label("Étape 2 sur 8");
        h.get_by_label("Précédent").click();
        h.run_steps(3);
        h.get_by_label("Étape 1 sur 8");
        h.state_mut().guide_step = None;
        h.run_steps(3);
        assert!(h.query_by_label("Étape 1 sur 8").is_none());
    }
}
