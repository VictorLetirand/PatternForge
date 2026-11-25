//! US-09.4a — nid de tailles (décision 329).
//!
//! Lecture pure, **rien n'est stocké** (schéma 4 inchangé) : pour chaque taille d'une gamme, le
//! projet est évalué par [`Project::graded_project`] (le même que pour toutes les lectures par
//! taille) et les contours à tracer de la feuille en sont lus : les **tracés** (polylignes
//! d'échantillonnage de leur géométrie, calques visibles seulement) et les **contours de coupe
//! calculés** des pièces à marge de couture (ceux que le plan dessine en filigrane).
//!
//! **Point d'empilement** ([`NestAnchor`]) : un point de la feuille (libre ou construit) dont la
//! position *dans chaque taille* est ramenée à sa position dans la taille de base de la gamme ;
//! toutes les coordonnées de la taille sont donc **translatées** de
//! `position(base) − position(taille)`. [`NestAnchor::Origin`] = origine de la feuille, aucune
//! translation. La taille de base est le contour de référence : jamais translatée.
//!
//! Les coordonnées restent celles de la feuille (mm, Y vers le haut) : le nid n'est pas un nouvel
//! objet du projet, c'est une lecture. Un point inconnu, une feuille ou une gamme inconnue ou
//! vide, un point d'empilement illisible dans une taille sont des erreurs typées.

use crate::error::CoreError;
use crate::model::*;
use pf_geometry::PointMm;
use std::fmt;

/// Pas d'échantillonnage d'un tracé courbe (nombre de pas par courbe).
const SAMPLE_STEPS: usize = 64;

/// Point d'empilement du nid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NestAnchor {
    /// Origine de la feuille : aucune translation.
    Origin,
    /// Point (libre ou construit) de la feuille lue.
    Point(PointId),
}

/// Refus d'une lecture du nid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NestError {
    UnknownRange(SizeRangeId),
    EmptyRange(SizeRangeId),
    UnknownSheet(SheetId),
    UnknownPoint(PointId),
    /// Le point d'empilement ne se calcule pas dans cette taille (ou la taille ne s'évalue pas).
    AnchorUnreadable {
        size: SizeId,
        reason: String,
    },
}

impl fmt::Display for NestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NestError::UnknownRange(id) => write!(f, "gamme inconnue (id {})", id.0),
            NestError::EmptyRange(id) => write!(f, "gamme sans taille (id {})", id.0),
            NestError::UnknownSheet(id) => write!(f, "feuille inconnue (id {})", id.0),
            NestError::UnknownPoint(id) => write!(f, "point d'empilement inconnu (id {})", id.0),
            NestError::AnchorUnreadable { size, reason } => write!(
                f,
                "point d'empilement illisible dans la taille (id {}) : {reason}",
                size.0
            ),
        }
    }
}

impl std::error::Error for NestError {}

impl From<NestError> for CoreError {
    fn from(e: NestError) -> CoreError {
        CoreError::Invalid(e.to_string())
    }
}

/// D'où vient un contour du nid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NestOutlineKind {
    /// Tracé de la feuille.
    Path { entity: EntityId, role: PathRole },
    /// Contour de coupe calculé d'une pièce à marge de couture.
    Cut { piece: PieceId },
}

/// Un contour à tracer, déjà translaté.
#[derive(Debug, Clone, PartialEq)]
pub struct NestOutline {
    pub kind: NestOutlineKind,
    /// Polyligne en mm, en coordonnées de la feuille.
    pub points: Vec<PointMm>,
    /// Le dernier point rejoint le premier (cercle, contour de coupe sans pliure).
    pub closed: bool,
}

/// Une taille du nid.
#[derive(Debug, Clone, PartialEq)]
pub struct NestSize {
    pub size: SizeId,
    /// Taille de base de la gamme : contour de référence, jamais translaté.
    pub is_base: bool,
    /// Position du point d'empilement dans cette taille, **avant** translation.
    pub anchor_position: PointMm,
    /// Translation appliquée à tous les points (mm) : `position(base) − position(taille)`.
    pub shift: (f64, f64),
    pub outlines: Vec<NestOutline>,
    /// Ce qui ne se calcule pas dans cette taille (message du cœur) ; jamais deviné.
    pub issues: Vec<String>,
}

impl NestSize {
    /// Point le plus haut (Y maximal, puis X minimal) de tous les contours : là où se pose
    /// l'étiquette de la taille.
    pub fn top_point(&self) -> Option<PointMm> {
        self.outlines
            .iter()
            .flat_map(|o| o.points.iter().copied())
            .reduce(|best, p| {
                if p.y > best.y || (p.y == best.y && p.x < best.x) {
                    p
                } else {
                    best
                }
            })
    }
}

/// Le nid d'une gamme sur une feuille.
#[derive(Debug, Clone, PartialEq)]
pub struct SizeNest {
    pub range: SizeRangeId,
    pub sheet: SheetId,
    pub base: SizeId,
    pub anchor: NestAnchor,
    /// Position du point d'empilement en taille de base (le point commun de toutes les tailles).
    pub anchor_position: PointMm,
    /// Tailles dans l'ordre de la gamme.
    pub sizes: Vec<NestSize>,
}

