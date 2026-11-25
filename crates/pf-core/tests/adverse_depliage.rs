//! Campagne adverse : depliage (graine fixe).

mod adverse_support;
use adverse_support::*;

use pf_core::command::{Command, PieceSource, PointRef};
use pf_core::model::*;
use pf_core::{contour, persist, DerivedKind, DerivedOp, Document, Project};
use pf_geometry::PointMm;
use std::panic::{catch_unwind, AssertUnwindSafe};

// ---------------------------------------------------------------------------
// US-04.7 — déplier : campagne aléatoire à graine fixe
// ---------------------------------------------------------------------------

#[derive(Default)]
struct UnfoldStats {
    unfold_ok: usize,
    unfold_refused: usize,
    previews: usize,
    other_ok: usize,
    other_refused: usize,
    noops: usize,
    undone: usize,
    reloaded: usize,
    regenerated: usize,
    stale_seen: usize,
    links_broken: usize,
    checked: usize,
    curved_checked: usize,
}

/// Une à trois pièces : demi-corps polygonal (axe vertical ou incliné), rectangle,
/// dôme à Bézier, demi-disque à arc, triangle, cercle (non dépliable).
fn uf_initial_pieces(r: &mut Rng, p: &mut Project, sheet: SheetId) {
    for k in 0..(1 + r.below(3)) {
        let at = PointMm::new(k as f64 * 400.0 + r.unit() * 40.0, r.unit() * 80.0);
        let angle = if r.chance(40) {
            r.unit() * std::f64::consts::TAU
        } else {
            0.0
        };
        let (sn, cs) = angle.sin_cos();
        // Repère local → feuille : rotation autour de `at`.
        let f = move |x: f64, y: f64| PointMm::new(at.x + x * cs - y * sn, at.y + x * sn + y * cs);
        let seg = |p: &mut Project, a: PointMm, b: PointMm| {
            p.apply(Command::AddSegment {
                sheet,
                from: PointRef::New(a),
                to: PointRef::New(b),
            })
            .unwrap()
            .entities[0]
        };
        let entities = match r.below(6) {
            0 => {
                let pts = [
                    f(0.0, 0.0),
                    f(40.0 + r.unit() * 60.0, 0.0),
                    f(60.0 + r.unit() * 60.0, 40.0 + r.unit() * 30.0),
                    f(30.0 + r.unit() * 60.0, 90.0 + r.unit() * 30.0),
                    f(0.0, 110.0 + r.unit() * 20.0),
                ];
                (0..pts.len())
                    .map(|i| seg(p, pts[i], pts[(i + 1) % pts.len()]))
                    .collect::<Vec<_>>()
            }
            1 => {
                let (w, h) = (60.0 + r.unit() * 120.0, 40.0 + r.unit() * 100.0);
                let pts = [f(0.0, 0.0), f(w, 0.0), f(w, h), f(0.0, h)];
                (0..4).map(|i| seg(p, pts[i], pts[(i + 1) % 4])).collect()
            }
            2 => {
                let w = 80.0 + r.unit() * 100.0;
                let line = seg(p, f(0.0, 0.0), f(w, 0.0));
                let curve = p
                    .apply(Command::AddCurve {
                        sheet,
                        start: PointRef::New(f(w, 0.0)),
                        c1: f(w, 60.0 + r.unit() * 60.0),
                        c2: f(0.0, 60.0 + r.unit() * 60.0),
                        end: PointRef::New(f(0.0, 0.0)),
                    })
                    .unwrap()
                    .entities[0];
                vec![line, curve]
            }
            3 => {
                let rad = 30.0 + r.unit() * 60.0;
                let line = seg(p, f(-rad, 0.0), f(rad, 0.0));
                let arc = p
                    .apply(Command::AddArcThreePoints {
                        sheet,
                        start: PointRef::New(f(rad, 0.0)),
                        through: PointRef::New(f(0.0, rad)),
                        end: PointRef::New(f(-rad, 0.0)),
                    })
                    .unwrap()
                    .entities[0];
                vec![line, arc]
            }
            4 => {
                let pts = [
                    f(0.0, 0.0),
                    f(60.0 + r.unit() * 80.0, 0.0),
                    f(20.0 + r.unit() * 40.0, 50.0 + r.unit() * 80.0),
                ];
                (0..3).map(|i| seg(p, pts[i], pts[(i + 1) % 3])).collect()
            }
            _ => {
                p.apply(Command::AddCircle {
                    sheet,
                    center: PointRef::New(at),
                    radius_mm: 20.0 + r.unit() * 50.0,
                })
                .unwrap()
                .entities
            }
        };
        p.apply(Command::ExtractPiece {
            sheet,
            source: PieceSource::Paths { entities },
            name: format!("Moitié {k}"),
            detached: r.chance(20),
        })
        .unwrap();
    }
}

