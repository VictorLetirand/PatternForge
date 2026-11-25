//! PatternForge — géométrie pure.
//!
//! Toutes les coordonnées sont des **millimètres en `f64`**. Repère : X vers la
//! droite, Y vers le haut ; les angles sont en degrés, sens trigonométrique
//! (anti-horaire), 0° = +X. Aucune notion de pixel ni de zoom n'existe ici.

pub mod arc;
pub use arc::*;
pub mod curve_fit;
pub mod dart;
pub mod edges;
pub mod flare;
pub mod fold;
pub mod hem;
pub mod intersect;
pub use intersect::*;
pub mod junction;
pub mod offset;
pub use offset::*;
pub mod hatch;
pub mod homography;
pub use homography::Homography;
pub mod pleat;
pub mod polygon;
pub use polygon::{is_simple_polygon, point_in_polygon, Lasso};
pub mod quick;
pub use quick::{RectSpec, RoundedItem, RoundedLayout};
pub mod region;
pub use region::{find_region, OpenGap, Region, RegionEdge, RegionError};
pub mod relative;
pub use relative::*;
pub mod seam;
pub mod shape;
pub use seam::{
    seam_contour, seam_contour_with, vertex_turn, CornerKind, CornerSpec, EdgeMargin, SeamOptions,
    VertexTurn, MAX_CORNER_MM, MAX_SEAM_MM, SEAM_MITER_LIMIT,
};
pub mod slot;
pub mod solve;
pub mod spline;
pub mod split;
pub use spline::*;
pub mod tangent;
pub use tangent::*;

use serde::{Deserialize, Serialize};
use std::fmt;
use std::ops::{Add, Mul, Sub};

/// En dessous de cette longueur (mm), un segment est considéré comme nul.
pub const EPSILON_MM: f64 = 1e-9;
/// Coordonnée absolue maximale acceptée (mm) : 1 km. Au-delà : rejet.
pub const MAX_COORD_MM: f64 = 1.0e6;
/// Tolérance par défaut du calcul de longueur développée (mm).
pub const DEFAULT_LENGTH_TOLERANCE_MM: f64 = 1e-6;

#[derive(Debug, Clone, PartialEq)]
pub enum GeometryError {
    /// Valeur NaN ou infinie.
    NonFinite(&'static str),
    /// Coordonnée hors de l'espace de travail autorisé.
    OutOfRange(f64),
    /// Segment de longueur nulle : direction indéfinie.
    ZeroLengthSegment,
    /// Courbe dont tous les points sont confondus.
    DegenerateCurve,
    /// Longueur négative, nulle ou non finie là où une longueur > 0 est requise.
    InvalidLength(f64),
    /// Distance demandée en dehors du segment.
    DistanceOutOfSegment { distance_mm: f64, length_mm: f64 },
    /// Tolérance de calcul invalide.
    InvalidTolerance(f64),
    /// Droites parallèles ou confondues : pas de point d'intersection unique.
    ParallelLines,
    /// Paramètre hors de l'intervalle autorisé (proportion, position sur courbe).
    ParameterOutOfRange(f64),
    /// Cercle ou arc impossible à construire (message explicatif).
    InvalidArc(String),
    /// Courbe par points de passage impossible à construire (message explicatif).
    InvalidSpline(String),
    /// Deux tracés superposés sur une longueur non nulle : une infinité de solutions.
    OverlappingTraces,
    /// Construction de point relatif impossible (message explicatif).
    InvalidConstruction(String),
}

impl fmt::Display for GeometryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonFinite(what) => write!(f, "valeur non finie (NaN/infini) pour {what}"),
            Self::OutOfRange(v) => write!(
                f,
                "coordonnée {v} mm hors de l'espace de travail (±{MAX_COORD_MM} mm)"
            ),
            Self::ZeroLengthSegment => {
                write!(f, "segment de longueur nulle : direction indéfinie")
            }
            Self::DegenerateCurve => write!(f, "courbe dégénérée : tous ses points sont confondus"),
            Self::InvalidLength(v) => {
                write!(
                    f,
                    "longueur invalide ({v} mm) : une valeur finie > 0 est requise"
                )
            }
            Self::DistanceOutOfSegment {
                distance_mm,
                length_mm,
            } => write!(
                f,
                "distance {distance_mm} mm en dehors du segment (longueur {length_mm} mm)"
            ),
            Self::InvalidTolerance(v) => write!(f, "tolérance de calcul invalide ({v})"),
            Self::ParallelLines => write!(
                f,
                "droites parallèles ou confondues : pas de point d'intersection unique"
            ),
            Self::ParameterOutOfRange(v) => {
                write!(f, "paramètre {v} hors de l'intervalle autorisé (0 à 1)")
            }
            Self::InvalidArc(msg) | Self::InvalidSpline(msg) | Self::InvalidConstruction(msg) => {
                write!(f, "{msg}")
            }
            Self::OverlappingTraces => write!(
                f,
                "les deux tracés se superposent : une infinité de points d'intersection"
            ),
        }
    }
}

impl GeometryError {
    /// Code stable de la variante (US-19.1) : `snake_case`, jamais renommé.
    pub fn code(&self) -> &'static str {
        match self {
            Self::NonFinite(_) => "non_finite",
            Self::OutOfRange(_) => "out_of_range",
            Self::ZeroLengthSegment => "zero_length_segment",
            Self::DegenerateCurve => "degenerate_curve",
            Self::InvalidLength(_) => "invalid_length",
            Self::DistanceOutOfSegment { .. } => "distance_out_of_segment",
            Self::InvalidTolerance(_) => "invalid_tolerance",
            Self::ParallelLines => "parallel_lines",
            Self::ParameterOutOfRange(_) => "parameter_out_of_range",
            Self::InvalidArc(_) => "invalid_arc",
            Self::InvalidSpline(_) => "invalid_spline",
            Self::OverlappingTraces => "overlapping_traces",
            Self::InvalidConstruction(_) => "invalid_construction",
        }
    }

    /// Paramètres typés du message ; les nombres sont écrits comme `Display` les écrit.
    pub fn params(&self) -> Vec<(&'static str, String)> {
        match self {
            Self::NonFinite(what) => vec![("what", (*what).to_string())],
            Self::OutOfRange(v) => {
                vec![("value", v.to_string()), ("max", MAX_COORD_MM.to_string())]
            }
            Self::InvalidLength(v) | Self::InvalidTolerance(v) | Self::ParameterOutOfRange(v) => {
                vec![("value", v.to_string())]
            }
            Self::DistanceOutOfSegment {
                distance_mm,
                length_mm,
            } => vec![
                ("distance", distance_mm.to_string()),
                ("length", length_mm.to_string()),
            ],
            Self::InvalidArc(d) | Self::InvalidSpline(d) | Self::InvalidConstruction(d) => {
                vec![("detail", d.clone())]
            }
            Self::ZeroLengthSegment
            | Self::DegenerateCurve
            | Self::ParallelLines
            | Self::OverlappingTraces => Vec::new(),
        }
    }

    /// Vrai quand un paramètre est un texte français écrit par le cœur.
    pub fn has_free_text(&self) -> bool {
        matches!(
            self,
            Self::NonFinite(_)
                | Self::InvalidArc(_)
                | Self::InvalidSpline(_)
                | Self::InvalidConstruction(_)
        )
    }
}

impl std::error::Error for GeometryError {}

pub type Result<T> = std::result::Result<T, GeometryError>;

/// Point du plan, en millimètres.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PointMm {
    pub x: f64,
    pub y: f64,
}

/// Vecteur du plan, en millimètres.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VectorMm {
    pub x: f64,
    pub y: f64,
}

impl PointMm {
    pub const ORIGIN: PointMm = PointMm { x: 0.0, y: 0.0 };

    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    /// Vérifie que le point est fini et dans l'espace de travail.
    pub fn validate(self) -> Result<Self> {
        for v in [self.x, self.y] {
            if !v.is_finite() {
                return Err(GeometryError::NonFinite("une coordonnée"));
            }
            if v.abs() > MAX_COORD_MM {
                return Err(GeometryError::OutOfRange(v));
            }
        }
        Ok(self)
    }

