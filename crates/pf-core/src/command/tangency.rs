//! US-02.3 — tangence et raccords : tangente à une courbe, droite tangente à
//! un cercle, congé et chanfrein. Les calculs sont dans `pf_geometry::tangent` ;
//! ce module applique leur résultat au modèle, en une seule commande atomique.
//!
//! Choix de modélisation (voir `docs/DECISIONS.md`, décisions 49 à 53) :
//! - un raccord **raccourcit** les tracés en remplaçant leur extrémité par un
//!   nouveau point au point de tangence (le tracé garde son identifiant, donc
//!   son calque, son groupe, son nom, son rôle, ses crans) ;
//! - un cercle n'est jamais coupé ;
//! - rien n'est perdu en silence : crans hors du tracé raccourci supprimés,
//!   points construits sur un tracé raccourci figés, contours de pièce
//!   incompatibles vidés, tout cela signalé dans `Outcome.notes`.

use super::*;
use crate::input::format_mm;
use pf_geometry::{check_radius, distance_mm, nearest_solution, Circle, FilletPath, Trim, TrimEnd};
use serde::{Deserialize, Serialize};

/// Position d'une tangente sur une courbe.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TangentAt {
    /// Proportion de la longueur développée, de 0 (départ) à 1 (arrivée).
    Fraction(f64),
    /// Point de la courbe le plus proche de cette position.
    Near(PointMm),
}

/// Tracé raccordable, vu comme un chemin géométrique.
fn fillet_path(s: &Sheet, id: EntityId) -> Result<FilletPath, CoreError> {
    match s.shape(id)? {
        Shape::Line(a, b) => Ok(FilletPath::Segment { a, b }),
        Shape::Arc(arc) => Ok(FilletPath::Arc(arc)),
        Shape::Circle(c) => Ok(FilletPath::Circle(c.circle)),
        _ => Err(CoreError::Invalid(format!(
            "{} : seuls les segments, les arcs et les cercles se raccordent",
            s.entity_name(id)
        ))),
    }
}

impl Sheet {
    /// Tracé vu comme chemin raccordable (segment, arc ou cercle), pour les
    /// aperçus de l'interface ; refus pour les autres types.
    pub fn fillet_path(&self, id: EntityId) -> Result<FilletPath, CoreError> {
        fillet_path(self, id)
    }
}

impl Project {
    /// Point libre en `at` ; réutilise un point déjà situé exactement là (1e-9 mm,
    /// pas la tolérance de fusion de 1e-6 mm : la tangence doit rester exacte).
    pub(crate) fn tangency_point(
        &mut self,
        sheet: SheetId,
        at: PointMm,
        layer: Option<LayerId>,
        out: &mut Outcome,
    ) -> Result<PointId, CoreError> {
        at.validate()?;
        let s = self.sheet(sheet)?;
        for p in &s.points {
            if let Ok(pos) = s.position(p.id) {
                if distance_mm(pos, at) <= EPSILON_MM {
                    return Ok(p.id);
                }
            }
        }
        let id = PointId(self.alloc_id());
        let s = self.sheet_mut(sheet)?;
        let label = s.next_label();
        s.points.push(Point {
            id,
            label,
            definition: PointDefinition::Free { x: at.x, y: at.y },
            role: PointRole::Construction,
            layer,
        });
        out.points.push(id);
        Ok(id)
    }

    /// Droite tangente à un cercle ou un arc, depuis un point (US-02.3).
    pub(super) fn add_tangent_line(
        &mut self,
        sheet: SheetId,
        from: PointRef,
        target: EntityId,
        pick: PointMm,
        length_mm: Option<f64>,
        out: &mut Outcome,
    ) -> Result<(), CoreError> {
        pick.validate()?;
        let (circle, arc) = match self.sheet(sheet)?.shape(target)? {
            Shape::Circle(c) => (c.circle, None),
            Shape::Arc(a) => (Circle::new(a.center, a.radius_mm)?, Some(a)),
            _ => {
                return Err(CoreError::Invalid(format!(
                    "{} : la tangente depuis un point se construit sur un cercle ou un arc",
                    self.sheet(sheet)?.entity_name(target)
                )))
            }
        };
        let layer = self.sheet(sheet)?.entity(target)?.layer;
        let a = self.resolve_ref(sheet, from, out)?;
        let p = self.sheet(sheet)?.position(a)?;
        let mut solutions = pf_geometry::tangent_points_from_point(&circle, p)?;
        if let Some(arc) = arc {
            solutions.retain(|t| arc.contains_angle_of(*t));
            if solutions.is_empty() {
                return Err(CoreError::Invalid(format!(
                    "{} : aucune tangente depuis ce point ne touche l'arc (elle toucherait son \
                     prolongement en cercle)",
                    self.sheet(sheet)?.entity_name(target)
                )));
            }
        }
        let end = if solutions.len() == 1 && distance_mm(solutions[0], p) <= EPSILON_MM {
            // Point sur le cercle : une seule tangente, de longueur donnée, du côté visé.
            let len = length_mm.ok_or_else(|| {
                CoreError::Invalid(
                    "le point est sur le cercle : indiquez la longueur de la tangente".into(),
                )
            })?;
            check_radius(len)?;
            if let Some(arc) = arc {
                if !arc.contains_angle_of(p) {
                    return Err(CoreError::Invalid(
                        "le point est sur le prolongement du cercle, hors de l'arc".into(),
                    ));
                }
            }
            let u = (p - circle.center).unit()?.rotated_left();
            let k = if (pick - p).dot(u) >= 0.0 { len } else { -len };
            p + u * k
        } else {
            nearest_solution(&solutions, pick)
                .ok_or_else(|| CoreError::Invalid("aucune tangente depuis ce point".into()))?
        };
        let b = self.tangency_point(sheet, end, layer, out)?;
        self.add_segment(sheet, a, b, None, out)
    }

