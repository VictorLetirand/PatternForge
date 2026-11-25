//! Points relatifs (US-02.5) : décalage dx/dy, milieu, projection orthogonale,
//! bissectrice, direction + distance, compas. Maths pures en mm ; chaque
//! fonction refuse les cas impossibles avec un message, sans résultat partiel.

use crate::{
    arc::{circle_circle_intersections, Arc, Circle},
    distance_mm, point_from_polar, sin_cos_deg, CubicBezier, GeometryError, PointMm, Result,
    Spline, VectorMm, EPSILON_MM,
};

fn bad(msg: impl Into<String>) -> GeometryError {
    GeometryError::InvalidConstruction(msg.into())
}

fn finite(v: f64, what: &'static str) -> Result<f64> {
    if v.is_finite() {
        Ok(v)
    } else {
        Err(GeometryError::NonFinite(what))
    }
}

/// Point décalé de `(dx, dy)` depuis `from`.
pub fn point_offset(from: PointMm, dx_mm: f64, dy_mm: f64) -> Result<PointMm> {
    from.validate()?;
    finite(dx_mm, "dx")?;
    finite(dy_mm, "dy")?;
    PointMm::new(from.x + dx_mm, from.y + dy_mm).validate()
}

/// Point à `distance_mm` (> 0) de `from`, dans la direction `angle_deg`
/// (degrés, anti-horaire, 0° = +X).
pub fn point_at_direction(from: PointMm, distance_mm: f64, angle_deg: f64) -> Result<PointMm> {
    point_from_polar(from, distance_mm, angle_deg)
}

/// Angle absolu (degrés) de la direction de `a` vers `b` plus un angle relatif.
pub fn relative_angle_deg(a: PointMm, b: PointMm, relative_deg: f64) -> Result<f64> {
    finite(relative_deg, "l'angle")?;
    Ok(crate::direction_angle_deg(a, b)? + relative_deg)
}

/// Point sur la bissectrice intérieure de l'angle `first`–`vertex`–`second`, à
/// `distance_mm` (> 0) du sommet. Refuse un point confondu avec le sommet et
/// un angle plat (les deux côtés de la bissectrice seraient aussi valables).
pub fn bisector_point(
    vertex: PointMm,
    first: PointMm,
    second: PointMm,
    distance_mm: f64,
) -> Result<PointMm> {
    vertex.validate()?;
    first.validate()?;
    second.validate()?;
    if !distance_mm.is_finite() || distance_mm <= EPSILON_MM {
        return Err(GeometryError::InvalidLength(distance_mm));
    }
    let (u, v) = match ((first - vertex).unit(), (second - vertex).unit()) {
        (Ok(u), Ok(v)) => (u, v),
        _ => {
            return Err(bad(
                "bissectrice impossible : un des points est confondu avec le sommet",
            ))
        }
    };
    let sum = VectorMm::new(u.x + v.x, u.y + v.y);
    if sum.norm() <= 1e-9 {
        return Err(bad(
            "bissectrice impossible : l'angle est plat (les deux côtés sont opposés)",
        ));
    }
    (vertex + sum.unit()? * distance_mm).validate()
}

/// Pied de la perpendiculaire abaissée de `p` sur la **droite** (AB).
pub fn project_on_line(p: PointMm, a: PointMm, b: PointMm) -> Result<PointMm> {
    p.validate()?;
    let u = (b - a).unit()?;
    (a + u * (p - a).dot(u)).validate()
}

/// Pied de la perpendiculaire sur un cercle : point du cercle le plus proche
/// de `p` (refusé si `p` est au centre : tous les points conviennent).
pub fn project_on_circle(p: PointMm, circle: &Circle) -> Result<PointMm> {
    p.validate()?;
    let v = p - circle.center;
    if v.norm() <= EPSILON_MM {
        return Err(bad(
            "projection impossible : le point est au centre du cercle",
        ));
    }
    (circle.center + v.unit()? * circle.radius_mm).validate()
}

/// Pied de la perpendiculaire sur un arc ; refusé si le pied est hors de l'arc.
pub fn project_on_arc(p: PointMm, arc: &Arc) -> Result<PointMm> {
    let c = Circle::new(arc.center, arc.radius_mm)?;
    let foot = project_on_circle(p, &c)?;
    if !arc.contains_angle_of(foot) {
        return Err(bad(
            "projection impossible : le pied de la perpendiculaire est hors de l'arc",
        ));
    }
    Ok(foot)
}

/// Pied de la perpendiculaire sur une Bézier cubique : point `C(t)` avec
/// `(C(t) − p) · C'(t) = 0`, `t` dans `[0, 1]`, le plus proche de `p`.
/// `None` si aucun pied orthogonal n'existe (le plus proche est une extrémité
/// non orthogonale).
pub fn project_on_cubic(p: PointMm, c: &CubicBezier) -> Option<PointMm> {
    let (t0, _) = c.nearest_t(p);
    let second = |t: f64| -> VectorMm {
        let u = 1.0 - t;
        let (a, b) = ((c.c2 - c.c1, c.c1 - c.p0), (c.p3 - c.c2, c.c2 - c.c1));
        VectorMm::new(
            6.0 * (u * (a.0.x - a.1.x) + t * (b.0.x - b.1.x)),
            6.0 * (u * (a.0.y - a.1.y) + t * (b.0.y - b.1.y)),
        )
    };
    let mut t = t0;
    for _ in 0..20 {
        let d1 = c.derivative_at(t);
        let q = c.point_at(t) - p;
        let f = q.dot(d1);
        let df = d1.dot(d1) + q.dot(second(t));
        if df.abs() < 1e-14 {
            break;
        }
        let next = (t - f / df).clamp(0.0, 1.0);
        if (next - t).abs() < 1e-16 {
            t = next;
            break;
        }
        t = next;
    }
    let d1 = c.derivative_at(t);
    let q = c.point_at(t) - p;
    let n = d1.norm();
    if n <= EPSILON_MM || q.dot(d1).abs() / n > 1e-7 {
        return None;
    }
    Some(c.point_at(t))
}

