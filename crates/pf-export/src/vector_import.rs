//! Import SVG et PDF vectoriel (US-12.4) : socle commun aux deux lecteurs (`svg_import`,
//! `pdf_import`). Un lecteur produit des **formes** (chemins de segments, de Bézier cubiques et
//! d'arcs, ou ellipses) dans le repère de la source avec leur matrice courante ; ce module les
//! convertit en `ImportItem` (mm) pour **une seule** commande `ImportGeometry`, calcule l'aperçu
//! (boîte englobante, comptes) et le **rapport de ce qui est ignoré**.
//!
//! Règles :
//!
//! - rien n'est perdu en silence : tout élément non lu (type non pris en charge, valeur
//!   illisible, dégénéré, hors espace de travail) est compté et nommé dans le rapport ;
//! - Bézier cubique -> `ImportItem::Bezier` ; quadratique élevée en cubique (exact) ; arc de
//!   cercle (rapport d'axes 1, matrice de similitude) -> arc ou cercle du modèle ; **ellipse,
//!   arc elliptique ou cercle déformé -> polyligne** dont l'écart à la courbe ne dépasse pas
//!   [`DEFAULT_TOLERANCE_MM`] (déclaré dans l'aperçu) ;
//! - bornes : fichier de 50 Mo, 200 000 objets, coordonnées finies et inférieures à 1e6 mm.

use pf_core::{ImportItem, ImportObject, MAX_IMPORT_OBJECTS};
use pf_geometry::{Bbox, PointMm};
use std::collections::BTreeMap;

/// Taille maximale du fichier lu.
pub const MAX_FILE_BYTES: usize = 50 * 1024 * 1024;
/// Écart maximal (mm) entre une courbe approchée par une polyligne et la courbe elle-même.
pub const DEFAULT_TOLERANCE_MM: f64 = 0.05;
/// Valeur absolue maximale d'une coordonnée importée (mm).
pub const MAX_COORD_MM: f64 = 1.0e6;
/// Échelle utilisateur acceptée (facteur appliqué en plus de l'unité).
pub const MIN_SCALE: f64 = 1.0e-3;
pub const MAX_SCALE: f64 = 1.0e3;
/// Nombre maximal de segments d'une polyligne d'approximation (un seul arc).
const MAX_FLATTEN_STEPS: usize = 20_000;
/// Un rayon et un demi-segment qui diffèrent de moins que cela sont un demi-cercle exact : la
/// source arrondit ses nombres (4 décimales), ce qui rendrait le centre instable.
pub const SEMICIRCLE_SNAP: f64 = 2.0e-4;

// ----- erreurs ---------------------------------------------------------------------------------

/// Fichier refusé : aucun objet n'est créé.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VectorImportError {
    TooLarge {
        bytes: usize,
    },
    NotSvg,
    NotPdf,
    /// SVG mal formé (balise non fermée, attribut mal quoté...), au décalage donné.
    Xml {
        offset: usize,
    },
    /// PDF tronqué ou incomplet.
    Truncated,
    /// PDF dont la structure est incohérente.
    Structure(&'static str),
    Encrypted,
    /// Fonction PDF non prise en charge : `filter`, `predictor`, `object_streams`, `too_big`,
    /// `total_too_big` (budget de décompression du document).
    Unsupported(&'static str),
    /// Plus d'éléments, de sommets ou d'attributs que la lecture n'en accepte (refus avant la
    /// conversion).
    TooManyElements {
        max: usize,
    },
    TooManyObjects {
        count: usize,
    },
    BadScale,
    PageOutOfRange {
        page: usize,
        pages: usize,
    },
    NoPage,
}

// ----- rapport ---------------------------------------------------------------------------------

/// Pourquoi un élément n'est pas importé.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum IgnoreReason {
    /// Type d'élément ou opérateur non lu (texte, image, dégradé, masque...).
    Unsupported,
    /// Valeur illisible (nombre non fini, donnée de chemin mal formée...).
    Invalid,
    /// Rien à tracer (rayon, côté ou matrice nuls).
    Degenerate,
    /// Coordonnées hors de l'espace de travail.
    OutOfRange,
    /// Trop de segments pour une approximation.
    TooComplex,
    /// Chemin PDF jamais terminé par un opérateur de tracé.
    Unterminated,
}

