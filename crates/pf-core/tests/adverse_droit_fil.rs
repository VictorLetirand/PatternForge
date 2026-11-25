//! Campagne adverse : droit fil (graine fixe).

mod adverse_support;
use adverse_support::*;

use pf_core::command::{Command, PieceSource, PointRef};
use pf_core::model::*;
use pf_core::{persist, DerivedKind, DerivedOp, Document, Project};
use pf_geometry::PointMm;
use std::panic::{catch_unwind, AssertUnwindSafe};

// ---------------------------------------------------------------------------
// US-06.5 — droit-fil avancé : direction par rapport à un tracé, biais, sens,
// tolérance de rotation, lignes supplémentaires
// ---------------------------------------------------------------------------

use pf_core::piece::PieceIssue;

use pf_core::{ExtraGrainLine, GrainDirection, GrainSense, PieceGrain, GRAIN_MAX_EXTRA_LINES};

fn gr_angle(r: &mut Rng) -> f64 {
    match r.below(10) {
        0..=3 => 45.0,
        4..=7 => -45.0,
        8 => f64::NAN,
        _ => r.unit() * 180.0 - 90.0,
    }
}

fn gr_tolerance(r: &mut Rng) -> Option<f64> {
    match r.below(12) {
        0..=3 => None,
        4 => Some(f64::NAN),
        5 => Some(-1.0),
        6 => Some(181.0),
        7 => Some(180.0),
        8 => Some(0.0),
        _ => Some(r.unit() * 180.0),
    }
}

fn gr_sense(r: &mut Rng) -> GrainSense {
    match r.below(3) {
        0 => GrainSense::Unmarked,
        1 => GrainSense::Nap,
        _ => GrainSense::Pattern,
    }
}

fn gr_direction(r: &mut Rng, refs: &[EntityId], any: &[EntityId]) -> GrainDirection {
    let reference = |r: &mut Rng| {
        // Presque toujours un tracé de construction ; parfois n'importe quel tracé.
        if r.chance(12) {
            pick(r, any, EntityId(BOGUS))
        } else {
            pick(r, refs, EntityId(BOGUS))
        }
    };
    match r.below(10) {
        0 | 1 => GrainDirection::Drawn {},
        2..=4 => GrainDirection::Parallel {
            reference: reference(r),
        },
        5 | 6 => GrainDirection::Perpendicular {
            reference: reference(r),
        },
        _ => GrainDirection::Bias {
            angle_deg: gr_angle(r),
        },
    }
}

fn gr_grain(r: &mut Rng, lines: &[EntityId], refs: &[EntityId], any: &[EntityId]) -> PieceGrain {
    let extra_count = match r.below(8) {
        0 => 0,
        1..=5 => 1 + r.below(3),
        6 => 1 + r.below(GRAIN_MAX_EXTRA_LINES),
        _ => GRAIN_MAX_EXTRA_LINES + 1,
    };
    // Lignes supplémentaires distinctes la plupart du temps (une fois sur dix : au hasard).
    let mut pool: Vec<EntityId> = lines.to_vec();
    let distinct = r.chance(90);
    let extra = (0..extra_count)
        .map(|_| {
            let entity = if distinct && !pool.is_empty() {
                pool.remove(r.below(pool.len()))
            } else {
                pick(r, lines, EntityId(BOGUS))
            };
            ExtraGrainLine {
                entity,
                direction: gr_direction(r, refs, any),
                sense: gr_sense(r),
                reversed: r.chance(25),
            }
        })
        .collect();
    PieceGrain {
        direction: gr_direction(r, refs, any),
        sense: gr_sense(r),
        reversed: r.chance(25),
        rotation_tolerance_deg: gr_tolerance(r),
        extra,
    }
}

