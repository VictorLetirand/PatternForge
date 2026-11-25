//! Commandes d'édition atomiques. L'interface ne modifie jamais le modèle
//! directement : elle émet une [`Command`], appliquée sur une copie du projet,
//! validée, puis seulement alors adoptée. Une commande en échec ne laisse
//! aucune trace.

use crate::error::CoreError;
use crate::grading::GradingCopyMode;
use crate::model::*;
use pf_geometry::{PointMm, Side, Transform, EPSILON_MM};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

mod extract;
mod kept_rect;
mod paths;
mod quick;
mod symmetry;
pub use quick::CENTERS_LAYER_NAME;
mod tangency;
pub use paths::SPLIT_TOLERANCE_MM;
pub use tangency::TangentAt;

/// Deux points plus proches que cela (mm) sont considérés comme le même.
pub const MERGE_TOLERANCE_MM: f64 = 1e-6;

fn is_zero_f64(v: &f64) -> bool {
    *v == 0.0
}

/// Désigne un point existant, des coordonnées où créer un point libre, ou
/// une définition de point dérivé (milieu, intersection, point sur tracé).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PointRef {
    Existing(PointId),
    New(PointMm),
    Derived(PointDefinition),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CurveHandle {
    Start,
    End,
}

/// Nombre maximal de parts d'une division.
pub const MAX_DIVISION_PARTS: usize = 1000;

/// Découpage demandé à `Command::DivideTrace`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DivisionSpec {
    /// `n` parts égales (2 à [`MAX_DIVISION_PARTS`]).
    Equal { parts: usize },
    /// Une part par proportion (2 à [`MAX_DIVISION_PARTS`] valeurs finies > 0).
    Proportions { values: Vec<f64> },
}

impl DivisionSpec {
    /// Fractions cumulées (strictement croissantes, dans ]0 ; 1[) des points de division.
    pub fn ratios(&self) -> Result<Vec<f64>, CoreError> {
        let bad = |m: &str| CoreError::Invalid(format!("division : {m}"));
        let ratios: Vec<f64> = match self {
            DivisionSpec::Equal { parts } => {
                if !(2..=MAX_DIVISION_PARTS).contains(parts) {
                    return Err(bad(&format!(
                        "le nombre de parts doit être compris entre 2 et {MAX_DIVISION_PARTS}"
                    )));
                }
                (1..*parts).map(|i| i as f64 / *parts as f64).collect()
            }
            DivisionSpec::Proportions { values } => {
                if !(2..=MAX_DIVISION_PARTS).contains(&values.len()) {
                    return Err(bad(&format!(
                        "il faut entre 2 et {MAX_DIVISION_PARTS} proportions"
                    )));
                }
                if values.iter().any(|v| !v.is_finite() || *v <= 0.0) {
                    return Err(bad("les proportions doivent être finies et positives"));
                }
                let total: f64 = values.iter().sum();
                if !total.is_finite() || total <= 0.0 {
                    return Err(bad("la somme des proportions doit être finie et positive"));
                }
                let mut acc = 0.0;
                values[..values.len() - 1]
                    .iter()
                    .map(|v| {
                        acc += v;
                        acc / total
                    })
                    .collect()
            }
        };
        let mut prev = 0.0;
        for r in &ratios {
            if !(*r > prev && *r < 1.0) {
                return Err(bad(
                    "proportions trop disproportionnées pour être distinguées",
                ));
            }
            prev = *r;
        }
        Ok(ratios)
    }
}

