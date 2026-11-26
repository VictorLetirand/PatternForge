//! PH-3b : scénario rejouable « porte-cartes en cuir », de la feuille vide au PDF, au SVG, au DXF
//! et au HPGL, construit **uniquement** par `Project::apply(Command)` (voir `construction.rs`).
//!
//! Dimensions de contrôle, **calculées à la main** (mm) :
//! - fond 100 × 70, coins arrondis r = 5 : périmètre = 2(a + b) − 8r + 2πr = 340 − 40 + 10π =
//!   331,4159 ; aire = ab − (4 − π)r² = 7 000 − 25(4 − π) = 6 978,5398 ; coupe nette : contour de
//!   coupe = contour de couture ;
//! - patte : bords de 30 + 80 + 30 et demi-cercle de rayon 40 : périmètre = 140 + 40π = 265,6637 ;
//!   aire = 80 × 30 + π × 40² / 2 = 4 913,2741 ;
//! - poche 90 × 50, bord haut en arc de flèche 5 : rayon R = (45² + 5²) / (2 × 5) = 205, centre
//!   (165 ; −150), demi-angle θ = asin(45 / 205) ; couture : 190 + 2Rθ = 280,7389, aire =
//!   4 500 + R² (2θ − sin 2θ) / 2 ;
//! - coupe de la poche : marge de cuir 6 (côtés et bas), rempli 8 sur l'arc. Le sommet entre un
//!   côté et l'arc est un onglet avec la **tangente** à l'arc : le décalage du côté (x = 216) coupe la
//!   tangente décalée de 8 en y = 50 + (8 × 205 − 6 × 45) / 200 = 56,85, puis un pas de 4,35 mm
//!   (= 891,75 / 205) le long de la tangente rejoint l'arc décalé (rayon 213, même centre, même
//!   demi-angle θ) en (210 + 8 × 45 / 205 ; 50 + 8 × 200 / 205).
//!
//! Honnêteté : rien ici ne prouve l'ouverture des fichiers dans un logiciel tiers, ni une
//! impression ou un traçage réels (étapes humaines).

mod common;
#[path = "scenario_porte_cartes/construction.rs"]
mod construction;

use common::coupe::{attr, mm_pt, read_pdf as read_strokes, read_svg, PdfStroke, SvgItem};
use common::dxf::{self as rd, Ent};
use common::hpgl::read_hpgl;
use construction::{build, PorteCartes};
use pf_core::persist;
use pf_export::dxf::export_dxf;
use pf_export::hpgl::export_hpgl;
use pf_export::pdf::export_pdf;
use pf_export::svg::export_svg;
use pf_export::Scene;
use std::f64::consts::PI;
use std::fs;
use std::path::PathBuf;

const HINT: &str = "régénérer avec : cargo run -p pf-export --example generer_exemples";

/// Deux points (un segment, ou un point et sa normale).
type Seg = ((f64, f64), (f64, f64));

fn near(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

fn examples() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples")
}

fn pc() -> PorteCartes {
    build().expect("le scénario doit s'exécuter sans refus")
}

// --- Valeurs attendues, écrites à la main -------------------------------------------------------

fn body_perimeter() -> f64 {
    2.0 * (100.0 + 70.0) - 8.0 * 5.0 + 2.0 * PI * 5.0
}

fn body_area() -> f64 {
    100.0 * 70.0 - (4.0 - PI) * 5.0 * 5.0
}

fn flap_perimeter() -> f64 {
    30.0 + 80.0 + 30.0 + PI * 40.0
}

fn flap_area() -> f64 {
    80.0 * 30.0 + PI * 40.0 * 40.0 / 2.0
}

/// Demi-angle de l'arc de la poche vu du centre (165 ; −150), rayon 205, demi-corde 45.
fn theta() -> f64 {
    (45.0f64 / 205.0).asin()
}

fn pocket_perimeter() -> f64 {
    90.0 + 50.0 + 50.0 + 2.0 * 205.0 * theta()
}

fn pocket_area() -> f64 {
    90.0 * 50.0 + 205.0 * 205.0 / 2.0 * (2.0 * theta() - (2.0 * theta()).sin())
}

/// Sommets droits de la coupe de la poche (modèle), sens anti-horaire depuis (114 ; −6) ; l'arc de
/// rayon 213 relie les deux derniers sommets du haut.
fn pocket_cut_vertices() -> [(f64, f64); 6] {
    let (nx, ny) = (45.0 / 205.0, 200.0 / 205.0);
    [
        (114.0, -6.0),
        (216.0, -6.0),
        (216.0, 56.85),
        (210.0 + 8.0 * nx, 50.0 + 8.0 * ny),
        (120.0 - 8.0 * nx, 50.0 + 8.0 * ny),
        (114.0, 56.85),
    ]
}

/// Longueurs des bords de la coupe de la poche (cycle) : côté, bas, côté, pas, corde de l'arc, pas.
fn pocket_cut_chords() -> [f64; 6] {
    [62.85, 102.0, 62.85, 4.35, 2.0 * 213.0 * theta().sin(), 4.35]
}

fn pocket_cut_perimeter() -> f64 {
    102.0 + 2.0 * 62.85 + 2.0 * 4.35 + 2.0 * 213.0 * theta()
}

fn pocket_cut_area() -> f64 {
    shoelace(&pocket_cut_vertices()) + 213.0 * 213.0 / 2.0 * (2.0 * theta() - (2.0 * theta()).sin())
}

fn shoelace(p: &[(f64, f64)]) -> f64 {
    let n = p.len();
    (0..n)
        .map(|i| p[i].0 * p[(i + 1) % n].1 - p[(i + 1) % n].0 * p[i].1)
        .sum::<f64>()
        .abs()
        / 2.0
}

/// Crans de rempli (couture, normale unitaire) : à 1/4, 1/2 et 3/4 de l'arc (⌈90,74 / 25⌉ = 4
/// intervalles égaux), soit aux angles θ/2, 0 et −θ/2 de la verticale du centre.
fn fold_notches() -> [Seg; 3] {
    let f = |phi: f64| {
        let n = (phi.sin(), phi.cos());
        ((165.0 + 205.0 * n.0, -150.0 + 205.0 * n.1), n)
    };
    [f(theta() / 2.0), f(0.0), f(-theta() / 2.0)]
}

