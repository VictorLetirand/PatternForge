//! US-13.3 — étalonnage de l'imprimante : facteurs de correction X / Y, étalon renforcé, profils.
//!
//! Un test au moins par critère d'acceptation (CA1 à CA7). Les PDF sont relus par le lecteur
//! indépendant de `tests/common` (aucune constante de l'export) ; les attendus sont recalculés
//! ici. **Rien de ceci ne remplace la mesure d'une feuille imprimée** (AT-09, humaine) : on
//! prouve que le PDF contient les bonnes cotes, pas que l'imprimante les respecte.

mod common;

use common::*;
use pf_export::calibration::{
    Axis, ProfileError, ScaleError, MAX_PROFILES, PROFILE_NAME_MAX_CHARS, SCALE_MAX, SCALE_MIN,
};
use pf_export::pdf::{
    export_calibration_pdf_with, export_pdf, export_pdf_with, page_count_with, plan_tiles_with,
};
use pf_export::{
    AssemblyOptions, ExportError, Margins, Orientation, PageFormat, PageSetup, PageSetupError,
    PrintScale, PrinterProfiles,
};

const TOL: f64 = 1e-3;

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() <= TOL
}

fn scaled(x: f64, y: f64) -> PageSetup {
    PageSetup {
        scale: PrintScale { x, y },
        ..PageSetup::default()
    }
}

fn tile_pages(pages: &[Page]) -> Vec<&Page> {
    pages.iter().filter(|p| p.has(" · page ")).collect()
}

/// Segments noirs (patron) de plus de 5 mm.
fn black(p: &Page) -> Vec<Line> {
    p.lines
        .iter()
        .copied()
        .filter(|l| l.gray == 0.0 && l.len() > 5.01)
        .collect()
}

fn horizontal(l: &Line) -> bool {
    near(l.a.1, l.b.1)
}

fn vertical(l: &Line) -> bool {
    near(l.a.0, l.b.0)
}

// ---------------------------------------------------------------------------
// CA1 : facteurs bornés et validés ; défaut inchangé à l'octet
// ---------------------------------------------------------------------------

