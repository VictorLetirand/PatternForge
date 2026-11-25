//! US-03.2 — constructions **maintenues** (segment longueur/angle, perpendiculaire, parallèle).
//!
//! Un segment créé par l'une de ces constructions garde ses paramètres de création
//! (`Construction`, provenance). Avec `kept` (`Command::SetConstructionKept`), les points
//! construits **suivent** leur référence : déplacer le point de départ, ou le segment de référence,
//! déplace le résultat dans la même étape annulable, en chaîne si besoin (ADR-B : graphe orienté
//! sans cycle, aucun solveur global).
//!
//! Le point d'arrivée reste un point **libre** dont les coordonnées sont écrites dans le fichier :
//! un lecteur qui ignore le lien voit la bonne géométrie, délier garde les positions courantes.
//! Invariant (validé à l'ouverture et après chaque commande) : position écrite = position
//! recalculée, à `KEPT_TOLERANCE_MM` près.
//!
//! Points suivis : `Polar` et `Perpendicular` → le point d'arrivée `to` ; `Parallel` → les deux
//! extrémités. Parents : point de départ (`Polar`, `Perpendicular`), extrémités du segment de
//! référence (`Perpendicular`, `Parallel`). Le rectangle n'est pas repris (US-03.2b).
//!
//! Règles par opération (`Project::sync_kept`, dans `Project::apply`) :
//! - parent déplacé : le point suivi est recalculé (ordre topologique) ; une géométrie dégénérée
//!   (référence confondue, longueur nulle…) refuse la commande entière ;
//! - point suivi déplacé directement (déplacer, transformer…) : lien **délié**, avec remarque ;
//! - référence supprimée ou changée d'extrémités / de sens, point suivi redéfini, extrémités ou
//!   paramètres changés, construction effacée (prolonger, raccourcir, inverser…) : lien délié, avec remarque ;
//! - copie de feuille / import : lien conservé (identifiants remappés) ; presse-papiers : copie
//!   non maintenue, avec remarque ; dupliquer : la copie n'a pas de construction.

use crate::command::Outcome;
use crate::error::CoreError;
use crate::model::{Construction, EntityId, EntityKind, PointId, Project, Sheet};
use pf_geometry::{PointMm, Side};
use std::collections::{HashMap, HashSet};

/// Écart toléré entre la position écrite d'un point suivi et la position recalculée.
pub const KEPT_TOLERANCE_MM: f64 = 1e-9;

/// Un segment maintenu : identifiant, construction (avec `kept`) et extrémités.
#[derive(Debug, Clone, Copy)]
pub(crate) struct KeptLink {
    pub segment: EntityId,
    pub construction: Construction,
    pub from: PointId,
    pub to: PointId,
}

impl KeptLink {
    /// Points écrits par la construction.
    pub(crate) fn dependents(&self) -> Vec<PointId> {
        match self.construction {
            Construction::Parallel { .. } => vec![self.from, self.to],
            _ => vec![self.to],
        }
    }
}

impl Sheet {
    /// Segments maintenus de la feuille, dans l'ordre des tracés.
    pub(crate) fn kept_links(&self) -> Vec<KeptLink> {
        self.entities
            .iter()
            .filter_map(|e| match e.kind {
                EntityKind::Segment {
                    from,
                    to,
                    construction: Some(c),
                } if c.is_kept() => Some(KeptLink {
                    segment: e.id,
                    construction: c,
                    from,
                    to,
                }),
                _ => None,
            })
            .collect()
    }

    /// Segments maintenus de la feuille (identifiants, dans l'ordre des tracés).
    pub fn kept_segment_ids(&self) -> Vec<EntityId> {
        self.kept_links().into_iter().map(|l| l.segment).collect()
    }

    /// Le segment est-il maintenu par sa construction ?
    pub fn is_kept_segment(&self, segment: EntityId) -> bool {
        matches!(
            self.entity(segment).map(|e| &e.kind),
            Ok(EntityKind::Segment {
                construction: Some(c),
                ..
            }) if c.is_kept()
        )
    }

    /// Segment maintenu qui écrit la position de ce point, s'il y en a un.
    pub fn kept_segment_of_point(&self, point: PointId) -> Option<EntityId> {
        self.kept_links()
            .into_iter()
            .find(|l| l.dependents().contains(&point))
            .map(|l| l.segment)
    }

