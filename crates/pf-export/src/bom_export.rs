//! US-11.2 nomenclature et US-11.3 fiche technique (tranche 111) : lecture seule, rien n'est stocké.
//!
//! La nomenclature est celle du cœur (`Project::bill_of_materials`) : une ligne par pièce, un total
//! par matière (découpes et surface nette par taille). Mise en forme avec le module du rapport de
//! contrôle (`report`) : mêmes conventions CSV (UTF-8 avec BOM, `;`, CR LF, point décimal,
//! `csv_text`) et même mise en page PDF A4 portrait. Sortie déterministe (ni date ni heure).
//!
//! **CSV** : `resume`, `piece` (une ligne par pièce, aire à 0,01 mm² près par taille, cellule
//! illisible vide), `matiere` (total par matière, surface nette par taille).
//! **PDF** : synthèse, tableau des pièces, tableau des matières, aires par pièce et par taille ;
//! les tailles qui ne tiennent pas dans la largeur sont coupées en blocs. Texte ASCII ou Latin-1.
//!
//! **Par variante (US-11.1)** : `export_bom_variant` ; la synthèse nomme la variante et ses options
//! actives, le tableau des pièces compte les entrées de la variante (option entre crochets après le
//! nom de la pièce). Sans variante choisie, sortie identique à celle d'avant (octets).
//!
//! La **fiche technique** est un PDF : synthèse, vignettes vectorielles des pièces (chacune
//! ajustée à sa case, **pas à l'échelle**, dit dans le titre), tableau de mesures du barème E10 de
//! la gamme (« non renseigné » quand la valeur manque, jamais devinée) et zone de commentaires :
//! commentaires des pièces et note saisie à l'export (non stockée) ; cadre **vide** sinon.

use crate::report::{
    clean, csv_bytes, nm, pdf_col, pdf_col_literal, pdf_document_items, tx, width_mm, Cell,
    GraphicBlock, Item, PdfFrame, Row, Section, BODY_PT, MARGIN_MM, PAD_MM, PAGE_W_MM,
};
use crate::report::{ReportExport, ReportFormat};
use crate::ExportError;
use pf_core::{Bom, MeasurementUnit, PieceCategory, Project, Shape, SizeRangeId, VariantId};
use pf_geometry::PointMm;
use pf_i18n::{lookup, Lang};

fn t(lang: Lang, key: &str) -> String {
    lookup(lang, key)
}

fn ta(lang: Lang, key: &str, args: &[(&str, &str)]) -> String {
    let mut s = lookup(lang, key);
    for (n, v) in args {
        s = s.replace(&format!("{{{n}}}"), v);
    }
    s
}

fn category_text(lang: Lang, c: Option<PieceCategory>) -> String {
    let key = match c {
        None => return String::new(),
        Some(PieceCategory::Fabric) => "piece.category.fabric",
        Some(PieceCategory::Lining) => "piece.category.lining",
        Some(PieceCategory::Interfacing) => "piece.category.interfacing",
        Some(PieceCategory::Leather) => "piece.category.leather",
        Some(PieceCategory::Reinforcement) => "piece.category.reinforcement",
    };
    t(lang, key)
}

fn material_text(lang: Lang, m: &Option<String>) -> String {
    m.clone().unwrap_or_else(|| t(lang, "bom.unset"))
}

/// Étiquettes des colonnes de taille : celles de la gamme, ou « Projet » sans gamme.
fn column_labels(bom: &Bom, lang: Lang) -> Vec<String> {
    if bom.labels.is_empty() {
        vec![t(lang, "bom.size.base")]
    } else {
        bom.labels.clone()
    }
}

fn range_name(project: &Project, range: Option<SizeRangeId>) -> String {
    range
        .and_then(|r| project.size_range(r).ok())
        .map(|r| r.name.clone())
        .unwrap_or_default()
}

/// Aire du PDF : mm² entiers ; « — » si illisible.
fn pdf_area(v: Option<f64>) -> String {
    match v.filter(|v| v.is_finite()) {
        None => "—".to_string(),
        Some(v) => {
            let s = format!("{v:.0}");
            if s == "-0" {
                "0".into()
            } else {
                s
            }
        }
    }
}

