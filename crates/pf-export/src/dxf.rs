//! Export DXF de type AAMA / ASTM D6673 (US-12.1).
//!
//! **Honnêteté** : c'est un export « de type AAMA / ASTM », écrit d'après une connaissance
//! générale du format sans le texte de la norme. La table de couches ([`LAYERS`]) est **à
//! confirmer avec la spécification officielle** ([`LAYER_TABLE_CONFIRMED`]) et l'ouverture
//! dans un logiciel métier n'a **pas** été vérifiée (étape humaine de la story).
//!
//! Choix de format (voir `docs/DECISIONS.md`) :
//!
//! - DXF texte ASCII, version **R12 (`AC1009`)** : structure sans identifiants d'objet,
//!   lisible par à peu près tous les lecteurs. Les variables `$INSUNITS` et `$MEASUREMENT`
//!   sont plus récentes que R12 : elles sont écrites telles que le cahier les demande ; un
//!   lecteur strict R12 les ignore.
//! - Sections `HEADER`, `TABLES` (`LTYPE`, `LAYER`), `BLOCKS`, `ENTITIES`, `EOF`.
//! - **Une pièce = un bloc** `PIECE_<id>`, inséré une fois à l'origine. La géométrie du bloc est
//!   déjà en coordonnées de feuille de plan : placement manuel de la pièce (US-04.5, rotation
//!   comprise) ou placement automatique du plan, jamais de chevauchement des boîtes.
//! - Unités : millimètres, échelle 1:1 (`$INSUNITS` = 4, `$MEASUREMENT` = 1), 5 décimales.
//! - Courbes **discrétisées** en polylignes (flèche au plus [`CURVE_TOLERANCE_MM`]), sans arc ni
//!   spline DXF ; cercles de perçage et de bouton écrits en `CIRCLE` exacts.
//! - Texte : ASCII pur, tout autre caractère est écrit `\U+XXXX`.
//!
//! Taille unique : la gradation (E09 / E10) n'existe pas, **aucun fichier de règles `.rul`** n'est
//! écrit. Le contour de coupe n'est **jamais calculé** (pas de marge de couture, E05) : le
//! contour de la pièce est écrit tel que dessiné.
//!
//! Lecture seule : le projet n'est jamais modifié. Rien n'est perdu en silence : tout ce qui
//! n'est pas écrit figure dans [`DxfReport::issues`].

use crate::{grain_arrow_marks, PieceExportMode};
use pf_core::notch::NotchPlace;
use pf_core::{
    ButtonShape, CoreError, MarkingShape, PathRole, Piece, PieceId, Placement, PlanItem, Project,
    Shape,
};
use pf_geometry::region::EdgeGeom;
use pf_geometry::{Arc, Bbox, CubicBezier, PointMm};
use std::collections::BTreeMap;
use std::fmt::{self, Write as _};

/// Flèche maximale (mm) de la discrétisation des courbes en polylignes.
pub const CURVE_TOLERANCE_MM: f64 = 0.001;
/// Version DXF écrite (`$ACADVER`).
pub const DXF_VERSION: &str = "AC1009";
/// Valeur de `$INSUNITS` : 4 = millimètres.
pub const INSUNITS_MM: i64 = 4;
/// Valeur de `$MEASUREMENT` : 1 = métrique.
pub const MEASUREMENT_METRIC: i64 = 1;
/// Hauteur du texte d'annotation (mm).
pub const TEXT_HEIGHT_MM: f64 = 4.0;
/// Longueur maximale (caractères) d'une ligne de texte avant troncature signalée.
pub const TEXT_MAX_CHARS: usize = 80;
/// Valeur absolue maximale d'une coordonnée écrite (mm) : au-delà, export refusé.
pub const MAX_ABS_MM: f64 = 1.0e8;
/// Nombre de décimales des coordonnées.
pub const DECIMALS: usize = 5;
/// La table de couches n'a **pas** été confirmée avec la spécification officielle.
pub const LAYER_TABLE_CONFIRMED: bool = false;

const MAX_SUBDIVISION_DEPTH: u32 = 18;
const MAX_ARC_STEPS: usize = 200_000;
const MERGE_MM: f64 = 1e-9;
const DEGENERATE_AREA_MM2: f64 = 1e-9;

/// Rôle d'une couche normalisée.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayerRole {
    /// Contour de pièce (la ligne de coupe quand le tracé a ce rôle).
    Boundary,
    /// Points de rotation (réservé : non écrit).
    TurnPoints,
    /// Points de courbe (réservé : non écrit).
    CurvePoints,
    Notches,
    Grainline,
    InternalLines,
    InternalCutouts,
    /// Ligne de miroir (réservé : le modèle n'a pas de ligne de miroir).
    Mirror,
    /// Perçages, pointages, croix, boutons.
    Drill,
    Sewing,
    Text,
    /// Jambes des pinces (numéro libre hors de la table d'US-12.1, à confirmer). Déclarée dans
    /// le fichier **seulement** si une pince est écrite (les autres fichiers restent identiques).
    DartLegs,
    /// Bords sur le pli (PH-5b) : ligne de pliure et libellé « ON FOLD » (numéro libre hors de la
    /// table d'US-12.1, à confirmer). Déclarée dans le fichier **seulement** si un bord est écrit.
    FoldLine,
}

/// Une couche de la table normalisée. Le nom de la couche dans le fichier est son numéro
/// (`"1"`, `"4"`, …), comme dans les fichiers de type AAMA.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DxfLayer {
    pub number: u32,
    /// Libellé descriptif (documentation et rapport ; non écrit dans le fichier).
    pub name: &'static str,
    pub role: LayerRole,
    /// Couleur d'écran (index ACI) : jamais le seul moyen de reconnaître un rôle.
    pub color: u16,
    /// L'export écrit des entités sur cette couche. Faux = réservée.
    pub written: bool,
}

/// **La** table de couches (une seule source). Numéros et rôles **à confirmer** avec la
/// spécification officielle ASTM D6673 / AAMA ([`LAYER_TABLE_CONFIRMED`]). Triée par numéro.
pub const LAYERS: &[DxfLayer] = &[
    DxfLayer {
        number: 1,
        name: "PIECE BOUNDARY",
        role: LayerRole::Boundary,
        color: 7,
        written: true,
    },
    DxfLayer {
        number: 2,
        name: "TURN POINTS",
        role: LayerRole::TurnPoints,
        color: 1,
        written: false,
    },
    DxfLayer {
        number: 3,
        name: "CURVE POINTS",
        role: LayerRole::CurvePoints,
        color: 3,
        written: false,
    },
    DxfLayer {
        number: 4,
        name: "NOTCHES",
        role: LayerRole::Notches,
        color: 5,
        written: true,
    },
    DxfLayer {
        number: 6,
        name: "GRAINLINE",
        role: LayerRole::Grainline,
        color: 2,
        written: true,
    },
    DxfLayer {
        number: 7,
        name: "INTERNAL LINES",
        role: LayerRole::InternalLines,
        color: 4,
        written: true,
    },
    DxfLayer {
        number: 8,
        name: "INTERNAL CUTOUTS",
        role: LayerRole::InternalCutouts,
        color: 6,
        written: true,
    },
    DxfLayer {
        number: 10,
        name: "MIRROR LINE",
        role: LayerRole::Mirror,
        color: 8,
        written: false,
    },
    DxfLayer {
        number: 13,
        name: "DRILL HOLES AND MARKS",
        role: LayerRole::Drill,
        color: 1,
        written: true,
    },
    DxfLayer {
        number: 15,
        name: "SEWING LINE",
        role: LayerRole::Sewing,
        color: 3,
        written: true,
    },
    DxfLayer {
        number: 84,
        name: "TEXT",
        role: LayerRole::Text,
        color: 7,
        written: true,
    },
    DxfLayer {
        number: 90,
        name: "DART LEGS",
        role: LayerRole::DartLegs,
        color: 30,
        written: true,
    },
    DxfLayer {
        number: 91,
        name: "FOLD LINE",
        role: LayerRole::FoldLine,
        color: 40,
        written: true,
    },
];

