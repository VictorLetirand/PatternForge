//! Modèle documentaire : projet → feuilles → points et tracés.
//!
//! Le rôle métier d'un tracé ([`PathRole`]), la manière dont un point est
//! construit ([`PointDefinition`]) et son rôle ([`PointRole`]) sont des
//! concepts volontairement séparés. Toutes les valeurs sont en millimètres.

use crate::error::CoreError;
use pf_geometry::{
    Arc, Bbox, CircleTrace, CubicBezier, HermiteNode, PointMm, Side, Spline, VectorMm,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};

/// Version du schéma écrite par cette version du logiciel.
pub const SCHEMA_VERSION: u32 = 6;
/// Plus ancienne version encore lisible (migrée à l'ouverture).
pub const MIN_SCHEMA_VERSION: u32 = 1;
/// Marqueur de format, premier garde-fou à l'ouverture.
pub const FORMAT_TAG: &str = "patternforge";
/// Extension des fichiers projet.
pub const FILE_EXTENSION: &str = "pforge";

/// Nom d'un fichier de projet sans son extension : `.pforge`, l'ancienne `.pforge.json` (toujours
/// lue) ou `.json`.
pub fn project_stem(file_name: &str) -> &str {
    [".pforge.json", ".pforge", ".json"]
        .iter()
        .find_map(|s| file_name.strip_suffix(s))
        .unwrap_or(file_name)
}
/// Longueur maximale d'un nom (projet, feuille, étiquette).
pub const MAX_NAME_LEN: usize = 120;
/// Longueur maximale du commentaire d'une pièce (US-04.2), en caractères.
pub const MAX_COMMENT_LEN: usize = 2000;

macro_rules! id_type {
    ($name:ident) => {
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(pub u64);
    };
}
id_type!(SheetId);
id_type!(PointId);
id_type!(EntityId);
id_type!(NotchId);
id_type!(DimensionId);
id_type!(PieceId);
id_type!(LayerId);
id_type!(GroupId);
id_type!(GuideId);
id_type!(MarkingId);
id_type!(DartId);
id_type!(PleatId);
id_type!(FlareId);
id_type!(ButtonGroupId);
id_type!(TopstitchId);
id_type!(StitchLineId);
id_type!(ZoneId);
id_type!(StitchPairId);
id_type!(FullDimensionId);
id_type!(SeamPairId);
id_type!(SizeRangeId);
id_type!(SizeId);
id_type!(GradingRuleId);
id_type!(MeasurementId);
id_type!(ClientId);
id_type!(AlterationId);
id_type!(FinishedMeasureId);
id_type!(CuttingPlanId);
id_type!(PlacementId);
id_type!(VariantId);
id_type!(NamingRuleId);
id_type!(VariantEntryId);
id_type!(HideDefectId);
id_type!(QualityZoneId);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub format: String,
    pub schema_version: u32,
    pub name: String,
    /// Prochain identifiant libre ; les identifiants ne sont jamais réutilisés.
    pub next_id: u64,
    pub sheets: Vec<Sheet>,
    /// Pièces du projet (entités, schéma v3) ; absent du fichier si aucune.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pieces: Vec<Piece>,
    /// Marges de couture par défaut par catégorie de pièce (US-05.5) ; absent = valeurs d'atelier
    /// intégrées (`SeamDefaults::builtin`). Jamais appliquées rétroactivement aux marges posées.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seam_defaults: Option<crate::piece_seam::SeamDefaults>,
    /// Coutures appariées (US-08.1) : **déclarations** « ce bord se coud avec celui-là » ; les
    /// longueurs, l'écart et l'embu sont calculés à la lecture ; absent du fichier si vide.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub seams: Vec<crate::seam_pair::SeamPair>,
    /// Lignes de piqûre appariées (US-08.6) : **déclarations** seulement ; nombre de trous, pas et
    /// vis-à-vis sont calculés à la lecture ; absent du fichier si vide.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stitch_pairs: Vec<crate::stitch_pair::StitchPair>,
    /// Table de variables (US-03.1a) : nom, unité d'affichage et **texte** de l'expression ; les
    /// valeurs sont calculées à la lecture (`variable_values`) ; absent du fichier si vide.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub variables: Vec<crate::variable::Variable>,
    /// Gammes de tailles (US-09.1, schéma v4) : tailles ordonnées et taille de base de chaque
    /// gamme ; absent du fichier si vide (projet sans gradation).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sizes: Vec<crate::size::SizeRange>,
    /// Règles de gradation nommées (schéma v5, décision 334) : stockage seulement, aucune
    /// commande ni interface avant US-09.3b ; absent du fichier si vide.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub grading_rules: Vec<crate::grading_rule::GradingRule>,
    /// Barème de mesures : mesures × tailles (schéma v5, US-10.1) ; absent du fichier si vide.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub measurements: Vec<crate::measurement::Measurement>,
    /// Fiches client : jeux de mesures individuels (schéma v5, US-10.2) ; absent si vide.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub clients: Vec<crate::measurement::Client>,
    /// Retouches nommées (schéma v5, US-10.3) ; absent du fichier si vide.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub alterations: Vec<crate::measurement::Alteration>,
    /// Mesures du vêtement fini et leurs tolérances (schéma v5, US-10.4) ; absent si vide.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub finished_measures: Vec<crate::measurement::FinishedMeasure>,
    /// Plans de coupe (schéma v5, socle d'E15) : laize, lisières et pièces posées ; état posé par
    /// l'utilisateur, rien de dérivé n'est stocké ; absent du fichier si vide.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cutting_plans: Vec<crate::cutting_plan::CuttingPlan>,
    /// Variantes du modèle (schéma 5, US-11.1) : sous-ensembles de pièces **référencées** avec
    /// quantités, matières et options ; rien de dérivé n'est stocké ; absent du fichier si vide.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub variants: Vec<crate::variant::Variant>,
    /// Conventions de nommage (schéma 5, US-11.5) : règles préfixe + compteur, appliquées à la
    /// demande seulement ; aucun état de compteur n'est stocké ; absent du fichier si vide.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub naming_rules: Vec<crate::naming::NamingRule>,
    /// Remarques écrites par la migration d'un ancien fichier, conservées à
    /// l'enregistrement et affichables (ex. pièce migrée sans contour).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub migration_notes: Vec<String>,
    /// Champs inconnus conservés tels quels (emplacement réservé aux clés des versions futures).
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sheet {
    pub id: SheetId,
    pub name: String,
    pub points: Vec<Point>,
    pub entities: Vec<Entity>,
    /// Crans posés sur les tracés (ajout facultatif au format v2 : absent = aucun).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notches: Vec<Notch>,
    /// Cotations (mesures affichées, non contraintes ; absent = aucune).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dimensions: Vec<Dimension>,
    /// Calques, du plus bas au plus haut (facultatif : absent = aucun calque,
    /// tous les objets sont dans le calque par défaut).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub layers: Vec<Layer>,
    /// Groupes de tracés (facultatif : absent = aucun groupe).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub groups: Vec<Group>,
    /// Lignes d'aide infinies (US-02.8 ; facultatif : absent = aucune).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub guides: Vec<Guide>,
    /// Surpiqûres liées à un bord (US-06.8) ; facultatif : absent = aucune.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub topstitches: Vec<crate::topstitch::Topstitch>,
    /// Cotes complètes (US-06.7) : longueur développée, angle, rayon, projections ; mesures
    /// observées, jamais des contraintes ; facultatif : absent = aucune.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub full_dimensions: Vec<crate::full_dimension::FullDimension>,
    /// Formules liées à des paramètres numériques de points construits (US-03.1b) : seul le texte
    /// est stocké, la valeur évaluée reste dans la définition du point ; facultatif.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub formulas: Vec<crate::formula::ParamFormula>,
    /// Longueurs développées imposées à des courbes de Bézier (US-03.3) : une par courbe, l'inconnue
    /// (poignée ou extrémité libre) reste écrite dans le modèle ; facultatif.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub length_constraints: Vec<crate::length_constraint::CurveLength>,
    /// Écarts de gradation par (point libre, taille) (US-09.2a, schéma v4) : position en taille T =
    /// position écrite + écart cumulé ; la taille de base n'a jamais d'entrée ; facultatif.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub gradings: Vec<crate::grading::PointGrading>,
    /// Image de fond calibrée (schéma v6, E14) : stockage seulement, l'image n'est jamais intégrée
    /// au fichier ; facultatif : absent = aucune.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<crate::sheet_background::Background>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// Ligne d'aide infinie d'une feuille : droite passant par `(x_mm, y_mm)`,
/// dirigée par `angle_deg` (0 = horizontale, 90 = verticale). Jamais exportée
/// par défaut ; accrochable ; `axis` la désigne comme axe de symétrie.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Guide {
    pub id: GuideId,
    pub x_mm: f64,
    pub y_mm: f64,
    pub angle_deg: f64,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub axis: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<LayerId>,
}

/// Groupe de tracés d'une feuille : on le sélectionne et le transforme d'un
/// bloc. Un tracé appartient à au plus un groupe.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Group {
    pub id: GroupId,
    pub members: Vec<EntityId>,
}

/// Calque nommé d'une feuille. Les objets sans calque sont dans le « calque
/// par défaut » implicite : toujours visible, déverrouillé, non supprimable.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Layer {
    pub id: LayerId,
    pub name: String,
    #[serde(default = "default_true")]
    pub visible: bool,
    #[serde(default)]
    pub locked: bool,
}

fn default_true() -> bool {
    true
}

/// Un tracé du contour d'une pièce, avec son sens de parcours.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PieceEdge {
    pub entity: EntityId,
    /// Vrai si le tracé est parcouru de son arrivée vers son départ.
    #[serde(default)]
    pub reversed: bool,
}

/// Lien d'une pièce avec les tracés qui la définissent.
///
/// Lecture via [`PieceLinkWire`] (variantes à champs vides) : une variante unitaire d'un enum à
/// étiquette interne ignorerait une clé en trop (D-04). L'écriture est inchangée.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", from = "PieceLinkWire")]
pub enum PieceLink {
    /// Référence les tracés d'une feuille.
    Linked { sheet: SheetId },
    /// Pièce détachée (copie propre ; pas encore créée par l'interface).
    Detached,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum PieceLinkWire {
    Linked { sheet: SheetId },
    Detached {},
}

impl From<PieceLinkWire> for PieceLink {
    fn from(w: PieceLinkWire) -> Self {
        match w {
            PieceLinkWire::Linked { sheet } => PieceLink::Linked { sheet },
            PieceLinkWire::Detached {} => PieceLink::Detached,
        }
    }
}

/// Catégorie de matière d'une pièce (US-04.2). Vocabulaire à faire relire par une
/// modéliste ; ne pas ajouter de valeur sans décision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PieceCategory {
    /// Tissu.
    Fabric,
    /// Doublure.
    Lining,
    /// Entoilage.
    Interfacing,
    /// Cuir.
    Leather,
    /// Renfort.
    Reinforcement,
}

impl PieceCategory {
    pub const ALL: [PieceCategory; 5] = [
        PieceCategory::Fabric,
        PieceCategory::Lining,
        PieceCategory::Interfacing,
        PieceCategory::Leather,
        PieceCategory::Reinforcement,
    ];

    /// Identifiant stable, identique à la valeur écrite dans le fichier.
    pub fn key(self) -> &'static str {
        match self {
            PieceCategory::Fabric => "fabric",
            PieceCategory::Lining => "lining",
            PieceCategory::Interfacing => "interfacing",
            PieceCategory::Leather => "leather",
            PieceCategory::Reinforcement => "reinforcement",
        }
    }
}

