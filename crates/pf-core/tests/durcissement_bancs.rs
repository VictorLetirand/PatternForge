//! Durcissement 1 : bancs de B1 (création de points) et B2 (modifications d'un gros projet).
//! Ignorés par défaut :
//! `cargo test --release -p pf-core --test durcissement_bancs -- --ignored --nocapture --test-threads 1`

mod support;

use pf_core::*;
use pf_geometry::PointMm;
use std::time::Instant;

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}

/// B1 : `n` points libres créés un par un dans la même feuille.
fn free_points(n: usize) -> f64 {
    let mut p = Project::new("B1");
    let sheet = p.sheets[0].id;
    let t = Instant::now();
    for i in 0..n {
        let at = PointMm::new((i % 100) as f64 * 10.0, (i / 100) as f64 * 10.0);
        p.apply(Command::AddPoint { sheet, at }).unwrap();
    }
    ms(t)
}

/// B1 : chaîne de `n` points construits, chacun décalé du précédent.
fn chained_points(n: usize) -> f64 {
    let mut p = Project::new("B1");
    let sheet = p.sheets[0].id;
    let mut last = p
        .apply(Command::AddPoint {
            sheet,
            at: PointMm::new(0.0, 0.0),
        })
        .unwrap()
        .points[0];
    let t = Instant::now();
    for _ in 1..n {
        last = p
            .apply(Command::AddPointAt {
                sheet,
                point: PointRef::Derived(PointDefinition::Offset {
                    from: last,
                    dx_mm: 0.5,
                    dy_mm: 0.0,
                }),
            })
            .unwrap()
            .points[0];
    }
    ms(t)
}

#[test]
#[ignore = "banc : voir l'en-tête du fichier"]
fn banc_b1_creation_de_points() {
    for n in [200, 800] {
        eprintln!("[B1] {n} points libres : {:.0} ms", free_points(n));
    }
    for n in [100, 300] {
        eprintln!("[B1] chaîne de {n} points : {:.0} ms", chained_points(n));
    }
}

/// B2 : durée d'une modification par `Document::execute`, hors geste et pendant un geste.
fn modifications(megabytes: usize) {
    let g = support::generate(megabytes * 1_000_000, 100);
    let sheet = g.project.sheets[0].id;
    let point = g.project.sheets[0].points[0].id;
    let home = g.project.sheets[0].position(point).unwrap();
    let mv = |i: usize| Command::MovePoint {
        sheet,
        point,
        to: PointMm::new(home.x - 0.01 * (i + 1) as f64, home.y),
    };
    let clone_ms = {
        let t = Instant::now();
        for _ in 0..20 {
            std::hint::black_box(g.project.clone());
        }
        ms(t) / 20.0
    };
    let validate_ms = {
        let t = Instant::now();
        for _ in 0..5 {
            g.project.validate().unwrap();
        }
        ms(t) / 5.0
    };
    const N: usize = 30;
    let mut doc = Document::new(g.project.clone());
    let t = Instant::now();
    for i in 0..N {
        doc.execute(mv(i)).unwrap();
    }
    let single = ms(t) / N as f64;
    let mut doc = Document::new(g.project.clone());
    doc.begin_group();
    let t = Instant::now();
    for i in 0..N {
        doc.execute(mv(i)).unwrap();
    }
    let grouped = ms(t) / N as f64;
    doc.end_group();
    eprintln!(
        "[B2] {megabytes} Mo : clone {clone_ms:.2} ms | validation {validate_ms:.2} ms | \
         execute hors geste {single:.2} ms | execute en geste {grouped:.2} ms"
    );
}

#[test]
#[ignore = "banc : voir l'en-tête du fichier"]
fn banc_b2_modifications() {
    for mb in [1, 10] {
        modifications(mb);
    }
}
