//! US-02.7 — modifier un tracé : prolonger / raccourcir (d'une valeur ou
//! jusqu'à un autre tracé), couper en un point, fusionner deux tracés bout à
//! bout, inverser le sens. Chaque opération est une seule commande atomique
//! (donc une seule étape annulable).
//!
//! Choix de modélisation (voir `docs/DECISIONS.md`, décisions 63 à 67) :
//! - le tracé modifié garde son identifiant (calque, groupe, nom, rôle, crans) ;
//!   à la coupe, le second tronçon reçoit les mêmes calque, groupe et rôle, et le
//!   nom « X (2) » ; à la fusion, le premier tracé survit ;
//! - les mécanismes du congé sont réutilisés (`trim_path`, `freeze_dependents`,
//!   `tangency_point`, `drop_unused_point`) ;
//! - rien n'est perdu en silence : cran supprimé, point construit figé, nom ou
//!   liaison perdus, contour de pièce à vérifier, tout est dans `Outcome.notes`.

use super::*;
use crate::input::format_mm;
use pf_geometry::{distance_mm, Circle, CubicBezier, Trace, Trim, TrimEnd, FILLET_MIN_KEPT_MM};
use std::f64::consts::TAU;

/// Écart maximal (mm) entre le point de coupe visé et le tracé.
pub const SPLIT_TOLERANCE_MM: f64 = 1e-6;
/// Écart maximal (mm) à l'alignement ou au cercle commun pour fusionner.
const MERGE_FIT_TOLERANCE_MM: f64 = 1e-7;

/// Précisions d'une coupe pilotée par le code (extraction de pièce, US-04.1) :
/// `t` est le paramètre exact de la coupe sur une courbe de Bézier (la projection
/// du point visé, imprécise sur une longue courbe, est alors évitée) et `point` le
/// point d'extrémité à réutiliser au lieu d'en créer un (les tronçons de plusieurs
/// tracés qui se croisent partagent ainsi exactement le même point).
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct SplitHint {
    pub t: Option<f64>,
    pub point: Option<PointId>,
}

fn neg(o: OffsetMm) -> OffsetMm {
    OffsetMm {
        dx: -o.dx,
        dy: -o.dy,
    }
}

fn flip_end(e: SegmentEnd) -> SegmentEnd {
    match e {
        SegmentEnd::Start => SegmentEnd::End,
        SegmentEnd::End => SegmentEnd::Start,
    }
}

fn kind_label(k: &EntityKind) -> &'static str {
    match k {
        EntityKind::Segment { .. } => "segment",
        EntityKind::Curve { .. } => "courbe de Bézier",
        EntityKind::Circle { .. } => "cercle",
        EntityKind::Arc { .. } => "arc",
        EntityKind::Spline { .. } => "courbe lisse",
    }
}

/// Points de passage parcourus à l'envers (tangentes opposées, entrante et
/// sortante échangées aux points anguleux).
fn reversed_nodes(nodes: &[SplineNode]) -> Vec<SplineNode> {
    nodes
        .iter()
        .rev()
        .map(|n| {
            if n.corner {
                SplineNode {
                    point: n.point,
                    corner: true,
                    tangent: n.tangent_in.map(neg),
                    tangent_in: n.tangent.map(neg),
                }
            } else {
                SplineNode {
                    point: n.point,
                    corner: false,
                    tangent: n.tangent.map(neg),
                    tangent_in: None,
                }
            }
        })
        .collect()
}

/// Même tracé parcouru dans l'autre sens (géométrie inchangée).
fn reversed_kind(kind: &EntityKind) -> EntityKind {
    match kind {
        EntityKind::Segment { from, to, .. } => EntityKind::Segment {
            from: *to,
            to: *from,
            construction: None,
        },
        EntityKind::Curve {
            start,
            handle_start,
            handle_end,
            end,
        } => EntityKind::Curve {
            start: *end,
            handle_start: *handle_end,
            handle_end: *handle_start,
            end: *start,
        },
        EntityKind::Circle {
            center,
            radius_mm,
            start_deg,
            ccw,
        } => EntityKind::Circle {
            center: *center,
            radius_mm: *radius_mm,
            start_deg: *start_deg,
            ccw: !*ccw,
        },
        EntityKind::Arc {
            center,
            start,
            end,
            ccw,
        } => EntityKind::Arc {
            center: *center,
            start: *end,
            end: *start,
            ccw: !*ccw,
        },
        EntityKind::Spline { nodes } => EntityKind::Spline {
            nodes: reversed_nodes(nodes),
        },
    }
}

/// Un point construit sur le tracé inversé reste au même endroit.
fn reversed_definition(def: &mut PointDefinition, entity: EntityId) {
    match def {
        PointDefinition::OnSegment {
            segment, origin, ..
        } if *segment == entity => *origin = flip_end(*origin),
        PointDefinition::OnSegmentRatio { segment, ratio } if *segment == entity => {
            *ratio = 1.0 - *ratio
        }
        PointDefinition::OnCurve { curve, t } if *curve == entity => *t = 1.0 - *t,
        PointDefinition::AlongCurve { curve, origin, .. } if *curve == entity => {
            *origin = flip_end(*origin)
        }
        PointDefinition::Division { trace, ratio, .. } if *trace == entity => *ratio = 1.0 - *ratio,
        PointDefinition::Polar {
            reference: Some(r),
            angle_deg,
            ..
        } if *r == entity => *angle_deg = (*angle_deg + 180.0).rem_euclid(360.0),
        _ => {}
    }
}

