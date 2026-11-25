//! US-09.8b — export du tableau « mesures par taille » d'une gamme (`Project::size_measures`) :
//! CSV et PDF, lecture seule (aucune `Command`, aucun champ du projet, schéma 4 inchangé).
//!
//! Le tableau est celui du cœur : une ligne par mesure, une valeur par taille (celle de la
//! **géométrie évaluée de cette taille**, jamais celle de la base), le pas moyen en mm (mm²) et en
//! %, l'alerte d'irrégularité. Rien n'est recalculé ici hors la mise en forme. La sortie est
//! déterministe (ni date, ni heure) : mêmes entrées et même langue = mêmes octets.
//!
//! **CSV** (mêmes conventions que le rapport de contrôle : UTF-8 avec BOM, `;`, CR LF, point
//! décimal, protection contre l'injection de formule par `csv_text`) : section `resume` (projet,
//! gamme, taille de base, nombre de mesures et d'alertes, unités, précision, avertissement), puis
//! section `mesure` : `Mesure;Unité;<tailles…>;Pas moyen;Pas moyen (%);Alerte`, valeurs à 0,01
//! près, **cellule illisible vide**, alerte en texte (jamais seulement un indicateur de couleur).
//!
//! **PDF** : A4 portrait, même mise en page que le rapport de contrôle (`report::pdf_document`),
//! titre et nom de la gamme en tête, synthèse, puis le tableau. Valeurs : longueurs à 0,1 mm, aires
//! à 1 mm², **cellule illisible « — »**. Une ligne en alerte est suivie d'une ligne « (!) Irrégulier :
//! … (S-M) » : symbole **et** texte. Quand les tailles ne tiennent pas dans la largeur de la page,
//! le tableau est coupé en blocs de tailles consécutives (étiquette, unité et pas moyen répétés).

use crate::report::{
    clean, csv_bytes, nm, pdf_col, pdf_col_literal, pdf_document, tx, width_mm, Cell, PdfFrame,
    Row, Section, BODY_PT, MARGIN_MM, PAD_MM, PAGE_W_MM,
};
use crate::report::{ReportExport, ReportFormat};
use crate::ExportError;
use pf_core::{
    CoreError, EntityKind, MeasureKind, MeasureUnit, Project, SeamWhich, Sheet, SizeMeasureRow,
    SizeMeasures, SizeRangeId, IRREGULARITY_MEDIAN_RATIO,
};
use pf_i18n::{lookup, Lang};

/// Symbole d'une alerte dans le PDF (ASCII : présent dans la police embarquée) ; le texte de
/// l'alerte le suit toujours.
pub const ALERT_MARK: &str = "(!)";

/// Largeur (mm) des colonnes fixes du PDF : mesure, unité, pas moyen, pas moyen en %.
const W_LABEL: f64 = 42.0;
const W_UNIT: f64 = 10.0;
const W_MEAN: f64 = 17.0;
const W_PERCENT: f64 = 17.0;
/// Largeur minimale d'une colonne de taille.
const W_SIZE_MIN: f64 = 9.0;

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

fn unit_text(kind: &MeasureKind) -> &'static str {
    match kind.unit() {
        MeasureUnit::Mm => "mm",
        MeasureUnit::Mm2 => "mm²",
    }
}

/// Nom lisible d'un tracé (« Segment AB »), préfixe traduit ; les libellés de points viennent du
/// modèle.
fn entity_label(lang: Lang, sheet: &Sheet, id: pf_core::EntityId) -> String {
    let Ok(e) = sheet.entity(id) else {
        return t(lang, "entity.missing");
    };
    match &e.kind {
        EntityKind::Segment { from, to, .. } => format!(
            "{} {}{}",
            t(lang, "entity.segment"),
            sheet.label(*from),
            sheet.label(*to)
        ),
        EntityKind::Curve { start, end, .. } => format!(
            "{} {}{}",
            t(lang, "entity.curve"),
            sheet.label(*start),
            sheet.label(*end)
        ),
        EntityKind::Circle { center, .. } => {
            format!("{} {}", t(lang, "entity.circle"), sheet.label(*center))
        }
        EntityKind::Arc {
            center, start, end, ..
        } => format!(
            "{} {}{} ({} {})",
            t(lang, "entity.arc"),
            sheet.label(*start),
            sheet.label(*end),
            t(lang, "entity.arc.center"),
            sheet.label(*center)
        ),
        EntityKind::Spline { nodes } => format!(
            "{} {}…{}",
            t(lang, "entity.spline"),
            nodes.first().map_or("?", |n| sheet.label(n.point)),
            nodes.last().map_or("?", |n| sheet.label(n.point))
        ),
    }
}

