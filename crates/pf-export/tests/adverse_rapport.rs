//! US-08.5 — campagne adverse du rapport de contrôle : 160 projets aléatoires à graine fixe
//! (polygones, couples, crans, déplacements, suppressions, fiches riches à caractères spéciaux).
//!
//! Invariants : jamais de panique ; le nombre de lignes CSV de chaque section et chaque nombre relu
//! (CSV puis PDF) sont **recalculés indépendamment** d'après les lectures du modèle (jamais via
//! `ControlReport`) ; sortie déterministe ; projet inchangé (sérialisation) ; PDF dans la zone
//! imprimable ; anomalies triées par type puis couple. Des compteurs prouvent que les contrôles
//! forts se sont exécutés (un contrôle qui ne compare rien ne prouve rien).

mod common;

use common::csv_reader::{number, parse, sections, Section};
use common::pdf_text::repo_font;
use common::rapport::random_project;
use common::report_pdf::{media_boxes, pages};
use pf_core::notch_check::NotchMatch;
use pf_core::persist;
use pf_core::seam_junction::{JunctionIssue, JunctionStatus};
use pf_core::seam_pair::{SeamStatus, SeamWhich};
use pf_core::*;
use pf_export::report::{export_control_report, ReportFormat};
use pf_i18n::Lang;
use std::collections::BTreeMap;

const SEQUENCES: u64 = 160;

#[derive(Default)]
struct Counters {
    projects: usize,
    seam_rows: usize,
    notch_rows: usize,
    junction_rows: usize,
    piece_rows: usize,
    anomaly_rows: usize,
    numbers_csv: usize,
    numbers_pdf: usize,
    pdf_lines: usize,
    kinds: BTreeMap<String, usize>,
    multi_page: usize,
    texts: usize,
}

/// Protection contre l'injection de formule, réécrite ici : un texte qui commence par `= + - @`,
/// tabulation ou retour chariot est précédé d'une apostrophe.
fn protect(s: &str) -> String {
    if s.starts_with(['=', '+', '-', '@', '\t', '\r']) {
        format!("'{s}")
    } else {
        s.to_string()
    }
}

fn two(v: f64) -> String {
    let s = format!("{v:.2}");
    if s == "-0.00" {
        "0.00".into()
    } else {
        s
    }
}

/// Compare une cellule numérique (relue) à la valeur recalculée : vide ⇔ absente, sinon à 0,01 près.
fn check_num(cell: &str, expected: Option<f64>, what: &str, c: &mut Counters) {
    match (number(cell), expected) {
        (None, None) => {}
        (Some(got), Some(want)) => {
            assert!(
                (got - want).abs() <= 0.005 + 1e-9,
                "{what} : {got} lu pour {want}"
            );
            c.numbers_csv += 1;
        }
        (a, b) => panic!("{what} : lu {a:?}, recalculé {b:?}"),
    }
}

fn type_rank(label: &str) -> usize {
    [
        "Couture hors tolérance",
        "Couture non calculable",
        "Cran sans vis-à-vis",
        "Cran décalé",
        "Crans non calculables",
        "Raccord cassé",
        "Raccord non calculable",
        "Avertissement de pièce",
    ]
    .iter()
    .position(|l| *l == label)
    .unwrap_or_else(|| panic!("type d'anomalie inconnu : {label}"))
}

