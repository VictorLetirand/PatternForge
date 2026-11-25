//! Lignes de piqûre (US-06.9, maroquinerie) : série de trous de piqûre **liée** à un bord.
//!
//! Une ligne ([`StitchLine`]) mémorise sa **définition** : les tracés consécutifs du contour de
//! la pièce qui la portent, la forme du trou, le pas demandé, le retrait du bord (vers
//! l'intérieur), un nombre de trous fixé ou automatique, et une éventuelle ligne qu'elle
//! **continue**. Les **positions ne sont jamais stockées** : elles sont recalculées à chaque
//! lecture d'après la construction, donc elles suivent le bord dans la même étape annulable que
//! la modification qui le change (aucune synchronisation à rejouer).
//!
//! Trajet des trous : le bord décalé de `setback_mm` vers l'intérieur de la pièce
//! (`pf_geometry::offset_chain`, coins en pointe), les trous étant répartis par **abscisse
//! curviligne sur ce trajet** : le pas est donc continu autour d'un angle. `⌈L / pas⌉` intervalles
//! égaux en automatique (premier trou au début, dernier à la fin ; contour entier fermé : `L / n`
//! sans doublon), `n` trous fixés sinon (un seul : au milieu). Un trou dont le centre sort de la
//! pièce refuse la commande (rien n'est écrit). Tout est en mm.

use crate::command::Outcome;
use crate::error::CoreError;
use crate::model::*;
use crate::offset::trace_of;
use crate::seam_pair::SeamSide;
use pf_geometry::{
    offset_chain, CircleTrace, OffsetOptions, PointMm, Trace, VectorMm, DEFAULT_LENGTH_TOLERANCE_MM,
};
use serde::{Deserialize, Serialize};

/// Plus petit pas demandé (mm).
pub const STITCH_MIN_PITCH_MM: f64 = 0.5;
/// Plus grand pas demandé (mm).
pub const STITCH_MAX_PITCH_MM: f64 = 100.0;
/// Plus petit retrait du bord (mm) : en dessous, le centre d'un trou serait dans l'erreur
/// d'échantillonnage du contour (test « dans la pièce »).
pub const STITCH_MIN_SETBACK_MM: f64 = 0.5;
/// Plus grand retrait du bord (mm).
pub const STITCH_MAX_SETBACK_MM: f64 = 100.0;
/// Plus petite dimension d'un trou (mm).
pub const STITCH_MIN_HOLE_MM: f64 = 0.1;
/// Plus grande dimension d'un trou (mm).
pub const STITCH_MAX_HOLE_MM: f64 = 20.0;
/// Nombre maximal de trous d'une ligne.
pub const STITCH_MAX_HOLES: usize = 500;
/// Nombre maximal de lignes par pièce.
pub const STITCH_MAX_LINES: usize = 50;
/// Plus petit espacement réel entre deux trous (mm).
pub const STITCH_MIN_SPACING_MM: f64 = 0.01;
/// Écart maximal toléré entre deux morceaux du trajet décalé (mm).
const GAP_MM: f64 = 1e-4;

/// Forme du trou de piqûre (poinçon). `length_mm` est mesurée le long de la tangente du bord,
/// `width_mm` à travers.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum StitchHole {
    /// Poinçon rond.
    Round { diameter_mm: f64 },
    /// Poinçon plat : rectangle `length_mm` × `width_mm`.
    Flat { length_mm: f64, width_mm: f64 },
    /// Losange « français » : diagonales `length_mm` (tangente) et `width_mm` (normale).
    French { length_mm: f64, width_mm: f64 },
    /// Oblique « japonais » : rectangle `length_mm` × `width_mm` incliné de `slant_deg` sur la
    /// tangente (anti-horaire, dans [0 ; 180[).
    Japanese {
        length_mm: f64,
        width_mm: f64,
        slant_deg: f64,
    },
}

