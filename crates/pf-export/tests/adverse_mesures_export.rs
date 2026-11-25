//! US-09.8b — campagne adverse de l'export des mesures par taille : 160 projets aléatoires à graine
//! fixe (rectangle dont les trois coins B, C, D sont gradués au hasard dans chaque taille, gammes de
//! 1 à 8 tailles, base quelconque, libellés piégés `= + - @ ; "`).
//!
//! L'**oracle est indépendant** du cœur : les valeurs sont recalculées depuis « base + écart » avec
//! `hypot` (longueurs des quatre côtés, périmètre) et la formule du lacet (aire) ; le pas moyen, le
//! pourcentage et l'alerte (changement de sens, écart à la médiane des pas de plus de 50 %) sont
//! réécrits ici d'après la définition de `size_measures.rs`, jamais appelés. Le CSV français attendu
//! est construit ligne à ligne puis comparé **octet à octet** ; le PDF est relu par le lecteur
//! indépendant de `common/` (valeurs, symbole et texte d'alerte, blocs de tailles). Des compteurs
//! prouvent que les contrôles forts se sont exécutés.

mod common;

use common::csv_reader::parse;
use common::report_pdf::{pages, Page};
use pf_core::*;
use pf_export::report::ReportFormat;
use pf_export::size_measures_export::export_size_measures;
use pf_geometry::PointMm;
use pf_i18n::Lang;

const SEQUENCES: u64 = 160;

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    /// Entier dans `[lo ; hi]`.
    fn range(&mut self, lo: i64, hi: i64) -> i64 {
        lo + (self.next() % (hi - lo + 1) as u64) as i64
    }
    fn pick<'a>(&mut self, items: &[&'a str]) -> &'a str {
        items[self.next() as usize % items.len()]
    }
}

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

#[derive(Default)]
struct Counters {
    sequences: usize,
    csv_rows: usize,
    csv_numbers: usize,
    pdf_values: usize,
    pdf_means: usize,
    alerts_sign: usize,
    alerts_median: usize,
    pdf_alert_lines: usize,
    single_size: usize,
    special_labels: usize,
}

// ----- Oracle -----------------------------------------------------------------------------------

/// Sommets A, B, C, D d'une taille : A fixe, B, C, D = position de base + écart.
fn corners(delta: &[(i64, i64); 3]) -> [(f64, f64); 4] {
    [
        (0.0, 0.0),
        (400.0 + delta[0].0 as f64, delta[0].1 as f64),
        (400.0 + delta[1].0 as f64, 600.0 + delta[1].1 as f64),
        (delta[2].0 as f64, 600.0 + delta[2].1 as f64),
    ]
}

fn side(p: (f64, f64), q: (f64, f64)) -> f64 {
    (q.0 - p.0).hypot(q.1 - p.1)
}

/// Lignes attendues (hors libellé) : périmètre, aire, AB, BC, CD, DA ; une valeur par taille.
fn oracle_values(per_size: &[[(f64, f64); 4]]) -> Vec<Vec<f64>> {
    let mut rows: Vec<Vec<f64>> = vec![Vec::new(); 6];
    for c in per_size {
        let (ab, bc, cd, da) = (
            side(c[0], c[1]),
            side(c[1], c[2]),
            side(c[2], c[3]),
            side(c[3], c[0]),
        );
        // Lacet.
        let mut twice = 0.0;
        for i in 0..4 {
            let (p, q) = (c[i], c[(i + 1) % 4]);
            twice += p.0 * q.1 - q.0 * p.1;
        }
        rows[0].push(ab + bc + cd + da);
        rows[1].push(twice.abs() / 2.0);
        rows[2].push(ab);
        rows[3].push(bc);
        rows[4].push(cd);
        rows[5].push(da);
    }
    rows
}

struct Stats {
    mean: Option<f64>,
    percent: Option<f64>,
    /// Pas signalés et motif (`true` = changement de sens).
    alert: Option<(Vec<usize>, bool)>,
}