fn area_cell(v: Option<f64>, pdf: bool) -> Cell {
    if pdf {
        Cell::Num(pdf_area(v))
    } else {
        nm(v)
    }
}

fn summary(project: &Project, bom: &Bom, lang: Lang) -> Section {
    let row = |k: &str, v: Cell| Row {
        cells: vec![tx(t(lang, k)), v],
        note: None,
    };
    let unreadable = bom
        .lines
        .iter()
        .filter(|l| l.areas_mm2.iter().any(Option::is_none))
        .count();
    let chosen = bom.variant.and_then(|id| project.variant(id).ok());
    let variant_row = match chosen {
        Some(v) => row("bom.sum.variants", tx(v.name.clone())),
        // Sans variante dans le projet : texte historique (octets inchangés).
        None if project.variants.is_empty() => {
            row("bom.sum.variants", tx(t(lang, "bom.variants.none")))
        }
        None => row("bom.sum.variants", tx(t(lang, "bom.variants.all"))),
    };
    let mut rows = vec![
        row("report.sum.project", tx(project.name.clone())),
        row("sizexp.sum.range", tx(range_name(project, bom.range))),
        row("bom.sum.pieces", Cell::Num(bom.lines.len().to_string())),
        row(
            "bom.sum.materials",
            Cell::Num(bom.materials.len().to_string()),
        ),
        row("bom.sum.unreadable", Cell::Num(unreadable.to_string())),
        variant_row,
    ];
    if let Some(v) = chosen {
        let active = if v.options_active.is_empty() {
            t(lang, "bom.options.none")
        } else {
            v.options_active.join(", ")
        };
        rows.push(row("bom.sum.options", tx(active)));
    }
    rows.push(row("bom.sum.rule", tx(t(lang, "bom.rule"))));
    rows.push(row("report.sum.notice", tx(t(lang, "bom.footer"))));
    Section {
        code: "resume",
        title: t(lang, "report.sec.summary"),
        header: vec![t(lang, "report.col.element"), t(lang, "report.col.value")],
        rows,
        pdf: vec![
            pdf_col("report.col.element", 55.0, &[0]),
            pdf_col("report.col.value", 125.0, &[1]),
        ],
        empty: String::new(),
    }
}

fn piece_rows(bom: &Bom, lang: Lang, pdf: bool) -> Vec<Row> {
    bom.lines
        .iter()
        .map(|l| {
            let name = match &l.option {
                Some(o) => format!("{} [{o}]", l.name),
                None => l.name.clone(),
            };
            let mut cells = vec![
                tx(name),
                tx(l.code.clone().unwrap_or_default()),
                tx(l.sheet.clone()),
                tx(material_text(lang, &l.material)),
                tx(category_text(lang, l.category)),
                Cell::Num(l.quantity.to_string()),
                tx(if l.mirrored_pair {
                    t(lang, "bom.yes")
                } else {
                    String::new()
                }),
                Cell::Num(l.cuts.to_string()),
            ];
            cells.extend(l.areas_mm2.iter().map(|a| area_cell(*a, pdf)));
            Row { cells, note: None }
        })
        .collect()
}

fn material_rows(bom: &Bom, lang: Lang, pdf: bool) -> Vec<Row> {
    bom.materials
        .iter()
        .map(|m| {
            let mut cells = vec![
                tx(material_text(lang, &m.material)),
                Cell::Num(m.pieces.to_string()),
                Cell::Num(m.cuts.to_string()),
            ];
            cells.extend(m.net_area_mm2.iter().map(|a| area_cell(*a, pdf)));
            Row { cells, note: None }
        })
        .collect()
}

fn piece_header(lang: Lang, labels: &[String]) -> Vec<String> {
    let mut h: Vec<String> = [
        "bom.col.piece",
        "bom.col.code",
        "bom.col.sheet",
        "bom.col.material",
        "bom.col.category",
        "bom.col.qty",
        "bom.col.pair",
        "bom.col.cuts",
    ]
    .iter()
    .map(|k| t(lang, k))
    .collect();
    h.extend(
        labels
            .iter()
            .map(|l| ta(lang, "bom.col.area", &[("size", l)])),
    );
    h
}

