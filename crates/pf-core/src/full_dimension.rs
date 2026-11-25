//! US-06.7 — cotations complètes : longueur développée d'un tracé, angle, rayon et diamètre,
//! projections horizontale et verticale, avec texte orienté le long de la ligne de cote et
//! légende libre (décisions 204 à 214).
//!
//! Ce sont des **mesures observées**, jamais des contraintes : aucune donnée de mesure n'est
//! stockée, la valeur est recalculée à chaque lecture depuis la construction (elle « suit » la
//! cible quand on la déplace) et la cote ne force rien. Seuls sont stockés la **cible** (points ou
//! tracé de la feuille), la **mise en page** (décalage de la ligne, rayon de l'arc d'angle,
//! direction du trait de rayon, décalage du texte) et la **légende**. Elles s'ajoutent aux
//! cotations entre deux points d'US-02 (`Sheet.dimensions`, module `dimension`), qui restent
//! inchangées : aucun doublon (la distance entre deux points reste la cotation existante).
//!
//! Une cote dont la cible est supprimée est **retirée avec remarque** dans la même étape
//! (`prune_dead_full_dimensions`), comme les éléments liés existants : jamais de référence
//! morte. Une cote dont la géométrie devient indéfinie après un déplacement (côtés d'angle
//! superposés, extrémités confondues…) n'est ni refusée ni supprimée : elle est « non
//! calculable » (`Sheet::full_dimension_geometry` renvoie une erreur expliquée) et n'est ni
//! dessinée ni exportée tant que la construction ne la rend pas de nouveau définie.

use crate::command::Outcome;
use crate::dimension::MAX_DIMENSION_OFFSET_MM;
use crate::error::CoreError;
use crate::measure;
use crate::model::*;
use pf_geometry::{
    ccw_angle_deg, readable_text_angle_deg, sin_cos_deg, PointMm, VectorMm,
    DEFAULT_LENGTH_TOLERANCE_MM,
};
use serde::{Deserialize, Serialize};

/// Nombre maximal de cotes complètes par feuille.
pub const MAX_FULL_DIMENSIONS_PER_SHEET: usize = 500;
/// Longueur maximale d'une légende, en caractères (accents autorisés).
pub const MAX_FULL_DIMENSION_LABEL_CHARS: usize = 40;
/// Décalage maximal (valeur absolue, mm) d'une ligne de cote ou du texte le long de la ligne.
pub const MAX_FULL_DIMENSION_OFFSET_MM: f64 = MAX_DIMENSION_OFFSET_MM;
/// Rayon minimal de l'arc d'une cote d'angle (mm).
pub const MIN_ANGLE_ARC_RADIUS_MM: f64 = 1.0;
/// Rayon maximal de l'arc d'une cote d'angle (mm).
pub const MAX_ANGLE_ARC_RADIUS_MM: f64 = 10_000.0;
/// Rayon par défaut de l'arc d'une cote d'angle créée depuis l'interface (mm).
pub const DEFAULT_ANGLE_ARC_RADIUS_MM: f64 = 20.0;
/// En dessous de cet écart (degrés) à 0° ou à 360°, les deux côtés d'un angle sont
/// considérés comme superposés : l'angle est indéfini.
pub const SUPERPOSED_RAYS_DEG: f64 = 1e-6;
/// Remontée (mm) du texte au-dessus de la ligne de cote, perpendiculairement à celle-ci,
/// dans le sens de lecture (à gauche de la direction du texte).
pub const TEXT_LIFT_MM: f64 = 1.0;

