//! Campagne adverse : separer assembler (graine fixe).

mod adverse_support;
use adverse_support::*;

use pf_core::command::{Command, PieceSource, PointRef};
use pf_core::model::*;
use pf_core::{persist, DerivedKind, DerivedOp, Document, Project};
use pf_geometry::PointMm;
use std::panic::{catch_unwind, AssertUnwindSafe};

// ---------------------------------------------------------------------------
// US-04.6 — séparer / assembler : campagne aléatoire à graine fixe
// ---------------------------------------------------------------------------

#[derive(Default)]
struct SplitJoinStats {
    split_ok: usize,
    split_refused: usize,
    join_ok: usize,
    join_refused: usize,
    join_roundtrip_ok: usize,
    other_ok: usize,
    other_refused: usize,
    noops: usize,
    undone: usize,
    reloaded: usize,
    previews: usize,
    links_broken: usize,
    internals_carried: usize,
    grain_carried: usize,
}

/// Dessine une à trois pièces de formes variées sur la feuille (rectangle, cercle,
/// triangle, dôme à Bézier) et renvoie leurs identifiants.
fn sj_initial_pieces(r: &mut Rng, p: &mut Project, sheet: SheetId) {
    for k in 0..(1 + r.below(3)) {
        let at = PointMm::new(k as f64 * 300.0 + r.unit() * 40.0, r.unit() * 100.0);
        let seg = |p: &mut Project, a: PointMm, b: PointMm| {
            p.apply(Command::AddSegment {
                sheet,
                from: PointRef::New(a),
                to: PointRef::New(b),
            })
            .unwrap()
            .entities[0]
        };
        let entities = match r.below(4) {
            0 => {
                p.apply(Command::AddRectangle {
                    sheet,
                    corner: PointRef::New(at),
                    width_mm: 60.0 + r.unit() * 150.0,
                    height_mm: 40.0 + r.unit() * 120.0,
                })
                .unwrap()
                .entities
            }
            1 => {
                p.apply(Command::AddCircle {
                    sheet,
                    center: PointRef::New(at),
                    radius_mm: 20.0 + r.unit() * 60.0,
                })
                .unwrap()
                .entities
            }
            2 => {
                let c = PointMm::new(at.x + 50.0 + r.unit() * 50.0, at.y + 60.0 + r.unit() * 60.0);
                let b = PointMm::new(at.x + 80.0 + r.unit() * 80.0, at.y);
                vec![seg(p, at, b), seg(p, b, c), seg(p, c, at)]
            }
            _ => {
                let w = 80.0 + r.unit() * 100.0;
                let b = PointMm::new(at.x + w, at.y);
                let line = seg(p, at, b);
                let curve = p
                    .apply(Command::AddCurve {
                        sheet,
                        start: PointRef::New(b),
                        c1: PointMm::new(b.x, b.y + 60.0 + r.unit() * 60.0),
                        c2: PointMm::new(at.x, at.y + 60.0 + r.unit() * 60.0),
                        end: PointRef::New(at),
                    })
                    .unwrap()
                    .entities[0];
                vec![line, curve]
            }
        };
        p.apply(Command::ExtractPiece {
            sheet,
            source: PieceSource::Paths { entities },
            name: format!("Forme {k}"),
            detached: r.chance(25),
        })
        .unwrap();
    }
}

/// Boîte englobante du contour d'une pièce (0..100 si elle est illisible).
fn sj_box(cur: &Project, piece: PieceId) -> (PointMm, PointMm) {
    if cur.piece_sheet(piece).is_err() || cur.piece(piece).is_err() {
        return (PointMm::new(0.0, 0.0), PointMm::new(100.0, 100.0));
    }
    let (mut lo, mut hi) = (PointMm::new(1e9, 1e9), PointMm::new(-1e9, -1e9));
    for q in piece_polygon(cur, piece) {
        lo = PointMm::new(lo.x.min(q.x), lo.y.min(q.y));
        hi = PointMm::new(hi.x.max(q.x), hi.y.max(q.y));
    }
    if lo.x > hi.x {
        return (PointMm::new(0.0, 0.0), PointMm::new(100.0, 100.0));
    }
    (lo, hi)
}

