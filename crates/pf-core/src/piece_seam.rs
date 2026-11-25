//! Marge de couture par bord (US-05.1, décision 291).
//!
//! `Piece.seam` (facultatif, omis du fichier quand il n'apporte rien) porte la **valeur par
//! défaut** de la pièce et des **surcharges par tracé du contour** (marge au début et à la fin du
//! tracé : début ≠ fin = marge progressive). Le **contour de coupe n'est jamais stocké** : il est
//! calculé à la lecture par [`Project::piece_cut_contour`] d'après le contour de couture courant
//! (donc toujours à jour, comme `piece_grain_lines`). Un calcul impossible (contour ouvert,
//! géométrie dégénérée, marge trop grande pour la forme) est un **contrôle**
//! (`PieceIssue::CutContourFailed`), jamais le refus d'une commande de la mère.
//!
//! Conventions (à faire valider par une modéliste, voir le suivi) :
//!
//! - la marge est mesurée vers l'**extérieur** de la pièce ;
//! - « début » et « fin » d'un tracé suivent le contour **lu dans le sens anti-horaire**, quel
//!   que soit le sens enregistré ; l'inversion d'un tracé ne change donc rien ;
//! - une marge progressive n'existe que sur un **segment** (une courbe a une marge constante) ;
//! - coins (US-05.2) : par défaut **onglet** (prolongement des décalages), biseau si la pointe
//!   dépasse `max_corner_mm` (défaut : 4 × la plus grande marge adjacente). Un type de coin peut
//!   être choisi **par sommet** (`SeamAllowance.corners`) : perpendiculaire à la couture précédente
//!   / suivante, symétrique (retour de couture), tronqué, en marche, ourlet rabattu (définitions
//!   dans `pf_geometry::seam`). Le sommet est identifié par le **tracé qui commence en ce sommet**
//!   (contour lu en anti-horaire), comme « début » d'une marge par bord ; seuls les coins
//!   saillants des sommets non lisses sont acceptés (refus expliqué à la commande) ;
//! - marge au plus [`MAX_SEAM_MM`] (1000 mm), nulle permise.

use crate::command::Outcome;
use crate::error::CoreError;
use crate::model::*;
use pf_geometry::hem::{
    fold_notches, FoldNotchPlan, DEFAULT_FOLD_NOTCH_STEP_MM, MAX_FOLD_NOTCHES,
    MAX_FOLD_NOTCH_STEP_MM, MIN_FOLD_NOTCH_STEP_MM,
};
use pf_geometry::region::{trace_geometry, EdgeGeom};
use pf_geometry::seam::{
    seam_contour_closed_darts, seam_contour_with, vertex_turn, CornerKind, CornerSpec, DartClosing,
    EdgeMargin, SeamOptions, VertexTurn,
};
use pf_geometry::split::{chain_length_mm, chain_signed_area_mm2};
use pf_geometry::{PointMm, Trace};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub use pf_geometry::seam::{MAX_CORNER_MM, MAX_SEAM_MM};

/// Type de coin d'un sommet (US-05.2). L'onglet est le coin par défaut : il n'est jamais écrit.
/// Variantes à champs (D-04 : pas de variante unitaire dans un enum serde à étiquette interne).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum CornerType {
    /// Coupe perpendiculaire à la couture précédente.
    PerpendicularPrevious {},
    /// Coupe perpendiculaire à la couture suivante.
    PerpendicularNext {},
    /// Retour de couture : coupe perpendiculaire à la bissectrice.
    Symmetric {},
    /// Pointe de l'onglet coupée à `depth_mm` de la pointe.
    Truncated { depth_mm: f64 },
    /// En marche : deux paliers perpendiculaires aux coutures.
    Stepped {},
    /// Ourlet rabattu, pli sur la couture suivante.
    HemFoldNext {},
    /// Ourlet rabattu, pli sur la couture précédente.
    HemFoldPrevious {},
}

impl CornerType {
    fn kind(self) -> CornerKind {
        match self {
            CornerType::PerpendicularPrevious {} => CornerKind::PerpendicularPrevious,
            CornerType::PerpendicularNext {} => CornerKind::PerpendicularNext,
            CornerType::Symmetric {} => CornerKind::Symmetric,
            CornerType::Truncated { depth_mm } => CornerKind::Truncated { depth_mm },
            CornerType::Stepped {} => CornerKind::Stepped,
            CornerType::HemFoldNext {} => CornerKind::HemFoldNext,
            CornerType::HemFoldPrevious {} => CornerKind::HemFoldPrevious,
        }
    }
}

/// Type de coin choisi au sommet où **commence** le tracé `entity` (contour lu en anti-horaire).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CornerAllowance {
    pub entity: EntityId,
    pub corner: CornerType,
}

/// Sommet non lisse du contour de couture, où un type de coin peut être choisi.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SeamVertex {
    /// Tracé qui commence en ce sommet (identifiant stable du sommet).
    pub entity: EntityId,
    pub point: PointMm,
    /// Coin saillant (un type de coin s'y applique) ; faux = coin rentrant.
    pub convex: bool,
}

/// Surcharge de marge d'un tracé du contour : valeur au premier point du tracé (`start_mm`) et au
/// dernier (`end_mm`), dans le sens anti-horaire du contour. Différentes = marge progressive
/// (segments seulement).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EdgeAllowance {
    pub entity: EntityId,
    pub start_mm: f64,
    pub end_mm: f64,
}

/// Ourlet (US-05.3) sur un bord **droit** du contour : marge d'ourlet de `height_mm` au-delà du
/// bord, repliée vers l'intérieur (le bord de couture est la ligne de pli). Aux deux extrémités,
/// la marge adjacente est réfléchie par rapport à la ligne de pli (forme symétrisée, coins
/// saillants seulement, sauf type de coin choisi explicitement à ce sommet).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HemAllowance {
    pub entity: EntityId,
    /// Hauteur de l'ourlet (mm, 0 à 1000).
    pub height_mm: f64,
}

