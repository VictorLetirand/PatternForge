//! US-03.6 — historique de construction : fenêtre « Dépendances » du dock (menu Affichage) et
//! confirmation avant une suppression qui touche d'autres objets.
//!
//! L'arbre de l'objet sélectionné (parents, dépendants) est relu à chaque image par
//! `Project::dependency_tree` (calcul dérivé, jamais stocké, aucune `Command`). Direct par défaut ;
//! « Afficher tous les niveaux » ajoute les dépendances transitives. Les deux sections sont
//! **repliées par défaut** (le compteur de l'en-tête reste visible). Chaque ligne porte une
//! icône dessinée **et** un texte (la nature du lien), jamais la couleur seule ; un clic
//! sélectionne l'objet (feuille comprise, comme le diagnostic des liens).
//!
//! La confirmation de suppression est une boîte modale **seulement** quand
//! `Project::deletion_impact` annonce des objets supprimés en plus ou des liens retirés ; une
//! suppression sans dépendant reste immédiate. Aucun calcul métier ici : tout vient du cœur.
//!
//! Vocabulaire : « longueur imposée » ne désigne que les liens de ce type (courbe de Bézier).

use crate::app::App;
use crate::dock::ToolWin;
use crate::editor::Selection;
use crate::text::entity_name;
use eframe::egui::{self, RichText};
use pf_core::{
    DeletionImpact, DependencyEdge, DependencyEntry, DependencyNature, DependencyObject, Project,
    SheetId,
};
use pf_i18n::{tr, tr_args};
use pf_ui::components::{button, ButtonKind};

/// État de la fenêtre : niveaux affichés et sections dépliées (tout replié au départ).
#[derive(Debug, Clone, Copy, Default)]
pub struct DependenciesDialog {
    /// Tous les niveaux (transitifs) au lieu des seuls liens directs.
    pub all: bool,
    pub parents_open: bool,
    pub children_open: bool,
}

/// Lignes listées au plus dans la boîte de confirmation (la suite est comptée).
const MODAL_MAX_ROWS: usize = 12;

fn nature_label(n: DependencyNature) -> String {
    tr(match n {
        DependencyNature::Construction => "dep.nature.construction",
        DependencyNature::PathPoint => "dep.nature.path_point",
        DependencyNature::KeptConstruction => "dep.nature.kept_construction",
        DependencyNature::FormulaReference => "dep.nature.formula_reference",
        DependencyNature::VariableReference => "dep.nature.variable_reference",
        DependencyNature::ImposedLength => "dep.nature.imposed_length",
        DependencyNature::LengthEquality => "dep.nature.length_equality",
        DependencyNature::NotchReference => "dep.nature.notch_reference",
        DependencyNature::DimensionReference => "dep.nature.dimension_reference",
        DependencyNature::TopstitchSource => "dep.nature.topstitch_source",
        DependencyNature::TopstitchLine => "dep.nature.topstitch_line",
        DependencyNature::PieceOutline => "dep.nature.piece_outline",
        DependencyNature::PieceGrain => "dep.nature.piece_grain",
        DependencyNature::PieceFold => "dep.nature.piece_fold",
        DependencyNature::PieceInternal => "dep.nature.piece_internal",
        DependencyNature::PieceMarking => "dep.nature.piece_marking",
        DependencyNature::PieceDart => "dep.nature.piece_dart",
        DependencyNature::PieceButtons => "dep.nature.piece_buttons",
        DependencyNature::PieceDerived => "dep.nature.piece_derived",
        DependencyNature::SeamPair => "dep.nature.seam_pair",
        DependencyNature::SizeGrading => "dep.nature.size_grading",
    })
}

