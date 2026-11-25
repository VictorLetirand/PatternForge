//! Contours fermés : chaînes de segments et de courbes reliées par leurs
//! points, sans embranchement. La fermeture est **topologique** (points
//! partagés), pas une coïncidence de coordonnées.
//!
//! Périmètre et aire sont des mesures observées, calculées à la demande.

use crate::error::CoreError;
use crate::model::{EntityId, EntityKind, PathRole, PointId, Sheet};
use pf_geometry::{
    distance_mm, Arc, CircleTrace, CubicBezier, PointMm, DEFAULT_LENGTH_TOLERANCE_MM,
};
use std::collections::BTreeMap;

/// Un tracé du contour, avec son sens de parcours.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContourEdge {
    pub entity: EntityId,
    /// Vrai si le tracé est parcouru de son arrivée vers son départ.
    pub reversed: bool,
}

/// Chaîne fermée simple : chaque point y relie exactement deux tracés.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Contour {
    pub edges: Vec<ContourEdge>,
    /// Points dans l'ordre de parcours (un par tracé).
    pub points: Vec<PointId>,
    /// Rôle commun à tous les tracés, s'ils ont tous le même.
    pub role: Option<PathRole>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ContourMeasure {
    pub perimeter_mm: f64,
    /// Aire en mm², toujours positive.
    pub area_mm2: f64,
    /// Sens de parcours des tracés tels qu'ils sont enregistrés.
    pub clockwise: bool,
    /// Le contour se recoupe lui-même : l'aire n'a alors pas de sens métier.
    pub self_intersecting: bool,
}

fn ends(sheet: &Sheet, id: EntityId) -> Option<(PointId, PointId)> {
    sheet.entity_points(id).ok()
}

fn is_circle(sheet: &Sheet, id: EntityId) -> bool {
    sheet
        .entity(id)
        .is_ok_and(|e| matches!(e.kind, EntityKind::Circle { .. }))
}

/// Un cercle entier est un contour fermé à lui seul (un seul tracé).
fn circle_contour(sheet: &Sheet, id: EntityId) -> Result<Contour, CoreError> {
    let center = match sheet.entity(id)?.kind {
        EntityKind::Circle { center, .. } => center,
        _ => return Err(CoreError::Invalid("ce tracé n'est pas un cercle".into())),
    };
    Ok(Contour {
        edges: vec![ContourEdge {
            entity: id,
            reversed: false,
        }],
        points: vec![center],
        role: sheet.entity(id).ok().map(|e| e.role),
    })
}

/// Tous les contours fermés simples de la feuille, dans un ordre stable
/// (par plus petit identifiant de tracé).
pub fn closed_contours(sheet: &Sheet) -> Vec<Contour> {
    let ids: Vec<EntityId> = sheet
        .entities
        .iter()
        .filter(|e| !is_circle(sheet, e.id))
        .map(|e| e.id)
        .collect();
    let mut out: Vec<Contour> = components(sheet, &ids)
        .into_iter()
        .filter_map(|component| chain(sheet, &component).ok())
        .collect();
    out.extend(
        sheet
            .entities
            .iter()
            .filter(|e| is_circle(sheet, e.id))
            .filter_map(|e| circle_contour(sheet, e.id).ok()),
    );
    out.sort_by_key(|c| c.edges.iter().map(|e| e.entity).min());
    out
}

/// Contour formé exactement par ces tracés ; erreur expliquée sinon.
pub fn contour_from_entities(sheet: &Sheet, entities: &[EntityId]) -> Result<Contour, CoreError> {
    let mut ids = entities.to_vec();
    ids.sort();
    ids.dedup();
    for &id in &ids {
        sheet.entity(id)?;
    }
    if ids.iter().any(|&id| is_circle(sheet, id)) {
        if ids.len() == 1 {
            return circle_contour(sheet, ids[0]);
        }
        return Err(CoreError::Invalid(
            "un cercle forme un contour à lui seul : il ne se chaîne pas avec d'autres tracés"
                .into(),
        ));
    }
    if components(sheet, &ids).len() != 1 {
        return Err(CoreError::Invalid(
            "ces tracés ne forment pas une seule chaîne continue".into(),
        ));
    }
    chain(sheet, &ids)
}

/// Regroupe les tracés reliés entre eux par des points communs.
fn components(sheet: &Sheet, ids: &[EntityId]) -> Vec<Vec<EntityId>> {
    let mut by_point: BTreeMap<PointId, Vec<EntityId>> = BTreeMap::new();
    for &id in ids {
        if let Some((a, b)) = ends(sheet, id) {
            by_point.entry(a).or_default().push(id);
            by_point.entry(b).or_default().push(id);
        }
    }
    let mut seen: Vec<EntityId> = Vec::new();
    let mut out = Vec::new();
    let mut sorted = ids.to_vec();
    sorted.sort();
    for &start in &sorted {
        if seen.contains(&start) {
            continue;
        }
        let mut component = Vec::new();
        let mut stack = vec![start];
        while let Some(id) = stack.pop() {
            if seen.contains(&id) {
                continue;
            }
            seen.push(id);
            component.push(id);
            if let Some((a, b)) = ends(sheet, id) {
                for p in [a, b] {
                    stack.extend(by_point.get(&p).into_iter().flatten().copied());
                }
            }
        }
        component.sort();
        out.push(component);
    }
    out
}

