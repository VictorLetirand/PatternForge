//! Droit-fil avancé (US-06.5, décisions 186 à 194).
//!
//! Le droit-fil d'une pièce reste un **tracé** de la feuille (`Piece.grainline`, rôle
//! « droit-fil ») : sa longueur et sa position (le milieu du tracé) sont celles du tracé
//! dessiné, comme avant. `Piece.grain` (facultatif, omis du fichier quand il n'apporte rien)
//! ajoute :
//!
//! - la **direction** de la flèche : celle du tracé dessiné (`Drawn`, comportement
//!   historique), **parallèle** ou **perpendiculaire** à un autre tracé de la feuille, ou
//!   **biais à ±45°** ; elle est **calculée à la lecture** (elle suit le tracé de référence
//!   s'il bouge), jamais stockée ;
//! - le **sens** du tissu (`GrainSense` : aucun, poil, motif) ;
//! - une **inversion** de la flèche (`reversed`) ;
//! - la **tolérance de rotation** au placement (degrés, [0 ; 180]) ;
//! - jusqu'à [`GRAIN_MAX_EXTRA_LINES`] **lignes supplémentaires** (rayures, raccords), chacune
//!   avec son tracé, sa direction, son sens et son inversion.
//!
//! Conventions (à faire relire par une modéliste) :
//!
//! - Axe de référence du biais : l'axe X de la feuille (Y vers le haut). `+45°` = direction
//!   `(cos 45°, sin 45°)`, `−45°` = `(cos 45°, −sin 45°)` ; comme axes non orientés, ces deux
//!   directions sont les mêmes que l'axe de référence soit X ou Y.
//! - Direction d'un tracé de référence : la **corde** (du premier au dernier point du tracé),
//!   pour un segment comme pour une courbe de Bézier, un arc ou une courbe lisse ; un cercle
//!   ou un tracé dont les extrémités sont confondues n'a pas de direction (refus).
//!   Perpendiculaire = parallèle tournée de +90° (anti-horaire).
//! - Flèche : même longueur et même milieu que le tracé dessiné ; elle va de `from` à `to`.
//!   `reversed` l'inverse (utile pour le poil et le motif, qui ont un sens).
//! - Tolérance : une rotation `θ` du placement (US-04.5) fait dévier le droit-fil de `θ` ramené
//!   dans `[0 ; 180]` (sens orienté : poil ou motif) ou dans `[0 ; 90]` (aucun sens : le
//!   droit-fil est un axe, tourner de 180° le laisse parallèle). Au-delà de la tolérance
//!   (absente = 0), la pièce porte un avertissement : jamais un refus.

use crate::command::{Outcome, MERGE_TOLERANCE_MM};
use crate::error::CoreError;
use crate::model::*;
use crate::piece_plan::normalize_rotation_deg;
use pf_geometry::{PointMm, VectorMm};
use serde::{Deserialize, Serialize};
use std::f64::consts::FRAC_1_SQRT_2;

/// Nombre maximal de lignes supplémentaires par pièce (en plus de la ligne principale).
pub const GRAIN_MAX_EXTRA_LINES: usize = 8;
/// Plus grande tolérance de rotation acceptée (degrés).
pub const GRAIN_MAX_ROTATION_TOLERANCE_DEG: f64 = 180.0;
/// Écart (degrés) en dessous duquel une déviation égale à la tolérance n'est pas dépassée
/// (absorbe l'arrondi des flottants ; une rotation de 30,000001° pour 30° est signalée).
pub const GRAIN_DEVIATION_SLACK_DEG: f64 = 1e-9;

/// Direction du droit-fil. Variantes **à champs** (jamais d'unité : défaut D-04).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum GrainDirection {
    /// Direction du tracé dessiné (comportement historique).
    Drawn {},
    /// Parallèle à la corde d'un autre tracé de la feuille.
    Parallel { reference: EntityId },
    /// Perpendiculaire (+90°) à la corde d'un autre tracé de la feuille.
    Perpendicular { reference: EntityId },
    /// Biais : `angle_deg` vaut exactement 45 ou −45 depuis l'axe X de la feuille.
    Bias { angle_deg: f64 },
}

