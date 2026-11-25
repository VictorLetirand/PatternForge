//! Lecture d'un DXF texte générique (US-12.3) : lignes, polylignes, arcs, cercles, courbes
//! (SPLINE) et points deviennent de la **géométrie de construction ordinaire** (objets
//! `pf_core::ImportObject`, importés par `Command::ImportGeometry`). Aucune pièce, aucune
//! contrainte, aucun rôle n'est inventé.
//!
//! **Honnêteté** : ce lecteur est écrit d'après une connaissance générale du format, sans les
//! spécifications officielles, et n'a été essayé que sur des fichiers écrits à la main dans les
//! tests et sur ceux de l'export de ce même projet. Il n'est **pas** prouvé sur des fichiers
//! produits par des logiciels réels. Ne jamais écrire « compatible ».
//!
//! Format lu : DXF **texte** (paires code / valeur, R12 à R2018). Le DXF binaire est refusé avec
//! un message précis. Texte UTF-8, sinon relu en Windows-1252 (signalé). Les noms de couches
//! `\U+XXXX` sont décodés.
//!
//! Entités lues : `LINE`, `LWPOLYLINE`, `POLYLINE` / `VERTEX` / `SEQEND` (2D ; les polylignes
//! 3D, maillages et faces sont ignorés avec rapport), `ARC`, `CIRCLE`, `POINT`, `SPLINE`. Un
//! **bulge** devient un arc. Le sens de l'extrusion `(0 ; 0 ; −1)` (miroir de l'axe X) est géré ;
//! toute autre extrusion fait ignorer l'entité (rapport). Les coordonnées Z sont ignorées
//! (rapport). Les `SPLINE` à points de contrôle sont **évaluées** (B-spline rationnelle, nœuds et
//! poids du fichier) puis converties en segments d'écart au plus [`SPLINE_TOLERANCE_MM`] : pas
//! une courbe éditable du modèle, mais une forme exacte à cette tolérance ; une `SPLINE` qui
//! n'a que des points d'ajustement devient une courbe lisse du modèle **passant par ces points**
//! (la forme exacte du logiciel d'origine n'est pas reproduite : remarque).
//!
//! Tout le reste (TEXT, MTEXT, HATCH, DIMENSION, INSERT, 3DFACE…) est **compté par type** dans
//! le rapport avec son motif : jamais de perte silencieuse. Les entités des blocs ne sont pas
//! lues (comptées).
//!
//! Limites documentées : [`MAX_FILE_BYTES`], [`MAX_ENTITIES`] et `pf_core::MAX_IMPORT_OBJECTS`.

use pf_core::{ImportItem, ImportObject, MAX_IMPORT_OBJECTS};
use pf_geometry::{Arc, Bbox, PointMm, Spline, MAX_COORD_MM};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;

/// Taille maximale d'un fichier lu (octets).
pub const MAX_FILE_BYTES: usize = 50 * 1024 * 1024;
/// Nombre maximal d'entités lues dans la section ENTITIES.
pub const MAX_ENTITIES: usize = 200_000;
/// Plafonds de **lecture** : sommets (polylignes, points de splines) de tout le fichier, et paires
/// code / valeur retenues pour une même entité (sommets d'une POLYLINE compris).
pub const MAX_DXF_VERTICES: usize = 2 * pf_core::MAX_IMPORT_OBJECTS;
pub const MAX_ENTITY_PAIRS: usize = 4_000_000;
/// Écart maximal (mm) entre une `SPLINE` évaluée et sa polyligne.
pub const SPLINE_TOLERANCE_MM: f64 = 0.005;
/// Nombre maximal de points d'une `SPLINE` aplatie.
pub const MAX_SPLINE_POINTS: usize = 20_000;
/// Écart maximal (mm) toléré pour remplacer un arc de très grand rayon par un segment.
pub const FLATTEN_BULGE_MM: f64 = 0.01;
/// Deux points plus proches que cela (mm) sont confondus (comme `pf_core::MERGE_TOLERANCE_MM`).
const MERGE_MM: f64 = 1e-6;
const MAX_LAYER_CHARS: usize = 120;
const MAX_NURBS_DEGREE: usize = 25;

// ----- erreurs ---------------------------------------------------------------------------------

/// Refus de lecture ou d'import : aucun objet n'est produit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DxfImportError {
    /// Fichier plus grand que [`MAX_FILE_BYTES`].
    TooLarge { bytes: usize },
    /// DXF binaire : non pris en charge.
    BinaryDxf,
    /// Ce n'est pas un DXF texte (octets nuls, ou rien qui y ressemble).
    NotDxf,
    /// Code de groupe invalide à cette ligne.
    InvalidGroupCode { line: usize },
    /// Fichier tronqué (paire incomplète, section ou fin de fichier absente).
    Truncated { line: usize },
    /// Contenu inattendu hors d'une section.
    Structure { line: usize },
    /// Plus de [`MAX_ENTITIES`] entités.
    TooManyEntities,
    /// Plus de sommets, ou de données d'une même entité, que la lecture n'en accepte.
    TooManyVertices { max: usize },
    /// Plus de `MAX_IMPORT_OBJECTS` objets à créer.
    TooManyObjects { count: usize },
    /// L'unité du fichier est demandée mais le fichier n'en déclare pas de reconnue.
    UnitRequired,
}

impl fmt::Display for DxfImportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge { bytes } => write!(
                f,
                "fichier trop volumineux ({bytes} octets, limite {MAX_FILE_BYTES})"
            ),
            Self::BinaryDxf => write!(
                f,
                "DXF binaire non pris en charge : enregistrez-le en DXF texte (ASCII)"
            ),
            Self::NotDxf => write!(f, "ce fichier n'est pas un DXF texte"),
            Self::InvalidGroupCode { line } => {
                write!(f, "code de groupe invalide à la ligne {line}")
            }
            Self::Truncated { line } => write!(f, "fichier tronqué à la ligne {line}"),
            Self::Structure { line } => write!(f, "contenu inattendu à la ligne {line}"),
            Self::TooManyEntities => write!(f, "plus de {MAX_ENTITIES} entités : import refusé"),
            Self::TooManyVertices { max } => write!(
                f,
                "plus de {max} sommets ou données dans le fichier : import refusé"
            ),
            Self::TooManyObjects { count } => write!(
                f,
                "{count} objets à créer, plus que la limite de {MAX_IMPORT_OBJECTS} par import"
            ),
            Self::UnitRequired => {
                write!(f, "le fichier ne déclare pas d'unité : choisissez-en une")
            }
        }
    }
}

impl std::error::Error for DxfImportError {}

// ----- unités ----------------------------------------------------------------------------------

/// Unité de longueur des coordonnées d'un fichier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DxfUnit {
    Millimetre,
    Centimetre,
    Decimetre,
    Metre,
    Inch,
    Foot,
    Yard,
}

impl DxfUnit {
    /// Unités proposées au choix (les autres ne viennent que de la déclaration du fichier).
    pub const CHOICES: [DxfUnit; 5] = [
        DxfUnit::Millimetre,
        DxfUnit::Centimetre,
        DxfUnit::Metre,
        DxfUnit::Inch,
        DxfUnit::Foot,
    ];

    /// Facteur exact vers le millimètre (25,4 mm par pouce, 304,8 mm par pied).
    pub fn mm_per_unit(self) -> f64 {
        match self {
            DxfUnit::Millimetre => 1.0,
            DxfUnit::Centimetre => 10.0,
            DxfUnit::Decimetre => 100.0,
            DxfUnit::Metre => 1000.0,
            DxfUnit::Inch => 25.4,
            DxfUnit::Foot => 304.8,
            DxfUnit::Yard => 914.4,
        }
    }

