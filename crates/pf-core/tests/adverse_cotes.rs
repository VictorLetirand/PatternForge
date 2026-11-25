//! Campagne adverse : cotes (graine fixe).

mod adverse_support;
use adverse_support::*;

use pf_core::command::{Command, PieceSource, PointRef};
use pf_core::model::*;
use pf_core::{persist, Document, Project};
use pf_geometry::{PointMm, Transform};
use std::collections::BTreeMap;
use std::panic::{catch_unwind, AssertUnwindSafe};

use pf_core::FullDimensionKind;

// ---------------------------------------------------------------------------
// US-06.7 — cotations complètes : longueur développée, angle, rayon, diamètre, projections
// ---------------------------------------------------------------------------

#[derive(Default)]
struct CoteStats {
    accepted: usize,
    refused: usize,
    noops: usize,
    undone: usize,
    reloaded: usize,
    exports: usize,
    created: BTreeMap<String, usize>,
    oracle_checked: usize,
    oracle_undefined: usize,
    oracle_gray: usize,
    model_err: usize,
    pruned_notes: usize,
    must_refuse: usize,
    must_accept: usize,
    locked_refusals: usize,
    copies: usize,
    pastes: usize,
    splits: usize,
}

fn cs_offset(r: &mut Rng) -> f64 {
    match r.below(12) {
        0..=8 => r.unit() * 60.0 - 30.0,
        9 => 0.0,
        _ => [f64::NAN, f64::INFINITY, 1e9, -1e9, 10_000.0, 10_000.5][r.below(6)],
    }
}

fn cs_offset_ok(v: f64) -> bool {
    v.is_finite() && v.abs() <= 10_000.0
}

fn cs_arc_radius(r: &mut Rng) -> f64 {
    match r.below(12) {
        0..=8 => 1.0 + r.unit() * 60.0,
        9 => 1.0,
        _ => [0.0, 0.5, f64::NAN, 10_000.0, 10_001.0, -5.0][r.below(6)],
    }
}

fn cs_arc_radius_ok(v: f64) -> bool {
    v.is_finite() && (1.0..=10_000.0).contains(&v)
}

fn cs_direction(r: &mut Rng) -> f64 {
    match r.below(12) {
        0..=9 => r.unit() * 720.0 - 360.0,
        _ => [f64::NAN, f64::INFINITY, 360.0, 0.0][r.below(4)],
    }
}

fn cs_label(r: &mut Rng) -> Option<String> {
    match r.below(12) {
        0..=2 => None,
        3..=8 => Some(
            [
                "Épaule",
                "Tour de taille",
                "Œil — côté",
                "  Ourlet  ",
                "Ø 12",
                "Łódź",
                "",
            ][r.below(7)]
            .to_string(),
        ),
        9 => Some("x".repeat(41)),
        10 => Some("é".repeat(40)),
        _ => Some("a\nb".to_string()),
    }
}

fn cs_label_ok(l: &Option<String>) -> bool {
    match l {
        None => true,
        Some(s) => !s.chars().any(char::is_control) && s.trim().chars().count() <= 40,
    }
}

fn cs_shift(r: &mut Rng) -> f64 {
    match r.below(10) {
        0..=7 => r.unit() * 40.0 - 20.0,
        _ => [f64::NAN, f64::NEG_INFINITY, 1e9][r.below(3)],
    }
}

fn cs_kind(r: &mut Rng, s: &Sheet) -> FullDimensionKind {
    let ents: Vec<EntityId> = s.entities.iter().map(|e| e.id).collect();
    let pts: Vec<PointId> = s.points.iter().map(|p| p.id).collect();
    let round: Vec<EntityId> = s
        .entities
        .iter()
        .filter(|e| matches!(e.kind, EntityKind::Arc { .. } | EntityKind::Circle { .. }))
        .map(|e| e.id)
        .collect();
    let rpick = |r: &mut Rng| {
        if r.chance(75) && !round.is_empty() {
            round[r.below(round.len())]
        } else {
            pick(r, &ents, EntityId(BOGUS))
        }
    };
    match r.below(6) {
        0 => FullDimensionKind::PathLength {
            entity: pick(r, &ents, EntityId(BOGUS)),
            offset_mm: cs_offset(r),
        },
        1 => FullDimensionKind::Angle {
            vertex: pick(r, &pts, PointId(BOGUS)),
            first: pick(r, &pts, PointId(BOGUS)),
            second: pick(r, &pts, PointId(BOGUS)),
            radius_mm: cs_arc_radius(r),
        },
        2 => FullDimensionKind::Radius {
            entity: rpick(r),
            angle_deg: cs_direction(r),
        },
        3 => FullDimensionKind::Diameter {
            entity: rpick(r),
            angle_deg: cs_direction(r),
        },
        4 => FullDimensionKind::Horizontal {
            from: pick(r, &pts, PointId(BOGUS)),
            to: pick(r, &pts, PointId(BOGUS)),
            offset_mm: cs_offset(r),
        },
        _ => FullDimensionKind::Vertical {
            from: pick(r, &pts, PointId(BOGUS)),
            to: pick(r, &pts, PointId(BOGUS)),
            offset_mm: cs_offset(r),
        },
    }
}