/// Segment exporté d'un cran de rempli (modèle) : de la coupe (couture + 8 n) vers la couture, sur
/// min(5 ; 8) = 5 mm, soit jusqu'à couture + 3 n.
fn fold_notch_segments() -> Vec<Seg> {
    fold_notches()
        .iter()
        .map(|((x, y), n)| {
            (
                (x + 8.0 * n.0, y + 8.0 * n.1),
                (x + 3.0 * n.0, y + 3.0 * n.1),
            )
        })
        .collect()
}

/// Crans de couture de 5 mm vers l'extérieur : fond et poche à 20 et 70 mm du début du bord bas
/// (x = 5 + 20, 5 + 70 pour le fond ; 120 + 20, 120 + 70 pour la poche), patte au milieu du haut.
fn flat_notch_segments() -> Vec<Seg> {
    vec![
        ((25.0, 0.0), (25.0, -5.0)),
        ((75.0, 0.0), (75.0, -5.0)),
        ((140.0, 0.0), (140.0, -5.0)),
        ((190.0, 0.0), (190.0, -5.0)),
        ((40.0, -20.0), (40.0, -15.0)),
    ]
}

fn hole_x(k: usize) -> f64 {
    6.0 + 4.0 * k as f64
}

/// Lignes d'étiquette de chaque pièce (haut → bas) et centre du bloc (milieu de l'emprise).
/// Le fond et la patte sont en coupe nette (PH-5c) : « Coupe nette » en dernière ligne.
fn label_lines(name: &str, code: &str) -> Vec<String> {
    let mut lines = vec![
        name.to_string(),
        format!("Code : {code}"),
        "Quantité : 1".to_string(),
        "Matière : Cuir".to_string(),
        "Catégorie : Cuir".to_string(),
    ];
    if name != "Poche" {
        lines.push("Coupe nette".to_string());
    }
    lines
}

/// Rang de la première ligne d'étiquette de chaque pièce dans la suite des étiquettes
/// (Fond 6 lignes, Poche 5, Patte 6).
const LABEL_FIRST: [usize; 3] = [0, 6, 11];

const LABELS: [(&str, &str, (f64, f64)); 3] = [
    ("Fond", "PC-01", (50.0, 35.0)),
    ("Poche", "PC-02", (165.0, 27.5)),
    ("Patte", "PC-03", (40.0, -55.0)),
];

/// Ordonnée de la ligne de base de la ligne `i` sur `n` : pas 5 mm, corps 4 mm (baisse 1,4 mm).
fn baseline_y(cy: f64, i: usize, n: usize) -> f64 {
    cy + ((n as f64 - 1.0) / 2.0 - i as f64) * 5.0 - 1.4
}

fn ascii(s: &str) -> String {
    s.replace(['é', 'è'], "e")
}

fn sorted(mut v: Vec<f64>) -> Vec<f64> {
    v.sort_by(f64::total_cmp);
    v
}

/// Mêmes longueurs, à l'ordre près.
fn same_lengths(got: &[f64], want: &[f64], tol: f64) -> bool {
    let (g, w) = (sorted(got.to_vec()), sorted(want.to_vec()));
    g.len() == w.len() && g.iter().zip(&w).all(|(a, b)| near(*a, *b, tol))
}

/// Segments `found` et `expected` égaux à l'ordre et au sens près.
fn same_segments(found: &[Seg], expected: &[Seg], tol: f64) -> bool {
    let eq = |a: (f64, f64), b: (f64, f64)| near(a.0, b.0, tol) && near(a.1, b.1, tol);
    found.len() == expected.len()
        && expected.iter().all(|e| {
            found
                .iter()
                .any(|f| (eq(f.0, e.0) && eq(f.1, e.1)) || (eq(f.0, e.1) && eq(f.1, e.0)))
        })
}

// ---------------------------------------------------------------------------
// Mesures du modèle
// ---------------------------------------------------------------------------

#[test]
fn fond_coins_arrondis_coupe_nette_et_percages() {
    let c = pc();
    let p = &c.project;
    let contour = p.piece_contour(c.body).unwrap();
    assert!(contour.closed && !contour.self_intersecting && !contour.clockwise);
    assert_eq!(contour.edges.len(), 8, "4 bords droits et 4 congés");
    assert!(near(contour.perimeter_mm, body_perimeter(), 1e-9));
    assert!(near(contour.area_mm2, body_area(), 1e-6));
    // Valeurs numériques de contrôle (arrondies) : 331,4159 mm ; 6 978,5398 mm².
    assert!(near(body_perimeter(), 331.415_927, 1e-6));
    assert!(near(body_area(), 6_978.539_816, 1e-6));
    // Bords : 90, 60, 90, 60 et quatre quarts de cercle de rayon 5.
    let sheet = p.sheet(c.sheet).unwrap();
    let lens: Vec<f64> = contour
        .edges
        .iter()
        .map(|e| sheet.path_length_mm(e.entity).unwrap())
        .collect();
    let mut want = vec![90.0, 60.0, 90.0, 60.0];
    want.extend([5.0 * PI / 2.0; 4]);
    assert!(same_lengths(&lens, &want, 1e-9), "{lens:?}");
    // Coupe nette déclarée, sans marge : le contour de coupe est celui de couture.
    let piece = p.piece(c.body).unwrap();
    assert!(piece.net_cut && piece.seam.is_none());
    let cut = p.piece_cut_contour(c.body).unwrap();
    assert!(near(cut.perimeter_mm, body_perimeter(), 1e-9));
    assert!(near(cut.area_mm2, body_area(), 1e-6));
    // Perçages : 23 trous de 1,2 mm à 3 mm du bord bas, pas de 4 mm, de x = 6 à x = 94.
    let marks = p.piece_markings(c.body);
    assert_eq!(marks.len(), 23);
    assert_eq!(c.holes.len(), 23);
    for (k, m) in marks.iter().enumerate() {
        assert_eq!(
            m.marking.shape,
            pf_core::MarkingShape::Drill { diameter_mm: 1.2 }
        );
        assert!(
            near(m.at.x, hole_x(k), 1e-9) && near(m.at.y, 3.0, 1e-9),
            "{m:?}"
        );
        assert_eq!(m.inside_outline, Some(true));
    }
    assert!(near(hole_x(22), 94.0, 1e-9));
    // Crans de repère : à 20 et 70 mm du début du bord bas (qui commence en x = 5).
    assert_notches(p, c.sheet, c.body_bottom, &[20.0, 70.0], (5.0, 0.0));
}