/// D'où vient le contour d'une pièce extraite (US-04.1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PieceSource {
    /// Plus petite région fermée qui contient ce point, même si les tracés qui la
    /// bordent se croisent sans point commun.
    Click { at: PointMm },
    /// Chaîne fermée formée exactement par ces tracés (aucun n'est coupé).
    Paths { entities: Vec<EntityId> },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Command {
    RenameProject {
        name: String,
    },
    CreateSheet {
        name: String,
    },
    RenameSheet {
        sheet: SheetId,
        name: String,
    },
    DeleteSheet {
        sheet: SheetId,
    },
    /// Déplace la feuille à l'index donné dans la liste.
    MoveSheet {
        sheet: SheetId,
        new_index: usize,
    },
    /// Duplique la feuille (tous les identifiants régénérés, pièces liées
    /// copiées) sous le nom « X (copie) », juste après l'originale.
    DuplicateSheet {
        sheet: SheetId,
    },
    /// Ajoute en fin de liste une feuille lue dans un autre projet, avec ses
    /// pièces liées ; identifiants régénérés, nom rendu unique.
    ImportSheet {
        sheet: Box<Sheet>,
        #[serde(default)]
        pieces: Vec<Piece>,
    },
    /// Crée une pièce liée à la feuille (contour vide ; voir `SetPieceOutline`).
    AddPiece {
        sheet: SheetId,
        name: String,
        quantity: u32,
        material: Option<String>,
        mirrored_pair: bool,
    },
    /// Modifie la fiche d'une pièce (nom, quantité, matière, paire en miroir).
    SetPieceInfo {
        piece: PieceId,
        name: String,
        quantity: u32,
        material: Option<String>,
        mirrored_pair: bool,
    },
    /// Remplace tous les champs de la fiche de pièce (US-04.2) : nom, code,
    /// quantité (≥ 1), paire en miroir, matière, catégorie, commentaire. Les textes
    /// sont rognés ; vide = champ effacé. Le contour et le droit-fil ne changent pas.
    SetPieceCard {
        piece: PieceId,
        name: String,
        code: Option<String>,
        quantity: u32,
        mirrored_pair: bool,
        material: Option<String>,
        category: Option<PieceCategory>,
        comment: Option<String>,
    },
    DeletePiece {
        piece: PieceId,
    },
    /// Place la pièce dans le plan de pièces (US-04.5) : translation et rotation de
    /// mise en page, **sans toucher à la construction** (tracés, points, crans,
    /// cotations). `None` = retour au placement automatique. Refus : pièce inconnue,
    /// valeur non finie ou hors bornes (voir `Placement::normalized`).
    SetPiecePlacement {
        piece: PieceId,
        placement: Option<crate::piece_plan::Placement>,
    },
    /// Remplace le contour (ordonné) et le droit-fil d'une pièce.
    SetPieceOutline {
        piece: PieceId,
        outline: Vec<PieceEdge>,
        grainline: Option<EntityId>,
    },
    /// Remplace les réglages avancés du droit-fil d'une pièce (US-06.5) : direction de la
    /// ligne principale (tracé dessiné, parallèle ou perpendiculaire à un tracé de la
    /// feuille, biais ±45°), sens (aucun, poil, motif), inversion, tolérance de rotation
    /// au placement et lignes supplémentaires. Tout par défaut = aucun réglage stocké ;
    /// sans effet (aucune étape) si rien ne change. Refus sans trace : pièce inconnue ou
    /// sans droit-fil, ligne qui n'est pas un segment de rôle droit-fil de la feuille de la
    /// pièce, doublon, trop de lignes, tracé de référence absent, d'une autre feuille, sans
    /// direction (cercle, tracé fermé) ou ligne de la même pièce, biais autre que ±45°,
    /// tolérance non finie ou hors [0 ; 180].
    SetPieceGrain {
        piece: PieceId,
        grain: crate::piece_grain::PieceGrain,
    },
    /// Remplace la marge de couture d'une pièce (US-05.1) : valeur par défaut (0 à 1000 mm) et
    /// surcharges par tracé du contour (`start_mm` au premier point du tracé, `end_mm` au dernier,
    /// dans le sens anti-horaire du contour ; différentes = marge progressive, segments
    /// seulement). `None` ou une marge vide (défaut nul, sans surcharge) = aucune marge stockée ;
    /// sans effet (aucune étape) si rien ne change. Le contour de coupe n'est jamais stocké :
    /// `Project::piece_cut_contour` le calcule. Refus sans trace : pièce inconnue, dérivée liée,
    /// sans contour fermé, valeur négative / non finie / hors bornes, tracé hors du contour,
    /// doublon, marge progressive sur une courbe, calque verrouillé.
    SetSeamAllowance {
        piece: PieceId,
        seam: Option<crate::piece_seam::SeamAllowance>,
    },
    /// Déclare ou retire la « coupe nette » d'une pièce (US-05.6, cuir bord franc) : le contour de
    /// coupe est explicitement le contour de couture. Exclusive avec une marge de couture ; sans
    /// effet (aucune étape) si rien ne change. Refus sans trace : pièce inconnue, dérivée liée,
    /// sans contour fermé, pièce qui a déjà une marge de couture (la retirer d'abord).
    SetNetCut {
        piece: PieceId,
        net_cut: bool,
    },
    /// Remplace le jeu de marges de couture par défaut par catégorie de pièce (US-05.5 ; `None` =
    /// valeurs d'atelier). Ne réécrit **aucune** marge déjà posée ; sans effet (aucune étape) si
    /// rien ne change. Refus sans trace : valeur négative, non finie ou au-delà de 1000 mm.
    SetSeamDefaults {
        defaults: Option<crate::piece_seam::SeamDefaults>,
    },
    /// Ajoute un élément interne (ligne ou repère lié à la construction) à une
    /// pièce (US-04.3). Refus : cible absente ou d'une autre feuille, type
    /// incohérent, doublon, tracé du contour ou droit-fil.
    AddPieceInternal {
        piece: PieceId,
        target: InternalTarget,
        kind: InternalKind,
    },
    /// Retire un élément interne ; la construction n'est pas touchée.
    RemovePieceInternal {
        piece: PieceId,
        target: InternalTarget,
    },
    /// Change le type d'un élément interne.
    SetPieceInternalKind {
        piece: PieceId,
        target: InternalTarget,
        kind: InternalKind,
    },
    /// Importe en une étape les éléments de la construction entièrement dans le
    /// contour de la pièce (voir `Project::piece_internal_import`) ; sans effet si
    /// rien n'est à ajouter, refusée si la pièce n'a pas de contour fermé.
    ImportPieceInternals {
        piece: PieceId,
    },
    /// Ajoute un marquage intérieur (pointage, perçage ou croix) à une pièce
    /// (US-06.3) ; l'identifiant créé est dans `Outcome::markings`. L'ancrage est un
    /// point de la feuille de la pièce ou un bout de pince reculé. Refus sans trace :
    /// pièce inconnue ou sans feuille, point absent ou d'une autre feuille, diamètre ou
    /// côté non fini ou hors bornes, recul hors bornes, pointe et direction confondues.
    AddPieceMarking {
        piece: PieceId,
        at: crate::piece_marking::MarkingAnchor,
        shape: crate::piece_marking::MarkingShape,
    },
    /// Rangée de perçages **au pas** le long d'un bord du contour (PH-4b), en une étape
    /// annulable : un marquage ordinaire par perçage (identifiants dans `Outcome::markings`),
    /// ancré sur un point libre rangé dans le calque masqué « Points de perçages » (positions
    /// calculées à la création, figées ensuite ; la rangée n'est pas une entité). Voir
    /// [`MarkingRunSpec`](crate::MarkingRunSpec). Refus sans trace : pièce inconnue ou sans
    /// contour fermé, bord qui n'est pas une suite continue du contour, pas, nombre (1 à 500),
    /// distance (0 à 100 mm), marges ou dimensions hors bornes, plus de 1000 marquages sur la
    /// pièce, points d'ancrage dans un calque verrouillé. Un perçage hors de la pièce reste un
    /// avertissement (`PieceIssue::MarkingOutsideOutline`).
    AddPieceMarkingRun {
        piece: PieceId,
        run: crate::piece_marking_run::MarkingRunSpec,
    },
    /// Ajoute une ligne de piqûre **liée** à un bord du contour (US-06.9) ; l'identifiant créé
    /// est dans `Outcome::stitch_lines`. Les trous sont calculés à la lecture (jamais stockés) et
    /// suivent le bord. Refus sans trace : pièce sans contour fermé, tracés qui ne se suivent pas
    /// dans le contour, pas / retrait / nombre / dimensions hors bornes, trou hors de la pièce,
    /// continuation qui ne suit pas la ligne précédente. Voir [`StitchLineSpec`](crate::StitchLineSpec).
    AddStitchLine {
        piece: PieceId,
        line: crate::stitch_line::StitchLineSpec,
    },
    /// Remplace les paramètres d'une ligne de piqûre (mêmes refus que l'ajout) ; même identifiant.
    SetStitchLine {
        piece: PieceId,
        line: StitchLineId,
        spec: crate::stitch_line::StitchLineSpec,
    },
    /// Retire une ligne de piqûre (les lignes qui la continuaient sont rendues indépendantes).
    RemoveStitchLine {
        piece: PieceId,
        line: StitchLineId,
    },
    /// Fusionne deux lignes adjacentes de même forme, pas et retrait en une seule (pas continu
    /// autour de l'angle ; la première garde son identifiant).
    MergeStitchLines {
        piece: PieceId,
        first: StitchLineId,
        second: StitchLineId,
    },
    /// Déclare deux lignes de piqûre **appariées** (US-08.6) ; l'identifiant du couple est dans
    /// `Outcome::stitch_pairs`. Refus sans trace : ligne inconnue, même ligne des deux côtés,
    /// ligne déjà dans un couple.
    PairStitchLines {
        a: crate::stitch_pair::StitchRef,
        b: crate::stitch_pair::StitchRef,
    },
    /// Rompt un couple de lignes de piqûre (les lignes restent).
    UnpairStitchLines {
        pair: StitchPairId,
    },
    /// Reporte une ligne de piqûre sur la pièce voisine (US-08.6) en une étape : crée la ligne de
    /// `to_piece` sur `to_edges` (même forme, pas, retrait et nombre de trous) et la déclare
    /// appariée à la source. Identifiants dans `Outcome::stitch_lines` / `stitch_pairs`. Mêmes
    /// refus que `AddStitchLine`, plus : source déjà appariée, même pièce.
    CarryStitchLine {
        from: crate::stitch_pair::StitchRef,
        to_piece: PieceId,
        to_edges: Vec<EntityId>,
    },
    /// Ajoute une zone de parage, de collage ou de découpe interne (US-06.10) : bande de largeur
    /// donnée le long d'un bord du contour (liée au bord, recalculée à la lecture) ou polygone
    /// libre ; l'identifiant créé est dans `Outcome::zones`. Refus sans trace : largeur hors
    /// bornes, bord qui est le contour entier ou qui ne se suit pas, surface qui se recoupe, trop
    /// petite ou qui sort de la pièce. Voir [`ZoneSpec`](crate::ZoneSpec).
    AddPieceZone {
        piece: PieceId,
        zone: crate::piece_zone::ZoneSpec,
    },
    /// Remplace nature et forme d'une zone (mêmes refus que l'ajout) ; même identifiant.
    SetPieceZone {
        piece: PieceId,
        zone: ZoneId,
        spec: crate::piece_zone::ZoneSpec,
    },
    /// Retire une zone.
    RemovePieceZone {
        piece: PieceId,
        zone: ZoneId,
    },
    /// Retire un marquage ; la construction n'est pas touchée.
    RemovePieceMarking {
        piece: PieceId,
        marking: MarkingId,
    },
    /// Remplace l'ancrage et la forme d'un marquage (mêmes contrôles que l'ajout) ;
    /// sans effet (aucune étape) si rien ne change.
    SetPieceMarking {
        piece: PieceId,
        marking: MarkingId,
        at: crate::piece_marking::MarkingAnchor,
        shape: crate::piece_marking::MarkingShape,
    },
    /// Pose une pince sur une pièce (US-07.1) ; l'identifiant créé est dans `Outcome::darts`.
    /// `apex` = sommet (point de la feuille de la pièce, existant ou créé dans la même étape,
    /// intérieur au contour), `toward` = point d'axe (direction du sommet vers la base), `length_mm` = distance du sommet au milieu de la
    /// base, `opening_mm` = largeur de la base (≤ longueur), `fixed` = jambe qui reste en place à
    /// la fermeture. Les pieds (calculés) doivent être sur un même tracé du contour. Refus sans
    /// trace : pièce inconnue, dérivée liée ou sans contour fermé, point absent ou d'une autre
    /// feuille, valeurs non finies ou hors bornes, sommet = point d'axe, sommet hors de la pièce,
    /// pieds hors du contour ou sur deux tracés, jambe qui traverse le contour.
    AddPieceDart {
        piece: PieceId,
        apex: PointRef,
        toward: PointRef,
        length_mm: f64,
        opening_mm: f64,
        fixed: pf_geometry::Side,
    },
    /// Déclare un couple de bords à coudre ensemble (US-08.1) ; l'identifiant créé est dans
    /// `Outcome::seam_pairs`. `a` = côté à embuer. Une **déclaration** : longueurs, écart et embu
    /// sont lus par `Project::seam_pair_report`, jamais imposés. Refus sans trace : pièce
    /// inconnue, tracés qui ne se suivent pas dans le contour, tracé déjà cousu dans un autre
    /// couple (ou dans l'autre côté), embu ou tolérance non finis ou hors de 0 à 1000 mm.
    AddSeamPair {
        a: crate::seam_pair::SeamSide,
        b: crate::seam_pair::SeamSide,
        embu_mm: Option<f64>,
        tolerance_mm: Option<f64>,
    },
    /// Remplace un couple (mêmes contrôles) ; sans effet (aucune étape) si rien ne change.
    SetSeamPair {
        id: SeamPairId,
        a: crate::seam_pair::SeamSide,
        b: crate::seam_pair::SeamSide,
        embu_mm: Option<f64>,
        tolerance_mm: Option<f64>,
    },
    /// Retire un couple ; les pièces ne sont pas touchées.
    RemoveSeamPair {
        id: SeamPairId,
    },
    /// Marche de pièces (US-08.2) : reporte les crans du côté `from` du couple sur l'autre côté,
    /// à la même abscisse curviligne de couture (embu réparti proportionnellement), en **vrais
    /// crans** non liés (identifiants dans `Outcome::notches`). `which` : `None` = tous les crans
    /// du côté source, sinon la liste. Refus sans trace : couple inconnu, côté non calculable,
    /// cran hors du côté source, aucun cran à reporter, tous déjà présents (±0,01 mm).
    MarchNotches {
        pair: SeamPairId,
        from: crate::seam_pair::SeamWhich,
        #[serde(default)]
        which: Option<Vec<NotchId>>,
    },
    /// Retire une pince ; la construction n'est pas touchée.
    RemovePieceDart {
        piece: PieceId,
        dart: DartId,
    },
    /// Remplace les paramètres d'une pince (mêmes contrôles que la pose) ; sans effet (aucune
    /// étape) si rien ne change.
    SetPieceDart {
        piece: PieceId,
        dart: DartId,
        apex: PointId,
        toward: PointId,
        length_mm: f64,
        opening_mm: f64,
        fixed: pf_geometry::Side,
    },
    /// Règle la coupe « pince fermée » d'une pince (PH-5a) : le contour de coupe est alors calculé
    /// pince fermée puis rouvert à la bouche. Sans effet (aucune étape) si rien ne change. Refus
    /// sans trace : pièce ou pince inconnue, pièce dérivée liée. Un calcul impossible n'est pas
    /// un refus (`PieceIssue::CutDartFailed`).
    SetPieceDartCutClosed {
        piece: PieceId,
        dart: DartId,
        closed: bool,
    },
    /// Déclare (`on`) ou retire un bord « sur le pli » d'une pièce (PH-5b) : le bord n'a ni marge de
    /// couture ni coupe, il est écrit comme pliure à l'export. Sans effet (aucune étape) si rien ne
    /// change. Refus sans trace : pièce inconnue, pièce dérivée liée, tracé absent de la feuille ;
    /// à la pose, tracé hors du contour ou non rectiligne.
    SetPieceFold {
        piece: PieceId,
        edge: EntityId,
        on: bool,
    },
    /// Déclare (`no_grain`) ou retire « sans droit-fil » sur une pièce (PH-5d, cuir) : plus
    /// d'avertissement « droit-fil manquant », mention « Sans droit-fil » dans les exports. Sans
    /// effet (aucune étape) si rien ne change. Refus sans trace : pièce inconnue, pièce dérivée
    /// liée, pièce qui a déjà un droit-fil ; inversement, poser un droit-fil sur une pièce
    /// déclarée sans droit-fil est refusé (retirer d'abord la déclaration).
    SetPieceNoGrain {
        piece: PieceId,
        no_grain: bool,
    },
    /// Pose l'image de fond calibrée d'une feuille (US-14.1, E14) : l'image est **déjà copiée** à
    /// côté du projet (chemin relatif, empreinte et dimensions lues par l'appelant), jamais
    /// intégrée au JSON ; le résidu est calculé par la commande. Refus sans trace : feuille
    /// inconnue, fond déjà posé, chemin non relatif, empreinte mal formée, points hors de l'image,
    /// confondus, alignés ou (4 points) hors ordre anti-horaire, distance nulle ou non finie.
    SetBackground {
        sheet: SheetId,
        image: String,
        sha256: String,
        width_px: u32,
        height_px: u32,
        calibration: crate::background_ops::CalibrationSpec,
    },
    /// Remplace la calibration du fond (deux ou quatre points). Refus : pas de fond, fond verrouillé.
    CalibrateBackground {
        sheet: SheetId,
        calibration: crate::background_ops::CalibrationSpec,
    },
    /// Opacité [0 ; 1], visibilité et verrou du fond. Sans effet (aucune étape) si rien ne change.
    SetBackgroundDisplay {
        sheet: SheetId,
        opacity: f64,
        visible: bool,
        locked: bool,
    },
    /// Relie le fond à un autre fichier (ou au même, modifié), de mêmes dimensions ; la calibration
    /// est conservée. Refus : pas de fond, fond verrouillé, dimensions différentes.
    RelinkBackground {
        sheet: SheetId,
        image: String,
        sha256: String,
        width_px: u32,
        height_px: u32,
    },
    /// Retire le fond (le fichier image n'est jamais supprimé). Refus : pas de fond, verrouillé.
    RemoveBackground {
        sheet: SheetId,
    },
    /// Ajoute une variable à la fin de la table (US-03.1a). Refus sans trace : nom invalide,
    /// réservé ou déjà pris (casse comprise), expression qui ne s'analyse ou ne s'évalue pas
    /// (syntaxe, variable inconnue, cycle, division par zéro, valeur non finie), table pleine.
    AddVariable {
        name: String,
        unit: crate::variable::VariableUnit,
        expr: String,
    },
    /// Remplace l'expression d'une variable ; sans effet (aucune étape) si le texte (rogné) est
    /// inchangé. Refus sans trace : variable inconnue, expression invalide, cycle.
    SetVariableExpr {
        name: String,
        expr: String,
    },
    /// Change l'unité affichée (étiquette, aucune conversion) ; sans effet si identique.
    SetVariableUnit {
        name: String,
        unit: crate::variable::VariableUnit,
    },
    /// Renomme une variable et réécrit, dans la même étape, ses références dans les expressions
    /// des autres (jetons entiers : `B` ne touche pas `B2`). Refus sans trace : conflit de nom
    /// (casse comprise), nom invalide, variable inconnue.
    RenameVariable {
        from: String,
        to: String,
    },
    /// Supprime une variable ; refusé tant qu'une autre variable ou une formule liée s'y réfère
    /// (aucune cascade).
    RemoveVariable {
        name: String,
    },
    /// Pose ou remplace l'expression d'une variable **pour une taille** (US-09.7) : dans cette
    /// taille, la variable prend cette expression au lieu de celle de base (formule ou nombre).
    /// Sans effet (aucune étape) si le texte (rogné) est inchangé. Refus sans trace : variable ou
    /// taille inconnue, taille de base (sa valeur est l'expression de la variable), expression
    /// vide, qui ne s'analyse pas ou dont la table ne s'évalue pas dans cette taille (variable
    /// inconnue, cycle, division par zéro, valeur non finie, formule liée inévaluable).
    SetVariableSizeExpr {
        name: String,
        size: SizeId,
        expr: String,
    },
    /// Retire la valeur propre d'une variable pour une taille (elle reprend l'expression de
    /// base) ; sans effet (aucune étape) si elle n'en avait pas. Refus : variable ou taille
    /// inconnue.
    ClearVariableSizeExpr {
        name: String,
        size: SizeId,
    },
    /// Ajoute une mesure au barème (US-10.1), sans valeur. Refus sans trace : nom vide, non rogné
    /// ou trop long, déjà pris (casse ignorée), limite de 500 mesures.
    AddMeasurement {
        name: String,
        unit: crate::measurement::MeasurementUnit,
    },
    /// Renomme une mesure ; même nom = aucune étape. Refus : mesure inconnue, nom invalide ou pris.
    RenameMeasurement {
        measurement: MeasurementId,
        name: String,
    },
    /// Supprime une mesure du barème **et** ses références (valeurs des fiches client, retouches,
    /// mesures finies) dans la même étape, avec une remarque qui les compte.
    RemoveMeasurement {
        measurement: MeasurementId,
    },
    /// Pose la valeur d'une mesure pour une taille (mm ou degrés selon l'unité de la mesure) ;
    /// même valeur = aucune étape. Refus : mesure ou taille inconnue, valeur non finie, négative
    /// ou au-delà de 1 000 000.
    SetMeasurementValue {
        measurement: MeasurementId,
        size: SizeId,
        value: f64,
    },
    /// Efface la valeur d'une mesure pour une taille (non renseignée) ; déjà vide = aucune étape.
    ClearMeasurementValue {
        measurement: MeasurementId,
        size: SizeId,
    },
    /// Importe des lignes de barème (CSV lu par `measurement_csv`) : fusion par nom (casse ignorée),
    /// unité identique exigée pour une mesure existante ; une cellule vide laisse la valeur
    /// existante, sauf `clear_empty` (elle l'efface). Tout ou rien, une seule étape.
    ImportMeasurements {
        rows: Vec<crate::measurement_ops::MeasurementImportRow>,
        #[serde(default)]
        clear_empty: bool,
    },
    /// Crée une fiche client vide (US-10.2). Refus : nom invalide ou pris (casse ignorée), limite.
    AddClient {
        name: String,
    },
    /// Renomme une fiche client ; même nom = aucune étape.
    RenameClient {
        client: ClientId,
        name: String,
    },
    /// Supprime une fiche client.
    RemoveClient {
        client: ClientId,
    },
    /// Pose la valeur d'une mesure du barème dans une fiche client (mêmes bornes que le barème).
    SetClientValue {
        client: ClientId,
        measurement: MeasurementId,
        value: f64,
    },
    /// Efface la valeur d'une mesure dans une fiche (la mesure redevient manquante, jamais
    /// devinée) ; déjà manquante = aucune étape.
    ClearClientValue {
        client: ClientId,
        measurement: MeasurementId,
    },
    /// Génère une taille « sur-mesure » depuis une fiche client (US-10.2) : ajoute la taille
    /// `label` à la fin de `range` et remplit sa colonne du barème avec les valeurs de la fiche.
    /// Une mesure absente de la fiche reste **vide** (jamais devinée ni copiée d'une autre taille ;
    /// signalée par `Project::client_missing_measurements`). Si `label` existe déjà dans la gamme :
    /// refusé, sauf `replace` (confirmation explicite) qui réécrit alors **toute** sa colonne ;
    /// colonne déjà identique = aucune étape. Refus sans trace : fiche ou gamme inconnue, libellé
    /// invalide ou pris sans `replace`, limite de 60 tailles. La taille est ensuite indépendante de
    /// la fiche (aucun lien stocké).
    GenerateClientSize {
        client: ClientId,
        range: SizeRangeId,
        label: String,
        #[serde(default)]
        replace: bool,
    },
    /// Crée une retouche nommée (US-10.3) : `delta` (mm ou degrés selon la mesure) s'ajoute à la
    /// valeur de la mesure `target`, **à la lecture** (rien n'est modifié dans le barème ni dans la
    /// géométrie). Refus sans trace : nom invalide ou pris (casse ignorée), mesure inconnue, delta
    /// non fini ou au-delà de 1 000 000, limite de 500.
    AddAlteration {
        name: String,
        target: MeasurementId,
        delta: f64,
    },
    /// Renomme une retouche ; même nom = aucune étape.
    RenameAlteration {
        alteration: AlterationId,
        name: String,
    },
    /// Supprime une retouche.
    RemoveAlteration {
        alteration: AlterationId,
    },
    /// Change la mesure ciblée par une retouche (refus : mesure inconnue).
    SetAlterationTarget {
        alteration: AlterationId,
        target: MeasurementId,
    },
    /// Change le delta d'une retouche (mêmes bornes qu'à la création).
    SetAlterationDelta {
        alteration: AlterationId,
        delta: f64,
    },
    /// Crée une mesure finie (US-10.4) : valeur lue sur une couture appariée ou une cote complète,
    /// comparée à la mesure `measurement` du barème avec les tolérances −/+ (≥ 0, mêmes unités).
    /// Refus sans trace : nom invalide ou pris, mesure / couture / cote inconnue, unité de la
    /// source différente de celle de la mesure, tolérance non finie / négative / hors bornes.
    AddFinishedMeasure {
        name: String,
        measurement: MeasurementId,
        source: crate::measurement::FinishedSource,
        tolerance_minus: f64,
        tolerance_plus: f64,
    },
    /// Remplace la définition d'une mesure finie (mêmes refus ; mêmes valeurs = aucune étape).
    SetFinishedMeasure {
        finished: FinishedMeasureId,
        name: String,
        measurement: MeasurementId,
        source: crate::measurement::FinishedSource,
        tolerance_minus: f64,
        tolerance_plus: f64,
    },
    /// Supprime une mesure finie.
    RemoveFinishedMeasure {
        finished: FinishedMeasureId,
    },
    /// Crée un plan de coupe (socle d'E15 : stockage seulement). Refus sans trace : nom vide, trop
    /// long ou déjà pris (casse ignorée), matière vide, laize non finie / ≤ 0 / > 10 000 mm,
    /// lisière négative ou telle que 2 × lisière ≥ laize, écart hors de 0 à 1 000 mm, droit-fil
    /// hors de [0 ; 360[. Identifiant dans `Outcome::cutting_plans`.
    AddCuttingPlan {
        spec: crate::cutting_plan::CuttingPlanSpec,
    },
    /// Crée une variante vide (US-11.1). Refus sans trace : nom vide, trop long ou déjà pris (casse
    /// ignorée), plus de 50 variantes. Identifiant dans `Outcome::variants`.
    AddVariant {
        name: String,
    },
    /// Crée une règle de nommage (US-11.5). Refus sans trace : nom vide, trop long ou déjà pris,
    /// préfixe/suffixe/séparateur avec caractère de contrôle ou trop long, préfixe vide sans
    /// compteur, compteur hors bornes, cible déjà couverte par une autre règle. Identifiant dans
    /// `Outcome::naming_rules`. Ne renomme rien.
    AddNamingRule {
        spec: crate::naming::NamingRuleSpec,
    },
    /// Remplace les paramètres d'une règle (mêmes refus ; mêmes valeurs = aucune étape).
    UpdateNamingRule {
        rule: NamingRuleId,
        spec: crate::naming::NamingRuleSpec,
    },
    /// Supprime une règle (les noms déjà donnés ne changent pas).
    RemoveNamingRule {
        rule: NamingRuleId,
    },
    /// Renommage en lot explicite : tous les objets de la cible reçoivent début, début + pas, …
    /// dans l'ordre du projet (aperçu : `Project::naming_preview`). Refus : règle sans compteur,
    /// compteur dépassant 999 999 999, nom produit invalide. Une seule étape d'annulation.
    ApplyNamingRule {
        rule: NamingRuleId,
    },
    /// Renomme une variante (même nom = aucune étape ; mêmes refus que la création).
    RenameVariant {
        variant: VariantId,
        name: String,
    },
    /// Supprime une variante et ses entrées (les pièces ne changent pas).
    RemoveVariant {
        variant: VariantId,
    },
    /// Copie une variante (options, état des options, entrées) sous un nouveau nom. Identifiant
    /// dans `Outcome::variants`.
    DuplicateVariant {
        variant: VariantId,
        name: String,
    },
    /// Ajoute une pièce à une variante. Refus sans trace : variante ou pièce inconnue, quantité
    /// hors de 1 à 999, matière vide, option inconnue de la variante, même pièce avec la même
    /// option déjà présente, plus de 500 entrées.
    AddVariantEntry {
        variant: VariantId,
        piece: PieceId,
        quantity: u32,
        material: Option<String>,
        option: Option<String>,
    },
    /// Remplace quantité, matière et option de l'entrée (`piece`, `option`) ; mêmes refus que
    /// l'ajout ; mêmes valeurs = aucune étape.
    UpdateVariantEntry {
        variant: VariantId,
        piece: PieceId,
        option: Option<String>,
        quantity: u32,
        material: Option<String>,
        new_option: Option<String>,
    },
    /// Retire l'entrée (`piece`, `option`) d'une variante.
    RemoveVariantEntry {
        variant: VariantId,
        piece: PieceId,
        option: Option<String>,
    },
    /// Ajoute une option (inactive) à une variante ; nom unique sans tenir compte de la casse.
    AddVariantOption {
        variant: VariantId,
        option: String,
    },
    /// Retire une option ; refusé si une entrée l'utilise (l'entrée est nommée).
    RemoveVariantOption {
        variant: VariantId,
        option: String,
    },
    /// Renomme une option d'une variante (schéma 6) : réécrit l'option, son état actif et les
    /// entrées qui la citent. Refus sans trace : variante ou option inconnue, nom vide ou trop
    /// long, nom déjà pris par une autre option (casse ignorée). Même nom = aucune étape.
    RenameVariantOption {
        variant: VariantId,
        option: String,
        name: String,
    },
    /// Déplace une option à la position `to` (0 = première) dans la liste de la variante.
    /// Refus sans trace : variante ou option inconnue, position hors de la liste. Même position =
    /// aucune étape.
    MoveVariantOption {
        variant: VariantId,
        option: String,
        to: usize,
    },
    /// Active ou désactive une option dans la variante (même état = aucune étape).
    SetVariantOptionActive {
        variant: VariantId,
        option: String,
        active: bool,
    },
    /// Remplace les paramètres d'un plan de coupe (mêmes refus ; mêmes valeurs = aucune étape ;
    /// les placements ne changent pas).
    UpdateCuttingPlan {
        plan: CuttingPlanId,
        spec: crate::cutting_plan::CuttingPlanSpec,
    },
    /// Supprime un plan de coupe avec ses placements.
    RemoveCuttingPlan {
        plan: CuttingPlanId,
    },
    /// Pose une instance d'une pièce dans un plan (une pièce peut l'être plusieurs fois).
    /// `size` absent = taille de base ; rotation finie (±10⁶ °) ramenée dans [0 ; 360[ ; position
    /// finie, |v| ≤ 10⁶ mm. Refus sans trace : plan, pièce ou taille inconnus, valeur invalide.
    /// Identifiant dans `Outcome::placements`.
    AddPlacement {
        plan: CuttingPlanId,
        piece: PieceId,
        size: Option<SizeId>,
        x_mm: f64,
        y_mm: f64,
        rotation_deg: f64,
        mirrored: bool,
    },
    /// Remplace taille, position, rotation et miroir d'un placement (la pièce ne change pas ;
    /// mêmes valeurs après normalisation = aucune étape).
    UpdatePlacement {
        plan: CuttingPlanId,
        placement: PlacementId,
        size: Option<SizeId>,
        x_mm: f64,
        y_mm: f64,
        rotation_deg: f64,
        mirrored: bool,
    },
    /// Retire un placement d'un plan.
    RemovePlacement {
        plan: CuttingPlanId,
        placement: PlacementId,
    },
    /// Cuir (US-15.5) : définit ou remplace le contour de la peau d'un plan (polygone fermé
    /// implicitement, mm ; défauts et zones conservés). Refus sans trace : polygone non simple,
    /// hors bornes, défaut existant qui ne tient plus dans le contour.
    SetHideContour {
        plan: CuttingPlanId,
        contour: Vec<crate::PlanePoint>,
    },
    /// Retire la peau d'un plan avec ses défauts et ses zones (refus : pas de peau).
    RemoveHide {
        plan: CuttingPlanId,
    },
    /// Ajoute un défaut à la peau (contenu dans le contour, ne recoupe pas un autre défaut ;
    /// note blanche = sans note). Identifiant dans `Outcome::hide_defects`.
    AddHideDefect {
        plan: CuttingPlanId,
        kind: crate::DefectKind,
        polygon: Vec<crate::PlanePoint>,
        note: Option<String>,
    },
    /// Remplace genre, polygone et note d'un défaut (mêmes refus).
    UpdateHideDefect {
        plan: CuttingPlanId,
        defect: HideDefectId,
        kind: crate::DefectKind,
        polygon: Vec<crate::PlanePoint>,
        note: Option<String>,
    },
    /// Retire un défaut.
    RemoveHideDefect {
        plan: CuttingPlanId,
        defect: HideDefectId,
    },
    /// Ajoute une zone de qualité (`grade` 1 à 3). Identifiant dans `Outcome::quality_zones`.
    AddQualityZone {
        plan: CuttingPlanId,
        grade: u8,
        polygon: Vec<crate::PlanePoint>,
    },
    /// Remplace grade et polygone d'une zone de qualité.
    UpdateQualityZone {
        plan: CuttingPlanId,
        zone: QualityZoneId,
        grade: u8,
        polygon: Vec<crate::PlanePoint>,
    },
    /// Retire une zone de qualité.
    RemoveQualityZone {
        plan: CuttingPlanId,
        zone: QualityZoneId,
    },
    /// Crée une gamme de tailles (US-09.1) : `labels` dans l'ordre, `base` = rang (depuis 0) de
    /// la taille de base. Refus sans trace : nom ou libellé vide / trop long / en double (casse
    /// ignorée), aucune taille, base hors de la liste, limites (20 gammes, 60 tailles).
    AddSizeRange {
        name: String,
        kind: crate::size::SizeKind,
        labels: Vec<String>,
        base: usize,
    },
    /// Renomme une gamme (sans effet si le nom est inchangé).
    RenameSizeRange {
        range: SizeRangeId,
        name: String,
    },
    /// Supprime une gamme et ses tailles (refusé si un écart de gradation en dépend : aucun
    /// n'existe encore).
    RemoveSizeRange {
        range: SizeRangeId,
    },
    /// Ajoute une taille à `range` au rang `at` (`None` = à la fin). Ne modifie aucun autre objet.
    AddSize {
        range: SizeRangeId,
        label: String,
        at: Option<usize>,
    },
    /// Retire une taille. Refusé pour la taille de base (désigner une autre base d'abord), pour
    /// la dernière taille d'une gamme et si un écart de gradation en dépend (aucun n'existe
    /// encore). Ne modifie aucun autre objet.
    RemoveSize {
        size: SizeId,
    },
    RenameSize {
        size: SizeId,
        label: String,
    },
    /// Déplace une taille au rang `to` (depuis 0) de sa gamme ; l'ordre des autres est conservé.
    MoveSize {
        size: SizeId,
        to: usize,
    },
    /// Marque ou démarque une taille de rupture.
    SetSizeBreak {
        size: SizeId,
        break_size: bool,
    },
    /// Désigne la taille de base de sa gamme (aucun écart n'existe encore : la géométrie de base
    /// ne change pas).
    SetBaseSize {
        size: SizeId,
    },
    /// Pose, modifie ou retire (`dx_mm` et `dy_mm` nuls) l'écart de gradation d'un point **libre**
    /// pour une taille (US-09.2a) : position en cette taille = position écrite + écart cumulé,
    /// repère de la feuille. Sans effet (aucune étape) si rien ne change. Refus sans trace :
    /// feuille, point ou taille inconnus, taille de base (écart nul implicite), valeur non finie
    /// ou au-delà de ±1 000 000 mm, point construit, sur un calque verrouillé, suivi par une
    /// construction maintenue ou ajusté par une longueur imposée, position graduée hors de
    /// l'espace de travail. Un écart nul se pose toujours (c'est ainsi qu'on lève un conflit).
    SetPointGrading {
        sheet: SheetId,
        point: PointId,
        size: SizeId,
        dx_mm: f64,
        dy_mm: f64,
    },
    /// Retire tous les écarts d'un point (US-09.2a) ; sans effet s'il n'en a pas.
    ClearPointGrading {
        sheet: SheetId,
        point: PointId,
    },
    /// Reporte la règle de gradation d'un point libre vers d'autres points libres de la même
    /// feuille (US-09.3a) : pour chaque taille, les écarts de `source` (transformés selon `mode`)
    /// remplacent **tous** ceux des cibles. Atomique : un seul refus annule tout (refus de
    /// `SetPointGrading` pour chaque cible, **même si les écarts produits sont nuls**, plus liste
    /// vide, doublon, cible = source). Source seulement lue. Sans effet (aucune étape) si les
    /// cibles ont déjà ces écarts.
    CopyPointGrading {
        sheet: SheetId,
        source: PointId,
        targets: Vec<PointId>,
        mode: GradingCopyMode,
    },
    /// Gradation proportionnelle (US-09.3a) : les points `points` reçoivent, pour chaque taille,
    /// l'écart interpolé entre ceux de `from` et de `to` (t = projection orthogonale de la
    /// position de base du point sur le segment `from → to`, bornée à [0 ; 1]). Remplace tous les
    /// écarts des points. Refusée si `from` et `to` ont la même position de base.
    InterpolatePointGrading {
        sheet: SheetId,
        from: PointId,
        to: PointId,
        points: Vec<PointId>,
    },
    /// Lie (`Some`) ou délie (`None`) un paramètre numérique d'un point construit (US-03.1b :
    /// distance et angle d'un point en direction, dx et dy d'un point décalé) à une formule qui
    /// cite des variables. Lier écrit la valeur calculée ; délier garde la valeur courante en
    /// nombre figé et n'est pas une étape si rien n'était lié. Toute commande qui change une
    /// variable réévalue ensuite les formules dans la même étape. Refus sans trace : cible
    /// inconnue ou sans ce paramètre, formule vide, invalide, variable inconnue, valeur non
    /// finie, géométrie dégénérée.
    SetParameterExpr {
        target: crate::formula::ParamTarget,
        expr: Option<String>,
    },
    /// Lie (`kept`) ou délie la construction d'un segment créé par longueur et angle,
    /// perpendiculaire ou parallèle (US-03.2) : lié, déplacer la référence (point de départ ou
    /// segment de référence) déplace le ou les points construits dans la même étape ; délié, les
    /// positions courantes restent. Sans effet (aucune étape) si l'état demandé est déjà le bon.
    /// Refus sans trace : segment inconnu ou sans construction mémorisée, référence supprimée,
    /// segment qui ne correspond plus à sa construction (écart > 1e-6 mm), point déjà suivi par une
    /// autre construction, point d'arc ou point construit, cycle, calque verrouillé.
    SetConstructionKept {
        sheet: SheetId,
        segment: EntityId,
        kept: bool,
    },
    /// Pose, modifie (`Some`) ou retire (`None`) la **longueur développée imposée** d'une courbe de
    /// Bézier (US-03.3) : la longueur cible et l'inconnue (longueur d'une poignée, ou extrémité libre
    /// déplacée le long d'une direction). La contrainte est résolue à 1e-6 mm dans la même étape et
    /// re-résolue quand ses parents bougent. Retirer garde la géométrie courante ; sans effet (aucune
    /// étape) si rien ne change. Refus sans trace : courbe inconnue ou d'un autre type, cible non
    /// positive, point qui n'est pas une extrémité, extrémité non libre / d'arc / déjà suivie ou
    /// déplacée, cible hors de la plage atteignable (message avec les bornes), plusieurs solutions,
    /// poignée nulle, calque verrouillé.
    SetCurveLengthConstraint {
        sheet: SheetId,
        curve: EntityId,
        spec: Option<crate::length_constraint::LengthSpec>,
    },
    /// Lie (`Some`) la cible d'une longueur imposée **existante** à une formule sur les variables
    /// (US-03.3b), ou la délie (`None` : la valeur courante est gardée comme nombre). La valeur
    /// évaluée est écrite et résolue dans la même étape ; changer une variable ré-évalue et
    /// re-résout. Refus sans trace : aucune longueur imposée sur la courbe, cible déjà égale à un
    /// autre tracé, formule vide / non évaluable / valeur non positive, cible hors plage. Délier
    /// sans formule : aucune étape.
    SetCurveLengthExpr {
        sheet: SheetId,
        curve: EntityId,
        expr: Option<String>,
    },
    /// Fixe (`Some`) la cible d'une longueur imposée **existante** à la **somme** des longueurs de
    /// plusieurs tracés, de la même feuille ou d'**autres feuilles**, plus un décalage en nombre ou
    /// maintenu par une **formule** (US-03.4b) ; ou la délie (`None` : la valeur courante est gardée
    /// comme nombre). Remplace une formule de cible. La valeur calculée est écrite et résolue dans
    /// la même étape, dans l'ordre global des dépendances. Refus sans trace : aucune longueur imposée
    /// sur la courbe, aucun tracé, tracé ou feuille inconnus, tracé cité deux fois, auto-référence,
    /// tracé non mesurable (dégénéré), cycle (même entre deux feuilles), formule vide / non
    /// évaluable, cible <= 0 ou hors plage. Sans changement : aucune étape.
    SetCurveLengthEquality {
        sheet: SheetId,
        curve: EntityId,
        equality: Option<crate::length_constraint::LengthEquality>,
    },
    /// Transfère une pince vers un autre bord (US-07.2) : la pièce est **reconstruite** sur une
    /// nouvelle feuille (même identifiant, construction d'origine intacte), la partie du contour
    /// entre la base d'origine et la cible tourne autour du sommet de `part` fois l'angle
    /// d'ouverture. Cible = point du tracé `edge` (bord du contour) à `distance_mm` de son
    /// début. `part` ∈ ]0 ; 1] : 1 = transfert total (la pince d'origine disparaît), moins =
    /// partiel (une pince réduite reste, la pince transférée reprend l'identifiant `dart`, la
    /// restante en reçoit un : `Outcome::darts` = [transférée, restante]). Somme des angles et
    /// longueurs hors pince conservées. Refus sans trace : pièce, pince ou tracé inconnus,
    /// dérivée liée, autres pinces sur la pièce, part ou distance hors bornes, cible sur la base
    /// ou le sommet, pince non calculable, contour ou pince obtenus invalides.
    TransferPieceDart {
        piece: PieceId,
        dart: DartId,
        edge: EntityId,
        distance_mm: f64,
        part: f64,
    },
    /// Pose `count` plis (US-07.3) de même profondeur sur une pièce : la pièce est **reconstruite**
    /// sur une nouvelle feuille (même identifiant, construction intacte) et son contour est
    /// **écarté** le long de chaque ligne de pli de la largeur du pli (plat : 2 × profondeur ;
    /// creux : 4 × profondeur). La ligne `from → to` est la droite du premier pli ; `sense` =
    /// côté où s'étend la bande ; les `count` plis sont répartis à pas égal sur `span_mm` du
    /// côté du sens (ignoré si `count` = 1). Identifiants dans `Outcome::pleats`. Refus sans
    /// trace : pièce inconnue, dérivée liée ou portant des pinces, ligne qui ne traverse pas le
    /// contour en exactement deux points, profondeur, nombre ou étendue hors bornes, plis qui
    /// se chevauchent ou se croisent, contour obtenu invalide.
    AddPiecePleats {
        piece: PieceId,
        from: pf_geometry::PointMm,
        to: pf_geometry::PointMm,
        depth_mm: f64,
        kind: crate::piece_pleat::PleatKind,
        sense: pf_geometry::Side,
        count: u32,
        span_mm: f64,
    },
    /// Ampleur par ouverture en éventail (US-07.4) : la pièce est coupée selon `lines`
    /// (perpendiculaires au bord fixe `fixed_from → fixed_to`, un côté droit du contour ; `at_mm` =
    /// position depuis `fixed_from`) et chaque morceau tourne autour du pivot de sa ligne, sur le
    /// bord fixe, pour écarter le bord opposé de `opening_mm` (corde en haut). Le bord fixe garde
    /// sa longueur. La pièce est **reconstruite** sur une nouvelle feuille (même identifiant,
    /// construction intacte) ; identifiants des lignes dans `Outcome::flares`. Refus sans trace :
    /// pièce inconnue, dérivée liée, portant des pinces, des plis ou déjà une ampleur, bord fixe
    /// qui n'est pas un côté droit, nombre de lignes hors de 1 à 50, position hors du bord ou
    /// doublée, ouverture non finie ou hors bornes, ligne qui ne traverse pas le contour en deux
    /// points, morceaux qui se chevaucheraient.
    AddPieceFlare {
        piece: PieceId,
        fixed_from: pf_geometry::PointMm,
        fixed_to: pf_geometry::PointMm,
        lines: Vec<crate::piece_flare::FlareLineSpec>,
    },
    /// Ajoute à une pièce un groupe de boutons ou de boutonnières (US-06.4) réparti
    /// régulièrement entre `from` et `to` ; l'identifiant créé est dans
    /// `Outcome::button_groups`. `count` de 1 à `BUTTON_MAX_COUNT` ; l'angle libre d'une
    /// boutonnière est ramené dans [0 ; 360[ ; `edge_margin_mm` ≥ 0 est un contrôle
    /// affiché, jamais une contrainte. Refus sans trace : pièce inconnue ou sans feuille,
    /// point absent ou d'une autre feuille, points confondus ou identiques, nombre,
    /// diamètre, longueur, largeur ou recul non finis ou hors bornes, largeur > longueur.
    AddButtonGroup {
        piece: PieceId,
        from: PointId,
        to: PointId,
        count: u32,
        element: crate::piece_buttons::ButtonElement,
        edge_margin_mm: f64,
    },
    /// Remplace points, nombre, élément et recul d'un groupe (mêmes contrôles que
    /// l'ajout) ; sans effet (aucune étape) si rien ne change.
    SetButtonGroup {
        piece: PieceId,
        group: ButtonGroupId,
        from: PointId,
        to: PointId,
        count: u32,
        element: crate::piece_buttons::ButtonElement,
        edge_margin_mm: f64,
    },
    /// Retire un groupe ; la construction n'est pas touchée.
    RemoveButtonGroup {
        piece: PieceId,
        group: ButtonGroupId,
    },
    /// Crée une pièce dérivée de `mother` (US-04.4) : feuille dédiée, pièce liée et
    /// géométrie (décalage du contour de la mère), en une étape annulable. Le nom
    /// par défaut est « <mère> – <type> ». Refus sans trace : mère sans contour
    /// fermé lisible, distance hors bornes, décalage vide ou dégénéré, profondeur.
    CreateDerivedPiece {
        mother: PieceId,
        kind: crate::derived::DerivedKind,
        operation: crate::derived::DerivedOp,
        #[serde(default)]
        name: Option<String>,
    },
    /// Casse le lien d'une pièce dérivée : la géométrie reste, plus de mise à jour.
    BreakDerivedLink {
        piece: PieceId,
    },
    /// Déplie une pièce construite sur une pliure (US-04.7) : crée une pièce entière
    /// symétrique, **liée** à `piece` (dérivation `DerivedOp::Unfold`), sur une feuille
    /// dédiée, en une étape annulable. `fold` est un segment du contour de `piece`
    /// (éventuellement prolongé par les segments consécutifs alignés) ; la moitié est le
    /// reste du contour, la pliure est retirée. Le nom par défaut est « <pièce> – dépliée ».
    /// Refus sans trace : pièce inconnue ou dérivée liée, contour ouvert ou qui se recoupe,
    /// pliure absente du contour ou non rectiligne, contour qui traverse la droite de
    /// pliure ou la touche ailleurs qu'à ses extrémités, pliure déjà dépliée, résultat
    /// invalide. La construction n'est jamais modifiée.
    UnfoldPiece {
        piece: PieceId,
        fold: EntityId,
        #[serde(default)]
        name: Option<String>,
    },
    /// Crée une pièce depuis une région fermée ou une chaîne de tracés (US-04.1),
    /// en une seule étape annulable. **Liée** (défaut) : le contour référence des
    /// tracés de la feuille, que l'on coupe aux croisements s'il le faut (les
    /// tronçons restent des tracés réels, la pièce suit la construction).
    /// **Détachée** : le contour est copié dans une nouvelle feuille dédiée, la
    /// construction n'est pas touchée.
    ExtractPiece {
        sheet: SheetId,
        source: PieceSource,
        name: String,
        #[serde(default)]
        detached: bool,
    },
    /// Sépare une pièce par la **droite infinie** qui passe par `a` et `b` (US-04.6) :
    /// la pièce est remplacée, dans la même étape annulable, par deux pièces détachées
    /// fermées (« nom (1/2) » à gauche de `a → b`, « nom (2/2) » à droite), chacune sur sa
    /// propre feuille ; la construction n'est pas modifiée. Refus sans trace : pièce
    /// inconnue, dérivée liée, contour ouvert ou qui se recoupe, droite dégénérée, qui ne
    /// traverse pas le contour, qui le touche en un point, le longe ou le traverse en plus
    /// de deux points.
    SplitPiece {
        piece: PieceId,
        a: PointMm,
        b: PointMm,
    },
    /// Découpe une pièce selon une ligne interne (US-07.5) : comme `SplitPiece` (deux pièces
    /// détachées « (1/2) » à gauche de `P → Q` et « (2/2) » à droite, une feuille dédiée chacune),
    /// mais la ligne est un segment (`CutLine::Points`) ou un tracé de la feuille (segment, Bézier,
    /// arc), coupé aux deux traversées du contour ; `notches` pose des crans de repère de vrais
    /// crans, aux mêmes distances depuis le départ de la ligne, sur les deux bords de découpe
    /// (identifiants dans `Outcome::notches`). Recoller = `JoinPieces` par ces deux bords. Refus
    /// sans trace : pièce inconnue ou dérivée liée, contour ouvert ou qui se recoupe, ligne hors
    /// de la pièce, qui ne traverse pas le contour de bord à bord, qui le longe, le traverse en
    /// plus de deux points ou se recoupe, valeurs non finies ou hors bornes, tracé qui n'est pas
    /// un segment, une Bézier ou un arc.
    CutPiece {
        piece: PieceId,
        line: crate::piece_cut::CutLine,
        notches: crate::piece_cut::CutNotches,
    },
    /// Assemble deux pièces le long d'un bord de chacune (US-04.6) : la seconde est
    /// déplacée rigidement dans le repère de la première pour que les deux bords
    /// coïncident (bord de la seconde retourné), la jonction est retirée, les deux pièces
    /// sont remplacées par une pièce détachée, en une étape annulable. `tolerance_mm` borne
    /// l'écart de longueur **et** l'écart de forme des bords ; l'écart est rapporté dans
    /// les remarques. Refus sans trace : voir `Project::join_preview`.
    JoinPieces {
        first: PieceId,
        first_edge: EntityId,
        second: PieceId,
        second_edge: EntityId,
        tolerance_mm: f64,
    },
    AddPoint {
        sheet: SheetId,
        at: PointMm,
    },
    /// Crée un point (libre ou dérivé) ; ne fait rien si un point existe déjà là.
    AddPointAt {
        sheet: SheetId,
        point: PointRef,
    },
    /// Crée un point construit dont des paramètres sont **maintenus par une formule dès la
    /// création** (US-03.1, formule à la création) : chaque formule est évaluée avec les
    /// variables, sa valeur remplace celle de `definition`, le point est créé et les liens sont
    /// posés, en une seule étape. Refus sans trace : formule vide, invalide ou en double,
    /// paramètre absent de la définition, point déjà présent au même endroit (rien à lier).
    AddPointWithFormulas {
        sheet: SheetId,
        definition: PointDefinition,
        formulas: Vec<crate::formula::ParamExpr>,
    },
    AddSegment {
        sheet: SheetId,
        from: PointRef,
        to: PointRef,
    },
    /// Segment défini par un départ, une longueur et un angle de direction.
    AddSegmentPolar {
        sheet: SheetId,
        from: PointRef,
        length_mm: f64,
        angle_deg: f64,
    },
    /// Segment perpendiculaire à `reference`, partant de `from`.
    AddPerpendicular {
        sheet: SheetId,
        reference: EntityId,
        from: PointRef,
        length_mm: f64,
        side: Side,
    },
    /// Rectangle de quatre segments reliés. `width_mm` et `height_mm` sont
    /// signés (négatif = vers la gauche / le bas) mais jamais nuls.
    AddRectangle {
        sheet: SheetId,
        corner: PointRef,
        width_mm: f64,
        height_mm: f64,
    },
    /// Rectangle dont les constructions sont mémorisées (US-03.2b) : mêmes points et mêmes
    /// positions qu'`AddRectangle` (`width_mm` et `height_mm` signés, jamais nuls), mais le côté
    /// de départ est un segment longueur / angle et le côté opposé une parallèle à ce côté
    /// (voir `command/kept_rect.rs`). `kept` : les deux constructions sont **maintenues**
    /// (déplacer le coin déplace le reste) ; sinon elles sont seulement mémorisées et chaque
    /// côté peut être lié ensuite par `SetConstructionKept`. Une étape annulable ; refus sans
    /// trace (dimension nulle ou non finie, point fusionné qui ne peut pas être suivi…).
    AddConstructedRectangle {
        sheet: SheetId,
        corner: PointRef,
        width_mm: f64,
        height_mm: f64,
        kept: bool,
    },
    /// Polygone de construction rapide (PH-4a) : un point par sommet (fusionné avec un point
    /// existant au même endroit, comme `AddPoint`) et un segment par côté ; fermé = le dernier
    /// sommet est relié au premier. 2 à 200 sommets (3 à 200 si fermé). `role` et `layer`
    /// s'appliquent aux tracés et aux points **créés**. Une seule étape annulable ; refus sans
    /// trace : sommets non finis ou hors de l'espace de travail, consécutifs confondus
    /// (≤ 1e-6 mm), trop ou pas assez de sommets, côté déjà tracé, calque inconnu ou verrouillé.
    AddPolygon {
        sheet: SheetId,
        vertices: Vec<PointRef>,
        closed: bool,
        #[serde(default)]
        role: PathRole,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        layer: Option<LayerId>,
    },
    /// Rectangle orienté (PH-4a) : origine, largeur, hauteur (> 1e-6 mm), rotation autour de
    /// l'origine, option « centré » (voir [`pf_geometry::RectSpec`]) ; 4 points, 4 segments.
    /// Distinct d'`AddRectangle` (aligné sur les axes, signé), conservé tel quel.
    AddOrientedRectangle {
        sheet: SheetId,
        rect: pf_geometry::RectSpec,
        #[serde(default)]
        role: PathRole,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        layer: Option<LayerId>,
    },
    /// Rectangle à coins arrondis (PH-4a) : 4 segments et 4 arcs tangents, de vrais arcs du
    /// modèle (centre, départ, arrivée) ; rayon `0 < r ≤ min(w, h) / 2` (au maximum : un stade,
    /// deux demi-cercles). Les centres d'arcs (au plus 4) sont des points ordinaires ; avec
    /// `hide_centers`, ils sont rangés dans le calque masqué « Centres d'arcs » (créé au besoin,
    /// signalé dans les remarques) et ne sont ni dessinés ni sélectionnables tant qu'il l'est.
    AddRoundedRectangle {
        sheet: SheetId,
        rect: pf_geometry::RectSpec,
        radius_mm: f64,
        #[serde(default)]
        hide_centers: bool,
        #[serde(default)]
        role: PathRole,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        layer: Option<LayerId>,
    },
    /// Segment parallèle à `reference`, de même longueur, décalé de `distance_mm`.
    AddParallel {
        sheet: SheetId,
        reference: EntityId,
        distance_mm: f64,
        side: Side,
    },
    /// Cercle de centre et rayon donnés (US-02.1).
    AddCircle {
        sheet: SheetId,
        center: PointRef,
        radius_mm: f64,
    },
    /// Cercle passant par trois points (refusé s'ils sont alignés).
    AddCircleThreePoints {
        sheet: SheetId,
        a: PointRef,
        b: PointRef,
        c: PointRef,
    },
    /// Cercle de rayon donné passant par deux points ; le centre est du côté
    /// `side` du sens a → b (le choix se fait par proximité du clic dans l'éditeur).
    AddCircleTwoPointsRadius {
        sheet: SheetId,
        a: PointRef,
        b: PointRef,
        radius_mm: f64,
        side: Side,
    },
    /// Arc de centre, rayon et angles de début/fin donnés (degrés, `ccw` =
    /// sens anti-horaire, le défaut de l'interface).
    AddArc {
        sheet: SheetId,
        center: PointRef,
        radius_mm: f64,
        start_deg: f64,
        end_deg: f64,
        ccw: bool,
    },
    /// Arc passant par `start`, `through` et `end` (refusé s'ils sont alignés).
    AddArcThreePoints {
        sheet: SheetId,
        start: PointRef,
        through: PointRef,
        end: PointRef,
    },
    /// Arc de `start` à `end` de rayon donné ; le centre est du côté `side` de
    /// start → end, `ccw` choisit lequel des deux arcs (court/long) est tracé.
    AddArcTwoPointsRadius {
        sheet: SheetId,
        start: PointRef,
        end: PointRef,
        radius_mm: f64,
        side: Side,
        ccw: bool,
    },
    /// Change le rayon d'un cercle (paramètre de construction, pas une contrainte).
    SetCircleRadius {
        sheet: SheetId,
        entity: EntityId,
        radius_mm: f64,
    },
    /// Congé (raccord de rayon donné) entre deux segments, arcs ou cercles
    /// (US-02.3) : crée l'arc de raccord et ramène les extrémités des deux
    /// tracés aux points de tangence (un cercle n'est pas coupé). `pick_*` sont
    /// des points visés sur chaque tracé : ils désignent le côté du coin gardé.
    FilletPaths {
        sheet: SheetId,
        first: EntityId,
        second: EntityId,
        radius_mm: f64,
        pick_first: PointMm,
        pick_second: PointMm,
    },
    /// Chanfrein entre deux segments : coupe à `distance_*_mm` du coin et
    /// relie les deux points par un segment.
    ChamferPaths {
        sheet: SheetId,
        first: EntityId,
        second: EntityId,
        distance_first_mm: f64,
        distance_second_mm: f64,
        pick_first: PointMm,
        pick_second: PointMm,
    },
    /// Droite tangente à un cercle ou un arc, depuis un point : segment du
    /// point au point de tangence le plus proche de `pick`. Si le point est
    /// sur le cercle, `length_mm` donne la longueur de la tangente.
    AddTangentLine {
        sheet: SheetId,
        from: PointRef,
        target: EntityId,
        pick: PointMm,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        length_mm: Option<f64>,
    },
    /// Segment tangent à une courbe (Bézier, courbe lisse, arc, cercle) en un
    /// point, de longueur donnée, dans le sens du tracé (`backwards` : à
    /// l'envers). Construction statique : elle ne suit pas la courbe ensuite.
    AddTangentAt {
        sheet: SheetId,
        entity: EntityId,
        at: TangentAt,
        length_mm: f64,
        #[serde(default)]
        backwards: bool,
    },
    /// Transforme en place des points libres ; les points construits suivent
    /// leurs tracés, et les poignées des courbes dont les deux extrémités sont
    /// transformées suivent la rotation ou la symétrie.
    Transform {
        sheet: SheetId,
        points: Vec<PointId>,
        transform: Transform,
    },
    /// Copie des tracés (et de leurs extrémités) après transformation.
    Duplicate {
        sheet: SheetId,
        entities: Vec<EntityId>,
        transform: Transform,
    },
    AddPointOnSegment {
        sheet: SheetId,
        segment: EntityId,
        distance_mm: f64,
        origin: SegmentEnd,
    },
    /// Pose un cran sur un segment ou une courbe, à `distance_mm` de son départ.
    AddNotch {
        sheet: SheetId,
        entity: EntityId,
        distance_mm: f64,
        side: Side,
        length_mm: f64,
        style: NotchStyle,
    },
    /// Remplace les paramètres d'un cran (la position doit rester sur le tracé).
    SetNotch {
        sheet: SheetId,
        notch: NotchId,
        distance_mm: f64,
        side: Side,
        length_mm: f64,
        style: NotchStyle,
    },
    DeleteNotch {
        sheet: SheetId,
        notch: NotchId,
    },
    /// Pose un cran **ancré** (US-06.2) : sa position n'est pas une distance saisie mais
    /// se calcule d'après un point, le milieu ou une proportion du bord (voir
    /// [`NotchAnchor`]). L'identifiant créé est dans `Outcome::notches`. Refus sans
    /// trace : tracé ou point absent, proportion hors [0 ; 1] ou non finie, report qui
    /// sort du bord, longueur de cran hors bornes.
    AddAnchoredNotch {
        sheet: SheetId,
        entity: EntityId,
        anchor: NotchAnchor,
        side: Side,
        length_mm: f64,
        style: NotchStyle,
    },
    /// Change l'ancrage d'un cran. `Some` pose ou remplace l'ancrage (mêmes refus que
    /// l'ajout) ; `None` détache : le cran reste à sa position actuelle, désormais à
    /// distance fixe. Sans effet (aucune étape) si rien ne change.
    SetNotchAnchor {
        sheet: SheetId,
        notch: NotchId,
        anchor: Option<NotchAnchor>,
    },
    /// Règle la forme d'un cran (US-06.1) : forme (trait, T, V, U, fente), largeur (`None` =
    /// largeur par défaut ; refusée pour le trait) et « sur la coupe » (le cran est projeté sur
    /// le contour de coupe calculé, voir `Project::notch_on_cut`). Refus sans trace : largeur
    /// hors bornes, U plus large que le double de sa profondeur, « sur la coupe » pour un
    /// tracé qui n'est pas dans le contour d'une pièce de la feuille. Aucune étape si rien ne
    /// change.
    SetNotchShape {
        sheet: SheetId,
        notch: NotchId,
        form: NotchForm,
        width_mm: Option<f64>,
        on_cut: bool,
    },
    /// Fait d'un cran un **cran en coin** (US-06.1b) ou le remet ordinaire : posé au début ou à la
    /// fin de son tracé, dans le contour d'une pièce, il est orienté selon la bissectrice de
    /// l'angle du contour (voir `Project::notch_corner`). Un cran à distance fixe est ancré au
    /// bout de son tracé (proportion 0 ou 1) pour suivre le sommet ; le décocher laisse l'ancrage.
    /// Refus sans trace : cran qui n'est pas à
    /// un sommet identifiable (position, pièce, tracés adjacents, angle nul), ou déjà « sur la
    /// coupe » (combinaison reportée). Aucune étape si rien ne change.
    SetNotchCorner {
        sheet: SheetId,
        notch: NotchId,
        corner: bool,
    },
    /// Déclare (`true`) ou retire (`false`) un cran **proportionnel** (US-09.5c) : dans une taille
    /// gradée, il garde sa fraction de la longueur de son tracé au lieu d'une distance fixe
    /// (calculé à la lecture, jamais stocké ; la taille de base est inchangée). Aucune étape si
    /// rien ne change. Refus sans trace : cran ou feuille inconnus, cran ancré (l'ancrage est
    /// conservé : il suit déjà son bord).
    SetNotchProportional {
        sheet: SheetId,
        notch: NotchId,
        proportional: bool,
    },
    /// Ajoute une cotation (mesure affichée, non contrainte) entre deux points.
    AddDimension {
        sheet: SheetId,
        from: PointId,
        to: PointId,
        offset_mm: f64,
    },
    SetDimensionOffset {
        sheet: SheetId,
        dimension: DimensionId,
        offset_mm: f64,
    },
    DeleteDimension {
        sheet: SheetId,
        dimension: DimensionId,
    },
    /// Ajoute une cote complète (US-06.7) : longueur développée d'un tracé, angle, rayon,
    /// diamètre, projection horizontale ou verticale. **Mesure observée**, jamais une
    /// contrainte : la valeur est recalculée à chaque lecture. Refusée si un objet cité est
    /// absent, d'une autre feuille ou verrouillé, si la cible n'a pas le bon type (arc sans
    /// rayon…) ou si la géométrie est dégénérée (côtés d'angle superposés, extrémités ou
    /// points confondus), si la légende est trop longue ou contient un caractère de contrôle,
    /// ou si une valeur n'est pas finie.
    AddFullDimension {
        sheet: SheetId,
        kind: crate::full_dimension::FullDimensionKind,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        label: Option<String>,
        #[serde(default, skip_serializing_if = "is_zero_f64")]
        text_shift_mm: f64,
    },
    /// Change la mise en page d'une cote complète : décalage de la ligne (mm), rayon de l'arc
    /// d'angle (mm) ou direction du trait de rayon / de diamètre (degrés), selon son type.
    SetFullDimensionPlacement {
        sheet: SheetId,
        dimension: FullDimensionId,
        placement: f64,
    },
    /// Pose, remplace ou retire (`None` ou texte vide) la légende d'une cote complète.
    SetFullDimensionLabel {
        sheet: SheetId,
        dimension: FullDimensionId,
        label: Option<String>,
    },
    /// Déplace le texte d'une cote complète le long de sa ligne (mm, 0 = au milieu).
    SetFullDimensionTextShift {
        sheet: SheetId,
        dimension: FullDimensionId,
        text_shift_mm: f64,
    },
    DeleteFullDimension {
        sheet: SheetId,
        dimension: FullDimensionId,
    },
    /// Ajoute une ligne d'aide infinie (US-02.8) passant par `(x_mm, y_mm)`,
    /// dirigée par `angle_deg` (ramené dans `[0 ; 180[`). `axis` la marque
    /// comme axe de symétrie.
    AddGuide {
        sheet: SheetId,
        x_mm: f64,
        y_mm: f64,
        angle_deg: f64,
        #[serde(default)]
        axis: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        layer: Option<LayerId>,
    },
    /// Replace une ligne d'aide (point de passage et direction).
    MoveGuide {
        sheet: SheetId,
        guide: GuideId,
        x_mm: f64,
        y_mm: f64,
        angle_deg: f64,
    },
    /// Marque ou démarque une ligne d'aide comme axe de symétrie.
    SetGuideAxis {
        sheet: SheetId,
        guide: GuideId,
        axis: bool,
    },
    DeleteGuide {
        sheet: SheetId,
        guide: GuideId,
    },
    /// Décalage de contour (US-02.10) : crée un **nouveau** jeu de tracés
    /// parallèle à la chaîne `paths` (bout à bout, ouverte ou fermée), à la
    /// distance signée `distance_mm` (> 0 = à gauche du sens de la chaîne, voir
    /// `crate::offset`), avec le rôle `role`. Le résultat n'est pas lié à
    /// l'original et n'est pas recalculé s'il change. Les extrémités des
    /// tracés d'origine ne sont pas modifiées (un calque verrouillé peut donc
    /// être décalé) ; le résultat, lui, ne peut pas aller dans un calque verrouillé.
    OffsetPaths {
        sheet: SheetId,
        paths: Vec<EntityId>,
        distance_mm: f64,
        corner: pf_geometry::CornerStyle,
        role: PathRole,
    },
    /// Symétrie liée (US-02.11) : crée, pour les points `points` et les tracés
    /// `entities` (avec leurs extrémités), des points miroir **maintenus**
    /// (`PointDefinition::Mirror`) et des tracés miroir de sens inversé. Un
    /// point situé sur l'axe n'est pas dupliqué : il est partagé.
    LinkMirror {
        sheet: SheetId,
        points: Vec<PointId>,
        entities: Vec<EntityId>,
        axis: MirrorAxis,
    },
    /// Déplie la symétrie : les points miroir de cet axe deviennent libres ;
    /// les tracés qui se rejoignent sur l'axe forment alors un contour fermé.
    UnfoldMirror {
        sheet: SheetId,
        axis: MirrorAxis,
    },
    /// Plusieurs commandes appliquées comme une seule : tout ou rien, une
    /// seule étape d'historique.
    Batch {
        commands: Vec<Command>,
    },
    /// Courbe de Bézier ; `c1` et `c2` sont les poignées en coordonnées absolues.
    AddCurve {
        sheet: SheetId,
        start: PointRef,
        c1: PointMm,
        c2: PointMm,
        end: PointRef,
    },
    /// Courbe lisse par points de passage (US-02.2) : au moins deux points
    /// distincts, tangentes calculées (Catmull-Rom), tous points lisses.
    AddSpline {
        sheet: SheetId,
        points: Vec<PointRef>,
    },
    /// Insère un point de passage à la position `index` (0 = avant le premier,
    /// nombre de points = après le dernier).
    AddSplinePoint {
        sheet: SheetId,
        spline: EntityId,
        index: usize,
        point: PointRef,
    },
    /// Retire le point de passage `index` du tracé (le point reste sur la
    /// feuille) ; refusé s'il ne restait que deux points.
    RemoveSplinePoint {
        sheet: SheetId,
        spline: EntityId,
        index: usize,
    },
    /// Remplace le réglage d'un point de passage : lisse ou anguleux, tangente
    /// sortante (et entrante en point lisse) et tangente entrante d'un point
    /// anguleux ; `None` = tangente calculée.
    SetSplineNode {
        sheet: SheetId,
        spline: EntityId,
        index: usize,
        corner: bool,
        tangent: Option<OffsetMm>,
        tangent_in: Option<OffsetMm>,
    },
    /// Remplace une courbe de Bézier par la courbe par points de passage à
    /// deux points de même forme exacte (même identifiant de tracé).
    CurveToSpline {
        sheet: SheetId,
        curve: EntityId,
    },
    /// Remplace une courbe à exactement deux points de passage par la Bézier
    /// cubique de même forme exacte (même identifiant de tracé).
    SplineToCurve {
        sheet: SheetId,
        spline: EntityId,
    },
    MovePoint {
        sheet: SheetId,
        point: PointId,
        to: PointMm,
    },
    SetPointDistance {
        sheet: SheetId,
        point: PointId,
        distance_mm: f64,
    },
    /// Proportion d'un point sur segment, ou paramètre d'un point sur courbe (0 à 1).
    SetPointParameter {
        sheet: SheetId,
        point: PointId,
        value: f64,
    },
    /// Paramètres numériques d'un point relatif (US-02.5), dans l'ordre de sa
    /// définition : décalage `[dx, dy]` ; distance le long d'un tracé `[d]` ;
    /// bissectrice `[d]` ; direction `[distance, angle]` ; compas `[rA, rB]`.
    SetPointValues {
        sheet: SheetId,
        point: PointId,
        values: Vec<f64>,
    },
    /// Divise un tracé (segment, Bézier, courbe lisse, arc, cercle) en parts
    /// égales ou proportionnelles de **longueur d'arc** (US-02.6) : crée
    /// `parts - 1` points construits maintenus (`PointDefinition::Division`), en
    /// une seule étape. `start` (cercle seulement) : point qui fixe l'origine des
    /// angles. Le tracé lui-même n'est pas découpé (US-02.7).
    DivideTrace {
        sheet: SheetId,
        trace: EntityId,
        spec: DivisionSpec,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        start: Option<PointId>,
    },
    /// Prolonge (`delta_mm` > 0) ou raccourcit (`delta_mm` < 0) un segment ou
    /// un arc par l'extrémité `end` (US-02.7). Les courbes sont refusées.
    ExtendPath {
        sheet: SheetId,
        entity: EntityId,
        end: SegmentEnd,
        delta_mm: f64,
    },
    /// Prolonge ou raccourcit un segment ou un arc par l'extrémité `end`
    /// jusqu'à son intersection avec `target` (le support, droite ou cercle,
    /// est prolongé) ; `pick` choisit entre plusieurs solutions (US-02.7).
    TrimPathTo {
        sheet: SheetId,
        entity: EntityId,
        end: SegmentEnd,
        target: EntityId,
        pick: PointMm,
    },
    /// Coupe un tracé en un point (US-02.7) : le tracé garde le premier
    /// tronçon, le second est créé (mêmes calque, groupe, rôle ; crans répartis).
    SplitPath {
        sheet: SheetId,
        entity: EntityId,
        at: PointMm,
    },
    /// Fusionne deux tracés adjacents de même type compatible (US-02.7) ; `first` survit.
    MergePaths {
        sheet: SheetId,
        first: EntityId,
        second: EntityId,
    },
    /// Inverse le sens d'un tracé sans changer sa forme (US-02.7).
    ReversePath {
        sheet: SheetId,
        entity: EntityId,
    },
    MoveCurveHandle {
        sheet: SheetId,
        curve: EntityId,
        handle: CurveHandle,
        to: PointMm,
    },
    SetPointLabel {
        sheet: SheetId,
        point: PointId,
        label: String,
    },
    SetPointRole {
        sheet: SheetId,
        point: PointId,
        role: PointRole,
    },
    SetPathRole {
        sheet: SheetId,
        entity: EntityId,
        role: PathRole,
    },
    /// Crée une surpiqûre liée à un bord (US-06.8) : une ligne de rôle surpiqûre générée par
    /// décalage de `source` à `distance_mm` (> 0), du côté `side`, recalculée quand le bord
    /// change. Refusée si le décalage est dégénéré.
    AddTopstitch {
        sheet: SheetId,
        source: crate::topstitch::TopstitchSource,
        distance_mm: f64,
        side: crate::topstitch::TopstitchSide,
    },
    /// Change la distance et le côté d'une surpiqûre liée ; refusée si le nouveau décalage est
    /// dégénéré (rien ne change).
    SetTopstitch {
        sheet: SheetId,
        topstitch: TopstitchId,
        distance_mm: f64,
        side: crate::topstitch::TopstitchSide,
    },
    /// Casse le lien d'une surpiqûre : ses lignes deviennent des tracés libres.
    BreakTopstitchLink {
        sheet: SheetId,
        topstitch: TopstitchId,
    },
    Delete {
        sheet: SheetId,
        points: Vec<PointId>,
        entities: Vec<EntityId>,
    },
    /// Crée un calque (au-dessus des autres, visible, déverrouillé).
    AddLayer {
        sheet: SheetId,
        name: String,
    },
    RenameLayer {
        sheet: SheetId,
        layer: LayerId,
        name: String,
    },
    SetLayerVisible {
        sheet: SheetId,
        layer: LayerId,
        visible: bool,
    },
    SetLayerLocked {
        sheet: SheetId,
        layer: LayerId,
        locked: bool,
    },
    /// Déplace le calque à l'index donné dans la liste (ordre d'empilement).
    MoveLayer {
        sheet: SheetId,
        layer: LayerId,
        new_index: usize,
    },
    /// Supprime un calque. Refusé s'il contient des objets, sauf si `move_to`
    /// désigne le calque qui les reçoit.
    DeleteLayer {
        sheet: SheetId,
        layer: LayerId,
        move_to: Option<LayerId>,
    },
    /// Range des points et des tracés dans un calque (`None` = calque par défaut).
    SetObjectsLayer {
        sheet: SheetId,
        points: Vec<PointId>,
        entities: Vec<EntityId>,
        layer: Option<LayerId>,
    },
    /// Nomme un tracé (`None` ou texte vide : retire le nom). Le nom est
    /// unique dans la feuille.
    SetEntityName {
        sheet: SheetId,
        entity: EntityId,
        name: Option<String>,
    },
    /// Groupe des tracés (au moins deux, aucun déjà groupé).
    CreateGroup {
        sheet: SheetId,
        entities: Vec<EntityId>,
    },
    /// Dissout un groupe ; ses tracés restent en place.
    Ungroup {
        sheet: SheetId,
        group: GroupId,
    },
    /// Colle un fragment du presse-papiers (US-01.6) : identifiants régénérés,
    /// objets rangés dans `layer`, refusé si ce calque est verrouillé.
    PasteFragment {
        sheet: SheetId,
        fragment: Box<crate::clipboard::Fragment>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        layer: Option<LayerId>,
    },
    /// Insère une entrée de bibliothèque (US-11.4) : une seule étape. Pièce ou modèle = nouvelles
    /// feuilles ; bloc = dans `sheet` (calque `layer`). Nouveaux identifiants, copie sans lien.
    /// Variables : réutilisées si identiques, refus nommé si une du même nom a une autre valeur,
    /// sauf `rename_variables` (insérées sous un nom libre `nom_2`…).
    InsertLibrary {
        content: Box<crate::library::LibraryContent>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sheet: Option<SheetId>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        layer: Option<LayerId>,
        #[serde(default)]
        rename_variables: bool,
    },
    /// Importe de la géométrie de construction (US-12.3) : points, segments, cercles, arcs et
    /// courbes ordinaires, fusionnés avec les points existants, rangés dans des calques
    /// nommés (créés au besoin). Une seule étape annulable ; voir `import_geometry`.
    ImportGeometry {
        sheet: SheetId,
        objects: Vec<crate::import_geometry::ImportObject>,
    },
    /// Comme `ImportGeometry` dans une feuille créée par la commande (US-12.4) : une seule étape.
    ImportGeometryNewSheet {
        name: String,
        objects: Vec<crate::import_geometry::ImportObject>,
    },
    /// Importe des pièces d'un DXF de type AAMA / ASTM (US-12.2) : une pièce détachée par entrée
    /// (feuille dédiée, contour, crans, droit-fil), en une seule étape annulable ; refus sans
    /// trace au moindre défaut. Voir `import_aama`.
    ImportAamaPieces {
        pieces: Vec<crate::import_aama::AamaPiece>,
    },
    /// Applique la commande, puis range dans `layer` les points et tracés
    /// qu'elle a créés (calque actif de l'éditeur).
    InLayer {
        sheet: SheetId,
        layer: LayerId,
        command: Box<Command>,
    },
}