/// Ce que mesure la cote, avec sa mise en page. Les identifiants cités sont ceux de la
/// feuille qui porte la cote.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum FullDimensionKind {
    /// Longueur développée d'un tracé (segment, courbe, arc, courbe par points de passage ;
    /// un cercle n'a pas d'extrémités : voir `Radius` / `Diameter`). La ligne de cote est
    /// parallèle à la **corde** du tracé, décalée de `offset_mm` (positif à gauche du sens
    /// départ → arrivée).
    PathLength { entity: EntityId, offset_mm: f64 },
    /// Angle de `first` à `second` autour de `vertex`, mesuré dans le sens **anti-horaire**
    /// (convention du projet), dans `]0° ; 360°[` ; un angle plat (180°) est accepté, deux
    /// côtés superposés (0° ou 360°) sont refusés. `radius_mm` est le rayon de l'arc de cote.
    Angle {
        vertex: PointId,
        first: PointId,
        second: PointId,
        radius_mm: f64,
    },
    /// Rayon d'un arc ou d'un cercle ; `angle_deg` ∈ `[0 ; 360[` est la direction du trait
    /// depuis le centre.
    Radius { entity: EntityId, angle_deg: f64 },
    /// Diamètre (2 r) d'un arc ou d'un cercle ; `angle_deg` ∈ `[0 ; 360[` est la direction du
    /// trait passant par le centre.
    Diameter { entity: EntityId, angle_deg: f64 },
    /// Projection **signée** sur l'axe X de `from` vers `to` (ΔX = x(to) − x(from)). La ligne
    /// de cote est horizontale, à l'ordonnée de `from` plus `offset_mm`.
    Horizontal {
        from: PointId,
        to: PointId,
        offset_mm: f64,
    },
    /// Projection **signée** sur l'axe Y de `from` vers `to` (ΔY = y(to) − y(from)). La ligne
    /// de cote est verticale, à l'abscisse de `from` plus `offset_mm`.
    Vertical {
        from: PointId,
        to: PointId,
        offset_mm: f64,
    },
}

/// Cote complète d'une feuille (mesure observée, jamais une contrainte).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FullDimension {
    pub id: FullDimensionId,
    pub kind: FullDimensionKind,
    /// Légende libre (au plus [`MAX_FULL_DIMENSION_LABEL_CHARS`] caractères, sans caractère
    /// de contrôle, jamais vide ni bordée d'espaces) ; omise quand il n'y en a pas.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Décalage du texte le long de la ligne de cote, depuis son milieu (mm, dans le sens
    /// de lecture) ; omis quand il est nul.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub text_shift_mm: f64,
}

fn is_zero(v: &f64) -> bool {
    *v == 0.0
}