impl IgnoreReason {
    pub fn key(self) -> &'static str {
        match self {
            IgnoreReason::Unsupported => "unsupported",
            IgnoreReason::Invalid => "invalid",
            IgnoreReason::Degenerate => "degenerate",
            IgnoreReason::OutOfRange => "out_of_range",
            IgnoreReason::TooComplex => "too_complex",
            IgnoreReason::Unterminated => "unterminated",
        }
    }
}

/// Remarques : l'élément est importé, avec une réserve.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum NoteKind {
    /// Attribut `style` ignoré (couleurs, traits, `display`...).
    StyleIgnored,
    /// Attribut `clip-path`, `mask` ou `filter` ignoré : la géométrie est importée sans découpe.
    ClipIgnored,
    /// Quadratique élevée en cubique (exact).
    QuadraticRaised,
    /// Ellipse, arc elliptique ou cercle déformé remplacé par une polyligne (tolérance déclarée).
    Flattened,
    /// Demi-cercle reconnu malgré l'arrondi des nombres du fichier.
    SemicircleSnapped,
    /// Rotation de page PDF (`/Rotate`) non appliquée.
    PageRotation,
    /// Chemin de découpe PDF (`W n`) non importé comme géométrie.
    ClipPathDropped,
    /// Unité utilisateur de la page PDF (`/UserUnit`) appliquée à l'échelle.
    UserUnit,
}

impl NoteKind {
    pub fn key(self) -> &'static str {
        match self {
            NoteKind::StyleIgnored => "style_ignored",
            NoteKind::ClipIgnored => "clip_ignored",
            NoteKind::QuadraticRaised => "quadratic_raised",
            NoteKind::Flattened => "flattened",
            NoteKind::SemicircleSnapped => "semicircle_snapped",
            NoteKind::PageRotation => "page_rotation",
            NoteKind::ClipPathDropped => "clip_path_dropped",
            NoteKind::UserUnit => "user_unit",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IgnoredElements {
    pub kind: String,
    pub reason: IgnoreReason,
    pub count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VectorNote {
    pub kind: NoteKind,
    pub count: usize,
}

/// Ce qui est importé, ce qui ne l'est pas, et les remarques.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct VectorReport {
    /// Éléments géométriques rencontrés (importés + ignorés).
    pub read: usize,
    /// Éléments importés, par type (`path`, `rect`, `circle`, `m l c`...).
    pub imported: BTreeMap<String, usize>,
    /// Objets du modèle créés (un segment par côté, une courbe par Bézier...).
    pub objects: usize,
    pub ignored: Vec<IgnoredElements>,
    pub notes: Vec<VectorNote>,
}

impl VectorReport {
    pub fn imported_total(&self) -> usize {
        self.imported.values().sum()
    }
    pub fn ignored_total(&self) -> usize {
        self.ignored.iter().map(|i| i.count).sum()
    }
    pub fn ignored_of(&self, kind: &str) -> usize {
        self.ignored
            .iter()
            .filter(|i| i.kind == kind)
            .map(|i| i.count)
            .sum()
    }
    pub fn note(&self, kind: NoteKind) -> usize {
        self.notes
            .iter()
            .filter(|n| n.kind == kind)
            .map(|n| n.count)
            .sum()
    }
}

/// Résultat de la conversion : ce que l'aperçu montre est exactement ce que la commande importera.
#[derive(Debug, Clone, PartialEq)]
pub struct VectorBuild {
    /// Millimètres par unité de la source (échelle utilisateur comprise).
    pub mm_per_unit: f64,
    pub tolerance_mm: f64,
    pub objects: Vec<ImportObject>,
    /// Boîte englobante en mm (`None` si rien n'est importé).
    pub bbox_mm: Option<Bbox>,
    pub report: VectorReport,
}

// ----- matrices --------------------------------------------------------------------------------

pub type P2 = (f64, f64);

/// Transformation affine `x' = a x + c y + e`, `y' = b x + d y + f`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Matrix {
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub d: f64,
    pub e: f64,
    pub f: f64,
}

impl Matrix {
    pub const IDENTITY: Matrix = Matrix {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        e: 0.0,
        f: 0.0,
    };

