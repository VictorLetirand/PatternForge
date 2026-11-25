//! Plans de coupe, **proposition de placement automatique** (US-15.4). Module pur : aucune E/S,
//! aucune interface, aucun stockage. Le résultat est une [`Command::Batch`] de commandes
//! ordinaires (`RemovePlacement` puis `AddPlacement`) : **une seule étape annulable**, placements
//! modifiables ensuite à la main. Une « proposition » n'est **ni optimale ni garantie minimale**
//! (le problème est NP-difficile : aucune garantie d'optimum n'est donnée).
//!
//! Zone de pose : celle de la pose en rangées ([`row_span`]), donc un seul côté du pli pour une
//! étoffe pliée. La proposition **remplace tous les placements du plan** ; les pièces demandées
//! sont posées exactement `quantity` fois (`mirrored_quantity` fois en miroir, seulement pour une
//! pièce déclarée en paire).
//!
//! Mode [`AutoMode::CompleteMissing`] (tranche 133, décisions 437 à 439) : les placements
//! existants sont **conservés tels quels** (aucune commande sur eux), seules les instances
//! manquantes (quantité demandée moins posées) sont posées dans les espaces libres (niveau 1 :
//! coins libres autour des rectangles englobants ; niveau 2 : glissement des seules nouvelles
//! pièces). Existant invalide ou posé au-delà de la quantité : refus nommé, jamais de réparation.
//! Rien à compléter : [`AutoError::NothingToComplete`] (aucune commande, donc aucun pas d'annulation).
//!
//! Deux niveaux, déterministes (aucun hasard) :
//! 1. **rectangles englobants** : au plus [`MAX_TRIALS`] essais (4 ordres de tri × étagères ou
//!    squelette / bottom-left), rotations permises par le droit-fil (0° ou 180°, et 90° ou 270° si
//!    la rotation est autorisée) ; on garde la plus petite longueur (donc le meilleur rendement) ;
//! 2. **contour réel** : chaque pièce glisse vers le bas puis vers la gauche tant qu'elle ne
//!    touche aucune voisine (distance exacte entre contours, courbes aplaties à
//!    [`FLATTEN_TOLERANCE_MM`], pas de recul `distance - écart mini`, donc jamais de
//!    chevauchement). Le niveau 2 ne fait que descendre ou aller à gauche : il ne peut pas
//!    allonger le métrage, et on retombe sur le niveau 1 si la vérification échoue.
//!
//! Avant de rendre la proposition, elle est appliquée à une **copie** du projet et relue par les
//! diagnostics du lot I : zéro chevauchement, zéro débordement, écart mini respecté, quantités
//! exactes. Sinon : refus, jamais de proposition partielle muette. Le temps est borné
//! (`time_limit_ms`, 10 s par défaut) et l'appel est annulable (`cancel`).

use crate::command::Command;
use crate::cutting_layout::{
    place_polygon, polygon_distance_mm, row_span, PieceShape, RotationCheck,
};
use crate::cutting_plan::{CuttingPlacement, CuttingPlan};
use crate::cutting_yield::plan_yield;
use crate::error::CoreError;
use crate::model::*;
use pf_geometry::PointMm;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// Nombre maximal d'essais du niveau 1.
pub const MAX_TRIALS: usize = 8;
/// Borne de temps par défaut (ms).
pub const DEFAULT_TIME_LIMIT_MS: u64 = 10_000;
/// Nombre maximal de pièces posées par une proposition.
pub const MAX_INSTANCES: usize = 400;
mod hide_auto;

const EPS: f64 = 1e-6;
const MIN_STEP: f64 = 0.01;
const MAX_SLIDE_ITER: usize = 40;
const MAX_PASSES: usize = 6;

/// Une pièce à poser : taille, nombre de copies normales et de copies en miroir.
#[derive(Debug, Clone, PartialEq)]
pub struct AutoItem {
    pub piece: PieceId,
    pub size: Option<SizeId>,
    pub quantity: u32,
    pub mirrored_quantity: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AutoOptions {
    /// `false` : rotation 0° (ou 180°) seulement ; `true` : aussi 90° / 270° si le droit-fil le permet.
    pub allow_rotation: bool,
    pub time_limit_ms: u64,
}

impl Default for AutoOptions {
    fn default() -> Self {
        AutoOptions {
            allow_rotation: true,
            time_limit_ms: DEFAULT_TIME_LIMIT_MS,
        }
    }
}

/// Ce que la proposition fait des placements existants du plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AutoMode {
    /// Tout replacer : les placements actuels sont remplacés (décision 411).
    #[default]
    ReplaceAll,
    /// Compléter : les placements actuels sont conservés tels quels ; seules les instances
    /// manquantes (quantité demandée moins posées) sont posées dans les espaces libres.
    CompleteMissing,
}