impl FullDimensionKind {
    /// Identifiant stable, identique à la valeur écrite dans le fichier.
    pub fn key(&self) -> &'static str {
        match self {
            FullDimensionKind::PathLength { .. } => "path_length",
            FullDimensionKind::Angle { .. } => "angle",
            FullDimensionKind::Radius { .. } => "radius",
            FullDimensionKind::Diameter { .. } => "diameter",
            FullDimensionKind::Horizontal { .. } => "horizontal",
            FullDimensionKind::Vertical { .. } => "vertical",
        }
    }

    /// Points cités par la cote.
    pub fn points(&self) -> Vec<PointId> {
        match *self {
            FullDimensionKind::Angle {
                vertex,
                first,
                second,
                ..
            } => vec![vertex, first, second],
            FullDimensionKind::Horizontal { from, to, .. }
            | FullDimensionKind::Vertical { from, to, .. } => vec![from, to],
            _ => Vec::new(),
        }
    }

    /// Tracé cité par la cote, s'il y en a un.
    pub fn entity(&self) -> Option<EntityId> {
        match *self {
            FullDimensionKind::PathLength { entity, .. }
            | FullDimensionKind::Radius { entity, .. }
            | FullDimensionKind::Diameter { entity, .. } => Some(entity),
            _ => None,
        }
    }

    /// La même cote vers d'autres objets (copie de feuille, collage) ; une référence que
    /// `pt` ou `en` refuse fait échouer la réécriture.
    pub(crate) fn remap(
        &self,
        pt: &dyn Fn(PointId) -> Result<PointId, CoreError>,
        en: &dyn Fn(EntityId) -> Result<EntityId, CoreError>,
    ) -> Result<FullDimensionKind, CoreError> {
        Ok(match *self {
            FullDimensionKind::PathLength { entity, offset_mm } => FullDimensionKind::PathLength {
                entity: en(entity)?,
                offset_mm,
            },
            FullDimensionKind::Angle {
                vertex,
                first,
                second,
                radius_mm,
            } => FullDimensionKind::Angle {
                vertex: pt(vertex)?,
                first: pt(first)?,
                second: pt(second)?,
                radius_mm,
            },
            FullDimensionKind::Radius { entity, angle_deg } => FullDimensionKind::Radius {
                entity: en(entity)?,
                angle_deg,
            },
            FullDimensionKind::Diameter { entity, angle_deg } => FullDimensionKind::Diameter {
                entity: en(entity)?,
                angle_deg,
            },
            FullDimensionKind::Horizontal {
                from,
                to,
                offset_mm,
            } => FullDimensionKind::Horizontal {
                from: pt(from)?,
                to: pt(to)?,
                offset_mm,
            },
            FullDimensionKind::Vertical {
                from,
                to,
                offset_mm,
            } => FullDimensionKind::Vertical {
                from: pt(from)?,
                to: pt(to)?,
                offset_mm,
            },
        })
    }

    /// Valeur de mise en page : décalage (mm) pour la longueur et les projections, rayon de
    /// l'arc (mm) pour l'angle, direction du trait (degrés) pour le rayon et le diamètre.
    pub fn placement(&self) -> f64 {
        match *self {
            FullDimensionKind::PathLength { offset_mm, .. }
            | FullDimensionKind::Horizontal { offset_mm, .. }
            | FullDimensionKind::Vertical { offset_mm, .. } => offset_mm,
            FullDimensionKind::Angle { radius_mm, .. } => radius_mm,
            FullDimensionKind::Radius { angle_deg, .. }
            | FullDimensionKind::Diameter { angle_deg, .. } => angle_deg,
        }
    }

    /// La même cote avec une autre mise en page (voir [`Self::placement`]) ; la valeur est
    /// vérifiée, un angle de direction est ramené dans `[0 ; 360[`.
    pub fn with_placement(self, value: f64) -> Result<FullDimensionKind, CoreError> {
        let bad = |m: &str| CoreError::Invalid(format!("cote : {m}"));
        let mut k = self;
        match &mut k {
            FullDimensionKind::PathLength { offset_mm, .. }
            | FullDimensionKind::Horizontal { offset_mm, .. }
            | FullDimensionKind::Vertical { offset_mm, .. } => {
                if !value.is_finite() || value.abs() > MAX_FULL_DIMENSION_OFFSET_MM {
                    return Err(bad(&format!(
                        "le décalage doit être un nombre d'au plus {MAX_FULL_DIMENSION_OFFSET_MM} \
                         mm en valeur absolue"
                    )));
                }
                // `-0.0` et `0.0` sont le même décalage : un seul codage dans le fichier.
                *offset_mm = value + 0.0;
            }
            FullDimensionKind::Angle { radius_mm, .. } => {
                if !value.is_finite()
                    || !(MIN_ANGLE_ARC_RADIUS_MM..=MAX_ANGLE_ARC_RADIUS_MM).contains(&value)
                {
                    return Err(bad(&format!(
                        "le rayon de l'arc d'angle doit être compris entre \
                         {MIN_ANGLE_ARC_RADIUS_MM} et {MAX_ANGLE_ARC_RADIUS_MM} mm"
                    )));
                }
                *radius_mm = value;
            }
            FullDimensionKind::Radius { angle_deg, .. }
            | FullDimensionKind::Diameter { angle_deg, .. } => {
                if !value.is_finite() {
                    return Err(bad("la direction du trait doit être un nombre fini"));
                }
                let a = value.rem_euclid(360.0);
                *angle_deg = if a >= 360.0 { 0.0 } else { a };
            }
        }
        Ok(k)
    }

    /// Contrôles qui ne dépendent pas de la feuille : nombres finis et bornés, points
    /// distincts. Appelée à la création et à l'ouverture d'un fichier.
    pub fn validate_static(&self) -> Result<(), CoreError> {
        let bad = |m: &str| CoreError::Invalid(format!("cote : {m}"));
        let distinct = match *self {
            FullDimensionKind::Angle {
                vertex,
                first,
                second,
                ..
            } => (vertex != first && vertex != second && first != second)
                .then_some(())
                .ok_or("les trois points de l'angle doivent être différents"),
            FullDimensionKind::Horizontal { from, to, .. }
            | FullDimensionKind::Vertical { from, to, .. } => (from != to)
                .then_some(())
                .ok_or("les deux points doivent être différents"),
            _ => Ok(()),
        };
        distinct.map_err(bad)?;
        let placement = self.placement();
        let normalized = self.with_placement(placement)?;
        // Un angle de direction écrit hors de `[0 ; 360[` serait ramené : le fichier doit
        // déjà être sous forme normale (relecture identique à l'octet).
        if normalized.placement() != placement {
            return Err(bad("valeur de mise en page non normalisée"));
        }
        Ok(())
    }
}