/// Feuille qui contient un point ou un tracé (les identifiants sont uniques dans le projet).
fn sheet_of_object(project: &Project, object: &DependencyObject) -> Option<SheetId> {
    project
        .sheets
        .iter()
        .find(|s| match object {
            DependencyObject::Point(p) => s.point(*p).is_ok(),
            DependencyObject::Entity(e) => s.entity(*e).is_ok(),
            DependencyObject::Notch(n) => s.notch(*n).is_ok(),
            DependencyObject::Dimension(d) => s.dimension(*d).is_ok(),
            DependencyObject::Guide(g) => s.guide(*g).is_ok(),
            DependencyObject::FullDimension(d) => s.full_dimension(*d).is_ok(),
            DependencyObject::Topstitch(t) => s.topstitches.iter().any(|x| x.id == *t),
            _ => false,
        })
        .map(|s| s.id)
}

/// Nom lisible d'un objet du graphe (préfixes traduits, étiquettes de points du modèle).
pub(crate) fn object_name(project: &Project, object: &DependencyObject) -> String {
    let sheet = sheet_of_object(project, object).and_then(|s| project.sheet(s).ok());
    match (object, sheet) {
        (DependencyObject::Point(p), Some(s)) => format!("{} {}", tr("entity.point"), s.label(*p)),
        (DependencyObject::Entity(e), Some(s)) => entity_name(s, *e),
        (DependencyObject::Notch(n), Some(s)) => {
            let host = s
                .notch(*n)
                .map(|n| entity_name(s, n.entity))
                .unwrap_or_default();
            format!(
                "{} {}",
                tr("entity.notch"),
                tr_args("piece.internal.notch_name", &[("name", &host)])
            )
        }
        (DependencyObject::Dimension(d), Some(s)) => match s.dimension(*d) {
            Ok(d) => format!(
                "{} {}{}",
                tr("entity.dimension"),
                s.label(d.from),
                s.label(d.to)
            ),
            Err(_) => tr("entity.dimension"),
        },
        (DependencyObject::Guide(_), _) => tr("dep.obj.guide"),
        (DependencyObject::FullDimension(_), _) => tr("dep.obj.fulldim"),
        (DependencyObject::Topstitch(_), _) => tr("dep.obj.topstitch"),
        (DependencyObject::Piece(id), _) => match project.piece(*id) {
            Ok(p) => tr_args("report.ref.piece", &[("name", &p.name)]),
            Err(_) => tr("dep.obj.missing"),
        },
        (DependencyObject::SeamPair(id), _) => {
            let n = project
                .seams
                .iter()
                .position(|s| s.id == *id)
                .map_or(0, |i| i + 1);
            tr_args("report.ref.pair", &[("n", &n.to_string())])
        }
        (DependencyObject::Variable(name), _) => {
            tr_args("dep.obj.variable", &[("name", name.as_str())])
        }
        (DependencyObject::Size(id), _) => match project.find_size(*id) {
            Some((r, i)) => tr_args("dep.obj.size", &[("name", &r.sizes[i].label)]),
            None => tr("dep.obj.missing"),
        },
        _ => tr("dep.obj.missing"),
    }
}

/// Objets de la sélection, dans l'ordre points, tracés, crans, cotations, lignes d'aide.
pub(crate) fn selection_objects(sel: &Selection) -> Vec<DependencyObject> {
    let mut out: Vec<DependencyObject> = Vec::new();
    out.extend(sel.points.iter().map(|p| DependencyObject::Point(*p)));
    out.extend(sel.entities.iter().map(|e| DependencyObject::Entity(*e)));
    out.extend(sel.notches.iter().map(|n| DependencyObject::Notch(*n)));
    out.extend(
        sel.dimensions
            .iter()
            .map(|d| DependencyObject::Dimension(*d)),
    );
    out.extend(sel.guides.iter().map(|g| DependencyObject::Guide(*g)));
    out
}

/// Flèche dessinée : vers le haut pour un parent, vers le bas pour un dépendant. Toujours suivie
/// du texte de la nature du lien.
fn arrow_icon(ui: &mut egui::Ui, up: bool) {
    let tokens = pf_ui::theme::tokens(ui.ctx());
    let stroke = egui::Stroke::new(1.6_f32, tokens.text_secondary);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(20.0, 18.0), egui::Sense::hover());
    let c = rect.center();
    let s = if up { 1.0_f32 } else { -1.0 };
    let p = ui.painter();
    p.line_segment(
        [c + egui::vec2(0.0, -6.0 * s), c + egui::vec2(0.0, 6.0 * s)],
        stroke,
    );
    p.line_segment(
        [
            c + egui::vec2(0.0, -6.0 * s),
            c + egui::vec2(-4.0, -2.0 * s),
        ],
        stroke,
    );
    p.line_segment(
        [c + egui::vec2(0.0, -6.0 * s), c + egui::vec2(4.0, -2.0 * s)],
        stroke,
    );
}

