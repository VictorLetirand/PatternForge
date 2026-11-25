//! US-11.2 — nomenclature (tranche 111). Lecture pure : rien n'est stocké, schéma inchangé.
//!
//! Une ligne par pièce (feuille, matière, catégorie, quantité) avec son aire nette **par taille**
//! (contour de couture du projet évalué dans cette taille, comme `size_measures`), puis un total
//! par matière : nombre de découpes et surface nette (aire × découpes) par taille.
//!
//! Conventions (à faire valider) : **découpes = quantité × 2 si la pièce est « paire en miroir »**,
//! sinon quantité ; matière = texte de la fiche, espaces de bord retirés, casse ignorée pour le
//! regroupement (graphie de la première pièce conservée), vide = « non renseignée » (groupe à
//! part, en dernier) ; une aire illisible (contour ouvert, qui se recoupe, absent) vaut `None` et
//! rend `None` le total de sa matière dans cette taille (jamais une somme partielle). Sans gamme,
//! une seule colonne : le projet tel qu'il est.
//!
//! **Par variante (US-11.1, tranche 119)** : `bill_of_materials_variant(range, Some(v))` lit les
//! **entrées** de la variante au lieu des pièces : une ligne par entrée dont l'option est active (ou
//! absente) ; découpes = quantité de l'entrée × 2 si la pièce est une paire en miroir ; matière =
//! surcharge de l'entrée sinon matière de la pièce ; l'option d'une entrée est portée par la ligne.
//! Sans variante (`None`) : le projet entier, comme avant (`quantity` de la pièce).

use crate::error::CoreError;
use crate::model::*;

/// Une pièce de la nomenclature.
#[derive(Debug, Clone, PartialEq)]
pub struct BomLine {
    pub piece: PieceId,
    pub name: String,
    pub code: Option<String>,
    pub sheet: String,
    pub material: Option<String>,
    pub category: Option<PieceCategory>,
    pub quantity: u32,
    /// Option de l'entrée de variante (jamais en mode « tout le projet »).
    pub option: Option<String>,
    pub mirrored_pair: bool,
    /// Nombre de découpes : `quantity`, doublé pour une paire en miroir.
    pub cuts: u32,
    /// Aire d'une pièce (mm²), une valeur par colonne de taille ; `None` = illisible.
    pub areas_mm2: Vec<Option<f64>>,
}