fn text(e: CoreError) -> String {
    e.to_string()
}

impl Project {
    /// Nid des tailles de la gamme `range` sur la feuille `sheet` : contours de chaque taille,
    /// translatés pour que le point d'empilement coïncide. Lecture seule, recalculée à chaque
    /// appel, jamais stockée.
    pub fn size_nest(
        &self,
        sheet: SheetId,
        range: SizeRangeId,
        anchor: NestAnchor,
    ) -> Result<SizeNest, NestError> {
        let r = self
            .sizes
            .iter()
            .find(|r| r.id == range)
            .ok_or(NestError::UnknownRange(range))?;
        if r.sizes.is_empty() {
            return Err(NestError::EmptyRange(range));
        }
        let written = self
            .sheet(sheet)
            .map_err(|_| NestError::UnknownSheet(sheet))?;
        if let NestAnchor::Point(id) = anchor {
            if written.point(id).is_err() {
                return Err(NestError::UnknownPoint(id));
            }
        }
        let unreadable =
            |size: SizeId, reason: String| NestError::AnchorUnreadable { size, reason };
        // Position du point d'empilement dans chaque taille, sur le projet évalué de la taille.
        let mut evaluated = Vec::with_capacity(r.sizes.len());
        for s in &r.sizes {
            let g = self
                .graded_project(Some(s.id))
                .map_err(|e| unreadable(s.id, text(e)))?;
            let at = match anchor {
                NestAnchor::Origin => PointMm::ORIGIN,
                NestAnchor::Point(id) => g
                    .sheet(sheet)
                    .and_then(|sh| sh.position(id))
                    .map_err(|e| unreadable(s.id, text(e)))?,
            };
            evaluated.push((s.id, g, at));
        }
        let base_at = evaluated
            .iter()
            .find(|(id, _, _)| *id == r.base)
            .map(|(_, _, at)| *at)
            .ok_or(NestError::EmptyRange(range))?;
        let sizes = evaluated
            .iter()
            .map(|(id, g, at)| {
                let is_base = *id == r.base;
                let shift = if is_base || anchor == NestAnchor::Origin {
                    (0.0, 0.0)
                } else {
                    (base_at.x - at.x, base_at.y - at.y)
                };
                let (outlines, issues) = nest_outlines(g, sheet, shift);
                NestSize {
                    size: *id,
                    is_base,
                    anchor_position: *at,
                    shift,
                    outlines,
                    issues,
                }
            })
            .collect();
        Ok(SizeNest {
            range,
            sheet,
            base: r.base,
            anchor,
            anchor_position: base_at,
            sizes,
        })
    }
}

fn shifted(points: Vec<PointMm>, shift: (f64, f64)) -> Vec<PointMm> {
    points
        .into_iter()
        .map(|p| PointMm::new(p.x + shift.0, p.y + shift.1))
        .collect()
}

/// Contours à tracer d'une feuille d'un projet évalué, translatés de `shift`.
fn nest_outlines(
    g: &Project,
    sheet: SheetId,
    shift: (f64, f64),
) -> (Vec<NestOutline>, Vec<String>) {
    let mut outlines = Vec::new();
    let mut issues = Vec::new();
    let Ok(s) = g.sheet(sheet) else {
        return (outlines, issues);
    };
    for e in s.entities.iter().filter(|e| s.layer_visible(e.layer)) {
        match s.shape(e.id) {
            Ok(shape) => outlines.push(NestOutline {
                kind: NestOutlineKind::Path {
                    entity: e.id,
                    role: e.role,
                },
                closed: matches!(shape, Shape::Circle(_)),
                points: shifted(shape.sample(SAMPLE_STEPS), shift),
            }),
            Err(err) => issues.push(format!("{} : {err}", s.entity_name(e.id))),
        }
    }
    for piece in g.pieces_on(sheet).into_iter().filter(|p| p.seam.is_some()) {
        let cut = match g.piece_cut_contour(piece.id) {
            Ok(c) => c,
            Err(err) => {
                issues.push(format!("{} : {err}", piece.name));
                continue;
            }
        };
        // Les portions qui suivent un pli ne sont pas des bords de coupe : suites ouvertes.
        let mut runs: Vec<Vec<PointMm>> = vec![Vec::new()];
        for (i, t) in cut.traces.iter().enumerate() {
            if cut.fold_cuts.iter().any(|f| f.trace == i) {
                if runs.last().is_some_and(|r| !r.is_empty()) {
                    runs.push(Vec::new());
                }
                continue;
            }
            let run = runs.last_mut().expect("une suite");
            for geom in pf_geometry::region::trace_geometry(t) {
                for p in pf_geometry::split::edge_sample(&geom, SAMPLE_STEPS) {
                    if run
                        .last()
                        .is_none_or(|l| pf_geometry::distance_mm(*l, p) > 1e-9)
                    {
                        run.push(p);
                    }
                }
            }
        }
        let closed = cut.fold_cuts.is_empty();
        for run in runs.into_iter().filter(|r| r.len() >= 2) {
            outlines.push(NestOutline {
                kind: NestOutlineKind::Cut { piece: piece.id },
                points: shifted(run, shift),
                closed,
            });
        }
    }
    (outlines, issues)
}