#[test]
fn ca1_facteurs_valides_bornes_et_defaut_identique_a_l_octet() {
    // Défaut : 1,0 / 1,0, aucune option.
    let d = PageSetup::default();
    assert_eq!(d.scale, PrintScale { x: 1.0, y: 1.0 });
    assert!(d.scale.is_identity() && !d.control_marks && !d.shows_control_marks());
    assert!(d.is_default());

    // Octets du PDF par défaut : les trois entrées donnent le même fichier, sans trace de la
    // correction ni des règles de contrôle (les `examples/` sont comparés par `exemples.rs`).
    let scene = rect_scene(150.0, 80.0);
    let a = export_pdf(std::slice::from_ref(&scene)).unwrap();
    let b = export_pdf_with(std::slice::from_ref(&scene), &d).unwrap();
    let c = export_pdf_with(
        std::slice::from_ref(&scene),
        &PageSetup {
            scale: PrintScale::new(1.0, 1.0).unwrap(),
            ..d
        },
    )
    .unwrap();
    assert_eq!(a.bytes, b.bytes);
    assert_eq!(a.bytes, c.bytes);
    let text = common::pdf_text::readable(&a.bytes);
    assert!(
        !text.contains("orrection d'"),
        "pas de mention sans correction"
    );
    assert!(
        !text.contains("contrôle"),
        "pas de règle de contrôle par défaut"
    );

    // Bornes : [0,90 ; 1,10] fermées ; hors bornes, NaN, infinis, 0 et négatifs refusés.
    assert_eq!((SCALE_MIN, SCALE_MAX), (0.90, 1.10));
    for ok in [0.90, 0.9999, 1.0, 1.0001, 1.10] {
        assert!(PrintScale::new(ok, 1.0).is_ok() && PrintScale::new(1.0, ok).is_ok());
    }
    for bad in [
        0.0,
        -1.0,
        0.8999,
        1.1001,
        2.0,
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
    ] {
        let ex = PrintScale::new(bad, 1.0).unwrap_err();
        let ey = PrintScale::new(1.0, bad).unwrap_err();
        let (nx, ny) = (
            matches!(ex, ScaleError::NotFinite(Axis::X)),
            bad.is_finite(),
        );
        assert!(nx != ny || matches!(ex, ScaleError::OutOfRange { axis: Axis::X, .. }));
        assert!(matches!(
            ey,
            ScaleError::NotFinite(Axis::Y) | ScaleError::OutOfRange { axis: Axis::Y, .. }
        ));
        assert!(!ex.to_string().is_empty());
        // Un réglage porteur d'un facteur invalide est refusé partout, sans rien produire.
        let setup = PageSetup {
            scale: PrintScale { x: bad, y: 1.0 },
            ..PageSetup::default()
        };
        assert!(matches!(setup.validate(), Err(PageSetupError::Scale(_))));
        let e = export_pdf_with(std::slice::from_ref(&scene), &setup).unwrap_err();
        assert!(
            matches!(e, ExportError::Page(PageSetupError::Scale(_))),
            "{bad}"
        );
        assert_eq!(
            page_count_with(std::slice::from_ref(&scene), &setup),
            Err(e)
        );
        assert!(plan_tiles_with(&scene, &setup).is_err());
        assert!(export_calibration_pdf_with(&setup).is_err());
    }

    // Facteur déduit d'une mesure : attendu / mesuré, arrondi à 6 décimales.
    let s = PrintScale::from_measures(99.5, 100.25).unwrap();
    assert!(near(s.x, 100.0 / 99.5) && near(s.y, 100.0 / 100.25));
    assert_eq!(s.x, 1.005025);
    // Une mesure de 100,00 mm = pas de correction.
    assert!(PrintScale::from_measures(100.0, 100.004)
        .unwrap()
        .is_identity());
    // Écart de plus de 10 % ou mesure absurde : refus.
    for bad in [0.0, -1.0, 50.0, 200.0, 90.0, 111.2, f64::NAN, f64::INFINITY] {
        assert!(PrintScale::from_measures(bad, 100.0).is_err(), "{bad}");
        assert!(PrintScale::from_measures(100.0, bad).is_err(), "{bad}");
    }
    assert!(PrintScale::from_measures(90.91, 111.11).is_ok());
}

// ---------------------------------------------------------------------------
// CA2 : application au rendu, dimensions du PDF = attendu × facteur
// ---------------------------------------------------------------------------

/// Côtés (largeur, hauteur) du carré étalon relu dans la page étalon.
fn etalon_square(p: &Page, fx: f64, fy: f64) -> (f64, f64) {
    let r = p
        .rects
        .iter()
        .find(|r| near(r.2, 100.0 * fx) && near(r.3, 100.0 * fy))
        .unwrap_or_else(|| {
            panic!(
                "carré étalon {}×{} absent : {:?}",
                100.0 * fx,
                100.0 * fy,
                p.rects
            )
        });
    (r.2, r.3)
}

