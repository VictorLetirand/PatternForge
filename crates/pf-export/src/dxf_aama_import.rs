//! Import des pièces d'un DXF de type AAMA / ASTM (US-12.2) : lecteur **séparé** de l'export
//! (`dxf`) et de l'import générique (`dxf_import`, qui ne déplie pas les blocs). Il lit les
//! blocs `INSERT`és une fois à l'origine, comme l'écrit l'export d'US-12.1 (une pièce = un bloc),
//! et reconnaît les couches par **la table unique** [`crate::dxf::LAYERS`] (nom de couche = son
//! numéro) : contour de coupe (1) ou de couture (15), crans (4), droit-fil (6), texte (84).
//!
//! **Honnêteté** : cette table n'est pas confirmée avec la spécification officielle
//! ([`crate::dxf::LAYER_TABLE_CONFIRMED`]) ; ce lecteur relit ce que l'export écrit et ce qu'un
//! fichier écrit à la main selon la même table contient. L'ouverture d'un fichier produit par un
//! logiciel métier n'est **pas** vérifiée. **Gradation (`.rul`) : non lue** (aucune spécification
//! dans le cahier ; voir [`AamaReport::rul_read`]).
//!
//! Rien ne se perd en silence : tout élément du fichier qui n'est pas importé est compté dans le
//! rapport ([`AamaIgnored`]), avec sa pièce (bloc), sa couche, son type et son motif.

use crate::dxf::{LayerRole, LAYERS};
use crate::dxf_import::{
    decode_dxf_escapes, DeclaredUnit, DxfImportError, DxfUnit, UnitChoice, MAX_ENTITIES,
    MAX_FILE_BYTES,
};
use pf_core::{
    locate_notch, AamaNotch, AamaPiece, Command, Project, MAX_AAMA_PIECES, MAX_AAMA_VERTICES,
};
use pf_geometry::{PointMm, MAX_COORD_MM};
use std::collections::{BTreeMap, HashMap};

/// Écart maximal (mm) d'un arc (bulge) remplacé par des segments.
pub const ARC_TOLERANCE_MM: f64 = 0.01;
const MAX_ARC_STEPS: usize = 10_000;
const MERGE_MM: f64 = 1e-6;

/// Pourquoi un élément du fichier n'est pas importé.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AamaReason {
    /// Couche de la table, pas encore lue par cet import (lignes internes, perçages…).
    NotImported,
    /// Couche absente de la table.
    UnknownLayer,
    /// Type d'entité non lu sur cette couche.
    Unsupported,
    /// Valeur manquante, illisible ou non finie.
    Invalid,
    /// Longueur ou contour dégénéré.
    Degenerate,
    /// Contour ouvert.
    OpenContour,
    /// Deuxième contour (même couche, ou couture quand une coupe existe).
    ExtraContour,
    /// Cran dont ni le pied ni la pointe ne touche le contour.
    NotOnContour,
    /// Trait de pointe de flèche du droit-fil (seule la ligne principale est importée).
    GrainMark,
    /// Ligne de texte non reconnue.
    TextLine,
    /// Entité hors de tout bloc : l'identité de la pièce est inconnue.
    NoBlock,
    /// Pièce sans contour exploitable.
    NoContour,
    /// Bloc jamais inséré.
    BlockNotInserted,
    /// `INSERT` avec échelle, rotation ou réseau.
    Transformed,
    /// `INSERT` d'un bloc inconnu.
    UnknownBlock,
    /// `INSERT` dans un bloc.
    Nested,
    /// Coordonnées hors de l'espace de travail.
    OutOfRange,
    /// Trop de sommets ou de pièces.
    TooComplex,
    /// La pièce est refusée par le modèle (message en détail).
    Refused,
}

impl AamaReason {
    pub fn key(self) -> &'static str {
        match self {
            AamaReason::NotImported => "not_imported",
            AamaReason::UnknownLayer => "unknown_layer",
            AamaReason::Unsupported => "unsupported",
            AamaReason::Invalid => "invalid",
            AamaReason::Degenerate => "degenerate",
            AamaReason::OpenContour => "open_contour",
            AamaReason::ExtraContour => "extra_contour",
            AamaReason::NotOnContour => "not_on_contour",
            AamaReason::GrainMark => "grain_mark",
            AamaReason::TextLine => "text_line",
            AamaReason::NoBlock => "no_block",
            AamaReason::NoContour => "no_contour",
            AamaReason::BlockNotInserted => "block_not_inserted",
            AamaReason::Transformed => "transformed",
            AamaReason::UnknownBlock => "unknown_block",
            AamaReason::Nested => "nested",
            AamaReason::OutOfRange => "out_of_range",
            AamaReason::TooComplex => "too_complex",
            AamaReason::Refused => "refused",
        }
    }
}

