//! Campagne adverse US-10.1 / US-10.2 : 220 séquences à graine fixe de 28 pas (commandes valides,
//! invalides, fantômes, suppression de taille / gamme, import CSV généré et corrompu).
//!
//! L'oracle est un modèle de données écrit ici (cartes triées, règles de validité réécrites) :
//! il ne lit ni le code des commandes ni celui du CSV. Il prédit accepté / refusé, l'état complet
//! après chaque pas, les erreurs CSV (ligne, colonne), les lignes importées, les mesures
//! manquantes et la taille la plus proche (recalculées par force brute). Un refus laisse les octets
//! du projet identiques. Les compteurs prouvent que chaque contrôle fort s'est exécuté.
//!
//! Seule exception documentée : l'acceptation de `RemoveSize` / `RemoveSizeRange` (règles de
//! tailles d'US-09.1) n'est pas prédite ; l'effet sur le barème, lui, l'est.

use pf_core::persist;
use pf_core::*;
use std::collections::{BTreeMap, BTreeSet};

struct R(u64);
impl R {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        ((self.next() >> 11) as usize) % n
    }
    fn chance(&mut self, pct: usize) -> bool {
        self.below(100) < pct
    }
}

#[derive(Clone, Debug, PartialEq)]
struct OM {
    id: u64,
    name: String,
    deg: bool,
    vals: BTreeMap<u64, f64>,
}

#[derive(Clone, Debug, PartialEq)]
struct OC {
    id: u64,
    name: String,
    vals: BTreeMap<u64, f64>,
}

#[derive(Clone, Debug)]
struct Oracle {
    ms: Vec<OM>,
    cs: Vec<OC>,
    /// (id de gamme, tailles (id, libellé)).
    ranges: Vec<(u64, Vec<(u64, String)>)>,
    ever: BTreeSet<u64>,
}

#[derive(Default, Debug)]
struct Counters {
    accepted: usize,
    refused: usize,
    adds: usize,
    renames: usize,
    cascades_client: usize,
    cascades_size: usize,
    sets: usize,
    client_sets: usize,
    bad_names: usize,
    bad_values: usize,
    csv_ok: usize,
    csv_err: usize,
    csv_header_err: usize,
    csv_field_count_err: usize,
    csv_rows: usize,
    csv_cells_cleared: usize,
    csv_blank: usize,
    exports_checked: usize,
    proposals: usize,
    proposals_some: usize,
    ties: usize,
    incomparable: usize,
    refusal_bytes_checked: usize,
}

fn trimmed(n: &str) -> Option<String> {
    let t = n.trim();
    (!t.is_empty()).then(|| t.to_string())
}

fn good(v: f64) -> bool {
    v.is_finite() && (0.0..=1_000_000.0).contains(&v)
}

fn lower(s: &str) -> String {
    s.to_lowercase()
}

const NAMES: [&str; 12] = [
    "Poitrine", "poitrine", "POITRINE", "Taille", "Hanche", "Dos", "Épaule", "  Cou  ", "", "   ",
    "Bras", "Jambe",
];
const VALUES: [f64; 11] = [
    0.0,
    1.0,
    12.5,
    880.0,
    1_000_000.0,
    1_000_000.5,
    -1.0,
    f64::NAN,
    f64::INFINITY,
    0.001,
    399.999,
];

fn new_project() -> (Project, Oracle) {
    let mut p = Project::new("Adverse bareme");
    p.apply(Command::AddSizeRange {
        name: "Femme".into(),
        kind: SizeKind::Alphanumeric,
        labels: vec!["S".into(), "M".into(), "L".into()],
        base: 1,
    })
    .unwrap();
    p.apply(Command::AddSizeRange {
        name: "Homme".into(),
        kind: SizeKind::Numeric,
        labels: vec!["46".into(), "48".into()],
        base: 0,
    })
    .unwrap();
    let ranges = p
        .sizes
        .iter()
        .map(|r| {
            (
                r.id.0,
                r.sizes.iter().map(|s| (s.id.0, s.label.clone())).collect(),
            )
        })
        .collect();
    let mut ever = BTreeSet::new();
    for r in &p.sizes {
        ever.insert(r.id.0);
        ever.extend(r.sizes.iter().map(|s| s.id.0));
    }
    (
        p,
        Oracle {
            ms: vec![],
            cs: vec![],
            ranges,
            ever,
        },
    )
}

