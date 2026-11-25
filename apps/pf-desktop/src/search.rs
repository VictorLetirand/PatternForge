//! Recherche d'objets de la feuille active (US-01.5) : liste filtrable par
//! nom, rôle, type et calque ; un clic sélectionne l'objet et cadre la vue.
//!
//! La logique ([`search`]) est pure et en lecture seule : aucune `Command`.
//!
//! Choix documentés :
//! - les objets d'un calque **masqué** sont **exclus** des résultats (ils ne
//!   sont pas sélectionnables, `Selection::retain_valid` les retirerait) ;
//! - le filtre de rôle ne porte que sur les tracés (les points, crans et
//!   cotations n'ont pas de rôle de tracé : ils sont écartés si un rôle est exigé) ;
//! - un objet dont la géométrie ne se résout pas (référence cassée) n'est pas listé ;
//! - un cran suit le calque de son tracé, une cotation celui de son premier point.

use crate::a11y::ComboExt;
use crate::app::App;
use crate::editor::{Selection, Target};
use crate::text::{entity_name, role_label};
use eframe::egui;
use pf_core::{EntityKind, LayerId, PathRole, Sheet};
use pf_geometry::Bbox;
use pf_i18n::tr;

/// Type d'objet d'un résultat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectKind {
    Segment,
    Curve,
    /// Cercle ou arc de cercle.
    Round,
    Notch,
    Dimension,
    Point,
}

/// Filtre de type : « Tracé » regroupe segments et courbes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KindFilter {
    Path,
    Segment,
    Curve,
    Round,
    Notch,
    Dimension,
    Point,
}

impl KindFilter {
    pub const ALL: [KindFilter; 7] = [
        KindFilter::Path,
        KindFilter::Segment,
        KindFilter::Curve,
        KindFilter::Round,
        KindFilter::Notch,
        KindFilter::Dimension,
        KindFilter::Point,
    ];

    fn matches(self, kind: ObjectKind) -> bool {
        match self {
            KindFilter::Path => matches!(
                kind,
                ObjectKind::Segment | ObjectKind::Curve | ObjectKind::Round
            ),
            KindFilter::Round => kind == ObjectKind::Round,
            KindFilter::Segment => kind == ObjectKind::Segment,
            KindFilter::Curve => kind == ObjectKind::Curve,
            KindFilter::Notch => kind == ObjectKind::Notch,
            KindFilter::Dimension => kind == ObjectKind::Dimension,
            KindFilter::Point => kind == ObjectKind::Point,
        }
    }

    pub fn label(self) -> String {
        match self {
            KindFilter::Path => tr("search.kind.path"),
            KindFilter::Segment => tr("entity.segment"),
            KindFilter::Curve => tr("entity.curve"),
            KindFilter::Round => tr("entity.round"),
            KindFilter::Notch => tr("entity.notch"),
            KindFilter::Dimension => tr("entity.dimension"),
            KindFilter::Point => tr("entity.point"),
        }
    }
}

/// Filtres combinés (ET). `None` = pas de filtre.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Filters {
    pub kind: Option<KindFilter>,
    pub role: Option<PathRole>,
    /// `Some(None)` = calque par défaut ; `Some(Some(id))` = ce calque.
    pub layer: Option<Option<LayerId>>,
}

/// Un objet trouvé.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchResult {
    pub target: Target,
    pub kind: ObjectKind,
    pub name: String,
    pub role: Option<PathRole>,
    pub layer: Option<LayerId>,
    /// Emprise de l'objet (mm), pour le cadrage.
    pub bbox: Bbox,
}

