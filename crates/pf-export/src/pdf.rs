//! Export PDF vectoriel à l'échelle 1:1 (A4 par défaut ; autres formats : `page.rs`).
//!
//! Le PDF est écrit directement (aucune dépendance) : pages de la taille choisie, flux non
//! compressés, coordonnées absolues en points (`pt = mm × 72 / 25,4`), sans
//! matrice de transformation. Chaque cote du fichier se relit donc telle
//! quelle — c'est ce que font les tests.
//!
//! Texte (US-06.6) : police **Noto Sans embarquée** (`font.rs`, sous-ensemble des glyphes
//! écrits, `CIDFontType2` / `Identity-H`, `ToUnicode`) ; les flux de police sont en ASCII
//! hexadécimal (le fichier reste de l'ASCII pur) et ouverts par `stream` + CR LF.
//!
//! Structure : une page étalon (carré de 100 mm, règle, consignes, légende ; une page de plus
//! si la légende ne tient pas dans la zone imprimable, quel que soit le réglage), puis,
//! si l'option est active (US-13.2), **une page de plan d'assemblage** (miniature, pas à
//! l'échelle 1:1, toujours après l'étalon et avant les pages de patron), puis les feuilles
//! découpées en tuiles avec recouvrement (10 mm par défaut ; une seule page continue pour un
//! rouleau).

use crate::assembly::{rotate_quarter, scene_prims, Grid, Prim};
use crate::calibration::PrintScale;
use crate::content::{OutputCategory, OutputContent};
use crate::page::{PageLayout, PageSetup, PageSetupError, DEFAULT_OVERLAP_MM, ROLL_LENGTH_MAX_MM};
use crate::{
    num, stroke_style, ExportError, PathGeom, Scene, A4_HEIGHT_MM, A4_WIDTH_MM, CALIBRATION_MM,
    MM_TO_PT,
};
use pf_core::PathRole;
use pf_geometry::{Bbox, PointMm};
use pf_i18n::Lang;
use std::collections::BTreeSet;

/// Marge de sécurité de l'imprimante sur chaque bord (réglage par défaut).
pub const PAGE_MARGIN_MM: f64 = 10.0;
/// Bandeau de titre en haut de la zone imprimable.
pub const HEADER_MM: f64 = 8.0;
/// Recouvrement par défaut entre deux pages voisines (réglable : `PageSetup::overlap_mm`).
pub const OVERLAP_MM: f64 = DEFAULT_OVERLAP_MM;
/// Blanc ajouté autour du contenu avant découpage.
pub const CONTENT_PAD_MM: f64 = 5.0;
/// Fenêtre de patron visible sur une page A4 aux marges par défaut.
pub const WINDOW_W_MM: f64 = A4_WIDTH_MM - 2.0 * PAGE_MARGIN_MM;
pub const WINDOW_H_MM: f64 = A4_HEIGHT_MM - 2.0 * PAGE_MARGIN_MM - HEADER_MM;
/// Au-delà, l'export est refusé (jamais tronqué).
pub const MAX_PAGES: usize = 80;
/// Plus grande grille examinée avec « économiser le papier » (les pages vides y sont omises,
/// seules les pages imprimées comptent pour `MAX_PAGES`) ; au-delà, refus sans examen.
pub const MAX_GRID_CELLS: usize = 2000;

/// Une page du découpage : fenêtre du patron imprimée sur cette page.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tile {
    /// Colonne et ligne dans la grille d'origine (0 = L1C1, en haut à gauche) : elles restent
    /// celles de la grille quand des pages vides sont omises (« économiser le papier »).
    pub col: usize,
    pub row: usize,
    /// Taille de la grille d'origine (pages omises comprises).
    pub cols: usize,
    pub rows: usize,
    /// Coin bas-gauche de la fenêtre, en coordonnées du patron (mm), **dans le repère de la
    /// feuille tournée** de `turns` quarts de tour **puis mise à l'échelle** de `scale` (US-13.3 :
    /// les fenêtres de page, le recouvrement et les repères sont des mm de papier ; sans
    /// correction ce repère est celui du patron tourné).
    pub world_min: PointMm,
    /// Page de papier portant cette fenêtre (taille, marges, taille de la fenêtre).
    pub layout: PageLayout,
    /// Quarts de tour appliqués à la feuille à l'export (0 à 3, sens trigonométrique) ; le
    /// modèle n'est jamais modifié (US-13.2).
    pub turns: u8,
    /// Recouvrement avec les pages voisines (mm de papier).
    pub overlap_mm: f64,
    /// Correction d'échelle de l'imprimante appliquée au patron (US-13.3) ; `1,0 / 1,0` = aucune.
    pub scale: PrintScale,
}

impl Tile {
    /// Position sur la page (mm depuis le coin bas-gauche) d'un point du patron : quart de
    /// tour, puis correction d'échelle de l'imprimante.
    pub fn to_page_mm(&self, p: PointMm) -> PointMm {
        let r = rotate_quarter(p, self.turns);
        self.frame_to_page_mm(PointMm::new(r.x * self.scale.x, r.y * self.scale.y))
    }

    /// Position sur la page d'un point du repère de la feuille tournée et corrigée (celui de
    /// `world_min`).
    pub fn frame_to_page_mm(&self, f: PointMm) -> PointMm {
        PointMm::new(
            self.layout.margins.left_mm + (f.x - self.world_min.x),
            self.layout.margins.bottom_mm + (f.y - self.world_min.y),
        )
    }

    /// Identifiant lisible : « L1C2 » (ligne 1, colonne 2 de la grille d'origine).
    pub fn id(&self) -> String {
        format!("L{}C{}", self.row + 1, self.col + 1)
    }
}

fn tiles_needed(length: f64, window: f64, overlap: f64) -> usize {
    if length <= window {
        1
    } else {
        ((length - overlap) / (window - overlap) - 1e-9)
            .ceil()
            .max(1.0) as usize
    }
}

fn rotated_bbox(bb: &Bbox, turns: u8) -> Bbox {
    let mut r = Bbox::from_point(rotate_quarter(bb.min, turns));
    r.include(rotate_quarter(bb.max, turns));
    r
}

/// Emprise dans le repère de la page : feuille tournée puis corrigée (facteurs > 0).
fn frame_bbox(bb: &Bbox, turns: u8, scale: PrintScale) -> Bbox {
    let r = rotated_bbox(bb, turns);
    let mut s = Bbox::from_point(PointMm::new(r.min.x * scale.x, r.min.y * scale.y));
    s.include(PointMm::new(r.max.x * scale.x, r.max.y * scale.y));
    s
}

/// Grille de pages d'une emprise (dans le repère de la feuille tournée). `prims` : éléments
/// imprimés dans ce repère ; les cases qu'aucun ne touche sont omises (numérotation conservée).
fn grid_tiles(
    frame: &Bbox,
    layout: PageLayout,
    overlap: f64,
    turns: u8,
    scale: PrintScale,
    prims: Option<&[Prim]>,
) -> Result<Vec<Tile>, ExportError> {
    let (w, h) = (
        frame.width() + 2.0 * CONTENT_PAD_MM,
        frame.height() + 2.0 * CONTENT_PAD_MM,
    );
    let x0 = frame.min.x - CONTENT_PAD_MM;
    let top = frame.max.y + CONTENT_PAD_MM;
    let (window_w, window_h) = (layout.window_w_mm, layout.window_h_mm);
    let (cols, rows) = (
        tiles_needed(w, window_w, overlap),
        tiles_needed(h, window_h, overlap),
    );
    let cells = cols.saturating_mul(rows);
    let cap = if prims.is_some() {
        MAX_GRID_CELLS
    } else {
        MAX_PAGES
    };
    if cells > cap {
        return Err(ExportError::TooManyPages {
            pages: cells,
            max: cap,
        });
    }
    let grid = Grid {
        x0,
        top,
        sx: window_w - overlap,
        sy: window_h - overlap,
        ww: window_w,
        wh: window_h,
        cols,
        rows,
    };
    let occupied = prims.map(|p| grid.occupied(p));
    let mut tiles = Vec::with_capacity(cells);
    for row in 0..rows {
        for col in 0..cols {
            if occupied.as_ref().is_some_and(|o| !o[row * cols + col]) {
                continue;
            }
            tiles.push(Tile {
                col,
                row,
                cols,
                rows,
                world_min: PointMm::new(
                    x0 + col as f64 * (window_w - overlap),
                    top - window_h - row as f64 * (window_h - overlap),
                ),
                layout,
                turns,
                overlap_mm: overlap,
                scale,
            });
        }
    }
    if tiles.is_empty() {
        // Ne devrait jamais arriver (l'emprise est couverte) : garder une page plutôt que rien.
        tiles.push(Tile {
            col: 0,
            row: 0,
            cols,
            rows,
            world_min: PointMm::new(x0, top - window_h),
            layout,
            turns,
            overlap_mm: overlap,
            scale,
        });
    }
    if tiles.len() > MAX_PAGES {
        return Err(ExportError::TooManyPages {
            pages: tiles.len(),
            max: MAX_PAGES,
        });
    }
    Ok(tiles)
}

/// Découpage déterministe d'une scène en pages A4 aux marges par défaut, ligne par ligne depuis
/// le haut-gauche (L1C1). Aucune mise à l'échelle n'est jamais appliquée.
pub fn plan_tiles(scene: &Scene) -> Result<Vec<Tile>, ExportError> {
    plan_tiles_with(scene, &PageSetup::default())
}

/// Découpage d'une scène selon un réglage de page. Un rouleau donne une seule tuile (refus si
/// le contenu est plus large que la zone imprimable ou plus long que le maximum).
pub fn plan_tiles_with(scene: &Scene, setup: &PageSetup) -> Result<Vec<Tile>, ExportError> {
    plan_tiles_turns(scene, setup, true)
}

/// Même découpage que [`plan_tiles_with`] mais **sans rotation** de la feuille (HPGL tuilé,
/// PH-5e : le tracé n'est jamais tourné) ; « économiser le papier » omet toujours les pages vides.
pub fn plan_tiles_unrotated(scene: &Scene, setup: &PageSetup) -> Result<Vec<Tile>, ExportError> {
    plan_tiles_turns(scene, setup, false)
}

fn plan_tiles_turns(
    scene: &Scene,
    setup: &PageSetup,
    rotate: bool,
) -> Result<Vec<Tile>, ExportError> {
    setup.validate()?;
    scene.check_finite()?;
    let bb = scene
        .bbox()
        .ok_or_else(|| ExportError::EmptySheet(scene.sheet_name.clone()))?;
    let scale = setup.scale;
    if setup.format.is_roll() {
        let fb = frame_bbox(&bb, 0, scale);
        let (w, h) = (
            fb.width() + 2.0 * CONTENT_PAD_MM,
            fb.height() + 2.0 * CONTENT_PAD_MM,
        );
        let x0 = fb.min.x - CONTENT_PAD_MM;
        let top = fb.max.y + CONTENT_PAD_MM;
        let layout = setup.layout(h);
        if w > layout.window_w_mm {
            return Err(PageSetupError::ContentTooWide {
                content_mm: w,
                printable_mm: layout.window_w_mm,
            }
            .into());
        }
        if layout.window_h_mm > ROLL_LENGTH_MAX_MM {
            return Err(PageSetupError::RollTooLong {
                length_mm: layout.window_h_mm,
            }
            .into());
        }
        return Ok(vec![Tile {
            col: 0,
            row: 0,
            cols: 1,
            rows: 1,
            world_min: PointMm::new(x0, top - layout.window_h_mm),
            layout,
            turns: 0,
            overlap_mm: setup.overlap_mm,
            scale,
        }]);
    }
    let layout = setup.layout(0.0);
    let overlap = setup.overlap_mm;
    if !setup.assembly.save_paper {
        return grid_tiles(&frame_bbox(&bb, 0, scale), layout, overlap, 0, scale, None);
    }
    // Économiser le papier : pages vides omises ; la feuille est tournée (à l'export seulement)
    // quand cela réduit strictement le nombre de pages et qu'aucun droit-fil ne l'interdit.
    let prims = scene_prims(scene);
    let allowed = if rotate {
        scene.allowed_quarter_turns()
    } else {
        [true, false, false, false]
    };
    let mut best: Option<Result<Vec<Tile>, ExportError>> = None;
    for turns in 0..4u8 {
        if !allowed[usize::from(turns)] {
            continue;
        }
        let rotated: Vec<Prim> = prims
            .iter()
            .map(|p| p.rotated(turns).scaled(scale.x, scale.y))
            .collect();
        let r = grid_tiles(
            &frame_bbox(&bb, turns, scale),
            layout,
            overlap,
            turns,
            scale,
            Some(&rotated),
        );
        best = match (best, r) {
            (None, r) => Some(r),
            (Some(Err(_)), Ok(t)) => Some(Ok(t)),
            (Some(Ok(b)), Ok(t)) if t.len() < b.len() => Some(Ok(t)),
            (b, _) => b,
        };
    }
    best.expect("le quart de tour 0 est toujours permis")
}

/// Nombre total de pages du PDF A4 par défaut (page étalon comprise), connu avant l'export.
pub fn page_count(scenes: &[Scene]) -> Result<usize, ExportError> {
    page_count_with(scenes, &PageSetup::default())
}

/// Nombre total de pages du PDF pour un réglage de page (pages d'étalon comprises).
pub fn page_count_with(scenes: &[Scene], setup: &PageSetup) -> Result<usize, ExportError> {
    page_count_content(scenes, setup, &OutputContent::default())
}

/// Nombre total de pages pour un réglage de page et un contenu de sortie (US-13.4) : `scenes`
/// est déjà filtrée par `content`, qui sert encore à la légende de l'étalon (sa longueur peut
/// changer le nombre de pages d'étalon).
pub fn page_count_content(
    scenes: &[Scene],
    setup: &PageSetup,
    content: &OutputContent,
) -> Result<usize, ExportError> {
    Ok(plan_document(scenes, setup, content)?.total)
}

#[derive(Debug, Clone, PartialEq)]
pub struct PdfExport {
    pub bytes: Vec<u8>,
    pub page_count: usize,
    /// Taille (largeur, hauteur) en mm de chaque page, dans l'ordre du fichier.
    pub page_sizes_mm: Vec<(f64, f64)>,
}

struct DocPlan {
    plans: Vec<Vec<Tile>>,
    /// 1 si une page de plan d'assemblage est insérée après l'étalon, sinon 0.
    plan_pages: usize,
    total: usize,
}

/// Une page de plan n'a de sens que pour des feuilles découpées en pages (pas un rouleau).
fn plan_page_count(scenes: &[Scene], setup: &PageSetup) -> usize {
    usize::from(setup.assembly.plan && !setup.format.is_roll() && !scenes.is_empty())
}

fn plan_document(
    scenes: &[Scene],
    setup: &PageSetup,
    content: &OutputContent,
) -> Result<DocPlan, ExportError> {
    setup.validate()?;
    let plans: Vec<Vec<Tile>> = scenes
        .iter()
        .map(|s| plan_tiles_with(s, setup))
        .collect::<Result<_, _>>()?;
    let tiles: usize = plans.iter().map(Vec::len).sum();
    let plan_pages = plan_page_count(scenes, setup);
    let total = calibration_pages(scenes, &plans, 0, setup, content, Lang::Fr)
        .pages
        .len()
        + plan_pages
        + tiles;
    if total > MAX_PAGES {
        return Err(ExportError::TooManyPages {
            pages: total,
            max: MAX_PAGES,
        });
    }
    Ok(DocPlan {
        plans,
        plan_pages,
        total,
    })
}

