//! Campagne adverse : elements internes (graine fixe).

mod adverse_support;
use adverse_support::*;

use pf_core::command::{Command, PieceSource, PointRef};
use pf_core::model::*;
use pf_core::{persist, Document, Project};
use pf_geometry::{PointMm, Side};
use std::panic::{catch_unwind, AssertUnwindSafe};

fn internal_kind_for(r: &mut Rng, target: InternalTarget, coherent: bool) -> InternalKind {
    if coherent {
        let pool: &[InternalKind] = match target {
            InternalTarget::Path { .. } => &InternalKind::LINES,
            _ => &InternalKind::MARKS,
        };
        pool[r.below(pool.len())]
    } else {
        [
            InternalKind::Fold,
            InternalKind::Topstitch,
            InternalKind::PocketPosition,
            InternalKind::Mark,
            InternalKind::Buttonhole,
            InternalKind::Other,
        ][r.below(6)]
    }
}

/// Une commande de la campagne US-04.3 (ou `None` : annuler, rétablir, sauvegarder).
fn internals_command(r: &mut Rng, cur: &Project, sheet: SheetId, op: usize) -> Option<Command> {
    let pieces: Vec<PieceId> = cur.pieces.iter().map(|p| p.id).collect();
    let piece = pick(r, &pieces, PieceId(BOGUS));
    let sh = cur.sheet(sheet).ok();
    let ents: Vec<EntityId> = sh
        .map(|s| s.entities.iter().map(|e| e.id).collect())
        .unwrap_or_default();
    let pts: Vec<PointId> = sh
        .map(|s| s.points.iter().map(|p| p.id).collect())
        .unwrap_or_default();
    let nts: Vec<NotchId> = sh
        .map(|s| s.notches.iter().map(|n| n.id).collect())
        .unwrap_or_default();
    let internals: Vec<InternalElement> = cur
        .piece(piece)
        .map(|p| p.internal.clone())
        .unwrap_or_default();
    let any_target = |r: &mut Rng| match r.below(3) {
        0 => InternalTarget::Path {
            entity: pick(r, &ents, EntityId(BOGUS)),
        },
        1 => InternalTarget::Point {
            point: pick(r, &pts, PointId(BOGUS)),
        },
        _ => InternalTarget::Notch {
            notch: pick(r, &nts, NotchId(BOGUS)),
        },
    };
    let known_or_any = |r: &mut Rng| {
        if !internals.is_empty() && r.chance(85) {
            internals[r.below(internals.len())].target
        } else {
            any_target(r)
        }
    };
    Some(match op {
        0..=15 => Command::ImportPieceInternals { piece },
        16..=33 => {
            let target = any_target(r);
            let coherent = r.chance(75);
            let kind = internal_kind_for(r, target, coherent);
            Command::AddPieceInternal {
                piece,
                target,
                kind,
            }
        }
        34..=41 => Command::RemovePieceInternal {
            piece,
            target: known_or_any(r),
        },
        42..=49 => {
            let target = known_or_any(r);
            let coherent = r.chance(85);
            let kind = internal_kind_for(r, target, coherent);
            Command::SetPieceInternalKind {
                piece,
                target,
                kind,
            }
        }
        50..=57 => match known_or_any(r) {
            InternalTarget::Path { entity } if r.chance(70) => Command::Delete {
                sheet,
                points: vec![],
                entities: vec![entity],
            },
            InternalTarget::Point { point } => Command::Delete {
                sheet,
                points: vec![point],
                entities: vec![],
            },
            InternalTarget::Notch { notch } => Command::DeleteNotch { sheet, notch },
            InternalTarget::Path { .. } => Command::Delete {
                sheet,
                points: vec![],
                entities: vec![pick(r, &ents, EntityId(BOGUS))],
            },
        },
        58..=67 => Command::MovePoint {
            sheet,
            point: pick(r, &pts, PointId(BOGUS)),
            to: PointMm::new(r.unit() * 260.0 - 30.0, r.unit() * 210.0 - 30.0),
        },
        68..=73 => match r.below(3) {
            0 => Command::AddPoint {
                sheet,
                at: PointMm::new(r.unit() * 220.0, r.unit() * 160.0),
            },
            1 => Command::AddSegment {
                sheet,
                from: PointRef::New(PointMm::new(r.unit() * 200.0, r.unit() * 150.0)),
                to: PointRef::New(PointMm::new(r.unit() * 200.0, r.unit() * 150.0)),
            },
            _ => {
                let e = pick(r, &ents, EntityId(BOGUS));
                let len = sh.and_then(|s| s.path_length_mm(e).ok()).unwrap_or(10.0);
                Command::AddNotch {
                    sheet,
                    entity: e,
                    distance_mm: r.unit() * len,
                    side: Side::Left,
                    length_mm: 4.0,
                    style: NotchStyle::Single,
                }
            }
        },
        74..=77 => {
            if r.chance(60) {
                let e = pick(r, &ents, EntityId(BOGUS));
                let len = sh.and_then(|s| s.path_length_mm(e).ok()).unwrap_or(1.0);
                let at = sh
                    .and_then(|s| s.path_point_at(e, r.unit() * len).ok())
                    .map_or(PointMm::new(1.0, 1.0), |x| x.0);
                Command::SplitPath {
                    sheet,
                    entity: e,
                    at,
                }
            } else {
                Command::MergePaths {
                    sheet,
                    first: pick(r, &ents, EntityId(BOGUS)),
                    second: pick(r, &ents, EntityId(BOGUS)),
                }
            }
        }
        78..=80 => Command::DuplicateSheet { sheet },
        81..=83 => Command::SetPathRole {
            sheet,
            entity: pick(r, &ents, EntityId(BOGUS)),
            role: PathRole::ALL[r.below(PathRole::ALL.len())],
        },
        84..=85 => {
            // Le contour ou le droit-fil change sous les éléments internes.
            let outline = cur
                .piece(piece)
                .map(|p| p.outline.clone())
                .unwrap_or_default();
            Command::SetPieceOutline {
                piece,
                outline,
                grainline: if r.chance(60) {
                    Some(pick(r, &ents, EntityId(BOGUS)))
                } else {
                    None
                },
            }
        }
        _ => return None,
    })
}

