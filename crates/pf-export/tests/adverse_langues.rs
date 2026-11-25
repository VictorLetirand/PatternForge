//! US-19.2 - campagne adverse des textes d'export localisés : 160 séquences à graine fixe. Chaque
//! séquence tire un réglage de page (format, orientation, marges, recouvrement, options
//! d'assemblage, correction d'échelle, règles de contrôle) et 1 à 3 feuilles (rectangle de taille
//! variable, rôles de trait, pinces, marquages, trous de piqûre, boutons, sens du droit-fil).
//!
//! **Oracle** écrit à la main, sans lire le catalogue : lignes de légende attendues selon ce qui est
//! dessiné (table FR/EN ci-dessous), invariants entre langues (pages, tailles de page, nombre de
//! lignes de texte, traits dessinés identiques), absence de français et de « ? » dans le PDF
//! anglais, absence d'anglais dans le PDF français, octets français identiques à l'export sans
//! langue.

mod common;

use common::{read_pdf, Rng};
use pf_core::{ButtonShape, GrainSense, MarkingShape, PathRole, StitchHole};
use pf_export::calibration::PrintScale;
use pf_export::page::{AssemblyOptions, Margins, Orientation, PageFormat, PageSetup};
use pf_export::pdf::{export_pdf_content, export_pdf_lang};
use pf_export::{
    OutputContent, PathGeom, Scene, SceneButton, SceneDart, SceneMarking, ScenePath,
    SceneStitchHole,
};
use pf_geometry::PointMm;
use pf_i18n::Lang;

const SEQUENCES: u64 = 160;

/// Légende de chaque rôle : (français, anglais), écrits à la main.
fn role_lines(role: PathRole) -> (&'static str, &'static str) {
    match role {
        PathRole::Construction => (
            "Construction — trait fin continu",
            "Construction - thin continuous line",
        ),
        PathRole::SewingContour => ("Ligne de couture — tirets", "Sewing line - dashes"),
        PathRole::CuttingContour => (
            "Ligne de coupe — trait épais continu",
            "Cutting line - thick continuous line",
        ),
        PathRole::Grainline => (
            "Droit-fil — flèche à deux pointes, tiret-point",
            "Grainline - double-headed arrow, dash-dot",
        ),
        PathRole::Fold => ("Pliure — tiret, point", "Fold - dash, dot"),
        PathRole::AnnotationAxis => ("Axe de repère — pointillé", "Reference axis - dotted"),
        PathRole::Topstitch => (
            "Surpiqûre — petits tirets serrés",
            "Topstitching - small close dashes",
        ),
        PathRole::CenterFront => (
            "Milieu devant — trait long, un tiret court",
            "Center front - long line, one short dash",
        ),
        PathRole::CenterBack => (
            "Milieu dos — trait long, deux tirets courts",
            "Center back - long line, two short dashes",
        ),
        PathRole::WaistLine => (
            "Ligne de taille — tirets longs fins",
            "Waistline - long thin dashes",
        ),
        PathRole::HipLine => (
            "Ligne de hanche — tirets courts fins",
            "Hipline - short thin dashes",
        ),
        PathRole::BustLine => (
            "Ligne de poitrine — tiret long, tiret court",
            "Bust line - long dash, short dash",
        ),
        PathRole::SkivingLine => (
            "Ligne de parage — tiret, deux points",
            "Skiving line - dash, two dots",
        ),
        PathRole::GlueLine => ("Ligne de collage — gros points", "Glue line - large dots"),
    }
}

const BASE: [PathRole; 6] = [
    PathRole::Construction,
    PathRole::SewingContour,
    PathRole::CuttingContour,
    PathRole::Grainline,
    PathRole::Fold,
    PathRole::AnnotationAxis,
];
const SPECIAL: [PathRole; 8] = [
    PathRole::Topstitch,
    PathRole::CenterFront,
    PathRole::CenterBack,
    PathRole::WaistLine,
    PathRole::HipLine,
    PathRole::BustLine,
    PathRole::SkivingLine,
    PathRole::GlueLine,
];

const FRENCH_WORDS: [&str; 12] = [
    "étalon",
    "Légende",
    "Papier",
    "feuille",
    "Assemblage",
    "Imprimer",
    "patron",
    "Cran",
    "Pince",
    "Bouton",
    "Rouleau",
    "Contenu",
];
const ENGLISH_WORDS: [&str; 6] = [
    "Print at",
    "Notch",
    "Dart",
    "Paper:",
    "Line legend",
    "Roll:",
];

