//! US-03.6 — historique de construction : arbre de dépendances d'un objet et impact d'une
//! suppression (lecture seule, **jamais stocké**, aucun changement de format).
//!
//! Le graphe est **recalculé à chaque appel** depuis le modèle : `Project::dependency_edges`
//! unifie, en arêtes orientées « parent → enfant » avec une [`DependencyNature`], tous les liens
//! réellement présents :
//!
//! - points construits (tracés, points et ligne d'aide dont ils dépendent) ;
//! - points d'un tracé (un tracé dépend de ses points) ;
//! - constructions maintenues (US-03.2) : parents → points suivis ;
//! - formules (US-03.1b) et variables (US-03.1a) : variable citée → point / variable ;
//! - gradation (US-09.2a) : taille dont l'écart déplace un point libre → ce point ;
//! - longueurs imposées (US-03.3) et égalités (US-03.4) : référence → courbe, courbe → point
//!   d'extrémité glissante ;
//! - crans (tracé, ancrage), cotations et cotes complètes (points, tracés), surpiqûres liées ;
//! - pièces : contour, droit-fil, pli, éléments internes, marquages, pinces, boutons, pièce mère
//!   d'une dérivée, couples de coutures.
//!
//! Limites assumées : les surcharges de marge de couture et l'appartenance à un groupe ou à un
//! calque ne sont pas des dépendances ; la provenance mémorisée d'un segment non maintenu
//! (`construction`) n'est pas un lien.
//!
//! `Project::dependency_tree` fait un parcours en largeur (chaque objet une seule fois, à sa
//! profondeur la plus courte, sans boucle même si le graphe en contient : un point d'extrémité
//! d'une courbe qu'il ajuste est à la fois parent et enfant de cette courbe). Le tri est
//! déterministe : profondeur, puis ordre des objets.
//!
//! `Project::deletion_impact` **rejoue la vraie commande** sur une copie du projet
//! (`Project::apply`) et compare avant / après : aucune règle n'est réécrite ici, l'annonce ne
//! peut donc pas diverger du comportement réel. Le projet lui-même n'est jamais modifié.

use crate::command::Command;
use crate::expr;
use crate::full_dimension::FullDimensionKind;
use crate::length_constraint::LengthAdjust;
use crate::model::*;
use crate::piece_grain::GrainDirection;
use crate::piece_marking::MarkingAnchor;
use crate::topstitch::TopstitchSource;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

/// Objet du graphe de dépendances.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DependencyObject {
    Point(PointId),
    Entity(EntityId),
    Guide(GuideId),
    Notch(NotchId),
    Dimension(DimensionId),
    FullDimension(FullDimensionId),
    Topstitch(TopstitchId),
    Piece(PieceId),
    SeamPair(SeamPairId),
    /// Variable, par son nom (elle n'appartient à aucune feuille).
    Variable(String),
    /// Taille d'une gamme (US-09.2a) : elle n'appartient à aucune feuille.
    Size(SizeId),
}

/// Nature d'une dépendance (de quel type de lien il s'agit), dans l'ordre d'affichage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DependencyNature {
    /// Parent de construction d'un point construit (tracé, point ou ligne d'aide).
    Construction,
    /// Point d'un tracé (extrémité, centre, point de passage).
    PathPoint,
    /// Construction maintenue : parent d'un point suivi.
    KeptConstruction,
    /// Variable citée par la formule d'un paramètre.
    FormulaReference,
    /// Variable citée par l'expression d'une autre variable.
    VariableReference,
    /// Longueur imposée : le point d'extrémité ajusté par le lien.
    ImposedLength,
    /// Égalité de longueurs : tracé de référence.
    LengthEquality,
    /// Cran : tracé qui le porte, point ou tracé de son ancrage.
    NotchReference,
    /// Cotation ou cote complète : point ou tracé mesuré.
    DimensionReference,
    /// Source d'une surpiqûre liée.
    TopstitchSource,
    /// Ligne générée par une surpiqûre liée.
    TopstitchLine,
    /// Pièce : tracé de son contour.
    PieceOutline,
    /// Pièce : droit-fil (ligne et tracé de référence).
    PieceGrain,
    /// Pièce : bord sur le pli.
    PieceFold,
    /// Pièce : élément interne.
    PieceInternal,
    /// Pièce : marquage (perçage, croix, fente).
    PieceMarking,
    /// Pièce : pince.
    PieceDart,
    /// Pièce : groupe de boutons.
    PieceButtons,
    /// Pièce dérivée : pièce mère (ou pliure de dépliage).
    PieceDerived,
    /// Couple de coutures appariées : pièce et tracés cousus.
    SeamPair,
    /// Gradation de taille (US-09.2a) : la taille dont l'écart déplace un point libre.
    SizeGrading,
}

