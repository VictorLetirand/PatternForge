//! Campagne adverse : pieces derivees (graine fixe).

mod adverse_support;
use adverse_support::*;

use pf_core::command::{Command, PieceSource, PointRef};
use pf_core::model::*;
use pf_core::{persist, DerivedKind, DerivedOp, Document, Project};
use pf_geometry::{PointMm, Transform};
use std::panic::{catch_unwind, AssertUnwindSafe};

// ---------------------------------------------------------------------------
// US-04.4 : pièces dérivées
// ---------------------------------------------------------------------------

#[derive(Default)]
struct DerivedStats {
    created: usize,
    accepted: usize,
    refused: usize,
    noops: usize,
    regenerated: usize,
    stale_events: usize,
    rebuilt: usize,
    broken: usize,
    orphaned: usize,
    copies: usize,
    direct_refused: usize,
    exact: usize,
    distance: usize,
    undone: usize,
    reloaded: usize,
}

/// Contour d'une pièce échantillonné par tracé (`n` pas), sens du contour.
fn derived_polygon(p: &Project, piece: PieceId, n: usize) -> Vec<PointMm> {
    let sheet = p.piece_sheet(piece).unwrap();
    let mut poly: Vec<PointMm> = Vec::new();
    for e in &p.piece(piece).unwrap().outline {
        let mut pts = sheet.shape(e.entity).unwrap().sample(n);
        if e.reversed {
            pts.reverse();
        }
        poly.extend(pts);
    }
    poly
}

/// Sommets (x, y) d'un contour qui n'est fait que de segments chaînés, sinon `None`.
fn segment_vertices(p: &Project, piece: PieceId) -> Option<Vec<(f64, f64)>> {
    let pc = p.piece(piece).ok()?;
    let sheet = p.piece_sheet(piece).ok()?;
    let mut ends = Vec::new();
    let mut v = Vec::new();
    for e in &pc.outline {
        let (a, b) = match sheet.entity(e.entity).ok()?.kind {
            EntityKind::Segment { from, to, .. } => (from, to),
            _ => return None,
        };
        let (s, t) = if e.reversed { (b, a) } else { (a, b) };
        let q = sheet.position(s).ok()?;
        v.push((q.x, q.y));
        ends.push((s, t));
    }
    let n = ends.len();
    (n >= 3 && (0..n).all(|i| ends[i].1 == ends[(i + 1) % n].0)).then_some(v)
}

/// Aire et périmètre attendus du décalage `d` (positif = extérieur) de la mère,
/// calculés à part : cercle (formule) ou polygone strictement convexe dont aucune
/// arête ne s'inverse et dont aucun coin extérieur ne dépasse 1,5 fois la distance
/// (loin de la limite de pointe). `None` hors de ces cas.
fn expected_derived_measures(p: &Project, mother: PieceId, d: f64) -> Option<(f64, f64)> {
    let pc = p.piece(mother).ok()?;
    let sheet = p.piece_sheet(mother).ok()?;
    if let [e] = pc.outline.as_slice() {
        if let EntityKind::Circle { radius_mm, .. } = sheet.entity(e.entity).ok()?.kind {
            let r = radius_mm + d;
            return (r > 1e-3)
                .then_some((std::f64::consts::PI * r * r, 2.0 * std::f64::consts::PI * r));
        }
    }
    let mut v = segment_vertices(p, mother)?;
    let signed = |v: &[(f64, f64)]| -> f64 {
        (0..v.len())
            .map(|i| 0.5 * (v[i].0 * v[(i + 1) % v.len()].1 - v[(i + 1) % v.len()].0 * v[i].1))
            .sum()
    };
    if signed(&v) < 0.0 {
        v.reverse();
    }
    let n = v.len();
    // Strictement convexe.
    for i in 0..n {
        let (a, b, c) = (v[i], v[(i + 1) % n], v[(i + 2) % n]);
        if (b.0 - a.0) * (c.1 - b.1) - (b.1 - a.1) * (c.0 - b.0) < 1e-3 {
            return None;
        }
    }
    let line = |i: usize| {
        let (a, b) = (v[i], v[(i + 1) % n]);
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let l = dx.hypot(dy);
        ((a.0 + d * dy / l, a.1 - d * dx / l), (dx, dy))
    };
    let w: Vec<(f64, f64)> = (0..n)
        .map(|i| {
            let ((p0, d0), (p1, d1)) = (line((i + n - 1) % n), line(i));
            let t = ((p1.0 - p0.0) * d1.1 - (p1.1 - p0.1) * d1.0) / (d0.0 * d1.1 - d0.1 * d1.0);
            (p0.0 + t * d0.0, p0.1 + t * d0.1)
        })
        .collect();
    if w.iter().any(|q| !q.0.is_finite() || !q.1.is_finite()) {
        return None;
    }
    for i in 0..n {
        let (a, b) = (v[i], v[(i + 1) % n]);
        let (c, e) = (w[i], w[(i + 1) % n]);
        if (e.0 - c.0) * (b.0 - a.0) + (e.1 - c.1) * (b.1 - a.1) <= 0.0 {
            return None;
        }
        if (c.0 - a.0).hypot(c.1 - a.1) > 1.5 * d.abs() + 1e-9 {
            return None;
        }
    }
    let perim: f64 = (0..n)
        .map(|i| (w[(i + 1) % n].0 - w[i].0).hypot(w[(i + 1) % n].1 - w[i].1))
        .sum();
    Some((signed(&w).abs(), perim))
}

