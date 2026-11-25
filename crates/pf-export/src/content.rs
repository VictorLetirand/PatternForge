//! Contenu de la sortie (US-13.4) : quelles catégories d'éléments d'une feuille sont écrites
//! dans le PDF et le SVG.
//!
//! Le choix est un **filtre de [`Scene`]** : la scène complète est construite par
//! [`Scene::from_sheet_with`], puis [`OutputContent::apply`] en retire les catégories exclues. Le
//! PDF, le SVG, le nombre de pages estimé et l'aperçu de l'interface partent donc de la même
//! scène filtrée ; aucun rendu n'a de règle propre. Rien n'est stocké dans le projet : le
//! réglage est une préférence de l'application, global à toutes les feuilles.
//!
//! Choix de conception (voir `docs/DECISIONS.md`, 239 et suivantes) :
//!
//! - Une catégorie n'existe que si la scène a des éléments de ce genre : les 14 rôles de tracé
//!   sont regroupés en six catégories, puis crans, marquages, boutons, points, noms de points,
//!   cotations et cotes complètes, jambes de pince et étiquettes de pièce (15 catégories). Il n'y a
//!   pas de catégorie « courbes » (une courbe garde son rôle).
//! - Couture et coupe sont deux catégories indépendantes : couture seule, coupe seule ou les deux.
//!   Le « contour de coupe » est le tracé de rôle coupe tel que dessiné **et** le contour de
//!   coupe calculé des pièces à marge (US-05.4, `Scene::cut_contours`, catégorie coupe).
//! - Le découpage en pages se fait sur le contenu **retenu** : exclure une catégorie qui
//!   change la boîte englobante change la grille. À boîte identique, tout est identique.
//! - Les contraintes de droit-fil (`Scene::grain_limits`) sont conservées même si le droit-fil
//!   est exclu : la rotation d'économie de papier ne dépend pas de ce qui est dessiné.
//! - La page étalon, ses règles, ses repères et sa légende ne sont pas du contenu de patron :
//!   ils ne se filtrent pas. La légende suit les rôles réellement présents dans la scène filtrée.
//! - Taille : seule la taille unique du projet existe ([`OutputSize::ProjectSize`]) ; « plusieurs
//!   tailles » et « nid » attendent la gradation (E09, E10).

use crate::{ExportError, PieceExportMode, Scene, SceneOptions};
use pf_core::{PathRole, Project, SheetId};

/// Catégorie d'éléments exportables d'une feuille.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum OutputCategory {
    /// Tracés de rôle construction (droites, courbes, cercles, arcs, splines).
    Construction,
    /// Contour de couture (rôle ligne de couture).
    SewingContour,
    /// Contour de coupe (rôle ligne de coupe), tel que dessiné.
    CuttingContour,
    /// Droit-fil et ses flèches.
    Grainline,
    /// Pliure et lignes spéciales : surpiqûre, milieux, taille, hanche, poitrine, parage, collage.
    SpecialLines,
    /// Axes de repère et lignes d'aide exportées.
    Axes,
    /// Crans.
    Notches,
    /// Marquages : pointages, perçages, croix.
    Markings,
    /// Boutons et boutonnières.
    Buttons,
    /// Repères des points.
    Points,
    /// Noms écrits à côté des points (dépend des points).
    PointLabels,
    /// Cotations simples (mesures affichées).
    Dimensions,
    /// Cotes complètes (longueur développée, angle, rayon, diamètre, projections).
    FullDimensions,
    /// Jambes des pinces (catégorie propre : on peut masquer les pinces et garder les
    /// pointages, qui sont des marquages).
    Darts,
    /// Étiquettes de pièce (nom, code, quantité, matière, catégorie, commentaire).
    PieceLabels,
}

impl OutputCategory {
    pub const ALL: [OutputCategory; 15] = [
        Self::Construction,
        Self::SewingContour,
        Self::CuttingContour,
        Self::Grainline,
        Self::SpecialLines,
        Self::Axes,
        Self::Notches,
        Self::Markings,
        Self::Buttons,
        Self::Points,
        Self::PointLabels,
        Self::Dimensions,
        Self::FullDimensions,
        Self::Darts,
        Self::PieceLabels,
    ];