/// Éléments ignorés pour un même motif.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AamaIgnored {
    /// Pièce (nom) ou bloc ; vide hors de tout bloc.
    pub piece: String,
    pub layer: String,
    pub kind: String,
    pub reason: AamaReason,
    pub detail: String,
    pub count: usize,
}

/// Remarques : importé, avec une réserve.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AamaNoteKind {
    /// Sommets consécutifs confondus retirés.
    DuplicateVertices,
    /// Arcs (bulge) remplacés par des segments (tolérance [`ARC_TOLERANCE_MM`]).
    ArcsFlattened,
    /// Contour ouvert dont le dernier sommet répète le premier, fermé.
    ClosedByRepeat,
    /// Texte non UTF-8 relu en Latin-1.
    LegacyEncoding,
}

impl AamaNoteKind {
    pub fn key(self) -> &'static str {
        match self {
            AamaNoteKind::DuplicateVertices => "duplicate_vertices",
            AamaNoteKind::ArcsFlattened => "arcs_flattened",
            AamaNoteKind::ClosedByRepeat => "closed_by_repeat",
            AamaNoteKind::LegacyEncoding => "legacy_encoding",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AamaNote {
    pub kind: AamaNoteKind,
    pub count: usize,
}

/// Une pièce qui sera créée.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AamaPieceInfo {
    pub name: String,
    pub vertices: usize,
    pub notches: usize,
    pub grain: bool,
    pub sewing: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AamaReport {
    /// Blocs insérés dans le fichier.
    pub inserts: usize,
    pub pieces: Vec<AamaPieceInfo>,
    pub ignored: Vec<AamaIgnored>,
    pub notes: Vec<AamaNote>,
}

impl AamaReport {
    /// Le fichier de règles `.rul` (gradation) n'est **jamais** lu par cet import.
    pub fn rul_read(&self) -> bool {
        false
    }
    pub fn ignored_total(&self) -> usize {
        self.ignored.iter().map(|i| i.count).sum()
    }
    pub fn ignored_of(&self, reason: AamaReason) -> usize {
        self.ignored
            .iter()
            .filter(|i| i.reason == reason)
            .map(|i| i.count)
            .sum()
    }
    pub fn note(&self, kind: AamaNoteKind) -> usize {
        self.notes
            .iter()
            .filter(|n| n.kind == kind)
            .map(|n| n.count)
            .sum()
    }
    pub fn notches(&self) -> usize {
        self.pieces.iter().map(|p| p.notches).sum()
    }
}

/// Résultat de la conversion : ce que le rapport annonce est exactement ce que la commande crée.
#[derive(Debug, Clone, PartialEq)]
pub struct AamaBuild {
    pub unit: DxfUnit,
    pub pieces: Vec<AamaPiece>,
    pub report: AamaReport,
}

impl AamaBuild {
    /// La commande d'import (une seule étape annulable). `None` si aucune pièce n'est importable.
    pub fn command(&self) -> Option<Command> {
        (!self.pieces.is_empty()).then(|| Command::ImportAamaPieces {
            pieces: self.pieces.clone(),
        })
    }
}

// ----- lecture ---------------------------------------------------------------------------------

type P2 = (f64, f64);

#[derive(Debug, Clone, PartialEq)]
enum Geom {
    /// Sommets (x, y, bulge vers le suivant) et fermeture.
    Poly {
        verts: Vec<(f64, f64, f64)>,
        closed: bool,
    },
    Line(P2, P2),
    Text(String),
    Insert {
        name: String,
        at: P2,
        transformed: bool,
    },
    Other,
    Bad(AamaReason),
}

#[derive(Debug, Clone, PartialEq)]
struct Item {
    kind: String,
    layer: String,
    geom: Geom,
}

#[derive(Debug, Clone, PartialEq)]
struct Block {
    name: String,
    base: P2,
    items: Vec<Item>,
}

/// Fichier lu, en unités du fichier.
#[derive(Debug, Clone, PartialEq)]
pub struct AamaDocument {
    insunits: Option<i64>,
    blocks: Vec<Block>,
    top: Vec<Item>,
    legacy_encoding: bool,
}

struct Rec {
    kind: String,
    f: Vec<(i32, String)>,
}

impl Rec {
    fn get(&self, code: i32) -> Option<&str> {
        self.f
            .iter()
            .find(|(c, _)| *c == code)
            .map(|(_, v)| v.as_str())
    }
    fn num(&self, code: i32) -> Result<Option<f64>, ()> {
        match self.get(code) {
            None => Ok(None),
            Some(v) => v
                .trim()
                .parse::<f64>()
                .ok()
                .filter(|x| x.is_finite())
                .map(Some)
                .ok_or(()),
        }
    }
    fn layer(&self) -> String {
        self.get(8).map_or("0", str::trim).to_string()
    }
    fn mirrored_extrusion(&self) -> bool {
        self.get(230)
            .and_then(|v| v.trim().parse::<f64>().ok())
            .is_some_and(|z| (z - 1.0).abs() > 1e-9)
    }
}

fn pair(r: &Rec, cx: i32, cy: i32) -> Result<Option<P2>, ()> {
    match (r.num(cx)?, r.num(cy)?) {
        (Some(x), Some(y)) => Ok(Some((x, y))),
        _ => Ok(None),
    }
}

fn convert_polyline(head: &Rec, verts: &[Rec]) -> Geom {
    let flags = head
        .get(70)
        .and_then(|v| v.trim().parse::<i64>().ok())
        .unwrap_or(0);
    if flags & (8 | 16 | 32 | 2 | 4) != 0 || head.mirrored_extrusion() {
        return Geom::Bad(AamaReason::Unsupported);
    }
    let mut out = Vec::new();
    for v in verts {
        let Ok(Some((x, y))) = pair(v, 10, 20) else {
            return Geom::Bad(AamaReason::Invalid);
        };
        let Ok(b) = v.num(42) else {
            return Geom::Bad(AamaReason::Invalid);
        };
        out.push((x, y, b.unwrap_or(0.0)));
    }
    Geom::Poly {
        verts: out,
        closed: flags & 1 != 0,
    }
}

fn convert_lwpolyline(r: &Rec) -> Geom {
    let flags = r
        .get(70)
        .and_then(|v| v.trim().parse::<i64>().ok())
        .unwrap_or(0);
    if r.mirrored_extrusion() {
        return Geom::Bad(AamaReason::Unsupported);
    }
    let mut verts: Vec<(f64, f64, f64)> = Vec::new();
    let mut pending_x: Option<f64> = None;
    for (c, v) in &r.f {
        let num = || v.trim().parse::<f64>().ok().filter(|x| x.is_finite());
        match c {
            10 => {
                if pending_x.is_some() {
                    return Geom::Bad(AamaReason::Invalid);
                }
                match num() {
                    Some(x) => pending_x = Some(x),
                    None => return Geom::Bad(AamaReason::Invalid),
                }
            }
            20 => match (pending_x.take(), num()) {
                (Some(x), Some(y)) => verts.push((x, y, 0.0)),
                _ => return Geom::Bad(AamaReason::Invalid),
            },
            42 => match (verts.last_mut(), num()) {
                (Some(last), Some(b)) => last.2 = b,
                _ => return Geom::Bad(AamaReason::Invalid),
            },
            _ => {}
        }
    }
    if pending_x.is_some() {
        return Geom::Bad(AamaReason::Invalid);
    }
    Geom::Poly {
        verts,
        closed: flags & 1 != 0,
    }
}

fn convert(kind: &str, r: &Rec, verts: &[Rec]) -> Geom {
    match kind {
        "LINE" => {
            if r.mirrored_extrusion() {
                return Geom::Bad(AamaReason::Unsupported);
            }
            match (pair(r, 10, 20), pair(r, 11, 21)) {
                (Ok(Some(a)), Ok(Some(b))) => Geom::Line(a, b),
                _ => Geom::Bad(AamaReason::Invalid),
            }
        }
        "POLYLINE" => convert_polyline(r, verts),
        "LWPOLYLINE" => convert_lwpolyline(r),
        "TEXT" | "MTEXT" => match r.get(1) {
            Some(t) if kind == "TEXT" => Geom::Text(decode_dxf_escapes(t)),
            _ => Geom::Other,
        },
        "INSERT" => {
            let (Some(name), Ok(Some(at))) = (r.get(2), pair(r, 10, 20)) else {
                return Geom::Bad(AamaReason::Invalid);
            };
            let scale = |c| r.num(c).map(|v| v.unwrap_or(1.0));
            let (Ok(sx), Ok(sy), Ok(sz), Ok(rot)) = (
                scale(41),
                scale(42),
                scale(43),
                r.num(50).map(|v| v.unwrap_or(0.0)),
            ) else {
                return Geom::Bad(AamaReason::Invalid);
            };
            let count = |c| {
                r.get(c)
                    .and_then(|v| v.trim().parse::<i64>().ok())
                    .unwrap_or(1)
            };
            let transformed = (sx - 1.0).abs() > 1e-9
                || (sy - 1.0).abs() > 1e-9
                || (sz - 1.0).abs() > 1e-9
                || rot.rem_euclid(360.0).abs() > 1e-9
                    && (rot.rem_euclid(360.0) - 360.0).abs() > 1e-9
                || count(70) != 1
                || count(71) != 1
                || r.mirrored_extrusion();
            Geom::Insert {
                name: name.trim().to_string(),
                at,
                transformed,
            }
        }
        _ => Geom::Other,
    }
}

fn read_records(
    pairs: &[(i32, String)],
    mut i: usize,
    total: &mut usize,
) -> Result<(Vec<Rec>, usize), DxfImportError> {
    let mut out = Vec::new();
    loop {
        let Some((c, v)) = pairs.get(i) else {
            return Err(DxfImportError::Truncated {
                line: pairs.len() * 2,
            });
        };
        if *c != 0 {
            return Err(DxfImportError::Structure { line: i * 2 + 1 });
        }
        let kind = v.trim().to_ascii_uppercase();
        if kind == "ENDSEC" {
            return Ok((out, i + 1));
        }
        i += 1;
        let mut f = Vec::new();
        while let Some((c, v)) = pairs.get(i) {
            if *c == 0 {
                break;
            }
            f.push((*c, v.clone()));
            i += 1;
        }
        *total += 1;
        if *total > MAX_ENTITIES {
            return Err(DxfImportError::TooManyEntities);
        }
        out.push(Rec { kind, f });
    }
}

/// Convertit des enregistrements en éléments (POLYLINE + VERTEX + SEQEND regroupés).
fn items_of(recs: Vec<Rec>) -> Vec<Item> {
    let mut items = Vec::new();
    let mut it = recs.into_iter().peekable();
    while let Some(r) = it.next() {
        match r.kind.as_str() {
            "VERTEX" | "SEQEND" | "ENDBLK" | "BLOCK" => continue,
            "POLYLINE" => {
                let mut verts = Vec::new();
                while let Some(n) = it.peek() {
                    match n.kind.as_str() {
                        "VERTEX" => verts.extend(it.next()),
                        "SEQEND" => {
                            it.next();
                            break;
                        }
                        _ => break,
                    }
                }
                items.push(Item {
                    kind: "POLYLINE".into(),
                    layer: r.layer(),
                    geom: convert_polyline(&r, &verts),
                });
            }
            k => {
                let geom = convert(k, &r, &[]);
                items.push(Item {
                    kind: k.to_string(),
                    layer: r.layer(),
                    geom,
                });
            }
        }
    }
    items
}

impl AamaDocument {
    /// Lit un fichier DXF texte. Refus : voir [`DxfImportError`] (fichier tronqué, binaire,
    /// trop grand, trop d'entités…).
    pub fn parse(bytes: &[u8]) -> Result<AamaDocument, DxfImportError> {
        if bytes.len() > MAX_FILE_BYTES {
            return Err(DxfImportError::TooLarge { bytes: bytes.len() });
        }
        if bytes.starts_with(b"AutoCAD Binary DXF") {
            return Err(DxfImportError::BinaryDxf);
        }
        if bytes.contains(&0) {
            return Err(DxfImportError::NotDxf);
        }
        let (text, legacy): (String, bool) = match std::str::from_utf8(bytes) {
            Ok(t) => (t.strip_prefix('\u{feff}').unwrap_or(t).to_string(), false),
            Err(_) => (bytes.iter().map(|&b| b as char).collect(), true),
        };
        let mut pairs: Vec<(i32, String)> = Vec::new();
        let mut lines = text.lines().enumerate();
        while let Some((n, code)) = lines.next() {
            let code: i32 = code
                .trim()
                .parse()
                .map_err(|_| DxfImportError::InvalidGroupCode { line: n + 1 })?;
            let Some((_, value)) = lines.next() else {
                return Err(DxfImportError::Truncated { line: n + 1 });
            };
            pairs.push((code, value.to_string()));
        }
        let mut doc = AamaDocument {
            insunits: None,
            blocks: Vec::new(),
            top: Vec::new(),
            legacy_encoding: legacy,
        };
        let mut i = 0;
        let mut sections = 0usize;
        let mut total = 0usize;
        loop {
            let Some((c, v)) = pairs.get(i) else {
                return Err(DxfImportError::Truncated {
                    line: pairs.len() * 2,
                });
            };
            if *c != 0 {
                return Err(DxfImportError::Structure { line: i * 2 + 1 });
            }
            match v.trim() {
                "EOF" => break,
                "SECTION" => {
                    let Some((2, name)) = pairs.get(i + 1) else {
                        return Err(DxfImportError::Structure { line: i * 2 + 3 });
                    };
                    sections += 1;
                    i += 2;
                    match name.trim() {
                        "HEADER" => {
                            let mut var = String::new();
                            loop {
                                let Some((c, v)) = pairs.get(i) else {
                                    return Err(DxfImportError::Truncated {
                                        line: pairs.len() * 2,
                                    });
                                };
                                i += 1;
                                match (*c, v.trim()) {
                                    (0, "ENDSEC") => break,
                                    (0, _) => {
                                        return Err(DxfImportError::Structure { line: i * 2 - 1 })
                                    }
                                    (9, name) => var = name.to_string(),
                                    (70, n) if var == "$INSUNITS" => doc.insunits = n.parse().ok(),
                                    _ => {}
                                }
                            }
                        }
                        "ENTITIES" => {
                            let (recs, next) = read_records(&pairs, i, &mut total)?;
                            i = next;
                            doc.top.extend(items_of(recs));
                        }
                        "BLOCKS" => {
                            let (recs, next) = read_records(&pairs, i, &mut total)?;
                            i = next;
                            let mut cur: Option<Block> = None;
                            let mut body: Vec<Rec> = Vec::new();
                            for r in recs {
                                match r.kind.as_str() {
                                    "BLOCK" => {
                                        let base = match pair(&r, 10, 20) {
                                            Ok(Some(b)) => b,
                                            _ => (0.0, 0.0),
                                        };
                                        cur = Some(Block {
                                            name: r.get(2).unwrap_or("").trim().to_string(),
                                            base,
                                            items: Vec::new(),
                                        });
                                        body.clear();
                                    }
                                    "ENDBLK" => {
                                        if let Some(mut b) = cur.take() {
                                            b.items = items_of(std::mem::take(&mut body));
                                            doc.blocks.push(b);
                                        }
                                    }
                                    _ => body.push(r),
                                }
                            }
                            if cur.is_some() {
                                return Err(DxfImportError::Truncated {
                                    line: pairs.len() * 2,
                                });
                            }
                        }
                        _ => loop {
                            let Some((c, v)) = pairs.get(i) else {
                                return Err(DxfImportError::Truncated {
                                    line: pairs.len() * 2,
                                });
                            };
                            i += 1;
                            if *c == 0 && v.trim() == "ENDSEC" {
                                break;
                            }
                        },
                    }
                }
                _ => return Err(DxfImportError::Structure { line: i * 2 + 1 }),
            }
        }
        if sections == 0 {
            return Err(DxfImportError::NotDxf);
        }
        Ok(doc)
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

    /// Nombre de blocs du fichier.
    pub fn block_count(&self) -> usize {
        self.blocks.len()
    }

    /// Nombre d'insertions (`INSERT`) de la section ENTITIES.
    pub fn insert_count(&self) -> usize {
        self.top
            .iter()
            .filter(|i| matches!(i.geom, Geom::Insert { .. }))
            .count()
    }

    pub fn resolve_unit(&self, choice: UnitChoice) -> Result<DxfUnit, DxfImportError> {
        match choice {
            UnitChoice::Fixed(u) => Ok(u),
            UnitChoice::Declared => match self.declared_unit() {
                DeclaredUnit::Known(u) => Ok(u),
                _ => Err(DxfImportError::UnitRequired),
            },
        }
    }

    pub fn build_choice(&self, choice: UnitChoice) -> Result<AamaBuild, DxfImportError> {
        Ok(self.build(self.resolve_unit(choice)?))
    }

    /// Convertit les pièces dans une unité : pièces du modèle (mm) et rapport.
    pub fn build(&self, unit: DxfUnit) -> AamaBuild {
        let scale = unit.mm_per_unit();
        let mut acc = Acc::default();
        if self.legacy_encoding {
            acc.note(AamaNoteKind::LegacyEncoding, 1);
        }
        let mut used = vec![false; self.blocks.len()];
        let mut pieces: Vec<AamaPiece> = Vec::new();
        let mut inserts = 0usize;
        // Premier bloc de chaque nom, casse ASCII ignorée : une table, pas une recherche par
        // insertion.
        let mut by_name: HashMap<String, usize> = HashMap::new();
        for (i, b) in self.blocks.iter().enumerate() {
            by_name.entry(b.name.to_ascii_lowercase()).or_insert(i);
        }
        for it in &self.top {
            let Geom::Insert {
                name,
                at,
                transformed,
            } = &it.geom
            else {
                acc.ignore("", &it.layer, &it.kind, AamaReason::NoBlock, "");
                continue;
            };
            inserts += 1;
            let Some(&bi) = by_name.get(&name.to_ascii_lowercase()) else {
                acc.ignore(name, &it.layer, "INSERT", AamaReason::UnknownBlock, "");
                continue;
            };
            used[bi] = true;
            if *transformed {
                acc.ignore(name, &it.layer, "INSERT", AamaReason::Transformed, "");
                continue;
            }
            if pieces.len() >= MAX_AAMA_PIECES {
                acc.ignore(name, &it.layer, "INSERT", AamaReason::TooComplex, "pieces");
                continue;
            }
            let b = &self.blocks[bi];
            let shift = (at.0 - b.base.0, at.1 - b.base.1);
            if let Some(p) = build_piece(b, shift, scale, &mut acc) {
                pieces.push(p);
            }
        }
        for (b, u) in self.blocks.iter().zip(&used) {
            if !*u {
                for it in &b.items {
                    acc.ignore(
                        &b.name,
                        &it.layer,
                        &it.kind,
                        AamaReason::BlockNotInserted,
                        "",
                    );
                }
            }
        }
        let infos = pieces
            .iter()
            .map(|p| AamaPieceInfo {
                name: p.name.clone(),
                vertices: p.outline.len(),
                notches: p.notches.len(),
                grain: p.grain.is_some(),
                sewing: p.sewing,
            })
            .collect();
        AamaBuild {
            unit,
            pieces,
            report: AamaReport {
                inserts,
                pieces: infos,
                ignored: acc.ignored(),
                notes: acc.notes(),
            },
        }
    }
}

// ----- conversion d'une pièce ------------------------------------------------------------------

#[derive(Default)]
struct Acc {
    ignored: BTreeMap<(String, String, String, AamaReason, String), usize>,
    notes: BTreeMap<AamaNoteKind, usize>,
}

impl Acc {
    fn ignore(&mut self, piece: &str, layer: &str, kind: &str, reason: AamaReason, detail: &str) {
        *self
            .ignored
            .entry((
                piece.to_string(),
                layer.to_string(),
                kind.to_string(),
                reason,
                detail.to_string(),
            ))
            .or_default() += 1;
    }
    fn note(&mut self, kind: AamaNoteKind, n: usize) {
        if n > 0 {
            *self.notes.entry(kind).or_default() += n;
        }
    }
    fn ignored(&self) -> Vec<AamaIgnored> {
        self.ignored
            .iter()
            .map(
                |((piece, layer, kind, reason, detail), count)| AamaIgnored {
                    piece: piece.clone(),
                    layer: layer.clone(),
                    kind: kind.clone(),
                    reason: *reason,
                    detail: detail.clone(),
                    count: *count,
                },
            )
            .collect()
    }
    fn notes(&self) -> Vec<AamaNote> {
        self.notes
            .iter()
            .map(|(kind, count)| AamaNote {
                kind: *kind,
                count: *count,
            })
            .collect()
    }
}

fn mm(p: P2, shift: P2, scale: f64) -> Option<PointMm> {
    let (x, y) = ((p.0 + shift.0) * scale, (p.1 + shift.1) * scale);
    (x.is_finite() && y.is_finite() && x.abs() <= MAX_COORD_MM && y.abs() <= MAX_COORD_MM)
        .then(|| PointMm::new(x, y))
}

/// Remplace l'arc de `a` à `b` (bulge `bulge`) par des segments : points intermédiaires puis `b`.
fn flatten_arc(a: PointMm, b: PointMm, bulge: f64, out: &mut Vec<PointMm>) -> bool {
    let chord = pf_geometry::distance_mm(a, b);
    if chord <= MERGE_MM || !bulge.is_finite() {
        out.push(b);
        return false;
    }
    let theta = 4.0 * bulge.atan();
    let r = chord / (2.0 * (theta.abs() / 2.0).sin());
    if !r.is_finite() || r <= 0.0 {
        out.push(b);
        return false;
    }
    let step = 2.0 * (1.0 - (ARC_TOLERANCE_MM / r).min(1.0)).acos();
    let n = if step > 0.0 {
        ((theta.abs() / step).ceil() as usize).clamp(1, MAX_ARC_STEPS)
    } else {
        MAX_ARC_STEPS
    };
    let (mx, my) = ((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
    let (nx, ny) = (-(b.y - a.y) / chord, (b.x - a.x) / chord);
    let off = (chord / 2.0) * (1.0 - bulge * bulge) / (2.0 * bulge);
    let (cx, cy) = (mx + nx * off, my + ny * off);
    let phi = (a.y - cy).atan2(a.x - cx);
    for k in 1..n {
        let t = phi + theta * k as f64 / n as f64;
        out.push(PointMm::new(cx + r * t.cos(), cy + r * t.sin()));
    }
    out.push(b);
    true
}

struct Contour {
    pts: Vec<PointMm>,
}

/// Contour fermé en mm : arcs aplatis, doublons consécutifs retirés. `Err` = motif d'abandon.
fn contour_of(
    verts: &[(f64, f64, f64)],
    closed: bool,
    shift: P2,
    scale: f64,
    acc: &mut Acc,
) -> Result<Contour, AamaReason> {
    let mut v: Vec<(PointMm, f64)> = Vec::with_capacity(verts.len());
    for (x, y, b) in verts {
        let Some(p) = mm((*x, *y), shift, scale) else {
            return Err(AamaReason::OutOfRange);
        };
        v.push((p, *b));
    }
    let mut closed = closed;
    if !closed && v.len() >= 4 && pf_geometry::distance_mm(v[0].0, v[v.len() - 1].0) <= MERGE_MM {
        v.pop();
        closed = true;
        acc.note(AamaNoteKind::ClosedByRepeat, 1);
    }
    if !closed {
        return Err(AamaReason::OpenContour);
    }
    if v.len() > MAX_AAMA_VERTICES {
        return Err(AamaReason::TooComplex);
    }
    let mut pts: Vec<PointMm> = Vec::new();
    let mut flattened = 0usize;
    for i in 0..v.len() {
        let (a, bulge) = v[i];
        let b = v[(i + 1) % v.len()].0;
        pts.push(a);
        if bulge != 0.0 {
            let mut mid = Vec::new();
            if flatten_arc(a, b, bulge, &mut mid) {
                mid.pop();
                pts.extend(mid);
                flattened += 1;
            }
        }
        if pts.len() > MAX_AAMA_VERTICES {
            return Err(AamaReason::TooComplex);
        }
    }
    acc.note(AamaNoteKind::ArcsFlattened, flattened);
    let before = pts.len();
    pts.dedup_by(|b, a| pf_geometry::distance_mm(*a, *b) <= MERGE_MM);
    while pts.len() > 1 && pf_geometry::distance_mm(pts[0], pts[pts.len() - 1]) <= MERGE_MM {
        pts.pop();
    }
    acc.note(AamaNoteKind::DuplicateVertices, before - pts.len());
    if pts.len() < 3 {
        return Err(AamaReason::Degenerate);
    }
    Ok(Contour { pts })
}

fn layer_role(name: &str) -> Option<LayerRole> {
    let n: u32 = name.trim().parse().ok()?;
    LAYERS.iter().find(|l| l.number == n).map(|l| l.role)
}

struct Text {
    name: Option<String>,
    code: Option<String>,
    quantity: Option<u32>,
    material: Option<String>,
    mirrored: bool,
}

fn build_piece(block: &Block, shift: P2, scale: f64, acc: &mut Acc) -> Option<AamaPiece> {
    let bname = block.name.as_str();
    let (boundary, sewing_l, notch_l, grain_l, text_l) = (
        LayerRole::Boundary,
        LayerRole::Sewing,
        LayerRole::Notches,
        LayerRole::Grainline,
        LayerRole::Text,
    );
    let mut cuts: Vec<Contour> = Vec::new();
    let mut sews: Vec<Contour> = Vec::new();
    let mut notch_lines: Vec<(PointMm, PointMm)> = Vec::new();
    let mut grain_lines: Vec<(PointMm, PointMm)> = Vec::new();
    let mut text = Text {
        name: None,
        code: None,
        quantity: None,
        material: None,
        mirrored: false,
    };
    let mut deferred: Vec<(String, String, AamaReason, String)> = Vec::new();
    for it in &block.items {
        let role = layer_role(&it.layer);
        let mut skip = |reason: AamaReason, detail: &str| {
            deferred.push((
                it.layer.clone(),
                it.kind.clone(),
                reason,
                detail.to_string(),
            ));
        };
        match &it.geom {
            Geom::Bad(r) => skip(*r, ""),
            Geom::Insert { .. } => skip(AamaReason::Nested, ""),
            _ if role.is_none() => skip(AamaReason::UnknownLayer, ""),
            Geom::Poly { verts, closed } if role == Some(boundary) || role == Some(sewing_l) => {
                match contour_of(verts, *closed, shift, scale, acc) {
                    Ok(c) if role == Some(boundary) => cuts.push(c),
                    Ok(c) => sews.push(c),
                    Err(r) => skip(r, ""),
                }
            }
            Geom::Line(a, b) if role == Some(notch_l) || role == Some(grain_l) => {
                match (mm(*a, shift, scale), mm(*b, shift, scale)) {
                    (Some(a), Some(b)) if role == Some(notch_l) => notch_lines.push((a, b)),
                    (Some(a), Some(b)) => grain_lines.push((a, b)),
                    _ => skip(AamaReason::OutOfRange, ""),
                }
            }
            Geom::Text(t) if role == Some(text_l) => {
                let t = t.trim();
                let field = |key: &str| t.strip_prefix(key).map(|v| v.trim().to_string());
                if let Some(v) = field("Piece Name:") {
                    text.name = Some(v);
                } else if let Some(v) = field("Code:") {
                    text.code = Some(v);
                } else if let Some(v) = field("Quantity:") {
                    match v.parse::<u32>() {
                        Ok(q) if (1..=100_000).contains(&q) => text.quantity = Some(q),
                        _ => skip(AamaReason::Invalid, "Quantity"),
                    }
                } else if let Some(v) = field("Material:") {
                    text.material = Some(v);
                } else if t == "Mirrored pair: yes" {
                    text.mirrored = true;
                } else {
                    skip(AamaReason::TextLine, "");
                }
            }
            _ if role == Some(boundary)
                || role == Some(sewing_l)
                || role == Some(notch_l)
                || role == Some(grain_l)
                || role == Some(text_l) =>
            {
                skip(AamaReason::Unsupported, "")
            }
            _ => skip(AamaReason::NotImported, ""),
        }
    }
    let flush = |acc: &mut Acc, name: &str, deferred: Vec<(String, String, AamaReason, String)>| {
        for (l, k, r, d) in deferred {
            acc.ignore(name, &l, &k, r, &d);
        }
    };
    let name = text
        .name
        .clone()
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| bname.to_string());
    let sewing = cuts.is_empty();
    let other = if sewing { 0 } else { sews.len() };
    let mut contours = if sewing { sews } else { cuts };
    let extra_same = contours.len().saturating_sub(1);
    if contours.is_empty() {
        flush(acc, &name, deferred);
        acc.ignore(&name, "", "PIECE", AamaReason::NoContour, "");
        for _ in &notch_lines {
            acc.ignore(&name, "4", "LINE", AamaReason::NoContour, "");
        }
        for _ in &grain_lines {
            acc.ignore(&name, "6", "LINE", AamaReason::NoContour, "");
        }
        return None;
    }
    let outline = contours.remove(0).pts;
    for _ in 0..extra_same {
        let l = if sewing { "15" } else { "1" };
        acc.ignore(&name, l, "POLYLINE", AamaReason::ExtraContour, "");
    }
    for _ in 0..other {
        acc.ignore(&name, "15", "POLYLINE", AamaReason::ExtraContour, "sewing");
    }
    let mut notches = Vec::new();
    for (a, b) in notch_lines {
        if locate_notch(&outline, a, b).is_some() {
            notches.push(AamaNotch { foot: a, tip: b });
        } else {
            acc.ignore(&name, "4", "LINE", AamaReason::NotOnContour, "");
        }
    }
    let mut grain = None;
    let mut best = 0.0;
    let mut idx = None;
    for (k, (a, b)) in grain_lines.iter().enumerate() {
        let d = pf_geometry::distance_mm(*a, *b);
        if d > best {
            best = d;
            idx = Some(k);
        }
    }
    for (k, (a, b)) in grain_lines.iter().enumerate() {
        if Some(k) == idx {
            grain = Some((*a, *b));
        } else if pf_geometry::distance_mm(*a, *b) > MERGE_MM {
            acc.ignore(&name, "6", "LINE", AamaReason::GrainMark, "");
        } else {
            acc.ignore(&name, "6", "LINE", AamaReason::Degenerate, "");
        }
    }
    let piece = AamaPiece {
        name: name.clone(),
        code: text.code.filter(|c| !c.is_empty()),
        quantity: text.quantity.unwrap_or(1),
        material: text.material.filter(|m| !m.is_empty()),
        mirrored_pair: text.mirrored,
        outline,
        sewing,
        notches,
        grain,
    };
    // Le modèle a le dernier mot : une pièce qu'il refuserait est écartée et comptée, jamais
    // laissée faire échouer tout l'import.
    if let Err(e) = Project::new("aama").apply(Command::ImportAamaPieces {
        pieces: vec![piece.clone()],
    }) {
        flush(acc, &name, deferred);
        acc.ignore(&name, "", "PIECE", AamaReason::Refused, &e.to_string());
        return None;
    }
    flush(acc, &name, deferred);
    Some(piece)
}