/// Ordonne un groupe de tracés en chaîne fermée simple.
fn chain(sheet: &Sheet, ids: &[EntityId]) -> Result<Contour, CoreError> {
    let mut degree: BTreeMap<PointId, usize> = BTreeMap::new();
    for &id in ids {
        let (a, b) = sheet.entity_points(id)?;
        *degree.entry(a).or_default() += 1;
        *degree.entry(b).or_default() += 1;
    }
    if let Some((&p, _)) = degree.iter().find(|(_, d)| **d == 1) {
        return Err(CoreError::Invalid(format!(
            "contour ouvert : il s'arrête au point {}",
            sheet.label(p)
        )));
    }
    if let Some((&p, _)) = degree.iter().find(|(_, d)| **d > 2) {
        return Err(CoreError::Invalid(format!(
            "embranchement au point {} : plus de deux tracés s'y rejoignent",
            sheet.label(p)
        )));
    }
    let Some(&first) = ids.first() else {
        return Err(CoreError::Invalid("aucun tracé".into()));
    };
    let (start, mut at) = sheet.entity_points(first)?;
    let mut edges = vec![ContourEdge {
        entity: first,
        reversed: false,
    }];
    let mut points = vec![start];
    let mut used = vec![first];
    while at != start || used.len() < ids.len() {
        let next = ids.iter().copied().find(|id| {
            !used.contains(id) && ends(sheet, *id).is_some_and(|(a, b)| a == at || b == at)
        });
        let Some(next) = next else {
            return Err(CoreError::Invalid(
                "ces tracés ne forment pas un contour fermé".into(),
            ));
        };
        let (a, b) = sheet.entity_points(next)?;
        let reversed = a != at;
        points.push(at);
        edges.push(ContourEdge {
            entity: next,
            reversed,
        });
        used.push(next);
        at = if reversed { a } else { b };
    }
    let mut roles = edges
        .iter()
        .filter_map(|e| sheet.entity(e.entity).ok().map(|x| x.role));
    let first_role = roles.next();
    let role = first_role.filter(|r| roles.all(|x| x == *r));
    Ok(Contour {
        edges,
        points,
        role,
    })
}

/// Géométrie orientée d'un tracé du contour (une courbe par points de passage
/// y compte pour un tronçon par intervalle).
enum Leg {
    Line(PointMm, PointMm),
    Curve(CubicBezier),
    Arc(Arc),
    Circle(CircleTrace),
}

fn legs(sheet: &Sheet, edge: ContourEdge) -> Result<Vec<Leg>, CoreError> {
    if let EntityKind::Spline { .. } = sheet.entity(edge.entity)?.kind {
        let sp = sheet.spline_geometry(edge.entity)?;
        let sp = if edge.reversed { sp.reversed() } else { sp };
        return Ok(sp.segments().iter().map(|c| Leg::Curve(*c)).collect());
    }
    Ok(vec![leg(sheet, edge)?])
}

fn leg(sheet: &Sheet, edge: ContourEdge) -> Result<Leg, CoreError> {
    Ok(match sheet.entity(edge.entity)?.kind {
        EntityKind::Segment { .. } => {
            let (a, b) = sheet.segment_ends(edge.entity)?;
            if edge.reversed {
                Leg::Line(b, a)
            } else {
                Leg::Line(a, b)
            }
        }
        EntityKind::Curve { .. } => {
            let c = sheet.curve_geometry(edge.entity)?;
            Leg::Curve(if edge.reversed { c.reversed() } else { c })
        }
        EntityKind::Arc { .. } => {
            let a = sheet.arc_geometry(edge.entity)?;
            Leg::Arc(if edge.reversed { a.reversed() } else { a })
        }
        EntityKind::Circle { .. } => Leg::Circle(sheet.circle_geometry(edge.entity)?),
        EntityKind::Spline { .. } => unreachable!("traité par `legs`"),
    })
}

