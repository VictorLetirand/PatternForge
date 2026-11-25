//! PatternForge — exports physiques.
//!
//! L'export part d'une [`Scene`] en millimètres, construite depuis le modèle.
//! Il n'existe aucun paramètre de zoom, de DPI écran ou de palette d'interface :
//! par construction, la vue ne peut pas influencer une cote exportée.

pub mod assembly;
pub mod bom_export;
pub mod calibration;
pub mod content;
pub mod cutting_export;
pub mod dxf;
pub mod dxf_aama_import;
pub mod dxf_construction;
pub mod dxf_import;
mod finite;
pub mod font;
pub mod hpgl;
pub mod page;
pub mod pdf;
pub mod pdf_import;
pub mod raster;
pub mod report;
pub mod size_export;
pub mod size_measures_export;
pub mod svg;
pub mod svg_import;
pub mod vector_import;
pub mod zone;

pub use calibration::{PrintScale, PrinterProfile, PrinterProfiles};
pub use content::{ContentNote, ContentSummary, OutputCategory, OutputContent, OutputSize};
pub use page::{
    AssemblyOptions, Margins, Orientation, PageField, PageFormat, PageSetup, PageSetupError,
};
pub use zone::SceneZone;

use pf_core::{
    ButtonShape, CoreError, GrainSense, MarkingShape, PathRole, Project, Shape, SheetId,
};
use pf_geometry::region::EdgeGeom;
use pf_geometry::slot::Slot;
use pf_geometry::{Arc, Bbox, CircleTrace, CubicBezier, PointMm};
use std::fmt;

/// Conversion millimètres → points PDF : 72 points par pouce de 25,4 mm.
pub const MM_TO_PT: f64 = 72.0 / 25.4;
pub const A4_WIDTH_MM: f64 = 210.0;
pub const A4_HEIGHT_MM: f64 = 297.0;
/// Côté du carré étalon.
pub const CALIBRATION_MM: f64 = 100.0;

#[derive(Debug, Clone, PartialEq)]
pub enum ExportError {
    Core(CoreError),
    /// Rien à exporter sur cette feuille.
    EmptySheet(String),
    /// La feuille a du contenu, mais le choix de contenu de la sortie (US-13.4) n'en laisse
    /// aucun : refus, aucun fichier écrit.
    NothingSelected(String),
    TooManyPages {
        pages: usize,
        max: usize,
    },
    Io(String),
    /// Réglage de page invalide ou contenu impossible à placer (US-13.1).
    Page(PageSetupError),
    /// Une valeur de la feuille nommée n'est pas un nombre fini : rien n'est produit.
    NotFinite(String),
}

impl fmt::Display for ExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Core(e) => write!(f, "{e}"),
            Self::EmptySheet(name) => {
                write!(
                    f,
                    "la feuille « {name} » ne contient aucun tracé à exporter"
                )
            }
            Self::NothingSelected(name) => write!(
                f,
                "le contenu choisi pour la sortie ne laisse rien à exporter sur la feuille « {name} »"
            ),
            Self::TooManyPages { pages, max } => write!(
                f,
                "l'export demanderait {pages} pages (maximum {max}) : export refusé plutôt \
                 que tronqué"
            ),
            Self::Io(msg) => write!(f, "erreur de fichier : {msg}"),
            Self::Page(e) => write!(f, "{e}"),
            Self::NotFinite(name) => write!(
                f,
                "une coordonnée de la feuille « {name} » n'est pas un nombre fini : export refusé"
            ),
        }
    }
}

impl std::error::Error for ExportError {}

impl From<PageSetupError> for ExportError {
    fn from(e: PageSetupError) -> Self {
        ExportError::Page(e)
    }
}

