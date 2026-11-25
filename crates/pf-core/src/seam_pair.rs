//! US-08.1 — coutures appariées (décision 303).
//!
//! Un **couple** (`Project.seams`) est une **déclaration** : « ce bord de la pièce 1 se coud avec
//! ce bord de la pièce 2 ». Un bord = une pièce + une **suite continue de tracés de son contour
//! de couture** (dans l'ordre du contour, le départ pouvant faire le tour de la fermeture).
//! Longueurs, écart, embu mesuré et statut sont des **mesures observées** recalculées à chaque
//! lecture par [`Project::seam_pair_report`] : jamais stockées, jamais des contraintes (aucune
//! longueur n'est imposée à qui que ce soit).
//!
//! Conventions d'atelier (non validées) :
//! - longueur = longueur **développée du contour de couture** (tracés dessinés de la pièce, la
//!   marge de couture et le contour de coupe n'y comptent pas) ;
//! - le côté **A est le côté à embuer** : embu mesuré = longueur A − longueur B (négatif si A est
//!   plus court), embu attendu = valeur déclarée ≥ 0 (absente = 0, couture sans embu) ;
//! - écart = |A − B| ; statut « dans la tolérance » quand |embu mesuré − embu attendu| ≤
//!   tolérance (limite incluse), tolérance par défaut 1 mm ;
//! - une arête n'appartient qu'à **un** couple (refus sinon) ; les deux côtés peuvent être de la
//!   même pièce s'ils n'ont aucune arête commune (couture de tube, de pince dessinée en bord).

use crate::command::Outcome;
use crate::error::CoreError;
use crate::measure::entity_length;
use crate::model::*;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Tolérance par défaut d'un couple (mm), quand elle n'est pas déclarée.
pub const SEAM_DEFAULT_TOLERANCE_MM: f64 = 1.0;
/// Borne haute de l'embu attendu et de la tolérance (mm).
pub const SEAM_MAX_VALUE_MM: f64 = 1000.0;
/// Nombre maximal de couples par projet.
pub const SEAM_MAX_PAIRS: usize = 1000;

/// Un bord à coudre : une pièce et une suite continue de tracés de son contour.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeamSide {
    pub piece: PieceId,
    /// Tracés consécutifs du contour de la pièce, dans l'ordre du contour.
    pub edges: Vec<EntityId>,
}

/// Couple de bords appariés (déclaration stockée).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeamPair {
    pub id: SeamPairId,
    /// Côté à embuer.
    pub a: SeamSide,
    pub b: SeamSide,
    /// Embu attendu (mm, 0 à 1000) ; absent = aucun embu.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub embu_mm: Option<f64>,
    /// Tolérance (mm, 0 à 1000) ; absente = [`SEAM_DEFAULT_TOLERANCE_MM`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tolerance_mm: Option<f64>,
}

impl SeamPair {
    pub fn tolerance(&self) -> f64 {
        self.tolerance_mm.unwrap_or(SEAM_DEFAULT_TOLERANCE_MM)
    }
}

/// Côté d'un couple.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SeamWhich {
    A,
    B,
}

/// Raison pour laquelle un couple n'est pas calculable aujourd'hui.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeamIssue {
    /// La pièce n'existe plus.
    PieceMissing,
    /// Le contour de la pièce est vide, ouvert ou illisible.
    OutlineOpen,
    /// Un tracé du côté n'est plus dans le contour de la pièce.
    EdgeNotInOutline,
    /// Un tracé est dégénéré (longueur illisible ou nulle).
    Degenerate,
}

impl SeamIssue {
    pub fn key(&self) -> &'static str {
        match self {
            SeamIssue::PieceMissing => "piece_missing",
            SeamIssue::OutlineOpen => "outline_open",
            SeamIssue::EdgeNotInOutline => "edge_not_in_outline",
            SeamIssue::Degenerate => "degenerate",
        }
    }
}

/// Verdict d'un couple.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeamStatus {
    /// |embu mesuré − embu attendu| ≤ tolérance.
    Within,
    OutOfTolerance,
    NotComputable {
        side: SeamWhich,
        reason: SeamIssue,
    },
}

/// Lecture d'un couple (toutes les valeurs sont **observées**, recalculées à chaque appel).
#[derive(Debug, Clone, PartialEq)]
pub struct SeamPairReading {
    pub id: SeamPairId,
    /// Rang du couple dans la liste (à partir de 1) : libellé « couture n ».
    pub number: usize,
    pub length_a_mm: Option<f64>,
    pub length_b_mm: Option<f64>,
    /// |A − B|.
    pub gap_mm: Option<f64>,
    /// A − B (côté A à embuer ; négatif = A plus court).
    pub embu_measured_mm: Option<f64>,
    /// Valeur déclarée, 0 si absente.
    pub embu_expected_mm: f64,
    /// |embu mesuré − embu attendu|.
    pub deviation_mm: Option<f64>,
    pub tolerance_mm: f64,
    pub status: SeamStatus,
}

