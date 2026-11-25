//! Plans de coupe (socle d'E15, schéma 5 inchangé) : **stockage et commandes seulement**.
//!
//! `Project.cutting_plans` = liste de plans `{ id, name, material?, fabric_width_mm, fold,
//! selvedge_mm, min_gap_mm, grain_direction?, placements }`. Un placement
//! ([`CuttingPlacement`], à ne pas confondre avec [`crate::Placement`] du plan de pièces
//! d'US-04.5) pose **une** instance d'une pièce (taille optionnelle : absente = taille de base)
//! à `(x_mm, y_mm)` avec une rotation et un miroir éventuels.
//!
//! Le contenu est un **état posé** par l'utilisateur. Rien de dérivé n'est stocké : ni longueur
//! consommée, ni rendement, ni contour posé, ni chevauchement (lots suivants, US-15.2 / 15.3).
//! `fold = Folded` : la sémantique (moitié de la laize = plan de symétrie) est celle du lot
//! suivant ; ici la valeur est seulement conservée.
//!
//! Intégrité : supprimer une pièce ou une taille référencée par un placement est **refusé**
//! (la validation du projet nomme le plan) ; aucun état « obsolète ». Les plans ne sont pas
//! copiés par la duplication d'une feuille (une remarque le dit).
//! Unités : mm et degrés ; rotation et sens du droit-fil dans `[0 ; 360[`.

use crate::command::Outcome;
use crate::error::CoreError;
use crate::grading_rule::check_stored_name;
use crate::measurement_ops::{clean_name, same_name};
use crate::model::*;
use crate::piece_plan::normalize_rotation_deg;
use serde::{Deserialize, Serialize};

/// Plus grande laize (mm).
pub const MAX_FABRIC_WIDTH_MM: f64 = 10_000.0;
/// Plus grand écart minimal entre pièces (mm).
pub const MAX_MIN_GAP_MM: f64 = 1_000.0;
/// Borne de la position d'un placement (mm, en valeur absolue).
pub const MAX_PLACEMENT_COORD_MM: f64 = 1.0e6;
/// Borne de la rotation acceptée en entrée d'une commande (degrés, avant normalisation).
pub const MAX_PLACEMENT_ROTATION_INPUT_DEG: f64 = 1.0e6;

/// Matière posée à plat ou pliée en deux dans le sens de la laize.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FabricFold {
    Flat,
    Folded,
}

impl FabricFold {
    /// Identifiant stable, indépendant de la langue.
    pub fn key(self) -> &'static str {
        match self {
            FabricFold::Flat => "flat",
            FabricFold::Folded => "folded",
        }
    }
}

/// Paramètres d'un plan de coupe (création et remplacement).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CuttingPlanSpec {
    pub name: String,
    /// Nom de matière (E05.5), sans validation d'existence ; `None` = non renseignée.
    pub material: Option<String>,
    pub fabric_width_mm: f64,
    pub fold: FabricFold,
    pub selvedge_mm: f64,
    pub min_gap_mm: f64,
    /// Sens du droit-fil de la matière dans le plan (degrés) ; `None` = non renseigné.
    pub grain_direction: Option<f64>,
}

/// Une pièce posée dans un plan de coupe.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CuttingPlacement {
    pub id: PlacementId,
    pub piece: PieceId,
    /// Taille posée ; `None` = taille de base.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<SizeId>,
    pub x_mm: f64,
    pub y_mm: f64,
    /// Rotation en degrés, sens trigonométrique, dans `[0 ; 360[`.
    pub rotation_deg: f64,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub mirrored: bool,
}

/// Plan de coupe stocké dans [`Project::cutting_plans`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CuttingPlan {
    pub id: CuttingPlanId,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<String>,
    pub fabric_width_mm: f64,
    pub fold: FabricFold,
    pub selvedge_mm: f64,
    pub min_gap_mm: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grain_direction: Option<f64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub placements: Vec<CuttingPlacement>,
    /// Peau (E15, schéma v6) : contour, défauts et zones de qualité ; stockage seulement.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hide: Option<crate::hide::Hide>,
}