fn material_header(lang: Lang, labels: &[String]) -> Vec<String> {
    let mut h: Vec<String> = ["bom.col.material", "bom.col.pieces", "bom.col.cuts"]
        .iter()
        .map(|k| t(lang, k))
        .collect();
    h.extend(
        labels
            .iter()
            .map(|l| ta(lang, "bom.col.net", &[("size", l)])),
    );
    h
}

fn csv_sections(project: &Project, bom: &Bom, lang: Lang) -> Vec<Section> {
    let labels = column_labels(bom, lang);
    vec![
        summary(project, bom, lang),
        Section {
            code: "piece",
            title: String::new(),
            header: piece_header(lang, &labels),
            rows: piece_rows(bom, lang, false),
            pdf: Vec::new(),
            empty: String::new(),
        },
        Section {
            code: "matiere",
            title: String::new(),
            header: material_header(lang, &labels),
            rows: material_rows(bom, lang, false),
            pdf: Vec::new(),
            empty: String::new(),
        },
    ]
}

/// Largeur d'une colonne de taille : le texte le plus large (étiquette ou valeur) et les marges.
fn size_width(rows: &[Row], index: usize, label: &str) -> f64 {
    rows.iter()
        .map(|r| width_mm(r.cells[index].text(), BODY_PT))
        .fold(width_mm(label, BODY_PT), f64::max)
        + 2.0 * PAD_MM
        + 0.6
}

/// Titre d'un bloc : None s'il n'y a qu'un bloc, sinon (première, dernière) taille du bloc.
type TitleFn<'a> = dyn Fn(Option<(&str, &str)>) -> String + 'a;

/// Un tableau du PDF dont les `fixed.len()` premières colonnes sont fixes et le reste une colonne
/// par taille, coupé en blocs de tailles consécutives.
fn size_blocks(
    code: &'static str,
    title: &TitleFn<'_>,
    fixed: &[(&str, f64)],
    rows: &[Row],
    labels: &[String],
    lang: Lang,
) -> Vec<Section> {
    let nf = fixed.len();
    let widths: Vec<f64> = labels
        .iter()
        .enumerate()
        .map(|(i, l)| size_width(rows, nf + i, l).max(12.0))
        .collect();
    let room = PAGE_W_MM - 2.0 * MARGIN_MM - fixed.iter().map(|f| f.1).sum::<f64>();
    let mut blocks: Vec<(usize, usize)> = Vec::new();
    let mut start = 0;
    while start < labels.len() {
        let mut end = start;
        let mut used = 0.0;
        while end < labels.len() && (end == start || used + widths[end] <= room) {
            used += widths[end];
            end += 1;
        }
        blocks.push((start, end));
        start = end;
    }
    blocks
        .iter()
        .map(|(lo, hi)| {
            let mut cols: Vec<_> = fixed
                .iter()
                .enumerate()
                .map(|(i, (k, w))| pdf_col(k, *w, &[i]))
                .collect();
            for i in *lo..*hi {
                cols.push(pdf_col_literal(&labels[i], widths[i], &[nf + i]));
            }
            Section {
                code,
                title: if blocks.len() == 1 {
                    title(None)
                } else {
                    title(Some((&labels[*lo], &labels[hi - 1])))
                },
                header: Vec::new(),
                rows: rows
                    .iter()
                    .map(|r| Row {
                        cells: r.cells.clone(),
                        note: None,
                    })
                    .collect(),
                pdf: cols,
                empty: t(lang, "report.empty"),
            }
        })
        .collect()
}

