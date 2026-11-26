//! US-09.8 — export gradé : une taille, une sélection de tailles, le nid. Rectangle 400 × 600 en
//! base M, gamme XS S M L XL ; écarts cumulés `S (−5 ; −10)`, `L (+5 ; +10)`, `XL (+10 ; +22)`,
//! `XS (−9 ; −20)` : A fixe, B `(dx ; 0)`, C `(dx ; dy)`, D `(0 ; dy)`. Toutes les valeurs
//! attendues sont **calculées à la main** (jamais relues du code testé) ; les lecteurs SVG et PDF
//! sont ceux, indépendants, de `common/`.

mod common;

use common::contenu::{read_svg, Group};
use common::{read_pdf, Page};
use pf_core::*;
use pf_export::size_export::*;
use pf_export::{pdf, svg, ExportError, OutputContent, PageSetup, PathGeom, Scene, SceneOptions};
use pf_geometry::PointMm;
use std::fs;
use std::path::PathBuf;

const TOL: f64 = 1e-6;
/// (taille, dx, dy) cumulés depuis la base M.
const DELTAS: [(&str, f64, f64); 5] = [
    ("XS", -9.0, -20.0),
    ("S", -5.0, -10.0),
    ("M", 0.0, 0.0),
    ("L", 5.0, 10.0),
    ("XL", 10.0, 22.0),
];

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

fn examples() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples")
}

/// Dossier de travail **dans `target/`** (jamais à la racine du disque ni dans /tmp).
fn work_dir(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("export_gradue_{name}"));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn size(p: &Project, label: &str) -> SizeId {
    p.sizes
        .iter()
        .flat_map(|r| r.sizes.iter())
        .find(|s| s.label == label)
        .unwrap_or_else(|| panic!("taille {label}"))
        .id
}

struct Fx {
    p: Project,
    sheet: SheetId,
    range: SizeRangeId,
    /// A, B, C, D.
    corners: [PointId; 4],
}

fn rect() -> Fx {
    let mut p = Project::new("Export gradué");
    let sheet = p.sheets[0].id;
    p.apply(Command::AddSizeRange {
        name: "Femme".into(),
        kind: SizeKind::Alphanumeric,
        labels: DELTAS.iter().map(|d| d.0.to_string()).collect(),
        base: 2,
    })
    .unwrap();
    let range = p.sizes[0].id;
    let polygon = p
        .apply(Command::AddPolygon {
            sheet,
            vertices: [(0.0, 0.0), (400.0, 0.0), (400.0, 600.0), (0.0, 600.0)]
                .iter()
                .map(|&(x, y)| PointRef::New(pt(x, y)))
                .collect(),
            closed: true,
            role: PathRole::Construction,
            layer: None,
        })
        .unwrap();
    let corners = [
        polygon.points[0],
        polygon.points[1],
        polygon.points[2],
        polygon.points[3],
    ];
    for (label, dx, dy) in DELTAS.into_iter().filter(|t| t.0 != "M") {
        let s = size(&p, label);
        for (point, gx, gy) in [
            (corners[1], dx, 0.0),
            (corners[2], dx, dy),
            (corners[3], 0.0, dy),
        ] {
            p.apply(Command::SetPointGrading {
                sheet,
                point,
                size: s,
                dx_mm: gx,
                dy_mm: gy,
            })
            .unwrap();
        }
    }
    Fx {
        p,
        sheet,
        range,
        corners,
    }
}

fn json(p: &Project) -> String {
    persist::to_json(p).expect("projet valide")
}

fn scene(f: &Fx, size_id: Option<SizeId>) -> Scene {
    size_scene(
        &f.p,
        size_id,
        f.sheet,
        &OutputContent::default(),
        SceneOptions::default(),
    )
    .unwrap()
}

fn svg_of(f: &Fx, size_id: Option<SizeId>) -> String {
    svg::export_svg(&scene(f, size_id)).unwrap()
}

