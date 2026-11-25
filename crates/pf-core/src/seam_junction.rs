//! US-08.3 — contrôle des raccords d'un couple apparié (US-08.1) : continuité de courbe quand
//! deux pièces sont assemblées. **Lecture seule** : aucun champ, aucune `Command`, la mesure
//! n'est jamais une contrainte ([`Project::seam_junction_report`] recalcule à chaque appel).
//!
//! Conventions d'atelier (non validées) :
//! - lecture sur le contour de **couture dessiné** (jamais la coupe) ; une jonction = une
//!   extrémité d'un couple (deux par couple, début et fin du côté A, chacune avec l'extrémité
//!   de B qui lui est cousue) ;
//! - **angle intérieur** d'une pièce à la jonction = angle, du côté de la matière, entre la
//!   tangente de la couture et celle du bord qui suit le contour ; pour une courbe, la tangente
//!   de la courbe à l'extrémité (jamais la corde) ; `]0 ; 360[`, sens du contour pris en compte ;
//! - **angle assemblé** = somme des deux angles intérieurs ; **raccord lisse** quand elle vaut
//!   180° ; **écart** = somme − 180° (signé : > 0 « bosse », < 0 « creux ») ;
//! - tolérance de raccord : **2°** par défaut ([`JUNCTION_DEFAULT_TOLERANCE_DEG`], 0 à 90, non
//!   stockée), limite incluse (à 1e-9° près : bruit de flottant) ;
//! - extrémité de A cousue à quelle extrémité de B : **la logique de la marche de pièces**
//!   (US-08.2, `partner_is_end` = `march_target`, sens de couture déduit du sens des contours) ;
//! - **vue assemblée** : B déplacé rigidement (jamais retourné) pour que l'extrémité de B cousue
//!   au début de A coïncide avec lui et que les cordes de couture soient alignées ; l'écart à
//!   l'autre extrémité (embu) est affiché, jamais corrigé ; le modèle n'est pas modifié.

use crate::error::CoreError;
use crate::model::*;
use crate::offset::trace_of;
use crate::seam_march::{partner_is_end, sewn_opposite};
use crate::seam_pair::{SeamIssue, SeamPair, SeamSide, SeamWhich};
use pf_geometry::junction::vertex_interior_angle_deg;
use pf_geometry::region::{trace_geometry, EdgeGeom};
use pf_geometry::split::{edge_sample, Rigid};
use pf_geometry::{distance_mm, PointMm, VectorMm};

/// Tolérance de raccord par défaut (degrés).
pub const JUNCTION_DEFAULT_TOLERANCE_DEG: f64 = 2.0;
/// Plus grande tolérance de raccord acceptée (degrés).
pub const JUNCTION_MAX_TOLERANCE_DEG: f64 = 90.0;
/// Bruit de flottant admis à la limite de tolérance (degrés).
const LIMIT_EPS_DEG: f64 = 1e-9;
/// Pas d'échantillonnage des courbes de la vue assemblée (comme `contour_polyline`).
const VIEW_SAMPLES: usize = 48;
/// Une corde plus courte (mm) ne peut pas orienter la vue assemblée.
const MIN_CHORD_MM: f64 = 1e-9;

/// Extrémité d'un côté de couture (dans l'ordre du contour de sa pièce).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JunctionEnd {
    Start,
    End,
}

impl JunctionEnd {
    pub fn other(self) -> JunctionEnd {
        match self {
            JunctionEnd::Start => JunctionEnd::End,
            JunctionEnd::End => JunctionEnd::Start,
        }
    }
}

/// Raison pour laquelle un raccord n'est pas calculable aujourd'hui.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JunctionIssue {
    /// Le couple lui-même n'est pas lisible (contour ouvert, tracé dégénéré, pièce absente…).
    Seam(SeamIssue),
    /// Une tangente est indéfinie (courbe ou bord de longueur nulle).
    TangentUndefined,
    /// Les deux bords se replient l'un sur l'autre (angle intérieur de 0° ou 360°).
    Cusp,
    /// Vue assemblée : la corde de couture d'un côté est nulle (couture qui fait tout le tour).
    ChordDegenerate,
}