/// Contrôle d'une légende (voir [`FullDimension::label`]) et mise sous forme normale : les
/// espaces de début et de fin sont retirés, une légende vide devient « pas de légende ».
pub fn normalize_label(label: Option<&str>) -> Result<Option<String>, CoreError> {
    let Some(text) = label else { return Ok(None) };
    if text.chars().any(char::is_control) {
        return Err(CoreError::Invalid(
            "cote : la légende contient un caractère de contrôle".into(),
        ));
    }
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    if text.chars().count() > MAX_FULL_DIMENSION_LABEL_CHARS {
        return Err(CoreError::Invalid(format!(
            "cote : la légende dépasse {MAX_FULL_DIMENSION_LABEL_CHARS} caractères"
        )));
    }
    Ok(Some(text.to_string()))
}

/// Contrôle du décalage du texte.
pub fn check_text_shift(shift_mm: f64) -> Result<f64, CoreError> {
    if !shift_mm.is_finite() || shift_mm.abs() > MAX_FULL_DIMENSION_OFFSET_MM {
        return Err(CoreError::Invalid(format!(
            "cote : le décalage du texte doit être un nombre d'au plus \
             {MAX_FULL_DIMENSION_OFFSET_MM} mm en valeur absolue"
        )));
    }
    Ok(shift_mm + 0.0)
}

/// Ligne principale de la cote.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DimensionLine {
    /// Ligne droite de `a` à `b` ; flèches aux extrémités indiquées.
    Straight {
        a: PointMm,
        b: PointMm,
        arrow_a: bool,
        arrow_b: bool,
    },
    /// Arc de cercle de centre `center` : départ à `start_deg`, balayage anti-horaire
    /// `sweep_deg` ∈ `]0 ; 360[`.
    Arc {
        center: PointMm,
        radius_mm: f64,
        start_deg: f64,
        sweep_deg: f64,
    },
}

/// Géométrie calculée d'une cote : tout ce qu'il faut pour la dessiner et l'exporter, en mm.
#[derive(Debug, Clone, PartialEq)]
pub struct FullDimensionGeometry {
    pub id: FullDimensionId,
    pub kind: FullDimensionKind,
    /// Valeur mesurée : mm (longueur, rayon, diamètre, projection **signée**) ou degrés
    /// (angle anti-horaire dans `]0 ; 360[`).
    pub value: f64,
    /// Traits d'attache (du point coté vers la ligne de cote).
    pub extension_lines: Vec<(PointMm, PointMm)>,
    pub line: DimensionLine,
    /// Centre du texte, **sur** la ligne de cote (décalage du texte compris) ; le dessin le
    /// remonte de [`TEXT_LIFT_MM`] perpendiculairement.
    pub text_anchor: PointMm,
    /// Angle du texte (degrés, sens trigonométrique) : direction de la ligne de cote ramenée
    /// dans `]-90 ; 90]`, donc jamais à l'envers.
    pub text_angle_deg: f64,
    /// Légende, s'il y en a une.
    pub label: Option<String>,
    /// Valeur affichée avec son unité au pas de 0,01 (« 123.45 mm », « R 12.50 mm »,
    /// « 45.00° »), point décimal.
    pub value_text: String,
}

impl FullDimensionGeometry {
    /// Texte complet : « légende : valeur » ou la valeur seule, avec le séparateur décimal
    /// demandé (l'export utilise toujours le point).
    pub fn text(&self, decimal: char) -> String {
        let value = if decimal == '.' {
            self.value_text.clone()
        } else {
            self.value_text.replace('.', &decimal.to_string())
        };
        match &self.label {
            Some(l) => format!("{l} : {value}"),
            None => value,
        }
    }

    /// Vecteur unitaire du sens de lecture du texte.
    pub fn text_direction(&self) -> VectorMm {
        let (s, c) = sin_cos_deg(self.text_angle_deg);
        VectorMm::new(c, s)
    }

    /// Point où se pose le centre du texte : le point d'ancrage remonté de [`TEXT_LIFT_MM`]
    /// à gauche du sens de lecture.
    pub fn text_center(&self) -> PointMm {
        self.text_anchor + self.text_direction().rotated_left() * TEXT_LIFT_MM
    }
}

fn bad(msg: impl Into<String>) -> CoreError {
    CoreError::Invalid(format!("cote : {}", msg.into()))
}

