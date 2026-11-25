//! Sauvegarde automatique et récupération (US-17.1).
//!
//! - Projet enregistré : la copie de secours est `<projet>.pforge.json.autosave`,
//!   à côté du fichier principal (même volume, donc renommage atomique).
//! - Projet jamais enregistré : `<dossier de données>/recovery/untitled.pforge.json.autosave`
//!   (`%APPDATA%\PatternCraft\recovery\` sous Windows). Un seul emplacement.
//! - Le fichier principal n'est jamais écrit ici. La copie passe par un
//!   temporaire puis un renommage : un arrêt brutal laisse l'ancienne copie
//!   (ou aucune) et ne produit jamais un `.autosave` tronqué.
//! - Une copie illisible n'est jamais supprimée : elle est renommée en
//!   `.corrupt` et signalée.
//!
//! Toutes les fonctions prennent le dossier de données en paramètre (`None` :
//! pas d'emplacement pour un projet sans chemin) ; les tests n'écrivent jamais
//! dans le dossier utilisateur.

use crate::error::CoreError;
use crate::model::Project;
use crate::persist;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Suffixe ajouté au nom du fichier principal.
pub const AUTOSAVE_SUFFIX: &str = ".autosave";
/// Sous-dossier du dossier de données pour les projets sans chemin.
pub const RECOVERY_DIR: &str = "recovery";
/// Nom du fichier de secours d'un projet sans chemin.
pub const UNSAVED_NAME: &str = "untitled.pforge.json.autosave";

/// Copie de secours exploitable, plus récente que le fichier principal.
#[derive(Debug, Clone)]
pub struct Recovery {
    pub autosave: PathBuf,
    pub project: Project,
}

/// Résultat de la recherche d'une copie de secours.
#[derive(Debug, Clone)]
// `Project.sizes` (US-09.1) a fait passer l'écart de taille entre variantes au-dessus du seuil
// de clippy ; valeur rendue une fois au lancement, pas de boîte (refactor non demandé).
#[allow(clippy::large_enum_variant)]
pub enum RecoveryCheck {
    /// Rien à proposer.
    None,
    /// Une reprise est possible.
    Available(Recovery),
    /// La copie était illisible ; elle a été mise de côté (`moved_to`).
    Corrupt {
        autosave: PathBuf,
        moved_to: PathBuf,
        detail: String,
    },
}

/// Emplacement de la copie de secours (`None` : projet sans chemin et pas de
/// dossier de données).
pub fn autosave_path(project_path: Option<&Path>, data_dir: Option<&Path>) -> Option<PathBuf> {
    match project_path {
        Some(p) => {
            let mut name = p.file_name()?.to_os_string();
            name.push(AUTOSAVE_SUFFIX);
            Some(p.with_file_name(name))
        }
        None => data_dir.map(|d| d.join(RECOVERY_DIR).join(UNSAVED_NAME)),
    }
}

/// Écrit la copie de secours (validation puis écriture atomique) et renvoie son
/// chemin. Ne touche jamais au fichier principal.
pub fn write_autosave(
    project_path: Option<&Path>,
    data_dir: Option<&Path>,
    project: &Project,
) -> Result<PathBuf, CoreError> {
    let target = autosave_path(project_path, data_dir)
        .ok_or_else(|| CoreError::Io("aucun emplacement de copie de secours".into()))?;
    if let Some(dir) = target.parent().filter(|d| !d.as_os_str().is_empty()) {
        fs::create_dir_all(dir).map_err(|e| CoreError::Io(format!("{} : {e}", dir.display())))?;
    }
    persist::write_atomic(&target, &persist::to_json(project)?)?;
    Ok(target)
}