/// Défauts relevés dans les placements existants (mode compléter) : refus, jamais de réparation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ExistingProblems {
    pub overlaps: usize,
    pub gaps: usize,
    pub overflows: usize,
    pub rotation_refused: usize,
    pub unreadable: usize,
}

/// Une pièce déjà posée plus souvent que la quantité demandée (mode compléter).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OverQuantity {
    pub piece: PieceId,
    pub size: Option<SizeId>,
    pub mirrored: bool,
    pub placed: usize,
    pub wanted: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub enum UnplaceableReason {
    /// Plus large que la largeur utile dans toutes les rotations permises (mm).
    TooWide {
        width_mm: f64,
        span_mm: f64,
    },
    /// Aucune rotation ne respecte le droit-fil.
    NoRotation,
    Unreadable(String),
    /// Peau (US-15.5) : aucune position du contour réel ne respecte contour, défauts, écart et
    /// qualité.
    NoRoomOnHide,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Unplaceable {
    pub piece: PieceId,
    pub size: Option<SizeId>,
    pub reason: UnplaceableReason,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AutoError {
    NothingToPlace,
    TooMany(usize),
    /// Miroir demandé pour une pièce qui n'est pas en paire.
    MirrorNotAllowed(PieceId),
    Unplaceable(Vec<Unplaceable>),
    Cancelled,
    /// La proposition n'a pas passé la relecture : rien n'est proposé.
    VerificationFailed(String),
    Core(CoreError),
    /// Mode compléter : toutes les quantités demandées sont déjà posées (aucune étape).
    NothingToComplete,
    /// Mode compléter : les placements existants se chevauchent, débordent, etc. : refus.
    ExistingInvalid(ExistingProblems),
    /// Mode compléter : des pièces sont posées plus souvent que demandé : refus.
    ExistingExceeds(Vec<OverQuantity>),
}

impl From<CoreError> for AutoError {
    fn from(e: CoreError) -> Self {
        AutoError::Core(e)
    }
}

impl std::fmt::Display for AutoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AutoError::NothingToPlace => write!(f, "aucune pièce à poser"),
            AutoError::TooMany(n) => {
                write!(f, "trop de pièces à poser ({n}, au plus {MAX_INSTANCES})")
            }
            AutoError::MirrorNotAllowed(_) => write!(f, "miroir demandé pour une pièce hors paire"),
            AutoError::Unplaceable(v) => write!(f, "{} pièce(s) non posable(s)", v.len()),
            AutoError::Cancelled => write!(f, "calcul annulé"),
            AutoError::VerificationFailed(m) => {
                write!(f, "proposition refusée à la relecture : {m}")
            }
            AutoError::Core(e) => write!(f, "{e}"),
            AutoError::NothingToComplete => write!(f, "aucune pièce manquante à poser"),
            AutoError::ExistingInvalid(_) => {
                write!(f, "placements existants invalides : rien n'est proposé")
            }
            AutoError::ExistingExceeds(v) => {
                write!(f, "{} pièce(s) posée(s) au-delà de la quantité", v.len())
            }
        }
    }
}

impl std::error::Error for AutoError {}

/// Chiffres d'un placement valide.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LayoutFigures {
    pub length_mm: f64,
    pub yield_percent: f64,
}

/// Une pose proposée (mêmes champs que `AddPlacement`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AutoPose {
    pub piece: PieceId,
    pub size: Option<SizeId>,
    pub x_mm: f64,
    pub y_mm: f64,
    pub rotation_deg: f64,
    pub mirrored: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AutoProposal {
    /// `Batch` : à appliquer en une fois (une étape annulable).
    pub command: Command,
    pub poses: Vec<AutoPose>,
    /// Résultat du niveau 1 (rectangles englobants).
    pub level1: LayoutFigures,
    /// Résultat final (niveau 2 si accepté, sinon niveau 1).
    pub proposed: LayoutFigures,
    /// Placement actuel du plan, `None` s'il est vide ou invalide.
    pub current: Option<LayoutFigures>,
    /// Le niveau 2 a été retenu.
    pub refined: bool,
    /// La borne de temps a interrompu des essais ou l'affinage.
    pub time_limited: bool,
    pub trials: usize,
}

// ---------------------------------------------------------------------------------------------

struct RotShape {
    deg: f64,
    min: PointMm,
    w: f64,
    h: f64,
}

struct Variant {
    piece: PieceId,
    size: Option<SizeId>,
    mirrored: bool,
    shape: PieceShape,
    rots: Vec<RotShape>,
}

#[derive(Clone, Copy)]
struct Slot {
    inst: usize,
    rot: usize,
    bx: f64,
    by: f64,
}