/// La couche d'un rôle (toujours présente dans [`LAYERS`]).
pub fn layer_of(role: LayerRole) -> &'static DxfLayer {
    LAYERS
        .iter()
        .find(|l| l.role == role)
        .expect("chaque rôle a une couche")
}

/// Ce qui est signalé à l'utilisateur après un export.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DxfIssueKind {
    // ----- omissions : quelque chose n'est PAS écrit -----
    /// Pièce sans contour (pièce-étiquette) : rien n'est écrit pour elle.
    EmptyOutline,
    /// Contour illisible (tracé dégénéré) : pièce non écrite.
    OutlineUnreadable,
    /// Contour non fermé : pièce non écrite.
    OutlineOpen,
    /// Contour d'aire nulle : pièce non écrite.
    DegenerateOutline,
    /// Crans dont la géométrie ne se calcule pas.
    NotchUnreadable,
    /// Droit-fil désigné mais illisible.
    GrainUnreadable,
    /// Éléments internes illisibles.
    InternalUnreadable,
    /// Marquages illisibles.
    MarkingUnreadable,
    /// Groupes de boutons illisibles.
    ButtonsUnreadable,
    /// Texte tronqué à [`TEXT_MAX_CHARS`] caractères.
    TextTruncated,
    /// Pinces non calculables (`PieceIssue::DartInvalid`) : jambes non écrites.
    DartUnreadable,
    // ----- remarques : tout est écrit, mais avec une réserve -----
    /// Le contour a pour rôle « ligne de couture » : écrit sur la couche de couture, **aucun
    /// contour de coupe** n'est écrit (aucune marge de couture n'est calculée).
    NoCutContour,
    /// Le contour n'a pas le rôle « ligne de coupe » : écrit tel que dessiné sur la couche de
    /// contour de pièce.
    BoundaryNotCut,
    /// Le contour se recoupe lui-même.
    SelfIntersecting,
    /// La pièce n'a pas de droit-fil.
    MissingGrain,
    /// Pièce à marge de couture dont le contour de coupe n'a pas pu être calculé (US-05.4) : la
    /// coupe n'est **pas** écrite ; le contour de couture l'est (couche de couture).
    CutContourFailed,
    /// Crans de rempli non calculables (US-05.4) : le contour de coupe est écrit sans eux.
    FoldNotchesFailed,
}

impl DxfIssueKind {
    /// Identifiant stable, indépendant de la langue.
    pub fn key(self) -> &'static str {
        match self {
            DxfIssueKind::EmptyOutline => "empty_outline",
            DxfIssueKind::OutlineUnreadable => "outline_unreadable",
            DxfIssueKind::OutlineOpen => "outline_open",
            DxfIssueKind::DegenerateOutline => "degenerate_outline",
            DxfIssueKind::NotchUnreadable => "notch_unreadable",
            DxfIssueKind::GrainUnreadable => "grain_unreadable",
            DxfIssueKind::InternalUnreadable => "internal_unreadable",
            DxfIssueKind::MarkingUnreadable => "marking_unreadable",
            DxfIssueKind::ButtonsUnreadable => "buttons_unreadable",
            DxfIssueKind::TextTruncated => "text_truncated",
            DxfIssueKind::DartUnreadable => "dart_unreadable",
            DxfIssueKind::NoCutContour => "no_cut_contour",
            DxfIssueKind::BoundaryNotCut => "boundary_not_cut",
            DxfIssueKind::SelfIntersecting => "self_intersecting",
            DxfIssueKind::MissingGrain => "missing_grain",
            DxfIssueKind::CutContourFailed => "cut_contour_failed",
            DxfIssueKind::FoldNotchesFailed => "fold_notches_failed",
        }
    }

    /// Vrai si quelque chose n'est **pas** écrit (omission) ; faux pour une simple remarque.
    pub fn is_omission(self) -> bool {
        !matches!(
            self,
            DxfIssueKind::NoCutContour
                | DxfIssueKind::BoundaryNotCut
                | DxfIssueKind::SelfIntersecting
                | DxfIssueKind::MissingGrain
        )
    }
}

/// Un point signalé pour une pièce.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DxfIssue {
    pub piece: String,
    pub kind: DxfIssueKind,
    /// Nombre d'éléments concernés (1 pour une pièce ou une remarque).
    pub count: usize,
}

/// Une pièce écrite.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DxfPieceInfo {
    pub id: PieceId,
    pub name: String,
    /// Nom du bloc DXF.
    pub block: String,
    /// Entités écrites dans le bloc.
    pub entities: usize,
}

/// Entités écrites sur une couche.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DxfLayerUse {
    pub layer: &'static DxfLayer,
    pub entities: usize,
}

/// Rapport d'export : ce qui est écrit et ce qui ne l'est pas.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DxfReport {
    pub pieces: Vec<DxfPieceInfo>,
    /// Couches qui portent au moins une entité, dans l'ordre de la table.
    pub layers: Vec<DxfLayerUse>,
    pub issues: Vec<DxfIssue>,
}

impl DxfReport {
    /// Éléments **non écrits**.
    pub fn omissions(&self) -> impl Iterator<Item = &DxfIssue> {
        self.issues.iter().filter(|i| i.kind.is_omission())
    }
    /// Remarques (tout est écrit).
    pub fn notes(&self) -> impl Iterator<Item = &DxfIssue> {
        self.issues.iter().filter(|i| !i.kind.is_omission())
    }
}

/// Fichier produit et son rapport.
#[derive(Debug, Clone, PartialEq)]
pub struct DxfExport {
    pub text: String,
    pub report: DxfReport,
}