fn stats(v: &[f64]) -> Stats {
    let steps: Vec<f64> = v.windows(2).map(|w| w[1] - w[0]).collect();
    if steps.is_empty() {
        return Stats {
            mean: None,
            percent: None,
            alert: None,
        };
    }
    let mean = steps.iter().sum::<f64>() / steps.len() as f64;
    let pcs: Vec<f64> = v
        .windows(2)
        .map(|w| (w[1] - w[0]) / w[0].abs() * 100.0)
        .collect();
    let percent = pcs.iter().sum::<f64>() / pcs.len() as f64;
    let mut alert = None;
    if v.len() >= 3 {
        let mut flagged = vec![false; steps.len()];
        let mut sign = false;
        for i in 0..steps.len() - 1 {
            let (a, b) = (steps[i], steps[i + 1]);
            if (a > 1e-6 && b < -1e-6) || (a < -1e-6 && b > 1e-6) {
                sign = true;
                flagged[i] = true;
                flagged[i + 1] = true;
            }
        }
        let mut sorted = steps.clone();
        sorted.sort_by(f64::total_cmp);
        let n = sorted.len();
        let m = if n % 2 == 1 {
            sorted[n / 2]
        } else {
            (sorted[n / 2 - 1] + sorted[n / 2]) / 2.0
        };
        let limit = (0.5 * m.abs()).max(1e-6);
        let mut off = false;
        for (i, s) in steps.iter().enumerate() {
            if (s - m).abs() > limit {
                off = true;
                flagged[i] = true;
            }
        }
        if sign || off {
            let idx = (0..n).filter(|i| flagged[*i]).collect();
            alert = Some((idx, sign));
        }
    }
    Stats {
        mean: Some(mean),
        percent: Some(percent),
        alert,
    }
}

fn fmt2(v: f64) -> String {
    let s = format!("{v:.2}");
    if s == "-0.00" {
        "0.00".into()
    } else {
        s
    }
}

/// Champ texte du CSV : apostrophe devant `= + - @ TAB CR`, guillemets si `; " CR LF`.
fn protect(s: &str) -> String {
    let mut f = String::new();
    if s.starts_with(['=', '+', '-', '@', '\t', '\r']) {
        f.push('\'');
    }
    f.push_str(s);
    if f.contains([';', '"', '\r', '\n']) {
        format!("\"{}\"", f.replace('"', "\"\""))
    } else {
        f
    }
}

fn alert_text_fr(a: &(Vec<usize>, bool), labels: &[String]) -> String {
    let steps =
        a.0.iter()
            .map(|i| format!("{}-{}", labels[*i], labels[*i + 1]))
            .collect::<Vec<_>>()
            .join(", ");
    if a.1 {
        format!("Irrégulier : changement de sens ({steps})")
    } else {
        format!("Irrégulier : pas à plus de 50 % de la médiane ({steps})")
    }
}

// ----- Projet -----------------------------------------------------------------------------------

struct Case {
    project: Project,
    range: SizeRangeId,
    name: String,
    range_name: String,
    piece: String,
    labels: Vec<String>,
    base: usize,
    corners: Vec<[(f64, f64); 4]>,
}