fn modified(path: &Path) -> Option<SystemTime> {
    fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// Cherche une reprise à proposer. Une copie plus ancienne que le fichier
/// principal est périmée (le principal contient déjà mieux) : elle est
/// supprimée. Aucune écriture sur le fichier principal.
pub fn find_recovery(project_path: Option<&Path>, data_dir: Option<&Path>) -> RecoveryCheck {
    let Some(autosave) = autosave_path(project_path, data_dir) else {
        return RecoveryCheck::None;
    };
    let Some(auto_time) = modified(&autosave) else {
        return RecoveryCheck::None;
    };
    if let Some(main_time) = project_path.and_then(modified) {
        if auto_time <= main_time {
            let _ = fs::remove_file(&autosave);
            return RecoveryCheck::None;
        }
    }
    match persist::load(&autosave) {
        Ok(project) => RecoveryCheck::Available(Recovery { autosave, project }),
        Err(e) => {
            let mut name = autosave.as_os_str().to_owned();
            name.push(".corrupt");
            let moved_to = PathBuf::from(name);
            let _ = fs::remove_file(&moved_to);
            let moved_to = if fs::rename(&autosave, &moved_to).is_ok() {
                moved_to
            } else {
                autosave.clone()
            };
            RecoveryCheck::Corrupt {
                autosave,
                moved_to,
                detail: e.to_string(),
            }
        }
    }
}

/// Supprime la copie de secours (enregistrement normal, abandon explicite).
/// Sans copie : sans effet. Ne touche jamais au fichier principal.
pub fn discard_recovery(project_path: Option<&Path>, data_dir: Option<&Path>) {
    if let Some(p) = autosave_path(project_path, data_dir) {
        let _ = fs::remove_file(p);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Project;
    use std::time::Duration;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> TempDir {
            let dir = std::env::temp_dir().join(format!(
                "pf-recovery-test-{}-{tag}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).expect("dossier temporaire");
            TempDir(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn project(name: &str) -> Project {
        Project::new(name)
    }

    /// Force la date de modification (les écritures consécutives peuvent
    /// tomber dans la même granularité d'horloge).
    fn set_age(path: &Path, secs_ago: u64) {
        let t = SystemTime::now() - Duration::from_secs(secs_ago);
        fs::File::options()
            .write(true)
            .open(path)
            .expect("ouverture")
            .set_modified(t)
            .expect("date");
    }

    #[test]
    fn la_copie_est_a_cote_du_principal_et_ne_le_modifie_pas() {
        let t = TempDir::new("cote");
        let main = t.0.join("robe.pforge.json");
        persist::save(&main, &project("Robe")).expect("principal");
        let before = fs::read(&main).expect("lecture");
        let auto = write_autosave(Some(&main), None, &project("Robe modifiée")).expect("copie");
        assert_eq!(auto, t.0.join("robe.pforge.json.autosave"));
        assert_eq!(fs::read(&main).expect("lecture"), before);
        assert_eq!(
            persist::load(&auto).expect("relecture").name,
            "Robe modifiée"
        );
        assert!(!t.0.join("robe.pforge.json.autosave.tmp").exists());
    }

    #[test]
    fn sans_chemin_la_copie_va_dans_le_dossier_de_donnees() {
        let t = TempDir::new("donnees");
        let auto = write_autosave(None, Some(&t.0), &project("Neuf")).expect("copie");
        assert_eq!(auto, t.0.join("recovery").join(UNSAVED_NAME));
        assert!(matches!(
            find_recovery(None, Some(&t.0)),
            RecoveryCheck::Available(r) if r.project.name == "Neuf"
        ));
        assert!(write_autosave(None, None, &project("x")).is_err());
        assert!(matches!(find_recovery(None, None), RecoveryCheck::None));
    }

    #[test]
    fn reprise_proposee_si_la_copie_est_plus_recente() {
        let t = TempDir::new("recente");
        let main = t.0.join("a.pforge.json");
        persist::save(&main, &project("Ancien")).expect("principal");
        set_age(&main, 100);
        write_autosave(Some(&main), None, &project("Récent")).expect("copie");
        match find_recovery(Some(&main), None) {
            RecoveryCheck::Available(r) => assert_eq!(r.project.name, "Récent"),
            other => panic!("reprise attendue : {other:?}"),
        }
        // Chercher n'efface rien.
        assert!(autosave_path(Some(&main), None).expect("chemin").exists());
    }

    #[test]
    fn copie_perimee_non_proposee_et_supprimee() {
        let t = TempDir::new("perimee");
        let main = t.0.join("a.pforge.json");
        write_autosave(Some(&main), None, &project("Vieux")).expect("copie");
        set_age(&autosave_path(Some(&main), None).expect("chemin"), 100);
        persist::save(&main, &project("Principal")).expect("principal");
        assert!(matches!(
            find_recovery(Some(&main), None),
            RecoveryCheck::None
        ));
        assert!(!autosave_path(Some(&main), None).expect("chemin").exists());
        assert_eq!(persist::load(&main).expect("principal").name, "Principal");
    }

    #[test]
    fn ecriture_interrompue_principal_intact_et_reprise_offerte() {
        let t = TempDir::new("tronquee");
        let main = t.0.join("a.pforge.json");
        persist::save(&main, &project("Principal")).expect("principal");
        let before = fs::read(&main).expect("lecture");
        set_age(&main, 100);
        // Première copie complète, puis mort du processus pendant la suivante :
        // un temporaire tronqué reste, la copie précédente est intacte.
        write_autosave(Some(&main), None, &project("Copie 1")).expect("copie");
        let full = persist::to_json(&project("Copie 2")).expect("json");
        let tmp = t.0.join("a.pforge.json.autosave.tmp1234");
        fs::write(&tmp, &full[..full.len() / 2]).expect("temporaire tronqué");
        assert_eq!(fs::read(&main).expect("lecture"), before);
        match find_recovery(Some(&main), None) {
            RecoveryCheck::Available(r) => assert_eq!(r.project.name, "Copie 1"),
            other => panic!("reprise attendue : {other:?}"),
        }
        // Mort pendant la toute première copie : seul le temporaire existe,
        // rien n'est proposé, le principal reste intact.
        discard_recovery(Some(&main), None);
        assert!(matches!(
            find_recovery(Some(&main), None),
            RecoveryCheck::None
        ));
        assert_eq!(fs::read(&main).expect("lecture"), before);
    }

    #[test]
    fn copie_corrompue_ignoree_avec_message_sans_perte() {
        let t = TempDir::new("corrompue");
        let main = t.0.join("a.pforge.json");
        persist::save(&main, &project("Principal")).expect("principal");
        set_age(&main, 100);
        let auto = autosave_path(Some(&main), None).expect("chemin");
        fs::write(&auto, "{\"format\": \"patternforge").expect("copie tronquée");
        match find_recovery(Some(&main), None) {
            RecoveryCheck::Corrupt {
                moved_to, detail, ..
            } => {
                assert!(!detail.is_empty());
                assert!(moved_to.exists(), "le contenu est conservé");
                assert!(!auto.exists());
            }
            other => panic!("copie corrompue attendue : {other:?}"),
        }
        assert!(matches!(
            find_recovery(Some(&main), None),
            RecoveryCheck::None
        ));
        assert_eq!(persist::load(&main).expect("principal").name, "Principal");
    }

    #[test]
    fn l_enregistrement_normal_supprime_la_copie() {
        let t = TempDir::new("supprime");
        let main = t.0.join("a.pforge.json");
        write_autosave(Some(&main), None, &project("Brouillon")).expect("copie");
        write_autosave(None, Some(&t.0), &project("Brouillon")).expect("copie");
        persist::save(&main, &project("Final")).expect("enregistrement");
        discard_recovery(Some(&main), None);
        discard_recovery(None, Some(&t.0));
        assert!(!autosave_path(Some(&main), None).expect("chemin").exists());
        assert!(!autosave_path(None, Some(&t.0)).expect("chemin").exists());
        // Sans copie : sans effet ni erreur.
        discard_recovery(Some(&main), None);
        assert_eq!(persist::load(&main).expect("principal").name, "Final");
    }
}