fn polar(center: PointMm, radius: f64, angle_deg: f64) -> PointMm {
    let (s, c) = sin_cos_deg(angle_deg);
    PointMm::new(center.x + radius * c, center.y + radius * s)
}

impl Sheet {
    pub fn full_dimension(&self, id: FullDimensionId) -> Result<&FullDimension, CoreError> {
        self.full_dimensions
            .iter()
            .find(|d| d.id == id)
            .ok_or(CoreError::UnknownFullDimension(id))
    }

    /// Une cote est masquée dès que l'un des objets qu'elle cite l'est.
    pub fn full_dimension_visible(&self, d: &FullDimension) -> bool {
        d.kind.points().into_iter().all(|p| self.point_visible(p))
            && d.kind.entity().is_none_or(|e| self.entity_visible(e))
    }

    /// Une cote est verrouillée dès que l'un des objets qu'elle cite l'est.
    pub fn full_dimension_locked(&self, d: &FullDimension) -> bool {
        d.kind.points().into_iter().any(|p| self.point_locked(p))
            || d.kind.entity().is_some_and(|e| self.entity_locked(e))
    }

    /// Calque (le premier verrouillé) qui verrouille la cote, pour le message de refus.
    pub(crate) fn full_dimension_lock_layer(&self, d: &FullDimension) -> Option<LayerId> {
        d.kind
            .points()
            .into_iter()
            .find(|&p| self.point_locked(p))
            .and_then(|p| self.point(p).ok().and_then(|p| p.layer))
            .or_else(|| {
                d.kind
                    .entity()
                    .filter(|&e| self.entity_locked(e))
                    .and_then(|e| self.entity(e).ok().and_then(|e| e.layer))
            })
    }

    /// Valeur mesurée d'une cote, calculée maintenant depuis la construction (mm, ou degrés
    /// pour un angle).
    pub fn full_dimension_value(&self, id: FullDimensionId) -> Result<f64, CoreError> {
        Ok(self.full_dimension_geometry(id)?.value)
    }

    /// Géométrie d'une cote lue **maintenant** sur la construction. Erreur expliquée quand la
    /// cote est (devenue) non calculable.
    pub fn full_dimension_geometry(
        &self,
        id: FullDimensionId,
    ) -> Result<FullDimensionGeometry, CoreError> {
        let d = self.full_dimension(id)?;
        self.compute_full_dimension(d.id, d.kind, d.label.clone(), d.text_shift_mm)
    }

    /// Géométries de toutes les cotes calculables (les autres sont ignorées).
    pub fn all_full_dimension_geometries(&self) -> Vec<FullDimensionGeometry> {
        self.full_dimensions
            .iter()
            .filter_map(|d| self.full_dimension_geometry(d.id).ok())
            .collect()
    }

    /// Direction par défaut du trait de rayon ou de diamètre d'un tracé : milieu de l'arc,
    /// 45° pour un cercle.
    pub fn default_leader_angle_deg(&self, entity: EntityId) -> Result<f64, CoreError> {
        match self.shape(entity)? {
            Shape::Arc(a) => {
                let mid = a.start_rad + a.sweep_rad / 2.0;
                Ok(mid.to_degrees().rem_euclid(360.0))
            }
            Shape::Circle(_) => Ok(45.0),
            _ => Err(self.no_radius(entity)),
        }
    }

    fn no_radius(&self, entity: EntityId) -> CoreError {
        bad(format!(
            "{} n'a pas de rayon (seuls un arc et un cercle en ont un)",
            self.entity_name(entity)
        ))
    }

    /// Centre et rayon d'un arc ou d'un cercle.
    fn radius_target(&self, entity: EntityId) -> Result<(PointMm, f64), CoreError> {
        match self.shape(entity)? {
            Shape::Arc(a) => Ok((a.center, a.radius_mm)),
            Shape::Circle(c) => Ok((c.circle.center, c.circle.radius_mm)),
            _ => Err(self.no_radius(entity)),
        }
    }

