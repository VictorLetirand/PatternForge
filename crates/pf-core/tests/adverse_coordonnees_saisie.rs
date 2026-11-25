//! Campagne adverse : coordonnees saisie (graine fixe).

mod adverse_support;
use adverse_support::*;

use pf_geometry::PointMm;
use std::panic::{catch_unwind, AssertUnwindSafe};

// ---------------------------------------------------------------------------
// US-16.2 — saisie directe : analyse de coordonnées
// ---------------------------------------------------------------------------

/// Texte tiré au hasard dans un alphabet de morceaux valides, invalides,
/// extrêmes et unicode.
fn random_coordinate_text(r: &mut Rng) -> String {
    const PIECES: &[&str] = &[
        "0",
        "1",
        "7",
        "12",
        "45",
        "90",
        "180",
        "-5",
        "+3",
        "1,5",
        "2.75",
        ";",
        ",",
        ".",
        "@",
        "<",
        " ",
        "\t",
        "mm",
        "cm",
        "m",
        "°",
        "e",
        "E",
        "NaN",
        "inf",
        "-inf",
        "1e300",
        "é",
        "１２",
        "💥",
        "\u{a0}",
        "\0",
        "0.0000000001",
        "@@",
        "<<",
        ";;",
        "x",
        "10;20",
        "@5;5",
        "@10<30",
        "@0<0",
        "-0",
        "1/0",
        "999999999999999999999999999999999999999999",
    ];
    let mut text = String::new();
    for _ in 0..r.below(8) {
        if r.chance(3) {
            text.push('9');
            text.push_str(&"0".repeat(300 + r.below(200)));
        } else {
            text.push_str(PIECES[r.below(PIECES.len())]);
        }
    }
    text
}

#[test]
fn us162_campagne_aleatoire_des_coordonnees_graine_fixe() {
    use pf_core::input::{parse_coordinate_with, CoordInput};
    let mut r = Rng(0x1620_0002);
    let (mut ok, mut refused) = (0, 0);
    // 1. Textes quelconques : jamais de panique ; un succès reste dans l'espace de travail.
    for draw in 0..600 {
        let text = random_coordinate_text(&mut r);
        let origin = PointMm::new((r.unit() - 0.5) * 2e5, (r.unit() - 0.5) * 2e5);
        for decimal in [',', '.'] {
            let result = catch_unwind(AssertUnwindSafe(|| {
                parse_coordinate_with(&text, origin, decimal)
            }));
            let Ok(result) = result else {
                panic!("panique au tirage {draw} ({decimal:?}) : {text:?}");
            };
            match result {
                Ok(c) => {
                    let p = c.point();
                    assert!(
                        p.x.is_finite()
                            && p.y.is_finite()
                            && p.x.abs() <= pf_geometry::MAX_COORD_MM
                            && p.y.abs() <= pf_geometry::MAX_COORD_MM,
                        "tirage {draw} : point hors espace {p:?} pour {text:?}"
                    );
                    ok += 1;
                }
                Err(message) => {
                    assert!(!message.is_empty(), "message vide pour {text:?}");
                    refused += 1;
                }
            }
        }
    }
    assert!(refused > 500, "{refused} refus");
    // 2. Textes valides construits : le résultat est celui d'un calcul indépendant.
    // Les nombres sont écrits avec 3 (ou 2) décimales puis relus par l'oracle : seul
    // le calcul de la coordonnée est mis à l'épreuve.
    let number = |r: &mut Rng, span: f64| format!("{:.3}", (r.unit() - 0.5) * span);
    for draw in 0..300 {
        let (xs, ys) = (number(&mut r, 1e5), number(&mut r, 1e5));
        let (ox, oy) = (number(&mut r, 1e5), number(&mut r, 1e5));
        let origin = PointMm::new(ox.parse().unwrap(), oy.parse().unwrap());
        let (x, y): (f64, f64) = (xs.parse().unwrap(), ys.parse().unwrap());
        let length = format!("{:.2}", r.unit() * 5e4 + 0.01);
        let angle = format!("{:.2}", (r.unit() - 0.5) * 720.0);
        let (len_v, ang_v): (f64, f64) = (length.parse().unwrap(), angle.parse().unwrap());
        let fr = |s: &str| s.replace('.', ",");
        // (texte, séparateur décimal, point attendu, tolérance)
        let cases = [
            (format!("{};{}", fr(&xs), fr(&ys)), ',', (x, y), 0.0),
            (format!("{xs},{ys}"), '.', (x, y), 0.0),
            (
                format!("@{};{}", fr(&xs), fr(&ys)),
                ',',
                (origin.x + x, origin.y + y),
                0.0,
            ),
            (
                format!("@{xs};{ys}"),
                '.',
                (origin.x + x, origin.y + y),
                0.0,
            ),
            (
                format!("@{}<{}", fr(&length), fr(&angle)),
                ',',
                (
                    origin.x + len_v * ang_v.to_radians().cos(),
                    origin.y + len_v * ang_v.to_radians().sin(),
                ),
                1e-6,
            ),
        ];
        for (text, decimal, (ex, ey), tol) in cases {
            let c = parse_coordinate_with(&text, origin, decimal)
                .unwrap_or_else(|e| panic!("tirage {draw} : {text:?} refusé : {e}"));
            let p = c.point();
            assert!(
                (p.x - ex).abs() <= tol && (p.y - ey).abs() <= tol,
                "tirage {draw} : {text:?} -> {p:?}, attendu ({ex}, {ey})"
            );
            assert_eq!(
                matches!(c, CoordInput::Polar { .. }),
                text.contains('<'),
                "{text:?}"
            );
            ok += 1;
        }
    }
    assert!(ok > 1500, "{ok} succès");
}
