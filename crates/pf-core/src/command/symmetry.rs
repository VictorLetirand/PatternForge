//! US-02.11 — symétrie liée : la moitié construite est maintenue en miroir par
//! des points `PointDefinition::Mirror` (réflexion exacte, recalculée à chaque
//! évaluation) ; « déplier » rend la symétrie indépendante.
//!
//! Choix de modélisation (voir `docs/DECISIONS.md`) :
//! - un point à moins de `MERGE_TOLERANCE_MM` de l'axe est **partagé**, pas
//!   dupliqué : les deux moitiés se rejoignent topologiquement sur l'axe ;
//! - le tracé miroir est parcouru en sens inverse (extrémités échangées), ce
//!   qui conserve l'orientation d'un contour ; poignées, tangentes, départ de
//!   cercle et crans sont reflétés **à la création** (seuls les points sont
//!   maintenus) ;
//! - rôle et calque des originaux sont conservés ; le nom et le groupe, non.

use super::*;
use crate::contour::closed_contours;
use crate::relative::AxisLine;
use std::collections::{BTreeSet, HashSet};

/// Tracé miroir de `kind` : extrémités remplacées par leurs images et
/// échangées (sens inversé), vecteurs reflétés.
fn mirrored_kind(
    kind: &EntityKind,
    map: &BTreeMap<PointId, PointId>,
    line: &AxisLine,
) -> EntityKind {
    let refl = |o: OffsetMm| OffsetMm::from_vector(line.reflect_vector(o.vector()));
    let refl_rev = |o: OffsetMm| {
        let r = refl(o);
        OffsetMm {
            dx: -r.dx,
            dy: -r.dy,
        }
    };
    match kind {
        EntityKind::Segment { from, to, .. } => EntityKind::Segment {
            from: map[to],
            to: map[from],
            construction: None,
        },
        EntityKind::Curve {
            start,
            handle_start,
            handle_end,
            end,
        } => EntityKind::Curve {
            start: map[end],
            handle_start: refl(*handle_end),
            handle_end: refl(*handle_start),
            end: map[start],
        },
        EntityKind::Circle {
            center,
            radius_mm,
            start_deg,
            ccw,
        } => {
            let (s, c) = pf_geometry::sin_cos_deg(*start_deg);
            let v = line.reflect_vector(pf_geometry::VectorMm::new(c, s));
            EntityKind::Circle {
                center: map[center],
                radius_mm: *radius_mm,
                start_deg: v.y.atan2(v.x).to_degrees().rem_euclid(360.0),
                // Symétrie (sens inversé) puis inversion du parcours : inchangé.
                ccw: *ccw,
            }
        }
        EntityKind::Arc {
            center,
            start,
            end,
            ccw,
        } => EntityKind::Arc {
            center: map[center],
            start: map[end],
            end: map[start],
            ccw: *ccw,
        },
        EntityKind::Spline { nodes } => EntityKind::Spline {
            nodes: nodes
                .iter()
                .rev()
                .map(|n| {
                    if n.corner {
                        SplineNode {
                            point: map[&n.point],
                            corner: true,
                            tangent: n.tangent_in.map(refl_rev),
                            tangent_in: n.tangent.map(refl_rev),
                        }
                    } else {
                        SplineNode {
                            point: map[&n.point],
                            corner: false,
                            tangent: n.tangent.map(refl_rev),
                            tangent_in: None,
                        }
                    }
                })
                .collect(),
        },
    }
}