/// Une commande de la campagne US-06.5 ; `None` : annuler, rétablir, relire.
fn gr_command(r: &mut Rng, cur: &Project, sheet: SheetId, op: usize) -> Option<Command> {
    let pieces: Vec<PieceId> = cur.pieces.iter().map(|p| p.id).collect();
    let piece = pick(r, &pieces, PieceId(BOGUS));
    let own = cur
        .piece_sheet(piece)
        .ok()
        .or_else(|| cur.sheet(sheet).ok());
    let own_id = own.map_or(sheet, |s| s.id);
    let any: Vec<EntityId> = own
        .map(|s| s.entities.iter().map(|e| e.id).collect())
        .unwrap_or_default();
    let lines: Vec<EntityId> = own
        .map(|s| {
            s.entities
                .iter()
                .filter(|e| e.role == PathRole::Grainline)
                .map(|e| e.id)
                .collect()
        })
        .unwrap_or_default();
    let refs: Vec<EntityId> = own
        .map(|s| {
            s.entities
                .iter()
                .filter(|e| e.role != PathRole::Grainline)
                .map(|e| e.id)
                .collect()
        })
        .unwrap_or_default();
    let points: Vec<PointId> = own
        .map(|s| s.points.iter().map(|p| p.id).collect())
        .unwrap_or_default();
    Some(match op {
        0..=29 => Command::SetPieceGrain {
            piece,
            grain: gr_grain(r, &lines, &refs, &any),
        },
        // Mêmes réglages (sans effet) ou tout par défaut (retire les réglages).
        30..=34 => Command::SetPieceGrain {
            piece,
            grain: cur
                .piece(piece)
                .ok()
                .and_then(|p| p.grain.clone())
                .unwrap_or_default(),
        },
        35..=38 => Command::SetPieceGrain {
            piece,
            grain: PieceGrain::default(),
        },
        39..=46 => Command::Delete {
            sheet: own_id,
            points: vec![],
            entities: some(r, &any, 2),
        },
        47..=52 => Command::MovePoint {
            sheet: own_id,
            point: pick(r, &points, PointId(BOGUS)),
            to: PointMm::new(r.unit() * 220.0 - 10.0, r.unit() * 170.0 - 10.0),
        },
        53..=56 => Command::SetPathRole {
            sheet: own_id,
            entity: pick(r, &any, EntityId(BOGUS)),
            role: match r.below(3) {
                0 => PathRole::Grainline,
                1 => PathRole::Construction,
                _ => PathRole::Fold,
            },
        },
        57..=60 => Command::SplitPath {
            sheet: own_id,
            entity: pick(r, &any, EntityId(BOGUS)),
            at: PointMm::new(r.unit() * 200.0, r.unit() * 150.0),
        },
        61..=63 => Command::MergePaths {
            sheet: own_id,
            first: pick(r, &any, EntityId(BOGUS)),
            second: pick(r, &any, EntityId(BOGUS)),
        },
        64..=65 => Command::ReversePath {
            sheet: own_id,
            entity: pick(r, &any, EntityId(BOGUS)),
        },
        66..=71 => Command::SetPiecePlacement {
            piece,
            placement: if r.chance(15) {
                None
            } else {
                Some(pf_core::Placement {
                    x_mm: r.unit() * 400.0,
                    y_mm: r.unit() * 400.0,
                    rotation_deg: if r.chance(30) {
                        wild_rotation(r)
                    } else {
                        [0.0, 30.0, 90.0, 180.0, 270.0, 330.0][r.below(6)]
                    },
                })
            },
        },
        72..=75 => Command::SetPieceOutline {
            piece,
            outline: cur
                .piece(piece)
                .map(|p| p.outline.clone())
                .unwrap_or_default(),
            grainline: if r.chance(15) {
                None
            } else {
                Some(pick(r, &lines, EntityId(BOGUS)))
            },
        },
        76..=78 => Command::DuplicateSheet { sheet: own_id },
        79..=83 => {
            let (a, b) = sj_line(r, cur, piece);
            Command::SplitPiece { piece, a, b }
        }
        84..=86 => {
            let second = pick(r, &pieces, PieceId(BOGUS));
            let (first_edge, second_edge) =
                sj_matching_edges(cur, piece, second).unwrap_or((EntityId(BOGUS), EntityId(BOGUS)));
            Command::JoinPieces {
                first: piece,
                first_edge,
                second,
                second_edge,
                tolerance_mm: sj_tolerance(r),
            }
        }
        87..=88 => Command::CreateDerivedPiece {
            mother: piece,
            kind: DerivedKind::Lining,
            operation: DerivedOp::Offset {
                distance_mm: -2.0 - r.unit() * 10.0,
            },
            name: None,
        },
        89 => Command::DeletePiece { piece },
        90..=93 => Command::AddSegment {
            sheet: own_id,
            from: PointRef::New(PointMm::new(r.unit() * 200.0, r.unit() * 150.0)),
            to: PointRef::New(PointMm::new(r.unit() * 200.0, r.unit() * 150.0)),
        },
        _ => return None,
    })
}

