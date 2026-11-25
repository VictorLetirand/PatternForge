//! Campagne adverse : lignes speciales (graine fixe).

mod adverse_support;
use adverse_support::*;

use pf_core::command::{Command, PieceSource, PointRef};
use pf_core::model::*;
use pf_core::{contour, persist, DerivedKind, DerivedOp, Document, Project};
use pf_geometry::{PointMm, Transform};
use std::panic::{catch_unwind, AssertUnwindSafe};

// ---------------------------------------------------------------------------
// US-06.8 — lignes spéciales : surpiqûre liée à un bord
// ---------------------------------------------------------------------------

#[derive(Default)]
struct LineStats {
    accepted: usize,
    refused: usize,
    noops: usize,
    undone: usize,
    reloaded: usize,
    exports: usize,
    created: usize,
    regenerated: usize,
    stale_notes: usize,
    stale_seen: usize,
    broken: usize,
    open_links: usize,
    closed_links: usize,
    piece_links: usize,
    oracle_checked: usize,
    copies: usize,
    role_changes: usize,
    must_refuse: usize,
    locked_refusals: usize,
}

fn ls_distance(r: &mut Rng) -> f64 {
    match r.below(14) {
        0..=8 => 1.0 + r.unit() * 15.0,
        9 | 10 => r.unit() * 70.0,
        _ => [
            0.1,
            1000.0,
            0.099,
            1000.1,
            0.0,
            -2.0,
            f64::NAN,
            f64::INFINITY,
            1e-300,
            5e8,
        ][r.below(10)],
    }
}

fn ls_distance_valid(d: f64) -> bool {
    d.is_finite() && (0.1..=1000.0).contains(&d)
}

fn ls_side(r: &mut Rng) -> pf_core::TopstitchSide {
    use pf_core::TopstitchSide::*;
    [Inside, Outside, Left, Right][r.below(4)]
}

fn ls_source(r: &mut Rng, cur: &Project, s: &Sheet) -> pf_core::TopstitchSource {
    use pf_core::TopstitchSource;
    let ents: Vec<EntityId> = s.entities.iter().map(|e| e.id).collect();
    let pieces: Vec<PieceId> = cur.pieces.iter().map(|p| p.id).collect();
    match r.below(7) {
        0 | 1 => TopstitchSource::Paths {
            paths: vec![pick(r, &ents, EntityId(BOGUS))],
            from: None,
        },
        2 | 3 => {
            let closed = contour::closed_contours(s);
            let paths = if closed.is_empty() {
                some(r, &ents, 3)
            } else {
                closed[r.below(closed.len())]
                    .edges
                    .iter()
                    .map(|e| e.entity)
                    .collect()
            };
            TopstitchSource::Paths { paths, from: None }
        }
        4 => TopstitchSource::Piece {
            piece: pick(r, &pieces, PieceId(BOGUS)),
        },
        5 => {
            let start = pick(r, &ents, EntityId(BOGUS));
            let paths = pf_core::offset::detect_chain(s, start).unwrap_or_else(|_| vec![start]);
            let pts: Vec<PointId> = s.points.iter().map(|p| p.id).collect();
            let from = r.chance(30).then(|| pick(r, &pts, PointId(BOGUS)));
            TopstitchSource::Paths { paths, from }
        }
        _ => TopstitchSource::Paths {
            paths: some(r, &ents, 3),
            from: None,
        },
    }
}

