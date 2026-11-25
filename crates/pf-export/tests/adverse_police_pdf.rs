//! US-06.6 — campagne adverse de la police embarquée du PDF : 180 séquences à graine fixe.
//!
//! Chaque séquence tire une étiquette (1 à 5 lignes), 0 à 3 cotations et un nom de point dans un
//! alphabet de caractères **pris en charge** (ASCII, Latin-1, Latin étendu A, ponctuation, grec,
//! `€ ™ −`) et **hors jeu** (cyrillique, CJK, émoji hors plan de base, `≤ ≥ → ∞`, contrôles,
//! `U+0000`). Le PDF est relu par des lecteurs **indépendants du code d'export** (`common/
//! pdf_text.rs` : table `xref`, `ToUnicode`, `CIDToGIDMap`, `W`, police intégrée, police du dépôt
//! lue sur disque) :
//!
//! - chaque chaîne décodée par le `ToUnicode` du fichier égale la chaîne écrite, caractère à
//!   caractère (un caractère hors jeu devient « ? ») ;
//! - le décodage par `CIDToGIDMap` + `cmap` de la police du dépôt donne le même glyphe ;
//! - `W` égale les largeurs d'avance de la police ; chaque glyphe écrit est présent, entier, dans
//!   le sous-ensemble intégré ;
//! - toutes les lignes de l'étiquette et les cotations (centrées sur la même abscisse) ont le
//!   même centre, calculé avec les **vraies** largeurs d'avance (jamais une estimation) ;
//! - deux exports identiques donnent les mêmes octets (une séquence sur quatre).
//!
//! Des compteurs prouvent que chaque contrôle fort a réellement été exécuté. Honnêteté : aucune
//! ouverture dans un lecteur PDF tiers dans ce fichier (voir le journal pour le contrôle MuPDF).

mod common;

use common::pdf_text::{read_font, repo_font, to_unicode, Ttf};
use common::Rng;
use pf_core::PathRole;
use pf_export::pdf::export_pdf;
use pf_export::{PathGeom, Scene, SceneDimension, ScenePath, ScenePieceLabel, ScenePoint};
use pf_geometry::PointMm;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

const SEQUENCES: u64 = 180;
const _: () = assert!(SEQUENCES >= 150);

static TEXTS: AtomicUsize = AtomicUsize::new(0);
static CHARS_UNICODE: AtomicUsize = AtomicUsize::new(0);
static CHARS_GID: AtomicUsize = AtomicUsize::new(0);
static REPLACED: AtomicUsize = AtomicUsize::new(0);
static ACCENTED: AtomicUsize = AtomicUsize::new(0);
static EMPTY: AtomicUsize = AtomicUsize::new(0);
static LONG: AtomicUsize = AtomicUsize::new(0);
static WIDTHS: AtomicUsize = AtomicUsize::new(0);
static GLYPHS: AtomicUsize = AtomicUsize::new(0);
static CENTERS: AtomicUsize = AtomicUsize::new(0);
static REPEATS: AtomicUsize = AtomicUsize::new(0);
static EXPECTED_CHARS: AtomicUsize = AtomicUsize::new(0);

macro_rules! ck {
    ($seq:expr, $cond:expr, $($arg:tt)+) => {
        if !$cond {
            panic!("séquence {} : {}", $seq, format!($($arg)+));
        }
    };
}