    pub fn new(a: f64, b: f64, c: f64, d: f64, e: f64, f: f64) -> Matrix {
        Matrix { a, b, c, d, e, f }
    }

    /// `self` appliquée **après** `local` (le parent compose son enfant).
    pub fn compose(self, local: Matrix) -> Matrix {
        Matrix {
            a: self.a * local.a + self.c * local.b,
            b: self.b * local.a + self.d * local.b,
            c: self.a * local.c + self.c * local.d,
            d: self.b * local.c + self.d * local.d,
            e: self.a * local.e + self.c * local.f + self.e,
            f: self.b * local.e + self.d * local.f + self.f,
        }
    }

    pub fn apply(self, p: P2) -> P2 {
        (
            self.a * p.0 + self.c * p.1 + self.e,
            self.b * p.0 + self.d * p.1 + self.f,
        )
    }

    pub fn det(self) -> f64 {
        self.a * self.d - self.b * self.c
    }

    pub fn is_finite(self) -> bool {
        [self.a, self.b, self.c, self.d, self.e, self.f]
            .iter()
            .all(|v| v.is_finite())
    }

    fn norm2(self) -> f64 {
        self.a * self.a + self.b * self.b + self.c * self.c + self.d * self.d
    }

    /// Matrice qui écrase le plan (déterminant nul, ou presque, devant son ordre de grandeur).
    pub fn is_degenerate(self) -> bool {
        let n = self.norm2();
        n.is_nan() || n <= 0.0 || self.det().abs() <= 1e-12 * n
    }

    /// Facteur d'échelle si la partie linéaire est une similitude (rotation, symétrie,
    /// échelle uniforme) : un cercle reste un cercle.
    pub fn similarity(self) -> Option<f64> {
        let (u, v) = (
            self.a * self.a + self.b * self.b,
            self.c * self.c + self.d * self.d,
        );
        let dot = self.a * self.c + self.b * self.d;
        if u > 0.0 && (u - v).abs() <= 1e-9 * u && dot.abs() <= 1e-9 * u {
            Some(u.sqrt())
        } else {
            None
        }
    }
}

// ----- formes lues -----------------------------------------------------------------------------

/// Arc elliptique en paramétrage central (`P(t) = c + R(phi) (rx cos t, ry sin t)`), avec ses
/// extrémités exactes (celles du chemin, pour que la chaîne reste continue).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ArcSeg {
    pub c: P2,
    pub rx: f64,
    pub ry: f64,
    pub phi: f64,
    pub t1: f64,
    pub dt: f64,
    pub start: P2,
    pub end: P2,
}

impl ArcSeg {
    pub fn point_at(&self, t: f64) -> P2 {
        let (x, y) = (self.rx * t.cos(), self.ry * t.sin());
        let (s, c) = self.phi.sin_cos();
        (self.c.0 + c * x - s * y, self.c.1 + s * x + c * y)
    }