/// Objet de la construction référencé par un élément interne de pièce (US-04.3).
/// Toujours une **référence** : jamais une copie, la pièce suit la construction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum InternalTarget {
    /// Ligne interne : un tracé de la feuille.
    Path { entity: EntityId },
    /// Repère : un point de la feuille.
    Point { point: PointId },
    /// Repère : un cran de la feuille.
    Notch { notch: NotchId },
}

/// Nature d'un élément interne. Vocabulaire à faire relire par une modéliste ;
/// ne pas ajouter de valeur sans décision. Lignes (tracés) : `Fold`, `Topstitch`,
/// `PocketPosition`, `Other`. Repères (points, crans) : `Mark`, `Buttonhole`, `Other`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InternalKind {
    /// Pli.
    Fold,
    /// Surpiqûre.
    Topstitch,
    /// Position de poche (ligne).
    PocketPosition,
    /// Marque (perçage…), repère.
    Mark,
    /// Boutonnière, repère.
    Buttonhole,
    /// Autre (ligne ou repère).
    Other,
}

impl InternalKind {
    /// Types permis pour une ligne (tracé).
    pub const LINES: [InternalKind; 4] = [
        InternalKind::Fold,
        InternalKind::Topstitch,
        InternalKind::PocketPosition,
        InternalKind::Other,
    ];
    /// Types permis pour un repère (point ou cran).
    pub const MARKS: [InternalKind; 3] = [
        InternalKind::Mark,
        InternalKind::Buttonhole,
        InternalKind::Other,
    ];

    /// Identifiant stable, identique à la valeur écrite dans le fichier.
    pub fn key(self) -> &'static str {
        match self {
            InternalKind::Fold => "fold",
            InternalKind::Topstitch => "topstitch",
            InternalKind::PocketPosition => "pocket_position",
            InternalKind::Mark => "mark",
            InternalKind::Buttonhole => "buttonhole",
            InternalKind::Other => "other",
        }
    }

    /// Ce type convient-il à cette cible (ligne ou repère) ?
    pub fn fits(self, target: InternalTarget) -> bool {
        match target {
            InternalTarget::Path { .. } => Self::LINES.contains(&self),
            InternalTarget::Point { .. } | InternalTarget::Notch { .. } => {
                Self::MARKS.contains(&self)
            }
        }
    }

    /// Type par défaut d'une ligne d'après le rôle du tracé : une pliure donne
    /// un pli, tout le reste « autre ».
    pub fn default_for_role(role: PathRole) -> InternalKind {
        match role {
            PathRole::Fold => InternalKind::Fold,
            PathRole::Topstitch => InternalKind::Topstitch,
            _ => InternalKind::Other,
        }
    }
}

/// Élément interne d'une pièce (US-04.3) : référence stable vers un objet de la
/// construction et son type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InternalElement {
    pub target: InternalTarget,
    pub kind: InternalKind,
}

/// Pièce de patron : entité du projet (schéma v3). Elle référence les tracés
/// de son contour ; elle ne les possède pas.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Piece {
    pub id: PieceId,
    pub name: String,
    /// Code ou référence de la pièce (US-04.2) ; absent = pas de code.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    pub quantity: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<String>,
    /// Catégorie de matière (US-04.2) ; absente = non renseignée.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<PieceCategory>,
    /// Commentaire libre, plusieurs lignes permises (US-04.2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    /// Coupée en paire (×2 en miroir).
    #[serde(default)]
    pub mirrored_pair: bool,
    /// Contour ordonné ; peut être vide (pièce-étiquette).
    #[serde(default)]
    pub outline: Vec<PieceEdge>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grainline: Option<EntityId>,
    /// Droit-fil avancé (US-06.5) : direction par rapport à un tracé ou biais, sens du
    /// tissu, tolérance de rotation, lignes supplémentaires ; absent du fichier quand il
    /// n'apporte rien.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grain: Option<crate::piece_grain::PieceGrain>,
    /// Pièce « sans droit-fil » (PH-5d, cuir) : l'absence de droit-fil est une propriété de la
    /// matière, pas un oubli ; plus d'avertissement « droit-fil manquant ». Exclusive avec un
    /// droit-fil (`grainline`). Omise du fichier quand faux ; `false` explicite refusé à la lecture.
    #[serde(
        default,
        skip_serializing_if = "std::ops::Not::not",
        deserialize_with = "crate::piece_no_grain::no_grain_only_true"
    )]
    pub no_grain: bool,
    /// Marge de couture (US-05.1) : valeur par défaut et surcharges par tracé ; le contour de
    /// coupe est calculé à la lecture, jamais stocké ; absente du fichier quand elle est vide.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seam: Option<crate::piece_seam::SeamAllowance>,
    /// Pièce « coupe nette » (US-05.6, bord franc) : le contour de coupe **est** le contour de
    /// couture, déclaré explicitement (≠ pas encore de marge). Exclusive avec `seam`. Omise du
    /// fichier quand faux ; `false` explicite refusé à la lecture.
    #[serde(
        default,
        skip_serializing_if = "std::ops::Not::not",
        deserialize_with = "crate::piece_seam::net_cut_only_true"
    )]
    pub net_cut: bool,
    /// Bords du contour déclarés « sur le pli » (PH-5b) : tracés rectilignes du contour, sans marge
    /// de couture ni coupe, dessinés comme pliure à l'export ; triés par identifiant ; absent du
    /// fichier si vide. Une référence, jamais une copie : le bord suit la construction.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub on_fold: Vec<EntityId>,
    /// Éléments internes liés (US-04.3) : lignes et repères de la construction ;
    /// absent du fichier si vide.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub internal: Vec<InternalElement>,
    /// Perçages, pointages et croix (US-06.3) : marquages intérieurs exportés ;
    /// absent du fichier si vide.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub markings: Vec<crate::piece_marking::PieceMarking>,
    /// Pinces (US-07.1) : sommet, axe, longueur, ouverture ; pieds, fermeture et capuchon sont
    /// calculés à la lecture ; absent du fichier si vide.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub darts: Vec<crate::piece_dart::PieceDart>,
    /// Plis (US-07.3) : définitions des plis déjà **écartés** dans le contour ; la pièce repliée
    /// est calculée à la lecture ; absent du fichier si vide.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pleats: Vec<crate::piece_pleat::PiecePleat>,
    /// Lignes d'ampleur (US-07.4) : pivot et lèvres de chaque ouverture en éventail déjà faite dans
    /// le contour ; ouverture, angle et longueur sont calculés ; absent du fichier si vide.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub flares: Vec<crate::piece_flare::PieceFlare>,
    /// Groupes de boutons ou de boutonnières répartis entre deux points (US-06.4) :
    /// exportés comme marquage distinct ; absent du fichier si vide.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub buttons: Vec<crate::piece_buttons::ButtonGroup>,
    /// Lignes de piqûre (US-06.9) : définitions **liées** à un bord du contour ; positions des
    /// trous calculées à la lecture, jamais stockées ; absent du fichier si vide.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stitch_lines: Vec<crate::stitch_line::StitchLine>,
    /// Zones de parage, de collage et de découpe interne (US-06.10) : définitions seulement (bande
    /// liée à un bord ou polygone libre) ; la surface d'une bande est calculée à la lecture ;
    /// absent du fichier si vide.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub zones: Vec<crate::piece_zone::PieceZone>,
    /// Lien avec la pièce mère si c'est une pièce dérivée (US-04.4) ; absent du
    /// fichier sinon.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub derived: Option<crate::derived::DerivedSpec>,
    /// Placement manuel dans le plan de pièces (US-04.5) ; absent = placement
    /// automatique calculé à la lecture, jamais écrit dans le fichier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placement: Option<crate::piece_plan::Placement>,
    pub link: PieceLink,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// Pièce neutre : identifiant 0 (jamais valide dans un projet), nom vide, quantité 1,
/// détachée, tout le reste vide. `id` et `name` sont à fournir (`..Piece::default()`).
impl Default for Piece {
    fn default() -> Self {
        Piece {
            id: PieceId(0),
            name: String::new(),
            code: None,
            quantity: 1,
            material: None,
            category: None,
            comment: None,
            mirrored_pair: false,
            outline: Vec::new(),
            grainline: None,
            grain: None,
            no_grain: false,
            seam: None,
            net_cut: false,
            on_fold: Vec::new(),
            internal: Vec::new(),
            markings: Vec::new(),
            darts: Vec::new(),
            pleats: Vec::new(),
            flares: Vec::new(),
            buttons: Vec::new(),
            stitch_lines: Vec::new(),
            zones: Vec::new(),
            derived: None,
            placement: None,
            link: PieceLink::Detached,
            extra: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Point {
    pub id: PointId,
    pub label: String,
    pub definition: PointDefinition,
    #[serde(default)]
    pub role: PointRole,
    /// Calque ; absent = calque par défaut.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<LayerId>,
}

/// Comment la position du point est obtenue (source de vérité).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum PointDefinition {
    /// Coordonnées choisies librement.
    Free { x: f64, y: f64 },
    /// Point construit sur un segment, à une distance d'une de ses extrémités.
    /// Il est réévalué quand le segment change.
    OnSegment {
        segment: EntityId,
        distance_mm: f64,
        origin: SegmentEnd,
    },
    /// Point à une proportion du segment (0 = départ, 0,5 = milieu, 1 = arrivée).
    OnSegmentRatio { segment: EntityId, ratio: f64 },
    /// Intersection des droites qui portent deux segments. Reste définie même
    /// si les segments ne se touchent plus ; invalide s'ils deviennent parallèles.
    Intersection { first: EntityId, second: EntityId },
    /// Point d'une courbe au paramètre `t` (0 = départ, 1 = arrivée).
    OnCurve { curve: EntityId, t: f64 },
    /// Intersection de deux tracés dont au moins un est un cercle ou un arc
    /// (un arc est prolongé en cercle entier, un segment en droite). Quand il y
    /// a deux solutions, c'est celle la plus proche de `near` — le point visé à
    /// la création — qui est retenue, à chaque réévaluation.
    CircleIntersection {
        first: EntityId,
        second: EntityId,
        near: AimMm,
    },
    /// Intersection **géométrique** de deux tracés bornés quelconques (segment,
    /// Bézier, courbe lisse, arc, cercle) : un segment n'est pas prolongé, un arc
    /// non plus (US-02.4). Parmi les solutions, la plus proche de `near` — le
    /// point visé à la création — est reprise à chaque réévaluation.
    TraceIntersection {
        first: EntityId,
        second: EntityId,
        near: AimMm,
    },
    /// Point à `distance_mm` de longueur d'arc le long d'un tracé (Bézier, courbe
    /// lisse, arc, cercle, segment), mesurée depuis `origin` (US-02.5).
    AlongCurve {
        curve: EntityId,
        distance_mm: f64,
        origin: SegmentEnd,
    },
    /// Point décalé de `(dx_mm, dy_mm)` depuis un autre point (US-02.5).
    Offset {
        from: PointId,
        dx_mm: f64,
        dy_mm: f64,
    },
    /// Milieu de deux points (US-02.5).
    Midpoint { first: PointId, second: PointId },
    /// Pied de la perpendiculaire abaissée d'un point sur un tracé ; un segment
    /// est prolongé en droite (US-02.5).
    Projection { point: PointId, trace: EntityId },
    /// Point de la bissectrice intérieure de l'angle `first`–`vertex`–`second`,
    /// à `distance_mm` du sommet (US-02.5).
    Bisector {
        vertex: PointId,
        first: PointId,
        second: PointId,
        distance_mm: f64,
    },
    /// Point à `distance_mm` de `from`, dans la direction `angle_deg` : absolue
    /// (0° = +X) sans `reference`, sinon relative à la direction du segment
    /// `reference` (US-02.5).
    Polar {
        from: PointId,
        distance_mm: f64,
        angle_deg: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reference: Option<EntityId>,
    },
    /// Point d'arcs de compas : cercle de centre `center_a` et rayon `radius_a_mm`,
    /// cercle de centre `center_b` et rayon `radius_b_mm` ; la solution la plus
    /// proche de `near` est reprise à chaque réévaluation (US-02.5).
    Compass {
        center_a: PointId,
        center_b: PointId,
        radius_a_mm: f64,
        radius_b_mm: f64,
        near: AimMm,
    },
    /// Point de division d'un tracé (US-02.6) : situé à la fraction `ratio`
    /// (strictement entre 0 et 1) de la **longueur d'arc** depuis le départ du
    /// tracé ; la fraction est maintenue quand le tracé change de longueur. Sur
    /// un cercle, `start` (point de départ, facultatif) fixe l'origine des angles :
    /// la fraction est alors une fraction de tour depuis la direction centre → `start`.
    Division {
        trace: EntityId,
        ratio: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        start: Option<PointId>,
    },
    /// Image de `source` par réflexion exacte sur `axis` (US-02.11) : point
    /// maintenu par symétrie, jamais déplacé directement.
    Mirror { source: PointId, axis: MirrorAxis },
}

/// Axe d'une symétrie liée : ligne d'aide marquée « axe », ou segment de
/// référence (US-02.11).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum MirrorAxis {
    Guide { guide: GuideId },
    Segment { segment: EntityId },
}

/// Position visée (mm) mémorisée par un point d'intersection : sert seulement à
/// choisir entre deux solutions. Les clés inconnues sont refusées à la lecture.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AimMm {
    pub x: f64,
    pub y: f64,
}

