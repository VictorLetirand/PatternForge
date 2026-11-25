//! Aide des tests de l'image de fond (E14) : images **générées par le code** (damier, rectangle,
//! bruit à graine fixe), jamais de fichier binaire versionné ; dossiers temporaires propres au fil
//! d'exécution, vidés avant usage.
#![allow(dead_code)]

use std::path::PathBuf;

/// Dossier temporaire propre au fil d'exécution, vidé avant usage.
pub fn tmp(prefix: &str, name: &str) -> PathBuf {
    let thread = format!("{:?}", std::thread::current().id()).replace(['(', ')'], "");
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("{prefix}-{name}-{thread}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Générateur congruentiel (graine fixe, résultats reproductibles).
pub struct Lcg(pub u64);

impl Lcg {
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    /// Réel dans [lo ; hi[.
    pub fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * (self.next() as f64 / (1u64 << 31) as f64)
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// PNG en niveaux de gris 8 bits.
pub fn png_gray(w: u32, h: u32, pixel: impl Fn(u32, u32) -> u8) -> Vec<u8> {
    let data: Vec<u8> = (0..h)
        .flat_map(|y| (0..w).map(move |x| (x, y)))
        .map(|(x, y)| pixel(x, y))
        .collect();
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, w, h);
    enc.set_color(png::ColorType::Grayscale);
    enc.set_depth(png::BitDepth::Eight);
    let mut wr = enc.write_header().unwrap();
    wr.write_image_data(&data).unwrap();
    wr.finish().unwrap();
    out
}

/// PNG RGB 8 bits.
pub fn png_rgb(w: u32, h: u32, pixel: impl Fn(u32, u32) -> [u8; 3]) -> Vec<u8> {
    let data: Vec<u8> = (0..h)
        .flat_map(|y| (0..w).map(move |x| (x, y)))
        .flat_map(|(x, y)| pixel(x, y))
        .collect();
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, w, h);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    let mut wr = enc.write_header().unwrap();
    wr.write_image_data(&data).unwrap();
    wr.finish().unwrap();
    out
}

/// JPEG en niveaux de gris (qualité 95).
pub fn jpeg_gray(w: u16, h: u16, pixel: impl Fn(u32, u32) -> u8) -> Vec<u8> {
    let data: Vec<u8> = (0..u32::from(h))
        .flat_map(|y| (0..u32::from(w)).map(move |x| (x, y)))
        .map(|(x, y)| pixel(x, y))
        .collect();
    let mut out = Vec::new();
    jpeg_encoder::Encoder::new(&mut out, 95)
        .encode(&data, w, h, jpeg_encoder::ColorType::Luma)
        .unwrap();
    out
}

/// Damier de cases de `cell` pixels (noir 0 / blanc 255).
pub fn checker(cell: u32) -> impl Fn(u32, u32) -> u8 {
    move |x, y| {
        if (x / cell + y / cell) % 2 == 0 {
            255
        } else {
            0
        }
    }
}

/// Rectangle blanc `[x0 ; x1[ × [y0 ; y1[` sur fond noir.
pub fn rect(x0: u32, y0: u32, x1: u32, y1: u32) -> impl Fn(u32, u32) -> u8 {
    move |x, y| {
        if x >= x0 && x < x1 && y >= y0 && y < y1 {
            255
        } else {
            0
        }
    }
}

/// Bruit uniforme à graine fixe.
pub fn noise(seed: u64) -> impl Fn(u32, u32) -> u8 {
    move |x, y| {
        let mut g = Lcg(seed ^ (u64::from(x) << 32) ^ u64::from(y).wrapping_mul(0x9E37_79B9));
        g.next();
        (g.next() & 0xFF) as u8
    }
}