/// Rempli (maroquinerie, US-05.3) sur un bord : marge de coupe constante `value_mm` à rabattre.
/// Permis sur un bord **courbe** ; il y porte des crans de rempli **calculés** (jamais stockés),
/// par pas ou par nombre (défaut : un pas de 25 mm). Pas de cran sur un bord droit.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FoldAllowance {
    pub entity: EntityId,
    /// Valeur du rempli (mm, 0 à 1000).
    pub value_mm: f64,
    /// Pas maximal entre deux crans (mm, 1 à 1000) ; exclusif avec `notch_count`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notch_step_mm: Option<f64>,
    /// Nombre de crans (1 à 1000) ; exclusif avec `notch_step_mm`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notch_count: Option<u32>,
}

/// Cran de rempli **calculé** sur un bord courbe (lecture seule ; la pose dans le modèle de
/// marquage est E06).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FoldNotch {
    pub entity: EntityId,
    /// Rang (à partir de 1) le long du bord, contour lu en anti-horaire.
    pub index: usize,
    /// Longueur développée depuis le début du bord.
    pub distance_mm: f64,
    pub point: PointMm,
    /// Normale unitaire extérieure à la pièce.
    pub normal: pf_geometry::VectorMm,
}

/// Milieu d'un bord portant un rempli (PH-5c), **lu** pour écrire « Rempli 8 mm » à l'export :
/// jamais stocké. Le bord est lu en anti-horaire.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FoldLabelAnchor {
    pub entity: EntityId,
    /// Valeur du rempli (mm), telle que réglée.
    pub value_mm: f64,
    /// Milieu du bord (à mi-longueur développée).
    pub point: PointMm,
    /// Normale unitaire extérieure à la pièce en ce point.
    pub normal: pf_geometry::VectorMm,
    /// Longueur développée du bord (mm).
    pub length_mm: f64,
}

/// Marge de couture d'une pièce : valeur par défaut et surcharges par tracé. Absente du fichier
/// quand elle n'apporte rien (défaut 0 sans surcharge).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeamAllowance {
    /// Marge des tracés sans surcharge (mm, 0 à 1000).
    pub default_mm: f64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub edges: Vec<EdgeAllowance>,
    /// Ourlets (US-05.3) ; un tracé n'a qu'un réglage (surcharge, ourlet ou rempli).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hems: Vec<HemAllowance>,
    /// Rempliés (US-05.3).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub folds: Vec<FoldAllowance>,
    /// Types de coins choisis par sommet (US-05.2) ; les autres sommets sont en onglet.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub corners: Vec<CornerAllowance>,
    /// Longueur maximale (mm, 0 à 10 000) de la pointe d'un coin en onglet, du sommet à la
    /// pointe ; au-delà, le coin est coupé en biseau. Absente : 4 × la plus grande marge adjacente.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_corner_mm: Option<f64>,
}

/// Contour de coupe calculé, avec ses mesures **observées**.
#[derive(Debug, Clone, PartialEq)]
pub struct CutContour {
    /// Chaîne fermée, anti-horaire, dans l'ordre du parcours.
    pub traces: Vec<Trace>,
    pub perimeter_mm: f64,
    /// Aire en mm², toujours positive.
    pub area_mm2: f64,
    /// Portions de `traces` portées par un bord sur le pli (PH-5b) : la pliure, pas un bord de
    /// coupe. Vide sans déclaration (la coupe est alors exactement celle d'avant).
    pub fold_cuts: Vec<crate::piece_fold::FoldCut>,
}

fn invalid(msg: impl Into<String>) -> CoreError {
    CoreError::Invalid(msg.into())
}

fn check_value(v: f64) -> Result<(), CoreError> {
    if !v.is_finite() || !(0.0..=MAX_SEAM_MM).contains(&v) {
        return Err(invalid(format!(
            "une marge de couture doit être un nombre fini compris entre 0 et {MAX_SEAM_MM} mm"
        )));
    }
    Ok(())
}

impl SeamAllowance {
    /// Rien à enregistrer : défaut nul et aucune surcharge.
    pub fn is_default(&self) -> bool {
        self.default_mm == 0.0
            && self.edges.is_empty()
            && self.hems.is_empty()
            && self.folds.is_empty()
            && self.corners.is_empty()
            && self.max_corner_mm.is_none()
    }

    /// Marge `(début, fin)` d'un tracé : son ourlet, son rempli ou sa surcharge, sinon la valeur
    /// par défaut.
    pub fn margin_of(&self, entity: EntityId) -> (f64, f64) {
        if let Some(h) = self.hems.iter().find(|h| h.entity == entity) {
            return (h.height_mm, h.height_mm);
        }
        if let Some(f) = self.folds.iter().find(|f| f.entity == entity) {
            return (f.value_mm, f.value_mm);
        }
        self.edges
            .iter()
            .find(|e| e.entity == entity)
            .map_or((self.default_mm, self.default_mm), |e| {
                (e.start_mm, e.end_mm)
            })
    }