impl JunctionIssue {
    pub fn key(&self) -> &'static str {
        match self {
            JunctionIssue::Seam(r) => r.key(),
            JunctionIssue::TangentUndefined => "tangent_undefined",
            JunctionIssue::Cusp => "cusp",
            JunctionIssue::ChordDegenerate => "chord_degenerate",
        }
    }
}

/// Verdict d'une jonction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JunctionStatus {
    /// |écart| ≤ tolérance.
    Smooth,
    Broken,
    NotComputable {
        side: SeamWhich,
        reason: JunctionIssue,
    },
}

/// Lecture d'une jonction (valeurs **observées**, recalculées à chaque appel).
#[derive(Debug, Clone, PartialEq)]
pub struct JunctionReading {
    pub pair: SeamPairId,
    /// Rang du couple (à partir de 1), comme `SeamPairReading::number`.
    pub number: usize,
    /// Extrémité de A.
    pub end: JunctionEnd,
    /// Extrémité de B qui lui est cousue.
    pub partner_end: JunctionEnd,
    /// Sommet de jonction sur la pièce A / B (repère de la feuille de chaque pièce).
    pub point_a: Option<PointMm>,
    pub point_b: Option<PointMm>,
    pub angle_a_deg: Option<f64>,
    pub angle_b_deg: Option<f64>,
    /// Angle à la jonction assemblée = angle A + angle B.
    pub sum_deg: Option<f64>,
    /// Somme − 180° (> 0 : bosse, < 0 : creux).
    pub deviation_deg: Option<f64>,
    pub tolerance_deg: f64,
    pub status: JunctionStatus,
}

/// Une jonction dans la vue assemblée.
#[derive(Debug, Clone, PartialEq)]
pub struct AssembledJunction {
    pub reading: JunctionReading,
    /// Sommet de A (repère assemblé = repère de la feuille de A) et sommet de B placé.
    pub point_a: PointMm,
    pub point_b: PointMm,
    /// Distance entre les deux : 0 au début de la couture, différence des cordes à la fin.
    pub gap_mm: f64,
}

/// Vue assemblée d'un couple : données de dessin, rien n'est modifié.
#[derive(Debug, Clone, PartialEq)]
pub struct AssembledView {
    pub pair: SeamPairId,
    pub number: usize,
    /// Contours fermés (polylignes mm) : A tel quel, B déplacé rigidement.
    pub contour_a: Vec<PointMm>,
    pub contour_b: Vec<PointMm>,
    /// Lignes de couture seules.
    pub seam_a: Vec<PointMm>,
    pub seam_b: Vec<PointMm>,
    /// Début puis fin de la couture de A.
    pub junctions: Vec<AssembledJunction>,
    pub chord_a_mm: f64,
    pub chord_b_mm: f64,
    /// Écart à l'extrémité de fin (embu visible, jamais corrigé).
    pub end_gap_mm: f64,
    /// Rotation appliquée à B (degrés).
    pub rotation_deg: f64,
}

/// Les deux côtés d'un couple lus, avec le sens de couture et les longueurs de couture.
struct PairCalc {
    a: SideCalc,
    b: SideCalc,
    opposite: bool,
    ta: f64,
    tb: f64,
}

/// Géométrie lue d'un côté : contour entier en arêtes élémentaires, dans l'ordre du contour.
struct SideCalc {
    geoms: Vec<EdgeGeom>,
    /// Pour chaque tracé du contour, `[début, fin[` dans `geoms`.
    spans: Vec<(usize, usize)>,
    clockwise: bool,
    /// Rang, dans le contour, du premier tracé de la couture et nombre de tracés cousus.
    first: usize,
    count: usize,
}

