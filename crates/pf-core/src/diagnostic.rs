//! US-03.5 — diagnostic des **liens** du projet (lecture seule, **jamais stocké**).
//!
//! `Project::link_diagnostics` relit, à chaque appel, les quatre mécanismes de liens et renvoie
//! une liste triée et déterministe de [`LinkDiagnostic`] :
//!
//! - formules d'un paramètre (US-03.1b, `Sheet.formulas`) ;
//! - constructions maintenues (US-03.2a) : les points d'arrivée sont des points **suivis** ;
//! - longueurs imposées (US-03.3a) et égalités de longueurs (US-03.4a) : « longueur imposée »
//!   ne désigne que ces courbes ;
//! - variables (US-03.1a) : inutilisées = avertissement ; table invalide = erreur ;
//! - gradation (US-09.2a) : seuls les **conflits** sont listés (un point qui porte un écart de
//!   taille **et** que suit une construction maintenue ou une longueur imposée : le lien n'est
//!   pas réévalué par taille), en avertissement.
//!
//! Aucune logique de contrôle n'est dupliquée : l'état « résolu / non résolu » vient des mêmes
//! contrôles que l'ouverture du fichier (`formula_problems`, `kept_problems`, `length_problems`,
//! en mode « collecte » : tous les problèmes, sans s'arrêter au premier). Un lien qui échouerait
//! à l'ouverture est une **erreur** ; il n'apparaît alors pas aussi comme lien actif.
//!
//! Dans l'état actuel, une commande ne laisse jamais un lien non résolu (elle est refusée sans
//! trace) : les erreurs ne se voient donc que sur un fichier altéré ou un état construit à la
//! main ; les liens actifs, les calques verrouillés et les variables inutilisées sont les cas
//! courants. Un projet sans aucun lien ni variable donne une liste vide.
//!
//! Limite assumée : la détection « calque verrouillé » porte sur l'objet **écrit** par le lien
//! (point suivi, segment maintenu, courbe ou extrémité ajustée) ; un autre objet verrouillé qui
//! dépendrait de ce point n'est pas vu.

use crate::error::CoreError;
use crate::expr;
use crate::formula::ParamKind;
use crate::grading::GradingFollower;
use crate::length_constraint::LengthAdjust;
use crate::model::{EntityId, PointId, Project, Sheet, SheetId};
use crate::variable::evaluate_variables;
use std::collections::{HashMap, HashSet};

/// Gravité, de la plus grave à la moins grave (ordre de tri et de groupement).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DiagnosticSeverity {
    /// Lien non résolu : il serait refusé à l'ouverture du fichier.
    Error,
    /// Lien fragile : une propagation serait refusée, ou variable sans usage.
    Warning,
    /// Lien actif et résolu.
    Info,
}

/// Mécanisme de lien concerné, dans l'ordre d'affichage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LinkType {
    /// Formule d'un paramètre (point en direction ou décalé).
    Formula,
    /// Construction maintenue (segment longueur / angle, perpendiculaire, parallèle).
    KeptConstruction,
    /// Longueur imposée par une valeur (courbe de Bézier).
    ImposedLength,
    /// Longueur imposée égale à celle d'un autre tracé + décalage.
    LengthEquality,
    /// Variable (table du projet).
    Variable,
    /// Écart de gradation d'un point (US-09.2a) : conflit avec un lien non réévalué par taille.
    PointGrading,
}

/// Nature du constat.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DiagnosticKind {
    /// Lien actif, résolu (information).
    Active,
    /// Lien non résolu (erreur) : référence cassée, écart au-delà de la tolérance, cycle…
    Broken,
    /// L'objet écrit par le lien est sur un calque verrouillé (avertissement).
    LockedLayer,
    /// Variable que ni une formule ni une autre variable ne cite (avertissement).
    UnusedVariable,
    /// Point gradué que suit aussi un lien maintenu ou une longueur imposée (avertissement) : le
    /// lien n'est pas réévalué par taille, la position graduée ignore ce qu'il dicterait.
    GradedAndFollowed,
}