impl Default for GrainDirection {
    fn default() -> Self {
        GrainDirection::Drawn {}
    }
}

impl GrainDirection {
    pub fn is_drawn(&self) -> bool {
        matches!(self, GrainDirection::Drawn {})
    }

    /// Identifiant stable, indépendant de la langue.
    pub fn key(&self) -> &'static str {
        match self {
            GrainDirection::Drawn {} => "drawn",
            GrainDirection::Parallel { .. } => "parallel",
            GrainDirection::Perpendicular { .. } => "perpendicular",
            GrainDirection::Bias { .. } => "bias",
        }
    }

    /// Tracé de référence, s'il y en a un.
    pub fn reference(&self) -> Option<EntityId> {
        match *self {
            GrainDirection::Parallel { reference }
            | GrainDirection::Perpendicular { reference } => Some(reference),
            _ => None,
        }
    }

    fn check_shape(&self) -> Result<(), CoreError> {
        if let GrainDirection::Bias { angle_deg } = *self {
            if !(angle_deg == 45.0 || angle_deg == -45.0) {
                return Err(CoreError::Invalid(
                    "le biais du droit-fil vaut exactement 45° ou −45° (axe X de la feuille)"
                        .into(),
                ));
            }
        }
        Ok(())
    }
}

/// Sens du tissu marqué par une ligne de droit-fil.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GrainSense {
    /// Aucun sens : le droit-fil est un axe (flèche à deux pointes, comme avant).
    #[default]
    #[serde(rename = "none")]
    Unmarked,
    /// Sens du poil (velours, fourrure) : flèche unidirectionnelle.
    Nap,
    /// Sens du motif : flèche à deux pointes doublées.
    Pattern,
}

impl GrainSense {
    pub const ALL: [GrainSense; 3] = [GrainSense::Unmarked, GrainSense::Nap, GrainSense::Pattern];

    pub fn is_unmarked(&self) -> bool {
        *self == GrainSense::Unmarked
    }

    /// Identifiant stable, identique à la valeur écrite dans le fichier.
    pub fn key(self) -> &'static str {
        match self {
            GrainSense::Unmarked => "none",
            GrainSense::Nap => "nap",
            GrainSense::Pattern => "pattern",
        }
    }
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// Réglages d'une ligne (direction, sens, inversion), sans son tracé.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct GrainLineSpec {
    pub direction: GrainDirection,
    pub sense: GrainSense,
    pub reversed: bool,
}

/// Ligne de droit-fil supplémentaire (rayure, raccord).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtraGrainLine {
    /// Tracé de la feuille de la pièce (segment de rôle « droit-fil »).
    pub entity: EntityId,
    #[serde(default, skip_serializing_if = "GrainDirection::is_drawn")]
    pub direction: GrainDirection,
    #[serde(default, skip_serializing_if = "GrainSense::is_unmarked")]
    pub sense: GrainSense,
    #[serde(default, skip_serializing_if = "is_false")]
    pub reversed: bool,
}

impl ExtraGrainLine {
    pub fn spec(&self) -> GrainLineSpec {
        GrainLineSpec {
            direction: self.direction,
            sense: self.sense,
            reversed: self.reversed,
        }
    }
}

/// Réglages avancés du droit-fil d'une pièce (US-06.5) : ceux de la ligne principale
/// (`Piece.grainline`), la tolérance de rotation et les lignes supplémentaires. Absent du
/// fichier quand tout est par défaut.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PieceGrain {
    #[serde(default, skip_serializing_if = "GrainDirection::is_drawn")]
    pub direction: GrainDirection,
    #[serde(default, skip_serializing_if = "GrainSense::is_unmarked")]
    pub sense: GrainSense,
    #[serde(default, skip_serializing_if = "is_false")]
    pub reversed: bool,
    /// Rotation libre acceptée au placement, en degrés [0 ; 180] ; absente = 0.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotation_tolerance_deg: Option<f64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extra: Vec<ExtraGrainLine>,
}