impl From<PointMm> for AimMm {
    fn from(p: PointMm) -> Self {
        AimMm { x: p.x, y: p.y }
    }
}

impl AimMm {
    pub fn point(self) -> PointMm {
        PointMm::new(self.x, self.y)
    }
}

impl PointDefinition {
    /// Tracés dont dépend la position du point.
    pub fn dependencies(&self) -> Vec<EntityId> {
        match *self {
            PointDefinition::Free { .. } => Vec::new(),
            PointDefinition::OnSegment { segment, .. }
            | PointDefinition::OnSegmentRatio { segment, .. } => vec![segment],
            PointDefinition::Intersection { first, second }
            | PointDefinition::CircleIntersection { first, second, .. }
            | PointDefinition::TraceIntersection { first, second, .. } => vec![first, second],
            PointDefinition::OnCurve { curve, .. } | PointDefinition::AlongCurve { curve, .. } => {
                vec![curve]
            }
            PointDefinition::Projection { trace, .. } | PointDefinition::Division { trace, .. } => {
                vec![trace]
            }
            PointDefinition::Polar { reference, .. } => reference.into_iter().collect(),
            PointDefinition::Mirror {
                axis: MirrorAxis::Segment { segment },
                ..
            } => vec![segment],
            PointDefinition::Mirror { .. }
            | PointDefinition::Offset { .. }
            | PointDefinition::Midpoint { .. }
            | PointDefinition::Bisector { .. }
            | PointDefinition::Compass { .. } => Vec::new(),
        }
    }

    /// Points parents dont dépend la position (US-02.5).
    pub fn point_dependencies(&self) -> Vec<PointId> {
        match *self {
            PointDefinition::Offset { from, .. } | PointDefinition::Polar { from, .. } => {
                vec![from]
            }
            PointDefinition::Projection { point, .. } => vec![point],
            PointDefinition::Midpoint { first, second } => vec![first, second],
            PointDefinition::Bisector {
                vertex,
                first,
                second,
                ..
            } => vec![vertex, first, second],
            PointDefinition::Compass {
                center_a, center_b, ..
            } => vec![center_a, center_b],
            PointDefinition::Division { start, .. } => start.into_iter().collect(),
            PointDefinition::Mirror { source, .. } => vec![source],
            _ => Vec::new(),
        }
    }

    /// Ligne d'aide servant d'axe, pour une symétrie liée.
    pub fn axis_guide(&self) -> Option<GuideId> {
        match *self {
            PointDefinition::Mirror {
                axis: MirrorAxis::Guide { guide },
                ..
            } => Some(guide),
            _ => None,
        }
    }

    pub fn is_mirror(&self) -> bool {
        matches!(self, PointDefinition::Mirror { .. })
    }

    pub fn is_free(&self) -> bool {
        matches!(self, PointDefinition::Free { .. })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SegmentEnd {
    Start,
    End,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PointRole {
    #[default]
    Construction,
    /// Point de repère (montage, annotation).
    Reference,
}

/// Rôle métier d'un tracé. Il est porté par ce champ, jamais par une couleur.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PathRole {
    #[default]
    Construction,
    SewingContour,
    CuttingContour,
    Grainline,
    Fold,
    AnnotationAxis,
    /// Surpiqûre (US-06.8) : trait de piqûre apparent ; peut être générée par décalage
    /// d'un bord ([`crate::topstitch::Topstitch`]) ou simplement dessinée.
    Topstitch,
    /// Milieu devant (US-06.8).
    CenterFront,
    /// Milieu dos (US-06.8).
    CenterBack,
    /// Ligne de taille (US-06.8).
    WaistLine,
    /// Ligne de hanche (US-06.8).
    HipLine,
    /// Ligne de poitrine (US-06.8).
    BustLine,
    /// Ligne de parage, maroquinerie (US-06.8).
    SkivingLine,
    /// Ligne de collage, maroquinerie (US-06.8).
    GlueLine,
}

impl PathRole {
    /// Tous les rôles, dans l'ordre d'affichage : les six historiques puis les lignes
    /// spéciales (US-06.8).
    pub const ALL: [PathRole; 14] = [
        PathRole::Construction,
        PathRole::SewingContour,
        PathRole::CuttingContour,
        PathRole::Grainline,
        PathRole::Fold,
        PathRole::AnnotationAxis,
        PathRole::Topstitch,
        PathRole::CenterFront,
        PathRole::CenterBack,
        PathRole::WaistLine,
        PathRole::HipLine,
        PathRole::BustLine,
        PathRole::SkivingLine,
        PathRole::GlueLine,
    ];

    /// Les six rôles d'avant US-06.8 : la légende de la page étalon les liste toujours.
    pub const BASE: [PathRole; 6] = [
        PathRole::Construction,
        PathRole::SewingContour,
        PathRole::CuttingContour,
        PathRole::Grainline,
        PathRole::Fold,
        PathRole::AnnotationAxis,
    ];

    /// Les lignes spéciales d'US-06.8 (légende de la page étalon : seulement si utilisées).
    pub const SPECIAL: [PathRole; 8] = [
        PathRole::Topstitch,
        PathRole::CenterFront,
        PathRole::CenterBack,
        PathRole::WaistLine,
        PathRole::HipLine,
        PathRole::BustLine,
        PathRole::SkivingLine,
        PathRole::GlueLine,
    ];

    /// Ligne spéciale d'US-06.8 ?
    pub fn is_special(self) -> bool {
        Self::SPECIAL.contains(&self)
    }

    pub fn label_fr(self) -> &'static str {
        match self {
            PathRole::Construction => "Construction",
            PathRole::SewingContour => "Ligne de couture",
            PathRole::CuttingContour => "Ligne de coupe",
            PathRole::Grainline => "Droit-fil",
            PathRole::Fold => "Pliure",
            PathRole::AnnotationAxis => "Axe de repère",
            PathRole::Topstitch => "Surpiqûre",
            PathRole::CenterFront => "Milieu devant",
            PathRole::CenterBack => "Milieu dos",
            PathRole::WaistLine => "Ligne de taille",
            PathRole::HipLine => "Ligne de hanche",
            PathRole::BustLine => "Ligne de poitrine",
            PathRole::SkivingLine => "Ligne de parage",
            PathRole::GlueLine => "Ligne de collage",
        }
    }
}

/// Décalage relatif en mm (poignée de courbe par rapport à son extrémité).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OffsetMm {
    pub dx: f64,
    pub dy: f64,
}

impl OffsetMm {
    pub fn vector(self) -> VectorMm {
        VectorMm::new(self.dx, self.dy)
    }
    pub fn from_vector(v: VectorMm) -> Self {
        Self { dx: v.x, dy: v.y }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entity {
    pub id: EntityId,
    #[serde(default)]
    pub role: PathRole,
    pub kind: EntityKind,
    /// Calque ; absent = calque par défaut.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<LayerId>,
    /// Nom facultatif, unique dans la feuille ; absent = pas de nom.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum EntityKind {
    Segment {
        from: PointId,
        to: PointId,
        /// Paramètres saisis à la création. Ce n'est **pas** une contrainte :
        /// ils ne sont pas maintenus si un point est déplacé ensuite.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        construction: Option<Construction>,
    },
    /// Bézier cubique ; les poignées sont relatives à leur extrémité et la
    /// suivent donc quand elle se déplace.
    Curve {
        start: PointId,
        handle_start: OffsetMm,
        handle_end: OffsetMm,
        end: PointId,
    },
    /// Cercle entier : le centre est un point, le rayon un paramètre. Il a un
    /// point de départ (angle `start_deg`) et un sens de parcours, qui portent
    /// les crans et suivent rotations et symétries.
    Circle {
        center: PointId,
        radius_mm: f64,
        #[serde(default)]
        start_deg: f64,
        #[serde(default = "default_true")]
        ccw: bool,
    },
    /// Arc de cercle de `start` à `end` autour de `center` ; le rayon est la
    /// distance centre → départ et `end` doit être à la même distance.
    Arc {
        center: PointId,
        start: PointId,
        end: PointId,
        #[serde(default = "default_true")]
        ccw: bool,
    },
    /// Courbe par points de passage (US-02.2) : spline cubique d'Hermite par
    /// morceaux, au moins deux points distincts, chacun une seule fois.
    Spline { nodes: Vec<SplineNode> },
}

/// Nombre maximal de points de passage d'une courbe.
pub const MAX_SPLINE_NODES: usize = 1000;

/// Point de passage d'une courbe, avec ses tangentes (US-02.2).
///
/// Les tangentes sont des vecteurs d'Hermite en mm (dérivée par rapport au
/// paramètre de chaque tronçon, soit 3 × la poignée de Bézier). Elles ne
/// dépendent pas de la position : elles suivent les translations du point.
/// `None` = tangente calculée (Catmull-Rom en point lisse, corde en point
/// anguleux). Un **point lisse** a une seule tangente (`tangent`, sortante et
/// entrante) : la courbe est C1. Un **point anguleux** (`corner`, jamais aux
/// extrémités) a `tangent` sortante et `tangent_in` entrante, indépendantes.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SplineNode {
    pub point: PointId,
    #[serde(default, skip_serializing_if = "is_false")]
    pub corner: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tangent: Option<OffsetMm>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tangent_in: Option<OffsetMm>,
}

fn is_false(b: &bool) -> bool {
    !*b
}

impl SplineNode {
    pub fn new(point: PointId) -> SplineNode {
        SplineNode {
            point,
            corner: false,
            tangent: None,
            tangent_in: None,
        }
    }
}

/// Point de passage résolu : position et tangentes effectives, et si chacune
/// vient d'une saisie (`true`) ou du calcul par défaut (`false`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResolvedSplineNode {
    pub point: PointId,
    pub position: PointMm,
    pub corner: bool,
    pub tangent_in: VectorMm,
    pub tangent_out: VectorMm,
    pub tangent_in_explicit: bool,
    pub tangent_out_explicit: bool,
}