/// Minuscules sans accents : « Côté » et « cote » se valent.
pub fn fold(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars().flat_map(char::to_lowercase) {
        match c {
            // Échappements Unicode : le test « pas de français en dur » ignore ainsi ce tableau.
            '\u{e0}'..='\u{e5}' => out.push('a'),
            '\u{e7}' => out.push('c'),
            '\u{e8}'..='\u{eb}' => out.push('e'),
            '\u{ec}'..='\u{ef}' => out.push('i'),
            '\u{f1}' => out.push('n'),
            '\u{f2}'..='\u{f6}' => out.push('o'),
            '\u{f9}'..='\u{fc}' => out.push('u'),
            '\u{fd}' | '\u{ff}' => out.push('y'),
            '\u{153}' => out.push_str("oe"),
            '\u{e6}' => out.push_str("ae"),
            // Accents combinants (texte décomposé).
            '\u{300}'..='\u{36f}' => {}
            c => out.push(c),
        }
    }
    out
}

/// Libellé traduit d'un calque (`None` = calque par défaut).
pub fn layer_label(sheet: &Sheet, layer: Option<LayerId>) -> String {
    match layer.and_then(|l| sheet.layer(l).ok()) {
        Some(l) => l.name.clone(),
        None => tr("layers.default"),
    }
}

/// Objets visibles de la feuille qui satisfont la requête et les filtres, dans
/// l'ordre : tracés, crans, cotations, points. Requête vide = pas de filtre de nom.
pub fn search(sheet: &Sheet, query: &str, filters: &Filters) -> Vec<SearchResult> {
    let needle = fold(query.trim());
    let mut out = Vec::new();
    let mut push = |r: SearchResult, extra_names: &[&str]| {
        let name_ok = needle.is_empty()
            || fold(&r.name).contains(&needle)
            || extra_names.iter().any(|n| fold(n).contains(&needle));
        let kind_ok = filters.kind.is_none_or(|k| k.matches(r.kind));
        let role_ok = filters.role.is_none_or(|role| r.role == Some(role));
        let layer_ok = filters.layer.is_none_or(|l| r.layer == l);
        if name_ok && kind_ok && role_ok && layer_ok {
            out.push(r);
        }
    };

    for e in sheet
        .entities
        .iter()
        .filter(|e| sheet.layer_visible(e.layer))
    {
        let bbox = match e.kind {
            EntityKind::Segment { .. } => sheet.segment_ends(e.id).ok().map(|(a, b)| {
                let mut bb = Bbox::from_point(a);
                bb.include(b);
                bb
            }),
            EntityKind::Curve { .. } => sheet.curve_geometry(e.id).ok().map(|c| c.bbox()),
            EntityKind::Arc { .. } | EntityKind::Circle { .. } | EntityKind::Spline { .. } => {
                sheet.shape(e.id).ok().map(|s| s.bbox())
            }
        };
        let Some(bbox) = bbox else { continue };
        let generated = entity_name(sheet, e.id);
        let name = e.name.clone().unwrap_or_else(|| generated.clone());
        let kind = match e.kind {
            EntityKind::Segment { .. } => ObjectKind::Segment,
            EntityKind::Curve { .. } | EntityKind::Spline { .. } => ObjectKind::Curve,
            EntityKind::Arc { .. } | EntityKind::Circle { .. } => ObjectKind::Round,
        };
        push(
            SearchResult {
                target: Target::Entity(e.id),
                kind,
                name,
                role: Some(e.role),
                layer: e.layer,
                bbox,
            },
            &[&generated],
        );
    }

    for n in sheet.notches.iter().filter(|n| sheet.notch_visible(n)) {
        let Ok(marks) = sheet.notch_marks(n) else {
            continue;
        };
        let Some(first) = marks.first() else { continue };
        let mut bbox = Bbox::from_point(first.0);
        for (a, b) in &marks {
            bbox.include(*a);
            bbox.include(*b);
        }
        let layer = sheet.entity(n.entity).ok().and_then(|e| e.layer);
        push(
            SearchResult {
                target: Target::Notch(n.id),
                kind: ObjectKind::Notch,
                name: format!("{} — {}", tr("entity.notch"), entity_name(sheet, n.entity)),
                role: None,
                layer,
                bbox,
            },
            &[],
        );
    }

    for d in sheet
        .dimensions
        .iter()
        .filter(|d| sheet.dimension_visible(d))
    {
        let (Ok(a), Ok(b)) = (sheet.position(d.from), sheet.position(d.to)) else {
            continue;
        };
        let mut bbox = Bbox::from_point(a);
        bbox.include(b);
        push(
            SearchResult {
                target: Target::Dimension(d.id),
                kind: ObjectKind::Dimension,
                name: format!(
                    "{} {}{}",
                    tr("entity.dimension"),
                    sheet.label(d.from),
                    sheet.label(d.to)
                ),
                role: None,
                layer: sheet.point(d.from).ok().and_then(|p| p.layer),
                bbox,
            },
            &[],
        );
    }

    for p in sheet.points.iter().filter(|p| sheet.layer_visible(p.layer)) {
        let Ok(pos) = sheet.position(p.id) else {
            continue;
        };
        push(
            SearchResult {
                target: Target::Point(p.id),
                kind: ObjectKind::Point,
                name: format!("{} {}", tr("entity.point"), p.label),
                role: None,
                layer: p.layer,
                bbox: Bbox::from_point(pos),
            },
            &[],
        );
    }
    out
}

