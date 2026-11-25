//! US-16.6 - historique visible : liste des Ã©tapes annulables et retour Ã  une Ã©tape.
//!
//! `pf-core` ne donne ni la profondeur ni le contenu de la pile d'annulation : ce module tient
//! un journal d'Ã©tiquettes **cÃ´tÃ© interface**, alignÃ© sur les Ã©tapes du document, et rÃ©alise le
//! Â« retour Ã  une Ã©tape Â» par une succession d'annulations / rÃ©tablissements (une seule Ã©tape
//! composÃ©e du point de vue de l'utilisateur). Rien n'est ajoutÃ© au projet ni au fichier.
//!
//! Le journal n'est tenu qu'Ã  fenÃªtre ouverte : les Ã©tapes faites fenÃªtre fermÃ©e apparaissent
//! sans nom (Â« Ã‰tape n Â»). Il est resynchronisÃ© hors geste (pointeur relÃ¢chÃ©) pour ne jamais
//! clore un geste continu en cours.

use crate::app::App;
use crate::palette::pal;
use eframe::egui::{self, RichText};
use pf_core::{Command, Project};
use pf_i18n::{tr, tr_args};

/// Profondeur maximale de l'historique de `pf-core` (au-delÃ , la plus ancienne Ã©tape est perdue).
const MAX_STEPS: usize = 200;

/// Nom technique d'une commande (nom de variante), sans ses donnÃ©es.
pub fn command_name(command: &Command) -> String {
    let text = format!("{command:?}");
    text.chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect()
}

/// Journal des Ã©tapes : `labels[i]` nomme l'Ã©tape `i + 1` (l'Ã©tape 0 est l'Ã©tat d'ouverture).
#[derive(Debug, Default)]
pub struct HistoryLog {
    labels: Vec<String>,
    /// Nombre d'Ã©tapes appliquÃ©es (position courante).
    pos: usize,
    seen: Option<Project>,
}

impl HistoryLog {
    pub fn labels(&self) -> &[String] {
        &self.labels
    }

    pub fn position(&self) -> usize {
        self.pos
    }

    /// RÃ©concilie le journal avec le document. `undo` / `redo` : profondeurs mesurÃ©es.
    pub fn reconcile(&mut self, undo: usize, redo: usize, label: Option<String>) {
        let total = undo + redo;
        if total == self.labels.len() {
            if redo == 0 && undo == self.pos && undo >= MAX_STEPS {
                // Pile pleine : une nouvelle Ã©tape chasse la plus ancienne.
                self.labels.remove(0);
                self.labels.push(label.unwrap_or_default());
            }
        } else if redo == 0 && undo > self.pos && self.pos == self.labels.len() {
            let added = undo - self.pos;
            for i in 0..added {
                let last = i + 1 == added;
                self.labels
                    .push(if last { label.clone() } else { None }.unwrap_or_default());
            }
        } else {
            self.labels = vec![String::new(); total];
        }
        self.pos = undo;
    }
}

impl App {
    /// Mesure les profondeurs annuler / rÃ©tablir en les parcourant puis en restaurant l'Ã©tat.
    fn measure_history(&mut self) -> (usize, usize) {
        let doc = &mut self.editor.doc;
        let mut undo = 0;
        while doc.undo() {
            undo += 1;
        }
        for _ in 0..undo {
            doc.redo();
        }
        let mut redo = 0;
        while doc.redo() {
            redo += 1;
        }
        for _ in 0..redo {
            doc.undo();
        }
        (undo, redo)
    }

