//! E14 — lecture, copie et contrôle du **fichier** de l'image de fond.
//!
//! Tout ce qui touche au disque est ici ; `Project::apply` n'ouvre jamais d'image (les commandes
//! reçoivent le chemin relatif, l'empreinte et les dimensions déjà lus). Règles :
//!
//! - le format est reconnu à l'**entête** du fichier (PNG ou JPEG), jamais à l'extension ;
//! - les dimensions sont lues **avant** le décodage et bornées (`MAX_IMAGE_PIXELS`) : une image
//!   déclarant un milliard de pixels est refusée sans allocation ;
//! - l'image est **copiée** dans le dossier voisin `<nom-projet>.assets/` sous un nom qui contient
//!   les 12 premiers chiffres de son empreinte (jamais d'écrasement d'un autre fichier) ; elle
//!   n'est jamais écrite dans le JSON du projet ;
//! - un fichier absent, illisible ou d'empreinte différente est **signalé** (`ImageStatus`), jamais
//!   une erreur d'ouverture du projet ni un panic.

use crate::file_guard::sha256_hex;
use crate::sheet_background::{unsafe_image_path, Background, MAX_BACKGROUND_PX};
use std::io::Cursor;
use std::path::{Path, PathBuf};

/// Taille maximale du fichier image lu (octets).
pub const MAX_IMAGE_BYTES: u64 = 256 * 1024 * 1024;
/// Nombre maximal de pixels décodés (≈ 50 mégapixels : au-delà, refus avec message).
pub const MAX_IMAGE_PIXELS: u64 = 50_000_000;

/// Formats d'image acceptés.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFormat {
    Png,
    Jpeg,
}

impl ImageFormat {
    pub fn extension(self) -> &'static str {
        match self {
            ImageFormat::Png => "png",
            ImageFormat::Jpeg => "jpg",
        }
    }
}

/// Format reconnu à l'entête (signature PNG de 8 octets, marqueur JPEG `FF D8 FF`).
pub fn sniff(bytes: &[u8]) -> Option<ImageFormat> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        Some(ImageFormat::Png)
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some(ImageFormat::Jpeg)
    } else {
        None
    }
}

/// Image décodée en RGBA 8 bits (ligne par ligne, haut en bas).
#[derive(Debug, Clone, PartialEq)]
pub struct DecodedImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl DecodedImage {
    /// Niveaux de gris (luminance Rec. 601 : `(299 R + 587 G + 114 B) / 1000`, arrondie à
    /// l'inférieur) ; la transparence est ignorée.
    pub fn gray(&self) -> Vec<u8> {
        self.rgba
            .chunks_exact(4)
            .map(|p| {
                ((299 * u32::from(p[0]) + 587 * u32::from(p[1]) + 114 * u32::from(p[2])) / 1000)
                    as u8
            })
            .collect()
    }
}

fn check_size(w: u32, h: u32) -> Result<(), String> {
    if w == 0 || h == 0 {
        return Err("image vide (0 pixel de large ou de haut)".into());
    }
    if w > MAX_BACKGROUND_PX || h > MAX_BACKGROUND_PX {
        return Err(format!(
            "image trop grande ({w} × {h} px, côté maximal {MAX_BACKGROUND_PX})"
        ));
    }
    if u64::from(w) * u64::from(h) > MAX_IMAGE_PIXELS {
        return Err(format!(
            "image trop grande ({w} × {h} px, maximum {} mégapixels)",
            MAX_IMAGE_PIXELS / 1_000_000
        ));
    }
    Ok(())
}

/// Dimensions et format lus dans l'entête, sans décoder les pixels.
pub fn probe(bytes: &[u8]) -> Result<(u32, u32, ImageFormat), String> {
    let fmt = sniff(bytes).ok_or("ce fichier n'est ni un PNG ni un JPEG")?;
    let (w, h) = match fmt {
        ImageFormat::Png => {
            let dec = png::Decoder::new(Cursor::new(bytes));
            let reader = dec
                .read_info()
                .map_err(|e| format!("PNG illisible : {e}"))?;
            let i = reader.info();
            (i.width, i.height)
        }
        ImageFormat::Jpeg => {
            let mut dec = zune_jpeg::JpegDecoder::new(bytes);
            dec.decode_headers()
                .map_err(|e| format!("JPEG illisible : {e:?}"))?;
            let i = dec.info().ok_or("JPEG illisible : entête incomplet")?;
            (u32::from(i.width), u32::from(i.height))
        }
    };
    check_size(w, h)?;
    Ok((w, h, fmt))
}

