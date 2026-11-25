//! Durcissement 2 (tranche 142) : « Enregistrer sous » emporte les images de fond (A1) et une
//! panne du placement automatique est dite (A9). Images générées par le code, dossiers dans
//! `target/` propres au fil d'exécution.

use crate::app::App;
use crate::editor::Editor;
use crate::Launch;
use eframe::egui;
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use pf_core::background_image::{self as bi, ImageStatus};
use pf_core::{Background, CalibrationSpec, Command, Document, PlanePoint, Project};
use pf_i18n::{tr, tr_args, Lang};
use std::path::{Path, PathBuf};

fn tmp(name: &str) -> PathBuf {
    let thread = format!("{:?}", std::thread::current().id()).replace(['(', ')'], "");
    let d = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target")
        .join(format!("us142-{name}-{}-{thread}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d.canonicalize().unwrap()
}

fn harness(launch: Launch, size: egui::Vec2) -> Harness<'static, App> {
    let app = App::with_context(&egui::Context::default(), launch);
    let mut h = Harness::builder()
        .with_size(size)
        .build_state(|ctx, app: &mut App| app.ui(ctx), app);
    h.run_steps(6);
    h
}

fn plain() -> Harness<'static, App> {
    harness(Launch::default(), egui::vec2(1280.0, 800.0))
}

/// Image grise unie de 40 × 20 px, le niveau `shade` la distingue d'une autre.
fn write_image(path: &Path, shade: u8) {
    image::GrayImage::from_pixel(40, 20, image::Luma([shade]))
        .save(path)
        .unwrap();
}

/// Projet enregistré dans `folder/jupe.pforge.json` avec un fond posé.
fn project_with_background(h: &mut Harness<'static, App>, folder: &Path) -> (PathBuf, Background) {
    std::fs::create_dir_all(folder).unwrap();
    let file = folder.join("jupe.pforge.json");
    let mut doc = Document::new(Project::new("Jupe"));
    doc.save_as(&file).unwrap();
    h.state_mut().editor = Editor::new(doc);
    set_background(h, &file, 90);
    assert!(h.state_mut().commit_save(&file, false));
    (file, background(h))
}

fn set_background(h: &mut Harness<'static, App>, file: &Path, shade: u8) {
    let source = file.with_file_name(format!("photo{shade}.png"));
    write_image(&source, shade);
    let image = bi::import_image(file, &source).unwrap();
    let sheet = h.state().editor.sheet;
    h.state_mut()
        .editor
        .exec(Command::SetBackground {
            sheet,
            image: image.relative,
            sha256: image.sha256,
            width_px: image.width_px,
            height_px: image.height_px,
            calibration: CalibrationSpec::TwoPoints {
                points_px: [
                    PlanePoint { x: 0.0, y: 0.0 },
                    PlanePoint { x: 40.0, y: 0.0 },
                ],
                known_distance_mm: 100.0,
            },
        })
        .expect("pose du fond");
}

fn background(h: &Harness<'static, App>) -> Background {
    let ed = &h.state().editor;
    ed.project()
        .sheet(ed.sheet)
        .unwrap()
        .background
        .clone()
        .expect("fond")
}

/// Contenu d'un dossier : (chemin relatif, octets), trié.
fn tree(root: &Path) -> Vec<(String, Vec<u8>)> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, Vec<u8>)>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            let name = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            if path.is_dir() {
                out.push((format!("{name}/"), Vec::new()));
                walk(root, &path, out);
            } else {
                out.push((name, std::fs::read(&path).unwrap()));
            }
        }
    }
    let mut out = Vec::new();
    walk(root, root, &mut out);
    out.sort();
    out
}

