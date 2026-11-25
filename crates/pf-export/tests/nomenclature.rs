//! US-11.2 nomenclature et US-11.3 fiche technique : valeurs **calculées à la main** (aucune valeur
//! recopiée de la sortie du code).
//!
//! Jeu : « Dos » 400 × 600 (aire 240 000 mm²), cuir, quantité 2 en paire miroir (4 découpes) ;
//! « Patte » triangle rectangle 300 × 400 (aire 60 000), « cuir » (casse et espaces ignorés), 1 ;
//! « Quart » quart de disque de rayon 100 (aire π·100²/4 = 7 853,9816…), doublure, 3 ;
//! « Sans » carré 10 × 10 (aire 100), sans matière, 1. Taille M : le coin (400 ; 600) du dos est
//! décalé de (+20 ; +10) : aire = ½·(2·400·600 + 400·10 + 20·600) = 248 000.

mod common;
mod nomenc_support;

use common::csv_reader::parse;
use common::report_pdf::pages;
use nomenc_support::*;
use pf_core::*;
use pf_export::bom_export::{export_bom, export_tech_sheet};
use pf_export::report::ReportFormat;
use pf_i18n::Lang;

const QUARTER: f64 = std::f64::consts::PI * 100.0 * 100.0 / 4.0;

fn jeu() -> (Project, SizeRangeId) {
    let mut p = Project::new("Sac");
    let dos = poly_piece(
        &mut p,
        &[(0.0, 0.0), (400.0, 0.0), (400.0, 600.0), (0.0, 600.0)],
        "Dos",
    );
    let patte = poly_piece(
        &mut p,
        &[(1000.0, 0.0), (1300.0, 0.0), (1000.0, 400.0)],
        "Patte",
    );
    let quart = quarter_piece(&mut p, 2000.0, 0.0, 100.0, "Quart");
    let sans = poly_piece(
        &mut p,
        &[(3000.0, 0.0), (3010.0, 0.0), (3010.0, 10.0), (3000.0, 10.0)],
        "Sans",
    );
    card(&mut p, dos, 2, true, Some("Cuir"), None);
    card(&mut p, patte, 1, false, Some(" cuir "), None);
    card(&mut p, quart, 3, false, Some("Doublure"), None);
    card(&mut p, sans, 1, false, None, None);
    let range = add_range(&mut p, &["S", "M"], 0);
    grade(&mut p, range, 1, (400.0, 600.0), (20.0, 10.0));
    (p, range)
}

fn close(a: Option<f64>, b: f64) -> bool {
    a.is_some_and(|a| (a - b).abs() < 1e-6)
}

#[test]
fn quantites_matieres_et_surfaces_nettes_a_la_main() {
    let (p, range) = jeu();
    let bom = p.bill_of_materials(Some(range)).unwrap();
    assert_eq!(bom.labels, ["S", "M"]);
    assert_eq!(bom.lines.len(), 4);
    let cuts: Vec<u32> = bom.lines.iter().map(|l| l.cuts).collect();
    assert_eq!(cuts, [4, 1, 3, 1]);
    // Aires d'une pièce, par taille.
    assert!(close(bom.lines[0].areas_mm2[0], 240_000.0));
    assert!(close(bom.lines[0].areas_mm2[1], 248_000.0));
    assert!(close(bom.lines[1].areas_mm2[0], 60_000.0));
    assert!(close(bom.lines[1].areas_mm2[1], 60_000.0));
    assert!(close(bom.lines[2].areas_mm2[0], QUARTER));
    assert!(close(bom.lines[3].areas_mm2[1], 100.0));
    // Matières : Cuir (Dos + Patte regroupées), Doublure, puis « non renseignée » en dernier.
    let names: Vec<Option<&str>> = bom
        .materials
        .iter()
        .map(|m| m.material.as_deref())
        .collect();
    assert_eq!(names, [Some("Cuir"), Some("Doublure"), None]);
    let cuir = &bom.materials[0];
    assert_eq!((cuir.pieces, cuir.cuts), (2, 5));
    assert!(close(cuir.net_area_mm2[0], 4.0 * 240_000.0 + 60_000.0)); // 1 020 000
    assert!(close(cuir.net_area_mm2[1], 4.0 * 248_000.0 + 60_000.0)); // 1 052 000
    let dbl = &bom.materials[1];
    assert_eq!((dbl.pieces, dbl.cuts), (1, 3));
    assert!(close(dbl.net_area_mm2[0], 3.0 * QUARTER));
    assert!(close(bom.materials[2].net_area_mm2[1], 100.0));
}