/// Géométrie résolue d'un tracé (mm).
#[derive(Debug, Clone, PartialEq)]
pub enum Shape {
    Line(PointMm, PointMm),
    Cubic(CubicBezier),
    Arc(Arc),
    Circle(CircleTrace),
    Spline(Spline),
}

impl Shape {
    /// Tracé borné pour `pf_geometry::intersect`.
    pub fn to_trace(&self) -> pf_geometry::Trace {
        match self {
            Shape::Line(a, b) => pf_geometry::Trace::Line(*a, *b),
            Shape::Cubic(c) => pf_geometry::Trace::Cubic(*c),
            Shape::Arc(a) => pf_geometry::Trace::Arc(*a),
            Shape::Circle(c) => pf_geometry::Trace::Circle(c.circle),
            Shape::Spline(s) => pf_geometry::Trace::Spline(s.clone()),
        }
    }

    /// Polyligne d'approximation (affichage, tests de proximité).
    pub fn sample(&self, n: usize) -> Vec<PointMm> {
        match self {
            Shape::Line(a, b) => vec![*a, *b],
            Shape::Cubic(c) => c.sample(n),
            Shape::Arc(a) => a.sample(n),
            Shape::Circle(c) => c.sample(n.max(32)),
            Shape::Spline(s) => s.sample(n.div_ceil(s.segments().len()).max(8)),
        }
    }

    pub fn bbox(&self) -> Bbox {
        match self {
            Shape::Line(a, b) => {
                let mut bb = Bbox::from_point(*a);
                bb.include(*b);
                bb
            }
            Shape::Cubic(c) => c.bbox(),
            Shape::Arc(a) => a.bbox(),
            Shape::Circle(c) => c.circle.bbox(),
            Shape::Spline(s) => s.bbox(),
        }
    }

    pub fn distance_to_point(&self, p: PointMm) -> f64 {
        match self {
            Shape::Line(a, b) => pf_geometry::project_on_segment(p, *a, *b)
                .map(|r| r.1)
                .unwrap_or(f64::INFINITY),
            Shape::Cubic(c) => c.distance_to_point(p),
            Shape::Arc(a) => a.distance_to_point(p),
            Shape::Circle(c) => c.circle.distance_to_point(p),
            Shape::Spline(s) => s.distance_to_point(p),
        }
    }
}

/// Paramètres de construction mémorisés (provenance). Par défaut ils ne sont **pas** maintenus ;
/// avec `kept` (US-03.2, `Command::SetConstructionKept`) le ou les points construits suivent
/// leur référence (voir `kept.rs`). `kept` est omis du fichier quand il est faux.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Construction {
    Polar {
        length_mm: f64,
        angle_deg: f64,
        #[serde(default, skip_serializing_if = "is_false")]
        kept: bool,
    },
    Perpendicular {
        reference: EntityId,
        length_mm: f64,
        side: Side,
        #[serde(default, skip_serializing_if = "is_false")]
        kept: bool,
    },
    /// Segment parallèle à `reference`, décalé de `distance_mm` du côté `side`.
    Parallel {
        reference: EntityId,
        distance_mm: f64,
        side: Side,
        #[serde(default, skip_serializing_if = "is_false")]
        kept: bool,
    },
}

impl Construction {
    /// Le segment est-il maintenu par sa construction (US-03.2) ?
    pub fn is_kept(self) -> bool {
        match self {
            Construction::Polar { kept, .. }
            | Construction::Perpendicular { kept, .. }
            | Construction::Parallel { kept, .. } => kept,
        }
    }

    /// Même construction, avec l'état « maintenu » donné.
    pub fn with_kept(self, value: bool) -> Construction {
        match self {
            Construction::Polar {
                length_mm,
                angle_deg,
                ..
            } => Construction::Polar {
                length_mm,
                angle_deg,
                kept: value,
            },
            Construction::Perpendicular {
                reference,
                length_mm,
                side,
                ..
            } => Construction::Perpendicular {
                reference,
                length_mm,
                side,
                kept: value,
            },
            Construction::Parallel {
                reference,
                distance_mm,
                side,
                ..
            } => Construction::Parallel {
                reference,
                distance_mm,
                side,
                kept: value,
            },
        }
    }

    /// Segment de référence (perpendiculaire, parallèle).
    pub fn reference(self) -> Option<EntityId> {
        match self {
            Construction::Polar { .. } => None,
            Construction::Perpendicular { reference, .. }
            | Construction::Parallel { reference, .. } => Some(reference),
        }
    }
}

impl Project {
    /// Nouveau projet avec une première feuille vide.
    pub fn new(name: &str) -> Project {
        let mut p = Project {
            format: FORMAT_TAG.to_string(),
            schema_version: SCHEMA_VERSION,
            name: name.to_string(),
            next_id: 1,
            sheets: Vec::new(),
            pieces: Vec::new(),
            seam_defaults: None,
            seams: Vec::new(),
            stitch_pairs: Vec::new(),
            variables: Vec::new(),
            sizes: Vec::new(),
            grading_rules: Vec::new(),
            measurements: Vec::new(),
            clients: Vec::new(),
            alterations: Vec::new(),
            finished_measures: Vec::new(),
            cutting_plans: Vec::new(),
            variants: Vec::new(),
            naming_rules: Vec::new(),
            migration_notes: Vec::new(),
            extra: BTreeMap::new(),
        };
        let id = SheetId(p.alloc_id());
        p.sheets.push(Sheet::new(id, "Feuille 1"));
        p
    }

    pub(crate) fn alloc_id(&mut self) -> u64 {
        let id = self.next_id;
        // Saturation : un compteur au maximum (fichier forgé) donne ensuite un
        // identifiant >= next_id, refusé par `validate()` au lieu de déborder.
        self.next_id = self.next_id.saturating_add(1);
        id
    }

    pub fn sheet(&self, id: SheetId) -> Result<&Sheet, CoreError> {
        self.sheets
            .iter()
            .find(|s| s.id == id)
            .ok_or(CoreError::UnknownSheet(id))
    }

    pub(crate) fn sheet_mut(&mut self, id: SheetId) -> Result<&mut Sheet, CoreError> {
        self.sheets
            .iter_mut()
            .find(|s| s.id == id)
            .ok_or(CoreError::UnknownSheet(id))
    }

    pub fn piece(&self, id: PieceId) -> Result<&Piece, CoreError> {
        self.pieces
            .iter()
            .find(|p| p.id == id)
            .ok_or(CoreError::UnknownPiece(id))
    }

    pub(crate) fn piece_mut(&mut self, id: PieceId) -> Result<&mut Piece, CoreError> {
        self.pieces
            .iter_mut()
            .find(|p| p.id == id)
            .ok_or(CoreError::UnknownPiece(id))
    }

    /// Pièces liées à une feuille, dans l'ordre du projet.
    pub fn pieces_of(&self, sheet: SheetId) -> impl Iterator<Item = &Piece> {
        self.pieces
            .iter()
            .filter(move |p| p.link == PieceLink::Linked { sheet })
    }

    /// Valide l'intégralité du projet : identifiants, noms, références,
    /// géométries dégénérées. Appelée après chaque commande et à l'ouverture.
    pub fn validate(&self) -> Result<(), CoreError> {
        if self.format != FORMAT_TAG {
            return Err(CoreError::NotAPatternForgeFile);
        }
        if self.schema_version != SCHEMA_VERSION {
            return Err(CoreError::UnsupportedVersion {
                found: self.schema_version,
                supported: SCHEMA_VERSION,
            });
        }
        check_name(&self.name, "le nom du projet")?;
        if self.sheets.is_empty() {
            return Err(CoreError::Invalid(
                "le projet ne contient aucune feuille".into(),
            ));
        }
        let mut seen = HashSet::new();
        let mut check_id = |id: u64| -> Result<(), CoreError> {
            if id == 0 || id >= self.next_id {
                return Err(CoreError::Invalid(format!(
                    "identifiant {id} incohérent avec le compteur du projet ({})",
                    self.next_id
                )));
            }
            if !seen.insert(id) {
                return Err(CoreError::DuplicateId(id));
            }
            Ok(())
        };
        for sheet in &self.sheets {
            check_id(sheet.id.0)?;
            for p in &sheet.points {
                check_id(p.id.0)?;
            }
            for e in &sheet.entities {
                check_id(e.id.0)?;
            }
            for n in &sheet.notches {
                check_id(n.id.0)?;
            }
            for d in &sheet.dimensions {
                check_id(d.id.0)?;
            }
            for l in &sheet.layers {
                check_id(l.id.0)?;
            }
            for g in &sheet.groups {
                check_id(g.id.0)?;
            }
            for g in &sheet.guides {
                check_id(g.id.0)?;
            }
            for t in &sheet.topstitches {
                check_id(t.id.0)?;
            }
            for d in &sheet.full_dimensions {
                check_id(d.id.0)?;
            }
        }
        for piece in &self.pieces {
            check_id(piece.id.0)?;
            for m in &piece.markings {
                check_id(m.id.0)?;
            }
            for g in &piece.buttons {
                check_id(g.id.0)?;
            }
            for l in &piece.stitch_lines {
                check_id(l.id.0)?;
            }
            for z in &piece.zones {
                check_id(z.id.0)?;
            }
            for d in &piece.darts {
                check_id(d.id.0)?;
            }
            for l in &piece.pleats {
                check_id(l.id.0)?;
            }
            for l in &piece.flares {
                check_id(l.id.0)?;
            }
        }
        for s in &self.seams {
            check_id(s.id.0)?;
        }
        for s in &self.stitch_pairs {
            check_id(s.id.0)?;
        }
        for r in &self.sizes {
            check_id(r.id.0)?;
            for s in &r.sizes {
                check_id(s.id.0)?;
            }
        }
        for r in &self.grading_rules {
            check_id(r.id.0)?;
        }
        for m in &self.measurements {
            check_id(m.id.0)?;
        }
        for c in &self.clients {
            check_id(c.id.0)?;
        }
        for a in &self.alterations {
            check_id(a.id.0)?;
        }
        for f in &self.finished_measures {
            check_id(f.id.0)?;
        }
        for c in &self.cutting_plans {
            check_id(c.id.0)?;
            for p in &c.placements {
                check_id(p.id.0)?;
            }
            if let Some(h) = &c.hide {
                for d in &h.defects {
                    check_id(d.id.0)?;
                }
                for z in &h.quality_zones {
                    check_id(z.id.0)?;
                }
            }
        }
        for v in &self.variants {
            check_id(v.id.0)?;
            for e in &v.entries {
                check_id(e.id.0)?;
            }
        }
        for r in &self.naming_rules {
            check_id(r.id.0)?;
        }
        // Les positions calculées sont retenues pour toute la suite de la validation, qui ne
        // modifie rien : sans cela, une longue chaîne de constructions coûte son carré.
        let _caches: Vec<_> = self
            .sheets
            .iter()
            .map(crate::sheet_index::IndexGuard::new)
            .collect();
        for sheet in &self.sheets {
            sheet.validate()?;
        }
        for piece in &self.pieces {
            self.validate_piece(piece)?;
        }
        self.validate_seam_pairs()?;
        self.validate_stitch_pairs()?;
        self.validate_topstitches()?;
        self.validate_seam_defaults()?;
        self.validate_variables()?;
        self.validate_sizes()?;
        self.validate_variable_sizes()?;
        self.validate_grading_rules()?;
        self.validate_measurements()?;
        self.validate_formulas()?;
        self.validate_kept()?;
        self.validate_length_constraints()?;
        self.validate_gradings()?;
        self.validate_cutting_plans()?;
        self.validate_hides()?;
        self.validate_backgrounds()?;
        self.validate_variants()?;
        self.validate_naming_rules()?;
        Ok(())
    }