    /// Identifiant stable (préférences de l'application).
    pub fn key(self) -> &'static str {
        match self {
            Self::Construction => "construction",
            Self::SewingContour => "sewing",
            Self::CuttingContour => "cutting",
            Self::Grainline => "grainline",
            Self::SpecialLines => "special_lines",
            Self::Axes => "axes",
            Self::Notches => "notches",
            Self::Markings => "markings",
            Self::Buttons => "buttons",
            Self::Points => "points",
            Self::PointLabels => "point_labels",
            Self::Dimensions => "dimensions",
            Self::FullDimensions => "full_dimensions",
            Self::Darts => "darts",
            Self::PieceLabels => "piece_labels",
        }
    }

    pub fn from_key(key: &str) -> Option<OutputCategory> {
        Self::ALL.into_iter().find(|c| c.key() == key)
    }

    /// Catégorie d'un tracé selon son rôle (les 14 rôles sont couverts : un nouveau rôle ne
    /// compile pas tant qu'il n'a pas de catégorie).
    pub fn of_role(role: PathRole) -> OutputCategory {
        match role {
            PathRole::Construction => Self::Construction,
            PathRole::SewingContour => Self::SewingContour,
            PathRole::CuttingContour => Self::CuttingContour,
            PathRole::Grainline => Self::Grainline,
            PathRole::AnnotationAxis => Self::Axes,
            PathRole::Fold
            | PathRole::Topstitch
            | PathRole::CenterFront
            | PathRole::CenterBack
            | PathRole::WaistLine
            | PathRole::HipLine
            | PathRole::BustLine
            | PathRole::SkivingLine
            | PathRole::GlueLine => Self::SpecialLines,
        }
    }

    fn bit(self) -> u16 {
        1 << (self as u16)
    }
}

/// Taille exportée. Seule la taille unique du projet existe : le modèle de tailles (plusieurs
/// tailles, nid) dépend de la gradation (E09, E10) et d'une décision du propriétaire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OutputSize {
    /// La taille unique du projet (il n'y en a pas d'autre pour l'instant).
    #[default]
    ProjectSize,
}

impl OutputSize {
    pub fn key(self) -> &'static str {
        match self {
            Self::ProjectSize => "project",
        }
    }

    pub fn from_key(key: &str) -> Option<OutputSize> {
        match key {
            "project" => Some(Self::ProjectSize),
            _ => None,
        }
    }
}

/// Choix de contenu de la sortie. `OutputContent::default()` inclut tout : l'export est alors
/// identique à l'octet à celui d'avant US-13.4.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct OutputContent {
    /// Catégories exclues (un bit par catégorie) ; 0 = tout inclus.
    excluded: u16,
    pub size: OutputSize,
    /// Couture et/ou coupe calculée des pièces à marge de couture (US-05.4) ; `Both` par défaut.
    /// Réglage de construction de la scène, appliqué par [`OutputContent::scene`].
    pub piece_mode: PieceExportMode,
}

/// Ce qu'une sélection fait d'une feuille, pour l'aperçu : nombre d'éléments par catégorie.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CategoryCount {
    pub category: OutputCategory,
    /// Éléments de la feuille dans cette catégorie.
    pub available: usize,
    /// Éléments qui seront écrits.
    pub kept: usize,
}

/// Remarque sur une sélection (jamais une erreur : la sortie reste faisable).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentNote {
    /// Coupe seule demandée, mais la feuille n'a aucun tracé de rôle coupe : rien n'est inventé
    /// (aucune marge de couture n'existe encore).
    CuttingAbsent,
    /// Couture seule demandée, mais la feuille n'a aucun tracé de rôle couture.
    SewingAbsent,
    /// Noms de points demandés alors que les points sont exclus : aucun nom n'est écrit.
    LabelsWithoutPoints,
    /// La sélection ne laisse rien à écrire : l'export serait refusé.
    NothingKept,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentSummary {
    pub counts: Vec<CategoryCount>,
    pub notes: Vec<ContentNote>,
}

impl ContentSummary {
    pub fn total_available(&self) -> usize {
        self.counts.iter().map(|c| c.available).sum()
    }

    pub fn total_kept(&self) -> usize {
        self.counts.iter().map(|c| c.kept).sum()
    }
}