/// Une commande de la campagne US-04.6. `None` : annuler, rétablir, relire.
fn sj_command(
    r: &mut Rng,
    cur: &Project,
    op: usize,
    halves: &mut Option<(PieceId, PieceId)>,
) -> Option<Command> {
    let sheets: Vec<SheetId> = cur.sheets.iter().map(|s| s.id).collect();
    let pieces: Vec<PieceId> = cur.pieces.iter().map(|p| p.id).collect();
    let sheet = pick(r, &sheets, SheetId(BOGUS));
    let piece = pick(r, &pieces, PieceId(BOGUS));
    Some(match op {
        0..=29 => {
            let (a, b) = sj_line(r, cur, piece);
            Command::SplitPiece { piece, a, b }
        }
        30..=49 => {
            // Les deux moitiés de la dernière séparation, par leurs bords de coupe ;
            // sinon deux pièces au hasard avec des bords de longueurs voisines.
            let (first, second) = match *halves {
                Some((l, rt)) if r.chance(60) => (l, rt),
                _ => (piece, pick(r, &pieces, PieceId(BOGUS))),
            };
            let edges = if let Some((l, rt)) = *halves {
                if (first, second) == (l, rt) {
                    let last = |id: PieceId| {
                        cur.piece(id)
                            .ok()
                            .and_then(|p| p.outline.last().map(|e| e.entity))
                    };
                    last(l).zip(last(rt))
                } else {
                    sj_matching_edges(cur, first, second)
                }
            } else {
                sj_matching_edges(cur, first, second)
            };
            let (first_edge, second_edge) = match edges {
                Some(e) if !r.chance(8) => e,
                _ => (EntityId(BOGUS), EntityId(BOGUS + 1)),
            };
            Command::JoinPieces {
                first,
                first_edge,
                second,
                second_edge,
                tolerance_mm: if r.chance(70) { 0.5 } else { sj_tolerance(r) },
            }
        }
        50..=53 => Command::AddRectangle {
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
            detached: r.chance(40),
        },
        61..=64 => Command::CreateDerivedPiece {
            mother: piece,
            kind: DerivedKind::ALL[r.below(4)],
            operation: DerivedOp::Offset {
                distance_mm: (r.unit() * 12.0 + 2.0) * if r.chance(60) { -1.0 } else { 1.0 },
            },
            name: None,
        },
        65..=68 => Command::SetPiecePlacement {
            piece,
            placement: Some(pf_core::Placement {
                x_mm: wild_coord(r),
                y_mm: wild_coord(r),
                rotation_deg: wild_rotation(r),
            }),
        },
        69..=70 => Command::DeletePiece { piece },
        71..=78 => {
            // Une ligne dans la boîte de la pièce (futur élément interne ou droit-fil).
            let (lo, hi) = sj_box(cur, piece);
            let at = |r: &mut Rng| {
                PointRef::New(PointMm::new(
                    lo.x + r.unit() * (hi.x - lo.x),
                    lo.y + r.unit() * (hi.y - lo.y),
                ))
            };
            Command::AddSegment {
                sheet: cur.piece_sheet(piece).map_or(sheet, |s| s.id),
                from: at(r),
                to: at(r),
            }
        }
        79..=80 => Command::ImportPieceInternals { piece },
        81..=86 => {
            let ents: Vec<EntityId> = cur
                .piece_sheet(piece)
                .map(|s| s.entities.iter().map(|e| e.id).collect())
                .unwrap_or_default();
            Command::AddPieceInternal {
                piece,
                target: InternalTarget::Path {
                    entity: match ents.last() {
                        Some(last) if r.chance(75) => *last,
                        _ => pick(r, &ents, EntityId(BOGUS)),
                    },
                },
                kind: InternalKind::LINES[r.below(4)],
            }
        }
        87..=89 => {
            // Droit-fil : un tracé quelconque de la feuille de la pièce.
            let ents: Vec<EntityId> = cur
                .piece_sheet(piece)
                .map(|s| s.entities.iter().map(|e| e.id).collect())
                .unwrap_or_default();
            Command::SetPieceOutline {
                piece,
                outline: cur
                    .piece(piece)
                    .map(|p| p.outline.clone())
                    .unwrap_or_default(),
                grainline: Some(pick(r, &ents, EntityId(BOGUS))),
            }
        }
        90 => {
            let pts: Vec<PointId> = cur
                .sheet(sheet)
                .map(|s| s.points.iter().map(|p| p.id).collect())
                .unwrap_or_default();
            Command::MovePoint {
                sheet,
                point: pick(r, &pts, PointId(BOGUS)),
                to: PointMm::new(r.unit() * 400.0 - 50.0, r.unit() * 400.0 - 50.0),
            }
        }
        _ => return None,
    })
}

