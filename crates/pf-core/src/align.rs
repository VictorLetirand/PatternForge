//! Aligner et répartir (US-02.9). Aucune nouvelle `Command` : chaque fonction
//! planifie une `Command::Batch` de `MovePoint` (alignement, répartition) ou de
//! `Transform` (mise à l'horizontale ou à la verticale) qui s'annule en une
//! seule étape. Le plan est vérifié sur une copie du projet : s'il échoue (point
//! construit, calque verrouillé, géométrie dégénérée, résultat inexact), rien
//! n'est déplacé. `Ok(None)` signifie « rien à faire » : aucune étape d'historique.

use crate::command::Command;
use crate::error::CoreError;
use crate::model::{EntityId, EntityKind, PointDefinition, PointId, Project, SheetId};
use pf_geometry::{distance_mm, PointMm, Transform, EPSILON_MM};

/// Alignement de points sur une droite.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AlignMode {
    /// Sur la verticale du point le plus à gauche de la sélection.
    Left,
    /// Sur la verticale du point le plus à droite.
    Right,
    /// Sur l'horizontale du point le plus haut (Y vers le haut).
    Top,
    /// Sur l'horizontale du point le plus bas.
    Bottom,
    /// Sur l'horizontale passant par le centre de la boîte de la sélection.
    CenterHorizontal,
    /// Sur la verticale passant par le centre de la boîte de la sélection.
    CenterVertical,
    /// Projection orthogonale sur une droite de référence (guide, segment).
    OnLine { origin: PointMm, angle_deg: f64 },
}

/// Répartition de points.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DistributeMode {
    /// Espacement égal en X entre les deux points extrêmes en X.
    Horizontal,
    /// Espacement égal en Y entre les deux points extrêmes en Y.
    Vertical,
    /// Espacement égal sur la droite joignant les deux points les plus éloignés.
    AlongLine,
}

impl Project {
    /// Plan d'alignement des `points` ; `Ok(None)` si moins de 2 points ou déjà alignés.
    pub fn plan_align(
        &self,
        sheet: SheetId,
        points: &[PointId],
        mode: AlignMode,
    ) -> Result<Option<Command>, CoreError> {
        let s = self.sheet(sheet)?;
        let ids = distinct(points);
        if ids.len() < 2 {
            return Ok(None);
        }
        let pos: Vec<PointMm> = ids
            .iter()
            .map(|&p| s.position(p))
            .collect::<Result<_, _>>()?;
        let min_x = pos.iter().map(|p| p.x).fold(f64::INFINITY, f64::min);
        let max_x = pos.iter().map(|p| p.x).fold(f64::NEG_INFINITY, f64::max);
        let min_y = pos.iter().map(|p| p.y).fold(f64::INFINITY, f64::min);
        let max_y = pos.iter().map(|p| p.y).fold(f64::NEG_INFINITY, f64::max);
        let line = match mode {
            AlignMode::OnLine { origin, angle_deg } => {
                if !angle_deg.is_finite() {
                    return Err(CoreError::Invalid(
                        "alignement : l'angle de la droite doit être un nombre fini".into(),
                    ));
                }
                origin.validate()?;
                let (sn, cs) = pf_geometry::sin_cos_deg(angle_deg);
                Some((origin, pf_geometry::VectorMm::new(cs, sn)))
            }
            _ => None,
        };
        let targets: Vec<(PointId, PointMm)> = ids
            .iter()
            .zip(&pos)
            .map(|(&id, &p)| {
                let t = match mode {
                    AlignMode::Left => PointMm::new(min_x, p.y),
                    AlignMode::Right => PointMm::new(max_x, p.y),
                    AlignMode::Top => PointMm::new(p.x, max_y),
                    AlignMode::Bottom => PointMm::new(p.x, min_y),
                    AlignMode::CenterHorizontal => PointMm::new(p.x, min_y + (max_y - min_y) / 2.0),
                    AlignMode::CenterVertical => PointMm::new(min_x + (max_x - min_x) / 2.0, p.y),
                    AlignMode::OnLine { .. } => {
                        let (o, u) = line.expect("droite définie");
                        o + u * u.dot(p - o)
                    }
                };
                (id, t)
            })
            .collect();
        self.plan_moves(sheet, targets)
    }