#[derive(Default)]
struct GrainStats {
    accepted: usize,
    refused: usize,
    noops: usize,
    with_grain: usize,
    extras: usize,
    parallel: usize,
    perpendicular: usize,
    biases: usize,
    naps: usize,
    patterns: usize,
    detached_refs: usize,
    lines_removed: usize,
    grain_removed: usize,
    frozen: usize,
    split_lost: usize,
    splits_carried: usize,
    joins: usize,
    copies: usize,
    undone: usize,
    reloaded: usize,
    exports: usize,
    rotation_checks: usize,
    exceeded: usize,
    oracle_checked: usize,
}

/// Direction unitaire attendue, calculée indépendamment du code testé.
fn gr_expected_direction(
    sheet: &Sheet,
    entity: EntityId,
    spec: &pf_core::GrainLineSpec,
) -> Option<(f64, f64)> {
    let chord = |e: EntityId| -> Option<(f64, f64)> {
        let (p, q) = sheet.entity_points(e).ok()?;
        let (a, b) = (sheet.position(p).ok()?, sheet.position(q).ok()?);
        let n = (b.x - a.x).hypot(b.y - a.y);
        (n > 1e-6).then(|| ((b.x - a.x) / n, (b.y - a.y) / n))
    };
    let (ux, uy) = match spec.direction {
        GrainDirection::Drawn {} => chord(entity)?,
        GrainDirection::Parallel { reference } => chord(reference)?,
        GrainDirection::Perpendicular { reference } => {
            let (x, y) = chord(reference)?;
            (-y, x)
        }
        GrainDirection::Bias { angle_deg } => {
            let a = angle_deg.to_radians();
            (a.cos(), a.sin())
        }
    };
    Some(if spec.reversed { (-ux, -uy) } else { (ux, uy) })
}