    /// Arc défini par son centre : les extrémités sont calculées.
    pub fn from_params(c: P2, rx: f64, ry: f64, phi: f64, t1: f64, dt: f64) -> ArcSeg {
        let mut a = ArcSeg {
            c,
            rx,
            ry,
            phi,
            t1,
            dt,
            start: c,
            end: c,
        };
        a.start = a.point_at(t1);
        a.end = a.point_at(t1 + dt);
        a
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Seg {
    M(P2),
    L(P2),
    C(P2, P2, P2),
    A(ArcSeg),
    Z,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ShapeGeom {
    Path(Vec<Seg>),
    Ellipse { c: P2, rx: f64, ry: f64 },
}

/// Une forme lue : type d'élément (pour le rapport), géométrie dans son repère local, matrice
/// vers le repère de la source. `Err` = élément reconnu mais illisible (compté, jamais perdu).
#[derive(Debug, Clone, PartialEq)]
pub struct ShapeEntry {
    pub tag: String,
    pub geom: Result<ShapeGeom, IgnoreReason>,
    pub ctm: Matrix,
    /// Quadratique(s) élevée(s) en cubique dans cette forme (remarque).
    pub raised: bool,
    /// Demi-cercle(s) reconnu(s) malgré l'arrondi.
    pub snapped: bool,
}

/// Compteurs que le lecteur a déjà établis (éléments ignorés avant même la conversion).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ReadStats {
    pub ignored: BTreeMap<(String, IgnoreReason), usize>,
    pub notes: BTreeMap<NoteKind, usize>,
}

impl ReadStats {
    pub fn ignore(&mut self, kind: &str, reason: IgnoreReason) {
        let key: String = kind.chars().take(40).collect();
        *self.ignored.entry((key, reason)).or_insert(0) += 1;
    }
    pub fn note(&mut self, kind: NoteKind) {
        *self.notes.entry(kind).or_insert(0) += 1;
    }
}

// ----- conversion en objets du modèle ----------------------------------------------------------

fn pt(p: P2) -> PointMm {
    PointMm::new(p.0, p.1)
}

fn in_range(p: P2) -> bool {
    p.0.is_finite() && p.1.is_finite() && p.0.abs() <= MAX_COORD_MM && p.1.abs() <= MAX_COORD_MM
}

fn item_points_ok(item: &ImportItem) -> bool {
    let ok = |p: &PointMm| in_range((p.x, p.y));
    match item {
        ImportItem::Point { at } => ok(at),
        ImportItem::Segment { from, to } => ok(from) && ok(to),
        ImportItem::Circle { center, radius_mm } => {
            ok(center) && radius_mm.is_finite() && *radius_mm <= MAX_COORD_MM
        }
        ImportItem::Arc {
            center, start, end, ..
        } => ok(center) && ok(start) && ok(end),
        ImportItem::Spline { points } => points.iter().all(ok),
        ImportItem::Bezier { start, c1, c2, end } => ok(start) && ok(c1) && ok(c2) && ok(end),
    }
}

/// Pas angulaire d'une polyligne dont l'écart à un cercle de rayon `r_mm` reste sous `tol_mm`.
fn angular_step(r_mm: f64, tol_mm: f64) -> f64 {
    if r_mm <= tol_mm {
        return std::f64::consts::PI;
    }
    (2.0 * (1.0 - tol_mm / r_mm).acos()).clamp(1e-6, std::f64::consts::PI)
}

/// Un arc en polyligne (points **après** transformation, sans le point de départ).
fn flatten_arc(a: &ArcSeg, m: Matrix, tol_mm: f64) -> Result<Vec<P2>, IgnoreReason> {
    // L'image d'un cercle par une application linéaire de norme n s'écarte au plus de n fois
    // celle du cercle : la norme de Frobenius majore la norme spectrale.
    let r_mm = a.rx.max(a.ry) * m.norm2().sqrt();
    let step = angular_step(r_mm, tol_mm);
    let n = ((a.dt.abs() / step).ceil() as usize).max(1);
    if n > MAX_FLATTEN_STEPS {
        return Err(IgnoreReason::TooComplex);
    }
    let mut out = Vec::with_capacity(n);
    for i in 1..n {
        out.push(m.apply(a.point_at(a.t1 + a.dt * i as f64 / n as f64)));
    }
    out.push(m.apply(a.end));
    Ok(out)
}

struct Converted {
    items: Vec<ImportItem>,
    flattened: bool,
}

fn convert(geom: &ShapeGeom, m: Matrix, tol_mm: f64) -> Result<Converted, IgnoreReason> {
    let mut items = Vec::new();
    let mut flattened = false;
    match geom {
        ShapeGeom::Ellipse { c, rx, ry } => {
            if let (Some(s), true) = (m.similarity(), (rx - ry).abs() <= 1e-9 * rx.max(*ry)) {
                items.push(ImportItem::Circle {
                    center: pt(m.apply(*c)),
                    radius_mm: rx * s,
                });
            } else {
                flattened = true;
                let full = ArcSeg::from_params(*c, *rx, *ry, 0.0, 0.0, std::f64::consts::TAU);
                let first = m.apply(full.start);
                let mut prev = first;
                for p in flatten_arc(&full, m, tol_mm)? {
                    items.push(ImportItem::Segment {
                        from: pt(prev),
                        to: pt(p),
                    });
                    prev = p;
                }
            }
        }
        ShapeGeom::Path(segs) => {
            let (mut cur, mut start): (Option<P2>, Option<P2>) = (None, None);
            for seg in segs {
                // Déjà plus que ce qu'un import accepte : l'appelant refuse le fichier.
                if items.len() > MAX_IMPORT_OBJECTS {
                    break;
                }
                match *seg {
                    Seg::M(p) => {
                        cur = Some(p);
                        start = Some(p);
                    }
                    Seg::L(p) => {
                        if let Some(c) = cur {
                            items.push(ImportItem::Segment {
                                from: pt(m.apply(c)),
                                to: pt(m.apply(p)),
                            });
                        }
                        cur = Some(p);
                    }
                    Seg::C(c1, c2, p) => {
                        if let Some(c) = cur {
                            items.push(ImportItem::Bezier {
                                start: pt(m.apply(c)),
                                c1: pt(m.apply(c1)),
                                c2: pt(m.apply(c2)),
                                end: pt(m.apply(p)),
                            });
                        }
                        cur = Some(p);
                    }
                    Seg::A(a) => {
                        let circular = (a.rx - a.ry).abs() <= 1e-9 * a.rx.max(a.ry);
                        match (m.similarity(), circular) {
                            (Some(_), true) => items.push(ImportItem::Arc {
                                center: pt(m.apply(a.c)),
                                start: pt(m.apply(a.start)),
                                end: pt(m.apply(a.end)),
                                ccw: m.det() * a.dt > 0.0,
                            }),
                            _ => {
                                flattened = true;
                                let mut prev = m.apply(a.start);
                                for p in flatten_arc(&a, m, tol_mm)? {
                                    items.push(ImportItem::Segment {
                                        from: pt(prev),
                                        to: pt(p),
                                    });
                                    prev = p;
                                }
                            }
                        }
                        cur = Some(a.end);
                    }
                    Seg::Z => {
                        if let (Some(c), Some(s)) = (cur, start) {
                            if c != s {
                                items.push(ImportItem::Segment {
                                    from: pt(m.apply(c)),
                                    to: pt(m.apply(s)),
                                });
                            }
                        }
                        cur = start;
                    }
                }
            }
        }
    }
    Ok(Converted { items, flattened })
}

fn include_item(bb: &mut Option<Bbox>, item: &ImportItem) {
    let mut add = |p: PointMm| match bb {
        Some(b) => b.include(p),
        None => *bb = Some(Bbox::from_point(p)),
    };
    match item {
        ImportItem::Point { at } => add(*at),
        ImportItem::Segment { from, to } => {
            add(*from);
            add(*to);
        }
        ImportItem::Circle { center, radius_mm } => {
            add(PointMm::new(center.x - radius_mm, center.y - radius_mm));
            add(PointMm::new(center.x + radius_mm, center.y + radius_mm));
        }
        ImportItem::Arc {
            center,
            start,
            end,
            ccw,
        } => {
            add(*start);
            add(*end);
            let r = (start.x - center.x).hypot(start.y - center.y);
            let ang = |p: &PointMm| (p.y - center.y).atan2(p.x - center.x);
            let (a0, a1) = (ang(start), ang(end));
            let sweep = if *ccw {
                (a1 - a0).rem_euclid(std::f64::consts::TAU)
            } else {
                (a0 - a1).rem_euclid(std::f64::consts::TAU)
            };
            for k in 0..4 {
                let q = k as f64 * std::f64::consts::FRAC_PI_2;
                let off = if *ccw {
                    (q - a0).rem_euclid(std::f64::consts::TAU)
                } else {
                    (a0 - q).rem_euclid(std::f64::consts::TAU)
                };
                if off <= sweep {
                    add(PointMm::new(center.x + r * q.cos(), center.y + r * q.sin()));
                }
            }
        }
        ImportItem::Spline { points } => points.iter().for_each(|p| add(*p)),
        ImportItem::Bezier { start, c1, c2, end } => {
            let k = pf_geometry::CubicBezier::new(*start, *c1, *c2, *end);
            for i in 0..=64 {
                add(k.point_at(i as f64 / 64.0));
            }
        }
    }
}

/// Convertit les formes lues en objets du modèle. `base` = repère source -> mm.
pub fn build_shapes(
    shapes: &[ShapeEntry],
    stats: ReadStats,
    base: Matrix,
    mm_per_unit: f64,
    tol_mm: f64,
) -> Result<VectorBuild, VectorImportError> {
    let mut report = VectorReport::default();
    let mut ignored = stats.ignored;
    let mut notes = stats.notes;
    report.read = ignored.values().sum::<usize>();
    let mut objects: Vec<ImportObject> = Vec::new();
    let mut bbox: Option<Bbox> = None;
    for s in shapes {
        report.read += 1;
        let mut skip = |reason: IgnoreReason| {
            *ignored.entry((s.tag.clone(), reason)).or_insert(0) += 1;
        };
        let geom = match &s.geom {
            Ok(g) => g,
            Err(r) => {
                skip(*r);
                continue;
            }
        };
        let m = base.compose(s.ctm);
        if !m.is_finite() {
            skip(IgnoreReason::OutOfRange);
            continue;
        }
        if m.is_degenerate() {
            skip(IgnoreReason::Degenerate);
            continue;
        }
        let conv = match convert(geom, m, tol_mm) {
            Ok(c) => c,
            Err(r) => {
                skip(r);
                continue;
            }
        };
        if conv.items.is_empty() {
            skip(IgnoreReason::Degenerate);
            continue;
        }
        if conv.items.len() > MAX_IMPORT_OBJECTS {
            return Err(VectorImportError::TooManyObjects {
                count: objects.len() + conv.items.len(),
            });
        }
        if !conv.items.iter().all(item_points_ok) {
            skip(IgnoreReason::OutOfRange);
            continue;
        }
        *report.imported.entry(s.tag.clone()).or_insert(0) += 1;
        if s.raised {
            *notes.entry(NoteKind::QuadraticRaised).or_insert(0) += 1;
        }
        if s.snapped {
            *notes.entry(NoteKind::SemicircleSnapped).or_insert(0) += 1;
        }
        if conv.flattened {
            *notes.entry(NoteKind::Flattened).or_insert(0) += 1;
        }
        for item in conv.items {
            include_item(&mut bbox, &item);
            objects.push(ImportObject { item, layer: None });
        }
        if objects.len() > MAX_IMPORT_OBJECTS {
            return Err(VectorImportError::TooManyObjects {
                count: objects.len(),
            });
        }
    }
    report.objects = objects.len();
    report.ignored = ignored
        .into_iter()
        .map(|((kind, reason), count)| IgnoredElements {
            kind,
            reason,
            count,
        })
        .collect();
    report.notes = notes
        .into_iter()
        .map(|(kind, count)| VectorNote { kind, count })
        .collect();
    Ok(VectorBuild {
        mm_per_unit,
        tolerance_mm: tol_mm,
        objects,
        bbox_mm: bbox,
        report,
    })
}

pub(crate) fn check_scale(scale: f64) -> Result<(), VectorImportError> {
    if scale.is_finite() && (MIN_SCALE..=MAX_SCALE).contains(&scale) {
        Ok(())
    } else {
        Err(VectorImportError::BadScale)
    }
}