    /// Segment tangent à une courbe, de longueur donnée, depuis son point de contact.
    pub(super) fn add_tangent_at(
        &mut self,
        sheet: SheetId,
        entity: EntityId,
        at: TangentAt,
        length_mm: f64,
        backwards: bool,
        out: &mut Outcome,
    ) -> Result<(), CoreError> {
        check_radius(length_mm)?;
        let s = self.sheet(sheet)?;
        if matches!(s.shape(entity)?, Shape::Line(..)) {
            return Err(CoreError::Invalid(format!(
                "{} : un segment est sa propre tangente",
                s.entity_name(entity)
            )));
        }
        let len = s.path_length_mm(entity)?;
        let d = match at {
            TangentAt::Fraction(f) => {
                if !f.is_finite() || !(0.0..=1.0).contains(&f) {
                    return Err(pf_geometry::GeometryError::ParameterOutOfRange(f).into());
                }
                f * len
            }
            TangentAt::Near(p) => {
                p.validate()?;
                s.path_distance_of(entity, p)?
            }
        };
        let (contact, tangent) = s.path_point_at(entity, d.clamp(0.0, len))?;
        let layer = s.entity(entity)?.layer;
        let k = if backwards { -length_mm } else { length_mm };
        let end = (contact + tangent.unit()? * k).validate()?;
        let a = self.tangency_point(sheet, contact, layer, out)?;
        let b = self.tangency_point(sheet, end, layer, out)?;
        self.add_segment(sheet, a, b, None, out)
    }

    /// Points construits sur un des tracés `entities` : ils sont figés à leur
    /// position actuelle (le tracé va changer), avec une remarque.
    pub(super) fn freeze_dependents(
        &mut self,
        sheet: SheetId,
        entities: &[EntityId],
        out: &mut Outcome,
    ) -> Result<(), CoreError> {
        let s = self.sheet(sheet)?;
        let mut frozen = Vec::new();
        for p in &s.points {
            if p.definition
                .dependencies()
                .iter()
                .any(|d| entities.contains(d))
            {
                frozen.push((p.id, s.position(p.id)?, p.label.clone()));
            }
        }
        let s = self.sheet_mut(sheet)?;
        for (id, pos, label) in frozen {
            if let Some(p) = s.points.iter_mut().find(|p| p.id == id) {
                p.definition = PointDefinition::Free { x: pos.x, y: pos.y };
                out.notes.push(format!(
                    "Le point {label} dépendait d'un tracé raccourci : il est maintenant libre, \
                     à sa position actuelle."
                ));
            }
        }
        Ok(())
    }

