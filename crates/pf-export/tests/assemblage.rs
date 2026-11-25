//! US-13.2 — assemblage de pages : recouvrement paramétrable, repères de recalage, numérotation
//! et pages voisines, plan d'assemblage, option « économiser le papier » (pages vides omises,
//! rotation d'export dans le respect du droit-fil), refus sans trace, compatibilité.
//!
//! Les PDF sont relus par le lecteur de `common` (indépendant du code d'export) et les grilles
//! sont recalculées à part. Aucun de ces tests ne remplace l'essai sur papier (AT-09, humain).

mod common;

use common::*;
use pf_core::{Command, GrainSense, PathRole, PieceGrain, PieceId, PieceSource, PointRef, Project};
use pf_export::pdf::{
    export_pdf, export_pdf_with, page_count_with, plan_tiles_with, registration_marks, Tile,
    MAX_PAGES,
};
use pf_export::{
    AssemblyOptions, ExportError, GrainLimit, PageField, PageSetup, PageSetupError, PathGeom, Scene,
};

fn setup(ov: f64, marks: bool, plan: bool, save: bool) -> PageSetup {
    PageSetup {
        overlap_mm: ov,
        assembly: AssemblyOptions {
            marks,
            plan,
            save_paper: save,
        },
        ..PageSetup::default()
    }
}

/// En-tête lu d'une page de patron : « … · L1C2 sur 2×4 · page 3/9 · 100 % taille réelle ».
#[derive(Debug, Clone, PartialEq)]
struct Header {
    row: usize,
    col: usize,
    rows: usize,
    cols: usize,
    n: usize,
    total: usize,
    turned: Option<u32>,
}

fn header(p: &Page) -> Option<Header> {
    let s = p.strings.iter().find(|s| s.contains("taille réelle"))?;
    let parts: Vec<&str> = s.split(" · ").collect();
    let id = parts.get(1)?.split(' ').next()?;
    let (r, c) = id.strip_prefix('L')?.split_once('C')?;
    let grid = parts.get(1)?.split("sur ").nth(1)?;
    let (rows, cols) = grid.split_once('×')?;
    let (n, total) = parts.get(2)?.strip_prefix("page ")?.split_once('/')?;
    let turned = parts
        .iter()
        .find_map(|x| x.strip_prefix("feuille tournée de "))
        .and_then(|x| x.split('°').next())
        .and_then(|x| x.parse().ok());
    Some(Header {
        row: r.parse::<usize>().ok()? - 1,
        col: c.parse::<usize>().ok()? - 1,
        rows: rows.trim().parse().ok()?,
        cols: cols.trim().parse().ok()?,
        n: n.parse().ok()?,
        total: total.trim().parse().ok()?,
        turned,
    })
}

fn tile_pages(pages: &[Page]) -> Vec<(&Page, Header)> {
    pages
        .iter()
        .filter_map(|p| header(p).map(|h| (p, h)))
        .collect()
}

fn black_lengths(p: &Page) -> Vec<f64> {
    let mut v: Vec<f64> = p
        .lines
        .iter()
        .filter(|l| l.gray == 0.0 && l.len() > 5.0)
        .map(Line::len)
        .collect();
    v.sort_by(f64::total_cmp);
    v
}