impl SideCalc {
    fn n(&self) -> usize {
        self.spans.len()
    }

    /// Arête élémentaire qui porte l'extrémité, et tracé du contour qui la suit / la précède.
    fn first_geom(&self, entry: usize) -> Result<&EdgeGeom, JunctionIssue> {
        let (a, b) = self.spans[entry];
        (a < b)
            .then(|| &self.geoms[a])
            .ok_or(JunctionIssue::Seam(SeamIssue::Degenerate))
    }

    fn last_geom(&self, entry: usize) -> Result<&EdgeGeom, JunctionIssue> {
        let (a, b) = self.spans[entry];
        (a < b)
            .then(|| &self.geoms[b - 1])
            .ok_or(JunctionIssue::Seam(SeamIssue::Degenerate))
    }

    fn point(&self, end: JunctionEnd) -> Result<PointMm, JunctionIssue> {
        Ok(match end {
            JunctionEnd::Start => self.first_geom(self.first)?.start(),
            JunctionEnd::End => self
                .last_geom((self.first + self.count - 1) % self.n())?
                .end(),
        })
    }

    /// Angle intérieur de la pièce à l'extrémité de la couture.
    fn angle(&self, end: JunctionEnd) -> Result<f64, JunctionIssue> {
        let n = self.n();
        let (incoming, outgoing) = match end {
            JunctionEnd::Start => (
                self.last_geom((self.first + n - 1) % n)?.end_tangent(),
                self.first_geom(self.first)?.start_tangent(),
            ),
            JunctionEnd::End => (
                self.last_geom((self.first + self.count - 1) % n)?
                    .end_tangent(),
                self.first_geom((self.first + self.count) % n)?
                    .start_tangent(),
            ),
        };
        let (Some(i), Some(o)) = (incoming, outgoing) else {
            return Err(JunctionIssue::TangentUndefined);
        };
        let finite = |v: VectorMm| v.x.is_finite() && v.y.is_finite();
        if !(finite(i) && finite(o)) {
            return Err(JunctionIssue::TangentUndefined);
        }
        vertex_interior_angle_deg(i, o, self.clockwise).ok_or(JunctionIssue::Cusp)
    }

    /// Arêtes élémentaires de la couture seule.
    fn seam_geoms(&self) -> Vec<EdgeGeom> {
        let n = self.n();
        (0..self.count)
            .flat_map(|k| {
                let (a, b) = self.spans[(self.first + k) % n];
                self.geoms[a..b].iter().copied()
            })
            .collect()
    }
}

/// Polyligne d'une suite d'arêtes (points consécutifs confondus fusionnés).
fn polyline(edges: impl IntoIterator<Item = EdgeGeom>, close: bool) -> Vec<PointMm> {
    let mut out: Vec<PointMm> = Vec::new();
    for e in edges {
        for p in edge_sample(&e, VIEW_SAMPLES) {
            if out.last().is_none_or(|l| distance_mm(*l, p) > 1e-12) {
                out.push(p);
            }
        }
    }
    if close && out.len() > 2 && distance_mm(out[0], out[out.len() - 1]) <= 1e-9 {
        out.pop();
    }
    out
}

impl Project {
    fn junction_side(&self, side: &SeamSide) -> Result<SideCalc, SeamIssue> {
        self.seam_side_length(side)?;
        let piece = self
            .piece(side.piece)
            .map_err(|_| SeamIssue::PieceMissing)?;
        let clockwise = self
            .piece_contour(side.piece)
            .map_err(|_| SeamIssue::OutlineOpen)?
            .clockwise;
        let sheet = self
            .piece_sheet(side.piece)
            .map_err(|_| SeamIssue::OutlineOpen)?;
        let n = piece.outline.len();
        let first = piece
            .outline
            .iter()
            .position(|e| Some(&e.entity) == side.edges.first())
            .ok_or(SeamIssue::EdgeNotInOutline)?;
        for (k, e) in side.edges.iter().enumerate() {
            if piece.outline[(first + k) % n].entity != *e {
                return Err(SeamIssue::EdgeNotInOutline);
            }
        }
        let mut geoms = Vec::new();
        let mut spans = Vec::new();
        for e in &piece.outline {
            let shape = sheet.shape(e.entity).map_err(|_| SeamIssue::Degenerate)?;
            let a = geoms.len();
            geoms.extend(trace_geometry(&trace_of(shape, e.reversed)));
            spans.push((a, geoms.len()));
        }
        Ok(SideCalc {
            geoms,
            spans,
            clockwise,
            first,
            count: side.edges.len(),
        })
    }

