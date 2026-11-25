//! US-04.1 — extraire une pièce d'une région fermée ou d'une chaîne de tracés.
//!
//! Choix de modélisation (voir `docs/DECISIONS.md`, décisions 96 à 100) :
//! - la pièce stocke toujours `outline: Vec<PieceEdge>` (références à des tracés
//!   réels) : aucune donnée nouvelle, schéma 3 inchangé ;
//! - **liée** : quand la région est bordée par des portions de tracés qui se
//!   croisent sans point commun, ces tracés sont **coupés aux croisements** (mêmes
//!   mécanismes que `SplitPath`) et la pièce référence les tronçons. Un cercle
//!   coupé devient une suite d'arcs ; le tout est une seule commande, donc une
//!   seule étape annulable ;
//! - **détachée** : le contour est copié dans une nouvelle feuille dédiée
//!   (ADR-A) ; la construction n'est pas modifiée ;
//! - aire, périmètre et orientation ne sont pas enregistrés : ils sont recalculés
//!   (`Project::piece_contour`).

use super::paths::SplitHint;
use super::*;
use crate::contour::{contour_from_entities, measure_contour};
use crate::sheet_copy::unique_name;
use pf_geometry::region::{trace_geometry, EdgeGeom};
use pf_geometry::Trace;
use std::collections::{BTreeMap, HashSet};

/// Portion d'un tracé source utilisée par le contour.
struct Part {
    t_lo: f64,
    t_hi: f64,
    /// Extrémités dans le sens du parcours de la pièce.
    start: PointMm,
    end: PointMm,
}

/// Une arête du contour à créer.
struct BEdge {
    entity: EntityId,
    /// Parcourue de l'arrivée vers le départ du tracé source.
    reversed: bool,
    /// `None` : le tracé entier.
    part: Option<Part>,
    /// Géométrie dans le sens du parcours (copie détachée).
    geometry: Vec<EdgeGeom>,
    /// Cercle entier : centre, rayon, angle de départ, sens (copie détachée).
    circle: Option<(PointMm, f64, f64, bool)>,
    role: PathRole,
}

#[derive(Clone, Copy)]
struct Cut {
    t: f64,
    pos: PointMm,
}

/// Où sont passés les tronçons d'un tracé après les coupes.
enum SubMap {
    /// Tracé inchangé.
    Whole(EntityId),
    /// Tracé ouvert : `pieces[k]` couvre `[bounds[k] ; bounds[k + 1]]`.
    Open {
        bounds: Vec<f64>,
        pieces: Vec<EntityId>,
    },
    /// Cercle devenu arcs : `arcs[j]` couvre `[ts[j] ; ts[j + 1]]` (le dernier boucle sur `ts[0] + 1`).
    Circle { ts: Vec<f64>, arcs: Vec<EntityId> },
}

impl SubMap {
    fn find(&self, part: &Part) -> Option<EntityId> {
        match self {
            SubMap::Whole(e) => Some(*e),
            // Le milieu de la portion retrouve son tronçon sans dépendre d'un écart
            // de 1e-9 entre deux paramètres censés être égaux.
            SubMap::Open { bounds, pieces } => {
                let mid = 0.5 * (part.t_lo + part.t_hi);
                (0..pieces.len())
                    .find(|&k| bounds[k] <= mid && mid <= bounds[k + 1])
                    .map(|k| pieces[k])
            }
            SubMap::Circle { ts, arcs } => {
                let m = ts.len();
                let mid = (0.5 * (part.t_lo + part.t_hi)).rem_euclid(1.0);
                (0..m)
                    .find(|&j| {
                        let end = if j + 1 < m { ts[j + 1] } else { ts[0] + 1.0 };
                        (mid - ts[j]).rem_euclid(1.0) <= end - ts[j]
                    })
                    .map(|j| arcs[j])
            }
        }
    }
}

fn region_error(e: pf_geometry::RegionError) -> CoreError {
    CoreError::Invalid(e.to_string())
}

fn shape_ends(shape: &Shape) -> (PointMm, PointMm) {
    match shape {
        Shape::Line(a, b) => (*a, *b),
        Shape::Cubic(c) => (c.p0, c.p3),
        Shape::Arc(a) => (a.start_point(), a.end_point()),
        Shape::Spline(s) => (s.start_point(), s.end_point()),
        Shape::Circle(c) => (c.circle.center, c.circle.center),
    }
}

