//! Sélection avancée (US-16.1) : tout sélectionner, inverser, lasso, sélection
//! par critère (rôle, calque, type), par chaîne, et filtres persistants.
//!
//! Le modèle de sélection est purement interface : aucune de ces opérations ne
//! passe par une `Command`, donc ni l'historique ni l'état « modifié » du
//! projet ne bougent. Le filtre et le mode lasso sont des états d'éditeur,
//! jamais enregistrés dans le fichier.
//!
//! Choix documentés :
//! - ces opérations n'atteignent que les objets **visibles** (calque non
//!   masqué), **non verrouillés** (calque non verrouillé) et autorisés par le
//!   filtre ; les lignes d'aide seulement si elles sont affichées ;
//! - un membre de groupe entraîne son groupe (dans la limite des objets
//!   atteignables) ;
//! - le lasso et la fenêtre diffèrent sur un point : la fenêtre historique
//!   atteint aussi les calques verrouillés (comme le clic) ;
//! - « Inverser » est une involution pour toute sélection fermée par groupe
//!   (ce que produisent toutes ces opérations) ; une sélection partielle d'un
//!   groupe (Alt + clic) est complétée à l'inversion ;
//! - filtre entièrement décoché : plus rien n'est sélectionnable (autorisé,
//!   signalé dans la barre d'état).

use crate::app::App;
use crate::editor::{Editor, Selection, Target};
use crate::search::{search, Filters, KindFilter};
use crate::text::role_label;
use eframe::egui;
use pf_core::offset::detect_chain;
use pf_core::{
    DimensionId, EntityId, EntityKind, GuideId, LayerId, NotchId, PathRole, PointId, Sheet,
};
use pf_geometry::{Lasso, PointMm};
use pf_i18n::{tr, tr_args};
use std::collections::{HashMap, HashSet};
use std::hash::Hash;

/// Nombre de segments d'échantillonnage d'un tracé courbe pour le lasso.
const LASSO_SAMPLES: usize = 256;

/// Type d'objet, pour « sélectionner tout ce qui est… ».
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectType {
    Segment,
    Curve,
    Circle,
    Arc,
    /// Courbe lisse par points de passage.
    Spline,
    Point,
    Notch,
    Dimension,
    Guide,
}

impl ObjectType {
    pub const ALL: [ObjectType; 9] = [
        ObjectType::Segment,
        ObjectType::Curve,
        ObjectType::Circle,
        ObjectType::Arc,
        ObjectType::Spline,
        ObjectType::Point,
        ObjectType::Notch,
        ObjectType::Dimension,
        ObjectType::Guide,
    ];

    pub fn label(self) -> String {
        match self {
            ObjectType::Segment => tr("entity.segment"),
            ObjectType::Curve => tr("entity.curve"),
            ObjectType::Circle => tr("entity.circle"),
            ObjectType::Arc => tr("entity.arc"),
            ObjectType::Spline => tr("entity.spline"),
            ObjectType::Point => tr("entity.point"),
            ObjectType::Notch => tr("entity.notch"),
            ObjectType::Dimension => tr("entity.dimension"),
            ObjectType::Guide => tr("inspector.guide.title"),
        }
    }

    /// Filtre de recherche le plus proche (le type exact est revérifié ensuite).
    fn coarse(self) -> Option<KindFilter> {
        match self {
            ObjectType::Segment => Some(KindFilter::Segment),
            ObjectType::Curve | ObjectType::Spline => Some(KindFilter::Curve),
            ObjectType::Circle | ObjectType::Arc => Some(KindFilter::Round),
            ObjectType::Point => Some(KindFilter::Point),
            ObjectType::Notch => Some(KindFilter::Notch),
            ObjectType::Dimension => Some(KindFilter::Dimension),
            ObjectType::Guide => None,
        }
    }

    fn matches_entity(self, kind: &EntityKind) -> bool {
        matches!(
            (self, kind),
            (ObjectType::Segment, EntityKind::Segment { .. })
                | (ObjectType::Curve, EntityKind::Curve { .. })
                | (ObjectType::Circle, EntityKind::Circle { .. })
                | (ObjectType::Arc, EntityKind::Arc { .. })
                | (ObjectType::Spline, EntityKind::Spline { .. })
        )
    }
}

/// Critère de « sélectionner tout ce qui est… ».
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Criterion {
    /// Rôle de tracé (les points, crans, cotations et guides n'en ont pas).
    Role(PathRole),
    /// Calque (`None` = calque par défaut).
    Layer(Option<LayerId>),
    Kind(ObjectType),
}

/// Catégories d'objets que le clic, la fenêtre, le lasso, « Tout » et
/// « Inverser » peuvent atteindre. Tout est coché par défaut.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelectionFilter {
    pub points: bool,
    /// Tracés : segments, courbes, cercles, arcs, courbes lisses.
    pub paths: bool,
    pub notches: bool,
    pub dimensions: bool,
    pub guides: bool,
}

impl Default for SelectionFilter {
    fn default() -> Self {
        SelectionFilter {
            points: true,
            paths: true,
            notches: true,
            dimensions: true,
            guides: true,
        }
    }
}

impl SelectionFilter {
    fn flags(self) -> [bool; 5] {
        [
            self.points,
            self.paths,
            self.notches,
            self.dimensions,
            self.guides,
        ]
    }

    /// Nombre de catégories atteignables (sur 5).
    pub fn enabled_count(self) -> usize {
        self.flags().iter().filter(|f| **f).count()
    }

    pub fn is_all(self) -> bool {
        self.enabled_count() == 5
    }

    pub fn is_none(self) -> bool {
        self.enabled_count() == 0
    }
}

/// Échec de la sélection par chaîne : la sélection reste inchangée.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChainError {
    /// Aucun tracé sélectionné (ou sous le curseur) pour partir.
    NoPath,
    /// Tracé inexistant ou à la géométrie invalide.
    Unknown,
    /// La chaîne se divise (plus de deux tracés à un même point).
    Ambiguous,
    /// Aucun tracé de la chaîne n'est atteignable (masqué, verrouillé, filtré).
    Nothing,
}

impl ChainError {
    pub fn message(self) -> String {
        match self {
            ChainError::NoPath => tr("status.select.chain.none"),
            ChainError::Unknown => tr("status.select.chain.unknown"),
            ChainError::Ambiguous => tr("status.select.chain.ambiguous"),
            ChainError::Nothing => tr("status.select.chain.nothing"),
        }
    }
}

/// Objets atteignables, par identifiant (recherche rapide).
#[derive(Default)]
struct Allowed {
    points: HashSet<PointId>,
    entities: HashSet<EntityId>,
    notches: HashSet<NotchId>,
    dimensions: HashSet<DimensionId>,
    guides: HashSet<GuideId>,
}

impl Allowed {
    fn of(s: &Selection) -> Allowed {
        Allowed {
            points: s.points.iter().copied().collect(),
            entities: s.entities.iter().copied().collect(),
            notches: s.notches.iter().copied().collect(),
            dimensions: s.dimensions.iter().copied().collect(),
            guides: s.guides.iter().copied().collect(),
        }
    }
}

/// Éléments de `all` absents de `current`, dans l'ordre de `all`.
fn minus<T: Copy + Eq + Hash>(all: &[T], current: &[T]) -> Vec<T> {
    let current: HashSet<T> = current.iter().copied().collect();
    all.iter()
        .copied()
        .filter(|x| !current.contains(x))
        .collect()
}

fn merge<T: Copy + Eq + Hash>(into: &mut Vec<T>, add: Vec<T>) {
    let mut seen: HashSet<T> = into.iter().copied().collect();
    for x in add {
        if seen.insert(x) {
            into.push(x);
        }
    }
}

impl Editor {
    /// Tout ce que les opérations de sélection avancée peuvent atteindre :
    /// visible, non verrouillé, autorisé par le filtre (guides : s'ils sont affichés).
    pub fn selectable(&self) -> Selection {
        let sheet = self.sheet();
        let f = self.select_filter;
        let mut s = Selection::default();
        if f.points {
            s.points = sheet
                .points
                .iter()
                .filter(|p| sheet.layer_visible(p.layer) && !sheet.layer_locked(p.layer))
                .map(|p| p.id)
                .collect();
        }
        if f.paths {
            s.entities = sheet
                .entities
                .iter()
                .filter(|e| sheet.layer_visible(e.layer) && !sheet.layer_locked(e.layer))
                .map(|e| e.id)
                .collect();
        }
        if f.notches {
            s.notches = sheet
                .notches
                .iter()
                .filter(|n| sheet.notch_visible(n) && !sheet.entity_locked(n.entity))
                .map(|n| n.id)
                .collect();
        }
        if f.dimensions {
            s.dimensions = sheet
                .dimensions
                .iter()
                .filter(|d| {
                    sheet.dimension_visible(d)
                        && !sheet.point_locked(d.from)
                        && !sheet.point_locked(d.to)
                })
                .map(|d| d.id)
                .collect();
        }
        if f.guides && self.show_guides {
            s.guides = sheet
                .guides
                .iter()
                .filter(|g| sheet.guide_visible(g) && !sheet.guide_locked(g))
                .map(|g| g.id)
                .collect();
        }
        s
    }

