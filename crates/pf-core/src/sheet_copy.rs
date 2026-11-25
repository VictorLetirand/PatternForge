//! Duplication et import de feuille (US-01.4) : tous les identifiants de la
//! copie sont régénérés dans le projet cible et les liens internes sont
//! réécrits. Aucun identifiant de la source n'est conservé.
//!
//! La réécriture des objets d'une feuille ([`rewrite_content`]) est aussi
//! utilisée par le collage du presse-papiers (US-01.6).

use crate::command::Outcome;
use crate::error::CoreError;
use crate::model::*;
use crate::piece_grain::GrainDirection;
use crate::piece_marking::MarkingAnchor;
use std::collections::{BTreeMap, HashSet};

/// Correspondance ancien identifiant → nouvel identifiant (les identifiants
/// de tous les types partagent le même compteur : une seule table suffit).
pub(crate) struct IdMap(pub(crate) BTreeMap<u64, u64>);

impl IdMap {
    pub(crate) fn get(&self, old: u64) -> u64 {
        // Une référence morte (provenance d'une construction) est conservée telle quelle.
        self.0.get(&old).copied().unwrap_or(old)
    }
    pub(crate) fn strict(&self, old: u64, what: &str) -> Result<u64, CoreError> {
        self.0.get(&old).copied().ok_or_else(|| {
            CoreError::DeadReference(format!("{what} référence un objet absent (id {old})"))
        })
    }
}

/// Premier nom libre : `base`, puis `base (2)`, `base (3)`…
pub(crate) fn unique_name(base: &str, taken: &HashSet<String>) -> String {
    if !taken.contains(base) {
        return base.to_string();
    }
    (2u32..)
        .map(|n| format!("{base} ({n})"))
        .find(|c| !taken.contains(c))
        .expect("nom libre")
}

/// Objets d'une feuille réécrits avec leurs nouveaux identifiants.
pub(crate) struct Rewritten {
    pub points: Vec<Point>,
    pub entities: Vec<Entity>,
    pub notches: Vec<Notch>,
    pub dimensions: Vec<Dimension>,
    pub full_dimensions: Vec<crate::full_dimension::FullDimension>,
    pub groups: Vec<Group>,
    pub guides: Vec<Guide>,
    pub formulas: Vec<crate::formula::ParamFormula>,
    pub length_constraints: Vec<crate::length_constraint::CurveLength>,
    pub gradings: Vec<crate::grading::PointGrading>,
}