impl Project {
    pub(super) fn extract_piece(
        &mut self,
        sheet: SheetId,
        source: PieceSource,
        name: &str,
        detached: bool,
        out: &mut Outcome,
    ) -> Result<(), CoreError> {
        let name = name.trim().to_string();
        check_name(&name, "le nom de la pièce")?;
        let edges = match &source {
            PieceSource::Click { at } => {
                at.validate()?;
                self.region_boundary(sheet, *at)?
            }
            PieceSource::Paths { entities } => self.paths_boundary(sheet, entities)?,
        };
        if detached {
            self.detached_piece(sheet, &edges, &name, out)
        } else {
            self.linked_piece(sheet, edges, &name, out)
        }
    }

    /// Contour de la plus petite région fermée qui contient `at`.
    fn region_boundary(&self, sheet: SheetId, at: PointMm) -> Result<Vec<BEdge>, CoreError> {
        let s = self.sheet(sheet)?;
        let found = s.find_region_at(at).map_err(region_error)?;
        let mut out = Vec::new();
        for e in &found.region.edges {
            let entity = found.entities[e.source];
            let ent = s.entity(entity)?;
            let (circle, reversed) = match (&ent.kind, e.whole) {
                (
                    EntityKind::Circle {
                        center,
                        radius_mm,
                        ccw,
                        ..
                    },
                    true,
                ) => (Some((s.position(*center)?, *radius_mm, 0.0, true)), !*ccw),
                _ => (None, e.reversed),
            };
            out.push(BEdge {
                entity,
                reversed,
                part: (!e.whole).then_some(Part {
                    t_lo: e.t_lo,
                    t_hi: e.t_hi,
                    start: e.start,
                    end: e.end,
                }),
                geometry: e.geometry.clone(),
                circle,
                role: ent.role,
            });
        }
        Ok(out)
    }

    /// Contour formé exactement par ces tracés, orienté anti-horaire.
    fn paths_boundary(
        &self,
        sheet: SheetId,
        entities: &[EntityId],
    ) -> Result<Vec<BEdge>, CoreError> {
        let s = self.sheet(sheet)?;
        if entities.is_empty() {
            return Err(CoreError::Invalid("aucun tracé choisi".into()));
        }
        let contour = contour_from_entities(s, entities)?;
        let measure = measure_contour(s, &contour)?;
        let mut edges = contour.edges.clone();
        if measure.clockwise && edges.len() > 1 {
            edges.reverse();
            for e in &mut edges {
                e.reversed = !e.reversed;
            }
        }
        let mut out = Vec::new();
        for ce in edges {
            let ent = s.entity(ce.entity)?;
            let (geometry, circle) = match s.shape(ce.entity)? {
                Shape::Circle(ct) => (
                    trace_geometry(&Trace::Circle(ct.circle)),
                    Some((ct.circle.center, ct.circle.radius_mm, ct.start_deg, ct.ccw)),
                ),
                other => {
                    let g = trace_geometry(&other.to_trace());
                    let g = if ce.reversed {
                        g.iter().rev().map(EdgeGeom::reversed).collect()
                    } else {
                        g
                    };
                    (g, None)
                }
            };
            out.push(BEdge {
                entity: ce.entity,
                reversed: ce.reversed,
                part: None,
                geometry,
                circle,
                role: ent.role,
            });
        }
        Ok(out)
    }

    /// Point existant à moins de [`MERGE_TOLERANCE_MM`] de `pos`, sinon un point libre.
    fn cut_point_at(
        &mut self,
        sheet: SheetId,
        pos: PointMm,
        layer: Option<LayerId>,
        out: &mut Outcome,
    ) -> Result<PointId, CoreError> {
        let s = self.sheet(sheet)?;
        let mut best: Option<(PointId, f64)> = None;
        for p in &s.points {
            if let Ok(q) = s.position(p.id) {
                let d = pf_geometry::distance_mm(q, pos);
                if d <= MERGE_TOLERANCE_MM && best.is_none_or(|(_, bd)| d < bd) {
                    best = Some((p.id, d));
                }
            }
        }
        match best {
            Some((id, _)) => Ok(id),
            None => self.tangency_point(sheet, pos, layer, out),
        }
    }

