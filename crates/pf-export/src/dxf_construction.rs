//! Export DXF **générique** de la géométrie de construction (US-12.3), distinct de l'export de
//! pièces de type AAMA / ASTM (`crate::dxf`, US-12.1) dont il réutilise l'écrivain de bas niveau.
//!
//! Ce que le fichier contient : les tracés de la feuille courante (ou de toutes les feuilles,
//! posées côte à côte) tels que dessinés, sans pièces, blocs ni gradation.
//!
//! - DXF texte ASCII **R12 (`AC1009`)**, entités directement dans `ENTITIES` ; millimètres
//!   (`$INSUNITS` = 4), échelle 1:1, coordonnées à 6 décimales.
//! - segment → `LINE` ; arc → `ARC` (centre, rayon, angles en degrés, toujours anti-horaires :
//!   un arc horaire est écrit de son arrivée à son départ) ; cercle → `CIRCLE` ; point isolé →
//!   `POINT`.
//! - courbe de Bézier et courbe lisse → `POLYLINE` ouverte **discrétisée**, écart au plus
//!   [`CONSTRUCTION_TOLERANCE_MM`] (0,005 mm, donc ≤ 0,01 mm). `SPLINE` n'existe pas en R12 et
//!   le format de courbe du modèle (Hermite) n'a pas d'équivalent exact : la discrétisation est
//!   le choix honnête, signalé dans le rapport.
//! - **couches** : le nom du calque (couche `0` pour le calque par défaut) ; un tracé dont le
//!   rôle n'est pas « construction » est écrit sur la couche `<calque>.<rôle>` (`0.cutting`,
//!   `Devant.sewing`…) : le rôle n'est donc jamais porté par la seule couleur. Un calque masqué
//!   est écrit éteint (couleur négative), un calque verrouillé porte l'indicateur « verrouillé ».
//!   Les caractères non ASCII sont écrits `\U+XXXX`.
//!
//! Lecture seule : le projet n'est jamais modifié. Tout ce qui n'est pas écrit (crans, cotations,
//! lignes d'aide, groupes, noms…) figure dans le rapport : jamais de perte silencieuse.

use crate::dxf::{cubic_points, push_dedup, Writer, DXF_VERSION, INSUNITS_MM, MEASUREMENT_METRIC};
use pf_core::{CoreError, EntityKind, PathRole, PointDefinition, PointId, Project, Shape, SheetId};
use pf_geometry::{Bbox, PointMm};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::{self, Write as _};

/// Écart maximal (mm) de la discrétisation des courbes.
pub const CONSTRUCTION_TOLERANCE_MM: f64 = 0.005;
/// Décimales des coordonnées (1e-6 mm : même échelle que la fusion des points à l'import).
pub const CONSTRUCTION_DECIMALS: usize = 6;
/// Décimales des angles d'arc (degrés).
pub const ANGLE_DECIMALS: usize = 10;
/// Espace laissé entre deux feuilles écrites côte à côte (mm).
pub const SHEET_GAP_MM: f64 = 100.0;
/// Longueur maximale d'un nom de couche écrit.
const MAX_LAYER_NAME: usize = 255;

/// Ce qui est exporté.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConstructionScope {
    Sheet(SheetId),
    /// Toutes les feuilles, côte à côte de gauche à droite (la première reste en place).
    AllSheets,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ConstructionNoteKind {
    // ----- omissions : quelque chose n'est PAS écrit -----
    Notches,
    Dimensions,
    FullDimensions,
    Guides,
    Groups,
    /// Tracé illisible (géométrie dégénérée) : non écrit.
    Unreadable,
    // ----- remarques : la géométrie est écrite, pas ces attributs -----
    /// Noms de tracés non écrits.
    EntityNames,
    /// Étiquettes des points (A, B, C…) non écrites.
    PointLabels,
    /// Contours de pièces écrits comme tracés ordinaires (la pièce elle-même n'est pas écrite).
    PiecesAsPaths,
    /// Courbes de Bézier écrites en polylignes.
    CurvesDiscretized,
    /// Courbes lisses écrites en polylignes.
    SplinesDiscretized,
    /// Calques masqués écrits éteints.
    HiddenLayers,
}

