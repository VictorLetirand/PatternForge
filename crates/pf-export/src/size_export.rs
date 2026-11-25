//! US-09.8 — export gradé : sortie d'une taille, d'une sélection de tailles ou du nid.
//!
//! **Lecture seule** : rien n'est écrit dans le projet (schéma 4 inchangé). Une taille exportée est
//! le projet évalué par `Project::graded_project` ; il passe ensuite par **le même chemin** que
//! l'export de base (`OutputContent::scene`, `svg::export_svg`, `pdf::export_pdf_content`, DXF,
//! HPGL). Une taille qui est la base de sa gamme (ou « aucune taille ») lit le projet tel quel :
//! la sortie est alors identique à l'octet à l'export d'avant.
//!
//! Conventions (à faire valider, `SUIVI_RESTE_A_FAIRE.md` § 6) :
//! - nom de fichier d'une taille autre que la base : `<nom habituel>-<taille>` (libellé assaini :
//!   tout caractère qui n'est ni lettre, ni chiffre, ni `-`, ni `_` devient `_`) ;
//! - sélection de tailles : **un fichier par taille**, `<projet>-<taille>.<ext>` (la base aussi),
//!   noms rendus uniques sans tenir compte de la casse (`-2`, `-3`…) ; jamais de PDF multi-pages ;
//! - en-tête : le titre de la feuille devient « Taille L · <feuille> » (titre du SVG, en-tête des
//!   pages du PDF) quand la taille n'est pas la base ;
//! - nid : un fichier unique, contours des tailles cochées translatés comme `Project::size_nest`,
//!   motif de trait **découpé en tirets réels** par taille (la base en plein) et étiquette de
//!   taille au bout d'un trait de rappel : jamais la couleur seule (les exports n'ont pas de
//!   couleur), échelle 1:1 en mm.

use crate::content::OutputContent;
use crate::page::PageSetup;
use crate::{
    pdf, svg, ExportError, PathGeom, PathRole, Scene, SceneOptions, ScenePath, ScenePoint,
};
use pf_core::{GrainSense, Project, SheetId, SizeId, SizeNest};
use pf_geometry::PointMm;
use std::borrow::Cow;
use std::path::{Path, PathBuf};

/// Une taille exportable (toutes les gammes du projet).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportSize {
    pub id: SizeId,
    pub label: String,
    /// Nom de la gamme.
    pub range: String,
    /// Taille de base de sa gamme.
    pub is_base: bool,
}

/// Toutes les tailles du projet, gamme après gamme, dans l'ordre de chaque gamme.
pub fn export_sizes(project: &Project) -> Vec<ExportSize> {
    project
        .sizes
        .iter()
        .flat_map(|r| {
            r.sizes.iter().map(move |s| ExportSize {
                id: s.id,
                label: s.label.clone(),
                range: r.name.clone(),
                is_base: r.base == s.id,
            })
        })
        .collect()
}

/// Une taille du projet, si elle existe.
pub fn export_size(project: &Project, id: SizeId) -> Option<ExportSize> {
    export_sizes(project).into_iter().find(|s| s.id == id)
}

/// Taille exportée par défaut : la taille de base de la première gamme (`None` sans gamme).
pub fn default_export_size(project: &Project) -> Option<SizeId> {
    project.sizes.first().map(|r| r.base)
}

/// Projet lu par l'export d'une taille : le projet lui-même pour « aucune taille » ou une taille
/// de base (aucune copie, aucun octet de différence), sinon le projet évalué de la taille.
pub fn size_project(
    project: &Project,
    size: Option<SizeId>,
) -> Result<Cow<'_, Project>, ExportError> {
    let Some(id) = size else {
        return Ok(Cow::Borrowed(project));
    };
    match export_size(project, id) {
        Some(s) if s.is_base => Ok(Cow::Borrowed(project)),
        // Une taille inconnue : l'erreur du cœur (« taille inconnue »), jamais la base en silence.
        _ => Ok(Cow::Owned(project.graded_project(Some(id))?)),
    }
}

