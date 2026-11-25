//! US-04.4 — pièces dérivées : une pièce créée à partir d'une autre (parementure,
//! doublure, entoilage, renfort) par décalage de son contour, **liée** à la pièce
//! mère et recalculée quand celle-ci change (décisions 116 à 127).
//!
//! Le lien est une **dérivation recalculée**, pas un solveur ni une contrainte :
//! après chaque commande, les dérivées dont la mère a changé sont régénérées dans
//! la même étape atomique. La dérivée possède sa géométrie dans une feuille
//! dédiée (de vrais tracés) ; ses identifiants restent stables d'une régénération
//! à l'autre tant que la topologie du décalage est identique.

use crate::command::Outcome;
use crate::contour::{contour_from_entities, measure_contour};
use crate::error::CoreError;
use crate::model::*;
use crate::offset::trace_of;
use crate::sheet_copy::unique_name;
use pf_geometry::{distance_mm, offset_chain, OffsetOptions, PointMm, Trace, MAX_OFFSET_MM};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Nombre maximal d'ancêtres d'une pièce dérivée (dérivée de dérivée de…).
pub const MAX_DERIVATION_DEPTH: usize = 8;
/// Plus petite distance de décalage acceptée (mm, valeur absolue).
pub const MIN_DERIVED_OFFSET_MM: f64 = 0.01;
/// Plus grande distance de décalage acceptée (mm, valeur absolue).
pub const MAX_DERIVED_OFFSET_MM: f64 = MAX_OFFSET_MM;
/// Écart maximal (mm) entre la fin d'un tracé du décalage et le début du suivant :
/// les raccords entre une Bézier approchée (2e-5 mm) et un autre tracé ne sont pas
/// exacts ; la tolérance est celle du filtre des Béziers de `pf_geometry::offset_chain`.
pub(crate) const CHAIN_GAP_MM: f64 = 1e-4;
/// Longueur maximale de la raison d'obsolescence enregistrée.
pub(crate) const MAX_STALE_LEN: usize = 500;

/// Nature d'une pièce dérivée. **Simple étiquette** (vocabulaire non prouvé, à
/// faire relire par une modéliste) : elle n'implique ni marge, ni règle de coupe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DerivedKind {
    /// Parementure.
    Facing,
    /// Doublure.
    Lining,
    /// Entoilage.
    Interfacing,
    /// Renfort.
    Reinforcement,
    /// Pièce entière dépliée d'après une pièce construite sur une pliure (US-04.7) :
    /// jamais proposée pour un décalage (hors de [`DerivedKind::ALL`]), seulement avec
    /// l'opération [`DerivedOp::Unfold`].
    Unfolded,
}

impl DerivedKind {
    /// Types proposés pour un décalage (la pièce dépliée n'en fait pas partie).
    pub const ALL: [DerivedKind; 4] = [
        DerivedKind::Facing,
        DerivedKind::Lining,
        DerivedKind::Interfacing,
        DerivedKind::Reinforcement,
    ];

    /// Identifiant stable, identique à la valeur écrite dans le fichier.
    pub fn key(self) -> &'static str {
        match self {
            DerivedKind::Facing => "facing",
            DerivedKind::Lining => "lining",
            DerivedKind::Interfacing => "interfacing",
            DerivedKind::Reinforcement => "reinforcement",
            DerivedKind::Unfolded => "unfolded",
        }
    }

    /// Catégorie de matière donnée à la création (la parementure n'en a pas :
    /// la catégorie « parementure » n'existe pas).
    pub fn category(self) -> Option<PieceCategory> {
        match self {
            DerivedKind::Facing | DerivedKind::Unfolded => None,
            DerivedKind::Lining => Some(PieceCategory::Lining),
            DerivedKind::Interfacing => Some(PieceCategory::Interfacing),
            DerivedKind::Reinforcement => Some(PieceCategory::Reinforcement),
        }
    }

    /// Libellé français (nom proposé par défaut pour la pièce).
    pub fn label_fr(self) -> &'static str {
        match self {
            DerivedKind::Facing => "parementure",
            DerivedKind::Lining => "doublure",
            DerivedKind::Interfacing => "entoilage",
            DerivedKind::Reinforcement => "renfort",
            DerivedKind::Unfolded => "dépliée",
        }
    }
}

/// Opération qui produit le contour de la dérivée à partir de celui de la mère.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum DerivedOp {
    /// Décalage du contour de la mère : **positif = vers l'extérieur, négatif =
    /// vers l'intérieur**, quel que soit le sens d'enregistrement du contour.
    Offset { distance_mm: f64 },
    /// Pièce entière : la moitié de la mère, qui a pour pliure son tracé `fold`
    /// (un segment de son contour), suivie de son image en miroir (US-04.7). Si le tracé
    /// disparaît du contour, la dépliée est marquée obsolète, jamais la commande refusée.
    Unfold { fold: EntityId },
}

impl DerivedOp {
    /// Distance de décalage signée (mm) ; `None` pour une opération sans décalage.
    pub fn distance_mm(&self) -> Option<f64> {
        match self {
            DerivedOp::Offset { distance_mm } => Some(*distance_mm),
            DerivedOp::Unfold { .. } => None,
        }
    }