#[test]
fn csv_nomenclature_lignes_exactes() {
    let (p, range) = jeu();
    let out = export_bom(&p, Some(range), ReportFormat::Csv, Lang::Fr).unwrap();
    let rows = parse(&out.bytes);
    let piece_head = rows.iter().find(|r| r[0] == "#piece").unwrap();
    assert_eq!(
        piece_head[1..],
        [
            "Pièce",
            "Code",
            "Feuille",
            "Matière",
            "Catégorie",
            "Quantité",
            "Paire miroir",
            "Découpes",
            "Aire S (mm²)",
            "Aire M (mm²)"
        ]
    );
    let sheet = &p.sheets[0].name;
    let piece_rows: Vec<&Vec<String>> = rows.iter().filter(|r| r[0] == "piece").collect();
    assert_eq!(piece_rows.len(), 4);
    assert_eq!(
        piece_rows[0][1..],
        [
            "Dos",
            "",
            sheet,
            "Cuir",
            "",
            "2",
            "oui",
            "4",
            "240000.00",
            "248000.00"
        ]
    );
    // « cuir » est écrit tel que saisi (espaces de bord retirés), la paire est vide si non cochée.
    assert_eq!(piece_rows[1][4], "cuir");
    assert_eq!(piece_rows[1][7], "");
    assert_eq!(piece_rows[2][9], "7853.98");
    assert_eq!(piece_rows[3][4], "non renseignée");
    let mat: Vec<&Vec<String>> = rows.iter().filter(|r| r[0] == "matiere").collect();
    assert_eq!(mat.len(), 3);
    assert_eq!(mat[0][1..], ["Cuir", "2", "5", "1020000.00", "1052000.00"]);
    assert_eq!(mat[1][1..], ["Doublure", "1", "3", "23561.94", "23561.94"]);
    assert_eq!(
        mat[2][1..],
        ["non renseignée", "1", "1", "100.00", "100.00"]
    );
    assert_eq!(out.bytes[..3], [0xEF, 0xBB, 0xBF]);
}

#[test]
fn pdf_nomenclature_valeurs_et_texte_sans_point_d_interrogation() {
    let (p, range) = jeu();
    for lang in [Lang::Fr, Lang::En] {
        let out = export_bom(&p, Some(range), ReportFormat::Pdf, lang).unwrap();
        assert!(out.bytes.starts_with(b"%PDF"));
        let text: String = pages(&out.bytes)
            .iter()
            .map(|pg| pg.joined())
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(out.page_count, Some(pages(&out.bytes).len()));
        for v in [
            "1020000", "1052000", "248000", "240000", "23562", "7854", "Dos", "Patte",
        ] {
            assert!(text.lines().any(|l| l == v), "{lang:?}: « {v} » absent");
        }
        assert!(!text.contains('?'), "{lang:?}: caractère non écrit");
        let title = if lang == Lang::Fr {
            "Nomenclature"
        } else {
            "Bill of materials"
        };
        assert!(text.contains(title));
    }
}