/// Ce qu'une commande a créé, plus d'éventuelles remarques à afficher.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Outcome {
    pub sheet: Option<SheetId>,
    pub points: Vec<PointId>,
    pub entities: Vec<EntityId>,
    #[serde(default)]
    pub notches: Vec<NotchId>,
    #[serde(default)]
    pub dimensions: Vec<DimensionId>,
    /// Cotes complètes créées (US-06.7).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub full_dimensions: Vec<FullDimensionId>,
    #[serde(default)]
    pub pieces: Vec<PieceId>,
    #[serde(default)]
    pub layers: Vec<LayerId>,
    #[serde(default)]
    pub groups: Vec<GroupId>,
    #[serde(default)]
    pub guides: Vec<GuideId>,
    /// Marquages de pièce créés (US-06.3).
    #[serde(default)]
    pub markings: Vec<MarkingId>,
    /// Pinces posées (US-07.1).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub darts: Vec<DartId>,
    /// Couples de coutures appariées déclarés (US-08.1).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub seam_pairs: Vec<SeamPairId>,
    /// Gammes de tailles créées (US-09.1).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub size_ranges: Vec<SizeRangeId>,
    /// Tailles créées (US-09.1), dans l'ordre de la gamme.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sizes: Vec<SizeId>,
    /// Mesures du barème créées (US-10.1).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub measurements: Vec<MeasurementId>,
    /// Fiches client créées (US-10.2).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub clients: Vec<ClientId>,
    /// Retouches créées (US-10.3).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub alterations: Vec<AlterationId>,
    /// Mesures finies créées (US-10.4).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub finished_measures: Vec<FinishedMeasureId>,
    /// Plans de coupe créés (socle d'E15).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cutting_plans: Vec<CuttingPlanId>,
    /// Variantes créées ou dupliquées (US-11.1).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub variants: Vec<VariantId>,
    /// Règles de nommage créées (US-11.5).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub naming_rules: Vec<NamingRuleId>,
    /// Placements créés dans un plan de coupe.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub placements: Vec<PlacementId>,
    /// Défauts de peau créés (US-15.5).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hide_defects: Vec<HideDefectId>,
    /// Zones de qualité créées (US-15.5).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub quality_zones: Vec<QualityZoneId>,
    /// Lignes de piqûre créées (US-06.9).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stitch_lines: Vec<StitchLineId>,
    /// Couples de lignes de piqûre créés (US-08.6).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stitch_pairs: Vec<StitchPairId>,
    /// Zones de parage, de collage ou de découpe créées (US-06.10).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub zones: Vec<ZoneId>,
    /// Plis posés (US-07.3), dans l'ordre d'application (le plus éloigné d'abord).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pleats: Vec<PleatId>,
    /// Lignes d'ampleur posées (US-07.4), par position croissante sur le bord fixe.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub flares: Vec<FlareId>,
    /// Groupes de boutons ou de boutonnières créés (US-06.4).
    #[serde(default)]
    pub button_groups: Vec<ButtonGroupId>,
    /// Pièces dérivées dont la géométrie a été recalculée par la commande (US-04.4).
    #[serde(default)]
    pub regenerated: Vec<PieceId>,
    /// Surpiqûres liées créées (US-06.8).
    #[serde(default)]
    pub topstitches: Vec<TopstitchId>,
    /// Surpiqûres liées dont les lignes ont été (re)calculées par la commande (US-06.8).
    #[serde(default)]
    pub regenerated_topstitches: Vec<TopstitchId>,
    pub notes: Vec<String>,
}

