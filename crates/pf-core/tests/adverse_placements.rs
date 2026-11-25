//! Campagne adverse : placements (graine fixe).

mod adverse_support;
use adverse_support::*;

use pf_core::command::{Command, PieceSource, PointRef};
use pf_core::model::*;
use pf_core::{persist, DerivedKind, DerivedOp, Document, Project};
use pf_geometry::PointMm;
use std::panic::{catch_unwind, AssertUnwindSafe};

// ---------------------------------------------------------------------------
// US-04.5 — plan de pièces : placements, rotations extrêmes, copies, suppressions
// ---------------------------------------------------------------------------

/// Projet sans le champ `placement` des pièces (tout le reste, brut).
fn strip_placements(p: &Project) -> String {
    let mut v = serde_json::to_value(p).expect("sérialisation brute");
    if let Some(pieces) = v["pieces"].as_array_mut() {
        for piece in pieces {
            piece.as_object_mut().unwrap().remove("placement");
        }
    }
    v.to_string()
}

#[derive(Default)]
struct PlanStats {
    placement_ok: usize,
    placement_refused: usize,
    reset_auto: usize,
    other_ok: usize,
    other_refused: usize,
    noops: usize,
    copies: usize,
    deleted_with_placement: usize,
    extreme_normalised: usize,
    undone: usize,
    reloaded: usize,
    layouts: usize,
    overlaps: usize,
}

/// Une commande de la campagne US-04.5 (ou `None` : annuler, rétablir, relire).
fn plan_command(r: &mut Rng, cur: &Project, op: usize) -> Option<Command> {
    let sheets: Vec<SheetId> = cur.sheets.iter().map(|s| s.id).collect();
    let pieces: Vec<PieceId> = cur.pieces.iter().map(|p| p.id).collect();
    let sheet = pick(r, &sheets, SheetId(BOGUS));
    let sh = cur.sheet(sheet).ok();
    let pts: Vec<PointId> = sh
        .map(|s| s.points.iter().map(|p| p.id).collect())
        .unwrap_or_default();
    Some(match op {
        0..=39 => Command::SetPiecePlacement {
            piece: pick(r, &pieces, PieceId(BOGUS)),
            placement: Some(pf_core::Placement {
                x_mm: wild_coord(r),
                y_mm: wild_coord(r),
                rotation_deg: wild_rotation(r),
            }),
        },
        40..=47 => Command::SetPiecePlacement {
            piece: pick(r, &pieces, PieceId(BOGUS)),
            placement: None,
        },
        48..=53 => Command::AddRectangle {
            sheet,
            corner: PointRef::New(PointMm::new(r.unit() * 300.0, r.unit() * 300.0)),
            width_mm: 20.0 + r.unit() * 150.0,
            height_mm: 20.0 + r.unit() * 150.0,
        },
        54..=60 => Command::ExtractPiece {
            sheet,
            source: PieceSource::Click {
                at: PointMm::new(r.unit() * 400.0, r.unit() * 400.0),
            },
            name: format!("Pièce {}", r.below(1000)),
            detached: r.chance(30),
        },
        61..=64 => Command::CreateDerivedPiece {
            mother: pick(r, &pieces, PieceId(BOGUS)),
            kind: DerivedKind::ALL[r.below(4)],
            operation: DerivedOp::Offset {
                distance_mm: (r.unit() * 15.0 + 2.0) * if r.chance(60) { -1.0 } else { 1.0 },
            },
            name: None,
        },
        65..=69 => {
            let s = pick(r, &sheets, SheetId(BOGUS));
            if r.chance(70) {
                Command::DuplicateSheet { sheet: s }
            } else {
                cur.import_command(s)
                    .unwrap_or(Command::DuplicateSheet { sheet: s })
            }
        }
        70..=74 => Command::DeletePiece {
            piece: pick(r, &pieces, PieceId(BOGUS)),
        },
        75..=77 => Command::DeleteSheet {
            sheet: pick(r, &sheets, SheetId(BOGUS)),
        },
        78..=82 => Command::MovePoint {
            sheet,
            point: pick(r, &pts, PointId(BOGUS)),
            to: PointMm::new(r.unit() * 400.0 - 50.0, r.unit() * 400.0 - 50.0),
        },
        83..=85 => Command::AddPiece {
            sheet,
            name: format!("Étiquette {}", r.below(100)),
            quantity: 1,
            material: None,
            mirrored_pair: false,
        },
        _ => return None,
    })
}

