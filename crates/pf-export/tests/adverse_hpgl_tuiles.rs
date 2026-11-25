//! PH-5e — campagne adverse du HPGL tuilé : 170 séquences à graine fixe (feuilles de lignes
//! aléatoires, formats, orientations, marges, recouvrements, facteurs X / Y, repères, « économiser
//! le papier », rouleaux) et quelques entrées volontairement invalides.
//!
//! L'oracle est écrit sans le code de découpe : grille de pages recalculée à la main
//! (`common::grid_of`), lecteur HPGL indépendant (`common/hpgl.rs`), distance point-segment propre à
//! ce fichier. **Recomposition** : chaque page relue est replacée dans le repère de la feuille par
//! translation (coin bas-gauche de sa fenêtre + unités / 40) ; alors
//!   (A) aucun segment recollé ne sort du dessin d'origine (même plume, à 0,06 mm : arrondi de
//!       0,0125 mm par axe et plafonnement à la fenêtre) ;
//!   (B) chaque point d'échantillon (tous les 0,4 mm) de chaque segment d'origine est couvert par un
//!       segment recollé de même plume ;
//!   (C) aucune coordonnée hors de la fenêtre de sa page ;
//!   (D) mêmes pages que le PDF (`plan_tiles_*`), même nombre et mêmes numéros que la grille de
//!       l'oracle ; (E) numéro de page ASCII ; (F) croix de repère : 8 segments par bande ;
//!   (G) une feuille sur une seule page = l'export non tuilé, octet pour octet ; (H) déterminisme.
//!
//! Rien de ceci ne remplace un essai sur un vrai traceur.

mod common;

use common::hpgl::*;
use common::{grid_of, pt, Rng};
use pf_core::PathRole;
use pf_export::hpgl::{export_hpgl_pages, export_hpgl_with, HpglError, HpglOptions};
use pf_export::pdf::{plan_tiles_unrotated, plan_tiles_with};
use pf_export::{
    AssemblyOptions, ExportError, Margins, Orientation, PageFormat, PageSetup, PageSetupError,
    PathGeom, PrintScale, Scene, ScenePath,
};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

const SEQUENCES: u64 = 170;
/// Tolérance (mm) des comparaisons géométriques recollées.
const TOL: f64 = 0.06;
/// Bandeau de titre du PDF (mm), écrit en dur : il retranche la fenêtre de la zone imprimable.
const HEADER: f64 = 8.0;

static MULTI: AtomicUsize = AtomicUsize::new(0);
static SINGLE: AtomicUsize = AtomicUsize::new(0);
static ROLLS: AtomicUsize = AtomicUsize::new(0);
static MARKED: AtomicUsize = AtomicUsize::new(0);
static SAVED: AtomicUsize = AtomicUsize::new(0);
static SCALED: AtomicUsize = AtomicUsize::new(0);
static TOO_MANY: AtomicUsize = AtomicUsize::new(0);
static INVALID: AtomicUsize = AtomicUsize::new(0);
static CLIPPED: AtomicUsize = AtomicUsize::new(0);

macro_rules! ck {
    ($seq:expr, $cond:expr, $($arg:tt)+) => {
        match $cond {
            true => {}
            false => panic!("séquence {} : {}", $seq, format!($($arg)+)),
        }
    };
}

type Seg = (u8, (f64, f64), (f64, f64));

fn dist_point_segment(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let l2 = dx * dx + dy * dy;
    let t = if l2 == 0.0 {
        0.0
    } else {
        (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / l2).clamp(0.0, 1.0)
    };
    (p.0 - (a.0 + t * dx)).hypot(p.1 - (a.1 + t * dy))
}

fn near(p: (f64, f64), segs: &[Seg], pen: u8) -> bool {
    segs.iter()
        .any(|s| s.0 == pen && dist_point_segment(p, s.1, s.2) <= TOL)
}

/// Coordonnée sur la grille de 0,025 mm (les arrondis du fichier ne la déplacent pas).
fn grid_mm(rng: &mut Rng, lo: f64, hi: f64) -> f64 {
    (rng.range(lo, hi) / 0.025).round() * 0.025
}

struct Setup {
    page: PageSetup,
    /// Largeur et hauteur de la fenêtre de patron (zone imprimable moins le bandeau).
    ww: f64,
    wh: f64,
    roll: bool,
}