    /// Range le résultat dans la sélection (remplace, ou ajoute si `additive`),
    /// après fermeture par groupe dans la limite des objets atteignables.
    fn commit(&mut self, mut found: Selection, universe: &Selection, additive: bool) {
        let allowed = Allowed::of(universe);
        found.entities = self
            .sheet()
            .expand_groups(&found.entities)
            .into_iter()
            .filter(|e| allowed.entities.contains(e))
            .collect();
        if !additive {
            self.selection.clear();
        }
        let sel = &mut self.selection;
        merge(&mut sel.points, found.points);
        merge(&mut sel.entities, found.entities);
        merge(&mut sel.notches, found.notches);
        merge(&mut sel.dimensions, found.dimensions);
        merge(&mut sel.guides, found.guides);
    }

    fn report_count(&mut self) -> usize {
        let n = self.selection.len();
        self.info(tr_args("status.select.count", &[("n", &n.to_string())]));
        n
    }

    /// Tout sélectionner sur la feuille active. Retourne le nombre d'objets.
    pub fn select_all(&mut self) -> usize {
        let all = self.selectable();
        self.commit(all.clone(), &all, false);
        self.report_count()
    }

    /// Inverser la sélection : les objets atteignables qui n'étaient pas
    /// sélectionnés. Retourne le nombre d'objets sélectionnés ensuite.
    pub fn invert_selection(&mut self) -> usize {
        let all = self.selectable();
        let cur = &self.selection;
        let found = Selection {
            points: minus(&all.points, &cur.points),
            entities: minus(&all.entities, &cur.entities),
            notches: minus(&all.notches, &cur.notches),
            dimensions: minus(&all.dimensions, &cur.dimensions),
            guides: minus(&all.guides, &cur.guides),
        };
        self.commit(found, &all, false);
        self.report_count()
    }

    /// « Sélectionner tout ce qui est… » (rôle, calque, type). Retourne le
    /// nombre d'objets trouvés (0 : la sélection est alors inchangée si
    /// `additive`, vidée sinon).
    pub fn select_where(&mut self, criterion: Criterion, additive: bool) -> usize {
        let all = self.selectable();
        let allowed = Allowed::of(&all);
        let sheet = self.sheet();
        let mut found = Selection::default();
        let mut filters = Filters::default();
        match criterion {
            Criterion::Role(r) => filters.role = Some(r),
            Criterion::Layer(l) => filters.layer = Some(l),
            Criterion::Kind(k) => filters.kind = k.coarse(),
        }
        // Les lignes d'aide ne figurent pas dans la recherche : traitées à part.
        let guide_hit = |g: &pf_core::Guide| match criterion {
            Criterion::Role(_) => false,
            Criterion::Layer(l) => g.layer == l,
            Criterion::Kind(k) => k == ObjectType::Guide,
        };
        let skip_search = criterion == Criterion::Kind(ObjectType::Guide);
        if !skip_search {
            for r in search(sheet, "", &filters) {
                match r.target {
                    Target::Point(id) if allowed.points.contains(&id) => found.points.push(id),
                    Target::Entity(id) if allowed.entities.contains(&id) => {
                        let exact = match criterion {
                            Criterion::Kind(k) => {
                                sheet.entity(id).is_ok_and(|e| k.matches_entity(&e.kind))
                            }
                            _ => true,
                        };
                        if exact {
                            found.entities.push(id);
                        }
                    }
                    Target::Notch(id) if allowed.notches.contains(&id) => found.notches.push(id),
                    Target::Dimension(id) if allowed.dimensions.contains(&id) => {
                        found.dimensions.push(id)
                    }
                    _ => {}
                }
            }
        }
        found.guides = sheet
            .guides
            .iter()
            .filter(|g| allowed.guides.contains(&g.id) && guide_hit(g))
            .map(|g| g.id)
            .collect();
        self.commit(found, &all, additive);
        let n = self.report_count();
        if n == 0 {
            self.info(tr("search.none"));
        }
        n
    }

    /// Sélection au lasso. `touch` : retient aussi les objets que le lasso
    /// croise ; sinon l'objet doit être entièrement dedans. Un polygone
    /// dégénéré (moins de 3 sommets distincts, alignés, non fini) ne
    /// sélectionne rien : la sélection est vidée, ou inchangée si `additive`.
    pub fn select_lasso(&mut self, polygon: &[PointMm], additive: bool, touch: bool) -> usize {
        let Some(lasso) = Lasso::new(polygon) else {
            if !additive {
                self.selection.clear();
            }
            self.info(tr("status.select.lasso_degenerate"));
            return 0;
        };
        let all = self.selectable();
        let sheet = self.sheet();
        let hit = |pts: &[PointMm]| {
            if touch {
                lasso.touches_polyline(pts)
            } else {
                lasso.contains_polyline(pts)
            }
        };
        let mut found = Selection::default();
        for &id in &all.points {
            if sheet.position(id).is_ok_and(|p| lasso.contains_point(p)) {
                found.points.push(id);
            }
        }
        for &id in &all.entities {
            if let Ok(shape) = sheet.shape(id) {
                if hit(&shape.sample(LASSO_SAMPLES)) {
                    found.entities.push(id);
                }
            }
        }
        for n in sheet.notches.iter().filter(|n| all.notches.contains(&n.id)) {
            // Un cran en coin suit la bissectrice de son sommet (traits tels que dessinés).
            let marks = if n.corner {
                self.project()
                    .notch_placed_marks(self.sheet, n)
                    .map(|(_, m)| m)
            } else {
                sheet.notch_marks(n)
            };
            let Ok(marks) = marks else {
                continue;
            };
            let inside = if touch {
                marks.iter().any(|(a, b)| hit(&[*a, *b]))
            } else {
                !marks.is_empty() && marks.iter().all(|(a, b)| hit(&[*a, *b]))
            };
            if inside {
                found.notches.push(n.id);
            }
        }
        for d in sheet
            .dimensions
            .iter()
            .filter(|d| all.dimensions.contains(&d.id))
        {
            let Ok(g) = sheet.dimension_geometry(d.id) else {
                continue;
            };
            if hit(&[g.from, g.from_offset, g.to_offset, g.to]) {
                found.dimensions.push(d.id);
            }
        }
        // Une ligne d'aide est infinie : jamais « entièrement dans » un lasso.
        self.commit(found, &all, additive);
        self.report_count()
    }

    /// Chaîne connexe du tracé `id` (tracés bout à bout), refusée si elle se
    /// divise en un point.
    fn chain_of(&self, id: EntityId) -> Result<Vec<EntityId>, ChainError> {
        let sheet = self.sheet();
        if sheet.entity(id).is_err() {
            return Err(ChainError::Unknown);
        }
        let chain = detect_chain(sheet, id).map_err(|_| ChainError::Unknown)?;
        let mut degree: HashMap<PointId, usize> = HashMap::new();
        for e in &sheet.entities {
            if matches!(e.kind, EntityKind::Circle { .. }) {
                continue;
            }
            let (a, b) = sheet.entity_points(e.id).map_err(|_| ChainError::Unknown)?;
            *degree.entry(a).or_insert(0) += 1;
            if b != a {
                *degree.entry(b).or_insert(0) += 1;
            }
        }
        for &m in &chain {
            if is_circle(sheet, m) {
                continue;
            }
            let (a, b) = sheet.entity_points(m).map_err(|_| ChainError::Unknown)?;
            if [a, b]
                .iter()
                .any(|p| degree.get(p).copied().unwrap_or(0) > 2)
            {
                return Err(ChainError::Ambiguous);
            }
        }
        Ok(chain)
    }

    /// Sélectionne les tracés atteignables de `chain` ; signale l'échec sans toucher à la sélection.
    fn select_chain_members(
        &mut self,
        chain: Vec<EntityId>,
        additive: bool,
    ) -> Result<usize, ChainError> {
        let all = self.selectable();
        let allowed = Allowed::of(&all);
        let entities: Vec<EntityId> = chain
            .into_iter()
            .filter(|e| allowed.entities.contains(e))
            .collect();
        if entities.is_empty() {
            return Err(ChainError::Nothing);
        }
        let found = Selection {
            entities,
            ..Selection::default()
        };
        self.commit(found, &all, additive);
        Ok(self.report_count())
    }

