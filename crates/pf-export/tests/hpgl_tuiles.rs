//! PH-5e — HPGL tuilé : un fichier par page, mêmes pages que le PDF (US-13.2), tracés coupés
//! exactement aux bords de la fenêtre. Les valeurs sont écrites à la main (calculs en commentaire) ;
//! les fichiers sont relus par le lecteur HPGL indépendant de `common/hpgl.rs`.
//!
//! Le HPGL n'a été lu par aucun traceur ni logiciel tiers : rien ici ne le prouve.

mod common;

use common::hpgl::*;
use common::{grid_of, rect_at, rect_scene, scene_of};
use pf_export::hpgl::{export_hpgl, export_hpgl_pages, export_hpgl_with, HpglError, HpglOptions};
use pf_export::pdf::{page_count_with, plan_tiles_with};
use pf_export::{AssemblyOptions, Margins, PageFormat, PageSetup, PageSetupError, PathGeom};

/// Segment sans sens : extrémités en unités, ordre normalisé.
type Seg = ((i64, i64), (i64, i64));

/// Jupe de 255 × 650 mm (coin bas-gauche en 0 ; 0), quatre lignes de coupe.
fn jupe() -> pf_export::Scene {
    rect_scene(255.0, 650.0)
}

fn pages_of(scene: &pf_export::Scene, page: &PageSetup) -> pf_export::hpgl::HpglTiles {
    export_hpgl_pages(scene, page, HpglOptions::default()).expect("export tuilé")
}

/// Segments d'une page (sommets en unités) : chaque trait relu est découpé en segments, rendus
/// sans sens (ordre des extrémités normalisé) et triés.
fn segments(text: &str, pen: u8) -> Vec<Seg> {
    let r = read_hpgl(text).expect("lecture");
    let mut v: Vec<_> = r
        .strokes
        .iter()
        .filter(|s| s.pen == pen)
        .flat_map(|s| s.pts.windows(2).map(|w| (w[0].min(w[1]), w[0].max(w[1]))))
        .collect();
    v.sort();
    v
}

fn seg(a: (i64, i64), b: (i64, i64)) -> Seg {
    (a.min(b), a.max(b))
}

fn len_mm(text: &str) -> f64 {
    read_hpgl(text)
        .unwrap()
        .strokes
        .iter()
        .map(|s| s.length_mm())
        .sum()
}

// Calcul à la main, A4 portrait, marges 10, recouvrement 10 (défaut) :
//   fenêtre = (210 - 20) × (297 - 20 - 8) = 190 × 269 mm (le bandeau de 8 mm est celui du PDF) ;
//   emprise 255 × 650 + 5 mm de blanc de chaque côté = 265 × 660 ;
//   colonnes = ceil((265 - 10) / 180) = 2, lignes = ceil((660 - 10) / 259) = 3 → 6 pages ;
//   x0 = -5, haut = 655 : coin bas-gauche (col, ligne) = (-5 + 180·col ; 386 - 259·ligne).
//   Pages : L1C1 (-5 ; 386), L1C2 (175 ; 386), L2C1 (-5 ; 127), L2C2 (175 ; 127),
//           L3C1 (-5 ; -132), L3C2 (175 ; -132).

#[test]
fn ca1_la_jupe_est_refusee_sur_a4_sans_tuilage_et_decoupee_avec() {
    let s = jupe();
    let page = PageSetup::default();
    assert!(matches!(
        export_hpgl(&s, &page),
        Err(HpglError::SheetTooSmall { .. })
    ));
    let t = pages_of(&s, &page);
    assert_eq!(t.pages.len(), 6);
    let ids: Vec<&str> = t.pages.iter().map(|p| p.id.as_str()).collect();
    assert_eq!(ids, ["L1C1", "L1C2", "L2C1", "L2C2", "L3C1", "L3C2"]);
    for (k, p) in t.pages.iter().enumerate() {
        assert_eq!((p.number, p.total), (k + 1, 6));
        assert_eq!((p.row, p.col), (k / 2, k % 2));
    }
    assert!(!t.plan_ignored);
}

