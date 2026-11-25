//! US-05.4 — campagne adverse de l'export couture / coupe : 180 séquences à graine fixe.
//!
//! Chaque séquence construit une pièce (rectangle à marges par bord, rectangle sans marge,
//! rectangle à marges toutes nulles, quart de disque à rempli et crans, polygone à coupe
//! impossible), choisit un mode d'export au hasard et exporte la **même** pièce en SVG, PDF, DXF
//! et HPGL. L'oracle est indépendant : un rectangle `w × h` aux marges `(bas, droite, haut,
//! gauche)` a pour coupe exacte `(w + g + d) × (h + b + t)` (coins en onglet à angle droit), de
//! périmètre `2 (W + H)` et d'aire `W × H` ; un quart de disque de rayon `r` à rempli `v` a un arc
//! de coupe de rayon `r + v` et des crans radiaux de pied `r + v` et de pointe `r + v - min(5, v)`.
//! Aucune fonction de `pf_geometry::seam` ni de `piece_cut_contour` ne sert à juger.
//!
//! Propriétés : jamais de panique ; sortie identique à graine égale (octets) ; les modes se
//! comportent comme annoncé (couture seule = export d'avant, coupe seule sans couture, les deux
//! sans doublon) ; sans marge, trois modes = mêmes octets ; coupe impossible = avertissement
//! lisible, couture conservée. Des compteurs prouvent que chaque contrôle fort a été exécuté.

mod common;

use common::coupe::*;
use common::dxf as rd;
use common::hpgl::read_hpgl;
use pf_core::{Command, EntityId, PathRole, Project, SeamAllowance, Shape, SheetId};
use pf_core::{EdgeAllowance, PieceId};
use pf_export::dxf::{export_dxf_with, DxfIssueKind};
use pf_export::hpgl::export_hpgl;
use pf_export::pdf::export_pdf;
use pf_export::svg::export_svg;
use pf_export::{PageSetup, PathGeom, PieceExportMode, Scene};
use std::panic::{catch_unwind, AssertUnwindSafe};

const SEQUENCES: u64 = 180;
const SEED: u64 = 0x0504_C0DE;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
    /// Multiple de `step` dans `[lo ; hi]`.
    fn grid(&mut self, lo: f64, hi: f64, step: f64) -> f64 {
        ((lo + (hi - lo) * self.unit()) / step).round() * step
    }
}

#[derive(Default, Debug)]
struct Counters {
    rect_margins: usize,
    rect_zero_margins: usize,
    no_seam: usize,
    quarter: usize,
    impossible: usize,
    svg_vertices: usize,
    svg_modes: usize,
    pdf_spans: usize,
    pdf_dashes: usize,
    dxf_areas: usize,
    dxf_layers: usize,
    hpgl_lengths: usize,
    notch_radii: usize,
    notch_counts: usize,
    same_bytes_modes: usize,
    sewing_only_equals_before: usize,
    warnings: usize,
    determinism: usize,
}

const MODES: [PieceExportMode; 3] = [
    PieceExportMode::Both,
    PieceExportMode::SewingOnly,
    PieceExportMode::CutOnly,
];

fn scene_of(p: &Project, s: SheetId, m: PieceExportMode) -> Scene {
    Scene::from_sheet_with(
        p,
        s,
        pf_export::SceneOptions {
            piece_mode: m,
            ..Default::default()
        },
    )
    .unwrap()
}

fn page() -> PageSetup {
    PageSetup::default()
}

fn svg_of(p: &Project, s: SheetId, m: PieceExportMode) -> String {
    export_svg(&scene_of(p, s, m)).unwrap()
}