impl DependencyNature {
    /// Identifiant stable, indépendant de la langue.
    pub fn key(self) -> &'static str {
        match self {
            DependencyNature::Construction => "construction",
            DependencyNature::PathPoint => "path_point",
            DependencyNature::KeptConstruction => "kept_construction",
            DependencyNature::FormulaReference => "formula_reference",
            DependencyNature::VariableReference => "variable_reference",
            DependencyNature::ImposedLength => "imposed_length",
            DependencyNature::LengthEquality => "length_equality",
            DependencyNature::NotchReference => "notch_reference",
            DependencyNature::DimensionReference => "dimension_reference",
            DependencyNature::TopstitchSource => "topstitch_source",
            DependencyNature::TopstitchLine => "topstitch_line",
            DependencyNature::PieceOutline => "piece_outline",
            DependencyNature::PieceGrain => "piece_grain",
            DependencyNature::PieceFold => "piece_fold",
            DependencyNature::PieceInternal => "piece_internal",
            DependencyNature::PieceMarking => "piece_marking",
            DependencyNature::PieceDart => "piece_dart",
            DependencyNature::PieceButtons => "piece_buttons",
            DependencyNature::PieceDerived => "piece_derived",
            DependencyNature::SeamPair => "seam_pair",
            DependencyNature::SizeGrading => "size_grading",
        }
    }
}

/// Arête du graphe : `child` dépend de `parent`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DependencyEdge {
    pub parent: DependencyObject,
    pub child: DependencyObject,
    pub nature: DependencyNature,
}

/// Un objet de l'arbre, avec la façon dont on l'atteint depuis l'objet de départ.
#[derive(Debug, Clone, PartialEq)]
pub struct DependencyEntry {
    pub object: DependencyObject,
    /// Feuille de l'objet ; `None` pour une variable.
    pub sheet: Option<SheetId>,
    /// 1 = lien direct, 2 et plus = transitif.
    pub depth: usize,
    /// Nature du lien qui relie l'objet à `via` (le plus petit en cas de liens parallèles).
    pub nature: DependencyNature,
    /// Objet intermédiaire : l'objet de départ pour une profondeur 1.
    pub via: DependencyObject,
}

/// Arbre de dépendances d'un objet : ses parents (dont il dépend) et ses enfants (dépendants).
#[derive(Debug, Clone, PartialEq)]
pub struct DependencyTree {
    pub object: DependencyObject,
    pub sheet: Option<SheetId>,
    /// Objets dont `object` dépend, triés par profondeur puis par objet.
    pub parents: Vec<DependencyEntry>,
    /// Objets qui dépendent de `object`, triés par profondeur puis par objet.
    pub children: Vec<DependencyEntry>,
}

impl DependencyTree {
    /// Parents directs.
    pub fn direct_parents(&self) -> impl Iterator<Item = &DependencyEntry> {
        self.parents.iter().filter(|e| e.depth == 1)
    }

    /// Enfants directs.
    pub fn direct_children(&self) -> impl Iterator<Item = &DependencyEntry> {
        self.children.iter().filter(|e| e.depth == 1)
    }
}

/// Ce que ferait une suppression, annoncé avant de l'exécuter.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DeletionImpact {
    /// Objets demandés (ceux de la sélection).
    pub requested: Vec<DependencyObject>,
    /// Message de refus : la commande réelle serait refusée (rien ne serait modifié).
    pub refused: Option<String>,
    /// Objets supprimés **en plus** des objets demandés (crans, cotations, pièces…), hors
    /// extrémités libres nettoyées.
    pub removed: Vec<DependencyObject>,
    /// Extrémités libres des tracés supprimés, retirées avec eux (comportement habituel).
    pub cleaned: Vec<DependencyObject>,
    /// Liens retirés alors que l'enfant est conservé (point construit rendu libre, formule ou
    /// construction maintenue retirée, contour de pièce vidé…) : `parent` est l'objet supprimé
    /// ou un objet conservé dont le lien disparaît.
    pub unlinked: Vec<DependencyEdge>,
    /// Remarques du cœur (celles que la commande renverrait).
    pub notes: Vec<String>,
}

