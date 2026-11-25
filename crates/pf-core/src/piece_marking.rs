//! Perçages, pointages et croix de pièce (US-06.3, décisions 158 à 168).
//!
//! Un **marquage** est un signe intérieur à la pièce, **exporté** (PDF, SVG) comme
//! marquage distinct : un poinçon (point plein), un perçage (cercle de diamètre
//! donné) ou une croix. Il se compose d'une **forme** ([`MarkingShape`]) et d'un
//! **ancrage** ([`MarkingAnchor`]) : un point de la construction, ou le « bout de
//! pince reculé » (pointe de pince, point de direction, recul en mm). Il n'est ni
//! un élément interne (US-04.3, référence typée sans forme ni taille, jamais
//! exporté) ni une pince : aucune opération de pince (E07), aucune marge (E05).
//!
//! La position est **calculée à la lecture** d'après la construction : un
//! marquage d'une pièce liée suit donc ses points. Tout est en mm.

use crate::command::{Outcome, MERGE_TOLERANCE_MM};
use crate::error::CoreError;
use crate::model::*;
use pf_geometry::{PointMm, VectorMm};
use serde::{Deserialize, Serialize};

/// Plus petit diamètre de perçage accepté (mm) : en dessous, le cercle n'est plus lisible.
pub const MARKING_MIN_DIAMETER_MM: f64 = 0.1;
/// Plus grand diamètre de perçage accepté (mm).
pub const MARKING_MAX_DIAMETER_MM: f64 = 100.0;
/// Plus grande longueur hors tout d'une fente de poinçon (mm) ; sa largeur suit les bornes du
/// diamètre d'un perçage.
pub const MARKING_MAX_SLOT_LENGTH_MM: f64 = 100.0;
/// Plus petit côté de croix accepté (mm), d'un bout à l'autre d'une branche.
pub const MARKING_MIN_CROSS_MM: f64 = 0.5;
/// Plus grand côté de croix accepté (mm).
pub const MARKING_MAX_CROSS_MM: f64 = 100.0;
/// Plus petit recul d'un bout de pince accepté (mm).
pub const MARKING_MIN_SETBACK_MM: f64 = 0.01;
/// Plus grand recul d'un bout de pince accepté (mm).
pub const MARKING_MAX_SETBACK_MM: f64 = 1000.0;
/// Nombre maximal de marquages par pièce.
pub const MARKING_MAX_PER_PIECE: usize = 1000;
/// Diamètre (mm) du disque plein qui représente un pointage à l'export et à l'écran.
pub const PUNCH_DIAMETER_MM: f64 = 1.0;

/// Forme d'un marquage. Les dimensions sont en mm, finies et bornées (voir les
/// constantes `MARKING_*`).
///
/// Lecture via [`MarkingShapeWire`] (variante à champs vides) : une variante unitaire d'un enum à
/// étiquette interne ignorerait une clé en trop (D-04). L'écriture est inchangée.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", from = "MarkingShapeWire")]
pub enum MarkingShape {
    /// Pointage : un poinçon, dessiné comme un petit disque plein de
    /// [`PUNCH_DIAMETER_MM`] (sa taille n'est pas un paramètre).
    Punch,
    /// Perçage : cercle de diamètre donné (mm), centré sur la position.
    Drill { diameter_mm: f64 },
    /// Croix « + » à branches horizontale et verticale ; `size_mm` est la longueur
    /// d'une branche d'un bout à l'autre.
    Cross { size_mm: f64 },
    /// Fente de poinçon (PH-4b) : rectangle à bouts arrondis de `length_mm` (hors tout) ×
    /// `width_mm`, centré sur la position, dont l'axe de la longueur fait l'angle `angle_deg`
    /// avec +X (anti-horaire, dans [0 ; 360[). L'angle est **stocké** : il ne suit pas une
    /// rotation de la pièce.
    Slot {
        length_mm: f64,
        width_mm: f64,
        angle_deg: f64,
    },
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum MarkingShapeWire {
    Punch {},
    Drill {
        diameter_mm: f64,
    },
    Cross {
        size_mm: f64,
    },
    Slot {
        length_mm: f64,
        width_mm: f64,
        angle_deg: f64,
    },
}

impl From<MarkingShapeWire> for MarkingShape {
    fn from(w: MarkingShapeWire) -> Self {
        match w {
            MarkingShapeWire::Punch {} => MarkingShape::Punch,
            MarkingShapeWire::Drill { diameter_mm } => MarkingShape::Drill { diameter_mm },
            MarkingShapeWire::Cross { size_mm } => MarkingShape::Cross { size_mm },
            MarkingShapeWire::Slot {
                length_mm,
                width_mm,
                angle_deg,
            } => MarkingShape::Slot {
                length_mm,
                width_mm,
                angle_deg,
            },
        }
    }
}

impl MarkingShape {
    /// Identifiant stable, indépendant de la langue.
    pub fn key(self) -> &'static str {
        match self {
            MarkingShape::Punch => "punch",
            MarkingShape::Drill { .. } => "drill",
            MarkingShape::Cross { .. } => "cross",
            MarkingShape::Slot { .. } => "slot",
        }
    }