    /// Valeurs finies et bornées, pas de doublon, zéro négatif écrasé, surcharges triées par
    /// tracé (forme canonique de la commande).
    pub fn normalized(mut self) -> Result<SeamAllowance, CoreError> {
        check_value(self.default_mm)?;
        self.default_mm += 0.0;
        let mut seen = HashSet::new();
        for e in &mut self.edges {
            check_value(e.start_mm)?;
            check_value(e.end_mm)?;
            e.start_mm += 0.0;
            e.end_mm += 0.0;
            if !seen.insert(e.entity) {
                return Err(invalid(format!(
                    "le tracé {} a deux surcharges de marge : une seule par tracé",
                    e.entity.0
                )));
            }
        }
        self.edges.sort_by_key(|e| e.entity);
        let name = |what: &str, e: EntityId| {
            invalid(format!(
                "le tracé {} a déjà un réglage de bord ({what}) : surcharge, ourlet et rempli \
                 s'excluent, un seul par tracé",
                e.0
            ))
        };
        for h in &mut self.hems {
            check_value(h.height_mm).map_err(|_| {
                invalid(format!(
                    "une hauteur d'ourlet doit être un nombre fini compris entre 0 et \
                     {MAX_SEAM_MM} mm"
                ))
            })?;
            h.height_mm += 0.0;
            if !seen.insert(h.entity) {
                return Err(name("ourlet", h.entity));
            }
        }
        for f in &mut self.folds {
            check_value(f.value_mm).map_err(|_| {
                invalid(format!(
                    "une valeur de rempli doit être un nombre fini compris entre 0 et \
                     {MAX_SEAM_MM} mm"
                ))
            })?;
            f.value_mm += 0.0;
            if !seen.insert(f.entity) {
                return Err(name("rempli", f.entity));
            }
            if f.notch_step_mm.is_some() && f.notch_count.is_some() {
                return Err(invalid(format!(
                    "le rempli du tracé {} a un pas et un nombre de crans : un seul des deux",
                    f.entity.0
                )));
            }
            if let Some(s) = &mut f.notch_step_mm {
                if !s.is_finite() || !(MIN_FOLD_NOTCH_STEP_MM..=MAX_FOLD_NOTCH_STEP_MM).contains(s)
                {
                    return Err(invalid(format!(
                        "le pas des crans de rempli doit être un nombre fini compris entre \
                         {MIN_FOLD_NOTCH_STEP_MM} et {MAX_FOLD_NOTCH_STEP_MM} mm"
                    )));
                }
                *s += 0.0;
            }
            if f.notch_count
                .is_some_and(|n| n == 0 || n as usize > MAX_FOLD_NOTCHES)
            {
                return Err(invalid(format!(
                    "le nombre de crans de rempli doit être compris entre 1 et {MAX_FOLD_NOTCHES}"
                )));
            }
        }
        self.hems.sort_by_key(|h| h.entity);
        self.folds.sort_by_key(|f| f.entity);
        if let Some(l) = &mut self.max_corner_mm {
            if !l.is_finite() || !(0.0..=MAX_CORNER_MM).contains(l) {
                return Err(invalid(format!(
                    "la longueur maximale d'une pointe doit être un nombre fini compris entre 0 et \
                     {MAX_CORNER_MM} mm"
                )));
            }
            *l += 0.0;
        }
        let mut seen = HashSet::new();
        for c in &mut self.corners {
            if !seen.insert(c.entity) {
                return Err(invalid(format!(
                    "le sommet au début du tracé {} a deux types de coin : un seul par sommet",
                    c.entity.0
                )));
            }
            if let CornerType::Truncated { depth_mm } = &mut c.corner {
                if !depth_mm.is_finite() || !(0.0..=MAX_CORNER_MM).contains(depth_mm) {
                    return Err(invalid(format!(
                        "la profondeur d'un coin tronqué doit être un nombre fini compris entre 0 \
                         et {MAX_CORNER_MM} mm"
                    )));
                }
                *depth_mm += 0.0;
            }
        }
        self.corners.sort_by_key(|c| c.entity);
        Ok(self)
    }
}

/// Marge de couture de repli (mm) pour une pièce **sans catégorie** (« non défini ») : la valeur
/// du tissu. Convention d'atelier non validée par une modéliste.
pub const FALLBACK_SEAM_MM: f64 = 10.0;

/// Jeu de marges de couture par défaut, une valeur (mm, 0 à 1000) par catégorie de pièce
/// (US-05.5). Table complète : aucune valeur n'est facultative. Stockée dans le projet (champ
/// `seam_defaults`) pour que le fichier reste reproductible ; **jamais appliquée
/// rétroactivement** : elle ne propose que la valeur d'une marge qu'on crée ou qu'on réapplique
/// explicitement.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeamDefaults {
    pub fabric_mm: f64,
    pub lining_mm: f64,
    pub interfacing_mm: f64,
    pub leather_mm: f64,
    pub reinforcement_mm: f64,
}

impl SeamDefaults {
    /// Valeurs d'atelier courantes (convention non validée) : tissu 10, doublure 10, entoilage 5,
    /// cuir 6, renfort 5 mm.
    pub const fn builtin() -> SeamDefaults {
        SeamDefaults {
            fabric_mm: 10.0,
            lining_mm: 10.0,
            interfacing_mm: 5.0,
            leather_mm: 6.0,
            reinforcement_mm: 5.0,
        }
    }

    pub fn get(&self, category: PieceCategory) -> f64 {
        match category {
            PieceCategory::Fabric => self.fabric_mm,
            PieceCategory::Lining => self.lining_mm,
            PieceCategory::Interfacing => self.interfacing_mm,
            PieceCategory::Leather => self.leather_mm,
            PieceCategory::Reinforcement => self.reinforcement_mm,
        }
    }

    pub fn set(&mut self, category: PieceCategory, mm: f64) {
        match category {
            PieceCategory::Fabric => self.fabric_mm = mm,
            PieceCategory::Lining => self.lining_mm = mm,
            PieceCategory::Interfacing => self.interfacing_mm = mm,
            PieceCategory::Leather => self.leather_mm = mm,
            PieceCategory::Reinforcement => self.reinforcement_mm = mm,
        }
    }

    /// Valeurs finies et bornées, zéro négatif écrasé.
    pub fn normalized(mut self) -> Result<SeamDefaults, CoreError> {
        for c in PieceCategory::ALL {
            let v = self.get(c);
            if !v.is_finite() || !(0.0..=MAX_SEAM_MM).contains(&v) {
                return Err(invalid(format!(
                    "la marge par défaut de la catégorie « {} » doit être un nombre fini compris \
                     entre 0 et {MAX_SEAM_MM} mm",
                    c.key()
                )));
            }
            self.set(c, v + 0.0);
        }
        Ok(self)
    }
}