/// Rectangle `(x0 ; y0)`, `w × h`, marges `[bas, droite, haut, gauche]` par bord (`None` = pas de
/// marge enregistrée). Renvoie le projet, la feuille et la pièce.
fn rectangle(
    x0: f64,
    y0: f64,
    w: f64,
    h: f64,
    margins: Option<[f64; 4]>,
) -> (Project, SheetId, PieceId) {
    let mut p = Project::new("Adverse");
    let s = p.sheets[0].id;
    let (piece, edges) = common::dxf_models::rect_piece(
        &mut p,
        s,
        (x0, y0),
        (w, h),
        "Rect",
        PathRole::SewingContour,
        false,
    );
    if let Some(m) = margins {
        // Identification des bords par leur géométrie (indépendante de l'ordre des tracés).
        let sheet = p.sheet(s).unwrap();
        let mut by_side: [Option<EntityId>; 4] = [None; 4];
        for &e in &edges {
            let Shape::Line(a, b) = sheet.shape(e).unwrap() else {
                panic!("côté non droit")
            };
            let side = if a.y == b.y && a.y == y0 {
                0
            } else if a.x == b.x && a.x == x0 + w {
                1
            } else if a.y == b.y && a.y == y0 + h {
                2
            } else {
                3
            };
            by_side[side] = Some(e);
        }
        let list: Vec<EdgeAllowance> = (0..4)
            .map(|i| EdgeAllowance {
                entity: by_side[i].expect("quatre côtés"),
                start_mm: m[i],
                end_mm: m[i],
            })
            .collect();
        p.apply(Command::SetSeamAllowance {
            piece,
            seam: Some(SeamAllowance {
                default_mm: 0.0,
                edges: list,
                ..Default::default()
            }),
        })
        .unwrap();
    }
    (p, s, piece)
}

/// Sommets de l'enveloppe d'une pièce rectangulaire, calculés à la main.
fn rect_cut_extents(x0: f64, y0: f64, w: f64, h: f64, m: [f64; 4]) -> ((f64, f64), (f64, f64)) {
    ((x0 - m[3], y0 - m[0]), (x0 + w + m[1], y0 + h + m[2]))
}

fn bbox_of_edges(scene: &Scene) -> ((f64, f64), (f64, f64)) {
    let mut lo = (f64::MAX, f64::MAX);
    let mut hi = (f64::MIN, f64::MIN);
    for c in &scene.cut_contours {
        for g in &c.edges {
            let PathGeom::Line(a, b) = g else {
                panic!("la coupe d'un rectangle n'a que des segments")
            };
            for q in [a, b] {
                lo = (lo.0.min(q.x), lo.1.min(q.y));
                hi = (hi.0.max(q.x), hi.1.max(q.y));
            }
        }
    }
    (lo, hi)
}

fn polys_on(dxf: &rd::Dxf, layer: &str) -> Vec<Vec<(f64, f64)>> {
    dxf.blocks
        .iter()
        .flat_map(|(_, e)| e)
        .filter_map(|e| match e {
            rd::Ent::Polyline { layer: l, pts, .. } if l == layer => Some(pts.clone()),
            _ => None,
        })
        .collect()
}

fn lines_on(dxf: &rd::Dxf, layer: &str) -> Vec<((f64, f64), (f64, f64))> {
    dxf.blocks
        .iter()
        .flat_map(|(_, e)| e)
        .filter_map(|e| match e {
            rd::Ent::Line { layer: l, a, b } if l == layer => Some((*a, *b)),
            _ => None,
        })
        .collect()
}

fn pen_len(plt: &str, pen: u8) -> f64 {
    read_hpgl(plt)
        .unwrap()
        .strokes
        .iter()
        .filter(|s| s.pen == pen)
        .map(|s| s.length_mm())
        .sum()
}

