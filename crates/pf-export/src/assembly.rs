//! Assemblage de pages (US-13.2) : rotation d'export par quarts de tour, primitives imprimées
//! d'une scène et test « cette fenêtre de page contient-elle quelque chose ? » pour omettre les
//! pages vides.
//!
//! Rien ici ne modifie le modèle ni la scène : la rotation est une isométrie appliquée au dessin
//! (`Tile::to_page_mm`), jamais aux données. Les courbes sont découpées en petits morceaux dont
//! la boîte englobante de contrôle contient le morceau (le test n'oublie donc jamais un contenu :
//! une page conservée à tort coûte du papier, une page omise à tort coûterait du patron).

use crate::{PathGeom, Scene};
use pf_core::PathRole;
use pf_geometry::{CubicBezier, PointMm};

/// Tolérance (mm) du test « la fenêtre touche le contenu » : un contenu qui effleure le bord
/// d'une fenêtre la garde.
pub const WINDOW_EPS_MM: f64 = 1e-6;
/// Longueur (mm) des morceaux de courbe testés ; plus petit = test plus fin.
const CURVE_PIECE_MM: f64 = 2.0;
/// Nombre maximal de morceaux par courbe.
const CURVE_PIECES_MAX: usize = 256;

/// Tourne un point de `turns` quarts de tour dans le sens trigonométrique, autour de l'origine
/// (isométrie exacte : aucun sinus ni cosinus, les distances sont conservées).
pub fn rotate_quarter(p: PointMm, turns: u8) -> PointMm {
    match turns % 4 {
        0 => p,
        1 => PointMm::new(-p.y, p.x),
        2 => PointMm::new(-p.x, -p.y),
        _ => PointMm::new(p.y, -p.x),
    }
}

/// Élément imprimé simplifié : un segment ou un rectangle aligné sur les axes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Prim {
    Seg(PointMm, PointMm),
    Rect(PointMm, PointMm),
}

fn rect_of(points: &[PointMm]) -> Prim {
    let mut lo = points[0];
    let mut hi = points[0];
    for p in &points[1..] {
        lo = PointMm::new(lo.x.min(p.x), lo.y.min(p.y));
        hi = PointMm::new(hi.x.max(p.x), hi.y.max(p.y));
    }
    Prim::Rect(lo, hi)
}

impl Prim {
    pub(crate) fn rotated(self, turns: u8) -> Prim {
        match self {
            Prim::Seg(a, b) => Prim::Seg(rotate_quarter(a, turns), rotate_quarter(b, turns)),
            Prim::Rect(lo, hi) => rect_of(&[rotate_quarter(lo, turns), rotate_quarter(hi, turns)]),
        }
    }

    /// Mise à l'échelle `(sx, sy)` autour de l'origine (facteurs strictement positifs : l'ordre
    /// des bornes est conservé). Sans correction (`1,0 / 1,0`), l'élément est rendu tel quel.
    pub(crate) fn scaled(self, sx: f64, sy: f64) -> Prim {
        let f = |p: PointMm| PointMm::new(p.x * sx, p.y * sy);
        match self {
            Prim::Seg(a, b) => Prim::Seg(f(a), f(b)),
            Prim::Rect(lo, hi) => Prim::Rect(f(lo), f(hi)),
        }
    }

    /// Boîte englobante : (min, max).
    pub(crate) fn bounds(&self) -> (PointMm, PointMm) {
        match *self {
            Prim::Seg(a, b) => (
                PointMm::new(a.x.min(b.x), a.y.min(b.y)),
                PointMm::new(a.x.max(b.x), a.y.max(b.y)),
            ),
            Prim::Rect(lo, hi) => (lo, hi),
        }
    }

    /// Touche-t-il le rectangle `[x0 ; x1] × [y0 ; y1]` (bords compris) ?
    pub(crate) fn hits(&self, x0: f64, y0: f64, x1: f64, y1: f64) -> bool {
        match *self {
            Prim::Rect(lo, hi) => lo.x <= x1 && hi.x >= x0 && lo.y <= y1 && hi.y >= y0,
            Prim::Seg(a, b) => {
                // Découpage de Liang-Barsky du segment par le rectangle.
                let (dx, dy) = (b.x - a.x, b.y - a.y);
                let (mut t0, mut t1) = (0.0_f64, 1.0_f64);
                for (p, q) in [
                    (-dx, a.x - x0),
                    (dx, x1 - a.x),
                    (-dy, a.y - y0),
                    (dy, y1 - a.y),
                ] {
                    if p == 0.0 {
                        if q < 0.0 {
                            return false;
                        }
                    } else {
                        let r = q / p;
                        if p < 0.0 {
                            t0 = t0.max(r);
                        } else {
                            t1 = t1.min(r);
                        }
                        if t0 > t1 {
                            return false;
                        }
                    }
                }
                true
            }
        }
    }
}