    fn linked_piece(
        &mut self,
        sheet: SheetId,
        edges: Vec<BEdge>,
        name: &str,
        out: &mut Outcome,
    ) -> Result<(), CoreError> {
        // Coupes demandées, par tracé : au paramètre de chaque bout de portion.
        let mut cuts: BTreeMap<EntityId, Vec<Cut>> = BTreeMap::new();
        for e in &edges {
            let Some(p) = &e.part else { continue };
            let circle = matches!(
                self.sheet(sheet)?.entity(e.entity)?.kind,
                EntityKind::Circle { .. }
            );
            let (at_lo, at_hi) = if e.reversed {
                (p.end, p.start)
            } else {
                (p.start, p.end)
            };
            let list = cuts.entry(e.entity).or_default();
            for (t, pos) in [(p.t_lo, at_lo), (p.t_hi, at_hi)] {
                let t = if circle { t.rem_euclid(1.0) } else { t };
                list.push(Cut { t, pos });
            }
        }
        let mut maps: BTreeMap<EntityId, SubMap> = BTreeMap::new();
        for (entity, list) in cuts {
            let is_circle = matches!(
                self.sheet(sheet)?.entity(entity)?.kind,
                EntityKind::Circle { .. }
            );
            let map = if is_circle {
                self.cut_circle(sheet, entity, list, out)?
            } else {
                self.cut_open(sheet, entity, list, out)?
            };
            maps.insert(entity, map);
        }
        let mut outline = Vec::new();
        for e in &edges {
            let entity = match &e.part {
                None => e.entity,
                Some(p) => maps.get(&e.entity).and_then(|m| m.find(p)).ok_or_else(|| {
                    CoreError::Invalid(
                        "incohérence interne : une portion de contour ne retrouve pas son tronçon"
                            .into(),
                    )
                })?,
            };
            outline.push(PieceEdge {
                entity,
                reversed: e.reversed,
            });
        }
        let ids: Vec<EntityId> = outline.iter().map(|e| e.entity).collect();
        contour_from_entities(self.sheet(sheet)?, &ids).map_err(|e| {
            CoreError::Invalid(format!(
                "le contour trouvé n'est pas une chaîne de tracés raccordés par des points \
                 communs ({e})"
            ))
        })?;
        let id = PieceId(self.alloc_id());
        self.pieces.push(Piece {
            id,
            name: name.to_string(),
            outline,
            link: PieceLink::Linked { sheet },
            ..Piece::default()
        });
        out.pieces.push(id);
        Ok(())
    }

    /// Coupe un tracé ouvert (segment, arc, Bézier, courbe lisse à ses points de passage).
    fn cut_open(
        &mut self,
        sheet: SheetId,
        entity: EntityId,
        mut cuts: Vec<Cut>,
        out: &mut Outcome,
    ) -> Result<SubMap, CoreError> {
        let s = self.sheet(sheet)?;
        let shape = s.shape(entity)?;
        let layer = s.entity(entity)?.layer;
        let name = s.entity_name(entity);
        let (start, end) = shape_ends(&shape);
        let near = |a: PointMm, b: PointMm| pf_geometry::distance_mm(a, b) <= MERGE_TOLERANCE_MM;
        cuts.retain(|c| !near(c.pos, start) && !near(c.pos, end));
        cuts.sort_by(|a, b| a.t.total_cmp(&b.t));
        cuts.dedup_by(|a, b| near(a.pos, b.pos));
        if cuts.is_empty() {
            return Ok(SubMap::Whole(entity));
        }
        // Une courbe lisse ne se coupe qu'en un de ses points de passage.
        let is_spline = matches!(shape, Shape::Spline(_));
        let mut at: Vec<PointMm> = cuts.iter().map(|c| c.pos).collect();
        if let Shape::Spline(sp) = &shape {
            let nodes = s.spline_nodes(entity)?;
            let n = sp.segments().len() as f64;
            for (c, a) in cuts.iter().zip(at.iter_mut()) {
                let k = (c.t * n).round();
                if (c.t * n - k).abs() > 1e-6 || k < 1.0 || k as usize >= nodes.len() - 1 {
                    return Err(CoreError::Invalid(format!(
                        "{name} : la région est bordée par une partie de cette courbe lisse qui \
                         commence ou finit ailleurs qu'à un point de passage ; ajoutez-y un point \
                         de passage à cet endroit (une courbe lisse ne se coupe qu'en un point de \
                         passage)"
                    )));
                }
                *a = nodes[k as usize].position;
            }
        }
        let mut tail: Vec<EntityId> = Vec::new();
        for k in (0..cuts.len()).rev() {
            let upper = if k + 1 < cuts.len() {
                cuts[k + 1].t
            } else {
                1.0
            };
            let point = if is_spline {
                None
            } else {
                Some(self.cut_point_at(sheet, cuts[k].pos, layer, out)?)
            };
            let hint = SplitHint {
                t: Some(cuts[k].t / upper),
                point,
            };
            self.split_path(sheet, entity, at[k], hint, out)?;
            let created = out.entities.last().copied().ok_or_else(|| {
                CoreError::Invalid("incohérence interne : coupe sans nouveau tracé".into())
            })?;
            tail.push(created);
        }
        tail.reverse();
        let mut pieces = vec![entity];
        pieces.extend(tail);
        let mut bounds = vec![0.0];
        bounds.extend(cuts.iter().map(|c| c.t));
        bounds.push(1.0);
        Ok(SubMap::Open { bounds, pieces })
    }