/// Pied de la perpendiculaire sur une courbe par points de passage : le pied
/// orthogonal le plus proche parmi tous ses tronçons.
pub fn project_on_spline(p: PointMm, s: &Spline) -> Result<PointMm> {
    s.segments()
        .iter()
        .filter_map(|seg| project_on_cubic(p, seg))
        .min_by(|a, b| distance_mm(*a, p).total_cmp(&distance_mm(*b, p)))
        .ok_or_else(|| bad("projection impossible : aucun pied de perpendiculaire sur la courbe"))
}

/// Intersection de deux arcs de compas : cercle de centre `a`, rayon `ra`, et
/// cercle de centre `b`, rayon `rb`. Toutes les solutions (0, 1 ou 2) ;
/// `Err` si les centres sont confondus ou les rayons invalides.
pub fn compass_solutions(a: PointMm, ra: f64, b: PointMm, rb: f64) -> Result<Vec<PointMm>> {
    let (ca, cb) = (Circle::new(a, ra)?, Circle::new(b, rb)?);
    if distance_mm(a, b) <= EPSILON_MM {
        return Err(bad("compas impossible : les deux centres sont confondus"));
    }
    circle_circle_intersections(&ca, &cb)
}

/// Direction unitaire d'angle absolu `angle_deg` (exacte aux multiples de 90°).
pub fn unit_at_deg(angle_deg: f64) -> VectorMm {
    let (s, c) = sin_cos_deg(angle_deg);
    VectorMm::new(c, s)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOL: f64 = 1e-9;
    fn p(x: f64, y: f64) -> PointMm {
        PointMm::new(x, y)
    }

    #[test]
    fn decalage_exact() {
        let q = point_offset(p(10.0, 20.0), -3.5, 4.25).unwrap();
        assert_eq!((q.x, q.y), (6.5, 24.25));
        assert!(point_offset(p(0.0, 0.0), f64::NAN, 0.0).is_err());
        assert!(point_offset(p(0.0, 0.0), 2.0e6, 0.0).is_err());
    }

    #[test]
    fn bissectrice_d_un_angle_droit() {
        let q = bisector_point(p(0.0, 0.0), p(10.0, 0.0), p(0.0, 10.0), 2.0_f64.sqrt()).unwrap();
        assert!(distance_mm(q, p(1.0, 1.0)) < TOL);
    }

    #[test]
    fn bissectrice_refus() {
        let v = p(1.0, 1.0);
        assert!(bisector_point(v, v, p(5.0, 5.0), 1.0).is_err());
        assert!(bisector_point(v, p(0.0, 1.0), p(2.0, 1.0), 1.0).is_err()); // plat
        assert!(bisector_point(v, p(3.0, 1.0), p(1.0, 4.0), 0.0).is_err());
        // angle nul : la bissectrice est la demi-droite commune
        let q = bisector_point(v, p(3.0, 1.0), p(5.0, 1.0), 2.0).unwrap();
        assert!(distance_mm(q, p(3.0, 1.0)) < TOL);
    }

    #[test]
    fn projections() {
        let q = project_on_line(p(3.0, 4.0), p(0.0, 0.0), p(10.0, 0.0)).unwrap();
        assert!(distance_mm(q, p(3.0, 0.0)) < TOL);
        // au-delà du segment : droite prolongée
        let q = project_on_line(p(30.0, 4.0), p(0.0, 0.0), p(10.0, 0.0)).unwrap();
        assert!(distance_mm(q, p(30.0, 0.0)) < TOL);
        let c = Circle::new(p(0.0, 0.0), 5.0).unwrap();
        let q = project_on_circle(p(0.0, 20.0), &c).unwrap();
        assert!(distance_mm(q, p(0.0, 5.0)) < TOL);
        assert!(project_on_circle(p(0.0, 0.0), &c).is_err());
    }

    #[test]
    fn projection_sur_bezier_est_orthogonale() {
        let c = CubicBezier::new(p(0.0, 0.0), p(0.0, 10.0), p(10.0, 10.0), p(10.0, 0.0));
        let target = p(5.0, 20.0);
        let q = project_on_cubic(target, &c).unwrap();
        // symétrie : le pied est le sommet de l'arche (5 ; 7,5)
        assert!(distance_mm(q, p(5.0, 7.5)) < TOL);
        // un point hors de portée orthogonale d'une extrémité est refusé
        let line = CubicBezier::new(p(0.0, 0.0), p(1.0, 0.0), p(2.0, 0.0), p(3.0, 0.0));
        assert!(project_on_cubic(p(-5.0, 1.0), &line).is_none());
    }

    #[test]
    fn compas() {
        let s = compass_solutions(p(0.0, 0.0), 5.0, p(6.0, 0.0), 5.0).unwrap();
        assert_eq!(s.len(), 2);
        assert!(s.iter().any(|q| distance_mm(*q, p(3.0, 4.0)) < TOL));
        assert!(s.iter().any(|q| distance_mm(*q, p(3.0, -4.0)) < TOL));
        assert!(compass_solutions(p(0.0, 0.0), 1.0, p(10.0, 0.0), 1.0)
            .unwrap()
            .is_empty());
        assert!(compass_solutions(p(0.0, 0.0), 1.0, p(0.0, 0.0), 2.0).is_err());
        assert!(compass_solutions(p(0.0, 0.0), -1.0, p(1.0, 0.0), 2.0).is_err());
    }
}