impl PieceGrain {
    /// Rien à enregistrer : tous les réglages sont ceux par défaut.
    pub fn is_default(&self) -> bool {
        self.direction.is_drawn()
            && self.sense.is_unmarked()
            && !self.reversed
            && self.rotation_tolerance_deg.is_none()
            && self.extra.is_empty()
    }

    /// Cette ligne fait-elle partie des lignes supplémentaires ?
    pub(crate) fn has_line(&self, entity: EntityId) -> bool {
        self.extra.iter().any(|e| e.entity == entity)
    }

    pub fn main_spec(&self) -> GrainLineSpec {
        GrainLineSpec {
            direction: self.direction,
            sense: self.sense,
            reversed: self.reversed,
        }
    }

    /// Tolérance effective (0 quand elle est absente).
    pub fn tolerance_deg(&self) -> f64 {
        self.rotation_tolerance_deg.unwrap_or(0.0)
    }

    /// Valeurs finies et bornées, forme canonique (tolérance nulle = absente, zéro négatif
    /// écrasé). Les contrôles qui demandent le projet sont faits par `check_grain`.
    pub fn normalized(mut self) -> Result<PieceGrain, CoreError> {
        if let Some(t) = self.rotation_tolerance_deg {
            if !t.is_finite() || !(0.0..=GRAIN_MAX_ROTATION_TOLERANCE_DEG).contains(&t) {
                return Err(CoreError::Invalid(format!(
                    "la tolérance de rotation du droit-fil doit être un nombre fini compris \
                     entre 0 et {GRAIN_MAX_ROTATION_TOLERANCE_DEG} °"
                )));
            }
            self.rotation_tolerance_deg = if t == 0.0 { None } else { Some(t) };
        }
        if self.extra.len() > GRAIN_MAX_EXTRA_LINES {
            return Err(CoreError::Invalid(format!(
                "une pièce porte au plus {GRAIN_MAX_EXTRA_LINES} lignes de droit-fil \
                 supplémentaires (en plus de la principale)"
            )));
        }
        self.direction.check_shape()?;
        for e in &self.extra {
            e.direction.check_shape()?;
        }
        Ok(self)
    }

    /// Forme stockée : refus si ce n'est pas la forme canonique de `normalized`.
    fn validate_stored(&self) -> Result<(), CoreError> {
        let n = self.clone().normalized()?;
        if n != *self || self.is_default() {
            return Err(CoreError::Invalid(
                "réglages de droit-fil non canoniques (valeurs par défaut écrites, tolérance \
                 nulle ou zéro négatif) : omettez-les"
                    .into(),
            ));
        }
        Ok(())
    }
}

/// Une ligne de droit-fil résolue : la flèche à dessiner, calculée d'après la construction.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResolvedGrainLine {
    /// Tracé de la ligne (rôle « droit-fil »).
    pub entity: EntityId,
    /// Ligne principale (`Piece.grainline`) ou supplémentaire.
    pub main: bool,
    pub spec: GrainLineSpec,
    /// Début de la flèche (mm).
    pub from: PointMm,
    /// Fin de la flèche (mm), dans le sens du droit-fil.
    pub to: PointMm,
    /// Angle de `from` vers `to` depuis l'axe X, dans `[0 ; 360[` (degrés).
    pub direction_deg: f64,
    /// Longueur de la flèche = celle du tracé dessiné (mm).
    pub length_mm: f64,
}

