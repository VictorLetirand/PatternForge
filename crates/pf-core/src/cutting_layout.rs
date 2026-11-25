//! Plans de coupe, lecture (US-15.1 / US-15.2) : **contours posés, contraintes et diagnostics**,
//! calculés à la lecture, jamais stockés. Module pur : aucune E/S, aucune interface.
//!
//! Plan de la laize : `x` suit la **laize** (de 0 à `fabric_width_mm`, lisières comprises), `y`
//! suit la **longueur** de l'étoffe et croît **vers le haut** (axes mathématiques, comme la
//! construction). Le début de l'étoffe est `y = 0` : un contour posé à `y < 0` déborde.
//!
//! Convention de pose d'un [`CuttingPlacement`] (à lire dans cet ordre) :
//! 1. le contour de **coupe** de la pièce (à défaut, sa couture : marge nulle) à la taille voulue
//!    est ramené dans son repère local : l'**origine de la pièce** est le coin bas-gauche de la
//!    boîte englobante de ce contour (taille et marges comprises) ;
//! 2. **miroir** si `mirrored` : symétrie autour de l'axe y local, `(x, y) -> (-x, y)` ;
//! 3. **rotation** de `rotation_deg` autour de l'origine de la pièce, sens trigonométrique
//!    (angle positif = sens inverse des aiguilles d'une montre, y vers le haut) :
//!    `(x, y) -> (x cos θ - y sin θ, x sin θ + y cos θ)` ;
//! 4. **translation** de `(x_mm, y_mm)`.
//!
//! Exemple : carré 100 × 100, `(10 ; 20)`, rotation 90° : le coin local `(100 ; 100)` devient
//! `(-100 ; 100)` puis `(-90 ; 120)` ; le carré posé occupe `x ∈ [-90 ; 10]`, `y ∈ [20 ; 120]`.
//!
//! Zone utile : `[lisière ; laize - lisière]`. Pour `Folded`, le pli est à `x = laize / 2` : une
//! pièce doit tenir **d'un seul côté**, soit dans `[lisière ; laize/2]`, soit dans
//! `[laize/2 ; laize - lisière]` (elle ne chevauche pas le pli).
//!
//! Courbes aplaties à [`FLATTEN_TOLERANCE_MM`] ; chevauchement = aire d'intersection exacte
//! (polygone à polygone) > [`OVERLAP_MIN_MM2`] ; écart = distance exacte entre contours.

use crate::command::Command;
use crate::cutting_plan::{CuttingPlan, FabricFold};
use crate::error::CoreError;
use crate::measurement_ops::same_name;
use crate::model::*;
use pf_geometry::region::{trace_geometry, EdgeGeom};
use pf_geometry::{sin_cos_deg, Bbox, PointMm};
use std::collections::HashMap;

/// Tolérance d'aplatissement des courbes (mm).
pub const FLATTEN_TOLERANCE_MM: f64 = 0.05;
/// Aire d'intersection au-delà de laquelle deux contours se chevauchent (mm²).
pub const OVERLAP_MIN_MM2: f64 = 0.01;
/// Écart d'angle toléré autour de 0° / 180° pour le droit-fil (degrés).
pub const GRAIN_TOLERANCE_DEG: f64 = 0.01;
const EPS: f64 = 1e-6;

/// Contour de coupe d'une pièce à une taille, dans son repère local.
#[derive(Debug, Clone, PartialEq)]
pub struct PieceShape {
    /// Contour aplati, anti-horaire, coin bas-gauche de la boîte à l'origine.
    pub polygon: Vec<PointMm>,
    /// Aire de coupe exacte (courbes non aplaties), mm².
    pub area_mm2: f64,
    pub width_mm: f64,
    pub height_mm: f64,
    /// Droit-fil principal (local), `None` sans droit-fil.
    pub grain: Option<(PointMm, PointMm)>,
}

/// Rotation d'un placement au regard du droit-fil.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RotationCheck {
    Allowed,
    /// Permise mais à vérifier (pièce sans droit-fil ou plan sans sens).
    AllowedUnchecked(WarningKind),
    /// Refusée : écart de droit-fil (degrés, entre 0,01 et 90) hors de 0° / 180°.
    Refused {
        deviation_deg: f64,
    },
}

/// Signalements non bloquants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WarningKind {
    /// « Sans droit-fil, vérifier le sens » : la pièce n'a pas de droit-fil.
    PieceWithoutGrain,
    /// Le plan n'a pas de sens de droit-fil : rien à vérifier.
    PlanWithoutGrain,
    /// Posée en miroir alors que la pièce n'est pas déclarée en paire.
    MirrorWithoutPair,
}

