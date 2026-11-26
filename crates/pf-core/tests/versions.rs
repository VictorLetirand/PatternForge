//! US-17.2 - versions nommées (`pf_core::versions`) sur de vrais fichiers temporaires.
//! Valeurs et noms de fichiers **écrits à la main**.

use pf_core::file_guard::revision_of;
use pf_core::versions::*;
use pf_core::*;
use pf_geometry::PointMm;
use std::fs;
use std::path::{Path, PathBuf};

fn tmp(name: &str) -> PathBuf {
    let d = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target")
        .join(format!("us172-{name}"));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d.canonicalize().unwrap()
}

fn project_with(name: &str, n: u32) -> Project {
    let mut p = Project::new(name);
    let s = p.sheets[0].id;
    for i in 0..n {
        p.apply(Command::AddPoint {
            sheet: s,
            at: PointMm::new(f64::from(i) * 10.0, 5.0),
        })
        .unwrap();
    }
    p
}

fn proj_path(d: &Path) -> PathBuf {
    d.join("jupe.pforge.json")
}

const T0: u64 = 1_700_000_000; // 2023-11-14 22:13:20 UTC

#[test]
fn dossier_voisin_du_projet() {
    assert_eq!(
        versions_dir(Path::new("a/b/jupe.pforge.json")),
        PathBuf::from("a/b/jupe.pforge.versions")
    );
    assert_eq!(
        versions_dir(Path::new("x.json")),
        PathBuf::from("x.pforge.versions")
    );
}

#[test]
fn horodatage_calcule_a_la_main() {
    assert_eq!(format_timestamp(0), "19700101T000000Z");
    assert_eq!(format_timestamp(T0), "20231114T221320Z");
    // 2000-02-29 : 946 684 800 (1er janvier 2000) + 59 jours.
    assert_eq!(format_timestamp(951_782_400), "20000229T000000Z");
    // Dernière seconde de 1999 : 946 684 799.
    assert_eq!(format_timestamp(946_684_799), "19991231T235959Z");
}

#[test]
fn noms_valides_et_refuses() {
    assert_eq!(validate_name("  Avant jupe  ").unwrap(), "Avant jupe");
    assert_eq!(validate_name("Épaule été").unwrap(), "Épaule été");
    assert_eq!(validate_name(&"a".repeat(60)).unwrap().len(), 60);
    assert_eq!(validate_name(&"a".repeat(61)), Err(NameError::TooLong));
    assert_eq!(validate_name("   "), Err(NameError::Empty));
    assert_eq!(validate_name(""), Err(NameError::Empty));
    for c in ['<', '>', ':', '"', '/', '\\', '|', '?', '*'] {
        assert_eq!(
            validate_name(&format!("a{c}b")),
            Err(NameError::ForbiddenChar(c))
        );
    }
    assert_eq!(
        validate_name("a\u{7}b"),
        Err(NameError::ForbiddenChar('\u{7}'))
    );
    assert_eq!(validate_name("con"), Err(NameError::Reserved));
    assert_eq!(validate_name("Com1.txt"), Err(NameError::Reserved));
    assert_eq!(validate_name("LPT9"), Err(NameError::Reserved));
    assert_eq!(validate_name("com0"), Ok("com0".to_string()));
    assert_eq!(validate_name("fin."), Err(NameError::TrailingDot));
}

#[test]
fn nom_de_fichier_assaini() {
    assert_eq!(sanitize_for_file("Avant jupe!"), "Avant-jupe");
    assert_eq!(sanitize_for_file("a  --  b"), "a-b");
    assert_eq!(sanitize_for_file("???"), "version");
}

