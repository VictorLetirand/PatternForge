//! Construction rapide (PH-4a) : polygone, rectangle orienté, rectangle à coins arrondis.
//! Maths pures en mm : sommets, points de raccord et centres des arcs, mesures exactes
//! (formules fermées) servant à l'aperçu de l'interface.
//!
//! Conventions : rectangle de coins `P0 P1 P2 P3` dans le sens anti-horaire du repère local
//! (axes `u` = (cos, sin) et `v` = (−sin, cos)) ; `P0` est l'origine, ou le coin bas-gauche
//! quand le rectangle est centré sur l'origine ; la rotation (degrés, normalisée à [0 ; 360[)
//! se fait **autour de l'origine du rectangle** (son premier coin, ou son centre).

use crate::{distance_mm, GeometryError, PointMm, Result, EPSILON_MM};
use std::f64::consts::PI;

/// Côté, rayon ou distance entre sommets consécutifs minimal (mm) : égal à la tolérance de
/// fusion des points du modèle (en dessous, deux points n'en feraient qu'un).
pub const MIN_SIDE_MM: f64 = 1e-6;
/// Nombre maximal de sommets d'un polygone.
pub const MAX_POLYGON_VERTICES: usize = 200;

fn finite(v: f64, what: &'static str) -> Result<f64> {
    if v.is_finite() {
        Ok(v)
    } else {
        Err(GeometryError::NonFinite(what))
    }
}

/// Cosinus et sinus d'un angle en degrés, exacts pour les multiples de 90°.
fn cos_sin_deg(angle_deg: f64) -> (f64, f64) {
    let a = angle_deg.rem_euclid(360.0);
    if a == 0.0 {
        (1.0, 0.0)
    } else if a == 90.0 {
        (0.0, 1.0)
    } else if a == 180.0 {
        (-1.0, 0.0)
    } else if a == 270.0 {
        (0.0, -1.0)
    } else {
        let r = a.to_radians();
        (r.cos(), r.sin())
    }
}

/// Rectangle orienté : origine, dimensions, rotation autour de l'origine, option « centré ».
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RectSpec {
    pub origin: PointMm,
    pub width_mm: f64,
    pub height_mm: f64,
    #[serde(default)]
    pub rotation_deg: f64,
    /// Vrai : `origin` est le centre du rectangle ; faux : son premier coin.
    #[serde(default)]
    pub centered: bool,
}

impl RectSpec {
    fn check(&self) -> Result<()> {
        self.origin.validate()?;
        finite(self.rotation_deg, "l'angle de rotation")?;
        for v in [self.width_mm, self.height_mm] {
            finite(v, "une dimension")?;
            if v <= MIN_SIDE_MM {
                return Err(GeometryError::InvalidLength(v));
            }
        }
        Ok(())
    }

    /// Point du repère local `(lx, ly)` (mm, depuis le premier coin non tourné) dans le plan.
    fn world(&self, lx: f64, ly: f64) -> PointMm {
        let (c, s) = cos_sin_deg(self.rotation_deg);
        let (ox, oy) = if self.centered {
            (-self.width_mm / 2.0, -self.height_mm / 2.0)
        } else {
            (0.0, 0.0)
        };
        let (x, y) = (ox + lx, oy + ly);
        PointMm::new(
            self.origin.x + (x * c - y * s),
            self.origin.y + (x * s + y * c),
        )
    }

    /// Les quatre coins `P0 P1 P2 P3` (sens anti-horaire).
    pub fn corners(&self) -> Result<[PointMm; 4]> {
        self.check()?;
        let (w, h) = (self.width_mm, self.height_mm);
        let c = [
            self.world(0.0, 0.0),
            self.world(w, 0.0),
            self.world(w, h),
            self.world(0.0, h),
        ];
        for p in c {
            p.validate()?;
        }
        Ok(c)
    }
}