impl Project {
    /// Applique une commande de façon atomique.
    pub fn apply(&mut self, command: Command) -> Result<Outcome, CoreError> {
        self.apply_returning_previous(command)
            .map(|(outcome, _)| outcome)
    }

    /// Comme `apply`, et rend en plus le projet **d'avant** la commande : il est remplacé par le
    /// brouillon, donc l'historique l'obtient sans seconde copie. En cas de refus, `self` est
    /// intact.
    pub(crate) fn apply_returning_previous(
        &mut self,
        command: Command,
    ) -> Result<(Outcome, Project), CoreError> {
        let mut draft = self.clone();
        let unlinking = matches!(command, Command::SetConstructionKept { kept: false, .. });
        let mut outcome = draft.apply_unchecked(command)?;
        // Formules maintenues (US-03.1b) : réévaluées dans la même étape, avant tout ce qui lit la
        // géométrie ; une formule non évaluable refuse la commande entière.
        let moved = draft.sync_formulas(self, &mut outcome)?;
        // Constructions maintenues (US-03.2) : les points suivis sont recalculés dans la même
        // étape (après les formules, qui peuvent déplacer un parent) ; une construction devenue
        // impossible refuse la commande entière, un point suivi déplacé directement est délié.
        let kept_moved = draft.sync_kept(self, unlinking, &mut outcome)?;
        // Longueurs imposées (US-03.3) : après les constructions maintenues (un parent peut avoir
        // bougé), l'inconnue de chaque courbe est re-résolue dans la même étape ; hors plage =
        // commande refusée sans trace, message avec les bornes.
        let length_moved = draft.sync_length(self, &mut outcome)?;
        let moved: Vec<String> = moved
            .into_iter()
            .chain(kept_moved.iter().cloned())
            .chain(length_moved.iter().cloned())
            .collect();
        // Une géométrie rendue impossible par une formule nomme les paramètres recalculés.
        let named = |e: CoreError| -> CoreError {
            if moved.is_empty() {
                e
            } else {
                CoreError::Invalid(format!(
                    "{e} (paramètres maintenus recalculés par cette commande : {})",
                    moved.join(", ")
                ))
            }
        };
        // Un point ou un tracé cité par un cran ancré supprimé retire le cran (US-06.2) ;
        // avant les éléments internes, qui peuvent référencer ce cran.
        draft.prune_dead_notch_anchors(self, &mut outcome);
        // Écarts de gradation (US-09.2a) : un point supprimé ou devenu construit perd ses écarts.
        draft.prune_dead_gradings(&mut outcome);
        // Une cible supprimée retire ses éléments internes dans la même étape.
        draft.prune_dead_internals(self, &mut outcome);
        // Une cote complète (US-06.7) dont la cible disparaît est retirée avec remarque.
        draft.prune_dead_full_dimensions(&mut outcome);
        // Idem pour les marquages (US-06.3) dont un point disparaît.
        draft.prune_dead_markings(self, &mut outcome);
        // Idem pour les pinces (US-07.1).
        draft.prune_dead_darts(self, &mut outcome);
        // Idem pour les groupes de boutons (US-06.4).
        draft.prune_dead_buttons(self, &mut outcome);
        // Réglages de droit-fil (US-06.5) : référence supprimée = direction du tracé dessiné,
        // ligne supprimée = retirée, plus de droit-fil = réglages retirés (avec remarque).
        draft.prune_dead_grain(self, &mut outcome);
        // Marge de couture (US-05.1) : une surcharge dont le tracé a quitté le contour est retirée.
        draft.prune_dead_seam(self, &mut outcome);
        // Bords sur le pli (PH-5b) : un tracé supprimé retire la déclaration, avec remarque.
        draft.prune_dead_fold(self, &mut outcome);
        // Lignes de piqûre (US-06.9) : un bord qui n'est plus dans le contour retire la ligne.
        draft.prune_dead_stitch_lines(self, &mut outcome);
        draft.prune_dead_stitch_pairs(&mut outcome);
        // Zones (US-06.10) : un bord qui n'est plus dans le contour retire la bande.
        draft.prune_dead_zones(self, &mut outcome);
        // Une mère supprimée casse le lien de ses dérivées dans la même étape.
        draft.break_orphan_derivations(self, &mut outcome);
        // Le verrou prime sur la validation : la raison du refus est le calque.
        self.check_locks(&draft).map_err(|e| {
            match (&e, kept_moved.is_empty(), length_moved.is_empty()) {
                (CoreError::LayerLocked(m), false, _) => CoreError::LayerLocked(format!(
                    "{m} (points recalculés par une construction maintenue : {})",
                    kept_moved.join(", ")
                )),
                (CoreError::LayerLocked(m), true, false) => CoreError::LayerLocked(format!(
                    "{m} (recalculé par une longueur imposée : {})",
                    length_moved.join(", ")
                )),
                _ => e,
            }
        })?;
        // Les dérivées dont la mère a changé sont recalculées dans la même étape
        // (sans effet quand le projet n'a aucune pièce dérivée).
        draft.sync_derived(self, &mut outcome).map_err(named)?;
        // Surpiqûres liées (US-06.8) : un bord, une pièce ou une ligne supprimés cassent le
        // lien (avec remarque) ; puis création / recalcul des lignes générées dans la même
        // étape (sans effet quand aucune feuille n'a de surpiqûre liée).
        draft.break_dead_topstitches(self, &mut outcome);
        draft.sync_topstitches(self, &mut outcome).map_err(named)?;
        // Coutures appariées (US-08.1) : une pièce ou un tracé disparu du contour retire le
        // couple (avec remarque) ; après la régénération des dérivées, qui peut refaire un contour.
        draft.prune_dead_seam_pairs(self, &mut outcome);
        // Mesures finies (US-10.4) : une couture, une cote ou une feuille disparue les retire
        // (après les coutures et les cotes, dont la suppression est traitée plus haut).
        draft.prune_dead_finished_measures(&mut outcome);
        draft.validate().map_err(named)?;
        let previous = std::mem::replace(self, draft);
        Ok((outcome, previous))
    }

