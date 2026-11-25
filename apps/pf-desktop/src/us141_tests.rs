//! US-14.1 / US-14.2 : image de fond calibrée et décalque assisté, tests egui FR et EN et tests de
//! l'éditeur (accrochage aux contours, lissage, une seule étape annulable). Valeurs écrites à la
//! main ; images **générées par le code** dans un dossier temporaire propre au fil d'exécution.

use crate::app::App;
use crate::dock::ToolWin;
use crate::editor::{Editor, Tool};
use crate::Launch;
use eframe::egui;
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use pf_core::{CalibrationSpec, Command, Document, PlanePoint, Project};
use pf_geometry::PointMm;
use std::path::{Path, PathBuf};

fn harness() -> Harness<'static, App> {
    let app = App::with_context(&egui::Context::default(), Launch::default());
    let mut h = Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .build_state(|ctx, app: &mut App| app.ui(ctx), app);
    h.run();
    h
}

fn tmp(name: &str) -> PathBuf {
    let thread = format!("{:?}", std::thread::current().id()).replace(['(', ')'], "");
    let d = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target")
        .join(format!(
            "us141-ui-{name}-{}-{thread}",
            pf_i18n::lang().code()
        ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d.canonicalize().unwrap()
}

/// Image 200 × 100 : rectangle blanc `[50 ; 150[ × [20 ; 80[` sur fond noir.
fn write_rect_image(path: &Path) {
    let mut img = image::GrayImage::new(200, 100);
    for (x, y, p) in img.enumerate_pixels_mut() {
        let inside = (50..150).contains(&x) && (20..80).contains(&y);
        *p = image::Luma([if inside { 255 } else { 0 }]);
    }
    img.save(path).unwrap();
}

fn saved_editor(dir: &Path) -> (Editor, PathBuf) {
    let path = dir.join("jupe.pforge.json");
    let mut doc = Document::new(Project::new("Jupe"));
    doc.save_as(&path).unwrap();
    (Editor::new(doc), path)
}

fn pp(x: f64, y: f64) -> PlanePoint {
    PlanePoint { x, y }
}

/// Pose le fond (échelle 1 mm / px : (0 ; 0)-(100 ; 0) px = 100 mm).
fn set_background(ed: &mut Editor, project_file: &Path, image: &Path) {
    let img = pf_core::background_image::import_image(project_file, image).unwrap();
    let sheet = ed.sheet;
    ed.exec(Command::SetBackground {
        sheet,
        image: img.relative,
        sha256: img.sha256,
        width_px: img.width_px,
        height_px: img.height_px,
        calibration: CalibrationSpec::TwoPoints {
            points_px: [pp(0.0, 0.0), pp(100.0, 0.0)],
            known_distance_mm: 100.0,
        },
    })
    .expect("pose du fond");
}

fn wait_edges(ed: &mut Editor) {
    for _ in 0..500 {
        ed.trace.poll_edges();
        if !ed.trace.computing() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("contours non calculés en 5 s");
}

// ----- éditeur : contours, accrochage, lissage ---------------------------------------------------

#[test]
fn accrochage_aux_contours_valeur_a_la_main_et_repli() {
    let dir = tmp("accrochage");
    let (mut ed, file) = saved_editor(&dir);
    let src = dir.join("rect.png");
    write_rect_image(&src);
    set_background(&mut ed, &file, &src);
    let bg = ed
        .project()
        .sheet(ed.sheet)
        .unwrap()
        .background
        .clone()
        .unwrap();
    ed.trace.start_edges(dir.clone(), bg);
    assert!(ed.trace.computing());
    wait_edges(&mut ed);
    assert!(ed.trace.edges.is_some() && ed.trace.error.is_none());
    ed.trace.snap_edges = true;
    ed.trace.threshold = 64;
    // Pixel (x, y) = (49,5 ; 50,5) -> mm (49,5 ; -50,5) (échelle 1, y du plan vers le haut).
    // Clic à 2,5 mm à gauche du bord x = 50 px : contour le plus proche = colonne 49 (49,5), rayon
    // de recherche 2 x tol = 6 mm.
    let near = ed.trace_edge_near(PointMm::new(47.0, -50.5), 3.0).unwrap();
    assert!(
        (near.x - 49.5).abs() < 1e-9 && (near.y + 50.5).abs() < 1e-9,
        "{near:?}"
    );
    // Rien dans le rayon : pas d'accrochage ; seuil au maximum 255 : le bord (255) accroche
    // encore, un seuil de 256 n'existe pas (borné à 255 par l'interface).
    assert_eq!(ed.trace_edge_near(PointMm::new(20.0, -50.5), 3.0), None);
    // Clic de l'outil : point sur le contour si proche, sinon position ordinaire (grille).
    ed.set_tool(Tool::Trace);
    ed.trace_click(PointMm::new(47.0, -50.5), 3.0);
    assert_eq!(ed.trace.points, vec![near]);
    ed.trace_click(PointMm::new(20.0, -50.5), 3.0);
    assert_eq!(ed.trace.points.len(), 2);
    let free = ed.trace.points[1];
    assert!(
        (free.x - 20.0).abs() < 1.0 && (free.y + 50.5).abs() < 1.0,
        "{free:?}"
    );
    // Accrochage désactivé : aucun contour.
    ed.trace.snap_edges = false;
    assert_eq!(ed.trace_edge_near(PointMm::new(47.0, -50.5), 3.0), None);
    // Un point confondu avec le précédent est refusé (message), rien n'est ajouté.
    ed.trace_click(free, 3.0);
    assert_eq!(ed.trace.points.len(), 2);
    assert!(ed.status.error);
    // Aucune écriture dans le projet : le décalque n'est pas encore appliqué.
    assert!(ed.project().sheet(ed.sheet).unwrap().entities.is_empty());
}

#[test]
fn annuler_le_calcul_de_contours_jette_le_resultat() {
    let dir = tmp("annulation");
    let (mut ed, file) = saved_editor(&dir);
    let src = dir.join("rect.png");
    write_rect_image(&src);
    set_background(&mut ed, &file, &src);
    let bg = ed
        .project()
        .sheet(ed.sheet)
        .unwrap()
        .background
        .clone()
        .unwrap();
    ed.trace.start_edges(dir.clone(), bg);
    ed.trace.cancel_edges();
    assert!(!ed.trace.computing());
    std::thread::sleep(std::time::Duration::from_millis(100));
    assert!(!ed.trace.poll_edges());
    assert!(ed.trace.edges.is_none());
    // Image modifiée : le calcul échoue proprement (empreinte), aucune carte.
    let bg = ed
        .project()
        .sheet(ed.sheet)
        .unwrap()
        .background
        .clone()
        .unwrap();
    let copy = dir.join(&bg.image);
    std::fs::write(&copy, b"modifie").unwrap();
    ed.trace.start_edges(dir.clone(), bg);
    wait_edges(&mut ed);
    assert!(ed.trace.edges.is_none());
    assert!(ed
        .trace
        .error
        .as_deref()
        .is_some_and(|e| e.contains("modifiée")));
}

#[test]
fn appliquer_le_decalque_est_une_etape_et_une_geometrie_ordinaire() {
    let mut ed = Editor::new(Document::new(Project::new("Jupe")));
    // Quart de cercle de rayon 100 mm échantillonné tous les 3° (31 points).
    ed.trace.points = (0..=30)
        .map(|i| {
            let a = f64::from(i) * 3.0_f64.to_radians();
            PointMm::new(100.0 * a.cos(), 100.0 * a.sin())
        })
        .collect();
    ed.trace.tolerance_text = "0.2".into();
    let preview = ed.trace_fit().unwrap();
    assert!(preview.max_error_mm <= 0.2, "{}", preview.max_error_mm);
    assert!(!preview.curves.is_empty() && preview.curves.len() <= 3);
    let out = ed.trace_apply(true).expect("application");
    // Aperçu = résultat : autant de tracés que de Béziers, une seule étape annulable.
    assert_eq!(out.entities.len(), preview.curves.len());
    assert!(ed.trace.points.is_empty());
    assert!(ed.doc.can_undo());
    ed.undo();
    assert!(ed.project().sheet(ed.sheet).unwrap().entities.is_empty());
    assert!(!ed.doc.can_undo());
    ed.redo();
    assert_eq!(
        ed.project().sheet(ed.sheet).unwrap().entities.len(),
        preview.curves.len()
    );
    // Moins de deux points ou tolérance illisible : refus sans trace.
    ed.trace.points = vec![PointMm::new(0.0, 0.0)];
    assert!(ed.trace_apply(true).is_none());
    ed.trace.points = vec![PointMm::new(0.0, 0.0), PointMm::new(10.0, 0.0)];
    ed.trace.tolerance_text = "abc".into();
    let before = ed.project().clone();
    assert!(ed.trace_apply(true).is_none());
    assert_eq!(*ed.project(), before);
    // Segments droits : n - 1 tracés.
    assert!(ed.trace_apply(false).is_some());
    assert_eq!(
        ed.project().sheet(ed.sheet).unwrap().entities.len(),
        preview.curves.len() + 1
    );
}

#[test]
fn l_outil_decalque_n_a_pas_de_raccourci_par_defaut_mais_est_dans_la_famille_tracer() {
    assert_eq!(crate::shortcuts::current().label(Tool::Trace), "");
    assert!(crate::panels::ToolFamily::Draw
        .tools()
        .contains(&Tool::Trace));
    assert!(!crate::help::tool_gesture(Tool::Trace).is_empty());
}

// ----- interface : parcours FR et EN -----------------------------------------------------------

struct Labels {
    view_menu: &'static str,
    menu_item: &'static str,
    import: &'static str,
    place: &'static str,
    residual: &'static str,
    remove: &'static str,
    apply_smooth: &'static str,
    computing_or_compute: &'static str,
    unsaved_hint: &'static str,
}

fn walk(l: Labels) {
    // Projet jamais enregistré : refus propre de l'import (rien n'est copié).
    let mut h = harness();
    h.get_by_label(l.view_menu).click_accesskit();
    h.run_steps(3);
    h.get_by_label(l.menu_item).click_accesskit();
    h.run_steps(4);
    let dir = tmp("ui");
    let src = dir.join("photo.png");
    write_rect_image(&src);
    h.state_mut().background.as_mut().unwrap().path_text = src.display().to_string();
    h.get_by_label(l.import).click_accesskit();
    h.run_steps(4);
    assert!(h.state().background.as_ref().unwrap().pending.is_none());
    h.get_by_label_contains(l.unsaved_hint);

    // Projet enregistré : import, calibration à deux points, résidu affiché.
    let (ed, file) = saved_editor(&dir);
    h.state_mut().editor = ed;
    h.state_mut().fit_pending = true;
    h.run_steps(4);
    h.get_by_label(l.import).click_accesskit();
    h.run_steps(5);
    let win = h
        .ctx
        .memory(|m| m.area_rect(ToolWin::Background.id()))
        .expect("fenêtre Image de fond");
    assert!(
        h.state().dock.rect.contains_rect(win),
        "{win:?} hors du dock"
    );
    assert!(!win.intersects(h.state().canvas_rect));
    {
        let d = h.state_mut().background.as_mut().unwrap();
        let p = d.pending.as_mut().expect("aperçu de calibration");
        p.points = vec![pp(0.0, 0.0), pp(100.0, 0.0)];
        d.dist_text = "100".into();
    }
    h.run_steps(2);
    h.get_by_label(l.place).click_accesskit();
    h.run_steps(5);
    let bg = h
        .state()
        .editor
        .project()
        .sheet(h.state().editor.sheet)
        .unwrap()
        .background
        .clone()
        .expect("fond posé");
    assert!(bg.image.starts_with("jupe.assets/"));
    assert!(dir.join(&bg.image).exists());
    h.get_by_label_contains(l.residual);
    h.get_by_label_contains(l.computing_or_compute);
    // Une seule étape pour la pose ; le fond n'est pas dans le JSON du projet autrement que par référence.
    let json = pf_core::persist::to_json(h.state().editor.project()).unwrap();
    assert!(json.contains("\"two_points\"") && json.len() < 3000);
    assert!(file.exists());

    // Décalque : trois points puis « Appliquer lissé » = une étape annulable, géométrie ordinaire.
    h.state_mut().editor.set_tool(Tool::Trace);
    {
        let ed = &mut h.state_mut().editor;
        for (x, y) in [(10.0, -10.0), (30.0, -20.0), (50.0, -22.0), (70.0, -15.0)] {
            ed.trace_click(PointMm::new(x, y), 0.5);
        }
        assert_eq!(ed.trace.points.len(), 4);
    }
    h.run_steps(3);
    let before = h
        .state()
        .editor
        .project()
        .sheet(h.state().editor.sheet)
        .unwrap()
        .entities
        .len();
    h.get_by_label(l.apply_smooth).click_accesskit();
    h.run_steps(4);
    let after = h
        .state()
        .editor
        .project()
        .sheet(h.state().editor.sheet)
        .unwrap()
        .entities
        .len();
    assert!(after > before, "{before} -> {after}");
    h.state_mut().editor.undo();
    assert_eq!(
        h.state()
            .editor
            .project()
            .sheet(h.state().editor.sheet)
            .unwrap()
            .entities
            .len(),
        before
    );

    // Retrait du fond : le fichier image reste, la référence disparaît.
    h.get_by_label(l.remove).click_accesskit();
    h.run_steps(4);
    assert!(h
        .state()
        .editor
        .project()
        .sheet(h.state().editor.sheet)
        .unwrap()
        .background
        .is_none());
    assert!(dir.join(&bg.image).exists());
}

#[test]
fn fenetre_image_de_fond_en_francais() {
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
    walk(Labels {
        view_menu: "Affichage",
        menu_item: "Image de fond (photo, scan)…",
        import: "Importer",
        place: "Poser le fond",
        residual: "Résidu observé (RMS)",
        remove: "Retirer le fond",
        apply_smooth: "Appliquer lissé (courbes)",
        computing_or_compute: "Calculer les contours",
        unsaved_hint: "Enregistrez d'abord le projet",
    });
}

#[test]
fn fenetre_image_de_fond_en_anglais() {
    pf_i18n::set_lang(pf_i18n::Lang::En);
    walk(Labels {
        view_menu: "View",
        menu_item: "Background image (photo, scan)…",
        import: "Import",
        place: "Place the background",
        residual: "Observed residual (RMS)",
        remove: "Remove the background",
        apply_smooth: "Apply smoothed (curves)",
        computing_or_compute: "Compute edges",
        unsaved_hint: "Save the project first",
    });
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
}