fn build(seed: u64) -> Case {
    let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0xD1B5_4A32_D192_ED03);
    for _ in 0..4 {
        rng.next();
    }
    let n = rng.range(1, 8) as usize;
    let base = rng.range(0, n as i64 - 1) as usize;
    let prefix = ["", "", "=", "+", "-", "@", "a;", "q\""];
    let labels: Vec<String> = (0..n)
        .map(|i| format!("{}{}", rng.pick(&prefix), 30 + 2 * i))
        .collect();
    let name = rng
        .pick(&["Patron", "=Patron", "a;b", "x\"y", "=1+1;2", "-T"])
        .to_string();
    let range_name = rng.pick(&["Femme", "@Femme", "a;b", "G\"1"]).to_string();
    let piece = format!("P{}", rng.range(1, 99));
    let mut project = Project::new(&name);
    let sheet = project.sheets[0].id;
    project
        .apply(Command::AddSizeRange {
            name: range_name.clone(),
            kind: SizeKind::Alphanumeric,
            labels: labels.clone(),
            base,
        })
        .unwrap();
    let range = project.sizes[0].id;
    let mut ents = Vec::new();
    for (a, b) in [
        ((0.0, 0.0), (400.0, 0.0)),
        ((400.0, 0.0), (400.0, 600.0)),
        ((400.0, 600.0), (0.0, 600.0)),
        ((0.0, 600.0), (0.0, 0.0)),
    ] {
        ents.push(
            project
                .apply(Command::AddSegment {
                    sheet,
                    from: PointRef::New(pt(a.0, a.1)),
                    to: PointRef::New(pt(b.0, b.1)),
                })
                .unwrap()
                .entities[0],
        );
    }
    project
        .apply(Command::ExtractPiece {
            sheet,
            source: PieceSource::Paths { entities: ents },
            name: piece.clone(),
            detached: false,
        })
        .unwrap();
    let find = |p: &Project, at: PointMm| {
        let s = p.sheet(sheet).unwrap();
        s.points
            .iter()
            .find(|q| pf_geometry::distance_mm(s.position(q.id).unwrap(), at) < 1e-9)
            .unwrap()
            .id
    };
    let pts = [
        find(&project, pt(400.0, 0.0)),
        find(&project, pt(400.0, 600.0)),
        find(&project, pt(0.0, 600.0)),
    ];
    let mut per_size = Vec::new();
    for i in 0..n {
        let mut d = [(0i64, 0i64); 3];
        if i != base {
            for slot in &mut d {
                *slot = (rng.range(-40, 40), rng.range(-40, 40));
            }
            let size = project.sizes[0].sizes[i].id;
            for (k, p) in pts.iter().enumerate() {
                project
                    .apply(Command::SetPointGrading {
                        sheet,
                        point: *p,
                        size,
                        dx_mm: d[k].0 as f64,
                        dy_mm: d[k].1 as f64,
                    })
                    .unwrap();
            }
        }
        per_size.push(corners(&d));
    }
    Case {
        project,
        range,
        name,
        range_name,
        piece,
        labels,
        base,
        corners: per_size,
    }
}

/// CSV français attendu, construit depuis l'oracle.
fn expected_csv(c: &Case, counters: &mut Counters) -> String {
    let values = oracle_values(&c.corners);
    let names = [
        format!("Périmètre · {}", c.piece),
        format!("Aire (mm²) · {}", c.piece),
        "Segment AB".to_string(),
        "Segment BC".to_string(),
        "Segment CD".to_string(),
        "Segment DA".to_string(),
    ];
    let all_stats: Vec<Stats> = values.iter().map(|v| stats(v)).collect();
    let alerts = all_stats.iter().filter(|s| s.alert.is_some()).count();
    let mut lines: Vec<String> = vec![
        "\u{FEFF}#resume;Élément;Valeur".into(),
        format!("resume;Projet;{}", protect(&c.name)),
        format!("resume;Gamme;{}", protect(&c.range_name)),
        format!("resume;Taille de base;{}", protect(&c.labels[c.base])),
        "resume;Mesures;6".into(),
        format!("resume;Évolutions irrégulières;{alerts}"),
        "resume;Unités;Longueurs en mm, aires en mm². Pas moyen : moyenne des écarts entre tailles consécutives.".into(),
        "resume;Précision;\"2 décimales ; cellule vide : non calculable dans cette taille\"".into(),
        "resume;Avertissement;Mesures observées, lecture seule, non validées par une modéliste".into(),
    ];
    let mut head = vec!["#mesure".to_string(), "Mesure".into(), "Unité".into()];
    head.extend(c.labels.iter().map(|l| protect(l)));
    head.extend(["Pas moyen".into(), "Pas moyen (%)".into(), "Alerte".into()]);
    lines.push(head.join(";"));
    for (i, v) in values.iter().enumerate() {
        let st = &all_stats[i];
        let mut cells = vec![
            "mesure".to_string(),
            protect(&names[i]),
            if i == 1 { "mm²" } else { "mm" }.to_string(),
        ];
        cells.extend(v.iter().map(|x| fmt2(*x)));
        cells.push(st.mean.map(fmt2).unwrap_or_default());
        cells.push(st.percent.map(fmt2).unwrap_or_default());
        cells.push(
            st.alert
                .as_ref()
                .map(|a| {
                    if a.1 {
                        counters.alerts_sign += 1;
                    } else {
                        counters.alerts_median += 1;
                    }
                    protect(&alert_text_fr(a, &c.labels))
                })
                .unwrap_or_default(),
        );
        counters.csv_numbers += v.len() + 2;
        counters.csv_rows += 1;
        lines.push(cells.join(";"));
    }
    lines.push(String::new());
    lines.join("\r\n")
}

