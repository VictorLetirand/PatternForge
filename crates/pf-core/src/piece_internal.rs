//! Éléments internes d'une pièce (US-04.3) : lignes (pli, surpiqûre, position de
//! poche) et repères (marque, boutonnière) **liés** à la construction. Rien n'est
//! copié : la géométrie est résolue à la demande, donc une pièce suit la
//! construction. Les types sont à faire relire par une modéliste.

use crate::command::Outcome;
use crate::error::CoreError;
use crate::model::*;
use pf_geometry::{Lasso, PointMm};
use std::collections::HashSet;

/// Nombre d'échantillons par tracé courbe pour juger de l'appartenance au contour.
const INSIDE_SAMPLES: usize = 64;

/// Géométrie résolue d'un élément interne, en mm.
#[derive(Debug, Clone, PartialEq)]
pub enum InternalGeometry {
    /// Ligne : longueur développée du tracé.
    Path { length_mm: f64 },
    /// Repère : position du point, ou du pied du cran sur son tracé.
    Position { at: PointMm },
    /// La géométrie ne peut pas être calculée (tracé dégénéré).
    Unreadable,
}

/// Un élément interne et ce qu'il désigne aujourd'hui dans la construction.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedInternal {
    pub element: InternalElement,
    /// Nom du tracé, étiquette du point, ou nom du tracé qui porte le cran.
    pub name: String,
    pub geometry: InternalGeometry,
    /// `Some(false)` : l'élément n'est plus entièrement dans le contour ;
    /// `None` : pas de jugement possible (contour absent, ouvert ou illisible).
    pub inside_outline: Option<bool>,
}

/// Pourquoi un objet n'est pas proposé à l'import.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    /// Pas entièrement à l'intérieur du contour.
    OutsideOutline,
    /// Tracé du contour de la pièce.
    OutlineEdge,
    /// Tracé du contour d'une autre pièce de la feuille.
    OtherPieceOutline,
    /// Rôle « ligne de couture » ou « ligne de coupe ».
    ContourRole,
    /// Droit-fil.
    Grainline,
    /// Cran posé sur un tracé du contour.
    OutlineNotch,
    /// Géométrie illisible.
    Unreadable,
}

impl SkipReason {
    /// Identifiant stable, indépendant de la langue.
    pub fn key(self) -> &'static str {
        match self {
            SkipReason::OutsideOutline => "outside_outline",
            SkipReason::OutlineEdge => "outline_edge",
            SkipReason::OtherPieceOutline => "other_piece_outline",
            SkipReason::ContourRole => "contour_role",
            SkipReason::Grainline => "grainline",
            SkipReason::OutlineNotch => "outline_notch",
            SkipReason::Unreadable => "unreadable",
        }
    }
}

/// Rapport d'un import : ce qui est ajouté (ou le serait), ce qui est ignoré et pourquoi.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct InternalImportReport {
    pub added: Vec<InternalElement>,
    pub skipped: Vec<(InternalTarget, SkipReason)>,
    /// Objets déjà éléments internes de la pièce (jamais ajoutés deux fois).
    pub already: usize,
}

/// Nombre d'éléments internes par type (fiche de pièce).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct InternalCounts {
    pub fold: usize,
    pub topstitch: usize,
    pub pocket_position: usize,
    /// Lignes de type « autre ».
    pub other_lines: usize,
    pub mark: usize,
    pub buttonhole: usize,
    /// Repères de type « autre ».
    pub other_marks: usize,
}

impl InternalCounts {
    pub fn lines(&self) -> usize {
        self.fold + self.topstitch + self.pocket_position + self.other_lines
    }
    pub fn marks(&self) -> usize {
        self.mark + self.buttonhole + self.other_marks
    }
    pub fn total(&self) -> usize {
        self.lines() + self.marks()
    }
}

/// Texte court désignant une cible dans un message.
pub(crate) fn target_text(target: InternalTarget) -> String {
    match target {
        InternalTarget::Path { entity } => format!("tracé {}", entity.0),
        InternalTarget::Point { point } => format!("point {}", point.0),
        InternalTarget::Notch { notch } => format!("cran {}", notch.0),
    }
}

fn sheet_has(s: &Sheet, target: InternalTarget) -> bool {
    match target {
        InternalTarget::Path { entity } => s.entity(entity).is_ok(),
        InternalTarget::Point { point } => s.point(point).is_ok(),
        InternalTarget::Notch { notch } => s.notch(notch).is_ok(),
    }
}

impl Project {
    /// La cible existe-t-elle sur la feuille de la pièce (toute feuille pour une
    /// pièce détachée, comme pour son contour) ?
    pub(crate) fn internal_target_exists(&self, link: PieceLink, target: InternalTarget) -> bool {
        match link {
            PieceLink::Linked { sheet } => self.sheet(sheet).is_ok_and(|s| sheet_has(s, target)),
            PieceLink::Detached => self.sheets.iter().any(|s| sheet_has(s, target)),
        }
    }

