//! Boutons et boutonnières d'une pièce (US-06.4, décisions 176 à 184).
//!
//! Un **groupe** répartit `count` éléments identiques (boutons de diamètre donné, ou
//! boutonnières de longueur et largeur données) **régulièrement entre deux points** de
//! la construction : le premier élément est au premier point, le dernier au second,
//! l'espacement est `distance / (count - 1)` ; `count = 1` pose l'élément au premier
//! point. Les positions ne sont **jamais stockées** : elles sont calculées à la lecture
//! d'après les deux points, qui peuvent donc être déplacés (le groupe suit la
//! construction).
//!
//! Le **recul du bord** est un *contrôle*, pas une contrainte : c'est la distance
//! minimale souhaitée entre le **bord de l'élément** (cercle du bouton, contour de la
//! fente de la boutonnière) et le contour de la pièce. Un élément plus proche (ou qui
//! touche, traverse le contour, ou sort de la pièce) est signalé par
//! `PieceIssue::ButtonsTooCloseToEdge` ; rien n'est refusé ni déplacé. La distance est
//! calculée d'après la géométrie exacte du contour (segments, arcs : formules fermées ;
//! Béziers : minimum affiné à mieux que 1e-6 mm), voir `pf_geometry::slot`.
//!
//! Ce n'est ni un marquage (US-06.3 : une position, une forme), ni un élément interne
//! (US-04.3 : référence sans forme ni export), ni une opération de couture (E07) : les
//! fixations n'ajoutent aucune marge et ne modifient pas le contour.

use crate::command::{Outcome, MERGE_TOLERANCE_MM};
use crate::error::CoreError;
use crate::model::*;
use crate::offset::trace_of;
use crate::piece_marking::sheet_with_points;
use pf_geometry::region::{trace_geometry, EdgeGeom};
use pf_geometry::slot::{segment_edge_distance_mm, Slot};
use pf_geometry::{direction_angle_deg, distance_mm, Lasso, PointMm};
use serde::{Deserialize, Serialize};

/// Nombre maximal d'éléments d'un groupe.
pub const BUTTON_MAX_COUNT: u32 = 100;
/// Plus petit diamètre de bouton accepté (mm).
pub const BUTTON_MIN_DIAMETER_MM: f64 = 1.0;
/// Plus grand diamètre de bouton accepté (mm).
pub const BUTTON_MAX_DIAMETER_MM: f64 = 100.0;
/// Plus petite longueur (hors tout) de boutonnière acceptée (mm).
pub const BUTTONHOLE_MIN_LENGTH_MM: f64 = 2.0;
/// Plus grande longueur de boutonnière acceptée (mm).
pub const BUTTONHOLE_MAX_LENGTH_MM: f64 = 100.0;
/// Plus petite largeur de boutonnière acceptée (mm).
pub const BUTTONHOLE_MIN_WIDTH_MM: f64 = 0.5;
/// Plus grande largeur de boutonnière acceptée (mm).
pub const BUTTONHOLE_MAX_WIDTH_MM: f64 = 50.0;
/// Plus grand recul du bord accepté (mm) ; le plus petit est 0 (aucun recul exigé).
pub const BUTTON_MAX_EDGE_MARGIN_MM: f64 = 1000.0;
/// Nombre maximal de groupes par pièce.
pub const BUTTON_MAX_GROUPS_PER_PIECE: usize = 100;
/// Marge numérique (mm) sous laquelle un recul est considéré comme respecté.
const MARGIN_EPSILON_MM: f64 = 1e-9;
/// Distance (mm) en dessous de laquelle l'âme de la fente touche le contour.
const TOUCH_MM: f64 = 1e-9;

/// Remet un angle dans `[0, 360[` (jamais 360, jamais -0).
pub fn normalize_degrees(degrees: f64) -> f64 {
    let a = degrees.rem_euclid(360.0);
    if a >= 360.0 {
        0.0
    } else {
        a + 0.0
    }
}