#[test]
fn a1_enregistrer_sous_dans_un_autre_dossier_emporte_les_images() {
    let root = tmp("copie");
    let mut h = plain();
    let (_, bg) = project_with_background(&mut h, &root.join("a"));
    // Une image sans référence dans le dossier (fond retiré, rétabli par annulation) suit aussi.
    std::fs::write(root.join("a/jupe.assets/ancienne.png"), b"ancienne").unwrap();
    let before = tree(&root.join("a/jupe.assets"));
    std::fs::create_dir_all(root.join("b")).unwrap();
    let target = root.join("b/robe.pforge.json");
    assert!(h.state_mut().commit_save(&target, false));
    assert_eq!(h.state().error_dialog, None);
    assert_eq!(
        bi::status(&root.join("b"), &background(&h)),
        ImageStatus::Ok
    );
    assert_eq!(background(&h), bg, "la référence n'est pas réécrite");
    assert_eq!(tree(&root.join("b/jupe.assets")), before);
    assert_eq!(tree(&root.join("a/jupe.assets")), before, "original intact");
    assert!(!root.join("b/robe.assets").exists());

    // Une image importée ensuite va dans « robe.assets » ; un second « Enregistrer sous » emporte
    // les deux dossiers cités.
    let sheet = h.state().editor.sheet;
    h.state_mut()
        .editor
        .exec(Command::RemoveBackground { sheet })
        .expect("retrait");
    set_background(&mut h, &target, 200);
    assert!(background(&h).image.starts_with("robe.assets/"));
    std::fs::create_dir_all(root.join("c")).unwrap();
    assert!(h
        .state_mut()
        .commit_save(&root.join("c/x.pforge.json"), false));
    assert_eq!(h.state().error_dialog, None);
    assert_eq!(
        bi::status(&root.join("c"), &background(&h)),
        ImageStatus::Ok
    );
    // « jupe.assets » n'est plus cité ni lié à l'ancien nom : il n'est pas recopié.
    assert!(!root.join("c/jupe.assets").exists());
    h.state_mut().editor.undo();
    h.state_mut().editor.undo();
    assert!(h.state_mut().commit_save(&target, false));
    assert_eq!(
        bi::status(&root.join("b"), &background(&h)),
        ImageStatus::Ok
    );
}

#[test]
fn a1_meme_dossier_ou_fichier_identique_rien_a_copier() {
    let root = tmp("meme");
    let mut h = plain();
    let (_, bg) = project_with_background(&mut h, &root);
    let before = tree(&root.join("jupe.assets"));
    let target = root.join("robe.pforge.json");
    assert!(h.state_mut().commit_save(&target, false));
    assert_eq!(h.state().error_dialog, None);
    assert_eq!(bi::status(&root, &bg), ImageStatus::Ok);
    assert_eq!(tree(&root.join("jupe.assets")), before);
    assert!(!root.join("robe.assets").exists());

    // À destination, la même image est déjà là : aucun message, rien de réécrit.
    let other = root.join("d");
    std::fs::create_dir_all(other.join("jupe.assets")).unwrap();
    std::fs::copy(root.join(&bg.image), other.join(&bg.image)).unwrap();
    assert!(h
        .state_mut()
        .commit_save(&other.join("robe.pforge.json"), false));
    assert_eq!(h.state().error_dialog, None);
    assert_eq!(bi::status(&other, &bg), ImageStatus::Ok);
}

/// Fond retiré avant « Enregistrer sous » : plus aucune référence, mais une annulation le rétablit.
#[test]
fn a1_fond_retire_puis_retabli_apres_enregistrer_sous() {
    let root = tmp("retire");
    let mut h = plain();
    let (_, bg) = project_with_background(&mut h, &root.join("a"));
    let sheet = h.state().editor.sheet;
    h.state_mut()
        .editor
        .exec(Command::RemoveBackground { sheet })
        .expect("retrait");
    let other = root.join("b");
    std::fs::create_dir_all(&other).unwrap();
    assert!(h
        .state_mut()
        .commit_save(&other.join("robe.pforge.json"), false));
    assert_eq!(h.state().error_dialog, None);
    h.state_mut().editor.undo();
    assert_eq!(background(&h), bg);
    assert_eq!(bi::status(&other, &bg), ImageStatus::Ok);
}

/// Un fichier différent déjà là n'est pas remplacé ; le projet est enregistré, le message nomme le
/// fichier ; l'original ne change pas.
fn conflict(lang: Lang, title: &str, line: &str) {
    pf_i18n::set_lang(lang);
    let root = tmp("conflit");
    let mut h = plain();
    let (file, bg) = project_with_background(&mut h, &root.join("a"));
    let original = tree(&root.join("a"));
    let other = root.join("b");
    std::fs::create_dir_all(other.join("jupe.assets")).unwrap();
    std::fs::write(other.join(&bg.image), b"un autre contenu").unwrap();
    let target = other.join("robe.pforge.json");
    assert!(
        h.state_mut().commit_save(&target, false),
        "projet enregistré"
    );
    assert!(pf_core::persist::load(&target).is_ok());
    assert_eq!(h.state().editor.doc.path.as_deref(), Some(target.as_path()));
    assert!(!h.state().editor.doc.is_dirty());
    assert_eq!(
        std::fs::read(other.join(&bg.image)).unwrap(),
        b"un autre contenu"
    );
    assert_eq!(tree(&root.join("a")), original);
    assert!(file.exists());
    let message = h.state().error_dialog.clone().expect("message");
    assert!(message.contains(title), "{message}");
    assert!(
        message.contains(&line.replace("{file}", &bg.image)),
        "{message}"
    );
    h.run_steps(3);
    h.get_by_label_contains(title);
    pf_i18n::set_lang(Lang::Fr);
}