impl From<CoreError> for ExportError {
    fn from(e: CoreError) -> Self {
        ExportError::Core(e)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum PathGeom {
    Line(PointMm, PointMm),
    Cubic(CubicBezier),
    /// Arc de cercle : exporté en arc vrai (SVG) ou en Béziers d'erreur < 0,005 mm (PDF).
    Arc(Arc),
    /// Cercle entier, avec son point de départ et son sens.
    Circle(CircleTrace),
    /// Courbe par points de passage : suite continue de Béziers cubiques
    /// (un seul tracé, donc un seul motif de pointillés).
    Spline(Vec<CubicBezier>),
}

/// Erreur maximale (mm) de la découpe en Béziers des arcs et cercles du PDF.
pub const ARC_EXPORT_TOLERANCE_MM: f64 = 0.002;

impl PathGeom {
    /// Béziers cubiques d'un arc, d'un cercle ou d'une courbe par points de
    /// passage (une seule suite continue).
    pub fn arc_beziers(&self) -> Vec<CubicBezier> {
        match self {
            PathGeom::Arc(a) => a.to_beziers(ARC_EXPORT_TOLERANCE_MM),
            PathGeom::Circle(c) => c
                .halves()
                .iter()
                .flat_map(|h| h.to_beziers(ARC_EXPORT_TOLERANCE_MM))
                .collect(),
            PathGeom::Spline(segments) => segments.clone(),
            _ => Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ScenePath {
    pub geom: PathGeom,
    pub role: PathRole,
    /// Sens du tissu d'une ligne de droit-fil (US-06.5) ; `Unmarked` pour tout autre tracé
    /// et pour un droit-fil sans réglage (exporté comme avant, à l'octet).
    pub sense: GrainSense,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ScenePoint {
    pub pos: PointMm,
    pub label: String,
    /// Écrire le nom du point à côté du repère (US-13.4) ; vrai par défaut.
    pub show_label: bool,
}

/// Cotation résolue en mm : ligne de cote décalée et texte de la valeur.
#[derive(Debug, Clone, PartialEq)]
pub struct SceneDimension {
    pub from: PointMm,
    pub to: PointMm,
    pub from_offset: PointMm,
    pub to_offset: PointMm,
    /// Valeur formatée, par exemple « 170.00 mm ».
    pub text: String,
}

/// Taille du texte d'une cotation (mm de hauteur de corps).
pub const DIMENSION_TEXT_MM: f64 = 3.0;

impl SceneDimension {
    /// Centre horizontal et ligne de base du texte. Cotation plutôt horizontale :
    /// texte au-dessus ou au-dessous de la ligne de cote, côté opposé aux points
    /// cotés. Cotation plutôt verticale : texte à côté de la ligne, au même côté.
    pub fn text_anchor(&self) -> PointMm {
        let mid = PointMm::new(
            (self.from_offset.x + self.to_offset.x) / 2.0,
            (self.from_offset.y + self.to_offset.y) / 2.0,
        );
        let base = PointMm::new(
            (self.from.x + self.to.x) / 2.0,
            (self.from.y + self.to.y) / 2.0,
        );
        let away = mid - base;
        let along = self.to_offset - self.from_offset;
        if along.y.abs() > along.x.abs() {
            let side = if away.x < -1e-9 { -1.0 } else { 1.0 };
            PointMm::new(
                mid.x + side * (1.5 + self.text_half_width_mm()),
                mid.y - DIMENSION_TEXT_MM * 0.35,
            )
        } else if away.y < -1e-9 {
            PointMm::new(mid.x, mid.y - 1.5 - DIMENSION_TEXT_MM * 0.7)
        } else {
            PointMm::new(mid.x, mid.y + 1.5)
        }
    }

    /// Demi-largeur approximative du texte (mm), pour le centrer et le cadrer.
    pub fn text_half_width_mm(&self) -> f64 {
        self.text.chars().count() as f64 * DIMENSION_TEXT_MM * 0.55 / 2.0
    }

    /// Traits de la cotation : traits d'attache, ligne de cote et petites
    /// flèches (mêmes pointes de 4 mm que le droit-fil).
    pub fn marks(&self) -> Vec<(PointMm, PointMm)> {
        let mut marks = Vec::new();
        if pf_geometry::distance_mm(self.from, self.from_offset) > 1e-9 {
            marks.push((self.from, self.from_offset));
            marks.push((self.to, self.to_offset));
        }
        marks.push((self.from_offset, self.to_offset));
        marks.extend(grainline_arrow_marks(self.from_offset, self.to_offset));
        marks
    }
}

/// Cote complète (US-06.7) résolue en mm : traits d'attache, ligne de cote (droite ou arc
/// approché par des segments de 2° au plus), pointes de flèche, et texte **orienté le long de
/// la ligne** (jamais à l'envers : angle dans `]-90 ; 90]`).
#[derive(Debug, Clone, PartialEq)]
pub struct SceneFullDimension {
    /// Identifiant du type de cote (`path_length`, `angle`, `radius`, `diameter`,
    /// `horizontal`, `vertical`), écrit dans le SVG.
    pub kind: &'static str,
    /// Valeur mesurée (mm, ou degrés pour un angle ; projections signées).
    pub value: f64,
    pub marks: Vec<(PointMm, PointMm)>,
    /// Centre visuel du texte (déjà remonté au-dessus de la ligne de cote).
    pub text_center: PointMm,
    /// Angle du texte, degrés, sens trigonométrique, dans `]-90 ; 90]`.
    pub text_angle_deg: f64,
    /// Légende et valeur, au point décimal : par exemple « Épaule : 80.00 mm ».
    pub text: String,
}

/// Plus petite longueur (mm) d'une ligne de cote qui porte des pointes de flèche.
const FULL_DIM_MIN_ARROW_LINE_MM: f64 = 3.0;

impl SceneFullDimension {
    fn arrow(
        tip: PointMm,
        toward_inside: pf_geometry::VectorMm,
        len: f64,
    ) -> [(PointMm, PointMm); 2] {
        [25.0, -25.0].map(|angle| {
            let (s, c) = pf_geometry::sin_cos_deg(angle);
            let v = pf_geometry::VectorMm::new(
                c * toward_inside.x - s * toward_inside.y,
                s * toward_inside.x + c * toward_inside.y,
            );
            (tip, tip + v * len)
        })
    }

    /// Résolution d'une cote calculée par le modèle.
    pub fn from_geometry(g: &pf_core::full_dimension::FullDimensionGeometry) -> Self {
        use pf_core::full_dimension::DimensionLine;
        let mut marks: Vec<(PointMm, PointMm)> = g.extension_lines.clone();
        match g.line {
            DimensionLine::Straight {
                a,
                b,
                arrow_a,
                arrow_b,
            } => {
                marks.push((a, b));
                let len = pf_geometry::distance_mm(a, b);
                if len >= FULL_DIM_MIN_ARROW_LINE_MM {
                    if let Ok(u) = (b - a).unit() {
                        let l = ARROW_LEN_MM.min(len / 3.0);
                        if arrow_a {
                            marks.extend(Self::arrow(a, u, l));
                        }
                        if arrow_b {
                            marks.extend(Self::arrow(b, pf_geometry::VectorMm::new(-u.x, -u.y), l));
                        }
                    }
                }
            }
            DimensionLine::Arc {
                center,
                radius_mm,
                start_deg,
                sweep_deg,
            } => {
                let n = ((sweep_deg / 2.0).ceil() as usize).clamp(2, 180);
                let at = |deg: f64| {
                    let (s, c) = pf_geometry::sin_cos_deg(deg);
                    PointMm::new(center.x + radius_mm * c, center.y + radius_mm * s)
                };
                let mut prev = at(start_deg);
                for i in 1..=n {
                    let q = at(start_deg + sweep_deg * i as f64 / n as f64);
                    marks.push((prev, q));
                    prev = q;
                }
                let arc_len = radius_mm * sweep_deg.to_radians();
                if arc_len >= FULL_DIM_MIN_ARROW_LINE_MM {
                    let l = ARROW_LEN_MM.min(arc_len / 3.0);
                    let tangent = |deg: f64| {
                        let (s, c) = pf_geometry::sin_cos_deg(deg);
                        pf_geometry::VectorMm::new(-s, c)
                    };
                    // Pointe au départ : les branches repartent dans le sens du balayage ;
                    // à l'arrivée : en sens inverse.
                    marks.extend(Self::arrow(at(start_deg), tangent(start_deg), l));
                    let end = start_deg + sweep_deg;
                    let back = tangent(end);
                    marks.extend(Self::arrow(
                        at(end),
                        pf_geometry::VectorMm::new(-back.x, -back.y),
                        l,
                    ));
                }
            }
        }
        SceneFullDimension {
            kind: g.kind.key(),
            value: g.value,
            marks,
            text_center: g.text_center(),
            text_angle_deg: g.text_angle_deg,
            text: g.text('.'),
        }
    }

    /// Largeur approchée du texte (mm), comme pour les cotations d'US-02.
    pub fn text_width_mm(&self) -> f64 {
        self.text.chars().count() as f64 * DIMENSION_TEXT_MM * 0.55
    }

    fn direction(&self) -> (f64, f64) {
        let (s, c) = pf_geometry::sin_cos_deg(self.text_angle_deg);
        (c, s)
    }

    /// Milieu de la ligne de base du texte.
    pub fn baseline_center(&self) -> PointMm {
        let (c, s) = self.direction();
        // Hauteur de capitale approchée : 0,7 × corps ; le centre visuel est à moitié.
        let down = DIMENSION_TEXT_MM * 0.35;
        PointMm::new(self.text_center.x + s * down, self.text_center.y - c * down)
    }

    /// Début de la ligne de base du texte quand il est posé autour de `center` selon
    /// `angle_deg` (même formule que [`Self::baseline_start`], pour un dessin tourné).
    pub fn baseline_start_at(&self, center: PointMm, angle_deg: f64) -> PointMm {
        let (s, c) = pf_geometry::sin_cos_deg(angle_deg);
        let down = DIMENSION_TEXT_MM * 0.35;
        let (bx, by) = (center.x + s * down, center.y - c * down);
        let half = self.text_width_mm() / 2.0;
        PointMm::new(bx - c * half, by - s * half)
    }

    /// Début (à gauche, dans le sens de lecture) de la ligne de base du texte.
    pub fn baseline_start(&self) -> PointMm {
        let (c, s) = self.direction();
        let (b, half) = (self.baseline_center(), self.text_width_mm() / 2.0);
        PointMm::new(b.x - c * half, b.y - s * half)
    }

    /// Les quatre coins du rectangle du texte (pour l'emprise).
    pub fn text_corners(&self) -> [PointMm; 4] {
        let (c, s) = self.direction();
        let (hw, hh) = (self.text_width_mm() / 2.0, DIMENSION_TEXT_MM / 2.0);
        let o = self.text_center;
        [(-hw, -hh), (hw, -hh), (hw, hh), (-hw, hh)]
            .map(|(x, y)| PointMm::new(o.x + x * c - y * s, o.y + x * s + y * c))
    }
}

/// Épaisseur (mm) du trait des marquages de pièce.
pub const MARKING_STROKE_MM: f64 = 0.35;

/// Marquage de pièce (US-06.3) résolu en mm : un pointage (disque plein de
/// [`pf_core::piece_marking::PUNCH_DIAMETER_MM`]), un perçage (cercle du diamètre
/// donné) ou une croix « + ». Exporté dans un groupe nommé distinct, avec une forme
/// reconnaissable sans la couleur.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SceneMarking {
    pub pos: PointMm,
    pub shape: MarkingShape,
}

impl SceneMarking {
    /// Nom de la forme dans les fichiers exportés (attribut `data-form` du SVG).
    pub fn form_id(&self) -> &'static str {
        match self.shape {
            MarkingShape::Punch => "pointage",
            MarkingShape::Drill { .. } => "percage",
            MarkingShape::Cross { .. } => "croix",
            MarkingShape::Slot { .. } => "fente",
        }
    }

    /// Rayon (mm) et remplissage du cercle ; `None` pour une croix ou une fente.
    pub fn circle(&self) -> Option<(f64, bool)> {
        match self.shape {
            MarkingShape::Punch => Some((pf_core::piece_marking::PUNCH_DIAMETER_MM / 2.0, true)),
            MarkingShape::Drill { diameter_mm } => Some((diameter_mm / 2.0, false)),
            MarkingShape::Cross { .. } | MarkingShape::Slot { .. } => None,
        }
    }

    /// Fente de poinçon (PH-4b) ; `None` pour une autre forme.
    pub fn slot(&self) -> Option<Slot> {
        self.shape.slot(self.pos)
    }

    /// Contour fermé de la fente (PDF) : point de départ puis segments ; `None` sinon.
    pub fn slot_outline(&self) -> Option<(PointMm, Vec<SlotSegment>)> {
        self.slot().map(slot_path)
    }

    /// Les deux branches de la croix (horizontale puis verticale) ; vide sinon.
    pub fn cross_marks(&self) -> Vec<(PointMm, PointMm)> {
        match self.shape {
            MarkingShape::Cross { size_mm } => {
                let h = size_mm / 2.0;
                let c = self.pos;
                vec![
                    (PointMm::new(c.x - h, c.y), PointMm::new(c.x + h, c.y)),
                    (PointMm::new(c.x, c.y - h), PointMm::new(c.x, c.y + h)),
                ]
            }
            _ => Vec::new(),
        }
    }

    /// Demi-emprise (mm) du dessin autour de la position.
    pub fn half_extent_mm(&self) -> f64 {
        match self.shape {
            MarkingShape::Cross { size_mm } => size_mm / 2.0,
            MarkingShape::Slot { .. } => self.slot().map_or(0.0, |s| {
                let (hx, hy) = s.half_extent();
                hx.max(hy)
            }),
            _ => self.circle().map_or(0.0, |(r, _)| r),
        }
    }

    /// Béziers du cercle (PDF) ; vide pour une croix.
    pub fn circle_beziers(&self) -> Vec<CubicBezier> {
        let Some((r, _)) = self.circle() else {
            return Vec::new();
        };
        CircleTrace::new(self.pos, r, 0.0, true)
            .map(|c| PathGeom::Circle(c).arc_beziers())
            .unwrap_or_default()
    }
}

/// Trou de piqûre (US-06.9) résolu en mm : centre, forme du poinçon et direction du bord. Exporté
/// dans un groupe nommé distinct des marquages (SVG, PDF) ; couche des perçages et marques (DXF),
/// plume des marquages (HPGL). La forme est reconnaissable sans la couleur.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SceneStitchHole {
    pub pos: PointMm,
    pub hole: pf_core::StitchHole,
    /// Direction du bord en ce point (degrés, anti-horaire depuis +X).
    pub tangent_deg: f64,
}

impl SceneStitchHole {
    /// Nom de la forme dans les fichiers exportés (attribut `data-form` du SVG).
    pub fn form_id(&self) -> &'static str {
        match self.hole {
            pf_core::StitchHole::Round { .. } => "rond",
            pf_core::StitchHole::Flat { .. } => "plat",
            pf_core::StitchHole::French { .. } => "francais",
            pf_core::StitchHole::Japanese { .. } => "oblique",
        }
    }

    /// Rayon (mm) du cercle d'un trou rond ; `None` pour une forme à sommets.
    pub fn circle_radius(&self) -> Option<f64> {
        match self.hole {
            pf_core::StitchHole::Round { diameter_mm } => Some(diameter_mm / 2.0),
            _ => None,
        }
    }

    /// Sommets du contour fermé d'un trou plat, français ou oblique ; `None` pour un trou rond.
    pub fn vertices(&self) -> Option<[PointMm; 4]> {
        self.hole.polygon(self.pos, self.tangent_deg)
    }

    /// Contour fermé en segments droits (PDF) : point de départ puis segments ; `None` pour un
    /// trou rond.
    pub fn outline(&self) -> Option<(PointMm, Vec<SlotSegment>)> {
        let v = self.vertices()?;
        Some((v[0], v[1..].iter().map(|p| SlotSegment::Line(*p)).collect()))
    }

    /// Demi-emprise (mm) du dessin autour du centre.
    pub fn half_extent_mm(&self) -> f64 {
        self.hole.half_extent_mm()
    }

    /// Béziers du cercle d'un trou rond (PDF) ; vide sinon.
    pub fn circle_beziers(&self) -> Vec<CubicBezier> {
        let Some(r) = self.circle_radius() else {
            return Vec::new();
        };
        CircleTrace::new(self.pos, r, 0.0, true)
            .map(|c| PathGeom::Circle(c).arc_beziers())
            .unwrap_or_default()
    }
}

/// Épaisseur (mm) du trait des boutons et boutonnières.
pub const BUTTON_STROKE_MM: f64 = MARKING_STROKE_MM;

/// Segment d'un contour de fente exporté : droite vers un point, ou courbe de Bézier.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SlotSegment {
    Line(PointMm),
    Curve(CubicBezier),
}

/// Bouton ou boutonnière (US-06.4) résolu en mm : un cercle de diamètre donné, ou une
/// fente (rectangle à bouts arrondis) de longueur × largeur tournée de son angle.
/// Exporté dans un groupe nommé distinct des marquages, avec une forme reconnaissable
/// sans la couleur.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SceneButton {
    pub pos: PointMm,
    pub shape: ButtonShape,
}