fn pdf_sections(project: &Project, bom: &Bom, lang: Lang) -> Vec<Section> {
    let labels = column_labels(bom, lang);
    let mut out = vec![summary(project, bom, lang)];
    let prow = piece_rows(bom, lang, true);
    let count = bom.lines.len().to_string();
    // Pièces : colonnes fixes seules (les aires par taille ont leur propre tableau).
    let fixed_rows: Vec<Row> = prow
        .iter()
        .map(|r| Row {
            cells: r.cells[..8].to_vec(),
            note: None,
        })
        .collect();
    out.push(Section {
        code: "piece",
        title: ta(lang, "bom.sec.pieces", &[("count", &count)]),
        header: Vec::new(),
        rows: fixed_rows,
        pdf: vec![
            pdf_col("bom.col.piece", 34.0, &[0]),
            pdf_col("bom.col.code", 18.0, &[1]),
            pdf_col("bom.col.sheet", 24.0, &[2]),
            pdf_col("bom.col.material", 30.0, &[3]),
            pdf_col("bom.col.category", 24.0, &[4]),
            pdf_col("bom.col.qty", 14.0, &[5]),
            pdf_col("bom.col.pair", 16.0, &[6]),
            pdf_col("bom.col.cuts", 20.0, &[7]),
        ],
        empty: t(lang, "report.empty"),
    });
    let mcount = bom.materials.len().to_string();
    let mrows = material_rows(bom, lang, true);
    out.extend(size_blocks(
        "matiere",
        &|b| match b {
            None => ta(lang, "bom.sec.materials", &[("count", &mcount)]),
            Some((a, z)) => ta(
                lang,
                "bom.sec.materials_block",
                &[("count", &mcount), ("first", a), ("last", z)],
            ),
        },
        &[
            ("bom.col.material", 46.0),
            ("bom.col.pieces", 16.0),
            ("bom.col.cuts", 20.0),
        ],
        &mrows,
        &labels,
        lang,
    ));
    // Aires par pièce et par taille : nom puis une colonne par taille.
    let arows: Vec<Row> = prow
        .iter()
        .map(|r| {
            let mut cells = vec![r.cells[0].clone()];
            cells.extend(r.cells[8..].iter().cloned());
            Row { cells, note: None }
        })
        .collect();
    out.extend(size_blocks(
        "aire",
        &|b| match b {
            None => ta(lang, "bom.sec.areas", &[("count", &count)]),
            Some((a, z)) => ta(
                lang,
                "bom.sec.areas_block",
                &[("count", &count), ("first", a), ("last", z)],
            ),
        },
        &[("bom.col.piece", 50.0)],
        &arows,
        &labels,
        lang,
    ));
    out
}

fn frame(project: &Project, bom: &Bom, lang: Lang) -> PdfFrame {
    let name = range_name(project, bom.range);
    let title = if bom.range.is_some() {
        ta(lang, "bom.title", &[("range", &clean(&name))])
    } else {
        t(lang, "bom.title.base")
    };
    PdfFrame {
        doc_title: format!("{} - {}", clean(&project.name), title),
        title,
        continued: t(lang, "bom.continued"),
        footer: t(lang, "bom.footer"),
    }
}

/// Mise en forme d'une nomenclature déjà calculée.
pub fn format_bom(project: &Project, bom: &Bom, format: ReportFormat, lang: Lang) -> ReportExport {
    match format {
        ReportFormat::Csv => ReportExport {
            format,
            bytes: csv_bytes(&csv_sections(project, bom, lang)),
            page_count: None,
        },
        ReportFormat::Pdf => {
            let sections = pdf_sections(project, bom, lang);
            let items: Vec<Item> = sections.iter().map(Item::Table).collect();
            let (bytes, pages) = pdf_document_items(&frame(project, bom, lang), &items, lang);
            ReportExport {
                format,
                bytes,
                page_count: Some(pages),
            }
        }
    }
}

/// Export de la nomenclature (lecture seule). Gamme inconnue ou sans taille : refus.
pub fn export_bom(
    project: &Project,
    range: Option<SizeRangeId>,
    format: ReportFormat,
    lang: Lang,
) -> Result<ReportExport, ExportError> {
    export_bom_variant(project, range, None, format, lang)
}