/// Titre d'une feuille exportée dans une taille : « Taille L · Jupe » / « Size L · Jupe ».
pub fn size_sheet_title(lang: pf_i18n::Lang, sheet_name: &str, size_label: &str) -> String {
    pf_i18n::lookup(lang, "export.size.sheet")
        .replace("{size}", size_label)
        .replace("{sheet}", sheet_name)
}

/// Scène d'une feuille dans une taille : même chemin que l'export de base (contenu de sortie
/// compris) sur le projet évalué ; le titre porte le nom de la taille quand elle n'est pas la base.
pub fn size_scene(
    project: &Project,
    size: Option<SizeId>,
    sheet: SheetId,
    content: &OutputContent,
    options: SceneOptions,
) -> Result<Scene, ExportError> {
    let read = size_project(project, size)?;
    let mut scene = content.scene(&read, sheet, options)?;
    if let Some(info) = size
        .and_then(|id| export_size(project, id))
        .filter(|s| !s.is_base)
    {
        scene.sheet_name = size_sheet_title(options.lang, &scene.sheet_name, &info.label);
    }
    Ok(scene)
}

/// Partie de nom de fichier : lettres, chiffres, `-` et `_` gardés, le reste devient `_`.
pub fn sanitize_file_part(text: &str) -> String {
    let s: String = text
        .trim()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let s = s.trim().to_string();
    if s.is_empty() {
        "x".to_string()
    } else {
        s
    }
}

/// Nom (sans extension) d'un export : `stem` pour la base ou « aucune taille », sinon
/// `<stem>-<taille assainie>`.
pub fn size_file_stem(stem: &str, size: Option<&ExportSize>) -> String {
    match size {
        Some(s) if !s.is_base => format!("{stem}-{}", sanitize_file_part(&s.label)),
        _ => stem.to_string(),
    }
}

/// Noms rendus uniques **sans tenir compte de la casse** (un système de fichiers de Windows les
/// confondrait) : le deuxième « XL » devient « XL-2 », puis « XL-3 »…
pub fn unique_file_names(names: &[String]) -> Vec<String> {
    let mut seen: Vec<String> = Vec::new();
    let mut out = Vec::with_capacity(names.len());
    for name in names {
        let mut candidate = name.clone();
        let mut n = 1;
        while seen.contains(&candidate.to_lowercase()) {
            n += 1;
            candidate = format!("{name}-{n}");
        }
        seen.push(candidate.to_lowercase());
        out.push(candidate);
    }
    out
}

/// Format d'un fichier par taille.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SizeFormat {
    Pdf,
    Svg,
}

impl SizeFormat {
    pub fn extension(self) -> &'static str {
        match self {
            SizeFormat::Pdf => "pdf",
            SizeFormat::Svg => "svg",
        }
    }
}

/// Un fichier calculé pour une taille : ses octets, ou l'erreur de cette taille.
#[derive(Debug, Clone, PartialEq)]
pub struct SizeFile {
    pub size: SizeId,
    pub label: String,
    /// Nom de fichier (sans dossier), unique dans la sélection.
    pub file_name: String,
    pub bytes: Result<Vec<u8>, ExportError>,
}