/// Orientation de la **longueur** d'une boutonnière. Variantes à champs (même vides) :
/// une variante unitaire de serde accepterait des clés en trop (défaut D-04).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ButtonholeOrientation {
    /// Longueur parallèle à l'axe X de la feuille (0°).
    Horizontal {},
    /// Longueur parallèle à l'axe Y de la feuille (90°).
    Vertical {},
    /// Longueur perpendiculaire à l'axe des deux points du groupe (axe + 90°) : c'est
    /// l'usage d'une patte de boutonnage.
    Perpendicular {},
    /// Angle libre, en degrés depuis +X (anti-horaire), **normalisé dans [0 ; 360[** :
    /// −90 est enregistré 270.
    Angle { degrees: f64 },
}

impl ButtonholeOrientation {
    /// Identifiant stable, indépendant de la langue.
    pub fn key(self) -> &'static str {
        match self {
            ButtonholeOrientation::Horizontal {} => "horizontal",
            ButtonholeOrientation::Vertical {} => "vertical",
            ButtonholeOrientation::Perpendicular {} => "perpendicular",
            ButtonholeOrientation::Angle { .. } => "angle",
        }
    }

    /// Angle (degrés, `[0 ; 360[`) de la longueur pour un groupe dont l'axe
    /// (premier point → second point) est à `axis_deg`.
    pub fn resolve_deg(self, axis_deg: f64) -> f64 {
        match self {
            ButtonholeOrientation::Horizontal {} => 0.0,
            ButtonholeOrientation::Vertical {} => 90.0,
            ButtonholeOrientation::Perpendicular {} => normalize_degrees(axis_deg + 90.0),
            ButtonholeOrientation::Angle { degrees } => normalize_degrees(degrees),
        }
    }

    /// Même orientation après une rotation de `delta_deg` de la pièce (assemblage) :
    /// « horizontale » devient un angle libre, « perpendiculaire » reste telle quelle
    /// (l'axe tourne avec la pièce).
    pub(crate) fn rotated(self, delta_deg: f64) -> ButtonholeOrientation {
        if delta_deg.abs() < 1e-9 || (360.0 - delta_deg.abs()).abs() < 1e-9 {
            return self;
        }
        match self {
            ButtonholeOrientation::Horizontal {} => ButtonholeOrientation::Angle {
                degrees: normalize_degrees(delta_deg),
            },
            ButtonholeOrientation::Vertical {} => ButtonholeOrientation::Angle {
                degrees: normalize_degrees(90.0 + delta_deg),
            },
            ButtonholeOrientation::Perpendicular {} => self,
            ButtonholeOrientation::Angle { degrees } => ButtonholeOrientation::Angle {
                degrees: normalize_degrees(degrees + delta_deg),
            },
        }
    }
}

/// Élément répété d'un groupe. Dimensions en mm, finies et bornées (constantes
/// `BUTTON_*` / `BUTTONHOLE_*`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ButtonElement {
    /// Bouton : cercle de diamètre donné, centré sur la position.
    Button { diameter_mm: f64 },
    /// Boutonnière : fente (rectangle à bouts arrondis) de `length_mm` hors tout
    /// sur `width_mm`, centrée sur la position, tournée selon `orientation`.
    Buttonhole {
        length_mm: f64,
        width_mm: f64,
        orientation: ButtonholeOrientation,
    },
}

fn check_range(value: f64, min: f64, max: f64, what: &str) -> Result<(), CoreError> {
    if value.is_finite() && (min..=max).contains(&value) {
        Ok(())
    } else {
        Err(CoreError::Invalid(format!(
            "{what} doit être un nombre fini compris entre {min} et {max} mm"
        )))
    }
}

