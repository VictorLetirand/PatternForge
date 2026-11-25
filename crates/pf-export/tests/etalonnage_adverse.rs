//! US-13.3 — campagne adverse de l'étalonnage : 180 séquences à graine fixe qui combinent
//! format, orientation, marges, recouvrement, options d'assemblage (repères, plan, économie de
//! papier), étalon renforcé et facteurs X / Y (bornes comprises, valeurs hors bornes, NaN,
//! infinis) sur des scènes variées, jusqu'à deux feuilles par export.
//!
//! L'oracle est écrit ici, sans le code d'export : la grille de pages est recalculée sur
//! l'emprise **corrigée** (quart de tour puis facteurs, dans les axes du papier), l'occupation
//! des cases est obtenue en échantillonnant la géométrie tous les 0,1 mm, la longueur attendue
//! de chaque segment est `hypot(dx · fx, dy · fy)` après rotation, les PDF sont relus par un
//! lecteur indépendant. Invariants vérifiés à chaque séquence : jamais de panique, longueurs
//! rendues = longueurs × facteur (à 1e-3 mm), carré étalon corrigé, règles et repères de
//! contrôle non corrigés, repères de pages voisines cohérents, nombre de pages prévu, pas de
//! page vide avec « économiser », déterminisme à l'octet, erreurs propres hors bornes.
//! Rien de ceci ne remplace la mesure d'une feuille imprimée (AT-09).

mod common;

use common::*;
use pf_export::pdf::{export_pdf_with, page_count_with, plan_tiles_with};
use pf_export::{
    AssemblyOptions, ExportError, GrainLimit, Margins, Orientation, PageFormat, PageSetup,
    PageSetupError, PathGeom, PrintScale, Scene,
};
use pf_geometry::{CircleTrace, CubicBezier};

const SEQUENCES: u64 = 180;
const OVERHEAD_MAX_ETALON: usize = 4;

