//! Aides communes des tests de la ligne de commande : projets, dossiers temporaires, lancement du
//! vrai binaire.

#![allow(dead_code)]

use pf_core::*;
use pf_geometry::PointMm;
use std::path::{Path, PathBuf};
use std::process::{Command as Proc, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// Dossier temporaire neuf (supprimé par l'appelant s'il le souhaite ; le dossier système est
/// nettoyé par le système).
pub fn temp_dir(tag: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir =
        std::env::temp_dir().join(format!("pf-cli-test-{}-{}-{}", std::process::id(), tag, n));
    // Un identifiant de processus réutilisé peut laisser un dossier d'un passage précédent.
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

pub fn run(args: &[&str]) -> Output {
    Proc::new(env!("CARGO_BIN_EXE_patternforge-cli"))
        .args(args)
        .output()
        .expect("le binaire se lance")
}

pub fn run_s(args: &[String]) -> Output {
    Proc::new(env!("CARGO_BIN_EXE_patternforge-cli"))
        .args(args)
        .output()
        .expect("le binaire se lance")
}

pub fn code(o: &Output) -> i32 {
    o.status
        .code()
        .expect("le processus ne s'est pas terminé par un signal")
}

pub fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

pub fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

pub fn p(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// Carré de `side` mm dont le coin bas gauche est en (0 ; 0), une feuille. Avec `sizes`, une
/// gamme S, M (base), L dont chaque sommet de L est décalé de (+10 ; +10) mm sauf le premier.
pub fn square_project(name: &str, side: f64, sizes: bool) -> Project {
    let mut project = Project::new(name);
    let sheet = project.sheets[0].id;
    let poly = project
        .apply(Command::AddPolygon {
            sheet,
            vertices: [(0.0, 0.0), (side, 0.0), (side, side), (0.0, side)]
                .iter()
                .map(|&(x, y)| PointRef::New(PointMm::new(x, y)))
                .collect(),
            closed: true,
            role: PathRole::Construction,
            layer: None,
        })
        .unwrap();
    if sizes {
        project
            .apply(Command::AddSizeRange {
                name: "Gamme".into(),
                kind: SizeKind::Alphanumeric,
                labels: vec!["S".into(), "M".into(), "L".into()],
                base: 1,
            })
            .unwrap();
        let l = project.sizes[0].sizes[2].id;
        for corner in poly.points.iter().skip(1) {
            project
                .apply(Command::SetPointGrading {
                    sheet,
                    point: *corner,
                    size: l,
                    dx_mm: 10.0,
                    dy_mm: 10.0,
                })
                .unwrap();
        }
    }
    project
}

pub fn save(project: &Project, path: &Path) {
    persist::save(path, project).unwrap();
}

/// Fichier d'exemple du dépôt.
pub fn example(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("examples")
        .join(name)
}

/// Générateur congruentiel à graine fixe.
pub struct Rng(pub u64);

impl Rng {
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }

    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}
