//! Tangence et raccords (US-02.3) : droites tangentes à un cercle depuis un
//! point, congé (raccord de rayon donné) entre deux tracés (segment, arc,
//! cercle) et chanfrein entre deux segments. Maths pures en mm.

use crate::{
    check_radius, circle_circle_intersections, circle_line_intersections, distance_mm,
    line_intersection, Arc, Circle, GeometryError, PointMm, Result, VectorMm, EPSILON_MM,
    TANGENCY_TOLERANCE_MM,
};
use std::f64::consts::TAU;

/// Longueur minimale (mm) qui doit rester d'un tracé après raccord.
pub const FILLET_MIN_KEPT_MM: f64 = 1e-6;

fn err(msg: &str) -> GeometryError {
    GeometryError::InvalidArc(msg.to_string())
}

/// Points de tangence des droites issues de `p` vers le cercle : 2 solutions
/// (point extérieur), 1 (le point est sur le cercle : `p` lui-même), refus si
/// le point est à l'intérieur. Ordre : gauche puis droite du sens `p` → centre.
pub fn tangent_points_from_point(circle: &Circle, p: PointMm) -> Result<Vec<PointMm>> {
    p.validate()?;
    let r = circle.radius_mm;
    let v = p - circle.center;
    let d = v.norm();
    if (d - r).abs() <= TANGENCY_TOLERANCE_MM {
        return Ok(vec![p]);
    }
    if d < r {
        return Err(err(
            "le point est à l'intérieur du cercle : aucune tangente ne passe par lui",
        ));
    }
    let u = VectorMm::new(v.x / d, v.y / d);
    let n = u.rotated_left();
    let foot = circle.center + u * (r * r / d);
    let h = r * (d * d - r * r).max(0.0).sqrt() / d;
    Ok(vec![foot + n * h, foot + n * -h])
}

/// Chemin raccordable : segment, arc ou cercle entier.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FilletPath {
    Segment { a: PointMm, b: PointMm },
    Arc(Arc),
    Circle(Circle),
}

/// Extrémité d'un tracé remplacée par le point de tangence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrimEnd {
    Start,
    End,
}

/// Coupe d'un tracé : l'extrémité `end` est ramenée au point de tangence ;
/// `removed_mm` est la longueur retirée de ce côté (0 si rien n'est retiré).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Trim {
    pub end: TrimEnd,
    pub removed_mm: f64,
}

/// Résultat d'un congé. L'arc de raccord va de `tangent_first` à
/// `tangent_second` autour de `center` (balayage < 180°) ; un cercle n'est
/// jamais coupé (`trim` = `None`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fillet {
    pub center: PointMm,
    pub radius_mm: f64,
    pub tangent_first: PointMm,
    pub tangent_second: PointMm,
    pub ccw: bool,
    pub trim_first: Option<Trim>,
    pub trim_second: Option<Trim>,
}

/// Support infini d'un tracé : droite ou cercle.
enum Support {
    Line { a: PointMm, u: VectorMm },
    Circle(Circle),
}

impl FilletPath {
    fn support(&self) -> Result<Support> {
        Ok(match *self {
            FilletPath::Segment { a, b } => {
                a.validate()?;
                b.validate()?;
                Support::Line {
                    a,
                    u: (b - a).unit()?,
                }
            }
            FilletPath::Arc(arc) => Support::Circle(Circle::new(arc.center, arc.radius_mm)?),
            FilletPath::Circle(c) => Support::Circle(c),
        })
    }

    /// Longueur du tracé ouvert (`None` pour un cercle entier).
    fn open_length(&self) -> Option<f64> {
        match *self {
            FilletPath::Segment { a, b } => Some(distance_mm(a, b)),
            FilletPath::Arc(arc) => Some(arc.length_mm()),
            FilletPath::Circle(_) => None,
        }
    }

    /// Abscisse curviligne de `p` (supposé sur le support) : projection pour un
    /// segment, longueur d'arc depuis le départ dans `[0, périmètre)` pour un
    /// arc ou un cercle (le cercle part de l'angle 0).
    fn raw_s(&self, p: PointMm) -> f64 {
        match *self {
            FilletPath::Segment { a, b } => match (b - a).unit() {
                Ok(u) => (p - a).dot(u),
                Err(_) => 0.0,
            },
            FilletPath::Arc(arc) => {
                let v = p - arc.center;
                let ang = v.y.atan2(v.x);
                let rel = if arc.sweep_rad > 0.0 {
                    (ang - arc.start_rad).rem_euclid(TAU)
                } else {
                    (arc.start_rad - ang).rem_euclid(TAU)
                };
                rel * arc.radius_mm
            }
            FilletPath::Circle(c) => {
                let v = p - c.center;
                v.y.atan2(v.x).rem_euclid(TAU) * c.radius_mm
            }
        }
    }

