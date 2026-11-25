//! Campagne adverse : marquages (graine fixe).

mod adverse_support;
use adverse_support::*;

use pf_core::command::{Command, PieceSource, PointRef};
use pf_core::model::*;
use pf_core::{persist, DerivedKind, DerivedOp, Document, Project};
use pf_geometry::PointMm;
use std::panic::{catch_unwind, AssertUnwindSafe};

// ---------------------------------------------------------------------------
// US-06.3 — perçages, pointages, croix et bouts de pince reculés
// ---------------------------------------------------------------------------

use pf_core::piece_marking::{
    MARKING_MAX_DIAMETER_MM, MARKING_MAX_SETBACK_MM, MARKING_MIN_CROSS_MM, MARKING_MIN_DIAMETER_MM,
    MARKING_MIN_SETBACK_MM,
};

use pf_core::{MarkingAnchor, MarkingShape, PieceMarking};

fn mk_shape(r: &mut Rng) -> MarkingShape {
    match r.below(3) {
        0 => MarkingShape::Punch,
        1 => MarkingShape::Drill {
            diameter_mm: mk_size(r, MARKING_MIN_DIAMETER_MM, MARKING_MAX_DIAMETER_MM),
        },
        _ => MarkingShape::Cross {
            size_mm: mk_size(r, MARKING_MIN_CROSS_MM, MARKING_MAX_DIAMETER_MM),
        },
    }
}

fn mk_anchor(r: &mut Rng, pts: &[PointId]) -> MarkingAnchor {
    if r.chance(50) {
        MarkingAnchor::Point {
            point: pick(r, pts, PointId(BOGUS)),
        }
    } else {
        let tip = pick(r, pts, PointId(BOGUS));
        MarkingAnchor::DartTip {
            tip,
            toward: if r.chance(6) {
                tip
            } else {
                pick(r, pts, PointId(BOGUS))
            },
            setback_mm: mk_size(r, MARKING_MIN_SETBACK_MM, MARKING_MAX_SETBACK_MM),
        }
    }
}

