//! US-15.5 — cuir : **commandes** de la peau d'un plan de coupe (contour, défauts, zones de
//! qualité) et lectures de saisie (polygone d'une pièce ou d'une polyligne fermée).
//!
//! Chaque commande est un pas d'annulation ; tout refus est sans trace et nomme la cause (la
//! validation de `hide.rs` décide de ce qu'une peau peut porter). Refus propres aux commandes :
//! - pas de peau : ajouter un défaut ou une zone, ou retirer la peau, exige un contour d'abord ;
//! - un défaut qui en recoupe un autre (aire d'intersection > [`OVERLAP_MIN_MM2`]) : fusionnez-les ;
//! - un nouveau contour qui ne contient plus un défaut existant (message de la validation).
//!
//! Une note de défaut blanche vaut « sans note ». Les ids de défaut et de zone sont du compteur
//! du projet.

use crate::command::Outcome;
use crate::contour::{contour_from_entities, contour_polyline};
use crate::cutting_layout::{intersection_area_mm2, OVERLAP_MIN_MM2};
use crate::error::CoreError;
use crate::hide::{DefectKind, Hide, HideDefect, QualityZone};
use crate::hide_layout::ccw;
use crate::model::*;
use crate::PlanePoint;

fn bad(msg: String) -> CoreError {
    CoreError::Invalid(msg)
}

/// Points sans `-0.0`.
fn clean(points: &[PlanePoint]) -> Vec<PlanePoint> {
    points
        .iter()
        .map(|p| PlanePoint {
            x: p.x + 0.0,
            y: p.y + 0.0,
        })
        .collect()
}

fn clean_note(note: &Option<String>) -> Option<String> {
    note.as_ref()
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty())
}

impl Project {
    fn plan_hide_mut(
        &mut self,
        plan: CuttingPlanId,
    ) -> Result<(&str, &mut Option<Hide>), CoreError> {
        let p = self
            .cutting_plans
            .iter_mut()
            .find(|p| p.id == plan)
            .ok_or_else(|| bad(format!("plan de coupe introuvable (id {})", plan.0)))?;
        Ok((p.name.as_str(), &mut p.hide))
    }

    /// Peau du plan, ou refus nommé (« définissez d'abord le contour »).
    fn existing_hide(&self, plan: CuttingPlanId) -> Result<Hide, CoreError> {
        let p = self.cutting_plan(plan)?;
        p.hide.clone().ok_or_else(|| {
            bad(format!(
                "plan de coupe « {} » : pas de peau, définissez d'abord son contour",
                p.name
            ))
        })
    }

    fn store_hide(&mut self, plan: CuttingPlanId, hide: Hide) -> Result<(), CoreError> {
        let (name, slot) = self.plan_hide_mut(plan)?;
        let what = format!("plan de coupe « {name} »");
        hide.validate(&what)?;
        *slot = Some(hide);
        Ok(())
    }

    /// `SetHideContour` : définit ou remplace le contour de la peau (défauts et zones conservés ;
    /// refus si un défaut ne tient plus dans le nouveau contour).
    pub(crate) fn set_hide_contour(
        &mut self,
        plan: CuttingPlanId,
        contour: &[PlanePoint],
    ) -> Result<(), CoreError> {
        let mut hide = self.cutting_plan(plan)?.hide.clone().unwrap_or(Hide {
            contour: Vec::new(),
            defects: Vec::new(),
            quality_zones: Vec::new(),
        });
        hide.contour = clean(contour);
        self.store_hide(plan, hide)
    }

    /// `RemoveHide` : retire la peau avec ses défauts et ses zones.
    pub(crate) fn remove_hide(&mut self, plan: CuttingPlanId) -> Result<(), CoreError> {
        self.existing_hide(plan)?;
        let (_, slot) = self.plan_hide_mut(plan)?;
        *slot = None;
        Ok(())
    }

    fn check_defect_overlap(hide: &Hide, candidate: &HideDefect) -> Result<(), CoreError> {
        let mine = ccw(&candidate.polygon);
        for other in hide.defects.iter().filter(|d| d.id != candidate.id) {
            if intersection_area_mm2(&mine, &ccw(&other.polygon)) > OVERLAP_MIN_MM2 {
                return Err(bad(format!(
                    "le défaut recoupe le défaut {} : fusionnez-les en un seul polygone",
                    other.id.0
                )));
            }
        }
        Ok(())
    }

    /// `AddHideDefect`.
    pub(crate) fn add_hide_defect(
        &mut self,
        plan: CuttingPlanId,
        kind: DefectKind,
        polygon: &[PlanePoint],
        note: &Option<String>,
        out: &mut Outcome,
    ) -> Result<(), CoreError> {
        let mut hide = self.existing_hide(plan)?;
        let id = HideDefectId(self.alloc_id());
        let d = HideDefect {
            id,
            kind,
            polygon: clean(polygon),
            note: clean_note(note),
        };
        let what = format!(
            "plan de coupe « {} », défaut",
            self.cutting_plan(plan)?.name
        );
        Self::check_defect_overlap(&hide, &d).map_err(|e| bad(format!("{what} : {e}")))?;
        hide.defects.push(d);
        self.store_hide(plan, hide)?;
        out.hide_defects.push(id);
        Ok(())
    }