/// Invariants d'une dérivée à jour et lectures sans panique ; une dérivée
/// obsolète n'a pas bougé pendant la commande qui vient d'être acceptée.
fn check_derived_state(
    before: Option<&Project>,
    now: &Project,
    label: &str,
    deep: bool,
    stats: &mut DerivedStats,
) {
    use pf_core::piece::PieceIssue;
    for d in now.pieces.iter().filter(|p| p.derived.is_some()) {
        let spec = d.derived.as_ref().unwrap();
        let (card, issues, info) = catch_unwind(AssertUnwindSafe(|| {
            (
                now.piece_card(d.id),
                now.piece_issues(d.id),
                now.piece_derivation(d.id),
            )
        }))
        .unwrap_or_else(|_| panic!("lecture panique : {label}"));
        let card = card.unwrap();
        let info = info.unwrap_or_else(|| panic!("piece_derivation vide : {label}"));
        assert_eq!(card.derived.as_ref(), Some(&info), "{label}");
        assert_eq!(card.issues, issues, "{label}");
        assert!(now.piece(spec.mother).is_ok(), "mère absente : {label}");
        assert!(matches!(d.link, PieceLink::Linked { .. }), "{label}");
        let flagged: Vec<&PieceIssue> = issues
            .iter()
            .filter(|i| matches!(i, PieceIssue::DerivedStale { .. }))
            .collect();
        match &spec.stale {
            Some(reason) => {
                assert_eq!(flagged.len(), 1, "{label}");
                assert!(
                    matches!(flagged[0], PieceIssue::DerivedStale { reason: r } if r == reason),
                    "{label}"
                );
                if let Some(b) = before.and_then(|b| b.piece(d.id).ok()) {
                    if b.derived.is_some() {
                        assert_eq!(
                            b.outline, d.outline,
                            "géométrie bougée (obsolète) : {label}"
                        );
                        let PieceLink::Linked { sheet: sid } = d.link else {
                            unreachable!()
                        };
                        assert_eq!(
                            before.unwrap().sheet(sid).unwrap(),
                            now.sheet(sid).unwrap(),
                            "feuille d'une dérivée obsolète modifiée : {label}"
                        );
                    }
                }
            }
            None => {
                assert!(flagged.is_empty(), "{label}");
                let c = now
                    .piece_contour(d.id)
                    .unwrap_or_else(|e| panic!("dérivée à jour sans contour ({e}) : {label}"));
                assert!(c.closed && !c.self_intersecting, "{label}");
                // Un dépliage n'a pas de décalage : contrôlé par la campagne US-04.7.
                let Some(dist) = spec.operation.distance_mm() else {
                    continue;
                };
                let Ok(m) = now.piece_contour(spec.mother) else {
                    panic!("dérivée à jour d'une mère sans contour : {label}");
                };
                if dist < 0.0 {
                    assert!(c.area_mm2 < m.area_mm2 + 1e-6, "aire : {label}");
                } else {
                    assert!(c.area_mm2 > m.area_mm2 - 1e-6, "aire : {label}");
                }
                if let Some((area, perimeter)) = expected_derived_measures(now, spec.mother, dist) {
                    assert!(
                        (c.area_mm2 - area).abs() <= 1e-5 * (1.0 + area),
                        "aire {} ≠ attendu {area} : {label}",
                        c.area_mm2
                    );
                    assert!(
                        (c.perimeter_mm - perimeter).abs() <= 1e-5 * (1.0 + perimeter),
                        "périmètre {} ≠ attendu {perimeter} : {label}",
                        c.perimeter_mm
                    );
                    stats.exact += 1;
                }
                // Distance à la mère : jamais plus près que |d| (0,02 mm : polyligne).
                // Coûteux en débogage : un état sur trois.
                if deep {
                    let mother_line = derived_polygon(now, spec.mother, 400);
                    for q in derived_polygon(now, d.id, 24) {
                        let got = dist_to_polyline(&mother_line, q);
                        assert!(
                            got >= dist.abs() - 0.02,
                            "point à {got} < {} de la mère : {label}",
                            dist.abs()
                        );
                    }
                    stats.distance += 1;
                }
            }
        }
    }
}

