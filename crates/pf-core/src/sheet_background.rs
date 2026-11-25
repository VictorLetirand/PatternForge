//! E14 — image de fond d'une feuille : **stockage seulement** (schéma 6, décision 455).
//!
//! `Sheet.background` = référence à une image **extérieure au fichier** (chemin relatif au
//! projet, empreinte SHA-256, dimensions en pixels), sa calibration (deux points + distance connue,
//! ou quatre points + rectangle connu) et son affichage (opacité, visible, verrouillé). L'image
//! n'est **jamais** intégrée au JSON. Aucune commande, aucune interface : la validation décide
//! seulement de ce qu'un fichier peut porter.
//!
//! La validation ne lit pas l'image (elle ne peut pas savoir si l'empreinte est vraie) : elle
//! contrôle la forme (chemin relatif sûr, 64 chiffres hexadécimaux minuscules, dimensions > 0,
//! nombres finis, distances > 0, opacité dans [0 ; 1]).

use crate::error::CoreError;
use crate::model::*;
use serde::{Deserialize, Serialize};

/// Plus grande dimension acceptée d'une image, en pixels.
pub const MAX_BACKGROUND_PX: u32 = 100_000;
/// Longueur maximale du chemin de l'image, en caractères.
pub const MAX_BACKGROUND_PATH_CHARS: usize = 260;
/// Borne des coordonnées et des distances stockées (pixels ou millimètres).
pub const MAX_BACKGROUND_VALUE: f64 = 10_000_000.0;

/// Point à deux coordonnées (pixels de l'image ou millimètres selon le champ qui le porte).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanePoint {
    pub x: f64,
    pub y: f64,
}

/// Calibration de l'image : lien entre ses pixels et les millimètres du plan.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Calibration {
    /// Deux points de l'image distants de `known_distance_mm` dans la réalité.
    TwoPoints {
        points_px: [PlanePoint; 2],
        known_distance_mm: f64,
        /// Écart observé à la calibration (mm), jamais une contrainte.
        residual_mm: f64,
    },
    /// Quatre coins (dans l'ordre du contour) d'un rectangle de `known_width_mm` × `known_height_mm`.
    FourPoints {
        points_px: [PlanePoint; 4],
        known_width_mm: f64,
        known_height_mm: f64,
        residual_mm: f64,
    },
}

/// Image de fond d'une feuille.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Background {
    /// Chemin **relatif au projet**, séparateur `/`, jamais absolu, jamais `..`.
    pub image: String,
    /// Empreinte SHA-256 de l'image : 64 chiffres hexadécimaux minuscules.
    pub sha256: String,
    pub width_px: u32,
    pub height_px: u32,
    pub calibration: Calibration,
    /// Opacité d'affichage dans [0 ; 1].
    pub opacity: f64,
    pub visible: bool,
    pub locked: bool,
}

fn bad(msg: String) -> CoreError {
    CoreError::Invalid(msg)
}

/// Pourquoi un chemin d'image est refusé (`None` = chemin relatif sûr).
pub fn unsafe_image_path(path: &str) -> Option<&'static str> {
    if path.is_empty() || path.trim() != path {
        return Some("vide ou avec des espaces en début ou en fin");
    }
    if path.chars().count() > MAX_BACKGROUND_PATH_CHARS {
        return Some("trop long");
    }
    if path.chars().any(char::is_control) {
        return Some("caractère de contrôle");
    }
    if path.contains('\\') {
        return Some("séparateur « \\ » refusé (écrire « / »)");
    }
    if path.starts_with('/') {
        return Some("chemin absolu");
    }
    if path.contains(':') {
        return Some("chemin absolu ou lecteur (« : » refusé)");
    }
    for seg in path.split('/') {
        match seg {
            "" => return Some("segment vide (« // » ou « / » final)"),
            "." => return Some("segment « . » refusé"),
            ".." => return Some("segment « .. » refusé (sortie du dossier du projet)"),
            _ => {}
        }
    }
    None
}

