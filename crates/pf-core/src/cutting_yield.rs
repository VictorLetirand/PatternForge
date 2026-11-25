//! Plans de coupe, métrage (US-15.3) : **longueur consommée, surface utilisée et rendement**,
//! calculés à la lecture à partir des diagnostics de [`crate::cutting_layout`] ; rien n'est stocké.
//!
//! - **longueur consommée** : de `y = 0` au bord le plus éloigné des contours posés (axe de la
//!   longueur de l'étoffe), sans marge inventée ;
//! - **surface utilisée** : somme des aires de **coupe** exactes des contours posés ;
//! - **laize utile** : laize moins deux lisières (aussi pour une étoffe pliée : le pli ne change
//!   pas la zone où les pièces sont posées) ;
//! - **rendement** : surface utilisée / (laize utile × longueur consommée), en pourcent ;
//! - **par taille** : une ligne par taille posée (taille de base en premier, puis l'ordre des
//!   gammes) ; la part d'une taille est sa surface sur la même surface de référence, donc la somme
//!   des parts égale le rendement du plan.
//!
//! Un plan avec chevauchement, écart violé, débordement, rotation refusée ou contour illisible est
//! **invalide** : le type ne porte alors **aucun chiffre** ([`PlanYield::figures`] vaut `None`), il
//! ne peut donc pas être affiché comme un nombre propre. Un plan sans pose est **vide**
//! (« non renseigné »). « Par variante » : non fait (les plans ne dépendent pas des variantes).

use crate::cutting_layout::PlanAnalysis;
use crate::cutting_plan::CuttingPlan;
use crate::error::CoreError;
use crate::model::*;

/// État du métrage d'un plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum YieldStatus {
    /// Aucune pose : non renseigné.
    Empty,
    /// Défauts de pose : aucun chiffre.
    Invalid,
    Valid,
}

impl YieldStatus {
    /// Identifiant stable, indépendant de la langue.
    pub fn key(self) -> &'static str {
        match self {
            YieldStatus::Empty => "empty",
            YieldStatus::Invalid => "invalid",
            YieldStatus::Valid => "valid",
        }
    }
}

/// Ligne par taille posée (`None` = taille de base).
#[derive(Debug, Clone, PartialEq)]
pub struct SizeYield {
    pub size: Option<SizeId>,
    pub pieces: usize,
    pub used_area_mm2: f64,
    /// Part du rendement du plan due à cette taille, en pourcent.
    pub share_percent: f64,
}

/// Chiffres d'un plan **valide**.
#[derive(Debug, Clone, PartialEq)]
pub struct YieldFigures {
    pub useful_width_mm: f64,
    pub length_mm: f64,
    pub used_area_mm2: f64,
    pub yield_percent: f64,
    pub by_size: Vec<SizeYield>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlanYield {
    pub status: YieldStatus,
    /// `Some` seulement si `status == Valid`.
    pub figures: Option<YieldFigures>,
}

/// Métrage d'un plan d'après ses diagnostics (lecture pure).
pub fn plan_yield(project: &Project, plan: &CuttingPlan, analysis: &PlanAnalysis) -> PlanYield {
    if plan.placements.is_empty() {
        return PlanYield {
            status: YieldStatus::Empty,
            figures: None,
        };
    }
    if !analysis.is_valid() || analysis.outlines.is_empty() {
        return PlanYield {
            status: YieldStatus::Invalid,
            figures: None,
        };
    }
    let useful_width_mm = plan.fabric_width_mm - 2.0 * plan.selvedge_mm;
    let length_mm = analysis
        .outlines
        .iter()
        .map(|o| o.bbox.max.y)
        .fold(0.0_f64, f64::max);
    let used_area_mm2: f64 = analysis.outlines.iter().map(|o| o.area_mm2).sum();
    let reference = useful_width_mm * length_mm;
    if reference <= 0.0 || !reference.is_finite() {
        return PlanYield {
            status: YieldStatus::Invalid,
            figures: None,
        };
    }
    let rank = |s: Option<SizeId>| -> (usize, usize) {
        s.and_then(|id| {
            let (range, i) = project.find_size(id)?;
            let r = project.sizes.iter().position(|x| x.id == range.id)?;
            Some((r + 1, i))
        })
        .unwrap_or((0, 0))
    };
    let mut sizes: Vec<Option<SizeId>> = Vec::new();
    for o in &analysis.outlines {
        if !sizes.contains(&o.size) {
            sizes.push(o.size);
        }
    }
    sizes.sort_by_key(|s| rank(*s));
    let by_size = sizes
        .into_iter()
        .map(|size| {
            let mine = analysis.outlines.iter().filter(|o| o.size == size);
            let area: f64 = mine.clone().map(|o| o.area_mm2).sum();
            SizeYield {
                size,
                pieces: mine.count(),
                used_area_mm2: area,
                share_percent: 100.0 * area / reference,
            }
        })
        .collect();
    PlanYield {
        status: YieldStatus::Valid,
        figures: Some(YieldFigures {
            useful_width_mm,
            length_mm,
            used_area_mm2,
            yield_percent: 100.0 * used_area_mm2 / reference,
            by_size,
        }),
    }
}

impl Project {
    /// Métrage d'un plan de coupe (diagnostics recalculés).
    pub fn cutting_plan_yield(&self, plan: CuttingPlanId) -> Result<PlanYield, CoreError> {
        let analysis = self.analyze_cutting_plan(plan)?;
        Ok(plan_yield(self, self.cutting_plan(plan)?, &analysis))
    }
}
