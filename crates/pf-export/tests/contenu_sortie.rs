//! US-13.4 — contenu de la sortie : choix des catégories écrites dans le PDF et le SVG.
//!
//! Un test au moins par critère d'acceptation (CA1 à CA5, CA8 ; l'aperçu CA6, la persistance CA7
//! et la fenêtre CA9 sont testés dans `apps/pf-desktop`). Les fichiers produits sont relus par
//! des lecteurs indépendants du code d'export (`common/contenu.rs`). Aucun test ne dit qu'un
//! rendu papier a été essayé : l'impression reste à valider par un humain (AT-09).

mod common;

use common::contenu::*;
use common::{read_pdf, Rng};
use pf_core::PathRole;
use pf_export::content::count;
use pf_export::pdf::{export_pdf, export_pdf_content, export_pdf_with, PdfExport};
use pf_export::svg::export_svg;
use pf_export::{
    ContentNote, ExportError, OutputCategory as C, OutputContent, OutputSize, PageSetup, Scene,
    SceneOptions,
};

fn rich() -> (pf_core::Project, pf_core::SheetId) {
    project(Features::all(), 200.0, 120.0)
}

fn full_scene() -> Scene {
    let (p, s) = rich();
    Scene::from_sheet(&p, s).unwrap()
}

/// Feuille qui tient sur une seule page A4 (fenêtre de 190 × 269 mm).
fn small_scene() -> Scene {
    let (p, s) = project(Features::all(), 120.0, 90.0);
    Scene::from_sheet(&p, s).unwrap()
}

/// PDF d'une scène déjà filtrée par `content`, A4 par défaut (comme l'application).
fn pdf_of(scene: &Scene, content: &OutputContent) -> PdfExport {
    export_pdf_content(std::slice::from_ref(scene), &PageSetup::default(), content).unwrap()
}

/// Comptage direct des champs de la scène, avec une table des rôles écrite ici (indépendante de
/// `OutputCategory::of_role`).
fn raw_count(scene: &Scene, c: C) -> usize {
    let role_is = |r: PathRole, cat: C| {
        matches!(
            (r, cat),
            (PathRole::Construction, C::Construction)
                | (PathRole::SewingContour, C::SewingContour)
                | (PathRole::CuttingContour, C::CuttingContour)
                | (PathRole::Grainline, C::Grainline)
                | (PathRole::AnnotationAxis, C::Axes)
                | (
                    PathRole::Fold
                        | PathRole::Topstitch
                        | PathRole::CenterFront
                        | PathRole::CenterBack
                        | PathRole::WaistLine
                        | PathRole::HipLine
                        | PathRole::BustLine
                        | PathRole::SkivingLine
                        | PathRole::GlueLine,
                    C::SpecialLines
                )
        )
    };
    match c {
        C::Notches => scene.notch_marks.len(),
        C::Markings => scene.markings.len(),
        C::Buttons => scene.buttons.len(),
        C::Points => scene.points.len(),
        C::PointLabels => scene.points.iter().filter(|p| p.show_label).count(),
        C::Dimensions => scene.dimensions.len(),
        C::FullDimensions => scene.full_dimensions.len(),
        C::Darts => scene.darts.len(),
        C::PieceLabels => scene.piece_labels.len(),
        role_cat => scene
            .paths
            .iter()
            .filter(|p| role_is(p.role, role_cat))
            .count(),
    }
}

// ---------------------------------------------------------------------------------------------
// CA1 — choix des calques
// ---------------------------------------------------------------------------------------------

#[test]
fn ca1_chaque_categorie_correspond_a_des_elements_existants_de_la_scene() {
    let scene = full_scene();
    for c in C::ALL {
        assert!(raw_count(&scene, c) > 0, "catégorie sans élément : {c:?}");
        assert_eq!(count(&scene, c), raw_count(&scene, c), "{c:?}");
    }
    // Les 14 rôles ont une catégorie ; les six catégories de rôle sont toutes atteintes.
    let mut hit = std::collections::HashSet::new();
    for r in PathRole::ALL {
        hit.insert(C::of_role(r));
    }
    assert_eq!(hit.len(), 6, "{hit:?}");
    assert_eq!(PathRole::ALL.len(), 14);
}