/// Export de la nomenclature d'une variante (`Some`) ou de tout le projet (`None`, identique à
/// [`export_bom`]). Variante inconnue : refus.
pub fn export_bom_variant(
    project: &Project,
    range: Option<SizeRangeId>,
    variant: Option<VariantId>,
    format: ReportFormat,
    lang: Lang,
) -> Result<ReportExport, ExportError> {
    let bom = project.bill_of_materials_variant(range, variant)?;
    Ok(format_bom(project, &bom, format, lang))
}

// ---------------------------------------------------------------------------
// US-11.3 — fiche technique
// ---------------------------------------------------------------------------

const THUMB_COLS: usize = 4;
const THUMB_CELL_W: f64 = 45.0;
const THUMB_DRAW_H: f64 = 36.0;
const THUMB_CELL_H: f64 = 44.0;
const THUMB_ROWS_PER_BLOCK: usize = 5;
const THUMB_PAD: f64 = 3.0;
const THUMB_STROKE_MM: f64 = 0.25;

fn shape_points(s: &Shape) -> Vec<PointMm> {
    match s {
        Shape::Line(a, b) => vec![*a, *b],
        Shape::Cubic(c) => c.sample(24),
        Shape::Arc(a) => a.sample(24),
        Shape::Circle(c) => c.sample(48),
        Shape::Spline(sp) => sp.sample(12),
    }
}

/// Polylignes du contour d'une pièce (une par tracé), repère du modèle.
fn piece_polylines(project: &Project, piece: pf_core::PieceId) -> Vec<Vec<PointMm>> {
    let Ok(p) = project.piece(piece) else {
        return Vec::new();
    };
    let Ok(sheet) = project.piece_sheet(piece) else {
        return Vec::new();
    };
    p.outline
        .iter()
        .filter_map(|e| sheet.shape(e.entity).ok())
        .map(|s| shape_points(&s))
        .filter(|pts| pts.iter().all(|q| q.x.is_finite() && q.y.is_finite()))
        .collect()
}

fn truncate_to(text: &str, max_mm: f64) -> String {
    let text = clean(text);
    if width_mm(&text, BODY_PT) <= max_mm {
        return text;
    }
    let mut out = String::new();
    for ch in text.chars() {
        let mut next = out.clone();
        next.push(ch);
        next.push_str("...");
        if width_mm(&next, BODY_PT) > max_mm {
            break;
        }
        out.push(ch);
    }
    out.push_str("...");
    out
}

/// Blocs de vignettes : une case par pièce, contour ajusté à la case (échelle propre à chaque
/// pièce), nom sous le dessin.
fn thumbnail_blocks(project: &Project, lang: Lang) -> Vec<GraphicBlock> {
    let per_block = THUMB_COLS * THUMB_ROWS_PER_BLOCK;
    let title = t(lang, "tech.sec.thumbs");
    let pieces: Vec<&pf_core::Piece> = project.pieces.iter().collect();
    if pieces.is_empty() {
        return Vec::new();
    }
    pieces
        .chunks(per_block)
        .map(|chunk| {
            let rows = chunk.len().div_ceil(THUMB_COLS);
            let mut polys = Vec::new();
            let mut labels = Vec::new();
            for (i, p) in chunk.iter().enumerate() {
                let (cx, cy) = (
                    (i % THUMB_COLS) as f64 * THUMB_CELL_W,
                    (i / THUMB_COLS) as f64 * THUMB_CELL_H,
                );
                let lines = piece_polylines(project, p.id);
                let all: Vec<PointMm> = lines.iter().flatten().copied().collect();
                if all.is_empty() {
                    labels.push((
                        cx + THUMB_PAD,
                        cy + THUMB_DRAW_H / 2.0,
                        BODY_PT,
                        t(lang, "tech.unreadable"),
                    ));
                } else {
                    let (minx, maxx) = all
                        .iter()
                        .fold((f64::MAX, f64::MIN), |(a, b), q| (a.min(q.x), b.max(q.x)));
                    let (miny, maxy) = all
                        .iter()
                        .fold((f64::MAX, f64::MIN), |(a, b), q| (a.min(q.y), b.max(q.y)));
                    let (w, h) = ((maxx - minx).max(1e-9), (maxy - miny).max(1e-9));
                    let box_w = THUMB_CELL_W - 2.0 * THUMB_PAD;
                    let box_h = THUMB_DRAW_H - 2.0 * THUMB_PAD;
                    let scale = (box_w / w).min(box_h / h);
                    let (ox, oy) = (
                        cx + THUMB_PAD + (box_w - w * scale) / 2.0,
                        cy + THUMB_PAD + (box_h - h * scale) / 2.0,
                    );
                    for line in lines {
                        let pts = line
                            .iter()
                            .map(|q| {
                                PointMm::new(ox + (q.x - minx) * scale, oy + (maxy - q.y) * scale)
                            })
                            .collect();
                        polys.push((pts, THUMB_STROKE_MM));
                    }
                }
                labels.push((
                    cx + THUMB_PAD,
                    cy + THUMB_DRAW_H + 3.0,
                    BODY_PT,
                    truncate_to(&p.name, THUMB_CELL_W - 2.0 * THUMB_PAD),
                ));
            }
            GraphicBlock {
                title: title.clone(),
                height: rows as f64 * THUMB_CELL_H,
                polys,
                labels,
            }
        })
        .collect()
}