    pub fn lerp(self, other: PointMm, t: f64) -> PointMm {
        PointMm::new(
            self.x + (other.x - self.x) * t,
            self.y + (other.y - self.y) * t,
        )
    }
}

impl VectorMm {
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
    pub fn norm(self) -> f64 {
        self.x.hypot(self.y)
    }
    pub fn dot(self, o: VectorMm) -> f64 {
        self.x * o.x + self.y * o.y
    }
    pub fn cross(self, o: VectorMm) -> f64 {
        self.x * o.y - self.y * o.x
    }
    /// Vecteur unitaire ; erreur si la norme est nulle.
    pub fn unit(self) -> Result<VectorMm> {
        let n = self.norm();
        if !n.is_finite() {
            return Err(GeometryError::NonFinite("un vecteur"));
        }
        if n <= EPSILON_MM {
            return Err(GeometryError::ZeroLengthSegment);
        }
        Ok(VectorMm::new(self.x / n, self.y / n))
    }
    /// Rotation de +90° (sens anti-horaire) : exacte, sans trigonométrie.
    pub fn rotated_left(self) -> VectorMm {
        VectorMm::new(-self.y, self.x)
    }
}

impl Sub for PointMm {
    type Output = VectorMm;
    fn sub(self, o: PointMm) -> VectorMm {
        VectorMm::new(self.x - o.x, self.y - o.y)
    }
}
impl Add<VectorMm> for PointMm {
    type Output = PointMm;
    fn add(self, v: VectorMm) -> PointMm {
        PointMm::new(self.x + v.x, self.y + v.y)
    }
}
impl Mul<f64> for VectorMm {
    type Output = VectorMm;
    fn mul(self, k: f64) -> VectorMm {
        VectorMm::new(self.x * k, self.y * k)
    }
}

/// Côté d'une perpendiculaire par rapport au sens de la référence A→B.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    /// À gauche en regardant de A vers B (+90°, anti-horaire).
    Left,
    /// À droite en regardant de A vers B (−90°).
    Right,
}

impl Side {
    pub fn flipped(self) -> Side {
        match self {
            Side::Left => Side::Right,
            Side::Right => Side::Left,
        }
    }
}

/// Distance euclidienne en mm (aucune validation : mesure brute).
pub fn distance_mm(a: PointMm, b: PointMm) -> f64 {
    (b - a).norm()
}

/// Longueur d'un segment validé ; erreur si un point est invalide ou si le
/// segment est nul.
pub fn segment_length_mm(a: PointMm, b: PointMm) -> Result<f64> {
    a.validate()?;
    b.validate()?;
    let d = distance_mm(a, b);
    if d <= EPSILON_MM {
        return Err(GeometryError::ZeroLengthSegment);
    }
    Ok(d)
}

/// `(sin, cos)` d'un angle en degrés, **exacts** pour les multiples de 90°.
pub fn sin_cos_deg(angle_deg: f64) -> (f64, f64) {
    let a = angle_deg.rem_euclid(360.0);
    if a == 0.0 {
        (0.0, 1.0)
    } else if a == 90.0 {
        (1.0, 0.0)
    } else if a == 180.0 {
        (0.0, -1.0)
    } else if a == 270.0 {
        (-1.0, 0.0)
    } else {
        a.to_radians().sin_cos()
    }
}

/// Angle de direction de A vers B, en degrés dans `[0, 360)`.
pub fn direction_angle_deg(a: PointMm, b: PointMm) -> Result<f64> {
    segment_length_mm(a, b)?;
    let v = b - a;
    let deg = v.y.atan2(v.x).to_degrees().rem_euclid(360.0);
    // rem_euclid peut renvoyer 360.0 pour un angle négatif infinitésimal.
    Ok(if deg >= 360.0 { 0.0 } else { deg })
}

/// Angle **anti-horaire** de `u` vers `v`, en degrés dans `[0, 360[` (US-06.7).
/// Erreur si un vecteur est nul ou non fini.
pub fn ccw_angle_deg(u: VectorMm, v: VectorMm) -> Result<f64> {
    if !(u.norm().is_finite() && v.norm().is_finite()) {
        return Err(GeometryError::NonFinite("un vecteur"));
    }
    if u.norm() <= EPSILON_MM || v.norm() <= EPSILON_MM {
        return Err(GeometryError::ZeroLengthSegment);
    }
    let deg = u.cross(v).atan2(u.dot(v)).to_degrees().rem_euclid(360.0);
    Ok(if deg >= 360.0 { 0.0 } else { deg })
}

/// Angle d'un texte posé le long d'une direction (`direction_deg`, sens trigonométrique) :
/// ramené dans `]-90, 90]` pour que le texte ne se lise jamais à l'envers (US-06.7).
/// Une direction verticale donne toujours `90` (le texte monte).
pub fn readable_text_angle_deg(direction_deg: f64) -> f64 {
    let mut a = direction_deg.rem_euclid(360.0);
    if a > 180.0 {
        a -= 360.0;
    }
    // `a` est dans ]-180, 180].
    if a > 90.0 {
        a -= 180.0;
    } else if a <= -90.0 {
        a += 180.0;
    }
    a
}

/// Pas du verrouillage d'angle (Maj enfoncée pendant un tracé), en degrés.
pub const ANGLE_LOCK_STEP_DEG: f64 = 15.0;

/// Ramène un angle au multiple de `step_deg` le plus proche, mesuré depuis
/// l'horizontale du repère (0° = +X, sens trigonométrique). Résultat dans
/// `[0, 360)` : −90° donne 270°.
///
/// Règle d'arrondi : l'angle est d'abord normalisé dans `[0, 360)`, puis un
/// demi-pas exact monte vers le multiple supérieur (7,5° → 15° pour un pas de
/// 15° ; 352,5° → 360° soit 0°). Erreur si l'angle n'est pas fini ou si le pas
/// n'est pas un nombre de `]0, 360]`.
pub fn snap_angle_deg(angle_deg: f64, step_deg: f64) -> Result<f64> {
    if !angle_deg.is_finite() {
        return Err(GeometryError::NonFinite("l'angle"));
    }
    if !step_deg.is_finite() || step_deg <= 0.0 || step_deg > 360.0 {
        return Err(GeometryError::InvalidTolerance(step_deg));
    }
    let a = angle_deg.rem_euclid(360.0);
    let snapped = ((a / step_deg) + 0.5).floor() * step_deg;
    Ok(snapped.rem_euclid(360.0))
}

/// Direction de A vers B ramenée au multiple de `step_deg` le plus proche
/// (voir [`snap_angle_deg`]). Erreur si A et B sont confondus ou non finis.
pub fn snap_direction_deg(a: PointMm, b: PointMm, step_deg: f64) -> Result<f64> {
    snap_angle_deg(direction_angle_deg(a, b)?, step_deg)
}

/// Extrémité d'un segment défini par un départ, une longueur (> 0) et un angle.
pub fn point_from_polar(start: PointMm, length_mm: f64, angle_deg: f64) -> Result<PointMm> {
    start.validate()?;
    if !length_mm.is_finite() || length_mm <= EPSILON_MM {
        return Err(GeometryError::InvalidLength(length_mm));
    }
    if !angle_deg.is_finite() {
        return Err(GeometryError::NonFinite("l'angle"));
    }
    let (s, c) = sin_cos_deg(angle_deg);
    PointMm::new(start.x + length_mm * c, start.y + length_mm * s).validate()
}

/// Point situé à `distance_mm` de A sur le segment AB.
///
/// La distance doit être dans `[0, |AB|]` : aucun prolongement silencieux.
pub fn point_on_segment(a: PointMm, b: PointMm, distance_mm: f64) -> Result<PointMm> {
    let len = segment_length_mm(a, b)?;
    if !distance_mm.is_finite() {
        return Err(GeometryError::NonFinite("la distance"));
    }
    if distance_mm < 0.0 || distance_mm > len + EPSILON_MM {
        return Err(GeometryError::DistanceOutOfSegment {
            distance_mm,
            length_mm: len,
        });
    }
    Ok(a.lerp(b, (distance_mm / len).min(1.0)))
}

