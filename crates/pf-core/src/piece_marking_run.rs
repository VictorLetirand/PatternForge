//! Perçages au pas le long d'un bord (PH-4b).
//!
//! Une **rangée** de perçages (ronds ou fentes de poinçon) répartis à intervalles égaux sur la
//! **longueur développée** d'un bord de la pièce (un tracé, ou une suite continue de tracés du
//! contour, dans l'ordre du contour), à une distance donnée du bord mesurée par la **normale**,
//! côté intérieur ou extérieur. Positions **calculées à la création** : la rangée n'est pas une
//! entité, chaque perçage créé est un [`PieceMarking`](crate::PieceMarking) ordinaire (même
//! identifiant stable, mêmes exports) et reste indépendant des autres.
//!
//! Ancrage : un point **libre** par perçage, rangé dans le calque masqué
//! [`MARKING_RUN_POINTS_LAYER_NAME`] (aucun point visible) ; un point déjà présent au même endroit
//! est réutilisé. Les positions sont donc figées : déplacer un point du contour ne les déplace
//! pas. Tout est en mm.

use crate::command::{Command, Outcome};
use crate::error::CoreError;
use crate::model::*;
use crate::piece_marking::{MarkingAnchor, MarkingShape, MARKING_MAX_PER_PIECE};
use crate::seam_pair::SeamSide;
use pf_geometry::PointMm;
use serde::{Deserialize, Serialize};

/// Nombre maximal de perçages d'une rangée.
pub const MARKING_RUN_MAX_COUNT: usize = 500;
/// Plus petit pas demandé (mm).
pub const MARKING_RUN_MIN_STEP_MM: f64 = 0.5;
/// Plus grand pas demandé (mm).
pub const MARKING_RUN_MAX_STEP_MM: f64 = 1000.0;
/// Plus grande distance au bord (mm).
pub const MARKING_RUN_MAX_DISTANCE_MM: f64 = 100.0;
/// Plus petit espacement réel entre deux perçages (mm) : en dessous, ils se confondraient.
pub const MARKING_RUN_MIN_SPACING_MM: f64 = 0.001;
/// Nom du calque (masqué) qui reçoit les points d'ancrage des perçages au pas.
pub const MARKING_RUN_POINTS_LAYER_NAME: &str = "Points de perçages";

/// Répartition : par pas demandé ou par nombre de perçages.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum RunSpacing {
    /// Pas demandé (0,5 à 1000 mm) : ⌈L / pas⌉ intervalles **égaux**, le dernier perçage tombe
    /// exactement à la fin ; l'espacement réel est donc inférieur ou égal au pas demandé.
    Step { step_mm: f64 },
    /// Nombre de perçages (1 à 500). Un seul perçage : au milieu de l'intervalle utile.
    Count { count: u32 },
}

/// Côté du bord où se posent les perçages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunSide {
    /// Côté intérieur de la pièce (défaut de l'interface).
    Inside,
    Outside,
}

/// Forme des perçages d'une rangée.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum RunForm {
    /// Perçage rond.
    Drill { diameter_mm: f64 },
    /// Fente de poinçon orientée **le long du bord** (tangente locale, dans le sens du contour).
    Slot { length_mm: f64, width_mm: f64 },
}

impl RunForm {
    /// Forme du marquage d'un perçage dont la tangente locale fait `tangent_deg`.
    pub fn shape(self, tangent_deg: f64) -> MarkingShape {
        match self {
            RunForm::Drill { diameter_mm } => MarkingShape::Drill { diameter_mm },
            RunForm::Slot {
                length_mm,
                width_mm,
            } => MarkingShape::Slot {
                length_mm,
                width_mm,
                angle_deg: tangent_deg,
            },
        }
    }
}

/// Paramètres d'une rangée de perçages au pas.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarkingRunSpec {
    /// Tracés consécutifs du contour de la pièce, dans l'ordre du contour. L'abscisse 0 est le
    /// début du premier tracé, dans le sens du contour.
    pub edges: Vec<EntityId>,
    pub spacing: RunSpacing,
    /// Abscisse du premier perçage (mm depuis le début du bord) ; ≥ 0.
    #[serde(default)]
    pub start_margin_mm: f64,
    /// Marge de fin (mm) : le dernier perçage est à `longueur − marge` ; ≥ 0.
    #[serde(default)]
    pub end_margin_mm: f64,
    /// Distance du centre de chaque perçage au bord, mesurée par la normale (0 à 100 mm).
    pub distance_mm: f64,
    pub side: RunSide,
    pub form: RunForm,
}

/// Un perçage calculé (lecture ; rien n'est modifié).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RunHole {
    /// Abscisse curviligne depuis le début du bord (mm).
    pub abscissa_mm: f64,
    /// Centre du perçage (mm).
    pub position: PointMm,
    /// Direction du bord en ce point (sens du contour), degrés depuis +X dans [0 ; 360[.
    pub tangent_deg: f64,
    /// Tracé du contour qui porte l'abscisse.
    pub edge: EntityId,
}

