//! US-12.7 — format ouvert : schéma JSON `docs/pforge-v6.schema.json`, validateur en ligne de
//! commande `pf-validate` et cohérence schéma ↔ chargeur.
//!
//! Le validateur de schéma utilisé ici est un **mini-validateur écrit pour les tests**
//! (`format_ouvert/validateur.rs`, indépendant de `pf_core`) : Python `jsonschema` n'est pas
//! installé sur la machine de développement et aucun paquet n'a été téléchargé. Le schéma n'a donc
//! jamais été confronté à une bibliothèque tierce (limite documentée).

#[path = "../../pf-export/tests/common/dxf_models.rs"]
mod dxf_models;
#[path = "format_ouvert/validateur.rs"]
mod validateur;

use pf_core::{
    persist, Command, DivisionSpec, FullDimensionKind, GrainDirection, GrainSense, MirrorAxis,
    NamingCounter, NamingRuleSpec, NamingTarget, NotchAnchor, NotchForm, NotchStyle, PathRole,
    PieceGrain, Placement, PointDefinition, PointId, PointRef, Project, SegmentEnd, TopstitchSide,
    TopstitchSource, SCHEMA_VERSION,
};
use pf_geometry::{PointMm, Side};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command as Proc;

// Requis par `dxf_models` (`use super::{pt, Rng}`).
pub fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

/// Générateur xorshift64* à graine fixe (même suite que celui des tests d'export).
pub struct Rng(pub u64);

impl Rng {
    pub fn step(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    pub fn unit(&mut self) -> f64 {
        (self.step() >> 11) as f64 / (1u64 << 53) as f64
    }
    pub fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.unit()
    }
    pub fn pick<T: Copy>(&mut self, xs: &[T]) -> T {
        xs[(self.step() % xs.len() as u64) as usize]
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.step() % n
    }
    pub fn pick_clone<T: Clone>(&mut self, xs: &[T]) -> T {
        xs[(self.step() % xs.len() as u64) as usize].clone()
    }
}

use dxf_models::{add_grain, add_point, rect_piece, rich_project, seg};

const SCHEMA_TEXT: &str = include_str!("../../../docs/pforge-v6.schema.json");
const SPEC_TEXT: &str = include_str!("../../../docs/FORMAT_PFORGE_V6.md");

fn schema() -> Value {
    serde_json::from_str(SCHEMA_TEXT).expect("le schéma est du JSON")
}

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn tmp() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("format_ouvert");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

// ---------------------------------------------------------------------------
// Corpus
// ---------------------------------------------------------------------------

