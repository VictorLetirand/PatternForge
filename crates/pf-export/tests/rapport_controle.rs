//! US-08.5 — rapport de contrôle (CSV et PDF). Les valeurs attendues du scénario de référence sont
//! calculées **à la main** (voir `common/rapport.rs::Ca1`) et écrites en dur ; le CSV est relu par
//! un lecteur RFC 4180 indépendant (`common/csv_reader.rs`) et le PDF par `common/report_pdf.rs`
//! (table `ToUnicode` du fichier, police du dépôt lue sur disque).
//!
//! CA1 valeurs du scénario, CA2 aller-retour CSV, CA3 PDF et pagination, CA4 déterminisme et lecture
//! seule, CA5 projet vide, CA6 refus sans trace, CA7 honnêteté des textes.

mod common;

use common::csv_reader::{number, parse, sections, Section};
use common::pdf_text::repo_font;
use common::rapport::{ca1, declare, poly, Ca1};
use common::report_pdf::{media_boxes, pages};
use pf_core::persist;
use pf_core::*;
use pf_export::report::{export_control_report, ReportFormat};
use pf_i18n::Lang;
use std::collections::BTreeMap;

fn csv(p: &Project, lang: Lang) -> BTreeMap<String, Section> {
    sections(&parse(
        &export_control_report(p, ReportFormat::Csv, lang).bytes,
    ))
}

fn row(cells: &[&str]) -> Vec<String> {
    cells.iter().map(|c| c.to_string()).collect()
}

fn ids(e: &[EntityId]) -> String {
    e.iter()
        .map(|x| x.0.to_string())
        .collect::<Vec<_>>()
        .join("+")
}

// ----- CA1 : valeurs calculées à la main ------------------------------------------------------

#[test]
fn ca1_les_lignes_csv_contiennent_les_nombres_calcules_a_la_main() {
    let c = ca1();
    let s = csv(&c.p, Lang::Fr);
    let sheet = c.p.sheets[0].name.clone();

    // Coutures : 110 − 100 = 10 mm d'embu mesuré, 10 attendu, écart 0, tolérance 1.
    assert_eq!(
        s["couture"].rows,
        vec![row(&[
            "1",
            "Devant",
            &c.ea[1].0.to_string(),
            "Dos",
            &c.eb[0].0.to_string(),
            "110.00",
            "100.00",
            "10.00",
            "10.00",
            "10.00",
            "0.00",
            "1.00",
            "Dans la tolérance",
            ""
        ])]
    );
    // Pièces : 50 × 110 ; trapèze (aire 4779,591275, périmètre 291,954982).
    assert_eq!(
        s["piece"].rows,
        vec![
            row(&[
                &sheet,
                "Devant",
                "",
                "",
                "1",
                "",
                "5500.00",
                "320.00",
                "oui",
                "droit-fil manquant"
            ]),
            row(&[
                &sheet,
                "Dos",
                "",
                "",
                "1",
                "",
                "4779.59",
                "291.95",
                "oui",
                "droit-fil manquant"
            ]),
        ]
    );
    // Crans : A 22 sans vis-à-vis (attendu à 80 mm de B) ; A 55 ↔ B 50 (55 × 100 / 110 = 50).
    assert_eq!(
        s["crans"].rows,
        vec![
            row(&[
                "1",
                "A",
                &c.n_a22.0.to_string(),
                "22.00",
                "80.00",
                "",
                "",
                "1.00",
                "Manquant",
                ""
            ]),
            row(&[
                "1",
                "A",
                &c.n_a55.0.to_string(),
                "55.00",
                "50.00",
                "50.00",
                "0.00",
                "1.00",
                "Apparié",
                ""
            ]),
            row(&[
                "1",
                "B",
                &c.n_b50.0.to_string(),
                "50.00",
                "55.00",
                "55.00",
                "0.00",
                "1.00",
                "Apparié",
                ""
            ]),
        ]
    );
    // Raccords : 90 + 80 = 170 (écart −10, cassé), 90 + 90 = 180 (lisse).
    assert_eq!(
        s["raccord"].rows,
        vec![
            row(&[
                "1",
                "Début",
                "Fin",
                "90.00",
                "80.00",
                "170.00",
                "-10.00",
                "2.00",
                "Cassé : creux",
                ""
            ]),
            row(&["1", "Fin", "Début", "90.00", "90.00", "180.00", "0.00", "2.00", "Lisse", ""]),
        ]
    );
    // Anomalies : triées par type (cran, raccord, pièces), 4 au total.
    assert_eq!(
        s["anomalie"].rows,
        vec![
            row(&[
                "Cran sans vis-à-vis",
                &format!("Couture 1, côté A, cran {}", c.n_a22.0),
                "Aucun vis-à-vis trouvé sur l'autre bord",
                "",
                "1.00",
                ""
            ]),
            row(&[
                "Raccord cassé",
                "Couture 1, bout Début",
                "Écart à 180° : -10.00° (tolérance 2.00°)",
                "-10.00",
                "2.00",
                ""
            ]),
            row(&[
                "Avertissement de pièce",
                "Pièce « Devant »",
                "droit-fil manquant",
                "",
                "",
                ""
            ]),
            row(&[
                "Avertissement de pièce",
                "Pièce « Dos »",
                "droit-fil manquant",
                "",
                "",
                ""
            ]),
        ]
    );
    // Synthèse : en-tête et compteurs.
    let resume: BTreeMap<String, String> = s["resume"]
        .rows
        .iter()
        .map(|r| (r[0].clone(), r[1].clone()))
        .collect();
    assert_eq!(resume["Projet"], "Veste");
    assert_eq!(resume["Version du format"], "6");
    assert_eq!(resume["Feuilles"], "1");
    assert_eq!(resume["Pièces"], "2");
    assert_eq!(resume["Couples cousus"], "1");
    assert_eq!(resume["Anomalies"], "4");
    assert_eq!(resume["Cran sans vis-à-vis"], "1");
    assert_eq!(resume["Raccord cassé"], "1");
    assert_eq!(resume["Avertissement de pièce"], "2");
    assert_eq!(resume["Couture hors tolérance"], "0");
    assert_eq!(resume["Synthèse"], "4 anomalie(s)");
    assert_eq!(
        resume["Avertissement"],
        "Mesures observées, non validées par une modéliste"
    );
}