/// Contrôles de lecture du plan, à chaque pas : jamais de panique, cohérence avec les
/// fiches de pièces, aucune référence morte, lecture pure.
fn check_plan_reading(p: &Project, label: &str, st: &mut PlanStats) {
    let raw = snap(p);
    let Ok(plan) = catch_unwind(AssertUnwindSafe(|| p.plan_layout())) else {
        panic!("panique de plan_layout : {label}");
    };
    assert_eq!(plan.len(), p.pieces.len(), "{label}");
    for item in &plan {
        let piece = p
            .piece(item.piece)
            .unwrap_or_else(|e| panic!("pièce du plan absente : {label} : {e}"));
        assert_eq!(item.manual, piece.placement.is_some(), "{label}");
        match &item.skipped {
            Some(_) => {
                assert!(item.outline.is_empty() && item.bbox.is_none(), "{label}");
                assert!(
                    item.area_mm2.is_none() && item.perimeter_mm.is_none(),
                    "{label}"
                );
            }
            None => {
                let card = p.piece_card(item.piece).unwrap();
                assert_eq!(item.area_mm2, card.area_mm2, "aire du plan : {label}");
                assert_eq!(item.perimeter_mm, card.perimeter_mm, "périmètre : {label}");
                let bb = item.bbox.expect("boîte");
                assert!(item.outline.len() >= 2, "{label}");
                for q in &item.outline {
                    assert!(q.x.is_finite() && q.y.is_finite(), "{label}");
                    assert!(
                        q.x >= bb.min.x - 1e-9
                            && q.x <= bb.max.x + 1e-9
                            && q.y >= bb.min.y - 1e-9
                            && q.y <= bb.max.y + 1e-9,
                        "{label}"
                    );
                }
                if let Some(pl) = piece.placement {
                    assert_eq!(item.placement, Some(pl), "{label}");
                }
            }
        }
    }
    for (a, b) in p.plan_overlaps() {
        assert!(
            p.piece(a).is_ok() && p.piece(b).is_ok() && a != b,
            "{label}"
        );
        st.overlaps += 1;
    }
    // Sans placement manuel, jamais de chevauchement de boîtes.
    if p.pieces.iter().all(|x| x.placement.is_none()) {
        assert!(
            p.plan_overlaps().is_empty(),
            "chevauchement automatique : {label}"
        );
    }
    assert_eq!(
        snap(p),
        raw,
        "la lecture du plan a modifié le projet : {label}"
    );
    st.layouts += 1;
}