/// Une commande de la campagne US-06.3 (ou `None` : annuler, rétablir, relire).
fn mk_command(r: &mut Rng, cur: &Project, sheet: SheetId, op: usize) -> Option<Command> {
    let pieces: Vec<PieceId> = cur.pieces.iter().map(|p| p.id).collect();
    let piece = pick(r, &pieces, PieceId(BOGUS));
    let own = cur
        .piece_sheet(piece)
        .ok()
        .or_else(|| cur.sheet(sheet).ok());
    let pts: Vec<PointId> = own
        .map(|s| s.points.iter().map(|p| p.id).collect())
        .unwrap_or_default();
    let main_pts: Vec<PointId> = cur
        .sheet(sheet)
        .map(|s| s.points.iter().map(|p| p.id).collect())
        .unwrap_or_default();
    let known: Vec<PieceMarking> = cur
        .piece(piece)
        .map(|p| p.markings.clone())
        .unwrap_or_default();
    let known_id = |r: &mut Rng| {
        if !known.is_empty() && r.chance(88) {
            known[r.below(known.len())].id
        } else {
            MarkingId(BOGUS)
        }
    };
    let add = |r: &mut Rng, piece: PieceId, pts: &[PointId]| Command::AddPieceMarking {
        piece,
        at: mk_anchor(r, pts),
        shape: mk_shape(r),
    };
    Some(match op {
        0..=27 => add(r, piece, &pts),
        28..=39 => {
            let marking = known_id(r);
            match known.iter().find(|m| m.id == marking) {
                // Une fois sur trois : les mêmes valeurs (sans effet, aucune étape).
                Some(m) if r.chance(33) => Command::SetPieceMarking {
                    piece,
                    marking,
                    at: m.at,
                    shape: m.shape,
                },
                _ => Command::SetPieceMarking {
                    piece,
                    marking,
                    at: mk_anchor(r, &pts),
                    shape: mk_shape(r),
                },
            }
        }
        40..=47 => Command::RemovePieceMarking {
            piece,
            marking: known_id(r),
        },
        48..=57 => {
            let point = pick(r, &main_pts, PointId(BOGUS));
            // Parfois exactement sur un autre point : pointe et direction confondues.
            let onto = cur
                .sheet(sheet)
                .ok()
                .and_then(|s| s.position(pick(r, &main_pts, PointId(BOGUS))).ok());
            let to = match onto {
                Some(p) if r.chance(18) => p,
                _ => PointMm::new(r.unit() * 260.0 - 30.0, r.unit() * 210.0 - 30.0),
            };
            Command::MovePoint { sheet, point, to }
        }
        58..=63 => {
            // Surtout les points qui portent un marquage : ils doivent être retirés avec lui.
            let anchors: Vec<PointId> = cur
                .pieces
                .iter()
                .flat_map(|p| p.markings.iter().flat_map(|m| m.at.points()))
                .filter(|p| main_pts.contains(p))
                .collect();
            let points = if !anchors.is_empty() && r.chance(75) {
                some(r, &anchors, 2)
            } else {
                some(r, &main_pts, 2)
            };
            Command::Delete {
                sheet,
                points,
                entities: vec![],
            }
        }
        64..=66 => Command::DuplicateSheet { sheet },
        67..=70 => {
            let (a, b) = sj_line(r, cur, piece);
            Command::SplitPiece { piece, a, b }
        }
        71..=74 => {
            // Les deux moitiés d'une séparation, le long de leur arête de coupe.
            let pair = cur.pieces.iter().find_map(|a| {
                let twin = a.name.strip_suffix(" (1/2)")?;
                cur.pieces
                    .iter()
                    .find(|b| b.name == format!("{twin} (2/2)"))
                    .map(|b| (a.id, b.id))
            });
            let (a, b) = pair?;
            let (ea, eb) = sj_matching_edges(cur, a, b)?;
            Command::JoinPieces {
                first: a,
                first_edge: ea,
                second: b,
                second_edge: eb,
                tolerance_mm: 10.0,
            }
        }
        75..=77 => {
            let outline: Vec<EntityId> = cur
                .piece(piece)
                .map(|p| p.outline.iter().map(|e| e.entity).collect())
                .unwrap_or_default();
            Command::UnfoldPiece {
                piece,
                fold: pick(r, &outline, EntityId(BOGUS)),
                name: None,
            }
        }
        78..=81 => Command::CreateDerivedPiece {
            mother: piece,
            kind: DerivedKind::Lining,
            operation: DerivedOp::Offset {
                distance_mm: (r.unit() - 0.5) * 40.0,
            },
            name: None,
        },
        82..=87 => Command::Batch {
            commands: (0..(2 + r.below(3)))
                .map(|_| {
                    if r.chance(25) && !known.is_empty() {
                        Command::RemovePieceMarking {
                            piece,
                            marking: known_id(r),
                        }
                    } else {
                        add(r, piece, &pts)
                    }
                })
                .collect(),
        },
        _ => return None,
    })
}

#[derive(Default)]
struct MarkStats {
    accepted: usize,
    refused: usize,
    noops: usize,
    added: usize,
    darts: usize,
    drills: usize,
    crosses: usize,
    punches: usize,
    pruned: usize,
    outside: usize,
    splits_carried: usize,
    joins: usize,
    copies: usize,
    derived_with_marks: usize,
    /// Marquages d'une dérivée retirés parce que la régénération a recréé leur point.
    own_pruned: usize,
    undone: usize,
    reloaded: usize,
    exports: usize,
}