/// Pièce dérivée dont le contour contient une courbe (Bézier ou lisse).
fn is_curved_derived(p: &Project, piece: PieceId) -> bool {
    let (Ok(pc), Ok(sheet)) = (p.piece(piece), p.piece_sheet(piece)) else {
        return false;
    };
    pc.derived.is_some()
        && pc.outline.iter().any(|e| {
            sheet.entity(e.entity).is_ok_and(|x| {
                matches!(x.kind, EntityKind::Curve { .. } | EntityKind::Spline { .. })
            })
        })
}

/// Une commande de la campagne US-04.4 (ou `None` : annuler, rétablir, relire).
fn derived_command(r: &mut Rng, cur: &Project, op: usize) -> Option<Command> {
    let sheets: Vec<SheetId> = cur.sheets.iter().map(|s| s.id).collect();
    let pieces: Vec<PieceId> = cur.pieces.iter().map(|p| p.id).collect();
    let derived: Vec<&Piece> = cur.pieces.iter().filter(|p| p.derived.is_some()).collect();
    // Feuille visée : la feuille de la mère le plus souvent.
    let sheet = if r.chance(70) {
        sheets[0]
    } else {
        pick(r, &sheets, SheetId(BOGUS))
    };
    let sh = cur.sheet(sheet).ok();
    let pts: Vec<PointId> = sh
        .map(|s| s.points.iter().map(|p| p.id).collect())
        .unwrap_or_default();
    let ents: Vec<EntityId> = sh
        .map(|s| s.entities.iter().map(|e| e.id).collect())
        .unwrap_or_default();
    let coord = |r: &mut Rng| match r.below(10) {
        0 => [1e7, -1e7, 1e-9, 0.0, f64::NAN, 1e300][r.below(6)],
        1..=3 => r.unit() * 20.0 - 10.0,
        _ => r.unit() * 400.0 - 50.0,
    };
    let distance = |r: &mut Rng| match r.below(20) {
        0 => [0.0, f64::NAN, f64::INFINITY, 1e9, 0.001, -1e9, 0.01][r.below(7)],
        1..=3 => (r.unit() * 150.0 + 20.0) * if r.chance(50) { 1.0 } else { -1.0 },
        _ => (r.unit() * 18.0 + 1.0) * if r.chance(55) { -1.0 } else { 1.0 },
    };
    Some(match op {
        0..=22 => Command::MovePoint {
            sheet,
            point: pick(r, &pts, PointId(BOGUS)),
            to: PointMm::new(coord(r), coord(r)),
        },
        23..=32 => {
            let transform = match r.below(3) {
                0 => Transform::Translate {
                    dx_mm: r.unit() * 200.0 - 100.0,
                    dy_mm: r.unit() * 200.0 - 100.0,
                },
                1 => Transform::Rotate {
                    center: PointMm::new(r.unit() * 100.0, r.unit() * 100.0),
                    angle_deg: r.unit() * 360.0,
                },
                _ => Transform::Mirror {
                    axis_point: PointMm::new(r.unit() * 100.0, r.unit() * 100.0),
                    axis_angle_deg: r.unit() * 180.0,
                },
            };
            let chosen = if r.chance(70) {
                pts.clone()
            } else {
                some(r, &pts, 4)
            };
            Command::Transform {
                sheet,
                points: chosen,
                transform,
            }
        }
        33..=36 => Command::SetCircleRadius {
            sheet,
            entity: pick(r, &ents, EntityId(BOGUS)),
            radius_mm: if r.chance(85) {
                r.unit() * 120.0 + 5.0
            } else {
                coord(r)
            },
        },
        37..=40 => Command::Delete {
            sheet,
            points: if r.chance(30) {
                some(r, &pts, 2)
            } else {
                vec![]
            },
            entities: if r.chance(30) {
                vec![]
            } else {
                some(r, &ents, 1)
            },
        },
        41..=56 if cur.pieces.len() < 6 => Command::CreateDerivedPiece {
            // Pas de dérivée d'une dérivée à courbes : le décalage d'un décalage de Bézier
            // compte des centaines de tracés (coût de test, pas un défaut).
            mother: pick(
                r,
                &pieces
                    .iter()
                    .copied()
                    .filter(|id| !is_curved_derived(cur, *id))
                    .collect::<Vec<_>>(),
                PieceId(BOGUS),
            ),
            kind: DerivedKind::ALL[r.below(4)],
            operation: DerivedOp::Offset {
                distance_mm: distance(r),
            },
            name: r.chance(30).then(|| format!("Dérivée {}", r.below(1000))),
        },
        57..=62 => Command::BreakDerivedLink {
            piece: if r.chance(85) && !derived.is_empty() {
                derived[r.below(derived.len())].id
            } else {
                pick(r, &pieces, PieceId(BOGUS))
            },
        },
        63..=66 => {
            let s = pick(r, &sheets, SheetId(BOGUS));
            if r.chance(70) {
                Command::DuplicateSheet { sheet: s }
            } else {
                match cur.import_command(s) {
                    Ok(cmd) => cmd,
                    Err(_) => Command::DuplicateSheet { sheet: s },
                }
            }
        }
        67..=73 => {
            // Modification directe de la géométrie d'une dérivée.
            let target = if derived.is_empty() {
                None
            } else {
                Some(derived[r.below(derived.len())])
            };
            match target.and_then(|d| match d.link {
                PieceLink::Linked { sheet } => cur.sheet(sheet).ok(),
                PieceLink::Detached => None,
            }) {
                Some(s) if !s.points.is_empty() => Command::MovePoint {
                    sheet: s.id,
                    point: s.points[r.below(s.points.len())].id,
                    to: PointMm::new(r.unit() * 300.0, r.unit() * 300.0),
                },
                _ => return None,
            }
        }
        74..=77 => Command::DeletePiece {
            piece: pick(r, &pieces, PieceId(BOGUS)),
        },
        78..=79 => Command::DeleteSheet {
            sheet: pick(r, &sheets, SheetId(BOGUS)),
        },
        83..=84 => Command::Batch {
            // Créer une dérivée puis modifier la mère dans le même lot.
            commands: vec![
                Command::CreateDerivedPiece {
                    mother: pick(r, &pieces, PieceId(BOGUS)),
                    kind: DerivedKind::ALL[r.below(4)],
                    operation: DerivedOp::Offset {
                        distance_mm: distance(r),
                    },
                    name: None,
                },
                Command::MovePoint {
                    sheet,
                    point: pick(r, &pts, PointId(BOGUS)),
                    to: PointMm::new(coord(r), coord(r)),
                },
            ],
        },
        80..=82 => Command::Batch {
            commands: vec![
                Command::MovePoint {
                    sheet,
                    point: pick(r, &pts, PointId(BOGUS)),
                    to: PointMm::new(coord(r), coord(r)),
                },
                Command::MovePoint {
                    sheet,
                    point: pick(r, &pts, PointId(BOGUS)),
                    to: PointMm::new(coord(r), coord(r)),
                },
            ],
        },
        _ => return None,
    })
}

