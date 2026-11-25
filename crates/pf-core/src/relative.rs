//! Évaluation des points relatifs (US-02.5) : tous sont des points construits
//! maintenus, réévalués à partir de leurs parents (points ou tracés). La
//! détection de cycle passe par la résolution `path` de `position_rec`, à pile explicite : aucune
//! fonction d'ici ne doit ignorer l'échec d'un parent (il sert aussi à le réclamer).

use crate::error::CoreError;
use crate::model::{
    EntityId, EntityKind, MirrorAxis, PointDefinition, Resolution, SegmentEnd, Shape, Sheet,
};
use pf_geometry::{distance_mm, PointMm, VectorMm, DEFAULT_LENGTH_TOLERANCE_MM};

impl Shape {
    /// Longueur développée du tracé, en mm.
    pub fn length_mm(&self) -> Result<f64, CoreError> {
        Ok(match self {
            Shape::Line(a, b) => pf_geometry::segment_length_mm(*a, *b)?,
            Shape::Cubic(c) => c.length_mm(DEFAULT_LENGTH_TOLERANCE_MM)?,
            Shape::Arc(a) => a.length_mm(),
            Shape::Circle(c) => c.length_mm(),
            Shape::Spline(s) => s.length_mm()?,
        })
    }

    /// Point situé à `distance_mm` de longueur d'arc du départ du tracé.
    pub fn point_at_length(&self, distance_mm: f64) -> Result<PointMm, CoreError> {
        Ok(match self {
            Shape::Line(a, b) => pf_geometry::point_on_segment(*a, *b, distance_mm)?,
            Shape::Cubic(c) => {
                c.point_at(c.t_at_length_mm(distance_mm, DEFAULT_LENGTH_TOLERANCE_MM)?)
            }
            Shape::Arc(a) => a.point_at_length(distance_mm)?.0,
            Shape::Circle(c) => c.point_at_length(distance_mm)?.0,
            Shape::Spline(s) => s.point_at_length(distance_mm)?.0,
        })
    }
}

impl Sheet {
    fn parent_pos(
        &self,
        id: crate::model::PointId,
        who: &str,
        path: &mut Resolution,
    ) -> Result<PointMm, CoreError> {
        self.position_rec(id, path).map_err(|e| match e {
            CoreError::UnknownPoint(p) => CoreError::DeadReference(format!(
                "le point {who} dépend d'un point absent (id {})",
                p.0
            )),
            other => other,
        })
    }

    fn trace_shape(
        &self,
        id: EntityId,
        who: &str,
        path: &mut Resolution,
    ) -> Result<Shape, CoreError> {
        self.shape_rec(id, path).map_err(|e| match e {
            CoreError::UnknownEntity(_) => CoreError::DeadReference(format!(
                "le point {who} dépend d'un tracé absent (id {})",
                id.0
            )),
            other => other,
        })
    }