    /// Paramètres dans les bornes : fini, 0,01 mm ≤ |d| ≤ 100 000 mm.
    pub fn validate(&self) -> Result<(), CoreError> {
        let Some(d) = self.distance_mm() else {
            return Ok(());
        };
        if !d.is_finite() || d.abs() < MIN_DERIVED_OFFSET_MM || d.abs() > MAX_DERIVED_OFFSET_MM {
            return Err(CoreError::Invalid(
                "pièce dérivée : la distance de décalage doit être un nombre fini, d'au moins \
                 0,01 mm et d'au plus 100 000 mm en valeur absolue"
                    .into(),
            ));
        }
        Ok(())
    }

    /// Contour de la dérivée d'après la chaîne **anti-horaire** de la mère :
    /// une seule chaîne fermée (ou un cercle), sinon une raison.
    fn apply(&self, chain: &[Trace]) -> Result<Vec<Trace>, String> {
        let Some(d) = self.distance_mm() else {
            return Err("cette opération n'est pas un décalage".into());
        };
        // Chaîne anti-horaire : l'extérieur est à droite, donc `d` négatif.
        let out = offset_chain(chain, -d, &OffsetOptions::default()).map_err(|e| e.to_string())?;
        check_closed_chain(&out)?;
        Ok(out)
    }
}

/// Le résultat du décalage est un cercle, ou une chaîne fermée d'au moins deux
/// segments, courbes ou arcs dont les extrémités consécutives se rejoignent.
fn check_closed_chain(out: &[Trace]) -> Result<(), String> {
    if matches!(out, [Trace::Circle(_)]) {
        return Ok(());
    }
    let unusable = || "le décalage ne donne pas un contour exploitable".to_string();
    if out.len() < 2
        || out
            .iter()
            .any(|t| !matches!(t, Trace::Line(..) | Trace::Cubic(_) | Trace::Arc(_)))
    {
        return Err(unusable());
    }
    for j in 0..out.len() {
        let next = &out[(j + 1) % out.len()];
        if distance_mm(trace_end(&out[j]), trace_start(next)) > CHAIN_GAP_MM {
            return Err(
                "le décalage ne donne pas un contour fermé d'un seul tenant (la forme se sépare \
                 en plusieurs morceaux ou n'est plus fermée)"
                    .into(),
            );
        }
    }
    Ok(())
}

pub(crate) fn trace_start(t: &Trace) -> PointMm {
    match t {
        Trace::Line(a, _) => *a,
        Trace::Cubic(c) => c.p0,
        Trace::Arc(a) => a.start_point(),
        Trace::Circle(c) => c.center,
        Trace::Spline(s) => s.start_point(),
    }
}

/// Position du sommet qui précède le tracé `j` : l'extrémité de l'arc qui le précède
/// (un arc doit rester exact : son rayon est contrôlé), sinon le début du tracé `j`.
pub(crate) fn joint_at(traces: &[Trace], j: usize) -> PointMm {
    match &traces[(j + traces.len() - 1) % traces.len()] {
        Trace::Arc(a) => a.end_point(),
        _ => trace_start(&traces[j]),
    }
}

pub(crate) fn trace_end(t: &Trace) -> PointMm {
    match t {
        Trace::Line(_, b) => *b,
        Trace::Cubic(c) => c.p3,
        Trace::Arc(a) => a.end_point(),
        Trace::Circle(c) => c.center,
        Trace::Spline(s) => s.end_point(),
    }
}

/// Lien d'une pièce dérivée avec sa mère (champ `derived` de la pièce).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DerivedSpec {
    pub mother: PieceId,
    pub kind: DerivedKind,
    pub operation: DerivedOp,
    /// Raison pour laquelle la dernière mise à jour a échoué ; la dérivée garde
    /// alors sa dernière géométrie valide. Absent = à jour.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stale: Option<String>,
}

/// État d'une pièce dérivée.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DerivedState {
    /// Calculée d'après l'état actuel de la mère.
    Current,
    /// La dernière mise à jour a échoué : géométrie de la dernière version valide.
    Stale { reason: String },
}

/// Lecture d'une dérivation (décision 126).
#[derive(Debug, Clone, PartialEq)]
pub struct DerivedInfo {
    pub mother: PieceId,
    pub mother_name: String,
    pub kind: DerivedKind,
    pub operation: DerivedOp,
    pub state: DerivedState,
}

/// Opération de dérivation d'une pièce, si elle est dérivée.
fn spec_op(project: &Project, piece: PieceId) -> Option<DerivedOp> {
    project
        .piece(piece)
        .ok()
        .and_then(|p| p.derived.as_ref())
        .map(|d| d.operation)
}

/// Raison réduite à une ligne bornée (pas de caractère de contrôle).
pub(crate) fn clean_reason(reason: &str) -> String {
    let flat: String = reason
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let flat = flat.trim();
    if flat.chars().count() > MAX_STALE_LEN {
        let cut: String = flat.chars().take(MAX_STALE_LEN - 1).collect();
        format!("{cut}…")
    } else {
        flat.to_string()
    }
}

