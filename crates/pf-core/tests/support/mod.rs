//! US-17.4 - générateur de projets synthétiques volumineux (1 Mo, 10 Mo, 50 Mo), déterministe.
//!
//! Une feuille modèle contient `rects` rectangles 100 x 50 mm posés sur une grille de 10 colonnes
//! (pas 120 x 70 mm, donc sans chevauchement), chacun extrait en pièce « Piece N » : 4 points,
//! 4 segments et 1 pièce par rectangle. Les feuilles suivantes sont des copies (`DuplicateSheet`).
//! Les nombres sont **calculés** : `sheets * rects * 4` points, autant de tracés, `sheets * rects`
//! pièces, et le nombre de feuilles est le plus petit qui atteint la taille visée.

#![allow(dead_code)]

use pf_core::persist;
use pf_core::*;
use pf_geometry::PointMm;

pub struct Generated {
    pub project: Project,
    pub sheets: usize,
    pub rects_per_sheet: usize,
    pub points: usize,
    pub paths: usize,
    pub pieces: usize,
    /// Taille du JSON enregistré (octets).
    pub json_bytes: usize,
}

/// Comptes attendus pour `sheets` feuilles de `rects` rectangles.
pub fn expected_counts(sheets: usize, rects: usize) -> (usize, usize, usize) {
    (4 * rects * sheets, 4 * rects * sheets, rects * sheets)
}

fn template(rects: usize) -> Project {
    let mut p = Project::new("Volumineux");
    let s = p.sheets[0].id;
    for i in 0..rects {
        let (col, row) = ((i % 10) as f64, (i / 10) as f64);
        let r = p
            .apply(Command::AddRectangle {
                sheet: s,
                corner: PointRef::New(PointMm::new(col * 120.0, row * 70.0)),
                width_mm: 100.0,
                height_mm: 50.0,
            })
            .unwrap();
        p.apply(Command::ExtractPiece {
            sheet: s,
            source: PieceSource::Paths {
                entities: r.entities,
            },
            name: format!("Piece {i}"),
            detached: false,
        })
        .unwrap();
    }
    p
}

/// Projet d'au moins `target_bytes` octets de JSON (au plus une feuille de plus que nécessaire).
pub fn generate(target_bytes: usize, rects: usize) -> Generated {
    let mut p = template(rects);
    let one = persist::to_json(&p).unwrap().len();
    let first = p.sheets[0].id;
    let mut sheets = 1;
    let mut estimate = one;
    // Estimation de départ (les identifiants à plus de chiffres grossissent un peu les copies),
    // puis ajout feuille par feuille jusqu'à la taille visée.
    while estimate < target_bytes {
        p.apply(Command::DuplicateSheet { sheet: first }).unwrap();
        sheets += 1;
        estimate += one;
    }
    let mut json_bytes = persist::to_json(&p).unwrap().len();
    while json_bytes < target_bytes {
        p.apply(Command::DuplicateSheet { sheet: first }).unwrap();
        sheets += 1;
        json_bytes = persist::to_json(&p).unwrap().len();
    }
    let (points, paths, pieces) = expected_counts(sheets, rects);
    Generated {
        project: p,
        sheets,
        rects_per_sheet: rects,
        points,
        paths,
        pieces,
        json_bytes,
    }
}
