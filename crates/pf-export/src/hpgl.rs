//! Export HPGL / PLT pour traceur (US-12.5).
//!
//! **Honnêteté** : fichier texte ASCII « de type HPGL », limité aux instructions `IN`, `PA`, `SP`,
//! `PU`, `PD`. Il n'a **pas** été vérifié sur un vrai traceur ni dans un logiciel de découpe ; la
//! fidélité de l'échelle sur machine est une étape humaine (voir `docs/STATUS.md`). Aucune
//! compatibilité n'est annoncée.
//!
//! Choix de format (voir `docs/DECISIONS.md`) :
//!
//! - Unités de traçage : 1 unité = 0,025 mm (40 unités par mm), échelle 1:1, coordonnées
//!   entières **arrondies au plus proche** : l'erreur d'arrondi d'un sommet est d'au plus
//!   0,0125 mm par axe. Origine `(0 ; 0)` = coin bas-gauche de la zone ; le dessin y est calé par
//!   le coin bas-gauche de son emprise (les marges de la mise en page ne décalent rien).
//! - Courbes discrétisées en polylignes (flèche au plus [`CURVE_TOLERANCE_MM`] ≤ 0,01 mm, mêmes
//!   fonctions que le DXF). Les textes (cotes, noms de points) et les repères de points ne sont
//!   **pas** tracés (comptés dans [`HpglReport`]) ; les pointages sont tracés par leur cercle.
//! - Plumes : table unique [`HpglPen`] (rôle → plume 1 à 8) ; seules les plumes utilisées sont
//!   sélectionnées (`SPn;`), une seule fois chacune, par numéro croissant. Aucune pause de changement
//!   de plume n'est écrite ; pas de `LT` (le type de trait n'est pas porté par le fichier).
//! - Ordre de tracé : tri par plume puis plus proche voisin depuis l'origine (départage stable ;
//!   polyligne ouverte parcourue dans le sens le plus court ; contour fermé commencé à son sommet
//!   le plus proche). L'ordre optimisé n'est jamais plus long à vide que l'ordre naturel (scène
//!   triée par plume) : le plus court des deux est retenu.
//! - Format de page (US-13.1) : [`export_hpgl`] écrit une seule sortie, jamais découpée en pages.
//!   Feuille fixe : le tracé doit tenir dans la zone imprimable, sinon refus (rouleau, format plus
//!   grand ou [`export_hpgl_pages`]) ; rouleau : largeur imprimable, longueur = emprise.
//!   **Pas de rotation de 90°** : l'orientation paysage de la mise en page échange les côtés de la
//!   zone, le tracé n'est jamais tourné.
//! - HPGL tuilé (PH-5e) : [`export_hpgl_pages`] écrit **un fichier par page**, avec le découpage
//!   exact du PDF (`pdf::plan_tiles_unrotated` : mêmes fenêtres, recouvrement, ordre, numéros ; le
//!   seul écart est l'absence de rotation). Voir la section « Tuilage » plus bas.
//! - Facteurs X / Y de calibration (US-13.3) : le tracé est multiplié par chaque facteur, comme la
//!   géométrie du PDF, avant le calcul de l'emprise et le contrôle de la zone.
//!
//! Lecture seule : la scène n'est jamais modifiée.

use crate::calibration::PrintScale;
use crate::dxf::{arc_points, arc_steps, cubic_points, edge_points, push_dedup};
use crate::page::{PageSetup, PageSetupError, ROLL_LENGTH_MAX_MM};
use crate::pdf::{plan_tiles_unrotated, registration_marks, Tile, MARK_ARM_MM};
use crate::{grain_arrow_marks, ExportError, PathGeom, Scene};
use pf_core::PathRole;
use pf_geometry::{CircleTrace, CubicBezier, PointMm};
use std::fmt::{self, Write as _};

/// Unités de traçage par millimètre (1 unité = 0,025 mm).
pub const UNITS_PER_MM: f64 = 40.0;
/// Erreur d'arrondi maximale d'un sommet sur un axe (mm) : une demi-unité.
pub const ROUNDING_MM: f64 = 0.0125;
/// Flèche maximale (mm) de la discrétisation des courbes.
pub const CURVE_TOLERANCE_MM: f64 = 0.005;
/// Plus grande coordonnée écrite (unités) : borne prudente, bien en deçà de l'entier 32 bits.
pub const MAX_COORD_UNITS: i64 = 8_388_607;
/// Nombre maximal de points d'une instruction `PD` (les suivantes continuent plume baissée).
pub const MAX_PD_POINTS: usize = 32;

/// Plume d'un type de trait : table unique rôle → plume (1 à 8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum HpglPen {
    /// 1 : contour de coupe.
    Cutting,
    /// 2 : contour de couture.
    Sewing,
    /// 3 : construction.
    Construction,
    /// 4 : droit-fil et ses flèches.
    Grainline,
    /// 5 : crans.
    Notches,
    /// 6 : marquages, boutons et boutonnières.
    Marks,
    /// 7 : cotations et cotes complètes (traits) et étiquettes de pièce (texte `LB`).
    Dimensions,
    /// 8 : pliure, lignes spéciales et axes.
    Other,
    /// 9 : jambes des pinces.
    Darts,
}

impl HpglPen {
    pub const ALL: [HpglPen; 9] = [
        Self::Cutting,
        Self::Sewing,
        Self::Construction,
        Self::Grainline,
        Self::Notches,
        Self::Marks,
        Self::Dimensions,
        Self::Other,
        Self::Darts,
    ];

    /// Numéro de plume écrit dans `SPn;`.
    pub fn number(self) -> u8 {
        self as u8 + 1
    }