/// Contour de la mère prêt à être décalé.
struct MotherChain {
    /// Chaîne anti-horaire.
    traces: Vec<Trace>,
    /// Rôle commun des tracés de la mère, s'il existe.
    role: Option<PathRole>,
    area_mm2: f64,
}

/// Ce qu'il faut contrôler sur la géométrie d'une dérivée une fois construite.
#[derive(Debug, Clone, Copy)]
enum Check {
    /// Décalage : aire cohérente avec le sens du décalage.
    Offset { mother_area: f64, distance_mm: f64 },
    /// Dépliage : aire égale à deux fois celle de la moitié (1e-9 relatif).
    Unfold { half_area: f64 },
}

/// Contour calculé d'une dérivation.
struct Computed {
    traces: Vec<Trace>,
    /// Rôle commun des tracés de la mère, s'il existe.
    role: Option<PathRole>,
    check: Check,
}

/// Modifications à appliquer sur place à une feuille dérivée.
#[derive(Default)]
pub(crate) struct Plan {
    pub(crate) points: Vec<(PointId, PointMm)>,
    pub(crate) curves: Vec<(EntityId, OffsetMm, OffsetMm)>,
    pub(crate) arcs: Vec<(EntityId, bool)>,
    pub(crate) circles: Vec<(EntityId, f64)>,
}

/// Plan de mise à jour sur place : seulement si la feuille a exactement la
/// topologie du décalage (même nombre de tracés, même type à chaque rang, mêmes
/// points partagés, points libres) ; `None` sinon.
fn in_place_plan(sheet: &Sheet, outline: &[PieceEdge], traces: &[Trace]) -> Option<Plan> {
    if outline.len() != traces.len() || outline.iter().any(|e| e.reversed) {
        return None;
    }
    let free = |id: PointId| sheet.point(id).is_ok_and(|p| p.definition.is_free());
    let mut plan = Plan::default();
    if let [Trace::Circle(c)] = traces {
        let EntityKind::Circle { center, .. } = sheet.entity(outline[0].entity).ok()?.kind else {
            return None;
        };
        if !free(center) {
            return None;
        }
        plan.points.push((center, c.center));
        plan.circles.push((outline[0].entity, c.radius_mm));
        return Some(plan);
    }
    let n = traces.len();
    let mut starts = Vec::with_capacity(n);
    let mut ends = Vec::with_capacity(n);
    for (edge, trace) in outline.iter().zip(traces) {
        let kind = &sheet.entity(edge.entity).ok()?.kind;
        match (kind, trace) {
            (EntityKind::Segment { from, to, .. }, Trace::Line(..)) => {
                starts.push(*from);
                ends.push(*to);
            }
            (EntityKind::Curve { start, end, .. }, Trace::Cubic(_)) => {
                starts.push(*start);
                ends.push(*end);
            }
            (
                EntityKind::Arc {
                    center, start, end, ..
                },
                Trace::Arc(a),
            ) => {
                if !free(*center) {
                    return None;
                }
                plan.points.push((*center, a.center));
                plan.arcs.push((edge.entity, a.sweep_rad > 0.0));
                starts.push(*start);
                ends.push(*end);
            }
            _ => return None,
        }
    }
    for j in 0..n {
        let next = (j + 1) % n;
        if ends[j] != starts[next] || !free(starts[j]) {
            return None;
        }
        plan.points.push((starts[j], joint_at(traces, j)));
        if let Trace::Cubic(c) = &traces[j] {
            let (ps, pe) = (joint_at(traces, j), joint_at(traces, next));
            plan.curves.push((
                outline[j].entity,
                OffsetMm::from_vector(c.c1 - ps),
                OffsetMm::from_vector(c.c2 - pe),
            ));
        }
    }
    Some(plan)
}

impl Project {
    /// Nombre d'ancêtres de `piece` (0 si elle n'est pas dérivée) ; borné : une
    /// chaîne qui dépasse [`MAX_DERIVATION_DEPTH`] ou boucle renvoie `MAX + 1`.
    fn derivation_depth(&self, piece: PieceId) -> usize {
        let mut depth = 0;
        let mut cur = self.piece(piece).ok().and_then(|p| p.derived.as_ref());
        while let Some(spec) = cur {
            depth += 1;
            if depth > MAX_DERIVATION_DEPTH {
                break;
            }
            cur = self
                .piece(spec.mother)
                .ok()
                .and_then(|p| p.derived.as_ref());
        }
        depth
    }

    /// Lecture de la dérivation d'une pièce ; `None` si elle n'est pas dérivée.
    pub fn piece_derivation(&self, piece: PieceId) -> Option<DerivedInfo> {
        let spec = self.piece(piece).ok()?.derived.as_ref()?;
        Some(DerivedInfo {
            mother: spec.mother,
            mother_name: self
                .piece(spec.mother)
                .map(|m| m.name.clone())
                .unwrap_or_default(),
            kind: spec.kind,
            operation: spec.operation,
            state: match &spec.stale {
                None => DerivedState::Current,
                Some(reason) => DerivedState::Stale {
                    reason: reason.clone(),
                },
            },
        })
    }

