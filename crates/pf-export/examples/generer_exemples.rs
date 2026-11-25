//! Régénère les fichiers du dossier `examples/` :
//! `cargo run -p pf-export --example generer_exemples`

use pf_core::{Command, PathRole, PointRef};
use pf_export::{pdf, svg, Scene};
use pf_geometry::PointMm;
use std::fs;
use std::path::Path;

#[path = "../tests/scenario_jupe/construction.rs"]
mod construction;
#[path = "../tests/scenario_porte_cartes/construction.rs"]
mod porte_cartes;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    fs::create_dir_all(&dir)?;

    let project = pf_core::demo_project()?;
    pf_core::persist::save(&dir.join("geometrie-exercice.pforge.json"), &project)?;

    let scene = Scene::from_sheet(&project, project.sheets[0].id)?;
    fs::write(dir.join("geometrie-exercice.svg"), svg::export_svg(&scene)?)?;
    let out = pdf::export_pdf(std::slice::from_ref(&scene))?;
    fs::write(dir.join("geometrie-exercice.pdf"), &out.bytes)?;
    println!("geometrie-exercice.pdf : {} page(s)", out.page_count);

    fs::write(
        dir.join("etalon-100mm.pdf"),
        pdf::export_calibration_pdf().bytes,
    )?;
    fs::write(
        dir.join("etalon-100mm.svg"),
        svg::export_svg(&Scene::calibration_square())?,
    )?;

    // Pièce plus grande qu'un A4 : rectangle de 600 × 450 mm en ligne de coupe (AT-11).
    let mut big = pf_core::Project::new("Grande pièce 600 × 450");
    let sheet = big.sheets[0].id;
    big.apply(Command::RenameSheet {
        sheet,
        name: "Panneau".into(),
    })?;
    let corners = [(0.0, 0.0), (600.0, 0.0), (600.0, 450.0), (0.0, 450.0)];
    for i in 0..4 {
        let (a, b) = (corners[i], corners[(i + 1) % 4]);
        let out = big.apply(Command::AddSegment {
            sheet,
            from: PointRef::New(PointMm::new(a.0, a.1)),
            to: PointRef::New(PointMm::new(b.0, b.1)),
        })?;
        big.apply(Command::SetPathRole {
            sheet,
            entity: out.entities[0],
            role: PathRole::CuttingContour,
        })?;
    }
    big.apply(Command::AddPiece {
        sheet,
        name: "Panneau".into(),
        quantity: 2,
        material: Some("Toile".into()),
        mirrored_pair: false,
    })?;
    pf_core::persist::save(&dir.join("grande-piece-600x450.pforge.json"), &big)?;
    let out = pdf::export_pdf(&[Scene::from_sheet(&big, sheet)?])?;
    fs::write(dir.join("grande-piece-600x450.pdf"), &out.bytes)?;
    println!("grande-piece-600x450.pdf : {} page(s)", out.page_count);

    // Pièce avec crans et droit-fil.
    let piece = pf_core::demo_piece_project()?;
    pf_core::persist::save(&dir.join("piece-crans-droit-fil.pforge.json"), &piece)?;
    let scene = Scene::from_sheet(&piece, piece.sheets[0].id)?;
    fs::write(
        dir.join("piece-crans-droit-fil.svg"),
        svg::export_svg(&scene)?,
    )?;
    let out = pdf::export_pdf(&[scene])?;
    fs::write(dir.join("piece-crans-droit-fil.pdf"), &out.bytes)?;
    println!("piece-crans-droit-fil.pdf : {} page(s)", out.page_count);

    // Jupe droite de base (PH-3a) : scénario par commandes partagé avec son test.
    let jupe = construction::build()?;
    pf_core::persist::save(&dir.join("jupe-droite.pforge.json"), &jupe.project)?;
    let scene = Scene::from_sheet(&jupe.project, jupe.sheet)?;
    fs::write(dir.join("jupe-droite.svg"), svg::export_svg(&scene)?)?;
    fs::write(
        dir.join("jupe-droite.dxf"),
        pf_export::dxf::export_dxf(&jupe.project)?.text,
    )?;
    fs::write(
        dir.join("jupe-droite.hpgl"),
        pf_export::hpgl::export_hpgl(&scene, &construction::hpgl_page())?.text,
    )?;
    let out = pdf::export_pdf(&[scene])?;
    fs::write(dir.join("jupe-droite.pdf"), &out.bytes)?;
    println!("jupe-droite.pdf : {} page(s)", out.page_count);

    // Porte-cartes en cuir (PH-3b) : scénario par commandes partagé avec son test.
    let pc = porte_cartes::build()?;
    pf_core::persist::save(&dir.join("porte-cartes.pforge.json"), &pc.project)?;
    let scene = Scene::from_sheet(&pc.project, pc.sheet)?;
    fs::write(dir.join("porte-cartes.svg"), svg::export_svg(&scene)?)?;
    fs::write(
        dir.join("porte-cartes.dxf"),
        pf_export::dxf::export_dxf(&pc.project)?.text,
    )?;
    fs::write(
        dir.join("porte-cartes.hpgl"),
        pf_export::hpgl::export_hpgl(&scene, &porte_cartes::hpgl_page())?.text,
    )?;
    let out = pdf::export_pdf(&[scene])?;
    fs::write(dir.join("porte-cartes.pdf"), &out.bytes)?;
    println!("porte-cartes.pdf : {} page(s)", out.page_count);

    println!("Exemples écrits dans {}", dir.canonicalize()?.display());
    Ok(())
}