impl ButtonElement {
    /// Identifiant stable, indépendant de la langue.
    pub fn key(self) -> &'static str {
        match self {
            ButtonElement::Button { .. } => "button",
            ButtonElement::Buttonhole { .. } => "buttonhole",
        }
    }

    /// Forme enregistrée valide ? (angle déjà dans `[0 ; 360[`, largeur ≤ longueur.)
    pub fn validate_stored(self) -> Result<(), CoreError> {
        match self {
            ButtonElement::Button { diameter_mm } => check_range(
                diameter_mm,
                BUTTON_MIN_DIAMETER_MM,
                BUTTON_MAX_DIAMETER_MM,
                "le diamètre d'un bouton",
            ),
            ButtonElement::Buttonhole {
                length_mm,
                width_mm,
                orientation,
            } => {
                check_range(
                    length_mm,
                    BUTTONHOLE_MIN_LENGTH_MM,
                    BUTTONHOLE_MAX_LENGTH_MM,
                    "la longueur d'une boutonnière",
                )?;
                check_range(
                    width_mm,
                    BUTTONHOLE_MIN_WIDTH_MM,
                    BUTTONHOLE_MAX_WIDTH_MM,
                    "la largeur d'une boutonnière",
                )?;
                if width_mm > length_mm {
                    return Err(CoreError::Invalid(
                        "la largeur d'une boutonnière ne peut pas dépasser sa longueur".into(),
                    ));
                }
                if let ButtonholeOrientation::Angle { degrees } = orientation {
                    if !degrees.is_finite() || !(0.0..360.0).contains(&degrees) {
                        return Err(CoreError::Invalid(
                            "l'angle d'une boutonnière doit être compris dans [0 ; 360[ degrés"
                                .into(),
                        ));
                    }
                }
                Ok(())
            }
        }
    }

    /// Forme d'une commande : angle libre ramené dans `[0 ; 360[` (refus s'il n'est
    /// pas fini), puis mêmes contrôles que [`ButtonElement::validate_stored`].
    pub fn normalized(self) -> Result<ButtonElement, CoreError> {
        let out = match self {
            ButtonElement::Buttonhole {
                length_mm,
                width_mm,
                orientation: ButtonholeOrientation::Angle { degrees },
            } => {
                if !degrees.is_finite() {
                    return Err(CoreError::Invalid(
                        "l'angle d'une boutonnière doit être un nombre fini".into(),
                    ));
                }
                ButtonElement::Buttonhole {
                    length_mm,
                    width_mm,
                    orientation: ButtonholeOrientation::Angle {
                        degrees: normalize_degrees(degrees),
                    },
                }
            }
            other => other,
        };
        out.validate_stored()?;
        Ok(out)
    }

    /// Élément résolu pour un groupe dont l'axe est à `axis_deg`.
    pub fn shape(self, axis_deg: f64) -> ButtonShape {
        match self {
            ButtonElement::Button { diameter_mm } => ButtonShape::Button { diameter_mm },
            ButtonElement::Buttonhole {
                length_mm,
                width_mm,
                orientation,
            } => ButtonShape::Buttonhole {
                length_mm,
                width_mm,
                angle_deg: orientation.resolve_deg(axis_deg),
            },
        }
    }

    pub(crate) fn rotated(self, delta_deg: f64) -> ButtonElement {
        match self {
            ButtonElement::Buttonhole {
                length_mm,
                width_mm,
                orientation,
            } => ButtonElement::Buttonhole {
                length_mm,
                width_mm,
                orientation: orientation.rotated(delta_deg),
            },
            other => other,
        }
    }
}

/// Groupe de boutons ou de boutonnières d'une pièce : identifiant stable, deux
/// points de la feuille de la pièce, nombre d'éléments, élément et recul du bord.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ButtonGroup {
    pub id: ButtonGroupId,
    /// Position du premier élément.
    pub from: PointId,
    /// Position du dernier élément. Les deux points restent distincts même pour un seul
    /// élément : ils définissent aussi l'axe (orientation « perpendiculaire »).
    pub to: PointId,
    /// Nombre d'éléments, de 1 à [`BUTTON_MAX_COUNT`].
    pub count: u32,
    pub element: ButtonElement,
    /// Recul du bord souhaité (mm, ≥ 0) : contrôle affiché, jamais une contrainte.
    pub edge_margin_mm: f64,
}

/// Élément d'un groupe tel qu'il est dessiné, en mm.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ButtonShape {
    /// Cercle de diamètre donné.
    Button { diameter_mm: f64 },
    /// Fente de `length_mm` hors tout sur `width_mm`, longueur à `angle_deg`
    /// (`[0 ; 360[`, depuis +X, anti-horaire).
    Buttonhole {
        length_mm: f64,
        width_mm: f64,
        angle_deg: f64,
    },
}