/// Extrémité d'une perpendiculaire à AB, partant de `from`, de longueur donnée.
pub fn perpendicular_end(
    ref_a: PointMm,
    ref_b: PointMm,
    from: PointMm,
    length_mm: f64,
    side: Side,
) -> Result<PointMm> {
    from.validate()?;
    segment_length_mm(ref_a, ref_b)?;
    if !length_mm.is_finite() || length_mm <= EPSILON_MM {
        return Err(GeometryError::InvalidLength(length_mm));
    }
    let n = (ref_b - ref_a).unit()?.rotated_left();
    let k = match side {
        Side::Left => length_mm,
        Side::Right => -length_mm,
    };
    (from + n * k).validate()
}

/// De quel côté de la droite orientée A→B se trouve `p` (`None` si aligné).
pub fn side_of(a: PointMm, b: PointMm, p: PointMm) -> Option<Side> {
    let c = (b - a).cross(p - a);
    if c > 0.0 {
        Some(Side::Left)
    } else if c < 0.0 {
        Some(Side::Right)
    } else {
        None
    }
}

/// Angle intérieur entre deux vecteurs, en degrés dans `[0, 180]`.
pub fn interior_angle_deg(u: VectorMm, v: VectorMm) -> Result<f64> {
    if u.norm() <= EPSILON_MM || v.norm() <= EPSILON_MM {
        return Err(GeometryError::ZeroLengthSegment);
    }
    if !(u.norm().is_finite() && v.norm().is_finite()) {
        return Err(GeometryError::NonFinite("un vecteur"));
    }
    Ok(u.cross(v).abs().atan2(u.dot(v)).to_degrees())
}

/// Angle intérieur au sommet `vertex`, formé par les points `a` et `c`.
pub fn angle_at_vertex_deg(a: PointMm, vertex: PointMm, c: PointMm) -> Result<f64> {
    a.validate()?;
    vertex.validate()?;
    c.validate()?;
    interior_angle_deg(a - vertex, c - vertex)
}

/// Projection de `p` sur le segment AB : distance depuis A le long du segment
/// (bornée à `[0, |AB|]`) et distance de `p` au segment.
pub fn project_on_segment(p: PointMm, a: PointMm, b: PointMm) -> Result<(f64, f64)> {
    let len = segment_length_mm(a, b)?;
    let u = (b - a).unit()?;
    let along = (p - a).dot(u).clamp(0.0, len);
    let foot = a + u * along;
    Ok((along, distance_mm(p, foot)))
}

/// Milieu de deux points.
pub fn midpoint(a: PointMm, b: PointMm) -> PointMm {
    a.lerp(b, 0.5)
}

/// Intersection des **droites** (AB) et (CD), même au-delà des segments.
/// Erreur si un segment est nul ou si les droites sont parallèles ou confondues.
pub fn line_intersection(a: PointMm, b: PointMm, c: PointMm, d: PointMm) -> Result<PointMm> {
    let (u, v) = (b - a, d - c);
    let (lu, lv) = (segment_length_mm(a, b)?, segment_length_mm(c, d)?);
    let denom = u.cross(v);
    if denom.abs() <= 1e-12 * lu * lv {
        return Err(GeometryError::ParallelLines);
    }
    let t = (c - a).cross(v) / denom;
    (a + u * t).validate()
}

/// Intersection des **segments** [AB] et [CD] : `None` s'ils ne se croisent
/// pas (ou sont parallèles). Les extrémités comptent, à `tol_mm` près.
pub fn segment_intersection(
    a: PointMm,
    b: PointMm,
    c: PointMm,
    d: PointMm,
    tol_mm: f64,
) -> Result<Option<PointMm>> {
    let p = match line_intersection(a, b, c, d) {
        Ok(p) => p,
        Err(GeometryError::ParallelLines) => return Ok(None),
        Err(e) => return Err(e),
    };
    let on_ab = project_on_segment(p, a, b)?.1 <= tol_mm;
    let on_cd = project_on_segment(p, c, d)?.1 <= tol_mm;
    Ok((on_ab && on_cd).then_some(p))
}

/// Transformation rigide du plan : déplacement, rotation ou symétrie axiale.
/// Elle conserve les longueurs ; la symétrie inverse le sens de parcours.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Transform {
    Translate {
        dx_mm: f64,
        dy_mm: f64,
    },
    /// Rotation anti-horaire de `angle_deg` autour de `center`.
    Rotate {
        center: PointMm,
        angle_deg: f64,
    },
    /// Symétrie par rapport à la droite passant par `axis_point` et dirigée par `axis_angle_deg`.
    Mirror {
        axis_point: PointMm,
        axis_angle_deg: f64,
    },
}

impl Transform {
    /// Refuse les valeurs non finies ou hors de l'espace de travail.
    pub fn validate(&self) -> Result<()> {
        match *self {
            Transform::Translate { dx_mm, dy_mm } => {
                if !dx_mm.is_finite() || !dy_mm.is_finite() {
                    return Err(GeometryError::NonFinite("le déplacement"));
                }
            }
            Transform::Rotate { center, angle_deg } => {
                center.validate()?;
                if !angle_deg.is_finite() {
                    return Err(GeometryError::NonFinite("l'angle"));
                }
            }
            Transform::Mirror {
                axis_point,
                axis_angle_deg,
            } => {
                axis_point.validate()?;
                if !axis_angle_deg.is_finite() {
                    return Err(GeometryError::NonFinite("l'angle de l'axe"));
                }
            }
        }
        Ok(())
    }

    /// Image d'un point (validée : finie et dans l'espace de travail).
    pub fn point(&self, p: PointMm) -> Result<PointMm> {
        let q = match *self {
            Transform::Translate { dx_mm, dy_mm } => PointMm::new(p.x + dx_mm, p.y + dy_mm),
            Transform::Rotate { center, .. } => center + self.vector(p - center),
            Transform::Mirror { axis_point, .. } => axis_point + self.vector(p - axis_point),
        };
        q.validate()
    }

    /// Image d'un vecteur (la translation ne le change pas).
    pub fn vector(&self, v: VectorMm) -> VectorMm {
        match *self {
            Transform::Translate { .. } => v,
            Transform::Rotate { angle_deg, .. } => {
                let (s, c) = sin_cos_deg(angle_deg);
                VectorMm::new(c * v.x - s * v.y, s * v.x + c * v.y)
            }
            Transform::Mirror { axis_angle_deg, .. } => {
                let (s, c) = sin_cos_deg(axis_angle_deg);
                let u = VectorMm::new(c, s);
                let k = 2.0 * u.dot(v);
                VectorMm::new(u.x * k - v.x, u.y * k - v.y)
            }
        }
    }
}

/// Boîte englobante alignée sur les axes, en mm.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bbox {
    pub min: PointMm,
    pub max: PointMm,
}

impl Bbox {
    pub fn from_point(p: PointMm) -> Bbox {
        Bbox { min: p, max: p }
    }
    pub fn include(&mut self, p: PointMm) {
        self.min.x = self.min.x.min(p.x);
        self.min.y = self.min.y.min(p.y);
        self.max.x = self.max.x.max(p.x);
        self.max.y = self.max.y.max(p.y);
    }
    pub fn union(&mut self, o: Bbox) {
        self.include(o.min);
        self.include(o.max);
    }
    pub fn width(&self) -> f64 {
        self.max.x - self.min.x
    }
    pub fn height(&self) -> f64 {
        self.max.y - self.min.y
    }
    pub fn contains(&self, p: PointMm) -> bool {
        p.x >= self.min.x && p.x <= self.max.x && p.y >= self.min.y && p.y <= self.max.y
    }
    pub fn contains_bbox(&self, o: &Bbox) -> bool {
        self.contains(o.min) && self.contains(o.max)
    }
}

/// Courbe de Bézier cubique en coordonnées absolues (mm).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CubicBezier {
    pub p0: PointMm,
    pub c1: PointMm,
    pub c2: PointMm,
    pub p3: PointMm,
}

// Quadrature de Gauss-Legendre à 5 points sur [-1, 1].
const GL5: [(f64, f64); 5] = [
    (0.0, 0.568_888_888_888_888_9),
    (-0.538_469_310_105_683, 0.478_628_670_499_366_47),
    (0.538_469_310_105_683, 0.478_628_670_499_366_47),
    (-0.906_179_845_938_664, 0.236_926_885_056_189_08),
    (0.906_179_845_938_664, 0.236_926_885_056_189_08),
];