/// Entrée de création invalide **quel que soit l'état** de la feuille : objet absent, points
/// confondus par identifiant, valeur non finie ou hors bornes.
fn cs_static_invalid(s: &Sheet, k: &FullDimensionKind) -> bool {
    let missing = k.points().iter().any(|p| s.point(*p).is_err())
        || k.entity().is_some_and(|e| s.entity(e).is_err());
    let same = match *k {
        FullDimensionKind::Angle {
            vertex,
            first,
            second,
            ..
        } => vertex == first || vertex == second || first == second,
        FullDimensionKind::Horizontal { from, to, .. }
        | FullDimensionKind::Vertical { from, to, .. } => from == to,
        _ => false,
    };
    let value_bad = match *k {
        FullDimensionKind::PathLength { offset_mm, .. }
        | FullDimensionKind::Horizontal { offset_mm, .. }
        | FullDimensionKind::Vertical { offset_mm, .. } => !cs_offset_ok(offset_mm),
        FullDimensionKind::Angle { radius_mm, .. } => !cs_arc_radius_ok(radius_mm),
        FullDimensionKind::Radius { angle_deg, .. }
        | FullDimensionKind::Diameter { angle_deg, .. } => !angle_deg.is_finite(),
    };
    missing || same || value_bad
}

enum Ora {
    Value(f64),
    Undefined,
    Gray,
}

/// Longueur d'une Bézier par Simpson sur |B'(t)|, indépendante de `pf-geometry`.
fn cs_bezier_len(b: &pf_geometry::CubicBezier) -> f64 {
    let speed = |t: f64| {
        let u = 1.0 - t;
        let dx = 3.0
            * (u * u * (b.c1.x - b.p0.x)
                + 2.0 * u * t * (b.c2.x - b.c1.x)
                + t * t * (b.p3.x - b.c2.x));
        let dy = 3.0
            * (u * u * (b.c1.y - b.p0.y)
                + 2.0 * u * t * (b.c2.y - b.c1.y)
                + t * t * (b.p3.y - b.c2.y));
        dx.hypot(dy)
    };
    let n = 4_000;
    let h = 1.0 / n as f64;
    let mut sum = speed(0.0) + speed(1.0);
    for i in 1..n {
        sum += speed(i as f64 * h) * if i % 2 == 1 { 4.0 } else { 2.0 };
    }
    sum * h / 3.0
}

