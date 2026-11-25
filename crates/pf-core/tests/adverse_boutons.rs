//! Campagne adverse : boutons (graine fixe).

mod adverse_support;
use adverse_support::*;

use pf_core::command::{Command, PieceSource, PointRef};
use pf_core::model::*;
use pf_core::{persist, DerivedKind, DerivedOp, Document, Project};
use pf_geometry::PointMm;
use std::panic::{catch_unwind, AssertUnwindSafe};

// ---------------------------------------------------------------------------
// US-06.4 — boutons et boutonnières répartis entre deux points
// ---------------------------------------------------------------------------

use pf_core::piece_buttons::{
    BUTTONHOLE_MAX_LENGTH_MM, BUTTONHOLE_MAX_WIDTH_MM, BUTTONHOLE_MIN_LENGTH_MM,
    BUTTONHOLE_MIN_WIDTH_MM, BUTTON_MAX_COUNT, BUTTON_MAX_DIAMETER_MM, BUTTON_MAX_EDGE_MARGIN_MM,
    BUTTON_MIN_DIAMETER_MM,
};

use pf_core::{ButtonElement, ButtonGroup, ButtonShape, ButtonholeOrientation};

fn bt_count(r: &mut Rng) -> u32 {
    match r.below(10) {
        0..=6 => 1 + r.below(8) as u32,
        7 => [1, 2, BUTTON_MAX_COUNT][r.below(3)],
        8 => [0, BUTTON_MAX_COUNT + 1, 1_000, u32::MAX][r.below(4)],
        _ => 1 + r.below(BUTTON_MAX_COUNT as usize) as u32,
    }
}

fn bt_orientation(r: &mut Rng) -> ButtonholeOrientation {
    match r.below(4) {
        0 => ButtonholeOrientation::Horizontal {},
        1 => ButtonholeOrientation::Vertical {},
        2 => ButtonholeOrientation::Perpendicular {},
        _ => ButtonholeOrientation::Angle {
            degrees: match r.below(8) {
                0..=3 => r.unit() * 360.0,
                4 => (r.unit() - 0.5) * 2000.0,
                5 => [0.0, 90.0, 180.0, 270.0, 360.0, -90.0, -0.0][r.below(7)],
                6 => [f64::NAN, f64::INFINITY, f64::NEG_INFINITY][r.below(3)],
                _ => {
                    let e = extreme_numbers();
                    e[r.below(e.len())]
                }
            },
        },
    }
}

fn bt_element(r: &mut Rng) -> ButtonElement {
    if r.chance(45) {
        ButtonElement::Button {
            diameter_mm: mk_size(r, BUTTON_MIN_DIAMETER_MM, BUTTON_MAX_DIAMETER_MM),
        }
    } else {
        ButtonElement::Buttonhole {
            length_mm: mk_size(r, BUTTONHOLE_MIN_LENGTH_MM, BUTTONHOLE_MAX_LENGTH_MM),
            width_mm: mk_size(r, BUTTONHOLE_MIN_WIDTH_MM, BUTTONHOLE_MAX_WIDTH_MM),
            orientation: bt_orientation(r),
        }
    }
}

fn bt_margin(r: &mut Rng) -> f64 {
    if r.chance(70) {
        r.unit() * 30.0
    } else {
        mk_size(r, 0.0, BUTTON_MAX_EDGE_MARGIN_MM)
    }
}