// ---------------------------------------------------------------------------------------------
// Rectangle : oracle exact
// ---------------------------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn check_rect(
    c: &mut Counters,
    x0: f64,
    y0: f64,
    w: f64,
    h: f64,
    margins: Option<[f64; 4]>,
    mode: PieceExportMode,
) {
    let (p, s, _) = rectangle(x0, y0, w, h, margins);
    let seam = p.pieces[0].seam.is_some();
    let m = margins.unwrap_or([0.0; 4]);
    if !seam {
        // Sans marge enregistrée : aucun contour calculé, trois modes = mêmes octets, quatre
        // sorties, et pas de doublon (quatre côtés de couture exactement).
        c.no_seam += 1;
        let base = scene_of(&p, s, PieceExportMode::Both);
        assert!(base.cut_contours.is_empty() && base.cut_warnings.is_empty());
        let (svg, pdf, plt) = (
            export_svg(&base).unwrap(),
            export_pdf(std::slice::from_ref(&base)).unwrap().bytes,
            export_hpgl(&base, &page()).unwrap().text,
        );
        let dxf = export_dxf_with(&p, PieceExportMode::Both).unwrap().text;
        for mm in MODES {
            let sc = scene_of(&p, s, mm);
            assert_eq!(export_svg(&sc).unwrap(), svg, "{mm:?}");
            assert_eq!(
                export_pdf(std::slice::from_ref(&sc)).unwrap().bytes,
                pdf,
                "{mm:?}"
            );
            assert_eq!(export_hpgl(&sc, &page()).unwrap().text, plt, "{mm:?}");
            assert_eq!(export_dxf_with(&p, mm).unwrap().text, dxf, "{mm:?}");
            c.same_bytes_modes += 1;
        }
        assert_eq!(read_svg(&svg).with_role("couture").len(), 4);
        assert!(near(pen_len(&plt, 2), 2.0 * (w + h), 0.05));
        c.hpgl_lengths += 1;
        return;
    }

    let (lo, hi) = rect_cut_extents(x0, y0, w, h, m);
    let (cw, ch) = (hi.0 - lo.0, hi.1 - lo.1);
    let scene = scene_of(&p, s, mode);
    let svg = export_svg(&scene).unwrap();
    let doc = read_svg(&svg);
    let want_cut = mode.includes_cut();
    let want_sewing = mode.includes_sewing();

    // Scène : contour calculé = enveloppe attendue (extrêmes des segments).
    if want_cut {
        assert_eq!(scene.cut_contours.len(), 1);
        let (l, hh) = bbox_of_edges(&scene);
        assert!(
            near(l.0, lo.0, 1e-6) && near(l.1, lo.1, 1e-6),
            "{l:?} / {lo:?}"
        );
        assert!(
            near(hh.0, hi.0, 1e-6) && near(hh.1, hi.1, 1e-6),
            "{hh:?} / {hi:?}"
        );
    } else {
        assert!(scene.cut_contours.is_empty());
    }

    // SVG : présence selon le mode, sommets calculés à la main.
    assert_eq!(doc.with_role("coupe-calculee").len(), usize::from(want_cut));
    assert_eq!(
        doc.with_role("couture").len(),
        if want_sewing { 4 } else { 0 }
    );
    c.svg_modes += 1;
    // Emprise écrite : coupe si présente, sinon couture ; marge blanche 10 mm, Y retourné.
    let (blo, bhi) = if want_cut {
        (lo, hi)
    } else {
        ((x0, y0), (x0 + w, y0 + h))
    };
    assert!(near(doc.width, bhi.0 - blo.0 + 20.0, 1e-3));
    assert!(near(doc.height, bhi.1 - blo.1 + 20.0, 1e-3));
    let to_svg = |x: f64, y: f64| (x - blo.0 + 10.0, bhi.1 - y + 10.0);
    if want_cut {
        let corners = [
            to_svg(lo.0, lo.1),
            to_svg(hi.0, lo.1),
            to_svg(hi.0, hi.1),
            to_svg(lo.0, hi.1),
        ];
        let found = &doc.with_role("coupe-calculee")[0].pts;
        // Deux marges nulles contiguës ne créent pas de sommet : tous les sommets lus sont des
        // coins attendus, et tous les coins attendus sont lus (marge ou non).
        assert!(same_set(found, &corners, 1e-3), "{found:?} / {corners:?}");
        c.svg_vertices += 1;
    }

    // Couture seule = export d'avant (mêmes octets que la pièce sans marge).
    if mode == PieceExportMode::SewingOnly {
        let (q, s2, _) = rectangle(x0, y0, w, h, None);
        assert_eq!(svg, svg_of(&q, s2, PieceExportMode::Both));
        c.sewing_only_equals_before += 1;
    }

    // PDF : étendue et pointillés.
    let pdf = export_pdf(std::slice::from_ref(&scene)).unwrap().bytes;
    let strokes = read_pdf(&pdf);
    let closed: Vec<&PdfStroke> = strokes.iter().filter(|t| t.closed).collect();
    assert_eq!(closed.len(), usize::from(want_cut));
    let sewing_dashed = strokes.iter().filter(|t| t.dash_pt.len() == 2).count();
    assert_eq!(sewing_dashed, if want_sewing { 4 } else { 0 });
    c.pdf_dashes += 1;
    if let Some(cut) = closed.first() {
        let span = |f: fn(&(f64, f64)) -> f64| {
            let v: Vec<f64> = cut.pts.iter().map(f).collect();
            v.iter().cloned().fold(f64::MIN, f64::max) - v.iter().cloned().fold(f64::MAX, f64::min)
        };
        assert!(
            near(span(|q| q.0), mm_pt(cw), 3e-3),
            "{} / {}",
            span(|q| q.0),
            mm_pt(cw)
        );
        assert!(near(span(|q| q.1), mm_pt(ch), 3e-3));
        assert_eq!(cut.dash_pt.len(), 4);
        assert!(near(cut.dash_pt[0], mm_pt(10.0), 2e-3));
        c.pdf_spans += 1;
    }

    // DXF : couches 1 (coupe) et 15 (couture), aires.
    let out = export_dxf_with(&p, mode).unwrap();
    let dxf = rd::parse(&out.text).unwrap();
    let (l1, l15) = (polys_on(&dxf, "1"), polys_on(&dxf, "15"));
    assert_eq!(l1.len(), usize::from(want_cut));
    assert_eq!(l15.len(), usize::from(want_sewing));
    c.dxf_layers += 1;
    if want_cut {
        assert!(near(
            rd::area(&l1[0]).abs(),
            cw * ch,
            1e-4 * (1.0 + cw * ch)
        ));
        assert!(near(rd::perimeter(&l1[0], true), 2.0 * (cw + ch), 1e-4));
        c.dxf_areas += 1;
    }
    if want_sewing {
        assert!(near(rd::area(&l15[0]).abs(), w * h, 1e-4 * (1.0 + w * h)));
        c.dxf_areas += 1;
    }
    assert_eq!(
        out.report
            .issues
            .iter()
            .any(|i| i.kind == DxfIssueKind::NoCutContour),
        !want_cut
    );

    // HPGL : plume 1 = coupe (périmètre), plume 2 = couture (périmètre).
    let plt = export_hpgl(&scene, &page()).unwrap().text;
    let (cut_len, sew_len) = (pen_len(&plt, 1), pen_len(&plt, 2));
    assert!(near(
        cut_len,
        if want_cut { 2.0 * (cw + ch) } else { 0.0 },
        0.1
    ));
    assert!(near(
        sew_len,
        if want_sewing { 2.0 * (w + h) } else { 0.0 },
        0.1
    ));
    c.hpgl_lengths += 1;

    // Déterminisme.
    assert_eq!(svg, svg_of(&p, s, mode));
    assert_eq!(pdf, export_pdf(&[scene_of(&p, s, mode)]).unwrap().bytes);
    c.determinism += 1;
    if m == [0.0; 4] {
        c.rect_zero_margins += 1;
    } else {
        c.rect_margins += 1;
    }
}

