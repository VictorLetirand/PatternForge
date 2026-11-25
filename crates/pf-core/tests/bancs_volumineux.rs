//! US-17.4 - projets volumineux. Le générateur (`tests/support`) est testé dans la suite normale
//! (petites tailles). Les **bancs** de 1, 10 et 50 Mo sont `#[ignore]` pour ne pas ralentir la
//! suite ; ils se lancent à la main, en release :
//!
//! `cargo test --release -p pf-core --test bancs_volumineux -- --ignored --nocapture`
//!
//! Ils mesurent ouverture, validation, `apply` d'une commande simple (`Document::execute`),
//! annuler / rétablir, sauvegarde et export SVG d'une feuille, et **échouent au-dessus des
//! budgets** de `docs/BUDGETS_PERF.md` (relevés sur la machine de développement, avec marge).

mod support;

use pf_core::persist;
use pf_core::*;
use pf_export::{svg, Scene};
use std::path::PathBuf;
use std::time::Instant;
use support::{expected_counts, generate};

const RECTS: usize = 100;

fn tmp(name: &str) -> PathBuf {
    let d = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target")
        .join(format!("us174-{name}"));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d.canonicalize().unwrap()
}

#[test]
fn generateur_nombres_calcules_deterministes_et_taille_atteinte() {
    let g = support::generate(200_000, 20);
    assert!(g.json_bytes >= 200_000, "{}", g.json_bytes);
    // Une feuille de 20 rectangles pèse moins de 200 Ko : plusieurs feuilles, au plus une de trop.
    assert!(g.sheets >= 2);
    let one = persist::to_json(&support::generate(1, 20).project)
        .unwrap()
        .len();
    assert!(
        g.json_bytes < 200_000 + one + one / 10,
        "pas plus d'une feuille de trop"
    );
    // Comptes = formule écrite à la main : 4 points, 4 tracés et 1 pièce par rectangle.
    let (pts, paths, pieces) = expected_counts(g.sheets, 20);
    assert_eq!((g.points, g.paths, g.pieces), (pts, paths, pieces));
    assert_eq!(g.project.sheets.len(), g.sheets);
    assert_eq!(
        g.project
            .sheets
            .iter()
            .map(|s| s.points.len())
            .sum::<usize>(),
        4 * 20 * g.sheets
    );
    assert_eq!(
        g.project
            .sheets
            .iter()
            .map(|s| s.entities.len())
            .sum::<usize>(),
        4 * 20 * g.sheets
    );
    assert_eq!(g.project.pieces.len(), 20 * g.sheets);
    // Déterminisme : mêmes octets deux fois, et aller-retour par le fichier.
    let again = support::generate(200_000, 20);
    let (a, b) = (
        persist::to_json(&g.project).unwrap(),
        persist::to_json(&again.project).unwrap(),
    );
    assert_eq!(a, b);
    assert_eq!(a.len(), g.json_bytes);
    assert_eq!(persist::from_json(&a).unwrap(), g.project);
}

#[test]
fn generateur_plus_grand_donne_plus_de_feuilles() {
    let small = support::generate(100_000, 10);
    let big = support::generate(400_000, 10);
    assert!(big.sheets > small.sheets);
    assert!(big.json_bytes >= 400_000 && small.json_bytes >= 100_000);
}

struct Measures {
    open_ms: u128,
    validate_ms: u128,
    apply_ms: u128,
    undo_ms: u128,
    redo_ms: u128,
    save_ms: u128,
    svg_ms: u128,
}

fn ms(t: Instant) -> u128 {
    t.elapsed().as_millis()
}

fn measure(label: &str, target_mb: usize) -> Measures {
    let dir = tmp(label);
    let t = Instant::now();
    let g = generate(target_mb * 1024 * 1024, RECTS);
    let gen_ms = ms(t);
    let path = dir.join("gros.pforge.json");
    persist::save(&path, &g.project).unwrap();
    let on_disk = std::fs::metadata(&path).unwrap().len();
    eprintln!(
        "[{label}] généré en {gen_ms} ms : {} feuilles, {} points, {} tracés, {} pièces, {:.1} Mo",
        g.sheets,
        g.points,
        g.paths,
        g.pieces,
        on_disk as f64 / 1048576.0
    );
    assert!(on_disk as usize >= target_mb * 1024 * 1024);
    drop(g);

    let t = Instant::now();
    let mut doc = Document::open(&path).unwrap();
    let open_ms = ms(t);
    let t = Instant::now();
    doc.project().validate().unwrap();
    let validate_ms = ms(t);
    let first = doc.project().sheets[0].id;
    let t = Instant::now();
    doc.execute(Command::RenameSheet {
        sheet: first,
        name: "Renommee".into(),
    })
    .unwrap();
    let apply_ms = ms(t);
    let t = Instant::now();
    assert!(doc.undo());
    let undo_ms = ms(t);
    let t = Instant::now();
    assert!(doc.redo());
    let redo_ms = ms(t);
    let t = Instant::now();
    doc.save_checked(&path).unwrap();
    let save_ms = ms(t);
    let t = Instant::now();
    let scene = Scene::from_sheet(doc.project(), first).unwrap();
    let out = svg::export_svg(&scene).unwrap();
    let svg_ms = ms(t);
    assert!(out.starts_with("<?xml") || out.starts_with("<svg"));
    let m = Measures {
        open_ms,
        validate_ms,
        apply_ms,
        undo_ms,
        redo_ms,
        save_ms,
        svg_ms,
    };
    eprintln!(
        "[{label}] ouverture {} ms | validation {} ms | apply {} ms | annuler {} ms | rétablir {} ms | sauvegarde {} ms | SVG (1 feuille) {} ms",
        m.open_ms, m.validate_ms, m.apply_ms, m.undo_ms, m.redo_ms, m.save_ms, m.svg_ms
    );
    let _ = std::fs::remove_dir_all(dir);
    m
}

/// Budgets en millisecondes (voir `docs/BUDGETS_PERF.md`) : ouverture, validation, apply,
/// annuler, rétablir, sauvegarde, SVG.
fn check(m: &Measures, b: [u128; 7]) {
    let got = [
        m.open_ms,
        m.validate_ms,
        m.apply_ms,
        m.undo_ms,
        m.redo_ms,
        m.save_ms,
        m.svg_ms,
    ];
    let names = [
        "ouverture",
        "validation",
        "apply",
        "annuler",
        "rétablir",
        "sauvegarde",
        "export SVG",
    ];
    for i in 0..7 {
        assert!(
            got[i] <= b[i],
            "budget dépassé : {} = {} ms > {} ms",
            names[i],
            got[i],
            b[i]
        );
    }
}

const BUDGET_1_MB: [u128; 7] = [100, 20, 20, 20, 20, 100, 20];
const BUDGET_10_MB: [u128; 7] = [700, 80, 100, 20, 20, 500, 80];
const BUDGET_50_MB: [u128; 7] = [6000, 500, 600, 50, 50, 4000, 500];

#[test]
#[ignore = "banc volumineux : cargo test --release --test bancs_volumineux -- --ignored"]
fn banc_1_mo() {
    check(&measure("1mo", 1), BUDGET_1_MB);
}

#[test]
#[ignore = "banc volumineux : cargo test --release --test bancs_volumineux -- --ignored"]
fn banc_10_mo() {
    check(&measure("10mo", 10), BUDGET_10_MB);
}

#[test]
#[ignore = "banc volumineux : cargo test --release --test bancs_volumineux -- --ignored"]
fn banc_50_mo() {
    check(&measure("50mo", 50), BUDGET_50_MB);
}
