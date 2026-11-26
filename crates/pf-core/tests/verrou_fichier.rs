//! US-17.3 : verrou et accès concurrent, sur de vrais fichiers temporaires. Deux « instances »
//! sont deux `Document` (et deux `FileLock`) ouverts sur le même fichier.

use pf_core::file_guard::{self, FileLock, LockError, LOCK_STALE_SECS};
use pf_core::{persist, Command, Document, ExternalChange, Project, SaveError};
use std::path::PathBuf;

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!(
            "pf-verrou-{}-{tag}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dossier temporaire");
        TempDir(dir)
    }
    fn file(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn rename(doc: &mut Document, name: &str) {
    doc.execute(Command::RenameProject { name: name.into() })
        .expect("renommage");
}

fn on_disk_name(path: &std::path::Path) -> String {
    persist::load(path).expect("relecture").name
}

fn saved_project(tmp: &TempDir) -> PathBuf {
    let path = tmp.file("p.pforge.json");
    persist::save(&path, &Project::new("Départ")).expect("écriture");
    path
}

#[test]
fn empreinte_d_un_fichier_connu() {
    let tmp = TempDir::new("empreinte");
    let path = tmp.file("abc.bin");
    std::fs::write(&path, b"abc").unwrap();
    // Vecteur SHA-256 publié (FIPS 180-2) pour « abc ».
    assert_eq!(
        file_guard::file_revision(&path).unwrap().as_deref(),
        Some("sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")
    );
    assert_eq!(
        file_guard::file_revision(&tmp.file("absent")).unwrap(),
        None
    );
}

#[test]
fn enregistrement_normal_sans_modification_externe() {
    let tmp = TempDir::new("normal");
    let path = saved_project(&tmp);
    let mut doc = Document::open(&path).unwrap();
    assert_eq!(doc.external_change().unwrap(), ExternalChange::Unchanged);
    rename(&mut doc, "Un");
    doc.save_checked(&path).expect("pas de conflit");
    rename(&mut doc, "Deux");
    doc.save_checked(&path)
        .expect("la révision suit chaque écriture");
    assert_eq!(on_disk_name(&path), "Deux");
}

#[test]
fn deux_instances_la_seconde_est_refusee_sans_trace() {
    let tmp = TempDir::new("deux");
    let path = saved_project(&tmp);
    let mut a = Document::open(&path).unwrap();
    let mut b = Document::open(&path).unwrap();
    rename(&mut a, "Version A");
    a.save_checked(&path).unwrap();
    let after_a = std::fs::read(&path).unwrap();

    rename(&mut b, "Version B");
    let refus = b.save_checked(&path);
    assert_eq!(refus, Err(SaveError::External(ExternalChange::Modified)));
    assert_eq!(std::fs::read(&path).unwrap(), after_a, "octets identiques");
    let names: Vec<_> = std::fs::read_dir(&tmp.0)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, vec!["p.pforge.json".to_string()], "aucun temporaire");
    assert!(b.is_dirty(), "le travail de B n'est pas perdu");
    assert_eq!(b.project().name, "Version B");

    // Choix explicite d'écraser : la version de B gagne, puis la révision est à jour.
    b.save_as(&path).unwrap();
    assert_eq!(on_disk_name(&path), "Version B");
    rename(&mut b, "Version B2");
    b.save_checked(&path)
        .expect("plus de conflit après écrasement");
}

#[test]
fn reecriture_identique_n_est_pas_une_modification() {
    let tmp = TempDir::new("identique");
    let path = saved_project(&tmp);
    let mut doc = Document::open(&path).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    std::fs::write(&path, &bytes).unwrap();
    assert_eq!(doc.external_change().unwrap(), ExternalChange::Unchanged);
    rename(&mut doc, "Un");
    doc.save_checked(&path).expect("empreinte, pas date");
}

#[test]
fn modification_d_un_octet_est_detectee() {
    let tmp = TempDir::new("octet");
    let path = saved_project(&tmp);
    let doc = Document::open(&path).unwrap();
    let mut bytes = std::fs::read(&path).unwrap();
    bytes.push(b'\n');
    std::fs::write(&path, &bytes).unwrap();
    assert_eq!(doc.external_change().unwrap(), ExternalChange::Modified);
}

#[test]
fn fichier_supprime_est_signale_et_non_recree() {
    let tmp = TempDir::new("supprime");
    let path = saved_project(&tmp);
    let mut doc = Document::open(&path).unwrap();
    std::fs::remove_file(&path).unwrap();
    rename(&mut doc, "Un");
    assert_eq!(
        doc.save_checked(&path),
        Err(SaveError::External(ExternalChange::Removed))
    );
    assert!(!path.exists());
    doc.save_as(&path).expect("recréation explicite");
    assert_eq!(on_disk_name(&path), "Un");
}

#[test]
fn enregistrer_sous_un_autre_chemin_n_est_pas_controle() {
    let tmp = TempDir::new("autre");
    let path = saved_project(&tmp);
    let mut doc = Document::open(&path).unwrap();
    std::fs::write(&path, b"{}").unwrap();
    let other = tmp.file("autre.pforge.json");
    rename(&mut doc, "Copie");
    doc.save_checked(&other).expect("autre chemin");
    assert_eq!(on_disk_name(&other), "Copie");
    assert_eq!(std::fs::read(&path).unwrap(), b"{}", "l'original reste");
}

