//! Oracle de résolution des points : transcription de l'ancien calcul récursif (sans cache),
//! écrite uniquement avec l'API publique.

#![allow(dead_code)]

use pf_core::*;
use pf_geometry::{distance_mm, PointMm, VectorMm};

pub type R<T> = Result<T, CoreError>;

pub fn o_pos(s: &Sheet, id: PointId, path: &mut Vec<PointId>) -> R<PointMm> {
    if path.contains(&id) {
        return Err(CoreError::CyclicReference(id));
    }
    path.push(id);
    let r = s
        .points
        .iter()
        .find(|p| p.id == id)
        .ok_or(CoreError::UnknownPoint(id))
        .and_then(|p| o_eval(s, &p.definition, &p.label, path));
    path.pop();
    r
}

pub fn o_ent(s: &Sheet, id: EntityId) -> R<&Entity> {
    s.entities
        .iter()
        .find(|e| e.id == id)
        .ok_or(CoreError::UnknownEntity(id))
}

fn o_segment_ends(
    s: &Sheet,
    segment: EntityId,
    who: &str,
    path: &mut Vec<PointId>,
) -> R<(PointMm, PointMm)> {
    let ids = match o_ent(s, segment).map(|e| &e.kind) {
        Ok(EntityKind::Segment { from, to, .. }) => Some((*from, *to)),
        _ => None,
    };
    let (from, to) = ids.ok_or_else(|| {
        CoreError::DeadReference(format!(
            "le point {who} dépend d'un segment absent (id {})",
            segment.0
        ))
    })?;
    Ok((o_pos(s, from, path)?, o_pos(s, to, path)?))
}

pub fn o_curve(s: &Sheet, id: EntityId, path: &mut Vec<PointId>) -> R<pf_geometry::CubicBezier> {
    match o_ent(s, id)?.kind {
        EntityKind::Curve {
            start,
            handle_start,
            handle_end,
            end,
        } => {
            let (p0, p3) = (o_pos(s, start, path)?, o_pos(s, end, path)?);
            Ok(pf_geometry::CubicBezier::new(
                p0,
                p0 + handle_start.vector(),
                p3 + handle_end.vector(),
                p3,
            ))
        }
        _ => Err(CoreError::NotACurve(id)),
    }
}

pub fn o_spline_nodes(
    s: &Sheet,
    id: EntityId,
    path: &mut Vec<PointId>,
) -> R<Vec<ResolvedSplineNode>> {
    let EntityKind::Spline { nodes } = &o_ent(s, id)?.kind else {
        return Err(CoreError::Invalid(format!(
            "{} n'est pas une courbe par points de passage",
            s.entity_name(id)
        )));
    };
    let pos = nodes
        .iter()
        .map(|n| o_pos(s, n.point, path))
        .collect::<R<Vec<_>>>()?;
    Ok(nodes
        .iter()
        .enumerate()
        .map(|(i, n)| {
            let prev = i.checked_sub(1).map(|j| pos[j]);
            let next = pos.get(i + 1).copied();
            let (def_in, def_out) = if n.corner {
                pf_geometry::default_corner_tangents(prev, pos[i], next)
            } else {
                let t = pf_geometry::default_smooth_tangent(prev, pos[i], next);
                (t, t)
            };
            let out = n.tangent.map(OffsetMm::vector);
            let inn = if n.corner {
                n.tangent_in.map(OffsetMm::vector)
            } else {
                out
            };
            ResolvedSplineNode {
                point: n.point,
                position: pos[i],
                corner: n.corner,
                tangent_in: inn.unwrap_or(def_in),
                tangent_out: out.unwrap_or(def_out),
                tangent_in_explicit: inn.is_some(),
                tangent_out_explicit: out.is_some(),
            }
        })
        .collect())
}