    pub(crate) fn resolve_ref(
        &mut self,
        sheet: SheetId,
        r: PointRef,
        out: &mut Outcome,
    ) -> Result<PointId, CoreError> {
        match r {
            PointRef::Existing(id) => {
                self.sheet(sheet)?.point(id)?;
                Ok(id)
            }
            PointRef::New(at) => {
                at.validate()?;
                self.create_point(sheet, PointDefinition::Free { x: at.x, y: at.y }, out)
            }
            PointRef::Derived(definition) => self.create_point(sheet, definition, out),
        }
    }

    /// Crée un point avec cette définition, ou réutilise un point existant
    /// situé exactement au même endroit (pas de doublon).
    fn create_point(
        &mut self,
        sheet: SheetId,
        definition: PointDefinition,
        out: &mut Outcome,
    ) -> Result<PointId, CoreError> {
        let s = self.sheet(sheet)?;
        // Chaque position existante n'est calculée qu'une fois pour la recherche du doublon.
        let existing = s.with_read_cache(|| {
            let at = s.evaluate(&definition)?;
            Ok::<_, CoreError>(s.points.iter().find_map(|p| {
                let pos = s.position(p.id).ok()?;
                (pf_geometry::distance_mm(pos, at) <= MERGE_TOLERANCE_MM).then_some(p.id)
            }))
        })?;
        if let Some(id) = existing {
            return Ok(id);
        }
        let id = PointId(self.alloc_id());
        let s = self.sheet_mut(sheet)?;
        let label = s.next_label();
        s.points.push(Point {
            id,
            label,
            definition,
            role: PointRole::Construction,
            layer: None,
        });
        out.points.push(id);
        Ok(id)
    }

    fn add_segment(
        &mut self,
        sheet: SheetId,
        from: PointId,
        to: PointId,
        construction: Option<Construction>,
        out: &mut Outcome,
    ) -> Result<(), CoreError> {
        if from == to {
            return Err(pf_geometry::GeometryError::ZeroLengthSegment.into());
        }
        let s = self.sheet(sheet)?;
        let duplicate = s.entities.iter().any(|e| {
            matches!(e.kind, EntityKind::Segment { from: f, to: t, .. }
                if (f == from && t == to) || (f == to && t == from))
        });
        if duplicate {
            return Err(CoreError::Invalid(format!(
                "un segment relie déjà {} et {}",
                s.label(from),
                s.label(to)
            )));
        }
        let id = EntityId(self.alloc_id());
        self.sheet_mut(sheet)?.entities.push(Entity {
            id,
            role: PathRole::Construction,
            layer: None,
            name: None,
            kind: EntityKind::Segment {
                from,
                to,
                construction,
            },
        });
        out.entities.push(id);
        Ok(())
    }

    fn add_circle_entity(
        &mut self,
        sheet: SheetId,
        center: PointId,
        radius_mm: f64,
        start_deg: f64,
        out: &mut Outcome,
    ) -> Result<(), CoreError> {
        let c = self.sheet(sheet)?.position(center)?;
        pf_geometry::CircleTrace::new(c, radius_mm, start_deg, true)?;
        let id = EntityId(self.alloc_id());
        self.sheet_mut(sheet)?.entities.push(Entity {
            id,
            role: PathRole::Construction,
            layer: None,
            name: None,
            kind: EntityKind::Circle {
                center,
                radius_mm,
                start_deg,
                ccw: true,
            },
        });
        out.entities.push(id);
        Ok(())
    }

    fn add_arc_entity(
        &mut self,
        sheet: SheetId,
        center: PointId,
        start: PointId,
        end: PointId,
        ccw: bool,
        out: &mut Outcome,
    ) -> Result<(), CoreError> {
        if center == start || center == end || start == end {
            return Err(CoreError::Invalid(
                "arc dégénéré : le centre et les deux extrémités doivent être trois points \
                 distincts"
                    .into(),
            ));
        }
        let s = self.sheet(sheet)?;
        pf_geometry::Arc::from_center_points(
            s.position(center)?,
            s.position(start)?,
            s.position(end)?,
            ccw,
        )?;
        let id = EntityId(self.alloc_id());
        self.sheet_mut(sheet)?.entities.push(Entity {
            id,
            role: PathRole::Construction,
            layer: None,
            name: None,
            kind: EntityKind::Arc {
                center,
                start,
                end,
                ccw,
            },
        });
        out.entities.push(id);
        Ok(())
    }

    /// Déplace un point libre. Les arcs qui l'utilisent suivent : déplacer le
    /// centre translate les extrémités de l'arc ; déplacer une extrémité change
    /// le rayon et ramène l'autre extrémité sur le nouveau cercle, le long du
    /// rayon. Les points à déplacer doivent être libres, sinon refus.
    fn move_point(&mut self, sheet: SheetId, point: PointId, to: PointMm) -> Result<(), CoreError> {
        let s = self.sheet(sheet)?;
        let old = s.position(point)?;
        let mut moves: Vec<(PointId, PointMm)> = vec![(point, to)];
        let push = |moves: &mut Vec<(PointId, PointMm)>,
                    id: PointId,
                    pos: PointMm|
         -> Result<(), CoreError> {
            match moves.iter().find(|(m, _)| *m == id) {
                Some((_, known)) if pf_geometry::distance_mm(*known, pos) > 1e-9 => {
                    Err(CoreError::Invalid(format!(
                        "le point {} est imposé à deux endroits par des arcs qui le partagent : \
                         déplacement refusé",
                        s.label(id)
                    )))
                }
                Some(_) => Ok(()),
                None => {
                    moves.push((id, pos));
                    Ok(())
                }
            }
        };
        for e in &s.entities {
            if let EntityKind::Arc {
                center, start, end, ..
            } = e.kind
            {
                if point == center {
                    let d = to - old;
                    for q in [start, end] {
                        push(&mut moves, q, (s.position(q)? + d).validate()?)?;
                    }
                } else if point == start || point == end {
                    let other = if point == start { end } else { start };
                    let c = s.position(center)?;
                    let r = pf_geometry::distance_mm(c, to);
                    pf_geometry::check_radius(r)?;
                    let u = (s.position(other)? - c).unit()?;
                    push(&mut moves, other, (c + u * r).validate()?)?;
                }
            }
        }
        let s = self.sheet_mut(sheet)?;
        for (id, pos) in moves {
            let label = s.label(id).to_string();
            let p = s
                .points
                .iter_mut()
                .find(|p| p.id == id)
                .ok_or(CoreError::UnknownPoint(id))?;
            match &mut p.definition {
                PointDefinition::Free { x, y } => {
                    *x = pos.x;
                    *y = pos.y;
                }
                PointDefinition::Mirror { .. } => {
                    return Err(CoreError::Invalid(format!(
                        "le point {label} est maintenu par symétrie : modifiez son original ou \
                         dépliez la symétrie"
                    )));
                }
                _ => {
                    return Err(CoreError::Invalid(format!(
                        "le point {label} est construit à partir d'autres tracés : il ne se \
                         déplace pas librement"
                    )));
                }
            }
        }
        Ok(())
    }

    fn spline_nodes_mut(
        &mut self,
        sheet: SheetId,
        spline: EntityId,
    ) -> Result<&mut Vec<SplineNode>, CoreError> {
        match &mut self.sheet_mut(sheet)?.entity_mut(spline)?.kind {
            EntityKind::Spline { nodes } => Ok(nodes),
            _ => Err(CoreError::Invalid(format!(
                "le tracé {} n'est pas une courbe par points de passage",
                spline.0
            ))),
        }
    }