impl StitchHole {
    /// Identifiant stable, indépendant de la langue.
    pub fn key(self) -> &'static str {
        match self {
            StitchHole::Round { .. } => "round",
            StitchHole::Flat { .. } => "flat",
            StitchHole::French { .. } => "french",
            StitchHole::Japanese { .. } => "japanese",
        }
    }

    /// Dimensions finies et dans les bornes, sinon refus expliqué.
    pub fn validate(self) -> Result<(), CoreError> {
        let dims: &[f64] = match &self {
            StitchHole::Round { diameter_mm } => &[*diameter_mm][..],
            StitchHole::Flat {
                length_mm,
                width_mm,
            }
            | StitchHole::French {
                length_mm,
                width_mm,
            }
            | StitchHole::Japanese {
                length_mm,
                width_mm,
                ..
            } => &[*length_mm, *width_mm][..],
        };
        if dims
            .iter()
            .any(|d| !d.is_finite() || !(STITCH_MIN_HOLE_MM..=STITCH_MAX_HOLE_MM).contains(d))
        {
            return Err(bad(format!(
                "les dimensions d'un trou de piqûre doivent être des nombres finis compris entre \
                 {STITCH_MIN_HOLE_MM} et {STITCH_MAX_HOLE_MM} mm"
            )));
        }
        if let StitchHole::Japanese { slant_deg, .. } = self {
            if !slant_deg.is_finite() || !(0.0..180.0).contains(&slant_deg) {
                return Err(bad(
                    "l'inclinaison d'un trou oblique doit être dans [0 ; 180[",
                ));
            }
        }
        Ok(())
    }

    /// Contour fermé (quatre sommets, anti-horaire) du trou centré en `center` quand la tangente
    /// du bord fait `tangent_deg` ; `None` pour un trou rond (cercle).
    pub fn polygon(self, center: PointMm, tangent_deg: f64) -> Option<[PointMm; 4]> {
        let local: [(f64, f64); 4] = match self {
            StitchHole::Round { .. } => return None,
            StitchHole::Flat {
                length_mm: l,
                width_mm: w,
            } => [
                (l / 2.0, -w / 2.0),
                (l / 2.0, w / 2.0),
                (-l / 2.0, w / 2.0),
                (-l / 2.0, -w / 2.0),
            ],
            StitchHole::French {
                length_mm: l,
                width_mm: w,
            } => [
                (l / 2.0, 0.0),
                (0.0, w / 2.0),
                (-l / 2.0, 0.0),
                (0.0, -w / 2.0),
            ],
            StitchHole::Japanese {
                length_mm: l,
                width_mm: w,
                slant_deg,
            } => {
                let (s, c) = slant_deg.to_radians().sin_cos();
                [
                    (l / 2.0, -w / 2.0),
                    (l / 2.0, w / 2.0),
                    (-l / 2.0, w / 2.0),
                    (-l / 2.0, -w / 2.0),
                ]
                .map(|(x, y)| (x * c - y * s, x * s + y * c))
            }
        };
        let (s, c) = tangent_deg.to_radians().sin_cos();
        Some(local.map(|(x, y)| PointMm::new(center.x + x * c - y * s, center.y + x * s + y * c)))
    }

    /// Demi-emprise (mm) du trou autour de son centre.
    pub fn half_extent_mm(self) -> f64 {
        match self {
            StitchHole::Round { diameter_mm } => diameter_mm / 2.0,
            StitchHole::Flat {
                length_mm,
                width_mm,
            }
            | StitchHole::French {
                length_mm,
                width_mm,
            }
            | StitchHole::Japanese {
                length_mm,
                width_mm,
                ..
            } => length_mm.hypot(width_mm) / 2.0,
        }
    }
}

/// Paramètres d'une ligne de piqûre (création et remplacement).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StitchLineSpec {
    /// Tracés **consécutifs** du contour de la pièce, dans l'ordre du contour (un seul = un
    /// bord ; tous = le contour entier). L'abscisse 0 est le début du premier.
    pub edges: Vec<EntityId>,
    pub hole: StitchHole,
    /// Pas demandé (mm) : intervalles égaux, l'espacement réel est inférieur ou égal au pas.
    pub pitch_mm: f64,
    /// Retrait du bord, vers l'intérieur de la pièce (mm).
    pub setback_mm: f64,
    /// Nombre de trous **fixé** ; absent = automatique d'après le pas.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<u32>,
    /// Ligne **continuée** : le pas se poursuit depuis son dernier trou (voir [`StitchLayout`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub continues: Option<StitchLineId>,
}

/// Ligne de piqûre stockée dans [`Piece::stitch_lines`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StitchLine {
    pub id: StitchLineId,
    pub edges: Vec<EntityId>,
    pub hole: StitchHole,
    pub pitch_mm: f64,
    pub setback_mm: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub continues: Option<StitchLineId>,
}