impl ButtonShape {
    /// Identifiant stable, indépendant de la langue.
    pub fn key(self) -> &'static str {
        match self {
            ButtonShape::Button { .. } => "button",
            ButtonShape::Buttonhole { .. } => "buttonhole",
        }
    }

    /// Rayon du bouton, ou demi-largeur de la boutonnière (mm).
    pub fn radius_mm(self) -> f64 {
        match self {
            ButtonShape::Button { diameter_mm } => diameter_mm / 2.0,
            ButtonShape::Buttonhole { width_mm, .. } => width_mm / 2.0,
        }
    }

    /// Fente de la boutonnière centrée en `center` ; `None` pour un bouton.
    pub fn slot(self, center: PointMm) -> Option<Slot> {
        match self {
            ButtonShape::Button { .. } => None,
            ButtonShape::Buttonhole {
                length_mm,
                width_mm,
                angle_deg,
            } => Some(Slot {
                center,
                length_mm,
                width_mm,
                angle_deg,
            }),
        }
    }

    /// Âme de l'élément : les deux centres de bouts de la fente (confondus avec le
    /// centre pour un bouton). L'élément est l'ensemble des points à au plus
    /// `radius_mm` de cette âme.
    pub fn spine(self, center: PointMm) -> (PointMm, PointMm) {
        self.slot(center)
            .map_or((center, center), |slot| slot.spine())
    }

    /// Demi-emprise (mm) selon X puis selon Y, boîte alignée sur les axes.
    pub fn half_extent(self, center: PointMm) -> (f64, f64) {
        match self.slot(center) {
            Some(slot) => slot.half_extent(),
            None => (self.radius_mm(), self.radius_mm()),
        }
    }
}

/// Un élément d'un groupe, avec sa position et son contrôle de recul.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResolvedButton {
    /// Rang dans le groupe, à partir de 0 (0 = au premier point).
    pub index: usize,
    /// Centre de l'élément, en mm.
    pub center: PointMm,
    pub shape: ButtonShape,
    /// Distance signée (mm) entre le bord de l'élément et le contour de la pièce :
    /// positive quand l'élément est entièrement dans la pièce, **négative** quand il
    /// touche ou traverse le contour (−rayon) ou est dehors (−distance − rayon).
    /// `None` : pas de jugement possible (contour absent, ouvert ou illisible).
    pub clearance_mm: Option<f64>,
    /// `clearance_mm` plus petit que le recul demandé par le groupe.
    pub too_close: bool,
}

/// Un groupe et la position actuelle de ses éléments (calculée d'après la construction).
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedButtonGroup {
    pub group: ButtonGroup,
    pub from: PointMm,
    pub to: PointMm,
    /// Angle (degrés, `[0 ; 360[`) de l'axe premier point → second point.
    pub axis_deg: f64,
    /// Espacement entre deux éléments consécutifs (mm) ; 0 pour un seul élément.
    pub spacing_mm: f64,
    pub elements: Vec<ResolvedButton>,
}

/// Positions de `count` éléments répartis de `from` à `to` : le premier en `from`, le
/// dernier exactement en `to`, espacement égal à `distance / (count - 1)`.
pub fn distribute(from: PointMm, to: PointMm, count: u32) -> Vec<PointMm> {
    let n = count.max(1) as usize;
    (0..n)
        .map(|i| {
            if i == 0 || n == 1 {
                from
            } else if i == n - 1 {
                to
            } else {
                from.lerp(to, i as f64 / (n - 1) as f64)
            }
        })
        .collect()
}

/// Contour fermé d'une pièce en arêtes élémentaires, avec un test d'appartenance.
struct Outline {
    edges: Vec<EdgeGeom>,
    lasso: Lasso,
}

impl Outline {
    /// Voir [`ResolvedButton::clearance_mm`].
    fn clearance_mm(&self, shape: ButtonShape, center: PointMm) -> f64 {
        let (a, b) = shape.spine(center);
        let r = shape.radius_mm();
        let d = self
            .edges
            .iter()
            .map(|e| segment_edge_distance_mm(a, b, e))
            .fold(f64::INFINITY, f64::min);
        if d <= TOUCH_MM {
            -r
        } else if self.lasso.contains_point(center) {
            d - r
        } else {
            -(d + r)
        }
    }
}