#[test]
fn ca1_memes_pages_que_le_pdf() {
    let s = jupe();
    let page = PageSetup::default();
    let t = pages_of(&s, &page);
    let tiles = plan_tiles_with(&s, &page).unwrap();
    assert_eq!(t.pages.len(), tiles.len());
    for (p, tile) in t.pages.iter().zip(&tiles) {
        assert_eq!(
            (p.col, p.row, p.id.clone()),
            (tile.col, tile.row, tile.id())
        );
    }
    // Le PDF ajoute une page étalon aux pages de patron.
    assert_eq!(page_count_with(&[s], &page).unwrap(), t.pages.len() + 1);
}

#[test]
fn ca2_segments_coupes_exactement_aux_bords_de_chaque_page() {
    let t = pages_of(&jupe(), &PageSetup::default());
    // Unités = 0,025 mm : 1 mm = 40 unités. Fenêtre 190 × 269 = 7600 × 10760 unités.
    let expected: [Vec<Seg>; 6] = [
        // L1C1 (-5 ; 386) : arête gauche x = 0 → 5 mm, de y = 386 (0) à 650 (264 mm = 10560) ;
        // arête haute y = 650 → 264 mm, de x = 0 (5 mm = 200) au bord droit x = 185 (190 mm = 7600).
        vec![
            seg((200, 0), (200, 10560)),
            seg((200, 10560), (7600, 10560)),
        ],
        // L1C2 (175 ; 386) : arête haute de x = 175 (0) à 255 (80 mm = 3200), arête droite x = 255.
        vec![
            seg((0, 10560), (3200, 10560)),
            seg((3200, 0), (3200, 10560)),
        ],
        // L2C1 (-5 ; 127), hauteur 269 : arête gauche seule (x = 0 ; 5 mm), sur toute la fenêtre.
        vec![seg((200, 0), (200, 10760))],
        // L2C2 (175 ; 127) : arête droite seule (x = 255 ; 80 mm).
        vec![seg((3200, 0), (3200, 10760))],
        // L3C1 (-5 ; -132) : arête gauche de y = 0 (132 mm = 5280) à 137 (269 mm), arête basse
        // y = 0 (132 mm) de x = 0 à 185.
        vec![
            seg((200, 5280), (200, 10760)),
            seg((200, 5280), (7600, 5280)),
        ],
        // L3C2 (175 ; -132) : arête basse de x = 175 à 255, arête droite de y = 0 à 137.
        vec![
            seg((0, 5280), (3200, 5280)),
            seg((3200, 5280), (3200, 10760)),
        ],
    ];
    for (k, want) in expected.iter().enumerate() {
        let mut want = want.clone();
        want.sort();
        assert_eq!(segments(&t.pages[k].text, 1), want, "page {}", k + 1);
    }
}

#[test]
fn ca2_aucun_point_hors_de_la_page_et_longueurs_calculees() {
    let t = pages_of(&jupe(), &PageSetup::default());
    for p in &t.pages {
        let r = read_hpgl(&p.text).unwrap();
        for s in &r.strokes {
            for q in &s.pts {
                assert!(
                    (0..=7600).contains(&q.0) && (0..=10760).contains(&q.1),
                    "{q:?}"
                );
            }
        }
    }
    // Longueurs (mm) des tracés par page : 264 + 185, 80 + 264, 269, 269, 137 + 185, 80 + 137.
    let want = [449.0, 344.0, 269.0, 269.0, 322.0, 217.0];
    for (p, w) in t.pages.iter().zip(want) {
        let got: f64 = read_hpgl(&p.text)
            .unwrap()
            .strokes
            .iter()
            .filter(|s| s.pen == 1)
            .map(|s| s.length_mm())
            .sum();
        assert!(
            (got - w).abs() < 1e-3,
            "page {} : {got} au lieu de {w}",
            p.number
        );
        assert!((p.report.pen_down_mm - w).abs() < 1e-3);
    }
    // Somme = périmètre 2 × (255 + 650) = 1810 + 60 mm doublés dans les bandes de recouvrement
    // (3 bandes × 2 arêtes × 10 mm).
    let total: f64 = t.pages.iter().map(|p| p.report.pen_down_mm).sum();
    assert!((total - 1870.0).abs() < 1e-3, "{total}");
}