fn near(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

fn near_pt(a: (f64, f64), b: (f64, f64), tol: f64) -> bool {
    near(a.0, b.0, tol) && near(a.1, b.1, tol)
}

/// A4 portrait, marges de 10 mm : fenêtre de 190 × 269 mm.
const WW: f64 = 190.0;
const WH: f64 = 269.0;

/// Centres des croix de recalage d'une page (traits gris 0,3 de 6 mm : une branche
/// horizontale et une verticale de même milieu), en mm sur la page.
fn crosses(p: &Page) -> Vec<(f64, f64)> {
    let arms: Vec<&Line> = p
        .lines
        .iter()
        .filter(|l| near(l.gray, 0.3, 1e-9) && near(l.len(), 6.0, 0.01))
        .collect();
    let mid = |l: &Line| ((l.a.0 + l.b.0) / 2.0, (l.a.1 + l.b.1) / 2.0);
    let mut out = Vec::new();
    for h in arms.iter().filter(|l| near(l.a.1, l.b.1, 1e-9)) {
        let m = mid(h);
        assert!(
            arms.iter()
                .any(|v| near(v.a.0, v.b.0, 1e-9) && near_pt(mid(v), m, 1e-3)),
            "branche horizontale sans branche verticale en {m:?}"
        );
        out.push(m);
    }
    out
}

// ---------------------------------------------------------------------------
// CA1 — recouvrement paramétrable
// ---------------------------------------------------------------------------

#[test]
fn ca1_le_recouvrement_pilote_le_decoupage_les_positions_et_les_traits_de_raccord() {
    let scene = rect_scene(600.0, 450.0);
    for ov in [0.0, 5.0, 10.0, 25.0, 50.0] {
        let s = setup(ov, false, false, false);
        let tiles = plan_tiles_with(&scene, &s).unwrap();
        let (cols, rows) = (pages_to_cover(610.0, WW, ov), pages_to_cover(460.0, WH, ov));
        assert_eq!(tiles.len(), cols * rows, "recouvrement {ov}");
        for t in &tiles {
            assert_eq!((t.cols, t.rows), (cols, rows));
            assert_eq!(t.overlap_mm, ov);
            let want = (
                -5.0 + t.col as f64 * (WW - ov),
                455.0 - WH - t.row as f64 * (WH - ov),
            );
            assert!(
                near_pt((t.world_min.x, t.world_min.y), want, 1e-9),
                "{ov} : {:?} au lieu de {want:?}",
                t.world_min
            );
        }
        // Les fenêtres couvrent toute l'emprise de la pièce.
        let last = tiles.last().unwrap();
        assert!(last.world_min.x + WW >= 600.0 + 5.0 - 1e-9, "{ov}");
        assert!(last.world_min.y <= -5.0 + 1e-9, "{ov}");
        // Dans le PDF : traits de raccord à `ov` du bord, côtés non réduits.
        let out = export_pdf_with(std::slice::from_ref(&scene), &s).unwrap();
        assert_eq!(out.page_count, 1 + cols * rows);
        assert_eq!(
            page_count_with(std::slice::from_ref(&scene), &s).unwrap(),
            out.page_count
        );
        let pages = read_pdf(&out.bytes);
        for (p, h) in tile_pages(&pages) {
            let mut xs: Vec<f64> = p
                .lines
                .iter()
                .filter(|l| near(l.gray, 0.6, 1e-9) && near(l.a.0, l.b.0, 1e-9))
                .map(|l| l.a.0)
                .collect();
            xs.sort_by(f64::total_cmp);
            let mut want = Vec::new();
            if h.col > 0 {
                want.push(10.0 + ov);
            }
            if h.col + 1 < cols {
                want.push(10.0 + WW - ov);
            }
            want.sort_by(f64::total_cmp);
            assert_eq!(xs.len(), want.len(), "{ov} {h:?}");
            for (a, b) in xs.iter().zip(&want) {
                assert!(near(*a, *b, 1e-3), "{ov} {h:?} : {a} au lieu de {b}");
            }
            let mut ys: Vec<f64> = p
                .lines
                .iter()
                .filter(|l| near(l.gray, 0.6, 1e-9) && near(l.a.1, l.b.1, 1e-9))
                .map(|l| l.a.1)
                .collect();
            ys.sort_by(f64::total_cmp);
            let mut want = Vec::new();
            if h.row + 1 < rows {
                want.push(10.0 + ov);
            }
            if h.row > 0 {
                want.push(10.0 + WH - ov);
            }
            want.sort_by(f64::total_cmp);
            assert_eq!(ys.len(), want.len(), "{ov} {h:?}");
            for (a, b) in ys.iter().zip(&want) {
                assert!(near(*a, *b, 1e-3), "{ov} {h:?} : {a} au lieu de {b}");
            }
            // Échelle 1:1 : les quatre côtés gardent leur longueur.
            let lens = black_lengths(p);
            assert_eq!(lens.len(), 4);
            for (got, want) in lens.iter().zip([450.0, 450.0, 600.0, 600.0]) {
                assert!(near(*got, want, 1e-3), "{got} au lieu de {want}");
            }
        }
        // Le texte de l'étalon annonce le recouvrement réel.
        assert!(
            pages[0].has(&format!("recouvrement de {ov} mm")),
            "{ov} : {}",
            pages[0].text
        );
    }
}

#[test]
fn ca1_un_recouvrement_plus_grand_demande_au_moins_autant_de_pages() {
    // Plus le recouvrement croît, plus le pas diminue : jamais moins de pages.
    let scene = rect_scene(1500.0, 1100.0);
    let mut last = 0;
    for ov in [0.0, 10.0, 20.0, 30.0, 40.0, 50.0] {
        let n = plan_tiles_with(&scene, &setup(ov, false, false, false))
            .unwrap()
            .len();
        assert!(n >= last, "{ov} : {n} < {last}");
        last = n;
    }
}

// ---------------------------------------------------------------------------
// CA2 — repères d'assemblage
// ---------------------------------------------------------------------------

/// Repères d'une bande verticale (voisin de droite) : ceux qui sont à 6 mm des bords haut et bas
/// de la fenêtre, dans la bande, pour la page `t` (repère de la feuille).
fn vertical_strip_marks(t: &Tile, all: &[Tile], x_lo: f64) -> Vec<(f64, f64)> {
    let ov = t.overlap_mm;
    let mut v: Vec<(f64, f64)> = registration_marks(t, all)
        .iter()
        .filter(|m| {
            m.x >= x_lo - 1e-9
                && m.x <= x_lo + ov + 1e-9
                && (near(m.y - t.world_min.y, 6.0, 1e-9)
                    || near(m.y - t.world_min.y, t.layout.window_h_mm - 6.0, 1e-9))
        })
        .map(|m| (m.x, m.y))
        .collect();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v
}

#[test]
fn ca2_les_reperes_de_pages_voisines_designent_les_memes_points() {
    let scene = rect_scene(600.0, 450.0);
    for ov in [10.0, 20.0, 35.0] {
        let s = setup(ov, true, false, false);
        let tiles = plan_tiles_with(&scene, &s).unwrap();
        let (cols, rows) = (tiles[0].cols, tiles[0].rows);
        assert!(cols >= 2 && rows >= 2);
        let at = |c: usize, r: usize| tiles.iter().find(|t| t.col == c && t.row == r).unwrap();
        for r in 0..rows {
            for c in 0..cols - 1 {
                let (a, b) = (at(c, r), at(c + 1, r));
                let x_lo = b.world_min.x; // bord gauche de la page de droite = début de la bande
                let (ma, mb) = (
                    vertical_strip_marks(a, &tiles, x_lo),
                    vertical_strip_marks(b, &tiles, x_lo),
                );
                assert_eq!(ma.len(), 4, "{ov} ({c},{r})");
                assert_eq!(ma.len(), mb.len());
                for (p, q) in ma.iter().zip(&mb) {
                    assert!(near_pt(*p, *q, 1e-6), "{ov} ({c},{r}) : {p:?} / {q:?}");
                }
                // Les repères sont dans la bande : coins de la zone de recouvrement.
                for p in &ma {
                    assert!(p.0 > x_lo && p.0 < x_lo + ov, "{p:?} hors de la bande");
                }
            }
        }
        // Même chose pour les bandes horizontales (voisin du dessous).
        for r in 0..rows - 1 {
            for c in 0..cols {
                let (a, b) = (at(c, r), at(c, r + 1));
                let strip = |t: &Tile| -> Vec<(f64, f64)> {
                    let mut v: Vec<(f64, f64)> = registration_marks(t, &tiles)
                        .iter()
                        .filter(|m| {
                            m.y >= a.world_min.y - 1e-9
                                && m.y <= a.world_min.y + ov + 1e-9
                                && (near(m.x - t.world_min.x, 6.0, 1e-9)
                                    || near(m.x - t.world_min.x, WW - 6.0, 1e-9))
                        })
                        .map(|m| (m.x, m.y))
                        .collect();
                    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
                    v
                };
                let (ma, mb) = (strip(a), strip(b));
                assert_eq!(ma.len(), 4, "{ov} ({c},{r}) bas");
                for (p, q) in ma.iter().zip(&mb) {
                    assert!(near_pt(*p, *q, 1e-6), "{ov} ({c},{r}) : {p:?} / {q:?}");
                }
            }
        }
    }
}

#[test]
fn ca2_les_croix_dessinees_dans_le_pdf_sont_aux_positions_calculees_et_se_recalent() {
    let scene = rect_scene(600.0, 450.0);
    let s = setup(10.0, true, false, false);
    let tiles = plan_tiles_with(&scene, &s).unwrap();
    let (cols, rows) = (tiles[0].cols, tiles[0].rows);
    let out = export_pdf_with(std::slice::from_ref(&scene), &s).unwrap();
    let pages = read_pdf(&out.bytes);
    assert_eq!(pages.len(), 1 + cols * rows);
    // Oracle : grille recalculée ici (blanc de 5 mm, fenêtres 190 × 269, pas 180 × 259).
    let g = grid_of((0.0, 0.0), (600.0, 450.0), WW, WH, 10.0);
    assert_eq!((g.cols, g.rows), (cols, rows));
    let to_frame = |col: usize, row: usize, p: (f64, f64)| {
        let w = g.wmin(col, row);
        (p.0 - 10.0 + w.0, p.1 - 10.0 + w.1)
    };
    for t in &tiles {
        let page = &pages[1 + t.row * cols + t.col];
        let found = crosses(page);
        let expect = registration_marks(t, &tiles);
        assert_eq!(
            found.len(),
            expect.len(),
            "page L{}C{}",
            t.row + 1,
            t.col + 1
        );
        for m in &expect {
            let q = t.frame_to_page_mm(*m);
            assert!(
                found.iter().any(|f| near_pt(*f, (q.x, q.y), 1e-3)),
                "croix manquante en {q:?}"
            );
        }
    }
    // Recalage : les croix de la bande commune se superposent (positions en repère feuille).
    for r in 0..rows {
        for c in 0..cols - 1 {
            let a = crosses(&pages[1 + r * cols + c]);
            let b = crosses(&pages[1 + r * cols + c + 1]);
            let x_lo = g.wmin(c + 1, r).0;
            let wy = g.wmin(c, r).1;
            let strip = |list: &[(f64, f64)], col: usize| -> Vec<(f64, f64)> {
                let mut v: Vec<(f64, f64)> = list
                    .iter()
                    .map(|p| to_frame(col, r, *p))
                    .filter(|p| {
                        p.0 >= x_lo - 1e-3
                            && p.0 <= x_lo + 10.0 + 1e-3
                            && (near(p.1 - wy, 6.0, 1e-3) || near(p.1 - wy, WH - 6.0, 1e-3))
                    })
                    .collect();
                v.sort_by(|a, b| a.partial_cmp(b).unwrap());
                v
            };
            let (sa, sb) = (strip(&a, c), strip(&b, c + 1));
            assert_eq!(sa.len(), 4, "({c},{r})");
            assert_eq!(sb.len(), 4, "({c},{r})");
            for (p, q) in sa.iter().zip(&sb) {
                // Le PDF arrondit à 1e-4 pt (≈ 3,5e-5 mm) : on compare à 1e-3 mm.
                assert!(near_pt(*p, *q, 1e-3), "({c},{r}) : {p:?} / {q:?}");
            }
        }
    }
    // Sans l'option : aucune croix.
    let plain = export_pdf(std::slice::from_ref(&scene)).unwrap();
    assert!(read_pdf(&plain.bytes).iter().all(|p| crosses(p).is_empty()));
}

// ---------------------------------------------------------------------------
// CA3 — numérotation et pages voisines
// ---------------------------------------------------------------------------

#[test]
fn ca3_chaque_page_porte_son_identifiant_son_rang_et_ses_voisines_au_bord() {
    let scene = rect_scene(600.0, 450.0);
    let with = export_pdf_with(
        std::slice::from_ref(&scene),
        &setup(10.0, true, false, false),
    )
    .unwrap();
    let without = export_pdf(std::slice::from_ref(&scene)).unwrap();
    for (out, marks) in [(&with, true), (&without, false)] {
        let pages = read_pdf(&out.bytes);
        let tp = tile_pages(&pages);
        assert_eq!(tp.len(), 8);
        for (k, (p, h)) in tp.iter().enumerate() {
            // Identifiant L/C, « page n/N » et rang dans le fichier (étalon = page 1).
            assert_eq!((h.cols, h.rows, h.total), (4, 2, 9));
            assert_eq!(h.n, k + 2, "page n/N");
            assert_eq!((h.row, h.col), (k / 4, k % 4), "ordre ligne par ligne");
            // Pages voisines : identifiant et numéro de page du fichier.
            let neighbours = [
                (h.col + 1 < 4, h.row, h.col + 1),
                (h.col > 0, h.row, h.col.wrapping_sub(1)),
                (h.row > 0, h.row.wrapping_sub(1), h.col),
                (h.row + 1 < 2, h.row + 1, h.col),
            ];
            for (exists, r, c) in neighbours {
                if !exists {
                    continue;
                }
                let label = format!("L{}C{} p.{}", r + 1, c + 1, 2 + r * 4 + c);
                assert_eq!(p.has(&label), marks, "page {} : « {label} »", h.n);
            }
            // Jamais l'identifiant d'une page qui n'est pas voisine (ni de soi-même).
            if marks {
                let own = format!("L{}C{} p.", h.row + 1, h.col + 1);
                assert!(!p.has(&own), "page {} se désigne elle-même", h.n);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// CA4 — plan d'assemblage
// ---------------------------------------------------------------------------

#[test]
fn ca4_le_plan_vient_apres_l_etalon_n_est_pas_a_l_echelle_et_les_pages_restent_1_pour_1() {
    let scene = rect_scene(600.0, 450.0);
    let s = setup(10.0, true, true, false);
    let out = export_pdf_with(std::slice::from_ref(&scene), &s).unwrap();
    let base = export_pdf(std::slice::from_ref(&scene)).unwrap();
    assert_eq!(out.page_count, base.page_count + 1);
    assert_eq!(
        page_count_with(std::slice::from_ref(&scene), &s).unwrap(),
        out.page_count
    );
    let pages = read_pdf(&out.bytes);
    assert_eq!(pages.len(), out.page_count);
    // Étalon (page 1) puis plan (page 2), puis les 8 pages de patron.
    assert!(pages[0].has("page étalon"));
    assert!(pages[1].has("Plan d'assemblage des pages"));
    assert!(pages[1].has("pas à l'échelle 1:1"), "{}", pages[1].text);
    assert!(pages[1].has("plan à l'échelle 1:"));
    assert!(pages[1].has("Repères : croix à faire coïncider"));
    assert_eq!(pages[1].media_mm().0.round(), 210.0);
    // Grille miniature : 8 cases pleines numérotées avec leur numéro de page de fichier.
    let cells: Vec<_> = pages[1].rects.iter().filter(|r| r.4 == 0.0).collect();
    assert_eq!(cells.len(), 8);
    for r in 0..2 {
        for c in 0..4 {
            assert!(pages[1].has(&format!("L{}C{}", r + 1, c + 1)));
            assert!(pages[1].has(&format!("p.{}", 3 + r * 4 + c)));
        }
    }
    for cell in &cells {
        // Réduite : plus petite que la fenêtre réelle, mêmes proportions (190 × 269).
        assert!(cell.2 < WW / 2.0, "case {} mm de large", cell.2);
        assert!(near(cell.2 / cell.3, WW / WH, 1e-3));
    }
    // Étalon intact : le carré fait toujours 100 mm ; pages de patron 1:1 et numérotées.
    let square = pages[0]
        .rects
        .iter()
        .find(|r| near(r.2, 100.0, 1e-3) && near(r.3, 100.0, 1e-3));
    assert!(square.is_some(), "carré étalon de 100 mm");
    let tp = tile_pages(&pages);
    assert_eq!(tp.len(), 8);
    for (k, (p, h)) in tp.iter().enumerate() {
        assert_eq!((h.n, h.total), (k + 3, 10));
        let lens = black_lengths(p);
        assert_eq!(lens.len(), 4);
        for (got, want) in lens.iter().zip([450.0, 450.0, 600.0, 600.0]) {
            assert!(near(*got, want, 1e-3), "{got} au lieu de {want}");
        }
        assert!(p.has("100 % taille réelle"));
    }
    // Les repères d'une page désignent le numéro de page de fichier **avec** le plan.
    assert!(tp[0].0.has("L1C2 p.4"), "{}", tp[0].0.text);
    // Sans l'option : pas de page de plan.
    assert!(read_pdf(&base.bytes)
        .iter()
        .all(|p| !p.has("Plan d'assemblage")));
}

#[test]
fn ca4_la_page_de_plan_compte_dans_le_maximum_de_pages() {
    // 79 pages de patron + l'étalon = 80 : accepté ; le plan en ajoute une : refusé.
    let scene = rect_scene(14_220.0, 100.0);
    let plain = PageSetup::default();
    let n = page_count_with(std::slice::from_ref(&scene), &plain).unwrap();
    assert_eq!(n, MAX_PAGES);
    let err = page_count_with(
        std::slice::from_ref(&scene),
        &setup(10.0, false, true, false),
    )
    .unwrap_err();
    assert_eq!(
        err,
        ExportError::TooManyPages {
            pages: MAX_PAGES + 1,
            max: MAX_PAGES
        }
    );
    assert!(export_pdf_with(
        std::slice::from_ref(&scene),
        &setup(10.0, false, true, false)
    )
    .is_err());
    // 78 pages + étalon + plan = 80 : accepté jusqu'à l'écriture.
    let scene = rect_scene(2340.0, 1554.0);
    let s = setup(10.0, false, true, false);
    assert_eq!(plan_tiles_with(&scene, &s).unwrap().len(), 78);
    let out = export_pdf_with(std::slice::from_ref(&scene), &s).unwrap();
    assert_eq!(out.page_count, MAX_PAGES);
    assert_eq!(read_pdf(&out.bytes).len(), MAX_PAGES);
}

// ---------------------------------------------------------------------------
// CA5 — économiser le papier
// ---------------------------------------------------------------------------

/// Deux rectangles de 100 × 100 aux extrémités d'une diagonale de 700 × 600 : la plupart des
/// cases de la grille sont vides.
fn two_blocks() -> Scene {
    let mut paths = rect_at(0.0, 0.0, 100.0, 100.0);
    paths.extend(rect_at(600.0, 500.0, 100.0, 100.0));
    let mut s = scene_of(paths);
    // Un droit-fil strict (sens marqué, tolérance 0) interdit toute rotation : on isole ici
    // l'omission des pages vides.
    s.grain_limits = vec![GrainLimit {
        oriented: true,
        tolerance_deg: 0.0,
    }];
    s
}

#[test]
fn ca5_les_pages_vides_sont_omises_avec_la_numerotation_de_grille_d_origine() {
    let scene = two_blocks();
    let s = setup(10.0, true, true, true);
    let tiles = plan_tiles_with(&scene, &s).unwrap();
    // Grille d'origine 4 colonnes × 3 lignes ; seules L1C4, L2C1 et L3C1 portent du contenu.
    let g = grid_of((0.0, 0.0), (700.0, 600.0), WW, WH, 10.0);
    assert_eq!((g.cols, g.rows), (4, 3));
    let ids: Vec<(usize, usize)> = tiles.iter().map(|t| (t.row, t.col)).collect();
    assert_eq!(ids, vec![(0, 3), (1, 0), (2, 0)]);
    for t in &tiles {
        assert_eq!((t.cols, t.rows, t.turns), (4, 3, 0));
    }
    // Oracle d'occupation par échantillonnage (indépendant du test segment / rectangle).
    let samples = sample_paths(
        &scene
            .paths
            .iter()
            .map(|p| p.geom.clone())
            .collect::<Vec<_>>(),
        0.1,
    );
    let (sure, maybe) = occupancy(&g, &samples, 0.2);
    assert_eq!(sure, maybe, "aucun cas limite ici");
    let want: Vec<(usize, usize)> = (0..g.rows)
        .flat_map(|r| (0..g.cols).map(move |c| (r, c)))
        .filter(|&(r, c)| sure[r * g.cols + c])
        .collect();
    assert_eq!(ids, want);
    // PDF : 1 étalon + 1 plan + 3 pages ; numéros « page n/N » sur le fichier réel.
    let out = export_pdf_with(std::slice::from_ref(&scene), &s).unwrap();
    assert_eq!(out.page_count, 5);
    assert_eq!(
        page_count_with(std::slice::from_ref(&scene), &s).unwrap(),
        5
    );
    let pages = read_pdf(&out.bytes);
    let tp = tile_pages(&pages);
    let got: Vec<(usize, usize, usize, usize)> = tp
        .iter()
        .map(|(_, h)| (h.row, h.col, h.n, h.total))
        .collect();
    assert_eq!(got, vec![(0, 3, 3, 5), (1, 0, 4, 5), (2, 0, 5, 5)]);
    // Les repères ne désignent que des pages qui existent : L2C1 → L3C1 (page 5) seulement.
    assert!(tp[1].0.has("L3C1 p.5"));
    assert!(!tp[1].0.has("L1C1") && !tp[1].0.has("L2C2"));
    assert!(tp[0].0.strings.iter().all(|s| !s.contains(" p.")));
    // Le plan signale les pages omises et conserve la grille d'origine (12 cases).
    let plan = &pages[1];
    assert!(plan.has("Pages omises (9)"), "{}", plan.text);
    assert!(plan.has("L1C1") && plan.has("L1C2"));
    assert_eq!(plan.strings.iter().filter(|s| *s == "omise").count(), 9);
    assert_eq!(plan.rects.iter().filter(|r| r.4 == 0.0).count(), 3);
    assert!(pages[0].has("dont 9 omise(s)"), "{}", pages[0].text);
}

#[test]
fn ca5_une_diagonale_garde_les_cases_traversees_et_omet_les_coins_vides() {
    let mut scene = scene_of(vec![PathGeom::Line(pt(0.0, 0.0), pt(900.0, 700.0))]);
    scene.grain_limits = vec![GrainLimit {
        oriented: true,
        tolerance_deg: 0.0,
    }];
    let s = setup(10.0, false, false, true);
    let tiles = plan_tiles_with(&scene, &s).unwrap();
    let g = grid_of((0.0, 0.0), (900.0, 700.0), WW, WH, 10.0);
    let samples = sample_paths(&[scene.paths[0].geom.clone()], 0.05);
    let (sure, maybe) = occupancy(&g, &samples, 0.2);
    let got: Vec<bool> = (0..g.rows * g.cols)
        .map(|i| tiles.iter().any(|t| t.row * g.cols + t.col == i))
        .collect();
    for i in 0..g.rows * g.cols {
        assert!(!sure[i] || got[i], "case {i} occupée omise");
        assert!(!got[i] || maybe[i], "case {i} vide conservée");
    }
    assert!(tiles.len() < g.rows * g.cols, "des coins sont vides");
    // Les coins haut-gauche et bas-droite d'une diagonale montante sont vides.
    assert!(!tiles.iter().any(|t| (t.row, t.col) == (0, 0)));
    assert!(!tiles
        .iter()
        .any(|t| (t.row, t.col) == (g.rows - 1, g.cols - 1)));
}

/// Pièce rectangulaire `w × h` mm avec un droit-fil horizontal ; le réglage de droit-fil est
/// celui donné (tolérance, sens).
fn piece_scene(w: f64, h: f64, grain: Option<PieceGrain>) -> (Project, Scene) {
    let mut p = Project::new("Assemblage");
    let s = p.sheets[0].id;
    p.apply(Command::AddRectangle {
        sheet: s,
        corner: PointRef::New(pt(0.0, 0.0)),
        width_mm: w,
        height_mm: h,
    })
    .unwrap();
    let piece: PieceId = p
        .apply(Command::ExtractPiece {
            sheet: s,
            source: PieceSource::Click {
                at: pt(w / 2.0, h / 2.0),
            },
            name: "Pièce".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    let main = p
        .apply(Command::AddSegment {
            sheet: s,
            from: PointRef::New(pt(w * 0.1, h / 2.0)),
            to: PointRef::New(pt(w * 0.9, h / 2.0)),
        })
        .unwrap()
        .entities[0];
    p.apply(Command::SetPathRole {
        sheet: s,
        entity: main,
        role: PathRole::Grainline,
    })
    .unwrap();
    let outline = p.piece(piece).unwrap().outline.clone();
    p.apply(Command::SetPieceOutline {
        piece,
        outline,
        grainline: Some(main),
    })
    .unwrap();
    if let Some(g) = grain {
        p.apply(Command::SetPieceGrain { piece, grain: g }).unwrap();
    }
    let scene = Scene::from_sheet(&p, s).unwrap();
    (p, scene)
}

#[test]
fn ca5_la_rotation_reduit_le_nombre_de_pages_sans_toucher_aux_longueurs_ni_au_modele() {
    // 700 × 100 mm : 4 pages en A4 portrait ; tournée de 90°, 100 × 700 mm : 3 pages.
    let scene = rect_scene(700.0, 100.0);
    let before = scene.clone();
    let plain = plan_tiles_with(&scene, &setup(10.0, false, false, false)).unwrap();
    assert_eq!(plain.len(), 4);
    let s = setup(10.0, true, true, true);
    let tiles = plan_tiles_with(&scene, &s).unwrap();
    assert_eq!(tiles.len(), 3);
    // Oracle : une rotation de 90° donne x ∈ [-100 ; 0], y ∈ [0 ; 700] → 1 colonne, 3 lignes.
    let g = grid_of((-100.0, 0.0), (0.0, 700.0), WW, WH, 10.0);
    assert_eq!((g.cols, g.rows), (1, 3));
    // 90° et 270° donnent le même nombre de pages : le plus petit quart de tour est retenu.
    assert!(tiles.iter().all(|t| t.turns == 1));
    for t in &tiles {
        let w = g.wmin(t.col, t.row);
        assert!(near_pt((t.world_min.x, t.world_min.y), w, 1e-9));
    }
    let out = export_pdf_with(std::slice::from_ref(&scene), &s).unwrap();
    assert_eq!(out.page_count, 1 + 1 + 3);
    let pages = read_pdf(&out.bytes);
    let tp = tile_pages(&pages);
    assert_eq!(tp.len(), 3);
    for (p, h) in &tp {
        assert_eq!(h.turned, Some(90));
        assert!(p.has("feuille tournée de 90°"));
        // Toutes les longueurs sont conservées (isométrie).
        let lens = black_lengths(p);
        assert_eq!(lens.len(), 4);
        for (got, want) in lens.iter().zip([100.0, 100.0, 700.0, 700.0]) {
            assert!(near(*got, want, 1e-3), "{got} au lieu de {want}");
        }
        // Chaque coin de la pièce est dessiné là où la rotation d'un quart de tour le met.
        for corner in [(0.0, 0.0), (700.0, 0.0), (700.0, 100.0), (0.0, 100.0)] {
            let f = rot(corner, 1);
            let w = g.wmin(h.col, h.row);
            let expect = (f.0 - w.0 + 10.0, f.1 - w.1 + 10.0);
            assert!(
                p.lines
                    .iter()
                    .any(|l| near_pt(l.a, expect, 1e-3) || near_pt(l.b, expect, 1e-3)),
                "coin {corner:?} attendu en {expect:?}"
            );
        }
    }
    // Étalon et plan le disent ; la scène n'a pas bougé d'un millimètre.
    assert!(pages[0].has("feuille tournée de 90°"), "{}", pages[0].text);
    assert!(
        pages[1].has("Feuille tournée de 90° à l'export"),
        "{}",
        pages[1].text
    );
    assert_eq!(scene, before);
}

#[test]
fn ca5_la_rotation_n_est_jamais_choisie_si_elle_ne_reduit_pas_les_pages() {
    // 100 × 100 : une page dans tous les sens.
    let tiles =
        plan_tiles_with(&rect_scene(100.0, 100.0), &setup(10.0, false, false, true)).unwrap();
    assert_eq!((tiles.len(), tiles[0].turns), (1, 0));
    // 100 × 700 : déjà le minimum (3 pages) ; la rotation en donnerait 4.
    let tiles =
        plan_tiles_with(&rect_scene(100.0, 700.0), &setup(10.0, false, false, true)).unwrap();
    assert_eq!((tiles.len(), tiles[0].turns), (3, 0));
}

#[test]
fn ca5_le_droit_fil_interdit_ou_autorise_la_rotation_selon_sa_tolerance() {
    let limit = |oriented, tolerance_deg| {
        let mut s = rect_scene(700.0, 100.0);
        s.grain_limits = vec![GrainLimit {
            oriented,
            tolerance_deg,
        }];
        s
    };
    let turns = |s: &Scene| {
        let t = plan_tiles_with(s, &setup(10.0, false, false, true)).unwrap();
        (t.len(), t[0].turns)
    };
    // Axe sans tolérance : seul 180° est permis, il ne réduit rien → pas de rotation.
    assert_eq!(
        limit(false, 0.0).allowed_quarter_turns(),
        [true, false, true, false]
    );
    assert_eq!(turns(&limit(false, 0.0)), (4, 0));
    // Axe avec 90° de tolérance : 90° permis → 3 pages.
    assert_eq!(limit(false, 90.0).allowed_quarter_turns(), [true; 4]);
    assert_eq!(turns(&limit(false, 90.0)), (3, 1));
    // Juste en dessous : refusé ; à 90° pile : accepté (une déviation égale est acceptée).
    assert_eq!(turns(&limit(false, 89.999)), (4, 0));
    assert_eq!(turns(&limit(false, 90.0)), (3, 1));
    // Ligne orientée (poil, motif) : 180° la retourne, 90° la dévie de 90°.
    assert_eq!(
        limit(true, 0.0).allowed_quarter_turns(),
        [true, false, false, false]
    );
    assert_eq!(
        limit(true, 90.0).allowed_quarter_turns(),
        [true, true, false, true]
    );
    assert_eq!(limit(true, 180.0).allowed_quarter_turns(), [true; 4]);
    assert_eq!(turns(&limit(true, 0.0)), (4, 0));
    assert_eq!(turns(&limit(true, 89.0)), (4, 0));
    assert_eq!(turns(&limit(true, 90.0)), (3, 1));
    // Deux lignes : la plus stricte décide.
    let mut s = rect_scene(700.0, 100.0);
    s.grain_limits = vec![
        GrainLimit {
            oriented: false,
            tolerance_deg: 90.0,
        },
        GrainLimit {
            oriented: false,
            tolerance_deg: 10.0,
        },
    ];
    assert_eq!(turns(&s), (4, 0));
    // Valeur douteuse (non finie) : on ne tourne pas.
    for bad in [f64::NAN, f64::NEG_INFINITY] {
        assert_eq!(
            limit(false, bad).allowed_quarter_turns(),
            [true, false, false, false]
        );
        assert_eq!(turns(&limit(false, bad)), (4, 0));
    }
    // Sans droit-fil : librement tournable.
    assert_eq!(rect_scene(700.0, 100.0).allowed_quarter_turns(), [true; 4]);
}

#[test]
fn ca5_les_limites_de_rotation_sont_lues_dans_le_modele() {
    let turns =
        |grain: Option<PieceGrain>| piece_scene(700.0, 100.0, grain).1.allowed_quarter_turns();
    // Droit-fil sans réglage : axe, aucune tolérance.
    assert_eq!(turns(None), [true, false, true, false]);
    // Tolérance de 90° saisie sur la pièce.
    let tolerant = PieceGrain {
        rotation_tolerance_deg: Some(90.0),
        ..PieceGrain::default()
    };
    assert_eq!(turns(Some(tolerant.clone())), [true; 4]);
    // Sens du poil sans tolérance : plus aucune rotation.
    let nap = PieceGrain {
        sense: GrainSense::Nap,
        ..PieceGrain::default()
    };
    assert_eq!(turns(Some(nap.clone())), [true, false, false, false]);
    // Poil avec 90° de tolérance : 90° et 270° permis, pas 180°.
    let nap90 = PieceGrain {
        rotation_tolerance_deg: Some(90.0),
        ..nap
    };
    assert_eq!(turns(Some(nap90)), [true, true, false, true]);
    // De bout en bout : la pièce tolérante est tournée à l'export, l'autre pas ; le projet
    // n'est pas modifié.
    let (project, scene) = piece_scene(700.0, 100.0, Some(tolerant));
    let before = project.clone();
    let s = setup(10.0, false, false, true);
    let t = plan_tiles_with(&scene, &s).unwrap();
    assert_eq!((t.len(), t[0].turns), (3, 1));
    let (_, strict) = piece_scene(700.0, 100.0, None);
    let t = plan_tiles_with(&strict, &s).unwrap();
    assert_eq!((t.len(), t[0].turns), (4, 0));
    assert_eq!(project, before);
}

#[test]
fn ca5_un_trace_de_droit_fil_sans_piece_et_une_pièce_sans_droit_fil() {
    // Segment de rôle droit-fil qui n'appartient à aucune pièce : traité sans tolérance.
    let mut p = Project::new("Libre");
    let s = p.sheets[0].id;
    let e = p
        .apply(Command::AddSegment {
            sheet: s,
            from: PointRef::New(pt(0.0, 0.0)),
            to: PointRef::New(pt(700.0, 100.0)),
        })
        .unwrap()
        .entities[0];
    p.apply(Command::SetPathRole {
        sheet: s,
        entity: e,
        role: PathRole::Grainline,
    })
    .unwrap();
    let scene = Scene::from_sheet(&p, s).unwrap();
    assert_eq!(
        scene.grain_limits,
        vec![GrainLimit {
            oriented: false,
            tolerance_deg: 0.0
        }]
    );
    assert_eq!(scene.allowed_quarter_turns(), [true, false, true, false]);
    // Même géométrie sans rôle de droit-fil : libre de tourner.
    let mut q = Project::new("Construction");
    let s2 = q.sheets[0].id;
    q.apply(Command::AddSegment {
        sheet: s2,
        from: PointRef::New(pt(0.0, 0.0)),
        to: PointRef::New(pt(700.0, 100.0)),
    })
    .unwrap();
    let scene = Scene::from_sheet(&q, s2).unwrap();
    assert!(scene.grain_limits.is_empty());
    assert_eq!(scene.allowed_quarter_turns(), [true; 4]);
}

// ---------------------------------------------------------------------------
// CA6 — refus sans trace
// ---------------------------------------------------------------------------

#[test]
fn ca6_un_recouvrement_invalide_est_refuse_partout_sans_rien_produire() {
    let scene = rect_scene(600.0, 450.0);
    for bad in [
        -0.001,
        50.001,
        100.0,
        -10.0,
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        1e300,
    ] {
        let s = setup(bad, true, true, true);
        let expect = |e: &ExportError| {
            if bad.is_finite() {
                matches!(
                    e,
                    ExportError::Page(PageSetupError::OverlapOutOfRange { .. })
                )
            } else {
                *e == ExportError::Page(PageSetupError::NotFinite(PageField::Overlap))
            }
        };
        let e = export_pdf_with(std::slice::from_ref(&scene), &s).unwrap_err();
        assert!(expect(&e), "{bad} : {e:?}");
        assert!(expect(
            &page_count_with(std::slice::from_ref(&scene), &s).unwrap_err()
        ));
        assert!(expect(&plan_tiles_with(&scene, &s).unwrap_err()));
        assert!(expect(
            &pf_export::pdf::export_calibration_pdf_with(&s).unwrap_err()
        ));
        assert!(!e.to_string().is_empty());
    }
    // Bornes incluses acceptées.
    for ok in [0.0, 50.0] {
        assert!(
            export_pdf_with(std::slice::from_ref(&scene), &setup(ok, true, true, true)).is_ok()
        );
    }
}

// ---------------------------------------------------------------------------
// CA7 — compatibilité : réglage par défaut = export historique
// ---------------------------------------------------------------------------

#[test]
fn ca7_le_reglage_par_defaut_n_ajoute_rien_et_reste_identique() {
    let d = PageSetup::default();
    assert!(d.is_default());
    assert_eq!(d.overlap_mm, 10.0);
    assert_eq!(d.assembly, AssemblyOptions::default());
    assert!(!d.assembly.marks && !d.assembly.plan && !d.assembly.save_paper);
    let scene = rect_scene(600.0, 450.0);
    let a = export_pdf(std::slice::from_ref(&scene)).unwrap();
    let b = export_pdf_with(
        std::slice::from_ref(&scene),
        &setup(10.0, false, false, false),
    )
    .unwrap();
    assert_eq!(a, b);
    let pages = read_pdf(&a.bytes);
    assert_eq!(pages.len(), 9);
    for p in &pages {
        assert!(!p.has("Plan d'assemblage") && !p.has(" p."));
        assert!(crosses(p).is_empty());
        assert!(!p.has("tournée"));
    }
    // Les fichiers d'exemple (réglage par défaut) sont comparés à l'octet par `exemples.rs`.
    // Activer une option change bien le fichier (et seulement alors).
    for (m, p, s) in [
        (true, false, false),
        (false, true, false),
        (false, false, true),
    ] {
        let c = export_pdf_with(std::slice::from_ref(&scene), &setup(10.0, m, p, s)).unwrap();
        // « Économiser » seul n'ajoute rien sur une pièce pleine : même nombre de pages mais
        // le réglage n'est plus « par défaut » (légende de l'étalon).
        assert_ne!(a.bytes, c.bytes, "{m} {p} {s}");
    }
}

#[test]
fn le_recouvrement_par_defaut_est_celui_de_l_ancien_export() {
    // L'ancien export découpait avec 10 mm : mêmes pages, mêmes coins.
    let scene = rect_scene(600.0, 450.0);
    let t = plan_tiles_with(&scene, &PageSetup::default()).unwrap();
    assert_eq!(t.len(), 8);
    assert!(t.iter().all(|t| t.turns == 0 && t.overlap_mm == 10.0));
    assert_eq!(pf_export::pdf::OVERLAP_MM, 10.0);
}

// ---------------------------------------------------------------------------
// Divers
// ---------------------------------------------------------------------------

#[test]
fn un_rouleau_ignore_recouvrement_plan_et_rotation() {
    let roll = |marks, plan, save| PageSetup {
        format: pf_export::PageFormat::Roll { width_mm: 914.0 },
        overlap_mm: 25.0,
        assembly: AssemblyOptions {
            marks,
            plan,
            save_paper: save,
        },
        ..PageSetup::default()
    };
    let scene = rect_scene(700.0, 100.0);
    let a = export_pdf_with(std::slice::from_ref(&scene), &roll(false, false, false)).unwrap();
    let b = export_pdf_with(std::slice::from_ref(&scene), &roll(true, true, true)).unwrap();
    assert_eq!(a.page_count, 2, "étalon + une page continue");
    assert_eq!(b.page_count, 2, "pas de page de plan pour un rouleau");
    let t = plan_tiles_with(&scene, &roll(true, true, true)).unwrap();
    assert_eq!((t.len(), t[0].turns), (1, 0));
}

#[test]
fn deux_exports_identiques_sont_identiques_a_l_octet() {
    let scene = two_blocks();
    for s in [
        setup(10.0, true, true, true),
        setup(0.0, true, false, true),
        setup(37.5, false, true, false),
    ] {
        let a = export_pdf_with(std::slice::from_ref(&scene), &s).unwrap();
        let b = export_pdf_with(std::slice::from_ref(&scene), &s).unwrap();
        assert_eq!(a, b);
    }
}

#[test]
fn les_textes_n_affirment_aucune_validation_d_impression() {
    let scene = rect_scene(600.0, 450.0);
    let out =
        export_pdf_with(std::slice::from_ref(&scene), &setup(10.0, true, true, true)).unwrap();
    let pages = read_pdf(&out.bytes);
    let all: String = pages.iter().map(|p| p.text.clone()).collect();
    assert!(all.contains("Export expérimental tant que"));
    for forbidden in ["validé", "calibré", "garanti", "certifié"] {
        assert!(!all.contains(forbidden), "« {forbidden} » dans le PDF");
    }
}