    pub(crate) fn validate_piece(&self, piece: &Piece) -> Result<(), CoreError> {
        check_name(&piece.name, "le nom de la pièce")?;
        if piece.quantity == 0 {
            return Err(CoreError::Invalid(
                "la quantité d'une pièce doit être ≥ 1".into(),
            ));
        }
        if let Some(m) = &piece.material {
            check_name(m, "la matière de la pièce")?;
        }
        if let Some(c) = &piece.code {
            check_name(c, "le code de la pièce")?;
        }
        if let Some(c) = &piece.comment {
            check_comment(c)?;
        }
        // Un champ typé de la fiche ne peut pas coexister avec une valeur
        // homonyme conservée dans `extra` (écriture de deux clés identiques).
        if let Some(k) = [
            "code",
            "category",
            "comment",
            "internal",
            "markings",
            "darts",
            "pleats",
            "flares",
            "buttons",
            "stitch_lines",
            "zones",
            "derived",
            "placement",
            "grain",
            "no_grain",
            "seam",
            "net_cut",
            "on_fold",
        ]
        .into_iter()
        .find(|k| piece.extra.contains_key(*k))
        {
            return Err(CoreError::Invalid(format!(
                "la pièce « {} » porte un champ « {k} » non conforme à la fiche de pièce",
                piece.name
            )));
        }
        let linked_sheet = match piece.link {
            PieceLink::Linked { sheet } => Some(self.sheet(sheet).map_err(|_| {
                CoreError::DeadReference(format!(
                    "la pièce « {} » est liée à une feuille absente (id {})",
                    piece.name, sheet.0
                ))
            })?),
            PieceLink::Detached => None,
        };
        let exists = |entity: EntityId| match linked_sheet {
            Some(s) => s.entity(entity).is_ok(),
            None => self.sheets.iter().any(|s| s.entity(entity).is_ok()),
        };
        let mut seen = HashSet::new();
        for edge in &piece.outline {
            if !exists(edge.entity) {
                return Err(CoreError::DeadReference(format!(
                    "le contour de la pièce « {} » référence un tracé absent (id {})",
                    piece.name, edge.entity.0
                )));
            }
            if !seen.insert(edge.entity) {
                return Err(CoreError::Invalid(format!(
                    "le contour de la pièce « {} » utilise deux fois le tracé {}",
                    piece.name, edge.entity.0
                )));
            }
        }
        if let Some(g) = piece.grainline {
            if !exists(g) {
                return Err(CoreError::DeadReference(format!(
                    "le droit-fil de la pièce « {} » référence un tracé absent (id {})",
                    piece.name, g.0
                )));
            }
        }
        self.validate_internals(piece)?;
        self.validate_markings(piece)?;
        self.validate_darts(piece)?;
        Self::validate_pleats(piece)?;
        Self::validate_flares(piece)?;
        self.validate_buttons(piece)?;
        self.validate_stitch_lines(piece)?;
        self.validate_zones(piece)?;
        self.validate_grain(piece)?;
        self.validate_seam(piece)?;
        self.validate_on_fold(piece)?;
        Self::validate_no_grain(piece)?;
        if let Some(pl) = &piece.placement {
            pl.validate_stored()
                .map_err(|e| CoreError::Invalid(format!("pièce « {} » : {e}", piece.name)))?;
        }
        self.validate_derived(piece)
    }

    /// Éléments internes d'une pièce : références vivantes et sur la feuille de la
    /// pièce, type cohérent avec la cible, pas de doublon, jamais un tracé du
    /// contour ni le droit-fil.
    fn validate_internals(&self, piece: &Piece) -> Result<(), CoreError> {
        let mut seen = HashSet::new();
        for el in &piece.internal {
            let found = self.internal_target_exists(piece.link, el.target);
            let what = crate::piece_internal::target_text(el.target);
            if !found {
                return Err(CoreError::DeadReference(format!(
                    "un élément interne de la pièce « {} » référence un objet absent de sa feuille ({what})",
                    piece.name
                )));
            }
            if !el.kind.fits(el.target) {
                return Err(CoreError::Invalid(format!(
                    "élément interne de la pièce « {} » ({what}) : le type « {} » ne convient pas à cet objet",
                    piece.name,
                    el.kind.key()
                )));
            }
            if !seen.insert(el.target) {
                return Err(CoreError::Invalid(format!(
                    "la pièce « {} » liste deux fois le même élément interne ({what})",
                    piece.name
                )));
            }
            if let InternalTarget::Path { entity } = el.target {
                if piece.outline.iter().any(|e| e.entity == entity)
                    || piece.grainline == Some(entity)
                {
                    return Err(CoreError::Invalid(format!(
                        "{what} fait partie du contour ou du droit-fil de la pièce « {} » : \
                         il ne peut pas être aussi un élément interne",
                        piece.name
                    )));
                }
            }
        }
        Ok(())
    }
}

pub(crate) fn check_name(name: &str, what: &str) -> Result<(), CoreError> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(CoreError::Invalid(format!("{what} ne peut pas être vide")));
    }
    if name.chars().count() > MAX_NAME_LEN {
        return Err(CoreError::Invalid(format!(
            "{what} dépasse {MAX_NAME_LEN} caractères"
        )));
    }
    if name.chars().any(|c| c.is_control()) {
        return Err(CoreError::Invalid(format!(
            "{what} contient un caractère de contrôle"
        )));
    }
    Ok(())
}

/// Commentaire de pièce : non vide, borné, sauts de ligne permis mais aucun autre
/// caractère de contrôle (NUL, tabulation, retour chariot…).
pub(crate) fn check_comment(text: &str) -> Result<(), CoreError> {
    if text.trim().is_empty() {
        return Err(CoreError::Invalid(
            "le commentaire de la pièce ne peut pas être vide".into(),
        ));
    }
    if text.chars().count() > MAX_COMMENT_LEN {
        return Err(CoreError::Invalid(format!(
            "le commentaire de la pièce dépasse {MAX_COMMENT_LEN} caractères"
        )));
    }
    if text.chars().any(|c| c.is_control() && c != '\n') {
        return Err(CoreError::Invalid(
            "le commentaire de la pièce contient un caractère de contrôle".into(),
        ));
    }
    Ok(())
}

/// Profondeur à partir de laquelle une résolution isolée indexe la feuille.
const INDEXED_DEPTH: usize = 32;

/// État d'une résolution de points : points en cours (détection des cycles), résultats déjà
/// connus, parent réclamé par la définition en cours.
#[derive(Default)]
pub(crate) struct Resolution {
    active: HashSet<PointId>,
    done: HashMap<PointId, Result<PointMm, CoreError>>,
    missing: Option<PointId>,
    driving: bool,
}

impl Sheet {
    pub fn new(id: SheetId, name: &str) -> Sheet {
        Sheet {
            id,
            name: name.to_string(),
            points: Vec::new(),
            entities: Vec::new(),
            notches: Vec::new(),
            dimensions: Vec::new(),
            layers: Vec::new(),
            groups: Vec::new(),
            guides: Vec::new(),
            topstitches: Vec::new(),
            full_dimensions: Vec::new(),
            formulas: Vec::new(),
            length_constraints: Vec::new(),
            gradings: Vec::new(),
            background: None,
            extra: BTreeMap::new(),
        }
    }

    pub fn layer(&self, id: LayerId) -> Result<&Layer, CoreError> {
        if let Some(pos) = crate::sheet_index::layer_position(self, id) {
            return pos
                .and_then(|i| self.layers.get(i))
                .ok_or(CoreError::UnknownLayer(id));
        }
        self.layers
            .iter()
            .find(|l| l.id == id)
            .ok_or(CoreError::UnknownLayer(id))
    }

    /// Visible ? Le calque par défaut (`None`) l'est toujours.
    pub fn layer_visible(&self, layer: Option<LayerId>) -> bool {
        layer.is_none_or(|l| self.layer(l).map(|l| l.visible).unwrap_or(true))
    }

    /// Verrouillé ? Le calque par défaut (`None`) ne l'est jamais.
    pub fn layer_locked(&self, layer: Option<LayerId>) -> bool {
        layer.is_some_and(|l| self.layer(l).map(|l| l.locked).unwrap_or(false))
    }

    /// Calque de nom donné (nom affiché dans les messages).
    pub fn layer_name(&self, layer: Option<LayerId>) -> String {
        match layer.and_then(|l| self.layer(l).ok()) {
            Some(l) => l.name.clone(),
            None => "calque par défaut".to_string(),
        }
    }

    pub fn point_visible(&self, id: PointId) -> bool {
        self.point(id).is_ok_and(|p| self.layer_visible(p.layer))
    }

    pub fn entity_visible(&self, id: EntityId) -> bool {
        self.entity(id).is_ok_and(|e| self.layer_visible(e.layer))
    }

    /// Un cran suit le calque de son tracé.
    pub fn notch_visible(&self, n: &Notch) -> bool {
        self.entity_visible(n.entity)
    }

    /// Une cotation est masquée dès que l'un de ses points l'est.
    pub fn dimension_visible(&self, d: &Dimension) -> bool {
        self.point_visible(d.from) && self.point_visible(d.to)
    }

    pub fn point_locked(&self, id: PointId) -> bool {
        self.point(id).is_ok_and(|p| self.layer_locked(p.layer))
    }

    pub fn entity_locked(&self, id: EntityId) -> bool {
        self.entity(id).is_ok_and(|e| self.layer_locked(e.layer))
    }

    pub fn point(&self, id: PointId) -> Result<&Point, CoreError> {
        if let Some(pos) = crate::sheet_index::point_position(self, id) {
            return pos
                .and_then(|i| self.points.get(i))
                .ok_or(CoreError::UnknownPoint(id));
        }
        self.points
            .iter()
            .find(|p| p.id == id)
            .ok_or(CoreError::UnknownPoint(id))
    }

    /// Groupe qui contient ce tracé, s'il y en a un.
    pub fn group_of(&self, entity: EntityId) -> Option<&Group> {
        self.groups.iter().find(|g| g.members.contains(&entity))
    }

    /// Ajoute à la liste les membres des groupes dont un tracé est déjà listé
    /// (sans doublon, ordre d'origine conservé).
    pub fn expand_groups(&self, entities: &[EntityId]) -> Vec<EntityId> {
        let mut out = entities.to_vec();
        for &e in entities {
            if let Some(g) = self.group_of(e) {
                for &m in &g.members {
                    if !out.contains(&m) {
                        out.push(m);
                    }
                }
            }
        }
        out
    }

    pub fn group(&self, id: GroupId) -> Result<&Group, CoreError> {
        self.groups
            .iter()
            .find(|g| g.id == id)
            .ok_or(CoreError::UnknownGroup(id))
    }

    pub(crate) fn entity_mut(&mut self, id: EntityId) -> Result<&mut Entity, CoreError> {
        self.entities
            .iter_mut()
            .find(|e| e.id == id)
            .ok_or(CoreError::UnknownEntity(id))
    }

    pub fn entity(&self, id: EntityId) -> Result<&Entity, CoreError> {
        if let Some(pos) = crate::sheet_index::entity_position(self, id) {
            return pos
                .and_then(|i| self.entities.get(i))
                .ok_or(CoreError::UnknownEntity(id));
        }
        self.entities
            .iter()
            .find(|e| e.id == id)
            .ok_or(CoreError::UnknownEntity(id))
    }

    /// Étiquette d'un point, ou `?` s'il est introuvable.
    pub fn label(&self, id: PointId) -> &str {
        self.point(id).map(|p| p.label.as_str()).unwrap_or("?")
    }