fn bad(msg: String) -> CoreError {
    CoreError::Invalid(msg)
}

fn finite_in(v: f64, min: f64, max: f64, what: &str) -> Result<(), CoreError> {
    if !v.is_finite() {
        return Err(bad(format!("{what} : valeur non finie")));
    }
    if v < min || v > max {
        return Err(bad(format!("{what} : {v} hors de [{min} ; {max}]")));
    }
    Ok(())
}

/// Valeurs d'un plan, sans le nom ni la matière (déjà nettoyés).
fn check_plan_numbers(spec: &CuttingPlanSpec, what: &str) -> Result<(), CoreError> {
    if !spec.fabric_width_mm.is_finite()
        || spec.fabric_width_mm <= 0.0
        || spec.fabric_width_mm > MAX_FABRIC_WIDTH_MM
    {
        return Err(bad(format!(
            "{what} : la laize doit être finie, > 0 et ≤ {MAX_FABRIC_WIDTH_MM} mm"
        )));
    }
    if !spec.selvedge_mm.is_finite() || spec.selvedge_mm < 0.0 {
        return Err(bad(format!("{what} : la lisière doit être finie et ≥ 0")));
    }
    if 2.0 * spec.selvedge_mm >= spec.fabric_width_mm {
        return Err(bad(format!(
            "{what} : deux lisières ({} mm) doivent laisser une largeur utile dans la laize ({} mm)",
            2.0 * spec.selvedge_mm,
            spec.fabric_width_mm
        )));
    }
    finite_in(
        spec.min_gap_mm,
        0.0,
        MAX_MIN_GAP_MM,
        &format!("{what} : écart minimal"),
    )?;
    if let Some(g) = spec.grain_direction {
        if !g.is_finite() || !(0.0..360.0).contains(&g) {
            return Err(bad(format!(
                "{what} : le droit-fil doit être fini et dans [0 ; 360["
            )));
        }
    }
    Ok(())
}

fn check_placement_numbers(p: &CuttingPlacement, what: &str) -> Result<(), CoreError> {
    finite_in(
        p.x_mm,
        -MAX_PLACEMENT_COORD_MM,
        MAX_PLACEMENT_COORD_MM,
        &format!("{what} : x"),
    )?;
    finite_in(
        p.y_mm,
        -MAX_PLACEMENT_COORD_MM,
        MAX_PLACEMENT_COORD_MM,
        &format!("{what} : y"),
    )?;
    if !p.rotation_deg.is_finite() || !(0.0..360.0).contains(&p.rotation_deg) {
        return Err(bad(format!(
            "{what} : la rotation doit être finie et dans [0 ; 360["
        )));
    }
    Ok(())
}

impl CuttingPlanSpec {
    /// Nom et matière rognés, grain normalisé : forme stockée de la spécification.
    fn cleaned(&self) -> Result<CuttingPlanSpec, CoreError> {
        let name = clean_name(&self.name, "le nom du plan de coupe")?;
        let material = self
            .material
            .as_deref()
            .map(|m| clean_name(m, "la matière du plan de coupe"))
            .transpose()?;
        Ok(CuttingPlanSpec {
            name,
            material,
            // `+ 0.0` écrase -0.0 ; les bornes sont vérifiées par `check_plan_numbers`.
            fabric_width_mm: self.fabric_width_mm + 0.0,
            fold: self.fold,
            selvedge_mm: self.selvedge_mm + 0.0,
            min_gap_mm: self.min_gap_mm + 0.0,
            grain_direction: self.grain_direction.map(|g| g + 0.0),
        })
    }
}

impl Project {
    pub fn cutting_plan(&self, id: CuttingPlanId) -> Result<&CuttingPlan, CoreError> {
        self.cutting_plans
            .iter()
            .find(|p| p.id == id)
            .ok_or_else(|| bad(format!("plan de coupe introuvable (id {})", id.0)))
    }

