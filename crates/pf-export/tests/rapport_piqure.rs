//! US-08.6 — la section « trous de piqure » du rapport de contrôle (CSV et PDF).
//!
//! Projet : pièce A (rectangle 100 × 70 en (0 ; 0)) avec une ligne de 5 trous (x = 0, 25, 50, 75,
//! 100 ; y = 3) et pièce B (rectangle 100 × 70 en (200 ; 0)) avec une ligne de 4 trous, déclarées
//! appariées. Attendu (calculé à la main, cousues en sens opposés) : pas moyen 25,00 et 33,33,
//! écart −8,33 ; trou 2 (25 ; 3) et trou 4 (75 ; 3) décalés de 8,33 ; trou 3 (50 ; 3) manquant sur B.

mod common;

use common::csv_reader::{parse, sections};
use common::report_pdf::pages;
use pf_core::*;
use pf_export::report::{export_control_report, ReportFormat};
use pf_geometry::PointMm;
use pf_i18n::Lang;

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

fn project() -> Project {
    let mut p = Project::new("Piqure rapport");
    let s = p.sheets[0].id;
    let mut piece = |x0: f64, name: &str| -> (PieceId, EntityId) {
        p.apply(Command::AddPolygon {
            sheet: s,
            vertices: [(x0, 0.0), (x0 + 100.0, 0.0), (x0 + 100.0, 70.0), (x0, 70.0)]
                .map(|(x, y)| PointRef::New(pt(x, y)))
                .to_vec(),
            closed: true,
            role: PathRole::default(),
            layer: None,
        })
        .unwrap();
        let id = p
            .apply(Command::ExtractPiece {
                sheet: s,
                source: PieceSource::Click {
                    at: pt(x0 + 50.0, 35.0),
                },
                name: name.into(),
                detached: false,
            })
            .unwrap()
            .pieces[0];
        let sheet = p.sheet(s).unwrap();
        let bottom = sheet
            .entities
            .iter()
            .find(|e| {
                sheet.segment_ends(e.id).is_ok_and(|(a, b)| {
                    a.y == 0.0 && b.y == 0.0 && (a.x - x0).abs().min((b.x - x0).abs()) < 1e-9
                })
            })
            .unwrap()
            .id;
        (id, bottom)
    };
    let (a, ea) = piece(0.0, "A");
    let (b, eb) = piece(200.0, "B");
    let spec = |e, n| StitchLineSpec {
        edges: vec![e],
        hole: StitchHole::Round { diameter_mm: 1.2 },
        pitch_mm: 4.0,
        setback_mm: 3.0,
        count: Some(n),
        continues: None,
    };
    let la = p
        .apply(Command::AddStitchLine {
            piece: a,
            line: spec(ea, 5),
        })
        .unwrap()
        .stitch_lines[0];
    let lb = p
        .apply(Command::AddStitchLine {
            piece: b,
            line: spec(eb, 4),
        })
        .unwrap()
        .stitch_lines[0];
    p.apply(Command::PairStitchLines {
        a: StitchRef { piece: a, line: la },
        b: StitchRef { piece: b, line: lb },
    })
    .unwrap();
    p
}

fn row(cells: &[&str]) -> Vec<String> {
    cells.iter().map(|c| c.to_string()).collect()
}

fn csv(p: &Project, lang: Lang) -> std::collections::BTreeMap<String, common::csv_reader::Section> {
    sections(&parse(
        &export_control_report(p, ReportFormat::Csv, lang).bytes,
    ))
}

