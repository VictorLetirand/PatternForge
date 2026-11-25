//! US-02.10 — décalage de contour : chaînes de tracés, côté, commande `OffsetPaths`.
//!
//! La géométrie (courbe parallèle, coins, auto-intersections) est dans
//! `pf_geometry::offset_chain` ; ce module ordonne les tracés bout à bout et
//! crée le résultat comme **nouveaux** tracés indépendants de l'original.
//!
//! Sens de la chaîne : celui du tracé de plus petit identifiant, tel qu'il est
//! enregistré ; les autres tracés sont parcourus à la suite (retournés si
//! besoin). `distance_mm > 0` = à **gauche** de ce sens. Un cercle seul est
//! parcouru dans son propre sens (`ccw`) : à gauche = vers le centre s'il est
//! anti-horaire. L'interface choisit le signe avec [`offset_side`], jamais à
//! la main.

use crate::command::{Outcome, MERGE_TOLERANCE_MM};
use crate::error::CoreError;
use crate::model::*;
use pf_geometry::{
    distance_mm, offset_chain, CornerStyle, OffsetOptions, PointMm, Trace, DEFAULT_MITER_LIMIT,
};
use std::collections::BTreeMap;

/// Plus grand nombre de tracés dans une chaîne à décaler.
pub const MAX_OFFSET_PATHS: usize = 1000;

/// Chaîne ordonnée prête à être décalée.
#[derive(Debug, Clone)]
pub struct OrderedChain {
    /// Tracés dans l'ordre de parcours, avec `true` s'ils sont parcourus à l'envers.
    pub edges: Vec<(EntityId, bool)>,
    pub closed: bool,
    /// Géométrie parcourue dans le sens de la chaîne.
    pub traces: Vec<Trace>,
    /// Cercle seul : `ccw` du cercle (le signe de la distance en dépend).
    pub circle_ccw: Option<bool>,
}

fn invalid(msg: String) -> CoreError {
    CoreError::Invalid(msg)
}

pub(crate) fn trace_of(shape: Shape, reversed: bool) -> Trace {
    match (shape, reversed) {
        (Shape::Line(a, b), false) => Trace::Line(a, b),
        (Shape::Line(a, b), true) => Trace::Line(b, a),
        (Shape::Cubic(c), false) => Trace::Cubic(c),
        (Shape::Cubic(c), true) => Trace::Cubic(c.reversed()),
        (Shape::Arc(a), false) => Trace::Arc(a),
        (Shape::Arc(a), true) => Trace::Arc(a.reversed()),
        (Shape::Spline(s), false) => Trace::Spline(s),
        (Shape::Spline(s), true) => Trace::Spline(s.reversed()),
        (Shape::Circle(c), _) => Trace::Circle(c.circle),
    }
}

/// Ordonne `paths` en une chaîne ouverte ou fermée ; erreur expliquée sinon
/// (vide, tracé absent, embranchement, plusieurs chaînes, cercle mélangé).
pub fn ordered_chain(sheet: &Sheet, paths: &[EntityId]) -> Result<OrderedChain, CoreError> {
    let mut ids = paths.to_vec();
    ids.sort();
    ids.dedup();
    if ids.is_empty() {
        return Err(invalid("décalage : aucun tracé à décaler".into()));
    }
    if ids.len() > MAX_OFFSET_PATHS {
        return Err(invalid(format!(
            "décalage : trop de tracés (au plus {MAX_OFFSET_PATHS})"
        )));
    }
    for &id in &ids {
        sheet.entity(id)?;
    }
    if let Some(&c) = ids.iter().find(|&&id| {
        matches!(
            sheet.entity(id).map(|e| &e.kind),
            Ok(EntityKind::Circle { .. })
        )
    }) {
        if ids.len() != 1 {
            return Err(invalid(
                "décalage : un cercle se décale seul, il ne se chaîne pas avec d'autres tracés"
                    .into(),
            ));
        }
        let Shape::Circle(ct) = sheet.shape(c)? else {
            return Err(invalid("décalage : cercle illisible".into()));
        };
        return Ok(OrderedChain {
            edges: vec![(c, false)],
            closed: true,
            traces: vec![Trace::Circle(ct.circle)],
            circle_ccw: Some(ct.ccw),
        });
    }
    // Degrés des points.
    let mut ends: BTreeMap<EntityId, (PointId, PointId)> = BTreeMap::new();
    let mut degree: BTreeMap<PointId, usize> = BTreeMap::new();
    for &id in &ids {
        let (a, b) = sheet.entity_points(id)?;
        if a == b {
            return Err(invalid(format!(
                "décalage : {} se referme sur lui-même (non pris en charge)",
                sheet.entity_name(id)
            )));
        }
        ends.insert(id, (a, b));
        *degree.entry(a).or_default() += 1;
        *degree.entry(b).or_default() += 1;
    }
    if let Some((&p, _)) = degree.iter().find(|(_, d)| **d > 2) {
        return Err(invalid(format!(
            "décalage : embranchement au point {} (plus de deux tracés s'y rejoignent)",
            sheet.label(p)
        )));
    }
    // Parcours à partir du plus petit identifiant, dans son sens enregistré.
    let first = ids[0];
    let (start_pt, mut at) = ends[&first];
    let mut used = vec![first];
    let mut forward = vec![(first, false)];
    let take = |used: &[EntityId], at: PointId| -> Option<EntityId> {
        ids.iter()
            .copied()
            .find(|id| !used.contains(id) && (ends[id].0 == at || ends[id].1 == at))
    };
    while let Some(next) = take(&used, at) {
        let (a, b) = ends[&next];
        let reversed = a != at;
        at = if reversed { a } else { b };
        used.push(next);
        forward.push((next, reversed));
    }
    let closed = at == start_pt && forward.len() >= 2;
    let mut backward: Vec<(EntityId, bool)> = Vec::new();
    if !closed {
        let mut cur = start_pt;
        while let Some(prev) = take(&used, cur) {
            let (a, b) = ends[&prev];
            // Il doit finir en `cur` : retourné si son départ y est.
            let reversed = b != cur;
            cur = if reversed { b } else { a };
            used.push(prev);
            backward.push((prev, reversed));
        }
        backward.reverse();
    }
    if used.len() != ids.len() {
        return Err(invalid(
            "décalage : ces tracés ne forment pas une seule chaîne continue (bout à bout)".into(),
        ));
    }
    let mut edges = backward;
    edges.extend(forward);
    let mut traces = Vec::with_capacity(edges.len());
    for &(id, reversed) in &edges {
        traces.push(trace_of(sheet.shape(id)?, reversed));
    }
    Ok(OrderedChain {
        edges,
        closed,
        traces,
        circle_ccw: None,
    })
}