#[test]
fn ca5_sans_recouvrement_la_somme_des_pages_egale_le_trace_non_tuile() {
    // Recouvrement 0 : colonnes ceil(265 / 190) = 2, lignes ceil(660 / 269) = 3 ; aucune arête
    // de la jupe ne tombe sur une limite de fenêtre (x = 185 ; y = 117 et 386) : aucun doublon.
    let page = PageSetup {
        overlap_mm: 0.0,
        ..PageSetup::default()
    };
    let t = pages_of(&jupe(), &page);
    assert_eq!(t.pages.len(), 6);
    let total: f64 = t.pages.iter().map(|p| len_mm(&p.text)).sum();
    // Le numéro de page n'est pas un tracé (texte LB) : seules les 4 arêtes comptent.
    assert!((total - 1810.0).abs() < 1e-3, "{total}");
    // Même longueur que l'export non tuilé de la même scène sur un grand rouleau.
    let big = PageSetup {
        format: PageFormat::Roll { width_mm: 914.0 },
        ..PageSetup::default()
    };
    let single = export_hpgl(&jupe(), &big).unwrap();
    assert!((len_mm(&single.text) - total).abs() < 1e-3);
}

#[test]
fn ca3_numero_de_page_ascii_au_dessus_de_la_fenetre() {
    let t = pages_of(&jupe(), &PageSetup::default());
    for p in &t.pages {
        assert!(p.text.is_ascii());
        let r = read_hpgl(&p.text).unwrap();
        assert_eq!(r.labels.len(), 1);
        let l = &r.labels[0];
        assert_eq!(l.text, format!("{}/6 {}", p.number, p.id));
        // Plume 7 ; ligne de base à (0 ; 269 + 2,5 mm) : dans le bandeau de 8 mm du PDF.
        assert_eq!((l.pen, l.at), (7, (0, 10860)));
    }
    assert!(t.pages[1].text.contains("LB2/6 L1C2\u{3};"));
}

#[test]
fn ca3_reperes_croix_aux_memes_points_que_le_pdf() {
    let page = PageSetup {
        assembly: AssemblyOptions {
            marks: true,
            ..AssemblyOptions::default()
        },
        ..PageSetup::default()
    };
    let t = pages_of(&jupe(), &page);
    // L1C1 : voisines à droite (L1C2) et en dessous (L2C1). Bande droite : x = 175 + 3 et
    // 175 + 10 - 3 → 178 et 182 (relatif -5 : 183 et 187), y = 386 + 6 et 386 + 269 - 6 (rel 6 et 263).
    // Bande basse : y = 386 + 3 et 386 + 10 - 3 (rel 3 et 10), x = -5 + 6 et -5 + 190 - 6 (rel 6 et 184).
    // 8 croix de 2 × 6 mm = 96 mm de plume 8.
    let p1 = segments(&t.pages[0].text, 8);
    assert_eq!(p1.len(), 16);
    // Croix de (183 ; 6) : (180 ; 6)-(186 ; 6) et (183 ; 3)-(183 ; 9), en unités.
    assert!(p1.contains(&seg((7200, 240), (7440, 240))));
    assert!(p1.contains(&seg((7320, 120), (7320, 360))));
    let len8 = |text: &str| -> f64 {
        read_hpgl(text)
            .unwrap()
            .strokes
            .iter()
            .filter(|s| s.pen == 8)
            .map(|s| s.length_mm())
            .sum()
    };
    assert!((len8(&t.pages[0].text) - 96.0).abs() < 1e-3);
    // L1C2 : gauche et dessous = 8 croix ; L2C2 : gauche, dessus, dessous = 12 croix.
    assert!((len8(&t.pages[1].text) - 96.0).abs() < 1e-3);
    assert!((len8(&t.pages[3].text) - 144.0).abs() < 1e-3);
    // Sans l'option, aucune croix.
    let n = pages_of(&jupe(), &PageSetup::default());
    assert!(n.pages.iter().all(|p| len8(&p.text) == 0.0));
}

#[test]
fn ca1_une_feuille_qui_tient_sur_une_page_est_identique_a_l_export_non_tuile() {
    let s = rect_scene(100.0, 60.0);
    let page = PageSetup::default();
    let t = pages_of(&s, &page);
    assert_eq!(t.pages.len(), 1);
    let plain = export_hpgl_with(&s, &page, HpglOptions::default()).unwrap();
    assert_eq!(t.pages[0].text, plain.text);
    assert_eq!(t.pages[0].report, plain.report);
    assert!(!t.pages[0].text.contains("LB"));
}