fn piece_name(project: &Project, id: pf_core::PieceId) -> String {
    project
        .piece(id)
        .map(|p| p.name.clone())
        .unwrap_or_default()
}

fn pair_number(project: &Project, pair: pf_core::SeamPairId) -> usize {
    project
        .seams
        .iter()
        .position(|s| s.id == pair)
        .map_or(0, |i| i + 1)
}

/// Libellé d'une ligne du tableau (celui de la fenêtre « Mesures par taille »).
pub fn size_row_label(lang: Lang, project: &Project, kind: &MeasureKind) -> String {
    match *kind {
        MeasureKind::PiecePerimeter { piece } => ta(
            lang,
            "sizemeas.row.perimeter",
            &[("name", &piece_name(project, piece))],
        ),
        MeasureKind::PieceArea { piece } => ta(
            lang,
            "sizemeas.row.area",
            &[("name", &piece_name(project, piece))],
        ),
        MeasureKind::PieceCutPerimeter { piece } => ta(
            lang,
            "sizemeas.row.cut",
            &[("name", &piece_name(project, piece))],
        ),
        MeasureKind::SeamLength { pair, side } => {
            let seam = project.seams.iter().find(|s| s.id == pair);
            let (letter, piece) = match side {
                SeamWhich::A => ("A", seam.map(|s| s.a.piece)),
                SeamWhich::B => ("B", seam.map(|s| s.b.piece)),
            };
            ta(
                lang,
                "sizemeas.row.seam_len",
                &[
                    ("number", &pair_number(project, pair).to_string()),
                    ("side", letter),
                    (
                        "name",
                        &piece.map(|p| piece_name(project, p)).unwrap_or_default(),
                    ),
                ],
            )
        }
        MeasureKind::SeamGap { pair } => ta(
            lang,
            "sizemeas.row.seam_gap",
            &[("number", &pair_number(project, pair).to_string())],
        ),
        MeasureKind::PathLength { sheet, entity } => match project.sheet(sheet) {
            Ok(s) => {
                let name = entity_label(lang, s, entity);
                if project.sheets.len() > 1 {
                    ta(
                        lang,
                        "sizemeas.row.path_sheet",
                        &[("sheet", &s.name), ("name", &name)],
                    )
                } else {
                    name
                }
            }
            Err(_) => t(lang, "entity.missing"),
        },
    }
}

