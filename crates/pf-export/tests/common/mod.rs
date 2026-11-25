//! Outils communs aux tests d'assemblage de pages (US-13.2).
//!
//! Le lecteur de PDF est écrit ici, indépendant du code d'export : il ne réutilise ni
//! `MM_TO_PT`, ni les constantes de pages, ni les fonctions de dessin. Les conversions
//! points → mm utilisent 25,4 / 72 écrit en dur.
#![allow(dead_code)]

pub mod contenu;
pub mod coupe;
pub mod csv_reader;
pub mod dxf;
pub mod dxf_check;
pub mod dxf_generic;
pub mod dxf_models;
pub mod hpgl;
pub mod pdf_text;
pub mod rapport;
pub mod report_pdf;

use pf_core::PathRole;
use pf_export::{PathGeom, Scene, ScenePath};
use pf_geometry::PointMm;

pub const PT_TO_MM: f64 = 25.4 / 72.0;

#[derive(Debug, Clone, Copy)]
pub struct Line {
    pub a: (f64, f64),
    pub b: (f64, f64),
    pub gray: f64,
}

impl Line {
    pub fn len(&self) -> f64 {
        (self.b.0 - self.a.0).hypot(self.b.1 - self.a.1)
    }
}

#[derive(Debug, Default)]
pub struct Page {
    /// MediaBox lue, en points.
    pub media_pt: (f64, f64),
    pub lines: Vec<Line>,
    /// Rectangles `re` en mm : x, y, largeur, hauteur, gris, vrai si rectangle de découpe.
    pub rects: Vec<(f64, f64, f64, f64, f64, bool)>,
    /// Chaînes de texte décodées, dans l'ordre du flux.
    pub strings: Vec<String>,
    pub text: String,
}

impl Page {
    pub fn media_mm(&self) -> (f64, f64) {
        (self.media_pt.0 * PT_TO_MM, self.media_pt.1 * PT_TO_MM)
    }
    pub fn has(&self, needle: &str) -> bool {
        self.strings.iter().any(|s| s.contains(needle))
    }
}

pub fn read_pdf(bytes: &[u8]) -> Vec<Page> {
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
    let declared: usize = text[text.find("/Count ").unwrap() + 7..]
        .split_whitespace()
        .next()
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(declared, pages.len(), "/Count du catalogue");
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
                let mut s = String::new();
                let mut depth = 1;
                while let Some(c) = chars.next() {
                    match c {
                        '\\' => {
                            let first = chars.clone().next().unwrap_or(' ');
                            if first.is_digit(8) {
                                let mut code = 0u32;
                                for _ in 0..3 {
                                    match chars.clone().next().and_then(|d| d.to_digit(8)) {
                                        Some(d) => {
                                            code = code * 8 + d;
                                            chars.next();
                                        }
                                        None => break,
                                    }
                                }
                                s.push(match code {
                                    0x85 => '…',
                                    0x92 => '’',
                                    0x96 => '–',
                                    0x97 => '—',
                                    c => char::from_u32(c).unwrap_or('?'),
                                });
                            } else {
                                s.push(first);
                                chars.next();
                            }
                        }
                        '(' => {
                            depth += 1;
                            s.push(c);
                        }
                        ')' => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                            s.push(c);
                        }
                        _ => s.push(c),
                    }
                }
                page.text.push_str(&s);
                page.text.push('\n');
                page.strings.push(s);
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
    let mut gray = 0.0;
    for (i, t) in tokens.iter().enumerate() {
        if let Ok(v) = t.parse::<f64>() {
            nums.push(v);
            continue;
        }
        let n = nums.len();
        let mm = |v: f64| v * PT_TO_MM;
        match t.as_str() {
            "G" => gray = nums[n - 1],
            "m" => here = (mm(nums[n - 2]), mm(nums[n - 1])),
            "l" => {
                let b = (mm(nums[n - 2]), mm(nums[n - 1]));
                page.lines.push(Line { a: here, b, gray });
                here = b;
            }
            "re" => {
                let clip = tokens.get(i + 1).map(String::as_str) == Some("W");
                page.rects.push((
                    mm(nums[n - 4]),
                    mm(nums[n - 3]),
                    mm(nums[n - 2]),
                    mm(nums[n - 1]),
                    gray,
                    clip,
                ));
            }
            _ => {}
        }
        nums.clear();
    }
    page
}

// ---------------------------------------------------------------------------
// Générateur pseudo-aléatoire à graine fixe
// ---------------------------------------------------------------------------

pub struct Rng(pub u64);