#[test]
fn ca1_lignes_csv_calculees_a_la_main_en_francais_et_en_anglais() {
    let p = project();
    let s = csv(&p, Lang::Fr);
    let sec = &s["trous_piqure"];
    assert_eq!(sec.header.len(), 13);
    assert_eq!(
        sec.rows,
        vec![
            row(&[
                "1",
                "A",
                "B",
                "5",
                "4",
                "25.00",
                "33.33",
                "-8.33",
                "",
                "Nombre de trous different",
                "",
                "",
                ""
            ]),
            row(&[
                "1", "A", "B", "5", "4", "25.00", "33.33", "-8.33", "2", "Decale", "25.00", "3.00",
                "8.33"
            ]),
            row(&[
                "1",
                "A",
                "B",
                "5",
                "4",
                "25.00",
                "33.33",
                "-8.33",
                "3",
                "Manque sur B",
                "50.00",
                "3.00",
                ""
            ]),
            row(&[
                "1", "A", "B", "5", "4", "25.00", "33.33", "-8.33", "4", "Decale", "75.00", "3.00",
                "8.33"
            ]),
        ]
    );
    let en = csv(&p, Lang::En);
    assert_eq!(en["trous_piqure"].rows[2][9], "Missing on B");
    assert_eq!(en["trous_piqure"].header[0], "Pair");
}

#[test]
fn ca2_un_trou_en_trop_est_liste_avec_sa_position() {
    let mut p = project();
    // Couple inverse : la ligne a 4 trous devient A, celle a 5 trous B ; le trou central (50 ; 3)
    // de la piece A est en trop.
    let (a, b) = (p.stitch_pairs[0].a, p.stitch_pairs[0].b);
    let id = p.stitch_pairs[0].id;
    p.apply(Command::UnpairStitchLines { pair: id }).unwrap();
    p.apply(Command::PairStitchLines { a: b, b: a }).unwrap();
    let s = csv(&p, Lang::Fr);
    let rows = &s["trous_piqure"].rows;
    let extra: Vec<&Vec<String>> = rows.iter().filter(|r| r[9] == "En trop sur B").collect();
    assert_eq!(extra.len(), 1);
    assert_eq!(&extra[0][8..9], &["5".to_string()][..]);
    assert_eq!(
        &extra[0][10..12],
        &["50.00".to_string(), "3.00".to_string()][..]
    );
}

#[test]
fn ca3_section_absente_sans_couple_et_le_reste_du_rapport_est_identique() {
    let p = project();
    let mut bare = p.clone();
    bare.stitch_pairs.clear();
    let without = csv(&bare, Lang::Fr);
    assert!(!without.contains_key("trous_piqure"));
    let with = csv(&p, Lang::Fr);
    assert_eq!(with["resume"].rows, without["resume"].rows);
    assert_eq!(with["anomalie"].rows, without["anomalie"].rows);
    let a = export_control_report(&bare, ReportFormat::Csv, Lang::Fr).bytes;
    let b = export_control_report(&p, ReportFormat::Csv, Lang::Fr).bytes;
    assert!(
        b.len() > a.len() && b.starts_with(&a),
        "la section s'ajoute a la fin"
    );
}

#[test]
fn ca4_pdf_ascii_valeurs_et_titre() {
    let p = project();
    let bytes = export_control_report(&p, ReportFormat::Pdf, Lang::Fr).bytes;
    let pgs = pages(&bytes);
    let all: Vec<_> = pgs.iter().flat_map(|pg| pg.texts.iter()).collect();
    for v in ["25.00", "33.33", "-8.33", "8.33", "50.00", "3.00"] {
        assert!(all.iter().any(|o| o.text == v), "valeur {v} absente du PDF");
    }
    assert!(
        all.iter()
            .any(|o| o.text.starts_with("Trous de piqure (1)")),
        "titre de section"
    );
    for o in &all {
        assert!(
            o.text.is_ascii() || !o.text.contains("piqure"),
            "{}",
            o.text
        );
    }
    // Sans couple, le PDF ne contient pas le titre.
    let mut bare = p.clone();
    bare.stitch_pairs.clear();
    let without = pages(&export_control_report(&bare, ReportFormat::Pdf, Lang::Fr).bytes);
    assert!(without
        .iter()
        .flat_map(|pg| pg.texts.iter())
        .all(|o| !o.text.contains("piqure")));
}
