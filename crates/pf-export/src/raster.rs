//! US-12.6 : export PNG et JPEG d'une scène, rastérisée depuis les mêmes primitives que le SVG.
//!
//! Même cadre que le SVG (boîte englobante + marge de 10 mm, axe Y retourné), fond blanc, trait
//! noir de l'épaisseur et du pointillé du rôle (`stroke_style`), niveaux de gris 8 bits.
//! Dimensions en pixels = `round(mm / 25,4 × dpi)` ; la résolution est écrite dans le fichier
//! (pHYs en pixels par mètre pour le PNG, densité JFIF en points par pouce pour le JPEG).
//!
//! Dessinés : tracés, pointes de droit-fil, contours de coupe calculés et leurs crans, crans,
//! jambes de pince. Non dessinés (déclaré) : textes, cotes, points, marquages, zones, boutons.

use crate::{
    stroke_style, ExportError, PathGeom, PathRole, Scene, COMPUTED_CUT_DASH_MM,
    COMPUTED_CUT_STROKE_MM, DART_DASH_MM, DART_STROKE_MM,
};
use pf_geometry::PointMm;

pub const MIN_DPI: u32 = 36;
pub const MAX_DPI: u32 = 1200;
/// Marge blanche autour du contenu (mm), celle du SVG.
pub const RASTER_MARGIN_MM: f64 = crate::svg::SVG_MARGIN_MM;
/// Nombre maximal de pixels d'une image (refus au-delà, avant toute allocation).
pub const MAX_PIXELS: u64 = 120_000_000;
/// Côté maximal (JPEG : 65 535).
pub const MAX_SIDE_PX: u32 = 65_535;
/// Qualité JPEG utilisée (1 à 100).
pub const JPEG_QUALITY: u8 = 90;

/// Image en niveaux de gris, 255 = blanc.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Raster {
    pub width: u32,
    pub height: u32,
    pub dpi: u32,
    pub gray: Vec<u8>,
}

/// Nombre de pixels d'une longueur en mm : `round(mm / 25,4 × dpi)`.
pub fn mm_to_px(mm: f64, dpi: u32) -> u32 {
    (mm / 25.4 * f64::from(dpi)).round() as u32
}

fn refuse(msg: String) -> ExportError {
    ExportError::Io(msg)
}

/// Dimensions en pixels de l'image d'une scène (sans la dessiner) ; refus d'un dpi hors de
/// 36 à 1200, d'une scène vide ou d'une image trop grande.
pub fn raster_size(scene: &Scene, dpi: u32) -> Result<(u32, u32), ExportError> {
    if !(MIN_DPI..=MAX_DPI).contains(&dpi) {
        return Err(refuse(format!(
            "résolution {dpi} dpi hors de {MIN_DPI} à {MAX_DPI}"
        )));
    }
    scene.check_finite()?;
    let bb = scene
        .bbox()
        .ok_or_else(|| ExportError::EmptySheet(scene.sheet_name.clone()))?;
    let (w_mm, h_mm) = (
        bb.width() + 2.0 * RASTER_MARGIN_MM,
        bb.height() + 2.0 * RASTER_MARGIN_MM,
    );
    if !(w_mm.is_finite() && h_mm.is_finite()) {
        return Err(refuse("dimensions non finies".to_string()));
    }
    let (w, h) = (mm_to_px(w_mm, dpi), mm_to_px(h_mm, dpi));
    if w == 0
        || h == 0
        || w > MAX_SIDE_PX
        || h > MAX_SIDE_PX
        || u64::from(w) * u64::from(h) > MAX_PIXELS
    {
        return Err(refuse(format!(
            "image de {w} x {h} pixels refusée (maximum {MAX_PIXELS} pixels, {MAX_SIDE_PX} par côté)"
        )));
    }
    Ok((w, h))
}

struct Canvas {
    w: usize,
    h: usize,
    px: Vec<u8>,
    /// Pixels par mm.
    k: f64,
    min: PointMm,
    top: f64,
}