    fn cutting_plan_mut(&mut self, id: CuttingPlanId) -> Result<&mut CuttingPlan, CoreError> {
        self.cutting_plans
            .iter_mut()
            .find(|p| p.id == id)
            .ok_or_else(|| bad(format!("plan de coupe introuvable (id {})", id.0)))
    }

    fn check_new_plan_name(
        &self,
        name: &str,
        except: Option<CuttingPlanId>,
    ) -> Result<(), CoreError> {
        if self
            .cutting_plans
            .iter()
            .any(|p| Some(p.id) != except && same_name(&p.name, name))
        {
            return Err(bad(format!(
                "un plan de coupe « {name} » existe déjà (casse ignorée)"
            )));
        }
        Ok(())
    }

    /// `AddCuttingPlan`.
    pub(crate) fn add_cutting_plan(
        &mut self,
        spec: &CuttingPlanSpec,
        out: &mut Outcome,
    ) -> Result<(), CoreError> {
        let s = spec.cleaned()?;
        self.check_new_plan_name(&s.name, None)?;
        check_plan_numbers(&s, &format!("plan de coupe « {} »", s.name))?;
        let id = CuttingPlanId(self.alloc_id());
        self.cutting_plans.push(CuttingPlan {
            id,
            name: s.name,
            material: s.material,
            fabric_width_mm: s.fabric_width_mm,
            fold: s.fold,
            selvedge_mm: s.selvedge_mm,
            min_gap_mm: s.min_gap_mm,
            grain_direction: s.grain_direction,
            placements: Vec::new(),
            hide: None,
        });
        out.cutting_plans.push(id);
        Ok(())
    }

    /// `UpdateCuttingPlan` : remplace les paramètres (les placements ne changent pas).
    pub(crate) fn update_cutting_plan(
        &mut self,
        id: CuttingPlanId,
        spec: &CuttingPlanSpec,
    ) -> Result<(), CoreError> {
        self.cutting_plan(id)?;
        let s = spec.cleaned()?;
        self.check_new_plan_name(&s.name, Some(id))?;
        check_plan_numbers(&s, &format!("plan de coupe « {} »", s.name))?;
        let plan = self.cutting_plan_mut(id)?;
        plan.name = s.name;
        plan.material = s.material;
        plan.fabric_width_mm = s.fabric_width_mm;
        plan.fold = s.fold;
        plan.selvedge_mm = s.selvedge_mm;
        plan.min_gap_mm = s.min_gap_mm;
        plan.grain_direction = s.grain_direction;
        Ok(())
    }

    /// `RemoveCuttingPlan` : le plan et ses placements disparaissent ensemble.
    pub(crate) fn remove_cutting_plan(&mut self, id: CuttingPlanId) -> Result<(), CoreError> {
        self.cutting_plan(id)?;
        self.cutting_plans.retain(|p| p.id != id);
        Ok(())
    }

    /// Placement dans la forme stockée : références vérifiées, nombres bornés, rotation ramenée
    /// dans `[0 ; 360[` (entrée bornée à ±10⁶ °).
    fn stored_placement(
        &self,
        plan_name: &str,
        raw: CuttingPlacement,
    ) -> Result<CuttingPlacement, CoreError> {
        let what = format!("plan de coupe « {plan_name} », placement");
        self.piece(raw.piece)?;
        if let Some(s) = raw.size {
            if self.find_size(s).is_none() {
                return Err(CoreError::DeadReference(format!(
                    "{what} : taille inconnue (id {})",
                    s.0
                )));
            }
        }
        finite_in(
            raw.rotation_deg,
            -MAX_PLACEMENT_ROTATION_INPUT_DEG,
            MAX_PLACEMENT_ROTATION_INPUT_DEG,
            &format!("{what} : rotation"),
        )?;
        let p = CuttingPlacement {
            x_mm: raw.x_mm + 0.0,
            y_mm: raw.y_mm + 0.0,
            rotation_deg: normalize_rotation_deg(raw.rotation_deg),
            ..raw
        };
        check_placement_numbers(&p, &what)?;
        Ok(p)
    }