// ---------------------------------------------------------------------------------------------
// Quart de disque : arc de coupe et crans de rempli
// ---------------------------------------------------------------------------------------------

fn check_quarter(c: &mut Counters, r: f64, v: f64, n: u32, mode: PieceExportMode) {
    let (p, s, _) = quarter_fold_with(r, v, n);
    let mut scene = scene_of(&p, s, mode);
    // Sans le libellé « Rempli … mm » (PH-5c, ses propres tests) : il déplacerait l'emprise.
    scene.fold_overs.clear();
    let svg = export_svg(&scene).unwrap();
    let doc = read_svg(&svg);
    let big = r + v;
    let len = 5.0f64.min(v);
    if !mode.includes_cut() {
        assert!(scene.cut_contours.is_empty());
        assert!(doc.with_role("cran-rempli").is_empty());
        let dxf = rd::parse(&export_dxf_with(&p, mode).unwrap().text).unwrap();
        assert!(lines_on(&dxf, "4").is_empty());
        c.quarter += 1;
        return;
    }
    // Arc de coupe de rayon r + v (formaté comme l'écrivain : « A 72 72 »).
    let cut = doc.with_role("coupe-calculee");
    assert_eq!(cut.len(), 1);
    let arc = format!("A {} {}", big, big);
    assert!(
        cut[0].d.as_ref().unwrap().contains(&arc),
        "{arc} / {:?}",
        cut[0].d
    );
    // Crans : n traits radiaux (centre (0 ; 0) → SVG (10 ; big + 10)).
    let marks = doc.with_role("cran-rempli");
    assert_eq!(marks.len(), n as usize);
    c.notch_counts += 1;
    let centre = (10.0, big + 10.0);
    for mk in &marks {
        let (a, b) = (mk.pts[0], mk.pts[1]);
        let (ra, rb) = (
            (a.0 - centre.0).hypot(a.1 - centre.1),
            (b.0 - centre.0).hypot(b.1 - centre.1),
        );
        assert!(near(ra, big, 2e-3), "pied {ra} / {big}");
        assert!(near(rb, big - len, 2e-3), "pointe {rb} / {}", big - len);
        let sin =
            ((a.0 - centre.0) * (b.1 - centre.1) - (a.1 - centre.1) * (b.0 - centre.0)) / (ra * rb);
        assert!(sin.abs() < 1e-4);
        // Quart de disque : pied dans le quadrant (x ≥ 0, y ≥ 0 du modèle).
        assert!(a.0 >= centre.0 - 1e-3 && a.1 <= centre.1 + 1e-3);
        c.notch_radii += 1;
    }
    // DXF : couche 4, mêmes longueurs ; HPGL : plume 5.
    let out = export_dxf_with(&p, mode).unwrap();
    let dxf = rd::parse(&out.text).unwrap();
    let dl = lines_on(&dxf, "4");
    assert_eq!(dl.len(), n as usize);
    for (a, b) in dl {
        assert!(near(rd::dist(a, b), len, 1e-4));
    }
    let plt = export_hpgl(&scene, &page()).unwrap().text;
    let replay = read_hpgl(&plt).unwrap();
    let n5: Vec<_> = replay.strokes.iter().filter(|st| st.pen == 5).collect();
    assert_eq!(n5.len(), n as usize);
    assert!(n5.iter().all(|st| near(st.length_mm(), len, 0.05)));
    c.hpgl_lengths += 1;
    // PDF : n traits pleins de la longueur d'un cran.
    // Sans les repères de points (petites croix de 5 mm), seuls les crans sont des traits pleins.
    let no_points = pf_export::OutputContent::default()
        .without(pf_export::OutputCategory::Points)
        .apply(&scene)
        .unwrap();
    let strokes = read_pdf(&export_pdf(&[no_points]).unwrap().bytes);
    let k = strokes
        .iter()
        .filter(|t| !t.closed && t.pts.len() == 2 && t.dash_pt.is_empty())
        .filter(|t| near(stroke_len_pt(t), mm_pt(len), 6e-3))
        .count();
    assert_eq!(k, n as usize);
    c.quarter += 1;
}

