use crate::model::{
    DimensionId, EntityId, FullDimensionId, GroupId, GuideId, LayerId, NotchId, PieceId, PointId,
    SheetId,
};
use pf_geometry::GeometryError;
use std::fmt;

/// Erreurs métier, toujours affichables telles quelles à l'utilisatrice.
#[derive(Debug, Clone, PartialEq)]
pub enum CoreError {
    Geometry(GeometryError),
    UnknownSheet(SheetId),
    UnknownPoint(PointId),
    UnknownEntity(EntityId),
    UnknownNotch(NotchId),
    UnknownDimension(DimensionId),
    UnknownFullDimension(FullDimensionId),
    UnknownPiece(PieceId),
    UnknownLayer(LayerId),
    UnknownGroup(GroupId),
    UnknownGuide(GuideId),
    /// Commande refusée : elle toucherait un objet d'un calque verrouillé.
    LayerLocked(String),
    NotASegment(EntityId),
    NotACurve(EntityId),
    /// Référence vers un élément supprimé ou absent.
    DeadReference(String),
    CyclicReference(PointId),
    DuplicateId(u64),
    /// Donnée ou opération invalide, avec explication.
    Invalid(String),
    /// Fichier illisible (JSON malformé, champ manquant…).
    Malformed(String),
    NotAPatternForgeFile,
    UnsupportedVersion {
        found: u32,
        supported: u32,
    },
    Io(String),
}

impl fmt::Display for CoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Geometry(e) => write!(f, "{e}"),
            Self::UnknownSheet(id) => write!(f, "feuille introuvable (id {})", id.0),
            Self::UnknownPoint(id) => write!(f, "point introuvable (id {})", id.0),
            Self::UnknownEntity(id) => write!(f, "tracé introuvable (id {})", id.0),
            Self::UnknownNotch(id) => write!(f, "cran introuvable (id {})", id.0),
            Self::UnknownDimension(id) => write!(f, "cotation introuvable (id {})", id.0),
            Self::UnknownFullDimension(id) => write!(f, "cote introuvable (id {})", id.0),
            Self::UnknownPiece(id) => write!(f, "pièce introuvable (id {})", id.0),
            Self::UnknownLayer(id) => write!(f, "calque introuvable (id {})", id.0),
            Self::UnknownGroup(id) => write!(f, "groupe introuvable (id {})", id.0),
            Self::UnknownGuide(id) => write!(f, "ligne d'aide introuvable (id {})", id.0),
            Self::LayerLocked(msg) => write!(f, "{msg}"),
            Self::NotASegment(id) => write!(f, "le tracé {} n'est pas un segment", id.0),
            Self::NotACurve(id) => write!(f, "le tracé {} n'est pas une courbe", id.0),
            Self::DeadReference(msg) => write!(f, "référence manquante : {msg}"),
            Self::CyclicReference(id) => {
                write!(f, "dépendance circulaire entre points (point id {})", id.0)
            }
            Self::DuplicateId(id) => write!(f, "identifiant en double dans le projet ({id})"),
            Self::Invalid(msg) => write!(f, "{msg}"),
            Self::Malformed(msg) => write!(f, "fichier projet illisible : {msg}"),
            Self::NotAPatternForgeFile => {
                write!(f, "ce fichier n'est pas un projet PatternForge")
            }
            Self::UnsupportedVersion { found, supported } => write!(
                f,
                "ce projet utilise le format version {found}, plus récent que celui pris en \
                 charge (version {supported}). Mettez PatternForge à jour ; le fichier n'a pas \
                 été modifié."
            ),
            Self::Io(msg) => write!(f, "erreur de fichier : {msg}"),
        }
    }
}

impl CoreError {
    /// Code stable de la variante (US-19.1) : `snake_case`, jamais renommé. Le message
    /// d'interface est la clé `err.<code>` du catalogue, remplie par [`CoreError::params`].
    pub fn code(&self) -> &'static str {
        match self {
            Self::Geometry(_) => "geometry",
            Self::UnknownSheet(_) => "unknown_sheet",
            Self::UnknownPoint(_) => "unknown_point",
            Self::UnknownEntity(_) => "unknown_entity",
            Self::UnknownNotch(_) => "unknown_notch",
            Self::UnknownDimension(_) => "unknown_dimension",
            Self::UnknownFullDimension(_) => "unknown_full_dimension",
            Self::UnknownPiece(_) => "unknown_piece",
            Self::UnknownLayer(_) => "unknown_layer",
            Self::UnknownGroup(_) => "unknown_group",
            Self::UnknownGuide(_) => "unknown_guide",
            Self::LayerLocked(_) => "layer_locked",
            Self::NotASegment(_) => "not_a_segment",
            Self::NotACurve(_) => "not_a_curve",
            Self::DeadReference(_) => "dead_reference",
            Self::CyclicReference(_) => "cyclic_reference",
            Self::DuplicateId(_) => "duplicate_id",
            Self::Invalid(_) => "invalid",
            Self::Malformed(_) => "malformed",
            Self::NotAPatternForgeFile => "not_a_patternforge_file",
            Self::UnsupportedVersion { .. } => "unsupported_version",
            Self::Io(_) => "io",
        }
    }

    /// Paramètres typés du message : `(nom, valeur)`. Pour les variantes à texte libre
    /// (`detail`), le texte vient du cœur et reste en français (limite déclarée).
    pub fn params(&self) -> Vec<(&'static str, String)> {
        match self {
            Self::Geometry(_) | Self::NotAPatternForgeFile => Vec::new(),
            Self::UnknownSheet(id) => vec![("id", id.0.to_string())],
            Self::UnknownPoint(id) | Self::CyclicReference(id) => vec![("id", id.0.to_string())],
            Self::UnknownEntity(id) | Self::NotASegment(id) | Self::NotACurve(id) => {
                vec![("id", id.0.to_string())]
            }
            Self::UnknownNotch(id) => vec![("id", id.0.to_string())],
            Self::UnknownDimension(id) => vec![("id", id.0.to_string())],
            Self::UnknownFullDimension(id) => vec![("id", id.0.to_string())],
            Self::UnknownPiece(id) => vec![("id", id.0.to_string())],
            Self::UnknownLayer(id) => vec![("id", id.0.to_string())],
            Self::UnknownGroup(id) => vec![("id", id.0.to_string())],
            Self::UnknownGuide(id) => vec![("id", id.0.to_string())],
            Self::DuplicateId(id) => vec![("id", id.to_string())],
            Self::LayerLocked(d)
            | Self::DeadReference(d)
            | Self::Invalid(d)
            | Self::Malformed(d)
            | Self::Io(d) => vec![("detail", d.clone())],
            Self::UnsupportedVersion { found, supported } => vec![
                ("found", found.to_string()),
                ("supported", supported.to_string()),
            ],
        }
    }

    /// Vrai quand le message contient un texte libre écrit en français par le cœur.
    pub fn has_free_text(&self) -> bool {
        matches!(
            self,
            Self::LayerLocked(_)
                | Self::DeadReference(_)
                | Self::Invalid(_)
                | Self::Malformed(_)
                | Self::Io(_)
        )
    }
}

impl std::error::Error for CoreError {}

impl From<GeometryError> for CoreError {
    fn from(e: GeometryError) -> Self {
        CoreError::Geometry(e)
    }
}