impl WarningKind {
    pub fn key(self) -> &'static str {
        match self {
            WarningKind::PieceWithoutGrain => "piece_without_grain",
            WarningKind::PlanWithoutGrain => "plan_without_grain",
            WarningKind::MirrorWithoutPair => "mirror_without_pair",
        }
    }
}

/// Pourquoi un contour posé déborde de la zone utile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverflowReason {
    /// Hors de la zone utile (lisière ou bord de la laize).
    OutsideUseful,
    /// Avant le début de l'étoffe (`y < 0`).
    BeforeStart,
    /// Pliée : la pièce enjambe le pli.
    CrossesFold,
}

impl OverflowReason {
    pub fn key(self) -> &'static str {
        match self {
            OverflowReason::OutsideUseful => "outside_useful",
            OverflowReason::BeforeStart => "before_start",
            OverflowReason::CrossesFold => "crosses_fold",
        }
    }
}

/// Un contour posé.
#[derive(Debug, Clone, PartialEq)]
pub struct PlacedOutline {
    pub placement: PlacementId,
    pub piece: PieceId,
    pub size: Option<SizeId>,
    /// Anti-horaire, dans le plan de la laize.
    pub polygon: Vec<PointMm>,
    pub bbox: Bbox,
    /// Aire de coupe exacte (invariante par pose), mm².
    pub area_mm2: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Overlap {
    pub a: PlacementId,
    pub b: PlacementId,
    pub area_mm2: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GapViolation {
    pub a: PlacementId,
    pub b: PlacementId,
    pub distance_mm: f64,
    pub required_mm: f64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Overflow {
    pub placement: PlacementId,
    pub reasons: Vec<OverflowReason>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RotationRefused {
    pub placement: PlacementId,
    pub deviation_deg: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlacementWarning {
    pub placement: PlacementId,
    pub kind: WarningKind,
}

/// Une pièce (et taille) posée plus souvent que demandé.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OverPlaced {
    pub piece: PieceId,
    pub size: Option<SizeId>,
    pub placed: usize,
    pub expected: usize,
}

/// Diagnostics d'un plan, recalculés à chaque lecture.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PlanAnalysis {
    pub outlines: Vec<PlacedOutline>,
    /// Placements dont le contour ne peut pas être calculé (message du cœur).
    pub unreadable: Vec<(PlacementId, String)>,
    pub overlaps: Vec<Overlap>,
    pub gaps: Vec<GapViolation>,
    pub overflows: Vec<Overflow>,
    pub rotation_refused: Vec<RotationRefused>,
    pub warnings: Vec<PlacementWarning>,
    /// Pièces de la matière du plan sans aucune pose dans ce plan.
    pub unplaced: Vec<PieceId>,
    pub over_placed: Vec<OverPlaced>,
    /// Poses qui touchent un défaut de la peau (US-15.5) ; vide sans peau.
    pub defect_hits: Vec<crate::hide_layout::DefectHit>,
    /// Poses qui sortent du contour de la peau (US-15.5) ; vide sans peau.
    pub hide_overflows: Vec<crate::hide_layout::HideOverflow>,
    /// Qualité de chaque pose (US-15.5) ; vide sans peau.
    pub qualities: Vec<crate::hide_layout::PlacementQuality>,
}

impl PlanAnalysis {
    /// Plan sans chevauchement, débordement, rotation refusée, écart violé ni contour illisible
    /// (les signalements et les pièces non posées ne l'invalident pas).
    pub fn is_valid(&self) -> bool {
        self.overlaps.is_empty()
            && self.gaps.is_empty()
            && self.overflows.is_empty()
            && self.rotation_refused.is_empty()
            && self.unreadable.is_empty()
            && self.defect_hits.is_empty()
            && self.hide_overflows.is_empty()
    }
}

// ---------------------------------------------------------------------------------------------
// Zone utile
// ---------------------------------------------------------------------------------------------

/// Bandes `[x_min ; x_max]` où une pièce peut tenir : une en plat, deux (de part et d'autre du
/// pli) en pliée.
pub fn useful_bands(plan: &CuttingPlan) -> Vec<(f64, f64)> {
    let (w, s) = (plan.fabric_width_mm, plan.selvedge_mm);
    match plan.fold {
        FabricFold::Flat => vec![(s, w - s)],
        FabricFold::Folded => vec![(s, w / 2.0), (w / 2.0, w - s)],
    }
}

/// Largeur utile à la pose en rangées : la laize moins deux lisières (plat), la demi-laize
/// moins une lisière (pliée : on pose d'un seul côté du pli).
pub fn row_span(plan: &CuttingPlan) -> (f64, f64) {
    useful_bands(plan)[0]
}

// ---------------------------------------------------------------------------------------------
// Géométrie des contours
// ---------------------------------------------------------------------------------------------

fn flatten_cubic(c: &pf_geometry::CubicBezier, depth: u32, out: &mut Vec<PointMm>) {
    let chord = c.p3 - c.p0;
    let len = chord.norm();
    let dist = |p: PointMm| {
        if len <= 1e-12 {
            pf_geometry::distance_mm(p, c.p0)
        } else {
            chord.cross(p - c.p0).abs() / len
        }
    };
    if depth == 0 || 0.75 * dist(c.c1).max(dist(c.c2)) <= FLATTEN_TOLERANCE_MM {
        out.push(c.p3);
        return;
    }
    let q0 = c.p0.lerp(c.c1, 0.5);
    let q1 = c.c1.lerp(c.c2, 0.5);
    let q2 = c.c2.lerp(c.p3, 0.5);
    let r0 = q0.lerp(q1, 0.5);
    let r1 = q1.lerp(q2, 0.5);
    let m = r0.lerp(r1, 0.5);
    flatten_cubic(
        &pf_geometry::CubicBezier::new(c.p0, q0, r0, m),
        depth - 1,
        out,
    );
    flatten_cubic(
        &pf_geometry::CubicBezier::new(m, r1, q2, c.p3),
        depth - 1,
        out,
    );
}

fn flatten_edge(e: &EdgeGeom, out: &mut Vec<PointMm>) {
    match e {
        EdgeGeom::Line(_, b) => out.push(*b),
        EdgeGeom::Cubic(c) => flatten_cubic(c, 18, out),
        EdgeGeom::Arc(a) => {
            let r = a.radius_mm;
            let step = if r <= FLATTEN_TOLERANCE_MM {
                std::f64::consts::PI
            } else {
                2.0 * (1.0 - FLATTEN_TOLERANCE_MM / r).acos()
            };
            let n = ((a.sweep_rad.abs() / step).ceil() as usize).clamp(1, 20_000);
            for i in 1..=n {
                let ang = a.start_rad + a.sweep_rad * i as f64 / n as f64;
                out.push(PointMm::new(
                    a.center.x + r * ang.cos(),
                    a.center.y + r * ang.sin(),
                ));
            }
        }
    }
}

/// Aire signée (shoelace), positive pour un sens anti-horaire.
pub fn polygon_signed_area(p: &[PointMm]) -> f64 {
    let n = p.len();
    (0..n)
        .map(|i| {
            let (a, b) = (p[i], p[(i + 1) % n]);
            a.x * b.y - b.x * a.y
        })
        .sum::<f64>()
        / 2.0
}

fn bbox_of(points: &[PointMm]) -> Option<Bbox> {
    let (first, rest) = points.split_first()?;
    let mut bb = Bbox::from_point(*first);
    rest.iter().for_each(|p| bb.include(*p));
    Some(bb)
}

/// Point du repère local de la pièce posé dans le plan (convention du module).
pub fn place_point(
    local: PointMm,
    x_mm: f64,
    y_mm: f64,
    rotation_deg: f64,
    mirrored: bool,
) -> PointMm {
    let q = if mirrored {
        PointMm::new(-local.x, local.y)
    } else {
        local
    };
    let (s, c) = sin_cos_deg(rotation_deg);
    PointMm::new(c * q.x - s * q.y + x_mm, s * q.x + c * q.y + y_mm)
}

/// Contour posé (anti-horaire, le miroir inverse le sens de parcours donc on le rétablit).
pub fn place_polygon(
    local: &[PointMm],
    x_mm: f64,
    y_mm: f64,
    rotation_deg: f64,
    mirrored: bool,
) -> Vec<PointMm> {
    let mut v: Vec<PointMm> = local
        .iter()
        .map(|p| place_point(*p, x_mm, y_mm, rotation_deg, mirrored))
        .collect();
    if polygon_signed_area(&v) < 0.0 {
        v.reverse();
    }
    v
}

// ---------------------------------------------------------------------------------------------
// Opérations polygone à polygone
// ---------------------------------------------------------------------------------------------

fn tri_area2(a: PointMm, b: PointMm, c: PointMm) -> f64 {
    (b - a).cross(c - a)
}

/// Triangulation par oreilles d'un polygone simple anti-horaire.
pub fn triangulate_polygon(poly: &[PointMm]) -> Vec<[PointMm; 3]> {
    let mut idx: Vec<usize> = (0..poly.len()).collect();
    let mut tris = Vec::new();
    let inside = |p: PointMm, a: PointMm, b: PointMm, c: PointMm| {
        let same = |q: PointMm| (q.x - p.x).abs() <= 1e-12 && (q.y - p.y).abs() <= 1e-12;
        !(same(a) || same(b) || same(c))
            && tri_area2(a, b, p) >= -1e-12
            && tri_area2(b, c, p) >= -1e-12
            && tri_area2(c, a, p) >= -1e-12
    };
    let mut start = 0usize;
    while idx.len() > 3 {
        let n = idx.len();
        let mut found = None;
        for k in 0..n {
            let i = (start + k) % n;
            let (ip, ic, inx) = (idx[(i + n - 1) % n], idx[i], idx[(i + 1) % n]);
            let (a, b, c) = (poly[ip], poly[ic], poly[inx]);
            if tri_area2(a, b, c) <= 1e-12 {
                continue;
            }
            if idx
                .iter()
                .filter(|&&j| j != ip && j != ic && j != inx)
                .any(|&j| inside(poly[j], a, b, c))
            {
                continue;
            }
            found = Some(i);
            break;
        }
        let i = match found {
            Some(i) => {
                let (ip, ic, inx) = (idx[(i + n - 1) % n], idx[i], idx[(i + 1) % n]);
                tris.push([poly[ip], poly[ic], poly[inx]]);
                i
            }
            // Sommet dégénéré (colinéaire, doublon) ou contour auto-sécant : on retire le sommet
            // de plus petite contribution pour toujours terminer.
            None => (0..n)
                .min_by(|&i, &j| {
                    let f = |i: usize| {
                        tri_area2(
                            poly[idx[(i + n - 1) % n]],
                            poly[idx[i]],
                            poly[idx[(i + 1) % n]],
                        )
                        .abs()
                    };
                    f(i).total_cmp(&f(j))
                })
                .unwrap_or(0),
        };
        idx.remove(i);
        start = if i == 0 { 0 } else { i - 1 };
    }
    if idx.len() == 3 {
        let t = [poly[idx[0]], poly[idx[1]], poly[idx[2]]];
        if tri_area2(t[0], t[1], t[2]) > 1e-12 {
            tris.push(t);
        }
    }
    tris
}

/// Sutherland-Hodgman : `subject` découpé par le triangle convexe anti-horaire `clip`.
fn clip_convex(subject: &[PointMm], clip: &[PointMm; 3]) -> Vec<PointMm> {
    let mut out: Vec<PointMm> = subject.to_vec();
    for k in 0..3 {
        let (a, b) = (clip[k], clip[(k + 1) % 3]);
        let input = std::mem::take(&mut out);
        let n = input.len();
        for i in 0..n {
            let (p, q) = (input[i], input[(i + 1) % n]);
            let (sp, sq) = ((b - a).cross(p - a), (b - a).cross(q - a));
            if sp >= 0.0 {
                out.push(p);
            }
            if (sp > 0.0 && sq < 0.0) || (sp < 0.0 && sq > 0.0) {
                let t = sp / (sp - sq);
                out.push(p.lerp(q, t));
            }
        }
        if out.is_empty() {
            break;
        }
    }
    out
}

fn tri_bbox(t: &[PointMm; 3]) -> Bbox {
    bbox_of(t).unwrap_or_else(|| Bbox::from_point(t[0]))
}

fn bbox_overlap(a: &Bbox, b: &Bbox, margin: f64) -> bool {
    a.min.x <= b.max.x + margin
        && b.min.x <= a.max.x + margin
        && a.min.y <= b.max.y + margin
        && b.min.y <= a.max.y + margin
}

/// Aire d'intersection exacte de deux polygones simples anti-horaires (mm²).
pub fn intersection_area_mm2(a: &[PointMm], b: &[PointMm]) -> f64 {
    let (Some(ba), Some(bb)) = (bbox_of(a), bbox_of(b)) else {
        return 0.0;
    };
    if !bbox_overlap(&ba, &bb, 0.0) {
        return 0.0;
    }
    let (ta, tb) = (triangulate_polygon(a), triangulate_polygon(b));
    let boxes_b: Vec<Bbox> = tb.iter().map(tri_bbox).collect();
    let mut total = 0.0;
    for t in &ta {
        let bt = tri_bbox(t);
        for (u, bu) in tb.iter().zip(&boxes_b) {
            if bbox_overlap(&bt, bu, 0.0) {
                total += polygon_signed_area(&clip_convex(t, u)).max(0.0);
            }
        }
    }
    total
}

/// Polygone simple anti-horaire triangulé une fois, pour des intersections répétées (US-15.5 :
/// contour, défauts et zones d'une peau face à de nombreuses poses). Même résultat que
/// [`intersection_area_mm2`].
#[derive(Debug, Clone)]
pub struct TriSet {
    tris: Vec<[PointMm; 3]>,
    boxes: Vec<Bbox>,
    bbox: Option<Bbox>,
}

impl TriSet {
    pub fn new(poly: &[PointMm]) -> TriSet {
        let tris = triangulate_polygon(poly);
        let boxes = tris.iter().map(tri_bbox).collect();
        TriSet {
            tris,
            boxes,
            bbox: bbox_of(poly),
        }
    }

    pub fn bbox(&self) -> Option<Bbox> {
        self.bbox
    }

    /// Aire d'intersection exacte avec un autre polygone triangulé (mm²).
    pub fn intersection_area(&self, other: &TriSet) -> f64 {
        let (Some(ba), Some(bb)) = (&self.bbox, &other.bbox) else {
            return 0.0;
        };
        if !bbox_overlap(ba, bb, 0.0) {
            return 0.0;
        }
        let mut total = 0.0;
        for (t, bt) in self.tris.iter().zip(&self.boxes) {
            for (u, bu) in other.tris.iter().zip(&other.boxes) {
                if bbox_overlap(bt, bu, 0.0) {
                    total += polygon_signed_area(&clip_convex(t, u)).max(0.0);
                }
            }
        }
        total
    }
}

fn point_segment_distance(p: PointMm, a: PointMm, b: PointMm) -> f64 {
    let d = b - a;
    let l2 = d.dot(d);
    let t = if l2 <= 1e-18 {
        0.0
    } else {
        ((p - a).dot(d) / l2).clamp(0.0, 1.0)
    };
    pf_geometry::distance_mm(p, a.lerp(b, t))
}

fn segments_cross(a: PointMm, b: PointMm, c: PointMm, d: PointMm) -> bool {
    let (d1, d2) = ((b - a).cross(c - a), (b - a).cross(d - a));
    let (d3, d4) = ((d - c).cross(a - c), (d - c).cross(b - c));
    ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0))
        && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0))
}