/// Contrôle d'un polygone dont les sommets sont déjà positionnés : 2 à 200 sommets (3 à 200
/// si fermé), sommets finis et dans l'espace de travail, sommets consécutifs distincts de plus
/// de [`MIN_SIDE_MM`] (le dernier et le premier aussi si fermé).
pub fn validate_polygon(vertices: &[PointMm], closed: bool) -> Result<()> {
    let n = vertices.len();
    let min = if closed { 3 } else { 2 };
    if n < min {
        return Err(GeometryError::InvalidConstruction(format!(
            "un polygone {} a au moins {min} sommets ({n} donné(s))",
            if closed { "fermé" } else { "ouvert" }
        )));
    }
    if n > MAX_POLYGON_VERTICES {
        return Err(GeometryError::InvalidConstruction(format!(
            "un polygone a au plus {MAX_POLYGON_VERTICES} sommets ({n} donnés)"
        )));
    }
    for p in vertices {
        p.validate()?;
    }
    let sides = if closed { n } else { n - 1 };
    for i in 0..sides {
        let (a, b) = (vertices[i], vertices[(i + 1) % n]);
        if distance_mm(a, b) <= MIN_SIDE_MM {
            return Err(GeometryError::InvalidConstruction(format!(
                "les sommets {} et {} sont confondus (côté de longueur nulle)",
                i + 1,
                (i + 1) % n + 1
            )));
        }
    }
    Ok(())
}

/// Périmètre (ou longueur de la ligne brisée) en mm.
pub fn polygon_length_mm(vertices: &[PointMm], closed: bool) -> f64 {
    let n = vertices.len();
    let sides = if closed { n } else { n.saturating_sub(1) };
    (0..sides)
        .map(|i| distance_mm(vertices[i], vertices[(i + 1) % n]))
        .sum()
}

/// Aire (valeur absolue, formule des lacets) d'un polygone fermé simple, en mm².
pub fn polygon_area_mm2(vertices: &[PointMm]) -> f64 {
    let n = vertices.len();
    let twice: f64 = (0..n)
        .map(|i| {
            let (a, b) = (vertices[i], vertices[(i + 1) % n]);
            a.x * b.y - b.x * a.y
        })
        .sum();
    twice.abs() / 2.0
}

/// Périmètre exact d'un rectangle de coins arrondis : `2(w−2r) + 2(h−2r) + 2πr`.
pub fn rounded_rectangle_perimeter_mm(w: f64, h: f64, r: f64) -> f64 {
    2.0 * (w - 2.0 * r) + 2.0 * (h - 2.0 * r) + 2.0 * PI * r
}

/// Aire exacte d'un rectangle de coins arrondis : `w·h − (4 − π)·r²`.
pub fn rounded_rectangle_area_mm2(w: f64, h: f64, r: f64) -> f64 {
    w * h - (4.0 - PI) * r * r
}

/// Mesures annoncées avant création (aperçu en lecture seule) : étendue, périmètre ou longueur,
/// aire d'un contour fermé, nombre de segments, d'arcs et de centres d'arcs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct QuickMeasure {
    pub width_mm: f64,
    pub height_mm: f64,
    pub perimeter_mm: f64,
    /// `None` pour un polygone ouvert.
    pub area_mm2: Option<f64>,
    pub segments: usize,
    pub arcs: usize,
    pub centers: usize,
}

/// Mesures d'un polygone (mêmes contrôles que la commande).
pub fn measure_polygon(vertices: &[PointMm], closed: bool) -> Result<QuickMeasure> {
    validate_polygon(vertices, closed)?;
    let (mut lo, mut hi) = (vertices[0], vertices[0]);
    for v in vertices {
        lo = PointMm::new(lo.x.min(v.x), lo.y.min(v.y));
        hi = PointMm::new(hi.x.max(v.x), hi.y.max(v.y));
    }
    Ok(QuickMeasure {
        width_mm: hi.x - lo.x,
        height_mm: hi.y - lo.y,
        perimeter_mm: polygon_length_mm(vertices, closed),
        area_mm2: closed.then(|| polygon_area_mm2(vertices)),
        segments: if closed {
            vertices.len()
        } else {
            vertices.len() - 1
        },
        arcs: 0,
        centers: 0,
    })
}

/// Mesures d'un rectangle orienté (dimensions saisies, pas l'étendue du rectangle tourné).
pub fn measure_rectangle(rect: &RectSpec) -> Result<QuickMeasure> {
    rect.corners()?;
    Ok(QuickMeasure {
        width_mm: rect.width_mm,
        height_mm: rect.height_mm,
        perimeter_mm: 2.0 * (rect.width_mm + rect.height_mm),
        area_mm2: Some(rect.width_mm * rect.height_mm),
        segments: 4,
        arcs: 0,
        centers: 0,
    })
}

