//! Campagne adverse : crans ancres (graine fixe).

mod adverse_support;
use adverse_support::*;

use pf_core::command::{Command, PieceSource, PointRef};
use pf_core::model::*;
use pf_core::{persist, DerivedKind, DerivedOp, Document, Project};
use pf_geometry::{PointMm, Side, Transform};
use std::panic::{catch_unwind, AssertUnwindSafe};

// ---------------------------------------------------------------------------
// US-06.2 — crans ancrés à un point, au milieu, à une proportion, ou en longueur reportée
// ---------------------------------------------------------------------------

/// Proportion tirée dans [0 ; 1], sur les bornes, juste dehors, ou franchement hostile.
fn na_ratio(r: &mut Rng) -> f64 {
    match r.below(10) {
        0..=5 => r.unit(),
        6 => [0.0, 1.0, 0.5][r.below(3)],
        7 => [-0.001, 1.000_000_1, 2.0, -1.0][r.below(4)],
        8 => [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 1e300][r.below(4)],
        _ => {
            let e = extreme_numbers();
            e[r.below(e.len())]
        }
    }
}

fn na_anchor(r: &mut Rng, pts: &[PointId], ents: &[EntityId]) -> NotchAnchor {
    match r.below(4) {
        0 => NotchAnchor::Point {
            point: pick(r, pts, PointId(BOGUS)),
        },
        1 => NotchAnchor::Midpoint {},
        2 => NotchAnchor::Ratio { ratio: na_ratio(r) },
        _ => NotchAnchor::Carried {
            entity: pick(r, ents, EntityId(BOGUS)),
            point: pick(r, pts, PointId(BOGUS)),
            from_end: r.chance(40),
        },
    }
}

fn na_length(r: &mut Rng) -> f64 {
    if r.chance(88) {
        2.0 + r.below(8) as f64
    } else {
        [0.0, -1.0, 100.0, 100.01, f64::NAN, f64::INFINITY][r.below(6)]
    }
}