fn ls_command(r: &mut Rng, cur: &Project, sheet: SheetId, op: usize) -> Option<Command> {
    let s = cur.sheet(sheet).ok()?;
    let ents: Vec<EntityId> = s.entities.iter().map(|e| e.id).collect();
    let pts: Vec<PointId> = s.points.iter().map(|p| p.id).collect();
    let links: Vec<pf_core::TopstitchId> = s.topstitches.iter().map(|t| t.id).collect();
    let pieces: Vec<PieceId> = cur.pieces.iter().map(|p| p.id).collect();
    let generated: Vec<EntityId> = s
        .topstitches
        .iter()
        .flat_map(|t| t.paths.iter().copied())
        .collect();
    Some(match op {
        0..=21 => Command::AddTopstitch {
            sheet,
            source: ls_source(r, cur, s),
            distance_mm: ls_distance(r),
            side: ls_side(r),
        },
        22..=31 => Command::SetTopstitch {
            sheet,
            topstitch: pick(r, &links, pf_core::TopstitchId(BOGUS)),
            distance_mm: ls_distance(r),
            side: ls_side(r),
        },
        32..=36 => Command::BreakTopstitchLink {
            sheet,
            topstitch: pick(r, &links, pf_core::TopstitchId(BOGUS)),
        },
        37..=48 => Command::MovePoint {
            sheet,
            point: pick(r, &pts, PointId(BOGUS)),
            to: if r.chance(80) {
                PointMm::new(r.unit() * 220.0, r.unit() * 170.0)
            } else {
                pt_mm(r)
            },
        },
        49..=53 => Command::Transform {
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
        54..=60 => Command::SetPathRole {
            sheet,
            entity: if r.chance(35) && !generated.is_empty() {
                generated[r.below(generated.len())]
            } else {
                pick(r, &ents, EntityId(BOGUS))
            },
            role: PathRole::ALL[r.below(PathRole::ALL.len())],
        },
        61..=65 => Command::Delete {
            sheet,
            points: if r.chance(30) {
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
        66..=68 => {
            let entity = pick(r, &ents, EntityId(BOGUS));
            Command::SplitPath {
                sheet,
                entity,
                at: ls_point_on(r, s, entity),
            }
        }
        69..=70 => Command::MergePaths {
            sheet,
            first: pick(r, &ents, EntityId(BOGUS)),
            second: pick(r, &ents, EntityId(BOGUS)),
        },
        71..=72 => Command::ReversePath {
            sheet,
            entity: pick(r, &ents, EntityId(BOGUS)),
        },
        73..=75 => Command::DuplicateSheet { sheet },
        76..=78 => Command::SplitPiece {
            piece: pick(r, &pieces, PieceId(BOGUS)),
            a: pt_mm(r),
            b: PointMm::new(r.unit() * 200.0, r.unit() * 150.0),
        },
        79..=80 => Command::DeletePiece {
            piece: pick(r, &pieces, PieceId(BOGUS)),
        },
        81..=85 => {
            if s.layers.is_empty() || r.chance(20) {
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
                } else {
                    let pool = if r.chance(50) && !generated.is_empty() {
                        generated.clone()
                    } else {
                        ents.clone()
                    };
                    Command::SetObjectsLayer {
                        sheet,
                        points: Vec::new(),
                        entities: some(r, &pool, 3),
                        layer: if r.chance(80) {
                            Some(pick(r, &layers, LayerId(BOGUS)))
                        } else {
                            None
                        },
                    }
                }
            }
        }
        86..=88 => {
            let closed = contour::closed_contours(s);
            let outline: Vec<PieceEdge> = if closed.is_empty() {
                Vec::new()
            } else {
                closed[r.below(closed.len())]
                    .edges
                    .iter()
                    .map(|e| PieceEdge {
                        entity: e.entity,
                        reversed: e.reversed,
                    })
                    .collect()
            };
            Command::SetPieceOutline {
                piece: pick(r, &pieces, PieceId(BOGUS)),
                outline,
                grainline: None,
            }
        }
        89..=91 => Command::CreateDerivedPiece {
            mother: pick(r, &pieces, PieceId(BOGUS)),
            kind: DerivedKind::Lining,
            operation: DerivedOp::Offset {
                distance_mm: if r.chance(50) {
                    -(2.0 + r.unit() * 12.0)
                } else {
                    2.0 + r.unit() * 12.0
                },
            },
            name: None,
        },
        92..=94 => Command::Batch {
            commands: (0..2)
                .map(|_| Command::MovePoint {
                    sheet,
                    point: pick(r, &pts, PointId(BOGUS)),
                    to: PointMm::new(r.unit() * 220.0, r.unit() * 170.0),
                })
                .collect(),
        },
        _ => return None,
    })
}

/// Mesure indépendante : une surpiqûre à jour dont le bord n'a que des segments, des arcs ou des
/// cercles (distances exactes) est partout à au moins `d` du bord (1e-3 mm près) et le touche à
/// `d` en au moins un point d'échantillon.
fn ls_check_state(now: &Project, label: &str, st: &mut LineStats) {
    for s in &now.sheets {
        for t in &s.topstitches {
            assert!(!t.paths.is_empty(), "{label} : surpiqûre sans ligne");
            for &e in &t.paths {
                let en = s
                    .entity(e)
                    .unwrap_or_else(|_| panic!("{label} : ligne générée absente"));
                assert_eq!(
                    en.role,
                    PathRole::Topstitch,
                    "{label} : rôle d'une ligne liée"
                );
            }
            if t.stale.is_some() {
                st.stale_seen += 1;
                continue;
            }
            let edge: Vec<EntityId> = match &t.source {
                pf_core::TopstitchSource::Paths { paths, .. } => paths.clone(),
                pf_core::TopstitchSource::Piece { piece } => now
                    .piece(*piece)
                    .unwrap_or_else(|_| panic!("{label} : pièce source absente"))
                    .outline
                    .iter()
                    .map(|e| e.entity)
                    .collect(),
            };
            let exact = edge.iter().all(|&e| {
                matches!(
                    s.shape(e),
                    Ok(Shape::Line(..)) | Ok(Shape::Arc(_)) | Ok(Shape::Circle(_))
                )
            });
            if !exact || edge.is_empty() {
                continue;
            }
            let mut touched = false;
            for &g in &t.paths {
                let shape = s.shape(g).expect("forme générée");
                // Un segment peut être le biseau d'un coin trop pointu (limite de pointe de
                // `offset_chain`) : son milieu est alors plus près du bord que `d`. Pour un
                // segment, seules ses extrémités sont donc bornées par le bas ; ses autres
                // points servent à trouver la distance `d`.
                let (samples, bounded_from): (Vec<PointMm>, usize) = match &shape {
                    Shape::Line(a, b) => (
                        (0..=16)
                            .map(|i| {
                                let k = f64::from(i) / 16.0;
                                PointMm::new(a.x + (b.x - a.x) * k, a.y + (b.y - a.y) * k)
                            })
                            .collect(),
                        usize::MAX,
                    ),
                    other => (other.sample(64), 0),
                };
                let last = samples.len() - 1;
                for (i, q) in samples.iter().enumerate() {
                    let dd = edge
                        .iter()
                        .map(|&e| s.shape(e).unwrap().distance_to_point(*q))
                        .fold(f64::INFINITY, f64::min);
                    let bounded = bounded_from == 0 || i == 0 || i == last;
                    assert!(
                        !bounded || dd >= t.distance_mm - 1e-3,
                        "{label} : un point de la surpiqûre est à {dd} mm du bord (< {})",
                        t.distance_mm
                    );
                    if (dd - t.distance_mm).abs() <= 1e-3 {
                        touched = true;
                    }
                }
            }
            assert!(
                touched,
                "{label} : la surpiqûre n'est nulle part à sa distance"
            );
            st.oracle_checked += 1;
        }
    }
}

#[test]
fn us068_campagne_aleatoire_des_lignes_speciales_graine_fixe() {
    let mut st = LineStats::default();
    for seq in 0..160u64 {
        let mut r = Rng(0x06_0800 + seq);
        let mut project = Project::new("Lignes spéciales aléa");
        let sheet = project.sheets[0].id;
        let rect = project
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
        if r.chance(60) {
            random_drawing(&mut r, &mut project, sheet);
        }
        // Des surpiqûres au départ : de la matière pour les autres commandes.
        for _ in 0..r.below(3) {
            let source = match r.below(3) {
                0 => pf_core::TopstitchSource::Piece { piece: base },
                1 => pf_core::TopstitchSource::Paths {
                    paths: rect.entities.clone(),
                    from: None,
                },
                _ => {
                    let e = project.sheet(sheet).unwrap().entities.clone();
                    let pick_e = e[r.below(e.len())].id;
                    pf_core::TopstitchSource::Paths {
                        paths: vec![pick_e],
                        from: None,
                    }
                }
            };
            let side = if matches!(source, pf_core::TopstitchSource::Piece { .. }) || r.chance(50) {
                if r.chance(50) {
                    pf_core::TopstitchSide::Inside
                } else {
                    pf_core::TopstitchSide::Outside
                }
            } else {
                pf_core::TopstitchSide::Left
            };
            let _ = project.apply(Command::AddTopstitch {
                sheet,
                source,
                distance_mm: 1.0 + r.unit() * 12.0,
                side,
            });
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
            if let Some(cmd) = ls_command(&mut r, &cur, sheet, op) {
                let name = variant_name(&cmd);
                // Entrée qui doit être refusée à coup sûr : distance hors bornes.
                let must_refuse = match &cmd {
                    Command::AddTopstitch { distance_mm, .. }
                    | Command::SetTopstitch { distance_mm, .. } => !ls_distance_valid(*distance_mm),
                    _ => false,
                };
                let locked_before = cur.sheets.iter().any(|s| s.layers.iter().any(|l| l.locked));
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
                        if must_refuse {
                            st.must_refuse += 1;
                        }
                        if locked_before && e.to_string().contains("verrouillé") {
                            st.locked_refusals += 1;
                        }
                    }
                    Ok(out) => {
                        assert!(
                            !must_refuse,
                            "{name} : une distance hors bornes a été acceptée : {label}"
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
                        st.created += out.topstitches.len();
                        st.regenerated += out.regenerated_topstitches.len();
                        st.stale_notes += out
                            .notes
                            .iter()
                            .filter(|n| n.contains("surpiqûre") && n.contains("pas pu être mise"))
                            .count();
                        st.broken += out
                            .notes
                            .iter()
                            .filter(|n| n.contains("n'est plus liée"))
                            .count();
                        match name.as_str() {
                            "DuplicateSheet" => st.copies += 1,
                            "SetPathRole" => st.role_changes += 1,
                            _ => {}
                        }
                        // Les liens créés par la commande sont comptés par forme.
                        for t in &out.topstitches {
                            let sh = doc.project().sheet(sheet).unwrap();
                            if let Ok(l) = sh.topstitch(*t) {
                                match &l.source {
                                    pf_core::TopstitchSource::Piece { .. } => st.piece_links += 1,
                                    pf_core::TopstitchSource::Paths { from: Some(_), .. } => {
                                        st.open_links += 1
                                    }
                                    pf_core::TopstitchSource::Paths { from: None, .. } => {
                                        st.closed_links += 1
                                    }
                                }
                            }
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
            ls_check_state(&doc.project().clone(), &label, &mut st);
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
        "lignes spéciales : {} modifications, {} refus ({} distances hors bornes, {} calques \
         verrouillés), {} sans changement ; {} surpiqûres créées ({} sur bord ouvert, {} sur bord \
         fermé, {} sur pièce), {} régénérations, {} états obsolètes signalés ({} vus), {} liens \
         cassés, {} copies de feuille, {} changements de rôle ; {} annulations, {} relectures, {} \
         exports, {} surpiqûres mesurées indépendamment",
        st.accepted,
        st.refused,
        st.must_refuse,
        st.locked_refusals,
        st.noops,
        st.created,
        st.open_links,
        st.closed_links,
        st.piece_links,
        st.regenerated,
        st.stale_notes,
        st.stale_seen,
        st.broken,
        st.copies,
        st.role_changes,
        st.undone,
        st.reloaded,
        st.exports,
        st.oracle_checked
    );
    assert!(
        st.accepted >= 800,
        "seulement {} modifications",
        st.accepted
    );
    assert!(st.refused >= 400, "seulement {} refus", st.refused);
    assert!(
        st.must_refuse >= 50,
        "seulement {} distances invalides refusées",
        st.must_refuse
    );
    assert!(
        st.created >= 150 && st.regenerated >= 300,
        "{} créations, {} régénérations",
        st.created,
        st.regenerated
    );
    assert!(
        st.oracle_checked >= 1000,
        "seulement {} surpiqûres mesurées",
        st.oracle_checked
    );
    assert!(
        st.stale_notes >= 5 && st.broken >= 10,
        "{} obsolescences, {} liens cassés",
        st.stale_notes,
        st.broken
    );
    assert!(
        st.piece_links >= 10 && st.closed_links >= 20 && st.open_links >= 10,
        "formes de bord : {} pièces, {} fermés, {} ouverts",
        st.piece_links,
        st.closed_links,
        st.open_links
    );
    assert!(
        st.copies >= 10 && st.role_changes >= 50,
        "{} copies, {} rôles",
        st.copies,
        st.role_changes
    );
    assert!(
        st.undone >= 100 && st.reloaded >= 25 && st.exports >= 600,
        "couverture insuffisante"
    );
    assert!(st.locked_refusals >= 1, "aucun refus de calque verrouillé");
}