fn assert_notches(
    p: &pf_core::Project,
    sheet: pf_core::SheetId,
    entity: pf_core::EntityId,
    distances: &[f64],
    start: (f64, f64),
) {
    let s = p.sheet(sheet).unwrap();
    let mine: Vec<_> = s.notches.iter().filter(|n| n.entity == entity).collect();
    assert_eq!(mine.len(), distances.len());
    for (n, d) in mine.iter().zip(distances) {
        assert!(near(s.notch_distance_mm(n).unwrap(), *d, 1e-9));
        let (foot, tip) = s.notch_marks(n).unwrap()[0];
        assert!(near(foot.x, start.0 + d, 1e-9) && near(foot.y, start.1, 1e-9));
        assert!(near(tip.x, start.0 + d, 1e-9) && near(tip.y, start.1 - 5.0, 1e-9));
    }
}

#[test]
fn patte_arc_et_coupe_nette() {
    let c = pc();
    let p = &c.project;
    let contour = p.piece_contour(c.flap).unwrap();
    assert!(contour.closed && !contour.self_intersecting && !contour.clockwise);
    assert!(near(contour.perimeter_mm, flap_perimeter(), 1e-9));
    assert!(near(contour.area_mm2, flap_area(), 1e-6));
    assert!(near(flap_perimeter(), 265.663_706, 1e-6));
    assert!(near(flap_area(), 4_913.274_123, 1e-6));
    let piece = p.piece(c.flap).unwrap();
    assert!(piece.net_cut && piece.seam.is_none());
    let cut = p.piece_cut_contour(c.flap).unwrap();
    assert!(near(cut.perimeter_mm, flap_perimeter(), 1e-9));
    assert!(near(cut.area_mm2, flap_area(), 1e-6));
    // Cran au milieu du bord haut (de 80 mm, parcouru de x = 80 vers x = 0) : pied (40 ; −20),
    // pointe (40 ; −15), côté extérieur = au-dessus.
    let s = p.sheet(c.sheet).unwrap();
    let n = s.notches.iter().find(|n| n.entity == c.flap_top).unwrap();
    assert!(near(s.notch_distance_mm(n).unwrap(), 40.0, 1e-9));
    let (foot, tip) = s.notch_marks(n).unwrap()[0];
    assert!(near(foot.x, 40.0, 1e-9) && near(foot.y, -20.0, 1e-9));
    assert!(near(tip.x, 40.0, 1e-9) && near(tip.y, -15.0, 1e-9));
}

#[test]
fn poche_marge_de_cuir_rempli_et_crans() {
    let c = pc();
    let p = &c.project;
    // Couture : 280,7389 mm ; 4 800,74 mm².
    let contour = p.piece_contour(c.pocket).unwrap();
    assert!(contour.closed && !contour.self_intersecting && !contour.clockwise);
    assert!(near(contour.perimeter_mm, pocket_perimeter(), 1e-9));
    assert!(near(contour.area_mm2, pocket_area(), 1e-6));
    assert!(near(pocket_perimeter(), 280.738_9, 1e-4));
    // Le jeu par défaut du cuir est de 6 mm (valeur d'atelier US-05.5) : marge posée = 6.
    assert!(near(
        p.seam_default_for(Some(pf_core::PieceCategory::Leather)),
        6.0,
        0.0
    ));
    assert!(near(p.category_seam_mm(c.pocket).unwrap(), 6.0, 0.0));
    let piece = p.piece(c.pocket).unwrap();
    assert!(!piece.net_cut);
    let seam = piece.seam.as_ref().unwrap();
    assert!(near(seam.default_mm, 6.0, 0.0));
    assert_eq!(seam.folds.len(), 1);
    assert_eq!(seam.folds[0].entity, c.pocket_arc);
    assert!(near(seam.folds[0].value_mm, 8.0, 0.0));
    // Coupe : rectangle agrandi de 6 mm, rempli de 8 mm sur l'arc.
    let cut = p.piece_cut_contour(c.pocket).unwrap();
    assert!(
        near(cut.perimeter_mm, pocket_cut_perimeter(), 1e-6),
        "{cut:?}"
    );
    assert!(near(cut.area_mm2, pocket_cut_area(), 1e-4), "{cut:?}");
    assert!(near(pocket_cut_perimeter(), 330.679_95, 1e-4));
    assert_eq!(
        cut.traces.len(),
        6,
        "quatre droites, deux pas et l'arc : 6 tracés"
    );
    let starts: Vec<(f64, f64)> = cut
        .traces
        .iter()
        .map(|t| match t {
            pf_geometry::Trace::Line(a, _) => (a.x, a.y),
            pf_geometry::Trace::Arc(a) => (
                a.center.x + a.radius_mm * a.start_rad.cos(),
                a.center.y + a.radius_mm * a.start_rad.sin(),
            ),
            other => panic!("tracé inattendu : {other:?}"),
        })
        .collect();
    for v in pocket_cut_vertices() {
        assert!(
            starts
                .iter()
                .any(|s| near(s.0, v.0, 1e-6) && near(s.1, v.1, 1e-6)),
            "sommet {v:?} absent de {starts:?}"
        );
    }
    // Crans de rempli : trois, aux quarts de l'arc (L = 90,7389 ; L / 4 = 22,6847).
    let notches = p.piece_fold_notches(c.pocket).unwrap();
    assert_eq!(notches.len(), 3);
    let quarter = 2.0 * 205.0 * theta() / 4.0;
    for (i, (n, ((sx, sy), (nx, ny)))) in notches.iter().zip(fold_notches()).enumerate() {
        assert_eq!(n.entity, c.pocket_arc);
        assert!(near(n.distance_mm, quarter * (i + 1) as f64, 1e-6), "{n:?}");
        assert!(
            near(n.point.x, sx, 1e-6) && near(n.point.y, sy, 1e-6),
            "{n:?}"
        );
        assert!(
            near(n.normal.x, nx, 1e-6) && near(n.normal.y, ny, 1e-6),
            "{n:?}"
        );
    }
    // Crans de repère du bord bas (de x = 120 à x = 210) : à 20 et 70 mm.
    assert_notches(p, c.sheet, c.pocket_bottom, &[20.0, 70.0], (120.0, 0.0));
}

#[test]
fn fiches_de_piece_et_avertissements() {
    let c = pc();
    let p = &c.project;
    assert_eq!(p.pieces.len(), 3);
    for (id, (name, code, _)) in [c.body, c.pocket, c.flap].into_iter().zip(LABELS) {
        let piece = p.piece(id).unwrap();
        assert_eq!(piece.name, name);
        assert_eq!(piece.code.as_deref(), Some(code));
        assert_eq!(piece.quantity, 1);
        assert_eq!(piece.material.as_deref(), Some("Cuir"));
        assert_eq!(piece.category, Some(pf_core::PieceCategory::Leather));
        // Seul avertissement : pas de droit-fil (le cuir n'en a pas, voir le suivi).
        let issues = p.piece_issues(id);
        assert_eq!(issues.len(), 1, "{issues:?}");
        assert!(format!("{:?}", issues[0]).contains("MissingGrainline"));
    }
}