    /// Identifiant stable (clés d'interface).
    pub fn key(self) -> &'static str {
        match self {
            Self::Cutting => "cutting",
            Self::Sewing => "sewing",
            Self::Construction => "construction",
            Self::Grainline => "grainline",
            Self::Notches => "notches",
            Self::Marks => "marks",
            Self::Dimensions => "dimensions",
            Self::Other => "other",
            Self::Darts => "darts",
        }
    }

    /// Plume d'un tracé selon son rôle (les 14 rôles sont couverts).
    pub fn of_role(role: PathRole) -> HpglPen {
        match role {
            PathRole::CuttingContour => Self::Cutting,
            PathRole::SewingContour => Self::Sewing,
            PathRole::Construction => Self::Construction,
            PathRole::Grainline => Self::Grainline,
            PathRole::AnnotationAxis
            | PathRole::Fold
            | PathRole::Topstitch
            | PathRole::CenterFront
            | PathRole::CenterBack
            | PathRole::WaistLine
            | PathRole::HipLine
            | PathRole::BustLine
            | PathRole::SkivingLine
            | PathRole::GlueLine => Self::Other,
        }
    }
}

/// Refus de l'export : aucun fichier n'est produit.
#[derive(Debug, Clone, PartialEq)]
pub enum HpglError {
    /// Rien de traçable (scène sans trait, ou traits réduits à moins d'une unité).
    Nothing,
    /// Une coordonnée n'est pas un nombre fini.
    NotFinite,
    /// Le dessin dépasse la plage des coordonnées entières écrites.
    OutOfRange { extent_mm: f64 },
    /// Réglage de page invalide, ou contenu plus large que le rouleau.
    Page(PageSetupError),
    /// Le tracé ne tient pas dans la zone imprimable d'une feuille fixe.
    SheetTooSmall {
        content_w_mm: f64,
        content_h_mm: f64,
        zone_w_mm: f64,
        zone_h_mm: f64,
    },
    /// Découpage en pages impossible (trop de pages, feuille vide : mêmes refus que le PDF).
    Tiling(ExportError),
}

impl fmt::Display for HpglError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Nothing => write!(f, "aucun trait à tracer : export HPGL refusé"),
            Self::NotFinite => write!(
                f,
                "une coordonnée n'est pas un nombre fini : export HPGL refusé"
            ),
            Self::OutOfRange { extent_mm } => write!(
                f,
                "le dessin ({} mm) dépasse la plage des coordonnées HPGL : export refusé",
                crate::num(*extent_mm)
            ),
            Self::Page(e) => write!(f, "{e}"),
            Self::SheetTooSmall {
                content_w_mm,
                content_h_mm,
                zone_w_mm,
                zone_h_mm,
            } => write!(
                f,
                "le tracé fait {} × {} mm, la zone de la feuille n'offre que {} × {} mm : choisir \
                 un rouleau ou un format plus grand",
                crate::num(*content_w_mm),
                crate::num(*content_h_mm),
                crate::num(*zone_w_mm),
                crate::num(*zone_h_mm)
            ),
            Self::Tiling(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for HpglError {}

impl From<PageSetupError> for HpglError {
    fn from(e: PageSetupError) -> Self {
        HpglError::Page(e)
    }
}

/// Options de l'export.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HpglOptions {
    /// Tri par plume puis plus proche voisin (vrai par défaut) ; faux = ordre naturel de la
    /// scène, trié par plume.
    pub optimize: bool,
}

impl Default for HpglOptions {
    fn default() -> Self {
        HpglOptions { optimize: true }
    }
}

/// Usage d'une plume.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PenUse {
    pub pen: HpglPen,
    /// Nombre de tracés (plume baissée) faits avec cette plume.
    pub strokes: usize,
    /// Longueur tracée plume baissée (mm), lue sur les coordonnées entières écrites.
    pub length_mm: f64,
}

/// Rapport affiché avant l'écriture.
#[derive(Debug, Clone, PartialEq)]
pub struct HpglReport {
    /// Plumes réellement utilisées, par numéro croissant.
    pub pens: Vec<PenUse>,
    pub strokes: usize,
    /// Taille du tracé (mm), après facteurs X / Y.
    pub width_mm: f64,
    pub height_mm: f64,
    /// Zone imprimable : largeur et hauteur (`None` pour un rouleau).
    pub zone_width_mm: f64,
    pub zone_height_mm: Option<f64>,
    pub pen_down_mm: f64,
    /// Déplacements plume levée de l'ordre écrit (mm), depuis l'origine.
    pub travel_mm: f64,
    /// Déplacements plume levée de l'ordre naturel (mm).
    pub natural_travel_mm: f64,
    /// Textes non tracés (cotes, noms de points) et repères de points non tracés.
    pub texts_omitted: usize,
    /// Lignes d'étiquette de pièce écrites (`LB`, plume 7).
    pub labels: usize,
    pub points_omitted: usize,
    /// Traits réduits à moins d'une unité, non écrits.
    pub degenerate_dropped: usize,
    pub scale: PrintScale,
}

/// Fichier HPGL et son rapport.
#[derive(Debug, Clone, PartialEq)]
pub struct HpglPlot {
    pub text: String,
    pub report: HpglReport,
}

impl HpglPlot {
    pub fn bytes(&self) -> &[u8] {
        self.text.as_bytes()
    }
}

type Pt = (i64, i64);

#[derive(Debug, Clone)]
struct IStroke {
    pen: HpglPen,
    pts: Vec<Pt>,
    closed: bool,
}

fn finite_pts(ps: &[PointMm]) -> bool {
    ps.iter().all(|p| p.x.is_finite() && p.y.is_finite())
}

fn cubic_finite(c: &CubicBezier) -> bool {
    finite_pts(&[c.p0, c.c1, c.c2, c.p3])
}

/// Vrai si tous les nombres du tracé sont finis (sinon la discrétisation les avalerait).
fn geom_finite(g: &PathGeom) -> bool {
    match g {
        PathGeom::Line(a, b) => finite_pts(&[*a, *b]),
        PathGeom::Cubic(c) => cubic_finite(c),
        PathGeom::Arc(a) => {
            finite_pts(&[a.center])
                && a.radius_mm.is_finite()
                && a.start_rad.is_finite()
                && a.sweep_rad.is_finite()
        }
        PathGeom::Circle(c) => {
            finite_pts(&[c.circle.center])
                && c.circle.radius_mm.is_finite()
                && c.start_deg.is_finite()
        }
        PathGeom::Spline(s) => s.iter().all(cubic_finite),
    }
}