/// Invariants vérifiés après chaque pas : jamais de référence morte, identifiants
/// uniques, dimensions dans les bornes, positions finies et égales à un recalcul
/// indépendant, cohérence fiche / liste / avertissement, lectures sans panique.
fn check_marks_state(now: &Project, label: &str, st: &mut MarkStats) {
    use pf_core::piece::PieceIssue;
    let mut ids = std::collections::BTreeSet::new();
    for piece in &now.pieces {
        for m in &piece.markings {
            assert!(
                ids.insert(m.id.0),
                "identifiant de marquage en double : {label}"
            );
            assert!(m.id.0 < now.next_id, "{label}");
            assert!(
                m.shape.validate().is_ok(),
                "dimension hors bornes {m:?} : {label}"
            );
            let pts = m.at.points();
            let alive = match piece.link {
                PieceLink::Linked { sheet } => {
                    let s = now.sheet(sheet).unwrap();
                    pts.iter().all(|p| s.point(*p).is_ok())
                }
                PieceLink::Detached => now
                    .sheets
                    .iter()
                    .any(|s| pts.iter().all(|p| s.point(*p).is_ok())),
            };
            assert!(alive, "référence morte {m:?} : {label}");
            match m.shape {
                MarkingShape::Punch => st.punches += 1,
                MarkingShape::Drill { .. } => st.drills += 1,
                MarkingShape::Cross { .. } => st.crosses += 1,
                MarkingShape::Slot { .. } => {}
            }
            if let MarkingAnchor::DartTip { tip, toward, .. } = m.at {
                assert_ne!(tip, toward, "{label}");
                st.darts += 1;
            }
        }
        let (list, card, issues) = catch_unwind(AssertUnwindSafe(|| {
            (
                now.piece_markings(piece.id),
                now.piece_card(piece.id),
                now.piece_issues(piece.id),
            )
        }))
        .unwrap_or_else(|_| panic!("lecture panique : {label}"));
        assert_eq!(
            list.len(),
            piece.markings.len(),
            "marquage illisible : {label}"
        );
        let position = |id: PointId| {
            now.sheets
                .iter()
                .find_map(|s| s.position(id).ok())
                .unwrap_or_else(|| panic!("point sans position : {label}"))
        };
        for (res, m) in list.iter().zip(&piece.markings) {
            assert_eq!(res.marking, *m, "{label}");
            assert!(res.at.x.is_finite() && res.at.y.is_finite(), "{label}");
            let want = match m.at {
                MarkingAnchor::Point { point } => position(point),
                MarkingAnchor::DartTip {
                    tip,
                    toward,
                    setback_mm,
                } => {
                    let (p, q) = (position(tip), position(toward));
                    let (dx, dy) = (q.x - p.x, q.y - p.y);
                    let d = dx.hypot(dy);
                    assert!(d > 1e-7, "pince dégénérée acceptée : {label}");
                    PointMm::new(p.x + dx * setback_mm / d, p.y + dy * setback_mm / d)
                }
            };
            let tol = 1e-6 * (1.0 + want.x.abs().max(want.y.abs()));
            assert!(
                (res.at.x - want.x).abs() < tol && (res.at.y - want.y).abs() < tol,
                "position {:?} au lieu de {want:?} : {label}",
                res.at
            );
        }
        let card = card.unwrap();
        assert_eq!(card.markings, piece.markings.len(), "{label}");
        assert_eq!(card.issues, issues, "{label}");
        let outside = list
            .iter()
            .filter(|x| x.inside_outline == Some(false))
            .count();
        let flagged = issues.iter().find_map(|i| match i {
            PieceIssue::MarkingOutsideOutline { count } => Some(*count),
            _ => None,
        });
        assert_eq!(flagged, (outside > 0).then_some(outside), "{label}");
        st.outside += usize::from(outside > 0);
    }
}

/// Export : une forme exportée par marquage, un seul groupe nommé, jamais de jeton non fini.
fn check_marks_export(p: &Project, label: &str, st: &mut MarkStats) {
    use pf_export::{svg::export_svg, ExportError, Scene};
    for s in &p.sheets {
        let scene = match catch_unwind(AssertUnwindSafe(|| Scene::from_sheet(p, s.id)))
            .unwrap_or_else(|_| panic!("{label} : PANIC dans Scene::from_sheet"))
        {
            Ok(sc) => sc,
            Err(ExportError::EmptySheet(_)) => continue,
            Err(e) => panic!("{label} : scène refusée : {e}"),
        };
        assert_eq!(
            scene.markings.len(),
            p.sheet_markings(s.id).len(),
            "{label}"
        );
        if let Ok(svg) = export_svg(&scene) {
            assert_eq!(
                svg.matches("data-form=").count(),
                scene.markings.len(),
                "{label}"
            );
            assert_eq!(
                svg.matches("data-role=\"marquage\"").count(),
                usize::from(!scene.markings.is_empty()),
                "{label}"
            );
        }
    }
    check_export(p, label);
    st.exports += 1;
}