impl StitchLine {
    fn from_spec(id: StitchLineId, s: StitchLineSpec) -> StitchLine {
        StitchLine {
            id,
            edges: s.edges,
            hole: s.hole,
            pitch_mm: s.pitch_mm,
            setback_mm: s.setback_mm,
            count: s.count,
            continues: s.continues,
        }
    }

    /// Paramètres de la ligne (sans identifiant).
    pub fn spec(&self) -> StitchLineSpec {
        StitchLineSpec {
            edges: self.edges.clone(),
            hole: self.hole,
            pitch_mm: self.pitch_mm,
            setback_mm: self.setback_mm,
            count: self.count,
            continues: self.continues,
        }
    }
}

/// Un trou calculé.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StitchPlaced {
    /// Numéro (1, 2, … dans le sens du bord).
    pub number: u32,
    /// Abscisse curviligne sur le trajet décalé, depuis son début (mm).
    pub abscissa_mm: f64,
    pub position: PointMm,
    /// Direction du bord en ce point (sens du contour), degrés dans [0 ; 360[.
    pub tangent_deg: f64,
}

/// Ligne calculée (lecture ; rien n'est stocké).
#[derive(Debug, Clone, PartialEq)]
pub struct StitchLayout {
    /// Longueur du trajet décalé (mm).
    pub length_mm: f64,
    /// Abscisse du premier trou : 0, ou la part du pas restant d'une ligne **continuée**.
    pub start_mm: f64,
    /// Espacement **réel** entre deux trous consécutifs (mm) ; 0 pour un seul trou.
    pub step_mm: f64,
    /// Contour entier fermé (le dernier trou n'est pas sur le premier).
    pub closed: bool,
    pub holes: Vec<StitchPlaced>,
}

/// Une ligne et ses trous (lecture pour l'export et l'affichage).
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedStitchLine {
    pub id: StitchLineId,
    pub hole: StitchHole,
    pub layout: StitchLayout,
}

fn bad(msg: impl Into<String>) -> CoreError {
    CoreError::Invalid(msg.into())
}

/// Nombre d'intervalles pour couvrir `span` avec des pas d'au plus `step` : ⌈span / step⌉, sans
/// qu'une erreur d'arrondi (0,3 / 0,1) ajoute un intervalle.
fn intervals(span: f64, step: f64) -> f64 {
    let q = span / step;
    let r = q.round();
    if (q - r).abs() <= 1e-9 * q.abs().max(1.0) {
        r
    } else {
        q.ceil()
    }
}

pub(crate) fn trace_len(t: &Trace) -> Result<f64, CoreError> {
    Ok(match t {
        Trace::Line(a, b) => pf_geometry::segment_length_mm(*a, *b)?,
        Trace::Cubic(c) => c.length_mm(DEFAULT_LENGTH_TOLERANCE_MM)?,
        Trace::Arc(a) => a.length_mm(),
        Trace::Spline(s) => s.length_mm()?,
        Trace::Circle(c) => c.circumference_mm(),
    })
}

pub(crate) fn trace_at(t: &Trace, d: f64) -> Result<(PointMm, VectorMm), CoreError> {
    let len = trace_len(t)?;
    let d = d.clamp(0.0, len);
    Ok(match t {
        Trace::Line(a, b) => (pf_geometry::point_on_segment(*a, *b, d)?, (*b - *a).unit()?),
        Trace::Cubic(c) => {
            let u = c.t_at_length_mm(d, DEFAULT_LENGTH_TOLERANCE_MM)?;
            (c.point_at(u), c.unit_tangent_at(u)?)
        }
        Trace::Arc(a) => a.point_at_length(d)?,
        Trace::Spline(s) => s.point_at_length(d)?,
        Trace::Circle(c) => {
            CircleTrace::new(c.center, c.radius_mm, 0.0, true)?.point_at_length(d)?
        }
    })
}

fn angle_deg(v: VectorMm) -> f64 {
    let mut deg = v.y.atan2(v.x).to_degrees();
    if deg < 0.0 {
        deg += 360.0;
    }
    if !(0.0..360.0).contains(&deg) {
        deg = 0.0;
    }
    deg
}

