//! PH-3a — scénario rejouable « jupe droite de base » (devant, sur le pli), de la feuille vide au
//! PDF, au SVG, au DXF et au HPGL, construit **uniquement** par `Project::apply(Command)`.
//!
//! Géométrie (mm, Y vers le haut), contour de couture anti-horaire A → D → C → H → B :
//! A (0 ; 600), D (0 ; 0), C (240 ; 0), H (240 ; 360), B (170 ; 600). Pince de taille : sommet
//! (85 ; 500), axe vers (85 ; 600), longueur 100, ouverture 20 (pieds (75 ; 600) et (95 ; 600)).
//!
//! Valeurs attendues, calculées à la main :
//! - couture : bords 600 (pli) + 240 (bas) + 360 (côté bas) + √(70² + 240²) = 250 (côté haut) +
//!   170 (taille) = 1 620 mm ; aire = 240 × 360 + (240 + 170) / 2 × 240 = 86 400 + 49 200 = 135 600 mm² ;
//! - taille après fermeture de la pince : 170 − 20 = 150 mm ; rotation 2·atan(10 / 100) ;
//! - coupe (marge 10, pli 0, ourlet 40) : côté vertical x = 250 ; le décalage du côté haut passe
//!   par (249,6 ; 362,8) (normale (0,96 ; 0,28) × 10), direction (−70 ; 240) : il coupe x = 250 en
//!   y3 = 362,8 − 240 × 0,4 / 70 et y = 610 en x = 177,5 ; sommets (0 ; −40), (250 ; −40),
//!   (250 ; y3), (177,5 ; 610), (0 ; 610) ; aire = 250 × (y3 + 40) + (250 + 177,5) / 2 × (610 − y3).
//!
//! Pince et étiquette exportées (tranche 69) : jambes sommet → pieds de √(10² + 100²) = √10 100 =
//! 100,4988 mm, calées sur le droit-fil (x = 120, de y = 100 à y = 400) : sommet 35 mm à gauche et
//! 100 mm au-dessus de son extrémité haute, pieds 45 et 25 mm à gauche et 200 mm au-dessus.
//! Étiquette : six lignes de 5 mm de pas, centrées à gauche du droit-fil (voir `ETIQUETTE`).
//!
//! Honnêteté : rien ici ne prouve l'ouverture des fichiers dans un logiciel tiers, ni une impression
//! ou un traçage réels (étapes humaines).

mod common;
#[path = "scenario_jupe/construction.rs"]
mod construction;

use common::coupe::{attr, mm_pt, read_pdf as read_strokes, read_svg};
use common::dxf::{self as rd, Ent};
use common::hpgl::read_hpgl;
use construction::{build, Jupe};
use pf_core::persist;
use pf_export::dxf::export_dxf;
use pf_export::hpgl::export_hpgl;
use pf_export::pdf::export_pdf;
use pf_export::svg::export_svg;
use pf_export::Scene;
use std::fs;
use std::path::PathBuf;

const HINT: &str = "régénérer avec : cargo run -p pf-export --example generer_exemples";

fn near(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

fn examples() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples")
}

/// Ordonnée du sommet de la coupe sur le côté vertical (calcul à la main, voir l'en-tête).
fn y3() -> f64 {
    362.8 - 240.0 * 0.4 / 70.0
}

fn cut_vertices() -> [(f64, f64); 5] {
    [
        (0.0, -40.0),
        (250.0, -40.0),
        (250.0, y3()),
        (177.5, 610.0),
        (0.0, 610.0),
    ]
}

/// Longueurs des bords de la coupe, dans l'ordre des sommets ci-dessus.
fn cut_edge_lengths() -> [f64; 5] {
    [
        250.0,
        y3() + 40.0,
        (610.0 - y3()) * 250.0 / 240.0,
        177.5,
        650.0,
    ]
}

const SEAM_EDGES: [f64; 5] = [600.0, 240.0, 360.0, 250.0, 170.0];
const SEAM_PERIMETER: f64 = 1620.0;
const SEAM_AREA: f64 = 135_600.0;

fn cut_perimeter() -> f64 {
    250.0 + (y3() + 40.0) + (610.0 - y3()) * 250.0 / 240.0 + 177.5 + 650.0
}

fn cut_area() -> f64 {
    250.0 * (y3() + 40.0) + (250.0 + 177.5) / 2.0 * (610.0 - y3())
}

fn shoelace(p: &[(f64, f64)]) -> f64 {
    let n = p.len();
    (0..n)
        .map(|i| p[i].0 * p[(i + 1) % n].1 - p[(i + 1) % n].0 * p[i].1)
        .sum::<f64>()
        .abs()
        / 2.0
}