/// PDF complet en A4 par défaut : page étalon puis toutes les scènes, tuilées.
pub fn export_pdf(scenes: &[Scene]) -> Result<PdfExport, ExportError> {
    export_pdf_with(scenes, &PageSetup::default())
}

/// PDF complet pour un réglage de page : page(s) étalon puis toutes les scènes, tuilées.
/// L'échelle est toujours 1:1 ; un réglage invalide est refusé avant toute écriture.
pub fn export_pdf_with(scenes: &[Scene], setup: &PageSetup) -> Result<PdfExport, ExportError> {
    export_pdf_content(scenes, setup, &OutputContent::default())
}

/// PDF pour un réglage de page et un contenu de sortie (US-13.4). `scenes` est déjà filtrée
/// (`OutputContent::scene`) ; `content` règle la légende de l'étalon, qui n'annonce pas un rôle
/// exclu. Avec le contenu par défaut, identique à l'octet à `export_pdf_with`.
pub fn export_pdf_content(
    scenes: &[Scene],
    setup: &PageSetup,
    content: &OutputContent,
) -> Result<PdfExport, ExportError> {
    export_pdf_lang(scenes, setup, content, Lang::Fr)
}

/// Comme [`export_pdf_content`], avec les textes écrits (étalon, légende, bandeaux, plan) dans la
/// langue `lang` (US-19.2). En français, identique à l'octet à `export_pdf_content`.
pub fn export_pdf_lang(
    scenes: &[Scene],
    setup: &PageSetup,
    content: &OutputContent,
    lang: Lang,
) -> Result<PdfExport, ExportError> {
    let DocPlan {
        plans,
        plan_pages,
        total,
    } = plan_document(scenes, setup, content)?;
    let cal = calibration_pages(scenes, &plans, total, setup, content, lang);
    let mut pages: Vec<PdfPage> = cal
        .pages
        .into_iter()
        .map(|content| PdfPage {
            content,
            w_mm: cal.page_w_mm,
            h_mm: cal.page_h_mm,
        })
        .collect();
    if plan_pages > 0 {
        let n = pages.len() + 1;
        pages.push(PdfPage {
            content: plan_page(scenes, &plans, setup, n, total, lang),
            w_mm: setup.page_width_mm(),
            h_mm: setup.page_height_mm().unwrap_or(cal.page_h_mm),
        });
    }
    let mut n = pages.len();
    for (scene, tiles) in scenes.iter().zip(&plans) {
        let ctx = TileCtx {
            lang,
            marks: setup.assembly.marks,
            control: setup.shows_control_marks(),
            tiles,
            base: n,
        };
        for tile in tiles {
            n += 1;
            pages.push(PdfPage {
                content: tile_page(scene, tile, n, total, &ctx),
                w_mm: tile.layout.page_w_mm,
                h_mm: tile.layout.page_h_mm,
            });
        }
    }
    let title = match scenes.first() {
        Some(s) => tl(lang, "pdfx.title.doc", &[("project", &s.project_name)]),
        None => tl(lang, "pdfx.title.calib", &[]),
    };
    Ok(PdfExport {
        bytes: assemble(&pages, &title),
        page_count: total,
        page_sizes_mm: pages.iter().map(|p| (p.w_mm, p.h_mm)).collect(),
    })
}

/// PDF d'une seule page : le carré étalon de 100 mm.
pub fn export_calibration_pdf() -> PdfExport {
    export_pdf(&[]).expect("la page étalon seule est toujours exportable")
}

/// Page étalon seule pour un réglage de page (la légende est vide : une seule page).
pub fn export_calibration_pdf_with(setup: &PageSetup) -> Result<PdfExport, ExportError> {
    export_pdf_with(&[], setup)
}

/// Page étalon seule, textes dans la langue `lang` (US-19.2).
pub fn export_calibration_pdf_lang(
    setup: &PageSetup,
    lang: Lang,
) -> Result<PdfExport, ExportError> {
    export_pdf_lang(&[], setup, &OutputContent::default(), lang)
}

pub(crate) struct PdfPage {
    pub(crate) content: String,
    pub(crate) w_mm: f64,
    pub(crate) h_mm: f64,
}

// ---------------------------------------------------------------------------
// Flux de contenu
// ---------------------------------------------------------------------------

pub(crate) struct Content(pub(crate) String);

fn pt(mm: f64) -> String {
    num(mm * MM_TO_PT)
}

impl Content {
    pub(crate) fn new() -> Self {
        Content(String::new())
    }
    fn op(&mut self, s: &str) {
        self.0.push_str(s);
        self.0.push('\n');
    }
    /// Couleur (niveau de gris), épaisseur (mm) et pointillé (mm) du trait.
    pub(crate) fn stroke(&mut self, gray: f64, width_mm: f64, dash_mm: &[f64]) {
        let dash: Vec<String> = dash_mm.iter().map(|d| pt(*d)).collect();
        self.op(&format!(
            "{} G {} w [{}] 0 d",
            num(gray),
            pt(width_mm),
            dash.join(" ")
        ));
    }
    pub(crate) fn line(&mut self, a: PointMm, b: PointMm) {
        self.op(&format!(
            "{} {} m {} {} l S",
            pt(a.x),
            pt(a.y),
            pt(b.x),
            pt(b.y)
        ));
    }
    fn cubic(&mut self, p: [PointMm; 4]) {
        self.op(&format!(
            "{} {} m {} {} {} {} {} {} c S",
            pt(p[0].x),
            pt(p[0].y),
            pt(p[1].x),
            pt(p[1].y),
            pt(p[2].x),
            pt(p[2].y),
            pt(p[3].x),
            pt(p[3].y)
        ));
    }
    /// Chaîne fermée d'arêtes (segments, arcs, Béziers) tracée comme un seul chemin : un seul
    /// pointillé qui court tout le contour (US-05.4). `to_page` convertit un point en mm.
    fn closed_chain(
        &mut self,
        edges: &[PathGeom],
        closed: bool,
        to_page: impl Fn(PointMm) -> PointMm,
    ) {
        let Some(first) = edges.first() else {
            return;
        };
        let start = match first {
            PathGeom::Line(a, _) => *a,
            PathGeom::Cubic(k) => k.p0,
            PathGeom::Arc(a) => a.start_point(),
            PathGeom::Circle(_) | PathGeom::Spline(_) => return,
        };
        let s0 = to_page(start);
        let mut s = format!("{} {} m", pt(s0.x), pt(s0.y));
        let curve = |s: &mut String, k: &pf_geometry::CubicBezier| {
            let (c1, c2, p3) = (to_page(k.c1), to_page(k.c2), to_page(k.p3));
            s.push_str(&format!(
                " {} {} {} {} {} {} c",
                pt(c1.x),
                pt(c1.y),
                pt(c2.x),
                pt(c2.y),
                pt(p3.x),
                pt(p3.y)
            ));
        };
        for e in edges {
            match e {
                PathGeom::Line(_, b) => {
                    let b = to_page(*b);
                    s.push_str(&format!(" {} {} l", pt(b.x), pt(b.y)));
                }
                PathGeom::Cubic(k) => curve(&mut s, k),
                PathGeom::Arc(_) => {
                    for k in e.arc_beziers() {
                        curve(&mut s, &k);
                    }
                }
                PathGeom::Circle(_) | PathGeom::Spline(_) => {}
            }
        }
        s.push_str(if closed { " h S" } else { " S" });
        self.op(&s);
    }
    /// Suite continue de Béziers tracée comme un seul chemin (un seul pointillé).
    fn beziers(&mut self, curves: &[[PointMm; 4]]) {
        let Some(first) = curves.first() else {
            return;
        };
        let mut s = format!("{} {} m", pt(first[0].x), pt(first[0].y));
        for k in curves {
            s.push_str(&format!(
                " {} {} {} {} {} {} c",
                pt(k[1].x),
                pt(k[1].y),
                pt(k[2].x),
                pt(k[2].y),
                pt(k[3].x),
                pt(k[3].y)
            ));
        }
        s.push_str(" S");
        self.op(&s);
    }
    /// Chemin fermé rempli en noir (disque d'un pointage).
    fn fill_beziers(&mut self, curves: &[[PointMm; 4]]) {
        let Some(first) = curves.first() else {
            return;
        };
        let mut s = format!("{} {} m", pt(first[0].x), pt(first[0].y));
        for k in curves {
            s.push_str(&format!(
                " {} {} {} {} {} {} c",
                pt(k[1].x),
                pt(k[1].y),
                pt(k[2].x),
                pt(k[2].y),
                pt(k[3].x),
                pt(k[3].y)
            ));
        }
        s.push_str(" h f");
        self.op(&s);
    }
    /// Contour fermé (fente de boutonnière) : droites et Béziers, tracé comme un seul chemin.
    fn closed_outline(&mut self, start: PointMm, segments: &[crate::SlotSegment]) {
        let mut s = format!("{} {} m", pt(start.x), pt(start.y));
        for seg in segments {
            match seg {
                crate::SlotSegment::Line(b) => s.push_str(&format!(" {} {} l", pt(b.x), pt(b.y))),
                crate::SlotSegment::Curve(k) => s.push_str(&format!(
                    " {} {} {} {} {} {} c",
                    pt(k.c1.x),
                    pt(k.c1.y),
                    pt(k.c2.x),
                    pt(k.c2.y),
                    pt(k.p3.x),
                    pt(k.p3.y)
                )),
            }
        }
        s.push_str(" h S");
        self.op(&s);
    }
    fn rect(&mut self, x: f64, y: f64, w: f64, h: f64) {
        self.op(&format!("{} {} {} {} re S", pt(x), pt(y), pt(w), pt(h)));
    }
    /// Texte orienté de `angle_deg` (sens trigonométrique) : matrice de texte `Tm`.
    fn text_rotated(&mut self, x_mm: f64, y_mm: f64, size_pt: f64, angle_deg: f64, text: &str) {
        let (s, c) = pf_geometry::sin_cos_deg(angle_deg);
        self.op(&format!(
            "BT /F1 {} Tf 0 g {} {} {} {} {} {} Tm {} Tj ET",
            num(size_pt),
            num(c),
            num(s),
            num(-s),
            num(c),
            pt(x_mm),
            pt(y_mm),
            hex_cids(text)
        ));
    }
    pub(crate) fn text(&mut self, x_mm: f64, y_mm: f64, size_pt: f64, text: &str) {
        self.op(&format!(
            "BT /F1 {} Tf 0 g {} {} Td {} Tj ET",
            num(size_pt),
            pt(x_mm),
            pt(y_mm),
            hex_cids(text)
        ));
    }
}

/// Texte écrit dans le flux : chaîne hexadécimale de CID sur 2 octets (`Identity-H`), un par
/// caractère. Un caractère absent de la police embarquée est écrit « ? » (jamais de `panic`).
fn hex_cids(text: &str) -> String {
    let face = crate::font::embedded();
    let mut out = String::with_capacity(2 + 4 * text.len());
    out.push('<');
    for ch in text.chars() {
        out.push_str(&format!("{:04X}", face.cid(ch)));
    }
    out.push('>');
    out
}

/// CID écrits dans les opérations de texte d'un flux de page (`<hex> Tj ET`).
fn used_cids(content: &str, used: &mut BTreeSet<u16>) {
    for line in content.lines().filter(|l| l.ends_with(" Tj ET")) {
        let (Some(a), Some(b)) = (line.rfind('<'), line.rfind('>')) else {
            continue;
        };
        let hex = &line.as_bytes()[(a + 1).min(b)..b];
        for q in hex.chunks_exact(4) {
            if let Some(v) = std::str::from_utf8(q)
                .ok()
                .and_then(|h| u16::from_str_radix(h, 16).ok())
            {
                used.insert(v);
            }
        }
    }
}

/// Chaîne de texte PDF (titre) : littérale si ASCII imprimable, sinon UTF-16BE avec BOM.
fn pdf_text_string(text: &str) -> String {
    if text.chars().all(|c| (' '..='~').contains(&c)) {
        let mut out = String::from("(");
        for ch in text.chars() {
            if matches!(ch, '(' | ')' | '\\') {
                out.push('\\');
            }
            out.push(ch);
        }
        out.push(')');
        out
    } else {
        let mut out = String::from("<FEFF");
        for u in text.encode_utf16() {
            out.push_str(&format!("{u:04X}"));
        }
        out.push('>');
        out
    }
}

fn shorten(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_string()
    } else {
        let s: String = text.chars().take(max.saturating_sub(1)).collect();
        format!("{s}…")
    }
}

/// Texte d'une clé `pdfx.*` dans `lang`, `{nom}` remplacés par `args`.
pub(crate) fn tl(lang: Lang, key: &str, args: &[(&str, &str)]) -> String {
    let mut s = pf_i18n::lookup(lang, key);
    for (n, v) in args {
        s = s.replace(&format!("{{{n}}}"), v);
    }
    s
}

/// Clé de légende d'un rôle de trait.
pub(crate) fn role_key(role: PathRole) -> &'static str {
    match role {
        PathRole::Construction => "pdfx.role.construction",
        PathRole::SewingContour => "pdfx.role.sewing_contour",
        PathRole::CuttingContour => "pdfx.role.cutting_contour",
        PathRole::Grainline => "pdfx.role.grainline",
        PathRole::Fold => "pdfx.role.fold",
        PathRole::AnnotationAxis => "pdfx.role.annotation_axis",
        PathRole::Topstitch => "pdfx.role.topstitch",
        PathRole::CenterFront => "pdfx.role.center_front",
        PathRole::CenterBack => "pdfx.role.center_back",
        PathRole::WaistLine => "pdfx.role.waist_line",
        PathRole::HipLine => "pdfx.role.hip_line",
        PathRole::BustLine => "pdfx.role.bust_line",
        PathRole::SkivingLine => "pdfx.role.skiving_line",
        PathRole::GlueLine => "pdfx.role.glue_line",
    }
}

fn role_description(role: PathRole, lang: Lang) -> String {
    tl(lang, role_key(role), &[])
}

struct Calibration {
    pages: Vec<String>,
    page_w_mm: f64,
    page_h_mm: f64,
}

/// Réserve sous la dernière ligne de la légende avant de passer à une page de suite.
const CAL_BOTTOM_SLACK_MM: f64 = 1.0;

/// Pose la légende de haut en bas ; quand une ligne ne tient plus dans la zone imprimable
/// (réglage non par défaut seulement), elle passe sur une page de suite.
struct Flow {
    lang: Lang,
    pages: Vec<Content>,
    y: f64,
    ytop: f64,
    left: f64,
    bottom: f64,
    wrap: bool,
    min_y: f64,
}

