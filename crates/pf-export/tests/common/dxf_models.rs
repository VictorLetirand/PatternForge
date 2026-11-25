//! Projets de test de l'export DXF (US-12.1) : un projet riche et déterministe, et un
//! générateur aléatoire à graine pour la campagne adverse. Tout passe par `Project::apply`.
#![allow(dead_code)]

use super::{pt, Rng};
use pf_core::{
    ButtonElement, ButtonholeOrientation, Command, DerivedKind, DerivedOp, EntityId, GrainSense,
    InternalKind, InternalTarget, MarkingAnchor, MarkingShape, NotchStyle, PathRole, PieceGrain,
    PieceId, PieceSource, Placement, PointId, PointRef, Project, SheetId,
};
use pf_geometry::Side;

pub fn point_at(p: &Project, s: SheetId, x: f64, y: f64) -> PointId {
    let sheet = p.sheet(s).unwrap();
    sheet
        .points
        .iter()
        .find(|q| {
            let pos = sheet.position(q.id).unwrap();
            (pos.x - x).abs() < 1e-9 && (pos.y - y).abs() < 1e-9
        })
        .unwrap_or_else(|| panic!("aucun point en ({x} ; {y})"))
        .id
}

pub fn add_point(p: &mut Project, s: SheetId, x: f64, y: f64) -> PointId {
    p.apply(Command::AddPoint {
        sheet: s,
        at: pt(x, y),
    })
    .unwrap()
    .points[0]
}

pub fn seg(p: &mut Project, s: SheetId, a: (f64, f64), b: (f64, f64), role: PathRole) -> EntityId {
    let e = p
        .apply(Command::AddSegment {
            sheet: s,
            from: PointRef::New(pt(a.0, a.1)),
            to: PointRef::New(pt(b.0, b.1)),
        })
        .unwrap()
        .entities[0];
    set_role(p, s, e, role);
    e
}

pub fn set_role(p: &mut Project, s: SheetId, e: EntityId, role: PathRole) {
    p.apply(Command::SetPathRole {
        sheet: s,
        entity: e,
        role,
    })
    .unwrap();
}

/// Rectangle `w × h` en `(x ; y)`, extrait en pièce ; tous les tracés ont le rôle donné.
pub fn rect_piece(
    p: &mut Project,
    s: SheetId,
    (x, y): (f64, f64),
    (w, h): (f64, f64),
    name: &str,
    role: PathRole,
    detached: bool,
) -> (PieceId, Vec<EntityId>) {
    let r = p
        .apply(Command::AddRectangle {
            sheet: s,
            corner: PointRef::New(pt(x, y)),
            width_mm: w,
            height_mm: h,
        })
        .unwrap();
    for &e in &r.entities {
        set_role(p, s, e, role);
    }
    let piece = p
        .apply(Command::ExtractPiece {
            sheet: s,
            source: PieceSource::Paths {
                entities: r.entities.clone(),
            },
            name: name.into(),
            detached,
        })
        .unwrap()
        .pieces[0];
    (piece, r.entities)
}

