//! Tous les tests d'intégration du paquet forment **un seul programme** : une seule édition de
//! liens, et les campagnes s'exécutent en parallèle sur tous les cœurs.
//!
//! Un nouveau fichier de test doit être ajouté à la liste ci-dessous (un test le vérifie).
//! Pour n'en lancer qu'un : `cargo test -p pf-export --test suite nom_du_fichier::`.

// Les modules partagés sont inclus par plusieurs fichiers, comme lorsqu'ils étaient séparés.
#![allow(clippy::duplicate_mod)]

mod common;

#[path = "adverse_cutting_export.rs"]
mod adverse_cutting_export;
#[path = "adverse_export_coupe.rs"]
mod adverse_export_coupe;
#[path = "adverse_export_gradue.rs"]
mod adverse_export_gradue;
#[path = "adverse_export_pince_etiquette.rs"]
mod adverse_export_pince_etiquette;
#[path = "adverse_hpgl_tuiles.rs"]
mod adverse_hpgl_tuiles;
#[path = "adverse_import_vectoriel.rs"]
mod adverse_import_vectoriel;
#[path = "adverse_langues.rs"]
mod adverse_langues;
#[path = "adverse_libelles.rs"]
mod adverse_libelles;
#[path = "adverse_mesures_export.rs"]
mod adverse_mesures_export;
#[path = "adverse_nomenclature.rs"]
mod adverse_nomenclature;
#[path = "adverse_police_pdf.rs"]
mod adverse_police_pdf;
#[path = "adverse_rapport.rs"]
mod adverse_rapport;
#[path = "arcs_export.rs"]
mod arcs_export;
#[path = "assemblage.rs"]
mod assemblage;
#[path = "assemblage_adverse.rs"]
mod assemblage_adverse;
#[path = "boutons.rs"]
mod boutons;
#[path = "contenu_sortie.rs"]
mod contenu_sortie;
#[path = "contenu_sortie_adverse.rs"]
mod contenu_sortie_adverse;
#[path = "cotes_completes.rs"]
mod cotes_completes;
#[path = "crans_avances.rs"]
mod crans_avances;
#[path = "crans_coin.rs"]
mod crans_coin;
#[path = "cuir_export.rs"]
mod cuir_export;
#[path = "cutting_export.rs"]
mod cutting_export;
#[path = "droit_fil.rs"]
mod droit_fil;
#[path = "durcissement_finitude.rs"]
mod durcissement_finitude;
#[path = "durcissement_imports.rs"]
mod durcissement_imports;
#[path = "dxf.rs"]
mod dxf;
#[path = "dxf_aama_import.rs"]
mod dxf_aama_import;
#[path = "dxf_adverse.rs"]
mod dxf_adverse;
#[path = "dxf_generique.rs"]
mod dxf_generique;
#[path = "dxf_generique_adverse.rs"]
mod dxf_generique_adverse;
#[path = "etalonnage.rs"]
mod etalonnage;
#[path = "etalonnage_adverse.rs"]
mod etalonnage_adverse;
#[path = "exemple_sac.rs"]
mod exemple_sac;
#[path = "exemples.rs"]
mod exemples;
#[path = "export_coupe.rs"]
mod export_coupe;
#[path = "export_coupe_nette.rs"]
mod export_coupe_nette;
#[path = "export_coupe_pince.rs"]
mod export_coupe_pince;
#[path = "export_gradue.rs"]
mod export_gradue;
#[path = "export_libelles.rs"]
mod export_libelles;
#[path = "export_pince_etiquette.rs"]
mod export_pince_etiquette;
#[path = "export_pliure.rs"]
mod export_pliure;
#[path = "export_sans_droit_fil.rs"]
mod export_sans_droit_fil;
#[path = "formats_page.rs"]
mod formats_page;
#[path = "gradation_etiquette.rs"]
mod gradation_etiquette;
#[path = "guides_export.rs"]
mod guides_export;
#[path = "hpgl.rs"]
mod hpgl;
#[path = "hpgl_adverse.rs"]
mod hpgl_adverse;
#[path = "hpgl_tuiles.rs"]
mod hpgl_tuiles;
#[path = "import_aller_retour.rs"]
mod import_aller_retour;
#[path = "import_pdf.rs"]
mod import_pdf;
#[path = "import_vectoriel.rs"]
mod import_vectoriel;
#[path = "layers_export.rs"]
mod layers_export;
#[path = "lignes_speciales.rs"]
mod lignes_speciales;
#[path = "marche_export.rs"]
mod marche_export;
#[path = "marquages.rs"]
mod marquages;
#[path = "mesures_tailles_export.rs"]
mod mesures_tailles_export;
#[path = "nomenclature.rs"]
mod nomenclature;
#[path = "nomenclature_variantes.rs"]
mod nomenclature_variantes;
#[path = "notch_anchor.rs"]
mod notch_anchor;
#[path = "percages_au_pas_export.rs"]
mod percages_au_pas_export;
#[path = "piqure_export.rs"]
mod piqure_export;
#[path = "police_pdf.rs"]
mod police_pdf;
#[path = "rapport_controle.rs"]
mod rapport_controle;
#[path = "rapport_mesures_finies.rs"]
mod rapport_mesures_finies;
#[path = "rapport_piqure.rs"]
mod rapport_piqure;
#[path = "rapport_pliure.rs"]
mod rapport_pliure;
#[path = "raster.rs"]
mod raster;
#[path = "scenario_jupe_droite.rs"]
mod scenario_jupe_droite;
#[path = "scenario_porte_cartes.rs"]
mod scenario_porte_cartes;
#[path = "splines_export.rs"]
mod splines_export;
#[path = "zones_export.rs"]
mod zones_export;

/// Fichiers déclarés comme programmes séparés dans `Cargo.toml`.
const SEPARES: &[&str] = &[];

#[test]
fn tous_les_fichiers_de_test_sont_inclus() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let suite = include_str!("suite.rs");
    for f in std::fs::read_dir(dir).unwrap() {
        let name = f.unwrap().file_name().into_string().unwrap();
        let Some(stem) = name.strip_suffix(".rs") else {
            continue;
        };
        if stem == "suite" || SEPARES.contains(&stem) {
            continue;
        }
        assert!(
            suite.contains(&format!("#[path = \"{name}\"]")),
            "{name} absent de tests/suite.rs : ses tests ne seraient jamais lancés"
        );
    }
}