/// Chaîne de tracés bout à bout qui contient `start`, détectée par les points
/// partagés : on s'arrête aux points où ne se rejoignent pas exactement deux
/// tracés de la feuille (extrémité libre ou embranchement). Un cercle est seul.
pub fn detect_chain(sheet: &Sheet, start: EntityId) -> Result<Vec<EntityId>, CoreError> {
    sheet.entity(start)?;
    if matches!(sheet.entity(start)?.kind, EntityKind::Circle { .. }) {
        return Ok(vec![start]);
    }
    let mut incident: BTreeMap<PointId, Vec<EntityId>> = BTreeMap::new();
    for e in &sheet.entities {
        if matches!(e.kind, EntityKind::Circle { .. }) {
            continue;
        }
        let (a, b) = sheet.entity_points(e.id)?;
        incident.entry(a).or_default().push(e.id);
        if b != a {
            incident.entry(b).or_default().push(e.id);
        }
    }
    let mut chain = vec![start];
    let (a, b) = sheet.entity_points(start)?;
    for origin in [a, b] {
        // Marche depuis une extrémité du tracé de départ, vers l'extérieur.
        let mut at = origin;
        while let Some(list) = incident.get(&at) {
            if list.len() != 2 {
                break;
            }
            let Some(&next) = list.iter().find(|&&id| !chain.contains(&id)) else {
                break;
            };
            chain.push(next);
            let (na, nb) = sheet.entity_points(next)?;
            at = if na == at { nb } else { na };
            if chain.len() > MAX_OFFSET_PATHS {
                break;
            }
        }
    }
    chain.sort();
    Ok(chain)
}

/// Signe à donner à `distance_mm` pour décaler `paths` du côté de `point`
/// (`+1.0` ou `-1.0`).
pub fn offset_side(sheet: &Sheet, paths: &[EntityId], point: PointMm) -> Result<f64, CoreError> {
    Ok(offset_signed_distance(sheet, paths, point)?.signum())
}

/// Valeur de `distance_mm` (signée) qui fait passer le décalage de `paths` par
/// `point` : distance au tracé, du bon côté. Sert à suivre la souris.
pub fn offset_signed_distance(
    sheet: &Sheet,
    paths: &[EntityId],
    point: PointMm,
) -> Result<f64, CoreError> {
    let chain = ordered_chain(sheet, paths)?;
    let signed = pf_geometry::chain_signed_distance(&chain.traces, point)?;
    Ok(match chain.circle_ccw {
        Some(false) => -signed,
        _ => signed,
    })
}

fn options(corner: CornerStyle) -> OffsetOptions {
    OffsetOptions {
        corner,
        miter_limit: DEFAULT_MITER_LIMIT,
    }
}

