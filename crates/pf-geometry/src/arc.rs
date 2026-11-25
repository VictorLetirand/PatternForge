//! Cercles et arcs de cercle (US-02.1) : construction, longueur exacte,
//! intersections, découpe en Béziers. Maths pures en mm ; angles en radians
//! en interne, en degrés aux frontières publiques (`*_deg`).

use crate::{
    distance_mm, Bbox, CubicBezier, GeometryError, PointMm, Result, VectorMm, EPSILON_MM,
    MAX_COORD_MM,
};
use std::f64::consts::{PI, TAU};

/// Écart de rayon toléré entre les deux extrémités d'un arc (mm).
pub const ARC_RADIUS_TOLERANCE_MM: f64 = 2.5e-6;
/// Tolérance de tangence des intersections (mm).
pub const TANGENCY_TOLERANCE_MM: f64 = 1e-9;
/// Erreur maximale visée pour la découpe d'un arc en Béziers (mm).
pub const ARC_BEZIER_TOLERANCE_MM: f64 = 0.002;

fn arc_err(msg: &str) -> GeometryError {
    GeometryError::InvalidArc(msg.to_string())
}

/// Vérifie qu'un rayon est fini, > 0 et dans l'espace de travail.
pub fn check_radius(radius_mm: f64) -> Result<f64> {
    if !radius_mm.is_finite() || radius_mm <= EPSILON_MM {
        return Err(GeometryError::InvalidLength(radius_mm));
    }
    if radius_mm > MAX_COORD_MM {
        return Err(GeometryError::OutOfRange(radius_mm));
    }
    Ok(radius_mm)
}

/// Cercle entier : centre et rayon.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Circle {
    pub center: PointMm,
    pub radius_mm: f64,
}

impl Circle {
    pub fn new(center: PointMm, radius_mm: f64) -> Result<Circle> {
        center.validate()?;
        check_radius(radius_mm)?;
        Ok(Circle { center, radius_mm })
    }

    pub fn circumference_mm(&self) -> f64 {
        TAU * self.radius_mm
    }

    pub fn area_mm2(&self) -> f64 {
        PI * self.radius_mm * self.radius_mm
    }

    /// Cercle passant par trois points ; refusé s'ils sont alignés ou confondus.
    pub fn through_three_points(a: PointMm, b: PointMm, c: PointMm) -> Result<Circle> {
        for p in [a, b, c] {
            p.validate()?;
        }
        let (u, v) = (b - a, c - a);
        let cross = u.cross(v);
        let scale = u.norm() * v.norm();
        if scale <= EPSILON_MM || cross.abs() <= 1e-12 * scale {
            return Err(arc_err(
                "les trois points sont alignés ou confondus : aucun cercle ne passe par eux",
            ));
        }
        let (a2, b2, c2) = (
            a.x * a.x + a.y * a.y,
            b.x * b.x + b.y * b.y,
            c.x * c.x + c.y * c.y,
        );
        let d = 2.0 * (a.x * (b.y - c.y) + b.x * (c.y - a.y) + c.x * (a.y - b.y));
        let ux = (a2 * (b.y - c.y) + b2 * (c.y - a.y) + c2 * (a.y - b.y)) / d;
        let uy = (a2 * (c.x - b.x) + b2 * (a.x - c.x) + c2 * (b.x - a.x)) / d;
        let center = PointMm::new(ux, uy);
        Circle::new(center, distance_mm(center, a))
    }

    /// Centres des cercles de rayon donné passant par `a` et `b` : `(gauche, droite)`
    /// par rapport au sens a → b. Refusé si le rayon est inférieur à la demi-corde.
    pub fn centers_through_two_points(
        a: PointMm,
        b: PointMm,
        radius_mm: f64,
    ) -> Result<(PointMm, PointMm)> {
        a.validate()?;
        b.validate()?;
        check_radius(radius_mm)?;
        let chord = distance_mm(a, b);
        if chord <= EPSILON_MM {
            return Err(GeometryError::ZeroLengthSegment);
        }
        let half = chord / 2.0;
        if radius_mm < half - TANGENCY_TOLERANCE_MM {
            return Err(arc_err(
                "le rayon est inférieur à la moitié de la distance entre les deux points",
            ));
        }
        let h = (radius_mm * radius_mm - half * half).max(0.0).sqrt();
        let mid = a.lerp(b, 0.5);
        let n = (b - a).unit()?.rotated_left();
        Ok((mid + n * h, mid + n * -h))
    }

    /// Point du cercle à l'angle donné (degrés, anti-horaire depuis +X).
    pub fn point_at_deg(&self, angle_deg: f64) -> PointMm {
        let (s, c) = crate::sin_cos_deg(angle_deg);
        PointMm::new(
            self.center.x + self.radius_mm * c,
            self.center.y + self.radius_mm * s,
        )
    }