impl StitchLineSpec {
    /// Contrôles qui ne dépendent pas de la géométrie.
    pub fn check_values(&self) -> Result<(), CoreError> {
        self.hole.validate()?;
        if !self.pitch_mm.is_finite()
            || !(STITCH_MIN_PITCH_MM..=STITCH_MAX_PITCH_MM).contains(&self.pitch_mm)
        {
            return Err(bad(format!(
                "le pas doit être un nombre fini compris entre {STITCH_MIN_PITCH_MM} et \
                 {STITCH_MAX_PITCH_MM} mm"
            )));
        }
        if !self.setback_mm.is_finite()
            || !(STITCH_MIN_SETBACK_MM..=STITCH_MAX_SETBACK_MM).contains(&self.setback_mm)
        {
            return Err(bad(format!(
                "le retrait du bord doit être un nombre fini compris entre {STITCH_MIN_SETBACK_MM} \
                 et {STITCH_MAX_SETBACK_MM} mm"
            )));
        }
        if let Some(n) = self.count {
            if !(1..=STITCH_MAX_HOLES as u32).contains(&n) {
                return Err(bad(format!(
                    "le nombre de trous doit être compris entre 1 et {STITCH_MAX_HOLES}"
                )));
            }
        }
        Ok(())
    }
}

impl Project {
    fn stitch_line_ref(
        &self,
        piece: PieceId,
        line: StitchLineId,
    ) -> Result<&StitchLine, CoreError> {
        let p = self.piece(piece)?;
        p.stitch_lines.iter().find(|l| l.id == line).ok_or_else(|| {
            bad(format!(
                "la ligne de piqûre {} n'existe pas sur la pièce « {} »",
                line.0, p.name
            ))
        })
    }

    /// Trajet décalé de la ligne : tracés, fermé ou non, longueur totale.
    fn stitch_path(
        &self,
        piece: PieceId,
        spec: &StitchLineSpec,
    ) -> Result<(Vec<Trace>, bool, f64), CoreError> {
        let what = "ligne de piqûre : bord";
        let side = SeamSide {
            piece,
            edges: spec.edges.clone(),
        };
        side.check(self, what)?;
        let frame = self.march_frame(&side, what)?;
        let sheet = self.sheet(frame.sheet)?;
        let mut chain = Vec::with_capacity(frame.edges.len());
        for &(entity, rev, _, _) in &frame.edges {
            chain.push(trace_of(sheet.shape(entity)?, rev));
        }
        // L'intérieur est à gauche d'un contour anti-horaire ; un cercle seul se décale toujours
        // vers son centre.
        let lone_circle = matches!(chain.as_slice(), [Trace::Circle(_)]);
        let d = if frame.clockwise && !lone_circle {
            -spec.setback_mm
        } else {
            spec.setback_mm
        };
        let traces = offset_chain(&chain, d, &OffsetOptions::default()).map_err(|e| {
            bad(format!(
                "le retrait ne donne pas de trajet exploitable ({e})"
            ))
        })?;
        if traces.is_empty() {
            return Err(bad("le retrait ne donne pas de trajet exploitable"));
        }
        let mut total = 0.0;
        let mut ends: Vec<(PointMm, PointMm)> = Vec::new();
        for t in &traces {
            let len = trace_len(t)?;
            total += len;
            if !matches!(t, Trace::Circle(_)) {
                ends.push((trace_at(t, 0.0)?.0, trace_at(t, len)?.0));
            }
        }
        for w in ends.windows(2) {
            if pf_geometry::distance_mm(w[0].1, w[1].0) > GAP_MM {
                return Err(bad(
                    "le retrait sépare le trajet en plusieurs morceaux (forme trop étroite pour \
                     ce retrait)",
                ));
            }
        }
        if !total.is_finite() || total <= STITCH_MIN_SPACING_MM {
            return Err(bad("le trajet décalé est trop court"));
        }
        Ok((traces, frame.whole, total))
    }