// ----- Lecture du PDF ---------------------------------------------------------------------------

fn number_of(token: &str) -> Option<f64> {
    token.parse::<f64>().ok()
}

fn check_pdf(c: &Case, bytes: &[u8], counters: &mut Counters) {
    let pg: Vec<Page> = pages(bytes);
    assert!(!pg.is_empty());
    let all: Vec<(usize, &common::report_pdf::TextOp)> = pg
        .iter()
        .enumerate()
        .flat_map(|(i, p)| p.texts.iter().map(move |t| (i, t)))
        .collect();
    for (_, t) in &all {
        assert!(t.x_mm >= 14.99 && t.x_mm <= 195.0, "hors zone : {t:?}");
        assert!(t.y_mm >= 0.0 && t.y_mm <= 297.0, "hors page : {t:?}");
    }
    // Chaque libellé de taille est écrit.
    for l in &c.labels {
        assert!(all.iter().any(|(_, t)| &t.text == l), "taille {l:?}");
    }
    let values = oracle_values(&c.corners);
    let names = [
        format!("Périmètre · {}", c.piece),
        format!("Aire (mm²) · {}", c.piece),
        "Segment AB".to_string(),
        "Segment BC".to_string(),
        "Segment CD".to_string(),
        "Segment DA".to_string(),
    ];
    let blocks = all.iter().filter(|(_, t)| t.text == names[0]).count();
    assert!(blocks >= 1);
    let mut flagged_rows = 0;
    for (r, name) in names.iter().enumerate() {
        let occurrences: Vec<_> = all.iter().filter(|(_, t)| &t.text == name).collect();
        assert_eq!(occurrences.len(), blocks, "{name}");
        let mut read: Vec<String> = Vec::new();
        let mut means: Vec<String> = Vec::new();
        for (page, label) in occurrences {
            let mut line: Vec<_> = all
                .iter()
                .filter(|(p, t)| p == page && (t.y_mm - label.y_mm).abs() < 1e-6)
                .map(|(_, t)| t)
                .collect();
            line.sort_by(|a, b| a.x_mm.total_cmp(&b.x_mm));
            let toks: Vec<&str> = line.iter().map(|t| t.text.as_str()).collect();
            assert_eq!(toks[0], name.as_str());
            assert_eq!(toks[1], if r == 1 { "mm²" } else { "mm" });
            // Reste : valeurs du bloc, puis pas moyen et pourcentage.
            let rest = &toks[2..];
            read.extend(rest[..rest.len() - 2].iter().map(|s| s.to_string()));
            means.push(rest[rest.len() - 2].to_string());
            means.push(rest[rest.len() - 1].to_string());
        }
        assert_eq!(read.len(), c.labels.len(), "{name} : {read:?}");
        let tol = if r == 1 { 0.5001 } else { 0.0501 };
        for (i, tok) in read.iter().enumerate() {
            let got = number_of(tok).unwrap_or_else(|| panic!("{name} : {tok:?}"));
            assert!(
                (got - values[r][i]).abs() <= tol,
                "{name} taille {i} : {got} ≠ {}",
                values[r][i]
            );
            counters.pdf_values += 1;
        }
        let st = stats(&values[r]);
        for pair in means.chunks(2) {
            match st.mean {
                Some(m) => {
                    let got = number_of(&pair[0]).expect("pas moyen");
                    assert!((got - m).abs() <= 0.0051, "{name} pas moyen {got} ≠ {m}");
                    let gp = number_of(&pair[1]).expect("pourcentage");
                    assert!((gp - st.percent.unwrap()).abs() <= 0.0051, "{name} %");
                    counters.pdf_means += 1;
                }
                None => assert_eq!(pair, ["—", "—"], "{name}"),
            }
        }
        if st.alert.is_some() {
            flagged_rows += 1;
        }
        if let Some(a) = &st.alert {
            let want = format!("(!) {}", alert_text_fr(a, &c.labels));
            // Une ligne d'alerte par bloc et par ligne de même texte, symbole et texte ensemble.
            let same = values
                .iter()
                .filter(|v| {
                    stats(v).alert.is_some_and(|b| {
                        alert_text_fr(&b, &c.labels) == alert_text_fr(a, &c.labels)
                    })
                })
                .count();
            let count = all.iter().filter(|(_, t)| t.text == want).count();
            assert_eq!(count, blocks * same, "alerte {name} : {want:?}");
            counters.pdf_alert_lines += 1;
        }
    }
    // Aucune ligne d'alerte en trop.
    let marks = all
        .iter()
        .filter(|(_, t)| t.text.starts_with("(!)"))
        .count();
    assert_eq!(marks, flagged_rows * blocks, "lignes d'alerte en trop");
}