    /// Diamètre ou côté saisi (mm) ; `None` pour un pointage et pour une fente (trois valeurs).
    pub fn size_mm(self) -> Option<f64> {
        match self {
            MarkingShape::Punch | MarkingShape::Slot { .. } => None,
            MarkingShape::Drill { diameter_mm } => Some(diameter_mm),
            MarkingShape::Cross { size_mm } => Some(size_mm),
        }
    }

    /// Forme tournée de `degrees` (anti-horaire) avec la pièce : seule l'angle d'une fente
    /// change (ramené dans [0 ; 360[) ; les autres formes sont symétriques ou sans orientation.
    pub fn rotated(self, degrees: f64) -> MarkingShape {
        match self {
            MarkingShape::Slot {
                length_mm,
                width_mm,
                angle_deg,
            } => {
                let mut a = (angle_deg + degrees) % 360.0;
                if a < 0.0 {
                    a += 360.0;
                }
                if !(0.0..360.0).contains(&a) {
                    a = 0.0;
                }
                MarkingShape::Slot {
                    length_mm,
                    width_mm,
                    angle_deg: a,
                }
            }
            other => other,
        }
    }

    /// La fente (rectangle à bouts arrondis) centrée en `center` ; `None` pour une autre forme.
    pub fn slot(self, center: PointMm) -> Option<pf_geometry::slot::Slot> {
        match self {
            MarkingShape::Slot {
                length_mm,
                width_mm,
                angle_deg,
            } => Some(pf_geometry::slot::Slot {
                center,
                length_mm,
                width_mm,
                angle_deg,
            }),
            _ => None,
        }
    }

    /// Dimension finie et dans les bornes, sinon refus expliqué.
    pub fn validate(self) -> Result<(), CoreError> {
        let (value, min, max, what) = match self {
            MarkingShape::Punch => return Ok(()),
            MarkingShape::Slot {
                length_mm,
                width_mm,
                angle_deg,
            } => {
                let width_ok = width_mm.is_finite()
                    && (MARKING_MIN_DIAMETER_MM..=MARKING_MAX_DIAMETER_MM).contains(&width_mm);
                if !width_ok
                    || !length_mm.is_finite()
                    || !(width_mm..=MARKING_MAX_SLOT_LENGTH_MM).contains(&length_mm)
                    || !angle_deg.is_finite()
                    || !(0.0..360.0).contains(&angle_deg)
                {
                    return Err(CoreError::Invalid(format!(
                        "une fente de poinçon exige une largeur de {MARKING_MIN_DIAMETER_MM} à \
                         {MARKING_MAX_DIAMETER_MM} mm, une longueur de la largeur à \
                         {MARKING_MAX_SLOT_LENGTH_MM} mm et un angle dans [0 ; 360["
                    )));
                }
                return Ok(());
            }
            MarkingShape::Drill { diameter_mm } => (
                diameter_mm,
                MARKING_MIN_DIAMETER_MM,
                MARKING_MAX_DIAMETER_MM,
                "le diamètre d'un perçage",
            ),
            MarkingShape::Cross { size_mm } => (
                size_mm,
                MARKING_MIN_CROSS_MM,
                MARKING_MAX_CROSS_MM,
                "le côté d'une croix",
            ),
        };
        if value.is_finite() && (min..=max).contains(&value) {
            Ok(())
        } else {
            Err(CoreError::Invalid(format!(
                "{what} doit être un nombre fini compris entre {min} et {max} mm"
            )))
        }
    }
}

