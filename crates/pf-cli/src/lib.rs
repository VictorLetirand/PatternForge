//! `patternforge-cli` (US-18.2, US-18.4) : valider, convertir, exporter et traiter par lot.
//!
//! Aucun algorithme d'export ici : tout passe par les fonctions de `pf-export` que l'interface
//! appelle (`size_scene`, `export_svg_lang`, `export_pdf_lang`, `export_dxf_with`,
//! `export_hpgl_pages`, `raster::export_png/jpeg`). Les codes de retour sont documentés dans
//! `docs/CLI.md`.

mod export;
mod text;

use export::{Fmt, SizeSel};
use pf_core::diagnose::{classify, file_schema_version};
use pf_core::persist;
use pf_i18n::Lang;
use std::path::{Path, PathBuf};
use text::m;

pub const OK: u8 = 0;
pub const INVALID: u8 = 1;
pub const IO: u8 = 2;
pub const ARG: u8 = 3;
pub const UNKNOWN_SIZE: u8 = 4;

/// Échec porteur de son code de retour et de son message déjà localisé.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub code: u8,
    pub msg: String,
}

pub(crate) fn fail<T>(code: u8, msg: String) -> Result<T, Failure> {
    Err(Failure { code, msg })
}

#[derive(Default)]
struct Opts {
    positional: Vec<String>,
    format: Option<String>,
    vers: Option<String>,
    taille: Option<String>,
    tailles_toutes: bool,
    sortie: Option<String>,
    ecraser: bool,
    dpi: Option<String>,
    recursif: bool,
    rapport_csv: Option<String>,
}

fn lang_of(args: &[String]) -> Lang {
    args.windows(2)
        .find(|w| w[0] == "--langue")
        .and_then(|w| Lang::from_code(&w[1]))
        .unwrap_or(Lang::Fr)
}

fn parse(args: &[String], lang: Lang) -> Result<Opts, Failure> {
    let mut o = Opts::default();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut value = |name: &str| -> Result<String, Failure> {
            match it.next() {
                Some(v) => Ok(v.clone()),
                None => fail(ARG, m(lang, "cli.err.missing", &[("name", name)])),
            }
        };
        match a.as_str() {
            "--format" => o.format = Some(value(a)?),
            "--vers" => o.vers = Some(value(a)?),
            "--taille" => o.taille = Some(value(a)?),
            "--sortie" => o.sortie = Some(value(a)?),
            "--dpi" => o.dpi = Some(value(a)?),
            "--rapport-csv" => o.rapport_csv = Some(value(a)?),
            "--langue" => {
                let v = value(a)?;
                if Lang::from_code(&v).is_none() {
                    return fail(
                        ARG,
                        m(lang, "cli.err.bad_value", &[("name", a), ("value", &v)]),
                    );
                }
            }
            "--tailles-toutes" => o.tailles_toutes = true,
            "--ecraser" => o.ecraser = true,
            "--recursif" => o.recursif = true,
            s if s.starts_with("--") => {
                return fail(ARG, m(lang, "cli.err.unknown_option", &[("name", s)]))
            }
            _ => o.positional.push(a.clone()),
        }
    }
    Ok(o)
}

/// Point d'entrée : `args` sans le nom du programme ; écrit sur `out` / `err` et renvoie le code.
pub fn run(args: &[String], out: &mut dyn std::io::Write, err: &mut dyn std::io::Write) -> u8 {
    let lang = lang_of(args);
    let result = dispatch(args, lang, out);
    match result {
        Ok(code) => code,
        Err(f) => {
            let _ = writeln!(err, "patternforge-cli : {}", f.msg);
            f.code
        }
    }
}