#[test]
fn campagne_adverse_export_des_mesures_par_taille() {
    let mut counters = Counters::default();
    for seed in 1..=SEQUENCES {
        let c = build(seed);
        let before = pf_core::persist::to_json(&c.project).expect("projet valide");
        if c.labels.len() == 1 {
            counters.single_size += 1;
        }
        counters.special_labels += c
            .labels
            .iter()
            .filter(|l| l.starts_with(['=', '+', '-', '@', 'a', 'q']))
            .count();

        // CSV : octets attendus.
        let csv = export_size_measures(&c.project, c.range, ReportFormat::Csv, Lang::Fr)
            .unwrap_or_else(|e| panic!("graine {seed} : {e}"));
        let expected = expected_csv(&c, &mut counters);
        assert_eq!(
            String::from_utf8(csv.bytes.clone()).unwrap(),
            expected,
            "graine {seed}"
        );
        // Le CSV est du CSV bien formé (lecteur indépendant) : 1 + 9 lignes de synthèse, 7 de tableau.
        let rows = parse(&csv.bytes);
        assert_eq!(rows.len(), 8 + 1 + 6 + 1, "graine {seed} : {rows:?}");

        // PDF : valeurs, alertes, blocs.
        let pdf = export_size_measures(&c.project, c.range, ReportFormat::Pdf, Lang::Fr).unwrap();
        check_pdf(&c, &pdf.bytes, &mut counters);

        // Déterminisme et projet inchangé.
        let again = export_size_measures(&c.project, c.range, ReportFormat::Pdf, Lang::Fr).unwrap();
        assert_eq!(
            pdf.bytes, again.bytes,
            "graine {seed} : PDF non déterministe"
        );
        assert_eq!(
            pf_core::persist::to_json(&c.project).unwrap(),
            before,
            "graine {seed} : projet modifié"
        );
        counters.sequences += 1;
    }
    // Les contrôles forts se sont exécutés.
    assert_eq!(counters.sequences, SEQUENCES as usize);
    assert!(counters.csv_rows >= 6 * 150, "{}", counters.csv_rows);
    assert!(counters.csv_numbers > 3000, "{}", counters.csv_numbers);
    assert!(counters.pdf_values > 3000, "{}", counters.pdf_values);
    assert!(counters.pdf_means > 500, "{}", counters.pdf_means);
    assert!(
        counters.alerts_sign > 5,
        "alertes de sens : {}",
        counters.alerts_sign
    );
    assert!(
        counters.alerts_median > 5,
        "alertes de médiane : {}",
        counters.alerts_median
    );
    assert!(counters.pdf_alert_lines >= counters.alerts_sign + counters.alerts_median);
    assert!(counters.single_size >= 5, "{}", counters.single_size);
    assert!(counters.special_labels > 50, "{}", counters.special_labels);
}