pub fn o_shape(s: &Sheet, id: EntityId, path: &mut Vec<PointId>) -> R<Shape> {
    let label =
        |e: pf_geometry::GeometryError| CoreError::Invalid(format!("{} : {e}", s.entity_name(id)));
    Ok(match o_ent(s, id)?.kind {
        EntityKind::Segment { from, to, .. } => {
            Shape::Line(o_pos(s, from, path)?, o_pos(s, to, path)?)
        }
        EntityKind::Spline { .. } => {
            let hermite: Vec<_> = o_spline_nodes(s, id, path)?
                .into_iter()
                .map(|n| pf_geometry::HermiteNode {
                    point: n.position,
                    tangent_in: n.tangent_in,
                    tangent_out: n.tangent_out,
                })
                .collect();
            Shape::Spline(pf_geometry::Spline::from_nodes(&hermite).map_err(label)?)
        }
        EntityKind::Curve { .. } => Shape::Cubic(o_curve(s, id, path)?),
        EntityKind::Circle {
            center,
            radius_mm,
            start_deg,
            ccw,
        } => Shape::Circle(
            pf_geometry::CircleTrace::new(o_pos(s, center, path)?, radius_mm, start_deg, ccw)
                .map_err(label)?,
        ),
        EntityKind::Arc {
            center,
            start,
            end,
            ccw,
        } => Shape::Arc(
            pf_geometry::Arc::from_center_points(
                o_pos(s, center, path)?,
                o_pos(s, start, path)?,
                o_pos(s, end, path)?,
                ccw,
            )
            .map_err(label)?,
        ),
    })
}

fn o_intersections(a: &Shape, b: &Shape) -> R<Vec<PointMm>> {
    let circle = |s: &Shape| match s {
        Shape::Arc(x) => pf_geometry::Circle::new(x.center, x.radius_mm).ok(),
        Shape::Circle(x) => Some(x.circle),
        _ => None,
    };
    match (a, b, circle(a), circle(b)) {
        (_, _, Some(c1), Some(c2)) => Ok(pf_geometry::circle_circle_intersections(&c1, &c2)?),
        (_, Shape::Line(p, q), Some(c), None) | (Shape::Line(p, q), _, None, Some(c)) => {
            Ok(pf_geometry::circle_line_intersections(&c, *p, *q)?)
        }
        _ => Err(CoreError::Invalid(
            "l'intersection demandée exige un cercle ou un arc et un segment, un autre cercle              ou un autre arc"
                .into(),
        )),
    }
}

fn o_two_shapes(
    s: &Sheet,
    first: EntityId,
    second: EntityId,
    who: &str,
    path: &mut Vec<PointId>,
) -> R<(Shape, Shape)> {
    let dead = |id: EntityId| {
        CoreError::DeadReference(format!(
            "le point {who} dépend d'un tracé absent (id {})",
            id.0
        ))
    };
    let a = o_shape(s, first, path).map_err(|e| match e {
        CoreError::UnknownEntity(_) => dead(first),
        o => o,
    })?;
    let b = o_shape(s, second, path).map_err(|e| match e {
        CoreError::UnknownEntity(_) => dead(second),
        o => o,
    })?;
    Ok((a, b))
}

fn o_parent(s: &Sheet, id: PointId, who: &str, path: &mut Vec<PointId>) -> R<PointMm> {
    o_pos(s, id, path).map_err(|e| match e {
        CoreError::UnknownPoint(p) => CoreError::DeadReference(format!(
            "le point {who} dépend d'un point absent (id {})",
            p.0
        )),
        other => other,
    })
}

fn o_trace(s: &Sheet, id: EntityId, who: &str, path: &mut Vec<PointId>) -> R<Shape> {
    o_shape(s, id, path).map_err(|e| match e {
        CoreError::UnknownEntity(_) => CoreError::DeadReference(format!(
            "le point {who} dépend d'un tracé absent (id {})",
            id.0
        )),
        other => other,
    })
}

fn is_segment(s: &Sheet, id: EntityId) -> bool {
    matches!(
        o_ent(s, id).map(|e| &e.kind),
        Ok(EntityKind::Segment { .. })
    )
}