impl Project {
    pub(super) fn link_mirror(
        &mut self,
        sheet: SheetId,
        points: &[PointId],
        entities: &[EntityId],
        axis: MirrorAxis,
        out: &mut Outcome,
    ) -> Result<(), CoreError> {
        let s = self.sheet(sheet)?;
        if let MirrorAxis::Guide { guide } = axis {
            if !s.guide(guide)?.axis {
                return Err(CoreError::Invalid(
                    "cette ligne d'aide n'est pas marquée comme axe de symétrie".into(),
                ));
            }
        }
        let line = s.mirror_axis(axis)?;
        let ents: BTreeSet<EntityId> = entities.iter().copied().collect();
        let mut sources: BTreeSet<PointId> = points.iter().copied().collect();
        if ents.is_empty() && sources.is_empty() {
            return Err(CoreError::Invalid(
                "symétrie liée : sélectionnez des points ou des tracés".into(),
            ));
        }
        if let MirrorAxis::Segment { segment } = axis {
            if ents.contains(&segment) {
                return Err(CoreError::Invalid(
                    "symétrie liée : l'axe ne peut pas faire partie de la sélection".into(),
                ));
            }
        }
        for &e in &ents {
            sources.extend(s.entity_all_points(e)?);
        }
        for &p in &sources {
            let point = s.point(p)?;
            if point.definition.is_mirror() {
                return Err(CoreError::Invalid(format!(
                    "le point {} est déjà maintenu par symétrie : il ne peut pas être la source \
                     d'une autre symétrie",
                    point.label
                )));
            }
        }
        // Points miroir (partagés s'ils sont sur l'axe).
        let mut map: BTreeMap<PointId, PointId> = BTreeMap::new();
        for &p in &sources {
            let s = self.sheet(sheet)?;
            if line.distance(s.position(p)?) <= MERGE_TOLERANCE_MM {
                map.insert(p, p);
                continue;
            }
            let definition = PointDefinition::Mirror { source: p, axis };
            s.evaluate(&definition)?;
            let src = s.point(p)?.clone();
            let label = s.next_label();
            let id = PointId(self.alloc_id());
            self.sheet_mut(sheet)?.points.push(Point {
                id,
                label,
                definition,
                role: src.role,
                layer: src.layer,
            });
            out.points.push(id);
            map.insert(p, id);
        }
        // Tracés miroir.
        for &e in &ents {
            let src = self.sheet(sheet)?.entity(e)?.clone();
            let kind = mirrored_kind(&src.kind, &map, &line);
            let skip = match &kind {
                EntityKind::Segment { from, to, .. } => {
                    from == to
                        || self.sheet(sheet)?.entities.iter().any(|x| {
                            matches!(x.kind, EntityKind::Segment { from: f, to: t, .. }
                                if (f == *from && t == *to) || (f == *to && t == *from))
                        })
                }
                EntityKind::Circle { center, .. } => map.get(center) == Some(center),
                _ => false,
            };
            if skip {
                out.notes.push(format!(
                    "{} est sur l'axe : il n'a pas de miroir distinct.",
                    self.sheet(sheet)?.entity_name(e)
                ));
                continue;
            }
            let id = EntityId(self.alloc_id());
            self.sheet_mut(sheet)?.entities.push(Entity {
                id,
                role: src.role,
                layer: src.layer,
                name: None,
                kind,
            });
            out.entities.push(id);
            let old: Vec<Notch> = self
                .sheet(sheet)?
                .notches
                .iter()
                .filter(|n| n.entity == e)
                .cloned()
                .collect();
            if !old.is_empty() {
                let len = self.sheet(sheet)?.path_length_mm(id)?;
                for n in old {
                    let nid = NotchId(self.alloc_id());
                    // Le sens est inversé : la position se mesure depuis l'autre
                    // bout ; le côté, lui, ne change pas (symétrie + inversion).
                    // Un cran ancré garde un ancrage que la copie peut suivre (milieu,
                    // proportion depuis l'autre bout, point miroir) ; sinon il est recopié
                    // à distance fixe, avec une remarque.
                    let (anchor, distance_mm) = match n.anchor {
                        None => (None, (len - n.distance_mm).clamp(0.0, len)),
                        Some(NotchAnchor::Midpoint {}) => (n.anchor, 0.0),
                        Some(NotchAnchor::Ratio { ratio }) => {
                            (Some(NotchAnchor::Ratio { ratio: 1.0 - ratio }), 0.0)
                        }
                        Some(NotchAnchor::Point { point }) if map.contains_key(&point) => {
                            (Some(NotchAnchor::Point { point: map[&point] }), 0.0)
                        }
                        Some(_) => {
                            let fixed = self.sheet(sheet)?.notch_fixed(&n)?;
                            out.notes.push(
                                "Un cran ancré est recopié à distance fixe : son ancrage cite \
                                 un objet qui n'a pas de miroir."
                                    .into(),
                            );
                            (None, (len - fixed.distance_mm).clamp(0.0, len))
                        }
                    };
                    if n.corner {
                        out.notes.push(
                            "Un cran en coin est recopié comme cran ordinaire : la copie n'est \
                             dans aucune pièce."
                                .into(),
                        );
                    }
                    self.sheet_mut(sheet)?.notches.push(Notch {
                        id: nid,
                        entity: id,
                        distance_mm,
                        anchor,
                        on_cut: false,
                        corner: false,
                        ..n
                    });
                    out.notches.push(nid);
                }
            }
        }
        if out.points.is_empty() && out.entities.is_empty() {
            return Err(CoreError::Invalid(
                "symétrie liée : tout ce qui est sélectionné est déjà sur l'axe".into(),
            ));
        }
        Ok(())
    }