impl CubicBezier {
    pub fn new(p0: PointMm, c1: PointMm, c2: PointMm, p3: PointMm) -> Self {
        Self { p0, c1, c2, p3 }
    }

    /// Vérifie les quatre points et refuse la courbe réduite à un point.
    pub fn validate(&self) -> Result<()> {
        for p in [self.p0, self.c1, self.c2, self.p3] {
            p.validate()?;
        }
        let spread = distance_mm(self.p0, self.c1)
            + distance_mm(self.c1, self.c2)
            + distance_mm(self.c2, self.p3);
        if spread <= EPSILON_MM {
            return Err(GeometryError::DegenerateCurve);
        }
        Ok(())
    }

    pub fn point_at(&self, t: f64) -> PointMm {
        let u = 1.0 - t;
        let (b0, b1, b2, b3) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
        PointMm::new(
            b0 * self.p0.x + b1 * self.c1.x + b2 * self.c2.x + b3 * self.p3.x,
            b0 * self.p0.y + b1 * self.c1.y + b2 * self.c2.y + b3 * self.p3.y,
        )
    }

    pub fn derivative_at(&self, t: f64) -> VectorMm {
        let u = 1.0 - t;
        let (d0, d1, d2) = (self.c1 - self.p0, self.c2 - self.c1, self.p3 - self.c2);
        VectorMm::new(
            3.0 * (u * u * d0.x + 2.0 * u * t * d1.x + t * t * d2.x),
            3.0 * (u * u * d0.y + 2.0 * u * t * d1.y + t * t * d2.y),
        )
    }

    /// Corde : distance droite entre les deux extrémités (≠ longueur développée).
    pub fn chord_mm(&self) -> f64 {
        distance_mm(self.p0, self.p3)
    }

    fn gl5(&self, t0: f64, t1: f64) -> f64 {
        let half = 0.5 * (t1 - t0);
        let mid = 0.5 * (t0 + t1);
        GL5.iter()
            .map(|(x, w)| w * self.derivative_at(mid + half * x).norm())
            .sum::<f64>()
            * half
    }

    fn length_rec(&self, t0: f64, t1: f64, whole: f64, tol: f64, depth: u32) -> f64 {
        let mid = 0.5 * (t0 + t1);
        let (l, r) = (self.gl5(t0, mid), self.gl5(mid, t1));
        if depth == 0 || (l + r - whole).abs() <= tol {
            l + r
        } else {
            self.length_rec(t0, mid, l, tol * 0.5, depth - 1)
                + self.length_rec(mid, t1, r, tol * 0.5, depth - 1)
        }
    }

    /// Longueur développée (longueur d'arc) en mm, par quadrature adaptative
    /// de la norme de la dérivée. `tolerance_mm` borne l'erreur estimée.
    pub fn length_mm(&self, tolerance_mm: f64) -> Result<f64> {
        self.validate()?;
        if !tolerance_mm.is_finite() || tolerance_mm <= 0.0 {
            return Err(GeometryError::InvalidTolerance(tolerance_mm));
        }
        let whole = self.gl5(0.0, 1.0);
        Ok(self.length_rec(0.0, 1.0, whole, tolerance_mm, 24))
    }

    /// Boîte englobante exacte (extrema de chaque coordonnée).
    pub fn bbox(&self) -> Bbox {
        let mut bb = Bbox::from_point(self.p0);
        bb.include(self.p3);
        for (a, b, c, d) in [
            (self.p0.x, self.c1.x, self.c2.x, self.p3.x),
            (self.p0.y, self.c1.y, self.c2.y, self.p3.y),
        ] {
            // Dérivée / 3 = qa t² + qb t + qc
            let qa = -a + 3.0 * b - 3.0 * c + d;
            let qb = 2.0 * (a - 2.0 * b + c);
            let qc = b - a;
            let mut roots = [f64::NAN; 2];
            if qa.abs() < 1e-12 {
                if qb.abs() > 1e-12 {
                    roots[0] = -qc / qb;
                }
            } else {
                let disc = qb * qb - 4.0 * qa * qc;
                if disc >= 0.0 {
                    let s = disc.sqrt();
                    roots = [(-qb + s) / (2.0 * qa), (-qb - s) / (2.0 * qa)];
                }
            }
            for t in roots {
                if t > 0.0 && t < 1.0 {
                    bb.include(self.point_at(t));
                }
            }
        }
        bb
    }

    /// Polyligne d'approximation à `n` segments (affichage, tests de proximité).
    pub fn sample(&self, n: usize) -> Vec<PointMm> {
        let n = n.max(1);
        (0..=n)
            .map(|i| self.point_at(i as f64 / n as f64))
            .collect()
    }

    /// Distance approchée d'un point à la courbe (échantillonnage fin).
    pub fn distance_to_point(&self, p: PointMm) -> f64 {
        let pts = self.sample(96);
        pts.windows(2)
            .map(|w| match project_on_segment(p, w[0], w[1]) {
                Ok((_, d)) => d,
                Err(_) => distance_mm(p, w[0]),
            })
            .fold(f64::INFINITY, f64::min)
    }

    /// Contribution de la courbe à l'aire signée d'un contour fermé
    /// (théorème de Green : ½ ∫ (x·y′ − y·x′) dt), par rapport à `origin`.
    /// L'intégrande est un polynôme de degré 5 : la quadrature est exacte.
    pub fn signed_area_contribution_mm2(&self, origin: PointMm) -> f64 {
        GL5.iter()
            .map(|(x, w)| {
                let t = 0.5 + 0.5 * x;
                let (p, d) = (self.point_at(t) - origin, self.derivative_at(t));
                w * p.cross(d)
            })
            .sum::<f64>()
            * 0.25
    }

    /// Courbe parcourue dans l'autre sens.
    pub fn reversed(&self) -> CubicBezier {
        CubicBezier::new(self.p3, self.c2, self.c1, self.p0)
    }

    /// Longueur développée de l'arc entre les paramètres 0 et `t`.
    pub fn length_to_t_mm(&self, t: f64, tolerance_mm: f64) -> Result<f64> {
        self.validate()?;
        if !tolerance_mm.is_finite() || tolerance_mm <= 0.0 {
            return Err(GeometryError::InvalidTolerance(tolerance_mm));
        }
        if !t.is_finite() || !(0.0..=1.0).contains(&t) {
            return Err(GeometryError::ParameterOutOfRange(t));
        }
        if t == 0.0 {
            return Ok(0.0);
        }
        let whole = self.gl5(0.0, t);
        Ok(self.length_rec(0.0, t, whole, tolerance_mm, 24))
    }