    fn apply_unchecked(&mut self, command: Command) -> Result<Outcome, CoreError> {
        let mut out = Outcome::default();
        match command {
            Command::RenameProject { name } => self.name = name.trim().to_string(),
            Command::CreateSheet { name } => {
                let id = SheetId(self.alloc_id());
                self.sheets.push(Sheet::new(id, name.trim()));
                out.sheet = Some(id);
            }
            Command::RenameSheet { sheet, name } => {
                self.sheet_mut(sheet)?.name = name.trim().to_string();
            }
            Command::DeleteSheet { sheet } => {
                self.sheet(sheet)?;
                if self.sheets.len() == 1 {
                    return Err(CoreError::Invalid(
                        "impossible de supprimer la dernière feuille du projet".into(),
                    ));
                }
                self.sheets.retain(|s| s.id != sheet);
                let before = self.pieces.len();
                self.pieces
                    .retain(|p| p.link != PieceLink::Linked { sheet });
                if self.pieces.len() < before {
                    out.notes.push(format!(
                        "{} pièce(s) supprimée(s) avec leur feuille.",
                        before - self.pieces.len()
                    ));
                }
            }
            Command::MoveSheet { sheet, new_index } => {
                let from = self
                    .sheets
                    .iter()
                    .position(|s| s.id == sheet)
                    .ok_or(CoreError::UnknownSheet(sheet))?;
                let s = self.sheets.remove(from);
                let idx = new_index.min(self.sheets.len());
                self.sheets.insert(idx, s);
            }
            Command::DuplicateSheet { sheet } => {
                let index = self
                    .sheets
                    .iter()
                    .position(|s| s.id == sheet)
                    .ok_or(CoreError::UnknownSheet(sheet))?;
                let source = self.sheets[index].clone();
                let pieces: Vec<Piece> = self.pieces_of(sheet).cloned().collect();
                self.insert_sheet_copy(&source, &pieces, index + 1, true, &mut out)?;
            }
            Command::ImportSheet { sheet, pieces } => {
                let end = self.sheets.len();
                self.insert_sheet_copy(&sheet, &pieces, end, false, &mut out)?;
            }
            Command::AddPiece {
                sheet,
                name,
                quantity,
                material,
                mirrored_pair,
            } => {
                self.sheet(sheet)?;
                let id = PieceId(self.alloc_id());
                self.pieces.push(Piece {
                    id,
                    name: name.trim().to_string(),
                    quantity,
                    material: clean_material(material),
                    mirrored_pair,
                    outline: Vec::new(),
                    link: PieceLink::Linked { sheet },
                    ..Piece::default()
                });
                out.pieces.push(id);
            }
            Command::SetPieceInfo {
                piece,
                name,
                quantity,
                material,
                mirrored_pair,
            } => {
                let p = self.piece_mut(piece)?;
                p.name = name.trim().to_string();
                p.quantity = quantity;
                p.material = clean_material(material);
                p.mirrored_pair = mirrored_pair;
            }
            Command::SetPieceCard {
                piece,
                name,
                code,
                quantity,
                mirrored_pair,
                material,
                category,
                comment,
            } => {
                let p = self.piece_mut(piece)?;
                p.name = name.trim().to_string();
                p.code = clean_material(code);
                p.quantity = quantity;
                p.mirrored_pair = mirrored_pair;
                p.material = clean_material(material);
                p.category = category;
                // Fins de ligne Windows ramenées à « \n » ; un « \r » isolé reste refusé.
                p.comment = clean_material(comment.map(|c| c.replace("\r\n", "\n")));
            }
            Command::DeletePiece { piece } => {
                self.piece(piece)?;
                self.pieces.retain(|p| p.id != piece);
            }
            Command::SetPiecePlacement { piece, placement } => {
                let placement = placement.map(|p| p.normalized()).transpose()?;
                self.piece_mut(piece)?.placement = placement;
            }
            Command::SetPieceOutline {
                piece,
                outline,
                grainline,
            } => {
                let p = self.piece_mut(piece)?;
                p.outline = outline;
                p.grainline = grainline;
            }
            Command::SetPieceGrain { piece, grain } => self.set_piece_grain(piece, grain)?,
            Command::SetSeamAllowance { piece, seam } => self.set_seam_allowance(piece, seam)?,
            Command::SetNetCut { piece, net_cut } => self.set_net_cut(piece, net_cut)?,
            Command::SetSeamDefaults { defaults } => self.set_seam_defaults(defaults)?,
            Command::AddPieceInternal {
                piece,
                target,
                kind,
            } => self.add_piece_internal(piece, target, kind)?,
            Command::RemovePieceInternal { piece, target } => {
                self.remove_piece_internal(piece, target)?
            }
            Command::SetPieceInternalKind {
                piece,
                target,
                kind,
            } => self.set_piece_internal_kind(piece, target, kind)?,
            Command::ImportPieceInternals { piece } => self.import_piece_internals(piece)?,
            Command::AddPieceMarking { piece, at, shape } => {
                self.add_piece_marking(piece, at, shape, &mut out)?
            }
            Command::AddPieceMarkingRun { piece, run } => {
                self.add_piece_marking_run(piece, &run, &mut out)?
            }
            Command::AddStitchLine { piece, line } => {
                self.add_stitch_line(piece, line, &mut out)?
            }
            Command::PairStitchLines { a, b } => self.pair_stitch_lines(a, b, &mut out)?,
            Command::UnpairStitchLines { pair } => self.unpair_stitch_lines(pair)?,
            Command::CarryStitchLine {
                from,
                to_piece,
                to_edges,
            } => self.carry_stitch_line(from, to_piece, to_edges, &mut out)?,
            Command::SetStitchLine { piece, line, spec } => {
                self.set_stitch_line(piece, line, spec)?
            }
            Command::AddPieceZone { piece, zone } => self.add_piece_zone(piece, zone, &mut out)?,
            Command::SetPieceZone { piece, zone, spec } => {
                self.set_piece_zone(piece, zone, spec)?
            }
            Command::RemovePieceZone { piece, zone } => self.remove_piece_zone(piece, zone)?,
            Command::RemoveStitchLine { piece, line } => {
                self.remove_stitch_line(piece, line, &mut out)?
            }
            Command::MergeStitchLines {
                piece,
                first,
                second,
            } => self.merge_stitch_lines(piece, first, second)?,
            Command::RemovePieceMarking { piece, marking } => {
                self.remove_piece_marking(piece, marking)?
            }
            Command::SetPieceMarking {
                piece,
                marking,
                at,
                shape,
            } => self.set_piece_marking(piece, marking, at, shape)?,
            Command::AddPieceDart {
                piece,
                apex,
                toward,
                length_mm,
                opening_mm,
                fixed,
            } => {
                self.add_piece_dart(piece, apex, toward, length_mm, opening_mm, fixed, &mut out)?
            }
            Command::AddSeamPair {
                a,
                b,
                embu_mm,
                tolerance_mm,
            } => self.add_seam_pair(a, b, embu_mm, tolerance_mm, &mut out)?,
            Command::SetSeamPair {
                id,
                a,
                b,
                embu_mm,
                tolerance_mm,
            } => self.set_seam_pair(id, a, b, embu_mm, tolerance_mm)?,
            Command::RemoveSeamPair { id } => self.remove_seam_pair(id)?,
            Command::MarchNotches { pair, from, which } => {
                self.march_notches(pair, from, which, &mut out)?
            }
            Command::RemovePieceDart { piece, dart } => self.remove_piece_dart(piece, dart)?,
            Command::SetPieceDart {
                piece,
                dart,
                apex,
                toward,
                length_mm,
                opening_mm,
                fixed,
            } => self.set_piece_dart(piece, dart, apex, toward, length_mm, opening_mm, fixed)?,
            Command::SetPieceDartCutClosed {
                piece,
                dart,
                closed,
            } => self.set_piece_dart_cut_closed(piece, dart, closed)?,
            Command::SetPieceFold { piece, edge, on } => self.set_piece_fold(piece, edge, on)?,
            Command::SetPieceNoGrain { piece, no_grain } => {
                self.set_piece_no_grain(piece, no_grain)?
            }
            Command::SetBackground {
                sheet,
                image,
                sha256,
                width_px,
                height_px,
                calibration,
            } => self.set_background(sheet, image, sha256, width_px, height_px, calibration)?,
            Command::CalibrateBackground { sheet, calibration } => {
                self.calibrate_background(sheet, calibration)?
            }
            Command::SetBackgroundDisplay {
                sheet,
                opacity,
                visible,
                locked,
            } => self.set_background_display(sheet, opacity, visible, locked)?,
            Command::RelinkBackground {
                sheet,
                image,
                sha256,
                width_px,
                height_px,
            } => self.relink_background(sheet, image, sha256, width_px, height_px)?,
            Command::RemoveBackground { sheet } => self.remove_background(sheet)?,
            Command::AddVariable { name, unit, expr } => self.add_variable(name, unit, expr)?,
            Command::SetVariableExpr { name, expr } => self.set_variable_expr(&name, expr)?,
            Command::SetVariableUnit { name, unit } => self.set_variable_unit(&name, unit)?,
            Command::RenameVariable { from, to } => self.rename_variable(&from, &to)?,
            Command::RemoveVariable { name } => self.remove_variable(&name)?,
            Command::SetVariableSizeExpr { name, size, expr } => {
                self.set_variable_size_expr(&name, size, expr)?
            }
            Command::ClearVariableSizeExpr { name, size } => {
                self.clear_variable_size_expr(&name, size)?
            }
            Command::AddMeasurement { name, unit } => {
                self.add_measurement(&name, unit, &mut out)?
            }
            Command::RenameMeasurement { measurement, name } => {
                self.rename_measurement(measurement, &name)?
            }
            Command::RemoveMeasurement { measurement } => {
                self.remove_measurement(measurement, &mut out)?
            }
            Command::SetMeasurementValue {
                measurement,
                size,
                value,
            } => self.set_measurement_value(measurement, size, value)?,
            Command::ClearMeasurementValue { measurement, size } => {
                self.clear_measurement_value(measurement, size)?
            }
            Command::ImportMeasurements { rows, clear_empty } => {
                self.import_measurements(&rows, clear_empty, &mut out)?
            }
            Command::AddClient { name } => self.add_client(&name, &mut out)?,
            Command::RenameClient { client, name } => self.rename_client(client, &name)?,
            Command::RemoveClient { client } => self.remove_client(client)?,
            Command::SetClientValue {
                client,
                measurement,
                value,
            } => self.set_client_value(client, measurement, value)?,
            Command::ClearClientValue {
                client,
                measurement,
            } => self.clear_client_value(client, measurement)?,
            Command::GenerateClientSize {
                client,
                range,
                label,
                replace,
            } => self.generate_client_size(client, range, &label, replace, &mut out)?,
            Command::AddAlteration {
                name,
                target,
                delta,
            } => self.add_alteration(&name, target, delta, &mut out)?,
            Command::RenameAlteration { alteration, name } => {
                self.rename_alteration(alteration, &name)?
            }
            Command::RemoveAlteration { alteration } => self.remove_alteration(alteration)?,
            Command::SetAlterationTarget { alteration, target } => {
                self.set_alteration_target(alteration, target)?
            }
            Command::SetAlterationDelta { alteration, delta } => {
                self.set_alteration_delta(alteration, delta)?
            }
            Command::AddVariant { name } => self.add_variant(&name, &mut out)?,
            Command::AddNamingRule { spec } => self.add_naming_rule(&spec, &mut out)?,
            Command::UpdateNamingRule { rule, spec } => self.update_naming_rule(rule, &spec)?,
            Command::RemoveNamingRule { rule } => self.remove_naming_rule(rule)?,
            Command::ApplyNamingRule { rule } => self.apply_naming_rule(rule)?,
            Command::RenameVariant { variant, name } => self.rename_variant(variant, &name)?,
            Command::RemoveVariant { variant } => self.remove_variant(variant)?,
            Command::DuplicateVariant { variant, name } => {
                self.duplicate_variant(variant, &name, &mut out)?
            }
            Command::AddVariantEntry {
                variant,
                piece,
                quantity,
                material,
                option,
            } => self.add_variant_entry(variant, piece, quantity, &material, &option)?,
            Command::UpdateVariantEntry {
                variant,
                piece,
                option,
                quantity,
                material,
                new_option,
            } => self.update_variant_entry(
                variant,
                piece,
                &option,
                quantity,
                &material,
                &new_option,
            )?,
            Command::RemoveVariantEntry {
                variant,
                piece,
                option,
            } => self.remove_variant_entry(variant, piece, &option)?,
            Command::AddVariantOption { variant, option } => {
                self.add_variant_option(variant, &option)?
            }
            Command::RemoveVariantOption { variant, option } => {
                self.remove_variant_option(variant, &option)?
            }
            Command::RenameVariantOption {
                variant,
                option,
                name,
            } => self.rename_variant_option(variant, &option, &name)?,
            Command::MoveVariantOption {
                variant,
                option,
                to,
            } => self.move_variant_option(variant, &option, to)?,
            Command::SetVariantOptionActive {
                variant,
                option,
                active,
            } => self.set_variant_option_active(variant, &option, active)?,
            Command::AddCuttingPlan { spec } => self.add_cutting_plan(&spec, &mut out)?,
            Command::UpdateCuttingPlan { plan, spec } => self.update_cutting_plan(plan, &spec)?,
            Command::RemoveCuttingPlan { plan } => self.remove_cutting_plan(plan)?,
            Command::AddPlacement {
                plan,
                piece,
                size,
                x_mm,
                y_mm,
                rotation_deg,
                mirrored,
            } => self.add_placement(
                plan,
                crate::cutting_plan::CuttingPlacement {
                    id: PlacementId(0),
                    piece,
                    size,
                    x_mm,
                    y_mm,
                    rotation_deg,
                    mirrored,
                },
                &mut out,
            )?,
            Command::UpdatePlacement {
                plan,
                placement,
                size,
                x_mm,
                y_mm,
                rotation_deg,
                mirrored,
            } => self.update_placement(
                plan,
                crate::cutting_plan::CuttingPlacement {
                    id: placement,
                    piece: PieceId(0),
                    size,
                    x_mm,
                    y_mm,
                    rotation_deg,
                    mirrored,
                },
            )?,
            Command::RemovePlacement { plan, placement } => {
                self.remove_placement(plan, placement)?
            }
            Command::SetHideContour { plan, contour } => self.set_hide_contour(plan, &contour)?,
            Command::RemoveHide { plan } => self.remove_hide(plan)?,
            Command::AddHideDefect {
                plan,
                kind,
                polygon,
                note,
            } => self.add_hide_defect(plan, kind, &polygon, &note, &mut out)?,
            Command::UpdateHideDefect {
                plan,
                defect,
                kind,
                polygon,
                note,
            } => self.update_hide_defect(plan, defect, kind, &polygon, &note)?,
            Command::RemoveHideDefect { plan, defect } => self.remove_hide_defect(plan, defect)?,
            Command::AddQualityZone {
                plan,
                grade,
                polygon,
            } => self.add_quality_zone(plan, grade, &polygon, &mut out)?,
            Command::UpdateQualityZone {
                plan,
                zone,
                grade,
                polygon,
            } => self.update_quality_zone(plan, zone, grade, &polygon)?,
            Command::RemoveQualityZone { plan, zone } => self.remove_quality_zone(plan, zone)?,
            Command::AddFinishedMeasure {
                name,
                measurement,
                source,
                tolerance_minus,
                tolerance_plus,
            } => self.add_finished_measure(
                &name,
                measurement,
                source,
                tolerance_minus,
                tolerance_plus,
                &mut out,
            )?,
            Command::SetFinishedMeasure {
                finished,
                name,
                measurement,
                source,
                tolerance_minus,
                tolerance_plus,
            } => self.set_finished_measure(
                finished,
                &name,
                measurement,
                source,
                tolerance_minus,
                tolerance_plus,
            )?,
            Command::RemoveFinishedMeasure { finished } => {
                self.remove_finished_measure(finished)?
            }
            Command::SetNotchProportional {
                sheet,
                notch,
                proportional,
            } => self.set_notch_proportional(sheet, notch, proportional)?,
            c @ (Command::AddSizeRange { .. }
            | Command::RenameSizeRange { .. }
            | Command::RemoveSizeRange { .. }
            | Command::AddSize { .. }
            | Command::RemoveSize { .. }
            | Command::RenameSize { .. }
            | Command::MoveSize { .. }
            | Command::SetSizeBreak { .. }
            | Command::SetBaseSize { .. }) => self.apply_size_command(c, &mut out)?,
            c @ (Command::SetPointGrading { .. }
            | Command::ClearPointGrading { .. }
            | Command::CopyPointGrading { .. }
            | Command::InterpolatePointGrading { .. }) => {
                self.apply_grading_command(c, &mut out)?
            }
            Command::SetParameterExpr { target, expr } => self.set_parameter_expr(target, expr)?,
            Command::SetConstructionKept {
                sheet,
                segment,
                kept,
            } => self.set_construction_kept(sheet, segment, kept)?,
            Command::SetCurveLengthConstraint { sheet, curve, spec } => {
                self.set_curve_length_constraint(sheet, curve, spec)?
            }
            Command::SetCurveLengthExpr { sheet, curve, expr } => {
                self.set_curve_length_expr(sheet, curve, expr)?
            }
            Command::SetCurveLengthEquality {
                sheet,
                curve,
                equality,
            } => self.set_curve_length_equality(sheet, curve, equality)?,
            Command::TransferPieceDart {
                piece,
                dart,
                edge,
                distance_mm,
                part,
            } => self.transfer_piece_dart(piece, dart, edge, distance_mm, part, &mut out)?,
            Command::AddPiecePleats {
                piece,
                from,
                to,
                depth_mm,
                kind,
                sense,
                count,
                span_mm,
            } => self.add_piece_pleats(
                piece, from, to, depth_mm, kind, sense, count, span_mm, &mut out,
            )?,
            Command::AddPieceFlare {
                piece,
                fixed_from,
                fixed_to,
                lines,
            } => self.add_piece_flare(piece, fixed_from, fixed_to, &lines, &mut out)?,
            Command::AddButtonGroup {
                piece,
                from,
                to,
                count,
                element,
                edge_margin_mm,
            } => self.add_button_group(
                piece,
                crate::piece_buttons::ButtonGroup {
                    id: ButtonGroupId(0),
                    from,
                    to,
                    count,
                    element,
                    edge_margin_mm,
                },
                &mut out,
            )?,
            Command::SetButtonGroup {
                piece,
                group,
                from,
                to,
                count,
                element,
                edge_margin_mm,
            } => self.set_button_group(
                piece,
                crate::piece_buttons::ButtonGroup {
                    id: group,
                    from,
                    to,
                    count,
                    element,
                    edge_margin_mm,
                },
            )?,
            Command::RemoveButtonGroup { piece, group } => {
                self.remove_button_group(piece, group)?
            }
            Command::CreateDerivedPiece {
                mother,
                kind,
                operation,
                name,
            } => self.create_derived_piece(mother, kind, operation, name, &mut out)?,
            Command::BreakDerivedLink { piece } => self.break_derived_link(piece)?,
            Command::UnfoldPiece { piece, fold, name } => self.create_derived_piece(
                piece,
                crate::derived::DerivedKind::Unfolded,
                crate::derived::DerivedOp::Unfold { fold },
                name,
                &mut out,
            )?,
            Command::ExtractPiece {
                sheet,
                source,
                name,
                detached,
            } => self.extract_piece(sheet, source, &name, detached, &mut out)?,
            Command::SplitPiece { piece, a, b } => self.split_piece(piece, a, b, &mut out)?,
            Command::CutPiece {
                piece,
                line,
                notches,
            } => self.cut_piece(piece, line, notches, &mut out)?,
            Command::JoinPieces {
                first,
                first_edge,
                second,
                second_edge,
                tolerance_mm,
            } => self.join_pieces(
                first,
                first_edge,
                second,
                second_edge,
                tolerance_mm,
                &mut out,
            )?,
            Command::AddPoint { sheet, at } => {
                self.resolve_ref(sheet, PointRef::New(at), &mut out)?;
            }
            Command::AddPointAt { sheet, point } => {
                self.resolve_ref(sheet, point, &mut out)?;
            }
            Command::AddPointWithFormulas {
                sheet,
                definition,
                formulas,
            } => self.add_point_with_formulas(sheet, definition, formulas, &mut out)?,
            Command::AddSegment { sheet, from, to } => {
                let a = self.resolve_ref(sheet, from, &mut out)?;
                let b = self.resolve_ref(sheet, to, &mut out)?;
                self.add_segment(sheet, a, b, None, &mut out)?;
            }
            Command::AddSegmentPolar {
                sheet,
                from,
                length_mm,
                angle_deg,
            } => {
                let a = self.resolve_ref(sheet, from, &mut out)?;
                let start = self.sheet(sheet)?.position(a)?;
                let end = pf_geometry::point_from_polar(start, length_mm, angle_deg)?;
                let b = self.resolve_ref(sheet, PointRef::New(end), &mut out)?;
                let c = Construction::Polar {
                    length_mm,
                    angle_deg: angle_deg.rem_euclid(360.0),
                    kept: false,
                };
                self.add_segment(sheet, a, b, Some(c), &mut out)?;
            }
            Command::AddPerpendicular {
                sheet,
                reference,
                from,
                length_mm,
                side,
            } => {
                let (ra, rb) = self.sheet(sheet)?.segment_ends(reference)?;
                let a = self.resolve_ref(sheet, from, &mut out)?;
                let start = self.sheet(sheet)?.position(a)?;
                let end = pf_geometry::perpendicular_end(ra, rb, start, length_mm, side)?;
                let b = self.resolve_ref(sheet, PointRef::New(end), &mut out)?;
                let c = Construction::Perpendicular {
                    reference,
                    length_mm,
                    side,
                    kept: false,
                };
                self.add_segment(sheet, a, b, Some(c), &mut out)?;
            }
            Command::AddNotch {
                sheet,
                entity,
                distance_mm,
                side,
                length_mm,
                style,
            } => {
                let id = NotchId(self.alloc_id());
                let s = self.sheet_mut(sheet)?;
                s.entity(entity)?;
                s.notches.push(Notch {
                    id,
                    entity,
                    distance_mm,
                    side,
                    length_mm,
                    style,
                    anchor: None,
                    form: NotchForm::Line,
                    width_mm: None,
                    on_cut: false,
                    corner: false,
                    proportional: false,
                });
                out.notches.push(id);
            }
            Command::AddAnchoredNotch {
                sheet,
                entity,
                anchor,
                side,
                length_mm,
                style,
            } => {
                let id = NotchId(self.alloc_id());
                let s = self.sheet_mut(sheet)?;
                s.entity(entity)?;
                s.notches.push(Notch {
                    id,
                    entity,
                    distance_mm: 0.0,
                    side,
                    length_mm,
                    style,
                    anchor: Some(anchor),
                    form: NotchForm::Line,
                    width_mm: None,
                    on_cut: false,
                    corner: false,
                    proportional: false,
                });
                out.notches.push(id);
            }
            Command::SetNotchAnchor {
                sheet,
                notch,
                anchor,
            } => {
                let s = self.sheet_mut(sheet)?;
                let current = s.notch(notch)?.clone();
                let new = match anchor {
                    Some(a) => {
                        if current.proportional {
                            out.notes.push(
                                "Le cran n'est plus proportionnel : un cran ancré suit déjà son \
                                 bord."
                                    .into(),
                            );
                        }
                        Notch {
                            distance_mm: 0.0,
                            anchor: Some(a),
                            proportional: false,
                            ..current
                        }
                    }
                    None => s.notch_fixed(&current)?,
                };
                if let Some(n) = s.notches.iter_mut().find(|n| n.id == notch) {
                    *n = new;
                }
            }
            Command::SetNotchShape {
                sheet,
                notch,
                form,
                width_mm,
                on_cut,
            } => {
                if on_cut {
                    let entity = self.sheet(sheet)?.notch(notch)?.entity;
                    if !self
                        .pieces_on(sheet)
                        .iter()
                        .any(|p| p.outline.iter().any(|o| o.entity == entity))
                    {
                        return Err(CoreError::Invalid(
                            "cran sur la coupe : le tracé du cran n'est dans le contour d'aucune \
                             pièce de la feuille"
                                .into(),
                        ));
                    }
                }
                let n = self
                    .sheet_mut(sheet)?
                    .notches
                    .iter_mut()
                    .find(|n| n.id == notch)
                    .ok_or(CoreError::UnknownNotch(notch))?;
                n.form = form;
                n.width_mm = width_mm;
                n.on_cut = on_cut;
            }
            Command::SetNotchCorner {
                sheet,
                notch,
                corner,
            } => {
                let mut probe = self.sheet(sheet)?.notch(notch)?.clone();
                probe.corner = corner;
                // Un cran à distance fixe est ancré au bout de son tracé (proportion 0 ou 1) :
                // sa position se lit alors d'après le tracé et le cran reste au sommet quand
                // la longueur du tracé change (déplacer un sommet, inverser le tracé).
                let mut pin = None;
                if corner {
                    if probe.on_cut {
                        return Err(CoreError::Invalid(
                            crate::notch::CORNER_ON_CUT_REFUSED.into(),
                        ));
                    }
                    let s = self.sheet(sheet)?;
                    self.corner_of(s, &probe)?;
                    if probe.anchor.is_none() {
                        let at_end = probe.distance_mm > 0.5 * s.path_length_mm(probe.entity)?;
                        pin = Some(NotchAnchor::Ratio {
                            ratio: if at_end { 1.0 } else { 0.0 },
                        });
                    }
                }
                if let Some(n) = self
                    .sheet_mut(sheet)?
                    .notches
                    .iter_mut()
                    .find(|n| n.id == notch)
                {
                    n.corner = corner;
                    if pin.is_some() {
                        n.anchor = pin;
                        n.distance_mm = 0.0;
                        if n.proportional {
                            n.proportional = false;
                            out.notes.push(
                                "Le cran n'est plus proportionnel : un cran en coin est ancré \
                                 au sommet."
                                    .into(),
                            );
                        }
                    }
                }
            }
            Command::SetNotch {
                sheet,
                notch,
                distance_mm: d,
                side: sd,
                length_mm: l,
                style: st,
            } => {
                let s = self.sheet_mut(sheet)?;
                // Un cran ancré garde sa position calculée : seul un `d` égal à la position
                // actuelle est accepté (l'interface renvoie la valeur affichée), sinon on
                // demande de changer ou de détacher l'ancrage.
                let anchored_at = {
                    let n = s.notch(notch)?;
                    n.anchor.is_some().then(|| s.notch_distance_mm(n))
                };
                if let Some(at) = anchored_at {
                    let gap = (at? - d).abs();
                    // Un écart non numérique (valeur non finie) est refusé comme les autres.
                    if gap.is_nan() || gap > 1e-6 {
                        return Err(CoreError::Invalid(
                            "ce cran est ancré : sa position se calcule d'après son ancrage \
                             (changez ou détachez l'ancrage pour la modifier)"
                                .into(),
                        ));
                    }
                }
                let n = s
                    .notches
                    .iter_mut()
                    .find(|n| n.id == notch)
                    .ok_or(CoreError::UnknownNotch(notch))?;
                if n.anchor.is_none() {
                    n.distance_mm = d;
                }
                n.side = sd;
                n.length_mm = l;
                n.style = st;
            }
            Command::DeleteNotch { sheet, notch } => {
                let s = self.sheet_mut(sheet)?;
                if !s.notches.iter().any(|n| n.id == notch) {
                    return Err(CoreError::UnknownNotch(notch));
                }
                s.notches.retain(|n| n.id != notch);
            }
            Command::AddDimension {
                sheet,
                from,
                to,
                offset_mm,
            } => {
                crate::dimension::check_offset(offset_mm)?;
                if from == to {
                    return Err(CoreError::Invalid(
                        "cotation : les deux points doivent être différents".into(),
                    ));
                }
                let s = self.sheet(sheet)?;
                let (a, b) = (s.position(from)?, s.position(to)?);
                pf_geometry::segment_length_mm(a, b)?;
                let id = DimensionId(self.alloc_id());
                self.sheet_mut(sheet)?.dimensions.push(Dimension {
                    id,
                    from,
                    to,
                    offset_mm,
                });
                out.dimensions.push(id);
            }
            Command::SetDimensionOffset {
                sheet,
                dimension,
                offset_mm,
            } => {
                crate::dimension::check_offset(offset_mm)?;
                self.sheet_mut(sheet)?
                    .dimensions
                    .iter_mut()
                    .find(|d| d.id == dimension)
                    .ok_or(CoreError::UnknownDimension(dimension))?
                    .offset_mm = offset_mm;
            }
            Command::DeleteDimension { sheet, dimension } => {
                let s = self.sheet_mut(sheet)?;
                if !s.dimensions.iter().any(|d| d.id == dimension) {
                    return Err(CoreError::UnknownDimension(dimension));
                }
                s.dimensions.retain(|d| d.id != dimension);
            }
            Command::AddFullDimension {
                sheet,
                kind,
                label,
                text_shift_mm,
            } => self.add_full_dimension(sheet, kind, label.as_deref(), text_shift_mm, &mut out)?,
            Command::SetFullDimensionPlacement {
                sheet,
                dimension,
                placement,
            } => self.set_full_dimension_placement(sheet, dimension, placement)?,
            Command::SetFullDimensionLabel {
                sheet,
                dimension,
                label,
            } => self.set_full_dimension_label(sheet, dimension, label.as_deref())?,
            Command::SetFullDimensionTextShift {
                sheet,
                dimension,
                text_shift_mm,
            } => self.set_full_dimension_text_shift(sheet, dimension, text_shift_mm)?,
            Command::DeleteFullDimension { sheet, dimension } => {
                self.delete_full_dimension(sheet, dimension)?
            }
            Command::AddGuide {
                sheet,
                x_mm,
                y_mm,
                angle_deg,
                axis,
                layer,
            } => {
                let mut guide = Guide {
                    id: GuideId(0),
                    x_mm,
                    y_mm,
                    angle_deg,
                    axis,
                    layer,
                };
                guide.validate()?;
                guide.angle_deg = crate::guide::normalize_angle_deg(angle_deg);
                self.sheet(sheet)?;
                if let Some(l) = layer {
                    self.sheet(sheet)?.layer(l)?;
                }
                guide.id = GuideId(self.alloc_id());
                out.guides.push(guide.id);
                self.sheet_mut(sheet)?.guides.push(guide);
            }
            Command::MoveGuide {
                sheet,
                guide,
                x_mm,
                y_mm,
                angle_deg,
            } => {
                let g = self
                    .sheet_mut(sheet)?
                    .guides
                    .iter_mut()
                    .find(|g| g.id == guide)
                    .ok_or(CoreError::UnknownGuide(guide))?;
                let moved = Guide {
                    x_mm,
                    y_mm,
                    angle_deg,
                    ..g.clone()
                };
                moved.validate()?;
                *g = Guide {
                    angle_deg: crate::guide::normalize_angle_deg(angle_deg),
                    ..moved
                };
            }
            Command::SetGuideAxis { sheet, guide, axis } => {
                self.sheet_mut(sheet)?
                    .guides
                    .iter_mut()
                    .find(|g| g.id == guide)
                    .ok_or(CoreError::UnknownGuide(guide))?
                    .axis = axis;
            }
            Command::DeleteGuide { sheet, guide } => {
                self.sheet(sheet)?.guide(guide)?;
                self.freeze_mirrors_of_guide(sheet, guide, &mut out)?;
                self.sheet_mut(sheet)?.guides.retain(|g| g.id != guide);
            }
            Command::LinkMirror {
                sheet,
                points,
                entities,
                axis,
            } => self.link_mirror(sheet, &points, &entities, axis, &mut out)?,
            Command::UnfoldMirror { sheet, axis } => self.unfold_mirror(sheet, axis, &mut out)?,
            Command::Batch { commands } => {
                for c in commands {
                    let o = self.apply_unchecked(c)?;
                    out.sheet = o.sheet.or(out.sheet);
                    out.points.extend(o.points);
                    out.entities.extend(o.entities);
                    out.notches.extend(o.notches);
                    out.dimensions.extend(o.dimensions);
                    out.full_dimensions.extend(o.full_dimensions);
                    out.pieces.extend(o.pieces);
                    out.layers.extend(o.layers);
                    out.groups.extend(o.groups);
                    out.guides.extend(o.guides);
                    out.notes.extend(o.notes);
                    out.cutting_plans.extend(o.cutting_plans);
                    out.placements.extend(o.placements);
                }
            }
            Command::AddRectangle {
                sheet,
                corner,
                width_mm,
                height_mm,
            } => {
                for v in [width_mm, height_mm] {
                    if !v.is_finite() || v.abs() <= EPSILON_MM {
                        return Err(pf_geometry::GeometryError::InvalidLength(v).into());
                    }
                }
                let first = self.resolve_ref(sheet, corner, &mut out)?;
                let p0 = self.sheet(sheet)?.position(first)?;
                let others = [
                    PointMm::new(p0.x + width_mm, p0.y),
                    PointMm::new(p0.x + width_mm, p0.y + height_mm),
                    PointMm::new(p0.x, p0.y + height_mm),
                ];
                let mut ids = vec![first];
                for q in others {
                    ids.push(self.resolve_ref(sheet, PointRef::New(q), &mut out)?);
                }
                for i in 0..4 {
                    self.add_segment(sheet, ids[i], ids[(i + 1) % 4], None, &mut out)?;
                }
            }
            Command::AddConstructedRectangle {
                sheet,
                corner,
                width_mm,
                height_mm,
                kept,
            } => self.add_constructed_rectangle(
                sheet,
                corner,
                (width_mm, height_mm),
                kept,
                &mut out,
            )?,
            Command::AddPolygon {
                sheet,
                vertices,
                closed,
                role,
                layer,
            } => self.add_polygon(sheet, &vertices, closed, role, layer, &mut out)?,
            Command::AddOrientedRectangle {
                sheet,
                rect,
                role,
                layer,
            } => {
                let corners = rect.corners()?;
                let vertices: Vec<PointRef> = corners.iter().map(|c| PointRef::New(*c)).collect();
                self.add_polygon(sheet, &vertices, true, role, layer, &mut out)?
            }
            Command::AddRoundedRectangle {
                sheet,
                rect,
                radius_mm,
                hide_centers,
                role,
                layer,
            } => self.add_rounded_rectangle(
                sheet,
                &rect,
                radius_mm,
                hide_centers,
                (role, layer),
                &mut out,
            )?,
            Command::AddParallel {
                sheet,
                reference,
                distance_mm,
                side,
            } => {
                if !distance_mm.is_finite() || distance_mm <= EPSILON_MM {
                    return Err(pf_geometry::GeometryError::InvalidLength(distance_mm).into());
                }
                let (ra, rb) = self.sheet(sheet)?.segment_ends(reference)?;
                let normal = (rb - ra).unit()?.rotated_left();
                let k = match side {
                    Side::Left => distance_mm,
                    Side::Right => -distance_mm,
                };
                let a = self.resolve_ref(
                    sheet,
                    PointRef::New((ra + normal * k).validate()?),
                    &mut out,
                )?;
                let b = self.resolve_ref(
                    sheet,
                    PointRef::New((rb + normal * k).validate()?),
                    &mut out,
                )?;
                let c = Construction::Parallel {
                    reference,
                    distance_mm,
                    side,
                    kept: false,
                };
                self.add_segment(sheet, a, b, Some(c), &mut out)?;
            }
            Command::AddCircle {
                sheet,
                center,
                radius_mm,
            } => {
                pf_geometry::check_radius(radius_mm)?;
                let c = self.resolve_ref(sheet, center, &mut out)?;
                self.add_circle_entity(sheet, c, radius_mm, 0.0, &mut out)?;
            }
            Command::AddCircleThreePoints { sheet, a, b, c } => {
                let (ia, ib, ic) = (
                    self.resolve_ref(sheet, a, &mut out)?,
                    self.resolve_ref(sheet, b, &mut out)?,
                    self.resolve_ref(sheet, c, &mut out)?,
                );
                let s = self.sheet(sheet)?;
                let pa = s.position(ia)?;
                let circle = pf_geometry::Circle::through_three_points(
                    pa,
                    s.position(ib)?,
                    s.position(ic)?,
                )?;
                let start = pf_geometry::direction_angle_deg(circle.center, pa)?;
                let center = self.resolve_ref(sheet, PointRef::New(circle.center), &mut out)?;
                self.add_circle_entity(sheet, center, circle.radius_mm, start, &mut out)?;
            }
            Command::AddCircleTwoPointsRadius {
                sheet,
                a,
                b,
                radius_mm,
                side,
            } => {
                let (ia, ib) = (
                    self.resolve_ref(sheet, a, &mut out)?,
                    self.resolve_ref(sheet, b, &mut out)?,
                );
                let s = self.sheet(sheet)?;
                let pa = s.position(ia)?;
                let (left, right) = pf_geometry::Circle::centers_through_two_points(
                    pa,
                    s.position(ib)?,
                    radius_mm,
                )?;
                let c = if side == Side::Left { left } else { right };
                let start = pf_geometry::direction_angle_deg(c, pa)?;
                let center = self.resolve_ref(sheet, PointRef::New(c), &mut out)?;
                self.add_circle_entity(sheet, center, radius_mm, start, &mut out)?;
            }
            Command::AddArc {
                sheet,
                center,
                radius_mm,
                start_deg,
                end_deg,
                ccw,
            } => {
                let ic = self.resolve_ref(sheet, center, &mut out)?;
                let c = self.sheet(sheet)?.position(ic)?;
                let arc =
                    pf_geometry::Arc::from_center_angles(c, radius_mm, start_deg, end_deg, ccw)?;
                let is = self.resolve_ref(sheet, PointRef::New(arc.start_point()), &mut out)?;
                let ie = self.resolve_ref(sheet, PointRef::New(arc.end_point()), &mut out)?;
                self.add_arc_entity(sheet, ic, is, ie, ccw, &mut out)?;
            }
            Command::AddArcThreePoints {
                sheet,
                start,
                through,
                end,
            } => {
                let (is, it, ie) = (
                    self.resolve_ref(sheet, start, &mut out)?,
                    self.resolve_ref(sheet, through, &mut out)?,
                    self.resolve_ref(sheet, end, &mut out)?,
                );
                let s = self.sheet(sheet)?;
                let (ps, pt, pe) = (s.position(is)?, s.position(it)?, s.position(ie)?);
                let circle = pf_geometry::Circle::through_three_points(ps, pt, pe)?;
                let ccw = pf_geometry::Arc::through_three_points(ps, pt, pe)?.is_ccw();
                let ic = self.resolve_ref(sheet, PointRef::New(circle.center), &mut out)?;
                self.add_arc_entity(sheet, ic, is, ie, ccw, &mut out)?;
            }
            Command::AddArcTwoPointsRadius {
                sheet,
                start,
                end,
                radius_mm,
                side,
                ccw,
            } => {
                let (is, ie) = (
                    self.resolve_ref(sheet, start, &mut out)?,
                    self.resolve_ref(sheet, end, &mut out)?,
                );
                let s = self.sheet(sheet)?;
                let (ps, pe) = (s.position(is)?, s.position(ie)?);
                let (left, right) =
                    pf_geometry::Circle::centers_through_two_points(ps, pe, radius_mm)?;
                let c = if side == Side::Left { left } else { right };
                let ic = self.resolve_ref(sheet, PointRef::New(c), &mut out)?;
                self.add_arc_entity(sheet, ic, is, ie, ccw, &mut out)?;
            }
            Command::SetCircleRadius {
                sheet,
                entity,
                radius_mm,
            } => {
                pf_geometry::check_radius(radius_mm)?;
                let s = self.sheet_mut(sheet)?;
                match &mut s
                    .entities
                    .iter_mut()
                    .find(|e| e.id == entity)
                    .ok_or(CoreError::UnknownEntity(entity))?
                    .kind
                {
                    EntityKind::Circle { radius_mm: r, .. } => *r = radius_mm,
                    _ => {
                        return Err(CoreError::Invalid(format!(
                            "le tracé {} n'est pas un cercle",
                            entity.0
                        )));
                    }
                }
            }
            Command::FilletPaths {
                sheet,
                first,
                second,
                radius_mm,
                pick_first,
                pick_second,
            } => self.fillet_paths(
                sheet,
                first,
                second,
                radius_mm,
                (pick_first, pick_second),
                &mut out,
            )?,
            Command::ChamferPaths {
                sheet,
                first,
                second,
                distance_first_mm,
                distance_second_mm,
                pick_first,
                pick_second,
            } => self.chamfer_paths(
                sheet,
                first,
                second,
                (distance_first_mm, distance_second_mm),
                (pick_first, pick_second),
                &mut out,
            )?,
            Command::AddTangentLine {
                sheet,
                from,
                target,
                pick,
                length_mm,
            } => self.add_tangent_line(sheet, from, target, pick, length_mm, &mut out)?,
            Command::AddTangentAt {
                sheet,
                entity,
                at,
                length_mm,
                backwards,
            } => self.add_tangent_at(sheet, entity, at, length_mm, backwards, &mut out)?,
            Command::Transform {
                sheet,
                points,
                transform,
            } => {
                transform.validate()?;
                let s = self.sheet(sheet)?;
                let mut ids = points.clone();
                ids.sort();
                ids.dedup();
                let mut moves = Vec::new();
                for &p in &ids {
                    let point = s.point(p)?;
                    if matches!(point.definition, PointDefinition::Free { .. }) {
                        moves.push((p, transform.point(s.position(p)?)?));
                    }
                }
                if moves.is_empty() {
                    return Err(CoreError::Invalid(
                        "aucun point libre à transformer : les points construits suivent leurs \
                         tracés"
                            .into(),
                    ));
                }
                let curves: Vec<EntityId> = s
                    .entities
                    .iter()
                    .filter(|e| {
                        matches!(e.kind, EntityKind::Curve { start, end, .. }
                            if ids.contains(&start) && ids.contains(&end))
                    })
                    .map(|e| e.id)
                    .collect();
                let moved: Vec<PointId> = moves.iter().map(|m| m.0).collect();
                let s = self.sheet_mut(sheet)?;
                for (id, pos) in moves {
                    if let Some(p) = s.points.iter_mut().find(|p| p.id == id) {
                        p.definition = PointDefinition::Free { x: pos.x, y: pos.y };
                    }
                }
                // Écarts de gradation (US-09.2a) : une translation les laisse tels quels ; une
                // rotation ou une symétrie les fait tourner / symétriser avec les points (le plan
                // gradué tourne comme le plan de base).
                if !matches!(transform, Transform::Translate { .. }) {
                    let mut turned = 0;
                    for g in s.gradings.iter_mut().filter(|g| moved.contains(&g.point)) {
                        let v = transform.vector(pf_geometry::VectorMm::new(g.dx_mm, g.dy_mm));
                        g.dx_mm = v.x;
                        g.dy_mm = v.y;
                        turned += 1;
                    }
                    if turned > 0 {
                        out.notes.push(format!(
                            "{turned} écart(s) de gradation ont tourné ou été symétrisés avec \
                             leurs points."
                        ));
                    }
                }
                // Cercles : départ et sens suivent la transformation ; arcs : un
                // arc entièrement transformé garde ses extrémités, la symétrie
                // inverse son sens.
                let mirror = matches!(transform, Transform::Mirror { .. });
                for e in s.entities.iter_mut() {
                    match &mut e.kind {
                        EntityKind::Circle {
                            center,
                            start_deg,
                            ccw,
                            ..
                        } if moved.contains(center) => {
                            let (sn, cs) = pf_geometry::sin_cos_deg(*start_deg);
                            let v = transform.vector(pf_geometry::VectorMm::new(cs, sn));
                            *start_deg = v.y.atan2(v.x).to_degrees().rem_euclid(360.0);
                            if mirror {
                                *ccw = !*ccw;
                            }
                        }
                        EntityKind::Arc {
                            center,
                            start,
                            end,
                            ccw,
                        } if mirror
                            && moved.contains(center)
                            && moved.contains(start)
                            && moved.contains(end) =>
                        {
                            *ccw = !*ccw;
                        }
                        _ => {}
                    }
                }
                if mirror {
                    let mirrored: Vec<EntityId> = s
                        .entities
                        .iter()
                        .filter(|e| e.kind.all_points().iter().all(|p| ids.contains(p)))
                        .map(|e| e.id)
                        .collect();
                    for n in s
                        .notches
                        .iter_mut()
                        .filter(|n| mirrored.contains(&n.entity))
                    {
                        n.side = n.side.flipped();
                    }
                }
                // Courbes par points de passage entièrement transformées : leurs
                // tangentes saisies suivent la rotation ou la symétrie.
                for e in s.entities.iter_mut() {
                    if let EntityKind::Spline { nodes } = &mut e.kind {
                        if nodes.iter().all(|n| ids.contains(&n.point)) {
                            for n in nodes.iter_mut() {
                                for t in [&mut n.tangent, &mut n.tangent_in].into_iter().flatten() {
                                    *t = OffsetMm::from_vector(transform.vector(t.vector()));
                                }
                            }
                        }
                    }
                }
                for e in s.entities.iter_mut().filter(|e| curves.contains(&e.id)) {
                    if let EntityKind::Curve {
                        handle_start,
                        handle_end,
                        ..
                    } = &mut e.kind
                    {
                        *handle_start =
                            OffsetMm::from_vector(transform.vector(handle_start.vector()));
                        *handle_end = OffsetMm::from_vector(transform.vector(handle_end.vector()));
                    }
                }
            }
            Command::Duplicate {
                sheet,
                entities,
                transform,
            } => {
                transform.validate()?;
                let mut ids = entities.clone();
                ids.sort();
                ids.dedup();
                if ids.is_empty() {
                    return Err(CoreError::Invalid("aucun tracé à dupliquer".into()));
                }
                // Écarts de gradation (US-09.2a) : les copies sont à la position de base.
                let graded: std::collections::BTreeSet<PointId> = {
                    let s = self.sheet(sheet)?;
                    ids.iter()
                        .filter_map(|e| s.entity_all_points(*e).ok())
                        .flatten()
                        .filter(|p| s.gradings.iter().any(|g| g.point == *p))
                        .collect()
                };
                if !graded.is_empty() {
                    out.notes.push(format!(
                        "{} point(s) gradué(s) sont dupliqués sans leurs écarts de gradation : \
                         les copies sont à la position de base.",
                        graded.len()
                    ));
                }
                let mut copies: BTreeMap<PointId, PointId> = BTreeMap::new();
                for &e in &ids {
                    let entity = self.sheet(sheet)?.entity(e)?.clone();
                    let (a, b) = self.sheet(sheet)?.entity_points(e)?;
                    let extra: Vec<PointId> = match &entity.kind {
                        EntityKind::Arc { center, .. } => vec![*center],
                        EntityKind::Spline { nodes } => nodes.iter().map(|n| n.point).collect(),
                        _ => Vec::new(),
                    };
                    for p in [a, b].into_iter().chain(extra.iter().copied()) {
                        let copy = match copies.get(&p) {
                            Some(&known) => known,
                            None => {
                                let pos = self.sheet(sheet)?.position(p)?;
                                let q = transform.point(pos)?;
                                self.resolve_ref(sheet, PointRef::New(q), &mut out)?
                            }
                        };
                        copies.insert(p, copy);
                    }
                    let (na, nb) = (copies[&a], copies[&b]);
                    match &entity.kind {
                        EntityKind::Segment { .. } => {
                            // Une copie identique à un segment existant (côté sur l'axe d'une
                            // symétrie) est réutilisée, pas dupliquée.
                            let exists = self.sheet(sheet)?.entities.iter().any(|x| {
                                matches!(x.kind, EntityKind::Segment { from, to, .. }
                                    if (from == na && to == nb) || (from == nb && to == na))
                            });
                            if exists || na == nb {
                                continue;
                            }
                            self.add_segment(sheet, na, nb, None, &mut out)?;
                        }
                        &EntityKind::Circle {
                            radius_mm,
                            start_deg,
                            ccw,
                            ..
                        } => {
                            let (sn, cs) = pf_geometry::sin_cos_deg(start_deg);
                            let v = transform.vector(pf_geometry::VectorMm::new(cs, sn));
                            let flip = matches!(transform, Transform::Mirror { .. });
                            let id = EntityId(self.alloc_id());
                            self.sheet_mut(sheet)?.entities.push(Entity {
                                id,
                                role: PathRole::Construction,
                                layer: None,
                                name: None,
                                kind: EntityKind::Circle {
                                    center: na,
                                    radius_mm,
                                    start_deg: v.y.atan2(v.x).to_degrees().rem_euclid(360.0),
                                    ccw: ccw != flip,
                                },
                            });
                            out.entities.push(id);
                        }
                        &EntityKind::Arc { ccw, .. } => {
                            let nc = copies[&extra[0]];
                            let flip = matches!(transform, Transform::Mirror { .. });
                            let id = EntityId(self.alloc_id());
                            self.sheet_mut(sheet)?.entities.push(Entity {
                                id,
                                role: PathRole::Construction,
                                layer: None,
                                name: None,
                                kind: EntityKind::Arc {
                                    center: nc,
                                    start: na,
                                    end: nb,
                                    ccw: ccw != flip,
                                },
                            });
                            out.entities.push(id);
                        }
                        &EntityKind::Curve {
                            handle_start,
                            handle_end,
                            ..
                        } => {
                            let id = EntityId(self.alloc_id());
                            self.sheet_mut(sheet)?.entities.push(Entity {
                                id,
                                role: PathRole::Construction,
                                layer: None,
                                name: None,
                                kind: EntityKind::Curve {
                                    start: na,
                                    handle_start: OffsetMm::from_vector(
                                        transform.vector(handle_start.vector()),
                                    ),
                                    handle_end: OffsetMm::from_vector(
                                        transform.vector(handle_end.vector()),
                                    ),
                                    end: nb,
                                },
                            });
                            out.entities.push(id);
                        }
                        EntityKind::Spline { nodes } => {
                            let moved =
                                |t: OffsetMm| OffsetMm::from_vector(transform.vector(t.vector()));
                            let nodes = nodes
                                .iter()
                                .map(|n| SplineNode {
                                    point: copies[&n.point],
                                    corner: n.corner,
                                    tangent: n.tangent.map(moved),
                                    tangent_in: n.tangent_in.map(moved),
                                })
                                .collect();
                            let id = EntityId(self.alloc_id());
                            self.sheet_mut(sheet)?.entities.push(Entity {
                                id,
                                role: PathRole::Construction,
                                layer: None,
                                name: None,
                                kind: EntityKind::Spline { nodes },
                            });
                            out.entities.push(id);
                        }
                    }
                    // La copie garde le rôle métier et les crans de l'original.
                    if let Some(&new_id) = out.entities.last() {
                        let flip = matches!(transform, Transform::Mirror { .. });
                        let old: Vec<Notch> = self
                            .sheet(sheet)?
                            .notches
                            .iter()
                            .filter(|n| n.entity == e)
                            .cloned()
                            .collect();
                        for n in old {
                            let id = NotchId(self.alloc_id());
                            if n.corner {
                                out.notes.push(
                                    "Un cran en coin est recopié comme cran ordinaire : la copie \
                                     n'est dans aucune pièce."
                                        .into(),
                                );
                            }
                            // Un cran ancré garde son ancrage quand la copie peut le suivre
                            // (milieu, proportion, point copié) ; sinon il est recopié à
                            // distance fixe, avec une remarque.
                            let n = match n.anchor {
                                Some(NotchAnchor::Point { point })
                                    if copies.contains_key(&point) =>
                                {
                                    Notch {
                                        anchor: Some(NotchAnchor::Point {
                                            point: copies[&point],
                                        }),
                                        ..n
                                    }
                                }
                                Some(NotchAnchor::Midpoint {} | NotchAnchor::Ratio { .. }) => n,
                                Some(_) => {
                                    out.notes.push(
                                        "Un cran ancré est recopié à distance fixe : son \
                                         ancrage cite un objet qui n'est pas copié."
                                            .into(),
                                    );
                                    self.sheet(sheet)?.notch_fixed(&n)?
                                }
                                None => n,
                            };
                            self.sheet_mut(sheet)?.notches.push(Notch {
                                id,
                                entity: new_id,
                                side: if flip { n.side.flipped() } else { n.side },
                                // La copie n'est dans aucune pièce : pas de projection.
                                on_cut: false,
                                corner: false,
                                ..n
                            });
                            out.notches.push(id);
                        }
                        if let Some(copy) = self
                            .sheet_mut(sheet)?
                            .entities
                            .iter_mut()
                            .find(|x| x.id == new_id)
                        {
                            copy.role = entity.role;
                        }
                    }
                }
                if out.entities.is_empty() {
                    return Err(CoreError::Invalid(
                        "rien à copier : toutes les copies existent déjà".into(),
                    ));
                }
            }
            Command::AddPointOnSegment {
                sheet,
                segment,
                distance_mm,
                origin,
            } => {
                let (a, b) = self.sheet(sheet)?.segment_ends(segment)?;
                let (a, b) = match origin {
                    SegmentEnd::Start => (a, b),
                    SegmentEnd::End => (b, a),
                };
                // Valide la distance avant toute création (AT-03).
                pf_geometry::point_on_segment(a, b, distance_mm)?;
                let id = PointId(self.alloc_id());
                let s = self.sheet_mut(sheet)?;
                let label = s.next_label();
                s.points.push(Point {
                    id,
                    label,
                    definition: PointDefinition::OnSegment {
                        segment,
                        distance_mm,
                        origin,
                    },
                    role: PointRole::Construction,
                    layer: None,
                });
                out.points.push(id);
            }
            Command::AddCurve {
                sheet,
                start,
                c1,
                c2,
                end,
            } => {
                c1.validate()?;
                c2.validate()?;
                let a = self.resolve_ref(sheet, start, &mut out)?;
                let b = self.resolve_ref(sheet, end, &mut out)?;
                let s = self.sheet(sheet)?;
                let (p0, p3) = (s.position(a)?, s.position(b)?);
                let id = EntityId(self.alloc_id());
                self.sheet_mut(sheet)?.entities.push(Entity {
                    id,
                    role: PathRole::Construction,
                    layer: None,
                    name: None,
                    kind: EntityKind::Curve {
                        start: a,
                        handle_start: OffsetMm::from_vector(c1 - p0),
                        handle_end: OffsetMm::from_vector(c2 - p3),
                        end: b,
                    },
                });
                out.entities.push(id);
            }
            Command::AddSpline { sheet, points } => {
                if points.len() < 2 {
                    return Err(CoreError::Invalid(
                        "une courbe par points de passage en exige au moins deux".into(),
                    ));
                }
                let mut nodes = Vec::with_capacity(points.len());
                for r in points {
                    nodes.push(SplineNode::new(self.resolve_ref(sheet, r, &mut out)?));
                }
                let id = EntityId(self.alloc_id());
                self.sheet_mut(sheet)?.entities.push(Entity {
                    id,
                    role: PathRole::Construction,
                    layer: None,
                    name: None,
                    kind: EntityKind::Spline { nodes },
                });
                out.entities.push(id);
            }
            Command::AddSplinePoint {
                sheet,
                spline,
                index,
                point,
            } => {
                let node = SplineNode::new(self.resolve_ref(sheet, point, &mut out)?);
                let nodes = self.spline_nodes_mut(sheet, spline)?;
                if index > nodes.len() {
                    return Err(CoreError::Invalid(format!(
                        "position {index} hors de la courbe ({} points de passage)",
                        nodes.len()
                    )));
                }
                nodes.insert(index, node);
            }
            Command::RemoveSplinePoint {
                sheet,
                spline,
                index,
            } => {
                let nodes = self.spline_nodes_mut(sheet, spline)?;
                if nodes.len() <= 2 {
                    return Err(CoreError::Invalid(
                        "une courbe par points de passage garde au moins deux points".into(),
                    ));
                }
                if index >= nodes.len() {
                    return Err(CoreError::Invalid(format!(
                        "point de passage {index} inexistant ({} points)",
                        nodes.len()
                    )));
                }
                nodes.remove(index);
                // Un point devenu extrémité n'a plus qu'une tangente, lisse :
                // au départ la sortante, à l'arrivée l'entrante.
                let last = nodes.len() - 1;
                nodes[0].corner = false;
                nodes[0].tangent_in = None;
                if nodes[last].corner {
                    nodes[last].tangent = nodes[last].tangent_in;
                }
                nodes[last].corner = false;
                nodes[last].tangent_in = None;
            }
            Command::SetSplineNode {
                sheet,
                spline,
                index,
                corner,
                tangent,
                tangent_in,
            } => {
                let nodes = self.spline_nodes_mut(sheet, spline)?;
                let len = nodes.len();
                let node = nodes.get_mut(index).ok_or_else(|| {
                    CoreError::Invalid(format!(
                        "point de passage {index} inexistant ({len} points)"
                    ))
                })?;
                node.corner = corner;
                node.tangent = tangent;
                node.tangent_in = tangent_in;
            }
            Command::CurveToSpline { sheet, curve } => {
                let e = self.sheet_mut(sheet)?.entity_mut(curve)?;
                let EntityKind::Curve {
                    start,
                    handle_start,
                    handle_end,
                    end,
                } = e.kind
                else {
                    return Err(CoreError::NotACurve(curve));
                };
                let scaled = |o: OffsetMm, k: f64| OffsetMm {
                    dx: o.dx * k,
                    dy: o.dy * k,
                };
                e.kind = EntityKind::Spline {
                    nodes: vec![
                        SplineNode {
                            tangent: Some(scaled(handle_start, 3.0)),
                            ..SplineNode::new(start)
                        },
                        SplineNode {
                            tangent: Some(scaled(handle_end, -3.0)),
                            ..SplineNode::new(end)
                        },
                    ],
                };
            }
            Command::SplineToCurve { sheet, spline } => {
                let nodes = self.sheet(sheet)?.spline_nodes(spline)?;
                if nodes.len() != 2 {
                    return Err(CoreError::Invalid(format!(
                        "seule une courbe à deux points de passage se convertit en Bézier \
                         (celle-ci en a {})",
                        nodes.len()
                    )));
                }
                let (a, b) = (nodes[0], nodes[1]);
                let kind = EntityKind::Curve {
                    start: a.point,
                    handle_start: OffsetMm::from_vector(a.tangent_out * (1.0 / 3.0)),
                    handle_end: OffsetMm::from_vector(b.tangent_in * (-1.0 / 3.0)),
                    end: b.point,
                };
                self.sheet_mut(sheet)?.entity_mut(spline)?.kind = kind;
            }
            Command::MovePoint { sheet, point, to } => {
                to.validate()?;
                self.move_point(sheet, point, to)?;
            }
            Command::SetPointDistance {
                sheet,
                point,
                distance_mm,
            } => {
                let s = self.sheet_mut(sheet)?;
                let label = s.label(point).to_string();
                let p = s
                    .points
                    .iter_mut()
                    .find(|p| p.id == point)
                    .ok_or(CoreError::UnknownPoint(point))?;
                match &mut p.definition {
                    PointDefinition::OnSegment { distance_mm: d, .. } => *d = distance_mm,
                    _ => {
                        return Err(CoreError::Invalid(format!(
                            "le point {label} n'est pas défini par une distance sur un segment"
                        )));
                    }
                }
            }
            Command::SetPointParameter {
                sheet,
                point,
                value,
            } => {
                let s = self.sheet_mut(sheet)?;
                let label = s.label(point).to_string();
                let p = s
                    .points
                    .iter_mut()
                    .find(|p| p.id == point)
                    .ok_or(CoreError::UnknownPoint(point))?;
                match &mut p.definition {
                    PointDefinition::OnSegmentRatio { ratio, .. } => *ratio = value,
                    PointDefinition::OnCurve { t, .. } => *t = value,
                    PointDefinition::Division { ratio, .. } => *ratio = value,
                    _ => {
                        return Err(CoreError::Invalid(format!(
                            "le point {label} n'est pas défini par une proportion"
                        )));
                    }
                }
            }
            Command::SetPointValues {
                sheet,
                point,
                values,
            } => {
                let s = self.sheet_mut(sheet)?;
                let label = s.label(point).to_string();
                let p = s
                    .points
                    .iter_mut()
                    .find(|p| p.id == point)
                    .ok_or(CoreError::UnknownPoint(point))?;
                let wrong = |n: usize| {
                    CoreError::Invalid(format!(
                        "le point {label} attend {n} valeur(s), {} reçue(s)",
                        values.len()
                    ))
                };
                match &mut p.definition {
                    PointDefinition::Offset { dx_mm, dy_mm, .. } => {
                        let [dx, dy] = values[..] else {
                            return Err(wrong(2));
                        };
                        (*dx_mm, *dy_mm) = (dx, dy);
                    }
                    PointDefinition::AlongCurve { distance_mm, .. }
                    | PointDefinition::Bisector { distance_mm, .. } => {
                        let [d] = values[..] else {
                            return Err(wrong(1));
                        };
                        *distance_mm = d;
                    }
                    PointDefinition::Polar {
                        distance_mm,
                        angle_deg,
                        ..
                    } => {
                        let [d, a] = values[..] else {
                            return Err(wrong(2));
                        };
                        (*distance_mm, *angle_deg) = (d, a);
                    }
                    PointDefinition::Compass {
                        radius_a_mm,
                        radius_b_mm,
                        ..
                    } => {
                        let [ra, rb] = values[..] else {
                            return Err(wrong(2));
                        };
                        (*radius_a_mm, *radius_b_mm) = (ra, rb);
                    }
                    _ => {
                        return Err(CoreError::Invalid(format!(
                            "le point {label} n'est pas un point relatif à paramètres"
                        )));
                    }
                }
            }
            Command::DivideTrace {
                sheet,
                trace,
                spec,
                start,
            } => {
                let ratios = spec.ratios()?;
                let s = self.sheet(sheet)?;
                s.entity(trace)?;
                if s.entity_locked(trace) {
                    let layer = s.entity(trace)?.layer;
                    return Err(locked_error(s, layer, &s.entity_name(trace)));
                }
                if let Some(p) = start {
                    s.point(p)?;
                }
                for ratio in ratios {
                    let definition = PointDefinition::Division {
                        trace,
                        ratio,
                        start,
                    };
                    // Évalué avant création : refus sans trace si la division est impossible.
                    self.sheet(sheet)?.evaluate(&definition)?;
                    let id = PointId(self.alloc_id());
                    let s = self.sheet_mut(sheet)?;
                    let label = s.next_label();
                    s.points.push(Point {
                        id,
                        label,
                        definition,
                        role: PointRole::Construction,
                        layer: None,
                    });
                    out.points.push(id);
                }
                out.sheet = Some(sheet);
            }
            Command::ExtendPath {
                sheet,
                entity,
                end,
                delta_mm,
            } => self.extend_path(sheet, entity, end, delta_mm, &mut out)?,
            Command::TrimPathTo {
                sheet,
                entity,
                end,
                target,
                pick,
            } => self.trim_path_to(sheet, entity, end, target, pick, &mut out)?,
            Command::SplitPath { sheet, entity, at } => {
                self.split_path(sheet, entity, at, paths::SplitHint::default(), &mut out)?
            }
            Command::MergePaths {
                sheet,
                first,
                second,
            } => self.merge_paths(sheet, first, second, &mut out)?,
            Command::ReversePath { sheet, entity } => self.reverse_path(sheet, entity, &mut out)?,
            Command::OffsetPaths {
                sheet,
                paths,
                distance_mm,
                corner,
                role,
            } => self.offset_paths(sheet, &paths, distance_mm, corner, role, &mut out)?,
            Command::MoveCurveHandle {
                sheet,
                curve,
                handle,
                to,
            } => {
                to.validate()?;
                let geom = self.sheet(sheet)?.curve_geometry(curve)?;
                let s = self.sheet_mut(sheet)?;
                let e = s
                    .entities
                    .iter_mut()
                    .find(|e| e.id == curve)
                    .ok_or(CoreError::UnknownEntity(curve))?;
                if let EntityKind::Curve {
                    handle_start,
                    handle_end,
                    ..
                } = &mut e.kind
                {
                    match handle {
                        CurveHandle::Start => *handle_start = OffsetMm::from_vector(to - geom.p0),
                        CurveHandle::End => *handle_end = OffsetMm::from_vector(to - geom.p3),
                    }
                }
            }
            Command::SetPointLabel {
                sheet,
                point,
                label,
            } => {
                let s = self.sheet_mut(sheet)?;
                let label = label.trim().to_string();
                check_name(&label, "l'étiquette du point")?;
                if s.points.iter().any(|p| p.id != point && p.label == label) {
                    return Err(CoreError::Invalid(format!(
                        "l'étiquette « {label} » est déjà utilisée sur cette feuille"
                    )));
                }
                s.points
                    .iter_mut()
                    .find(|p| p.id == point)
                    .ok_or(CoreError::UnknownPoint(point))?
                    .label = label;
            }
            Command::SetPointRole { sheet, point, role } => {
                self.sheet_mut(sheet)?
                    .points
                    .iter_mut()
                    .find(|p| p.id == point)
                    .ok_or(CoreError::UnknownPoint(point))?
                    .role = role;
            }
            Command::SetPathRole {
                sheet,
                entity,
                role,
            } => {
                self.sheet_mut(sheet)?
                    .entities
                    .iter_mut()
                    .find(|e| e.id == entity)
                    .ok_or(CoreError::UnknownEntity(entity))?
                    .role = role;
            }
            Command::AddTopstitch {
                sheet,
                source,
                distance_mm,
                side,
            } => self.add_topstitch(sheet, source, distance_mm, side, &mut out)?,
            Command::SetTopstitch {
                sheet,
                topstitch,
                distance_mm,
                side,
            } => self.set_topstitch(sheet, topstitch, distance_mm, side)?,
            Command::BreakTopstitchLink { sheet, topstitch } => {
                self.break_topstitch_link(sheet, topstitch, &mut out)?
            }
            Command::Delete {
                sheet,
                points,
                entities,
            } => {
                self.delete(sheet, &points, &entities, &mut out)?;
            }
            Command::AddLayer { sheet, name } => {
                let id = LayerId(self.alloc_id());
                self.sheet_mut(sheet)?.layers.push(Layer {
                    id,
                    name: name.trim().to_string(),
                    visible: true,
                    locked: false,
                });
                out.layers.push(id);
            }
            Command::RenameLayer { sheet, layer, name } => {
                self.layer_mut(sheet, layer)?.name = name.trim().to_string();
            }
            Command::SetLayerVisible {
                sheet,
                layer,
                visible,
            } => self.layer_mut(sheet, layer)?.visible = visible,
            Command::SetLayerLocked {
                sheet,
                layer,
                locked,
            } => self.layer_mut(sheet, layer)?.locked = locked,
            Command::MoveLayer {
                sheet,
                layer,
                new_index,
            } => {
                let s = self.sheet_mut(sheet)?;
                let from = s
                    .layers
                    .iter()
                    .position(|l| l.id == layer)
                    .ok_or(CoreError::UnknownLayer(layer))?;
                let l = s.layers.remove(from);
                let idx = new_index.min(s.layers.len());
                s.layers.insert(idx, l);
            }
            Command::DeleteLayer {
                sheet,
                layer,
                move_to,
            } => {
                let s = self.sheet_mut(sheet)?;
                let name = s.layer(layer)?.name.clone();
                if let Some(target) = move_to {
                    s.layer(target)?;
                    if target == layer {
                        return Err(CoreError::Invalid(
                            "le calque de destination doit être différent du calque supprimé"
                                .into(),
                        ));
                    }
                }
                let n = s.points.iter().filter(|p| p.layer == Some(layer)).count()
                    + s.entities.iter().filter(|e| e.layer == Some(layer)).count()
                    + s.guides.iter().filter(|g| g.layer == Some(layer)).count();
                if n > 0 && move_to.is_none() {
                    return Err(CoreError::Invalid(format!(
                        "le calque « {name} » contient {n} objet(s) : choisissez un calque de \
                         destination pour les déplacer, ou videz-le d'abord"
                    )));
                }
                for p in s.points.iter_mut().filter(|p| p.layer == Some(layer)) {
                    p.layer = move_to;
                }
                for e in s.entities.iter_mut().filter(|e| e.layer == Some(layer)) {
                    e.layer = move_to;
                }
                for g in s.guides.iter_mut().filter(|g| g.layer == Some(layer)) {
                    g.layer = move_to;
                }
                s.layers.retain(|l| l.id != layer);
                if n > 0 {
                    out.notes.push(format!(
                        "{n} objet(s) déplacé(s) hors du calque « {name} »."
                    ));
                }
            }
            Command::SetObjectsLayer {
                sheet,
                points,
                entities,
                layer,
            } => {
                let s = self.sheet_mut(sheet)?;
                if let Some(l) = layer {
                    s.layer(l)?;
                }
                for &p in &points {
                    s.point(p)?;
                }
                for &e in &entities {
                    s.entity(e)?;
                }
                for p in s.points.iter_mut().filter(|p| points.contains(&p.id)) {
                    p.layer = layer;
                }
                for e in s.entities.iter_mut().filter(|e| entities.contains(&e.id)) {
                    e.layer = layer;
                }
            }
            Command::SetEntityName {
                sheet,
                entity,
                name,
            } => {
                let name = name.map(|n| n.trim().to_string()).filter(|n| !n.is_empty());
                let s = self.sheet_mut(sheet)?;
                if let Some(n) = &name {
                    check_name(n, "le nom du tracé")?;
                    if s.entities
                        .iter()
                        .any(|e| e.id != entity && e.name.as_deref() == Some(n.as_str()))
                    {
                        return Err(CoreError::Invalid(format!(
                            "le nom « {n} » est déjà utilisé sur cette feuille"
                        )));
                    }
                }
                s.entities
                    .iter_mut()
                    .find(|e| e.id == entity)
                    .ok_or(CoreError::UnknownEntity(entity))?
                    .name = name;
            }
            Command::CreateGroup { sheet, entities } => {
                let mut members = entities.clone();
                members.sort();
                members.dedup();
                if members.len() < 2 {
                    return Err(CoreError::Invalid(
                        "un groupe contient au moins deux tracés distincts".into(),
                    ));
                }
                let id = GroupId(self.alloc_id());
                let s = self.sheet_mut(sheet)?;
                for &m in &members {
                    s.entity(m)?;
                    if s.group_of(m).is_some() {
                        return Err(CoreError::Invalid(format!(
                            "{} fait déjà partie d'un groupe : dégroupez-le d'abord",
                            s.entity_name(m)
                        )));
                    }
                }
                s.groups.push(Group { id, members });
                out.groups.push(id);
            }
            Command::Ungroup { sheet, group } => {
                let s = self.sheet_mut(sheet)?;
                s.group(group)?;
                s.groups.retain(|g| g.id != group);
            }
            Command::PasteFragment {
                sheet,
                fragment,
                layer,
            } => self.paste_fragment(sheet, &fragment, layer, &mut out)?,
            Command::InsertLibrary {
                content,
                sheet,
                layer,
                rename_variables,
            } => self.insert_library(&content, sheet, layer, rename_variables, &mut out)?,
            Command::ImportGeometry { sheet, objects } => {
                self.import_geometry(sheet, objects, &mut out)?
            }
            Command::ImportGeometryNewSheet { name, objects } => {
                self.import_geometry_new_sheet(&name, objects, &mut out)?
            }
            Command::ImportAamaPieces { pieces } => self.import_aama_pieces(pieces, &mut out)?,
            Command::InLayer {
                sheet,
                layer,
                command,
            } => {
                self.sheet(sheet)?.layer(layer)?;
                out = self.apply_unchecked(*command)?;
                if let Ok(s) = self.sheet_mut(sheet) {
                    for p in s.points.iter_mut().filter(|p| out.points.contains(&p.id)) {
                        p.layer = Some(layer);
                    }
                    for e in s
                        .entities
                        .iter_mut()
                        .filter(|e| out.entities.contains(&e.id))
                    {
                        e.layer = Some(layer);
                    }
                    for g in s.guides.iter_mut().filter(|g| out.guides.contains(&g.id)) {
                        g.layer = Some(layer);
                    }
                }
            }
        }
        Ok(out)
    }