#[test]
fn ca2_les_cotes_du_pdf_valent_l_attendu_fois_le_facteur_sur_chaque_axe() {
    let scene = rect_scene(150.0, 80.0);
    for (fx, fy) in [
        (1.0, 1.0),
        (1.02, 1.0),
        (1.0, 0.97),
        (1.02, 0.97),
        (0.90, 1.10),
        (1.10, 0.90),
        (1.005025, 1.0),
    ] {
        let setup = scaled(fx, fy);
        let out = export_pdf_with(std::slice::from_ref(&scene), &setup).unwrap();
        let pages = read_pdf(&out.bytes);
        // Carré étalon : 100 × facteur sur chaque axe.
        let (w, h) = etalon_square(&pages[0], fx, fy);
        assert!(near(w, 100.0 * fx) && near(h, 100.0 * fy), "{fx} {fy}");
        // Règle de 100 mm de l'étalon : même facteur (ligne horizontale de 100·fx).
        assert!(
            pages[0]
                .lines
                .iter()
                .any(|l| horizontal(l) && near(l.len(), 100.0 * fx)),
            "{fx} {fy}"
        );
        // Longueur connue : les côtés 150 (X) et 80 (Y) du patron.
        let t = tile_pages(&pages);
        assert_eq!(t.len(), 1, "{fx} {fy}");
        let segs = black(t[0]);
        let (hs, vs): (Vec<Line>, Vec<Line>) = segs.iter().partition(|l| horizontal(l));
        assert_eq!((hs.len(), vs.len()), (2, 2), "{fx} {fy}");
        assert!(hs.iter().all(|l| near(l.len(), 150.0 * fx)), "{fx} {fy}");
        assert!(vs.iter().all(|l| near(l.len(), 80.0 * fy)), "{fx} {fy}");
        // Autour du coin bas-gauche de la zone imprimable : le coin du patron est à 5 mm de
        // blanc du bord gauche de la fenêtre quelle que soit la correction.
        let xs: Vec<f64> = segs.iter().flat_map(|l| [l.a.0, l.b.0]).collect();
        let (lo, hi) = (
            xs.iter().cloned().fold(f64::MAX, f64::min),
            xs.iter().cloned().fold(f64::MIN, f64::max),
        );
        assert!(near(lo, 10.0 + 5.0), "{fx} {fy} : coin gauche {lo}");
        assert!(near(hi - lo, 150.0 * fx), "{fx} {fy}");
        // Le sens du facteur : une imprimante qui rétrécit de k imprime 100 × f × k = 100 mm
        // quand f = 1 / k ; la mesure puis l'impression se referment sur 100 mm.
        let k = 99.5 / 100.0;
        let f = PrintScale::from_measures(99.5, 99.5).unwrap().x;
        assert!(((100.0 * f) * k - 100.0).abs() < 1e-3);
    }
    // L'étalon seul (sans patron) est corrigé de la même façon, en paysage aussi.
    let setup = PageSetup {
        orientation: Orientation::Landscape,
        ..scaled(1.03, 0.98)
    };
    let pages = read_pdf(&export_calibration_pdf_with(&setup).unwrap().bytes);
    etalon_square(&pages[0], 1.03, 0.98);
    // L'étalon porte le facteur en toutes lettres.
    assert!(pages
        .iter()
        .any(|p| p.has("Correction d'échelle active : X ×1.03, Y ×0.98")));
}

// ---------------------------------------------------------------------------
// CA3 : découpage en pages et repères avec facteurs ≠ 1
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
struct Hdr {
    row: usize,
    col: usize,
    cols: usize,
    rows: usize,
}

fn hdr(p: &Page) -> Hdr {
    let s = p
        .strings
        .iter()
        .find(|s| s.contains("taille réelle"))
        .unwrap();
    let parts: Vec<&str> = s.split(" · ").collect();
    let id = parts[1].split(' ').next().unwrap();
    let (r, c) = id.strip_prefix('L').unwrap().split_once('C').unwrap();
    let (rows, cols) = parts[1]
        .split("sur ")
        .nth(1)
        .unwrap()
        .split_once('×')
        .unwrap();
    Hdr {
        row: r.parse::<usize>().unwrap() - 1,
        col: c.parse::<usize>().unwrap() - 1,
        cols: cols.parse().unwrap(),
        rows: rows.parse().unwrap(),
    }
}

fn crosses(p: &Page) -> Vec<(f64, f64)> {
    p.lines
        .iter()
        .filter(|l| near(l.gray, 0.3) && (l.len() - 6.0).abs() < 0.01 && near(l.a.1, l.b.1))
        .map(|l| ((l.a.0 + l.b.0) / 2.0, (l.a.1 + l.b.1) / 2.0))
        .collect()
}

