//! US-09.5a — géométrie dérivée par taille (décision 327).
//!
//! Aucune règle n'est dupliquée : une taille est un **projet évalué** (`Project::graded_project` :
//! points libres déplacés de leur écart, points construits, poignées, pièces dérivées et
//! surpiqûres liées recalculés depuis eux) sur lequel on appelle les lectures ordinaires du cœur
//! (`piece_contour`, `piece_cut_contour`, `notch_placed_marks`, `piece_darts`…). Rien n'est stocké.
//!
//! Conventions (à faire valider par une modéliste) :
//! - la marge de couture est une valeur en mm **non graduée** : le contour de coupe est le
//!   décalage exact du contour de couture de la taille, donc la même marge dans toutes les tailles ;
//! - un cran à **distance fixe** reste à `distance_mm` depuis le départ de son tracé, quelle que
//!   soit la longueur du tracé dans la taille ; un cran **proportionnel** (`Notch::proportional`,
//!   US-09.5c) garde sa fraction de cette longueur ; un cran ancré en proportion (`Ratio`,
//!   `Midpoint`) suit la longueur du tracé ; un cran ancré à un point suit ce point ;
//! - un cran à distance fixe plus loin que la fin du tracé dans une taille est posé à l'extrémité
//!   ([`DerivedLimit::NotchBeyondEdge`]) ; la taille de base ne peut pas être changée vers une
//!   taille où cela arrive (la géométrie écrite serait invalide) ;
//! - les poignées de Bézier et les tangentes de courbes lisses sont des décalages écrits : elles
//!   suivent leur extrémité par translation, sans être agrandies ;
//! - les plis (US-07.3) et lignes d'ampleur (US-07.4) stockent leurs lignes en **coordonnées**
//!   (pas de référence à un point) : elles ne suivent pas la gradation, ce que
//!   [`DerivedLimit::AbsoluteLines`] signale ; liens maintenus et longueurs imposées ne sont pas
//!   réévalués par taille ([`DerivedLimit::Maintained`]) ; les formules liées suivent les valeurs
//!   de variables par taille (US-09.7, module `size_values`).

use crate::contour::{closed_contours, measure_contour, ContourMeasure};
use crate::error::CoreError;
use crate::grading::GradingFollower;
use crate::model::*;
use crate::notch::NotchPlace;
use crate::piece::{PieceContour, PieceIssue};
use crate::piece_dart::ResolvedDart;
use crate::piece_grain::ResolvedGrainLine;
use crate::piece_pleat::{FoldedPiece, ResolvedPleat};
use crate::piece_seam::{CutContour, FoldNotch};
use pf_geometry::PointMm;
use std::fmt;

/// Refus d'une lecture par taille.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DerivedError {
    UnknownSize(SizeId),
    UnknownSheet(SheetId),
    /// Les valeurs de variables de la taille ne s'évaluent pas (US-09.7).
    Evaluation(String),
}

impl fmt::Display for DerivedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DerivedError::Evaluation(msg) => write!(f, "{msg}"),
            DerivedError::UnknownSize(id) => write!(f, "taille inconnue (id {})", id.0),
            DerivedError::UnknownSheet(id) => write!(f, "feuille inconnue (id {})", id.0),
        }
    }
}

impl std::error::Error for DerivedError {}

impl From<DerivedError> for CoreError {
    fn from(e: DerivedError) -> CoreError {
        CoreError::Invalid(e.to_string())
    }
}

/// Ce qui ne suit pas la taille (signalé, jamais caché).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DerivedLimit {
    /// Plis ou lignes d'ampleur de cette pièce : lignes écrites en coordonnées, non graduées.
    AbsoluteLines { piece: PieceId },
    /// Cran à distance fixe plus loin que la fin de son tracé dans cette taille : il est posé à
    /// l'extrémité du tracé (la distance écrite n'est pas modifiée).
    NotchBeyondEdge { notch: NotchId },
    /// Point suivi par une construction maintenue ou une longueur imposée : non réévalué.
    Maintained { point: PointId, by: GradingFollower },
}

/// Un cran de la feuille dans la taille.
#[derive(Debug, Clone, PartialEq)]
pub struct NotchDerived {
    pub notch: NotchId,
    pub entity: EntityId,
    /// Position effective depuis le départ du tracé (mm).
    pub distance_mm: Result<f64, String>,
    /// Lieu et traits du cran tel qu'il est dessiné et exporté.
    pub marks: Result<(NotchPlace, Vec<(PointMm, PointMm)>), String>,
}

/// Contour fermé de la feuille (hors pièces), mesuré dans la taille.
#[derive(Debug, Clone, PartialEq)]
pub struct ContourDerived {
    pub entities: Vec<EntityId>,
    pub measure: Result<ContourMeasure, String>,
}