impl Flow {
    fn c(&mut self) -> &mut Content {
        self.pages.last_mut().expect("au moins une page")
    }
    /// Descend de `dy` ; `reserve` = place nécessaire plus bas pour la même ligne.
    fn step(&mut self, dy: f64, reserve: f64) {
        self.y -= dy;
        if self.wrap && self.y - reserve < self.bottom + CAL_BOTTOM_SLACK_MM {
            let (left, ytop) = (self.left, self.ytop);
            let lang = self.lang;
            let mut page = Content::new();
            page.text(left, ytop - 10.0, 12.0, &tl(lang, "pdfx.calib.cont", &[]));
            self.pages.push(page);
            self.y = ytop - 22.0;
        }
        self.min_y = self.min_y.min(self.y - reserve);
    }
}

/// Page(s) étalon : carré de 100 mm, règle, consignes, légende. Aux réglages par défaut,
/// identique à l'octet à l'ancienne page étalon A4.
fn calibration_pages(
    scenes: &[Scene],
    plans: &[Vec<Tile>],
    total: usize,
    setup: &PageSetup,
    content: &OutputContent,
    lang: Lang,
) -> Calibration {
    let page_w = setup.page_width_mm();
    match setup.page_height_mm() {
        Some(page_h) => {
            let (pages, _) =
                draw_calibration(scenes, plans, total, setup, content, lang, page_h, true);
            Calibration {
                pages,
                page_w_mm: page_w,
                page_h_mm: page_h,
            }
        }
        None => {
            // Rouleau : une seule page étalon, assez longue pour tout contenir. Première
            // passe sans limite (haut de page à 0) pour mesurer, seconde à la bonne hauteur.
            let m = setup.margins;
            let (_, min_y) =
                draw_calibration(scenes, plans, total, setup, content, lang, m.top_mm, false);
            let needed = m.top_mm - min_y + m.bottom_mm + CAL_BOTTOM_SLACK_MM;
            let step = crate::page::ROLL_LENGTH_STEP_MM;
            let page_h = (needed / step).ceil() * step;
            let (pages, _) =
                draw_calibration(scenes, plans, total, setup, content, lang, page_h, false);
            Calibration {
                pages,
                page_w_mm: page_w,
                page_h_mm: page_h,
            }
        }
    }
}

/// Dessine l'étalon sur une page de hauteur `page_h` ; rend les pages et la plus basse
/// ordonnée atteinte (mm).
#[allow(clippy::too_many_arguments)]
fn draw_calibration(
    scenes: &[Scene],
    plans: &[Vec<Tile>],
    total: usize,
    setup: &PageSetup,
    content: &OutputContent,
    lang: Lang,
    page_h: f64,
    wrap: bool,
) -> (Vec<String>, f64) {
    let m = setup.margins;
    // Haut et gauche de la zone imprimable : tout l'étalon est placé depuis ce coin, si bien
    // qu'en A4 aux marges de 10 mm les coordonnées sont celles de l'ancienne page.
    let ytop = page_h - m.top_mm;
    let printable_w = setup.page_width_mm() - m.left_mm - m.right_mm;
    let left = m.left_mm + 10.0;
    let mut c = Content::new();
    c.text(left, ytop - 10.0, 16.0, &tl(lang, "pdfx.title.calib", &[]));
    c.text(left, ytop - 18.0, 10.0, &tl(lang, "pdfx.calib.print", &[]));
    c.text(left, ytop - 23.0, 10.0, &tl(lang, "pdfx.calib.check", &[]));
    c.text(left, ytop - 28.0, 10.0, &tl(lang, "pdfx.calib.wrong", &[]));

    // Carré étalon (A4 par défaut : x 55 → 155 mm, y 150 → 250 mm), centré dans la zone
    // imprimable.
    let (sx, sy) = (
        m.left_mm + (printable_w - CALIBRATION_MM) / 2.0,
        ytop - 137.0,
    );
    // Correction d'échelle de l'imprimante (US-13.3) : le carré et sa règle sont **corrigés**
    // (autour de leur coin bas-gauche, texte inchangé) ; sans correction, `fx = fy = 1,0` et
    // chaque coordonnée est celle de l'ancienne page, à l'octet.
    let (fx, fy) = (setup.scale.x, setup.scale.y);
    c.stroke(0.0, 0.3, &[]);
    c.rect(sx, sy, CALIBRATION_MM * fx, CALIBRATION_MM * fy);
    c.text(sx + 40.0 * fx, sy + 48.0 * fy, 11.0, "100 × 100 mm");
    c.text(sx + 44.0 * fx, sy - 5.0, 9.0, "100 mm");

    // Règle graduée de 100 mm (corrigée comme le carré).
    let ry = if fy == 1.0 {
        ytop - 157.0
    } else {
        sy - 20.0 * fy
    };
    c.stroke(0.0, 0.2, &[]);
    c.line(
        PointMm::new(sx, ry),
        PointMm::new(sx + CALIBRATION_MM * fx, ry),
    );
    for i in 0..=100 {
        let h = if i % 10 == 0 {
            4.0
        } else if i % 5 == 0 {
            2.8
        } else {
            1.6
        };
        let x = sx + f64::from(i) * fx;
        c.line(PointMm::new(x, ry), PointMm::new(x, ry + h));
        if i % 10 == 0 {
            c.text(x - 1.2, ry - 4.0, 7.0, &i.to_string());
        }
    }
    c.text(sx + CALIBRATION_MM * fx + 6.0, ry - 4.0, 7.0, "mm");

    // Étalon renforcé (option ou correction active) : deux règles en mm papier **non corrigés**,
    // pour mesurer l'imprimante sans désactiver la correction.
    let control = setup.shows_control_marks();
    let (len_h, len_v) =
        control_ruler_lengths(printable_w, ytop, m.bottom_mm, setup.format.is_roll());
    let mut extra_bottom = f64::INFINITY;
    if control {
        let x_v = m.left_mm + printable_w - 10.0;
        let v_top = ytop - 36.0;
        c.stroke(0.0, 0.2, &[]);
        control_ruler(&mut c, PointMm::new(left, ytop - 172.0), true, len_h);
        control_ruler(&mut c, PointMm::new(x_v, v_top), false, len_v);
        extra_bottom = v_top - len_v - 4.0;
    }

    // Légende : le rôle d'un trait se lit sans couleur.
    let y0 = ytop - 175.0 - if control { 20.0 } else { 0.0 };
    let mut fl = Flow {
        lang,
        pages: vec![c],
        y: y0,
        ytop,
        left,
        bottom: m.bottom_mm,
        wrap,
        min_y: y0.min(extra_bottom),
    };
    fl.step(0.0, 0.0);
    let y = fl.y;
    fl.c()
        .text(left, y, 10.0, &tl(lang, "pdfx.legend.title", &[]));
    for role in PathRole::BASE {
        // Contenu de sortie (US-13.4) : un rôle exclu n'est pas annoncé (toujours inclus par
        // défaut : page étalon identique à l'octet).
        if !content.includes(OutputCategory::of_role(role)) {
            continue;
        }
        fl.step(6.0, 0.0);
        let y = fl.y;
        let c = fl.c();
        let st = stroke_style(role);
        c.stroke(0.0, st.width_mm, st.dash_mm);
        c.line(
            PointMm::new(left, y + 1.0),
            PointMm::new(left + 35.0, y + 1.0),
        );
        c.text(left + 40.0, y, 9.0, &role_description(role, lang));
    }
    // Lignes spéciales (US-06.8) : seulement celles qui servent, resserrées (la page étalon
    // reste identique à l'octet quand aucune n'est utilisée).
    for role in PathRole::SPECIAL {
        if !scenes
            .iter()
            .any(|s| s.paths.iter().any(|p| p.role == role))
        {
            continue;
        }
        fl.step(4.5, 0.0);
        let y = fl.y;
        let c = fl.c();
        let st = stroke_style(role);
        c.stroke(0.0, st.width_mm, st.dash_mm);
        c.line(
            PointMm::new(left, y + 1.0),
            PointMm::new(left + 35.0, y + 1.0),
        );
        c.text(left + 40.0, y, 8.0, &role_description(role, lang));
    }
    if content.includes(OutputCategory::Notches) {
        fl.step(6.0, 0.0);
        let y = fl.y;
        let c = fl.c();
        c.stroke(0.0, stroke_style(PathRole::CuttingContour).width_mm, &[]);
        c.line(
            PointMm::new(left + 17.5, y - 1.0),
            PointMm::new(left + 17.5, y + 4.0),
        );
        c.text(left + 40.0, y, 9.0, &tl(lang, "pdfx.legend.notch", &[]));
    }
    // Légende du sens du droit-fil (US-06.5) : seulement quand une ligne en porte un.
    if scenes.iter().any(|s| {
        s.paths
            .iter()
            .any(|p| p.sense != pf_core::GrainSense::Unmarked)
    }) {
        fl.step(6.0, 0.0);
        let y = fl.y;
        let c = fl.c();
        c.stroke(0.0, stroke_style(PathRole::Grainline).width_mm, &[]);
        let (a, b) = (
            PointMm::new(left + 5.0, y + 1.0),
            PointMm::new(left + 30.0, y + 1.0),
        );
        c.line(a, b);
        for (p, q) in crate::grain_arrow_marks(a, b, pf_core::GrainSense::Nap) {
            c.line(p, q);
        }
        c.text(
            left + 40.0,
            y,
            9.0,
            &tl(lang, "pdfx.legend.grain_sense", &[]),
        );
    }
    // Légende des pinces : seulement quand au moins une est écrite (même raison).
    if scenes.iter().any(|s| !s.darts.is_empty()) {
        fl.step(6.0, 0.0);
        let y = fl.y;
        let c = fl.c();
        c.stroke(0.0, crate::DART_STROKE_MM, crate::DART_DASH_MM);
        c.line(
            PointMm::new(left, y + 1.0),
            PointMm::new(left + 35.0, y + 1.0),
        );
        c.text(left + 40.0, y, 9.0, &tl(lang, "pdfx.legend.dart", &[]));
    }
    // Légende des marquages : seulement quand au moins un existe (sinon la page étalon
    // reste identique à l'octet).
    if scenes.iter().any(|s| !s.markings.is_empty()) {
        fl.step(6.0, 0.0);
        let y = fl.y;
        let c = fl.c();
        c.stroke(0.0, crate::MARKING_STROKE_MM, &[]);
        if let Ok(sample) =
            pf_geometry::CircleTrace::new(PointMm::new(left + 17.5, y + 1.5), 2.0, 0.0, true)
        {
            let curves: Vec<[PointMm; 4]> = PathGeom::Circle(sample)
                .arc_beziers()
                .iter()
                .map(|k| [k.p0, k.c1, k.c2, k.p3])
                .collect();
            c.beziers(&curves);
        }
        // Légende inchangée (octets) tant qu'aucune fente de poinçon n'est exportée.
        let with_slot = scenes
            .iter()
            .any(|s| s.markings.iter().any(|m| m.slot().is_some()));
        c.text(
            left + 40.0,
            y,
            9.0,
            &if with_slot {
                tl(lang, "pdfx.legend.marking_slot", &[])
            } else {
                tl(lang, "pdfx.legend.marking", &[])
            },
        );
    }
    // Légende des trous de piqûre : seulement quand au moins un existe (octets des exemples).
    if scenes.iter().any(|s| !s.stitch_holes.is_empty()) {
        fl.step(6.0, 0.0);
        let y = fl.y;
        let c = fl.c();
        c.stroke(0.0, crate::MARKING_STROKE_MM, &[]);
        if let Ok(sample) =
            pf_geometry::CircleTrace::new(PointMm::new(left + 17.5, y + 1.5), 1.0, 0.0, true)
        {
            let curves: Vec<[PointMm; 4]> = PathGeom::Circle(sample)
                .arc_beziers()
                .iter()
                .map(|k| [k.p0, k.c1, k.c2, k.p3])
                .collect();
            c.beziers(&curves);
        }
        c.text(
            left + 40.0,
            y,
            9.0,
            &tl(lang, "pdfx.legend.stitch_hole", &[]),
        );
    }
    // Légende des boutons : seulement quand au moins un existe (même raison).
    if scenes.iter().any(|s| !s.buttons.is_empty()) {
        fl.step(6.0, 0.0);
        let y = fl.y;
        let c = fl.c();
        c.stroke(0.0, crate::BUTTON_STROKE_MM, &[]);
        let sample = crate::SceneButton {
            pos: PointMm::new(left + 17.5, y + 1.5),
            shape: pf_core::ButtonShape::Buttonhole {
                length_mm: 9.0,
                width_mm: 3.0,
                angle_deg: 0.0,
            },
        };
        if let Some((start, segments)) = sample.slot_outline() {
            c.closed_outline(start, &segments);
        }
        c.text(left + 40.0, y, 9.0, &tl(lang, "pdfx.legend.button", &[]));
    }

    fl.step(12.0, 0.0);
    let y = fl.y;
    fl.c().text(
        left,
        y,
        10.0,
        &tl(
            lang,
            "pdfx.contents",
            &[
                ("total", &total.to_string()),
                ("setup", &setup.describe_lang(lang)),
            ],
        ),
    );
    for (scene, tiles) in scenes.iter().zip(plans).take(8) {
        fl.step(5.0, 0.0);
        let y = fl.y;
        let (cols, rows) = tiles.first().map(|t| (t.cols, t.rows)).unwrap_or((0, 0));
        let kind = if scene.is_construction_only() {
            tl(lang, "pdfx.sheet.kind", &[])
        } else {
            String::new()
        };
        // Pages vides omises et feuille tournée (US-13.2) : rien n'est écrit sans l'option.
        let omitted = (rows * cols).saturating_sub(tiles.len());
        let mut extra = String::new();
        if omitted > 0 {
            extra.push_str(&tl(
                lang,
                "pdfx.sheet.omitted",
                &[("n", &omitted.to_string())],
            ));
        }
        if let Some(t) = tiles.first().filter(|t| t.turns > 0) {
            extra.push_str(&tl(
                lang,
                "pdfx.sheet.turned",
                &[("deg", &(90 * u32::from(t.turns)).to_string())],
            ));
        }
        fl.c().text(
            left,
            y,
            9.0,
            &tl(
                lang,
                "pdfx.sheet.line",
                &[
                    ("name", &shorten(&scene.sheet_name, 40)),
                    ("pages", &tiles.len().to_string()),
                    ("rows", &rows.to_string()),
                    ("cols", &cols.to_string()),
                    ("extra", &extra),
                    ("kind", &kind),
                ],
            ),
        );
    }
    // Réglage de page : seulement quand il n'est pas celui par défaut (A4 portrait, 10 mm).
    if !setup.is_default() {
        fl.step(6.0, 0.0);
        let y = fl.y;
        fl.c().text(
            left,
            y,
            9.0,
            &tl(
                lang,
                "pdfx.paper",
                &[
                    ("setup", &setup.describe_lang(lang)),
                    ("top", &num(m.top_mm)),
                    ("bottom", &num(m.bottom_mm)),
                    ("left", &num(m.left_mm)),
                    ("right", &num(m.right_mm)),
                ],
            ),
        );
    }
    if !setup.scale.is_identity() {
        for line in [
            tl(
                lang,
                "pdfx.scale.active",
                &[
                    ("x", &factor_text(setup.scale.x)),
                    ("y", &factor_text(setup.scale.y)),
                ],
            ),
            tl(lang, "pdfx.scale.note", &[]),
        ] {
            fl.step(5.0, 0.0);
            let y = fl.y;
            fl.c().text(left, y, 8.0, &line);
        }
    }
    if control {
        fl.step(5.0, 0.0);
        let y = fl.y;
        fl.c().text(
            left,
            y,
            8.0,
            &tl(
                lang,
                "pdfx.control.rulers",
                &[("h", &num(len_h)), ("v", &num(len_v))],
            ),
        );
    }
    fl.step(8.0, 5.0);
    let y = fl.y;
    let assembly = if setup.format.is_roll() {
        tl(lang, "pdfx.assembly.roll", &[])
    } else {
        tl(
            lang,
            "pdfx.assembly.overlap",
            &[("ov", &num(setup.overlap_mm))],
        )
    };
    let c = fl.c();
    c.text(left, y, 9.0, &assembly);
    c.text(left, y - 5.0, 9.0, &tl(lang, "pdfx.experimental", &[]));
    let min_y = fl.min_y;
    (fl.pages.into_iter().map(|c| c.0).collect(), min_y)
}