#[test]
fn ca3_decoupage_recouvrement_et_reperes_restent_coherents_avec_des_facteurs() {
    // Patron de 600 × 400 mm : avec (1,04 ; 0,96) il occupe 624 × 384 mm sur le papier.
    let scene = rect_scene(600.0, 400.0);
    for (fx, fy, ov) in [(1.04, 0.96, 10.0), (0.90, 1.10, 25.0), (1.10, 0.90, 0.0)] {
        let setup = PageSetup {
            overlap_mm: ov,
            assembly: AssemblyOptions {
                marks: true,
                ..AssemblyOptions::default()
            },
            ..scaled(fx, fy)
        };
        let out = export_pdf_with(std::slice::from_ref(&scene), &setup).unwrap();
        let pages = read_pdf(&out.bytes);
        // Le nombre de pages prévu est exactement celui des pages écrites.
        assert_eq!(
            page_count_with(std::slice::from_ref(&scene), &setup).unwrap(),
            out.page_count
        );
        assert_eq!(pages.len(), out.page_count);
        // Grille recalculée ici sur l'emprise corrigée (une page papier couvre fenêtre / facteur).
        let (ww, wh) = (190.0, 297.0 - 20.0 - 8.0);
        let g = grid_of((0.0, 0.0), (600.0 * fx, 400.0 * fy), ww, wh, ov);
        let tiles = tile_pages(&pages);
        assert_eq!(tiles.len(), g.cols * g.rows, "{fx} {fy} {ov}");
        let plan = plan_tiles_with(&scene, &setup).unwrap();
        assert_eq!(plan.len(), tiles.len());
        for t in &plan {
            let w = g.wmin(t.col, t.row);
            assert!(
                near(t.world_min.x, w.0) && near(t.world_min.y, w.1),
                "{fx} {fy}"
            );
            assert_eq!(t.scale, setup.scale);
        }
        // Chaque page : les arêtes verticales du patron (x = 0 et x = 600 mm) tombent à la
        // position prévue par la grille corrigée.
        let by = |r: usize, c: usize| {
            tiles
                .iter()
                .copied()
                .find(|p| {
                    let h = hdr(p);
                    h.row == r && h.col == c
                })
                .unwrap()
        };
        for p in &tiles {
            let h = hdr(p);
            assert_eq!((h.cols, h.rows), (g.cols, g.rows));
            let w = g.wmin(h.col, h.row);
            for l in black(p).iter().filter(|l| vertical(l)) {
                let wx = l.a.0 - 10.0 + w.0;
                assert!(
                    near(wx, 0.0) || near(wx, 600.0 * fx),
                    "{fx} {fy} : arête verticale à {wx}"
                );
            }
        }
        // Repères : deux pages voisines portent les mêmes points du repère corrigé, donc
        // les mêmes points du patron (division par le facteur).
        let frame = |h: Hdr, p: (f64, f64)| {
            let w = g.wmin(h.col, h.row);
            (p.0 - 10.0 + w.0, p.1 - 10.0 + w.1)
        };
        let mut compared = 0;
        for r in 0..g.rows {
            for c in 0..g.cols {
                let a = by(r, c);
                let ha = hdr(a);
                let mut neighbours = Vec::new();
                if c + 1 < g.cols {
                    neighbours.push((by(r, c + 1), true));
                }
                if r + 1 < g.rows {
                    neighbours.push((by(r + 1, c), false));
                }
                for (b, right) in neighbours {
                    let hb = hdr(b);
                    // Bande commune : [wmin(b).x ; + ov] (voisine de droite) ou [wmin(a).y ; + ov] (dessous).
                    let (wa, wb) = (g.wmin(ha.col, ha.row), g.wmin(hb.col, hb.row));
                    // Croix de la bande : à 6 mm des bords de la fenêtre le long de la bande.
                    let band = |p: (f64, f64)| {
                        if right {
                            p.0 >= wb.0 - TOL
                                && p.0 <= wb.0 + ov + TOL
                                && (near(p.1 - wa.1, 6.0) || near(p.1 - wa.1, wh - 6.0))
                        } else {
                            p.1 >= wa.1 - TOL
                                && p.1 <= wa.1 + ov + TOL
                                && (near(p.0 - wa.0, 6.0) || near(p.0 - wa.0, ww - 6.0))
                        }
                    };
                    let pick = |page: &Page, h: Hdr| {
                        let mut v: Vec<(f64, f64)> = crosses(page)
                            .into_iter()
                            .map(|q| frame(h, q))
                            .filter(|q| band(*q))
                            .collect();
                        v.sort_by_key(|q| {
                            ((q.0 * 100.0).round() as i64, (q.1 * 100.0).round() as i64)
                        });
                        v
                    };
                    let (va, vb) = (pick(a, ha), pick(b, hb));
                    assert!(
                        !va.is_empty(),
                        "{fx} {fy} {ov} : aucune croix dans la bande"
                    );
                    assert_eq!(va.len(), vb.len());
                    for (p, q) in va.iter().zip(&vb) {
                        assert!(
                            near(p.0, q.0) && near(p.1, q.1),
                            "{fx} {fy} {ov} : {p:?} / {q:?} ({va:?} {vb:?})"
                        );
                        // Même point du patron : repère / facteur, à la même position.
                        assert!(near(p.0 / fx, q.0 / fx) && near(p.1 / fy, q.1 / fy));
                        compared += 1;
                    }
                }
            }
        }
        assert!(compared >= 4, "{fx} {fy} {ov} : {compared} croix comparées");
    }
}

