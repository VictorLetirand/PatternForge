//! Régénère `examples/aide/sac-cabas.pforge` (US-16.9) :
//! `cargo run -p pf-export --example generer_exemple_sac`

use std::path::Path;

#[path = "../tests/scenario_sac/construction.rs"]
mod construction;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    pf_core::persist::save(&dir.join("aide/sac-cabas.pforge"), &construction::build()?)?;
    Ok(())
}
