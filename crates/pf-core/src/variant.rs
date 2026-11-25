//! US-11.1 — variantes d'un modèle (tranche 119 ; identifiant d'entrée et options renommables /
//! déplaçables : schéma 6, tranche 137).
//!
//! Un modèle = la liste de **pièces** du projet (uniques) ; une variante est un sous-ensemble de
//! ces pièces **référencées** (jamais copiées) avec une quantité, une matière facultative et une
//! option facultative par entrée. Modifier une pièce partagée (nom, marge, géométrie, matière) la
//! modifie donc dans toutes les variantes : c'est le critère d'acceptation de la story.
//!
//! `Project.variants` = liste de `{ id, name, options?, options_active?, entries? }`. Une
//! **option** est un nom (« avec poche ») propre à la variante ; une entrée peut y être rattachée ;
//! l'option est active ou non **dans la variante** (`options_active`). Une entrée est identifiée
//! par le couple (pièce, option) : deux entrées de même pièce et même option sont refusées ; depuis
//! le schéma 6 elle porte aussi un `VariantEntryId` stable (les commandes restent adressées par
//! (pièce, option)).
//!
//! Rien de dérivé n'est stocké (quantité totale, surface, nomenclature : `Project::bill_of_
//! materials_variant`). Les variantes ne sont **pas** les tailles (`Project.sizes`) et rien ne
//! « suit » automatiquement : la valeur par défaut d'un projet est « pas de variante ».
//!
//! Intégrité : supprimer une pièce citée par une variante est **refusé** (la validation nomme la
//! variante) ; supprimer une option utilisée par une entrée est refusé (l'entrée est nommée).

use crate::command::Outcome;
use crate::error::CoreError;
use crate::grading_rule::check_stored_name;
use crate::measurement_ops::{clean_name, same_name};
use crate::model::*;
use serde::{Deserialize, Serialize};

/// Nombre maximal de variantes d'un projet.
pub const MAX_VARIANTS: usize = 50;
/// Nombre maximal d'entrées d'une variante.
pub const MAX_VARIANT_ENTRIES: usize = 500;
/// Nombre maximal d'options d'une variante.
pub const MAX_VARIANT_OPTIONS: usize = 20;
/// Plus grande quantité d'une entrée.
pub const MAX_VARIANT_QUANTITY: u32 = 999;

/// Une pièce retenue dans une variante.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VariantEntry {
    /// Identifiant stable (schéma v6, décision 457) : attribué à la création par le compteur du
    /// projet, jamais réutilisé ; la migration v5 → v6 le donne dans l'ordre du fichier. Les
    /// commandes existantes continuent d'adresser l'entrée par (pièce, option).
    pub id: VariantEntryId,
    pub piece: PieceId,
    /// Nombre d'instances dans la variante, de 1 à 999 (la paire en miroir double à la lecture).
    pub quantity: u32,
    /// Surcharge de la matière de la pièce (texte libre, comme E05.5) ; absent = celle de la pièce.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<String>,
    /// Option de la variante à laquelle l'entrée est rattachée ; absent = toujours présente.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub option: Option<String>,
}

/// Variante stockée dans [`Project::variants`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Variant {
    pub id: VariantId,
    pub name: String,
    /// Options nommées de la variante (uniques sans tenir compte de la casse).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<String>,
    /// Options actives, sous-ensemble de `options` (une option absente d'ici est inactive).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options_active: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entries: Vec<VariantEntry>,
}

impl Variant {
    /// Vrai si l'entrée compte dans la variante (sans option, ou option active).
    pub fn entry_counts(&self, e: &VariantEntry) -> bool {
        match &e.option {
            None => true,
            Some(o) => self.options_active.iter().any(|a| a == o),
        }
    }

    pub fn option_active(&self, option: &str) -> bool {
        self.options_active.iter().any(|a| a == option)
    }
}

fn bad(msg: String) -> CoreError {
    CoreError::Invalid(msg)
}

fn check_quantity(q: u32, what: &str) -> Result<(), CoreError> {
    if !(1..=MAX_VARIANT_QUANTITY).contains(&q) {
        return Err(bad(format!(
            "{what} : la quantité doit être comprise entre 1 et {MAX_VARIANT_QUANTITY} (reçu {q})"
        )));
    }
    Ok(())
}

impl Project {
    pub fn variant(&self, id: VariantId) -> Result<&Variant, CoreError> {
        self.variants
            .iter()
            .find(|v| v.id == id)
            .ok_or_else(|| bad(format!("variante introuvable (id {})", id.0)))
    }

    fn variant_mut(&mut self, id: VariantId) -> Result<&mut Variant, CoreError> {
        self.variants
            .iter_mut()
            .find(|v| v.id == id)
            .ok_or_else(|| bad(format!("variante introuvable (id {})", id.0)))
    }