    /// Extrémités (identifiants) du segment de référence d'un lien, sinon erreur expliquée.
    fn kept_reference_ids(
        &self,
        link: &KeptLink,
        reference: EntityId,
    ) -> Result<(PointId, PointId), CoreError> {
        if reference == link.segment {
            return Err(CoreError::Invalid(format!(
                "{} : un segment ne peut pas être sa propre référence",
                self.entity_name(link.segment)
            )));
        }
        self.segment_point_ids(reference).map_err(|_| {
            CoreError::Invalid(format!(
                "{} : le segment de référence n'existe plus",
                self.entity_name(link.segment)
            ))
        })
    }

    /// Points dont dépend la position écrite par le lien.
    fn kept_parents(&self, link: &KeptLink) -> Result<Vec<PointId>, CoreError> {
        Ok(match link.construction {
            Construction::Polar { .. } => vec![link.from],
            Construction::Perpendicular { reference, .. } => {
                let (ra, rb) = self.kept_reference_ids(link, reference)?;
                vec![link.from, ra, rb]
            }
            Construction::Parallel { reference, .. } => {
                let (ra, rb) = self.kept_reference_ids(link, reference)?;
                vec![ra, rb]
            }
        })
    }

    /// Positions que la construction donne aux points suivis, d'après les positions actuelles
    /// des parents. Erreur expliquée si la géométrie est dégénérée.
    pub(crate) fn kept_expected(
        &self,
        link: &KeptLink,
    ) -> Result<Vec<(PointId, PointMm)>, CoreError> {
        let name = self.entity_name(link.segment);
        let geo = |e: pf_geometry::GeometryError| {
            CoreError::Invalid(format!("{name} (construction maintenue) : {e}"))
        };
        match link.construction {
            Construction::Polar {
                length_mm,
                angle_deg,
                ..
            } => {
                let a = self.position(link.from)?;
                let b = pf_geometry::point_from_polar(a, length_mm, angle_deg).map_err(geo)?;
                Ok(vec![(link.to, b)])
            }
            Construction::Perpendicular {
                reference,
                length_mm,
                side,
                ..
            } => {
                let (ia, ib) = self.kept_reference_ids(link, reference)?;
                let (ra, rb) = (self.position(ia)?, self.position(ib)?);
                let start = self.position(link.from)?;
                let end =
                    pf_geometry::perpendicular_end(ra, rb, start, length_mm, side).map_err(geo)?;
                Ok(vec![(link.to, end)])
            }
            Construction::Parallel {
                reference,
                distance_mm,
                side,
                ..
            } => {
                let (ia, ib) = self.kept_reference_ids(link, reference)?;
                let (ra, rb) = (self.position(ia)?, self.position(ib)?);
                pf_geometry::segment_length_mm(ra, rb).map_err(geo)?;
                let normal = (rb - ra).unit().map_err(geo)?.rotated_left();
                let k = match side {
                    Side::Left => distance_mm,
                    Side::Right => -distance_mm,
                };
                let a = (ra + normal * k).validate().map_err(geo)?;
                let b = (rb + normal * k).validate().map_err(geo)?;
                Ok(vec![(link.from, a), (link.to, b)])
            }
        }
    }

    /// Vérifie la structure des liens (points libres, un seul lien par point, pas d'extrémité
    /// d'arc, référence vivante, aucun cycle) et les renvoie dans l'ordre de calcul : un lien
    /// vient après ceux dont dépendent ses parents.
    pub(crate) fn kept_plan(&self) -> Result<Vec<KeptLink>, CoreError> {
        self.kept_plan_at().map_err(|(_, e)| e)
    }