/// Facteur de correction écrit sans zéros inutiles (6 décimales au plus).
fn factor_text(v: f64) -> String {
    let mut s = format!("{v:.6}");
    while s.contains('.') && (s.ends_with('0') || s.ends_with('.')) {
        s.pop();
    }
    s
}

/// Longueurs (mm) des règles de contrôle de la page étalon : horizontale et verticale, au plus
/// 200 mm, toujours d'au moins 100 mm (zone imprimable minimale 160 × 170 mm). Un rouleau a
/// une page assez longue pour 200 mm.
fn control_ruler_lengths(printable_w: f64, ytop: f64, bottom: f64, roll: bool) -> (f64, f64) {
    let tens = |v: f64| (v.min(200.0) / 10.0).floor() * 10.0;
    let v = if roll {
        200.0
    } else {
        tens(ytop - 36.0 - bottom - 8.0)
    };
    (tens(printable_w - 22.0), v)
}

/// Règle graduée de `len` mm (traits au mm, au 5 mm et au 10 mm, chiffres tous les 10 mm).
/// Horizontale : de `origin` vers la droite, graduations vers le haut, chiffres dessous.
/// Verticale : de `origin` vers le bas, graduations vers la gauche, chiffres à droite.
fn control_ruler(c: &mut Content, origin: PointMm, horizontal: bool, len: f64) {
    let whole = len as i32;
    let end = f64::from(whole);
    if horizontal {
        c.line(origin, PointMm::new(origin.x + end, origin.y));
    } else {
        c.line(origin, PointMm::new(origin.x, origin.y - end));
    }
    for i in 0..=whole {
        let h = if i % 10 == 0 {
            4.0
        } else if i % 5 == 0 {
            2.8
        } else {
            1.6
        };
        let d = f64::from(i);
        if horizontal {
            let x = origin.x + d;
            c.line(PointMm::new(x, origin.y), PointMm::new(x, origin.y + h));
            if i % 10 == 0 {
                c.text(x - 1.2, origin.y - 4.0, 7.0, &i.to_string());
            }
        } else {
            let y = origin.y - d;
            c.line(PointMm::new(origin.x, y), PointMm::new(origin.x - h, y));
            if i % 10 == 0 {
                c.text(origin.x + 1.5, y - 1.0, 7.0, &i.to_string());
            }
        }
    }
}

/// Contexte d'une page de patron : pages voisines de la même feuille et option de repères.
struct TileCtx<'a> {
    lang: Lang,
    marks: bool,
    /// Repères de contrôle de 100 mm (mm papier non corrigés, US-13.3).
    control: bool,
    /// Pages de la feuille (omises exclues), dans l'ordre du fichier.
    tiles: &'a [Tile],
    /// Nombre de pages du fichier avant la première page de cette feuille.
    base: usize,
}

impl TileCtx<'_> {
    /// Page voisine `(col + dc, row + dr)` si elle existe : son index dans `tiles`.
    fn neighbor(&self, tile: &Tile, dc: isize, dr: isize) -> Option<usize> {
        let col = tile.col.checked_add_signed(dc)?;
        let row = tile.row.checked_add_signed(dr)?;
        self.tiles.iter().position(|t| t.col == col && t.row == row)
    }
}

/// Demi-branche (mm) d'un repère de recalage en croix.
pub const MARK_ARM_MM: f64 = 3.0;
/// Retrait (mm) des repères par rapport aux bords de la bande de recouvrement et de la fenêtre.
pub const MARK_INSET_MM: f64 = 3.0;
/// Retrait (mm) des repères par rapport au bord de la fenêtre le long de la bande.
pub const MARK_ALONG_MM: f64 = 6.0;

/// Repères de recalage d'une page, dans le repère de la feuille tournée (celui de
/// `tile.world_min`) : quatre croix dans chaque bande de recouvrement qui touche une page
/// voisine existante. Deux pages voisines portent les **mêmes points** (à l'arrondi près) : la
/// bande droite de `(c, r)` et la bande gauche de `(c + 1, r)` couvrent le même rectangle.
pub fn registration_marks(tile: &Tile, tiles: &[Tile]) -> Vec<PointMm> {
    let ctx = TileCtx {
        lang: Lang::Fr,
        marks: true,
        control: false,
        tiles,
        base: 0,
    };
    let (ww, wh, ov) = (
        tile.layout.window_w_mm,
        tile.layout.window_h_mm,
        tile.overlap_mm,
    );
    let (wx, wy) = (tile.world_min.x, tile.world_min.y);
    let i = MARK_INSET_MM.min(ov / 2.0);
    let along = MARK_ALONG_MM;
    let mut marks = Vec::new();
    // Bandes verticales (voisins à gauche et à droite), puis horizontales (haut et bas).
    for (dc, x_lo) in [(-1isize, wx), (1, wx + ww - ov)] {
        if ctx.neighbor(tile, dc, 0).is_some() {
            for x in [x_lo + i, x_lo + ov - i] {
                for y in [wy + along, wy + wh - along] {
                    marks.push(PointMm::new(x, y));
                }
            }
        }
    }
    // Ligne `row - 1` = vers le haut du patron ; `row + 1` = vers le bas.
    for (dr, y_lo) in [(-1isize, wy + wh - ov), (1, wy)] {
        if ctx.neighbor(tile, 0, dr).is_some() {
            for y in [y_lo + i, y_lo + ov - i] {
                for x in [wx + along, wx + ww - along] {
                    marks.push(PointMm::new(x, y));
                }
            }
        }
    }
    marks
}

/// Flèche vectorielle (les flèches « → » ne sont pas dans Noto Sans) de `tail` à `head`.
fn arrow(c: &mut Content, tail: PointMm, head: PointMm) {
    c.line(tail, head);
    if let Ok(u) = (head - tail).unit() {
        for angle in [25.0, -25.0] {
            let (s, k) = pf_geometry::sin_cos_deg(angle);
            let back = pf_geometry::VectorMm::new(-u.x, -u.y);
            let v = pf_geometry::VectorMm::new(k * back.x - s * back.y, s * back.x + k * back.y);
            c.line(head, head + v * 1.6);
        }
    }
}

/// Repères de recalage et pages voisines sur les bords (option « repères d'assemblage »).
fn assembly_marks(c: &mut Content, tile: &Tile, ctx: &TileCtx) {
    let (mx, my) = (tile.layout.margins.left_mm, tile.layout.margins.bottom_mm);
    let (ww, wh) = (tile.layout.window_w_mm, tile.layout.window_h_mm);
    c.stroke(0.3, 0.2, &[]);
    for f in registration_marks(tile, ctx.tiles) {
        let q = tile.frame_to_page_mm(f);
        c.line(
            PointMm::new(q.x - MARK_ARM_MM, q.y),
            PointMm::new(q.x + MARK_ARM_MM, q.y),
        );
        c.line(
            PointMm::new(q.x, q.y - MARK_ARM_MM),
            PointMm::new(q.x, q.y + MARK_ARM_MM),
        );
    }
    // Page voisine : identifiant et numéro de page, avec une flèche vers son bord.
    let (cx, cy) = (mx + ww / 2.0, my + wh / 2.0);
    let width = |t: &str| t.chars().count() as f64 * 6.0 * 0.5 * 25.4 / 72.0;
    for (dc, dr) in [(1isize, 0isize), (-1, 0), (0, -1), (0, 1)] {
        let Some(k) = ctx.neighbor(tile, dc, dr) else {
            continue;
        };
        let other = &ctx.tiles[k];
        let label = format!("{} p.{}", other.id(), ctx.base + k + 1);
        let w = width(&label);
        let (tail, head, text_at) = match (dc, dr) {
            (1, 0) => (
                PointMm::new(mx + ww - 9.0, cy),
                PointMm::new(mx + ww - 2.0, cy),
                PointMm::new(mx + ww - 10.0 - w, cy - 0.8),
            ),
            (-1, 0) => (
                PointMm::new(mx + 9.0, cy),
                PointMm::new(mx + 2.0, cy),
                PointMm::new(mx + 10.0, cy - 0.8),
            ),
            (0, -1) => (
                PointMm::new(cx, my + wh - 9.0),
                PointMm::new(cx, my + wh - 2.0),
                PointMm::new(cx + 2.0, my + wh - 6.0),
            ),
            _ => (
                PointMm::new(cx, my + 9.0),
                PointMm::new(cx, my + 2.0),
                PointMm::new(cx + 2.0, my + 5.0),
            ),
        };
        arrow(c, tail, head);
        c.text(text_at.x, text_at.y, 6.0, &label);
    }
}

/// Gris des repères de contrôle (distinct des croix d'assemblage, 0,3, et du patron, 0).
pub const CONTROL_GRAY: f64 = 0.45;

/// Repères de contrôle d'une page de patron : deux règles de 100 mm **en mm papier, non
/// corrigés**, le long du bord bas et du bord gauche de la fenêtre (graduations de 10 mm vers
/// l'intérieur). Mesurés sur le papier, ils disent l'écart de l'imprimante même quand une
/// correction est active.
fn control_marks(c: &mut Content, mx: f64, my: f64, lang: Lang) {
    c.stroke(CONTROL_GRAY, 0.2, &[]);
    c.line(PointMm::new(mx, my), PointMm::new(mx + CALIBRATION_MM, my));
    c.line(PointMm::new(mx, my), PointMm::new(mx, my + CALIBRATION_MM));
    for i in 0..=10 {
        let d = f64::from(i) * 10.0;
        let h = if i % 10 == 0 { 3.5 } else { 2.0 };
        c.line(PointMm::new(mx + d, my), PointMm::new(mx + d, my + h));
        c.line(PointMm::new(mx, my + d), PointMm::new(mx + h, my + d));
    }
    c.text(
        mx + 12.0,
        my + 3.0,
        5.0,
        &tl(lang, "pdfx.control.mark", &[]),
    );
}