/// Décode l'image (PNG ou JPEG) en RGBA. Refus propre : fichier tronqué, corrompu, trop grand.
pub fn decode(bytes: &[u8]) -> Result<DecodedImage, String> {
    let (w, h, fmt) = probe(bytes)?;
    let n = (w as usize) * (h as usize);
    let rgba = match fmt {
        ImageFormat::Png => {
            let mut dec = png::Decoder::new(Cursor::new(bytes));
            dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
            let mut reader = dec
                .read_info()
                .map_err(|e| format!("PNG illisible : {e}"))?;
            let size = reader
                .output_buffer_size()
                .ok_or("PNG : taille de sortie trop grande")?;
            let mut buf = vec![0u8; size];
            let out = reader
                .next_frame(&mut buf)
                .map_err(|e| format!("PNG illisible : {e}"))?;
            let data = &buf[..out.buffer_size()];
            let mut rgba = Vec::with_capacity(n * 4);
            match out.color_type {
                png::ColorType::Grayscale => {
                    for g in data {
                        rgba.extend_from_slice(&[*g, *g, *g, 255]);
                    }
                }
                png::ColorType::GrayscaleAlpha => {
                    for p in data.chunks_exact(2) {
                        rgba.extend_from_slice(&[p[0], p[0], p[0], p[1]]);
                    }
                }
                png::ColorType::Rgb => {
                    for p in data.chunks_exact(3) {
                        rgba.extend_from_slice(&[p[0], p[1], p[2], 255]);
                    }
                }
                png::ColorType::Rgba => rgba.extend_from_slice(data),
                png::ColorType::Indexed => return Err("PNG indexé non converti".into()),
            }
            rgba
        }
        ImageFormat::Jpeg => {
            let mut dec = zune_jpeg::JpegDecoder::new(bytes);
            let pixels = dec
                .decode()
                .map_err(|e| format!("JPEG illisible : {e:?}"))?;
            let comps = pixels.len() / n.max(1);
            if pixels.len() != n * comps || !(comps == 1 || comps == 3 || comps == 4) {
                return Err("JPEG : composantes de couleur non gérées".into());
            }
            let mut rgba = Vec::with_capacity(n * 4);
            for p in pixels.chunks_exact(comps) {
                match comps {
                    1 => rgba.extend_from_slice(&[p[0], p[0], p[0], 255]),
                    _ => rgba.extend_from_slice(&[p[0], p[1], p[2], 255]),
                }
            }
            rgba
        }
    };
    if rgba.len() != n * 4 {
        return Err("image décodée de taille inattendue".into());
    }
    Ok(DecodedImage {
        width: w,
        height: h,
        rgba,
    })
}

/// Lit un fichier de taille bornée.
pub fn read_bounded(path: &Path) -> Result<Vec<u8>, String> {
    let meta = std::fs::metadata(path).map_err(|e| format!("fichier illisible : {e}"))?;
    if !meta.is_file() {
        return Err("ce chemin n'est pas un fichier".into());
    }
    if meta.len() > MAX_IMAGE_BYTES {
        return Err(format!(
            "fichier trop gros ({} Mo, maximum {} Mo)",
            meta.len() / 1_000_000,
            MAX_IMAGE_BYTES / 1_000_000
        ));
    }
    std::fs::read(path).map_err(|e| format!("fichier illisible : {e}"))
}

/// Image prête à être posée : copie faite, empreinte et dimensions lues.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportedImage {
    /// Chemin relatif au projet (`<nom-projet>.assets/<nom>`), séparateur `/`.
    pub relative: String,
    pub sha256: String,
    pub width_px: u32,
    pub height_px: u32,
}

/// Dossier voisin des images d'un projet : `jupe.pforge.json` -> `jupe.assets`.
pub fn assets_dir_name(project_file: &Path) -> Option<String> {
    let name = project_file.file_name()?.to_str()?;
    let stem = name
        .strip_suffix(".pforge.json")
        .or_else(|| name.strip_suffix(".json"))
        .unwrap_or(name);
    if stem.is_empty() {
        None
    } else {
        Some(format!("{stem}.assets"))
    }
}

fn sanitize_stem(source: &Path) -> String {
    let stem = source
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("image");
    let s: String = stem
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .take(40)
        .collect();
    if s.is_empty() {
        "image".into()
    } else {
        s
    }
}

