//! US-10.4 — le tableau « mesures finies » du rapport de contrôle (CSV et PDF).
//!
//! Projet : une feuille, un segment de 100 mm (0 ; 0) → (100 ; 0) coté en longueur ; gamme S M L
//! (base M) ; mesure « Largeur » (mm) M = 102 ; mesure finie « Largeur finie » tolérances 1 / 1.
//! Attendu (calculé à la main) : mesuré 100, référence 102, écart −2, **hors tolérance** ; avec
//! la retouche +1 : référence 103, retouche 1, écart −3. Section absente sans mesure finie.

mod common;

use common::csv_reader::{parse, sections};
use common::report_pdf::pages;
use pf_core::*;
use pf_export::report::{export_control_report, ReportFormat};
use pf_geometry::PointMm;
use pf_i18n::Lang;

struct Fx {
    p: Project,
    largeur: MeasurementId,
    m: SizeId,
}

fn fx() -> Fx {
    let mut p = Project::new("Mesures finies");
    let sheet = p.sheets[0].id;
    p.apply(Command::AddSizeRange {
        name: "Femme".into(),
        kind: SizeKind::Alphanumeric,
        labels: vec!["S".into(), "M".into(), "L".into()],
        base: 1,
    })
    .unwrap();
    let m = p.sizes[0].sizes[1].id;
    let e = p
        .apply(Command::AddSegment {
            sheet,
            from: PointRef::New(PointMm::new(0.0, 0.0)),
            to: PointRef::New(PointMm::new(100.0, 0.0)),
        })
        .unwrap()
        .entities[0];
    let dim = p
        .apply(Command::AddFullDimension {
            sheet,
            kind: FullDimensionKind::PathLength {
                entity: e,
                offset_mm: 5.0,
            },
            label: None,
            text_shift_mm: 0.0,
        })
        .unwrap()
        .full_dimensions[0];
    let largeur = p
        .apply(Command::AddMeasurement {
            name: "Largeur".into(),
            unit: MeasurementUnit::Mm,
        })
        .unwrap()
        .measurements[0];
    p.apply(Command::SetMeasurementValue {
        measurement: largeur,
        size: m,
        value: 102.0,
    })
    .unwrap();
    p.apply(Command::AddFinishedMeasure {
        name: "Largeur finie".into(),
        measurement: largeur,
        source: FinishedSource::FullDimension {
            sheet,
            dimension: dim,
        },
        tolerance_minus: 1.0,
        tolerance_plus: 1.0,
    })
    .unwrap();
    Fx { p, largeur, m }
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
fn ca1_ligne_csv_a_la_main_et_en_anglais() {
    let f = fx();
    let s = csv(&f.p, Lang::Fr);
    assert_eq!(
        s["mesure_finie"].rows,
        vec![row(&[
            "Largeur finie",
            "Largeur (mm)",
            "M",
            "100.00",
            "102.00",
            "",
            "-2.00",
            "1.00",
            "1.00",
            "Hors tolérance",
            ""
        ])]
    );
    assert_eq!(s["mesure_finie"].header.len(), 11);
    let en = csv(&f.p, Lang::En);
    assert_eq!(en["mesure_finie"].rows[0][9], "Out of tolerance");
    assert_eq!(en["mesure_finie"].header[0], "Finished measurement");
}

#[test]
fn ca2_avec_retouche_la_reference_et_la_colonne_retouche() {
    let mut f = fx();
    f.p.apply(Command::AddAlteration {
        name: "Largeur +1".into(),
        target: f.largeur,
        delta: 1.0,
    })
    .unwrap();
    let s = csv(&f.p, Lang::Fr);
    let r = &s["mesure_finie"].rows[0];
    assert_eq!(
        &r[3..8],
        &row(&["100.00", "103.00", "1.00", "-3.00", "1.00"])[..]
    );
    assert_eq!(r[9], "Hors tolérance");
}

#[test]
fn ca3_conforme_et_non_comparable() {
    let mut f = fx();
    // Barème 101 : écart −1 = borne basse incluse → conforme.
    f.p.apply(Command::SetMeasurementValue {
        measurement: f.largeur,
        size: f.m,
        value: 101.0,
    })
    .unwrap();
    assert_eq!(
        csv(&f.p, Lang::Fr)["mesure_finie"].rows[0][9],
        "Dans la tolérance"
    );
    // Valeur du barème effacée : non comparable, jamais devinée.
    f.p.apply(Command::ClearMeasurementValue {
        measurement: f.largeur,
        size: f.m,
    })
    .unwrap();
    let s = csv(&f.p, Lang::Fr);
    let r = &s["mesure_finie"].rows[0];
    assert_eq!(&r[3..8], &row(&["100.00", "", "", "", "1.00"])[..]);
    assert_eq!(r[9], "Non comparable");
    assert_eq!(r[10], "Valeur du barème absente");
}

#[test]
fn ca4_section_absente_sans_mesure_finie_et_aucune_anomalie_ajoutee() {
    let f = fx();
    let mut bare = f.p.clone();
    bare.finished_measures.clear();
    let without = csv(&bare, Lang::Fr);
    assert!(!without.contains_key("mesure_finie"));
    // Le récapitulatif (anomalies) est identique avec et sans mesure finie.
    let with = csv(&f.p, Lang::Fr);
    assert_eq!(with["resume"].rows, without["resume"].rows);
    assert_eq!(with["anomalie"].rows, without["anomalie"].rows);
    // Octets du rapport sans mesure finie : la section n'ajoute rien.
    let a = export_control_report(&bare, ReportFormat::Csv, Lang::Fr).bytes;
    let b = export_control_report(&f.p, ReportFormat::Csv, Lang::Fr).bytes;
    assert!(b.len() > a.len());
    assert!(
        b.starts_with(&a),
        "la section s'ajoute à la fin, le reste est identique"
    );
}

#[test]
fn ca5_pdf_ascii_valeurs_et_aucun_caractere_absent() {
    let f = fx();
    let bytes = export_control_report(&f.p, ReportFormat::Pdf, Lang::Fr).bytes;
    let pgs = pages(&bytes);
    let all: Vec<_> = pgs.iter().flat_map(|p| p.texts.iter()).collect();
    for v in ["100.00", "102.00", "-2.00", "1.00"] {
        assert!(all.iter().any(|o| o.text == v), "valeur {v} absente du PDF");
    }
    let text = pgs
        .iter()
        .map(|p| p.joined())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains("Mesures finies (1)"), "{text}");
    assert!(text.contains("Hors tol"), "{text}");
    assert!(
        !text.contains('?'),
        "caractère absent de la police dans le PDF"
    );
    // Lecture seule : le projet n'a pas changé.
    let before = persist::to_json(&f.p).unwrap();
    let _ = export_control_report(&f.p, ReportFormat::Pdf, Lang::En);
    assert_eq!(persist::to_json(&f.p).unwrap(), before);
}