#[test]
fn creation_ecrit_les_octets_du_format_sans_toucher_le_projet() {
    let d = tmp("creation");
    let pp = proj_path(&d);
    let p = project_with("Jupe", 3);
    persist::save(&pp, &p).unwrap();
    let before = fs::read(&pp).unwrap();
    let info = create(&pp, &p, "Avant jupe!", T0).unwrap();
    assert_eq!(info.file, "20231114T221320Z-Avant-jupe.pforge");
    assert_eq!(info.name, "Avant jupe!");
    assert_eq!(info.created, "2023-11-14 22:13:20");
    let vf = versions_dir(&pp).join(&info.file);
    let bytes = fs::read(&vf).unwrap();
    assert_eq!(bytes, persist::to_json(&p).unwrap().as_bytes());
    assert_eq!(info.revision, revision_of(&bytes));
    assert_eq!(info.size, bytes.len() as u64);
    assert_eq!(fs::read(&pp).unwrap(), before, "projet inchangé");
    // Index présent, format versionné.
    let idx: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(versions_dir(&pp).join(INDEX_FILE)).unwrap())
            .unwrap();
    assert_eq!(idx["format"], "patternforge-versions");
    assert_eq!(idx["version"], 1);
    assert_eq!(idx["entries"][0]["name"], "Avant jupe!");
    assert_eq!(idx["entries"][0]["revision"], info.revision);
    // Rien d'autre que la version, l'index (et pas de verrou ni de temporaire restant).
    let mut names: Vec<String> = fs::read_dir(versions_dir(&pp))
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    assert_eq!(names, vec![info.file.clone(), "index.json".to_string()]);
    let l = list(&pp).unwrap();
    assert!(!l.index_rebuilt);
    assert_eq!(l.versions, vec![info]);
}

#[test]
fn refus_de_nom_sans_trace() {
    let d = tmp("refus_nom");
    let pp = proj_path(&d);
    let p = project_with("J", 1);
    for bad in ["", "  ", "a/b", "NUL", "x.", &"z".repeat(61)] {
        assert!(matches!(
            create(&pp, &p, bad, T0),
            Err(VersionError::Name(_))
        ));
    }
    assert!(!versions_dir(&pp).exists(), "aucun dossier créé");
}

#[test]
fn deux_versions_meme_seconde_ne_s_ecrasent_pas() {
    let d = tmp("meme_seconde");
    let pp = proj_path(&d);
    let a = create(&pp, &project_with("J", 1), "v", T0).unwrap();
    let b = create(&pp, &project_with("J", 2), "v", T0).unwrap();
    assert_eq!(a.file, "20231114T221320Z-v.pforge");
    assert_eq!(b.file, "20231114T221321Z-v.pforge");
    assert_ne!(a.revision, b.revision);
    assert_eq!(read(&pp, &a.file).unwrap(), project_with("J", 1));
    assert_eq!(read(&pp, &b.file).unwrap(), project_with("J", 2));
    let l = list(&pp).unwrap();
    assert_eq!(l.versions.len(), 2);
    assert_eq!(l.versions[0].file, a.file, "ordre chronologique");
}

