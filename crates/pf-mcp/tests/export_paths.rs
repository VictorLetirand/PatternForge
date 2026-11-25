//! A8 (audit du 25/11) : chemins d'export hostiles de l'outil `export` : jonctions et liens, flux
//! NTFS (`a:b`), noms terminés par un point ou une espace, dossiers `.assets` et `.versions`.
//!
//! Oracle indépendant du code testé : (1) chaque fragment de chemin est étiqueté sain ou hostile à
//! la construction ; (2) l'arbre complet (projet **et** dossier voisin « dehors ») est relu avant et
//! après chaque appel : un refus ne laisse aucune trace, un export accepté ne crée que des SVG dans
//! le projet, hors des dossiers protégés, et ne modifie rien d'existant.
//!
//! Les jonctions sont créées par `mklink /J` (aucun droit particulier) ; ailleurs, liens
//! symboliques.

use pf_mcp::Server;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

struct Sandbox {
    root: PathBuf,
    server: Server,
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        // `remove_dir_all` retire les jonctions sans suivre leur cible.
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[cfg(windows)]
fn link_dir(link: &Path, target: &Path) {
    let plain = |p: &Path| p.display().to_string().replace('/', "\\");
    let out = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J", &plain(link), &plain(target)])
        .output()
        .expect("cmd");
    assert!(out.status.success(), "mklink /J : {out:?}");
}

#[cfg(unix)]
fn link_dir(link: &Path, target: &Path) {
    std::os::unix::fs::symlink(target, link).expect("lien");
}

fn sandbox(tag: &str) -> Sandbox {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("export_paths-{}-{tag}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let project = root.join("projet");
    for dir in [
        "dehors",
        "projet/demo.assets",
        "projet/demo.pforge.versions",
        "projet/sortie",
    ] {
        fs::create_dir_all(root.join(dir)).unwrap();
    }
    for file in [
        "dehors/temoin.svg",
        "projet/demo.assets/temoin.svg",
        "projet/demo.pforge.versions/temoin.svg",
    ] {
        fs::write(root.join(file), b"temoin").unwrap();
    }
    let file = project.join("demo.pforge.json");
    pf_core::persist::save(&file, &pf_core::demo_project().expect("démo")).unwrap();
    link_dir(&project.join("vers_dehors"), &root.join("dehors"));
    link_dir(&project.join("vers_assets"), &project.join("demo.assets"));
    link_dir(
        &project.join("vers_versions"),
        &project.join("demo.pforge.versions"),
    );
    link_dir(&project.join("vers_sortie"), &project.join("sortie"));
    fs::create_dir_all(root.join("disparu")).unwrap();
    link_dir(&project.join("cassee"), &root.join("disparu"));
    fs::remove_dir(root.join("disparu")).unwrap();
    Sandbox {
        server: Server::with_write(file),
        root,
    }
}

/// Entrées de l'arbre, liens non suivis : chemin relatif -> contenu (`None` = dossier ou lien).
fn snapshot(root: &Path) -> BTreeMap<String, Option<Vec<u8>>> {
    fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<String, Option<Vec<u8>>>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            let name = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            let meta = fs::symlink_metadata(&path).unwrap();
            if meta.is_file() {
                out.insert(name, Some(fs::read(&path).unwrap()));
            } else {
                out.insert(name, None);
                if meta.is_dir() && !meta.file_type().is_symlink() {
                    walk(root, &path, out);
                }
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(root, root, &mut out);
    out
}

impl Sandbox {
    fn export(&mut self, path: &str) -> Result<(), String> {
        let line = json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{
            "name":"export","arguments":{"format":"svg","path":path,"overwrite":true}}})
        .to_string();
        let out = self.server.handle_line(&line).expect("réponse");
        let r: Value = serde_json::from_str(&out).expect("JSON");
        assert!(r.get("error").is_none(), "{path:?} : {r}");
        if r["result"]["isError"] == false {
            Ok(())
        } else {
            Err(r["result"]["structuredContent"]["error"]
                .as_str()
                .expect("message")
                .to_string())
        }
    }

    /// Refus attendu : message, et rien de créé, modifié ni supprimé nulle part.
    fn refused(&mut self, path: &str) -> String {
        let before = snapshot(&self.root);
        let result = self.export(path);
        let after = snapshot(&self.root);
        assert!(result.is_err(), "{path:?} aurait dû être refusé");
        assert_eq!(after, before, "{path:?} : le refus a laissé une trace");
        result.unwrap_err()
    }

    /// Export attendu : seuls de nouveaux dossiers et SVG dans le projet, hors dossiers protégés.
    /// Ils sont retirés ensuite.
    fn accepted(&mut self, path: &str) {
        let before = snapshot(&self.root);
        if let Err(e) = self.export(path) {
            panic!("{path:?} aurait dû être accepté : {e}");
        }
        let after = snapshot(&self.root);
        for (name, content) in &before {
            assert_eq!(after.get(name), Some(content), "{path:?} : {name} touché");
        }
        let new: Vec<(&String, &Option<Vec<u8>>)> = after
            .iter()
            .filter(|(name, _)| !before.contains_key(*name))
            .collect();
        assert!(
            new.iter().any(|(_, c)| c.is_some()),
            "{path:?} : rien d'écrit"
        );
        for (name, content) in new.iter().rev() {
            let lower = name.to_lowercase();
            assert!(lower.starts_with("projet/"), "{path:?} : {name}");
            assert!(
                !lower.contains(".assets") && !lower.contains(".versions"),
                "{path:?} : {name}"
            );
            let full = self.root.join(name);
            match content {
                Some(bytes) => {
                    assert!(
                        bytes.starts_with(b"<?xml"),
                        "{path:?} : {name} n'est pas un SVG"
                    );
                    fs::remove_file(full).unwrap();
                }
                None => fs::remove_dir(full).unwrap(),
            }
        }
        assert_eq!(snapshot(&self.root), before);
    }
}

#[test]
fn une_jonction_qui_sort_du_projet_est_refusee() {
    let mut s = sandbox("dehors");
    for path in [
        "vers_dehors/x.svg",
        "vers_dehors/temoin.svg",
        "vers_dehors/nouveau/x.svg",
        "vers_dehors",
    ] {
        let why = s.refused(path);
        assert!(why.contains("chemin d'export refusé"), "{path} : {why}");
    }
    let why = s.refused("cassee/x.svg");
    assert!(why.contains("chemin d'export refusé"), "{why}");
}

#[test]
fn un_flux_ntfs_est_refuse() {
    let mut s = sandbox("flux");
    let absolute = format!("{}/projet/a.svg:flux", s.root.display());
    for path in [
        "a.svg:flux",
        "sortie/a.svg:flux",
        "demo.pforge.json::$DATA",
        "demo.pforge.json:flux",
        "sortie::$INDEX_ALLOCATION/a.svg",
        absolute.as_str(),
    ] {
        let why = s.refused(path);
        assert!(why.contains("chemin d'export refusé"), "{path} : {why}");
    }
}

#[test]
fn les_dossiers_du_fond_et_des_versions_sont_refuses() {
    let mut s = sandbox("proteges");
    for path in [
        "demo.assets/x.svg",
        "demo.assets/temoin.svg",
        "DEMO.Assets/temoin.svg",
        "demo.assets",
        "autre.assets/x.svg",
        "demo.pforge.versions/x.svg",
        "demo.pforge.versions/temoin.svg",
        "demo.pforge.versions",
        ".versions/x.svg",
        "sortie/x.versions/x.svg",
        "vers_assets/x.svg",
        "vers_assets/temoin.svg",
        "vers_versions/temoin.svg",
        "vers_versions",
    ] {
        let why = s.refused(path);
        assert!(why.contains("chemin d'export refusé"), "{path} : {why}");
    }
}

#[test]
fn un_point_ou_une_espace_finale_ne_contourne_rien() {
    let mut s = sandbox("finale");
    for path in [
        "demo.pforge.json.",
        "demo.pforge.json ",
        "demo.pforge.json. .",
        "demo.assets./temoin.svg",
        "demo.assets /temoin.svg",
        "a.svg.",
    ] {
        let why = s.refused(path);
        assert!(why.contains("chemin d'export refusé"), "{path} : {why}");
    }
}

#[test]
fn les_chemins_ordinaires_restent_acceptes() {
    let mut s = sandbox("sains");
    let absolute = format!("{}/projet/sortie/abs.svg", s.root.display());
    for path in [
        "a.svg",
        "./a.svg",
        "sortie/a.svg",
        "nouveau/encore/a.svg",
        "sortie",
        "vers_sortie/a.svg",
        "vers_sortie",
        ".cache/a.svg",
        "mes assets/a.svg",
        absolute.as_str(),
    ] {
        s.accepted(path);
    }
}

/// Lien symbolique **de fichier** : exige un droit particulier sous Windows ; sans lui, le cas
/// n'est pas joué (les jonctions ci-dessus couvrent les dossiers).
#[test]
fn un_lien_de_fichier_qui_sort_du_projet_est_refuse() {
    let mut s = sandbox("lien-fichier");
    let target = s.root.join("dehors/temoin.svg");
    let link = s.root.join("projet/lien.svg");
    #[cfg(windows)]
    let made = std::os::windows::fs::symlink_file(&target, &link);
    #[cfg(unix)]
    let made = std::os::unix::fs::symlink(&target, &link);
    if let Err(e) = made {
        eprintln!("lien de fichier non créé ({e}) : cas non joué");
        return;
    }
    s.refused("lien.svg");
    assert_eq!(fs::read(target).unwrap(), b"temoin");
}

struct Rng(u64);

impl Rng {
    fn below(&mut self, n: usize) -> usize {
        // xorshift64
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % n as u64) as usize
    }
    fn pick<'a>(&mut self, list: &[&'a str]) -> &'a str {
        list[self.below(list.len())]
    }
}

