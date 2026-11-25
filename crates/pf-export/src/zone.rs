//! Zones de parage, de collage et de découpe interne (US-06.10) dans les exports : contour de la
//! surface, **motif** propre au type (hachures, points, croisillons), **couleur** et **étiquette**
//! texte (ASCII). Un type n'est jamais reconnaissable par la seule couleur : le PDF, en niveaux
//! de gris, et le HPGL, sans couleur, gardent le motif et l'étiquette.
//!
//! Les motifs sont calculés ici, en mm, par `pf_geometry::hatch` (hachures découpées par le
//! polygone) : aucun calcul n'est refait dans l'interface ni dans un format.

use pf_core::ZoneKind;
use pf_geometry::hatch::{dot_grid, hatch_segments, interior_point};
use pf_geometry::PointMm;

/// Épaisseur du contour d'une zone (mm).
pub const ZONE_STROKE_MM: f64 = 0.25;
/// Épaisseur d'une hachure (mm).
pub const ZONE_HATCH_MM: f64 = 0.15;
/// Rayon d'un point du motif de collage (mm).
pub const ZONE_DOT_RADIUS_MM: f64 = 0.25;
/// Taille du texte de l'étiquette (mm de hauteur de corps).
pub const ZONE_LABEL_MM: f64 = 3.0;

/// Zone résolue en mm (surface fermée ; le dernier sommet n'est pas répété).
#[derive(Debug, Clone, PartialEq)]
pub struct SceneZone {
    pub kind: ZoneKind,
    pub polygon: Vec<PointMm>,
    /// Largeur saisie d'une bande ; `None` pour un polygone libre.
    pub width_mm: Option<f64>,
}

/// Dessin d'une zone : contour fermé, hachures, points et point d'ancrage de l'étiquette.
#[derive(Debug, Clone, PartialEq)]
pub struct ZonePattern {
    pub hatch: Vec<(PointMm, PointMm)>,
    pub dots: Vec<PointMm>,
    pub label_at: Option<PointMm>,
}

impl SceneZone {
    /// Identifiant du type dans les fichiers exportés (attribut `data-zone` du SVG).
    pub fn id(&self) -> &'static str {
        match self.kind {
            ZoneKind::Skive => "parage",
            ZoneKind::Glue => "collage",
            ZoneKind::InnerCut => "decoupe-interne",
        }
    }

    /// Étiquette texte (ASCII : le PDF n'embarque qu'un sous-ensemble de police).
    pub fn label(&self) -> &'static str {
        match self.kind {
            ZoneKind::Skive => "PARAGE",
            ZoneKind::Glue => "COLLAGE",
            ZoneKind::InnerCut => "DECOUPE INTERNE",
        }
    }

    /// Couleur (SVG).
    pub fn color(&self) -> &'static str {
        match self.kind {
            ZoneKind::Skive => "#B45309",
            ZoneKind::Glue => "#15803D",
            ZoneKind::InnerCut => "#7E22CE",
        }
    }

    /// Niveau de gris (PDF) : distinct par type.
    pub fn gray(&self) -> f64 {
        match self.kind {
            ZoneKind::Skive => 0.25,
            ZoneKind::Glue => 0.45,
            ZoneKind::InnerCut => 0.0,
        }
    }

    /// Pointillé (mm) du contour : plein, tirets, tiret-point.
    pub fn dash(&self) -> &'static [f64] {
        match self.kind {
            ZoneKind::Skive => &[],
            ZoneKind::Glue => &[2.0, 1.0],
            ZoneKind::InnerCut => &[4.0, 1.0, 0.8, 1.0],
        }
    }

    /// Hachures, points et ancrage de l'étiquette : parage = hachures à 45°, collage = grille de
    /// points, découpe interne = croisillons (45° et 135°).
    pub fn pattern(&self) -> ZonePattern {
        let mut hatch = Vec::new();
        let mut dots = Vec::new();
        match self.kind {
            ZoneKind::Skive => hatch = hatch_segments(&self.polygon, 45.0, 2.0),
            ZoneKind::Glue => dots = dot_grid(&self.polygon, 2.5),
            ZoneKind::InnerCut => {
                hatch = hatch_segments(&self.polygon, 45.0, 3.0);
                hatch.extend(hatch_segments(&self.polygon, 135.0, 3.0));
            }
        }
        ZonePattern {
            hatch,
            dots,
            label_at: interior_point(&self.polygon),
        }
    }

    /// Côtés du contour fermé.
    pub fn edges(&self) -> Vec<(PointMm, PointMm)> {
        let n = self.polygon.len();
        (0..n)
            .map(|i| (self.polygon[i], self.polygon[(i + 1) % n]))
            .collect()
    }
}