    pub fn bbox(&self) -> Bbox {
        let r = self.radius_mm;
        Bbox {
            min: PointMm::new(self.center.x - r, self.center.y - r),
            max: PointMm::new(self.center.x + r, self.center.y + r),
        }
    }

    pub fn distance_to_point(&self, p: PointMm) -> f64 {
        (distance_mm(self.center, p) - self.radius_mm).abs()
    }
}

/// Arc de cercle : centre, rayon, angle de départ et balayage signé
/// (> 0 anti-horaire, < 0 horaire), `0 < |balayage| < 360°`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Arc {
    pub center: PointMm,
    pub radius_mm: f64,
    /// Angle du point de départ, radians.
    pub start_rad: f64,
    /// Balayage signé, radians.
    pub sweep_rad: f64,
}

impl Arc {
    /// Arc défini par centre, rayon et angles de début/fin en degrés. Le sens
    /// choisit lequel des deux arcs on obtient (anti-horaire par défaut côté interface).
    pub fn from_center_angles(
        center: PointMm,
        radius_mm: f64,
        start_deg: f64,
        end_deg: f64,
        ccw: bool,
    ) -> Result<Arc> {
        center.validate()?;
        check_radius(radius_mm)?;
        if !start_deg.is_finite() || !end_deg.is_finite() {
            return Err(GeometryError::NonFinite("un angle d'arc"));
        }
        let sweep_deg = if ccw {
            (end_deg - start_deg).rem_euclid(360.0)
        } else {
            -(start_deg - end_deg).rem_euclid(360.0)
        };
        if sweep_deg.abs() < 1e-9 || (360.0 - sweep_deg.abs()) < 1e-9 {
            return Err(arc_err(
                "arc dégénéré : angles de début et de fin identiques (utilisez un cercle)",
            ));
        }
        Ok(Arc {
            center,
            radius_mm,
            start_rad: start_deg.rem_euclid(360.0).to_radians(),
            sweep_rad: sweep_deg.to_radians(),
        })
    }

    /// Arc défini par ses points : le rayon est la distance centre → départ ;
    /// l'arrivée doit être sur le même cercle (à 1e-6 mm).
    pub fn from_center_points(
        center: PointMm,
        start: PointMm,
        end: PointMm,
        ccw: bool,
    ) -> Result<Arc> {
        for p in [center, start, end] {
            p.validate()?;
        }
        let (vs, ve) = (start - center, end - center);
        let radius = vs.norm();
        check_radius(radius)?;
        if (ve.norm() - radius).abs() > ARC_RADIUS_TOLERANCE_MM {
            return Err(arc_err(
                "les deux extrémités de l'arc ne sont pas à la même distance du centre",
            ));
        }
        let (a0, a1) = (vs.y.atan2(vs.x), ve.y.atan2(ve.x));
        let sweep = if ccw {
            (a1 - a0).rem_euclid(TAU)
        } else {
            -(a0 - a1).rem_euclid(TAU)
        };
        // Même direction (à ~1e-9 mm près sur l'arc) : arc nul ou tour complet.
        if sweep.abs() * radius <= EPSILON_MM || (TAU - sweep.abs()) * radius <= EPSILON_MM {
            return Err(arc_err(
                "arc dégénéré : les deux extrémités sont confondues (utilisez un cercle)",
            ));
        }
        Ok(Arc {
            center,
            radius_mm: radius,
            start_rad: a0,
            sweep_rad: sweep,
        })
    }

    /// Arc passant par trois points (départ, point intermédiaire, arrivée).
    /// Renvoie l'arc et son centre ; refusé si les points sont alignés.
    pub fn through_three_points(start: PointMm, through: PointMm, end: PointMm) -> Result<Arc> {
        let circle = Circle::through_three_points(start, through, end)?;
        // Sens : celui dans lequel `through` est rencontré entre départ et arrivée.
        let ccw = (through - start).cross(end - start) > 0.0;
        // `end` est recalé radialement : l'arc est exact pour son centre.
        let dir = (end - circle.center).unit()?;
        let end_on = circle.center + dir * circle.radius_mm;
        let start_on = circle.center + (start - circle.center).unit()? * circle.radius_mm;
        Arc::from_center_points(circle.center, start_on, end_on, ccw)
    }

    pub fn start_deg(&self) -> f64 {
        self.start_rad.to_degrees().rem_euclid(360.0)
    }

    pub fn end_deg(&self) -> f64 {
        (self.start_rad + self.sweep_rad)
            .to_degrees()
            .rem_euclid(360.0)
    }