/// Points de passage résolus parcourus à l'envers.
fn reversed_resolved(nodes: Vec<ResolvedSplineNode>) -> Vec<ResolvedSplineNode> {
    nodes
        .into_iter()
        .rev()
        .map(|n| ResolvedSplineNode {
            tangent_in: n.tangent_out * -1.0,
            tangent_out: n.tangent_in * -1.0,
            tangent_in_explicit: n.tangent_out_explicit,
            tangent_out_explicit: n.tangent_in_explicit,
            ..n
        })
        .collect()
}

/// Tracé obtenu en fusionnant `first` et `second`, qui partagent le point `p`.
/// `case_a` : `p` est l'arrivée de `first` (sinon son départ) ; `flip2` : il
/// faut inverser `second` pour qu'il suive (cas A) ou précède (cas B) `first`.
fn merged_kind(
    s: &Sheet,
    first: EntityId,
    second: EntityId,
    p: PointId,
    case_a: bool,
    flip2: bool,
) -> Result<EntityKind, CoreError> {
    let bad = |msg: String| {
        CoreError::Invalid(format!(
            "fusion impossible entre {} et {} : {msg}",
            s.entity_name(first),
            s.entity_name(second)
        ))
    };
    let (a1, b1) = s.entity_points(first)?;
    let (a2, b2) = s.entity_points(second)?;
    let far2 = if p == a2 { b2 } else { a2 };
    // Extrémités du tracé fusionné, dans le sens de `first`.
    let (a, b) = if case_a { (a1, far2) } else { (far2, b1) };
    let pp = s.position(p)?;
    match (&s.entity(first)?.kind, &s.entity(second)?.kind) {
        (EntityKind::Segment { .. }, EntityKind::Segment { .. }) => {
            let (pa, pb) = (s.position(a)?, s.position(b)?);
            let u = (pb - pa)
                .unit()
                .map_err(|_| bad("les extrémités du tracé obtenu sont confondues".into()))?;
            let off = u.cross(pp - pa).abs();
            if off > MERGE_FIT_TOLERANCE_MM {
                return Err(bad(format!(
                    "les segments ne sont pas colinéaires (écart {} mm)",
                    format_mm(off)
                )));
            }
            if (pp - pa).dot(u) <= EPSILON_MM || (pb - pp).dot(u) <= EPSILON_MM {
                return Err(bad("les segments se replient l'un sur l'autre".into()));
            }
            Ok(EntityKind::Segment {
                from: a,
                to: b,
                construction: None,
            })
        }
        (EntityKind::Arc { center, .. }, EntityKind::Arc { .. }) => {
            let (g1, g2) = (s.arc_geometry(first)?, s.arc_geometry(second)?);
            if distance_mm(g1.center, g2.center) > MERGE_FIT_TOLERANCE_MM
                || (g1.radius_mm - g2.radius_mm).abs() > MERGE_FIT_TOLERANCE_MM
            {
                return Err(bad("les arcs ne sont pas sur le même cercle".into()));
            }
            if g1.is_ccw() != (g2.is_ccw() != flip2) {
                return Err(bad("les arcs ne tournent pas dans le même sens".into()));
            }
            if g1.length_mm() + g2.length_mm() > TAU * g1.radius_mm - FILLET_MIN_KEPT_MM {
                return Err(bad(
                    "l'arc obtenu ferait un tour complet (utilisez un cercle)".into(),
                ));
            }
            Ok(EntityKind::Arc {
                center: *center,
                start: a,
                end: b,
                ccw: g1.is_ccw(),
            })
        }
        (EntityKind::Curve { .. }, EntityKind::Curve { .. }) => {
            let (g1, g2) = (s.curve_geometry(first)?, s.curve_geometry(second)?);
            let g2 = if flip2 { g2.reversed() } else { g2 };
            // c1 finit en `p`, c2 y commence.
            let (c1, c2) = if case_a { (g1, g2) } else { (g2, g1) };
            let (u, v) = (pp - c1.c2, c2.c1 - pp);
            let (la, lb) = (u.norm(), v.norm());
            if la <= EPSILON_MM
                || lb <= EPSILON_MM
                || u.cross(v).abs() > 1e-9 * la * lb
                || u.dot(v) <= 0.0
            {
                return Err(bad(
                    "les courbes ne se raccordent pas en ligne droite ; seules les courbes \
                     issues d'une coupe d'une même courbe de Bézier se fusionnent"
                        .into(),
                ));
            }
            let t = la / (la + lb);
            let k1 = c1.p0 + (c1.c1 - c1.p0) * (1.0 / t);
            let k2 = c2.p3 + (c2.c2 - c2.p3) * (1.0 / (1.0 - t));
            let (q0, q1, q2) = (c1.p0.lerp(k1, t), k1.lerp(k2, t), k2.lerp(c2.p3, t));
            let (r0, r1) = (q0.lerp(q1, t), q1.lerp(q2, t));
            let x = r0.lerp(r1, t);
            if distance_mm(x, pp) > SPLIT_TOLERANCE_MM
                || distance_mm(r0, c1.c2) > SPLIT_TOLERANCE_MM
                || distance_mm(r1, c2.c1) > SPLIT_TOLERANCE_MM
            {
                return Err(bad(
                    "les deux courbes ne prolongent pas une même courbe de Bézier".into(),
                ));
            }
            let whole = CubicBezier::new(c1.p0, k1, k2, c2.p3);
            whole.validate().map_err(|e| bad(e.to_string()))?;
            let (start, end) = (a, b);
            Ok(EntityKind::Curve {
                start,
                handle_start: OffsetMm::from_vector(k1 - c1.p0),
                handle_end: OffsetMm::from_vector(k2 - c2.p3),
                end,
            })
        }
        (EntityKind::Spline { nodes: n1 }, EntityKind::Spline { nodes: n2 }) => {
            let (r1, r2) = (s.spline_nodes(first)?, s.spline_nodes(second)?);
            let oriented = |nodes: &[SplineNode], res: Vec<ResolvedSplineNode>, rev: bool| {
                if rev {
                    (reversed_nodes(nodes), reversed_resolved(res))
                } else {
                    (nodes.to_vec(), res)
                }
            };
            let ((na, ra), (nb, rb)) = if case_a {
                (oriented(n1, r1, false), oriented(n2, r2, flip2))
            } else {
                (oriented(n2, r2, flip2), oriented(n1, r1, false))
            };
            let (Some(last), Some(head)) = (ra.last(), rb.first()) else {
                return Err(bad("courbe sans point de passage".into()));
            };
            let (tin, tout) = (last.tangent_in, head.tangent_out);
            let same = (tin.x - tout.x).hypot(tin.y - tout.y) <= EPSILON_MM;
            let junction = SplineNode {
                point: p,
                corner: !same,
                tangent: Some(OffsetMm::from_vector(tout)),
                tangent_in: if same {
                    None
                } else {
                    Some(OffsetMm::from_vector(tin))
                },
            };
            let mut nodes: Vec<SplineNode> = na[..na.len() - 1].to_vec();
            nodes.push(junction);
            nodes.extend_from_slice(&nb[1..]);
            Ok(EntityKind::Spline { nodes })
        }
        (k1, k2) => Err(bad(format!(
            "types incompatibles ({} et {})",
            kind_label(k1),
            kind_label(k2)
        ))),
    }
}