fn pdf_of(f: &Fx, size_id: Option<SizeId>) -> Vec<u8> {
    pdf::export_pdf_content(
        &[scene(f, size_id)],
        &PageSetup::default(),
        &OutputContent::default(),
    )
    .unwrap()
    .bytes
}

// ----- CA1 : la taille de base sort à l'octet près comme avant ---------------------------------

#[test]
fn ca1_base_octets_identiques_a_l_export_d_avant_sur_le_rectangle() {
    let f = rect();
    let plain = Scene::from_sheet(&f.p, f.sheet).unwrap();
    let want_svg = svg::export_svg(&plain).unwrap();
    let want_pdf = pdf::export_pdf(&[plain]).unwrap().bytes;
    for choice in [None, Some(size(&f.p, "M"))] {
        assert_eq!(svg_of(&f, choice), want_svg, "{choice:?}");
        assert_eq!(pdf_of(&f, choice), want_pdf, "{choice:?}");
    }
    // Aucun nom de taille dans la sortie de base.
    assert!(!want_svg.contains("Taille"));
}

#[test]
fn ca1_exemples_livres_identiques_avec_ou_sans_gamme_de_tailles() {
    for stem in ["jupe-droite", "porte-cartes", "geometrie-exercice"] {
        let mut project = persist::load(&examples().join(format!("{stem}.pforge"))).unwrap();
        let sheet = project.sheets[0].id;
        let want_svg = fs::read(examples().join(format!("{stem}.svg"))).unwrap();
        let want_pdf = fs::read(examples().join(format!("{stem}.pdf"))).unwrap();
        let check = |p: &Project, choice: Option<SizeId>, what: &str| {
            let scene = size_scene(
                p,
                choice,
                sheet,
                &OutputContent::default(),
                SceneOptions::default(),
            )
            .unwrap();
            assert_eq!(
                svg::export_svg(&scene).unwrap().into_bytes(),
                want_svg,
                "{stem} {what} : SVG"
            );
            assert_eq!(
                pdf::export_pdf(&[scene]).unwrap().bytes,
                want_pdf,
                "{stem} {what} : PDF"
            );
        };
        check(&project, None, "sans gamme");
        // Avec une gamme : « aucune taille » et la taille de base lisent le projet tel quel.
        project
            .apply(Command::AddSizeRange {
                name: "Gamme".into(),
                kind: SizeKind::Alphanumeric,
                labels: ["S", "M", "L"].iter().map(|s| s.to_string()).collect(),
                base: 1,
            })
            .unwrap();
        let base = size(&project, "M");
        check(&project, None, "gamme, aucune taille");
        check(&project, Some(base), "gamme, taille de base");
    }
}

// ----- CA2 : une taille, coordonnées et cotes calculées à la main ------------------------------

#[test]
fn ca2_svg_de_la_taille_l_aux_coordonnees_calculees_a_la_main() {
    let f = rect();
    let text = svg_of(&f, Some(size(&f.p, "L")));
    // L : rectangle 405 × 610 (A (0 ; 0), B (405 ; 0), C (405 ; 610), D (0 ; 610)) ; emprise
    // 405 × 610 + marge 10 mm de chaque côté ; SVG = (x + 10 ; 610 − y + 10).
    let doc = read_svg(&text);
    assert!((doc.width - 425.0).abs() < TOL && (doc.height - 630.0).abs() < TOL);
    let lines: Vec<Vec<(f64, f64)>> = doc
        .elems
        .iter()
        .filter(|e| e.group == Group::Paths && e.tag == "line")
        .map(|e| e.pts.clone())
        .collect();
    let want = [
        [(10.0, 620.0), (415.0, 620.0)],
        [(415.0, 620.0), (415.0, 10.0)],
        [(415.0, 10.0), (10.0, 10.0)],
        [(10.0, 10.0), (10.0, 620.0)],
    ];
    assert_eq!(lines.len(), 4, "{lines:?}");
    for (got, want) in lines.iter().zip(want) {
        for (g, w) in got.iter().zip(want) {
            assert!(
                (g.0 - w.0).abs() < TOL && (g.1 - w.1).abs() < TOL,
                "{got:?} ≠ {want:?}"
            );
        }
    }
    // Les quatre points de la feuille sont aux mêmes coins.
    let circles: Vec<(f64, f64)> = doc
        .elems
        .iter()
        .filter(|e| e.tag == "circle")
        .map(|e| e.pts[0])
        .collect();
    for w in [(10.0, 620.0), (415.0, 620.0), (415.0, 10.0), (10.0, 10.0)] {
        assert!(
            circles
                .iter()
                .any(|c| (c.0 - w.0).abs() < TOL && (c.1 - w.1).abs() < TOL),
            "point {w:?} absent de {circles:?}"
        );
    }
    // Le nom de la taille est dans le titre ; jamais pour la base.
    assert!(
        text.contains("<title>Export gradué — Taille L · "),
        "{text}"
    );
}

