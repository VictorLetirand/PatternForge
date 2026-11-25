//! US-08.4 — contrôle des crans d'un couple apparié (US-08.1) : chaque cran d'un côté doit avoir son
//! vis-à-vis sur l'autre côté. **Lecture seule** : rien n'est stocké, la mesure n'est jamais une
//! contrainte ([`Project::notch_check_report`] recalcule à chaque appel).
//!
//! Le vis-à-vis **attendu** d'un cran se calcule exactement comme la marche de pièces (US-08.2,
//! `march_target` : même abscisse curviligne, embu proportionnel, sens de couture déduit du sens des
//! contours).
//!
//! Conventions d'atelier (non validées) :
//! - tolérance de cran : **1 mm** par défaut ([`NOTCH_CHECK_TOLERANCE_MM`]), limite incluse ; non
//!   stockée (paramètre de [`Project::notch_check_report_with`]) ;
//! - l'écart d'un couple (cran A, cran B) est le **plus grand** des deux écarts, mesurés chacun sur
//!   le côté qui reçoit le vis-à-vis attendu (donc dans les deux repères) ;
//! - appariement **un-pour-un**, glouton par écart croissant (égalités : ordre d'abscisse puis
//!   identifiant) ; écart ≤ tolérance : *apparié* ; sinon, jusqu'à [`NOTCH_SHIFT_RANGE_MM`] (ou la
//!   tolérance si elle est plus grande) : *décalé* ; sinon *manquant* ;
//! - crans à la même position (±0,01 mm, mesuré aussi après report) : comptés une fois.

use crate::error::CoreError;
use crate::model::*;
use crate::seam_march::{march_target, sewn_opposite, Frame, MARCH_DUPLICATE_TOLERANCE_MM};
use crate::seam_pair::{SeamIssue, SeamPair, SeamStatus, SeamWhich};
use pf_geometry::Side;

/// Tolérance de cran par défaut (mm).
pub const NOTCH_CHECK_TOLERANCE_MM: f64 = 1.0;
/// Écart maximal (mm) au-delà duquel un cran n'est plus « décalé » mais « manquant ».
pub const NOTCH_SHIFT_RANGE_MM: f64 = 10.0;

/// Verdict d'un cran.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotchMatch {
    /// Vis-à-vis trouvé à moins de la tolérance.
    Matched,
    /// Vis-à-vis trouvé, mais plus loin que la tolérance.
    Shifted,
    /// Aucun vis-à-vis dans la plage.
    Missing,
}

/// Un cran d'un côté du couple et son vis-à-vis.
#[derive(Debug, Clone, PartialEq)]
pub struct NotchCheckItem {
    pub notch: NotchId,
    /// Côté du couple qui porte ce cran.
    pub side: SeamWhich,
    pub entity: EntityId,
    pub distance_mm: f64,
    /// Côté de la pièce (gauche / droite du tracé) du cran.
    pub notch_side: Side,
    /// Abscisse du cran sur son côté (mm depuis le début du côté).
    pub position_mm: f64,
    /// Abscisse attendue de son vis-à-vis sur l'autre côté (comme la marche de pièces).
    pub expected_mm: f64,
    /// Vis-à-vis retenu (appariement un-pour-un) et son abscisse sur l'autre côté.
    pub partner: Option<NotchId>,
    pub found_mm: Option<f64>,
    /// |attendue − trouvée| sur l'autre côté.
    pub gap_mm: Option<f64>,
    pub status: NotchMatch,
}

/// Lecture d'un couple.
#[derive(Debug, Clone, PartialEq)]
pub struct NotchCheckReading {
    pub id: SeamPairId,
    /// Rang du couple (à partir de 1), comme `SeamPairReading::number`.
    pub number: usize,
    pub tolerance_mm: f64,
    /// Couple non calculable : côté et raison (alors `items` est vide).
    pub not_computable: Option<(SeamWhich, SeamIssue)>,
    /// Crans de A puis de B, chacun dans l'ordre des abscisses.
    pub items: Vec<NotchCheckItem>,
}

impl NotchCheckReading {
    pub fn count(&self, side: SeamWhich) -> usize {
        self.items.iter().filter(|i| i.side == side).count()
    }
    pub fn count_status(&self, status: NotchMatch) -> usize {
        self.items.iter().filter(|i| i.status == status).count()
    }
    /// Crans sans vis-à-vis, d'un côté.
    pub fn missing(&self, side: SeamWhich) -> Vec<NotchId> {
        self.items
            .iter()
            .filter(|i| i.side == side && i.status == NotchMatch::Missing)
            .map(|i| i.notch)
            .collect()
    }
}

struct Cand {
    id: NotchId,
    entity: EntityId,
    distance_mm: f64,
    side: Side,
    pos: f64,
}

/// Écart absolu, circulaire quand le côté est le contour entier.
fn gap(d: f64, f: &Frame) -> f64 {
    let d = d.abs();
    if f.whole {
        d.min((f.total - d).max(0.0))
    } else {
        d
    }
}