    /// Pièces directement dérivées de `mother`, dans l'ordre du projet.
    pub fn derived_pieces_of(&self, mother: PieceId) -> Vec<PieceId> {
        self.pieces
            .iter()
            .filter(|p| p.derived.as_ref().is_some_and(|d| d.mother == mother))
            .map(|p| p.id)
            .collect()
    }

    /// Validation du champ `derived` d'une pièce (à l'ouverture et après chaque
    /// commande) : feuille dédiée, mère existante, ni auto-référence ni cycle,
    /// profondeur bornée, paramètres dans les bornes, raison bornée.
    pub(crate) fn validate_derived(&self, piece: &Piece) -> Result<(), CoreError> {
        let Some(spec) = &piece.derived else {
            return Ok(());
        };
        let name = &piece.name;
        if !matches!(piece.link, PieceLink::Linked { .. }) {
            return Err(CoreError::Invalid(format!(
                "la pièce dérivée « {name} » doit être liée à sa feuille dédiée"
            )));
        }
        spec.operation.validate()?;
        if (spec.kind == DerivedKind::Unfolded)
            != matches!(spec.operation, DerivedOp::Unfold { .. })
        {
            return Err(CoreError::Invalid(format!(
                "la pièce dérivée « {name} » : le type et l'opération de dérivation sont incohérents"
            )));
        }
        if let Some(reason) = &spec.stale {
            if reason.trim().is_empty()
                || reason.chars().count() > MAX_STALE_LEN
                || reason.chars().any(|c| c.is_control())
            {
                return Err(CoreError::Invalid(format!(
                    "la pièce dérivée « {name} » porte une raison d'obsolescence invalide"
                )));
            }
        }
        if spec.mother == piece.id {
            return Err(CoreError::Invalid(format!(
                "la pièce « {name} » ne peut pas être dérivée d'elle-même"
            )));
        }
        let mut cur = Some(spec.mother);
        let mut depth = 0;
        while let Some(m) = cur {
            depth += 1;
            if m == piece.id {
                return Err(CoreError::Invalid(format!(
                    "la pièce « {name} » fait partie d'un cycle de dérivation"
                )));
            }
            if depth > MAX_DERIVATION_DEPTH {
                return Err(CoreError::Invalid(format!(
                    "la chaîne de dérivation de la pièce « {name} » dépasse \
                     {MAX_DERIVATION_DEPTH} niveaux (ou contient un cycle)"
                )));
            }
            let mother = self.piece(m).map_err(|_| {
                CoreError::DeadReference(format!(
                    "la pièce dérivée « {name} » référence une pièce mère absente (id {})",
                    m.0
                ))
            })?;
            cur = mother.derived.as_ref().map(|d| d.mother);
        }
        Ok(())
    }