#[test]
fn us063_campagne_aleatoire_des_marquages_graine_fixe() {
    let mut st = MarkStats::default();
    for seq in 0..170u64 {
        let mut r = Rng(0x06_0300 + seq);
        let mut project = Project::new("Marquages aléa");
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
        random_drawing(&mut r, &mut project, sheet);
        for _ in 0..(3 + r.below(5)) {
            let _ = project.apply(Command::AddPoint {
                sheet,
                at: PointMm::new(r.unit() * 230.0 - 15.0, r.unit() * 180.0 - 15.0),
            });
        }
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
        // Quelques marquages valides au départ : de la matière pour les autres commandes.
        for _ in 0..r.below(5) {
            let pts: Vec<PointId> = project
                .sheet(sheet)
                .unwrap()
                .points
                .iter()
                .map(|p| p.id)
                .collect();
            let _ = project.apply(Command::AddPieceMarking {
                piece: base,
                at: mk_anchor(&mut r, &pts),
                shape: mk_shape(&mut r),
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
            if let Some(cmd) = mk_command(&mut r, &cur, sheet, op) {
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
                        let after = doc.project();
                        st.added += out.markings.len();
                        for id in &out.markings {
                            assert!(
                                after
                                    .pieces
                                    .iter()
                                    .any(|p| p.markings.iter().any(|m| m.id == *id)),
                                "marquage annoncé introuvable : {label}"
                            );
                        }
                        st.pruned += out
                            .notes
                            .iter()
                            .filter(|n| n.contains("marquage") && n.contains("retiré"))
                            .count();
                        st.own_pruned += out
                            .notes
                            .iter()
                            .filter(|n| n.contains("recréé ou confondu"))
                            .count();
                        let carried = |after: &Project| {
                            out.pieces
                                .iter()
                                .filter(|id| {
                                    after.piece(**id).is_ok_and(|p| !p.markings.is_empty())
                                })
                                .count()
                        };
                        match name.as_str() {
                            "SplitPiece" => st.splits_carried += carried(after),
                            "JoinPieces" => st.joins += 1,
                            "DuplicateSheet" => st.copies += carried(after),
                            _ => {}
                        }
                        st.derived_with_marks += out
                            .regenerated
                            .iter()
                            .filter(|id| after.piece(**id).is_ok_and(|p| !p.markings.is_empty()))
                            .count();
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
            check_marks_state(&doc.project().clone(), &label, &mut st);
            if step % 4 == 3 {
                check_marks_export(doc.project(), &label, &mut st);
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
        "marquages : {} modifications, {} refus, {} sans changement, {} marquages créés, \
         {} retraits par suppression ou régénération ({} par régénération), {} reprises par \
         séparation, {} assemblages, {} par copie de feuille, {} dérivées régénérées avec marquages, {} \
         pièces avec avertissement « hors contour », {} annulations, {} relectures, {} exports \
         contrôlés ; vus : {} pointages, {} perçages, {} croix, {} bouts de pince",
        st.accepted,
        st.refused,
        st.noops,
        st.added,
        st.pruned,
        st.own_pruned,
        st.splits_carried,
        st.joins,
        st.copies,
        st.derived_with_marks,
        st.outside,
        st.undone,
        st.reloaded,
        st.exports,
        st.punches,
        st.drills,
        st.crosses,
        st.darts
    );
    assert!(
        st.accepted >= 700,
        "seulement {} modifications",
        st.accepted
    );
    assert!(st.refused >= 400, "seulement {} refus", st.refused);
    assert!(st.added >= 300, "seulement {} marquages créés", st.added);
    assert!(
        st.darts >= 300 && st.drills >= 300 && st.crosses >= 300 && st.punches >= 300,
        "formes insuffisamment couvertes"
    );
    assert!(st.pruned >= 15, "seulement {} retraits", st.pruned);
    assert!(st.own_pruned >= 1, "aucun retrait par régénération");
    assert!(
        st.splits_carried >= 5,
        "séparations : {}",
        st.splits_carried
    );
    assert!(st.copies >= 10, "copies : {}", st.copies);
    assert!(st.outside >= 50, "seulement {} avertissements", st.outside);
    assert!(
        st.undone >= 100 && st.reloaded >= 80 && st.exports >= 1000,
        "couverture insuffisante"
    );
}