    /// Nom lisible d'un tracé : « Segment AB », « Courbe AC ».
    pub fn entity_name(&self, id: EntityId) -> String {
        match self.entity(id).map(|e| e.kind.clone()) {
            Ok(EntityKind::Segment { from, to, .. }) => {
                format!("Segment {}{}", self.label(from), self.label(to))
            }
            Ok(EntityKind::Curve { start, end, .. }) => {
                format!("Courbe {}{}", self.label(start), self.label(end))
            }
            Ok(EntityKind::Circle { center, .. }) => format!("Cercle {}", self.label(center)),
            Ok(EntityKind::Arc {
                center, start, end, ..
            }) => format!(
                "Arc {}{} (centre {})",
                self.label(start),
                self.label(end),
                self.label(center)
            ),
            Ok(EntityKind::Spline { nodes }) => format!(
                "Courbe lisse {}…{}",
                nodes.first().map_or("?", |n| self.label(n.point)),
                nodes.last().map_or("?", |n| self.label(n.point))
            ),
            Err(_) => "Tracé introuvable".to_string(),
        }
    }

    /// Position résolue d'un point, en mm.
    pub fn position(&self, id: PointId) -> Result<PointMm, CoreError> {
        self.position_rec(id, &mut Resolution::default())
    }

    /// Exécute `f` avec un cache des positions et un index des objets de la feuille : chaque
    /// point n'est calculé qu'une fois. À utiliser pour toute lecture de masse (la feuille est
    /// empruntée tant que `f` s'exécute, le cache ne peut donc pas se périmer).
    pub fn with_read_cache<T>(&self, f: impl FnOnce() -> T) -> T {
        let _cache = crate::sheet_index::IndexGuard::new(self);
        f()
    }

    /// Position d'un point pendant une résolution. Ne se rappelle jamais elle-même à travers
    /// les parents : la pile du programme ne dépend pas de la longueur des constructions.
    pub(crate) fn position_rec(
        &self,
        id: PointId,
        path: &mut Resolution,
    ) -> Result<PointMm, CoreError> {
        if let Some(r) = path.done.get(&id) {
            return r.clone();
        }
        if let Some(r) = crate::sheet_index::cached_position(self, id) {
            return r;
        }
        if path.active.contains(&id) {
            return Err(CoreError::CyclicReference(id));
        }
        // Un point libre n'a aucun parent : ni pile, ni cache à entretenir.
        let point = self.point(id)?;
        if point.definition.is_free() {
            return self.evaluate_rec(&point.definition, &point.label, path);
        }
        if path.driving {
            // Parent pas encore calculé : `resolve` le calculera puis reprendra la définition
            // en cours. Cette erreur ne sert qu'à l'interrompre, elle n'est jamais retournée.
            path.missing = Some(id);
            return Err(CoreError::UnknownPoint(id));
        }
        self.resolve(id, path)
    }

    /// Parcours en profondeur à **pile explicite**, dans l'ordre exact où les définitions
    /// demandent leurs parents (mêmes valeurs et mêmes erreurs qu'une récursion).
    fn resolve(&self, root: PointId, path: &mut Resolution) -> Result<PointMm, CoreError> {
        path.driving = true;
        path.active.insert(root);
        let mut parents = Vec::new();
        let mut id = root;
        let mut index = None;
        loop {
            path.missing = None;
            let result = self.position_inner(id, path);
            if let Some(parent) = path.missing.take() {
                path.active.insert(parent);
                parents.push(id);
                id = parent;
                // Longue chaîne : indexer la feuille pour ne pas la reparcourir à chaque rang.
                if parents.len() == INDEXED_DEPTH {
                    index = Some(crate::sheet_index::IndexGuard::new(self));
                }
                continue;
            }
            path.active.remove(&id);
            crate::sheet_index::store_position(self, id, &result);
            let Some(child) = parents.pop() else {
                path.driving = false;
                // Un échec peut avoir retenu des refus qui dépendent du point d'entrée.
                if result.is_err() {
                    path.done.clear();
                } else {
                    path.done.insert(id, result.clone());
                }
                drop(index);
                return result;
            };
            path.done.insert(id, result);
            id = child;
        }
    }

    fn position_inner(&self, id: PointId, path: &mut Resolution) -> Result<PointMm, CoreError> {
        let point = self.point(id)?;
        self.evaluate_rec(&point.definition, &point.label, path)
    }

    /// Position que donnerait une définition de point, sans créer le point.
    pub fn evaluate(&self, definition: &PointDefinition) -> Result<PointMm, CoreError> {
        self.evaluate_rec(definition, "nouveau", &mut Resolution::default())
    }

    fn segment_ends_rec(
        &self,
        segment: EntityId,
        who: &str,
        path: &mut Resolution,
    ) -> Result<(PointMm, PointMm), CoreError> {
        let (from, to) = self.segment_point_ids(segment).map_err(|_| {
            CoreError::DeadReference(format!(
                "le point {who} dépend d'un segment absent (id {})",
                segment.0
            ))
        })?;
        Ok((self.position_rec(from, path)?, self.position_rec(to, path)?))
    }

    fn evaluate_rec(
        &self,
        definition: &PointDefinition,
        who: &str,
        path: &mut Resolution,
    ) -> Result<PointMm, CoreError> {
        let geo = |e: pf_geometry::GeometryError| CoreError::Invalid(format!("point {who} : {e}"));
        match *definition {
            PointDefinition::Free { x, y } => Ok(PointMm::new(x, y).validate()?),
            PointDefinition::OnSegment {
                segment,
                distance_mm,
                origin,
            } => {
                let (a, b) = self.segment_ends_rec(segment, who, path)?;
                let (a, b) = match origin {
                    SegmentEnd::Start => (a, b),
                    SegmentEnd::End => (b, a),
                };
                pf_geometry::point_on_segment(a, b, distance_mm).map_err(geo)
            }
            PointDefinition::OnSegmentRatio { segment, ratio } => {
                if !ratio.is_finite() || !(0.0..=1.0).contains(&ratio) {
                    return Err(geo(pf_geometry::GeometryError::ParameterOutOfRange(ratio)));
                }
                let (a, b) = self.segment_ends_rec(segment, who, path)?;
                pf_geometry::segment_length_mm(a, b).map_err(geo)?;
                Ok(a.lerp(b, ratio))
            }
            PointDefinition::Intersection { first, second } => {
                let (a, b) = self.segment_ends_rec(first, who, path)?;
                let (c, d) = self.segment_ends_rec(second, who, path)?;
                pf_geometry::line_intersection(a, b, c, d).map_err(geo)
            }
            PointDefinition::CircleIntersection {
                first,
                second,
                near,
            } => {
                let solutions = self.circle_intersections_rec(first, second, who, path)?;
                pf_geometry::nearest_solution(&solutions, near.point()).ok_or_else(|| {
                    CoreError::Invalid(format!(
                        "le point {who} : les deux tracés ne se coupent plus"
                    ))
                })
            }
            PointDefinition::TraceIntersection {
                first,
                second,
                near,
            } => {
                near.point().validate()?;
                let hits = self.trace_intersections_rec(first, second, who, path)?;
                hits.iter()
                    .map(|h| h.point)
                    .min_by(|p, q| {
                        pf_geometry::distance_mm(*p, near.point())
                            .total_cmp(&pf_geometry::distance_mm(*q, near.point()))
                    })
                    .ok_or_else(|| {
                        CoreError::Invalid(format!(
                            "le point {who} : les deux tracés ne se coupent plus"
                        ))
                    })
            }
            PointDefinition::OnCurve { curve, t } => {
                let geom = self.curve_geometry_rec(curve, path).map_err(|e| match e {
                    CoreError::UnknownEntity(_) | CoreError::NotACurve(_) => {
                        CoreError::DeadReference(format!(
                            "le point {who} dépend d'une courbe absente (id {})",
                            curve.0
                        ))
                    }
                    other => other,
                })?;
                geom.point_at_checked(t).map_err(geo)
            }
            PointDefinition::AlongCurve { .. }
            | PointDefinition::Offset { .. }
            | PointDefinition::Midpoint { .. }
            | PointDefinition::Projection { .. }
            | PointDefinition::Bisector { .. }
            | PointDefinition::Polar { .. }
            | PointDefinition::Compass { .. }
            | PointDefinition::Division { .. }
            | PointDefinition::Mirror { .. } => self.relative_rec(definition, who, path),
        }
    }

    /// Identifiants des extrémités d'un segment.
    pub fn segment_point_ids(&self, id: EntityId) -> Result<(PointId, PointId), CoreError> {
        match self.entity(id)?.kind {
            EntityKind::Segment { from, to, .. } => Ok((from, to)),
            _ => Err(CoreError::NotASegment(id)),
        }
    }

    /// Extrémités résolues d'un segment.
    pub fn segment_ends(&self, id: EntityId) -> Result<(PointMm, PointMm), CoreError> {
        let (from, to) = self.segment_point_ids(id)?;
        Ok((self.position(from)?, self.position(to)?))
    }

    /// Géométrie absolue d'une courbe.
    pub fn curve_geometry(&self, id: EntityId) -> Result<CubicBezier, CoreError> {
        self.curve_geometry_rec(id, &mut Resolution::default())
    }

    fn curve_geometry_rec(
        &self,
        id: EntityId,
        path: &mut Resolution,
    ) -> Result<CubicBezier, CoreError> {
        match self.entity(id)?.kind {
            EntityKind::Curve {
                start,
                handle_start,
                handle_end,
                end,
            } => {
                let (p0, p3) = (
                    self.position_rec(start, path)?,
                    self.position_rec(end, path)?,
                );
                Ok(CubicBezier::new(
                    p0,
                    p0 + handle_start.vector(),
                    p3 + handle_end.vector(),
                    p3,
                ))
            }
            _ => Err(CoreError::NotACurve(id)),
        }
    }

    /// Géométrie résolue d'un tracé quel que soit son type.
    pub fn shape(&self, id: EntityId) -> Result<Shape, CoreError> {
        self.shape_rec(id, &mut Resolution::default())
    }

    pub(crate) fn shape_rec(
        &self,
        id: EntityId,
        path: &mut Resolution,
    ) -> Result<Shape, CoreError> {
        let label = |e: pf_geometry::GeometryError| {
            CoreError::Invalid(format!("{} : {e}", self.entity_name(id)))
        };
        Ok(match self.entity(id)?.kind {
            EntityKind::Segment { from, to, .. } => {
                Shape::Line(self.position_rec(from, path)?, self.position_rec(to, path)?)
            }
            EntityKind::Spline { .. } => Shape::Spline(
                Spline::from_nodes(&self.spline_hermite_rec(id, path)?).map_err(label)?,
            ),
            EntityKind::Curve { .. } => Shape::Cubic(self.curve_geometry_rec(id, path)?),
            EntityKind::Circle {
                center,
                radius_mm,
                start_deg,
                ccw,
            } => Shape::Circle(
                CircleTrace::new(self.position_rec(center, path)?, radius_mm, start_deg, ccw)
                    .map_err(label)?,
            ),
            EntityKind::Arc {
                center,
                start,
                end,
                ccw,
            } => Shape::Arc(
                Arc::from_center_points(
                    self.position_rec(center, path)?,
                    self.position_rec(start, path)?,
                    self.position_rec(end, path)?,
                    ccw,
                )
                .map_err(label)?,
            ),
        })
    }

    /// Géométrie d'un arc.
    pub fn arc_geometry(&self, id: EntityId) -> Result<Arc, CoreError> {
        match self.shape(id)? {
            Shape::Arc(a) => Ok(a),
            _ => Err(CoreError::Invalid(format!(
                "{} n'est pas un arc",
                self.entity_name(id)
            ))),
        }
    }