    pub(super) fn unfold_mirror(
        &mut self,
        sheet: SheetId,
        axis: MirrorAxis,
        out: &mut Outcome,
    ) -> Result<(), CoreError> {
        let frozen = self.freeze_where(
            sheet,
            |d| matches!(d, PointDefinition::Mirror { axis: a, .. } if *a == axis),
        )?;
        if frozen.is_empty() {
            return Err(CoreError::Invalid(
                "aucune symétrie liée n'utilise cet axe".into(),
            ));
        }
        let ids: HashSet<PointId> = frozen.iter().copied().collect();
        let s = self.sheet(sheet)?;
        let mirrored: HashSet<EntityId> = s
            .entities
            .iter()
            .filter(|e| e.kind.all_points().iter().any(|p| ids.contains(p)))
            .map(|e| e.id)
            .collect();
        let joined = closed_contours(s)
            .iter()
            .filter(|c| {
                c.edges.iter().any(|e| mirrored.contains(&e.entity))
                    && c.edges.iter().any(|e| !mirrored.contains(&e.entity))
            })
            .count();
        out.notes.push(format!(
            "Symétrie dépliée : {} point(s) miroir sont maintenant libres.",
            frozen.len()
        ));
        out.notes.push(if joined > 0 {
            format!("{joined} contour(s) fermé(s) obtenu(s) par la jonction sur l'axe.")
        } else {
            "Les tracés de la moitié et de son miroir ne forment pas de contour fermé : ils \
             sont laissés tels quels."
                .to_string()
        });
        Ok(())
    }

    /// Les points miroir d'une ligne d'aide qui va être supprimée deviennent libres.
    pub(super) fn freeze_mirrors_of_guide(
        &mut self,
        sheet: SheetId,
        guide: GuideId,
        out: &mut Outcome,
    ) -> Result<(), CoreError> {
        let frozen = self.freeze_where(sheet, |d| d.axis_guide() == Some(guide))?;
        if !frozen.is_empty() {
            out.notes.push(format!(
                "{} point(s) miroir dépendaient de la ligne d'aide supprimée : ils sont \
                 maintenant libres, à leur position actuelle.",
                frozen.len()
            ));
        }
        Ok(())
    }

    /// Fige à leur position actuelle les points dont la définition satisfait `pred`.
    fn freeze_where(
        &mut self,
        sheet: SheetId,
        pred: impl Fn(&PointDefinition) -> bool,
    ) -> Result<Vec<PointId>, CoreError> {
        let s = self.sheet(sheet)?;
        let mut frozen = Vec::new();
        for p in s.points.iter().filter(|p| pred(&p.definition)) {
            frozen.push((p.id, s.position(p.id)?));
        }
        let s = self.sheet_mut(sheet)?;
        for (id, pos) in &frozen {
            if let Some(p) = s.points.iter_mut().find(|p| p.id == *id) {
                p.definition = PointDefinition::Free { x: pos.x, y: pos.y };
            }
        }
        Ok(frozen.into_iter().map(|(id, _)| id).collect())
    }
}