fn cyc_lengths(p: &[(f64, f64)]) -> Vec<f64> {
    let n = p.len();
    (0..n)
        .map(|i| (p[(i + 1) % n].0 - p[i].0).hypot(p[(i + 1) % n].1 - p[i].1))
        .collect()
}

/// Mêmes longueurs, à l'ordre et au point de départ près.
fn same_lengths(got: &[f64], want: &[f64], tol: f64) -> bool {
    let mut rest = want.to_vec();
    got.len() == want.len()
        && got.iter().all(|g| {
            rest.iter()
                .position(|w| near(*g, *w, tol))
                .map(|i| rest.remove(i))
                .is_some()
        })
}

fn jupe() -> Jupe {
    build().expect("le scénario doit s'exécuter sans refus")
}

// ---------------------------------------------------------------------------
// Mesures du modèle
// ---------------------------------------------------------------------------

#[test]
fn piece_de_couture_perimetre_aire_et_bords() {
    let j = jupe();
    let p = &j.project;
    let c = p.piece_contour(j.piece).unwrap();
    assert!(c.closed && !c.self_intersecting && !c.clockwise);
    assert!(near(c.perimeter_mm, SEAM_PERIMETER, 1e-9), "{c:?}");
    assert!(near(c.area_mm2, SEAM_AREA, 1e-6), "{c:?}");
    let sheet = p.sheet(j.sheet).unwrap();
    for (e, want) in [
        (j.cf, 600.0),
        (j.hem, 240.0),
        (j.side_low, 360.0),
        (j.side_up, 250.0),
        (j.waist, 170.0),
    ] {
        assert!(near(sheet.path_length_mm(e).unwrap(), want, 1e-9));
    }
    // Aucun avertissement de marge ni de pince sur ce patron.
    assert!(p.piece_darts(j.piece)[0].calc.is_ok());
}

#[test]
fn pince_ouverture_fermeture_et_taille() {
    let j = jupe();
    let p = &j.project;
    let calc = p.piece_darts(j.piece)[0].calc.clone().unwrap();
    // Largeur de pince = ouverture donnée ; pieds (75 ; 600) et (95 ; 600) ; sommet (85 ; 500).
    let s = &calc.shape;
    assert!(near(s.opening_mm, 20.0, 1e-9));
    assert!(near(
        (s.right_foot.x - s.left_foot.x).hypot(s.right_foot.y - s.left_foot.y),
        20.0,
        1e-9
    ));
    assert!(near(s.left_foot.x, 75.0, 1e-9) && near(s.left_foot.y, 600.0, 1e-9));
    assert!(near(s.right_foot.x, 95.0, 1e-9) && near(s.right_foot.y, 600.0, 1e-9));
    assert!(near(s.apex.x, 85.0, 1e-9) && near(s.apex.y, 500.0, 1e-9));
    assert!(near(s.leg_left_mm, 10100f64.sqrt(), 1e-9));
    // La pince est portée par la taille ; après fermeture : 170 − 20 = 150 mm.
    assert_eq!(calc.carrier, j.waist);
    assert!(near(calc.closed.before_mm, 170.0, 1e-9));
    assert!(near(calc.closed.removed_mm, 20.0, 1e-9));
    assert!(near(calc.closed.after_mm, 150.0, 1e-9));
    assert!(near(
        calc.closed.rotation_deg,
        2.0 * (10.0f64 / 100.0).atan().to_degrees(),
        1e-9
    ));
    // Tour de taille du devant (1/4 × 4 = 600 mm de développement complet si ×4) : 150 × 4 = 600.
    assert!(near(calc.closed.after_mm * 4.0, 600.0, 1e-9));
    // Le pointage est à 10 mm de la pointe, sur l'axe.
    let marks = p.piece_markings(j.piece);
    assert_eq!(marks.len(), 1);
    assert!(near(marks[0].at.x, 85.0, 1e-9) && near(marks[0].at.y, 510.0, 1e-9));
}

#[test]
fn crans_distances_et_traits() {
    let j = jupe();
    let sheet = j.project.sheet(j.sheet).unwrap();
    assert_eq!(sheet.notches.len(), 4);
    // (tracé, distance depuis le départ du tracé, pied attendu, pointe attendue).
    let want = [
        (j.side_low, 360.0, (240.0, 360.0), (245.0, 360.0)),
        (j.cf, 240.0, (0.0, 360.0), (-5.0, 360.0)),
        (j.waist, 75.0, (95.0, 600.0), (95.0, 605.0)),
        (j.waist, 95.0, (75.0, 600.0), (75.0, 605.0)),
    ];
    for (n, (entity, dist, foot, tip)) in sheet.notches.iter().zip(want) {
        assert_eq!(n.entity, entity);
        assert!(near(sheet.notch_distance_mm(n).unwrap(), dist, 1e-9));
        let marks = sheet.notch_marks(n).unwrap();
        assert_eq!(marks.len(), 1);
        let (f, t) = marks[0];
        assert!(near(f.x, foot.0, 1e-9) && near(f.y, foot.1, 1e-9), "{f:?}");
        assert!(near(t.x, tip.0, 1e-9) && near(t.y, tip.1, 1e-9), "{t:?}");
    }
}