/// Déviation du droit-fil causée par la rotation `rotation_deg` du placement. `oriented` :
/// la ligne a un sens (poil, motif) ; sinon c'est un axe, la déviation est dans `[0 ; 90]`.
pub fn grain_rotation_deviation_deg(rotation_deg: f64, oriented: bool) -> f64 {
    let t = normalize_rotation_deg(rotation_deg);
    if oriented {
        if t > 180.0 {
            360.0 - t
        } else {
            t
        }
    } else {
        let r = t % 180.0;
        if r > 90.0 {
            180.0 - r
        } else {
            r
        }
    }
}

/// Contrôle de la rotation d'une pièce dans le plan (US-06.5), observé à la lecture.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GrainRotation {
    /// Rotation du placement, dans `[0 ; 360[`.
    pub rotation_deg: f64,
    /// Pire déviation parmi les lignes de droit-fil (degrés).
    pub deviation_deg: f64,
    /// Tolérance de la pièce (0 si absente).
    pub tolerance_deg: f64,
}

impl GrainRotation {
    /// Déviation strictement au-dessus de la tolérance (une déviation égale est acceptée).
    pub fn exceeded(&self) -> bool {
        self.deviation_deg > self.tolerance_deg + GRAIN_DEVIATION_SLACK_DEG
    }
}

/// Feuille qui porte ce tracé, pour une pièce de ce lien.
fn sheet_of(project: &Project, link: PieceLink, entity: EntityId) -> Option<&Sheet> {
    match link {
        PieceLink::Linked { sheet } => project
            .sheet(sheet)
            .ok()
            .filter(|s| s.entity(entity).is_ok()),
        PieceLink::Detached => project.sheets.iter().find(|s| s.entity(entity).is_ok()),
    }
}

/// Corde d'un tracé (premier et dernier point) ; `None` si illisible ou de longueur nulle
/// (cercle, courbe fermée, tracé dégénéré).
fn chord(sheet: &Sheet, entity: EntityId) -> Option<(PointMm, PointMm)> {
    let (p, q) = sheet.entity_points(entity).ok()?;
    let (a, b) = (sheet.position(p).ok()?, sheet.position(q).ok()?);
    let n = (b - a).norm();
    (n.is_finite() && n > MERGE_TOLERANCE_MM).then_some((a, b))
}

fn unit_chord(sheet: &Sheet, entity: EntityId) -> Option<VectorMm> {
    let (a, b) = chord(sheet, entity)?;
    (b - a).unit().ok()
}

/// Résout une ligne : `None` si le tracé ou sa référence est illisible ou dégénéré.
fn resolve_line(
    sheet: &Sheet,
    entity: EntityId,
    spec: GrainLineSpec,
    main: bool,
) -> Option<ResolvedGrainLine> {
    let (a, b) = chord(sheet, entity)?;
    let length = (b - a).norm();
    let u = match spec.direction {
        GrainDirection::Drawn {} => (b - a).unit().ok()?,
        GrainDirection::Parallel { reference } => unit_chord(sheet, reference)?,
        GrainDirection::Perpendicular { reference } => unit_chord(sheet, reference)?.rotated_left(),
        GrainDirection::Bias { angle_deg } => VectorMm::new(
            FRAC_1_SQRT_2,
            if angle_deg < 0.0 {
                -FRAC_1_SQRT_2
            } else {
                FRAC_1_SQRT_2
            },
        ),
    };
    let u = if spec.reversed {
        VectorMm::new(-u.x, -u.y)
    } else {
        u
    };
    // Direction du tracé dessiné : les extrémités gardent les coordonnées du tracé (aucun
    // arrondi), seule l'inversion échange les bouts.
    let (from, to) = if spec.direction.is_drawn() {
        if spec.reversed {
            (b, a)
        } else {
            (a, b)
        }
    } else {
        let mid = a.lerp(b, 0.5);
        let half = u * (length / 2.0);
        (
            PointMm::new(mid.x - half.x, mid.y - half.y),
            PointMm::new(mid.x + half.x, mid.y + half.y),
        )
    };
    if ![from.x, from.y, to.x, to.y].iter().all(|v| v.is_finite()) {
        return None;
    }
    let deg = u.y.atan2(u.x).to_degrees();
    Some(ResolvedGrainLine {
        entity,
        main,
        spec,
        from,
        to,
        direction_deg: normalize_rotation_deg(deg),
        length_mm: length,
    })
}