    /// `UpdateHideDefect` : genre, polygone et note (l'id ne change pas).
    pub(crate) fn update_hide_defect(
        &mut self,
        plan: CuttingPlanId,
        defect: HideDefectId,
        kind: DefectKind,
        polygon: &[PlanePoint],
        note: &Option<String>,
    ) -> Result<(), CoreError> {
        let mut hide = self.existing_hide(plan)?;
        let new = HideDefect {
            id: defect,
            kind,
            polygon: clean(polygon),
            note: clean_note(note),
        };
        let what = format!(
            "plan de coupe « {} », défaut",
            self.cutting_plan(plan)?.name
        );
        let slot = hide
            .defects
            .iter_mut()
            .find(|d| d.id == defect)
            .ok_or_else(|| bad(format!("{what} {} introuvable", defect.0)))?;
        *slot = new.clone();
        Self::check_defect_overlap(&hide, &new).map_err(|e| bad(format!("{what} : {e}")))?;
        self.store_hide(plan, hide)
    }

    /// `RemoveHideDefect`.
    pub(crate) fn remove_hide_defect(
        &mut self,
        plan: CuttingPlanId,
        defect: HideDefectId,
    ) -> Result<(), CoreError> {
        let mut hide = self.existing_hide(plan)?;
        let before = hide.defects.len();
        hide.defects.retain(|d| d.id != defect);
        if hide.defects.len() == before {
            return Err(bad(format!(
                "plan de coupe « {} » : défaut {} introuvable",
                self.cutting_plan(plan)?.name,
                defect.0
            )));
        }
        self.store_hide(plan, hide)
    }

    /// `AddQualityZone`.
    pub(crate) fn add_quality_zone(
        &mut self,
        plan: CuttingPlanId,
        grade: u8,
        polygon: &[PlanePoint],
        out: &mut Outcome,
    ) -> Result<(), CoreError> {
        let mut hide = self.existing_hide(plan)?;
        let id = QualityZoneId(self.alloc_id());
        hide.quality_zones.push(QualityZone {
            id,
            grade,
            polygon: clean(polygon),
        });
        self.store_hide(plan, hide)?;
        out.quality_zones.push(id);
        Ok(())
    }

    /// `UpdateQualityZone` : grade et polygone.
    pub(crate) fn update_quality_zone(
        &mut self,
        plan: CuttingPlanId,
        zone: QualityZoneId,
        grade: u8,
        polygon: &[PlanePoint],
    ) -> Result<(), CoreError> {
        let mut hide = self.existing_hide(plan)?;
        let name = self.cutting_plan(plan)?.name.clone();
        let slot = hide
            .quality_zones
            .iter_mut()
            .find(|z| z.id == zone)
            .ok_or_else(|| {
                bad(format!(
                    "plan de coupe « {name} » : zone de qualité {} introuvable",
                    zone.0
                ))
            })?;
        slot.grade = grade;
        slot.polygon = clean(polygon);
        self.store_hide(plan, hide)
    }

    /// `RemoveQualityZone`.
    pub(crate) fn remove_quality_zone(
        &mut self,
        plan: CuttingPlanId,
        zone: QualityZoneId,
    ) -> Result<(), CoreError> {
        let mut hide = self.existing_hide(plan)?;
        let before = hide.quality_zones.len();
        hide.quality_zones.retain(|z| z.id != zone);
        if hide.quality_zones.len() == before {
            return Err(bad(format!(
                "plan de coupe « {} » : zone de qualité {} introuvable",
                self.cutting_plan(plan)?.name,
                zone.0
            )));
        }
        self.store_hide(plan, hide)
    }

    /// Polygone d'une polyligne fermée de la feuille (tracés `entities` formant exactement un
    /// contour fermé ; courbes échantillonnées). Lecture seule, pour la saisie de la peau.
    pub fn sheet_closed_polygon(
        &self,
        sheet: SheetId,
        entities: &[EntityId],
    ) -> Result<Vec<PlanePoint>, CoreError> {
        let sh = self.sheet(sheet)?;
        let contour = contour_from_entities(sh, entities)?;
        let pts = contour_polyline(sh, &contour.edges, 16)?;
        Ok(pts
            .into_iter()
            .map(|p| PlanePoint { x: p.x, y: p.y })
            .collect())
    }

    /// Contour de coupe d'une pièce (taille de base) comme polygone, dans son repère local (coin
    /// bas-gauche de la boîte à l'origine). Lecture seule, pour la saisie de la peau.
    pub fn piece_polygon(&self, piece: PieceId) -> Result<Vec<PlanePoint>, CoreError> {
        let shape = self.piece_shape(piece, None)?;
        Ok(shape
            .polygon
            .into_iter()
            .map(|p| PlanePoint { x: p.x, y: p.y })
            .collect())
    }
}