    /// Identifiant stable, indépendant de la langue.
    pub fn key(self) -> &'static str {
        match self {
            DxfUnit::Millimetre => "mm",
            DxfUnit::Centimetre => "cm",
            DxfUnit::Decimetre => "dm",
            DxfUnit::Metre => "m",
            DxfUnit::Inch => "in",
            DxfUnit::Foot => "ft",
            DxfUnit::Yard => "yd",
        }
    }

    /// Unité d'une valeur de `$INSUNITS` prise en charge (1 pouce, 2 pied, 4 mm, 5 cm, 6 m,
    /// 10 yard, 14 dm) ; les autres (miles, microns, années-lumière…) ne le sont pas.
    pub fn from_insunits(code: i64) -> Option<DxfUnit> {
        Some(match code {
            1 => DxfUnit::Inch,
            2 => DxfUnit::Foot,
            4 => DxfUnit::Millimetre,
            5 => DxfUnit::Centimetre,
            6 => DxfUnit::Metre,
            10 => DxfUnit::Yard,
            14 => DxfUnit::Decimetre,
            _ => return None,
        })
    }
}

/// Ce que le fichier déclare dans `$INSUNITS`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeclaredUnit {
    /// Variable absente.
    Missing,
    /// 0 = « sans unité ».
    Unitless,
    Known(DxfUnit),
    /// Valeur reconnue du format mais non prise en charge ici.
    Unsupported(i64),
}

/// Choix de l'utilisatrice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnitChoice {
    /// L'unité déclarée par le fichier.
    Declared,
    Fixed(DxfUnit),
}

// ----- rapport ---------------------------------------------------------------------------------

/// Pourquoi une entité n'est pas importée.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum IgnoreReason {
    /// Type d'entité que cet import ne lit pas.
    Unsupported,
    /// Valeur manquante ou illisible (nombre non fini, champ obligatoire absent…).
    Invalid,
    /// Longueur, rayon ou balayage nul.
    Degenerate,
    /// Polyligne 3D, maillage ou face.
    Polyline3d,
    /// Extrusion autre que ±Z.
    Extrusion,
    /// Coordonnées hors de l'espace de travail dans l'unité choisie.
    OutOfRange,
    /// Trop de points pour une courbe.
    TooComplex,
}

impl IgnoreReason {
    pub fn key(self) -> &'static str {
        match self {
            IgnoreReason::Unsupported => "unsupported",
            IgnoreReason::Invalid => "invalid",
            IgnoreReason::Degenerate => "degenerate",
            IgnoreReason::Polyline3d => "polyline_3d",
            IgnoreReason::Extrusion => "extrusion",
            IgnoreReason::OutOfRange => "out_of_range",
            IgnoreReason::TooComplex => "too_complex",
        }
    }
}

/// Entités d'un type non importées pour un même motif.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IgnoredEntities {
    pub kind: String,
    pub reason: IgnoreReason,
    pub count: usize,
}

/// Remarques : tout est importé, mais avec une réserve.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ImportNoteKind {
    /// Coordonnée Z non nulle ignorée (entités).
    ZIgnored,
    /// `SPLINE` évaluée puis convertie en segments (entités).
    SplineSegments,
    /// `SPLINE` à points d'ajustement : courbe lisse passant par ces points (entités).
    SplineFitPoints,
    /// Courbe fermée coupée en deux courbes (entités).
    ClosedCurveSplit,
    /// Arc de très grand rayon remplacé par un segment (parties).
    BulgeFlattened,
    /// Polyligne ajustée (courbe ou spline) lue telle que listée (entités).
    PolylineFit,
    /// Parties de polylignes de longueur nulle retirées.
    ZeroLengthParts,
    /// Objets identiques fusionnés.
    DuplicatesRemoved,
    /// Texte non UTF-8 relu en Windows-1252 (1 fois).
    LegacyEncoding,
    /// Entités définies dans des blocs, non lues.
    BlockContents,
    /// Noms de couches modifiés (caractères de contrôle, longueur).
    LayerRenamed,
}

impl ImportNoteKind {
    pub fn key(self) -> &'static str {
        match self {
            ImportNoteKind::ZIgnored => "z_ignored",
            ImportNoteKind::SplineSegments => "spline_segments",
            ImportNoteKind::SplineFitPoints => "spline_fit_points",
            ImportNoteKind::ClosedCurveSplit => "closed_curve_split",
            ImportNoteKind::BulgeFlattened => "bulge_flattened",
            ImportNoteKind::PolylineFit => "polyline_fit",
            ImportNoteKind::ZeroLengthParts => "zero_length_parts",
            ImportNoteKind::DuplicatesRemoved => "duplicates_removed",
            ImportNoteKind::LegacyEncoding => "legacy_encoding",
            ImportNoteKind::BlockContents => "block_contents",
            ImportNoteKind::LayerRenamed => "layer_renamed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImportNote {
    pub kind: ImportNoteKind,
    pub count: usize,
}

/// Une couche du fichier et le calque qui la reçoit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportLayerInfo {
    /// Nom dans le fichier.
    pub dxf_name: String,
    /// Calque du modèle (`None` = calque par défaut, pour la couche « 0 »).
    pub calque: Option<String>,
    /// Objets créés dans ce calque.
    pub objects: usize,
}

/// Rapport d'import : ce qui est importé, ce qui ne l'est pas, avec les remarques.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ImportReport {
    /// Entités lues par type (avant tout filtrage).
    pub read: BTreeMap<String, usize>,
    /// Entités importées par type.
    pub imported: BTreeMap<String, usize>,
    /// Nombre d'objets du modèle créés (un segment par côté de polyligne…).
    pub objects: usize,
    pub ignored: Vec<IgnoredEntities>,
    pub notes: Vec<ImportNote>,
    pub layers: Vec<ImportLayerInfo>,
}

impl ImportReport {
    /// Nombre total d'entités ignorées.
    pub fn ignored_total(&self) -> usize {
        self.ignored.iter().map(|i| i.count).sum()
    }
    /// Nombre d'entités ignorées d'un type.
    pub fn ignored_of(&self, kind: &str) -> usize {
        self.ignored
            .iter()
            .filter(|i| i.kind == kind)
            .map(|i| i.count)
            .sum()
    }
    pub fn note(&self, kind: ImportNoteKind) -> usize {
        self.notes
            .iter()
            .filter(|n| n.kind == kind)
            .map(|n| n.count)
            .sum()
    }
}

/// Résultat de la conversion dans une unité : ce que l'aperçu montre est exactement ce que la
/// commande importera.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportBuild {
    pub unit: DxfUnit,
    /// Facteur appliqué aux coordonnées du fichier (mm par unité).
    pub scale: f64,
    pub objects: Vec<ImportObject>,
    /// Boîte englobante en mm (`None` si rien n'est importé).
    pub bbox_mm: Option<Bbox>,
    pub report: ImportReport,
}

// ----- entités lues ----------------------------------------------------------------------------

type P2 = (f64, f64);

#[derive(Debug, Clone, PartialEq)]
struct Vertex {
    p: P2,
    bulge: f64,
}

#[derive(Debug, Clone, PartialEq)]
struct SplineData {
    degree: usize,
    knots: Vec<f64>,
    ctrl: Vec<P2>,
    weights: Vec<f64>,
    fit: Vec<P2>,
    closed: bool,
}

#[derive(Debug, Clone, PartialEq)]
enum Geom {
    Point(P2),
    Line(P2, P2),
    Circle {
        c: P2,
        r: f64,
    },
    /// Arc anti-horaire de `a0` à `a1` (degrés).
    Arc {
        c: P2,
        r: f64,
        a0: f64,
        a1: f64,
    },
    Poly {
        verts: Vec<Vertex>,
        closed: bool,
    },
    Spline(SplineData),
}

