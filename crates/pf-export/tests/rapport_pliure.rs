//! Dette de test PH-5b : le rapport de contrôle (CSV et PDF) d'un projet qui a une pliure.
//!
//! Lecteurs indépendants de `tests/common/` (`lecteurs_libelles/`), valeurs écrites à la main.
//!
//! Projet : une feuille (« Feuille 1 »), deux pièces rectangulaires.
//! - « Demi-devant » 100 × 100 en (0 ; 0), marge de couture 10 mm, bord gauche sur le pli : un bord
//!   sur le pli, longueur 100 mm ; contour de couture : aire 100 × 100 = 10 000 mm², périmètre
//!   4 × 100 = 400 mm ;
//! - « Manche » 60 × 40 en (200 ; 0), sans pliure : aire 2 400 mm², périmètre 200 mm.
//!
//! Attendu : section `pli` à **une seule ligne** (la manche n'y figure pas), « 1 » bord et
//! « 100.00 » mm (deux décimales, point décimal) ; la section est absente d'un projet sans pliure.
//!
//! PDF : colonnes de la section de 40 / 70 / 30 / 40 mm, marge de page 15 mm et marge de cellule
//! 1 mm : colonnes à x = 16, 16 + 40 = 56, 56 + 70 = 126, 126 + 30 = 156 ; titre à x = 15.

mod lecteurs_libelles;

use lecteurs_libelles::*;
use pf_core::{Command, PathRole, Project, SeamAllowance};
use pf_export::report::{export_control_report, ReportFormat};
use pf_i18n::Lang;

fn project(with_fold: bool) -> Project {
    let mut p = Project::new("Pliure");
    let s = p.sheets[0].id;
    let (devant, e) = rect(
        &mut p,
        s,
        (0.0, 0.0),
        (100.0, 100.0),
        "Demi-devant",
        PathRole::SewingContour,
    );
    rect(
        &mut p,
        s,
        (200.0, 0.0),
        (60.0, 40.0),
        "Manche",
        PathRole::SewingContour,
    );
    p.apply(Command::SetSeamAllowance {
        piece: devant,
        seam: Some(SeamAllowance {
            default_mm: 10.0,
            ..Default::default()
        }),
    })
    .unwrap();
    if with_fold {
        p.apply(Command::SetPieceFold {
            piece: devant,
            edge: e.left,
            on: true,
        })
        .unwrap();
    }
    assert_eq!(p.sheets[0].name, "Feuille 1");
    p
}

fn row(cells: &[&str]) -> Vec<String> {
    cells.iter().map(|c| (*c).to_string()).collect()
}

/// Codes de section (`#code`) dans l'ordre du fichier.
fn sections(rows: &[Vec<String>]) -> Vec<&str> {
    rows.iter().filter_map(|r| r[0].strip_prefix('#')).collect()
}

#[test]
fn csv_section_pli_une_ligne_pour_la_seule_piece_pliee() {
    let rows = csv_rows(&export_control_report(&project(true), ReportFormat::Csv, Lang::Fr).bytes);
    assert_eq!(
        sections(&rows),
        ["resume", "anomalie", "piece", "pli", "couture", "crans", "raccord"]
    );
    // Section `pli` : en-tête puis une ligne, entre `piece` et `couture`.
    let at = rows.iter().position(|r| r[0] == "#pli").unwrap();
    assert_eq!(
        rows[at],
        row(&[
            "#pli",
            "Feuille",
            "Pièce",
            "Bords sur le pli",
            "Longueur sur le pli (mm)"
        ])
    );
    assert_eq!(
        rows[at + 1],
        row(&["pli", "Feuille 1", "Demi-devant", "1", "100.00"])
    );
    assert!(
        rows[at + 2][0].starts_with("#couture"),
        "{:?}",
        rows[at + 2]
    );
    assert_eq!(rows.iter().filter(|r| r[0] == "pli").count(), 1);
    // La section `piece` garde ses deux lignes : aire et périmètre de la couture.
    let pieces: Vec<&Vec<String>> = rows.iter().filter(|r| r[0] == "piece").collect();
    assert_eq!(pieces.len(), 2);
    assert_eq!(
        &pieces[0][1..10],
        row(&[
            "Feuille 1",
            "Demi-devant",
            "",
            "",
            "1",
            "",
            "10000.00",
            "400.00",
            "oui"
        ])
    );
    assert_eq!(
        &pieces[1][1..10],
        row(&[
            "Feuille 1",
            "Manche",
            "",
            "",
            "1",
            "",
            "2400.00",
            "200.00",
            "oui"
        ])
    );
}