#[test]
fn campagne_adverse_a_graine_fixe() {
    // Fragments sains / hostiles. Les liens n'existent qu'à la racine du projet (première place).
    const FIRST_LINK_OK: [&str; 1] = ["vers_sortie"];
    const FIRST_LINK_BAD: [&str; 4] = ["vers_dehors", "vers_assets", "vers_versions", "cassee"];
    const DIR_OK: [&str; 6] = ["sortie", "nouveau", "Été 2", "a.b", ".cache", "mes assets"];
    const DIR_BAD: [&str; 9] = [
        "..",
        "demo.assets",
        "Demo.ASSETS",
        "demo.pforge.versions",
        "autre.Versions",
        "d:flux",
        "demo.assets.",
        "sortie ",
        "CON",
    ];
    const FILE_OK: [&str; 4] = ["a.svg", "plan v2.svg", "É.svg", "b.SVG"];
    const FILE_BAD: [&str; 12] = [
        "a.svg:flux",
        "a.svg::$DATA",
        "demo.pforge.json",
        "demo.pforge.json.",
        "demo.pforge.json ",
        "demo.pforge.json::$DATA",
        "x.JSON",
        "y.lock",
        "z.svg.",
        "w.svg ",
        "NUL.svg",
        "..",
    ];
    let mut s = sandbox("campagne");
    let project = s.root.join("projet").display().to_string();
    let mut rng = Rng(0x5EED_2026_1125_0008);
    let (mut accepted, mut refused, mut absolute, mut folder) = (0, 0, 0, 0);
    let (mut seen_bad_dir, mut seen_bad_file, mut seen_bad_link) = (0, 0, 0);
    for case in 0..420 {
        // Une fois sur deux, tout est sain : les acceptations sont aussi mises à l'épreuve.
        let hostile_allowed = case % 2 == 0;
        let mut hostile = false;
        let mut parts: Vec<&str> = Vec::new();
        if rng.below(3) == 0 {
            if hostile_allowed && rng.below(2) == 0 {
                parts.push(rng.pick(&FIRST_LINK_BAD));
                hostile = true;
                seen_bad_link += 1;
            } else {
                parts.push(rng.pick(&FIRST_LINK_OK));
            }
        }
        for _ in 0..rng.below(3) {
            if hostile_allowed && rng.below(4) == 0 {
                parts.push(rng.pick(&DIR_BAD));
                hostile = true;
                seen_bad_dir += 1;
            } else {
                parts.push(rng.pick(&DIR_OK));
            }
        }
        // Sortie « dossier » (sans nom de fichier) ou fichier.
        if parts.is_empty() || rng.below(6) != 0 {
            if hostile_allowed && rng.below(2) == 0 {
                parts.push(rng.pick(&FILE_BAD));
                hostile = true;
                seen_bad_file += 1;
            } else {
                parts.push(rng.pick(&FILE_OK));
            }
        } else {
            folder += 1;
        }
        let separator = if cfg!(windows) && rng.below(3) == 0 {
            "\\"
        } else {
            "/"
        };
        let mut path = parts.join(separator);
        if rng.below(5) == 0 {
            path = format!("{project}{}{path}", std::path::MAIN_SEPARATOR);
            absolute += 1;
        }
        if hostile {
            s.refused(&path);
            refused += 1;
        } else {
            s.accepted(&path);
            accepted += 1;
        }
    }
    let seen = (accepted, refused, absolute, folder);
    assert!(
        accepted >= 200 && refused >= 100 && absolute >= 40 && folder >= 10,
        "{seen:?}"
    );
    assert!(
        seen_bad_dir >= 20 && seen_bad_file >= 60 && seen_bad_link >= 15,
        "{seen_bad_dir} {seen_bad_file} {seen_bad_link}"
    );
}