#[derive(Clone, Copy, PartialEq)]
enum Sign {
    Any,
    Positive,
    NonNegative,
}

fn check_value(v: f64, what: &str, sign: Sign) -> Result<(), CoreError> {
    if !v.is_finite() || v.abs() > MAX_BACKGROUND_VALUE {
        return Err(bad(format!("{what} : valeur non finie ou hors bornes")));
    }
    if sign == Sign::Positive && v <= 0.0 {
        return Err(bad(format!("{what} : doit être > 0 (reçu {v})")));
    }
    if sign == Sign::NonNegative && v < 0.0 {
        return Err(bad(format!("{what} : doit être ≥ 0 (reçu {v})")));
    }
    Ok(())
}

fn check_points(points: &[PlanePoint], what: &str) -> Result<(), CoreError> {
    for (i, p) in points.iter().enumerate() {
        check_value(p.x, &format!("{what} : point {} (x)", i + 1), Sign::Any)?;
        check_value(p.y, &format!("{what} : point {} (y)", i + 1), Sign::Any)?;
        if points[..i].iter().any(|q| q.x == p.x && q.y == p.y) {
            return Err(bad(format!(
                "{what} : le point {} est confondu avec un autre (distance nulle)",
                i + 1
            )));
        }
    }
    Ok(())
}

impl Background {
    /// Contrôle de forme (voir le module) ; `what` nomme la feuille dans les messages.
    pub fn validate(&self, what: &str) -> Result<(), CoreError> {
        let what = format!("{what} : image de fond");
        if let Some(why) = unsafe_image_path(&self.image) {
            return Err(bad(format!(
                "{what} : chemin « {} » refusé ({why})",
                self.image
            )));
        }
        if self.sha256.len() != 64
            || !self
                .sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(bad(format!(
                "{what} : l'empreinte doit avoir 64 chiffres hexadécimaux minuscules"
            )));
        }
        for (v, name) in [(self.width_px, "largeur"), (self.height_px, "hauteur")] {
            if v == 0 || v > MAX_BACKGROUND_PX {
                return Err(bad(format!(
                    "{what} : {name} en pixels hors de [1 ; {MAX_BACKGROUND_PX}] (reçu {v})"
                )));
            }
        }
        if !self.opacity.is_finite() || !(0.0..=1.0).contains(&self.opacity) {
            return Err(bad(format!(
                "{what} : l'opacité doit être comprise entre 0 et 1 (reçu {})",
                self.opacity
            )));
        }
        let what = format!("{what} (calibration)");
        match &self.calibration {
            Calibration::TwoPoints {
                points_px,
                known_distance_mm,
                residual_mm,
            } => {
                check_points(points_px, &what)?;
                check_value(
                    *known_distance_mm,
                    &format!("{what} : distance connue"),
                    Sign::Positive,
                )?;
                check_value(*residual_mm, &format!("{what} : résidu"), Sign::NonNegative)?;
            }
            Calibration::FourPoints {
                points_px,
                known_width_mm,
                known_height_mm,
                residual_mm,
            } => {
                check_points(points_px, &what)?;
                check_value(
                    *known_width_mm,
                    &format!("{what} : largeur connue"),
                    Sign::Positive,
                )?;
                check_value(
                    *known_height_mm,
                    &format!("{what} : hauteur connue"),
                    Sign::Positive,
                )?;
                check_value(*residual_mm, &format!("{what} : résidu"), Sign::NonNegative)?;
            }
        }
        Ok(())
    }
}

impl Project {
    /// Validation des images de fond (appelée par `Project::validate`).
    pub(crate) fn validate_backgrounds(&self) -> Result<(), CoreError> {
        for s in &self.sheets {
            if let Some(b) = &s.background {
                b.validate(&format!("feuille « {} »", s.name))?;
            }
        }
        Ok(())
    }
}
