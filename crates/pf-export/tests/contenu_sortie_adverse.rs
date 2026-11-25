//! US-13.4 — campagne adverse du contenu de la sortie : 180 séquences à graine fixe qui combinent
//! des projets variés (pièce avec crans, marquages, boutons, droit-fil, surpiqûre, lignes
//! spéciales, cotations, cotes complètes, points ; d'une page à plusieurs ; parfois deux feuilles
//! par export, parfois le projet de démonstration), des sélections de catégories au hasard (dont
//! la sélection par défaut et la sélection vide) et les options de page d'US-13.1 à 13.3
//! (format, orientation, recouvrement, repères, plan, étalon renforcé, économie de papier,
//! facteurs X / Y).
//!
//! L'oracle est écrit sans le code d'export : les fichiers SVG et PDF sont relus par des lecteurs
//! indépendants (`common/contenu.rs`) ; les catégories des éléments du SVG viennent de leurs
//! identifiants de fichier ; la grille de pages est recalculée depuis la taille du SVG (boîte du
//! contenu retenu). Invariants vérifiés à chaque séquence : jamais de panique ; exclure une
//! catégorie retire exactement ses éléments (comptage par rôle ou par groupe dans le SVG), les
//! autres gardent leurs coordonnées à une translation constante près (nulle quand la boîte
//! englobante ne change pas) ; à boîte identique, le flux du PDF est une sous-suite exacte de
//! celui de l'export complet ; contenu par défaut = octets de l'export sans filtre ; PDF et SVG
//! d'accord entre eux (décompte des éléments dessinés, textes) ; nombre de pages = pages d'étalon,
//! plan et grille attendue ; déterminisme à l'octet ; sélection vide refusée proprement.
//!
//! Rien de ceci ne remplace une impression : AT-09 reste non prouvé.

mod common;

use common::contenu::*;
use common::{read_pdf, Rng};
use pf_export::pdf::{export_pdf_content, export_pdf_with, page_count_content};
use pf_export::svg::export_svg;
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

use pf_export::{
    AssemblyOptions, ExportError, Margins, Orientation, OutputCategory as C, OutputContent,
    PageFormat, PageSetup, PrintScale, Scene,
};

const SEQUENCES: u64 = 180;

/// Nombre de séquences qui ont réellement exécuté chaque contrôle fort (garde contre une
/// campagne qui passerait sans rien vérifier).
static GRID_CHECKS: AtomicUsize = AtomicUsize::new(0);
static OPS_CHECKS: AtomicUsize = AtomicUsize::new(0);
static SUBSEQUENCE_CHECKS: AtomicUsize = AtomicUsize::new(0);
static REFUSALS: AtomicUsize = AtomicUsize::new(0);

macro_rules! ck {
    ($seq:expr, $cond:expr, $($arg:tt)+) => {
        if !$cond {
            panic!("séquence {} : {}", $seq, format!($($arg)+));
        }
    };
}

/// Catégorie écrite pour cette sélection (les noms de points dépendent des points).
fn kept(content: &OutputContent, c: C) -> bool {
    match c {
        C::PointLabels => content.includes(C::Points) && content.includes(C::PointLabels),
        other => content.includes(other),
    }
}

fn random_content(rng: &mut Rng) -> OutputContent {
    match rng.below(12) {
        0 => OutputContent::default(),
        1 => C::ALL
            .into_iter()
            .fold(OutputContent::default(), |c, cat| c.without(cat)),
        2 => OutputContent::sewing_only(),
        3 => OutputContent::cutting_only(),
        _ => {
            let mut c = OutputContent::default();
            let odds = 2 + rng.below(4);
            for cat in C::ALL {
                c.set(cat, rng.below(odds) != 0);
            }
            c
        }
    }
}

/// Réglage de page, avec les dimensions de la fenêtre de patron (écrites ici, en dur).
fn random_setup(rng: &mut Rng) -> (PageSetup, f64, f64) {
    let (format, w, h) = match rng.below(3) {
        0 => (PageFormat::A4, 210.0, 297.0),
        1 => (PageFormat::A3, 297.0, 420.0),
        _ => (PageFormat::Letter, 215.9, 279.4),
    };
    let landscape = rng.below(3) == 0;
    let (pw, ph) = if landscape { (h, w) } else { (w, h) };
    let scale = if rng.below(3) == 0 {
        PrintScale { x: 1.0, y: 1.0 }
    } else {
        PrintScale {
            x: rng.range(0.95, 1.05),
            y: rng.range(0.95, 1.05),
        }
    };
    let setup = PageSetup {
        format,
        orientation: if landscape {
            Orientation::Landscape
        } else {
            Orientation::Portrait
        },
        margins: Margins::default(),
        overlap_mm: rng.pick(&[0.0, 10.0, 10.0, 20.0, 30.0]),
        assembly: AssemblyOptions {
            marks: rng.below(3) == 0,
            plan: rng.below(3) == 0,
            save_paper: rng.below(5) == 0,
        },
        scale,
        control_marks: rng.below(4) == 0,
    };
    (setup, pw - 20.0, ph - 20.0 - 8.0)
}