/// Copie `source` à côté du projet (`project_file` = chemin du `.pforge.json` **enregistré**) et
/// retourne la référence à poser. Refus expliqué : fichier illisible, ni PNG ni JPEG, image vide ou
/// trop grande, projet sans chemin, copie impossible. Une copie déjà présente de même contenu est
/// réutilisée ; un fichier de même nom mais de contenu différent est refusé (jamais écrasé).
pub fn import_image(project_file: &Path, source: &Path) -> Result<ImportedImage, String> {
    let bytes = read_bounded(source)?;
    let (w, h, fmt) = probe(&bytes)?;
    let sha = sha256_hex(&bytes);
    let dir_name = assets_dir_name(project_file).ok_or("enregistrez d'abord le projet")?;
    let parent = project_file.parent().unwrap_or_else(|| Path::new("."));
    let file_name = format!(
        "{}-{}.{}",
        &sha[..12],
        sanitize_stem(source),
        fmt.extension()
    );
    let relative = format!("{dir_name}/{file_name}");
    if let Some(why) = unsafe_image_path(&relative) {
        return Err(format!("nom de dossier du projet inutilisable ({why})"));
    }
    let dir = parent.join(&dir_name);
    std::fs::create_dir_all(&dir).map_err(|e| format!("dossier des images : {e}"))?;
    let target = dir.join(&file_name);
    if target.exists() {
        let existing =
            std::fs::read(&target).map_err(|e| format!("copie existante illisible : {e}"))?;
        if sha256_hex(&existing) != sha {
            return Err(format!(
                "« {file_name} » existe déjà avec un autre contenu : rien n'a été écrasé"
            ));
        }
    } else {
        let tmp = dir.join(format!("{file_name}.tmp"));
        std::fs::write(&tmp, &bytes).map_err(|e| format!("copie de l'image : {e}"))?;
        if let Err(e) = std::fs::rename(&tmp, &target) {
            let _ = std::fs::remove_file(&tmp);
            return Err(format!("copie de l'image : {e}"));
        }
    }
    Ok(ImportedImage {
        relative,
        sha256: sha,
        width_px: w,
        height_px: h,
    })
}

/// État du fichier d'une image de fond.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageStatus {
    /// Le fichier existe et son empreinte est celle du projet.
    Ok,
    /// Fichier absent (ou chemin refusé).
    Missing,
    /// Fichier présent mais d'empreinte différente de celle du projet.
    Modified,
    /// Fichier présent mais illisible (message).
    Unreadable(String),
}

/// Chemin disque d'un chemin relatif **sûr** (`None` si le chemin est refusé par la validation).
pub fn resolve(project_dir: &Path, relative: &str) -> Option<PathBuf> {
    if unsafe_image_path(relative).is_some() {
        return None;
    }
    Some(
        relative
            .split('/')
            .fold(project_dir.to_path_buf(), |p, s| p.join(s)),
    )
}

/// Contrôle le fichier de `bg` dans `project_dir` (dossier du `.pforge.json`). Ne lit que les
/// octets (empreinte), ne décode rien ; jamais de panic.
pub fn status(project_dir: &Path, bg: &Background) -> ImageStatus {
    let Some(path) = resolve(project_dir, &bg.image) else {
        return ImageStatus::Missing;
    };
    match std::fs::metadata(&path) {
        Err(_) => return ImageStatus::Missing,
        Ok(m) if !m.is_file() => return ImageStatus::Missing,
        Ok(_) => {}
    }
    match read_bounded(&path) {
        Err(e) => ImageStatus::Unreadable(e),
        Ok(bytes) if sha256_hex(&bytes) == bg.sha256 => ImageStatus::Ok,
        Ok(_) => ImageStatus::Modified,
    }
}

/// Charge et décode l'image de `bg` ; refuse si l'empreinte ou les dimensions ne concordent pas.
pub fn load(project_dir: &Path, bg: &Background) -> Result<DecodedImage, String> {
    let path = resolve(project_dir, &bg.image).ok_or("chemin d'image refusé")?;
    let bytes = read_bounded(&path)?;
    if sha256_hex(&bytes) != bg.sha256 {
        return Err("l'image a été modifiée depuis la calibration (empreinte différente)".into());
    }
    let img = decode(&bytes)?;
    if img.width != bg.width_px || img.height != bg.height_px {
        return Err("dimensions de l'image différentes de celles du projet".into());
    }
    Ok(img)
}