    /// Ramène l'extrémité `trim.end` du tracé au point `at`. Renvoie le point
    /// d'extrémité du tracé de ce côté après coupe (nouveau, ou l'ancien si
    /// rien n'est retiré). Les crans hors du tracé raccourci sont supprimés
    /// (remarque), ceux qui restent sont recalés.
    pub(super) fn trim_path(
        &mut self,
        sheet: SheetId,
        entity: EntityId,
        trim: Trim,
        at: PointMm,
        out: &mut Outcome,
    ) -> Result<PointId, CoreError> {
        let s = self.sheet(sheet)?;
        let e = s.entity(entity)?;
        let layer = e.layer;
        let (from_end, to_end) = match e.kind {
            EntityKind::Segment { from, to, .. } => (from, to),
            EntityKind::Arc { start, end, .. } => (start, end),
            _ => {
                return Err(CoreError::Invalid(
                    "seuls les segments et les arcs se raccourcissent".into(),
                ))
            }
        };
        let old = if trim.end == TrimEnd::Start {
            from_end
        } else {
            to_end
        };
        if trim.removed_mm <= EPSILON_MM {
            return Ok(old);
        }
        let name = s.entity_name(entity);
        let old_len = s.path_length_mm(entity)?;
        self.release_notch_anchors(sheet, &[entity], out)?;
        let new = self.tangency_point(sheet, at, layer, out)?;
        let s = self.sheet_mut(sheet)?;
        match &mut s.entity_mut(entity)?.kind {
            EntityKind::Segment {
                from,
                to,
                construction,
            } => {
                if trim.end == TrimEnd::Start {
                    *from = new;
                } else {
                    *to = new;
                }
                // Les paramètres de création décrivaient l'ancien segment.
                *construction = None;
            }
            EntityKind::Arc { start, end, .. } => {
                if trim.end == TrimEnd::Start {
                    *start = new;
                } else {
                    *end = new;
                }
            }
            _ => {}
        }
        // Crans : recalés, ou supprimés s'ils tombent dans la partie retirée.
        let new_len = old_len - trim.removed_mm;
        let mut dropped = Vec::new();
        for n in s.notches.iter_mut().filter(|n| n.entity == entity) {
            match trim.end {
                TrimEnd::Start => {
                    let d = n.distance_mm - trim.removed_mm;
                    if d < -EPSILON_MM {
                        dropped.push(n.id);
                    } else {
                        n.distance_mm = d.max(0.0);
                    }
                }
                TrimEnd::End => {
                    if n.distance_mm > new_len + EPSILON_MM {
                        dropped.push(n.id);
                    } else {
                        n.distance_mm = n.distance_mm.min(new_len);
                    }
                }
            }
        }
        for id in &dropped {
            if let Some(n) = s.notches.iter().find(|n| n.id == *id) {
                out.notes.push(format!(
                    "Le cran à {} mm de {name} tombait dans la partie retirée : supprimé.",
                    format_mm(n.distance_mm)
                ));
            }
        }
        s.notches.retain(|n| !dropped.contains(&n.id));
        self.drop_unused_point(sheet, old)?;
        Ok(new)
    }

    /// L'ancienne extrémité, libre et devenue inutile, disparaît (comme à la
    /// suppression d'un tracé) ; un repère ou un point coté est conservé.
    pub(super) fn drop_unused_point(
        &mut self,
        sheet: SheetId,
        old: PointId,
    ) -> Result<(), CoreError> {
        let s = self.sheet_mut(sheet)?;
        let unused = !s
            .entities
            .iter()
            .any(|x| x.kind.all_points().contains(&old))
            && !s.dimensions.iter().any(|d| d.from == old || d.to == old)
            && !s
                .points
                .iter()
                .any(|p| p.definition.point_dependencies().contains(&old));
        s.points.retain(|p| {
            !(p.id == old
                && unused
                && matches!(p.definition, PointDefinition::Free { .. })
                && p.role == PointRole::Construction)
        });
        Ok(())
    }

    /// Range un tracé de raccord comme ses parents : calque du premier, rôle
    /// commun s'ils en ont un, groupe commun ; insère-le dans les contours de
    /// pièce où les deux tracés se suivent (sinon vide le contour concerné).
    fn finish_joint(
        &mut self,
        sheet: SheetId,
        first: EntityId,
        second: EntityId,
        joint: EntityId,
        trimmed: &[EntityId],
        out: &mut Outcome,
    ) -> Result<(), CoreError> {
        let s = self.sheet_mut(sheet)?;
        let (layer, role_a) = {
            let a = s.entity(first)?;
            (a.layer, a.role)
        };
        let role_b = s.entity(second)?.role;
        for p in s.points.iter_mut().filter(|p| out.points.contains(&p.id)) {
            p.layer = layer;
        }
        let j = s.entity_mut(joint)?;
        j.layer = layer;
        j.role = if role_a == role_b {
            role_a
        } else {
            PathRole::Construction
        };
        let shared = s
            .groups
            .iter_mut()
            .find(|g| g.members.contains(&first) && g.members.contains(&second));
        if let Some(g) = shared {
            g.members.push(joint);
        }
        for piece in &mut self.pieces {
            let linked_here = match piece.link {
                PieceLink::Linked { sheet: sh } => sh == sheet,
                PieceLink::Detached => true,
            };
            if !linked_here {
                continue;
            }
            let pos = |e: EntityId| piece.outline.iter().position(|x| x.entity == e);
            let (ia, ib) = (pos(first), pos(second));
            let n = piece.outline.len();
            match (ia, ib) {
                (Some(a), Some(b)) => {
                    let forward = (a + 1) % n == b;
                    let backward = (b + 1) % n == a;
                    if forward || backward {
                        // Entre le dernier et le premier : en fin de liste.
                        let (at, reversed) = if forward {
                            (a + 1, false)
                        } else {
                            (b + 1, true)
                        };
                        piece.outline.insert(
                            at,
                            PieceEdge {
                                entity: joint,
                                reversed,
                            },
                        );
                    } else {
                        piece.outline.clear();
                        out.notes.push(format!(
                            "Le contour de la pièce « {} » ne chaînait pas les deux tracés \
                             raccordés : il est vidé.",
                            piece.name
                        ));
                    }
                }
                (Some(_), None) | (None, Some(_)) => {
                    let touched = [(ia, first), (ib, second)]
                        .iter()
                        .any(|(i, e)| i.is_some() && trimmed.contains(e));
                    if touched {
                        out.notes.push(format!(
                            "Un tracé raccourci appartient à la pièce « {} » sans son voisin \
                             raccordé : vérifiez son contour.",
                            piece.name
                        ));
                    }
                }
                (None, None) => {}
            }
        }
        Ok(())
    }

