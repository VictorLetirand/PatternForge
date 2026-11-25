//! US-09.9 — contrôle de forme entre tailles (décision 330).
//!
//! Lecture pure, **rien n'est stocké** (schéma 4 inchangé, format inchangé) : pour chaque tracé
//! courbe de la feuille (Bézier ou courbe lisse), la forme est lue sur
//! [`Project::graded_project`] de **chaque** taille de la gamme (aucune formule de position
//! dupliquée) et analysée par [`pf_geometry::shape::analyze_curvature`] : rebroussements,
//! inflexions, courbure maximale `|κ|` (1/mm) et rayon minimal.
//!
//! **Signalements** (informations, jamais un refus) :
//! - [`ShapeSignalKind::CuspIntroduced`] : la taille de base n'a aucun rebroussement, cette taille
//!   en a (le cas du cahier) ;
//! - [`ShapeSignalKind::InflectionChanged`] : nombre d'inflexions différent de celui de la base ;
//! - [`ShapeSignalKind::CurvatureJump`] : la courbure maximale varie de plus de
//!   [`CURVATURE_JUMP_RATIO`] (50 %) de celle de la taille **précédente** de la gamme (ordre de la
//!   gamme, jamais un tri) ; signalé sur la taille suivante. Une courbure inférieure à
//!   [`pf_geometry::shape::CURVATURE_ZERO_PER_MM`] est « droite » : droite → droite n'est pas un
//!   saut, droite → courbe l'est (variation indéfinie, `percent` vide). Les paires dont une taille
//!   a un rebroussement ne sont pas comparées (la courbure y est indéfinie : le rebroussement est
//!   déjà signalé) ;
//! - [`ShapeSignalKind::Degenerate`] : la courbe ne se lit pas dans cette taille (tous ses points
//!   confondus, ou position / tangente incalculable). Sans lecture de la base, il n'y a pas de
//!   référence : aucun signalement d'écart à la base pour cette courbe.
//!
//! Les seuils et tolérances sont des constantes nommées (convention d'atelier à faire valider par
//! une modéliste, `docs/SUIVI_RESTE_A_FAIRE.md` § 6).

use crate::error::CoreError;
use crate::model::*;
use pf_geometry::shape::{analyze_curvature, CURVATURE_ZERO_PER_MM};
use pf_geometry::GeometryError;
use std::fmt;

/// Variation relative de la courbure maximale entre deux tailles consécutives au-delà de
/// laquelle un saut est signalé (50 % de la valeur de la taille précédente, strictement).
pub const CURVATURE_JUMP_RATIO: f64 = 0.5;

/// Refus d'une lecture de contrôle de forme.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShapeControlError {
    UnknownRange(SizeRangeId),
    EmptyRange(SizeRangeId),
    UnknownSheet(SheetId),
}

impl fmt::Display for ShapeControlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ShapeControlError::UnknownRange(id) => write!(f, "gamme inconnue (id {})", id.0),
            ShapeControlError::EmptyRange(id) => write!(f, "gamme sans taille (id {})", id.0),
            ShapeControlError::UnknownSheet(id) => write!(f, "feuille inconnue (id {})", id.0),
        }
    }
}

impl std::error::Error for ShapeControlError {}

impl From<ShapeControlError> for CoreError {
    fn from(e: ShapeControlError) -> CoreError {
        CoreError::Invalid(e.to_string())
    }
}

/// Nature d'une courbe contrôlée.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CurveKind {
    /// Bézier cubique.
    Bezier,
    /// Courbe lisse par points de passage.
    Smooth,
}

/// Pourquoi une courbe ne se lit pas dans une taille.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DegenerateReason {
    /// Tous les points de la courbe sont confondus (longueur nulle).
    ZeroLength,
    /// Une position ou une tangente ne se calcule pas dans cette taille.
    Unreadable,
}

/// Forme d'une courbe dans une taille.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShapeMetrics {
    pub cusps: usize,
    pub inflections: usize,
    /// Plus grand `|κ|` en 1/mm (0 pour une droite).
    pub max_curvature_per_mm: f64,
    /// Rayon minimal en mm ; `None` si la courbe est tenue pour droite.
    pub min_radius_mm: Option<f64>,
}

/// Lecture d'une courbe dans une taille.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ShapeReading {
    Metrics(ShapeMetrics),
    Degenerate(DegenerateReason),
}

/// Forme d'une courbe dans une taille de la gamme.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CurveSizeShape {
    pub size: SizeId,
    pub reading: ShapeReading,
}