impl Oracle {
    fn sizes(&self) -> Vec<u64> {
        self.ranges
            .iter()
            .flat_map(|(_, s)| s.iter().map(|x| x.0))
            .collect()
    }
    fn dup_m(&self, name: &str, except: Option<u64>) -> bool {
        self.ms
            .iter()
            .any(|m| Some(m.id) != except && lower(&m.name) == lower(name))
    }
    fn dup_c(&self, name: &str, except: Option<u64>) -> bool {
        self.cs
            .iter()
            .any(|c| Some(c.id) != except && lower(&c.name) == lower(name))
    }
    fn drop_sizes(&mut self, gone: &[u64]) -> usize {
        let mut n = 0;
        for m in &mut self.ms {
            for g in gone {
                if m.vals.remove(g).is_some() {
                    n += 1;
                }
            }
        }
        n
    }
}

fn state_matches(p: &Project, o: &Oracle) {
    assert_eq!(p.measurements.len(), o.ms.len(), "nombre de mesures");
    for (pm, om) in p.measurements.iter().zip(&o.ms) {
        assert_eq!(pm.id.0, om.id);
        assert_eq!(pm.name, om.name);
        assert_eq!(
            pm.unit == MeasurementUnit::Deg,
            om.deg,
            "unité de {}",
            om.name
        );
        let got: BTreeMap<u64, f64> = pm.values.iter().map(|v| (v.size.0, v.value)).collect();
        assert_eq!(got.len(), pm.values.len(), "pas de taille en double");
        assert_eq!(got, om.vals, "valeurs de {}", om.name);
    }
    assert_eq!(p.clients.len(), o.cs.len(), "nombre de fiches");
    for (pc, oc) in p.clients.iter().zip(&o.cs) {
        assert_eq!(pc.id.0, oc.id);
        assert_eq!(pc.name, oc.name);
        let got: BTreeMap<u64, f64> = pc
            .values
            .iter()
            .map(|v| (v.measurement.0, v.value))
            .collect();
        assert_eq!(got.len(), pc.values.len());
        assert_eq!(got, oc.vals, "valeurs de la fiche {}", oc.name);
    }
    // Sauvegarde et relecture à l'identique.
    let text = persist::to_json(p).expect("valide");
    assert_eq!(&persist::from_json(&text).expect("relu"), p);
}

/// Mesures manquantes et taille la plus proche, recalculées à la main (force brute).
fn check_readings(p: &Project, o: &Oracle, r: &mut R, c: &mut Counters) {
    if o.cs.is_empty() || o.ranges.is_empty() {
        return;
    }
    let oc = &o.cs[r.below(o.cs.len())];
    let (rid, sizes) = &o.ranges[r.below(o.ranges.len())];
    let missing: Vec<u64> =
        o.ms.iter()
            .filter(|m| !oc.vals.contains_key(&m.id))
            .map(|m| m.id)
            .collect();
    let got: Vec<u64> = p
        .client_missing_measurements(ClientId(oc.id))
        .unwrap()
        .iter()
        .map(|m| m.0)
        .collect();
    assert_eq!(got, missing, "mesures manquantes");
    let prop = p
        .client_size_proposal(ClientId(oc.id), SizeRangeId(*rid))
        .unwrap();
    assert_eq!(
        prop.missing.iter().map(|m| m.0).collect::<Vec<_>>(),
        missing
    );
    c.proposals += 1;
    let mut scores: Vec<Option<f64>> = Vec::new();
    for (sid, _) in sizes {
        let mut score = 0.0;
        let mut mm = 0;
        let mut gone = false;
        let mut gaps = Vec::new();
        for m in &o.ms {
            let Some(cv) = oc.vals.get(&m.id) else {
                continue;
            };
            match m.vals.get(sid) {
                Some(sv) => {
                    gaps.push((m.id, cv - sv));
                    if !m.deg {
                        score += (cv - sv).abs();
                        mm += 1;
                    }
                }
                None => gone = true,
            }
        }
        let cand = prop.candidates.iter().find(|x| x.size.0 == *sid).unwrap();
        let got_gaps: Vec<(u64, f64)> =
            cand.gaps.iter().map(|g| (g.measurement.0, g.gap)).collect();
        assert_eq!(got_gaps.len(), gaps.len());
        for (a, b) in got_gaps.iter().zip(&gaps) {
            assert_eq!(a.0, b.0);
            assert!((a.1 - b.1).abs() < 1e-9, "écart {a:?} / {b:?}");
        }
        assert_eq!(cand.missing_in_size.is_empty(), !gone);
        let s = (!gone && mm > 0).then_some(score);
        match (cand.score_mm, s) {
            (Some(a), Some(b)) => assert!((a - b).abs() < 1e-9, "score {a} / {b}"),
            (None, None) => {
                if gone {
                    c.incomparable += 1;
                }
            }
            other => panic!("comparabilité différente : {other:?}"),
        }
        scores.push(s);
    }
    let best = scores
        .iter()
        .flatten()
        .copied()
        .fold(f64::INFINITY, f64::min);
    let want_nearest = scores
        .iter()
        .position(|s| s.is_some_and(|x| x == best))
        .map(|i| sizes[i].0);
    assert_eq!(
        prop.nearest.map(|s| s.0),
        want_nearest,
        "taille la plus proche"
    );
    let want_tied: Vec<u64> = scores
        .iter()
        .enumerate()
        .filter(|(i, s)| {
            Some(sizes[*i].0) != want_nearest && s.is_some_and(|x| (x - best).abs() <= 1e-9)
        })
        .map(|(i, _)| sizes[i].0)
        .collect();
    assert_eq!(prop.tied.iter().map(|s| s.0).collect::<Vec<_>>(), want_tied);
    if want_nearest.is_some() {
        c.proposals_some += 1;
    }
    if !want_tied.is_empty() {
        c.ties += 1;
    }
}

