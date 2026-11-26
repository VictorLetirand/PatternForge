//! US-17.2 (fenêtre « Versions ») et US-17.4 (ouverture en fil séparé) : tests egui FR et EN.
//! Valeurs écrites à la main : P1 (10 ; 20) déplacé en (13 ; 24) = 5,000 mm.

use crate::app::App;
use crate::dock::ToolWin;
use crate::editor::Editor;
use crate::Launch;
use eframe::egui;
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use pf_core::versions;
use pf_core::{Command, Document, PointId, PointRef, Project};
use pf_geometry::PointMm;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

fn harness() -> Harness<'static, App> {
    let app = App::with_context(&egui::Context::default(), Launch::default());
    let mut h = Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .build_state(|ctx, app: &mut App| app.ui(ctx), app);
    h.run();
    h
}

fn tmp(name: &str) -> PathBuf {
    let d = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target")
        .join(format!("us172-ui-{name}-{}", pf_i18n::lang().code()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d.canonicalize().unwrap()
}

/// Projet « Jupe » : P1 (10 ; 20), P2 (100 ; 0), un segment P1-P2 ; enregistré dans `dir`.
fn saved_project(dir: &std::path::Path) -> (Document, PointId, PathBuf) {
    let mut p = Project::new("Jupe");
    let s = p.sheets[0].id;
    let p1 = p
        .apply(Command::AddPoint {
            sheet: s,
            at: PointMm::new(10.0, 20.0),
        })
        .unwrap()
        .points[0];
    p.apply(Command::AddSegment {
        sheet: s,
        from: PointRef::Existing(p1),
        to: PointRef::New(PointMm::new(100.0, 0.0)),
    })
    .unwrap();
    let path = dir.join("jupe.pforge.json");
    let mut doc = Document::new(p);
    doc.save_as(&path).unwrap();
    (doc, p1, path)
}

struct Labels {
    file_menu: &'static str,
    menu_item: &'static str,
    create: &'static str,
    compare: &'static str,
    restore: &'static str,
    delete: &'static str,
    delete_yes: &'static str,
    size_unit: &'static str,
    points_line: &'static str,
    move_line: &'static str,
    legend: &'static str,
    before_restore: &'static str,
    err_char: &'static str,
}

fn walk(l: Labels) {
    let dir = tmp("versions");
    let (doc, p1, path) = saved_project(&dir);
    let v1_project = doc.project().clone();
    let mut h = harness();
    h.state_mut().editor = Editor::new(doc);
    h.state_mut().fit_pending = true;
    h.run_steps(4);

    h.get_by_label(l.file_menu).click_accesskit();
    h.run_steps(3);
    h.get_by_label(l.menu_item).click_accesskit();
    h.run_steps(4);
    h.get_by_label(l.create);
    let win = h
        .ctx
        .memory(|m| m.area_rect(ToolWin::Versions.id()))
        .expect("fenêtre Versions");
    assert!(
        h.state().dock.rect.contains_rect(win),
        "{win:?} hors du dock"
    );
    assert!(!win.intersects(h.state().canvas_rect));

    // Création par le bouton : nom saisi, fichier voisin créé, projet inchangé.
    let before_bytes = std::fs::read(&path).unwrap();
    h.state_mut().versions.as_mut().unwrap().name = "Etat A".into();
    h.get_by_label(l.create).click_accesskit();
    h.run_steps(4);
    let vdir = versions::versions_dir(&path);
    let files: Vec<String> = std::fs::read_dir(&vdir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with("-Etat-A.pforge"))
        .collect();
    assert_eq!(files.len(), 1, "{files:?}");
    assert_eq!(std::fs::read(&path).unwrap(), before_bytes);
    assert_eq!(h.get_all_by_label_contains(l.size_unit).count(), 1);

    // Nom interdit : refus affiché, aucune version de plus.
    h.state_mut().versions.as_mut().unwrap().name = "a/b".into();
    h.get_by_label(l.create).click_accesskit();
    h.run_steps(4);
    h.get_by_label_contains(l.err_char);
    assert_eq!(versions::list(&path).unwrap().versions.len(), 1);

    // Modification du projet : P1 (10 ; 20) -> (13 ; 24), puis comparaison avec l'état courant.
    let s = h.state().editor.project().sheets[0].id;
    h.state_mut().editor.exec(Command::MovePoint {
        sheet: s,
        point: p1,
        to: PointMm::new(13.0, 24.0),
    });
    let modified = h.state().editor.project().clone();
    assert_ne!(modified, v1_project);
    h.run_steps(3);
    h.get_all_by_label_contains(l.size_unit)
        .next()
        .unwrap()
        .click_accesskit();
    h.run_steps(3);
    h.get_by_label(l.compare).click_accesskit();
    h.run_steps(4);
    h.get_by_label_contains(l.points_line);
    h.get_by_label_contains(l.move_line);
    h.get_by_label_contains(l.legend);

    // Restauration : l'état courant est instantané d'abord, puis UN pas annulable.
    h.get_by_label(l.restore).click_accesskit();
    h.run_steps(4);
    assert_eq!(h.state().editor.project(), &v1_project);
    let listed = versions::list(&path).unwrap().versions;
    assert_eq!(listed.len(), 2);
    assert!(listed.iter().any(|v| v.name == l.before_restore));
    let auto = listed.iter().find(|v| v.name == l.before_restore).unwrap();
    assert_eq!(versions::read(&path, &auto.file).unwrap(), modified);
    h.state_mut().editor.undo();
    assert_eq!(h.state().editor.project(), &modified, "un seul undo");

    // Suppression avec confirmation.
    h.run_steps(3);
    h.get_all_by_label_contains(l.size_unit)
        .next()
        .unwrap()
        .click_accesskit();
    h.run_steps(3);
    h.get_by_label(l.delete).click_accesskit();
    h.run_steps(3);
    assert_eq!(
        versions::list(&path).unwrap().versions.len(),
        2,
        "pas encore"
    );
    h.get_by_label(l.delete_yes).click_accesskit();
    h.run_steps(4);
    assert_eq!(versions::list(&path).unwrap().versions.len(), 1);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn us172_fr_fenetre_versions() {
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
    walk(Labels {
        file_menu: "Fichier",
        menu_item: "Versions…",
        create: "Créer une version",
        compare: "Comparer",
        restore: "Restaurer",
        delete: "Supprimer",
        delete_yes: "Supprimer définitivement",
        size_unit: " Ko",
        points_line: "points : 0 ajouté(s) (+), 0 supprimé(s) (−), 1 déplacé(s) (->)",
        move_line: "déplacé de 5,000 mm",
        legend: "trait tireté rouge avec × = supprimé",
        before_restore: "avant restauration",
        err_char: "Caractère interdit dans le nom : /",
    });
}

#[test]
fn us172_en_versions_window() {
    pf_i18n::set_lang(pf_i18n::Lang::En);
    walk(Labels {
        file_menu: "File",
        menu_item: "Versions…",
        create: "Create a version",
        compare: "Compare",
        restore: "Restore",
        delete: "Delete",
        delete_yes: "Delete permanently",
        size_unit: " KB",
        points_line: "points: 0 added (+), 0 removed (−), 1 moved (->)",
        move_line: "moved by 5.000 mm",
        legend: "dashed red line with × = removed",
        before_restore: "before restore",
        err_char: "Forbidden character in the name: /",
    });
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
}

#[test]
fn us172_projet_non_enregistre_demande_d_enregistrer() {
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
    let mut h = harness();
    h.state_mut().open_versions();
    h.run_steps(4);
    h.get_by_label(
        "Enregistrez d'abord le projet : les versions sont rangées à côté de son fichier.",
    );
    assert!(!h.state_mut().create_version("x"));
    assert!(h.state().versions.as_ref().unwrap().list.is_empty());
}

#[test]
fn us172_version_abimee_signalee_et_refusee_a_la_restauration() {
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
    let dir = tmp("abimee");
    let (doc, p1, path) = saved_project(&dir);
    let mut h = harness();
    h.state_mut().editor = Editor::new(doc);
    h.state_mut().open_versions();
    assert!(h.state_mut().create_version("bonne"));
    let s = h.state().editor.project().sheets[0].id;
    h.state_mut().editor.exec(Command::MovePoint {
        sheet: s,
        point: p1,
        to: PointMm::new(1.0, 1.0),
    });
    let file = versions::list(&path).unwrap().versions[0].file.clone();
    let f = versions::versions_dir(&path).join(&file);
    let bytes = std::fs::read(&f).unwrap();
    std::fs::write(&f, &bytes[..bytes.len() / 2]).unwrap();
    h.state_mut().versions_reload();
    h.run_steps(4);
    h.get_by_label_contains("(abîmée");
    let current = h.state().editor.project().clone();
    assert!(!h.state_mut().restore_version_file(&file));
    assert_eq!(h.state().editor.project(), &current, "rien de restauré");
    assert_eq!(
        versions::list(&path).unwrap().versions.len(),
        1,
        "pas d'instantané automatique sur un refus"
    );
    assert_eq!(std::fs::read(&f).unwrap().len(), bytes.len() / 2);
    let _ = std::fs::remove_dir_all(dir);
}

// ===== US-17.4 : ouverture en fil séparé ===========================================================

fn wait_job(h: &mut Harness<'_, App>) {
    for _ in 0..3000 {
        h.run_steps(1);
        if h.state().open_job.is_none() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    panic!("ouverture jamais terminée");
}

#[test]
fn us174_l_interface_repond_pendant_l_ouverture_et_l_annulation_est_sans_effet() {
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
    let dir = tmp("ouverture");
    let (_, _, big) = saved_project(&dir);
    let mut h = harness();
    let current = h.state().editor.project().clone();
    let hold = Arc::new(AtomicBool::new(true));
    h.state_mut().start_open_job(&big, None, Some(hold.clone()));
    let frame0 = h.state().frame;
    h.run_steps(30);
    // Le fil est retenu, et pourtant l'interface a dessiné 30 images et affiche la progression.
    assert!(h.state().open_job.is_some());
    assert!(h.state().frame >= frame0 + 30, "interface gelée");
    h.get_by_label("Ouverture du projet");
    h.get_by_label_contains("Lecture du fichier");
    // Le clic sur « Annuler » est traité pendant l'attente : document courant intact, fichier
    // jamais ouvert.
    h.get_by_label("Annuler l'ouverture").click_accesskit();
    h.run_steps(3);
    assert!(h.state().open_job.is_none());
    assert_eq!(h.state().editor.project(), &current);
    assert_eq!(h.state().editor.doc.path, None);
    assert!(h
        .state()
        .editor
        .status
        .text
        .starts_with("Ouverture annulée"));
    hold.store(false, Ordering::Relaxed);
    h.run_steps(5);
    assert_eq!(h.state().editor.doc.path, None, "résultat tardif jeté");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn us174_ouverture_en_fil_remplace_le_document_a_la_fin() {
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
    let dir = tmp("ouverture-ok");
    let (doc, _, path) = saved_project(&dir);
    let expected = doc.project().clone();
    let mut h = harness();
    // Seuil abaissé : la voie normale (`open_loaded`) choisit le fil pour ce petit fichier.
    h.state_mut().open_async_bytes = 1;
    h.state_mut().open_loaded(&path, None);
    assert!(h.state().open_job.is_some(), "ouverture lancée en fil");
    wait_job(&mut h);
    assert_eq!(h.state().editor.project(), &expected);
    assert_eq!(h.state().editor.doc.path.as_deref(), Some(path.as_path()));
    assert!(!h.state().editor.doc.is_dirty());
    // La révision lue par le fil protège l'enregistrement comme une ouverture normale.
    assert_eq!(
        h.state().editor.doc.external_change().unwrap(),
        pf_core::ExternalChange::Unchanged
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn us174_fichier_invalide_ouvert_en_fil_donne_une_erreur_sans_toucher_au_document() {
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
    let dir = tmp("ouverture-ko");
    let bad = dir.join("faux.pforge.json");
    std::fs::write(&bad, "{ \"format\": \"autre\" ").unwrap();
    let mut h = harness();
    let current = h.state().editor.project().clone();
    h.state_mut().open_async_bytes = 1;
    h.state_mut().open_loaded(&bad, None);
    wait_job(&mut h);
    assert!(h.state().error_dialog.is_some());
    assert_eq!(h.state().editor.project(), &current);
    assert_eq!(std::fs::read(&bad).unwrap(), b"{ \"format\": \"autre\" ");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn us174_sous_le_seuil_l_ouverture_reste_synchrone() {
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
    let dir = tmp("ouverture-sync");
    let (doc, _, path) = saved_project(&dir);
    let expected = doc.project().clone();
    let mut h = harness();
    h.state_mut().open_loaded(&path, None);
    assert!(h.state().open_job.is_none());
    assert_eq!(h.state().editor.project(), &expected);
    let _ = std::fs::remove_dir_all(dir);
}