    /// Géométrie d'une courbe par points de passage.
    pub fn spline_geometry(&self, id: EntityId) -> Result<Spline, CoreError> {
        match self.shape(id)? {
            Shape::Spline(s) => Ok(s),
            _ => Err(CoreError::Invalid(format!(
                "{} n'est pas une courbe par points de passage",
                self.entity_name(id)
            ))),
        }
    }

    /// Points de passage d'une courbe, avec position et tangentes effectives.
    pub fn spline_nodes(&self, id: EntityId) -> Result<Vec<ResolvedSplineNode>, CoreError> {
        self.spline_resolved_rec(id, &mut Resolution::default())
    }

    fn spline_resolved_rec(
        &self,
        id: EntityId,
        path: &mut Resolution,
    ) -> Result<Vec<ResolvedSplineNode>, CoreError> {
        let EntityKind::Spline { nodes } = &self.entity(id)?.kind else {
            return Err(CoreError::Invalid(format!(
                "{} n'est pas une courbe par points de passage",
                self.entity_name(id)
            )));
        };
        let pos = nodes
            .iter()
            .map(|n| self.position_rec(n.point, path))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(nodes
            .iter()
            .enumerate()
            .map(|(i, n)| {
                let prev = i.checked_sub(1).map(|j| pos[j]);
                let next = pos.get(i + 1).copied();
                let (def_in, def_out) = if n.corner {
                    pf_geometry::default_corner_tangents(prev, pos[i], next)
                } else {
                    let t = pf_geometry::default_smooth_tangent(prev, pos[i], next);
                    (t, t)
                };
                let out = n.tangent.map(OffsetMm::vector);
                let inn = if n.corner {
                    n.tangent_in.map(OffsetMm::vector)
                } else {
                    out
                };
                ResolvedSplineNode {
                    point: n.point,
                    position: pos[i],
                    corner: n.corner,
                    tangent_in: inn.unwrap_or(def_in),
                    tangent_out: out.unwrap_or(def_out),
                    tangent_in_explicit: inn.is_some(),
                    tangent_out_explicit: out.is_some(),
                }
            })
            .collect())
    }

    fn spline_hermite_rec(
        &self,
        id: EntityId,
        path: &mut Resolution,
    ) -> Result<Vec<HermiteNode>, CoreError> {
        Ok(self
            .spline_resolved_rec(id, path)?
            .into_iter()
            .map(|n| HermiteNode {
                point: n.position,
                tangent_in: n.tangent_in,
                tangent_out: n.tangent_out,
            })
            .collect())
    }

    /// Contrôles structurels d'une courbe par points de passage.
    pub(crate) fn check_spline_nodes(
        &self,
        id: EntityId,
        nodes: &[SplineNode],
    ) -> Result<(), CoreError> {
        let bad = |msg: &str| {
            Err(CoreError::Invalid(format!(
                "{} : {msg}",
                self.entity_name(id)
            )))
        };
        if nodes.len() < 2 {
            return bad("une courbe par points de passage en exige au moins deux");
        }
        if nodes.len() > MAX_SPLINE_NODES {
            return bad("trop de points de passage");
        }
        let mut seen = HashSet::new();
        for (i, n) in nodes.iter().enumerate() {
            self.point(n.point)?;
            if !seen.insert(n.point) {
                return bad("un point de passage ne peut figurer qu'une fois");
            }
            let end = i == 0 || i == nodes.len() - 1;
            if n.corner && end {
                return bad("une extrémité ne peut pas être un point anguleux");
            }
            if n.tangent_in.is_some() && !n.corner {
                return bad("la tangente entrante n'existe qu'en un point anguleux");
            }
            for t in [n.tangent, n.tangent_in].into_iter().flatten() {
                if !t.dx.is_finite() || !t.dy.is_finite() {
                    return bad("tangente non finie");
                }
            }
        }
        Ok(())
    }

    /// Géométrie d'un cercle entier.
    pub fn circle_geometry(&self, id: EntityId) -> Result<CircleTrace, CoreError> {
        match self.shape(id)? {
            Shape::Circle(c) => Ok(c),
            _ => Err(CoreError::Invalid(format!(
                "{} n'est pas un cercle",
                self.entity_name(id)
            ))),
        }
    }

    fn circle_intersections_rec(
        &self,
        first: EntityId,
        second: EntityId,
        who: &str,
        path: &mut Resolution,
    ) -> Result<Vec<PointMm>, CoreError> {
        let dead = |id: EntityId| {
            CoreError::DeadReference(format!(
                "le point {who} dépend d'un tracé absent (id {})",
                id.0
            ))
        };
        let a = self.shape_rec(first, path).map_err(|e| match e {
            CoreError::UnknownEntity(_) => dead(first),
            o => o,
        })?;
        let b = self.shape_rec(second, path).map_err(|e| match e {
            CoreError::UnknownEntity(_) => dead(second),
            o => o,
        })?;
        shape_intersections(&a, &b)
    }

    /// Solutions d'intersection de deux tracés dont l'un au moins est un cercle
    /// ou un arc (prolongé en cercle entier) ; l'autre peut être un segment
    /// (prolongé en droite) ou un cercle/arc. 0, 1 (tangence) ou 2 points.
    pub fn circle_intersections(
        &self,
        first: EntityId,
        second: EntityId,
    ) -> Result<Vec<PointMm>, CoreError> {
        self.circle_intersections_rec(first, second, "nouveau", &mut Resolution::default())
    }

    fn trace_intersections_rec(
        &self,
        first: EntityId,
        second: EntityId,
        who: &str,
        path: &mut Resolution,
    ) -> Result<Vec<pf_geometry::Hit>, CoreError> {
        if first == second {
            return Err(CoreError::Invalid(format!(
                "le point {who} : une intersection exige deux tracés différents"
            )));
        }
        let dead = |id: EntityId| {
            CoreError::DeadReference(format!(
                "le point {who} dépend d'un tracé absent (id {})",
                id.0
            ))
        };
        let a = self.shape_rec(first, path).map_err(|e| match e {
            CoreError::UnknownEntity(_) => dead(first),
            o => o,
        })?;
        let b = self.shape_rec(second, path).map_err(|e| match e {
            CoreError::UnknownEntity(_) => dead(second),
            o => o,
        })?;
        pf_geometry::intersect(&a.to_trace(), &b.to_trace())
            .map_err(|e| CoreError::Invalid(format!("le point {who} : {e}")))
    }

    /// Toutes les intersections géométriques de deux tracés (bornés), triées
    /// par paramètre du premier tracé. Vide s'ils ne se coupent pas ; erreur
    /// s'ils se superposent ou si l'un est dégénéré.
    pub fn trace_intersections(
        &self,
        first: EntityId,
        second: EntityId,
    ) -> Result<Vec<pf_geometry::Hit>, CoreError> {
        self.trace_intersections_rec(first, second, "nouveau", &mut Resolution::default())
    }

    /// Points référencés directement par un tracé : extrémités, ou `(centre, centre)` pour un cercle.
    pub fn entity_points(&self, id: EntityId) -> Result<(PointId, PointId), CoreError> {
        match self.entity(id)?.kind {
            EntityKind::Segment { from, to, .. } => Ok((from, to)),
            EntityKind::Curve { start, end, .. } => Ok((start, end)),
            EntityKind::Arc { start, end, .. } => Ok((start, end)),
            EntityKind::Circle { center, .. } => Ok((center, center)),
            EntityKind::Spline { ref nodes } => match (nodes.first(), nodes.last()) {
                (Some(a), Some(b)) => Ok((a.point, b.point)),
                _ => Err(CoreError::Invalid("courbe sans point de passage".into())),
            },
        }
    }

    /// Tous les points dont dépend un tracé (centre d'un arc compris).
    pub fn entity_all_points(&self, id: EntityId) -> Result<Vec<PointId>, CoreError> {
        Ok(self.entity(id)?.kind.all_points())
    }

    /// Vrai si un tracé utilise ce point (extrémité ou centre).
    pub fn point_is_used(&self, id: PointId) -> bool {
        self.entities
            .iter()
            .any(|e| e.kind.all_points().contains(&id))
    }

    /// Boîte englobante de toute la feuille (points, tracés, poignées exclues).
    pub fn bbox(&self) -> Option<Bbox> {
        let _cache = crate::sheet_index::IndexGuard::new(self);
        let mut bb: Option<Bbox> = None;
        let mut add = |p: PointMm| match bb.as_mut() {
            Some(b) => b.include(p),
            None => bb = Some(Bbox::from_point(p)),
        };
        for p in &self.points {
            if let Ok(pos) = self.position(p.id) {
                add(pos);
            }
        }
        for e in &self.entities {
            if matches!(e.kind, EntityKind::Segment { .. }) {
                continue;
            }
            if let Ok(shape) = self.shape(e.id) {
                let cb = shape.bbox();
                add(cb.min);
                add(cb.max);
            }
        }
        bb
    }

    /// Prochaine étiquette libre : A…Z, puis A1…Z1, etc.
    pub fn next_label(&self) -> String {
        let used: HashSet<&str> = self.points.iter().map(|p| p.label.as_str()).collect();
        for round in 0u32.. {
            for c in 'A'..='Z' {
                let label = if round == 0 {
                    c.to_string()
                } else {
                    format!("{c}{round}")
                };
                if !used.contains(label.as_str()) {
                    return label;
                }
            }
        }
        unreachable!()
    }