fn near(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

fn weird(rng: &mut Rng, lo: f64, hi: f64) -> f64 {
    if rng.below(16) != 0 {
        return rng.range(lo, hi);
    }
    match rng.below(10) {
        0 => f64::NAN,
        1 => f64::INFINITY,
        2 => f64::NEG_INFINITY,
        3 => lo - 1e-6,
        4 => hi + 1e-6,
        5 => lo,
        6 => hi,
        7 => -rng.range(0.1, 50.0),
        8 => 1e300,
        _ => rng.range(lo, hi),
    }
}

/// Facteur tantôt ordinaire, tantôt extrême, tantôt piège.
fn factor(rng: &mut Rng) -> f64 {
    if rng.below(14) == 0 {
        return match rng.below(6) {
            0 => f64::NAN,
            1 => f64::INFINITY,
            2 => 0.0,
            3 => -1.0,
            4 => 0.8999999,
            _ => 1.1000001,
        };
    }
    match rng.below(10) {
        0 => 0.9,
        1 => 1.1,
        2 | 3 => 1.0,
        _ => rng.range(0.9, 1.1),
    }
}

struct Case {
    setup: PageSetup,
    scenes: Vec<Scene>,
    valid: bool,
    scale_ok: bool,
    ww: f64,
    wh: f64,
    page_h: f64,
}

fn random_paths(rng: &mut Rng) -> Vec<PathGeom> {
    let (ox, oy) = (rng.range(-500.0, 500.0), rng.range(-500.0, 500.0));
    match rng.below(6) {
        0 => rect_at(ox, oy, rng.range(10.0, 1500.0), rng.range(10.0, 1500.0)),
        1 => (0..2 + rng.below(5))
            .map(|_| {
                PathGeom::Line(
                    pt(ox + rng.range(0.0, 1200.0), oy + rng.range(0.0, 1000.0)),
                    pt(ox + rng.range(0.0, 1200.0), oy + rng.range(0.0, 1000.0)),
                )
            })
            .collect(),
        2 => {
            let mut v = vec![PathGeom::Line(
                pt(ox, oy),
                pt(ox + rng.range(20.0, 900.0), oy + rng.range(20.0, 700.0)),
            )];
            v.push(PathGeom::Circle(
                CircleTrace::new(
                    pt(ox + rng.range(0.0, 600.0), oy + rng.range(0.0, 600.0)),
                    rng.range(10.0, 250.0),
                    0.0,
                    true,
                )
                .unwrap(),
            ));
            let p = |rng: &mut Rng| pt(ox + rng.range(0.0, 700.0), oy + rng.range(0.0, 700.0));
            v.push(PathGeom::Cubic(CubicBezier::new(
                p(rng),
                p(rng),
                p(rng),
                p(rng),
            )));
            v
        }
        3 => {
            let (w, h) = (rng.range(300.0, 1800.0), rng.range(300.0, 1500.0));
            let mut v = rect_at(ox, oy, 80.0, 80.0);
            v.extend(rect_at(ox + w, oy + h, 80.0, 80.0));
            v
        }
        4 => {
            let (long, short) = (rng.range(300.0, 3000.0), rng.range(20.0, 120.0));
            if rng.below(2) == 0 {
                rect_at(ox, oy, long, short)
            } else {
                rect_at(ox, oy, short, long)
            }
        }
        _ => vec![PathGeom::Line(
            pt(ox, oy),
            pt(ox + rng.range(200.0, 1500.0), oy + rng.range(200.0, 1500.0)),
        )],
    }
}

fn random_case(seq: u64) -> Case {
    let mut rng = Rng(0xC2B2_AE3D_27D4_EB4F ^ seq.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0x1303);
    for _ in 0..4 {
        rng.next();
    }
    let (format, pw, ph) = match rng.below(5) {
        0 => (PageFormat::A4, 210.0, 297.0),
        1 => (PageFormat::A3, 297.0, 420.0),
        2 => (PageFormat::Letter, 215.9, 279.4),
        3 => (PageFormat::A0, 841.0, 1189.0),
        _ => {
            let (w, h) = (rng.range(190.0, 900.0), rng.range(190.0, 1000.0));
            (
                PageFormat::Custom {
                    width_mm: w,
                    height_mm: h,
                },
                w,
                h,
            )
        }
    };
    let orientation = if rng.below(2) == 0 {
        Orientation::Portrait
    } else {
        Orientation::Landscape
    };
    let (pw, ph) = if orientation == Orientation::Landscape {
        (ph, pw)
    } else {
        (pw, ph)
    };
    let margins = Margins {
        top_mm: weird(&mut rng, 0.0, 40.0),
        bottom_mm: weird(&mut rng, 0.0, 40.0),
        left_mm: weird(&mut rng, 0.0, 40.0),
        right_mm: weird(&mut rng, 0.0, 40.0),
    };
    let overlap_mm = weird(&mut rng, 0.0, 50.0);
    let assembly = AssemblyOptions {
        marks: rng.below(2) == 0,
        plan: rng.below(2) == 0,
        save_paper: rng.below(3) != 0,
    };
    let scale = PrintScale {
        x: factor(&mut rng),
        y: factor(&mut rng),
    };
    let control_marks = rng.below(3) == 0;
    let setup = PageSetup {
        format,
        orientation,
        margins,
        overlap_mm,
        assembly,
        scale,
        control_marks,
    };
    let m = [
        margins.top_mm,
        margins.bottom_mm,
        margins.left_mm,
        margins.right_mm,
    ];
    let margins_ok = m.iter().all(|v| v.is_finite() && (0.0..=100.0).contains(v));
    let overlap_ok = overlap_mm.is_finite() && (0.0..=50.0).contains(&overlap_mm);
    let side_ok = |v: f64| (190.0..=5000.0).contains(&v);
    let sides_ok = side_ok(pw) && side_ok(ph);
    let scale_ok = [scale.x, scale.y]
        .iter()
        .all(|v| v.is_finite() && (0.9..=1.1).contains(v));
    let (ww, wh) = (
        pw - margins.left_mm - margins.right_mm,
        ph - margins.top_mm - margins.bottom_mm - 8.0,
    );
    let valid =
        scale_ok && margins_ok && overlap_ok && sides_ok && ww >= 160.0 && wh + 8.0 >= 170.0;

    let limits = [
        vec![],
        vec![GrainLimit {
            oriented: false,
            tolerance_deg: 0.0,
        }],
        vec![GrainLimit {
            oriented: true,
            tolerance_deg: 0.0,
        }],
        vec![GrainLimit {
            oriented: false,
            tolerance_deg: 90.0,
        }],
        vec![GrainLimit {
            oriented: true,
            tolerance_deg: 180.0,
        }],
    ];
    let n_scenes = 1 + usize::from(rng.below(4) == 0);
    let scenes = (0..n_scenes)
        .map(|k| {
            let mut s = scene_of(random_paths(&mut rng));
            s.sheet_name = format!("S{k}");
            s.grain_limits = limits[rng.below(limits.len() as u64) as usize].clone();
            s
        })
        .collect();
    Case {
        setup,
        scenes,
        valid,
        scale_ok,
        ww,
        wh,
        page_h: ph,
    }
}

// ---------------------------------------------------------------------------
// Lecture des pages
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct Hdr {
    sheet: String,
    row: usize,
    col: usize,
    rows: usize,
    cols: usize,
    n: usize,
    total: usize,
    turns: u8,
    corrected: bool,
}

fn hdr(p: &Page) -> Option<Hdr> {
    let s = p.strings.iter().find(|s| s.contains(" · page "))?;
    let parts: Vec<&str> = s.split(" · ").collect();
    let sheet = parts[0].split(" — ").nth(1)?.to_string();
    let id = parts[1].split(' ').next()?;
    let (r, c) = id.strip_prefix('L')?.split_once('C')?;
    let (rows, cols) = parts[1].split("sur ").nth(1)?.split_once('×')?;
    let (n, total) = parts[2].strip_prefix("page ")?.split_once('/')?;
    let turns = parts
        .iter()
        .find_map(|x| x.strip_prefix("feuille tournée de "))
        .and_then(|x| x.split('°').next())
        .and_then(|x| x.parse::<u32>().ok())
        .map_or(0, |d| (d / 90) as u8);
    Some(Hdr {
        sheet,
        row: r.parse::<usize>().ok()? - 1,
        col: c.parse::<usize>().ok()? - 1,
        rows: rows.parse().ok()?,
        cols: cols.parse().ok()?,
        n: n.parse().ok()?,
        total: total.parse().ok()?,
        turns,
        corrected: parts.iter().any(|x| x.starts_with("correction d'échelle")),
    })
}

fn black_lengths(p: &Page) -> Vec<f64> {
    let mut v: Vec<f64> = p
        .lines
        .iter()
        .filter(|l| l.gray == 0.0 && l.len() > 5.01)
        .map(Line::len)
        .collect();
    v.sort_by(f64::total_cmp);
    v
}

fn crosses(p: &Page) -> Vec<(f64, f64)> {
    let mid = |l: &Line| ((l.a.0 + l.b.0) / 2.0, (l.a.1 + l.b.1) / 2.0);
    p.lines
        .iter()
        .filter(|l| near(l.gray, 0.3, 1e-9) && near(l.len(), 6.0, 0.01) && near(l.a.1, l.b.1, 1e-9))
        .map(mid)
        .collect()
}

fn control_lines(p: &Page) -> Vec<Line> {
    p.lines
        .iter()
        .copied()
        .filter(|l| near(l.gray, 0.45, 1e-9))
        .collect()
}

fn allowed(limits: &[GrainLimit], turns: u8) -> bool {
    let theta = 90.0 * f64::from(turns);
    limits.iter().all(|l| {
        let dev = if l.oriented {
            theta.min(360.0 - theta)
        } else {
            (theta % 180.0).min(180.0 - theta % 180.0)
        };
        dev <= l.tolerance_deg + 1e-9
    })
}

struct Candidate {
    turns: u8,
    grid: Grid,
    sure: Vec<bool>,
    maybe: Vec<bool>,
}

/// Oracle d'une feuille pour un quart de tour : grille de l'emprise tournée puis corrigée.
fn candidates(case: &Case, scene: &Scene) -> Vec<Candidate> {
    let geoms: Vec<PathGeom> = scene.paths.iter().map(|p| p.geom.clone()).collect();
    let samples = sample_paths(&geoms, 0.1);
    let (fx, fy) = (case.setup.scale.x, case.setup.scale.y);
    let mut out = Vec::new();
    for turns in 0..4u8 {
        let save = case.setup.assembly.save_paper;
        if turns > 0 && !(save && allowed(&scene.grain_limits, turns)) {
            continue;
        }
        let rotated: Vec<(f64, f64)> = samples
            .iter()
            .map(|&p| {
                let r = rot(p, turns);
                (r.0 * fx, r.1 * fy)
            })
            .collect();
        let (mut lo, mut hi) = ((f64::MAX, f64::MAX), (f64::MIN, f64::MIN));
        for &(x, y) in &rotated {
            lo = (lo.0.min(x), lo.1.min(y));
            hi = (hi.0.max(x), hi.1.max(y));
        }
        let grid = grid_of(lo, hi, case.ww, case.wh, case.setup.overlap_mm);
        let (sure, maybe) = if save {
            occupancy(&grid, &rotated, 0.2)
        } else {
            (
                vec![true; grid.cols * grid.rows],
                vec![true; grid.cols * grid.rows],
            )
        };
        out.push(Candidate {
            turns,
            grid,
            sure,
            maybe,
        });
    }
    out
}

fn count(v: &[bool]) -> usize {
    v.iter().filter(|b| **b).count()
}

/// Longueurs attendues (> 5,01 mm sur le papier) des segments de la scène pour un quart de tour
/// et des facteurs : `hypot(dx · fx, dy · fy)` du vecteur tourné. Calcul indépendant de l'export.
fn expected_lengths(scene: &Scene, turns: u8, fx: f64, fy: f64) -> Vec<f64> {
    let mut v: Vec<f64> = scene
        .paths
        .iter()
        .filter_map(|p| match &p.geom {
            PathGeom::Line(a, b) => {
                let d = rot((b.x - a.x, b.y - a.y), turns);
                Some((d.0 * fx).hypot(d.1 * fy))
            }
            _ => None,
        })
        .filter(|l| *l > 5.01)
        .collect();
    v.sort_by(f64::total_cmp);
    v
}

fn floor10(v: f64) -> f64 {
    (v.min(200.0) / 10.0).floor() * 10.0
}

// ---------------------------------------------------------------------------
// Campagne
// ---------------------------------------------------------------------------

#[test]
fn campagne_adverse_de_180_sequences_d_etalonnage() {
    let (mut ok_count, mut err_count, mut scale_refused, mut too_many) = (0, 0, 0, 0);
    let (mut corrected_pages, mut control_pages, mut marks_checked, mut extremes) = (0, 0, 0, 0);
    let mut rotated_count = 0;
    for seq in 0..SEQUENCES {
        let case = random_case(seq);
        let tag = format!(
            "séquence {seq} (x {:?}, y {:?})",
            case.setup.scale.x, case.setup.scale.y
        );
        let (fx, fy) = (case.setup.scale.x, case.setup.scale.y);
        let result = export_pdf_with(&case.scenes, &case.setup);
        let counted = page_count_with(&case.scenes, &case.setup);

        // --- Réglage invalide : erreur propre, rien produit.
        if !case.valid {
            let e = result.unwrap_err();
            assert!(matches!(e, ExportError::Page(_)), "{tag} : {e:?}");
            assert!(!e.to_string().is_empty(), "{tag}");
            assert_eq!(counted.unwrap_err(), e, "{tag}");
            for s in &case.scenes {
                assert!(plan_tiles_with(s, &case.setup).is_err(), "{tag}");
            }
            if !case.scale_ok {
                assert!(
                    matches!(e, ExportError::Page(PageSetupError::Scale(_))),
                    "{tag} : le facteur est contrôlé en premier : {e:?}"
                );
                scale_refused += 1;
            }
            err_count += 1;
            continue;
        }

        // --- Réglage valide : prévision indépendante du nombre de pages.
        let per_scene: Vec<Vec<Candidate>> =
            case.scenes.iter().map(|s| candidates(&case, s)).collect();
        let plan_extra = usize::from(case.setup.assembly.plan);
        let (mut min_tiles, mut max_tiles) = (0, 0);
        for cands in &per_scene {
            min_tiles += cands.iter().map(|c| count(&c.sure)).min().unwrap();
            max_tiles += cands.iter().map(|c| count(&c.maybe)).min().unwrap();
        }
        let grid_cap = if case.setup.assembly.save_paper {
            2000
        } else {
            80
        };
        let grid_too_big = per_scene
            .iter()
            .any(|cands| cands.iter().all(|c| c.grid.cols * c.grid.rows > grid_cap));

        let out = match result {
            Ok(o) => o,
            Err(e) => {
                assert!(
                    matches!(e, ExportError::TooManyPages { .. }),
                    "{tag} : erreur inattendue {e:?}"
                );
                assert!(
                    grid_too_big || min_tiles + plan_extra + 1 > 80,
                    "{tag} : refusé à tort ({min_tiles} pages + plan {plan_extra})"
                );
                assert_eq!(counted.unwrap_err(), e, "{tag}");
                too_many += 1;
                continue;
            }
        };
        assert!(!grid_too_big, "{tag} : grille trop grande acceptée");
        assert!(out.page_count <= 80, "{tag}");
        assert!(out.page_count > min_tiles + plan_extra, "{tag}");
        assert!(
            out.page_count <= max_tiles + plan_extra + OVERHEAD_MAX_ETALON,
            "{tag} : {} pages au lieu d'au plus {}",
            out.page_count,
            max_tiles + plan_extra + OVERHEAD_MAX_ETALON
        );
        ok_count += 1;
        if fx == 0.9 || fx == 1.1 || fy == 0.9 || fy == 1.1 {
            extremes += 1;
        }

        // --- Déterminisme, décompte et structure.
        let again = export_pdf_with(&case.scenes, &case.setup).unwrap();
        assert_eq!(out, again, "{tag} : export non déterministe");
        assert_eq!(counted.unwrap(), out.page_count, "{tag}");
        let pages = read_pdf(&out.bytes);
        assert_eq!(pages.len(), out.page_count, "{tag}");
        assert_eq!(out.page_sizes_mm.len(), out.page_count, "{tag}");

        // --- Étalon : carré de 100 mm corrigé ; règles de contrôle non corrigées.
        assert!(pages[0].has("page étalon"), "{tag}");
        assert!(
            pages[0]
                .rects
                .iter()
                .any(|r| near(r.2, 100.0 * fx, 1e-3) && near(r.3, 100.0 * fy, 1e-3)),
            "{tag} : carré étalon corrigé absent"
        );
        let first_tile = pages.iter().position(|p| hdr(p).is_some()).unwrap();
        let etalon_pages = pages[..first_tile]
            .iter()
            .filter(|p| p.has("page étalon"))
            .count();
        assert!((1..=OVERHEAD_MAX_ETALON).contains(&etalon_pages), "{tag}");
        assert_eq!(first_tile, etalon_pages + plan_extra, "{tag}");
        let identity = fx == 1.0 && fy == 1.0;
        let control = case.setup.control_marks || !identity;
        let text_all: String = pages.iter().map(|p| p.text.as_str()).collect();
        assert_eq!(
            text_all.contains("Correction d'échelle active"),
            !identity,
            "{tag}"
        );
        assert_eq!(text_all.contains("Règles de contrôle 0–"), control, "{tag}");
        if !identity {
            assert!(
                text_all.contains("à vérifier par mesure au réglet sur le papier"),
                "{tag}"
            );
        }
        for forbidden in ["calibrée", "calibré", "validée", "validé"] {
            assert!(!text_all.to_lowercase().contains(forbidden), "{tag}");
        }
        if control {
            // Longueurs de règle recalculées ici : horizontale = zone imprimable − 22, verticale =
            // hauteur sous le haut de page moins 36 + marge basse + 8 (200 mm au plus).
            let printable_w = case.ww;
            let ytop = case.page_h - case.setup.margins.top_mm;
            let lh = floor10(printable_w - 22.0);
            let lv = floor10(ytop - 36.0 - case.setup.margins.bottom_mm - 8.0);
            assert!(lh >= 100.0 && lv >= 100.0, "{tag}");
            let first = &pages[0];
            assert!(
                first
                    .lines
                    .iter()
                    .any(|l| near(l.a.1, l.b.1, 1e-3) && near(l.len(), lh, 1e-3)),
                "{tag} : règle horizontale de {lh} mm"
            );
            assert!(
                first
                    .lines
                    .iter()
                    .any(|l| near(l.a.0, l.b.0, 1e-3) && near(l.len(), lv, 1e-3)),
                "{tag} : règle verticale de {lv} mm"
            );
            control_pages += 1;
        }
        if !identity {
            corrected_pages += 1;
        }
        if plan_extra == 1 {
            assert!(
                pages[etalon_pages].has("Plan d'assemblage des pages"),
                "{tag}"
            );
        }

        // --- Pages de patron, feuille par feuille.
        let tiles: Vec<(&Page, Hdr)> = pages
            .iter()
            .filter_map(|p| hdr(p).map(|h| (p, h)))
            .collect();
        assert_eq!(tiles.len(), pages.len() - first_tile, "{tag}");
        let mut k = 0;
        for (si, (scene, cands)) in case.scenes.iter().zip(&per_scene).enumerate() {
            let sheet = format!("S{si}");
            let mine: Vec<&(&Page, Hdr)> = tiles.iter().filter(|(_, h)| h.sheet == sheet).collect();
            assert!(!mine.is_empty(), "{tag} {sheet} : aucune page");
            for (_, h) in &mine {
                assert_eq!(h.n, first_tile + k + 1, "{tag} : numéro de page");
                assert_eq!(h.total, out.page_count, "{tag}");
                assert_eq!(h.corrected, !identity, "{tag} : mention de la correction");
                k += 1;
            }
            let turns = mine[0].1.turns;
            assert!(mine.iter().all(|(_, h)| h.turns == turns), "{tag}");
            let cand = cands
                .iter()
                .find(|c| c.turns == turns)
                .unwrap_or_else(|| panic!("{tag} : rotation {turns} non permise"));
            if turns > 0 {
                assert!(allowed(&scene.grain_limits, turns), "{tag}");
                rotated_count += 1;
            } else if !case.setup.assembly.save_paper {
                assert_eq!(turns, 0, "{tag}");
            }
            let g = &cand.grid;
            for (_, h) in &mine {
                assert_eq!((h.cols, h.rows), (g.cols, g.rows), "{tag} : grille");
                assert!(
                    cand.maybe[h.row * g.cols + h.col],
                    "{tag} {sheet} : page vide conservée"
                );
            }
            let kept: Vec<usize> = mine.iter().map(|(_, h)| h.row * g.cols + h.col).collect();
            for (i, s) in cand.sure.iter().enumerate() {
                assert!(
                    !*s || kept.contains(&i),
                    "{tag} {sheet} : case occupée omise"
                );
            }
            if case.setup.assembly.save_paper {
                for other in cands.iter().filter(|c| c.turns != turns) {
                    assert!(count(&other.maybe) >= kept.len(), "{tag} {sheet}");
                }
            } else {
                assert_eq!(kept.len(), g.cols * g.rows, "{tag}");
            }

            // Longueurs rendues = longueurs × facteur, sur chaque page.
            let want = expected_lengths(scene, turns, fx, fy);
            for (page, h) in &mine {
                let got = black_lengths(page);
                assert_eq!(
                    got.len(),
                    want.len(),
                    "{tag} {sheet} L{}C{}",
                    h.row + 1,
                    h.col + 1
                );
                for (a, b) in got.iter().zip(&want) {
                    assert!(near(*a, *b, 1e-3), "{tag} : longueur {a} au lieu de {b}");
                }
                // Repères de contrôle : deux règles de 100 mm non corrigées, ou aucune.
                let c = control_lines(page);
                if control {
                    let hz = c
                        .iter()
                        .filter(|l| near(l.a.1, l.b.1, 1e-3) && near(l.len(), 100.0, 1e-3))
                        .count();
                    let vt = c
                        .iter()
                        .filter(|l| near(l.a.0, l.b.0, 1e-3) && near(l.len(), 100.0, 1e-3))
                        .count();
                    assert_eq!((hz, vt), (1, 1), "{tag} : repères de contrôle");
                } else {
                    assert!(c.is_empty(), "{tag} : repères de contrôle sans option");
                }
            }

            // Repères d'assemblage : cohérence entre pages voisines, dans le repère corrigé.
            let (ml, mb) = (case.setup.margins.left_mm, case.setup.margins.bottom_mm);
            let ov = case.setup.overlap_mm;
            let frame = |h: &Hdr, p: (f64, f64)| {
                let w = g.wmin(h.col, h.row);
                (p.0 - ml + w.0, p.1 - mb + w.1)
            };
            let page_of = |r: usize, c: usize| mine.iter().find(|(_, h)| h.row == r && h.col == c);
            let key = |q: &(f64, f64)| ((q.0 * 100.0).round() as i64, (q.1 * 100.0).round() as i64);
            for (page, h) in &mine {
                let found = crosses(page);
                if !case.setup.assembly.marks {
                    assert!(found.is_empty(), "{tag} : croix sans l'option");
                    continue;
                }
                marks_checked += 1;
                if let Some((other, oh)) = page_of(h.row, h.col + 1) {
                    let x_lo = g.wmin(oh.col, oh.row).0;
                    let wy = g.wmin(h.col, h.row).1;
                    let strip = |list: &[(f64, f64)], hh: &Hdr| {
                        let mut v: Vec<(f64, f64)> = list
                            .iter()
                            .map(|p| frame(hh, *p))
                            .filter(|p| {
                                p.0 >= x_lo - 1e-3
                                    && p.0 <= x_lo + ov + 1e-3
                                    && (near(p.1 - wy, 6.0, 1e-3)
                                        || near(p.1 - wy, case.wh - 6.0, 1e-3))
                            })
                            .collect();
                        v.sort_by_key(key);
                        v
                    };
                    let (a, b) = (strip(&found, h), strip(&crosses(other), oh));
                    assert_eq!(a.len(), 4, "{tag} : croix de la bande droite");
                    assert_eq!(a.len(), b.len(), "{tag}");
                    for (p, q) in a.iter().zip(&b) {
                        assert!(
                            near(p.0, q.0, 1e-3) && near(p.1, q.1, 1e-3),
                            "{tag} : {p:?} / {q:?}"
                        );
                    }
                }
                if let Some((other, oh)) = page_of(h.row + 1, h.col) {
                    let y_lo = g.wmin(h.col, h.row).1;
                    let wx = g.wmin(h.col, h.row).0;
                    let strip = |list: &[(f64, f64)], hh: &Hdr| {
                        let mut v: Vec<(f64, f64)> = list
                            .iter()
                            .map(|p| frame(hh, *p))
                            .filter(|p| {
                                p.1 >= y_lo - 1e-3
                                    && p.1 <= y_lo + ov + 1e-3
                                    && (near(p.0 - wx, 6.0, 1e-3)
                                        || near(p.0 - wx, case.ww - 6.0, 1e-3))
                            })
                            .collect();
                        v.sort_by_key(key);
                        v
                    };
                    let (a, b) = (strip(&found, h), strip(&crosses(other), oh));
                    assert_eq!(a.len(), 4, "{tag} : croix de la bande basse");
                    assert_eq!(a.len(), b.len(), "{tag}");
                    for (p, q) in a.iter().zip(&b) {
                        assert!(
                            near(p.0, q.0, 1e-3) && near(p.1, q.1, 1e-3),
                            "{tag} : {p:?} / {q:?}"
                        );
                    }
                }
            }
        }
        assert_eq!(k, tiles.len(), "{tag} : toutes les pages sont attribuées");
    }
    eprintln!(
        "campagne : {SEQUENCES} séquences, {ok_count} abouties, {err_count} réglages refusés \
         (dont {scale_refused} pour un facteur), {too_many} refus de pages, {corrected_pages} \
         corrigées, {extremes} avec un facteur extrême, {control_pages} avec règles de contrôle, \
         {rotated_count} feuilles tournées, {marks_checked} pages à repères"
    );

    assert!(ok_count >= 80, "{ok_count} séquences abouties");
    assert!(err_count >= 20, "{err_count} refus");
    assert!(scale_refused >= 10, "{scale_refused} facteurs refusés");
    assert!(corrected_pages >= 50, "{corrected_pages} corrigées");
    assert!(extremes >= 10, "{extremes} facteurs extrêmes");
    assert!(control_pages >= 50, "{control_pages} règles de contrôle");
    assert!(rotated_count >= 5, "{rotated_count} rotations");
    assert!(marks_checked >= 100, "{marks_checked} pages avec repères");
}