/// Une commande de la campagne US-06.2 (ou `None` : annuler, rétablir, relire).
fn na_command(r: &mut Rng, cur: &Project, main: SheetId, op: usize) -> Option<Command> {
    let sheets: Vec<SheetId> = cur.sheets.iter().map(|s| s.id).collect();
    let sheet = if r.chance(78) {
        main
    } else {
        pick(r, &sheets, SheetId(BOGUS))
    };
    let (pts, ents, notches): (Vec<PointId>, Vec<EntityId>, Vec<Notch>) = cur
        .sheet(sheet)
        .map(|s| {
            (
                s.points.iter().map(|p| p.id).collect(),
                s.entities.iter().map(|e| e.id).collect(),
                s.notches.clone(),
            )
        })
        .unwrap_or_default();
    let anchored_pts: Vec<PointId> = notches
        .iter()
        .filter_map(|n| n.anchor.and_then(|a| a.point()))
        .collect();
    let known = |r: &mut Rng| {
        if !notches.is_empty() && r.chance(90) {
            notches[r.below(notches.len())].clone()
        } else {
            Notch {
                id: NotchId(BOGUS),
                entity: EntityId(BOGUS),
                distance_mm: 0.0,
                side: Side::Left,
                length_mm: 5.0,
                style: NotchStyle::Single,
                anchor: None,
                form: NotchForm::Line,
                width_mm: None,
                on_cut: false,
                corner: false,
                proportional: false,
            }
        }
    };
    let side = |r: &mut Rng| {
        if r.chance(50) {
            Side::Left
        } else {
            Side::Right
        }
    };
    let style = |r: &mut Rng| {
        if r.chance(25) {
            NotchStyle::Double
        } else {
            NotchStyle::Single
        }
    };
    let add = |r: &mut Rng| Command::AddAnchoredNotch {
        sheet,
        entity: pick(r, &ents, EntityId(BOGUS)),
        anchor: na_anchor(r, &pts, &ents),
        side: side(r),
        length_mm: na_length(r),
        style: style(r),
    };
    let on_path = |r: &mut Rng, e: EntityId| -> PointMm {
        let s = cur.sheet(sheet).ok();
        s.and_then(|s| {
            let len = s.path_length_mm(e).ok()?;
            s.path_point_at(e, len * r.unit()).ok().map(|x| x.0)
        })
        .unwrap_or(PointMm::new(r.unit() * 200.0, r.unit() * 200.0))
    };
    Some(match op {
        0..=26 => add(r),
        27..=34 => {
            let n = known(r);
            let anchor = match r.below(8) {
                0 | 1 => None,
                // Les mêmes valeurs : sans effet, aucune étape.
                2 | 3 if n.anchor.is_some() => n.anchor,
                _ => Some(na_anchor(r, &pts, &ents)),
            };
            Command::SetNotchAnchor {
                sheet,
                notch: n.id,
                anchor,
            }
        }
        35..=39 => {
            let n = known(r);
            let at = cur
                .sheet(sheet)
                .ok()
                .and_then(|s| s.notch_distance_mm(&n).ok());
            Command::SetNotch {
                sheet,
                notch: n.id,
                distance_mm: match at {
                    Some(d) if r.chance(60) => d,
                    _ => num(r),
                },
                side: side(r),
                length_mm: na_length(r),
                style: style(r),
            }
        }
        40..=51 => {
            let point = if !anchored_pts.is_empty() && r.chance(65) {
                anchored_pts[r.below(anchored_pts.len())]
            } else {
                pick(r, &pts, PointId(BOGUS))
            };
            Command::MovePoint {
                sheet,
                point,
                to: PointMm::new(r.unit() * 260.0 - 30.0, r.unit() * 210.0 - 30.0),
            }
        }
        52..=59 => {
            // Surtout les points et tracés cités par un ancrage : ils doivent emporter le cran.
            let cited: Vec<EntityId> = notches
                .iter()
                .filter_map(|n| match n.anchor {
                    Some(NotchAnchor::Carried { entity, .. }) => Some(entity),
                    _ => None,
                })
                .collect();
            let points = if !anchored_pts.is_empty() && r.chance(70) {
                some(r, &anchored_pts, 2)
            } else {
                some(r, &pts, 2)
            };
            let entities = if !cited.is_empty() && r.chance(35) {
                some(r, &cited, 1)
            } else if r.chance(15) {
                some(r, &ents, 1)
            } else {
                vec![]
            };
            Command::Delete {
                sheet,
                points,
                entities,
            }
        }
        60..=63 => Command::ReversePath {
            sheet,
            entity: pick(r, &ents, EntityId(BOGUS)),
        },
        64..=67 => {
            let entity = pick(r, &ents, EntityId(BOGUS));
            let at = on_path(r, entity);
            Command::SplitPath { sheet, entity, at }
        }
        68..=70 => Command::MergePaths {
            sheet,
            first: pick(r, &ents, EntityId(BOGUS)),
            second: pick(r, &ents, EntityId(BOGUS)),
        },
        71..=73 => Command::ExtendPath {
            sheet,
            entity: pick(r, &ents, EntityId(BOGUS)),
            end: if r.chance(50) {
                SegmentEnd::Start
            } else {
                SegmentEnd::End
            },
            delta_mm: (r.unit() - 0.5) * 80.0,
        },
        74..=76 => Command::DuplicateSheet { sheet },
        77..=79 => Command::Duplicate {
            sheet,
            entities: some(r, &ents, 2),
            transform: Transform::Translate {
                dx_mm: (r.unit() - 0.5) * 200.0,
                dy_mm: (r.unit() - 0.5) * 200.0,
            },
        },
        80..=82 => Command::LinkMirror {
            sheet,
            points: some(r, &pts, 2),
            entities: some(r, &ents, 2),
            axis: MirrorAxis::Segment {
                segment: pick(r, &ents, EntityId(BOGUS)),
            },
        },
        83..=86 => {
            let fragment = cur
                .copy_fragment(sheet, &some(r, &pts, 3), &some(r, &ents, 3), &[], &[])
                .ok()?;
            Command::PasteFragment {
                sheet,
                fragment: Box::new(fragment),
                layer: None,
            }
        }
        87..=89 => Command::Batch {
            commands: (0..(2 + r.below(3)))
                .map(|_| {
                    if r.chance(25) && !notches.is_empty() {
                        Command::DeleteNotch {
                            sheet,
                            notch: known(r).id,
                        }
                    } else {
                        add(r)
                    }
                })
                .collect(),
        },
        90..=91 => Command::AddPieceInternal {
            piece: pick(
                r,
                &cur.pieces.iter().map(|p| p.id).collect::<Vec<_>>(),
                PieceId(BOGUS),
            ),
            target: InternalTarget::Notch { notch: known(r).id },
            kind: InternalKind::Mark,
        },
        92 => Command::CreateDerivedPiece {
            mother: pick(
                r,
                &cur.pieces.iter().map(|p| p.id).collect::<Vec<_>>(),
                PieceId(BOGUS),
            ),
            kind: DerivedKind::Lining,
            operation: DerivedOp::Offset {
                distance_mm: -2.0 - r.unit() * 10.0,
            },
            name: None,
        },
        93 => Command::SplitPiece {
            piece: pick(
                r,
                &cur.pieces.iter().map(|p| p.id).collect::<Vec<_>>(),
                PieceId(BOGUS),
            ),
            a: PointMm::new(20.0 + r.unit() * 150.0, -10.0),
            b: PointMm::new(20.0 + r.unit() * 150.0, 170.0),
        },
        _ => return None,
    })
}