// ---------------------------------------------------------------------------------------------
// Coupe impossible
// ---------------------------------------------------------------------------------------------

fn check_impossible(c: &mut Counters, mode: PieceExportMode) {
    let (p, s, _) = impossible_cut();
    let scene = scene_of(&p, s, mode);
    let svg = export_svg(&scene).unwrap();
    let doc = read_svg(&svg);
    if mode.includes_cut() {
        assert_eq!(scene.cut_warnings.len(), 1);
        let msg = scene.cut_warnings[0].to_string();
        assert!(msg.contains("Encoche"), "{msg}");
        assert!(doc.with_role("coupe-calculee").is_empty());
        // Couture exportée dans tous les cas (jamais de fichier vide silencieux).
        assert_eq!(doc.with_role("couture").len(), 8);
        let out = export_dxf_with(&p, mode).unwrap();
        assert!(out
            .report
            .omissions()
            .any(|i| i.kind == DxfIssueKind::CutContourFailed));
        assert_eq!(polys_on(&rd::parse(&out.text).unwrap(), "15").len(), 1);
        assert!(export_hpgl(&scene, &page()).is_ok());
        assert!(export_pdf(&[scene]).is_ok());
        c.warnings += 1;
    } else {
        assert!(scene.cut_warnings.is_empty());
        assert_eq!(doc.with_role("couture").len(), 8);
    }
    c.impossible += 1;
}

// ---------------------------------------------------------------------------------------------