fn bad(msg: impl Into<String>) -> CoreError {
    CoreError::Invalid(msg.into())
}

impl SeamSide {
    /// Références valides : pièce existante, au moins un tracé, tracés consécutifs du contour
    /// (dans l'ordre, sans doublon).
    pub(crate) fn check(&self, project: &Project, what: &str) -> Result<(), CoreError> {
        let piece = project
            .piece(self.piece)
            .map_err(|_| bad(format!("{what} : pièce inconnue")))?;
        if self.edges.is_empty() {
            return Err(bad(format!("{what} : aucun tracé")));
        }
        let n = piece.outline.len();
        if self.edges.len() > n {
            return Err(bad(format!(
                "{what} : plus de tracés que le contour de la pièce « {} »",
                piece.name
            )));
        }
        let Some(start) = piece.outline.iter().position(|e| e.entity == self.edges[0]) else {
            return Err(bad(format!(
                "{what} : un tracé n'appartient pas au contour de la pièce « {} »",
                piece.name
            )));
        };
        for (i, e) in self.edges.iter().enumerate() {
            if piece.outline[(start + i) % n].entity != *e {
                return Err(bad(format!(
                    "{what} : les tracés ne se suivent pas dans le contour de la pièce « {} »",
                    piece.name
                )));
            }
        }
        Ok(())
    }
}

fn check_value(v: Option<f64>, what: &str) -> Result<(), CoreError> {
    if let Some(v) = v {
        if !v.is_finite() || !(0.0..=SEAM_MAX_VALUE_MM).contains(&v) {
            return Err(bad(format!(
                "{what} doit être comprise entre 0 et {SEAM_MAX_VALUE_MM} mm"
            )));
        }
    }
    Ok(())
}

impl Project {
    /// Contrôle de toutes les coutures appariées (ouverture et après chaque commande).
    pub(crate) fn validate_seam_pairs(&self) -> Result<(), CoreError> {
        if self.seams.len() > SEAM_MAX_PAIRS {
            return Err(bad(format!(
                "trop de coutures appariées (maximum {SEAM_MAX_PAIRS})"
            )));
        }
        let mut used: HashSet<(PieceId, EntityId)> = HashSet::new();
        for (i, s) in self.seams.iter().enumerate() {
            let n = i + 1;
            s.a.check(self, &format!("couture appariée {n}, côté A"))?;
            s.b.check(self, &format!("couture appariée {n}, côté B"))?;
            check_value(s.embu_mm, &format!("l'embu de la couture appariée {n}"))?;
            check_value(
                s.tolerance_mm,
                &format!("la tolérance de la couture appariée {n}"),
            )?;
            for side in [&s.a, &s.b] {
                for e in &side.edges {
                    if !used.insert((side.piece, *e)) {
                        return Err(bad(format!(
                            "couture appariée {n} : un tracé est déjà cousu dans un couple (une \
                             arête n'appartient qu'à un couple)"
                        )));
                    }
                }
            }
        }
        Ok(())
    }

    /// Côté de couture formé de `count` tracés consécutifs du contour de la pièce, à partir de
    /// `start` (le tour de la fermeture est permis). Refus : pièce ou tracé inconnu, `count` nul
    /// ou supérieur au nombre de tracés du contour.
    pub fn seam_side_range(
        &self,
        piece: PieceId,
        start: EntityId,
        count: usize,
    ) -> Result<SeamSide, CoreError> {
        let p = self.piece(piece)?;
        let n = p.outline.len();
        let i = p
            .outline
            .iter()
            .position(|e| e.entity == start)
            .ok_or_else(|| bad("ce tracé n'appartient pas au contour de la pièce"))?;
        if count == 0 || count > n {
            return Err(bad("nombre de tracés hors du contour"));
        }
        Ok(SeamSide {
            piece,
            edges: (0..count).map(|k| p.outline[(i + k) % n].entity).collect(),
        })
    }

    /// Longueur de couture d'un côté (mm) : somme des longueurs développées des tracés du contour
    /// dessiné (jamais la coupe).
    pub(crate) fn seam_side_length(&self, side: &SeamSide) -> Result<f64, SeamIssue> {
        let piece = self
            .piece(side.piece)
            .map_err(|_| SeamIssue::PieceMissing)?;
        match self.piece_contour(side.piece) {
            Ok(c) if c.closed => {}
            _ => return Err(SeamIssue::OutlineOpen),
        }
        let sheet = self
            .piece_sheet(side.piece)
            .map_err(|_| SeamIssue::OutlineOpen)?;
        let mut total = 0.0;
        for e in &side.edges {
            if !piece.outline.iter().any(|o| o.entity == *e) {
                return Err(SeamIssue::EdgeNotInOutline);
            }
            let l = entity_length(sheet, *e)
                .map_err(|_| SeamIssue::Degenerate)?
                .value;
            if !l.is_finite() || l <= 0.0 {
                return Err(SeamIssue::Degenerate);
            }
            total += l;
        }
        Ok(total)
    }