    /// Un cercle coupé en `m ≥ 2` points devient `m` arcs (le premier garde l'identifiant).
    fn cut_circle(
        &mut self,
        sheet: SheetId,
        entity: EntityId,
        mut cuts: Vec<Cut>,
        out: &mut Outcome,
    ) -> Result<SubMap, CoreError> {
        let near = |a: PointMm, b: PointMm| pf_geometry::distance_mm(a, b) <= MERGE_TOLERANCE_MM;
        cuts.sort_by(|a, b| a.t.total_cmp(&b.t));
        cuts.dedup_by(|a, b| near(a.pos, b.pos));
        while cuts.len() > 1 && near(cuts[0].pos, cuts[cuts.len() - 1].pos) {
            cuts.pop();
        }
        if cuts.len() < 2 {
            return Ok(SubMap::Whole(entity));
        }
        let s = self.sheet(sheet)?;
        let name = s.entity_name(entity);
        if s.notches.iter().any(|n| n.entity == entity) {
            return Err(CoreError::Invalid(format!(
                "{name} porte des crans : un cercle à découper en arcs ne peut pas en porter \
                 (supprimez-les ou tracez des arcs)"
            )));
        }
        if let Some(p) = self
            .pieces
            .iter()
            .find(|p| p.outline.iter().any(|e| e.entity == entity))
        {
            return Err(CoreError::Invalid(format!(
                "{name} est déjà le contour de la pièce « {} » : il ne peut pas être découpé",
                p.name
            )));
        }
        let ent = s.entity(entity)?.clone();
        let EntityKind::Circle { center, .. } = ent.kind else {
            return Err(CoreError::Invalid(format!("{name} n'est pas un cercle")));
        };
        self.freeze_dependents(sheet, &[entity], out)?;
        let mut points = Vec::new();
        for c in &cuts {
            points.push(self.cut_point_at(sheet, c.pos, ent.layer, out)?);
        }
        let m = points.len();
        let mut arcs = Vec::new();
        for j in 0..m {
            let (a, b) = (points[j], points[(j + 1) % m]);
            if a == b {
                return Err(CoreError::Invalid(format!(
                    "{name} : deux coupes tombent sur le même point"
                )));
            }
            let s = self.sheet(sheet)?;
            pf_geometry::Arc::from_center_points(
                s.position(center)?,
                s.position(a)?,
                s.position(b)?,
                true,
            )?;
            let kind = EntityKind::Arc {
                center,
                start: a,
                end: b,
                ccw: true,
            };
            if j == 0 {
                self.sheet_mut(sheet)?.entity_mut(entity)?.kind = kind;
                arcs.push(entity);
            } else {
                let id = EntityId(self.alloc_id());
                let s = self.sheet_mut(sheet)?;
                let idx = s
                    .entities
                    .iter()
                    .position(|x| x.id == arcs[j - 1])
                    .ok_or(CoreError::UnknownEntity(arcs[j - 1]))?;
                s.entities.insert(
                    idx + 1,
                    Entity {
                        id,
                        role: ent.role,
                        layer: ent.layer,
                        name: None,
                        kind,
                    },
                );
                if let Some(g) = s.groups.iter_mut().find(|g| g.members.contains(&entity)) {
                    g.members.push(id);
                }
                out.entities.push(id);
                arcs.push(id);
            }
        }
        out.notes.push(format!(
            "{name} est découpé en {m} arcs pour former le contour de la pièce."
        ));
        Ok(SubMap::Circle {
            ts: cuts.iter().map(|c| c.t).collect(),
            arcs,
        })
    }