    /// Comme [`Sheet::kept_plan`], avec le segment en cause quand l'erreur peut lui être
    /// attribuée (diagnostic, US-03.5).
    pub(crate) fn kept_plan_at(&self) -> Result<Vec<KeptLink>, (Option<EntityId>, CoreError)> {
        let links = self.kept_links();
        if links.is_empty() {
            return Ok(links);
        }
        let arc_points: HashSet<PointId> = self
            .entities
            .iter()
            .filter_map(|e| match e.kind {
                EntityKind::Arc {
                    center, start, end, ..
                } => Some([center, start, end]),
                _ => None,
            })
            .flatten()
            .collect();
        let mut owner: HashMap<PointId, usize> = HashMap::new();
        let mut parents: Vec<Vec<PointId>> = Vec::with_capacity(links.len());
        for (i, l) in links.iter().enumerate() {
            let name = self.entity_name(l.segment);
            let at = |e: CoreError| (Some(l.segment), e);
            parents.push(self.kept_parents(l).map_err(at)?);
            for d in l.dependents() {
                let p = self.point(d).map_err(at)?;
                if !p.definition.is_free() {
                    return Err(at(CoreError::Invalid(format!(
                        "{name} : le point {} est construit à partir d'autres tracés, une \
                         construction maintenue ne peut pas le déplacer",
                        p.label
                    ))));
                }
                if arc_points.contains(&d) {
                    return Err(at(CoreError::Invalid(format!(
                        "{name} : le point {} est le centre ou une extrémité d'un arc, une \
                         construction maintenue ne peut pas le déplacer",
                        p.label
                    ))));
                }
                if let Some(j) = owner.insert(d, i) {
                    return Err(at(CoreError::Invalid(format!(
                        "le point {} est déjà suivi par {} : {name} ne peut pas le suivre aussi",
                        p.label,
                        self.entity_name(links[j].segment)
                    ))));
                }
            }
        }
        let deps = |p: PointId| -> Vec<PointId> {
            if let Some(&i) = owner.get(&p) {
                return parents[i].clone();
            }
            match self.point(p) {
                Ok(pt) => {
                    let mut v = pt.definition.point_dependencies();
                    for e in pt.definition.dependencies() {
                        if let Ok(ent) = self.entity(e) {
                            v.extend(ent.kind.all_points());
                        }
                    }
                    v
                }
                Err(_) => Vec::new(),
            }
        };
        // Parcours en profondeur, itératif : un lien est placé quand son premier point suivi est
        // terminé, donc après les liens dont dépendent ses parents.
        let mut state: HashMap<PointId, u8> = HashMap::new();
        let mut placed = vec![false; links.len()];
        let mut order = Vec::with_capacity(links.len());
        for start in links.iter().flat_map(|l| l.dependents()) {
            if state.get(&start) == Some(&2) {
                continue;
            }
            state.insert(start, 1);
            let mut stack = vec![(start, deps(start), 0usize)];
            while let Some(top) = stack.last_mut() {
                if top.2 < top.1.len() {
                    let q = top.1[top.2];
                    top.2 += 1;
                    match state.get(&q) {
                        Some(1) => {
                            return Err((
                                owner.get(&q).map(|&i| links[i].segment),
                                CoreError::Invalid(format!(
                                    "dépendance circulaire entre constructions maintenues autour \
                                     du point {}",
                                    self.label(q)
                                )),
                            ));
                        }
                        Some(_) => {}
                        None => {
                            state.insert(q, 1);
                            stack.push((q, deps(q), 0));
                        }
                    }
                } else {
                    let p = top.0;
                    stack.pop();
                    state.insert(p, 2);
                    if let Some(&i) = owner.get(&p) {
                        if !placed[i] {
                            placed[i] = true;
                            order.push(links[i]);
                        }
                    }
                }
            }
        }
        Ok(order)
    }

    fn set_segment_kept(&mut self, segment: EntityId, value: bool) {
        if let Some(e) = self.entities.iter_mut().find(|e| e.id == segment) {
            if let EntityKind::Segment {
                construction: Some(c),
                ..
            } = &mut e.kind
            {
                *c = c.with_kept(value);
            }
        }
    }

    pub(crate) fn write_free_point(&mut self, point: PointId, at: PointMm) {
        if let Some(p) = self.points.iter_mut().find(|p| p.id == point) {
            p.definition = crate::model::PointDefinition::Free { x: at.x, y: at.y };
        }
    }
}

fn kept_state(sheet: &Sheet) -> bool {
    sheet.entities.iter().any(|e| {
        matches!(
            e.kind,
            EntityKind::Segment {
                construction: Some(c),
                ..
            } if c.is_kept()
        )
    })
}