/// Distance exacte entre les contours de deux polygones (0 si leurs bords se touchent ou se
/// coupent ; un contour entièrement dans l'autre est traité par le chevauchement, pas ici).
pub fn polygon_distance_mm(a: &[PointMm], b: &[PointMm]) -> f64 {
    let (na, nb) = (a.len(), b.len());
    let mut best = f64::INFINITY;
    for i in 0..na {
        let (p, q) = (a[i], a[(i + 1) % na]);
        let sb = Bbox {
            min: PointMm::new(p.x.min(q.x), p.y.min(q.y)),
            max: PointMm::new(p.x.max(q.x), p.y.max(q.y)),
        };
        for j in 0..nb {
            let (r, s) = (b[j], b[(j + 1) % nb]);
            let tb = Bbox {
                min: PointMm::new(r.x.min(s.x), r.y.min(s.y)),
                max: PointMm::new(r.x.max(s.x), r.y.max(s.y)),
            };
            if !bbox_overlap(&sb, &tb, best.min(1e12)) {
                continue;
            }
            if segments_cross(p, q, r, s) {
                return 0.0;
            }
            let d = point_segment_distance(p, r, s)
                .min(point_segment_distance(q, r, s))
                .min(point_segment_distance(r, p, q))
                .min(point_segment_distance(s, p, q));
            best = best.min(d);
        }
    }
    best
}