/// Morceaux d'une Bézier cubique : rectangles qui contiennent chacun un morceau (polygone de
/// contrôle de la portion `[t0 ; t1]`, forme de Hermite exacte).
fn bezier_pieces(c: &CubicBezier, out: &mut Vec<Prim>) {
    let poly = pf_geometry::distance_mm(c.p0, c.c1)
        + pf_geometry::distance_mm(c.c1, c.c2)
        + pf_geometry::distance_mm(c.c2, c.p3);
    let n = if poly.is_finite() {
        ((poly / CURVE_PIECE_MM).ceil() as usize).clamp(1, CURVE_PIECES_MAX)
    } else {
        1
    };
    for i in 0..n {
        let (t0, t1) = (i as f64 / n as f64, (i + 1) as f64 / n as f64);
        let (p0, p3) = (c.point_at(t0), c.point_at(t1));
        let (d0, d1) = (c.derivative_at(t0), c.derivative_at(t1));
        let k = (t1 - t0) / 3.0;
        let c1 = PointMm::new(p0.x + k * d0.x, p0.y + k * d0.y);
        let c2 = PointMm::new(p3.x - k * d1.x, p3.y - k * d1.y);
        out.push(rect_of(&[p0, c1, c2, p3]));
    }
}

/// Tout ce que `tile_page` dessine pour cette scène, en coordonnées de la feuille.
pub(crate) fn scene_prims(scene: &Scene) -> Vec<Prim> {
    let mut v = Vec::new();
    for path in &scene.paths {
        match &path.geom {
            &PathGeom::Line(a, b) => {
                v.push(Prim::Seg(a, b));
                if path.role == PathRole::Grainline {
                    for (p, q) in crate::grain_arrow_marks(a, b, path.sense) {
                        v.push(Prim::Seg(p, q));
                    }
                }
            }
            PathGeom::Cubic(k) => bezier_pieces(k, &mut v),
            PathGeom::Arc(_) | PathGeom::Circle(_) | PathGeom::Spline(_) => {
                for k in path.geom.arc_beziers() {
                    bezier_pieces(&k, &mut v);
                }
            }
        }
    }
    for cut in &scene.cut_contours {
        for g in &cut.edges {
            match g {
                &PathGeom::Line(a, b) => v.push(Prim::Seg(a, b)),
                PathGeom::Cubic(k) => bezier_pieces(k, &mut v),
                PathGeom::Arc(_) => {
                    for k in g.arc_beziers() {
                        bezier_pieces(&k, &mut v);
                    }
                }
                PathGeom::Circle(_) | PathGeom::Spline(_) => {}
            }
        }
        for (p, q) in cut.notch_marks.iter().chain(&cut.cut_notch_marks) {
            v.push(Prim::Seg(*p, *q));
        }
    }
    for (p, q) in &scene.notch_marks {
        v.push(Prim::Seg(*p, *q));
    }
    for m in &scene.markings {
        let h = m.half_extent_mm();
        v.push(Prim::Rect(
            PointMm::new(m.pos.x - h, m.pos.y - h),
            PointMm::new(m.pos.x + h, m.pos.y + h),
        ));
    }
    for m in &scene.stitch_holes {
        let h = m.half_extent_mm();
        v.push(Prim::Rect(
            PointMm::new(m.pos.x - h, m.pos.y - h),
            PointMm::new(m.pos.x + h, m.pos.y + h),
        ));
    }
    for z in &scene.zones {
        for (p, q) in z.edges() {
            v.push(Prim::Seg(p, q));
        }
    }
    for b in &scene.buttons {
        let (hx, hy) = b.half_extent_mm();
        v.push(Prim::Rect(
            PointMm::new(b.pos.x - hx, b.pos.y - hy),
            PointMm::new(b.pos.x + hx, b.pos.y + hy),
        ));
    }
    for d in &scene.dimensions {
        for (p, q) in d.marks() {
            v.push(Prim::Seg(p, q));
        }
        let (t, hw) = (d.text_anchor(), d.text_half_width_mm());
        v.push(Prim::Rect(
            PointMm::new(t.x - hw, t.y),
            PointMm::new(t.x + hw, t.y + crate::DIMENSION_TEXT_MM),
        ));
    }
    for d in &scene.full_dimensions {
        for (p, q) in &d.marks {
            v.push(Prim::Seg(*p, *q));
        }
        v.push(rect_of(&d.text_corners()));
    }
    // Un point n'est dessiné que s'il est dans la fenêtre : un rectangle de la taille du repère.
    for p in &scene.points {
        v.push(Prim::Rect(p.pos, p.pos));
    }
    v
}