impl CurveSizeShape {
    pub fn metrics(&self) -> Option<&ShapeMetrics> {
        match &self.reading {
            ShapeReading::Metrics(m) => Some(m),
            ShapeReading::Degenerate(_) => None,
        }
    }
}

/// Évolution de la courbure maximale entre deux tailles consécutives.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CurvatureStep {
    /// Suivante − précédente (1/mm).
    pub delta: f64,
    /// `delta` en % de la précédente ; `None` si elle est tenue pour nulle (droite).
    pub percent: Option<f64>,
}

/// Ce qu'un signalement dit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ShapeSignalKind {
    /// Rebroussement absent en taille de base, présent dans cette taille.
    CuspIntroduced { cusps: usize },
    /// Nombre d'inflexions différent de celui de la taille de base.
    InflectionChanged { base: usize, found: usize },
    /// Courbure maximale variée de plus de [`CURVATURE_JUMP_RATIO`] depuis la taille `from`.
    CurvatureJump { from: SizeId, percent: Option<f64> },
    /// Courbe illisible dans cette taille.
    Degenerate { reason: DegenerateReason },
}

/// Un signalement, porté par une taille.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShapeSignal {
    pub size: SizeId,
    pub kind: ShapeSignalKind,
}

/// Contrôle d'une courbe sur toute la gamme.
#[derive(Debug, Clone, PartialEq)]
pub struct CurveShape {
    pub entity: EntityId,
    pub kind: CurveKind,
    /// Une lecture par taille, dans l'ordre de la gamme.
    pub sizes: Vec<CurveSizeShape>,
    /// Un pas par couple de tailles consécutives (`sizes.len() − 1` éléments) ; `None` si l'une
    /// des deux ne se lit pas.
    pub steps: Vec<Option<CurvatureStep>>,
    /// Signalements, dans l'ordre de la gamme puis de leur gravité.
    pub signals: Vec<ShapeSignal>,
}

impl CurveShape {
    /// Aucun signalement.
    pub fn is_clean(&self) -> bool {
        self.signals.is_empty()
    }
}

/// Contrôle de forme d'une feuille pour une gamme.
#[derive(Debug, Clone, PartialEq)]
pub struct ShapeControl {
    pub range: SizeRangeId,
    pub sheet: SheetId,
    /// Tailles dans l'ordre de la gamme.
    pub sizes: Vec<SizeId>,
    pub base: SizeId,
    /// Courbes de la feuille, dans l'ordre de ses tracés.
    pub curves: Vec<CurveShape>,
}

impl ShapeControl {
    /// Courbes signalées.
    pub fn flagged(&self) -> impl Iterator<Item = &CurveShape> {
        self.curves.iter().filter(|c| !c.is_clean())
    }
}

/// Lecture d'une courbe dans un projet déjà évalué dans une taille.
fn read_curve(sheet: &Sheet, entity: EntityId) -> ShapeReading {
    let segments = match sheet.shape(entity) {
        Ok(Shape::Cubic(c)) => vec![c],
        Ok(Shape::Spline(s)) => s.segments().to_vec(),
        _ => return ShapeReading::Degenerate(DegenerateReason::Unreadable),
    };
    match analyze_curvature(&segments) {
        Ok(r) => ShapeReading::Metrics(ShapeMetrics {
            cusps: r.cusps,
            inflections: r.inflections,
            max_curvature_per_mm: r.max_curvature_per_mm,
            min_radius_mm: (r.max_curvature_per_mm >= CURVATURE_ZERO_PER_MM)
                .then(|| 1.0 / r.max_curvature_per_mm),
        }),
        Err(GeometryError::DegenerateCurve) => {
            ShapeReading::Degenerate(DegenerateReason::ZeroLength)
        }
        Err(_) => ShapeReading::Degenerate(DegenerateReason::Unreadable),
    }
}

/// Évolution de la courbure maximale de `a` à `b` et saut éventuel (`Some(percent)` quand il y a
/// saut). Fonction pure : c'est la définition documentée en tête de module.
pub fn curvature_step(a: &ShapeMetrics, b: &ShapeMetrics) -> (CurvatureStep, bool) {
    let (ka, kb) = (a.max_curvature_per_mm, b.max_curvature_per_mm);
    let delta = kb - ka;
    if ka < CURVATURE_ZERO_PER_MM {
        let jump = kb >= CURVATURE_ZERO_PER_MM;
        return (
            CurvatureStep {
                delta,
                percent: None,
            },
            jump,
        );
    }
    let ratio = delta / ka;
    (
        CurvatureStep {
            delta,
            percent: Some(ratio * 100.0),
        },
        delta.abs() > CURVATURE_JUMP_RATIO * ka,
    )
}

