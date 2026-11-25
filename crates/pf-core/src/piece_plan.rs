//! Plan de pièces (US-04.5) : vue de **mise en page** de toutes les pièces du projet.
//!
//! Chaque pièce y a un placement : une translation et une rotation appliquées à
//! l'affichage et aux mesures du plan, **jamais** aux tracés, points, crans ni cotations
//! de la construction. Le placement manuel est une donnée du projet (`Piece.placement`,
//! omise du fichier quand absente) ; sans lui, le placement automatique est calculé à la
//! lecture, jamais stocké.
//!
//! Convention : `Placement { x_mm, y_mm, rotation_deg }` place le **centre de la boîte
//! englobante du contour** (en coordonnées de construction) au point `(x, y)` du plan, puis
//! tourne la pièce de `rotation_deg` (sens trigonométrique, Y vers le haut) autour de ce
//! centre. Tourner une pièce ne déplace donc pas son centre.
//!
//! Limites : les boîtes englobantes sont celles de la polyligne d'approximation du
//! contour (48 pas par courbe) ; le chevauchement signalé est celui des boîtes, pas un
//! recouvrement exact des contours.

use crate::contour::{contour_polyline, ContourEdge};
use crate::error::CoreError;
use crate::model::{Piece, PieceId, Project};
use pf_geometry::{sin_cos_deg, Bbox, PointMm, MAX_COORD_MM};
use serde::{Deserialize, Serialize};

/// Espace laissé entre deux boîtes par le placement automatique (mm).
pub const PLAN_GAP_MM: f64 = 20.0;
/// Largeur d'une étagère du placement automatique (mm) : on passe à la rangée
/// suivante quand la pièce suivante dépasserait cette largeur (une pièce plus large
/// occupe seule sa rangée).
pub const PLAN_WIDTH_MM: f64 = 2000.0;
/// Borne de la rotation acceptée en entrée (degrés, avant normalisation).
pub const MAX_PLACEMENT_ROTATION_INPUT_DEG: f64 = 1.0e6;
/// Pas d'échantillonnage des courbes pour la polyligne du plan.
const POLYLINE_SAMPLES: usize = 48;

/// Placement d'une pièce dans le plan (voir la convention du module).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Placement {
    /// Abscisse du centre de la boîte englobante de la pièce, en mm (|x| ≤ 1 000 000).
    pub x_mm: f64,
    /// Ordonnée du centre de la boîte englobante de la pièce, en mm (|y| ≤ 1 000 000).
    pub y_mm: f64,
    /// Rotation en degrés, sens trigonométrique. Stockée dans `[0 ; 360[`.
    pub rotation_deg: f64,
}

/// Angle ramené dans `[0 ; 360[`. `rem_euclid` peut rendre exactement 360 pour un
/// très petit angle négatif (défaut D-03) ; `+ 0.0` change `-0.0` en `0.0`.
pub fn normalize_rotation_deg(angle_deg: f64) -> f64 {
    let r = angle_deg.rem_euclid(360.0);
    if r >= 360.0 {
        0.0
    } else {
        r + 0.0
    }
}

impl Placement {
    /// Valeurs finies et bornées (position ≤ 10⁶ mm, rotation ≤ 10⁶°), rotation ramenée
    /// dans `[0 ; 360[`, zéros négatifs écrasés.
    pub fn normalized(self) -> Result<Placement, CoreError> {
        if !self.x_mm.is_finite() || !self.y_mm.is_finite() {
            return Err(CoreError::Invalid(
                "placement : position non finie".to_string(),
            ));
        }
        if self.x_mm.abs() > MAX_COORD_MM || self.y_mm.abs() > MAX_COORD_MM {
            return Err(CoreError::Invalid(format!(
                "placement : position hors de l'espace de travail (±{MAX_COORD_MM} mm)"
            )));
        }
        if !self.rotation_deg.is_finite() {
            return Err(CoreError::Invalid(
                "placement : rotation non finie".to_string(),
            ));
        }
        if self.rotation_deg.abs() > MAX_PLACEMENT_ROTATION_INPUT_DEG {
            return Err(CoreError::Invalid(format!(
                "placement : rotation hors bornes (±{MAX_PLACEMENT_ROTATION_INPUT_DEG} °)"
            )));
        }
        Ok(Placement {
            x_mm: self.x_mm + 0.0,
            y_mm: self.y_mm + 0.0,
            rotation_deg: normalize_rotation_deg(self.rotation_deg),
        })
    }