impl DiagnosticKind {
    /// Gravité attachée à la nature du constat.
    pub fn severity(self) -> DiagnosticSeverity {
        match self {
            DiagnosticKind::Broken => DiagnosticSeverity::Error,
            DiagnosticKind::LockedLayer
            | DiagnosticKind::UnusedVariable
            | DiagnosticKind::GradedAndFollowed => DiagnosticSeverity::Warning,
            DiagnosticKind::Active => DiagnosticSeverity::Info,
        }
    }
}

/// Objet concerné par un constat.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum DiagnosticObject {
    Point(PointId),
    Entity(EntityId),
    /// Variable, par son nom (elle n'appartient à aucune feuille).
    Variable(String),
}

/// Un constat du diagnostic. Aucun texte d'interface : l'application compose le message avec
/// ses catalogues (seul `detail` est le message du cœur, en français, pour les erreurs).
#[derive(Debug, Clone, PartialEq)]
pub struct LinkDiagnostic {
    pub severity: DiagnosticSeverity,
    pub kind: DiagnosticKind,
    pub link: LinkType,
    /// Feuille de l'objet principal ; `None` pour une variable.
    pub sheet: Option<SheetId>,
    /// Objets concernés ; le premier est l'objet principal (celui qu'un clic sélectionne). Les
    /// points d'un lien de formule, de construction maintenue ou de longueur imposée (extrémité
    /// glissante) sont des points **suivis**, dont la position est dictée par le lien.
    pub objects: Vec<DiagnosticObject>,
    /// Texte de la formule, ou nom de la variable ; vide sinon.
    pub text: String,
    /// Paramètre d'une formule.
    pub param: Option<ParamKind>,
    /// Valeur courante du paramètre (formule), ou longueur imposée en mm.
    pub value: Option<f64>,
    /// Décalage signé d'une égalité de longueurs, en mm.
    pub offset_mm: Option<f64>,
    /// Message du cœur pour une erreur (déjà localisé : feuille, objet) ; vide sinon.
    pub detail: String,
    /// Ce qui suit le point gradué (conflit de gradation) ; `None` sinon.
    pub follower: Option<GradingFollower>,
}

impl LinkDiagnostic {
    fn new(
        kind: DiagnosticKind,
        link: LinkType,
        sheet: Option<SheetId>,
        objects: Vec<DiagnosticObject>,
    ) -> LinkDiagnostic {
        LinkDiagnostic {
            severity: kind.severity(),
            kind,
            link,
            sheet,
            objects,
            text: String::new(),
            param: None,
            value: None,
            offset_mm: None,
            detail: String::new(),
            follower: None,
        }
    }

    /// Objet principal (celui qu'un clic sélectionne), s'il y en a un.
    pub fn primary(&self) -> Option<&DiagnosticObject> {
        self.objects.first()
    }

    /// Points suivis par ce lien (position dictée par lui).
    pub fn followed_points(&self) -> impl Iterator<Item = PointId> + '_ {
        self.objects.iter().filter_map(|o| match o {
            DiagnosticObject::Point(p) => Some(*p),
            _ => None,
        })
    }
}

type Keyed = Vec<(usize, LinkDiagnostic)>;

impl Project {
    /// Diagnostic des liens du projet, calculé à la lecture (lecture seule, aucune trace dans le
    /// fichier). Trié par gravité (erreurs, avertissements, informations), puis par type de
    /// lien, puis par feuille, puis dans l'ordre de stockage : deux appels donnent la même
    /// liste, et une relecture du fichier aussi.
    pub fn link_diagnostics(&self) -> Vec<LinkDiagnostic> {
        let mut keyed: Keyed = Vec::new();
        self.diagnose_variables(&mut keyed);
        let formula_problems = self.formula_problems();
        let length_formulas = self.length_formula_problems();
        for (si, sheet) in self.sheets.iter().enumerate() {
            diagnose_formulas(sheet, si, &formula_problems, &mut keyed);
            diagnose_kept(sheet, si, &mut keyed);
            let formula_lengths: Vec<(Option<EntityId>, CoreError)> = length_formulas
                .iter()
                .filter(|(s, _, _)| *s == sheet.id)
                .map(|(_, c, e)| (Some(*c), CoreError::Invalid(e.to_string())))
                .collect();
            diagnose_lengths(self, sheet, si, formula_lengths, &mut keyed);
            diagnose_gradings(sheet, si, &mut keyed);
        }
        // Tri stable : à clé égale, l'ordre de génération (ordre de stockage) est conservé.
        keyed.sort_by_key(|(si, d)| (d.severity, d.link, *si));
        keyed.into_iter().map(|(_, d)| d).collect()
    }