/// Pièce à quatre côtés : segment, courbe de Bézier, courbe lisse, arc. `None` si la
/// construction est refusée (générateur aléatoire).
#[allow(clippy::too_many_arguments)]
pub fn curved_piece(
    p: &mut Project,
    s: SheetId,
    (x, y): (f64, f64),
    (w, h): (f64, f64),
    bulge: f64,
    name: &str,
    role: PathRole,
    detached: bool,
) -> Option<(PieceId, Vec<EntityId>)> {
    let (a, b, c, d) = ((x, y), (x + w, y), (x + w, y + h), (x, y + h));
    let e1 = p
        .apply(Command::AddSegment {
            sheet: s,
            from: PointRef::New(pt(a.0, a.1)),
            to: PointRef::New(pt(b.0, b.1)),
        })
        .ok()?
        .entities[0];
    let pb = point_at(p, s, b.0, b.1);
    let e2 = p
        .apply(Command::AddCurve {
            sheet: s,
            start: PointRef::Existing(pb),
            c1: pt(b.0 + bulge, b.1 + h * 0.3),
            c2: pt(c.0 + bulge * 0.5, c.1 - h * 0.3),
            end: PointRef::New(pt(c.0, c.1)),
        })
        .ok()?
        .entities[0];
    let pc = point_at(p, s, c.0, c.1);
    let e3 = p
        .apply(Command::AddSpline {
            sheet: s,
            points: vec![
                PointRef::Existing(pc),
                PointRef::New(pt(x + w * 0.55, c.1 + bulge * 0.8)),
                PointRef::New(pt(x + w * 0.2, c.1 + bulge * 0.2)),
                PointRef::New(pt(d.0, d.1)),
            ],
        })
        .ok()?
        .entities[0];
    let pd = point_at(p, s, d.0, d.1);
    let pa = point_at(p, s, a.0, a.1);
    let e4 = p
        .apply(Command::AddArcThreePoints {
            sheet: s,
            start: PointRef::Existing(pd),
            through: PointRef::New(pt(x - bulge.abs() - 5.0, y + h * 0.5)),
            end: PointRef::Existing(pa),
        })
        .ok()?
        .entities[0];
    let es = vec![e1, e2, e3, e4];
    for &e in &es {
        set_role(p, s, e, role);
    }
    let piece = p
        .apply(Command::ExtractPiece {
            sheet: s,
            source: PieceSource::Paths {
                entities: es.clone(),
            },
            name: name.into(),
            detached,
        })
        .ok()?
        .pieces[0];
    Some((piece, es))
}

/// Droit-fil (segment de rôle droit-fil) assigné à la pièce, avec un sens de tissu.
pub fn add_grain(
    p: &mut Project,
    s: SheetId,
    piece: PieceId,
    a: (f64, f64),
    b: (f64, f64),
    sense: GrainSense,
) -> EntityId {
    let main = seg(p, s, a, b, PathRole::Grainline);
    let outline = p.piece(piece).unwrap().outline.clone();
    p.apply(Command::SetPieceOutline {
        piece,
        outline,
        grainline: Some(main),
    })
    .unwrap();
    if sense != GrainSense::Unmarked {
        p.apply(Command::SetPieceGrain {
            piece,
            grain: PieceGrain {
                sense,
                ..PieceGrain::default()
            },
        })
        .unwrap();
    }
    main
}

pub fn add_notch(
    p: &mut Project,
    s: SheetId,
    entity: EntityId,
    frac: f64,
    side: Side,
    style: NotchStyle,
) -> bool {
    let len = p.sheet(s).unwrap().path_length_mm(entity).unwrap();
    p.apply(Command::AddNotch {
        sheet: s,
        entity,
        distance_mm: len * frac,
        side,
        length_mm: 5.0,
        style,
    })
    .is_ok()
}

/// Projet riche et déterministe : une pièce rectangulaire de coupe avec droit-fil, crans,
/// marquages, boutons, boutonnière, éléments internes, une pièce à courbes, une pièce
/// détachée, une dérivée (décalage), une dépliée, une pièce-étiquette.
pub struct Rich {
    pub project: Project,
    pub front: PieceId,
    pub curved: PieceId,
    pub detached: PieceId,
    pub derived: PieceId,
    pub unfolded: PieceId,
    pub label: PieceId,
}