#[test]
fn ca1_le_scenario_hors_tolerance_est_signale_comme_tel() {
    // Embu attendu 4 mm au lieu de 10 : écart 6 mm > 1 mm.
    let mut c = ca1();
    let sa = c.p.seam_side_range(c.a, c.ea[1], 1).unwrap();
    let sb = c.p.seam_side_range(c.b, c.eb[0], 1).unwrap();
    c.p.apply(Command::SetSeamPair {
        id: c.pair,
        a: sa,
        b: sb,
        embu_mm: Some(4.0),
        tolerance_mm: None,
    })
    .unwrap();
    let s = csv(&c.p, Lang::Fr);
    assert_eq!(s["couture"].rows[0][10], "6.00");
    assert_eq!(s["couture"].rows[0][12], "Hors tolérance");
    assert_eq!(s["anomalie"].rows[0][0], "Couture hors tolérance");
    assert_eq!(
        s["anomalie"].rows[0][2],
        "Écart à l'embu attendu : 6.00 mm (tolérance 1.00 mm)"
    );
    assert_eq!(s["anomalie"].rows[0][3], "6.00");
}

// ----- CA2 : lecteur CSV indépendant, champs spéciaux ------------------------------------------

#[test]
fn ca2_les_champs_speciaux_font_l_aller_retour_et_les_nombres_tiennent_au_centieme() {
    let mut c = ca1();
    c.p.name = "=1+1;\"x\" ligne 2".into();
    let nasty = "Éléonore; \"Œuvre\" suite = 1";
    c.p.apply(Command::SetPieceCard {
        piece: c.a,
        name: "Dev;ant \"A\"".into(),
        code: Some("=CMD|' /C calc'!A0".into()),
        quantity: 2,
        mirrored_pair: false,
        material: Some(nasty.into()),
        category: Some(PieceCategory::Lining),
        comment: None,
    })
    .unwrap();
    let bytes = export_control_report(&c.p, ReportFormat::Csv, Lang::Fr).bytes;
    assert_eq!(&bytes[..3], &[0xEF, 0xBB, 0xBF], "BOM UTF-8");
    // Un LF isolé n'est permis que dans un champ entre guillemets : le lecteur indépendant
    // échoue sur toute autre forme.
    assert!(bytes.ends_with(b"\r\n"));
    let s = sections(&parse(&bytes));
    // Texte commençant par « = » : apostrophe ajoutée, le reste intact.
    let resume: BTreeMap<String, String> = s["resume"]
        .rows
        .iter()
        .map(|r| (r[0].clone(), r[1].clone()))
        .collect();
    assert_eq!(resume["Projet"], "'=1+1;\"x\" ligne 2");
    let piece_a = &s["piece"].rows[0];
    assert_eq!(piece_a[1], "Dev;ant \"A\"");
    assert_eq!(piece_a[2], "'=CMD|' /C calc'!A0");
    assert_eq!(piece_a[3], "Doublure");
    assert_eq!(piece_a[4], "2");
    assert_eq!(piece_a[5], nasty);
    // Nombres à 0,01 près, relus et comparés au recalcul direct (jamais NaN / inf).
    let card = c.p.piece_card(c.a).unwrap();
    let area = number(&piece_a[6]).unwrap();
    assert!((area - card.area_mm2.unwrap()).abs() <= 0.005 + 1e-9);
    let per = number(&piece_a[7]).unwrap();
    assert!((per - card.perimeter_mm.unwrap()).abs() <= 0.005 + 1e-9);
    // Le nombre négatif du raccord n'est jamais préfixé d'une apostrophe.
    assert_eq!(s["raccord"].rows[0][6], "-10.00");
    // Toutes les lignes de chaque section ont la largeur de l'en-tête (vérifié par `sections`).
    for code in ["resume", "anomalie", "piece", "couture", "crans", "raccord"] {
        assert!(s.contains_key(code), "section {code}");
    }
}

