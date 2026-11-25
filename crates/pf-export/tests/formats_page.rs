//! US-13.1 — formats de page, orientation, marges imprimante, rouleau.
//!
//! Les fichiers produits sont relus par un lecteur écrit ici, indépendant du code d'export
//! (il ne réutilise ni `MM_TO_PT`, ni les constantes de pages, ni les fonctions de dessin) :
//! les tailles attendues sont des valeurs en points écrites en dur. Aucun de ces tests ne
//! remplace la mesure d'une feuille imprimée (AT-09, humaine).

#[path = "common/pdf_text.rs"]
mod pdf_text;

use pf_core::PathRole;
use pf_export::pdf::{
    export_calibration_pdf_with, export_pdf, export_pdf_with, page_count_with, plan_tiles_with,
};
use pf_export::{
    ExportError, Margins, Orientation, PageFormat, PageSetup, PageSetupError, Scene, ScenePath,
    ScenePoint,
};
use pf_geometry::PointMm;

// ---------------------------------------------------------------------------
// Lecteur indépendant
// ---------------------------------------------------------------------------

const PT_TO_MM: f64 = 25.4 / 72.0;

#[derive(Debug, Default)]
struct Page {
    /// MediaBox lue, en points : largeur, hauteur.
    media_pt: (f64, f64),
    /// Segments `m l`, en mm.
    lines: Vec<((f64, f64), (f64, f64))>,
    /// Rectangles `re`, en mm : x, y, largeur, hauteur, et vrai si c'est un rectangle de
    /// découpe (`re W n`).
    rects: Vec<(f64, f64, f64, f64, bool)>,
    /// Origines des textes (`Td`), en mm.
    texts: Vec<(f64, f64)>,
    text: String,
}

impl Page {
    fn media_mm(&self) -> (f64, f64) {
        (self.media_pt.0 * PT_TO_MM, self.media_pt.1 * PT_TO_MM)
    }
}

fn read_pdf(bytes: &[u8]) -> Vec<Page> {
    let readable = pdf_text::readable(bytes);
    let text = readable.as_str();
    let boxes: Vec<(f64, f64)> = text
        .match_indices("/MediaBox [")
        .map(|(i, _)| {
            let rest = &text[i + 11..];
            let end = rest.find(']').unwrap();
            let v: Vec<f64> = rest[..end]
                .split_whitespace()
                .map(|n| n.parse().unwrap())
                .collect();
            assert_eq!((v[0], v[1]), (0.0, 0.0));
            (v[2], v[3])
        })
        .collect();
    let mut pages = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("stream\n") {
        let body = &rest[start + 7..];
        let end = body.find("endstream").unwrap();
        pages.push(read_stream(&body[..end]));
        rest = &body[end + 9..];
    }
    assert_eq!(boxes.len(), pages.len(), "une MediaBox par page");
    let count_decl = text.find("/Count ").map(|i| {
        text[i + 7..]
            .split_whitespace()
            .next()
            .unwrap()
            .parse::<usize>()
            .unwrap()
    });
    assert_eq!(count_decl, Some(pages.len()), "/Count du catalogue");
    for (p, b) in pages.iter_mut().zip(boxes) {
        p.media_pt = b;
    }
    pages
}