/// Où se pose un marquage : un point de la construction, ou un bout de pince reculé.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum MarkingAnchor {
    /// Sur un point de la feuille de la pièce (il peut être déplacé : le marquage suit).
    Point { point: PointId },
    /// Bout de pince reculé : le point situé à `setback_mm` de la pointe `tip`, sur
    /// la droite qui va de `tip` vers `toward` (la direction de la pince, définie
    /// par ces deux points). C'est le seul calcul de pince : aucune pince complète.
    DartTip {
        tip: PointId,
        toward: PointId,
        setback_mm: f64,
    },
}

impl MarkingAnchor {
    /// Points de la construction dont dépend l'ancrage.
    pub fn points(&self) -> Vec<PointId> {
        match *self {
            MarkingAnchor::Point { point } => vec![point],
            MarkingAnchor::DartTip { tip, toward, .. } => vec![tip, toward],
        }
    }

    /// Identifiant stable, indépendant de la langue.
    pub fn key(&self) -> &'static str {
        match self {
            MarkingAnchor::Point { .. } => "point",
            MarkingAnchor::DartTip { .. } => "dart_tip",
        }
    }
}

/// Marquage d'une pièce : identifiant stable, ancrage et forme.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PieceMarking {
    pub id: MarkingId,
    pub at: MarkingAnchor,
    pub shape: MarkingShape,
}

/// Un marquage et sa position actuelle (calculée d'après la construction).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResolvedMarking {
    pub marking: PieceMarking,
    /// Position du centre, en mm.
    pub at: PointMm,
    /// `Some(false)` : le centre n'est pas dans le contour ; `None` : pas de
    /// jugement possible (contour absent, ouvert ou illisible).
    pub inside_outline: Option<bool>,
}

/// Position d'un ancrage sur une feuille (`None` : point absent, illisible, ou
/// pointe et direction confondues).
fn anchor_position(sheet: &Sheet, at: &MarkingAnchor) -> Option<PointMm> {
    match *at {
        MarkingAnchor::Point { point } => sheet.position(point).ok(),
        MarkingAnchor::DartTip {
            tip,
            toward,
            setback_mm,
        } => {
            let p = sheet.position(tip).ok()?;
            let q = sheet.position(toward).ok()?;
            let v: VectorMm = q - p;
            let d = v.norm();
            if !d.is_finite() || d <= MERGE_TOLERANCE_MM {
                return None;
            }
            let at = p + v * (setback_mm / d);
            (at.x.is_finite() && at.y.is_finite()).then_some(at)
        }
    }
}

/// La feuille qui porte tous les points demandés : celle de la pièce si elle est
/// liée, la première feuille qui les porte tous si elle est détachée.
pub(crate) fn sheet_with_points<'a>(
    project: &'a Project,
    link: PieceLink,
    points: &[PointId],
) -> Option<&'a Sheet> {
    let has = |s: &Sheet| points.iter().all(|&p| s.point(p).is_ok());
    match link {
        PieceLink::Linked { sheet } => project.sheet(sheet).ok().filter(|s| has(s)),
        PieceLink::Detached => project.sheets.iter().find(|s| has(s)),
    }
}

impl Project {
    /// La feuille de la pièce porte-t-elle encore les points de cet ancrage ?
    pub(crate) fn marking_anchor_alive(&self, link: PieceLink, at: &MarkingAnchor) -> bool {
        sheet_with_points(self, link, &at.points()).is_some()
    }

    /// Position actuelle d'un ancrage pour une pièce de ce lien (`None` : voir
    /// `anchor_position`). Un projet valide n'en contient jamais.
    pub(crate) fn marking_position(&self, link: PieceLink, at: &MarkingAnchor) -> Option<PointMm> {
        sheet_with_points(self, link, &at.points()).and_then(|s| anchor_position(s, at))
    }

    /// Marquages d'une pièce avec leur position actuelle (lecture pure ; liste vide
    /// pour une pièce inconnue). Un marquage dont la position ne peut pas être
    /// calculée est impossible dans un projet valide ; il serait omis.
    pub fn piece_markings(&self, piece: PieceId) -> Vec<ResolvedMarking> {
        let Ok(p) = self.piece(piece) else {
            return Vec::new();
        };
        if p.markings.is_empty() {
            return Vec::new();
        }
        let lasso = self.outline_lasso(piece).ok();
        p.markings
            .iter()
            .filter_map(|&marking| {
                let at = self.marking_position(p.link, &marking.at)?;
                Some(ResolvedMarking {
                    marking,
                    at,
                    inside_outline: lasso.as_ref().map(|l| l.contains_point(at)),
                })
            })
            .collect()
    }

