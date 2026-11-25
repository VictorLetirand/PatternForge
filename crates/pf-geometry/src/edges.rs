//! Contours d'une image en niveaux de gris (US-14.2) : gradient de Sobel, seuil, accrochage.
//!
//! Calcul pur, **à la demande**, annulable (`cancel` est relu à chaque ligne). Le résultat est une
//! aide d'accrochage : il n'est ni stocké ni lié au modèle.
//!
//! Convention : l'amplitude d'un pixel est `sqrt(gx² + gy²) / 4` avec les noyaux de Sobel 3 × 3
//! (coefficients 1-2-1), arrondie et bornée à 255 (un octet par pixel) ; pour une marche franche
//! de `h` niveaux de gris elle vaut `h`. Les pixels du bord de l'image valent 0.
//! Un pixel est « contour » si son amplitude est **≥** au seuil (seuil ≤ 0 refusé).

use std::sync::atomic::{AtomicBool, Ordering};

/// Nombre maximal de pixels traités (≈ 100 mégapixels).
pub const MAX_EDGE_PIXELS: usize = 100_000_000;

/// Amplitude de gradient par pixel.
#[derive(Debug, Clone, PartialEq)]
pub struct EdgeMap {
    pub width: usize,
    pub height: usize,
    /// `width * height` amplitudes, ligne par ligne.
    pub magnitude: Vec<u8>,
}

/// Calcule l'amplitude de Sobel. `None` si `cancel` est levé en cours de calcul ; erreur si les
/// dimensions ne correspondent pas aux données ou dépassent `MAX_EDGE_PIXELS`.
pub fn sobel(
    gray: &[u8],
    width: usize,
    height: usize,
    cancel: &AtomicBool,
) -> Result<Option<EdgeMap>, String> {
    let n = width
        .checked_mul(height)
        .filter(|n| *n <= MAX_EDGE_PIXELS)
        .ok_or_else(|| "image trop grande".to_string())?;
    if n == 0 || gray.len() != n {
        return Err("dimensions incohérentes avec les pixels".to_string());
    }
    let mut mag = vec![0u8; n];
    if width >= 3 && height >= 3 {
        let g = |x: usize, y: usize| gray[y * width + x] as f32;
        for y in 1..height - 1 {
            if cancel.load(Ordering::Relaxed) {
                return Ok(None);
            }
            for x in 1..width - 1 {
                let gx = (g(x + 1, y - 1) + 2.0 * g(x + 1, y) + g(x + 1, y + 1))
                    - (g(x - 1, y - 1) + 2.0 * g(x - 1, y) + g(x - 1, y + 1));
                let gy = (g(x - 1, y + 1) + 2.0 * g(x, y + 1) + g(x + 1, y + 1))
                    - (g(x - 1, y - 1) + 2.0 * g(x, y - 1) + g(x + 1, y - 1));
                mag[y * width + x] = ((gx * gx + gy * gy).sqrt() / 4.0).round().min(255.0) as u8;
            }
        }
    }
    Ok(Some(EdgeMap {
        width,
        height,
        magnitude: mag,
    }))
}

impl EdgeMap {
    /// Pixel de contour (amplitude ≥ `threshold`) le plus proche de `(px, py)` dans un rayon de
    /// `radius_px` pixels (centres de pixels : `(x + 0,5 ; y + 0,5)`). À distance égale : amplitude
    /// la plus forte, puis plus petit `y`, puis plus petit `x` (résultat déterministe). Retourne le
    /// centre du pixel en pixels d'image. `None` : rien dans le rayon, ou entrée non finie.
    pub fn nearest_edge(
        &self,
        px: f64,
        py: f64,
        radius_px: f64,
        threshold: f32,
    ) -> Option<(f64, f64)> {
        if !px.is_finite() || !py.is_finite() || !radius_px.is_finite() || radius_px < 0.0 {
            return None;
        }
        if !threshold.is_finite() || threshold <= 0.0 {
            return None;
        }
        let x0 = ((px - radius_px).floor().max(0.0)) as usize;
        let y0 = ((py - radius_px).floor().max(0.0)) as usize;
        let x1 = (((px + radius_px).ceil())
            .min(self.width as f64 - 1.0)
            .max(0.0)) as usize;
        let y1 = (((py + radius_px).ceil())
            .min(self.height as f64 - 1.0)
            .max(0.0)) as usize;
        let mut best: Option<(f64, u8, usize, usize)> = None;
        for y in y0..=y1 {
            for x in x0..=x1 {
                let m = self.magnitude[y * self.width + x];
                if (m as f32) < threshold {
                    continue;
                }
                let d = ((x as f64 + 0.5 - px).powi(2) + (y as f64 + 0.5 - py).powi(2)).sqrt();
                if d > radius_px {
                    continue;
                }
                let better = match best {
                    None => true,
                    Some((bd, bm, _, _)) => d < bd || (d == bd && m > bm),
                };
                if better {
                    best = Some((d, m, x, y));
                }
            }
        }
        best.map(|(_, _, x, y)| (x as f64 + 0.5, y as f64 + 0.5))
    }