fn read_stream(stream: &str) -> Page {
    let mut page = Page::default();
    let mut tokens: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut chars = stream.chars();
    while let Some(ch) = chars.next() {
        match ch {
            '(' => {
                let mut depth = 1;
                while let Some(c) = chars.next() {
                    match c {
                        '\\' => {
                            // Échappement : `\(`, `\)`, `\\` ou code octal `\ooo`.
                            let mut it = chars.clone();
                            let first = it.next().unwrap_or(' ');
                            if first.is_digit(8) {
                                let mut code = 0u32;
                                for _ in 0..3 {
                                    let d = chars.clone().next().and_then(|d| d.to_digit(8));
                                    match d {
                                        Some(d) => {
                                            code = code * 8 + d;
                                            chars.next();
                                        }
                                        None => break,
                                    }
                                }
                                page.text.push(match code {
                                    0x85 => '…',
                                    0x92 => '’',
                                    0x96 => '–',
                                    0x97 => '—',
                                    c => char::from_u32(c).unwrap_or('?'),
                                });
                            } else {
                                page.text.push(first);
                                chars.next();
                            }
                        }
                        '(' => depth += 1,
                        ')' => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        _ => page.text.push(c),
                    }
                }
                page.text.push('\n');
                tokens.push("()".into());
            }
            '[' => {
                for c in chars.by_ref() {
                    if c == ']' {
                        break;
                    }
                }
                tokens.push("[]".into());
            }
            c if c.is_whitespace() => {
                if !cur.is_empty() {
                    tokens.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    let mut nums: Vec<f64> = Vec::new();
    let mut here = (0.0, 0.0);
    for (i, t) in tokens.iter().enumerate() {
        if let Ok(v) = t.parse::<f64>() {
            nums.push(v);
            continue;
        }
        let n = nums.len();
        let mm = |v: f64| v * PT_TO_MM;
        match t.as_str() {
            "m" => here = (mm(nums[n - 2]), mm(nums[n - 1])),
            "l" => {
                let b = (mm(nums[n - 2]), mm(nums[n - 1]));
                page.lines.push((here, b));
                here = b;
            }
            "re" => {
                let clip = tokens.get(i + 1).map(String::as_str) == Some("W");
                page.rects.push((
                    mm(nums[n - 4]),
                    mm(nums[n - 3]),
                    mm(nums[n - 2]),
                    mm(nums[n - 1]),
                    clip,
                ));
            }
            "Td" => page.texts.push((mm(nums[n - 2]), mm(nums[n - 1]))),
            _ => {}
        }
        nums.clear();
    }
    page
}

fn len(l: &((f64, f64), (f64, f64))) -> f64 {
    (l.1 .0 - l.0 .0).hypot(l.1 .1 - l.0 .1)
}

// ---------------------------------------------------------------------------
// Scènes et réglages
// ---------------------------------------------------------------------------

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

/// Rectangle `w × h` mm dont le coin bas-gauche est en (0 ; 0), en ligne de coupe.
fn rect_scene(w: f64, h: f64) -> Scene {
    let c = [pt(0., 0.), pt(w, 0.), pt(w, h), pt(0., h)];
    Scene {
        project_name: "Test".into(),
        sheet_name: "Pièce".into(),
        paths: (0..4)
            .map(|i| ScenePath {
                geom: pf_export::PathGeom::Line(c[i], c[(i + 1) % 4]),
                role: PathRole::CuttingContour,
                sense: pf_core::GrainSense::Unmarked,
            })
            .collect(),
        points: vec![ScenePoint {
            pos: c[0],
            label: "A".into(),
            show_label: true,
        }],
        notch_marks: Vec::new(),
        dimensions: Vec::new(),
        full_dimensions: Vec::new(),
        markings: Vec::new(),
        stitch_holes: Vec::new(),
        zones: Vec::new(),
        buttons: Vec::new(),
        grain_limits: Vec::new(),
        cut_contours: Vec::new(),
        cut_warnings: Vec::new(),
        darts: Vec::new(),
        dart_warnings: Vec::new(),
        piece_labels: Vec::new(),
        folds: Vec::new(),
        fold_overs: Vec::new(),
    }
}

fn setup(format: PageFormat, orientation: Orientation) -> PageSetup {
    PageSetup {
        format,
        orientation,
        margins: Margins::default(),
        ..PageSetup::default()
    }
}

/// Tailles attendues en points (valeurs écrites en dur, arrondies à 1e-4).
const A4_PT: (f64, f64) = (595.2756, 841.8898);
const A3_PT: (f64, f64) = (841.8898, 1190.5512);
const A0_PT: (f64, f64) = (2383.937, 3370.3937);
const LETTER_PT: (f64, f64) = (612.0, 792.0);

fn near(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

fn all_media(pdf: &[u8]) -> Vec<(f64, f64)> {
    read_pdf(pdf).iter().map(|p| p.media_pt).collect()
}

// ---------------------------------------------------------------------------
// CA : formats et orientation
// ---------------------------------------------------------------------------

#[test]
fn mediabox_exactes_de_chaque_format_en_portrait_et_en_paysage() {
    let cases = [
        (PageFormat::A4, A4_PT),
        (PageFormat::A3, A3_PT),
        (PageFormat::A0, A0_PT),
        (PageFormat::Letter, LETTER_PT),
    ];
    // Une pièce de 400 × 300 mm : plusieurs pages en A4, une seule en A0.
    let scene = rect_scene(400.0, 300.0);
    for (format, (w, h)) in cases {
        for (orientation, expect) in [
            (Orientation::Portrait, (w, h)),
            (Orientation::Landscape, (h, w)),
        ] {
            let s = setup(format, orientation);
            let out = export_pdf_with(std::slice::from_ref(&scene), &s).unwrap();
            let boxes = all_media(&out.bytes);
            assert_eq!(boxes.len(), out.page_count);
            for b in boxes {
                assert!(
                    near(b.0, expect.0, 0.005) && near(b.1, expect.1, 0.005),
                    "{format:?} {orientation:?} : {b:?} au lieu de {expect:?}"
                );
            }
            // Taille déclarée par l'export, en mm : celle du format (A4 210 × 297, etc.).
            let (mw, mh) = out.page_sizes_mm[0];
            assert!(near(mw * 72.0 / 25.4, expect.0, 0.005));
            assert!(near(mh * 72.0 / 25.4, expect.1, 0.005));
        }
    }
}

#[test]
fn dimensions_en_mm_des_formats_normalises() {
    let size = |f| {
        let s = setup(f, Orientation::Portrait);
        (s.page_width_mm(), s.page_height_mm().unwrap())
    };
    assert_eq!(size(PageFormat::A4), (210.0, 297.0));
    assert_eq!(size(PageFormat::A3), (297.0, 420.0));
    assert_eq!(size(PageFormat::A0), (841.0, 1189.0));
    assert_eq!(size(PageFormat::Letter), (215.9, 279.4));
}

#[test]
fn format_libre_aux_bornes_et_hors_bornes() {
    let free = |w, h| {
        setup(
            PageFormat::Custom {
                width_mm: w,
                height_mm: h,
            },
            Orientation::Portrait,
        )
    };
    // Bornes incluses : 190 et 5000 mm.
    for (w, h) in [
        (190.0, 190.0),
        (5000.0, 5000.0),
        (190.0, 5000.0),
        (500.0, 300.0),
    ] {
        let out = export_calibration_pdf_with(&free(w, h)).unwrap();
        let b = all_media(&out.bytes);
        assert_eq!(b.len(), out.page_count);
        assert!(
            near(b[0].0 * PT_TO_MM, w, 0.002) && near(b[0].1 * PT_TO_MM, h, 0.002),
            "{w} × {h} : {b:?}"
        );
    }
    // Paysage : largeur et hauteur échangées.
    let land = PageSetup {
        orientation: Orientation::Landscape,
        ..free(500.0, 300.0)
    };
    let b = all_media(&export_calibration_pdf_with(&land).unwrap().bytes);
    assert!(near(b[0].0 * PT_TO_MM, 300.0, 0.002) && near(b[0].1 * PT_TO_MM, 500.0, 0.002));
    // Hors bornes ou non finis : refusés.
    for (w, h) in [
        (189.99, 300.0),
        (300.0, 189.99),
        (5000.01, 300.0),
        (300.0, 5000.01),
        (0.0, 300.0),
        (-300.0, 300.0),
        (f64::NAN, 300.0),
        (300.0, f64::INFINITY),
        (f64::NEG_INFINITY, 300.0),
        (f64::MAX, f64::MAX),
        (f64::MIN_POSITIVE, 300.0),
    ] {
        let err = export_calibration_pdf_with(&free(w, h)).unwrap_err();
        assert!(matches!(err, ExportError::Page(_)), "{w} × {h} : {err}");
        assert!(!err.to_string().is_empty());
    }
}

// ---------------------------------------------------------------------------
// CA : rouleau
// ---------------------------------------------------------------------------

#[test]
fn rouleau_largeur_exacte_et_longueur_au_moins_egale_a_l_emprise() {
    let roll = setup(PageFormat::Roll { width_mm: 914.0 }, Orientation::Portrait);
    // Pièce de 600 × 1234 mm : emprise avec blanc = 610 × 1244 mm.
    let scene = rect_scene(600.0, 1234.0);
    let out = export_pdf_with(std::slice::from_ref(&scene), &roll).unwrap();
    assert_eq!(out.page_count, 2, "étalon + une seule page continue");
    let pages = read_pdf(&out.bytes);
    let content = &pages[1];
    let (w, h) = content.media_mm();
    assert!(near(w, 914.0, 0.002), "largeur {w}");
    // Longueur = zone du patron (emprise 1244 arrondie à 1250) + bandeau 8 + marges 10 + 10.
    assert!(h >= 1244.0 + 8.0 + 20.0 - 1e-6, "longueur {h}");
    assert!(near(h, 1250.0 + 8.0 + 20.0, 0.002), "longueur {h}");
    // Rien n'est tronqué : le patron entier est dans la fenêtre, en une seule page.
    let tiles = plan_tiles_with(&scene, &roll).unwrap();
    assert_eq!(tiles.len(), 1);
    let t = tiles[0];
    assert!(t.world_min.x <= -5.0 && t.world_min.y <= -5.0);
    assert!(t.world_min.y + t.layout.window_h_mm >= 1234.0 + 5.0);
    assert!(t.layout.window_w_mm >= 610.0);
    // Étalon : même largeur de rouleau.
    assert!(near(pages[0].media_mm().0, 914.0, 0.002));
}

#[test]
fn rouleau_orientation_ignoree_et_longueur_arrondie_au_dessus() {
    let scene = rect_scene(300.0, 801.0); // emprise 811 → 820
    let a = export_pdf_with(
        std::slice::from_ref(&scene),
        &setup(PageFormat::Roll { width_mm: 600.0 }, Orientation::Portrait),
    )
    .unwrap();
    let b = export_pdf_with(
        std::slice::from_ref(&scene),
        &setup(PageFormat::Roll { width_mm: 600.0 }, Orientation::Landscape),
    )
    .unwrap();
    assert_eq!(
        a.bytes, b.bytes,
        "l'orientation n'a aucun effet sur un rouleau"
    );
    let pages = read_pdf(&a.bytes);
    assert!(near(pages[1].media_mm().1, 820.0 + 28.0, 0.002));
}

#[test]
fn rouleau_refus_clairs_contenu_trop_large_ou_trop_long_ou_largeur_invalide() {
    let roll = |w| setup(PageFormat::Roll { width_mm: w }, Orientation::Portrait);
    // Plus large que la zone imprimable (580 mm) : refusé, jamais réduit.
    let err = export_pdf_with(&[rect_scene(580.0, 100.0)], &roll(600.0)).unwrap_err();
    assert!(matches!(
        err,
        ExportError::Page(PageSetupError::ContentTooWide { .. })
    ));
    assert!(err.to_string().contains("refusé"), "{err}");
    // Juste assez large : accepté (570 + 10 de blanc = 580).
    assert!(export_pdf_with(&[rect_scene(570.0, 100.0)], &roll(600.0)).is_ok());
    // Trop long : plus de 20 000 mm.
    let err = export_pdf_with(&[rect_scene(100.0, 20_000.0)], &roll(600.0)).unwrap_err();
    assert!(
        matches!(err, ExportError::Page(PageSetupError::RollTooLong { .. })),
        "{err}"
    );
    assert!(export_pdf_with(&[rect_scene(100.0, 19_900.0)], &roll(600.0)).is_ok());
    // Largeur de rouleau invalide.
    for w in [0.0, -1.0, 189.9, 5000.1, f64::NAN, f64::INFINITY] {
        assert!(
            matches!(
                export_pdf_with(&[rect_scene(100.0, 100.0)], &roll(w)),
                Err(ExportError::Page(_))
            ),
            "{w}"
        );
    }
}

// ---------------------------------------------------------------------------
// CA : marges
// ---------------------------------------------------------------------------

fn margins(top: f64, bottom: f64, left: f64, right: f64) -> Margins {
    Margins {
        top_mm: top,
        bottom_mm: bottom,
        left_mm: left,
        right_mm: right,
    }
}

#[test]
fn marges_le_contenu_reste_dans_la_zone_imprimable_positions_mesurees() {
    let scene = rect_scene(400.0, 300.0);
    let m = margins(7.0, 12.0, 20.0, 5.0);
    for (format, orientation) in [
        (PageFormat::A4, Orientation::Portrait),
        (PageFormat::A4, Orientation::Landscape),
        (PageFormat::A3, Orientation::Landscape),
        (PageFormat::Letter, Orientation::Portrait),
        (
            PageFormat::Custom {
                width_mm: 450.0,
                height_mm: 380.0,
            },
            Orientation::Portrait,
        ),
    ] {
        let s = PageSetup {
            format,
            orientation,
            margins: m,
            ..PageSetup::default()
        };
        let out = export_pdf_with(std::slice::from_ref(&scene), &s).unwrap();
        let pages = read_pdf(&out.bytes);
        let first_tile = pages.len() - plan_tiles_with(&scene, &s).unwrap().len();
        for page in &pages[first_tile..] {
            let (pw, ph) = page.media_mm();
            // La découpe (et le cadre) est la zone imprimable moins le bandeau de titre.
            let clip = page.rects.iter().find(|r| r.4).unwrap();
            assert!(near(clip.0, m.left_mm, 1e-3), "{format:?} x {}", clip.0);
            assert!(near(clip.1, m.bottom_mm, 1e-3), "{format:?} y {}", clip.1);
            assert!(
                near(clip.0 + clip.2, pw - m.right_mm, 1e-3),
                "{format:?} droite"
            );
            assert!(
                near(clip.1 + clip.3 + 8.0, ph - m.top_mm, 1e-3),
                "{format:?} haut"
            );
            // Aucun texte (titre compris) ne sort de la zone imprimable.
            for &(x, y) in &page.texts {
                assert!(x >= m.left_mm - 1e-3 && x <= pw - m.right_mm);
                assert!(y >= m.bottom_mm - 1e-3 && y <= ph - m.top_mm);
            }
        }
    }
    // Position mesurée du coin (0 ; 0) du patron sur une page unique de A3 paysage : le
    // contenu est calé en haut à gauche de la fenêtre, avec 5 mm de blanc.
    let big = PageSetup {
        format: PageFormat::A3,
        orientation: Orientation::Landscape,
        margins: m,
        ..PageSetup::default()
    };
    let scene = rect_scene(100.0, 100.0);
    let pages = read_pdf(
        &export_pdf_with(std::slice::from_ref(&scene), &big)
            .unwrap()
            .bytes,
    );
    let page = &pages[1];
    let (_, ph) = page.media_mm();
    let wh = ph - m.top_mm - m.bottom_mm - 8.0;
    let bottom_edge = page
        .lines
        .iter()
        .find(|l| near(len(l), 100.0, 1e-3) && l.0 .1 == l.1 .1)
        .unwrap();
    assert!(near(
        bottom_edge.0 .0.min(bottom_edge.1 .0),
        m.left_mm + 5.0,
        1e-3
    ));
    assert!(
        near(bottom_edge.0 .1, m.bottom_mm + wh - 105.0, 1e-3),
        "y {}",
        bottom_edge.0 .1
    );
}

#[test]
fn marges_nulles_et_maximales_acceptees() {
    for mm in [0.0, 100.0] {
        let s = PageSetup {
            format: PageFormat::A0,
            orientation: Orientation::Portrait,
            margins: Margins::uniform(mm),
            ..PageSetup::default()
        };
        let out = export_pdf_with(&[rect_scene(300.0, 200.0)], &s).unwrap();
        let pages = read_pdf(&out.bytes);
        let clip = pages[1].rects.iter().find(|r| r.4).unwrap();
        assert!(near(clip.0, mm, 1e-3) && near(clip.1, mm, 1e-3));
    }
}

#[test]
fn marges_invalides_refusees_sans_trace() {
    let with = |m: Margins| PageSetup {
        margins: m,
        ..PageSetup::default()
    };
    let scene = rect_scene(100.0, 100.0);
    let bad = [
        margins(-0.001, 10.0, 10.0, 10.0),
        margins(10.0, -5.0, 10.0, 10.0),
        margins(10.0, 10.0, f64::NAN, 10.0),
        margins(10.0, 10.0, 10.0, f64::INFINITY),
        margins(100.01, 10.0, 10.0, 10.0),
        margins(10.0, 10.0, 10.0, 1e300),
        // Zone imprimable trop petite : la somme des marges dépasse la page ou presque.
        margins(10.0, 10.0, 60.0, 60.0),
        margins(80.0, 80.0, 10.0, 10.0),
        margins(100.0, 100.0, 100.0, 100.0),
    ];
    for m in bad {
        let s = with(m);
        assert!(s.validate().is_err(), "{m:?}");
        let before = scene.clone();
        let err = export_pdf_with(std::slice::from_ref(&scene), &s).unwrap_err();
        assert!(matches!(err, ExportError::Page(_)), "{m:?} : {err}");
        assert_eq!(scene, before);
        assert!(page_count_with(std::slice::from_ref(&scene), &s).is_err());
        assert!(plan_tiles_with(&scene, &s).is_err());
    }
    // Limite : zone imprimable minimale 160 × 170 mm acceptée en A4 portrait (50 + 0 de marge).
    assert!(with(margins(60.0, 67.0, 25.0, 25.0)).validate().is_ok());
    assert!(with(margins(60.0, 67.1, 25.0, 25.0)).validate().is_err());
}

// ---------------------------------------------------------------------------
// CA : étalon, échelle 1:1
// ---------------------------------------------------------------------------

#[test]
fn le_carre_etalon_mesure_100_mm_dans_chaque_format() {
    let m = margins(9.0, 14.0, 17.0, 11.0);
    let formats = [
        PageFormat::A4,
        PageFormat::A3,
        PageFormat::A0,
        PageFormat::Letter,
        PageFormat::Custom {
            width_mm: 200.0,
            height_mm: 200.0,
        },
        PageFormat::Custom {
            width_mm: 700.0,
            height_mm: 250.0,
        },
        PageFormat::Roll { width_mm: 600.0 },
        PageFormat::Roll { width_mm: 190.0 },
    ];
    for format in formats {
        for orientation in [Orientation::Portrait, Orientation::Landscape] {
            for margins in [Margins::default(), m, Margins::uniform(0.0)] {
                let s = PageSetup {
                    format,
                    orientation,
                    margins,
                    ..PageSetup::default()
                };
                let out = export_calibration_pdf_with(&s).unwrap();
                let pages = read_pdf(&out.bytes);
                // Une page de suite n'existe que si la légende ne tient pas sous l'étalon.
                assert_eq!(pages.len(), out.page_count);
                let page = &pages[0];
                let (pw, ph) = page.media_mm();
                let sq = page
                    .rects
                    .iter()
                    .find(|r| near(r.2, 100.0, 1e-3) && near(r.3, 100.0, 1e-3))
                    .unwrap_or_else(|| {
                        panic!("{format:?} {orientation:?} : pas de carré de 100 mm")
                    });
                // Dans la zone imprimable.
                assert!(
                    sq.0 >= margins.left_mm - 1e-6 && sq.0 + 100.0 <= pw - margins.right_mm + 1e-6
                );
                assert!(
                    sq.1 >= margins.bottom_mm - 1e-6 && sq.1 + 100.0 <= ph - margins.top_mm + 1e-6
                );
                // La règle graduée de 100 mm et ses 101 graduations.
                let ruler = page
                    .lines
                    .iter()
                    .filter(|l| near(len(l), 100.0, 1e-3))
                    .count();
                assert!(ruler >= 1);
                assert!(page.lines.iter().filter(|l| len(l) < 5.0).count() >= 101);
                // Tout le texte tient dans la zone imprimable.
                for &(x, y) in &page.texts {
                    assert!(x >= margins.left_mm - 1e-6 && x <= pw - margins.right_mm);
                    assert!(y >= margins.bottom_mm - 1e-6 && y <= ph - margins.top_mm);
                }
            }
        }
    }
}

#[test]
fn echelle_1_pour_1_une_cote_de_150_mm_mesure_150_mm_partout() {
    // Segment de 150 mm de long dans une pièce de 150 × 80 mm.
    let scene = rect_scene(150.0, 80.0);
    for s in [
        PageSetup::default(),
        setup(PageFormat::A3, Orientation::Landscape),
        setup(PageFormat::A0, Orientation::Portrait),
        setup(PageFormat::Letter, Orientation::Landscape),
        setup(
            PageFormat::Custom {
                width_mm: 320.0,
                height_mm: 260.0,
            },
            Orientation::Portrait,
        ),
        setup(PageFormat::Roll { width_mm: 914.0 }, Orientation::Portrait),
        PageSetup {
            margins: margins(3.0, 4.0, 30.0, 6.0),
            ..setup(PageFormat::A3, Orientation::Portrait)
        },
    ] {
        let pages = read_pdf(
            &export_pdf_with(std::slice::from_ref(&scene), &s)
                .unwrap()
                .bytes,
        );
        let n150: usize = pages
            .iter()
            .skip(1)
            .map(|p| p.lines.iter().filter(|l| near(len(l), 150.0, 1e-3)).count())
            .sum();
        let n80: usize = pages
            .iter()
            .skip(1)
            .map(|p| p.lines.iter().filter(|l| near(len(l), 80.0, 1e-3)).count())
            .sum();
        assert_eq!((n150, n80), (2, 2), "{s:?}");
    }
}

#[test]
fn contenu_plus_grand_que_la_zone_imprimable_decoupe_jamais_reduit() {
    // 600 × 450 mm en A4 : plusieurs pages, chaque côté garde sa longueur sur les pages.
    let scene = rect_scene(600.0, 450.0);
    let s = setup(PageFormat::A4, Orientation::Landscape);
    let tiles = plan_tiles_with(&scene, &s).unwrap();
    assert!(tiles.len() > 1);
    let pages = read_pdf(
        &export_pdf_with(std::slice::from_ref(&scene), &s)
            .unwrap()
            .bytes,
    );
    // Les côtés de 600 mm apparaissent, coupés par la fenêtre, avec leur longueur vraie
    // (le découpage est fait par la fenêtre `re W n`, le tracé n'est jamais redimensionné).
    let count = |target: f64| -> usize {
        pages
            .iter()
            .skip(pages.len() - tiles.len())
            .map(|p| {
                p.lines
                    .iter()
                    .filter(|l| near(len(l), target, 1e-3))
                    .count()
            })
            .sum()
    };
    // Chaque page de patron porte les deux côtés de 600 mm et les deux de 450 mm, entiers.
    assert_eq!(count(600.0), 2 * tiles.len());
    assert_eq!(count(450.0), 2 * tiles.len());
    // Un rouleau trop étroit pour ce contenu refuse au lieu de réduire.
    let narrow = setup(PageFormat::Roll { width_mm: 400.0 }, Orientation::Portrait);
    assert!(matches!(
        export_pdf_with(std::slice::from_ref(&scene), &narrow),
        Err(ExportError::Page(PageSetupError::ContentTooWide { .. }))
    ));
}

#[test]
fn la_legende_trop_longue_passe_sur_une_page_de_suite_dans_la_zone_imprimable() {
    // Une scène qui utilise les 14 rôles : la légende ne tient pas sous l'étalon en A4
    // paysage (190 mm de zone imprimable en hauteur).
    let mut scene = rect_scene(100.0, 100.0);
    let roles: Vec<PathRole> = PathRole::BASE
        .iter()
        .chain(PathRole::SPECIAL.iter())
        .copied()
        .collect();
    for (i, role) in roles.into_iter().enumerate() {
        let y = 10.0 + i as f64;
        scene.paths.push(ScenePath {
            geom: pf_export::PathGeom::Line(pt(0.0, y), pt(50.0, y)),
            role,
            sense: pf_core::GrainSense::Unmarked,
        });
    }
    let s = setup(PageFormat::A4, Orientation::Landscape);
    let out = export_pdf_with(std::slice::from_ref(&scene), &s).unwrap();
    let pages = read_pdf(&out.bytes);
    assert_eq!(pages.len(), out.page_count);
    assert_eq!(
        page_count_with(std::slice::from_ref(&scene), &s).unwrap(),
        out.page_count
    );
    assert!(pages[1].text.contains("suite"), "page de suite attendue");
    for page in &pages[..2] {
        let (pw, ph) = page.media_mm();
        for &(x, y) in &page.texts {
            assert!(x >= 10.0 - 1e-6 && x <= pw - 10.0);
            assert!(
                y >= 10.0 - 1e-6 && y <= ph - 10.0,
                "texte à y = {y} hors zone"
            );
        }
    }
    // Le texte de synthèse annonce le bon nombre de pages.
    let all_text: String = pages.iter().map(|p| p.text.clone()).collect();
    assert!(
        all_text.contains(&format!("{} page(s) A4 paysage", out.page_count)),
        "{all_text}"
    );
}

// ---------------------------------------------------------------------------
// CA : défauts, déterminisme
// ---------------------------------------------------------------------------

#[test]
fn le_reglage_par_defaut_est_l_export_historique_a_l_octet() {
    let project = pf_core::demo_piece_project().unwrap();
    let scene = Scene::from_sheet(&project, project.sheets[0].id).unwrap();
    let a = export_pdf(std::slice::from_ref(&scene)).unwrap();
    let b = export_pdf_with(std::slice::from_ref(&scene), &PageSetup::default()).unwrap();
    assert_eq!(a, b);
    assert!(PageSetup::default().is_default());
    // Étalon A4 : coordonnées historiques (carré de 55 à 155 mm en x, de 150 à 250 en y).
    let pages = read_pdf(&b.bytes);
    let sq = pages[0]
        .rects
        .iter()
        .find(|r| near(r.2, 100.0, 1e-3))
        .unwrap();
    assert!(near(sq.0, 55.0, 1e-3) && near(sq.1, 150.0, 1e-3));
    assert!(pages[0].text.contains("page(s) A4, étalon compris"));
    assert!(near(pages[0].media_pt.0, A4_PT.0, 0.005));
    // Les fichiers `examples/` (comparés par `exemples.rs`) sont produits par ce même chemin.
    assert_eq!(pages.len(), b.page_count);
}

#[test]
fn export_deterministe_pour_chaque_reglage() {
    let scene = rect_scene(300.0, 200.0);
    for s in [
        PageSetup::default(),
        setup(PageFormat::A0, Orientation::Landscape),
        setup(PageFormat::Roll { width_mm: 500.0 }, Orientation::Portrait),
        PageSetup {
            margins: margins(1.0, 2.0, 3.0, 4.0),
            ..PageSetup::default()
        },
    ] {
        let a = export_pdf_with(std::slice::from_ref(&scene), &s).unwrap();
        let b = export_pdf_with(std::slice::from_ref(&scene), &s).unwrap();
        assert_eq!(a, b);
    }
}

#[test]
fn feuille_vide_et_trop_de_pages_restent_refuses() {
    let mut empty = rect_scene(10.0, 10.0);
    empty.paths.clear();
    empty.points.clear();
    assert!(matches!(
        export_pdf_with(&[empty], &PageSetup::default()),
        Err(ExportError::EmptySheet(_))
    ));
    let huge = rect_scene(5000.0, 5000.0);
    assert!(matches!(
        export_pdf_with(std::slice::from_ref(&huge), &PageSetup::default()),
        Err(ExportError::TooManyPages { .. })
    ));
    // Le même contenu est découpé (jamais réduit) sur le plus grand format libre : 2 × 2
    // pages de patron plus l'étalon.
    let big = setup(
        PageFormat::Custom {
            width_mm: 5000.0,
            height_mm: 5000.0,
        },
        Orientation::Portrait,
    );
    assert_eq!(export_pdf_with(&[huge], &big).unwrap().page_count, 5);
}

// ---------------------------------------------------------------------------
// Campagne adverse : réglages et contenus tirés au hasard, graine fixe
// ---------------------------------------------------------------------------

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        // xorshift64*
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
    fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.unit()
    }
    fn pick<T: Copy>(&mut self, xs: &[T]) -> T {
        xs[(self.next() % xs.len() as u64) as usize]
    }
}

fn weird(rng: &mut Rng, lo: f64, hi: f64) -> f64 {
    if rng.next() % 4 != 0 {
        return rng.range(lo, hi);
    }
    match rng.next() % 12 {
        0 => f64::NAN,
        1 => f64::INFINITY,
        2 => f64::NEG_INFINITY,
        3 => 0.0,
        4 => -rng.range(0.0, 50.0),
        5 => lo - 1e-9,
        6 => hi + 1e-9,
        7 => lo,
        8 => hi,
        9 => 1e300,
        _ => rng.range(lo, hi),
    }
}

#[test]
fn campagne_adverse_formats_marges_rouleaux_graine_fixe() {
    let mut rng = Rng(0x00C0_FFEE_2026_0013);
    let mut accepted = 0;
    let mut refused = 0;
    for seq in 0..240 {
        let format = match rng.next() % 6 {
            0 => PageFormat::A4,
            1 => PageFormat::A3,
            2 => PageFormat::A0,
            3 => PageFormat::Letter,
            4 => PageFormat::Custom {
                width_mm: weird(&mut rng, 190.0, 5000.0),
                height_mm: weird(&mut rng, 190.0, 5000.0),
            },
            _ => PageFormat::Roll {
                width_mm: weird(&mut rng, 190.0, 5000.0),
            },
        };
        let s = PageSetup {
            format,
            orientation: rng.pick(&[Orientation::Portrait, Orientation::Landscape]),
            margins: margins(
                weird(&mut rng, 0.0, 100.0),
                weird(&mut rng, 0.0, 100.0),
                weird(&mut rng, 0.0, 100.0),
                weird(&mut rng, 0.0, 100.0),
            ),
            ..PageSetup::default()
        };
        let n_scenes = (rng.next() % 3) as usize;
        let scenes: Vec<Scene> = (0..n_scenes)
            .map(|_| match rng.next() % 5 {
                0 => rect_scene(rng.range(0.0, 3.0), rng.range(0.0, 3.0)),
                1 => rect_scene(rng.range(10.0, 400.0), rng.range(10.0, 400.0)),
                2 => rect_scene(rng.range(500.0, 1500.0), rng.range(500.0, 2500.0)),
                3 => rect_scene(rng.range(1.0, 100.0), rng.range(1000.0, 25_000.0)),
                _ => rect_scene(rng.range(1.0, 7000.0), rng.range(1.0, 7000.0)),
            })
            .collect();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            export_pdf_with(&scenes, &s)
        }));
        let result = result.unwrap_or_else(|_| panic!("panique, séquence {seq} : {s:?}"));
        let valid = s.validate().is_ok();
        match result {
            Ok(out) => {
                accepted += 1;
                assert!(valid, "séquence {seq} : réglage invalide accepté {s:?}");
                let pages = read_pdf(&out.bytes);
                assert_eq!(pages.len(), out.page_count);
                assert_eq!(out.page_sizes_mm.len(), pages.len());
                assert_eq!(page_count_with(&scenes, &s).unwrap(), out.page_count);
                for (p, (w, h)) in pages.iter().zip(&out.page_sizes_mm) {
                    let (mw, mh) = p.media_mm();
                    assert!(near(mw, *w, 0.002) && near(mh, *h, 0.002), "séquence {seq}");
                    assert!(
                        *w >= 190.0 && *w <= 5000.0 + 1e-9,
                        "séquence {seq} : largeur {w}"
                    );
                    assert!(
                        *h > 0.0 && *h <= 20_000.0 + 60.0 + 200.0,
                        "séquence {seq} : {h}"
                    );
                    // Chaque trait reste sur la page (hors découpe : seuls les cadres sont
                    // garantis ; les tracés sont découpés par la fenêtre).
                    for r in p.rects.iter().filter(|r| r.4) {
                        assert!(r.0 >= -1e-3 && r.1 >= -1e-3, "séquence {seq}");
                        assert!(
                            r.0 + r.2 <= w + 1e-3 && r.1 + r.3 <= h + 1e-3,
                            "séquence {seq}"
                        );
                    }
                }
                // Déterminisme.
                assert_eq!(export_pdf_with(&scenes, &s).unwrap(), out);
            }
            Err(e) => {
                refused += 1;
                let known = matches!(
                    e,
                    ExportError::Page(_)
                        | ExportError::TooManyPages { .. }
                        | ExportError::EmptySheet(_)
                );
                assert!(known, "séquence {seq} : erreur inattendue {e:?}");
                if !valid {
                    assert!(matches!(e, ExportError::Page(_)), "séquence {seq} : {e:?}");
                }
                assert!(!e.to_string().is_empty());
            }
        }
    }
    assert!(
        accepted >= 20 && refused >= 20,
        "{accepted} acceptés, {refused} refusés"
    );
}