#[derive(Default)]
struct NaStats {
    accepted: usize,
    refused: usize,
    noops: usize,
    created: usize,
    pruned: usize,
    released: usize,
    fixed_on_copy: usize,
    point_k: usize,
    mid_k: usize,
    ratio_k: usize,
    carried_k: usize,
    carried_end_k: usize,
    moved: usize,
    undone: usize,
    reloaded: usize,
    exports: usize,
}

/// Invariants vérifiés après chaque pas : jamais de référence morte, la distance stockée d'un
/// cran ancré est 0, la position calculée est dans le bord et égale à un recalcul indépendant,
/// les traits se calculent, les éléments internes citent des crans vivants.
fn na_check_state(now: &Project, label: &str, st: &mut NaStats) {
    for s in &now.sheets {
        for n in &s.notches {
            let len = s
                .path_length_mm(n.entity)
                .unwrap_or_else(|e| panic!("{label} : {e}"));
            let Some(a) = n.anchor else {
                assert!(
                    n.distance_mm >= 0.0 && n.distance_mm <= len + 1e-9,
                    "cran fixe hors du bord : {label}"
                );
                continue;
            };
            assert_eq!(n.distance_mm, 0.0, "distance stockée non nulle : {label}");
            if let Some(p) = a.point() {
                assert!(s.point(p).is_ok(), "point mort {a:?} : {label}");
            }
            if let NotchAnchor::Carried { entity, .. } = a {
                assert!(
                    s.entity(entity).is_ok(),
                    "tracé source mort {a:?} : {label}"
                );
            }
            let d = catch_unwind(AssertUnwindSafe(|| s.notch_distance_mm(n)))
                .unwrap_or_else(|_| panic!("lecture panique {a:?} : {label}"))
                .unwrap_or_else(|e| panic!("position illisible {a:?} : {e} : {label}"));
            assert!(
                d.is_finite() && d >= 0.0 && d <= len + 1e-9,
                "position {d} hors du bord ({len}) : {label}"
            );
            let tol = 1e-9 * (1.0 + len);
            match a {
                NotchAnchor::Midpoint {} => {
                    assert!((d - len / 2.0).abs() <= tol, "milieu : {label}");
                    st.mid_k += 1;
                }
                NotchAnchor::Ratio { ratio } => {
                    assert!((d - ratio * len).abs() <= tol, "proportion : {label}");
                    st.ratio_k += 1;
                }
                NotchAnchor::Point { point } => {
                    // Le pied du cran est à la distance minimale du point au tracé (à 0,05 mm
                    // près : limite connue de `CubicBezier::nearest_t`).
                    let at = s.position(point).unwrap();
                    let (base, _) = s.path_point_at(n.entity, d).unwrap();
                    let min = s.shape(n.entity).unwrap().distance_to_point(at);
                    let got = pf_geometry::distance_mm(base, at);
                    assert!(
                        got <= min + 0.05 + 1e-6 * (1.0 + min),
                        "pied à {got} mm du point, distance minimale {min} : {label}"
                    );
                    st.point_k += 1;
                }
                NotchAnchor::Carried {
                    entity,
                    point,
                    from_end,
                } => {
                    let at = s.position(point).unwrap();
                    let c = s.path_distance_of(entity, at).unwrap();
                    let want = if from_end { len - c } else { c };
                    assert!((d - want).abs() <= 1e-6 * (1.0 + len), "report : {label}");
                    st.carried_k += 1;
                    st.carried_end_k += usize::from(from_end);
                }
            }
            let marks = catch_unwind(AssertUnwindSafe(|| s.notch_marks(n)))
                .unwrap_or_else(|_| panic!("traits : panique : {label}"))
                .unwrap_or_else(|e| panic!("traits illisibles : {e} : {label}"));
            assert!(
                marks.iter().all(|(p, q)| p.x.is_finite()
                    && p.y.is_finite()
                    && q.x.is_finite()
                    && q.y.is_finite()),
                "trait non fini : {label}"
            );
        }
    }
    assert_no_dead_internal(now, label);
}