// ---------- lecteur CSV indépendant pour contrôler l'export ----------

fn split_csv(text: &str, sep: char) -> Vec<Vec<String>> {
    let mut out = Vec::new();
    for line in text.split("\r\n").filter(|l| !l.is_empty()) {
        let mut rec = vec![String::new()];
        let mut q = false;
        let cs: Vec<char> = line.chars().collect();
        let mut i = 0;
        while i < cs.len() {
            let ch = cs[i];
            let last = rec.last_mut().unwrap();
            if q && ch == '"' && cs.get(i + 1) == Some(&'"') {
                last.push('"');
                i += 1;
            } else if ch == '"' {
                q = !q;
            } else if ch == sep && !q {
                rec.push(String::new());
            } else {
                last.push(ch);
            }
            i += 1;
        }
        out.push(rec);
    }
    out
}

fn decode_export(bytes: &[u8], enc: CsvEncoding) -> String {
    match enc {
        CsvEncoding::Windows1252 => bytes.iter().map(|b| *b as char).collect(),
        CsvEncoding::Utf8 => String::from_utf8(bytes.to_vec()).unwrap(),
        CsvEncoding::Utf8Bom => {
            assert_eq!(&bytes[..3], &[0xEF, 0xBB, 0xBF]);
            String::from_utf8(bytes[3..].to_vec()).unwrap()
        }
    }
}

fn pick_format(r: &mut R) -> CsvFormat {
    let separator = [';', ',', '\t'][r.below(3)];
    let decimal = if separator == ',' {
        '.'
    } else {
        ['.', ','][r.below(2)]
    };
    let encoding = [
        CsvEncoding::Utf8,
        CsvEncoding::Utf8Bom,
        CsvEncoding::Windows1252,
    ][r.below(3)];
    CsvFormat {
        separator,
        decimal,
        encoding,
    }
}

fn check_export(p: &Project, o: &Oracle, r: &mut R, c: &mut Counters) {
    if o.ranges.is_empty() {
        return;
    }
    let (rid, sizes) = &o.ranges[r.below(o.ranges.len())];
    let fmt = pick_format(r);
    let out = export_measurement_csv(p, SizeRangeId(*rid), &fmt).expect("export");
    let rows = split_csv(&decode_export(&out.bytes, fmt.encoding), fmt.separator);
    assert_eq!(rows.len(), o.ms.len() + 1);
    let mut head = vec!["mesure".to_string(), "unite".to_string()];
    head.extend(sizes.iter().map(|s| s.1.clone()));
    assert_eq!(rows[0], head);
    let mut omitted = 0;
    for (row, m) in rows[1..].iter().zip(&o.ms) {
        assert_eq!(row[0], m.name);
        assert_eq!(row[1], if m.deg { "deg" } else { "mm" });
        for (k, (sid, _)) in sizes.iter().enumerate() {
            let cell = &row[2 + k];
            match m.vals.get(sid) {
                Some(v) => {
                    let back: f64 = cell.replace(fmt.decimal, ".").parse().expect("nombre");
                    assert_eq!(back, *v, "aller-retour exact de {} / {sid}", m.name);
                }
                None => assert!(cell.is_empty()),
            }
        }
        omitted += m
            .vals
            .keys()
            .filter(|k| !sizes.iter().any(|s| s.0 == **k))
            .count();
    }
    assert_eq!(out.omitted_values, omitted);
    c.exports_checked += 1;
}

