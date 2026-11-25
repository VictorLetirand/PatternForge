//! Contrôle des trous de piqûre entre pièces (US-08.6, maroquinerie) : deux lignes de piqûre
//! (US-06.9) **déclarées appariées** (`Project::stitch_pairs`) et leur lecture.
//!
//! La déclaration est la seule chose stockée. Tout le reste est **lu** à chaque appel et n'est
//! jamais une contrainte (mesure observée, comme US-08.1 et 08.4) : nombre de trous de chaque
//! ligne, pas moyen et écart de pas, vis-à-vis de chaque trou (même abscisse curviligne
//! proportionnelle, sens de couture déduit du sens des contours : code partagé avec la marche de
//! pièces, `march_target`), trous en trop ou manquants avec leur position.
//! La **tolérance n'est pas stockée** : paramètre de [`Project::stitch_check_report_with`].
//!
//! Le **report** ([`Command::CarryStitchLine`](crate::Command::CarryStitchLine)) crée en une étape
//! la ligne de la pièce voisine (même forme, même pas, même retrait, **même nombre de trous**) sur
//! le bord choisi, puis la déclare appariée à la ligne source : la fraction d'abscisse de chaque
//! trou est conservée (donc l'abscisse elle-même quand les deux bords ont la même longueur).
//! Les comparaisons se font par abscisse sur chaque ligne : elles ne dépendent ni de la rotation
//! ni du décalage de la pièce dans le plan (repère de chaque pièce).
//!
//! Conventions d'atelier (non validées) :
//! - tolérance de vis-à-vis : **0,5 mm** par défaut ([`STITCH_CHECK_TOLERANCE_MM`]), limite incluse ;
//! - l'écart d'un couple de trous est le **plus grand** des deux écarts mesurés sur chaque ligne ;
//! - appariement **un-pour-un** glouton par écart croissant ; au-delà de la tolérance et jusqu'à la
//!   moitié du plus petit pas moyen (ou la tolérance si elle est plus grande) : *décalé* ; sinon
//!   *manquant* ;
//! - numérotation appariée : le trou k de A et son vis-à-vis de B portent le même numéro k ; les
//!   trous de B sans vis-à-vis suivent (n + 1, …) ;
//! - pas moyen = (abscisse du dernier trou − abscisse du premier) ÷ (nombre de trous − 1) ; écart
//!   de pas = pas moyen de A − pas moyen de B.

use crate::command::Outcome;
use crate::error::CoreError;
use crate::model::*;
use crate::seam_march::{march_target, sewn_opposite};
use crate::seam_pair::{SeamSide, SeamWhich};
use crate::stitch_line::{StitchLayout, StitchLine, StitchLineSpec, StitchPlaced};
use serde::{Deserialize, Serialize};

/// Tolérance de vis-à-vis par défaut (mm).
pub const STITCH_CHECK_TOLERANCE_MM: f64 = 0.5;
/// Nombre maximal de couples de lignes de piqûre.
pub const STITCH_MAX_PAIRS: usize = 200;

/// Une ligne de piqûre d'une pièce.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StitchRef {
    pub piece: PieceId,
    pub line: StitchLineId,
}

/// Déclaration « ces deux lignes de piqûre se cousent ensemble » (A est la référence).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StitchPair {
    pub id: StitchPairId,
    pub a: StitchRef,
    pub b: StitchRef,
}

/// Verdict d'un trou.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StitchMatch {
    /// Vis-à-vis à moins de la tolérance.
    Matched,
    /// Vis-à-vis trouvé, plus loin que la tolérance.
    Shifted,
    /// Ce côté a un trou dont l'**autre** côté n'a pas le vis-à-vis : le côté indiqué est celui
    /// qui **manque** (B manquant = trou de A sans vis-à-vis ; A manquant = trou en trop sur B).
    Missing(SeamWhich),
}