/// Courbe décalée (aperçu ou création) sans modifier la feuille.
pub fn offset_preview(
    sheet: &Sheet,
    paths: &[EntityId],
    distance_mm: f64,
    corner: CornerStyle,
) -> Result<Vec<Trace>, CoreError> {
    let chain = ordered_chain(sheet, paths)?;
    let d = match chain.circle_ccw {
        Some(false) => -distance_mm,
        _ => distance_mm,
    };
    Ok(offset_chain(&chain.traces, d, &options(corner))?)
}

/// Points créés par une commande de décalage : jamais fusionnés avec des points
/// existants (le résultat n'est pas lié à l'original), seulement entre eux.
struct Joints {
    sheet: SheetId,
    created: Vec<(PointMm, PointId)>,
}

impl Project {
    fn offset_new_point(
        &mut self,
        sheet: SheetId,
        at: PointMm,
        out: &mut Outcome,
    ) -> Result<PointId, CoreError> {
        at.validate()?;
        let id = PointId(self.alloc_id());
        let s = self.sheet_mut(sheet)?;
        let label = s.next_label();
        s.points.push(Point {
            id,
            label,
            definition: PointDefinition::Free { x: at.x, y: at.y },
            role: PointRole::Construction,
            layer: None,
        });
        out.points.push(id);
        Ok(id)
    }

    fn offset_joint(
        &mut self,
        joints: &mut Joints,
        at: PointMm,
        out: &mut Outcome,
    ) -> Result<PointId, CoreError> {
        if let Some((_, id)) = joints
            .created
            .iter()
            .find(|(p, _)| distance_mm(*p, at) <= MERGE_TOLERANCE_MM)
        {
            return Ok(*id);
        }
        let id = self.offset_new_point(joints.sheet, at, out)?;
        joints.created.push((at, id));
        Ok(id)
    }

    /// Crée le décalage de `paths` (voir le module). Une seule commande : tout
    /// ou rien.
    pub(crate) fn offset_paths(
        &mut self,
        sheet: SheetId,
        paths: &[EntityId],
        distance_mm: f64,
        corner: CornerStyle,
        role: PathRole,
        out: &mut Outcome,
    ) -> Result<(), CoreError> {
        let s = self.sheet(sheet)?;
        let chain = ordered_chain(s, paths)?;
        let circle = match chain.circle_ccw {
            Some(ccw) => match s.entity(chain.edges[0].0)?.kind {
                EntityKind::Circle { start_deg, .. } => Some((start_deg, ccw)),
                _ => None,
            },
            None => None,
        };
        let d = if chain.circle_ccw == Some(false) {
            -distance_mm
        } else {
            distance_mm
        };
        let result = offset_chain(&chain.traces, d, &options(corner))?;
        let mut joints = Joints {
            sheet,
            created: Vec::new(),
        };
        let mut made = 0usize;
        for t in result {
            let kind = match t {
                Trace::Line(a, b) => {
                    let (from, to) = (
                        self.offset_joint(&mut joints, a, out)?,
                        self.offset_joint(&mut joints, b, out)?,
                    );
                    if from == to {
                        continue;
                    }
                    EntityKind::Segment {
                        from,
                        to,
                        construction: None,
                    }
                }
                Trace::Arc(a) => {
                    let center = self.offset_new_point(sheet, a.center, out)?;
                    let start = self.offset_joint(&mut joints, a.start_point(), out)?;
                    let end = self.offset_joint(&mut joints, a.end_point(), out)?;
                    if start == end {
                        continue;
                    }
                    EntityKind::Arc {
                        center,
                        start,
                        end,
                        ccw: a.sweep_rad > 0.0,
                    }
                }
                Trace::Cubic(c) => {
                    let start = self.offset_joint(&mut joints, c.p0, out)?;
                    let end = self.offset_joint(&mut joints, c.p3, out)?;
                    if start == end {
                        continue;
                    }
                    let s = self.sheet(sheet)?;
                    let (ps, pe) = (s.position(start)?, s.position(end)?);
                    EntityKind::Curve {
                        start,
                        handle_start: OffsetMm::from_vector(c.c1 - ps),
                        handle_end: OffsetMm::from_vector(c.c2 - pe),
                        end,
                    }
                }
                Trace::Circle(c) => {
                    let center = self.offset_new_point(sheet, c.center, out)?;
                    let (start_deg, ccw) = circle.unwrap_or((0.0, true));
                    EntityKind::Circle {
                        center,
                        radius_mm: c.radius_mm,
                        start_deg,
                        ccw,
                    }
                }
                Trace::Spline(_) => {
                    return Err(invalid("décalage : résultat inattendu".into()));
                }
            };
            let id = EntityId(self.alloc_id());
            self.sheet_mut(sheet)?.entities.push(Entity {
                id,
                role,
                layer: None,
                name: None,
                kind,
            });
            out.entities.push(id);
            made += 1;
        }
        if made == 0 {
            return Err(invalid(
                "décalage : le résultat est vide (tracés réduits à un point)".into(),
            ));
        }
        out.sheet = Some(sheet);
        Ok(())
    }
}