#[test]
fn ca2_retours_ligne_et_guillemets_dans_un_rapport_construit_a_la_main() {
    // Le projet refuse les caractères de contrôle ; le formateur, lui, doit rester correct.
    let c = ca1();
    let mut r = c.p.control_report();
    let wild = "ligne 1\r\nligne 2\n\"cité\"; fin\r";
    r.project_name = format!("={wild}");
    r.pieces[1].name = wild.to_string();
    r.pieces[1].code = Some("\tTab".into());
    r.pieces[1].material = Some("@x".into());
    let out = pf_export::report::format_control_report(&r, ReportFormat::Csv, Lang::Fr);
    let s = sections(&parse(&out.bytes));
    let resume: BTreeMap<String, String> = s["resume"]
        .rows
        .iter()
        .map(|x| (x[0].clone(), x[1].clone()))
        .collect();
    assert_eq!(resume["Projet"], format!("'={wild}"));
    assert_eq!(s["piece"].rows[1][1], wild);
    assert_eq!(s["piece"].rows[1][2], "'\tTab");
    assert_eq!(s["piece"].rows[1][5], "'@x");
    // Le PDF du même rapport reste dans la page (les retours ligne deviennent des espaces).
    let pdf = pf_export::report::format_control_report(&r, ReportFormat::Pdf, Lang::Fr);
    assert_in_printable_area(&pdf.bytes);
}

#[test]
fn ca2_un_nombre_non_calculable_est_un_champ_vide_avec_sa_raison() {
    // Contour de B rendu ouvert : le couple n'est plus calculable.
    let mut c = ca1();
    let outline: Vec<PieceEdge> = c.p.piece(c.b).unwrap().outline[..3].to_vec();
    c.p.apply(Command::SetPieceOutline {
        piece: c.b,
        outline,
        grainline: None,
    })
    .unwrap();
    let s = csv(&c.p, Lang::Fr);
    let seam = &s["couture"].rows[0];
    assert_eq!(seam[5], "110.00");
    assert_eq!(seam[6], "", "longueur de B non calculable = champ vide");
    assert_eq!(seam[12], "Non calculable");
    assert_eq!(seam[13], "Côté B : contour ouvert ou illisible");
    assert_eq!(s["anomalie"].rows[0][0], "Couture non calculable");
    assert_eq!(
        s["anomalie"].rows[0][2],
        "Côté B : contour ouvert ou illisible"
    );
    // Pas de doublon : ni raccord ni cran « non calculable » pour un couple déjà signalé.
    assert!(s["anomalie"]
        .rows
        .iter()
        .all(|r| r[0] != "Raccord non calculable" && r[0] != "Crans non calculables"));
}