    /// Abscisse de `p` sur le tracé prolongé : un point d'un arc hors de
    /// l'arc est placé avant le départ (< 0) ou après l'arrivée (> longueur),
    /// du côté le plus proche.
    fn extended_s(&self, p: PointMm) -> f64 {
        match *self {
            FilletPath::Arc(arc) => {
                let s = self.raw_s(p);
                let (len, circ) = (arc.length_mm(), TAU * arc.radius_mm);
                if s <= len + 1e-9 || s - len <= circ - s {
                    s
                } else {
                    s - circ
                }
            }
            _ => self.raw_s(p),
        }
    }

    /// Le point (sur le support) est-il sur le tracé, à 1e-9 mm près ?
    fn on_path(&self, p: PointMm) -> bool {
        match *self {
            FilletPath::Segment { .. } => {
                let s = self.raw_s(p);
                s >= -EPSILON_MM && s <= self.open_length().unwrap_or(0.0) + EPSILON_MM
            }
            FilletPath::Arc(arc) => arc.contains_angle_of(p),
            FilletPath::Circle(_) => true,
        }
    }
}

/// Écart signé le plus court entre deux abscisses d'un cercle (période `circ`).
fn wrapped(delta: f64, circ: f64) -> f64 {
    let d = delta.rem_euclid(circ);
    if d > circ / 2.0 {
        d - circ
    } else {
        d
    }
}

fn sign(v: f64) -> f64 {
    if v > 0.0 {
        1.0
    } else {
        -1.0
    }
}

/// Position d'un point par rapport au coin `P` le long d'un tracé.
struct Placed {
    /// Écart signé coin → point, le long du tracé.
    delta: f64,
    /// Longueur retirée et extrémité coupée si le point est le point de tangence.
    trim: Option<Trim>,
}

/// Évalue le point de tangence `t` d'un tracé : doit être du bon côté du coin
/// (celui du point visé), sur le tracé, et laisser une partie non coupée.
/// `Ok(None)` = candidat refusé ; `Err(true)` = refusé parce que le point de
/// tangence tombe hors du tracé ou ne laisse rien (rayon trop grand).
fn place(
    path: &FilletPath,
    corner: PointMm,
    pick: PointMm,
    t: PointMm,
) -> std::result::Result<Placed, bool> {
    match path.open_length() {
        Some(len) => {
            let s_corner = path.extended_s(corner);
            let s_pick = match *path {
                FilletPath::Arc(arc) => arc.nearest_length_mm(pick),
                _ => path.raw_s(pick),
            };
            let dir_pick = s_pick - s_corner;
            if dir_pick.abs() <= EPSILON_MM {
                return Err(false);
            }
            let mut s_t = path.raw_s(t);
            if let FilletPath::Arc(arc) = *path {
                if TAU * arc.radius_mm - s_t <= EPSILON_MM {
                    s_t = 0.0;
                }
            }
            let side_ok = (s_t - s_corner) * dir_pick > EPSILON_MM;
            if !side_ok {
                return Err(false);
            }
            if !path.on_path(t) {
                return Err(true);
            }
            let s_t = s_t.clamp(0.0, len);
            if dir_pick > 0.0 {
                // Partie conservée : [s_t, len] ; l'extrémité de départ est coupée.
                if len - s_t < FILLET_MIN_KEPT_MM {
                    return Err(true);
                }
                Ok(Placed {
                    delta: s_t - s_corner,
                    trim: Some(Trim {
                        end: TrimEnd::Start,
                        removed_mm: s_t,
                    }),
                })
            } else {
                if s_t < FILLET_MIN_KEPT_MM {
                    return Err(true);
                }
                Ok(Placed {
                    delta: s_t - s_corner,
                    trim: Some(Trim {
                        end: TrimEnd::End,
                        removed_mm: len - s_t,
                    }),
                })
            }
        }
        None => {
            let FilletPath::Circle(c) = *path else {
                return Err(false);
            };
            let circ = TAU * c.radius_mm;
            let s_corner = path.raw_s(corner);
            let d_pick = wrapped(path.raw_s(pick) - s_corner, circ);
            let d_t = wrapped(path.raw_s(t) - s_corner, circ);
            if d_pick.abs() <= EPSILON_MM || d_t * d_pick <= EPSILON_MM {
                return Err(false);
            }
            Ok(Placed {
                delta: d_t,
                trim: None,
            })
        }
    }
}