fn check_csv(p: &Project, s: &BTreeMap<String, Section>, c: &mut Counters) {
    // --- nombres de lignes : comptes directs sur le modèle -------------------------------------
    let seams = p.seam_pair_report();
    let notch = p.notch_check_report();
    let junctions = p.seam_junction_report();
    assert_eq!(s["piece"].rows.len(), p.pieces.len(), "lignes de pièces");
    assert_eq!(s["couture"].rows.len(), p.seams.len(), "lignes de coutures");
    assert_eq!(
        junctions.len(),
        2 * p.seams.len(),
        "deux raccords par couple"
    );
    assert_eq!(
        s["raccord"].rows.len(),
        junctions.len(),
        "lignes de raccords"
    );
    let expected_notch_rows: usize = notch
        .iter()
        .map(|n| {
            if n.not_computable.is_some() {
                1
            } else {
                n.items.len()
            }
        })
        .sum();
    assert_eq!(
        s["crans"].rows.len(),
        expected_notch_rows,
        "lignes de crans"
    );
    c.piece_rows += s["piece"].rows.len();
    c.seam_rows += s["couture"].rows.len();
    c.junction_rows += s["raccord"].rows.len();
    c.notch_rows += s["crans"].rows.len();

    // --- pièces ---------------------------------------------------------------------------------
    for (row, piece) in s["piece"].rows.iter().zip(&p.pieces) {
        let closed = p.piece_contour(piece.id).is_ok_and(|k| k.closed);
        assert_eq!(row[8], if closed { "oui" } else { "non" });
        let contour = p.piece_contour(piece.id).ok().filter(|k| k.closed);
        check_num(&row[6], contour.as_ref().map(|k| k.area_mm2), "aire", c);
        check_num(
            &row[7],
            contour.as_ref().map(|k| k.perimeter_mm),
            "périmètre",
            c,
        );
        assert_eq!(row[4], piece.quantity.to_string());
        // Champs texte : aller-retour exact (apostrophe devant une formule possible).
        assert_eq!(row[1], protect(&piece.name), "nom de pièce");
        assert_eq!(row[2], protect(piece.code.as_deref().unwrap_or("")), "code");
        assert_eq!(
            row[5],
            protect(piece.material.as_deref().unwrap_or("")),
            "matière"
        );
        let sheet = p
            .piece_sheet(piece.id)
            .map(|x| x.name.clone())
            .unwrap_or_default();
        assert_eq!(row[0], protect(&sheet), "feuille");
        c.texts += 4;
    }

    // --- coutures -------------------------------------------------------------------------------
    for (row, r) in s["couture"].rows.iter().zip(&seams) {
        assert_eq!(row[0], r.number.to_string());
        let name = |id: PieceId| p.piece(id).map(|q| q.name.clone()).unwrap_or_default();
        let pair = &p.seams[r.number - 1];
        assert_eq!(row[1], protect(&name(pair.a.piece)), "pièce A");
        assert_eq!(row[3], protect(&name(pair.b.piece)), "pièce B");
        c.texts += 2;
        check_num(&row[5], r.length_a_mm, "longueur A", c);
        check_num(&row[6], r.length_b_mm, "longueur B", c);
        check_num(&row[7], r.gap_mm, "écart", c);
        check_num(&row[8], r.embu_measured_mm, "embu mesuré", c);
        check_num(&row[9], Some(r.embu_expected_mm), "embu attendu", c);
        check_num(&row[10], r.deviation_mm, "écart à l'embu", c);
        check_num(&row[11], Some(r.tolerance_mm), "tolérance", c);
        let status = match r.status {
            SeamStatus::Within => "Dans la tolérance",
            SeamStatus::OutOfTolerance => "Hors tolérance",
            SeamStatus::NotComputable { .. } => "Non calculable",
        };
        assert_eq!(row[12], status);
        assert_eq!(
            row[13].is_empty(),
            !matches!(r.status, SeamStatus::NotComputable { .. }),
            "raison présente exactement si non calculable"
        );
    }

    // --- crans ----------------------------------------------------------------------------------
    let mut it = s["crans"].rows.iter();
    for n in &notch {
        if n.not_computable.is_some() {
            let row = it.next().unwrap();
            assert_eq!(row[0], n.number.to_string());
            assert_eq!(row[8], "Non calculable");
            assert!(!row[9].is_empty());
            continue;
        }
        for item in &n.items {
            let row = it.next().unwrap();
            assert_eq!(row[0], n.number.to_string());
            assert_eq!(row[1], if item.side == SeamWhich::A { "A" } else { "B" });
            assert_eq!(row[2], item.notch.0.to_string());
            check_num(&row[3], Some(item.position_mm), "position du cran", c);
            check_num(&row[4], Some(item.expected_mm), "cran attendu", c);
            check_num(&row[5], item.found_mm, "cran trouvé", c);
            check_num(&row[6], item.gap_mm, "écart de cran", c);
            check_num(&row[7], Some(n.tolerance_mm), "tolérance de cran", c);
            let status = match item.status {
                NotchMatch::Matched => "Apparié",
                NotchMatch::Shifted => "Décalé",
                NotchMatch::Missing => "Manquant",
            };
            assert_eq!(row[8], status);
        }
    }
    assert!(it.next().is_none());

    // --- raccords -------------------------------------------------------------------------------
    for (row, j) in s["raccord"].rows.iter().zip(&junctions) {
        assert_eq!(row[0], j.number.to_string());
        check_num(&row[3], j.angle_a_deg, "angle A", c);
        check_num(&row[4], j.angle_b_deg, "angle B", c);
        check_num(&row[5], j.sum_deg, "somme", c);
        check_num(&row[6], j.deviation_deg, "écart d'angle", c);
        check_num(&row[7], Some(j.tolerance_deg), "tolérance d'angle", c);
    }

    // --- anomalies : comptes par type recalculés à part --------------------------------------
    let mut want: BTreeMap<&str, usize> = BTreeMap::new();
    let mut add = |k: &'static str, n: usize| {
        if n > 0 {
            *want.entry(k).or_default() += n;
        }
    };
    add(
        "Couture hors tolérance",
        seams
            .iter()
            .filter(|r| r.status == SeamStatus::OutOfTolerance)
            .count(),
    );
    add(
        "Couture non calculable",
        seams
            .iter()
            .filter(|r| matches!(r.status, SeamStatus::NotComputable { .. }))
            .count(),
    );
    for (n, r) in notch.iter().zip(&seams) {
        if n.not_computable.is_some() {
            if !matches!(r.status, SeamStatus::NotComputable { .. }) {
                add("Crans non calculables", 1);
            }
            continue;
        }
        add(
            "Cran sans vis-à-vis",
            n.items
                .iter()
                .filter(|i| i.status == NotchMatch::Missing)
                .count(),
        );
        add(
            "Cran décalé",
            n.items
                .iter()
                .filter(|i| i.status == NotchMatch::Shifted && i.side == SeamWhich::A)
                .count(),
        );
    }
    add(
        "Raccord cassé",
        junctions
            .iter()
            .filter(|j| j.status == JunctionStatus::Broken)
            .count(),
    );
    add(
        "Raccord non calculable",
        junctions
            .iter()
            .filter(|j| {
                matches!(
                    j.status,
                    JunctionStatus::NotComputable { reason, .. }
                        if !matches!(reason, JunctionIssue::Seam(_))
                )
            })
            .count(),
    );
    add(
        "Avertissement de pièce",
        p.pieces.iter().map(|q| p.piece_issues(q.id).len()).sum(),
    );
    let mut got: BTreeMap<String, usize> = BTreeMap::new();
    for r in &s["anomalie"].rows {
        *got.entry(r[0].clone()).or_default() += 1;
    }
    let want: BTreeMap<String, usize> = want.into_iter().map(|(k, v)| (k.to_string(), v)).collect();
    assert_eq!(got, want, "anomalies par type");
    c.anomaly_rows += s["anomalie"].rows.len();
    for (k, v) in &got {
        *c.kinds.entry(k.clone()).or_default() += v;
    }
    // Synthèse : le compteur total et « aucune anomalie » explicite.
    let total: usize = got.values().sum();
    let resume: BTreeMap<&str, &str> = s["resume"]
        .rows
        .iter()
        .map(|r| (r[0].as_str(), r[1].as_str()))
        .collect();
    assert_eq!(resume["Projet"], protect(&p.name), "nom du projet");
    assert_eq!(resume["Anomalies"], total.to_string());
    // Avertissements de pièce : la référence porte le nom de la pièce, texte inchangé.
    let piece_refs: Vec<String> = p
        .pieces
        .iter()
        .flat_map(|q| {
            p.piece_issues(q.id)
                .into_iter()
                .map(move |_| q.name.clone())
        })
        .map(|n| format!("Pièce « {n} »"))
        .collect();
    let mut got_refs: Vec<String> = s["anomalie"]
        .rows
        .iter()
        .filter(|r| r[0] == "Avertissement de pièce")
        .map(|r| r[1].clone())
        .collect();
    got_refs.sort();
    let mut want_refs = piece_refs;
    want_refs.sort();
    assert_eq!(
        got_refs, want_refs,
        "références des avertissements de pièce"
    );
    c.texts += got_refs.len();
    assert_eq!(
        resume["Synthèse"],
        if total == 0 {
            "Aucune anomalie".to_string()
        } else {
            format!("{total} anomalie(s)")
        }
    );
    // Tri : par type, puis par couple pour les anomalies de couple.
    let ranks: Vec<usize> = s["anomalie"]
        .rows
        .iter()
        .map(|r| type_rank(&r[0]))
        .collect();
    assert!(
        ranks.windows(2).all(|w| w[0] <= w[1]),
        "types non triés : {ranks:?}"
    );
    let couple = |r: &Vec<String>| -> usize {
        r[1].strip_prefix("Couture ")
            .and_then(|t| t.split(|c: char| !c.is_ascii_digit()).next())
            .and_then(|t| t.parse().ok())
            .unwrap_or(0)
    };
    for w in s["anomalie"].rows.windows(2) {
        if w[0][0] == w[1][0] && w[0][0] != "Avertissement de pièce" {
            assert!(couple(&w[0]) <= couple(&w[1]), "couples non triés");
        }
    }
}