#[test]
fn us062_campagne_aleatoire_des_crans_ancres_graine_fixe() {
    let mut st = NaStats::default();
    for seq in 0..160u64 {
        let mut r = Rng(0x06_0200 + seq);
        let mut project = Project::new("Crans ancrés aléa");
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
        for _ in 0..(3 + r.below(5)) {
            let _ = project.apply(Command::AddPoint {
                sheet,
                at: PointMm::new(r.unit() * 230.0 - 15.0, r.unit() * 180.0 - 15.0),
            });
        }
        // Quelques crans valides au départ : de la matière pour les autres commandes.
        for _ in 0..r.below(6) {
            let (pts, ents): (Vec<PointId>, Vec<EntityId>) = {
                let s = project.sheet(sheet).unwrap();
                (
                    s.points.iter().map(|p| p.id).collect(),
                    s.entities.iter().map(|e| e.id).collect(),
                )
            };
            let _ = project.apply(Command::AddAnchoredNotch {
                sheet,
                entity: pick(&mut r, &ents, EntityId(BOGUS)),
                anchor: na_anchor(&mut r, &pts, &ents),
                side: Side::Left,
                length_mm: 5.0,
                style: NotchStyle::Single,
            });
        }
        let initial_json = snap(&project);
        let mut doc = Document::new(project);
        let mut undo_stack: Vec<String> = Vec::new();
        let mut redo_stack: Vec<String> = Vec::new();
        for step in 0..22 {
            let label = format!("graine {seq} pas {step}");
            let before_json = snap(doc.project());
            let cur = doc.project().clone();
            let op = r.below(100);
            if let Some(cmd) = na_command(&mut r, &cur, sheet, op) {
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
                        st.created += out.notches.len();
                        for id in &out.notches {
                            assert!(
                                doc.project()
                                    .sheets
                                    .iter()
                                    .any(|s| s.notches.iter().any(|n| n.id == *id)),
                                "cran annoncé introuvable : {label}"
                            );
                        }
                        st.pruned += out
                            .notes
                            .iter()
                            .filter(|n| n.contains("ancré") && n.contains("retiré"))
                            .count();
                        st.released += out
                            .notes
                            .iter()
                            .filter(|n| n.contains("devient un cran à distance fixe"))
                            .count();
                        st.fixed_on_copy += out
                            .notes
                            .iter()
                            .filter(|n| n.contains("à distance fixe") && !n.contains("devient"))
                            .count();
                        st.moved += usize::from(name == "MovePoint");
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
            na_check_state(&doc.project().clone(), &label, &mut st);
            if step % 5 == 4 {
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
        "crans ancrés : {} modifications, {} refus, {} sans changement, {} crans créés, {} retraits \
         (ancrage mort), {} détachements (tracé modifié), {} recopies à distance fixe, {} \
         déplacements, {} annulations, {} relectures, {} exports ; vus : {} points, {} milieux, \
         {} proportions, {} reports ({} depuis l'arrivée)",
        st.accepted,
        st.refused,
        st.noops,
        st.created,
        st.pruned,
        st.released,
        st.fixed_on_copy,
        st.moved,
        st.undone,
        st.reloaded,
        st.exports,
        st.point_k,
        st.mid_k,
        st.ratio_k,
        st.carried_k,
        st.carried_end_k
    );
    assert!(
        st.accepted >= 800,
        "seulement {} modifications",
        st.accepted
    );
    assert!(st.refused >= 300, "seulement {} refus", st.refused);
    assert!(st.created >= 300, "seulement {} crans créés", st.created);
    assert!(
        st.point_k >= 300 && st.mid_k >= 300 && st.ratio_k >= 300 && st.carried_k >= 200,
        "ancrages insuffisamment couverts"
    );
    assert!(
        st.carried_end_k >= 50,
        "{} reports depuis l'arrivée",
        st.carried_end_k
    );
    assert!(st.pruned >= 15, "seulement {} retraits", st.pruned);
    assert!(st.released >= 15, "seulement {} détachements", st.released);
    assert!(
        st.undone >= 100 && st.reloaded >= 40 && st.exports >= 400,
        "couverture insuffisante"
    );
}