impl OutputContent {
    pub fn includes(&self, category: OutputCategory) -> bool {
        self.excluded & category.bit() == 0
    }

    pub fn set(&mut self, category: OutputCategory, included: bool) {
        if included {
            self.excluded &= !category.bit();
        } else {
            self.excluded |= category.bit();
        }
    }

    pub fn with(mut self, category: OutputCategory, included: bool) -> OutputContent {
        self.set(category, included);
        self
    }

    pub fn without(self, category: OutputCategory) -> OutputContent {
        self.with(category, false)
    }

    /// Seules ces catégories sont incluses.
    pub fn only(categories: &[OutputCategory]) -> OutputContent {
        let mut c = OutputContent::default();
        for cat in OutputCategory::ALL {
            c.set(cat, categories.contains(&cat));
        }
        c
    }

    /// Tout, sauf le contour de coupe.
    pub fn sewing_only() -> OutputContent {
        Self::default().without(OutputCategory::CuttingContour)
    }

    /// Tout, sauf le contour de couture.
    pub fn cutting_only() -> OutputContent {
        Self::default().without(OutputCategory::SewingContour)
    }

    /// Tout inclus : la sortie est celle d'avant US-13.4.
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    pub fn excluded(&self) -> Vec<OutputCategory> {
        OutputCategory::ALL
            .into_iter()
            .filter(|c| !self.includes(*c))
            .collect()
    }

    /// Les noms de points sont écrits seulement si les points le sont aussi.
    pub fn shows_point_labels(&self) -> bool {
        self.includes(OutputCategory::Points) && self.includes(OutputCategory::PointLabels)
    }

    /// Scène filtrée. Sélection par défaut : copie identique. Sinon les éléments des catégories
    /// exclues sont retirés, les autres gardent leurs coordonnées et leur ordre. Une feuille
    /// déjà vide reste `EmptySheet` ; une sélection qui ne laisse rien donne `NothingSelected`.
    pub fn apply(&self, scene: &Scene) -> Result<Scene, ExportError> {
        if self.is_default() {
            return Ok(scene.clone());
        }
        if scene.bbox().is_none() {
            return Err(ExportError::EmptySheet(scene.sheet_name.clone()));
        }
        let mut out = scene.clone();
        out.paths
            .retain(|p| self.includes(OutputCategory::of_role(p.role)));
        if !self.includes(OutputCategory::CuttingContour) {
            out.cut_contours.clear();
        }
        if !self.includes(OutputCategory::Notches) {
            out.notch_marks.clear();
            // Crans du contour de coupe calculé (rempli, US-06.1 sur la coupe) : même catégorie.
            for c in &mut out.cut_contours {
                c.notch_marks.clear();
                c.cut_notch_marks.clear();
            }
        }
        if !self.includes(OutputCategory::Markings) {
            out.markings.clear();
            out.stitch_holes.clear();
            out.zones.clear();
        }
        if !self.includes(OutputCategory::Buttons) {
            out.buttons.clear();
        }
        if !self.includes(OutputCategory::Points) {
            out.points.clear();
        } else if !self.shows_point_labels() {
            for p in &mut out.points {
                p.show_label = false;
            }
        }
        if !self.includes(OutputCategory::Dimensions) {
            out.dimensions.clear();
        }
        if !self.includes(OutputCategory::FullDimensions) {
            out.full_dimensions.clear();
        }
        if !self.includes(OutputCategory::Darts) {
            out.darts.clear();
            out.dart_warnings.clear();
        }
        if !self.includes(OutputCategory::PieceLabels) {
            out.piece_labels.clear();
            out.fold_overs.clear();
        }
        // Bords sur le pli (PH-5b) : « pliure et lignes spéciales » (la catégorie la plus proche).
        if !self.includes(OutputCategory::SpecialLines) {
            out.folds.clear();
        }
        if out.bbox().is_none() {
            return Err(ExportError::NothingSelected(scene.sheet_name.clone()));
        }
        Ok(out)
    }

    /// Scène d'une feuille du projet pour l'export : **le seul chemin** utilisé par l'export
    /// PDF, l'export SVG, le nombre de pages et l'aperçu.
    pub fn scene(
        &self,
        project: &Project,
        sheet: SheetId,
        options: SceneOptions,
    ) -> Result<Scene, ExportError> {
        let options = SceneOptions {
            piece_mode: self.piece_mode,
            ..options
        };
        self.apply(&Scene::from_sheet_with(project, sheet, options)?)
    }

