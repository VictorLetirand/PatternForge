//! US-17.3 (verrou, modification externe) et US-17.5 (journal d'erreurs) côté interface : boîtes
//! en français et en anglais, sur de vrais fichiers temporaires. Les deux « instances » sont deux
//! `App` ouvertes sur le même fichier. Le sélecteur de fichier du système (« Enregistrer
//! sous… ») n'est pas pilotable ici : voir les limites du journal de tranche.

use crate::app::App;
use crate::Launch;
use eframe::egui;
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use pf_core::file_guard::{self, FileLock};
use pf_core::{persist, Command, Project};
use pf_i18n::{tr, Lang};
use std::path::{Path, PathBuf};

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!(
            "pf-desktop-safeguard-{}-{tag}-{:?}",
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

fn harness(file: Option<&Path>, lang: Lang) -> Harness<'static, App> {
    let app = App::with_context(
        &egui::Context::default(),
        Launch {
            file: file.map(Path::to_path_buf),
            lang: Some(lang),
            ..Launch::default()
        },
    );
    let mut h = Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .build_state(|ctx, app: &mut App| app.ui(ctx), app);
    h.run_steps(3);
    h
}

fn saved(dir: &TempDir, name: &str) -> PathBuf {
    let path = dir.0.join("robe.pforge.json");
    persist::save(&path, &Project::new(name)).expect("projet");
    path
}

fn rename(h: &mut Harness<'static, App>, name: &str) {
    h.state_mut()
        .editor
        .doc
        .execute(Command::RenameProject { name: name.into() })
        .expect("renommage");
}

const LANGS: [Lang; 2] = [Lang::Fr, Lang::En];

#[test]
fn modification_externe_refus_puis_ecraser() {
    for lang in LANGS {
        let t = TempDir::new(&format!("ecraser-{}", lang.code()));
        let path = saved(&t, "D\u{e9}part");
        let mut h = harness(Some(&path), lang);
        rename(&mut h, "Mon travail");
        persist::save(&path, &Project::new("Autre programme")).expect("modif externe");
        let external = std::fs::read(&path).expect("lecture");

        assert!(!h.state_mut().save(false), "refus\u{e9}");
        h.run_steps(3);
        assert!(h.state().conflict_prompt.is_some());
        h.get_by_label(&tr("file.conflict.title"));
        h.get_by_label(&tr("file.conflict.save_as"));
        assert_eq!(
            std::fs::read(&path).unwrap(),
            external,
            "rien d'\u{e9}crit ({lang:?})"
        );
        assert!(h.state().editor.doc.is_dirty(), "le travail reste");

        h.get_by_label(&tr("file.conflict.overwrite")).click();
        h.run_steps(3);
        assert!(h.state().conflict_prompt.is_none());
        assert_eq!(persist::load(&path).unwrap().name, "Mon travail");
        assert!(!h.state().editor.doc.is_dirty());
    }
}

#[test]
fn modification_externe_annuler_ne_change_rien() {
    for lang in LANGS {
        let t = TempDir::new(&format!("annuler-{}", lang.code()));
        let path = saved(&t, "D\u{e9}part");
        let mut h = harness(Some(&path), lang);
        rename(&mut h, "Mon travail");
        persist::save(&path, &Project::new("Autre")).unwrap();
        let external = std::fs::read(&path).unwrap();
        assert!(!h.state_mut().save(false));
        h.run_steps(3);
        assert!(h.get_all_by_label(&tr("dialog.cancel")).count() >= 1);
        h.key_press(egui::Key::Escape);
        h.run_steps(3);
        assert!(h.state().conflict_prompt.is_none());
        assert_eq!(std::fs::read(&path).unwrap(), external);
        assert!(h.state().editor.doc.is_dirty());
    }
}

#[test]
fn echap_annule_et_entree_ne_detruit_rien() {
    let t = TempDir::new("clavier");
    let path = saved(&t, "D\u{e9}part");
    let mut h = harness(Some(&path), Lang::Fr);
    rename(&mut h, "Mon travail");
    persist::save(&path, &Project::new("Autre")).unwrap();
    let external = std::fs::read(&path).unwrap();
    assert!(!h.state_mut().save(false));
    h.run_steps(3);
    h.key_press(egui::Key::Escape);
    h.run_steps(3);
    assert!(h.state().conflict_prompt.is_none());
    assert_eq!(std::fs::read(&path).unwrap(), external);
}

#[test]
fn fichier_supprime_propose_de_recreer() {
    for lang in LANGS {
        let t = TempDir::new(&format!("supprime-{}", lang.code()));
        let path = saved(&t, "D\u{e9}part");
        let mut h = harness(Some(&path), lang);
        rename(&mut h, "Mon travail");
        std::fs::remove_file(&path).unwrap();
        assert!(!h.state_mut().save(false));
        h.run_steps(3);
        h.get_by_label(&tr("file.conflict.recreate")).click();
        h.run_steps(3);
        assert_eq!(persist::load(&path).unwrap().name, "Mon travail");
    }
}