/// Projet avec ce que `rich_project` n'a pas : calques, groupes, lignes d'aide, cotations,
/// cotes complètes, surpiqûre liée, crans ancrés, points construits, droit-fil avancé, placement.
fn extra_project() -> Project {
    let mut p = Project::new("Extras é");
    let s = p.sheets[0].id;
    let layer = p
        .apply(Command::AddLayer {
            sheet: s,
            name: "Calque A".into(),
        })
        .unwrap()
        .layers[0];
    let hidden = p
        .apply(Command::AddLayer {
            sheet: s,
            name: "Calque B".into(),
        })
        .unwrap()
        .layers[0];
    p.apply(Command::SetLayerVisible {
        sheet: s,
        layer: hidden,
        visible: false,
    })
    .unwrap();
    let guide = p
        .apply(Command::AddGuide {
            sheet: s,
            x_mm: 50.0,
            y_mm: 0.0,
            angle_deg: 90.0,
            axis: true,
            layer: None,
        })
        .unwrap()
        .guides[0];
    let (piece, edges) = rect_piece(
        &mut p,
        s,
        (0.0, 0.0),
        (200.0, 120.0),
        "Corps",
        PathRole::CuttingContour,
        false,
    );
    p.apply(Command::AddTopstitch {
        sheet: s,
        source: TopstitchSource::Piece { piece },
        distance_mm: 4.0,
        side: TopstitchSide::Inside,
    })
    .unwrap();
    let a = add_point(&mut p, s, 10.0, 10.0);
    let b = add_point(&mut p, s, 90.0, 40.0);
    for anchor in [
        NotchAnchor::Midpoint {},
        NotchAnchor::Ratio { ratio: 0.25 },
        NotchAnchor::Point { point: a },
    ] {
        p.apply(Command::AddAnchoredNotch {
            sheet: s,
            entity: edges[0],
            anchor,
            side: Side::Left,
            length_mm: 5.0,
            style: NotchStyle::Single,
        })
        .unwrap();
    }
    // Cran en U, large de 4 mm, projeté sur la coupe (US-06.1).
    let shaped = p
        .apply(Command::AddNotch {
            sheet: s,
            entity: edges[1],
            distance_mm: 20.0,
            side: Side::Right,
            length_mm: 6.0,
            style: NotchStyle::Double,
        })
        .unwrap()
        .notches[0];
    p.apply(Command::SetNotchShape {
        sheet: s,
        notch: shaped,
        form: NotchForm::U,
        width_mm: Some(4.0),
        on_cut: true,
    })
    .unwrap();
    // Cran en coin au sommet du contour de la pièce (US-06.1b).
    let corner_notch = p
        .apply(Command::AddNotch {
            sheet: s,
            entity: edges[0],
            distance_mm: 0.0,
            side: Side::Right,
            length_mm: 5.0,
            style: NotchStyle::Single,
        })
        .unwrap()
        .notches[0];
    p.apply(Command::SetNotchCorner {
        sheet: s,
        notch: corner_notch,
        corner: true,
    })
    .unwrap();
    let derived = [
        PointDefinition::Midpoint {
            first: a,
            second: b,
        },
        PointDefinition::Offset {
            from: a,
            dx_mm: 5.0,
            dy_mm: -3.0,
        },
        PointDefinition::OnSegmentRatio {
            segment: edges[0],
            ratio: 0.3,
        },
        PointDefinition::OnSegment {
            segment: edges[0],
            distance_mm: 20.0,
            origin: SegmentEnd::End,
        },
        PointDefinition::Polar {
            from: a,
            distance_mm: 30.0,
            angle_deg: 15.0,
            reference: Some(edges[0]),
        },
    ];
    for def in derived {
        p.apply(Command::AddPointAt {
            sheet: s,
            point: PointRef::Derived(def),
        })
        .unwrap();
    }
    p.apply(Command::DivideTrace {
        sheet: s,
        trace: edges[1],
        spec: DivisionSpec::Equal { parts: 3 },
        start: None,
    })
    .unwrap();
    p.apply(Command::LinkMirror {
        sheet: s,
        points: vec![a],
        entities: vec![],
        axis: MirrorAxis::Guide { guide },
    })
    .unwrap();
    p.apply(Command::AddDimension {
        sheet: s,
        from: a,
        to: b,
        offset_mm: 10.0,
    })
    .unwrap();
    let circle = p
        .apply(Command::AddCircle {
            sheet: s,
            center: PointRef::New(pt(150.0, 60.0)),
            radius_mm: 12.0,
        })
        .unwrap()
        .entities[0];
    for kind in [
        FullDimensionKind::PathLength {
            entity: edges[0],
            offset_mm: 8.0,
        },
        FullDimensionKind::Horizontal {
            from: a,
            to: b,
            offset_mm: -6.0,
        },
        FullDimensionKind::Radius {
            entity: circle,
            angle_deg: 30.0,
        },
    ] {
        p.apply(Command::AddFullDimension {
            sheet: s,
            kind,
            label: Some("Cote é".into()),
            text_shift_mm: 2.0,
        })
        .unwrap();
    }
    let free = |p: &mut Project, x: f64, y: f64| -> PointId { add_point(p, s, x, y) };
    let (q1, q2, q3) = (
        free(&mut p, 300.0, 0.0),
        free(&mut p, 340.0, 30.0),
        free(&mut p, 380.0, 0.0),
    );
    p.apply(Command::AddArcThreePoints {
        sheet: s,
        start: PointRef::Existing(q1),
        through: PointRef::Existing(q2),
        end: PointRef::Existing(q3),
    })
    .unwrap();
    p.apply(Command::AddCurve {
        sheet: s,
        start: PointRef::Existing(q1),
        c1: pt(310.0, 60.0),
        c2: pt(360.0, 60.0),
        end: PointRef::Existing(q3),
    })
    .unwrap();
    let spline = p
        .apply(Command::AddSpline {
            sheet: s,
            points: vec![
                PointRef::New(pt(300.0, 100.0)),
                PointRef::New(pt(330.0, 130.0)),
                PointRef::New(pt(380.0, 110.0)),
            ],
        })
        .unwrap()
        .entities[0];
    p.apply(Command::SetSplineNode {
        sheet: s,
        spline,
        index: 1,
        corner: true,
        tangent: Some(pf_core::OffsetMm { dx: 20.0, dy: 5.0 }),
        tangent_in: Some(pf_core::OffsetMm { dx: 10.0, dy: -5.0 }),
    })
    .unwrap();
    let _main = add_grain(
        &mut p,
        s,
        piece,
        (100.0, 20.0),
        (100.0, 100.0),
        GrainSense::Pattern,
    );
    let second = seg(&mut p, s, (40.0, 20.0), (40.0, 90.0), PathRole::Grainline);
    p.apply(Command::SetPieceGrain {
        piece,
        grain: PieceGrain {
            direction: GrainDirection::Bias { angle_deg: 45.0 },
            sense: GrainSense::Nap,
            reversed: true,
            rotation_tolerance_deg: Some(5.0),
            extra: vec![pf_core::ExtraGrainLine {
                entity: second,
                direction: GrainDirection::Parallel {
                    reference: edges[1],
                },
                sense: GrainSense::Unmarked,
                reversed: false,
            }],
        },
    })
    .unwrap();
    p.apply(Command::SetPiecePlacement {
        piece,
        placement: Some(Placement {
            x_mm: 100.0,
            y_mm: -60.0,
            rotation_deg: 90.0,
        }),
    })
    .unwrap();
    // Marge de couture (US-05.1) : défaut non nul (une surcharge retirée laisse une marge valide),
    // une marge progressive et une constante, sur des segments du contour.
    let outline: Vec<_> = p.piece(piece).unwrap().outline.clone();
    p.apply(Command::SetSeamAllowance {
        piece,
        seam: Some(pf_core::SeamAllowance {
            default_mm: 8.0,
            edges: vec![
                pf_core::EdgeAllowance {
                    entity: outline[0].entity,
                    start_mm: 2.0,
                    end_mm: 6.0,
                },
                pf_core::EdgeAllowance {
                    entity: outline[2].entity,
                    start_mm: 4.0,
                    end_mm: 4.0,
                },
            ],
            // Types de coins (US-05.2) : un type avec paramètre, un sans, et la longueur maximale.
            corners: vec![
                pf_core::CornerAllowance {
                    entity: outline[1].entity,
                    corner: pf_core::CornerType::Truncated { depth_mm: 1.5 },
                },
                pf_core::CornerAllowance {
                    entity: outline[2].entity,
                    corner: pf_core::CornerType::Stepped {},
                },
            ],
            max_corner_mm: Some(30.0),
            // Ourlet et rempli sur des bords droits (US-05.3).
            hems: vec![pf_core::HemAllowance {
                entity: outline[3].entity,
                height_mm: 25.0,
            }],
            folds: vec![pf_core::FoldAllowance {
                entity: outline[1].entity,
                value_mm: 6.0,
                notch_step_mm: None,
                notch_count: None,
            }],
        }),
    })
    .unwrap();
    // Quartier de disque : rempli sur l'arc, avec un pas puis un nombre de crans (US-05.3).
    let q = |p: &mut Project, a: (f64, f64), b: (f64, f64)| {
        p.apply(Command::AddSegment {
            sheet: s,
            from: PointRef::New(pt(a.0, a.1)),
            to: PointRef::New(pt(b.0, b.1)),
        })
        .unwrap()
        .entities[0]
    };
    let qb = q(&mut p, (500.0, 0.0), (600.0, 0.0));
    let ql = q(&mut p, (500.0, 100.0), (500.0, 0.0));
    let at = |p: &Project, x: f64, y: f64| -> PointId {
        let sh = p.sheet(s).unwrap();
        sh.points
            .iter()
            .find(|k| pf_geometry::distance_mm(sh.position(k.id).unwrap(), pt(x, y)) < 1e-9)
            .unwrap()
            .id
    };
    let (qa, qc) = (at(&p, 600.0, 0.0), at(&p, 500.0, 100.0));
    let qarc = p
        .apply(Command::AddArcThreePoints {
            sheet: s,
            start: PointRef::Existing(qa),
            through: PointRef::New(pt(500.0 + 100.0 * 0.5f64.sqrt(), 100.0 * 0.5f64.sqrt())),
            end: PointRef::Existing(qc),
        })
        .unwrap()
        .entities[0];
    let quarter = p
        .apply(Command::ExtractPiece {
            sheet: s,
            source: pf_core::PieceSource::Paths {
                entities: vec![qb, qarc, ql],
            },
            name: "Quartier".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    p.apply(Command::SetSeamAllowance {
        piece: quarter,
        seam: Some(pf_core::SeamAllowance {
            default_mm: 5.0,
            folds: vec![pf_core::FoldAllowance {
                entity: qarc,
                value_mm: 12.0,
                notch_step_mm: Some(30.0),
                notch_count: None,
            }],
            ..Default::default()
        }),
    })
    .unwrap();
    let round = p
        .apply(Command::AddCircle {
            sheet: s,
            center: PointRef::New(pt(700.0, 0.0)),
            radius_mm: 20.0,
        })
        .unwrap()
        .entities[0];
    let round_piece = p
        .apply(Command::ExtractPiece {
            sheet: s,
            source: pf_core::PieceSource::Paths {
                entities: vec![round],
            },
            name: "Rond".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    p.apply(Command::SetSeamAllowance {
        piece: round_piece,
        seam: Some(pf_core::SeamAllowance {
            folds: vec![pf_core::FoldAllowance {
                entity: round,
                value_mm: 3.0,
                notch_step_mm: None,
                notch_count: Some(5),
            }],
            ..Default::default()
        }),
    })
    .unwrap();
    // Pièce « coupe nette » (US-05.6).
    let franc = p
        .apply(Command::AddCircle {
            sheet: s,
            center: PointRef::New(pt(800.0, 0.0)),
            radius_mm: 15.0,
        })
        .unwrap()
        .entities[0];
    let franc_piece = p
        .apply(Command::ExtractPiece {
            sheet: s,
            source: pf_core::PieceSource::Paths {
                entities: vec![franc],
            },
            name: "Bord franc".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    p.apply(Command::SetNetCut {
        piece: franc_piece,
        net_cut: true,
    })
    .unwrap();
    // Marges par matière (US-05.5).
    p.apply(Command::SetSeamDefaults {
        defaults: Some(pf_core::SeamDefaults {
            fabric_mm: 12.0,
            ..pf_core::SeamDefaults::builtin()
        }),
    })
    .unwrap();
    // Pince (US-07.1) sur un rectangle 200 × 150 : sommet (1000 ; 100), axe vers (1000 ; 0).
    p.apply(Command::AddRectangle {
        sheet: s,
        corner: PointRef::New(pt(900.0, 0.0)),
        width_mm: 200.0,
        height_mm: 150.0,
    })
    .unwrap();
    let dart_piece = p
        .apply(Command::ExtractPiece {
            sheet: s,
            source: pf_core::PieceSource::Click {
                at: pt(1000.0, 75.0),
            },
            name: "Pincé".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    let dart_apex = p
        .apply(Command::AddPoint {
            sheet: s,
            at: pt(1000.0, 100.0),
        })
        .unwrap()
        .points[0];
    let dart_axis = p
        .apply(Command::AddPoint {
            sheet: s,
            at: pt(1000.0, 0.0),
        })
        .unwrap()
        .points[0];
    let dart = p
        .apply(Command::AddPieceDart {
            piece: dart_piece,
            apex: PointRef::Existing(dart_apex),
            toward: PointRef::Existing(dart_axis),
            length_mm: 100.0,
            opening_mm: 20.0,
            fixed: Side::Right,
        })
        .unwrap()
        .darts[0];
    // Coupe « pince fermée » (PH-5a) : le champ optionnel `cut_closed` est dans le corpus.
    p.apply(Command::SetPieceDartCutClosed {
        piece: dart_piece,
        dart,
        closed: true,
    })
    .unwrap();
    // Bord sur le pli (PH-5b) : le champ optionnel `on_fold` est dans le corpus.
    let fold_edge = p.piece(dart_piece).unwrap().outline[0].entity;
    p.apply(Command::SetPieceFold {
        piece: dart_piece,
        edge: fold_edge,
        on: true,
    })
    .unwrap();
    // Pièce sans droit-fil (PH-5d) : le champ optionnel `no_grain` est dans le corpus.
    p.apply(Command::SetPieceNoGrain {
        piece: dart_piece,
        no_grain: true,
    })
    .unwrap();
    // Pli (US-07.3) sur un rectangle 200 × 100 : pli plat de profondeur 10 à x = 1500.
    p.apply(Command::AddRectangle {
        sheet: s,
        corner: PointRef::New(pt(1400.0, 0.0)),
        width_mm: 200.0,
        height_mm: 100.0,
    })
    .unwrap();
    let pleat_piece = p
        .apply(Command::ExtractPiece {
            sheet: s,
            source: pf_core::PieceSource::Click {
                at: pt(1450.0, 50.0),
            },
            name: "Plissé".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    p.apply(Command::AddPiecePleats {
        piece: pleat_piece,
        from: pt(1500.0, 0.0),
        to: pt(1500.0, 100.0),
        depth_mm: 10.0,
        kind: pf_core::PleatKind::Flat,
        sense: Side::Right,
        count: 1,
        span_mm: 0.0,
    })
    .unwrap();
    // Ampleur (US-07.4) sur un rectangle 200 × 100 : bord fixe = le bas, deux lignes ouvertes de 20 mm.
    p.apply(Command::AddRectangle {
        sheet: s,
        corner: PointRef::New(pt(1700.0, 0.0)),
        width_mm: 200.0,
        height_mm: 100.0,
    })
    .unwrap();
    let flare_piece = p
        .apply(Command::ExtractPiece {
            sheet: s,
            source: pf_core::PieceSource::Click {
                at: pt(1750.0, 50.0),
            },
            name: "Évasée".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    p.apply(Command::AddPieceFlare {
        piece: flare_piece,
        fixed_from: pt(1700.0, 0.0),
        fixed_to: pt(1900.0, 0.0),
        lines: vec![
            pf_core::FlareLineSpec {
                at_mm: 70.0,
                opening_mm: 20.0,
            },
            pf_core::FlareLineSpec {
                at_mm: 140.0,
                opening_mm: 20.0,
            },
        ],
    })
    .unwrap();
    // Découpe (US-07.5) d'un rectangle 200 × 100 par une ligne oblique : deux pièces détachées dont
    // l'arête de découpe de la seconde est retournée, quatre crans de repère. Aucun type nouveau.
    p.apply(Command::AddRectangle {
        sheet: s,
        corner: PointRef::New(pt(2000.0, 0.0)),
        width_mm: 200.0,
        height_mm: 100.0,
    })
    .unwrap();
    let cut_piece = p
        .apply(Command::ExtractPiece {
            sheet: s,
            source: pf_core::PieceSource::Click {
                at: pt(2050.0, 50.0),
            },
            name: "Découpée".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    p.apply(Command::CutPiece {
        piece: cut_piece,
        line: pf_core::CutLine::Points {
            a: pt(2040.0, -20.0),
            b: pt(2140.0, 120.0),
        },
        notches: pf_core::CutNotches::Count { count: 2 },
    })
    .unwrap();
    // Couture appariée (US-08.1) : un bord de la pièce pincée avec deux bords consécutifs de la
    // pièce plissée, embu et tolérance déclarés.
    let seam_a = p
        .seam_side_range(
            dart_piece,
            p.piece(dart_piece).unwrap().outline[0].entity,
            1,
        )
        .unwrap();
    let seam_b = p
        .seam_side_range(
            pleat_piece,
            p.piece(pleat_piece).unwrap().outline[0].entity,
            2,
        )
        .unwrap();
    p.apply(Command::AddSeamPair {
        a: seam_a,
        b: seam_b,
        embu_mm: Some(3.0),
        tolerance_mm: Some(1.5),
    })
    .unwrap();
    // Perçages au pas (PH-4b) : fentes de poinçon (variante `slot`) le long d'un bord du corps.
    p.apply(Command::AddPieceMarkingRun {
        piece,
        run: pf_core::MarkingRunSpec {
            edges: vec![edges[1]],
            spacing: pf_core::RunSpacing::Count { count: 3 },
            start_margin_mm: 5.0,
            end_margin_mm: 5.0,
            distance_mm: 4.0,
            side: pf_core::RunSide::Inside,
            form: pf_core::RunForm::Slot {
                length_mm: 6.0,
                width_mm: 1.5,
            },
        },
    })
    .unwrap();
    p.apply(Command::CreateGroup {
        sheet: s,
        entities: vec![edges[2], edges[3]],
    })
    .unwrap();
    p.apply(Command::SetEntityName {
        sheet: s,
        entity: edges[0],
        name: Some("Bord haut".into()),
    })
    .unwrap();
    p.apply(Command::SetObjectsLayer {
        sheet: s,
        points: vec![b],
        entities: vec![],
        layer: Some(layer),
    })
    .unwrap();
    // Table de variables (US-03.1a) : unités mm / deg / none, une référence à une autre variable.
    for (name, unit, expr) in [
        ("poitrine", pf_core::VariableUnit::Mm, "920"),
        ("aisance", pf_core::VariableUnit::Mm, "40"),
        ("largeur", pf_core::VariableUnit::Mm, "(poitrine+aisance)/4"),
        ("angle", pf_core::VariableUnit::Deg, "min(12,5;pi)"),
        (
            "rapport",
            pf_core::VariableUnit::Unitless,
            "largeur/poitrine",
        ),
    ] {
        p.apply(Command::AddVariable {
            name: name.into(),
            unit,
            expr: expr.into(),
        })
        .unwrap();
    }
    // Valeurs maintenues par une formule (US-03.1b) : dx du point décalé (40 / 8 = 5), distance
    // (40 - 10 = 30) et angle (240 / 16 = 15) du point en direction ; les valeurs écrites ne
    // changent pas, seul le lien est ajouté.
    let (offset, polar) = {
        let sheet = p.sheet(s).unwrap();
        let find = |want: fn(&PointDefinition) -> bool| {
            sheet
                .points
                .iter()
                .find(|q| want(&q.definition))
                .map(|q| q.id)
                .unwrap()
        };
        (
            find(|d| matches!(d, PointDefinition::Offset { .. })),
            find(|d| matches!(d, PointDefinition::Polar { .. })),
        )
    };
    for (point, param, expr) in [
        (offset, pf_core::ParamKind::Dx, "aisance/8"),
        (polar, pf_core::ParamKind::Distance, "aisance-10"),
        (polar, pf_core::ParamKind::Angle, "largeur/16"),
    ] {
        p.apply(Command::SetParameterExpr {
            target: pf_core::ParamTarget {
                sheet: s,
                point,
                param,
            },
            expr: Some(expr.into()),
        })
        .unwrap();
    }
    // Constructions maintenues (US-03.2) : longueur / angle, perpendiculaire, parallèle ; un projet
    // sans lien s'écrit sans champ `kept`.
    let origin = p
        .apply(Command::AddPoint {
            sheet: s,
            at: pf_geometry::PointMm::new(2000.0, 2000.0),
        })
        .unwrap()
        .points[0];
    let polar = p
        .apply(Command::AddSegmentPolar {
            sheet: s,
            from: PointRef::Existing(origin),
            length_mm: 100.0,
            angle_deg: 30.0,
        })
        .unwrap();
    let perp = p
        .apply(Command::AddPerpendicular {
            sheet: s,
            reference: polar.entities[0],
            from: PointRef::Existing(origin),
            length_mm: 40.0,
            side: pf_geometry::Side::Left,
        })
        .unwrap();
    let par = p
        .apply(Command::AddParallel {
            sheet: s,
            reference: polar.entities[0],
            distance_mm: 25.0,
            side: pf_geometry::Side::Right,
        })
        .unwrap();
    for segment in [polar.entities[0], perp.entities[0], par.entities[0]] {
        p.apply(Command::SetConstructionKept {
            sheet: s,
            segment,
            kept: true,
        })
        .unwrap();
    }
    // Longueurs imposées (US-03.3) : une extrémité glissante, une poignée ajustée ; un projet sans
    // contrainte s'écrit sans clé `length_constraints`.
    let mut first_curve = None;
    for (y, slide) in [(3000.0, true), (3200.0, false)] {
        let curve = p
            .apply(Command::AddCurve {
                sheet: s,
                start: PointRef::New(pf_geometry::PointMm::new(3000.0, y)),
                c1: pf_geometry::PointMm::new(3040.0, y + 60.0),
                c2: pf_geometry::PointMm::new(3160.0, y + 60.0),
                end: PointRef::New(pf_geometry::PointMm::new(3200.0, y)),
            })
            .unwrap();
        let (start, end, id) = (curve.points[0], curve.points[1], curve.entities[0]);
        let natural = p.sheet(s).unwrap().curve_developed_mm(id).unwrap();
        first_curve.get_or_insert(id);
        p.apply(Command::SetCurveLengthConstraint {
            sheet: s,
            curve: id,
            spec: Some(pf_core::LengthSpec {
                length_mm: natural + 25.0,
                adjust: if slide {
                    pf_core::LengthAdjust::Slide {
                        point: end,
                        angle_deg: 15.0,
                    }
                } else {
                    pf_core::LengthAdjust::Handle { point: start }
                },
                reference: None,
            }),
        })
        .unwrap();
    }
    // Égalité de longueurs (US-03.4) : une courbe égale à la première + 12 mm (clé `reference`).
    let curve = p
        .apply(Command::AddCurve {
            sheet: s,
            start: PointRef::New(pf_geometry::PointMm::new(3000.0, 3400.0)),
            c1: pf_geometry::PointMm::new(3040.0, 3460.0),
            c2: pf_geometry::PointMm::new(3160.0, 3460.0),
            end: PointRef::New(pf_geometry::PointMm::new(3200.0, 3400.0)),
        })
        .unwrap();
    p.apply(Command::SetCurveLengthConstraint {
        sheet: s,
        curve: curve.entities[0],
        spec: Some(pf_core::LengthSpec {
            length_mm: 1.0,
            adjust: pf_core::LengthAdjust::Slide {
                point: curve.points[1],
                angle_deg: 0.0,
            },
            reference: Some(pf_core::LengthRef {
                entity: first_curve.expect("première courbe"),
                offset_mm: 12.0,
            }),
        }),
    })
    .unwrap();
    // Courbe lisse à longueur imposée par une formule (US-03.3b) : « largeur » = (920 + 40) / 4 = 240.
    let smooth = p
        .apply(Command::AddSpline {
            sheet: s,
            points: vec![
                PointRef::New(pf_geometry::PointMm::new(3300.0, 3400.0)),
                PointRef::New(pf_geometry::PointMm::new(3350.0, 3440.0)),
                PointRef::New(pf_geometry::PointMm::new(3500.0, 3400.0)),
            ],
        })
        .unwrap();
    let smooth_len = p
        .sheet(s)
        .unwrap()
        .curve_developed_mm(smooth.entities[0])
        .unwrap();
    p.apply(Command::SetCurveLengthConstraint {
        sheet: s,
        curve: smooth.entities[0],
        spec: Some(pf_core::LengthSpec {
            length_mm: smooth_len,
            adjust: pf_core::LengthAdjust::Slide {
                point: smooth.points[2],
                angle_deg: 0.0,
            },
            reference: None,
        }),
    })
    .unwrap();
    p.apply(Command::SetCurveLengthExpr {
        sheet: s,
        curve: smooth.entities[0],
        expr: Some("largeur".into()),
    })
    .unwrap();
    // Gammes de tailles (US-09.1) : une alphanumérique avec rupture, une numérique.
    p.apply(Command::AddSizeRange {
        name: "Femme".into(),
        kind: pf_core::SizeKind::Alphanumeric,
        labels: ["XS", "S", "M", "L", "XL"].map(String::from).to_vec(),
        base: 2,
    })
    .unwrap();
    let rupture = p.sizes[0].sizes[1].id;
    p.apply(Command::SetSizeBreak {
        size: rupture,
        break_size: true,
    })
    .unwrap();
    p.apply(Command::AddSizeRange {
        name: "Tour".into(),
        kind: pf_core::SizeKind::Numeric,
        labels: ["34", "36", "38"].map(String::from).to_vec(),
        base: 0,
    })
    .unwrap();
    // Gradation (US-09.2a) : écarts d'un point libre pour deux tailles de « Femme » et une de « Tour ».
    let free = {
        let sheet = p.sheet(s).unwrap();
        sheet
            .points
            .iter()
            .find(|q| {
                matches!(q.definition, pf_core::PointDefinition::Free { .. })
                    && sheet.grading_follower(q.id).is_none()
                    && !sheet.point_locked(q.id)
            })
            .expect("un point libre non suivi")
            .id
    };
    for (size, dx, dy) in [
        (p.sizes[0].sizes[1].id, -5.0, -10.0),
        (p.sizes[0].sizes[3].id, 5.0, 10.0),
        (p.sizes[1].sizes[2].id, 0.0, 8.5),
    ] {
        p.apply(Command::SetPointGrading {
            sheet: s,
            point: free,
            size,
            dx_mm: dx,
            dy_mm: dy,
        })
        .unwrap();
    }
    p
}

fn as_value(p: &Project) -> Value {
    serde_json::from_str(&persist::to_json(p).unwrap()).unwrap()
}

/// Corpus : (nom, fichier relu comme `Value`). Les fichiers de `examples/` et des fixtures sont
/// lus tels quels (v3) ou réenregistrés en v3 (fixtures v1 / v2) ; le reste vient de commandes.
fn corpus() -> Vec<(String, Value)> {
    let mut out = Vec::new();
    let mut names: Vec<_> = std::fs::read_dir(repo().join("examples"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.to_string_lossy().ends_with(".pforge.json"))
        .collect();
    names.sort();
    for path in names {
        let text = std::fs::read_to_string(&path).unwrap();
        let name = format!("examples/{}", path.file_name().unwrap().to_string_lossy());
        out.push((name, serde_json::from_str(&text).unwrap()));
    }
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut fx: Vec<_> = std::fs::read_dir(fixtures)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    fx.sort();
    for path in fx {
        let project = persist::load(&path).unwrap();
        let name = format!(
            "fixture migrée {}",
            path.file_name().unwrap().to_string_lossy()
        );
        out.push((name, as_value(&project)));
    }
    out.push((
        "demo_project".into(),
        as_value(&pf_core::demo_project().unwrap()),
    ));
    out.push((
        "demo_piece_project".into(),
        as_value(&pf_core::demo_piece_project().unwrap()),
    ));
    out.push(("rich_project".into(), as_value(&rich_project().project)));
    out.push(("extra_project".into(), as_value(&extra_project())));
    out.push(("v5_project".into(), as_value(&v5_project())));
    for seed in 1..=6u64 {
        let p = dxf_models::random_project(&mut Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15)));
        out.push((format!("random_project #{seed}"), as_value(&p)));
    }
    out
}

fn generated(corpus: &[(String, Value)]) -> usize {
    corpus
        .iter()
        .filter(|(n, _)| !n.starts_with("examples/") && !n.starts_with("fixture"))
        .count()
}

// ---------------------------------------------------------------------------
// Verdicts
// ---------------------------------------------------------------------------

fn schema_ok(schema: &Value, doc: &Value) -> bool {
    validateur::validate(schema, doc).is_empty()
}

fn loader_ok(doc: &Value) -> bool {
    persist::from_json(&doc.to_string()).is_ok()
}

struct Run {
    code: i32,
    stdout: String,
    stderr: String,
}

fn cli(args: &[&str]) -> Run {
    let out = Proc::new(env!("CARGO_BIN_EXE_pf-validate"))
        .args(args)
        .output()
        .expect("lancement de pf-validate");
    Run {
        code: out.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

fn write_tmp(name: &str, text: &str) -> String {
    let path = tmp().join(name);
    std::fs::write(&path, text).unwrap();
    path.to_string_lossy().into_owned()
}

// ---------------------------------------------------------------------------
// CA1 / CA2 — le schéma accepte le corpus et suit le code
// ---------------------------------------------------------------------------

#[test]
fn ca2_exemples_et_corpus_valident_contre_le_schema() {
    let schema = schema();
    let corpus = corpus();
    assert!(
        corpus
            .iter()
            .filter(|(n, _)| n.starts_with("examples/"))
            .count()
            >= 3,
        "les trois fichiers de examples/"
    );
    assert!(
        generated(&corpus) >= 10,
        "au moins 10 projets produits par les tests"
    );
    for (name, doc) in &corpus {
        let errors = validateur::validate(&schema, doc);
        assert!(errors.is_empty(), "{name} : le schéma refuse : {errors:?}");
        assert!(loader_ok(doc), "{name} : le chargeur refuse");
    }
}

/// Le corpus couvre ce que la spécification décrit (sinon « valide » ne prouverait rien).
#[test]
fn ca2_le_corpus_couvre_les_objets_du_format() {
    fn collect(v: &Value, keys: &mut std::collections::BTreeSet<String>) {
        match v {
            Value::Object(o) => {
                for (k, x) in o {
                    keys.insert(k.clone());
                    if let (true, Some(t)) = (k == "type" || k == "mode", x.as_str()) {
                        keys.insert(format!("{k}={t}"));
                    }
                    collect(x, keys);
                }
            }
            Value::Array(a) => a.iter().for_each(|x| collect(x, keys)),
            _ => {}
        }
    }
    let mut keys = std::collections::BTreeSet::new();
    for (_, doc) in corpus() {
        collect(&doc, &mut keys);
    }
    for expected in [
        "pieces",
        "notches",
        "dimensions",
        "layers",
        "groups",
        "guides",
        "topstitches",
        "full_dimensions",
        "outline",
        "grainline",
        "grain",
        "seam",
        "net_cut",
        "on_fold",
        "no_grain",
        "seam_defaults",
        "fabric_mm",
        "reinforcement_mm",
        "default_mm",
        "edges",
        "corners",
        "max_corner_mm",
        "hems",
        "height_mm",
        "folds",
        "value_mm",
        "notch_step_mm",
        "notch_count",
        "form",
        "on_cut",
        "corner",
        "type=truncated",
        "type=stepped",
        "internal",
        "markings",
        "darts",
        "seams",
        "variables",
        "expr",
        "unit",
        "formulas",
        "kept",
        "length_constraints",
        "sizes",
        "gradings",
        "grading_rules",
        "measurements",
        "clients",
        "alterations",
        "finished_measures",
        "per_size",
        "proportional",
        "tolerance_minus",
        "type=seam",
        "type=full_dimension",
        "mode=mirror_x",
        "break_size",
        "adjust",
        "type=slide",
        "type=handle",
        "param",
        "embu_mm",
        "tolerance_mm",
        "pleats",
        "flares",
        "hinge_x_mm",
        "depth_mm",
        "opening_mm",
        "fixed",
        "buttons",
        "derived",
        "placement",
        "anchor",
        "migration_notes",
        "construction",
        "tangent",
        "type=free",
        "type=midpoint",
        "type=offset",
        "type=polar",
        "type=on_segment",
        "type=on_segment_ratio",
        "type=division",
        "type=mirror",
        "type=segment",
        "type=curve",
        "type=circle",
        "type=arc",
        "type=spline",
        "type=punch",
        "type=drill",
        "type=cross",
        "type=slot",
        "type=button",
        "type=buttonhole",
        "type=linked",
        "type=detached",
        "type=offset",
        "type=unfold",
        "type=path_length",
        "type=horizontal",
        "type=radius",
        "type=ratio",
        "type=point",
        "type=piece",
        "mode=bias",
        "mode=parallel",
    ] {
        assert!(keys.contains(expected), "le corpus n'a pas « {expected} »");
    }
}

#[test]
fn ca2_mutations_ciblees_refusees_par_schema_et_chargeur() {
    let schema = schema();
    let base = as_value(&rich_project().project);
    assert!(schema_ok(&schema, &base) && loader_ok(&base));
    type Mutator = Box<dyn Fn(&mut Value)>;
    let cases: [(&str, Mutator); 6] = [
        (
            "champ inconnu",
            Box::new(|d| d["sheets"][0]["points"][0]["zz"] = json!(1)),
        ),
        (
            "type faux",
            Box::new(|d| d["sheets"][0]["points"][0]["label"] = json!(5)),
        ),
        (
            "énumération inconnue",
            Box::new(|d| d["sheets"][0]["entities"][0]["role"] = json!("inconnu")),
        ),
        (
            "champ requis absent",
            Box::new(|d| {
                d.as_object_mut().unwrap().remove("next_id");
            }),
        ),
        ("version 7", Box::new(|d| d["schema_version"] = json!(7))),
        (
            "variante inconnue",
            Box::new(|d| d["sheets"][0]["points"][0]["definition"]["type"] = json!("inconnue")),
        ),
    ];
    for (name, mutate) in cases {
        let mut doc = base.clone();
        mutate(&mut doc);
        assert!(!schema_ok(&schema, &doc), "{name} : le schéma accepte");
        assert!(!loader_ok(&doc), "{name} : le chargeur accepte");
        let file = write_tmp("cible.pforge.json", &doc.to_string());
        assert_eq!(cli(&[&file]).code, 1, "{name} : pf-validate");
    }
}

/// Les listes d'énumérations du schéma suivent les types du code (garde-fou contre la dérive).
#[test]
fn ca2_les_enumerations_du_schema_suivent_le_code() {
    let schema = schema();
    let listed = |name: &str| -> Vec<String> {
        let mut v: Vec<String> = schema["$defs"][name]["enum"]
            .as_array()
            .unwrap_or_else(|| panic!("{name} n'est pas une énumération"))
            .iter()
            .map(|x| x.as_str().unwrap().to_string())
            .collect();
        v.sort();
        v
    };
    let mut roles: Vec<String> = PathRole::ALL
        .iter()
        .map(|r| {
            serde_json::to_value(r)
                .unwrap()
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect();
    roles.sort();
    assert_eq!(listed("path_role"), roles);
    let mut cats: Vec<String> = pf_core::PieceCategory::ALL
        .iter()
        .map(|c| c.key().into())
        .collect();
    cats.sort();
    let piece_cat = schema["$defs"]["piece"]["properties"]["category"]["enum"].clone();
    let mut got: Vec<String> = piece_cat
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_str().unwrap().into())
        .collect();
    got.sort();
    assert_eq!(got, cats);
    let mut kinds: Vec<String> = pf_core::InternalKind::LINES
        .iter()
        .chain(pf_core::InternalKind::MARKS.iter())
        .map(|k| k.key().to_string())
        .collect();
    kinds.sort();
    kinds.dedup();
    let mut got: Vec<String> = schema["$defs"]["internal_element"]["properties"]["kind"]["enum"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_str().unwrap().into())
        .collect();
    got.sort();
    assert_eq!(got, kinds);
    assert_eq!(
        schema["properties"]["schema_version"]["const"],
        json!(SCHEMA_VERSION)
    );
    assert_eq!(
        schema["properties"]["format"]["const"],
        json!(pf_core::FORMAT_TAG)
    );
}

// ---------------------------------------------------------------------------
// CA3 — pf-validate
// ---------------------------------------------------------------------------

#[test]
fn ca3_fichier_valide_code_0_et_fichier_intact() {
    let file = repo().join("examples/piece-crans-droit-fil.pforge.json");
    let file = file.to_string_lossy().into_owned();
    let before = std::fs::read(&file).unwrap();
    let run = cli(&[&file]);
    assert_eq!(run.code, 0, "{}{}", run.stdout, run.stderr);
    assert!(run.stdout.contains("valide"), "{}", run.stdout);
    assert_eq!(
        std::fs::read(&file).unwrap(),
        before,
        "le fichier a été modifié"
    );
    // Aucun fichier voisin créé.
    let dir = repo().join("examples");
    let leftovers = std::fs::read_dir(dir)
        .unwrap()
        .filter(|e| {
            e.as_ref()
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains(".tmp")
        })
        .count();
    assert_eq!(leftovers, 0);
}

#[test]
fn us032_construction_maintenue_pf_validate_0_valide_1_position_alteree() {
    // Valide : le corpus contient trois constructions maintenues.
    let good = as_value(&extra_project());
    assert!(good.to_string().contains("\"kept\":true"));
    let ok = cli(&[&write_tmp("maintenu-ok.pforge.json", &good.to_string())]);
    assert_eq!(ok.code, 0, "{}", ok.stdout);
    // Une extrémité écrite 5 mm trop loin : refus localisé (code 1), schéma content.
    let mut doc = good.clone();
    let id = doc["sheets"][0]["entities"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["kind"]["construction"]["kept"] == json!(true))
        .map(|e| e["kind"]["to"].clone())
        .unwrap();
    let pt = doc["sheets"][0]["points"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|q| q["id"] == id)
        .unwrap();
    let x = pt["definition"]["x"].as_f64().unwrap();
    pt["definition"]["x"] = json!(x + 5.0);
    assert!(validateur::validate(&schema(), &doc).is_empty());
    let bad = cli(&[&write_tmp("maintenu-ko.pforge.json", &doc.to_string())]);
    assert_eq!(bad.code, 1, "{}", bad.stdout);
    assert!(
        bad.stdout.contains("construction maintenue"),
        "{}",
        bad.stdout
    );
}

#[test]
fn us033_longueur_imposee_pf_validate_0_valide_1_longueur_alteree() {
    let good = as_value(&extra_project());
    assert!(good.to_string().contains("\"length_constraints\""));
    let ok = cli(&[&write_tmp("imposee-ok.pforge.json", &good.to_string())]);
    assert_eq!(ok.code, 0, "{}", ok.stdout);
    // Longueur imposée écrite 1 mm plus loin que la géométrie : refus localisé (code 1).
    let mut doc = good.clone();
    let c = &mut doc["sheets"][0]["length_constraints"][0]["length_mm"];
    let v = c.as_f64().unwrap();
    *c = json!(v + 1.0);
    assert!(validateur::validate(&schema(), &doc).is_empty());
    let bad = cli(&[&write_tmp("imposee-ko.pforge.json", &doc.to_string())]);
    assert_eq!(bad.code, 1, "{}", bad.stdout);
    assert!(bad.stdout.contains("longueur imposée"), "{}", bad.stdout);
    // Type d'inconnue inconnu : refusé par le schéma comme par le chargeur.
    let mut doc = good;
    doc["sheets"][0]["length_constraints"][0]["adjust"]["type"] = json!("autre");
    assert!(!validateur::validate(&schema(), &doc).is_empty());
    assert_eq!(
        cli(&[&write_tmp("imposee-type.pforge.json", &doc.to_string())]).code,
        1
    );
}

#[test]
fn us033b_cible_par_formule_pf_validate_0_valide_1_formule_alteree() {
    let good = as_value(&extra_project());
    let text = good.to_string();
    assert!(
        text.contains("\"expr\":\"largeur\""),
        "formule de longueur absente"
    );
    let ok = cli(&[&write_tmp("formule-longueur-ok.pforge.json", &text)]);
    assert_eq!(ok.code, 0, "{}", ok.stdout);
    let k = |doc: &Value| -> usize {
        doc["sheets"][0]["length_constraints"]
            .as_array()
            .unwrap()
            .iter()
            .position(|c| c.get("expr").is_some())
            .unwrap()
    };
    // Formule changée sans recalcul (« largeur » = 240, « poitrine » = 920) : code 1, schéma muet.
    let mut doc = good.clone();
    let i = k(&doc);
    doc["sheets"][0]["length_constraints"][i]["expr"] = json!("poitrine");
    assert!(validateur::validate(&schema(), &doc).is_empty());
    let bad = cli(&[&write_tmp(
        "formule-longueur-ko.pforge.json",
        &doc.to_string(),
    )]);
    assert_eq!(bad.code, 1, "{}", bad.stdout);
    assert!(bad.stdout.contains("formule"), "{}", bad.stdout);
    // Variable inconnue : refus du chargeur. Texte vide ou non texte : refus du schéma et du chargeur.
    let mut doc = good.clone();
    doc["sheets"][0]["length_constraints"][i]["expr"] = json!("absente");
    assert_eq!(
        cli(&[&write_tmp(
            "formule-longueur-inconnue.pforge.json",
            &doc.to_string()
        )])
        .code,
        1
    );
    for bad_value in [json!(""), json!(12)] {
        let mut doc = good.clone();
        doc["sheets"][0]["length_constraints"][i]["expr"] = bad_value;
        assert!(!validateur::validate(&schema(), &doc).is_empty());
        assert_eq!(
            cli(&[&write_tmp(
                "formule-longueur-type.pforge.json",
                &doc.to_string()
            )])
            .code,
            1
        );
    }
}

#[test]
fn us034_egalite_de_longueurs_pf_validate_0_valide_1_cible_alteree() {
    let good = as_value(&extra_project());
    let text = good.to_string();
    assert!(text.contains("\"reference\"") && text.contains("\"offset_mm\":12.0"));
    let ok = cli(&[&write_tmp("egalite-ok.pforge.json", &text)]);
    assert_eq!(ok.code, 0, "{}", ok.stdout);
    let k = |doc: &mut Value| -> usize {
        doc["sheets"][0]["length_constraints"]
            .as_array()
            .unwrap()
            .iter()
            .position(|c| c.get("reference").is_some())
            .unwrap()
    };
    // Décalage modifié sans recalcul : la cible écrite ne correspond plus (code 1, schéma muet).
    let mut doc = good.clone();
    let i = k(&mut doc);
    doc["sheets"][0]["length_constraints"][i]["reference"]["offset_mm"] = json!(13.0);
    assert!(validateur::validate(&schema(), &doc).is_empty());
    let bad = cli(&[&write_tmp("egalite-ko.pforge.json", &doc.to_string())]);
    assert_eq!(bad.code, 1, "{}", bad.stdout);
    assert!(bad.stdout.contains("longueur imposée"), "{}", bad.stdout);
    // Référence inconnue : refus du chargeur ; champ inconnu : refus du schéma comme du chargeur.
    let mut doc = good.clone();
    doc["sheets"][0]["length_constraints"][i]["reference"]["entity"] = json!(999_999);
    assert_eq!(
        cli(&[&write_tmp("egalite-inconnue.pforge.json", &doc.to_string())]).code,
        1
    );
    let mut doc = good;
    doc["sheets"][0]["length_constraints"][i]["reference"]["extra"] = json!(1);
    assert!(!validateur::validate(&schema(), &doc).is_empty());
    assert_eq!(
        cli(&[&write_tmp("egalite-extra.pforge.json", &doc.to_string())]).code,
        1
    );
}

#[test]
fn ca3_fichier_invalide_code_1_avec_localisation() {
    // JSON syntaxiquement faux : ligne et colonne de l'analyseur.
    let broken = write_tmp(
        "json-casse.pforge.json",
        "{\n  \"format\": \"patternforge\",\n  oups\n}",
    );
    let run = cli(&[&broken]);
    assert_eq!(run.code, 1, "{}", run.stdout);
    assert!(
        run.stdout.contains("[json]") && run.stdout.contains("ligne 3"),
        "{}",
        run.stdout
    );
    let machine = cli(&[&broken, "--json"]);
    let v: Value = serde_json::from_str(&machine.stdout).unwrap();
    assert_eq!(v["valid"], json!(false));
    assert_eq!(v["errors"][0]["kind"], json!("json"));
    assert_eq!(v["errors"][0]["line"], json!(3));

    // Pas un projet PatternForge.
    let other = write_tmp("autre.pforge.json", "{\"hello\": 1}");
    let run = cli(&[&other]);
    assert_eq!(run.code, 1);
    assert!(run.stdout.contains("[format]"), "{}", run.stdout);

    // Version plus récente.
    let mut doc = as_value(&pf_core::demo_piece_project().unwrap());
    doc["schema_version"] = json!(7);
    let newer = write_tmp("v7.pforge.json", &doc.to_string());
    let run = cli(&[&newer]);
    assert_eq!(run.code, 1);
    assert!(
        run.stdout.contains("[version]") && run.stdout.contains("version 7"),
        "{}",
        run.stdout
    );

    // Champ inconnu : type de l'erreur et nom du champ.
    let mut doc = as_value(&pf_core::demo_piece_project().unwrap());
    doc["sheets"][0]["points"][0]["zz"] = json!(1);
    let unknown = write_tmp("inconnu.pforge.json", &doc.to_string());
    let run = cli(&[&unknown]);
    assert_eq!(run.code, 1);
    assert!(
        run.stdout.contains("[structure]") && run.stdout.contains("zz"),
        "{}",
        run.stdout
    );

    // Référence manquante : message du chargeur, citant l'identifiant.
    let mut doc = as_value(&pf_core::demo_piece_project().unwrap());
    let first_notch = doc["sheets"][0]["notches"][0]["entity"].clone();
    doc["sheets"][0]["notches"][0]["entity"] = json!(first_notch.as_u64().unwrap() + 900);
    let dead = write_tmp("reference.pforge.json", &doc.to_string());
    let run = cli(&[&dead]);
    assert_eq!(run.code, 1);
    assert!(run.stdout.contains("[reference]"), "{}", run.stdout);
    assert!(
        run.stdout.contains("absent") || run.stdout.contains("introuvable"),
        "{}",
        run.stdout
    );
}

#[test]
fn ca3_illisible_ou_usage_code_2() {
    let missing = tmp().join("absent.pforge.json");
    let run = cli(&[&missing.to_string_lossy()]);
    assert_eq!(run.code, 2, "{}", run.stdout);
    assert!(run.stdout.contains("[lecture]"));
    // Octets invalides en UTF-8.
    let path = tmp().join("binaire.pforge.json");
    std::fs::write(&path, [0xff, 0xfe, 0x00, 0x80]).unwrap();
    assert_eq!(cli(&[&path.to_string_lossy()]).code, 2);
    // Usage.
    assert_eq!(cli(&[]).code, 2);
    assert_eq!(cli(&["a.json", "b.json"]).code, 2);
    assert_eq!(cli(&["--inconnue"]).code, 2);
    let help = cli(&["--aide"]);
    assert_eq!(help.code, 0);
    assert!(help.stdout.contains("Codes de sortie"));
    assert!(cli(&[]).stderr.contains("Usage"));
}

#[test]
fn ca3_sortie_json_deterministe_et_ancien_format_relu_sans_modification() {
    let file = repo().join("crates/pf-core/tests/fixtures/v1-exercice.pforge.json");
    let file = file.to_string_lossy().into_owned();
    let before = std::fs::read(&file).unwrap();
    let a = cli(&[&file, "--json"]);
    let b = cli(&[&file, "--json"]);
    assert_eq!(a.code, 0);
    assert_eq!(a.stdout, b.stdout, "sortie --json non déterministe");
    assert_eq!(a.stdout.lines().count(), 1);
    let v: Value = serde_json::from_str(&a.stdout).unwrap();
    assert_eq!(v["valid"], json!(true));
    assert_eq!(v["file_schema_version"], json!(1));
    assert_eq!(v["app_schema_version"], json!(6));
    assert_eq!(v["migrated_in_memory"], json!(true));
    assert!(v["project"]["sheets"].as_u64().unwrap() >= 1);
    // Clés triées (sortie stable pour une machine).
    let keys: Vec<&String> = v.as_object().unwrap().keys().collect();
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(keys, sorted);
    assert_eq!(
        std::fs::read(&file).unwrap(),
        before,
        "le fichier ancien a été modifié"
    );
    let human = cli(&[&file]);
    assert!(human.stdout.contains("version 1") && human.stdout.contains("n'a pas été modifié"));
}

// ---------------------------------------------------------------------------
// CA4 — campagne de mutations : schéma, chargeur et pf-validate d'accord, ou écart connu
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord)]
enum Kind {
    DeleteKey,
    AddKey,
    ChangeType,
    SetVersion,
    SetEnum,
    ChangeValue,
    RemoveElement,
}

const KINDS: [Kind; 7] = [
    Kind::DeleteKey,
    Kind::AddKey,
    Kind::ChangeType,
    Kind::SetVersion,
    Kind::SetEnum,
    Kind::ChangeValue,
    Kind::RemoveElement,
];

struct Mutation {
    kind: Kind,
    /// Pointeur JSON de l'objet touché (ajout de clé), du champ ou de l'élément.
    pointer: String,
    /// Nouvelle valeur (changements) ou valeur de `type` de l'objet (ajout de clé).
    value: Value,
}

impl Mutation {
    fn describe(&self) -> String {
        format!("{:?} {} {}", self.kind, self.pointer, self.value)
    }
    fn last_key(&self) -> &str {
        self.pointer.rsplit('/').next().unwrap_or("")
    }
}

/// Champs `Option<T>` du modèle : le chargeur serde accepte `null` pour eux, le schéma non.
const OPTIONAL_KEYS: [&str; 28] = [
    "width_mm",
    "seam_defaults",
    "layer",
    "name",
    "construction",
    "tangent",
    "tangent_in",
    "code",
    "material",
    "category",
    "comment",
    "grainline",
    "grain",
    "seam",
    "derived",
    "placement",
    "anchor",
    "label",
    "stale",
    "rotation_tolerance_deg",
    "reference",
    "start",
    "from",
    "max_corner_mm",
    "notch_step_mm",
    "notch_count",
    "embu_mm",
    "tolerance_mm",
];

const ENUM_KEYS: [&str; 11] = [
    "format", "role", "side", "style", "origin", "type", "mode", "kind", "sense", "category",
    "form",
];

fn pointers(v: &Value, at: String, out: &mut Vec<String>) {
    match v {
        Value::Object(o) => {
            for (k, x) in o {
                let p = format!("{at}/{k}");
                out.push(p.clone());
                pointers(x, p, out);
            }
        }
        Value::Array(a) => {
            for (i, x) in a.iter().enumerate() {
                let p = format!("{at}/{i}");
                out.push(p.clone());
                pointers(x, p, out);
            }
        }
        _ => {}
    }
}

fn json_type(v: &Value) -> u8 {
    match v {
        Value::Null => 0,
        Value::Bool(_) => 1,
        Value::Number(_) => 2,
        Value::String(_) => 3,
        Value::Array(_) => 4,
        Value::Object(_) => 5,
    }
}

/// Une mutation tirée de `rng` sur `base`, ou `None` si le tirage n'a rien à toucher.
fn mutate(rng: &mut Rng, base: &Value, kind: Kind) -> Option<(Mutation, Value)> {
    let mut doc = base.clone();
    let mut all = Vec::new();
    pointers(base, String::new(), &mut all);
    let pick = |rng: &mut Rng, list: &[String]| -> Option<String> {
        (!list.is_empty()).then(|| list[rng.below(list.len() as u64) as usize].clone())
    };
    let objects = |pred: &dyn Fn(&Value) -> bool| -> Vec<String> {
        let mut v: Vec<String> = all
            .iter()
            .filter(|p| base.pointer(p).is_some_and(pred))
            .cloned()
            .collect();
        if pred(base) {
            v.push(String::new());
        }
        v
    };
    match kind {
        Kind::DeleteKey => {
            let objs = objects(&|v| v.as_object().is_some_and(|o| !o.is_empty()));
            let at = pick(rng, &objs)?;
            let keys: Vec<String> = base.pointer(&at)?.as_object()?.keys().cloned().collect();
            let key = keys[rng.below(keys.len() as u64) as usize].clone();
            doc.pointer_mut(&at)?.as_object_mut()?.remove(&key);
            Some((
                Mutation {
                    kind,
                    pointer: format!("{at}/{key}"),
                    value: Value::Null,
                },
                doc,
            ))
        }
        Kind::AddKey => {
            let objs = objects(&|v| v.is_object());
            let at = pick(rng, &objs)?;
            let tag = base
                .pointer(&at)?
                .get("type")
                .cloned()
                .unwrap_or(Value::Null);
            doc.pointer_mut(&at)?
                .as_object_mut()?
                .insert("zz_inconnu".into(), json!(1));
            Some((
                Mutation {
                    kind,
                    pointer: at,
                    value: tag,
                },
                doc,
            ))
        }
        Kind::ChangeType => {
            let at = pick(rng, &all)?;
            let current = json_type(base.pointer(&at)?);
            let candidates: Vec<Value> = [
                json!(null),
                json!("x"),
                json!(0.5),
                json!(true),
                json!([]),
                json!({}),
            ]
            .into_iter()
            .filter(|c| json_type(c) != current)
            .collect();
            let new = candidates[rng.below(candidates.len() as u64) as usize].clone();
            *doc.pointer_mut(&at)? = new.clone();
            Some((
                Mutation {
                    kind,
                    pointer: at,
                    value: new,
                },
                doc,
            ))
        }
        Kind::SetVersion => {
            let new = rng.pick_clone(&[
                json!(1),
                json!(2),
                json!(3),
                json!(4),
                json!(5),
                json!(7),
                json!(0),
                json!(99),
                json!("3"),
                json!(3.5),
                json!(null),
                json!(-1),
                json!(6.0),
            ]);
            doc["schema_version"] = new.clone();
            Some((
                Mutation {
                    kind,
                    pointer: "/schema_version".into(),
                    value: new,
                },
                doc,
            ))
        }
        Kind::SetEnum => {
            let strings: Vec<String> = all
                .iter()
                .filter(|p| {
                    base.pointer(p).is_some_and(Value::is_string)
                        && ENUM_KEYS.contains(&p.rsplit('/').next().unwrap_or(""))
                })
                .cloned()
                .collect();
            let at = pick(rng, &strings)?;
            *doc.pointer_mut(&at)? = json!("inconnu");
            Some((
                Mutation {
                    kind,
                    pointer: at,
                    value: json!("inconnu"),
                },
                doc,
            ))
        }
        Kind::ChangeValue => {
            let leaves: Vec<String> = all
                .iter()
                .filter(|p| {
                    p.as_str() != "/schema_version"
                        && base
                            .pointer(p)
                            .is_some_and(|v| v.is_number() || v.is_string())
                })
                .cloned()
                .collect();
            let at = pick(rng, &leaves)?;
            let new = if base.pointer(&at)?.is_string() {
                json!("")
            } else {
                rng.pick_clone(&[
                    json!(0),
                    json!(-1),
                    json!(1),
                    json!(2),
                    json!(7),
                    json!(1_000_000),
                ])
            };
            *doc.pointer_mut(&at)? = new.clone();
            Some((
                Mutation {
                    kind,
                    pointer: at,
                    value: new,
                },
                doc,
            ))
        }
        Kind::RemoveElement => {
            let arrays = objects(&|v| v.as_array().is_some_and(|a| !a.is_empty()));
            let at = pick(rng, &arrays)?;
            let len = base.pointer(&at)?.as_array()?.len();
            let index = rng.below(len as u64) as usize;
            doc.pointer_mut(&at)?.as_array_mut()?.remove(index);
            Some((
                Mutation {
                    kind,
                    pointer: format!("{at}/{index}"),
                    value: Value::Null,
                },
                doc,
            ))
        }
    }
}

/// Écarts connus entre schéma et chargeur : liste **fermée**. `None` = écart non expliqué.
fn explain(m: &Mutation, schema_accepts: bool, loader_accepts: bool) -> Option<&'static str> {
    match (schema_accepts, loader_accepts) {
        // Le schéma ne vérifie ni les références, ni les identifiants uniques, ni les bornes
        // métier, ni les noms vides : seul le chargeur le fait.
        (true, false) if matches!(m.kind, Kind::ChangeValue | Kind::RemoveElement) => {
            Some("semantique")
        }
        // Le chargeur (serde) lit `null` comme « absent » pour un champ Option.
        (false, true)
            if m.kind == Kind::ChangeType
                && m.value.is_null()
                && OPTIONAL_KEYS.contains(&m.last_key()) =>
        {
            Some("null_optionnel")
        }
        // Versions 1 à 4 : migrées par le chargeur ; le schéma décrit la version 5 seulement.
        (false, true)
            if m.kind == Kind::SetVersion
                && (m.value == json!(1)
                    || m.value == json!(2)
                    || m.value == json!(3)
                    || m.value == json!(4)
                    || m.value == json!(5)) =>
        {
            Some("ancienne_version")
        }
        // `6.0` : égal à 6 pour JSON Schema, refusé par serde pour un entier.
        (true, false) if m.kind == Kind::SetVersion && m.value == json!(6.0) => {
            Some("entier_en_flottant")
        }
        _ => None,
    }
}

#[derive(Default)]
struct Report {
    total: usize,
    by_kind: BTreeMap<Kind, usize>,
    agree: usize,
    gaps: BTreeMap<&'static str, usize>,
    unexplained: Vec<String>,
    cli_problems: Vec<String>,
}

fn campaign(schema: &Value, count: usize, with_cli: bool) -> Report {
    let corpus = corpus();
    let mut rng = Rng(0x5EED_F0F0_1207_0001);
    let mut report = Report::default();
    let mut i = 0;
    while report.total < count {
        i += 1;
        assert!(i < count * 20, "trop de tirages sans mutation");
        let kind = KINDS[report.total % KINDS.len()];
        let base = &corpus[rng.below(corpus.len() as u64) as usize].1;
        let Some((m, doc)) = mutate(&mut rng, base, kind) else {
            continue;
        };
        report.total += 1;
        *report.by_kind.entry(kind).or_default() += 1;
        let (s, l) = (schema_ok(schema, &doc), loader_ok(&doc));
        if with_cli {
            let file = write_tmp(&format!("m{}.pforge.json", report.total), &doc.to_string());
            let run = cli(&[&file, "--json"]);
            let expected = if l { 0 } else { 1 };
            if run.code != expected {
                report.cli_problems.push(format!(
                    "{} : code {} au lieu de {expected} ({})",
                    m.describe(),
                    run.code,
                    run.stderr.trim()
                ));
            } else if serde_json::from_str::<Value>(&run.stdout)
                .map(|v| v["valid"] != json!(l))
                .unwrap_or(true)
            {
                report
                    .cli_problems
                    .push(format!("{} : sortie --json incohérente", m.describe()));
            }
        }
        if s == l {
            report.agree += 1;
        } else if let Some(class) = explain(&m, s, l) {
            *report.gaps.entry(class).or_default() += 1;
        } else {
            report
                .unexplained
                .push(format!("schéma {s}, chargeur {l} : {}", m.describe()));
        }
    }
    report
}

#[test]
fn ca4_campagne_de_mutations_schema_chargeur_et_cli() {
    let report = campaign(&schema(), 240, true);
    println!(
        "campagne : {} mutations, {} d'accord, écarts expliqués {:?}, par type {:?}",
        report.total, report.agree, report.gaps, report.by_kind
    );
    assert!(report.total >= 150);
    for kind in KINDS {
        assert!(
            report.by_kind.get(&kind).copied().unwrap_or(0) >= 10,
            "{kind:?} trop rare"
        );
    }
    assert!(
        report.cli_problems.is_empty(),
        "pf-validate : {:#?}",
        report.cli_problems
    );
    assert!(
        report.unexplained.is_empty(),
        "écarts schéma / chargeur non listés ({}) : {:#?}",
        report.unexplained.len(),
        report.unexplained
    );
    assert!(
        report.agree * 10 >= report.total * 7,
        "trop d'écarts, même expliqués"
    );
}

/// Contrôle de la campagne : un schéma volontairement cassé doit la faire échouer.
#[test]
fn ca4_la_campagne_detecte_un_schema_casse() {
    // Casse 1 : le schéma accepte un champ inconnu sur les points.
    let mut broken = schema();
    broken["$defs"]["point"]["additionalProperties"] = json!(true);
    let r = campaign(&broken, 240, false);
    assert!(
        !r.unexplained.is_empty(),
        "champ inconnu sur un point non détecté"
    );
    // Casse 2 : les nombres en mm acceptent n'importe quel type.
    let mut broken = schema();
    broken["$defs"]["mm"] = json!({});
    let r = campaign(&broken, 240, false);
    assert!(
        !r.unexplained.is_empty(),
        "type de nombre non vérifié non détecté"
    );
    // Casse 3 : la version n'est plus contrôlée.
    let mut broken = schema();
    broken["properties"]["schema_version"] = json!({ "type": "integer" });
    let r = campaign(&broken, 240, false);
    assert!(
        !r.unexplained.is_empty(),
        "version non contrôlée non détectée"
    );
    // Le schéma intact n'a aucun écart inexpliqué (même campagne, sans processus).
    assert!(campaign(&schema(), 240, false).unexplained.is_empty());
}

#[test]
fn ca4_la_sortie_json_est_deterministe_sur_les_mutations() {
    let corpus = corpus();
    let mut rng = Rng(0xD0D0_CAFE_0000_0007);
    for i in 0..14 {
        let base = &corpus[rng.below(corpus.len() as u64) as usize].1;
        let Some((_, doc)) = mutate(&mut rng, base, KINDS[i % KINDS.len()]) else {
            continue;
        };
        let file = write_tmp(&format!("det{i}.pforge.json"), &doc.to_string());
        let a = cli(&[&file, "--json"]);
        let b = cli(&[&file, "--json"]);
        assert_eq!((a.code, &a.stdout), (b.code, &b.stdout), "mutation {i}");
        assert!(a.code == 0 || a.code == 1);
    }
}

/// Écarts documentés, vérifiés un par un (si l'un disparaît, la spécification est à mettre à jour).
#[test]
fn ca4_ecarts_connus_sont_ceux_de_la_specification() {
    let schema = schema();
    let base = as_value(&rich_project().project);
    let verdicts = |doc: &Value| (schema_ok(&schema, doc), loader_ok(doc));

    // D-04 (corrigé) : clé en trop sur `punch` ou `detached`, refusée par le schéma et le chargeur.
    let mut doc = base.clone();
    let markings = &mut doc["pieces"][0]["markings"];
    let punch = markings
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|m| m["shape"]["type"] == "punch")
        .expect("un pointage dans rich_project");
    punch["shape"]["zz"] = json!(1);
    assert_eq!(verdicts(&doc), (false, false), "D-04");

    let mut doc = base.clone();
    let detached = doc["pieces"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|p| p["link"]["type"] == "detached")
        .expect("une pièce détachée dans rich_project");
    detached["link"]["zz"] = json!(1);
    assert_eq!(verdicts(&doc), (false, false), "D-04 detached");

    // `null` sur un champ optionnel.
    let mut doc = base.clone();
    doc["pieces"][0]["code"] = json!(null);
    assert_eq!(verdicts(&doc), (false, true), "null optionnel");

    // Version 2 (ou 3) : migrée par le chargeur, hors du périmètre du schéma.
    let mut doc = base.clone();
    doc["schema_version"] = json!(2);
    assert_eq!(verdicts(&doc), (false, true), "ancienne version");

    // Entier écrit en flottant : accepté par le schéma (spec JSON Schema), refusé par serde.
    let mut doc = base.clone();
    let text = doc
        .to_string()
        .replacen("\"schema_version\":6", "\"schema_version\":6.0", 1);
    doc = serde_json::from_str(&text).unwrap();
    assert_eq!(verdicts(&doc), (true, false), "6.0");

    // Référence morte : le schéma ne vérifie pas les références.
    let mut doc = base.clone();
    doc["sheets"][0]["notches"][0]["entity"] = json!(999_999);
    assert_eq!(verdicts(&doc), (true, false), "référence morte");

    // Identifiant dupliqué : idem.
    let mut doc = base.clone();
    let first = doc["sheets"][0]["points"][0]["id"].clone();
    doc["sheets"][0]["points"][1]["id"] = first;
    assert_eq!(verdicts(&doc), (true, false), "identifiant dupliqué");

    // D-02 (corrigé) : clé JSON dupliquée. Le schéma reçoit un document déjà analysé et ne peut pas
    // la voir ; le chargeur la refuse.
    let text = persist::to_json(&rich_project().project).unwrap();
    let dup = text.replacen(
        "\"next_id\"",
        "\"name\": \"Doublon\",
  \"next_id\"",
        1,
    );
    assert_ne!(dup, text);
    let parsed: Value = serde_json::from_str(&dup).unwrap();
    assert!(
        schema_ok(&schema, &parsed),
        "D-02 : le schéma voit le document analysé"
    );
    let err = persist::from_json(&dup).expect_err("D-02 : clé dupliquée refusée");
    assert!(err.to_string().contains("« name »"), "{err}");
}

/// Tout le corpus passe par la ligne de commande, comme le ferait un utilisateur.
#[test]
fn ca4_pieces_du_corpus_validees_par_la_ligne_de_commande() {
    for (i, (name, doc)) in corpus().into_iter().enumerate() {
        let file = write_tmp(&format!("corpus{i}.pforge.json"), &doc.to_string());
        let run = cli(&[&file]);
        assert_eq!(run.code, 0, "{name} : {}", run.stdout);
    }
}

// ---------------------------------------------------------------------------
// CA5 / CA6 — non-régression et honnêteté
// ---------------------------------------------------------------------------

#[test]
fn ca5_le_format_est_en_version_6() {
    assert_eq!(SCHEMA_VERSION, 6);
    let doc = as_value(&Project::new("X"));
    assert_eq!(doc["schema_version"], json!(6));
    assert!(schema_ok(&schema(), &doc));
}

#[test]
fn ca6_la_specification_reste_honnete() {
    for must in [
        "tel qu'implémenté à ce commit",
        "n'est pas figée",
        "ne remplace pas le validateur",
        "licence de la spécification : à décider par le propriétaire",
        "D-02",
        "D-04",
        "écrit à la main",
    ] {
        assert!(
            SPEC_TEXT.to_lowercase().contains(&must.to_lowercase()),
            "la spécification doit contenir « {must} »"
        );
    }
    let lower = SPEC_TEXT.to_lowercase();
    for banned in [
        "norme officielle",
        "format stable",
        "garanti stable",
        "compatible aama",
    ] {
        assert!(
            !lower.contains(banned),
            "formulation non permise : {banned}"
        );
    }
    assert!(SCHEMA_TEXT.contains("n'est pas une norme"));
}

// ---------------------------------------------------------------------------
// US-09.1 — tailles (schéma v4) et chemin de migration depuis la v3 (réécrite en v5)
// ---------------------------------------------------------------------------

/// Dossier des fixtures v3 : copies des `examples/` d'avant la montée de version, plus un projet
/// v3 avec formules, constructions maintenues et longueurs imposées.
fn fixtures_v3() -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures_v3");
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    v.sort();
    assert!(v.len() >= 6, "les fixtures v3 ont disparu");
    v
}

fn schema_v3() -> Value {
    serde_json::from_str(include_str!("../../../docs/pforge-v3.schema.json")).unwrap()
}

/// Les fichiers v3 restent décrits par le schéma v3, lus par le chargeur (migration en mémoire),
/// et réécrits en v5 valides pour le schéma v5 ; `pf-validate` les accepte sans y toucher.
#[test]
fn us091_fixtures_v3_schema_v3_migration_v5_et_pf_validate() {
    let (s3, s5) = (schema_v3(), schema());
    for path in fixtures_v3() {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let text = std::fs::read_to_string(&path).unwrap();
        let doc: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(doc["schema_version"], json!(3), "{name}");
        assert!(schema_ok(&s3, &doc), "{name} : schéma v3");
        assert!(
            !schema_ok(&s5, &doc),
            "{name} : le schéma v5 exige la version 5"
        );
        // Migration en mémoire : le projet est celui du fichier + version 5 (champs de tailles vides).
        let project = persist::from_json(&text).unwrap();
        assert_eq!(project.schema_version, 6, "{name}");
        assert!(project.sizes.is_empty(), "{name}");
        let written = persist::to_json(&project).unwrap();
        let mut expected = doc.clone();
        expected["schema_version"] = json!(6);
        let written_value: Value = serde_json::from_str(&written).unwrap();
        assert_eq!(
            written_value, expected,
            "{name} : perte ou ajout silencieux"
        );
        assert_eq!(
            written,
            text.replace("\"schema_version\": 3", "\"schema_version\": 6")
                .replace("\r\n", "\n"),
            "{name} : seule la ligne de version change"
        );
        assert!(schema_ok(&s5, &written_value), "{name} : v5 écrit invalide");
        assert_eq!(persist::from_json(&written).unwrap(), project, "{name}");
        // pf-validate : 0, migré en mémoire, fichier intact.
        let before = std::fs::read(&path).unwrap();
        let run = cli(&[&path.to_string_lossy(), "--json"]);
        assert_eq!(run.code, 0, "{name} : {}", run.stdout);
        let v: Value = serde_json::from_str(&run.stdout).unwrap();
        assert_eq!(v["file_schema_version"], json!(3));
        assert_eq!(v["app_schema_version"], json!(6));
        assert_eq!(v["migrated_in_memory"], json!(true));
        assert_eq!(
            std::fs::read(&path).unwrap(),
            before,
            "{name} : fichier modifié"
        );
    }
}

/// La fixture à formules / constructions maintenues / longueurs imposées garde ses liens.
#[test]
fn us091_fixture_v3_liens_conserves_apres_migration() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures_v3/v3-formules-maintenues-longueurs.pforge.json");
    let text = std::fs::read_to_string(path).unwrap();
    for key in [
        "\"formulas\"",
        "\"kept\"",
        "\"length_constraints\"",
        "\"reference\"",
    ] {
        assert!(text.contains(key), "la fixture doit contenir {key}");
    }
    let p = persist::from_json(&text).unwrap();
    assert!(!p.variables.is_empty());
    assert!(p.sheets.iter().any(|s| !s.formulas.is_empty()));
    assert!(p.sheets.iter().any(|s| !s.length_constraints.is_empty()));
    // Les positions évaluées après migration sont celles du fichier (liens recalculés = valeurs
    // écrites, à 1e-9 près : le chargeur l'a vérifié) ; aucune n'est modifiée par la migration.
    let raw: Value = serde_json::from_str(&text).unwrap();
    for (si, sheet) in raw["sheets"].as_array().unwrap().iter().enumerate() {
        for (pi, pt) in sheet["points"].as_array().unwrap().iter().enumerate() {
            if pt["definition"]["type"] == "free" {
                let got = p.sheets[si].position(p.sheets[si].points[pi].id).unwrap();
                assert_eq!(
                    got.x.to_bits(),
                    pt["definition"]["x"].as_f64().unwrap().to_bits()
                );
                assert_eq!(
                    got.y.to_bits(),
                    pt["definition"]["y"].as_f64().unwrap().to_bits()
                );
            }
        }
    }
    // Ouvrir sans enregistrer n'écrit rien ; enregistrer écrit en v5 et relit le même projet.
    let dir = tmp().join("us091");
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("copie-v3.pforge.json");
    std::fs::write(&file, &text).unwrap();
    let mut doc = pf_core::Document::open(&file).unwrap();
    assert!(!doc.is_dirty());
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        text,
        "ouverture silencieuse"
    );
    doc.save_as(&file).unwrap();
    let saved = std::fs::read_to_string(&file).unwrap();
    assert!(saved.contains("\"schema_version\": 6") && !saved.contains("\"schema_version\": 3"));
    assert_eq!(persist::from_json(&saved).unwrap(), *doc.project());
}

fn sized_project() -> Project {
    let mut p = pf_core::demo_piece_project().unwrap();
    p.apply(Command::AddSizeRange {
        name: "Femme".into(),
        kind: pf_core::SizeKind::Alphanumeric,
        labels: ["XS", "S", "M", "L", "XL"].map(String::from).to_vec(),
        base: 2,
    })
    .unwrap();
    let s = p.sizes[0].sizes[1].id;
    p.apply(Command::SetSizeBreak {
        size: s,
        break_size: true,
    })
    .unwrap();
    p.apply(Command::AddSizeRange {
        name: "Tour".into(),
        kind: pf_core::SizeKind::Numeric,
        labels: ["34", "36", "38"].map(String::from).to_vec(),
        base: 0,
    })
    .unwrap();
    p
}

fn graded_project() -> Project {
    let mut p = sized_project();
    let s = p.sheets[0].id;
    let point = p
        .apply(Command::AddPoint {
            sheet: s,
            at: PointMm::new(3000.0, 3100.0),
        })
        .unwrap()
        .points[0];
    for (i, (dx, dy)) in [(-5.0, -10.0), (5.0, 10.0)].into_iter().enumerate() {
        let size = p.sizes[0].sizes[[1, 3][i]].id;
        p.apply(Command::SetPointGrading {
            sheet: s,
            point,
            size,
            dx_mm: dx,
            dy_mm: dy,
        })
        .unwrap();
    }
    p
}

#[test]
fn us092_ecarts_de_gradation_schema_chargeur_et_pf_validate() {
    let schema = schema();
    let good = as_value(&graded_project());
    let n = good["sheets"][0]["gradings"].as_array().unwrap().len();
    assert_eq!(n, 2);
    assert!(schema_ok(&schema, &good) && loader_ok(&good));
    let ok = cli(&[&write_tmp("gradation-ok.pforge.json", &good.to_string())]);
    assert_eq!(ok.code, 0, "{}", ok.stdout);
    // Un projet sans gradation n'a pas la clé (octets d'avant).
    assert!(as_value(&sized_project())["sheets"][0]
        .get("gradings")
        .is_none());
    // Altérations refusées par le chargeur et par pf-validate (code 1) ; le schéma ne voit que la
    // structure : on note ce qu'il dit pour chaque cas.
    type Mutator = Box<dyn Fn(&mut Value)>;
    let cases: Vec<(&str, Mutator, bool)> = vec![
        (
            "écart en double",
            Box::new(|d| {
                let first = d["sheets"][0]["gradings"][0].clone();
                d["sheets"][0]["gradings"]
                    .as_array_mut()
                    .unwrap()
                    .push(first);
            }),
            true,
        ),
        (
            "taille inconnue",
            Box::new(|d| d["sheets"][0]["gradings"][0]["size"] = json!(987_654)),
            true,
        ),
        (
            "point inconnu",
            Box::new(|d| d["sheets"][0]["gradings"][0]["point"] = json!(987_654)),
            true,
        ),
        (
            "écart sur la taille de base",
            Box::new(|d| {
                let base = d["sizes"][0]["base"].clone();
                d["sheets"][0]["gradings"][0]["size"] = base;
            }),
            true,
        ),
        (
            "écart nul",
            Box::new(|d| {
                d["sheets"][0]["gradings"][0]["dx_mm"] = json!(0.0);
                d["sheets"][0]["gradings"][0]["dy_mm"] = json!(0.0);
            }),
            true,
        ),
        (
            "valeur hors bornes",
            Box::new(|d| d["sheets"][0]["gradings"][0]["dx_mm"] = json!(2.0e6)),
            false,
        ),
        (
            "valeur non numérique",
            Box::new(|d| d["sheets"][0]["gradings"][0]["dy_mm"] = json!("12")),
            false,
        ),
        (
            "champ inconnu",
            Box::new(|d| d["sheets"][0]["gradings"][0]["zz"] = json!(1)),
            false,
        ),
        (
            "champ manquant",
            Box::new(|d| {
                d["sheets"][0]["gradings"][0]
                    .as_object_mut()
                    .unwrap()
                    .remove("size");
            }),
            false,
        ),
    ];
    for (name, mutate, schema_accepts) in cases {
        let mut doc = good.clone();
        mutate(&mut doc);
        assert!(!loader_ok(&doc), "{name} : le chargeur accepte");
        assert_eq!(
            schema_ok(&schema, &doc),
            schema_accepts,
            "{name} : schéma {:?}",
            validateur::validate(&schema, &doc)
        );
        let file = write_tmp("gradation-ko.pforge.json", &doc.to_string());
        let run = cli(&[&file]);
        assert_eq!(run.code, 1, "{name} : {}", run.stdout);
    }
    // Un point construit gradué est refusé (il ne se gradue pas) ; la clé avant la v4 aussi.
    let mut constructed = good.clone();
    let offset_point = constructed["sheets"][0]["points"]
        .as_array()
        .unwrap()
        .iter()
        .find(|q| q["definition"]["type"] == "offset")
        .map(|q| q["id"].clone());
    if let Some(id) = offset_point {
        constructed["sheets"][0]["gradings"][0]["point"] = id;
        assert!(!loader_ok(&constructed), "point construit gradué accepté");
    }
    let mut v3 = good.clone();
    v3["schema_version"] = json!(3);
    v3.as_object_mut().unwrap().remove("sizes");
    assert!(!loader_ok(&v3));
    let run = cli(&[&write_tmp("gradation-v3.pforge.json", &v3.to_string())]);
    assert_eq!(run.code, 1, "{}", run.stdout);
    assert!(run.stdout.contains("gradings"), "{}", run.stdout);
}

#[test]
fn us091_tailles_schema_chargeur_et_pf_validate() {
    let schema = schema();
    let good = as_value(&sized_project());
    assert!(good["sizes"].as_array().unwrap().len() == 2);
    assert!(schema_ok(&schema, &good) && loader_ok(&good));
    let ok = cli(&[&write_tmp("tailles-ok.pforge.json", &good.to_string())]);
    assert_eq!(ok.code, 0, "{}", ok.stdout);
    // Altérations refusées par le chargeur et par pf-validate (code 1) ; le schéma, lui, ne voit
    // que la structure : on note ce qu'il dit pour chaque cas.
    type Mutator = Box<dyn Fn(&mut Value)>;
    let cases: Vec<(&str, Mutator, bool)> = vec![
        (
            "identifiant de taille en double",
            Box::new(|d| d["sizes"][0]["sizes"][1]["id"] = d["sizes"][0]["sizes"][0]["id"].clone()),
            true,
        ),
        (
            "base absente de la gamme",
            Box::new(|d| d["sizes"][0]["base"] = d["sizes"][1]["sizes"][0]["id"].clone()),
            true,
        ),
        (
            "deux gammes de même identifiant",
            Box::new(|d| d["sizes"][1]["id"] = d["sizes"][0]["id"].clone()),
            true,
        ),
        (
            "libellé vide",
            Box::new(|d| d["sizes"][0]["sizes"][0]["label"] = json!("")),
            true,
        ),
        (
            "libellé en double",
            Box::new(|d| d["sizes"][0]["sizes"][0]["label"] = json!("s")),
            true,
        ),
        (
            "gamme sans taille",
            Box::new(|d| d["sizes"][0]["sizes"] = json!([])),
            false,
        ),
        (
            "type de gamme inconnu",
            Box::new(|d| d["sizes"][0]["kind"] = json!("romain")),
            false,
        ),
        (
            "champ inconnu dans une taille",
            Box::new(|d| d["sizes"][0]["sizes"][0]["zz"] = json!(1)),
            false,
        ),
        (
            "base manquante",
            Box::new(|d| {
                d["sizes"][0].as_object_mut().unwrap().remove("base");
            }),
            false,
        ),
        (
            "rupture non booléenne",
            Box::new(|d| d["sizes"][0]["sizes"][1]["break_size"] = json!("oui")),
            false,
        ),
    ];
    for (name, mutate, schema_accepts) in cases {
        let mut doc = good.clone();
        mutate(&mut doc);
        assert!(!loader_ok(&doc), "{name} : le chargeur accepte");
        assert_eq!(
            schema_ok(&schema, &doc),
            schema_accepts,
            "{name} : schéma {:?}",
            validateur::validate(&schema, &doc)
        );
        let file = write_tmp("tailles-ko.pforge.json", &doc.to_string());
        let run = cli(&[&file]);
        assert_eq!(run.code, 1, "{name} : {}", run.stdout);
    }
    // Un fichier v3 qui contient `sizes` : refusé (clé réservée), pf-validate code 1.
    let mut v3 = good.clone();
    v3["schema_version"] = json!(3);
    assert!(!loader_ok(&v3));
    let run = cli(&[&write_tmp("tailles-v3.pforge.json", &v3.to_string())]);
    assert_eq!(run.code, 1, "{}", run.stdout);
    assert!(run.stdout.contains("réservée"), "{}", run.stdout);
}

// ---------------------------------------------------------------------------
// Décision 334 — stockage v5 (règles nommées, barème, clients, retouches, mesures finies)
// ---------------------------------------------------------------------------

/// Projet qui porte **tous** les objets v5 (construits directement, sans commande : il n'en existe pas).
fn v5_project() -> Project {
    use pf_core::*;
    let mut p = extra_project();
    let sz: Vec<SizeId> = p.sizes[0].sizes.iter().map(|s| s.id).collect();
    let seam = p.seams[0].id;
    let (sheet, dim) = p
        .sheets
        .iter()
        .find_map(|s| s.full_dimensions.first().map(|d| (s.id, d.id)))
        .unwrap();
    p.sheets
        .iter_mut()
        .flat_map(|s| s.notches.iter_mut())
        .find(|n| n.anchor.is_none())
        .unwrap()
        .proportional = true;
    p.variables[0].per_size = vec![
        VariableSizeExpr {
            size: sz[0],
            expr: "10".into(),
        },
        VariableSizeExpr {
            size: sz[3],
            expr: "12,5".into(),
        },
    ];
    let mut next = p.next_id;
    let mut id = || {
        next += 1;
        next - 1
    };
    p.grading_rules = vec![
        GradingRule {
            id: GradingRuleId(id()),
            name: "Hanche".into(),
            mode: GradingCopyMode::MirrorX,
            deltas: vec![
                GradingRuleDelta {
                    size: sz[0],
                    dx_mm: -5.0,
                    dy_mm: -10.0,
                },
                GradingRuleDelta {
                    size: sz[3],
                    dx_mm: 5.0,
                    dy_mm: 10.0,
                },
            ],
        },
        GradingRule {
            id: GradingRuleId(id()),
            name: "Épaule".into(),
            mode: GradingCopyMode::Same,
            deltas: vec![GradingRuleDelta {
                size: sz[1],
                dx_mm: 2.5,
                dy_mm: 0.0,
            }],
        },
    ];
    let waist = MeasurementId(id());
    let angle = MeasurementId(id());
    p.measurements = vec![
        Measurement {
            id: waist,
            name: "Tour de taille".into(),
            unit: MeasurementUnit::Mm,
            values: sz
                .iter()
                .zip([640.0, 680.0, 720.0, 760.0])
                .map(|(&size, value)| MeasurementValue { size, value })
                .collect(),
        },
        Measurement {
            id: angle,
            name: "Pente d'épaule".into(),
            unit: MeasurementUnit::Deg,
            values: Vec::new(),
        },
    ];
    p.clients = vec![Client {
        id: ClientId(id()),
        name: "Fiche A".into(),
        values: vec![ClientValue {
            measurement: waist,
            value: 701.5,
        }],
    }];
    p.alterations = vec![Alteration {
        id: AlterationId(id()),
        name: "Manche +15".into(),
        target: waist,
        delta: 15.0,
    }];
    p.finished_measures = vec![
        FinishedMeasure {
            id: FinishedMeasureId(id()),
            name: "Longueur de couture".into(),
            measurement: waist,
            source: FinishedSource::Seam { seam },
            tolerance_minus: 5.0,
            tolerance_plus: 10.0,
        },
        FinishedMeasure {
            id: FinishedMeasureId(id()),
            name: "Cote".into(),
            measurement: angle,
            source: FinishedSource::FullDimension {
                sheet,
                dimension: dim,
            },
            tolerance_minus: 0.0,
            tolerance_plus: 0.0,
        },
    ];
    p.next_id = next;
    p.validate().unwrap();
    p
}

fn schema_v4() -> Value {
    serde_json::from_str(include_str!("../../../docs/pforge-v4.schema.json")).unwrap()
}

const V5_PROJECT_KEYS: [&str; 5] = [
    "grading_rules",
    "measurements",
    "clients",
    "alterations",
    "finished_measures",
];

#[test]
fn us334_objets_v5_schema_chargeur_et_pf_validate() {
    let schema = schema();
    let good = as_value(&v5_project());
    for key in V5_PROJECT_KEYS {
        assert!(good[key].as_array().is_some_and(|a| !a.is_empty()), "{key}");
    }
    assert!(schema_ok(&schema, &good) && loader_ok(&good));
    assert_eq!(
        cli(&[&write_tmp("v5-ok.pforge.json", &good.to_string())]).code,
        0
    );
    // Relu = réécrit à l'identique.
    let text = persist::to_json(&v5_project()).unwrap();
    assert_eq!(
        persist::to_json(&persist::from_json(&text).unwrap()).unwrap(),
        text
    );
    // (nom, mutation, le schéma accepte encore ?) : le chargeur refuse toujours, pf-validate code 1.
    type Case = (&'static str, Box<dyn Fn(&mut Value)>, bool);
    let cases: Vec<Case> = vec![
        (
            "clé en trop sur une règle",
            Box::new(|d| d["grading_rules"][0]["zz"] = json!(1)),
            false,
        ),
        (
            "clé en trop sur un écart de règle",
            Box::new(|d| d["grading_rules"][0]["deltas"][0]["zz"] = json!(1)),
            false,
        ),
        (
            "clé en trop sur une mesure",
            Box::new(|d| d["measurements"][0]["zz"] = json!(1)),
            false,
        ),
        (
            "clé en trop sur une source finie",
            Box::new(|d| d["finished_measures"][0]["source"]["zz"] = json!(1)),
            false,
        ),
        (
            "clé en trop sur per_size",
            Box::new(|d| d["variables"][0]["per_size"][0]["zz"] = json!(1)),
            false,
        ),
        (
            "mode de copie inconnu",
            Box::new(|d| d["grading_rules"][0]["mode"] = json!("rotation")),
            false,
        ),
        (
            "unité inconnue",
            Box::new(|d| d["measurements"][0]["unit"] = json!("cm")),
            false,
        ),
        (
            "type de source inconnu",
            Box::new(|d| d["finished_measures"][0]["source"]["type"] = json!("piece")),
            false,
        ),
        (
            "valeur de mesure texte",
            Box::new(|d| d["measurements"][0]["values"][0]["value"] = json!("640")),
            false,
        ),
        (
            "proportional texte",
            Box::new(|d| {
                for s in d["sheets"].as_array_mut().unwrap() {
                    for n in s
                        .get_mut("notches")
                        .and_then(Value::as_array_mut)
                        .into_iter()
                        .flatten()
                    {
                        if n.get("proportional").is_some() {
                            n["proportional"] = json!("oui");
                        }
                    }
                }
            }),
            false,
        ),
        (
            "valeur hors bornes",
            Box::new(|d| d["measurements"][0]["values"][0]["value"] = json!(1e300)),
            false,
        ),
        (
            "taille inconnue dans une règle",
            Box::new(|d| d["grading_rules"][0]["deltas"][0]["size"] = json!(99999)),
            true,
        ),
        (
            "taille inconnue dans une mesure",
            Box::new(|d| d["measurements"][0]["values"][0]["size"] = json!(99999)),
            true,
        ),
        (
            "taille inconnue dans per_size",
            Box::new(|d| d["variables"][0]["per_size"][0]["size"] = json!(99999)),
            true,
        ),
        (
            "mesure inconnue dans une fiche",
            Box::new(|d| d["clients"][0]["values"][0]["measurement"] = json!(99999)),
            true,
        ),
        (
            "mesure inconnue dans une retouche",
            Box::new(|d| d["alterations"][0]["target"] = json!(99999)),
            true,
        ),
        (
            "mesure inconnue dans une mesure finie",
            Box::new(|d| d["finished_measures"][0]["measurement"] = json!(99999)),
            true,
        ),
        (
            "couture inconnue",
            Box::new(|d| d["finished_measures"][0]["source"]["seam"] = json!(99999)),
            true,
        ),
        (
            "cote inconnue",
            Box::new(|d| d["finished_measures"][1]["source"]["dimension"] = json!(99999)),
            true,
        ),
        (
            "identifiant en double",
            Box::new(|d| d["alterations"][0]["id"] = d["clients"][0]["id"].clone()),
            true,
        ),
        (
            "nom en double (casse ignorée)",
            Box::new(|d| d["grading_rules"][1]["name"] = json!("HANCHE")),
            true,
        ),
        (
            "règle sans écart",
            Box::new(|d| d["grading_rules"][1]["deltas"] = json!([])),
            false,
        ),
        (
            "même taille deux fois dans une règle",
            Box::new(|d| {
                let first = d["grading_rules"][0]["deltas"][0].clone();
                d["grading_rules"][0]["deltas"][1] = first;
            }),
            true,
        ),
        (
            "per_size dont l'expression ne s'évalue pas",
            Box::new(|d| d["variables"][0]["per_size"][0]["expr"] = json!("1 / 0")),
            true,
        ),
        (
            "tolérance négative",
            Box::new(|d| d["finished_measures"][0]["tolerance_plus"] = json!(-1.0)),
            false,
        ),
        (
            "proportional avec un ancrage",
            Box::new(|d| {
                for s in d["sheets"].as_array_mut().unwrap() {
                    for n in s
                        .get_mut("notches")
                        .and_then(Value::as_array_mut)
                        .into_iter()
                        .flatten()
                    {
                        if n.get("proportional").is_some() {
                            n["anchor"] = json!({"type": "midpoint"});
                        }
                    }
                }
            }),
            true,
        ),
    ];
    for (name, mutate, schema_accepts) in cases {
        let mut doc = good.clone();
        mutate(&mut doc);
        assert_ne!(doc, good, "{name} : la mutation n'a rien changé");
        assert!(!loader_ok(&doc), "{name} : le chargeur accepte");
        assert_eq!(
            schema_ok(&schema, &doc),
            schema_accepts,
            "{name} : schéma {:?}",
            validateur::validate(&schema, &doc)
        );
        let run = cli(&[&write_tmp("v5-ko.pforge.json", &doc.to_string())]);
        assert_eq!(run.code, 1, "{name} : {}", run.stdout);
    }
}

/// Retire de `doc` tous les objets v5 (projet, `per_size`, `proportional`).
fn strip_v5(doc: &mut Value) {
    for k in V5_PROJECT_KEYS {
        doc.as_object_mut().unwrap().remove(k);
    }
    for v in doc["variables"].as_array_mut().unwrap() {
        v.as_object_mut().unwrap().remove("per_size");
    }
    for s in doc["sheets"].as_array_mut().unwrap() {
        for n in s
            .get_mut("notches")
            .and_then(Value::as_array_mut)
            .into_iter()
            .flatten()
        {
            n.as_object_mut().unwrap().remove("proportional");
        }
    }
}

/// Un fichier déclaré v4 (ou plus ancien) ne contient aucun objet v5 : clé réservée, refus.
#[test]
fn us334_objets_v5_dans_un_fichier_v4_refuses() {
    let good = as_value(&v5_project());
    let mut base = good.clone();
    strip_v5(&mut base);
    assert!(
        loader_ok(&base),
        "la base sans objet v5 doit s'ouvrir : {:?}",
        persist::from_json(&base.to_string()).err()
    );
    let mut docs: Vec<(String, Value)> = Vec::new();
    for key in V5_PROJECT_KEYS {
        let mut d = base.clone();
        d[key] = good[key].clone();
        docs.push((key.to_string(), d));
    }
    let mut d = base.clone();
    d["variables"][0]["per_size"] = good["variables"][0]["per_size"].clone();
    docs.push(("per_size".into(), d));
    let mut d = base.clone();
    'copy: for (si, s) in good["sheets"].as_array().unwrap().iter().enumerate() {
        for (ni, n) in s["notches"].as_array().into_iter().flatten().enumerate() {
            if n.get("proportional").is_some() {
                d["sheets"][si]["notches"][ni]["proportional"] = json!(true);
                break 'copy;
            }
        }
    }
    docs.push(("proportional".into(), d));
    for (key, doc) in docs {
        for version in [4, 3, 1] {
            let mut doc = doc.clone();
            doc["schema_version"] = json!(version);
            if version < 4 {
                // La clé `sizes` est elle-même réservée à la v4 : on ne garde que la clé testée.
                doc.as_object_mut().unwrap().remove("sizes");
            }
            let err = persist::from_json(&doc.to_string()).expect_err(&key);
            let msg = err.to_string();
            assert!(msg.contains("réservée"), "{key} v{version} : {msg}");
            assert!(
                version < 4 || msg.contains(&key),
                "{key} v{version} : message sans la clé : {msg}"
            );
            let run = cli(&[&write_tmp("v4-reserve.pforge.json", &doc.to_string())]);
            assert_eq!(run.code, 1, "{key} v{version} : {}", run.stdout);
        }
    }
}

/// Les fichiers v4 du dépôt (fixtures + exemples) : décrits par le schéma v4, lus par le chargeur
/// (migration en mémoire), réécrits en v5 valides pour le schéma v5, `pf-validate` code 0.
#[test]
fn us334_fixtures_v4_schema_v4_migration_v5_et_pf_validate() {
    let (s4, s5) = (schema_v4(), schema());
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures_v4");
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    files.sort();
    assert!(files.len() >= 6, "les fixtures v4 ont disparu");
    for path in files {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let text = std::fs::read_to_string(&path).unwrap();
        let doc: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(doc["schema_version"], json!(4), "{name}");
        assert!(schema_ok(&s4, &doc), "{name} : schéma v4");
        assert!(
            !schema_ok(&s5, &doc),
            "{name} : le schéma v5 exige la version 5"
        );
        let project = persist::from_json(&text).unwrap();
        assert_eq!(project.schema_version, 6, "{name}");
        let written = persist::to_json(&project).unwrap();
        let mut expected = doc.clone();
        expected["schema_version"] = json!(6);
        let written_value: Value = serde_json::from_str(&written).unwrap();
        assert_eq!(
            written_value, expected,
            "{name} : perte ou ajout silencieux"
        );
        assert_eq!(
            written,
            text.replace("\"schema_version\": 4", "\"schema_version\": 6"),
            "{name} : seule la ligne de version change"
        );
        assert!(schema_ok(&s5, &written_value), "{name} : v5 écrit invalide");
        let before = std::fs::read(&path).unwrap();
        let run = cli(&[&path.to_string_lossy(), "--json"]);
        assert_eq!(run.code, 0, "{name} : {}", run.stdout);
        let v: Value = serde_json::from_str(&run.stdout).unwrap();
        assert_eq!(v["file_schema_version"], json!(4));
        assert_eq!(v["app_schema_version"], json!(6));
        assert_eq!(v["migrated_in_memory"], json!(true));
        assert_eq!(
            std::fs::read(&path).unwrap(),
            before,
            "{name} : fichier modifié"
        );
    }
}

/// Projet v5 avec deux plans de coupe (socle d'E15) : un plan complet et un plan nu.
fn plans_coupe_project() -> Project {
    use pf_core::cutting_plan::{CuttingPlanSpec, FabricFold};
    let mut p = v5_project();
    let piece = p.pieces[0].id;
    let size = p.sizes[0].sizes[1].id;
    let plan = p
        .apply(Command::AddCuttingPlan {
            spec: CuttingPlanSpec {
                name: "Plan A".into(),
                material: Some("Coton".into()),
                fabric_width_mm: 1500.0,
                fold: FabricFold::Folded,
                selvedge_mm: 10.0,
                min_gap_mm: 5.0,
                grain_direction: Some(90.0),
            },
        })
        .unwrap()
        .cutting_plans[0];
    for (size, rot, mirrored) in [(Some(size), 90.0, true), (None, 0.0, false)] {
        p.apply(Command::AddPlacement {
            plan,
            piece,
            size,
            x_mm: 12.5,
            y_mm: -3.0,
            rotation_deg: rot,
            mirrored,
        })
        .unwrap();
    }
    p.apply(Command::AddCuttingPlan {
        spec: CuttingPlanSpec {
            name: "Plan B".into(),
            material: None,
            fabric_width_mm: 1400.0,
            fold: FabricFold::Flat,
            selvedge_mm: 0.0,
            min_gap_mm: 0.0,
            grain_direction: None,
        },
    })
    .unwrap();
    p
}

#[test]
fn plans_coupe_schema_chargeur_et_pf_validate() {
    let schema = schema();
    let p = plans_coupe_project();
    let good = as_value(&p);
    assert_eq!(good["cutting_plans"].as_array().unwrap().len(), 2);
    assert!(schema_ok(&schema, &good) && loader_ok(&good));
    assert_eq!(
        cli(&[&write_tmp("plans-ok.pforge.json", &good.to_string())]).code,
        0
    );
    // Relu = réécrit à l'identique ; sans plan, la clé n'existe pas (octets de la v5 d'avant).
    let text = persist::to_json(&p).unwrap();
    assert_eq!(
        persist::to_json(&persist::from_json(&text).unwrap()).unwrap(),
        text
    );
    assert!(!persist::to_json(&v5_project())
        .unwrap()
        .contains("cutting_plans"));
    // (nom, mutation, le schéma accepte encore ?) : le chargeur refuse toujours, pf-validate code 1.
    type Case = (&'static str, Box<dyn Fn(&mut Value)>, bool);
    let cases: Vec<Case> = vec![
        (
            "clé en trop sur un plan",
            Box::new(|d| d["cutting_plans"][0]["zz"] = json!(1)),
            false,
        ),
        (
            "clé en trop sur un placement",
            Box::new(|d| d["cutting_plans"][0]["placements"][0]["zz"] = json!(1)),
            false,
        ),
        (
            "pliage inconnu",
            Box::new(|d| d["cutting_plans"][0]["fold"] = json!("rolled")),
            false,
        ),
        (
            "laize texte",
            Box::new(|d| d["cutting_plans"][0]["fabric_width_mm"] = json!("1500")),
            false,
        ),
        (
            "laize nulle",
            Box::new(|d| d["cutting_plans"][0]["fabric_width_mm"] = json!(0.0)),
            true,
        ),
        (
            "laize > 10 000",
            Box::new(|d| d["cutting_plans"][0]["fabric_width_mm"] = json!(10000.5)),
            false,
        ),
        (
            "lisière trop grande",
            Box::new(|d| d["cutting_plans"][0]["selvedge_mm"] = json!(750.0)),
            true,
        ),
        (
            "écart > 1 000",
            Box::new(|d| d["cutting_plans"][0]["min_gap_mm"] = json!(1000.5)),
            false,
        ),
        (
            "droit-fil 360",
            Box::new(|d| d["cutting_plans"][0]["grain_direction"] = json!(360.0)),
            true,
        ),
        (
            "droit-fil négatif",
            Box::new(|d| d["cutting_plans"][0]["grain_direction"] = json!(-1.0)),
            false,
        ),
        (
            "nom vide",
            Box::new(|d| d["cutting_plans"][0]["name"] = json!("")),
            false,
        ),
        (
            "nom dupliqué (casse)",
            Box::new(|d| d["cutting_plans"][1]["name"] = json!("plan a")),
            true,
        ),
        (
            "pièce inconnue",
            Box::new(|d| d["cutting_plans"][0]["placements"][0]["piece"] = json!(99999)),
            true,
        ),
        (
            "taille inconnue",
            Box::new(|d| d["cutting_plans"][0]["placements"][0]["size"] = json!(99999)),
            true,
        ),
        (
            "id de placement dupliqué",
            Box::new(|d| {
                let id = d["cutting_plans"][0]["placements"][0]["id"].clone();
                d["cutting_plans"][0]["placements"][1]["id"] = id;
            }),
            true,
        ),
        (
            "rotation 360",
            Box::new(|d| d["cutting_plans"][0]["placements"][0]["rotation_deg"] = json!(360.0)),
            true,
        ),
        (
            "rotation négative",
            Box::new(|d| d["cutting_plans"][0]["placements"][0]["rotation_deg"] = json!(-1.0)),
            false,
        ),
        (
            "position hors bornes",
            Box::new(|d| d["cutting_plans"][0]["placements"][0]["x_mm"] = json!(1.0e7)),
            false,
        ),
        (
            "miroir texte",
            Box::new(|d| d["cutting_plans"][0]["placements"][0]["mirrored"] = json!("oui")),
            false,
        ),
    ];
    for (name, mutate, schema_accepts) in cases {
        let mut doc = good.clone();
        mutate(&mut doc);
        assert_ne!(doc, good, "{name} : la mutation n'a rien changé");
        assert!(!loader_ok(&doc), "{name} : le chargeur accepte");
        assert_eq!(
            schema_ok(&schema, &doc),
            schema_accepts,
            "{name} : schéma {:?}",
            validateur::validate(&schema, &doc)
        );
        let run = cli(&[&write_tmp("plans-ko.pforge.json", &doc.to_string())]);
        assert_eq!(run.code, 1, "{name} : {}", run.stdout);
    }
}

/// Projet v5 avec deux variantes (US-11.1) : une complète (option active, matière, option d'entrée)
/// et une nue.
fn variantes_project() -> Project {
    let mut p = v5_project();
    let (a, b) = (p.pieces[0].id, p.pieces[1].id);
    let v = p
        .apply(Command::AddVariant {
            name: "Chemise".into(),
        })
        .unwrap()
        .variants[0];
    p.apply(Command::AddVariantOption {
        variant: v,
        option: "avec poche".into(),
    })
    .unwrap();
    for (piece, quantity, material, option) in
        [(a, 1, None, None), (b, 2, Some("Lin"), Some("avec poche"))]
    {
        p.apply(Command::AddVariantEntry {
            variant: v,
            piece,
            quantity,
            material: material.map(str::to_string),
            option: option.map(str::to_string),
        })
        .unwrap();
    }
    p.apply(Command::SetVariantOptionActive {
        variant: v,
        option: "avec poche".into(),
        active: true,
    })
    .unwrap();
    p.apply(Command::AddVariant { name: "Nue".into() }).unwrap();
    p
}

#[test]
fn variantes_schema_chargeur_et_pf_validate() {
    let schema = schema();
    let p = variantes_project();
    let good = as_value(&p);
    assert_eq!(good["variants"].as_array().unwrap().len(), 2);
    assert!(schema_ok(&schema, &good) && loader_ok(&good));
    assert_eq!(
        cli(&[&write_tmp("variantes-ok.pforge.json", &good.to_string())]).code,
        0
    );
    // Relu = réécrit à l'identique ; sans variante, la clé n'existe pas (octets de la v5 d'avant).
    let text = persist::to_json(&p).unwrap();
    assert_eq!(
        persist::to_json(&persist::from_json(&text).unwrap()).unwrap(),
        text
    );
    assert!(!persist::to_json(&v5_project())
        .unwrap()
        .contains("variants"));
    // (nom, mutation, le schéma accepte encore ?) : le chargeur refuse toujours, pf-validate code 1.
    type Case = (&'static str, Box<dyn Fn(&mut Value)>, bool);
    let cases: Vec<Case> = vec![
        (
            "clé en trop sur une variante",
            Box::new(|d| d["variants"][0]["zz"] = json!(1)),
            false,
        ),
        (
            "clé en trop sur une entrée",
            Box::new(|d| d["variants"][0]["entries"][0]["zz"] = json!(1)),
            false,
        ),
        (
            "quantité 0",
            Box::new(|d| d["variants"][0]["entries"][0]["quantity"] = json!(0)),
            false,
        ),
        (
            "quantité 1000",
            Box::new(|d| d["variants"][0]["entries"][0]["quantity"] = json!(1000)),
            false,
        ),
        (
            "quantité décimale",
            Box::new(|d| d["variants"][0]["entries"][0]["quantity"] = json!(1.5)),
            false,
        ),
        (
            "quantité texte",
            Box::new(|d| d["variants"][0]["entries"][0]["quantity"] = json!("2")),
            false,
        ),
        (
            "pièce inconnue",
            Box::new(|d| d["variants"][0]["entries"][0]["piece"] = json!(99999)),
            true,
        ),
        (
            "option d'entrée inconnue",
            Box::new(|d| d["variants"][0]["entries"][1]["option"] = json!("fantôme")),
            true,
        ),
        (
            "option active inconnue",
            Box::new(|d| d["variants"][0]["options_active"] = json!(["fantôme"])),
            true,
        ),
        (
            "nom dupliqué (casse)",
            Box::new(|d| d["variants"][1]["name"] = json!("chemise")),
            true,
        ),
        (
            "nom vide",
            Box::new(|d| d["variants"][1]["name"] = json!("")),
            false,
        ),
        (
            "options en texte",
            Box::new(|d| d["variants"][0]["options"] = json!("avec poche")),
            false,
        ),
        (
            "entrée dupliquée",
            Box::new(|d| {
                let e = d["variants"][0]["entries"][0].clone();
                d["variants"][0]["entries"].as_array_mut().unwrap().push(e);
            }),
            true,
        ),
    ];
    for (name, mutate, schema_accepts) in cases {
        let mut doc = good.clone();
        mutate(&mut doc);
        assert_ne!(doc, good, "{name} : la mutation n'a rien changé");
        assert!(!loader_ok(&doc), "{name} : le chargeur accepte");
        assert_eq!(
            schema_ok(&schema, &doc),
            schema_accepts,
            "{name} : schéma {:?}",
            validateur::validate(&schema, &doc)
        );
        let run = cli(&[&write_tmp("variantes-ko.pforge.json", &doc.to_string())]);
        assert_eq!(run.code, 1, "{name} : {}", run.stdout);
    }
}

/// Projet v5 avec deux règles de nommage (US-11.5).
fn nommage_project() -> Project {
    let mut p = v5_project();
    for (name, target, prefix, start, step, width) in [
        ("Pièces", NamingTarget::Piece, "P", 1, 1, 3),
        ("Feuilles", NamingTarget::Sheet, "F", 10, 10, 0),
    ] {
        p.apply(Command::AddNamingRule {
            spec: NamingRuleSpec {
                name: name.into(),
                target,
                prefix: prefix.into(),
                separator: "-".into(),
                suffix: "x".into(),
                counter: Some(NamingCounter { start, step, width }),
            },
        })
        .unwrap();
    }
    p
}

#[test]
fn nommage_schema_chargeur_et_pf_validate() {
    let schema = schema();
    let p = nommage_project();
    let good = as_value(&p);
    assert_eq!(good["naming_rules"].as_array().unwrap().len(), 2);
    assert!(schema_ok(&schema, &good) && loader_ok(&good));
    assert_eq!(
        cli(&[&write_tmp("nommage-ok.pforge.json", &good.to_string())]).code,
        0
    );
    let text = persist::to_json(&p).unwrap();
    assert_eq!(
        persist::to_json(&persist::from_json(&text).unwrap()).unwrap(),
        text
    );
    assert!(!persist::to_json(&v5_project())
        .unwrap()
        .contains("naming_rules"));
    // (nom, mutation, le schéma accepte encore ?) : le chargeur refuse toujours, pf-validate code 1.
    type Case = (&'static str, Box<dyn Fn(&mut Value)>, bool);
    let cases: Vec<Case> = vec![
        (
            "clé en trop sur une règle",
            Box::new(|d| d["naming_rules"][0]["zz"] = json!(1)),
            false,
        ),
        (
            "clé en trop sur le compteur",
            Box::new(|d| d["naming_rules"][0]["counter"]["zz"] = json!(1)),
            false,
        ),
        (
            "cible inconnue",
            Box::new(|d| d["naming_rules"][0]["target"] = json!("point")),
            false,
        ),
        (
            "pas 0",
            Box::new(|d| d["naming_rules"][0]["counter"]["step"] = json!(0)),
            false,
        ),
        (
            "pas 1001",
            Box::new(|d| d["naming_rules"][0]["counter"]["step"] = json!(1001)),
            false,
        ),
        (
            "début 1000000",
            Box::new(|d| d["naming_rules"][0]["counter"]["start"] = json!(1_000_000)),
            false,
        ),
        (
            "largeur 10",
            Box::new(|d| d["naming_rules"][0]["counter"]["width"] = json!(10)),
            false,
        ),
        (
            "préfixe trop long",
            Box::new(|d| d["naming_rules"][0]["prefix"] = json!("P".repeat(41))),
            false,
        ),
        (
            "séparateur trop long",
            Box::new(|d| d["naming_rules"][0]["separator"] = json!("-----")),
            false,
        ),
        (
            "règles en texte",
            Box::new(|d| d["naming_rules"] = json!("oui")),
            false,
        ),
        (
            "nom dupliqué (casse)",
            Box::new(|d| d["naming_rules"][1]["name"] = json!("pièces")),
            true,
        ),
        (
            "même cible deux fois",
            Box::new(|d| d["naming_rules"][1]["target"] = json!("piece")),
            true,
        ),
        (
            "préfixe avec espace de bord",
            Box::new(|d| d["naming_rules"][0]["prefix"] = json!("P ")),
            true,
        ),
    ];
    for (name, mutate, schema_accepts) in cases {
        let mut doc = good.clone();
        mutate(&mut doc);
        assert_ne!(doc, good, "{name} : la mutation n'a rien changé");
        assert!(!loader_ok(&doc), "{name} : le chargeur accepte");
        assert_eq!(
            schema_ok(&schema, &doc),
            schema_accepts,
            "{name} : schéma {:?}",
            validateur::validate(&schema, &doc)
        );
        let run = cli(&[&write_tmp("nommage-ko.pforge.json", &doc.to_string())]);
        assert_eq!(run.code, 1, "{name} : {}", run.stdout);
    }
}