impl Project {
    /// `SetConstructionKept` : lie (`true`) ou délie (`false`) la construction d'un segment.
    /// Sans effet (aucune étape) si l'état demandé est déjà le bon. Lier refuse un segment sans
    /// construction mémorisée, dont la référence a disparu, ou qui ne correspond plus à sa
    /// construction (écart > 1e-6 mm : points déplacés depuis la création).
    pub(crate) fn set_construction_kept(
        &mut self,
        sheet: crate::model::SheetId,
        segment: EntityId,
        kept: bool,
    ) -> Result<(), CoreError> {
        let s = self.sheet(sheet)?;
        let EntityKind::Segment {
            from,
            to,
            construction,
        } = s.entity(segment)?.kind
        else {
            return Err(CoreError::NotASegment(segment));
        };
        let name = s.entity_name(segment);
        let Some(c) = construction else {
            if !kept {
                return Ok(());
            }
            return Err(CoreError::Invalid(format!(
                "{name} : aucune construction mémorisée (seuls les segments créés par longueur \
                 et angle, perpendiculaire ou parallèle peuvent être maintenus)"
            )));
        };
        if c.is_kept() == kept {
            return Ok(());
        }
        if kept {
            let link = KeptLink {
                segment,
                construction: c.with_kept(true),
                from,
                to,
            };
            s.kept_parents(&link)?;
            if let Some(c) = link
                .dependents()
                .into_iter()
                .find_map(|d| s.length_constraint_moving_point(d))
            {
                return Err(CoreError::Invalid(format!(
                    "{name} : un point de cette construction est déplacé par la longueur imposée de                      {} ; retirez d'abord cette contrainte",
                    s.entity_name(c.curve)
                )));
            }
            for (point, expected) in s.kept_expected(&link)? {
                let gap = pf_geometry::distance_mm(s.position(point)?, expected);
                if gap > crate::command::MERGE_TOLERANCE_MM {
                    return Err(CoreError::Invalid(format!(
                        "{name} ne correspond plus à sa construction : le point {} est à {} mm \
                         de la position calculée (points déplacés depuis la création). Recréez \
                         la construction pour la maintenir.",
                        s.label(point),
                        crate::input::format_number(gap, 3)
                    )));
                }
            }
        }
        self.sheet_mut(sheet)?.set_segment_kept(segment, kept);
        Ok(())
    }

    /// Réévalue les constructions maintenues après une commande (dans la même étape). `old` est
    /// le projet d'avant la commande ; `quiet` : la commande est un délien explicite (pas de
    /// remarque). Renvoie la désignation des points recalculés.
    pub(crate) fn sync_kept(
        &mut self,
        old: &Project,
        quiet: bool,
        out: &mut Outcome,
    ) -> Result<Vec<String>, CoreError> {
        if !self.sheets.iter().any(kept_state) && !old.sheets.iter().any(kept_state) {
            return Ok(Vec::new());
        }
        let mut moved = Vec::new();
        for si in 0..self.sheets.len() {
            let Some(old_sheet) = old.sheets.iter().find(|s| s.id == self.sheets[si].id) else {
                continue;
            };
            let sheet = &mut self.sheets[si];
            let old_links = old_sheet.kept_links();
            // Liens perdus : la construction a disparu ou n'est plus maintenue.
            for l in &old_links {
                if sheet.entity(l.segment).is_ok() && !sheet.is_kept_segment(l.segment) && !quiet {
                    out.notes.push(format!(
                        "{} n'est plus maintenu : sa construction a été modifiée par cette \
                         commande (les positions actuelles sont conservées).",
                        old_sheet.entity_name(l.segment)
                    ));
                }
            }
            // Liens devenus impossibles : déliés avec remarque, jamais perdus sans trace.
            for l in sheet.kept_links() {
                let name = sheet.entity_name(l.segment);
                let before = old_links.iter().find(|o| o.segment == l.segment);
                let reason = if sheet.kept_parents(&l).is_err() {
                    Some("son segment de référence a été supprimé".to_string())
                } else if before.is_some_and(|b| {
                    b.from != l.from
                        || b.to != l.to
                        || b.construction.with_kept(false) != l.construction.with_kept(false)
                }) {
                    Some("ses extrémités ou ses paramètres de création ont changé".to_string())
                } else if before.is_some()
                    && l.construction.reference().is_some_and(|r| {
                        old_sheet.segment_point_ids(r).ok() != sheet.segment_point_ids(r).ok()
                    })
                {
                    Some("son segment de référence a changé d'extrémités ou de sens".to_string())
                } else if before.is_some() {
                    l.dependents()
                        .into_iter()
                        .find(|d| sheet.point(*d).map_or(true, |p| !p.definition.is_free()))
                        .map(|d| format!("le point {} n'est plus un point libre", sheet.label(d)))
                } else {
                    None
                };
                if let Some(reason) = reason {
                    sheet.set_segment_kept(l.segment, false);
                    out.notes.push(format!(
                        "{name} n'est plus maintenu : {reason} (les positions actuelles sont \
                         conservées)."
                    ));
                }
            }
            for link in sheet.kept_plan()? {
                let expected = sheet.kept_expected(&link)?;
                let mut mismatch = false;
                let mut direct: Option<PointId> = None;
                for (d, e) in &expected {
                    let current = sheet.position(*d)?;
                    if pf_geometry::distance_mm(current, *e) > KEPT_TOLERANCE_MM {
                        mismatch = true;
                    }
                    let was = old_sheet.position(*d).ok();
                    if was.is_some_and(|w| w != current) {
                        direct = Some(*d);
                    }
                }
                if !mismatch {
                    continue;
                }
                if let Some(d) = direct {
                    // Déplacé directement : on ne le ramène pas, on délie.
                    let name = sheet.entity_name(link.segment);
                    sheet.set_segment_kept(link.segment, false);
                    out.notes.push(format!(
                        "{name} n'est plus maintenu : le point {} a été déplacé directement \
                         (les positions actuelles sont conservées).",
                        sheet.label(d)
                    ));
                    continue;
                }
                for (d, e) in expected {
                    moved.push(format!("point {}", sheet.label(d)));
                    sheet.write_free_point(d, e);
                }
            }
        }
        Ok(moved)
    }