/// Centres des cercles de rayon `r` tangents aux deux supports, avec le point
/// de tangence sur chacun.
fn candidates(a: &Support, b: &Support, r: f64) -> Vec<(PointMm, PointMm, PointMm)> {
    // Courbes décalées : droites ±r, cercles de rayon R+r, R−r (si R > r) ou
    // r−R (si R < r) ; `s` donne le sens du point de tangence par rapport au
    // rayon centre → centre du congé.
    enum Off {
        Line(PointMm, PointMm),
        Circle(Circle, f64),
    }
    let offsets = |s: &Support| -> Vec<Off> {
        match *s {
            Support::Line { a, u } => {
                let n = u.rotated_left();
                [r, -r]
                    .into_iter()
                    .map(|k| Off::Line(a + n * k, a + n * k + u))
                    .collect()
            }
            Support::Circle(c) => {
                let mut v = vec![(c.radius_mm + r, 1.0)];
                if c.radius_mm > r + EPSILON_MM {
                    v.push((c.radius_mm - r, 1.0));
                } else if r > c.radius_mm + EPSILON_MM {
                    v.push((r - c.radius_mm, -1.0));
                }
                v.into_iter()
                    .filter_map(|(rho, s)| {
                        Circle::new(c.center, rho).ok().map(|k| Off::Circle(k, s))
                    })
                    .collect()
            }
        }
    };
    let tangent = |s: &Support, o: PointMm, sgn: f64| -> Option<PointMm> {
        match *s {
            Support::Line { a, u } => Some(a + u * (o - a).dot(u)),
            Support::Circle(c) => {
                let dir = (o - c.center).unit().ok()?;
                Some(c.center + dir * (c.radius_mm * sgn))
            }
        }
    };
    let mut out = Vec::new();
    for oa in offsets(a) {
        for ob in offsets(b) {
            let centers: Vec<PointMm> = match (&oa, &ob) {
                (Off::Line(p, q), Off::Line(s, t)) => {
                    line_intersection(*p, *q, *s, *t).into_iter().collect()
                }
                (Off::Line(p, q), Off::Circle(c, _)) => {
                    circle_line_intersections(c, *p, *q).unwrap_or_default()
                }
                (Off::Circle(c, _), Off::Line(p, q)) => {
                    circle_line_intersections(c, *p, *q).unwrap_or_default()
                }
                (Off::Circle(c1, _), Off::Circle(c2, _)) => {
                    circle_circle_intersections(c1, c2).unwrap_or_default()
                }
            };
            let sa = if let Off::Circle(_, s) = oa { s } else { 1.0 };
            let sb = if let Off::Circle(_, s) = ob { s } else { 1.0 };
            for o in centers {
                if let (Some(ta), Some(tb)) = (tangent(a, o, sa), tangent(b, o, sb)) {
                    out.push((o, ta, tb));
                }
            }
        }
    }
    out
}

/// Coin de deux supports : leur intersection la plus proche des points visés.
fn corner_of(
    first: &FilletPath,
    second: &FilletPath,
    pick_first: PointMm,
    pick_second: PointMm,
) -> Result<PointMm> {
    let (a, b) = (first.support()?, second.support()?);
    let points: Vec<PointMm> = match (&a, &b) {
        (Support::Line { a: p, u }, Support::Line { a: q, u: v }) => {
            line_intersection(*p, *p + *u, *q, *q + *v)
                .map_err(|_| {
                    err("les deux segments sont parallèles : aucun raccord n'est possible")
                })
                .map(|c| vec![c])?
        }
        (Support::Line { a: p, u }, Support::Circle(c))
        | (Support::Circle(c), Support::Line { a: p, u }) => {
            circle_line_intersections(c, *p, *p + *u)?
        }
        (Support::Circle(c1), Support::Circle(c2)) => circle_circle_intersections(c1, c2)?,
    };
    points
        .into_iter()
        .min_by(|p, q| {
            let score = |x: &PointMm| distance_mm(*x, pick_first) + distance_mm(*x, pick_second);
            score(p).total_cmp(&score(q))
        })
        .ok_or_else(|| {
            err("les deux tracés ne se coupent pas, même prolongés : aucun raccord n'est possible")
        })
}