// ---------------------------------------------------------------------------
// SVG
// ---------------------------------------------------------------------------

fn svg_of(c: &PorteCartes) -> String {
    export_svg(&Scene::from_sheet(&c.project, c.sheet).unwrap()).unwrap()
}

/// Longueur d'un tracé SVG : droite (corde) ou arc `A r r …` (2 r asin(corde / 2 r)).
fn svg_len(it: &SvgItem) -> f64 {
    let chord = (it.pts[1].0 - it.pts[0].0).hypot(it.pts[1].1 - it.pts[0].1);
    match it.d.as_deref().filter(|d| d.contains(" A ")) {
        None => chord,
        Some(d) => {
            let t: Vec<&str> = d.split_whitespace().collect();
            let r: f64 = t[t.iter().position(|x| *x == "A").unwrap() + 1]
                .parse()
                .unwrap();
            2.0 * r * (chord / (2.0 * r)).min(1.0).asin()
        }
    }
}

#[test]
fn svg_mesures_coupe_nette_couture_et_coupe_calculee() {
    let c = pc();
    let text = svg_of(&c);
    let doc = read_svg(&text);
    // Coupe nette : fond (8 tracés) et patte (4) en rôle « coupe », écrits une seule fois ; la
    // poche a sa couture (4 tracés) et sa coupe calculée ; aucune couture ni coupe calculée pour
    // le fond et la patte.
    let cut = doc.with_role("coupe");
    assert_eq!(cut.len(), 12);
    assert_eq!(doc.with_role("couture").len(), 4);
    assert_eq!(doc.with_role("coupe-calculee").len(), 1);
    // Repère du fichier : le point le plus à gauche des coupes est x = 0 du modèle, le plus haut
    // est y = 70 (haut du fond).
    let xmin = cut
        .iter()
        .flat_map(|i| &i.pts)
        .map(|p| p.0)
        .fold(f64::MAX, f64::min);
    let ymin = cut
        .iter()
        .flat_map(|i| &i.pts)
        .map(|p| p.1)
        .fold(f64::MAX, f64::min);
    let (ox, oy) = (xmin, ymin + 70.0);
    let model = |p: (f64, f64)| (p.0 - ox, oy - p.1);
    let (body, flap): (Vec<&&SvgItem>, Vec<&&SvgItem>) = cut
        .iter()
        .partition(|i| i.pts.iter().all(|p| model(*p).1 >= -1e-6));
    assert_eq!((body.len(), flap.len()), (8, 4));
    let sum = |v: &[&&SvgItem]| v.iter().map(|i| svg_len(i)).sum::<f64>();
    assert!(near(sum(&body), body_perimeter(), 1e-3), "{}", sum(&body));
    assert!(near(sum(&flap), flap_perimeter(), 1e-3), "{}", sum(&flap));
    // Couture de la poche : 90 + 50 + arc de rayon 205 + 50.
    let seam = doc.with_role("couture");
    assert!(seam
        .iter()
        .any(|i| i.d.as_deref().is_some_and(|d| d.contains("A 205 205"))));
    let seam_sum: f64 = seam.iter().map(|i| svg_len(i)).sum();
    assert!(near(seam_sum, pocket_perimeter(), 1e-3), "{seam_sum}");
    // Coupe calculée : arc de rayon 213 ; sommets et longueurs attendus.
    let calc = doc.with_role("coupe-calculee")[0];
    assert!(calc.d.as_deref().unwrap().contains("A 213 213"));
    let mut pts: Vec<(f64, f64)> = calc.pts.iter().map(|p| model(*p)).collect();
    assert_eq!(
        pts.pop(),
        Some(pts[0]),
        "tracé refermé : 7 points, 6 sommets"
    );
    for v in pocket_cut_vertices() {
        assert!(
            pts.iter()
                .any(|p| near(p.0, v.0, 1e-3) && near(p.1, v.1, 1e-3)),
            "sommet {v:?} absent de {pts:?}"
        );
    }
    let chords: Vec<f64> = (0..6)
        .map(|i| (pts[(i + 1) % 6].0 - pts[i].0).hypot(pts[(i + 1) % 6].1 - pts[i].1))
        .collect();
    assert!(
        same_lengths(&chords, &pocket_cut_chords(), 1e-3),
        "{chords:?}"
    );
    // Périmètre : droites + longueur de l'arc de rayon 213 (corde 93,51).
    let arc_chord = *chords
        .iter()
        .find(|c| near(**c, pocket_cut_chords()[4], 1e-3))
        .expect("corde de l'arc");
    let arc_len = 2.0 * 213.0 * (arc_chord / 426.0).asin();
    let total = chords.iter().sum::<f64>() - arc_chord + arc_len;
    assert!(near(total, pocket_cut_perimeter(), 1e-3), "{total}");
}

fn svg_segments(group: &str, model: &dyn Fn((f64, f64)) -> (f64, f64)) -> Vec<Seg> {
    group
        .lines()
        .map(str::trim)
        .filter(|l| l.starts_with("<line"))
        .map(|l| {
            let n = |k: &str| attr(l, k).unwrap().parse::<f64>().unwrap();
            (model((n("x1"), n("y1"))), model((n("x2"), n("y2"))))
        })
        .collect()
}