#[derive(Debug, Clone, PartialEq)]
struct RawEntity {
    kind: &'static str,
    layer: String,
    geom: Geom,
}

/// Fichier lu : entités en unités du fichier, avant choix de l'unité.
#[derive(Debug, Clone, PartialEq)]
pub struct DxfDocument {
    version: Option<String>,
    insunits: Option<i64>,
    entities: Vec<RawEntity>,
    read: BTreeMap<String, usize>,
    ignored: BTreeMap<(String, IgnoreReason), usize>,
    z_ignored: usize,
    polyline_fit: usize,
    blocks: usize,
    block_entities: usize,
    legacy_encoding: bool,
}

// ----- décodage du texte -----------------------------------------------------------------------

const CP1252_HIGH: [char; 32] = [
    '\u{20AC}', '\u{81}', '\u{201A}', '\u{0192}', '\u{201E}', '\u{2026}', '\u{2020}', '\u{2021}',
    '\u{02C6}', '\u{2030}', '\u{0160}', '\u{2039}', '\u{0152}', '\u{8D}', '\u{017D}', '\u{8F}',
    '\u{90}', '\u{2018}', '\u{2019}', '\u{201C}', '\u{201D}', '\u{2022}', '\u{2013}', '\u{2014}',
    '\u{02DC}', '\u{2122}', '\u{0161}', '\u{203A}', '\u{0153}', '\u{9D}', '\u{017E}', '\u{0178}',
];

fn decode_cp1252(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|&b| match b {
            0x80..=0x9F => CP1252_HIGH[(b - 0x80) as usize],
            _ => b as char,
        })
        .collect()
}

/// Texte DXF : `\U+XXXX` (unité UTF-16) décodé ; le reste tel quel.
pub fn decode_dxf_escapes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut units: Vec<u16> = Vec::new();
    let flush = |units: &mut Vec<u16>, out: &mut String| {
        if !units.is_empty() {
            out.extend(char::decode_utf16(units.drain(..)).map(|r| r.unwrap_or('\u{FFFD}')));
        }
    };
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\'
            && i + 7 <= b.len()
            && b[i + 1] == b'U'
            && b[i + 2] == b'+'
            && b[i + 3..i + 7].iter().all(u8::is_ascii_hexdigit)
        {
            if let Ok(u) = u16::from_str_radix(&s[i + 3..i + 7], 16) {
                units.push(u);
                i += 7;
                continue;
            }
        }
        flush(&mut units, &mut out);
        let ch = s[i..].chars().next().unwrap_or('\u{FFFD}');
        out.push(ch);
        i += ch.len_utf8();
    }
    flush(&mut units, &mut out);
    out
}

// ----- lecture des paires ----------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
struct Pair<'a> {
    code: i32,
    value: &'a str,
    line: usize,
}

struct Reader<'a> {
    lines: std::str::Lines<'a>,
    line_no: usize,
    peeked: Option<Pair<'a>>,
}

impl<'a> Reader<'a> {
    fn raw_next(&mut self) -> Result<Option<Pair<'a>>, DxfImportError> {
        loop {
            let Some(code_line) = self.lines.next() else {
                return Ok(None);
            };
            self.line_no += 1;
            let line = self.line_no;
            let Some(value) = self.lines.next() else {
                return Err(DxfImportError::Truncated { line });
            };
            self.line_no += 1;
            let code: i32 = code_line
                .trim()
                .parse()
                .map_err(|_| DxfImportError::InvalidGroupCode { line })?;
            if !(-5..=1071).contains(&code) {
                return Err(DxfImportError::InvalidGroupCode { line });
            }
            if code == 999 {
                continue;
            }
            return Ok(Some(Pair { code, value, line }));
        }
    }

    fn next(&mut self) -> Result<Option<Pair<'a>>, DxfImportError> {
        match self.peeked.take() {
            Some(p) => Ok(Some(p)),
            None => self.raw_next(),
        }
    }

    fn peek(&mut self) -> Result<Option<Pair<'a>>, DxfImportError> {
        if self.peeked.is_none() {
            self.peeked = self.raw_next()?;
        }
        Ok(self.peeked)
    }

    /// Paire obligatoire (la fin du fichier ici est une troncature).
    fn need(&mut self) -> Result<Pair<'a>, DxfImportError> {
        let line = self.line_no;
        self.next()?.ok_or(DxfImportError::Truncated { line })
    }

    /// Les paires d'un enregistrement, jusqu'au prochain code 0.
    fn fields(&mut self) -> Result<Vec<Pair<'a>>, DxfImportError> {
        let mut out = Vec::new();
        while let Some(p) = self.peek()? {
            if p.code == 0 {
                break;
            }
            out.push(p);
            self.peeked = None;
            if out.len() > MAX_ENTITY_PAIRS {
                return Err(DxfImportError::TooManyVertices {
                    max: MAX_ENTITY_PAIRS,
                });
            }
        }
        Ok(out)
    }
}

// ----- champs d'une entité ---------------------------------------------------------------------

struct Fields<'a, 'b>(&'b [Pair<'a>]);

impl<'a, 'b> Fields<'a, 'b> {
    fn get(&self, code: i32) -> Option<&'a str> {
        self.0.iter().find(|p| p.code == code).map(|p| p.value)
    }
    /// Nombre fini facultatif ; `Err` si présent mais illisible.
    fn opt(&self, code: i32) -> Result<Option<f64>, IgnoreReason> {
        match self.get(code) {
            None => Ok(None),
            Some(v) => parse_num(v).map(Some).ok_or(IgnoreReason::Invalid),
        }
    }
    fn req(&self, code: i32) -> Result<f64, IgnoreReason> {
        self.opt(code)?.ok_or(IgnoreReason::Invalid)
    }
    fn layer(&self) -> String {
        decode_dxf_escapes(self.get(8).unwrap_or("0").trim())
    }
    /// Z non nul parmi ces codes ?
    fn z_nonzero(&self, codes: &[i32]) -> Result<bool, IgnoreReason> {
        for &c in codes {
            if self.opt(c)?.is_some_and(|z| z.abs() > 1e-9) {
                return Ok(true);
            }
        }
        Ok(false)
    }
    /// Axe d'extrusion : `Ok(false)` = +Z, `Ok(true)` = −Z (miroir de X), autre = refus.
    fn flip(&self) -> Result<bool, IgnoreReason> {
        let n = (
            self.opt(210)?.unwrap_or(0.0),
            self.opt(220)?.unwrap_or(0.0),
            self.opt(230)?.unwrap_or(1.0),
        );
        if n.0.abs() > 1e-9 || n.1.abs() > 1e-9 {
            return Err(IgnoreReason::Extrusion);
        }
        if (n.2 - 1.0).abs() <= 1e-9 {
            Ok(false)
        } else if (n.2 + 1.0).abs() <= 1e-9 {
            Ok(true)
        } else {
            Err(IgnoreReason::Extrusion)
        }
    }
}

/// Nombre fini (NaN, infinis refusés).
fn parse_num(s: &str) -> Option<f64> {
    s.trim().parse::<f64>().ok().filter(|v| v.is_finite())
}

fn parse_int(s: &str) -> Option<i64> {
    s.trim().parse::<i64>().ok()
}

fn flip_x(p: P2, flip: bool) -> P2 {
    if flip {
        (-p.0, p.1)
    } else {
        p
    }
}

// ----- conversion d'un enregistrement ----------------------------------------------------------

struct Converted {
    kind: &'static str,
    layer: String,
    geom: Geom,
    z: bool,
    fit: bool,
}