#[test]
fn ca1_exclure_une_categorie_retire_exactement_ses_elements_et_garde_les_autres() {
    let scene = full_scene();
    for c in C::ALL {
        let content = OutputContent::default().without(c);
        let out = content.apply(&scene).unwrap();
        for other in C::ALL {
            let expected = if other == c || (c == C::Points && other == C::PointLabels) {
                0
            } else {
                raw_count(&scene, other)
            };
            assert_eq!(
                raw_count(&out, other),
                expected,
                "sans {c:?} : catégorie {other:?}"
            );
        }
        // Les éléments conservés sont identiques (coordonnées, ordre) à ceux de la scène complète.
        let kept: Vec<_> = scene
            .paths
            .iter()
            .filter(|p| C::of_role(p.role) != c)
            .cloned()
            .collect();
        assert_eq!(out.paths, kept, "sans {c:?}");
        if c != C::Notches {
            assert_eq!(out.notch_marks, scene.notch_marks);
        }
        if c != C::Dimensions {
            assert_eq!(out.dimensions, scene.dimensions);
        }
        if c != C::FullDimensions {
            assert_eq!(out.full_dimensions, scene.full_dimensions);
        }
        // Le droit-fil conserve ses contraintes de rotation même exclu.
        assert_eq!(out.grain_limits, scene.grain_limits);
    }
}

#[test]
fn ca1_le_svg_ne_contient_plus_la_categorie_exclue_seulement() {
    let scene = full_scene();
    let full = read_svg(&export_svg(&scene).unwrap());
    for c in C::ALL {
        let out = OutputContent::default().without(c).apply(&scene).unwrap();
        let svg = read_svg(&export_svg(&out).unwrap());
        for other in C::ALL {
            let removed = other == c || (c == C::Points && other == C::PointLabels);
            let expected = if removed { 0 } else { full.count(other) };
            assert_eq!(svg.count(other), expected, "sans {c:?} : {other:?}");
        }
    }
}

// ---------------------------------------------------------------------------------------------
// CA2 — couture et/ou coupe
// ---------------------------------------------------------------------------------------------

fn roles_in(svg: &str) -> Vec<String> {
    let mut r: Vec<String> = read_svg(svg)
        .elems
        .iter()
        .filter_map(|e| e.role.clone())
        .collect();
    r.sort();
    r.dedup();
    r
}

#[test]
fn ca2_couture_seule_coupe_seule_ou_les_deux() {
    let scene = full_scene();
    let both = roles_in(&export_svg(&scene).unwrap());
    assert!(both.contains(&"couture".to_string()) && both.contains(&"coupe".to_string()));
    let sew = OutputContent::sewing_only().apply(&scene).unwrap();
    let r = roles_in(&export_svg(&sew).unwrap());
    assert!(
        r.contains(&"couture".to_string()) && !r.contains(&"coupe".to_string()),
        "{r:?}"
    );
    let cut = OutputContent::cutting_only().apply(&scene).unwrap();
    let r = roles_in(&export_svg(&cut).unwrap());
    assert!(
        r.contains(&"coupe".to_string()) && !r.contains(&"couture".to_string()),
        "{r:?}"
    );
    // Même chose dans le PDF : la légende de la page étalon n'annonce pas le rôle exclu.
    let text = |s: &Scene, c: &OutputContent| read_pdf(&pdf_of(s, c).bytes)[0].text.clone();
    let (cs, cc, cd) = (
        OutputContent::sewing_only(),
        OutputContent::cutting_only(),
        OutputContent::default(),
    );
    assert!(
        text(&sew, &cs).contains("Ligne de couture") && !text(&sew, &cs).contains("Ligne de coupe")
    );
    assert!(
        text(&cut, &cc).contains("Ligne de coupe") && !text(&cut, &cc).contains("Ligne de couture")
    );
    assert!(
        text(&scene, &cd).contains("Ligne de coupe")
            && text(&scene, &cd).contains("Ligne de couture")
    );
}