#[test]
fn svg_crans_percages_et_etiquettes() {
    let c = pc();
    let text = svg_of(&c);
    let doc = read_svg(&text);
    let xmin = doc
        .with_role("coupe")
        .iter()
        .flat_map(|i| i.pts.clone())
        .map(|p| p.0)
        .fold(f64::MAX, f64::min);
    let ymin = doc
        .with_role("coupe")
        .iter()
        .flat_map(|i| i.pts.clone())
        .map(|p| p.1)
        .fold(f64::MAX, f64::min);
    let model = move |p: (f64, f64)| (p.0 - xmin, ymin + 70.0 - p.1);
    // Crans de couture : cinq traits de 5 mm ; crans de rempli : trois traits de 5 mm sur la coupe.
    let (_, after) = text
        .split_once("data-role=\"cran\"")
        .expect("groupe des crans");
    let flat = svg_segments(after.split("</g>").next().unwrap(), &model);
    assert!(
        same_segments(&flat, &flat_notch_segments(), 1e-3),
        "{flat:?}"
    );
    let (_, rest) = text
        .split_once("data-role=\"cran-rempli\"")
        .expect("crans de rempli");
    let fold_lines: Vec<String> = text
        .lines()
        .filter(|l| l.contains("data-role=\"cran-rempli\""))
        .map(str::to_string)
        .collect();
    assert_eq!(fold_lines.len(), 3, "{rest}");
    let fold = svg_segments(&fold_lines.join("\n"), &model);
    assert!(
        same_segments(&fold, &fold_notch_segments(), 1e-3),
        "{fold:?}"
    );
    // Perçages : 23 cercles de 1,2 mm (rayon 0,6), centres à y = 3, x = 6 + 4k.
    let holes: Vec<(f64, f64, f64)> = text
        .lines()
        .map(str::trim)
        .filter(|l| l.starts_with("<circle") && l.contains("data-form=\"percage\""))
        .map(|l| {
            let n = |k: &str| attr(l, k).unwrap().parse::<f64>().unwrap();
            let c = model((n("cx"), n("cy")));
            (c.0, c.1, n("r"))
        })
        .collect();
    assert_eq!(holes.len(), 23);
    for (k, h) in holes.iter().enumerate() {
        assert!(
            near(h.0, hole_x(k), 1e-3) && near(h.1, 3.0, 1e-3) && near(h.2, 0.6, 1e-9),
            "{h:?}"
        );
    }
    // Étiquettes : cinq lignes par pièce, centrées sur l'emprise, accents fidèles.
    let labels: Vec<(f64, f64, String)> = text
        .lines()
        .map(str::trim)
        .filter(|l| l.starts_with("<text") && l.contains("data-role=\"etiquette-piece\""))
        .map(|l| {
            let n = |k: &str| attr(l, k).unwrap().parse::<f64>().unwrap();
            let m = model((n("x"), n("y")));
            (
                m.0,
                m.1,
                l.split_once('>')
                    .unwrap()
                    .1
                    .rsplit_once("</text>")
                    .unwrap()
                    .0
                    .to_string(),
            )
        })
        .collect();
    assert_eq!(labels.len(), 17, "{labels:?}");
    for (pi, (name, code, (cx, cy))) in LABELS.iter().enumerate() {
        let want = label_lines(name, code);
        for (i, line) in want.iter().enumerate() {
            let (x, y, t) = &labels[LABEL_FIRST[pi] + i];
            assert_eq!(t, line);
            assert!(near(*x, *cx, 1e-3), "{x}");
            assert!(near(*y, baseline_y(*cy, i, want.len()), 1e-3), "{y}");
        }
    }
    // Rempli de la poche (PH-5c) : « Rempli 8 mm » au milieu de l'arc, côté intérieur. L'arc
    // culmine en (165 ; 55), normale intérieure (0 ; −1) : centre à 1,5 + 3,5 / 2 = 3,25 mm sous
    // le sommet, soit (165 ; 51,75) ; ligne de base 0,35 × 3,5 = 1,225 mm plus bas : 50,525.
    let over: Vec<(f64, f64, String)> = text
        .lines()
        .map(str::trim)
        .filter(|l| l.starts_with("<text") && l.contains("data-role=\"libelle-rempli\""))
        .map(|l| {
            let n = |k: &str| attr(l, k).unwrap().parse::<f64>().unwrap();
            let m = model((n("x"), n("y")));
            let t = l
                .split_once('>')
                .unwrap()
                .1
                .rsplit_once("</text>")
                .unwrap()
                .0;
            (m.0, m.1, t.to_string())
        })
        .collect();
    assert_eq!(over.len(), 1, "{over:?}");
    assert_eq!(over[0].2, "Rempli 8 mm");
    assert!(
        near(over[0].0, 165.0, 1e-3) && near(over[0].1, 50.525, 1e-3),
        "{over:?}"
    );
}

// ---------------------------------------------------------------------------
// DXF
// ---------------------------------------------------------------------------

fn block_of<'a>(d: &'a rd::Dxf, name: &str) -> &'a [Ent] {
    let tag = format!("Piece Name: {name}");
    d.blocks
        .iter()
        .map(|(_, e)| e.as_slice())
        .find(|e| {
            e.iter()
                .any(|x| matches!(x, Ent::Text { text, .. } if *text == tag))
        })
        .unwrap_or_else(|| panic!("bloc de « {name} » absent"))
}

fn polys<'a>(ents: &'a [Ent], layer: &str) -> Vec<&'a Vec<(f64, f64)>> {
    ents.iter()
        .filter_map(|e| match e {
            Ent::Polyline {
                layer: l,
                pts,
                closed: true,
            } if l == layer => Some(pts),
            _ => None,
        })
        .collect()
}

fn lines_of(ents: &[Ent], layer: &str) -> Vec<Seg> {
    ents.iter()
        .filter_map(|e| match e {
            Ent::Line { layer: l, a, b } if l == layer => Some((*a, *b)),
            _ => None,
        })
        .collect()
}

fn min_x(p: &[(f64, f64)]) -> f64 {
    p.iter().map(|q| q.0).fold(f64::MAX, f64::min)
}
fn min_y(p: &[(f64, f64)]) -> f64 {
    p.iter().map(|q| q.1).fold(f64::MAX, f64::min)
}
fn max_y(p: &[(f64, f64)]) -> f64 {
    p.iter().map(|q| q.1).fold(f64::MIN, f64::max)
}