#[test]
fn ca2_chaque_taille_a_ses_cotes_svg_et_pdf() {
    let f = rect();
    for (label, dx, dy) in DELTAS {
        let id = size(&f.p, label);
        let (w, h) = (400.0 + dx, 600.0 + dy);
        let doc = read_svg(&svg_of(&f, Some(id)));
        assert!(
            (doc.width - (w + 20.0)).abs() < TOL && (doc.height - (h + 20.0)).abs() < TOL,
            "{label} : {} × {}",
            doc.width,
            doc.height
        );
        // PDF : des traits de w et h mm (mm → pt 72 / 25,4 relu en mm), et aucun de 400 / 600
        // hors de la taille de base.
        let pages = read_pdf(&pdf_of(&f, Some(id)));
        let has = |len: f64| {
            pages
                .iter()
                .flat_map(|p| &p.lines)
                .any(|l| (l.len() - len).abs() < 0.01)
        };
        assert!(
            has(w) && has(h),
            "{label} : traits de {w} et {h} mm absents"
        );
        if label != "M" {
            assert!(!has(400.0) && !has(600.0), "{label} : cotes de la base");
            assert!(
                pages.iter().any(|p| p.has(&format!("Taille {label}"))),
                "{label} : en-tête de page sans nom de taille"
            );
        }
    }
}

#[test]
fn ca2_carre_de_100_mm_intact_dans_toutes_les_tailles() {
    let f = rect();
    let first_page =
        |id: Option<SizeId>| -> Page { read_pdf(&pdf_of(&f, id)).into_iter().next().unwrap() };
    let base = first_page(None);
    let squares = |p: &Page| {
        p.rects
            .iter()
            .filter(|r| (r.2 - 100.0).abs() < 0.01 && (r.3 - 100.0).abs() < 0.01)
            .count()
    };
    assert_eq!(squares(&base), 1, "carré étalon de la base");
    for (label, _, _) in DELTAS {
        let page = first_page(Some(size(&f.p, label)));
        assert_eq!(squares(&page), squares(&base), "{label}");
        assert_eq!(page.media_pt, base.media_pt, "{label}");
    }
}

#[test]
fn ca2_taille_inconnue_refusee_jamais_la_base_en_silence() {
    let f = rect();
    let r = size_scene(
        &f.p,
        Some(SizeId(987_654)),
        f.sheet,
        &OutputContent::default(),
        SceneOptions::default(),
    );
    assert!(matches!(r, Err(ExportError::Core(_))), "{r:?}");
}

// ----- CA3 : noms de fichiers ------------------------------------------------------------------