fn circle_points(c: &CircleTrace) -> Vec<PointMm> {
    let mut out = Vec::new();
    let n = arc_steps(
        c.circle.radius_mm,
        std::f64::consts::TAU,
        CURVE_TOLERANCE_MM,
    )
    .max(8);
    for p in c.sample(n) {
        push_dedup(&mut out, p);
    }
    out
}

fn geom_points(g: &PathGeom) -> Vec<PointMm> {
    let tol = CURVE_TOLERANCE_MM;
    let mut out = Vec::new();
    match g {
        PathGeom::Line(a, b) => {
            push_dedup(&mut out, *a);
            push_dedup(&mut out, *b);
        }
        PathGeom::Cubic(c) => cubic_points(c, tol, &mut out),
        PathGeom::Arc(a) => arc_points(a, tol, &mut out),
        PathGeom::Circle(c) => return circle_points(c),
        PathGeom::Spline(s) => {
            for c in s {
                cubic_points(c, tol, &mut out);
            }
        }
    }
    out
}

type Stroke = (HpglPen, Vec<PointMm>);

fn push_segment(
    strokes: &mut Vec<Stroke>,
    pen: HpglPen,
    a: PointMm,
    b: PointMm,
) -> Result<(), HpglError> {
    if !finite_pts(&[a, b]) {
        return Err(HpglError::NotFinite);
    }
    strokes.push((pen, vec![a, b]));
    Ok(())
}

/// Traits de la scène en mm (avant facteurs), avec leur plume : d'abord les tracés (ordre de la
/// scène), puis crans, cotes, marquages, boutons.
fn collect(scene: &Scene) -> Result<Vec<Stroke>, HpglError> {
    let mut strokes: Vec<Stroke> = Vec::new();
    for p in &scene.paths {
        if !geom_finite(&p.geom) {
            return Err(HpglError::NotFinite);
        }
        let pen = HpglPen::of_role(p.role);
        strokes.push((pen, geom_points(&p.geom)));
        if let (PathRole::Grainline, &PathGeom::Line(a, b)) = (p.role, &p.geom) {
            for (q, r) in grain_arrow_marks(a, b, p.sense) {
                push_segment(&mut strokes, pen, q, r)?;
            }
        }
    }
    // Contours de coupe calculés (US-05.4) : plume « coupe » ; les crans de rempli, plume crans.
    for cut in &scene.cut_contours {
        let mut pts: Vec<PointMm> = Vec::new();
        for g in &cut.edges {
            if !geom_finite(g) {
                return Err(HpglError::NotFinite);
            }
            for p in geom_points(g) {
                push_dedup(&mut pts, p);
            }
        }
        if pts.len() >= 2 {
            strokes.push((HpglPen::Cutting, pts));
        }
        for (a, b) in cut.notch_marks.iter().chain(&cut.cut_notch_marks) {
            push_segment(&mut strokes, HpglPen::Notches, *a, *b)?;
        }
    }
    // Bords sur le pli (PH-5b) : plume « autres » (pliure, lignes spéciales), jamais la coupe.
    for f in &scene.folds {
        push_segment(&mut strokes, HpglPen::Other, f.from, f.to)?;
    }
    for (a, b) in &scene.notch_marks {
        push_segment(&mut strokes, HpglPen::Notches, *a, *b)?;
    }
    for d in &scene.dimensions {
        for (a, b) in d.marks() {
            push_segment(&mut strokes, HpglPen::Dimensions, a, b)?;
        }
    }
    for d in &scene.full_dimensions {
        for (a, b) in &d.marks {
            push_segment(&mut strokes, HpglPen::Dimensions, *a, *b)?;
        }
    }
    for m in &scene.markings {
        if !finite_pts(&[m.pos]) || !m.half_extent_mm().is_finite() {
            return Err(HpglError::NotFinite);
        }
        for (a, b) in m.cross_marks() {
            push_segment(&mut strokes, HpglPen::Marks, a, b)?;
        }
        if let Some((r, _)) = m.circle() {
            if let Ok(c) = CircleTrace::new(m.pos, r, 0.0, true) {
                strokes.push((HpglPen::Marks, circle_points(&c)));
            }
        }
        if let Some(slot) = m.slot() {
            let mut pts = Vec::new();
            for e in slot.outline() {
                edge_points(&e, CURVE_TOLERANCE_MM, &mut pts);
            }
            strokes.push((HpglPen::Marks, pts));
        }
    }
    for m in &scene.stitch_holes {
        if !finite_pts(&[m.pos]) || !m.half_extent_mm().is_finite() {
            return Err(HpglError::NotFinite);
        }
        if let Some(r) = m.circle_radius() {
            if let Ok(c) = CircleTrace::new(m.pos, r, 0.0, true) {
                strokes.push((HpglPen::Marks, circle_points(&c)));
            }
        } else if let Some(v) = m.vertices() {
            let mut pts = v.to_vec();
            pts.push(v[0]);
            strokes.push((HpglPen::Marks, pts));
        }
    }
    // Zones (US-06.10) : contour fermé, hachures et points (traits de 0,3 mm), plume des marquages.
    for z in &scene.zones {
        if !finite_pts(&z.polygon) {
            return Err(HpglError::NotFinite);
        }
        if let Some(first) = z.polygon.first() {
            let mut pts = z.polygon.clone();
            pts.push(*first);
            strokes.push((HpglPen::Marks, pts));
        }
        let pat = z.pattern();
        for (a, b) in &pat.hatch {
            push_segment(&mut strokes, HpglPen::Marks, *a, *b)?;
        }
        for p in &pat.dots {
            push_segment(
                &mut strokes,
                HpglPen::Marks,
                *p,
                PointMm::new(p.x + 0.3, p.y),
            )?;
        }
    }
    for d in &scene.darts {
        if !finite_pts(&[d.apex, d.left_foot, d.right_foot]) {
            return Err(HpglError::NotFinite);
        }
    }
    for b in &scene.buttons {
        let (hx, hy) = b.half_extent_mm();
        if !finite_pts(&[b.pos]) || !hx.is_finite() || !hy.is_finite() {
            return Err(HpglError::NotFinite);
        }
        if let Some(r) = b.circle_radius() {
            if let Ok(c) = CircleTrace::new(b.pos, r, 0.0, true) {
                strokes.push((HpglPen::Marks, circle_points(&c)));
            }
        }
        if let Some(slot) = b.slot() {
            let mut pts = Vec::new();
            for e in slot.outline() {
                edge_points(&e, CURVE_TOLERANCE_MM, &mut pts);
            }
            strokes.push((HpglPen::Marks, pts));
        }
    }
    for d in &scene.darts {
        for (a, b) in d.legs() {
            push_segment(&mut strokes, HpglPen::Darts, a, b)?;
        }
    }
    Ok(strokes)
}