/// Valeur d'une cote recalculée **sans** `full_dimension_geometry` : positions brutes,
/// `atan2`, Simpson. `Undefined` quand la définition est dégénérée, `Gray` près d'un seuil.
fn cs_oracle(s: &Sheet, k: &FullDimensionKind) -> Ora {
    let pos = |p: PointId| s.position(p);
    // Seuils du modèle : 1e-6 mm (longueurs) ; zone grise autour.
    let classify = |len: f64| -> Option<Ora> {
        if len <= 5e-7 {
            Some(Ora::Undefined)
        } else if len < 2e-6 {
            Some(Ora::Gray)
        } else {
            None
        }
    };
    match *k {
        FullDimensionKind::Horizontal { from, to, .. }
        | FullDimensionKind::Vertical { from, to, .. } => {
            let (Ok(a), Ok(b)) = (pos(from), pos(to)) else {
                return Ora::Gray;
            };
            let d = if matches!(k, FullDimensionKind::Horizontal { .. }) {
                b.x - a.x
            } else {
                b.y - a.y
            };
            classify(d.abs()).unwrap_or(Ora::Value(d))
        }
        FullDimensionKind::Angle {
            vertex,
            first,
            second,
            ..
        } => {
            let (Ok(v), Ok(p1), Ok(p2)) = (pos(vertex), pos(first), pos(second)) else {
                return Ora::Gray;
            };
            let (u1, u2) = ((p1.x - v.x, p1.y - v.y), (p2.x - v.x, p2.y - v.y));
            let (n1, n2) = (u1.0.hypot(u1.1), u2.0.hypot(u2.1));
            if let Some(o) = classify(n1).or_else(|| classify(n2)) {
                return o;
            }
            let (a1, a2) = (u1.1.atan2(u1.0).to_degrees(), u2.1.atan2(u2.0).to_degrees());
            let ccw = (a2 - a1).rem_euclid(360.0);
            let near = ccw.min(360.0 - ccw);
            if near < 5e-7 {
                Ora::Undefined
            } else if near < 2e-6 {
                Ora::Gray
            } else {
                Ora::Value(ccw)
            }
        }
        FullDimensionKind::Radius { entity, .. } | FullDimensionKind::Diameter { entity, .. } => {
            let factor = if matches!(k, FullDimensionKind::Diameter { .. }) {
                2.0
            } else {
                1.0
            };
            match s.entity(entity).map(|e| e.kind.clone()) {
                Ok(EntityKind::Circle { radius_mm, .. }) => Ora::Value(radius_mm * factor),
                Ok(EntityKind::Arc { center, start, .. }) => match (pos(center), pos(start)) {
                    (Ok(c), Ok(st)) => Ora::Value((st.x - c.x).hypot(st.y - c.y) * factor),
                    _ => Ora::Gray,
                },
                Ok(_) => Ora::Undefined,
                Err(_) => Ora::Gray,
            }
        }
        FullDimensionKind::PathLength { entity, .. } => {
            let Ok(ent) = s.entity(entity) else {
                return Ora::Gray;
            };
            if matches!(ent.kind, EntityKind::Circle { .. }) {
                return Ora::Undefined;
            }
            let Ok((p0, p1)) = s.entity_points(entity) else {
                return Ora::Gray;
            };
            let (Ok(a), Ok(b)) = (pos(p0), pos(p1)) else {
                return Ora::Gray;
            };
            if let Some(o) = classify((b.x - a.x).hypot(b.y - a.y)) {
                return o;
            }
            match &ent.kind {
                EntityKind::Segment { .. } => Ora::Value((b.x - a.x).hypot(b.y - a.y)),
                EntityKind::Curve { .. } => match s.shape(entity) {
                    Ok(Shape::Cubic(c)) => Ora::Value(cs_bezier_len(&c)),
                    _ => Ora::Gray,
                },
                EntityKind::Spline { .. } => match s.shape(entity) {
                    Ok(Shape::Spline(sp)) => {
                        Ora::Value(sp.segments().iter().map(cs_bezier_len).sum())
                    }
                    _ => Ora::Gray,
                },
                EntityKind::Arc {
                    center,
                    start,
                    end,
                    ccw,
                } => {
                    let (Ok(c), Ok(st), Ok(en)) = (pos(*center), pos(*start), pos(*end)) else {
                        return Ora::Gray;
                    };
                    let r = (st.x - c.x).hypot(st.y - c.y);
                    let (a1, a2) = (
                        (st.y - c.y).atan2(st.x - c.x),
                        (en.y - c.y).atan2(en.x - c.x),
                    );
                    let tau = std::f64::consts::TAU;
                    let sweep = if *ccw {
                        (a2 - a1).rem_euclid(tau)
                    } else {
                        (a1 - a2).rem_euclid(tau)
                    };
                    Ora::Value(r * sweep)
                }
                EntityKind::Circle { .. } => Ora::Undefined,
            }
        }
    }
}

fn cs_close(got: f64, expected: f64, kind: &FullDimensionKind) -> bool {
    // Splines et Béziers : tolérances d'intégration ; le reste est exact à 1e-6 près.
    let loose = matches!(kind, FullDimensionKind::PathLength { .. });
    let tol = if loose { 1e-3 } else { 1e-6 };
    (got - expected).abs() <= tol * (1.0 + expected.abs())
}