    /// Met le journal Ã  jour si le projet a changÃ© (hors geste en cours).
    pub(crate) fn sync_history(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.pointer.any_down()) || self.editor.is_dragging() {
            return;
        }
        if self.history.seen.as_ref() == Some(self.editor.doc.project()) {
            return;
        }
        let (undo, redo) = self.measure_history();
        let label = self.editor.last_command.take();
        self.history.reconcile(undo, redo, label);
        self.history.seen = Some(self.editor.doc.project().clone());
    }

    /// Revient Ã  l'Ã©tape `target` (0 = ouverture) par annulations / rÃ©tablissements.
    pub fn goto_step(&mut self, target: usize) {
        let mut guard = self.history.pos + self.history.labels.len() + 2;
        while self.history.pos > target && self.editor.doc.can_undo() && guard > 0 {
            self.editor.undo();
            self.history.pos -= 1;
            guard -= 1;
        }
        while self.history.pos < target && self.editor.doc.can_redo() && guard > 0 {
            self.editor.redo();
            self.history.pos += 1;
            guard -= 1;
        }
        self.history.seen = Some(self.editor.doc.project().clone());
        self.editor.info(tr_args(
            "history.went_to",
            &[("n", &self.history.pos.to_string())],
        ));
    }

    /// FenÃªtre Â« Historique Â» : Ã©tapes de la plus rÃ©cente Ã  l'Ã©tat d'ouverture.
    pub fn history_window(&mut self, ctx: &egui::Context) {
        if !self.show_history {
            return;
        }
        self.sync_history(ctx);
        let mut open = true;
        let mut goto = None;
        let muted = pal().muted;
        let (pos, count) = (self.history.position(), self.history.labels().len());
        egui::Window::new(tr("history.title"))
            .open(&mut open)
            .default_width(320.0)
            .show(ctx, |ui| {
                crate::a11y::label(ui, RichText::new(tr("history.note")).small().color(muted));
                ui.separator();
                egui::ScrollArea::vertical()
                    .max_height(360.0)
                    .show(ui, |ui| {
                        for step in (0..=count).rev() {
                            let name = if step == 0 {
                                tr("history.initial")
                            } else {
                                let label = &self.history.labels()[step - 1];
                                if label.is_empty() {
                                    tr_args("history.step", &[("n", &step.to_string())])
                                } else {
                                    tr_args(
                                        "history.step_named",
                                        &[("n", &step.to_string()), ("name", label)],
                                    )
                                }
                            };
                            ui.horizontal(|ui| {
                                if step == pos {
                                    crate::a11y::label(ui, RichText::new(">").strong());
                                    crate::a11y::label(
                                        ui,
                                        RichText::new(format!("{name} {}", tr("history.current")))
                                            .strong(),
                                    );
                                } else if step > pos {
                                    crate::a11y::label(
                                        ui,
                                        RichText::new(format!("{name} {}", tr("history.undone")))
                                            .color(muted),
                                    );
                                } else {
                                    crate::a11y::label(ui, name);
                                }
                                if step != pos && ui.button(tr("history.goto")).clicked() {
                                    goto = Some(step);
                                }
                            });
                        }
                    });
            });
        if let Some(step) = goto {
            self.goto_step(step);
        }
        self.show_history = open;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Launch;
    use egui_kittest::Harness;
    use pf_geometry::PointMm;

    fn harness() -> Harness<'static, App> {
        let app = App::with_context(&egui::Context::default(), Launch::default());
        let mut h = Harness::builder()
            .with_size(egui::vec2(1280.0, 800.0))
            .build_state(|ctx, app: &mut App| app.ui(ctx), app);
        h.run();
        h
    }

    fn add_point(h: &mut Harness<'static, App>, x: f64) {
        let sheet = h.state().editor.sheet;
        h.state_mut().editor.exec(Command::AddPoint {
            sheet,
            at: PointMm::new(x, 0.0),
        });
        h.run_steps(2);
    }

    fn points(h: &Harness<'static, App>) -> usize {
        h.state().editor.sheet().points.len()
    }

    #[test]
    fn le_journal_suit_les_etapes_nommees() {
        let mut h = harness();
        h.state_mut().show_history = true;
        h.run_steps(2);
        add_point(&mut h, 10.0);
        add_point(&mut h, 20.0);
        let log = &h.state().history;
        assert_eq!(log.position(), 2);
        assert_eq!(log.labels().len(), 2);
        assert!(log.labels().iter().all(|l| l == "AddPoint"));
    }

    #[test]
    fn le_retour_a_une_etape_annule_puis_retablit() {
        let mut h = harness();
        h.state_mut().show_history = true;
        h.run_steps(2);
        let base = points(&h);
        for x in [10.0, 20.0, 30.0] {
            add_point(&mut h, x);
        }
        h.state_mut().goto_step(1);
        h.run_steps(2);
        assert_eq!(points(&h), base + 1);
        assert_eq!(h.state().history.position(), 1);
        assert_eq!(
            h.state().history.labels().len(),
            3,
            "Ã©tapes annulÃ©es gardÃ©es"
        );
        h.state_mut().goto_step(3);
        h.run_steps(2);
        assert_eq!(points(&h), base + 3);
        h.state_mut().goto_step(0);
        h.run_steps(2);
        assert_eq!(points(&h), base);
        assert!(!h.state().editor.doc.can_undo());
    }

    #[test]
    fn une_nouvelle_action_apres_retour_efface_les_etapes_annulees() {
        let mut h = harness();
        h.state_mut().show_history = true;
        h.run_steps(2);
        for x in [10.0, 20.0, 30.0] {
            add_point(&mut h, x);
        }
        h.state_mut().goto_step(1);
        h.run_steps(2);
        add_point(&mut h, 99.0);
        assert_eq!(h.state().history.labels().len(), 2);
        assert_eq!(h.state().history.position(), 2);
    }

    #[test]
    fn un_undo_du_menu_est_reflete_sans_perdre_les_noms() {
        let mut h = harness();
        h.state_mut().show_history = true;
        h.run_steps(2);
        add_point(&mut h, 10.0);
        h.state_mut().editor.undo();
        h.run_steps(2);
        assert_eq!(h.state().history.position(), 0);
        assert_eq!(h.state().history.labels(), ["AddPoint"]);
    }

    #[test]
    fn le_retour_ne_touche_pas_au_fichier_enregistre() {
        let mut h = harness();
        h.state_mut().show_history = true;
        h.run_steps(2);
        add_point(&mut h, 10.0);
        h.state_mut().goto_step(0);
        assert!(!h.state().editor.doc.is_dirty());
    }

    #[test]
    fn la_reconciliation_gere_pile_pleine_et_etapes_inconnues() {
        let mut log = HistoryLog::default();
        log.reconcile(3, 0, Some("X".into()));
        assert_eq!(log.labels(), ["", "", "X"]);
        log.reconcile(2, 1, None);
        assert_eq!((log.position(), log.labels().len()), (2, 3));
        let mut full = HistoryLog::default();
        full.reconcile(MAX_STEPS, 0, None);
        full.reconcile(MAX_STEPS, 0, Some("N".into()));
        assert_eq!(full.labels().len(), MAX_STEPS);
        assert_eq!(full.labels().last().map(String::as_str), Some("N"));
    }
}