#[test]
fn index_absent_ou_faux_est_reconstruit_sans_perte() {
    let d = tmp("index");
    let pp = proj_path(&d);
    let v1 = create(&pp, &project_with("J", 1), "Premier essai", T0).unwrap();
    let v2 = create(&pp, &project_with("J", 2), "Second", T0 + 60).unwrap();
    let idx = versions_dir(&pp).join(INDEX_FILE);
    // 1. Index supprimé : reconstruit, noms tirés des fichiers (assainis).
    fs::remove_file(&idx).unwrap();
    let l = list(&pp).unwrap();
    assert!(l.index_rebuilt);
    assert_eq!(l.versions.len(), 2);
    assert_eq!(l.versions[0].name, "Premier-essai");
    assert_eq!(l.versions[0].revision, v1.revision);
    assert_eq!(l.versions[1].revision, v2.revision);
    assert!(l.versions.iter().all(|v| !v.damaged));
    assert!(idx.is_file(), "index récrit");
    assert!(!list(&pp).unwrap().index_rebuilt, "stable ensuite");
    // 2. Index illisible.
    fs::write(&idx, "{ pas du json").unwrap();
    let l = list(&pp).unwrap();
    assert!(l.index_rebuilt && l.versions.len() == 2);
    // 3. Index faux : une entrée sans fichier, un nom qui ne correspond plus.
    let mut v: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&idx).unwrap()).unwrap();
    v["entries"][0]["name"] = "Autre nom".into();
    v["entries"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({
            "file": "20200101T000000Z-fantome.pforge.json", "name": "fantome",
            "created": "x", "revision": "sha256:00", "size": 1
        }));
    fs::write(&idx, serde_json::to_string(&v).unwrap()).unwrap();
    let l = list(&pp).unwrap();
    assert!(l.index_rebuilt);
    assert_eq!(l.versions.len(), 2, "fantôme retiré");
    assert_eq!(l.versions[0].name, "Premier-essai", "nom faux écarté");
    // 4. Format inconnu (clé en trop) : refusé, reconstruit.
    fs::write(
        &idx,
        r#"{"format":"patternforge-versions","version":1,"entries":[],"extra":1}"#,
    )
    .unwrap();
    assert_eq!(list(&pp).unwrap().versions.len(), 2);
    // Aucun fichier de version n'a été touché.
    assert_eq!(read(&pp, &v1.file).unwrap(), project_with("J", 1));
    assert_eq!(read(&pp, &v2.file).unwrap(), project_with("J", 2));
}

#[test]
fn fichier_tronque_est_signale_jamais_supprime_ni_lu() {
    let d = tmp("tronque");
    let pp = proj_path(&d);
    let v1 = create(&pp, &project_with("J", 3), "bon", T0).unwrap();
    let v2 = create(&pp, &project_with("J", 4), "abime", T0 + 10).unwrap();
    let f2 = versions_dir(&pp).join(&v2.file);
    let bytes = fs::read(&f2).unwrap();
    fs::write(&f2, &bytes[..bytes.len() / 2]).unwrap();
    let l = list(&pp).unwrap();
    assert!(!l.versions[0].damaged);
    assert!(l.versions[1].damaged);
    assert!(matches!(read(&pp, &v2.file), Err(VersionError::Damaged(_))));
    assert_eq!(read(&pp, &v1.file).unwrap(), project_with("J", 3));
    assert_eq!(
        fs::read(&f2).unwrap().len(),
        bytes.len() / 2,
        "non supprimé"
    );
    // Même taille mais contenu changé : l'empreinte le voit.
    let mut same_len = bytes.clone();
    let i = same_len.iter().position(|b| *b == b'1').unwrap();
    same_len[i] = b'2';
    fs::write(&f2, &same_len).unwrap();
    assert!(matches!(read(&pp, &v2.file), Err(VersionError::Damaged(_))));
    // Sans index, un fichier tronqué est repéré à la lecture du contenu.
    fs::write(&f2, &bytes[..bytes.len() / 2]).unwrap();
    fs::remove_file(versions_dir(&pp).join(INDEX_FILE)).unwrap();
    let l = list(&pp).unwrap();
    assert!(l.versions[1].damaged && !l.versions[0].damaged);
    // L'index récrit n'efface pas le signalement : une version abîmée n'y est jamais inscrite.
    let again = list(&pp).unwrap();
    assert!(again.versions[1].damaged && !again.index_rebuilt);
}