/// Données dérivées d'une pièce dans la taille.
#[derive(Debug, Clone, PartialEq)]
pub struct PieceDerived {
    pub piece: PieceId,
    /// Aire, périmètre, fermeture (« pièce fermée ») du contour de couture.
    pub contour: Result<PieceContour, String>,
    /// Contour de coupe (marges de couture comprises).
    pub cut: Result<CutContour, String>,
    pub fold_notches: Result<Vec<FoldNotch>, String>,
    pub darts: Vec<ResolvedDart>,
    pub grain_lines: Vec<ResolvedGrainLine>,
    pub pleats: Vec<ResolvedPleat>,
    /// Pièce repliée, seulement si la pièce porte des plis.
    pub folded: Option<Result<FoldedPiece, String>>,
    /// Avertissements (contour ouvert, pince non calculable, coupe impossible…).
    pub issues: Vec<PieceIssue>,
}

/// Tout ce qui se lit d'une feuille dans une taille.
#[derive(Debug, Clone, PartialEq)]
pub struct SizeDerived {
    pub sheet: SheetId,
    /// Taille demandée (`None` = taille de base).
    pub size: Option<SizeId>,
    /// Positions de tous les points (comme `evaluate_sheet`).
    pub points: Vec<(PointId, PointMm)>,
    /// Points et tracés qui ne se calculent pas dans cette taille.
    pub issues: Vec<String>,
    /// Longueur développée de chaque tracé.
    pub path_lengths: Vec<(EntityId, Result<f64, String>)>,
    pub contours: Vec<ContourDerived>,
    pub notches: Vec<NotchDerived>,
    pub pieces: Vec<PieceDerived>,
    pub limits: Vec<DerivedLimit>,
}

fn text<T>(r: Result<T, CoreError>) -> Result<T, String> {
    r.map_err(|e| e.to_string())
}

impl Project {
    /// Données dérivées principales d'une feuille dans la taille `size` (`None` = taille de base).
    /// Lecture seule, recalculée à chaque appel, jamais stockée : les lectures ordinaires du cœur
    /// sont appelées sur le projet évalué de la taille. Taille ou feuille inconnue = erreur typée.
    pub fn derived_for_size(
        &self,
        sheet: SheetId,
        size: Option<SizeId>,
    ) -> Result<SizeDerived, DerivedError> {
        if let Some(id) = size {
            if self.find_size(id).is_none() {
                return Err(DerivedError::UnknownSize(id));
            }
        }
        if self.sheet(sheet).is_err() {
            return Err(DerivedError::UnknownSheet(sheet));
        }
        let p = self
            .graded_project(size)
            .map_err(|e| DerivedError::Evaluation(e.to_string()))?;
        let s = p
            .sheet(sheet)
            .map_err(|_| DerivedError::UnknownSheet(sheet))?;
        let points = s
            .points
            .iter()
            .filter_map(|q| s.position(q.id).ok().map(|pos| (q.id, pos)))
            .collect();
        let path_lengths = s
            .entities
            .iter()
            .map(|e| (e.id, text(s.path_length_mm(e.id))))
            .collect();
        let contours = closed_contours(s)
            .into_iter()
            .map(|c| ContourDerived {
                entities: c.edges.iter().map(|e| e.entity).collect(),
                measure: text(measure_contour(s, &c)),
            })
            .collect();
        let notches = s
            .notches
            .iter()
            .map(|n| NotchDerived {
                notch: n.id,
                entity: n.entity,
                distance_mm: text(s.notch_distance_mm(n)),
                marks: text(p.notch_placed_marks(sheet, n)),
            })
            .collect();
        let pieces: Vec<PieceDerived> = p
            .pieces_on(sheet)
            .into_iter()
            .map(|piece| {
                let id = piece.id;
                PieceDerived {
                    piece: id,
                    contour: text(p.piece_contour(id)),
                    cut: text(p.piece_cut_contour(id)),
                    fold_notches: text(p.piece_fold_notches(id)),
                    darts: p.piece_darts(id),
                    grain_lines: p.piece_grain_lines(id),
                    pleats: p.piece_pleat_marks(id),
                    folded: (!piece.pleats.is_empty()).then(|| text(p.piece_folded(id))),
                    issues: p.piece_issues(id),
                }
            })
            .collect();
        let mut limits = Vec::new();
        let graded_here = size.is_some_and(|id| s.gradings.iter().any(|g| g.size == id));
        for n in s.notches.iter().filter(|n| n.anchor.is_none()) {
            if s.path_length_mm(n.entity)
                .is_ok_and(|len| n.distance_mm > len + 1e-9)
            {
                limits.push(DerivedLimit::NotchBeyondEdge { notch: n.id });
            }
        }
        if graded_here {
            for piece in p.pieces_on(sheet) {
                if !piece.pleats.is_empty() || !piece.flares.is_empty() {
                    limits.push(DerivedLimit::AbsoluteLines { piece: piece.id });
                }
            }
            for q in &s.points {
                if let Some(by) = s.grading_follower(q.id) {
                    limits.push(DerivedLimit::Maintained { point: q.id, by });
                }
            }
        }
        Ok(SizeDerived {
            sheet,
            size,
            points,
            issues: s.evaluation_issues(),
            path_lengths,
            contours,
            notches,
            pieces,
            limits,
        })
    }
}
