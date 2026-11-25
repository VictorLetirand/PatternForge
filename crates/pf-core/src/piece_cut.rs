//! US-07.5 — découpe d'une pièce selon une ligne interne, avec crans de repère automatiques
//! des deux côtés (décision 301).
//!
//! `Command::CutPiece` coupe une pièce selon **une ligne** (deux points, ou un tracé de la feuille :
//! segment, courbe de Bézier ou arc) qui traverse le contour de bord à bord en exactement deux
//! points ; seule la portion entre les deux traversées sert. Comme « Séparer » (US-04.6), la pièce
//! est remplacée par deux **pièces détachées sur des feuilles dédiées** (la construction d'origine
//! n'est pas touchée). Les deux bords nouvellement créés sont **la même ligne** : ils ont la même
//! géométrie, parcourue du même point de départ `P` (début de la ligne) au même point d'arrivée
//! `Q` ; les crans de repère sont posés aux **mêmes distances depuis `P`** sur les deux bords (le
//! bord de la moitié de droite est parcouru de `Q` vers `P` dans le contour : il est marqué
//! « retourné », ce qui garde la distance d'un cran identique à celle de l'autre bord).
//!
//! Recoller les morceaux = `Command::JoinPieces` par les deux bords de découpe (US-04.6).
//!
//! Convention d'atelier (non validée) : par défaut deux crans, au tiers et aux deux tiers de la
//! ligne ; une ligne qui dépasse du contour est coupée aux deux traversées ; la ligne se compte
//! depuis son départ (`a` pour deux points, début du tracé pour un tracé).

use crate::command::Outcome;
use crate::error::CoreError;
use crate::model::*;
use crate::piece_split_join::{edge_of_shape, geoms_of};
use pf_geometry::region::EdgeGeom;
use pf_geometry::split::{chain_length_mm, chain_signed_area_mm2, cut_chain_by_edge};
use pf_geometry::{PointMm, Side};
use serde::{Deserialize, Serialize};

/// Nombre maximal de crans de repère par bord de découpe.
pub const CUT_MAX_NOTCHES: u32 = 50;
/// Nombre de crans proposé par défaut (au tiers et aux deux tiers).
pub const CUT_DEFAULT_NOTCH_COUNT: u32 = 2;
/// Plus petit pas entre crans (mm).
pub const CUT_STEP_MIN_MM: f64 = 1.0;
/// Plus grand pas entre crans (mm).
pub const CUT_STEP_MAX_MM: f64 = 1000.0;
/// Nombre maximal de tracés proposés par `Project::cut_candidates`.
pub const CUT_MAX_CANDIDATES: usize = 200;
/// Un cran à moins que cela (mm) de l'extrémité de la ligne n'est pas posé.
const END_MARGIN_MM: f64 = 1e-6;

/// Ligne de découpe.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CutLine {
    /// Segment de `a` à `b` (coordonnées en mm).
    Points { a: PointMm, b: PointMm },
    /// Tracé de la feuille de la pièce (segment, courbe de Bézier ou arc), de son départ à son
    /// arrivée.
    Path { entity: EntityId },
}

/// Crans de repère posés sur chacun des deux bords de découpe.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CutNotches {
    /// `count` crans à intervalles égaux (de 0 à [`CUT_MAX_NOTCHES`]) : `count = 2` donne le tiers
    /// et les deux tiers.
    Count { count: u32 },
    /// Un cran tous les `step_mm` depuis le départ de la ligne (jamais à l'extrémité).
    Step { step_mm: f64 },
}

impl Default for CutNotches {
    fn default() -> Self {
        CutNotches::Count {
            count: CUT_DEFAULT_NOTCH_COUNT,
        }
    }
}

fn invalid(msg: impl Into<String>) -> CoreError {
    CoreError::Invalid(msg.into())
}

impl CutNotches {
    /// Refus expliqué : nombre ou pas hors bornes.
    pub fn validate(&self) -> Result<(), CoreError> {
        match *self {
            CutNotches::Count { count } if count > CUT_MAX_NOTCHES => Err(invalid(format!(
                "le nombre de crans de repère doit être compris entre 0 et {CUT_MAX_NOTCHES}"
            ))),
            CutNotches::Step { step_mm }
                if !step_mm.is_finite()
                    || !(CUT_STEP_MIN_MM..=CUT_STEP_MAX_MM).contains(&step_mm) =>
            {
                Err(invalid(format!(
                    "le pas des crans de repère doit être un nombre compris entre \
                     {CUT_STEP_MIN_MM} et {CUT_STEP_MAX_MM} mm"
                )))
            }
            _ => Ok(()),
        }
    }