    fn stitch_layout_spec(
        &self,
        piece: PieceId,
        spec: &StitchLineSpec,
        depth: usize,
    ) -> Result<StitchLayout, CoreError> {
        spec.check_values()?;
        let (traces, whole, total) = self.stitch_path(piece, spec)?;
        let mut start = 0.0;
        if let Some(prev_id) = spec.continues {
            if depth >= STITCH_MAX_LINES {
                return Err(bad("la continuation est circulaire ou trop longue"));
            }
            let prev = self.stitch_line_ref(piece, prev_id)?;
            let p = self.piece(piece)?;
            let n = p.outline.len();
            let follows = prev
                .edges
                .last()
                .and_then(|last| p.outline.iter().position(|e| e.entity == *last))
                .is_some_and(|i| p.outline[(i + 1) % n].entity == spec.edges[0]);
            if !follows || prev.edges.len() >= n {
                return Err(bad(
                    "une ligne continuée doit commencer sur le tracé qui suit immédiatement la fin \
                     de la ligne précédente (contour non entier)",
                ));
            }
            let before = self.stitch_layout_spec(piece, &prev.spec(), depth + 1)?;
            let last = before
                .holes
                .last()
                .ok_or_else(|| bad("la ligne précédente n'a aucun trou"))?;
            let gap = pf_geometry::distance_mm(last.position, trace_at(&traces[0], 0.0)?.0);
            let step = if before.holes.len() > 1 {
                before.step_mm
            } else {
                prev.pitch_mm
            };
            start = (step - gap).max(0.0);
            if total - start <= STITCH_MIN_SPACING_MM {
                return Err(bad("la ligne précédente ne laisse rien à continuer"));
            }
        }
        let closed = whole && start == 0.0;
        let span = total - start;
        let (count, step) = if closed {
            let n = match spec.count {
                Some(n) => n as f64,
                None => intervals(total, spec.pitch_mm).max(1.0),
            };
            (n as usize, total / n)
        } else {
            match spec.count {
                Some(1) => (1, 0.0),
                Some(n) => (n as usize, span / (n - 1) as f64),
                None => {
                    if span <= STITCH_MIN_SPACING_MM {
                        return Err(bad("le trajet est trop court pour un trou"));
                    }
                    let k = intervals(span, spec.pitch_mm);
                    ((k + 1.0) as usize, span / k)
                }
            }
        };
        if count > STITCH_MAX_HOLES {
            return Err(bad(format!(
                "cette ligne donnerait plus de {STITCH_MAX_HOLES} trous"
            )));
        }
        if count > 1 && step < STITCH_MIN_SPACING_MM {
            return Err(bad(
                "l'espacement des trous est trop petit : ils se confondraient",
            ));
        }
        let cum: Vec<f64> = traces
            .iter()
            .scan(0.0, |acc, t| {
                let start = *acc;
                *acc += trace_len(t).unwrap_or(0.0);
                Some(start)
            })
            .collect();
        let mut holes = Vec::with_capacity(count);
        for i in 0..count {
            let s = if count == 1 && !closed {
                start + span / 2.0
            } else if !closed && i + 1 == count {
                total
            } else {
                start + i as f64 * step
            }
            .clamp(0.0, total);
            let k = (0..traces.len())
                .rev()
                .find(|&k| cum[k] <= s + 1e-12)
                .unwrap_or(0);
            let (position, tangent) = trace_at(&traces[k], s - cum[k])?;
            position.validate()?;
            holes.push(StitchPlaced {
                number: i as u32 + 1,
                abscissa_mm: s,
                position,
                tangent_deg: angle_deg(tangent),
            });
        }
        Ok(StitchLayout {
            length_mm: total,
            start_mm: start,
            step_mm: if count > 1 { step } else { 0.0 },
            closed,
            holes,
        })
    }

    /// Trous d'une ligne (lecture pure ; refus expliqué si le bord ne la porte plus).
    pub fn stitch_line_layout(
        &self,
        piece: PieceId,
        line: StitchLineId,
    ) -> Result<StitchLayout, CoreError> {
        let spec = self.stitch_line_ref(piece, line)?.spec();
        let layout = self.stitch_layout_spec(piece, &spec, 0)?;
        let lasso = self.outline_lasso(piece)?;
        if let Some(h) = layout
            .holes
            .iter()
            .find(|h| !lasso.contains_point(h.position))
        {
            return Err(bad(format!(
                "le trou {} sortirait de la pièce (retrait de {} mm trop grand pour ce bord)",
                h.number, spec.setback_mm
            )));
        }
        Ok(layout)
    }

    /// Lignes de piqûre d'une pièce avec leurs trous (lecture pure) ; une ligne que le bord ne
    /// porte plus est omise (impossible dans un projet valide).
    pub fn piece_stitch_lines(&self, piece: PieceId) -> Vec<ResolvedStitchLine> {
        let Ok(p) = self.piece(piece) else {
            return Vec::new();
        };
        p.stitch_lines
            .iter()
            .filter_map(|l| {
                Some(ResolvedStitchLine {
                    id: l.id,
                    hole: l.hole,
                    layout: self.stitch_line_layout(piece, l.id).ok()?,
                })
            })
            .collect()
    }