    /// Plan de répartition ; `Ok(None)` si moins de 3 points ou déjà répartis.
    pub fn plan_distribute(
        &self,
        sheet: SheetId,
        points: &[PointId],
        mode: DistributeMode,
    ) -> Result<Option<Command>, CoreError> {
        let s = self.sheet(sheet)?;
        let ids = distinct(points);
        if ids.len() < 3 {
            return Ok(None);
        }
        let mut items: Vec<(PointId, PointMm)> = ids
            .iter()
            .map(|&id| Ok((id, s.position(id)?)))
            .collect::<Result<_, CoreError>>()?;
        let n1 = (items.len() - 1) as f64;
        let mut targets = Vec::with_capacity(items.len());
        match mode {
            DistributeMode::Horizontal | DistributeMode::Vertical => {
                let horizontal = mode == DistributeMode::Horizontal;
                let key = |p: PointMm| if horizontal { p.x } else { p.y };
                items.sort_by(|a, b| key(a.1).total_cmp(&key(b.1)).then(a.0.cmp(&b.0)));
                let (lo, hi) = (key(items[0].1), key(items[items.len() - 1].1));
                for (i, &(id, p)) in items.iter().enumerate() {
                    let v = if i == 0 {
                        lo
                    } else if i + 1 == items.len() {
                        hi
                    } else {
                        lo + (hi - lo) * (i as f64) / n1
                    };
                    targets.push((
                        id,
                        if horizontal {
                            PointMm::new(v, p.y)
                        } else {
                            PointMm::new(p.x, v)
                        },
                    ));
                }
            }
            DistributeMode::AlongLine => {
                let (mut ia, mut ib, mut best) = (0, 1, -1.0);
                for i in 0..items.len() {
                    for j in i + 1..items.len() {
                        let d = distance_mm(items[i].1, items[j].1);
                        if d > best {
                            (ia, ib, best) = (i, j, d);
                        }
                    }
                }
                if best <= EPSILON_MM {
                    return Err(CoreError::Invalid(
                        "répartition impossible : les points sont tous confondus".into(),
                    ));
                }
                let (a, b) = (items[ia].1, items[ib].1);
                let u = (b - a).unit()?;
                items.sort_by(|x, y| {
                    u.dot(x.1 - a)
                        .total_cmp(&u.dot(y.1 - a))
                        .then(x.0.cmp(&y.0))
                });
                for (i, &(id, _)) in items.iter().enumerate() {
                    let t = if i == 0 {
                        a
                    } else if i + 1 == items.len() {
                        b
                    } else {
                        a.lerp(b, (i as f64) / n1)
                    };
                    targets.push((id, t));
                }
            }
        }
        self.plan_moves(sheet, targets)
    }

    /// Plan de mise à l'horizontale (`horizontal`) ou à la verticale de segments.
    ///
    /// Sans `scope`, chaque segment pivote avec tout ce qui lui est attaché (points
    /// connectés par des tracés ou des points partagés), autour de `pivot` ou, à
    /// défaut, de son point de départ. Avec `scope` (sélection de plusieurs objets),
    /// seul le premier segment sert de référence et `scope` pivote en bloc.
    /// Refus si un segment est dégénéré ou si le résultat ne les met pas tous d'aplomb.
    pub fn plan_level(
        &self,
        sheet: SheetId,
        segments: &[EntityId],
        horizontal: bool,
        pivot: Option<PointMm>,
        scope: Option<&[PointId]>,
    ) -> Result<Option<Command>, CoreError> {
        if segments.is_empty() {
            return Err(CoreError::Invalid("aucun segment à mettre d'aplomb".into()));
        }
        if let Some(p) = pivot {
            p.validate()?;
        }
        let segments = if scope.is_some() {
            &segments[..1]
        } else {
            segments
        };
        let mut scratch = self.clone();
        let mut commands = Vec::new();
        for &seg in segments {
            let s = scratch.sheet(sheet)?;
            let (from, to) = segment_ends(s, seg)?;
            let (a, b) = (s.position(from)?, s.position(to)?);
            let (dx, dy) = (b.x - a.x, b.y - a.y);
            if distance_mm(a, b) <= EPSILON_MM {
                return Err(CoreError::Invalid(format!(
                    "{} est dégénéré (longueur nulle) : impossible de le mettre {}",
                    s.entity_name(seg),
                    if horizontal {
                        "à l'horizontale"
                    } else {
                        "à la verticale"
                    }
                )));
            }
            let off = if horizontal { dy } else { dx };
            if off.abs() <= EPSILON_MM {
                continue;
            }
            let theta = dy.atan2(dx).to_degrees();
            let m = (if horizontal { theta } else { theta - 90.0 }).rem_euclid(180.0);
            let phi = if m > 90.0 { m - 180.0 } else { m };
            let points = match scope {
                Some(p) => distinct(p),
                None => attached_points(s, from, to),
            };
            let cmd = Command::Transform {
                sheet,
                points,
                transform: Transform::Rotate {
                    center: pivot.unwrap_or(a),
                    angle_deg: -phi,
                },
            };
            scratch.apply(cmd.clone())?;
            commands.push(cmd);
        }
        if commands.is_empty() {
            return Ok(None);
        }
        let s = scratch.sheet(sheet)?;
        for &seg in segments {
            let (from, to) = segment_ends(s, seg)?;
            let (a, b) = (s.position(from)?, s.position(to)?);
            let off = if horizontal { b.y - a.y } else { b.x - a.x };
            if off.abs() > EPSILON_MM {
                return Err(CoreError::Invalid(format!(
                    "{} ne peut pas être mis {} : il est lié rigidement à un autre objet \
                     (autre segment, point construit)",
                    s.entity_name(seg),
                    if horizontal {
                        "à l'horizontale"
                    } else {
                        "à la verticale"
                    }
                )));
            }
        }
        Ok(Some(Command::Batch { commands }))
    }

