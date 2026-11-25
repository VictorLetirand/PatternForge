//! US-02.8 — les lignes d'aide ne sont jamais exportées par défaut ; une
//! option explicite les inclut, rognées autour du contenu.

use pf_core::{Command, PathRole, PointRef, Project, SheetId};
use pf_export::{pdf, svg, ExportError, PathGeom, Scene, SceneOptions, GUIDE_MARGIN_MM};
use pf_geometry::PointMm;

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

/// Segment 0→100 sur l'axe x, plus une horizontale (y = 5), une verticale
/// (x = 30) et une oblique dans un calque que l'on peut masquer.
fn project() -> (Project, SheetId, pf_core::LayerId) {
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
            name: "Aides".into(),
        })
        .unwrap()
        .layers[0];
    for (x, y, a, l) in [
        (0.0, 5.0, 0.0, None),
        (30.0, 0.0, 90.0, None),
        (0.0, 0.0, 45.0, Some(layer)),
    ] {
        p.apply(Command::AddGuide {
            sheet: s,
            x_mm: x,
            y_mm: y,
            angle_deg: a,
            axis: false,
            layer: l,
        })
        .unwrap();
    }
    (p, s, layer)
}

fn lines(scene: &Scene) -> Vec<(PointMm, PointMm)> {
    scene
        .paths
        .iter()
        .filter_map(|p| match p.geom {
            PathGeom::Line(a, b) => Some((a, b)),
            _ => None,
        })
        .collect()
}

#[test]
fn par_defaut_aucun_guide_dans_la_scene_ni_dans_les_fichiers() {
    let (p, s, _) = project();
    let scene = Scene::from_sheet(&p, s).unwrap();
    assert_eq!(scene.paths.len(), 1, "seul le segment est exporté");
    // Octet pour octet identique à la même feuille sans aucun guide.
    let mut bare = p.clone();
    bare.sheets[0].guides.clear();
    let bare_scene = Scene::from_sheet(&bare, s).unwrap();
    assert_eq!(scene, bare_scene);
    assert_eq!(
        svg::export_svg(&scene).unwrap(),
        svg::export_svg(&bare_scene).unwrap()
    );
    assert_eq!(
        pdf::export_pdf(&[scene]).unwrap().bytes,
        pdf::export_pdf(&[bare_scene]).unwrap().bytes
    );
    // Options par défaut explicites : même chose.
    let explicit = Scene::from_sheet_with(&p, s, SceneOptions::default()).unwrap();
    assert_eq!(explicit.paths.len(), 1);
}

#[test]
fn option_explicite_inclut_les_guides_rognes_autour_du_contenu() {
    let (p, s, _) = project();
    let options = SceneOptions {
        include_guides: true,
        ..SceneOptions::default()
    };
    let scene = Scene::from_sheet_with(&p, s, options).unwrap();
    // Segment + 3 guides.
    assert_eq!(scene.paths.len(), 4);
    let base = Scene::from_sheet(&p, s).unwrap();
    let bb = base.bbox().unwrap();
    let m = GUIDE_MARGIN_MM;
    let l = lines(&scene);
    // Horizontale y = 5 : de x = -10 à x = 110 (emprise 0..100 + marge).
    assert!(l.contains(&(pt(bb.min.x - m, 5.0), pt(bb.max.x + m, 5.0))));
    // Verticale x = 30 : de y = -10 à y = 10 (emprise 0..0 + marge).
    assert!(l.contains(&(pt(30.0, bb.min.y - m), pt(30.0, bb.max.y + m))));
    // Rendus avec un style d'axe, distinct d'un tracé de coupe.
    let guides: Vec<_> = scene
        .paths
        .iter()
        .filter(|p| p.role == PathRole::AnnotationAxis)
        .collect();
    assert_eq!(guides.len(), 3);
    // Les deux formats se produisent sans erreur avec les guides inclus.
    assert!(svg::export_svg(&scene).is_ok());
    assert!(pdf::export_pdf(&[scene]).is_ok());
}

#[test]
fn guides_d_un_calque_masque_exclus_sauf_option_calques_masques() {
    let (mut p, s, layer) = project();
    p.apply(Command::SetLayerVisible {
        sheet: s,
        layer,
        visible: false,
    })
    .unwrap();
    let only_guides = SceneOptions {
        include_guides: true,
        ..SceneOptions::default()
    };
    let scene = Scene::from_sheet_with(&p, s, only_guides).unwrap();
    assert_eq!(scene.paths.len(), 3, "segment + 2 guides visibles");
    let all = SceneOptions {
        include_guides: true,
        include_hidden_layers: true,
        ..SceneOptions::default()
    };
    assert_eq!(Scene::from_sheet_with(&p, s, all).unwrap().paths.len(), 4);
}

#[test]
fn des_guides_seuls_ne_rendent_pas_une_feuille_exportable() {
    let mut p = Project::new("vide");
    let s = p.sheets[0].id;
    p.apply(Command::AddGuide {
        sheet: s,
        x_mm: 0.0,
        y_mm: 0.0,
        angle_deg: 0.0,
        axis: false,
        layer: None,
    })
    .unwrap();
    let options = SceneOptions {
        include_guides: true,
        ..SceneOptions::default()
    };
    assert!(matches!(
        Scene::from_sheet_with(&p, s, options),
        Err(ExportError::EmptySheet(_))
    ));
}

#[test]
fn guide_hors_de_l_emprise_n_est_pas_exporte() {
    let (mut p, s, _) = project();
    p.apply(Command::AddGuide {
        sheet: s,
        x_mm: 0.0,
        y_mm: 5000.0,
        angle_deg: 0.0,
        axis: false,
        layer: None,
    })
    .unwrap();
    let options = SceneOptions {
        include_guides: true,
        ..SceneOptions::default()
    };
    let scene = Scene::from_sheet_with(&p, s, options).unwrap();
    assert!(!lines(&scene).iter().any(|(a, _)| a.y == 5000.0));
}
