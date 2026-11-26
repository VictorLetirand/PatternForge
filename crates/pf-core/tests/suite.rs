//! Tous les tests d'intégration du paquet forment **un seul programme** : une seule édition de
//! liens, et les campagnes s'exécutent en parallèle sur tous les cœurs.
//!
//! Un nouveau fichier de test doit être ajouté à la liste ci-dessous (un test le vérifie).
//! Pour n'en lancer qu'un : `cargo test -p pf-core --test suite nom_du_fichier::`.

// Les modules partagés sont inclus par plusieurs fichiers, comme lorsqu'ils étaient séparés.
#![allow(clippy::duplicate_mod)]

#[path = "acceptance.rs"]
mod acceptance;
#[path = "adverse_ampleur.rs"]
mod adverse_ampleur;
#[path = "adverse_bareme.rs"]
mod adverse_bareme;
#[path = "adverse_bibliotheque.rs"]
mod adverse_bibliotheque;
#[path = "adverse_boutons.rs"]
mod adverse_boutons;
#[path = "adverse_coins.rs"]
mod adverse_coins;
#[path = "adverse_construction.rs"]
mod adverse_construction;
#[path = "adverse_constructions_maintenues.rs"]
mod adverse_constructions_maintenues;
#[path = "adverse_controle_crans.rs"]
mod adverse_controle_crans;
#[path = "adverse_controle_forme.rs"]
mod adverse_controle_forme;
#[path = "adverse_controle_piqure.rs"]
mod adverse_controle_piqure;
#[path = "adverse_coordonnees_saisie.rs"]
mod adverse_coordonnees_saisie;
#[path = "adverse_cotes.rs"]
mod adverse_cotes;
#[path = "adverse_coupe_nette.rs"]
mod adverse_coupe_nette;
#[path = "adverse_coupe_pince.rs"]
mod adverse_coupe_pince;
#[path = "adverse_coutures.rs"]
mod adverse_coutures;
#[path = "adverse_crans.rs"]
mod adverse_crans;
#[path = "adverse_crans_ancres.rs"]
mod adverse_crans_ancres;
#[path = "adverse_crans_coin.rs"]
mod adverse_crans_coin;
#[path = "adverse_cuir_peau.rs"]
mod adverse_cuir_peau;
#[path = "adverse_cutting_auto.rs"]
mod adverse_cutting_auto;
#[path = "adverse_cutting_auto_completer.rs"]
mod adverse_cutting_auto_completer;
#[path = "adverse_cutting_layout.rs"]
mod adverse_cutting_layout;
#[path = "adverse_cutting_yield.rs"]
mod adverse_cutting_yield;
#[path = "adverse_decoupes.rs"]
mod adverse_decoupes;
#[path = "adverse_dependances.rs"]
mod adverse_dependances;
#[path = "adverse_depliage.rs"]
mod adverse_depliage;
#[path = "adverse_diagnostic.rs"]
mod adverse_diagnostic;
#[path = "adverse_droit_fil.rs"]
mod adverse_droit_fil;
#[path = "adverse_e03_restes.rs"]
mod adverse_e03_restes;
#[path = "adverse_egalite_4b.rs"]
mod adverse_egalite_4b;
#[path = "adverse_egalite_longueurs.rs"]
mod adverse_egalite_longueurs;
#[path = "adverse_elements_internes.rs"]
mod adverse_elements_internes;
#[path = "adverse_extraction_pieces.rs"]
mod adverse_extraction_pieces;
#[path = "adverse_fiche_piece.rs"]
mod adverse_fiche_piece;
#[path = "adverse_fichiers_corrompus.rs"]
mod adverse_fichiers_corrompus;
#[path = "adverse_fond_image.rs"]
mod adverse_fond_image;
#[path = "adverse_formules.rs"]
mod adverse_formules;
#[path = "adverse_formules_restes.rs"]
mod adverse_formules_restes;
#[path = "adverse_gradation.rs"]
mod adverse_gradation;
#[path = "adverse_gradation_derivee.rs"]
mod adverse_gradation_derivee;
#[path = "adverse_gradation_report.rs"]
mod adverse_gradation_report;
#[path = "adverse_journal_erreurs.rs"]
mod adverse_journal_erreurs;
#[path = "adverse_lignes_speciales.rs"]
mod adverse_lignes_speciales;
#[path = "adverse_longueur_imposee.rs"]
mod adverse_longueur_imposee;
#[path = "adverse_longueur_lisse.rs"]
mod adverse_longueur_lisse;
#[path = "adverse_marche.rs"]
mod adverse_marche;
#[path = "adverse_marquages.rs"]
mod adverse_marquages;
#[path = "adverse_matieres.rs"]
mod adverse_matieres;
#[path = "adverse_mesures_tailles.rs"]
mod adverse_mesures_tailles;
#[path = "adverse_nid_tailles.rs"]
mod adverse_nid_tailles;
#[path = "adverse_nombres_extremes.rs"]
mod adverse_nombres_extremes;
#[path = "adverse_nommage.rs"]
mod adverse_nommage;
#[path = "adverse_ourlets.rs"]
mod adverse_ourlets;
#[path = "adverse_percages.rs"]
mod adverse_percages;
#[path = "adverse_pieces_derivees.rs"]
mod adverse_pieces_derivees;
#[path = "adverse_pinces.rs"]
mod adverse_pinces;
#[path = "adverse_piqure.rs"]
mod adverse_piqure;
#[path = "adverse_placements.rs"]
mod adverse_placements;
#[path = "adverse_plans_coupe.rs"]
mod adverse_plans_coupe;
#[path = "adverse_plis.rs"]
mod adverse_plis;
#[path = "adverse_pliure.rs"]
mod adverse_pliure;
#[path = "adverse_raccords.rs"]
mod adverse_raccords;
#[path = "adverse_retouches_mesures_finies.rs"]
mod adverse_retouches_mesures_finies;
#[path = "adverse_sans_droit_fil.rs"]
mod adverse_sans_droit_fil;
#[path = "adverse_seam.rs"]
mod adverse_seam;
#[path = "adverse_separer_assembler.rs"]
mod adverse_separer_assembler;
#[path = "adverse_sequences.rs"]
mod adverse_sequences;
#[path = "adverse_taille_client.rs"]
mod adverse_taille_client;
#[path = "adverse_taille_valeurs.rs"]
mod adverse_taille_valeurs;
#[path = "adverse_tailles.rs"]
mod adverse_tailles;
#[path = "adverse_transfert_pince.rs"]
mod adverse_transfert_pince;
#[path = "adverse_v5.rs"]
mod adverse_v5;
#[path = "adverse_v6.rs"]
mod adverse_v6;
#[path = "adverse_variables.rs"]
mod adverse_variables;
#[path = "adverse_variantes.rs"]
mod adverse_variantes;
#[path = "adverse_verrou.rs"]
mod adverse_verrou;
#[path = "adverse_versions.rs"]
mod adverse_versions;
#[path = "adverse_zones.rs"]
mod adverse_zones;
#[path = "align.rs"]
mod align;
#[path = "ampleur.rs"]
mod ampleur;
#[path = "bareme.rs"]
mod bareme;
#[path = "bibliotheque.rs"]
mod bibliotheque;
#[path = "circles_arcs.rs"]
mod circles_arcs;
#[path = "cles_json.rs"]
mod cles_json;
#[path = "clipboard.rs"]
mod clipboard;
#[path = "construction_rapide.rs"]
mod construction_rapide;
#[path = "constructions_maintenues.rs"]
mod constructions_maintenues;
#[path = "contours.rs"]
mod contours;
#[path = "controle_crans.rs"]
mod controle_crans;
#[path = "controle_forme.rs"]
mod controle_forme;
#[path = "controle_piqure.rs"]
mod controle_piqure;
#[path = "coupe_pince_fermee.rs"]
mod coupe_pince_fermee;
#[path = "coutures.rs"]
mod coutures;
#[path = "cuir_peau.rs"]
mod cuir_peau;
#[path = "cutting_auto.rs"]
mod cutting_auto;
#[path = "cutting_auto_completer.rs"]
mod cutting_auto_completer;
#[path = "cutting_layout.rs"]
mod cutting_layout;
#[path = "cutting_yield.rs"]
mod cutting_yield;
#[path = "d04_variantes.rs"]
mod d04_variantes;
#[path = "decimal_separator.rs"]
mod decimal_separator;
#[path = "decoupes.rs"]
mod decoupes;
#[path = "dependances.rs"]
mod dependances;
#[path = "derived_points.rs"]
mod derived_points;
#[path = "diagnostic.rs"]
mod diagnostic;
#[path = "dimensions.rs"]
mod dimensions;
#[path = "dimensions_full.rs"]
mod dimensions_full;
#[path = "divisions.rs"]
mod divisions;
#[path = "durcissement_a2.rs"]
mod durcissement_a2;
#[path = "durcissement_b1_b2.rs"]
mod durcissement_b1_b2;
#[path = "durcissement_resolution.rs"]
mod durcissement_resolution;
#[path = "edit_commands.rs"]
mod edit_commands;
#[path = "egalite_longueurs.rs"]
mod egalite_longueurs;
#[path = "egalite_longueurs_4b.rs"]
mod egalite_longueurs_4b;
#[path = "expressions.rs"]
mod expressions;
#[path = "fond_image.rs"]
mod fond_image;
#[path = "format_ouvert.rs"]
mod format_ouvert;
#[path = "formules.rs"]
mod formules;
#[path = "formules_restes.rs"]
mod formules_restes;
#[path = "gradation.rs"]
mod gradation;
#[path = "gradation_derivee.rs"]
mod gradation_derivee;
#[path = "gradation_report.rs"]
mod gradation_report;
#[path = "groups.rs"]
mod groups;
#[path = "guides.rs"]
mod guides;
#[path = "import_geometry.rs"]
mod import_geometry;
#[path = "jeu_essai.rs"]
mod jeu_essai;
#[path = "journal_erreurs.rs"]
mod journal_erreurs;
#[path = "layers.rs"]
mod layers;
#[path = "lignes_piqure.rs"]
mod lignes_piqure;
#[path = "longueur_courbe_lisse.rs"]
mod longueur_courbe_lisse;
#[path = "longueur_imposee.rs"]
mod longueur_imposee;
#[path = "marche.rs"]
mod marche;
#[path = "mesures_tailles.rs"]
mod mesures_tailles;
#[path = "nid_tailles.rs"]
mod nid_tailles;
#[path = "nommage.rs"]
mod nommage;
#[path = "notch_anchor.rs"]
mod notch_anchor;
#[path = "notch_corner.rs"]
mod notch_corner;
#[path = "notch_shapes.rs"]
mod notch_shapes;
#[path = "notches.rs"]
mod notches;
#[path = "offset.rs"]
mod offset;
#[path = "path_edit.rs"]
mod path_edit;
#[path = "percages_au_pas.rs"]
mod percages_au_pas;
#[path = "piece_buttons.rs"]
mod piece_buttons;
#[path = "piece_card.rs"]
mod piece_card;
#[path = "piece_coins.rs"]
mod piece_coins;
#[path = "piece_coupe_nette.rs"]
mod piece_coupe_nette;
#[path = "piece_darts.rs"]
mod piece_darts;
#[path = "piece_default.rs"]
mod piece_default;
#[path = "piece_derived.rs"]
mod piece_derived;
#[path = "piece_extract.rs"]
mod piece_extract;
#[path = "piece_grain.rs"]
mod piece_grain;
#[path = "piece_hems.rs"]
mod piece_hems;
#[path = "piece_internals.rs"]
mod piece_internals;
#[path = "piece_marks.rs"]
mod piece_marks;
#[path = "piece_matieres.rs"]
mod piece_matieres;
#[path = "piece_plan.rs"]
mod piece_plan;
#[path = "piece_seam.rs"]
mod piece_seam;
#[path = "piece_split_join.rs"]
mod piece_split_join;
#[path = "piece_unfold.rs"]
mod piece_unfold;
#[path = "plans_coupe.rs"]
mod plans_coupe;
#[path = "plis.rs"]
mod plis;
#[path = "pliure.rs"]
mod pliure;
#[path = "raccords.rs"]
mod raccords;
#[path = "relative_points.rs"]
mod relative_points;
#[path = "restes_e03.rs"]
mod restes_e03;
#[path = "retouches_mesures_finies.rs"]
mod retouches_mesures_finies;
#[path = "sans_droit_fil.rs"]
mod sans_droit_fil;
#[path = "schema_v3.rs"]
mod schema_v3;
#[path = "schema_v5.rs"]
mod schema_v5;
#[path = "schema_v6.rs"]
mod schema_v6;
#[path = "sheet_copy.rs"]
mod sheet_copy;
#[path = "special_lines.rs"]
mod special_lines;
#[path = "splines.rs"]
mod splines;
#[path = "symmetry.rs"]
mod symmetry;
#[path = "taille_client.rs"]
mod taille_client;
#[path = "taille_valeurs.rs"]
mod taille_valeurs;
#[path = "tailles.rs"]
mod tailles;
#[path = "tangency.rs"]
mod tangency;
#[path = "trace_intersections.rs"]
mod trace_intersections;
#[path = "transfert_pince.rs"]
mod transfert_pince;
#[path = "validate_index.rs"]
mod validate_index;
#[path = "variables.rs"]
mod variables;
#[path = "variantes.rs"]
mod variantes;
#[path = "verrou_fichier.rs"]
mod verrou_fichier;
#[path = "version_diff.rs"]
mod version_diff;
#[path = "versions.rs"]
mod versions;
#[path = "zones_piece.rs"]
mod zones_piece;

/// Fichiers déclarés comme programmes séparés dans `Cargo.toml`.
const SEPARES: &[&str] = &["bancs_volumineux", "durcissement_bancs"];

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