fn pick(rng: &mut Rng, n: u64) -> u64 {
    rng.next() % n
}

fn line(a: (f64, f64), b: (f64, f64), role: PathRole, sense: GrainSense) -> ScenePath {
    ScenePath {
        geom: PathGeom::Line(PointMm::new(a.0, a.1), PointMm::new(b.0, b.1)),
        role,
        sense,
    }
}

struct Flags {
    dart: bool,
    marking: bool,
    slot: bool,
    stitch: bool,
    button: bool,
    nap: bool,
    special: Vec<PathRole>,
}

fn random_scene(rng: &mut Rng, k: usize, flags: &mut Flags) -> Scene {
    let mut s = Scene::calibration_square();
    s.project_name = "Projet".into();
    s.sheet_name = format!("Feuille {}", k + 1);
    let (w, h) = (60.0 + pick(rng, 900) as f64, 60.0 + pick(rng, 700) as f64);
    let role = if pick(rng, 4) == 0 {
        PathRole::Construction
    } else {
        PathRole::CuttingContour
    };
    let c = [(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)];
    s.paths = (0..4)
        .map(|i| line(c[i], c[(i + 1) % 4], role, GrainSense::Unmarked))
        .collect();
    for r in SPECIAL {
        if pick(rng, 5) == 0 {
            s.paths
                .push(line((5.0, 5.0), (25.0, 5.0), r, GrainSense::Unmarked));
            if !flags.special.contains(&r) {
                flags.special.push(r);
            }
        }
    }
    if pick(rng, 3) == 0 {
        s.paths.push(line(
            (5.0, 9.0),
            (30.0, 9.0),
            PathRole::Grainline,
            GrainSense::Nap,
        ));
        flags.nap = true;
    }
    if pick(rng, 3) == 0 {
        s.darts.push(SceneDart {
            piece: "P".into(),
            apex: PointMm::new(20.0, 30.0),
            left_foot: PointMm::new(15.0, 10.0),
            right_foot: PointMm::new(25.0, 10.0),
        });
        flags.dart = true;
    }
    if pick(rng, 3) == 0 {
        let slot = pick(rng, 2) == 0;
        s.markings.push(SceneMarking {
            pos: PointMm::new(30.0, 30.0),
            shape: if slot {
                MarkingShape::Slot {
                    length_mm: 8.0,
                    width_mm: 2.0,
                    angle_deg: 0.0,
                }
            } else {
                MarkingShape::Punch
            },
        });
        flags.marking = true;
        flags.slot |= slot;
    }
    if pick(rng, 3) == 0 {
        s.stitch_holes.push(SceneStitchHole {
            pos: PointMm::new(40.0, 30.0),
            hole: StitchHole::Round { diameter_mm: 1.0 },
            tangent_deg: 0.0,
        });
        flags.stitch = true;
    }
    if pick(rng, 3) == 0 {
        s.buttons.push(SceneButton {
            pos: PointMm::new(50.0, 30.0),
            shape: ButtonShape::Button { diameter_mm: 10.0 },
        });
        flags.button = true;
    }
    s
}

fn random_setup(rng: &mut Rng) -> PageSetup {
    let mut s = PageSetup {
        format: match pick(rng, 6) {
            0 => PageFormat::A4,
            1 => PageFormat::A3,
            2 => PageFormat::Letter,
            3 => PageFormat::Custom {
                width_mm: 250.0 + pick(rng, 300) as f64,
                height_mm: 300.0 + pick(rng, 400) as f64,
            },
            4 => PageFormat::Roll {
                width_mm: 600.0 + pick(rng, 400) as f64,
            },
            _ => PageFormat::A0,
        },
        ..PageSetup::default()
    };
    if pick(rng, 2) == 0 {
        s.orientation = Orientation::Landscape;
    }
    if pick(rng, 2) == 0 {
        s.margins = Margins {
            top_mm: pick(rng, 25) as f64,
            bottom_mm: pick(rng, 25) as f64,
            left_mm: pick(rng, 25) as f64,
            right_mm: pick(rng, 25) as f64,
        };
    }
    if pick(rng, 2) == 0 {
        s.overlap_mm = 5.0 + pick(rng, 30) as f64;
    }
    s.assembly = AssemblyOptions {
        marks: pick(rng, 2) == 0,
        plan: pick(rng, 2) == 0,
        save_paper: pick(rng, 3) == 0,
    };
    if pick(rng, 3) == 0 {
        s.scale = PrintScale {
            x: 1.0 + pick(rng, 40) as f64 / 1000.0,
            y: 1.0 - pick(rng, 40) as f64 / 1000.0,
        };
    }
    s.control_marks = pick(rng, 3) == 0;
    s
}