impl Canvas {
    fn to_px(&self, p: PointMm) -> (f64, f64) {
        (
            (p.x - self.min.x + RASTER_MARGIN_MM) * self.k,
            (self.top - p.y + RASTER_MARGIN_MM) * self.k,
        )
    }

    fn segment(&mut self, a: (f64, f64), b: (f64, f64), half: f64) {
        let r = half + 1.0;
        let x0 = ((a.0.min(b.0) - r).floor().max(0.0)) as usize;
        let y0 = ((a.1.min(b.1) - r).floor().max(0.0)) as usize;
        let x1 = (((a.0.max(b.0) + r).ceil()).min(self.w as f64 - 1.0)).max(-1.0);
        let y1 = (((a.1.max(b.1) + r).ceil()).min(self.h as f64 - 1.0)).max(-1.0);
        if x1 < 0.0 || y1 < 0.0 {
            return;
        }
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let len2 = dx * dx + dy * dy;
        for y in y0..=(y1 as usize) {
            for x in x0..=(x1 as usize) {
                let (cx, cy) = (x as f64 + 0.5, y as f64 + 0.5);
                let t = if len2 > 0.0 {
                    (((cx - a.0) * dx + (cy - a.1) * dy) / len2).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let d = ((cx - a.0 - t * dx).powi(2) + (cy - a.1 - t * dy).powi(2)).sqrt();
                let cov = (half + 0.5 - d).clamp(0.0, 1.0);
                if cov > 0.0 {
                    let v = (255.0 * (1.0 - cov)).round() as u8;
                    let cell = &mut self.px[y * self.w + x];
                    *cell = (*cell).min(v);
                }
            }
        }
    }

    /// Trace une polyligne (points du modèle, mm) avec épaisseur et pointillé en mm.
    fn stroke(&mut self, pts: &[PointMm], width_mm: f64, dash_mm: &[f64], closed: bool) {
        let mut v: Vec<(f64, f64)> = pts.iter().map(|p| self.to_px(*p)).collect();
        if closed {
            if let Some(f) = v.first().copied() {
                v.push(f);
            }
        }
        let half = (width_mm * self.k / 2.0).max(0.5);
        let dash: Vec<f64> = dash_mm.iter().map(|d| d * self.k).collect();
        if dash.is_empty() || dash.iter().any(|d| *d <= 0.0) {
            for w in v.windows(2) {
                self.segment(w[0], w[1], half);
            }
            return;
        }
        let (mut i, mut left, mut on) = (0usize, dash[0], true);
        for w in v.windows(2) {
            let (a, b) = (w[0], w[1]);
            let len = ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt();
            let mut pos = 0.0;
            while pos < len {
                let step = left.min(len - pos);
                if on {
                    let (t0, t1) = (pos / len, (pos + step) / len);
                    let p = |t: f64| (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
                    self.segment(p(t0), p(t1), half);
                }
                pos += step;
                left -= step;
                if left <= 1e-9 {
                    i = (i + 1) % dash.len();
                    left = dash[i];
                    on = !on;
                }
            }
        }
    }
}

fn steps(canvas: &Canvas, length_mm: f64) -> usize {
    ((length_mm * canvas.k / 2.0).ceil() as usize).clamp(4, 4096)
}

fn flatten(canvas: &Canvas, g: &PathGeom) -> (Vec<PointMm>, bool) {
    let cubic = |c: &pf_geometry::CubicBezier| {
        let d = |a: PointMm, b: PointMm| (b.x - a.x).hypot(b.y - a.y);
        let l = d(c.p0, c.c1) + d(c.c1, c.c2) + d(c.c2, c.p3);
        c.sample(steps(canvas, l))
    };
    match g {
        &PathGeom::Line(a, b) => (vec![a, b], false),
        PathGeom::Cubic(c) => (cubic(c), false),
        PathGeom::Arc(a) => (
            a.sample(steps(canvas, a.radius_mm * a.sweep_rad.abs())),
            false,
        ),
        PathGeom::Circle(c) => (
            c.sample(steps(canvas, c.circle.radius_mm * std::f64::consts::TAU)),
            false,
        ),
        PathGeom::Spline(segs) => {
            let mut pts: Vec<PointMm> = Vec::new();
            for c in segs {
                let s = cubic(c);
                pts.extend(if pts.is_empty() { &s[..] } else { &s[1..] });
            }
            (pts, false)
        }
    }
}

/// Rastérise la scène à `dpi` (36 à 1200).
pub fn rasterize(scene: &Scene, dpi: u32) -> Result<Raster, ExportError> {
    let (w, h) = raster_size(scene, dpi)?;
    let bb = scene.bbox().expect("raster_size a vérifié la boîte");
    let mut c = Canvas {
        w: w as usize,
        h: h as usize,
        px: vec![255; w as usize * h as usize],
        k: f64::from(dpi) / 25.4,
        min: bb.min,
        top: bb.max.y,
    };
    for path in &scene.paths {
        let st = stroke_style(path.role);
        let (pts, closed) = flatten(&c, &path.geom);
        c.stroke(&pts, st.width_mm, st.dash_mm, closed);
        if let (PathGeom::Line(a, b), PathRole::Grainline) = (&path.geom, path.role) {
            for (p, q) in crate::grain_arrow_marks(*a, *b, path.sense) {
                c.stroke(&[p, q], st.width_mm, st.dash_mm, false);
            }
        }
    }
    for cut in &scene.cut_contours {
        for e in &cut.edges {
            let (pts, _) = flatten(&c, e);
            c.stroke(&pts, COMPUTED_CUT_STROKE_MM, COMPUTED_CUT_DASH_MM, false);
        }
        for (p, q) in cut.notch_marks.iter().chain(&cut.cut_notch_marks) {
            c.stroke(&[*p, *q], COMPUTED_CUT_STROKE_MM, &[], false);
        }
    }
    let notch = stroke_style(PathRole::CuttingContour).width_mm;
    for (p, q) in &scene.notch_marks {
        c.stroke(&[*p, *q], notch, &[], false);
    }
    for d in &scene.darts {
        for (p, q) in d.legs() {
            c.stroke(&[p, q], DART_STROKE_MM, DART_DASH_MM, false);
        }
    }
    Ok(Raster {
        width: w,
        height: h,
        dpi,
        gray: c.px,
    })
}

/// PNG en niveaux de gris, chunk pHYs en pixels par mètre (`round(dpi / 0,0254)`).
pub fn export_png(scene: &Scene, dpi: u32) -> Result<Vec<u8>, ExportError> {
    let r = rasterize(scene, dpi)?;
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, r.width, r.height);
    enc.set_color(png::ColorType::Grayscale);
    enc.set_depth(png::BitDepth::Eight);
    let ppm = (f64::from(dpi) / 0.0254).round() as u32;
    enc.set_pixel_dims(Some(png::PixelDimensions {
        xppu: ppm,
        yppu: ppm,
        unit: png::Unit::Meter,
    }));
    let io = |e: png::EncodingError| refuse(format!("encodage PNG : {e}"));
    let mut writer = enc.write_header().map_err(io)?;
    writer.write_image_data(&r.gray).map_err(io)?;
    writer.finish().map_err(io)?;
    Ok(out)
}

/// JPEG en niveaux de gris (qualité [`JPEG_QUALITY`]), densité JFIF en points par pouce.
pub fn export_jpeg(scene: &Scene, dpi: u32) -> Result<Vec<u8>, ExportError> {
    let r = rasterize(scene, dpi)?;
    let mut out = Vec::new();
    let mut enc = jpeg_encoder::Encoder::new(&mut out, JPEG_QUALITY);
    enc.set_density(jpeg_encoder::PixelDensity::dpi(dpi as u16));
    enc.encode(
        &r.gray,
        r.width as u16,
        r.height as u16,
        jpeg_encoder::ColorType::Luma,
    )
    .map_err(|e| refuse(format!("encodage JPEG : {e}")))?;
    Ok(out)
}