    fn check_new_variant_name(
        &self,
        name: &str,
        except: Option<VariantId>,
    ) -> Result<(), CoreError> {
        if self
            .variants
            .iter()
            .any(|v| Some(v.id) != except && same_name(&v.name, name))
        {
            return Err(bad(format!(
                "une variante « {name} » existe déjà (casse ignorée)"
            )));
        }
        Ok(())
    }

    /// `AddVariant` : variante vide (aucune entrée, aucune option).
    pub(crate) fn add_variant(&mut self, name: &str, out: &mut Outcome) -> Result<(), CoreError> {
        let name = clean_name(name, "le nom de la variante")?;
        self.check_new_variant_name(&name, None)?;
        if self.variants.len() >= MAX_VARIANTS {
            return Err(bad(format!("trop de variantes : {MAX_VARIANTS} au plus")));
        }
        let id = VariantId(self.alloc_id());
        self.variants.push(Variant {
            id,
            name,
            options: Vec::new(),
            options_active: Vec::new(),
            entries: Vec::new(),
        });
        out.variants.push(id);
        Ok(())
    }

    /// `RenameVariant` (même nom = aucune étape).
    pub(crate) fn rename_variant(&mut self, id: VariantId, name: &str) -> Result<(), CoreError> {
        self.variant(id)?;
        let name = clean_name(name, "le nom de la variante")?;
        self.check_new_variant_name(&name, Some(id))?;
        self.variant_mut(id)?.name = name;
        Ok(())
    }

    /// `RemoveVariant` : la variante et ses entrées ; les pièces ne changent pas.
    pub(crate) fn remove_variant(&mut self, id: VariantId) -> Result<(), CoreError> {
        self.variant(id)?;
        self.variants.retain(|v| v.id != id);
        Ok(())
    }

    /// `DuplicateVariant` : copie des options, de leur état et des entrées sous un nouveau nom.
    pub(crate) fn duplicate_variant(
        &mut self,
        id: VariantId,
        name: &str,
        out: &mut Outcome,
    ) -> Result<(), CoreError> {
        let src = self.variant(id)?.clone();
        let name = clean_name(name, "le nom de la variante")?;
        self.check_new_variant_name(&name, None)?;
        if self.variants.len() >= MAX_VARIANTS {
            return Err(bad(format!("trop de variantes : {MAX_VARIANTS} au plus")));
        }
        let new_id = VariantId(self.alloc_id());
        let mut copy = Variant {
            id: new_id,
            name,
            ..src
        };
        for e in &mut copy.entries {
            e.id = VariantEntryId(self.alloc_id());
        }
        self.variants.push(copy);
        out.variants.push(new_id);
        Ok(())
    }

    /// Nom d'option tel qu'il est stocké dans la variante (casse ignorée), sinon refus.
    fn resolve_option(&self, v: &Variant, option: &str) -> Result<String, CoreError> {
        let o = option.trim();
        v.options
            .iter()
            .find(|s| same_name(s, o))
            .cloned()
            .ok_or_else(|| bad(format!("variante « {} » : option « {o} » inconnue", v.name)))
    }

    fn resolve_entry_option(
        &self,
        v: &Variant,
        option: &Option<String>,
    ) -> Result<Option<String>, CoreError> {
        option
            .as_deref()
            .map(|o| self.resolve_option(v, o))
            .transpose()
    }

    fn stored_material(material: &Option<String>, v: &str) -> Result<Option<String>, CoreError> {
        material
            .as_deref()
            .map(|m| {
                clean_name(
                    m,
                    &format!("la matière d'une entrée de la variante « {v} »"),
                )
            })
            .transpose()
    }

    /// `AddVariantEntry`.
    pub(crate) fn add_variant_entry(
        &mut self,
        id: VariantId,
        piece: PieceId,
        quantity: u32,
        material: &Option<String>,
        option: &Option<String>,
    ) -> Result<(), CoreError> {
        let v = self.variant(id)?;
        let what = format!("variante « {} »", v.name);
        self.piece(piece)?;
        check_quantity(quantity, &what)?;
        let material = Self::stored_material(material, &v.name)?;
        let option = self.resolve_entry_option(v, option)?;
        if v.entries
            .iter()
            .any(|e| e.piece == piece && e.option == option)
        {
            return Err(bad(format!(
                "{what} : la pièce {} figure déjà avec la même option",
                self.piece(piece)?.name
            )));
        }
        if v.entries.len() >= MAX_VARIANT_ENTRIES {
            return Err(bad(format!(
                "{what} : trop d'entrées ({MAX_VARIANT_ENTRIES} au plus)"
            )));
        }
        let entry_id = VariantEntryId(self.alloc_id());
        self.variant_mut(id)?.entries.push(VariantEntry {
            id: entry_id,
            piece,
            quantity,
            material,
            option,
        });
        Ok(())
    }