impl ConstructionNoteKind {
    pub fn key(self) -> &'static str {
        match self {
            Self::Notches => "notches",
            Self::Dimensions => "dimensions",
            Self::FullDimensions => "full_dimensions",
            Self::Guides => "guides",
            Self::Groups => "groups",
            Self::EntityNames => "entity_names",
            Self::PointLabels => "point_labels",
            Self::Unreadable => "unreadable",
            Self::PiecesAsPaths => "pieces_as_paths",
            Self::CurvesDiscretized => "curves_discretized",
            Self::SplinesDiscretized => "splines_discretized",
            Self::HiddenLayers => "hidden_layers",
        }
    }

    /// Vrai si quelque chose n'est **pas** écrit.
    pub fn is_omission(self) -> bool {
        matches!(
            self,
            Self::Notches
                | Self::Dimensions
                | Self::FullDimensions
                | Self::Guides
                | Self::Groups
                | Self::Unreadable
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConstructionNote {
    pub kind: ConstructionNoteKind,
    pub count: usize,
}

/// Une couche écrite.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConstructionLayerUse {
    /// Nom dans le fichier.
    pub name: String,
    /// Rôle du tracé quand la couche n'est pas celle de la construction.
    pub role: Option<PathRole>,
    pub entities: usize,
}

/// Une feuille écrite.
#[derive(Debug, Clone, PartialEq)]
pub struct ConstructionSheetInfo {
    pub name: String,
    /// Décalage en X appliqué à la feuille (0 pour la première).
    pub offset_x_mm: f64,
    pub entities: usize,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ConstructionReport {
    pub sheets: Vec<ConstructionSheetInfo>,
    pub layers: Vec<ConstructionLayerUse>,
    /// Entités écrites par type DXF (`LINE`, `ARC`, `CIRCLE`, `POINT`, `POLYLINE`).
    pub entities: BTreeMap<&'static str, usize>,
    pub notes: Vec<ConstructionNote>,
    /// Boîte englobante écrite (mm).
    pub bbox_mm: Option<Bbox>,
}

impl ConstructionReport {
    pub fn omissions(&self) -> impl Iterator<Item = &ConstructionNote> {
        self.notes.iter().filter(|n| n.kind.is_omission())
    }
    pub fn remarks(&self) -> impl Iterator<Item = &ConstructionNote> {
        self.notes.iter().filter(|n| !n.kind.is_omission())
    }
    pub fn total_entities(&self) -> usize {
        self.entities.values().sum()
    }
    pub fn count(&self, kind: ConstructionNoteKind) -> usize {
        self.notes
            .iter()
            .filter(|n| n.kind == kind)
            .map(|n| n.count)
            .sum()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConstructionExport {
    pub text: String,
    pub report: ConstructionReport,
}

impl ConstructionExport {
    pub fn bytes(&self) -> &[u8] {
        self.text.as_bytes()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ConstructionError {
    /// Rien à écrire (feuille sans tracé ni point).
    NothingToExport,
    /// Une coordonnée n'est pas un nombre fini.
    NonFinite,
    /// Une coordonnée dépasse la limite de l'écrivain.
    OutOfRange,
    Core(CoreError),
}

impl fmt::Display for ConstructionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NothingToExport => write!(f, "rien à exporter : la feuille est vide"),
            Self::NonFinite => write!(f, "une coordonnée n'est pas un nombre fini : export refusé"),
            Self::OutOfRange => write!(f, "une coordonnée dépasse la limite : export refusé"),
            Self::Core(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ConstructionError {}

impl From<CoreError> for ConstructionError {
    fn from(e: CoreError) -> Self {
        Self::Core(e)
    }
}

// ----- couches ---------------------------------------------------------------------------------

/// Identifiant ASCII d'un rôle, utilisé dans le nom de couche.
pub fn role_slug(role: PathRole) -> &'static str {
    match role {
        PathRole::Construction => "construction",
        PathRole::SewingContour => "sewing",
        PathRole::CuttingContour => "cutting",
        PathRole::Grainline => "grain",
        PathRole::Fold => "fold",
        PathRole::AnnotationAxis => "axis",
        PathRole::Topstitch => "topstitch",
        PathRole::CenterFront => "center_front",
        PathRole::CenterBack => "center_back",
        PathRole::WaistLine => "waist",
        PathRole::HipLine => "hip",
        PathRole::BustLine => "bust",
        PathRole::SkivingLine => "skiving",
        PathRole::GlueLine => "glue",
    }
}

fn role_color(role: PathRole) -> i64 {
    match role {
        PathRole::Construction => 7,
        PathRole::SewingContour => 3,
        PathRole::CuttingContour => 1,
        PathRole::Grainline => 2,
        PathRole::Fold => 5,
        PathRole::AnnotationAxis => 8,
        _ => 6,
    }
}

/// Nom de couche sûr : ASCII, sans caractère interdit, `\U+XXXX` pour le reste.
fn sanitize_layer(name: &str) -> String {
    let mut out = String::new();
    for ch in name.trim().chars() {
        if ch.is_ascii() {
            if ch.is_ascii_control() || "<>/\\\":;?*|=,`'".contains(ch) {
                out.push('_');
            } else {
                out.push(ch);
            }
        } else if ch.is_control() {
            out.push('_');
        } else {
            let mut buf = [0u16; 2];
            for unit in ch.encode_utf16(&mut buf) {
                let _ = write!(out, "\\U+{unit:04X}");
            }
        }
        if out.len() >= MAX_LAYER_NAME - 8 {
            break;
        }
    }
    if out.is_empty() {
        out.push('_');
    }
    out
}

#[derive(Default)]
struct Layers {
    /// (calque, rôle) → nom écrit.
    by_key: HashMap<(Option<String>, PathRole), String>,
    taken: HashSet<String>,
    /// Dans l'ordre de création : nom, rôle, calque masqué, calque verrouillé, entités.
    rows: Vec<LayerRow>,
}

struct LayerRow {
    name: String,
    role: PathRole,
    hidden: bool,
    locked: bool,
    entities: usize,
}

impl Layers {
    fn name_of(
        &mut self,
        calque: Option<&str>,
        role: PathRole,
        hidden: bool,
        locked: bool,
    ) -> usize {
        let key = (calque.map(str::to_string), role);
        if let Some(name) = self.by_key.get(&key) {
            return self
                .rows
                .iter()
                .position(|r| &r.name == name)
                .unwrap_or_default();
        }
        let mut base = match calque {
            None => "0".to_string(),
            Some(c) => {
                let s = sanitize_layer(c);
                if s == "0" {
                    "0_".to_string()
                } else {
                    s
                }
            }
        };
        if role != PathRole::Construction {
            base = format!("{base}.{}", role_slug(role));
        }
        let mut name = base.clone();
        let mut n = 2;
        while !self.taken.insert(name.to_lowercase()) {
            name = format!("{base}_{n}");
            n += 1;
        }
        self.by_key.insert(key, name.clone());
        self.rows.push(LayerRow {
            name,
            role,
            hidden,
            locked,
            entities: 0,
        });
        self.rows.len() - 1
    }
}

// ----- entités ---------------------------------------------------------------------------------

enum CEntity {
    Line {
        layer: usize,
        a: PointMm,
        b: PointMm,
    },
    Point {
        layer: usize,
        p: PointMm,
    },
    Circle {
        layer: usize,
        c: PointMm,
        r: f64,
    },
    Arc {
        layer: usize,
        c: PointMm,
        r: f64,
        a0: f64,
        a1: f64,
    },
    Poly {
        layer: usize,
        pts: Vec<PointMm>,
    },
}

impl CEntity {
    fn layer(&self) -> usize {
        match self {
            CEntity::Line { layer, .. }
            | CEntity::Point { layer, .. }
            | CEntity::Circle { layer, .. }
            | CEntity::Arc { layer, .. }
            | CEntity::Poly { layer, .. } => *layer,
        }
    }
    fn dxf_type(&self) -> &'static str {
        match self {
            CEntity::Line { .. } => "LINE",
            CEntity::Point { .. } => "POINT",
            CEntity::Circle { .. } => "CIRCLE",
            CEntity::Arc { .. } => "ARC",
            CEntity::Poly { .. } => "POLYLINE",
        }
    }
    fn bounds(&self, bb: &mut Option<Bbox>) {
        let mut add = |p: PointMm| match bb {
            Some(b) => b.include(p),
            None => *bb = Some(Bbox::from_point(p)),
        };
        match self {
            CEntity::Line { a, b, .. } => {
                add(*a);
                add(*b);
            }
            CEntity::Point { p, .. } => add(*p),
            CEntity::Circle { c, r, .. } => {
                add(PointMm::new(c.x - r, c.y - r));
                add(PointMm::new(c.x + r, c.y + r));
            }
            CEntity::Arc { c, r, a0, a1, .. } => {
                // Boîte de l'arc exact : extrémités et axes traversés.
                let at = |deg: f64| {
                    let (s, co) = deg.to_radians().sin_cos();
                    PointMm::new(c.x + r * co, c.y + r * s)
                };
                add(at(*a0));
                add(at(*a1));
                let sweep = (a1 - a0).rem_euclid(360.0);
                for k in 0..4 {
                    let axis = f64::from(k) * 90.0;
                    if (axis - a0).rem_euclid(360.0) <= sweep {
                        add(at(axis));
                    }
                }
            }
            CEntity::Poly { pts, .. } => pts.iter().for_each(|p| add(*p)),
        }
    }
    fn shifted(mut self, dx: f64) -> CEntity {
        let sh = |p: &mut PointMm| p.x += dx;
        match &mut self {
            CEntity::Line { a, b, .. } => {
                sh(a);
                sh(b);
            }
            CEntity::Point { p, .. } => sh(p),
            CEntity::Circle { c, .. } | CEntity::Arc { c, .. } => sh(c),
            CEntity::Poly { pts, .. } => pts.iter_mut().for_each(sh),
        }
        self
    }
}

fn add_note(
    notes: &mut BTreeMap<ConstructionNoteKind, usize>,
    kind: ConstructionNoteKind,
    n: usize,
) {
    if n > 0 {
        *notes.entry(kind).or_default() += n;
    }
}

/// Entités d'une feuille (sans décalage).
fn sheet_entities(
    project: &Project,
    sheet_id: SheetId,
    layers: &mut Layers,
    notes: &mut BTreeMap<ConstructionNoteKind, usize>,
) -> Result<Vec<CEntity>, CoreError> {
    let sheet = project.sheet(sheet_id)?;
    // Positions en une passe : une recherche par point serait quadratique.
    let mut pos: HashMap<PointId, PointMm> = HashMap::with_capacity(sheet.points.len());
    for p in &sheet.points {
        let at = match p.definition {
            PointDefinition::Free { x, y } => Some(PointMm::new(x, y)),
            _ => sheet.position(p.id).ok(),
        };
        if let Some(at) = at {
            pos.insert(p.id, at);
        }
    }
    let calque_info = |layer: Option<pf_core::LayerId>| -> (Option<String>, bool, bool) {
        match layer.and_then(|l| sheet.layer(l).ok()) {
            Some(l) => (Some(l.name.clone()), !l.visible, l.locked),
            None => (None, false, false),
        }
    };
    let mut out: Vec<CEntity> = Vec::with_capacity(sheet.entities.len());
    let mut used: HashSet<PointId> = HashSet::new();
    let (mut curves, mut splines, mut unreadable) = (0, 0, 0);
    for e in &sheet.entities {
        used.extend(e.kind.all_points());
        let (calque, hidden, locked) = calque_info(e.layer);
        let layer = layers.name_of(calque.as_deref(), e.role, hidden, locked);
        let p = |id: PointId| pos.get(&id).copied();
        match &e.kind {
            EntityKind::Segment { from, to, .. } => match (p(*from), p(*to)) {
                (Some(a), Some(b)) => out.push(CEntity::Line { layer, a, b }),
                _ => unreadable += 1,
            },
            EntityKind::Circle {
                center, radius_mm, ..
            } => match p(*center) {
                Some(c) => out.push(CEntity::Circle {
                    layer,
                    c,
                    r: *radius_mm,
                }),
                None => unreadable += 1,
            },
            EntityKind::Arc {
                center,
                start,
                end,
                ccw,
            } => {
                let arc = match (p(*center), p(*start), p(*end)) {
                    (Some(c), Some(s), Some(en)) => {
                        pf_geometry::Arc::from_center_points(c, s, en, *ccw).ok()
                    }
                    _ => None,
                };
                match arc {
                    Some(a) => {
                        let (a0, a1) = if a.is_ccw() {
                            (a.start_deg(), a.end_deg())
                        } else {
                            (a.end_deg(), a.start_deg())
                        };
                        out.push(CEntity::Arc {
                            layer,
                            c: a.center,
                            r: a.radius_mm,
                            a0,
                            a1,
                        });
                    }
                    None => unreadable += 1,
                }
            }
            EntityKind::Curve { .. } | EntityKind::Spline { .. } => {
                let mut pts: Vec<PointMm> = Vec::new();
                match sheet.shape(e.id) {
                    Ok(Shape::Cubic(c)) => {
                        cubic_points(&c, CONSTRUCTION_TOLERANCE_MM, &mut pts);
                        curves += 1;
                    }
                    Ok(Shape::Spline(s)) => {
                        for c in s.segments() {
                            cubic_points(c, CONSTRUCTION_TOLERANCE_MM, &mut pts);
                        }
                        splines += 1;
                    }
                    _ => {}
                }
                if pts.len() >= 2 {
                    let mut dedup = Vec::with_capacity(pts.len());
                    pts.into_iter().for_each(|q| push_dedup(&mut dedup, q));
                    out.push(CEntity::Poly { layer, pts: dedup });
                } else {
                    unreadable += 1;
                }
            }
        }
    }
    // Points isolés (aucun tracé ne les utilise).
    for pt in &sheet.points {
        if used.contains(&pt.id) {
            continue;
        }
        let Some(at) = pos.get(&pt.id).copied() else {
            unreadable += 1;
            continue;
        };
        let (calque, hidden, locked) = calque_info(pt.layer);
        let layer = layers.name_of(calque.as_deref(), PathRole::Construction, hidden, locked);
        out.push(CEntity::Point { layer, p: at });
    }
    add_note(notes, ConstructionNoteKind::CurvesDiscretized, curves);
    add_note(notes, ConstructionNoteKind::SplinesDiscretized, splines);
    add_note(notes, ConstructionNoteKind::Unreadable, unreadable);
    add_note(notes, ConstructionNoteKind::Notches, sheet.notches.len());
    add_note(
        notes,
        ConstructionNoteKind::Dimensions,
        sheet.dimensions.len(),
    );
    add_note(
        notes,
        ConstructionNoteKind::FullDimensions,
        sheet.full_dimensions.len(),
    );
    add_note(notes, ConstructionNoteKind::Guides, sheet.guides.len());
    add_note(notes, ConstructionNoteKind::Groups, sheet.groups.len());
    add_note(
        notes,
        ConstructionNoteKind::EntityNames,
        sheet.entities.iter().filter(|e| e.name.is_some()).count(),
    );
    add_note(notes, ConstructionNoteKind::PointLabels, sheet.points.len());
    add_note(
        notes,
        ConstructionNoteKind::PiecesAsPaths,
        project.pieces_of(sheet_id).count(),
    );
    Ok(out)
}

fn write_entity(w: &mut Writer, e: &CEntity, layers: &[LayerRow]) {
    let dec = CONSTRUCTION_DECIMALS;
    let name = layers[e.layer()].name.as_str();
    let pt = |w: &mut Writer, base: i32, p: PointMm| {
        w.real_dec(base, p.x, dec);
        w.real_dec(base + 10, p.y, dec);
        w.real_dec(base + 20, 0.0, dec);
    };
    match e {
        CEntity::Line { a, b, .. } => {
            w.begin_named("LINE", name);
            pt(w, 10, *a);
            pt(w, 11, *b);
        }
        CEntity::Point { p, .. } => {
            w.begin_named("POINT", name);
            pt(w, 10, *p);
        }
        CEntity::Circle { c, r, .. } => {
            w.begin_named("CIRCLE", name);
            pt(w, 10, *c);
            w.real_dec(40, *r, dec);
        }
        CEntity::Arc { c, r, a0, a1, .. } => {
            w.begin_named("ARC", name);
            pt(w, 10, *c);
            w.real_dec(40, *r, dec);
            w.real_dec(50, *a0, ANGLE_DECIMALS);
            w.real_dec(51, *a1, ANGLE_DECIMALS);
        }
        CEntity::Poly { pts, .. } => {
            w.begin_named("POLYLINE", name);
            w.int(66, 1);
            pt(w, 10, PointMm::new(0.0, 0.0));
            w.int(70, 0);
            for p in pts {
                w.begin_named("VERTEX", name);
                pt(w, 10, *p);
            }
            w.begin_named("SEQEND", name);
        }
    }
}

/// Exporte la géométrie de construction d'une feuille ou de toutes les feuilles. Rien n'est écrit
/// sur disque ici : l'appelant écrit [`ConstructionExport::bytes`].
pub fn export_construction_dxf(
    project: &Project,
    scope: ConstructionScope,
) -> Result<ConstructionExport, ConstructionError> {
    let sheet_ids: Vec<SheetId> = match scope {
        ConstructionScope::Sheet(id) => {
            project.sheet(id)?;
            vec![id]
        }
        ConstructionScope::AllSheets => project.sheets.iter().map(|s| s.id).collect(),
    };
    let mut layers = Layers::default();
    let mut notes: BTreeMap<ConstructionNoteKind, usize> = BTreeMap::new();
    let mut entities: Vec<CEntity> = Vec::new();
    let mut infos: Vec<ConstructionSheetInfo> = Vec::new();
    let mut next_min_x: Option<f64> = None;
    for id in sheet_ids {
        let sheet = project.sheet(id)?;
        let local = sheet_entities(project, id, &mut layers, &mut notes)?;
        let mut bb: Option<Bbox> = None;
        local.iter().for_each(|e| e.bounds(&mut bb));
        let offset = match (next_min_x, bb) {
            (Some(min_x), Some(b)) => min_x - b.min.x,
            _ => 0.0,
        };
        let n = local.len();
        entities.extend(local.into_iter().map(|e| e.shifted(offset)));
        if let Some(b) = bb {
            next_min_x = Some(b.max.x + offset + SHEET_GAP_MM);
        }
        infos.push(ConstructionSheetInfo {
            name: sheet.name.clone(),
            offset_x_mm: offset,
            entities: n,
        });
    }
    if entities.is_empty() {
        return Err(ConstructionError::NothingToExport);
    }
    let mut bb: Option<Bbox> = None;
    entities.iter().for_each(|e| e.bounds(&mut bb));
    let bb = bb.expect("au moins une entité");
    let mut counts: BTreeMap<&'static str, usize> = BTreeMap::new();
    for e in &entities {
        *counts.entry(e.dxf_type()).or_default() += 1;
        layers.rows[e.layer()].entities += 1;
    }
    add_note(
        &mut notes,
        ConstructionNoteKind::HiddenLayers,
        layers
            .rows
            .iter()
            .filter(|r| r.hidden && r.entities > 0)
            .count(),
    );

    let mut w = Writer {
        s: String::new(),
        non_finite: false,
        out_of_range: false,
    };
    let dec = CONSTRUCTION_DECIMALS;
    w.pair(0, "SECTION");
    w.pair(2, "HEADER");
    w.pair(9, "$ACADVER");
    w.pair(1, DXF_VERSION);
    w.pair(9, "$INSBASE");
    w.point(10, PointMm::new(0.0, 0.0));
    w.pair(9, "$EXTMIN");
    w.real_dec(10, bb.min.x, dec);
    w.real_dec(20, bb.min.y, dec);
    w.real_dec(30, 0.0, dec);
    w.pair(9, "$EXTMAX");
    w.real_dec(10, bb.max.x, dec);
    w.real_dec(20, bb.max.y, dec);
    w.real_dec(30, 0.0, dec);
    w.pair(9, "$INSUNITS");
    w.int(70, INSUNITS_MM);
    w.pair(9, "$MEASUREMENT");
    w.int(70, MEASUREMENT_METRIC);
    w.pair(0, "ENDSEC");
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
    w.real_dec(40, 0.0, dec);
    w.pair(0, "ENDTAB");
    w.pair(0, "TABLE");
    w.pair(2, "LAYER");
    let has_zero = layers.rows.iter().any(|r| r.name == "0");
    w.int(70, layers.rows.len() as i64 + i64::from(!has_zero));
    let layer_row = |w: &mut Writer, name: &str, color: i64, locked: bool| {
        w.pair(0, "LAYER");
        w.pair(2, name);
        w.int(70, if locked { 4 } else { 0 });
        w.int(62, color);
        w.pair(6, "CONTINUOUS");
    };
    if !has_zero {
        layer_row(&mut w, "0", 7, false);
    }
    for r in &layers.rows {
        let color = role_color(r.role);
        layer_row(
            &mut w,
            &r.name,
            if r.hidden { -color } else { color },
            r.locked,
        );
    }
    w.pair(0, "ENDTAB");
    w.pair(0, "ENDSEC");
    w.pair(0, "SECTION");
    w.pair(2, "ENTITIES");
    for e in &entities {
        write_entity(&mut w, e, &layers.rows);
    }
    w.pair(0, "ENDSEC");
    w.pair(0, "EOF");
    if w.non_finite {
        return Err(ConstructionError::NonFinite);
    }
    if w.out_of_range {
        return Err(ConstructionError::OutOfRange);
    }
    Ok(ConstructionExport {
        text: w.s,
        report: ConstructionReport {
            sheets: infos,
            layers: layers
                .rows
                .iter()
                .filter(|r| r.entities > 0)
                .map(|r| ConstructionLayerUse {
                    name: r.name.clone(),
                    role: (r.role != PathRole::Construction).then_some(r.role),
                    entities: r.entities,
                })
                .collect(),
            entities: counts,
            notes: notes
                .into_iter()
                .map(|(kind, count)| ConstructionNote { kind, count })
                .collect(),
            bbox_mm: Some(bb),
        },
    })
}