/// Un numéro de trou apparié, avec ses deux trous.
#[derive(Debug, Clone, PartialEq)]
pub struct StitchCheckItem {
    /// Numéro apparié (celui de A ; les trous de B sans vis-à-vis suivent).
    pub number: u32,
    pub a: Option<StitchPlaced>,
    pub b: Option<StitchPlaced>,
    /// Écart (mm, sur la ligne qui reçoit le vis-à-vis attendu) ; seulement si apparié.
    pub gap_mm: Option<f64>,
    pub status: StitchMatch,
}

/// Raison d'un couple non calculable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StitchIssue {
    /// Pièce ou ligne disparue (impossible dans un projet valide).
    MissingLine,
    /// Calcul des trous impossible (bord dégénéré, trou hors de la pièce…).
    Degenerate,
}

impl StitchIssue {
    /// Identifiant stable.
    pub fn key(self) -> &'static str {
        match self {
            StitchIssue::MissingLine => "missing_line",
            StitchIssue::Degenerate => "degenerate",
        }
    }
}

/// Lecture d'un couple de lignes.
#[derive(Debug, Clone, PartialEq)]
pub struct StitchCheckReading {
    pub id: StitchPairId,
    /// Rang du couple (à partir de 1).
    pub number: usize,
    pub a: StitchRef,
    pub b: StitchRef,
    pub tolerance_mm: f64,
    pub not_computable: Option<(SeamWhich, StitchIssue)>,
    pub count_a: usize,
    pub count_b: usize,
    /// Pas moyen de chaque ligne (mm) et écart A − B ; 0 pour une ligne d'un seul trou.
    pub mean_step_a: f64,
    pub mean_step_b: f64,
    pub step_gap_mm: f64,
    pub items: Vec<StitchCheckItem>,
}

impl StitchCheckReading {
    /// Même nombre de trous des deux côtés.
    pub fn counts_equal(&self) -> bool {
        self.count_a == self.count_b
    }
    pub fn count_status(&self, wanted: StitchMatch) -> usize {
        self.items.iter().filter(|i| i.status == wanted).count()
    }
    /// Trous sans vis-à-vis (manquants ou en trop), des deux côtés.
    pub fn unmatched(&self) -> impl Iterator<Item = &StitchCheckItem> {
        self.items
            .iter()
            .filter(|i| matches!(i.status, StitchMatch::Missing(_)))
    }
}

fn bad(msg: impl Into<String>) -> CoreError {
    CoreError::Invalid(msg.into())
}

fn mean_step(l: &StitchLayout) -> f64 {
    match (l.holes.first(), l.holes.last()) {
        (Some(a), Some(b)) if l.holes.len() > 1 => {
            (b.abscissa_mm - a.abscissa_mm) / (l.holes.len() - 1) as f64
        }
        _ => 0.0,
    }
}

/// Écart absolu, circulaire quand la ligne est un contour entier fermé.
fn gap(d: f64, total: f64, closed: bool) -> f64 {
    let d = d.abs();
    if closed {
        d.min((total - d).max(0.0))
    } else {
        d
    }
}

impl Project {
    fn stitch_pair_side(
        &self,
        r: StitchRef,
    ) -> Result<(StitchLayout, crate::seam_march::Frame), CoreError> {
        let line = self
            .piece(r.piece)?
            .stitch_lines
            .iter()
            .find(|l| l.id == r.line)
            .ok_or_else(|| bad("ligne de piqûre absente"))?;
        let layout = self.stitch_line_layout(r.piece, r.line)?;
        let side = SeamSide {
            piece: r.piece,
            edges: line.edges.clone(),
        };
        let frame = self.march_frame(&side, "ligne de piqûre")?;
        Ok((layout, frame))
    }