fn strokes(pages: &[common::Page]) -> Vec<(usize, usize)> {
    pages
        .iter()
        .map(|p| (p.lines.len(), p.rects.len()))
        .collect()
}

#[test]
fn campagne_adverse_textes_d_export_localises() {
    let mut rng = Rng(0x1902_5EED);
    let content = OutputContent::default();
    let (mut with_plan, mut with_roll, mut with_scale, mut with_dart, mut with_slot) =
        (0, 0, 0, 0, 0);
    let (mut with_stitch, mut with_button, mut with_nap, mut with_special, mut with_paper) =
        (0, 0, 0, 0, 0);
    let mut checked = 0;
    for seq in 0..SEQUENCES {
        let setup = random_setup(&mut rng);
        let mut flags = Flags {
            dart: false,
            marking: false,
            slot: false,
            stitch: false,
            button: false,
            nap: false,
            special: Vec::new(),
        };
        let n = 1 + pick(&mut rng, 3) as usize;
        let scenes: Vec<Scene> = (0..n)
            .map(|k| random_scene(&mut rng, k, &mut flags))
            .collect();
        let (fr, en) = match (
            export_pdf_lang(&scenes, &setup, &content, Lang::Fr),
            export_pdf_lang(&scenes, &setup, &content, Lang::En),
        ) {
            (Ok(f), Ok(e)) => (f, e),
            (Err(a), Err(b)) => {
                // Un réglage ou une feuille refusés le sont dans les deux langues, pour la même raison.
                assert_eq!(a.to_string(), b.to_string(), "séquence {seq}");
                continue;
            }
            (a, b) => panic!("séquence {seq} : {a:?} / {b:?}"),
        };
        checked += 1;
        let legacy = export_pdf_content(&scenes, &setup, &content).unwrap();
        assert_eq!(legacy.bytes, fr.bytes, "séquence {seq} : français modifié");
        assert_eq!(fr.page_count, en.page_count, "séquence {seq}");
        assert_eq!(fr.page_sizes_mm, en.page_sizes_mm, "séquence {seq}");

        let (pf, pe) = (read_pdf(&fr.bytes), read_pdf(&en.bytes));
        assert_eq!(strokes(&pf), strokes(&pe), "séquence {seq} : traits");
        for (a, b) in pf.iter().zip(&pe) {
            assert_eq!(a.strings.len(), b.strings.len(), "séquence {seq} : lignes");
        }
        let all_fr: Vec<String> = pf.iter().flat_map(|p| p.strings.clone()).collect();
        let all_en: Vec<String> = pe.iter().flat_map(|p| p.strings.clone()).collect();
        let joined_en = all_en.join("\n");
        let joined_fr = all_fr.join("\n");
        assert!(
            !joined_en.contains('?') && !joined_en.contains('\u{fffd}'),
            "séquence {seq}"
        );
        for w in FRENCH_WORDS {
            assert!(
                !joined_en.contains(w),
                "séquence {seq} : « {w} » en anglais"
            );
        }
        for w in ENGLISH_WORDS {
            assert!(
                !joined_fr.contains(w),
                "séquence {seq} : « {w} » en français"
            );
        }

        let want = |fr_line: &str, en_line: &str| {
            assert!(
                all_fr.iter().any(|l| l == fr_line),
                "séquence {seq} FR : {fr_line}"
            );
            assert!(
                all_en.iter().any(|l| l == en_line),
                "séquence {seq} EN : {en_line}"
            );
        };
        let absent = |fr_line: &str, en_line: &str| {
            assert!(
                !all_fr.iter().any(|l| l == fr_line),
                "séquence {seq} FR : {fr_line}"
            );
            assert!(
                !all_en.iter().any(|l| l == en_line),
                "séquence {seq} EN : {en_line}"
            );
        };
        want(
            "PatternForge — page étalon",
            "PatternForge - calibration page",
        );
        want("Légende des traits", "Line legend");
        want(
            "Cran — petit trait perpendiculaire au bord",
            "Notch - short line perpendicular to the edge",
        );
        for r in BASE {
            let (f, e) = role_lines(r);
            want(f, e);
        }
        for r in SPECIAL {
            let (f, e) = role_lines(r);
            if flags.special.contains(&r) {
                want(f, e);
            } else {
                absent(f, e);
            }
        }
        let cond = |on: bool, f: &str, e: &str| {
            if on {
                want(f, e)
            } else {
                absent(f, e)
            }
        };
        cond(
            flags.nap,
            "Sens du droit-fil — poil : une pointe fermée ; motif : pointes doublées",
            "Grainline direction - nap: one closed head; print: doubled heads",
        );
        cond(
            flags.dart,
            "Pince — jambes (sommet vers les deux pieds), trait long et deux points",
            "Dart - legs (apex towards the two ends), long line and two dots",
        );
        cond(
            flags.marking && flags.slot,
            "Marquage — pointage (disque plein), perçage (cercle), croix, fente",
            "Marking - punch point (filled disc), drill hole (circle), cross, slot",
        );
        cond(
            flags.marking && !flags.slot,
            "Marquage — pointage (disque plein), perçage (cercle), croix",
            "Marking - punch point (filled disc), drill hole (circle), cross",
        );
        cond(
            flags.stitch,
            "Trou de piqure - rond (cercle), plat, francais (losange), oblique",
            "Stitching hole - round (circle), flat, French (diamond), slanted",
        );
        cond(
            flags.button,
            "Bouton (cercle), boutonnière (fente à bouts arrondis)",
            "Button (circle), buttonhole (slot with rounded ends)",
        );
        let roll = matches!(setup.format, PageFormat::Roll { .. });
        cond(
            roll,
            "Rouleau : une page continue par feuille, sans assemblage.",
            "Roll: one continuous page per sheet, no assembly.",
        );
        cond(
            !roll,
            &format!(
                "Assemblage : superposer les bandes de recouvrement de {} mm, traits gris de raccord alignés.",
                setup.overlap_mm
            ),
            &format!(
                "Assembly: overlap the {} mm overlap strips, grey joining lines aligned.",
                setup.overlap_mm
            ),
        );
        let paper_fr = all_fr.iter().any(|l| l.starts_with("Papier : "));
        let paper_en = all_en.iter().any(|l| l.starts_with("Paper: "));
        assert_eq!(paper_fr, paper_en, "séquence {seq}");
        assert_eq!(paper_fr, setup != PageSetup::default(), "séquence {seq}");
        let scale_fr = all_fr
            .iter()
            .any(|l| l.starts_with("Correction d'échelle active"));
        let scale_en = all_en
            .iter()
            .any(|l| l.starts_with("Scale correction active"));
        assert_eq!(scale_fr, scale_en, "séquence {seq}");
        assert_eq!(
            scale_fr,
            setup.scale.x != 1.0 || setup.scale.y != 1.0,
            "séquence {seq}"
        );
        let plan_fr = all_fr.iter().any(|l| l == "Plan d'assemblage des pages");
        let plan_en = all_en.iter().any(|l| l == "Page assembly plan");
        assert_eq!(plan_fr, plan_en, "séquence {seq}");

        with_plan += usize::from(plan_fr);
        with_roll += usize::from(roll);
        with_scale += usize::from(scale_fr);
        with_dart += usize::from(flags.dart);
        with_slot += usize::from(flags.slot);
        with_stitch += usize::from(flags.stitch);
        with_button += usize::from(flags.button);
        with_nap += usize::from(flags.nap);
        with_special += usize::from(!flags.special.is_empty());
        with_paper += usize::from(paper_fr);
    }
    assert!(checked >= 150, "séquences valides : {checked}");
    for (name, n) in [
        ("plan", with_plan),
        ("rouleau", with_roll),
        ("échelle", with_scale),
        ("pince", with_dart),
        ("fente", with_slot),
        ("piqûre", with_stitch),
        ("bouton", with_button),
        ("poil", with_nap),
        ("lignes spéciales", with_special),
        ("papier", with_paper),
    ] {
        assert!(n >= 10, "couverture « {name} » : {n}");
    }
}
