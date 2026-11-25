//! US-11.1 nomenclature par variante (tranche 119) : export CSV et PDF d'une variante, valeurs
//! **calculées à la main**.
//!
//! Jeu : « Dos » 400 × 600 (aire 240 000 mm²), cuir, quantité 1 ; « Poche » carré 100 × 100
//! (10 000), cuir, quantité 1 ; « Manche » rectangle 100 × 200 (20 000), toile, paire en miroir,
//! quantité 1. Variante « Chemise » : Dos × 1, Poche × 2 (option « avec poche », active), Manche
//! × 1 en « Lin » (surcharge, paire = 2 découpes).
//! Cuir = 240 000 + 2 × 10 000 = 260 000 ; Lin = 2 × 20 000 = 40 000.

mod common;
mod nomenc_support;

use common::csv_reader::parse;
use common::report_pdf::pages;
use nomenc_support::*;
use pf_core::*;
use pf_export::bom_export::{export_bom, export_bom_variant};
use pf_export::report::ReportFormat;
use pf_i18n::Lang;

fn jeu() -> (Project, VariantId) {
    let mut p = Project::new("Sac");
    let dos = poly_piece(
        &mut p,
        &[(0.0, 0.0), (400.0, 0.0), (400.0, 600.0), (0.0, 600.0)],
        "Dos",
    );
    let poche = poly_piece(
        &mut p,
        &[
            (1000.0, 0.0),
            (1100.0, 0.0),
            (1100.0, 100.0),
            (1000.0, 100.0),
        ],
        "Poche",
    );
    let manche = poly_piece(
        &mut p,
        &[
            (2000.0, 0.0),
            (2100.0, 0.0),
            (2100.0, 200.0),
            (2000.0, 200.0),
        ],
        "Manche",
    );
    card(&mut p, dos, 1, false, Some("Cuir"), None);
    card(&mut p, poche, 1, false, Some("Cuir"), None);
    card(&mut p, manche, 1, true, Some("Toile"), None);
    let v = p
        .apply(Command::AddVariant {
            name: "Chemise".into(),
        })
        .unwrap()
        .variants[0];
    // L'option doit exister avant les entrées qui la citent.
    p.apply(Command::AddVariantOption {
        variant: v,
        option: "avec poche".into(),
    })
    .unwrap();
    for (piece, q, m, o) in [
        (dos, 1, None, None),
        (poche, 2, None, Some("avec poche")),
        (manche, 1, Some("Lin"), None),
    ] {
        p.apply(Command::AddVariantEntry {
            variant: v,
            piece,
            quantity: q,
            material: m.map(str::to_string),
            option: o.map(str::to_string),
        })
        .unwrap();
    }
    p.apply(Command::SetVariantOptionActive {
        variant: v,
        option: "avec poche".into(),
        active: true,
    })
    .unwrap();
    (p, v)
}

fn row<'a>(rows: &'a [Vec<String>], code: &str, first: &str) -> &'a Vec<String> {
    rows.iter()
        .find(|r| r[0] == code && r.get(1).is_some_and(|c| c == first))
        .unwrap_or_else(|| panic!("ligne {code}/{first} absente : {rows:?}"))
}

#[test]
fn csv_d_une_variante_lignes_exactes() {
    let (p, v) = jeu();
    let out = export_bom_variant(&p, None, Some(v), ReportFormat::Csv, Lang::Fr).unwrap();
    let rows = parse(&out.bytes);
    // Synthèse : variante et options actives.
    assert_eq!(row(&rows, "resume", "Par variante")[2], "Chemise");
    assert_eq!(row(&rows, "resume", "Options actives")[2], "avec poche");
    assert_eq!(row(&rows, "resume", "Pièces")[2], "3");
    // Lignes : une par entrée ; l'option est entre crochets après le nom ; découpes = quantité de
    // l'entrée (× 2 pour la paire).
    let piece: Vec<&Vec<String>> = rows.iter().filter(|r| r[0] == "piece").collect();
    assert_eq!(piece.len(), 3);
    assert_eq!(piece[0][1], "Dos");
    assert_eq!(
        (
            piece[0][4].as_str(),
            piece[0][6].as_str(),
            piece[0][8].as_str()
        ),
        ("Cuir", "1", "1")
    );
    assert_eq!(piece[1][1], "Poche [avec poche]");
    assert_eq!((piece[1][6].as_str(), piece[1][8].as_str()), ("2", "2"));
    assert_eq!(piece[2][1], "Manche");
    assert_eq!(
        (
            piece[2][4].as_str(),
            piece[2][6].as_str(),
            piece[2][7].as_str(),
            piece[2][8].as_str()
        ),
        ("Lin", "1", "oui", "2")
    );
    // Matières : Cuir 2 lignes, 3 découpes, 260 000 ; Lin 1 ligne, 2 découpes, 40 000.
    let mat: Vec<&Vec<String>> = rows.iter().filter(|r| r[0] == "matiere").collect();
    assert_eq!(mat.len(), 2);
    assert_eq!(mat[0][1..], ["Cuir", "2", "3", "260000.00"]);
    assert_eq!(mat[1][1..], ["Lin", "1", "2", "40000.00"]);
}

