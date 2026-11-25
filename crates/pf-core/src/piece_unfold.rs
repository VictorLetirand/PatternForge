//! US-04.7 — déplier : une pièce construite sur une pliure devient une pièce entière
//! symétrique, **liée** à la pièce source (décisions 147 à 160).
//!
//! La pliure est un **segment du contour** de la pièce (un tracé de son `outline`, droit,
//! éventuellement prolongé par les segments consécutifs alignés à 1e-6 mm). La moitié est
//! le reste du contour ; le résultat est la moitié suivie de son image en miroir, en un seul
//! contour fermé dont la pliure est retirée (voir `pf_geometry::fold`).
//!
//! Le lien est une dérivation du type d'US-04.4 (`DerivedOp::Unfold`) : la pièce dépliée vit
//! sur sa feuille dédiée, ses tracés sont de vrais tracés, elle est régénérée dans la même
//! étape annulable quand la source change et marquée obsolète (`DerivedStale`) si le
//! recalcul échoue. Aucun champ de modèle n'est ajouté hors de la variante d'opération.

use crate::command::{Command, Outcome};
use crate::contour::{contour_polyline, ContourEdge};
use crate::derived::DerivedOp;
use crate::error::CoreError;
use crate::model::*;
use crate::piece_split_join::{geoms_of, suffixed, ChainEdge};
use pf_geometry::fold::{unfold_chain, FoldError, Unfolded};
use pf_geometry::split::{chain_signed_area_mm2, edge_trace};
use pf_geometry::{PointMm, Trace};

/// Suffixe du nom de la pièce dépliée (nom par défaut).
const NAME_SUFFIX: &str = " – dépliée";
/// Suffixe du code de la pièce dépliée.
const CODE_SUFFIX: &str = "-D";

/// Géométrie d'un dépliage, avant création des tracés.
pub(crate) struct UnfoldGeom {
    pub traces: Vec<Trace>,
    /// Rôle commun des tracés de la moitié (le défaut s'il n'y en a pas).
    pub role: PathRole,
    /// Aire de la moitié (mm²), pliure comprise.
    pub half_area_mm2: f64,
}

/// Segment du contour qui peut servir de pliure.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FoldCandidate {
    pub entity: EntityId,
    pub length_mm: f64,
    /// Le tracé a le rôle « pliure » (proposé après les bords déclarés sur le pli).
    pub is_fold_role: bool,
    /// Le tracé est déclaré « sur le pli » sur la pièce (PH-5b) : proposé en premier.
    pub on_fold: bool,
}

/// Aperçu d'un dépliage (lecture seule). Toutes les mesures sont **observées** sur le
/// résultat de la commande rejouée sur une copie du projet.
#[derive(Debug, Clone, PartialEq)]
pub struct UnfoldPreview {
    /// Nom que recevrait la pièce dépliée.
    pub name: String,
    pub fold_length_mm: f64,
    pub fold_start: PointMm,
    pub fold_end: PointMm,
    pub half_area_mm2: f64,
    pub half_perimeter_mm: f64,
    pub area_mm2: f64,
    pub perimeter_mm: f64,
    /// Contour de la moitié (polyligne fermée).
    pub half_outline: Vec<PointMm>,
    /// Contour de la pièce dépliée (polyligne fermée).
    pub outline: Vec<PointMm>,
    /// Remarques de la commande (droit-fil et éléments internes non repris…).
    pub notes: Vec<String>,
}

fn fold_message(e: &FoldError, name: &str) -> String {
    format!("pièce « {name} » : {e}")
}

impl Project {
    /// Segments du contour de `piece` qui peuvent servir de pliure, dans l'ordre du
    /// contour (un tracé de rôle « pliure » d'abord). Vide si la pièce est inconnue.
    pub fn unfold_candidates(&self, piece: PieceId) -> Vec<FoldCandidate> {
        let (Ok(p), Ok(sheet)) = (self.piece(piece), self.piece_sheet(piece)) else {
            return Vec::new();
        };
        let mut out: Vec<FoldCandidate> = p
            .outline
            .iter()
            .filter_map(|e| {
                let ent = sheet.entity(e.entity).ok()?;
                if !matches!(ent.kind, EntityKind::Segment { .. }) {
                    return None;
                }
                Some(FoldCandidate {
                    entity: e.entity,
                    length_mm: sheet.path_length_mm(e.entity).ok()?,
                    is_fold_role: ent.role == PathRole::Fold,
                    on_fold: p.on_fold.contains(&e.entity),
                })
            })
            .collect();
        out.sort_by_key(|c| (!c.on_fold, !c.is_fold_role));
        out
    }