/// Triangle d'en-tête de section : plié = pointe à droite, déplié = pointe en bas.
fn fold_icon(ui: &mut egui::Ui, open: bool) {
    let tokens = pf_ui::theme::tokens(ui.ctx());
    let stroke = egui::Stroke::new(1.6_f32, tokens.text_secondary);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::hover());
    let c = rect.center();
    let pts: [egui::Pos2; 3] = if open {
        [
            c + egui::vec2(-4.0, -2.0),
            c + egui::vec2(4.0, -2.0),
            c + egui::vec2(0.0, 3.0),
        ]
    } else {
        [
            c + egui::vec2(-2.0, -4.0),
            c + egui::vec2(-2.0, 4.0),
            c + egui::vec2(3.0, 0.0),
        ]
    };
    let p = ui.painter();
    p.line_segment([pts[0], pts[1]], stroke);
    p.line_segment([pts[1], pts[2]], stroke);
    p.line_segment([pts[2], pts[0]], stroke);
}

fn entry_text(project: &Project, e: &DependencyEntry) -> String {
    let name = object_name(project, &e.object);
    let nature = nature_label(e.nature);
    if e.depth <= 1 {
        tr_args("dep.row", &[("name", &name), ("nature", &nature)])
    } else {
        tr_args(
            "dep.row.deep",
            &[
                ("name", &name),
                ("nature", &nature),
                ("depth", &e.depth.to_string()),
                ("via", &object_name(project, &e.via)),
            ],
        )
    }
}

impl App {
    /// Menu Affichage › « Dépendances » : ouvre (ou garde ouverte) la fenêtre.
    pub fn open_dependencies(&mut self) {
        if self.dependencies.is_none() {
            self.dependencies = Some(DependenciesDialog::default());
        }
    }

    /// Clic sur une ligne : feuille de l'objet active, objet sélectionné, vue cadrée. Une variable
    /// ouvre « Variables » ; un objet sans sélection possible dans le plan (pièce, couture…)
    /// ne fait que changer de feuille.
    pub fn activate_dependency(&mut self, object: &DependencyObject, sheet: Option<SheetId>) {
        if matches!(object, DependencyObject::Variable(_)) {
            self.open_variables();
            return;
        }
        // Une taille (US-09.2a) n'est pas dans le plan : sa fenêtre est celle de la gradation.
        if matches!(object, DependencyObject::Size(_)) {
            self.open_grading();
            return;
        }
        let Some(sheet_id) = sheet else {
            return;
        };
        if self.plan.is_some() {
            self.leave_plan();
        }
        self.editor.set_sheet(sheet_id);
        self.editor.selection.clear();
        let focus = match object {
            DependencyObject::Point(p) => {
                self.editor.selection.points.push(*p);
                Some(pf_core::DiagnosticObject::Point(*p))
            }
            DependencyObject::Entity(e) => {
                self.editor.selection.entities.push(*e);
                Some(pf_core::DiagnosticObject::Entity(*e))
            }
            DependencyObject::Notch(n) => {
                self.editor.selection.notches.push(*n);
                None
            }
            DependencyObject::Dimension(d) => {
                self.editor.selection.dimensions.push(*d);
                None
            }
            DependencyObject::Guide(g) => {
                self.editor.selection.guides.push(*g);
                None
            }
            _ => None,
        };
        let bbox = focus
            .as_ref()
            .and_then(|o| crate::diagnostic::object_bbox(self.editor.sheet(), o));
        if let Some(bbox) = bbox {
            self.focus_on(bbox);
        }
    }

