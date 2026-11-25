//! Durcissement 1, A5 : une valeur non finie dans **n'importe quelle** catégorie d'une scène
//! fait refuser chaque écrivain (SVG, PDF, PNG, JPEG, HPGL, découpage en pages) par une erreur
//! nommée, sans aucun octet produit. Sur le code d'avant la correction, SVG et PDF écrivaient
//! « NaN » ou « inf » dans le fichier.

use pf_core::{ButtonShape, GrainSense, MarkingShape, PathRole, StitchHole, ZoneKind};
use pf_export::pdf::{export_pdf, plan_tiles};
use pf_export::*;
use pf_geometry::{Arc, CircleTrace, CubicBezier, PointMm, VectorMm};
use std::panic::{catch_unwind, AssertUnwindSafe};

fn p(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

fn base() -> Scene {
    Scene::calibration_square()
}

fn cubic(v: f64) -> CubicBezier {
    CubicBezier::new(p(0.0, 0.0), p(v, 10.0), p(20.0, 10.0), p(30.0, 0.0))
}

fn line_path(geom: PathGeom) -> ScenePath {
    ScenePath {
        geom,
        role: PathRole::SewingContour,
        sense: GrainSense::Unmarked,
    }
}

/// Chaque cas pose la valeur `v` à un endroit de la scène.
#[allow(clippy::type_complexity)]
fn cases() -> Vec<(&'static str, Box<dyn Fn(&mut Scene, f64)>)> {
    let mut out: Vec<(&'static str, Box<dyn Fn(&mut Scene, f64)>)> = Vec::new();
    macro_rules! case {
        ($name:literal, $s:ident, $v:ident, $body:expr) => {
            out.push(($name, Box::new(|$s: &mut Scene, $v: f64| $body)));
        };
    }
    case!("segment x", s, v, {
        s.paths
            .push(line_path(PathGeom::Line(p(v, 0.0), p(1.0, 1.0))))
    });
    case!("segment y", s, v, {
        s.paths
            .push(line_path(PathGeom::Line(p(0.0, 0.0), p(1.0, v))))
    });
    case!("bézier", s, v, {
        s.paths.push(line_path(PathGeom::Cubic(cubic(v))))
    });
    case!("courbe lisse", s, v, {
        s.paths
            .push(line_path(PathGeom::Spline(vec![cubic(1.0), cubic(v)])))
    });
    case!("arc rayon", s, v, {
        s.paths.push(line_path(PathGeom::Arc(Arc {
            center: p(0.0, 0.0),
            radius_mm: v,
            start_rad: 0.0,
            sweep_rad: 1.0,
        })))
    });
    case!("cercle départ", s, v, {
        let mut c = CircleTrace::new(p(5.0, 5.0), 3.0, 0.0, true).unwrap();
        c.start_deg = v;
        s.paths.push(line_path(PathGeom::Circle(c)))
    });
    case!("point", s, v, {
        s.points.push(ScenePoint {
            pos: p(v, 0.0),
            label: "A".into(),
            show_label: true,
        })
    });
    case!("cran", s, v, s.notch_marks.push((p(0.0, 0.0), p(0.0, v))));
    case!("cotation", s, v, {
        s.dimensions.push(SceneDimension {
            from: p(0.0, 0.0),
            to: p(10.0, 0.0),
            from_offset: p(0.0, v),
            to_offset: p(10.0, 5.0),
            text: "10".into(),
        })
    });
    case!("cote complète", s, v, {
        s.full_dimensions.push(SceneFullDimension {
            kind: "angle",
            value: 1.0,
            marks: vec![(p(0.0, 0.0), p(10.0, 0.0))],
            text_center: p(5.0, 5.0),
            text_angle_deg: v,
            text: "1".into(),
        })
    });
    case!("marquage position", s, v, {
        s.markings.push(SceneMarking {
            pos: p(v, 0.0),
            shape: MarkingShape::Punch,
        })
    });
    case!("marquage diamètre", s, v, {
        s.markings.push(SceneMarking {
            pos: p(1.0, 1.0),
            shape: MarkingShape::Drill { diameter_mm: v },
        })
    });
    case!("trou de piqûre", s, v, {
        s.stitch_holes.push(SceneStitchHole {
            pos: p(1.0, 1.0),
            hole: StitchHole::Round { diameter_mm: 1.0 },
            tangent_deg: v,
        })
    });
    case!("zone", s, v, {
        s.zones.push(SceneZone {
            kind: ZoneKind::Glue,
            polygon: vec![p(0.0, 0.0), p(20.0, 0.0), p(20.0, v)],
            width_mm: None,
        })
    });
    case!("bouton", s, v, {
        s.buttons.push(SceneButton {
            pos: p(1.0, 1.0),
            shape: ButtonShape::Button { diameter_mm: v },
        })
    });
    case!("contour de coupe", s, v, {
        s.cut_contours.push(SceneCutContour {
            piece: "P".into(),
            edges: vec![PathGeom::Line(p(0.0, 0.0), p(v, 0.0))],
            closed: false,
            notch_marks: Vec::new(),
            cut_notch_marks: Vec::new(),
        })
    });
    case!("cran de rempli", s, v, {
        s.cut_contours.push(SceneCutContour {
            piece: "P".into(),
            edges: vec![PathGeom::Line(p(0.0, 0.0), p(1.0, 0.0))],
            closed: false,
            notch_marks: vec![(p(0.0, 0.0), p(0.0, v))],
            cut_notch_marks: Vec::new(),
        })
    });
    case!("pince", s, v, {
        s.darts.push(SceneDart {
            piece: "P".into(),
            apex: p(0.0, v),
            left_foot: p(0.0, 0.0),
            right_foot: p(5.0, 0.0),
        })
    });
    case!("étiquette de pièce", s, v, {
        s.piece_labels.push(ScenePieceLabel {
            piece: "P".into(),
            center: p(v, 0.0),
            lines: vec!["Devant".into()],
        })
    });
    case!("pliure", s, v, {
        s.folds.push(SceneFold {
            piece: "P".into(),
            from: p(0.0, 0.0),
            to: p(0.0, v),
            label: "SUR LE PLI".into(),
        })
    });
    case!("rempli", s, v, {
        s.fold_overs.push(SceneFoldOver {
            piece: "P".into(),
            at: p(1.0, 1.0),
            inward: VectorMm::new(v, 0.0),
            label: "Rempli".into(),
        })
    });
    out
}

/// Nom du refus rendu par chaque écrivain (`None` = des octets ont été produits).
fn writers(scene: &Scene) -> Vec<(&'static str, Option<String>)> {
    fn run<T, E: std::fmt::Debug>(f: impl FnOnce() -> Result<T, E>) -> Option<String> {
        match catch_unwind(AssertUnwindSafe(f)) {
            Ok(Ok(_)) => None,
            Ok(Err(e)) => Some(format!("{e:?}")),
            Err(_) => Some("panique".into()),
        }
    }
    let page = PageSetup::default();
    vec![
        ("svg", run(|| svg::export_svg(scene))),
        ("pdf", run(|| export_pdf(std::slice::from_ref(scene)))),
        ("pages", run(|| plan_tiles(scene))),
        ("png", run(|| raster::export_png(scene, 72))),
        ("jpeg", run(|| raster::export_jpeg(scene, 72))),
        ("hpgl", run(|| hpgl::export_hpgl(scene, &page))),
        (
            "hpgl tuilé",
            run(|| hpgl::export_hpgl_pages(scene, &page, hpgl::HpglOptions::default())),
        ),
    ]
}

#[test]
fn a5_la_scene_de_base_s_exporte() {
    for (writer, refusal) in writers(&base()) {
        assert_eq!(refusal, None, "{writer}");
    }
}

/// Valeurs finies dans chaque cas : les mêmes scènes s'exportent (le refus vient bien de la
/// valeur, pas de la forme du cas).
#[test]
fn a5_les_memes_scenes_finies_s_exportent() {
    for (name, set) in cases() {
        let mut s = base();
        set(&mut s, 2.0);
        assert_eq!(svg::export_svg(&s).err(), None, "{name}");
        assert_eq!(export_pdf(&[s]).err(), None, "{name}");
    }
}

#[test]
fn a5_valeur_non_finie_refusee_par_chaque_ecrivain() {
    let mut accepted = Vec::new();
    for (name, set) in cases() {
        for v in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let mut s = base();
            set(&mut s, v);
            for (writer, refusal) in writers(&s) {
                // HPGL a son propre type d'erreur : `NotFinite`, ou `Tiling(NotFinite)`.
                if !refusal.as_deref().is_some_and(|r| r.contains("NotFinite")) {
                    accepted.push(format!(
                        "{name} = {v} : {writer} -> {}",
                        refusal.map_or("écrit".to_string(), |r| r.chars().take(60).collect())
                    ));
                }
            }
        }
    }
    assert!(
        accepted.is_empty(),
        "{} cas sans refus nommé :\n{}",
        accepted.len(),
        accepted.join("\n")
    );
}