fn d2(a: Pt, b: Pt) -> i64 {
    let (dx, dy) = (a.0 - b.0, a.1 - b.1);
    dx * dx + dy * dy
}

fn dist_units(a: Pt, b: Pt) -> f64 {
    (d2(a, b) as f64).sqrt()
}

/// Déplacement plume levée (unités) d'un ordre de tracés, depuis l'origine.
fn travel_units(order: &[IStroke]) -> f64 {
    let mut pos = (0, 0);
    let mut total = 0.0;
    for s in order {
        total += dist_units(pos, s.pts[0]);
        pos = s.pts[s.pts.len() - 1];
    }
    total
}

/// Contour fermé (premier sommet = dernier) recommencé au sommet `k`.
fn rotate_closed(pts: &[Pt], k: usize) -> Vec<Pt> {
    let n = pts.len() - 1;
    let mut out: Vec<Pt> = (0..n).map(|i| pts[(k + i) % n]).collect();
    out.push(pts[k]);
    out
}

/// Plus proche voisin au sein d'une plume ; `pos` passe à la fin du dernier tracé.
fn nearest_order(group: Vec<IStroke>, pos: &mut Pt) -> Vec<IStroke> {
    let mut rest = group;
    let mut out = Vec::with_capacity(rest.len());
    while !rest.is_empty() {
        // (distance², tracé, sommet de départ, sens inversé) ; `<` strict = premier gagnant.
        let mut best: (i64, usize, usize, bool) = (i64::MAX, 0, 0, false);
        for (i, s) in rest.iter().enumerate() {
            if s.closed {
                for (k, p) in s.pts[..s.pts.len() - 1].iter().enumerate() {
                    let d = d2(*pos, *p);
                    if d < best.0 {
                        best = (d, i, k, false);
                    }
                }
            } else {
                let d0 = d2(*pos, s.pts[0]);
                if d0 < best.0 {
                    best = (d0, i, 0, false);
                }
                let d1 = d2(*pos, s.pts[s.pts.len() - 1]);
                if d1 < best.0 {
                    best = (d1, i, 0, true);
                }
            }
        }
        let mut s = rest.remove(best.1);
        if s.closed {
            s.pts = rotate_closed(&s.pts, best.2);
        } else if best.3 {
            s.pts.reverse();
        }
        *pos = s.pts[s.pts.len() - 1];
        out.push(s);
    }
    out
}

/// Contrôle de la zone de traçage selon le format de page (US-13.1). Renvoie la zone.
fn check_fit(page: &PageSetup, w_mm: f64, h_mm: f64) -> Result<(f64, Option<f64>), HpglError> {
    let zone_w = page.printable_width_mm();
    match page.printable_height_mm() {
        None => {
            if w_mm > zone_w + 1e-9 {
                return Err(PageSetupError::ContentTooWide {
                    content_mm: w_mm,
                    printable_mm: zone_w,
                }
                .into());
            }
            if h_mm > ROLL_LENGTH_MAX_MM {
                return Err(PageSetupError::RollTooLong { length_mm: h_mm }.into());
            }
            Ok((zone_w, None))
        }
        Some(zone_h) => {
            if w_mm > zone_w + 1e-9 || h_mm > zone_h + 1e-9 {
                return Err(HpglError::SheetTooSmall {
                    content_w_mm: w_mm,
                    content_h_mm: h_mm,
                    zone_w_mm: zone_w,
                    zone_h_mm: zone_h,
                });
            }
            Ok((zone_w, Some(zone_h)))
        }
    }
}

/// Plume des étiquettes de pièce (texte) : la plume des annotations.
const LABEL_PEN: HpglPen = HpglPen::Dimensions;

/// Texte `LB` : ASCII pur (accents translittérés, `×` → `x`, `;` → `,`, contrôles → espace, tout
/// autre caractère non ASCII → `?`), pour que le fichier reste du HPGL ASCII sans jeu de
/// caractères à choisir.
fn hpgl_text(s: &str) -> String {
    let mut out = String::new();
    for ch in s.chars() {
        match ch {
            ';' => out.push(','),
            c if c.is_control() => out.push(' '),
            c if c.is_ascii() => out.push(c),
            'à' | 'â' | 'ä' | 'á' => out.push('a'),
            'À' | 'Â' | 'Ä' | 'Á' => out.push('A'),
            'ç' => out.push('c'),
            'Ç' => out.push('C'),
            'é' | 'è' | 'ê' | 'ë' => out.push('e'),
            'É' | 'È' | 'Ê' | 'Ë' => out.push('E'),
            'î' | 'ï' | 'í' => out.push('i'),
            'Î' | 'Ï' | 'Í' => out.push('I'),
            'ô' | 'ö' | 'ó' => out.push('o'),
            'Ô' | 'Ö' | 'Ó' => out.push('O'),
            'ù' | 'û' | 'ü' | 'ú' => out.push('u'),
            'Ù' | 'Û' | 'Ü' | 'Ú' => out.push('U'),
            'ÿ' => out.push('y'),
            'œ' => out.push_str("oe"),
            'Œ' => out.push_str("OE"),
            'æ' => out.push_str("ae"),
            'Æ' => out.push_str("AE"),
            '×' => out.push('x'),
            '·' | '–' | '—' => out.push('-'),
            '’' | '‘' => out.push('\''),
            '…' => out.push_str("..."),
            _ => out.push('?'),
        }
    }
    out
}

/// Ligne d'étiquette prête à écrire : début de la ligne de base (unités) et texte ASCII.
struct ILabel {
    at: Pt,
    text: String,
}