#[test]
fn action_en_attente_continue_apres_ecrasement() {
    let t = TempDir::new("attente");
    let path = saved(&t, "D\u{e9}part");
    let mut h = harness(Some(&path), Lang::Fr);
    rename(&mut h, "Mon travail");
    persist::save(&path, &Project::new("Autre")).unwrap();
    // « Enregistrer » depuis la boîte « modifications non enregistrées » (nouveau projet).
    h.state_mut().pending = Some(crate::app::Pending::New);
    h.run_steps(3);
    h.get_by_label(&tr("dialog.unsaved.save")).click();
    h.run_steps(3);
    assert!(
        h.state().conflict_prompt.is_some(),
        "conflit d\u{e9}tect\u{e9}"
    );
    assert!(h.state().pending.is_some(), "l'action attend le choix");
    h.get_by_label(&tr("file.conflict.overwrite")).click();
    h.run_steps(3);
    assert_eq!(persist::load(&path).unwrap().name, "Mon travail");
    assert!(h.state().pending.is_none());
    assert_eq!(h.state().editor.doc.path, None, "nouveau projet ouvert");
}

#[test]
fn deux_instances_la_seconde_voit_le_fichier_ouvert_ailleurs() {
    for lang in LANGS {
        let t = TempDir::new(&format!("verrou-{}", lang.code()));
        let path = saved(&t, "D\u{e9}part");
        let first = harness(Some(&path), lang);
        assert!(file_guard::lock_path(&path).exists(), "verrou tenu");
        assert!(first.state().file_lock.is_some());

        let mut second = harness(Some(&path), lang);
        assert!(second.state().lock_prompt.is_some());
        assert_eq!(second.state().editor.doc.path, None, "pas ouvert");
        second.get_by_label(&tr("file.locked.title"));
        second.get_by_label(&tr("file.locked.open"));
        // Annuler (par défaut) : rien n'est ouvert.
        assert!(second.get_all_by_label(&tr("dialog.cancel")).count() >= 1);
        second.key_press(egui::Key::Escape);
        second.run_steps(3);
        assert!(second.state().lock_prompt.is_none());
        assert_eq!(second.state().editor.doc.path, None);

        // Ouvrir quand même : ouvert sans verrou, le verrou de la première reste intact.
        let mut third = harness(Some(&path), lang);
        third.get_by_label(&tr("file.locked.open")).click();
        third.run_steps(3);
        assert_eq!(
            third.state().editor.doc.path.as_deref(),
            Some(path.as_path())
        );
        assert!(third.state().file_lock.is_none());
        assert!(file_guard::lock_path(&path).exists());
        drop(third);
        assert!(
            file_guard::lock_path(&path).exists(),
            "pas le verrou de la premi\u{e8}re"
        );
        drop(first);
        assert!(
            !file_guard::lock_path(&path).exists(),
            "lib\u{e9}r\u{e9} \u{e0} la fermeture"
        );
    }
}

#[test]
fn verrou_perime_est_repris_a_l_ouverture() {
    let t = TempDir::new("perime");
    let path = saved(&t, "D\u{e9}part");
    std::fs::write(file_guard::lock_path(&path), "vieux 4242 1000").unwrap();
    let h = harness(Some(&path), Lang::Fr);
    assert!(h.state().lock_prompt.is_none());
    assert_eq!(h.state().editor.project().name, "D\u{e9}part");
    assert!(h.state().file_lock.is_some());
}

#[test]
fn enregistrer_sous_un_fichier_ouvert_ailleurs_est_refuse() {
    let t = TempDir::new("sous");
    let target = t.0.join("occupe.pforge.json");
    persist::save(&target, &Project::new("Occup\u{e9}")).unwrap();
    let _other = FileLock::acquire(&target, crate::safeguard::unix_now()).unwrap();
    let mut h = harness(None, Lang::Fr);
    let before = std::fs::read(&target).unwrap();
    assert!(!h.state_mut().commit_save(&target, false));
    h.run_steps(3);
    assert!(h.state().error_dialog.is_some());
    assert_eq!(std::fs::read(&target).unwrap(), before);
    assert_eq!(h.state().editor.doc.path, None);
}