    /// Calcule la géométrie d'une cote (existante ou en projet : même chemin de calcul pour
    /// les refus à la création et pour la lecture).
    pub(crate) fn compute_full_dimension(
        &self,
        id: FullDimensionId,
        kind: FullDimensionKind,
        label: Option<String>,
        text_shift_mm: f64,
    ) -> Result<FullDimensionGeometry, CoreError> {
        let tol = DEFAULT_LENGTH_TOLERANCE_MM;
        let mut extension_lines = Vec::new();
        let (value, value_text, line, base, text_dir_deg) = match kind {
            FullDimensionKind::PathLength { entity, offset_mm } => {
                if matches!(self.entity(entity)?.kind, EntityKind::Circle { .. }) {
                    return Err(bad(
                        "un cercle n'a pas d'extrémités : utilisez le rayon ou le diamètre",
                    ));
                }
                let m = measure::entity_length(self, entity)?;
                let (p0, p1) = self.entity_points(entity)?;
                let (a, b) = (self.position(p0)?, self.position(p1)?);
                if pf_geometry::distance_mm(a, b) <= tol {
                    return Err(bad(
                        "les extrémités du tracé sont confondues : la ligne de cote n'a pas de \
                         direction",
                    ));
                }
                let u = (b - a).unit()?;
                let n = u.rotated_left() * offset_mm;
                let (pa, pb) = (a + n, b + n);
                if offset_mm.abs() > pf_geometry::EPSILON_MM {
                    extension_lines.push((a, pa));
                    extension_lines.push((b, pb));
                }
                let prefix = if matches!(self.entity(entity)?.kind, EntityKind::Segment { .. }) {
                    ""
                } else {
                    "dév. "
                };
                let dir = u.y.atan2(u.x).to_degrees();
                (
                    m.value,
                    format!("{prefix}{:.2} mm", m.value),
                    DimensionLine::Straight {
                        a: pa,
                        b: pb,
                        arrow_a: true,
                        arrow_b: true,
                    },
                    pf_geometry::midpoint(pa, pb),
                    dir,
                )
            }
            FullDimensionKind::Angle {
                vertex,
                first,
                second,
                radius_mm,
            } => {
                let (v, p1, p2) = (
                    self.position(vertex)?,
                    self.position(first)?,
                    self.position(second)?,
                );
                let (u1, u2) = (p1 - v, p2 - v);
                if u1.norm() <= tol || u2.norm() <= tol {
                    return Err(bad("un côté de l'angle est de longueur nulle"));
                }
                let sweep = ccw_angle_deg(u1, u2)?;
                if !(SUPERPOSED_RAYS_DEG..=360.0 - SUPERPOSED_RAYS_DEG).contains(&sweep) {
                    return Err(bad(
                        "les deux côtés de l'angle sont superposés : l'angle (0° ou 360°) est \
                         indéfini",
                    ));
                }
                let start = u1.y.atan2(u1.x).to_degrees().rem_euclid(360.0);
                for (p, u) in [(p1, u1), (p2, u2)] {
                    if u.norm() < radius_mm - pf_geometry::EPSILON_MM {
                        let dir = u.unit()?;
                        extension_lines.push((p, v + dir * radius_mm));
                    }
                }
                let mid = start + sweep / 2.0;
                (
                    sweep,
                    format!("{sweep:.2}°"),
                    DimensionLine::Arc {
                        center: v,
                        radius_mm,
                        start_deg: start,
                        sweep_deg: sweep,
                    },
                    polar(v, radius_mm, mid),
                    mid + 90.0,
                )
            }
            FullDimensionKind::Radius { entity, angle_deg } => {
                let (c, r) = self.radius_target(entity)?;
                let tip = polar(c, r, angle_deg);
                (
                    r,
                    format!("R {r:.2} mm"),
                    DimensionLine::Straight {
                        a: c,
                        b: tip,
                        arrow_a: false,
                        arrow_b: true,
                    },
                    pf_geometry::midpoint(c, tip),
                    angle_deg,
                )
            }
            FullDimensionKind::Diameter { entity, angle_deg } => {
                let (c, r) = self.radius_target(entity)?;
                (
                    2.0 * r,
                    format!("Ø {:.2} mm", 2.0 * r),
                    DimensionLine::Straight {
                        a: polar(c, r, angle_deg + 180.0),
                        b: polar(c, r, angle_deg),
                        arrow_a: true,
                        arrow_b: true,
                    },
                    c,
                    angle_deg,
                )
            }
            FullDimensionKind::Horizontal {
                from,
                to,
                offset_mm,
            } => {
                let (a, b) = (self.position(from)?, self.position(to)?);
                let d = b.x - a.x;
                if d.abs() <= tol {
                    return Err(bad(
                        "les deux points ont la même abscisse : la cote horizontale est nulle",
                    ));
                }
                let y = a.y + offset_mm;
                let (pa, pb) = (PointMm::new(a.x, y), PointMm::new(b.x, y));
                if offset_mm.abs() > pf_geometry::EPSILON_MM {
                    extension_lines.push((a, pa));
                }
                if (b.y - y).abs() > pf_geometry::EPSILON_MM {
                    extension_lines.push((b, pb));
                }
                (
                    d,
                    format!("{:.2} mm", d.abs()),
                    DimensionLine::Straight {
                        a: pa,
                        b: pb,
                        arrow_a: true,
                        arrow_b: true,
                    },
                    pf_geometry::midpoint(pa, pb),
                    0.0,
                )
            }
            FullDimensionKind::Vertical {
                from,
                to,
                offset_mm,
            } => {
                let (a, b) = (self.position(from)?, self.position(to)?);
                let d = b.y - a.y;
                if d.abs() <= tol {
                    return Err(bad(
                        "les deux points ont la même ordonnée : la cote verticale est nulle",
                    ));
                }
                let x = a.x + offset_mm;
                let (pa, pb) = (PointMm::new(x, a.y), PointMm::new(x, b.y));
                if offset_mm.abs() > pf_geometry::EPSILON_MM {
                    extension_lines.push((a, pa));
                }
                if (b.x - x).abs() > pf_geometry::EPSILON_MM {
                    extension_lines.push((b, pb));
                }
                (
                    d,
                    format!("{:.2} mm", d.abs()),
                    DimensionLine::Straight {
                        a: pa,
                        b: pb,
                        arrow_a: true,
                        arrow_b: true,
                    },
                    pf_geometry::midpoint(pa, pb),
                    90.0,
                )
            }
        };
        let text_angle_deg = readable_text_angle_deg(text_dir_deg);
        let (s, c) = sin_cos_deg(text_angle_deg);
        let text_anchor = PointMm::new(base.x + text_shift_mm * c, base.y + text_shift_mm * s);
        Ok(FullDimensionGeometry {
            id,
            kind,
            value,
            extension_lines,
            line,
            text_anchor,
            text_angle_deg,
            label,
            value_text,
        })
    }