    pub fn sweep_deg(&self) -> f64 {
        self.sweep_rad.to_degrees()
    }

    pub fn is_ccw(&self) -> bool {
        self.sweep_rad > 0.0
    }

    /// Longueur d'arc exacte : `r · |balayage|`.
    pub fn length_mm(&self) -> f64 {
        self.radius_mm * self.sweep_rad.abs()
    }

    fn at_angle(&self, a: f64) -> PointMm {
        PointMm::new(
            self.center.x + self.radius_mm * a.cos(),
            self.center.y + self.radius_mm * a.sin(),
        )
    }

    pub fn start_point(&self) -> PointMm {
        self.at_angle(self.start_rad)
    }

    pub fn end_point(&self) -> PointMm {
        self.at_angle(self.start_rad + self.sweep_rad)
    }

    /// Point à `distance_mm` du départ le long de l'arc, avec la tangente unitaire
    /// dans le sens du parcours. La distance doit être dans `[0, longueur]`.
    pub fn point_at_length(&self, distance_mm: f64) -> Result<(PointMm, VectorMm)> {
        let len = self.length_mm();
        if !distance_mm.is_finite() {
            return Err(GeometryError::NonFinite("la distance"));
        }
        if distance_mm < 0.0 || distance_mm > len + 1e-9 {
            return Err(GeometryError::DistanceOutOfSegment {
                distance_mm,
                length_mm: len,
            });
        }
        let sign = self.sweep_rad.signum();
        let a = self.start_rad + sign * distance_mm.min(len) / self.radius_mm;
        let radial = VectorMm::new(a.cos(), a.sin());
        Ok((self.at_angle(a), radial.rotated_left() * sign))
    }

    /// Fraction `[0, 1]` du balayage atteinte à l'angle de `p` (None si hors de l'arc).
    pub(crate) fn fraction_of(&self, p: PointMm) -> Option<f64> {
        let v = p - self.center;
        if v.norm() <= EPSILON_MM {
            return None;
        }
        let a = v.y.atan2(v.x);
        let rel = if self.sweep_rad > 0.0 {
            (a - self.start_rad).rem_euclid(TAU)
        } else {
            (self.start_rad - a).rem_euclid(TAU)
        };
        let s = self.sweep_rad.abs();
        // Tolérance angulaire équivalente à 1e-9 mm sur l'arc.
        let tol = 1e-9 / self.radius_mm;
        if rel <= s + tol {
            Some((rel / s).min(1.0))
        } else if rel >= TAU - tol {
            Some(0.0)
        } else {
            None
        }
    }

    /// Le point (supposé sur le cercle) est-il sur l'arc ?
    pub fn contains_angle_of(&self, p: PointMm) -> bool {
        self.fraction_of(p).is_some()
    }

    /// Distance, le long de l'arc depuis son départ, du point de l'arc le plus
    /// proche de `p` (extrémité la plus proche si `p` est hors du secteur).
    pub fn nearest_length_mm(&self, p: PointMm) -> f64 {
        match self.fraction_of(p) {
            Some(f) => f * self.length_mm(),
            None => {
                if distance_mm(p, self.start_point()) <= distance_mm(p, self.end_point()) {
                    0.0
                } else {
                    self.length_mm()
                }
            }
        }
    }

    pub fn distance_to_point(&self, p: PointMm) -> f64 {
        match self.fraction_of(p) {
            Some(_) => (distance_mm(self.center, p) - self.radius_mm).abs(),
            None => distance_mm(p, self.start_point()).min(distance_mm(p, self.end_point())),
        }
    }

    /// Arc parcouru dans l'autre sens (mêmes points, sens de parcours inversé).
    pub fn reversed(&self) -> Arc {
        Arc {
            center: self.center,
            radius_mm: self.radius_mm,
            start_rad: self.start_rad + self.sweep_rad,
            sweep_rad: -self.sweep_rad,
        }
    }

    /// Contribution à l'aire signée d'un contour (Green : ½ ∮ x dy − y dx),
    /// par rapport à `origin`, dans le sens de parcours de l'arc. Exacte.
    pub fn signed_area_contribution_mm2(&self, origin: PointMm) -> f64 {
        let q = self.center - origin;
        let (a, b) = (self.start_rad, self.start_rad + self.sweep_rad);
        let r = self.radius_mm;
        0.5 * (r * r * self.sweep_rad + r * (q.x * (b.sin() - a.sin()) + q.y * (a.cos() - b.cos())))
    }

    pub fn bbox(&self) -> Bbox {
        let mut bb = Bbox::from_point(self.start_point());
        bb.include(self.end_point());
        for k in 0..4 {
            let p = self.at_angle(k as f64 * PI / 2.0);
            if self.fraction_of(p).is_some() {
                bb.include(p);
            }
        }
        bb
    }

