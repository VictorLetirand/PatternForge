//! Séparateur décimal d'affichage : réglage par fil d'exécution, sans effet sur la saisie.

use pf_core::input::{
    decimal_separator, format_deg, format_mm, format_number, parse_length_mm, set_decimal_separator,
};

#[test]
fn point_decimal_en_affichage() {
    set_decimal_separator('.');
    assert_eq!(decimal_separator(), '.');
    assert_eq!(format_mm(120.0), "120.0 mm");
    assert_eq!(format_mm(141.421_356), "141.42 mm");
    assert_eq!(format_deg(90.0), "90.0°");
    assert_eq!(format_number(-0.000_01, 2), "0.0");
    assert_eq!(format_number(f64::NAN, 2), "—");
    // La saisie accepte toujours les deux formes.
    assert_eq!(parse_length_mm("120,5"), Ok(120.5));
    assert_eq!(parse_length_mm("120.5"), Ok(120.5));
}

#[test]
fn defaut_virgule_et_retour_a_la_virgule() {
    // Chaque test tourne dans son propre fil : le défaut est la virgule.
    assert_eq!(decimal_separator(), ',');
    assert_eq!(format_mm(42.5), "42,5 mm");
    set_decimal_separator('.');
    set_decimal_separator(',');
    assert_eq!(format_mm(42.5), "42,5 mm");
}

#[test]
fn le_reglage_est_propre_a_chaque_fil() {
    set_decimal_separator('.');
    let other = std::thread::spawn(|| (decimal_separator(), format_mm(1.5)))
        .join()
        .expect("fil de test");
    assert_eq!(other, (',', "1,5 mm".to_string()));
    assert_eq!(format_mm(1.5), "1.5 mm");
}