fn dispatch(args: &[String], lang: Lang, out: &mut dyn std::io::Write) -> Result<u8, Failure> {
    let Some(cmd) = args.iter().find(|a| !a.starts_with("--")) else {
        let _ = writeln!(out, "{}", m(lang, "cli.usage", &[]));
        return if args.iter().any(|a| a == "--aide") {
            Ok(OK)
        } else {
            fail(ARG, m(lang, "cli.err.missing", &[("name", "commande")]))
        };
    };
    if args.iter().any(|a| a == "--aide") {
        let _ = writeln!(out, "{}", m(lang, "cli.usage", &[]));
        return Ok(OK);
    }
    let rest: Vec<String> = {
        let mut skipped = false;
        args.iter()
            .filter(|a| {
                if !skipped && *a == cmd {
                    skipped = true;
                    false
                } else {
                    true
                }
            })
            .cloned()
            .collect()
    };
    let o = parse(&rest, lang)?;
    match cmd.as_str() {
        "valider" => valider(&o, lang, out),
        "convertir" => {
            let fmt = match &o.vers {
                Some(v) => Fmt::parse(v, lang)?,
                None => return fail(ARG, m(lang, "cli.err.missing", &[("name", "--vers")])),
            };
            if matches!(fmt, Fmt::Png | Fmt::Jpeg) {
                return fail(ARG, m(lang, "cli.err.format", &[("value", fmt.ext())]));
            }
            exporter(&o, fmt, lang, out)
        }
        "exporter" => {
            let fmt = match &o.format {
                Some(v) => Fmt::parse(v, lang)?,
                None => return fail(ARG, m(lang, "cli.err.missing", &[("name", "--format")])),
            };
            exporter(&o, fmt, lang, out)
        }
        "lot" => lot(&o, lang, out),
        other => fail(ARG, m(lang, "cli.err.unknown_command", &[("name", other)])),
    }
}

fn one_file(o: &Opts, lang: Lang) -> Result<&str, Failure> {
    match o.positional.as_slice() {
        [f] => Ok(f),
        [] => fail(ARG, m(lang, "cli.err.missing", &[("name", "fichier")])),
        _ => fail(ARG, m(lang, "cli.err.too_many", &[])),
    }
}

/// Charge un projet ; une erreur du chargeur devient un code 1 (invalide) ou 2 (illisible).
pub(crate) fn load(path: &Path, lang: Lang) -> Result<pf_core::Project, Failure> {
    match persist::load(path) {
        Ok(p) => Ok(p),
        Err(e) => {
            let text = std::fs::read_to_string(path).ok();
            let text = text.as_deref().map(|t| t.trim_start_matches('\u{feff}'));
            let f = classify(&e, text, text.and_then(file_schema_version));
            let code = if f.kind == "lecture" { IO } else { INVALID };
            fail(code, describe(&f, path, lang))
        }
    }
}

fn describe(f: &pf_core::diagnose::Finding, path: &Path, lang: Lang) -> String {
    let place = match (f.line, f.column) {
        (Some(l), Some(c)) => m(
            lang,
            "cli.place",
            &[("line", &l.to_string()), ("column", &c.to_string())],
        ),
        _ => String::new(),
    };
    m(
        lang,
        "cli.invalid",
        &[
            ("file", &path.display().to_string()),
            ("kind", f.kind),
            ("detail", &f.message),
            ("place", &place),
        ],
    )
}

fn valider(o: &Opts, lang: Lang, out: &mut dyn std::io::Write) -> Result<u8, Failure> {
    let path = Path::new(one_file(o, lang)?);
    let p = load(path, lang)?;
    let _ = writeln!(
        out,
        "{}",
        m(
            lang,
            "cli.valid",
            &[
                ("file", &path.display().to_string()),
                ("name", &p.name),
                ("sheets", &p.sheets.len().to_string()),
                ("pieces", &p.pieces.len().to_string()),
            ],
        )
    );
    Ok(OK)
}

fn parse_dpi(o: &Opts, fmt: Fmt, lang: Lang) -> Result<u32, Failure> {
    let Some(v) = &o.dpi else {
        return Ok(300);
    };
    if !matches!(fmt, Fmt::Png | Fmt::Jpeg) {
        return fail(ARG, m(lang, "cli.err.dpi_format", &[]));
    }
    match v.parse::<u32>() {
        Ok(d) if (pf_export::raster::MIN_DPI..=pf_export::raster::MAX_DPI).contains(&d) => Ok(d),
        _ => fail(
            ARG,
            m(
                lang,
                "cli.err.dpi",
                &[
                    ("value", v),
                    ("min", &pf_export::raster::MIN_DPI.to_string()),
                    ("max", &pf_export::raster::MAX_DPI.to_string()),
                ],
            ),
        ),
    }
}