impl SceneButton {
    /// Nom de la forme dans les fichiers exportés (attribut `data-form` du SVG).
    pub fn form_id(&self) -> &'static str {
        match self.shape {
            ButtonShape::Button { .. } => "bouton",
            ButtonShape::Buttonhole { .. } => "boutonniere",
        }
    }

    /// Rayon (mm) du bouton ; `None` pour une boutonnière.
    pub fn circle_radius(&self) -> Option<f64> {
        match self.shape {
            ButtonShape::Button { diameter_mm } => Some(diameter_mm / 2.0),
            ButtonShape::Buttonhole { .. } => None,
        }
    }

    /// Fente de la boutonnière ; `None` pour un bouton.
    pub fn slot(&self) -> Option<Slot> {
        self.shape.slot(self.pos)
    }

    /// Demi-emprise (mm) selon X puis selon Y.
    pub fn half_extent_mm(&self) -> (f64, f64) {
        self.shape.half_extent(self.pos)
    }

    /// Béziers du cercle du bouton (PDF) ; vide pour une boutonnière.
    pub fn circle_beziers(&self) -> Vec<CubicBezier> {
        let Some(r) = self.circle_radius() else {
            return Vec::new();
        };
        CircleTrace::new(self.pos, r, 0.0, true)
            .map(|c| PathGeom::Circle(c).arc_beziers())
            .unwrap_or_default()
    }

    /// Contour fermé de la fente : point de départ puis segments (PDF) ; `None` pour un
    /// bouton. Les bouts sont de vrais demi-cercles (Béziers à 0,002 mm près).
    pub fn slot_outline(&self) -> Option<(PointMm, Vec<SlotSegment>)> {
        self.slot().map(slot_path)
    }
}

/// Contour fermé d'une fente : point de départ puis segments (PDF). Les bouts sont de vrais
/// demi-cercles (Béziers à 0,002 mm près).
fn slot_path(slot: Slot) -> (PointMm, Vec<SlotSegment>) {
    let edges = slot.outline();
    let start = edges[0].start();
    let mut segments = Vec::new();
    for e in edges {
        match e {
            EdgeGeom::Line(_, b) => segments.push(SlotSegment::Line(b)),
            EdgeGeom::Arc(a) => segments.extend(
                a.to_beziers(ARC_EXPORT_TOLERANCE_MM)
                    .into_iter()
                    .map(SlotSegment::Curve),
            ),
            EdgeGeom::Cubic(c) => segments.push(SlotSegment::Curve(c)),
        }
    }
    (start, segments)
}

/// Contenu d'une feuille prêt à exporter, entièrement résolu en mm.
#[derive(Debug, Clone, PartialEq)]
pub struct Scene {
    pub project_name: String,
    pub sheet_name: String,
    pub paths: Vec<ScenePath>,
    pub points: Vec<ScenePoint>,
    /// Traits de crans, chacun du pied (sur le bord) à la pointe, en mm.
    pub notch_marks: Vec<(PointMm, PointMm)>,
    /// Cotations (mesures affichées, non imposées).
    pub dimensions: Vec<SceneDimension>,
    /// Cotes complètes (US-06.7) : longueur développée, angle, rayon, diamètre, projections,
    /// avec texte orienté et légende. Vide = rien n'est ajouté à l'export (les fichiers sans
    /// cote complète sont identiques à l'octet).
    pub full_dimensions: Vec<SceneFullDimension>,
    /// Marquages de pièce (US-06.3) : perçages, pointages, croix. Vide = rien n'est
    /// ajouté à l'export (les fichiers sans marquage sont identiques à l'octet).
    pub markings: Vec<SceneMarking>,
    /// Trous des lignes de piqûre (US-06.9). Vide = rien n'est ajouté à l'export (les fichiers
    /// sans ligne de piqûre sont identiques à l'octet).
    pub stitch_holes: Vec<SceneStitchHole>,
    /// Zones de parage, de collage et de découpe interne (US-06.10) : surface, motif et étiquette
    /// par type. Vide = rien n'est ajouté à l'export (les fichiers sans zone sont identiques à
    /// l'octet).
    pub zones: Vec<SceneZone>,
    /// Boutons et boutonnières (US-06.4). Vide = rien n'est ajouté à l'export.
    pub buttons: Vec<SceneButton>,
    /// Contraintes de rotation des droit-fil (US-13.2) : une par ligne de droit-fil. Vide =
    /// aucune pièce n'a de droit-fil, la feuille est librement tournable à l'export.
    pub grain_limits: Vec<GrainLimit>,
    /// Contours de coupe **calculés** (US-05.4) des pièces qui ont une marge de couture. Vide =
    /// rien n'est ajouté à l'export (les fichiers sans marge sont identiques à l'octet).
    pub cut_contours: Vec<SceneCutContour>,
    /// Contours de coupe qu'il a été impossible de calculer : la coupe de ces pièces est omise
    /// de l'export (jamais en silence : l'interface et l'appelant les lisent ici).
    pub cut_warnings: Vec<CutWarning>,
    /// Jambes des pinces calculables (US-07.1). Vide = rien n'est ajouté à l'export (les fichiers
    /// sans pince sont identiques à l'octet). La ligne pliée / le capuchon n'est **jamais** écrit.
    pub darts: Vec<SceneDart>,
    /// Pinces non calculables (`PieceIssue::DartInvalid`) : omises de l'export, signalées ici.
    pub dart_warnings: Vec<DartWarning>,
    /// Étiquettes de pièce (fiche renseignée seulement). Vide = rien n'est ajouté à l'export.
    pub piece_labels: Vec<ScenePieceLabel>,
    /// Bords « sur le pli » (PH-5b) : ligne de pliure et libellé. Vide = rien n'est ajouté à
    /// l'export (les fichiers sans pliure déclarée sont identiques à l'octet). Un bord sur le pli
    /// n'est écrit ni comme couture ni comme coupe : seulement ici.
    pub folds: Vec<SceneFold>,
    /// Libellés « Rempli 8 mm » (PH-5c), un par bord à rempli ; catégorie des étiquettes de pièce.
    /// Vide = rien n'est ajouté à l'export (les fichiers sans rempli sont identiques à l'octet).
    pub fold_overs: Vec<SceneFoldOver>,
}

/// Épaisseur (mm) et pointillé (mm) d'un bord sur le pli : trait très long / point, distinct de
/// tous les rôles de tracé, des jambes de pince et de la coupe calculée (jamais la couleur seule).
pub const FOLD_EDGE_STROKE_MM: f64 = 0.5;
pub const FOLD_EDGE_DASH_MM: &[f64] = &[20.0, 3.0, 1.0, 3.0];
/// Hauteur de corps (mm) du libellé « SUR LE PLI » et écart (mm) entre la pliure et le libellé.
pub const FOLD_LABEL_TEXT_MM: f64 = 3.5;
pub const FOLD_LABEL_GAP_MM: f64 = 1.5;

/// Largeur approchée (mm) d'un libellé de pliure (majuscules larges : 0,62 em par caractère).
pub fn fold_label_width_mm(label: &str) -> f64 {
    label.chars().count() as f64 * FOLD_LABEL_TEXT_MM * 0.62
}

/// Centre du libellé d'une pliure `from → to` (contour anti-horaire : l'intérieur est à gauche) :
/// milieu du bord, côté intérieur, à [`FOLD_LABEL_GAP_MM`] de la ligne. Le texte est **horizontal**
/// (toujours lisible) : le décalage tient compte de sa largeur et de sa hauteur selon l'orientation
/// du bord. Même repère que `from` et `to` (mm de la feuille, du plan DXF ou de la page).
pub fn fold_label_center(from: PointMm, to: PointMm, label: &str) -> PointMm {
    let mid = PointMm::new((from.x + to.x) / 2.0, (from.y + to.y) / 2.0);
    let Ok(u) = (to - from).unit() else {
        return mid;
    };
    edge_label_center(mid, pf_geometry::VectorMm::new(-u.y, u.x), label)
}

/// Centre d'un libellé **horizontal** posé contre un bord : `at` est le point du bord, `inward` la
/// normale unitaire côté intérieur. Le décalage tient compte de la largeur et de la hauteur du
/// texte selon l'orientation du bord (logique de [`fold_label_center`], réutilisée par PH-5c).
pub fn edge_label_center(at: PointMm, inward: pf_geometry::VectorMm, label: &str) -> PointMm {
    let (hw, hh) = (fold_label_width_mm(label) / 2.0, FOLD_LABEL_TEXT_MM / 2.0);
    let off = FOLD_LABEL_GAP_MM + inward.x.abs() * hw + inward.y.abs() * hh;
    PointMm::new(at.x + inward.x * off, at.y + inward.y * off)
}

/// Rectangle (min, max) d'un libellé de bord centré en `c`.
fn edge_label_rect(c: PointMm, label: &str) -> (PointMm, PointMm) {
    let (hw, hh) = (fold_label_width_mm(label) / 2.0, FOLD_LABEL_TEXT_MM / 2.0);
    (
        PointMm::new(c.x - hw, c.y - hh),
        PointMm::new(c.x + hw, c.y + hh),
    )
}

fn rects_overlap(a: &(PointMm, PointMm), b: &(PointMm, PointMm)) -> bool {
    a.0.x < b.1.x && b.0.x < a.1.x && a.0.y < b.1.y && b.0.y < a.1.y
}