impl Project {
    /// Crans d'un côté (dans l'ordre des abscisses), doublons retirés.
    fn check_cands(&self, f: &Frame, other_total: f64) -> Result<Vec<Cand>, CoreError> {
        let mut all = Vec::new();
        for n in &self.sheet(f.sheet)?.notches {
            if let Some(pos) = self.march_abscissa(f, n)? {
                all.push(Cand {
                    id: n.id,
                    entity: n.entity,
                    distance_mm: n.distance_mm,
                    side: n.side,
                    pos,
                });
            }
        }
        all.sort_by(|a, b| a.pos.total_cmp(&b.pos).then(a.id.0.cmp(&b.id.0)));
        let tol = MARCH_DUPLICATE_TOLERANCE_MM / (other_total / f.total).min(1.0);
        let mut kept: Vec<Cand> = Vec::new();
        for c in all {
            if !kept.iter().any(|k| gap(k.pos - c.pos, f) <= tol) {
                kept.push(c);
            }
        }
        Ok(kept)
    }

    fn check_pair(&self, p: &SeamPair, tol: f64) -> Result<Vec<NotchCheckItem>, CoreError> {
        let fa = self.march_frame(&p.a, "côté A")?;
        let fb = self.march_frame(&p.b, "côté B")?;
        let opposite = sewn_opposite(&fa, &fb);
        let na = self.check_cands(&fa, fb.total)?;
        let nb = self.check_cands(&fb, fa.total)?;
        let ea: Vec<f64> = na
            .iter()
            .map(|c| march_target(c.pos, fa.total, fb.total, opposite).1)
            .collect();
        let eb: Vec<f64> = nb
            .iter()
            .map(|c| march_target(c.pos, fb.total, fa.total, opposite).1)
            .collect();
        let range = NOTCH_SHIFT_RANGE_MM.max(tol);
        let mut cands: Vec<(f64, usize, usize)> = Vec::new();
        for (i, a) in na.iter().enumerate() {
            for (j, b) in nb.iter().enumerate() {
                let cost = gap(ea[i] - b.pos, &fb).max(gap(eb[j] - a.pos, &fa));
                if cost <= range {
                    cands.push((cost, i, j));
                }
            }
        }
        cands.sort_by(|x, y| x.0.total_cmp(&y.0).then(x.1.cmp(&y.1)).then(x.2.cmp(&y.2)));
        let mut ma: Vec<Option<(usize, f64)>> = vec![None; na.len()];
        let mut mb: Vec<Option<(usize, f64)>> = vec![None; nb.len()];
        for (cost, i, j) in cands {
            if ma[i].is_none() && mb[j].is_none() {
                ma[i] = Some((j, cost));
                mb[j] = Some((i, cost));
            }
        }
        let mut items = Vec::new();
        let mut push = |side: SeamWhich,
                        c: &Cand,
                        expected: f64,
                        partner: Option<(usize, f64)>,
                        other: &[Cand],
                        other_frame: &Frame| {
            let found = partner.map(|(k, _)| other[k].pos);
            items.push(NotchCheckItem {
                notch: c.id,
                side,
                entity: c.entity,
                distance_mm: c.distance_mm,
                notch_side: c.side,
                position_mm: c.pos,
                expected_mm: expected,
                partner: partner.map(|(k, _)| other[k].id),
                found_mm: found,
                gap_mm: found.map(|f| gap(expected - f, other_frame)),
                status: match partner {
                    None => NotchMatch::Missing,
                    Some((_, cost)) if cost <= tol => NotchMatch::Matched,
                    Some(_) => NotchMatch::Shifted,
                },
            });
        };
        for (i, c) in na.iter().enumerate() {
            push(SeamWhich::A, c, ea[i], ma[i], &nb, &fb);
        }
        for (j, c) in nb.iter().enumerate() {
            push(SeamWhich::B, c, eb[j], mb[j], &na, &fa);
        }
        Ok(items)
    }

    /// Contrôle des crans de chaque couple, avec la tolérance de cran par défaut.
    pub fn notch_check_report(&self) -> Vec<NotchCheckReading> {
        self.notch_check_report_with(NOTCH_CHECK_TOLERANCE_MM)
    }

    /// Idem avec une tolérance de cran explicite (mm, ≥ 0 ; sinon la valeur par défaut).
    pub fn notch_check_report_with(&self, tolerance_mm: f64) -> Vec<NotchCheckReading> {
        let tol = if tolerance_mm.is_finite() && tolerance_mm >= 0.0 {
            tolerance_mm
        } else {
            NOTCH_CHECK_TOLERANCE_MM
        };
        let seams = self.seam_pair_report();
        self.seams
            .iter()
            .zip(seams)
            .map(|(p, r)| {
                let mut out = NotchCheckReading {
                    id: p.id,
                    number: r.number,
                    tolerance_mm: tol,
                    not_computable: None,
                    items: Vec::new(),
                };
                if let SeamStatus::NotComputable { side, reason } = r.status {
                    out.not_computable = Some((side, reason));
                } else {
                    match self.check_pair(p, tol) {
                        Ok(items) => out.items = items,
                        Err(_) => out.not_computable = Some((SeamWhich::A, SeamIssue::Degenerate)),
                    }
                }
                out
            })
            .collect()
    }
}