    /// Déplacements `(point, cible)` : refus listant les points construits qui
    /// devraient bouger ; vérifie sur une copie que chaque point finit à sa cible.
    fn plan_moves(
        &self,
        sheet: SheetId,
        targets: Vec<(PointId, PointMm)>,
    ) -> Result<Option<Command>, CoreError> {
        let s = self.sheet(sheet)?;
        let mut moves = Vec::new();
        let mut fixed = Vec::new();
        for &(id, t) in &targets {
            t.validate()?;
            if distance_mm(s.position(id)?, t) > EPSILON_MM {
                if matches!(s.point(id)?.definition, PointDefinition::Free { .. }) {
                    moves.push((id, t));
                } else {
                    fixed.push(s.label(id).to_string());
                }
            }
        }
        if !fixed.is_empty() {
            return Err(CoreError::Invalid(format!(
                "les points construits ne se déplacent pas librement : {} (aucun point déplacé)",
                fixed.join(", ")
            )));
        }
        if moves.is_empty() {
            return Ok(None);
        }
        let cmd = Command::Batch {
            commands: moves
                .iter()
                .map(|&(point, to)| Command::MovePoint { sheet, point, to })
                .collect(),
        };
        let mut scratch = self.clone();
        scratch.apply(cmd.clone())?;
        let after = scratch.sheet(sheet)?;
        for &(id, t) in &targets {
            if distance_mm(after.position(id)?, t) > EPSILON_MM {
                return Err(CoreError::Invalid(format!(
                    "le point {} ne peut pas atteindre sa position : il suit un arc ou un \
                     point construit qui bouge (aucun point déplacé)",
                    after.label(id)
                )));
            }
        }
        Ok(Some(cmd))
    }
}

fn distinct(points: &[PointId]) -> Vec<PointId> {
    let mut v = points.to_vec();
    v.sort();
    v.dedup();
    v
}

fn segment_ends(s: &crate::model::Sheet, id: EntityId) -> Result<(PointId, PointId), CoreError> {
    match s.entity(id)?.kind {
        EntityKind::Segment { from, to, .. } => Ok((from, to)),
        _ => Err(CoreError::Invalid(format!(
            "{} n'est pas un segment",
            s.entity_name(id)
        ))),
    }
}

/// Composante connexe des points atteints depuis les extrémités d'un segment
/// par les tracés qui partagent des points.
fn attached_points(s: &crate::model::Sheet, from: PointId, to: PointId) -> Vec<PointId> {
    let mut seen = vec![from, to];
    let mut grown = true;
    while grown {
        grown = false;
        for e in &s.entities {
            let pts = e.kind.all_points();
            if pts.iter().any(|p| seen.contains(p)) {
                for p in pts {
                    if !seen.contains(&p) {
                        seen.push(p);
                        grown = true;
                    }
                }
            }
        }
    }
    distinct(&seen)
}