/// Une commande de la campagne US-06.4 (ou `None` : annuler, rétablir, relire).
fn bt_command(r: &mut Rng, cur: &Project, sheet: SheetId, op: usize) -> Option<Command> {
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
    let known: Vec<ButtonGroup> = cur
        .piece(piece)
        .map(|p| p.buttons.clone())
        .unwrap_or_default();
    let known_id = |r: &mut Rng| {
        if !known.is_empty() && r.chance(88) {
            known[r.below(known.len())].id
        } else {
            ButtonGroupId(BOGUS)
        }
    };
    let add = |r: &mut Rng, piece: PieceId, pts: &[PointId]| {
        let from = pick(r, pts, PointId(BOGUS));
        Command::AddButtonGroup {
            piece,
            from,
            // Une fois sur quinze : deux fois le même point.
            to: if r.chance(7) {
                from
            } else {
                pick(r, pts, PointId(BOGUS))
            },
            count: bt_count(r),
            element: bt_element(r),
            edge_margin_mm: bt_margin(r),
        }
    };
    Some(match op {
        0..=27 => add(r, piece, &pts),
        28..=39 => {
            let group = known_id(r);
            match known.iter().find(|g| g.id == group) {
                // Une fois sur trois : les mêmes valeurs (sans effet, aucune étape).
                Some(g) if r.chance(33) => Command::SetButtonGroup {
                    piece,
                    group,
                    from: g.from,
                    to: g.to,
                    count: g.count,
                    element: g.element,
                    edge_margin_mm: g.edge_margin_mm,
                },
                _ => Command::SetButtonGroup {
                    piece,
                    group,
                    from: pick(r, &pts, PointId(BOGUS)),
                    to: pick(r, &pts, PointId(BOGUS)),
                    count: bt_count(r),
                    element: bt_element(r),
                    edge_margin_mm: bt_margin(r),
                },
            }
        }
        40..=47 => Command::RemoveButtonGroup {
            piece,
            group: known_id(r),
        },
        48..=57 => {
            let point = pick(r, &main_pts, PointId(BOGUS));
            // Parfois exactement sur un autre point : les deux points d'un groupe confondus.
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
            // Surtout les points qui portent un groupe : ils doivent être retirés avec lui.
            let anchors: Vec<PointId> = cur
                .pieces
                .iter()
                .flat_map(|p| p.buttons.iter().flat_map(|g| [g.from, g.to]))
                .filter(|p| main_pts.contains(p))
                .collect();
            let points = if !anchors.is_empty() && r.chance(75) {
                let mut v = vec![anchors[r.below(anchors.len())]];
                if r.chance(40) {
                    v.push(anchors[r.below(anchors.len())]);
                }
                v
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
                        Command::RemoveButtonGroup {
                            piece,
                            group: known_id(r),
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
struct ButtonStats {
    accepted: usize,
    refused: usize,
    noops: usize,
    added: usize,
    buttons: usize,
    holes: usize,
    elements: usize,
    too_close: usize,
    unjudged: usize,
    oracle_checked: usize,
    pruned: usize,
    splits_carried: usize,
    split_lost: usize,
    joins: usize,
    copies: usize,
    unfolds_noted: usize,
    derived_with_groups: usize,
    own_pruned: usize,
    undone: usize,
    reloaded: usize,
    exports: usize,
}

/// Distance de deux segments `[a, b]` et `[c, d]`, formule fermée écrite ici (indépendante du
/// cœur) : 0 s'ils se coupent, sinon le plus petit écart entre une extrémité et l'autre segment.
fn oracle_seg_seg(a: PointMm, b: PointMm, c: PointMm, d: PointMm) -> f64 {
    let cross =
        |o: PointMm, p: PointMm, q: PointMm| (p.x - o.x) * (q.y - o.y) - (p.y - o.y) * (q.x - o.x);
    let (d1, d2, d3, d4) = (
        cross(a, b, c),
        cross(a, b, d),
        cross(c, d, a),
        cross(c, d, b),
    );
    if ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0))
        && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0))
    {
        return 0.0;
    }
    let point_seg = |p: PointMm, s: PointMm, e: PointMm| {
        let (vx, vy) = (e.x - s.x, e.y - s.y);
        let l2 = vx * vx + vy * vy;
        let t = if l2 <= 0.0 {
            0.0
        } else {
            (((p.x - s.x) * vx + (p.y - s.y) * vy) / l2).clamp(0.0, 1.0)
        };
        (p.x - (s.x + vx * t)).hypot(p.y - (s.y + vy * t))
    };
    point_seg(a, c, d)
        .min(point_seg(b, c, d))
        .min(point_seg(c, a, b))
        .min(point_seg(d, a, b))
}

/// Polygone d'une pièce dont le contour fermé n'a que des segments (`None` sinon).
fn straight_polygon(now: &Project, piece: PieceId) -> Option<Vec<PointMm>> {
    let contour = now.piece_contour(piece).ok()?;
    if !contour.closed {
        return None;
    }
    let sheet = now.piece_sheet(piece).ok()?;
    let mut pts = Vec::new();
    for e in &contour.edges {
        match sheet.shape(e.entity).ok()? {
            Shape::Line(a, b) => pts.push(if e.reversed { b } else { a }),
            _ => return None,
        }
    }
    Some(pts)
}

/// Invariants vérifiés après chaque pas : jamais de référence morte, identifiants uniques,
/// bornes, positions égales à un recalcul indépendant (premier élément au premier point, dernier
/// au second, espacement égal), recul recalculé indépendamment sur les contours à segments,
/// cohérence fiche / liste / avertissement, lectures sans panique.
fn check_buttons_state(now: &Project, label: &str, st: &mut ButtonStats) {
    use pf_core::piece::PieceIssue;
    let mut ids = std::collections::BTreeSet::new();
    for piece in &now.pieces {
        for g in &piece.buttons {
            assert!(
                ids.insert(g.id.0),
                "identifiant de groupe en double : {label}"
            );
            assert!(g.id.0 < now.next_id, "{label}");
            assert!(g.element.validate_stored().is_ok(), "{g:?} : {label}");
            assert!((1..=BUTTON_MAX_COUNT).contains(&g.count), "{label}");
            assert!(
                g.edge_margin_mm.is_finite()
                    && (0.0..=BUTTON_MAX_EDGE_MARGIN_MM).contains(&g.edge_margin_mm),
                "{label}"
            );
            assert_ne!(g.from, g.to, "{label}");
            let alive = match piece.link {
                PieceLink::Linked { sheet } => {
                    let s = now.sheet(sheet).unwrap();
                    s.point(g.from).is_ok() && s.point(g.to).is_ok()
                }
                PieceLink::Detached => now
                    .sheets
                    .iter()
                    .any(|s| s.point(g.from).is_ok() && s.point(g.to).is_ok()),
            };
            assert!(alive, "référence morte {g:?} : {label}");
            match g.element {
                ButtonElement::Button { .. } => st.buttons += 1,
                ButtonElement::Buttonhole { .. } => st.holes += 1,
            }
        }
        let (list, card, issues) = catch_unwind(AssertUnwindSafe(|| {
            (
                now.piece_buttons(piece.id),
                now.piece_card(piece.id),
                now.piece_issues(piece.id),
            )
        }))
        .unwrap_or_else(|_| panic!("lecture panique : {label}"));
        assert_eq!(
            list.len(),
            piece.buttons.len(),
            "groupe illisible : {label}"
        );
        let position = |id: PointId| {
            now.sheets
                .iter()
                .find_map(|s| s.position(id).ok())
                .unwrap_or_else(|| panic!("point sans position : {label}"))
        };
        let polygon = straight_polygon(now, piece.id);
        let mut flagged = 0;
        for (res, g) in list.iter().zip(&piece.buttons) {
            assert_eq!(res.group, *g, "{label}");
            let (a, b) = (position(g.from), position(g.to));
            assert_eq!(res.elements.len(), g.count as usize, "{label}");
            // Positions : premier élément au premier point, dernier au second, pas régulier.
            let n = g.count as usize;
            for (i, e) in res.elements.iter().enumerate() {
                assert_eq!(e.index, i, "{label}");
                assert!(e.center.x.is_finite() && e.center.y.is_finite(), "{label}");
                let want = if i == 0 {
                    a
                } else if i == n - 1 {
                    b
                } else {
                    let t = i as f64 / (n - 1) as f64;
                    PointMm::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t)
                };
                let tol = 1e-6 * (1.0 + want.x.abs().max(want.y.abs()));
                assert!(
                    (e.center.x - want.x).abs() < tol && (e.center.y - want.y).abs() < tol,
                    "position {:?} au lieu de {want:?} : {label}",
                    e.center
                );
                st.elements += 1;
            }
            if n > 1 {
                let gap = (b.x - a.x).hypot(b.y - a.y) / (n - 1) as f64;
                assert!((res.spacing_mm - gap).abs() < 1e-6 * (1.0 + gap), "{label}");
                for w in res.elements.windows(2) {
                    let d = (w[1].center.x - w[0].center.x).hypot(w[1].center.y - w[0].center.y);
                    assert!(
                        (d - gap).abs() < 1e-6 * (1.0 + gap),
                        "pas irrégulier : {label}"
                    );
                }
            }
            assert!((0.0..360.0).contains(&res.axis_deg), "{label}");
            for e in &res.elements {
                match (g.element, e.shape) {
                    (
                        ButtonElement::Button { diameter_mm },
                        ButtonShape::Button { diameter_mm: d },
                    ) => {
                        assert_eq!(diameter_mm, d, "{label}");
                    }
                    (
                        ButtonElement::Buttonhole {
                            length_mm,
                            width_mm,
                            ..
                        },
                        ButtonShape::Buttonhole {
                            length_mm: l,
                            width_mm: w,
                            angle_deg,
                        },
                    ) => {
                        assert_eq!((length_mm, width_mm), (l, w), "{label}");
                        assert!(
                            (0.0..360.0).contains(&angle_deg),
                            "angle {angle_deg} : {label}"
                        );
                    }
                    other => panic!("forme incohérente {other:?} : {label}"),
                }
                match e.clearance_mm {
                    Some(c) => {
                        assert!(c.is_finite(), "{label}");
                        assert_eq!(e.too_close, c < g.edge_margin_mm - 1e-9, "{label}");
                    }
                    None => {
                        assert!(!e.too_close, "{label}");
                        st.unjudged += 1;
                    }
                }
                flagged += usize::from(e.too_close);
                // Recul recalculé indépendamment (contour de segments seulement).
                if let (Some(poly), Some(got)) = (&polygon, e.clearance_mm) {
                    let (s0, s1) = e.shape.spine(e.center);
                    let r = e.shape.radius_mm();
                    let d = (0..poly.len())
                        .map(|k| oracle_seg_seg(s0, s1, poly[k], poly[(k + 1) % poly.len()]))
                        .fold(f64::INFINITY, f64::min);
                    let inside = pf_geometry::point_in_polygon(e.center, poly);
                    let want = if d <= 1e-9 {
                        -r
                    } else if inside {
                        d - r
                    } else {
                        -(d + r)
                    };
                    assert!(
                        (got - want).abs() < 1e-6 * (1.0 + want.abs()),
                        "recul {got} au lieu de {want} : {label}"
                    );
                    st.oracle_checked += 1;
                }
            }
        }
        st.too_close += flagged;
        let card = card.unwrap();
        assert_eq!(card.issues, issues, "{label}");
        let reported = issues.iter().find_map(|i| match i {
            PieceIssue::ButtonsTooCloseToEdge { count } => Some(*count),
            _ => None,
        });
        assert_eq!(reported, (flagged > 0).then_some(flagged), "{label}");
    }
}

/// Export : un élément exporté par élément, un seul groupe nommé, jamais de jeton non fini.
fn check_buttons_export(p: &Project, label: &str, st: &mut ButtonStats) {
    use pf_export::{svg::export_svg, ExportError, Scene};
    for s in &p.sheets {
        let scene = match catch_unwind(AssertUnwindSafe(|| Scene::from_sheet(p, s.id)))
            .unwrap_or_else(|_| panic!("{label} : PANIC dans Scene::from_sheet"))
        {
            Ok(sc) => sc,
            Err(ExportError::EmptySheet(_)) => continue,
            Err(e) => panic!("{label} : scène refusée : {e}"),
        };
        assert_eq!(scene.buttons.len(), p.sheet_buttons(s.id).len(), "{label}");
        if let Ok(svg) = export_svg(&scene) {
            assert_eq!(
                svg.matches("data-form=\"bouton\"").count()
                    + svg.matches("data-form=\"boutonniere\"").count(),
                scene.buttons.len(),
                "{label}"
            );
            assert_eq!(
                svg.matches("data-role=\"bouton\"").count(),
                usize::from(!scene.buttons.is_empty()),
                "{label}"
            );
        }
    }
    check_export(p, label);
    st.exports += 1;
}

#[test]
fn us064_campagne_aleatoire_des_boutons_graine_fixe() {
    let mut st = ButtonStats::default();
    for seq in 0..170u64 {
        let mut r = Rng(0x06_0400 + seq);
        let mut project = Project::new("Boutons aléa");
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
        // Quelques groupes au départ : de la matière pour les autres commandes.
        for _ in 0..r.below(5) {
            let pts: Vec<PointId> = project
                .sheet(sheet)
                .unwrap()
                .points
                .iter()
                .map(|p| p.id)
                .collect();
            let _ = project.apply(Command::AddButtonGroup {
                piece: base,
                from: pick(&mut r, &pts, PointId(BOGUS)),
                to: pick(&mut r, &pts, PointId(BOGUS)),
                count: bt_count(&mut r),
                element: bt_element(&mut r),
                edge_margin_mm: bt_margin(&mut r),
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
            if let Some(cmd) = bt_command(&mut r, &cur, sheet, op) {
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
                        st.added += out.button_groups.len();
                        for id in &out.button_groups {
                            assert!(
                                after
                                    .pieces
                                    .iter()
                                    .any(|p| p.buttons.iter().any(|g| g.id == *id)),
                                "groupe annoncé introuvable : {label}"
                            );
                        }
                        st.pruned += out
                            .notes
                            .iter()
                            .filter(|n| n.contains("groupe de boutons") && n.contains("retiré"))
                            .count();
                        st.own_pruned += out
                            .notes
                            .iter()
                            .filter(|n| n.contains("groupe de boutons") && n.contains("recréé"))
                            .count();
                        st.split_lost += out
                            .notes
                            .iter()
                            .filter(|n| {
                                n.contains("groupe(s) de boutons") && n.contains("ligne de coupe")
                            })
                            .count();
                        st.unfolds_noted += out
                            .notes
                            .iter()
                            .filter(|n| n.contains("groupe(s) de boutons") && n.contains("dépliée"))
                            .count();
                        let carried = |after: &Project| {
                            out.pieces
                                .iter()
                                .filter(|id| after.piece(**id).is_ok_and(|p| !p.buttons.is_empty()))
                                .count()
                        };
                        match name.as_str() {
                            "SplitPiece" => st.splits_carried += carried(after),
                            "JoinPieces" => st.joins += carried(after),
                            "DuplicateSheet" => st.copies += carried(after),
                            _ => {}
                        }
                        st.derived_with_groups += out
                            .regenerated
                            .iter()
                            .filter(|id| after.piece(**id).is_ok_and(|p| !p.buttons.is_empty()))
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
            check_buttons_state(&doc.project().clone(), &label, &mut st);
            if step % 4 == 3 {
                check_buttons_export(doc.project(), &label, &mut st);
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
        "boutons : {} modifications, {} refus, {} sans changement, {} groupes créés, {} retraits \
         par suppression de point ({} par régénération), {} reprises par séparation ({} groupes \
         non repris), {} assemblages, {} par copie de feuille, {} dépliages avec remarque, {} \
         dérivées régénérées avec groupes, {} annulations, {} relectures, {} exports contrôlés ; \
         vus : {} groupes de boutons, {} de boutonnières, {} éléments ({} trop près du bord, {} \
         sans jugement, {} recul recalculé indépendamment)",
        st.accepted,
        st.refused,
        st.noops,
        st.added,
        st.pruned,
        st.own_pruned,
        st.splits_carried,
        st.split_lost,
        st.joins,
        st.copies,
        st.unfolds_noted,
        st.derived_with_groups,
        st.undone,
        st.reloaded,
        st.exports,
        st.buttons,
        st.holes,
        st.elements,
        st.too_close,
        st.unjudged,
        st.oracle_checked
    );
    assert!(
        st.accepted >= 700,
        "seulement {} modifications",
        st.accepted
    );
    assert!(st.refused >= 400, "seulement {} refus", st.refused);
    assert!(st.added >= 300, "seulement {} groupes créés", st.added);
    assert!(
        st.buttons >= 300 && st.holes >= 300,
        "formes insuffisamment couvertes"
    );
    assert!(st.pruned >= 15, "seulement {} retraits", st.pruned);
    assert!(
        st.splits_carried >= 5,
        "séparations : {}",
        st.splits_carried
    );
    assert!(st.copies >= 10, "copies : {}", st.copies);
    assert!(
        st.too_close >= 100,
        "seulement {} éléments trop près",
        st.too_close
    );
    assert!(
        st.oracle_checked >= 1000,
        "seulement {} reculs recalculés",
        st.oracle_checked
    );
    assert!(
        st.undone >= 100 && st.reloaded >= 80 && st.exports >= 1000,
        "couverture insuffisante"
    );
}