/// Réécrit points, tracés, crans, cotations et groupes de `source` d'après
/// `map` ; toute référence interne absente de la table est refusée.
pub(crate) fn rewrite_content(map: &IdMap, source: &Sheet) -> Result<Rewritten, CoreError> {
    // Toute référence interne doit exister dans la source.
    let layer = |l: Option<LayerId>| -> Result<Option<LayerId>, CoreError> {
        l.map(|l| map.strict(l.0, "un objet").map(LayerId))
            .transpose()
    };
    let pt = |p: PointId| map.strict(p.0, "un tracé").map(PointId);
    let en = |e: EntityId| map.strict(e.0, "un objet").map(EntityId);
    let mut out = Rewritten {
        points: Vec::new(),
        entities: Vec::new(),
        notches: Vec::new(),
        dimensions: Vec::new(),
        full_dimensions: Vec::new(),
        groups: Vec::new(),
        guides: Vec::new(),
        formulas: Vec::new(),
        length_constraints: Vec::new(),
        gradings: Vec::new(),
    };
    for g in &source.gradings {
        out.gradings.push(crate::grading::PointGrading {
            point: pt(g.point)?,
            ..*g
        });
    }
    for c in &source.length_constraints {
        use crate::length_constraint::LengthAdjust;
        let on_this_sheet = |sheet: Option<SheetId>| sheet.is_none_or(|id| id == source.id);
        out.length_constraints
            .push(crate::length_constraint::CurveLength {
                curve: en(c.curve)?,
                length_mm: c.length_mm,
                adjust: match c.adjust {
                    LengthAdjust::Handle { point } => LengthAdjust::Handle { point: pt(point)? },
                    LengthAdjust::Slide { point, angle_deg } => LengthAdjust::Slide {
                        point: pt(point)?,
                        angle_deg,
                    },
                },
                reference: c
                    .reference
                    .map(|r| {
                        Ok::<_, CoreError>(crate::length_constraint::LengthRef {
                            entity: if on_this_sheet(c.link.as_ref().and_then(|l| l.sheet)) {
                                en(r.entity)?
                            } else {
                                r.entity
                            },
                            offset_mm: r.offset_mm,
                        })
                    })
                    .transpose()?,
                expr: c.expr.clone(),
                // Égalité étendue (US-03.4b) : les tracés de la feuille suivent la copie ; ceux
                // d'une autre feuille gardent leur identifiant (la copie reste dans le même projet,
                // une importation les délie plus loin).
                link: c
                    .link
                    .as_ref()
                    .map(|l| {
                        Ok::<_, CoreError>(crate::length_constraint::LengthLink {
                            sheet: l.sheet,
                            more: l
                                .more
                                .iter()
                                .map(|t| {
                                    Ok(crate::length_constraint::LengthTerm {
                                        entity: if on_this_sheet(t.sheet) {
                                            en(t.entity)?
                                        } else {
                                            t.entity
                                        },
                                        sheet: t.sheet,
                                    })
                                })
                                .collect::<Result<Vec<_>, CoreError>>()?,
                            offset_expr: l.offset_expr.clone(),
                        })
                    })
                    .transpose()?,
            });
    }
    for f in &source.formulas {
        out.formulas.push(crate::formula::ParamFormula {
            point: pt(f.point)?,
            ..f.clone()
        });
    }
    for p in &source.points {
        let mut np = p.clone();
        np.id = PointId(map.get(p.id.0));
        np.layer = layer(p.layer)?;
        np.definition = match p.definition {
            PointDefinition::Free { .. } => p.definition,
            PointDefinition::OnSegment {
                segment,
                distance_mm,
                origin,
            } => PointDefinition::OnSegment {
                segment: en(segment)?,
                distance_mm,
                origin,
            },
            PointDefinition::OnSegmentRatio { segment, ratio } => PointDefinition::OnSegmentRatio {
                segment: en(segment)?,
                ratio,
            },
            PointDefinition::Intersection { first, second } => PointDefinition::Intersection {
                first: en(first)?,
                second: en(second)?,
            },
            PointDefinition::OnCurve { curve, t } => PointDefinition::OnCurve {
                curve: en(curve)?,
                t,
            },
            PointDefinition::CircleIntersection {
                first,
                second,
                near,
            } => PointDefinition::CircleIntersection {
                first: en(first)?,
                second: en(second)?,
                near,
            },
            PointDefinition::TraceIntersection {
                first,
                second,
                near,
            } => PointDefinition::TraceIntersection {
                first: en(first)?,
                second: en(second)?,
                near,
            },
            PointDefinition::AlongCurve {
                curve,
                distance_mm,
                origin,
            } => PointDefinition::AlongCurve {
                curve: en(curve)?,
                distance_mm,
                origin,
            },
            PointDefinition::Offset { from, dx_mm, dy_mm } => PointDefinition::Offset {
                from: pt(from)?,
                dx_mm,
                dy_mm,
            },
            PointDefinition::Midpoint { first, second } => PointDefinition::Midpoint {
                first: pt(first)?,
                second: pt(second)?,
            },
            PointDefinition::Projection { point, trace } => PointDefinition::Projection {
                point: pt(point)?,
                trace: en(trace)?,
            },
            PointDefinition::Bisector {
                vertex,
                first,
                second,
                distance_mm,
            } => PointDefinition::Bisector {
                vertex: pt(vertex)?,
                first: pt(first)?,
                second: pt(second)?,
                distance_mm,
            },
            PointDefinition::Polar {
                from,
                distance_mm,
                angle_deg,
                reference,
            } => PointDefinition::Polar {
                from: pt(from)?,
                distance_mm,
                angle_deg,
                reference: reference.map(en).transpose()?,
            },
            PointDefinition::Compass {
                center_a,
                center_b,
                radius_a_mm,
                radius_b_mm,
                near,
            } => PointDefinition::Compass {
                center_a: pt(center_a)?,
                center_b: pt(center_b)?,
                radius_a_mm,
                radius_b_mm,
                near,
            },
            PointDefinition::Division {
                trace,
                ratio,
                start,
            } => PointDefinition::Division {
                trace: en(trace)?,
                ratio,
                start: start.map(pt).transpose()?,
            },
            PointDefinition::Mirror { source, axis } => PointDefinition::Mirror {
                source: pt(source)?,
                axis: match axis {
                    MirrorAxis::Guide { guide } => MirrorAxis::Guide {
                        guide: map.strict(guide.0, "un point miroir").map(GuideId)?,
                    },
                    MirrorAxis::Segment { segment } => MirrorAxis::Segment {
                        segment: en(segment)?,
                    },
                },
            },
        };
        out.points.push(np);
    }
    for e in &source.entities {
        let mut ne = e.clone();
        ne.id = EntityId(map.get(e.id.0));
        ne.layer = layer(e.layer)?;
        ne.kind = match e.kind {
            EntityKind::Segment {
                from,
                to,
                construction,
            } => EntityKind::Segment {
                from: pt(from)?,
                to: pt(to)?,
                construction: construction.map(|c| match c {
                    Construction::Polar { .. } => c,
                    Construction::Perpendicular {
                        reference,
                        length_mm,
                        side,
                        kept,
                    } => Construction::Perpendicular {
                        reference: EntityId(map.get(reference.0)),
                        length_mm,
                        side,
                        kept,
                    },
                    Construction::Parallel {
                        reference,
                        distance_mm,
                        side,
                        kept,
                    } => Construction::Parallel {
                        reference: EntityId(map.get(reference.0)),
                        distance_mm,
                        side,
                        kept,
                    },
                }),
            },
            EntityKind::Curve {
                start,
                handle_start,
                handle_end,
                end,
            } => EntityKind::Curve {
                start: pt(start)?,
                handle_start,
                handle_end,
                end: pt(end)?,
            },
            EntityKind::Circle {
                center,
                radius_mm,
                start_deg,
                ccw,
            } => EntityKind::Circle {
                center: pt(center)?,
                radius_mm,
                start_deg,
                ccw,
            },
            EntityKind::Arc {
                center,
                start,
                end,
                ccw,
            } => EntityKind::Arc {
                center: pt(center)?,
                start: pt(start)?,
                end: pt(end)?,
                ccw,
            },
            EntityKind::Spline { ref nodes } => EntityKind::Spline {
                nodes: nodes
                    .iter()
                    .map(|n| {
                        Ok(SplineNode {
                            point: pt(n.point)?,
                            ..*n
                        })
                    })
                    .collect::<Result<_, CoreError>>()?,
            },
        };
        out.entities.push(ne);
    }
    for n in &source.notches {
        let mut nn = n.clone();
        nn.id = NotchId(map.get(n.id.0));
        nn.entity = en(n.entity)?;
        nn.anchor = match n.anchor {
            Some(NotchAnchor::Point { point }) => Some(NotchAnchor::Point { point: pt(point)? }),
            Some(NotchAnchor::Carried {
                entity,
                point,
                from_end,
            }) => Some(NotchAnchor::Carried {
                entity: en(entity)?,
                point: pt(point)?,
                from_end,
            }),
            other => other,
        };
        out.notches.push(nn);
    }
    for d in &source.dimensions {
        let mut nd = d.clone();
        nd.id = DimensionId(map.get(d.id.0));
        nd.from = pt(d.from)?;
        nd.to = pt(d.to)?;
        out.dimensions.push(nd);
    }
    for d in &source.full_dimensions {
        let mut nd = d.clone();
        nd.id = FullDimensionId(map.get(d.id.0));
        nd.kind = d.kind.remap(&pt, &en)?;
        out.full_dimensions.push(nd);
    }
    for g in &source.groups {
        let members = g
            .members
            .iter()
            .map(|&m| en(m))
            .collect::<Result<Vec<_>, _>>()?;
        out.groups.push(Group {
            id: GroupId(map.get(g.id.0)),
            members,
        });
    }
    for g in &source.guides {
        let mut ng = g.clone();
        ng.id = GuideId(map.get(g.id.0));
        ng.layer = layer(g.layer)?;
        out.guides.push(ng);
    }
    Ok(out)
}