/// Grille de fenêtres : fenêtre `(col, row)` = `[x0 + col·sx ; + ww] × [top − wh − row·sy ; top − row·sy]`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Grid {
    pub x0: f64,
    pub top: f64,
    pub sx: f64,
    pub sy: f64,
    pub ww: f64,
    pub wh: f64,
    pub cols: usize,
    pub rows: usize,
}

impl Grid {
    /// Fenêtre de la case `(col, row)` : `(x0, y0, x1, y1)`.
    pub(crate) fn window(&self, col: usize, row: usize) -> (f64, f64, f64, f64) {
        let x = self.x0 + col as f64 * self.sx;
        let y = self.top - self.wh - row as f64 * self.sy;
        (x, y, x + self.ww, y + self.wh)
    }

    /// Cases dont la fenêtre touche au moins un élément (`occupied[row * cols + col]`). Les
    /// éléments sont déjà dans le repère de la grille.
    pub(crate) fn occupied(&self, prims: &[Prim]) -> Vec<bool> {
        let mut occ = vec![false; self.cols * self.rows];
        let e = WINDOW_EPS_MM;
        let range = |lo: f64, hi: f64, count: usize| -> Option<(usize, usize)> {
            if !(lo.is_finite() && hi.is_finite()) || hi < lo {
                return None;
            }
            let a = lo.max(0.0).floor();
            let b = hi.min(count as f64 - 1.0).ceil();
            (a <= b).then_some((a as usize, b as usize))
        };
        for prim in prims {
            let (lo, hi) = prim.bounds();
            // Cases candidates : colonnes et lignes dont la fenêtre croise la boîte de l'élément.
            let cols = range(
                (lo.x - e - self.x0 - self.ww) / self.sx,
                (hi.x + e - self.x0) / self.sx,
                self.cols,
            );
            let rows = range(
                (self.top - self.wh - hi.y - e) / self.sy,
                (self.top - lo.y + e) / self.sy,
                self.rows,
            );
            let (Some((c0, c1)), Some((r0, r1))) = (cols, rows) else {
                continue;
            };
            for row in r0..=r1 {
                for col in c0..=c1 {
                    if occ[row * self.cols + col] {
                        continue;
                    }
                    let (x0, y0, x1, y1) = self.window(col, row);
                    if prim.hits(x0 - e, y0 - e, x1 + e, y1 + e) {
                        occ[row * self.cols + col] = true;
                    }
                }
            }
        }
        occ
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quarts_de_tour_isometries_exactes() {
        let p = PointMm::new(3.0, 4.0);
        assert_eq!(rotate_quarter(p, 0), p);
        assert_eq!(rotate_quarter(p, 1), PointMm::new(-4.0, 3.0));
        assert_eq!(rotate_quarter(p, 2), PointMm::new(-3.0, -4.0));
        assert_eq!(rotate_quarter(p, 3), PointMm::new(4.0, -3.0));
        assert_eq!(rotate_quarter(rotate_quarter(p, 1), 3), p);
        for t in 0..4 {
            let q = rotate_quarter(PointMm::new(0.0, 0.0), t);
            assert_eq!((q.x.abs(), q.y.abs()), (0.0, 0.0));
        }
    }

    #[test]
    fn segment_contre_rectangle() {
        let s = |a: (f64, f64), b: (f64, f64)| {
            Prim::Seg(PointMm::new(a.0, a.1), PointMm::new(b.0, b.1))
        };
        // Diagonale qui traverse un coin sans toucher le coin opposé.
        let d = s((-10.0, -10.0), (110.0, 110.0));
        assert!(d.hits(0.0, 0.0, 10.0, 10.0));
        assert!(!d.hits(50.0, 0.0, 60.0, 10.0));
        // Bord compris.
        assert!(s((10.0, -5.0), (10.0, 5.0)).hits(0.0, 0.0, 10.0, 10.0));
        assert!(!s((10.1, -5.0), (10.1, 5.0)).hits(0.0, 0.0, 10.0, 10.0));
        // Segment dégénéré (un point) dedans et dehors.
        assert!(s((1.0, 1.0), (1.0, 1.0)).hits(0.0, 0.0, 10.0, 10.0));
        assert!(!s((11.0, 1.0), (11.0, 1.0)).hits(0.0, 0.0, 10.0, 10.0));
    }
}