/// Une pliure plausible (un segment du contour) ou, parfois, une pliure absurde.
fn uf_fold(r: &mut Rng, cur: &Project, piece: PieceId) -> EntityId {
    let cands = cur.unfold_candidates(piece);
    if cands.is_empty() || r.chance(8) {
        let all: Vec<EntityId> = cur
            .piece_sheet(piece)
            .map(|s| s.entities.iter().map(|e| e.id).collect())
            .unwrap_or_default();
        return pick(r, &all, EntityId(BOGUS));
    }
    cands[r.below(cands.len())].entity
}

/// Une commande de la campagne US-04.7. `None` : annuler, rétablir, relire.
fn uf_command(r: &mut Rng, cur: &Project, op: usize) -> Option<Command> {
    let sheets: Vec<SheetId> = cur.sheets.iter().map(|s| s.id).collect();
    let pieces: Vec<PieceId> = cur.pieces.iter().map(|p| p.id).collect();
    let sheet = pick(r, &sheets, SheetId(BOGUS));
    let piece = pick(r, &pieces, PieceId(BOGUS));
    Some(match op {
        0..=34 => Command::UnfoldPiece {
            piece,
            fold: uf_fold(r, cur, piece),
            name: None,
        },
        35..=42 => {
            // Une dépliée au hasard, ou sa mère : séparer, assembler, dériver, casser.
            let (a, b) = sj_line(r, cur, piece);
            Command::SplitPiece { piece, a, b }
        }
        43..=46 => {
            let second = pick(r, &pieces, PieceId(BOGUS));
            let edges = sj_matching_edges(cur, piece, second);
            let (first_edge, second_edge) = match edges {
                Some(e) if !r.chance(10) => e,
                _ => (EntityId(BOGUS), EntityId(BOGUS + 1)),
            };
            Command::JoinPieces {
                first: piece,
                first_edge,
                second,
                second_edge,
                tolerance_mm: 0.5,
            }
        }
        47..=50 => Command::CreateDerivedPiece {
            mother: piece,
            kind: DerivedKind::ALL[r.below(4)],
            operation: DerivedOp::Offset {
                distance_mm: (r.unit() * 8.0 + 2.0) * if r.chance(60) { -1.0 } else { 1.0 },
            },
            name: None,
        },
        51..=56 => Command::BreakDerivedLink { piece },
        57..=59 => Command::DeletePiece { piece },
        60..=62 => Command::SetPiecePlacement {
            piece,
            placement: Some(pf_core::Placement {
                x_mm: wild_coord(r),
                y_mm: wild_coord(r),
                rotation_deg: wild_rotation(r),
            }),
        },
        63..=76 => {
            // Déplacer un point : le plus souvent de la feuille de la mère (régénération,
            // obsolescence), parfois d'une feuille de dépliée (refusé tant que le lien existe).
            let target = if r.chance(85) {
                cur.piece_sheet(piece).map_or(sheet, |s| s.id)
            } else {
                sheet
            };
            let pts: Vec<(PointId, PointMm)> = cur
                .sheet(target)
                .map(|s| {
                    s.points
                        .iter()
                        .filter_map(|p| s.position(p.id).ok().map(|at| (p.id, at)))
                        .collect()
                })
                .unwrap_or_default();
            let (point, at) = if pts.is_empty() || r.chance(4) {
                (PointId(BOGUS), PointMm::new(0.0, 0.0))
            } else {
                pts[r.below(pts.len())]
            };
            let to = if r.chance(15) {
                PointMm::new(r.unit() * 500.0 - 100.0, r.unit() * 400.0 - 100.0)
            } else {
                PointMm::new(at.x + r.unit() * 50.0 - 25.0, at.y + r.unit() * 50.0 - 25.0)
            };
            Command::MovePoint {
                sheet: target,
                point,
                to,
            }
        }
        77..=79 => {
            // Le contour de la pièce, privé d'un tracé (ouvert) ou tel quel avec un droit-fil.
            let outline = cur
                .piece(piece)
                .map(|p| p.outline.clone())
                .unwrap_or_default();
            let mut kept = outline.clone();
            if !kept.is_empty() && r.chance(60) {
                let at = r.below(kept.len());
                kept.remove(at);
            }
            Command::SetPieceOutline {
                piece,
                outline: kept,
                grainline: None,
            }
        }
        80..=81 => Command::DuplicateSheet { sheet },
        82 => Command::AddRectangle {
            sheet,
            corner: PointRef::New(PointMm::new(r.unit() * 300.0, r.unit() * 300.0)),
            width_mm: 20.0 + r.unit() * 150.0,
            height_mm: 20.0 + r.unit() * 150.0,
        },
        83..=84 => Command::ExtractPiece {
            sheet,
            source: PieceSource::Click {
                at: PointMm::new(r.unit() * 500.0, r.unit() * 300.0),
            },
            name: format!("Pièce {}", r.below(1000)),
            detached: r.chance(40),
        },
        _ => return None,
    })
}