#[test]
fn ca1_un_rouleau_n_est_jamais_decoupe() {
    let page = PageSetup {
        format: PageFormat::Roll { width_mm: 914.0 },
        ..PageSetup::default()
    };
    let t = pages_of(&jupe(), &page);
    assert_eq!(t.pages.len(), 1);
    assert_eq!(t.pages[0].text, export_hpgl(&jupe(), &page).unwrap().text);
}

#[test]
fn ca2_paysage_marge_et_recouvrement_respectes() {
    // A4 paysage, marges 15, recouvrement 20 : fenêtre (297 - 30) × (210 - 30 - 8) = 267 × 172.
    // Rectangle 600 × 300 : emprise + blanc = 610 × 310 ; colonnes ceil(590 / 247) = 3 ;
    // lignes ceil(290 / 152) = 2 ; 6 pages.
    let page = PageSetup {
        orientation: pf_export::Orientation::Landscape,
        margins: Margins::uniform(15.0),
        overlap_mm: 20.0,
        ..PageSetup::default()
    };
    let s = rect_scene(600.0, 300.0);
    let t = pages_of(&s, &page);
    assert_eq!(t.pages.len(), 6);
    let g = grid_of((0.0, 0.0), (600.0, 300.0), 267.0, 172.0, 20.0);
    assert_eq!((g.cols, g.rows), (3, 2));
    let max = (267.0f64 * 40.0) as i64;
    for p in &t.pages {
        let r = read_hpgl(&p.text).unwrap();
        assert!(r
            .strokes
            .iter()
            .flat_map(|s| &s.pts)
            .all(|q| (0..=max).contains(&q.0) && (0..=(172 * 40)).contains(&q.1)));
        // Fenêtre (col ; ligne) : coin bas-gauche attendu par l'oracle de grille.
        let (wx, wy) = g.wmin(p.col, p.row);
        // Un point connu de l'arête gauche du rectangle (x = 0) est à (0 - wx) mm sur la page.
        if wx < 0.0 && wx + 267.0 > 0.0 && wy < 100.0 && wy + 172.0 > 100.0 {
            let want_x = ((0.0 - wx) * 40.0).round() as i64;
            assert!(r
                .strokes
                .iter()
                .any(|s| s.pts.iter().all(|q| q.0 == want_x) && s.pts.len() >= 2));
        }
    }
}

#[test]
fn ca2_facteurs_x_y_appliques_comme_pour_le_pdf() {
    // Facteur X = 1,05 : rectangle 255 × 650 → 267,75 × 650 en mm papier ; fenêtre inchangée.
    // Colonnes ceil((267,75 + 10 - 10) / 180) = 2 ; lignes 3.
    let page = PageSetup {
        scale: pf_export::PrintScale { x: 1.05, y: 1.0 },
        ..PageSetup::default()
    };
    let s = jupe();
    let t = pages_of(&s, &page);
    let tiles = plan_tiles_with(&s, &page).unwrap();
    assert_eq!(t.pages.len(), tiles.len());
    assert_eq!(t.pages.len(), 6);
    // Périmètre en mm papier : 2 × (267,75 + 650) = 1835,5. Recouvrements doublés : 2 bandes
    // entre lignes (2 arêtes verticales × 10 mm chacune = 40) et 1 bande entre colonnes (2 arêtes
    // horizontales × 10 mm = 20) → 1835,5 + 60 = 1895,5.
    let total: f64 = t.pages.iter().map(|p| len_mm(&p.text)).sum();
    assert!((total - 1895.5).abs() < 0.2, "{total}");
}

