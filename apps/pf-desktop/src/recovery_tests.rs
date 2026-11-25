//! US-17.1 : copie de secours périodique, boîte « Récupérer / Ignorer »,
//! suppression à l'enregistrement. Dossiers temporaires uniquement : le vrai
//! dossier de données utilisateur n'est jamais touché (`recovery_dir` injecté).

use crate::app::App;
use crate::Launch;
use eframe::egui::{self, Key, Modifiers};
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use pf_core::recovery::{autosave_path, write_autosave};
use pf_core::{persist, Command, Project};
use pf_i18n::tr;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!(
            "pf-desktop-recovery-{}-{tag}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dossier temporaire");
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn set_age(path: &Path, secs_ago: u64) {
    std::fs::File::options()
        .write(true)
        .open(path)
        .expect("ouverture")
        .set_modified(SystemTime::now() - Duration::from_secs(secs_ago))
        .expect("date");
}

fn harness_with(app: App) -> Harness<'static, App> {
    let mut h = Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .build_state(|ctx, app: &mut App| app.ui(ctx), app);
    h.run_steps(3);
    h
}

fn app_for(file: &Path) -> App {
    App::with_context(
        &egui::Context::default(),
        Launch {
            file: Some(file.to_path_buf()),
            ..Launch::default()
        },
    )
}

/// Projet principal enregistré (ancien) + copie de secours plus récente.
fn main_with_newer_autosave(t: &TempDir) -> PathBuf {
    let main = t.0.join("robe.pforge.json");
    persist::save(&main, &Project::new("Principal")).expect("principal");
    set_age(&main, 100);
    write_autosave(Some(&main), None, &Project::new("Copie de secours")).expect("copie");
    main
}

#[test]
fn intervalle_de_copie_defaut_60_et_zero_desactive() {
    use crate::app::autosave_interval as parse;
    use serde_json::json;
    assert_eq!(parse(None), Some(Duration::from_secs(60)));
    assert_eq!(parse(Some(&json!("x"))), Some(Duration::from_secs(60)));
    assert_eq!(parse(Some(&json!(15))), Some(Duration::from_secs(15)));
    assert_eq!(parse(Some(&json!(0))), None);
}

#[test]
fn boite_de_recuperation_recuperer_laisse_le_projet_modifie_et_le_principal_intact() {
    let t = TempDir::new("recuperer");
    let main = main_with_newer_autosave(&t);
    let before = std::fs::read(&main).expect("lecture");
    let mut h = harness_with(app_for(&main));
    h.get_by_label(&tr("recovery.title"));
    h.get_by_label(&tr("recovery.ignore"));
    h.get_by_label(&tr("recovery.recover")).click();
    h.run_steps(3);
    let app = h.state();
    assert!(app.recovery_prompt.is_none());
    assert_eq!(app.editor.project().name, "Copie de secours");
    assert!(app.editor.doc.is_dirty(), "a enregistrer pour conserver");
    assert_eq!(app.editor.doc.path.as_deref(), Some(main.as_path()));
    assert!(!app.editor.doc.can_undo(), "historique intact");
    assert_eq!(std::fs::read(&main).expect("lecture"), before);
}

#[test]
fn boite_de_recuperation_ignorer_supprime_la_copie() {
    let t = TempDir::new("ignorer");
    let main = main_with_newer_autosave(&t);
    let mut h = harness_with(app_for(&main));
    h.get_by_label(&tr("recovery.ignore")).click();
    h.run_steps(3);
    let app = h.state();
    assert!(app.recovery_prompt.is_none());
    assert_eq!(app.editor.project().name, "Principal");
    assert!(!app.editor.doc.is_dirty());
    assert!(!autosave_path(Some(&main), None).expect("chemin").exists());
}

#[test]
fn copie_corrompue_au_demarrage_message_et_pas_de_boite() {
    let t = TempDir::new("corrompue");
    let main = t.0.join("robe.pforge.json");
    persist::save(&main, &Project::new("Principal")).expect("principal");
    set_age(&main, 100);
    let auto = autosave_path(Some(&main), None).expect("chemin");
    std::fs::write(&auto, "{\"format\": ").expect("copie tronquee");
    let h = harness_with(app_for(&main));
    let app = h.state();
    assert!(app.recovery_prompt.is_none());
    assert!(app.error_dialog.is_some(), "message affiche");
    assert_eq!(app.editor.project().name, "Principal");
}

#[test]
fn projet_sans_chemin_reprise_depuis_le_dossier_de_donnees() {
    let t = TempDir::new("sanschemin");
    write_autosave(None, Some(&t.0), &Project::new("Jamais enregistre")).expect("copie");
    let mut app = App::with_context(&egui::Context::default(), Launch::default());
    app.recovery_dir = Some(t.0.clone());
    app.check_recovery();
    let mut h = harness_with(app);
    h.get_by_label(&tr("recovery.recover")).click();
    h.run_steps(3);
    let app = h.state();
    assert_eq!(app.editor.project().name, "Jamais enregistre");
    assert!(app.editor.doc.path.is_none());
    assert!(app.editor.doc.is_dirty());
}

#[test]
fn copie_periodique_si_modifie_puis_supprimee_a_ctrl_s() {
    let t = TempDir::new("periodique");
    let main = t.0.join("robe.pforge.json");
    persist::save(&main, &Project::new("Principal")).expect("principal");
    let before = std::fs::read(&main).expect("lecture");
    let mut app = app_for(&main);
    app.autosave_interval = Some(Duration::from_secs(10));
    let auto = autosave_path(Some(&main), None).expect("chemin");
    let t0 = Instant::now();

    // Document propre : pas de copie.
    app.autosave_tick(t0 + Duration::from_secs(11));
    assert!(!auto.exists());

    // Modifié, mais intervalle non écoulé : rien ; puis copie.
    let sheet = app.editor.project().sheets[0].id;
    app.editor
        .doc
        .execute(Command::RenameSheet {
            sheet,
            name: "Dos".into(),
        })
        .expect("commande");
    let undo_before = app.editor.doc.can_undo();
    app.autosave_tick(t0 + Duration::from_secs(15));
    assert!(!auto.exists());
    app.autosave_tick(t0 + Duration::from_secs(22));
    assert!(auto.exists());
    assert_eq!(persist::load(&auto).expect("copie").sheets[0].name, "Dos");
    assert_eq!(std::fs::read(&main).expect("lecture"), before);
    assert_eq!(app.editor.doc.can_undo(), undo_before);
    assert!(app.editor.doc.is_dirty());

    // Pas de nouvelle écriture tant que rien n'a changé.
    std::fs::remove_file(&auto).expect("suppression");
    app.autosave_tick(t0 + Duration::from_secs(40));
    assert!(!auto.exists());

    // Intervalle 0 : désactivée.
    app.autosave_interval = None;
    app.autosave_tick(t0 + Duration::from_secs(400));
    assert!(!auto.exists());

    // Ctrl+S supprime la copie de secours.
    write_autosave(Some(&main), None, app.editor.project()).expect("copie");
    let mut h = harness_with(app);
    h.key_press_modifiers(Modifiers::COMMAND, Key::S);
    h.run_steps(3);
    assert!(!auto.exists());
    assert!(!h.state().editor.doc.is_dirty());
    assert_eq!(
        persist::load(&main).expect("principal").sheets[0].name,
        "Dos"
    );
}
