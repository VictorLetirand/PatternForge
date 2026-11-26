//! Exemple de sac (US-16.9) : le fichier d'exemple est celui du scénario, et il se valide.

use pf_core::persist;
use std::path::PathBuf;

#[path = "scenario_sac/construction.rs"]
mod construction;

fn file() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/aide/sac-cabas.pforge")
}

#[test]
fn fichier_d_exemple_a_jour() {
    let built = construction::build().unwrap();
    let tmp = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "pf_sac_cabas-{}-{:?}.pforge.json",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_file(&tmp);
    persist::save(&tmp, &built).unwrap();
    let want = std::fs::read(&tmp).unwrap();
    let _ = std::fs::remove_file(&tmp);
    let have = std::fs::read(file()).expect("examples/aide/sac-cabas.pforge");
    assert!(
        want == have,
        "régénérer : cargo run -p pf-export --example generer_exemple_sac"
    );
}

#[test]
fn le_sac_se_relit_avec_deux_pieces_completes() {
    let p = persist::load(&file()).unwrap();
    assert_eq!(p.sheets.len(), 2);
    for sheet in &p.sheets {
        let pieces = p.pieces_on(sheet.id);
        assert_eq!(pieces.len(), 1);
        let piece = pieces[0];
        assert!(piece.grainline.is_some());
        assert!(piece.code.is_some());
    }
}