#[test]
fn ca2_texte_garde_sur_toute_page_dont_la_fenetre_contient_le_debut_de_la_ligne() {
    // Étiquette « PIECE-TEST » centrée en (191 ; 133,4) : largeur approchée 10 × 4 × 0,55 = 22 mm,
    // début de la ligne de base = (191 - 11 ; 133,4 - 1,4) = (180 ; 132). Ce point est dans les
    // bandes de recouvrement des deux colonnes (x de 175 à 185) et des lignes 2 et 3 (y de 127
    // à 137) : le texte est écrit sur L2C1, L2C2, L3C1, L3C2 (pages 3 à 6), pas sur L1.
    let mut s = jupe();
    s.piece_labels.push(pf_export::ScenePieceLabel {
        piece: "P".into(),
        center: common::pt(191.0, 133.4),
        lines: vec!["PIECE-TEST".into()],
    });
    let t = pages_of(&s, &PageSetup::default());
    assert_eq!(t.pages.len(), 6);
    let has: Vec<bool> = t
        .pages
        .iter()
        .map(|p| {
            read_hpgl(&p.text)
                .unwrap()
                .labels
                .iter()
                .any(|l| l.text == "PIECE-TEST")
        })
        .collect();
    assert_eq!(has, [false, false, true, true, true, true]);
    // Position sur L2C2 (175 ; 127) : (180 - 175 ; 132 - 127) = (5 ; 5) mm = (200 ; 200) unités.
    let r = read_hpgl(&t.pages[3].text).unwrap();
    let l = r.labels.iter().find(|l| l.text == "PIECE-TEST").unwrap();
    assert_eq!(l.at, (200, 200));
}

#[test]
fn ca5_refus_propres_sans_trace() {
    let page = PageSetup::default();
    // Rien à tracer.
    let empty = scene_of(vec![]);
    assert!(matches!(
        export_hpgl_pages(&empty, &page, HpglOptions::default()),
        Err(HpglError::Nothing)
    ));
    // Coordonnée non finie.
    let nan = scene_of(vec![PathGeom::Line(
        common::pt(0.0, 0.0),
        common::pt(f64::NAN, 5.0),
    )]);
    assert!(matches!(
        export_hpgl_pages(&nan, &page, HpglOptions::default()),
        Err(HpglError::NotFinite)
    ));
    // Trop de pages (mêmes refus que le PDF : 80 au plus).
    let huge = scene_of(rect_at(0.0, 0.0, 4000.0, 4000.0));
    assert!(matches!(
        export_hpgl_pages(&huge, &page, HpglOptions::default()),
        Err(HpglError::Tiling(
            pf_export::ExportError::TooManyPages { .. }
        ))
    ));
    // Réglage de page absurde : recouvrement hors bornes, marge hors bornes.
    for bad in [
        PageSetup {
            overlap_mm: 80.0,
            ..PageSetup::default()
        },
        PageSetup {
            margins: Margins::uniform(150.0),
            ..PageSetup::default()
        },
    ] {
        let e = export_hpgl_pages(&jupe(), &bad, HpglOptions::default()).unwrap_err();
        assert!(matches!(e, HpglError::Page(_)), "{e:?}");
    }
    assert!(matches!(
        export_hpgl_pages(
            &jupe(),
            &PageSetup {
                overlap_mm: 50.0,
                margins: Margins::uniform(100.0),
                ..PageSetup::default()
            },
            HpglOptions::default()
        ),
        Err(HpglError::Page(PageSetupError::PrintableTooSmall { .. }))
    ));
}

#[test]
fn ca1_le_plan_d_assemblage_est_ignore_avec_une_remarque() {
    let page = PageSetup {
        assembly: AssemblyOptions {
            plan: true,
            ..AssemblyOptions::default()
        },
        ..PageSetup::default()
    };
    let t = pages_of(&jupe(), &page);
    assert_eq!(t.pages.len(), 6);
    assert!(t.plan_ignored);
}

#[test]
fn ca1_economiser_le_papier_omet_les_pages_vides_sans_rotation() {
    // Deux carrés éloignés : la grille a des cases vides ; elles sont omises (numérotation de la
    // grille conservée), la feuille n'est jamais tournée.
    let mut paths = rect_at(0.0, 0.0, 50.0, 50.0);
    paths.extend(rect_at(400.0, 800.0, 50.0, 50.0));
    let s = scene_of(paths);
    let page = PageSetup {
        assembly: AssemblyOptions {
            save_paper: true,
            ..AssemblyOptions::default()
        },
        ..PageSetup::default()
    };
    let t = pages_of(&s, &page);
    assert_eq!(t.pages.len(), 2);
    let tiles = pf_export::pdf::plan_tiles_unrotated(&s, &page).unwrap();
    assert_eq!(tiles.len(), 2);
    assert!(tiles.iter().all(|k| k.turns == 0));
    for (p, k) in t.pages.iter().zip(&tiles) {
        assert_eq!((p.col, p.row), (k.col, k.row));
    }
}