impl Project {
    fn outline_geometry(&self, piece: PieceId) -> Option<Outline> {
        let contour = self.piece_contour(piece).ok()?;
        if !contour.closed {
            return None;
        }
        let sheet = self.piece_sheet(piece).ok()?;
        let lasso = self.outline_lasso(piece).ok()?;
        let mut edges = Vec::new();
        for e in &contour.edges {
            let shape = sheet.shape(e.entity).ok()?;
            edges.extend(trace_geometry(&trace_of(shape, e.reversed)));
        }
        (!edges.is_empty()).then_some(Outline { edges, lasso })
    }

    /// Positions des deux points d'un groupe (`None` : point absent de la feuille de
    /// la pièce, illisible, ou points confondus).
    pub(crate) fn button_ends(
        &self,
        link: PieceLink,
        g: &ButtonGroup,
    ) -> Option<(PointMm, PointMm)> {
        let sheet = sheet_with_points(self, link, &[g.from, g.to])?;
        let (a, b) = (sheet.position(g.from).ok()?, sheet.position(g.to).ok()?);
        (distance_mm(a, b) > MERGE_TOLERANCE_MM).then_some((a, b))
    }

    fn resolve_button_group(
        &self,
        link: PieceLink,
        g: &ButtonGroup,
        outline: Option<&Outline>,
    ) -> Option<ResolvedButtonGroup> {
        let (a, b) = self.button_ends(link, g)?;
        let axis_deg = direction_angle_deg(a, b).ok()?;
        let shape = g.element.shape(axis_deg);
        let elements = distribute(a, b, g.count)
            .into_iter()
            .enumerate()
            .map(|(index, center)| {
                let clearance_mm = outline.map(|o| o.clearance_mm(shape, center));
                ResolvedButton {
                    index,
                    center,
                    shape,
                    clearance_mm,
                    too_close: clearance_mm
                        .is_some_and(|c| c < g.edge_margin_mm - MARGIN_EPSILON_MM),
                }
            })
            .collect();
        Some(ResolvedButtonGroup {
            group: *g,
            from: a,
            to: b,
            axis_deg,
            spacing_mm: if g.count > 1 {
                distance_mm(a, b) / f64::from(g.count - 1)
            } else {
                0.0
            },
            elements,
        })
    }

    /// Groupes de boutons d'une pièce avec la position et le contrôle de recul de
    /// chaque élément (lecture pure ; liste vide pour une pièce inconnue). Un groupe
    /// dont les points ne se lisent plus est impossible dans un projet valide.
    pub fn piece_buttons(&self, piece: PieceId) -> Vec<ResolvedButtonGroup> {
        let Ok(p) = self.piece(piece) else {
            return Vec::new();
        };
        if p.buttons.is_empty() {
            return Vec::new();
        }
        let outline = self.outline_geometry(piece);
        p.buttons
            .iter()
            .filter_map(|g| self.resolve_button_group(p.link, g, outline.as_ref()))
            .collect()
    }

    /// Nombre d'éléments d'une pièce plus proches du bord que le recul demandé.
    pub fn piece_buttons_too_close(&self, piece: PieceId) -> usize {
        self.piece_buttons(piece)
            .iter()
            .flat_map(|g| &g.elements)
            .filter(|e| e.too_close)
            .count()
    }

    /// Éléments de toutes les pièces dont le contour est sur cette feuille : centre en
    /// mm et forme, dans l'ordre des pièces, des groupes puis des éléments. Sert à
    /// l'export.
    pub fn sheet_buttons(&self, sheet: SheetId) -> Vec<(PointMm, ButtonShape)> {
        self.pieces_on(sheet)
            .into_iter()
            .filter(|p| !p.buttons.is_empty())
            .flat_map(|p| {
                p.buttons
                    .iter()
                    .filter_map(|g| self.resolve_button_group(p.link, g, None))
                    .flat_map(|r| r.elements.into_iter().map(|e| (e.center, e.shape)))
            })
            .collect()
    }