    /// Polygone du contour de la pièce (tracés échantillonnés dans l'ordre et le
    /// sens de la pièce). Refus expliqué si le contour est absent, ouvert ou dégénéré.
    pub(crate) fn outline_lasso(&self, piece: PieceId) -> Result<Lasso, CoreError> {
        let contour = self.piece_contour(piece)?;
        let name = &self.piece(piece)?.name;
        if !contour.closed {
            return Err(CoreError::Invalid(format!(
                "le contour de la pièce « {name} » n'est pas fermé"
            )));
        }
        let sheet = self.piece_sheet(piece)?;
        let mut polygon = Vec::new();
        for edge in &contour.edges {
            let mut pts = sheet.shape(edge.entity)?.sample(INSIDE_SAMPLES);
            if edge.reversed {
                pts.reverse();
            }
            polygon.extend(pts);
        }
        Lasso::new(&polygon).ok_or_else(|| {
            CoreError::Invalid(format!("le contour de la pièce « {name} » est dégénéré"))
        })
    }

    /// Éléments internes d'une pièce avec leur géométrie actuelle (lecture pure ;
    /// liste vide pour une pièce inconnue). Les cibles disparues sont impossibles
    /// dans un projet valide ; elles seraient rendues « illisibles ».
    pub fn piece_internals(&self, piece: PieceId) -> Vec<ResolvedInternal> {
        let Ok(p) = self.piece(piece) else {
            return Vec::new();
        };
        if p.internal.is_empty() {
            return Vec::new();
        }
        let sheets: Vec<&Sheet> = match self.piece_sheet(piece) {
            Ok(s) => vec![s],
            Err(_) => self.sheets.iter().collect(),
        };
        let lasso = self.outline_lasso(piece).ok();
        p.internal
            .iter()
            .map(|&element| {
                let found = sheets.iter().find(|s| sheet_has(s, element.target));
                let (name, geometry, inside) = match (found, element.target) {
                    (Some(s), InternalTarget::Path { entity }) => {
                        let geometry = s
                            .path_length_mm(entity)
                            .map_or(InternalGeometry::Unreadable, |length_mm| {
                                InternalGeometry::Path { length_mm }
                            });
                        let inside = lasso.as_ref().and_then(|l| {
                            s.shape(entity)
                                .ok()
                                .map(|sh| l.contains_polyline(&sh.sample(INSIDE_SAMPLES)))
                        });
                        (s.entity_name(entity), geometry, inside)
                    }
                    (Some(s), InternalTarget::Point { point }) => {
                        let at = s.position(point).ok();
                        (
                            s.label(point).to_string(),
                            at.map_or(InternalGeometry::Unreadable, |at| {
                                InternalGeometry::Position { at }
                            }),
                            at.and_then(|a| lasso.as_ref().map(|l| l.contains_point(a))),
                        )
                    }
                    (Some(s), InternalTarget::Notch { notch }) => {
                        let at = s.notch(notch).ok().and_then(|n| {
                            let d = s.notch_distance_mm(n).ok()?;
                            s.path_point_at(n.entity, d).ok().map(|r| r.0)
                        });
                        let host = s
                            .notch(notch)
                            .map(|n| s.entity_name(n.entity))
                            .unwrap_or_default();
                        (
                            host,
                            at.map_or(InternalGeometry::Unreadable, |at| {
                                InternalGeometry::Position { at }
                            }),
                            at.and_then(|a| lasso.as_ref().map(|l| l.contains_point(a))),
                        )
                    }
                    (None, _) => ("?".to_string(), InternalGeometry::Unreadable, None),
                };
                ResolvedInternal {
                    element,
                    name,
                    geometry,
                    inside_outline: inside,
                }
            })
            .collect()
    }

    /// Nombre d'éléments internes d'une pièce par type.
    pub fn piece_internal_counts(&self, piece: PieceId) -> InternalCounts {
        let mut c = InternalCounts::default();
        let Ok(p) = self.piece(piece) else {
            return c;
        };
        for el in &p.internal {
            let is_line = matches!(el.target, InternalTarget::Path { .. });
            match (el.kind, is_line) {
                (InternalKind::Fold, _) => c.fold += 1,
                (InternalKind::Topstitch, _) => c.topstitch += 1,
                (InternalKind::PocketPosition, _) => c.pocket_position += 1,
                (InternalKind::Mark, _) => c.mark += 1,
                (InternalKind::Buttonhole, _) => c.buttonhole += 1,
                (InternalKind::Other, true) => c.other_lines += 1,
                (InternalKind::Other, false) => c.other_marks += 1,
            }
        }
        c
    }

