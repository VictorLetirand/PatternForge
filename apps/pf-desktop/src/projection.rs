//! US-13.7 : projection plein écran. Conversion mm <-> pixels pure, sans interface.
//!
//! Calibration : on saisit la largeur **physique** de l'image projetée (mm) ; la largeur de
//! l'écran en pixels physiques est connue. `pixels_par_mm = largeur_px / largeur_mm`. Si la valeur
//! saisie est exacte, 100 mm affichés = 100 mm projetés (**calculé, non mesuré sur un écran ou
//! un mur** : **[HUMAIN]**). Les pixels ne reviennent jamais dans le modèle : tout est dérivé
//! à l'affichage, l'aller-retour mm -> px -> mm est exact aux erreurs d'arrondi flottant près
//! (aucun arrondi n'est appliqué aux valeurs).

pub const MIN_SCREEN_MM: f64 = 50.0;
pub const MAX_SCREEN_MM: f64 = 20_000.0;
pub const MIN_GRID_MM: f64 = 1.0;
pub const MAX_GRID_MM: f64 = 500.0;
pub const MIN_STROKE_MM: f64 = 0.1;
pub const MAX_STROKE_MM: f64 = 20.0;
pub const STANDARD_SQUARE_MM: f64 = 100.0;
/// Au plus ce nombre de traits de grille par direction (sinon l'écran n'est qu'une grille).
pub const MAX_GRID_LINES: f64 = 2000.0;

