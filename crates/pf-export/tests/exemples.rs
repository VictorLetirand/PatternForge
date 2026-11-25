//! Les fichiers livrés dans `examples/` doivent rester ouvrables et à jour.

use pf_export::{pdf, svg, Scene};
use std::fs;
use std::path::PathBuf;

fn examples() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples")
}

const HINT: &str = "régénérer avec : cargo run -p pf-export --example generer_exemples";

#[test]
fn projet_exemple_ouvrable_et_a_jour() {
    let path = examples().join("geometrie-exercice.pforge.json");
    let loaded = pf_core::persist::load(&path).expect("le projet d'exemple doit s'ouvrir");
    assert_eq!(loaded, pf_core::demo_project().unwrap(), "{HINT}");
}

#[test]
fn exports_exemples_a_jour() {
    let project = pf_core::demo_project().unwrap();
    let scene = Scene::from_sheet(&project, project.sheets[0].id).unwrap();
    let read =
        |name: &str| fs::read(examples().join(name)).unwrap_or_else(|e| panic!("{name} : {e}"));
    assert_eq!(
        read("geometrie-exercice.svg"),
        svg::export_svg(&scene).unwrap().into_bytes(),
        "{HINT}"
    );
    assert_eq!(
        read("geometrie-exercice.pdf"),
        pdf::export_pdf(&[scene]).unwrap().bytes,
        "{HINT}"
    );
    assert_eq!(
        read("etalon-100mm.pdf"),
        pdf::export_calibration_pdf().bytes,
        "{HINT}"
    );
}

#[test]
fn piece_avec_crans_et_droit_fil_a_jour_et_complete() {
    let project = pf_core::demo_piece_project().unwrap();
    let loaded =
        pf_core::persist::load(&examples().join("piece-crans-droit-fil.pforge.json")).unwrap();
    assert_eq!(loaded, project, "{HINT}");
    let scene = Scene::from_sheet(&project, project.sheets[0].id).unwrap();
    let read = |name: &str| fs::read(examples().join(name)).unwrap();
    let svg_text = svg::export_svg(&scene).unwrap();
    assert_eq!(
        read("piece-crans-droit-fil.svg"),
        svg_text.clone().into_bytes(),
        "{HINT}"
    );
    assert_eq!(
        read("piece-crans-droit-fil.pdf"),
        pdf::export_pdf(&[scene]).unwrap().bytes,
        "{HINT}"
    );
    // Trois crans dont un double : quatre traits de 5 mm, étiquetés « cran ».
    let (_, after) = svg_text
        .split_once("data-role=\"cran\"")
        .expect("groupe des crans");
    let group = after.split("</g>").next().unwrap();
    assert_eq!(group.matches("<line").count(), 4, "{group}");
    // Droit-fil : le trait et ses deux pointes (1 + 4 lignes).
    assert_eq!(svg_text.matches("data-role=\"droit-fil\"").count(), 5);
    assert!(svg_text.contains("data-role=\"coupe\""));
    // Deux cotations : valeurs mesurées affichées en texte.
    assert!(svg_text.contains("data-role=\"cotation\""));
    assert!(svg_text.contains(">170.00 mm</text>") && svg_text.contains(">120.00 mm</text>"));
}