fn bounds(pts: &[PointMm]) -> (PointMm, PointMm) {
    let mut lo = PointMm::new(f64::INFINITY, f64::INFINITY);
    let mut hi = PointMm::new(f64::NEG_INFINITY, f64::NEG_INFINITY);
    for p in pts {
        lo = PointMm::new(lo.x.min(p.x), lo.y.min(p.y));
        hi = PointMm::new(hi.x.max(p.x), hi.y.max(p.y));
    }
    (lo, hi)
}

fn rotation_candidates(
    plan: &CuttingPlan,
    shape: &PieceShape,
    mirrored: bool,
    allow_rotation: bool,
) -> Vec<f64> {
    let ok = |r: &f64| {
        !matches!(
            Project::rotation_check(plan, shape, *r, mirrored),
            RotationCheck::Refused { .. }
        )
    };
    let mut v = Vec::new();
    if let Some(r) = [0.0, 180.0].into_iter().find(ok) {
        v.push(r);
    }
    if allow_rotation {
        if let Some(r) = [90.0, 270.0].into_iter().find(ok) {
            v.push(r);
        }
    }
    v
}

fn cancelled(cancel: &AtomicBool) -> Result<(), AutoError> {
    if cancel.load(Ordering::Relaxed) {
        Err(AutoError::Cancelled)
    } else {
        Ok(())
    }
}

/// Squelette (bottom-left) : `width` = largeur utile + écart, rectangles gonflés de l'écart.
fn skyline(
    order: &[usize],
    inst_var: &[usize],
    variants: &[Variant],
    width: f64,
    gap: f64,
) -> Vec<Slot> {
    let mut segs: Vec<(f64, f64, f64)> = vec![(0.0, width, 0.0)];
    let mut out = Vec::new();
    for &inst in order {
        let v = &variants[inst_var[inst]];
        let mut best: Option<(f64, f64, f64, usize)> = None;
        for (ri, r) in v.rots.iter().enumerate() {
            let (w, h) = (r.w + gap, r.h + gap);
            for i in 0..segs.len() {
                let x = segs[i].0;
                if x + w > width + EPS {
                    break;
                }
                let y = segs
                    .iter()
                    .filter(|s| s.0 < x + w - EPS && s.0 + s.1 > x + EPS)
                    .map(|s| s.2)
                    .fold(0.0_f64, f64::max);
                let cand = (y + h, y, x, ri);
                let better = match best {
                    None => true,
                    Some(b) => (cand.0, cand.1, cand.2) < (b.0, b.1, b.2),
                };
                if better {
                    best = Some(cand);
                }
            }
        }
        let Some((_, y, x, ri)) = best else { continue };
        let r = &v.rots[ri];
        let (w, h) = (r.w + gap, r.h + gap);
        out.push(Slot {
            inst,
            rot: ri,
            bx: x,
            by: y,
        });
        let mut next: Vec<(f64, f64, f64)> = Vec::new();
        for s in &segs {
            if s.0 < x - EPS {
                next.push((s.0, (x - s.0).min(s.1), s.2));
            }
            if s.0 + s.1 > x + w + EPS {
                let nx = s.0.max(x + w);
                next.push((nx, s.0 + s.1 - nx, s.2));
            }
        }
        next.push((x, w, y + h));
        next.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut merged: Vec<(f64, f64, f64)> = Vec::new();
        for s in next {
            match merged.last_mut() {
                Some(l) if (l.2 - s.2).abs() <= EPS && (l.0 + l.1 - s.0).abs() <= EPS => l.1 += s.1,
                _ => merged.push(s),
            }
        }
        segs = merged;
    }
    out
}

/// Étagères : chaque rangée prend la hauteur de sa première pièce.
fn shelves(
    order: &[usize],
    inst_var: &[usize],
    variants: &[Variant],
    width: f64,
    gap: f64,
) -> Vec<Slot> {
    struct Shelf {
        y: f64,
        h: f64,
        cursor: f64,
    }
    let mut sh: Vec<Shelf> = Vec::new();
    let mut top = 0.0_f64;
    let mut out = Vec::new();
    for &inst in order {
        let v = &variants[inst_var[inst]];
        let mut best: Option<(f64, usize, usize)> = None;
        for (si, s) in sh.iter().enumerate() {
            for (ri, r) in v.rots.iter().enumerate() {
                let w = r.w + gap;
                if s.cursor + w <= width + EPS && r.h <= s.h + EPS {
                    let left = width - (s.cursor + w);
                    if best.is_none_or(|b| left < b.0) {
                        best = Some((left, si, ri));
                    }
                }
            }
        }
        if let Some((_, si, ri)) = best {
            let s = &mut sh[si];
            out.push(Slot {
                inst,
                rot: ri,
                bx: s.cursor,
                by: s.y,
            });
            s.cursor += v.rots[ri].w + gap;
            continue;
        }
        let Some((ri, r)) = v
            .rots
            .iter()
            .enumerate()
            .filter(|(_, r)| r.w + gap <= width + EPS)
            .min_by(|a, b| a.1.h.total_cmp(&b.1.h))
        else {
            continue;
        };
        out.push(Slot {
            inst,
            rot: ri,
            bx: 0.0,
            by: top,
        });
        sh.push(Shelf {
            y: top,
            h: r.h,
            cursor: r.w + gap,
        });
        top += r.h + gap;
    }
    out
}