impl Project {
    /// Alloue un nouvel identifiant pour chaque objet de `source` (hors la
    /// feuille elle-même) et l'inscrit dans `map`.
    pub(crate) fn fresh_ids(&mut self, source: &Sheet, map: &mut IdMap) {
        let mut fresh = |p: &mut Project, old: u64| {
            let new = p.alloc_id();
            map.0.insert(old, new);
        };
        for p in &source.points {
            fresh(self, p.id.0);
        }
        for e in &source.entities {
            fresh(self, e.id.0);
        }
        for n in &source.notches {
            fresh(self, n.id.0);
        }
        for d in &source.dimensions {
            fresh(self, d.id.0);
        }
        for d in &source.full_dimensions {
            fresh(self, d.id.0);
        }
        for l in &source.layers {
            fresh(self, l.id.0);
        }
        for g in &source.groups {
            fresh(self, g.id.0);
        }
        for g in &source.guides {
            fresh(self, g.id.0);
        }
    }

    /// Commande qui importe, dans un autre projet, une feuille de celui-ci
    /// (avec ses pièces liées). Ce projet n'est pas modifié.
    pub fn import_command(&self, sheet: SheetId) -> Result<crate::command::Command, CoreError> {
        Ok(crate::command::Command::ImportSheet {
            sheet: Box::new(self.sheet(sheet)?.clone()),
            pieces: self.pieces_of(sheet).cloned().collect(),
        })
    }