#[test]
fn us044_campagne_aleatoire_des_pieces_derivees_graine_fixe() {
    let mut st = DerivedStats::default();
    for seq in 0..170u64 {
        let mut r = Rng(0x04_0400 + seq);
        let mut project = Project::new("Dérivées aléa");
        let sheet = project.sheets[0].id;
        let at = |r: &mut Rng| PointMm::new(r.unit() * 100.0, r.unit() * 100.0);
        // Mère : rectangle, triangle, cercle ou lentille (segment + Bézier).
        // (La lentille n'est tirée qu'une fois sur douze, avec moins de pas : le décalage
        // d'une Bézier donne une trentaine de tracés, parfois des milliers quand une
        // poignée part très loin, et chaque pas relit plusieurs fois ces contours.)
        let lens = seq % 12 == 5;
        let entities: Vec<EntityId> = match if lens { 3 } else { [0, 0, 1, 1, 2][r.below(5)] } {
            0 => {
                project
                    .apply(Command::AddRectangle {
                        sheet,
                        corner: PointRef::New(at(&mut r)),
                        width_mm: 60.0 + r.unit() * 200.0,
                        height_mm: 50.0 + r.unit() * 150.0,
                    })
                    .unwrap()
                    .entities
            }
            1 => {
                let (a, b, c) = (
                    PointMm::new(0.0, 0.0),
                    PointMm::new(80.0 + r.unit() * 120.0, r.unit() * 20.0),
                    PointMm::new(r.unit() * 100.0, 60.0 + r.unit() * 100.0),
                );
                let mut ids = Vec::new();
                for (s, e) in [(a, b), (b, c), (c, a)] {
                    ids.extend(
                        project
                            .apply(Command::AddSegment {
                                sheet,
                                from: PointRef::New(s),
                                to: PointRef::New(e),
                            })
                            .unwrap()
                            .entities,
                    );
                }
                ids
            }
            2 => {
                project
                    .apply(Command::AddCircle {
                        sheet,
                        center: PointRef::New(at(&mut r)),
                        radius_mm: 30.0 + r.unit() * 70.0,
                    })
                    .unwrap()
                    .entities
            }
            _ => {
                let w = 80.0 + r.unit() * 100.0;
                let mut ids = project
                    .apply(Command::AddSegment {
                        sheet,
                        from: PointRef::New(PointMm::new(0.0, 0.0)),
                        to: PointRef::New(PointMm::new(w, 0.0)),
                    })
                    .unwrap()
                    .entities;
                ids.extend(
                    project
                        .apply(Command::AddCurve {
                            sheet,
                            start: PointRef::New(PointMm::new(w, 0.0)),
                            c1: PointMm::new(w, 15.0 + r.unit() * 25.0),
                            c2: PointMm::new(0.0, 15.0 + r.unit() * 25.0),
                            end: PointRef::New(PointMm::new(0.0, 0.0)),
                        })
                        .unwrap()
                        .entities,
                );
                ids
            }
        };
        let mother = project
            .apply(Command::ExtractPiece {
                sheet,
                source: PieceSource::Paths { entities },
                name: "Mère".into(),
                detached: false,
            })
            .unwrap()
            .pieces[0];
        for _ in 0..(1 + r.below(3)) {
            let parent = match project.pieces.last().map(|p| p.id) {
                Some(last) if r.chance(25) && !is_curved_derived(&project, last) => last,
                _ => mother,
            };
            let d = (3.0 + r.unit() * 12.0) * if r.chance(60) { -1.0 } else { 1.0 };
            if project
                .apply(Command::CreateDerivedPiece {
                    mother: parent,
                    kind: DerivedKind::ALL[r.below(4)],
                    operation: DerivedOp::Offset { distance_mm: d },
                    name: None,
                })
                .is_ok()
            {
                st.created += 1;
            }
        }
        let mut doc = Document::new(project);
        let mut undo_stack: Vec<String> = Vec::new();
        let mut redo_stack: Vec<String> = Vec::new();
        for step in 0..if lens { 12 } else { 22 } {
            let label = format!("graine {seq} pas {step}");
            let before_json = snap(doc.project());
            let cur = doc.project().clone();
            let op = r.below(100);
            let stale_before = cur
                .pieces
                .iter()
                .filter(|p| p.derived.as_ref().is_some_and(|d| d.stale.is_some()))
                .count();
            let mut accepted_cmd = false;
            if let Some(cmd) = derived_command(&mut r, &cur, op) {
                let name = variant_name(&cmd);
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
                        if e.to_string().contains("Cassez le lien") {
                            st.direct_refused += 1;
                        }
                    }
                    Ok(out) => {
                        doc.project()
                            .validate()
                            .unwrap_or_else(|e| panic!("{name} : {label} : {e}"));
                        if snap(doc.project()) == before_json {
                            st.noops += 1;
                        } else {
                            st.accepted += 1;
                            accepted_cmd = true;
                            undo_stack.push(before_json.clone());
                            redo_stack.clear();
                        }
                        st.regenerated += out.regenerated.len();
                        for id in &out.regenerated {
                            assert!(
                                doc.project().piece(*id).is_ok_and(|p| p.derived.is_some()),
                                "régénérée sans lien : {label}"
                            );
                        }
                        st.rebuilt += out
                            .notes
                            .iter()
                            .filter(|n| n.contains("changé de structure"))
                            .count();
                        st.orphaned += out
                            .notes
                            .iter()
                            .filter(|n| n.contains("n'est plus dérivée"))
                            .count();
                        st.created += usize::from(name == "CreateDerivedPiece");
                        st.broken += usize::from(name == "BreakDerivedLink");
                        st.copies += usize::from(name == "DuplicateSheet" || name == "ImportSheet");
                        let stale_after = doc
                            .project()
                            .pieces
                            .iter()
                            .filter(|p| p.derived.as_ref().is_some_and(|d| d.stale.is_some()))
                            .count();
                        st.stale_events += stale_after.saturating_sub(stale_before);
                    }
                }
            } else if op < 93 {
                if r.chance(60) {
                    let expected = undo_stack.pop();
                    assert_eq!(doc.undo(), expected.is_some(), "{label}");
                    if let Some(state) = expected {
                        assert_eq!(snap(doc.project()), state, "annuler : {label}");
                        redo_stack.push(before_json);
                        st.undone += 1;
                    }
                } else {
                    let expected = redo_stack.pop();
                    assert_eq!(doc.redo(), expected.is_some(), "{label}");
                    if let Some(state) = expected {
                        assert_eq!(snap(doc.project()), state, "rétablir : {label}");
                        undo_stack.push(before_json);
                    }
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
            let now = doc.project().clone();
            check_derived_state(
                accepted_cmd.then_some(&cur),
                &now,
                &label,
                step % 3 == 0,
                &mut st,
            );
        }
    }
    eprintln!(
        "dérivées : {} créées, {} modifications, {} refus ({} « cassez le lien »), {} sans \
         changement, {} régénérations, {} passages en obsolète, {} reconstructions, {} liens \
         cassés, {} liens rompus par suppression de la mère, {} copies de feuille, {} \
         comparaisons exactes, {} contrôles de distance, {} annulations, {} relectures",
        st.created,
        st.accepted,
        st.refused,
        st.direct_refused,
        st.noops,
        st.regenerated,
        st.stale_events,
        st.rebuilt,
        st.broken,
        st.orphaned,
        st.copies,
        st.exact,
        st.distance,
        st.undone,
        st.reloaded
    );
    assert!(st.created >= 150, "seulement {} créations", st.created);
    assert!(
        st.accepted >= 600,
        "seulement {} modifications",
        st.accepted
    );
    assert!(st.refused >= 300, "seulement {} refus", st.refused);
    assert!(
        st.regenerated >= 300,
        "seulement {} régénérations",
        st.regenerated
    );
    assert!(
        st.stale_events >= 20,
        "seulement {} obsolescences",
        st.stale_events
    );
    assert!(
        st.exact >= 300,
        "seulement {} comparaisons exactes",
        st.exact
    );
    assert!(
        st.distance >= 500,
        "seulement {} contrôles de distance",
        st.distance
    );
    assert!(
        st.direct_refused >= 20,
        "seulement {} refus directs",
        st.direct_refused
    );
    assert!(
        st.broken >= 15 && st.copies >= 10 && st.orphaned >= 5,
        "couverture insuffisante : {} {} {}",
        st.broken,
        st.copies,
        st.orphaned
    );
    assert!(
        st.undone >= 100 && st.reloaded >= 80,
        "annuler / relire : {} / {}",
        st.undone,
        st.reloaded
    );
}