#[test]
fn a1_conflit_dit_en_francais() {
    conflict(
        Lang::Fr,
        "des images de fond n'ont pas été copiées",
        "« {file} » : un fichier différent porte déjà ce nom.",
    );
}

#[test]
fn a1_conflit_dit_en_anglais() {
    conflict(
        Lang::En,
        "some background images were not copied",
        "“{file}”: a different file already has this name.",
    );
}

#[test]
fn a1_copie_impossible_le_projet_reste_enregistre() {
    let root = tmp("echec");
    let mut h = plain();
    let (_, bg) = project_with_background(&mut h, &root.join("a"));
    let other = root.join("b");
    std::fs::create_dir_all(&other).unwrap();
    // Un fichier occupe la place du dossier des images : la copie ne peut pas se faire.
    std::fs::write(other.join("jupe.assets"), b"fichier").unwrap();
    let target = other.join("robe.pforge.json");
    assert!(h.state_mut().commit_save(&target, false));
    assert!(pf_core::persist::load(&target).is_ok());
    assert_eq!(
        std::fs::read(other.join("jupe.assets")).unwrap(),
        b"fichier"
    );
    assert_eq!(bi::status(&root.join("a"), &bg), ImageStatus::Ok);
    let message = h.state().error_dialog.clone().expect("message");
    assert!(message.contains(&bg.image), "{message}");
}

/// Le calcul s'arrête sans rendre de résultat : message, projet et historique intacts, puis le
/// calcul suivant fonctionne.
fn auto_failure(lang: Lang, expected: &str) {
    pf_i18n::set_lang(lang);
    let launch = Launch {
        demo: true,
        stage: Some("plan-coupe-auto".into()),
        ..Launch::default()
    };
    let mut h = harness(launch, egui::vec2(1600.0, 1000.0));
    h.get_by_label(&tr("cutting.auto.discard")).click();
    h.run_steps(3);
    let before = h.state().editor.project().clone();
    let can_undo = h.state().editor.doc.can_undo();
    h.state_mut().cutting.as_mut().unwrap().auto_fail = true;
    h.get_by_label(&tr("cutting.auto.run")).click();
    for _ in 0..400 {
        h.step();
        if h.state().cutting.as_ref().unwrap().auto_job.is_none()
            && h.state().error_dialog.is_some()
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    h.run_steps(3);
    assert_eq!(h.state().error_dialog.as_deref(), Some(expected));
    h.get_by_label(expected);
    let d = h.state().cutting.as_ref().unwrap();
    assert!(d.auto_job.is_none() && d.auto_out.is_none());
    assert_eq!(*h.state().editor.project(), before);
    assert_eq!(h.state().editor.doc.can_undo(), can_undo);

    h.state_mut().error_dialog = None;
    h.state_mut().cutting.as_mut().unwrap().auto_fail = false;
    h.run_steps(3);
    h.get_by_label(&tr("cutting.auto.run")).click();
    for _ in 0..400 {
        h.step();
        if h.state().cutting.as_ref().unwrap().auto_out.is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    h.run_steps(2);
    h.get_by_label(&tr_args(
        "cutting.auto.proposed",
        &[("l", "500"), ("y", "76.91")],
    ));
    assert_eq!(h.state().error_dialog, None);
    pf_i18n::set_lang(Lang::Fr);
}

#[test]
fn a9_panne_du_placement_automatique_dite_en_francais() {
    auto_failure(
        Lang::Fr,
        "Le placement automatique s'est arrêté de façon inattendue. Aucun placement n'a été modifié.",
    );
}

#[test]
fn a9_panne_du_placement_automatique_dite_en_anglais() {
    auto_failure(
        Lang::En,
        "Automatic placement stopped unexpectedly. No placement was changed.",
    );
}