    /// Insère une copie de `source` (et de ses pièces liées `pieces`) avec de
    /// nouveaux identifiants, à l'index `index` ; renvoie l'identifiant de la
    /// nouvelle feuille. `copy_suffix` : le nom devient « X (copie) » (unique).
    pub(crate) fn insert_sheet_copy(
        &mut self,
        source: &Sheet,
        pieces: &[Piece],
        index: usize,
        copy_suffix: bool,
        out: &mut Outcome,
    ) -> Result<SheetId, CoreError> {
        let mut map = IdMap(BTreeMap::new());
        let new_sheet_id = self.alloc_id();
        map.0.insert(source.id.0, new_sheet_id);
        self.fresh_ids(source, &mut map);

        let mut base = source.name.trim().to_string();
        if copy_suffix {
            base = format!("{base} (copie)");
        }
        let taken: HashSet<String> = self.sheets.iter().map(|s| s.name.clone()).collect();
        let mut sheet = Sheet {
            id: SheetId(new_sheet_id),
            name: unique_name(&base, &taken),
            points: Vec::new(),
            entities: Vec::new(),
            notches: Vec::new(),
            dimensions: Vec::new(),
            layers: source.layers.clone(),
            groups: Vec::new(),
            guides: Vec::new(),
            // Le lien d'une surpiqûre n'est jamais copié (US-06.8, comme les pièces
            // dérivées) : ses lignes deviennent des tracés libres dans la copie.
            topstitches: Vec::new(),
            full_dimensions: Vec::new(),
            formulas: Vec::new(),
            length_constraints: Vec::new(),
            gradings: Vec::new(),
            background: source.background.clone(),
            extra: source.extra.clone(),
        };
        if !source.topstitches.is_empty() {
            out.notes.push(format!(
                "{} surpiqûre(s) liée(s) de la feuille « {} » ne sont pas reprises : leurs lignes                  sont des tracés libres dans la copie.",
                source.topstitches.len(),
                source.name
            ));
        }
        for l in &mut sheet.layers {
            l.id = LayerId(map.get(l.id.0));
        }
        let content = rewrite_content(&map, source)?;
        sheet.points = content.points;
        sheet.entities = content.entities;
        sheet.notches = content.notches;
        sheet.dimensions = content.dimensions;
        sheet.full_dimensions = content.full_dimensions;
        sheet.groups = content.groups;
        sheet.guides = content.guides;
        // Formules maintenues (US-03.1b) : le lien suit la copie (points remappés) ; une formule
        // qui ne s'évalue pas dans ce projet (feuille importée d'un autre) est retirée, avec
        // remarque, et la valeur reste en nombre figé.
        sheet.formulas = content.formulas;
        self.retain_adoptable_formulas(&mut sheet, out);
        // Longueurs imposées (US-03.3) : le lien suit la copie (identifiants remappés) ; la
        // géométrie est identique, l'invariant de longueur reste vrai.
        sheet.length_constraints = content.length_constraints;
        if !copy_suffix {
            Project::drop_foreign_length_links(&mut sheet, out);
        }
        self.retain_adoptable_length_formulas(&mut sheet, out);
        // Écarts de gradation (US-09.2a) : une copie dans le même projet les garde (points
        // remappés, mêmes tailles). Une feuille importée d'un autre projet ne les garde pas : ses
        // identifiants de taille ne désignent rien ici (remarque).
        if copy_suffix {
            sheet.gradings = content.gradings;
        } else if !content.gradings.is_empty() {
            out.notes.push(format!(
                "{} écart(s) de gradation de la feuille « {} » ne sont pas repris : les tailles \
                 d'un autre projet n'existent pas ici.",
                content.gradings.len(),
                source.name
            ));
        }

        let mut new_pieces = Vec::new();
        for piece in pieces {
            let mut np = piece.clone();
            if self
                .seams
                .iter()
                .any(|s| s.a.piece == piece.id || s.b.piece == piece.id)
            {
                out.notes.push(format!(
                    "Les coutures appariées de la pièce « {} » ne sont pas reprises dans la copie.",
                    piece.name
                ));
            }
            if self
                .cutting_plans
                .iter()
                .any(|c| c.placements.iter().any(|p| p.piece == piece.id))
            {
                out.notes.push(format!(
                    "Les placements des plans de coupe de la pièce « {} » ne sont pas repris dans la copie.",
                    piece.name
                ));
            }
            np.id = PieceId(self.alloc_id());
            np.link = PieceLink::Linked { sheet: sheet.id };
            // La copie revient au placement automatique (US-04.5, décision 130) :
            // jamais superposée exactement à l'original.
            np.placement = None;
            // Le lien de dérivation n'est jamais copié (décision 125) : la copie
            // est une pièce ordinaire.
            if np.derived.take().is_some() {
                out.notes.push(format!(
                    "La copie de la pièce « {} » n'est plus liée à sa pièce mère.",
                    np.name
                ));
            }
            for edge in &mut np.outline {
                edge.entity = EntityId(map.strict(edge.entity.0, "le contour d'une pièce")?);
            }
            np.grainline = np
                .grainline
                .map(|g| map.strict(g.0, "le droit-fil d'une pièce").map(EntityId))
                .transpose()?;
            // Droit-fil avancé (US-06.5) : tracés des lignes et références remappés.
            if let Some(g) = np.grain.as_mut() {
                let what = "le droit-fil avancé d'une pièce";
                let ent = |e: EntityId| map.strict(e.0, what).map(EntityId);
                let dir = |d: GrainDirection| -> Result<GrainDirection, CoreError> {
                    Ok(match d {
                        GrainDirection::Parallel { reference } => GrainDirection::Parallel {
                            reference: ent(reference)?,
                        },
                        GrainDirection::Perpendicular { reference } => {
                            GrainDirection::Perpendicular {
                                reference: ent(reference)?,
                            }
                        }
                        other => other,
                    })
                };
                g.direction = dir(g.direction)?;
                for e in &mut g.extra {
                    e.entity = ent(e.entity)?;
                    e.direction = dir(e.direction)?;
                }
            }
            // Marge de couture (US-05.1) : tracés des surcharges remappés.
            if let Some(s) = np.seam.as_mut() {
                for e in &mut s.edges {
                    e.entity = EntityId(map.strict(e.entity.0, "la marge de couture d'une pièce")?);
                }
                for h in &mut s.hems {
                    h.entity = EntityId(map.strict(h.entity.0, "un ourlet de pièce")?);
                }
                for f in &mut s.folds {
                    f.entity = EntityId(map.strict(f.entity.0, "un rempli de pièce")?);
                }
                for c in &mut s.corners {
                    c.entity = EntityId(map.strict(c.entity.0, "un type de coin d'une pièce")?);
                }
            }
            // Bords sur le pli (PH-5b) : tracés remappés (ordre trié conservé par le remappage
            // monotone ; retrié par sûreté).
            for e in &mut np.on_fold {
                *e = EntityId(map.strict(e.0, "un bord sur le pli d'une pièce")?);
            }
            np.on_fold.sort();
            for el in &mut np.internal {
                let what = "un élément interne de pièce";
                el.target = match el.target {
                    InternalTarget::Path { entity } => InternalTarget::Path {
                        entity: EntityId(map.strict(entity.0, what)?),
                    },
                    InternalTarget::Point { point } => InternalTarget::Point {
                        point: PointId(map.strict(point.0, what)?),
                    },
                    InternalTarget::Notch { notch } => InternalTarget::Notch {
                        notch: NotchId(map.strict(notch.0, what)?),
                    },
                };
            }
            // Marquages (US-06.3) : points remappés, nouveaux identifiants.
            for m in &mut np.markings {
                let what = "un marquage de pièce";
                let pt = |p: PointId| map.strict(p.0, what).map(PointId);
                m.at = match m.at {
                    MarkingAnchor::Point { point } => MarkingAnchor::Point { point: pt(point)? },
                    MarkingAnchor::DartTip {
                        tip,
                        toward,
                        setback_mm,
                    } => MarkingAnchor::DartTip {
                        tip: pt(tip)?,
                        toward: pt(toward)?,
                        setback_mm,
                    },
                };
                m.id = MarkingId(self.alloc_id());
            }
            // Pinces (US-07.1) : points remappés, nouveaux identifiants.
            for d in &mut np.darts {
                let what = "une pince de pièce";
                d.apex = PointId(map.strict(d.apex.0, what)?);
                d.toward = PointId(map.strict(d.toward.0, what)?);
                d.id = DartId(self.alloc_id());
            }
            // Plis (US-07.3) : coordonnées absolues, nouveaux identifiants.
            for l in &mut np.pleats {
                l.id = PleatId(self.alloc_id());
            }
            // Lignes d'ampleur (US-07.4) : coordonnées absolues, nouveaux identifiants.
            for l in &mut np.flares {
                l.id = FlareId(self.alloc_id());
            }
            // Groupes de boutons (US-06.4) : points remappés, nouveaux identifiants.
            for g in &mut np.buttons {
                let what = "un groupe de boutons de pièce";
                g.from = PointId(map.strict(g.from.0, what)?);
                g.to = PointId(map.strict(g.to.0, what)?);
                g.id = ButtonGroupId(self.alloc_id());
            }
            // Lignes de piqûre (US-06.9) : tracés remappés, nouveaux identifiants (les
            // continuations suivent).
            let renamed: Vec<(StitchLineId, StitchLineId)> = np
                .stitch_lines
                .iter()
                .map(|l| (l.id, StitchLineId(self.alloc_id())))
                .collect();
            for l in &mut np.stitch_lines {
                for e in &mut l.edges {
                    *e = EntityId(map.strict(e.0, "une ligne de piqûre de pièce")?);
                }
                l.continues = l
                    .continues
                    .and_then(|c| renamed.iter().find(|(old, _)| *old == c).map(|(_, n)| *n));
                if let Some((_, new)) = renamed.iter().find(|(old, _)| *old == l.id) {
                    l.id = *new;
                }
            }
            // Zones (US-06.10) : tracés des bandes remappés, nouveaux identifiants.
            for z in &mut np.zones {
                if let crate::piece_zone::ZoneShape::Band { edges, .. } = &mut z.shape {
                    for e in edges.iter_mut() {
                        *e = EntityId(map.strict(e.0, "une zone de pièce")?);
                    }
                }
                z.id = ZoneId(self.alloc_id());
            }
            new_pieces.push(np);
        }

        out.sheet = Some(sheet.id);
        out.points = sheet.points.iter().map(|p| p.id).collect();
        out.entities = sheet.entities.iter().map(|e| e.id).collect();
        out.notches = sheet.notches.iter().map(|n| n.id).collect();
        out.dimensions = sheet.dimensions.iter().map(|d| d.id).collect();
        out.full_dimensions = sheet.full_dimensions.iter().map(|d| d.id).collect();
        out.layers = sheet.layers.iter().map(|l| l.id).collect();
        out.groups = sheet.groups.iter().map(|g| g.id).collect();
        out.guides = sheet.guides.iter().map(|g| g.id).collect();
        out.pieces = new_pieces.iter().map(|p| p.id).collect();
        let id = sheet.id;
        let index = index.min(self.sheets.len());
        self.sheets.insert(index, sheet);
        self.pieces.extend(new_pieces);
        Ok(id)
    }
}