impl DeletionImpact {
    /// Suppression sans conséquence sur d'autres objets : ni objet supprimé en plus, ni lien
    /// retiré. Un refus n'est pas « sans conséquence » : l'appelant le traite à part.
    pub fn is_trivial(&self) -> bool {
        self.removed.is_empty() && self.unlinked.is_empty()
    }
}

impl Project {
    /// Tous les objets du graphe, avec leur feuille (`None` pour une variable).
    fn dependency_nodes(&self) -> BTreeMap<DependencyObject, Option<SheetId>> {
        let mut nodes = BTreeMap::new();
        for s in &self.sheets {
            let sid = Some(s.id);
            for p in &s.points {
                nodes.insert(DependencyObject::Point(p.id), sid);
            }
            for e in &s.entities {
                nodes.insert(DependencyObject::Entity(e.id), sid);
            }
            for g in &s.guides {
                nodes.insert(DependencyObject::Guide(g.id), sid);
            }
            for n in &s.notches {
                nodes.insert(DependencyObject::Notch(n.id), sid);
            }
            for d in &s.dimensions {
                nodes.insert(DependencyObject::Dimension(d.id), sid);
            }
            for d in &s.full_dimensions {
                nodes.insert(DependencyObject::FullDimension(d.id), sid);
            }
            for t in &s.topstitches {
                nodes.insert(DependencyObject::Topstitch(t.id), sid);
            }
        }
        for p in &self.pieces {
            let sheet = self.piece_sheet(p.id).ok().map(|s| s.id);
            nodes.insert(DependencyObject::Piece(p.id), sheet);
        }
        for sp in &self.seams {
            let sheet = self.piece_sheet(sp.a.piece).ok().map(|s| s.id);
            nodes.insert(DependencyObject::SeamPair(sp.id), sheet);
        }
        for v in &self.variables {
            nodes.insert(DependencyObject::Variable(v.name.clone()), None);
        }
        for r in &self.sizes {
            for s in &r.sizes {
                nodes.insert(DependencyObject::Size(s.id), None);
            }
        }
        nodes
    }