    /// Tableau des couples appariés : mesures **observées** recalculées à chaque appel (suit donc
    /// toute modification des pièces), jamais stockées. Un couple impossible à lire est
    /// `NotComputable`, jamais un refus de commande.
    pub fn seam_pair_report(&self) -> Vec<SeamPairReading> {
        self.seams
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let expected = s.embu_mm.unwrap_or(0.0);
                let tol = s.tolerance();
                let la = self.seam_side_length(&s.a);
                let lb = self.seam_side_length(&s.b);
                let mut r = SeamPairReading {
                    id: s.id,
                    number: i + 1,
                    length_a_mm: la.ok(),
                    length_b_mm: lb.ok(),
                    gap_mm: None,
                    embu_measured_mm: None,
                    embu_expected_mm: expected,
                    deviation_mm: None,
                    tolerance_mm: tol,
                    status: SeamStatus::Within,
                };
                match (la, lb) {
                    (Err(reason), _) => {
                        r.status = SeamStatus::NotComputable {
                            side: SeamWhich::A,
                            reason,
                        }
                    }
                    (_, Err(reason)) => {
                        r.status = SeamStatus::NotComputable {
                            side: SeamWhich::B,
                            reason,
                        }
                    }
                    (Ok(a), Ok(b)) => {
                        let measured = a - b;
                        let deviation = (measured - expected).abs();
                        r.gap_mm = Some((a - b).abs());
                        r.embu_measured_mm = Some(measured);
                        r.deviation_mm = Some(deviation);
                        if deviation > tol {
                            r.status = SeamStatus::OutOfTolerance;
                        }
                    }
                }
                r
            })
            .collect()
    }

    pub(crate) fn add_seam_pair(
        &mut self,
        a: SeamSide,
        b: SeamSide,
        embu_mm: Option<f64>,
        tolerance_mm: Option<f64>,
        out: &mut Outcome,
    ) -> Result<(), CoreError> {
        let id = SeamPairId(self.alloc_id());
        self.seams.push(SeamPair {
            id,
            a,
            b,
            embu_mm,
            tolerance_mm,
        });
        out.seam_pairs.push(id);
        Ok(())
    }

    pub(crate) fn set_seam_pair(
        &mut self,
        id: SeamPairId,
        a: SeamSide,
        b: SeamSide,
        embu_mm: Option<f64>,
        tolerance_mm: Option<f64>,
    ) -> Result<(), CoreError> {
        let s = self
            .seams
            .iter_mut()
            .find(|s| s.id == id)
            .ok_or_else(|| bad("couture appariée inconnue"))?;
        s.a = a;
        s.b = b;
        s.embu_mm = embu_mm;
        s.tolerance_mm = tolerance_mm;
        Ok(())
    }

    pub(crate) fn remove_seam_pair(&mut self, id: SeamPairId) -> Result<(), CoreError> {
        let i = self
            .seams
            .iter()
            .position(|s| s.id == id)
            .ok_or_else(|| bad("couture appariée inconnue"))?;
        self.seams.remove(i);
        Ok(())
    }

    /// Retire, avec une remarque, les couples qui existaient avant la commande et dont une pièce
    /// ou un tracé a disparu du contour (ou ne se suit plus). Un couple introduit par la
    /// commande elle-même reste refusé par la validation. Jamais un refus de la commande mère.
    pub(crate) fn prune_dead_seam_pairs(&mut self, before: &Project, out: &mut Outcome) {
        if self.seams.is_empty() {
            return;
        }
        let dead: Vec<SeamPairId> = self
            .seams
            .iter()
            .filter(|s| {
                before.seams.contains(s)
                    && (s.a.check(self, "").is_err() || s.b.check(self, "").is_err())
            })
            .map(|s| s.id)
            .collect();
        for id in dead {
            let Some(i) = self.seams.iter().position(|s| s.id == id) else {
                continue;
            };
            let s = self.seams.remove(i);
            let name = |side: &SeamSide| {
                before
                    .piece(side.piece)
                    .map(|p| p.name.clone())
                    .unwrap_or_default()
            };
            out.notes.push(format!(
                "La couture appariée entre « {} » et « {} » référençait une pièce ou un tracé \
                 qui a disparu du contour : elle est retirée.",
                name(&s.a),
                name(&s.b)
            ));
        }
    }
}