/// Invariants des pièces dépliées après une commande acceptée : à jour = entière,
/// symétrique, d'aire double ; obsolète = non bougée pendant la commande ; jamais de
/// panique de lecture.
fn uf_check_state(before: Option<&Project>, now: &Project, label: &str, st: &mut UnfoldStats) {
    now.validate()
        .unwrap_or_else(|e| panic!("projet invalide : {label} : {e}"));
    for u in &now.pieces {
        let Some(spec) = &u.derived else { continue };
        let DerivedOp::Unfold { fold } = spec.operation else {
            continue;
        };
        assert_eq!(spec.kind, DerivedKind::Unfolded, "{label}");
        assert!(matches!(u.link, PieceLink::Linked { .. }), "{label}");
        let mother = now
            .piece(spec.mother)
            .unwrap_or_else(|_| panic!("mère absente : {label}"));
        let Ok(card) = catch_unwind(AssertUnwindSafe(|| now.piece_card(u.id))) else {
            panic!("piece_card panique : {label}");
        };
        card.unwrap_or_else(|e| panic!("{label} : {e}"));
        match &spec.stale {
            Some(_) => {
                st.stale_seen += 1;
                if let Some(b) = before.and_then(|b| b.piece(u.id).ok()) {
                    if b.derived.is_some() {
                        assert_eq!(b.outline, u.outline, "dépliée obsolète bougée : {label}");
                        let PieceLink::Linked { sheet: sid } = u.link else {
                            unreachable!()
                        };
                        // La feuille peut recevoir d'autres tracés (construction libre) :
                        // seule la géométrie du contour ne doit pas bouger.
                        for e in &u.outline {
                            assert_eq!(
                                before.unwrap().sheet(sid).unwrap().shape(e.entity),
                                now.sheet(sid).unwrap().shape(e.entity),
                                "contour d'une dépliée obsolète modifié : {label}"
                            );
                        }
                    }
                }
            }
            None => {
                assert!(
                    mother.outline.iter().any(|e| e.entity == fold),
                    "pliure hors du contour d'une dépliée à jour : {label}"
                );
                let cm = now
                    .piece_contour(mother.id)
                    .unwrap_or_else(|e| panic!("source illisible ({e}) : {label}"));
                let cu = now
                    .piece_contour(u.id)
                    .unwrap_or_else(|e| panic!("dépliée illisible ({e}) : {label}"));
                assert!(
                    cu.closed && !cu.self_intersecting && !cu.clockwise,
                    "{label}"
                );
                assert!(
                    (cu.area_mm2 - 2.0 * cm.area_mm2).abs() <= 1e-9 * cu.area_mm2 + 1e-9,
                    "aire {} ≠ 2 × {} : {label}",
                    cu.area_mm2,
                    cm.area_mm2
                );
                // Symétrie par rapport à la droite portée par la pliure.
                let ms = now.piece_sheet(mother.id).unwrap();
                let (pa, pb) = ms.segment_point_ids(fold).unwrap();
                let (a, b) = (ms.position(pa).unwrap(), ms.position(pb).unwrap());
                let dir = (b - a).unit().unwrap();
                let line = piece_polyline(now, u.id);
                let reflect = |q: PointMm| {
                    let v = q - a;
                    let k = 2.0 * dir.dot(v);
                    PointMm::new(a.x + dir.x * k - v.x, a.y + dir.y * k - v.y)
                };
                for q in &line {
                    let image = reflect(*q);
                    assert!(
                        line.iter()
                            .any(|x| pf_geometry::distance_mm(*x, image) < 1e-4),
                        "pas de symétrique pour {q:?} : {label}"
                    );
                }
                st.checked += 1;
                let us = now.piece_sheet(u.id).unwrap();
                if u.outline
                    .iter()
                    .any(|e| !matches!(us.shape(e.entity), Ok(pf_core::Shape::Line(..))))
                {
                    st.curved_checked += 1;
                }
            }
        }
    }
}

/// Polyligne (48 pas par courbe) du contour d'une pièce.
fn piece_polyline(p: &Project, piece: PieceId) -> Vec<PointMm> {
    let sheet = p.piece_sheet(piece).unwrap();
    let edges: Vec<contour::ContourEdge> = p
        .piece(piece)
        .unwrap()
        .outline
        .iter()
        .map(|e| contour::ContourEdge {
            entity: e.entity,
            reversed: e.reversed,
        })
        .collect();
    contour::contour_polyline(sheet, &edges, 48).unwrap_or_default()
}