impl Project {
    /// Jeu de marges par défaut en vigueur : celui du projet, sinon les valeurs d'atelier.
    pub fn seam_defaults_effective(&self) -> SeamDefaults {
        self.seam_defaults.unwrap_or(SeamDefaults::builtin())
    }

    /// Marge proposée à la création de la marge d'une pièce de catégorie `category` ; sans
    /// catégorie, la valeur de repli [`FALLBACK_SEAM_MM`].
    pub fn seam_default_for(&self, category: Option<PieceCategory>) -> f64 {
        category.map_or(FALLBACK_SEAM_MM, |c| self.seam_defaults_effective().get(c))
    }

    /// Valeur de la catégorie d'une pièce, pour « appliquer la valeur de la catégorie » ; refus
    /// expliqué si la pièce n'a pas de catégorie.
    pub fn category_seam_mm(&self, piece: PieceId) -> Result<f64, CoreError> {
        let p = self.piece(piece)?;
        let c = p.category.ok_or_else(|| {
            invalid(format!(
                "la pièce « {} » n'a pas de catégorie (non défini) : choisissez-en une pour \
                 appliquer la marge de sa catégorie",
                p.name
            ))
        })?;
        Ok(self.seam_defaults_effective().get(c))
    }

    /// Validation du jeu chargé : bornes, et forme canonique (différent des valeurs d'atelier).
    pub(crate) fn validate_seam_defaults(&self) -> Result<(), CoreError> {
        let Some(d) = &self.seam_defaults else {
            return Ok(());
        };
        d.normalized()
            .map_err(|e| invalid(format!("marges par catégorie : {e}")))?;
        if *d == SeamDefaults::builtin() {
            return Err(invalid(
                "marges par catégorie non canoniques (identiques aux valeurs d'atelier) : \
                 omettez-les",
            ));
        }
        Ok(())
    }

    /// `SetSeamDefaults` : remplace le jeu du projet (`None` ou valeurs d'atelier = le retire).
    pub(crate) fn set_seam_defaults(
        &mut self,
        defaults: Option<SeamDefaults>,
    ) -> Result<(), CoreError> {
        self.seam_defaults = match defaults {
            Some(d) => Some(d.normalized()?).filter(|d| *d != SeamDefaults::builtin()),
            None => None,
        };
        Ok(())
    }