fn check_grain_state(now: &Project, label: &str, st: &mut GrainStats) {
    for p in &now.pieces {
        let lines = now.piece_grain_lines(p.id);
        if let Some(g) = &p.grain {
            st.with_grain += 1;
            assert!(!g.is_default(), "réglages par défaut stockés : {label}");
            assert!(p.grainline.is_some(), "réglages sans droit-fil : {label}");
            assert!(g.extra.len() <= GRAIN_MAX_EXTRA_LINES, "{label}");
            assert_eq!(lines.len(), 1 + g.extra.len(), "ligne illisible : {label}");
            st.extras += g.extra.len();
            let main = p.grainline.unwrap();
            let sheet = now
                .sheets
                .iter()
                .find(|s| s.entity(main).is_ok())
                .unwrap_or_else(|| panic!("tracé du droit-fil absent : {label}"));
            let mut seen = vec![main];
            for e in &g.extra {
                assert!(!seen.contains(&e.entity), "ligne en double : {label}");
                seen.push(e.entity);
            }
            let mut specs = vec![g.main_spec()];
            specs.extend(g.extra.iter().map(|e| e.spec()));
            for (l, spec) in lines.iter().zip(&specs) {
                // Jamais de référence morte, ni vers une ligne de la pièce.
                if let Some(rf) = spec.direction.reference() {
                    assert!(sheet.entity(rf).is_ok(), "référence morte : {label}");
                    assert!(!seen.contains(&rf), "référence vers soi : {label}");
                }
                match spec.direction {
                    GrainDirection::Parallel { .. } => st.parallel += 1,
                    GrainDirection::Perpendicular { .. } => st.perpendicular += 1,
                    GrainDirection::Bias { .. } => st.biases += 1,
                    GrainDirection::Drawn {} => {}
                }
                match spec.sense {
                    GrainSense::Nap => st.naps += 1,
                    GrainSense::Pattern => st.patterns += 1,
                    GrainSense::Unmarked => {}
                }
                assert!(
                    matches!(sheet.shape(l.entity), Ok(Shape::Line(..)))
                        && sheet.entity(l.entity).unwrap().role == PathRole::Grainline,
                    "ligne qui n'est pas un segment de droit-fil : {label}"
                );
                // Oracle indépendant : direction, longueur, milieu.
                let (ex, ey) = gr_expected_direction(sheet, l.entity, spec)
                    .unwrap_or_else(|| panic!("direction illisible : {label}"));
                let n = (l.to.x - l.from.x).hypot(l.to.y - l.from.y);
                assert!(n > 0.0, "flèche nulle : {label}");
                let (gx, gy) = ((l.to.x - l.from.x) / n, (l.to.y - l.from.y) / n);
                assert!(
                    (gx - ex).abs() < 1e-9 && (gy - ey).abs() < 1e-9,
                    "direction {gx},{gy} attendue {ex},{ey} : {label}"
                );
                let (p0, p1) = sheet.entity_points(l.entity).unwrap();
                let (a, b) = (sheet.position(p0).unwrap(), sheet.position(p1).unwrap());
                let drawn_len = (b.x - a.x).hypot(b.y - a.y);
                assert!((n - drawn_len).abs() < 1e-6, "longueur : {label}");
                assert!(
                    ((l.from.x + l.to.x) / 2.0 - (a.x + b.x) / 2.0).abs() < 1e-6
                        && ((l.from.y + l.to.y) / 2.0 - (a.y + b.y) / 2.0).abs() < 1e-6,
                    "milieu : {label}"
                );
                st.oracle_checked += 1;
            }
        }
        // Contrôle de rotation : recalculé par les angles, sans la formule du code.
        if let (Some(placement), false) = (p.placement, lines.is_empty()) {
            let r = now
                .piece_grain_rotation(p.id)
                .unwrap_or_else(|| panic!("contrôle absent : {label}"));
            let tolerance = p
                .grain
                .as_ref()
                .and_then(|g| g.rotation_tolerance_deg)
                .unwrap_or(0.0);
            let theta = placement.rotation_deg.to_radians();
            let mut worst = 0.0_f64;
            for l in &lines {
                let (dx, dy) = (l.to.x - l.from.x, l.to.y - l.from.y);
                let (rx, ry) = (
                    theta.cos() * dx - theta.sin() * dy,
                    theta.sin() * dx + theta.cos() * dy,
                );
                let mut angle = (dx * ry - dy * rx)
                    .atan2(dx * rx + dy * ry)
                    .abs()
                    .to_degrees();
                if l.spec.sense == GrainSense::Unmarked && angle > 90.0 {
                    angle = 180.0 - angle;
                }
                worst = worst.max(angle);
            }
            assert!(
                (r.deviation_deg - worst).abs() < 1e-6,
                "déviation {} attendue {worst} : {label}",
                r.deviation_deg
            );
            assert!((r.tolerance_deg - tolerance).abs() < 1e-12, "{label}");
            // Les cas limites sont vérifiés exactement par `piece_grain.rs` ; ici, hors d'un
            // voisinage de 1e-6 de la tolérance, le verdict doit être celui de l'oracle.
            if (worst - tolerance).abs() > 1e-6 {
                assert_eq!(r.exceeded(), worst > tolerance, "verdict : {label}");
                assert_eq!(
                    now.piece_issues(p.id)
                        .contains(&PieceIssue::GrainRotationExceeded),
                    worst > tolerance,
                    "avertissement : {label}"
                );
            }
            st.rotation_checks += 1;
            st.exceeded += usize::from(r.exceeded());
        } else {
            assert!(
                !now.piece_issues(p.id)
                    .contains(&PieceIssue::GrainRotationExceeded),
                "avertissement sans placement manuel : {label}"
            );
        }
    }
}