#[test]
fn ca3_la_correction_peut_changer_le_nombre_de_pages_et_il_est_prevu_exactement() {
    // 340 mm de large : 2 pages (190 de fenêtre, 10 de recouvrement) ; ×1,10 → 384 mm : 3.
    let scene = rect_scene(340.0, 100.0);
    let n = |fx: f64| {
        let setup = scaled(fx, 1.0);
        let out = export_pdf_with(std::slice::from_ref(&scene), &setup).unwrap();
        assert_eq!(
            page_count_with(std::slice::from_ref(&scene), &setup).unwrap(),
            out.page_count
        );
        tile_pages(&read_pdf(&out.bytes)).len()
    };
    assert_eq!(n(1.0), pages_to_cover(350.0, 190.0, 10.0));
    assert_eq!(n(1.10), pages_to_cover(340.0 * 1.10 + 10.0, 190.0, 10.0));
    assert!(n(1.10) > n(1.0));
    assert!(n(0.90) <= n(1.0));
}

#[test]
fn ca3_rouleau_corrige_la_largeur_et_la_longueur() {
    let scene = rect_scene(300.0, 500.0);
    let setup = PageSetup {
        format: PageFormat::Roll { width_mm: 400.0 },
        margins: Margins::uniform(10.0),
        ..scaled(1.10, 1.05)
    };
    let out = export_pdf_with(std::slice::from_ref(&scene), &setup).unwrap();
    let pages = read_pdf(&out.bytes);
    assert_eq!(
        page_count_with(std::slice::from_ref(&scene), &setup).unwrap(),
        pages.len()
    );
    let t = tile_pages(&pages);
    assert_eq!(t.len(), 1);
    let segs = black(t[0]);
    assert!(segs
        .iter()
        .filter(|l| horizontal(l))
        .all(|l| near(l.len(), 330.0)));
    assert!(segs
        .iter()
        .filter(|l| vertical(l))
        .all(|l| near(l.len(), 525.0)));
    // Un contenu qui ne tient plus une fois corrigé est refusé, jamais réduit : 380 × 1,10 + 10
    // dépasse les 380 mm imprimables du rouleau de 400 mm.
    let wide = rect_scene(340.0, 100.0);
    assert!(export_pdf_with(std::slice::from_ref(&wide), &scaled_roll(1.0)).is_ok());
    assert!(matches!(
        export_pdf_with(std::slice::from_ref(&wide), &scaled_roll(1.10)),
        Err(ExportError::Page(PageSetupError::ContentTooWide { .. }))
    ));
}