/// Rangée calculée : aperçu et base de la création.
#[derive(Debug, Clone, PartialEq)]
pub struct MarkingRunLayout {
    /// Longueur développée du bord (mm).
    pub length_mm: f64,
    /// Espacement **réel** entre deux perçages consécutifs (mm) ; 0 pour un seul perçage.
    pub step_mm: f64,
    pub holes: Vec<RunHole>,
}

fn bad(msg: impl Into<String>) -> CoreError {
    CoreError::Invalid(msg.into())
}

fn finite_in(v: f64, min: f64, max: f64) -> bool {
    v.is_finite() && (min..=max).contains(&v)
}

/// Nombre d'intervalles pour couvrir `span` avec des pas d'au plus `step` : ⌈span / step⌉, sans
/// qu'une erreur d'arrondi (0,3 / 0,1) ajoute un intervalle.
fn intervals(span: f64, step: f64) -> f64 {
    let q = span / step;
    let r = q.round();
    if (q - r).abs() <= 1e-9 * q.abs().max(1.0) {
        r
    } else {
        q.ceil()
    }
}

impl Project {
    /// Tracés du contour de la pièce à partir d'une sélection : ceux de `selection` qui forment
    /// une suite continue, dans l'ordre du contour (le tour de la fermeture est permis). Refus :
    /// sélection vide, tracé hors du contour, suite non continue.
    pub fn marking_run_edges(
        &self,
        piece: PieceId,
        selection: &[EntityId],
    ) -> Result<Vec<EntityId>, CoreError> {
        let p = self.piece(piece)?;
        let n = p.outline.len();
        let chosen: Vec<bool> = p
            .outline
            .iter()
            .map(|e| selection.contains(&e.entity))
            .collect();
        let count = chosen.iter().filter(|&&c| c).count();
        if count == 0
            || selection
                .iter()
                .any(|s| !p.outline.iter().any(|e| e.entity == *s))
        {
            return Err(bad(
                "le bord doit être un ou plusieurs tracés du contour de la pièce",
            ));
        }
        let start = if count == n {
            0
        } else {
            let starts: Vec<usize> = (0..n)
                .filter(|&i| chosen[i] && !chosen[(i + n - 1) % n])
                .collect();
            match starts.as_slice() {
                [i] => *i,
                _ => return Err(bad("les tracés choisis ne forment pas une suite continue")),
            }
        };
        Ok((0..count)
            .map(|k| p.outline[(start + k) % n].entity)
            .collect())
    }