    /// Distances des crans depuis le départ d'une ligne de `len_mm` de long.
    pub fn positions(&self, len_mm: f64) -> Result<Vec<f64>, CoreError> {
        self.validate()?;
        if !len_mm.is_finite() || len_mm <= 0.0 {
            return Err(invalid("la ligne de découpe n'a pas de longueur"));
        }
        match *self {
            CutNotches::Count { count } => Ok((1..=count)
                .map(|k| len_mm * f64::from(k) / f64::from(count + 1))
                .collect()),
            CutNotches::Step { step_mm } => {
                let n = ((len_mm - END_MARGIN_MM) / step_mm).ceil() - 1.0;
                if n > f64::from(CUT_MAX_NOTCHES) {
                    return Err(invalid(format!(
                        "ce pas poserait plus de {CUT_MAX_NOTCHES} crans sur la ligne de \
                         {len_mm:.1} mm : augmentez le pas"
                    )));
                }
                Ok((1..)
                    .map(|k| f64::from(k) * step_mm)
                    .take_while(|d| *d < len_mm - END_MARGIN_MM)
                    .collect())
            }
        }
    }
}

/// Mesures **observées** d'un morceau.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CutHalfInfo {
    pub area_mm2: f64,
    pub perimeter_mm: f64,
    /// Longueur de son bord de découpe (mm) : la même pour les deux morceaux.
    pub edge_mm: f64,
}

/// Aperçu d'une découpe (lecture seule).
#[derive(Debug, Clone, PartialEq)]
pub struct CutPreview {
    /// Points de traversée du contour : `P` (repère commun des crans) puis `Q`.
    pub points: [PointMm; 2],
    /// Longueur de la ligne de découpe entre `P` et `Q` (mm).
    pub cut_length_mm: f64,
    /// Morceau à gauche de `P → Q` (futur « 1/2 »).
    pub left: CutHalfInfo,
    /// Morceau à droite (futur « 2/2 »).
    pub right: CutHalfInfo,
    /// Distances des crans depuis `P` (mm), identiques sur les deux bords.
    pub notch_distances_mm: Vec<f64>,
    /// Raison pour laquelle la commande serait refusée ; `None` si elle réussirait.
    pub refusal: Option<String>,
}

impl Project {
    /// Géométrie de la ligne de découpe, ou refus expliqué.
    fn cut_line_geom(&self, piece: PieceId, line: &CutLine) -> Result<EdgeGeom, CoreError> {
        match line {
            CutLine::Points { a, b } => {
                a.validate()?;
                b.validate()?;
                Ok(EdgeGeom::Line(*a, *b))
            }
            CutLine::Path { entity } => {
                let sheet = self.piece_sheet(piece)?;
                sheet.entity(*entity)?;
                if self
                    .piece(piece)?
                    .outline
                    .iter()
                    .any(|e| e.entity == *entity)
                {
                    return Err(invalid(
                        "la ligne de découpe est un bord du contour de la pièce",
                    ));
                }
                edge_of_shape(&sheet.shape(*entity)?).ok_or_else(|| {
                    invalid(
                        "la ligne de découpe doit être un segment, une courbe de Bézier ou un \
                         arc (pas un cercle ni une spline)",
                    )
                })
            }
        }
    }

    /// Aperçu de `Command::CutPiece` : mêmes calculs, rien n'est modifié. Erreur si la pièce, la
    /// ligne ou la coupe sont inutilisables ; sinon `refusal` donne la raison d'un éventuel refus
    /// de la commande (rejouée sur une copie).
    pub fn cut_preview(
        &self,
        piece: PieceId,
        line: &CutLine,
        notches: &CutNotches,
    ) -> Result<CutPreview, CoreError> {
        notches.validate()?;
        let geom = self.cut_line_geom(piece, line)?;
        let chain = self.piece_chain(piece)?;
        let cut =
            cut_chain_by_edge(&geoms_of(&chain), &geom).map_err(|e| invalid(e.to_string()))?;
        let info = |h: &[pf_geometry::split::CutEdge]| {
            let g: Vec<EdgeGeom> = h.iter().map(|e| e.geom).collect();
            CutHalfInfo {
                area_mm2: chain_signed_area_mm2(&g).abs(),
                perimeter_mm: chain_length_mm(&g),
                edge_mm: h.last().map_or(0.0, |e| e.geom.length_mm()),
            }
        };
        let (left, right) = (info(&cut.left), info(&cut.right));
        let mut refusal = None;
        let notch_distances_mm = match notches.positions(left.edge_mm) {
            Ok(v) => v,
            Err(e) => {
                refusal = Some(e.to_string());
                Vec::new()
            }
        };
        if refusal.is_none() {
            let mut trial = self.clone();
            refusal = trial
                .apply(crate::command::Command::CutPiece {
                    piece,
                    line: *line,
                    notches: *notches,
                })
                .err()
                .map(|e| e.to_string());
        }
        Ok(CutPreview {
            points: cut.points,
            cut_length_mm: left.edge_mm,
            left,
            right,
            notch_distances_mm,
            refusal,
        })
    }

    /// Tracés de la feuille de la pièce qui peuvent servir de ligne de découpe : segment, courbe
    /// de Bézier ou arc, hors du contour, qui traversent le contour de bord à bord (au plus
    /// [`CUT_MAX_CANDIDATES`]). Lecture seule, même calcul que la commande.
    pub fn cut_candidates(&self, piece: PieceId) -> Vec<EntityId> {
        let (Ok(sheet), Ok(chain), Ok(p)) = (
            self.piece_sheet(piece),
            self.piece_chain(piece),
            self.piece(piece),
        ) else {
            return Vec::new();
        };
        let geoms = geoms_of(&chain);
        sheet
            .entities
            .iter()
            .filter(|e| !p.outline.iter().any(|o| o.entity == e.id))
            .filter_map(|e| {
                let g = edge_of_shape(&sheet.shape(e.id).ok()?)?;
                cut_chain_by_edge(&geoms, &g).ok().map(|_| e.id)
            })
            .take(CUT_MAX_CANDIDATES)
            .collect()
    }