impl Project {
    /// Lignes de droit-fil d'une pièce (principale d'abord), avec la flèche calculée d'après
    /// la construction actuelle. Lecture pure ; vide pour une pièce inconnue ou sans droit-fil.
    /// Une ligne illisible est omise (impossible dans un projet valide qui porte `grain`).
    pub fn piece_grain_lines(&self, piece: PieceId) -> Vec<ResolvedGrainLine> {
        let Ok(p) = self.piece(piece) else {
            return Vec::new();
        };
        let Some(main) = p.grainline else {
            return Vec::new();
        };
        let Some(sheet) = sheet_of(self, p.link, main) else {
            return Vec::new();
        };
        let default = PieceGrain::default();
        let g = p.grain.as_ref().unwrap_or(&default);
        let mut lines = Vec::new();
        lines.extend(resolve_line(sheet, main, g.main_spec(), true));
        for e in &g.extra {
            lines.extend(resolve_line(sheet, e.entity, e.spec(), false));
        }
        lines
    }

    /// Lignes de droit-fil des pièces **qui portent des réglages avancés** et dont le contour
    /// est sur cette feuille : sert à l'export. Une pièce sans `grain` n'y figure jamais (son
    /// droit-fil s'exporte comme avant).
    pub fn sheet_grain_lines(&self, sheet: SheetId) -> Vec<ResolvedGrainLine> {
        self.pieces_on(sheet)
            .into_iter()
            .filter(|p| p.grain.is_some())
            .flat_map(|p| self.piece_grain_lines(p.id))
            .collect()
    }

    /// Rotation du placement manuel de la pièce comparée à la tolérance de son droit-fil
    /// (US-06.5). `None` : pas de placement manuel, pas de droit-fil lisible. Le placement
    /// automatique n'est jamais tourné.
    pub fn piece_grain_rotation(&self, piece: PieceId) -> Option<GrainRotation> {
        let p = self.piece(piece).ok()?;
        let placement = p.placement?;
        let lines = self.piece_grain_lines(piece);
        if lines.is_empty() {
            return None;
        }
        let rotation = normalize_rotation_deg(placement.rotation_deg);
        let deviation = lines
            .iter()
            .map(|l| grain_rotation_deviation_deg(rotation, !l.spec.sense.is_unmarked()))
            .fold(0.0_f64, f64::max);
        Some(GrainRotation {
            rotation_deg: rotation,
            deviation_deg: deviation,
            tolerance_deg: p.grain.as_ref().map_or(0.0, PieceGrain::tolerance_deg),
        })
    }