    /// `AddPlacement` : une pièce peut être posée plusieurs fois (la quantité).
    pub(crate) fn add_placement(
        &mut self,
        plan: CuttingPlanId,
        raw: CuttingPlacement,
        out: &mut Outcome,
    ) -> Result<(), CoreError> {
        let name = self.cutting_plan(plan)?.name.clone();
        let id = PlacementId(self.alloc_id());
        let p = self.stored_placement(&name, CuttingPlacement { id, ..raw })?;
        self.cutting_plan_mut(plan)?.placements.push(p);
        out.placements.push(id);
        Ok(())
    }

    /// `UpdatePlacement` : position, rotation, miroir et taille (la pièce ne change pas).
    pub(crate) fn update_placement(
        &mut self,
        plan: CuttingPlanId,
        raw: CuttingPlacement,
    ) -> Result<(), CoreError> {
        let p = self.cutting_plan(plan)?;
        let name = p.name.clone();
        let old = p
            .placements
            .iter()
            .find(|q| q.id == raw.id)
            .copied()
            .ok_or_else(|| {
                bad(format!(
                    "plan de coupe « {name} » : placement introuvable (id {})",
                    raw.id.0
                ))
            })?;
        let new = self.stored_placement(
            &name,
            CuttingPlacement {
                piece: old.piece,
                ..raw
            },
        )?;
        if let Some(slot) = self
            .cutting_plan_mut(plan)?
            .placements
            .iter_mut()
            .find(|q| q.id == raw.id)
        {
            *slot = new;
        }
        Ok(())
    }

    /// `RemovePlacement`.
    pub(crate) fn remove_placement(
        &mut self,
        plan: CuttingPlanId,
        placement: PlacementId,
    ) -> Result<(), CoreError> {
        let p = self.cutting_plan_mut(plan)?;
        let before = p.placements.len();
        p.placements.retain(|q| q.id != placement);
        if p.placements.len() == before {
            return Err(bad(format!(
                "plan de coupe « {} » : placement introuvable (id {})",
                p.name, placement.0
            )));
        }
        Ok(())
    }

    /// Validation des plans (appelée par `Project::validate`) : noms, bornes, références. Les
    /// identifiants sont vérifiés (unicité, compteur) par `Project::validate`.
    pub(crate) fn validate_cutting_plans(&self) -> Result<(), CoreError> {
        for (i, p) in self.cutting_plans.iter().enumerate() {
            let what = format!("plan de coupe « {} »", p.name);
            check_stored_name(&p.name, &format!("le nom du plan de coupe {}", p.id.0))?;
            if self.cutting_plans[..i]
                .iter()
                .any(|o| same_name(&o.name, &p.name))
            {
                return Err(bad(format!(
                    "deux plans de coupe « {} » (casse ignorée)",
                    p.name
                )));
            }
            if let Some(m) = &p.material {
                check_stored_name(m, &format!("la matière du {what}"))?;
            }
            let spec = CuttingPlanSpec {
                name: p.name.clone(),
                material: p.material.clone(),
                fabric_width_mm: p.fabric_width_mm,
                fold: p.fold,
                selvedge_mm: p.selvedge_mm,
                min_gap_mm: p.min_gap_mm,
                grain_direction: p.grain_direction,
            };
            check_plan_numbers(&spec, &what)?;
            for pl in &p.placements {
                let at = format!("{what}, placement {}", pl.id.0);
                if self.piece(pl.piece).is_err() {
                    return Err(CoreError::DeadReference(format!(
                        "le {what} pose la pièce {} qui n'existe plus : retirez d'abord ses placements",
                        pl.piece.0
                    )));
                }
                if let Some(s) = pl.size {
                    if self.find_size(s).is_none() {
                        return Err(CoreError::DeadReference(format!(
                            "le {what} pose une pièce dans la taille {} qui n'existe plus : retirez d'abord ses placements",
                            s.0
                        )));
                    }
                }
                check_placement_numbers(pl, &at)?;
            }
        }
        Ok(())
    }
}