#[test]
fn premier_enregistrement_prend_le_verrou_et_enregistrer_sous_le_deplace() {
    let t = TempDir::new("deplace");
    let (a, b) = (t.0.join("a.pforge.json"), t.0.join("b.pforge.json"));
    let mut h = harness(None, Lang::Fr);
    assert!(h.state_mut().commit_save(&a, false));
    assert!(file_guard::lock_path(&a).exists());
    assert!(h.state_mut().commit_save(&b, false));
    assert!(
        !file_guard::lock_path(&a).exists(),
        "ancien verrou lib\u{e9}r\u{e9}"
    );
    assert!(file_guard::lock_path(&b).exists());
    // Enregistrement normal suivant : le fichier n'a pas changé, pas de conflit.
    rename(&mut h, "Suite");
    assert!(h.state_mut().save(false));
    assert!(h.state().conflict_prompt.is_none());
}

#[test]
fn journal_une_erreur_affichee_est_gardee_assainie_et_exportee() {
    for lang in LANGS {
        let t = TempDir::new(&format!("journal-{}", lang.code()));
        let settings = t.0.join("settings.json");
        let path = saved(&t, "Plan secret Martin");
        let mut h = harness(Some(&path), lang);
        h.state_mut().settings_path = Some(settings.clone());
        let raw = format!(
            "Cannot write {} : acc\u{e8}s refus\u{e9} pour \u{ab} Plan secret Martin \u{bb}",
            path.display()
        );
        h.state_mut().error_dialog = Some(raw);
        h.run_steps(3);
        let entries: Vec<_> = h.state().error_log.entries().to_vec();
        assert_eq!(entries.len(), 1, "{lang:?}");
        assert_eq!(entries[0].kind, "dialog");
        assert_eq!(
            entries[0].message,
            "Cannot write <\u{2026}> : acc\u{e8}s refus\u{e9} pour \u{ab}\u{2026}\u{bb}"
        );
        // Une même boîte affichée plusieurs images n'ajoute rien.
        h.run_steps(3);
        assert_eq!(h.state().error_log.len(), 1);
        // Réglage hors projet, relu à l'identique.
        assert_eq!(crate::error_journal::load(&settings), h.state().error_log);
        let on_disk = std::fs::read_to_string(&settings).unwrap();
        for secret in ["Martin", "pf-desktop-safeguard", "robe.pforge"] {
            assert!(!on_disk.contains(secret), "{secret} dans {on_disk}");
        }

        // Fenêtre et export.
        h.state_mut().error_dialog = None;
        h.state_mut().show_error_log = true;
        h.run_steps(3);
        h.get_by_label(&tr("errlog.title"));
        h.get_by_label(&entries[0].message);
        let out = t.0.join("journal.txt");
        h.state_mut().export_error_log_to(&out);
        let text = std::fs::read_to_string(&out).unwrap();
        let mut lines = text.lines();
        assert_eq!(lines.next(), Some(tr("errlog.export_header").as_str()));
        assert_eq!(lines.next(), Some(""));
        let line = lines.next().unwrap();
        assert!(line.ends_with(
            "[dialog] Cannot write <\u{2026}> : acc\u{e8}s refus\u{e9} pour \u{ab}\u{2026}\u{bb}"
        ));
        assert_eq!(lines.next(), None);
        assert!(!text.contains("Martin") && !text.contains("pf-desktop-safeguard"));
    }
}

#[test]
fn journal_message_d_etat_rouge_et_refus_de_conflit_sont_gardes() {
    let t = TempDir::new("etat");
    let path = saved(&t, "D\u{e9}part");
    let mut h = harness(Some(&path), Lang::Fr);
    h.state_mut().editor.error("Refused length".to_string());
    h.run_steps(2);
    rename(&mut h, "Mon travail");
    persist::save(&path, &Project::new("Autre")).unwrap();
    assert!(!h.state_mut().save(false));
    let kinds: Vec<_> = h
        .state()
        .error_log
        .entries()
        .iter()
        .map(|e| (e.kind.as_str(), e.message.as_str()))
        .collect();
    assert_eq!(
        kinds,
        vec![
            ("status", "Refused length"),
            (
                "conflict",
                "Enregistrement refus\u{e9} : fichier modifi\u{e9} ailleurs."
            )
        ]
    );
}

#[test]
fn journal_effacer_et_fenetre_vide() {
    let t = TempDir::new("effacer");
    let settings = t.0.join("settings.json");
    let mut h = harness(None, Lang::Fr);
    h.state_mut().settings_path = Some(settings.clone());
    h.state_mut().show_error_log = true;
    h.run_steps(3);
    h.get_by_label(&tr("errlog.empty"));
    h.state_mut().error_dialog = Some("Une erreur".into());
    h.run_steps(3);
    h.state_mut().error_dialog = None;
    h.run_steps(3);
    assert_eq!(h.state().error_log.len(), 1);
    h.get_by_label(&tr("errlog.clear")).click();
    h.run_steps(3);
    assert!(h.state().error_log.is_empty());
    assert!(crate::error_journal::load(&settings).is_empty());
}
