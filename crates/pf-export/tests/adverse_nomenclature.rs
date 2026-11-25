//! US-11.2 — campagne adverse de la nomenclature : 160 projets aléatoires à graine fixe (1 à 6
//! pièces rectangulaires, triangulaires ou « papillons » au contour qui se recoupe, gammes de 1 à 4
//! tailles ou aucune, matières à casse et espaces variables, quantités, paires miroir, libellés
//! piégés `=`).
//!
//! L'**oracle est indépendant** du cœur : l'aire de chaque taille est recalculée par la formule du
//! lacet sur les sommets « base + écart » ; le regroupement par matière (espaces de bord retirés,
//! casse ignorée, première graphie conservée, non renseignée en dernier), les découpes (quantité,
//! doublée pour une paire) et les surfaces nettes sont réécrits ici d'après la règle documentée de
//! `bom.rs`, jamais appelés. Le CSV est relu par le lecteur indépendant de `common/` et comparé
//! champ à champ ; le PDF est relu par le lecteur indépendant de `nomenc_support` (valeurs et
//! nombre de cellules illisibles). Des compteurs prouvent que les contrôles forts se sont exécutés.

mod common;
mod nomenc_support;

use common::csv_reader::parse;
use nomenc_support::*;
use pf_core::*;
use pf_export::bom_export::export_bom;
use pf_export::report::ReportFormat;
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
    fn range(&mut self, lo: i64, hi: i64) -> i64 {
        lo + (self.next() % (hi - lo + 1) as u64) as i64
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Rect,
    Tri,
    Bowtie,
}

struct Spec {
    kind: Kind,
    w: f64,
    h: f64,
    quantity: u32,
    pair: bool,
    material: Option<&'static str>,
    /// Écart du sommet libre dans chaque taille (0 pour la base).
    delta: Vec<(f64, f64)>,
}

#[derive(Default)]
struct Counters {
    sequences: usize,
    with_range: usize,
    without_range: usize,
    unreadable_pieces: usize,
    unreadable_totals: usize,
    merged_spellings: usize,
    unset_groups: usize,
    pairs: usize,
    cells: usize,
    protected: usize,
    pdf_dashes: usize,
}

// ----- Oracle -----------------------------------------------------------------------------------

fn shoelace(v: &[(f64, f64)]) -> f64 {
    let mut twice = 0.0;
    for i in 0..v.len() {
        let (p, q) = (v[i], v[(i + 1) % v.len()]);
        twice += p.0 * q.1 - q.0 * p.1;
    }
    twice.abs() / 2.0
}

fn vertices(s: &Spec, size: usize) -> Vec<(f64, f64)> {
    let (dx, dy) = s.delta[size];
    match s.kind {
        Kind::Rect => vec![(0.0, 0.0), (s.w, 0.0), (s.w + dx, s.h + dy), (0.0, s.h)],
        Kind::Tri => vec![(0.0, 0.0), (s.w, 0.0), (dx, s.h + dy)],
        Kind::Bowtie => unreachable!(),
    }
}

fn expected_area(s: &Spec, size: usize) -> Option<f64> {
    (s.kind != Kind::Bowtie).then(|| shoelace(&vertices(s, size)))
}

/// Matière normalisée : (clé de regroupement, graphie affichée).
fn norm(m: Option<&str>) -> Option<(String, String)> {
    let t = m?.trim();
    (!t.is_empty()).then(|| (t.to_lowercase(), t.to_string()))
}

struct Group {
    shown: Option<String>,
    pieces: usize,
    cuts: u32,
    net: Vec<Option<f64>>,
    spellings: Vec<String>,
}

fn oracle_groups(specs: &[Spec], columns: usize) -> Vec<Group> {
    let mut groups: Vec<(Option<String>, Group)> = Vec::new();
    for s in specs {
        let n = norm(s.material);
        let key = n.as_ref().map(|k| k.0.clone());
        let at = groups.iter().position(|g| g.0 == key).unwrap_or_else(|| {
            groups.push((
                key,
                Group {
                    shown: n.as_ref().map(|k| k.1.clone()),
                    pieces: 0,
                    cuts: 0,
                    net: vec![Some(0.0); columns],
                    spellings: Vec::new(),
                },
            ));
            groups.len() - 1
        });
        let g = &mut groups[at].1;
        let cuts = s.quantity * if s.pair { 2 } else { 1 };
        g.pieces += 1;
        g.cuts += cuts;
        if let Some(k) = &n {
            if !g.spellings.contains(&k.1) {
                g.spellings.push(k.1.clone());
            }
        }
        for c in 0..columns {
            g.net[c] = match (g.net[c], expected_area(s, c)) {
                (Some(t), Some(a)) => Some(t + a * f64::from(cuts)),
                _ => None,
            };
        }
    }
    let mut out: Vec<Group> = groups.into_iter().map(|g| g.1).collect();
    out.sort_by_key(|g| g.shown.is_none());
    out
}

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

fn csv_area(v: Option<f64>) -> String {
    v.map_or(String::new(), |v| format!("{v:.2}"))
}