    fn diagnose_variables(&self, out: &mut Keyed) {
        if self.variables.is_empty() {
            return;
        }
        if let Err(e) = evaluate_variables(&self.variables) {
            let mut d =
                LinkDiagnostic::new(DiagnosticKind::Broken, LinkType::Variable, None, Vec::new());
            d.detail = CoreError::from(e).to_string();
            out.push((0, d));
        }
        let mut cited: HashSet<String> = HashSet::new();
        let texts = self.variables.iter().map(|v| v.expr.as_str()).chain(
            self.sheets
                .iter()
                .flat_map(|s| s.formulas.iter())
                .map(|f| f.expr.as_str()),
        );
        let texts = texts.chain(
            self.sheets
                .iter()
                .flat_map(|s| s.length_constraints.iter())
                .flat_map(|c| c.formula_texts()),
        );
        for text in texts {
            if let Ok(e) = expr::parse(text) {
                cited.extend(e.references().into_iter().map(|r| r.name));
            }
        }
        for v in &self.variables {
            if !cited.contains(&v.name) {
                let mut d = LinkDiagnostic::new(
                    DiagnosticKind::UnusedVariable,
                    LinkType::Variable,
                    None,
                    vec![DiagnosticObject::Variable(v.name.clone())],
                );
                d.text = v.name.clone();
                out.push((0, d));
            }
        }
    }
}

fn diagnose_formulas(
    sheet: &Sheet,
    si: usize,
    problems: &Result<Vec<(SheetId, usize, CoreError)>, CoreError>,
    out: &mut Keyed,
) {
    for (index, f) in sheet.formulas.iter().enumerate() {
        let objects = vec![DiagnosticObject::Point(f.point)];
        let broken = match problems {
            Err(e) => Some(e.to_string()),
            Ok(list) => list
                .iter()
                .find(|(s, i, _)| *s == sheet.id && *i == index)
                .map(|(_, _, e)| e.to_string()),
        };
        let kind = if broken.is_some() {
            DiagnosticKind::Broken
        } else {
            DiagnosticKind::Active
        };
        let mut d = LinkDiagnostic::new(kind, LinkType::Formula, Some(sheet.id), objects.clone());
        d.text = f.expr.clone();
        d.param = Some(f.param);
        d.value = sheet.parameter_value(f.point, f.param);
        d.detail = broken.unwrap_or_default();
        out.push((si, d));
        if sheet.point_locked(f.point) {
            let mut w = LinkDiagnostic::new(
                DiagnosticKind::LockedLayer,
                LinkType::Formula,
                Some(sheet.id),
                objects,
            );
            w.text = f.expr.clone();
            w.param = Some(f.param);
            out.push((si, w));
        }
    }
}

/// Premier message d'erreur par segment, et message des problèmes non attribuables.
fn split_problems(
    problems: Vec<(Option<EntityId>, CoreError)>,
) -> (HashMap<EntityId, String>, Option<String>) {
    let mut by_object = HashMap::new();
    let mut general = None;
    for (at, e) in problems {
        match at {
            Some(id) => {
                by_object.entry(id).or_insert_with(|| e.to_string());
            }
            None => {
                general.get_or_insert_with(|| e.to_string());
            }
        }
    }
    (by_object, general)
}

/// Conflits de gradation : un point gradué que suit un lien non réévalué par taille.
fn diagnose_gradings(sheet: &Sheet, si: usize, out: &mut Keyed) {
    let mut seen = HashSet::new();
    for g in &sheet.gradings {
        if !seen.insert(g.point) {
            continue;
        }
        if let Some(by) = sheet.grading_follower(g.point) {
            let mut d = LinkDiagnostic::new(
                DiagnosticKind::GradedAndFollowed,
                LinkType::PointGrading,
                Some(sheet.id),
                vec![DiagnosticObject::Point(g.point)],
            );
            d.follower = Some(by);
            out.push((si, d));
        }
    }
}

