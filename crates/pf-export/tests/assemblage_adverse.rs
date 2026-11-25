//! US-13.2 — campagne adverse de l'assemblage de pages : 240 séquences à graine fixe qui
//! combinent format, orientation, marges, recouvrement et options (repères, plan, économie de
//! papier) sur des scènes variées (rectangles, segments, cercles, Béziers, blocs éloignés,
//! pièces longues, droit-fil de toutes tolérances), jusqu'à deux feuilles par export.
//!
//! L'oracle est écrit ici, sans le code d'export : la grille est recalculée, l'occupation des
//! cases est obtenue en échantillonnant la géométrie tous les 0,1 mm (cases « sûrement
//! occupées » / « peut-être occupées » pour absorber les cas limites d'arrondi), les PDF sont
//! relus par un lecteur indépendant. Invariants vérifiés à chaque séquence : jamais de panique,
//! échelle 1:1 (carré étalon de 100 mm, longueurs de la scène à 1e-3 mm), repères de pages
//! voisines cohérents, aucune page vide avec « économiser », nombre de pages prévu, déterminisme
//! à l'octet, erreurs propres hors bornes. Rien ne remplace l'essai sur papier (AT-09).

mod common;

use common::*;
use pf_export::pdf::{export_pdf_with, page_count_with, plan_tiles_with};
use pf_export::{
    AssemblyOptions, ExportError, GrainLimit, Margins, Orientation, PageFormat, PageSetup,
    PageSetupError, PathGeom, Scene,
};
use pf_geometry::{CircleTrace, CubicBezier};

const SEQUENCES: u64 = 240;
/// Pages d'étalon au plus (légende en pages de suite sur les petits formats).
const OVERHEAD_MAX_ETALON: usize = 4;

fn near(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

/// Valeur de réglage tantôt ordinaire, tantôt piège.
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

struct Case {
    setup: PageSetup,
    scenes: Vec<Scene>,
    /// Réglage valide d'après l'oracle (bornes recalculées ici).
    valid: bool,
    /// Fenêtre de patron.
    ww: f64,
    wh: f64,
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
        _ => {
            // Diagonale seule : beaucoup de cases vides.
            vec![PathGeom::Line(
                pt(ox, oy),
                pt(ox + rng.range(200.0, 1500.0), oy + rng.range(200.0, 1500.0)),
            )]
        }
    }
}

fn random_case(seq: u64) -> Case {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15 ^ seq.wrapping_mul(0xD1B5_4A32_D192_ED03) ^ 0x2545);
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
    let setup = PageSetup {
        format,
        orientation,
        margins,
        overlap_mm,
        assembly,
        ..PageSetup::default()
    };
    // Validité d'après l'oracle : bornes recalculées ici.
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
    let (ww, wh) = (
        pw - margins.left_mm - margins.right_mm,
        ph - margins.top_mm - margins.bottom_mm - 8.0,
    );
    let valid = margins_ok && overlap_ok && sides_ok && ww >= 160.0 && wh + 8.0 >= 170.0;

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
            tolerance_deg: 90.0,
        }],
        vec![GrainLimit {
            oriented: true,
            tolerance_deg: 180.0,
        }],
        vec![
            GrainLimit {
                oriented: false,
                tolerance_deg: 90.0,
            },
            GrainLimit {
                oriented: true,
                tolerance_deg: 45.0,
            },
        ],
    ];
    let n_scenes = 1 + usize::from(rng.below(4) == 0); // une feuille, parfois deux
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
        ww,
        wh,
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
}