fn tile_page(scene: &Scene, tile: &Tile, n: usize, total: usize, ctx: &TileCtx) -> String {
    let mut c = Content::new();
    // Fenêtre de patron : coin bas-gauche (mx, my) et taille (ww, wh) sur la page.
    let (mx, my) = (tile.layout.margins.left_mm, tile.layout.margins.bottom_mm);
    let (ww, wh) = (tile.layout.window_w_mm, tile.layout.window_h_mm);
    let lang = ctx.lang;
    let kind = if scene.is_construction_only() {
        tl(lang, "pdfx.tile.construction", &[])
    } else {
        String::new()
    };
    let turned = if tile.turns > 0 {
        tl(
            lang,
            "pdfx.tile.turned",
            &[("deg", &(90 * u32::from(tile.turns)).to_string())],
        )
    } else {
        String::new()
    };
    let corrected = if tile.scale.is_identity() {
        String::new()
    } else {
        tl(
            lang,
            "pdfx.tile.corrected",
            &[
                ("x", &factor_text(tile.scale.x)),
                ("y", &factor_text(tile.scale.y)),
            ],
        )
    };
    c.text(
        mx,
        my + wh + 2.5,
        8.0,
        &tl(
            lang,
            "pdfx.tile.head",
            &[
                ("project", &shorten(&scene.project_name, 28)),
                ("sheet", &shorten(&scene.sheet_name, 28)),
                ("row", &(tile.row + 1).to_string()),
                ("col", &(tile.col + 1).to_string()),
                ("rows", &tile.rows.to_string()),
                ("cols", &tile.cols.to_string()),
                ("n", &n.to_string()),
                ("total", &total.to_string()),
                ("kind", &kind),
                ("turned", &turned),
                ("corrected", &corrected),
            ],
        ),
    );

    // Cadre de la fenêtre et traits de raccord (gris, hors rôles métier). Un trait de raccord
    // n'est tracé que du côté d'une page voisine qui existe (une page vide peut être omise).
    c.stroke(0.6, 0.15, &[]);
    c.rect(mx, my, ww, wh);
    c.stroke(0.6, 0.15, &[2.0, 2.0]);
    let ov = tile.overlap_mm;
    let (x_lo, x_hi) = (mx + ov, mx + ww - ov);
    let (y_lo, y_hi) = (my + ov, my + wh - ov);
    if ctx.neighbor(tile, -1, 0).is_some() {
        c.line(PointMm::new(x_lo, my), PointMm::new(x_lo, my + wh));
    }
    if ctx.neighbor(tile, 1, 0).is_some() {
        c.line(PointMm::new(x_hi, my), PointMm::new(x_hi, my + wh));
    }
    if ctx.neighbor(tile, 0, -1).is_some() {
        c.line(PointMm::new(mx, y_hi), PointMm::new(mx + ww, y_hi));
    }
    if ctx.neighbor(tile, 0, 1).is_some() {
        c.line(PointMm::new(mx, y_lo), PointMm::new(mx + ww, y_lo));
    }
    if ctx.control {
        control_marks(&mut c, mx, my, ctx.lang);
    }
    if ctx.marks {
        assembly_marks(&mut c, tile, ctx);
    }

    // Patron, découpé par la fenêtre mais jamais redimensionné.
    c.op("q");
    c.op(&format!(
        "{} {} {} {} re W n",
        pt(mx),
        pt(my),
        pt(ww),
        pt(wh)
    ));
    for path in &scene.paths {
        let st = stroke_style(path.role);
        c.stroke(0.0, st.width_mm, st.dash_mm);
        match &path.geom {
            &PathGeom::Line(a, b) => {
                c.line(tile.to_page_mm(a), tile.to_page_mm(b));
                if path.role == PathRole::Grainline {
                    for (p, q) in crate::grain_arrow_marks(a, b, path.sense) {
                        c.line(tile.to_page_mm(p), tile.to_page_mm(q));
                    }
                }
            }
            PathGeom::Cubic(k) => c.cubic([
                tile.to_page_mm(k.p0),
                tile.to_page_mm(k.c1),
                tile.to_page_mm(k.c2),
                tile.to_page_mm(k.p3),
            ]),
            PathGeom::Arc(_) | PathGeom::Circle(_) | PathGeom::Spline(_) => {
                let curves: Vec<[PointMm; 4]> = path
                    .geom
                    .arc_beziers()
                    .iter()
                    .map(|k| {
                        [
                            tile.to_page_mm(k.p0),
                            tile.to_page_mm(k.c1),
                            tile.to_page_mm(k.c2),
                            tile.to_page_mm(k.p3),
                        ]
                    })
                    .collect();
                c.beziers(&curves);
            }
        }
    }
    // Contours de coupe calculés (US-05.4) : un seul chemin fermé par pièce, trait long / trait
    // court (jamais la couleur seule), puis les crans de rempli en trait plein.
    for cut in &scene.cut_contours {
        c.stroke(
            0.0,
            crate::COMPUTED_CUT_STROKE_MM,
            crate::COMPUTED_CUT_DASH_MM,
        );
        c.closed_chain(&cut.edges, cut.closed, |p| tile.to_page_mm(p));
        if !cut.notch_marks.is_empty() || !cut.cut_notch_marks.is_empty() {
            c.stroke(0.0, crate::COMPUTED_CUT_STROKE_MM, &[]);
            for (p, q) in cut.notch_marks.iter().chain(&cut.cut_notch_marks) {
                c.line(tile.to_page_mm(*p), tile.to_page_mm(*q));
            }
        }
    }
    let notch_style = stroke_style(PathRole::CuttingContour);
    c.stroke(0.0, notch_style.width_mm, &[]);
    for (p, q) in &scene.notch_marks {
        c.line(tile.to_page_mm(*p), tile.to_page_mm(*q));
    }
    // Jambes de pince : trait long / point / point (jamais la couleur seule).
    if !scene.darts.is_empty() {
        c.stroke(0.0, crate::DART_STROKE_MM, crate::DART_DASH_MM);
        for d in &scene.darts {
            for (p, q) in d.legs() {
                c.line(tile.to_page_mm(p), tile.to_page_mm(q));
            }
        }
    }
    // Bords sur le pli (PH-5b) : trait long / point, libellé horizontal côté intérieur. Le
    // libellé est calculé dans le repère de la page (jamais à l'envers, même feuille tournée).
    if !scene.folds.is_empty() {
        c.stroke(0.0, crate::FOLD_EDGE_STROKE_MM, crate::FOLD_EDGE_DASH_MM);
        for f in &scene.folds {
            c.line(tile.to_page_mm(f.from), tile.to_page_mm(f.to));
        }
        let fold_size = crate::FOLD_LABEL_TEXT_MM * MM_TO_PT;
        for f in &scene.folds {
            let center =
                crate::fold_label_center(tile.to_page_mm(f.from), tile.to_page_mm(f.to), &f.label);
            let half = crate::font::text_width_mm(&f.label, fold_size) / 2.0;
            c.text(
                center.x - half,
                center.y - crate::FOLD_LABEL_TEXT_MM * 0.35,
                fold_size,
                &f.label,
            );
        }
    }
    // Marquages de pièce (US-06.3) : formes vectorielles en mm → pt, sans pointillé.
    if !scene.markings.is_empty() {
        c.stroke(0.0, crate::MARKING_STROKE_MM, &[]);
        for m in &scene.markings {
            if let Some((start, segments)) = m.slot_outline() {
                let to_page = |seg: &crate::SlotSegment| match seg {
                    crate::SlotSegment::Line(p) => crate::SlotSegment::Line(tile.to_page_mm(*p)),
                    crate::SlotSegment::Curve(k) => {
                        crate::SlotSegment::Curve(pf_geometry::CubicBezier::new(
                            tile.to_page_mm(k.p0),
                            tile.to_page_mm(k.c1),
                            tile.to_page_mm(k.c2),
                            tile.to_page_mm(k.p3),
                        ))
                    }
                };
                let page: Vec<crate::SlotSegment> = segments.iter().map(to_page).collect();
                c.closed_outline(tile.to_page_mm(start), &page);
                continue;
            }
            let curves: Vec<[PointMm; 4]> = m
                .circle_beziers()
                .iter()
                .map(|k| {
                    [
                        tile.to_page_mm(k.p0),
                        tile.to_page_mm(k.c1),
                        tile.to_page_mm(k.c2),
                        tile.to_page_mm(k.p3),
                    ]
                })
                .collect();
            match m.circle() {
                Some((_, true)) => c.fill_beziers(&curves),
                Some((_, false)) => c.beziers(&curves),
                None => {
                    for (p, q) in m.cross_marks() {
                        c.line(tile.to_page_mm(p), tile.to_page_mm(q));
                    }
                }
            }
        }
    }
    // Trous de piqûre (US-06.9) : cercles et polygones vectoriels en mm → pt, sans pointillé.
    if !scene.stitch_holes.is_empty() {
        c.stroke(0.0, crate::MARKING_STROKE_MM, &[]);
        for m in &scene.stitch_holes {
            if let Some((start, segments)) = m.outline() {
                let page: Vec<crate::SlotSegment> = segments
                    .iter()
                    .map(|seg| match seg {
                        crate::SlotSegment::Line(p) => {
                            crate::SlotSegment::Line(tile.to_page_mm(*p))
                        }
                        crate::SlotSegment::Curve(k) => crate::SlotSegment::Curve(*k),
                    })
                    .collect();
                c.closed_outline(tile.to_page_mm(start), &page);
                continue;
            }
            let curves: Vec<[PointMm; 4]> = m
                .circle_beziers()
                .iter()
                .map(|k| {
                    [
                        tile.to_page_mm(k.p0),
                        tile.to_page_mm(k.c1),
                        tile.to_page_mm(k.c2),
                        tile.to_page_mm(k.p3),
                    ]
                })
                .collect();
            c.beziers(&curves);
        }
    }
    // Zones (US-06.10) : contour, motif et étiquette ASCII, niveau de gris propre au type.
    for z in &scene.zones {
        let pat = z.pattern();
        let to = |p: &PointMm| tile.to_page_mm(*p);
        if let Some(first) = z.polygon.first() {
            c.stroke(z.gray(), crate::zone::ZONE_STROKE_MM, z.dash());
            let rest: Vec<crate::SlotSegment> = z.polygon[1..]
                .iter()
                .map(|p| crate::SlotSegment::Line(to(p)))
                .collect();
            c.closed_outline(to(first), &rest);
        }
        c.stroke(z.gray(), crate::zone::ZONE_HATCH_MM, &[]);
        for (a, b) in &pat.hatch {
            c.line(to(a), to(b));
        }
        if !pat.dots.is_empty() {
            c.op(&format!("{} g", num(z.gray())));
            for d in &pat.dots {
                if let Ok(circle) =
                    pf_geometry::CircleTrace::new(to(d), crate::zone::ZONE_DOT_RADIUS_MM, 0.0, true)
                {
                    let curves: Vec<[PointMm; 4]> = PathGeom::Circle(circle)
                        .arc_beziers()
                        .iter()
                        .map(|k| [k.p0, k.c1, k.c2, k.p3])
                        .collect();
                    c.fill_beziers(&curves);
                }
            }
            c.op("0 g");
        }
        if let Some(at) = pat.label_at {
            let at = to(&at);
            let size = crate::zone::ZONE_LABEL_MM * MM_TO_PT;
            let half = crate::font::text_width_mm(z.label(), size) / 2.0;
            c.text(at.x - half, at.y, size, z.label());
        }
    }
    // Boutons et boutonnières (US-06.4) : cercles et fentes vectoriels en mm → pt.
    if !scene.buttons.is_empty() {
        c.stroke(0.0, crate::BUTTON_STROKE_MM, &[]);
        for b in &scene.buttons {
            if let Some((start, segments)) = b.slot_outline() {
                let to_page = |seg: &crate::SlotSegment| match seg {
                    crate::SlotSegment::Line(p) => crate::SlotSegment::Line(tile.to_page_mm(*p)),
                    crate::SlotSegment::Curve(k) => {
                        crate::SlotSegment::Curve(pf_geometry::CubicBezier::new(
                            tile.to_page_mm(k.p0),
                            tile.to_page_mm(k.c1),
                            tile.to_page_mm(k.c2),
                            tile.to_page_mm(k.p3),
                        ))
                    }
                };
                let page: Vec<crate::SlotSegment> = segments.iter().map(to_page).collect();
                c.closed_outline(tile.to_page_mm(start), &page);
            } else {
                let curves: Vec<[PointMm; 4]> = b
                    .circle_beziers()
                    .iter()
                    .map(|k| {
                        [
                            tile.to_page_mm(k.p0),
                            tile.to_page_mm(k.c1),
                            tile.to_page_mm(k.c2),
                            tile.to_page_mm(k.p3),
                        ]
                    })
                    .collect();
                c.beziers(&curves);
            }
        }
    }
    c.stroke(0.0, 0.2, &[]);
    for d in &scene.dimensions {
        for (p, q) in d.marks() {
            c.line(tile.to_page_mm(p), tile.to_page_mm(q));
        }
        let t = tile.to_page_mm(d.text_anchor());
        let size = crate::DIMENSION_TEXT_MM * MM_TO_PT;
        // Largeur réelle (police embarquée) ; la position d'ancrage du modèle, partagée avec
        // SVG / DXF / HPGL, garde sa demi-largeur estimée.
        let real_hw = crate::font::text_width_mm(&d.text, size) / 2.0;
        if tile.turns == 0 {
            c.text(t.x - real_hw, t.y, size, &d.text);
        } else {
            // Feuille tournée : le texte suit sa ligne de cote et se lit sans être à l'envers.
            let (a, b) = {
                let anchor = d.text_anchor();
                let hw = real_hw;
                (
                    tile.to_page_mm(PointMm::new(anchor.x - hw, anchor.y)),
                    tile.to_page_mm(PointMm::new(anchor.x + hw, anchor.y)),
                )
            };
            // Quarts de tour 1 : lecture montante ; 2 et 3 : on part de l'autre extrémité.
            let (start, angle) = match tile.turns {
                1 => (a, 90.0),
                2 => (b, 0.0),
                _ => (b, 90.0),
            };
            c.text_rotated(start.x, start.y, size, angle, &d.text);
        }
    }
    // Cotes complètes (US-06.7) : lignes puis texte orienté (jamais à l'envers).
    for d in &scene.full_dimensions {
        for (p, q) in &d.marks {
            c.line(tile.to_page_mm(*p), tile.to_page_mm(*q));
        }
    }
    for d in &scene.full_dimensions {
        let dim_pt = crate::DIMENSION_TEXT_MM * MM_TO_PT;
        let real_w = crate::font::text_width_mm(&d.text, dim_pt);
        // Début de la ligne de base d'un texte centré sur `center` (largeur réelle).
        let start_at = |center: PointMm, angle: f64| {
            let (s, co) = pf_geometry::sin_cos_deg(angle);
            let down = crate::DIMENSION_TEXT_MM * 0.35;
            PointMm::new(
                center.x + s * down - co * real_w / 2.0,
                center.y - co * down - s * real_w / 2.0,
            )
        };
        if tile.turns == 0 {
            let t = start_at(tile.to_page_mm(d.text_center), d.text_angle_deg);
            c.text_rotated(t.x, t.y, dim_pt, d.text_angle_deg, &d.text);
        } else {
            // Feuille tournée : l'angle suit la rotation, ramené dans ]-90 ; 90] (jamais à
            // l'envers), le texte reste centré sur le même point de la cote.
            let mut angle = (d.text_angle_deg + 90.0 * f64::from(tile.turns)) % 360.0;
            if angle > 270.0 {
                angle -= 360.0;
            } else if angle > 90.0 {
                angle -= 180.0;
            }
            let start = start_at(tile.to_page_mm(d.text_center), angle);
            c.text_rotated(
                start.x,
                start.y,
                crate::DIMENSION_TEXT_MM * MM_TO_PT,
                angle,
                &d.text,
            );
        }
    }
    // Étiquettes de pièce : même police embarquée que les cotes. Feuille tournée : le
    // bloc reste centré sur le même point et se lit sans être à l'envers (angle 0 ou 90°).
    let label_size = crate::LABEL_TEXT_MM * MM_TO_PT;
    for l in &scene.piece_labels {
        let centre = tile.to_page_mm(l.center);
        let angle = if matches!(tile.turns, 1 | 3) {
            90.0
        } else {
            0.0
        };
        let (s, co) = pf_geometry::sin_cos_deg(angle);
        let n = l.lines.len() as f64;
        for (i, line) in l.lines.iter().enumerate() {
            let up = ((n - 1.0) / 2.0 - i as f64) * crate::LABEL_LINE_PITCH_MM
                - crate::LABEL_TEXT_MM * 0.35;
            let half = crate::font::text_width_mm(line, label_size) / 2.0;
            // `up` suit la normale du texte (-sin, cos), `half` son sens de lecture (cos, sin).
            let x = centre.x - s * up - co * half;
            let y = centre.y + co * up - s * half;
            if angle == 0.0 {
                c.text(x, y, label_size, line);
            } else {
                c.text_rotated(x, y, label_size, angle, line);
            }
        }
    }
    // Libellés « Rempli 8 mm » (PH-5c) : texte horizontal calculé dans le repère de la page (la
    // normale intérieure suit le quart de tour), jamais à l'envers.
    let over_size = crate::FOLD_LABEL_TEXT_MM * MM_TO_PT;
    for f in &scene.fold_overs {
        let at = tile.to_page_mm(f.at);
        let n = tile.to_page_mm(f.at + f.inward) - at;
        let inward = n.unit().unwrap_or(n);
        let center = crate::edge_label_center(at, inward, &f.label);
        let half = crate::font::text_width_mm(&f.label, over_size) / 2.0;
        c.text(
            center.x - half,
            center.y - crate::FOLD_LABEL_TEXT_MM * 0.35,
            over_size,
            &f.label,
        );
    }
    for p in &scene.points {
        let q = tile.to_page_mm(p.pos);
        if q.x < mx || q.x > mx + ww || q.y < my || q.y > my + wh {
            continue;
        }
        c.line(PointMm::new(q.x - 1.0, q.y), PointMm::new(q.x + 1.0, q.y));
        c.line(PointMm::new(q.x, q.y - 1.0), PointMm::new(q.x, q.y + 1.0));
        if p.show_label {
            c.text(q.x + 1.5, q.y + 1.5, 7.0, &shorten(&p.label, 12));
        }
    }
    c.op("Q");
    c.0
}

// ---------------------------------------------------------------------------
// Plan d'assemblage (US-13.2)
// ---------------------------------------------------------------------------

/// Nombre maximal de feuilles représentées sur la page de plan.
const PLAN_MAX_SCENES: usize = 6;
/// Plus petit corps (pt) d'un numéro de case : en dessous, les cases ne sont pas numérotées.
const PLAN_MIN_LABEL_PT: f64 = 3.0;

/// Largeur (mm) d'un texte de corps `size_pt`, d'après les largeurs d'avance de la police embarquée.
fn text_width_mm(text: &str, size_pt: f64) -> f64 {
    crate::font::text_width_mm(text, size_pt)
}