#[test]
fn coupe_avec_marges_et_ourlet() {
    let j = jupe();
    let cut = j.project.piece_cut_contour(j.piece).unwrap();
    assert!(near(cut.perimeter_mm, cut_perimeter(), 1e-6), "{cut:?}");
    assert!(near(cut.area_mm2, cut_area(), 1e-4), "{cut:?}");
    // Valeurs numériques de contrôle (arrondies) : 1 737,857 mm ; 153 489,286 mm².
    assert!(near(cut_perimeter(), 1737.857143, 1e-5));
    assert!(near(cut_area(), 153_489.285714, 1e-4));
    // Chaque sommet attendu est un sommet de la coupe calculée.
    let got: Vec<(f64, f64)> = cut
        .traces
        .iter()
        .map(|t| match t {
            pf_geometry::Trace::Line(a, _) => (a.x, a.y),
            other => panic!("segment attendu : {other:?}"),
        })
        .collect();
    assert_eq!(got.len(), 5, "{got:?}");
    for w in cut_vertices() {
        assert!(
            got.iter()
                .any(|g| near(g.0, w.0, 1e-6) && near(g.1, w.1, 1e-6)),
            "sommet {w:?} absent de {got:?}"
        );
    }
    // Ourlet : le bas de la coupe est à 40 mm sous la ligne de pli (y = −40), à 10 mm du côté.
    assert!(got.iter().filter(|g| near(g.1, -40.0, 1e-6)).count() == 2);
}

#[test]
fn fiche_de_piece_et_droit_fil() {
    let j = jupe();
    let piece = j.project.piece(j.piece).unwrap();
    assert_eq!(piece.name, "Jupe devant");
    assert_eq!(piece.code.as_deref(), Some("JD-01"));
    assert_eq!(piece.quantity, 1);
    assert_eq!(piece.material.as_deref(), Some("Tissu"));
    assert_eq!(piece.category, Some(pf_core::PieceCategory::Fabric));
    assert!(piece.comment.as_deref().unwrap().contains("pli"));
    let lines = j.project.piece_grain_lines(j.piece);
    assert_eq!(lines.len(), 1);
    let g = &lines[0];
    assert!(near(
        (g.to.x - g.from.x).hypot(g.to.y - g.from.y),
        300.0,
        1e-9
    ));
    assert!(near(g.from.x, 120.0, 1e-9) && near(g.to.x, 120.0, 1e-9));
    // Aucune pièce sans avertissement de coupe ni de pince.
    for issue in j.project.piece_issues(j.piece) {
        let text = format!("{issue:?}");
        assert!(
            !text.contains("CutContour") && !text.contains("Dart") && !text.contains("Fold"),
            "{text}"
        );
    }
}

// ---------------------------------------------------------------------------
// Exports
// ---------------------------------------------------------------------------

fn svg_of(j: &Jupe) -> String {
    export_svg(&Scene::from_sheet(&j.project, j.sheet).unwrap()).unwrap()
}