/// Mesures d'un rectangle arrondi (mêmes contrôles et même disposition que la commande).
pub fn measure_rounded_rectangle(rect: &RectSpec, radius_mm: f64) -> Result<QuickMeasure> {
    let lay = rounded_rectangle_layout(rect, radius_mm)?;
    Ok(QuickMeasure {
        width_mm: rect.width_mm,
        height_mm: rect.height_mm,
        perimeter_mm: rounded_rectangle_perimeter_mm(rect.width_mm, rect.height_mm, radius_mm),
        area_mm2: Some(rounded_rectangle_area_mm2(
            rect.width_mm,
            rect.height_mm,
            radius_mm,
        )),
        segments: lay.line_count(),
        arcs: lay.arc_count(),
        centers: lay.center_count(),
    })
}

/// Un élément du contour d'un rectangle arrondi ; les nombres sont des index de
/// [`RoundedLayout::points`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoundedItem {
    Line {
        from: usize,
        to: usize,
    },
    /// Arc anti-horaire de `start` à `end` autour de `center`.
    Arc {
        center: usize,
        start: usize,
        end: usize,
    },
}

/// Points et éléments d'un rectangle arrondi, dans l'ordre du contour (anti-horaire).
#[derive(Debug, Clone, PartialEq)]
pub struct RoundedLayout {
    pub points: Vec<PointMm>,
    /// Vrai pour les centres d'arcs (même index que `points`).
    pub is_center: Vec<bool>,
    pub items: Vec<RoundedItem>,
}

impl RoundedLayout {
    pub fn center_count(&self) -> usize {
        self.is_center.iter().filter(|c| **c).count()
    }
    pub fn arc_count(&self) -> usize {
        self.items
            .iter()
            .filter(|i| matches!(i, RoundedItem::Arc { .. }))
            .count()
    }
    pub fn line_count(&self) -> usize {
        self.items.len() - self.arc_count()
    }
}

