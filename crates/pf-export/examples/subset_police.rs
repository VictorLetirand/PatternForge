//! Produit le sous-ensemble de Noto Sans Regular livré dans `assets/fonts/` (US-06.6) :
//! `cargo run -p pf-export --example subset_police -- NotoSans-Regular.ttf sortie.ttf`
//! (premier argument : police complète téléchargée de la source officielle, voir
//! `assets/fonts/README.md`). Aucun outil tiers : le sous-ensembleur est celui du crate.

use pf_export::font::Face;
use std::collections::BTreeSet;

/// Plages conservées : latin de base et Latin-1, Latin étendu A, ponctuation courante,
/// monnaies, moins, grec (angles et symboles). Noto Sans n'a ni flèches ni ≤ ≥ ≠ ≈ ∞ : ces
/// caractères sont écrits « ? » dans le PDF.
const RANGES: &[(u32, u32)] = &[
    (0x0020, 0x007E),
    (0x00A0, 0x00FF),
    (0x0100, 0x017F),
    (0x0192, 0x0192),
    (0x02C6, 0x02C6),
    (0x02DC, 0x02DC),
    (0x0391, 0x03A9),
    (0x03B1, 0x03C9),
    (0x2010, 0x2015),
    (0x2016, 0x2016),
    (0x2018, 0x201E),
    (0x2020, 0x2022),
    (0x2026, 0x2026),
    (0x2030, 0x2030),
    (0x2032, 0x2033),
    (0x2039, 0x203A),
    (0x20AC, 0x20AC),
    (0x2122, 0x2122),
    (0x2206, 0x2206),
    (0x2212, 0x2212),
];

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let (input, output) = match args.as_slice() {
        [_, i, o] => (i, o),
        _ => return Err("usage : subset_police <NotoSans-Regular.ttf> <sortie.ttf>".into()),
    };
    let data = std::fs::read(input)?;
    let face = Face::parse(&data).ok_or("police illisible")?;
    let chars: BTreeSet<char> = RANGES
        .iter()
        .flat_map(|(a, b)| (*a..=*b).filter_map(char::from_u32))
        .filter(|c| face.glyph(*c).is_some())
        .collect();
    let out = face.standalone_subset(&chars);
    std::fs::write(output, &out)?;
    println!("{} caractères, {} octets", chars.len(), out.len());
    Ok(())
}