    /// Éléments retenus par catégorie sur une scène **complète**, et remarques.
    pub fn summary(&self, full: &Scene) -> ContentSummary {
        let counts: Vec<CategoryCount> = OutputCategory::ALL
            .into_iter()
            .map(|category| {
                let available = count(full, category);
                let kept = match category {
                    OutputCategory::PointLabels if !self.shows_point_labels() => 0,
                    _ if self.includes(category) => available,
                    _ => 0,
                };
                CategoryCount {
                    category,
                    available,
                    kept,
                }
            })
            .collect();
        let mut notes = Vec::new();
        let n = |c: OutputCategory| count(full, c);
        if self.includes(OutputCategory::CuttingContour)
            && !self.includes(OutputCategory::SewingContour)
            && n(OutputCategory::CuttingContour) == 0
        {
            notes.push(ContentNote::CuttingAbsent);
        }
        if self.includes(OutputCategory::SewingContour)
            && !self.includes(OutputCategory::CuttingContour)
            && n(OutputCategory::SewingContour) == 0
        {
            notes.push(ContentNote::SewingAbsent);
        }
        if self.includes(OutputCategory::PointLabels) && !self.includes(OutputCategory::Points) {
            notes.push(ContentNote::LabelsWithoutPoints);
        }
        if counts.iter().all(|c| c.kept == 0) {
            notes.push(ContentNote::NothingKept);
        }
        ContentSummary { counts, notes }
    }
}

/// Nombre d'éléments d'une catégorie dans une scène (un tracé, un trait de cran, un marquage,
/// un bouton, un point, une cotation = un élément).
pub fn count(scene: &Scene, category: OutputCategory) -> usize {
    match category {
        OutputCategory::Notches => scene.notch_marks.len(),
        OutputCategory::Markings => {
            scene.markings.len() + scene.stitch_holes.len() + scene.zones.len()
        }
        OutputCategory::Buttons => scene.buttons.len(),
        OutputCategory::Points => scene.points.len(),
        OutputCategory::PointLabels => scene.points.iter().filter(|p| p.show_label).count(),
        OutputCategory::Dimensions => scene.dimensions.len(),
        OutputCategory::FullDimensions => scene.full_dimensions.len(),
        OutputCategory::Darts => scene.darts.len(),
        // Étiquettes de pièce et libellés « Rempli … mm » (PH-5c) : même catégorie.
        OutputCategory::PieceLabels => scene.piece_labels.len() + scene.fold_overs.len(),
        role_category => {
            let drawn = scene
                .paths
                .iter()
                .filter(|p| OutputCategory::of_role(p.role) == role_category)
                .count();
            // Contours de coupe calculés (US-05.4) : un élément par pièce.
            if role_category == OutputCategory::CuttingContour {
                drawn + scene.cut_contours.len()
            } else if role_category == OutputCategory::SpecialLines {
                // Bords sur le pli (PH-5b) : un élément par bord.
                drawn + scene.folds.len()
            } else {
                drawn
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn les_quatorze_roles_ont_une_categorie_et_les_cles_sont_uniques() {
        for role in PathRole::ALL {
            let c = OutputCategory::of_role(role);
            assert!(OutputCategory::ALL.contains(&c));
        }
        let mut keys: Vec<&str> = OutputCategory::ALL.iter().map(|c| c.key()).collect();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), OutputCategory::ALL.len());
        for c in OutputCategory::ALL {
            assert_eq!(OutputCategory::from_key(c.key()), Some(c));
        }
        assert_eq!(OutputCategory::from_key("inconnue"), None);
    }

    #[test]
    fn les_bits_sont_distincts_et_le_defaut_inclut_tout() {
        let mut seen = 0u16;
        for c in OutputCategory::ALL {
            assert_eq!(seen & c.bit(), 0);
            seen |= c.bit();
        }
        let d = OutputContent::default();
        assert!(d.is_default() && d.excluded().is_empty());
        assert!(OutputCategory::ALL.into_iter().all(|c| d.includes(c)));
    }
}