impl DxfExport {
    pub fn bytes(&self) -> &[u8] {
        self.text.as_bytes()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum DxfError {
    /// Le projet n'a aucune pièce.
    NoPieces,
    /// Il y a des pièces mais aucune n'est exportable ; le détail est dans les points signalés.
    NothingExported(Vec<DxfIssue>),
    /// Une coordonnée n'est pas un nombre fini.
    NonFinite,
    /// Une coordonnée dépasse [`MAX_ABS_MM`].
    OutOfRange,
    Core(CoreError),
}

impl fmt::Display for DxfError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DxfError::NoPieces => write!(
                f,
                "le projet ne contient aucune pièce à exporter en DXF (extrayez d'abord une pièce)"
            ),
            DxfError::NothingExported(issues) => write!(
                f,
                "aucune pièce ne peut être écrite ({} élément(s) signalé(s))",
                issues.len()
            ),
            DxfError::NonFinite => {
                write!(f, "une coordonnée n'est pas un nombre fini : export refusé")
            }
            DxfError::OutOfRange => {
                write!(f, "une coordonnée dépasse {MAX_ABS_MM} mm : export refusé")
            }
            DxfError::Core(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for DxfError {}

impl From<CoreError> for DxfError {
    fn from(e: CoreError) -> Self {
        DxfError::Core(e)
    }
}

// ----- discrétisation --------------------------------------------------------------------------

pub(crate) fn push_dedup(out: &mut Vec<PointMm>, p: PointMm) {
    if out
        .last()
        .is_none_or(|l| pf_geometry::distance_mm(*l, p) > MERGE_MM)
    {
        out.push(p);
    }
}

fn split_cubic(c: &CubicBezier) -> (CubicBezier, CubicBezier) {
    let m = |a: PointMm, b: PointMm| a.lerp(b, 0.5);
    let (p01, p12, p23) = (m(c.p0, c.c1), m(c.c1, c.c2), m(c.c2, c.p3));
    let (a, b) = (m(p01, p12), m(p12, p23));
    let mid = m(a, b);
    (
        CubicBezier::new(c.p0, p01, a, mid),
        CubicBezier::new(mid, b, p23, c.p3),
    )
}

/// Majorant rigoureux de la distance de la courbe à sa corde : `B(t) − L(t) =
/// 3t(1−t)[(1−t)e₁ + t·e₂]` donc `‖B − L‖ ≤ ¾·max(‖e₁‖, ‖e₂‖)`, `eᵢ` = écart des points de
/// contrôle aux tiers de la corde. Un NaN compte comme « assez plat » (l'export le refuse à
/// l'écriture).
fn cubic_is_flat(c: &CubicBezier, tol: f64) -> bool {
    let e1 = pf_geometry::distance_mm(c.c1, c.p0.lerp(c.p3, 1.0 / 3.0));
    let e2 = pf_geometry::distance_mm(c.c2, c.p0.lerp(c.p3, 2.0 / 3.0));
    let deviation = 0.75 * e1.max(e2);
    deviation.is_nan() || deviation <= tol
}

/// Aire inutilisable : nulle, minuscule ou NaN.
fn negligible_area(area_mm2: f64) -> bool {
    area_mm2.is_nan() || area_mm2 <= DEGENERATE_AREA_MM2
}

fn cubic_rec(c: &CubicBezier, tol: f64, depth: u32, out: &mut Vec<PointMm>) {
    if depth >= MAX_SUBDIVISION_DEPTH || cubic_is_flat(c, tol) {
        push_dedup(out, c.p3);
        return;
    }
    let (l, r) = split_cubic(c);
    cubic_rec(&l, tol, depth + 1, out);
    cubic_rec(&r, tol, depth + 1, out);
}

pub(crate) fn cubic_points(c: &CubicBezier, tol: f64, out: &mut Vec<PointMm>) {
    push_dedup(out, c.p0);
    cubic_rec(c, tol, 0, out);
}

/// Pas d'angle maximal (rad) pour une flèche d'au plus `tol` sur un rayon `r`.
fn max_step_rad(r: f64, tol: f64) -> f64 {
    if r > tol {
        2.0 * (1.0 - tol / r).acos()
    } else {
        std::f64::consts::PI
    }
}

pub(crate) fn arc_steps(r: f64, sweep_rad: f64, tol: f64) -> usize {
    let step = max_step_rad(r, tol).max(1e-9);
    let n = (sweep_rad.abs() / step).ceil();
    if n.is_finite() {
        (n as usize).clamp(1, MAX_ARC_STEPS)
    } else {
        1
    }
}

pub(crate) fn arc_points(a: &Arc, tol: f64, out: &mut Vec<PointMm>) {
    for p in a.sample(arc_steps(a.radius_mm, a.sweep_rad, tol)) {
        push_dedup(out, p);
    }
}

/// Polyligne d'un tracé : sommets de départ à arrivée (cercle : fermé, premier = dernier).
fn shape_points(shape: &Shape, tol: f64) -> Vec<PointMm> {
    let mut out = Vec::new();
    match shape {
        Shape::Line(a, b) => {
            push_dedup(&mut out, *a);
            push_dedup(&mut out, *b);
        }
        Shape::Cubic(c) => cubic_points(c, tol, &mut out),
        Shape::Arc(a) => arc_points(a, tol, &mut out),
        Shape::Circle(c) => {
            let n = arc_steps(c.circle.radius_mm, std::f64::consts::TAU, tol).max(8);
            for p in c.sample(n) {
                push_dedup(&mut out, p);
            }
        }
        Shape::Spline(s) => {
            for c in s.segments() {
                cubic_points(c, tol, &mut out);
            }
        }
    }
    out
}

pub(crate) fn edge_points(e: &EdgeGeom, tol: f64, out: &mut Vec<PointMm>) {
    match e {
        EdgeGeom::Line(a, b) => {
            push_dedup(out, *a);
            push_dedup(out, *b);
        }
        EdgeGeom::Cubic(c) => cubic_points(c, tol, out),
        EdgeGeom::Arc(a) => arc_points(a, tol, out),
    }
}

fn shoelace(pts: &[PointMm]) -> f64 {
    let n = pts.len();
    (0..n)
        .map(|i| {
            let (a, b) = (pts[i], pts[(i + 1) % n]);
            a.x * b.y - b.x * a.y
        })
        .sum::<f64>()
        / 2.0
}

// ----- entités ---------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum Entity {
    Line {
        layer: u32,
        a: PointMm,
        b: PointMm,
    },
    Point {
        layer: u32,
        p: PointMm,
    },
    Circle {
        layer: u32,
        c: PointMm,
        r: f64,
    },
    Polyline {
        layer: u32,
        pts: Vec<PointMm>,
        closed: bool,
    },
    Text {
        layer: u32,
        at: PointMm,
        text: String,
    },
}

impl Entity {
    fn layer(&self) -> u32 {
        match self {
            Entity::Line { layer, .. }
            | Entity::Point { layer, .. }
            | Entity::Circle { layer, .. }
            | Entity::Polyline { layer, .. }
            | Entity::Text { layer, .. } => *layer,
        }
    }

    fn include_in(&self, bb: &mut Option<Bbox>) {
        let mut add = |p: PointMm| match bb {
            Some(b) => b.include(p),
            None => *bb = Some(Bbox::from_point(p)),
        };
        match self {
            Entity::Line { a, b, .. } => {
                add(*a);
                add(*b);
            }
            Entity::Point { p, .. } => add(*p),
            Entity::Circle { c, r, .. } => {
                add(PointMm::new(c.x - r, c.y - r));
                add(PointMm::new(c.x + r, c.y + r));
            }
            Entity::Polyline { pts, .. } => pts.iter().for_each(|p| add(*p)),
            Entity::Text { at, text, .. } => {
                add(*at);
                add(PointMm::new(
                    at.x + text_width_mm(text),
                    at.y + TEXT_HEIGHT_MM,
                ));
            }
        }
    }
}

/// Largeur approchée (mm) d'un texte, pour l'emprise et le centrage.
fn text_width_mm(text: &str) -> f64 {
    text.chars().count() as f64 * TEXT_HEIGHT_MM * 0.6
}

/// Rectangle (min, max) d'un texte DXF dont `at` est le coin bas-gauche.
fn text_rect(at: PointMm, text: &str) -> (PointMm, PointMm) {
    (
        at,
        PointMm::new(at.x + text_width_mm(text), at.y + TEXT_HEIGHT_MM),
    )
}

struct Block {
    name: String,
    entities: Vec<Entity>,
}

/// Texte DXF : ASCII pur ; contrôles remplacés par une espace, `\` et tout caractère non ASCII
/// écrits `\U+XXXX` (unités UTF-16). Renvoie le texte et si une troncature a eu lieu.
pub(crate) fn dxf_text(s: &str) -> (String, bool) {
    let mut out = String::new();
    let mut truncated = false;
    for (i, ch) in s.chars().enumerate() {
        if i >= TEXT_MAX_CHARS {
            truncated = true;
            break;
        }
        if ch.is_ascii() && ch != '\\' {
            out.push(if ch.is_ascii_control() { ' ' } else { ch });
        } else if ch.is_control() {
            out.push(' ');
        } else {
            let mut buf = [0u16; 2];
            for unit in ch.encode_utf16(&mut buf) {
                let _ = write!(out, "\\U+{unit:04X}");
            }
        }
    }
    (out, truncated)
}

// ----- construction des pièces -----------------------------------------------------------------

struct Xf {
    placement: Placement,
    origin: PointMm,
}

impl Xf {
    fn p(&self, p: PointMm) -> PointMm {
        self.placement.apply(self.origin, p)
    }
    fn all(&self, pts: &[PointMm]) -> Vec<PointMm> {
        pts.iter().map(|p| self.p(*p)).collect()
    }
}

/// Suites ouvertes d'un contour dont les parties « pliure » (vrai) ne sont pas écrites (PH-5b) :
/// chaque suite est continue, la première commence juste après une pliure. Vide si tout est pliure.
fn open_runs(parts: Vec<(bool, Vec<PointMm>)>) -> Vec<Vec<PointMm>> {
    let n = parts.len();
    let start = (0..n)
        .find(|&i| parts[i].0 && !parts[(i + 1) % n].0)
        .map_or(0, |i| (i + 1) % n);
    let mut runs: Vec<Vec<PointMm>> = Vec::new();
    let mut current: Vec<PointMm> = Vec::new();
    for k in 0..n {
        let (fold, pts) = &parts[(start + k) % n];
        if *fold {
            if !current.is_empty() {
                runs.push(std::mem::take(&mut current));
            }
        } else {
            for p in pts {
                push_dedup(&mut current, *p);
            }
        }
    }
    if !current.is_empty() {
        runs.push(current);
    }
    runs
}

fn closed_without_repeat(mut pts: Vec<PointMm>) -> Vec<PointMm> {
    if pts.len() > 2 && pf_geometry::distance_mm(pts[0], pts[pts.len() - 1]) <= MERGE_MM {
        pts.pop();
    }
    pts
}

struct Collector {
    issues: Vec<DxfIssue>,
}

impl Collector {
    fn add(&mut self, piece: &str, kind: DxfIssueKind, count: usize) {
        if count > 0 {
            self.issues.push(DxfIssue {
                piece: piece.to_string(),
                kind,
                count,
            });
        }
    }
}

/// Bloc d'une pièce, ou `None` (avec un point signalé) si elle n'est pas écrite.
fn build_piece(
    project: &Project,
    piece: &Piece,
    item: &PlanItem,
    block_name: String,
    issues: &mut Collector,
    mode: PieceExportMode,
) -> Option<Block> {
    let name = piece.name.as_str();
    let (Some(placement), Some(origin)) = (item.placement, item.origin) else {
        let kind = if piece.outline.is_empty() {
            DxfIssueKind::EmptyOutline
        } else {
            DxfIssueKind::OutlineUnreadable
        };
        issues.add(name, kind, 1);
        return None;
    };
    let xf = Xf { placement, origin };
    let Ok(contour) = project.piece_contour(piece.id) else {
        issues.add(name, DxfIssueKind::OutlineUnreadable, 1);
        return None;
    };
    if !contour.closed {
        issues.add(name, DxfIssueKind::OutlineOpen, 1);
        return None;
    }
    let Ok(sheet) = project.piece_sheet(piece.id) else {
        issues.add(name, DxfIssueKind::OutlineUnreadable, 1);
        return None;
    };
    // Bords sur le pli (PH-5b) : ni couture ni coupe, écrits à part (couche 91).
    let fold_edges = project.piece_fold_edges(piece.id);
    let fold_ids: Vec<pf_core::EntityId> = fold_edges.iter().map(|f| f.entity).collect();
    let mut per_edge: Vec<(pf_core::EntityId, Vec<PointMm>)> = Vec::new();
    let mut outline: Vec<PointMm> = Vec::new();
    let mut roles = Vec::new();
    for e in &piece.outline {
        let (Ok(shape), Ok(entity)) = (sheet.shape(e.entity), sheet.entity(e.entity)) else {
            issues.add(name, DxfIssueKind::OutlineUnreadable, 1);
            return None;
        };
        roles.push(entity.role);
        let mut pts = shape_points(&shape, CURVE_TOLERANCE_MM);
        if e.reversed {
            pts.reverse();
        }
        if !fold_ids.is_empty() {
            per_edge.push((e.entity, pts.clone()));
        }
        for p in pts {
            push_dedup(&mut outline, p);
        }
    }
    let outline = closed_without_repeat(outline);
    if outline.len() < 3
        || negligible_area(contour.area_mm2)
        || negligible_area(shoelace(&outline).abs())
    {
        issues.add(name, DxfIssueKind::DegenerateOutline, 1);
        return None;
    }
    let mut ents: Vec<Entity> = Vec::new();

    // Contour : ligne de coupe dessinée, ligne de couture dessinée, ou tracé tel que dessiné.
    let placed = xf.all(&outline);
    // Contour tel que dessiné : une chaîne fermée ; avec un bord sur le pli, des suites ouvertes
    // qui sautent ce bord.
    let contour_ents = |layer: u32| -> Vec<Entity> {
        if fold_ids.is_empty() {
            return vec![Entity::Polyline {
                layer,
                pts: placed.clone(),
                closed: true,
            }];
        }
        let parts = per_edge
            .iter()
            .map(|(id, pts)| (fold_ids.contains(id), pts.clone()))
            .collect();
        open_runs(parts)
            .into_iter()
            .map(|run| Entity::Polyline {
                layer,
                pts: xf.all(&run),
                closed: false,
            })
            .collect()
    };
    if piece.seam.is_some() {
        // Pièce à marge de couture (US-05.4) : le contour dessiné est la couture (couche 15) et
        // la coupe calculée est écrite sur la couche du contour de pièce (couche 1).
        let notch_layer = layer_of(LayerRole::Notches).number;
        let cut = if mode.includes_cut() {
            match project.piece_cut_contour(piece.id) {
                Ok(cut) => {
                    let notches = project.piece_fold_notches(piece.id).unwrap_or_else(|_| {
                        issues.add(name, DxfIssueKind::FoldNotchesFailed, 1);
                        Vec::new()
                    });
                    Some((cut, notches))
                }
                Err(_) => {
                    issues.add(name, DxfIssueKind::CutContourFailed, 1);
                    None
                }
            }
        } else {
            issues.add(name, DxfIssueKind::NoCutContour, 1);
            None
        };
        // Couture écrite si demandée ; si la coupe seule a échoué, couture écrite à la place
        // (l'omission de la coupe est signalée).
        if mode.includes_sewing() || (mode.includes_cut() && cut.is_none()) {
            ents.extend(contour_ents(layer_of(LayerRole::Sewing).number));
        }
        if let Some((cut, notches)) = cut {
            if cut.fold_cuts.is_empty() {
                let mut pts: Vec<PointMm> = Vec::new();
                for g in cut
                    .traces
                    .iter()
                    .flat_map(pf_geometry::region::trace_geometry)
                {
                    edge_points(&g, CURVE_TOLERANCE_MM, &mut pts);
                }
                ents.push(Entity::Polyline {
                    layer: layer_of(LayerRole::Boundary).number,
                    pts: xf.all(&closed_without_repeat(pts)),
                    closed: true,
                });
            } else {
                // La portion qui suit la pliure n'est pas un bord de coupe (PH-5b).
                let parts = cut
                    .traces
                    .iter()
                    .enumerate()
                    .map(|(i, t)| {
                        let mut pts: Vec<PointMm> = Vec::new();
                        for g in pf_geometry::region::trace_geometry(t) {
                            edge_points(&g, CURVE_TOLERANCE_MM, &mut pts);
                        }
                        (cut.fold_cuts.iter().any(|f| f.trace == i), pts)
                    })
                    .collect();
                for run in open_runs(parts) {
                    ents.push(Entity::Polyline {
                        layer: layer_of(LayerRole::Boundary).number,
                        pts: xf.all(&run),
                        closed: false,
                    });
                }
            }
            // Crans de rempli : petits traits du contour de coupe vers la couture.
            if let Some(seam) = piece.seam.as_ref() {
                for n in &notches {
                    let Some(f) = seam.folds.iter().find(|f| f.entity == n.entity) else {
                        continue;
                    };
                    let len = crate::FOLD_NOTCH_EXPORT_MM.min(f.value_mm);
                    if len.partial_cmp(&1e-9) != Some(std::cmp::Ordering::Greater) {
                        continue;
                    }
                    let foot = n.point + n.normal * f.value_mm;
                    ents.push(Entity::Line {
                        layer: notch_layer,
                        a: xf.p(foot),
                        b: xf.p(foot + n.normal * (-len)),
                    });
                }
            }
            // Crans « sur la coupe » (US-06.1) : projetés sur ce contour, même couche des crans.
            let mut cut_bad = 0;
            for n in sheet.notches.iter().filter(|n| {
                n.on_cut
                    && project
                        .notch_piece(sheet.id, n)
                        .is_some_and(|p| p.id == piece.id)
            }) {
                match project.notch_on_cut_marks(sheet.id, n.id) {
                    Ok(marks) => {
                        for (a, b) in marks {
                            ents.push(Entity::Line {
                                layer: notch_layer,
                                a: xf.p(a),
                                b: xf.p(b),
                            });
                        }
                    }
                    Err(_) => cut_bad += 1,
                }
            }
            issues.add(name, DxfIssueKind::NotchUnreadable, cut_bad);
        }
    } else if piece.net_cut {
        // Coupe nette (US-05.6) : contour de couture = contour de coupe, déclaré ; écrit une
        // seule fois, sur la couche de coupe (1) sauf en « couture seule » (couche 15).
        let layer_no = if mode.includes_cut() {
            layer_of(LayerRole::Boundary).number
        } else {
            issues.add(name, DxfIssueKind::NoCutContour, 1);
            layer_of(LayerRole::Sewing).number
        };
        ents.extend(contour_ents(layer_no));
    } else {
        let all_role = |r: PathRole| roles.iter().all(|x| *x == r);
        let layer_no = if all_role(PathRole::SewingContour) {
            issues.add(name, DxfIssueKind::NoCutContour, 1);
            layer_of(LayerRole::Sewing).number
        } else {
            if !all_role(PathRole::CuttingContour) {
                issues.add(name, DxfIssueKind::BoundaryNotCut, 1);
            }
            layer_of(LayerRole::Boundary).number
        };
        ents.extend(contour_ents(layer_no));
    }
    // Lignes de pliure et libellé (PH-5b) : la ligne suit la pliure de la coupe quand elle est
    // écrite ; le libellé est en anglais, comme le reste du texte du DXF.
    let mut label_rects: Vec<(PointMm, PointMm)> = Vec::new();
    if !fold_edges.is_empty() {
        let fold_layer = layer_of(LayerRole::FoldLine).number;
        let label = pf_i18n::lookup(pf_i18n::Lang::En, "onfold.label");
        for l in project.piece_fold_lines(piece.id, mode.includes_cut()) {
            let (a, b) = (xf.p(l.from), xf.p(l.to));
            ents.push(Entity::Line {
                layer: fold_layer,
                a,
                b,
            });
            let (text, _) = dxf_text(&label);
            let c = crate::fold_label_center(a, b, &text);
            let at = PointMm::new(c.x - text_width_mm(&text) / 2.0, c.y - TEXT_HEIGHT_MM / 2.0);
            label_rects.push(text_rect(at, &text));
            ents.push(Entity::Text {
                layer: fold_layer,
                at,
                text,
            });
        }
    }
    if contour.self_intersecting {
        issues.add(name, DxfIssueKind::SelfIntersecting, 1);
    }

    // Crans posés sur les tracés du contour.
    let notch_layer = layer_of(LayerRole::Notches).number;
    let mut bad_notches = 0;
    for n in sheet
        .notches
        .iter()
        .filter(|n| piece.outline.iter().any(|e| e.entity == n.entity))
    {
        match project.notch_placed_marks(sheet.id, n) {
            // Crans « sur la coupe » d'une pièce à marge : écrits avec le contour de coupe.
            Ok((NotchPlace::Cut, _)) => {}
            Ok((_, marks)) => {
                for (a, b) in marks {
                    ents.push(Entity::Line {
                        layer: notch_layer,
                        a: xf.p(a),
                        b: xf.p(b),
                    });
                }
            }
            Err(_) => bad_notches += 1,
        }
    }
    issues.add(name, DxfIssueKind::NotchUnreadable, bad_notches);

    // Droit-fil : la ligne et la flèche (mêmes pointes que le PDF et le SVG).
    let grain_layer = layer_of(LayerRole::Grainline).number;
    if piece.grainline.is_some() {
        let lines = project.piece_grain_lines(piece.id);
        if lines.is_empty() {
            issues.add(name, DxfIssueKind::GrainUnreadable, 1);
        }
        for l in lines {
            ents.push(Entity::Line {
                layer: grain_layer,
                a: xf.p(l.from),
                b: xf.p(l.to),
            });
            for (a, b) in grain_arrow_marks(l.from, l.to, l.spec.sense) {
                ents.push(Entity::Line {
                    layer: grain_layer,
                    a: xf.p(a),
                    b: xf.p(b),
                });
            }
        }
    } else if !piece.no_grain {
        // Une pièce déclarée sans droit-fil (PH-5d, cuir) n'est pas signalée.
        issues.add(name, DxfIssueKind::MissingGrain, 1);
    }

    // Éléments internes : lignes ouvertes (7), cercles internes (8), repères (7, points).
    let lines_layer = layer_of(LayerRole::InternalLines).number;
    let cutout_layer = layer_of(LayerRole::InternalCutouts).number;
    let mut bad_internal = 0;
    for r in project.piece_internals(piece.id) {
        match (r.element.target, &r.geometry) {
            (
                pf_core::InternalTarget::Path { entity },
                pf_core::piece_internal::InternalGeometry::Path { .. },
            ) => {
                let found = project.sheets.iter().find_map(|s| s.shape(entity).ok());
                match found {
                    Some(Shape::Circle(c)) => ents.push(Entity::Circle {
                        layer: cutout_layer,
                        c: xf.p(c.circle.center),
                        r: c.circle.radius_mm,
                    }),
                    Some(shape) => {
                        let pts = shape_points(&shape, CURVE_TOLERANCE_MM);
                        if pts.len() < 2 {
                            bad_internal += 1;
                        } else {
                            ents.push(Entity::Polyline {
                                layer: lines_layer,
                                pts: xf.all(&pts),
                                closed: false,
                            });
                        }
                    }
                    None => bad_internal += 1,
                }
            }
            (_, pf_core::piece_internal::InternalGeometry::Position { at }) => {
                ents.push(Entity::Point {
                    layer: lines_layer,
                    p: xf.p(*at),
                });
            }
            _ => bad_internal += 1,
        }
    }
    issues.add(name, DxfIssueKind::InternalUnreadable, bad_internal);

    // Perçages, pointages, croix : couche des perçages et marques.
    let drill_layer = layer_of(LayerRole::Drill).number;
    let marks = project.piece_markings(piece.id);
    issues.add(
        name,
        DxfIssueKind::MarkingUnreadable,
        piece.markings.len().saturating_sub(marks.len()),
    );
    for m in marks {
        let at = m.at;
        match m.marking.shape {
            MarkingShape::Punch => ents.push(Entity::Point {
                layer: drill_layer,
                p: xf.p(at),
            }),
            MarkingShape::Drill { diameter_mm } => ents.push(Entity::Circle {
                layer: drill_layer,
                c: xf.p(at),
                r: diameter_mm / 2.0,
            }),
            MarkingShape::Slot { .. } => {
                if let Some(slot) = m.marking.shape.slot(at) {
                    let mut pts = Vec::new();
                    for e in slot.outline() {
                        edge_points(&e, CURVE_TOLERANCE_MM, &mut pts);
                    }
                    ents.push(Entity::Polyline {
                        layer: drill_layer,
                        pts: xf.all(&closed_without_repeat(pts)),
                        closed: true,
                    });
                }
            }
            MarkingShape::Cross { size_mm } => {
                let h = size_mm / 2.0;
                for (a, b) in [
                    (PointMm::new(at.x - h, at.y), PointMm::new(at.x + h, at.y)),
                    (PointMm::new(at.x, at.y - h), PointMm::new(at.x, at.y + h)),
                ] {
                    ents.push(Entity::Line {
                        layer: drill_layer,
                        a: xf.p(a),
                        b: xf.p(b),
                    });
                }
            }
        }
    }

    // Trous des lignes de piqûre (US-06.9) : couche des perçages et marques, cercle exact pour un
    // trou rond, polyligne fermée pour les autres formes.
    for line in project.piece_stitch_lines(piece.id) {
        for h in &line.layout.holes {
            let sh = crate::SceneStitchHole {
                pos: h.position,
                hole: line.hole,
                tangent_deg: h.tangent_deg,
            };
            if let Some(r) = sh.circle_radius() {
                ents.push(Entity::Circle {
                    layer: drill_layer,
                    c: xf.p(sh.pos),
                    r,
                });
            } else if let Some(v) = sh.vertices() {
                ents.push(Entity::Polyline {
                    layer: drill_layer,
                    pts: xf.all(&v),
                    closed: true,
                });
            }
        }
    }

    // Zones (US-06.10) : contour fermé (découpe interne sur la couche des découpes, parage et
    // collage sur celle des marques), motif (traits ou points) et étiquette ASCII sur la couche
    // du texte. Surfaces calculées comme pour l'affichage.
    for z in project.piece_zones(piece.id) {
        let scene_zone = crate::SceneZone {
            kind: z.kind,
            polygon: z.polygon,
            width_mm: z.width_mm,
        };
        let layer = if z.kind == pf_core::ZoneKind::InnerCut {
            cutout_layer
        } else {
            drill_layer
        };
        let pat = scene_zone.pattern();
        ents.push(Entity::Polyline {
            layer,
            pts: xf.all(&scene_zone.polygon),
            closed: true,
        });
        for (a, b) in &pat.hatch {
            ents.push(Entity::Line {
                layer: drill_layer,
                a: xf.p(*a),
                b: xf.p(*b),
            });
        }
        for p in &pat.dots {
            ents.push(Entity::Point {
                layer: drill_layer,
                p: xf.p(*p),
            });
        }
        if let Some(at) = pat.label_at {
            ents.push(Entity::Text {
                layer: layer_of(LayerRole::Text).number,
                at: xf.p(at),
                text: scene_zone.label().to_string(),
            });
        }
    }

    // Boutons (cercle, couche des perçages et marques) et boutonnières (fente fermée, découpe).
    let groups = project.piece_buttons(piece.id);
    issues.add(
        name,
        DxfIssueKind::ButtonsUnreadable,
        piece.buttons.len().saturating_sub(groups.len()),
    );
    for g in groups {
        for el in g.elements {
            match el.shape {
                ButtonShape::Button { diameter_mm } => ents.push(Entity::Circle {
                    layer: drill_layer,
                    c: xf.p(el.center),
                    r: diameter_mm / 2.0,
                }),
                ButtonShape::Buttonhole { .. } => {
                    if let Some(slot) = el.shape.slot(el.center) {
                        let mut pts = Vec::new();
                        for e in slot.outline() {
                            edge_points(&e, CURVE_TOLERANCE_MM, &mut pts);
                        }
                        ents.push(Entity::Polyline {
                            layer: cutout_layer,
                            pts: xf.all(&closed_without_repeat(pts)),
                            closed: true,
                        });
                    }
                }
            }
        }
    }

    // Jambes de pince : sommet → pied gauche et sommet → pied droit, couche dédiée. Une pince non
    // calculable est omise et signalée ; la ligne pliée / le capuchon n'est pas écrit.
    let dart_layer = layer_of(LayerRole::DartLegs).number;
    let mut bad_darts = 0;
    for r in project.piece_darts(piece.id) {
        match r.calc {
            Ok(c) => {
                for foot in [c.shape.left_foot, c.shape.right_foot] {
                    ents.push(Entity::Line {
                        layer: dart_layer,
                        a: xf.p(c.shape.apex),
                        b: xf.p(foot),
                    });
                }
            }
            Err(_) => bad_darts += 1,
        }
    }
    issues.add(name, DxfIssueKind::DartUnreadable, bad_darts);

    // Texte : nom, code, quantité, matière, paire — au centre de la pièce placée.
    let mut lines = vec![format!("Piece Name: {}", piece.name)];
    if let Some(code) = &piece.code {
        lines.push(format!("Code: {code}"));
    }
    lines.push(format!("Quantity: {}", piece.quantity));
    if let Some(m) = &piece.material {
        lines.push(format!("Material: {m}"));
    }
    if piece.mirrored_pair {
        lines.push("Mirrored pair: yes".to_string());
    }
    if let Some(c) = piece.category {
        lines.push(format!("Category: {}", c.key()));
    }
    if let Some(c) = piece.comment.as_deref() {
        lines.extend(
            c.lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .take(crate::LABEL_MAX_COMMENT_LINES)
                .map(|l| format!("Comment: {l}")),
        );
    }
    // Coupe nette (PH-5c) : mention en dernière ligne du bloc de texte.
    if piece.net_cut {
        lines.push(pf_i18n::lookup(pf_i18n::Lang::En, "export.netcut.label"));
    }
    // « Sans droit-fil » (PH-5d) : dernière ligne du bloc de texte.
    if piece.no_grain {
        lines.push(pf_i18n::lookup(pf_i18n::Lang::En, "export.nograin.label"));
    }
    let mut bb = Bbox::from_point(placed[0]);
    placed.iter().for_each(|p| bb.include(*p));
    let (cx, cy) = ((bb.min.x + bb.max.x) / 2.0, (bb.min.y + bb.max.y) / 2.0);
    let n = lines.len() as f64;
    let mut truncated = 0;
    for (k, line) in lines.iter().enumerate() {
        let (text, cut) = dxf_text(line);
        truncated += usize::from(cut);
        let width = text_width_mm(&text);
        ents.push(Entity::Text {
            layer: layer_of(LayerRole::Text).number,
            at: PointMm::new(
                cx - width / 2.0,
                cy + (n - 1.0) * 3.0 - k as f64 * 6.0 - TEXT_HEIGHT_MM / 2.0,
            ),
            text,
        });
    }
    issues.add(name, DxfIssueKind::TextTruncated, truncated);

    // Rempli (PH-5c) : « Fold-over 8 mm » (anglais, comme tout le texte du DXF) au milieu de chaque
    // bord à rempli, côté intérieur, sur la couche du texte de la pièce ; le libellé glisse le long
    // du bord pour ne pas recouvrir le bloc de texte ni un libellé déjà posé.
    let anchors = project.piece_fold_label_anchors(piece.id);
    if !anchors.is_empty() {
        let widest = lines
            .iter()
            .map(|l| text_width_mm(&dxf_text(l).0))
            .fold(0.0, f64::max);
        label_rects.push((
            PointMm::new(cx - widest / 2.0, cy - n * 3.0),
            PointMm::new(cx + widest / 2.0, cy + n * 3.0),
        ));
        for a in anchors {
            let at = xf.p(a.point);
            let out = xf.p(a.point + a.normal) - at;
            let outward = out.unit().unwrap_or(out);
            let (text, _) = dxf_text(&crate::fold_over_label_text(a.value_mm, pf_i18n::Lang::En));
            let (on_edge, inward) =
                crate::place_edge_label(at, outward, a.length_mm, &text, &label_rects);
            let c = crate::edge_label_center(on_edge, inward, &text);
            let at = PointMm::new(c.x - text_width_mm(&text) / 2.0, c.y - TEXT_HEIGHT_MM / 2.0);
            label_rects.push(text_rect(at, &text));
            ents.push(Entity::Text {
                layer: layer_of(LayerRole::Text).number,
                at,
                text,
            });
        }
    }

    Some(Block {
        name: block_name,
        entities: ents,
    })
}

// ----- écriture --------------------------------------------------------------------------------

pub(crate) struct Writer {
    pub(crate) s: String,
    pub(crate) non_finite: bool,
    pub(crate) out_of_range: bool,
}

impl Writer {
    pub(crate) fn pair(&mut self, code: i32, value: &str) {
        let _ = write!(self.s, "{code:>3}\n{value}\n");
    }

    pub(crate) fn int(&mut self, code: i32, v: i64) {
        let _ = write!(self.s, "{code:>3}\n{v}\n");
    }

    fn real(&mut self, code: i32, v: f64) {
        self.real_dec(code, v, DECIMALS);
    }

    /// Nombre réel à `decimals` décimales (l'export de construction en écrit davantage).
    pub(crate) fn real_dec(&mut self, code: i32, v: f64, decimals: usize) {
        let text = if !v.is_finite() {
            self.non_finite = true;
            format!("{:.*}", decimals, 0.0)
        } else if v.abs() > MAX_ABS_MM {
            self.out_of_range = true;
            format!("{:.*}", decimals, 0.0)
        } else {
            let t = format!("{v:.decimals$}");
            if t.starts_with('-') && t[1..].bytes().all(|b| b == b'0' || b == b'.') {
                t[1..].to_string()
            } else {
                t
            }
        };
        let _ = write!(self.s, "{code:>3}\n{text}\n");
    }

    pub(crate) fn point(&mut self, base: i32, p: PointMm) {
        self.real(base, p.x);
        self.real(base + 10, p.y);
        self.real(base + 20, 0.0);
    }

    /// Début d'entité sur une couche **nommée** (export de construction).
    pub(crate) fn begin_named(&mut self, kind: &str, layer: &str) {
        self.pair(0, kind);
        self.pair(8, layer);
    }

    fn begin(&mut self, kind: &str, layer: u32) {
        self.pair(0, kind);
        self.pair(8, &layer.to_string());
    }

    fn entity(&mut self, e: &Entity) {
        match e {
            Entity::Line { layer, a, b } => {
                self.begin("LINE", *layer);
                self.point(10, *a);
                self.point(11, *b);
            }
            Entity::Point { layer, p } => {
                self.begin("POINT", *layer);
                self.point(10, *p);
            }
            Entity::Circle { layer, c, r } => {
                self.begin("CIRCLE", *layer);
                self.point(10, *c);
                self.real(40, *r);
            }
            Entity::Polyline { layer, pts, closed } => {
                self.begin("POLYLINE", *layer);
                self.int(66, 1);
                self.point(10, PointMm::new(0.0, 0.0));
                self.int(70, i64::from(*closed));
                for p in pts {
                    self.begin("VERTEX", *layer);
                    self.point(10, *p);
                }
                self.begin("SEQEND", *layer);
            }
            Entity::Text { layer, at, text } => {
                self.begin("TEXT", *layer);
                self.point(10, *at);
                self.real(40, TEXT_HEIGHT_MM);
                self.pair(1, text);
            }
        }
    }
}

/// Exporte toutes les pièces du projet en un fichier DXF (taille unique, lecture seule).
///
/// Erreurs : [`DxfError::NoPieces`] sans pièce, [`DxfError::NothingExported`] si aucune pièce
/// n'est écrivable. Rien n'est écrit sur disque ici : l'appelant écrit [`DxfExport::bytes`].
pub fn export_dxf(project: &Project) -> Result<DxfExport, DxfError> {
    export_dxf_with(project, PieceExportMode::default())
}

/// Comme [`export_dxf`], avec le mode d'export des pièces à marge de couture (US-05.4) :
/// couche 15 = couture (contour dessiné), couche 1 = coupe calculée, couche 4 = crans de rempli.
/// Une pièce sans marge est écrite comme avant, quel que soit le mode.
pub fn export_dxf_with(project: &Project, mode: PieceExportMode) -> Result<DxfExport, DxfError> {
    project.validate()?;
    if project.pieces.is_empty() {
        return Err(DxfError::NoPieces);
    }
    let layout = project.plan_layout();
    let mut issues = Collector { issues: Vec::new() };
    let mut blocks: Vec<Block> = Vec::new();
    let mut infos: Vec<DxfPieceInfo> = Vec::new();
    for piece in &project.pieces {
        let Some(item) = layout.iter().find(|i| i.piece == piece.id) else {
            issues.add(&piece.name, DxfIssueKind::OutlineUnreadable, 1);
            continue;
        };
        let block_name = format!("PIECE_{}", piece.id.0);
        if let Some(b) = build_piece(project, piece, item, block_name.clone(), &mut issues, mode) {
            infos.push(DxfPieceInfo {
                id: piece.id,
                name: piece.name.clone(),
                block: block_name,
                entities: b.entities.len(),
            });
            blocks.push(b);
        }
    }
    if blocks.is_empty() {
        return Err(DxfError::NothingExported(issues.issues));
    }

    let mut bb: Option<Bbox> = None;
    let mut counts: BTreeMap<u32, usize> = BTreeMap::new();
    for e in blocks.iter().flat_map(|b| &b.entities) {
        e.include_in(&mut bb);
        *counts.entry(e.layer()).or_default() += 1;
    }
    let bb = bb.expect("au moins une entité");

    let mut w = Writer {
        s: String::new(),
        non_finite: false,
        out_of_range: false,
    };
    // HEADER
    w.pair(0, "SECTION");
    w.pair(2, "HEADER");
    w.pair(9, "$ACADVER");
    w.pair(1, DXF_VERSION);
    w.pair(9, "$INSBASE");
    w.point(10, PointMm::new(0.0, 0.0));
    w.pair(9, "$EXTMIN");
    w.point(10, bb.min);
    w.pair(9, "$EXTMAX");
    w.point(10, bb.max);
    w.pair(9, "$INSUNITS");
    w.int(70, INSUNITS_MM);
    w.pair(9, "$MEASUREMENT");
    w.int(70, MEASUREMENT_METRIC);
    w.pair(0, "ENDSEC");
    // TABLES
    w.pair(0, "SECTION");
    w.pair(2, "TABLES");
    w.pair(0, "TABLE");
    w.pair(2, "LTYPE");
    w.int(70, 1);
    w.pair(0, "LTYPE");
    w.pair(2, "CONTINUOUS");
    w.int(70, 0);
    w.pair(3, "Solid line");
    w.int(72, 65);
    w.int(73, 0);
    w.real(40, 0.0);
    w.pair(0, "ENDTAB");
    w.pair(0, "TABLE");
    w.pair(2, "LAYER");
    // La couche des jambes de pince n'est déclarée que si elle porte une entité.
    let declared: Vec<&DxfLayer> = LAYERS
        .iter()
        .filter(|l| {
            !matches!(l.role, LayerRole::DartLegs | LayerRole::FoldLine)
                || counts.contains_key(&l.number)
        })
        .collect();
    w.int(70, declared.len() as i64 + 1);
    let layer_row = |w: &mut Writer, name: &str, color: u16| {
        w.pair(0, "LAYER");
        w.pair(2, name);
        w.int(70, 0);
        w.int(62, i64::from(color));
        w.pair(6, "CONTINUOUS");
    };
    layer_row(&mut w, "0", 7);
    for l in declared {
        layer_row(&mut w, &l.number.to_string(), l.color);
    }
    w.pair(0, "ENDTAB");
    w.pair(0, "ENDSEC");
    // BLOCKS
    w.pair(0, "SECTION");
    w.pair(2, "BLOCKS");
    for b in &blocks {
        w.pair(0, "BLOCK");
        w.pair(8, "0");
        w.pair(2, &b.name);
        w.int(70, 0);
        w.point(10, PointMm::new(0.0, 0.0));
        w.pair(3, &b.name);
        for e in &b.entities {
            w.entity(e);
        }
        w.pair(0, "ENDBLK");
        w.pair(8, "0");
    }
    w.pair(0, "ENDSEC");
    // ENTITIES
    w.pair(0, "SECTION");
    w.pair(2, "ENTITIES");
    for b in &blocks {
        w.pair(0, "INSERT");
        w.pair(8, "0");
        w.pair(2, &b.name);
        w.point(10, PointMm::new(0.0, 0.0));
    }
    w.pair(0, "ENDSEC");
    w.pair(0, "EOF");

    if w.non_finite {
        return Err(DxfError::NonFinite);
    }
    if w.out_of_range {
        return Err(DxfError::OutOfRange);
    }
    let layers = LAYERS
        .iter()
        .filter_map(|l| {
            counts
                .get(&l.number)
                .map(|&entities| DxfLayerUse { layer: l, entities })
        })
        .collect();
    Ok(DxfExport {
        text: w.s,
        report: DxfReport {
            pieces: infos,
            layers,
            issues: issues.issues,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn writer() -> Writer {
        Writer {
            s: String::new(),
            non_finite: false,
            out_of_range: false,
        }
    }

    #[test]
    fn nombre_non_fini_ou_hors_bornes_est_signale_jamais_ecrit() {
        for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let mut w = writer();
            w.real(10, bad);
            assert!(w.non_finite && !w.out_of_range);
            assert!(!w.s.to_lowercase().contains("nan") && !w.s.to_lowercase().contains("inf"));
        }
        let mut w = writer();
        w.real(10, 2.0 * MAX_ABS_MM);
        assert!(w.out_of_range && !w.non_finite);
    }

    #[test]
    fn nombres_en_notation_fixe_sans_zero_negatif() {
        let mut w = writer();
        w.real(10, -0.000_000_1);
        w.real(20, 1.5e-7);
        w.real(30, -12.345_678_9);
        w.real(40, 1234.0);
        assert_eq!(
            w.s,
            " 10\n0.00000\n 20\n0.00000\n 30\n-12.34568\n 40\n1234.00000\n"
        );
    }

    #[test]
    fn texte_ascii_pur_echappe_et_tronque() {
        assert_eq!(dxf_text("Devant é").0, "Devant \\U+00E9");
        assert_eq!(dxf_text("a\\b\nc").0, "a\\U+005Cb c");
        // Hors plan de base : paire de substituts, jamais de caractère non ASCII.
        let (t, _) = dxf_text("x\u{1F600}");
        assert_eq!(t, "x\\U+D83D\\U+DE00");
        assert!(t.is_ascii());
        let (long, cut) = dxf_text(&"n".repeat(TEXT_MAX_CHARS + 5));
        assert!(cut && long.len() == TEXT_MAX_CHARS);
        assert!(!dxf_text("court").1);
    }

    #[test]
    fn discretisation_fleche_bornee() {
        // Quart de cercle de rayon 100 mm en Bézier.
        let r = 100.0;
        let k = 0.552_284_749_830_793_4 * r;
        let c = CubicBezier::new(
            PointMm::new(r, 0.0),
            PointMm::new(r, k),
            PointMm::new(k, r),
            PointMm::new(0.0, r),
        );
        let mut pts = Vec::new();
        cubic_points(&c, CURVE_TOLERANCE_MM, &mut pts);
        assert!(pts.len() > 50);
        // Sommets sur la courbe, longueur de la polyligne = longueur développée à 0,005 mm.
        let dense = c.sample(20_000);
        for p in &pts {
            let d = dense
                .windows(2)
                .map(|w| {
                    let (u, v) = (w[1] - w[0], *p - w[0]);
                    let t = (v.dot(u) / u.dot(u)).clamp(0.0, 1.0);
                    pf_geometry::distance_mm(*p, w[0] + u * t)
                })
                .fold(f64::INFINITY, f64::min);
            assert!(d < 0.001, "sommet à {d} mm de la courbe");
        }
        let len: f64 = pts
            .windows(2)
            .map(|w| pf_geometry::distance_mm(w[0], w[1]))
            .sum();
        let exact = c.length_mm(1e-9).unwrap();
        assert!((len - exact).abs() < 0.005, "{len} contre {exact}");
    }

    #[test]
    fn arc_respecte_la_fleche_et_petit_rayon_ne_boucle_pas() {
        let a = Arc {
            center: PointMm::new(0.0, 0.0),
            radius_mm: 50.0,
            start_rad: 0.3,
            sweep_rad: 2.0,
        };
        let mut pts = Vec::new();
        arc_points(&a, CURVE_TOLERANCE_MM, &mut pts);
        assert!(pts.len() > 10);
        for w in pts.windows(2) {
            let mid = PointMm::new((w[0].x + w[1].x) / 2.0, (w[0].y + w[1].y) / 2.0);
            let sagitta = 50.0 - mid.x.hypot(mid.y);
            assert!(sagitta <= CURVE_TOLERANCE_MM * 1.0001, "flèche {sagitta}");
        }
        assert!(arc_steps(0.0005, 3.0, CURVE_TOLERANCE_MM) >= 1);
    }
}