    fn stitch_check_items(
        &self,
        p: &StitchPair,
        tol: f64,
    ) -> Result<(StitchLayout, StitchLayout, Vec<StitchCheckItem>), (SeamWhich, StitchIssue)> {
        let (la, fa) = self
            .stitch_pair_side(p.a)
            .map_err(|_| (SeamWhich::A, StitchIssue::Degenerate))?;
        let (lb, fb) = self
            .stitch_pair_side(p.b)
            .map_err(|_| (SeamWhich::B, StitchIssue::Degenerate))?;
        let opposite = sewn_opposite(&fa, &fb);
        let (ta, tb) = (la.length_mm, lb.length_mm);
        let (ca, cb) = (la.closed, lb.closed);
        let ea: Vec<f64> = la
            .holes
            .iter()
            .map(|h| march_target(h.abscissa_mm, ta, tb, opposite).1)
            .collect();
        let eb: Vec<f64> = lb
            .holes
            .iter()
            .map(|h| march_target(h.abscissa_mm, tb, ta, opposite).1)
            .collect();
        let range = tol.max(0.5 * mean_step(&la).min(mean_step(&lb)));
        let mut cands: Vec<(f64, usize, usize)> = Vec::new();
        for (i, a) in la.holes.iter().enumerate() {
            for (j, b) in lb.holes.iter().enumerate() {
                let cost =
                    gap(ea[i] - b.abscissa_mm, tb, cb).max(gap(eb[j] - a.abscissa_mm, ta, ca));
                if cost <= range {
                    cands.push((cost, i, j));
                }
            }
        }
        cands.sort_by(|x, y| x.0.total_cmp(&y.0).then(x.1.cmp(&y.1)).then(x.2.cmp(&y.2)));
        let mut ma: Vec<Option<(usize, f64)>> = vec![None; la.holes.len()];
        let mut mb: Vec<bool> = vec![false; lb.holes.len()];
        for (cost, i, j) in cands {
            if ma[i].is_none() && !mb[j] {
                ma[i] = Some((j, cost));
                mb[j] = true;
            }
        }
        let mut items = Vec::new();
        for (i, a) in la.holes.iter().enumerate() {
            match ma[i] {
                Some((j, cost)) => items.push(StitchCheckItem {
                    number: a.number,
                    a: Some(*a),
                    b: Some(lb.holes[j]),
                    gap_mm: Some(cost),
                    status: if cost <= tol {
                        StitchMatch::Matched
                    } else {
                        StitchMatch::Shifted
                    },
                }),
                None => items.push(StitchCheckItem {
                    number: a.number,
                    a: Some(*a),
                    b: None,
                    gap_mm: None,
                    status: StitchMatch::Missing(SeamWhich::B),
                }),
            }
        }
        let mut next = la.holes.len() as u32;
        for (j, b) in lb.holes.iter().enumerate() {
            if !mb[j] {
                next += 1;
                items.push(StitchCheckItem {
                    number: next,
                    a: None,
                    b: Some(*b),
                    gap_mm: None,
                    status: StitchMatch::Missing(SeamWhich::A),
                });
            }
        }
        Ok((la, lb, items))
    }

    /// Contrôle des trous de chaque couple, avec la tolérance par défaut.
    pub fn stitch_check_report(&self) -> Vec<StitchCheckReading> {
        self.stitch_check_report_with(STITCH_CHECK_TOLERANCE_MM)
    }