    /// Chaîne élémentaire de `piece` et son dépliage le long de `fold` : raisons de refus
    /// en clair. Les pièces dérivées liées sont refusées (décision 150) ; la pliure doit
    /// être un segment du contour.
    fn unfold_chain_of(
        &self,
        piece: PieceId,
        fold: EntityId,
    ) -> Result<(Vec<ChainEdge>, Unfolded), String> {
        let p = self.piece(piece).map_err(|e| e.to_string())?;
        let name = &p.name;
        if p.derived.is_some() {
            return Err(format!(
                "la pièce « {name} » est une pièce dérivée liée à sa mère : cassez le lien avant                  de la déplier"
            ));
        }
        if p.outline.is_empty() {
            return Err(format!("la pièce « {name} » n'a pas de contour"));
        }
        if !p.outline.iter().any(|e| e.entity == fold) {
            return Err(format!(
                "la pliure désignée n'est pas un tracé du contour de la pièce « {name} »"
            ));
        }
        let sheet = self.piece_sheet(piece).map_err(|e| e.to_string())?;
        let kind = &sheet.entity(fold).map_err(|e| e.to_string())?.kind;
        if !matches!(kind, EntityKind::Segment { .. }) {
            return Err(format!(
                "la pliure de la pièce « {name} » doit être un segment rectiligne (ni courbe,                  ni arc, ni cercle, ni courbe lisse)"
            ));
        }
        let chain = self.piece_chain(piece).map_err(|e| e.to_string())?;
        let index = chain
            .iter()
            .position(|e| e.source == fold)
            .ok_or_else(|| format!("la pliure de la pièce « {name} » est introuvable"))?;
        let u = unfold_chain(&geoms_of(&chain), index).map_err(|e| fold_message(&e, name))?;
        Ok((chain, u))
    }

    /// Géométrie du dépliage de `piece` le long de `fold`.
    pub(crate) fn unfold_geometry(
        &self,
        piece: PieceId,
        fold: EntityId,
    ) -> Result<UnfoldGeom, String> {
        let (chain, u) = self.unfold_chain_of(piece, fold)?;
        let n = chain.len();
        let half_roles: Vec<PathRole> = (0..n - u.fold_edges)
            .map(|j| chain[(u.first_fold_edge + u.fold_edges + j) % n].role)
            .collect();
        let role = match half_roles.split_first() {
            Some((first, rest)) if rest.iter().all(|r| r == first) => *first,
            _ => PathRole::default(),
        };
        Ok(UnfoldGeom {
            traces: u.chain.iter().map(edge_trace).collect(),
            role,
            half_area_mm2: chain_signed_area_mm2(&geoms_of(&chain)),
        })
    }

    /// Aperçu d'un dépliage : la commande `UnfoldPiece` rejouée sur une copie du projet
    /// (mêmes refus), puis lecture du résultat. Le projet n'est pas modifié. Coûteux :
    /// à appeler à la demande, pas à chaque image sur un grand projet.
    pub fn unfold_preview(
        &self,
        piece: PieceId,
        fold: EntityId,
        name: Option<String>,
    ) -> Result<UnfoldPreview, CoreError> {
        let before = self.piece_contour(piece)?;
        let mut copy = self.clone();
        let out = copy.apply(Command::UnfoldPiece { piece, fold, name })?;
        let id = *out
            .pieces
            .first()
            .ok_or_else(|| CoreError::Invalid("le dépliage n'a rien créé".into()))?;
        let new = copy.piece(id)?;
        let measured = copy.piece_contour(id)?;
        let polyline = |project: &Project, piece: &Piece| -> Result<Vec<PointMm>, CoreError> {
            let sheet = project.piece_sheet(piece.id)?;
            let edges: Vec<ContourEdge> = piece
                .outline
                .iter()
                .map(|e| ContourEdge {
                    entity: e.entity,
                    reversed: e.reversed,
                })
                .collect();
            contour_polyline(sheet, &edges, 48)
        };
        let source = self.piece(piece)?;
        let (_, u) = self
            .unfold_chain_of(piece, fold)
            .map_err(CoreError::Invalid)?;
        Ok(UnfoldPreview {
            name: new.name.clone(),
            fold_length_mm: u.fold_length_mm,
            fold_start: u.fold_start,
            fold_end: u.fold_end,
            half_area_mm2: before.area_mm2,
            half_perimeter_mm: before.perimeter_mm,
            area_mm2: measured.area_mm2,
            perimeter_mm: measured.perimeter_mm,
            half_outline: polyline(self, source)?,
            outline: polyline(&copy, new)?,
            notes: out.notes,
        })
    }