fn measure_unit(u: MeasurementUnit) -> &'static str {
    match u {
        MeasurementUnit::Mm => "mm",
        MeasurementUnit::Deg => "deg",
    }
}

/// Tableau de mesures du barème E10 pour les tailles de `range` : une ligne par mesure, valeur
/// par taille à 0,1 près (« non renseigné » si absente). Rien si le barème est vide ou sans gamme.
fn measure_sections(project: &Project, range: Option<SizeRangeId>, lang: Lang) -> Vec<Section> {
    let sizes = range.and_then(|r| project.size_range(r).ok());
    let unset = t(lang, "tech.unset");
    let (Some(r), false) = (sizes, project.measurements.is_empty()) else {
        return vec![Section {
            code: "mesure",
            title: t(lang, "tech.sec.measures_empty"),
            header: Vec::new(),
            rows: Vec::new(),
            pdf: Vec::new(),
            empty: t(lang, "tech.measures.none"),
        }];
    };
    let labels: Vec<String> = r.sizes.iter().map(|s| s.label.clone()).collect();
    let rows: Vec<Row> =
        project
            .measurements
            .iter()
            .map(|m| {
                let mut cells = vec![tx(m.name.clone()), tx(measure_unit(m.unit))];
                cells.extend(r.sizes.iter().map(
                    |s| match m.values.iter().find(|v| v.size == s.id) {
                        Some(v) if v.value.is_finite() => {
                            let x = (v.value * 10.0).round() / 10.0;
                            Cell::Num(if x.fract() == 0.0 {
                                format!("{x:.0}")
                            } else {
                                format!("{x:.1}")
                            })
                        }
                        _ => Cell::Num(unset.clone()),
                    },
                ));
                Row { cells, note: None }
            })
            .collect();
    let count = rows.len().to_string();
    size_blocks(
        "mesure",
        &|b| match b {
            None => ta(lang, "tech.sec.measures", &[("count", &count)]),
            Some((a, z)) => ta(
                lang,
                "tech.sec.measures_block",
                &[("count", &count), ("first", a), ("last", z)],
            ),
        },
        &[("bom.col.measure", 56.0), ("bom.col.unit", 12.0)],
        &rows,
        &labels,
        lang,
    )
}

/// Commentaires saisis : ceux des pièces (fiche de pièce) puis la note de l'export.
fn comment_rows(project: &Project, note: Option<&str>, lang: Lang) -> Vec<Row> {
    let mut rows: Vec<Row> = project
        .pieces
        .iter()
        .filter_map(|p| {
            let c = p
                .comment
                .as_deref()
                .map(str::trim)
                .filter(|c| !c.is_empty())?;
            Some(Row {
                cells: vec![tx(p.name.clone()), tx(c.replace(['\r', '\n'], " "))],
                note: None,
            })
        })
        .collect();
    if let Some(n) = note.map(str::trim).filter(|n| !n.is_empty()) {
        rows.push(Row {
            cells: vec![
                tx(t(lang, "tech.note.label")),
                tx(n.replace(['\r', '\n'], " ")),
            ],
            note: None,
        });
    }
    rows
}