/// Fichiers d'une sélection de tailles : un par taille (doublons de la sélection ignorés), dans
/// l'ordre donné ; l'échec d'une taille n'arrête pas les autres.
#[allow(clippy::too_many_arguments)]
pub fn size_files(
    project: &Project,
    sheet: SheetId,
    sizes: &[SizeId],
    stem: &str,
    format: SizeFormat,
    setup: &PageSetup,
    content: &OutputContent,
    options: SceneOptions,
) -> Vec<SizeFile> {
    let mut ids: Vec<SizeId> = Vec::new();
    for id in sizes {
        if !ids.contains(id) {
            ids.push(*id);
        }
    }
    let labels: Vec<String> = ids
        .iter()
        .map(|id| export_size(project, *id).map_or_else(|| format!("id{}", id.0), |s| s.label))
        .collect();
    let stems: Vec<String> = labels
        .iter()
        .map(|l| format!("{stem}-{}", sanitize_file_part(l)))
        .collect();
    let stems = unique_file_names(&stems);
    ids.iter()
        .zip(labels)
        .zip(stems)
        .map(|((id, label), name)| {
            let bytes = size_scene(project, Some(*id), sheet, content, options).and_then(|scene| {
                match format {
                    SizeFormat::Svg => {
                        svg::export_svg_lang(&scene, options.lang).map(String::into_bytes)
                    }
                    SizeFormat::Pdf => pdf::export_pdf_lang(
                        std::slice::from_ref(&scene),
                        setup,
                        content,
                        options.lang,
                    )
                    .map(|out| out.bytes),
                }
            });
            SizeFile {
                size: *id,
                label,
                file_name: format!("{name}.{}", format.extension()),
                bytes,
            }
        })
        .collect()
}

/// Un fichier qui n'a pas pu être produit ou écrit.
#[derive(Debug, Clone, PartialEq)]
pub struct SizeFailure {
    pub size: SizeId,
    pub label: String,
    pub file_name: String,
    pub error: ExportError,
}

/// Résultat de l'écriture d'une sélection : fichiers écrits et échecs (jamais de perte muette).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SizeWriteReport {
    pub written: Vec<(SizeId, PathBuf)>,
    pub failed: Vec<SizeFailure>,
}

/// Écrit les fichiers dans `dir` (le dossier choisi par l'utilisateur, rien ailleurs : les noms
/// sont assainis, sans séparateur de chemin). Un échec n'empêche pas les autres tailles.
pub fn write_size_files(dir: &Path, files: &[SizeFile]) -> SizeWriteReport {
    let mut report = SizeWriteReport::default();
    for f in files {
        let result = match &f.bytes {
            Ok(bytes) => std::fs::write(dir.join(&f.file_name), bytes)
                .map_err(|e| ExportError::Io(e.to_string())),
            Err(e) => Err(e.clone()),
        };
        match result {
            Ok(()) => report.written.push((f.size, dir.join(&f.file_name))),
            Err(error) => report.failed.push(SizeFailure {
                size: f.size,
                label: f.label.clone(),
                file_name: f.file_name.clone(),
                error,
            }),
        }
    }
    report
}

// ----- nid ------------------------------------------------------------------------------------

/// Motifs de trait (longueur du trait puis du vide, en mm, alternés) des tailles autres que la
/// base, dans l'ordre de leur rang dans la gamme ; la base est en plein. Mêmes motifs que le plan.
pub const NEST_PATTERNS_MM: [&[f64]; 6] = [
    &[8.0, 5.0],
    &[1.5, 3.0],
    &[10.0, 3.0, 1.5, 3.0],
    &[18.0, 5.0],
    &[10.0, 3.0, 1.5, 3.0, 1.5, 3.0],
    &[3.0, 2.5],
];

/// Écart (mm) entre la colonne des étiquettes et le plus à droite des contours.
pub const NEST_LABEL_GAP_MM: f64 = 12.0;
/// Pas vertical minimal (mm) entre deux étiquettes de la colonne.
pub const NEST_LABEL_PITCH_MM: f64 = 6.0;
/// Demi-longueur (mm) du bras gauche de la croix du point d'empilement.
pub const NEST_ANCHOR_ARM_MM: f64 = 3.0;

/// Le nid mis en scène, et ce qui ne se calcule pas dans une taille (taille, message du cœur).
#[derive(Debug, Clone, PartialEq)]
pub struct NestExport {
    pub scene: Scene,
    pub issues: Vec<(String, String)>,
}

/// Motif de la taille de rang `rank` parmi les tailles autres que la base (`None` = base : plein).
pub fn nest_pattern(rank: Option<usize>) -> &'static [f64] {
    match rank {
        None => &[],
        Some(i) => NEST_PATTERNS_MM[i % NEST_PATTERNS_MM.len()],
    }
}

