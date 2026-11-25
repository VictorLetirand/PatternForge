//! PH-5d — pièce « sans droit-fil » (cuir : la peau n'a pas de sens de tissage).
//!
//! `Piece.no_grain` déclare que l'absence de droit-fil est **voulue** : la pièce n'émet plus
//! l'avertissement « droit-fil manquant » (fiche, rapport de contrôle, rapport DXF) et les exports
//! écrivent « Sans droit-fil » / « No grain » dans son étiquette.
//!
//! Conventions (à faire valider par une modéliste, voir le suivi) :
//!
//! - la déclaration est **exclusive** avec un droit-fil : la déclarer sur une pièce qui en a un est
//!   refusé, poser un droit-fil sur une pièce déclarée sans droit-fil est refusé (jamais un retrait
//!   silencieux de l'un ou de l'autre) ; la matière (`material`, `category`) n'implique rien : une
//!   pièce de catégorie « cuir » sans déclaration garde son avertissement ;
//! - une pièce dérivée liée ne se déclare pas elle-même (la dépliée et la décalée reprennent la
//!   déclaration de leur source à la création) ;
//! - rien n'est stocké d'autre qu'un booléen.

use crate::error::CoreError;
use crate::model::*;
use serde::Deserialize;

fn invalid(msg: impl Into<String>) -> CoreError {
    CoreError::Invalid(msg.into())
}

/// Lecture de `no_grain` : seul `true` est écrit, `false` explicite est refusé (forme unique).
pub(crate) fn no_grain_only_true<'de, D: serde::Deserializer<'de>>(d: D) -> Result<bool, D::Error> {
    if bool::deserialize(d)? {
        Ok(true)
    } else {
        Err(serde::de::Error::custom(
            "no_grain : omettez le champ plutôt que d'écrire false",
        ))
    }
}

impl Project {
    /// `SetPieceNoGrain` : déclare ou retire « sans droit-fil ». Sans effet (aucune étape) si rien
    /// ne change. Refus expliqué, sans trace : pièce inconnue, pièce dérivée liée, et, à la
    /// déclaration, pièce qui a déjà un droit-fil.
    pub(crate) fn set_piece_no_grain(
        &mut self,
        piece: PieceId,
        no_grain: bool,
    ) -> Result<(), CoreError> {
        let p = self.piece(piece)?;
        if p.derived.is_some() {
            return Err(invalid(format!(
                "la pièce « {} » est une pièce dérivée liée à sa mère : cassez le lien avant de \
                 déclarer qu'elle est sans droit-fil",
                p.name
            )));
        }
        if no_grain == p.no_grain {
            return Ok(());
        }
        if no_grain && (p.grainline.is_some() || p.grain.is_some()) {
            return Err(invalid(format!(
                "la pièce « {} » a un droit-fil : retirez-le avant de la déclarer sans droit-fil",
                p.name
            )));
        }
        self.piece_mut(piece)?.no_grain = no_grain;
        Ok(())
    }

    /// Validation d'une pièce chargée ou modifiée : « sans droit-fil » exclut tout droit-fil. C'est
    /// ici que sont refusées les commandes qui poseraient un droit-fil sur une pièce déclarée.
    pub(crate) fn validate_no_grain(piece: &Piece) -> Result<(), CoreError> {
        if piece.no_grain && (piece.grainline.is_some() || piece.grain.is_some()) {
            return Err(invalid(format!(
                "pièce « {} » : déclarée sans droit-fil, elle ne peut pas avoir de droit-fil \
                 (retirez la déclaration « sans droit-fil » avant d'en poser un)",
                piece.name
            )));
        }
        Ok(())
    }
}