#[test]
fn us047_campagne_aleatoire_depliage_graine_fixe() {
    let mut st = UnfoldStats::default();
    for seq in 0..170u64 {
        let mut r = Rng(0x04_0700 + seq);
        let mut project = Project::new("Déplier aléa");
        let sheet = project.sheets[0].id;
        uf_initial_pieces(&mut r, &mut project, sheet);
        let initial_json = snap(&project);
        let mut doc = Document::new(project);
        let mut undo_stack: Vec<String> = Vec::new();
        let mut redo_stack: Vec<String> = Vec::new();
        for step in 0..22 {
            let label = format!("graine {seq} pas {step}");
            let before_json = snap(doc.project());
            let cur = doc.project().clone();
            let op = r.below(100);
            if let Some(cmd) = uf_command(&mut r, &cur, op) {
                let name = variant_name(&cmd);
                // Aperçu (lecture pure) : jamais de panique, jamais de modification.
                let preview_ok = match &cmd {
                    Command::UnfoldPiece { piece, fold, name } => {
                        let Ok(res) = catch_unwind(AssertUnwindSafe(|| {
                            cur.unfold_preview(*piece, *fold, name.clone())
                        })) else {
                            panic!("panique de unfold_preview : {label}");
                        };
                        st.previews += 1;
                        Some(res.is_ok())
                    }
                    _ => None,
                };
                assert_eq!(
                    snap(&cur),
                    before_json,
                    "un aperçu a modifié le projet : {label}"
                );
                let is_unfold = name == "UnfoldPiece";
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
                        if is_unfold {
                            st.unfold_refused += 1;
                            assert_ne!(preview_ok, Some(true), "aperçu optimiste : {label} : {e}");
                        } else {
                            st.other_refused += 1;
                        }
                    }
                    Ok(out) => {
                        let after = doc.project();
                        uf_check_state(Some(&cur), after, &label, &mut st);
                        let changed = snap(after) != before_json;
                        if !changed {
                            st.noops += 1;
                        } else {
                            undo_stack.push(before_json.clone());
                            redo_stack.clear();
                        }
                        st.regenerated += out.regenerated.len();
                        if is_unfold {
                            st.unfold_ok += 1;
                            assert!(changed, "dépliage sans changement : {label}");
                            assert_eq!(preview_ok, Some(true), "aperçu pessimiste : {label}");
                            assert_eq!(out.pieces.len(), 1, "{label}");
                            // La construction existante est inchangée : seule une feuille s'ajoute.
                            assert_eq!(
                                serde_json::to_value(&after.sheets[..cur.sheets.len()]).unwrap(),
                                serde_json::to_value(&cur.sheets).unwrap(),
                                "le dépliage a touché la construction : {label}"
                            );
                            assert_eq!(after.sheets.len(), cur.sheets.len() + 1, "{label}");
                            let u = after.piece(out.pieces[0]).unwrap();
                            assert!(
                                matches!(
                                    u.derived.as_ref().map(|d| d.operation),
                                    Some(DerivedOp::Unfold { .. })
                                ),
                                "{label}"
                            );
                        } else {
                            st.other_ok += 1;
                        }
                        if matches!(&cmd, Command::BreakDerivedLink { .. }) {
                            st.links_broken += 1;
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
        "déplier : {} dépliages ({} refusés), {} aperçus, {} autres commandes acceptées, {} \
         refusées, {} sans changement, {} annulations, {} relectures, {} régénérations, {} états \
         obsolètes vus, {} liens cassés, {} dépliées contrôlées dont {} courbes",
        st.unfold_ok,
        st.unfold_refused,
        st.previews,
        st.other_ok,
        st.other_refused,
        st.noops,
        st.undone,
        st.reloaded,
        st.regenerated,
        st.stale_seen,
        st.links_broken,
        st.checked,
        st.curved_checked
    );
    assert!(st.unfold_ok >= 150, "seulement {} dépliages", st.unfold_ok);
    assert!(
        st.unfold_refused >= 100,
        "seulement {} refus de dépliage",
        st.unfold_refused
    );
    assert!(
        st.regenerated >= 40,
        "seulement {} régénérations",
        st.regenerated
    );
    assert!(
        st.stale_seen >= 10,
        "seulement {} états obsolètes",
        st.stale_seen
    );
    assert!(
        st.links_broken >= 10,
        "seulement {} liens cassés",
        st.links_broken
    );
    assert!(st.checked >= 300, "seulement {} contrôles", st.checked);
    assert!(
        st.curved_checked >= 50,
        "seulement {} dépliées courbes",
        st.curved_checked
    );
    assert!(
        st.undone >= 50 && st.reloaded >= 30,
        "{} / {}",
        st.undone,
        st.reloaded
    );
}