fn texts(ents: &[Ent]) -> Vec<&str> {
    ents.iter()
        .filter_map(|e| match e {
            Ent::Text { layer, text, .. } if layer == "84" => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

#[test]
fn dxf_couches_mesures_crans_percages_et_etiquettes() {
    let c = pc();
    let out = export_dxf(&c.project).unwrap();
    assert_eq!(out.report.omissions().count(), 0, "{:?}", out.report.issues);
    // Seule remarque : pas de droit-fil sur les trois pièces (tout est écrit).
    assert_eq!(out.report.notes().count(), 3, "{:?}", out.report.issues);
    let d = rd::parse(&out.text).unwrap();
    assert_eq!(d.blocks.len(), 3, "une pièce = un bloc");
    // Les arcs sont aplatis en segments : périmètre sous-estimé de moins de 0,01 mm, aire de moins
    // de 0,15 mm².
    let (per_tol, area_tol) = (0.01, 0.15);

    // Fond : coupe nette = une seule polyligne, couche 1 (aucune couche 15).
    let body = block_of(&d, "Fond");
    assert!(polys(body, "15").is_empty());
    let cut = polys(body, "1");
    assert_eq!(cut.len(), 1);
    assert!(near(rd::perimeter(cut[0], true), body_perimeter(), per_tol));
    assert!(near(rd::area(cut[0]).abs(), body_area(), area_tol));
    let (x0, y0) = (min_x(cut[0]), min_y(cut[0]));
    assert!(
        near(rd::bbox(cut[0]).1 .0 - x0, 100.0, 1e-6)
            && near(rd::bbox(cut[0]).1 .1 - y0, 70.0, 1e-6)
    );
    // Perçages (couche 13) : 23 cercles de rayon 0,6.
    let holes: Vec<(f64, f64, f64)> = body
        .iter()
        .filter_map(|e| match e {
            Ent::Circle { layer, c, r } if layer == "13" => Some((c.0 - x0, c.1 - y0, *r)),
            _ => None,
        })
        .collect();
    assert_eq!(holes.len(), 23);
    for (k, h) in holes.iter().enumerate() {
        assert!(
            near(h.0, hole_x(k), 1e-3) && near(h.1, 3.0, 1e-3) && near(h.2, 0.6, 1e-6),
            "{h:?}"
        );
    }
    // Crans du fond : 2 traits de 5 mm vers l'extérieur.
    let rel = |l: Vec<Seg>, o: (f64, f64)| -> Vec<Seg> {
        l.into_iter()
            .map(|(a, b)| ((a.0 - o.0, a.1 - o.1), (b.0 - o.0, b.1 - o.1)))
            .collect()
    };
    assert!(same_segments(
        &rel(lines_of(body, "4"), (x0, y0)),
        &flat_notch_segments()[0..2],
        1e-3
    ));

    // Poche : couture (15) et coupe calculée (1), crans de repère et de rempli (couche 4).
    let pocket = block_of(&d, "Poche");
    let seam = polys(pocket, "15");
    let cut = polys(pocket, "1");
    assert_eq!((seam.len(), cut.len()), (1, 1));
    assert!(near(
        rd::perimeter(seam[0], true),
        pocket_perimeter(),
        per_tol
    ));
    assert!(near(rd::area(seam[0]).abs(), pocket_area(), area_tol));
    assert!(near(
        rd::perimeter(cut[0], true),
        pocket_cut_perimeter(),
        per_tol
    ));
    assert!(near(rd::area(cut[0]).abs(), pocket_cut_area(), area_tol));
    let (px, py) = (min_x(seam[0]), min_y(seam[0]));
    // Marge : la coupe déborde la couture de 6 mm à gauche et en bas.
    assert!(near(min_x(cut[0]), px - 6.0, 1e-6) && near(min_y(cut[0]), py - 6.0, 1e-6));
    let mut expected = flat_notch_segments()[2..4].to_vec();
    expected.extend(fold_notch_segments());
    let o = (px - 120.0, py);
    assert!(same_segments(
        &rel(lines_of(pocket, "4"), o),
        &expected,
        1e-3
    ));

    // Patte : coupe nette, un cran.
    let flap = block_of(&d, "Patte");
    assert!(polys(flap, "15").is_empty());
    let cut = polys(flap, "1");
    assert_eq!(cut.len(), 1);
    assert!(near(rd::perimeter(cut[0], true), flap_perimeter(), per_tol));
    assert!(near(rd::area(cut[0]).abs(), flap_area(), area_tol));
    let o = (min_x(cut[0]), max_y(cut[0]) + 20.0);
    assert!(same_segments(
        &rel(lines_of(flap, "4"), o),
        &flat_notch_segments()[4..5],
        1e-3
    ));

    // Étiquettes de pièce (couche 84) : nom, code, quantité, matière, catégorie ; le fond et la
    // patte (coupe nette) ajoutent « Net cut », la poche (rempli 8 mm) un texte « Fold-over 8 mm »
    // (PH-5c).
    for (name, code, _) in LABELS {
        let mut want = vec![
            format!("Piece Name: {name}"),
            format!("Code: {code}"),
            "Quantity: 1".to_string(),
            "Material: Cuir".to_string(),
            "Category: leather".to_string(),
        ];
        match name {
            "Poche" => want.push("Fold-over 8 mm".to_string()),
            _ => want.push("Net cut".to_string()),
        }
        assert_eq!(texts(block_of(&d, name)), want);
    }
    // Position du libellé du rempli, repère modèle (déduit de `o`) : centre (165 ; 51,75), texte de
    // 14 caractères × 4 × 0,6 = 33,6 mm de large, coin bas-gauche (165 − 16,8 ; 51,75 − 2).
    let over = block_of(&d, "Poche")
        .iter()
        .find_map(|e| match e {
            Ent::Text { text, at, .. } if text == "Fold-over 8 mm" => Some(*at),
            _ => None,
        })
        .unwrap();
    assert!(
        near(over.0 - (px - 120.0), 148.2, 1e-3) && near(over.1 - py, 49.75, 1e-3),
        "{over:?}"
    );
}

// ---------------------------------------------------------------------------
// HPGL
// ---------------------------------------------------------------------------

#[test]
fn hpgl_traces_plumes_percages_et_etiquettes() {
    let c = pc();
    let scene = Scene::from_sheet(&c.project, c.sheet).unwrap();
    let plot = export_hpgl(&scene, &construction::hpgl_page()).expect("A4 paysage suffit");
    assert!(plot.text.is_ascii());
    let r = read_hpgl(&plot.text).unwrap();
    let on_pen = |pen: u8| -> Vec<&common::hpgl::Stroke> {
        r.strokes.iter().filter(|s| s.pen == pen).collect()
    };
    let lens = |pen: u8| {
        on_pen(pen)
            .iter()
            .map(|s| s.length_mm())
            .collect::<Vec<_>>()
    };
    // Plume 1 (coupe) : fond (90, 90, 60, 60, 4 quarts de cercle), patte (30, 80, 30, demi-cercle),
    // coupe calculée de la poche (un seul tracé fermé). Arrondi au 1/40 mm : 0,1 mm de tolérance.
    let mut want = vec![
        90.0,
        90.0,
        60.0,
        60.0,
        30.0,
        80.0,
        30.0,
        PI * 40.0,
        pocket_cut_perimeter(),
    ];
    want.extend([5.0 * PI / 2.0; 4]);
    assert!(same_lengths(&lens(1), &want, 0.1), "{:?}", lens(1));
    // Plume 2 (couture) : poche seulement.
    let want = [90.0, 50.0, 50.0, 2.0 * 205.0 * theta()];
    assert!(same_lengths(&lens(2), &want, 0.1), "{:?}", lens(2));
    // Plume 5 (crans) : huit traits de 5 mm. Plume 6 (marquages) : 23 perçages de 1,2 mm de
    // diamètre (polygone inscrit : 3,753 pour 3,770 mm).
    assert!(
        lens(5).len() == 8 && lens(5).iter().all(|l| near(*l, 5.0, 0.05)),
        "{:?}",
        lens(5)
    );
    assert!(
        lens(6).len() == 23 && lens(6).iter().all(|l| near(*l, 1.2 * PI, 0.05)),
        "{:?}",
        lens(6)
    );
    // Repère : le bord bas du fond (90 mm, le plus bas) commence au modèle (5 ; 0).
    let bottom = on_pen(1)
        .into_iter()
        .filter(|s| near(s.length_mm(), 90.0, 0.1))
        .min_by_key(|s| s.pts[0].1.min(s.pts[1].1))
        .unwrap();
    let start = *bottom.pts.iter().min().unwrap();
    let (ox, oy) = (start.0 as f64 - 5.0 * 40.0, start.1 as f64);
    let near_u = |a: i64, b: f64| (a as f64 - b).abs() <= 2.0;
    // Perçages : centre = milieu de l'emprise du polygone, y = 3 mm, x = 6 + 4k mm.
    let mut centres: Vec<(f64, f64)> = on_pen(6)
        .iter()
        .map(|s| {
            let (x0, x1) = (
                s.pts.iter().map(|p| p.0).min().unwrap(),
                s.pts.iter().map(|p| p.0).max().unwrap(),
            );
            let (y0, y1) = (
                s.pts.iter().map(|p| p.1).min().unwrap(),
                s.pts.iter().map(|p| p.1).max().unwrap(),
            );
            ((x0 + x1) as f64 / 2.0, (y0 + y1) as f64 / 2.0)
        })
        .collect();
    centres.sort_by(|a, b| a.0.total_cmp(&b.0));
    for (k, ce) in centres.iter().enumerate() {
        assert!(
            near(ce.0, ox + hole_x(k) * 40.0, 2.0) && near(ce.1, oy + 3.0 * 40.0, 2.0),
            "{ce:?}"
        );
    }
    // Étiquettes : 17 lignes `LB` (6 + 5 + 6) à la plume 7, ASCII, caractère de 4 mm, puis le
    // libellé du rempli (PH-5c).
    assert_eq!(r.labels.len(), 18, "{:?}", r.labels);
    for (pi, (name, code, (cx, cy))) in LABELS.iter().enumerate() {
        let want = label_lines(name, code);
        for (i, line) in want.iter().enumerate() {
            let l = &r.labels[LABEL_FIRST[pi] + i];
            assert_eq!(l.text, ascii(line));
            assert_eq!(l.pen, 7);
            let start_x = cx - line.chars().count() as f64 * 2.2 / 2.0;
            assert!(near_u(l.at.0, ox + start_x * 40.0), "{:?}", l.at);
            assert!(
                near_u(l.at.1, oy + baseline_y(*cy, i, want.len()) * 40.0),
                "{:?}",
                l.at
            );
        }
    }
    // « Rempli 8 mm » : centre (165 ; 51,75), 11 × 3,5 × 0,62 = 23,87 mm de large : départ de la
    // ligne de base en (165 − 11,935 ; 51,75 − 1,225) = (153,065 ; 50,525).
    let over = &r.labels[17];
    assert_eq!((over.text.as_str(), over.pen), ("Rempli 8 mm", 7));
    assert!(near_u(over.at.0, ox + 153.065 * 40.0), "{:?}", over.at);
    assert!(near_u(over.at.1, oy + 50.525 * 40.0), "{:?}", over.at);
    assert_eq!(plot.report.labels, 18);
}

// ---------------------------------------------------------------------------
// PDF
// ---------------------------------------------------------------------------

fn pdf_len_mm(s: &PdfStroke) -> f64 {
    let (a, b) = (s.pts[0], *s.pts.last().unwrap());
    (b.0 - a.0).hypot(b.1 - a.1) * 25.4 / 72.0
}

#[test]
fn pdf_pages_traits_percages_et_etiquettes() {
    let c = pc();
    let scene = Scene::from_sheet(&c.project, c.sheet).unwrap();
    let out = export_pdf(&[scene]).unwrap();
    // Emprise 210 × 160 mm : étalon + 2 colonnes × 1 ligne de tuiles, toutes en A4 portrait.
    assert_eq!(out.page_count, 3);
    for (w, h) in &out.page_sizes_mm {
        assert!(near(*w, 210.0, 0.1) && near(*h, 297.0, 0.1), "{w} × {h}");
    }
    let pages = common::read_pdf(&out.bytes);
    assert_eq!(pages.len(), 3);
    assert!(pages[0].has("« Porte-cartes » : 2 page(s), 1 ligne(s) × 2 colonne(s)"));
    // Étiquettes (police embarquée, accents relus) : lignes consécutives par pièce (la mention
    // « Coupe nette » pour le fond et la patte, PH-5c) et le libellé « Rempli 8 mm ».
    assert!(pages.iter().any(|p| p.has("Rempli 8 mm")));
    for (name, code, _) in LABELS {
        let want = label_lines(name, code);
        assert!(
            pages.iter().any(|p| p.strings.windows(want.len()).any(|w| w
                .iter()
                .map(String::as_str)
                .eq(want.iter().map(String::as_str)))),
            "étiquette de {name} absente"
        );
    }
    // Traits de la première page de patron (toute la scène, découpée par la fenêtre de page).
    let strokes = read_strokes(&out.bytes);
    let width = |s: &PdfStroke, mm: f64| near(s.width_pt, mm_pt(mm), 1e-3);
    // Coupe nette (0,6 mm, trait plein) : 4 quarts de cercle (corde 5√2) et un demi-cercle (corde
    // 80), + coupe calculée de la poche en trait-point 10 / 2,5 / 2,5 / 2,5, fermée.
    let arcs: Vec<f64> = strokes
        .iter()
        .filter(|s| s.curves > 0 && s.dash_pt.is_empty() && width(s, 0.6) && s.pts.len() <= 4)
        .map(pdf_len_mm)
        .collect();
    let mut want = vec![5.0 * 2f64.sqrt(); 4];
    want.push(80.0);
    assert!(same_lengths(&arcs, &want, 1e-3), "{arcs:?}");
    let dash_cut: Vec<f64> = [10.0, 2.5, 2.5, 2.5].iter().map(|v| mm_pt(*v)).collect();
    let calc: Vec<&PdfStroke> = strokes
        .iter()
        .filter(|s| {
            s.dash_pt.len() == 4
                && s.dash_pt
                    .iter()
                    .zip(&dash_cut)
                    .all(|(a, b)| near(*a, *b, 1e-3))
        })
        .collect();
    assert_eq!(calc.len(), 1);
    assert!(calc[0].closed && calc[0].curves == 1);
    let chords: Vec<f64> = calc[0]
        .pts
        .windows(2)
        .map(|w| (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1) * 25.4 / 72.0)
        .collect();
    assert!(
        same_lengths(&chords, &pocket_cut_chords(), 1e-3),
        "{chords:?}"
    );
    // Couture de la poche (tirets 3 / 2) : 90, 50, 50 et l'arc (corde 90).
    let dash_seam = [mm_pt(3.0), mm_pt(2.0)];
    let seam: Vec<f64> = strokes
        .iter()
        .filter(|s| {
            s.dash_pt.len() == 2
                && s.dash_pt
                    .iter()
                    .zip(&dash_seam)
                    .all(|(a, b)| near(*a, *b, 1e-3))
        })
        .map(pdf_len_mm)
        .collect();
    assert!(
        same_lengths(&seam, &[90.0, 50.0, 50.0, 90.0], 1e-3),
        "{seam:?}"
    );
    // Repère : bord bas du fond = le plus bas des deux traits horizontaux pleins de 90 mm.
    let bottom = strokes
        .iter()
        .filter(|s| {
            s.pts.len() == 2
                && s.dash_pt.is_empty()
                && width(s, 0.6)
                && near(pdf_len_mm(s), 90.0, 1e-3)
                && near(s.pts[0].1, s.pts[1].1, 1e-6)
        })
        .min_by(|a, b| a.pts[0].1.total_cmp(&b.pts[0].1))
        .unwrap();
    let (bx, by) = (bottom.pts[0].0.min(bottom.pts[1].0), bottom.pts[0].1);
    // Perçages (0,35 mm, 4 nœuds de Bézier) : diamètre 1,2 mm, centre à y = 3, x = 6 + 4k.
    let mm = |v: f64| v * 25.4 / 72.0;
    let mut holes: Vec<(f64, f64, f64)> = strokes
        .iter()
        .filter(|s| s.curves == 4 && s.pts.len() == 5 && width(s, 0.35))
        .map(|s| {
            (
                mm(s.pts[0].0 + s.pts[2].0) / 2.0,
                mm(s.pts[0].1),
                mm(s.pts[0].0 - s.pts[2].0),
            )
        })
        .collect();
    holes.sort_by(|a, b| a.0.total_cmp(&b.0));
    assert_eq!(holes.len(), 23);
    for (k, h) in holes.iter().enumerate() {
        assert!(near(h.0 - mm(bx), hole_x(k) - 5.0, 1e-2), "{h:?}");
        assert!(
            near(h.1 - mm(by), 3.0, 1e-2) && near(h.2, 1.2, 1e-2),
            "{h:?}"
        );
    }
    // Crans : huit traits de 5 mm (0,6 mm).
    let notches = strokes
        .iter()
        .filter(|s| {
            s.pts.len() == 2
                && s.dash_pt.is_empty()
                && width(s, 0.6)
                && near(pdf_len_mm(s), 5.0, 0.02)
        })
        .count();
    assert_eq!(notches, 8);
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
    let (a, b) = (pc(), pc());
    assert_eq!(a.project, b.project);
    assert_eq!(
        persist::to_json(&a.project).unwrap(),
        persist::to_json(&b.project).unwrap()
    );
    assert_eq!(exports(&a.project, a.sheet), exports(&b.project, b.sheet));
}

#[test]
fn projet_charge_depuis_l_exemple_exporte_comme_le_projet_construit() {
    let c = pc();
    let loaded = persist::load(&examples().join("porte-cartes.pforge")).expect(HINT);
    assert_eq!(loaded, c.project, "{HINT}");
    assert_eq!(
        exports(&loaded, loaded.sheets[0].id),
        exports(&c.project, c.sheet)
    );
}

#[test]
fn fichiers_d_exemple_a_jour() {
    let c = pc();
    let scene = Scene::from_sheet(&c.project, c.sheet).unwrap();
    let read =
        |name: &str| fs::read(examples().join(name)).unwrap_or_else(|e| panic!("{name} : {e}"));
    assert_eq!(
        read("porte-cartes.pforge"),
        persist::to_json(&c.project).unwrap().into_bytes(),
        "{HINT}"
    );
    assert_eq!(
        read("porte-cartes.svg"),
        export_svg(&scene).unwrap().into_bytes(),
        "{HINT}"
    );
    assert_eq!(
        read("porte-cartes.pdf"),
        export_pdf(std::slice::from_ref(&scene)).unwrap().bytes,
        "{HINT}"
    );
    assert_eq!(
        read("porte-cartes.dxf"),
        export_dxf(&c.project).unwrap().text.into_bytes(),
        "{HINT}"
    );
    assert_eq!(
        read("porte-cartes.hpgl"),
        export_hpgl(&scene, &construction::hpgl_page())
            .unwrap()
            .text
            .into_bytes(),
        "{HINT}"
    );
}
/// Les fichiers d'exemple connus sont tous présents (aucun fichier existant retiré ni renommé).
#[test]
fn dossier_examples_contient_les_fichiers_attendus() {
    let names: Vec<String> = fs::read_dir(examples())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    for base in [
        "etalon-100mm.pdf",
        "etalon-100mm.svg",
        "geometrie-exercice.pdf",
        "geometrie-exercice.pforge",
        "geometrie-exercice.svg",
        "grande-piece-600x450.pdf",
        "grande-piece-600x450.pforge",
        "piece-crans-droit-fil.pdf",
        "piece-crans-droit-fil.pforge",
        "piece-crans-droit-fil.svg",
    ] {
        assert!(names.iter().any(|n| n == base), "{base} absent");
    }
    for stem in ["jupe-droite", "porte-cartes"] {
        for ext in ["pforge", "svg", "pdf", "dxf", "hpgl"] {
            let name = format!("{stem}.{ext}");
            assert!(names.contains(&name), "{name} absent");
        }
    }
}