    fn layer_mut(&mut self, sheet: SheetId, layer: LayerId) -> Result<&mut Layer, CoreError> {
        self.sheet_mut(sheet)?
            .layers
            .iter_mut()
            .find(|l| l.id == layer)
            .ok_or(CoreError::UnknownLayer(layer))
    }

    /// Refuse toute commande qui modifierait, déplacerait ou supprimerait un
    /// objet d'un calque verrouillé (état avant la commande), ou qui en
    /// ajouterait dans un calque verrouillé (état après). Sans calque
    /// verrouillé, ne coûte rien.
    fn check_locks(&self, new: &Project) -> Result<(), CoreError> {
        let any_locked = |p: &Project| p.sheets.iter().any(|s| s.layers.iter().any(|l| l.locked));
        if !any_locked(self) && !any_locked(new) {
            return Ok(());
        }
        for old in &self.sheets {
            match new.sheets.iter().find(|s| s.id == old.id) {
                Some(n) => old.check_locks(n)?,
                None => {
                    if let Some(what) = old.first_locked_object() {
                        return Err(locked_error(old, None, &what));
                    }
                }
            }
        }
        Ok(())
    }

    pub(crate) fn delete(
        &mut self,
        sheet: SheetId,
        points: &[PointId],
        entities: &[EntityId],
        out: &mut Outcome,
    ) -> Result<(), CoreError> {
        let s = self.sheet(sheet)?;
        for &p in points {
            s.point(p)?;
        }
        // Extrémités des tracés supprimés : candidates au nettoyage.
        let mut candidates = Vec::new();
        for &e in entities {
            candidates.extend(s.entity_all_points(e)?);
        }
        // Les points construits sur un segment supprimé sont figés à leur
        // position actuelle plutôt que supprimés ou laissés orphelins.
        let mut frozen = Vec::new();
        for p in &s.points {
            let depends = p.definition.dependencies();
            if depends.iter().any(|d| entities.contains(d)) && !points.contains(&p.id) {
                frozen.push((p.id, s.position(p.id)?, p.label.clone()));
            }
        }
        let s = self.sheet_mut(sheet)?;
        for (id, pos, label) in frozen {
            if let Some(p) = s.points.iter_mut().find(|p| p.id == id) {
                p.definition = PointDefinition::Free { x: pos.x, y: pos.y };
                out.notes.push(format!(
                    "Le point {label} dépendait d'un tracé supprimé : il est maintenant libre."
                ));
            }
        }
        s.entities.retain(|e| !entities.contains(&e.id));
        for g in &mut s.groups {
            g.members.retain(|m| !entities.contains(m));
        }
        s.groups.retain(|g| !g.members.is_empty());
        // Une pièce dont le contour ou le droit-fil perd un tracé : le contour
        // est vidé (un contour partiel serait trompeur), le droit-fil retiré ;
        // la pièce elle-même reste (décision 36).
        for piece in &mut self.pieces {
            if piece.outline.iter().any(|e| entities.contains(&e.entity)) {
                piece.outline.clear();
                out.notes.push(format!(
                    "Le contour de la pièce « {} » référençait un tracé supprimé : il est vidé.",
                    piece.name
                ));
            }
            if piece.grainline.is_some_and(|g| entities.contains(&g)) {
                piece.grainline = None;
                out.notes.push(format!(
                    "Le droit-fil de la pièce « {} » référençait un tracé supprimé : retiré.",
                    piece.name
                ));
            }
        }
        let s = self.sheet_mut(sheet)?;
        let before = s.notches.len();
        s.notches.retain(|n| !entities.contains(&n.entity));
        if s.notches.len() < before {
            out.notes.push(format!(
                "{} cran(s) supprimé(s) avec leur tracé.",
                before - s.notches.len()
            ));
        }
        for &p in points {
            if let Some(child) = s
                .points
                .iter()
                .find(|q| !points.contains(&q.id) && q.definition.point_dependencies().contains(&p))
            {
                return Err(CoreError::Invalid(format!(
                    "le point {} sert de parent au point construit {} : supprimez celui-ci                      d'abord ou ajoutez-le à la sélection",
                    s.label(p),
                    child.label
                )));
            }
            if s.point_is_used(p) {
                let n = s
                    .entities
                    .iter()
                    .filter(|e| e.kind.all_points().contains(&p))
                    .count();
                return Err(CoreError::Invalid(format!(
                    "le point {} est utilisé par {n} tracé(s) non sélectionné(s) : \
                     supprimez-les d'abord ou ajoutez-les à la sélection",
                    s.label(p)
                )));
            }
        }
        s.points.retain(|p| !points.contains(&p.id));
        let parents: std::collections::HashSet<PointId> = s
            .points
            .iter()
            .flat_map(|p| p.definition.point_dependencies())
            .collect();
        // Extrémités libres devenues inutiles.
        s.points.retain(|p| {
            !(candidates.contains(&p.id)
                && matches!(p.definition, PointDefinition::Free { .. })
                && !s_uses(&s.entities, p.id)
                && !parents.contains(&p.id))
        });
        let before = s.dimensions.len();
        let alive: std::collections::HashSet<PointId> = s.points.iter().map(|p| p.id).collect();
        s.dimensions
            .retain(|d| alive.contains(&d.from) && alive.contains(&d.to));
        if s.dimensions.len() < before {
            out.notes.push(format!(
                "{} cotation(s) supprimée(s) avec leurs points.",
                before - s.dimensions.len()
            ));
        }
        Ok(())
    }
}