#[derive(Default)]
struct InternalsStats {
    accepted: usize,
    refused: usize,
    noops: usize,
    imports: usize,
    imported_elems: usize,
    pruned: usize,
    outside: usize,
    undone: usize,
    reloaded: usize,
    copies: usize,
    splits: usize,
}

/// Invariants vérifiés après chaque pas : aucune référence morte, lectures pures
/// sans panique, cohérence fiche / liste / avertissement, import à blanc sans doublon.
fn check_internals_state(now: &Project, label: &str, stats: &mut InternalsStats) {
    use pf_core::piece::PieceIssue;
    assert_no_dead_internal(now, label);
    for p in &now.pieces {
        let (list, card, issues, rep) = catch_unwind(AssertUnwindSafe(|| {
            (
                now.piece_internals(p.id),
                now.piece_card(p.id),
                now.piece_issues(p.id),
                now.piece_internal_import(p.id),
            )
        }))
        .unwrap_or_else(|_| panic!("lecture panique : {label}"));
        assert_eq!(list.len(), p.internal.len(), "{label}");
        let card = card.unwrap();
        assert_eq!(card.internal.total(), p.internal.len(), "{label}");
        assert_eq!(card.issues, issues, "{label}");
        let sorted = list
            .iter()
            .filter(|x| x.inside_outline == Some(false))
            .count();
        let flagged = issues.iter().find_map(|i| match i {
            PieceIssue::InternalOutsideOutline { count } => Some(*count),
            _ => None,
        });
        assert_eq!(flagged, (sorted > 0).then_some(sorted), "{label}");
        stats.outside += usize::from(sorted > 0);
        if let Ok(rep) = rep {
            for el in &rep.added {
                assert!(
                    !p.internal.iter().any(|x| x.target == el.target),
                    "doublon proposé : {label}"
                );
            }
        }
    }
}