/// Chaque cote de chaque feuille : valeur comparée à l'oracle, propriétés du texte, références.
fn cs_check_state(now: &Project, label: &str, st: &mut CoteStats) {
    for s in &now.sheets {
        for d in &s.full_dimensions {
            for p in d.kind.points() {
                assert!(
                    s.point(p).is_ok(),
                    "{label} : référence morte (point {})",
                    p.0
                );
            }
            if let Some(e) = d.kind.entity() {
                assert!(
                    s.entity(e).is_ok(),
                    "{label} : référence morte (tracé {})",
                    e.0
                );
            }
            let geometry = s.full_dimension_geometry(d.id);
            if geometry.is_err() {
                st.model_err += 1;
            }
            match (cs_oracle(s, &d.kind), geometry) {
                (Ora::Value(v), Ok(g)) => {
                    assert!(
                        cs_close(g.value, v, &d.kind),
                        "{label} : {:?} vaut {} (oracle {v})",
                        d.kind,
                        g.value
                    );
                    st.oracle_checked += 1;
                }
                (Ora::Value(v), Err(e)) => {
                    panic!(
                        "{label} : {:?} jugée non calculable ({e}) alors que l'oracle donne {v}",
                        d.kind
                    )
                }
                (Ora::Undefined, Ok(g)) => panic!(
                    "{label} : {:?} calculée ({}) alors que sa définition est dégénérée",
                    d.kind, g.value
                ),
                (Ora::Undefined, Err(e)) => {
                    assert!(!e.to_string().is_empty(), "{label}");
                    st.oracle_undefined += 1;
                }
                (Ora::Gray, _) => {
                    st.oracle_gray += 1;
                }
            }
            if let Ok(g) = s.full_dimension_geometry(d.id) {
                assert!(
                    g.text_angle_deg > -90.0 - 1e-9 && g.text_angle_deg <= 90.0 + 1e-9,
                    "{label} : texte à {}°",
                    g.text_angle_deg
                );
                assert!(
                    g.value.is_finite()
                        && g.text_anchor.x.is_finite()
                        && g.text_anchor.y.is_finite()
                );
                if matches!(d.kind, FullDimensionKind::Angle { .. }) {
                    assert!(
                        g.value > 0.0 && g.value < 360.0,
                        "{label} : angle {}",
                        g.value
                    );
                }
                assert!(g.text('.').contains(&g.value_text));
            }
        }
    }
}

/// Déplacement qui rend **exprès** une cote existante dégénérée (projection nulle, côtés d'angle
/// superposés, extrémités confondues) : la cote n'est ni refusée ni supprimée, elle devient
/// non calculable.
fn cs_spoil(r: &mut Rng, s: &Sheet, sheet: SheetId) -> Option<Command> {
    if s.full_dimensions.is_empty() {
        return None;
    }
    let d = &s.full_dimensions[r.below(s.full_dimensions.len())];
    let pos = |p: PointId| s.position(p).ok();
    let (point, to) = match d.kind {
        FullDimensionKind::Horizontal { from, to, .. } => {
            (to, PointMm::new(pos(from)?.x, pos(to)?.y))
        }
        FullDimensionKind::Vertical { from, to, .. } => {
            (to, PointMm::new(pos(to)?.x, pos(from)?.y))
        }
        FullDimensionKind::Angle {
            vertex,
            first,
            second,
            ..
        } => {
            let (v, f) = (pos(vertex)?, pos(first)?);
            match r.below(3) {
                0 => (
                    second,
                    PointMm::new(v.x + 2.0 * (f.x - v.x), v.y + 2.0 * (f.y - v.y)),
                ),
                1 => (second, v),
                _ => (first, v),
            }
        }
        FullDimensionKind::PathLength { entity, .. } => {
            let (a, b) = s.entity_points(entity).ok()?;
            (b, pos(a)?)
        }
        _ => return None,
    };
    Some(Command::MovePoint { sheet, point, to })
}