    /// Contrôle d'un groupe pour une pièce : forme, nombre, recul, deux points distincts
    /// sur la feuille de la pièce.
    fn check_button_group(
        &self,
        piece_name: &str,
        link: PieceLink,
        g: &ButtonGroup,
    ) -> Result<(), CoreError> {
        if !(1..=BUTTON_MAX_COUNT).contains(&g.count) {
            return Err(CoreError::Invalid(format!(
                "le nombre d'éléments d'un groupe doit être compris entre 1 et {BUTTON_MAX_COUNT}"
            )));
        }
        g.element.validate_stored()?;
        if !g.edge_margin_mm.is_finite()
            || !(0.0..=BUTTON_MAX_EDGE_MARGIN_MM).contains(&g.edge_margin_mm)
        {
            return Err(CoreError::Invalid(format!(
                "le recul du bord doit être un nombre fini compris entre 0 et \
                 {BUTTON_MAX_EDGE_MARGIN_MM} mm"
            )));
        }
        if g.from == g.to {
            return Err(CoreError::Invalid(
                "les deux points d'un groupe de boutons doivent être deux points différents".into(),
            ));
        }
        let points = [g.from, g.to];
        if sheet_with_points(self, link, &points).is_none() {
            let missing = points
                .iter()
                .find(|&&p| !self.sheets.iter().any(|s| s.point(p).is_ok()));
            return Err(match missing {
                Some(&p) => CoreError::UnknownPoint(p),
                None => CoreError::DeadReference(format!(
                    "un groupe de boutons de la pièce « {piece_name} » référence un point absent \
                     de la feuille de la pièce"
                )),
            });
        }
        if self.button_ends(link, g).is_none() {
            return Err(CoreError::Invalid(format!(
                "les deux points du groupe de boutons de la pièce « {piece_name} » sont \
                 confondus ou illisibles : la répartition n'est pas définie"
            )));
        }
        Ok(())
    }

    /// Validation d'une pièce chargée ou modifiée : nombre de groupes et contrôle de chacun.
    pub(crate) fn validate_buttons(&self, piece: &Piece) -> Result<(), CoreError> {
        if piece.buttons.len() > BUTTON_MAX_GROUPS_PER_PIECE {
            return Err(CoreError::Invalid(format!(
                "la pièce « {} » porte plus de {BUTTON_MAX_GROUPS_PER_PIECE} groupes de boutons",
                piece.name
            )));
        }
        for g in &piece.buttons {
            self.check_button_group(&piece.name, piece.link, g)?;
        }
        Ok(())
    }

    fn button_index(&self, piece: PieceId, group: ButtonGroupId) -> Result<usize, CoreError> {
        let p = self.piece(piece)?;
        p.buttons.iter().position(|g| g.id == group).ok_or_else(|| {
            CoreError::Invalid(format!(
                "le groupe de boutons {} n'existe pas sur la pièce « {} »",
                group.0, p.name
            ))
        })
    }

    /// Ajoute un groupe (refus expliqué : pièce sans feuille, point absent ou d'une
    /// autre feuille, points confondus, nombre ou dimension hors bornes, recul négatif
    /// ou non fini, trop de groupes).
    pub(crate) fn add_button_group(
        &mut self,
        piece: PieceId,
        spec: ButtonGroup,
        out: &mut Outcome,
    ) -> Result<(), CoreError> {
        let p = self.piece(piece)?;
        let (name, link) = (p.name.clone(), p.link);
        if self.piece_sheet(piece).is_err() {
            return Err(CoreError::Invalid(format!(
                "la pièce « {name} » n'a pas de contour : on ne sait pas sur quelle feuille \
                 chercher les points"
            )));
        }
        if p.buttons.len() >= BUTTON_MAX_GROUPS_PER_PIECE {
            return Err(CoreError::Invalid(format!(
                "la pièce « {name} » porte déjà {BUTTON_MAX_GROUPS_PER_PIECE} groupes de boutons"
            )));
        }
        let mut group = spec;
        group.element = spec.element.normalized()?;
        self.check_button_group(&name, link, &group)?;
        group.id = ButtonGroupId(self.alloc_id());
        self.piece_mut(piece)?.buttons.push(group);
        out.button_groups.push(group.id);
        Ok(())
    }

    pub(crate) fn remove_button_group(
        &mut self,
        piece: PieceId,
        group: ButtonGroupId,
    ) -> Result<(), CoreError> {
        let i = self.button_index(piece, group)?;
        self.piece_mut(piece)?.buttons.remove(i);
        Ok(())
    }