/// Pose le libellé d'un bord (PH-5c) : au milieu du bord (`at`, normale **extérieure** `outward`,
/// longueur développée `length_mm`), côté intérieur ; s'il recouvre un obstacle (autre étiquette),
/// il glisse le long de la tangente par pas de [`FOLD_LABEL_TEXT_MM`], alternativement de chaque
/// côté, sans sortir de la demi-longueur du bord ; sans place libre il reste au milieu. Retourne le
/// point du bord retenu et la normale intérieure. Repère de la feuille, texte horizontal.
pub fn place_edge_label(
    at: PointMm,
    outward: pf_geometry::VectorMm,
    length_mm: f64,
    label: &str,
    obstacles: &[(PointMm, PointMm)],
) -> (PointMm, pf_geometry::VectorMm) {
    let inward = pf_geometry::VectorMm::new(-outward.x, -outward.y);
    let tangent = pf_geometry::VectorMm::new(-outward.y, outward.x);
    let slack = (length_mm / 2.0 - fold_label_width_mm(label) / 2.0).max(0.0);
    let steps = (slack / FOLD_LABEL_TEXT_MM).floor() as i64;
    let free = |p: PointMm| {
        let r = edge_label_rect(edge_label_center(p, inward, label), label);
        !obstacles.iter().any(|o| rects_overlap(&r, o))
    };
    if free(at) {
        return (at, inward);
    }
    for j in 1..=steps {
        for sign in [1.0, -1.0] {
            let p = at + tangent * (sign * j as f64 * FOLD_LABEL_TEXT_MM);
            if free(p) {
                return (p, inward);
            }
        }
    }
    (at, inward)
}

/// Valeur (mm) d'un rempli écrite au plus court : trois décimales au plus, zéros finaux retirés
/// (8 → « 8 », 6,5 → « 6,5 » en français, « 6.5 » en anglais).
pub fn fold_over_value_text(value_mm: f64, lang: pf_i18n::Lang) -> String {
    let t = format!("{value_mm:.3}");
    let t = t.trim_end_matches('0').trim_end_matches('.');
    t.replace('.', &lang.decimal_separator().to_string())
}

/// Libellé d'un rempli : « Rempli 8 mm » / « Fold-over 8 mm ».
pub fn fold_over_label_text(value_mm: f64, lang: pf_i18n::Lang) -> String {
    pf_i18n::lookup(lang, "export.foldover.label")
        .replace("{value}", &fold_over_value_text(value_mm, lang))
}

/// Libellé « Rempli 8 mm » d'un bord à rempli (PH-5c) : texte horizontal, côté intérieur, au
/// milieu du bord (`at`, normale intérieure `inward`). Un bord sans rempli n'en a pas.
#[derive(Debug, Clone, PartialEq)]
pub struct SceneFoldOver {
    pub piece: String,
    pub at: PointMm,
    pub inward: pf_geometry::VectorMm,
    pub label: String,
}

impl SceneFoldOver {
    /// Centre du libellé (voir [`edge_label_center`]).
    pub fn text_center(&self) -> PointMm {
        edge_label_center(self.at, self.inward, &self.label)
    }

    /// Début (à gauche) de la ligne de base du libellé.
    pub fn baseline_start(&self) -> PointMm {
        let c = self.text_center();
        PointMm::new(
            c.x - fold_label_width_mm(&self.label) / 2.0,
            c.y - FOLD_LABEL_TEXT_MM * 0.35,
        )
    }

    /// Rectangle (min, max) du libellé, pour l'emprise.
    pub fn text_rect(&self) -> (PointMm, PointMm) {
        edge_label_rect(self.text_center(), &self.label)
    }
}

/// Bord sur le pli résolu en mm : la ligne de pliure (anti-horaire, intérieur à gauche de
/// `from → to`) et le libellé dans la langue de l'export.
#[derive(Debug, Clone, PartialEq)]
pub struct SceneFold {
    pub piece: String,
    pub from: PointMm,
    pub to: PointMm,
    pub label: String,
}

impl SceneFold {
    /// Centre du libellé (voir [`fold_label_center`]).
    pub fn text_center(&self) -> PointMm {
        fold_label_center(self.from, self.to, &self.label)
    }

    /// Début (à gauche) de la ligne de base du libellé.
    pub fn baseline_start(&self) -> PointMm {
        let c = self.text_center();
        PointMm::new(
            c.x - fold_label_width_mm(&self.label) / 2.0,
            c.y - FOLD_LABEL_TEXT_MM * 0.35,
        )
    }

    /// Rectangle (min, max) du libellé, pour l'emprise.
    pub fn text_rect(&self) -> (PointMm, PointMm) {
        let c = self.text_center();
        let (hw, hh) = (
            fold_label_width_mm(&self.label) / 2.0,
            FOLD_LABEL_TEXT_MM / 2.0,
        );
        (
            PointMm::new(c.x - hw, c.y - hh),
            PointMm::new(c.x + hw, c.y + hh),
        )
    }
}

/// Épaisseur (mm) et pointillé (mm) des jambes d'une pince : trait long / point / point, distinct
/// de tous les rôles de tracé (jamais la couleur seule).
pub const DART_STROKE_MM: f64 = 0.4;
pub const DART_DASH_MM: &[f64] = &[8.0, 1.5, 1.5, 1.5, 1.5, 1.5];

/// Jambes d'une pince en mm : sommet → pied gauche et sommet → pied droit (égales par
/// construction). Aperçu de fermeture et capuchon : jamais exportés.
#[derive(Debug, Clone, PartialEq)]
pub struct SceneDart {
    pub piece: String,
    pub apex: PointMm,
    pub left_foot: PointMm,
    pub right_foot: PointMm,
}

impl SceneDart {
    /// Les deux jambes, gauche puis droite.
    pub fn legs(&self) -> [(PointMm, PointMm); 2] {
        [(self.apex, self.left_foot), (self.apex, self.right_foot)]
    }
}

/// Pince qu'il a été impossible de calculer : elle n'est pas exportée.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartWarning {
    pub piece: String,
    pub reason: String,
}

impl fmt::Display for DartWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "pince de la pièce « {} » non exportée : {}",
            self.piece, self.reason
        )
    }
}

/// Hauteur de corps (mm) du texte d'une étiquette de pièce, et pas entre deux lignes.
pub const LABEL_TEXT_MM: f64 = 4.0;
pub const LABEL_LINE_PITCH_MM: f64 = 5.0;
/// Longueur maximale (caractères) d'une ligne d'étiquette ; au-delà « … » termine la ligne.
pub const LABEL_MAX_CHARS: usize = 60;
/// Nombre maximal de lignes de commentaire écrites.
pub const LABEL_MAX_COMMENT_LINES: usize = 6;
/// Espace (mm) entre le droit-fil et le bloc de texte de l'étiquette.
pub const LABEL_GRAIN_GAP_MM: f64 = 3.0;

/// Étiquette d'une pièce : bloc de lignes centré sur `center` (convention : voir
/// [`piece_label_center`]), lignes lues du haut vers le bas.
#[derive(Debug, Clone, PartialEq)]
pub struct ScenePieceLabel {
    pub piece: String,
    pub center: PointMm,
    pub lines: Vec<String>,
}

impl ScenePieceLabel {
    /// Largeur approchée (mm) d'une ligne.
    pub fn line_width_mm(line: &str) -> f64 {
        line.chars().count() as f64 * LABEL_TEXT_MM * 0.55
    }

    /// Largeur approchée du bloc (la plus longue ligne).
    pub fn width_mm(&self) -> f64 {
        self.lines
            .iter()
            .map(|l| Self::line_width_mm(l))
            .fold(0.0, f64::max)
    }

    /// Milieu de la ligne de base de la ligne `i` (0 = en haut).
    pub fn baseline_center(&self, i: usize) -> PointMm {
        let n = self.lines.len() as f64;
        PointMm::new(
            self.center.x,
            self.center.y + ((n - 1.0) / 2.0 - i as f64) * LABEL_LINE_PITCH_MM
                - LABEL_TEXT_MM * 0.35,
        )
    }

    /// Début (à gauche) de la ligne de base de la ligne `i`.
    pub fn baseline_start(&self, i: usize) -> PointMm {
        let b = self.baseline_center(i);
        PointMm::new(b.x - Self::line_width_mm(&self.lines[i]) / 2.0, b.y)
    }

    /// Rectangle (min, max) du bloc, pour l'emprise.
    pub fn rect(&self) -> (PointMm, PointMm) {
        let (hw, hh) = (
            self.width_mm() / 2.0,
            self.lines.len() as f64 * LABEL_LINE_PITCH_MM / 2.0,
        );
        (
            PointMm::new(self.center.x - hw, self.center.y - hh),
            PointMm::new(self.center.x + hw, self.center.y + hh),
        )
    }
}

fn label_clip(text: &str) -> String {
    let flat: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let t = flat.trim();
    if t.chars().count() <= LABEL_MAX_CHARS {
        t.to_string()
    } else {
        let s: String = t.chars().take(LABEL_MAX_CHARS - 1).collect();
        format!("{s}…")
    }
}

fn category_label_fr(c: pf_core::PieceCategory) -> &'static str {
    match c {
        pf_core::PieceCategory::Fabric => "Tissu",
        pf_core::PieceCategory::Lining => "Doublure",
        pf_core::PieceCategory::Interfacing => "Entoilage",
        pf_core::PieceCategory::Leather => "Cuir",
        pf_core::PieceCategory::Reinforcement => "Renfort",
    }
}