    /// Graphe complet des dépendances : arêtes uniques, triées. Une arête n'existe que si ses
    /// deux extrémités existent (un fichier altéré ne produit pas d'objet fantôme).
    pub fn dependency_edges(&self) -> Vec<DependencyEdge> {
        let nodes = self.dependency_nodes();
        self.raw_dependency_edges()
            .into_iter()
            .filter(|e| {
                e.parent != e.child && nodes.contains_key(&e.parent) && nodes.contains_key(&e.child)
            })
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    fn raw_dependency_edges(&self) -> Vec<DependencyEdge> {
        use DependencyNature as N;
        use DependencyObject as O;
        let mut out: Vec<DependencyEdge> = Vec::new();
        let mut add = |parent: O, child: O, nature: N| {
            out.push(DependencyEdge {
                parent,
                child,
                nature,
            })
        };
        let var_names: HashSet<&str> = self.variables.iter().map(|v| v.name.as_str()).collect();
        let cited = |text: &str| -> Vec<String> {
            expr::parse(text)
                .map(|e| e.references().into_iter().map(|r| r.name).collect())
                .unwrap_or_default()
        };
        for v in &self.variables {
            for name in cited(&v.expr) {
                if var_names.contains(name.as_str()) {
                    add(
                        O::Variable(name),
                        O::Variable(v.name.clone()),
                        N::VariableReference,
                    );
                }
            }
        }
        for s in &self.sheets {
            for p in &s.points {
                let child = O::Point(p.id);
                for e in p.definition.dependencies() {
                    add(O::Entity(e), child.clone(), N::Construction);
                }
                for q in p.definition.point_dependencies() {
                    add(O::Point(q), child.clone(), N::Construction);
                }
                if let Some(g) = p.definition.axis_guide() {
                    add(O::Guide(g), child.clone(), N::Construction);
                }
            }
            for e in &s.entities {
                for p in e.kind.all_points() {
                    add(O::Point(p), O::Entity(e.id), N::PathPoint);
                }
            }
            for link in s.kept_links() {
                let followed: Vec<PointId> = link.dependents();
                let mut parents: Vec<O> = Vec::new();
                match link.construction {
                    Construction::Polar { .. } => parents.push(O::Point(link.from)),
                    Construction::Perpendicular { reference, .. } => {
                        parents.push(O::Point(link.from));
                        parents.push(O::Entity(reference));
                    }
                    Construction::Parallel { reference, .. } => parents.push(O::Entity(reference)),
                }
                for parent in parents {
                    for &f in &followed {
                        add(parent.clone(), O::Point(f), N::KeptConstruction);
                    }
                }
            }
            for g in &s.gradings {
                add(O::Size(g.size), O::Point(g.point), N::SizeGrading);
            }
            for f in &s.formulas {
                for name in cited(&f.expr) {
                    if var_names.contains(name.as_str()) {
                        add(O::Variable(name), O::Point(f.point), N::FormulaReference);
                    }
                }
            }
            for c in &s.length_constraints {
                for text in c.formula_texts() {
                    for name in cited(text) {
                        if var_names.contains(name.as_str()) {
                            add(O::Variable(name), O::Entity(c.curve), N::FormulaReference);
                        }
                    }
                }
                // Somme de tracés (US-03.4b) : un lien par tracé, de toute feuille (identifiants
                // uniques dans le projet).
                for t in c.terms() {
                    add(O::Entity(t.entity), O::Entity(c.curve), N::LengthEquality);
                }
                if let LengthAdjust::Slide { point, .. } = c.adjust {
                    add(O::Entity(c.curve), O::Point(point), N::ImposedLength);
                }
            }
            for n in &s.notches {
                let child = O::Notch(n.id);
                add(O::Entity(n.entity), child.clone(), N::NotchReference);
                match n.anchor {
                    Some(NotchAnchor::Point { point }) => {
                        add(O::Point(point), child.clone(), N::NotchReference)
                    }
                    Some(NotchAnchor::Carried { entity, point, .. }) => {
                        add(O::Point(point), child.clone(), N::NotchReference);
                        add(O::Entity(entity), child.clone(), N::NotchReference);
                    }
                    _ => {}
                }
            }
            for d in &s.dimensions {
                add(O::Point(d.from), O::Dimension(d.id), N::DimensionReference);
                add(O::Point(d.to), O::Dimension(d.id), N::DimensionReference);
            }
            for d in &s.full_dimensions {
                let child = O::FullDimension(d.id);
                let parents: Vec<O> = match d.kind {
                    FullDimensionKind::PathLength { entity, .. }
                    | FullDimensionKind::Radius { entity, .. }
                    | FullDimensionKind::Diameter { entity, .. } => vec![O::Entity(entity)],
                    FullDimensionKind::Angle {
                        vertex,
                        first,
                        second,
                        ..
                    } => vec![O::Point(vertex), O::Point(first), O::Point(second)],
                    FullDimensionKind::Horizontal { from, to, .. }
                    | FullDimensionKind::Vertical { from, to, .. } => {
                        vec![O::Point(from), O::Point(to)]
                    }
                };
                for parent in parents {
                    add(parent, child.clone(), N::DimensionReference);
                }
            }
            for t in &s.topstitches {
                let child = O::Topstitch(t.id);
                match &t.source {
                    TopstitchSource::Piece { piece } => {
                        add(O::Piece(*piece), child.clone(), N::TopstitchSource)
                    }
                    TopstitchSource::Paths { paths, from } => {
                        for e in paths {
                            add(O::Entity(*e), child.clone(), N::TopstitchSource);
                        }
                        if let Some(p) = from {
                            add(O::Point(*p), child.clone(), N::TopstitchSource);
                        }
                    }
                }
                for e in &t.paths {
                    add(child.clone(), O::Entity(*e), N::TopstitchLine);
                }
            }
        }
        for piece in &self.pieces {
            let child = O::Piece(piece.id);
            for e in &piece.outline {
                add(O::Entity(e.entity), child.clone(), N::PieceOutline);
            }
            if let Some(g) = piece.grainline {
                add(O::Entity(g), child.clone(), N::PieceGrain);
            }
            if let Some(grain) = &piece.grain {
                let mut directions = vec![grain.direction];
                for x in &grain.extra {
                    add(O::Entity(x.entity), child.clone(), N::PieceGrain);
                    directions.push(x.direction);
                }
                for d in directions {
                    match d {
                        GrainDirection::Parallel { reference }
                        | GrainDirection::Perpendicular { reference } => {
                            add(O::Entity(reference), child.clone(), N::PieceGrain)
                        }
                        GrainDirection::Drawn {} | GrainDirection::Bias { .. } => {}
                    }
                }
            }
            for e in &piece.on_fold {
                add(O::Entity(*e), child.clone(), N::PieceFold);
            }
            for i in &piece.internal {
                let parent = match i.target {
                    InternalTarget::Path { entity } => O::Entity(entity),
                    InternalTarget::Point { point } => O::Point(point),
                    InternalTarget::Notch { notch } => O::Notch(notch),
                };
                add(parent, child.clone(), N::PieceInternal);
            }
            for m in &piece.markings {
                let points = match m.at {
                    MarkingAnchor::Point { point } => vec![point],
                    MarkingAnchor::DartTip { tip, toward, .. } => vec![tip, toward],
                };
                for p in points {
                    add(O::Point(p), child.clone(), N::PieceMarking);
                }
            }
            for d in &piece.darts {
                for p in d.points() {
                    add(O::Point(p), child.clone(), N::PieceDart);
                }
            }
            for b in &piece.buttons {
                add(O::Point(b.from), child.clone(), N::PieceButtons);
                add(O::Point(b.to), child.clone(), N::PieceButtons);
            }
            if let Some(d) = &piece.derived {
                add(O::Piece(d.mother), child.clone(), N::PieceDerived);
                if let crate::derived::DerivedOp::Unfold { fold } = d.operation {
                    add(O::Entity(fold), child.clone(), N::PieceDerived);
                }
            }
        }
        for sp in &self.seams {
            let child = O::SeamPair(sp.id);
            for side in [&sp.a, &sp.b] {
                add(O::Piece(side.piece), child.clone(), N::SeamPair);
                for e in &side.edges {
                    add(O::Entity(*e), child.clone(), N::SeamPair);
                }
            }
        }
        out
    }

    /// Arbre de dépendances de `object` : ses parents et ses enfants, directs puis transitifs
    /// (`max_depth` : profondeur maximale, `None` = tout). `None` si l'objet n'existe pas.
    ///
    /// Chaque objet apparaît une fois, à sa profondeur la plus courte ; l'objet de départ
    /// n'apparaît jamais dans ses propres listes (même sur un graphe à boucle).
    pub fn dependency_tree(
        &self,
        object: &DependencyObject,
        max_depth: Option<usize>,
    ) -> Option<DependencyTree> {
        let nodes = self.dependency_nodes();
        let sheet = *nodes.get(object)?;
        let edges = self.dependency_edges();
        let mut up: HashMap<&DependencyObject, Vec<(&DependencyObject, DependencyNature)>> =
            HashMap::new();
        let mut down: HashMap<&DependencyObject, Vec<(&DependencyObject, DependencyNature)>> =
            HashMap::new();
        for e in &edges {
            up.entry(&e.child).or_default().push((&e.parent, e.nature));
            down.entry(&e.parent)
                .or_default()
                .push((&e.child, e.nature));
        }
        let walk =
            |adjacency: &HashMap<&DependencyObject, Vec<(&DependencyObject, DependencyNature)>>| {
                let mut seen: HashSet<&DependencyObject> = HashSet::new();
                seen.insert(object);
                let mut entries: Vec<DependencyEntry> = Vec::new();
                let mut frontier: Vec<&DependencyObject> = vec![object];
                let mut depth = 0usize;
                while !frontier.is_empty() && max_depth.is_none_or(|m| depth < m) {
                    depth += 1;
                    let mut next: Vec<&DependencyObject> = Vec::new();
                    for via in frontier {
                        let Some(list) = adjacency.get(via) else {
                            continue;
                        };
                        for &(other, nature) in list {
                            if seen.insert(other) {
                                next.push(other);
                                entries.push(DependencyEntry {
                                    object: other.clone(),
                                    sheet: nodes.get(other).copied().flatten(),
                                    depth,
                                    nature,
                                    via: via.clone(),
                                });
                            }
                        }
                    }
                    next.sort();
                    frontier = next;
                }
                entries.sort_by(|a, b| (a.depth, &a.object).cmp(&(b.depth, &b.object)));
                entries
            };
        Some(DependencyTree {
            object: object.clone(),
            sheet,
            parents: walk(&up),
            children: walk(&down),
        })
    }

    /// Commande exacte qui supprime ces objets de la feuille `sheet` (celle de l'éditeur :
    /// cotations, crans et lignes d'aide d'abord, puis points et tracés d'un coup ; un
    /// `Batch` s'il y a plusieurs commandes). `None` si rien n'est à supprimer ; un type
    /// d'objet qui n'a pas de suppression directe est ignoré.
    pub fn deletion_command(sheet: SheetId, objects: &[DependencyObject]) -> Option<Command> {
        let mut commands: Vec<Command> = Vec::new();
        for o in objects {
            if let DependencyObject::Dimension(dimension) = o {
                commands.push(Command::DeleteDimension {
                    sheet,
                    dimension: *dimension,
                });
            }
        }
        for o in objects {
            if let DependencyObject::Notch(notch) = o {
                commands.push(Command::DeleteNotch {
                    sheet,
                    notch: *notch,
                });
            }
        }
        for o in objects {
            if let DependencyObject::Guide(guide) = o {
                commands.push(Command::DeleteGuide {
                    sheet,
                    guide: *guide,
                });
            }
        }
        let points: Vec<PointId> = objects
            .iter()
            .filter_map(|o| match o {
                DependencyObject::Point(p) => Some(*p),
                _ => None,
            })
            .collect();
        let entities: Vec<EntityId> = objects
            .iter()
            .filter_map(|o| match o {
                DependencyObject::Entity(e) => Some(*e),
                _ => None,
            })
            .collect();
        if !points.is_empty() || !entities.is_empty() {
            commands.push(Command::Delete {
                sheet,
                points,
                entities,
            });
        }
        match commands.len() {
            0 => None,
            1 => commands.pop(),
            _ => Some(Command::Batch { commands }),
        }
    }

    /// Ce que ferait la suppression de `objects` (feuille `sheet`) : rejoue la vraie commande
    /// sur une copie du projet et compare les graphes avant / après. Lecture seule.
    pub fn deletion_impact(&self, sheet: SheetId, objects: &[DependencyObject]) -> DeletionImpact {
        let mut requested: Vec<DependencyObject> = objects.to_vec();
        requested.sort();
        requested.dedup();
        let mut impact = DeletionImpact {
            requested,
            ..DeletionImpact::default()
        };
        let Some(command) = Project::deletion_command(sheet, objects) else {
            return impact;
        };
        let mut after = self.clone();
        let outcome = match after.apply(command) {
            Ok(o) => o,
            Err(e) => {
                impact.refused = Some(e.to_string());
                return impact;
            }
        };
        impact.notes = outcome.notes;
        let before_nodes = self.dependency_nodes();
        let after_nodes = after.dependency_nodes();
        let asked: HashSet<&DependencyObject> = impact.requested.iter().collect();
        // Extrémités libres des tracés demandés : nettoyées avec eux (comportement habituel).
        let mut ends: HashSet<PointId> = HashSet::new();
        if let Ok(s) = self.sheet(sheet) {
            for o in &impact.requested {
                if let DependencyObject::Entity(e) = o {
                    if let Ok(points) = s.entity_all_points(*e) {
                        ends.extend(
                            points
                                .into_iter()
                                .filter(|p| s.point(*p).is_ok_and(|p| p.definition.is_free())),
                        );
                    }
                }
            }
        }
        let gone: BTreeSet<DependencyObject> = before_nodes
            .keys()
            .filter(|o| !after_nodes.contains_key(*o))
            .cloned()
            .collect();
        for o in &gone {
            if asked.contains(o) {
                continue;
            }
            match o {
                DependencyObject::Point(p) if ends.contains(p) => impact.cleaned.push(o.clone()),
                _ => impact.removed.push(o.clone()),
            }
        }
        let before_edges = self.dependency_edges();
        let after_edges: HashSet<DependencyEdge> = after.dependency_edges().into_iter().collect();
        for e in before_edges {
            if !after_edges.contains(&e)
                && after_nodes.contains_key(&e.child)
                && before_nodes.contains_key(&e.child)
            {
                impact.unlinked.push(e);
            }
        }
        impact
    }
}