/// Plan de coupe, calculé sans toucher au modèle.
enum CutPlan {
    Segment {
        from: PointId,
        to: PointId,
        at: PointMm,
    },
    Arc {
        center: PointId,
        start: PointId,
        end: PointId,
        ccw: bool,
        at: PointMm,
    },
    Curve {
        start: PointId,
        end: PointId,
        at: PointMm,
        /// (poignée de départ, poignée d'arrivée) des deux tronçons.
        first: (OffsetMm, OffsetMm),
        second: (OffsetMm, OffsetMm),
    },
    Spline {
        first: Vec<SplineNode>,
        second: Vec<SplineNode>,
    },
}

impl Project {
    /// Prolonge (`delta_mm` > 0) ou raccourcit (`delta_mm` < 0) un segment ou
    /// un arc par l'extrémité `end`.
    pub(super) fn extend_path(
        &mut self,
        sheet: SheetId,
        entity: EntityId,
        end: SegmentEnd,
        delta_mm: f64,
        out: &mut Outcome,
    ) -> Result<(), CoreError> {
        if !delta_mm.is_finite() {
            return Err(pf_geometry::GeometryError::InvalidLength(delta_mm).into());
        }
        if delta_mm.abs() <= EPSILON_MM {
            return Err(CoreError::Invalid(
                "la valeur de prolongement ne peut pas être nulle".into(),
            ));
        }
        let s = self.sheet(sheet)?;
        let name = s.entity_name(entity);
        let too_long = |len: f64| {
            CoreError::Invalid(format!(
                "{name} : raccourcir de {} mm dépasse la longueur du tracé ({} mm)",
                format_mm(-delta_mm),
                format_mm(len)
            ))
        };
        let at = match s.shape(entity)? {
            Shape::Line(a, b) => {
                let len = distance_mm(a, b);
                if delta_mm < 0.0 && -delta_mm > len - FILLET_MIN_KEPT_MM {
                    return Err(too_long(len));
                }
                let u = (b - a).unit()?;
                match end {
                    SegmentEnd::End => b + u * delta_mm,
                    SegmentEnd::Start => a + u * -delta_mm,
                }
            }
            Shape::Arc(arc) => {
                let len = arc.length_mm();
                if delta_mm < 0.0 && -delta_mm > len - FILLET_MIN_KEPT_MM {
                    return Err(too_long(len));
                }
                let r = arc.radius_mm;
                if len + delta_mm > TAU * r - FILLET_MIN_KEPT_MM {
                    return Err(CoreError::Invalid(format!(
                        "{name} : l'arc obtenu ferait un tour complet (utilisez un cercle)"
                    )));
                }
                let sign = arc.sweep_rad.signum();
                let angle = match end {
                    SegmentEnd::End => arc.start_rad + sign * (len + delta_mm) / r,
                    SegmentEnd::Start => arc.start_rad - sign * delta_mm / r,
                };
                PointMm::new(
                    arc.center.x + r * angle.cos(),
                    arc.center.y + r * angle.sin(),
                )
            }
            _ => {
                return Err(CoreError::Invalid(format!(
                    "{name} : seuls les segments et les arcs se prolongent ou se raccourcissent \
                     (une courbe de Bézier ou lisse, un cercle : refusé, la forme ne se prolonge \
                     pas sans la déformer)"
                )))
            }
        };
        at.validate()?;
        self.move_path_end(sheet, entity, end, at, delta_mm, out)
    }