#[test]
fn dimensions_de_l_apercu() {
    // Valeurs lues par l'aperçu de l'interface.
    let s = setup(PageFormat::A3, Orientation::Landscape);
    assert_eq!(
        (s.page_width_mm(), s.page_height_mm()),
        (420.0, Some(297.0))
    );
    assert_eq!(
        (s.printable_width_mm(), s.printable_height_mm()),
        (400.0, Some(277.0))
    );
    assert_eq!(s.describe(), "A3 paysage");
    let roll = setup(PageFormat::Roll { width_mm: 914.0 }, Orientation::Landscape);
    assert_eq!(roll.describe(), "rouleau 914 mm");
    assert_eq!(roll.printable_height_mm(), None);
}

/// D-05 (corrigé avec US-06.6) : la page étalon A4 **par défaut** passe désormais à une page de
/// suite quand la légende ne tient pas (14 rôles, des boutons, une feuille) ; auparavant la fin
/// de la légende descendait sous la marge basse, jusque hors de la page. Quand la légende tient,
/// la page est inchangée (`le_reglage_par_defaut_est_l_export_historique_a_l_octet`).
#[test]
fn d05_la_legende_de_l_etalon_a4_par_defaut_passe_sur_une_page_de_suite() {
    let mut scene = rect_scene(100.0, 100.0);
    let roles: Vec<PathRole> = PathRole::BASE
        .iter()
        .chain(PathRole::SPECIAL.iter())
        .copied()
        .collect();
    for (i, role) in roles.into_iter().enumerate() {
        let y = 10.0 + i as f64;
        scene.paths.push(ScenePath {
            geom: pf_export::PathGeom::Line(pt(0.0, y), pt(50.0, y)),
            role,
            sense: pf_core::GrainSense::Unmarked,
        });
    }
    scene.buttons.push(pf_export::SceneButton {
        pos: pt(50.0, 50.0),
        shape: pf_core::ButtonShape::Button { diameter_mm: 10.0 },
    });
    let out = export_pdf(std::slice::from_ref(&scene)).unwrap();
    let pages = read_pdf(&out.bytes);
    // Étalon sur deux pages (page de suite), puis les pages de patron.
    assert_eq!(
        out.page_count,
        2 + plan_tiles_with(&scene, &PageSetup::default())
            .unwrap()
            .len()
    );
    assert!(pages[1].text.contains("page étalon (suite)"));
    // Aucun texte de l'étalon ni de sa suite sous la marge basse de 10 mm.
    for (i, page) in pages[..2].iter().enumerate() {
        let lowest = page.texts.iter().map(|t| t.1).fold(f64::INFINITY, f64::min);
        assert!(lowest >= 10.0, "page {i} : y le plus bas = {lowest}");
    }
}

#[test]
fn nombre_de_pages_etalon_par_format_constate() {
    let pages = |s: &PageSetup| export_calibration_pdf_with(s).unwrap().page_count;
    assert_eq!(pages(&PageSetup::default()), 1);
    assert_eq!(pages(&setup(PageFormat::A3, Orientation::Landscape)), 1);
    assert_eq!(pages(&setup(PageFormat::A0, Orientation::Portrait)), 1);
    assert_eq!(
        pages(&setup(
            PageFormat::Roll { width_mm: 600.0 },
            Orientation::Portrait
        )),
        1
    );
    // En A4 paysage (190 mm utiles en hauteur) la légende passe sur une page de suite.
    assert_eq!(pages(&setup(PageFormat::A4, Orientation::Landscape)), 2);
}