pub fn rich_project() -> Rich {
    let mut p = Project::new("Export DXF");
    let s = p.sheets[0].id;
    let (front, front_edges) = rect_piece(
        &mut p,
        s,
        (0.0, 0.0),
        (200.0, 120.0),
        "Devant é",
        PathRole::CuttingContour,
        false,
    );
    p.apply(Command::SetPieceCard {
        piece: front,
        name: "Devant é".into(),
        code: Some("D-01".into()),
        quantity: 2,
        mirrored_pair: true,
        material: Some("Coton".into()),
        category: None,
        comment: None,
    })
    .unwrap();
    add_grain(
        &mut p,
        s,
        front,
        (100.0, 20.0),
        (100.0, 100.0),
        GrainSense::Nap,
    );
    add_notch(
        &mut p,
        s,
        front_edges[0],
        0.3,
        Side::Left,
        NotchStyle::Single,
    );
    add_notch(
        &mut p,
        s,
        front_edges[1],
        0.5,
        Side::Left,
        NotchStyle::Double,
    );
    // Marquages : perçage Ø 6, pointage, croix.
    let a = add_point(&mut p, s, 60.0, 60.0);
    let b = add_point(&mut p, s, 40.0, 40.0);
    let c = add_point(&mut p, s, 150.0, 70.0);
    for (at, shape) in [
        (a, MarkingShape::Drill { diameter_mm: 6.0 }),
        (b, MarkingShape::Punch),
        (c, MarkingShape::Cross { size_mm: 8.0 }),
    ] {
        p.apply(Command::AddPieceMarking {
            piece: front,
            at: MarkingAnchor::Point { point: at },
            shape,
        })
        .unwrap();
    }
    // Boutons Ø 12 (3) et boutonnières 24 × 4 (2).
    let (b1, b2) = (
        add_point(&mut p, s, 30.0, 100.0),
        add_point(&mut p, s, 170.0, 100.0),
    );
    p.apply(Command::AddButtonGroup {
        piece: front,
        from: b1,
        to: b2,
        count: 3,
        element: ButtonElement::Button { diameter_mm: 12.0 },
        edge_margin_mm: 0.0,
    })
    .unwrap();
    let (h1, h2) = (
        add_point(&mut p, s, 30.0, 20.0),
        add_point(&mut p, s, 60.0, 20.0),
    );
    p.apply(Command::AddButtonGroup {
        piece: front,
        from: h1,
        to: h2,
        count: 2,
        element: ButtonElement::Buttonhole {
            length_mm: 24.0,
            width_mm: 4.0,
            orientation: ButtonholeOrientation::Horizontal {},
        },
        edge_margin_mm: 0.0,
    })
    .unwrap();
    // Éléments internes : un pli (ligne), un repère (point), un cercle interne.
    let fold = seg(
        &mut p,
        s,
        (20.0, 80.0),
        (180.0, 80.0),
        PathRole::Construction,
    );
    let circle = p
        .apply(Command::AddCircle {
            sheet: s,
            center: PointRef::New(pt(120.0, 40.0)),
            radius_mm: 7.0,
        })
        .unwrap()
        .entities[0];
    for (target, kind) in [
        (InternalTarget::Path { entity: fold }, InternalKind::Fold),
        (InternalTarget::Path { entity: circle }, InternalKind::Other),
        (InternalTarget::Point { point: a }, InternalKind::Mark),
    ] {
        p.apply(Command::AddPieceInternal {
            piece: front,
            target,
            kind,
        })
        .unwrap();
    }
    // Pièce à courbes (liée, couture).
    let (curved, curved_edges) = curved_piece(
        &mut p,
        s,
        (300.0, 0.0),
        (150.0, 100.0),
        25.0,
        "Courbe",
        PathRole::SewingContour,
        false,
    )
    .unwrap();
    add_notch(
        &mut p,
        s,
        curved_edges[1],
        0.5,
        Side::Right,
        NotchStyle::Single,
    );
    add_notch(
        &mut p,
        s,
        curved_edges[2],
        0.4,
        Side::Left,
        NotchStyle::Single,
    );
    add_notch(
        &mut p,
        s,
        curved_edges[3],
        0.5,
        Side::Left,
        NotchStyle::Double,
    );
    // Pièce détachée.
    let (detached, _) = rect_piece(
        &mut p,
        s,
        (600.0, 0.0),
        (80.0, 50.0),
        "Détachée",
        PathRole::Construction,
        true,
    );
    // Dérivée : doublure décalée de −5 mm de la pièce de devant.
    let derived = p
        .apply(Command::CreateDerivedPiece {
            mother: front,
            kind: DerivedKind::Lining,
            operation: DerivedOp::Offset { distance_mm: -5.0 },
            name: None,
        })
        .unwrap()
        .pieces[0];
    // Dépliée : demi-pièce dont le bord gauche est la pliure.
    let (half, half_edges) = rect_piece(
        &mut p,
        s,
        (0.0, 300.0),
        (90.0, 140.0),
        "Moitié",
        PathRole::CuttingContour,
        false,
    );
    let sheet = p.sheet(s).unwrap();
    let fold_edge = *half_edges
        .iter()
        .find(|&&e| {
            let (u, v) = sheet.entity_points(e).unwrap();
            let (u, v) = (sheet.position(u).unwrap(), sheet.position(v).unwrap());
            u.x == 0.0 && v.x == 0.0
        })
        .unwrap();
    let unfolded = p
        .apply(Command::UnfoldPiece {
            piece: half,
            fold: fold_edge,
            name: None,
        })
        .unwrap()
        .pieces[0];
    // Pièce-étiquette sans contour.
    let label = p
        .apply(Command::AddPiece {
            sheet: s,
            name: "Étiquette".into(),
            quantity: 1,
            material: None,
            mirrored_pair: false,
        })
        .unwrap()
        .pieces[0];
    Rich {
        project: p,
        front,
        curved,
        detached,
        derived,
        unfolded,
        label,
    }
}