    /// Ce que « Importer les éléments internes » ajouterait à la pièce (lecture
    /// pure). Sont proposés les tracés, points libres (qui ne servent d'extrémité à
    /// aucun tracé) et crans de la feuille **entièrement dans le contour**, hors
    /// contour de la pièce, droit-fil, rôles couture/coupe et contours des autres
    /// pièces. Type par défaut d'une ligne : voir [`InternalKind::default_for_role`] ;
    /// repères : « marque ». Refus expliqué si la pièce n'a pas de contour fermé.
    /// Limite : un objet dont une extrémité est sur le contour au bruit flottant près
    /// peut être jugé « hors du contour » (voir DECISIONS 110).
    pub fn piece_internal_import(&self, piece: PieceId) -> Result<InternalImportReport, CoreError> {
        let p = self.piece(piece)?;
        let lasso = self.outline_lasso(piece)?;
        let sheet = self.piece_sheet(piece)?;
        let existing: HashSet<InternalTarget> = p.internal.iter().map(|e| e.target).collect();
        let mut other_outlines = HashSet::new();
        for o in self.pieces_on(sheet.id) {
            if o.id != p.id {
                other_outlines.extend(o.outline.iter().map(|e| e.entity));
            }
        }
        let mut added = Vec::new();
        let mut skipped = Vec::new();
        let mut already = 0;
        for e in &sheet.entities {
            let target = InternalTarget::Path { entity: e.id };
            if existing.contains(&target) {
                already += 1;
                continue;
            }
            let reason = if p.outline.iter().any(|x| x.entity == e.id) {
                Some(SkipReason::OutlineEdge)
            } else if e.role == PathRole::Grainline || p.grainline == Some(e.id) {
                Some(SkipReason::Grainline)
            } else if matches!(e.role, PathRole::SewingContour | PathRole::CuttingContour) {
                Some(SkipReason::ContourRole)
            } else if other_outlines.contains(&e.id) {
                Some(SkipReason::OtherPieceOutline)
            } else {
                match sheet.shape(e.id) {
                    Err(_) => Some(SkipReason::Unreadable),
                    Ok(sh) if lasso.contains_polyline(&sh.sample(INSIDE_SAMPLES)) => None,
                    Ok(_) => Some(SkipReason::OutsideOutline),
                }
            };
            match reason {
                None => added.push(InternalElement {
                    target,
                    kind: InternalKind::default_for_role(e.role),
                }),
                Some(r) => skipped.push((target, r)),
            }
        }
        for pt in &sheet.points {
            let target = InternalTarget::Point { point: pt.id };
            // Une extrémité de tracé n'est pas un repère : seuls les points libres.
            if sheet.point_is_used(pt.id) {
                continue;
            }
            if existing.contains(&target) {
                already += 1;
                continue;
            }
            match sheet.position(pt.id) {
                Err(_) => skipped.push((target, SkipReason::Unreadable)),
                Ok(at) if lasso.contains_point(at) => added.push(InternalElement {
                    target,
                    kind: InternalKind::Mark,
                }),
                Ok(_) => skipped.push((target, SkipReason::OutsideOutline)),
            }
        }
        for n in &sheet.notches {
            let target = InternalTarget::Notch { notch: n.id };
            if existing.contains(&target) {
                already += 1;
                continue;
            }
            if p.outline.iter().any(|x| x.entity == n.entity) {
                skipped.push((target, SkipReason::OutlineNotch));
                continue;
            }
            let at = sheet
                .notch_distance_mm(n)
                .and_then(|d| sheet.path_point_at(n.entity, d));
            match at {
                Err(_) => skipped.push((target, SkipReason::Unreadable)),
                Ok((at, _)) if lasso.contains_point(at) => added.push(InternalElement {
                    target,
                    kind: InternalKind::Mark,
                }),
                Ok(_) => skipped.push((target, SkipReason::OutsideOutline)),
            }
        }
        Ok(InternalImportReport {
            added,
            skipped,
            already,
        })
    }