#[test]
fn svg_mesures_crans_droit_fil_et_coupe() {
    let j = jupe();
    let text = svg_of(&j);
    let doc = read_svg(&text);
    // Couture : un seul tracé fermé de 5 sommets, bords lus dans le fichier.
    let sewing = doc.with_role("couture");
    assert_eq!(sewing.len(), 5, "un tracé par bord : {}", sewing.len());
    let lens: Vec<f64> = sewing
        .iter()
        .map(|i| (i.pts[1].0 - i.pts[0].0).hypot(i.pts[1].1 - i.pts[0].1))
        .collect();
    assert!(same_lengths(&lens, &SEAM_EDGES, 1e-3), "{lens:?}");
    // Coupe calculée : un contour de 5 sommets, périmètre et aire attendus.
    let cut = doc.with_role("coupe-calculee");
    assert_eq!(cut.len(), 1);
    // Le tracé du fichier est refermé sur son premier sommet : 6 points pour 5 sommets.
    let mut cut_pts = cut[0].pts.clone();
    assert_eq!(cut_pts.len(), 6, "{cut_pts:?}");
    assert_eq!(cut_pts.pop(), Some(cut_pts[0]));
    assert!(same_lengths(
        &cyc_lengths(&cut_pts),
        &cut_edge_lengths(),
        1e-3
    ));
    assert!(near(shoelace(&cut_pts), cut_area(), 0.5));
    // Crans : quatre traits de 5 mm.
    let (_, after) = text.split_once("data-role=\"cran\"").expect("crans");
    let group = after.split("</g>").next().unwrap();
    let notch_lens: Vec<f64> = group
        .lines()
        .filter(|l| l.trim().starts_with("<line"))
        .map(|l| {
            let n = |k: &str| attr(l, k).unwrap().parse::<f64>().unwrap();
            (n("x2") - n("x1")).hypot(n("y2") - n("y1"))
        })
        .collect();
    assert_eq!(notch_lens.len(), 4, "{group}");
    assert!(notch_lens.iter().all(|l| near(*l, 5.0, 1e-3)));
    // Droit-fil : un trait de 300 mm (et ses pointes de flèche).
    let grain = doc.with_role("droit-fil");
    assert_eq!(grain.len(), 5);
    assert!(grain.iter().any(|g| near(
        (g.pts[1].0 - g.pts[0].0).hypot(g.pts[1].1 - g.pts[0].1),
        300.0,
        1e-3
    )));
    // Pointage de la pince.
    assert!(text.contains("data-role=\"marquage\""));
}

fn dxf_polys(d: &rd::Dxf, layer: &str) -> Vec<Vec<(f64, f64)>> {
    d.blocks
        .iter()
        .flat_map(|(_, e)| e)
        .filter_map(|e| match e {
            Ent::Polyline {
                layer: l,
                pts,
                closed: true,
            } if l == layer => Some(pts.clone()),
            _ => None,
        })
        .collect()
}

fn dxf_lines(d: &rd::Dxf, layer: &str) -> Vec<((f64, f64), (f64, f64))> {
    d.blocks
        .iter()
        .flat_map(|(_, e)| e)
        .filter_map(|e| match e {
            Ent::Line { layer: l, a, b } if l == layer => Some((*a, *b)),
            _ => None,
        })
        .collect()
}