/// Lignes de l'étiquette d'une pièce : nom, code, quantité (avec « paire, ×2 en miroir »),
/// matière, catégorie, puis les lignes du commentaire. **Vide** tant que la fiche n'est pas
/// renseignée (ni code, ni matière, ni catégorie, ni commentaire, quantité 1, pas de paire) :
/// une pièce qui n'a que son nom s'exporte comme avant, à l'octet. Un champ vide n'écrit rien.
pub fn piece_label_lines(p: &pf_core::Piece) -> Vec<String> {
    fn text(o: &Option<String>) -> Option<&str> {
        o.as_deref().map(str::trim).filter(|s| !s.is_empty())
    }
    let (code, material, comment) = (text(&p.code), text(&p.material), text(&p.comment));
    if code.is_none()
        && material.is_none()
        && comment.is_none()
        && p.category.is_none()
        && p.quantity == 1
        && !p.mirrored_pair
    {
        return Vec::new();
    }
    let mut lines = Vec::new();
    let name = label_clip(&p.name);
    if !name.is_empty() {
        lines.push(name);
    }
    if let Some(c) = code {
        lines.push(label_clip(&format!("Code : {c}")));
    }
    let mut q = format!("Quantité : {}", p.quantity);
    if p.mirrored_pair {
        q.push_str(" (paire, ×2 en miroir)");
    }
    lines.push(q);
    if let Some(m) = material {
        lines.push(label_clip(&format!("Matière : {m}")));
    }
    if let Some(c) = p.category {
        lines.push(format!("Catégorie : {}", category_label_fr(c)));
    }
    if let Some(c) = comment {
        lines.extend(
            c.lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .take(LABEL_MAX_COMMENT_LINES)
                .map(label_clip),
        );
    }
    lines
}

/// Centre du bloc d'étiquette. **Convention** : milieu de l'emprise du contour de la pièce ; si
/// la pièce a un droit-fil, milieu du droit-fil décalé, à gauche de son sens, de la demi-largeur
/// du bloc + [`LABEL_GRAIN_GAP_MM`] (le texte longe le droit-fil sans le recouvrir).
fn piece_label_center(project: &Project, piece: &pf_core::Piece, width_mm: f64) -> Option<PointMm> {
    if let Some(g) = project.piece_grain_lines(piece.id).first() {
        let mid = PointMm::new((g.from.x + g.to.x) / 2.0, (g.from.y + g.to.y) / 2.0);
        if let Ok(u) = (g.to - g.from).unit() {
            let off = width_mm / 2.0 + LABEL_GRAIN_GAP_MM;
            return Some(PointMm::new(mid.x - u.y * off, mid.y + u.x * off));
        }
    }
    let sheet = project.piece_sheet(piece.id).ok()?;
    let mut bb: Option<Bbox> = None;
    for e in &piece.outline {
        let b = sheet.shape(e.entity).ok()?.bbox();
        match bb.as_mut() {
            Some(t) => {
                t.include(b.min);
                t.include(b.max);
            }
            None => bb = Some(b),
        }
    }
    let bb = bb?;
    Some(PointMm::new(
        (bb.min.x + bb.max.x) / 2.0,
        (bb.min.y + bb.max.y) / 2.0,
    ))
}

/// Pinces et étiquettes des pièces de la feuille. Une pièce dont tous les tracés de contour sont
/// masqués (calque masqué) n'écrit ni pince ni étiquette, sauf `all`. Une pièce sans contour n'a
/// pas d'étiquette (aucun emplacement).
fn piece_darts_and_labels(
    project: &Project,
    sheet: SheetId,
    all: bool,
    lang: pf_i18n::Lang,
) -> (Vec<SceneDart>, Vec<DartWarning>, Vec<ScenePieceLabel>) {
    let (mut darts, mut warnings, mut labels) = (Vec::new(), Vec::new(), Vec::new());
    let Ok(s) = project.sheet(sheet) else {
        return (darts, warnings, labels);
    };
    for piece in project.pieces_on(sheet) {
        if !all
            && !piece.outline.is_empty()
            && piece.outline.iter().all(|o| !s.entity_visible(o.entity))
        {
            continue;
        }
        for r in project.piece_darts(piece.id) {
            match r.calc {
                Ok(c) => darts.push(SceneDart {
                    piece: piece.name.clone(),
                    apex: c.shape.apex,
                    left_foot: c.shape.left_foot,
                    right_foot: c.shape.right_foot,
                }),
                Err(reason) => warnings.push(DartWarning {
                    piece: piece.name.clone(),
                    reason,
                }),
            }
        }
        let mut lines = piece_label_lines(piece);
        // Coupe nette (PH-5c) puis « Sans droit-fil » (PH-5d) : mentions dans l'étiquette, dernières
        // lignes ; une pièce dont la fiche n'est pas renseignée reçoit alors son nom puis la mention.
        if piece.net_cut || piece.no_grain {
            if lines.is_empty() {
                lines.extend(Some(label_clip(&piece.name)).filter(|n| !n.is_empty()));
            }
            if piece.net_cut {
                lines.push(pf_i18n::lookup(lang, "export.netcut.label"));
            }
            if piece.no_grain {
                lines.push(pf_i18n::lookup(lang, "export.nograin.label"));
            }
        }
        if lines.is_empty() {
            continue;
        }
        let width = lines
            .iter()
            .map(|l| ScenePieceLabel::line_width_mm(l))
            .fold(0.0, f64::max);
        if let Some(center) = piece_label_center(project, piece, width) {
            labels.push(ScenePieceLabel {
                piece: piece.name.clone(),
                center,
                lines,
            });
        }
    }
    (darts, warnings, labels)
}

/// Libellés « Rempli 8 mm » (PH-5c) des pièces de la feuille : un par bord à rempli de valeur non
/// nulle, au milieu du bord (lecture `piece_fold_label_anchors`, rien n'est recalculé ni stocké),
/// indépendant du mode couture / coupe. Même visibilité que l'étiquette de pièce. Les autres
/// étiquettes (pièce, pliure, rempliés déjà posés) sont des obstacles : le libellé glisse le long du
/// bord pour ne pas les recouvrir.
fn piece_fold_overs(
    project: &Project,
    sheet: SheetId,
    lang: pf_i18n::Lang,
    all: bool,
    labels: &[ScenePieceLabel],
    folds: &[SceneFold],
) -> Vec<SceneFoldOver> {
    let mut out: Vec<SceneFoldOver> = Vec::new();
    let Ok(s) = project.sheet(sheet) else {
        return out;
    };
    let mut obstacles: Vec<(PointMm, PointMm)> = labels
        .iter()
        .map(ScenePieceLabel::rect)
        .chain(folds.iter().map(SceneFold::text_rect))
        .collect();
    for piece in project.pieces_on(sheet) {
        if !all
            && !piece.outline.is_empty()
            && piece.outline.iter().all(|o| !s.entity_visible(o.entity))
        {
            continue;
        }
        for a in project.piece_fold_label_anchors(piece.id) {
            let label = fold_over_label_text(a.value_mm, lang);
            let (at, inward) = place_edge_label(a.point, a.normal, a.length_mm, &label, &obstacles);
            let item = SceneFoldOver {
                piece: piece.name.clone(),
                at,
                inward,
                label,
            };
            obstacles.push(item.text_rect());
            out.push(item);
        }
    }
    out
}

/// Épaisseur (mm) et pointillé (mm) du contour de coupe **calculé** : trait long / trait court,
/// comme l'écran (« coupe calculée »), distinct de la couture (pointillé 3 / 2), de la coupe
/// dessinée (trait plein) et de tous les autres rôles (jamais la couleur seule).
pub const COMPUTED_CUT_STROKE_MM: f64 = 0.6;
pub const COMPUTED_CUT_DASH_MM: &[f64] = &[10.0, 2.5, 2.5, 2.5];
/// Longueur (mm) d'un cran de rempli exporté, du contour de coupe vers la couture (bornée par la
/// valeur du rempli : le trait ne dépasse jamais la couture).
pub const FOLD_NOTCH_EXPORT_MM: f64 = 5.0;

/// Ce que l'export fait des deux contours d'une pièce qui a une marge de couture (US-05.4).
/// Une pièce **sans** marge n'est jamais dédoublée : son contour (couture ou coupe tel que
/// dessiné) est exporté une seule fois, quel que soit le mode. Une pièce **« coupe nette »**
/// (US-05.6, contour de couture = contour de coupe, déclaré) aussi : écrite une fois, comme coupe
/// en « coupe seule » et « les deux », comme couture en « couture seule ».
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PieceExportMode {
    /// Couture (tracés du contour) **et** coupe calculée.
    #[default]
    Both,
    /// Contour de couture seul ; aucun calcul de coupe.
    SewingOnly,
    /// Contour de coupe calculé seul ; le contour de couture n'est pas écrit.
    CutOnly,
}

impl PieceExportMode {
    pub const ALL: [PieceExportMode; 3] = [Self::Both, Self::SewingOnly, Self::CutOnly];

    /// Identifiant stable (préférences, clés d'interface).
    pub fn key(self) -> &'static str {
        match self {
            Self::Both => "both",
            Self::SewingOnly => "sewing",
            Self::CutOnly => "cut",
        }
    }

    pub fn from_key(key: &str) -> Option<PieceExportMode> {
        Self::ALL.into_iter().find(|m| m.key() == key)
    }

    pub fn includes_sewing(self) -> bool {
        self != Self::CutOnly
    }

    pub fn includes_cut(self) -> bool {
        self != Self::SewingOnly
    }
}

/// Contour de coupe calculé d'une pièce, en mm : chaîne fermée anti-horaire d'arêtes
/// (`Line`, `Arc`, `Cubic`) et petits traits des crans de rempli, chacun du contour de coupe
/// vers l'intérieur (vers la couture).
#[derive(Debug, Clone, PartialEq)]
pub struct SceneCutContour {
    pub piece: String,
    pub edges: Vec<PathGeom>,
    /// Vrai : chaîne fermée (le dernier bord rejoint le premier) ; faux (PH-5b) : suite ouverte,
    /// un bord sur le pli ayant été retiré de la coupe (une pièce peut en avoir plusieurs).
    pub closed: bool,
    pub notch_marks: Vec<(PointMm, PointMm)>,
    /// Crans posés **sur la coupe** (US-06.1) : forme du cran sur le contour de coupe, segment par
    /// segment, du côté de la coupe vers la couture. Distincts des crans de rempli.
    pub cut_notch_marks: Vec<(PointMm, PointMm)>,
}

/// Élément du contour de coupe qu'il a été impossible de calculer : il n'est pas exporté.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CutWarning {
    pub piece: String,
    pub reason: String,
    /// Faux : le contour de coupe entier est omis. Vrai : seuls les crans de rempli le sont (le
    /// contour de coupe, lui, est exporté).
    pub notches_only: bool,
    /// Vrai : seuls des crans **sur la coupe** (US-06.1) sont omis (l'emporte sur `notches_only`).
    pub cut_notches: bool,
    /// Le contour de couture a été exporté à la place (mode « coupe seule », coupe omise).
    pub sewing_kept: bool,
}