    /// Nombre de pixels de contour pour ce seuil (affichage et tests).
    pub fn count_edges(&self, threshold: f32) -> usize {
        self.magnitude
            .iter()
            .filter(|m| f32::from(**m) >= threshold)
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect_image(w: usize, h: usize) -> Vec<u8> {
        // Rectangle blanc (255) de x 10..30, y 10..20 sur fond noir.
        let mut v = vec![0u8; w * h];
        for y in 10..20 {
            for x in 10..30 {
                v[y * w + x] = 255;
            }
        }
        v
    }

    #[test]
    fn marche_franche_donne_amplitude_255_et_zone_plane_0() {
        let (w, h) = (40, 30);
        let m = sobel(&rect_image(w, h), w, h, &AtomicBool::new(false))
            .unwrap()
            .unwrap();
        // Pixel juste à l'intérieur du bord gauche (x = 10, y = 15) : marche verticale de 255.
        assert_eq!(m.magnitude[15 * w + 10], 255);
        assert_eq!(m.magnitude[15 * w + 9], 255);
        // Plat au milieu du rectangle et loin dans le fond.
        assert_eq!(m.magnitude[15 * w + 20], 0);
        assert_eq!(m.magnitude[3 * w + 3], 0);
    }

    #[test]
    fn accrochage_au_plus_proche_et_dans_le_rayon() {
        let (w, h) = (40, 30);
        let m = sobel(&rect_image(w, h), w, h, &AtomicBool::new(false))
            .unwrap()
            .unwrap();
        // Clic à 2 px à gauche du bord x = 10 : le contour le plus proche est la colonne x = 9
        // (centre 9,5), sur la ligne du clic.
        let (ex, ey) = m.nearest_edge(7.5, 15.5, 5.0, 64.0).unwrap();
        assert_eq!((ex, ey), (9.5, 15.5));
        // Rayon trop petit : rien.
        assert_eq!(m.nearest_edge(3.5, 15.5, 2.0, 64.0), None);
        // Coin de la boîte de recherche hors du disque : le pixel (9,5 ; 9,5) est à 4,24 px de
        // (6,5 ; 6,5), donc hors du rayon 4 mais dans le rayon 4,3.
        assert_eq!(m.nearest_edge(6.5, 6.5, 4.0, 64.0), None);
        assert_eq!(m.nearest_edge(6.5, 6.5, 4.3, 64.0), Some((9.5, 9.5)));
        // Seuil trop haut : rien.
        assert_eq!(m.nearest_edge(9.5, 15.5, 5.0, 300.0), None);
    }

    #[test]
    fn annulation_et_refus() {
        let (w, h) = (40, 30);
        assert_eq!(
            sobel(&rect_image(w, h), w, h, &AtomicBool::new(true)),
            Ok(None)
        );
        assert!(sobel(&[0; 10], w, h, &AtomicBool::new(false)).is_err());
        assert!(sobel(&[], 0, 0, &AtomicBool::new(false)).is_err());
        // Image plus petite que le noyau : amplitude nulle, pas de panique.
        let tiny = sobel(&[1, 2, 3, 4], 2, 2, &AtomicBool::new(false))
            .unwrap()
            .unwrap();
        assert!(tiny.magnitude.iter().all(|m| *m == 0));
    }
}