// ---------------------------------------------------------------------------------------------
// Pièce → contour local
// ---------------------------------------------------------------------------------------------

fn invalid(msg: String) -> CoreError {
    CoreError::Invalid(msg)
}

/// Refus de « Poser la sélection » (aucune trace dans le projet). Les variantes portent les
/// valeurs : l'interface les met dans la langue choisie ; `Display` est le texte français du cœur.
#[derive(Debug, Clone, PartialEq)]
pub enum RowLayoutError {
    NothingToPlace,
    /// Pièce plus large que la largeur utile (mm).
    TooWide {
        name: String,
        width_mm: f64,
        span_mm: f64,
    },
    /// Aucune des rotations 0°, 180°, 90°, 270° ne respecte le droit-fil.
    NoRotation {
        name: String,
    },
    Core(CoreError),
}

impl From<CoreError> for RowLayoutError {
    fn from(e: CoreError) -> Self {
        RowLayoutError::Core(e)
    }
}

impl std::fmt::Display for RowLayoutError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RowLayoutError::NothingToPlace => write!(f, "aucune pièce à poser"),
            RowLayoutError::TooWide {
                name,
                width_mm,
                span_mm,
            } => write!(
                f,
                "« {name} » ({width_mm:.1} mm) est plus large que la largeur utile ({span_mm:.1} mm)"
            ),
            RowLayoutError::NoRotation { name } => {
                write!(f, "« {name} » : aucune rotation ne respecte le droit-fil")
            }
            RowLayoutError::Core(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for RowLayoutError {}

/// Première rotation de 0°, 180°, 90°, 270° (sans miroir) que le droit-fil permet.
pub fn first_allowed_rotation(plan: &CuttingPlan, shape: &PieceShape) -> Option<f64> {
    [0.0, 180.0, 90.0, 270.0].into_iter().find(|r| {
        !matches!(
            Project::rotation_check(plan, shape, *r, false),
            RotationCheck::Refused { .. }
        )
    })
}

impl Project {
    /// Contour de coupe aplati d'une pièce à une taille (`None` = taille de base), dans son repère
    /// local, avec son aire exacte et son droit-fil principal.
    pub fn piece_shape(
        &self,
        piece: PieceId,
        size: Option<SizeId>,
    ) -> Result<PieceShape, CoreError> {
        let name = self.piece(piece)?.name.clone();
        let graded;
        let project = if size.is_some() {
            graded = self.graded_project(size)?;
            &graded
        } else {
            self
        };
        let p = project.piece(piece)?;
        if p.outline.is_empty() {
            return Err(invalid(format!("la pièce « {name} » n'a pas de contour")));
        }
        let cut = project
            .piece_cut_contour(piece)
            .map_err(|e| invalid(format!("contour de coupe de « {name} » : {e}")))?;
        let edges: Vec<EdgeGeom> = cut.traces.iter().flat_map(trace_geometry).collect();
        let first = edges
            .first()
            .ok_or_else(|| invalid(format!("contour de coupe de « {name} » vide")))?;
        let mut poly = vec![first.start()];
        for e in &edges {
            flatten_edge(e, &mut poly);
        }
        if poly.len() > 1 && pf_geometry::distance_mm(poly[0], poly[poly.len() - 1]) <= EPS {
            poly.pop();
        }
        poly.dedup_by(|a, b| pf_geometry::distance_mm(*a, *b) <= 1e-9);
        if poly.len() < 3 || poly.iter().any(|q| !q.x.is_finite() || !q.y.is_finite()) {
            return Err(invalid(format!("contour de coupe de « {name} » dégénéré")));
        }
        if polygon_signed_area(&poly) < 0.0 {
            poly.reverse();
        }
        let bb = bbox_of(&poly).ok_or_else(|| invalid(format!("contour de « {name} » vide")))?;
        let o = bb.min;
        let rel = |q: PointMm| PointMm::new(q.x - o.x, q.y - o.y);
        let grain = project
            .piece_grain_lines(piece)
            .into_iter()
            .find(|g| g.main)
            .map(|g| (rel(g.from), rel(g.to)));
        Ok(PieceShape {
            polygon: poly.into_iter().map(rel).collect(),
            area_mm2: cut.area_mm2,
            width_mm: bb.width(),
            height_mm: bb.height(),
            grain,
        })
    }

    /// Contraintes de rotation d'une pose (voir [`RotationCheck`]).
    pub fn rotation_check(
        plan: &CuttingPlan,
        shape: &PieceShape,
        rotation_deg: f64,
        mirrored: bool,
    ) -> RotationCheck {
        let Some(sense) = plan.grain_direction else {
            return RotationCheck::AllowedUnchecked(WarningKind::PlanWithoutGrain);
        };
        let Some((from, to)) = shape.grain else {
            return RotationCheck::AllowedUnchecked(WarningKind::PieceWithoutGrain);
        };
        let a = place_point(from, 0.0, 0.0, rotation_deg, mirrored);
        let b = place_point(to, 0.0, 0.0, rotation_deg, mirrored);
        let angle = (b.y - a.y).atan2(b.x - a.x).to_degrees();
        let d = (angle - sense).rem_euclid(180.0);
        let dev = d.min(180.0 - d);
        if dev <= GRAIN_TOLERANCE_DEG {
            RotationCheck::Allowed
        } else {
            RotationCheck::Refused { deviation_deg: dev }
        }
    }

    /// Pièces candidates d'un plan : avec contour, de la matière du plan (toutes si le plan n'a
    /// pas de matière).
    pub fn plan_candidate_pieces(&self, plan: &CuttingPlan) -> Vec<PieceId> {
        self.pieces
            .iter()
            .filter(|p| !p.outline.is_empty())
            .filter(|p| match &plan.material {
                None => true,
                Some(m) => p.material.as_deref().is_some_and(|pm| same_name(pm, m)),
            })
            .map(|p| p.id)
            .collect()
    }

    /// Diagnostics complets d'un plan (lecture pure).
    pub fn analyze_cutting_plan(&self, plan: CuttingPlanId) -> Result<PlanAnalysis, CoreError> {
        let plan = self.cutting_plan(plan)?;
        let mut an = PlanAnalysis::default();
        let mut cache: HashMap<(PieceId, Option<SizeId>), Result<PieceShape, String>> =
            HashMap::new();
        let bands = useful_bands(plan);
        let (w, sel) = (plan.fabric_width_mm, plan.selvedge_mm);
        for pl in &plan.placements {
            let shape = cache
                .entry((pl.piece, pl.size))
                .or_insert_with(|| {
                    self.piece_shape(pl.piece, pl.size)
                        .map_err(|e| e.to_string())
                })
                .clone();
            let shape = match shape {
                Ok(s) => s,
                Err(e) => {
                    an.unreadable.push((pl.id, e));
                    continue;
                }
            };
            match Self::rotation_check(plan, &shape, pl.rotation_deg, pl.mirrored) {
                RotationCheck::Allowed => {}
                RotationCheck::AllowedUnchecked(kind) => an.warnings.push(PlacementWarning {
                    placement: pl.id,
                    kind,
                }),
                RotationCheck::Refused { deviation_deg } => {
                    an.rotation_refused.push(RotationRefused {
                        placement: pl.id,
                        deviation_deg,
                    })
                }
            }
            if pl.mirrored && !self.piece(pl.piece)?.mirrored_pair {
                an.warnings.push(PlacementWarning {
                    placement: pl.id,
                    kind: WarningKind::MirrorWithoutPair,
                });
            }
            let polygon = place_polygon(
                &shape.polygon,
                pl.x_mm,
                pl.y_mm,
                pl.rotation_deg,
                pl.mirrored,
            );
            let Some(bbox) = bbox_of(&polygon) else {
                continue;
            };
            let mut reasons = Vec::new();
            let fits = |lo: f64, hi: f64| bbox.min.x >= lo - EPS && bbox.max.x <= hi + EPS;
            // Avec une peau, son contour remplace la laize (US-15.5) : voir `hide_layout`.
            if plan.hide.is_none() {
                if !bands.iter().any(|&(lo, hi)| fits(lo, hi)) {
                    reasons.push(if plan.fold == FabricFold::Folded && fits(sel, w - sel) {
                        OverflowReason::CrossesFold
                    } else {
                        OverflowReason::OutsideUseful
                    });
                }
                if bbox.min.y < -EPS {
                    reasons.push(OverflowReason::BeforeStart);
                }
            }
            if !reasons.is_empty() {
                an.overflows.push(Overflow {
                    placement: pl.id,
                    reasons,
                });
            }
            an.outlines.push(PlacedOutline {
                placement: pl.id,
                piece: pl.piece,
                size: pl.size,
                polygon,
                bbox,
                area_mm2: shape.area_mm2,
            });
        }
        if let Some(h) = &plan.hide {
            crate::hide_layout::analyze_hide(h, &mut an);
        }
        let n = an.outlines.len();
        for i in 0..n {
            for j in (i + 1)..n {
                let (a, b) = (&an.outlines[i], &an.outlines[j]);
                if !bbox_overlap(&a.bbox, &b.bbox, plan.min_gap_mm) {
                    continue;
                }
                let area = intersection_area_mm2(&a.polygon, &b.polygon);
                if area > OVERLAP_MIN_MM2 {
                    an.overlaps.push(Overlap {
                        a: a.placement,
                        b: b.placement,
                        area_mm2: area,
                    });
                } else if plan.min_gap_mm > 0.0 {
                    let d = polygon_distance_mm(&a.polygon, &b.polygon);
                    if d < plan.min_gap_mm - EPS {
                        an.gaps.push(GapViolation {
                            a: a.placement,
                            b: b.placement,
                            distance_mm: d,
                            required_mm: plan.min_gap_mm,
                        });
                    }
                }
            }
        }
        let mut counts: Vec<((PieceId, Option<SizeId>), usize)> = Vec::new();
        for pl in &plan.placements {
            match counts.iter_mut().find(|(k, _)| *k == (pl.piece, pl.size)) {
                Some((_, c)) => *c += 1,
                None => counts.push(((pl.piece, pl.size), 1)),
            }
        }
        for ((piece, size), placed) in counts {
            let p = self.piece(piece)?;
            let expected = p.quantity as usize * if p.mirrored_pair { 2 } else { 1 };
            if placed > expected {
                an.over_placed.push(OverPlaced {
                    piece,
                    size,
                    placed,
                    expected,
                });
            }
        }
        an.unplaced = self
            .plan_candidate_pieces(plan)
            .into_iter()
            .filter(|id| !plan.placements.iter().any(|pl| pl.piece == *id))
            .collect();
        Ok(an)
    }

    /// « Poser la sélection » : commande unique (`Batch` d'`AddPlacement`, une étape annulable)
    /// qui dépose `wanted` en **rangées simples** au-dessus de ce qui est déjà posé : de gauche à
    /// droite, retour à la ligne à la largeur utile ([`row_span`]), écart `min_gap_mm`, rotation 0°
    /// (sinon la première de 180°, 90°, 270° que le droit-fil permet), sans miroir. Refus clair
    /// d'une pièce plus large que la largeur utile ou sans contour lisible. Aucune optimisation.
    pub fn plan_row_layout(
        &self,
        plan: CuttingPlanId,
        wanted: &[(PieceId, Option<SizeId>)],
    ) -> Result<Command, RowLayoutError> {
        let plan_ref = self.cutting_plan(plan)?;
        if wanted.is_empty() {
            return Err(RowLayoutError::NothingToPlace);
        }
        let (x_lo, x_hi) = row_span(plan_ref);
        let gap = plan_ref.min_gap_mm;
        let existing = self.analyze_cutting_plan(plan)?;
        let mut row_y = existing
            .outlines
            .iter()
            .map(|o| o.bbox.max.y)
            .fold(None, |m: Option<f64>, v| Some(m.map_or(v, |m| m.max(v))))
            .map_or(0.0, |m| (m + gap).max(0.0));
        let (mut cursor, mut row_h) = (x_lo, 0.0_f64);
        let mut commands = Vec::new();
        let mut cache: HashMap<(PieceId, Option<SizeId>), PieceShape> = HashMap::new();
        for &(piece, size) in wanted {
            if let std::collections::hash_map::Entry::Vacant(e) = cache.entry((piece, size)) {
                e.insert(self.piece_shape(piece, size)?);
            }
            let shape = &cache[&(piece, size)];
            let name = &self.piece(piece)?.name;
            let rotation = first_allowed_rotation(plan_ref, shape)
                .ok_or_else(|| RowLayoutError::NoRotation { name: name.clone() })?;
            let placed = place_polygon(&shape.polygon, 0.0, 0.0, rotation, false);
            let bb = bbox_of(&placed).ok_or_else(|| {
                RowLayoutError::Core(invalid(format!("contour de « {name} » vide")))
            })?;
            let (pw, ph) = (bb.width(), bb.height());
            if pw > x_hi - x_lo + EPS {
                return Err(RowLayoutError::TooWide {
                    name: name.clone(),
                    width_mm: pw,
                    span_mm: x_hi - x_lo,
                });
            }
            if cursor > x_lo && cursor + pw > x_hi + EPS {
                row_y += row_h + gap;
                cursor = x_lo;
                row_h = 0.0;
            }
            commands.push(Command::AddPlacement {
                plan,
                piece,
                size,
                x_mm: cursor - bb.min.x,
                y_mm: row_y - bb.min.y,
                rotation_deg: rotation,
                mirrored: false,
            });
            cursor += pw + gap;
            row_h = row_h.max(ph);
        }
        Ok(Command::Batch { commands })
    }
}