// ----- CA3 : PDF ---------------------------------------------------------------------------------

/// Les mots d'une expression se retrouvent dans le texte (une cellule étroite peut couper
/// l'expression sur deux lignes : l'ordre exact n'est pas exigé).
fn words_present(text: &str, needle: &str) -> bool {
    let have: std::collections::BTreeSet<&str> = text.split_whitespace().collect();
    needle.split_whitespace().all(|w| have.contains(w))
}

fn has_text(page: &common::report_pdf::Page, t: &str) -> bool {
    page.texts.iter().any(|o| o.text == t)
}

#[test]
fn ca3_le_pdf_relu_contient_les_valeurs_du_scenario() {
    let c = ca1();
    let bytes = export_control_report(&c.p, ReportFormat::Pdf, Lang::Fr).bytes;
    assert!(bytes.starts_with(b"%PDF-1.4"));
    let pgs = pages(&bytes);
    let all: Vec<_> = pgs.iter().flat_map(|p| p.texts.iter()).collect();
    for v in [
        "110.00", "100.00", "10.00", "1.00", "170.00", "-10.00", "90.00", "80.00", "180.00",
        "2.00", "5500.00", "320.00", "4779.59", "291.95", "22.00", "55.00", "50.00", "0.00",
    ] {
        assert!(all.iter().any(|o| o.text == v), "valeur {v} absente du PDF");
    }
    let text = pgs
        .iter()
        .map(|p| p.joined())
        .collect::<Vec<_>>()
        .join("\n");
    for needle in [
        "Rapport de contrôle",
        "Anomalies (4)",
        "Cran sans vis-à-vis",
        "Raccord cassé",
        "Dans la tolérance",
        "Cassé : creux",
        "Manquant",
        "Apparié",
        "droit-fil manquant",
    ] {
        assert!(words_present(&text, needle), "« {needle} » absent du PDF");
    }
    assert!(
        !text.contains('?'),
        "caractère absent de la police dans le PDF"
    );
    // Pied de page et numéros de page.
    let n = pgs.len();
    for (i, p) in pgs.iter().enumerate() {
        let foot = format!(
            "Mesures observées, non validées par une modéliste - page {}/{}",
            i + 1,
            n
        );
        assert!(has_text(p, &foot), "pied de page de la page {}", i + 1);
    }
    assert_eq!(
        export_control_report(&c.p, ReportFormat::Pdf, Lang::Fr).page_count,
        Some(n)
    );
}

/// Pages A4 portrait ; chaque ligne de texte est dans la zone imprimable (largeur réelle lue dans
/// la police du dépôt) ; chaque filet aussi.
fn assert_in_printable_area(bytes: &[u8]) {
    let font = repo_font();
    let boxes = media_boxes(bytes);
    let pgs = pages(bytes);
    assert_eq!(boxes.len(), pgs.len());
    for (w, h) in &boxes {
        assert!(
            (w - 210.0).abs() < 0.01 && (h - 297.0).abs() < 0.01,
            "A4 portrait"
        );
    }
    for (i, p) in pgs.iter().enumerate() {
        for t in &p.texts {
            let w = font.width_mm(&t.text, t.size_pt);
            assert!(
                t.x_mm >= 15.0 - 1e-3,
                "p{} x={} « {} »",
                i + 1,
                t.x_mm,
                t.text
            );
            assert!(
                t.x_mm + w <= 195.0 + 1e-3,
                "p{} déborde à droite : x={} w={w} « {} »",
                i + 1,
                t.x_mm,
                t.text
            );
            assert!(
                t.y_mm >= 10.0 && t.y_mm <= 297.0 - 12.0,
                "p{} y={} « {} »",
                i + 1,
                t.y_mm,
                t.text
            );
        }
        for r in &p.rules {
            assert!(r.x1_mm >= 15.0 - 1e-3 && r.x2_mm <= 195.0 + 1e-3);
            assert!(r.y_mm >= 15.0 && r.y_mm <= 282.0 + 1e-3);
        }
    }
}