    /// Déplace l'extrémité `end` d'un segment ou d'un arc au point `at` (la
    /// longueur change de `delta_mm`) : points construits figés, cotations du
    /// tracé suivies, crans recalés, ancienne extrémité supprimée si inutile.
    fn move_path_end(
        &mut self,
        sheet: SheetId,
        entity: EntityId,
        end: SegmentEnd,
        at: PointMm,
        delta_mm: f64,
        out: &mut Outcome,
    ) -> Result<(), CoreError> {
        let s = self.sheet(sheet)?;
        let e = s.entity(entity)?;
        let layer = e.layer;
        let (first_pt, last_pt) = match e.kind {
            EntityKind::Segment { from, to, .. } => (from, to),
            EntityKind::Arc { start, end, .. } => (start, end),
            _ => {
                return Err(CoreError::Invalid(
                    "seuls les segments et les arcs se prolongent ou se raccourcissent".into(),
                ))
            }
        };
        let (old, other) = match end {
            SegmentEnd::Start => (first_pt, last_pt),
            SegmentEnd::End => (last_pt, first_pt),
        };
        let name = s.entity_name(entity);
        let neighbours: Vec<String> = s
            .entities
            .iter()
            .filter(|x| x.id != entity && x.kind.all_points().contains(&old))
            .map(|x| s.entity_name(x.id))
            .collect();
        self.freeze_dependents(sheet, &[entity], out)?;
        self.release_notch_anchors(sheet, &[entity], out)?;
        let new = self.tangency_point(sheet, at, layer, out)?;
        if new == other {
            return Err(CoreError::Invalid(format!(
                "{name} : le tracé obtenu serait réduit à un point"
            )));
        }
        // Une cotation qui mesure ce tracé d'un bout à l'autre suit l'extrémité.
        let s = self.sheet_mut(sheet)?;
        for d in s.dimensions.iter_mut() {
            if d.from == old && d.to == other {
                d.from = new;
            } else if d.to == old && d.from == other {
                d.to = new;
            }
        }
        if delta_mm < 0.0 {
            let trim = Trim {
                end: match end {
                    SegmentEnd::Start => TrimEnd::Start,
                    SegmentEnd::End => TrimEnd::End,
                },
                removed_mm: -delta_mm,
            };
            self.trim_path(sheet, entity, trim, at, out)?;
        } else {
            let s = self.sheet_mut(sheet)?;
            match &mut s.entity_mut(entity)?.kind {
                EntityKind::Segment {
                    from,
                    to,
                    construction,
                } => {
                    match end {
                        SegmentEnd::Start => *from = new,
                        SegmentEnd::End => *to = new,
                    }
                    *construction = None;
                }
                EntityKind::Arc { start, end: e, .. } => match end {
                    SegmentEnd::Start => *start = new,
                    SegmentEnd::End => *e = new,
                },
                _ => {}
            }
            if end == SegmentEnd::Start {
                for n in s.notches.iter_mut().filter(|n| n.entity == entity) {
                    n.distance_mm += delta_mm;
                }
            }
            self.drop_unused_point(sheet, old)?;
        }
        if !neighbours.is_empty() {
            out.notes.push(format!(
                "{name} n'est plus relié à {} : son extrémité a été déplacée.",
                neighbours.join(", ")
            ));
        }
        Ok(())
    }

    /// Prolonge ou raccourcit un segment ou un arc jusqu'à son intersection
    /// avec `target` (le support du tracé est prolongé : droite ou cercle ;
    /// `target` reste borné). Parmi plusieurs solutions, la plus proche de `pick`.
    pub(super) fn trim_path_to(
        &mut self,
        sheet: SheetId,
        entity: EntityId,
        end: SegmentEnd,
        target: EntityId,
        pick: PointMm,
        out: &mut Outcome,
    ) -> Result<(), CoreError> {
        pick.validate()?;
        if entity == target {
            return Err(CoreError::Invalid(
                "choisissez deux tracés différents".into(),
            ));
        }
        let s = self.sheet(sheet)?;
        let (name, tname) = (s.entity_name(entity), s.entity_name(target));
        let wrap =
            |e: pf_geometry::GeometryError| CoreError::Invalid(format!("{name} et {tname} : {e}"));
        let tshape = s.shape(target)?;
        // (point d'intersection, variation de longueur du tracé), longueur actuelle,
        // longueur maximale admise.
        let (candidates, len, max_len): (Vec<(PointMm, f64)>, f64, f64) = match s.shape(entity)? {
            Shape::Line(a, b) => {
                let len = distance_mm(a, b);
                let u = (b - a).unit()?;
                let bb = tshape.bbox();
                let reach = [
                    bb.min,
                    bb.max,
                    PointMm::new(bb.min.x, bb.max.y),
                    PointMm::new(bb.max.x, bb.min.y),
                ]
                .iter()
                .map(|c| distance_mm(*c, a))
                .fold(0.0, f64::max)
                    + len
                    + 1.0;
                let line = Trace::Line(a + u * -reach, b + u * reach);
                let hits = pf_geometry::intersect(&line, &tshape.to_trace()).map_err(wrap)?;
                let c = hits
                    .iter()
                    .map(|h| {
                        let sc = (h.point - a).dot(u);
                        let delta = match end {
                            SegmentEnd::End => sc - len,
                            SegmentEnd::Start => -sc,
                        };
                        (h.point, delta)
                    })
                    .collect();
                (c, len, f64::INFINITY)
            }
            Shape::Arc(arc) => {
                let circle = Circle::new(arc.center, arc.radius_mm)?;
                let hits = pf_geometry::intersect(&Trace::Circle(circle), &tshape.to_trace())
                    .map_err(wrap)?;
                let sign = arc.sweep_rad.signum();
                let len = arc.length_mm();
                let c = hits
                    .iter()
                    .map(|h| {
                        let v = h.point - arc.center;
                        let angle = v.y.atan2(v.x);
                        let swept = match end {
                            SegmentEnd::End => ((angle - arc.start_rad) * sign).rem_euclid(TAU),
                            SegmentEnd::Start => {
                                ((arc.start_rad + arc.sweep_rad - angle) * sign).rem_euclid(TAU)
                            }
                        };
                        (h.point, swept * arc.radius_mm - len)
                    })
                    .collect();
                (c, len, TAU * arc.radius_mm - FILLET_MIN_KEPT_MM)
            }
            _ => {
                return Err(CoreError::Invalid(format!(
                    "{name} : seuls les segments et les arcs se prolongent ou se raccourcissent \
                     jusqu'à un autre tracé"
                )))
            }
        };
        if candidates.is_empty() {
            return Err(CoreError::Invalid(format!(
                "{name} et {tname} ne se coupent pas, même en prolongeant {name}"
            )));
        }
        let usable: Vec<&(PointMm, f64)> = candidates
            .iter()
            .filter(|(_, d)| {
                d.abs() > EPSILON_MM && len + d >= FILLET_MIN_KEPT_MM && len + d <= max_len
            })
            .collect();
        let Some(&&(_, delta)) = usable
            .iter()
            .min_by(|a, b| distance_mm(a.0, pick).total_cmp(&distance_mm(b.0, pick)))
        else {
            return Err(CoreError::Invalid(format!(
                "{name} s'arrête déjà sur {tname}, ou l'intersection ne laisse aucune longueur \
                 exploitable"
            )));
        };
        self.extend_path(sheet, entity, end, delta, out)
    }