#[test]
fn aire_illisible_rend_le_total_illisible() {
    let mut p = Project::new("P");
    let a = poly_piece(
        &mut p,
        &[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)],
        "Carre",
    );
    // Papillon : contour qui se recoupe, aire sans sens métier.
    let b = poly_piece(
        &mut p,
        &[(100.0, 0.0), (200.0, 100.0), (200.0, 0.0), (100.0, 100.0)],
        "Noeud",
    );
    card(&mut p, a, 1, false, Some("Toile"), None);
    card(&mut p, b, 2, false, Some("Toile"), None);
    let bom = p.bill_of_materials(None).unwrap();
    assert_eq!(bom.labels.len(), 0);
    assert!(close(bom.lines[0].areas_mm2[0], 100.0));
    assert_eq!(bom.lines[1].areas_mm2, [None]);
    assert_eq!(bom.materials[0].net_area_mm2, [None]);
    assert_eq!(bom.materials[0].cuts, 3);
    let csv = parse(
        &export_bom(&p, None, ReportFormat::Csv, Lang::Fr)
            .unwrap()
            .bytes,
    );
    let head = csv.iter().find(|r| r[0] == "#matiere").unwrap();
    assert_eq!(head[4], "Surface nette Projet (mm²)");
    let m = csv.iter().find(|r| r[0] == "matiere").unwrap();
    assert_eq!(m[1..], ["Toile", "2", "3", ""]);
    let pdf = export_bom(&p, None, ReportFormat::Pdf, Lang::Fr).unwrap();
    let text = pages(&pdf.bytes)
        .iter()
        .map(|pg| pg.joined())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.lines().any(|l| l == "—"));
}

#[test]
fn lecture_seule_gamme_inconnue_et_projet_vide() {
    let (p, range) = jeu();
    let before = format!("{p:?}");
    for f in [ReportFormat::Csv, ReportFormat::Pdf] {
        export_bom(&p, Some(range), f, Lang::Fr).unwrap();
    }
    export_tech_sheet(&p, Some(range), Some("x"), Lang::Fr).unwrap();
    assert_eq!(before, format!("{p:?}"));
    assert!(export_bom(&p, Some(SizeRangeId(999_999)), ReportFormat::Csv, Lang::Fr).is_err());
    let empty = Project::new("Vide");
    let bom = empty.bill_of_materials(None).unwrap();
    assert!(bom.lines.is_empty() && bom.materials.is_empty());
    assert!(export_bom(&empty, None, ReportFormat::Pdf, Lang::En)
        .unwrap()
        .bytes
        .starts_with(b"%PDF"));
    assert!(export_tech_sheet(&empty, None, None, Lang::En)
        .unwrap()
        .bytes
        .starts_with(b"%PDF"));
}