fn check_pdf(p: &Project, bytes: &[u8], c: &mut Counters) {
    let font = repo_font();
    let pgs = pages(bytes);
    assert!(!pgs.is_empty());
    assert_eq!(media_boxes(bytes).len(), pgs.len());
    if pgs.len() > 1 {
        c.multi_page += 1;
    }
    let n = pgs.len();
    for (i, pg) in pgs.iter().enumerate() {
        for t in &pg.texts {
            let w = font.width_mm(&t.text, t.size_pt);
            assert!(
                t.x_mm >= 15.0 - 1e-3 && t.x_mm + w <= 195.0 + 1e-3,
                "{t:?} w={w}"
            );
            assert!(t.y_mm >= 10.0 && t.y_mm <= 285.0, "{t:?}");
            c.pdf_lines += 1;
        }
        assert!(
            pg.texts
                .iter()
                .any(|t| t.text.ends_with(&format!("page {}/{}", i + 1, n))),
            "pied de la page {}",
            i + 1
        );
    }
    let all: Vec<&str> = pgs
        .iter()
        .flat_map(|pg| pg.texts.iter().map(|t| t.text.as_str()))
        .collect();
    let has = |v: &str| all.contains(&v);
    // Chaque nombre recalculé d'une couture et d'un raccord se retrouve dans le PDF.
    for r in p.seam_pair_report() {
        for v in [
            r.length_a_mm,
            r.length_b_mm,
            r.gap_mm,
            r.embu_measured_mm,
            Some(r.embu_expected_mm),
            Some(r.tolerance_mm),
        ]
        .into_iter()
        .flatten()
        {
            assert!(
                has(&two(v)),
                "couture {} : {} absent du PDF",
                r.number,
                two(v)
            );
            c.numbers_pdf += 1;
        }
    }
    for j in p.seam_junction_report() {
        for v in [j.angle_a_deg, j.angle_b_deg, j.sum_deg, j.deviation_deg]
            .into_iter()
            .flatten()
        {
            assert!(
                has(&two(v)),
                "raccord {} : {} absent du PDF",
                j.number,
                two(v)
            );
            c.numbers_pdf += 1;
        }
    }
    for q in &p.pieces {
        if let Some(k) = p.piece_contour(q.id).ok().filter(|k| k.closed) {
            assert!(has(&two(k.area_mm2)), "aire {} absente", two(k.area_mm2));
            assert!(
                has(&two(k.perimeter_mm)),
                "périmètre {} absent",
                two(k.perimeter_mm)
            );
            c.numbers_pdf += 2;
        }
    }
}