/// Mode compléter, niveau 1 : coins libres (bottom-left) autour des rectangles englobants déjà
/// occupés (`fixed`, coordonnées absolues). `bx` est relatif à `x_lo`. Un coin libre existe
/// toujours au-dessus de tout (largeur déjà vérifiée par `prepare_auto`).
fn corners(
    order: &[usize],
    inst_var: &[usize],
    variants: &[Variant],
    fixed: &[(PointMm, PointMm)],
    (x_lo, x_hi): (f64, f64),
    gap: f64,
) -> Vec<Slot> {
    type Rect = (f64, f64, f64, f64);
    let mut occ: Vec<Rect> = fixed.iter().map(|(a, b)| (a.x, a.y, b.x, b.y)).collect();
    let add = |c: &mut Vec<(f64, f64)>, o: Rect| {
        c.extend([
            (o.2 + gap, o.1),
            (o.2 + gap, 0.0),
            (x_lo, o.3 + gap),
            (o.0, o.3 + gap),
        ]);
    };
    let mut cands: Vec<(f64, f64)> = vec![(x_lo, 0.0)];
    for o in &occ {
        add(&mut cands, *o);
    }
    let mut out = Vec::new();
    for &inst in order {
        let v = &variants[inst_var[inst]];
        let hmin = v.rots.iter().map(|r| r.h).fold(f64::INFINITY, f64::min);
        cands.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.total_cmp(&b.0)));
        cands.dedup();
        let mut best: Option<(f64, f64, f64, usize)> = None;
        for &(x, y) in &cands {
            if best.is_some_and(|b| y + hmin > b.0 + EPS) {
                break;
            }
            for (ri, r) in v.rots.iter().enumerate() {
                let (x2, y2) = (x + r.w, y + r.h);
                if x < x_lo - EPS || x2 > x_hi + EPS {
                    continue;
                }
                let free = occ.iter().all(|o| {
                    x >= o.2 + gap - EPS
                        || x2 + gap <= o.0 + EPS
                        || y >= o.3 + gap - EPS
                        || y2 + gap <= o.1 + EPS
                });
                if free && best.is_none_or(|b| (y2, y, x) < (b.0, b.1, b.2)) {
                    best = Some((y2, y, x, ri));
                }
            }
        }
        let Some((_, y, x, ri)) = best else { continue };
        let r = &v.rots[ri];
        let rect = (x, y, x + r.w, y + r.h);
        occ.push(rect);
        add(&mut cands, rect);
        out.push(Slot {
            inst,
            rot: ri,
            bx: x - x_lo,
            by: y,
        });
    }
    out
}

fn slots_length(slots: &[Slot], inst_var: &[usize], variants: &[Variant]) -> f64 {
    slots
        .iter()
        .map(|s| s.by + variants[inst_var[s.inst]].rots[s.rot].h)
        .fold(0.0_f64, f64::max)
}

/// Pose courante du niveau 2.
#[derive(Clone)]
struct Cur {
    /// Placement existant conservé (mode compléter) : jamais déplacé, jamais rendu.
    fixed: bool,
    var: usize,
    deg: f64,
    x: f64,
    y: f64,
    poly: Vec<PointMm>,
    min: PointMm,
    max: PointMm,
}

impl Cur {
    fn set(&mut self, v: &Variant, x: f64, y: f64) {
        self.x = x;
        self.y = y;
        self.poly = place_polygon(&v.shape.polygon, x, y, self.deg, v.mirrored);
        let (lo, hi) = bounds(&self.poly);
        self.min = lo;
        self.max = hi;
    }
}