fn line(a: PointMm, b: PointMm) -> ScenePath {
    ScenePath {
        geom: PathGeom::Line(a, b),
        role: PathRole::Construction,
        sense: GrainSense::Unmarked,
    }
}

/// Découpe une polyligne en tirets réels (`pattern` : trait, vide, trait, vide…, nombre pair) ;
/// motif vide = tous les tronçons. Un tiret qui franchit un sommet donne plusieurs segments.
pub fn dash_polyline(points: &[PointMm], closed: bool, pattern: &[f64]) -> Vec<(PointMm, PointMm)> {
    let mut chain: Vec<PointMm> = points.to_vec();
    if closed {
        if let Some(first) = chain.first().copied() {
            chain.push(first);
        }
    }
    let mut out = Vec::new();
    let (mut idx, mut rest) = (0usize, pattern.first().copied().unwrap_or(0.0));
    for w in chain.windows(2) {
        let (a, b) = (w[0], w[1]);
        let len = pf_geometry::distance_mm(a, b);
        if len < 1e-9 {
            continue;
        }
        if pattern.is_empty() {
            out.push((a, b));
            continue;
        }
        let (ux, uy) = ((b.x - a.x) / len, (b.y - a.y) / len);
        let at = |d: f64| PointMm::new(a.x + ux * d, a.y + uy * d);
        let mut pos = 0.0;
        while pos < len - 1e-12 {
            let take = rest.min(len - pos);
            if idx % 2 == 0 && take > 1e-12 {
                out.push((at(pos), at(pos + take)));
            }
            pos += take;
            rest -= take;
            if rest <= 1e-12 {
                idx += 1;
                rest = pattern[idx % pattern.len()];
            }
        }
    }
    out
}

fn text_width_mm(text: &str) -> f64 {
    2.0 + 1.9 * text.chars().count() as f64
}