    /// Refus à la création : la cote doit être définie **maintenant** (cible du bon type,
    /// géométrie non dégénérée) et ses objets doivent exister dans cette feuille.
    pub(crate) fn check_new_full_dimension(
        &self,
        kind: FullDimensionKind,
    ) -> Result<(), CoreError> {
        kind.validate_static()?;
        self.check_full_dimension_refs(&kind)?;
        self.compute_full_dimension(FullDimensionId(0), kind, None, 0.0)
            .map(|_| ())
    }

    fn check_full_dimension_refs(&self, kind: &FullDimensionKind) -> Result<(), CoreError> {
        for p in kind.points() {
            self.point(p)?;
        }
        if let Some(e) = kind.entity() {
            self.entity(e)?;
        }
        Ok(())
    }

    /// Validation d'une feuille : références existantes, valeurs normalisées, légendes,
    /// plafond. Géométrie non vérifiée (voir l'en-tête du module : « non calculable »).
    pub(crate) fn validate_full_dimensions(&self) -> Result<(), CoreError> {
        if self.full_dimensions.len() > MAX_FULL_DIMENSIONS_PER_SHEET {
            return Err(CoreError::Invalid(format!(
                "la feuille « {} » porte plus de {MAX_FULL_DIMENSIONS_PER_SHEET} cotes",
                self.name
            )));
        }
        for d in &self.full_dimensions {
            d.kind.validate_static()?;
            self.check_full_dimension_refs(&d.kind)?;
            if let Some(l) = &d.label {
                if normalize_label(Some(l))?.as_deref() != Some(l.as_str()) {
                    return Err(CoreError::Invalid(
                        "cote : légende vide ou bordée d'espaces".into(),
                    ));
                }
            }
            check_text_shift(d.text_shift_mm)?;
        }
        Ok(())
    }

    /// Vrai si tous les objets cités par la cote existent encore.
    fn full_dimension_alive(&self, d: &FullDimension) -> bool {
        d.kind.points().into_iter().all(|p| self.point(p).is_ok())
            && d.kind.entity().is_none_or(|e| self.entity(e).is_ok())
    }
}