/// Pages nécessaires pour couvrir `len` mm : on compte jusqu'à couvrir (formulation propre).
fn cover(len: f64, window: f64, overlap: f64) -> usize {
    let mut n = 1usize;
    while window + (n - 1) as f64 * (window - overlap) < len - 1e-9 {
        n += 1;
    }
    n
}

/// Pages de patron attendues pour un SVG de contenu retenu, ou `None` si la valeur est trop près
/// d'une limite de page pour trancher (l'arrondi du SVG est de 1e-4 mm).
fn expected_tiles(svg: &Svg, setup: &PageSetup, ww: f64, wh: f64) -> Option<usize> {
    let (bw, bh) = (svg.width - 20.0, svg.height - 20.0);
    let at = |d: f64| {
        cover((bw + d) * setup.scale.x + 10.0, ww, setup.overlap_mm)
            * cover((bh + d) * setup.scale.y + 10.0, wh, setup.overlap_mm)
    };
    let (a, b) = (at(-6e-4), at(6e-4));
    (a == b).then_some(a)
}

struct Kept {
    full: Scene,
    filtered: Scene,
    census: Svg,
    full_census: Svg,
    /// Translation des éléments conservés et boîte du SVG inchangée.
    same_box: bool,
}

fn run(seq: u64) {
    let mut rng = Rng(0xA13C_4E77_90B1_5D29 ^ seq.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0x1304);
    for _ in 0..4 {
        rng.next();
    }
    let content = random_content(&mut rng);
    let (setup, ww, wh) = random_setup(&mut rng);
    let n_scenes = 1 + usize::from(rng.below(5) == 0);
    let mut kept_scenes: Vec<Kept> = Vec::new();
    let mut excluded_texts: Vec<String> = Vec::new();
    for k in 0..n_scenes {
        let (project, sheet) = if seq % 10 == 7 && k == 0 {
            let p = pf_core::demo_piece_project().unwrap();
            let s = p.sheets[0].id;
            (p, s)
        } else {
            let (p, s, _) = random_project(&mut rng);
            (p, s)
        };
        let before = project.clone();
        let full = Scene::from_sheet(&project, sheet).unwrap();
        let full_svg = export_svg(&full).unwrap();
        let full_census = read_svg(&full_svg);
        let retained: usize = C::ALL
            .iter()
            .filter(|c| kept(&content, **c))
            .map(|c| full_census.count(*c))
            .sum();
        let applied = content.apply(&full);
        ck!(seq, project == before, "le projet a changé");
        let filtered = match applied {
            Err(e) => {
                ck!(
                    seq,
                    retained == 0 && matches!(e, ExportError::NothingSelected(_)),
                    "refus inattendu {e:?} (éléments retenus : {retained})"
                );
                REFUSALS.fetch_add(1, Relaxed);
                continue;
            }
            Ok(f) => f,
        };
        ck!(seq, retained > 0, "export accepté sans rien à écrire");
        // Déterminisme et sélection par défaut.
        let again = content.apply(&full).unwrap();
        ck!(seq, again == filtered, "filtre non déterministe");
        let svg_text = export_svg(&filtered).unwrap();
        ck!(
            seq,
            svg_text == export_svg(&again).unwrap(),
            "SVG non déterministe"
        );
        if content.is_default() {
            ck!(seq, filtered == full, "défaut : scène modifiée");
            ck!(seq, svg_text == full_svg, "défaut : SVG différent");
        }
        // Exclure une catégorie retire exactement ses éléments.
        let census = read_svg(&svg_text);
        for c in C::ALL {
            let expected = if kept(&content, c) {
                full_census.count(c)
            } else {
                0
            };
            ck!(
                seq,
                census.count(c) == expected,
                "{c:?} : {} éléments au lieu de {expected} (exclus : {:?})",
                census.count(c),
                content.excluded()
            );
        }
        // Les éléments conservés gardent leurs coordonnées (translation constante).
        let shift = match same_up_to_shift(&full_census, &census, |c| kept(&content, c)) {
            Ok(s) => s,
            Err(e) => panic!("séquence {seq} : {e}"),
        };
        let (dx, dy) = shift.unwrap_or((0.0, 0.0));
        let same_box = dx.abs() < 1e-9
            && dy.abs() < 1e-9
            && (census.width - full_census.width).abs() < 1e-9
            && (census.height - full_census.height).abs() < 1e-9;
        // Le SVG est toujours en mm, boîte du contenu retenu + 2 × 10 mm.
        ck!(
            seq,
            census.width >= 20.0 && census.height >= 20.0,
            "SVG trop petit"
        );
        for c in [
            C::Dimensions,
            C::FullDimensions,
            C::PointLabels,
            C::PieceLabels,
        ] {
            if !kept(&content, c) {
                excluded_texts.extend(full_census.texts(c));
            }
        }
        kept_scenes.push(Kept {
            full,
            filtered,
            census,
            full_census,
            same_box,
        });
    }
    if kept_scenes.is_empty() {
        // Sélection vide partout : aucun export ne part (refus propre, vérifié ci-dessus).
        return;
    }

    // ----- PDF -----
    let scenes: Vec<Scene> = kept_scenes.iter().map(|k| k.filtered.clone()).collect();
    let out = export_pdf_content(&scenes, &setup, &content)
        .unwrap_or_else(|e| panic!("séquence {seq} : export refusé : {e}"));
    let again = export_pdf_content(&scenes, &setup, &content).unwrap();
    ck!(seq, out.bytes == again.bytes, "PDF non déterministe");
    ck!(
        seq,
        page_count_content(&scenes, &setup, &content).unwrap() == out.page_count,
        "nombre de pages estimé ≠ réel"
    );
    let pdf = read_pdf_streams(&out.bytes);
    ck!(
        seq,
        pdf.pages == out.page_count && pdf.streams.len() == pdf.pages,
        "pages : /Count {} flux {} annoncé {}",
        pdf.pages,
        pdf.streams.len(),
        out.page_count
    );
    let cal = pdf
        .streams
        .iter()
        .filter(|s| s.contains("page étalon"))
        .count();
    let plan = pdf
        .streams
        .iter()
        .filter(|s| s.contains("Plan d'assemblage des pages"))
        .count();
    ck!(
        seq,
        plan == usize::from(setup.assembly.plan),
        "page de plan : {plan}"
    );
    ck!(
        seq,
        cal >= 1 && cal + plan + pdf.tiles.len() == pdf.pages,
        "étalon {cal} + plan {plan} + patron {} ≠ {}",
        pdf.tiles.len(),
        pdf.pages
    );
    if content.is_default() {
        ck!(
            seq,
            out.bytes == export_pdf_with(&scenes, &setup).unwrap().bytes,
            "défaut : PDF différent de l'export sans filtre"
        );
    }
    let strings: Vec<String> = read_pdf(&out.bytes)
        .iter()
        .flat_map(|p| p.strings.clone())
        .collect();

    // Grille attendue (hors économie de papier, qui omet des pages vides et tourne la feuille).
    let grid: Vec<Option<usize>> = kept_scenes
        .iter()
        .map(|k| expected_tiles(&k.census, &setup, ww, wh))
        .collect();
    if !setup.assembly.save_paper && grid.iter().all(Option::is_some) {
        let total: usize = grid.iter().map(|g| g.unwrap()).sum();
        ck!(
            seq,
            pdf.tiles.len() == total,
            "{} pages de patron au lieu de {total}",
            pdf.tiles.len()
        );
        GRID_CHECKS.fetch_add(1, Relaxed);
    }
    // Une page par feuille : décompte des éléments dessinés, d'accord avec le SVG.
    if pdf.tiles.len() == kept_scenes.len() {
        let mut expected = 0usize;
        for k in &kept_scenes {
            let svg = &k.census;
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
            expected += [
                C::Construction,
                C::SewingContour,
                C::CuttingContour,
                C::Grainline,
                C::SpecialLines,
                C::Axes,
                C::Notches,
                C::Buttons,
                C::PointLabels,
                C::Dimensions,
                C::FullDimensions,
                C::Darts,
                C::PieceLabels,
            ]
            .iter()
            .map(|c| svg.count(*c))
            .sum::<usize>()
                + marking_ops
                + 2 * svg.count(C::Points);
        }
        ck!(
            seq,
            pdf.ops() == expected,
            "{} opérations dessinées au lieu de {expected}",
            pdf.ops()
        );
        OPS_CHECKS.fetch_add(1, Relaxed);
    }
    // Textes : ceux des catégories écrites y sont, ceux des catégories exclues n'y sont plus.
    let mut present: Vec<String> = Vec::new();
    for k in &kept_scenes {
        for c in [
            C::Dimensions,
            C::FullDimensions,
            C::PointLabels,
            C::PieceLabels,
        ] {
            present.extend(k.census.texts(c));
        }
    }
    for t in present.iter().filter(|t| t.is_ascii()) {
        ck!(seq, strings.contains(t), "texte « {t} » absent du PDF");
    }
    for t in excluded_texts
        .iter()
        .filter(|t| t.is_ascii() && !present.contains(t))
    {
        ck!(
            seq,
            !strings.contains(t),
            "texte exclu « {t} » écrit dans le PDF"
        );
    }

    // Boîte englobante inchangée pour toutes les feuilles : le flux conservé est une sous-suite
    // exacte du flux de l'export complet (mêmes coordonnées, même ordre, même grille).
    if !content.is_default() && !setup.assembly.save_paper && kept_scenes.iter().all(|k| k.same_box)
    {
        let fulls: Vec<Scene> = kept_scenes.iter().map(|k| k.full.clone()).collect();
        let reference = read_pdf_streams(&export_pdf_with(&fulls, &setup).unwrap().bytes);
        ck!(
            seq,
            reference.tiles.len() == pdf.tiles.len(),
            "grille différente à boîte identique : {} / {}",
            reference.tiles.len(),
            pdf.tiles.len()
        );
        for (big, small) in reference.tiles.iter().zip(&pdf.tiles) {
            ck!(
                seq,
                is_subsequence(&small.clip, &big.clip),
                "flux de la page modifié (exclus : {:?})",
                content.excluded()
            );
        }
        SUBSEQUENCE_CHECKS.fetch_add(1, Relaxed);
    }
    // Les SVG complets servent aussi de garde : toute catégorie retenue y existait.
    for k in &kept_scenes {
        for c in C::ALL {
            ck!(
                seq,
                k.census.count(c) <= k.full_census.count(c),
                "{c:?} en trop"
            );
        }
    }
}