    /// Suppression de la sélection : immédiate si elle n'a pas de conséquence sur d'autres
    /// objets (ou si la commande serait refusée : le message habituel s'affiche), sinon la
    /// boîte de confirmation est ouverte (`pending_delete`). L'annulation reste en une étape.
    pub fn request_delete(&mut self) {
        if self.pending_delete.is_some() {
            return;
        }
        if self.editor.dart_selected.is_some() || self.editor.selection.is_empty() {
            self.editor.delete_selection();
            return;
        }
        let objects = selection_objects(&self.editor.selection);
        let sheet = self.editor.sheet;
        let impact = self.editor.project().deletion_impact(sheet, &objects);
        if impact.refused.is_some() || impact.is_trivial() {
            self.editor.delete_selection();
        } else {
            self.pending_delete = Some(impact);
        }
    }

    /// Confirmation : la même suppression qu'avant, en une étape annulable.
    pub fn confirm_delete(&mut self) {
        if self.pending_delete.take().is_some() {
            self.editor.delete_selection();
        }
    }

    /// Contenu de la boîte de confirmation (appelé dans la boîte modale de `dialogs`).
    pub(crate) fn delete_modal_rows(&self, ui: &mut egui::Ui, impact: &DeletionImpact) {
        let project = self.editor.project();
        let secondary = pf_ui::theme::tokens(ui.ctx()).text_secondary;
        crate::a11y::label(
            ui,
            tr_args(
                "dep.delete.body",
                &[("count", &impact.requested.len().to_string())],
            ),
        );
        let rows = |ui: &mut egui::Ui, texts: Vec<String>| {
            for text in texts.iter().take(MODAL_MAX_ROWS) {
                ui.horizontal(|ui| {
                    arrow_icon(ui, false);
                    crate::a11y::label(ui, text);
                });
            }
            if texts.len() > MODAL_MAX_ROWS {
                crate::a11y::label(
                    ui,
                    RichText::new(tr_args(
                        "dep.delete.more",
                        &[("count", &(texts.len() - MODAL_MAX_ROWS).to_string())],
                    ))
                    .color(secondary),
                );
            }
        };
        if !impact.removed.is_empty() {
            ui.add_space(6.0);
            crate::a11y::label(
                ui,
                RichText::new(tr_args(
                    "dep.delete.removed",
                    &[("count", &impact.removed.len().to_string())],
                ))
                .strong(),
            );
            rows(
                ui,
                impact
                    .removed
                    .iter()
                    .map(|o| object_name(project, o))
                    .collect(),
            );
        }
        if !impact.unlinked.is_empty() {
            ui.add_space(6.0);
            crate::a11y::label(
                ui,
                RichText::new(tr_args(
                    "dep.delete.unlinked",
                    &[("count", &impact.unlinked.len().to_string())],
                ))
                .strong(),
            );
            rows(
                ui,
                impact
                    .unlinked
                    .iter()
                    .map(|e: &DependencyEdge| {
                        tr_args(
                            "dep.delete.unlinked.row",
                            &[
                                ("child", &object_name(project, &e.child)),
                                ("nature", &nature_label(e.nature)),
                                ("parent", &object_name(project, &e.parent)),
                            ],
                        )
                    })
                    .collect(),
            );
        }
        ui.add_space(8.0);
    }