/// Glisse la pièce `idx` dans la direction `(dx, dy)` (vecteur unitaire) d'au plus `limit` mm.
/// Retourne la distance parcourue.
fn slide(
    cur: &mut [Cur],
    idx: usize,
    variants: &[Variant],
    dir: (f64, f64),
    limit: f64,
    gap: f64,
) -> f64 {
    let mut total = 0.0;
    for _ in 0..MAX_SLIDE_ITER {
        let remaining = limit - total;
        if remaining < MIN_STEP {
            break;
        }
        let me = &cur[idx];
        let (lo, hi) = (
            PointMm::new(
                me.min.x + (dir.0 * remaining).min(0.0) - gap,
                me.min.y + (dir.1 * remaining).min(0.0) - gap,
            ),
            PointMm::new(
                me.max.x + (dir.0 * remaining).max(0.0) + gap,
                me.max.y + (dir.1 * remaining).max(0.0) + gap,
            ),
        );
        let mut dist = f64::INFINITY;
        for (j, o) in cur.iter().enumerate() {
            if j == idx || o.max.x < lo.x || o.min.x > hi.x || o.max.y < lo.y || o.min.y > hi.y {
                continue;
            }
            dist = dist.min(polygon_distance_mm(&me.poly, &o.poly));
        }
        let step = remaining.min(dist - gap);
        if step.is_nan() || step < MIN_STEP {
            break;
        }
        let (nx, ny) = (me.x + dir.0 * step, me.y + dir.1 * step);
        let var = &variants[me.var];
        cur[idx].set(var, nx, ny);
        total += step;
    }
    total
}

struct Prepared {
    variants: Vec<Variant>,
    inst_var: Vec<usize>,
    expected: HashMap<(PieceId, Option<SizeId>, bool), usize>,
}

impl Project {
    fn prepare_auto(
        &self,
        plan: &CuttingPlan,
        items: &[AutoItem],
        options: &AutoOptions,
    ) -> Result<Prepared, AutoError> {
        let (x_lo, x_hi) = row_span(plan);
        let span = x_hi - x_lo;
        let total: usize = items
            .iter()
            .map(|i| i.quantity as usize + i.mirrored_quantity as usize)
            .sum();
        if total == 0 {
            return Err(AutoError::NothingToPlace);
        }
        if total > MAX_INSTANCES {
            return Err(AutoError::TooMany(total));
        }
        let mut shapes: HashMap<(PieceId, Option<SizeId>), Result<PieceShape, String>> =
            HashMap::new();
        let mut variants: Vec<Variant> = Vec::new();
        let mut inst_var = Vec::new();
        let mut expected: HashMap<(PieceId, Option<SizeId>, bool), usize> = HashMap::new();
        let mut bad: Vec<Unplaceable> = Vec::new();
        for item in items {
            let piece = self.piece(item.piece)?;
            if item.mirrored_quantity > 0 && !piece.mirrored_pair {
                return Err(AutoError::MirrorNotAllowed(item.piece));
            }
            for (mirrored, n) in [(false, item.quantity), (true, item.mirrored_quantity)] {
                if n == 0 {
                    continue;
                }
                let vi = match variants.iter().position(|v| {
                    (v.piece, v.size, v.mirrored) == (item.piece, item.size, mirrored)
                }) {
                    Some(vi) => vi,
                    None => {
                        let shape = shapes
                            .entry((item.piece, item.size))
                            .or_insert_with(|| {
                                self.piece_shape(item.piece, item.size)
                                    .map_err(|e| e.to_string())
                            })
                            .clone();
                        let shape = match shape {
                            Ok(s) => s,
                            Err(e) => {
                                if !bad
                                    .iter()
                                    .any(|b| (b.piece, b.size) == (item.piece, item.size))
                                {
                                    bad.push(Unplaceable {
                                        piece: item.piece,
                                        size: item.size,
                                        reason: UnplaceableReason::Unreadable(e),
                                    });
                                }
                                continue;
                            }
                        };
                        let degs =
                            rotation_candidates(plan, &shape, mirrored, options.allow_rotation);
                        let mut rots = Vec::new();
                        let mut narrowest = f64::INFINITY;
                        for deg in &degs {
                            let poly = place_polygon(&shape.polygon, 0.0, 0.0, *deg, mirrored);
                            let (lo, hi) = bounds(&poly);
                            let (w, h) = (hi.x - lo.x, hi.y - lo.y);
                            narrowest = narrowest.min(w);
                            if w <= span + EPS {
                                rots.push(RotShape {
                                    deg: *deg,
                                    min: lo,
                                    w,
                                    h,
                                });
                            }
                        }
                        if rots.is_empty() {
                            if !bad
                                .iter()
                                .any(|b| (b.piece, b.size) == (item.piece, item.size))
                            {
                                bad.push(Unplaceable {
                                    piece: item.piece,
                                    size: item.size,
                                    reason: if degs.is_empty() {
                                        UnplaceableReason::NoRotation
                                    } else {
                                        UnplaceableReason::TooWide {
                                            width_mm: narrowest,
                                            span_mm: span,
                                        }
                                    },
                                });
                            }
                            continue;
                        }
                        variants.push(Variant {
                            piece: item.piece,
                            size: item.size,
                            mirrored,
                            shape,
                            rots,
                        });
                        variants.len() - 1
                    }
                };
                *expected
                    .entry((item.piece, item.size, mirrored))
                    .or_insert(0) += n as usize;
                inst_var.extend(std::iter::repeat_n(vi, n as usize));
            }
        }
        if !bad.is_empty() {
            return Err(AutoError::Unplaceable(bad));
        }
        Ok(Prepared {
            variants,
            inst_var,
            expected,
        })
    }