// ---------- pas de CSV généré ----------

fn encode(text: &str, enc: CsvEncoding) -> Vec<u8> {
    match enc {
        CsvEncoding::Utf8 => text.as_bytes().to_vec(),
        CsvEncoding::Utf8Bom => {
            let mut v = vec![0xEF, 0xBB, 0xBF];
            v.extend_from_slice(text.as_bytes());
            v
        }
        CsvEncoding::Windows1252 => text
            .chars()
            .map(|ch| match ch {
                'É' => 0xC9,
                c if c.is_ascii() => c as u8,
                other => panic!("caractère inattendu {other}"),
            })
            .collect(),
    }
}

const CSV_NAMES: [&str; 11] = [
    "Poitrine",
    "poitrine",
    "Taille",
    "Hanche",
    "Dos",
    "Épaule",
    "  Cou  ",
    "",
    "Bras; gauche",
    "Jambe \"x\"",
    "Cuisse",
];

fn quoted(s: &str, sep: char) -> String {
    if s.contains(sep) || s.contains('"') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

type ExpRow = (String, bool, Vec<(u64, Option<f64>)>);

fn csv_step(p: &mut Project, o: &mut Oracle, r: &mut R, c: &mut Counters) {
    if o.ranges.is_empty() {
        return;
    }
    let (rid, sizes) = o.ranges[r.below(o.ranges.len())].clone();
    let fmt = pick_format(r);
    let (sep, dec) = (fmt.separator, fmt.decimal);
    let other = if dec == '.' { ',' } else { '.' };
    let eol = if r.chance(50) { "\n" } else { "\r\n" };
    let mut errors: Vec<(usize, Option<usize>)> = Vec::new();
    // Mode propre : aucune corruption (le fichier doit être lu et importé).
    let clean = r.chance(55);
    // Colonnes : sous-ensemble ordonné au hasard, parfois une étiquette inconnue ou répétée.
    let mut cols: Vec<usize> = (0..sizes.len()).filter(|_| r.chance(75)).collect();
    if cols.len() > 1 && r.chance(50) {
        let last = cols.len() - 1;
        cols.swap(0, last);
    }
    let mut head = vec!["mesure".to_string(), "unite".to_string()];
    let mut col_ids: Vec<u64> = Vec::new();
    for (k, i) in cols.iter().enumerate() {
        head.push(quoted(&sizes[*i].1, sep));
        col_ids.push(sizes[*i].0);
        let _ = k;
    }
    if !clean && r.chance(8) {
        head.push("XL".into());
        errors.push((1, Some(head.len())));
        c.csv_header_err += 1;
    } else if !clean && !cols.is_empty() && r.chance(6) {
        head.push(quoted(&sizes[cols[0]].1, sep));
        errors.push((1, Some(head.len())));
        c.csv_header_err += 1;
    }
    let header_bad = !errors.is_empty();
    let mut text = format!("{}{eol}", head.join(&sep.to_string()));
    let mut line = 1usize;
    let mut blanks = 0usize;
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut expected: Vec<ExpRow> = Vec::new();
    let n_rows = if clean { 1 + r.below(6) } else { r.below(7) };
    for _ in 0..n_rows {
        if r.chance(10) {
            text.push_str(eol);
            line += 1;
            blanks += 1;
        }
        line += 1;
        let mut name = CSV_NAMES[r.below(CSV_NAMES.len())];
        let mut unit_text = ["mm", "deg", "MM", "cm", ""][r.below(5)];
        if clean {
            let mut tries = 0;
            while (trimmed(name).is_none_or(|n| seen.contains(&lower(&n)))) && tries < 50 {
                name = CSV_NAMES[r.below(CSV_NAMES.len())];
                tries += 1;
            }
            unit_text = match o.ms.iter().find(|m| lower(&m.name) == lower(name.trim())) {
                Some(m) if m.deg => "deg",
                Some(_) => "MM",
                None => ["mm", "deg"][r.below(2)],
            };
        }
        let mut fields = vec![quoted(name, sep), unit_text.to_string()];
        let mut cells: Vec<(u64, Option<f64>)> = Vec::new();
        let mut cell_errs: Vec<usize> = Vec::new();
        for (k, id) in col_ids.iter().enumerate() {
            let col = 3 + k;
            let kind = if clean { r.below(7) } else { r.below(10) };
            let (txt, val, ok): (String, Option<f64>, bool) = match kind {
                0..=4 => {
                    let v = [0.0, 12.5, 880.0, 920.0, 900.0, 7.125][r.below(6)];
                    let s = format!("{v}").replace('.', &dec.to_string());
                    (s, Some(v), true)
                }
                5 | 6 if clean || kind == 5 => (String::new(), None, true),
                6 => ("abc".into(), None, false),
                7 => ("-3".into(), None, false),
                8 => {
                    let t = if other == sep {
                        "1x5".to_string()
                    } else {
                        format!("12{other}5")
                    };
                    (t, None, false)
                }
                _ => (
                    [" 1 000", "1e3", "2000000"][r.below(3)].to_string(),
                    None,
                    false,
                ),
            };
            if !ok {
                cell_errs.push(col);
            }
            fields.push(txt);
            cells.push((*id, val));
        }
        let short = !clean && r.chance(8);
        if short {
            fields.pop();
        }
        text.push_str(&fields.join(&sep.to_string()));
        text.push_str(eol);
        if short {
            errors.push((line, None));
            c.csv_field_count_err += 1;
            continue;
        }
        let mut row_ok = true;
        match trimmed(name) {
            None => {
                errors.push((line, Some(1)));
                row_ok = false;
            }
            Some(n) if !seen.insert(lower(&n)) => {
                errors.push((line, Some(1)));
                row_ok = false;
            }
            Some(_) => {}
        }
        let unit = match unit_text.to_lowercase().as_str() {
            "mm" => Some(false),
            "deg" => Some(true),
            _ => None,
        };
        match unit {
            None => {
                errors.push((line, Some(2)));
                row_ok = false;
            }
            Some(deg) => {
                if let Some(m) = o.ms.iter().find(|m| lower(&m.name) == lower(name.trim())) {
                    if m.deg != deg {
                        errors.push((line, Some(2)));
                        row_ok = false;
                    }
                }
            }
        }
        for col in &cell_errs {
            errors.push((line, Some(*col)));
            row_ok = false;
        }
        if row_ok {
            expected.push((name.trim().to_string(), unit == Some(true), cells));
        }
    }
    if n_rows == 0 && errors.is_empty() {
        errors.push((0, None));
    }
    if !header_bad && errors.is_empty() && expected.is_empty() {
        errors.push((0, None));
    }
    let bytes = encode(&text, fmt.encoding);
    let before = persist::to_json(p).unwrap();
    let res = read_measurement_csv(p, SizeRangeId(rid), &bytes, &fmt);
    assert_eq!(
        persist::to_json(p).unwrap(),
        before,
        "la lecture ne modifie rien"
    );
    if header_bad {
        // Seules les erreurs d'en-tête sont rapportées (les lignes ne sont pas interprétables).
        let got: Vec<(usize, Option<usize>)> = res
            .expect_err("en-tête invalide")
            .iter()
            .map(|e| (e.line, e.column))
            .collect();
        assert_eq!(
            got,
            errors
                .iter()
                .filter(|e| e.0 == 1)
                .copied()
                .collect::<Vec<_>>()
        );
        c.csv_err += 1;
        return;
    }
    if !errors.is_empty() {
        let got: Vec<(usize, Option<usize>)> = res
            .expect_err("erreurs attendues")
            .iter()
            .map(|e| (e.line, e.column))
            .collect();
        assert_eq!(got, errors, "erreurs de lecture\n{text}");
        c.csv_err += 1;
        return;
    }
    let read = res.unwrap_or_else(|e| panic!("lecture attendue valide : {e:?}\n{text}"));
    assert_eq!(read.blank_lines, blanks);
    c.csv_blank += blanks;
    assert_eq!(read.rows.len(), expected.len());
    for (row, (name, deg, cells)) in read.rows.iter().zip(&expected) {
        assert_eq!(&row.name, name);
        assert_eq!(row.unit == MeasurementUnit::Deg, *deg);
        let got: Vec<(u64, Option<f64>)> = row.cells.iter().map(|x| (x.size.0, x.value)).collect();
        assert_eq!(&got, cells);
    }
    c.csv_ok += 1;
    let clear = r.chance(50);
    let out = p
        .apply(Command::ImportMeasurements {
            rows: read.rows,
            clear_empty: clear,
        })
        .unwrap_or_else(|e| panic!("import valide refusé : {e}"));
    let mut created = out.measurements.iter();
    for (name, deg, cells) in &expected {
        let idx = match o.ms.iter().position(|m| lower(&m.name) == lower(name)) {
            Some(i) => i,
            None => {
                let id = created.next().expect("id créé").0;
                assert!(o.ever.insert(id), "id jamais réutilisé");
                o.ms.push(OM {
                    id,
                    name: name.clone(),
                    deg: *deg,
                    vals: BTreeMap::new(),
                });
                o.ms.len() - 1
            }
        };
        for (sid, v) in cells {
            match v {
                Some(v) => {
                    o.ms[idx].vals.insert(*sid, *v);
                }
                None if clear && o.ms[idx].vals.remove(sid).is_some() => {
                    c.csv_cells_cleared += 1;
                }
                None => {}
            }
        }
        c.csv_rows += 1;
    }
    assert!(created.next().is_none(), "aucun id en trop");
}

// ---------- pas de commande ----------

/// Copie du projet sans aucune valeur de barème pour ces tailles (par `ClearMeasurementValue`).
fn bareme_free(p: &Project, o: &Oracle, sizes: &[u64]) -> Project {
    let mut q = p.clone();
    for m in &o.ms {
        for s in sizes {
            if m.vals.contains_key(s) {
                q.apply(Command::ClearMeasurementValue {
                    measurement: MeasurementId(m.id),
                    size: SizeId(*s),
                })
                .unwrap();
            }
        }
    }
    q
}

fn ghost(r: &mut R) -> u64 {
    999_000 + r.below(50) as u64
}

type Update = Box<dyn FnOnce(&mut Oracle, &Outcome, &mut Counters)>;

fn step(p: &mut Project, o: &mut Oracle, r: &mut R, c: &mut Counters) {
    let op = r.below(30);
    let before = persist::to_json(p).unwrap();
    // (commande, accepté d'après l'oracle, mise à jour de l'oracle si accepté)
    let mut after: Option<Update>;
    let cmd: Command;
    let expect: bool;
    let pick_m = |r: &mut R, o: &Oracle| -> u64 {
        if o.ms.is_empty() || r.chance(10) {
            ghost(r)
        } else {
            o.ms[r.below(o.ms.len())].id
        }
    };
    let pick_c = |r: &mut R, o: &Oracle| -> u64 {
        if o.cs.is_empty() || r.chance(10) {
            ghost(r)
        } else {
            o.cs[r.below(o.cs.len())].id
        }
    };
    let pick_s = |r: &mut R, o: &Oracle| -> u64 {
        let all = o.sizes();
        if all.is_empty() || r.chance(8) {
            ghost(r)
        } else {
            all[r.below(all.len())]
        }
    };
    match op {
        0..=3 => {
            let name = NAMES[r.below(NAMES.len())].to_string();
            let deg = r.chance(30);
            expect = trimmed(&name).is_some_and(|n| !o.dup_m(&n, None));
            if trimmed(&name).is_none() {
                c.bad_names += 1;
            }
            cmd = Command::AddMeasurement {
                name: name.clone(),
                unit: if deg {
                    MeasurementUnit::Deg
                } else {
                    MeasurementUnit::Mm
                },
            };
            after = Some(Box::new(move |o, out, c| {
                let id = out.measurements[0].0;
                assert_eq!(out.measurements.len(), 1);
                assert!(o.ever.insert(id), "id jamais réutilisé");
                o.ms.push(OM {
                    id,
                    name: name.trim().to_string(),
                    deg,
                    vals: BTreeMap::new(),
                });
                c.adds += 1;
            }));
        }
        4..=5 => {
            let id = pick_m(r, o);
            let name = NAMES[r.below(NAMES.len())].to_string();
            expect = o.ms.iter().any(|m| m.id == id)
                && trimmed(&name).is_some_and(|n| !o.dup_m(&n, Some(id)));
            cmd = Command::RenameMeasurement {
                measurement: MeasurementId(id),
                name: name.clone(),
            };
            after = Some(Box::new(move |o, _, c| {
                o.ms.iter_mut().find(|m| m.id == id).unwrap().name = name.trim().to_string();
                c.renames += 1;
            }));
        }
        6..=7 => {
            let id = pick_m(r, o);
            expect = o.ms.iter().any(|m| m.id == id);
            cmd = Command::RemoveMeasurement {
                measurement: MeasurementId(id),
            };
            after = Some(Box::new(move |o, _, c| {
                o.ms.retain(|m| m.id != id);
                for cl in &mut o.cs {
                    if cl.vals.remove(&id).is_some() {
                        c.cascades_client += 1;
                    }
                }
            }));
        }
        8..=12 => {
            let (m, s) = (pick_m(r, o), pick_s(r, o));
            let v = if r.chance(40) {
                VALUES[r.below(VALUES.len())]
            } else {
                [880.0, 920.0, 880.0, 920.0, 400.0, 0.0][r.below(6)]
            };
            expect = o.ms.iter().any(|x| x.id == m) && o.sizes().contains(&s) && good(v);
            if !good(v) {
                c.bad_values += 1;
            }
            cmd = Command::SetMeasurementValue {
                measurement: MeasurementId(m),
                size: SizeId(s),
                value: v,
            };
            after = Some(Box::new(move |o, _, c| {
                o.ms.iter_mut()
                    .find(|x| x.id == m)
                    .unwrap()
                    .vals
                    .insert(s, v);
                c.sets += 1;
            }));
        }
        13..=14 => {
            let (m, s) = (pick_m(r, o), pick_s(r, o));
            expect = o.ms.iter().any(|x| x.id == m) && o.sizes().contains(&s);
            cmd = Command::ClearMeasurementValue {
                measurement: MeasurementId(m),
                size: SizeId(s),
            };
            after = Some(Box::new(move |o, _, _| {
                o.ms.iter_mut().find(|x| x.id == m).unwrap().vals.remove(&s);
            }));
        }
        15..=16 => {
            let name = NAMES[r.below(NAMES.len())].to_string();
            expect = trimmed(&name).is_some_and(|n| !o.dup_c(&n, None));
            cmd = Command::AddClient { name: name.clone() };
            after = Some(Box::new(move |o, out, _| {
                let id = out.clients[0].0;
                assert!(o.ever.insert(id), "id jamais réutilisé");
                o.cs.push(OC {
                    id,
                    name: name.trim().to_string(),
                    vals: BTreeMap::new(),
                });
            }));
        }
        17 => {
            let id = pick_c(r, o);
            let name = NAMES[r.below(NAMES.len())].to_string();
            expect = o.cs.iter().any(|x| x.id == id)
                && trimmed(&name).is_some_and(|n| !o.dup_c(&n, Some(id)));
            cmd = Command::RenameClient {
                client: ClientId(id),
                name: name.clone(),
            };
            after = Some(Box::new(move |o, _, _| {
                o.cs.iter_mut().find(|x| x.id == id).unwrap().name = name.trim().to_string();
            }));
        }
        18 => {
            let id = pick_c(r, o);
            expect = o.cs.iter().any(|x| x.id == id);
            cmd = Command::RemoveClient {
                client: ClientId(id),
            };
            after = Some(Box::new(move |o, _, _| o.cs.retain(|x| x.id != id)));
        }
        19..=23 => {
            let (cl, m) = (pick_c(r, o), pick_m(r, o));
            let v = if r.chance(40) {
                VALUES[r.below(VALUES.len())]
            } else {
                [900.0, 900.0, 910.0, 880.0, 400.0][r.below(5)]
            };
            expect = o.cs.iter().any(|x| x.id == cl) && o.ms.iter().any(|x| x.id == m) && good(v);
            if !good(v) {
                c.bad_values += 1;
            }
            cmd = Command::SetClientValue {
                client: ClientId(cl),
                measurement: MeasurementId(m),
                value: v,
            };
            after = Some(Box::new(move |o, _, c| {
                o.cs.iter_mut()
                    .find(|x| x.id == cl)
                    .unwrap()
                    .vals
                    .insert(m, v);
                c.client_sets += 1;
            }));
        }
        24 => {
            let (cl, m) = (pick_c(r, o), pick_m(r, o));
            expect = o.cs.iter().any(|x| x.id == cl) && o.ms.iter().any(|x| x.id == m);
            cmd = Command::ClearClientValue {
                client: ClientId(cl),
                measurement: MeasurementId(m),
            };
            after = Some(Box::new(move |o, _, _| {
                o.cs.iter_mut()
                    .find(|x| x.id == cl)
                    .unwrap()
                    .vals
                    .remove(&m);
            }));
        }
        25 => {
            // Suppression de taille : l'acceptation vient du cœur (règles d'US-09.1), l'effet sur
            // le barème est contrôlé.
            let all = o.sizes();
            if all.is_empty() {
                return;
            }
            let s = all[r.below(all.len())];
            let mut q = p.clone();
            let ok = q.apply(Command::RemoveSize { size: SizeId(s) }).is_ok();
            if ok {
                let out = p.apply(Command::RemoveSize { size: SizeId(s) }).unwrap();
                let n = o.drop_sizes(&[s]);
                for (_, sizes) in &mut o.ranges {
                    sizes.retain(|x| x.0 != s);
                }
                c.accepted += 1;
                c.cascades_size += n;
                assert_eq!(
                    out.notes.iter().any(|t| t.contains("valeur(s) du barème")),
                    n > 0,
                    "remarque de retrait"
                );
            } else {
                assert!(p.apply(Command::RemoveSize { size: SizeId(s) }).is_err());
                assert_eq!(persist::to_json(p).unwrap(), before);
                // Un refus ne doit jamais venir du barème : sans ses valeurs, même verdict.
                let mut free = bareme_free(p, o, &[s]);
                assert!(
                    free.apply(Command::RemoveSize { size: SizeId(s) }).is_err(),
                    "RemoveSize refusé à cause des valeurs du barème"
                );
                c.refused += 1;
            }
            return;
        }
        26 if r.chance(30) => {
            if o.ranges.is_empty() {
                return;
            }
            let (rid, sizes) = o.ranges[r.below(o.ranges.len())].clone();
            let res = p.apply(Command::RemoveSizeRange {
                range: SizeRangeId(rid),
            });
            if res.is_ok() {
                let ids: Vec<u64> = sizes.iter().map(|s| s.0).collect();
                c.cascades_size += o.drop_sizes(&ids);
                o.ranges.retain(|x| x.0 != rid);
                c.accepted += 1;
            } else {
                assert_eq!(persist::to_json(p).unwrap(), before);
                let ids: Vec<u64> = sizes.iter().map(|s| s.0).collect();
                let mut free = bareme_free(p, o, &ids);
                assert!(
                    free.apply(Command::RemoveSizeRange {
                        range: SizeRangeId(rid)
                    })
                    .is_err(),
                    "RemoveSizeRange refusé à cause des valeurs du barème"
                );
                c.refused += 1;
            }
            return;
        }
        _ => {
            csv_step(p, o, r, c);
            return;
        }
    }
    let res = p.apply(cmd);
    match (res, expect) {
        (Ok(out), true) => {
            c.accepted += 1;
            (after.take().expect("mise à jour"))(o, &out, c);
        }
        (Err(e), false) => {
            c.refused += 1;
            assert!(!e.to_string().is_empty());
            assert_eq!(persist::to_json(p).unwrap(), before, "refus sans trace");
            c.refusal_bytes_checked += 1;
        }
        (Ok(_), false) => panic!("accepté à tort (op {op})"),
        (Err(e), true) => panic!("refusé à tort (op {op}) : {e}"),
    }
}

#[test]
fn campagne_bareme_et_fiches_client() {
    let mut c = Counters::default();
    let mut seqs = 0;
    for seed in 1..=220u64 {
        let mut r = R(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let (mut p, mut o) = new_project();
        for i in 0..28 {
            step(&mut p, &mut o, &mut r, &mut c);
            state_matches(&p, &o);
            check_readings(&p, &o, &mut r, &mut c);
            if i % 5 == 4 {
                check_export(&p, &o, &mut r, &mut c);
            }
        }
        seqs += 1;
    }
    eprintln!("{c:#?}");
    assert_eq!(seqs, 220);
    assert!(c.accepted > 1500 && c.refused > 600, "{c:?}");
    assert!(
        c.adds > 200 && c.renames > 40 && c.sets > 300 && c.client_sets > 150,
        "{c:?}"
    );
    assert!(c.cascades_client > 10 && c.cascades_size > 100, "{c:?}");
    assert!(
        c.bad_names > 50 && c.bad_values > 100 && c.refusal_bytes_checked > 600,
        "{c:?}"
    );
    assert!(c.csv_ok > 300 && c.csv_err > 250, "{c:?}");
    assert!(c.csv_header_err > 20 && c.csv_field_count_err > 20, "{c:?}");
    assert!(
        c.csv_rows > 300 && c.csv_cells_cleared > 10 && c.csv_blank > 10,
        "{c:?}"
    );
    assert!(c.exports_checked > 800, "{c:?}");
    assert!(c.proposals > 2000 && c.proposals_some > 150, "{c:?}");
    assert!(c.ties > 5 && c.incomparable > 100, "{c:?}");
}