fn cs_command(r: &mut Rng, cur: &Project, sheet: SheetId, op: usize) -> Option<Command> {
    let s = cur.sheet(sheet).ok()?;
    let ents: Vec<EntityId> = s.entities.iter().map(|e| e.id).collect();
    let pts: Vec<PointId> = s.points.iter().map(|p| p.id).collect();
    let dims: Vec<FullDimensionId> = s.full_dimensions.iter().map(|d| d.id).collect();
    let pieces: Vec<PieceId> = cur.pieces.iter().map(|p| p.id).collect();
    Some(match op {
        0..=24 => Command::AddFullDimension {
            sheet,
            kind: cs_kind(r, s),
            label: if r.chance(40) { cs_label(r) } else { None },
            text_shift_mm: if r.chance(25) { cs_shift(r) } else { 0.0 },
        },
        25..=31 => Command::SetFullDimensionPlacement {
            sheet,
            dimension: pick(r, &dims, FullDimensionId(BOGUS)),
            placement: match r.below(3) {
                0 => cs_offset(r),
                1 => cs_arc_radius(r),
                _ => cs_direction(r),
            },
        },
        32..=36 => Command::SetFullDimensionLabel {
            sheet,
            dimension: pick(r, &dims, FullDimensionId(BOGUS)),
            label: cs_label(r),
        },
        37..=40 => Command::SetFullDimensionTextShift {
            sheet,
            dimension: pick(r, &dims, FullDimensionId(BOGUS)),
            text_shift_mm: cs_shift(r),
        },
        41..=44 => Command::DeleteFullDimension {
            sheet,
            dimension: pick(r, &dims, FullDimensionId(BOGUS)),
        },
        53..=60 => return cs_spoil(r, s, sheet),
        45..=52 => Command::MovePoint {
            sheet,
            point: pick(r, &pts, PointId(BOGUS)),
            to: match r.below(6) {
                0 => {
                    // Sur un autre point : superpose côtés et extrémités (cas dégénérés).
                    let q = pick(r, &pts, PointId(BOGUS));
                    s.position(q).unwrap_or(PointMm::new(10.0, 10.0))
                }
                1 => PointMm::new((r.below(20) * 10) as f64, (r.below(15) * 10) as f64),
                2 => pt_mm(r),
                _ => PointMm::new(r.unit() * 220.0, r.unit() * 170.0),
            },
        },
        61..=64 => Command::Transform {
            sheet,
            points: pts.clone(),
            transform: match r.below(3) {
                0 => Transform::Translate {
                    dx_mm: r.unit() * 80.0 - 40.0,
                    dy_mm: r.unit() * 80.0 - 40.0,
                },
                1 => Transform::Rotate {
                    center: pt_mm(r),
                    angle_deg: r.unit() * 360.0,
                },
                _ => Transform::Mirror {
                    axis_point: pt_mm(r),
                    axis_angle_deg: r.unit() * 180.0,
                },
            },
        },
        65..=69 => Command::Delete {
            sheet,
            points: if r.chance(40) {
                some(r, &pts, 2)
            } else {
                Vec::new()
            },
            entities: if r.chance(60) {
                some(r, &ents, 2)
            } else {
                Vec::new()
            },
        },
        70..=72 => {
            let entity = pick(r, &ents, EntityId(BOGUS));
            Command::SplitPath {
                sheet,
                entity,
                at: ls_point_on(r, s, entity),
            }
        }
        73..=74 => Command::MergePaths {
            sheet,
            first: pick(r, &ents, EntityId(BOGUS)),
            second: pick(r, &ents, EntityId(BOGUS)),
        },
        75..=76 => Command::ReversePath {
            sheet,
            entity: pick(r, &ents, EntityId(BOGUS)),
        },
        77..=79 => Command::DuplicateSheet { sheet },
        80..=82 => {
            let fragment = cur
                .copy_fragment(sheet, &some(r, &pts, 3), &some(r, &ents, 3), &[], &[])
                .ok()?;
            Command::PasteFragment {
                sheet,
                fragment: Box::new(fragment),
                layer: None,
            }
        }
        83..=86 => {
            if s.layers.is_empty() || r.chance(25) {
                Command::AddLayer {
                    sheet,
                    name: format!("Calque {}", r.below(1000)),
                }
            } else {
                let layers: Vec<LayerId> = s.layers.iter().map(|l| l.id).collect();
                if r.chance(50) {
                    Command::SetLayerLocked {
                        sheet,
                        layer: pick(r, &layers, LayerId(BOGUS)),
                        locked: r.chance(60),
                    }
                } else if r.chance(50) {
                    Command::SetLayerVisible {
                        sheet,
                        layer: pick(r, &layers, LayerId(BOGUS)),
                        visible: r.chance(50),
                    }
                } else {
                    Command::SetObjectsLayer {
                        sheet,
                        points: some(r, &pts, 2),
                        entities: some(r, &ents, 3),
                        layer: if r.chance(80) {
                            Some(pick(r, &layers, LayerId(BOGUS)))
                        } else {
                            None
                        },
                    }
                }
            }
        }
        87..=89 => Command::SplitPiece {
            piece: pick(r, &pieces, PieceId(BOGUS)),
            a: pt_mm(r),
            b: PointMm::new(r.unit() * 200.0, r.unit() * 150.0),
        },
        90..=92 => Command::AddArc {
            sheet,
            center: PointRef::New(PointMm::new(r.unit() * 200.0, r.unit() * 150.0)),
            radius_mm: 8.0 + r.unit() * 50.0,
            start_deg: r.unit() * 360.0,
            end_deg: r.unit() * 360.0,
            ccw: r.chance(70),
        },
        93..=94 => Command::AddCircle {
            sheet,
            center: PointRef::New(PointMm::new(r.unit() * 200.0, r.unit() * 150.0)),
            radius_mm: 5.0 + r.unit() * 40.0,
        },
        95..=96 => Command::AddCurve {
            sheet,
            start: PointRef::New(PointMm::new(r.unit() * 200.0, r.unit() * 150.0)),
            c1: PointMm::new(r.unit() * 200.0, r.unit() * 150.0),
            c2: PointMm::new(r.unit() * 200.0, r.unit() * 150.0),
            end: PointRef::New(PointMm::new(r.unit() * 200.0, r.unit() * 150.0)),
        },
        97 => Command::AddSpline {
            sheet,
            points: (0..3 + r.below(3))
                .map(|_| PointRef::New(PointMm::new(r.unit() * 200.0, r.unit() * 150.0)))
                .collect(),
        },
        _ => return None,
    })
}