#[test]
fn csv_en_anglais() {
    let rows = csv_rows(&export_control_report(&project(true), ReportFormat::Csv, Lang::En).bytes);
    let at = rows.iter().position(|r| r[0] == "#pli").unwrap();
    assert_eq!(
        rows[at],
        row(&[
            "#pli",
            "Sheet",
            "Piece",
            "Fold edges",
            "Length on fold (mm)"
        ])
    );
    assert_eq!(
        rows[at + 1],
        row(&["pli", "Feuille 1", "Demi-devant", "1", "100.00"])
    );
}

#[test]
fn csv_sans_pliure_pas_de_section_pli() {
    let rows = csv_rows(&export_control_report(&project(false), ReportFormat::Csv, Lang::Fr).bytes);
    assert_eq!(
        sections(&rows),
        ["resume", "anomalie", "piece", "couture", "crans", "raccord"]
    );
    assert!(rows.iter().all(|r| r[0] != "pli"));
}

/// Textes de la section dont le titre commence par `title` : ceux qui suivent le titre dans le flux
/// jusqu'au titre suivant (corps 10 pt).
fn section_texts(texts: &[PdfText], title: &str) -> Vec<PdfText> {
    let at = texts
        .iter()
        .position(|t| t.text.starts_with(title))
        .unwrap_or_else(|| panic!("titre « {title} » absent"));
    texts[at..]
        .iter()
        .enumerate()
        .take_while(|(i, t)| *i == 0 || (t.size_pt - 10.0).abs() > 1e-6)
        .map(|(_, t)| t.clone())
        .collect()
}

#[test]
fn pdf_section_pieces_sur_le_pli() {
    for (lang, title, head) in [
        (
            Lang::Fr,
            "Pièces sur le pli (1)",
            [
                "Feuille",
                "Pièce",
                "Bords sur le pli",
                "Longueur sur le pli (mm)",
            ],
        ),
        (
            Lang::En,
            "Pieces on fold (1)",
            ["Sheet", "Piece", "Fold edges", "Length on fold (mm)"],
        ),
    ] {
        let pdf = export_control_report(&project(true), ReportFormat::Pdf, lang);
        assert_eq!(pdf.page_count, Some(1));
        let texts = pdf_texts(&pdf.bytes);
        let sec = section_texts(&texts, title);
        // Titre à x = 15, corps 10 pt ; puis quatre en-têtes et quatre cellules, rien d'autre
        // (la manche n'y figure pas).
        assert_eq!(sec.len(), 1 + 4 + 4, "{sec:?}");
        assert!(near(sec[0].x_mm, 15.0, 0.01) && near(sec[0].size_pt, 10.0, 1e-6));
        let heads = &sec[1..5];
        let cells = &sec[5..9];
        let xs = [16.0, 56.0, 126.0, 156.0];
        for (i, (h, c)) in heads.iter().zip(cells).enumerate() {
            assert_eq!(h.text, head[i], "{lang:?}");
            assert!(near(h.x_mm, xs[i], 0.01), "{h:?}");
            assert!(near(c.x_mm, xs[i], 0.01), "{c:?}");
            assert!(near(c.y_mm, cells[0].y_mm, 0.01) && near(h.y_mm, heads[0].y_mm, 0.01));
        }
        let values: Vec<&str> = cells.iter().map(|c| c.text.as_str()).collect();
        assert_eq!(values, ["Feuille 1", "Demi-devant", "1", "100.00"]);
        // La ligne est sous l'en-tête, qui est sous le titre.
        assert!(sec[0].y_mm > heads[0].y_mm && heads[0].y_mm > cells[0].y_mm);
        // La section est entre « Pièces » et « Coutures » (ordre vertical du rapport).
        let y_of = |prefix: &str| {
            texts
                .iter()
                .find(|t| t.text.starts_with(prefix))
                .unwrap_or_else(|| panic!("« {prefix} » absent"))
                .y_mm
        };
        let (pieces, seams) = match lang {
            Lang::Fr => ("Pièces (2)", "Coutures appariées"),
            Lang::En => ("Pieces (2)", "Paired seams"),
        };
        assert!(y_of(pieces) > sec[0].y_mm && sec[0].y_mm > y_of(seams));
    }
}

#[test]
fn pdf_sans_pliure_pas_de_section_pli() {
    let pdf = export_control_report(&project(false), ReportFormat::Pdf, Lang::Fr);
    let texts = pdf_texts(&pdf.bytes);
    assert!(texts.iter().all(|t| !t.text.contains("sur le pli")));
    assert!(texts.iter().any(|t| t.text.starts_with("Pièces (2)")));
}