/// Dispose un rectangle à coins arrondis de rayon `radius_mm`. Un côté droit restant de moins
/// de 1e-9 mm est nul (rayon = moitié de la plus petite dimension) : les deux quarts d'arc
/// voisins forment alors **un** demi-cercle (stade : deux demi-cercles et deux segments ;
/// carré : deux demi-cercles). Refus : rayon ≤ 1e-6 mm, rayon supérieur à la moitié de la plus
/// petite dimension, côté droit restant compris entre 1e-9 et 1e-6 mm (indiscernable d'un
/// point à la tolérance de fusion).
pub fn rounded_rectangle_layout(rect: &RectSpec, radius_mm: f64) -> Result<RoundedLayout> {
    let p = rect.corners()?;
    finite(radius_mm, "le rayon")?;
    if radius_mm == 0.0 {
        return Err(GeometryError::InvalidArc(
            "rayon nul : utilisez le rectangle simple pour des coins vifs".into(),
        ));
    }
    if radius_mm <= MIN_SIDE_MM {
        return Err(GeometryError::InvalidLength(radius_mm));
    }
    let (w, h, r) = (rect.width_mm, rect.height_mm, radius_mm);
    let straight = [w - 2.0 * r, h - 2.0 * r];
    for l in straight {
        if l < -EPSILON_MM {
            return Err(GeometryError::InvalidArc(format!(
                "rayon {r} mm trop grand : au plus la moitié de la plus petite dimension ({} mm)",
                w.min(h) / 2.0
            )));
        }
        if l > EPSILON_MM && l < MIN_SIDE_MM {
            return Err(GeometryError::InvalidArc(format!(
                "le côté droit restant ({l} mm) serait plus court que {MIN_SIDE_MM} mm : \
                 prenez exactement la moitié de la plus petite dimension ou un rayon plus petit"
            )));
        }
    }
    let zero = [straight[0] <= EPSILON_MM, straight[1] <= EPSILON_MM];
    // Directions des côtés (unitaires, repère du plan) et tangentes.
    let dirs: [PointMm; 4] = {
        let d = |a: PointMm, b: PointMm| {
            let l = distance_mm(a, b);
            PointMm::new((b.x - a.x) / l, (b.y - a.y) / l)
        };
        [d(p[0], p[1]), d(p[1], p[2]), d(p[2], p[3]), d(p[3], p[0])]
    };
    let at = |q: PointMm, d: PointMm, k: f64| PointMm::new(q.x + d.x * k, q.y + d.y * k);
    // Début (S) et fin (E) du côté i, centre du coin i.
    let s: [PointMm; 4] = std::array::from_fn(|i| at(p[i], dirs[i], r));
    let e: [PointMm; 4] = std::array::from_fn(|i| at(p[(i + 1) % 4], dirs[i], -r));
    let c: [PointMm; 4] = std::array::from_fn(|i| {
        let prev = dirs[(i + 3) % 4];
        PointMm::new(
            p[i].x + dirs[i].x * r - prev.x * r,
            p[i].y + dirs[i].y * r - prev.y * r,
        )
    });
    let mut lay = RoundedLayout {
        points: Vec::new(),
        is_center: Vec::new(),
        items: Vec::new(),
    };
    let add = |lay: &mut RoundedLayout, q: PointMm, center: bool| {
        lay.points.push(q);
        lay.is_center.push(center);
        lay.points.len() - 1
    };
    if !zero[0] && !zero[1] {
        // Quatre côtés droits et quatre quarts d'arc, dans l'ordre du contour.
        let sp: Vec<usize> = (0..4).map(|i| add(&mut lay, s[i], false)).collect();
        let ep: Vec<usize> = (0..4).map(|i| add(&mut lay, e[i], false)).collect();
        let cp: Vec<usize> = (0..4).map(|i| add(&mut lay, c[i], true)).collect();
        for i in 0..4 {
            let next = (i + 1) % 4;
            lay.items.push(RoundedItem::Line {
                from: sp[i],
                to: ep[i],
            });
            lay.items.push(RoundedItem::Arc {
                center: cp[next],
                start: ep[i],
                end: sp[next],
            });
        }
        return Ok(lay);
    }
    // Un côté nul fusionne ses deux coins voisins en un demi-cercle. `a` = 0 : côtés 0 et 2
    // nuls (coins 0-1 et 2-3 fusionnés) ; `a` = 1 : côtés 1 et 3 nuls (coins 1-2 et 3-0).
    let a = if zero[0] { 0 } else { 1 };
    let k = |i: usize| (a + i) % 4;
    let c1 = add(&mut lay, c[k(0)], true);
    let st1 = add(&mut lay, e[k(3)], false);
    let en1 = add(&mut lay, s[k(1)], false);
    lay.items.push(RoundedItem::Arc {
        center: c1,
        start: st1,
        end: en1,
    });
    if zero[0] && zero[1] {
        // Carré de rayon moitié du côté : un cercle, deux demi-cercles de même centre.
        lay.items.push(RoundedItem::Arc {
            center: c1,
            start: en1,
            end: st1,
        });
        return Ok(lay);
    }
    let c2 = add(&mut lay, c[k(2)], true);
    let st2 = add(&mut lay, e[k(1)], false);
    let en2 = add(&mut lay, s[k(3)], false);
    lay.items.push(RoundedItem::Line { from: en1, to: st2 });
    lay.items.push(RoundedItem::Arc {
        center: c2,
        start: st2,
        end: en2,
    });
    lay.items.push(RoundedItem::Line { from: en2, to: st1 });
    Ok(lay)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(w: f64, h: f64) -> RectSpec {
        RectSpec {
            origin: PointMm::new(10.0, 20.0),
            width_mm: w,
            height_mm: h,
            rotation_deg: 0.0,
            centered: false,
        }
    }

    #[test]
    fn coins_calcules_a_la_main() {
        let c = rect(100.0, 50.0).corners().unwrap();
        assert_eq!(c[0], PointMm::new(10.0, 20.0));
        assert_eq!(c[1], PointMm::new(110.0, 20.0));
        assert_eq!(c[2], PointMm::new(110.0, 70.0));
        assert_eq!(c[3], PointMm::new(10.0, 70.0));
    }

    #[test]
    fn stade_deux_demi_cercles_et_deux_segments() {
        let l = rounded_rectangle_layout(&rect(100.0, 70.0), 35.0).unwrap();
        assert_eq!((l.arc_count(), l.line_count(), l.center_count()), (2, 2, 2));
    }

    #[test]
    fn rayon_trop_grand_refuse() {
        assert!(rounded_rectangle_layout(&rect(100.0, 70.0), 35.1).is_err());
        assert!(rounded_rectangle_layout(&rect(100.0, 70.0), 0.0).is_err());
        assert!(rounded_rectangle_layout(&rect(100.0, 70.0), f64::NAN).is_err());
    }
}
