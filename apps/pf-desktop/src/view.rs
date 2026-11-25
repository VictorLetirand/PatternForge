//! Transformation d'affichage monde (mm, Y vers le haut) ↔ écran (px, Y vers
//! le bas). C'est le **seul** endroit où des pixels rencontrent des
//! millimètres ; le modèle n'est jamais modifié par un zoom ou un déplacement
//! de la vue.

use pf_geometry::{Bbox, PointMm};

/// Pixels logiques par millimètre à « 100 % » (96 px par pouce).
pub const PX_PER_MM_AT_100: f64 = 96.0 / 25.4;
pub const MIN_ZOOM: f64 = PX_PER_MM_AT_100 * 0.02;
pub const MAX_ZOOM: f64 = PX_PER_MM_AT_100 * 64.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct View {
    /// Point du monde affiché au centre du canevas.
    pub center: PointMm,
    /// Pixels par millimètre.
    pub zoom: f64,
}

impl Default for View {
    fn default() -> Self {
        View {
            center: PointMm::new(60.0, 40.0),
            zoom: PX_PER_MM_AT_100,
        }
    }
}

impl View {
    /// Monde → écran. `canvas_center` est le centre du canevas en pixels.
    pub fn to_screen(self, p: PointMm, canvas_center: (f64, f64)) -> (f64, f64) {
        (
            canvas_center.0 + (p.x - self.center.x) * self.zoom,
            canvas_center.1 - (p.y - self.center.y) * self.zoom,
        )
    }

    /// Écran → monde (inverse exact de [`View::to_screen`]).
    pub fn to_world(self, s: (f64, f64), canvas_center: (f64, f64)) -> PointMm {
        PointMm::new(
            self.center.x + (s.0 - canvas_center.0) / self.zoom,
            self.center.y - (s.1 - canvas_center.1) / self.zoom,
        )
    }

    pub fn zoom_percent(&self) -> f64 {
        self.zoom / PX_PER_MM_AT_100 * 100.0
    }

    /// Zoom d'un facteur en gardant fixe le point du monde sous le curseur.
    pub fn zoom_about(&mut self, cursor: (f64, f64), canvas_center: (f64, f64), factor: f64) {
        if !factor.is_finite() || factor <= 0.0 {
            return;
        }
        let anchor = self.to_world(cursor, canvas_center);
        self.zoom = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        let after = self.to_world(cursor, canvas_center);
        self.center.x += anchor.x - after.x;
        self.center.y += anchor.y - after.y;
    }

    /// Déplace la vue d'un glissement en pixels.
    pub fn pan_pixels(&mut self, dx: f64, dy: f64) {
        self.center.x -= dx / self.zoom;
        self.center.y += dy / self.zoom;
    }

    /// Cadre le contenu dans un canevas de la taille donnée (px).
    pub fn fit(&mut self, bbox: Option<Bbox>, canvas_size: (f64, f64)) {
        let Some(bb) = bbox else {
            *self = View::default();
            return;
        };
        let (w, h) = (bb.width().max(20.0), bb.height().max(20.0));
        let margin = 0.8;
        let zoom = (canvas_size.0 * margin / w).min(canvas_size.1 * margin / h);
        self.zoom = zoom.clamp(MIN_ZOOM, PX_PER_MM_AT_100 * 4.0);
        self.center = PointMm::new((bb.min.x + bb.max.x) / 2.0, (bb.min.y + bb.max.y) / 2.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn at08_aller_retour_exact_a_tous_les_zooms() {
        let center = (640.0, 360.0);
        let pts = [
            PointMm::new(0., 0.),
            PointMm::new(120., 80.),
            PointMm::new(-37.25, 412.125),
        ];
        for percent in [25.0, 100.0, 400.0] {
            let v = View {
                zoom: PX_PER_MM_AT_100 * percent / 100.0,
                center: PointMm::new(13.7, -4.2),
            };
            for p in pts {
                let back = v.to_world(v.to_screen(p, center), center);
                assert!((back.x - p.x).abs() < 1e-9 && (back.y - p.y).abs() < 1e-9);
            }
            // La distance en mm reconstruite depuis l'écran ne dépend pas du zoom.
            let (a, b) = (v.to_screen(pts[0], center), v.to_screen(pts[1], center));
            let d = pf_geometry::distance_mm(v.to_world(a, center), v.to_world(b, center));
            assert!(
                (d - 144.222_051_018_559_57).abs() < 1e-9,
                "zoom {percent} % : {d}"
            );
        }
    }

    #[test]
    fn zoom_garde_le_point_sous_le_curseur() {
        let center = (500.0, 300.0);
        let mut v = View::default();
        let cursor = (123.0, 456.0);
        let before = v.to_world(cursor, center);
        v.zoom_about(cursor, center, 2.5);
        let after = v.to_world(cursor, center);
        assert!((before.x - after.x).abs() < 1e-9 && (before.y - after.y).abs() < 1e-9);
        v.zoom_about(cursor, center, 1.0e9);
        assert_eq!(v.zoom, MAX_ZOOM);
        v.zoom_about(cursor, center, f64::NAN);
        assert_eq!(v.zoom, MAX_ZOOM);
    }

    #[test]
    fn y_ecran_inverse() {
        let v = View::default();
        let up = v.to_screen(PointMm::new(60.0, 50.0), (0.0, 0.0));
        assert!(
            up.1 < 0.0,
            "un point plus haut dans le monde est plus haut à l'écran"
        );
    }
}