fn size_sel(o: &Opts, lang: Lang) -> Result<SizeSel, Failure> {
    match (&o.taille, o.tailles_toutes) {
        (Some(_), true) => fail(ARG, m(lang, "cli.err.size_both", &[])),
        (Some(n), false) => Ok(SizeSel::Named(n.clone())),
        (None, true) => Ok(SizeSel::All),
        (None, false) => Ok(SizeSel::Base),
    }
}

fn file_stem(path: &Path) -> String {
    let name = path
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    pf_export::size_export::sanitize_file_part(pf_core::project_stem(&name))
}

fn exporter(o: &Opts, fmt: Fmt, lang: Lang, out: &mut dyn std::io::Write) -> Result<u8, Failure> {
    let path = Path::new(one_file(o, lang)?);
    let dpi = parse_dpi(o, fmt, lang)?;
    let sel = size_sel(o, lang)?;
    let project = load(path, lang)?;
    let produced = export::produce(&project, &file_stem(path), fmt, &sel, lang, dpi)?;
    for s in &produced.skipped {
        let _ = writeln!(out, "{}", m(lang, "cli.skipped", &[("sheet", s)]));
    }
    let default_dir = path
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    let targets = resolve_targets(
        o.sortie.as_deref(),
        &default_dir,
        &produced.files,
        fmt,
        lang,
    )?;
    write_all(&targets, o.ecraser, lang)?;
    for (p, _) in &targets {
        let _ = writeln!(
            out,
            "{}",
            m(lang, "cli.written", &[("path", &p.display().to_string())])
        );
    }
    Ok(OK)
}

/// Chemins de sortie : `--sortie` est un dossier (créé au besoin) ou, s'il porte l'extension du
/// format et qu'un seul fichier est produit, le fichier lui-même.
fn resolve_targets(
    sortie: Option<&str>,
    default_dir: &Path,
    files: &[(String, Vec<u8>)],
    fmt: Fmt,
    lang: Lang,
) -> Result<Vec<(PathBuf, Vec<u8>)>, Failure> {
    let target = sortie.map(PathBuf::from);
    if let Some(t) = &target {
        let is_file = !t.is_dir()
            && t.extension()
                .is_some_and(|e| e.to_string_lossy().eq_ignore_ascii_case(fmt.ext()));
        if is_file {
            return match files {
                [(_, bytes)] => Ok(vec![(t.clone(), bytes.clone())]),
                _ => fail(
                    ARG,
                    m(
                        lang,
                        "cli.err.single_file",
                        &[("count", &files.len().to_string())],
                    ),
                ),
            };
        }
    }
    let dir = target.unwrap_or_else(|| default_dir.to_path_buf());
    Ok(files
        .iter()
        .map(|(name, bytes)| (dir.join(name), bytes.clone()))
        .collect())
}

/// Écrit tous les fichiers ; refuse **avant d'écrire quoi que ce soit** si l'un existe déjà
/// (sans `--ecraser`).
pub(crate) fn write_all(
    targets: &[(PathBuf, Vec<u8>)],
    overwrite: bool,
    lang: Lang,
) -> Result<(), Failure> {
    if !overwrite {
        if let Some((p, _)) = targets.iter().find(|(p, _)| p.exists()) {
            return fail(
                IO,
                m(
                    lang,
                    "cli.err.exists",
                    &[("path", &p.display().to_string())],
                ),
            );
        }
    }
    let io = |e: std::io::Error| Failure {
        code: IO,
        msg: m(lang, "cli.err.io", &[("detail", &e.to_string())]),
    };
    for (p, bytes) in targets {
        if let Some(dir) = p.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir).map_err(io)?;
        }
        std::fs::write(p, bytes).map_err(io)?;
    }
    Ok(())
}

// ----- lot -------------------------------------------------------------------------------------

struct Row {
    file: String,
    code: u8,
    files: usize,
    message: String,
}

fn collect(dir: &Path, recursive: bool, acc: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let p = entry?.path();
        if p.is_dir() {
            if recursive {
                collect(&p, true, acc)?;
            }
        } else if p.file_name().is_some_and(|n| {
            let n = n.to_string_lossy().to_lowercase();
            n.ends_with(".pforge") || n.ends_with(".pforge.json")
        }) {
            acc.push(p);
        }
    }
    Ok(())
}