#[test]
fn ca3_deux_cents_couples_remplissent_plusieurs_pages_sans_rien_deborder() {
    let mut p = Project::new("Grand projet");
    let s = p.sheets[0].id;
    let mut count = 0;
    for k in 0..100 {
        let x0 = 150.0 * f64::from(k % 10);
        let y0 = 150.0 * f64::from(k / 10);
        let (piece, e) = poly(
            &mut p,
            s,
            &[
                (x0, y0),
                (x0 + 100.0, y0),
                (x0 + 100.0, y0 + 100.0),
                (x0, y0 + 100.0),
            ],
            &format!("Pièce {k} avec un nom assez long pour passer à la ligne dans sa colonne"),
        );
        declare(&mut p, (piece, e[0]), (piece, e[2]), None, None);
        declare(&mut p, (piece, e[1]), (piece, e[3]), Some(2.0), Some(0.5));
        count += 2;
    }
    assert_eq!(p.seams.len(), 200);
    assert_eq!(count, 200);
    let bytes = export_control_report(&p, ReportFormat::Pdf, Lang::Fr).bytes;
    let pgs = pages(&bytes);
    assert!(pgs.len() >= 4, "{} page(s)", pgs.len());
    assert_in_printable_area(&bytes);
    // Numéros de page justes sur chaque page.
    for (i, pg) in pgs.iter().enumerate() {
        let foot = format!("page {}/{}", i + 1, pgs.len());
        assert!(
            pg.texts.iter().any(|t| t.text.ends_with(&foot)),
            "pied de la page {}",
            i + 1
        );
    }
    // Les 200 couples sont tous là (numéros de couple de 1 à 200 dans la section des coutures).
    let text = pgs
        .iter()
        .map(|p| p.joined())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains("Coutures appariées (200)"));
    // En-tête de tableau répété sur les pages de suite et titre de suite.
    assert!(pgs[1]
        .texts
        .iter()
        .any(|t| t.text == "Rapport de contrôle (suite)"));
}

#[test]
fn ca3_les_textes_speciaux_et_les_mots_tres_longs_restent_dans_la_page() {
    for seed in 0..6u64 {
        let p = common::rapport::random_project(seed);
        for lang in Lang::ALL {
            let bytes = export_control_report(&p, ReportFormat::Pdf, lang).bytes;
            assert_in_printable_area(&bytes);
        }
    }
}

// ----- CA4 : déterminisme, lecture seule ---------------------------------------------------------

#[test]
fn ca4_sortie_deterministe_et_projet_inchange() {
    let c = ca1();
    let before = persist::to_json(&c.p).unwrap();
    let project_before = c.p.clone();
    for format in [ReportFormat::Csv, ReportFormat::Pdf] {
        for lang in Lang::ALL {
            let a = export_control_report(&c.p, format, lang);
            let b = export_control_report(&c.p, format, lang);
            assert_eq!(a, b, "{format:?} {lang:?}");
        }
    }
    assert_eq!(persist::to_json(&c.p).unwrap(), before, "octets du projet");
    assert_eq!(c.p, project_before);
    // Un clone sérialisé puis relu donne le même rapport (aucune dépendance à un état caché).
    let reloaded = persist::from_json(&before).unwrap();
    assert_eq!(
        export_control_report(&reloaded, ReportFormat::Csv, Lang::Fr),
        export_control_report(&c.p, ReportFormat::Csv, Lang::Fr)
    );
    assert_eq!(
        export_control_report(&reloaded, ReportFormat::Pdf, Lang::Fr),
        export_control_report(&c.p, ReportFormat::Pdf, Lang::Fr)
    );
}