    /// Polyligne de `n` segments (affichage, tests de proximité).
    pub fn sample(&self, n: usize) -> Vec<PointMm> {
        let n = n.max(1);
        (0..=n)
            .map(|i| self.at_angle(self.start_rad + self.sweep_rad * i as f64 / n as f64))
            .collect()
    }

    /// Béziers cubiques qui approchent l'arc avec une erreur radiale inférieure
    /// à `tolerance_mm` (≤ 90° par morceau, davantage si le rayon l'exige).
    pub fn to_beziers(&self, tolerance_mm: f64) -> Vec<CubicBezier> {
        let tol = if tolerance_mm.is_finite() && tolerance_mm > 0.0 {
            tolerance_mm
        } else {
            ARC_BEZIER_TOLERANCE_MM
        };
        let mut n = (self.sweep_rad.abs() / (PI / 2.0)).ceil().max(1.0) as usize;
        while n < 100_000
            && self.radius_mm * bezier_arc_error(self.sweep_rad.abs() / n as f64) > tol
        {
            n += 1;
        }
        let step = self.sweep_rad / n as f64;
        let k = 4.0 / 3.0 * (step / 4.0).tan();
        (0..n)
            .map(|i| {
                let a0 = self.start_rad + step * i as f64;
                let a1 = a0 + step;
                let (p0, p3) = (self.at_angle(a0), self.at_angle(a1));
                let t0 = VectorMm::new(-a0.sin(), a0.cos()) * (self.radius_mm * k);
                let t1 = VectorMm::new(-a1.sin(), a1.cos()) * (self.radius_mm * k);
                CubicBezier::new(p0, p0 + t0, p3 + t1 * -1.0, p3)
            })
            .collect()
    }
}

/// Cercle entier considéré comme un tracé orienté : il a un point de départ
/// (angle `start_deg`) et un sens de parcours, pour porter des crans et subir
/// rotations et symétries comme n'importe quel tracé.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CircleTrace {
    pub circle: Circle,
    pub start_deg: f64,
    pub ccw: bool,
}

impl CircleTrace {
    pub fn new(center: PointMm, radius_mm: f64, start_deg: f64, ccw: bool) -> Result<CircleTrace> {
        if !start_deg.is_finite() {
            return Err(GeometryError::NonFinite("l'angle de départ du cercle"));
        }
        Ok(CircleTrace {
            circle: Circle::new(center, radius_mm)?,
            // Angle ramené dans [0, 360) : un angle énorme ne doit pas écraser les 180° ajoutés.
            start_deg: start_deg.rem_euclid(360.0),
            ccw,
        })
    }

    pub fn length_mm(&self) -> f64 {
        self.circle.circumference_mm()
    }

    /// Les deux demi-cercles successifs (départ → départ + 180° → départ + 360°).
    pub fn halves(&self) -> [Arc; 2] {
        let c = &self.circle;
        let (s, sweep) = (self.start_deg.to_radians(), if self.ccw { PI } else { -PI });
        let mk = |start_rad: f64| Arc {
            center: c.center,
            radius_mm: c.radius_mm,
            start_rad,
            sweep_rad: sweep,
        };
        [mk(s), mk(s + PI)]
    }

    /// Point à `distance_mm` du départ (0 ≤ d ≤ circonférence), avec la tangente.
    pub fn point_at_length(&self, distance_mm: f64) -> Result<(PointMm, VectorMm)> {
        let len = self.length_mm();
        if !distance_mm.is_finite() {
            return Err(GeometryError::NonFinite("la distance"));
        }
        if distance_mm < 0.0 || distance_mm > len + 1e-9 {
            return Err(GeometryError::DistanceOutOfSegment {
                distance_mm,
                length_mm: len,
            });
        }
        let sign = if self.ccw { 1.0 } else { -1.0 };
        let a = self.start_deg.to_radians() + sign * distance_mm.min(len) / self.circle.radius_mm;
        let radial = VectorMm::new(a.cos(), a.sin());
        Ok((
            self.circle.center + radial * self.circle.radius_mm,
            radial.rotated_left() * sign,
        ))
    }

    /// Distance, le long du parcours depuis le départ, du point du cercle le plus proche de `p`.
    pub fn nearest_length_mm(&self, p: PointMm) -> f64 {
        let v = p - self.circle.center;
        if v.norm() <= EPSILON_MM {
            return 0.0;
        }
        let a = v.y.atan2(v.x);
        let s = self.start_deg.to_radians();
        let rel = if self.ccw {
            (a - s).rem_euclid(TAU)
        } else {
            (s - a).rem_euclid(TAU)
        };
        rel * self.circle.radius_mm
    }