fn sequence(i: u64, c: &mut Counters) {
    let mut rng = Rng(SEED ^ i.wrapping_mul(0xA24B_AED4_963E_E407));
    let mode = MODES[rng.below(3) as usize];
    match i % 9 {
        // Quart de disque à rempli (arc, crans).
        0 | 1 => {
            let r = rng.grid(30.0, 90.0, 0.5);
            let v = rng.grid(1.0, 20.0, 0.5).max(1.0);
            let n = 1 + rng.below(6) as u32;
            check_quarter(c, r, v, n, mode);
        }
        // Coupe impossible.
        2 => check_impossible(c, mode),
        // Sans marge enregistrée.
        3 => {
            let (x0, y0) = (rng.grid(-150.0, 150.0, 0.5), rng.grid(-150.0, 150.0, 0.5));
            let (w, h) = (rng.grid(30.0, 100.0, 0.5), rng.grid(30.0, 100.0, 0.5));
            check_rect(c, x0, y0, w, h, None, mode);
        }
        // Marges toutes nulles, mais enregistrées : la coupe suit la couture.
        4 => {
            let (x0, y0) = (rng.grid(-150.0, 150.0, 0.5), rng.grid(-150.0, 150.0, 0.5));
            let (w, h) = (rng.grid(30.0, 100.0, 0.5), rng.grid(30.0, 100.0, 0.5));
            check_rect(c, x0, y0, w, h, Some([0.0; 4]), mode);
        }
        // Rectangle à marges par bord.
        _ => {
            let (x0, y0) = (rng.grid(-150.0, 150.0, 0.5), rng.grid(-150.0, 150.0, 0.5));
            let (w, h) = (rng.grid(30.0, 100.0, 0.5), rng.grid(30.0, 100.0, 0.5));
            let mut m = [0.0; 4];
            for v in &mut m {
                // 25 % de bords sans marge.
                *v = if rng.below(4) == 0 {
                    0.0
                } else {
                    rng.grid(0.5, 40.0, 0.25).max(0.5)
                };
            }
            check_rect(c, x0, y0, w, h, Some(m), mode);
        }
    }
}

#[test]
fn campagne_adverse_export_couture_et_coupe() {
    let mut c = Counters::default();
    for i in 0..SEQUENCES {
        let result = catch_unwind(AssertUnwindSafe(|| {
            let mut local = Counters::default();
            sequence(i, &mut local);
            local
        }));
        match result {
            Ok(local) => {
                c.rect_margins += local.rect_margins;
                c.rect_zero_margins += local.rect_zero_margins;
                c.no_seam += local.no_seam;
                c.quarter += local.quarter;
                c.impossible += local.impossible;
                c.svg_vertices += local.svg_vertices;
                c.svg_modes += local.svg_modes;
                c.pdf_spans += local.pdf_spans;
                c.pdf_dashes += local.pdf_dashes;
                c.dxf_areas += local.dxf_areas;
                c.dxf_layers += local.dxf_layers;
                c.hpgl_lengths += local.hpgl_lengths;
                c.notch_radii += local.notch_radii;
                c.notch_counts += local.notch_counts;
                c.same_bytes_modes += local.same_bytes_modes;
                c.sewing_only_equals_before += local.sewing_only_equals_before;
                c.warnings += local.warnings;
                c.determinism += local.determinism;
            }
            Err(e) => {
                let msg = e
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
                    .unwrap_or_default();
                panic!("séquence {i} : {msg}");
            }
        }
    }
    eprintln!("{c:#?}");
    // Les contrôles forts se sont réellement exécutés.
    assert_eq!(
        c.rect_margins + c.rect_zero_margins + c.no_seam + c.quarter + c.impossible,
        SEQUENCES as usize
    );
    assert!(c.rect_margins >= 60, "{c:?}");
    assert!(c.rect_zero_margins >= 12 && c.no_seam >= 12, "{c:?}");
    assert!(c.quarter >= 30 && c.impossible >= 12, "{c:?}");
    assert!(c.svg_vertices >= 40 && c.svg_modes >= 70, "{c:?}");
    assert!(c.pdf_spans >= 40 && c.pdf_dashes >= 70, "{c:?}");
    assert!(c.dxf_areas >= 70 && c.dxf_layers >= 70, "{c:?}");
    assert!(c.hpgl_lengths >= 100, "{c:?}");
    assert!(c.notch_radii >= 40 && c.notch_counts >= 12, "{c:?}");
    assert!(c.same_bytes_modes >= 36, "{c:?}");
    assert!(c.sewing_only_equals_before >= 10, "{c:?}");
    assert!(c.warnings >= 5, "{c:?}");
    assert!(c.determinism >= 70, "{c:?}");
}