    /// Commande de remplacement : retire les placements actuels (sauf en mode compléter, où ils
    /// sont conservés) puis ajoute les poses.
    fn auto_command(
        &self,
        plan: CuttingPlanId,
        poses: &[AutoPose],
        keep: bool,
    ) -> Result<Command, AutoError> {
        let mut commands: Vec<Command> = if keep {
            Vec::new()
        } else {
            self.cutting_plan(plan)?
                .placements
                .iter()
                .map(|p| Command::RemovePlacement {
                    plan,
                    placement: p.id,
                })
                .collect()
        };
        commands.extend(poses.iter().map(|p| Command::AddPlacement {
            plan,
            piece: p.piece,
            size: p.size,
            x_mm: p.x_mm,
            y_mm: p.y_mm,
            rotation_deg: p.rotation_deg,
            mirrored: p.mirrored,
        }));
        Ok(Command::Batch { commands })
    }

    /// Applique la commande à une copie et relit : validité complète et quantités exactes.
    fn verify_auto(
        &self,
        plan: CuttingPlanId,
        command: &Command,
        expected: &HashMap<(PieceId, Option<SizeId>, bool), usize>,
        kept: &[CuttingPlacement],
    ) -> Result<LayoutFigures, String> {
        let mut copy = self.clone();
        copy.apply(command.clone()).map_err(|e| e.to_string())?;
        let analysis = copy.analyze_cutting_plan(plan).map_err(|e| e.to_string())?;
        if !analysis.is_valid() {
            return Err("chevauchement, débordement, écart ou rotation refusés".into());
        }
        let plan_ref = copy.cutting_plan(plan).map_err(|e| e.to_string())?;
        if !kept.is_empty() && plan_ref.placements.get(..kept.len()) != Some(kept) {
            return Err("un placement existant a été modifié".into());
        }
        let mut got: HashMap<(PieceId, Option<SizeId>, bool), usize> = HashMap::new();
        for p in &plan_ref.placements {
            *got.entry((p.piece, p.size, p.mirrored)).or_insert(0) += 1;
        }
        if &got != expected {
            return Err("quantités posées différentes des quantités demandées".into());
        }
        let y = plan_yield(&copy, plan_ref, &analysis);
        let f = y.figures.ok_or("métrage non calculable")?;
        Ok(LayoutFigures {
            length_mm: f.length_mm,
            yield_percent: f.yield_percent,
        })
    }

    /// Proposition de placement automatique pour `plan` (lecture pure : le projet n'est pas
    /// modifié). Voir la documentation du module.
    pub fn propose_cutting_layout(
        &self,
        plan: CuttingPlanId,
        items: &[AutoItem],
        options: &AutoOptions,
        cancel: &AtomicBool,
    ) -> Result<AutoProposal, AutoError> {
        self.propose_cutting_layout_mode(plan, items, options, AutoMode::ReplaceAll, cancel)
    }

