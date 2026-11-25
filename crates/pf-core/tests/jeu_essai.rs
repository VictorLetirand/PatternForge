//! Jeu d'essai : trois projets validés par le **vrai** `pf-validate` (code 0), sans donnée
//! personnelle ni professionnelle.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/jeu_essai")
}

const FILES: [&str; 3] = [
    "jupe-droite-tailles.pforge.json",
    "porte-cartes-piqure.pforge.json",
    "ensemble-variantes-plan.pforge.json",
];

#[test]
fn les_trois_projets_sont_valides_pour_pf_validate() {
    for f in FILES {
        let out = Command::new(env!("CARGO_BIN_EXE_pf-validate"))
            .arg(dir().join(f))
            .output()
            .unwrap();
        assert_eq!(
            out.status.code(),
            Some(0),
            "{f} : {}",
            String::from_utf8_lossy(&out.stdout)
        );
    }
}

#[test]
fn contenu_des_projets_correspond_a_la_demande() {
    let jupe = pf_core::persist::load(&dir().join(FILES[0])).unwrap();
    assert_eq!(jupe.sizes.len(), 1);
    let labels: Vec<&str> = jupe.sizes[0]
        .sizes
        .iter()
        .map(|s| s.label.as_str())
        .collect();
    assert_eq!(labels, ["S", "M", "L"]);
    assert!(
        jupe.pieces.iter().any(|p| p.seam.is_some()),
        "marges de couture"
    );

    let pc = pf_core::persist::load(&dir().join(FILES[1])).unwrap();
    let piece = pc.pieces.iter().find(|p| !p.stitch_lines.is_empty());
    assert!(piece.is_some(), "une ligne de piqûre");

    let ens = pf_core::persist::load(&dir().join(FILES[2])).unwrap();
    assert_eq!(ens.variants.len(), 1);
    assert_eq!(ens.cutting_plans.len(), 1);
    assert_eq!(ens.cutting_plans[0].placements.len(), 3);
}

#[test]
fn aucune_donnee_personnelle_ni_chemin_dans_les_projets() {
    for f in FILES {
        let text = fs::read_to_string(dir().join(f)).unwrap();
        for banned in ["C:\\", "C:/", "Users", "@", "http", "\\\\", "/home/"] {
            assert!(!text.contains(banned), "{f} contient « {banned} »");
        }
    }
}