    /// Ajoute un élément interne (refus expliqué : cible absente, d'une autre
    /// feuille, type incohérent, doublon, tracé du contour ou droit-fil).
    pub(crate) fn add_piece_internal(
        &mut self,
        piece: PieceId,
        target: InternalTarget,
        kind: InternalKind,
    ) -> Result<(), CoreError> {
        let p = self.piece(piece)?;
        let sheet = self.piece_sheet(piece).map_err(|_| {
            CoreError::Invalid(format!(
                "la pièce « {} » n'a pas de contour : on ne sait pas sur quelle feuille chercher",
                p.name
            ))
        })?;
        if !sheet_has(sheet, target) {
            let elsewhere = self.sheets.iter().any(|s| sheet_has(s, target));
            return Err(if elsewhere {
                CoreError::Invalid(format!(
                    "le {} n'est pas sur la feuille « {} » de la pièce « {} »",
                    target_text(target),
                    sheet.name,
                    p.name
                ))
            } else {
                match target {
                    InternalTarget::Path { entity } => CoreError::UnknownEntity(entity),
                    InternalTarget::Point { point } => CoreError::UnknownPoint(point),
                    InternalTarget::Notch { notch } => CoreError::UnknownNotch(notch),
                }
            });
        }
        if !kind.fits(target) {
            return Err(CoreError::Invalid(format!(
                "le type « {} » ne convient pas à ce {} (lignes : pli, surpiqûre, position de poche, \
                 autre ; repères : marque, boutonnière, autre)",
                kind.key(),
                target_text(target)
            )));
        }
        if p.internal.iter().any(|e| e.target == target) {
            return Err(CoreError::Invalid(format!(
                "le {} est déjà un élément interne de la pièce « {} »",
                target_text(target),
                p.name
            )));
        }
        if let InternalTarget::Path { entity } = target {
            if p.outline.iter().any(|e| e.entity == entity) || p.grainline == Some(entity) {
                return Err(CoreError::Invalid(format!(
                    "le {} fait partie du contour ou du droit-fil de la pièce « {} » : il ne peut \
                     pas être aussi un élément interne",
                    target_text(target),
                    p.name
                )));
            }
        }
        self.piece_mut(piece)?
            .internal
            .push(InternalElement { target, kind });
        Ok(())
    }

    fn internal_index(&self, piece: PieceId, target: InternalTarget) -> Result<usize, CoreError> {
        let p = self.piece(piece)?;
        p.internal
            .iter()
            .position(|e| e.target == target)
            .ok_or_else(|| {
                CoreError::Invalid(format!(
                    "le {} n'est pas un élément interne de la pièce « {} »",
                    target_text(target),
                    p.name
                ))
            })
    }

    pub(crate) fn remove_piece_internal(
        &mut self,
        piece: PieceId,
        target: InternalTarget,
    ) -> Result<(), CoreError> {
        let i = self.internal_index(piece, target)?;
        self.piece_mut(piece)?.internal.remove(i);
        Ok(())
    }

    pub(crate) fn set_piece_internal_kind(
        &mut self,
        piece: PieceId,
        target: InternalTarget,
        kind: InternalKind,
    ) -> Result<(), CoreError> {
        let i = self.internal_index(piece, target)?;
        if !kind.fits(target) {
            return Err(CoreError::Invalid(format!(
                "le type « {} » ne convient pas à ce {}",
                kind.key(),
                target_text(target)
            )));
        }
        self.piece_mut(piece)?.internal[i].kind = kind;
        Ok(())
    }

    /// Ajoute tout ce que `piece_internal_import` propose (le rapport complet est
    /// lu avant, par `piece_internal_import` : l'interface le traduit et l'affiche).
    pub(crate) fn import_piece_internals(&mut self, piece: PieceId) -> Result<(), CoreError> {
        let report = self.piece_internal_import(piece)?;
        self.piece_mut(piece)?
            .internal
            .extend(report.added.iter().copied());
        Ok(())
    }

    /// Retire les éléments internes dont la cible a disparu pendant la commande
    /// (suppression d'un tracé, d'un point, d'un cran), avec une remarque. Ne touche
    /// que les éléments qui existaient avant la commande : une référence morte
    /// introduite par la commande elle-même reste refusée par la validation.
    pub(crate) fn prune_dead_internals(&mut self, before: &Project, out: &mut Outcome) {
        let dead: Vec<(PieceId, Vec<InternalTarget>)> = self
            .pieces
            .iter()
            .filter(|p| !p.internal.is_empty())
            .map(|p| {
                let old = before.pieces.iter().find(|o| o.id == p.id);
                let gone = p
                    .internal
                    .iter()
                    .filter(|e| {
                        old.is_some_and(|o| o.internal.contains(e))
                            && !self.internal_target_exists(p.link, e.target)
                    })
                    .map(|e| e.target)
                    .collect();
                (p.id, gone)
            })
            .filter(|(_, gone): &(PieceId, Vec<InternalTarget>)| !gone.is_empty())
            .collect();
        for (id, gone) in dead {
            if let Ok(p) = self.piece_mut(id) {
                p.internal.retain(|e| !gone.contains(&e.target));
                for t in gone {
                    out.notes.push(format!(
                        "Un élément interne de la pièce « {} » ({}) référençait un objet supprimé : \
                         il est retiré.",
                        p.name,
                        target_text(t)
                    ));
                }
            }
        }
    }
}