    /// Sélection par chaîne à partir du tracé `id`. Échec : message d'erreur
    /// affiché, sélection inchangée.
    pub fn select_chain_of(&mut self, id: EntityId, additive: bool) -> Result<usize, ChainError> {
        let result = self
            .chain_of(id)
            .and_then(|chain| self.select_chain_members(chain, additive));
        if let Err(e) = result {
            self.error(e.message());
        }
        result
    }

    /// Sélection par chaîne à partir des tracés sélectionnés (union de leurs
    /// chaînes). Un échec sur l'un d'eux laisse la sélection inchangée.
    pub fn select_chain(&mut self, additive: bool) -> Result<usize, ChainError> {
        let starts = self.selection.entities.clone();
        let result = if starts.is_empty() {
            Err(ChainError::NoPath)
        } else {
            starts
                .iter()
                .try_fold(
                    Vec::new(),
                    |mut acc, id| -> Result<Vec<EntityId>, ChainError> {
                        merge(&mut acc, self.chain_of(*id)?);
                        Ok(acc)
                    },
                )
                .and_then(|chain| self.select_chain_members(chain, additive))
        };
        if let Err(e) = result {
            self.error(e.message());
        }
        result
    }

    /// Double-clic : chaîne du tracé sous le curseur.
    pub fn select_chain_at(
        &mut self,
        world: PointMm,
        tol_mm: f64,
        additive: bool,
    ) -> Result<usize, ChainError> {
        match self.hit_test(world, tol_mm) {
            Some(Target::Entity(id)) => self.select_chain_of(id, additive),
            _ => {
                self.error(ChainError::NoPath.message());
                Err(ChainError::NoPath)
            }
        }
    }

    /// Change le filtre ; ce qui sort du filtre est désélectionné.
    pub fn set_select_filter(&mut self, filter: SelectionFilter) {
        self.select_filter = filter;
        let sel = &mut self.selection;
        if !filter.points {
            sel.points.clear();
        }
        if !filter.paths {
            sel.entities.clear();
        }
        if !filter.notches {
            sel.notches.clear();
        }
        if !filter.dimensions {
            sel.dimensions.clear();
        }
        if !filter.guides {
            sel.guides.clear();
        }
        let text = if filter.is_none() {
            tr("status.select.filter_none")
        } else if filter.is_all() {
            tr("status.select.filter_all")
        } else {
            tr_args(
                "status.select.filter_active",
                &[("n", &filter.enabled_count().to_string())],
            )
        };
        self.info(text);
    }

    /// Bascule entre fenêtre rectangulaire et lasso pour le glisser de l'outil Sélection.
    pub fn set_lasso_mode(&mut self, on: bool) {
        self.lasso_mode = on;
        self.info(if on {
            tr("status.select.lasso_on")
        } else {
            tr("status.select.lasso_off")
        });
    }
}

impl App {
    /// Sous-menu « Édition > Sélection » : tout, inverser, chaîne, par rôle /
    /// calque / type (Maj enfoncée : ajoute), mode lasso et filtres.
    pub fn selection_menu(&mut self, ui: &mut egui::Ui) {
        let shift = ui.input(|i| i.modifiers.shift);
        let layers: Vec<(LayerId, String)> = self
            .editor
            .sheet()
            .layers
            .iter()
            .map(|l| (l.id, l.name.clone()))
            .collect();
        ui.menu_button(tr("menu.edit.select"), |ui| {
            let item = |ui: &mut egui::Ui, text: String, shortcut: &str| {
                ui.add(egui::Button::new(text).shortcut_text(shortcut))
                    .clicked()
            };
            if item(ui, tr("menu.select.all"), "Ctrl+A") {
                self.editor.select_all();
                ui.close();
            }
            if item(ui, tr("menu.select.invert"), "Ctrl+I") {
                self.editor.invert_selection();
                ui.close();
            }
            let chain_keys = format!("Ctrl+{}+A", tr("key.shift"));
            let can_chain = !self.editor.selection.entities.is_empty();
            if ui
                .add_enabled(
                    can_chain,
                    egui::Button::new(tr("menu.select.chain")).shortcut_text(chain_keys),
                )
                .clicked()
            {
                let _ = self.editor.select_chain(true);
                ui.close();
            }
            ui.separator();
            ui.menu_button(tr("menu.select.by_role"), |ui| {
                for role in PathRole::ALL {
                    if ui.button(role_label(role)).clicked() {
                        self.editor.select_where(Criterion::Role(role), shift);
                        ui.close();
                    }
                }
            });
            ui.menu_button(tr("menu.select.by_layer"), |ui| {
                if ui.button(tr("layers.default")).clicked() {
                    self.editor.select_where(Criterion::Layer(None), shift);
                    ui.close();
                }
                for (id, name) in &layers {
                    if ui.button(name).clicked() {
                        self.editor.select_where(Criterion::Layer(Some(*id)), shift);
                        ui.close();
                    }
                }
            });
            ui.menu_button(tr("menu.select.by_type"), |ui| {
                for kind in ObjectType::ALL {
                    if ui.button(kind.label()).clicked() {
                        self.editor.select_where(Criterion::Kind(kind), shift);
                        ui.close();
                    }
                }
            });
            ui.small(tr("menu.select.add_note"));
            ui.separator();
            let mut lasso = self.editor.lasso_mode;
            if ui.checkbox(&mut lasso, tr("menu.select.lasso")).changed() {
                self.editor.set_lasso_mode(lasso);
            }
            ui.menu_button(tr("menu.select.filters"), |ui| {
                let mut f = self.editor.select_filter;
                let mut changed = false;
                changed |= ui
                    .checkbox(&mut f.points, tr("menu.select.filter.points"))
                    .changed();
                changed |= ui
                    .checkbox(&mut f.paths, tr("menu.select.filter.paths"))
                    .changed();
                changed |= ui
                    .checkbox(&mut f.notches, tr("menu.select.filter.notches"))
                    .changed();
                changed |= ui
                    .checkbox(&mut f.dimensions, tr("menu.select.filter.dimensions"))
                    .changed();
                changed |= ui
                    .checkbox(&mut f.guides, tr("menu.select.filter.guides"))
                    .changed();
                if changed {
                    self.editor.set_select_filter(f);
                }
            });
        });
    }
}