#[test]
fn us043_campagne_aleatoire_des_elements_internes_graine_fixe() {
    let mut st = InternalsStats::default();
    for seq in 0..180u64 {
        let mut r = Rng(0x04_0300 + seq);
        let mut project = Project::new("Internes aléa");
        let sheet = project.sheets[0].id;
        project
            .apply(Command::AddRectangle {
                sheet,
                corner: PointRef::New(PointMm::new(0.0, 0.0)),
                width_mm: 200.0,
                height_mm: 150.0,
            })
            .unwrap();
        project
            .apply(Command::ExtractPiece {
                sheet,
                source: PieceSource::Click {
                    at: PointMm::new(100.0, 75.0),
                },
                name: "Base".into(),
                detached: false,
            })
            .unwrap();
        random_drawing(&mut r, &mut project, sheet);
        for _ in 0..(2 + r.below(4)) {
            let _ = project.apply(Command::AddPoint {
                sheet,
                at: PointMm::new(r.unit() * 230.0 - 15.0, r.unit() * 180.0 - 15.0),
            });
        }
        let ents: Vec<EntityId> = project
            .sheet(sheet)
            .unwrap()
            .entities
            .iter()
            .map(|e| e.id)
            .collect();
        for _ in 0..(1 + r.below(4)) {
            let e = ents[r.below(ents.len())];
            let len = project
                .sheet(sheet)
                .unwrap()
                .path_length_mm(e)
                .unwrap_or(10.0);
            let _ = project.apply(Command::AddNotch {
                sheet,
                entity: e,
                distance_mm: r.unit() * len,
                side: if r.chance(50) {
                    Side::Left
                } else {
                    Side::Right
                },
                length_mm: 5.0,
                style: NotchStyle::Single,
            });
        }
        if r.chance(40) {
            let _ = project.apply(Command::ExtractPiece {
                sheet,
                source: PieceSource::Click {
                    at: PointMm::new(r.unit() * 200.0, r.unit() * 150.0),
                },
                name: "Seconde".into(),
                detached: r.chance(40),
            });
        }
        let mut doc = Document::new(project);
        let mut undo_stack: Vec<String> = Vec::new();
        let mut redo_stack: Vec<String> = Vec::new();
        for step in 0..22 {
            let label = format!("graine {seq} pas {step}");
            let before_json = snap(doc.project());
            let cur = doc.project().clone();
            let op = r.below(100);
            if let Some(cmd) = internals_command(&mut r, &cur, sheet, op) {
                let name = variant_name(&cmd);
                let result = catch_unwind(AssertUnwindSafe(|| doc.execute(cmd)));
                let Ok(result) = result else {
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
                        if name == "ImportPieceInternals" {
                            st.imports += 1;
                            let a: usize =
                                doc.project().pieces.iter().map(|p| p.internal.len()).sum();
                            let b: usize = cur.pieces.iter().map(|p| p.internal.len()).sum();
                            st.imported_elems += a.saturating_sub(b);
                        }
                        st.pruned += out
                            .notes
                            .iter()
                            .filter(|n| n.contains("référençait un objet supprimé"))
                            .count();
                        if name == "SplitPath" {
                            st.splits += out
                                .notes
                                .iter()
                                .filter(|n| n.contains("ligne interne"))
                                .count();
                        }
                        st.copies += usize::from(name == "DuplicateSheet");
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
            check_internals_state(&doc.project().clone(), &label, &mut st);
        }
    }
    eprintln!(
        "éléments internes : {} modifications, {} refus, {} sans changement, {} imports \
         ({} éléments), {} retraits par suppression, {} coupes de ligne interne, {} copies de \
         feuille, {} avertissements « hors contour », {} annulations, {} relectures",
        st.accepted,
        st.refused,
        st.noops,
        st.imports,
        st.imported_elems,
        st.pruned,
        st.splits,
        st.copies,
        st.outside,
        st.undone,
        st.reloaded
    );
    assert!(
        st.accepted >= 500,
        "seulement {} modifications",
        st.accepted
    );
    assert!(st.refused >= 300, "seulement {} refus", st.refused);
    assert!(
        st.imports >= 150 && st.imported_elems >= 100,
        "import : {} / {}",
        st.imports,
        st.imported_elems
    );
    assert!(st.pruned >= 20, "seulement {} retraits", st.pruned);
    assert!(
        st.undone >= 100 && st.reloaded >= 80 && st.copies >= 10,
        "couverture insuffisante"
    );
    assert!(st.outside >= 20, "seulement {} avertissements", st.outside);
}