fn select_pen(text: &mut String, current: &mut Option<HpglPen>, pen: HpglPen) {
    if *current != Some(pen) {
        if current.is_some() {
            text.push_str("PU;\n");
        }
        let _ = writeln!(text, "SP{};", pen.number());
        *current = Some(pen);
    }
}

/// Bloc des étiquettes : `SI` (taille de caractère en cm), puis `PU` et `LB` ... ETX (0x03) par
/// ligne. La taille n'est pas multipliée par les facteurs X / Y (seules les positions le sont).
fn write_labels(text: &mut String, current: &mut Option<HpglPen>, labels: &[ILabel]) {
    select_pen(text, current, LABEL_PEN);
    let h = crate::LABEL_TEXT_MM / 10.0;
    let _ = writeln!(text, "SI{},{};", crate::num(h * 0.55), crate::num(h));
    for l in labels {
        let _ = writeln!(text, "PU{},{};", l.at.0, l.at.1);
        let _ = writeln!(text, "LB{}\u{3};", l.text);
    }
}

/// Traits de la scène multipliés par les facteurs X / Y (sans les traits de moins de deux points) ;
/// renvoie aussi le nombre de traits collectés avant ce filtre.
fn scaled_strokes(scene: &Scene, scale: PrintScale) -> Result<(Vec<Stroke>, usize), HpglError> {
    let raw = collect(scene)?;
    let collected = raw.len();
    let scaled: Vec<Stroke> = raw
        .into_iter()
        .filter(|(_, pts)| pts.len() >= 2)
        .map(|(pen, pts)| {
            let pts = pts
                .into_iter()
                .map(|p| PointMm::new(p.x * scale.x, p.y * scale.y))
                .collect();
            (pen, pts)
        })
        .collect();
    if scaled.iter().any(|(_, pts)| !finite_pts(pts)) {
        return Err(HpglError::NotFinite);
    }
    Ok((scaled, collected))
}

/// Ordre d'écriture : `(ordre, déplacement à vide, déplacement de l'ordre naturel)`, en unités.
fn plan_order(strokes: &[IStroke], optimize: bool) -> (Vec<IStroke>, f64, f64) {
    // Ordre naturel : scène triée (stable) par plume.
    let mut natural: Vec<IStroke> = Vec::with_capacity(strokes.len());
    for pen in HpglPen::ALL {
        natural.extend(strokes.iter().filter(|s| s.pen == pen).cloned());
    }
    let natural_travel = travel_units(&natural);
    let mut order = natural;
    let mut travel = natural_travel;
    if optimize {
        let mut pos = (0, 0);
        let mut optimized: Vec<IStroke> = Vec::with_capacity(strokes.len());
        for pen in HpglPen::ALL {
            let group: Vec<IStroke> = strokes.iter().filter(|s| s.pen == pen).cloned().collect();
            optimized.extend(nearest_order(group, &mut pos));
        }
        let t = travel_units(&optimized);
        if t <= natural_travel {
            order = optimized;
            travel = t;
        }
    }
    (order, travel, natural_travel)
}

/// Texte du fichier : tracés dans l'ordre, étiquettes à leur place dans l'ordre des plumes.
fn write_plot(order: &[IStroke], ilabels: &[ILabel]) -> String {
    let mut text = String::from("IN;\nPA;\n");
    let mut current: Option<HpglPen> = None;
    let mut labels_pending = !ilabels.is_empty();
    for s in order {
        // Les étiquettes s'écrivent à leur place dans l'ordre croissant des plumes.
        if labels_pending && s.pen.number() > LABEL_PEN.number() {
            write_labels(&mut text, &mut current, ilabels);
            labels_pending = false;
        }
        if current != Some(s.pen) {
            if current.is_some() {
                text.push_str("PU;\n");
            }
            let _ = writeln!(text, "SP{};", s.pen.number());
            current = Some(s.pen);
        }
        let _ = writeln!(text, "PU{},{};", s.pts[0].0, s.pts[0].1);
        for chunk in s.pts[1..].chunks(MAX_PD_POINTS) {
            let coords: Vec<String> = chunk.iter().map(|p| format!("{},{}", p.0, p.1)).collect();
            let _ = writeln!(text, "PD{};", coords.join(","));
        }
    }
    if labels_pending {
        write_labels(&mut text, &mut current, ilabels);
    }
    text.push_str("PU;\nSP0;\n");
    text
}

/// Usage de chaque plume (par numéro croissant) et longueur totale plume baissée (mm).
fn pen_uses(order: &[IStroke]) -> (Vec<PenUse>, f64) {
    let mut pens = Vec::new();
    let mut pen_down = 0.0;
    for pen in HpglPen::ALL {
        let used: Vec<&IStroke> = order.iter().filter(|s| s.pen == pen).collect();
        if used.is_empty() {
            continue;
        }
        let units: f64 = used
            .iter()
            .map(|s| {
                s.pts
                    .windows(2)
                    .map(|w| dist_units(w[0], w[1]))
                    .sum::<f64>()
            })
            .sum();
        pen_down += units / UNITS_PER_MM;
        pens.push(PenUse {
            pen,
            strokes: used.len(),
            length_mm: units / UNITS_PER_MM,
        });
    }
    (pens, pen_down)
}

/// Export HPGL avec les options par défaut (ordre optimisé).
pub fn export_hpgl(scene: &Scene, page: &PageSetup) -> Result<HpglPlot, HpglError> {
    export_hpgl_with(scene, page, HpglOptions::default())
}