#[test]
fn suppression_retire_fichier_et_entree_rien_d_autre() {
    let d = tmp("suppression");
    let pp = proj_path(&d);
    let a = create(&pp, &project_with("J", 1), "a", T0).unwrap();
    let b = create(&pp, &project_with("J", 2), "b", T0 + 5).unwrap();
    let voisin = d.join("20231114T221320Z-a.pforge.json");
    fs::write(&voisin, "ne pas toucher").unwrap();
    delete(&pp, &a.file, T0 + 6).unwrap();
    assert!(!versions_dir(&pp).join(&a.file).exists());
    assert_eq!(list(&pp).unwrap().versions, vec![b.clone()]);
    assert_eq!(fs::read_to_string(&voisin).unwrap(), "ne pas toucher");
    // Noms hors dossier ou inconnus : refus, rien de supprimé.
    for bad in [
        "../jupe.pforge.json",
        "..\\x.pforge.json",
        "index.json",
        "20231114T221320Z-a.pforge.json",
        "",
    ] {
        assert!(
            matches!(delete(&pp, bad, T0), Err(VersionError::Unknown(_))),
            "{bad}"
        );
    }
    assert!(versions_dir(&pp).join(&b.file).is_file());
}

#[test]
fn dossier_verrouille_refuse_sans_rien_ecrire() {
    let d = tmp("verrou");
    let pp = proj_path(&d);
    let a = create(&pp, &project_with("J", 1), "a", T0).unwrap();
    let dir = versions_dir(&pp);
    // Verrou récent d'une autre instance (jeton, pid, heure).
    fs::write(dir.join("index.json.lock"), format!("autre 4242 {T0}")).unwrap();
    let before = fs::read(dir.join(INDEX_FILE)).unwrap();
    assert_eq!(
        create(&pp, &project_with("J", 2), "b", T0 + 1),
        Err(VersionError::Locked)
    );
    assert_eq!(delete(&pp, &a.file, T0 + 1), Err(VersionError::Locked));
    assert!(dir.join(&a.file).is_file());
    assert_eq!(fs::read(dir.join(INDEX_FILE)).unwrap(), before);
    assert_eq!(list(&pp).unwrap().versions.len(), 1);
    // Verrou abandonné (plus de 45 s) : repris.
    assert!(create(&pp, &project_with("J", 2), "b", T0 + 100).is_ok());
}

#[test]
fn restauration_est_une_seule_etape_annulable() {
    let d = tmp("restauration");
    let pp = proj_path(&d);
    let mut doc = Document::new(project_with("J", 1));
    let old = doc.project().clone();
    let v = create(&pp, &old, "v1", T0).unwrap();
    for i in 0..3 {
        let s = doc.project().sheets[0].id;
        doc.execute(Command::AddPoint {
            sheet: s,
            at: PointMm::new(500.0 + f64::from(i), 1.0),
        })
        .unwrap();
    }
    let current = doc.project().clone();
    // Ce que fait l'application : instantané « avant restauration », puis remplacement.
    create(&pp, &current, "avant restauration", T0 + 1).unwrap();
    doc.restore_project(read(&pp, &v.file).unwrap()).unwrap();
    assert_eq!(doc.project(), &old);
    assert!(doc.undo());
    assert_eq!(doc.project(), &current, "un seul undo rend l'état d'avant");
    assert!(doc.redo());
    assert_eq!(doc.project(), &old);
    // Restaurer l'état identique : aucune étape.
    let steps_before = doc.can_undo();
    doc.restore_project(old.clone()).unwrap();
    assert_eq!(doc.can_undo(), steps_before);
    assert!(!doc.undo() || doc.project() == &current);
    // L'instantané automatique existe bien, sous le bon nom.
    let l = list(&pp).unwrap();
    assert_eq!(l.versions.len(), 2);
    assert_eq!(l.versions[1].name, "avant restauration");
    assert_eq!(read(&pp, &l.versions[1].file).unwrap(), current);
}

#[test]
fn restauration_d_un_projet_invalide_est_refusee_sans_trace() {
    let mut doc = Document::new(project_with("J", 2));
    let before = doc.project().clone();
    let mut bad = before.clone();
    bad.next_id = 0; // identifiants déjà utilisés : invalide
    assert!(doc.restore_project(bad).is_err());
    assert_eq!(doc.project(), &before);
    assert!(!doc.can_undo());
}