/// Congé de rayon `radius_mm` entre deux tracés. `pick_*` sont des points
/// visés sur chaque tracé : ils désignent le côté du coin que l'on **garde**
/// (la partie qui se trouve de ce côté, au-delà du point de tangence).
///
/// Refus (sans résultat partiel) : rayon non fini, ≤ 0 ou hors espace ;
/// deux segments parallèles ; tracés qui ne se coupent pas, même prolongés ;
/// point de tangence hors d'un tracé ou ne laissant rien (rayon trop grand).
pub fn fillet(
    first: &FilletPath,
    second: &FilletPath,
    radius_mm: f64,
    pick_first: PointMm,
    pick_second: PointMm,
) -> Result<Fillet> {
    check_radius(radius_mm)?;
    pick_first.validate()?;
    pick_second.validate()?;
    let corner = corner_of(first, second, pick_first, pick_second)?;
    let (sa, sb) = (first.support()?, second.support()?);
    let mut best: Option<(f64, Fillet)> = None;
    let mut too_big = false;
    for (o, ta, tb) in candidates(&sa, &sb, radius_mm) {
        let pa = match place(first, corner, pick_first, ta) {
            Ok(p) => p,
            Err(big) => {
                too_big |= big;
                continue;
            }
        };
        let pb = match place(second, corner, pick_second, tb) {
            Ok(p) => p,
            Err(big) => {
                too_big |= big;
                continue;
            }
        };
        if distance_mm(ta, tb) <= EPSILON_MM {
            continue;
        }
        let score = pa.delta.abs() + pb.delta.abs();
        if best.as_ref().is_none_or(|(s, _)| score < *s) {
            let ccw = (ta - o).cross(tb - o) > 0.0;
            best = Some((
                score,
                Fillet {
                    center: o,
                    radius_mm,
                    tangent_first: ta,
                    tangent_second: tb,
                    ccw,
                    trim_first: pa.trim,
                    trim_second: pb.trim,
                },
            ));
        }
    }
    best.map(|(_, f)| f).ok_or_else(|| {
        if too_big {
            err("le rayon est trop grand pour ces tracés : un point de tangence tombe hors d'un tracé ou n'en laisse rien")
        } else {
            err("aucun raccord de ce rayon dans l'angle visé : cliquez les tracés de part et d'autre du coin à arrondir")
        }
    })
}

/// Chanfrein entre deux segments : points à `d_first` et `d_second` du coin
/// (intersection des droites), du côté des points visés. Les trois fonctions
/// de coupe sont celles du congé : `trim_*` donne l'extrémité à ramener au
/// point de chanfrein.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Chamfer {
    pub first: PointMm,
    pub second: PointMm,
    pub trim_first: Trim,
    pub trim_second: Trim,
}