    /// Contrôle des réglages pour une pièce : ligne principale, lignes supplémentaires,
    /// références. Refus expliqué.
    pub(crate) fn check_grain(
        &self,
        me: PieceId,
        piece_name: &str,
        link: PieceLink,
        main: Option<EntityId>,
        grain: &PieceGrain,
    ) -> Result<(), CoreError> {
        let Some(main) = main else {
            return Err(CoreError::Invalid(format!(
                "la pièce « {piece_name} » n'a pas de droit-fil : assignez-en un avant de régler \
                 sa direction, son sens ou ses lignes"
            )));
        };
        let Some(sheet) = sheet_of(self, link, main) else {
            return Err(CoreError::DeadReference(format!(
                "le droit-fil de la pièce « {piece_name} » référence un tracé absent (id {})",
                main.0
            )));
        };
        let mut own = vec![main];
        for e in &grain.extra {
            if own.contains(&e.entity) {
                return Err(CoreError::Invalid(format!(
                    "la pièce « {piece_name} » utilise deux fois le tracé {} comme ligne de \
                     droit-fil",
                    e.entity.0
                )));
            }
            own.push(e.entity);
        }
        // Une ligne réglée n'est à aucune autre pièce : sinon l'export de la flèche serait ambigu.
        for o in self.pieces.iter().filter(|o| o.id != me) {
            let shared = own.iter().find(|e| {
                o.grainline == Some(**e) || o.grain.as_ref().is_some_and(|g| g.has_line(**e))
            });
            if let Some(e) = shared {
                return Err(CoreError::Invalid(format!(
                    "la ligne de droit-fil {} de la pièce « {piece_name} » est déjà le droit-fil                      de la pièce « {} » : une ligne réglée n'appartient qu'à une pièce",
                    e.0, o.name
                )));
            }
        }
        let lines = std::iter::once((main, grain.main_spec()))
            .chain(grain.extra.iter().map(|e| (e.entity, e.spec())));
        for (entity, spec) in lines {
            let Ok(ent) = sheet.entity(entity) else {
                return Err(if self.sheets.iter().any(|s| s.entity(entity).is_ok()) {
                    CoreError::Invalid(format!(
                        "la ligne de droit-fil {} n'est pas sur la feuille de la pièce « \
                         {piece_name} »",
                        entity.0
                    ))
                } else {
                    CoreError::DeadReference(format!(
                        "une ligne de droit-fil de la pièce « {piece_name} » référence un tracé \
                         absent (id {})",
                        entity.0
                    ))
                });
            };
            if ent.role != PathRole::Grainline {
                return Err(CoreError::Invalid(format!(
                    "la ligne {} de la pièce « {piece_name} » doit avoir le rôle droit-fil",
                    entity.0
                )));
            }
            if !matches!(sheet.shape(entity), Ok(Shape::Line(..))) {
                return Err(CoreError::Invalid(format!(
                    "la ligne {} de la pièce « {piece_name} » doit être un segment",
                    entity.0
                )));
            }
            if chord(sheet, entity).is_none() {
                return Err(CoreError::Invalid(format!(
                    "la ligne {} de la pièce « {piece_name} » est de longueur nulle",
                    entity.0
                )));
            }
            spec.direction.check_shape()?;
            if let Some(r) = spec.direction.reference() {
                if sheet.entity(r).is_err() {
                    return Err(if self.sheets.iter().any(|s| s.entity(r).is_ok()) {
                        CoreError::Invalid(format!(
                            "le tracé de référence {} est sur une autre feuille que la pièce « \
                             {piece_name} »",
                            r.0
                        ))
                    } else {
                        CoreError::DeadReference(format!(
                            "le tracé de référence {} du droit-fil de la pièce « {piece_name} » \
                             est absent",
                            r.0
                        ))
                    });
                }
                if own.contains(&r) {
                    return Err(CoreError::Invalid(format!(
                        "le tracé de référence {} est une ligne de droit-fil de la pièce « \
                         {piece_name} » : choisissez un autre tracé",
                        r.0
                    )));
                }
                if chord(sheet, r).is_none() {
                    return Err(CoreError::Invalid(format!(
                        "le tracé de référence {} (pièce « {piece_name} ») n'a pas de direction : \
                         cercle ou tracé fermé, ou de longueur nulle",
                        r.0
                    )));
                }
            }
        }
        Ok(())
    }

    /// Validation d'une pièce chargée ou modifiée.
    pub(crate) fn validate_grain(&self, piece: &Piece) -> Result<(), CoreError> {
        let Some(g) = &piece.grain else {
            return Ok(());
        };
        g.validate_stored().map_err(|e| {
            CoreError::Invalid(format!("pièce « {} » : droit-fil : {e}", piece.name))
        })?;
        self.check_grain(piece.id, &piece.name, piece.link, piece.grainline, g)
    }