    /// Point de coupe : celui imposé par `hint`, sinon un point libre en `at`.
    fn cut_point(
        &mut self,
        sheet: SheetId,
        at: PointMm,
        layer: Option<LayerId>,
        hint: SplitHint,
        out: &mut Outcome,
    ) -> Result<PointId, CoreError> {
        match hint.point {
            Some(p) => Ok(p),
            None => self.tangency_point(sheet, at, layer, out),
        }
    }

    /// Coupe un segment, un arc, une courbe de Bézier (ou une courbe lisse à
    /// l'un de ses points de passage) en `at` : deux tracés, le second est créé.
    pub(super) fn split_path(
        &mut self,
        sheet: SheetId,
        entity: EntityId,
        at: PointMm,
        hint: SplitHint,
        out: &mut Outcome,
    ) -> Result<(), CoreError> {
        at.validate()?;
        let s = self.sheet(sheet)?;
        let name = s.entity_name(entity);
        let e = s.entity(entity)?.clone();
        let len = s.path_length_mm(entity)?;
        let outside = |gap: f64| {
            CoreError::Invalid(format!(
                "{name} : le point visé est à {} mm du tracé : la coupe se fait en un point du \
                 tracé",
                format_mm(gap)
            ))
        };
        let at_end = || {
            CoreError::Invalid(format!(
                "{name} : le point de coupe est une extrémité du tracé : rien à couper"
            ))
        };
        let plan = match &e.kind {
            EntityKind::Circle { .. } => {
                return Err(CoreError::Invalid(format!(
                    "{name} : un cercle n'a pas d'extrémité, il ne se coupe pas (tracez des arcs)"
                )))
            }
            EntityKind::Segment { from, to, .. } => {
                let d = s.path_distance_of(entity, at)?;
                let (x, _) = s.path_point_at(entity, d.clamp(0.0, len))?;
                if distance_mm(x, at) > SPLIT_TOLERANCE_MM {
                    return Err(outside(distance_mm(x, at)));
                }
                if d <= SPLIT_TOLERANCE_MM || d >= len - SPLIT_TOLERANCE_MM {
                    return Err(at_end());
                }
                CutPlan::Segment {
                    from: *from,
                    to: *to,
                    at: x,
                }
            }
            EntityKind::Arc {
                center,
                start,
                end,
                ccw,
            } => {
                let d = s.path_distance_of(entity, at)?;
                let (x, _) = s.path_point_at(entity, d.clamp(0.0, len))?;
                if distance_mm(x, at) > SPLIT_TOLERANCE_MM {
                    return Err(outside(distance_mm(x, at)));
                }
                if d <= SPLIT_TOLERANCE_MM || d >= len - SPLIT_TOLERANCE_MM {
                    return Err(at_end());
                }
                CutPlan::Arc {
                    center: *center,
                    start: *start,
                    end: *end,
                    ccw: *ccw,
                    at: x,
                }
            }
            EntityKind::Curve { start, end, .. } => {
                let c = s.curve_geometry(entity)?;
                let (d, t) = match hint.t {
                    Some(t) => (
                        c.length_to_t_mm(
                            t.clamp(0.0, 1.0),
                            pf_geometry::DEFAULT_LENGTH_TOLERANCE_MM,
                        )?,
                        t,
                    ),
                    None => {
                        let d = s.path_distance_of(entity, at)?;
                        let t = c.t_at_length_mm(
                            d.clamp(0.0, len),
                            pf_geometry::DEFAULT_LENGTH_TOLERANCE_MM,
                        )?;
                        (d, t)
                    }
                };
                let x = c.point_at(t);
                if distance_mm(x, at) > SPLIT_TOLERANCE_MM {
                    return Err(outside(distance_mm(x, at)));
                }
                if d <= SPLIT_TOLERANCE_MM || d >= len - SPLIT_TOLERANCE_MM {
                    return Err(at_end());
                }
                let (q0, q1, q2) = (c.p0.lerp(c.c1, t), c.c1.lerp(c.c2, t), c.c2.lerp(c.p3, t));
                let (r0, r1) = (q0.lerp(q1, t), q1.lerp(q2, t));
                let x = r0.lerp(r1, t);
                CutPlan::Curve {
                    start: *start,
                    end: *end,
                    at: x,
                    first: (
                        OffsetMm::from_vector(q0 - c.p0),
                        OffsetMm::from_vector(r0 - x),
                    ),
                    second: (
                        OffsetMm::from_vector(r1 - x),
                        OffsetMm::from_vector(q2 - c.p3),
                    ),
                }
            }
            EntityKind::Spline { nodes } => {
                let res = s.spline_nodes(entity)?;
                let i = res
                    .iter()
                    .position(|r| distance_mm(r.position, at) <= SPLIT_TOLERANCE_MM)
                    .ok_or_else(|| {
                        CoreError::Invalid(format!(
                            "{name} : une courbe lisse se coupe en l'un de ses points de passage \
                             (le point visé n'en est pas un)"
                        ))
                    })?;
                if i == 0 || i == nodes.len() - 1 {
                    return Err(at_end());
                }
                // Les deux nouvelles extrémités portent la tangente effective : la
                // forme de chaque tronçon est exactement celle d'avant.
                let mut first = nodes[..=i].to_vec();
                first[i] = SplineNode {
                    point: nodes[i].point,
                    corner: false,
                    tangent: Some(OffsetMm::from_vector(res[i].tangent_in)),
                    tangent_in: None,
                };
                let mut second = nodes[i..].to_vec();
                second[0] = SplineNode {
                    point: nodes[i].point,
                    corner: false,
                    tangent: Some(OffsetMm::from_vector(res[i].tangent_out)),
                    tangent_in: None,
                };
                CutPlan::Spline { first, second }
            }
        };
        let layer = e.layer;
        self.freeze_dependents(sheet, &[entity], out)?;
        self.release_notch_anchors(sheet, &[entity], out)?;
        let (first_kind, second_kind) = match plan {
            CutPlan::Segment { from, to, at } => {
                let x = self.cut_point(sheet, at, layer, hint, out)?;
                (
                    EntityKind::Segment {
                        from,
                        to: x,
                        construction: None,
                    },
                    EntityKind::Segment {
                        from: x,
                        to,
                        construction: None,
                    },
                )
            }
            CutPlan::Arc {
                center,
                start,
                end,
                ccw,
                at,
            } => {
                let x = self.cut_point(sheet, at, layer, hint, out)?;
                (
                    EntityKind::Arc {
                        center,
                        start,
                        end: x,
                        ccw,
                    },
                    EntityKind::Arc {
                        center,
                        start: x,
                        end,
                        ccw,
                    },
                )
            }
            CutPlan::Curve {
                start,
                end,
                at,
                first,
                second,
            } => {
                let x = self.cut_point(sheet, at, layer, hint, out)?;
                (
                    EntityKind::Curve {
                        start,
                        handle_start: first.0,
                        handle_end: first.1,
                        end: x,
                    },
                    EntityKind::Curve {
                        start: x,
                        handle_start: second.0,
                        handle_end: second.1,
                        end,
                    },
                )
            }
            CutPlan::Spline { first, second } => (
                EntityKind::Spline { nodes: first },
                EntityKind::Spline { nodes: second },
            ),
        };
        let new_id = EntityId(self.alloc_id());
        let s = self.sheet_mut(sheet)?;
        let second_name = match &e.name {
            None => None,
            Some(n) => {
                let candidate = format!("{n} (2)");
                if candidate.chars().count() <= MAX_NAME_LEN
                    && !s
                        .entities
                        .iter()
                        .any(|x| x.name.as_deref() == Some(candidate.as_str()))
                {
                    Some(candidate)
                } else {
                    out.notes.push(format!(
                        "Le nom « {n} » reste au premier tronçon ; le second n'a pas de nom."
                    ));
                    None
                }
            }
        };
        let idx = s
            .entities
            .iter()
            .position(|x| x.id == entity)
            .ok_or(CoreError::UnknownEntity(entity))?;
        s.entities[idx].kind = first_kind;
        s.entities.insert(
            idx + 1,
            Entity {
                id: new_id,
                role: e.role,
                layer: e.layer,
                name: second_name,
                kind: second_kind,
            },
        );
        if let Some(g) = s.groups.iter_mut().find(|g| g.members.contains(&entity)) {
            g.members.push(new_id);
        }
        // Crans : chacun reste à la même place géométrique, sur le bon tronçon.
        let (l1, l2) = (s.path_length_mm(entity)?, s.path_length_mm(new_id)?);
        for n in s.notches.iter_mut().filter(|n| n.entity == entity) {
            if n.distance_mm > l1 + EPSILON_MM {
                n.entity = new_id;
                n.distance_mm = (n.distance_mm - l1).clamp(0.0, l2);
            } else {
                n.distance_mm = n.distance_mm.min(l1);
            }
        }
        let new_name = s.entity_name(new_id);
        // Surpiqûres liées dont le bord est coupé : le second tronçon prolonge le bord (US-06.8).
        self.topstitch_after_split(sheet, entity, new_id);
        for piece in &mut self.pieces {
            if let Some(i) = piece.outline.iter().position(|x| x.entity == entity) {
                let reversed = piece.outline[i].reversed;
                // Parcouru à l'envers, le second tronçon vient avant le premier.
                let at = if reversed { i } else { i + 1 };
                piece.outline.insert(
                    at,
                    PieceEdge {
                        entity: new_id,
                        reversed,
                    },
                );
            }
            if piece.grainline == Some(entity) {
                out.notes.push(format!(
                    "Le droit-fil de la pièce « {} » reste sur le premier tronçon : vérifiez-le.",
                    piece.name
                ));
            }
            // Ligne interne coupée : le second tronçon la prolonge, avec le même type.
            let here = InternalTarget::Path { entity };
            if let Some(el) = piece.internal.iter().find(|i| i.target == here).copied() {
                piece.internal.push(InternalElement {
                    target: InternalTarget::Path { entity: new_id },
                    kind: el.kind,
                });
                out.notes.push(format!(
                    "La ligne interne de la pièce « {} » est coupée : le second tronçon est aussi                      un élément interne.",
                    piece.name
                ));
            }
        }
        out.entities.push(new_id);
        out.notes.push(format!(
            "{name} est coupé en deux : le second tronçon est {new_name}."
        ));
        Ok(())
    }