pub fn chamfer(
    first: (PointMm, PointMm),
    second: (PointMm, PointMm),
    d_first: f64,
    d_second: f64,
    pick_first: PointMm,
    pick_second: PointMm,
) -> Result<Chamfer> {
    for d in [d_first, d_second] {
        check_radius(d)?;
    }
    pick_first.validate()?;
    pick_second.validate()?;
    let pa = FilletPath::Segment {
        a: first.0,
        b: first.1,
    };
    let pb = FilletPath::Segment {
        a: second.0,
        b: second.1,
    };
    let corner = corner_of(&pa, &pb, pick_first, pick_second)?;
    let mut out = Vec::new();
    let mut too_big = false;
    for (path, pick, d) in [(&pa, pick_first, d_first), (&pb, pick_second, d_second)] {
        let FilletPath::Segment { a, b } = *path else {
            return Err(err("un chanfrein se fait entre deux segments"));
        };
        let u = (b - a).unit()?;
        let dir = sign((pick - corner).dot(u));
        if (pick - corner).dot(u).abs() <= EPSILON_MM {
            return Err(err(
                "le point visé sur le segment tombe sur le coin : cliquez à côté",
            ));
        }
        let t = corner + u * (dir * d);
        match place(path, corner, pick, t) {
            Ok(p) => out.push((
                t,
                p.trim.unwrap_or(Trim {
                    end: TrimEnd::Start,
                    removed_mm: 0.0,
                }),
            )),
            Err(big) => {
                too_big |= big;
                break;
            }
        }
    }
    if out.len() != 2 {
        return Err(if too_big {
            err("la distance est trop grande pour ce segment : le point de chanfrein tombe hors du tracé ou n'en laisse rien")
        } else {
            err("aucun chanfrein dans l'angle visé : cliquez les segments de part et d'autre du coin")
        });
    }
    if distance_mm(out[0].0, out[1].0) <= EPSILON_MM {
        return Err(err("les deux points de chanfrein sont confondus"));
    }
    Ok(Chamfer {
        first: out[0].0,
        second: out[1].0,
        trim_first: out[0].1,
        trim_second: out[1].1,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: f64, y: f64) -> PointMm {
        PointMm::new(x, y)
    }

    fn seg(a: PointMm, b: PointMm) -> FilletPath {
        FilletPath::Segment { a, b }
    }

    #[test]
    fn tangentes_depuis_un_point_exterieur() {
        let c = Circle::new(p(0.0, 0.0), 5.0).unwrap();
        let ts = tangent_points_from_point(&c, p(13.0, 0.0)).unwrap();
        assert_eq!(ts.len(), 2);
        for t in &ts {
            assert!((distance_mm(*t, c.center) - 5.0).abs() < 1e-9);
            // Rayon ⟂ tangente.
            assert!((*t - c.center).dot(p(13.0, 0.0) - *t).abs() < 1e-9);
        }
        // Longueur de la tangente : sqrt(13² − 5²) = 12.
        assert!((distance_mm(ts[0], p(13.0, 0.0)) - 12.0).abs() < 1e-9);
    }

    #[test]
    fn tangente_depuis_un_point_du_cercle_et_refus_interieur() {
        let c = Circle::new(p(0.0, 0.0), 5.0).unwrap();
        let ts = tangent_points_from_point(&c, p(0.0, 5.0)).unwrap();
        assert_eq!(ts, vec![p(0.0, 5.0)]);
        assert!(tangent_points_from_point(&c, p(1.0, 1.0)).is_err());
        assert!(tangent_points_from_point(&c, p(f64::NAN, 0.0)).is_err());
    }

    #[test]
    fn conge_segment_segment_angle_droit() {
        // Coin en (0,0), segments le long de +x et +y.
        let a = seg(p(0.0, 0.0), p(100.0, 0.0));
        let b = seg(p(0.0, 0.0), p(0.0, 100.0));
        let f = fillet(&a, &b, 10.0, p(50.0, 0.0), p(0.0, 50.0)).unwrap();
        assert!(distance_mm(f.center, p(10.0, 10.0)) < 1e-9);
        assert!(distance_mm(f.tangent_first, p(10.0, 0.0)) < 1e-9);
        assert!(distance_mm(f.tangent_second, p(0.0, 10.0)) < 1e-9);
        assert_eq!(f.trim_first.unwrap().end, TrimEnd::Start);
        assert!((f.trim_first.unwrap().removed_mm - 10.0).abs() < 1e-9);
        // (10,0) → (0,10) autour de (10,10) : sens horaire.
        assert!(!f.ccw);
    }

    #[test]
    fn conge_refuse_rayon_trop_grand_et_cas_impossibles() {
        let a = seg(p(0.0, 0.0), p(100.0, 0.0));
        let b = seg(p(0.0, 0.0), p(0.0, 30.0));
        assert!(fillet(&a, &b, 40.0, p(50.0, 0.0), p(0.0, 20.0)).is_err());
        for r in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert!(fillet(&a, &b, r, p(50.0, 0.0), p(0.0, 20.0)).is_err());
        }
        let par = seg(p(0.0, 5.0), p(100.0, 5.0));
        assert!(fillet(&a, &par, 2.0, p(50.0, 0.0), p(50.0, 5.0)).is_err());
        // Segment et cercle qui ne se coupent pas, même prolongés : pas de coin.
        let c = FilletPath::Circle(Circle::new(p(0.0, 50.0), 5.0).unwrap());
        assert!(fillet(&a, &c, 2.0, p(50.0, 0.0), p(0.0, 55.0)).is_err());
    }

    #[test]
    fn chanfrein_distances() {
        let a = (p(0.0, 0.0), p(100.0, 0.0));
        let b = (p(0.0, 0.0), p(0.0, 100.0));
        let c = chamfer(a, b, 10.0, 20.0, p(50.0, 0.0), p(0.0, 50.0)).unwrap();
        assert!(distance_mm(c.first, p(10.0, 0.0)) < 1e-9);
        assert!(distance_mm(c.second, p(0.0, 20.0)) < 1e-9);
        assert!(chamfer(a, b, 150.0, 20.0, p(50.0, 0.0), p(0.0, 50.0)).is_err());
        assert!(chamfer(a, b, 0.0, 20.0, p(50.0, 0.0), p(0.0, 50.0)).is_err());
        assert!(chamfer(a, b, f64::NAN, 20.0, p(50.0, 0.0), p(0.0, 50.0)).is_err());
    }
}