fn hdr(p: &Page) -> Option<Hdr> {
    let s = p.strings.iter().find(|s| s.contains("taille réelle"))?;
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

/// Rotation permise par le droit-fil, formule recalculée ici : déviation d'un axe dans
/// `[0 ; 90]`, d'une ligne orientée dans `[0 ; 180]`.
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

/// Oracle d'une feuille pour un quart de tour : grille et cases sûres / possibles.
struct Candidate {
    turns: u8,
    grid: Grid,
    sure: Vec<bool>,
    maybe: Vec<bool>,
}

fn candidates(case: &Case, scene: &Scene) -> Vec<Candidate> {
    let geoms: Vec<PathGeom> = scene.paths.iter().map(|p| p.geom.clone()).collect();
    let samples = sample_paths(&geoms, 0.1);
    let mut out = Vec::new();
    for turns in 0..4u8 {
        let save = case.setup.assembly.save_paper;
        if turns > 0 && !(save && allowed(&scene.grain_limits, turns)) {
            continue;
        }
        let rotated: Vec<(f64, f64)> = samples.iter().map(|&p| rot(p, turns)).collect();
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

/// Longueurs des segments (> 5,01 mm) de la scène, triées.
fn segment_lengths(scene: &Scene) -> Vec<f64> {
    let mut v: Vec<f64> = scene
        .paths
        .iter()
        .filter_map(|p| match &p.geom {
            PathGeom::Line(a, b) => Some((b.x - a.x).hypot(b.y - a.y)),
            _ => None,
        })
        .filter(|l| *l > 5.01)
        .collect();
    v.sort_by(f64::total_cmp);
    v
}

// ---------------------------------------------------------------------------
// Campagne
// ---------------------------------------------------------------------------

#[test]
fn campagne_adverse_de_240_sequences_d_assemblage_de_pages() {
    let (mut ok_count, mut err_count, mut rotated_count, mut omitted_count) = (0, 0, 0, 0);
    let mut too_many = 0;
    let (mut marks_checked, mut plans_checked) = (0, 0);
    for seq in 0..SEQUENCES {
        let case = random_case(seq);
        let tag = format!("séquence {seq}");
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
            // Le motif du refus est l'une des erreurs de réglage (jamais une panique ni un
            // refus de pages).
            assert!(
                matches!(
                    e,
                    ExportError::Page(
                        PageSetupError::NotFinite(_)
                            | PageSetupError::SideOutOfRange { .. }
                            | PageSetupError::MarginOutOfRange { .. }
                            | PageSetupError::PrintableTooSmall { .. }
                            | PageSetupError::OverlapOutOfRange { .. }
                    )
                ),
                "{tag} : {e:?}"
            );
            err_count += 1;
            continue;
        }

        // --- Réglage valide : prévision indépendante du nombre de pages.
        let per_scene: Vec<Vec<Candidate>> =
            case.scenes.iter().map(|s| candidates(&case, s)).collect();
        let plan_extra = usize::from(case.setup.assembly.plan);
        // Meilleur et pire nombre de pages de patron possibles (cas limites compris).
        let (mut min_tiles, mut max_tiles) = (0, 0);
        for cands in &per_scene {
            let lo = cands.iter().map(|c| count(&c.sure)).min().unwrap();
            let hi = cands.iter().map(|c| count(&c.maybe)).min().unwrap();
            min_tiles += lo;
            max_tiles += hi;
        }
        // Une grille plus grande que 2000 cases est refusée sans examen (avec économie).
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
                // Refus légitime seulement si même le meilleur cas dépasse le maximum.
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
        // Un export accepté respecte le maximum de pages, étalon et plan compris.
        assert!(out.page_count <= 80, "{tag}");
        assert!(out.page_count > min_tiles + plan_extra, "{tag}");
        assert!(
            out.page_count <= max_tiles + plan_extra + OVERHEAD_MAX_ETALON,
            "{tag} : {} pages au lieu d'au plus {}",
            out.page_count,
            max_tiles + plan_extra + OVERHEAD_MAX_ETALON
        );
        ok_count += 1;

        // --- Déterminisme, décompte et structure.
        let again = export_pdf_with(&case.scenes, &case.setup).unwrap();
        assert_eq!(out, again, "{tag} : export non déterministe");
        assert_eq!(counted.unwrap(), out.page_count, "{tag}");
        let pages = read_pdf(&out.bytes);
        assert_eq!(pages.len(), out.page_count, "{tag}");
        assert_eq!(out.page_sizes_mm.len(), out.page_count, "{tag}");

        // --- Étalon : 1:1, carré de 100 mm.
        assert!(pages[0].has("page étalon"), "{tag}");
        assert!(
            pages[0]
                .rects
                .iter()
                .any(|r| near(r.2, 100.0, 1e-2) && near(r.3, 100.0, 1e-2)),
            "{tag} : carré étalon absent"
        );
        let first_tile = pages.iter().position(|p| hdr(p).is_some()).unwrap();
        let etalon_pages = pages[..first_tile]
            .iter()
            .filter(|p| p.has("page étalon"))
            .count();
        assert!((1..=OVERHEAD_MAX_ETALON).contains(&etalon_pages), "{tag}");
        assert_eq!(
            first_tile,
            etalon_pages + plan_extra,
            "{tag} : étalon + plan avant les pages de patron"
        );
        if plan_extra == 1 {
            let plan = &pages[etalon_pages];
            assert!(plan.has("Plan d'assemblage des pages"), "{tag}");
            assert!(plan.has("pas à l'échelle 1:1"), "{tag}");
            assert!(pages[..etalon_pages]
                .iter()
                .all(|p| !p.has("Plan d'assemblage")));
            plans_checked += 1;
        } else {
            assert!(pages.iter().all(|p| !p.has("Plan d'assemblage")), "{tag}");
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
            // Pages contiguës, numérotées dans l'ordre du fichier.
            for (page, h) in &mine {
                assert_eq!(h.n, first_tile + k + 1, "{tag} : numéro de page");
                assert_eq!(h.total, out.page_count, "{tag}");
                let _ = page;
                k += 1;
            }
            // Quart de tour retenu : unique pour la feuille, permis par le droit-fil.
            let turns = mine[0].1.turns;
            assert!(mine.iter().all(|(_, h)| h.turns == turns), "{tag}");
            let cand = cands
                .iter()
                .find(|c| c.turns == turns)
                .unwrap_or_else(|| panic!("{tag} : rotation {turns} non permise"));
            if !case.setup.assembly.save_paper {
                assert_eq!(turns, 0, "{tag}");
            }
            if turns > 0 {
                assert!(
                    allowed(&scene.grain_limits, turns),
                    "{tag} : droit-fil violé"
                );
                rotated_count += 1;
            }
            let g = &cand.grid;
            for (_, h) in &mine {
                assert_eq!(
                    (h.cols, h.rows),
                    (g.cols, g.rows),
                    "{tag} : grille d'origine"
                );
                assert!(h.row < g.rows && h.col < g.cols, "{tag}");
                // Pas de page vide ; toute case sûrement occupée est présente.
                assert!(
                    cand.maybe[h.row * g.cols + h.col],
                    "{tag} {sheet} L{}C{} : page vide conservée",
                    h.row + 1,
                    h.col + 1
                );
            }
            let kept: Vec<usize> = mine.iter().map(|(_, h)| h.row * g.cols + h.col).collect();
            for (i, s) in cand.sure.iter().enumerate() {
                assert!(
                    !*s || kept.contains(&i),
                    "{tag} {sheet} : case occupée {i} omise"
                );
            }
            let mut sorted = kept.clone();
            sorted.sort_unstable();
            sorted.dedup();
            assert_eq!(sorted, kept, "{tag} : ordre ligne par ligne, sans doublon");
            if case.setup.assembly.save_paper {
                if kept.len() < g.cols * g.rows {
                    omitted_count += 1;
                }
                // Le choix minimise le nombre de pages parmi les rotations permises.
                for other in cands.iter().filter(|c| c.turns != turns) {
                    assert!(
                        count(&other.maybe) >= kept.len(),
                        "{tag} {sheet} : la rotation {} donnait {} pages < {}",
                        other.turns,
                        count(&other.maybe),
                        kept.len()
                    );
                }
            } else {
                assert_eq!(kept.len(), g.cols * g.rows, "{tag} : grille complète");
            }

            // 1:1 : toutes les longueurs de segments de la scène, sur chaque page.
            let want = segment_lengths(scene);
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
            }

            // Repères : cohérence entre pages voisines.
            let (ml, mb) = (case.setup.margins.left_mm, case.setup.margins.bottom_mm);
            let ov = case.setup.overlap_mm;
            let frame = |h: &Hdr, p: (f64, f64)| {
                let w = g.wmin(h.col, h.row);
                (p.0 - ml + w.0, p.1 - mb + w.1)
            };
            let page_of = |r: usize, c: usize| mine.iter().find(|(_, h)| h.row == r && h.col == c);
            for (page, h) in &mine {
                let found = crosses(page);
                if !case.setup.assembly.marks {
                    assert!(found.is_empty(), "{tag} : croix sans l'option");
                    assert!(
                        page.strings.iter().all(|s| !s.contains(" p.")),
                        "{tag} : voisines sans l'option"
                    );
                    continue;
                }
                marks_checked += 1;
                // Libellés des voisines : seulement des pages qui existent, avec leur rang.
                for s in page.strings.iter().filter(|s| s.contains(" p.")) {
                    let (id, rank) = s.split_once(" p.").unwrap();
                    let (r, c) = id.strip_prefix('L').unwrap().split_once('C').unwrap();
                    let (r, c): (usize, usize) = (r.parse().unwrap(), c.parse().unwrap());
                    let n = page_of(r - 1, c - 1)
                        .unwrap_or_else(|| panic!("{tag} : voisine {id} absente"))
                        .1
                        .n;
                    assert_eq!(rank.parse::<usize>().unwrap(), n, "{tag} : rang de {id}");
                    assert!(
                        (r.abs_diff(h.row + 1) + c.abs_diff(h.col + 1)) == 1,
                        "{tag} : {id} n'est pas voisine"
                    );
                }
                // Voisine de droite : croix des bandes verticales superposables.
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
                        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
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
                // Voisine du dessous.
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
                        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
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
        "campagne : {SEQUENCES} séquences, {ok_count} abouties, {err_count} réglages refusés, {too_many} refus de pages, \
         {rotated_count} feuilles tournées, {omitted_count} avec pages omises, \
         {marks_checked} pages à repères, {plans_checked} plans"
    );

    // La campagne a réellement exercé chaque chemin.
    assert!(ok_count >= 100, "{ok_count} séquences abouties");
    assert!(err_count >= 20, "{err_count} refus");
    assert!(rotated_count >= 10, "{rotated_count} rotations");
    assert!(
        omitted_count >= 30,
        "{omitted_count} feuilles avec pages omises"
    );
    assert!(marks_checked >= 150, "{marks_checked} pages avec repères");
    assert!(plans_checked >= 40, "{plans_checked} plans");
}