#[test]
fn option_desactivee_exclut_l_entree() {
    let (mut p, v) = jeu();
    p.apply(Command::SetVariantOptionActive {
        variant: v,
        option: "avec poche".into(),
        active: false,
    })
    .unwrap();
    let rows = parse(
        &export_bom_variant(&p, None, Some(v), ReportFormat::Csv, Lang::Fr)
            .unwrap()
            .bytes,
    );
    // Sans la poche (2 × 10 000) : Cuir = 240 000 en 1 ligne, 1 découpe.
    assert_eq!(rows.iter().filter(|r| r[0] == "piece").count(), 2);
    let mat: Vec<&Vec<String>> = rows.iter().filter(|r| r[0] == "matiere").collect();
    assert_eq!(mat[0][1..], ["Cuir", "1", "1", "240000.00"]);
    assert_eq!(row(&rows, "resume", "Options actives")[2], "aucune");
}

#[test]
fn sans_variante_choisie_la_sortie_est_celle_du_projet_entier() {
    let (p, _) = jeu();
    // Tout le projet : quantités des pièces (1 chacune), Manche en paire = 2 découpes ; Cuir =
    // 240 000 + 10 000 = 250 000 (2 lignes, 2 découpes) ; Toile = 2 × 20 000 = 40 000.
    for (format, lang) in [
        (ReportFormat::Csv, Lang::Fr),
        (ReportFormat::Csv, Lang::En),
        (ReportFormat::Pdf, Lang::Fr),
    ] {
        let a = export_bom(&p, None, format, lang).unwrap();
        let b = export_bom_variant(&p, None, None, format, lang).unwrap();
        assert_eq!(a.bytes, b.bytes);
    }
    let rows = parse(
        &export_bom(&p, None, ReportFormat::Csv, Lang::Fr)
            .unwrap()
            .bytes,
    );
    // Le projet a des variantes mais aucune n'est choisie : le dire, sans nommer de variante.
    assert_eq!(
        row(&rows, "resume", "Par variante")[2],
        "tout le projet (aucune variante choisie)"
    );
    assert!(rows
        .iter()
        .all(|r| r[0] != "resume" || r[1] != "Options actives"));
    let mat: Vec<&Vec<String>> = rows.iter().filter(|r| r[0] == "matiere").collect();
    assert_eq!(mat[0][1..], ["Cuir", "2", "2", "250000.00"]);
    assert_eq!(mat[1][1..], ["Toile", "1", "2", "40000.00"]);
    assert!(rows
        .iter()
        .filter(|r| r[0] == "piece")
        .all(|r| !r[1].contains('[')));
}

#[test]
fn projet_sans_variante_garde_le_texte_historique() {
    let (mut p, v) = jeu();
    p.apply(Command::RemoveVariant { variant: v }).unwrap();
    let rows = parse(
        &export_bom(&p, None, ReportFormat::Csv, Lang::Fr)
            .unwrap()
            .bytes,
    );
    assert_eq!(
        row(&rows, "resume", "Par variante")[2],
        "non disponible (les variantes ne sont pas faites)"
    );
}

#[test]
fn pdf_d_une_variante_valeurs_et_refus_variante_inconnue() {
    let (p, v) = jeu();
    for lang in [Lang::Fr, Lang::En] {
        let out = export_bom_variant(&p, None, Some(v), ReportFormat::Pdf, lang).unwrap();
        assert!(out.bytes.starts_with(b"%PDF"));
        let text: String = pages(&out.bytes)
            .iter()
            .map(|pg| pg.joined())
            .collect::<Vec<_>>()
            .join("\n");
        for needle in ["260000", "40000", "Chemise", "Lin", "Poche [avec poche]"] {
            assert!(
                text.lines().any(|l| l == needle),
                "{lang:?} : « {needle} » absent"
            );
        }
        assert!(!text.contains('?'), "{lang:?}: caractère non écrit");
    }
    assert!(export_bom_variant(
        &p,
        None,
        Some(VariantId(999_999)),
        ReportFormat::Csv,
        Lang::Fr
    )
    .is_err());
    // Lecture seule : le projet est inchangé.
    let before = format!("{p:?}");
    export_bom_variant(&p, None, Some(v), ReportFormat::Csv, Lang::Fr).unwrap();
    assert_eq!(before, format!("{p:?}"));
}