#[derive(Debug, Clone, PartialEq)]
pub enum ProjectionError {
    ScreenWidth(f64),
    GridStep(f64),
    Stroke(f64),
    ScreenPixels(f64),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProjectionSettings {
    pub screen_width_mm: f64,
    pub grid_step_mm: f64,
    pub stroke_mm: f64,
    pub mirror_h: bool,
    pub mirror_v: bool,
}

impl Default for ProjectionSettings {
    fn default() -> Self {
        ProjectionSettings {
            screen_width_mm: 600.0,
            grid_step_mm: 10.0,
            stroke_mm: 1.0,
            mirror_h: false,
            mirror_v: false,
        }
    }
}

fn in_range(v: f64, lo: f64, hi: f64) -> bool {
    v.is_finite() && (lo..=hi).contains(&v)
}

impl ProjectionSettings {
    pub fn validate(&self) -> Result<(), ProjectionError> {
        if !in_range(self.screen_width_mm, MIN_SCREEN_MM, MAX_SCREEN_MM) {
            return Err(ProjectionError::ScreenWidth(self.screen_width_mm));
        }
        if !in_range(self.grid_step_mm, MIN_GRID_MM, MAX_GRID_MM) {
            return Err(ProjectionError::GridStep(self.grid_step_mm));
        }
        if !in_range(self.stroke_mm, MIN_STROKE_MM, MAX_STROKE_MM) {
            return Err(ProjectionError::Stroke(self.stroke_mm));
        }
        Ok(())
    }
}

/// Pixels physiques par mm ; refuse un réglage invalide ou une largeur de pixels non finie.
pub fn pixels_per_mm(s: &ProjectionSettings, screen_px: f64) -> Result<f64, ProjectionError> {
    s.validate()?;
    if !screen_px.is_finite() || screen_px < 1.0 {
        return Err(ProjectionError::ScreenPixels(screen_px));
    }
    Ok(screen_px / s.screen_width_mm)
}

pub fn mm_to_px(mm: f64, px_per_mm: f64) -> f64 {
    mm * px_per_mm
}

pub fn px_to_mm(px: f64, px_per_mm: f64) -> f64 {
    px / px_per_mm
}

/// Épaisseur du trait en pixels physiques : jamais moins d'un pixel (un trait invisible ne se
/// projette pas), mais la valeur en mm de l'utilisateur n'est pas modifiée.
pub fn stroke_px(s: &ProjectionSettings, px_per_mm: f64) -> f64 {
    mm_to_px(s.stroke_mm, px_per_mm).max(1.0)
}

/// Miroir d'une coordonnée écran dans une zone `size` (px) : x -> size - x si miroir actif.
pub fn mirror(v: f64, size: f64, on: bool) -> f64 {
    if on {
        size - v
    } else {
        v
    }
}

/// Positions (mm) des traits de grille entre 0 et `extent_mm`, ou rien si la grille ferait plus de
/// `MAX_GRID_LINES` traits.
pub fn grid_lines_mm(step_mm: f64, extent_mm: f64) -> Vec<f64> {
    if !step_mm.is_finite() || step_mm <= 0.0 || !extent_mm.is_finite() || extent_mm <= 0.0 {
        return Vec::new();
    }
    let n = (extent_mm / step_mm).floor();
    if n > MAX_GRID_LINES {
        return Vec::new();
    }
    (0..=n as u64).map(|i| i as f64 * step_mm).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(width: f64) -> ProjectionSettings {
        ProjectionSettings {
            screen_width_mm: width,
            ..ProjectionSettings::default()
        }
    }

    #[test]
    fn ecran_de_600_mm_sur_1920_px_donne_320_px_pour_100_mm() {
        // 1920 / 600 = 3,2 px par mm ; 100 mm = 320 px ; 1 px = 1 / 3,2 = 0,3125 mm.
        let k = pixels_per_mm(&s(600.0), 1920.0).unwrap();
        assert_eq!(k, 3.2);
        assert_eq!(mm_to_px(100.0, k), 320.0);
        assert_eq!(px_to_mm(1.0, k), 0.3125);
    }

    #[test]
    fn aller_retour_exact_sur_des_valeurs_choisies() {
        let k = pixels_per_mm(&s(600.0), 1920.0).unwrap();
        for mm in [0.0, 0.3125, 1.0, 12.5, 100.0, 599.9] {
            assert_eq!(px_to_mm(mm_to_px(mm, k), k), mm);
        }
    }

    #[test]
    fn largeur_hors_bornes_ou_non_finie_refusee() {
        for w in [0.0, 49.9, 20_000.1, f64::NAN, f64::INFINITY, -600.0] {
            assert!(matches!(
                pixels_per_mm(&s(w), 1920.0),
                Err(ProjectionError::ScreenWidth(_))
            ));
        }
        assert!(pixels_per_mm(&s(50.0), 1920.0).is_ok());
        assert!(pixels_per_mm(&s(20_000.0), 1920.0).is_ok());
        for px in [0.0, f64::NAN, -5.0, f64::INFINITY] {
            assert!(matches!(
                pixels_per_mm(&s(600.0), px),
                Err(ProjectionError::ScreenPixels(_))
            ));
        }
    }

    #[test]
    fn pas_de_grille_et_trait_hors_bornes_refuses() {
        let base = ProjectionSettings::default();
        let a = ProjectionSettings {
            grid_step_mm: 0.5,
            ..base
        };
        assert_eq!(a.validate(), Err(ProjectionError::GridStep(0.5)));
        let b = ProjectionSettings {
            stroke_mm: 0.05,
            ..base
        };
        assert_eq!(b.validate(), Err(ProjectionError::Stroke(0.05)));
        let c = ProjectionSettings {
            stroke_mm: f64::NAN,
            ..base
        };
        assert!(c.validate().is_err());
    }

    #[test]
    fn le_trait_ne_descend_pas_sous_un_pixel_sans_changer_les_mm() {
        let a = ProjectionSettings {
            stroke_mm: 0.1,
            ..ProjectionSettings::default()
        };
        // 0,1 mm * 3,2 px/mm = 0,32 px -> 1 px ; 2 mm -> 6,4 px.
        assert_eq!(stroke_px(&a, 3.2), 1.0);
        let b = ProjectionSettings {
            stroke_mm: 2.0,
            ..a
        };
        assert_eq!(stroke_px(&b, 3.2), 6.4);
        assert_eq!(a.stroke_mm, 0.1);
    }

    #[test]
    fn miroir_et_grille() {
        assert_eq!(mirror(300.0, 1920.0, true), 1620.0);
        assert_eq!(mirror(300.0, 1920.0, false), 300.0);
        assert_eq!(grid_lines_mm(10.0, 35.0), vec![0.0, 10.0, 20.0, 30.0]);
        assert!(grid_lines_mm(1.0, 5000.0).is_empty());
        assert!(grid_lines_mm(f64::NAN, 5.0).is_empty());
        assert!(grid_lines_mm(0.0, 5.0).is_empty());
    }
}