    pub(crate) fn validate(&self) -> Result<(), CoreError> {
        // Index id -> position valable pendant toute la validation (la feuille n'est pas
        // modifiée) : les recherches de points, tracés et calques deviennent O(1).
        let _index = crate::sheet_index::IndexGuard::new(self);
        check_name(&self.name, "le nom de la feuille")?;
        for p in &self.points {
            if p.label.chars().count() > MAX_NAME_LEN {
                return Err(CoreError::Invalid("étiquette de point trop longue".into()));
            }
            self.position(p.id)?;
        }
        for e in &self.entities {
            let ctx = |err: CoreError| match err {
                CoreError::UnknownPoint(pid) => CoreError::DeadReference(format!(
                    "le tracé {} référence un point absent (id {})",
                    e.id.0, pid.0
                )),
                other => other,
            };
            match e.kind {
                EntityKind::Segment { construction, .. } => {
                    let (a, b) = self.segment_ends(e.id).map_err(ctx)?;
                    pf_geometry::segment_length_mm(a, b).map_err(|g| {
                        CoreError::Invalid(format!("{} : {g}", self.entity_name(e.id)))
                    })?;
                    // La référence d'une perpendiculaire peut avoir été supprimée
                    // depuis : c'est une provenance, pas une dépendance.
                    let finite = match construction {
                        Some(Construction::Polar {
                            length_mm,
                            angle_deg,
                            ..
                        }) => length_mm.is_finite() && angle_deg.is_finite(),
                        Some(Construction::Perpendicular { length_mm, .. }) => {
                            length_mm.is_finite()
                        }
                        Some(Construction::Parallel { distance_mm, .. }) => distance_mm.is_finite(),
                        None => true,
                    };
                    if !finite {
                        return Err(CoreError::Invalid(
                            "paramètres de construction non finis".into(),
                        ));
                    }
                }
                EntityKind::Curve { .. } => {
                    let c = self.curve_geometry(e.id).map_err(ctx)?;
                    c.validate().map_err(|g| {
                        CoreError::Invalid(format!("{} : {g}", self.entity_name(e.id)))
                    })?;
                }
                EntityKind::Circle { .. } => {
                    self.shape(e.id).map_err(ctx)?;
                }
                EntityKind::Spline { ref nodes } => {
                    self.check_spline_nodes(e.id, nodes).map_err(ctx)?;
                    self.shape(e.id).map_err(ctx)?;
                }
                EntityKind::Arc {
                    center, start, end, ..
                } => {
                    if center == start || center == end || start == end {
                        return Err(CoreError::Invalid(format!(
                            "{} : le centre et les extrémités doivent être trois points distincts",
                            self.entity_name(e.id)
                        )));
                    }
                    self.shape(e.id).map_err(ctx)?;
                }
            }
        }
        for n in &self.notches {
            let len = self.path_length_mm(n.entity).map_err(|e| match e {
                CoreError::UnknownEntity(id) => CoreError::DeadReference(format!(
                    "un cran référence un tracé absent (id {})",
                    id.0
                )),
                other => other,
            })?;
            if let Some(anchor) = &n.anchor {
                self.check_notch_anchor(n, anchor)?;
            } else if !n.distance_mm.is_finite()
                || n.distance_mm < 0.0
                || n.distance_mm > len + 1e-9
            {
                return Err(CoreError::Invalid(format!(
                    "cran sur {} : la position {} mm est hors du tracé (longueur {} mm)",
                    self.entity_name(n.entity),
                    pf_geometry_fmt(n.distance_mm),
                    pf_geometry_fmt(len)
                )));
            }
            if !n.length_mm.is_finite()
                || n.length_mm <= 0.0
                || n.length_mm > crate::notch::MAX_NOTCH_LENGTH_MM
            {
                return Err(CoreError::Invalid(format!(
                    "cran : la longueur doit être comprise entre 0 et {} mm",
                    crate::notch::MAX_NOTCH_LENGTH_MM
                )));
            }
            crate::notch::check_notch_shape(n)?;
        }
        let mut names = HashSet::new();
        for l in &self.layers {
            check_name(&l.name, "le nom du calque")?;
            if !names.insert(l.name.as_str()) {
                return Err(CoreError::Invalid(format!(
                    "le nom de calque « {} » est utilisé deux fois",
                    l.name
                )));
            }
        }
        let known = |layer: Option<LayerId>, what: &str| match layer {
            Some(l) if self.layer(l).is_err() => Err(CoreError::DeadReference(format!(
                "{what} référence un calque absent (id {})",
                l.0
            ))),
            _ => Ok(()),
        };
        for p in &self.points {
            known(p.layer, &format!("le point {}", p.label))?;
        }
        for e in &self.entities {
            known(e.layer, &format!("le tracé {}", e.id.0))?;
        }
        let mut entity_names = HashSet::new();
        for e in &self.entities {
            if let Some(n) = &e.name {
                check_name(n, "le nom du tracé")?;
                if !entity_names.insert(n.as_str()) {
                    return Err(CoreError::Invalid(format!(
                        "le nom de tracé « {n} » est utilisé deux fois"
                    )));
                }
            }
        }
        let mut grouped = HashSet::new();
        for g in &self.groups {
            if g.members.is_empty() {
                return Err(CoreError::Invalid("un groupe ne peut pas être vide".into()));
            }
            for &m in &g.members {
                if self.entity(m).is_err() {
                    return Err(CoreError::DeadReference(format!(
                        "un groupe référence un tracé absent (id {})",
                        m.0
                    )));
                }
                if !grouped.insert(m) {
                    return Err(CoreError::Invalid(format!(
                        "le tracé {} est dans deux groupes à la fois",
                        m.0
                    )));
                }
            }
        }
        for g in &self.guides {
            g.validate()?;
            known(g.layer, "une ligne d'aide")?;
        }
        for d in &self.dimensions {
            self.point(d.from)?;
            self.point(d.to)?;
            if d.from == d.to {
                return Err(CoreError::Invalid(
                    "cotation : les deux points doivent être différents".into(),
                ));
            }
            crate::dimension::check_offset(d.offset_mm)?;
        }
        self.validate_full_dimensions()?;
        Ok(())
    }
}

impl EntityKind {
    /// Tous les points qu'utilise ce tracé (extrémités, centre).
    pub fn all_points(&self) -> Vec<PointId> {
        match *self {
            EntityKind::Segment { from, to, .. } => vec![from, to],
            EntityKind::Curve { start, end, .. } => vec![start, end],
            EntityKind::Circle { center, .. } => vec![center],
            EntityKind::Arc {
                center, start, end, ..
            } => vec![start, end, center],
            EntityKind::Spline { ref nodes } => nodes.iter().map(|n| n.point).collect(),
        }
    }
}

/// Intersections de deux géométries dont au moins une est un cercle ou un arc.
pub(crate) fn shape_intersections(a: &Shape, b: &Shape) -> Result<Vec<PointMm>, CoreError> {
    let circle = |s: &Shape| match s {
        Shape::Arc(x) => pf_geometry::Circle::new(x.center, x.radius_mm).ok(),
        Shape::Circle(x) => Some(x.circle),
        _ => None,
    };
    match (a, b, circle(a), circle(b)) {
        (_, _, Some(c1), Some(c2)) => Ok(pf_geometry::circle_circle_intersections(&c1, &c2)?),
        (_, Shape::Line(p, q), Some(c), None) | (Shape::Line(p, q), _, None, Some(c)) => {
            Ok(pf_geometry::circle_line_intersections(&c, *p, *q)?)
        }
        _ => Err(CoreError::Invalid(
            "l'intersection demandée exige un cercle ou un arc et un segment, un autre cercle              ou un autre arc"
                .into(),
        )),
    }
}

fn pf_geometry_fmt(v: f64) -> String {
    crate::input::format_number(v, 2)
}

/// Style d'un cran.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotchStyle {
    /// Un trait perpendiculaire au bord.
    #[default]
    Single,
    /// Deux traits rapprochés.
    Double,
}

/// Forme d'un cran (US-06.1). La profondeur est `Notch::length_mm`, la largeur
/// `Notch::width_mm` (sans objet pour le trait).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotchForm {
    /// Un trait perpendiculaire au bord.
    #[default]
    Line,
    /// Un trait terminé par une barre perpendiculaire (en T).
    T,
    /// Un triangle (en V), pointe à la profondeur.
    V,
    /// Un U : deux côtés droits et un fond demi-circulaire.
    U,
    /// Une fente : rectangle étroit.
    Slot,
}

impl NotchForm {
    pub const ALL: [NotchForm; 5] = [
        NotchForm::Line,
        NotchForm::T,
        NotchForm::V,
        NotchForm::U,
        NotchForm::Slot,
    ];

    pub fn is_line(&self) -> bool {
        *self == NotchForm::Line
    }

    /// Identifiant stable, indépendant de la langue.
    pub fn key(&self) -> &'static str {
        match self {
            NotchForm::Line => "line",
            NotchForm::T => "t",
            NotchForm::V => "v",
            NotchForm::U => "u",
            NotchForm::Slot => "slot",
        }
    }
}

/// Ancrage facultatif d'un cran (US-06.2) : au lieu d'une distance saisie, la
/// position est **calculée à la lecture** d'après la construction (voir
/// `Sheet::notch_distance_mm`). Tous les objets cités sont de la feuille du cran.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum NotchAnchor {
    /// Au pied de la perpendiculaire abaissée d'un point de la feuille sur le bord.
    Point { point: PointId },
    /// Au milieu du bord, par longueur développée (courbes comprises).
    Midpoint {},
    /// À la proportion `ratio` ∈ [0 ; 1] de la longueur du bord, depuis son départ.
    Ratio { ratio: f64 },
    /// Longueur reportée : `point` (par exemple sur une autre pièce de la feuille)
    /// est projeté sur le tracé `entity` ; la distance obtenue le long de `entity`,
    /// depuis son départ, est reportée sur le bord du cran (depuis son départ, ou
    /// depuis son arrivée avec `from_end`). Refusée si elle dépasse le bord.
    Carried {
        entity: EntityId,
        point: PointId,
        #[serde(default, skip_serializing_if = "is_false")]
        from_end: bool,
    },
}

impl NotchAnchor {
    /// Point de la feuille dont dépend l'ancrage, s'il y en a un.
    pub fn point(&self) -> Option<PointId> {
        match *self {
            NotchAnchor::Point { point } | NotchAnchor::Carried { point, .. } => Some(point),
            NotchAnchor::Midpoint {} | NotchAnchor::Ratio { .. } => None,
        }
    }

    /// Identifiant stable, indépendant de la langue.
    pub fn key(&self) -> &'static str {
        match self {
            NotchAnchor::Point { .. } => "point",
            NotchAnchor::Midpoint {} => "midpoint",
            NotchAnchor::Ratio { .. } => "ratio",
            NotchAnchor::Carried { .. } => "carried",
        }
    }
}

/// Cran : repère perpendiculaire au bord d'un tracé, à une distance mesurée
/// le long de ce tracé depuis son départ. Il suit son tracé quand il change.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Notch {
    pub id: NotchId,
    pub entity: EntityId,
    /// Position le long du tracé, en mm depuis son départ (longueur développée pour une courbe).
    /// **Sans effet** quand le cran est ancré (`anchor`) : les commandes l'écrivent à 0 et la
    /// position se lit par `Sheet::notch_distance_mm`.
    pub distance_mm: f64,
    /// Côté du tracé (par rapport au sens départ → arrivée) vers lequel le cran pointe.
    pub side: Side,
    pub length_mm: f64,
    #[serde(default)]
    pub style: NotchStyle,
    /// Ancrage à un point ou à une proportion du bord (US-06.2) ; omis quand le cran est
    /// à distance fixe.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor: Option<NotchAnchor>,
    /// Forme du cran (US-06.1) ; omise pour le simple trait.
    #[serde(default, skip_serializing_if = "NotchForm::is_line")]
    pub form: NotchForm,
    /// Largeur du cran (mm) pour les formes T, V, U et fente ; omise = largeur par défaut
    /// (`notch::DEFAULT_NOTCH_WIDTH_MM`). Interdite pour le simple trait.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width_mm: Option<f64>,
    /// Cran **sur la coupe** (US-06.1) : défini sur le bord de couture, il est projeté
    /// perpendiculairement à la couture sur le contour de coupe calculé (position **lue**,
    /// jamais stockée, qui suit la marge). Omis quand le cran est sur la couture.
    #[serde(default, skip_serializing_if = "is_false")]
    pub on_cut: bool,
    /// Cran **en coin** (US-06.1b) : posé à un sommet de la couture (début ou fin de son tracé,
    /// ancré en proportion 0 ou 1 pour suivre le sommet), il est orienté selon la bissectrice de l'angle du contour de la pièce (position et
    /// direction **lues**, jamais stockées, voir `Project::notch_corner`). Omis quand faux ;
    /// incompatible avec `on_cut` (combinaison reportée, refusée).
    #[serde(default, skip_serializing_if = "is_false")]
    pub corner: bool,
    /// Cran **proportionnel** (schéma v5, US-09.5b) : sa distance suit la longueur du tracé d'une
    /// taille à l'autre au lieu de rester fixe. Seulement pour un cran à distance fixe (sans
    /// `anchor`, déjà proportionnel) ; omis quand faux. Stockage posé, pas encore utilisé.
    #[serde(default, skip_serializing_if = "is_false")]
    pub proportional: bool,
}

/// Cotation : mesure affichée de la distance entre deux points, décalée de
/// `offset_mm` (à gauche de from → to). Ce n'est pas une contrainte : la valeur
/// est recalculée depuis les points.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Dimension {
    pub id: DimensionId,
    pub from: PointId,
    pub to: PointId,
    pub offset_mm: f64,
}