    /// Contour de coupe d'une pièce : le contour de couture décalé de sa marge (voir
    /// `pf_geometry::seam`), **recalculé à chaque appel** d'après la construction actuelle. Une
    /// pièce sans marge, ou **déclarée en coupe nette** (US-05.6), donne une copie exacte du
    /// contour de couture (même périmètre, même aire). Erreur expliquée si le
    /// contour est vide, ouvert, dégénéré, ou si la marge est impossible pour la forme. Les pinces
    /// réglées « coupe pince fermée » (PH-5a) sont fermées à leur bouche (second élément : motif
    /// du repli sur la coupe ordinaire).
    fn cut_contour_report(
        &self,
        piece: PieceId,
    ) -> Result<(CutContour, Option<String>), CoreError> {
        let p = self.piece(piece)?;
        let chain = self
            .piece_chain_any(piece)
            .map_err(|e| invalid(format!("contour de coupe de « {} » : {e}", p.name)))?;
        let default = SeamAllowance::default();
        // Coupe nette (US-05.6) : jamais de marge, quoi que contienne `seam` (état invalide).
        let seam = p.seam.as_ref().filter(|_| !p.net_cut).unwrap_or(&default);
        // Bords sur le pli (PH-5b) : marge **nulle**, quoi que le réglage du bord (ignoré, jamais
        // effacé). Seuls les bords du contour (un segment) comptent.
        let on_fold = |entity: EntityId| p.on_fold.contains(&entity);
        let margins: Vec<EdgeMargin> = chain
            .iter()
            .map(|e| {
                let (start_mm, end_mm) = if on_fold(e.source) {
                    (0.0, 0.0)
                } else {
                    seam.margin_of(e.source)
                };
                EdgeMargin { start_mm, end_mm }
            })
            .collect();
        let geoms: Vec<_> = chain.iter().map(|e| e.geom).collect();
        // Coins d'ourlet (forme symétrisée) : au sommet de fin de l'ourlet, pli sur le bord
        // précédent ; au sommet de début, pli sur le bord suivant (qui l'emporte si deux ourlets
        // se suivent). Coins saillants seulement ; un type choisi à la main l'emporte.
        let n = chain.len();
        let mut by_vertex: Vec<(usize, CornerKind)> = Vec::new();
        let mut put = |vertex: usize, kind: CornerKind| {
            by_vertex.retain(|c| c.0 != vertex);
            by_vertex.push((vertex, kind));
        };
        // Le pli ne s'applique qu'à un coin saillant dont l'autre bord (`other`) est un segment :
        // le miroir d'une courbe n'est pas une courbe de même nature (repli sur la marge simple).
        let convex = |vertex: usize, other: usize| {
            matches!(geoms[other], EdgeGeom::Line(..))
                && vertex_turn(&geoms, vertex).is_ok_and(|t| t == VertexTurn::Convex)
        };
        let hem_starts = |entity: EntityId| {
            vertex_of(&chain, entity).ok_or_else(|| {
                invalid(format!(
                    "contour de coupe de « {} » : le tracé {} ne commence pas à un sommet du \
                     contour (ourlet sans effet possible)",
                    p.name, entity.0
                ))
            })
        };
        for h in seam.hems.iter().filter(|h| !on_fold(h.entity)) {
            let start = hem_starts(h.entity)?;
            let mut end = (start + 1) % n;
            while chain[end].source == h.entity && end != start {
                end = (end + 1) % n;
            }
            if convex(end, end) {
                put(end, CornerKind::HemFoldPrevious);
            }
        }
        for h in seam.hems.iter().filter(|h| !on_fold(h.entity)) {
            let start = hem_starts(h.entity)?;
            if convex(start, (start + n - 1) % n) {
                put(start, CornerKind::HemFoldNext);
            }
        }
        for c in &seam.corners {
            let vertex = vertex_of(&chain, c.entity).ok_or_else(|| {
                invalid(format!(
                    "contour de coupe de « {} » : le tracé {} ne commence pas à un sommet du \
                     contour (type de coin sans effet possible)",
                    p.name, c.entity.0
                ))
            })?;
            put(vertex, c.corner.kind());
        }
        let corners: Vec<CornerSpec> = by_vertex
            .into_iter()
            .map(|(vertex, kind)| CornerSpec { vertex, kind })
            .collect();
        let options = SeamOptions {
            corners,
            max_tip_mm: seam.max_corner_mm,
        };
        let plain = || {
            seam_contour_with(&geoms, &margins, &options)
                .map_err(|e| invalid(format!("contour de coupe de « {} » : {e}", p.name)))
        };
        // Coupe « pince fermée » (PH-5a) : tout ou rien. Une pince réglée qui ne se calcule pas,
        // ou un calcul qui échoue, laisse la coupe ordinaire et un motif (jamais un refus).
        let has_margin = margins.iter().any(|m| m.start_mm != 0.0 || m.end_mm != 0.0);
        let mut dart_failure: Option<String> = None;
        let mut closings: Vec<DartClosing> = Vec::new();
        if has_margin && !p.net_cut {
            for d in p.darts.iter().filter(|d| d.cut_closed) {
                match self.dart_calc(p, d) {
                    Ok(calc) => {
                        let src = calc.carrier;
                        let start = (0..n)
                            .find(|&i| {
                                chain[i].source == src && chain[(i + n - 1) % n].source != src
                            })
                            .unwrap_or(0);
                        let len = (0..n)
                            .take_while(|k| chain[(start + k) % n].source == src)
                            .count();
                        closings.push(DartClosing {
                            shape: calc.shape,
                            fixed: d.fixed,
                            carrier_start: start,
                            carrier_len: len,
                        });
                    }
                    Err(why) => {
                        dart_failure.get_or_insert(format!("pince {} : {why}", d.id.0));
                    }
                }
            }
        }
        let traces = if closings.is_empty() || dart_failure.is_some() {
            plain()?
        } else {
            match seam_contour_closed_darts(&geoms, &margins, &options, &closings) {
                Ok(t) => t,
                Err(e) => {
                    dart_failure = Some(e.to_string());
                    plain()?
                }
            }
        };
        // Portions de coupe portées par un bord sur le pli (PH-5b) : repérées et découpées aux
        // bornes du bord ; géométrie, aire et périmètre inchangés.
        let specs: Vec<crate::piece_fold::FoldSpec> = (0..n)
            .filter(|&i| on_fold(chain[i].source))
            .filter_map(|i| {
                let EdgeGeom::Line(a, b) = chain[i].geom else {
                    return None;
                };
                let collinear = |j: usize| {
                    chain[j].source != chain[i].source
                        && matches!(chain[j].geom, EdgeGeom::Line(c, d)
                            if pf_geometry::distance_mm(c, d) > 0.0
                                && (d - c).cross(b - a).abs()
                                    <= 1e-9 * (d - c).norm() * (b - a).norm()
                                && (d - c).dot(b - a) > 0.0
                                && (a - c).cross(b - a).abs() <= 1e-6 * (b - a).norm())
                };
                Some(crate::piece_fold::FoldSpec {
                    entity: chain[i].source,
                    a,
                    b,
                    prev_collinear: collinear((i + n - 1) % n),
                    next_collinear: collinear((i + 1) % n),
                })
            })
            .collect();
        let (traces, fold_cuts) = if specs.is_empty() {
            (traces, Vec::new())
        } else {
            crate::piece_fold::split_fold_traces(traces, &specs)
        };
        let elementary: Vec<_> = traces.iter().flat_map(trace_geometry).collect();
        Ok((
            CutContour {
                traces,
                perimeter_mm: chain_length_mm(&elementary),
                area_mm2: chain_signed_area_mm2(&elementary).abs(),
                fold_cuts,
            },
            dart_failure,
        ))
    }

    /// Contour de coupe d'une pièce, recalculé à chaque appel (voir `cut_contour_report`).
    pub fn piece_cut_contour(&self, piece: PieceId) -> Result<CutContour, CoreError> {
        self.cut_contour_report(piece).map(|(cut, _)| cut)
    }

    /// Pourquoi la coupe « pince fermée » (PH-5a) d'une pièce n'est pas appliquée : une pince
    /// réglée en coupe fermée n'est pas calculable, ou le calcul échoue (bouche sur un coin,
    /// bouches qui se touchent, bord trop court…). `None` : rien à signaler. La coupe renvoyée
    /// par `piece_cut_contour` est alors la coupe ordinaire (contrôle `CutDartFailed`).
    pub fn piece_cut_dart_failure(&self, piece: PieceId) -> Option<String> {
        self.cut_contour_report(piece).ok().and_then(|(_, f)| f)
    }