    pub fn dependencies_window(&mut self, ctx: &egui::Context) {
        let Some(mut dialog) = self.dependencies.take() else {
            return;
        };
        let project = self.editor.project();
        let selected = selection_objects(&self.editor.selection);
        let tree = selected
            .first()
            .and_then(|o| project.dependency_tree(o, None));
        let shown = |entries: &[DependencyEntry], all: bool| -> Vec<(DependencyEntry, String)> {
            entries
                .iter()
                .filter(|e| all || e.depth <= 1)
                .map(|e| (e.clone(), entry_text(project, e)))
                .collect()
        };
        let parents = tree
            .as_ref()
            .map(|t| shown(&t.parents, dialog.all))
            .unwrap_or_default();
        let children = tree
            .as_ref()
            .map(|t| shown(&t.children, dialog.all))
            .unwrap_or_default();
        let hidden = tree.as_ref().map_or(0, |t| {
            t.parents.len() + t.children.len() - parents.len() - children.len()
        });
        let title = selected.first().map(|o| object_name(project, o));
        let secondary = pf_ui::theme::tokens(ctx).text_secondary;
        let mut open = true;
        let mut close = false;
        let mut clicked: Option<(DependencyObject, Option<SheetId>)> = None;
        let slot = crate::dock::slot(ctx, &self.dock, ToolWin::Dependencies);
        let width = self.dock.content_width(ToolWin::Dependencies);
        egui::Window::new(tr("dep.title"))
            .id(ToolWin::Dependencies.id())
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
                        crate::a11y::label(ui, RichText::new(tr("dep.intro")).color(secondary));
                        ui.add_space(4.0);
                        match (&tree, &title) {
                            (Some(_), Some(name)) => {
                                crate::a11y::label(
                                    ui,
                                    RichText::new(tr_args("dep.object", &[("name", name)]))
                                        .strong(),
                                );
                                if selected.len() > 1 {
                                    crate::a11y::label(
                                        ui,
                                        RichText::new(tr_args(
                                            "dep.more",
                                            &[("count", &(selected.len() - 1).to_string())],
                                        ))
                                        .color(secondary),
                                    );
                                }
                                ui.checkbox(&mut dialog.all, tr("dep.all"));
                                if hidden > 0 {
                                    crate::a11y::label(
                                        ui,
                                        RichText::new(tr_args(
                                            "dep.hidden",
                                            &[("count", &hidden.to_string())],
                                        ))
                                        .color(secondary),
                                    );
                                }
                                for (is_parents, list, open_flag) in [
                                    (true, &parents, &mut dialog.parents_open),
                                    (false, &children, &mut dialog.children_open),
                                ] {
                                    ui.add_space(6.0);
                                    let header = tr_args(
                                        if is_parents {
                                            "dep.parents"
                                        } else {
                                            "dep.children"
                                        },
                                        &[("count", &list.len().to_string())],
                                    );
                                    ui.horizontal(|ui| {
                                        fold_icon(ui, *open_flag);
                                        let r = ui.add(
                                            egui::Button::new(RichText::new(header).strong())
                                                .frame(false),
                                        );
                                        if r.on_hover_cursor(egui::CursorIcon::PointingHand)
                                            .clicked()
                                        {
                                            *open_flag = !*open_flag;
                                        }
                                    });
                                    if !*open_flag {
                                        continue;
                                    }
                                    if list.is_empty() {
                                        crate::a11y::label(
                                            ui,
                                            RichText::new(tr(if is_parents {
                                                "dep.none.parents"
                                            } else {
                                                "dep.none.children"
                                            }))
                                            .color(secondary),
                                        );
                                    }
                                    for (entry, text) in list {
                                        ui.horizontal(|ui| {
                                            ui.add_space(
                                                12.0 * entry.depth.saturating_sub(1) as f32,
                                            );
                                            arrow_icon(ui, is_parents);
                                            let response = ui
                                                .add(
                                                    egui::Button::new(text.as_str())
                                                        .frame(false)
                                                        .wrap(),
                                                )
                                                .on_hover_cursor(egui::CursorIcon::PointingHand);
                                            if response.clicked() {
                                                clicked = Some((entry.object.clone(), entry.sheet));
                                            }
                                        });
                                    }
                                }
                            }
                            _ => {
                                crate::a11y::label(ui, RichText::new(tr("dep.empty")).strong());
                                crate::a11y::label(
                                    ui,
                                    RichText::new(tr("dep.empty.hint")).color(secondary),
                                );
                            }
                        }
                        ui.add_space(6.0);
                        close = button(ui, &tr("dialog.close"), ButtonKind::Secondary).clicked();
                    });
            });
        self.dependencies = Some(dialog);
        if let Some((object, sheet)) = clicked {
            self.activate_dependency(&object, sheet);
        }
        if close || !open {
            self.dependencies = None;
        }
    }
}