    /// Placement tel qu'il doit être stocké : normalisé (rotation dans `[0 ; 360[`).
    pub(crate) fn validate_stored(&self) -> Result<(), CoreError> {
        let n = self.normalized()?;
        if n.rotation_deg != self.rotation_deg {
            return Err(CoreError::Invalid(format!(
                "placement : rotation {} hors de [0 ; 360[",
                self.rotation_deg
            )));
        }
        Ok(())
    }

    /// Image d'un point de construction : centre de boîte `origin` ramené en `(x, y)`,
    /// rotation autour de ce centre. Rotation exacte pour les multiples de 90°.
    pub fn apply(&self, origin: PointMm, p: PointMm) -> PointMm {
        let (s, c) = sin_cos_deg(self.rotation_deg);
        let (dx, dy) = (p.x - origin.x, p.y - origin.y);
        PointMm::new(self.x_mm + c * dx - s * dy, self.y_mm + s * dx + c * dy)
    }
}

/// Pourquoi une pièce n'est pas dessinée dans le plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanSkip {
    /// La pièce n'a pas de contour (pièce-étiquette).
    EmptyOutline,
    /// Le contour existe mais ne peut pas être lu (tracé dégénéré).
    Unreadable,
}

impl PlanSkip {
    /// Identifiant stable, indépendant de la langue.
    pub fn key(&self) -> &'static str {
        match self {
            PlanSkip::EmptyOutline => "empty_outline",
            PlanSkip::Unreadable => "outline_unreadable",
        }
    }
}

/// Une pièce dans le plan, en lecture seule.
#[derive(Debug, Clone, PartialEq)]
pub struct PlanItem {
    pub piece: PieceId,
    pub name: String,
    /// Vrai si le placement vient de `Piece.placement` ; faux = placement automatique.
    pub manual: bool,
    /// Placement effectif (manuel ou automatique) ; `None` si la pièce n'est pas dessinée.
    pub placement: Option<Placement>,
    /// Centre de la boîte englobante du contour, en coordonnées de construction
    /// (point de référence du placement).
    pub origin: Option<PointMm>,
    /// Contour transformé par le placement (polyligne, sans point final dupliqué).
    pub outline: Vec<PointMm>,
    /// Boîte englobante du contour transformé, dans le plan.
    pub bbox: Option<Bbox>,
    /// Aire en mm² (invariante par placement) ; `None` si le contour n'est pas mesurable.
    pub area_mm2: Option<f64>,
    /// Périmètre en mm (invariant par placement).
    pub perimeter_mm: Option<f64>,
    /// Raison pour laquelle la pièce n'est pas dessinée.
    pub skipped: Option<PlanSkip>,
}

/// Contour d'une pièce en coordonnées de construction, avec sa boîte.
struct Source {
    polyline: Vec<PointMm>,
    bbox: Bbox,
    area_mm2: f64,
    perimeter_mm: f64,
}

fn bbox_of(points: &[PointMm]) -> Option<Bbox> {
    let (first, rest) = points.split_first()?;
    let mut bb = Bbox::from_point(*first);
    rest.iter().for_each(|p| bb.include(*p));
    Some(bb)
}

fn centre(b: &Bbox) -> PointMm {
    PointMm::new((b.min.x + b.max.x) / 2.0, (b.min.y + b.max.y) / 2.0)
}

impl Project {
    fn plan_source(&self, piece: &Piece) -> Result<Source, PlanSkip> {
        if piece.outline.is_empty() {
            return Err(PlanSkip::EmptyOutline);
        }
        let contour = self
            .piece_contour(piece.id)
            .map_err(|_| PlanSkip::Unreadable)?;
        let sheet = self
            .piece_sheet(piece.id)
            .map_err(|_| PlanSkip::Unreadable)?;
        let edges: Vec<ContourEdge> = piece
            .outline
            .iter()
            .map(|e| ContourEdge {
                entity: e.entity,
                reversed: e.reversed,
            })
            .collect();
        let polyline =
            contour_polyline(sheet, &edges, POLYLINE_SAMPLES).map_err(|_| PlanSkip::Unreadable)?;
        let bbox = bbox_of(&polyline).ok_or(PlanSkip::Unreadable)?;
        Ok(Source {
            polyline,
            bbox,
            area_mm2: contour.area_mm2,
            perimeter_mm: contour.perimeter_mm,
        })
    }