fn fiche_text(p: &Project, range: Option<SizeRangeId>, note: Option<&str>, lang: Lang) -> String {
    let out = export_tech_sheet(p, range, note, lang).unwrap();
    fiche_pages(&out.bytes)
        .iter()
        .flat_map(|pg| pg.texts.clone())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn fiche_technique_champs_absents_non_renseignes_jamais_devines() {
    let (p, range) = jeu();
    let fr = fiche_text(&p, Some(range), None, Lang::Fr);
    assert!(fr.contains("Fiche technique"));
    for name in ["Dos", "Patte", "Quart", "Sans"] {
        assert!(
            fr.lines().any(|l| l == name),
            "vignette « {name} » sans nom"
        );
    }
    // Aucun barème : « non renseigné », pas de tableau de mesures inventé.
    assert!(fr.contains("non renseigné (aucun barème"));
    // Aucun commentaire saisi : la zone est un cadre vide, sans ligne de note.
    assert!(fr.contains("Commentaires"));
    assert!(!fr.contains("Note"));
    let en = fiche_text(&p, Some(range), None, Lang::En);
    assert!(en.contains("Technical sheet") && en.contains("Comments"));
    assert!(!en.contains('?'));
}

#[test]
fn fiche_technique_bareme_et_commentaires() {
    let (mut p, range) = jeu();
    p.apply(Command::AddMeasurement {
        name: "Tour de taille".into(),
        unit: MeasurementUnit::Mm,
    })
    .unwrap();
    p.apply(Command::AddMeasurement {
        name: "Hauteur".into(),
        unit: MeasurementUnit::Mm,
    })
    .unwrap();
    let (m1, m2) = (p.measurements[0].id, p.measurements[1].id);
    let (s, mm) = {
        let r = p.size_range(range).unwrap();
        (r.sizes[0].id, r.sizes[1].id)
    };
    p.apply(Command::SetMeasurementValue {
        measurement: m1,
        size: s,
        value: 700.0,
    })
    .unwrap();
    p.apply(Command::SetMeasurementValue {
        measurement: m1,
        size: mm,
        value: 725.46,
    })
    .unwrap();
    p.apply(Command::SetMeasurementValue {
        measurement: m2,
        size: s,
        value: 380.0,
    })
    .unwrap();
    let dos = p.pieces[0].id;
    card(&mut p, dos, 2, true, Some("Cuir"), Some("Coudre à plat"));
    let text = fiche_text(&p, Some(range), Some("À valider"), Lang::Fr);
    for v in [
        "Tour de taille",
        "700",
        "725.5",
        "380",
        "non renseigné",
        "Coudre à plat",
        "À valider",
    ] {
        assert!(text.lines().any(|l| l == v), "« {v} » absent :\n{text}");
    }
    // La note n'est pas stockée.
    assert!(!format!("{p:?}").contains("À valider"));
}

/// Boîte (largeur, hauteur) des segments de vignette dont les deux x tombent dans `[x0 ; x1)`
/// (les filets de tableau et le cadre de commentaires, qui touchent la marge, sont écartés).
fn vignette_box(bytes: &[u8], x0: f64, x1: f64) -> Option<(f64, f64)> {
    let segs: Vec<[f64; 4]> = fiche_pages(bytes)
        .into_iter()
        .flat_map(|p| p.segments)
        .filter(|s| s[0] >= x0 + 1.0 && s[2] >= x0 + 1.0 && s[0] < x1 - 1.0 && s[2] < x1 - 1.0)
        .collect();
    if segs.is_empty() {
        return None;
    }
    let xs: Vec<f64> = segs.iter().flat_map(|s| [s[0], s[2]]).collect();
    let ys: Vec<f64> = segs.iter().flat_map(|s| [s[1], s[3]]).collect();
    let span = |v: &[f64]| {
        v.iter().cloned().fold(f64::MIN, f64::max) - v.iter().cloned().fold(f64::MAX, f64::min)
    };
    Some((span(&xs), span(&ys)))
}

#[test]
fn vignettes_vectorielles_ajustees_a_leur_case() {
    let (p, range) = jeu();
    let out = export_tech_sheet(&p, Some(range), None, Lang::Fr).unwrap();
    // Cases de 45 mm à partir de x = 15 ; dessin utile 39 × 30 mm.
    // Dos 400 × 600 : échelle min(39/400 ; 30/600) = 0,05 -> 20 × 30 mm.
    let (w, h) = vignette_box(&out.bytes, 15.0, 60.0).unwrap();
    assert!(
        (w - 20.0).abs() < 0.01 && (h - 30.0).abs() < 0.01,
        "{w} x {h}"
    );
    // Patte 300 × 400 : échelle min(39/300 ; 30/400) = 0,075 -> 22,5 × 30 mm.
    let (w, h) = vignette_box(&out.bytes, 60.0, 105.0).unwrap();
    assert!(
        (w - 22.5).abs() < 0.01 && (h - 30.0).abs() < 0.01,
        "{w} x {h}"
    );
    // Quart de disque de rayon 100 : échelle min(0,39 ; 0,3) = 0,3 -> 30 × 30 mm.
    let (w, h) = vignette_box(&out.bytes, 105.0, 150.0).unwrap();
    assert!(
        (w - 30.0).abs() < 0.01 && (h - 30.0).abs() < 0.01,
        "{w} x {h}"
    );
    // Carré 10 × 10 : échelle 3 -> 30 × 30.
    let (w, h) = vignette_box(&out.bytes, 150.0, 195.0).unwrap();
    assert!(
        (w - 30.0).abs() < 0.01 && (h - 30.0).abs() < 0.01,
        "{w} x {h}"
    );
}