    /// Mode compléter : relit les placements existants (refus nommé s'ils sont invalides ou
    /// excèdent la quantité demandée) et en déduit les instances manquantes.
    #[allow(clippy::type_complexity)]
    fn missing_items(
        &self,
        plan: CuttingPlanId,
        items: &[AutoItem],
    ) -> Result<
        (
            Vec<AutoItem>,
            Vec<Cur>,
            HashMap<(PieceId, Option<SizeId>, bool), usize>,
        ),
        AutoError,
    > {
        let analysis = self.analyze_cutting_plan(plan)?;
        if !analysis.is_valid() {
            return Err(AutoError::ExistingInvalid(ExistingProblems {
                overlaps: analysis.overlaps.len(),
                gaps: analysis.gaps.len(),
                overflows: analysis.overflows.len()
                    + analysis.hide_overflows.len()
                    + analysis.defect_hits.len(),
                rotation_refused: analysis.rotation_refused.len(),
                unreadable: analysis.unreadable.len(),
            }));
        }
        type Key = (PieceId, Option<SizeId>, bool);
        let mut have: HashMap<Key, usize> = HashMap::new();
        for p in &self.cutting_plan(plan)?.placements {
            *have.entry((p.piece, p.size, p.mirrored)).or_insert(0) += 1;
        }
        // Cibles cumulées par (pièce, taille, miroir), dans l'ordre des éléments.
        let mut wanted: Vec<(Key, usize)> = Vec::new();
        for it in items {
            for (mirrored, n) in [(false, it.quantity), (true, it.mirrored_quantity)] {
                let key = (it.piece, it.size, mirrored);
                match wanted.iter_mut().find(|(k, _)| *k == key) {
                    Some((_, w)) => *w += n as usize,
                    None => wanted.push((key, n as usize)),
                }
            }
        }
        let over: Vec<OverQuantity> = wanted
            .iter()
            .filter(|(k, w)| have.get(k).copied().unwrap_or(0) > *w)
            .map(|((piece, size, mirrored), w)| OverQuantity {
                piece: *piece,
                size: *size,
                mirrored: *mirrored,
                placed: have[&(*piece, *size, *mirrored)],
                wanted: *w,
            })
            .collect();
        if !over.is_empty() {
            return Err(AutoError::ExistingExceeds(over));
        }
        let missing_of = |piece: PieceId, size: Option<SizeId>, mirrored: bool| {
            wanted
                .iter()
                .find(|(k, _)| *k == (piece, size, mirrored))
                .map_or(0, |(k, w)| w - have.get(k).copied().unwrap_or(0))
        };
        let mut missing: Vec<AutoItem> = Vec::new();
        for it in items {
            if missing
                .iter()
                .any(|m| (m.piece, m.size) == (it.piece, it.size))
            {
                continue;
            }
            let (q, m) = (
                missing_of(it.piece, it.size, false),
                missing_of(it.piece, it.size, true),
            );
            if q + m > 0 {
                missing.push(AutoItem {
                    piece: it.piece,
                    size: it.size,
                    quantity: q as u32,
                    mirrored_quantity: m as u32,
                });
            }
        }
        let mut expected = have;
        for (k, w) in wanted {
            if w > 0 {
                expected.insert(k, w);
            }
        }
        let fixed = analysis
            .outlines
            .iter()
            .map(|o| {
                let (min, max) = bounds(&o.polygon);
                Cur {
                    fixed: true,
                    var: 0,
                    deg: 0.0,
                    x: 0.0,
                    y: 0.0,
                    poly: o.polygon.clone(),
                    min,
                    max,
                }
            })
            .collect();
        Ok((missing, fixed, expected))
    }