/// Projet aléatoire : 1 à 5 pièces variées (rectangles, pièces à courbes, détachées, dérivées,
/// dépliées, étiquettes), droit-fil, crans, marquages, boutons, éléments internes et
/// placements manuels tirés au hasard. Les commandes refusées sont ignorées (le projet reste
/// valide, c'est la règle de `apply`).
pub fn random_project(rng: &mut Rng) -> Project {
    const NAMES: [&str; 6] = [
        "Devant",
        "Dos é",
        "Manche ÿ ü",
        "Col \\ x",
        "Poche « 1 »",
        "Ceinture",
    ];
    let mut p = Project::new("Aléatoire");
    let s = p.sheets[0].id;
    let n = 1 + rng.below(5) as usize;
    let mut y = 0.0;
    for i in 0..n {
        let name = NAMES[rng.below(NAMES.len() as u64) as usize];
        let role = rng.pick(&[
            PathRole::CuttingContour,
            PathRole::CuttingContour,
            PathRole::SewingContour,
            PathRole::Construction,
        ]);
        let (w, h) = (rng.range(40.0, 400.0), rng.range(30.0, 300.0));
        let x = rng.range(-50.0, 50.0);
        let detached = rng.below(5) == 0;
        let kind = rng.below(10);
        let made = match kind {
            0..=2 => curved_piece(
                &mut p,
                s,
                (x, y),
                (w, h),
                rng.range(-30.0, 30.0),
                name,
                role,
                detached,
            ),
            3 => {
                // Pièce-étiquette ou pièce à contour ouvert (refusée par la validation ou non).
                let l = p
                    .apply(Command::AddPiece {
                        sheet: s,
                        name: name.into(),
                        quantity: 1,
                        material: None,
                        mirrored_pair: false,
                    })
                    .unwrap()
                    .pieces[0];
                if rng.below(2) == 0 {
                    let e = seg(&mut p, s, (x, y), (x + w, y + h), PathRole::Construction);
                    let _ = p.apply(Command::SetPieceOutline {
                        piece: l,
                        outline: vec![pf_core::PieceEdge {
                            entity: e,
                            reversed: false,
                        }],
                        grainline: None,
                    });
                }
                None
            }
            _ => Some(rect_piece(&mut p, s, (x, y), (w, h), name, role, detached)),
        };
        y -= 450.0;
        let Some((piece, _)) = made else { continue };
        let edges: Vec<EntityId> = p
            .piece(piece)
            .unwrap()
            .outline
            .iter()
            .map(|e| e.entity)
            .collect();
        let _ = i;
        let owner = if detached {
            p.piece_sheet(piece).unwrap().id
        } else {
            s
        };
        if rng.below(3) != 0 {
            let _ = p.apply(Command::SetPieceCard {
                piece,
                name: name.into(),
                code: (rng.below(2) == 0).then(|| format!("C-{}", rng.below(99))),
                quantity: 1 + rng.below(4) as u32,
                mirrored_pair: rng.below(2) == 0,
                material: (rng.below(2) == 0).then(|| "Lin".to_string()),
                category: None,
                comment: None,
            });
        }
        if rng.below(4) != 0 {
            let sense = rng.pick(&[GrainSense::Unmarked, GrainSense::Nap, GrainSense::Pattern]);
            add_grain(
                &mut p,
                owner,
                piece,
                (x + w * 0.5, y + 450.0 + h * 0.2),
                (x + w * 0.5, y + 450.0 + h * 0.8),
                sense,
            );
        }
        for &e in &edges {
            if rng.below(3) == 0 {
                let side = rng.pick(&[Side::Left, Side::Right]);
                let style = rng.pick(&[NotchStyle::Single, NotchStyle::Double]);
                add_notch(&mut p, owner, e, rng.range(0.1, 0.9), side, style);
            }
        }
        let y0 = y + 450.0;
        if rng.below(2) == 0 {
            let q1 = add_point(&mut p, owner, x + w * 0.3, y0 + h * 0.4);
            let q2 = add_point(&mut p, owner, x + w * 0.7, y0 + h * 0.6);
            for (at, shape) in [
                (
                    q1,
                    MarkingShape::Drill {
                        diameter_mm: rng.range(2.0, 12.0),
                    },
                ),
                (q2, MarkingShape::Punch),
                (
                    q2,
                    MarkingShape::Cross {
                        size_mm: rng.range(3.0, 12.0),
                    },
                ),
            ] {
                let _ = p.apply(Command::AddPieceMarking {
                    piece,
                    at: MarkingAnchor::Point { point: at },
                    shape,
                });
            }
        }
        if rng.below(2) == 0 {
            let q1 = add_point(&mut p, owner, x + w * 0.2, y0 + h * 0.8);
            let q2 = add_point(&mut p, owner, x + w * 0.8, y0 + h * 0.8);
            let element = if rng.below(2) == 0 {
                ButtonElement::Button {
                    diameter_mm: rng.range(5.0, 20.0),
                }
            } else {
                ButtonElement::Buttonhole {
                    length_mm: rng.range(10.0, 30.0),
                    width_mm: rng.range(2.0, 6.0),
                    orientation: rng.pick(&[
                        ButtonholeOrientation::Horizontal {},
                        ButtonholeOrientation::Vertical {},
                        ButtonholeOrientation::Perpendicular {},
                        ButtonholeOrientation::Angle { degrees: 37.0 },
                    ]),
                }
            };
            let _ = p.apply(Command::AddButtonGroup {
                piece,
                from: q1,
                to: q2,
                count: 1 + rng.below(4) as u32,
                element,
                edge_margin_mm: 0.0,
            });
        }
        if rng.below(3) == 0 {
            let l = seg(
                &mut p,
                owner,
                (x + w * 0.2, y0 + h * 0.5),
                (x + w * 0.8, y0 + h * 0.55),
                PathRole::Construction,
            );
            let _ = p.apply(Command::AddPieceInternal {
                piece,
                target: InternalTarget::Path { entity: l },
                kind: InternalKind::Fold,
            });
        }
        if rng.below(4) == 0 {
            let _ = p.apply(Command::CreateDerivedPiece {
                mother: piece,
                kind: DerivedKind::Lining,
                operation: DerivedOp::Offset {
                    distance_mm: -rng.range(1.0, 8.0),
                },
                name: None,
            });
        }
        if rng.below(3) == 0 {
            let _ = p.apply(Command::SetPiecePlacement {
                piece,
                placement: Some(Placement {
                    x_mm: rng.range(-500.0, 500.0),
                    y_mm: rng.range(-500.0, 500.0),
                    rotation_deg: rng.pick(&[0.0, 90.0, 180.0, 270.0, 33.3, 211.0]),
                }),
            });
        }
    }
    p
}