fn pdf_area(v: Option<f64>) -> String {
    v.map_or("—".to_string(), |v| format!("{v:.0}"))
}

// ----- Projet -----------------------------------------------------------------------------------

const MATERIALS: [Option<&str>; 9] = [
    Some("Cuir"),
    Some("cuir"),
    Some("  CUIR "),
    Some("Doublure"),
    Some("=Toile"),
    Some("toile"),
    Some("   "),
    None,
    Some("Peau; fine"),
];

fn build(seed: u64) -> (Project, Option<SizeRangeId>, Vec<Spec>, usize) {
    let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0xA076_1D64_78BD_642F);
    for _ in 0..4 {
        rng.next();
    }
    let n_pieces = rng.range(1, 6) as usize;
    let sizes = if rng.range(0, 4) == 0 {
        0
    } else {
        rng.range(1, 4) as usize
    };
    let base = if sizes > 0 {
        rng.range(0, sizes as i64 - 1) as usize
    } else {
        0
    };
    let columns = sizes.max(1);
    let mut project = Project::new("Adverse");
    let mut specs = Vec::new();
    let mut ids = Vec::new();
    for k in 0..n_pieces {
        let kind = match rng.range(0, 9) {
            0 => Kind::Bowtie,
            1..=3 => Kind::Tri,
            _ => Kind::Rect,
        };
        let (w, h) = (
            rng.range(10, 100) as f64 * 5.0,
            rng.range(10, 100) as f64 * 5.0,
        );
        let ox = 1000.0 * k as f64;
        let shift =
            |v: &[(f64, f64)]| -> Vec<(f64, f64)> { v.iter().map(|p| (p.0 + ox, p.1)).collect() };
        let pts = match kind {
            Kind::Rect => shift(&[(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)]),
            Kind::Tri => shift(&[(0.0, 0.0), (w, 0.0), (0.0, h)]),
            Kind::Bowtie => shift(&[(0.0, 0.0), (w, h), (w, 0.0), (0.0, h)]),
        };
        ids.push(poly_piece(&mut project, &pts, &format!("P{k}")));
        specs.push(Spec {
            kind,
            w,
            h,
            quantity: rng.range(1, 5) as u32,
            pair: rng.range(0, 2) == 0,
            material: MATERIALS[rng.next() as usize % MATERIALS.len()],
            delta: vec![(0.0, 0.0); columns],
        });
    }
    for (id, s) in ids.iter().zip(&specs) {
        card(&mut project, *id, s.quantity, s.pair, s.material, None);
    }
    let range = (sizes > 0).then(|| {
        let labels: Vec<String> = (0..sizes).map(|i| format!("{}", 36 + 2 * i)).collect();
        let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
        add_range(&mut project, &refs, base)
    });
    if let Some(r) = range {
        for (k, s) in specs.iter_mut().enumerate() {
            if s.kind == Kind::Bowtie {
                continue;
            }
            for size in 0..sizes {
                if size == base {
                    continue;
                }
                let d = (rng.range(-40, 40) as f64, rng.range(-40, 40) as f64);
                // Le coin libre : (w, h) du rectangle, (0, h) du triangle, décalé de 1000 par pièce.
                let at = match s.kind {
                    Kind::Rect => (s.w + 1000.0 * k as f64, s.h),
                    _ => (1000.0 * k as f64, s.h),
                };
                grade(&mut project, r, size, at, d);
                s.delta[size] = d;
            }
        }
    }
    (project, range, specs, columns)
}