#[test]
fn campagne_adverse_du_contenu_de_la_sortie() {
    for seq in 0..SEQUENCES {
        run(seq);
    }
    let (grid, ops, sub, refused) = (
        GRID_CHECKS.load(Relaxed),
        OPS_CHECKS.load(Relaxed),
        SUBSEQUENCE_CHECKS.load(Relaxed),
        REFUSALS.load(Relaxed),
    );
    eprintln!(
        "contrôles forts : grille {grid}, opérations {ops}, sous-suite {sub}, refus {refused}"
    );
    assert!(grid >= 100, "grille de pages vérifiée {grid} fois");
    assert!(ops >= 80, "décompte des opérations vérifié {ops} fois");
    assert!(sub >= 50, "sous-suite du flux vérifiée {sub} fois");
    assert!(refused >= 10, "refus vérifiés {refused} fois");
}

#[test]
fn la_campagne_couvre_les_cas_attendus() {
    // Garde contre une campagne vide : sur les mêmes graines, il y a des sélections vides, par
    // défaut, des exports à plusieurs pages et des exports à boîte inchangée.
    let (mut empty, mut default, mut multi, mut same_box, mut two) = (0, 0, 0, 0, 0);
    for seq in 0..SEQUENCES {
        let mut rng = Rng(0xA13C_4E77_90B1_5D29 ^ seq.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0x1304);
        for _ in 0..4 {
            rng.next();
        }
        let content = random_content(&mut rng);
        let (setup, ww, wh) = random_setup(&mut rng);
        let n_scenes = 1 + usize::from(rng.below(5) == 0);
        two += usize::from(n_scenes == 2);
        let nothing = C::ALL.iter().all(|c| !kept(&content, *c));
        empty += usize::from(nothing);
        default += usize::from(content.is_default());
        if nothing || seq % 10 == 7 {
            continue;
        }
        let (p, s, _) = random_project(&mut rng);
        let full = Scene::from_sheet(&p, s).unwrap();
        let svg = read_svg(&export_svg(&full).unwrap());
        multi += usize::from(expected_tiles(&svg, &setup, ww, wh).is_some_and(|t| t > 1));
        let inner = [
            C::Markings,
            C::Buttons,
            C::Grainline,
            C::Axes,
            C::SewingContour,
            C::Notches,
        ];
        same_box += usize::from(inner.iter().any(|c| !content.includes(*c)));
    }
    assert!(empty >= 5, "sélections vides : {empty}");
    assert!(default >= 5, "sélections par défaut : {default}");
    assert!(multi >= 5, "exports à plusieurs pages : {multi}");
    assert!(same_box >= 20, "exclusions intérieures : {same_box}");
    assert!(two >= 10, "exports de deux feuilles : {two}");
}