#[test]
fn ca4_ordre_stable_des_anomalies_par_type_puis_couple() {
    let c: Ca1 = ca1();
    let r = c.p.control_report();
    let kinds: Vec<_> = r.anomalies.iter().map(|a| a.kind).collect();
    let mut sorted = kinds.clone();
    sorted.sort();
    assert_eq!(kinds, sorted);
    assert_eq!(r.anomalies.len(), 4);
}

// ----- CA5 : projet vide ou sans couture ---------------------------------------------------------

#[test]
fn ca5_projet_vide_ou_sans_couture_donne_un_rapport_valide_et_aucune_anomalie() {
    let empty = Project::new("Vide");
    for lang in Lang::ALL {
        let s = csv(&empty, lang);
        for code in ["resume", "anomalie", "piece", "couture", "crans", "raccord"] {
            assert!(s.contains_key(code), "{code}");
        }
        for code in ["anomalie", "piece", "couture", "crans", "raccord"] {
            assert!(s[code].rows.is_empty(), "{code}");
        }
        let pdf = export_control_report(&empty, ReportFormat::Pdf, lang).bytes;
        assert_in_printable_area(&pdf);
        let text = pages(&pdf)
            .iter()
            .map(|p| p.joined())
            .collect::<Vec<_>>()
            .join("\n");
        let none = pf_i18n::lookup(lang, "report.anom.none");
        assert!(text.contains(&none), "{none}");
        assert!(s["resume"].rows.iter().any(|r| r[1] == none));
    }
    // Une pièce sans couture : une ligne de pièce, aucune anomalie de couture.
    let mut one = Project::new("Une pièce");
    let sh = one.sheets[0].id;
    let (piece, _) = poly(&mut one, sh, &[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)], "T");
    one.apply(Command::SetPieceOutline {
        piece,
        outline: one.piece(piece).unwrap().outline.clone(),
        grainline: one.piece(piece).unwrap().outline.first().map(|e| e.entity),
    })
    .ok();
    let s = csv(&one, Lang::Fr);
    assert_eq!(s["piece"].rows.len(), 1);
    assert!(s["couture"].rows.is_empty());
}

#[test]
fn ca5_un_fichier_existant_sans_couture_reste_identique_octet_pour_octet() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples");
    for name in ["jupe-droite.pforge", "porte-cartes.pforge"] {
        let raw = std::fs::read(format!("{dir}/{name}")).unwrap();
        let p = persist::from_json(std::str::from_utf8(&raw).unwrap()).unwrap();
        assert!(p.seams.is_empty(), "{name} : 0 couture");
        let before = persist::to_json(&p).unwrap();
        for format in [ReportFormat::Csv, ReportFormat::Pdf] {
            let r = export_control_report(&p, format, Lang::Fr);
            assert!(!r.bytes.is_empty());
        }
        assert_eq!(persist::to_json(&p).unwrap(), before, "{name}");
        // Le fichier relu puis réécrit est celui du dépôt : le rapport n'a rien ajouté.
        assert_eq!(
            before.replace("\r\n", "\n").trim_end(),
            std::str::from_utf8(&raw)
                .unwrap()
                .replace("\r\n", "\n")
                .trim_end(),
            "{name}"
        );
    }
}

// ----- CA6 : refus sans trace --------------------------------------------------------------------