    /// Paramètre `t` du point situé à `distance_mm` de longueur développée du départ.
    pub fn t_at_length_mm(&self, distance_mm: f64, tolerance_mm: f64) -> Result<f64> {
        let total = self.length_mm(tolerance_mm)?;
        if !distance_mm.is_finite()
            || distance_mm < 0.0
            || distance_mm > total + tolerance_mm.max(1e-9)
        {
            return Err(GeometryError::DistanceOutOfSegment {
                distance_mm,
                length_mm: total,
            });
        }
        let target = distance_mm.min(total);
        let (mut lo, mut hi) = (0.0_f64, 1.0_f64);
        for _ in 0..50 {
            let mid = 0.5 * (lo + hi);
            if self.length_to_t_mm(mid, tolerance_mm)? < target {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        Ok(0.5 * (lo + hi))
    }

    /// Tangente unitaire au paramètre `t` (corde locale si la dérivée s'annule).
    pub fn unit_tangent_at(&self, t: f64) -> Result<VectorMm> {
        let d = self.derivative_at(t);
        if d.norm() > 1e-9 {
            return d.unit();
        }
        let (t0, t1) = ((t - 1e-4).max(0.0), (t + 1e-4).min(1.0));
        (self.point_at(t1) - self.point_at(t0)).unit()
    }

    /// Point de la courbe au paramètre `t`, refusé hors de `[0, 1]`.
    pub fn point_at_checked(&self, t: f64) -> Result<PointMm> {
        if !t.is_finite() || !(0.0..=1.0).contains(&t) {
            return Err(GeometryError::ParameterOutOfRange(t));
        }
        Ok(self.point_at(t))
    }

    /// Paramètre du point de la courbe le plus proche de `p`, et sa distance.
    /// Recherche par échantillonnage puis affinage (section dorée).
    pub fn nearest_t(&self, p: PointMm) -> (f64, f64) {
        const N: usize = 128;
        let dist = |t: f64| distance_mm(self.point_at(t), p);
        let best = (0..=N)
            .map(|i| i as f64 / N as f64)
            .min_by(|a, b| dist(*a).total_cmp(&dist(*b)))
            .unwrap_or(0.0);
        let (mut lo, mut hi) = (
            (best - 1.0 / N as f64).max(0.0),
            (best + 1.0 / N as f64).min(1.0),
        );
        let phi = 0.618_033_988_749_894_9;
        for _ in 0..60 {
            let (m1, m2) = (hi - (hi - lo) * phi, lo + (hi - lo) * phi);
            if dist(m1) < dist(m2) {
                hi = m2;
            } else {
                lo = m1;
            }
        }
        let t = 0.5 * (lo + hi);
        (t, dist(t))
    }

    /// Intersections de la courbe avec le segment [AB] : paramètres `t`
    /// croissants et points. Les contacts tangents (sans traversée) peuvent
    /// ne pas être détectés.
    pub fn intersect_segment(&self, a: PointMm, b: PointMm) -> Result<Vec<(f64, PointMm)>> {
        self.validate()?;
        let len = segment_length_mm(a, b)?;
        let u = (b - a).unit()?;
        let side = |t: f64| u.cross(self.point_at(t) - a);
        const N: usize = 256;
        let mut hits: Vec<(f64, PointMm)> = Vec::new();
        let push = |t: f64, hits: &mut Vec<(f64, PointMm)>| {
            let p = self.point_at(t);
            let along = (p - a).dot(u);
            let fresh = hits.last().is_none_or(|(_, q)| distance_mm(*q, p) > 1e-7);
            if (-1e-9..=len + 1e-9).contains(&along) && fresh {
                hits.push((t, p));
            }
        };
        let mut t0 = 0.0;
        let mut s0 = side(t0);
        if s0 == 0.0 {
            push(t0, &mut hits);
        }
        for i in 1..=N {
            let t1 = i as f64 / N as f64;
            let s1 = side(t1);
            if s1 == 0.0 {
                push(t1, &mut hits);
            } else if s0 != 0.0 && (s0 < 0.0) != (s1 < 0.0) {
                let (mut lo, mut hi, mut slo) = (t0, t1, s0);
                for _ in 0..80 {
                    let mid = 0.5 * (lo + hi);
                    let sm = side(mid);
                    if (sm < 0.0) == (slo < 0.0) {
                        lo = mid;
                        slo = sm;
                    } else {
                        hi = mid;
                    }
                }
                push(0.5 * (lo + hi), &mut hits);
            }
            t0 = t1;
            s0 = s1;
        }
        Ok(hits)
    }
}

/// Aire signée d'un polygone (formule du lacet) : positive si les sommets
/// tournent dans le sens anti-horaire. En mm².
pub fn polygon_signed_area_mm2(vertices: &[PointMm]) -> f64 {
    let n = vertices.len();
    if n < 3 {
        return 0.0;
    }
    let o = vertices[0];
    (0..n)
        .map(|i| {
            let (p, q) = (vertices[i] - o, vertices[(i + 1) % n] - o);
            p.cross(q)
        })
        .sum::<f64>()
        * 0.5
}

#[cfg(test)]
mod tests_arclength {
    use super::*;

    fn p(x: f64, y: f64) -> PointMm {
        PointMm::new(x, y)
    }

    #[test]
    fn longueur_jusqu_a_t_et_retour() {
        let c = CubicBezier::new(p(0., 0.), p(30., 40.), p(70., 40.), p(100., 0.));
        let total = c.length_mm(1e-9).unwrap();
        assert_eq!(c.length_to_t_mm(0.0, 1e-9).unwrap(), 0.0);
        assert!((c.length_to_t_mm(1.0, 1e-9).unwrap() - total).abs() < 1e-7);
        // Symétrie : la moitié de la longueur est au paramètre 0,5.
        let half = c.length_to_t_mm(0.5, 1e-9).unwrap();
        assert!((half - total / 2.0).abs() < 1e-7);
        for d in [0.0, 12.5, total / 2.0, total - 0.001, total] {
            let t = c.t_at_length_mm(d, 1e-9).unwrap();
            let back = c.length_to_t_mm(t, 1e-9).unwrap();
            assert!((back - d).abs() < 1e-6, "{d} -> t {t} -> {back}");
        }
    }

    #[test]
    fn courbe_alignee_distance_egale_abscisse() {
        // Paramétrage non uniforme : le point à 50 mm de longueur est bien à x = 50.
        let c = CubicBezier::new(p(0., 0.), p(10., 0.), p(90., 0.), p(100., 0.));
        let t = c.t_at_length_mm(50.0, 1e-9).unwrap();
        assert!((c.point_at(t).x - 50.0).abs() < 1e-6);
        let t = c.t_at_length_mm(20.0, 1e-9).unwrap();
        assert!((c.point_at(t).x - 20.0).abs() < 1e-6);
    }

    #[test]
    fn distances_et_parametres_invalides_refuses() {
        let c = CubicBezier::new(p(0., 0.), p(30., 40.), p(70., 40.), p(100., 0.));
        assert!(c.t_at_length_mm(-1.0, 1e-6).is_err());
        assert!(c.t_at_length_mm(1.0e6, 1e-6).is_err());
        assert!(c.t_at_length_mm(f64::NAN, 1e-6).is_err());
        assert!(c.length_to_t_mm(1.5, 1e-6).is_err());
        assert!(c.length_to_t_mm(0.5, 0.0).is_err());
    }

    #[test]
    fn tangente_unitaire() {
        let c = CubicBezier::new(p(0., 0.), p(30., 40.), p(70., 40.), p(100., 0.));
        let t = c.unit_tangent_at(0.5).unwrap();
        assert!((t.x - 1.0).abs() < 1e-12 && t.y.abs() < 1e-12);
        let start = c.unit_tangent_at(0.0).unwrap();
        assert!((start.norm() - 1.0).abs() < 1e-12 && start.y > 0.0);
        // Poignée confondue avec l'extrémité : dérivée nulle, repli sur la corde locale.
        let cusp = CubicBezier::new(p(0., 0.), p(0., 0.), p(50., 50.), p(100., 0.));
        let t0 = cusp.unit_tangent_at(0.0).unwrap();
        assert!((t0.norm() - 1.0).abs() < 1e-9);
    }
}

#[cfg(test)]
mod tests_transform {
    use super::*;

    fn p(x: f64, y: f64) -> PointMm {
        PointMm::new(x, y)
    }

    #[test]
    fn rotation_exacte_aux_angles_droits() {
        let t = Transform::Rotate {
            center: p(0., 0.),
            angle_deg: 90.0,
        };
        assert_eq!(t.point(p(120., 0.)).unwrap(), p(0., 120.));
        let t = Transform::Rotate {
            center: p(10., 10.),
            angle_deg: 180.0,
        };
        assert_eq!(t.point(p(20., 15.)).unwrap(), p(0., 5.));
        let t = Transform::Rotate {
            center: p(0., 0.),
            angle_deg: -90.0,
        };
        assert_eq!(t.point(p(0., 80.)).unwrap(), p(80., 0.));
    }

    #[test]
    fn symetries_par_rapport_aux_axes() {
        let horizontal = Transform::Mirror {
            axis_point: p(0., 50.),
            axis_angle_deg: 0.0,
        };
        assert_eq!(horizontal.point(p(30., 80.)).unwrap(), p(30., 20.));
        let vertical = Transform::Mirror {
            axis_point: p(100., 0.),
            axis_angle_deg: 90.0,
        };
        assert_eq!(vertical.point(p(70., 12.)).unwrap(), p(130., 12.));
        let diagonal = Transform::Mirror {
            axis_point: p(0., 0.),
            axis_angle_deg: 45.0,
        };
        let q = diagonal.point(p(10., 0.)).unwrap();
        assert!(distance_mm(q, p(0., 10.)) < 1e-12);
    }

    #[test]
    fn conservent_les_longueurs_et_s_inversent() {
        let (a, b) = (p(13.5, -7.25), p(98.0, 41.0));
        let d = distance_mm(a, b);
        for t in [
            Transform::Translate {
                dx_mm: 33.3,
                dy_mm: -250.0,
            },
            Transform::Rotate {
                center: p(5., 9.),
                angle_deg: 37.0,
            },
            Transform::Mirror {
                axis_point: p(-20., 3.),
                axis_angle_deg: 112.0,
            },
        ] {
            let (ta, tb) = (t.point(a).unwrap(), t.point(b).unwrap());
            assert!((distance_mm(ta, tb) - d).abs() < 1e-9, "{t:?}");
        }
        let r = Transform::Rotate {
            center: p(5., 9.),
            angle_deg: 37.0,
        };
        let back = Transform::Rotate {
            center: p(5., 9.),
            angle_deg: -37.0,
        };
        assert!(distance_mm(back.point(r.point(a).unwrap()).unwrap(), a) < 1e-9);
        let m = Transform::Mirror {
            axis_point: p(-20., 3.),
            axis_angle_deg: 112.0,
        };
        assert!(distance_mm(m.point(m.point(a).unwrap()).unwrap(), a) < 1e-9);
        let tri = [p(0., 0.), p(40., 0.), p(0., 30.)];
        let mirrored: Vec<PointMm> = tri.iter().map(|q| m.point(*q).unwrap()).collect();
        let (s1, s2) = (
            polygon_signed_area_mm2(&tri),
            polygon_signed_area_mm2(&mirrored),
        );
        assert!((s1 + s2).abs() < 1e-9 && s1 > 0.0);
    }

    #[test]
    fn valeurs_invalides_refusees() {
        for t in [
            Transform::Translate {
                dx_mm: f64::NAN,
                dy_mm: 0.0,
            },
            Transform::Rotate {
                center: p(0., 0.),
                angle_deg: f64::INFINITY,
            },
            Transform::Mirror {
                axis_point: p(f64::NAN, 0.),
                axis_angle_deg: 0.0,
            },
        ] {
            assert!(t.validate().is_err(), "{t:?}");
        }
        let far = Transform::Translate {
            dx_mm: 2.0e6,
            dy_mm: 0.0,
        };
        assert!(far.point(p(0., 0.)).is_err());
    }

    #[test]
    fn vecteur_de_poignee_suit_la_rotation() {
        let t = Transform::Rotate {
            center: p(0., 0.),
            angle_deg: 90.0,
        };
        assert_eq!(t.vector(VectorMm::new(30., 0.)), VectorMm::new(0., 30.));
        let tr = Transform::Translate {
            dx_mm: 5.0,
            dy_mm: 5.0,
        };
        assert_eq!(tr.vector(VectorMm::new(30., 0.)), VectorMm::new(30., 0.));
    }
}

#[cfg(test)]
mod tests_intersections {
    use super::*;

    fn p(x: f64, y: f64) -> PointMm {
        PointMm::new(x, y)
    }

    #[test]
    fn droites_et_segments() {
        let x = line_intersection(p(0., 0.), p(100., 0.), p(40., -30.), p(40., 30.)).unwrap();
        assert_eq!(x, p(40., 0.));
        // Droites qui se croisent hors des segments.
        let far = line_intersection(p(0., 0.), p(10., 0.), p(40., 5.), p(40., 30.)).unwrap();
        assert_eq!(far, p(40., 0.));
        let none = segment_intersection(p(0., 0.), p(10., 0.), p(40., 5.), p(40., 30.), 1e-9);
        assert_eq!(none, Ok(None));
        let some = segment_intersection(p(0., 0.), p(100., 100.), p(0., 100.), p(100., 0.), 1e-9);
        assert_eq!(some, Ok(Some(p(50., 50.))));
        // Contact à une extrémité : compté.
        let touch = segment_intersection(p(0., 0.), p(50., 0.), p(50., 0.), p(50., 40.), 1e-9);
        assert_eq!(touch, Ok(Some(p(50., 0.))));
    }

    #[test]
    fn paralleles_confondues_et_degeneres() {
        let (a, b) = (p(0., 0.), p(100., 0.));
        assert_eq!(
            line_intersection(a, b, p(0., 5.), p(100., 5.)),
            Err(GeometryError::ParallelLines)
        );
        assert_eq!(
            line_intersection(a, b, p(20., 0.), p(60., 0.)),
            Err(GeometryError::ParallelLines)
        );
        assert_eq!(
            segment_intersection(a, b, p(0., 5.), p(100., 5.), 1e-9),
            Ok(None)
        );
        assert!(line_intersection(a, a, a, b).is_err());
        assert!(segment_intersection(a, b, b, b, 1e-9).is_err());
        assert!(line_intersection(a, b, p(f64::NAN, 0.), p(1., 1.)).is_err());
        // Presque parallèles mais pas tout à fait : un point lointain valide ou une erreur
        // d'espace de travail, jamais un résultat non fini.
        match line_intersection(a, b, p(0., 1.), p(100., 1.000_001)) {
            Ok(q) => assert!(q.x.is_finite() && q.y.is_finite()),
            Err(e) => assert!(matches!(
                e,
                GeometryError::OutOfRange(_) | GeometryError::ParallelLines
            )),
        }
    }

    #[test]
    fn intersection_symetrique_et_invariante_par_translation() {
        let (a, b, c, d) = (p(3., 7.), p(91., 44.), p(12., 80.), p(70., -15.));
        let x1 = line_intersection(a, b, c, d).unwrap();
        let x2 = line_intersection(c, d, a, b).unwrap();
        let x3 = line_intersection(b, a, d, c).unwrap();
        for q in [x2, x3] {
            assert!(distance_mm(x1, q) < 1e-9);
        }
        let t = VectorMm::new(5_000.25, -3_210.5);
        let moved = line_intersection(a + t, b + t, c + t, d + t).unwrap();
        assert!(distance_mm(moved, x1 + t) < 1e-8);
        // Le point trouvé est bien sur les deux droites.
        assert!((b - a).cross(x1 - a).abs() < 1e-8 && (d - c).cross(x1 - c).abs() < 1e-8);
    }

    #[test]
    fn milieu() {
        assert_eq!(midpoint(p(0., 0.), p(120., 80.)), p(60., 40.));
    }

    #[test]
    fn courbe_point_le_plus_proche() {
        let c = CubicBezier::new(p(0., 0.), p(30., 40.), p(70., 40.), p(100., 0.));
        let (t, d) = c.nearest_t(p(50., 30.));
        assert!((t - 0.5).abs() < 1e-6 && d < 1e-9, "t = {t}, d = {d}");
        let (t, d) = c.nearest_t(p(-20., -5.));
        assert!(t < 1e-6 && (d - distance_mm(p(-20., -5.), c.p0)).abs() < 1e-6);
        let (t, _) = c.nearest_t(p(140., 10.));
        assert!((t - 1.0).abs() < 1e-6);
        assert!(c.point_at_checked(1.5).is_err() && c.point_at_checked(f64::NAN).is_err());
        assert_eq!(c.point_at_checked(0.0).unwrap(), c.p0);
    }

    #[test]
    fn courbe_coupee_par_un_segment() {
        let c = CubicBezier::new(p(0., 0.), p(30., 40.), p(70., 40.), p(100., 0.));
        // Horizontale y = 15 : deux intersections symétriques.
        let hits = c.intersect_segment(p(-10., 15.), p(110., 15.)).unwrap();
        assert_eq!(hits.len(), 2, "{hits:?}");
        for (t, q) in &hits {
            assert!((q.y - 15.0).abs() < 1e-9);
            assert!(distance_mm(c.point_at(*t), *q) < 1e-12);
        }
        assert!((hits[0].1.x + hits[1].1.x - 100.0).abs() < 1e-8);
        // Segment trop court pour atteindre la courbe, puis au-dessus du sommet.
        assert!(c
            .intersect_segment(p(40., 15.), p(60., 15.))
            .unwrap()
            .is_empty());
        assert!(c
            .intersect_segment(p(-10., 31.), p(110., 31.))
            .unwrap()
            .is_empty());
        // Verticale par le sommet : une intersection en (50, 30).
        let top = c.intersect_segment(p(50., -5.), p(50., 60.)).unwrap();
        assert_eq!(top.len(), 1);
        assert!(distance_mm(top[0].1, p(50., 30.)) < 1e-9);
        assert!(c.intersect_segment(p(1., 1.), p(1., 1.)).is_err());
    }

    #[test]
    fn aire_sous_une_courbe_exacte() {
        // Contour : courbe de (0,0) à (100,0) par le haut, refermée par la corde.
        let c = CubicBezier::new(p(0., 0.), p(30., 40.), p(70., 40.), p(100., 0.));
        let o = p(0., 0.);
        // Parcours courbe (vers la droite, par le haut) puis retour le long de l'axe :
        // sens horaire, donc aire négative. La corde sur l'axe ne contribue pas.
        let area = c.signed_area_contribution_mm2(o);
        // Référence indépendante : polygone à 200 000 sommets.
        let poly = polygon_signed_area_mm2(&c.sample(200_000));
        assert!((area - poly).abs() < 1e-6, "{area} vs {poly}");
        // Valeur exacte : ∫ y dx avec x = 90t + 30t² − 20t³ et y = 120t(1 − t) → 2040 mm².
        assert!(
            (area + 2040.0).abs() < 1e-9,
            "aire attendue -2040 mm², obtenu {area}"
        );
        // Sens inverse : signe opposé ; origine quelconque : même aire une fois refermé.
        assert!((c.reversed().signed_area_contribution_mm2(o) + area).abs() < 1e-9);
        let far = p(-5000., 7000.);
        let closing = 0.5 * (c.p3 - far).cross(c.p0 - c.p3);
        assert!((c.signed_area_contribution_mm2(far) + closing - area).abs() < 1e-6);
    }

    #[test]
    fn aire_de_polygones() {
        let rect = [p(0., 0.), p(600., 0.), p(600., 450.), p(0., 450.)];
        assert_eq!(polygon_signed_area_mm2(&rect), 270_000.0);
        let mut cw = rect;
        cw.reverse();
        assert_eq!(polygon_signed_area_mm2(&cw), -270_000.0);
        let tri = [
            p(10_000., 10_000.),
            p(10_120., 10_000.),
            p(10_120., 10_080.),
        ];
        assert!((polygon_signed_area_mm2(&tri) - 4_800.0).abs() < 1e-6);
        assert_eq!(polygon_signed_area_mm2(&rect[..2]), 0.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: f64, y: f64) -> PointMm {
        PointMm::new(x, y)
    }
    fn close(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol
    }

    #[test]
    fn longueurs_de_reference() {
        assert_eq!(segment_length_mm(p(0., 0.), p(120., 0.)).unwrap(), 120.0);
        assert_eq!(segment_length_mm(p(120., 0.), p(120., 80.)).unwrap(), 80.0);
        let d = segment_length_mm(p(0., 0.), p(100., 100.)).unwrap();
        assert!(close(d, 141.421_356_237_3, 1e-9));
    }

    #[test]
    fn at01_segment_120_puis_perpendiculaire_80() {
        let a = p(0., 0.);
        let b = point_from_polar(a, 120.0, 0.0).unwrap();
        assert_eq!(b, p(120., 0.));
        let c = perpendicular_end(a, b, b, 80.0, Side::Left).unwrap();
        assert_eq!(c, p(120., 80.));
        let c2 = perpendicular_end(a, b, b, 80.0, Side::Right).unwrap();
        assert_eq!(c2, p(120., -80.));
        assert_eq!(angle_at_vertex_deg(a, b, c).unwrap(), 90.0);
        assert_eq!(distance_mm(b, c), 80.0);
    }

    #[test]
    fn perpendiculaire_sur_reference_oblique() {
        let (a, b) = (p(10., 20.), p(47.3, 91.7));
        for side in [Side::Left, Side::Right] {
            let c = perpendicular_end(a, b, b, 80.0, side).unwrap();
            assert!(close(distance_mm(b, c), 80.0, 1e-9));
            assert!(close(angle_at_vertex_deg(a, b, c).unwrap(), 90.0, 1e-9));
            assert_eq!(side_of(a, b, c), Some(side));
        }
    }

    #[test]
    fn polaire_angles_exacts_et_quelconques() {
        let o = p(0., 0.);
        assert_eq!(point_from_polar(o, 80.0, 90.0).unwrap(), p(0., 80.));
        assert_eq!(point_from_polar(o, 80.0, 180.0).unwrap(), p(-80., 0.));
        assert_eq!(point_from_polar(o, 80.0, 270.0).unwrap(), p(0., -80.));
        assert_eq!(point_from_polar(o, 80.0, -90.0).unwrap(), p(0., -80.));
        let q = point_from_polar(o, 100.0, 37.0).unwrap();
        assert!(close(distance_mm(o, q), 100.0, 1e-9));
        assert!(close(direction_angle_deg(o, q).unwrap(), 37.0, 1e-9));
    }

    #[test]
    fn at02_point_a_42_sur_120() {
        let (a, b) = (p(0., 0.), p(120., 0.));
        let pt = point_on_segment(a, b, 42.0).unwrap();
        assert_eq!(pt, p(42., 0.));
        assert_eq!(distance_mm(a, pt), 42.0);
        let (along, dist) = project_on_segment(pt, a, b).unwrap();
        assert!(close(along, 42.0, 1e-12) && close(dist, 0.0, 1e-12));
        assert_eq!(point_on_segment(a, b, 0.0).unwrap(), a);
        assert_eq!(point_on_segment(a, b, 120.0).unwrap(), b);
    }

    #[test]
    fn at03_distance_hors_segment_rejetee() {
        let (a, b) = (p(0., 0.), p(120., 0.));
        assert_eq!(
            point_on_segment(a, b, 130.0),
            Err(GeometryError::DistanceOutOfSegment {
                distance_mm: 130.0,
                length_mm: 120.0
            })
        );
        assert!(point_on_segment(a, b, -1.0).is_err());
        assert!(point_on_segment(a, b, f64::NAN).is_err());
    }

    #[test]
    fn at10_degeneres_rejetes_sans_panique() {
        let a = p(5., 5.);
        assert_eq!(
            segment_length_mm(a, a),
            Err(GeometryError::ZeroLengthSegment)
        );
        assert!(direction_angle_deg(a, a).is_err());
        assert!(point_on_segment(a, a, 0.0).is_err());
        assert!(perpendicular_end(a, a, a, 10.0, Side::Left).is_err());
        assert!(interior_angle_deg(VectorMm::new(0., 0.), VectorMm::new(1., 0.)).is_err());
        for bad in [f64::NAN, f64::INFINITY, -f64::INFINITY] {
            assert!(p(bad, 0.).validate().is_err());
            assert!(segment_length_mm(p(bad, 0.), a).is_err());
            assert!(point_from_polar(a, bad, 0.0).is_err());
            assert!(point_from_polar(a, 10.0, bad).is_err());
        }
        assert!(point_from_polar(a, 0.0, 0.0).is_err());
        assert!(point_from_polar(a, -5.0, 0.0).is_err());
        assert!(p(2.0e6, 0.).validate().is_err());
        let dot = CubicBezier::new(a, a, a, a);
        assert_eq!(dot.length_mm(1e-6), Err(GeometryError::DegenerateCurve));
        let ok = CubicBezier::new(p(0., 0.), p(30., 0.), p(70., 0.), p(100., 0.));
        assert!(ok.length_mm(0.0).is_err());
        assert!(ok.length_mm(f64::NAN).is_err());
    }

    #[test]
    fn at06_angle_droit_et_bornes() {
        let u = VectorMm::new(120., 0.);
        assert_eq!(interior_angle_deg(u, VectorMm::new(0., 80.)).unwrap(), 90.0);
        assert_eq!(
            interior_angle_deg(u, VectorMm::new(0., -80.)).unwrap(),
            90.0
        );
        assert_eq!(interior_angle_deg(u, VectorMm::new(5., 0.)).unwrap(), 0.0);
        assert_eq!(
            interior_angle_deg(u, VectorMm::new(-5., 0.)).unwrap(),
            180.0
        );
        assert!(close(
            interior_angle_deg(u, VectorMm::new(1., 1.)).unwrap(),
            45.0,
            1e-12
        ));
    }

    #[test]
    fn at04_bezier_colineaire_100() {
        let c = CubicBezier::new(p(0., 0.), p(30., 0.), p(70., 0.), p(100., 0.));
        let len = c.length_mm(DEFAULT_LENGTH_TOLERANCE_MM).unwrap();
        assert!(close(len, 100.0, 1e-6), "longueur = {len}");
        assert_eq!(c.chord_mm(), 100.0);
    }

    #[test]
    fn at05_bezier_courbee_plus_longue_que_la_corde() {
        let c = CubicBezier::new(p(0., 0.), p(30., 40.), p(70., 40.), p(100., 0.));
        let len = c.length_mm(DEFAULT_LENGTH_TOLERANCE_MM).unwrap();
        assert!(len > c.chord_mm() + 1.0, "longueur = {len}");
        // Référence indépendante : polyligne très fine (borne inférieure serrée).
        let poly: f64 = c
            .sample(200_000)
            .windows(2)
            .map(|w| distance_mm(w[0], w[1]))
            .sum();
        assert!(
            close(len, poly, 1e-6),
            "quadrature {len} vs polyligne {poly}"
        );
    }

    #[test]
    fn bezier_quart_de_cercle_approche() {
        // Approximation classique d'un quart de cercle de rayon 100 mm.
        let k = 100.0 * 0.552_284_749_830_793_4;
        let c = CubicBezier::new(p(100., 0.), p(100., k), p(k, 100.), p(0., 100.));
        let len = c.length_mm(1e-9).unwrap();
        // Longueur exacte du cercle : 157,0796 ; l'approximation Bézier vaut 157,09 env.
        assert!(
            close(len, 50.0 * std::f64::consts::PI, 0.05),
            "longueur = {len}"
        );
    }

    #[test]
    fn bezier_avec_rebroussement_reste_precise() {
        let c = CubicBezier::new(p(0., 0.), p(100., 0.), p(0., 0.), p(100., 0.));
        let len = c.length_mm(1e-6).unwrap();
        let poly: f64 = c
            .sample(400_000)
            .windows(2)
            .map(|w| distance_mm(w[0], w[1]))
            .sum();
        assert!(
            close(len, poly, 1e-3),
            "quadrature {len} vs polyligne {poly}"
        );
    }

    #[test]
    fn bbox_bezier_contient_la_courbe() {
        let c = CubicBezier::new(p(0., 0.), p(30., 40.), p(70., 40.), p(100., 0.));
        let bb = c.bbox();
        assert!(close(bb.max.y, 30.0, 1e-9));
        assert_eq!((bb.min.x, bb.min.y, bb.max.x), (0.0, 0.0, 100.0));
        for q in c.sample(500) {
            assert!(q.y <= bb.max.y + 1e-9);
        }
    }

    #[test]
    fn mesures_independantes_d_une_translation() {
        // Garde-fou : la longueur ne dépend pas de la position dans le plan.
        let c = CubicBezier::new(p(0., 0.), p(30., 40.), p(70., 40.), p(100., 0.));
        let t = VectorMm::new(12_345.678, -9_876.5);
        let c2 = CubicBezier::new(c.p0 + t, c.c1 + t, c.c2 + t, c.p3 + t);
        assert!(close(
            c.length_mm(1e-9).unwrap(),
            c2.length_mm(1e-9).unwrap(),
            1e-7
        ));
    }

    #[test]
    fn verrou_d_angle_valeurs_de_reference() {
        let s = |a: f64| snap_angle_deg(a, ANGLE_LOCK_STEP_DEG).unwrap();
        assert_eq!(s(0.0), 0.0);
        assert_eq!(s(7.4), 0.0);
        assert_eq!(s(7.5), 15.0, "demi-pas exact : vers le haut");
        assert_eq!(s(7.6), 15.0);
        assert_eq!(s(22.5), 30.0);
        assert_eq!(s(90.0), 90.0);
        assert_eq!(s(180.0), 180.0);
        assert_eq!(s(-90.0), 270.0, "normalisé dans [0, 360)");
        assert_eq!(s(450.0), 90.0);
        assert_eq!(s(352.4), 345.0);
        assert_eq!(s(352.5), 0.0, "360° se replie sur 0°");
        assert_eq!(s(359.9), 0.0);
        assert_eq!(s(-7.4), 0.0);
        assert_eq!(snap_angle_deg(50.0, 45.0).unwrap(), 45.0);
    }

    #[test]
    fn verrou_d_angle_refuse_les_entrees_invalides() {
        for a in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(matches!(
                snap_angle_deg(a, 15.0),
                Err(GeometryError::NonFinite(_))
            ));
        }
        for step in [0.0, -15.0, f64::NAN, f64::INFINITY, 400.0] {
            assert!(snap_angle_deg(10.0, step).is_err(), "pas {step}");
        }
        // Très grand angle fini : aucun panic, résultat dans [0, 360).
        let big = snap_angle_deg(1e300, 15.0).unwrap();
        assert!((0.0..360.0).contains(&big));
    }

    #[test]
    fn verrou_de_direction_entre_deux_points() {
        let o = p(0., 0.);
        assert_eq!(snap_direction_deg(o, p(100., 5.), 15.0).unwrap(), 0.0);
        assert_eq!(snap_direction_deg(o, p(0., -50.), 15.0).unwrap(), 270.0);
        // 100 / 60 : atan = 59,04° -> 60°.
        assert_eq!(snap_direction_deg(o, p(60., 100.), 15.0).unwrap(), 60.0);
        assert!(snap_direction_deg(o, o, 15.0).is_err(), "vecteur nul");
        assert!(snap_direction_deg(o, p(f64::NAN, 1.0), 15.0).is_err());
    }

    #[test]
    fn angle_anti_horaire_de_u_vers_v() {
        let (x, y) = (VectorMm::new(1., 0.), VectorMm::new(0., 1.));
        assert!((ccw_angle_deg(x, y).unwrap() - 90.0).abs() < 1e-12);
        assert!((ccw_angle_deg(y, x).unwrap() - 270.0).abs() < 1e-12);
        assert!((ccw_angle_deg(x, VectorMm::new(-1., 0.)).unwrap() - 180.0).abs() < 1e-12);
        assert_eq!(ccw_angle_deg(x, VectorMm::new(5., 0.)).unwrap(), 0.0);
        let a = ccw_angle_deg(x, VectorMm::new(1., -1.)).unwrap();
        assert!((a - 315.0).abs() < 1e-12, "{a}");
        assert!(ccw_angle_deg(x, VectorMm::new(0., 0.)).is_err());
        assert!(ccw_angle_deg(VectorMm::new(f64::NAN, 0.), y).is_err());
    }

    #[test]
    fn angle_de_texte_lisible_dans_les_quatre_quadrants() {
        let cases = [
            (0.0, 0.0),
            (30.0, 30.0),
            (90.0, 90.0),
            (120.0, -60.0),
            (180.0, 0.0),
            (200.0, 20.0),
            (270.0, 90.0),
            (300.0, -60.0),
            (-90.0, 90.0),
            (-30.0, -30.0),
            (450.0, 90.0),
            (-180.0, 0.0),
        ];
        for (dir, expected) in cases {
            let a = readable_text_angle_deg(dir);
            assert!(
                (a - expected).abs() < 1e-9,
                "{dir} -> {a} (attendu {expected})"
            );
        }
        // Toute direction donne un angle dans ]-90 ; 90].
        let mut d = -720.0;
        while d <= 720.0 {
            let a = readable_text_angle_deg(d);
            assert!(a > -90.0 && a <= 90.0, "{d} -> {a}");
            d += 0.37;
        }
    }
}