/// Scène du nid : contours des tailles `shown` (dans l'ordre de la gamme), motif de trait par
/// taille, étiquette de taille dans une colonne à droite reliée par un trait de rappel, croix et
/// nom du point d'empilement. Coordonnées de la feuille, en mm, échelle 1:1.
pub fn nest_scene(
    project: &Project,
    nest: &SizeNest,
    shown: &[SizeId],
    lang: pf_i18n::Lang,
) -> Result<NestExport, ExportError> {
    let sheet = project.sheet(nest.sheet)?;
    let range = project
        .size_range(nest.range)
        .map_err(|e| ExportError::Core(e.into()))?;
    let label_of = |id: SizeId| {
        range
            .size(id)
            .map_or_else(|| format!("id{}", id.0), |s| s.label.clone())
    };
    let mut paths: Vec<ScenePath> = Vec::new();
    let mut issues: Vec<(String, String)> = Vec::new();
    // (point d'attache, texte) des étiquettes ; le rang du motif ne dépend pas des tailles cochées.
    let mut tags: Vec<(PointMm, String)> = Vec::new();
    let mut rank = 0usize;
    for ns in &nest.sizes {
        let pattern = if ns.is_base {
            nest_pattern(None)
        } else {
            rank += 1;
            nest_pattern(Some(rank - 1))
        };
        if !shown.contains(&ns.size) {
            continue;
        }
        let label = label_of(ns.size);
        issues.extend(ns.issues.iter().map(|m| (label.clone(), m.clone())));
        for o in &ns.outlines {
            for (a, b) in dash_polyline(&o.points, o.closed, pattern) {
                paths.push(line(a, b));
            }
        }
        // Attache de l'étiquette : le point le plus à droite (puis le plus haut) du contour.
        let attach = ns
            .outlines
            .iter()
            .flat_map(|o| o.points.iter().copied())
            .reduce(|best, p| {
                if p.x > best.x || (p.x == best.x && p.y > best.y) {
                    p
                } else {
                    best
                }
            });
        if let Some(at) = attach {
            let text = if ns.is_base {
                pf_i18n::lookup(lang, "nest.tag.base").replace("{label}", &label)
            } else {
                label
            };
            tags.push((at, text));
        }
    }
    if tags.is_empty() {
        return Err(ExportError::NothingSelected(sheet.name.clone()));
    }
    // Colonne des étiquettes : de haut en bas, au pas minimal, à droite de tous les contours.
    let right = tags.iter().map(|t| t.0.x).fold(f64::MIN, f64::max);
    let column = right + NEST_LABEL_GAP_MM;
    let mut order: Vec<usize> = (0..tags.len()).collect();
    order.sort_by(|a, b| {
        tags[*b]
            .0
            .y
            .total_cmp(&tags[*a].0.y)
            .then(tags[*a].0.x.total_cmp(&tags[*b].0.x))
    });
    let mut points: Vec<ScenePoint> = Vec::new();
    let mut previous: Option<f64> = None;
    for i in order {
        let (at, text) = &tags[i];
        let y = previous.map_or(at.y, |p| at.y.min(p - NEST_LABEL_PITCH_MM));
        previous = Some(y);
        let anchor = PointMm::new(column, y);
        paths.push(line(*at, anchor));
        paths.push(line(anchor, PointMm::new(column + text_width_mm(text), y)));
        points.push(ScenePoint {
            pos: anchor,
            label: text.clone(),
            show_label: true,
        });
    }
    // Point d'empilement : croix dont le bras droit souligne son nom.
    let stack = nest.anchor_position;
    let name = pf_i18n::lookup(lang, "export.nest.anchor");
    paths.push(line(
        PointMm::new(stack.x - NEST_ANCHOR_ARM_MM, stack.y),
        PointMm::new(stack.x + text_width_mm(&name), stack.y),
    ));
    paths.push(line(
        PointMm::new(stack.x, stack.y - NEST_ANCHOR_ARM_MM),
        PointMm::new(stack.x, stack.y + NEST_ANCHOR_ARM_MM),
    ));
    points.push(ScenePoint {
        pos: stack,
        label: name,
        show_label: true,
    });
    let mut scene = Scene::calibration_square();
    scene.project_name = project.name.clone();
    scene.sheet_name = pf_i18n::lookup(lang, "export.nest.sheet")
        .replace("{range}", &range.name)
        .replace("{sheet}", &sheet.name);
    scene.paths = paths;
    scene.points = points;
    Ok(NestExport { scene, issues })
}

/// Octets du nid en SVG.
pub fn nest_svg(export: &NestExport) -> Result<Vec<u8>, ExportError> {
    nest_svg_lang(export, pf_i18n::Lang::Fr)
}

/// Octets du nid en SVG, titre dans la langue `lang` (US-19.2).
pub fn nest_svg_lang(export: &NestExport, lang: pf_i18n::Lang) -> Result<Vec<u8>, ExportError> {
    svg::export_svg_lang(&export.scene, lang).map(String::into_bytes)
}

/// Octets du nid en PDF (réglage de page donné, contenu par défaut : le nid n'est pas filtré).
pub fn nest_pdf(export: &NestExport, setup: &PageSetup) -> Result<Vec<u8>, ExportError> {
    nest_pdf_lang(export, setup, pf_i18n::Lang::Fr)
}

/// Octets du nid en PDF, textes dans la langue lang (US-19.2).
pub fn nest_pdf_lang(
    export: &NestExport,
    setup: &PageSetup,
    lang: pf_i18n::Lang,
) -> Result<Vec<u8>, ExportError> {
    pdf::export_pdf_lang(
        std::slice::from_ref(&export.scene),
        setup,
        &OutputContent::default(),
        lang,
    )
    .map(|out| out.bytes)
}

/// Nom (sans extension) du fichier du nid : `<projet>-<nid>-<gamme>` (parties assainies ; le mot
/// « nid » vient de la langue).
pub fn nest_file_stem(stem: &str, range_name: &str, lang: pf_i18n::Lang) -> String {
    format!(
        "{stem}-{}-{}",
        sanitize_file_part(&pf_i18n::lookup(lang, "export.nest.file")),
        sanitize_file_part(range_name)
    )
}