/// « S-M, M-L » : les pas en cause, par libellés de tailles.
fn steps_text(labels: &[String], steps: &[usize]) -> String {
    steps
        .iter()
        .map(|i| {
            format!(
                "{}-{}",
                labels.get(*i).map_or("?", String::as_str),
                labels.get(i + 1).map_or("?", String::as_str)
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Texte de l'alerte d'une ligne (`None` si elle n'est pas signalée) ; `labels` = étiquettes des
/// tailles dans l'ordre de la gamme.
pub fn size_alert_text(lang: Lang, row: &SizeMeasureRow, labels: &[String]) -> Option<String> {
    let irr = row.irregularity.as_ref()?;
    let steps = steps_text(labels, &irr.steps);
    Some(if irr.sign_change {
        ta(lang, "sizemeas.alert.sign", &[("steps", &steps)])
    } else {
        ta(
            lang,
            "sizemeas.alert.median",
            &[
                (
                    "percent",
                    &((IRREGULARITY_MEDIAN_RATIO * 100.0).round() as i64).to_string(),
                ),
                ("steps", &steps),
            ],
        )
    })
}

fn size_labels(project: &Project, range: SizeRangeId) -> Vec<String> {
    project
        .size_range(range)
        .map(|r| r.sizes.iter().map(|s| s.label.clone()).collect())
        .unwrap_or_default()
}

/// Valeur d'une cellule du PDF : aires à 1 mm² près, longueurs à 0,1 mm près (sans décimale
/// quand elle est nulle) ; « — » si la mesure n'est pas lisible dans cette taille.
fn pdf_value(kind: &MeasureKind, v: Option<f64>) -> String {
    let Some(v) = v.filter(|v| v.is_finite()) else {
        return "—".to_string();
    };
    match kind.unit() {
        MeasureUnit::Mm2 => {
            let s = format!("{v:.0}");
            if s == "-0" {
                "0".into()
            } else {
                s
            }
        }
        MeasureUnit::Mm => {
            let r = (v * 10.0).round() / 10.0;
            if r == 0.0 {
                "0".into()
            } else if r.fract() == 0.0 {
                format!("{r:.0}")
            } else {
                format!("{r:.1}")
            }
        }
    }
}

/// Cellule de pas moyen : chiffre à 0,01 près, vide en CSV et « — » en PDF si absent.
fn mean_cell(v: Option<f64>, pdf: bool) -> Cell {
    if pdf && v.filter(|v| v.is_finite()).is_none() {
        Cell::Num("—".to_string())
    } else {
        nm(v)
    }
}

fn rows(
    project: &Project,
    table: &SizeMeasures,
    labels: &[String],
    lang: Lang,
    pdf: bool,
) -> Vec<Row> {
    table
        .rows
        .iter()
        .map(|r| {
            let alert = size_alert_text(lang, r, labels);
            let mut cells = vec![
                tx(size_row_label(lang, project, &r.kind)),
                tx(unit_text(&r.kind)),
            ];
            cells.extend(r.values.iter().map(|v| {
                if pdf {
                    Cell::Num(pdf_value(&r.kind, *v))
                } else {
                    nm(*v)
                }
            }));
            cells.push(mean_cell(r.mean_step, pdf));
            cells.push(mean_cell(r.mean_step_percent, pdf));
            cells.push(tx(alert.clone().unwrap_or_default()));
            Row {
                cells,
                note: if pdf {
                    alert.map(|a| format!("{ALERT_MARK} {a}"))
                } else {
                    None
                },
            }
        })
        .collect()
}

fn summary(project: &Project, table: &SizeMeasures, lang: Lang, pdf: bool) -> Section {
    let range_name = project
        .size_range(table.range)
        .map(|r| r.name.clone())
        .unwrap_or_default();
    let labels = size_labels(project, table.range);
    let base = table
        .sizes
        .iter()
        .position(|s| *s == table.base)
        .and_then(|i| labels.get(i).cloned())
        .unwrap_or_default();
    let row = |k: &str, v: Cell| Row {
        cells: vec![tx(t(lang, k)), v],
        note: None,
    };
    let rows = vec![
        row("report.sum.project", tx(project.name.clone())),
        row("sizexp.sum.range", tx(range_name)),
        row("sizexp.sum.base", tx(base)),
        row("sizexp.sum.rows", Cell::Num(table.rows.len().to_string())),
        row(
            "sizexp.sum.alerts",
            Cell::Num(table.irregular_rows().count().to_string()),
        ),
        row("sizexp.sum.units", tx(t(lang, "sizexp.units"))),
        row(
            "sizexp.sum.precision",
            tx(t(
                lang,
                if pdf {
                    "sizexp.precision.pdf"
                } else {
                    "sizexp.precision.csv"
                },
            )),
        ),
        row("report.sum.notice", tx(t(lang, "sizexp.footer"))),
    ];
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

/// Section CSV : tous les tailles sur une ligne.
fn csv_table(project: &Project, table: &SizeMeasures, labels: &[String], lang: Lang) -> Section {
    let mut header = vec![
        t(lang, "sizemeas.csv.measure"),
        t(lang, "sizemeas.csv.unit"),
    ];
    header.extend(labels.iter().cloned());
    header.push(t(lang, "sizemeas.csv.trend"));
    header.push(t(lang, "sizexp.col.percent"));
    header.push(t(lang, "sizemeas.csv.alert"));
    Section {
        code: "mesure",
        title: String::new(),
        header,
        rows: rows(project, table, labels, lang, false),
        pdf: Vec::new(),
        empty: String::new(),
    }
}

/// Largeur d'une colonne de taille : le texte le plus large (étiquette ou valeur) et les marges.
fn size_col_width(rows: &[Row], index: usize, label: &str) -> f64 {
    let widest = rows
        .iter()
        .map(|r| width_mm(r.cells[index].text(), BODY_PT))
        .fold(width_mm(label, BODY_PT), f64::max);
    (widest + 2.0 * PAD_MM + 0.6).max(W_SIZE_MIN)
}

/// Sections du PDF : la synthèse, puis le tableau en blocs de tailles consécutives.
fn pdf_sections(
    project: &Project,
    table: &SizeMeasures,
    labels: &[String],
    lang: Lang,
) -> Vec<Section> {
    let all = rows(project, table, labels, lang, true);
    let n = labels.len();
    let widths: Vec<f64> = (0..n)
        .map(|i| size_col_width(&all, 2 + i, &labels[i]))
        .collect();
    let room = PAGE_W_MM - 2.0 * MARGIN_MM - W_LABEL - W_UNIT - W_MEAN - W_PERCENT;
    // Blocs gloutons : autant de tailles que la largeur le permet (au moins une).
    let mut blocks: Vec<(usize, usize)> = Vec::new();
    let mut start = 0;
    while start < n {
        let mut end = start;
        let mut used = 0.0;
        while end < n && (end == start || used + widths[end] <= room) {
            used += widths[end];
            end += 1;
        }
        blocks.push((start, end));
        start = end;
    }
    let mut out = vec![summary(project, table, lang, true)];
    let count = table.rows.len().to_string();
    for (lo, hi) in &blocks {
        let mut cols = vec![
            pdf_col("sizemeas.csv.measure", W_LABEL, &[0]),
            pdf_col("sizemeas.csv.unit", W_UNIT, &[1]),
        ];
        for i in *lo..*hi {
            cols.push(pdf_col_literal(&labels[i], widths[i], &[2 + i]));
        }
        cols.push(pdf_col("sizemeas.csv.trend", W_MEAN, &[2 + n]));
        cols.push(pdf_col("sizexp.col.percent", W_PERCENT, &[3 + n]));
        let title = if blocks.len() == 1 {
            ta(lang, "sizexp.sec.table", &[("count", &count)])
        } else {
            ta(
                lang,
                "sizexp.sec.block",
                &[
                    ("count", &count),
                    ("first", &labels[*lo]),
                    ("last", &labels[hi - 1]),
                ],
            )
        };
        out.push(Section {
            code: "mesure",
            title,
            header: Vec::new(),
            rows: all
                .iter()
                .map(|r| Row {
                    cells: r.cells.clone(),
                    note: r.note.clone(),
                })
                .collect(),
            pdf: cols,
            empty: t(lang, "report.empty"),
        });
    }
    out
}

/// Mise en forme d'un tableau déjà calculé (`Project::size_measures`) dans le format et la langue
/// demandés. `project` sert aux noms (pièces, feuilles, gamme) et ne doit être que celui dont le
/// tableau a été lu.
pub fn format_size_measures(
    project: &Project,
    table: &SizeMeasures,
    format: ReportFormat,
    lang: Lang,
) -> ReportExport {
    let labels = size_labels(project, table.range);
    match format {
        ReportFormat::Csv => ReportExport {
            format,
            bytes: csv_bytes(&[
                summary(project, table, lang, false),
                csv_table(project, table, &labels, lang),
            ]),
            page_count: None,
        },
        ReportFormat::Pdf => {
            let range_name = project
                .size_range(table.range)
                .map(|r| r.name.clone())
                .unwrap_or_default();
            let title = ta(lang, "sizexp.title", &[("range", &clean(&range_name))]);
            let frame = PdfFrame {
                doc_title: format!("{} - {}", clean(&project.name), title),
                title,
                continued: t(lang, "sizexp.continued"),
                footer: t(lang, "sizexp.footer"),
            };
            let (bytes, pages) =
                pdf_document(&frame, &pdf_sections(project, table, &labels, lang), lang);
            ReportExport {
                format,
                bytes,
                page_count: Some(pages),
            }
        }
    }
}

/// Export des mesures de la gamme `range` : lit `Project::size_measures` (lecture seule) puis met
/// en forme. Gamme inconnue ou sans taille : refus, rien n'est produit.
pub fn export_size_measures(
    project: &Project,
    range: SizeRangeId,
    format: ReportFormat,
    lang: Lang,
) -> Result<ReportExport, ExportError> {
    let table = project
        .size_measures(range)
        .map_err(|e| ExportError::Core(CoreError::from(e)))?;
    Ok(format_size_measures(project, &table, format, lang))
}