#[test]
fn document_sans_chemin_ou_recupere() {
    let tmp = TempDir::new("recupere");
    let path = saved_project(&tmp);
    let mut fresh = Document::new(Project::new("Neuf"));
    assert_eq!(fresh.external_change().unwrap(), ExternalChange::Unchanged);
    fresh.save_checked(&path).expect("premier enregistrement");
    // Reprise d'une copie de secours : la révision de départ est celle du disque.
    let mut rec = Document::recovered(
        Project::new("Secours"),
        Some(path.clone()),
        Project::new("Neuf"),
    );
    assert_eq!(rec.external_change().unwrap(), ExternalChange::Unchanged);
    std::fs::write(&path, b"{}").unwrap();
    assert_eq!(rec.external_change().unwrap(), ExternalChange::Modified);
    assert!(rec.save_checked(&path).is_err());
}

#[test]
fn verrou_pris_refuse_puis_libere() {
    let tmp = TempDir::new("verrou");
    let path = saved_project(&tmp);
    let lock = FileLock::acquire(&path, 1_000).expect("premier verrou");
    let lock_file = file_guard::lock_path(&path);
    assert!(lock_file.exists());
    match FileLock::acquire(&path, 1_005) {
        Err(LockError::Held(h)) => {
            assert_eq!(h.pid, std::process::id());
            assert_eq!(h.since, 1_000);
        }
        other => panic!("attendu Held, obtenu {other:?}"),
    }
    drop(lock);
    assert!(!lock_file.exists(), "verrou retiré à la destruction");
    let again = FileLock::acquire(&path, 1_006).expect("libre à nouveau");
    drop(again);
}

#[test]
fn verrou_abandonne_ou_illisible_est_repris() {
    let tmp = TempDir::new("abandonne");
    let path = saved_project(&tmp);
    let lock_file = file_guard::lock_path(&path);
    std::fs::write(&lock_file, "autre 4242 1000").unwrap();
    assert!(matches!(
        FileLock::acquire(&path, 1_000 + LOCK_STALE_SECS - 1),
        Err(LockError::Held(h)) if h.pid == 4242
    ));
    let taken = FileLock::acquire(&path, 1_000 + LOCK_STALE_SECS).expect("exactement périmé");
    drop(taken);
    for bad in ["", "n'importe quoi", "a b c", "a 1 2 3"] {
        std::fs::write(&lock_file, bad).unwrap();
        FileLock::acquire(&path, 10).unwrap_or_else(|e| panic!("{bad:?} : {e:?}"));
    }
}

#[test]
fn rafraichissement_garde_le_verrou_et_ne_vole_pas_celui_d_un_autre() {
    let tmp = TempDir::new("rafraichi");
    let path = saved_project(&tmp);
    let lock = FileLock::acquire(&path, 1_000).unwrap();
    lock.refresh(1_040).unwrap();
    assert!(matches!(
        FileLock::acquire(&path, 1_080),
        Err(LockError::Held(h)) if h.since == 1_040
    ));
    // Un autre propriétaire a repris le fichier : notre rafraîchissement et notre
    // destruction ne doivent ni l'écraser ni le supprimer.
    let lock_file = file_guard::lock_path(&path);
    std::fs::write(&lock_file, "autre 7 5000").unwrap();
    lock.refresh(5_001).unwrap();
    assert_eq!(std::fs::read_to_string(&lock_file).unwrap(), "autre 7 5000");
    drop(lock);
    assert!(lock_file.exists());
}

#[test]
fn le_verrou_ne_contient_aucun_chemin() {
    let tmp = TempDir::new("anonyme");
    let path = saved_project(&tmp);
    let _lock = FileLock::acquire(&path, 1_000).unwrap();
    let content = std::fs::read_to_string(file_guard::lock_path(&path)).unwrap();
    assert!(!content.contains(tmp.0.to_string_lossy().as_ref()));
    assert!(!content.contains("p.pforge"));
    assert_eq!(content.split_whitespace().count(), 3);
}

#[test]
fn dossier_introuvable_pas_de_verrou() {
    let tmp = TempDir::new("introuvable");
    let path = tmp.file("absent").join("p.pforge.json");
    assert!(matches!(FileLock::acquire(&path, 1), Err(LockError::Io(_))));
}

#[test]
fn enregistrer_sur_un_fichier_en_lecture_seule_est_refuse() {
    let dir = TempDir::new("lecture-seule");
    let path = dir.file("p.pforge.json");
    std::fs::write(&path, b"ancien").unwrap();
    let mut perm = std::fs::metadata(&path).unwrap().permissions();
    perm.set_readonly(true);
    std::fs::set_permissions(&path, perm.clone()).unwrap();
    let res = persist::save(&path, &Project::new("Lecture seule"));
    let content = std::fs::read(&path).unwrap();
    let files = std::fs::read_dir(&dir.0).unwrap().count();
    #[allow(clippy::permissions_set_readonly_false)]
    perm.set_readonly(false);
    std::fs::set_permissions(&path, perm).unwrap();
    assert!(res.is_err(), "écrasement d'un fichier en lecture seule");
    assert_eq!(content, b"ancien");
    assert_eq!(files, 1, "aucun fichier temporaire laissé");
}