    pub(crate) fn relative_rec(
        &self,
        definition: &PointDefinition,
        who: &str,
        path: &mut Resolution,
    ) -> Result<PointMm, CoreError> {
        let geo = |e: pf_geometry::GeometryError| CoreError::Invalid(format!("point {who} : {e}"));
        match *definition {
            PointDefinition::AlongCurve {
                curve,
                distance_mm: d,
                origin,
            } => {
                let shape = self.trace_shape(curve, who, path)?;
                let len = shape.length_mm()?;
                if !d.is_finite() || d < 0.0 || d > len + 1e-9 {
                    return Err(CoreError::Invalid(format!(
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
            PointDefinition::Offset { from, dx_mm, dy_mm } => {
                let p = self.parent_pos(from, who, path)?;
                pf_geometry::point_offset(p, dx_mm, dy_mm).map_err(geo)
            }
            PointDefinition::Midpoint { first, second } => {
                if first == second {
                    return Err(CoreError::Invalid(format!(
                        "le point {who} : un milieu exige deux points différents"
                    )));
                }
                let a = self.parent_pos(first, who, path)?;
                let b = self.parent_pos(second, who, path)?;
                Ok(pf_geometry::midpoint(a, b))
            }
            PointDefinition::Projection { point, trace } => {
                let p = self.parent_pos(point, who, path)?;
                let shape = self.trace_shape(trace, who, path)?;
                match shape {
                    Shape::Line(a, b) => pf_geometry::project_on_line(p, a, b).map_err(geo),
                    Shape::Circle(c) => pf_geometry::project_on_circle(p, &c.circle).map_err(geo),
                    Shape::Arc(a) => pf_geometry::project_on_arc(p, &a).map_err(geo),
                    Shape::Cubic(c) => pf_geometry::project_on_cubic(p, &c).ok_or_else(|| {
                        CoreError::Invalid(format!(
                            "le point {who} : aucun pied de perpendiculaire sur la courbe"
                        ))
                    }),
                    Shape::Spline(s) => pf_geometry::project_on_spline(p, &s).map_err(geo),
                }
            }
            PointDefinition::Bisector {
                vertex,
                first,
                second,
                distance_mm: d,
            } => {
                let v = self.parent_pos(vertex, who, path)?;
                let a = self.parent_pos(first, who, path)?;
                let b = self.parent_pos(second, who, path)?;
                pf_geometry::bisector_point(v, a, b, d).map_err(geo)
            }
            PointDefinition::Polar {
                from,
                distance_mm: d,
                angle_deg,
                reference,
            } => {
                let p = self.parent_pos(from, who, path)?;
                let angle = match reference {
                    None => angle_deg,
                    Some(r) => {
                        if !matches!(
                            self.entity(r).map(|e| &e.kind),
                            Ok(EntityKind::Segment { .. })
                        ) {
                            return Err(CoreError::DeadReference(format!(
                                "le point {who} dépend d'un segment de référence absent (id {})",
                                r.0
                            )));
                        }
                        let (a, b) = match self.trace_shape(r, who, path)? {
                            Shape::Line(a, b) => (a, b),
                            _ => unreachable!("segment vérifié ci-dessus"),
                        };
                        pf_geometry::relative_angle_deg(a, b, angle_deg).map_err(geo)?
                    }
                };
                pf_geometry::point_at_direction(p, d, angle).map_err(geo)
            }
            PointDefinition::Compass {
                center_a,
                center_b,
                radius_a_mm,
                radius_b_mm,
                near,
            } => {
                near.point().validate()?;
                let a = self.parent_pos(center_a, who, path)?;
                let b = self.parent_pos(center_b, who, path)?;
                let sols =
                    pf_geometry::compass_solutions(a, radius_a_mm, b, radius_b_mm).map_err(geo)?;
                sols.iter()
                    .copied()
                    .min_by(|p, q| {
                        distance_mm(*p, near.point()).total_cmp(&distance_mm(*q, near.point()))
                    })
                    .ok_or_else(|| {
                        CoreError::Invalid(format!(
                            "le point {who} : les deux arcs de compas ne se coupent pas"
                        ))
                    })
            }
            PointDefinition::Division {
                trace,
                ratio,
                start,
            } => {
                if !ratio.is_finite() || ratio <= 0.0 || ratio >= 1.0 {
                    return Err(CoreError::Invalid(format!(
                        "le point {who} : la fraction de division doit être strictement \
                         comprise entre 0 et 1"
                    )));
                }
                let shape = self.trace_shape(trace, who, path)?;
                match (shape, start) {
                    (Shape::Circle(c), Some(s)) => {
                        let s = self.parent_pos(s, who, path)?;
                        let v = s - c.circle.center;
                        if v.x.hypot(v.y) <= pf_geometry::EPSILON_MM {
                            return Err(CoreError::Invalid(format!(
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
                    (_, Some(_)) => Err(CoreError::Invalid(format!(
                        "le point {who} : un point de départ ne s'applique qu'à un cercle"
                    ))),
                    (shape, None) => shape.point_at_length(ratio * shape.length_mm()?),
                }
            }
            PointDefinition::Mirror { source, axis } => {
                let p = self.parent_pos(source, who, path)?;
                let line = self.mirror_axis_rec(axis, who, path)?;
                line.reflect(p).map_err(geo)
            }
            _ => unreachable!("définition non relative"),
        }
    }

    fn mirror_axis_rec(
        &self,
        axis: MirrorAxis,
        who: &str,
        path: &mut Resolution,
    ) -> Result<AxisLine, CoreError> {
        match axis {
            MirrorAxis::Guide { guide } => {
                let g = self.guide(guide).map_err(|_| {
                    CoreError::DeadReference(format!(
                        "le point {who} dépend d'une ligne d'aide absente (id {})",
                        guide.0
                    ))
                })?;
                Ok(AxisLine {
                    origin: g.origin(),
                    dir: g.direction(),
                })
            }
            MirrorAxis::Segment { segment } => {
                if !matches!(
                    self.entity(segment).map(|e| &e.kind),
                    Ok(EntityKind::Segment { .. })
                ) {
                    return Err(CoreError::DeadReference(format!(
                        "le point {who} dépend d'un segment d'axe absent (id {})",
                        segment.0
                    )));
                }
                let Shape::Line(a, b) = self.trace_shape(segment, who, path)? else {
                    unreachable!("segment vérifié ci-dessus")
                };
                let dir = (b - a).unit().map_err(|_| {
                    CoreError::Invalid(format!("le point {who} : l'axe de symétrie est dégénéré"))
                })?;
                Ok(AxisLine { origin: a, dir })
            }
        }
    }

    /// Droite d'une symétrie liée ; refusée si l'axe est dégénéré ou absent.
    pub fn mirror_axis(&self, axis: MirrorAxis) -> Result<AxisLine, CoreError> {
        self.mirror_axis_rec(axis, "miroir", &mut Resolution::default())
    }
}

/// Droite d'axe d'une symétrie : un point et une direction unitaire.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AxisLine {
    pub origin: PointMm,
    pub dir: VectorMm,
}

impl AxisLine {
    /// Réflexion exacte d'un point (validée : finie, dans l'espace de travail).
    pub fn reflect(&self, p: PointMm) -> Result<PointMm, pf_geometry::GeometryError> {
        let v = p - self.origin;
        (self.origin + self.reflect_vector(v)).validate()
    }

    /// Réflexion d'un vecteur (poignée, tangente).
    pub fn reflect_vector(&self, v: VectorMm) -> VectorMm {
        let k = 2.0 * self.dir.dot(v);
        VectorMm::new(self.dir.x * k - v.x, self.dir.y * k - v.y)
    }

    /// Distance (mm) d'un point à la droite.
    pub fn distance(&self, p: PointMm) -> f64 {
        self.dir.cross(p - self.origin).abs()
    }
}