impl Rng {
    pub fn next(&mut self) -> u64 {
        // xorshift64*
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    pub fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
    pub fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.unit()
    }
    pub fn pick<T: Copy>(&mut self, xs: &[T]) -> T {
        xs[(self.next() % xs.len() as u64) as usize]
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

// ---------------------------------------------------------------------------
// Scènes
// ---------------------------------------------------------------------------

pub fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

pub fn scene_of(paths: Vec<PathGeom>) -> Scene {
    Scene {
        project_name: "Test".into(),
        sheet_name: "Feuille".into(),
        paths: paths
            .into_iter()
            .map(|geom| ScenePath {
                geom,
                role: PathRole::CuttingContour,
                sense: pf_core::GrainSense::Unmarked,
            })
            .collect(),
        points: Vec::new(),
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

/// Rectangle `w × h` mm dont le coin bas-gauche est en (x ; y), en ligne de coupe.
pub fn rect_at(x: f64, y: f64, w: f64, h: f64) -> Vec<PathGeom> {
    let c = [pt(x, y), pt(x + w, y), pt(x + w, y + h), pt(x, y + h)];
    (0..4)
        .map(|i| PathGeom::Line(c[i], c[(i + 1) % 4]))
        .collect()
}

pub fn rect_scene(w: f64, h: f64) -> Scene {
    scene_of(rect_at(0.0, 0.0, w, h))
}

/// Plus petit nombre de pages de fenêtre `window` qui couvrent `len` mm avec un recouvrement
/// `overlap` : formulation indépendante de celle du code (on compte jusqu'à couvrir).
pub fn pages_to_cover(len: f64, window: f64, overlap: f64) -> usize {
    let mut n = 1usize;
    while window + (n - 1) as f64 * (window - overlap) < len - 1e-9 {
        n += 1;
        assert!(n < 100_000, "grille démesurée");
    }
    n
}

// ---------------------------------------------------------------------------
// Oracle de grille : recalculée ici, sans le code d'export
// ---------------------------------------------------------------------------

/// Quart de tour dans le sens trigonométrique.
pub fn rot(p: (f64, f64), turns: u8) -> (f64, f64) {
    match turns % 4 {
        0 => p,
        1 => (-p.1, p.0),
        2 => (-p.0, -p.1),
        _ => (p.1, -p.0),
    }
}

/// Grille de pages d'une emprise (blanc de 5 mm autour), fenêtres `ww × wh`.
#[derive(Debug, Clone, Copy)]
pub struct Grid {
    pub x0: f64,
    pub top: f64,
    pub ww: f64,
    pub wh: f64,
    pub ov: f64,
    pub cols: usize,
    pub rows: usize,
}

pub fn grid_of(min: (f64, f64), max: (f64, f64), ww: f64, wh: f64, ov: f64) -> Grid {
    Grid {
        x0: min.0 - 5.0,
        top: max.1 + 5.0,
        ww,
        wh,
        ov,
        cols: pages_to_cover(max.0 - min.0 + 10.0, ww, ov),
        rows: pages_to_cover(max.1 - min.1 + 10.0, wh, ov),
    }
}

impl Grid {
    /// Coin bas-gauche de la fenêtre `(col, row)`.
    pub fn wmin(&self, col: usize, row: usize) -> (f64, f64) {
        (
            self.x0 + col as f64 * (self.ww - self.ov),
            self.top - self.wh - row as f64 * (self.wh - self.ov),
        )
    }
}

/// Cases sûrement occupées et cases peut-être occupées d'une liste de points d'échantillon
/// (déjà dans le repère de la grille) : un échantillon à l'intérieur de la fenêtre rétrécie de
/// `delta` occupe sûrement la case ; à l'intérieur de la fenêtre agrandie de `delta`, il peut
/// l'occuper (les cas limites d'arrondi restent dans l'écart entre les deux).
pub fn occupancy(g: &Grid, samples: &[(f64, f64)], delta: f64) -> (Vec<bool>, Vec<bool>) {
    let mut sure = vec![false; g.cols * g.rows];
    let mut maybe = vec![false; g.cols * g.rows];
    let (sx, sy) = (g.ww - g.ov, g.wh - g.ov);
    for &(x, y) in samples {
        let c0 = ((x - delta - g.x0 - g.ww) / sx).ceil().max(0.0) as usize;
        let c1 = ((x + delta - g.x0) / sx).floor().min(g.cols as f64 - 1.0);
        let r0 = ((g.top - g.wh - y - delta) / sy).ceil().max(0.0) as usize;
        let r1 = ((g.top - y + delta) / sy).floor().min(g.rows as f64 - 1.0);
        if c1 < 0.0 || r1 < 0.0 {
            continue;
        }
        for row in r0..=(r1 as usize) {
            for col in c0..=(c1 as usize) {
                let (wx, wy) = g.wmin(col, row);
                let inside =
                    |d: f64| x >= wx - d && x <= wx + g.ww + d && y >= wy - d && y <= wy + g.wh + d;
                if inside(delta) {
                    maybe[row * g.cols + col] = true;
                }
                if inside(-delta) {
                    sure[row * g.cols + col] = true;
                }
            }
        }
    }
    (sure, maybe)
}

/// Points d'échantillon (au plus tous les `step` mm) de tracés : segments, cercles et
/// Béziers cubiques.
pub fn sample_paths(paths: &[PathGeom], step: f64) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    for g in paths {
        match g {
            PathGeom::Line(a, b) => {
                let n = ((b.x - a.x).hypot(b.y - a.y) / step).ceil().max(1.0) as usize;
                for i in 0..=n {
                    let t = i as f64 / n as f64;
                    out.push((a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t));
                }
            }
            PathGeom::Circle(c) => {
                let r = c.circle.radius_mm;
                let n = (std::f64::consts::TAU * r / step).ceil().max(8.0) as usize;
                for i in 0..n {
                    let t = std::f64::consts::TAU * i as f64 / n as f64;
                    out.push((
                        c.circle.center.x + r * t.cos(),
                        c.circle.center.y + r * t.sin(),
                    ));
                }
            }
            PathGeom::Cubic(k) => {
                let n = 20_000;
                for i in 0..=n {
                    let q = k.point_at(i as f64 / n as f64);
                    out.push((q.x, q.y));
                }
            }
            _ => unreachable!("tracé non géré par l'oracle"),
        }
    }
    out
}