fn scratch(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("pf-rapport-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn listing(d: &std::path::Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(d)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    v.sort();
    v
}

#[test]
fn ca6_destination_illisible_erreur_et_aucun_fichier() {
    let c = ca1();
    let report = export_control_report(&c.p, ReportFormat::Csv, Lang::Fr);
    let dir = scratch("refus");
    // Dossier inexistant.
    let missing = dir.join("absent").join("r.csv");
    assert!(report.write_to(&missing).is_err());
    assert!(!dir.join("absent").exists());
    assert!(listing(&dir).is_empty());
    // Destination = un dossier existant.
    let sub = dir.join("sous.csv");
    std::fs::create_dir(&sub).unwrap();
    assert!(report.write_to(&sub).is_err());
    assert_eq!(listing(&dir), vec!["sous.csv".to_string()]);
    assert!(listing(&sub).is_empty());
    // Destination vide.
    assert!(report.write_to(std::path::Path::new("")).is_err());
    // Fichier existant en lecture seule : intact, aucun temporaire.
    let ro = dir.join("lecture.csv");
    std::fs::write(&ro, b"ancien").unwrap();
    let mut perm = std::fs::metadata(&ro).unwrap().permissions();
    perm.set_readonly(true);
    std::fs::set_permissions(&ro, perm).unwrap();
    let res = report.write_to(&ro);
    assert_eq!(std::fs::read(&ro).unwrap(), b"ancien");
    let mut perm = std::fs::metadata(&ro).unwrap().permissions();
    #[allow(clippy::permissions_set_readonly_false)]
    perm.set_readonly(false);
    std::fs::set_permissions(&ro, perm).unwrap();
    assert!(res.is_err(), "écrasement d'un fichier en lecture seule");
    assert_eq!(
        listing(&dir),
        vec!["lecture.csv".to_string(), "sous.csv".to_string()]
    );
    // Écriture normale : le fichier est exactement le rapport.
    let ok = dir.join("ok.csv");
    report.write_to(&ok).unwrap();
    assert_eq!(std::fs::read(&ok).unwrap(), report.bytes);
    assert!(listing(&dir).contains(&"ok.csv".to_string()));
    assert_eq!(listing(&dir).len(), 3);
    let _ = std::fs::remove_dir_all(&dir);
}

// ----- CA7 : honnêteté ---------------------------------------------------------------------------

const FORBIDDEN_FR: &[&str] = &[
    "conforme",
    "validé",
    "prêt à fabriquer",
    "pret a fabriquer",
    "compatible",
    "approuvé",
];
const FORBIDDEN_EN: &[&str] = &[
    "conforming",
    "compliant",
    "validated",
    "ready to produce",
    "ready to manufacture",
    "compatible",
    "approved",
];

fn assert_honest(lang: Lang, text: &str) {
    let lower = text.to_lowercase();
    // Seule mention permise : la phrase d'avertissement elle-même.
    let notice = pf_i18n::lookup(lang, "report.footer").to_lowercase();
    let lower = lower.replace(&notice, "");
    let words = if lang == Lang::Fr {
        FORBIDDEN_FR
    } else {
        FORBIDDEN_EN
    };
    for w in words {
        assert!(
            !lower.contains(w),
            "« {w} » dans un texte du rapport ({lang:?}) : {lower}"
        );
    }
}

#[test]
fn ca7_aucun_texte_ne_dit_conforme_valide_ou_pret_a_fabriquer() {
    let c = ca1();
    let empty = Project::new("Vide");
    for lang in Lang::ALL {
        for p in [&c.p, &empty] {
            let csv_text =
                String::from_utf8(export_control_report(p, ReportFormat::Csv, lang).bytes).unwrap();
            assert_honest(lang, &csv_text);
            let pdf = export_control_report(p, ReportFormat::Pdf, lang).bytes;
            let text = pages(&pdf)
                .iter()
                .map(|p| p.joined())
                .collect::<Vec<_>>()
                .join("\n");
            assert_honest(lang, &text);
            assert!(text.contains(&pf_i18n::lookup(lang, "report.footer")));
        }
        // Tous les textes du catalogue propres au rapport, au menu et à la fenêtre.
        for (k, v) in lang.catalog() {
            if k.starts_with("report.")
                || *k == "menu.file.export_report"
                || k.starts_with("seams.status")
            {
                assert_honest(lang, v);
            }
        }
    }
}

#[test]
fn les_cles_de_langue_du_rapport_existent_dans_les_deux_catalogues() {
    let src = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/report.rs"))
        .unwrap()
        .replace("\r\n", "\n");
    let code = &src[..src.find("#[cfg(test)]").unwrap_or(src.len())];
    // Tout littéral qui a la forme d'une clé (a.b.c) doit exister dans les deux langues.
    let mut keys = std::collections::BTreeSet::new();
    let mut rest = code;
    while let Some(i) = rest.find('"') {
        let tail = &rest[i + 1..];
        let Some(j) = tail.find('"') else { break };
        let lit = &tail[..j];
        if lit.contains('.')
            && lit.split('.').count() >= 2
            && lit
                .chars()
                .all(|c| c.is_ascii_lowercase() || c == '.' || c == '_')
            && !lit.starts_with('.')
            && !lit.ends_with('.')
        {
            keys.insert(lit.to_string());
        }
        rest = &tail[j + 1..];
    }
    assert!(keys.len() > 60, "scan défaillant : {}", keys.len());
    for k in AnomalyKindKeys::all() {
        keys.insert(format!("report.kind.{k}"));
    }
    for lang in Lang::ALL {
        let cat: std::collections::BTreeSet<&str> =
            lang.catalog().iter().map(|(k, _)| *k).collect();
        let missing: Vec<_> = keys.iter().filter(|k| !cat.contains(k.as_str())).collect();
        assert!(missing.is_empty(), "{lang:?} : {missing:?}");
    }
}

struct AnomalyKindKeys;
impl AnomalyKindKeys {
    fn all() -> Vec<&'static str> {
        pf_core::control_report::AnomalyKind::ALL
            .iter()
            .map(|k| k.key())
            .collect()
    }
}