    /// Calcul de la rangée (lecture pure) : refus expliqués, aucune trace. Voir
    /// [`MarkingRunSpec`] pour les conventions.
    pub(crate) fn marking_run_layout(
        &self,
        piece: PieceId,
        spec: &MarkingRunSpec,
    ) -> Result<MarkingRunLayout, CoreError> {
        let p = self.piece(piece)?;
        if !finite_in(spec.distance_mm, 0.0, MARKING_RUN_MAX_DISTANCE_MM) {
            return Err(bad(format!(
                "la distance au bord doit être un nombre fini compris entre 0 et \
                 {MARKING_RUN_MAX_DISTANCE_MM} mm"
            )));
        }
        for (v, what) in [
            (spec.start_margin_mm, "la marge de départ"),
            (spec.end_margin_mm, "la marge de fin"),
        ] {
            if !v.is_finite() || v < 0.0 {
                return Err(bad(format!(
                    "{what} doit être un nombre fini, au moins 0 mm"
                )));
            }
        }
        spec.form.shape(0.0).validate()?;
        match spec.spacing {
            RunSpacing::Step { step_mm } => {
                if !finite_in(step_mm, MARKING_RUN_MIN_STEP_MM, MARKING_RUN_MAX_STEP_MM) {
                    return Err(bad(format!(
                        "le pas doit être un nombre fini compris entre {MARKING_RUN_MIN_STEP_MM} \
                         et {MARKING_RUN_MAX_STEP_MM} mm"
                    )));
                }
            }
            RunSpacing::Count { count } => {
                if !(1..=MARKING_RUN_MAX_COUNT as u32).contains(&count) {
                    return Err(bad(format!(
                        "le nombre de perçages doit être compris entre 1 et \
                         {MARKING_RUN_MAX_COUNT}"
                    )));
                }
            }
        }
        let side = SeamSide {
            piece,
            edges: spec.edges.clone(),
        };
        side.check(self, "perçages au pas : bord")?;
        let frame = self.march_frame(&side, "perçages au pas : bord")?;
        let total = frame.total;
        let (d0, f) = (spec.start_margin_mm, spec.end_margin_mm);
        let span = total - d0 - f;
        let count = match spec.spacing {
            RunSpacing::Step { step_mm } => {
                if span.is_nan() || span <= MARKING_RUN_MIN_SPACING_MM {
                    return Err(bad(format!(
                        "les marges ({d0} + {f} mm) ne laissent pas de place sur un bord de \
                         {total:.3} mm"
                    )));
                }
                let n = intervals(span, step_mm) + 1.0;
                if n > MARKING_RUN_MAX_COUNT as f64 {
                    return Err(bad(format!(
                        "le pas de {step_mm} mm donnerait plus de {MARKING_RUN_MAX_COUNT} \
                         perçages sur ce bord"
                    )));
                }
                n as usize
            }
            RunSpacing::Count { count } => {
                if span.is_nan() || span < 0.0 {
                    return Err(bad(format!(
                        "les marges ({d0} + {f} mm) dépassent la longueur du bord ({total:.3} mm)"
                    )));
                }
                count as usize
            }
        };
        let step = if count > 1 {
            span / (count - 1) as f64
        } else {
            0.0
        };
        if count > 1 && step < MARKING_RUN_MIN_SPACING_MM {
            return Err(bad(format!(
                "l'espacement des perçages ({step:.6} mm) est trop petit : ils se confondraient"
            )));
        }
        if frame.whole && count > 1 && d0 + f < MARKING_RUN_MIN_SPACING_MM {
            return Err(bad(
                "contour entier : le dernier perçage tomberait sur le premier ; indiquez une \
                 marge de départ ou de fin non nulle",
            ));
        }
        if p.markings.len() + count > MARKING_MAX_PER_PIECE {
            return Err(bad(format!(
                "la pièce « {} » porte déjà {} marquages : {count} de plus dépasseraient la \
                 limite de {MARKING_MAX_PER_PIECE}",
                p.name,
                p.markings.len()
            )));
        }
        let sheet = self.sheet(frame.sheet)?;
        let mut holes = Vec::with_capacity(count);
        for i in 0..count {
            let s = if count == 1 {
                d0 + span / 2.0
            } else if i + 1 == count {
                total - f
            } else {
                d0 + i as f64 * step
            }
            .clamp(0.0, total);
            let k = frame
                .edges
                .iter()
                .position(|e| s <= e.3 + e.2 + 1e-12)
                .unwrap_or(frame.edges.len() - 1);
            let (entity, rev, len, start) = frame.edges[k];
            let along = (s - start).clamp(0.0, len);
            let (at, tangent) =
                sheet.path_point_at(entity, if rev { len - along } else { along })?;
            // Sens du contour ; l'intérieur est à gauche d'un contour anti-horaire.
            let dir = if rev { tangent * -1.0 } else { tangent };
            let inward = if frame.clockwise {
                dir.rotated_left() * -1.0
            } else {
                dir.rotated_left()
            };
            let sign = match spec.side {
                RunSide::Inside => 1.0,
                RunSide::Outside => -1.0,
            };
            let position = PointMm::new(
                at.x + inward.x * spec.distance_mm * sign,
                at.y + inward.y * spec.distance_mm * sign,
            );
            position.validate()?;
            let mut deg = dir.y.atan2(dir.x).to_degrees();
            if deg < 0.0 {
                deg += 360.0;
            }
            if !(0.0..360.0).contains(&deg) {
                deg = 0.0;
            }
            holes.push(RunHole {
                abscissa_mm: s,
                position,
                tangent_deg: deg,
                edge: entity,
            });
        }
        Ok(MarkingRunLayout {
            length_mm: total,
            step_mm: step,
            holes,
        })
    }

    /// Aperçu en **lecture seule** : positions et espacement réel, avec **les mêmes refus** que
    /// [`Command::AddPieceMarkingRun`] (la commande est rejouée sur une copie du projet).
    pub fn marking_run_preview(
        &self,
        piece: PieceId,
        spec: &MarkingRunSpec,
    ) -> Result<MarkingRunLayout, CoreError> {
        let layout = self.marking_run_layout(piece, spec)?;
        self.clone().apply(Command::AddPieceMarkingRun {
            piece,
            run: spec.clone(),
        })?;
        Ok(layout)
    }

    /// Crée la rangée : un point d'ancrage (calque masqué) et un marquage par perçage. Les
    /// identifiants des marquages sont dans `out.markings`.
    pub(crate) fn add_piece_marking_run(
        &mut self,
        piece: PieceId,
        spec: &MarkingRunSpec,
        out: &mut Outcome,
    ) -> Result<(), CoreError> {
        let layout = self.marking_run_layout(piece, spec)?;
        let sheet = self.piece_sheet(piece)?.id;
        let layer = self.hidden_layer(sheet, MARKING_RUN_POINTS_LAYER_NAME, out)?;
        let before = out.points.len();
        for h in &layout.holes {
            let point = self.tangency_point(sheet, h.position, Some(layer), out)?;
            self.add_piece_marking(
                piece,
                MarkingAnchor::Point { point },
                spec.form.shape(h.tangent_deg),
                out,
            )?;
        }
        let name = self.sheet(sheet)?.layer_name(Some(layer));
        let state = if self.sheet(sheet)?.layer_visible(Some(layer)) {
            "visible"
        } else {
            "masqué"
        };
        out.notes.push(format!(
            "{} perçage(s) posé(s), espacement réel {:.4} mm ; {} point(s) d'ancrage dans le \
             calque {state} « {name} ».",
            layout.holes.len(),
            layout.step_mm,
            out.points.len() - before
        ));
        Ok(())
    }
}