/// Export HPGL d'une scène (déjà filtrée par le contenu de la sortie, US-13.4).
pub fn export_hpgl_with(
    scene: &Scene,
    page: &PageSetup,
    options: HpglOptions,
) -> Result<HpglPlot, HpglError> {
    page.validate()?;
    scene.check_finite().map_err(|_| HpglError::NotFinite)?;
    let scale = page.scale;
    let (scaled, collected) = scaled_strokes(scene, scale)?;
    let Some(first) = scaled.first().map(|(_, pts)| pts[0]) else {
        return Err(HpglError::Nothing);
    };
    let (mut min, mut max) = (first, first);
    for p in scaled.iter().flat_map(|(_, pts)| pts) {
        min = PointMm::new(min.x.min(p.x), min.y.min(p.y));
        max = PointMm::new(max.x.max(p.x), max.y.max(p.y));
    }
    // Étiquettes de pièce : leur rectangle entre dans l'emprise (jamais de coordonnée négative).
    let scaled_rect = |l: &crate::ScenePieceLabel| {
        let (lo, hi) = l.rect();
        (
            PointMm::new(lo.x * scale.x, lo.y * scale.y),
            PointMm::new(hi.x * scale.x, hi.y * scale.y),
        )
    };
    for l in &scene.piece_labels {
        let (lo, hi) = scaled_rect(l);
        if !finite_pts(&[lo, hi]) {
            return Err(HpglError::NotFinite);
        }
        min = PointMm::new(min.x.min(lo.x), min.y.min(lo.y));
        max = PointMm::new(max.x.max(hi.x), max.y.max(hi.y));
    }
    // Libellés des bords sur le pli (PH-5b) : leur rectangle entre dans l'emprise.
    for f in &scene.folds {
        let (lo, hi) = f.text_rect();
        let (lo, hi) = (
            PointMm::new(lo.x * scale.x, lo.y * scale.y),
            PointMm::new(hi.x * scale.x, hi.y * scale.y),
        );
        if !finite_pts(&[lo, hi]) {
            return Err(HpglError::NotFinite);
        }
        min = PointMm::new(min.x.min(lo.x), min.y.min(lo.y));
        max = PointMm::new(max.x.max(hi.x), max.y.max(hi.y));
    }
    // Libellés « Rempli … mm » (PH-5c) : même traitement.
    for f in &scene.fold_overs {
        let (lo, hi) = f.text_rect();
        let (lo, hi) = (
            PointMm::new(lo.x * scale.x, lo.y * scale.y),
            PointMm::new(hi.x * scale.x, hi.y * scale.y),
        );
        if !finite_pts(&[lo, hi]) {
            return Err(HpglError::NotFinite);
        }
        min = PointMm::new(min.x.min(lo.x), min.y.min(lo.y));
        max = PointMm::new(max.x.max(hi.x), max.y.max(hi.y));
    }
    let extent = (max.x - min.x).max(max.y - min.y);
    if !extent.is_finite() || extent * UNITS_PER_MM > MAX_COORD_UNITS as f64 {
        return Err(HpglError::OutOfRange { extent_mm: extent });
    }
    let mut dropped = collected - scaled.len();
    let mut strokes: Vec<IStroke> = Vec::new();
    for (pen, pts) in scaled {
        let mut ipts: Vec<Pt> = Vec::with_capacity(pts.len());
        for p in &pts {
            let q = (
                ((p.x - min.x) * UNITS_PER_MM).round() as i64,
                ((p.y - min.y) * UNITS_PER_MM).round() as i64,
            );
            if ipts.last() != Some(&q) {
                ipts.push(q);
            }
        }
        if ipts.len() < 2 {
            dropped += 1;
            continue;
        }
        let closed = ipts.len() >= 4 && ipts[0] == ipts[ipts.len() - 1];
        strokes.push(IStroke {
            pen,
            pts: ipts,
            closed,
        });
    }
    if strokes.is_empty() {
        return Err(HpglError::Nothing);
    }
    let (mut wx, mut wy) = (0, 0);
    for p in strokes.iter().flat_map(|s| &s.pts) {
        wx = wx.max(p.0);
        wy = wy.max(p.1);
    }
    let to_units = |p: PointMm| {
        (
            ((p.x - min.x) * UNITS_PER_MM).round() as i64,
            ((p.y - min.y) * UNITS_PER_MM).round() as i64,
        )
    };
    let mut ilabels: Vec<ILabel> = Vec::new();
    for l in &scene.piece_labels {
        let (_, hi) = scaled_rect(l);
        let hi = to_units(hi);
        wx = wx.max(hi.0);
        wy = wy.max(hi.1);
        for (i, line) in l.lines.iter().enumerate() {
            let b = l.baseline_start(i);
            ilabels.push(ILabel {
                at: to_units(PointMm::new(b.x * scale.x, b.y * scale.y)),
                text: hpgl_text(line),
            });
        }
    }
    for f in &scene.folds {
        let (_, hi) = f.text_rect();
        let hi = to_units(PointMm::new(hi.x * scale.x, hi.y * scale.y));
        wx = wx.max(hi.0);
        wy = wy.max(hi.1);
        let b = f.baseline_start();
        ilabels.push(ILabel {
            at: to_units(PointMm::new(b.x * scale.x, b.y * scale.y)),
            text: hpgl_text(&f.label),
        });
    }
    for f in &scene.fold_overs {
        let (_, hi) = f.text_rect();
        let hi = to_units(PointMm::new(hi.x * scale.x, hi.y * scale.y));
        wx = wx.max(hi.0);
        wy = wy.max(hi.1);
        let b = f.baseline_start();
        ilabels.push(ILabel {
            at: to_units(PointMm::new(b.x * scale.x, b.y * scale.y)),
            text: hpgl_text(&f.label),
        });
    }
    let (width_mm, height_mm) = (wx as f64 / UNITS_PER_MM, wy as f64 / UNITS_PER_MM);
    let (zone_width_mm, zone_height_mm) = check_fit(page, width_mm, height_mm)?;

    let (order, travel, natural_travel) = plan_order(&strokes, options.optimize);
    let text = write_plot(&order, &ilabels);
    let (pens, pen_down) = pen_uses(&order);
    let report = HpglReport {
        pens,
        strokes: order.len(),
        width_mm,
        height_mm,
        zone_width_mm,
        zone_height_mm,
        pen_down_mm: pen_down,
        travel_mm: travel / UNITS_PER_MM,
        natural_travel_mm: natural_travel / UNITS_PER_MM,
        labels: ilabels.len(),
        texts_omitted: scene.dimensions.len()
            + scene.full_dimensions.len()
            + scene.points.iter().filter(|p| p.show_label).count(),
        points_omitted: scene.points.len(),
        degenerate_dropped: dropped,
        scale,
    };
    Ok(HpglPlot { text, report })
}