    /// Proposition selon le [`AutoMode`] (lecture pure). `CompleteMissing` sur un plan sans
    /// placement se comporte comme `ReplaceAll` (rien à conserver).
    pub fn propose_cutting_layout_mode(
        &self,
        plan: CuttingPlanId,
        items: &[AutoItem],
        options: &AutoOptions,
        mode: AutoMode,
        cancel: &AtomicBool,
    ) -> Result<AutoProposal, AutoError> {
        let started = Instant::now();
        let deadline = started + Duration::from_millis(options.time_limit_ms);
        let plan_ref = self.cutting_plan(plan)?;
        // Peau (US-15.5) : proposition dédiée, la laize rectangulaire ne contraint plus la pose.
        if plan_ref.hide.is_some() {
            return self.propose_hide_layout(plan, items, options, None, mode, cancel);
        }
        let keep = mode == AutoMode::CompleteMissing && !plan_ref.placements.is_empty();
        let kept: Vec<CuttingPlacement> = if keep {
            plan_ref.placements.clone()
        } else {
            Vec::new()
        };
        let complete = if keep {
            cancelled(cancel)?;
            if items.iter().all(|i| i.quantity + i.mirrored_quantity == 0) {
                return Err(AutoError::NothingToPlace);
            }
            let found = self.missing_items(plan, items)?;
            if found.0.is_empty() {
                return Err(AutoError::NothingToComplete);
            }
            Some(found)
        } else {
            None
        };
        let items = complete.as_ref().map_or(items, |c| &c.0[..]);
        let fixed: &[Cur] = complete.as_ref().map_or(&[], |c| &c.1[..]);
        let prep = self.prepare_auto(plan_ref, items, options)?;
        let (x_lo, x_hi) = row_span(plan_ref);
        let gap = plan_ref.min_gap_mm;
        let (width, n) = (x_hi - x_lo + gap, prep.inst_var.len());
        let Prepared {
            variants,
            inst_var,
            expected,
        } = &prep;
        let expected = complete.as_ref().map_or(expected, |c| &c.2);
        let fixed_boxes: Vec<(PointMm, PointMm)> = fixed.iter().map(|c| (c.min, c.max)).collect();
        let area = |i: usize| variants[inst_var[i]].shape.area_mm2;
        let dims = |i: usize| {
            let r = &variants[inst_var[i]].rots[0];
            (r.w, r.h)
        };
        type Key<'a> = Box<dyn Fn(usize) -> f64 + 'a>;
        let keys: Vec<Key> = vec![
            Box::new(area),
            Box::new(|i| dims(i).1),
            Box::new(|i| dims(i).0.max(dims(i).1)),
            Box::new(|i| dims(i).0),
        ];
        let mut best: Option<(f64, Vec<Slot>)> = None;
        let (mut trials, mut time_limited) = (0, false);
        'outer: for key in &keys {
            let mut order: Vec<usize> = (0..n).collect();
            order.sort_by(|a, b| key(*b).total_cmp(&key(*a)).then(a.cmp(b)));
            for use_skyline in [true, false] {
                cancelled(cancel)?;
                if trials > 0 && Instant::now() >= deadline {
                    time_limited = true;
                    break 'outer;
                }
                let slots = if complete.is_some() {
                    if !use_skyline {
                        continue;
                    }
                    corners(&order, inst_var, variants, &fixed_boxes, (x_lo, x_hi), gap)
                } else if use_skyline {
                    skyline(&order, inst_var, variants, width, gap)
                } else {
                    shelves(&order, inst_var, variants, width, gap)
                };
                trials += 1;
                if slots.len() != n {
                    continue;
                }
                let len = slots_length(&slots, inst_var, variants);
                if best.as_ref().is_none_or(|b| len < b.0 - EPS) {
                    best = Some((len, slots));
                }
            }
        }
        let Some((_, mut slots1)) = best else {
            return Err(AutoError::VerificationFailed(
                "aucun essai n'a posé toutes les pièces".into(),
            ));
        };
        slots1.sort_by_key(|s| s.inst);
        let to_cur = |s: &Slot| {
            let v = &variants[inst_var[s.inst]];
            let r = &v.rots[s.rot];
            let mut c = Cur {
                fixed: false,
                var: inst_var[s.inst],
                deg: r.deg,
                x: 0.0,
                y: 0.0,
                poly: Vec::new(),
                min: PointMm::new(0.0, 0.0),
                max: PointMm::new(0.0, 0.0),
            };
            c.set(v, s.bx + x_lo - r.min.x, s.by - r.min.y);
            c
        };
        let poses_of = |cur: &[Cur]| -> Vec<AutoPose> {
            cur.iter()
                .filter(|c| !c.fixed)
                .map(|c| {
                    let v = &variants[c.var];
                    AutoPose {
                        piece: v.piece,
                        size: v.size,
                        x_mm: c.x,
                        y_mm: c.y,
                        rotation_deg: c.deg,
                        mirrored: v.mirrored,
                    }
                })
                .collect()
        };
        let mut cur: Vec<Cur> = fixed.to_vec();
        cur.extend(slots1.iter().map(to_cur));
        let poses1 = poses_of(&cur);
        let cmd1 = self.auto_command(plan, &poses1, keep)?;
        let fig1 = self
            .verify_auto(plan, &cmd1, expected, &kept)
            .map_err(AutoError::VerificationFailed)?;

        // Niveau 2 : glisser vers le bas puis vers la gauche jusqu'au contact.
        for _ in 0..MAX_PASSES {
            let mut moved = false;
            let mut order: Vec<usize> = (0..cur.len()).filter(|i| !cur[*i].fixed).collect();
            order.sort_by(|a, b| {
                cur[*a]
                    .min
                    .y
                    .total_cmp(&cur[*b].min.y)
                    .then(cur[*a].min.x.total_cmp(&cur[*b].min.x))
                    .then(a.cmp(b))
            });
            for idx in order {
                cancelled(cancel)?;
                if Instant::now() >= deadline {
                    time_limited = true;
                    break;
                }
                let down = cur[idx].min.y.max(0.0);
                moved |= slide(&mut cur, idx, variants, (0.0, -1.0), down, gap) > 0.0;
                let left = (cur[idx].min.x - x_lo).max(0.0);
                moved |= slide(&mut cur, idx, variants, (-1.0, 0.0), left, gap) > 0.0;
            }
            if !moved || time_limited {
                break;
            }
        }
        let poses2 = poses_of(&cur);
        let level2 = self.auto_command(plan, &poses2, keep).ok().and_then(|c| {
            self.verify_auto(plan, &c, expected, &kept)
                .ok()
                .map(|f| (c, f))
        });
        let (command, poses, proposed, refined) = match level2 {
            Some((c, f)) if f.yield_percent >= fig1.yield_percent - 1e-9 && poses2 != poses1 => {
                (c, poses2, f, true)
            }
            _ => (cmd1, poses1, fig1, false),
        };
        let current = self
            .cutting_plan_yield(plan)
            .ok()
            .and_then(|y| y.figures)
            .map(|f| LayoutFigures {
                length_mm: f.length_mm,
                yield_percent: f.yield_percent,
            });
        Ok(AutoProposal {
            command,
            poses,
            level1: fig1,
            proposed,
            current,
            refined,
            time_limited,
            trials,
        })
    }
}