    /// Nom, code et remarques de la pièce dépliée créée d'après `source`.
    pub(crate) fn unfolded_identity(
        source: &Piece,
        fold: EntityId,
        name: Option<&str>,
        out: &mut Outcome,
    ) -> (String, Option<String>) {
        let name = match name.map(str::trim) {
            Some(n) if !n.is_empty() => n.to_string(),
            _ => suffixed(&source.name, NAME_SUFFIX),
        };
        let code = source
            .code
            .as_deref()
            .map(|c| suffixed(c, CODE_SUFFIX))
            .filter(|c| !c.trim().is_empty());
        let internal = source.internal.len();
        let grain = source.grainline.is_some();
        if grain || internal > 0 {
            out.notes.push(format!(
                "Le droit-fil et les éléments internes de la pièce « {} » ne sont pas repris sur \
                 la pièce dépliée ({} élément(s) interne(s){}) : à replacer sur la pièce entière.",
                source.name,
                internal,
                if grain { ", droit-fil" } else { "" }
            ));
        }
        // Marquages (US-06.3) : jamais reportés, jamais perdus en silence.
        if !source.markings.is_empty() {
            out.notes.push(format!(
                "Les {} marquage(s) de la pièce « {} » ne sont pas repris sur la pièce dépliée : \
                 à replacer sur la pièce entière.",
                source.markings.len(),
                source.name
            ));
        }
        // Pinces (US-07.1) : jamais reportées, jamais perdues en silence.
        if !source.darts.is_empty() {
            out.notes.push(format!(
                "Les {} pince(s) de la pièce « {} » ne sont pas reprises sur la pièce dépliée : à \
                 replacer sur la pièce entière.",
                source.darts.len(),
                source.name
            ));
        }
        // Marge de couture (US-05.1) : jamais reportée (la dépliée est dérivée liée), jamais
        // perdue en silence.
        if source.seam.is_some() {
            out.notes.push(format!(
                "La marge de couture de la pièce « {} » n'est pas reprise sur la pièce dépliée : \
                 à régler sur la pièce entière.",
                source.name
            ));
        }
        // Coupe nette (US-05.6) : même règle que la marge (la dépliée est dérivée liée).
        if source.net_cut {
            out.notes.push(format!(
                "La coupe nette de la pièce « {} » n'est pas reprise sur la pièce dépliée : à \
                 déclarer sur la pièce entière.",
                source.name
            ));
        }
        // Bords sur le pli (PH-5b) : celui qu'on déplie disparaît du contour de la pièce entière ;
        // les autres ne sont pas repris (jamais perdus en silence).
        let other_folds = source.on_fold.iter().filter(|e| **e != fold).count();
        if other_folds > 0 {
            out.notes.push(format!(
                "Les {other_folds} autre(s) bord(s) sur le pli de la pièce « {} » ne sont pas \
                 repris sur la pièce dépliée : à déclarer sur la pièce entière.",
                source.name
            ));
        }
        // Groupes de boutons (US-06.4) : même règle, jamais perdus en silence.
        if !source.buttons.is_empty() {
            out.notes.push(format!(
                "Les {} groupe(s) de boutons de la pièce « {} » ne sont pas repris sur la pièce \
                 dépliée : à replacer sur la pièce entière.",
                source.buttons.len(),
                source.name
            ));
        }
        if !source.stitch_lines.is_empty() {
            out.notes.push(format!(
                "Les {} ligne(s) de piqûre de la pièce « {} » ne sont pas reprises sur la pièce                  dépliée : à replacer sur la pièce entière.",
                source.stitch_lines.len(),
                source.name
            ));
        }
        if !source.zones.is_empty() {
            out.notes.push(format!(
                "Les {} zone(s) de la pièce « {} » ne sont pas reprises sur la pièce dépliée : à \
                 replacer sur la pièce entière.",
                source.zones.len(),
                source.name
            ));
        }
        (name, code)
    }

    /// Refuse un second dépliage de la même pliure de la même pièce.
    pub(crate) fn check_not_already_unfolded(
        &self,
        mother: PieceId,
        fold: EntityId,
    ) -> Result<(), CoreError> {
        let same = self.pieces.iter().find(|p| {
            p.derived
                .as_ref()
                .is_some_and(|d| d.mother == mother && d.operation == DerivedOp::Unfold { fold })
        });
        match same {
            Some(p) => Err(CoreError::Invalid(format!(
                "cette pliure est déjà dépliée : la pièce « {} » en est le résultat",
                p.name
            ))),
            None => Ok(()),
        }
    }
}