    /// Marquages de toutes les pièces dont le contour est sur cette feuille : position
    /// en mm et forme, dans l'ordre des pièces puis des marquages. Sert à l'export.
    pub fn sheet_markings(&self, sheet: SheetId) -> Vec<(PointMm, MarkingShape)> {
        self.pieces_on(sheet)
            .into_iter()
            .filter(|p| !p.markings.is_empty())
            .flat_map(|p| {
                p.markings
                    .iter()
                    .filter_map(|m| self.marking_position(p.link, &m.at).map(|at| (at, m.shape)))
            })
            .collect()
    }

    /// Contrôle d'un ancrage et d'une forme pour une pièce : points sur la feuille
    /// de la pièce, recul borné, pointe et direction distinctes.
    fn check_marking(
        &self,
        piece_name: &str,
        link: PieceLink,
        at: &MarkingAnchor,
        shape: MarkingShape,
    ) -> Result<(), CoreError> {
        shape.validate()?;
        if let MarkingAnchor::DartTip {
            tip,
            toward,
            setback_mm,
        } = *at
        {
            if !setback_mm.is_finite()
                || !(MARKING_MIN_SETBACK_MM..=MARKING_MAX_SETBACK_MM).contains(&setback_mm)
            {
                return Err(CoreError::Invalid(format!(
                    "le recul d'un bout de pince doit être un nombre fini compris entre \
                     {MARKING_MIN_SETBACK_MM} et {MARKING_MAX_SETBACK_MM} mm"
                )));
            }
            if tip == toward {
                return Err(CoreError::Invalid(
                    "la pointe de la pince et le point de direction doivent être deux points \
                     différents"
                        .into(),
                ));
            }
        }
        let points = at.points();
        let Some(sheet) = sheet_with_points(self, link, &points) else {
            let missing = points
                .iter()
                .find(|&&p| !self.sheets.iter().any(|s| s.point(p).is_ok()));
            return Err(match missing {
                Some(&p) => CoreError::UnknownPoint(p),
                None => CoreError::DeadReference(format!(
                    "le marquage de la pièce « {piece_name} » référence un point absent de la \
                     feuille de la pièce"
                )),
            });
        };
        if anchor_position(sheet, at).is_none() {
            return Err(CoreError::Invalid(match at {
                MarkingAnchor::Point { .. } => {
                    format!("la position du point de la pièce « {piece_name} » est illisible")
                }
                MarkingAnchor::DartTip { .. } => format!(
                    "la pointe de la pince et le point de direction de la pièce « {piece_name} » \
                     sont confondus ou illisibles : la direction de la pince n'est pas définie"
                ),
            }));
        }
        Ok(())
    }

    /// Validation d'une pièce chargée ou modifiée : nombre, dimensions, ancrages.
    pub(crate) fn validate_markings(&self, piece: &Piece) -> Result<(), CoreError> {
        if piece.markings.len() > MARKING_MAX_PER_PIECE {
            return Err(CoreError::Invalid(format!(
                "la pièce « {} » porte plus de {MARKING_MAX_PER_PIECE} marquages",
                piece.name
            )));
        }
        for m in &piece.markings {
            self.check_marking(&piece.name, piece.link, &m.at, m.shape)?;
        }
        Ok(())
    }

    fn marking_index(&self, piece: PieceId, marking: MarkingId) -> Result<usize, CoreError> {
        let p = self.piece(piece)?;
        p.markings
            .iter()
            .position(|m| m.id == marking)
            .ok_or_else(|| {
                CoreError::Invalid(format!(
                    "le marquage {} n'existe pas sur la pièce « {} »",
                    marking.0, p.name
                ))
            })
    }