    pub fn sample(&self, n: usize) -> Vec<PointMm> {
        let n = n.max(3);
        let sign = if self.ccw { 1.0 } else { -1.0 };
        let s = self.start_deg.to_radians();
        (0..=n)
            .map(|i| {
                let a = s + sign * TAU * i as f64 / n as f64;
                PointMm::new(
                    self.circle.center.x + self.circle.radius_mm * a.cos(),
                    self.circle.center.y + self.circle.radius_mm * a.sin(),
                )
            })
            .collect()
    }

    /// Aire signée (positive si le parcours est anti-horaire).
    pub fn signed_area_mm2(&self) -> f64 {
        let a = self.circle.area_mm2();
        if self.ccw {
            a
        } else {
            -a
        }
    }
}

/// Erreur radiale relative maximale de l'approximation d'un arc de balayage
/// `phi` par une Bézier cubique de tangentes optimales (forme fermée connue,
/// majorée par prudence d'un facteur 1,5).
fn bezier_arc_error(phi: f64) -> f64 {
    let s = (phi / 4.0).sin();
    let c = (phi / 4.0).cos();
    1.5 * (2.0 / 27.0) * s.powi(6) / (c * c)
}

/// Intersections du cercle avec la **droite** (AB), même au-delà du segment.
/// 0, 1 (tangente à 1e-9 mm près) ou 2 solutions, ordonnées le long de A → B.
pub fn circle_line_intersections(circle: &Circle, a: PointMm, b: PointMm) -> Result<Vec<PointMm>> {
    a.validate()?;
    b.validate()?;
    let u = (b - a).unit()?;
    let r = circle.radius_mm;
    let rel = circle.center - a;
    let along = rel.dot(u);
    let dist = u.cross(rel); // distance signée centre → droite
    let foot = a + u * along;
    if dist.abs() > r + TANGENCY_TOLERANCE_MM {
        return Ok(Vec::new());
    }
    if (dist.abs() - r).abs() <= TANGENCY_TOLERANCE_MM {
        return Ok(vec![foot]);
    }
    let h = (r * r - dist * dist).sqrt();
    Ok(vec![foot + u * -h, foot + u * h])
}

/// Intersections de deux cercles : 0, 1 (tangents, à 1e-9 mm) ou 2 solutions.
/// Cercles confondus : erreur (une infinité de solutions).
pub fn circle_circle_intersections(c1: &Circle, c2: &Circle) -> Result<Vec<PointMm>> {
    let v = c2.center - c1.center;
    let d = v.norm();
    let (r1, r2) = (c1.radius_mm, c2.radius_mm);
    if d <= EPSILON_MM {
        if (r1 - r2).abs() <= TANGENCY_TOLERANCE_MM {
            return Err(arc_err(
                "cercles confondus : une infinité de points d'intersection",
            ));
        }
        return Ok(Vec::new());
    }
    let tol = TANGENCY_TOLERANCE_MM;
    if d > r1 + r2 + tol || d < (r1 - r2).abs() - tol {
        return Ok(Vec::new());
    }
    let u = VectorMm::new(v.x / d, v.y / d);
    let a = (d * d + r1 * r1 - r2 * r2) / (2.0 * d);
    let foot = c1.center + u * a;
    if (d - (r1 + r2)).abs() <= tol || (d - (r1 - r2).abs()).abs() <= tol {
        return Ok(vec![foot]);
    }
    let h = (r1 * r1 - a * a).max(0.0).sqrt();
    let n = u.rotated_left();
    Ok(vec![foot + n * h, foot + n * -h])
}