#[test]
fn us045_campagne_aleatoire_des_placements_graine_fixe() {
    let mut st = PlanStats::default();
    for seq in 0..170u64 {
        let mut r = Rng(0x04_0500 + seq);
        let mut project = Project::new("Plan aléa");
        let sheet = project.sheets[0].id;
        // Une à trois pièces de formes variées (rectangle, cercle, triangle).
        for k in 0..(1 + r.below(3)) {
            let at = PointMm::new(k as f64 * 250.0 + r.unit() * 40.0, r.unit() * 100.0);
            let entities = match r.below(3) {
                0 => {
                    project
                        .apply(Command::AddRectangle {
                            sheet,
                            corner: PointRef::New(at),
                            width_mm: 60.0 + r.unit() * 150.0,
                            height_mm: 40.0 + r.unit() * 120.0,
                        })
                        .unwrap()
                        .entities
                }
                1 => {
                    project
                        .apply(Command::AddCircle {
                            sheet,
                            center: PointRef::New(at),
                            radius_mm: 20.0 + r.unit() * 60.0,
                        })
                        .unwrap()
                        .entities
                }
                _ => {
                    let c =
                        PointMm::new(at.x + 50.0 + r.unit() * 50.0, at.y + 60.0 + r.unit() * 60.0);
                    let b = PointMm::new(at.x + 80.0 + r.unit() * 80.0, at.y);
                    let mut ids = Vec::new();
                    for (s, e) in [(at, b), (b, c), (c, at)] {
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
            };
            project
                .apply(Command::ExtractPiece {
                    sheet,
                    source: PieceSource::Paths { entities },
                    name: format!("Forme {k}"),
                    detached: false,
                })
                .unwrap();
        }
        let mut doc = Document::new(project);
        let mut undo_stack: Vec<String> = Vec::new();
        let mut redo_stack: Vec<String> = Vec::new();
        for step in 0..24 {
            let label = format!("graine {seq} pas {step}");
            let before_json = snap(doc.project());
            let cur = doc.project().clone();
            let op = r.below(100);
            if let Some(cmd) = plan_command(&mut r, &cur, op) {
                let name = variant_name(&cmd);
                let is_placement = name == "SetPiecePlacement";
                let (target, placement_sent) = match &cmd {
                    Command::SetPiecePlacement { piece, placement } => (Some(*piece), *placement),
                    _ => (None, None),
                };
                let Ok(result) = catch_unwind(AssertUnwindSafe(|| doc.execute(cmd))) else {
                    panic!("panique ({name}) : {label}");
                };
                match result {
                    Err(e) => {
                        assert!(!e.to_string().is_empty(), "{label}");
                        assert_eq!(
                            snap(doc.project()),
                            before_json,
                            "refus avec trace ({name}) : {label}"
                        );
                        if is_placement {
                            st.placement_refused += 1;
                        } else {
                            st.other_refused += 1;
                        }
                    }
                    Ok(out) => {
                        let after = doc.project();
                        after
                            .validate()
                            .unwrap_or_else(|e| panic!("{name} : {label} : {e}"));
                        let changed = snap(after) != before_json;
                        if !changed {
                            st.noops += 1;
                        } else {
                            undo_stack.push(before_json.clone());
                            redo_stack.clear();
                        }
                        if is_placement {
                            st.placement_ok += 1;
                            st.reset_auto += usize::from(placement_sent.is_none());
                            // La construction, les fiches, les mesures : octet pour octet.
                            assert_eq!(
                                strip_placements(after),
                                strip_placements(&cur),
                                "un placement a touché la construction : {label}"
                            );
                            let stored = target
                                .and_then(|id| after.piece(id).ok())
                                .and_then(|p| p.placement);
                            assert_eq!(stored.is_some(), placement_sent.is_some(), "{label}");
                            if let (Some(sent), Some(stored)) = (placement_sent, stored) {
                                assert!((0.0..360.0).contains(&stored.rotation_deg), "{label}");
                                assert!(stored.x_mm.is_finite() && stored.y_mm.is_finite());
                                assert!(stored.rotation_deg.is_sign_positive(), "{label}");
                                if sent.rotation_deg != stored.rotation_deg {
                                    st.extreme_normalised += 1;
                                }
                            }
                            // Les autres pièces gardent leur placement.
                            for old in cur.pieces.iter().filter(|p| Some(p.id) != target) {
                                assert_eq!(
                                    after.piece(old.id).unwrap().placement,
                                    old.placement,
                                    "{label}"
                                );
                            }
                        } else {
                            st.other_ok += 1;
                            // Aucune autre commande ne touche aux placements existants.
                            for old in &cur.pieces {
                                if let Ok(now) = after.piece(old.id) {
                                    assert_eq!(
                                        now.placement, old.placement,
                                        "{name} a modifié un placement : {label}"
                                    );
                                } else if old.placement.is_some() {
                                    st.deleted_with_placement += 1;
                                }
                            }
                            // Les pièces nouvelles (extraites, dérivées, copies) sont
                            // en placement automatique.
                            for new in out.pieces.iter().filter_map(|id| after.piece(*id).ok()) {
                                assert!(new.placement.is_none(), "{name} : {label}");
                            }
                            st.copies +=
                                usize::from(name == "DuplicateSheet" || name == "ImportSheet");
                        }
                    }
                }
            } else if op < 95 {
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
            check_plan_reading(&doc.project().clone(), &label, &mut st);
        }
        // Fin de séquence : relecture identique à l'octet.
        let json = persist::to_json(doc.project()).unwrap();
        let back = persist::from_json(&json).unwrap();
        assert_eq!(&back, doc.project(), "graine {seq}");
        assert_eq!(persist::to_json(&back).unwrap(), json, "graine {seq}");
    }
    eprintln!(
        "plan : {} placements acceptés ({} retours à l'automatique, {} rotations normalisées), \
         {} refusés, {} autres commandes acceptées, {} refusées, {} sans changement, {} copies, \
         {} placements perdus avec leur pièce, {} annulations, {} relectures, {} lectures du \
         plan, {} chevauchements vus",
        st.placement_ok,
        st.reset_auto,
        st.extreme_normalised,
        st.placement_refused,
        st.other_ok,
        st.other_refused,
        st.noops,
        st.copies,
        st.deleted_with_placement,
        st.undone,
        st.reloaded,
        st.layouts,
        st.overlaps
    );
    assert!(
        st.placement_ok >= 500,
        "seulement {} placements",
        st.placement_ok
    );
    assert!(
        st.placement_refused >= 300,
        "seulement {} refus",
        st.placement_refused
    );
    assert!(
        st.reset_auto >= 50,
        "seulement {} retours auto",
        st.reset_auto
    );
    assert!(
        st.extreme_normalised >= 100,
        "seulement {} rotations normalisées",
        st.extreme_normalised
    );
    assert!(
        st.other_ok >= 400,
        "seulement {} autres commandes",
        st.other_ok
    );
    assert!(st.copies >= 30, "seulement {} copies", st.copies);
    assert!(
        st.deleted_with_placement >= 10,
        "seulement {} suppressions de pièces placées",
        st.deleted_with_placement
    );
    assert!(
        st.undone >= 100 && st.reloaded >= 60,
        "annuler / relire : {} / {}",
        st.undone,
        st.reloaded
    );
    assert!(
        st.overlaps >= 10,
        "seulement {} chevauchements",
        st.overlaps
    );
}