fn convert_simple(kind: &str, f: &[Pair<'_>]) -> Result<Option<Converted>, IgnoreReason> {
    let f = Fields(f);
    let layer = f.layer();
    let done = |kind: &'static str, geom: Geom, z: bool| {
        Ok(Some(Converted {
            kind,
            layer: layer.clone(),
            geom,
            z,
            fit: false,
        }))
    };
    match kind {
        "LINE" => {
            let a = (f.req(10)?, f.req(20)?);
            let b = (f.req(11)?, f.req(21)?);
            let z = f.z_nonzero(&[30, 31])?;
            done("LINE", Geom::Line(a, b), z)
        }
        "POINT" => {
            let a = (f.req(10)?, f.req(20)?);
            let z = f.z_nonzero(&[30])?;
            done("POINT", Geom::Point(a), z)
        }
        "CIRCLE" => {
            let flip = f.flip()?;
            let c = flip_x((f.req(10)?, f.req(20)?), flip);
            let r = f.req(40)?;
            let z = f.z_nonzero(&[30])?;
            if r <= 0.0 {
                return Err(IgnoreReason::Degenerate);
            }
            done("CIRCLE", Geom::Circle { c, r }, z)
        }
        "ARC" => {
            let flip = f.flip()?;
            let c = flip_x((f.req(10)?, f.req(20)?), flip);
            let r = f.req(40)?;
            let (mut a0, mut a1) = (f.req(50)?, f.req(51)?);
            let z = f.z_nonzero(&[30])?;
            if r <= 0.0 {
                return Err(IgnoreReason::Degenerate);
            }
            if flip {
                // x → −x : l'arc anti-horaire de a0 à a1 devient celui de 180 − a1 à 180 − a0.
                (a0, a1) = (180.0 - a1, 180.0 - a0);
            }
            done("ARC", Geom::Arc { c, r, a0, a1 }, z)
        }
        "LWPOLYLINE" => {
            let flip = f.flip()?;
            let flags = f.opt(70)?.unwrap_or(0.0) as i64;
            let mut verts: Vec<Vertex> = Vec::new();
            let mut pending_x: Option<f64> = None;
            for p in f.0 {
                match p.code {
                    10 => {
                        let x = parse_num(p.value).ok_or(IgnoreReason::Invalid)?;
                        if pending_x.is_some() {
                            return Err(IgnoreReason::Invalid);
                        }
                        pending_x = Some(x);
                    }
                    20 => {
                        let y = parse_num(p.value).ok_or(IgnoreReason::Invalid)?;
                        let x = pending_x.take().ok_or(IgnoreReason::Invalid)?;
                        verts.push(Vertex {
                            p: flip_x((x, y), flip),
                            bulge: 0.0,
                        });
                    }
                    42 => {
                        let b = parse_num(p.value).ok_or(IgnoreReason::Invalid)?;
                        let v = verts.last_mut().ok_or(IgnoreReason::Invalid)?;
                        v.bulge = if flip { -b } else { b };
                    }
                    _ => {}
                }
            }
            if pending_x.is_some() {
                return Err(IgnoreReason::Invalid);
            }
            let z = f.z_nonzero(&[38])?;
            if verts.len() < 2 {
                return Err(IgnoreReason::Degenerate);
            }
            done(
                "LWPOLYLINE",
                Geom::Poly {
                    verts,
                    closed: flags & 1 == 1,
                },
                z,
            )
        }
        "SPLINE" => {
            let flags = f.opt(70)?.unwrap_or(0.0) as i64;
            let degree = f.opt(71)?.unwrap_or(3.0);
            if !(1.0..=MAX_NURBS_DEGREE as f64).contains(&degree) {
                return Err(IgnoreReason::Invalid);
            }
            let (mut knots, mut weights) = (Vec::new(), Vec::new());
            let (mut ctrl, mut fit): (Vec<P2>, Vec<P2>) = (Vec::new(), Vec::new());
            let mut z = false;
            for p in f.0 {
                let v = || parse_num(p.value).ok_or(IgnoreReason::Invalid);
                match p.code {
                    40 => knots.push(v()?),
                    41 => weights.push(v()?),
                    10 => ctrl.push((v()?, f64::NAN)),
                    20 => ctrl.last_mut().ok_or(IgnoreReason::Invalid)?.1 = v()?,
                    11 => fit.push((v()?, f64::NAN)),
                    21 => fit.last_mut().ok_or(IgnoreReason::Invalid)?.1 = v()?,
                    30 | 31 => z |= v()?.abs() > 1e-9,
                    _ => {}
                }
            }
            if ctrl.iter().chain(fit.iter()).any(|p| p.1.is_nan()) {
                return Err(IgnoreReason::Invalid);
            }
            done(
                "SPLINE",
                Geom::Spline(SplineData {
                    degree: degree as usize,
                    knots,
                    ctrl,
                    weights,
                    fit,
                    closed: flags & 1 == 1,
                }),
                z,
            )
        }
        _ => Ok(None),
    }
}