fn is_circle(sheet: &Sheet, id: EntityId) -> bool {
    sheet
        .entity(id)
        .is_ok_and(|e| matches!(e.kind, EntityKind::Circle { .. }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pf_core::{Command, Document, NotchStyle, PointRef, Project, SheetId};
    use pf_geometry::Side;

    fn pt(x: f64, y: f64) -> PointMm {
        PointMm::new(x, y)
    }

    fn editor() -> Editor {
        Editor::new(Document::new(Project::new("Test")))
    }

    fn sheet_id(ed: &Editor) -> SheetId {
        ed.sheet
    }

    fn point(ed: &mut Editor, x: f64, y: f64) -> PointId {
        let sheet = sheet_id(ed);
        ed.exec(Command::AddPoint {
            sheet,
            at: pt(x, y),
        })
        .expect("point")
        .points[0]
    }

    fn segment(ed: &mut Editor, a: PointId, b: PointId) -> EntityId {
        let sheet = sheet_id(ed);
        ed.exec(Command::AddSegment {
            sheet,
            from: PointRef::Existing(a),
            to: PointRef::Existing(b),
        })
        .expect("segment")
        .entities[0]
    }

    fn seg_xy(ed: &mut Editor, a: (f64, f64), b: (f64, f64)) -> EntityId {
        let pa = point(ed, a.0, a.1);
        let pb = point(ed, b.0, b.1);
        segment(ed, pa, pb)
    }

    fn layer(ed: &mut Editor, name: &str) -> LayerId {
        let sheet = sheet_id(ed);
        ed.exec(Command::AddLayer {
            sheet,
            name: name.into(),
        })
        .expect("calque")
        .layers[0]
    }

    fn put_in_layer(ed: &mut Editor, entity: EntityId, layer: LayerId) {
        let sheet = sheet_id(ed);
        let (a, b) = ed.sheet().entity_points(entity).unwrap();
        ed.exec(Command::SetObjectsLayer {
            sheet,
            points: vec![a, b],
            entities: vec![entity],
            layer: Some(layer),
        })
        .expect("rangé");
    }

    fn hide(ed: &mut Editor, layer: LayerId) {
        let sheet = sheet_id(ed);
        ed.exec(Command::SetLayerVisible {
            sheet,
            layer,
            visible: false,
        })
        .expect("masqué");
    }

    fn lock(ed: &mut Editor, layer: LayerId) {
        let sheet = sheet_id(ed);
        ed.exec(Command::SetLayerLocked {
            sheet,
            layer,
            locked: true,
        })
        .expect("verrouillé");
    }

    /// Carré fermé de quatre segments ; retourne ses tracés.
    fn closed_square(ed: &mut Editor, x: f64, y: f64, side: f64) -> Vec<EntityId> {
        let p: Vec<PointId> = [(0.0, 0.0), (side, 0.0), (side, side), (0.0, side)]
            .iter()
            .map(|(dx, dy)| point(ed, x + dx, y + dy))
            .collect();
        (0..4).map(|i| segment(ed, p[i], p[(i + 1) % 4])).collect()
    }

    /// Instantané complet du projet : prouve qu'une opération n'a rien muté.
    fn snapshot(ed: &Editor) -> (Project, bool, bool, bool) {
        (
            ed.project().clone(),
            ed.doc.is_dirty(),
            ed.doc.can_undo(),
            ed.doc.can_redo(),
        )
    }

    // ----- CA1 : tout sélectionner et inverser -----------------------------

    #[test]
    fn ca1_tout_selectionner_exclut_masque_et_verrouille() {
        let mut ed = editor();
        let visible = seg_xy(&mut ed, (0.0, 0.0), (10.0, 0.0));
        let in_hidden = seg_xy(&mut ed, (0.0, 20.0), (10.0, 20.0));
        let in_locked = seg_xy(&mut ed, (0.0, 40.0), (10.0, 40.0));
        let hidden = layer(&mut ed, "Cache");
        let locked = layer(&mut ed, "Bloque");
        put_in_layer(&mut ed, in_hidden, hidden);
        put_in_layer(&mut ed, in_locked, locked);
        hide(&mut ed, hidden);
        lock(&mut ed, locked);
        let before = snapshot(&ed);
        let n = ed.select_all();
        assert_eq!(ed.selection.entities, vec![visible]);
        assert_eq!(
            ed.selection.points.len(),
            2,
            "ses deux extrémités seulement"
        );
        assert_eq!(n, 3);
        assert_eq!(snapshot(&ed), before, "aucune mutation du projet");
    }

    #[test]
    fn ca1_guides_seulement_s_ils_sont_affiches() {
        let mut ed = editor();
        let sheet = ed.sheet;
        ed.exec(Command::AddGuide {
            sheet,
            x_mm: 0.0,
            y_mm: 0.0,
            angle_deg: 0.0,
            axis: false,
            layer: None,
        })
        .unwrap();
        ed.select_all();
        assert_eq!(ed.selection.guides.len(), 1);
        ed.show_guides = false;
        ed.select_all();
        assert!(ed.selection.guides.is_empty());
    }

    #[test]
    fn ca1_un_membre_de_groupe_entraine_son_groupe() {
        let mut ed = editor();
        let a = seg_xy(&mut ed, (0.0, 0.0), (10.0, 0.0));
        let b = seg_xy(&mut ed, (0.0, 20.0), (10.0, 20.0));
        let c = seg_xy(&mut ed, (0.0, 40.0), (10.0, 40.0));
        let sheet = ed.sheet;
        ed.exec(Command::CreateGroup {
            sheet,
            entities: vec![a, b],
        })
        .unwrap();
        // Critère : le seul segment a (rôle) ; son groupe l'accompagne.
        ed.exec(Command::SetPathRole {
            sheet,
            entity: a,
            role: PathRole::Fold,
        })
        .unwrap();
        ed.select_where(Criterion::Role(PathRole::Fold), false);
        let mut got = ed.selection.entities.clone();
        got.sort();
        assert_eq!(got, vec![a, b], "le groupe suit");
        assert!(!ed.selection.entities.contains(&c));
        // Inversion : reste c seul (le groupe sélectionné sort en bloc).
        ed.invert_selection();
        assert_eq!(ed.selection.entities, vec![c]);
        ed.invert_selection();
        let mut back = ed.selection.entities.clone();
        back.sort();
        assert_eq!(back, vec![a, b], "involution sur une sélection fermée");
    }

    #[test]
    fn ca1_inverser_deux_fois_est_l_identite() {
        let mut ed = editor();
        closed_square(&mut ed, 0.0, 0.0, 10.0);
        seg_xy(&mut ed, (50.0, 0.0), (60.0, 5.0));
        ed.select_lasso(
            &[
                pt(-5.0, -5.0),
                pt(15.0, -5.0),
                pt(15.0, 15.0),
                pt(-5.0, 15.0),
            ],
            false,
            false,
        );
        let first = ed.selection.clone();
        assert!(!first.is_empty());
        ed.invert_selection();
        assert_ne!(ed.selection, first);
        ed.invert_selection();
        let sorted = |s: &Selection| {
            let mut s = s.clone();
            s.points.sort();
            s.entities.sort();
            s
        };
        assert_eq!(sorted(&ed.selection), sorted(&first));
        // Depuis le vide : inverser = tout.
        ed.selection.clear();
        ed.invert_selection();
        assert_eq!(sorted(&ed.selection), sorted(&ed.selectable()));
    }

    // ----- CA2 : lasso -------------------------------------------------------

    #[test]
    fn ca2_lasso_retient_ce_qui_est_entierement_dedans() {
        let mut ed = editor();
        let inside = seg_xy(&mut ed, (10.0, 10.0), (20.0, 10.0));
        let straddling = seg_xy(&mut ed, (20.0, 20.0), (80.0, 20.0));
        let outside = seg_xy(&mut ed, (90.0, 90.0), (95.0, 95.0));
        let lone = point(&mut ed, 5.0, 5.0);
        let sq = [pt(0.0, 0.0), pt(40.0, 0.0), pt(40.0, 40.0), pt(0.0, 40.0)];
        let n = ed.select_lasso(&sq, false, false);
        assert!(ed.selection.entities.contains(&inside));
        assert!(!ed.selection.entities.contains(&straddling));
        assert!(!ed.selection.entities.contains(&outside));
        assert!(ed.selection.points.contains(&lone));
        assert_eq!(n, ed.selection.len());
    }

    #[test]
    fn ca2_lasso_concave_refuse_la_sortie_par_arete() {
        let mut ed = editor();
        // Les deux extrémités sont dans les bras du U, le segment traverse la cavité.
        let bridge = seg_xy(&mut ed, (1.5, 8.0), (8.5, 8.0));
        let in_arm = seg_xy(&mut ed, (1.0, 1.0), (1.0, 9.0));
        let u = [
            pt(0.0, 0.0),
            pt(10.0, 0.0),
            pt(10.0, 10.0),
            pt(7.0, 10.0),
            pt(7.0, 3.0),
            pt(3.0, 3.0),
            pt(3.0, 10.0),
            pt(0.0, 10.0),
        ];
        ed.select_lasso(&u, false, false);
        assert!(!ed.selection.entities.contains(&bridge));
        assert!(ed.selection.entities.contains(&in_arm));
        // Mode « touche » : le pont est croisé donc retenu.
        ed.select_lasso(&u, false, true);
        assert!(ed.selection.entities.contains(&bridge));
    }

    #[test]
    fn ca2_lasso_courbes_arcs_cercles_splines() {
        let mut ed = editor();
        let sheet = ed.sheet;
        let circle = ed
            .exec(Command::AddCircle {
                sheet,
                center: PointRef::New(pt(50.0, 50.0)),
                radius_mm: 10.0,
            })
            .unwrap()
            .entities[0];
        let big_circle = ed
            .exec(Command::AddCircle {
                sheet,
                center: PointRef::New(pt(50.0, 50.0)),
                radius_mm: 60.0,
            })
            .unwrap()
            .entities[0];
        let spline = ed
            .exec(Command::AddSpline {
                sheet,
                points: vec![
                    PointRef::New(pt(40.0, 40.0)),
                    PointRef::New(pt(50.0, 60.0)),
                    PointRef::New(pt(60.0, 40.0)),
                ],
            })
            .unwrap()
            .entities[0];
        let curve = ed
            .exec(Command::AddCurve {
                sheet,
                start: PointRef::New(pt(45.0, 45.0)),
                c1: pt(45.0, 80.0),
                c2: pt(55.0, 80.0),
                end: PointRef::New(pt(55.0, 45.0)),
            })
            .unwrap()
            .entities[0];
        // Lasso autour de [20, 80]^2 : cercle de rayon 10 dedans, grand cercle dehors,
        // la Bézier monte à ~71 : dedans ; la spline reste dedans.
        let poly = [
            pt(20.0, 20.0),
            pt(80.0, 20.0),
            pt(80.0, 80.0),
            pt(20.0, 80.0),
        ];
        ed.select_lasso(&poly, false, false);
        for id in [circle, spline, curve] {
            assert!(ed.selection.entities.contains(&id), "{id:?}");
        }
        assert!(!ed.selection.entities.contains(&big_circle));
        // Lasso plus bas : la Bézier dépasse (y max ~ 71) donc exclue ; la spline
        // (y max ~ 57) reste.
        let low = [
            pt(20.0, 20.0),
            pt(80.0, 20.0),
            pt(80.0, 65.0),
            pt(20.0, 65.0),
        ];
        ed.select_lasso(&low, false, false);
        assert!(!ed.selection.entities.contains(&curve));
        assert!(ed.selection.entities.contains(&spline));
    }

    #[test]
    fn ca2_lasso_degenere_ne_selectionne_rien_sans_panique() {
        let mut ed = editor();
        seg_xy(&mut ed, (0.0, 0.0), (10.0, 0.0));
        ed.select_all();
        assert!(!ed.selection.is_empty());
        let nan = f64::NAN;
        for poly in [
            vec![],
            vec![pt(0.0, 0.0)],
            vec![pt(0.0, 0.0), pt(100.0, 0.0)],
            vec![pt(0.0, 0.0), pt(50.0, 0.0), pt(100.0, 0.0)],
            vec![pt(1.0, 1.0), pt(1.0, 1.0), pt(1.0, 1.0)],
            vec![pt(0.0, 0.0), pt(nan, 5.0), pt(100.0, 100.0)],
            vec![pt(f64::INFINITY, 0.0), pt(0.0, 1.0), pt(1.0, 0.0)],
        ] {
            ed.select_all();
            let n = ed.select_lasso(&poly, false, false);
            assert_eq!(n, 0, "{poly:?}");
            assert!(ed.selection.is_empty(), "{poly:?}");
        }
        // Additif : sélection conservée.
        ed.select_all();
        let before = ed.selection.clone();
        ed.select_lasso(&[pt(0.0, 0.0), pt(1.0, 1.0)], true, false);
        assert_eq!(ed.selection, before);
    }

    #[test]
    fn ca2_lasso_additif_et_auto_intersection() {
        let mut ed = editor();
        let left = seg_xy(&mut ed, (1.0, 4.0), (2.0, 6.0));
        let right = seg_xy(&mut ed, (8.0, 4.0), (9.0, 6.0));
        let top = seg_xy(&mut ed, (4.0, 8.0), (6.0, 8.0));
        // Papillon : lobes gauche et droit pleins, haut et bas vides (pair-impair).
        let bow = [pt(0.0, 0.0), pt(10.0, 10.0), pt(10.0, 0.0), pt(0.0, 10.0)];
        ed.select_lasso(&bow, false, false);
        assert!(ed.selection.entities.contains(&left));
        assert!(ed.selection.entities.contains(&right));
        assert!(!ed.selection.entities.contains(&top));
        // Additif : ajoute sans retirer.
        ed.selection.clear();
        ed.select_lasso(
            &[pt(3.0, 7.0), pt(7.0, 7.0), pt(7.0, 9.0), pt(3.0, 9.0)],
            false,
            false,
        );
        assert_eq!(ed.selection.entities, vec![top]);
        ed.select_lasso(&bow, true, false);
        for id in [left, right, top] {
            assert!(ed.selection.entities.contains(&id));
        }
    }

    #[test]
    fn ca2_lasso_cran_et_cotation() {
        let mut ed = editor();
        let sheet = ed.sheet;
        let a = point(&mut ed, 0.0, 0.0);
        let b = point(&mut ed, 50.0, 0.0);
        let e = segment(&mut ed, a, b);
        ed.exec(Command::AddNotch {
            sheet,
            entity: e,
            distance_mm: 25.0,
            side: Side::Left,
            length_mm: 5.0,
            style: NotchStyle::Single,
        })
        .unwrap();
        ed.exec(Command::AddDimension {
            sheet,
            from: a,
            to: b,
            offset_mm: 10.0,
        })
        .unwrap();
        // Lasso qui couvre le segment mais pas le décalage de la cotation (y = 10 ou -10).
        ed.select_lasso(
            &[pt(-5.0, -3.0), pt(55.0, -3.0), pt(55.0, 8.0), pt(-5.0, 8.0)],
            false,
            false,
        );
        assert_eq!(ed.selection.entities, vec![e]);
        assert_eq!(
            ed.selection.notches.len(),
            1,
            "cran de 5 mm côté gauche (y = 5)"
        );
        let wide = [
            pt(-5.0, -20.0),
            pt(55.0, -20.0),
            pt(55.0, 20.0),
            pt(-5.0, 20.0),
        ];
        ed.select_lasso(&wide, false, false);
        assert_eq!(ed.selection.dimensions.len(), 1);
    }

    // ----- CA3 : par critère ---------------------------------------------------

    #[test]
    fn ca3_par_role() {
        let mut ed = editor();
        let a = seg_xy(&mut ed, (0.0, 0.0), (10.0, 0.0));
        let b = seg_xy(&mut ed, (0.0, 20.0), (10.0, 20.0));
        seg_xy(&mut ed, (0.0, 40.0), (10.0, 40.0));
        let sheet = ed.sheet;
        for (e, role) in [(a, PathRole::CuttingContour), (b, PathRole::CuttingContour)] {
            ed.exec(Command::SetPathRole {
                sheet,
                entity: e,
                role,
            })
            .unwrap();
        }
        let before = snapshot(&ed);
        let n = ed.select_where(Criterion::Role(PathRole::CuttingContour), false);
        let mut got = ed.selection.entities.clone();
        got.sort();
        assert_eq!(got, vec![a, b]);
        assert!(
            ed.selection.points.is_empty(),
            "un rôle ne vise que des tracés"
        );
        assert_eq!(n, 2);
        assert_eq!(snapshot(&ed), before);
        // Rôle absent : rien, message.
        assert_eq!(ed.select_where(Criterion::Role(PathRole::Fold), false), 0);
        assert!(ed.selection.is_empty());
    }

    #[test]
    fn ca3_par_calque_remplace_ou_ajoute() {
        let mut ed = editor();
        let on_a = seg_xy(&mut ed, (0.0, 0.0), (10.0, 0.0));
        let on_default = seg_xy(&mut ed, (0.0, 20.0), (10.0, 20.0));
        let a = layer(&mut ed, "A");
        put_in_layer(&mut ed, on_a, a);
        ed.select_where(Criterion::Layer(Some(a)), false);
        assert_eq!(ed.selection.entities, vec![on_a]);
        assert_eq!(ed.selection.points.len(), 2);
        // Calque par défaut, remplace.
        ed.select_where(Criterion::Layer(None), false);
        assert_eq!(ed.selection.entities, vec![on_default]);
        // Maj : ajoute.
        ed.select_where(Criterion::Layer(Some(a)), true);
        let mut got = ed.selection.entities.clone();
        got.sort();
        assert_eq!(got, vec![on_a, on_default]);
        // Calque masqué : exclu même ciblé.
        hide(&mut ed, a);
        ed.select_where(Criterion::Layer(Some(a)), false);
        assert!(ed.selection.is_empty());
    }

    #[test]
    fn ca3_par_type_neuf_types() {
        let mut ed = editor();
        let sheet = ed.sheet;
        let a = point(&mut ed, 0.0, 0.0);
        let b = point(&mut ed, 50.0, 0.0);
        let seg = segment(&mut ed, a, b);
        let circle = ed
            .exec(Command::AddCircle {
                sheet,
                center: PointRef::New(pt(0.0, 100.0)),
                radius_mm: 5.0,
            })
            .unwrap()
            .entities[0];
        let arc = ed
            .exec(Command::AddArc {
                sheet,
                center: PointRef::New(pt(200.0, 100.0)),
                radius_mm: 10.0,
                start_deg: 0.0,
                end_deg: 90.0,
                ccw: true,
            })
            .unwrap()
            .entities[0];
        let spline = ed
            .exec(Command::AddSpline {
                sheet,
                points: vec![
                    PointRef::New(pt(0.0, 300.0)),
                    PointRef::New(pt(30.0, 310.0)),
                ],
            })
            .unwrap()
            .entities[0];
        let curve = ed
            .exec(Command::AddCurve {
                sheet,
                start: PointRef::New(pt(0.0, 400.0)),
                c1: pt(10.0, 420.0),
                c2: pt(30.0, 420.0),
                end: PointRef::New(pt(40.0, 400.0)),
            })
            .unwrap()
            .entities[0];
        ed.exec(Command::AddNotch {
            sheet,
            entity: seg,
            distance_mm: 20.0,
            side: Side::Left,
            length_mm: 5.0,
            style: NotchStyle::Single,
        })
        .unwrap();
        ed.exec(Command::AddDimension {
            sheet,
            from: a,
            to: b,
            offset_mm: 10.0,
        })
        .unwrap();
        ed.exec(Command::AddGuide {
            sheet,
            x_mm: 0.0,
            y_mm: 0.0,
            angle_deg: 45.0,
            axis: false,
            layer: None,
        })
        .unwrap();
        let only = |ed: &mut Editor, t: ObjectType| {
            ed.select_where(Criterion::Kind(t), false);
            ed.selection.clone()
        };
        assert_eq!(only(&mut ed, ObjectType::Segment).entities, vec![seg]);
        assert_eq!(only(&mut ed, ObjectType::Circle).entities, vec![circle]);
        assert_eq!(only(&mut ed, ObjectType::Arc).entities, vec![arc]);
        assert_eq!(only(&mut ed, ObjectType::Spline).entities, vec![spline]);
        assert_eq!(only(&mut ed, ObjectType::Curve).entities, vec![curve]);
        let s = only(&mut ed, ObjectType::Point);
        assert_eq!(s.points.len(), ed.sheet().points.len());
        assert!(s.entities.is_empty());
        assert_eq!(only(&mut ed, ObjectType::Notch).notches.len(), 1);
        assert_eq!(only(&mut ed, ObjectType::Dimension).dimensions.len(), 1);
        let g = only(&mut ed, ObjectType::Guide);
        assert_eq!(g.guides.len(), 1);
        assert_eq!(g.len(), 1);
    }

    // ----- CA4 : par chaîne ----------------------------------------------------

    #[test]
    fn ca4_chaine_fermee_ouverte_et_isolee() {
        let mut ed = editor();
        let square = closed_square(&mut ed, 0.0, 0.0, 10.0);
        // Chaîne ouverte de trois segments.
        let p: Vec<PointId> = (0..4)
            .map(|i| point(&mut ed, 100.0 + 20.0 * f64::from(i), 0.0))
            .collect();
        let open: Vec<EntityId> = (0..3).map(|i| segment(&mut ed, p[i], p[i + 1])).collect();
        let isolated = seg_xy(&mut ed, (0.0, 200.0), (10.0, 210.0));
        let before = snapshot(&ed);
        // Fermée : les quatre côtés depuis l'un d'eux.
        assert_eq!(ed.select_chain_of(square[2], false), Ok(4));
        let mut got = ed.selection.entities.clone();
        got.sort();
        let mut want = square.clone();
        want.sort();
        assert_eq!(got, want);
        // Ouverte, depuis le tracé du milieu.
        assert_eq!(ed.select_chain_of(open[1], false), Ok(3));
        let mut got = ed.selection.entities.clone();
        got.sort();
        assert_eq!(got, open);
        // Isolée : elle-même.
        assert_eq!(ed.select_chain_of(isolated, false), Ok(1));
        assert_eq!(ed.selection.entities, vec![isolated]);
        // Depuis la sélection courante, additif : la chaîne s'ajoute.
        ed.selection.entities = vec![open[0]];
        assert_eq!(ed.select_chain(false), Ok(3));
        assert_eq!(snapshot(&ed), before, "aucune mutation du projet");
    }

    #[test]
    fn ca4_bifurcation_inexistant_et_rien_de_selectionne() {
        let mut ed = editor();
        // T : trois segments au même point (0,0).
        let o = point(&mut ed, 0.0, 0.0);
        let ends: Vec<PointId> = [(10.0, 0.0), (0.0, 10.0), (-10.0, 0.0)]
            .iter()
            .map(|(x, y)| point(&mut ed, *x, *y))
            .collect();
        let legs: Vec<EntityId> = ends.iter().map(|e| segment(&mut ed, o, *e)).collect();
        ed.selection.entities = vec![legs[0]];
        let before = ed.selection.clone();
        assert_eq!(
            ed.select_chain_of(legs[0], false),
            Err(ChainError::Ambiguous)
        );
        assert!(ed.status.error, "{}", ed.status.text);
        assert_eq!(ed.selection, before, "sélection inchangée");
        assert_eq!(ed.select_chain(true), Err(ChainError::Ambiguous));
        assert_eq!(ed.selection, before);
        // Tracé inexistant.
        assert_eq!(
            ed.select_chain_of(EntityId(987_654), false),
            Err(ChainError::Unknown)
        );
        assert_eq!(ed.selection, before);
        // Rien de sélectionné.
        ed.selection.clear();
        assert_eq!(ed.select_chain(false), Err(ChainError::NoPath));
        assert!(ed.selection.is_empty());
        // Souris dans le vide.
        assert_eq!(
            ed.select_chain_at(pt(500.0, 500.0), 2.0, false),
            Err(ChainError::NoPath)
        );
    }

    #[test]
    fn ca4_chaine_verrouillee_ou_filtree_refusee() {
        let mut ed = editor();
        let e = seg_xy(&mut ed, (0.0, 0.0), (10.0, 0.0));
        let l = layer(&mut ed, "L");
        put_in_layer(&mut ed, e, l);
        lock(&mut ed, l);
        assert_eq!(ed.select_chain_of(e, false), Err(ChainError::Nothing));
        assert!(ed.selection.is_empty());
        // Chaîne par double-clic sur un tracé : la visée suit le filtre.
        let mut ed = editor();
        let sq = closed_square(&mut ed, 0.0, 0.0, 10.0);
        assert_eq!(ed.select_chain_at(pt(5.0, 0.0), 1.0, false), Ok(4));
        ed.set_select_filter(SelectionFilter {
            paths: false,
            ..SelectionFilter::default()
        });
        assert_eq!(
            ed.select_chain_at(pt(5.0, 0.0), 1.0, false),
            Err(ChainError::NoPath)
        );
        let _ = sq;
    }

    // ----- CA5 : filtres -----------------------------------------------------

    #[test]
    fn ca5_filtre_restreint_clic_fenetre_tout_inverser_lasso() {
        let mut ed = editor();
        let e = seg_xy(&mut ed, (0.0, 0.0), (50.0, 0.0));
        let only_paths = SelectionFilter {
            points: false,
            notches: false,
            dimensions: false,
            guides: false,
            ..SelectionFilter::default()
        };
        ed.set_select_filter(only_paths);
        // Clic sur une extrémité : le point est filtré, le tracé est atteint.
        ed.click(pt(0.0, 0.0), 2.0, false);
        assert_eq!(ed.selection.entities, vec![e]);
        assert!(ed.selection.points.is_empty());
        // Fenêtre, lasso, Tout, Inverser.
        ed.selection.clear();
        ed.select_window(pt(-5.0, -5.0), pt(60.0, 5.0), false);
        assert_eq!(ed.selection.len(), 1);
        ed.selection.clear();
        ed.select_lasso(
            &[pt(-5.0, -5.0), pt(60.0, -5.0), pt(60.0, 5.0), pt(-5.0, 5.0)],
            false,
            false,
        );
        assert_eq!(ed.selection.len(), 1);
        assert_eq!(ed.select_all(), 1);
        ed.invert_selection();
        assert!(ed.selection.is_empty());
        ed.invert_selection();
        assert_eq!(ed.selection.entities, vec![e]);
        // Critère de type : un type filtré n'est pas sélectionné.
        assert_eq!(
            ed.select_where(Criterion::Kind(ObjectType::Point), false),
            0
        );
        // Seuls les points.
        ed.set_select_filter(SelectionFilter {
            points: true,
            paths: false,
            notches: false,
            dimensions: false,
            guides: false,
        });
        ed.click(pt(25.0, 0.0), 2.0, false);
        assert!(ed.selection.is_empty(), "le tracé n'est plus atteignable");
        ed.click(pt(50.0, 0.0), 2.0, false);
        assert_eq!(ed.selection.points.len(), 1);
    }

    #[test]
    fn ca5_ce_qui_sort_du_filtre_est_deselectionne() {
        let mut ed = editor();
        let sheet = ed.sheet;
        let a = point(&mut ed, 0.0, 0.0);
        let b = point(&mut ed, 50.0, 0.0);
        let e = segment(&mut ed, a, b);
        ed.exec(Command::AddNotch {
            sheet,
            entity: e,
            distance_mm: 20.0,
            side: Side::Left,
            length_mm: 5.0,
            style: NotchStyle::Single,
        })
        .unwrap();
        ed.exec(Command::AddDimension {
            sheet,
            from: a,
            to: b,
            offset_mm: 10.0,
        })
        .unwrap();
        ed.select_all();
        assert!(!ed.selection.notches.is_empty() && !ed.selection.dimensions.is_empty());
        ed.set_select_filter(SelectionFilter {
            notches: false,
            ..SelectionFilter::default()
        });
        assert!(ed.selection.notches.is_empty());
        assert_eq!(ed.selection.entities, vec![e], "le reste est conservé");
        assert!(!ed.selection.dimensions.is_empty());
        ed.set_select_filter(SelectionFilter {
            points: false,
            ..SelectionFilter::default()
        });
        assert!(ed.selection.points.is_empty());
        // Tout décoché : rien n'est sélectionnable, sans erreur.
        ed.set_select_filter(SelectionFilter {
            points: false,
            paths: false,
            notches: false,
            dimensions: false,
            guides: false,
        });
        assert!(ed.selection.is_empty());
        assert_eq!(ed.select_all(), 0);
        ed.click(pt(25.0, 0.0), 2.0, false);
        assert!(ed.selection.is_empty());
        assert!(ed.select_filter.is_none());
        // Retour à tout : de nouveau sélectionnable.
        ed.set_select_filter(SelectionFilter::default());
        assert!(ed.select_all() > 0);
    }

    // ----- CA6 : verrouillé / masqué, projet non modifié -----------------------

    #[test]
    fn ca6_aucune_operation_n_atteint_le_masque_ni_le_verrouille() {
        let mut ed = editor();
        let ok = seg_xy(&mut ed, (0.0, 0.0), (10.0, 0.0));
        let h = seg_xy(&mut ed, (0.0, 20.0), (10.0, 20.0));
        let k = seg_xy(&mut ed, (0.0, 40.0), (10.0, 40.0));
        let (lh, lk) = (layer(&mut ed, "H"), layer(&mut ed, "K"));
        put_in_layer(&mut ed, h, lh);
        put_in_layer(&mut ed, k, lk);
        hide(&mut ed, lh);
        lock(&mut ed, lk);
        let big = [
            pt(-50.0, -50.0),
            pt(50.0, -50.0),
            pt(50.0, 80.0),
            pt(-50.0, 80.0),
        ];
        let before = snapshot(&ed);
        for step in 0..6 {
            let _ = match step {
                0 => ed.select_all(),
                1 => ed.invert_selection(),
                2 => ed.select_lasso(&big, false, false),
                3 => ed.select_lasso(&big, false, true),
                4 => ed.select_where(Criterion::Kind(ObjectType::Segment), false),
                _ => ed.select_where(Criterion::Layer(Some(lk)), false),
            };
            assert!(!ed.selection.entities.contains(&h), "masqué, étape {step}");
            assert!(
                !ed.selection.entities.contains(&k),
                "verrouillé, étape {step}"
            );
        }
        ed.select_all();
        assert_eq!(ed.selection.entities, vec![ok]);
        assert_eq!(snapshot(&ed), before);
    }

    #[test]
    fn ca6_projet_et_historique_inchanges_par_toutes_les_operations() {
        let mut ed = editor();
        let sq = closed_square(&mut ed, 0.0, 0.0, 10.0);
        let before = snapshot(&ed);
        let dirty_before = ed.doc.is_dirty();
        ed.select_all();
        ed.invert_selection();
        ed.select_where(Criterion::Role(PathRole::Construction), true);
        ed.select_where(Criterion::Kind(ObjectType::Segment), false);
        ed.select_lasso(
            &[pt(-1.0, -1.0), pt(20.0, -1.0), pt(20.0, 20.0)],
            true,
            true,
        );
        ed.select_chain_of(sq[0], true).unwrap();
        ed.set_select_filter(SelectionFilter {
            points: false,
            ..SelectionFilter::default()
        });
        ed.set_lasso_mode(true);
        ed.click(pt(5.0, 0.0), 2.0, false);
        ed.select_window(pt(-1.0, -1.0), pt(11.0, 11.0), false);
        assert_eq!(snapshot(&ed), before);
        assert_eq!(ed.doc.is_dirty(), dirty_before);
    }

    // ----- non-régression de la fenêtre ------------------------------------

    #[test]
    fn non_regression_fenetre_rectangulaire() {
        let mut ed = editor();
        let inside = seg_xy(&mut ed, (10.0, 10.0), (20.0, 10.0));
        let crossing = seg_xy(&mut ed, (10.0, 30.0), (200.0, 30.0));
        let lone = point(&mut ed, 5.0, 5.0);
        // Fenêtre : tout ce qui est entièrement dedans, points compris.
        ed.select_window(pt(0.0, 0.0), pt(40.0, 40.0), false);
        assert!(ed.selection.entities.contains(&inside));
        assert!(!ed.selection.entities.contains(&crossing));
        assert!(ed.selection.points.contains(&lone));
        assert_eq!(
            ed.selection.points.len(),
            4,
            "extrémités du tracé dedans, départ du long tracé, point seul"
        );
        // Coin opposé : même fenêtre.
        let mut other = editor();
        let i2 = seg_xy(&mut other, (10.0, 10.0), (20.0, 10.0));
        other.select_window(pt(40.0, 40.0), pt(0.0, 0.0), false);
        assert_eq!(other.selection.entities, vec![i2]);
        // Additif : conserve ; non additif : remplace.
        let far_end = ed.sheet().entity_points(crossing).unwrap().1;
        ed.select_window(pt(150.0, 25.0), pt(250.0, 35.0), true);
        assert!(ed.selection.entities.contains(&inside));
        assert!(ed.selection.points.contains(&far_end));
        assert_eq!(ed.selection.points.len(), 5);
        ed.select_window(pt(1000.0, 1000.0), pt(1100.0, 1100.0), false);
        assert!(ed.selection.is_empty());
        // Un groupe suit son membre.
        let sheet = ed.sheet;
        ed.exec(Command::CreateGroup {
            sheet,
            entities: vec![inside, crossing],
        })
        .unwrap();
        ed.select_window(pt(0.0, 0.0), pt(40.0, 15.0), false);
        assert!(ed.selection.entities.contains(&crossing));
    }

    // ----- campagne aléatoire (graine fixe) ------------------------------------

    /// Générateur xorshift64* : reproductible, sans dépendance.
    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 >> 12;
            self.0 ^= self.0 << 25;
            self.0 ^= self.0 >> 27;
            self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
        }
        fn below(&mut self, n: usize) -> usize {
            (self.next() % n.max(1) as u64) as usize
        }
        fn range(&mut self, lo: f64, hi: f64) -> f64 {
            lo + (self.next() >> 11) as f64 / (1u64 << 53) as f64 * (hi - lo)
        }
        fn coin(&mut self) -> bool {
            self.below(2) == 0
        }
    }

    fn random_editor(rng: &mut Rng) -> Editor {
        let mut ed = editor();
        let sheet = ed.sheet;
        let mut pts: Vec<PointId> = Vec::new();
        for _ in 0..(4 + rng.below(8)) {
            let at = pt(rng.range(-100.0, 100.0), rng.range(-100.0, 100.0));
            if let Some(o) = ed.exec(Command::AddPoint { sheet, at }) {
                pts.extend(o.points);
            }
        }
        let pick = |rng: &mut Rng, pts: &[PointId]| pts[rng.below(pts.len())];
        for _ in 0..rng.below(9) {
            let (a, b) = (pick(rng, &pts), pick(rng, &pts));
            let _ = ed.exec(Command::AddSegment {
                sheet,
                from: PointRef::Existing(a),
                to: PointRef::Existing(b),
            });
        }
        for _ in 0..rng.below(3) {
            let _ = ed.exec(Command::AddCircle {
                sheet,
                center: PointRef::Existing(pick(rng, &pts)),
                radius_mm: rng.range(1.0, 60.0),
            });
        }
        for _ in 0..rng.below(2) {
            let start = pick(rng, &pts);
            let end = pick(rng, &pts);
            let _ = ed.exec(Command::AddCurve {
                sheet,
                start: PointRef::Existing(start),
                c1: pt(rng.range(-120.0, 120.0), rng.range(-120.0, 120.0)),
                c2: pt(rng.range(-120.0, 120.0), rng.range(-120.0, 120.0)),
                end: PointRef::Existing(end),
            });
        }
        if rng.coin() {
            let _ = ed.exec(Command::AddSpline {
                sheet,
                points: (0..3)
                    .map(|_| PointRef::Existing(pick(rng, &pts)))
                    .collect(),
            });
        }
        if rng.coin() {
            let _ = ed.exec(Command::AddArc {
                sheet,
                center: PointRef::New(pt(rng.range(-50.0, 50.0), rng.range(-50.0, 50.0))),
                radius_mm: rng.range(1.0, 40.0),
                start_deg: rng.range(0.0, 360.0),
                end_deg: rng.range(0.0, 360.0),
                ccw: rng.coin(),
            });
        }
        let entities: Vec<EntityId> = ed.sheet().entities.iter().map(|e| e.id).collect();
        for _ in 0..rng.below(3) {
            if let Some(&e) = entities.get(rng.below(entities.len().max(1))) {
                let _ = ed.exec(Command::AddNotch {
                    sheet,
                    entity: e,
                    distance_mm: rng.range(0.0, 5.0),
                    side: Side::Left,
                    length_mm: 3.0,
                    style: NotchStyle::Single,
                });
            }
        }
        for _ in 0..rng.below(3) {
            let (from, to) = (pick(rng, &pts), pick(rng, &pts));
            let _ = ed.exec(Command::AddDimension {
                sheet,
                from,
                to,
                offset_mm: rng.range(-20.0, 20.0),
            });
        }
        for _ in 0..rng.below(3) {
            let _ = ed.exec(Command::AddGuide {
                sheet,
                x_mm: rng.range(-50.0, 50.0),
                y_mm: rng.range(-50.0, 50.0),
                angle_deg: rng.range(0.0, 180.0),
                axis: false,
                layer: None,
            });
        }
        for &e in &entities {
            let role = PathRole::ALL[rng.below(PathRole::ALL.len())];
            let _ = ed.exec(Command::SetPathRole {
                sheet,
                entity: e,
                role,
            });
        }
        for _ in 0..rng.below(3) {
            let (a, b) = (
                rng.below(entities.len().max(1)),
                rng.below(entities.len().max(1)),
            );
            if let (Some(&x), Some(&y)) = (entities.get(a), entities.get(b)) {
                let _ = ed.exec(Command::CreateGroup {
                    sheet,
                    entities: vec![x, y],
                });
            }
        }
        for i in 0..rng.below(3) {
            let l = layer(&mut ed, &format!("L{i}"));
            for &e in &entities {
                if rng.below(3) == 0 {
                    let (a, b) = ed.sheet().entity_points(e).unwrap();
                    let _ = ed.exec(Command::SetObjectsLayer {
                        sheet,
                        points: vec![a, b],
                        entities: vec![e],
                        layer: Some(l),
                    });
                }
            }
            match rng.below(4) {
                0 => hide(&mut ed, l),
                1 => lock(&mut ed, l),
                _ => {}
            }
        }
        ed
    }

    fn random_polygon(rng: &mut Rng) -> Vec<PointMm> {
        let n = rng.below(10);
        let mut poly: Vec<PointMm> = (0..n)
            .map(|_| pt(rng.range(-150.0, 150.0), rng.range(-150.0, 150.0)))
            .collect();
        match rng.below(8) {
            0 if !poly.is_empty() => poly.push(poly[0]),
            1 if !poly.is_empty() => poly.push(*poly.last().unwrap()),
            2 if !poly.is_empty() => {
                let i = rng.below(poly.len());
                poly[i] = pt(f64::NAN, 1.0);
            }
            3 => poly = vec![pt(0.0, 0.0), pt(10.0, 10.0), pt(20.0, 20.0)],
            _ => {}
        }
        poly
    }

    fn sorted(s: &Selection) -> Selection {
        let mut s = s.clone();
        s.points.sort();
        s.entities.sort();
        s.notches.sort();
        s.dimensions.sort();
        s.guides.sort();
        s
    }

    fn check_invariants(ed: &mut Editor, before: &(Project, bool, bool, bool), what: &str) {
        let sheet = ed.sheet().clone();
        let s = ed.selection.clone();
        let ok = |layer| sheet.layer_visible(layer) && !sheet.layer_locked(layer);
        for p in &s.points {
            let pt = sheet
                .point(*p)
                .unwrap_or_else(|_| panic!("{what} : point mort {p:?}"));
            assert!(ok(pt.layer), "{what} : point masqué ou verrouillé");
        }
        for e in &s.entities {
            let en = sheet
                .entity(*e)
                .unwrap_or_else(|_| panic!("{what} : tracé mort {e:?}"));
            assert!(ok(en.layer), "{what} : tracé masqué ou verrouillé");
            if let Some(g) = sheet.group_of(*e) {
                for m in &g.members {
                    let member = sheet.entity(*m).unwrap();
                    if ok(member.layer) && ed.select_filter.paths {
                        assert!(s.entities.contains(m), "{what} : groupe incomplet");
                    }
                }
            }
        }
        for n in &s.notches {
            let notch = sheet
                .notch(*n)
                .unwrap_or_else(|_| panic!("{what} : cran mort"));
            assert!(
                ok(sheet.entity(notch.entity).unwrap().layer),
                "{what} : cran"
            );
        }
        for d in &s.dimensions {
            let dim = sheet
                .dimension(*d)
                .unwrap_or_else(|_| panic!("{what} : cote morte"));
            assert!(
                ok(sheet.point(dim.from).unwrap().layer),
                "{what} : cotation"
            );
            assert!(ok(sheet.point(dim.to).unwrap().layer), "{what} : cotation");
        }
        for g in &s.guides {
            let guide = sheet
                .guide(*g)
                .unwrap_or_else(|_| panic!("{what} : guide mort"));
            assert!(ok(guide.layer), "{what} : guide");
        }
        // Tout contient tout résultat.
        let all = ed.selectable();
        for (mine, theirs) in [
            (
                s.points.len(),
                s.points.iter().filter(|p| all.points.contains(p)).count(),
            ),
            (
                s.entities.len(),
                s.entities
                    .iter()
                    .filter(|p| all.entities.contains(p))
                    .count(),
            ),
            (
                s.notches.len(),
                s.notches.iter().filter(|p| all.notches.contains(p)).count(),
            ),
            (
                s.dimensions.len(),
                s.dimensions
                    .iter()
                    .filter(|p| all.dimensions.contains(p))
                    .count(),
            ),
            (
                s.guides.len(),
                s.guides.iter().filter(|p| all.guides.contains(p)).count(),
            ),
        ] {
            assert_eq!(mine, theirs, "{what} : un résultat sort de « Tout »");
        }
        // Inverser deux fois = identité.
        ed.invert_selection();
        ed.invert_selection();
        assert_eq!(
            sorted(&ed.selection),
            sorted(&s),
            "{what} : inversion non involutive"
        );
        assert_eq!(&snapshot(ed), before, "{what} : le projet a été touché");
    }

    #[test]
    fn campagne_aleatoire_selection_graine_fixe() {
        let mut rng = Rng(0x5EED_1601);
        let mut ops_run = 0usize;
        let (mut non_empty, mut with_paths, mut chains_ok) = (0usize, 0usize, 0usize);
        for draw in 0..250 {
            let mut ed = random_editor(&mut rng);
            let before = snapshot(&ed);
            let layers: Vec<LayerId> = ed.sheet().layers.iter().map(|l| l.id).collect();
            let ids: Vec<EntityId> = ed.sheet().entities.iter().map(|e| e.id).collect();
            for step in 0..8 {
                let op = rng.below(9);
                let what = format!("tirage {draw}, étape {step}, op {op}");
                match op {
                    0 => {
                        ed.select_all();
                        assert_eq!(sorted(&ed.selection), sorted(&ed.selectable()), "{what}");
                    }
                    1 => {
                        ed.invert_selection();
                    }
                    2 => {
                        let poly = random_polygon(&mut rng);
                        let (add, touch) = (rng.coin(), rng.coin());
                        ed.select_lasso(&poly, add, touch);
                    }
                    3 => {
                        let role = PathRole::ALL[rng.below(PathRole::ALL.len())];
                        ed.select_where(Criterion::Role(role), rng.coin());
                    }
                    4 => {
                        let l = layers.get(rng.below(layers.len() + 1)).copied();
                        ed.select_where(Criterion::Layer(l), rng.coin());
                    }
                    5 => {
                        let k = ObjectType::ALL[rng.below(ObjectType::ALL.len())];
                        ed.select_where(Criterion::Kind(k), rng.coin());
                    }
                    6 => {
                        let id = ids
                            .get(rng.below(ids.len() + 1))
                            .copied()
                            .unwrap_or(EntityId(u64::MAX));
                        chains_ok += usize::from(ed.select_chain_of(id, rng.coin()).is_ok());
                    }
                    7 => {
                        let _ = ed.select_chain(rng.coin());
                    }
                    _ => {
                        let f = SelectionFilter {
                            points: rng.below(4) != 0,
                            paths: rng.below(4) != 0,
                            notches: rng.below(4) != 0,
                            dimensions: rng.below(4) != 0,
                            guides: rng.below(4) != 0,
                        };
                        ed.set_select_filter(f);
                    }
                }
                ops_run += 1;
                check_invariants(&mut ed, &before, &what);
                non_empty += usize::from(!ed.selection.is_empty());
                with_paths += usize::from(!ed.selection.entities.is_empty());
            }
        }
        assert_eq!(ops_run, 250 * 8);
        // La campagne doit réellement sélectionner des choses (pas un vide cohérent).
        assert!(non_empty > 700, "{non_empty}");
        assert!(with_paths > 400, "{with_paths}");
        assert!(chains_ok > 20, "{chains_ok}");
    }
}