// ---------------------------------------------------------------------------------------------
// Tuilage (PH-5e)
//
// Conventions :
// - Pages : exactement celles du PDF (`plan_tiles_unrotated`, fenêtre = zone imprimable moins le
//   bandeau de titre de 8 mm), numérotées de 1 à N dans l'ordre du PDF (ligne par ligne depuis le
//   haut-gauche), **sans** page d'étalon ni page de plan (le plan d'assemblage est ignoré,
//   `HpglTiles::plan_ignored`). Un rouleau n'est jamais découpé.
// - Un fichier par page ; origine = coin bas-gauche de la fenêtre de la page ; unités et plumes de
//   l'export non tuilé. Facteurs X / Y appliqués comme pour le PDF (fenêtres en mm papier).
// - Découpe : tout tracé (courbes déjà en polylignes, cercles de perçage, fentes) est coupé
//   **exactement** aux bords de la fenêtre (Liang-Barsky par segment) ; aucune coordonnée écrite n'est
//   hors de [0 ; largeur] × [0 ; hauteur] de la fenêtre, texte et numéro de page exceptés.
// - Texte `LB` (non découpable) : une ligne est écrite sur toute page dont la fenêtre contient
//   le **début de sa ligne de base** (la ligne peut donc dépasser le bord droit ; dans une bande de
//   recouvrement elle est écrite sur les deux pages).
// - Repères : option « repères d'assemblage » = croix de 6 mm (plume 8) aux mêmes points que le PDF
//   (`pdf::registration_marks`), coupées par la fenêtre comme un tracé ; les flèches et noms de pages
//   voisines du PDF ne sont pas écrits. Numéro de page « n/N Lr Cc » en ASCII, plume 7, au-dessus de
//   la fenêtre (ligne de base à 2,5 mm), à la place du bandeau de titre du PDF.
// - Une feuille qui tient sur une seule page (et dont l'export non tuilé réussit) donne **le
//   même fichier, octet pour octet, que [`export_hpgl_with`]**, sans numéro de page.
// ---------------------------------------------------------------------------------------------

/// Une page du HPGL tuilé : un fichier.
#[derive(Debug, Clone, PartialEq)]
pub struct HpglPage {
    /// Numéro de page, de 1 à `total` (même ordre que les pages de patron du PDF).
    pub number: usize,
    pub total: usize,
    /// Colonne et ligne dans la grille d'origine (0 = L1C1, en haut à gauche).
    pub col: usize,
    pub row: usize,
    /// Identifiant lisible « L1C2 ».
    pub id: String,
    pub text: String,
    /// Rapport de la page : `width_mm` / `height_mm` = fenêtre de la page ; `pens` et longueurs =
    /// ce qui est écrit sur cette page (recouvrements compris).
    pub report: HpglReport,
}

impl HpglPage {
    pub fn bytes(&self) -> &[u8] {
        self.text.as_bytes()
    }
}

/// HPGL découpé en pages (une page = un fichier).
#[derive(Debug, Clone, PartialEq)]
pub struct HpglTiles {
    pub pages: Vec<HpglPage>,
    /// Vrai si la page de plan d'assemblage était demandée : elle n'existe pas en HPGL.
    pub plan_ignored: bool,
}

/// Coupe le segment `a`-`b` par `[0 ; w] × [0 ; h]` : paramètres `(t0, t1)` de la partie dedans.
fn clip_segment(a: PointMm, b: PointMm, w: f64, h: f64) -> Option<(f64, f64)> {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let (mut t0, mut t1) = (0.0_f64, 1.0_f64);
    for (p, q) in [(-dx, a.x), (dx, w - a.x), (-dy, a.y), (dy, h - a.y)] {
        if p == 0.0 {
            if q < 0.0 {
                return None;
            }
        } else {
            let r = q / p;
            if p < 0.0 {
                if r > t1 {
                    return None;
                }
                t0 = t0.max(r);
            } else {
                if r < t0 {
                    return None;
                }
                t1 = t1.min(r);
            }
        }
    }
    Some((t0, t1))
}