    /// Validation à l'ouverture et après chaque commande : structure des liens et position
    /// écrite = position recalculée (à `KEPT_TOLERANCE_MM` près).
    pub(crate) fn validate_kept(&self) -> Result<(), CoreError> {
        for sheet in &self.sheets {
            if let Some((_, e)) = sheet.kept_problems().into_iter().next() {
                return Err(e);
            }
        }
        Ok(())
    }
}

impl Sheet {
    /// Tous les problèmes des constructions maintenues de la feuille (même contrôle que
    /// `validate_kept`, qui garde le premier ; le diagnostic US-03.5 les garde tous), chacun avec
    /// le segment en cause quand il lui est attribuable. Les erreurs sont déjà localisées.
    pub(crate) fn kept_problems(&self) -> Vec<(Option<EntityId>, CoreError)> {
        if !kept_state(self) {
            return Vec::new();
        }
        let _index = crate::sheet_index::IndexGuard::new(self);
        let ctx = |e: CoreError| {
            CoreError::Invalid(format!(
                "feuille « {} », construction maintenue : {e}",
                self.name
            ))
        };
        let plan = match self.kept_plan_at() {
            Ok(plan) => plan,
            Err((at, e)) => return vec![(at, ctx(e))],
        };
        let mut out = Vec::new();
        for link in plan {
            let expected = match self.kept_expected(&link) {
                Ok(expected) => expected,
                Err(e) => {
                    out.push((Some(link.segment), ctx(e)));
                    continue;
                }
            };
            for (d, e) in expected {
                let current = match self.position(d) {
                    Ok(current) => current,
                    Err(err) => {
                        out.push((Some(link.segment), err));
                        continue;
                    }
                };
                if pf_geometry::distance_mm(current, e) > KEPT_TOLERANCE_MM {
                    out.push((
                        Some(link.segment),
                        ctx(CoreError::Invalid(format!(
                            "{} : la position écrite du point {} ({} ; {}) ne correspond pas à la \
                             construction ({} ; {})",
                            self.entity_name(link.segment),
                            self.label(d),
                            crate::input::format_number(current.x, 6),
                            crate::input::format_number(current.y, 6),
                            crate::input::format_number(e.x, 6),
                            crate::input::format_number(e.y, 6),
                        ))),
                    ));
                }
            }
        }
        out
    }
}
