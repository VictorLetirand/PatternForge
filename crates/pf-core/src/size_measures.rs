//! US-09.6a — mesures par taille (décision 328).
//!
//! Lecture pure, **rien n'est stocké** (schéma 4 inchangé) : pour chaque taille d'une gamme, le
//! projet est évalué par [`Project::graded_project`] (le même que celui de
//! [`Project::derived_for_size`]) et les lectures ordinaires du cœur sont appelées dessus
//! (`path_length_mm`, `piece_contour`, `piece_cut_contour`, `seam_pair_report`) : aucune formule
//! n'est dupliquée. Les lignes sont déterminées par la **structure de la taille de base** ; une
//! cellule illisible dans une taille (tracé dégénéré, pièce ouverte, couple non calculable) est
//! `None`, jamais devinée.
//!
//! Évolution : pour deux tailles consécutives (ordre de la gamme, jamais un tri), le pas est
//! `valeur(suivante) − valeur(précédente)`, en mm (mm² pour une aire) et en % de la valeur
//! précédente (`None` si elle est nulle).
//!
//! **Irrégularité** (convention d'atelier à faire valider par une modéliste) : une ligne d'au moins
//! [`IRREGULARITY_MIN_SIZES`] tailles, **toutes lisibles**, est signalée quand
//! 1. deux pas consécutifs sont de **signes opposés** (chacun de valeur absolue > [`EPSILON`]) :
//!    la mesure croît puis décroît (ou l'inverse) le long de la gamme ; ou
//! 2. un pas s'écarte de la **médiane des pas** de la ligne de plus de
//!    [`IRREGULARITY_MEDIAN_RATIO`] × |médiane| (50 % par défaut), avec un plancher de [`EPSILON`]
//!    (une ligne dont tous les pas sont nuls n'est pas signalée ; une médiane nulle signale tout
//!    pas non nul). La médiane d'un nombre pair de pas est la moyenne des deux pas centraux.
//!
//! (Un changement de signe implique toujours un écart à la médiane supérieur au seuil : la règle 1
//! ne change donc pas quelles lignes sont signalées, elle en donne le **motif**.)
//!
//! Avec moins de trois tailles, ou une taille illisible, aucune alerte n'est calculée. Une alerte
//! est une **information**, jamais un refus ni une erreur de validation.

use crate::error::CoreError;
use crate::model::*;
use crate::seam_pair::{SeamPairReading, SeamWhich};
use std::fmt;

/// Nombre minimal de tailles pour évaluer l'irrégularité d'une ligne.
pub const IRREGULARITY_MIN_SIZES: usize = 3;
/// Écart relatif à la médiane des pas au-delà duquel un pas est irrégulier (50 %).
pub const IRREGULARITY_MEDIAN_RATIO: f64 = 0.5;
/// Plancher absolu (mm ou mm²) en dessous duquel un pas est considéré comme nul.
pub const EPSILON: f64 = 1e-6;

/// Refus d'une lecture de mesures par taille.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SizeMeasuresError {
    UnknownRange(SizeRangeId),
    EmptyRange(SizeRangeId),
}

impl fmt::Display for SizeMeasuresError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SizeMeasuresError::UnknownRange(id) => write!(f, "gamme inconnue (id {})", id.0),
            SizeMeasuresError::EmptyRange(id) => write!(f, "gamme sans taille (id {})", id.0),
        }
    }
}

impl std::error::Error for SizeMeasuresError {}

impl From<SizeMeasuresError> for CoreError {
    fn from(e: SizeMeasuresError) -> CoreError {
        CoreError::Invalid(e.to_string())
    }
}

/// Ce que mesure une ligne.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeasureKind {
    /// Périmètre du contour de couture d'une pièce fermée.
    PiecePerimeter { piece: PieceId },
    /// Aire du contour de couture (pièce fermée, sans auto-intersection).
    PieceArea { piece: PieceId },
    /// Périmètre du contour de coupe (seulement si la pièce a une marge de couture).
    PieceCutPerimeter { piece: PieceId },
    /// Longueur de couture d'un côté d'un couple apparié (E08).
    SeamLength { pair: SeamPairId, side: SeamWhich },
    /// Écart d'assemblage d'un couple : longueur A − longueur B (signé, comme l'embu mesuré).
    SeamGap { pair: SeamPairId },
    /// Longueur développée d'un tracé d'une feuille.
    PathLength { sheet: SheetId, entity: EntityId },
}

/// Unité d'une mesure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeasureUnit {
    Mm,
    Mm2,
}

/// Famille d'une ligne (regroupement d'affichage).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MeasureGroup {
    Pieces,
    Seams,
    Paths,
}

impl MeasureKind {
    pub fn unit(&self) -> MeasureUnit {
        match self {
            MeasureKind::PieceArea { .. } => MeasureUnit::Mm2,
            _ => MeasureUnit::Mm,
        }
    }