#[test]
fn ca1_le_pdf_anglais_est_traduit() {
    let c = ca1();
    let bytes = export_control_report(&c.p, ReportFormat::Pdf, Lang::En).bytes;
    let text = pages(&bytes)
        .iter()
        .map(|p| p.joined())
        .collect::<Vec<_>>()
        .join("\n");
    for needle in [
        "Control report",
        "Anomalies (4)",
        "Notch without counterpart",
        "Broken junction",
        "Within tolerance",
        "Observed measurements, not validated by a pattern maker",
    ] {
        assert!(words_present(&text, needle), "« {needle} »");
    }
    assert!(!text.contains('?'));
    assert_in_printable_area(&bytes);
    // Les nombres restent à point décimal.
    assert!(pages(&bytes)
        .iter()
        .flat_map(|p| p.texts.iter())
        .any(|t| t.text == "110.00"));
}

#[test]
fn les_identifiants_de_bord_sont_ceux_du_modele() {
    let c = ca1();
    let r = c.p.control_report();
    assert_eq!(ids(&r.seams[0].edges_a), c.ea[1].0.to_string());
    assert_eq!(ids(&r.seams[0].edges_b), c.eb[0].0.to_string());
}

/// Écrit des échantillons (PDF et CSV du scénario de référence, PDF de 200 couples) dans le dossier
/// donné par `PF_RAPPORT_SORTIE` ; sans cette variable, ne fait rien. Sert à regarder le rendu.
#[test]
fn echantillons_facultatifs_pour_controle_visuel() {
    let Some(dir) = std::env::var_os("PF_RAPPORT_SORTIE") else {
        return;
    };
    let dir = std::path::PathBuf::from(dir);
    std::fs::create_dir_all(&dir).unwrap();
    let c = ca1();
    for (name, format) in [
        ("ca1.pdf", ReportFormat::Pdf),
        ("ca1.csv", ReportFormat::Csv),
    ] {
        export_control_report(&c.p, format, Lang::Fr)
            .write_to(&dir.join(name))
            .unwrap();
    }
    export_control_report(&c.p, ReportFormat::Pdf, Lang::En)
        .write_to(&dir.join("ca1-en.pdf"))
        .unwrap();
    let mut p = Project::new("Grand projet");
    let s = p.sheets[0].id;
    for k in 0..100 {
        let x0 = 150.0 * f64::from(k % 10);
        let y0 = 150.0 * f64::from(k / 10);
        let top = y0 + 90.0 + f64::from(k % 7);
        let (piece, e) = poly(
            &mut p,
            s,
            &[
                (x0, y0),
                (x0 + 100.0, y0),
                (x0 + 100.0, top),
                (x0, y0 + 100.0),
            ],
            &format!("Pièce {k}"),
        );
        declare(&mut p, (piece, e[0]), (piece, e[2]), None, None);
        declare(&mut p, (piece, e[1]), (piece, e[3]), Some(2.0), Some(0.5));
    }
    export_control_report(&p, ReportFormat::Pdf, Lang::Fr)
        .write_to(&dir.join("grand.pdf"))
        .unwrap();
}