    /// Trous de toutes les pièces dont le contour est sur la feuille : centre, forme et tangente,
    /// dans l'ordre des pièces, des lignes puis des trous. Sert à l'export.
    pub fn sheet_stitch_holes(&self, sheet: SheetId) -> Vec<(PointMm, StitchHole, f64)> {
        self.pieces_on(sheet)
            .into_iter()
            .flat_map(|p| {
                self.piece_stitch_lines(p.id)
                    .into_iter()
                    .flat_map(|l| {
                        l.layout
                            .holes
                            .into_iter()
                            .map(move |h| (h.position, l.hole, h.tangent_deg))
                    })
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    /// Aperçu en **lecture seule** d'une ligne qui serait créée : mêmes refus que
    /// [`Command::AddStitchLine`](crate::Command::AddStitchLine) (la commande est rejouée sur une
    /// copie du projet).
    pub fn stitch_line_preview(
        &self,
        piece: PieceId,
        spec: &StitchLineSpec,
    ) -> Result<StitchLayout, CoreError> {
        let mut copy = self.clone();
        let out = copy.apply(crate::Command::AddStitchLine {
            piece,
            line: spec.clone(),
        })?;
        let id = *out
            .stitch_lines
            .first()
            .ok_or_else(|| bad("aperçu impossible"))?;
        copy.stitch_line_layout(piece, id)
    }

    /// Contrôle des lignes d'une pièce (ouverture et après chaque commande) : nombre, valeurs,
    /// références, continuations sans boucle, puis calcul complet (trous dans la pièce).
    pub(crate) fn validate_stitch_lines(&self, piece: &Piece) -> Result<(), CoreError> {
        if piece.stitch_lines.is_empty() {
            return Ok(());
        }
        let name = &piece.name;
        if piece.stitch_lines.len() > STITCH_MAX_LINES {
            return Err(bad(format!(
                "la pièce « {name} » porte plus de {STITCH_MAX_LINES} lignes de piqûre"
            )));
        }
        for (i, l) in piece.stitch_lines.iter().enumerate() {
            if piece.stitch_lines[..i].iter().any(|o| o.id == l.id) {
                return Err(bad(format!(
                    "la pièce « {name} » porte deux fois la ligne de piqûre {}",
                    l.id.0
                )));
            }
            if let Some(c) = l.continues {
                if c == l.id || !piece.stitch_lines.iter().any(|o| o.id == c) {
                    return Err(CoreError::DeadReference(format!(
                        "la ligne de piqûre {} de la pièce « {name} » en continue une qui n'existe \
                         pas",
                        l.id.0
                    )));
                }
            }
        }
        for l in &piece.stitch_lines {
            let ctx = |e: CoreError| {
                bad(format!(
                    "ligne de piqûre {} de la pièce « {name} » : {e}",
                    l.id.0
                ))
            };
            l.spec().check_values().map_err(ctx)?;
            self.stitch_line_layout(piece.id, l.id).map_err(ctx)?;
        }
        Ok(())
    }

    fn stitch_index(&self, piece: PieceId, line: StitchLineId) -> Result<usize, CoreError> {
        self.stitch_line_ref(piece, line)?;
        Ok(self
            .piece(piece)?
            .stitch_lines
            .iter()
            .position(|l| l.id == line)
            .unwrap_or(0))
    }

    pub(crate) fn add_stitch_line(
        &mut self,
        piece: PieceId,
        spec: StitchLineSpec,
        out: &mut Outcome,
    ) -> Result<(), CoreError> {
        spec.check_values()?;
        let p = self.piece(piece)?;
        if p.stitch_lines.len() >= STITCH_MAX_LINES {
            return Err(bad(format!(
                "la pièce « {} » porte déjà {STITCH_MAX_LINES} lignes de piqûre",
                p.name
            )));
        }
        let id = StitchLineId(self.alloc_id());
        self.piece_mut(piece)?
            .stitch_lines
            .push(StitchLine::from_spec(id, spec));
        out.stitch_lines.push(id);
        Ok(())
    }

    pub(crate) fn set_stitch_line(
        &mut self,
        piece: PieceId,
        line: StitchLineId,
        spec: StitchLineSpec,
    ) -> Result<(), CoreError> {
        spec.check_values()?;
        let i = self.stitch_index(piece, line)?;
        self.piece_mut(piece)?.stitch_lines[i] = StitchLine::from_spec(line, spec);
        Ok(())
    }

    pub(crate) fn remove_stitch_line(
        &mut self,
        piece: PieceId,
        line: StitchLineId,
        out: &mut Outcome,
    ) -> Result<(), CoreError> {
        let i = self.stitch_index(piece, line)?;
        let p = self.piece_mut(piece)?;
        p.stitch_lines.remove(i);
        for l in &mut p.stitch_lines {
            if l.continues == Some(line) {
                l.continues = None;
                out.notes.push(format!(
                    "La ligne de piqûre {} ne continue plus une autre ligne (supprimée).",
                    l.id.0
                ));
            }
        }
        Ok(())
    }

    /// Fusionne deux lignes **adjacentes** (même forme, pas, retrait, nombre automatique) en une
    /// seule qui garde l'identifiant de `first` : le pas devient continu autour de l'angle.
    pub(crate) fn merge_stitch_lines(
        &mut self,
        piece: PieceId,
        first: StitchLineId,
        second: StitchLineId,
    ) -> Result<(), CoreError> {
        if first == second {
            return Err(bad("il faut deux lignes différentes à fusionner"));
        }
        let a = self.stitch_line_ref(piece, first)?.clone();
        let b = self.stitch_line_ref(piece, second)?.clone();
        if a.hole != b.hole || a.pitch_mm != b.pitch_mm || a.setback_mm != b.setback_mm {
            return Err(bad(
                "les deux lignes doivent avoir la même forme de trou, le même pas et le même retrait",
            ));
        }
        if a.count.is_some() || b.count.is_some() {
            return Err(bad(
                "une ligne au nombre de trous fixé ne se fusionne pas (passez-la en automatique)",
            ));
        }
        let p = self.piece(piece)?;
        let n = p.outline.len();
        let adjacent = a
            .edges
            .last()
            .and_then(|e| p.outline.iter().position(|o| o.entity == *e))
            .is_some_and(|i| p.outline[(i + 1) % n].entity == b.edges[0]);
        if !adjacent || a.edges.len() + b.edges.len() > n {
            return Err(bad(
                "les deux lignes ne sont pas adjacentes : la seconde doit commencer sur le tracé \
                 qui suit la fin de la première",
            ));
        }
        let p = self.piece_mut(piece)?;
        let i = p
            .stitch_lines
            .iter()
            .position(|l| l.id == first)
            .unwrap_or(0);
        p.stitch_lines[i].edges.extend(b.edges.iter().copied());
        p.stitch_lines.retain(|l| l.id != second);
        for l in &mut p.stitch_lines {
            if l.continues == Some(second) {
                l.continues = Some(first);
            }
        }
        // Une continuation de `first` sur `second` n'a plus de sens.
        for l in &mut p.stitch_lines {
            if l.id == first && l.continues == Some(first) {
                l.continues = None;
            }
        }
        Ok(())
    }

    /// Retire les lignes dont un tracé n'est plus dans le contour (tracé supprimé, coupé ou
    /// contour réécrit), avec remarque. Ne touche que les lignes qui existaient avant la
    /// commande ; les continuations vers une ligne retirée sont rompues.
    pub(crate) fn prune_dead_stitch_lines(&mut self, before: &Project, out: &mut Outcome) {
        if self.pieces.iter().all(|p| p.stitch_lines.is_empty()) {
            return;
        }
        for pi in 0..self.pieces.len() {
            let p = &self.pieces[pi];
            if p.stitch_lines.is_empty() {
                continue;
            }
            let old = before.pieces.iter().find(|o| o.id == p.id);
            let gone: Vec<StitchLineId> = p
                .stitch_lines
                .iter()
                .filter(|l| {
                    old.is_some_and(|o| o.stitch_lines.contains(l))
                        && l.edges
                            .iter()
                            .any(|e| !p.outline.iter().any(|o| o.entity == *e))
                })
                .map(|l| l.id)
                .collect();
            if gone.is_empty() {
                continue;
            }
            let p = &mut self.pieces[pi];
            p.stitch_lines.retain(|l| !gone.contains(&l.id));
            for g in &gone {
                out.notes.push(format!(
                    "La ligne de piqûre {} de la pièce « {} » dépendait d'un bord qui n'est plus \
                     dans le contour : elle est retirée.",
                    g.0, p.name
                ));
            }
            for l in &mut p.stitch_lines {
                if l.continues.is_some_and(|c| gone.contains(&c)) {
                    l.continues = None;
                }
            }
        }
    }
}