    /// `UpdateVariantEntry` : l'entrée (`piece`, `option`) reçoit une nouvelle quantité, matière
    /// et option (la pièce ne change pas ; mêmes valeurs = aucune étape).
    pub(crate) fn update_variant_entry(
        &mut self,
        id: VariantId,
        piece: PieceId,
        option: &Option<String>,
        quantity: u32,
        material: &Option<String>,
        new_option: &Option<String>,
    ) -> Result<(), CoreError> {
        let v = self.variant(id)?;
        let what = format!("variante « {} »", v.name);
        let old_option = self.resolve_entry_option(v, option)?;
        let at = v
            .entries
            .iter()
            .position(|e| e.piece == piece && e.option == old_option)
            .ok_or_else(|| bad(format!("{what} : entrée introuvable")))?;
        let entry_id = v.entries[at].id;
        check_quantity(quantity, &what)?;
        let material = Self::stored_material(material, &v.name)?;
        let new_option = self.resolve_entry_option(v, new_option)?;
        if v.entries
            .iter()
            .enumerate()
            .any(|(i, e)| i != at && e.piece == piece && e.option == new_option)
        {
            return Err(bad(format!(
                "{what} : la pièce {} figure déjà avec cette option",
                self.piece(piece)?.name
            )));
        }
        self.variant_mut(id)?.entries[at] = VariantEntry {
            id: entry_id,
            piece,
            quantity,
            material,
            option: new_option,
        };
        Ok(())
    }

    /// `RemoveVariantEntry`.
    pub(crate) fn remove_variant_entry(
        &mut self,
        id: VariantId,
        piece: PieceId,
        option: &Option<String>,
    ) -> Result<(), CoreError> {
        let v = self.variant(id)?;
        let option = self.resolve_entry_option(v, option)?;
        let at = v
            .entries
            .iter()
            .position(|e| e.piece == piece && e.option == option)
            .ok_or_else(|| bad(format!("variante « {} » : entrée introuvable", v.name)))?;
        self.variant_mut(id)?.entries.remove(at);
        Ok(())
    }

    /// `AddVariantOption` : nouvelle option, **inactive** par défaut.
    pub(crate) fn add_variant_option(
        &mut self,
        id: VariantId,
        option: &str,
    ) -> Result<(), CoreError> {
        let v = self.variant(id)?;
        let o = clean_name(
            option,
            &format!("le nom d'une option de la variante « {} »", v.name),
        )?;
        if v.options.iter().any(|s| same_name(s, &o)) {
            return Err(bad(format!(
                "variante « {} » : l'option « {o} » existe déjà (casse ignorée)",
                v.name
            )));
        }
        if v.options.len() >= MAX_VARIANT_OPTIONS {
            return Err(bad(format!(
                "variante « {} » : trop d'options ({MAX_VARIANT_OPTIONS} au plus)",
                v.name
            )));
        }
        self.variant_mut(id)?.options.push(o);
        Ok(())
    }

    /// `RemoveVariantOption` : refusé si une entrée s'y rattache (l'entrée est nommée).
    pub(crate) fn remove_variant_option(
        &mut self,
        id: VariantId,
        option: &str,
    ) -> Result<(), CoreError> {
        let v = self.variant(id)?;
        let o = self.resolve_option(v, option)?;
        if let Some(e) = v.entries.iter().find(|e| e.option.as_ref() == Some(&o)) {
            return Err(bad(format!(
                "variante « {} » : l'option « {o} » est utilisée par l'entrée de la pièce « {} » : retirez ou changez cette entrée d'abord",
                v.name,
                self.piece(e.piece).map(|p| p.name.as_str()).unwrap_or("?")
            )));
        }
        let v = self.variant_mut(id)?;
        v.options.retain(|s| *s != o);
        v.options_active.retain(|s| *s != o);
        Ok(())
    }

    /// `RenameVariantOption` : renomme l'option **et** réécrit son état actif et les entrées qui la
    /// citent (une seule étape). Refus : nom vide ou invalide, option inconnue, nom déjà pris par
    /// une autre option (casse ignorée ; changer seulement la casse de la même option est permis).
    pub(crate) fn rename_variant_option(
        &mut self,
        id: VariantId,
        option: &str,
        name: &str,
    ) -> Result<(), CoreError> {
        let v = self.variant(id)?;
        let old = self.resolve_option(v, option)?;
        let new = clean_name(
            name,
            &format!("le nom d'une option de la variante « {} »", v.name),
        )?;
        if v.options.iter().any(|s| *s != old && same_name(s, &new)) {
            return Err(bad(format!(
                "variante « {} » : l'option « {new} » existe déjà (casse ignorée)",
                v.name
            )));
        }
        let v = self.variant_mut(id)?;
        for s in v.options.iter_mut().chain(v.options_active.iter_mut()) {
            if *s == old {
                s.clone_from(&new);
            }
        }
        for e in &mut v.entries {
            if e.option.as_ref() == Some(&old) {
                e.option = Some(new.clone());
            }
        }
        Ok(())
    }