    /// Fusionne deux tracés bout à bout : `first` survit (identifiant, nom,
    /// calque, rôle, groupe) et reçoit la géométrie, les crans et le contour de
    /// `second`, qui disparaît.
    pub(super) fn merge_paths(
        &mut self,
        sheet: SheetId,
        first: EntityId,
        second: EntityId,
        out: &mut Outcome,
    ) -> Result<(), CoreError> {
        if first == second {
            return Err(CoreError::Invalid(
                "choisissez deux tracés différents".into(),
            ));
        }
        let s = self.sheet(sheet)?;
        let (n1, n2) = (s.entity_name(first), s.entity_name(second));
        let (e1, e2) = (s.entity(first)?.clone(), s.entity(second)?.clone());
        let bad = |msg: String| {
            CoreError::Invalid(format!("fusion impossible entre {n1} et {n2} : {msg}"))
        };
        if std::mem::discriminant(&e1.kind) != std::mem::discriminant(&e2.kind) {
            return Err(bad(format!(
                "types incompatibles ({} et {})",
                kind_label(&e1.kind),
                kind_label(&e2.kind)
            )));
        }
        if matches!(e1.kind, EntityKind::Circle { .. }) {
            return Err(bad("un cercle est déjà fermé".into()));
        }
        if e1.layer != e2.layer {
            return Err(bad(format!(
                "ils sont sur des calques différents (« {} » et « {} »)",
                s.layer_name(e1.layer),
                s.layer_name(e2.layer)
            )));
        }
        if e1.role != e2.role {
            return Err(bad(format!(
                "leurs rôles diffèrent ({} et {})",
                e1.role.label_fr(),
                e2.role.label_fr()
            )));
        }
        let (g1, g2) = (
            s.group_of(first).map(|g| g.id),
            s.group_of(second).map(|g| g.id),
        );
        if let (Some(a), Some(b)) = (g1, g2) {
            if a != b {
                return Err(bad("ils sont dans deux groupes différents".into()));
            }
        }
        let (a1, b1) = s.entity_points(first)?;
        let (a2, b2) = s.entity_points(second)?;
        let shared: Vec<PointId> = [a1, b1]
            .into_iter()
            .filter(|q| *q == a2 || *q == b2)
            .collect();
        let p = match shared.as_slice() {
            [] => {
                return Err(bad(
                    "les tracés ne sont pas adjacents (aucune extrémité commune)".into(),
                ))
            }
            [p] => *p,
            _ => {
                return Err(bad(
                    "ils forment déjà une boucle fermée (deux extrémités communes)".into(),
                ))
            }
        };
        let case_a = p == b1;
        let flip2 = if case_a { p == b2 } else { p == a2 };
        let merged = merged_kind(s, first, second, p, case_a, flip2)?;
        let (l1, l2) = (s.path_length_mm(first)?, s.path_length_mm(second)?);
        self.freeze_dependents(sheet, &[first, second], out)?;
        self.release_notch_anchors(sheet, &[first, second], out)?;
        let s = self.sheet_mut(sheet)?;
        s.entity_mut(first)?.kind = merged;
        s.entities.retain(|x| x.id != second);
        // Crans de `second`, et de `first` si `second` passe devant lui.
        for n in s.notches.iter_mut() {
            if n.entity == first {
                if !case_a {
                    n.distance_mm += l2;
                }
            } else if n.entity == second {
                let d = n.distance_mm;
                let (nd, flip_side) = match (case_a, flip2) {
                    (true, false) => (l1 + d, false),
                    (true, true) => (l1 + l2 - d, true),
                    (false, false) => (d, false),
                    (false, true) => (l2 - d, true),
                };
                n.entity = first;
                n.distance_mm = nd;
                if flip_side {
                    n.side = n.side.flipped();
                }
            }
        }
        let lm = s.path_length_mm(first)?;
        for n in s.notches.iter_mut().filter(|n| n.entity == first) {
            n.distance_mm = n.distance_mm.clamp(0.0, lm);
        }
        // Groupes.
        match (g1, g2) {
            (None, Some(g)) => {
                if let Some(m) = s
                    .groups
                    .iter_mut()
                    .find(|x| x.id == g)
                    .and_then(|x| x.members.iter_mut().find(|m| **m == second))
                {
                    *m = first;
                }
            }
            (_, Some(g)) => {
                if let Some(x) = s.groups.iter_mut().find(|x| x.id == g) {
                    x.members.retain(|m| *m != second);
                }
            }
            _ => {}
        }
        if let Some(n) = &e2.name {
            out.notes.push(format!(
                "Le nom « {n} » disparaît avec le second tracé fusionné."
            ));
        }
        // Surpiqûres liées dont le bord est fusionné : le bord suit `first` (US-06.8).
        self.topstitch_after_merge(sheet, first, second);
        // Contours de pièce.
        for piece in &mut self.pieces {
            if piece.grainline == Some(second) {
                piece.grainline = Some(first);
            }
            // Ligne interne absorbée : elle devient `first`, sauf si `first` est déjà
            // contour, droit-fil ou élément interne de la pièce (alors elle est retirée).
            let (t_first, t_second) = (
                InternalTarget::Path { entity: first },
                InternalTarget::Path { entity: second },
            );
            if let Some(i) = piece.internal.iter().position(|x| x.target == t_second) {
                let taken = piece.internal.iter().any(|x| x.target == t_first)
                    || piece.outline.iter().any(|e| e.entity == first)
                    || piece.grainline == Some(first);
                if taken {
                    piece.internal.remove(i);
                } else {
                    piece.internal[i].target = t_first;
                }
                out.notes.push(format!(
                    "Une ligne interne de la pièce « {} » est fusionnée avec un autre tracé :                      vérifiez ses éléments internes.",
                    piece.name
                ));
            }
            let pos = |e: EntityId| piece.outline.iter().position(|x| x.entity == e);
            match (pos(first), pos(second)) {
                (Some(a), Some(b)) => {
                    let n = piece.outline.len();
                    if (a + 1) % n == b || (b + 1) % n == a {
                        piece.outline.remove(b);
                    } else {
                        piece.outline.clear();
                        out.notes.push(format!(
                            "Le contour de la pièce « {} » ne chaînait pas les deux tracés \
                             fusionnés : il est vidé.",
                            piece.name
                        ));
                    }
                }
                (None, Some(b)) => {
                    let edge = &mut piece.outline[b];
                    edge.entity = first;
                    edge.reversed ^= flip2;
                    out.notes.push(format!(
                        "Le tracé fusionné remplace un tracé de la pièce « {} » : vérifiez son \
                         contour.",
                        piece.name
                    ));
                }
                (Some(_), None) => out.notes.push(format!(
                    "Le tracé fusionné appartient à la pièce « {} » sans le tracé absorbé : \
                     vérifiez son contour.",
                    piece.name
                )),
                (None, None) => {}
            }
        }
        self.drop_unused_point(sheet, p)?;
        out.entities.push(first);
        Ok(())
    }