#[test]
fn dxf_couches_mesures_crans_et_etiquette() {
    let j = jupe();
    let out = export_dxf(&j.project).unwrap();
    assert_eq!(out.report.omissions().count(), 0, "{:?}", out.report.issues);
    let d = rd::parse(&out.text).unwrap();
    // Couche 15 = couture ; couche 1 = coupe calculée.
    let sewing = dxf_polys(&d, "15");
    let cut = dxf_polys(&d, "1");
    assert_eq!((sewing.len(), cut.len()), (1, 1));
    assert_eq!(sewing[0].len(), 5);
    assert!(same_lengths(&cyc_lengths(&sewing[0]), &SEAM_EDGES, 1e-3));
    assert!(near(rd::perimeter(&sewing[0], true), SEAM_PERIMETER, 1e-3));
    assert!(near(rd::area(&sewing[0]).abs(), SEAM_AREA, 1e-2));
    assert!(same_lengths(
        &cyc_lengths(&cut[0]),
        &cut_edge_lengths(),
        1e-3
    ));
    assert!(near(rd::perimeter(&cut[0], true), cut_perimeter(), 1e-3));
    assert!(near(rd::area(&cut[0]).abs(), cut_area(), 1e-2));
    // Crans (couche 4) : quatre traits de 5 mm dont le pied est sur la couture ; à 75 et 95 mm
    // de l'extrémité de la taille (sommet entre les bords de 250 et 170 mm), à 360 mm du bas du
    // côté (sommet entre 360 et 250) : distances mesurées depuis les sommets.
    let notches = dxf_lines(&d, "4");
    assert_eq!(notches.len(), 4);
    for (a, b) in &notches {
        assert!(near(rd::dist(*a, *b), 5.0, 1e-3));
        assert!(rd::dist_to_polyline(*a, &sewing[0], true) < 1e-3);
    }
    let feet: Vec<(f64, f64)> = notches.iter().map(|n| n.0).collect();
    let pts = &sewing[0];
    let n = pts.len();
    let lens = cyc_lengths(pts);
    // Sommet B : arête sortante de 170 (taille), arête entrante de 250 (côté haut).
    let b = (0..n)
        .find(|&i| near(lens[i], 170.0, 1e-3) && near(lens[(i + n - 1) % n], 250.0, 1e-3))
        .expect("sommet B");
    let mut waist: Vec<f64> = feet
        .iter()
        .map(|f| rd::dist(*f, pts[b]))
        .filter(|d| *d < 170.0 && *d > 1.0 && !near(*d, 0.0, 1e-3))
        .collect();
    waist.retain(|d| near(*d, 75.0, 1e-3) || near(*d, 95.0, 1e-3));
    waist.sort_by(f64::total_cmp);
    assert_eq!(waist.len(), 2, "{feet:?}");
    assert!(near(waist[0], 75.0, 1e-3) && near(waist[1], 95.0, 1e-3));
    // Sommet H : le pied du cran de hanche y est (360 mm depuis C, 250 jusqu'à B).
    let h = (0..n)
        .find(|&i| near(lens[i], 250.0, 1e-3) && near(lens[(i + n - 1) % n], 360.0, 1e-3))
        .expect("sommet H");
    assert!(feet.iter().any(|f| rd::dist(*f, pts[h]) < 1e-3));
    // Droit-fil (couche 6) : un trait de 300 mm au moins.
    assert!(dxf_lines(&d, "6")
        .iter()
        .any(|(a, b)| near(rd::dist(*a, *b), 300.0, 1e-3)));
    // Étiquette de pièce (couche 84) : nom, code, quantité, matière, catégorie, commentaire.
    let texts: Vec<&str> = d
        .blocks
        .iter()
        .flat_map(|(_, e)| e)
        .filter_map(|e| match e {
            Ent::Text { layer, text, .. } if layer == "84" => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        texts,
        [
            "Piece Name: Jupe devant",
            "Code: JD-01",
            "Quantity: 1",
            "Material: Tissu",
            "Category: fabric",
            "Comment: Couper au pli, milieu devant sur la pliure"
        ]
    );
    // Pointage de la pince (couche 13).
    let punches: Vec<&(f64, f64)> = d
        .blocks
        .iter()
        .flat_map(|(_, e)| e)
        .filter_map(|e| match e {
            Ent::Point { layer, p } if layer == "13" => Some(p),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(punches.len(), 1);
}

#[test]
fn hpgl_traces_couture_et_coupe() {
    let j = jupe();
    let scene = Scene::from_sheet(&j.project, j.sheet).unwrap();
    let plot = export_hpgl(&scene, &construction::hpgl_page()).unwrap();
    let replay = read_hpgl(&plot.text).unwrap();
    // Arrondi à 1/40 mm par sommet : 0,2 mm de tolérance sur le périmètre.
    // Coupe : un seul tracé fermé (plume 1) ; couture : un tracé par bord.
    assert!(
        replay
            .strokes
            .iter()
            .any(|s| near(s.length_mm(), cut_perimeter(), 0.2)),
        "coupe"
    );
    let seam: Vec<f64> = replay
        .strokes
        .iter()
        .map(|s| s.length_mm())
        .filter(|l| SEAM_EDGES.iter().any(|e| near(*l, *e, 0.1)))
        .collect();
    assert!(same_lengths(&seam, &SEAM_EDGES, 0.1), "{seam:?}");
}

#[test]
fn pdf_a4_un_pour_un_et_plusieurs_pages() {
    let j = jupe();
    let scene = Scene::from_sheet(&j.project, j.sheet).unwrap();
    let out = export_pdf(&[scene]).unwrap();
    // La jupe (650 × 250 mm) ne tient pas sur une page : étalon + tuiles, toutes en A4 portrait.
    assert_eq!(
        out.page_count, 7,
        "étalon + 2 colonnes × 3 lignes de tuiles"
    );
    for (w, h) in &out.page_sizes_mm {
        assert!(near(*w, 210.0, 0.1) && near(*h, 297.0, 0.1), "{w} × {h}");
    }
    let pages = common::read_pdf(&out.bytes);
    assert_eq!(pages.len(), out.page_count);
    // Aucune ligne n'est plus longue que la plus grande diagonale de page : pas de mise à
    // l'échelle ; le total des traits est cohérent avec les 1:1 (périmètre de couture au moins).
    let total: f64 = pages.iter().flat_map(|p| &p.lines).map(|l| l.len()).sum();
    assert!(total > SEAM_PERIMETER, "{total}");
}

// ---------------------------------------------------------------------------
// Pince et étiquette exportées (tranche 69)
// ---------------------------------------------------------------------------

/// Jambe attendue : √(10² + 100²), écrite à la main (10 = demi-ouverture, 100 = longueur).
const LEG: f64 = 100.498_756_211_208_9;

/// Lignes de l'étiquette (haut → bas) : nom, code, quantité, matière, catégorie, commentaire.
const ETIQUETTE: [&str; 6] = [
    "Jupe devant",
    "Code : JD-01",
    "Quantité : 1",
    "Matière : Tissu",
    "Catégorie : Tissu",
    "Couper au pli, milieu devant sur la pliure",
];

/// Version ASCII des mêmes lignes (HPGL : accents retirés).
const ETIQUETTE_ASCII: [&str; 6] = [
    "Jupe devant",
    "Code : JD-01",
    "Quantite : 1",
    "Matiere : Tissu",
    "Categorie : Tissu",
    "Couper au pli, milieu devant sur la pliure",
];

/// Centre du bloc : milieu du droit-fil (120 ; 250), décalé à gauche de la demi-largeur du bloc
/// (42 caractères × 4 mm × 0,55 = 92,4 mm de large) plus 3 mm : x = 120 − 46,2 − 3 = 70,8.
const CENTRE_X: f64 = 70.8;
const CENTRE_Y: f64 = 250.0;

/// Ordonnée (modèle) de la ligne de base de la ligne `i` : pas de 5 mm, six lignes, corps 4 mm
/// (baisse de 0,35 × 4 = 1,4 mm).
fn baseline_y(i: usize) -> f64 {
    CENTRE_Y + (2.5 - i as f64) * 5.0 - 1.4
}

/// Largeur d'une ligne (mm) : 0,55 × 4 mm par caractère.
fn line_w(line: &str) -> f64 {
    line.chars().count() as f64 * 2.2
}

/// Lignes `<text data-role="etiquette-piece" …>` du SVG : (x, y, texte), dans l'ordre du fichier.
fn svg_labels(svg: &str) -> Vec<(f64, f64, String)> {
    svg.lines()
        .map(str::trim)
        .filter(|l| l.starts_with("<text") && l.contains("data-role=\"etiquette-piece\""))
        .map(|l| {
            let n = |k: &str| attr(l, k).unwrap().parse::<f64>().unwrap();
            let inner = l.split_once('>').unwrap().1;
            (
                n("x"),
                n("y"),
                inner.rsplit_once("</text>").unwrap().0.to_string(),
            )
        })
        .collect()
}

#[test]
fn svg_pince_jambes_et_etiquette() {
    let j = jupe();
    let text = svg_of(&j);
    let doc = read_svg(&text);
    // Droit-fil de 300 mm : repère (x = 120, y de 100 à 400). En SVG, Y vers le bas.
    let grain = doc
        .with_role("droit-fil")
        .into_iter()
        .find(|g| near((g.pts[1].1 - g.pts[0].1).abs(), 300.0, 1e-3))
        .expect("droit-fil de 300 mm");
    let gx = grain.pts[0].0;
    let gtop = grain.pts[0].1.min(grain.pts[1].1);
    // Deux jambes, rôle « pince », de √10 100 mm chacune, sommet en commun.
    let legs = doc.with_role("pince");
    assert_eq!(legs.len(), 2);
    for l in &legs {
        let d = (l.pts[1].0 - l.pts[0].0).hypot(l.pts[1].1 - l.pts[0].1);
        assert!(near(d, LEG, 1e-3), "{d}");
        // Motif propre : jamais la couleur seule.
        assert_eq!(l.dash.as_deref(), Some("8 1.5 1.5 1.5 1.5 1.5"));
        assert_eq!(l.width.as_deref(), Some("0.4"));
    }
    let apex = (gx - 35.0, gtop - 100.0);
    for (l, foot_dx) in legs.iter().zip([-45.0, -25.0]) {
        assert!(near(l.pts[0].0, apex.0, 1e-3) && near(l.pts[0].1, apex.1, 1e-3));
        assert!(near(l.pts[1].0, gx + foot_dx, 1e-3), "{:?}", l.pts);
        assert!(near(l.pts[1].1, gtop - 200.0, 1e-3), "{:?}", l.pts);
    }
    // Le motif de la pince n'est celui d'aucun autre rôle du fichier.
    for it in &doc.items {
        if it.role.as_deref() != Some("pince") {
            assert_ne!(it.dash.as_deref(), Some("8 1.5 1.5 1.5 1.5 1.5"));
        }
    }
    // Étiquette : six lignes, accents fidèles, centre calculé à la main.
    let labels = svg_labels(&text);
    assert_eq!(labels.len(), 6, "{labels:?}");
    for (i, (x, y, t)) in labels.iter().enumerate() {
        assert_eq!(t, ETIQUETTE[i]);
        assert!(near(*x, gx + (CENTRE_X - 120.0), 1e-3), "{x}");
        assert!(near(*y, gtop + (400.0 - baseline_y(i)), 1e-3), "{y}");
    }
    // La ligne pliée / le capuchon n'est pas exporté : aucun autre élément de rôle pince.
    assert_eq!(text.matches("data-role=\"pince\"").count(), 2);
}

#[test]
fn dxf_pince_calque_90_et_etiquette_complete() {
    let j = jupe();
    let out = export_dxf(&j.project).unwrap();
    assert_eq!(out.report.omissions().count(), 0, "{:?}", out.report.issues);
    let d = rd::parse(&out.text).unwrap();
    assert!(
        d.layers.iter().any(|(n, _)| n == "90"),
        "couche 90 déclarée"
    );
    let legs = dxf_lines(&d, "90");
    assert_eq!(legs.len(), 2);
    // Droit-fil (couche 6) de 300 mm : repère, Y vers le haut.
    let grain = dxf_lines(&d, "6")
        .into_iter()
        .find(|(a, b)| near(rd::dist(*a, *b), 300.0, 1e-3))
        .unwrap();
    let (gx, gtop) = (grain.0 .0, grain.0 .1.max(grain.1 .1));
    for ((a, b), foot_dx) in legs.iter().zip([-45.0, -25.0]) {
        assert!(near(rd::dist(*a, *b), LEG, 1e-3));
        assert!(
            near(a.0, gx - 35.0, 1e-3) && near(a.1, gtop + 100.0, 1e-3),
            "{a:?}"
        );
        assert!(
            near(b.0, gx + foot_dx, 1e-3) && near(b.1, gtop + 200.0, 1e-3),
            "{b:?}"
        );
    }
    // Aucune jambe sur la couche de coupe (1) : couche distincte.
    assert!(dxf_lines(&d, "1").is_empty());
}

#[test]
fn hpgl_pince_plume_9_et_etiquette_lb() {
    let j = jupe();
    let scene = Scene::from_sheet(&j.project, j.sheet).unwrap();
    let plot = export_hpgl(&scene, &construction::hpgl_page()).unwrap();
    assert!(plot.text.is_ascii());
    let r = read_hpgl(&plot.text).unwrap();
    // Plume 9 : deux jambes de √10 100 mm (arrondi au 1/40 mm : 0,05 mm de marge).
    let legs: Vec<_> = r.strokes.iter().filter(|s| s.pen == 9).collect();
    assert_eq!(legs.len(), 2);
    assert!(legs.iter().all(|l| near(l.length_mm(), LEG, 0.05)));
    // Positions : relatives à l'extrémité haute du droit-fil (plume 4, 300 mm), en unités (40 / mm).
    let grain = r
        .strokes
        .iter()
        .find(|s| s.pen == 4 && near(s.length_mm(), 300.0, 0.1))
        .unwrap();
    let (gx, gtop) = (grain.pts[0].0, grain.pts.iter().map(|p| p.1).max().unwrap());
    let near_u = |a: i64, b: f64| (a as f64 - b).abs() <= 1.0;
    // L'ordre optimisé peut retourner un tracé : le sommet est l'extrémité commune.
    let ends: Vec<[(i64, i64); 2]> = legs.iter().map(|l| [l.pts[0], l.pts[1]]).collect();
    let apex = ends[0]
        .into_iter()
        .find(|p| ends[1].contains(p))
        .expect("sommet commun");
    assert!(
        near_u(apex.0 - gx, -35.0 * 40.0) && near_u(apex.1 - gtop, 100.0 * 40.0),
        "{apex:?}"
    );
    let mut feet: Vec<(i64, i64)> = ends
        .iter()
        .map(|e| if e[0] == apex { e[1] } else { e[0] })
        .collect();
    feet.sort();
    for (b, foot_dx) in feet.iter().zip([-45.0, -25.0]) {
        assert!(
            near_u(b.0 - gx, foot_dx * 40.0) && near_u(b.1 - gtop, 200.0 * 40.0),
            "{b:?}"
        );
    }
    // Étiquette : six lignes `LB` à la plume 7, ASCII, caractère de 4 mm (0,4 cm de haut).
    assert_eq!(r.labels.len(), 6, "{:?}", r.labels);
    for (i, l) in r.labels.iter().enumerate() {
        assert_eq!(l.text, ETIQUETTE_ASCII[i]);
        assert_eq!(l.pen, 7);
        assert!(near(l.size_cm.1, 0.4, 1e-9) && near(l.size_cm.0, 0.22, 1e-9));
        let start_x = CENTRE_X - line_w(ETIQUETTE[i]) / 2.0;
        assert!(near_u(l.at.0 - gx, (start_x - 120.0) * 40.0), "{:?}", l.at);
        assert!(
            near_u(l.at.1 - gtop, (baseline_y(i) - 400.0) * 40.0),
            "{:?}",
            l.at
        );
    }
    assert_eq!(plot.report.labels, 6);
}

#[test]
fn pdf_pince_trait_distinct_et_etiquette_en_police_embarquee() {
    let j = jupe();
    let scene = Scene::from_sheet(&j.project, j.sheet).unwrap();
    let out = export_pdf(&[scene]).unwrap();
    // Jambes : première page de patron (le sommet et les pieds y sont), pointillé propre.
    let strokes = read_strokes(&out.bytes);
    let dash: Vec<f64> = [8.0, 1.5, 1.5, 1.5, 1.5, 1.5]
        .iter()
        .map(|v| mm_pt(*v))
        .collect();
    let is_dart = |s: &common::coupe::PdfStroke| {
        s.dash_pt.len() == dash.len()
            && s.dash_pt.iter().zip(&dash).all(|(a, b)| near(*a, *b, 1e-3))
    };
    let legs: Vec<_> = strokes.iter().filter(|s| is_dart(s)).collect();
    assert_eq!(legs.len(), 2, "deux jambes au pointillé propre");
    assert!(legs.iter().all(|l| near(l.width_pt, mm_pt(0.4), 1e-3)));
    let mm = |v: f64| v * 25.4 / 72.0;
    let grain = strokes
        .iter()
        .find(|s| s.pts.len() == 2 && near(mm(common::coupe::stroke_len_pt(s)), 300.0, 1e-2))
        .expect("droit-fil de 300 mm");
    let (gx, gtop) = (grain.pts[0].0, grain.pts[0].1.max(grain.pts[1].1));
    for (l, foot_dx) in legs.iter().zip([-45.0, -25.0]) {
        assert!(near(mm(common::coupe::stroke_len_pt(l)), LEG, 1e-2));
        assert!(near(mm(l.pts[0].0 - gx), -35.0, 1e-2));
        assert!(near(mm(l.pts[0].1 - gtop), 100.0, 1e-2));
        assert!(near(mm(l.pts[1].0 - gx), foot_dx, 1e-2));
        assert!(near(mm(l.pts[1].1 - gtop), 200.0, 1e-2));
    }
    // Étiquette : les six lignes se suivent, dans l'ordre, dans le flux d'une page (police
    // embarquée : « é » relu par le `ToUnicode` du fichier).
    let pages = common::read_pdf(&out.bytes);
    assert!(
        pages.iter().any(|p| p
            .strings
            .windows(6)
            .any(|w| w.iter().map(String::as_str).eq(ETIQUETTE))),
        "bloc d'étiquette absent du PDF"
    );
}

// ---------------------------------------------------------------------------
// Rejouabilité et exemples
// ---------------------------------------------------------------------------

fn exports(p: &pf_core::Project, sheet: pf_core::SheetId) -> (String, Vec<u8>, String, String) {
    let scene = Scene::from_sheet(p, sheet).unwrap();
    (
        export_svg(&scene).unwrap(),
        export_pdf(std::slice::from_ref(&scene)).unwrap().bytes,
        export_dxf(p).unwrap().text,
        export_hpgl(&scene, &construction::hpgl_page())
            .unwrap()
            .text,
    )
}

#[test]
fn scenario_deterministe() {
    let (a, b) = (jupe(), jupe());
    assert_eq!(a.project, b.project);
    assert_eq!(
        persist::to_json(&a.project).unwrap(),
        persist::to_json(&b.project).unwrap()
    );
    assert_eq!(exports(&a.project, a.sheet), exports(&b.project, b.sheet));
}

#[test]
fn projet_charge_depuis_l_exemple_exporte_comme_le_projet_construit() {
    let j = jupe();
    let loaded = persist::load(&examples().join("jupe-droite.pforge")).expect(HINT);
    assert_eq!(loaded, j.project, "{HINT}");
    assert_eq!(
        exports(&loaded, loaded.sheets[0].id),
        exports(&j.project, j.sheet)
    );
}

#[test]
fn fichiers_d_exemple_a_jour() {
    let j = jupe();
    let scene = Scene::from_sheet(&j.project, j.sheet).unwrap();
    let read =
        |name: &str| fs::read(examples().join(name)).unwrap_or_else(|e| panic!("{name} : {e}"));
    assert_eq!(
        read("jupe-droite.pforge"),
        persist::to_json(&j.project).unwrap().into_bytes(),
        "{HINT}"
    );
    assert_eq!(
        read("jupe-droite.svg"),
        export_svg(&scene).unwrap().into_bytes(),
        "{HINT}"
    );
    assert_eq!(
        read("jupe-droite.pdf"),
        export_pdf(std::slice::from_ref(&scene)).unwrap().bytes,
        "{HINT}"
    );
    assert_eq!(
        read("jupe-droite.dxf"),
        export_dxf(&j.project).unwrap().text.into_bytes(),
        "{HINT}"
    );
    assert_eq!(
        read("jupe-droite.hpgl"),
        export_hpgl(&scene, &construction::hpgl_page())
            .unwrap()
            .text
            .into_bytes(),
        "{HINT}"
    );
}