/// Fenêtre « Rechercher » : état d'édition (non enregistré).
#[derive(Debug, Clone, Default)]
pub struct SearchDialog {
    pub query: String,
    pub filters: Filters,
    /// Donner le focus au champ de saisie à la prochaine image.
    pub focus: bool,
}

impl App {
    /// Ouvre (ou ramène au premier plan) la fenêtre de recherche, champ actif.
    pub fn open_search(&mut self) {
        self.search.get_or_insert_with(SearchDialog::default).focus = true;
    }

    /// Cadre la vue sur une emprise (mm), sans toucher au modèle.
    pub fn focus_on(&mut self, bbox: Bbox) {
        let r = self.canvas_rect;
        self.view
            .fit(Some(bbox), (f64::from(r.width()), f64::from(r.height())));
    }

    /// Clic sur un résultat : l'objet devient la sélection, la vue le cadre.
    pub fn activate_search_result(&mut self, result: &SearchResult) {
        let sel: &mut Selection = &mut self.editor.selection;
        sel.clear();
        match result.target {
            Target::Point(id) => sel.points.push(id),
            Target::Entity(id) => sel.entities.push(id),
            Target::Notch(id) => sel.notches.push(id),
            Target::Dimension(id) => sel.dimensions.push(id),
            Target::Guide(id) => sel.guides.push(id),
            Target::Handle(..) => {}
        }
        self.focus_on(result.bbox);
    }

