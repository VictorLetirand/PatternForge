//! US-01.2 CA2 — les calques masqués sont absents des exports par défaut.

use pf_core::{Command, NotchStyle, PointRef, Project, SheetId};
use pf_export::{pdf, svg, ExportError, Scene, SceneOptions};
use pf_geometry::{PointMm, Side};

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

/// Segment visible (0→100) et segment dans un calque masqué (y = 50, 40 mm),
/// avec cran et cotation sur ce dernier.
fn project() -> (Project, SheetId) {
    let mut p = Project::new("export");
    let s = p.sheets[0].id;
    p.apply(Command::AddSegment {
        sheet: s,
        from: PointRef::New(pt(0.0, 0.0)),
        to: PointRef::New(pt(100.0, 0.0)),
    })
    .unwrap();
    let layer = p
        .apply(Command::AddLayer {
            sheet: s,
            name: "Caché".into(),
        })
        .unwrap()
        .layers[0];
    let hidden = p
        .apply(Command::InLayer {
            sheet: s,
            layer,
            command: Box::new(Command::AddSegment {
                sheet: s,
                from: PointRef::New(pt(0.0, 50.0)),
                to: PointRef::New(pt(40.0, 50.0)),
            }),
        })
        .unwrap();
    p.apply(Command::AddNotch {
        sheet: s,
        entity: hidden.entities[0],
        distance_mm: 10.0,
        side: Side::Left,
        length_mm: 5.0,
        style: NotchStyle::Single,
    })
    .unwrap();
    p.apply(Command::AddDimension {
        sheet: s,
        from: hidden.points[0],
        to: hidden.points[1],
        offset_mm: 8.0,
    })
    .unwrap();
    p.apply(Command::SetLayerVisible {
        sheet: s,
        layer,
        visible: false,
    })
    .unwrap();
    (p, s)
}

#[test]
fn ca2_calque_masque_absent_de_la_scene_par_defaut() {
    let (p, s) = project();
    let scene = Scene::from_sheet(&p, s).unwrap();
    assert_eq!(scene.paths.len(), 1);
    assert_eq!(scene.points.len(), 2);
    assert!(scene.notch_marks.is_empty());
    assert!(scene.dimensions.is_empty());
    // Même résultat avec les options par défaut explicites.
    assert_eq!(
        scene,
        Scene::from_sheet_with(&p, s, SceneOptions::default()).unwrap()
    );
}

#[test]
fn ca2_option_inclure_les_calques_masques() {
    let (p, s) = project();
    let scene = Scene::from_sheet_with(
        &p,
        s,
        SceneOptions {
            include_hidden_layers: true,
            ..SceneOptions::default()
        },
    )
    .unwrap();
    assert_eq!(scene.paths.len(), 2);
    assert_eq!(scene.points.len(), 4);
    assert_eq!(scene.notch_marks.len(), 1);
    assert_eq!(scene.dimensions.len(), 1);
}

#[test]
fn ca2_svg_et_pdf_n_exportent_pas_le_calque_masque() {
    let (p, s) = project();
    let hidden = Scene::from_sheet(&p, s).unwrap();
    let all = Scene::from_sheet_with(
        &p,
        s,
        SceneOptions {
            include_hidden_layers: true,
            ..SceneOptions::default()
        },
    )
    .unwrap();
    let (svg_hidden, svg_all) = (
        svg::export_svg(&hidden).unwrap(),
        svg::export_svg(&all).unwrap(),
    );
    assert_ne!(svg_hidden, svg_all);
    assert!(svg_hidden.len() < svg_all.len());
    // Le segment masqué (y = 50 mm) est absent du SVG par défaut, présent avec l'option.
    let lines = |t: &str| t.matches("<line").count() + t.matches("<path").count();
    assert!(lines(&svg_hidden) < lines(&svg_all), "{svg_hidden}");
    let (pdf_hidden, pdf_all) = (
        pdf::export_pdf(&[hidden]).unwrap(),
        pdf::export_pdf(&[all]).unwrap(),
    );
    assert_ne!(pdf_hidden, pdf_all);
}

#[test]
fn ca2_feuille_dont_tout_est_masque_est_signalee_vide() {
    let (mut p, s) = project();
    let visible_one = p.sheets[0].entities[0].id;
    p.apply(Command::Delete {
        sheet: s,
        points: vec![],
        entities: vec![visible_one],
    })
    .unwrap();
    // On supprime le tracé visible : seul le tracé du calque masqué subsiste,
    // donc rien à exporter par défaut.
    assert!(matches!(
        Scene::from_sheet(&p, s),
        Err(ExportError::EmptySheet(_))
    ));
    assert!(Scene::from_sheet_with(
        &p,
        s,
        SceneOptions {
            include_hidden_layers: true,
            ..SceneOptions::default()
        }
    )
    .is_ok());
}