/// Page de plan : une miniature de la grille de pages de chaque feuille (cases numérotées,
/// pages omises barrées, emprise du patron, haut du patron), avec sa légende. **Pas à
/// l'échelle 1:1** : le plan l'écrit en toutes lettres et dessine ses cadres à l'échelle
/// réduite indiquée ; les pages de patron qui suivent restent à 100 %.
fn plan_page(
    scenes: &[Scene],
    plans: &[Vec<Tile>],
    setup: &PageSetup,
    n: usize,
    total: usize,
    lang: Lang,
) -> String {
    let m = setup.margins;
    let page_h = setup.page_height_mm().unwrap_or(0.0);
    let (left, right) = (m.left_mm + 5.0, setup.page_width_mm() - m.right_mm - 5.0);
    let (ytop, ybot) = (page_h - m.top_mm, m.bottom_mm);
    let mut c = Content::new();
    c.text(left, ytop - 8.0, 14.0, &tl(lang, "pdfx.plan.title", &[]));
    c.text(left, ytop - 14.0, 8.0, &tl(lang, "pdfx.plan.subtitle", &[]));

    let shown = scenes.len().min(PLAN_MAX_SCENES);
    let (area_top, area_bottom) = (ytop - 20.0, ybot + 32.0);
    let block_h = (area_top - area_bottom) / shown as f64;
    let mut first_page = n;
    for (i, (scene, tiles)) in scenes.iter().zip(plans).enumerate() {
        if i < shown {
            let top = area_top - i as f64 * block_h;
            plan_block(
                &mut c,
                scene,
                tiles,
                first_page,
                (left, right),
                (top, top - block_h),
                lang,
            );
        }
        first_page += tiles.len();
    }
    if scenes.len() > shown {
        c.text(
            left,
            area_bottom - 3.0,
            7.5,
            &tl(
                lang,
                "pdfx.plan.more",
                &[("n", &(scenes.len() - shown).to_string())],
            ),
        );
    }

    let ov = num(setup.overlap_mm);
    let marks = if setup.assembly.marks {
        tl(lang, "pdfx.plan.marks_on", &[])
    } else {
        tl(lang, "pdfx.plan.marks_off", &[])
    };
    let lines = [
        tl(lang, "pdfx.plan.legend.frames", &[]),
        tl(lang, "pdfx.plan.legend.overlap", &[("ov", &ov)]),
        tl(lang, "pdfx.plan.legend.numbers", &[]),
        marks,
        tl(lang, "pdfx.plan.legend.experimental", &[]),
    ];
    for (k, line) in lines.iter().enumerate() {
        c.text(left, ybot + 25.0 - 5.0 * k as f64, 7.5, line);
    }
    c.text(
        left,
        ybot,
        7.0,
        &tl(
            lang,
            "pdfx.plan.footer",
            &[("n", &n.to_string()), ("total", &total.to_string())],
        ),
    );
    c.0
}

/// Miniature de la grille d'une feuille dans la bande `top` → `bottom`.
fn plan_block(
    c: &mut Content,
    scene: &Scene,
    tiles: &[Tile],
    first_page: usize,
    (left, right): (f64, f64),
    (top, bottom): (f64, f64),
    lang: Lang,
) {
    let Some(t0) = tiles.first() else { return };
    let (cols, rows) = (t0.cols, t0.rows);
    let (ww, wh, ov) = (t0.layout.window_w_mm, t0.layout.window_h_mm, t0.overlap_mm);
    let (sx, sy) = (ww - ov, wh - ov);
    let gw = (cols - 1) as f64 * sx + ww;
    let gh = (rows - 1) as f64 * sy + wh;
    let gtop = top - 11.0;
    let (avail_w, avail_h) = (right - left, (gtop - bottom).max(1.0));
    let scale = (avail_w / gw).min(avail_h / gh);
    let gx = left + (avail_w - gw * scale) / 2.0;
    // Repère de la feuille tournée → plan.
    let origin_x = t0.world_min.x - t0.col as f64 * sx;
    let origin_top = t0.world_min.y + wh + t0.row as f64 * sy;
    let to_plan = |f: PointMm| {
        PointMm::new(
            gx + (f.x - origin_x) * scale,
            gtop - (origin_top - f.y) * scale,
        )
    };

    // En-tête : nom, taille de la grille, échelle du plan, rotation, pages omises.
    let omitted: Vec<String> = (0..rows)
        .flat_map(|r| (0..cols).map(move |k| (r, k)))
        .filter(|&(r, k)| !tiles.iter().any(|t| t.row == r && t.col == k))
        .map(|(r, k)| format!("L{}C{}", r + 1, k + 1))
        .collect();
    let denom = (1.0 / scale).round();
    c.text(
        left,
        top - 4.0,
        8.0,
        &shorten(
            &tl(
                lang,
                "pdfx.plan.block",
                &[
                    ("name", &shorten(&scene.sheet_name, 30)),
                    ("rows", &rows.to_string()),
                    ("cols", &cols.to_string()),
                    ("pages", &tiles.len().to_string()),
                    ("denom", &num(denom)),
                ],
            ),
            95,
        ),
    );
    let mut second = String::new();
    if t0.turns > 0 {
        second.push_str(&tl(
            lang,
            "pdfx.plan.turned",
            &[("deg", &(90 * u32::from(t0.turns)).to_string())],
        ));
    }
    if !omitted.is_empty() {
        let list: Vec<&str> = omitted.iter().take(6).map(String::as_str).collect();
        second.push_str(&tl(
            lang,
            "pdfx.plan.omitted",
            &[
                ("n", &omitted.len().to_string()),
                ("list", &list.join(", ")),
                ("more", if omitted.len() > 6 { ", …" } else { "" }),
            ],
        ));
    }
    if !second.is_empty() {
        c.text(left, top - 8.5, 8.0, &shorten(&second, 95));
    }
    // Haut du patron tel qu'imprimé.
    let up = rotate_quarter(PointMm::new(0.0, 1.0), t0.turns);
    let centre = PointMm::new(right - 8.0, top - 6.0);
    c.stroke(0.0, 0.3, &[]);
    arrow(
        c,
        PointMm::new(centre.x - 4.0 * up.x, centre.y - 4.0 * up.y),
        PointMm::new(centre.x + 4.0 * up.x, centre.y + 4.0 * up.y),
    );
    c.text(right - 34.0, top - 7.0, 7.0, &tl(lang, "pdfx.plan.up", &[]));

    // Cases : cadre plein si la page est imprimée, pointillé barré si elle est omise.
    let (cell_w, cell_h) = (ww * scale, wh * scale);
    let font = (cell_w * MM_TO_PT * 0.27).min(8.0);
    for row in 0..rows {
        for col in 0..cols {
            let x = gx + col as f64 * sx * scale;
            let y = gtop - (row as f64 * sy + wh) * scale;
            let id = format!("L{}C{}", row + 1, col + 1);
            let page = tiles
                .iter()
                .position(|t| t.row == row && t.col == col)
                .map(|k| first_page + k + 1);
            if page.is_some() {
                c.stroke(0.0, 0.25, &[]);
                c.rect(x, y, cell_w, cell_h);
            } else {
                c.stroke(0.5, 0.2, &[1.5, 1.0]);
                c.rect(x, y, cell_w, cell_h);
                c.line(PointMm::new(x, y), PointMm::new(x + cell_w, y + cell_h));
                c.line(PointMm::new(x, y + cell_h), PointMm::new(x + cell_w, y));
            }
            if font >= PLAN_MIN_LABEL_PT {
                let tx = x + (cell_w - text_width_mm(&id, font)) / 2.0;
                c.text(tx, y + cell_h / 2.0 + 0.5, font, &id);
                let sub = match page {
                    Some(p) => format!("p.{p}"),
                    None => tl(lang, "pdfx.plan.cell_omitted", &[]),
                };
                let sub_pt = font * 0.8;
                let sx_text = x + (cell_w - text_width_mm(&sub, sub_pt)) / 2.0;
                c.text(sx_text, y + cell_h / 2.0 - font * 0.4, sub_pt, &sub);
            }
        }
    }
    // Emprise du patron dans la grille.
    if let Some(bb) = scene.bbox() {
        let r = frame_bbox(&bb, t0.turns, t0.scale);
        let (lo, hi) = (to_plan(r.min), to_plan(r.max));
        c.stroke(0.4, 0.3, &[1.0, 1.0]);
        c.rect(lo.x, lo.y, hi.x - lo.x, hi.y - lo.y);
    }
}

// ---------------------------------------------------------------------------
// Assemblage du fichier
// ---------------------------------------------------------------------------

/// Objets de la police embarquée : la police composite (objet 3) et ses cinq objets annexes
/// (descendant, descripteur, `FontFile2`, `CIDToGIDMap`, `ToUnicode`), numérotés à partir de
/// `base`.
struct FontObjects {
    type0: String,
    others: Vec<String>,
}

/// Flux hors contenu de page : `stream` suivi de CR LF et `endstream` suivi de CR, pour qu'un
/// lecteur de test qui ne cherche que les flux de page (`stream\n`) ne les confonde pas avec
/// une page. Les données sont en ASCII (hexadécimal) : le fichier reste ASCII pur.
fn aux_stream(dict: &str, data: &str) -> String {
    format!(
        "<< /Length {} {dict} >>\nstream\r\n{data}\nendstream\r",
        data.len()
    )
}

fn hex_lines(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2 + bytes.len() / 48 + 1);
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 && i % 48 == 0 {
            out.push('\n');
        }
        out.push_str(&format!("{b:02X}"));
    }
    out.push('>');
    out
}

/// Police Noto Sans Regular réduite aux glyphes écrits dans `pages` : TrueType `CIDFontType2`,
/// codage `Identity-H`, `CIDToGIDMap` en flux, `W` lu dans la police, `ToUnicode` (le texte
/// reste copiable et cherchable, accents compris).
fn embedded_font_objects(pages: &[PdfPage], base: usize) -> FontObjects {
    let face = crate::font::embedded();
    let mut used = BTreeSet::new();
    for p in pages {
        used_cids(&p.content, &mut used);
    }
    let gids: BTreeSet<u16> = used.iter().map(|c| face.gid_of_cid(*c)).collect();
    let subset = face.subset(&gids);

    // Préfixe de sous-ensemble : six lettres tirées (FNV-1a) des CID utilisés, déterministe.
    let mut h: u32 = 0x811C_9DC5;
    for c in &used {
        for b in c.to_be_bytes() {
            h = (h ^ u32::from(b)).wrapping_mul(0x0100_0193);
        }
    }
    let tag: String = (0..6)
        .map(|i| char::from(b'A' + ((h >> (5 * i)) % 26) as u8))
        .collect();
    let name = format!("{tag}+NotoSans-Regular");

    let upem = f64::from(face.units_per_em());
    let k = 1000.0 / upem;
    let scaled = |v: i16| (f64::from(v) * k).round() as i64;
    let head = face.table(b"head").unwrap_or(&[]);
    let hhea = face.table(b"hhea").unwrap_or(&[]);
    let i16_at = |t: &[u8], o: usize| -> i16 {
        t.get(o..o + 2)
            .map_or(0, |b| i16::from_be_bytes([b[0], b[1]]))
    };
    let bbox: Vec<i64> = [36, 38, 40, 42]
        .iter()
        .map(|o| scaled(i16_at(head, *o)))
        .collect();
    let ascent = scaled(i16_at(hhea, 4));
    let descent = scaled(i16_at(hhea, 6));
    let cap = face
        .table(b"OS/2")
        .filter(|t| t.len() >= 90)
        .map_or(ascent, |t| scaled(i16_at(t, 88)));

    let w: String = used
        .iter()
        .map(|c| {
            let width = (f64::from(face.advance_of_cid(*c)) * k).round() as i64;
            format!("{c} [{width}]")
        })
        .collect::<Vec<_>>()
        .join(" ");

    let max = used.last().copied().unwrap_or(0);
    let mut map = Vec::with_capacity(2 * (usize::from(max) + 1));
    for cid in 0..=max {
        let gid = if used.contains(&cid) {
            face.gid_of_cid(cid)
        } else {
            0
        };
        map.extend(gid.to_be_bytes());
    }

    let mut bf = String::new();
    let entries: Vec<u16> = used.iter().copied().filter(|c| *c != 0).collect();
    for chunk in entries.chunks(100) {
        bf.push_str(&format!("{} beginbfchar\n", chunk.len()));
        for c in chunk {
            let ch = face.char_of_cid(*c).unwrap_or('?');
            let mut utf16 = [0u16; 2];
            let hex: String = ch
                .encode_utf16(&mut utf16)
                .iter()
                .map(|u| format!("{u:04X}"))
                .collect();
            bf.push_str(&format!("<{c:04X}> <{hex}>\n"));
        }
        bf.push_str("endbfchar\n");
    }
    let to_unicode = format!(
        "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n\
         /CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n\
         /CMapName /Adobe-Identity-UCS def\n/CMapType 2 def\n\
         1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n{bf}\
         endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend"
    );

    let type0 = format!(
        "<< /Type /Font /Subtype /Type0 /BaseFont /{name} /Encoding /Identity-H \
         /DescendantFonts [{base} 0 R] /ToUnicode {} 0 R >>",
        base + 4
    );
    let cid_font = format!(
        "<< /Type /Font /Subtype /CIDFontType2 /BaseFont /{name} \
         /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> \
         /FontDescriptor {} 0 R /DW 1000 /W [{w}] /CIDToGIDMap {} 0 R >>",
        base + 1,
        base + 3
    );
    let descriptor = format!(
        "<< /Type /FontDescriptor /FontName /{name} /Flags 32 /FontBBox [{} {} {} {}] \
         /ItalicAngle 0 /Ascent {ascent} /Descent {descent} /CapHeight {cap} /StemV 80 \
         /FontFile2 {} 0 R >>",
        bbox[0],
        bbox[1],
        bbox[2],
        bbox[3],
        base + 2
    );
    let font_file = aux_stream(
        &format!("/Length1 {} /Filter /ASCIIHexDecode", subset.len()),
        &hex_lines(&subset),
    );
    let cid_to_gid = aux_stream("/Filter /ASCIIHexDecode", &hex_lines(&map));
    FontObjects {
        type0,
        others: vec![
            cid_font,
            descriptor,
            font_file,
            cid_to_gid,
            aux_stream("", &to_unicode),
        ],
    }
}