#[test]
fn us067_campagne_aleatoire_des_cotes_graine_fixe() {
    let mut st = CoteStats::default();
    for seq in 0..180u64 {
        let mut r = Rng(0x06_0700 + seq);
        let mut project = Project::new("Cotes aléa");
        let sheet = project.sheets[0].id;
        project
            .apply(Command::AddRectangle {
                sheet,
                corner: PointRef::New(PointMm::new(0.0, 0.0)),
                width_mm: 200.0,
                height_mm: 150.0,
            })
            .unwrap();
        let _ = project.apply(Command::ExtractPiece {
            sheet,
            source: PieceSource::Click {
                at: PointMm::new(100.0, 75.0),
            },
            name: "Base".into(),
            detached: false,
        });
        random_drawing(&mut r, &mut project, sheet);
        // Des cotes au départ : de la matière pour les autres commandes.
        for _ in 0..(2 + r.below(5)) {
            let kind = cs_kind(&mut r, project.sheet(sheet).unwrap());
            let _ = project.apply(Command::AddFullDimension {
                sheet,
                kind,
                label: None,
                text_shift_mm: 0.0,
            });
        }
        let initial_json = snap(&project);
        let mut doc = Document::new(project);
        let mut undo_stack: Vec<String> = Vec::new();
        let mut redo_stack: Vec<String> = Vec::new();
        for step in 0..26 {
            let label = format!("graine {seq} pas {step}");
            let before_json = snap(doc.project());
            let cur = doc.project().clone();
            let op = r.below(100);
            if let Some(cmd) = cs_command(&mut r, &cur, sheet, op) {
                let name = variant_name(&cmd);
                let s = cur.sheet(sheet).unwrap();
                let any_locked = cur.sheets.iter().any(|x| x.layers.iter().any(|l| l.locked));
                // Entrée à refuser à coup sûr, ou à accepter à coup sûr (état sans verrou).
                let (must_refuse, must_accept) = match &cmd {
                    Command::AddFullDimension {
                        kind,
                        label: l,
                        text_shift_mm,
                        ..
                    } => (
                        cs_static_invalid(s, kind)
                            || !cs_label_ok(l)
                            || !cs_offset_ok(*text_shift_mm),
                        false,
                    ),
                    Command::SetFullDimensionPlacement {
                        dimension,
                        placement,
                        ..
                    } => match s.full_dimension(*dimension) {
                        Err(_) => (true, false),
                        Ok(d) => {
                            let ok = match d.kind {
                                FullDimensionKind::Angle { .. } => cs_arc_radius_ok(*placement),
                                FullDimensionKind::Radius { .. }
                                | FullDimensionKind::Diameter { .. } => placement.is_finite(),
                                _ => cs_offset_ok(*placement),
                            };
                            (!ok, ok && !any_locked)
                        }
                    },
                    Command::SetFullDimensionLabel {
                        dimension,
                        label: l,
                        ..
                    } => match s.full_dimension(*dimension) {
                        Err(_) => (true, false),
                        Ok(_) => (!cs_label_ok(l), cs_label_ok(l) && !any_locked),
                    },
                    Command::SetFullDimensionTextShift {
                        dimension,
                        text_shift_mm,
                        ..
                    } => match s.full_dimension(*dimension) {
                        Err(_) => (true, false),
                        Ok(_) => (
                            !cs_offset_ok(*text_shift_mm),
                            cs_offset_ok(*text_shift_mm) && !any_locked,
                        ),
                    },
                    Command::DeleteFullDimension { dimension, .. } => {
                        match s.full_dimension(*dimension) {
                            Err(_) => (true, false),
                            Ok(_) => (false, !any_locked),
                        }
                    }
                    _ => (false, false),
                };
                let created_kind = match &cmd {
                    Command::AddFullDimension { kind, .. } => Some(kind.key().to_string()),
                    _ => None,
                };
                let add_kind = match &cmd {
                    Command::AddFullDimension { kind, .. } => Some(*kind),
                    _ => None,
                };
                let Ok(result) = catch_unwind(AssertUnwindSafe(|| doc.execute(cmd))) else {
                    panic!("panique ({name}) : {label}");
                };
                match result {
                    Err(e) => {
                        st.refused += 1;
                        assert!(!e.to_string().is_empty(), "{label}");
                        assert_eq!(
                            snap(doc.project()),
                            before_json,
                            "refus avec trace ({name}) : {label}"
                        );
                        assert!(
                            !must_accept,
                            "{name} : commande valide refusée ({e}) : {label}"
                        );
                        if must_refuse {
                            st.must_refuse += 1;
                        }
                        if e.to_string().contains("verrouillé") {
                            st.locked_refusals += 1;
                        }
                        // Création refusée pour une raison de géométrie : l'oracle ne doit pas
                        // trouver une valeur nette (hors verrou et plafond).
                        if let Some(k) = add_kind {
                            let msg = e.to_string();
                            if !must_refuse
                                && !msg.contains("verrouillé")
                                && !msg.contains("au plus")
                            {
                                assert!(
                                    !matches!(cs_oracle(s, &k), Ora::Value(_)),
                                    "{name} : refus injustifié ({msg}) de {k:?} : {label}"
                                );
                            }
                        }
                    }
                    Ok(out) => {
                        assert!(
                            !must_refuse,
                            "{name} : une entrée invalide a été acceptée : {label}"
                        );
                        doc.project()
                            .validate()
                            .unwrap_or_else(|e| panic!("{name} : {label} : {e}"));
                        if snap(doc.project()) == before_json {
                            st.noops += 1;
                        } else {
                            st.accepted += 1;
                            undo_stack.push(before_json.clone());
                            redo_stack.clear();
                        }
                        if must_accept {
                            st.must_accept += 1;
                        }
                        if let Some(k) = created_kind {
                            *st.created.entry(k).or_default() += 1;
                            // La cote créée est bien là, définie (ou en zone grise) pour l'oracle.
                            let sh = doc.project().sheet(sheet).unwrap();
                            let d = sh.full_dimension(out.full_dimensions[0]).unwrap();
                            assert!(
                                !matches!(cs_oracle(sh, &d.kind), Ora::Undefined),
                                "{label} : cote dégénérée acceptée : {:?}",
                                d.kind
                            );
                        }
                        st.pruned_notes += out
                            .notes
                            .iter()
                            .filter(|n| n.contains("mesurait un objet supprimé"))
                            .count();
                        match name.as_str() {
                            "DuplicateSheet" => st.copies += 1,
                            "PasteFragment" => st.pastes += 1,
                            "SplitPiece" => st.splits += 1,
                            _ => {}
                        }
                    }
                }
            } else if r.chance(60) {
                let expected = undo_stack.pop();
                assert_eq!(doc.undo(), expected.is_some(), "{label}");
                if let Some(state) = expected {
                    assert_eq!(snap(doc.project()), state, "annuler : {label}");
                    redo_stack.push(before_json);
                    st.undone += 1;
                }
            } else if r.chance(50) {
                let expected = redo_stack.pop();
                assert_eq!(doc.redo(), expected.is_some(), "{label}");
                if let Some(state) = expected {
                    assert_eq!(snap(doc.project()), state, "rétablir : {label}");
                    undo_stack.push(before_json);
                }
            } else {
                let json = persist::to_json(doc.project())
                    .unwrap_or_else(|e| panic!("sauvegarde refusée : {label} : {e}"));
                let back = persist::from_json(&json)
                    .unwrap_or_else(|e| panic!("relecture refusée : {label} : {e}"));
                assert_eq!(&back, doc.project(), "relecture différente : {label}");
                assert_eq!(persist::to_json(&back).unwrap(), json, "octets : {label}");
                st.reloaded += 1;
            }
            cs_check_state(&doc.project().clone(), &label, &mut st);
            if step % 4 == 3 {
                check_export(doc.project(), &label);
                st.exports += 1;
            }
        }
        // Fin de séquence : relecture identique ; tout annuler redonne le départ.
        let json = persist::to_json(doc.project()).unwrap();
        let back = persist::from_json(&json).unwrap();
        assert_eq!(&back, doc.project(), "graine {seq}");
        assert_eq!(persist::to_json(&back).unwrap(), json, "graine {seq}");
        while doc.undo() {}
        assert_eq!(
            snap(doc.project()),
            initial_json,
            "tout annuler : graine {seq}"
        );
    }
    eprintln!(
        "cotes : {} modifications, {} refus ({} entrées invalides, {} calques verrouillés), {} sans \
         changement ; créées {:?} ; {} cotes comparées à l'oracle, {} jugées non calculables à \
         raison ; {} retraits avec remarque, {} copies de feuille, {} collages, {} séparations ; \
         {} acceptations attendues ; {} gris, {} erreurs du modèle ; {} annulations, {} relectures, {} exports",
        st.accepted,
        st.refused,
        st.must_refuse,
        st.locked_refusals,
        st.noops,
        st.created,
        st.oracle_checked,
        st.oracle_undefined,
        st.pruned_notes,
        st.copies,
        st.pastes,
        st.splits,
        st.must_accept,
        st.oracle_gray,
        st.model_err,
        st.undone,
        st.reloaded,
        st.exports
    );
    assert!(
        st.accepted >= 800,
        "seulement {} modifications",
        st.accepted
    );
    assert!(st.refused >= 400, "seulement {} refus", st.refused);
    assert!(
        st.must_refuse >= 150,
        "seulement {} refus attendus",
        st.must_refuse
    );
    assert!(
        st.must_accept >= 100,
        "seulement {} acceptations attendues",
        st.must_accept
    );
    for kind in [
        "path_length",
        "angle",
        "radius",
        "diameter",
        "horizontal",
        "vertical",
    ] {
        assert!(
            st.created.get(kind).copied().unwrap_or(0) >= 30,
            "type {kind} : seulement {:?} créations",
            st.created.get(kind)
        );
    }
    assert!(
        st.oracle_checked >= 3000,
        "seulement {} cotes mesurées",
        st.oracle_checked
    );
    assert!(
        st.oracle_undefined >= 20,
        "seulement {} cotes non calculables",
        st.oracle_undefined
    );
    assert!(
        st.pruned_notes >= 30,
        "seulement {} retraits",
        st.pruned_notes
    );
    assert!(
        st.copies >= 10 && st.pastes >= 10,
        "{} copies, {} collages",
        st.copies,
        st.pastes
    );
    assert!(
        st.undone >= 100 && st.reloaded >= 25 && st.exports >= 600,
        "couverture insuffisante"
    );
    assert!(st.locked_refusals >= 1, "aucun refus de calque verrouillé");
}