#[test]
fn ca2_pas_de_contour_de_coupe_invente_quand_il_n_y_a_pas_de_marge_de_couture() {
    // Pièce dessinée sans aucun tracé de rôle coupe (aucune marge de couture n'existe).
    let f = Features {
        cutting: false,
        ..Features::all()
    };
    let (p, s) = project(f, 200.0, 120.0);
    let scene = Scene::from_sheet(&p, s).unwrap();
    assert_eq!(raw_count(&scene, C::CuttingContour), 0);
    let content = OutputContent::cutting_only();
    let out = content.apply(&scene).unwrap();
    assert_eq!(raw_count(&out, C::CuttingContour), 0, "rien n'est inventé");
    assert_eq!(
        out.paths.len(),
        scene.paths.len() - raw_count(&scene, C::SewingContour)
    );
    let summary = content.summary(&scene);
    assert!(
        summary.notes.contains(&ContentNote::CuttingAbsent),
        "{summary:?}"
    );
    // Le reste de la feuille reste exporté : le refus n'a lieu que si plus rien ne reste.
    assert!(export_svg(&out).is_ok());
    // Symétrique : couture seule sans tracé de couture.
    let f = Features {
        sewing: false,
        ..Features::all()
    };
    let (p, s) = project(f, 200.0, 120.0);
    let scene = Scene::from_sheet(&p, s).unwrap();
    let content = OutputContent::sewing_only();
    assert!(content
        .summary(&scene)
        .notes
        .contains(&ContentNote::SewingAbsent));
    assert_eq!(
        raw_count(&content.apply(&scene).unwrap(), C::SewingContour),
        0
    );
}

#[test]
fn ca2_les_deux_contours_exclus_ne_laissent_que_le_reste() {
    let scene = full_scene();
    let out = OutputContent::default()
        .without(C::CuttingContour)
        .without(C::SewingContour)
        .apply(&scene)
        .unwrap();
    assert_eq!(
        raw_count(&out, C::CuttingContour) + raw_count(&out, C::SewingContour),
        0
    );
    assert!(raw_count(&out, C::Notches) > 0);
}

// ---------------------------------------------------------------------------------------------
// CA3 — cotations et étiquettes, légende cohérente
// ---------------------------------------------------------------------------------------------

#[test]
fn ca3_mesures_cotes_completes_et_noms_de_points_s_activent_separement() {
    let scene = full_scene();
    let full = read_svg(&export_svg(&scene).unwrap());
    let (dims, cotes, labels) = (
        full.texts(C::Dimensions),
        full.texts(C::FullDimensions),
        full.texts(C::PointLabels),
    );
    assert!(!dims.is_empty() && !cotes.is_empty() && !labels.is_empty());
    for (excluded, gone) in [
        (C::Dimensions, &dims),
        (C::FullDimensions, &cotes),
        (C::PointLabels, &labels),
    ] {
        let out = OutputContent::default()
            .without(excluded)
            .apply(&scene)
            .unwrap();
        let svg_text = export_svg(&out).unwrap();
        let svg = read_svg(&svg_text);
        for other in [C::Dimensions, C::FullDimensions, C::PointLabels] {
            let expected = if other == excluded {
                0
            } else {
                full.texts(other).len()
            };
            assert_eq!(svg.texts(other).len(), expected, "sans {excluded:?}");
        }
        // Noms de points exclus : les repères restent.
        assert_eq!(svg.count(C::Points), full.count(C::Points));
        // PDF : les textes de l'élément exclu ne sont plus écrits, les autres le restent.
        let pdf = pdf_of(&out, &OutputContent::default().without(excluded)).bytes;
        let strings: Vec<String> = read_pdf(&pdf)
            .iter()
            .flat_map(|p| p.strings.clone())
            .collect();
        for t in gone.iter().filter(|t| t.is_ascii()) {
            // Un texte exclu n'apparaît plus, sauf s'il est aussi celui d'une autre catégorie.
            let elsewhere = [C::Dimensions, C::FullDimensions, C::PointLabels]
                .iter()
                .filter(|o| **o != excluded)
                .any(|o| full.texts(*o).contains(t));
            if !elsewhere {
                assert!(
                    !strings.contains(t),
                    "« {t} » encore écrit sans {excluded:?}"
                );
            }
        }
        // Les textes des autres catégories restent écrits (ASCII : l'accent n'est pas relu).
        for other in [C::Dimensions, C::FullDimensions, C::PointLabels] {
            if other == excluded {
                continue;
            }
            for t in full.texts(other).iter().filter(|t| t.is_ascii()) {
                assert!(strings.contains(t), "« {t} » a disparu sans {excluded:?}");
            }
        }
    }
}