    /// Congé de rayon donné entre deux tracés (US-02.3).
    pub(super) fn fillet_paths(
        &mut self,
        sheet: SheetId,
        first: EntityId,
        second: EntityId,
        radius_mm: f64,
        pick: (PointMm, PointMm),
        out: &mut Outcome,
    ) -> Result<(), CoreError> {
        if first == second {
            return Err(CoreError::Invalid(
                "choisissez deux tracés différents".into(),
            ));
        }
        let s = self.sheet(sheet)?;
        let (pa, pb) = (fillet_path(s, first)?, fillet_path(s, second)?);
        let f = pf_geometry::fillet(&pa, &pb, radius_mm, pick.0, pick.1).map_err(|e| {
            CoreError::Invalid(format!(
                "congé impossible entre {} et {} : {e}",
                s.entity_name(first),
                s.entity_name(second)
            ))
        })?;
        let layer = s.entity(first)?.layer;
        let modified: Vec<EntityId> = [(first, f.trim_first), (second, f.trim_second)]
            .into_iter()
            .filter(|(_, t)| t.is_some_and(|t| t.removed_mm > EPSILON_MM))
            .map(|(e, _)| e)
            .collect();
        self.freeze_dependents(sheet, &modified, out)?;
        let ta = match f.trim_first {
            Some(t) => self.trim_path(sheet, first, t, f.tangent_first, out)?,
            None => self.tangency_point(sheet, f.tangent_first, layer, out)?,
        };
        let tb = match f.trim_second {
            Some(t) => self.trim_path(sheet, second, t, f.tangent_second, out)?,
            None => self.tangency_point(sheet, f.tangent_second, layer, out)?,
        };
        let center = self.tangency_point(sheet, f.center, layer, out)?;
        self.add_arc_entity(sheet, center, ta, tb, f.ccw, out)?;
        let joint = out.entities[out.entities.len() - 1];
        self.finish_joint(sheet, first, second, joint, &modified, out)
    }

    /// Chanfrein entre deux segments, à une distance du coin sur chacun (US-02.3).
    pub(super) fn chamfer_paths(
        &mut self,
        sheet: SheetId,
        first: EntityId,
        second: EntityId,
        distances: (f64, f64),
        pick: (PointMm, PointMm),
        out: &mut Outcome,
    ) -> Result<(), CoreError> {
        if first == second {
            return Err(CoreError::Invalid(
                "choisissez deux tracés différents".into(),
            ));
        }
        let s = self.sheet(sheet)?;
        let ends = |id: EntityId| -> Result<(PointMm, PointMm), CoreError> {
            match s.shape(id)? {
                Shape::Line(a, b) => Ok((a, b)),
                _ => Err(CoreError::Invalid(format!(
                    "{} : un chanfrein se fait entre deux segments",
                    s.entity_name(id)
                ))),
            }
        };
        let (ea, eb) = (ends(first)?, ends(second)?);
        let c = pf_geometry::chamfer(ea, eb, distances.0, distances.1, pick.0, pick.1).map_err(
            |e| {
                CoreError::Invalid(format!(
                    "chanfrein impossible entre {} et {} : {e}",
                    s.entity_name(first),
                    s.entity_name(second)
                ))
            },
        )?;
        let modified: Vec<EntityId> = [(first, c.trim_first), (second, c.trim_second)]
            .into_iter()
            .filter(|(_, t)| t.removed_mm > EPSILON_MM)
            .map(|(e, _)| e)
            .collect();
        self.freeze_dependents(sheet, &modified, out)?;
        let ta = self.trim_path(sheet, first, c.trim_first, c.first, out)?;
        let tb = self.trim_path(sheet, second, c.trim_second, c.second, out)?;
        self.add_segment(sheet, ta, tb, None, out)?;
        let joint = out.entities[out.entities.len() - 1];
        self.finish_joint(sheet, first, second, joint, &modified, out)
    }
}