    /// Crans de rempli **calculés** (jamais stockés) des bords courbes d'une pièce, bord par bord
    /// (ordre des tracés), chacun dans l'ordre du contour anti-horaire. Un bord droit n'a pas de
    /// cran. Erreur expliquée si le calcul est impossible (contrôle `FoldNotchesFailed`).
    pub fn piece_fold_notches(&self, piece: PieceId) -> Result<Vec<FoldNotch>, CoreError> {
        let p = self.piece(piece)?;
        let Some(seam) = p.seam.as_ref().filter(|s| !s.folds.is_empty()) else {
            return Ok(Vec::new());
        };
        let chain = self
            .piece_chain_any(piece)
            .map_err(|e| invalid(format!("crans de rempli de « {} » : {e}", p.name)))?;
        let n = chain.len();
        let mut out = Vec::new();
        for f in &seam.folds {
            // Arêtes du tracé, dans l'ordre du contour (en partant du sommet où il commence).
            let first = vertex_of(&chain, f.entity).unwrap_or(0);
            let edges: Vec<_> = (0..n)
                .map(|k| &chain[(first + k) % n])
                .take_while(|e| e.source == f.entity)
                .map(|e| e.geom)
                .collect();
            if edges.is_empty() {
                return Err(invalid(format!(
                    "crans de rempli de « {} » : le tracé {} n'est plus dans le contour",
                    p.name, f.entity.0
                )));
            }
            if edges
                .iter()
                .all(|e| matches!(e, pf_geometry::region::EdgeGeom::Line(..)))
            {
                continue;
            }
            let plan = match (f.notch_count, f.notch_step_mm) {
                (Some(c), _) => FoldNotchPlan::Count(c as usize),
                (None, Some(s)) => FoldNotchPlan::Step(s),
                (None, None) => FoldNotchPlan::Step(DEFAULT_FOLD_NOTCH_STEP_MM),
            };
            let notches = fold_notches(&edges, plan).map_err(|e| {
                invalid(format!(
                    "crans de rempli de « {} » (tracé {}) : {e}",
                    p.name, f.entity.0
                ))
            })?;
            out.extend(notches.into_iter().enumerate().map(|(i, k)| FoldNotch {
                entity: f.entity,
                index: i + 1,
                distance_mm: k.distance_mm,
                point: k.point,
                normal: k.normal,
            }));
        }
        Ok(out)
    }

    /// Milieu de chaque bord à rempli de valeur non nulle (PH-5c), droit ou courbe, dans l'ordre
    /// des réglages. Un bord sur le pli est ignoré (sa marge ne compte pas), un bord devenu
    /// illisible (absent du contour, longueur nulle) n'a pas de milieu : il est omis.
    pub fn piece_fold_label_anchors(&self, piece: PieceId) -> Vec<FoldLabelAnchor> {
        let Ok(p) = self.piece(piece) else {
            return Vec::new();
        };
        let Some(seam) = p.seam.as_ref().filter(|s| !s.folds.is_empty()) else {
            return Vec::new();
        };
        let Ok(chain) = self.piece_chain_any(piece) else {
            return Vec::new();
        };
        let n = chain.len();
        let mut out = Vec::new();
        for f in seam
            .folds
            .iter()
            .filter(|f| f.value_mm > 0.0 && !p.on_fold.contains(&f.entity))
        {
            let first = vertex_of(&chain, f.entity).unwrap_or(0);
            let edges: Vec<_> = (0..n)
                .map(|k| &chain[(first + k) % n])
                .take_while(|e| e.source == f.entity)
                .map(|e| e.geom)
                .collect();
            let Ok(mid) = fold_notches(&edges, FoldNotchPlan::Count(1)) else {
                continue;
            };
            if let [m] = mid.as_slice() {
                out.push(FoldLabelAnchor {
                    entity: f.entity,
                    value_mm: f.value_mm,
                    point: m.point,
                    normal: m.normal,
                    length_mm: m.distance_mm * 2.0,
                });
            }
        }
        out
    }

    /// Contrôles qui ne demandent pas de calcul : surcharges sur des tracés du contour, pas de
    /// marge progressive sur une courbe, ourlet sur un bord droit, crans de rempli sur un bord
    /// courbe. Sert à la validation d'une pièce chargée et à la commande.
    fn check_seam_refs(&self, piece: &Piece, seam: &SeamAllowance) -> Result<(), CoreError> {
        let is_segment = |entity: EntityId| {
            self.sheets
                .iter()
                .find_map(|s| s.entity(entity).ok())
                .is_some_and(|x| matches!(x.kind, EntityKind::Segment { .. }))
        };
        for h in &seam.hems {
            if !piece.outline.iter().any(|o| o.entity == h.entity) {
                return Err(CoreError::DeadReference(format!(
                    "l'ourlet de la pièce « {} » porte sur le tracé {}, qui n'est pas dans son \
                     contour",
                    piece.name, h.entity.0
                )));
            }
            if !is_segment(h.entity) {
                return Err(invalid(format!(
                    "ourlet : bord droit seulement (le tracé {} de la pièce « {} » est une courbe)",
                    h.entity.0, piece.name
                )));
            }
        }
        for f in &seam.folds {
            if !piece.outline.iter().any(|o| o.entity == f.entity) {
                return Err(CoreError::DeadReference(format!(
                    "le rempli de la pièce « {} » porte sur le tracé {}, qui n'est pas dans son \
                     contour",
                    piece.name, f.entity.0
                )));
            }
            if (f.notch_step_mm.is_some() || f.notch_count.is_some()) && is_segment(f.entity) {
                return Err(invalid(format!(
                    "crans de rempli : bord courbe seulement (le tracé {} de la pièce « {} » est \
                     un segment)",
                    f.entity.0, piece.name
                )));
            }
        }
        for e in &seam.edges {
            if !piece.outline.iter().any(|o| o.entity == e.entity) {
                return Err(CoreError::DeadReference(format!(
                    "la marge de la pièce « {} » porte sur le tracé {}, qui n'est pas dans son contour",
                    piece.name, e.entity.0
                )));
            }
            if e.start_mm != e.end_mm {
                let is_segment = self
                    .sheets
                    .iter()
                    .find_map(|s| s.entity(e.entity).ok())
                    .is_some_and(|x| matches!(x.kind, EntityKind::Segment { .. }));
                if !is_segment {
                    return Err(invalid(format!(
                        "marge progressive : segments seulement (le tracé {} de la pièce « {} » \
                         est une courbe, sa marge est constante)",
                        e.entity.0, piece.name
                    )));
                }
            }
        }
        for c in &seam.corners {
            if !piece.outline.iter().any(|o| o.entity == c.entity) {
                return Err(CoreError::DeadReference(format!(
                    "le type de coin de la pièce « {} » porte sur le tracé {}, qui n'est pas dans \
                     son contour",
                    piece.name, c.entity.0
                )));
            }
        }
        Ok(())
    }