fn near(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

// ---------------------------------------------------------------------------
// Alphabet
// ---------------------------------------------------------------------------

const LATIN1: &str =
    "ÀÁÂÃÄÅÆÇÈÉÊËÌÍÎÏÑÒÓÔÕÖ×ØÙÚÛÜÝßàáâãäåæçèéêëìíîïñòóôõö÷øùúûüýÿ°²³±µ¶·¼½¾«»¡¿\u{a0}";
const EXT_A: &str = "ĀāĂăĄąĆćČčĎďĐđĒēĘęĚěĞğĪīİıŁłŃńŇňŌōŐőŒœŘřŚśŞşŠšŤťŪūŮůŰűŸŹźŻżŽž";
const PUNCT: &str = "—–…‘’“”„•†‡‰′″‹›€™−";
const GREEK: &str = "ΔθπΣΩαβγδλμφω";
/// Hors police : cyrillique, CJK, hangul, émoji (hors plan de base), symboles absents de Noto
/// Sans, contrôles et `U+0000`.
const ABSENT: &str = "ЖжЯ日本語한😀🙂≤≥≠∞→←∈\u{200B}\t\n\r\u{0}\u{7f}\u{1}\u{1F600}";

fn ascii() -> String {
    (0x20u8..=0x7E).map(char::from).collect()
}

fn random_char(rng: &mut Rng, ascii: &[char], pools: &[Vec<char>]) -> char {
    match rng.below(100) {
        0..=44 => ascii[rng.below(ascii.len() as u64) as usize],
        45..=84 => {
            let p = &pools[rng.below(pools.len() as u64 - 1) as usize];
            p[rng.below(p.len() as u64) as usize]
        }
        _ => {
            let p = &pools[pools.len() - 1];
            p[rng.below(p.len() as u64) as usize]
        }
    }
}

fn random_text(rng: &mut Rng, ascii: &[char], pools: &[Vec<char>], font: &Ttf) -> String {
    let len = match rng.below(24) {
        0 => 0,
        1 => 150 + rng.below(100),
        _ => 1 + rng.below(30),
    };
    if len == 0 {
        EMPTY.fetch_add(1, Relaxed);
    }
    if len >= 150 {
        LONG.fetch_add(1, Relaxed);
    }
    let _ = font;
    (0..len).map(|_| random_char(rng, ascii, pools)).collect()
}

/// Chaîne affichée attendue : un caractère absent de la police devient « ? ».
fn expected(text: &str, font: &Ttf) -> String {
    text.chars()
        .map(|c| {
            if font.advance_of(c).is_some() {
                c
            } else {
                REPLACED.fetch_add(1, Relaxed);
                '?'
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Scène et lecture
// ---------------------------------------------------------------------------

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

fn scene(label: &[String], dims: &[String], point: &str) -> Scene {
    let c = [
        pt(0.0, 0.0),
        pt(200.0, 0.0),
        pt(200.0, 150.0),
        pt(0.0, 150.0),
    ];
    Scene {
        project_name: "Adverse".into(),
        sheet_name: "Pièce".into(),
        paths: (0..4)
            .map(|i| ScenePath {
                geom: PathGeom::Line(c[i], c[(i + 1) % 4]),
                role: PathRole::CuttingContour,
                sense: pf_core::GrainSense::Unmarked,
            })
            .collect(),
        points: vec![ScenePoint {
            pos: c[0],
            label: point.into(),
            show_label: true,
        }],
        notch_marks: Vec::new(),
        dimensions: dims
            .iter()
            .enumerate()
            .map(|(i, d)| SceneDimension {
                from: pt(0.0, 0.0),
                to: pt(200.0, 0.0),
                from_offset: pt(0.0, -10.0 - 8.0 * i as f64),
                to_offset: pt(200.0, -10.0 - 8.0 * i as f64),
                text: d.clone(),
            })
            .collect(),
        full_dimensions: Vec::new(),
        markings: Vec::new(),
        stitch_holes: Vec::new(),
        zones: Vec::new(),
        buttons: Vec::new(),
        grain_limits: Vec::new(),
        cut_contours: Vec::new(),
        cut_warnings: Vec::new(),
        darts: Vec::new(),
        dart_warnings: Vec::new(),
        piece_labels: vec![ScenePieceLabel {
            piece: "P".into(),
            center: pt(100.0, 75.0),
            lines: label.to_vec(),
        }],
        folds: Vec::new(),
        fold_overs: Vec::new(),
    }
}

/// Une opération de texte relue : CID, texte décodé par `ToUnicode`, abscisse (mm) et corps (pt).
struct Op {
    cids: Vec<u16>,
    text: String,
    x_mm: f64,
    size_pt: f64,
}

/// Opérations `BT /F1 s Tf 0 g x y Td <hex> Tj ET` de tout le fichier (lecteur du test).
fn read_ops(pdf: &str, map: &HashMap<u16, String>) -> Vec<Op> {
    let mut out = Vec::new();
    for line in pdf.split('\n').filter(|l| l.ends_with(" Tj ET")) {
        let toks: Vec<&str> = line.split_whitespace().collect();
        let Some(td) = toks.iter().position(|t| *t == "Td") else {
            continue; // texte orienté (`Tm`) : absent de cette campagne
        };
        let (a, b) = (line.rfind('<').unwrap(), line.rfind('>').unwrap());
        let hex = &line[a + 1..b];
        let cids: Vec<u16> = hex
            .as_bytes()
            .chunks(4)
            .map(|q| u16::from_str_radix(std::str::from_utf8(q).unwrap(), 16).unwrap())
            .collect();
        let text = cids
            .iter()
            .map(|c| map.get(c).cloned().unwrap_or_else(|| "\u{FFFD}".into()))
            .collect();
        out.push(Op {
            cids,
            text,
            x_mm: toks[td - 2].parse::<f64>().unwrap() * 25.4 / 72.0,
            size_pt: toks[td - 6].parse().unwrap(),
        });
    }
    out
}

// ---------------------------------------------------------------------------
// Campagne
// ---------------------------------------------------------------------------

#[test]
fn campagne_adverse_de_180_sequences_de_police_embarquee() {
    let font = repo_font();
    let ascii: Vec<char> = ascii().chars().collect();
    let pools: Vec<Vec<char>> = [LATIN1, EXT_A, PUNCT, GREEK, ABSENT]
        .iter()
        .map(|s| s.chars().collect())
        .collect();
    let mut seed = 0x06_06_F0_47u64;
    for seq in 0..SEQUENCES {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407 + seq);
        let mut rng = Rng(seed | 1);
        // Tirage : lignes d'étiquette (préfixe « i: » : chaque ligne est unique), cotations
        // (préfixe « d i: »), nom de point court (au plus 12 caractères : jamais abrégé).
        let n_lines = 1 + rng.below(5) as usize;
        let n_dims = rng.below(4) as usize;
        let label: Vec<String> = (0..n_lines)
            .map(|i| format!("{i}:{}", random_text(&mut rng, &ascii, &pools, &font)))
            .collect();
        let dims: Vec<String> = (0..n_dims)
            .map(|i| format!("d{i}:{}", random_text(&mut rng, &ascii, &pools, &font)))
            .collect();
        let point: String = (0..rng.below(8))
            .map(|_| random_char(&mut rng, &ascii, &pools))
            .collect();

        let sc = scene(&label, &dims, &point);
        let out = export_pdf(std::slice::from_ref(&sc))
            .unwrap_or_else(|e| panic!("séquence {seq} : export refusé : {e}"));
        if seq % 4 == 0 {
            let again = export_pdf(std::slice::from_ref(&sc)).unwrap();
            ck!(seq, again.bytes == out.bytes, "export non déterministe");
            REPEATS.fetch_add(1, Relaxed);
        }
        ck!(seq, out.bytes.is_ascii(), "PDF non ASCII");
        let pdf = std::str::from_utf8(&out.bytes).unwrap();

        // --- lecteurs indépendants --------------------------------------------------------
        let pf = read_font(&out.bytes);
        let ops = read_ops(pdf, &to_unicode(pdf));
        ck!(seq, !ops.is_empty(), "aucun texte relu");

        // Chaînes attendues (étiquette, cotations, point) ; multiset par texte exact.
        let mut want: HashMap<String, usize> = HashMap::new();
        for t in label.iter().chain(&dims) {
            *want.entry(expected(t, &font)).or_default() += 1;
        }
        if !point.is_empty() {
            *want.entry(expected(&point, &font)).or_default() += 1;
        }
        let mut got: HashMap<String, usize> = HashMap::new();
        for op in &ops {
            *got.entry(op.text.clone()).or_default() += 1;
        }
        for (text, n) in &want {
            let have = got.get(text).copied().unwrap_or(0);
            ck!(
                seq,
                have >= *n,
                "texte écrit {text:?} relu {have} fois (attendu {n})"
            );
            TEXTS.fetch_add(*n, Relaxed);
        }

        // --- caractère à caractère : ToUnicode, puis CIDToGIDMap + cmap de la police -----------
        for t in label.iter().chain(&dims) {
            let exp = expected(t, &font);
            let op = ops.iter().find(|o| o.text == exp);
            let op = op.unwrap_or_else(|| panic!("séquence {seq} : {exp:?} non retrouvé"));
            ck!(
                seq,
                op.cids.len() == exp.chars().count(),
                "{} CID pour {} caractères",
                op.cids.len(),
                exp.chars().count()
            );
            for (cid, ch) in op.cids.iter().zip(exp.chars()) {
                ck!(
                    seq,
                    pf.to_unicode.get(cid).map(String::as_str) == Some(ch.to_string().as_str()),
                    "ToUnicode de {cid:04X} ≠ {ch:?}"
                );
                CHARS_UNICODE.fetch_add(1, Relaxed);
                let gid = pf.cid_to_gid.get(usize::from(*cid)).copied();
                ck!(
                    seq,
                    gid == font.cmap.get(&(ch as u32)).copied(),
                    "CIDToGIDMap de {cid:04X} ≠ glyphe de {ch:?}"
                );
                CHARS_GID.fetch_add(1, Relaxed);
                if !ch.is_ascii() && ch != '?' {
                    ACCENTED.fetch_add(1, Relaxed);
                }
                EXPECTED_CHARS.fetch_add(1, Relaxed);
            }
        }

        // --- W, hmtx et glyphes du sous-ensemble intégré ----------------------------------------
        let embedded = common::pdf_text::parse_ttf(&pf.font_file);
        for (cid, width) in &pf.widths {
            let ch = pf.to_unicode[cid].chars().next().unwrap();
            let adv = i64::from(font.advance_of(ch).unwrap());
            ck!(
                seq,
                *width == adv * 1000 / i64::from(font.upem),
                "/W de {ch:?} : {width}"
            );
            let gid = pf.cid_to_gid[usize::from(*cid)];
            ck!(
                seq,
                i64::from(embedded.advance_of_gid(gid)) == adv,
                "hmtx intégré de {ch:?}"
            );
            WIDTHS.fetch_add(1, Relaxed);
            ck!(
                seq,
                embedded.glyph_len[usize::from(gid)] == font.glyph_len[usize::from(gid)],
                "glyphe {ch:?} (gid {gid}) absent ou tronqué du sous-ensemble"
            );
            GLYPHS.fetch_add(1, Relaxed);
        }
        ck!(
            seq,
            pdf.contains(&format!("/Length1 {}", pf.font_file.len())),
            "/Length1 ≠ taille de la police intégrée"
        );

        // --- centrage avec les vraies largeurs ---------------------------------------------------
        // Lignes d'étiquette et cotations horizontales sont centrées sur la même abscisse.
        let centred: Vec<&Op> = label
            .iter()
            .chain(&dims)
            .map(|t| {
                let exp = expected(t, &font);
                ops.iter().find(|o| o.text == exp).unwrap()
            })
            .collect();
        let centre = |o: &Op| o.x_mm + font.width_mm(&o.text, o.size_pt) / 2.0;
        let c0 = centre(centred[0]);
        for o in &centred {
            ck!(
                seq,
                near(centre(o), c0, 0.02),
                "centre {:.4} ≠ {:.4} pour {:?}",
                centre(o),
                c0,
                o.text.chars().take(12).collect::<String>()
            );
            CENTERS.fetch_add(1, Relaxed);
        }
    }

    // --- compteurs : les contrôles forts ont réellement tourné ---------------------------------
    let n = |c: &AtomicUsize| c.load(Relaxed);
    assert!(
        n(&TEXTS) >= SEQUENCES as usize,
        "textes relus : {}",
        n(&TEXTS)
    );
    assert_eq!(n(&CHARS_UNICODE), n(&EXPECTED_CHARS));
    assert_eq!(n(&CHARS_GID), n(&EXPECTED_CHARS));
    assert!(
        n(&EXPECTED_CHARS) > 3000,
        "caractères contrôlés : {}",
        n(&EXPECTED_CHARS)
    );
    assert!(
        n(&REPLACED) > 200,
        "caractères hors jeu remplacés : {}",
        n(&REPLACED)
    );
    assert!(
        n(&ACCENTED) > 500,
        "caractères non ASCII fidèles : {}",
        n(&ACCENTED)
    );
    assert!(
        n(&EMPTY) >= 3 && n(&LONG) >= 3,
        "vides {} longs {}",
        n(&EMPTY),
        n(&LONG)
    );
    assert!(n(&WIDTHS) > 2000 && n(&GLYPHS) == n(&WIDTHS));
    assert!(n(&CENTERS) >= SEQUENCES as usize);
    assert!(n(&REPEATS) >= 45, "répétitions : {}", n(&REPEATS));
}