fn csv_cell(s: &str) -> String {
    if s.contains([';', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

fn lot(o: &Opts, lang: Lang, out: &mut dyn std::io::Write) -> Result<u8, Failure> {
    let dir = Path::new(one_file(o, lang)?);
    let Some(sortie) = &o.sortie else {
        return fail(ARG, m(lang, "cli.err.missing", &[("name", "--sortie")]));
    };
    let fmt = match &o.format {
        Some(v) => Fmt::parse(v, lang)?,
        None => return fail(ARG, m(lang, "cli.err.missing", &[("name", "--format")])),
    };
    let dpi = parse_dpi(o, fmt, lang)?;
    let sel = size_sel(o, lang)?;
    if let Some(c) = &o.rapport_csv {
        if c.is_empty() || c.contains(['/', '\\']) || Path::new(c).file_name().is_none() {
            return fail(ARG, m(lang, "cli.err.csv_name", &[]));
        }
    }
    let io = |e: std::io::Error| Failure {
        code: IO,
        msg: m(lang, "cli.err.io", &[("detail", &e.to_string())]),
    };
    let mut found = Vec::new();
    collect(dir, o.recursif, &mut found).map_err(io)?;
    let rel = |p: &Path| {
        p.strip_prefix(dir)
            .unwrap_or(p)
            .to_string_lossy()
            .replace('\\', "/")
    };
    found.sort_by_key(|p| rel(p));
    let stems = pf_export::size_export::unique_file_names(
        &found.iter().map(|p| file_stem(p)).collect::<Vec<_>>(),
    );
    let out_dir = PathBuf::from(sortie);
    let mut rows: Vec<Row> = Vec::new();
    for (p, stem) in found.iter().zip(&stems) {
        let file = rel(p);
        let result = load(p, lang)
            .and_then(|project| export::produce(&project, stem, fmt, &sel, lang, dpi))
            .and_then(|produced| {
                let targets: Vec<(PathBuf, Vec<u8>)> = produced
                    .files
                    .into_iter()
                    .map(|(n, b)| (out_dir.join(n), b))
                    .collect();
                write_all(&targets, o.ecraser, lang).map(|()| targets.len())
            });
        rows.push(match result {
            Ok(n) => Row {
                file,
                code: OK,
                files: n,
                message: String::new(),
            },
            Err(f) => Row {
                file,
                code: f.code,
                files: 0,
                message: f.msg,
            },
        });
    }
    if rows.is_empty() {
        let _ = writeln!(
            out,
            "{}",
            m(
                lang,
                "cli.batch.none",
                &[("dir", &dir.display().to_string())]
            )
        );
    }
    let mut failed = 0;
    for r in &rows {
        if r.code == OK {
            let _ = writeln!(
                out,
                "{}",
                m(
                    lang,
                    "cli.batch.ok",
                    &[("file", &r.file), ("count", &r.files.to_string())]
                )
            );
        } else {
            failed += 1;
            let _ = writeln!(
                out,
                "{}",
                m(
                    lang,
                    "cli.batch.fail",
                    &[
                        ("file", &r.file),
                        ("code", &r.code.to_string()),
                        ("detail", &r.message)
                    ]
                )
            );
        }
    }
    let _ = writeln!(
        out,
        "{}",
        m(
            lang,
            "cli.batch.summary",
            &[
                ("ok", &(rows.len() - failed).to_string()),
                ("failed", &failed.to_string()),
                ("total", &rows.len().to_string()),
            ]
        )
    );
    if let Some(c) = &o.rapport_csv {
        let mut csv = format!("{}\n", m(lang, "cli.batch.csv_head", &[]));
        for r in &rows {
            csv.push_str(&format!(
                "{};{};{};{};{}\n",
                csv_cell(&r.file),
                if r.code == OK { "ok" } else { "echec" },
                r.code,
                r.files,
                csv_cell(&r.message.replace('\n', " "))
            ));
        }
        write_all(&[(out_dir.join(c), csv.into_bytes())], o.ecraser, lang)?;
    }
    Ok(if failed > 0 { INVALID } else { OK })
}