    /// Remplace les réglages avancés du droit-fil (`SetPieceGrain`). Tout par défaut =
    /// aucun réglage stocké.
    pub(crate) fn set_piece_grain(
        &mut self,
        piece: PieceId,
        grain: PieceGrain,
    ) -> Result<(), CoreError> {
        let p = self.piece(piece)?;
        let (name, link, main) = (p.name.clone(), p.link, p.grainline);
        let grain = grain.normalized()?;
        if grain.is_default() {
            self.piece_mut(piece)?.grain = None;
            return Ok(());
        }
        self.check_grain(piece, &name, link, main, &grain)?;
        self.piece_mut(piece)?.grain = Some(grain);
        Ok(())
    }

    /// Après une commande : ce qui n'a plus de sens est retiré **avec une remarque** (jamais
    /// une référence morte silencieuse). Ne touche que les réglages qui existaient, identiques,
    /// avant la commande : une référence morte introduite par la commande elle-même reste
    /// refusée par la validation.
    ///
    /// - plus de droit-fil principal : tous les réglages sont retirés ;
    /// - ligne supplémentaire dont le tracé a disparu (ou devenu la ligne principale) : retirée ;
    /// - tracé de référence supprimé : la direction repasse à celle du tracé dessiné.
    pub(crate) fn prune_dead_grain(&mut self, before: &Project, out: &mut Outcome) {
        if self.pieces.iter().all(|p| p.grain.is_none()) {
            return;
        }
        let ids: Vec<PieceId> = self
            .pieces
            .iter()
            .filter(|p| p.grain.is_some())
            .map(|p| p.id)
            .collect();
        for id in ids {
            let Some(old) = before.pieces.iter().find(|o| o.id == id).cloned() else {
                continue;
            };
            let Ok(p) = self.piece(id) else { continue };
            let (name, link, main) = (p.name.clone(), p.link, p.grainline);
            let Some(mut g) = p.grain.clone() else {
                continue;
            };
            let old_g = old.grain.clone().unwrap_or_default();
            let mut notes = Vec::new();
            match main {
                None => {
                    if old.grain.as_ref() == Some(&g) {
                        notes.push(format!(
                            "Les réglages du droit-fil de la pièce « {name} » (direction, sens, \
                             tolérance, lignes supplémentaires) sont retirés avec son droit-fil."
                        ));
                        self.piece_mut(id).map(|p| p.grain = None).ok();
                    }
                    out.notes.extend(notes);
                    continue;
                }
                Some(main) => {
                    let alive = |e: EntityId| {
                        sheet_of(self, link, main).is_some_and(|s| s.entity(e).is_ok())
                    };
                    let before_len = g.extra.len();
                    g.extra.retain(|e| {
                        let dead =
                            (!alive(e.entity) || e.entity == main) && old_g.extra.contains(e);
                        !dead
                    });
                    for _ in g.extra.len()..before_len {
                        notes.push(format!(
                            "Une ligne de droit-fil supplémentaire de la pièce « {name} » \
                             référençait un tracé supprimé : elle est retirée."
                        ));
                    }
                    let mut detach = |dir: &mut GrainDirection, was: bool, what: &str| {
                        if let Some(r) = dir.reference() {
                            if was && !alive(r) {
                                *dir = GrainDirection::Drawn {};
                                notes.push(format!(
                                    "{what} de la pièce « {name} » suivait un tracé supprimé : \
                                     sa direction est celle du tracé dessiné."
                                ));
                            }
                        }
                    };
                    let was_main = old_g.direction == g.direction;
                    detach(&mut g.direction, was_main, "Le droit-fil");
                    for e in &mut g.extra {
                        let was = old_g.extra.contains(e);
                        detach(&mut e.direction, was, "Une ligne de droit-fil");
                    }
                }
            }
            if g.is_default() {
                self.piece_mut(id).map(|p| p.grain = None).ok();
            } else {
                self.piece_mut(id).map(|p| p.grain = Some(g)).ok();
            }
            out.notes.extend(notes);
        }
    }
}