    /// Les deux côtés d'un couple, lus, avec le sens de couture (marche de pièces).
    fn junction_pair(&self, p: &SeamPair) -> Result<PairCalc, (SeamWhich, JunctionIssue)> {
        let seam = |w, r| (w, JunctionIssue::Seam(r));
        let a = self
            .junction_side(&p.a)
            .map_err(|r| seam(SeamWhich::A, r))?;
        let b = self
            .junction_side(&p.b)
            .map_err(|r| seam(SeamWhich::B, r))?;
        let fa = self
            .march_frame(&p.a, "côté A")
            .map_err(|_| seam(SeamWhich::A, SeamIssue::Degenerate))?;
        let fb = self
            .march_frame(&p.b, "côté B")
            .map_err(|_| seam(SeamWhich::B, SeamIssue::Degenerate))?;
        Ok(PairCalc {
            a,
            b,
            opposite: sewn_opposite(&fa, &fb),
            ta: fa.total,
            tb: fb.total,
        })
    }

    fn junction_rows(&self, number: usize, p: &SeamPair, tol: f64) -> Vec<JunctionReading> {
        let pair = self.junction_pair(p);
        [JunctionEnd::Start, JunctionEnd::End]
            .into_iter()
            .map(|end| {
                let mut r = JunctionReading {
                    pair: p.id,
                    number,
                    end,
                    partner_end: end.other(),
                    point_a: None,
                    point_b: None,
                    angle_a_deg: None,
                    angle_b_deg: None,
                    sum_deg: None,
                    deviation_deg: None,
                    tolerance_deg: tol,
                    status: JunctionStatus::Smooth,
                };
                let PairCalc {
                    a,
                    b,
                    opposite,
                    ta,
                    tb,
                } = match &pair {
                    Ok(x) => x,
                    Err((side, reason)) => {
                        r.status = JunctionStatus::NotComputable {
                            side: *side,
                            reason: *reason,
                        };
                        return r;
                    }
                };
                let partner = if partner_is_end(end == JunctionEnd::End, *ta, *tb, *opposite) {
                    JunctionEnd::End
                } else {
                    JunctionEnd::Start
                };
                r.partner_end = partner;
                r.point_a = a.point(end).ok();
                r.point_b = b.point(partner).ok();
                let (aa, ab) = (a.angle(end), b.angle(partner));
                r.angle_a_deg = aa.ok();
                r.angle_b_deg = ab.ok();
                match (aa, ab) {
                    (Err(reason), _) => {
                        r.status = JunctionStatus::NotComputable {
                            side: SeamWhich::A,
                            reason,
                        }
                    }
                    (_, Err(reason)) => {
                        r.status = JunctionStatus::NotComputable {
                            side: SeamWhich::B,
                            reason,
                        }
                    }
                    (Ok(x), Ok(y)) => {
                        let sum = x + y;
                        let dev = sum - 180.0;
                        r.sum_deg = Some(sum);
                        r.deviation_deg = Some(dev);
                        if dev.abs() > tol + LIMIT_EPS_DEG {
                            r.status = JunctionStatus::Broken;
                        }
                    }
                }
                r
            })
            .collect()
    }