    pub fn group(&self) -> MeasureGroup {
        match self {
            MeasureKind::PiecePerimeter { .. }
            | MeasureKind::PieceArea { .. }
            | MeasureKind::PieceCutPerimeter { .. } => MeasureGroup::Pieces,
            MeasureKind::SeamLength { .. } | MeasureKind::SeamGap { .. } => MeasureGroup::Seams,
            MeasureKind::PathLength { .. } => MeasureGroup::Paths,
        }
    }
}

/// Pas entre deux tailles consécutives.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MeasureStep {
    /// Suivante − précédente (mm ou mm²).
    pub delta: f64,
    /// `delta` en % de la valeur précédente ; `None` si elle est nulle.
    pub percent: Option<f64>,
}

/// Pourquoi une ligne est signalée.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Irregularity {
    /// Deux pas consécutifs de signes opposés.
    pub sign_change: bool,
    /// Au moins un pas trop éloigné de la médiane des pas.
    pub off_median: bool,
    /// Indices (à partir de 0, dans `steps`) des pas en cause.
    pub steps: Vec<usize>,
}

/// Une ligne du tableau : une mesure, une valeur par taille de la gamme.
#[derive(Debug, Clone, PartialEq)]
pub struct SizeMeasureRow {
    pub kind: MeasureKind,
    /// Une valeur par taille, dans l'ordre de la gamme ; `None` = non lisible dans cette taille.
    pub values: Vec<Option<f64>>,
    /// Un pas par couple de tailles consécutives (`values.len() − 1` éléments).
    pub steps: Vec<Option<MeasureStep>>,
    /// Pas moyen ; `None` si une valeur manque ou s'il n'y a qu'une taille.
    pub mean_step: Option<f64>,
    /// Moyenne des pas en % ; `None` si un pas n'a pas de pourcentage.
    pub mean_step_percent: Option<f64>,
    /// `Some` seulement si la ligne est signalée.
    pub irregularity: Option<Irregularity>,
}

/// Tableau des mesures d'une gamme.
#[derive(Debug, Clone, PartialEq)]
pub struct SizeMeasures {
    pub range: SizeRangeId,
    /// Tailles dans l'ordre de la gamme.
    pub sizes: Vec<SizeId>,
    pub base: SizeId,
    pub rows: Vec<SizeMeasureRow>,
}

impl SizeMeasures {
    /// Lignes signalées.
    pub fn irregular_rows(&self) -> impl Iterator<Item = &SizeMeasureRow> {
        self.rows.iter().filter(|r| r.irregularity.is_some())
    }
}

/// Pas, pas moyens et irrégularité d'une suite de valeurs (une par taille, dans l'ordre).
/// Fonction pure : c'est la définition documentée en tête de module.
pub fn analyze_series(values: &[Option<f64>]) -> SeriesAnalysis {
    let steps: Vec<Option<MeasureStep>> = values
        .windows(2)
        .map(|w| match (w[0], w[1]) {
            (Some(a), Some(b)) if a.is_finite() && b.is_finite() => Some(MeasureStep {
                delta: b - a,
                percent: (a.abs() > EPSILON).then(|| (b - a) / a.abs() * 100.0),
            }),
            _ => None,
        })
        .collect();
    let complete = !steps.is_empty() && steps.iter().all(Option::is_some);
    let deltas: Vec<f64> = steps.iter().flatten().map(|s| s.delta).collect();
    let mean_step = complete.then(|| deltas.iter().sum::<f64>() / deltas.len() as f64);
    let percents: Vec<f64> = steps.iter().flatten().filter_map(|s| s.percent).collect();
    let mean_step_percent = (complete && percents.len() == steps.len())
        .then(|| percents.iter().sum::<f64>() / percents.len() as f64);
    let irregularity = if complete && values.len() >= IRREGULARITY_MIN_SIZES {
        irregularity_of(&deltas)
    } else {
        None
    };
    SeriesAnalysis {
        steps,
        mean_step,
        mean_step_percent,
        irregularity,
    }
}

/// Résultat de [`analyze_series`].
#[derive(Debug, Clone, PartialEq)]
pub struct SeriesAnalysis {
    pub steps: Vec<Option<MeasureStep>>,
    pub mean_step: Option<f64>,
    pub mean_step_percent: Option<f64>,
    pub irregularity: Option<Irregularity>,
}

/// Médiane (moyenne des deux centraux si le nombre est pair) ; `deltas` non vide.
fn median(deltas: &[f64]) -> f64 {
    let mut s = deltas.to_vec();
    s.sort_by(|a, b| a.total_cmp(b));
    let n = s.len();
    if n % 2 == 1 {
        s[n / 2]
    } else {
        (s[n / 2 - 1] + s[n / 2]) / 2.0
    }
}