#[test]
fn campagne_adverse_nomenclature() {
    let mut c = Counters::default();
    for seed in 0..SEQUENCES {
        let (project, range, specs, columns) = build(seed);
        let before = format!("{project:?}");
        c.sequences += 1;
        if range.is_some() {
            c.with_range += 1;
        } else {
            c.without_range += 1;
        }
        let groups = oracle_groups(&specs, columns);

        // (a) le cœur
        let bom = project.bill_of_materials(range).unwrap();
        assert_eq!(bom.lines.len(), specs.len(), "seed {seed}");
        for (l, s) in bom.lines.iter().zip(&specs) {
            assert_eq!(
                l.cuts,
                s.quantity * if s.pair { 2 } else { 1 },
                "seed {seed}"
            );
            for col in 0..columns {
                match (l.areas_mm2[col], expected_area(s, col)) {
                    (None, None) => {}
                    (Some(a), Some(b)) => assert!((a - b).abs() < 1e-6, "seed {seed}: {a} / {b}"),
                    (a, b) => panic!("seed {seed}: aire {a:?} attendue {b:?}"),
                }
            }
        }
        assert_eq!(bom.materials.len(), groups.len(), "seed {seed}");
        for (m, g) in bom.materials.iter().zip(&groups) {
            assert_eq!(m.material, g.shown, "seed {seed}");
            assert_eq!((m.pieces, m.cuts), (g.pieces, g.cuts), "seed {seed}");
            for col in 0..columns {
                match (m.net_area_mm2[col], g.net[col]) {
                    (None, None) => c.unreadable_totals += 1,
                    (Some(a), Some(b)) => assert!((a - b).abs() < 1e-6, "seed {seed}: {a} / {b}"),
                    (a, b) => panic!("seed {seed}: total {a:?} attendu {b:?}"),
                }
            }
            if g.spellings.len() > 1 {
                c.merged_spellings += 1;
            }
            if g.shown.is_none() {
                c.unset_groups += 1;
            }
        }
        c.unreadable_pieces += specs.iter().filter(|s| s.kind == Kind::Bowtie).count();
        c.pairs += specs.iter().filter(|s| s.pair).count();

        // (b) CSV champ à champ
        let csv = parse(
            &export_bom(&project, range, ReportFormat::Csv, Lang::Fr)
                .unwrap()
                .bytes,
        );
        let sheet = &project.sheets[0].name;
        let piece_rows: Vec<&Vec<String>> = csv.iter().filter(|r| r[0] == "piece").collect();
        assert_eq!(piece_rows.len(), specs.len(), "seed {seed}");
        for (r, (i, s)) in piece_rows.iter().zip(specs.iter().enumerate()) {
            let mut want: Vec<String> = vec![
                format!("P{i}"),
                String::new(),
                sheet.clone(),
                norm(s.material).map_or("non renseignée".to_string(), |k| k.1),
                String::new(),
                s.quantity.to_string(),
                if s.pair { "oui".into() } else { String::new() },
                (s.quantity * if s.pair { 2 } else { 1 }).to_string(),
            ];
            want.extend((0..columns).map(|col| csv_area(expected_area(s, col))));
            let got: Vec<String> = r[1..].to_vec();
            let want_p: Vec<String> = want.iter().map(|f| unprotect(&protect(f))).collect();
            assert_eq!(got, want_p, "seed {seed}");
            c.cells += got.len();
            if want[3].starts_with('=') {
                assert_eq!(got[3], format!("'{}", want[3]), "seed {seed}");
                c.protected += 1;
            }
        }
        let mat_rows: Vec<&Vec<String>> = csv.iter().filter(|r| r[0] == "matiere").collect();
        assert_eq!(mat_rows.len(), groups.len(), "seed {seed}");
        for (r, g) in mat_rows.iter().zip(&groups) {
            let shown = g.shown.clone().unwrap_or_else(|| "non renseignée".into());
            let mut want = vec![shown, g.pieces.to_string(), g.cuts.to_string()];
            want.extend(g.net.iter().map(|v| csv_area(*v)));
            let want_p: Vec<String> = want.iter().map(|f| unprotect(&protect(f))).collect();
            assert_eq!(r[1..].to_vec(), want_p, "seed {seed}");
            c.cells += want.len();
        }

        // (c) PDF : valeurs et cellules illisibles
        let pdf = export_bom(&project, range, ReportFormat::Pdf, Lang::Fr).unwrap();
        let texts: Vec<String> = fiche_pages(&pdf.bytes)
            .into_iter()
            .flat_map(|p| p.texts)
            .collect();
        for g in &groups {
            for v in &g.net {
                let w = pdf_area(*v);
                assert!(texts.contains(&w), "seed {seed}: « {w} » absent du PDF");
            }
        }
        for s in &specs {
            for col in 0..columns {
                let w = pdf_area(expected_area(s, col));
                assert!(texts.contains(&w), "seed {seed}: « {w} » absent du PDF");
            }
        }
        let want_dashes = specs
            .iter()
            .map(|s| {
                (0..columns)
                    .filter(|c| expected_area(s, *c).is_none())
                    .count()
            })
            .sum::<usize>()
            + groups
                .iter()
                .map(|g| g.net.iter().filter(|v| v.is_none()).count())
                .sum::<usize>();
        let got_dashes = texts.iter().filter(|t| *t == "—").count();
        assert_eq!(got_dashes, want_dashes, "seed {seed}");
        c.pdf_dashes += got_dashes;
        assert!(!texts.iter().any(|t| t.contains('?')), "seed {seed}");

        // (d) lecture seule
        assert_eq!(before, format!("{project:?}"), "seed {seed}");
    }
    assert_eq!(c.sequences, 160);
    assert!(
        c.with_range >= 100 && c.without_range >= 15,
        "{} / {}",
        c.with_range,
        c.without_range
    );
    assert!(c.unreadable_pieces >= 20 && c.unreadable_totals >= 15);
    assert!(c.merged_spellings >= 30, "{}", c.merged_spellings);
    assert!(c.unset_groups >= 30 && c.pairs >= 100);
    assert!(
        c.protected >= 5 && c.pdf_dashes >= 30 && c.cells >= 2000,
        "{} {} {}",
        c.protected,
        c.pdf_dashes,
        c.cells
    );
}

/// Les champs sont comparés après lecture : la valeur lue est la valeur protégée puis dépouillée
/// des guillemets de CSV ; l'apostrophe de protection reste (elle fait partie du fichier).
fn unprotect(field: &str) -> String {
    if let Some(inner) = field.strip_prefix('"').and_then(|f| f.strip_suffix('"')) {
        inner.replace("\"\"", "\"")
    } else {
        field.to_string()
    }
}