    /// Idem avec une tolérance explicite (mm, ≥ 0 ; sinon la valeur par défaut).
    pub fn stitch_check_report_with(&self, tolerance_mm: f64) -> Vec<StitchCheckReading> {
        let tol = if tolerance_mm.is_finite() && tolerance_mm >= 0.0 {
            tolerance_mm
        } else {
            STITCH_CHECK_TOLERANCE_MM
        };
        self.stitch_pairs
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let mut out = StitchCheckReading {
                    id: p.id,
                    number: i + 1,
                    a: p.a,
                    b: p.b,
                    tolerance_mm: tol,
                    not_computable: None,
                    count_a: 0,
                    count_b: 0,
                    mean_step_a: 0.0,
                    mean_step_b: 0.0,
                    step_gap_mm: 0.0,
                    items: Vec::new(),
                };
                match self.stitch_check_items(p, tol) {
                    Ok((la, lb, items)) => {
                        out.count_a = la.holes.len();
                        out.count_b = lb.holes.len();
                        out.mean_step_a = mean_step(&la);
                        out.mean_step_b = mean_step(&lb);
                        out.step_gap_mm = out.mean_step_a - out.mean_step_b;
                        out.items = items;
                    }
                    Err(e) => out.not_computable = Some(e),
                }
                out
            })
            .collect()
    }

    /// Couple (et rang à partir de 1) qui contient cette ligne.
    pub fn stitch_pair_of(
        &self,
        piece: PieceId,
        line: StitchLineId,
    ) -> Option<(usize, &StitchPair)> {
        self.stitch_pairs
            .iter()
            .enumerate()
            .find(|(_, p)| {
                (p.a.piece == piece && p.a.line == line) || (p.b.piece == piece && p.b.line == line)
            })
            .map(|(i, p)| (i + 1, p))
    }

    /// Numéro apparié de chaque trou d'une ligne qui est dans un couple (lecture pure), dans
    /// l'ordre des trous de la ligne. `None` : ligne hors couple ou couple non calculable.
    pub fn stitch_paired_numbers(&self, piece: PieceId, line: StitchLineId) -> Option<Vec<u32>> {
        self.stitch_all_paired_numbers().remove(&(piece, line))
    }

    /// Numéros appariés de toutes les lignes qui sont dans un couple calculable (un seul calcul de
    /// chaque couple ; sert à l'affichage).
    pub fn stitch_all_paired_numbers(
        &self,
    ) -> std::collections::BTreeMap<(PieceId, StitchLineId), Vec<u32>> {
        let mut out = std::collections::BTreeMap::new();
        for reading in self.stitch_check_report() {
            if reading.not_computable.is_some() {
                continue;
            }
            for (side, count) in [(reading.a, reading.count_a), (reading.b, reading.count_b)] {
                let is_a = side == reading.a;
                let mut numbers = vec![0u32; count];
                for it in &reading.items {
                    let h = if is_a { it.a } else { it.b };
                    if let Some(h) = h {
                        if let Some(slot) = numbers.get_mut(h.number as usize - 1) {
                            *slot = it.number;
                        }
                    }
                }
                out.insert((side.piece, side.line), numbers);
            }
        }
        out
    }

    fn check_pair_refs(&self, a: StitchRef, b: StitchRef) -> Result<(), CoreError> {
        if a == b {
            return Err(bad("il faut deux lignes de piqûre différentes"));
        }
        for (r, which) in [(a, "A"), (b, "B")] {
            let p = self.piece(r.piece)?;
            if !p.stitch_lines.iter().any(|l| l.id == r.line) {
                return Err(bad(format!(
                    "la ligne de piqûre {} (côté {which}) n'existe pas sur la pièce « {} »",
                    r.line.0, p.name
                )));
            }
            if self.stitch_pair_of(r.piece, r.line).is_some() {
                return Err(bad(format!(
                    "la ligne de piqûre {} (côté {which}) est déjà dans un couple",
                    r.line.0
                )));
            }
        }
        Ok(())
    }

    pub(crate) fn pair_stitch_lines(
        &mut self,
        a: StitchRef,
        b: StitchRef,
        out: &mut Outcome,
    ) -> Result<(), CoreError> {
        if self.stitch_pairs.len() >= STITCH_MAX_PAIRS {
            return Err(bad(format!(
                "trop de couples de lignes de piqûre (maximum {STITCH_MAX_PAIRS})"
            )));
        }
        self.check_pair_refs(a, b)?;
        let id = StitchPairId(self.alloc_id());
        self.stitch_pairs.push(StitchPair { id, a, b });
        out.stitch_pairs.push(id);
        Ok(())
    }

    pub(crate) fn unpair_stitch_lines(&mut self, pair: StitchPairId) -> Result<(), CoreError> {
        let before = self.stitch_pairs.len();
        self.stitch_pairs.retain(|p| p.id != pair);
        if self.stitch_pairs.len() == before {
            return Err(bad("couple de lignes de piqûre inconnu"));
        }
        Ok(())
    }

    /// Report : crée la ligne de `to_piece` sur `to_edges` (même forme, pas, retrait et nombre de
    /// trous que la source) et la déclare appariée à la source (A = source).
    pub(crate) fn carry_stitch_line(
        &mut self,
        from: StitchRef,
        to_piece: PieceId,
        to_edges: Vec<EntityId>,
        out: &mut Outcome,
    ) -> Result<(), CoreError> {
        let src: StitchLine = self
            .piece(from.piece)?
            .stitch_lines
            .iter()
            .find(|l| l.id == from.line)
            .ok_or_else(|| bad("ligne de piqûre source inconnue"))?
            .clone();
        if self.stitch_pair_of(from.piece, from.line).is_some() {
            return Err(bad("la ligne source est déjà dans un couple"));
        }
        if to_piece == from.piece {
            return Err(bad("le report se fait sur une autre pièce"));
        }
        let count = self.stitch_line_layout(from.piece, from.line)?.holes.len();
        let spec = StitchLineSpec {
            edges: to_edges,
            hole: src.hole,
            pitch_mm: src.pitch_mm,
            setback_mm: src.setback_mm,
            count: u32::try_from(count).ok(),
            continues: None,
        };
        self.add_stitch_line(to_piece, spec, out)?;
        let line = *out
            .stitch_lines
            .last()
            .ok_or_else(|| bad("report impossible"))?;
        self.pair_stitch_lines(
            from,
            StitchRef {
                piece: to_piece,
                line,
            },
            out,
        )
    }

    /// Les couples dont une ligne ou une pièce a disparu sont retirés, avec remarque.
    pub(crate) fn prune_dead_stitch_pairs(&mut self, out: &mut Outcome) {
        if self.stitch_pairs.is_empty() {
            return;
        }
        let alive = |p: &Project, r: StitchRef| {
            p.pieces
                .iter()
                .any(|x| x.id == r.piece && x.stitch_lines.iter().any(|l| l.id == r.line))
        };
        let dead: Vec<StitchPairId> = self
            .stitch_pairs
            .iter()
            .filter(|p| !alive(self, p.a) || !alive(self, p.b))
            .map(|p| p.id)
            .collect();
        for id in dead {
            self.stitch_pairs.retain(|p| p.id != id);
            out.notes.push(
                "Un couple de lignes de piqûre dont une ligne a disparu est retiré.".to_string(),
            );
        }
    }

    /// Contrôle des couples (ouverture et après chaque commande) : références, une ligne dans un
    /// seul couple.
    pub(crate) fn validate_stitch_pairs(&self) -> Result<(), CoreError> {
        if self.stitch_pairs.len() > STITCH_MAX_PAIRS {
            return Err(bad(format!(
                "trop de couples de lignes de piqûre (maximum {STITCH_MAX_PAIRS})"
            )));
        }
        let mut used: Vec<StitchRef> = Vec::new();
        for (i, p) in self.stitch_pairs.iter().enumerate() {
            let n = i + 1;
            if p.a == p.b {
                return Err(bad(format!(
                    "couple de lignes de piqûre {n} : les deux côtés sont la même ligne"
                )));
            }
            for r in [p.a, p.b] {
                let ok = self
                    .piece(r.piece)
                    .is_ok_and(|x| x.stitch_lines.iter().any(|l| l.id == r.line));
                if !ok {
                    return Err(CoreError::DeadReference(format!(
                        "couple de lignes de piqûre {n} : une ligne n'existe pas"
                    )));
                }
                if used.contains(&r) {
                    return Err(bad(format!(
                        "couple de lignes de piqûre {n} : une ligne n'appartient qu'à un couple"
                    )));
                }
                used.push(r);
            }
        }
        Ok(())
    }
}
