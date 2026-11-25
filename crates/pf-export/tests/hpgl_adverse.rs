//! US-12.5 — campagne adverse de l'export HPGL : 160 séquences à graine fixe qui combinent des
//! scènes aléatoires (14 rôles, lignes, Béziers, arcs, cercles, splines, crans, cotations,
//! marquages, boutons, points ; coordonnées négatives comprises), des sélections de contenu au
//! hasard (US-13.4), des formats (feuilles, rouleaux, formats libres, paysage, marges), des
//! facteurs X / Y (US-13.3) et quelques entrées volontairement invalides (NaN, hors plage, facteur
//! hors bornes).
//!
//! L'oracle est écrit sans le code d'export (`common/hpgl.rs` : lecteur HPGL, table des plumes et
//! géométrie fine de la scène). Invariants vérifiés à chaque séquence : jamais de panique ; un
//! refus est le bon refus (rien d'écrit, erreur typée) ; sinon la relecture égale la scène (nombre
//! de tracés, longueur par plume, emprise, calage à l'origine) ; une seule sélection par plume,
//! numéros croissants ; l'ordre optimisé n'est jamais plus long à vide que l'ordre naturel et
//! trace les mêmes segments ; la somme des `PU` égale le rapport ; déterminisme à l'octet ; aucune
//! valeur non finie ni hors plage dans le fichier.
//!
//! Rien de ceci ne remplace un essai sur un vrai traceur.

mod common;

use common::hpgl::*;
use common::{pt, Rng};
use pf_export::hpgl::{export_hpgl, export_hpgl_with, HpglError, HpglOptions};
use pf_export::{
    Margins, Orientation, OutputCategory, OutputContent, PageFormat, PageSetup, PageSetupError,
    PathGeom, PrintScale,
};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

const SEQUENCES: u64 = 160;

static ACCEPTED: AtomicUsize = AtomicUsize::new(0);
static TOO_BIG: AtomicUsize = AtomicUsize::new(0);
static NOTHING: AtomicUsize = AtomicUsize::new(0);
static NAN_REFUSED: AtomicUsize = AtomicUsize::new(0);
static RANGE_REFUSED: AtomicUsize = AtomicUsize::new(0);
static SCALE_REFUSED: AtomicUsize = AtomicUsize::new(0);

macro_rules! ck {
    ($seq:expr, $cond:expr, $($arg:tt)+) => {
        match $cond {
            true => {}
            false => panic!("séquence {} : {}", $seq, format!($($arg)+)),
        }
    };
}

fn random_content(rng: &mut Rng) -> OutputContent {
    match rng.below(10) {
        0..=2 => OutputContent::default(),
        3 => OutputContent::sewing_only(),
        4 => OutputContent::cutting_only(),
        // Rien de traçable : des points seuls, ou plus rien du tout.
        5 => OutputContent::only(&[OutputCategory::Points, OutputCategory::PointLabels]),
        6 => OutputContent::only(&[]),
        _ => {
            let mut c = OutputContent::default();
            for cat in OutputCategory::ALL {
                c.set(cat, rng.below(4) != 0);
            }
            c
        }
    }
}

/// Réglage et zone imprimable attendue (écrite ici, en dur) : `(page, largeur, hauteur ou rouleau)`.
fn random_page(rng: &mut Rng) -> (PageSetup, f64, Option<f64>) {
    let m = rng.range(5.0, 20.0);
    let (format, w, h): (PageFormat, f64, Option<f64>) = match rng.below(5) {
        0 => (PageFormat::A4, 210.0, Some(297.0)),
        1 => (PageFormat::A3, 297.0, Some(420.0)),
        2 => (PageFormat::Letter, 215.9, Some(279.4)),
        3 => {
            let (w, h) = (rng.range(260.0, 700.0), rng.range(260.0, 700.0));
            (
                PageFormat::Custom {
                    width_mm: w,
                    height_mm: h,
                },
                w,
                Some(h),
            )
        }
        _ => {
            let w = rng.range(300.0, 914.0);
            (PageFormat::Roll { width_mm: w }, w, None)
        }
    };
    let landscape = rng.below(3) == 0;
    let (w, h) = match (landscape, h) {
        (true, Some(h)) => (h, Some(w)),
        _ => (w, h),
    };
    let scale = if rng.below(3) == 0 {
        PrintScale::default()
    } else {
        PrintScale {
            x: rng.range(0.92, 1.08),
            y: rng.range(0.92, 1.08),
        }
    };
    let page = PageSetup {
        format,
        orientation: if landscape {
            Orientation::Landscape
        } else {
            Orientation::Portrait
        },
        margins: Margins::uniform(m),
        scale,
        ..PageSetup::default()
    };
    (page, w - 2.0 * m, h.map(|h| h - 2.0 * m))
}