impl Project {
    /// Retire, avec remarque, les cotes dont une cible a disparu (jamais de référence
    /// morte) ; appelée par `Project::apply`. Sans effet quand aucune feuille n'a de cote.
    pub(crate) fn prune_dead_full_dimensions(&mut self, out: &mut Outcome) {
        if self.sheets.iter().all(|s| s.full_dimensions.is_empty()) {
            return;
        }
        for s in &mut self.sheets {
            let dead: Vec<FullDimensionId> = s
                .full_dimensions
                .iter()
                .filter(|d| !s.full_dimension_alive(d))
                .map(|d| d.id)
                .collect();
            if dead.is_empty() {
                continue;
            }
            s.full_dimensions.retain(|d| !dead.contains(&d.id));
            for _ in &dead {
                out.notes.push(format!(
                    "Une cote de la feuille « {} » mesurait un objet supprimé : elle est \
                     retirée.",
                    s.name
                ));
            }
        }
    }

    pub(crate) fn add_full_dimension(
        &mut self,
        sheet: SheetId,
        kind: FullDimensionKind,
        label: Option<&str>,
        text_shift_mm: f64,
        out: &mut Outcome,
    ) -> Result<(), CoreError> {
        let label = normalize_label(label)?;
        let text_shift_mm = check_text_shift(text_shift_mm)?;
        // La mise en page est ramenée à sa forme normale avant tout contrôle.
        let kind = kind.with_placement(kind.placement())?;
        let s = self.sheet(sheet)?;
        if s.full_dimensions.len() >= MAX_FULL_DIMENSIONS_PER_SHEET {
            return Err(CoreError::Invalid(format!(
                "cote : au plus {MAX_FULL_DIMENSIONS_PER_SHEET} cotes par feuille"
            )));
        }
        s.check_new_full_dimension(kind)?;
        let probe = FullDimension {
            id: FullDimensionId(0),
            kind,
            label: None,
            text_shift_mm: 0.0,
        };
        if s.full_dimension_locked(&probe) {
            return Err(CoreError::LayerLocked(format!(
                "cote : un objet mesuré est dans le calque « {} », verrouillé (déverrouillez-le \
                 d'abord)",
                s.layer_name(s.full_dimension_lock_layer(&probe))
            )));
        }
        let id = FullDimensionId(self.alloc_id());
        self.sheet_mut(sheet)?.full_dimensions.push(FullDimension {
            id,
            kind,
            label,
            text_shift_mm,
        });
        out.full_dimensions.push(id);
        Ok(())
    }

    fn full_dimension_mut(
        &mut self,
        sheet: SheetId,
        id: FullDimensionId,
    ) -> Result<&mut FullDimension, CoreError> {
        self.sheet_mut(sheet)?
            .full_dimensions
            .iter_mut()
            .find(|d| d.id == id)
            .ok_or(CoreError::UnknownFullDimension(id))
    }

    pub(crate) fn set_full_dimension_placement(
        &mut self,
        sheet: SheetId,
        id: FullDimensionId,
        placement: f64,
    ) -> Result<(), CoreError> {
        let d = self.full_dimension_mut(sheet, id)?;
        d.kind = d.kind.with_placement(placement)?;
        Ok(())
    }

    pub(crate) fn set_full_dimension_label(
        &mut self,
        sheet: SheetId,
        id: FullDimensionId,
        label: Option<&str>,
    ) -> Result<(), CoreError> {
        let label = normalize_label(label)?;
        self.full_dimension_mut(sheet, id)?.label = label;
        Ok(())
    }

    pub(crate) fn set_full_dimension_text_shift(
        &mut self,
        sheet: SheetId,
        id: FullDimensionId,
        shift_mm: f64,
    ) -> Result<(), CoreError> {
        let shift = check_text_shift(shift_mm)?;
        self.full_dimension_mut(sheet, id)?.text_shift_mm = shift;
        Ok(())
    }

    pub(crate) fn delete_full_dimension(
        &mut self,
        sheet: SheetId,
        id: FullDimensionId,
    ) -> Result<(), CoreError> {
        let s = self.sheet_mut(sheet)?;
        if !s.full_dimensions.iter().any(|d| d.id == id) {
            return Err(CoreError::UnknownFullDimension(id));
        }
        s.full_dimensions.retain(|d| d.id != id);
        Ok(())
    }
}