#[test]
fn ca3_les_noms_de_points_dependent_des_points() {
    let scene = full_scene();
    let content = OutputContent::default().without(C::Points);
    let out = content.apply(&scene).unwrap();
    assert!(out.points.is_empty());
    // Points exclus et noms demandés : aucun nom, et la remarque le dit.
    assert!(content
        .summary(&scene)
        .notes
        .contains(&ContentNote::LabelsWithoutPoints));
    assert_eq!(raw_count(&out, C::PointLabels), 0);
    let only_labels_off = OutputContent::default()
        .without(C::PointLabels)
        .apply(&scene)
        .unwrap();
    assert_eq!(only_labels_off.points.len(), scene.points.len());
    assert!(only_labels_off.points.iter().all(|p| !p.show_label));
}

#[test]
fn ca3_la_legende_de_l_etalon_ne_montre_pas_un_role_exclu() {
    let scene = full_scene();
    let legend = |content: OutputContent| -> String {
        let out = content.apply(&scene).unwrap();
        read_pdf(&pdf_of(&out, &content).bytes)[0].text.clone()
    };
    let all = legend(OutputContent::default());
    for needle in [
        "Construction —",
        "Ligne de couture",
        "Ligne de coupe",
        "Droit-fil —",
        "Pliure —",
        "Axe de repère",
        "Surpiqûre —",
        "Marquage —",
        "Bouton (cercle)",
        "Sens du droit-fil",
    ] {
        assert!(all.contains(needle), "légende par défaut sans « {needle} »");
    }
    for (excluded, needles) in [
        (C::Construction, &["Construction —"][..]),
        (C::SewingContour, &["Ligne de couture"]),
        (C::CuttingContour, &["Ligne de coupe"]),
        (C::Grainline, &["Droit-fil —", "Sens du droit-fil"]),
        (C::SpecialLines, &["Pliure —", "Surpiqûre —"]),
        (C::Axes, &["Axe de repère"]),
        (C::Markings, &["Marquage —"]),
        (C::Buttons, &["Bouton (cercle)"]),
    ] {
        let text = legend(OutputContent::default().without(excluded));
        for n in needles {
            assert!(
                !text.contains(n),
                "sans {excluded:?}, la légende annonce « {n} »"
            );
        }
        // Les autres rôles restent annoncés.
        for n in ["Ligne de couture", "Ligne de coupe"] {
            if !needles.contains(&n) {
                assert!(text.contains(n), "sans {excluded:?}, « {n} » a disparu");
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// CA4 — PDF et SVG, échelle, coordonnées, grille de pages
// ---------------------------------------------------------------------------------------------

fn tile_lines(bytes: &[u8]) -> Vec<Vec<String>> {
    read_pdf_streams(bytes)
        .tiles
        .into_iter()
        .map(|t| t.clip)
        .collect()
}

#[test]
fn ca4_a_boite_identique_coordonnees_et_pages_ne_changent_pas() {
    let scene = small_scene();
    let full_svg = read_svg(&export_svg(&scene).unwrap());
    let full_pdf = pdf_of(&scene, &OutputContent::default());
    // Catégories intérieures à la pièce : la boîte englobante ne bouge pas.
    for c in [
        C::Markings,
        C::Buttons,
        C::Grainline,
        C::Axes,
        C::SewingContour,
        C::Notches,
    ] {
        let out = OutputContent::default().without(c).apply(&scene).unwrap();
        assert_eq!(out.bbox(), scene.bbox(), "sans {c:?}");
        let svg = read_svg(&export_svg(&out).unwrap());
        assert_eq!((svg.width, svg.height), (full_svg.width, full_svg.height));
        let shift = same_up_to_shift(&full_svg, &svg, |k| {
            k != c && !(c == C::Points && k == C::PointLabels)
        })
        .unwrap();
        assert_eq!(
            shift.map(|(a, b)| (a.abs() < 1e-9, b.abs() < 1e-9)),
            Some((true, true))
        );
        let pdf = pdf_of(&out, &OutputContent::default().without(c));
        assert_eq!(pdf.page_count, full_pdf.page_count, "sans {c:?}");
        // Chaque ligne conservée est identique, au même endroit, dans le même ordre.
        let (a, b) = (tile_lines(&full_pdf.bytes), tile_lines(&pdf.bytes));
        assert_eq!(a.len(), 1, "une seule page de patron attendue");
        assert_eq!(a.len(), b.len());
        for (big, small) in a.iter().zip(&b) {
            assert!(
                is_subsequence(small, big),
                "sans {c:?} : lignes du PDF modifiées"
            );
            assert!(small.len() < big.len(), "sans {c:?} : rien de retiré");
        }
    }
}

#[test]
fn ca4_quand_la_boite_change_le_decoupage_suit_le_contenu_retenu() {
    // Pièce de 700 × 120 mm : plusieurs pages ; la construction est sous la pièce (de 25 à
    // 40 mm de hauteur de plus).
    let (p, s) = project(Features::all(), 700.0, 120.0);
    let scene = Scene::from_sheet(&p, s).unwrap();
    // Les points de la construction (centre du cercle, bouts du segment) sont retirés avec elle.
    let content = OutputContent::default()
        .without(C::Construction)
        .without(C::Points);
    let out = content.apply(&scene).unwrap();
    let (bb_full, bb_out) = (scene.bbox().unwrap(), out.bbox().unwrap());
    assert!(bb_out.height() < bb_full.height() - 20.0);
    let (a, b) = (
        read_svg(&export_svg(&scene).unwrap()),
        read_svg(&export_svg(&out).unwrap()),
    );
    assert!(b.height < a.height - 20.0);
    // Les éléments conservés sont les mêmes à une translation verticale près.
    let gone = |k: C| matches!(k, C::Construction | C::Points | C::PointLabels);
    let shift = same_up_to_shift(&a, &b, |k| !gone(k)).unwrap().unwrap();
    assert!(shift.0.abs() < 1e-9, "{shift:?}");
    assert!(
        (shift.1.abs() - (a.height - b.height)).abs() < 1e-3 || shift.1.abs() < 1e-9,
        "{shift:?}"
    );
    // Pages : celles du contenu retenu (A4 : 190 × 269 mm de fenêtre, recouvrement 10).
    let pdf = read_pdf_streams(&pdf_of(&out, &content).bytes);
    let cover = |len: f64, window: f64| {
        let mut n = 1usize;
        while window + (n - 1) as f64 * (window - 10.0) < len - 1e-9 {
            n += 1;
        }
        n
    };
    let (w, h) = (b.width - 20.0 + 10.0, b.height - 20.0 + 10.0);
    assert_eq!(pdf.tiles.len(), cover(w, 190.0) * cover(h, 269.0));
}

#[test]
fn ca4_pdf_et_svg_utilisent_la_meme_selection_et_l_echelle_reste_1_pour_1() {
    let scene = small_scene();
    for content in [
        OutputContent::sewing_only(),
        OutputContent::cutting_only(),
        OutputContent::default()
            .without(C::Dimensions)
            .without(C::Markings),
        OutputContent::only(&[C::CuttingContour, C::Notches]),
    ] {
        let out = content.apply(&scene).unwrap();
        let svg = read_svg(&export_svg(&out).unwrap());
        let pdf = read_pdf_streams(&pdf_of(&out, &content).bytes);
        // Une page : mêmes nombres d'éléments dessinés dans le SVG et le PDF (formules
        // de décompte écrites ici : un cercle de marquage = 1, une croix = 2 traits).
        assert_eq!(pdf.tiles.len(), 1);
        let marking_ops: usize = svg
            .of(C::Markings)
            .iter()
            .map(|e| {
                if e.form.as_deref() == Some("croix") {
                    2
                } else {
                    1
                }
            })
            .sum();
        let expected = svg.count(C::Construction)
            + svg.count(C::SewingContour)
            + svg.count(C::CuttingContour)
            + svg.count(C::Grainline)
            + svg.count(C::SpecialLines)
            + svg.count(C::Axes)
            + svg.count(C::Notches)
            + marking_ops
            + svg.count(C::Buttons)
            + svg.of(C::Dimensions).len()
            + svg.of(C::FullDimensions).len()
            + 2 * svg.count(C::Points)
            + svg.count(C::PointLabels)
            // Une jambe de pince = un trait ; une ligne d'étiquette = un texte.
            + svg.count(C::Darts)
            + svg.count(C::PieceLabels);
        assert_eq!(pdf.tiles[0].ops(), expected, "{content:?}");
        // Échelle 1:1 : le côté de 120 mm de la pièce mesure 120 mm dans le SVG (unité = mm).
        if content.includes(C::CuttingContour) {
            let longest = svg
                .of(C::CuttingContour)
                .iter()
                .map(|e| (e.pts[1].0 - e.pts[0].0).hypot(e.pts[1].1 - e.pts[0].1))
                .fold(0.0, f64::max);
            assert!((longest - 120.0).abs() < 1e-3, "{longest}");
        }
    }
}

#[test]
fn ca4_la_page_etalon_et_ses_reperes_ne_sont_pas_filtres() {
    let scene = full_scene();
    let default = pdf_of(&scene, &OutputContent::default());
    let only = OutputContent::only(&[C::CuttingContour]);
    let thin = pdf_of(&only.apply(&scene).unwrap(), &only);
    let (a, b) = (read_pdf(&default.bytes), read_pdf(&thin.bytes));
    // Le carré étalon de 100 mm est identique : même rectangle de 100 × 100 mm au même endroit.
    let squares = |p: &common::Page| -> Vec<(i64, i64, i64, i64)> {
        p.rects
            .iter()
            .filter(|r| (r.2 - 100.0).abs() < 0.01 && (r.3 - 100.0).abs() < 0.01)
            .map(|r| {
                let q = |v: f64| (v * 100.0).round() as i64;
                (q(r.0), q(r.1), q(r.2), q(r.3))
            })
            .collect()
    };
    assert_eq!(squares(&a[0]).len(), 1);
    assert_eq!(squares(&a[0]), squares(&b[0]));
    assert!(b[0].has("Export expérimental tant que cet étalon n'a pas été mesuré"));
}

// ---------------------------------------------------------------------------------------------
// CA5 — défaut = comportement actuel, à l'octet
// ---------------------------------------------------------------------------------------------

#[test]
fn ca5_le_contenu_par_defaut_donne_les_memes_octets_que_l_export_sans_filtre() {
    let default = OutputContent::default();
    assert!(default.is_default() && default.excluded().is_empty());
    assert_eq!(default.size, OutputSize::ProjectSize);
    let mut projects = vec![
        pf_core::demo_project().unwrap(),
        pf_core::demo_piece_project().unwrap(),
        rich().0,
    ];
    projects.push(project(Features::all(), 700.0, 120.0).0);
    for p in &projects {
        for sheet in &p.sheets {
            let Ok(raw) = Scene::from_sheet(p, sheet.id) else {
                continue;
            };
            let via = default.scene(p, sheet.id, SceneOptions::default()).unwrap();
            assert_eq!(via, raw);
            assert_eq!(export_svg(&via).unwrap(), export_svg(&raw).unwrap());
            for setup in [PageSetup::default(), PageSetup::default()] {
                assert_eq!(
                    export_pdf_with(std::slice::from_ref(&via), &setup)
                        .unwrap()
                        .bytes,
                    export_pdf_with(std::slice::from_ref(&raw), &setup)
                        .unwrap()
                        .bytes
                );
            }
        }
    }
}

#[test]
fn ca5_les_fichiers_livres_dans_examples_sont_les_sorties_du_contenu_par_defaut() {
    let examples = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let read = |n: &str| std::fs::read(examples.join(n)).unwrap();
    let default = OutputContent::default();
    for (project, svg_name, pdf_name) in [
        (
            pf_core::demo_project().unwrap(),
            "geometrie-exercice.svg",
            "geometrie-exercice.pdf",
        ),
        (
            pf_core::demo_piece_project().unwrap(),
            "piece-crans-droit-fil.svg",
            "piece-crans-droit-fil.pdf",
        ),
    ] {
        let scene = default
            .scene(&project, project.sheets[0].id, SceneOptions::default())
            .unwrap();
        assert_eq!(read(svg_name), export_svg(&scene).unwrap().into_bytes());
        assert_eq!(read(pdf_name), export_pdf(&[scene]).unwrap().bytes);
    }
}

// ---------------------------------------------------------------------------------------------
// CA8 — contenu vide : refus propre, rien n'est modifié
// ---------------------------------------------------------------------------------------------

#[test]
fn ca8_une_selection_vide_est_refusee_proprement_sans_rien_modifier() {
    let (p, s) = rich();
    let before = p.clone();
    let nothing = C::ALL
        .into_iter()
        .fold(OutputContent::default(), |c, cat| c.without(cat));
    match nothing.scene(&p, s, SceneOptions::default()) {
        Err(ExportError::NothingSelected(name)) => assert_eq!(name, p.sheet(s).unwrap().name),
        other => panic!("refus attendu : {other:?}"),
    }
    let message = ExportError::NothingSelected("Feuille 1".into()).to_string();
    assert!(
        message.contains("Feuille 1") && message.contains("rien à exporter"),
        "{message}"
    );
    assert_eq!(p, before, "le projet est inchangé");
    // Une feuille de construction seule (rectangle et ses points) : exclure la construction et
    // les points ne laisse rien, même refus.
    let scene = {
        let (p, s) = project(
            Features {
                cutting: false,
                sewing: false,
                construction: false,
                notches: false,
                markings: false,
                buttons: false,
                grain: false,
                topstitch: false,
                special: false,
                axes: false,
                dims: false,
                full_dims: false,
                extra_points: false,
                darts: false,
                label: false,
            },
            100.0,
            60.0,
        );
        Scene::from_sheet(&p, s).unwrap()
    };
    assert_eq!(raw_count(&scene, C::Construction), 4);
    let content = OutputContent::default().without(C::Construction);
    assert!(content.apply(&scene).is_ok(), "les points restent");
    let r = content.without(C::Points).apply(&scene);
    assert!(matches!(r, Err(ExportError::NothingSelected(_))), "{r:?}");
    // Le contenu par défaut d'une feuille vide reste « feuille vide » (message distinct).
    let empty = pf_core::Project::new("Vide");
    let id = empty.sheets[0].id;
    assert!(matches!(
        OutputContent::default().scene(&empty, id, SceneOptions::default()),
        Err(ExportError::EmptySheet(_))
    ));
    assert!(matches!(
        nothing.scene(&empty, id, SceneOptions::default()),
        Err(ExportError::EmptySheet(_))
    ));
    // Le récapitulatif annonce « rien retenu » avant l'export.
    assert!(nothing
        .summary(&full_scene())
        .notes
        .contains(&ContentNote::NothingKept));
}

#[test]
fn ca8_un_seul_element_restant_suffit_a_exporter() {
    let scene = full_scene();
    for c in C::ALL.into_iter().filter(|c| *c != C::PointLabels) {
        let out = OutputContent::only(&[c]).apply(&scene).unwrap();
        assert!(export_svg(&out).is_ok(), "{c:?}");
        let only = OutputContent::only(&[c]);
        assert!(
            export_pdf_content(std::slice::from_ref(&out), &PageSetup::default(), &only).is_ok(),
            "{c:?}"
        );
    }
    // Les noms de points seuls ne laissent rien : sans les points, aucun nom n'est écrit.
    let r = OutputContent::only(&[C::PointLabels]).apply(&scene);
    assert!(matches!(r, Err(ExportError::NothingSelected(_))), "{r:?}");
}

// ---------------------------------------------------------------------------------------------
// Divers : taille, clés stables
// ---------------------------------------------------------------------------------------------

#[test]
fn la_taille_est_la_taille_unique_du_projet() {
    assert_eq!(OutputSize::default(), OutputSize::ProjectSize);
    assert_eq!(OutputSize::ProjectSize.key(), "project");
    assert_eq!(
        OutputSize::from_key("project"),
        Some(OutputSize::ProjectSize)
    );
    // Plusieurs tailles et nid n'existent pas : aucune autre valeur n'est acceptée.
    for other in ["all", "nest", "several", "", "Project"] {
        assert_eq!(OutputSize::from_key(other), None, "{other}");
    }
}

#[test]
fn les_cles_de_categories_sont_stables() {
    let keys: Vec<&str> = C::ALL.iter().map(|c| c.key()).collect();
    assert_eq!(
        keys,
        [
            "construction",
            "sewing",
            "cutting",
            "grainline",
            "special_lines",
            "axes",
            "notches",
            "markings",
            "buttons",
            "points",
            "point_labels",
            "dimensions",
            "full_dimensions",
            "darts",
            "piece_labels"
        ]
    );
}

#[test]
fn le_determinisme_octet_a_octet_vaut_pour_toute_selection() {
    let scene = full_scene();
    let mut rng = Rng(0x13_04);
    for _ in 0..20 {
        let mut c = OutputContent::default();
        for cat in C::ALL {
            c.set(cat, rng.below(3) != 0);
        }
        let (Ok(a), Ok(b)) = (c.apply(&scene), c.apply(&scene)) else {
            continue;
        };
        assert_eq!(export_svg(&a).unwrap(), export_svg(&b).unwrap());
        assert_eq!(
            export_pdf(std::slice::from_ref(&a)).unwrap().bytes,
            export_pdf(std::slice::from_ref(&b)).unwrap().bytes
        );
    }
}