pub(crate) fn assemble(pages: &[PdfPage], title: &str) -> Vec<u8> {
    let mut objects: Vec<String> = Vec::new();
    let kids: Vec<String> = (0..pages.len())
        .map(|i| format!("{} 0 R", 5 + 2 * i))
        .collect();
    objects.push("<< /Type /Catalog /Pages 2 0 R >>".into());
    objects.push(format!(
        "<< /Type /Pages /Kids [{}] /Count {} >>",
        kids.join(" "),
        pages.len()
    ));
    let font = embedded_font_objects(pages, 5 + 2 * pages.len());
    objects.push(font.type0);
    objects.push(format!(
        "<< /Title {} /Producer (PatternForge {}) >>",
        pdf_text_string(title),
        env!("CARGO_PKG_VERSION")
    ));
    for (i, page) in pages.iter().enumerate() {
        let media = format!("[0 0 {} {}]", pt(page.w_mm), pt(page.h_mm));
        let content = &page.content;
        objects.push(format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox {media} /Resources << /Font << /F1 3 0 R >> >> \
             /Contents {} 0 R >>",
            6 + 2 * i
        ));
        objects.push(format!(
            "<< /Length {} >>\nstream\n{content}endstream",
            content.len()
        ));
    }
    objects.extend(font.others);

    let mut out = String::from("%PDF-1.4\n");
    let mut offsets = Vec::with_capacity(objects.len());
    for (i, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.push_str(&format!("{} 0 obj\n{body}\nendobj\n", i + 1));
    }
    let xref = out.len();
    out.push_str(&format!(
        "xref\n0 {}\n0000000000 65535 f \n",
        objects.len() + 1
    ));
    for off in offsets {
        out.push_str(&format!("{off:010} 00000 n \n"));
    }
    out.push_str(&format!(
        "trailer\n<< /Size {} /Root 1 0 R /Info 4 0 R >>\nstartxref\n{xref}\n%%EOF\n",
        objects.len() + 1
    ));
    debug_assert!(out.is_ascii());
    out.into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ScenePath, ScenePoint};

    /// Trait relevé dans un flux de page : extrémités en mm, gris, épaisseur.
    #[derive(Debug, Clone, Copy)]
    struct Drawn {
        a: (f64, f64),
        b: (f64, f64),
        gray: f64,
    }

    impl Drawn {
        fn len(&self) -> f64 {
            (self.b.0 - self.a.0).hypot(self.b.1 - self.a.1)
        }
    }

    struct PageDump {
        lines: Vec<Drawn>,
        rects: Vec<(f64, f64, f64, f64, f64)>,
        curves: usize,
        text: String,
    }

    /// Mini-lecteur indépendant : relit les opérateurs `m l c re G` d'un flux
    /// et reconvertit les points en mm.
    fn dump_pages(bytes: &[u8]) -> Vec<PageDump> {
        let text = std::str::from_utf8(bytes).unwrap();
        let mut pages = Vec::new();
        let mut rest = text;
        while let Some(start) = rest.find("stream\n") {
            let body = &rest[start + 7..];
            let end = body.find("endstream").unwrap();
            pages.push(dump_stream(&body[..end]));
            rest = &body[end + 9..];
        }
        pages
    }

    fn dump_stream(stream: &str) -> PageDump {
        let mm = |v: f64| v / MM_TO_PT;
        let mut d = PageDump {
            lines: vec![],
            rects: vec![],
            curves: 0,
            text: String::new(),
        };
        let mut stack: Vec<f64> = Vec::new();
        let (mut gray, mut cur) = (0.0, (0.0, 0.0));
        let mut chars = stream.chars().peekable();
        let mut token = String::new();
        let mut tokens: Vec<String> = Vec::new();
        while let Some(ch) = chars.next() {
            match ch {
                '<' => {
                    // Chaîne hexadécimale de CID (2 octets), décodée par la table de la police.
                    let hex: String = chars.by_ref().take_while(|c2| *c2 != '>').collect();
                    for q in hex.as_bytes().chunks(4) {
                        let cid = u16::from_str_radix(std::str::from_utf8(q).unwrap(), 16).unwrap();
                        d.text.push(
                            crate::font::embedded()
                                .char_of_cid(cid)
                                .unwrap_or('\u{fffd}'),
                        );
                    }
                    d.text.push('\n');
                }
                '[' => {
                    for c2 in chars.by_ref() {
                        if c2 == ']' {
                            break;
                        }
                    }
                }
                c if c.is_whitespace() => {
                    if !token.is_empty() {
                        tokens.push(std::mem::take(&mut token));
                    }
                }
                c => token.push(c),
            }
        }
        for t in tokens {
            if let Ok(v) = t.parse::<f64>() {
                stack.push(v);
                continue;
            }
            match t.as_str() {
                "G" => gray = stack[stack.len() - 1],
                "m" => cur = (mm(stack[stack.len() - 2]), mm(stack[stack.len() - 1])),
                "l" => {
                    let b = (mm(stack[stack.len() - 2]), mm(stack[stack.len() - 1]));
                    d.lines.push(Drawn { a: cur, b, gray });
                    cur = b;
                }
                "c" => d.curves += 1,
                "re" => {
                    let n = stack.len();
                    d.rects.push((
                        mm(stack[n - 4]),
                        mm(stack[n - 3]),
                        mm(stack[n - 2]),
                        mm(stack[n - 1]),
                        gray,
                    ));
                }
                _ => {}
            }
            stack.clear();
        }
        d
    }

    fn rect_scene(w: f64, h: f64) -> Scene {
        let c = [
            PointMm::new(0., 0.),
            PointMm::new(w, 0.),
            PointMm::new(w, h),
            PointMm::new(0., h),
        ];
        Scene {
            project_name: "Test".into(),
            sheet_name: "Grande pièce".into(),
            paths: (0..4)
                .map(|i| ScenePath {
                    geom: PathGeom::Line(c[i], c[(i + 1) % 4]),
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

    #[test]
    fn conversion_mm_points() {
        assert_eq!(MM_TO_PT, 72.0 / 25.4);
        assert!((25.4 * MM_TO_PT - 72.0).abs() < 1e-12);
        assert!((A4_WIDTH_MM * MM_TO_PT - 595.2756).abs() < 1e-4);
        assert!((A4_HEIGHT_MM * MM_TO_PT - 841.8898).abs() < 1e-4);
    }

    #[test]
    fn at09_etalon_a4_carre_de_100_mm() {
        let pdf = export_calibration_pdf();
        assert_eq!(pdf.page_count, 1);
        let text = std::str::from_utf8(&pdf.bytes).unwrap();
        assert!(text.starts_with("%PDF-1.4"));
        assert_eq!(text.matches("/MediaBox [0 0 595.2756 841.8898]").count(), 1);
        assert_eq!(text.matches("/Type /Page ").count(), 1);
        let pages = dump_pages(&pdf.bytes);
        let (x, y, w, h, gray) = pages[0].rects[0];
        assert!(
            (w - 100.0).abs() < 1e-4 && (h - 100.0).abs() < 1e-4,
            "carré {w} × {h} mm"
        );
        assert_eq!(gray, 0.0);
        assert!(x >= PAGE_MARGIN_MM && x + w <= A4_WIDTH_MM - PAGE_MARGIN_MM);
        assert!(y >= PAGE_MARGIN_MM && y + h <= A4_HEIGHT_MM - PAGE_MARGIN_MM);
        // La règle graduée fait aussi 100 mm, avec 101 graduations.
        let ruler = pages[0]
            .lines
            .iter()
            .find(|l| (l.len() - 100.0).abs() < 1e-4)
            .unwrap();
        assert_eq!(ruler.a.1, ruler.b.1);
        assert!(pages[0].lines.iter().filter(|l| l.len() < 5.0).count() >= 101);
        assert!(pages[0].text.contains("100 %"));
        // Vectoriel : aucune image incorporée.
        assert!(!text.contains("/Image") && !text.contains("/XObject"));
    }

    #[test]
    fn structure_pdf_table_xref_coherente() {
        let project = pf_core::demo_project().unwrap();
        let scene = Scene::from_sheet(&project, project.sheets[0].id).unwrap();
        let pdf = export_pdf(&[scene]).unwrap();
        let text = std::str::from_utf8(&pdf.bytes).unwrap();
        assert!(text.is_ascii());
        let sx = text.rfind("startxref\n").unwrap();
        let xref: usize = text[sx + 10..].lines().next().unwrap().parse().unwrap();
        assert!(text[xref..].starts_with("xref\n"));
        let mut lines = text[xref..].lines().skip(1);
        let count: usize = lines
            .next()
            .unwrap()
            .split(' ')
            .nth(1)
            .unwrap()
            .parse()
            .unwrap();
        lines.next(); // entrée libre 0
        for i in 1..count {
            let off: usize = lines.next().unwrap()[..10].parse().unwrap();
            assert!(
                text[off..].starts_with(&format!("{i} 0 obj\n")),
                "objet {i} mal référencé"
            );
        }
        // Chaque flux a la longueur annoncée.
        let mut rest = text;
        while let Some(p) = rest.find("/Length ") {
            let after = &rest[p + 8..];
            let len: usize = after[..after.find(' ').unwrap()].parse().unwrap();
            // Flux de page : `stream\n` ; flux de police : `stream\r\n` et `\n` avant `endstream`.
            let (at, open, skip) = match (after.find("stream\n"), after.find("stream\r\n")) {
                (Some(a), Some(b)) if a < b => (a, 7, 0),
                (Some(a), None) => (a, 7, 0),
                (_, Some(b)) => (b, 8, 1),
                (None, None) => panic!("flux sans stream"),
            };
            let body = &after[at + open..];
            assert!(
                body[len + skip..].starts_with("endstream"),
                "{}",
                &body[..len.min(60)]
            );
            rest = &body[len..];
        }
        assert_eq!(pdf.page_count, 2);
        assert_eq!(text.matches("/Type /Page ").count(), 2);
    }

    #[test]
    fn exercice_de_reference_cotes_dans_le_pdf() {
        let project = pf_core::demo_project().unwrap();
        let scene = Scene::from_sheet(&project, project.sheets[0].id).unwrap();
        let pdf = export_pdf(&[scene]).unwrap();
        let page = &dump_pages(&pdf.bytes)[1];
        let black: Vec<f64> = page
            .lines
            .iter()
            .filter(|l| l.gray == 0.0 && l.len() > 5.0)
            .map(Drawn::len)
            .collect();
        assert_eq!(black.len(), 2);
        assert!(
            (black[0] - 120.0).abs() < 1e-4 && (black[1] - 80.0).abs() < 1e-4,
            "{black:?}"
        );
        assert_eq!(page.curves, 1);
        assert!(page.text.contains("L1C1") && page.text.contains("page 2/2"));
        assert!(page.text.contains("tracé de construction"));
    }

    #[test]
    fn at11_piece_600_par_450_tuilee_sans_reduction() {
        let scene = rect_scene(600.0, 450.0);
        let tiles = plan_tiles(&scene).unwrap();
        assert_eq!((tiles[0].cols, tiles[0].rows), (4, 2));
        let pdf = export_pdf(std::slice::from_ref(&scene)).unwrap();
        assert_eq!(pdf.page_count, 9);
        assert_eq!(page_count(std::slice::from_ref(&scene)).unwrap(), 9);
        let pages = dump_pages(&pdf.bytes);
        assert_eq!(pages.len(), 9);

        // Sur chaque tuile, les côtés gardent leur vraie longueur (pas d'échelle).
        for (tile, page) in tiles.iter().zip(&pages[1..]) {
            let mut lens: Vec<f64> = page
                .lines
                .iter()
                .filter(|l| l.gray == 0.0 && l.len() > 5.0)
                .map(Drawn::len)
                .collect();
            lens.sort_by(f64::total_cmp);
            assert_eq!(lens.len(), 4);
            for (got, want) in lens.iter().zip([450.0, 450.0, 600.0, 600.0]) {
                assert!((got - want).abs() < 1e-3, "{got} au lieu de {want}");
            }
            // Le coin (0,0) du patron est dessiné là où la tuile l'attend.
            let expect = tile.to_page_mm(PointMm::new(0.0, 0.0));
            assert!(page
                .lines
                .iter()
                .any(|l| { (l.a.0 - expect.x).abs() < 1e-3 && (l.a.1 - expect.y).abs() < 1e-3 }));
            assert!(page
                .text
                .contains(&format!("L{}C{}", tile.row + 1, tile.col + 1)));
        }

        // Les fenêtres couvrent toute la pièce et se recouvrent de 10 mm.
        let bb = scene.bbox().unwrap();
        let (mut x_max, mut y_min) = (f64::MIN, f64::MAX);
        for t in &tiles {
            x_max = x_max.max(t.world_min.x + WINDOW_W_MM);
            y_min = y_min.min(t.world_min.y);
            if t.col > 0 {
                let left = tiles
                    .iter()
                    .find(|o| o.row == t.row && o.col == t.col - 1)
                    .unwrap();
                assert!((left.world_min.x + WINDOW_W_MM - t.world_min.x - OVERLAP_MM).abs() < 1e-9);
            }
            if t.row > 0 {
                let up = tiles
                    .iter()
                    .find(|o| o.col == t.col && o.row == t.row - 1)
                    .unwrap();
                assert!((t.world_min.y + WINDOW_H_MM - up.world_min.y - OVERLAP_MM).abs() < 1e-9);
            }
        }
        assert!(tiles[0].world_min.x <= bb.min.x && x_max >= bb.max.x);
        assert!(tiles[0].world_min.y + WINDOW_H_MM >= bb.max.y && y_min <= bb.min.y);
    }

    #[test]
    fn petite_piece_une_seule_tuile_et_limites() {
        assert_eq!(plan_tiles(&rect_scene(180.0, 259.0)).unwrap().len(), 1);
        assert_eq!(plan_tiles(&rect_scene(181.0, 259.0)).unwrap().len(), 2);
        assert_eq!(plan_tiles(&rect_scene(180.0, 260.0)).unwrap().len(), 2);
        let err = export_pdf(&[rect_scene(5000.0, 5000.0)]).unwrap_err();
        assert!(matches!(err, ExportError::TooManyPages { .. }));
        assert!(err.to_string().contains("refusé"));
    }

    #[test]
    fn export_deterministe() {
        let scene = rect_scene(300.0, 200.0);
        let a = export_pdf(std::slice::from_ref(&scene)).unwrap();
        let b = export_pdf(std::slice::from_ref(&scene)).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn texte_ecrit_en_cid_de_la_police_embarquee() {
        let face = crate::font::embedded();
        let (e, p, qm) = (face.cid('é'), face.cid('('), face.cid('?'));
        assert_eq!(
            hex_cids("é(?"),
            format!("<{e:04X}{p:04X}{qm:04X}>"),
            "un CID de 2 octets par caractère"
        );
        // Hors police : remplacé par « ? », jamais d'octet invalide ni de panic.
        assert_eq!(hex_cids("日本"), format!("<{qm:04X}{qm:04X}>"));
        assert_eq!(hex_cids(""), "<>");
        assert_eq!(pdf_text_string("Jupe (A)"), "(Jupe \\(A\\))");
        assert_eq!(pdf_text_string("é"), "<FEFF00E9>");
    }

    #[test]
    fn crans_et_fleches_de_droit_fil_dans_le_pdf() {
        let project = pf_core::demo_piece_project().unwrap();
        let mut scene = Scene::from_sheet(&project, project.sheets[0].id).unwrap();
        assert_eq!(scene.notch_marks.len(), 4);
        assert_eq!(scene.dimensions.len(), 2);
        scene.dimensions.clear();
        let pdf = export_pdf(&[scene]).unwrap();
        assert_eq!(pdf.page_count, 2);
        let page = &dump_pages(&pdf.bytes)[1];
        let count = |len: f64| {
            page.lines
                .iter()
                .filter(|l| l.gray == 0.0 && (l.len() - len).abs() < 1e-3)
                .count()
        };
        // 4 traits de cran (5 mm) ; le droit-fil de 80 mm porte 4 branches de 4 mm.
        assert_eq!(count(5.0), 4);
        assert_eq!(count(4.0), 4);
        assert_eq!(count(80.0), 1);
        // Les quatre côtés du rectangle gardent leurs vraies longueurs.
        assert_eq!(count(170.0), 2);
        assert_eq!(count(120.0), 2);
        // La légende de la page étalon décrit le cran.
        let first = &dump_pages(&pdf.bytes)[0];
        assert!(first.text.contains("Cran"));
    }

    #[test]
    fn cotations_dans_le_pdf_texte_et_traits() {
        let project = pf_core::demo_piece_project().unwrap();
        let scene = Scene::from_sheet(&project, project.sheets[0].id).unwrap();
        let pdf = export_pdf(&[scene]).unwrap();
        let page = &dump_pages(&pdf.bytes)[1];
        assert!(page.text.contains("170.00 mm"), "{}", page.text);
        assert!(page.text.contains("120.00 mm"), "{}", page.text);
        let count = |len: f64| {
            page.lines
                .iter()
                .filter(|l| (l.len() - len).abs() < 1e-3)
                .count()
        };
        // Deux traits d'attache de 15 mm par cotation, deux lignes de cote, 8 flèches de 4 mm.
        assert_eq!(count(15.0), 4);
        assert_eq!(count(4.0), 4 + 8);
        assert_eq!(count(170.0), 2 + 1);
        assert_eq!(count(120.0), 2 + 1);
    }

    #[test]
    fn fleche_trop_courte_omise() {
        let a = PointMm::new(0., 0.);
        assert!(crate::grainline_arrow_marks(a, PointMm::new(10., 0.)).is_empty());
        assert_eq!(
            crate::grainline_arrow_marks(a, PointMm::new(50., 0.)).len(),
            4
        );
        assert!(crate::grainline_arrow_marks(a, a).is_empty());
    }

    // ----- US-19.2 : textes d'export localisés -----

    fn lang_scenes() -> Vec<Scene> {
        let mut a = rect_scene(600.0, 450.0);
        a.sheet_name = "Grande pièce".into();
        let roles = PathRole::BASE.iter().chain(PathRole::SPECIAL.iter());
        for (i, role) in roles.enumerate() {
            let y = 10.0 + 5.0 * i as f64;
            a.paths.push(ScenePath {
                geom: PathGeom::Line(PointMm::new(50.0, y), PointMm::new(90.0, y)),
                role: *role,
                sense: if *role == PathRole::Grainline {
                    pf_core::GrainSense::Nap
                } else {
                    pf_core::GrainSense::Unmarked
                },
            });
        }
        a.darts.push(crate::SceneDart {
            piece: "P".into(),
            apex: PointMm::new(100.0, 100.0),
            left_foot: PointMm::new(95.0, 80.0),
            right_foot: PointMm::new(105.0, 80.0),
        });
        a.markings.push(crate::SceneMarking {
            pos: PointMm::new(120.0, 100.0),
            shape: pf_core::MarkingShape::Slot {
                length_mm: 8.0,
                width_mm: 2.0,
                angle_deg: 0.0,
            },
        });
        a.stitch_holes.push(crate::SceneStitchHole {
            pos: PointMm::new(130.0, 100.0),
            hole: pf_core::StitchHole::Round { diameter_mm: 1.0 },
            tangent_deg: 0.0,
        });
        a.buttons.push(crate::SceneButton {
            pos: PointMm::new(140.0, 100.0),
            shape: pf_core::ButtonShape::Button { diameter_mm: 10.0 },
        });
        let mut b = rect_scene(500.0, 300.0);
        b.sheet_name = "Esquisse".into();
        for p in &mut b.paths {
            p.role = PathRole::Construction;
        }
        vec![a, b]
    }

    fn lang_setup() -> PageSetup {
        PageSetup {
            format: crate::page::PageFormat::Custom {
                width_mm: 300.0,
                height_mm: 420.0,
            },
            orientation: crate::page::Orientation::Landscape,
            assembly: crate::page::AssemblyOptions {
                marks: true,
                plan: true,
                save_paper: false,
            },
            scale: PrintScale { x: 1.01, y: 0.99 },
            ..PageSetup::default()
        }
    }

    /// Lignes de texte écrites dans le PDF (relues par le mini-lecteur indépendant).
    fn pdf_lines(lang: Lang) -> (Vec<String>, usize, Vec<Vec<Tile>>) {
        let (scenes, setup) = (lang_scenes(), lang_setup());
        let out = export_pdf_lang(&scenes, &setup, &OutputContent::default(), lang).unwrap();
        let lines = dump_pages(&out.bytes)
            .into_iter()
            .flat_map(|p| p.text.lines().map(str::to_string).collect::<Vec<_>>())
            .collect();
        let tiles = scenes
            .iter()
            .map(|s| plan_tiles_with(s, &setup).unwrap())
            .collect();
        (lines, out.page_count, tiles)
    }

    fn role_text(role: PathRole, fr: bool) -> &'static str {
        let (f, e) = match role {
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
        };
        if fr {
            f
        } else {
            e
        }
    }

    /// Textes attendus, écrits à la main d'après les anciennes chaînes françaises (oracle
    /// indépendant du catalogue) et leur traduction anglaise.
    fn expected_lines(fr: bool, total: usize, tiles: &[Vec<Tile>]) -> Vec<String> {
        let p = |f: &str, e: &str| if fr { f.to_string() } else { e.to_string() };
        let tiles_total: usize = tiles.iter().map(Vec::len).sum();
        let first_n = total - tiles_total + 1;
        let mut v = vec![
            p("PatternForge — page étalon", "PatternForge - calibration page"),
            p(
                "Imprimer à 100 % (taille réelle). Désactiver « Ajuster à la page ».",
                "Print at 100% (actual size). Turn off \"Fit to page\".",
            ),
            p(
                "Contrôle : mesurer le carré au réglet. Chaque côté doit faire 100 mm (± 1 mm).",
                "Check: measure the square with a ruler. Each side must be 100 mm (+/- 1 mm).",
            ),
            p(
                "Si la mesure est fausse, ne pas découper : corriger l'échelle d'impression.",
                "If the measure is wrong, do not cut: fix the print scale.",
            ),
            p("Légende des traits", "Line legend"),
            p(
                "Cran — petit trait perpendiculaire au bord",
                "Notch - short line perpendicular to the edge",
            ),
            p(
                "Sens du droit-fil — poil : une pointe fermée ; motif : pointes doublées",
                "Grainline direction - nap: one closed head; print: doubled heads",
            ),
            p(
                "Pince — jambes (sommet vers les deux pieds), trait long et deux points",
                "Dart - legs (apex towards the two ends), long line and two dots",
            ),
            p(
                "Marquage — pointage (disque plein), perçage (cercle), croix, fente",
                "Marking - punch point (filled disc), drill hole (circle), cross, slot",
            ),
            p(
                "Trou de piqure - rond (cercle), plat, francais (losange), oblique",
                "Stitching hole - round (circle), flat, French (diamond), slanted",
            ),
            p(
                "Bouton (cercle), boutonnière (fente à bouts arrondis)",
                "Button (circle), buttonhole (slot with rounded ends)",
            ),
            p(
                &format!("Contenu du document : {total} page(s) libre 300 × 420 mm paysage, étalon compris"),
                &format!("Document contents: {total} page(s) custom 300 × 420 mm landscape, calibration included"),
            ),
            p(
                "Papier : libre 300 × 420 mm paysage — marges haut 10 / bas 10 / gauche 10 / droite 10 mm.",
                "Paper: custom 300 × 420 mm landscape - margins top 10 / bottom 10 / left 10 / right 10 mm.",
            ),
            p(
                "Correction d'échelle active : X ×1.01, Y ×0.99 (facteur = taille attendue / taille mesurée).",
                "Scale correction active: X ×1.01, Y ×0.99 (factor = expected size / measured size).",
            ),
            p(
                "Carré de 100 mm et règle ci-dessus : corrigés ; à vérifier par mesure au réglet sur le papier.",
                "100 mm square and ruler above: corrected; check by measuring with a ruler on the paper.",
            ),
            p(
                "Assemblage : superposer les bandes de recouvrement de 10 mm, traits gris de raccord alignés.",
                "Assembly: overlap the 10 mm overlap strips, grey joining lines aligned.",
            ),
            p(
                "Export expérimental tant que cet étalon n'a pas été mesuré sur papier.",
                "Experimental export until this calibration has been measured on paper.",
            ),
            p("Plan d'assemblage des pages", "Page assembly plan"),
            p(
                "Plan pas à l'échelle 1:1 (réduit). Les pages de patron qui suivent sont à 100 % (taille réelle).",
                "Plan not at 1:1 scale (reduced). The pattern pages that follow are at 100% (actual size).",
            ),
            p(
                "Repères : croix à faire coïncider avec celles de la page voisine ; flèche + identifiant au bord = page voisine.",
                "Marks: crosses to match with those of the neighbouring page; arrow + identifier at the edge = neighbouring page.",
            ),
            p(
                "Cadre plein : page imprimée. Cadre pointillé barré : page omise car vide (numérotation de la grille conservée).",
                "Solid frame: printed page. Crossed dotted frame: page skipped because empty (grid numbering kept).",
            ),
            p(
                "Pointillé fin : emprise du patron. Les cadres se recouvrent de 10 mm. Flèche : haut du patron tel qu'imprimé.",
                "Fine dotted line: pattern extent. Frames overlap by 10 mm. Arrow: top of the pattern as printed.",
            ),
            p(
                "Numéros : L = ligne, C = colonne de la grille ; p. = numéro de page du fichier PDF.",
                "Numbers: L = line (row), C = column of the grid; p. = page number in the PDF file.",
            ),
            p(
                "Export expérimental tant que l'étalon n'a pas été mesuré sur papier.",
                "Experimental export until the calibration has been measured on paper.",
            ),
            p(
                &format!("plan d'assemblage · page {}/{total}", first_n - 1),
                &format!("assembly plan · page {}/{total}", first_n - 1),
            ),
            p("haut du patron", "top of pattern"),
        ];
        for role in PathRole::BASE.iter().chain(PathRole::SPECIAL.iter()) {
            v.push(role_text(*role, fr).to_string());
        }
        // Têtes de page de patron et lignes « feuille » de l'étalon.
        let mut n = first_n;
        for (name, kind_fr, kind_en, t) in [
            ("Grande pièce", "", "", &tiles[0]),
            (
                "Esquisse",
                " · tracé de construction",
                " · construction drawing",
                &tiles[1],
            ),
        ] {
            let t0 = &t[0];
            let corr = p(
                " · correction d'échelle X ×1.01 Y ×0.99",
                " · scale correction X ×1.01 Y ×0.99",
            );
            v.push(p(
                &format!(
                    "Test — {name} · L1C1 sur {}×{} · page {n}/{total} · 100 % taille réelle{kind_fr}{corr}",
                    t0.rows, t0.cols
                ),
                &format!(
                    "Test - {name} · L1C1 of {}×{} · page {n}/{total} · 100% actual size{kind_en}{corr}",
                    t0.rows, t0.cols
                ),
            ));
            n += t.len();
            let (kf, ke) = if kind_fr.is_empty() {
                (String::new(), String::new())
            } else {
                (
                    " — tracé de construction, pas de ligne de coupe".to_string(),
                    " - construction drawing, no cutting line".to_string(),
                )
            };
            v.push(p(
                &format!(
                    "« {name} » : {} page(s), {} ligne(s) × {} colonne(s){kf}",
                    t.len(),
                    t0.rows,
                    t0.cols
                ),
                &format!(
                    "\"{name}\": {} page(s), {} row(s) × {} column(s){ke}",
                    t.len(),
                    t0.rows,
                    t0.cols
                ),
            ));
        }
        v
    }

    #[test]
    fn export_francais_inchange_textes_de_reference() {
        let (lines, total, tiles) = pdf_lines(Lang::Fr);
        for want in expected_lines(true, total, &tiles) {
            assert!(lines.contains(&want), "absent du PDF français : {want}");
        }
        // L'export historique (sans langue) est octet pour octet celui de `Lang::Fr`.
        let (scenes, setup) = (lang_scenes(), lang_setup());
        let old = export_pdf_content(&scenes, &setup, &OutputContent::default()).unwrap();
        let fr = export_pdf_lang(&scenes, &setup, &OutputContent::default(), Lang::Fr).unwrap();
        assert_eq!(old.bytes, fr.bytes);
    }

    #[test]
    fn export_anglais_textes_traduits_et_aucun_francais() {
        let (lines, total, tiles) = pdf_lines(Lang::En);
        for want in expected_lines(false, total, &tiles) {
            assert!(lines.contains(&want), "absent du PDF anglais : {want}");
        }
        let all = lines.join("\n");
        assert!(!all.contains('\u{fffd}') && !all.contains('?'), "{all}");
        for french in [
            "page étalon",
            "Légende",
            "droit-fil",
            "Papier",
            "feuille",
            "Assemblage",
            "ligne(s)",
            "Imprimer",
            "patron",
        ] {
            assert!(!all.contains(french), "« {french} » dans le PDF anglais");
        }
    }

    #[test]
    fn export_anglais_et_francais_different_seulement_par_les_textes() {
        let (scenes, setup) = (lang_scenes(), lang_setup());
        let c = OutputContent::default();
        let fr = export_pdf_lang(&scenes, &setup, &c, Lang::Fr).unwrap();
        let en = export_pdf_lang(&scenes, &setup, &c, Lang::En).unwrap();
        assert_ne!(fr.bytes, en.bytes);
        assert_eq!(fr.page_count, en.page_count);
        assert_eq!(fr.page_sizes_mm, en.page_sizes_mm);
        // Mêmes traits dessinés (le texte seul change, jamais la géométrie).
        let strokes = |b: &[u8]| {
            dump_pages(b)
                .iter()
                .map(|p| (p.lines.len(), p.rects.len(), p.curves))
                .collect::<Vec<_>>()
        };
        assert_eq!(strokes(&fr.bytes), strokes(&en.bytes));
    }

    #[test]
    fn textes_pdf_couverts_par_la_police_embarquee() {
        let face = crate::font::embedded();
        for lang in Lang::ALL {
            for (key, text) in lang.catalog() {
                if !key.starts_with("pdfx.") || key.starts_with("pdfx.svg.") {
                    continue;
                }
                for ch in text.chars() {
                    assert!(
                        face.glyph(ch).is_some(),
                        "{key} ({lang:?}) : « {ch} » absent de la police"
                    );
                }
            }
        }
    }

    #[test]
    fn rouleau_et_etalon_seul_localises() {
        let setup = PageSetup {
            format: crate::page::PageFormat::Roll { width_mm: 914.0 },
            ..PageSetup::default()
        };
        let fr = export_calibration_pdf_lang(&setup, Lang::Fr).unwrap();
        let en = export_calibration_pdf_lang(&setup, Lang::En).unwrap();
        let text = |b: &[u8]| {
            dump_pages(b)
                .iter()
                .map(|p| p.text.clone())
                .collect::<String>()
        };
        let (tf, te) = (text(&fr.bytes), text(&en.bytes));
        assert!(tf.contains("Rouleau : une page continue par feuille, sans assemblage."));
        assert!(tf.contains("Papier : rouleau 914 mm — marges haut 10"));
        assert!(te.contains("Roll: one continuous page per sheet, no assembly."));
        assert!(te.contains("Paper: roll 914 mm - margins top 10"));
        assert_eq!(export_calibration_pdf_with(&setup).unwrap().bytes, fr.bytes);
    }
}
