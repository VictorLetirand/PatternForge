//! Production des fichiers d'export d'un projet, par les fonctions de `pf-export`.

use crate::text::m;
use crate::{fail, Failure, INVALID, UNKNOWN_SIZE};
use pf_core::{Project, SizeId};
use pf_export::content::OutputContent;
use pf_export::hpgl::{export_hpgl_pages, HpglOptions};
use pf_export::page::PageSetup;
use pf_export::size_export::{
    export_sizes, sanitize_file_part, size_project, size_scene, unique_file_names,
};
use pf_export::{dxf, pdf, raster, svg, ExportError, Scene, SceneOptions};
use pf_i18n::Lang;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fmt {
    Pdf,
    Svg,
    Dxf,
    Hpgl,
    Png,
    Jpeg,
}

impl Fmt {
    pub fn parse(v: &str, lang: Lang) -> Result<Fmt, Failure> {
        match v.to_lowercase().as_str() {
            "pdf" => Ok(Fmt::Pdf),
            "svg" => Ok(Fmt::Svg),
            "dxf" => Ok(Fmt::Dxf),
            "hpgl" => Ok(Fmt::Hpgl),
            "png" => Ok(Fmt::Png),
            "jpeg" | "jpg" => Ok(Fmt::Jpeg),
            _ => fail(crate::ARG, m(lang, "cli.err.format", &[("value", v)])),
        }
    }

    pub fn ext(self) -> &'static str {
        match self {
            Fmt::Pdf => "pdf",
            Fmt::Svg => "svg",
            Fmt::Dxf => "dxf",
            Fmt::Hpgl => "plt",
            Fmt::Png => "png",
            Fmt::Jpeg => "jpg",
        }
    }
}

pub enum SizeSel {
    Base,
    All,
    Named(String),
}

pub struct Produced {
    /// (nom de fichier, octets), noms uniques sans tenir compte de la casse.
    pub files: Vec<(String, Vec<u8>)>,
    /// Feuilles vides ignorées.
    pub skipped: Vec<String>,
}

fn refuse(detail: &dyn std::fmt::Display, lang: Lang) -> Failure {
    Failure {
        code: INVALID,
        msg: m(lang, "cli.err.export", &[("detail", &detail.to_string())]),
    }
}

type SizeChoice = (Option<SizeId>, Option<String>, bool);

fn sizes_of(project: &Project, sel: &SizeSel, lang: Lang) -> Result<Vec<SizeChoice>, Failure> {
    let sizes = export_sizes(project);
    match sel {
        SizeSel::Base => Ok(vec![(None, None, false)]),
        SizeSel::All if sizes.is_empty() => Ok(vec![(None, None, false)]),
        SizeSel::All => Ok(sizes
            .into_iter()
            .map(|s| (Some(s.id), Some(s.label), true))
            .collect()),
        SizeSel::Named(n) => {
            if sizes.is_empty() {
                return fail(UNKNOWN_SIZE, m(lang, "cli.err.size_none", &[]));
            }
            match sizes
                .iter()
                .find(|s| s.label.eq_ignore_ascii_case(n.trim()))
            {
                Some(s) if s.is_base => Ok(vec![(Some(s.id), None, false)]),
                Some(s) => Ok(vec![(Some(s.id), Some(s.label.clone()), true)]),
                None => {
                    let list: Vec<&str> = sizes.iter().map(|s| s.label.as_str()).collect();
                    fail(
                        UNKNOWN_SIZE,
                        m(
                            lang,
                            "cli.err.size_unknown",
                            &[("name", n), ("list", &list.join(", "))],
                        ),
                    )
                }
            }
        }
    }
}

pub fn produce(
    project: &Project,
    stem: &str,
    fmt: Fmt,
    sel: &SizeSel,
    lang: Lang,
    dpi: u32,
) -> Result<Produced, Failure> {
    let content = OutputContent::default();
    let setup = PageSetup::default();
    let options = SceneOptions {
        lang,
        ..SceneOptions::default()
    };
    let mut raw: Vec<(String, Vec<u8>)> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    for (size, label, suffix) in sizes_of(project, sel, lang)? {
        let base = match (&label, suffix) {
            (Some(l), true) => format!("{stem}-{}", sanitize_file_part(l)),
            _ => stem.to_string(),
        };
        if fmt == Fmt::Dxf {
            let read = size_project(project, size).map_err(|e| refuse(&e, lang))?;
            let out =
                dxf::export_dxf_with(&read, content.piece_mode).map_err(|e| refuse(&e, lang))?;
            raw.push((base, out.bytes().to_vec()));
            continue;
        }
        let mut scenes: Vec<(String, Scene)> = Vec::new();
        for sheet in &project.sheets {
            match size_scene(project, size, sheet.id, &content, options) {
                Ok(s) => scenes.push((sheet.name.clone(), s)),
                Err(ExportError::EmptySheet(_) | ExportError::NothingSelected(_)) => {
                    skipped.push(sheet.name.clone());
                }
                Err(e) => return Err(refuse(&e, lang)),
            }
        }
        if scenes.is_empty() {
            return fail(INVALID, m(lang, "cli.err.nothing", &[]));
        }
        if fmt == Fmt::Pdf {
            let list: Vec<Scene> = scenes.into_iter().map(|(_, s)| s).collect();
            let out = pdf::export_pdf_lang(&list, &setup, &content, lang)
                .map_err(|e| refuse(&e, lang))?;
            raw.push((base, out.bytes));
            continue;
        }
        let multi = scenes.len() > 1;
        for (sheet_name, scene) in &scenes {
            let name = if multi {
                format!("{base}-{}", sanitize_file_part(sheet_name))
            } else {
                base.clone()
            };
            match fmt {
                Fmt::Svg => {
                    let t = svg::export_svg_lang(scene, lang).map_err(|e| refuse(&e, lang))?;
                    raw.push((name, t.into_bytes()));
                }
                Fmt::Png => raw.push((
                    name,
                    raster::export_png(scene, dpi).map_err(|e| refuse(&e, lang))?,
                )),
                Fmt::Jpeg => raw.push((
                    name,
                    raster::export_jpeg(scene, dpi).map_err(|e| refuse(&e, lang))?,
                )),
                Fmt::Hpgl => {
                    let tiles = export_hpgl_pages(scene, &setup, HpglOptions::default())
                        .map_err(|e| refuse(&e, lang))?;
                    for page in &tiles.pages {
                        let n = if page.total > 1 {
                            format!("{name}-p{}", page.number)
                        } else {
                            name.clone()
                        };
                        raw.push((n, page.bytes().to_vec()));
                    }
                }
                Fmt::Pdf | Fmt::Dxf => unreachable!("traités plus haut"),
            }
        }
    }
    let names = unique_file_names(&raw.iter().map(|(n, _)| n.clone()).collect::<Vec<_>>());
    let files = names
        .into_iter()
        .zip(raw)
        .map(|(n, (_, b))| (format!("{n}.{}", fmt.ext()), b))
        .collect();
    Ok(Produced { files, skipped })
}