    /// Copie le contour dans une nouvelle feuille (pièce détachée).
    fn detached_piece(
        &mut self,
        source_sheet: SheetId,
        edges: &[BEdge],
        name: &str,
        out: &mut Outcome,
    ) -> Result<(), CoreError> {
        self.sheet(source_sheet)?;
        let taken: HashSet<String> = self.sheets.iter().map(|s| s.name.clone()).collect();
        let sheet_name = unique_name(name, &taken);
        let sid = SheetId(self.alloc_id());
        self.sheets.push(Sheet::new(sid, &sheet_name));
        out.sheet = Some(sid);
        let mut outline = Vec::new();
        for e in edges {
            let kinds: Vec<EntityKind> = match e.circle {
                Some((center, radius_mm, start_deg, ccw)) => {
                    let c = self.copy_point(sid, center)?;
                    vec![EntityKind::Circle {
                        center: c,
                        radius_mm,
                        start_deg,
                        ccw,
                    }]
                }
                None => {
                    let mut v = Vec::new();
                    for g in &e.geometry {
                        v.push(self.copy_geom(sid, g)?);
                    }
                    v
                }
            };
            for kind in kinds {
                let id = EntityId(self.alloc_id());
                self.sheet_mut(sid)?.entities.push(Entity {
                    id,
                    role: e.role,
                    layer: None,
                    name: None,
                    kind,
                });
                out.entities.push(id);
                outline.push(PieceEdge {
                    entity: id,
                    reversed: false,
                });
            }
        }
        let ids: Vec<EntityId> = outline.iter().map(|e| e.entity).collect();
        contour_from_entities(self.sheet(sid)?, &ids).map_err(|e| {
            CoreError::Invalid(format!(
                "la copie du contour n'est pas une chaîne fermée ({e})"
            ))
        })?;
        let id = PieceId(self.alloc_id());
        self.pieces.push(Piece {
            id,
            name: name.to_string(),
            outline,
            ..Piece::default()
        });
        out.pieces.push(id);
        Ok(())
    }

    /// Point libre de la feuille `sheet` en `pos` (réutilisé s'il existe à 1e-6 mm).
    fn copy_point(&mut self, sheet: SheetId, pos: PointMm) -> Result<PointId, CoreError> {
        pos.validate()?;
        let s = self.sheet(sheet)?;
        for p in &s.points {
            if let Ok(q) = s.position(p.id) {
                if pf_geometry::distance_mm(q, pos) <= MERGE_TOLERANCE_MM {
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
            definition: PointDefinition::Free { x: pos.x, y: pos.y },
            role: PointRole::Construction,
            layer: None,
        });
        Ok(id)
    }

    fn copy_geom(&mut self, sheet: SheetId, g: &EdgeGeom) -> Result<EntityKind, CoreError> {
        Ok(match g {
            EdgeGeom::Line(a, b) => {
                let (from, to) = (self.copy_point(sheet, *a)?, self.copy_point(sheet, *b)?);
                if from == to {
                    return Err(pf_geometry::GeometryError::ZeroLengthSegment.into());
                }
                EntityKind::Segment {
                    from,
                    to,
                    construction: None,
                }
            }
            EdgeGeom::Cubic(c) => EntityKind::Curve {
                start: self.copy_point(sheet, c.p0)?,
                handle_start: OffsetMm::from_vector(c.c1 - c.p0),
                handle_end: OffsetMm::from_vector(c.c2 - c.p3),
                end: self.copy_point(sheet, c.p3)?,
            },
            EdgeGeom::Arc(a) => {
                let center = self.copy_point(sheet, a.center)?;
                let (start, end) = (
                    self.copy_point(sheet, a.start_point())?,
                    self.copy_point(sheet, a.end_point())?,
                );
                if start == end || start == center || end == center {
                    return Err(CoreError::Invalid(
                        "arc dégénéré dans le contour à copier".into(),
                    ));
                }
                EntityKind::Arc {
                    center,
                    start,
                    end,
                    ccw: a.sweep_rad > 0.0,
                }
            }
        })
    }
}