/// Contrôles de cohérence après une commande acceptée : aucune référence morte, ids de
/// pièces uniques, lecture pure des aperçus.
fn sj_check_state(p: &Project, label: &str) {
    p.validate()
        .unwrap_or_else(|e| panic!("projet invalide : {label} : {e}"));
    let mut ids = std::collections::HashSet::new();
    for piece in &p.pieces {
        assert!(ids.insert(piece.id), "pièce en double : {label}");
        if let Some(d) = &piece.derived {
            assert!(p.piece(d.mother).is_ok(), "mère absente : {label}");
        }
        // Chaque contour est lisible ou signalé, jamais une panique.
        let _ = p
            .piece_card(piece.id)
            .unwrap_or_else(|e| panic!("{label} : {e}"));
    }
}

#[test]
fn us046_campagne_aleatoire_separer_assembler_graine_fixe() {
    let mut st = SplitJoinStats::default();
    for seq in 0..170u64 {
        let mut r = Rng(0x04_0600 + seq);
        let mut project = Project::new("Séparer aléa");
        let sheet = project.sheets[0].id;
        sj_initial_pieces(&mut r, &mut project, sheet);
        let initial_json = snap(&project);
        let mut doc = Document::new(project);
        let mut undo_stack: Vec<String> = Vec::new();
        let mut redo_stack: Vec<String> = Vec::new();
        let mut halves: Option<(PieceId, PieceId)> = None;
        for step in 0..22 {
            let label = format!("graine {seq} pas {step}");
            let before_json = snap(doc.project());
            let cur = doc.project().clone();
            let op = r.below(100);
            if let Some(cmd) = sj_command(&mut r, &cur, op, &mut halves) {
                let name = variant_name(&cmd);
                // Aperçu (lecture pure) : jamais de panique, jamais de modification.
                let preview_ok = match &cmd {
                    Command::SplitPiece { piece, a, b } => {
                        let Ok(res) =
                            catch_unwind(AssertUnwindSafe(|| cur.split_preview(*piece, *a, *b)))
                        else {
                            panic!("panique de split_preview : {label}");
                        };
                        st.previews += 1;
                        Some(res.is_ok())
                    }
                    Command::JoinPieces {
                        first,
                        first_edge,
                        second,
                        second_edge,
                        tolerance_mm,
                    } => {
                        let Ok(res) = catch_unwind(AssertUnwindSafe(|| {
                            cur.join_preview(
                                *first,
                                *first_edge,
                                *second,
                                *second_edge,
                                *tolerance_mm,
                            )
                        })) else {
                            panic!("panique de join_preview : {label}");
                        };
                        st.previews += 1;
                        Some(matches!(&res, Ok(pre) if pre.refusal.is_none()))
                    }
                    _ => None,
                };
                assert_eq!(
                    snap(&cur),
                    before_json,
                    "un aperçu a modifié le projet : {label}"
                );
                let is_sj = name == "SplitPiece" || name == "JoinPieces";
                let Ok(result) = catch_unwind(AssertUnwindSafe(|| doc.execute(cmd.clone()))) else {
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
                        if is_sj {
                            if name == "SplitPiece" {
                                st.split_refused += 1;
                            } else {
                                st.join_refused += 1;
                            }
                            // Un aperçu qui annonce un succès ne doit pas être démenti.
                            assert_ne!(
                                preview_ok,
                                Some(true),
                                "aperçu optimiste : {name} : {label} : {e}"
                            );
                        } else {
                            st.other_refused += 1;
                        }
                    }
                    Ok(out) => {
                        let after = doc.project();
                        sj_check_state(after, &label);
                        let changed = snap(after) != before_json;
                        if !changed {
                            st.noops += 1;
                        } else {
                            undo_stack.push(before_json.clone());
                            redo_stack.clear();
                        }
                        if is_sj {
                            assert!(changed, "{name} sans changement : {label}");
                            assert_eq!(
                                preview_ok,
                                Some(true),
                                "aperçu pessimiste : {name} : {label}"
                            );
                            // La construction existante est inchangée : seules des feuilles s'ajoutent.
                            assert_eq!(
                                serde_json::to_value(&after.sheets[..cur.sheets.len()]).unwrap(),
                                serde_json::to_value(&cur.sheets).unwrap(),
                                "{name} a touché la construction : {label}"
                            );
                            assert!(after.sheets.len() > cur.sheets.len(), "{label}");
                            for id in &out.pieces {
                                let piece = after.piece(*id).unwrap();
                                assert_eq!(piece.link, PieceLink::Detached, "{label}");
                                assert!(
                                    piece.derived.is_none() && piece.placement.is_none(),
                                    "{label}"
                                );
                                st.internals_carried += piece.internal.len();
                                st.grain_carried += usize::from(piece.grainline.is_some());
                                let c = after.piece_contour(*id).unwrap();
                                assert!(
                                    c.closed && !c.self_intersecting && c.area_mm2 > 0.0,
                                    "{label}"
                                );
                            }
                        }
                        match &cmd {
                            Command::SplitPiece { piece, .. } => {
                                st.split_ok += 1;
                                assert_eq!(out.pieces.len(), 2, "{label}");
                                // Somme des aires = aire d'origine.
                                let a0 = cur.piece_contour(*piece).unwrap().area_mm2;
                                let sum: f64 = out
                                    .pieces
                                    .iter()
                                    .map(|i| after.piece_contour(*i).unwrap().area_mm2)
                                    .sum();
                                assert!(
                                    (sum - a0).abs() <= a0 * 1e-9 + 1e-9,
                                    "aires {sum} ≠ {a0} : {label}"
                                );
                                assert!(after.piece(*piece).is_err(), "{label}");
                                st.links_broken += cur
                                    .derived_pieces_of(*piece)
                                    .iter()
                                    .filter(|d| after.piece(**d).is_ok_and(|p| p.derived.is_none()))
                                    .count();
                                halves = Some((out.pieces[0], out.pieces[1]));
                            }
                            Command::JoinPieces {
                                first,
                                second,
                                tolerance_mm,
                                ..
                            } => {
                                st.join_ok += 1;
                                assert_eq!(out.pieces.len(), 1, "{label}");
                                assert!(
                                    after.piece(*first).is_err() && after.piece(*second).is_err()
                                );
                                let (a, b) = (
                                    cur.piece_contour(*first).unwrap(),
                                    cur.piece_contour(*second).unwrap(),
                                );
                                let m = after.piece_contour(out.pieces[0]).unwrap();
                                let slack = tolerance_mm * (a.perimeter_mm + b.perimeter_mm)
                                    + 1e-6 * (a.area_mm2 + b.area_mm2);
                                assert!(
                                    (m.area_mm2 - (a.area_mm2 + b.area_mm2)).abs() <= slack,
                                    "aire assemblée {} ≠ {} + {} : {label}",
                                    m.area_mm2,
                                    a.area_mm2,
                                    b.area_mm2
                                );
                                assert!(
                                    out.notes.iter().any(|n| n.contains("Écart de longueur")),
                                    "écart non affiché : {label}"
                                );
                                if halves.is_some_and(|(l, rt)| (l, rt) == (*first, *second)) {
                                    st.join_roundtrip_ok += 1;
                                }
                                halves = None;
                            }
                            _ => st.other_ok += 1,
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
        }
        // Fin de séquence : relecture identique à l'octet ; tout annuler redonne le départ.
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
        "séparer/assembler : {} séparations ({} refusées), {} assemblages ({} aller-retour, {} \
         refusés), {} autres commandes acceptées, {} refusées, {} sans changement, {} annulations, \
         {} relectures, {} aperçus, {} liens de dérivées cassés, {} éléments internes et {} \
         droit-fil reportés",
        st.split_ok,
        st.split_refused,
        st.join_ok,
        st.join_roundtrip_ok,
        st.join_refused,
        st.other_ok,
        st.other_refused,
        st.noops,
        st.undone,
        st.reloaded,
        st.previews,
        st.links_broken,
        st.internals_carried,
        st.grain_carried
    );
    assert!(st.split_ok >= 150, "seulement {} séparations", st.split_ok);
    assert!(
        st.split_refused >= 150,
        "seulement {} refus de séparation",
        st.split_refused
    );
    assert!(st.join_ok >= 30, "seulement {} assemblages", st.join_ok);
    assert!(
        st.join_refused >= 100,
        "seulement {} refus d'assemblage",
        st.join_refused
    );
    assert!(
        st.join_roundtrip_ok >= 10,
        "seulement {} aller-retour",
        st.join_roundtrip_ok
    );
    assert!(
        st.other_ok >= 300,
        "seulement {} autres commandes",
        st.other_ok
    );
    assert!(
        st.undone >= 50 && st.reloaded >= 30,
        "{} / {}",
        st.undone,
        st.reloaded
    );
}