impl fmt::Display for CutWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let what = if self.cut_notches {
            "crans sur la coupe"
        } else if self.notches_only {
            "crans de rempli"
        } else {
            "contour de coupe"
        };
        write!(
            f,
            "{what} de la pièce « {} » non exporté : {}{}",
            self.piece,
            self.reason,
            if self.sewing_kept {
                " (le contour de couture est exporté à la place)"
            } else {
                ""
            }
        )
    }
}

/// Rotation que le droit-fil d'une ligne accepte (US-13.2), lue dans le modèle : la tolérance
/// de rotation de sa pièce et le fait que la ligne ait un sens (poil, motif) ou non (axe).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GrainLimit {
    /// Ligne orientée : une rotation de 180° la retourne (déviation de 180°).
    pub oriented: bool,
    /// Rotation libre acceptée, en degrés (0 si la pièce n'en déclare aucune).
    pub tolerance_deg: f64,
}

/// Options de construction d'une scène.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SceneOptions {
    /// Couture et/ou coupe calculée des pièces à marge (US-05.4) ; `Both` par défaut.
    pub piece_mode: PieceExportMode,
    /// Inclure les objets des calques masqués (faux par défaut).
    pub include_hidden_layers: bool,
    /// Inclure les lignes d'aide (US-02.8), tracées comme des traits d'axe
    /// limités à l'emprise du contenu + [`GUIDE_MARGIN_MM`] (faux par défaut :
    /// un guide n'est jamais exporté sans demande explicite).
    pub include_guides: bool,
    /// Langue du libellé des bords sur le pli (« SUR LE PLI » / « ON FOLD »). Français par défaut.
    pub lang: pf_i18n::Lang,
}

/// Dépassement (mm) des lignes d'aide exportées au-delà de l'emprise du contenu.
pub const GUIDE_MARGIN_MM: f64 = 10.0;

impl Scene {
    /// Scène d'une feuille ; les calques masqués sont exclus.
    pub fn from_sheet(project: &Project, sheet: SheetId) -> Result<Scene, ExportError> {
        Self::from_sheet_with(project, sheet, SceneOptions::default())
    }

    pub fn from_sheet_with(
        project: &Project,
        sheet: SheetId,
        options: SceneOptions,
    ) -> Result<Scene, ExportError> {
        // Lecture de masse : chaque point de la feuille n'est calculé qu'une fois.
        project.sheet(sheet).map_or_else(
            |_| Self::build_from_sheet(project, sheet, options),
            |s| s.with_read_cache(|| Self::build_from_sheet(project, sheet, options)),
        )
    }

    fn build_from_sheet(
        project: &Project,
        sheet: SheetId,
        options: SceneOptions,
    ) -> Result<Scene, ExportError> {
        project.validate()?;
        let s = project.sheet(sheet)?;
        let all = options.include_hidden_layers;
        let mut paths = Vec::new();
        // Lignes de droit-fil des pièces qui portent des réglages avancés (US-06.5) : la
        // flèche calculée remplace le tracé dessiné. Vide = aucun changement à l'export.
        let grain_lines = project.sheet_grain_lines(sheet);
        let (cut_contours, cut_warnings, skipped) =
            piece_cut_contours(project, sheet, options.piece_mode, all);
        let mut cut_warnings = cut_warnings;
        // Coupe nette (US-05.6) : contour de couture = contour de coupe, déclaré. Les tracés du
        // contour sont écrits **une seule fois**, avec le rôle que demande le mode : coupe
        // (« coupe seule » et « les deux ») ou couture (« couture seule »).
        let net_role = if options.piece_mode.includes_cut() {
            PathRole::CuttingContour
        } else {
            PathRole::SewingContour
        };
        let net_cut_edges: Vec<pf_core::EntityId> = project
            .pieces_on(sheet)
            .into_iter()
            .filter(|p| p.net_cut)
            .flat_map(|p| p.outline.iter().map(|o| o.entity))
            .collect();
        // Bords sur le pli (PH-5b) : écrits comme pliure seulement, ni couture ni coupe.
        let (folds, fold_entities) = piece_folds(project, sheet, options, all);
        for e in s.entities.iter().filter(|e| all || s.entity_visible(e.id)) {
            if skipped.contains(&e.id) || fold_entities.contains(&e.id) {
                continue;
            }
            let role = if net_cut_edges.contains(&e.id) {
                net_role
            } else {
                e.role
            };
            if let Some(g) = grain_lines.iter().find(|g| g.entity == e.id) {
                paths.push(ScenePath {
                    geom: PathGeom::Line(g.from, g.to),
                    role,
                    sense: g.spec.sense,
                });
                continue;
            }
            let geom = match s.shape(e.id)? {
                Shape::Line(a, b) => PathGeom::Line(a, b),
                Shape::Cubic(c) => PathGeom::Cubic(c),
                Shape::Arc(a) => PathGeom::Arc(a),
                Shape::Circle(c) => PathGeom::Circle(c),
                Shape::Spline(sp) => PathGeom::Spline(sp.segments().to_vec()),
            };
            paths.push(ScenePath {
                geom,
                role,
                sense: GrainSense::Unmarked,
            });
        }
        let mut points = Vec::new();
        for p in s.points.iter().filter(|p| all || s.point_visible(p.id)) {
            points.push(ScenePoint {
                pos: s.position(p.id)?,
                label: p.label.clone(),
                show_label: true,
            });
        }
        if paths.is_empty() && points.is_empty() {
            return Err(ExportError::EmptySheet(s.name.clone()));
        }
        let mut dimensions = Vec::new();
        for d in s
            .dimensions
            .iter()
            .filter(|d| all || s.dimension_visible(d))
        {
            let g = s.dimension_geometry(d.id)?;
            dimensions.push(SceneDimension {
                from: g.from,
                to: g.to,
                from_offset: g.from_offset,
                to_offset: g.to_offset,
                text: format!("{:.2} mm", g.value_mm),
            });
        }
        // Une cote non calculable (géométrie devenue indéfinie) n'est pas exportée.
        let full_dimensions: Vec<SceneFullDimension> = s
            .full_dimensions
            .iter()
            .filter(|d| all || s.full_dimension_visible(d))
            .filter_map(|d| s.full_dimension_geometry(d.id).ok())
            .map(|g| SceneFullDimension::from_geometry(&g))
            .collect();
        let grain_limits = grain_limits_of(project, sheet);
        // Crans : sur la couture ici ; ceux « sur la coupe » d'une pièce à marge sont dans son
        // contour de coupe (`piece_cut_contours`), les autres (pièce sans marge, coupe nette) sont
        // projetés ici, le contour de coupe étant alors la couture. Un échec est signalé.
        let mut notch_marks = Vec::new();
        for n in s
            .notches
            .iter()
            .filter(|n| all || s.notch_visible(n))
            .filter(|n| {
                !(n.on_cut
                    && project
                        .notch_piece(sheet, n)
                        .is_some_and(|p| p.seam.is_some() && !p.net_cut))
            })
        {
            match project.notch_placed_marks(sheet, n) {
                Ok((_, marks)) => notch_marks.extend(marks),
                Err(e) if n.on_cut => cut_warnings.push(CutWarning {
                    piece: project
                        .notch_piece(sheet, n)
                        .map_or_else(|| s.name.clone(), |p| p.name.clone()),
                    reason: e.to_string(),
                    notches_only: true,
                    cut_notches: true,
                    sewing_kept: false,
                }),
                Err(_) => {}
            }
        }
        let (darts, dart_warnings, piece_labels) =
            piece_darts_and_labels(project, sheet, all, options.lang);
        let fold_overs = piece_fold_overs(project, sheet, options.lang, all, &piece_labels, &folds);
        let mut scene = Scene {
            project_name: project.name.clone(),
            sheet_name: s.name.clone(),
            paths,
            points,
            notch_marks,
            dimensions,
            full_dimensions,
            // Les marquages tiennent à la pièce, pas aux calques de la feuille.
            markings: project
                .sheet_markings(sheet)
                .into_iter()
                .map(|(pos, shape)| SceneMarking { pos, shape })
                .collect(),
            stitch_holes: project
                .sheet_stitch_holes(sheet)
                .into_iter()
                .map(|(pos, hole, tangent_deg)| SceneStitchHole {
                    pos,
                    hole,
                    tangent_deg,
                })
                .collect(),
            zones: project
                .sheet_zones(sheet)
                .into_iter()
                .map(|z| SceneZone {
                    kind: z.kind,
                    polygon: z.polygon,
                    width_mm: z.width_mm,
                })
                .collect(),
            buttons: project
                .sheet_buttons(sheet)
                .into_iter()
                .map(|(pos, shape)| SceneButton { pos, shape })
                .collect(),
            grain_limits,
            cut_contours,
            cut_warnings,
            darts,
            dart_warnings,
            piece_labels,
            folds,
            fold_overs,
        };
        if options.include_guides {
            // Emprise du contenu seul : les guides ne l'agrandissent pas.
            if let Some(mut bb) = scene.bbox() {
                bb.include(PointMm::new(
                    bb.min.x - GUIDE_MARGIN_MM,
                    bb.min.y - GUIDE_MARGIN_MM,
                ));
                bb.include(PointMm::new(
                    bb.max.x + GUIDE_MARGIN_MM,
                    bb.max.y + GUIDE_MARGIN_MM,
                ));
                for g in s.guides.iter().filter(|g| all || s.guide_visible(g)) {
                    if let Some((a, b)) = g.clip_to(&bb) {
                        scene.paths.push(ScenePath {
                            geom: PathGeom::Line(a, b),
                            role: PathRole::AnnotationAxis,
                            sense: GrainSense::Unmarked,
                        });
                    }
                }
            }
        }
        Ok(scene)
    }

    /// Scène étalon : un carré de 100 × 100 mm.
    pub fn calibration_square() -> Scene {
        Self::calibration_square_lang(pf_i18n::Lang::Fr)
    }