fn irregularity_of(deltas: &[f64]) -> Option<Irregularity> {
    let mut flagged = vec![false; deltas.len()];
    let mut sign_change = false;
    for (i, w) in deltas.windows(2).enumerate() {
        if (w[0] > EPSILON && w[1] < -EPSILON) || (w[0] < -EPSILON && w[1] > EPSILON) {
            sign_change = true;
            flagged[i] = true;
            flagged[i + 1] = true;
        }
    }
    let m = median(deltas);
    let threshold = (IRREGULARITY_MEDIAN_RATIO * m.abs()).max(EPSILON);
    let mut off_median = false;
    for (i, d) in deltas.iter().enumerate() {
        if (d - m).abs() > threshold {
            off_median = true;
            flagged[i] = true;
        }
    }
    (sign_change || off_median).then(|| Irregularity {
        sign_change,
        off_median,
        steps: flagged
            .iter()
            .enumerate()
            .filter_map(|(i, f)| f.then_some(i))
            .collect(),
    })
}

impl Project {
    /// Tableau des mesures de la gamme `range` : lignes = mesures (structure de la taille de
    /// base), colonnes = tailles dans l'ordre de la gamme. Lecture seule, recalculée à chaque
    /// appel, jamais stockée. Gamme inconnue ou sans taille = erreur typée.
    pub fn size_measures(&self, range: SizeRangeId) -> Result<SizeMeasures, SizeMeasuresError> {
        let r = self
            .sizes
            .iter()
            .find(|r| r.id == range)
            .ok_or(SizeMeasuresError::UnknownRange(range))?;
        if r.sizes.is_empty() {
            return Err(SizeMeasuresError::EmptyRange(range));
        }
        let sizes: Vec<SizeId> = r.sizes.iter().map(|s| s.id).collect();
        let kinds = self.measure_kinds();
        // Un projet évalué par taille ; une taille qui ne s'évalue pas laisse ses cellules vides.
        let graded: Vec<Option<(Project, Vec<SeamPairReading>)>> = sizes
            .iter()
            .map(|s| {
                self.graded_project(Some(*s)).ok().map(|g| {
                    let seams = g.seam_pair_report();
                    (g, seams)
                })
            })
            .collect();
        let rows = kinds
            .into_iter()
            .map(|kind| {
                let values: Vec<Option<f64>> = graded
                    .iter()
                    .map(|g| {
                        g.as_ref()
                            .and_then(|(g, seams)| g.measure_value(kind, seams))
                    })
                    .collect();
                let a = analyze_series(&values);
                SizeMeasureRow {
                    kind,
                    values,
                    steps: a.steps,
                    mean_step: a.mean_step,
                    mean_step_percent: a.mean_step_percent,
                    irregularity: a.irregularity,
                }
            })
            .collect();
        Ok(SizeMeasures {
            range,
            sizes,
            base: r.base,
            rows,
        })
    }

    /// Lignes du tableau d'après la structure de ce projet (la taille de base).
    fn measure_kinds(&self) -> Vec<MeasureKind> {
        let mut kinds = Vec::new();
        for p in &self.pieces {
            kinds.push(MeasureKind::PiecePerimeter { piece: p.id });
            kinds.push(MeasureKind::PieceArea { piece: p.id });
            if p.seam.as_ref().is_some_and(|s| !s.is_default()) {
                kinds.push(MeasureKind::PieceCutPerimeter { piece: p.id });
            }
        }
        for s in &self.seams {
            kinds.push(MeasureKind::SeamLength {
                pair: s.id,
                side: SeamWhich::A,
            });
            kinds.push(MeasureKind::SeamLength {
                pair: s.id,
                side: SeamWhich::B,
            });
            kinds.push(MeasureKind::SeamGap { pair: s.id });
        }
        for sheet in &self.sheets {
            for e in &sheet.entities {
                kinds.push(MeasureKind::PathLength {
                    sheet: sheet.id,
                    entity: e.id,
                });
            }
        }
        kinds
    }

    /// Valeur d'une mesure **dans ce projet** (déjà évalué dans une taille) ; `None` si elle ne
    /// se lit pas (objet absent, contour ouvert ou qui se recoupe, tracé dégénéré).
    fn measure_value(&self, kind: MeasureKind, seams: &[SeamPairReading]) -> Option<f64> {
        let finite = |v: f64| v.is_finite().then_some(v);
        match kind {
            MeasureKind::PiecePerimeter { piece } => {
                let c = self.piece_contour(piece).ok()?;
                c.closed.then_some(c.perimeter_mm).and_then(finite)
            }
            MeasureKind::PieceArea { piece } => {
                let c = self.piece_contour(piece).ok()?;
                (c.closed && !c.self_intersecting)
                    .then_some(c.area_mm2)
                    .and_then(finite)
            }
            MeasureKind::PieceCutPerimeter { piece } => self
                .piece_cut_contour(piece)
                .ok()
                .and_then(|c| finite(c.perimeter_mm)),
            MeasureKind::SeamLength { pair, side } => {
                let r = seams.iter().find(|r| r.id == pair)?;
                match side {
                    SeamWhich::A => r.length_a_mm,
                    SeamWhich::B => r.length_b_mm,
                }
            }
            MeasureKind::SeamGap { pair } => seams.iter().find(|r| r.id == pair)?.embu_measured_mm,
            MeasureKind::PathLength { sheet, entity } => self
                .sheet(sheet)
                .ok()?
                .path_length_mm(entity)
                .ok()
                .and_then(finite),
        }
    }
}
