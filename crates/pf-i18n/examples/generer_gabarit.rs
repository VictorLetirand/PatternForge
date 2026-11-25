//! Génère le gabarit de fichier de langue : `cargo run -q -p pf-i18n --example generer_gabarit`.
//! Écrit `docs/langues/gabarit.lang.json` (comparé par `tests/langues.rs`).

fn main() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("docs")
        .join("langues")
        .join("gabarit.lang.json");
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).expect("dossier docs/langues");
    }
    std::fs::write(&path, pf_i18n::langfile::template_json()).expect("écriture du gabarit");
    println!("{}", path.display());
}