    /// Scène étalon avec le nom de feuille dans la langue `lang` (US-19.2).
    pub fn calibration_square_lang(lang: pf_i18n::Lang) -> Scene {
        let c = CALIBRATION_MM;
        let corners = [
            PointMm::new(0., 0.),
            PointMm::new(c, 0.),
            PointMm::new(c, c),
            PointMm::new(0., c),
        ];
        Scene {
            project_name: "PatternForge".into(),
            sheet_name: pf_i18n::lookup(lang, "pdfx.svg.calib_sheet"),
            paths: (0..4)
                .map(|i| ScenePath {
                    geom: PathGeom::Line(corners[i], corners[(i + 1) % 4]),
                    role: PathRole::CuttingContour,
                    sense: GrainSense::Unmarked,
                })
                .collect(),
            points: Vec::new(),
            notch_marks: Vec::new(),
            dimensions: Vec::new(),
            full_dimensions: Vec::new(),
            markings: Vec::new(),
            stitch_holes: Vec::new(),
            zones: Vec::new(),
            buttons: Vec::new(),
            grain_limits: Vec::new(),
            cut_contours: Vec::new(),
            cut_warnings: Vec::new(),
            darts: Vec::new(),
            dart_warnings: Vec::new(),
            piece_labels: Vec::new(),
            folds: Vec::new(),
            fold_overs: Vec::new(),
        }
    }

    /// Quarts de tour (0 à 3, sens trigonométrique) que l'export peut appliquer à la feuille
    /// sans qu'aucun droit-fil dépasse la tolérance de rotation de sa pièce. Le quart de tour 0
    /// est toujours permis ; une valeur douteuse (tolérance non finie) interdit la rotation.
    pub fn allowed_quarter_turns(&self) -> [bool; 4] {
        let mut ok = [true; 4];
        for (k, allowed) in ok.iter_mut().enumerate().skip(1) {
            let rotation = 90.0 * k as f64;
            *allowed = self.grain_limits.iter().all(|l| {
                pf_core::piece_grain::grain_rotation_deviation_deg(rotation, l.oriented)
                    <= l.tolerance_deg + pf_core::piece_grain::GRAIN_DEVIATION_SLACK_DEG
            });
        }
        ok
    }

    /// Boîte englobante exacte du contenu.
    pub fn bbox(&self) -> Option<Bbox> {
        let mut bb: Option<Bbox> = None;
        let mut add = |p: PointMm| match bb.as_mut() {
            Some(b) => b.include(p),
            None => bb = Some(Bbox::from_point(p)),
        };
        for path in &self.paths {
            match &path.geom {
                &PathGeom::Line(a, b) => {
                    add(a);
                    add(b);
                }
                PathGeom::Cubic(c) => {
                    let cb = c.bbox();
                    add(cb.min);
                    add(cb.max);
                }
                PathGeom::Arc(a) => {
                    let cb = a.bbox();
                    add(cb.min);
                    add(cb.max);
                }
                PathGeom::Circle(c) => {
                    let cb = c.circle.bbox();
                    add(cb.min);
                    add(cb.max);
                }
                PathGeom::Spline(segments) => {
                    for c in segments {
                        let cb = c.bbox();
                        add(cb.min);
                        add(cb.max);
                    }
                }
            }
        }
        for c in &self.cut_contours {
            for g in &c.edges {
                match g {
                    &PathGeom::Line(a, b) => {
                        add(a);
                        add(b);
                    }
                    PathGeom::Cubic(k) => {
                        let cb = k.bbox();
                        add(cb.min);
                        add(cb.max);
                    }
                    PathGeom::Arc(a) => {
                        let cb = a.bbox();
                        add(cb.min);
                        add(cb.max);
                    }
                    PathGeom::Circle(_) | PathGeom::Spline(_) => {}
                }
            }
            for (a, b) in c.notch_marks.iter().chain(&c.cut_notch_marks) {
                add(*a);
                add(*b);
            }
        }
        for p in &self.points {
            add(p.pos);
        }
        for (a, b) in &self.notch_marks {
            add(*a);
            add(*b);
        }
        for d in &self.dimensions {
            for (a, b) in d.marks() {
                add(a);
                add(b);
            }
            let (t, hw) = (d.text_anchor(), d.text_half_width_mm());
            add(PointMm::new(t.x - hw, t.y));
            add(PointMm::new(t.x + hw, t.y + DIMENSION_TEXT_MM));
        }
        for d in &self.full_dimensions {
            for (a, b) in &d.marks {
                add(*a);
                add(*b);
            }
            for c in d.text_corners() {
                add(c);
            }
        }
        for m in &self.markings {
            let h = m.half_extent_mm();
            add(PointMm::new(m.pos.x - h, m.pos.y - h));
            add(PointMm::new(m.pos.x + h, m.pos.y + h));
        }
        for m in &self.stitch_holes {
            let h = m.half_extent_mm();
            add(PointMm::new(m.pos.x - h, m.pos.y - h));
            add(PointMm::new(m.pos.x + h, m.pos.y + h));
        }
        for z in &self.zones {
            for p in &z.polygon {
                add(*p);
            }
        }
        for b in &self.buttons {
            let (hx, hy) = b.half_extent_mm();
            add(PointMm::new(b.pos.x - hx, b.pos.y - hy));
            add(PointMm::new(b.pos.x + hx, b.pos.y + hy));
        }
        for d in &self.darts {
            add(d.apex);
            add(d.left_foot);
            add(d.right_foot);
        }
        for l in &self.piece_labels {
            let (lo, hi) = l.rect();
            add(lo);
            add(hi);
        }
        for f in &self.folds {
            add(f.from);
            add(f.to);
            let (lo, hi) = f.text_rect();
            add(lo);
            add(hi);
        }
        for f in &self.fold_overs {
            let (lo, hi) = f.text_rect();
            add(lo);
            add(hi);
        }
        bb
    }

    /// Vrai si aucun tracé n'est une ligne de coupe : l'export est alors
    /// annoncé comme tracé de construction, pas comme pièce à découper.
    pub fn is_construction_only(&self) -> bool {
        self.cut_contours.is_empty()
            && !self
                .paths
                .iter()
                .any(|p| p.role == PathRole::CuttingContour)
    }

    pub fn roles_used(&self) -> Vec<PathRole> {
        PathRole::ALL
            .into_iter()
            .filter(|r| self.paths.iter().any(|p| p.role == *r))
            .collect()
    }
}

/// Contours de coupe calculés des pièces à marge de la feuille (US-05.4), avertissements, et
/// tracés du contour de couture à ne pas écrire (mode « coupe seule », coupe calculée).
/// Une pièce sans marge (`seam` absent) n'est jamais touchée : son contour reste tel que dessiné.
/// Une pièce en coupe nette (US-05.6) n'a pas de coupe calculée : ses tracés changent de rôle.
fn piece_cut_contours(
    project: &Project,
    sheet: SheetId,
    mode: PieceExportMode,
    all: bool,
) -> (
    Vec<SceneCutContour>,
    Vec<CutWarning>,
    Vec<pf_core::EntityId>,
) {
    let (mut cuts, mut warnings, mut skipped) = (Vec::new(), Vec::new(), Vec::new());
    if !mode.includes_cut() {
        return (cuts, warnings, skipped);
    }
    for piece in project.pieces_on(sheet) {
        let Some(seam) = piece.seam.as_ref() else {
            continue;
        };
        let cut = match project.piece_cut_contour(piece.id) {
            Ok(v) => v,
            Err(e) => {
                warnings.push(CutWarning {
                    piece: piece.name.clone(),
                    reason: e.to_string(),
                    notches_only: false,
                    cut_notches: false,
                    sewing_kept: mode == PieceExportMode::CutOnly,
                });
                continue;
            }
        };
        let notches = project.piece_fold_notches(piece.id).unwrap_or_else(|e| {
            warnings.push(CutWarning {
                piece: piece.name.clone(),
                reason: e.to_string(),
                notches_only: true,
                cut_notches: false,
                sewing_kept: false,
            });
            Vec::new()
        });
        // Arêtes de la coupe, chacune marquée « pliure » si elle suit un bord sur le pli (PH-5b).
        let tagged: Vec<(PathGeom, bool)> = cut
            .traces
            .iter()
            .enumerate()
            .flat_map(|(i, t)| {
                let fold = cut.fold_cuts.iter().any(|f| f.trace == i);
                pf_geometry::region::trace_geometry(t)
                    .into_iter()
                    .map(move |g| {
                        (
                            match g {
                                EdgeGeom::Line(a, b) => PathGeom::Line(a, b),
                                EdgeGeom::Arc(a) => PathGeom::Arc(a),
                                EdgeGeom::Cubic(c) => PathGeom::Cubic(c),
                            },
                            fold,
                        )
                    })
            })
            .collect();
        // Sans pliure : une chaîne fermée, exactement comme avant. Avec pliure : la portion qui
        // suit la pliure n'est pas un bord de coupe, la coupe devient une ou plusieurs suites
        // ouvertes (la première reçoit les crans).
        let runs: Vec<(Vec<PathGeom>, bool)> = if tagged.iter().any(|(_, f)| *f) {
            let n = tagged.len();
            // Départ juste après une pliure, pour que chaque suite soit continue.
            let start = (0..n)
                .find(|&i| tagged[i].1 && !tagged[(i + 1) % n].1)
                .map_or(0, |i| (i + 1) % n);
            let mut runs: Vec<(Vec<PathGeom>, bool)> = Vec::new();
            let mut current: Vec<PathGeom> = Vec::new();
            for k in 0..n {
                let (g, fold) = &tagged[(start + k) % n];
                if *fold {
                    if !current.is_empty() {
                        runs.push((std::mem::take(&mut current), false));
                    }
                } else {
                    current.push(g.clone());
                }
            }
            if !current.is_empty() {
                runs.push((current, false));
            }
            runs
        } else {
            vec![(tagged.into_iter().map(|(g, _)| g).collect(), true)]
        };
        let notch_marks: Vec<(PointMm, PointMm)> = notches
            .iter()
            .filter_map(|n| {
                let value = seam.folds.iter().find(|f| f.entity == n.entity)?.value_mm;
                let len = FOLD_NOTCH_EXPORT_MM.min(value);
                if len.partial_cmp(&1e-9) != Some(std::cmp::Ordering::Greater) {
                    return None;
                }
                let foot = n.point + n.normal * value;
                Some((foot, foot + n.normal * (-len)))
            })
            .collect();
        // Crans « sur la coupe » (US-06.1) : projetés sur ce contour, de la coupe vers la couture.
        let mut cut_notch_marks: Vec<(PointMm, PointMm)> = Vec::new();
        if let Ok(s) = project.sheet(sheet) {
            for n in s.notches.iter().filter(|n| {
                n.on_cut
                    && (all || s.notch_visible(n))
                    && project
                        .notch_piece(sheet, n)
                        .is_some_and(|p| p.id == piece.id)
            }) {
                match project.notch_on_cut_marks(sheet, n.id) {
                    Ok(marks) => cut_notch_marks.extend(marks),
                    Err(e) => warnings.push(CutWarning {
                        piece: piece.name.clone(),
                        reason: e.to_string(),
                        notches_only: true,
                        cut_notches: true,
                        sewing_kept: false,
                    }),
                }
            }
        }
        let mut notch_marks = Some(notch_marks);
        let mut cut_notch_marks = Some(cut_notch_marks);
        let mut runs = runs;
        if runs.is_empty() {
            runs.push((Vec::new(), false));
        }
        for (edges, closed) in runs {
            cuts.push(SceneCutContour {
                piece: piece.name.clone(),
                edges,
                closed,
                notch_marks: notch_marks.take().unwrap_or_default(),
                cut_notch_marks: cut_notch_marks.take().unwrap_or_default(),
            });
        }
        if mode == PieceExportMode::CutOnly {
            skipped.extend(piece.outline.iter().map(|o| o.entity));
        }
    }
    (cuts, warnings, skipped)
}