impl Project {
    /// Contrôle de forme des courbes de `sheet` dans chaque taille de `range`. Lecture seule,
    /// recalculée à chaque appel, jamais stockée. Gamme inconnue ou sans taille, feuille inconnue :
    /// erreur typée.
    pub fn shape_control(
        &self,
        sheet: SheetId,
        range: SizeRangeId,
    ) -> Result<ShapeControl, ShapeControlError> {
        let r = self
            .sizes
            .iter()
            .find(|r| r.id == range)
            .ok_or(ShapeControlError::UnknownRange(range))?;
        if r.sizes.is_empty() {
            return Err(ShapeControlError::EmptyRange(range));
        }
        let written = self
            .sheet(sheet)
            .map_err(|_| ShapeControlError::UnknownSheet(sheet))?;
        let sizes: Vec<SizeId> = r.sizes.iter().map(|s| s.id).collect();
        let curves: Vec<(EntityId, CurveKind)> = written
            .entities
            .iter()
            .filter_map(|e| match e.kind {
                EntityKind::Curve { .. } => Some((e.id, CurveKind::Bezier)),
                EntityKind::Spline { .. } => Some((e.id, CurveKind::Smooth)),
                _ => None,
            })
            .collect();
        // Un projet évalué par taille ; une taille qui ne s'évalue pas rend ses courbes illisibles.
        let graded: Vec<Option<Project>> = sizes
            .iter()
            .map(|s| self.graded_project(Some(*s)).ok())
            .collect();
        let base_index = sizes.iter().position(|s| *s == r.base);
        let curves = curves
            .into_iter()
            .map(|(entity, kind)| {
                let shapes: Vec<CurveSizeShape> = sizes
                    .iter()
                    .zip(&graded)
                    .map(|(size, g)| CurveSizeShape {
                        size: *size,
                        reading: match g.as_ref().and_then(|g| g.sheet(sheet).ok()) {
                            Some(s) => read_curve(s, entity),
                            None => ShapeReading::Degenerate(DegenerateReason::Unreadable),
                        },
                    })
                    .collect();
                control_curve(entity, kind, shapes, base_index)
            })
            .collect();
        Ok(ShapeControl {
            range,
            sheet,
            sizes,
            base: r.base,
            curves,
        })
    }
}

/// Pas et signalements d'une courbe d'après ses lectures par taille.
fn control_curve(
    entity: EntityId,
    kind: CurveKind,
    shapes: Vec<CurveSizeShape>,
    base_index: Option<usize>,
) -> CurveShape {
    let base = base_index
        .and_then(|i| shapes.get(i))
        .and_then(CurveSizeShape::metrics)
        .copied();
    let steps: Vec<Option<CurvatureStep>> = shapes
        .windows(2)
        .map(|w| Some(curvature_step(w[0].metrics()?, w[1].metrics()?).0))
        .collect();
    let mut signals = Vec::new();
    for (i, s) in shapes.iter().enumerate() {
        match &s.reading {
            ShapeReading::Degenerate(reason) => signals.push(ShapeSignal {
                size: s.size,
                kind: ShapeSignalKind::Degenerate { reason: *reason },
            }),
            ShapeReading::Metrics(m) => {
                if let (Some(b), Some(bi)) = (base, base_index) {
                    if i != bi {
                        if b.cusps == 0 && m.cusps > 0 {
                            signals.push(ShapeSignal {
                                size: s.size,
                                kind: ShapeSignalKind::CuspIntroduced { cusps: m.cusps },
                            });
                        }
                        if m.inflections != b.inflections {
                            signals.push(ShapeSignal {
                                size: s.size,
                                kind: ShapeSignalKind::InflectionChanged {
                                    base: b.inflections,
                                    found: m.inflections,
                                },
                            });
                        }
                    }
                }
                if i > 0 {
                    if let Some(prev) = shapes[i - 1].metrics() {
                        if prev.cusps == 0 && m.cusps == 0 {
                            let (step, jump) = curvature_step(prev, m);
                            if jump {
                                signals.push(ShapeSignal {
                                    size: s.size,
                                    kind: ShapeSignalKind::CurvatureJump {
                                        from: shapes[i - 1].size,
                                        percent: step.percent,
                                    },
                                });
                            }
                        }
                    }
                }
            }
        }
    }
    CurveShape {
        entity,
        kind,
        sizes: shapes,
        steps,
        signals,
    }
}