#[test]
fn campagne_adverse_du_rapport_de_controle() {
    let mut c = Counters::default();
    for seed in 0..SEQUENCES {
        let p = random_project(seed);
        let before = persist::to_json(&p).expect("sérialisation");
        let snapshot = p.clone();
        c.projects += 1;

        let csv_a = export_control_report(&p, ReportFormat::Csv, Lang::Fr);
        let csv_b = export_control_report(&p, ReportFormat::Csv, Lang::Fr);
        assert_eq!(csv_a, csv_b, "CSV déterministe (graine {seed})");
        let s = sections(&parse(&csv_a.bytes));
        for code in ["resume", "anomalie", "piece", "couture", "crans", "raccord"] {
            assert!(s.contains_key(code), "section {code} (graine {seed})");
        }
        check_csv(&p, &s, &mut c);

        let pdf_a = export_control_report(&p, ReportFormat::Pdf, Lang::Fr);
        let pdf_b = export_control_report(&p, ReportFormat::Pdf, Lang::Fr);
        assert_eq!(pdf_a, pdf_b, "PDF déterministe (graine {seed})");
        check_pdf(&p, &pdf_a.bytes, &mut c);

        // Anglais : même structure, mêmes nombres (le point décimal ne dépend pas de la langue).
        let en = sections(&parse(
            &export_control_report(&p, ReportFormat::Csv, Lang::En).bytes,
        ));
        for code in ["piece", "couture", "crans", "raccord", "anomalie"] {
            assert_eq!(en[code].rows.len(), s[code].rows.len(), "{code} EN");
        }

        // Lecture seule : ni l'état du projet, ni sa sérialisation ne bougent.
        assert_eq!(p, snapshot, "projet modifié (graine {seed})");
        assert_eq!(
            persist::to_json(&p).unwrap(),
            before,
            "octets (graine {seed})"
        );
    }
    // Compteurs : les contrôles forts ont réellement comparé quelque chose.
    eprintln!(
        "campagne : {} projets, {} pièces, {} coutures, {} crans, {} raccords, {} anomalies, \
         {} nombres CSV, {} nombres PDF, {} lignes PDF, {} PDF multipages, types {:?}",
        c.projects,
        c.piece_rows,
        c.seam_rows,
        c.notch_rows,
        c.junction_rows,
        c.anomaly_rows,
        c.numbers_csv,
        c.numbers_pdf,
        c.pdf_lines,
        c.multi_page,
        c.kinds
    );
    assert_eq!(c.projects as u64, SEQUENCES);
    assert!(c.piece_rows >= 300, "pièces comparées : {}", c.piece_rows);
    assert!(c.seam_rows >= 150, "coutures comparées : {}", c.seam_rows);
    assert!(
        c.junction_rows >= 300,
        "raccords comparés : {}",
        c.junction_rows
    );
    assert!(c.notch_rows >= 100, "crans comparés : {}", c.notch_rows);
    assert!(
        c.anomaly_rows >= 200,
        "anomalies comparées : {}",
        c.anomaly_rows
    );
    assert!(c.numbers_csv >= 3000, "nombres CSV : {}", c.numbers_csv);
    assert!(c.texts >= 2000, "textes comparés : {}", c.texts);
    assert!(c.numbers_pdf >= 1500, "nombres PDF : {}", c.numbers_pdf);
    assert!(c.kinds.len() >= 4, "types d'anomalies vus : {:?}", c.kinds);
    assert!(c.kinds.contains_key("Avertissement de pièce"));
    assert!(c.kinds.contains_key("Couture non calculable"));
}