fn scaled_roll(fx: f64) -> PageSetup {
    PageSetup {
        format: PageFormat::Roll { width_mm: 400.0 },
        ..scaled(fx, 1.0)
    }
}

// ---------------------------------------------------------------------------
// CA4 : règles de contrôle en mm papier non corrigés
// ---------------------------------------------------------------------------

fn gray_lines(p: &Page, gray: f64) -> Vec<Line> {
    p.lines
        .iter()
        .copied()
        .filter(|l| near(l.gray, gray))
        .collect()
}

#[test]
fn ca4_etalon_renforce_regles_non_corrigees_absentes_par_defaut() {
    let scene = rect_scene(150.0, 80.0);
    // Défaut : aucune règle de contrôle (octets inchangés).
    let base = read_pdf(
        &export_pdf_with(std::slice::from_ref(&scene), &PageSetup::default())
            .unwrap()
            .bytes,
    );
    assert!(base[0]
        .lines
        .iter()
        .all(|l| !(near(l.len(), 160.0) || near(l.len(), 200.0))));
    for p in tile_pages(&base) {
        assert!(gray_lines(p, 0.45).is_empty());
        assert!(!p.has("repère de contrôle"));
    }

    // Option seule : règles et repères présents, facteurs restés à 1.
    let opt = PageSetup {
        control_marks: true,
        ..PageSetup::default()
    };
    // Option + correction : les règles NE SONT PAS corrigées, le carré l'est.
    for setup in [
        opt,
        PageSetup {
            control_marks: true,
            ..scaled(1.03, 0.98)
        },
        scaled(1.03, 0.98),
    ] {
        let (fx, fy) = (setup.scale.x, setup.scale.y);
        let pages = read_pdf(
            &export_pdf_with(std::slice::from_ref(&scene), &setup)
                .unwrap()
                .bytes,
        );
        let e = &pages[0];
        // A4 : règle horizontale de 160 mm (zone imprimable 190 − 22, au multiple de 10 inférieur),
        // verticale de 200 mm : longueurs de papier réelles, indépendantes du facteur.
        let h = e
            .lines
            .iter()
            .filter(|l| horizontal(l) && near(l.len(), 160.0))
            .count();
        let v = e
            .lines
            .iter()
            .filter(|l| vertical(l) && near(l.len(), 200.0))
            .count();
        assert_eq!((h, v), (1, 1), "{fx} {fy}");
        // Graduations : un trait par mm (161 et 201) plus la ligne ; chiffres tous les 10 mm.
        assert!(e.has("160") && e.has("200") && e.has("Règles de contrôle 0–160 et 0–200 mm"));
        // Le carré de 100 mm, lui, est corrigé.
        etalon_square(e, fx, fy);
        // Chaque page de patron porte deux règles de 100 mm non corrigées, de gris 0,45.
        for p in tile_pages(&pages) {
            let g = gray_lines(p, 0.45);
            assert_eq!(
                g.iter()
                    .filter(|l| horizontal(l) && near(l.len(), 100.0))
                    .count(),
                1
            );
            assert_eq!(
                g.iter()
                    .filter(|l| vertical(l) && near(l.len(), 100.0))
                    .count(),
                1
            );
            assert!(p.has("repère de contrôle 100 mm, non corrigé"));
            // Posées au coin bas-gauche de la fenêtre (marges de 10 mm).
            let base = g
                .iter()
                .find(|l| horizontal(l) && near(l.len(), 100.0))
                .unwrap();
            assert!(near(base.a.0, 10.0) && near(base.a.1, 10.0));
        }
    }
    // Un profil actif seul (sans l'option) active aussi les règles.
    assert!(scaled(1.01, 1.0).shows_control_marks());
}

// ---------------------------------------------------------------------------
// CA5 : profils nommés (logique de liste ; stockage et fenêtre : tests de l'application)
// ---------------------------------------------------------------------------