/// Bords sur le pli des pièces de la feuille (PH-5b) et tracés du contour qu'ils remplacent. La
/// ligne suit la pliure de la coupe calculée quand le mode écrit la coupe et que la pièce a une
/// marge (elle est alors prolongée jusqu'aux marges voisines), sinon c'est le bord lui-même. Un
/// bord dont le tracé est masqué n'est pas écrit (sauf `all`), comme le tracé.
fn piece_folds(
    project: &Project,
    sheet: SheetId,
    options: SceneOptions,
    all: bool,
) -> (Vec<SceneFold>, Vec<pf_core::EntityId>) {
    let (mut folds, mut entities) = (Vec::new(), Vec::new());
    let Ok(s) = project.sheet(sheet) else {
        return (folds, entities);
    };
    let label = pf_i18n::lookup(options.lang, "onfold.label");
    for piece in project.pieces_on(sheet) {
        if piece.on_fold.is_empty() {
            continue;
        }
        let edges = project.piece_fold_edges(piece.id);
        let lines = project.piece_fold_lines(piece.id, options.piece_mode.includes_cut());
        for (e, l) in edges.iter().zip(lines) {
            if all || s.entity_visible(e.entity) {
                entities.push(e.entity);
                folds.push(SceneFold {
                    piece: piece.name.clone(),
                    from: l.from,
                    to: l.to,
                    label: label.clone(),
                });
            }
        }
    }
    (folds, entities)
}

/// Contraintes de rotation des droit-fil de la feuille (US-13.2). Une pièce sans droit-fil n'en
/// pose aucune ; un droit-fil illisible ou un tracé de droit-fil qui n'appartient à aucune
/// pièce est traité sans tolérance (en cas de doute, on ne tourne pas).
fn grain_limits_of(project: &Project, sheet: SheetId) -> Vec<GrainLimit> {
    let mut limits = Vec::new();
    let mut owned = Vec::new();
    for p in project.pieces_on(sheet) {
        let Some(main) = p.grainline else { continue };
        owned.push(main);
        owned.extend(
            p.grain
                .iter()
                .flat_map(|g| g.extra.iter().map(|e| e.entity)),
        );
        let tolerance_deg = p.grain.as_ref().map_or(0.0, |g| g.tolerance_deg());
        let lines = project.piece_grain_lines(p.id);
        if lines.is_empty() {
            limits.push(GrainLimit {
                oriented: true,
                tolerance_deg: 0.0,
            });
        }
        for l in lines {
            limits.push(GrainLimit {
                oriented: !l.spec.sense.is_unmarked(),
                tolerance_deg,
            });
        }
    }
    if let Ok(s) = project.sheet(sheet) {
        for e in &s.entities {
            if e.role == PathRole::Grainline && !owned.contains(&e.id) {
                limits.push(GrainLimit {
                    oriented: false,
                    tolerance_deg: 0.0,
                });
            }
        }
    }
    limits
}

/// Longueur des branches d'une pointe de flèche de droit-fil (mm).
pub const ARROW_LEN_MM: f64 = 4.0;

/// Branches des deux pointes de flèche d'un droit-fil AB (quatre traits).
/// Aucune pointe si le trait est trop court pour les porter.
pub fn grainline_arrow_marks(a: PointMm, b: PointMm) -> Vec<(PointMm, PointMm)> {
    if pf_geometry::distance_mm(a, b) < 3.0 * ARROW_LEN_MM {
        return Vec::new();
    }
    let Ok(u) = (b - a).unit() else {
        return Vec::new();
    };
    let mut marks = Vec::with_capacity(4);
    for (tip, dir) in [(a, u), (b, pf_geometry::VectorMm::new(-u.x, -u.y))] {
        for angle in [25.0, -25.0] {
            let (s, c) = pf_geometry::sin_cos_deg(angle);
            let v = pf_geometry::VectorMm::new(c * dir.x - s * dir.y, s * dir.x + c * dir.y);
            marks.push((tip, tip + v * ARROW_LEN_MM));
        }
    }
    marks
}

/// Traits de la flèche d'une ligne de droit-fil `a → b` selon le sens du tissu (US-06.5) ; la
/// forme se lit sans la couleur :
///
/// - `Unmarked` : les deux pointes ouvertes de [`grainline_arrow_marks`] (inchangé) ;
/// - `Nap` (poil) : **une seule** pointe, fermée (triangle), au bout `b` ;
/// - `Pattern` (motif) : les deux pointes, chacune **doublée** (une seconde pointe ouverte à
///   [`GRAIN_DOUBLE_HEAD_MM`] en retrait).
///
/// Aucune pointe si le trait est trop court pour les porter.
pub fn grain_arrow_marks(a: PointMm, b: PointMm, sense: GrainSense) -> Vec<(PointMm, PointMm)> {
    if sense == GrainSense::Unmarked || pf_geometry::distance_mm(a, b) < 3.0 * ARROW_LEN_MM {
        return grainline_arrow_marks(a, b);
    }
    let Ok(u) = (b - a).unit() else {
        return Vec::new();
    };
    let branches = |tip: PointMm, dir: pf_geometry::VectorMm| -> [PointMm; 2] {
        [25.0, -25.0].map(|angle| {
            let (s, c) = pf_geometry::sin_cos_deg(angle);
            tip + pf_geometry::VectorMm::new(c * dir.x - s * dir.y, s * dir.x + c * dir.y)
                * ARROW_LEN_MM
        })
    };
    let back = pf_geometry::VectorMm::new(-u.x, -u.y);
    match sense {
        GrainSense::Nap => {
            let [p, q] = branches(b, back);
            vec![(b, p), (b, q), (p, q)]
        }
        _ => {
            let mut marks = grainline_arrow_marks(a, b);
            for (tip, dir) in [(a, u), (b, back)] {
                let inner = tip + dir * GRAIN_DOUBLE_HEAD_MM;
                for end in branches(inner, dir) {
                    marks.push((inner, end));
                }
            }
            marks
        }
    }
}

/// Retrait (mm) de la seconde pointe d'un droit-fil « motif ».
pub const GRAIN_DOUBLE_HEAD_MM: f64 = 2.0;

/// Style d'impression d'un rôle : épaisseur et pointillé en mm, lisibles en
/// monochrome (le rôle ne repose pas sur une couleur).
pub struct StrokeStyle {
    pub width_mm: f64,
    pub dash_mm: &'static [f64],
}

pub fn stroke_style(role: PathRole) -> StrokeStyle {
    let (width_mm, dash_mm): (f64, &'static [f64]) = match role {
        PathRole::Construction => (0.25, &[]),
        PathRole::SewingContour => (0.35, &[3.0, 2.0]),
        PathRole::CuttingContour => (0.6, &[]),
        PathRole::Grainline => (0.35, &[10.0, 1.5, 1.5, 1.5]),
        PathRole::Fold => (0.35, &[6.0, 2.0, 1.0, 2.0]),
        PathRole::AnnotationAxis => (0.25, &[0.6, 1.4]),
        // Lignes spéciales (US-06.8) : chaque rôle a son épaisseur **et** son pointillé, tous
        // différents de ceux des autres rôles (le rôle ne repose jamais sur la couleur).
        PathRole::Topstitch => (0.35, &[1.5, 1.0]),
        PathRole::CenterFront => (0.5, &[16.0, 3.0, 4.0, 3.0]),
        PathRole::CenterBack => (0.5, &[16.0, 3.0, 4.0, 3.0, 4.0, 3.0]),
        PathRole::WaistLine => (0.25, &[10.0, 3.0]),
        PathRole::HipLine => (0.25, &[5.0, 2.5]),
        PathRole::BustLine => (0.25, &[10.0, 2.0, 2.0, 2.0]),
        PathRole::SkivingLine => (0.45, &[5.0, 1.5, 0.6, 1.5, 0.6, 1.5]),
        PathRole::GlueLine => (0.7, &[0.7, 1.3]),
    };
    StrokeStyle { width_mm, dash_mm }
}

/// Formate un nombre pour SVG/PDF : 4 décimales au plus, sans zéros inutiles.
pub(crate) fn num(v: f64) -> String {
    let mut s = format!("{v:.4}");
    while s.contains('.') && (s.ends_with('0') || s.ends_with('.')) {
        s.pop();
    }
    if s == "-0" {
        s = "0".into();
    }
    s
}