fn diagnose_kept(sheet: &Sheet, si: usize, out: &mut Keyed) {
    let links = sheet.kept_links();
    if links.is_empty() {
        return;
    }
    let (broken, general) = split_problems(sheet.kept_problems());
    let mut unplaced = Vec::new();
    for link in &links {
        let mut objects = vec![DiagnosticObject::Entity(link.segment)];
        objects.extend(link.dependents().into_iter().map(DiagnosticObject::Point));
        let locked = sheet.entity_locked(link.segment)
            || link.dependents().into_iter().any(|p| sheet.point_locked(p));
        if let Some(detail) = broken.get(&link.segment) {
            let mut d = LinkDiagnostic::new(
                DiagnosticKind::Broken,
                LinkType::KeptConstruction,
                Some(sheet.id),
                objects.clone(),
            );
            d.detail = detail.clone();
            out.push((si, d));
        } else if general.is_some() {
            // Cycle ou autre problème d'ensemble : on ne sait pas quels liens sont résolus.
            unplaced.push(DiagnosticObject::Entity(link.segment));
        } else {
            out.push((
                si,
                LinkDiagnostic::new(
                    DiagnosticKind::Active,
                    LinkType::KeptConstruction,
                    Some(sheet.id),
                    objects.clone(),
                ),
            ));
        }
        if locked {
            out.push((
                si,
                LinkDiagnostic::new(
                    DiagnosticKind::LockedLayer,
                    LinkType::KeptConstruction,
                    Some(sheet.id),
                    objects,
                ),
            ));
        }
    }
    if let Some(detail) = general {
        let mut d = LinkDiagnostic::new(
            DiagnosticKind::Broken,
            LinkType::KeptConstruction,
            Some(sheet.id),
            unplaced,
        );
        d.detail = detail;
        out.push((si, d));
    }
}

fn diagnose_lengths(
    project: &Project,
    sheet: &Sheet,
    si: usize,
    formula_problems: Vec<(Option<EntityId>, CoreError)>,
    out: &mut Keyed,
) {
    if sheet.length_constraints.is_empty() {
        return;
    }
    let mut problems = sheet.length_problems(project);
    problems.extend(formula_problems);
    let (broken, general) = split_problems(problems);
    let mut unplaced = Vec::new();
    for c in &sheet.length_constraints {
        let link = if c.reference.is_some() {
            LinkType::LengthEquality
        } else {
            LinkType::ImposedLength
        };
        let mut objects = vec![DiagnosticObject::Entity(c.curve)];
        // Tracés de la même feuille seulement : un clic sélectionne dans la feuille du lien.
        for t in c.terms().iter().filter(|t| t.sheet.is_none()) {
            objects.push(DiagnosticObject::Entity(t.entity));
        }
        let slid = match c.adjust {
            LengthAdjust::Slide { point, .. } => Some(point),
            LengthAdjust::Handle { .. } => None,
        };
        objects.extend(slid.map(DiagnosticObject::Point));
        let locked = sheet.entity_locked(c.curve) || slid.is_some_and(|p| sheet.point_locked(p));
        let fill = |d: &mut LinkDiagnostic| {
            d.value = Some(c.length_mm);
            d.offset_mm = c.reference.map(|r| r.offset_mm);
            if let Some(text) = c.formula_texts().next() {
                d.text = text.to_string();
            }
        };
        if let Some(detail) = broken.get(&c.curve) {
            let mut d = LinkDiagnostic::new(
                DiagnosticKind::Broken,
                link,
                Some(sheet.id),
                objects.clone(),
            );
            fill(&mut d);
            d.detail = detail.clone();
            out.push((si, d));
        } else if general.is_some() {
            unplaced.push(DiagnosticObject::Entity(c.curve));
        } else {
            let mut d = LinkDiagnostic::new(
                DiagnosticKind::Active,
                link,
                Some(sheet.id),
                objects.clone(),
            );
            fill(&mut d);
            out.push((si, d));
        }
        if locked {
            let mut d =
                LinkDiagnostic::new(DiagnosticKind::LockedLayer, link, Some(sheet.id), objects);
            fill(&mut d);
            out.push((si, d));
        }
    }
    if let Some(detail) = general {
        let mut d = LinkDiagnostic::new(
            DiagnosticKind::Broken,
            LinkType::ImposedLength,
            Some(sheet.id),
            unplaced,
        );
        d.detail = detail;
        out.push((si, d));
    }
}