    /// Contour de la mère, orienté anti-horaire, prêt à être décalé ; une raison
    /// si la pièce n'a pas de contour fermé simple et mesurable.
    fn mother_chain(&self, mother: PieceId) -> Result<MotherChain, String> {
        let p = self.piece(mother).map_err(|e| e.to_string())?;
        if p.outline.is_empty() {
            return Err(format!("la pièce mère « {} » n'a pas de contour", p.name));
        }
        let sheet = self.piece_sheet(mother).map_err(|e| e.to_string())?;
        let ids: Vec<EntityId> = p.outline.iter().map(|e| e.entity).collect();
        let contour = contour_from_entities(sheet, &ids).map_err(|e| {
            format!(
                "le contour de la pièce mère « {} » n'est pas une chaîne fermée ({e})",
                p.name
            )
        })?;
        let m = measure_contour(sheet, &contour).map_err(|e| {
            format!(
                "le contour de la pièce mère « {} » est illisible ({e})",
                p.name
            )
        })?;
        if m.self_intersecting {
            return Err(format!(
                "le contour de la pièce mère « {} » se recoupe lui-même",
                p.name
            ));
        }
        if m.area_mm2.is_nan() || m.area_mm2 <= 1e-9 {
            return Err(format!(
                "le contour de la pièce mère « {} » est dégénéré (aire nulle)",
                p.name
            ));
        }
        let mut edges = contour.edges.clone();
        if m.clockwise && edges.len() > 1 {
            edges.reverse();
            for e in &mut edges {
                e.reversed = !e.reversed;
            }
        }
        let traces = edges
            .iter()
            .map(|e| {
                sheet
                    .shape(e.entity)
                    .map(|s| trace_of(s, e.reversed))
                    .map_err(|e| e.to_string())
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(MotherChain {
            traces,
            role: contour.role,
            area_mm2: m.area_mm2,
        })
    }

    /// Contour que produit `op` à partir de la pièce `mother`, avec ce qu'il faut vérifier.
    fn compute_derived(&self, mother: PieceId, op: &DerivedOp) -> Result<Computed, String> {
        match *op {
            DerivedOp::Offset { distance_mm } => {
                let chain = self.mother_chain(mother)?;
                let traces = op.apply(&chain.traces)?;
                Ok(Computed {
                    traces,
                    role: chain.role,
                    check: Check::Offset {
                        mother_area: chain.area_mm2,
                        distance_mm,
                    },
                })
            }
            DerivedOp::Unfold { fold } => {
                let g = self.unfold_geometry(mother, fold)?;
                Ok(Computed {
                    traces: g.traces,
                    role: Some(g.role),
                    check: Check::Unfold {
                        half_area: g.half_area_mm2,
                    },
                })
            }
        }
    }

    /// Signature du contour d'une pièce : les tracés de sa chaîne, dans l'ordre
    /// canonique (sens du plus petit identifiant). Bon marché (pas de mesure) :
    /// sert à savoir si la géométrie a changé pendant une commande.
    pub(crate) fn outline_signature(&self, piece: PieceId) -> Result<Vec<Trace>, String> {
        let p = self.piece(piece).map_err(|e| e.to_string())?;
        if p.outline.is_empty() {
            return Err("sans contour".into());
        }
        let sheet = self.piece_sheet(piece).map_err(|e| e.to_string())?;
        let ids: Vec<EntityId> = p.outline.iter().map(|e| e.entity).collect();
        let contour = contour_from_entities(sheet, &ids).map_err(|e| e.to_string())?;
        contour
            .edges
            .iter()
            .map(|e| {
                sheet
                    .shape(e.entity)
                    .map(|s| trace_of(s, e.reversed))
                    .map_err(|e| e.to_string())
            })
            .collect()
    }

    pub(crate) fn add_free_point(
        &mut self,
        sheet: SheetId,
        at: PointMm,
    ) -> Result<PointId, String> {
        at.validate().map_err(|e| e.to_string())?;
        let id = PointId(self.alloc_id());
        let s = self.sheet_mut(sheet).map_err(|e| e.to_string())?;
        let label = s.next_label();
        s.points.push(Point {
            id,
            label,
            definition: PointDefinition::Free { x: at.x, y: at.y },
            role: PointRole::Construction,
            layer: None,
        });
        Ok(id)
    }

    pub(crate) fn add_entity(
        &mut self,
        sheet: SheetId,
        role: PathRole,
        kind: EntityKind,
    ) -> Result<EntityId, String> {
        let id = EntityId(self.alloc_id());
        self.sheet_mut(sheet)
            .map_err(|e| e.to_string())?
            .entities
            .push(Entity {
                id,
                role,
                layer: None,
                name: None,
                kind,
            });
        Ok(id)
    }

    /// Crée les tracés du décalage dans `sheet` (points libres, jamais fusionnés) ;
    /// renvoie le contour dans l'ordre de la chaîne.
    fn build_derived_geometry(
        &mut self,
        sheet: SheetId,
        traces: &[Trace],
        role: PathRole,
    ) -> Result<Vec<PieceEdge>, String> {
        let edge = |entity| PieceEdge {
            entity,
            reversed: false,
        };
        if let [Trace::Circle(c)] = traces {
            let center = self.add_free_point(sheet, c.center)?;
            let id = self.add_entity(
                sheet,
                role,
                EntityKind::Circle {
                    center,
                    radius_mm: c.radius_mm,
                    start_deg: 0.0,
                    ccw: true,
                },
            )?;
            return Ok(vec![edge(id)]);
        }
        let n = traces.len();
        let mut joints = Vec::with_capacity(n);
        for j in 0..n {
            joints.push(self.add_free_point(sheet, joint_at(traces, j))?);
        }
        let mut outline = Vec::with_capacity(n);
        for (j, t) in traces.iter().enumerate() {
            let next = (j + 1) % n;
            let (a, b) = (joints[j], joints[next]);
            let kind = match t {
                Trace::Line(..) => EntityKind::Segment {
                    from: a,
                    to: b,
                    construction: None,
                },
                Trace::Cubic(c) => EntityKind::Curve {
                    start: a,
                    handle_start: OffsetMm::from_vector(c.c1 - joint_at(traces, j)),
                    handle_end: OffsetMm::from_vector(c.c2 - joint_at(traces, next)),
                    end: b,
                },
                Trace::Arc(arc) => EntityKind::Arc {
                    center: self.add_free_point(sheet, arc.center)?,
                    start: a,
                    end: b,
                    ccw: arc.sweep_rad > 0.0,
                },
                _ => return Err("le décalage ne donne pas un contour exploitable".into()),
            };
            outline.push(edge(self.add_entity(sheet, role, kind)?));
        }
        Ok(outline)
    }

    pub(crate) fn apply_plan(&mut self, sheet: SheetId, plan: Plan) -> Result<(), String> {
        let s = self.sheet_mut(sheet).map_err(|e| e.to_string())?;
        for (id, at) in plan.points {
            let p = s
                .points
                .iter_mut()
                .find(|p| p.id == id)
                .ok_or_else(|| "point de la dérivée introuvable".to_string())?;
            p.definition = PointDefinition::Free { x: at.x, y: at.y };
        }
        for (id, hs, he) in plan.curves {
            if let EntityKind::Curve {
                handle_start,
                handle_end,
                ..
            } = &mut s.entity_mut(id).map_err(|e| e.to_string())?.kind
            {
                *handle_start = hs;
                *handle_end = he;
            }
        }
        for (id, new_ccw) in plan.arcs {
            if let EntityKind::Arc { ccw, .. } =
                &mut s.entity_mut(id).map_err(|e| e.to_string())?.kind
            {
                *ccw = new_ccw;
            }
        }
        for (id, radius) in plan.circles {
            if let EntityKind::Circle { radius_mm, .. } =
                &mut s.entity_mut(id).map_err(|e| e.to_string())?.kind
            {
                *radius_mm = radius;
            }
        }
        Ok(())
    }

    /// Contrôle de la géométrie d'une dérivée : feuille valide, contour fermé
    /// simple, orienté anti-horaire, aire cohérente avec le sens du décalage, et
    /// pièce valide. Une raison sinon.
    fn verify_derived_geometry(&self, piece: PieceId, check: Check) -> Result<(), String> {
        let p = self.piece(piece).map_err(|e| e.to_string())?;
        let PieceLink::Linked { sheet } = p.link else {
            return Err("pièce dérivée sans feuille dédiée".into());
        };
        self.sheet(sheet)
            .map_err(|e| e.to_string())?
            .validate()
            .map_err(|e| e.to_string())?;
        let c = self.piece_contour(piece).map_err(|e| e.to_string())?;
        let bad = |why: &str| match check {
            Check::Offset { .. } => format!("le décalage ne donne pas un contour valide ({why})"),
            Check::Unfold { .. } => format!("le dépliage ne donne pas un contour valide ({why})"),
        };
        if !c.closed {
            return Err(bad("contour non fermé"));
        }
        if c.self_intersecting {
            return Err(bad("le contour se recoupe"));
        }
        if c.clockwise && p.outline.len() > 1 {
            return Err(bad("contour inversé"));
        }
        let area = c.area_mm2;
        let coherent = area.is_finite()
            && area > 1e-9
            && match check {
                Check::Offset {
                    mother_area,
                    distance_mm,
                } => {
                    if distance_mm > 0.0 {
                        area > mother_area - 1e-6
                    } else {
                        area < mother_area + 1e-6
                    }
                }
                Check::Unfold { half_area } => {
                    (area - 2.0 * half_area).abs() <= 1e-9 * (2.0 * half_area).max(1.0)
                }
            };
        if !coherent {
            return Err(bad(match check {
                Check::Offset { .. } => "aire incohérente avec la distance de décalage",
                Check::Unfold { .. } => "aire différente du double de celle de la moitié",
            }));
        }
        self.validate_piece(p).map_err(|e| e.to_string())
    }

    /// Crée la dérivée de `mother` : feuille dédiée, pièce liée, géométrie.
    pub(crate) fn create_derived_piece(
        &mut self,
        mother: PieceId,
        kind: DerivedKind,
        operation: DerivedOp,
        name: Option<String>,
        out: &mut Outcome,
    ) -> Result<(), CoreError> {
        operation.validate()?;
        let mother_name = self.piece(mother)?.name.clone();
        if self.derivation_depth(mother) + 1 > MAX_DERIVATION_DEPTH {
            return Err(CoreError::Invalid(format!(
                "pièce dérivée : au plus {MAX_DERIVATION_DEPTH} niveaux de dérivation"
            )));
        }
        if (kind == DerivedKind::Unfolded) != matches!(operation, DerivedOp::Unfold { .. }) {
            return Err(CoreError::Invalid(
                "pièce dérivée : le type « dépliée » va avec l'opération de dépliage, et elle seule"
                    .into(),
            ));
        }
        let source = self.piece(mother)?.clone();
        let (name, code) = match operation {
            DerivedOp::Unfold { fold } => {
                self.check_not_already_unfolded(mother, fold)?;
                Project::unfolded_identity(&source, fold, name.as_deref(), out)
            }
            DerivedOp::Offset { .. } => {
                let name = match name.as_deref().map(str::trim) {
                    Some(n) if !n.is_empty() => n.to_string(),
                    _ => format!("{mother_name} – {}", kind.label_fr()),
                };
                (name, None)
            }
        };
        // Crans (fixes ou ancrés, US-06.2) : jamais reportés, jamais perdus en silence.
        if let Ok(sheet) = self.piece_sheet(mother) {
            let on_contour = sheet
                .notches
                .iter()
                .filter(|n| source.outline.iter().any(|e| e.entity == n.entity))
                .count();
            if on_contour > 0 {
                out.notes.push(format!(
                    "{on_contour} cran(s) posé(s) sur le contour de la pièce « {} » ne sont pas \
                     repris sur la pièce dérivée (ils restent sur la feuille d'origine).",
                    source.name
                ));
            }
        }
        // Surpiqûres liées au contour de la mère (US-06.8) : jamais reportées.
        if let Ok(sheet) = self.piece_sheet(mother) {
            let linked = sheet
                .topstitches
                .iter()
                .filter(|t| t.source == crate::topstitch::TopstitchSource::Piece { piece: mother })
                .count();
            if linked > 0 {
                out.notes.push(format!(
                    "{linked} surpiqûre(s) liée(s) au contour de la pièce « {} » ne sont pas                      reprises sur la pièce dérivée (elles restent sur la feuille d'origine).",
                    source.name
                ));
            }
        }
        check_name(&name, "le nom de la pièce")?;
        let computed = self
            .compute_derived(mother, &operation)
            .map_err(CoreError::Invalid)?;
        let taken: HashSet<String> = self.sheets.iter().map(|s| s.name.clone()).collect();
        let sid = SheetId(self.alloc_id());
        self.sheets
            .push(Sheet::new(sid, &unique_name(&name, &taken)));
        let outline = self
            .build_derived_geometry(sid, &computed.traces, computed.role.unwrap_or_default())
            .map_err(CoreError::Invalid)?;
        let id = PieceId(self.alloc_id());
        self.pieces.push(Piece {
            id,
            name,
            code,
            // Une pièce dépliée hérite de la fiche de sa source (sauf le commentaire et la
            // paire, qui décrivent la moitié) ; un décalage repart d'une fiche neuve.
            quantity: if kind == DerivedKind::Unfolded {
                source.quantity
            } else {
                1
            },
            material: if kind == DerivedKind::Unfolded {
                source.material.clone()
            } else {
                None
            },
            category: if kind == DerivedKind::Unfolded {
                source.category
            } else {
                kind.category()
            },
            // Déclaration « sans droit-fil » (PH-5d) reprise de la source : une dérivée liée ne se
            // déclare pas elle-même.
            no_grain: source.no_grain,
            outline: outline.clone(),
            derived: Some(DerivedSpec {
                mother,
                kind,
                operation,
                stale: None,
            }),
            link: PieceLink::Linked { sheet: sid },
            ..Piece::default()
        });
        self.verify_derived_geometry(id, computed.check)
            .map_err(CoreError::Invalid)?;
        out.sheet = Some(sid);
        out.pieces.push(id);
        out.entities.extend(outline.iter().map(|e| e.entity));
        Ok(())
    }

    /// Casse le lien : la pièce devient ordinaire (sa géométrie reste).
    pub(crate) fn break_derived_link(&mut self, piece: PieceId) -> Result<(), CoreError> {
        let p = self.piece_mut(piece)?;
        if p.derived.take().is_none() {
            return Err(CoreError::Invalid(format!(
                "la pièce « {} » n'est pas une pièce dérivée : il n'y a pas de lien à casser",
                p.name
            )));
        }
        Ok(())
    }

    /// Une mère supprimée par la commande : le lien de ses dérivées est cassé dans
    /// la même étape, avec une remarque (décision 124).
    pub(crate) fn break_orphan_derivations(&mut self, before: &Project, out: &mut Outcome) {
        if !self.pieces.iter().any(|p| p.derived.is_some()) {
            return;
        }
        let orphans: Vec<PieceId> = self
            .pieces
            .iter()
            .filter(|p| {
                p.derived.as_ref().is_some_and(|d| {
                    !self.pieces.iter().any(|m| m.id == d.mother)
                        && before.pieces.iter().any(|m| m.id == d.mother)
                })
            })
            .map(|p| p.id)
            .collect();
        for id in orphans {
            if let Ok(p) = self.piece_mut(id) {
                p.derived = None;
                out.notes.push(format!(
                    "La pièce « {} » n'est plus dérivée : sa pièce mère a été supprimée.",
                    p.name
                ));
            }
        }
    }

    /// Après une commande : refuse de modifier directement la géométrie d'une
    /// dérivée liée, puis régénère les dérivées dont la mère a changé, dans l'ordre
    /// de profondeur (décisions 120 à 122). `before` est le projet avant la commande.
    pub(crate) fn sync_derived(
        &mut self,
        before: &Project,
        out: &mut Outcome,
    ) -> Result<(), CoreError> {
        // (profondeur, dérivée, mère, créée par cette commande)
        let mut todo: Vec<(usize, PieceId, PieceId, bool)> = Vec::new();
        for p in &self.pieces {
            let Some(spec) = &p.derived else { continue };
            let old = before
                .pieces
                .iter()
                .find(|o| o.id == p.id)
                .and_then(|o| o.derived.as_ref());
            match old {
                // Créée par la commande : recalculée d'après l'état final de la mère
                // (un `Batch` peut la modifier après la création).
                None => todo.push((self.derivation_depth(p.id), p.id, spec.mother, true)),
                Some(old)
                    if old.mother == spec.mother
                        && old.kind == spec.kind
                        && old.operation == spec.operation =>
                {
                    todo.push((self.derivation_depth(p.id), p.id, spec.mother, false));
                }
                Some(_) => {}
            }
        }
        todo.sort();
        for (_, d, m, created) in todo {
            if created {
                self.regenerate_with_rollback(d, out);
                continue;
            }
            // Un dépliage dépend aussi de l'identité des tracés du contour (la pliure).
            let unfold_target_changed = matches!(spec_op(self, d), Some(DerivedOp::Unfold { .. }))
                && before.piece(m).map(|p| &p.outline).ok()
                    != self.piece(m).map(|p| &p.outline).ok();
            let mother_changed =
                unfold_target_changed || before.outline_signature(m) != self.outline_signature(m);
            let own_changed = before.outline_signature(d) != self.outline_signature(d);
            // La régénération est la seule à écrire la géométrie d'une dérivée liée
            // (même si la mère change dans la même commande).
            if own_changed {
                let name = self.piece(d).map(|p| p.name.clone()).unwrap_or_default();
                let mother = self.piece(m).map(|p| p.name.clone()).unwrap_or_default();
                return Err(CoreError::Invalid(format!(
                    "la pièce « {name} » est dérivée de « {mother} » : sa géométrie est \
                     recalculée d'après celle-ci. Cassez le lien avant de la modifier."
                )));
            }
            if mother_changed {
                self.regenerate_with_rollback(d, out);
            }
        }
        Ok(())
    }

    /// Régénère une dérivée ; en cas d'échec, la feuille et la pièce reviennent à
    /// l'identique et la pièce est marquée obsolète (jamais un refus).
    fn regenerate_with_rollback(&mut self, d: PieceId, out: &mut Outcome) {
        let Ok(piece) = self.piece(d) else { return };
        let PieceLink::Linked { sheet: sid } = piece.link else {
            return;
        };
        let backup_piece = piece.clone();
        let Ok(backup_sheet) = self.sheet(sid).cloned() else {
            return;
        };
        let (next_id, notes_len) = (self.next_id, out.notes.len());
        match self.regenerate(d, sid, out) {
            Ok(()) => {
                if let Some(spec) = self.piece_mut(d).ok().and_then(|p| p.derived.as_mut()) {
                    spec.stale = None;
                }
                out.regenerated.push(d);
            }
            Err(reason) => {
                if let Some(s) = self.sheets.iter_mut().find(|s| s.id == sid) {
                    *s = backup_sheet;
                }
                if let Some(p) = self.pieces.iter_mut().find(|p| p.id == d) {
                    *p = backup_piece;
                }
                self.next_id = next_id;
                out.notes.truncate(notes_len);
                let reason = clean_reason(&reason);
                if let Ok(p) = self.piece_mut(d) {
                    out.notes.push(format!(
                        "La pièce dérivée « {} » n'a pas pu être mise à jour ({reason}) : elle \
                         garde sa dernière géométrie valide.",
                        p.name
                    ));
                    if let Some(spec) = p.derived.as_mut() {
                        spec.stale = Some(reason);
                    }
                }
            }
        }
    }

    fn regenerate(&mut self, d: PieceId, sid: SheetId, out: &mut Outcome) -> Result<(), String> {
        let piece = self.piece(d).map_err(|e| e.to_string())?;
        let Some(spec) = piece.derived.clone() else {
            return Ok(());
        };
        let (name, outline) = (piece.name.clone(), piece.outline.clone());
        let computed = self.compute_derived(spec.mother, &spec.operation)?;
        let plan = self
            .sheet(sid)
            .ok()
            .and_then(|s| in_place_plan(s, &outline, &computed.traces));
        match plan {
            Some(plan) => self.apply_plan(sid, plan)?,
            None => {
                let role = outline
                    .first()
                    .and_then(|e| self.sheet(sid).ok()?.entity(e.entity).ok().map(|x| x.role))
                    .or(computed.role)
                    .unwrap_or_default();
                if !outline.is_empty() {
                    let old: Vec<EntityId> = outline.iter().map(|e| e.entity).collect();
                    let mark = out.notes.len();
                    self.delete(sid, &[], &old, out)
                        .map_err(|e| e.to_string())?;
                    out.notes.truncate(mark);
                }
                let fresh = self.build_derived_geometry(sid, &computed.traces, role)?;
                let p = self.piece_mut(d).map_err(|e| e.to_string())?;
                p.outline = fresh;
                out.notes.push(format!(
                    "La géométrie de la pièce dérivée « {name} » a changé de structure : ses \
                     tracés sont recréés (nouveaux identifiants)."
                ));
            }
        }
        self.prune_own_internals(d, out);
        self.prune_own_markings(d, out);
        self.prune_own_buttons(d, out);
        self.verify_derived_geometry(d, computed.check)
    }

    /// Retire les éléments internes de la dérivée dont la cible a été recréée.
    fn prune_own_internals(&mut self, d: PieceId, out: &mut Outcome) {
        let Ok(piece) = self.piece(d) else { return };
        let link = piece.link;
        let gone: Vec<InternalTarget> = piece
            .internal
            .iter()
            .filter(|e| !self.internal_target_exists(link, e.target))
            .map(|e| e.target)
            .collect();
        if gone.is_empty() {
            return;
        }
        if let Ok(p) = self.piece_mut(d) {
            p.internal.retain(|e| !gone.contains(&e.target));
            for t in gone {
                out.notes.push(format!(
                    "Un élément interne de la pièce « {} » ({}) référençait un tracé recréé par \
                     la mise à jour : il est retiré.",
                    p.name,
                    crate::piece_internal::target_text(t)
                ));
            }
        }
    }
}