    /// Sommets non lisses du contour de couture d'une pièce (contour lu en anti-horaire), chacun
    /// identifié par le tracé qui y commence : c'est là qu'un type de coin se choisit.
    pub fn seam_vertices(&self, piece: PieceId) -> Result<Vec<SeamVertex>, CoreError> {
        let chain = self.piece_chain_any(piece)?;
        let geoms: Vec<_> = chain.iter().map(|e| e.geom).collect();
        let n = chain.len();
        let mut out = Vec::new();
        for j in 0..n {
            if chain[j].source == chain[(j + n - 1) % n].source {
                continue; // intérieur d'un tracé (cercle, spline) : jamais un coin
            }
            let turn = vertex_turn(&geoms, j).map_err(|e| invalid(e.to_string()))?;
            if matches!(turn, VertexTurn::Smooth | VertexTurn::Reversal) {
                continue;
            }
            out.push(SeamVertex {
                entity: chain[j].source,
                point: geoms[j].start(),
                convex: turn == VertexTurn::Convex,
            });
        }
        Ok(out)
    }

    /// Un **nouveau** type de coin doit viser un coin saillant d'un sommet non lisse du contour
    /// actuel (refus expliqué sinon). Les types déjà enregistrés ne sont pas recontrôlés : si la
    /// construction change, le contour de coupe devient un contrôle (`CutContourFailed`).
    fn check_new_corners(
        &self,
        piece: PieceId,
        seam: &SeamAllowance,
        old: Option<&SeamAllowance>,
    ) -> Result<(), CoreError> {
        let fresh: Vec<&CornerAllowance> = seam
            .corners
            .iter()
            .filter(|c| !old.is_some_and(|o| o.corners.contains(c)))
            .collect();
        if fresh.is_empty() {
            return Ok(());
        }
        let vertices = self.seam_vertices(piece)?;
        for c in fresh {
            match vertices.iter().find(|v| v.entity == c.entity) {
                None => {
                    return Err(invalid(format!(
                        "aucun coin au début du tracé {} : le sommet est inexistant ou lisse (une \
                         courbe lisse n'a pas de coin)",
                        c.entity.0
                    )))
                }
                Some(v) if !v.convex => {
                    return Err(invalid(format!(
                        "le coin au début du tracé {} est rentrant : un type de coin ne s'applique \
                         qu'à un coin saillant",
                        c.entity.0
                    )))
                }
                Some(_) => {}
            }
        }
        Ok(())
    }

    /// `SetNetCut` (US-05.6) : déclare ou retire la coupe nette. Exclusive avec une marge : refus
    /// expliqué si la pièce en a une (aucune marge n'est jamais effacée en silence).
    pub(crate) fn set_net_cut(&mut self, piece: PieceId, net_cut: bool) -> Result<(), CoreError> {
        let p = self.piece(piece)?;
        if p.derived.is_some() {
            return Err(invalid(format!(
                "la pièce « {} » est une pièce dérivée liée à sa mère : cassez le lien avant de \
                 déclarer sa coupe nette",
                p.name
            )));
        }
        if net_cut == p.net_cut {
            return Ok(());
        }
        if net_cut {
            if p.seam.is_some() {
                return Err(invalid(format!(
                    "la pièce « {} » a une marge de couture : une coupe nette n'a pas de marge, \
                     retirez d'abord la marge",
                    p.name
                )));
            }
            let c = self.piece_contour(piece).map_err(|_| {
                invalid(format!(
                    "la pièce « {} » n'a pas de contour : une coupe nette demande un contour fermé",
                    p.name
                ))
            })?;
            if !c.closed {
                return Err(invalid(format!(
                    "le contour de la pièce « {} » n'est pas fermé : une coupe nette demande un \
                     contour fermé",
                    p.name
                )));
            }
        }
        self.piece_mut(piece)?.net_cut = net_cut;
        Ok(())
    }

    /// Validation d'une pièce chargée ou modifiée.
    pub(crate) fn validate_seam(&self, piece: &Piece) -> Result<(), CoreError> {
        if piece.net_cut && piece.seam.is_some() {
            return Err(invalid(format!(
                "pièce « {} » : coupe nette et marge de couture s'excluent",
                piece.name
            )));
        }
        let Some(seam) = &piece.seam else {
            return Ok(());
        };
        let ctx =
            |e: CoreError| invalid(format!("pièce « {} » : marge de couture : {e}", piece.name));
        seam.clone().normalized().map_err(ctx)?;
        if seam.is_default() {
            return Err(invalid(format!(
                "pièce « {} » : marge de couture non canonique (défaut nul sans surcharge) : omettez-la",
                piece.name
            )));
        }
        self.check_seam_refs(piece, seam)
    }

    /// `SetSeamAllowance` : remplace la marge de couture d'une pièce (`None` = la retire).
    pub(crate) fn set_seam_allowance(
        &mut self,
        piece: PieceId,
        seam: Option<SeamAllowance>,
    ) -> Result<(), CoreError> {
        let p = self.piece(piece)?;
        if p.derived.is_some() {
            return Err(invalid(format!(
                "la pièce « {} » est une pièce dérivée liée à sa mère : cassez le lien avant de \
                 régler sa marge de couture",
                p.name
            )));
        }
        let seam = match seam {
            Some(s) => Some(s.normalized()?).filter(|s| !s.is_default()),
            None => None,
        };
        if seam.is_some() && p.net_cut {
            return Err(invalid(format!(
                "la pièce « {} » est déclarée en coupe nette (bord franc) : retirez d'abord la \
                 coupe nette pour lui donner une marge de couture",
                p.name
            )));
        }
        if let Some(s) = &seam {
            let c = self.piece_contour(piece).map_err(|_| {
                invalid(format!(
                    "la pièce « {} » n'a pas de contour : une marge de couture demande un contour fermé",
                    p.name
                ))
            })?;
            if !c.closed {
                return Err(invalid(format!(
                    "le contour de la pièce « {} » n'est pas fermé : une marge de couture demande \
                     un contour fermé",
                    p.name
                )));
            }
            self.check_seam_refs(p, s)?;
            self.check_new_corners(piece, s, p.seam.as_ref())?;
            let sheet = self.piece_sheet(piece)?;
            for o in &p.outline {
                let layer = sheet.entity(o.entity)?.layer;
                if sheet.layer_locked(layer) {
                    return Err(CoreError::LayerLocked(format!(
                        "marge de couture : un tracé du contour est dans le calque « {} », \
                         verrouillé (déverrouillez-le d'abord)",
                        sheet.layer_name(layer)
                    )));
                }
            }
        }
        self.piece_mut(piece)?.seam = seam;
        Ok(())
    }