/// Fiche technique (PDF) : lecture seule. `note` est un texte saisi à l'export, jamais stocké.
pub fn export_tech_sheet(
    project: &Project,
    range: Option<SizeRangeId>,
    note: Option<&str>,
    lang: Lang,
) -> Result<ReportExport, ExportError> {
    let bom = project.bill_of_materials(range)?;
    let mut items: Vec<Section> = Vec::new();
    let base = range
        .and_then(|r| project.size_range(r).ok())
        .and_then(|r| {
            r.sizes
                .iter()
                .find(|s| s.id == r.base)
                .map(|s| s.label.clone())
        })
        .unwrap_or_default();
    let row = |k: &str, v: Cell| Row {
        cells: vec![tx(t(lang, k)), v],
        note: None,
    };
    let or_unset = |s: String| {
        if s.trim().is_empty() {
            t(lang, "tech.unset")
        } else {
            s
        }
    };
    items.push(Section {
        code: "resume",
        title: t(lang, "report.sec.summary"),
        header: Vec::new(),
        rows: vec![
            row("report.sum.project", tx(or_unset(project.name.clone()))),
            row("sizexp.sum.range", tx(or_unset(range_name(project, range)))),
            row("sizexp.sum.base", tx(or_unset(base))),
            row("bom.sum.pieces", Cell::Num(bom.lines.len().to_string())),
            row(
                "tech.sum.sheets",
                Cell::Num(project.sheets.len().to_string()),
            ),
            row(
                "bom.sum.materials",
                Cell::Num(bom.materials.len().to_string()),
            ),
            row("report.sum.notice", tx(t(lang, "tech.footer"))),
        ],
        pdf: vec![
            pdf_col("report.col.element", 55.0, &[0]),
            pdf_col("report.col.value", 125.0, &[1]),
        ],
        empty: String::new(),
    });
    let thumbs = thumbnail_blocks(project, lang);
    let measures = measure_sections(project, range, lang);
    let comments = comment_rows(project, note, lang);
    let comment_section = Section {
        code: "commentaire",
        title: t(lang, "tech.sec.comments"),
        header: Vec::new(),
        rows: comments,
        pdf: vec![
            pdf_col("bom.col.piece", 40.0, &[0]),
            pdf_col("tech.col.comment", 140.0, &[1]),
        ],
        empty: String::new(),
    };
    let mut seq: Vec<Item> = vec![Item::Table(&items[0])];
    seq.extend(thumbs.into_iter().map(Item::Graphic));
    seq.extend(measures.iter().map(Item::Table));
    if comment_section.rows.is_empty() {
        seq.push(Item::Graphic(empty_comment_box(lang)));
    } else {
        seq.push(Item::Table(&comment_section));
    }
    let title = t(lang, "tech.title");
    let frame = PdfFrame {
        doc_title: format!("{} - {}", clean(&project.name), title),
        title,
        continued: t(lang, "tech.continued"),
        footer: t(lang, "tech.footer"),
    };
    let (bytes, pages) = pdf_document_items(&frame, &seq, lang);
    Ok(ReportExport {
        format: ReportFormat::Pdf,
        bytes,
        page_count: Some(pages),
    })
}

/// Cadre vide de la zone de commentaires (rien n'a été saisi).
fn empty_comment_box(lang: Lang) -> GraphicBlock {
    let (w, h) = (PAGE_W_MM - 2.0 * MARGIN_MM, 30.0);
    GraphicBlock {
        title: t(lang, "tech.sec.comments"),
        height: h + 2.0,
        polys: vec![(
            vec![
                PointMm::new(0.0, 0.0),
                PointMm::new(w, 0.0),
                PointMm::new(w, h),
                PointMm::new(0.0, h),
                PointMm::new(0.0, 0.0),
            ],
            0.2,
        )],
        labels: Vec::new(),
    }
}