/// Morceaux d'une polyligne (déjà en repère de la fenêtre) qui sont dans `[0 ; w] × [0 ; h]`.
fn clip_polyline(pts: &[PointMm], w: f64, h: f64) -> Vec<Vec<PointMm>> {
    let mut out: Vec<Vec<PointMm>> = Vec::new();
    let mut cur: Vec<PointMm> = Vec::new();
    for win in pts.windows(2) {
        let (a, b) = (win[0], win[1]);
        let Some((t0, t1)) = clip_segment(a, b, w, h) else {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            continue;
        };
        let at = |t: f64| {
            PointMm::new(
                (a.x + t * (b.x - a.x)).clamp(0.0, w),
                (a.y + t * (b.y - a.y)).clamp(0.0, h),
            )
        };
        if t0 > 0.0 && !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
        if cur.is_empty() {
            cur.push(if t0 > 0.0 { at(t0) } else { a });
        }
        cur.push(if t1 < 1.0 { at(t1) } else { b });
        if t1 < 1.0 {
            out.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Lignes de texte (début de la ligne de base multiplié par les facteurs, texte ASCII).
fn scaled_label_lines(
    scene: &Scene,
    scale: PrintScale,
) -> Result<Vec<(PointMm, String)>, HpglError> {
    let mut out = Vec::new();
    let mut push = |b: PointMm, text: String| {
        let p = PointMm::new(b.x * scale.x, b.y * scale.y);
        if !finite_pts(&[p]) {
            return Err(HpglError::NotFinite);
        }
        out.push((p, text));
        Ok(())
    };
    for l in &scene.piece_labels {
        for (i, line) in l.lines.iter().enumerate() {
            push(l.baseline_start(i), hpgl_text(line))?;
        }
    }
    for f in &scene.folds {
        push(f.baseline_start(), hpgl_text(&f.label))?;
    }
    for f in &scene.fold_overs {
        push(f.baseline_start(), hpgl_text(&f.label))?;
    }
    Ok(out)
}

/// Une page : traits coupés par la fenêtre, repères, étiquettes dont le début est dans la fenêtre.
fn build_page(
    strokes: &[Stroke],
    labels: &[(PointMm, String)],
    tiles: &[Tile],
    k: usize,
    page: &PageSetup,
    options: HpglOptions,
    omitted: (usize, usize),
) -> HpglPage {
    let tile = &tiles[k];
    let (ww, wh) = (tile.layout.window_w_mm, tile.layout.window_h_mm);
    let o = tile.world_min;
    let (max_x, max_y) = (
        (ww * UNITS_PER_MM + 1e-9).floor() as i64,
        (wh * UNITS_PER_MM + 1e-9).floor() as i64,
    );
    let mut marks: Vec<Stroke> = Vec::new();
    if page.assembly.marks {
        for c in registration_marks(tile, tiles) {
            for (dx, dy) in [(MARK_ARM_MM, 0.0), (0.0, MARK_ARM_MM)] {
                marks.push((
                    HpglPen::Other,
                    vec![
                        PointMm::new(c.x - dx, c.y - dy),
                        PointMm::new(c.x + dx, c.y + dy),
                    ],
                ));
            }
        }
    }
    let mut istrokes: Vec<IStroke> = Vec::new();
    let mut dropped = 0;
    for (pen, pts) in strokes.iter().chain(&marks) {
        let rel: Vec<PointMm> = pts
            .iter()
            .map(|p| PointMm::new(p.x - o.x, p.y - o.y))
            .collect();
        for piece in clip_polyline(&rel, ww, wh) {
            let mut ipts: Vec<Pt> = Vec::with_capacity(piece.len());
            for p in &piece {
                let q = (
                    ((p.x * UNITS_PER_MM).round() as i64).clamp(0, max_x),
                    ((p.y * UNITS_PER_MM).round() as i64).clamp(0, max_y),
                );
                if ipts.last() != Some(&q) {
                    ipts.push(q);
                }
            }
            if ipts.len() < 2 {
                // Un point seul : le tracé touche la fenêtre sans la traverser (ou s'effondre).
                if piece.len() >= 2 && piece.iter().any(|p| *p != piece[0]) {
                    dropped += 1;
                }
                continue;
            }
            let closed = ipts.len() >= 4 && ipts[0] == ipts[ipts.len() - 1];
            istrokes.push(IStroke {
                pen: *pen,
                pts: ipts,
                closed,
            });
        }
    }
    let to_units = |p: PointMm| {
        (
            ((p.x - o.x) * UNITS_PER_MM).round() as i64,
            ((p.y - o.y) * UNITS_PER_MM).round() as i64,
        )
    };
    let mut ilabels: Vec<ILabel> = Vec::new();
    for (b, text) in labels {
        let (x, y) = (b.x - o.x, b.y - o.y);
        if (0.0..=ww).contains(&x) && (0.0..=wh).contains(&y) {
            ilabels.push(ILabel {
                at: to_units(*b),
                text: text.clone(),
            });
        }
    }
    let (number, total) = (k + 1, tiles.len());
    ilabels.push(ILabel {
        at: (0, ((wh + PAGE_LABEL_RISE_MM) * UNITS_PER_MM).round() as i64),
        text: format!("{number}/{total} {}", tile.id()),
    });
    let (order, travel, natural_travel) = plan_order(&istrokes, options.optimize);
    let text = write_plot(&order, &ilabels);
    let (pens, pen_down) = pen_uses(&order);
    let report = HpglReport {
        pens,
        strokes: order.len(),
        width_mm: ww,
        height_mm: wh,
        zone_width_mm: ww,
        zone_height_mm: Some(wh),
        pen_down_mm: pen_down,
        travel_mm: travel / UNITS_PER_MM,
        natural_travel_mm: natural_travel / UNITS_PER_MM,
        labels: ilabels.len(),
        texts_omitted: omitted.0,
        points_omitted: omitted.1,
        degenerate_dropped: dropped,
        scale: page.scale,
    };
    HpglPage {
        number,
        total,
        col: tile.col,
        row: tile.row,
        id: tile.id(),
        text,
        report,
    }
}

/// Ligne de base du numéro de page au-dessus de la fenêtre (dans le bandeau de 8 mm du PDF).
const PAGE_LABEL_RISE_MM: f64 = 2.5;

/// Export HPGL découpé en pages (PH-5e) : une page = un fichier, mêmes pages que le PDF (voir la
/// section « Tuilage »). Les refus sont ceux de [`export_hpgl_with`] sauf `SheetTooSmall`, qui
/// n'existe plus ici (la feuille est découpée), plus `Tiling` (trop de pages, feuille vide).
/// Aucun résultat partiel : tout est calculé avant de rendre quoi que ce soit.
pub fn export_hpgl_pages(
    scene: &Scene,
    page: &PageSetup,
    options: HpglOptions,
) -> Result<HpglTiles, HpglError> {
    page.validate()?;
    scene.check_finite().map_err(|_| HpglError::NotFinite)?;
    let single = |plot: HpglPlot| HpglTiles {
        pages: vec![HpglPage {
            number: 1,
            total: 1,
            col: 0,
            row: 0,
            id: "L1C1".to_string(),
            text: plot.text,
            report: plot.report,
        }],
        plan_ignored: false,
    };
    if page.format.is_roll() {
        return export_hpgl_with(scene, page, options).map(single);
    }
    let (strokes, _) = scaled_strokes(scene, page.scale)?;
    if strokes.is_empty() {
        return Err(HpglError::Nothing);
    }
    let labels = scaled_label_lines(scene, page.scale)?;
    let tiles = plan_tiles_unrotated(scene, page).map_err(HpglError::Tiling)?;
    if tiles.len() == 1 {
        if let Ok(plot) = export_hpgl_with(scene, page, options) {
            return Ok(single(plot));
        }
    }
    let omitted = (
        scene.dimensions.len()
            + scene.full_dimensions.len()
            + scene.points.iter().filter(|p| p.show_label).count(),
        scene.points.len(),
    );
    let pages: Vec<HpglPage> = (0..tiles.len())
        .map(|k| build_page(&strokes, &labels, &tiles, k, page, options, omitted))
        .collect();
    if pages.iter().all(|p| p.report.strokes == 0) {
        return Err(HpglError::Nothing);
    }
    Ok(HpglTiles {
        pages,
        plan_ignored: page.assembly.plan,
    })
}