/// Total d'une matière.
#[derive(Debug, Clone, PartialEq)]
pub struct BomMaterial {
    /// `None` = matière non renseignée.
    pub material: Option<String>,
    pub pieces: usize,
    pub cuts: u32,
    /// Surface nette (mm²) par colonne : somme de aire × découpes ; `None` si une aire manque.
    pub net_area_mm2: Vec<Option<f64>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Bom {
    /// Gamme lue ; `None` = projet tel quel (une colonne).
    pub range: Option<SizeRangeId>,
    /// Variante lue ; `None` = tout le projet (une ligne par pièce, quantité de la pièce).
    pub variant: Option<VariantId>,
    /// Étiquettes des colonnes (tailles de la gamme, ou vide si pas de gamme).
    pub labels: Vec<String>,
    pub lines: Vec<BomLine>,
    pub materials: Vec<BomMaterial>,
}

fn area_of(p: &Project, piece: PieceId) -> Option<f64> {
    let c = p.piece_contour(piece).ok()?;
    (c.closed && !c.self_intersecting && c.area_mm2.is_finite()).then_some(c.area_mm2)
}

fn norm(m: &Option<String>) -> Option<String> {
    m.as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

impl Project {
    /// Nomenclature du projet, avec une colonne par taille de `range` (ou une seule colonne sans
    /// gamme). Gamme inconnue ou sans taille : erreur typée.
    pub fn bill_of_materials(&self, range: Option<SizeRangeId>) -> Result<Bom, CoreError> {
        self.bill_of_materials_variant(range, None)
    }

    /// Nomenclature d'une variante (`Some`) ou de tout le projet (`None`) ; voir l'en-tête du
    /// module. Variante inconnue : erreur typée.
    pub fn bill_of_materials_variant(
        &self,
        range: Option<SizeRangeId>,
        variant: Option<VariantId>,
    ) -> Result<Bom, CoreError> {
        let variant_ref = variant.map(|id| self.variant(id)).transpose()?;
        let (labels, evaluated): (Vec<String>, Vec<Option<Project>>) = match range {
            None => (Vec::new(), vec![None]),
            Some(id) => {
                let r =
                    self.sizes.iter().find(|r| r.id == id).ok_or_else(|| {
                        CoreError::Invalid(format!("gamme inconnue (id {})", id.0))
                    })?;
                if r.sizes.is_empty() {
                    return Err(CoreError::Invalid(format!(
                        "gamme sans taille (id {})",
                        id.0
                    )));
                }
                (
                    r.sizes.iter().map(|s| s.label.clone()).collect(),
                    r.sizes
                        .iter()
                        .map(|s| self.graded_project(Some(s.id)).ok())
                        .collect(),
                )
            }
        };
        let columns = evaluated.len();
        // Une ligne par (pièce, entrée) : tout le projet = chaque pièce telle quelle ; variante =
        // ses entrées comptées (option active ou absente).
        type Source<'a> = (&'a Piece, u32, Option<String>, Option<String>);
        let sources: Vec<Source> = match variant_ref {
            None => self
                .pieces
                .iter()
                .map(|p| (p, p.quantity, p.material.clone(), None))
                .collect(),
            Some(v) => v
                .entries
                .iter()
                .filter(|e| v.entry_counts(e))
                .filter_map(|e| {
                    let p = self.piece(e.piece).ok()?;
                    let material = e.material.clone().or_else(|| p.material.clone());
                    Some((p, e.quantity, material, e.option.clone()))
                })
                .collect(),
        };
        let mut lines = Vec::new();
        for (p, quantity, material, option) in sources {
            let areas: Vec<Option<f64>> = (0..columns)
                .map(|i| match (&evaluated[i], range) {
                    (Some(g), _) => area_of(g, p.id),
                    (None, None) => area_of(self, p.id),
                    (None, Some(_)) => None,
                })
                .collect();
            let sheet = self
                .piece_sheet(p.id)
                .map(|s| s.name.clone())
                .unwrap_or_default();
            lines.push(BomLine {
                piece: p.id,
                name: p.name.clone(),
                code: norm(&p.code),
                sheet,
                material: norm(&material),
                category: p.category,
                quantity,
                option,
                mirrored_pair: p.mirrored_pair,
                cuts: quantity.saturating_mul(if p.mirrored_pair { 2 } else { 1 }),
                areas_mm2: areas,
            });
        }
        let mut materials: Vec<BomMaterial> = Vec::new();
        let mut keys: Vec<Option<String>> = Vec::new();
        for l in &lines {
            let key = l.material.as_ref().map(|m| m.to_lowercase());
            let at = keys.iter().position(|k| *k == key).unwrap_or_else(|| {
                keys.push(key);
                materials.push(BomMaterial {
                    material: l.material.clone(),
                    pieces: 0,
                    cuts: 0,
                    net_area_mm2: vec![Some(0.0); columns],
                });
                materials.len() - 1
            });
            let m = &mut materials[at];
            m.pieces += 1;
            m.cuts = m.cuts.saturating_add(l.cuts);
            for (total, a) in m.net_area_mm2.iter_mut().zip(&l.areas_mm2) {
                *total = match (*total, a) {
                    (Some(t), Some(a)) => Some(t + a * f64::from(l.cuts)),
                    _ => None,
                };
            }
        }
        // « non renseignée » en dernier ; l'ordre d'apparition est conservé sinon.
        materials.sort_by_key(|m| m.material.is_none());
        Ok(Bom {
            range,
            variant,
            labels,
            lines,
            materials,
        })
    }
}