    /// `MoveVariantOption` : l'option passe à la position `to` (0 = première) ; les autres gardent
    /// leur ordre. Refus : option inconnue, position hors de la liste. Même position = aucune étape.
    pub(crate) fn move_variant_option(
        &mut self,
        id: VariantId,
        option: &str,
        to: usize,
    ) -> Result<(), CoreError> {
        let v = self.variant(id)?;
        let o = self.resolve_option(v, option)?;
        if to >= v.options.len() {
            return Err(bad(format!(
                "variante « {} » : position {to} hors de la liste de {} option(s)",
                v.name,
                v.options.len()
            )));
        }
        let v = self.variant_mut(id)?;
        if let Some(from) = v.options.iter().position(|s| *s == o) {
            let moved = v.options.remove(from);
            v.options.insert(to, moved);
        }
        Ok(())
    }

    /// `SetVariantOptionActive` (même état = aucune étape).
    pub(crate) fn set_variant_option_active(
        &mut self,
        id: VariantId,
        option: &str,
        active: bool,
    ) -> Result<(), CoreError> {
        let v = self.variant(id)?;
        let o = self.resolve_option(v, option)?;
        let v = self.variant_mut(id)?;
        let is = v.options_active.contains(&o);
        if active && !is {
            v.options_active.push(o);
        } else if !active && is {
            v.options_active.retain(|s| *s != o);
        }
        Ok(())
    }

    /// Validation des variantes (appelée par `Project::validate`) : noms, bornes, références.
    pub(crate) fn validate_variants(&self) -> Result<(), CoreError> {
        if self.variants.len() > MAX_VARIANTS {
            return Err(bad(format!("trop de variantes : {MAX_VARIANTS} au plus")));
        }
        for (i, v) in self.variants.iter().enumerate() {
            let what = format!("variante « {} »", v.name);
            check_stored_name(&v.name, &format!("le nom de la variante {}", v.id.0))?;
            if self.variants[..i]
                .iter()
                .any(|o| same_name(&o.name, &v.name))
            {
                return Err(bad(format!(
                    "deux variantes « {} » (casse ignorée)",
                    v.name
                )));
            }
            if v.options.len() > MAX_VARIANT_OPTIONS {
                return Err(bad(format!(
                    "{what} : trop d'options ({MAX_VARIANT_OPTIONS} au plus)"
                )));
            }
            if v.entries.len() > MAX_VARIANT_ENTRIES {
                return Err(bad(format!(
                    "{what} : trop d'entrées ({MAX_VARIANT_ENTRIES} au plus)"
                )));
            }
            for (j, o) in v.options.iter().enumerate() {
                check_stored_name(o, &format!("une option de la {what}"))?;
                if v.options[..j].iter().any(|p| same_name(p, o)) {
                    return Err(bad(format!(
                        "{what} : deux options « {o} » (casse ignorée)"
                    )));
                }
            }
            for (j, a) in v.options_active.iter().enumerate() {
                if !v.options.contains(a) {
                    return Err(bad(format!(
                        "{what} : l'option active « {a} » n'existe pas dans la variante"
                    )));
                }
                if v.options_active[..j].contains(a) {
                    return Err(bad(format!(
                        "{what} : l'option active « {a} » est écrite deux fois"
                    )));
                }
            }
            for (j, e) in v.entries.iter().enumerate() {
                if self.piece(e.piece).is_err() {
                    return Err(CoreError::DeadReference(format!(
                        "la {what} cite la pièce {} qui n'existe plus : retirez d'abord son entrée",
                        e.piece.0
                    )));
                }
                check_quantity(e.quantity, &what)?;
                if let Some(m) = &e.material {
                    check_stored_name(m, &format!("la matière d'une entrée de la {what}"))?;
                }
                if let Some(o) = &e.option {
                    if !v.options.contains(o) {
                        return Err(CoreError::DeadReference(format!(
                            "la {what} : l'entrée de la pièce {} cite l'option « {o} » qui n'existe pas",
                            e.piece.0
                        )));
                    }
                }
                if v.entries[..j]
                    .iter()
                    .any(|p| p.piece == e.piece && p.option == e.option)
                {
                    return Err(bad(format!(
                        "{what} : la pièce {} figure deux fois avec la même option",
                        e.piece.0
                    )));
                }
            }
        }
        Ok(())
    }
}