pub fn o_eval(s: &Sheet, def: &PointDefinition, who: &str, path: &mut Vec<PointId>) -> R<PointMm> {
    use PointDefinition as D;
    let geo = |e: pf_geometry::GeometryError| CoreError::Invalid(format!("point {who} : {e}"));
    let invalid = |m: String| CoreError::Invalid(m);
    match *def {
        D::Free { x, y } => Ok(PointMm::new(x, y).validate()?),
        D::OnSegment {
            segment,
            distance_mm,
            origin,
        } => {
            let (a, b) = o_segment_ends(s, segment, who, path)?;
            let (a, b) = match origin {
                SegmentEnd::Start => (a, b),
                SegmentEnd::End => (b, a),
            };
            pf_geometry::point_on_segment(a, b, distance_mm).map_err(geo)
        }
        D::OnSegmentRatio { segment, ratio } => {
            if !ratio.is_finite() || !(0.0..=1.0).contains(&ratio) {
                return Err(geo(pf_geometry::GeometryError::ParameterOutOfRange(ratio)));
            }
            let (a, b) = o_segment_ends(s, segment, who, path)?;
            pf_geometry::segment_length_mm(a, b).map_err(geo)?;
            Ok(a.lerp(b, ratio))
        }
        D::Intersection { first, second } => {
            let (a, b) = o_segment_ends(s, first, who, path)?;
            let (c, d) = o_segment_ends(s, second, who, path)?;
            pf_geometry::line_intersection(a, b, c, d).map_err(geo)
        }
        D::CircleIntersection {
            first,
            second,
            near,
        } => {
            let (a, b) = o_two_shapes(s, first, second, who, path)?;
            let sols = o_intersections(&a, &b)?;
            pf_geometry::nearest_solution(&sols, near.point()).ok_or_else(|| {
                invalid(format!(
                    "le point {who} : les deux tracés ne se coupent plus"
                ))
            })
        }
        D::TraceIntersection {
            first,
            second,
            near,
        } => {
            near.point().validate()?;
            if first == second {
                return Err(invalid(format!(
                    "le point {who} : une intersection exige deux tracés différents"
                )));
            }
            let (a, b) = o_two_shapes(s, first, second, who, path)?;
            let hits = pf_geometry::intersect(&a.to_trace(), &b.to_trace())
                .map_err(|e| invalid(format!("le point {who} : {e}")))?;
            hits.iter()
                .map(|h| h.point)
                .min_by(|p, q| {
                    distance_mm(*p, near.point()).total_cmp(&distance_mm(*q, near.point()))
                })
                .ok_or_else(|| {
                    invalid(format!(
                        "le point {who} : les deux tracés ne se coupent plus"
                    ))
                })
        }
        D::OnCurve { curve, t } => {
            let g = o_curve(s, curve, path).map_err(|e| match e {
                CoreError::UnknownEntity(_) | CoreError::NotACurve(_) => {
                    CoreError::DeadReference(format!(
                        "le point {who} dépend d'une courbe absente (id {})",
                        curve.0
                    ))
                }
                other => other,
            })?;
            g.point_at_checked(t).map_err(geo)
        }
        D::AlongCurve {
            curve,
            distance_mm: d,
            origin,
        } => {
            let shape = o_trace(s, curve, who, path)?;
            let len = shape.length_mm()?;
            if !d.is_finite() || d < 0.0 || d > len + 1e-9 {
                return Err(invalid(format!(
                    "le point {who} : la distance {d} mm dépasse la longueur du tracé \
                     ({len} mm) ou est négative"
                )));
            }
            let from_start = match origin {
                SegmentEnd::Start => d,
                SegmentEnd::End => len - d,
            };
            shape.point_at_length(from_start.clamp(0.0, len))
        }
        D::Offset { from, dx_mm, dy_mm } => {
            let p = o_parent(s, from, who, path)?;
            pf_geometry::point_offset(p, dx_mm, dy_mm).map_err(geo)
        }
        D::Midpoint { first, second } => {
            if first == second {
                return Err(invalid(format!(
                    "le point {who} : un milieu exige deux points différents"
                )));
            }
            let a = o_parent(s, first, who, path)?;
            let b = o_parent(s, second, who, path)?;
            Ok(pf_geometry::midpoint(a, b))
        }
        D::Projection { point, trace } => {
            let p = o_parent(s, point, who, path)?;
            match o_trace(s, trace, who, path)? {
                Shape::Line(a, b) => pf_geometry::project_on_line(p, a, b).map_err(geo),
                Shape::Circle(c) => pf_geometry::project_on_circle(p, &c.circle).map_err(geo),
                Shape::Arc(a) => pf_geometry::project_on_arc(p, &a).map_err(geo),
                Shape::Cubic(c) => pf_geometry::project_on_cubic(p, &c).ok_or_else(|| {
                    invalid(format!(
                        "le point {who} : aucun pied de perpendiculaire sur la courbe"
                    ))
                }),
                Shape::Spline(sp) => pf_geometry::project_on_spline(p, &sp).map_err(geo),
            }
        }
        D::Bisector {
            vertex,
            first,
            second,
            distance_mm: d,
        } => {
            let v = o_parent(s, vertex, who, path)?;
            let a = o_parent(s, first, who, path)?;
            let b = o_parent(s, second, who, path)?;
            pf_geometry::bisector_point(v, a, b, d).map_err(geo)
        }
        D::Polar {
            from,
            distance_mm: d,
            angle_deg,
            reference,
        } => {
            let p = o_parent(s, from, who, path)?;
            let angle = match reference {
                None => angle_deg,
                Some(r) => {
                    if !is_segment(s, r) {
                        return Err(CoreError::DeadReference(format!(
                            "le point {who} dépend d'un segment de référence absent (id {})",
                            r.0
                        )));
                    }
                    let Shape::Line(a, b) = o_trace(s, r, who, path)? else {
                        unreachable!()
                    };
                    pf_geometry::relative_angle_deg(a, b, angle_deg).map_err(geo)?
                }
            };
            pf_geometry::point_at_direction(p, d, angle).map_err(geo)
        }
        D::Compass {
            center_a,
            center_b,
            radius_a_mm,
            radius_b_mm,
            near,
        } => {
            near.point().validate()?;
            let a = o_parent(s, center_a, who, path)?;
            let b = o_parent(s, center_b, who, path)?;
            let sols =
                pf_geometry::compass_solutions(a, radius_a_mm, b, radius_b_mm).map_err(geo)?;
            sols.iter()
                .copied()
                .min_by(|p, q| {
                    distance_mm(*p, near.point()).total_cmp(&distance_mm(*q, near.point()))
                })
                .ok_or_else(|| {
                    invalid(format!(
                        "le point {who} : les deux arcs de compas ne se coupent pas"
                    ))
                })
        }
        D::Division {
            trace,
            ratio,
            start,
        } => {
            if !ratio.is_finite() || ratio <= 0.0 || ratio >= 1.0 {
                return Err(invalid(format!(
                    "le point {who} : la fraction de division doit être strictement \
                     comprise entre 0 et 1"
                )));
            }
            let shape = o_trace(s, trace, who, path)?;
            match (shape, start) {
                (Shape::Circle(c), Some(st)) => {
                    let st = o_parent(s, st, who, path)?;
                    let v = st - c.circle.center;
                    if v.x.hypot(v.y) <= pf_geometry::EPSILON_MM {
                        return Err(invalid(format!(
                            "le point {who} : le point de départ est confondu avec le \
                             centre du cercle"
                        )));
                    }
                    let from = pf_geometry::CircleTrace::new(
                        c.circle.center,
                        c.circle.radius_mm,
                        v.y.atan2(v.x).to_degrees(),
                        c.ccw,
                    )
                    .map_err(geo)?;
                    Ok(from
                        .point_at_length(ratio * from.length_mm())
                        .map_err(geo)?
                        .0)
                }
                (_, Some(_)) => Err(invalid(format!(
                    "le point {who} : un point de départ ne s'applique qu'à un cercle"
                ))),
                (shape, None) => shape.point_at_length(ratio * shape.length_mm()?),
            }
        }
        D::Mirror { source, axis } => {
            let p = o_parent(s, source, who, path)?;
            let (origin, dir) = match axis {
                MirrorAxis::Guide { guide } => {
                    let g = s.guide(guide).map_err(|_| {
                        CoreError::DeadReference(format!(
                            "le point {who} dépend d'une ligne d'aide absente (id {})",
                            guide.0
                        ))
                    })?;
                    (g.origin(), g.direction())
                }
                MirrorAxis::Segment { segment } => {
                    if !is_segment(s, segment) {
                        return Err(CoreError::DeadReference(format!(
                            "le point {who} dépend d'un segment d'axe absent (id {})",
                            segment.0
                        )));
                    }
                    let Shape::Line(a, b) = o_trace(s, segment, who, path)? else {
                        unreachable!()
                    };
                    let dir = (b - a).unit().map_err(|_| {
                        invalid(format!("le point {who} : l'axe de symétrie est dégénéré"))
                    })?;
                    (a, dir)
                }
            };
            let v: VectorMm = p - origin;
            let k = 2.0 * dir.dot(v);
            let r = VectorMm::new(dir.x * k - v.x, dir.y * k - v.y);
            (origin + r).validate().map_err(geo)
        }
    }
}