    /// Ligne proposée par défaut : verticale au milieu de la boîte du contour, qui la dépasse de
    /// 10 mm de chaque côté.
    pub fn default_cut_line(&self, piece: PieceId) -> Option<(PointMm, PointMm)> {
        let chain = self.piece_chain(piece).ok()?;
        let pts: Vec<PointMm> = chain
            .iter()
            .flat_map(|e| pf_geometry::split::edge_sample(&e.geom, 16))
            .collect();
        let min_x = pts.iter().map(|p| p.x).fold(f64::INFINITY, f64::min);
        let max_x = pts.iter().map(|p| p.x).fold(f64::NEG_INFINITY, f64::max);
        let min_y = pts.iter().map(|p| p.y).fold(f64::INFINITY, f64::min);
        let max_y = pts.iter().map(|p| p.y).fold(f64::NEG_INFINITY, f64::max);
        let x = 0.5 * (min_x + max_x);
        (x.is_finite() && min_y.is_finite() && max_y.is_finite())
            .then(|| (PointMm::new(x, min_y - 10.0), PointMm::new(x, max_y + 10.0)))
    }

    pub(crate) fn cut_piece(
        &mut self,
        piece: PieceId,
        line: CutLine,
        notches: CutNotches,
        out: &mut Outcome,
    ) -> Result<(), CoreError> {
        notches.validate()?;
        let geom = self.cut_line_geom(piece, &line)?;
        let chain = self.piece_chain(piece)?;
        let cut =
            cut_chain_by_edge(&geoms_of(&chain), &geom).map_err(|e| invalid(e.to_string()))?;
        let cut_len = cut.left.last().map_or(0.0, |e| e.geom.length_mm());
        // Refus avant toute écriture : les positions des crans doivent exister.
        let count = notches.positions(cut_len)?.len();
        self.split_with_cut(piece, &chain, cut, Some(&notches), out)?;
        for id in out.pieces.clone() {
            let c = self.piece_contour(id)?;
            if c.self_intersecting || !c.area_mm2.is_finite() || c.area_mm2 <= 1e-9 {
                return Err(invalid(
                    "la découpe donnerait un morceau dont le contour se recoupe ou est de surface nulle",
                ));
            }
        }
        out.notes.push(format!(
            "Ligne de découpe de {cut_len:.3} mm : {count} cran(s) de repère sur chacun des deux \
             bords, aux mêmes distances depuis le départ de la ligne."
        ));
        Ok(())
    }

    /// Pose les crans de repère sur l'arête de découpe `edge` de la feuille `sheet`. `flip` : bord
    /// de la moitié de droite, construit de `Q` vers `P` ; on le retourne (le tracé va de `P` à
    /// `Q`, l'arête du contour est marquée « retournée »).
    pub(crate) fn mark_cut_edge(
        &mut self,
        sheet: SheetId,
        edge: &mut PieceEdge,
        flip: bool,
        spec: &CutNotches,
        out: &mut Outcome,
    ) -> Result<(), CoreError> {
        if flip {
            let s = self.sheet_mut(sheet)?;
            let e = s
                .entities
                .iter_mut()
                .find(|e| e.id == edge.entity)
                .ok_or(CoreError::UnknownEntity(edge.entity))?;
            match &mut e.kind {
                EntityKind::Segment { from, to, .. } => std::mem::swap(from, to),
                EntityKind::Curve {
                    start,
                    handle_start,
                    handle_end,
                    end,
                } => {
                    std::mem::swap(start, end);
                    std::mem::swap(handle_start, handle_end);
                }
                EntityKind::Arc {
                    start, end, ccw, ..
                } => {
                    std::mem::swap(start, end);
                    *ccw = !*ccw;
                }
                _ => return Err(invalid("arête de découpe inattendue")),
            }
            edge.reversed = !edge.reversed;
        }
        let len = self.sheet(sheet)?.path_length_mm(edge.entity)?;
        // Moitié de gauche : l'intérieur est à gauche de `P → Q`, l'extérieur à droite.
        let side = if flip { Side::Left } else { Side::Right };
        for d in spec.positions(len)? {
            let id = NotchId(self.alloc_id());
            self.sheet_mut(sheet)?.notches.push(Notch {
                id,
                entity: edge.entity,
                distance_mm: d.clamp(0.0, len),
                side,
                length_mm: crate::notch::DEFAULT_NOTCH_LENGTH_MM,
                style: NotchStyle::Single,
                anchor: None,
                form: NotchForm::Line,
                width_mm: None,
                on_cut: false,
                corner: false,
                proportional: false,
            });
            out.notches.push(id);
        }
        Ok(())
    }
}