/// Polyligne d'approximation du contour parcouru dans l'ordre des arêtes
/// (`samples` pas par courbe, arc ou cercle). Les points consécutifs confondus
/// sont fusionnés ; si la chaîne revient à son départ, le dernier point (doublon
/// du premier) est retiré. Lecture seule, pour l'affichage et les boîtes englobantes.
pub fn contour_polyline(
    sheet: &Sheet,
    edges: &[ContourEdge],
    samples: usize,
) -> Result<Vec<PointMm>, CoreError> {
    let mut out: Vec<PointMm> = Vec::new();
    let mut push = |p: PointMm| {
        if out.last().is_none_or(|l| distance_mm(*l, p) > 1e-12) {
            out.push(p);
        }
    };
    for edge in edges {
        for l in legs(sheet, *edge)? {
            match l {
                Leg::Line(a, b) => {
                    push(a);
                    push(b);
                }
                Leg::Curve(c) => c.sample(samples).into_iter().for_each(&mut push),
                Leg::Arc(a) => a.sample(samples).into_iter().for_each(&mut push),
                Leg::Circle(c) => c.sample(samples).into_iter().for_each(&mut push),
            }
        }
    }
    if out.len() > 2 && distance_mm(out[0], out[out.len() - 1]) <= 1e-9 {
        out.pop();
    }
    Ok(out)
}

/// Périmètre, aire exacte (courbes comprises) et contrôle d'auto-intersection.
pub fn measure_contour(sheet: &Sheet, contour: &Contour) -> Result<ContourMeasure, CoreError> {
    let legs: Vec<Leg> = contour
        .edges
        .iter()
        .map(|e| legs(sheet, *e))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .flatten()
        .collect();
    let origin = match legs.first() {
        Some(Leg::Line(a, _)) => *a,
        Some(Leg::Curve(c)) => c.p0,
        Some(Leg::Arc(a)) => a.start_point(),
        Some(Leg::Circle(c)) => c.circle.center,
        None => return Err(CoreError::Invalid("contour vide".into())),
    };
    let (mut perimeter, mut signed) = (0.0, 0.0);
    // Polyligne d'approximation, par tracé, pour le test d'auto-intersection.
    let mut polylines: Vec<Vec<PointMm>> = Vec::new();
    for l in &legs {
        match l {
            Leg::Line(a, b) => {
                perimeter += distance_mm(*a, *b);
                signed += 0.5 * (*a - origin).cross(*b - origin);
                polylines.push(vec![*a, *b]);
            }
            Leg::Curve(c) => {
                perimeter += c.length_mm(DEFAULT_LENGTH_TOLERANCE_MM)?;
                signed += c.signed_area_contribution_mm2(origin);
                polylines.push(c.sample(48));
            }
            Leg::Arc(a) => {
                perimeter += a.length_mm();
                signed += a.signed_area_contribution_mm2(origin);
                polylines.push(a.sample(48));
            }
            Leg::Circle(c) => {
                perimeter += c.length_mm();
                signed += c.signed_area_mm2();
            }
        }
    }
    Ok(ContourMeasure {
        perimeter_mm: perimeter,
        area_mm2: signed.abs(),
        clockwise: signed < 0.0,
        self_intersecting: self_intersects(&polylines),
    })
}

/// Deux morceaux du contour se croisent-ils ailleurs qu'à leur jonction ?
fn self_intersects(polylines: &[Vec<PointMm>]) -> bool {
    let n = polylines.len();
    let crosses = |a: PointMm, b: PointMm, c: PointMm, d: PointMm| {
        let (u, v) = (b - a, d - c);
        let denom = u.cross(v);
        if denom.abs() < 1e-12 {
            return false;
        }
        let t = (c - a).cross(v) / denom;
        let s = (c - a).cross(u) / denom;
        let eps = 1e-9;
        t > eps && t < 1.0 - eps && s > eps && s < 1.0 - eps
    };
    // Boîtes englobantes : deux morceaux dont les boîtes ne se touchent pas ne se
    // croisent pas (un contour décalé compte des dizaines de morceaux).
    let boxes: Vec<(f64, f64, f64, f64)> = polylines
        .iter()
        .map(|pl| {
            pl.iter().fold(
                (f64::MAX, f64::MAX, f64::MIN, f64::MIN),
                |(x0, y0, x1, y1), q| (x0.min(q.x), y0.min(q.y), x1.max(q.x), y1.max(q.y)),
            )
        })
        .collect();
    let apart = |a: usize, b: usize| {
        let (p, q) = (boxes[a], boxes[b]);
        p.0 > q.2 + 1e-6 || q.0 > p.2 + 1e-6 || p.1 > q.3 + 1e-6 || q.1 > p.3 + 1e-6
    };
    for i in 0..n {
        for j in i..n {
            if i != j && apart(i, j) {
                continue;
            }
            for (ia, wa) in polylines[i].windows(2).enumerate() {
                for (ib, wb) in polylines[j].windows(2).enumerate() {
                    if i == j && ib <= ia + 1 {
                        continue;
                    }
                    if crosses(wa[0], wa[1], wb[0], wb[1]) {
                        return true;
                    }
                }
            }
        }
    }
    false
}