#[test]
fn ca5_profils_nommes_actif_ajout_modification_suppression() {
    let mut book = PrinterProfiles::new();
    assert_eq!(book.active_scale(), PrintScale::default());
    let a = book
        .add("Bureau", PrintScale::from_measures(99.5, 100.0).unwrap())
        .unwrap();
    let b = book
        .add("Atelier A0", PrintScale::new(0.99, 1.01).unwrap())
        .unwrap();
    assert_eq!((a, b), (0, 1));
    assert_eq!(book.active(), None, "ajouter n'active pas");
    book.set_active(Some(1)).unwrap();
    assert_eq!(book.active_scale(), PrintScale { x: 0.99, y: 1.01 });
    // Le profil actif alimente l'export.
    let scene = rect_scene(150.0, 80.0);
    let setup = PageSetup {
        scale: book.active_scale(),
        ..PageSetup::default()
    };
    let pages = read_pdf(
        &export_pdf_with(std::slice::from_ref(&scene), &setup)
            .unwrap()
            .bytes,
    );
    etalon_square(&pages[0], 0.99, 1.01);
    // Renommer / changer les facteurs.
    book.update(1, "Atelier", PrintScale::new(1.0, 1.0).unwrap())
        .unwrap();
    assert_eq!(book.list()[1].name, "Atelier");
    assert!(book.active_scale().is_identity());
    // Supprimer le profil actif : plus de correction.
    book.remove(1).unwrap();
    assert_eq!(book.active(), None);
    assert_eq!(book.list().len(), 1);
    // Supprimer un profil avant l'actif garde le même profil actif.
    book.add("Maison", PrintScale::new(1.02, 1.0).unwrap())
        .unwrap();
    book.set_active(Some(1)).unwrap();
    book.remove(0).unwrap();
    assert_eq!(book.active().unwrap().name, "Maison");
    // Relecture : doublons et profils invalides écartés, jamais appliqués.
    let loaded = PrinterProfiles::from_parts(
        vec![
            ("A".into(), 1.01, 1.0),
            ("a".into(), 1.02, 1.0),
            ("B".into(), 5.0, 1.0),
            ("".into(), 1.0, 1.0),
            ("C".into(), f64::NAN, 1.0),
            ("D".into(), 0.95, 1.05),
        ],
        Some("d"),
    );
    let names: Vec<&str> = loaded.list().iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["A", "D"]);
    assert_eq!(loaded.active().unwrap().name, "D");
    assert_eq!(
        PrinterProfiles::from_parts(vec![("A".into(), 1.0, 1.0)], Some("absent")).active(),
        None
    );
}

// ---------------------------------------------------------------------------
// CA6 : refus sans trace
// ---------------------------------------------------------------------------