    /// Mise en page du projet : une entrée par pièce (dans l'ordre du projet), avec le
    /// contour transformé par son placement effectif. Lecture pure.
    ///
    /// Placement automatique (pièce sans `placement`) : étagères, pièces dessinées triées
    /// par identifiant, boîtes de contour non tournées, écart [`PLAN_GAP_MM`], largeur
    /// [`PLAN_WIDTH_MM`], rangées vers le bas à partir de l'origine. Il est calculé avec
    /// **toutes** les pièces dessinables, même celles placées à la main : déplacer une
    /// pièce ne fait donc pas bouger les autres.
    pub fn plan_layout(&self) -> Vec<PlanItem> {
        let sources: Vec<(&Piece, Result<Source, PlanSkip>)> = self
            .pieces
            .iter()
            .map(|p| (p, self.plan_source(p)))
            .collect();
        // Placement automatique : centres par identifiant de pièce.
        let mut order: Vec<usize> = (0..sources.len())
            .filter(|&i| sources[i].1.is_ok())
            .collect();
        order.sort_by_key(|&i| sources[i].0.id);
        let mut auto: Vec<Option<PointMm>> = vec![None; sources.len()];
        let (mut x, mut row_top, mut row_h) = (0.0_f64, 0.0_f64, 0.0_f64);
        for i in order {
            let Ok(src) = &sources[i].1 else { continue };
            let (w, h) = (src.bbox.width(), src.bbox.height());
            if x > 0.0 && x + w > PLAN_WIDTH_MM {
                row_top -= row_h + PLAN_GAP_MM;
                x = 0.0;
                row_h = 0.0;
            }
            auto[i] = Some(PointMm::new(x + w / 2.0, row_top - h / 2.0));
            x += w + PLAN_GAP_MM;
            row_h = row_h.max(h);
        }
        sources
            .into_iter()
            .enumerate()
            .map(|(i, (piece, source))| match source {
                Err(skip) => PlanItem {
                    piece: piece.id,
                    name: piece.name.clone(),
                    manual: piece.placement.is_some(),
                    placement: piece.placement,
                    origin: None,
                    outline: Vec::new(),
                    bbox: None,
                    area_mm2: None,
                    perimeter_mm: None,
                    skipped: Some(skip),
                },
                Ok(src) => {
                    let origin = centre(&src.bbox);
                    let placement = piece.placement.unwrap_or_else(|| {
                        let c = auto[i].unwrap_or(origin);
                        Placement {
                            x_mm: c.x,
                            y_mm: c.y,
                            rotation_deg: 0.0,
                        }
                    });
                    let outline: Vec<PointMm> = src
                        .polyline
                        .iter()
                        .map(|p| placement.apply(origin, *p))
                        .collect();
                    PlanItem {
                        piece: piece.id,
                        name: piece.name.clone(),
                        manual: piece.placement.is_some(),
                        placement: Some(placement),
                        origin: Some(origin),
                        bbox: bbox_of(&outline),
                        outline,
                        area_mm2: Some(src.area_mm2),
                        perimeter_mm: Some(src.perimeter_mm),
                        skipped: None,
                    }
                }
            })
            .collect()
    }

    /// Paires de pièces dont les **boîtes englobantes** se recouvrent sur une surface non
    /// nulle (information, jamais un refus). Deux contours dont les boîtes se
    /// chevauchent ne se recouvrent pas forcément : pas de détection exacte.
    pub fn plan_overlaps(&self) -> Vec<(PieceId, PieceId)> {
        overlapping_boxes(&self.plan_layout())
    }
}

/// Paires (dans l'ordre des entrées) dont les boîtes se recouvrent sur une surface non nulle.
pub fn overlapping_boxes(items: &[PlanItem]) -> Vec<(PieceId, PieceId)> {
    const EPS: f64 = 1e-9;
    let mut out = Vec::new();
    for (i, a) in items.iter().enumerate() {
        let Some(ba) = a.bbox else { continue };
        for b in &items[i + 1..] {
            let Some(bb) = b.bbox else { continue };
            let apart_x = ba.min.x >= bb.max.x - EPS || bb.min.x >= ba.max.x - EPS;
            let apart_y = ba.min.y >= bb.max.y - EPS || bb.min.y >= ba.max.y - EPS;
            if !apart_x && !apart_y {
                out.push((a.piece, b.piece));
            }
        }
    }
    out
}