fn run(seq: u64) {
    let mut rng = Rng(seq.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let spread = if rng.below(5) == 0 { 2.8 } else { 1.0 };
    let full = random_scene(&mut rng, spread);
    let content = random_content(&mut rng);
    let (mut page, zone_w, zone_h) = random_page(&mut rng);

    // Réglage invalide : refus typé, rien d'écrit.
    if rng.below(16) == 0 {
        page.scale = PrintScale { x: 1.3, y: 1.0 };
        let e = export_hpgl(&full, &page).unwrap_err();
        ck!(
            seq,
            matches!(e, HpglError::Page(PageSetupError::Scale(_))),
            "{e:?}"
        );
        SCALE_REFUSED.fetch_add(1, Relaxed);
        return;
    }

    let mut scene = match content.apply(&full) {
        Ok(s) => s,
        Err(pf_export::ExportError::NothingSelected(_)) => {
            ck!(
                seq,
                reference(&full).is_empty() || !content.is_default(),
                "refus inattendu"
            );
            NOTHING.fetch_add(1, Relaxed);
            return;
        }
        Err(e) => panic!("séquence {seq} : {e}"),
    };

    // Entrées invalides injectées après le filtre de contenu.
    let injected = rng.below(14);
    if injected == 0 && !scene.paths.is_empty() {
        scene.paths[0].geom = PathGeom::Line(pt(f64::NAN, 1.0), pt(5.0, 5.0));
        let e = export_hpgl(&scene, &page).unwrap_err();
        ck!(seq, e == HpglError::NotFinite, "{e:?}");
        NAN_REFUSED.fetch_add(1, Relaxed);
        return;
    }
    if injected == 1 {
        scene
            .notch_marks
            .push((pt(0.0, 0.0), pt(2.0e6 * (1.0 + rng.unit()), 0.0)));
        let e = export_hpgl(&scene, &page).unwrap_err();
        ck!(seq, matches!(e, HpglError::OutOfRange { .. }), "{e:?}");
        RANGE_REFUSED.fetch_add(1, Relaxed);
        return;
    }

    let (sx, sy) = (page.scale.x, page.scale.y);
    let refs: Vec<Poly> = reference(&scene).into_iter().map(|r| r.1).collect();
    let result = export_hpgl(&scene, &page);
    let Some((x0, y0, x1, y1)) = bbox_of(refs.iter()) else {
        ck!(
            seq,
            result == Err(HpglError::Nothing),
            "scène sans trait : {result:?}"
        );
        NOTHING.fetch_add(1, Relaxed);
        return;
    };
    let (w, h) = ((x1 - x0) * sx, (y1 - y0) * sy);
    let too_big = w > zone_w + 0.5 || zone_h.is_some_and(|zh| h > zh + 0.5);
    let fits = w < zone_w - 0.5 && zone_h.is_none_or(|zh| h < zh - 0.5);
    match result {
        Err(e) => {
            ck!(
                seq,
                too_big || !fits,
                "refus d'une scène {w} x {h} dans {zone_w} x {zone_h:?} : {e}"
            );
            ck!(
                seq,
                matches!(
                    e,
                    HpglError::SheetTooSmall { .. }
                        | HpglError::Page(PageSetupError::ContentTooWide { .. })
                ),
                "{e:?}"
            );
            ck!(seq, !e.to_string().is_empty(), "message vide");
            TOO_BIG.fetch_add(1, Relaxed);
        }
        Ok(plot) => {
            ck!(
                seq,
                !too_big,
                "scène {w} x {h} acceptée dans {zone_w} x {zone_h:?}"
            );
            ACCEPTED.fetch_add(1, Relaxed);
            let r = check_against_scene(&scene, &plot.text, sx, sy)
                .unwrap_or_else(|e| panic!("séquence {seq} : {e}"));
            // Valeurs finies, entières, dans la plage.
            ck!(
                seq,
                !plot.text.contains("NaN") && !plot.text.contains("inf"),
                "valeur non finie"
            );
            for s in &r.strokes {
                for p in &s.pts {
                    ck!(
                        seq,
                        (0..=8_388_607).contains(&p.0) && (0..=8_388_607).contains(&p.1),
                        "hors plage"
                    );
                }
            }
            // Une sélection par plume, numéros croissants, seules les plumes présentes.
            let mut sorted = r.selections.clone();
            sorted.sort_unstable();
            sorted.dedup();
            ck!(seq, sorted == r.selections, "sélections {:?}", r.selections);
            let mut used: Vec<u8> = reference(&scene).iter().map(|x| x.0).collect();
            used.sort_unstable();
            used.dedup();
            ck!(
                seq,
                used == r.selections,
                "plumes {:?} / {:?}",
                used,
                r.selections
            );
            // Chemin à vide : rapport = somme des PU = recalcul ; optimisé ≤ naturel.
            ck!(
                seq,
                (r.pen_up_mm - plot.report.travel_mm).abs() < 1e-6,
                "PU {}",
                r.pen_up_mm
            );
            ck!(seq, (travel_of(&r) - r.pen_up_mm).abs() < 1e-6, "travel_of");
            ck!(
                seq,
                r.strokes.len() == plot.report.strokes,
                "nombre de tracés"
            );
            let nat = export_hpgl_with(&scene, &page, HpglOptions { optimize: false }).unwrap();
            let rn = read_hpgl(&nat.text).unwrap();
            ck!(
                seq,
                travel_of(&r) <= travel_of(&rn) + 1e-9,
                "à vide {} > {}",
                travel_of(&r),
                travel_of(&rn)
            );
            ck!(
                seq,
                segment_multiset(&r) == segment_multiset(&rn),
                "segments différents"
            );
            // Déterminisme à l'octet.
            ck!(
                seq,
                export_hpgl(&scene, &page).unwrap().text == plot.text,
                "non déterministe"
            );
            // Taille annoncée = taille relue.
            let max_x = r
                .strokes
                .iter()
                .flat_map(|s| &s.pts)
                .map(|p| p.0)
                .max()
                .unwrap();
            ck!(
                seq,
                (plot.report.width_mm - max_x as f64 / UNITS).abs() < 1e-9,
                "largeur annoncée"
            );
        }
    }
}

#[test]
fn campagne_adverse_hpgl_160_sequences() {
    for seq in 1..=SEQUENCES {
        run(seq);
    }
    // Garde : la campagne a réellement exercé chaque branche.
    assert!(
        ACCEPTED.load(Relaxed) >= 80,
        "acceptées : {}",
        ACCEPTED.load(Relaxed)
    );
    assert!(
        TOO_BIG.load(Relaxed) >= 8,
        "trop grandes : {}",
        TOO_BIG.load(Relaxed)
    );
    assert!(
        NOTHING.load(Relaxed) >= 3,
        "sans trait : {}",
        NOTHING.load(Relaxed)
    );
    assert!(
        NAN_REFUSED.load(Relaxed) >= 3,
        "NaN : {}",
        NAN_REFUSED.load(Relaxed)
    );
    assert!(
        RANGE_REFUSED.load(Relaxed) >= 3,
        "plage : {}",
        RANGE_REFUSED.load(Relaxed)
    );
    assert!(
        SCALE_REFUSED.load(Relaxed) >= 3,
        "échelle : {}",
        SCALE_REFUSED.load(Relaxed)
    );
}