    /// Inverse le sens d'un tracé (départ ↔ arrivée) sans changer sa forme :
    /// crans, points construits sur ce tracé et contours de pièce suivent.
    pub(super) fn reverse_path(
        &mut self,
        sheet: SheetId,
        entity: EntityId,
        out: &mut Outcome,
    ) -> Result<(), CoreError> {
        let s = self.sheet(sheet)?;
        let len = s.path_length_mm(entity)?;
        let kind = reversed_kind(&s.entity(entity)?.kind);
        // Un report depuis ce tracé change de sens avec lui : détaché plutôt que déplacé.
        let carried_from: Vec<NotchId> = s
            .notches
            .iter()
            .filter(
                |n| matches!(n.anchor, Some(NotchAnchor::Carried { entity: e, .. }) if e == entity),
            )
            .map(|n| n.id)
            .collect();
        if !carried_from.is_empty() {
            self.release_notch_anchors_of(sheet, &carried_from, out)?;
        }
        let s = self.sheet_mut(sheet)?;
        s.entity_mut(entity)?.kind = kind;
        for p in s.points.iter_mut() {
            reversed_definition(&mut p.definition, entity);
        }
        // Le côté d'un cran est donné par rapport au sens du tracé : il change
        // avec lui pour que le cran reste du même côté de la pièce.
        for n in s.notches.iter_mut().filter(|n| n.entity == entity) {
            // Un cran ancré garde son ancrage : la proportion et le report se comptent
            // depuis l'autre bout ; milieu et point ne changent pas.
            match &mut n.anchor {
                None => n.distance_mm = (len - n.distance_mm).clamp(0.0, len),
                Some(NotchAnchor::Ratio { ratio }) => *ratio = 1.0 - *ratio,
                Some(NotchAnchor::Carried { from_end, .. }) => *from_end = !*from_end,
                Some(NotchAnchor::Point { .. } | NotchAnchor::Midpoint {}) => {}
            }
            n.side = n.side.flipped();
        }
        for piece in &mut self.pieces {
            for edge in piece.outline.iter_mut().filter(|x| x.entity == entity) {
                edge.reversed = !edge.reversed;
            }
        }
        Ok(())
    }
}