fn random_page(rng: &mut Rng) -> Setup {
    let m = (rng.range(5.0, 20.0) * 4.0).round() / 4.0;
    let (format, w, h): (PageFormat, f64, Option<f64>) = match rng.below(8) {
        0 | 1 => (PageFormat::A4, 210.0, Some(297.0)),
        2 => (PageFormat::A3, 297.0, Some(420.0)),
        3 => (PageFormat::Letter, 215.9, Some(279.4)),
        4 | 5 => {
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
        6 => (PageFormat::A4, 210.0, Some(297.0)),
        _ => {
            let w = rng.range(400.0, 914.0);
            (PageFormat::Roll { width_mm: w }, w, None)
        }
    };
    let landscape = rng.below(3) == 0 && h.is_some();
    let (w, h) = match (landscape, h) {
        (true, Some(h)) => (h, Some(w)),
        _ => (w, h),
    };
    let scale = if rng.below(3) == 0 {
        PrintScale {
            x: rng.range(0.92, 1.08),
            y: rng.range(0.92, 1.08),
        }
    } else {
        PrintScale::default()
    };
    let page = PageSetup {
        format,
        orientation: if landscape {
            Orientation::Landscape
        } else {
            Orientation::Portrait
        },
        margins: Margins::uniform(m),
        overlap_mm: (rng.range(0.0, 50.0) * 2.0).round() / 2.0,
        assembly: AssemblyOptions {
            marks: rng.below(3) == 0,
            plan: rng.below(5) == 0,
            save_paper: rng.below(4) == 0,
        },
        scale,
        ..PageSetup::default()
    };
    Setup {
        page,
        ww: w - 2.0 * m,
        wh: h.map_or(0.0, |h| h - 2.0 * m - HEADER),
        roll: h.is_none(),
    }
}

const ROLES: [PathRole; 4] = [
    PathRole::CuttingContour,
    PathRole::SewingContour,
    PathRole::Construction,
    PathRole::AnnotationAxis,
];

/// Plume attendue des rôles ci-dessus (table écrite ici).
fn pen_of(role: PathRole) -> u8 {
    match role {
        PathRole::CuttingContour => 1,
        PathRole::SewingContour => 2,
        PathRole::Construction => 3,
        _ => 8,
    }
}

fn random_lines(rng: &mut Rng, spread: f64, marks: bool) -> (Scene, Vec<Seg>) {
    let n = 1 + rng.below(10) as usize;
    let (ox, oy) = (grid_mm(rng, -400.0, 400.0), grid_mm(rng, -400.0, 400.0));
    let (sw, sh) = (
        rng.range(60.0, 1500.0 * spread),
        rng.range(60.0, 1500.0 * spread),
    );
    let mut paths = Vec::new();
    let mut segs = Vec::new();
    for _ in 0..n {
        let a = (ox + grid_mm(rng, 0.0, sw), oy + grid_mm(rng, 0.0, sh));
        let b = if rng.below(4) == 0 {
            // Horizontale ou verticale : arêtes qui longent les bords de fenêtre.
            if rng.below(2) == 0 {
                (ox + grid_mm(rng, 0.0, sw), a.1)
            } else {
                (a.0, oy + grid_mm(rng, 0.0, sh))
            }
        } else {
            (ox + grid_mm(rng, 0.0, sw), oy + grid_mm(rng, 0.0, sh))
        };
        if a == b {
            continue;
        }
        // Les croix de repère sont écrites à la plume 8 : jamais de plume 8 dans le dessin avec.
        let role = if marks {
            ROLES[rng.below(3) as usize]
        } else {
            ROLES[rng.below(4) as usize]
        };
        paths.push(ScenePath {
            geom: PathGeom::Line(pt(a.0, a.1), pt(b.0, b.1)),
            role,
            sense: pf_core::GrainSense::Unmarked,
        });
        segs.push((pen_of(role), a, b));
    }
    if paths.is_empty() {
        paths.push(ScenePath {
            geom: PathGeom::Line(pt(ox, oy), pt(ox + 100.0, oy + 50.0)),
            role: PathRole::CuttingContour,
            sense: pf_core::GrainSense::Unmarked,
        });
        segs.push((1, (ox, oy), (ox + 100.0, oy + 50.0)));
    }
    let mut s = common::scene_of(vec![]);
    s.paths = paths;
    (s, segs)
}

fn run(seq: u64) {
    let mut rng = Rng(seq.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let kind = rng.below(24);
    let big = rng.below(10) == 0;
    let mut st = random_page(&mut rng);
    let options = HpglOptions {
        optimize: rng.below(4) != 0,
    };
    let (scene, segs) = random_lines(
        &mut rng,
        if big { 3.0 } else { 1.0 },
        st.page.assembly.marks,
    );

    // ----- entrées volontairement invalides : refus typé, rien d'écrit -----
    match kind {
        0 => {
            let mut s = scene.clone();
            s.paths[0].geom = PathGeom::Line(pt(0.0, 0.0), pt(f64::NAN, 3.0));
            let r = export_hpgl_pages(&s, &st.page, options);
            ck!(seq, matches!(r, Err(HpglError::NotFinite)), "NaN : {r:?}");
            INVALID.fetch_add(1, Relaxed);
            return;
        }
        1 => {
            let r = export_hpgl_pages(&common::scene_of(vec![]), &st.page, options);
            ck!(seq, matches!(r, Err(HpglError::Nothing)), "vide : {r:?}");
            INVALID.fetch_add(1, Relaxed);
            return;
        }
        2 => {
            st.page.overlap_mm = 50.0 + rng.range(0.5, 40.0);
            let r = export_hpgl_pages(&scene, &st.page, options);
            ck!(
                seq,
                matches!(
                    r,
                    Err(HpglError::Page(PageSetupError::OverlapOutOfRange { .. }))
                ),
                "recouvrement : {r:?}"
            );
            INVALID.fetch_add(1, Relaxed);
            return;
        }
        3 => {
            st.page.margins = Margins::uniform(100.0 + rng.range(1.0, 50.0));
            let r = export_hpgl_pages(&scene, &st.page, options);
            ck!(seq, matches!(r, Err(HpglError::Page(_))), "marge : {r:?}");
            INVALID.fetch_add(1, Relaxed);
            return;
        }
        _ => {}
    }

    let result = export_hpgl_pages(&scene, &st.page, options);
    let again = export_hpgl_pages(&scene, &st.page, options);
    ck!(seq, result == again, "résultat non déterministe");

    // ----- rouleau : jamais découpé -----
    if st.roll {
        ROLLS.fetch_add(1, Relaxed);
        let plain = export_hpgl_with(&scene, &st.page, options);
        match (result, plain) {
            (Ok(t), Ok(p)) => {
                ck!(
                    seq,
                    t.pages.len() == 1,
                    "rouleau en {} pages",
                    t.pages.len()
                );
                ck!(seq, t.pages[0].text == p.text, "rouleau ≠ export non tuilé");
            }
            (Err(a), Err(b)) => ck!(seq, a == b, "refus de rouleau différents {a:?} / {b:?}"),
            (a, b) => panic!("séquence {seq} : rouleau {a:?} / {b:?}"),
        }
        return;
    }

    // ----- grille de l'oracle -----
    let sc = st.page.scale;
    let scaled: Vec<Seg> = segs
        .iter()
        .map(|&(p, a, b)| (p, (a.0 * sc.x, a.1 * sc.y), (b.0 * sc.x, b.1 * sc.y)))
        .collect();
    let (mut lo, mut hi) = (scaled[0].1, scaled[0].1);
    for s in &scaled {
        for q in [s.1, s.2] {
            lo = (lo.0.min(q.0), lo.1.min(q.1));
            hi = (hi.0.max(q.0), hi.1.max(q.1));
        }
    }
    let g = grid_of(lo, hi, st.ww, st.wh, st.page.overlap_mm);
    let cells = g.cols * g.rows;
    if cells > 80 {
        TOO_MANY.fetch_add(1, Relaxed);
        ck!(
            seq,
            matches!(
                result,
                Err(HpglError::Tiling(ExportError::TooManyPages { .. }))
            ),
            "{cells} pages : {result:?}"
        );
        return;
    }
    let t = match result {
        Ok(t) => t,
        Err(e) => panic!("séquence {seq} : refus inattendu {e:?} ({cells} pages)"),
    };
    let save = st.page.assembly.save_paper;
    if save {
        SAVED.fetch_add(1, Relaxed);
    }
    if st.page.assembly.marks {
        MARKED.fetch_add(1, Relaxed);
    }
    if !sc.is_identity() {
        SCALED.fetch_add(1, Relaxed);
    }

    // (D) mêmes pages que le PDF et que la grille de l'oracle.
    let tiles = if save {
        plan_tiles_unrotated(&scene, &st.page)
    } else {
        plan_tiles_with(&scene, &st.page)
    }
    .expect("plan");
    ck!(
        seq,
        t.pages.len() == tiles.len(),
        "{} pages, le PDF en a {}",
        t.pages.len(),
        tiles.len()
    );
    if !save {
        ck!(
            seq,
            t.pages.len() == cells,
            "{} pages, grille {cells}",
            t.pages.len()
        );
    } else {
        ck!(seq, t.pages.len() <= cells, "plus de pages que de cases");
    }
    for (k, (p, tile)) in t.pages.iter().zip(&tiles).enumerate() {
        ck!(
            seq,
            (p.col, p.row, p.number, p.total) == (tile.col, tile.row, k + 1, tiles.len()),
            "page {k} : ({}, {}, {}, {}) ≠ PDF ({}, {})",
            p.col,
            p.row,
            p.number,
            p.total,
            tile.col,
            tile.row
        );
        if !save {
            ck!(
                seq,
                (p.row, p.col) == (k / g.cols, k % g.cols),
                "ordre des pages"
            );
        }
    }
    // (G) une seule page : l'export non tuilé, octet pour octet (sinon le numéro de page).
    if t.pages.len() == 1 {
        SINGLE.fetch_add(1, Relaxed);
        match export_hpgl_with(&scene, &st.page, options) {
            Ok(p) => ck!(
                seq,
                t.pages[0].text == p.text,
                "page unique ≠ export non tuilé"
            ),
            Err(_) => {
                let r = read_hpgl(&t.pages[0].text).expect("lecture");
                ck!(
                    seq,
                    r.labels.iter().any(|l| l.text == "1/1 L1C1"),
                    "numéro 1/1"
                );
            }
        }
        return;
    }
    MULTI.fetch_add(1, Relaxed);
    // Le plan d'assemblage n'existe pas en HPGL : remarque quand plusieurs pages sont écrites.
    ck!(
        seq,
        t.plan_ignored == st.page.assembly.plan,
        "remarque du plan"
    );

    // Recomposition : pages replacées dans le repère de la feuille (mm papier) par translation.
    let (max_x, max_y) = (
        (st.ww * 40.0 + 1e-9).floor() as i64,
        (st.wh * 40.0 + 1e-9).floor() as i64,
    );
    let mut glued: Vec<Seg> = Vec::new();
    let mut total_strokes = 0;
    for p in &t.pages {
        ck!(seq, p.text.is_ascii(), "page non ASCII");
        let r = read_hpgl(&p.text).unwrap_or_else(|e| panic!("séquence {seq} : lecture : {e}"));
        let (wx, wy) = g.wmin(p.col, p.row);
        // (E) numéro de page : plume 7, début (0 ; wh + 2,5 mm), texte « n/N Lr Cc ».
        ck!(seq, r.labels.len() == 1, "{} étiquettes", r.labels.len());
        let l = &r.labels[0];
        ck!(
            seq,
            l.text == format!("{}/{} L{}C{}", p.number, p.total, p.row + 1, p.col + 1)
                && l.pen == 7
                && l.at.0 == 0
                && (l.at.1 as f64 - (st.wh + 2.5) * 40.0).abs() <= 0.5,
            "numéro de page {l:?}"
        );
        let mut crosses = 0;
        for s in &r.strokes {
            ck!(seq, s.pts.len() >= 2, "trait d'un point");
            for q in &s.pts {
                // (C) dans la fenêtre de la page.
                ck!(
                    seq,
                    (0..=max_x).contains(&q.0) && (0..=max_y).contains(&q.1),
                    "point {q:?} hors de la fenêtre {max_x} × {max_y}"
                );
            }
            if s.pen == 8 && st.page.assembly.marks {
                crosses += s.pts.len() - 1;
            }
            total_strokes += 1;
            for w in s.pts.windows(2) {
                let f = |q: (i64, i64)| (wx + q.0 as f64 / 40.0, wy + q.1 as f64 / 40.0);
                glued.push((s.pen, f(w[0]), f(w[1])));
            }
        }
        // (F) repères : 8 segments (4 croix) par bande de recouvrement qui touche une voisine.
        if st.page.assembly.marks && !save {
            let has = |dc: isize, dr: isize| {
                let (c, r) = (p.col as isize + dc, p.row as isize + dr);
                c >= 0 && r >= 0 && (c as usize) < g.cols && (r as usize) < g.rows
            };
            let bands = [(1, 0), (-1, 0), (0, 1), (0, -1)]
                .iter()
                .filter(|(dc, dr)| has(*dc, *dr))
                .count();
            ck!(
                seq,
                crosses == 8 * bands,
                "{crosses} segments de croix, {bands} bandes"
            );
        }
    }
    ck!(seq, total_strokes > 0, "aucun tracé");

    // (A) aucun segment recollé hors du dessin d'origine.
    for s in &glued {
        let marks_only = s.0 == 8 && st.page.assembly.marks;
        if marks_only {
            continue;
        }
        let mid = ((s.1 .0 + s.2 .0) / 2.0, (s.1 .1 + s.2 .1) / 2.0);
        for q in [s.1, s.2, mid] {
            ck!(
                seq,
                near(q, &scaled, s.0),
                "segment inventé près de {q:?} (plume {})",
                s.0
            );
        }
    }
    // (B) chaque point d'échantillon du dessin d'origine est couvert.
    let mut cut = false;
    for s in &scaled {
        let len = (s.2 .0 - s.1 .0).hypot(s.2 .1 - s.1 .1);
        if len < 0.2 {
            continue;
        }
        let n = (len / 0.4).ceil() as usize;
        for i in 0..=n {
            let u = i as f64 / n as f64;
            let q = (
                s.1 .0 + u * (s.2 .0 - s.1 .0),
                s.1 .1 + u * (s.2 .1 - s.1 .1),
            );
            ck!(
                seq,
                near(q, &glued, s.0),
                "point {q:?} (plume {}) non couvert",
                s.0
            );
        }
        // Trait coupé par au moins une jonction : recollé en plusieurs morceaux.
        let pieces = glued
            .iter()
            .filter(|g2| {
                g2.0 == s.0
                    && dist_point_segment(g2.1, s.1, s.2) <= TOL
                    && dist_point_segment(g2.2, s.1, s.2) <= TOL
            })
            .count();
        cut |= pieces > 1;
    }
    if cut {
        CLIPPED.fetch_add(1, Relaxed);
    }
}

#[test]
fn campagne_adverse_hpgl_tuile() {
    for seq in 0..SEQUENCES {
        run(seq);
    }
    // La campagne a réellement exercé chaque cas (elle n'est pas vide de sens).
    assert!(
        MULTI.load(Relaxed) >= 60,
        "pages multiples : {}",
        MULTI.load(Relaxed)
    );
    assert!(
        SINGLE.load(Relaxed) >= 5,
        "page unique : {}",
        SINGLE.load(Relaxed)
    );
    assert!(
        ROLLS.load(Relaxed) >= 5,
        "rouleaux : {}",
        ROLLS.load(Relaxed)
    );
    assert!(
        MARKED.load(Relaxed) >= 15,
        "repères : {}",
        MARKED.load(Relaxed)
    );
    assert!(
        SAVED.load(Relaxed) >= 15,
        "économiser : {}",
        SAVED.load(Relaxed)
    );
    assert!(
        SCALED.load(Relaxed) >= 15,
        "facteurs : {}",
        SCALED.load(Relaxed)
    );
    assert!(
        TOO_MANY.load(Relaxed) >= 3,
        "trop de pages : {}",
        TOO_MANY.load(Relaxed)
    );
    assert!(
        INVALID.load(Relaxed) >= 15,
        "invalides : {}",
        INVALID.load(Relaxed)
    );
    assert!(
        CLIPPED.load(Relaxed) >= 40,
        "traits coupés : {}",
        CLIPPED.load(Relaxed)
    );
}