    /// Remplace points, nombre, élément et recul d'un groupe (même identifiant, même rang).
    pub(crate) fn set_button_group(
        &mut self,
        piece: PieceId,
        spec: ButtonGroup,
    ) -> Result<(), CoreError> {
        let i = self.button_index(piece, spec.id)?;
        let p = self.piece(piece)?;
        let (name, link) = (p.name.clone(), p.link);
        let mut group = spec;
        group.element = spec.element.normalized()?;
        self.check_button_group(&name, link, &group)?;
        self.piece_mut(piece)?.buttons[i] = group;
        Ok(())
    }

    /// Retire les groupes dont un point a disparu pendant la commande, avec une
    /// remarque. Ne touche que les groupes qui existaient avant la commande : une
    /// référence morte introduite par la commande elle-même reste refusée.
    pub(crate) fn prune_dead_buttons(&mut self, before: &Project, out: &mut Outcome) {
        if self.pieces.iter().all(|p| p.buttons.is_empty()) {
            return;
        }
        let dead: Vec<(PieceId, Vec<ButtonGroupId>)> = self
            .pieces
            .iter()
            .filter(|p| !p.buttons.is_empty())
            .map(|p| {
                let old = before.pieces.iter().find(|o| o.id == p.id);
                let gone = p
                    .buttons
                    .iter()
                    .filter(|g| {
                        old.is_some_and(|o| o.buttons.contains(g))
                            && sheet_with_points(self, p.link, &[g.from, g.to]).is_none()
                    })
                    .map(|g| g.id)
                    .collect();
                (p.id, gone)
            })
            .filter(|(_, gone): &(PieceId, Vec<ButtonGroupId>)| !gone.is_empty())
            .collect();
        for (id, gone) in dead {
            if let Ok(p) = self.piece_mut(id) {
                p.buttons.retain(|g| !gone.contains(&g.id));
                for _ in &gone {
                    out.notes.push(format!(
                        "Un groupe de boutons de la pièce « {} » référençait un point supprimé : \
                         il est retiré.",
                        p.name
                    ));
                }
            }
        }
    }

    /// Retire les groupes d'une pièce dérivée dont un point a été recréé ou dont les
    /// deux points sont devenus confondus après la régénération (jamais un refus).
    pub(crate) fn prune_own_buttons(&mut self, piece: PieceId, out: &mut Outcome) {
        let Ok(p) = self.piece(piece) else { return };
        let gone: Vec<ButtonGroupId> = p
            .buttons
            .iter()
            .filter(|g| self.button_ends(p.link, g).is_none())
            .map(|g| g.id)
            .collect();
        if gone.is_empty() {
            return;
        }
        if let Ok(p) = self.piece_mut(piece) {
            p.buttons.retain(|g| !gone.contains(&g.id));
            for _ in &gone {
                out.notes.push(format!(
                    "Un groupe de boutons de la pièce « {} » dépendait d'un point recréé ou \
                     confondu par la mise à jour : il est retiré.",
                    p.name
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotation_de_la_piece_et_orientation() {
        use ButtonholeOrientation as O;
        assert_eq!(O::Horizontal {}.rotated(0.0), O::Horizontal {});
        assert_eq!(O::Horizontal {}.rotated(360.0), O::Horizontal {});
        assert_eq!(O::Horizontal {}.rotated(-90.0), O::Angle { degrees: 270.0 });
        assert_eq!(O::Vertical {}.rotated(90.0), O::Angle { degrees: 180.0 });
        assert_eq!(O::Perpendicular {}.rotated(33.0), O::Perpendicular {});
        assert_eq!(
            O::Angle { degrees: 350.0 }.rotated(30.0),
            O::Angle { degrees: 20.0 }
        );
        let el = ButtonElement::Button { diameter_mm: 9.0 };
        assert_eq!(el.rotated(45.0), el);
    }

    #[test]
    fn angle_resolu_pour_chaque_orientation() {
        use ButtonholeOrientation as O;
        assert_eq!(O::Horizontal {}.resolve_deg(123.0), 0.0);
        assert_eq!(O::Vertical {}.resolve_deg(123.0), 90.0);
        assert_eq!(O::Perpendicular {}.resolve_deg(300.0), 30.0);
        assert_eq!(O::Angle { degrees: -90.0 }.resolve_deg(0.0), 270.0);
    }
}