    /// Ajoute un marquage (refus expliqué : pièce sans feuille, point absent ou d'une
    /// autre feuille, dimension hors bornes, pointe et direction confondues, trop de
    /// marquages).
    pub(crate) fn add_piece_marking(
        &mut self,
        piece: PieceId,
        at: MarkingAnchor,
        shape: MarkingShape,
        out: &mut Outcome,
    ) -> Result<(), CoreError> {
        let p = self.piece(piece)?;
        let (name, link) = (p.name.clone(), p.link);
        if self.piece_sheet(piece).is_err() {
            return Err(CoreError::Invalid(format!(
                "la pièce « {name} » n'a pas de contour : on ne sait pas sur quelle feuille \
                 chercher les points"
            )));
        }
        if p.markings.len() >= MARKING_MAX_PER_PIECE {
            return Err(CoreError::Invalid(format!(
                "la pièce « {name} » porte déjà {MARKING_MAX_PER_PIECE} marquages"
            )));
        }
        self.check_marking(&name, link, &at, shape)?;
        let id = MarkingId(self.alloc_id());
        self.piece_mut(piece)?
            .markings
            .push(PieceMarking { id, at, shape });
        out.markings.push(id);
        Ok(())
    }

    pub(crate) fn remove_piece_marking(
        &mut self,
        piece: PieceId,
        marking: MarkingId,
    ) -> Result<(), CoreError> {
        let i = self.marking_index(piece, marking)?;
        self.piece_mut(piece)?.markings.remove(i);
        Ok(())
    }

    /// Remplace l'ancrage et la forme d'un marquage (même identifiant, même rang).
    pub(crate) fn set_piece_marking(
        &mut self,
        piece: PieceId,
        marking: MarkingId,
        at: MarkingAnchor,
        shape: MarkingShape,
    ) -> Result<(), CoreError> {
        let i = self.marking_index(piece, marking)?;
        let p = self.piece(piece)?;
        let (name, link) = (p.name.clone(), p.link);
        self.check_marking(&name, link, &at, shape)?;
        let m = &mut self.piece_mut(piece)?.markings[i];
        m.at = at;
        m.shape = shape;
        Ok(())
    }

    /// Retire les marquages dont un point a disparu pendant la commande, avec une
    /// remarque. Ne touche que les marquages qui existaient avant la commande : une
    /// référence morte introduite par la commande elle-même reste refusée par la
    /// validation.
    pub(crate) fn prune_dead_markings(&mut self, before: &Project, out: &mut Outcome) {
        if self.pieces.iter().all(|p| p.markings.is_empty()) {
            return;
        }
        let dead: Vec<(PieceId, Vec<MarkingId>)> = self
            .pieces
            .iter()
            .filter(|p| !p.markings.is_empty())
            .map(|p| {
                let old = before.pieces.iter().find(|o| o.id == p.id);
                let gone = p
                    .markings
                    .iter()
                    .filter(|m| {
                        old.is_some_and(|o| o.markings.contains(m))
                            && !self.marking_anchor_alive(p.link, &m.at)
                    })
                    .map(|m| m.id)
                    .collect();
                (p.id, gone)
            })
            .filter(|(_, gone): &(PieceId, Vec<MarkingId>)| !gone.is_empty())
            .collect();
        for (id, gone) in dead {
            if let Ok(p) = self.piece_mut(id) {
                p.markings.retain(|m| !gone.contains(&m.id));
                for _ in &gone {
                    out.notes.push(format!(
                        "Un marquage de la pièce « {} » référençait un point supprimé : il est \
                         retiré.",
                        p.name
                    ));
                }
            }
        }
    }

    /// Retire les marquages d'une pièce dérivée dont un point a été recréé ou dont
    /// la direction est devenue indéfinie après la régénération (jamais un refus).
    pub(crate) fn prune_own_markings(&mut self, piece: PieceId, out: &mut Outcome) {
        let Ok(p) = self.piece(piece) else { return };
        let gone: Vec<MarkingId> = p
            .markings
            .iter()
            .filter(|m| self.marking_position(p.link, &m.at).is_none())
            .map(|m| m.id)
            .collect();
        if gone.is_empty() {
            return;
        }
        if let Ok(p) = self.piece_mut(piece) {
            p.markings.retain(|m| !gone.contains(&m.id));
            for _ in &gone {
                out.notes.push(format!(
                    "Un marquage de la pièce « {} » dépendait d'un point recréé ou confondu par \
                     la mise à jour : il est retiré.",
                    p.name
                ));
            }
        }
    }
}