    pub fn search_window(&mut self, ctx: &egui::Context) {
        let Some(mut dialog) = self.search.take() else {
            return;
        };
        let mut open = true;
        let mut chosen: Option<SearchResult> = None;
        let sheet = self.editor.sheet().clone();
        egui::Window::new(tr("search.title"))
            .id(egui::Id::new("search_window"))
            .open(&mut open)
            .default_width(340.0)
            .show(ctx, |ui| {
                let field = crate::a11y::add_text(
                    ui,
                    egui::TextEdit::singleline(&mut dialog.query)
                        .id_salt("search_query")
                        .hint_text(tr("search.hint"))
                        .desired_width(f32::INFINITY),
                );
                if dialog.focus {
                    field.request_focus();
                    dialog.focus = false;
                }
                let any = tr("search.any");
                egui::ComboBox::from_id_salt("search_kind")
                    .selected_text(dialog.filters.kind.map_or(any.clone(), KindFilter::label))
                    .show_ui_named(ui, |ui| {
                        ui.selectable_value(&mut dialog.filters.kind, None, tr("search.any"));
                        for k in KindFilter::ALL {
                            ui.selectable_value(&mut dialog.filters.kind, Some(k), k.label());
                        }
                    });
                egui::ComboBox::from_id_salt("search_role")
                    .selected_text(dialog.filters.role.map_or(any.clone(), role_label))
                    .show_ui_named(ui, |ui| {
                        ui.selectable_value(&mut dialog.filters.role, None, tr("search.any"));
                        for r in PathRole::ALL {
                            ui.selectable_value(&mut dialog.filters.role, Some(r), role_label(r));
                        }
                    });
                egui::ComboBox::from_id_salt("search_layer")
                    .selected_text(dialog.filters.layer.map_or(any, |l| layer_label(&sheet, l)))
                    .show_ui_named(ui, |ui| {
                        ui.selectable_value(&mut dialog.filters.layer, None, tr("search.any"));
                        ui.selectable_value(
                            &mut dialog.filters.layer,
                            Some(None),
                            tr("layers.default"),
                        );
                        for l in &sheet.layers {
                            ui.selectable_value(
                                &mut dialog.filters.layer,
                                Some(Some(l.id)),
                                &l.name,
                            );
                        }
                    });
                ui.separator();
                let results = search(&sheet, &dialog.query, &dialog.filters);
                if results.is_empty() {
                    crate::a11y::label(ui, tr("search.none"));
                } else {
                    crate::a11y::label(
                        ui,
                        pf_i18n::tr_args("search.count", &[("n", &results.len().to_string())]),
                    );
                }
                egui::ScrollArea::vertical()
                    .max_height(
                        280.0_f32
                            .min(crate::panels::window_room(ctx) - 230.0)
                            .max(80.0),
                    )
                    .show(ui, |ui| {
                        for r in &results {
                            let mut text = r.name.clone();
                            if let Some(role) = r.role {
                                text.push_str(&format!(" · {}", role_label(role)));
                            }
                            text.push_str(&format!(" · {}", layer_label(&sheet, r.layer)));
                            if ui.selectable_label(false, text).clicked() {
                                chosen = Some(r.clone());
                            }
                        }
                    });
                ui.small(tr("search.hidden_note"));
            });
        if let Some(r) = chosen {
            self.activate_search_result(&r);
        }
        if open {
            self.search = Some(dialog);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pf_core::{demo_project, Command, NotchStyle, Project};
    use pf_geometry::Side;

    /// Démo : feuille « Devant » = segments AB et BC, courbe AC, point P (+ A, B, C).
    fn demo() -> Project {
        demo_project().expect("démo")
    }

    fn names(r: &[SearchResult]) -> Vec<String> {
        r.iter().map(|r| r.name.clone()).collect()
    }

    fn find(sheet: &Sheet, query: &str) -> Vec<SearchResult> {
        search(sheet, query, &Filters::default())
    }

    #[test]
    fn ca_filtre_par_nom() {
        let p = demo();
        let s = &p.sheets[0];
        assert_eq!(names(&find(s, "segment bc")), vec!["Segment BC"]);
        assert_eq!(names(&find(s, "  Courbe ")), vec!["Courbe AC"]);
        assert!(find(s, "introuvable").is_empty());
        // Requête vide : tout (3 tracés + 4 points).
        assert_eq!(find(s, "").len(), s.entities.len() + s.points.len());
    }

    #[test]
    fn ca_nom_personnalise_insensible_a_la_casse_et_aux_accents() {
        let mut p = demo();
        let sheet = p.sheets[0].id;
        let ab = p.sheets[0].entities[0].id;
        p.apply(Command::SetEntityName {
            sheet,
            entity: ab,
            name: Some("Côté Éventail".into()),
        })
        .expect("nom");
        let s = &p.sheets[0];
        for q in ["cote eventail", "CÔTÉ ÉVENTAIL", "eVeNtAiL", "côte"] {
            assert_eq!(names(&find(s, q)), vec!["Côté Éventail"], "{q}");
        }
        // Le nom généré reste cherchable.
        assert_eq!(names(&find(s, "segment ab")), vec!["Côté Éventail"]);
        // Texte décomposé (e + accent combinant).
        assert_eq!(fold("e\u{301}"), "e");
        assert_eq!(fold("Œuvre ÆON"), "oeuvre aeon");
    }

    #[test]
    fn ca_filtre_par_role() {
        let mut p = demo();
        let sheet = p.sheets[0].id;
        let bc = p.sheets[0].entities[1].id;
        p.apply(Command::SetPathRole {
            sheet,
            entity: bc,
            role: PathRole::CuttingContour,
        })
        .expect("rôle");
        let s = &p.sheets[0];
        let f = Filters {
            role: Some(PathRole::CuttingContour),
            ..Filters::default()
        };
        assert_eq!(search(s, "", &f).len(), 1);
        assert_eq!(search(s, "", &f)[0].target, Target::Entity(bc));
        let f = Filters {
            role: Some(PathRole::Construction),
            ..Filters::default()
        };
        // Les 2 autres tracés ; aucun point (pas de rôle de tracé).
        assert_eq!(search(s, "", &f).len(), 2);
        let f = Filters {
            role: Some(PathRole::Fold),
            ..Filters::default()
        };
        assert!(search(s, "", &f).is_empty());
    }

    #[test]
    fn ca_filtre_par_type() {
        let mut p = demo();
        let sheet = p.sheets[0].id;
        let ab = p.sheets[0].entities[0].id;
        p.apply(Command::AddNotch {
            sheet,
            entity: ab,
            distance_mm: 30.0,
            side: Side::Left,
            length_mm: 5.0,
            style: NotchStyle::Single,
        })
        .expect("cran");
        let (a, b) = (p.sheets[0].points[0].id, p.sheets[0].points[1].id);
        p.apply(Command::AddDimension {
            sheet,
            from: a,
            to: b,
            offset_mm: 10.0,
        })
        .expect("cotation");
        let s = &p.sheets[0];
        let count = |k| {
            search(
                s,
                "",
                &Filters {
                    kind: Some(k),
                    ..Filters::default()
                },
            )
            .len()
        };
        assert_eq!(count(KindFilter::Path), 3);
        assert_eq!(count(KindFilter::Segment), 2);
        assert_eq!(count(KindFilter::Curve), 1);
        assert_eq!(count(KindFilter::Notch), 1);
        assert_eq!(count(KindFilter::Dimension), 1);
        assert_eq!(count(KindFilter::Point), s.points.len());
        let notch = &search(
            s,
            "cran",
            &Filters {
                kind: Some(KindFilter::Notch),
                ..Filters::default()
            },
        )[0];
        assert!(matches!(notch.target, Target::Notch(_)));
    }

    #[test]
    fn ca_filtre_par_calque_et_calques_masques_exclus() {
        let mut p = demo();
        let sheet = p.sheets[0].id;
        let ab = p.sheets[0].entities[0].id;
        let layer = p
            .apply(Command::AddLayer {
                sheet,
                name: "Boutons".into(),
            })
            .expect("calque")
            .layers[0];
        p.apply(Command::SetObjectsLayer {
            sheet,
            points: vec![],
            entities: vec![ab],
            layer: Some(layer),
        })
        .expect("rangé");
        let on_layer = Filters {
            layer: Some(Some(layer)),
            ..Filters::default()
        };
        let default = Filters {
            layer: Some(None),
            ..Filters::default()
        };
        let s = &p.sheets[0];
        assert_eq!(search(s, "", &on_layer)[0].target, Target::Entity(ab));
        assert_eq!(search(s, "", &on_layer).len(), 1);
        assert_eq!(
            search(s, "", &default).len(),
            s.entities.len() - 1 + s.points.len()
        );
        // Calque masqué : exclu, même filtré dessus.
        p.apply(Command::SetLayerVisible {
            sheet,
            layer,
            visible: false,
        })
        .expect("masqué");
        let s = &p.sheets[0];
        assert!(search(s, "", &on_layer).is_empty());
        assert!(!find(s, "").iter().any(|r| r.target == Target::Entity(ab)));
    }
}