#[test]
fn ca3_noms_assainis_et_taille_de_base_sans_suffixe() {
    let f = rect();
    let all = export_sizes(&f.p);
    assert_eq!(all.len(), 5);
    let find = |l: &str| all.iter().find(|s| s.label == l).unwrap();
    assert!(find("M").is_base && !find("L").is_base);
    assert_eq!(size_file_stem("Jupe", Some(find("M"))), "Jupe");
    assert_eq!(size_file_stem("Jupe", None), "Jupe");
    assert_eq!(size_file_stem("Jupe", Some(find("L"))), "Jupe-L");
    assert_eq!(default_export_size(&f.p), Some(size(&f.p, "M")));
    // Caractères interdits des noms de fichier : tous remplacés.
    assert_eq!(sanitize_file_part("A/B"), "A_B");
    assert_eq!(
        sanitize_file_part("a:b*c?d\"e<f>g|h\\i"),
        "a_b_c_d_e_f_g_h_i"
    );
    assert_eq!(sanitize_file_part("36 Long."), "36_Long_");
    assert_eq!(sanitize_file_part("  "), "x");
    // Casse : « XL » et « xl » se confondraient sous Windows.
    let names: Vec<String> = ["a-XL", "A-xl", "a-xl", "b"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(unique_file_names(&names), ["a-XL", "A-xl-2", "a-xl-3", "b"]);
}

#[test]
fn ca3_libelles_qui_se_confondent_apres_assainissement_donnent_des_fichiers_distincts() {
    let mut f = rect();
    // Le modèle refuse deux libellés égaux à la casse près ; « A/B » et « A_B » sont distincts
    // pour lui mais se confondent comme noms de fichier.
    f.p.apply(Command::AddSizeRange {
        name: "Bizarre".into(),
        kind: SizeKind::Alphanumeric,
        labels: ["A/B", "A_B", "a:b"]
            .iter()
            .map(|s| s.to_string())
            .collect(),
        base: 0,
    })
    .unwrap();
    let ids: Vec<SizeId> = f.p.sizes[1].sizes.iter().map(|s| s.id).collect();
    let files = size_files(
        &f.p,
        f.sheet,
        &ids,
        "Projet",
        SizeFormat::Svg,
        &PageSetup::default(),
        &OutputContent::default(),
        SceneOptions::default(),
    );
    let names: Vec<&str> = files.iter().map(|x| x.file_name.as_str()).collect();
    assert_eq!(
        names,
        ["Projet-A_B.svg", "Projet-A_B-2.svg", "Projet-a_b-3.svg"]
    );
    for n in names {
        assert!(
            !n.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|']),
            "{n}"
        );
    }
}

// ----- CA4 : sélection de tailles --------------------------------------------------------------

fn all_sizes(f: &Fx) -> Vec<SizeId> {
    f.p.sizes[0].sizes.iter().map(|s| s.id).collect()
}

#[test]
fn ca4_une_taille_un_fichier_distinct_pdf_et_svg() {
    let f = rect();
    for format in [SizeFormat::Svg, SizeFormat::Pdf] {
        let files = size_files(
            &f.p,
            f.sheet,
            &all_sizes(&f),
            "Jupe",
            format,
            &PageSetup::default(),
            &OutputContent::default(),
            SceneOptions::default(),
        );
        assert_eq!(files.len(), 5);
        let ext = format.extension();
        let names: Vec<String> = files.iter().map(|x| x.file_name.clone()).collect();
        assert_eq!(
            names,
            ["XS", "S", "M", "L", "XL"].map(|l| format!("Jupe-{l}.{ext}"))
        );
        // Cinq contenus distincts (chaque taille a sa géométrie).
        let bytes: Vec<&Vec<u8>> = files.iter().map(|x| x.bytes.as_ref().unwrap()).collect();
        for i in 0..5 {
            for j in i + 1..5 {
                assert_ne!(bytes[i], bytes[j], "{i} et {j} identiques");
            }
        }
        // La taille de base de la sélection est identique à l'export de base.
        let base = if format == SizeFormat::Svg {
            svg_of(&f, None).into_bytes()
        } else {
            pdf_of(&f, None)
        };
        assert_eq!(bytes[2], &base);
        // Doublon dans la sélection : un seul fichier.
        let dup = size_files(
            &f.p,
            f.sheet,
            &[size(&f.p, "L"), size(&f.p, "L")],
            "Jupe",
            format,
            &PageSetup::default(),
            &OutputContent::default(),
            SceneOptions::default(),
        );
        assert_eq!(dup.len(), 1);
    }
}

#[test]
fn ca4_ecriture_dans_le_dossier_choisi_seulement_et_echec_partiel_liste() {
    let f = rect();
    let dir = work_dir("partiel");
    let mut ids = all_sizes(&f);
    // Une taille inconnue : son erreur est listée, les autres sont écrites.
    ids.push(SizeId(987_654));
    let files = size_files(
        &f.p,
        f.sheet,
        &ids,
        "Jupe",
        SizeFormat::Svg,
        &PageSetup::default(),
        &OutputContent::default(),
        SceneOptions::default(),
    );
    // Un dossier du nom du fichier de L : l'écriture de L échoue.
    fs::create_dir(dir.join("Jupe-L.svg")).unwrap();
    let report = write_size_files(&dir, &files);
    let written: Vec<String> = report
        .written
        .iter()
        .map(|(_, p)| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        written,
        ["Jupe-XS.svg", "Jupe-S.svg", "Jupe-M.svg", "Jupe-XL.svg"]
    );
    let failed: Vec<(&str, bool)> = report
        .failed
        .iter()
        .map(|x| (x.label.as_str(), matches!(x.error, ExportError::Io(_))))
        .collect();
    assert_eq!(failed, [("L", true), ("id987654", false)]);
    assert!(matches!(report.failed[1].error, ExportError::Core(_)));
    // Rien d'autre dans le dossier : les quatre fichiers et le dossier posé par le test.
    let mut found: Vec<String> = fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    found.sort();
    assert_eq!(
        found,
        [
            "Jupe-L.svg",
            "Jupe-M.svg",
            "Jupe-S.svg",
            "Jupe-XL.svg",
            "Jupe-XS.svg"
        ]
    );
    // Le fichier de XL contient la géométrie de XL : 410 × 622 mm + marges.
    let xl = fs::read_to_string(dir.join("Jupe-XL.svg")).unwrap();
    let doc = read_svg(&xl);
    assert!((doc.width - 430.0).abs() < TOL && (doc.height - 642.0).abs() < TOL);
    // Dossier inexistant : toutes les tailles échouent, rien n'est créé ailleurs.
    let missing = dir.join("absent").join("sous-dossier");
    let report = write_size_files(&missing, &files[..2]);
    assert_eq!((report.written.len(), report.failed.len()), (0, 2));
    assert!(!missing.exists());
}

// ----- CA5 : nid -------------------------------------------------------------------------------

/// Durée de trait d'un tronçon de `len` mm pour un motif (trait, vide, …), calculée par périodes
/// entières puis par parcours du reste.
fn on_len(len: f64, pattern: &[f64]) -> f64 {
    if pattern.is_empty() {
        return len;
    }
    let period: f64 = pattern.iter().sum();
    let per_period: f64 = pattern.iter().step_by(2).sum();
    let full = (len / period).floor();
    let mut rest = len - full * period;
    let mut on = full * per_period;
    for (i, e) in pattern.iter().enumerate() {
        let part = rest.min(*e);
        if i % 2 == 0 {
            on += part;
        }
        rest -= part;
        if rest <= 0.0 {
            break;
        }
    }
    on
}

/// Motifs attendus, par rang parmi les tailles autres que la base (écrits à la main).
const PATTERNS: [&[f64]; 6] = [
    &[8.0, 5.0],
    &[1.5, 3.0],
    &[10.0, 3.0, 1.5, 3.0],
    &[18.0, 5.0],
    &[10.0, 3.0, 1.5, 3.0, 1.5, 3.0],
    &[3.0, 2.5],
];

fn segments(scene: &Scene) -> Vec<(PointMm, PointMm)> {
    scene
        .paths
        .iter()
        .filter_map(|p| match p.geom {
            PathGeom::Line(a, b) => Some((a, b)),
            _ => None,
        })
        .collect()
}

fn dist_to_segment(p: PointMm, a: PointMm, b: PointMm) -> f64 {
    let (vx, vy) = (b.x - a.x, b.y - a.y);
    let t = (((p.x - a.x) * vx + (p.y - a.y) * vy) / (vx * vx + vy * vy)).clamp(0.0, 1.0);
    (p.x - (a.x + t * vx)).hypot(p.y - (a.y + t * vy))
}

#[test]
fn ca5_nid_contours_translates_motifs_et_etiquettes() {
    let f = rect();
    // Point d'empilement C : dans la taille T, C est en (400 + dx ; 600 + dy), ramené en
    // (400 ; 600) : décalage (−dx ; −dy).
    let nest =
        f.p.size_nest(f.sheet, f.range, NestAnchor::Point(f.corners[2]))
            .unwrap();
    let all: Vec<SizeId> = all_sizes(&f);
    let out = nest_scene(&f.p, &nest, &all, pf_i18n::Lang::Fr).unwrap();
    assert!(out.issues.is_empty());
    // Rectangle de chaque taille, translaté à la main : A (−dx ; −dy), B (400 ; −dy),
    // C (400 ; 600), D (−dx ; 600).
    let edges_of = |dx: f64, dy: f64| {
        let (a, b, c, d) = (
            pt(-dx, -dy),
            pt(400.0, -dy),
            pt(400.0, 600.0),
            pt(-dx, 600.0),
        );
        [(a, b), (b, c), (c, d), (d, a)]
    };
    // Chaque taille seule (cochée seule) : ses tronçons de contour sont les premiers du dessin,
    // les quatre derniers tracés sont le trait de rappel, le soulignement de l'étiquette et la
    // croix du point d'empilement.
    let mut rank = 0;
    for (label, dx, dy) in DELTAS {
        let is_base = label == "M";
        let pattern: &[f64] = if is_base {
            &[]
        } else {
            rank += 1;
            PATTERNS[rank - 1]
        };
        let one = nest_scene(&f.p, &nest, &[size(&f.p, label)], pf_i18n::Lang::Fr).unwrap();
        let all_lines = segments(&one.scene);
        assert_eq!(all_lines.len(), one.scene.paths.len());
        let contour = &all_lines[..all_lines.len() - 4];
        let edges = edges_of(dx, dy);
        // Tout tronçon est sur un bord du rectangle translaté de cette taille.
        for (a, b) in contour {
            assert!(
                edges.iter().any(|e| dist_to_segment(*a, e.0, e.1) < 1e-6
                    && dist_to_segment(*b, e.0, e.1) < 1e-6),
                "{label} : tronçon {a:?} → {b:?} hors des bords"
            );
        }
        let got: f64 = contour
            .iter()
            .map(|(a, b)| pf_geometry::distance_mm(*a, *b))
            .sum();
        let want: f64 = edges
            .iter()
            .map(|(a, b)| on_len(pf_geometry::distance_mm(*a, *b), pattern))
            .sum();
        assert!(
            (got - want).abs() < 1e-6,
            "{label} : {got} mm de trait au lieu de {want}"
        );
        if is_base {
            // La base en plein : exactement les quatre bords, rien de découpé.
            assert_eq!(contour.len(), 4);
            assert!((got - 2000.0).abs() < 1e-6);
            for (a, b) in edges {
                assert!(
                    contour.iter().any(|(p, q)| {
                        (p.x - a.x).abs() < TOL
                            && (p.y - a.y).abs() < TOL
                            && (q.x - b.x).abs() < TOL
                            && (q.y - b.y).abs() < TOL
                    }),
                    "bord de la base {a:?} → {b:?} absent"
                );
            }
        } else {
            // Un motif découpe réellement : plus de tronçons que de bords, moins de trait que
            // de périmètre.
            assert!(contour.len() > 4, "{label} : contour non découpé");
            let perimeter: f64 = edges
                .iter()
                .map(|(a, b)| pf_geometry::distance_mm(*a, *b))
                .sum();
            assert!(got < perimeter, "{label}");
        }
    }
    // Motif de L (rang 2 : 10 / 3 / 1,5 / 3 mm) : bords de 405 et 610 mm, à la main
    // 23 × 11,5 + 2,5 = 267 et 34 × 11,5 + 10 + 1,5 = 402,5 ; deux de chaque.
    let l_edges = edges_of(5.0, 10.0);
    let on: f64 = l_edges
        .iter()
        .map(|(a, b)| on_len(pf_geometry::distance_mm(*a, *b), PATTERNS[2]))
        .sum();
    assert!((on - 1339.0).abs() < 1e-6, "{on}");
    // Étiquettes : une par taille, la base marquée ; point d'empilement nommé.
    let labels: Vec<&str> = out.scene.points.iter().map(|p| p.label.as_str()).collect();
    for want in ["XS", "S", "M (base)", "L", "XL", "Empilement"] {
        assert!(labels.contains(&want), "{want} absent de {labels:?}");
    }
    assert_eq!(labels.len(), 6);
    // Colonne des étiquettes à droite de tous les contours, au pas minimal.
    let mut ys: Vec<f64> = out
        .scene
        .points
        .iter()
        .filter(|p| p.label != "Empilement")
        .map(|p| p.pos.y)
        .collect();
    ys.sort_by(|a, b| b.total_cmp(a));
    for w in ys.windows(2) {
        assert!(w[0] - w[1] >= NEST_LABEL_PITCH_MM - 1e-9, "{ys:?}");
    }
    for p in out.scene.points.iter().filter(|p| p.label != "Empilement") {
        assert!(p.pos.x >= 400.0 + NEST_LABEL_GAP_MM - 1e-9);
    }
    // Sorties : SVG (étiquettes en texte, échelle 1:1 en mm) et PDF (étalon de 100 mm + textes).
    let svg_bytes = nest_svg(&out).unwrap();
    let text = String::from_utf8(svg_bytes).unwrap();
    for want in ["M (base)", "XL", "Empilement"] {
        assert!(text.contains(&format!(">{want}</text>")), "{want}");
    }
    assert!(text.contains("Nid de tailles Femme"));
    let bb = out.scene.bbox().unwrap();
    let doc = read_svg(&text);
    assert!((doc.width - (bb.width() + 20.0)).abs() < 1e-3);
    assert!((doc.height - (bb.height() + 20.0)).abs() < 1e-3);
    let pages = read_pdf(&nest_pdf(&out, &PageSetup::default()).unwrap());
    let squares = pages[0]
        .rects
        .iter()
        .filter(|r| (r.2 - 100.0).abs() < 0.01 && (r.3 - 100.0).abs() < 0.01)
        .count();
    assert_eq!(squares, 1, "carré de 100 mm de l'étalon");
    assert!(pages.iter().any(|p| p.has("M (base)")));
}

#[test]
fn ca5_nid_seules_les_tailles_cochees_et_erreurs_typees() {
    let f = rect();
    let nest = f.p.size_nest(f.sheet, f.range, NestAnchor::Origin).unwrap();
    let (s, l) = (size(&f.p, "S"), size(&f.p, "L"));
    let out = nest_scene(&f.p, &nest, &[l, s], pf_i18n::Lang::En).unwrap();
    let labels: Vec<&str> = out.scene.points.iter().map(|p| p.label.as_str()).collect();
    assert_eq!(labels.len(), 3);
    for want in ["S", "L", "Stack point"] {
        assert!(labels.contains(&want), "{labels:?}");
    }
    // Le motif d'une taille ne dépend pas des autres tailles cochées : S est de rang 1.
    let only_s = nest_scene(&f.p, &nest, &[s], pf_i18n::Lang::En).unwrap();
    assert!(only_s.scene.paths.len() < out.scene.paths.len());
    // Rien de coché : refus, aucun fichier.
    assert!(matches!(
        nest_scene(&f.p, &nest, &[], pf_i18n::Lang::Fr),
        Err(ExportError::NothingSelected(_))
    ));
}

// ----- CA6 : sans effet sur le projet ----------------------------------------------------------

#[test]
fn ca6_aucun_export_ne_modifie_le_projet() {
    let f = rect();
    let before = json(&f.p);
    let copy = f.p.clone();
    for (label, _, _) in DELTAS {
        let _ = svg_of(&f, Some(size(&f.p, label)));
        let _ = pdf_of(&f, Some(size(&f.p, label)));
    }
    let nest =
        f.p.size_nest(f.sheet, f.range, NestAnchor::Point(f.corners[1]))
            .unwrap();
    let out = nest_scene(&f.p, &nest, &all_sizes(&f), pf_i18n::Lang::Fr).unwrap();
    let _ = nest_svg(&out).unwrap();
    let dir = work_dir("effet");
    let files = size_files(
        &f.p,
        f.sheet,
        &all_sizes(&f),
        "Jupe",
        SizeFormat::Pdf,
        &PageSetup::default(),
        &OutputContent::default(),
        SceneOptions::default(),
    );
    let _ = write_size_files(&dir, &files);
    assert_eq!(json(&f.p), before);
    assert!(f.p == copy);
}

#[test]
fn ca_le_chemin_est_le_meme_que_l_export_de_base_pour_dxf_et_hpgl() {
    // Le DXF et le HPGL lisent le projet (ou la scène) de la taille : même chemin, rien de neuf.
    let f = rect();
    let l = size(&f.p, "L");
    let project = size_project(&f.p, Some(l)).unwrap();
    assert!(matches!(project, std::borrow::Cow::Owned(_)));
    let base = size_project(&f.p, Some(size(&f.p, "M"))).unwrap();
    assert!(matches!(base, std::borrow::Cow::Borrowed(_)));
    let scene = scene(&f, Some(l));
    let plots = pf_export::hpgl::export_hpgl_pages(
        &scene,
        &PageSetup::default(),
        pf_export::hpgl::HpglOptions::default(),
    )
    .unwrap();
    assert!(plots.pages.iter().any(|p| p.text.contains("PD")));
}

// ----- US-19.2 : langue des textes des fichiers par taille ------------------------------------

#[test]
fn us192_fichiers_par_taille_dans_la_langue_des_options() {
    let f = rect();
    let ids = all_sizes(&f);
    let files = |format, lang| {
        size_files(
            &f.p,
            f.sheet,
            &ids,
            "Projet",
            format,
            &PageSetup::default(),
            &OutputContent::default(),
            SceneOptions {
                lang,
                ..SceneOptions::default()
            },
        )
    };
    let svg_text = |lang| {
        let out = files(SizeFormat::Svg, lang);
        String::from_utf8(out[0].bytes.as_ref().unwrap().clone()).unwrap()
    };
    let fr = svg_text(pf_i18n::Lang::Fr);
    let en = svg_text(pf_i18n::Lang::En);
    assert!(fr.contains("Contenu : "), "{fr}");
    assert!(en.contains("Content: "), "{en}");
    assert!(!en.contains("Contenu"));
    let pdf_bytes = |lang| {
        files(SizeFormat::Pdf, lang)[0]
            .bytes
            .as_ref()
            .unwrap()
            .clone()
    };
    assert_ne!(
        pdf_bytes(pf_i18n::Lang::Fr),
        pdf_bytes(pf_i18n::Lang::En),
        "la légende du PDF suit la langue"
    );
}