#[test]
fn ca6_valeurs_invalides_refusees_la_liste_reste_identique() {
    let mut book = PrinterProfiles::new();
    book.add("Bureau", PrintScale::new(1.01, 0.99).unwrap())
        .unwrap();
    book.set_active(Some(0)).unwrap();
    let before = book.clone();
    let ok = PrintScale::default();
    let long = "x".repeat(PROFILE_NAME_MAX_CHARS + 1);
    assert_eq!(book.add("", ok), Err(ProfileError::NameEmpty));
    assert_eq!(book.add("   ", ok), Err(ProfileError::NameEmpty));
    assert_eq!(book.add("a\u{7}b", ok), Err(ProfileError::NameControlChar));
    assert_eq!(book.add("a\tb", ok), Err(ProfileError::NameControlChar));
    assert!(matches!(
        book.add(&long, ok),
        Err(ProfileError::NameTooLong { .. })
    ));
    assert_eq!(book.add("BUREAU", ok), Err(ProfileError::Duplicate));
    assert!(matches!(
        book.add(
            "Nouveau",
            PrintScale {
                x: f64::NAN,
                y: 1.0
            }
        ),
        Err(ProfileError::Scale(ScaleError::NotFinite(Axis::X)))
    ));
    assert!(matches!(
        book.add("Nouveau", PrintScale { x: 1.0, y: 0.0 }),
        Err(ProfileError::Scale(ScaleError::OutOfRange {
            axis: Axis::Y,
            ..
        }))
    ));
    assert_eq!(book.update(7, "Autre", ok), Err(ProfileError::Unknown));
    assert_eq!(book.remove(7), Err(ProfileError::Unknown));
    assert_eq!(book.set_active(Some(7)), Err(ProfileError::Unknown));
    assert_eq!(book, before, "aucun refus ne modifie la liste");
    // 40 caractères exactement : accepté ; la 17e entrée : refusée.
    let name40 = "é".repeat(PROFILE_NAME_MAX_CHARS);
    book.add(&name40, ok).unwrap();
    for i in 0..(MAX_PROFILES - 2) {
        book.add(&format!("p{i}"), ok).unwrap();
    }
    assert_eq!(book.list().len(), MAX_PROFILES);
    let full = book.clone();
    assert_eq!(
        book.add("17e", ok),
        Err(ProfileError::TooMany { max: MAX_PROFILES })
    );
    assert_eq!(book, full);
    // Renommer en doublon d'un autre profil : refusé ; sur lui-même (autre casse) : accepté.
    assert_eq!(book.update(0, "p0", ok), Err(ProfileError::Duplicate));
    book.update(0, "bureau", ok).unwrap();
}

// ---------------------------------------------------------------------------
// CA7 : honnêteté
// ---------------------------------------------------------------------------

#[test]
fn ca7_aucun_texte_ne_pretend_que_l_imprimante_est_calibree() {
    let scene = rect_scene(150.0, 80.0);
    for setup in [
        PageSetup::default(),
        scaled(1.02, 0.98),
        PageSetup {
            control_marks: true,
            assembly: AssemblyOptions {
                plan: true,
                marks: true,
                save_paper: false,
            },
            ..scaled(0.95, 1.05)
        },
    ] {
        let out = export_pdf_with(std::slice::from_ref(&scene), &setup).unwrap();
        let text = read_pdf(&out.bytes)
            .iter()
            .map(|p| p.text.to_lowercase())
            .collect::<String>();
        for forbidden in [
            "calibrée",
            "calibré",
            "validée",
            "validé",
            "garanti",
            "certifi",
            "précis",
        ] {
            assert!(!text.contains(forbidden), "texte « {forbidden} » interdit");
        }
        assert!(text.contains("export expérimental tant que cet étalon n'a pas été mesuré"));
        if !setup.scale.is_identity() {
            assert!(text.contains("à vérifier par mesure au réglet sur le papier"));
        }
    }
}

// ---------------------------------------------------------------------------
// Contrôle par un lecteur tiers (facultatif)
// ---------------------------------------------------------------------------

/// Si `PF_DUMP_PDF_DIR` désigne un dossier, y écrit trois PDF pour les relire avec un lecteur
/// tiers (MuPDF : voir `scripts/verifier_pdf.py`). Sans la variable, ne fait rien.
#[test]
fn ecrit_des_pdf_corriges_pour_un_lecteur_tiers_si_demande() {
    let Some(dir) = std::env::var_os("PF_DUMP_PDF_DIR") else {
        return;
    };
    let dir = std::path::PathBuf::from(dir);
    let scene = rect_scene(150.0, 80.0);
    for (name, setup) in [
        ("x102_y097", scaled(1.02, 0.97)),
        (
            "x090_y110_renforce",
            PageSetup {
                control_marks: true,
                ..scaled(0.90, 1.10)
            },
        ),
        ("defaut", PageSetup::default()),
    ] {
        let out = export_pdf_with(std::slice::from_ref(&scene), &setup).unwrap();
        std::fs::write(dir.join(format!("{name}.pdf")), out.bytes).unwrap();
    }
}