fn locked_error(sheet: &Sheet, layer: Option<LayerId>, what: &str) -> CoreError {
    CoreError::LayerLocked(format!(
        "{what} : le calque « {} » est verrouillé, l'objet ne peut pas être modifié, déplacé ou \
         supprimé (déverrouillez le calque d'abord)",
        sheet.layer_name(layer)
    ))
}

fn geometry_of(s: &Sheet, id: EntityId) -> Option<Vec<PointMm>> {
    match s.entity(id).ok()?.kind {
        EntityKind::Segment { .. } => s.segment_ends(id).ok().map(|(a, b)| vec![a, b]),
        EntityKind::Curve { .. } => s
            .curve_geometry(id)
            .ok()
            .map(|c| vec![c.p0, c.c1, c.c2, c.p3]),
        EntityKind::Arc { .. } => s
            .arc_geometry(id)
            .ok()
            .map(|a| vec![a.center, a.start_point(), a.end_point()]),
        EntityKind::Circle { .. } => s.circle_geometry(id).ok().map(|c| {
            vec![
                c.circle.center,
                PointMm::new(c.circle.radius_mm, c.start_deg),
            ]
        }),
        EntityKind::Spline { .. } => s.spline_geometry(id).ok().map(|sp| {
            sp.segments()
                .iter()
                .flat_map(|c| [c.p0, c.c1, c.c2, c.p3])
                .collect()
        }),
    }
}

impl Sheet {
    /// Premier objet d'un calque verrouillé de la feuille (pour les messages).
    fn first_locked_object(&self) -> Option<String> {
        if let Some(p) = self.points.iter().find(|p| self.layer_locked(p.layer)) {
            return Some(format!("le point {}", p.label));
        }
        self.entities
            .iter()
            .find(|e| self.layer_locked(e.layer))
            .map(|e| self.entity_name(e.id))
            .or_else(|| {
                (self.guides.iter().any(|g| self.guide_locked(g)))
                    .then(|| "une ligne d'aide".to_string())
            })
    }

    /// Compare cette feuille (avant) à `new` (après la commande).
    fn check_locks(&self, new: &Sheet) -> Result<(), CoreError> {
        let _before = crate::sheet_index::IndexGuard::new(self);
        let _after = crate::sheet_index::IndexGuard::new(new);
        for op in self.points.iter().filter(|p| self.layer_locked(p.layer)) {
            let what = format!("le point {}", op.label);
            let same = new.point(op.id).is_ok_and(|np| np == op)
                && self.position(op.id).ok() == new.position(op.id).ok();
            if !same {
                return Err(locked_error(self, op.layer, &what));
            }
        }
        for oe in self.entities.iter().filter(|e| self.layer_locked(e.layer)) {
            let same = new.entity(oe.id).is_ok_and(|ne| ne == oe)
                && geometry_of(self, oe.id) == geometry_of(new, oe.id);
            if !same {
                return Err(locked_error(self, oe.layer, &self.entity_name(oe.id)));
            }
            // Grouper ou dégrouper un tracé verrouillé le modifie.
            if self.group_of(oe.id).is_some() != new.group_of(oe.id).is_some() {
                return Err(locked_error(self, oe.layer, &self.entity_name(oe.id)));
            }
            for on in self.notches.iter().filter(|n| n.entity == oe.id) {
                // Un cran ancré qui change de position (point déplacé) est modifié.
                let moved = on.anchor.is_some()
                    && new.notch(on.id).is_ok_and(|nn| {
                        new.notch_distance_mm(nn).ok() != self.notch_distance_mm(on).ok()
                    });
                if new.notch(on.id) != Ok(on) || moved {
                    return Err(locked_error(
                        self,
                        oe.layer,
                        &format!("un cran de {}", self.entity_name(oe.id)),
                    ));
                }
            }
        }
        for od in &self.dimensions {
            let locked = self.point_locked(od.from) || self.point_locked(od.to);
            if locked && new.dimension(od.id) != Ok(od) {
                let layer = [od.from, od.to]
                    .into_iter()
                    .find(|&p| self.point_locked(p))
                    .and_then(|p| self.point(p).ok().and_then(|p| p.layer));
                return Err(locked_error(self, layer, "une cotation"));
            }
        }
        for od in &self.full_dimensions {
            if self.full_dimension_locked(od) && new.full_dimension(od.id) != Ok(od) {
                return Err(locked_error(
                    self,
                    self.full_dimension_lock_layer(od),
                    "une cote",
                ));
            }
        }
        for og in self.guides.iter().filter(|g| self.guide_locked(g)) {
            if new.guide(og.id) != Ok(og) {
                return Err(locked_error(self, og.layer, "une ligne d'aide"));
            }
        }
        for ng in new.guides.iter().filter(|g| new.guide_locked(g)) {
            if self.guide(ng.id).is_err() {
                return Err(locked_error(new, ng.layer, "une ligne d'aide"));
            }
        }
        // Ajouts dans un calque verrouillé (état après la commande).
        for np in new.points.iter().filter(|p| new.layer_locked(p.layer)) {
            if self.point(np.id).map_or(true, |op| op.layer != np.layer) {
                return Err(locked_error(
                    new,
                    np.layer,
                    &format!("le point {}", np.label),
                ));
            }
        }
        for ne in new.entities.iter().filter(|e| new.layer_locked(e.layer)) {
            if self.entity(ne.id).map_or(true, |oe| oe.layer != ne.layer) {
                return Err(locked_error(new, ne.layer, &new.entity_name(ne.id)));
            }
        }
        for nn in &new.notches {
            if new.entity_locked(nn.entity) && self.notch(nn.id).is_err() {
                let layer = new.entity(nn.entity).ok().and_then(|e| e.layer);
                return Err(locked_error(
                    new,
                    layer,
                    &format!("un cran sur {}", new.entity_name(nn.entity)),
                ));
            }
        }
        Ok(())
    }
}

fn clean_material(material: Option<String>) -> Option<String> {
    material
        .map(|m| m.trim().to_string())
        .filter(|m| !m.is_empty())
}

fn s_uses(entities: &[Entity], id: PointId) -> bool {
    entities.iter().any(|e| e.kind.all_points().contains(&id))
}