    /// Après une commande : une surcharge dont le tracé n'est plus dans le contour de la pièce est
    /// retirée **avec une remarque** (jamais une référence morte silencieuse) ; une marge qui
    /// devient vide disparaît. Ne touche que ce qui existait, identique, avant la commande : une
    /// référence morte introduite par la commande elle-même reste refusée par la validation.
    pub(crate) fn prune_dead_seam(&mut self, before: &Project, out: &mut Outcome) {
        if self.pieces.iter().all(|p| p.seam.is_none()) {
            return;
        }
        for p in &mut self.pieces {
            let Some(seam) = p.seam.as_mut() else {
                continue;
            };
            let old = before
                .pieces
                .iter()
                .find(|o| o.id == p.id)
                .and_then(|o| o.seam.as_ref());
            let alive: HashSet<EntityId> = p.outline.iter().map(|o| o.entity).collect();
            let len = seam.edges.len();
            seam.edges
                .retain(|e| alive.contains(&e.entity) || !old.is_some_and(|o| o.edges.contains(e)));
            let removed = len - seam.edges.len();
            if removed > 0 {
                out.notes.push(format!(
                    "{removed} marge(s) par bord de la pièce « {} » portaient sur un tracé qui n'est \
                     plus dans son contour : elles sont retirées.",
                    p.name
                ));
            }
            let len = seam.corners.len();
            seam.corners.retain(|c| {
                alive.contains(&c.entity) || !old.is_some_and(|o| o.corners.contains(c))
            });
            let removed = len - seam.corners.len();
            if removed > 0 {
                out.notes.push(format!(
                    "{removed} type(s) de coin de la pièce « {} » portaient sur un tracé qui n'est \
                     plus dans son contour : ils sont retirés.",
                    p.name
                ));
            }
            let len = seam.hems.len();
            seam.hems
                .retain(|h| alive.contains(&h.entity) || !old.is_some_and(|o| o.hems.contains(h)));
            let removed = len - seam.hems.len();
            if removed > 0 {
                out.notes.push(format!(
                    "{removed} ourlet(s) de la pièce « {} » portaient sur un tracé qui n'est plus \
                     dans son contour : ils sont retirés.",
                    p.name
                ));
            }
            let len = seam.folds.len();
            seam.folds
                .retain(|f| alive.contains(&f.entity) || !old.is_some_and(|o| o.folds.contains(f)));
            let removed = len - seam.folds.len();
            if removed > 0 {
                out.notes.push(format!(
                    "{removed} rempli(s) de la pièce « {} » portaient sur un tracé qui n'est plus \
                     dans son contour : ils sont retirés.",
                    p.name
                ));
            }
            if seam.is_default() && old.is_some_and(|o| !o.is_default()) {
                p.seam = None;
            }
        }
    }
}

/// Marge reportée sur une pièce issue de `orig` (séparer, assembler) : la valeur par défaut
/// seulement. Les surcharges par bord portent sur des tracés qui ne sont plus ceux de la
/// nouvelle pièce : elles ne sont pas reprises, et une remarque le dit.
pub(crate) fn carried_seam(orig: &Piece, notes: &mut Vec<String>) -> Option<SeamAllowance> {
    let seam = orig.seam.as_ref()?;
    if !seam.edges.is_empty() {
        notes.push(format!(
            "Les {} marge(s) par bord de la pièce « {} » ne sont pas reprises (les tracés ont \
             changé) : seule la marge par défaut ({} mm) l'est ; à régler sur la nouvelle pièce.",
            seam.edges.len(),
            orig.name,
            seam.default_mm
        ));
    }
    if !seam.hems.is_empty() || !seam.folds.is_empty() {
        notes.push(format!(
            "Les {} ourlet(s) et {} rempli(s) de la pièce « {} » ne sont pas repris (les tracés \
             ont changé) : à régler sur la nouvelle pièce.",
            seam.hems.len(),
            seam.folds.len(),
            orig.name
        ));
    }
    if !seam.corners.is_empty() {
        notes.push(format!(
            "Les {} type(s) de coin de la pièce « {} » ne sont pas repris (les sommets ont \
             changé) : à choisir de nouveau sur la nouvelle pièce.",
            seam.corners.len(),
            orig.name
        ));
    }
    Some(SeamAllowance {
        default_mm: seam.default_mm,
        max_corner_mm: seam.max_corner_mm,
        ..SeamAllowance::default()
    })
    .filter(|s| !s.is_default())
}

/// Lecture de `Piece.net_cut` : seul `true` est écrit (l'absence vaut faux) ; un `false` explicite
/// n'est pas la forme canonique et est refusé.
pub(crate) fn net_cut_only_true<'de, D: serde::Deserializer<'de>>(d: D) -> Result<bool, D::Error> {
    if bool::deserialize(d)? {
        Ok(true)
    } else {
        Err(serde::de::Error::custom(
            "net_cut : omettez le champ plutôt que d'écrire false",
        ))
    }
}

/// Sommet de la chaîne où commence le tracé `entity` (le tracé précédent est un autre tracé).
fn vertex_of(chain: &[crate::piece_split_join::ChainEdge], entity: EntityId) -> Option<usize> {
    let n = chain.len();
    (0..n).find(|&j| chain[j].source == entity && chain[(j + n - 1) % n].source != entity)
}