    /// Raccords de chaque couple (deux lignes par couple : début puis fin du côté A), avec la
    /// tolérance de raccord par défaut.
    pub fn seam_junction_report(&self) -> Vec<JunctionReading> {
        self.seam_junction_report_with(JUNCTION_DEFAULT_TOLERANCE_DEG)
            .unwrap_or_default()
    }

    /// Idem avec une tolérance explicite. Refus : tolérance non finie, négative ou > 90°.
    pub fn seam_junction_report_with(
        &self,
        tolerance_deg: f64,
    ) -> Result<Vec<JunctionReading>, CoreError> {
        if !tolerance_deg.is_finite()
            || !(0.0..=JUNCTION_MAX_TOLERANCE_DEG).contains(&tolerance_deg)
        {
            return Err(CoreError::Invalid(format!(
                "la tolérance de raccord doit être comprise entre 0 et {JUNCTION_MAX_TOLERANCE_DEG} degrés"
            )));
        }
        Ok(self
            .seams
            .iter()
            .enumerate()
            .flat_map(|(i, p)| self.junction_rows(i + 1, p, tolerance_deg))
            .collect())
    }

    /// Vue assemblée du couple (tolérance par défaut), sans rien modifier. Refus : couple
    /// inconnu, couple non calculable, cordes de couture nulles.
    pub fn seam_assembled_view(&self, pair: SeamPairId) -> Result<AssembledView, CoreError> {
        let bad = |m: String| CoreError::Invalid(m);
        let (i, p) = self
            .seams
            .iter()
            .enumerate()
            .find(|(_, s)| s.id == pair)
            .ok_or_else(|| bad("couture appariée inconnue".into()))?;
        let PairCalc { a, b, .. } = self
            .junction_pair(p)
            .map_err(|(_, r)| bad(format!("couple non calculable ({})", r.key())))?;
        let rows = self.junction_rows(i + 1, p, JUNCTION_DEFAULT_TOLERANCE_DEG);
        let chord = |r: Result<PointMm, JunctionIssue>| {
            r.map_err(|e| bad(format!("couple non calculable ({})", e.key())))
        };
        let (a0, a1) = (
            chord(a.point(JunctionEnd::Start))?,
            chord(a.point(JunctionEnd::End))?,
        );
        let (b0, b1) = (
            chord(b.point(rows[0].partner_end))?,
            chord(b.point(rows[1].partner_end))?,
        );
        let (chord_a, chord_b) = (distance_mm(a0, a1), distance_mm(b0, b1));
        if chord_a <= MIN_CHORD_MM || chord_b <= MIN_CHORD_MM {
            return Err(bad(format!(
                "vue assemblée impossible ({})",
                JunctionIssue::ChordDegenerate.key()
            )));
        }
        let rigid = Rigid::between_starts(b0, b1, a0, a1)
            .map_err(|e| bad(format!("vue assemblée impossible : {e}")))?;
        let moved = |edges: &[EdgeGeom]| -> Vec<EdgeGeom> {
            edges.iter().map(|e| rigid.apply_edge(e)).collect()
        };
        let junctions: Vec<AssembledJunction> = rows
            .iter()
            .zip([(a0, b0), (a1, b1)])
            .map(|(r, (pa, pb))| {
                let pb = rigid.apply(pb);
                AssembledJunction {
                    reading: r.clone(),
                    point_a: pa,
                    point_b: pb,
                    gap_mm: distance_mm(pa, pb),
                }
            })
            .collect();
        let end_gap_mm = junctions[1].gap_mm;
        Ok(AssembledView {
            pair,
            number: i + 1,
            contour_a: polyline(a.geoms.iter().copied(), true),
            contour_b: polyline(moved(&b.geoms), true),
            seam_a: polyline(a.seam_geoms(), false),
            seam_b: polyline(moved(&b.seam_geoms()), false),
            junctions,
            chord_a_mm: chord_a,
            chord_b_mm: chord_b,
            end_gap_mm,
            rotation_deg: rigid.angle_rad().to_degrees(),
        })
    }
}