fn check_grain_export(p: &Project, label: &str, st: &mut GrainStats) {
    use pf_export::{svg::export_svg, ExportError, Scene};
    for s in &p.sheets {
        let scene = match catch_unwind(AssertUnwindSafe(|| Scene::from_sheet(p, s.id)))
            .unwrap_or_else(|_| panic!("{label} : PANIC dans Scene::from_sheet"))
        {
            Ok(sc) => sc,
            Err(ExportError::EmptySheet(_)) => continue,
            Err(e) => panic!("{label} : scène refusée : {e}"),
        };
        let marked = p
            .sheet_grain_lines(s.id)
            .iter()
            .filter(|l| l.spec.sense != GrainSense::Unmarked)
            .count();
        assert_eq!(
            scene
                .paths
                .iter()
                .filter(|x| x.sense != GrainSense::Unmarked)
                .count(),
            marked,
            "{label}"
        );
        if let Ok(svg) = export_svg(&scene) {
            if marked == 0 {
                assert!(!svg.contains("data-grain-sense"), "{label}");
            }
        }
    }
    check_export(p, label);
    st.exports += 1;
}

#[test]
fn us065_campagne_aleatoire_du_droit_fil_graine_fixe() {
    let mut st = GrainStats::default();
    for seq in 0..170u64 {
        let mut r = Rng(0x06_0500 + seq);
        let mut project = Project::new("Droit-fil aléa");
        let sheet = project.sheets[0].id;
        project
            .apply(Command::AddRectangle {
                sheet,
                corner: PointRef::New(PointMm::new(0.0, 0.0)),
                width_mm: 200.0,
                height_mm: 150.0,
            })
            .unwrap();
        let base = project
            .apply(Command::ExtractPiece {
                sheet,
                source: PieceSource::Click {
                    at: PointMm::new(100.0, 75.0),
                },
                name: "Base".into(),
                detached: false,
            })
            .unwrap()
            .pieces[0];
        let add_seg = |project: &mut Project, r: &mut Rng, role: PathRole| -> Option<EntityId> {
            let e = project
                .apply(Command::AddSegment {
                    sheet,
                    from: PointRef::New(PointMm::new(r.unit() * 200.0, r.unit() * 150.0)),
                    to: PointRef::New(PointMm::new(r.unit() * 200.0, r.unit() * 150.0)),
                })
                .ok()?
                .entities[0];
            project
                .apply(Command::SetPathRole {
                    sheet,
                    entity: e,
                    role,
                })
                .ok()?;
            Some(e)
        };
        // Droit-fil principal et lignes de droit-fil / tracés de référence au hasard.
        let main = add_seg(&mut project, &mut r, PathRole::Grainline);
        let outline = project.piece(base).unwrap().outline.clone();
        let _ = project.apply(Command::SetPieceOutline {
            piece: base,
            outline,
            grainline: main,
        });
        for _ in 0..(2 + r.below(4)) {
            add_seg(&mut project, &mut r, PathRole::Grainline);
        }
        for _ in 0..(2 + r.below(3)) {
            add_seg(&mut project, &mut r, PathRole::Construction);
        }
        if r.chance(40) {
            let _ = project.apply(Command::AddCircle {
                sheet,
                center: PointRef::New(PointMm::new(r.unit() * 200.0, r.unit() * 150.0)),
                radius_mm: 5.0 + r.unit() * 30.0,
            });
        }
        random_drawing(&mut r, &mut project, sheet);
        if r.chance(35) {
            let _ = project.apply(Command::ExtractPiece {
                sheet,
                source: PieceSource::Click {
                    at: PointMm::new(r.unit() * 200.0, r.unit() * 150.0),
                },
                name: "Seconde".into(),
                detached: r.chance(50),
            });
        }
        if r.chance(30) {
            let _ = project.apply(Command::CreateDerivedPiece {
                mother: base,
                kind: DerivedKind::Lining,
                operation: DerivedOp::Offset {
                    distance_mm: -2.0 - r.unit() * 15.0,
                },
                name: None,
            });
        }
        // Quelques réglages au départ : de la matière pour les autres commandes.
        for _ in 0..(1 + r.below(4)) {
            let s = project.sheet(sheet).unwrap();
            let ents: Vec<EntityId> = s.entities.iter().map(|e| e.id).collect();
            let lines: Vec<EntityId> = s
                .entities
                .iter()
                .filter(|e| e.role == PathRole::Grainline)
                .map(|e| e.id)
                .collect();
            let refs: Vec<EntityId> = s
                .entities
                .iter()
                .filter(|e| e.role != PathRole::Grainline)
                .map(|e| e.id)
                .collect();
            let grain = gr_grain(&mut r, &lines, &refs, &ents);
            let _ = project.apply(Command::SetPieceGrain { piece: base, grain });
        }
        let initial_json = snap(&project);
        let mut doc = Document::new(project);
        let mut undo_stack: Vec<String> = Vec::new();
        let mut redo_stack: Vec<String> = Vec::new();
        for step in 0..24 {
            let label = format!("graine {seq} pas {step}");
            let before_json = snap(doc.project());
            let cur = doc.project().clone();
            let op = r.below(100);
            if let Some(cmd) = gr_command(&mut r, &cur, sheet, op) {
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
                    }
                    Ok(out) => {
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
                        let count =
                            |f: &dyn Fn(&str) -> bool| out.notes.iter().filter(|n| f(n)).count();
                        st.detached_refs += count(&|n| n.contains("suivait un tracé supprimé"));
                        st.lines_removed += count(&|n| {
                            n.contains("ligne de droit-fil supplémentaire") && n.contains("retirée")
                        });
                        st.grain_removed += count(&|n| {
                            n.contains("réglages du droit-fil") && n.contains("retirés")
                        });
                        st.frozen += count(&|n| n.contains("figée"));
                        st.split_lost +=
                            count(&|n| n.contains("ligne(s) de droit-fil supplémentaire(s)"));
                        let after = doc.project();
                        let carried = || {
                            out.pieces
                                .iter()
                                .filter(|id| after.piece(**id).is_ok_and(|p| p.grain.is_some()))
                                .count()
                        };
                        match name.as_str() {
                            "SplitPiece" => st.splits_carried += carried(),
                            "JoinPieces" => st.joins += carried(),
                            "DuplicateSheet" => st.copies += carried(),
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
            check_grain_state(&doc.project().clone(), &label, &mut st);
            if step % 4 == 3 {
                check_grain_export(doc.project(), &label, &mut st);
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
        "droit-fil : {} modifications, {} refus, {} sans changement ; vus : {} pièces à réglages, \
         {} lignes supplémentaires, {} parallèles, {} perpendiculaires, {} biais, {} poil, {} \
         motif ; {} références détachées, {} lignes retirées, {} réglages retirés, {} figeages, {} \
         lignes perdues à la séparation, {} reprises par séparation, {} par assemblage, {} par \
         copie ; {} annulations, {} relectures, {} exports, {} contrôles de rotation ({} \
         dépassés), {} flèches recalculées indépendamment",
        st.accepted,
        st.refused,
        st.noops,
        st.with_grain,
        st.extras,
        st.parallel,
        st.perpendicular,
        st.biases,
        st.naps,
        st.patterns,
        st.detached_refs,
        st.lines_removed,
        st.grain_removed,
        st.frozen,
        st.split_lost,
        st.splits_carried,
        st.joins,
        st.copies,
        st.undone,
        st.reloaded,
        st.exports,
        st.rotation_checks,
        st.exceeded,
        st.oracle_checked
    );
    assert!(
        st.accepted >= 600,
        "seulement {} modifications",
        st.accepted
    );
    assert!(st.refused >= 300, "seulement {} refus", st.refused);
    assert!(
        st.oracle_checked >= 1000,
        "seulement {} flèches vérifiées",
        st.oracle_checked
    );
    assert!(
        st.parallel >= 100 && st.perpendicular >= 100 && st.biases >= 100,
        "directions insuffisamment couvertes"
    );
    assert!(
        st.naps >= 100 && st.patterns >= 100,
        "sens insuffisamment couverts"
    );
    assert!(
        st.detached_refs >= 1 && st.lines_removed >= 5 && st.grain_removed >= 5,
        "retraits : {} références, {} lignes, {} réglages",
        st.detached_refs,
        st.lines_removed,
        st.grain_removed
    );
    assert!(
        st.splits_carried >= 10 && st.copies >= 10 && st.frozen >= 10,
        "séparations : {}, copies : {}, figeages : {}",
        st.splits_carried,
        st.copies,
        st.frozen
    );
    assert!(
        st.rotation_checks >= 300 && st.exceeded >= 100,
        "contrôles de rotation insuffisants"
    );
    assert!(
        st.undone >= 100 && st.reloaded >= 40 && st.exports >= 1000,
        "couverture insuffisante"
    );
}