/// POLYLINE : en-tête et sommets (2D). `Err` = ignorée ; les sommets sont toujours consommés.
fn convert_polyline(
    header: &[Pair<'_>],
    vertices: &[Vec<Pair<'_>>],
) -> Result<Converted, IgnoreReason> {
    let f = Fields(header);
    let flags = f.opt(70)?.unwrap_or(0.0) as i64;
    if flags & (8 | 16 | 64) != 0 {
        return Err(IgnoreReason::Polyline3d);
    }
    let flip = f.flip()?;
    let fit = flags & (2 | 4) != 0;
    let mut z = f.z_nonzero(&[30])?;
    let mut verts = Vec::new();
    for v in vertices {
        let vf = Fields(v);
        let vflags = vf.opt(70)?.unwrap_or(0.0) as i64;
        z |= vf.z_nonzero(&[30])?;
        if fit && vflags & 16 != 0 {
            continue; // point de contrôle du cadre d'une polyligne ajustée
        }
        let p = flip_x((vf.req(10)?, vf.req(20)?), flip);
        let b = if fit { 0.0 } else { vf.opt(42)?.unwrap_or(0.0) };
        verts.push(Vertex {
            p,
            bulge: if flip { -b } else { b },
        });
    }
    if verts.len() < 2 {
        return Err(IgnoreReason::Degenerate);
    }
    Ok(Converted {
        kind: "POLYLINE",
        layer: f.layer(),
        geom: Geom::Poly {
            verts,
            closed: flags & 1 == 1,
        },
        z,
        fit,
    })
}

// ----- lecture du fichier ----------------------------------------------------------------------

impl DxfDocument {
    /// Lit un fichier DXF texte. Refus : voir [`DxfImportError`].
    pub fn parse(bytes: &[u8]) -> Result<DxfDocument, DxfImportError> {
        if bytes.len() > MAX_FILE_BYTES {
            return Err(DxfImportError::TooLarge { bytes: bytes.len() });
        }
        if bytes.starts_with(b"AutoCAD Binary DXF") {
            return Err(DxfImportError::BinaryDxf);
        }
        if bytes.contains(&0) {
            return Err(DxfImportError::NotDxf);
        }
        let mut legacy = false;
        let owned;
        let text: &str = match std::str::from_utf8(bytes) {
            Ok(t) => t.strip_prefix('\u{feff}').unwrap_or(t),
            Err(_) => {
                legacy = true;
                owned = decode_cp1252(bytes);
                &owned
            }
        };
        let mut doc = DxfDocument {
            version: None,
            insunits: None,
            entities: Vec::new(),
            read: BTreeMap::new(),
            ignored: BTreeMap::new(),
            z_ignored: 0,
            polyline_fit: 0,
            blocks: 0,
            block_entities: 0,
            legacy_encoding: legacy,
        };
        let mut r = Reader {
            lines: text.lines(),
            line_no: 0,
            peeked: None,
        };
        let mut sections = 0usize;
        loop {
            let line = r.line_no;
            let Some(p) = r.next()? else {
                return Err(DxfImportError::Truncated { line });
            };
            if p.code != 0 {
                return Err(DxfImportError::Structure { line: p.line });
            }
            match p.value.trim() {
                "EOF" => break,
                "SECTION" => {
                    let name = r.need()?;
                    if name.code != 2 {
                        return Err(DxfImportError::Structure { line: name.line });
                    }
                    sections += 1;
                    match name.value.trim() {
                        "HEADER" => doc.read_header(&mut r)?,
                        "ENTITIES" => doc.read_entities(&mut r)?,
                        "BLOCKS" => doc.read_blocks(&mut r)?,
                        _ => skip_section(&mut r)?,
                    }
                }
                _ => return Err(DxfImportError::Structure { line: p.line }),
            }
        }
        if sections == 0 {
            return Err(DxfImportError::NotDxf);
        }
        Ok(doc)
    }

    fn read_header(&mut self, r: &mut Reader<'_>) -> Result<(), DxfImportError> {
        let mut var = String::new();
        loop {
            let p = r.need()?;
            match p.code {
                0 if p.value.trim() == "ENDSEC" => return Ok(()),
                0 => return Err(DxfImportError::Structure { line: p.line }),
                9 => var = p.value.trim().to_string(),
                70 | 1 => match (var.as_str(), p.code) {
                    ("$INSUNITS", 70) => self.insunits = parse_int(p.value),
                    ("$ACADVER", 1) => self.version = Some(p.value.trim().to_string()),
                    _ => {}
                },
                _ => {}
            }
        }
    }

    fn read_blocks(&mut self, r: &mut Reader<'_>) -> Result<(), DxfImportError> {
        loop {
            let p = r.need()?;
            if p.code != 0 {
                continue;
            }
            match p.value.trim() {
                "ENDSEC" => return Ok(()),
                "BLOCK" => self.blocks += 1,
                "ENDBLK" | "SEQEND" | "VERTEX" | "ATTRIB" => {}
                _ => self.block_entities += 1,
            }
        }
    }

    fn ignore(&mut self, kind: &str, reason: IgnoreReason) {
        *self.ignored.entry((kind.to_string(), reason)).or_default() += 1;
    }

    fn read_entities(&mut self, r: &mut Reader<'_>) -> Result<(), DxfImportError> {
        let mut count = 0usize;
        let mut total_vertices = 0usize;
        let mut add_vertices = |n: usize| {
            total_vertices += n;
            if total_vertices > MAX_DXF_VERTICES {
                Err(DxfImportError::TooManyVertices {
                    max: MAX_DXF_VERTICES,
                })
            } else {
                Ok(())
            }
        };
        loop {
            let p = r.need()?;
            if p.code != 0 {
                return Err(DxfImportError::Structure { line: p.line });
            }
            let kind = p.value.trim().to_ascii_uppercase();
            if kind == "ENDSEC" {
                return Ok(());
            }
            let fields = r.fields()?;
            if matches!(kind.as_str(), "SEQEND" | "ENDBLK" | "VERTEX") {
                continue;
            }
            count += 1;
            if count > MAX_ENTITIES {
                return Err(DxfImportError::TooManyEntities);
            }
            *self.read.entry(kind.clone()).or_default() += 1;
            // Compté avant la conversion, qui alloue un élément par sommet.
            let coded = |codes: &[i32]| fields.iter().filter(|p| codes.contains(&p.code)).count();
            match kind.as_str() {
                "LWPOLYLINE" => add_vertices(coded(&[10]))?,
                "SPLINE" => add_vertices(coded(&[10, 11]))?,
                _ => {}
            }
            let converted = if kind == "POLYLINE" {
                let mut vertices = Vec::new();
                let mut held = fields.len();
                while let Some(next) = r.peek()? {
                    match next.value.trim().to_ascii_uppercase().as_str() {
                        "VERTEX" if next.code == 0 => {
                            r.next()?;
                            let vertex = r.fields()?;
                            add_vertices(1)?;
                            held += vertex.len();
                            if held > MAX_ENTITY_PAIRS {
                                return Err(DxfImportError::TooManyVertices {
                                    max: MAX_ENTITY_PAIRS,
                                });
                            }
                            vertices.push(vertex);
                        }
                        "SEQEND" if next.code == 0 => {
                            r.next()?;
                            r.fields()?;
                            break;
                        }
                        _ => break,
                    }
                }
                convert_polyline(&fields, &vertices).map(Some)
            } else {
                convert_simple(&kind, &fields)
            };
            match converted {
                Ok(Some(c)) => {
                    self.z_ignored += usize::from(c.z);
                    self.polyline_fit += usize::from(c.fit);
                    self.entities.push(RawEntity {
                        kind: c.kind,
                        layer: c.layer,
                        geom: c.geom,
                    });
                }
                Ok(None) => self.ignore(&kind, IgnoreReason::Unsupported),
                Err(reason) => self.ignore(&kind, reason),
            }
        }
    }

    // ----- lecture seule ---------------------------------------------------------------------

    /// Version déclarée (`$ACADVER`), si présente.
    pub fn version(&self) -> Option<&str> {
        self.version.as_deref()
    }

    /// Unité déclarée par `$INSUNITS`.
    pub fn declared_unit(&self) -> DeclaredUnit {
        match self.insunits {
            None => DeclaredUnit::Missing,
            Some(0) => DeclaredUnit::Unitless,
            Some(c) => match DxfUnit::from_insunits(c) {
                Some(u) => DeclaredUnit::Known(u),
                None => DeclaredUnit::Unsupported(c),
            },
        }
    }

    /// Entités lues dans la section ENTITIES (tous types).
    pub fn entity_count(&self) -> usize {
        self.read.values().sum()
    }

    /// Unité retenue pour ce choix, ou [`DxfImportError::UnitRequired`].
    pub fn resolve_unit(&self, choice: UnitChoice) -> Result<DxfUnit, DxfImportError> {
        match choice {
            UnitChoice::Fixed(u) => Ok(u),
            UnitChoice::Declared => match self.declared_unit() {
                DeclaredUnit::Known(u) => Ok(u),
                _ => Err(DxfImportError::UnitRequired),
            },
        }
    }

    /// Convertit le fichier dans une unité : objets du modèle (mm), boîte englobante, rapport.
    pub fn build_choice(&self, choice: UnitChoice) -> Result<ImportBuild, DxfImportError> {
        self.build(self.resolve_unit(choice)?)
    }

    /// Voir [`DxfDocument::build_choice`].
    pub fn build(&self, unit: DxfUnit) -> Result<ImportBuild, DxfImportError> {
        let scale = unit.mm_per_unit();
        let mut b = Builder::new(scale);
        for e in &self.entities {
            b.entity(e)?;
        }
        Ok(b.finish(unit, self))
    }
}

fn skip_section(r: &mut Reader<'_>) -> Result<(), DxfImportError> {
    loop {
        let p = r.need()?;
        if p.code == 0 && p.value.trim() == "ENDSEC" {
            return Ok(());
        }
    }
}

// ----- construction des objets -----------------------------------------------------------------

fn dist(a: P2, b: P2) -> f64 {
    (a.0 - b.0).hypot(a.1 - b.1)
}

fn q(p: PointMm) -> (i64, i64) {
    ((p.x * 1e6).round() as i64, (p.y * 1e6).round() as i64)
}

fn pm(p: P2) -> PointMm {
    PointMm::new(p.0, p.1)
}

fn in_range(p: P2) -> bool {
    p.0.is_finite() && p.1.is_finite() && p.0.abs() <= MAX_COORD_MM && p.1.abs() <= MAX_COORD_MM
}

#[derive(Hash, PartialEq, Eq)]
enum Key {
    Point((i64, i64)),
    Segment((i64, i64), (i64, i64)),
    Circle((i64, i64), i64),
    Arc((i64, i64), (i64, i64), (i64, i64), bool),
    Spline(Vec<(i64, i64)>),
}

fn key_of(item: &ImportItem) -> Key {
    match item {
        ImportItem::Point { at } => Key::Point(q(*at)),
        ImportItem::Segment { from, to } => {
            let (a, b) = (q(*from), q(*to));
            Key::Segment(a.min(b), a.max(b))
        }
        ImportItem::Circle { center, radius_mm } => {
            Key::Circle(q(*center), (radius_mm * 1e6).round() as i64)
        }
        ImportItem::Arc {
            center,
            start,
            end,
            ccw,
        } => Key::Arc(q(*center), q(*start), q(*end), *ccw),
        ImportItem::Spline { points } => Key::Spline(points.iter().map(|p| q(*p)).collect()),
        ImportItem::Bezier { start, c1, c2, end } => {
            Key::Spline([start, c1, c2, end].iter().map(|p| q(**p)).collect())
        }
    }
}

struct Builder {
    scale: f64,
    objects: Vec<ImportObject>,
    seen: HashSet<(Option<String>, Key)>,
    layer_names: HashMap<String, (Option<String>, String)>,
    layer_order: Vec<String>,
    layer_counts: HashMap<String, usize>,
    renamed: usize,
    bbox: Option<Bbox>,
    report: ImportReport,
    ignored: BTreeMap<(String, IgnoreReason), usize>,
    notes: BTreeMap<ImportNoteKind, usize>,
}

impl Builder {
    fn new(scale: f64) -> Builder {
        Builder {
            scale,
            objects: Vec::new(),
            seen: HashSet::new(),
            layer_names: HashMap::new(),
            layer_order: Vec::new(),
            layer_counts: HashMap::new(),
            renamed: 0,
            bbox: None,
            report: ImportReport::default(),
            ignored: BTreeMap::new(),
            notes: BTreeMap::new(),
        }
    }

    fn note(&mut self, kind: ImportNoteKind, n: usize) {
        if n > 0 {
            *self.notes.entry(kind).or_default() += n;
        }
    }

    /// Nom de calque pour une couche du fichier (insensible à la casse, comme DXF).
    fn calque(&mut self, dxf: &str) -> Option<String> {
        let fold = dxf.to_lowercase();
        if let Some((calque, _)) = self.layer_names.get(&fold) {
            return calque.clone();
        }
        let t = dxf.trim();
        let cleaned: String = t
            .chars()
            .map(|c| if c.is_control() { '_' } else { c })
            .take(MAX_LAYER_CHARS)
            .collect();
        let calque = if cleaned.is_empty() || cleaned == "0" {
            None
        } else {
            Some(cleaned.clone())
        };
        if calque.is_some() && cleaned != t {
            self.renamed += 1;
        }
        self.layer_order.push(fold.clone());
        self.layer_names
            .insert(fold, (calque.clone(), dxf.to_string()));
        calque
    }

    fn include(&mut self, item: &ImportItem) {
        let mut add = |p: PointMm| match self.bbox.as_mut() {
            Some(b) => b.include(p),
            None => self.bbox = Some(Bbox::from_point(p)),
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
            } => match Arc::from_center_points(*center, *start, *end, *ccw) {
                Ok(a) => {
                    let bb = a.bbox();
                    add(bb.min);
                    add(bb.max);
                }
                Err(_) => {
                    add(*start);
                    add(*end);
                }
            },
            ImportItem::Spline { points } => match Spline::through_points(points) {
                Ok(s) => {
                    let bb = s.bbox();
                    add(bb.min);
                    add(bb.max);
                }
                Err(_) => points.iter().for_each(|p| add(*p)),
            },
            ImportItem::Bezier { start, c1, c2, end } => {
                let k = pf_geometry::CubicBezier::new(*start, *c1, *c2, *end);
                for i in 0..=32 {
                    add(k.point_at(f64::from(i) / 32.0));
                }
            }
        }
    }

    /// Ajoute les objets d'une entité ; renvoie le nombre d'objets réellement ajoutés.
    fn push_all(
        &mut self,
        layer: &Option<String>,
        items: Vec<ImportItem>,
    ) -> Result<usize, DxfImportError> {
        let mut added = 0;
        for item in items {
            if !self.seen.insert((layer.clone(), key_of(&item))) {
                self.note(ImportNoteKind::DuplicatesRemoved, 1);
                continue;
            }
            self.include(&item);
            self.objects.push(ImportObject {
                item,
                layer: layer.clone(),
            });
            added += 1;
            if self.objects.len() > MAX_IMPORT_OBJECTS {
                return Err(DxfImportError::TooManyObjects {
                    count: self.objects.len(),
                });
            }
        }
        Ok(added)
    }

    fn ignore(&mut self, kind: &str, reason: IgnoreReason) {
        *self.ignored.entry((kind.to_string(), reason)).or_default() += 1;
    }

    fn entity(&mut self, e: &RawEntity) -> Result<(), DxfImportError> {
        let s = self.scale;
        let sc = |p: P2| (p.0 * s, p.1 * s);
        let items: Result<Vec<ImportItem>, IgnoreReason> = match &e.geom {
            Geom::Point(p) => {
                let p = sc(*p);
                if in_range(p) {
                    Ok(vec![ImportItem::Point { at: pm(p) }])
                } else {
                    Err(IgnoreReason::OutOfRange)
                }
            }
            Geom::Line(a, c) => {
                let (a, c) = (sc(*a), sc(*c));
                if !in_range(a) || !in_range(c) {
                    Err(IgnoreReason::OutOfRange)
                } else if dist(a, c) <= MERGE_MM {
                    Err(IgnoreReason::Degenerate)
                } else {
                    Ok(vec![ImportItem::Segment {
                        from: pm(a),
                        to: pm(c),
                    }])
                }
            }
            Geom::Circle { c, r } => {
                let (c, r) = (sc(*c), r * s);
                if !in_range(c) || !r.is_finite() || r > MAX_COORD_MM {
                    Err(IgnoreReason::OutOfRange)
                } else if r <= MERGE_MM {
                    Err(IgnoreReason::Degenerate)
                } else {
                    Ok(vec![ImportItem::Circle {
                        center: pm(c),
                        radius_mm: r,
                    }])
                }
            }
            Geom::Arc { c, r, a0, a1 } => self.arc_items(sc(*c), r * s, *a0, *a1),
            Geom::Poly { verts, closed } => self.poly_items(verts, *closed),
            Geom::Spline(d) => self.spline_items(d),
        };
        match items {
            Ok(items) => {
                let layer = self.calque(&e.layer);
                let added = self.push_all(&layer, items)?;
                *self.layer_counts.entry(e.layer.to_lowercase()).or_default() += added;
                *self.report.imported.entry(e.kind.to_string()).or_default() += 1;
            }
            Err(reason) => self.ignore(e.kind, reason),
        }
        Ok(())
    }

    fn arc_items(
        &mut self,
        c: P2,
        r: f64,
        a0: f64,
        a1: f64,
    ) -> Result<Vec<ImportItem>, IgnoreReason> {
        if !in_range(c) || !r.is_finite() || r > MAX_COORD_MM {
            return Err(IgnoreReason::OutOfRange);
        }
        if r <= MERGE_MM {
            return Err(IgnoreReason::Degenerate);
        }
        let span = a1 - a0;
        if span >= 360.0 - 1e-9 {
            return Ok(vec![ImportItem::Circle {
                center: pm(c),
                radius_mm: r,
            }]);
        }
        let sweep = span.rem_euclid(360.0);
        if sweep < 1e-9 {
            return Err(IgnoreReason::Degenerate);
        }
        let at = |deg: f64| {
            let (sn, cs) = deg.to_radians().sin_cos();
            (c.0 + r * cs, c.1 + r * sn)
        };
        let (start, end) = (at(a0), at(a1));
        if !in_range(start) || !in_range(end) {
            return Err(IgnoreReason::OutOfRange);
        }
        if dist(start, end) <= MERGE_MM {
            return Err(IgnoreReason::Degenerate);
        }
        Ok(vec![ImportItem::Arc {
            center: pm(c),
            start: pm(start),
            end: pm(end),
            ccw: true,
        }])
    }

    fn poly_items(
        &mut self,
        verts: &[Vertex],
        closed: bool,
    ) -> Result<Vec<ImportItem>, IgnoreReason> {
        let s = self.scale;
        let mut pts: Vec<(P2, f64)> = Vec::with_capacity(verts.len());
        for v in verts {
            let p = (v.p.0 * s, v.p.1 * s);
            if !in_range(p) {
                return Err(IgnoreReason::OutOfRange);
            }
            pts.push((p, v.bulge));
        }
        // Sommets consécutifs confondus : le premier garde son bulge si le second n'a pas lieu.
        let mut cleaned: Vec<(P2, f64)> = Vec::with_capacity(pts.len());
        let mut zero = 0;
        for (p, b) in pts {
            match cleaned.last_mut() {
                Some(last) if dist(last.0, p) <= MERGE_MM => {
                    last.1 = b;
                    zero += 1;
                }
                _ => cleaned.push((p, b)),
            }
        }
        if closed
            && cleaned.len() > 1
            && dist(cleaned[0].0, cleaned[cleaned.len() - 1].0) <= MERGE_MM
        {
            cleaned.pop();
            zero += 1;
        }
        self.note(ImportNoteKind::ZeroLengthParts, zero);
        if cleaned.len() < 2 {
            return Err(IgnoreReason::Degenerate);
        }
        let n = cleaned.len();
        let count = if closed { n } else { n - 1 };
        let mut items = Vec::with_capacity(count);
        for i in 0..count {
            let (a, bulge) = cleaned[i];
            let (b, _) = cleaned[(i + 1) % n];
            match bulge_arc(a, b, bulge) {
                Bulge::Straight { flattened } => {
                    if flattened {
                        self.note(ImportNoteKind::BulgeFlattened, 1);
                    }
                    items.push(ImportItem::Segment {
                        from: pm(a),
                        to: pm(b),
                    });
                }
                Bulge::Arc { center, ccw } => items.push(ImportItem::Arc {
                    center: pm(center),
                    start: pm(a),
                    end: pm(b),
                    ccw,
                }),
                Bulge::Reject => return Err(IgnoreReason::OutOfRange),
            }
        }
        Ok(items)
    }

    fn spline_items(&mut self, d: &SplineData) -> Result<Vec<ImportItem>, IgnoreReason> {
        let s = self.scale;
        let nurbs_ok = d.ctrl.len() > d.degree
            && d.knots.len() == d.ctrl.len() + d.degree + 1
            && d.knots.windows(2).all(|w| w[0] <= w[1])
            && (d.weights.is_empty()
                || (d.weights.len() == d.ctrl.len() && d.weights.iter().all(|w| *w > 0.0)));
        if nurbs_ok {
            let tol = SPLINE_TOLERANCE_MM / s;
            let pts = flatten_nurbs(d, tol).ok_or(IgnoreReason::TooComplex)?;
            let mut mm: Vec<P2> = Vec::with_capacity(pts.len());
            for p in pts {
                let p = (p.0 * s, p.1 * s);
                if !in_range(p) {
                    return Err(IgnoreReason::OutOfRange);
                }
                if mm.last().is_none_or(|l| dist(*l, p) > MERGE_MM) {
                    mm.push(p);
                }
            }
            if mm.len() < 2 {
                return Err(IgnoreReason::Degenerate);
            }
            self.note(ImportNoteKind::SplineSegments, 1);
            return Ok(mm
                .windows(2)
                .map(|w| ImportItem::Segment {
                    from: pm(w[0]),
                    to: pm(w[1]),
                })
                .collect());
        }
        if d.fit.len() >= 2 {
            let mut mm: Vec<P2> = Vec::with_capacity(d.fit.len());
            for p in &d.fit {
                let p = (p.0 * s, p.1 * s);
                if !in_range(p) {
                    return Err(IgnoreReason::OutOfRange);
                }
                if mm.last().is_none_or(|l| dist(*l, p) > MERGE_MM) {
                    mm.push(p);
                }
            }
            let mut closed = d.closed;
            if mm.len() > 2 && dist(mm[0], mm[mm.len() - 1]) <= MERGE_MM {
                mm.pop();
                closed = true;
            }
            if mm.len() < 2 {
                return Err(IgnoreReason::Degenerate);
            }
            self.note(ImportNoteKind::SplineFitPoints, 1);
            let pts: Vec<PointMm> = mm.iter().map(|p| pm(*p)).collect();
            let mut curves: Vec<Vec<PointMm>> = Vec::new();
            if closed && pts.len() >= 3 {
                // Une courbe du modèle n'est pas fermée : deux courbes qui se rejoignent.
                let m = pts.len().div_ceil(2);
                curves.push(pts[..=m].to_vec());
                let mut second = pts[m..].to_vec();
                second.push(pts[0]);
                curves.push(second);
                self.note(ImportNoteKind::ClosedCurveSplit, 1);
            } else {
                curves.push(pts);
            }
            let mut items = Vec::new();
            for c in curves {
                // Au plus 1000 points par courbe du modèle : morceaux qui se rejoignent.
                let max = pf_core::MAX_SPLINE_NODES;
                let mut from = 0;
                loop {
                    let to = (from + max).min(c.len());
                    if to - from >= 2 {
                        items.push(ImportItem::Spline {
                            points: c[from..to].to_vec(),
                        });
                    }
                    if to == c.len() {
                        break;
                    }
                    from = to - 1;
                }
            }
            return Ok(items);
        }
        Err(IgnoreReason::Invalid)
    }

    fn finish(mut self, unit: DxfUnit, doc: &DxfDocument) -> ImportBuild {
        let mut ignored = doc.ignored.clone();
        for (k, v) in std::mem::take(&mut self.ignored) {
            *ignored.entry(k).or_default() += v;
        }
        self.report.read = doc.read.clone();
        self.report.ignored = ignored
            .into_iter()
            .map(|((kind, reason), count)| IgnoredEntities {
                kind,
                reason,
                count,
            })
            .collect();
        self.report.objects = self.objects.len();
        self.note(ImportNoteKind::ZIgnored, doc.z_ignored);
        self.note(ImportNoteKind::PolylineFit, doc.polyline_fit);
        self.note(
            ImportNoteKind::LegacyEncoding,
            usize::from(doc.legacy_encoding),
        );
        self.note(ImportNoteKind::BlockContents, doc.block_entities);
        self.note(ImportNoteKind::LayerRenamed, self.renamed);
        self.report.notes = self
            .notes
            .iter()
            .map(|(&kind, &count)| ImportNote { kind, count })
            .collect();
        self.report.layers = self
            .layer_order
            .iter()
            .map(|fold| {
                let (calque, dxf) = self.layer_names[fold].clone();
                ImportLayerInfo {
                    dxf_name: dxf,
                    calque,
                    objects: self.layer_counts.get(fold).copied().unwrap_or(0),
                }
            })
            .filter(|l| l.objects > 0)
            .collect();
        ImportBuild {
            unit,
            scale: self.scale,
            objects: self.objects,
            bbox_mm: self.bbox,
            report: self.report,
        }
    }
}

// ----- arcs de polyligne (bulge) ---------------------------------------------------------------

enum Bulge {
    Straight { flattened: bool },
    Arc { center: P2, ccw: bool },
    Reject,
}

/// Arc de la corde `a → b` pour un bulge DXF (`tan(angle / 4)`, positif = anti-horaire).
fn bulge_arc(a: P2, b: P2, bulge: f64) -> Bulge {
    if bulge.abs() < 1e-12 {
        return Bulge::Straight { flattened: false };
    }
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let c = dx.hypot(dy);
    let sagitta = bulge.abs() * c / 2.0;
    let radius = c * (1.0 + bulge * bulge) / (4.0 * bulge.abs());
    let off = (c / 2.0) * (1.0 - bulge * bulge) / (2.0 * bulge);
    let (mx, my) = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
    let center = (mx - dy / c * off, my + dx / c * off);
    let ok = radius.is_finite() && radius <= MAX_COORD_MM && in_range(center);
    if ok {
        Bulge::Arc {
            center,
            ccw: bulge > 0.0,
        }
    } else if sagitta.is_finite() && sagitta <= FLATTEN_BULGE_MM {
        Bulge::Straight { flattened: true }
    } else {
        Bulge::Reject
    }
}

// ----- B-splines -------------------------------------------------------------------------------

/// Point d'une B-spline rationnelle (de Boor, coordonnées homogènes).
fn nurbs_point(d: &SplineData, t: f64) -> P2 {
    let n = d.ctrl.len();
    let p = d.degree;
    let mut k = p;
    while k + 1 < n && t >= d.knots[k + 1] {
        k += 1;
    }
    let w = |i: usize| d.weights.get(i).copied().unwrap_or(1.0);
    let mut h: Vec<(f64, f64, f64)> = (0..=p)
        .map(|j| {
            let i = j + k - p;
            (d.ctrl[i].0 * w(i), d.ctrl[i].1 * w(i), w(i))
        })
        .collect();
    for r in 1..=p {
        for j in (r..=p).rev() {
            let i = j + k - p;
            let den = d.knots[i + p + 1 - r] - d.knots[i];
            let a = if den.abs() > 0.0 {
                (t - d.knots[i]) / den
            } else {
                0.0
            };
            h[j] = (
                (1.0 - a) * h[j - 1].0 + a * h[j].0,
                (1.0 - a) * h[j - 1].1 + a * h[j].1,
                (1.0 - a) * h[j - 1].2 + a * h[j].2,
            );
        }
    }
    let (x, y, wt) = h[p];
    (x / wt, y / wt)
}

fn chord_dist(p: P2, a: P2, b: P2) -> f64 {
    let (vx, vy) = (b.0 - a.0, b.1 - a.1);
    let len2 = vx * vx + vy * vy;
    if len2 <= 0.0 {
        return dist(p, a);
    }
    let t = (((p.0 - a.0) * vx + (p.1 - a.1) * vy) / len2).clamp(0.0, 1.0);
    dist(p, (a.0 + vx * t, a.1 + vy * t))
}

/// Polyligne de la courbe, écart au plus `tol` (unités du fichier) ; `None` si trop de points
/// ou une valeur non finie.
fn flatten_nurbs(d: &SplineData, tol: f64) -> Option<Vec<P2>> {
    let n = d.ctrl.len();
    let (t0, t1) = (d.knots[d.degree], d.knots[n]);
    if !t0.is_finite() || !t1.is_finite() || t1 <= t0 {
        return None;
    }
    let mut params = vec![t0];
    for &k in &d.knots[d.degree + 1..n] {
        if k > *params.last()? && k < t1 {
            params.push(k);
        }
    }
    params.push(t1);
    let mut out = vec![nurbs_point(d, t0)];
    for w in params.windows(2) {
        let pieces = 4;
        for i in 0..pieces {
            let ta = w[0] + (w[1] - w[0]) * i as f64 / pieces as f64;
            let tb = if i + 1 == pieces {
                w[1]
            } else {
                w[0] + (w[1] - w[0]) * (i + 1) as f64 / pieces as f64
            };
            let (pa, pb) = (*out.last()?, nurbs_point(d, tb));
            subdivide(d, ta, tb, pa, pb, tol, 0, &mut out)?;
        }
    }
    out.iter()
        .all(|p| p.0.is_finite() && p.1.is_finite())
        .then_some(out)
}

#[allow(clippy::too_many_arguments)]
fn subdivide(
    d: &SplineData,
    ta: f64,
    tb: f64,
    pa: P2,
    pb: P2,
    tol: f64,
    depth: u32,
    out: &mut Vec<P2>,
) -> Option<()> {
    if out.len() > MAX_SPLINE_POINTS {
        return None;
    }
    let tm = (ta + tb) / 2.0;
    let (p1, pmid, p3) = (
        nurbs_point(d, (ta + tm) / 2.0),
        nurbs_point(d, tm),
        nurbs_point(d, (tm + tb) / 2.0),
    );
    let flat = [p1, pmid, p3].iter().all(|p| chord_dist(*p, pa, pb) <= tol);
    if flat || depth >= 16 {
        out.push(pb);
        return Some(());
    }
    subdivide(d, ta, tm, pa, pmid, tol, depth + 1, out)?;
    subdivide(d, tm, tb, pmid, pb, tol, depth + 1, out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(entities: &str) -> Vec<u8> {
        format!("  0\nSECTION\n  2\nENTITIES\n{entities}  0\nENDSEC\n  0\nEOF\n").into_bytes()
    }

    #[test]
    fn echappements_unicode_decodes() {
        assert_eq!(decode_dxf_escapes("Fa\\U+00E7ade"), "Façade");
        assert_eq!(decode_dxf_escapes("a\\U+D83D\\U+DE00b"), "a\u{1F600}b");
        assert_eq!(decode_dxf_escapes("\\U+00"), "\\U+00");
        assert_eq!(decode_dxf_escapes("sans"), "sans");
    }

    #[test]
    fn bulge_demi_cercle() {
        // Bulge 1 de (0 ; 0) à (10 ; 0) : demi-cercle anti-horaire de centre (5 ; 0).
        match bulge_arc((0.0, 0.0), (10.0, 0.0), 1.0) {
            Bulge::Arc { center, ccw } => {
                assert!(ccw && (center.0 - 5.0).abs() < 1e-12 && center.1.abs() < 1e-12);
            }
            _ => panic!("arc attendu"),
        }
        match bulge_arc((0.0, 0.0), (10.0, 0.0), -0.5) {
            Bulge::Arc { ccw, .. } => assert!(!ccw),
            _ => panic!("arc attendu"),
        }
    }

    #[test]
    fn bspline_de_bezier_passe_par_les_extremites_et_le_milieu() {
        let d = SplineData {
            degree: 3,
            knots: vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
            ctrl: vec![(0.0, 0.0), (0.0, 10.0), (10.0, 10.0), (10.0, 0.0)],
            weights: vec![],
            fit: vec![],
            closed: false,
        };
        assert_eq!(nurbs_point(&d, 0.0), (0.0, 0.0));
        let e = nurbs_point(&d, 1.0);
        assert!((e.0 - 10.0).abs() < 1e-12 && e.1.abs() < 1e-12);
        let m = nurbs_point(&d, 0.5);
        assert!((m.0 - 5.0).abs() < 1e-12 && (m.1 - 7.5).abs() < 1e-12);
    }

    #[test]
    fn fichier_minimal_lu() {
        let bytes = doc("  0\nLINE\n  8\n0\n 10\n0\n 20\n0\n 11\n10\n 21\n0\n");
        let d = DxfDocument::parse(&bytes).unwrap();
        assert_eq!(d.entity_count(), 1);
        let b = d.build(DxfUnit::Millimetre).unwrap();
        assert_eq!(b.objects.len(), 1);
    }
}