/// Parmi des solutions, celle la plus proche de `pick` (choix par proximité du clic).
pub fn nearest_solution(solutions: &[PointMm], pick: PointMm) -> Option<PointMm> {
    solutions
        .iter()
        .copied()
        .min_by(|p, q| distance_mm(*p, pick).total_cmp(&distance_mm(*q, pick)))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOL: f64 = 1e-9;

    fn p(x: f64, y: f64) -> PointMm {
        PointMm::new(x, y)
    }

    #[test]
    fn longueur_exacte_quart_demi_et_tour_presque_complet() {
        let quart = Arc::from_center_angles(p(0.0, 0.0), 10.0, 0.0, 90.0, true).unwrap();
        assert!((quart.length_mm() - 10.0 * PI / 2.0).abs() < TOL);
        let demi = Arc::from_center_angles(p(5.0, 5.0), 20.0, 90.0, 270.0, true).unwrap();
        assert!((demi.length_mm() - 20.0 * PI).abs() < TOL);
        // Sens horaire de 0 à 90 : le grand arc (270°).
        let grand = Arc::from_center_angles(p(0.0, 0.0), 10.0, 0.0, 90.0, false).unwrap();
        assert!((grand.length_mm() - 10.0 * 1.5 * PI).abs() < TOL);
        assert!(!grand.is_ccw());
    }

    #[test]
    fn arcs_degeneres_refuses() {
        assert!(Arc::from_center_angles(p(0.0, 0.0), 0.0, 0.0, 90.0, true).is_err());
        assert!(Arc::from_center_angles(p(0.0, 0.0), -1.0, 0.0, 90.0, true).is_err());
        assert!(Arc::from_center_angles(p(0.0, 0.0), f64::NAN, 0.0, 90.0, true).is_err());
        assert!(Arc::from_center_angles(p(0.0, 0.0), f64::INFINITY, 0.0, 90.0, true).is_err());
        assert!(Arc::from_center_angles(p(0.0, 0.0), 5.0, 30.0, 30.0, true).is_err());
        assert!(Arc::from_center_angles(p(0.0, 0.0), 5.0, 0.0, 360.0, true).is_err());
        assert!(Arc::from_center_angles(p(0.0, 0.0), 5.0, f64::NAN, 10.0, true).is_err());
        assert!(Circle::new(p(0.0, 0.0), 0.0).is_err());
        assert!(Circle::new(p(f64::NAN, 0.0), 1.0).is_err());
    }

    #[test]
    fn trois_points_alignes_refuses() {
        assert!(Circle::through_three_points(p(0.0, 0.0), p(1.0, 1.0), p(2.0, 2.0)).is_err());
        assert!(Circle::through_three_points(p(0.0, 0.0), p(0.0, 0.0), p(2.0, 2.0)).is_err());
        assert!(Arc::through_three_points(p(0.0, 0.0), p(5.0, 0.0), p(9.0, 0.0)).is_err());
    }

    #[test]
    fn cercle_par_trois_points() {
        let c = Circle::through_three_points(p(10.0, 0.0), p(0.0, 10.0), p(-10.0, 0.0)).unwrap();
        assert!(distance_mm(c.center, p(0.0, 0.0)) < 1e-9);
        assert!((c.radius_mm - 10.0).abs() < 1e-9);
    }

    #[test]
    fn arc_par_trois_points_sens_et_passage() {
        // (10,0) -> (0,10) -> (-10,0) : anti-horaire, demi-cercle supérieur.
        let a = Arc::through_three_points(p(10.0, 0.0), p(0.0, 10.0), p(-10.0, 0.0)).unwrap();
        assert!(a.is_ccw());
        assert!((a.length_mm() - 10.0 * PI).abs() < 1e-9);
        // Dans l'autre ordre : horaire, même demi-cercle supérieur parcouru à l'envers.
        let b = Arc::through_three_points(p(-10.0, 0.0), p(0.0, 10.0), p(10.0, 0.0)).unwrap();
        assert!(!b.is_ccw());
        let (mid, _) = b.point_at_length(b.length_mm() / 2.0).unwrap();
        assert!(distance_mm(mid, p(0.0, 10.0)) < 1e-9);
    }

    #[test]
    fn deux_points_et_rayon() {
        let (l, r) = Circle::centers_through_two_points(p(-3.0, 0.0), p(3.0, 0.0), 5.0).unwrap();
        assert!(distance_mm(l, p(0.0, 4.0)) < 1e-9);
        assert!(distance_mm(r, p(0.0, -4.0)) < 1e-9);
        // Rayon = demi-corde : un seul centre.
        let (l, r) = Circle::centers_through_two_points(p(-3.0, 0.0), p(3.0, 0.0), 3.0).unwrap();
        assert!(distance_mm(l, r) < 1e-9);
        assert!(Circle::centers_through_two_points(p(-3.0, 0.0), p(3.0, 0.0), 2.9).is_err());
        assert!(Circle::centers_through_two_points(p(1.0, 1.0), p(1.0, 1.0), 2.0).is_err());
    }

    #[test]
    fn extremites_et_point_a_distance() {
        let a = Arc::from_center_angles(p(1.0, 2.0), 10.0, 0.0, 180.0, true).unwrap();
        assert!(distance_mm(a.start_point(), p(11.0, 2.0)) < 1e-9);
        assert!(distance_mm(a.end_point(), p(-9.0, 2.0)) < 1e-9);
        let (m, t) = a.point_at_length(a.length_mm() / 2.0).unwrap();
        assert!(distance_mm(m, p(1.0, 12.0)) < 1e-9);
        assert!((t.x + 1.0).abs() < 1e-9 && t.y.abs() < 1e-9);
        assert!(a.point_at_length(a.length_mm() + 1.0).is_err());
        assert!(a.point_at_length(-1.0).is_err());
        let r = a.reversed();
        assert!(distance_mm(r.start_point(), a.end_point()) < 1e-9);
        assert!((r.length_mm() - a.length_mm()).abs() < TOL);
    }

    #[test]
    fn aire_de_green_demi_disque_et_disque_decale() {
        // Demi-disque : arc ccw de 0 à 180 puis corde de retour. Aire = pi r^2 / 2.
        let r = 10.0;
        let c = p(3.0, -4.0);
        let arc = Arc::from_center_angles(c, r, 0.0, 180.0, true).unwrap();
        let origin = arc.start_point();
        let back = 0.5 * (arc.end_point() - origin).cross(origin - origin);
        let area = arc.signed_area_contribution_mm2(origin) + back;
        assert!((area - PI * r * r / 2.0).abs() < 1e-9);
        // Deux demi-cercles forment un disque, quelle que soit l'origine.
        let a2 = Arc::from_center_angles(c, r, 180.0, 0.0, true).unwrap();
        let o = p(100.0, -50.0);
        let total = arc.signed_area_contribution_mm2(o) + a2.signed_area_contribution_mm2(o);
        assert!((total - PI * r * r).abs() < 1e-8);
        // Sens inverse : aire négative.
        let neg = arc.reversed().signed_area_contribution_mm2(o)
            + a2.reversed().signed_area_contribution_mm2(o);
        assert!((neg + PI * r * r).abs() < 1e-8);
    }

    #[test]
    fn boite_englobante_et_distance() {
        let a = Arc::from_center_angles(p(0.0, 0.0), 10.0, 45.0, 135.0, true).unwrap();
        let bb = a.bbox();
        assert!((bb.max.y - 10.0).abs() < 1e-9);
        assert!((bb.min.y - 10.0 * (45.0_f64).to_radians().sin()).abs() < 1e-9);
        assert!(a.distance_to_point(p(0.0, 13.0)) - 3.0 < 1e-9);
        // Hors secteur : distance à l'extrémité la plus proche.
        let d = a.distance_to_point(p(0.0, -10.0));
        assert!((d - distance_mm(p(0.0, -10.0), a.start_point())).abs() < 1e-9 || d > 10.0);
        assert!(a.nearest_length_mm(p(0.0, 20.0)) - a.length_mm() / 2.0 < 1e-9);
    }

    #[test]
    fn beziers_erreur_inferieure_a_0_005_mm() {
        for (r, start, sweep_ccw) in [
            (1.0, 0.0, true),
            (50.0, 10.0, true),
            (500.0, 0.0, false),
            (5000.0, 33.0, true),
        ] {
            let a =
                Arc::from_center_angles(p(7.0, -3.0), r, start, start + 250.0, sweep_ccw).unwrap();
            let pieces = a.to_beziers(ARC_BEZIER_TOLERANCE_MM);
            let mut worst: f64 = 0.0;
            for b in &pieces {
                for i in 0..=64 {
                    let q = b.point_at(i as f64 / 64.0);
                    worst = worst.max((distance_mm(q, a.center) - r).abs());
                }
            }
            assert!(worst < 0.005, "rayon {r} : erreur {worst}");
            assert!(distance_mm(pieces[0].p0, a.start_point()) < 1e-9);
            assert!(distance_mm(pieces.last().unwrap().p3, a.end_point()) < 1e-9);
        }
    }

    #[test]
    fn cercle_oriente_depart_sens_et_aire() {
        let c = CircleTrace::new(p(1.0, 1.0), 10.0, 90.0, true).unwrap();
        let (q, t) = c.point_at_length(0.0).unwrap();
        assert!(distance_mm(q, p(1.0, 11.0)) < 1e-9);
        assert!((t.x + 1.0).abs() < 1e-9);
        let (q, _) = c.point_at_length(c.length_mm() / 4.0).unwrap();
        assert!(distance_mm(q, p(-9.0, 1.0)) < 1e-9);
        let cw = CircleTrace::new(p(1.0, 1.0), 10.0, 90.0, false).unwrap();
        let (q, _) = cw.point_at_length(cw.length_mm() / 4.0).unwrap();
        assert!(distance_mm(q, p(11.0, 1.0)) < 1e-9);
        assert!((cw.signed_area_mm2() + PI * 100.0).abs() < 1e-9);
        assert!((c.nearest_length_mm(p(-50.0, 1.0)) - c.length_mm() / 4.0).abs() < 1e-9);
        let [h1, h2] = c.halves();
        assert!((h1.length_mm() + h2.length_mm() - c.length_mm()).abs() < 1e-9);
        assert!(c.point_at_length(c.length_mm() + 1.0).is_err());
    }

    #[test]
    fn cercle_droite_zero_une_deux_solutions() {
        let c = Circle::new(p(0.0, 0.0), 5.0).unwrap();
        assert_eq!(
            circle_line_intersections(&c, p(-10.0, 6.0), p(10.0, 6.0))
                .unwrap()
                .len(),
            0
        );
        let tang = circle_line_intersections(&c, p(-10.0, 5.0), p(10.0, 5.0)).unwrap();
        assert_eq!(tang.len(), 1);
        assert!(distance_mm(tang[0], p(0.0, 5.0)) < 1e-9);
        let two = circle_line_intersections(&c, p(-10.0, 3.0), p(10.0, 3.0)).unwrap();
        assert_eq!(two.len(), 2);
        assert!(distance_mm(two[0], p(-4.0, 3.0)) < 1e-9);
        assert!(distance_mm(two[1], p(4.0, 3.0)) < 1e-9);
        // Tangence à 1e-9 près : encore 1 solution ; à 1e-6 : 0 solution.
        assert_eq!(
            circle_line_intersections(&c, p(-10.0, 5.0 + 5e-10), p(10.0, 5.0 + 5e-10))
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            circle_line_intersections(&c, p(-10.0, 5.0 + 1e-6), p(10.0, 5.0 + 1e-6))
                .unwrap()
                .len(),
            0
        );
        assert!(circle_line_intersections(&c, p(1.0, 1.0), p(1.0, 1.0)).is_err());
    }

    #[test]
    fn cercle_cercle_zero_une_deux_solutions() {
        let a = Circle::new(p(0.0, 0.0), 5.0).unwrap();
        // Deux solutions : classique 3-4-5.
        let b = Circle::new(p(6.0, 0.0), 5.0).unwrap();
        let two = circle_circle_intersections(&a, &b).unwrap();
        assert_eq!(two.len(), 2);
        assert!(two.iter().any(|q| distance_mm(*q, p(3.0, 4.0)) < 1e-9));
        assert!(two.iter().any(|q| distance_mm(*q, p(3.0, -4.0)) < 1e-9));
        // Tangents extérieurs, tangents intérieurs.
        let ext = circle_circle_intersections(&a, &Circle::new(p(8.0, 0.0), 3.0).unwrap()).unwrap();
        assert_eq!(ext.len(), 1);
        assert!(distance_mm(ext[0], p(5.0, 0.0)) < 1e-9);
        let int = circle_circle_intersections(&a, &Circle::new(p(2.0, 0.0), 3.0).unwrap()).unwrap();
        assert_eq!(int.len(), 1);
        assert!(distance_mm(int[0], p(5.0, 0.0)) < 1e-9);
        // Aucune : disjoints, ou l'un dans l'autre ; concentriques.
        assert!(
            circle_circle_intersections(&a, &Circle::new(p(20.0, 0.0), 3.0).unwrap())
                .unwrap()
                .is_empty()
        );
        assert!(
            circle_circle_intersections(&a, &Circle::new(p(1.0, 0.0), 1.0).unwrap())
                .unwrap()
                .is_empty()
        );
        assert!(
            circle_circle_intersections(&a, &Circle::new(p(0.0, 0.0), 2.0).unwrap())
                .unwrap()
                .is_empty()
        );
        assert!(circle_circle_intersections(&a, &a).is_err());
    }

    #[test]
    fn choix_par_proximite_du_clic() {
        let a = Circle::new(p(0.0, 0.0), 5.0).unwrap();
        let b = Circle::new(p(6.0, 0.0), 5.0).unwrap();
        let sols = circle_circle_intersections(&a, &b).unwrap();
        let haut = nearest_solution(&sols, p(3.5, 9.0)).unwrap();
        let bas = nearest_solution(&sols, p(2.0, -9.0)).unwrap();
        assert!(distance_mm(haut, p(3.0, 4.0)) < 1e-9);
        assert!(distance_mm(bas, p(3.0, -4.0)) < 1e-9);
        assert!(nearest_solution(&[], p(0.0, 0.0)).is_none());
    }

    #[test]
    fn intersection_de_droite_avec_arc_filtree_par_secteur() {
        let arc = Arc::from_center_angles(p(0.0, 0.0), 5.0, 0.0, 90.0, true).unwrap();
        let circle = Circle::new(arc.center, arc.radius_mm).unwrap();
        let sols = circle_line_intersections(&circle, p(-10.0, 3.0), p(10.0, 3.0)).unwrap();
        let on: Vec<_> = sols.iter().filter(|q| arc.contains_angle_of(**q)).collect();
        assert_eq!(on.len(), 1);
        assert!(distance_mm(*on[0], p(4.0, 3.0)) < 1e-9);
    }
}
