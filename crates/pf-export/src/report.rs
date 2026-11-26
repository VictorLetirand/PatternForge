//! US-08.5 — rapport de contrôle : mise en forme CSV et PDF du modèle `ControlReport` de
//! `pf-core` (lecture seule : aucune `Command`, aucun champ du projet, rien n'est stocké).
//!
//! Toutes les valeurs sont des **mesures observées** ; le rapport ne dit jamais qu'un patron est
//! « bon » : son pied de page (PDF) et sa dernière ligne de synthèse (CSV) le rappellent. La
//! sortie est déterministe (ni date, ni heure) : mêmes entrées et même langue = mêmes octets.
//!
//! **CSV** (un seul fichier) : UTF-8 avec BOM, séparateur `;`, fin de ligne CR LF, champs entre
//! guillemets (`"` doublé) quand ils contiennent `;`, `"`, CR ou LF, point décimal, valeurs en
//! mm (ou degrés) à 0,01 près, champ vide quand une valeur n'est pas calculable (jamais `NaN`).
//! Première colonne = code de section (`resume`, `anomalie`, `piece`, `couture`, `crans`,
//! `raccord`) ; une ligne d'en-tête par section, dont la première cellule est `#` + le code.
//! Un champ **texte** qui commence par `=`, `+`, `-`, `@`, tabulation ou retour chariot est
//! précédé d'une apostrophe (injection de formule) ; les nombres ne le sont jamais.
//!
//! **PDF** : A4 portrait fixe (le réglage de page de l'application est fait pour le patron 1:1,
//! pas pour ce rapport), marges de 15 mm, police Noto Sans embarquée (caractère absent = `?`),
//! une page n'est jamais dépassée (coupe de ligne d'après les largeurs réelles de la police,
//! en-tête de tableau répété), pied de page « mesures observées, non validées… — page n/N ».
//! Ordre : synthèse, **anomalies en premier**, pièces, coutures, crans, raccords, puis (US-10.4,
//! seulement s'il y en a) le tableau `mesure_finie` : valeur mesurée, référence du barème dans la
//! taille de base de la première gamme (retouches comprises), écart signé, tolérances, statut ;
//! il n'ajoute aucune anomalie au récapitulatif.

use crate::font::embedded;
use crate::pdf::{assemble, Content, PdfPage};
use pf_core::control_report::{Anomaly, AnomalyKind, ControlReport};
use pf_core::notch_check::NotchMatch;
use pf_core::piece::PieceIssue;
use pf_core::seam_junction::{JunctionEnd, JunctionStatus};
use pf_core::seam_pair::{SeamStatus, SeamWhich};
use pf_core::{
    FinishedReason, FinishedStatus, MeasurementUnit, PieceCategory, Project, StitchMatch,
};
use pf_i18n::{lookup, Lang};
use std::path::Path;

/// Format de sortie.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportFormat {
    Pdf,
    Csv,
}

impl ReportFormat {
    /// Extension de fichier, sans point.
    pub fn extension(self) -> &'static str {
        match self {
            ReportFormat::Pdf => "pdf",
            ReportFormat::Csv => "csv",
        }
    }
}

/// Échec d'écriture du rapport : aucun fichier n'est laissé.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReportError {
    /// Destination vide ou sans nom de fichier.
    NoDestination,
    /// Le système a refusé l'écriture (dossier absent, droits, disque…).
    Write(String),
}

impl std::fmt::Display for ReportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReportError::NoDestination => write!(f, "destination vide"),
            ReportError::Write(d) => write!(f, "{d}"),
        }
    }
}

/// Rapport formaté, prêt à écrire.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportExport {
    pub format: ReportFormat,
    pub bytes: Vec<u8>,
    /// Nombre de pages (PDF seulement).
    pub page_count: Option<usize>,
}

impl ReportExport {
    /// Écrit le fichier dans un fichier temporaire voisin puis le renomme : une destination
    /// illisible donne une erreur et aucun fichier (ni partiel, ni temporaire).
    pub fn write_to(&self, path: &Path) -> Result<(), ReportError> {
        use std::io::Write;
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .filter(|n| !n.is_empty())
            .ok_or(ReportError::NoDestination)?;
        pf_core::persist::refuse_read_only(path).map_err(|e| ReportError::Write(e.to_string()))?;
        let tmp = path.with_file_name(format!("{name}.pf-tmp-{}", std::process::id()));
        let result = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)
            .and_then(|mut f| f.write_all(&self.bytes).and_then(|()| f.flush()))
            .and_then(|()| std::fs::rename(&tmp, path));
        if let Err(e) = result {
            let _ = std::fs::remove_file(&tmp);
            return Err(ReportError::Write(e.to_string()));
        }
        Ok(())
    }
}

/// Rapport de contrôle d'un projet dans le format et la langue demandés (lecture seule).
pub fn export_control_report(project: &Project, format: ReportFormat, lang: Lang) -> ReportExport {
    format_control_report(&project.control_report(), format, lang)
}

/// Mise en forme d'un rapport déjà calculé.
pub fn format_control_report(
    report: &ControlReport,
    format: ReportFormat,
    lang: Lang,
) -> ReportExport {
    let sections = sections(report, lang);
    match format {
        ReportFormat::Csv => ReportExport {
            format,
            bytes: csv_bytes(&sections),
            page_count: None,
        },
        ReportFormat::Pdf => {
            let (bytes, pages) = pdf_bytes(report, &sections, lang);
            ReportExport {
                format,
                bytes,
                page_count: Some(pages),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Textes et nombres
// ---------------------------------------------------------------------------

struct T(Lang);

impl T {
    fn t(&self, key: &str) -> String {
        lookup(self.0, key)
    }
    fn a(&self, key: &str, args: &[(&str, &str)]) -> String {
        let mut s = lookup(self.0, key);
        for (n, v) in args {
            s = s.replace(&format!("{{{n}}}"), v);
        }
        s
    }
}

/// Nombre à 0,01 près, point décimal ; vide si absent ou non fini (jamais `NaN` / `inf`).
pub(crate) fn num(v: Option<f64>) -> String {
    match v {
        Some(v) if v.is_finite() => {
            let s = format!("{v:.2}");
            if s == "-0.00" {
                "0.00".into()
            } else {
                s
            }
        }
        _ => String::new(),
    }
}

fn int(n: usize) -> String {
    n.to_string()
}

fn letter(s: SeamWhich) -> &'static str {
    match s {
        SeamWhich::A => "A",
        SeamWhich::B => "B",
    }
}

fn ids(edges: &[pf_core::EntityId]) -> String {
    edges
        .iter()
        .map(|e| e.0.to_string())
        .collect::<Vec<_>>()
        .join("+")
}

fn category_text(t: &T, c: Option<PieceCategory>) -> String {
    match c {
        None => String::new(),
        Some(PieceCategory::Fabric) => t.t("piece.category.fabric"),
        Some(PieceCategory::Lining) => t.t("piece.category.lining"),
        Some(PieceCategory::Interfacing) => t.t("piece.category.interfacing"),
        Some(PieceCategory::Leather) => t.t("piece.category.leather"),
        Some(PieceCategory::Reinforcement) => t.t("piece.category.reinforcement"),
    }
}

/// Texte d'un avertissement de pièce (mêmes clés que la fiche de pièce).
pub fn piece_issue_text(lang: Lang, issue: &PieceIssue) -> String {
    use PieceIssue as I;
    let t = T(lang);
    let n = |c: &usize| c.to_string();
    match issue {
        I::NoName => t.t("piece.issue.no_name"),
        I::MissingGrainline => t.t("piece.issue.missing_grainline"),
        I::GrainlineMissingEntity => t.t("piece.issue.grainline_missing_entity"),
        I::EmptyOutline => t.t("piece.issue.empty_outline"),
        I::OutlineOpen => t.t("piece.issue.outline_open"),
        I::OutlineUnreadable => t.t("piece.issue.outline_unreadable"),
        I::OutlineSelfIntersecting => t.t("piece.issue.outline_self_intersecting"),
        I::InternalOutsideOutline { count } => t.a(
            "piece.issue.internal_outside_outline",
            &[("count", &n(count))],
        ),
        I::DartInvalid { count } => t.a("piece.issue.dart_invalid", &[("count", &n(count))]),
        I::MarkingOutsideOutline { count } => t.a(
            "piece.issue.marking_outside_outline",
            &[("count", &n(count))],
        ),
        I::ButtonsTooCloseToEdge { count } => t.a(
            "piece.issue.buttons_too_close_to_edge",
            &[("count", &n(count))],
        ),
        I::GrainRotationExceeded => t.t("piece.issue.grain_rotation_exceeded"),
        I::DuplicateCode { code } => t.a("piece.issue.duplicate_code", &[("code", code)]),
        I::DerivedStale { .. } => t.t("piece.issue.derived_stale"),
        I::CutContourFailed { reason } => {
            t.a("piece.issue.cut_contour_failed", &[("reason", reason)])
        }
        I::FoldNotchesFailed { reason } => {
            t.a("piece.issue.fold_notches_failed", &[("reason", reason)])
        }
        I::CutNotchesFailed { reason } => {
            t.a("piece.issue.cut_notches_failed", &[("reason", reason)])
        }
        I::CornerNotchesFailed { reason } => {
            t.a("piece.issue.corner_notches_failed", &[("reason", reason)])
        }
        I::CutDartFailed { reason } => t.a("piece.issue.cut_dart_failed", &[("reason", reason)]),
        I::FoldOutsideContour { count } => {
            t.a("piece.issue.fold_outside_contour", &[("count", &n(count))])
        }
        I::FoldMarginIgnored { count } => {
            t.a("piece.issue.fold_margin_ignored", &[("count", &n(count))])
        }
        I::NetCutWithSeam { hems, folds } => t.a(
            "piece.issue.net_cut_with_seam",
            &[("hems", &hems.to_string()), ("folds", &folds.to_string())],
        ),
    }
}

/// Raison d'un « non calculable » d'après son identifiant stable.
fn reason_text(t: &T, key: &str) -> String {
    let k = match key {
        "piece_missing" => "seams.reason.piece_missing",
        "outline_open" => "seams.reason.outline_open",
        "edge_not_in_outline" => "seams.reason.edge_not_in_outline",
        "degenerate" => "seams.reason.degenerate",
        "tangent_undefined" => "seams.join.reason.tangent",
        "cusp" => "seams.join.reason.cusp",
        "chord_degenerate" => "seams.join.reason.chord",
        other => return other.to_string(),
    };
    t.t(k)
}

fn side_reason(t: &T, side: SeamWhich, key: &str) -> String {
    t.a(
        "report.reason.line",
        &[("side", letter(side)), ("reason", &reason_text(t, key))],
    )
}

fn end_text(t: &T, e: JunctionEnd) -> String {
    match e {
        JunctionEnd::Start => t.t("seams.join.end.start"),
        JunctionEnd::End => t.t("seams.join.end.end"),
    }
}

fn anomaly_texts(t: &T, a: &Anomaly) -> (String, String, String) {
    let kind = t.t(&format!("report.kind.{}", a.kind.key()));
    let n = a.pair.map_or_else(String::new, |p| p.to_string());
    let side = a.side.map_or("", letter);
    let reference = match a.kind {
        AnomalyKind::PieceIssue => t.a(
            "report.ref.piece",
            &[("name", a.piece_name.as_deref().unwrap_or(""))],
        ),
        AnomalyKind::NotchMissing | AnomalyKind::NotchShifted => t.a(
            "report.ref.notch",
            &[
                ("n", &n),
                ("side", side),
                ("id", &a.notch.map_or(String::new(), |i| i.0.to_string())),
            ],
        ),
        AnomalyKind::JunctionBroken | AnomalyKind::JunctionNotComputable => t.a(
            "report.ref.junction",
            &[
                ("n", &n),
                ("end", &a.end.map_or(String::new(), |e| end_text(t, e))),
            ],
        ),
        _ => t.a("report.ref.pair", &[("n", &n)]),
    };
    let value = num(a.value);
    let tol = num(a.tolerance);
    let reason = a.reason.unwrap_or("");
    let detail = match a.kind {
        AnomalyKind::SeamOutOfTolerance => t.a(
            "report.detail.seam_out",
            &[("value", &value), ("tol", &tol)],
        ),
        AnomalyKind::SeamNotComputable
        | AnomalyKind::NotchNotComputable
        | AnomalyKind::JunctionNotComputable => {
            side_reason(t, a.side.unwrap_or(SeamWhich::A), reason)
        }
        AnomalyKind::NotchMissing => t.t("report.detail.notch_missing"),
        AnomalyKind::NotchShifted => t.a(
            "report.detail.notch_shifted",
            &[("value", &value), ("tol", &tol)],
        ),
        AnomalyKind::JunctionBroken => t.a(
            "report.detail.junction_broken",
            &[("value", &value), ("tol", &tol)],
        ),
        AnomalyKind::PieceIssue => a
            .issue
            .as_ref()
            .map(|i| piece_issue_text(t.0, i))
            .unwrap_or_default(),
    };
    (kind, reference, detail)
}

// ---------------------------------------------------------------------------
// Tableaux communs au CSV et au PDF
// ---------------------------------------------------------------------------

/// Cellule : texte libre (protégé contre l'injection de formule en CSV) ou nombre déjà formaté.
#[derive(Debug, Clone)]
pub(crate) enum Cell {
    Text(String),
    Num(String),
}

impl Cell {
    pub(crate) fn text(&self) -> &str {
        match self {
            Cell::Text(s) | Cell::Num(s) => s,
        }
    }
}

pub(crate) fn tx(s: impl Into<String>) -> Cell {
    Cell::Text(s.into())
}

pub(crate) fn nm(v: Option<f64>) -> Cell {
    Cell::Num(num(v))
}

/// Colonne du PDF : étiquette, largeur (mm) et colonnes du CSV qu'elle réunit.
pub(crate) struct PdfCol {
    /// Clé du catalogue, ou texte tel quel quand `literal` (libellé venu de l'utilisateur).
    pub(crate) label: String,
    pub(crate) literal: bool,
    pub(crate) width: f64,
    pub(crate) from: Vec<usize>,
}

pub(crate) struct Row {
    pub(crate) cells: Vec<Cell>,
    /// Ligne de remarque sous la rangée (PDF seulement ; l'information est aussi dans les cellules).
    pub(crate) note: Option<String>,
}

pub(crate) struct Section {
    pub(crate) code: &'static str,
    pub(crate) title: String,
    pub(crate) header: Vec<String>,
    pub(crate) rows: Vec<Row>,
    pub(crate) pdf: Vec<PdfCol>,
    /// Texte du PDF quand la section est vide.
    pub(crate) empty: String,
}

pub(crate) fn pdf_col(label: &str, width: f64, from: &[usize]) -> PdfCol {
    PdfCol {
        label: label.to_string(),
        literal: false,
        width,
        from: from.to_vec(),
    }
}

/// Colonne dont l'en-tête est un texte déjà prêt (jamais cherché dans le catalogue).
pub(crate) fn pdf_col_literal(label: &str, width: f64, from: &[usize]) -> PdfCol {
    PdfCol {
        literal: true,
        ..pdf_col(label, width, from)
    }
}

fn sections(r: &ControlReport, lang: Lang) -> Vec<Section> {
    let t = T(lang);
    let yes_no = |b: bool| t.t(if b { "report.yes" } else { "report.no" });
    let col = |keys: &[&str]| -> Vec<String> { keys.iter().map(|k| t.t(k)).collect() };
    let total = r.anomalies.len();

    // --- Synthèse ------------------------------------------------------------------------
    let mut resume_rows = vec![
        Row {
            cells: vec![tx(t.t("report.sum.project")), tx(r.project_name.clone())],
            note: None,
        },
        Row {
            cells: vec![
                tx(t.t("report.sum.format")),
                Cell::Num(r.schema_version.to_string()),
            ],
            note: None,
        },
        Row {
            cells: vec![tx(t.t("report.sum.sheets")), Cell::Num(int(r.sheet_count))],
            note: None,
        },
        Row {
            cells: vec![tx(t.t("report.sum.pieces")), Cell::Num(int(r.pieces.len()))],
            note: None,
        },
        Row {
            cells: vec![tx(t.t("report.sum.pairs")), Cell::Num(int(r.seams.len()))],
            note: None,
        },
        Row {
            cells: vec![tx(t.t("report.sum.anomalies")), Cell::Num(int(total))],
            note: None,
        },
    ];
    for k in AnomalyKind::ALL {
        resume_rows.push(Row {
            cells: vec![
                tx(t.t(&format!("report.kind.{}", k.key()))),
                Cell::Num(int(r.count(k))),
            ],
            note: None,
        });
    }
    resume_rows.push(Row {
        cells: vec![
            tx(t.t("report.sum.synthesis")),
            tx(if total == 0 {
                t.t("report.anom.none")
            } else {
                t.a("report.anom.count", &[("count", &total.to_string())])
            }),
        ],
        note: None,
    });
    resume_rows.push(Row {
        cells: vec![tx(t.t("report.sum.notice")), tx(t.t("report.footer"))],
        note: None,
    });
    let resume = Section {
        code: "resume",
        title: t.t("report.sec.summary"),
        header: col(&["report.col.element", "report.col.value"]),
        rows: resume_rows,
        pdf: vec![
            pdf_col("report.col.element", 70.0, &[0]),
            pdf_col("report.col.value", 110.0, &[1]),
        ],
        empty: String::new(),
    };

    // --- Anomalies -------------------------------------------------------------------------
    let anomaly_rows: Vec<Row> = r
        .anomalies
        .iter()
        .map(|a| {
            let (kind, reference, detail) = anomaly_texts(&t, a);
            Row {
                cells: vec![
                    tx(kind),
                    tx(reference),
                    tx(detail),
                    nm(a.value),
                    nm(a.tolerance),
                    tx(a.reason.map_or_else(String::new, |k| reason_text(&t, k))),
                ],
                note: None,
            }
        })
        .collect();
    let anomalies = Section {
        code: "anomalie",
        title: t.a("report.sec.anomalies", &[("count", &total.to_string())]),
        header: col(&[
            "report.col.type",
            "report.col.ref",
            "report.col.detail",
            "report.col.value",
            "report.col.tol",
            "report.col.reason",
        ]),
        rows: anomaly_rows,
        pdf: vec![
            pdf_col("report.col.type", 33.0, &[0]),
            pdf_col("report.col.ref", 40.0, &[1]),
            pdf_col("report.col.detail", 76.0, &[2]),
            pdf_col("report.col.value", 15.0, &[3]),
            pdf_col("report.col.tol", 16.0, &[4]),
        ],
        empty: t.t("report.anom.none"),
    };

    // --- Pièces ----------------------------------------------------------------------------
    let piece_rows: Vec<Row> = r
        .pieces
        .iter()
        .map(|p| {
            let issues: Vec<String> = p.issues.iter().map(|i| piece_issue_text(lang, i)).collect();
            Row {
                cells: vec![
                    tx(p.sheet.clone().unwrap_or_default()),
                    tx(p.name.clone()),
                    tx(p.code.clone().unwrap_or_default()),
                    tx(category_text(&t, p.category)),
                    Cell::Num(p.quantity.to_string()),
                    tx(p.material.clone().unwrap_or_default()),
                    nm(p.area_mm2),
                    nm(p.perimeter_mm),
                    tx(yes_no(p.closed)),
                    tx(issues.join(" | ")),
                ],
                note: (!issues.is_empty())
                    .then(|| t.a("report.piece.issues", &[("text", &issues.join(" | "))])),
            }
        })
        .collect();
    let pieces = Section {
        code: "piece",
        title: t.a(
            "report.sec.pieces",
            &[("count", &r.pieces.len().to_string())],
        ),
        header: col(&[
            "report.col.sheet",
            "report.col.name",
            "report.col.code",
            "report.col.category",
            "report.col.qty",
            "report.col.material",
            "report.col.area",
            "report.col.perimeter",
            "report.col.closed",
            "report.col.issues",
        ]),
        rows: piece_rows,
        pdf: vec![
            pdf_col("report.col.sheet", 22.0, &[0]),
            pdf_col("report.col.name", 46.0, &[1]),
            pdf_col("report.col.code", 18.0, &[2]),
            pdf_col("report.col.category", 18.0, &[3]),
            pdf_col("report.col.qty", 9.0, &[4]),
            pdf_col("report.col.material", 21.0, &[5]),
            pdf_col("report.col.area", 18.0, &[6]),
            pdf_col("report.col.perimeter", 18.0, &[7]),
            pdf_col("report.col.closed", 10.0, &[8]),
        ],
        empty: t.t("report.empty"),
    };

    // --- Coutures --------------------------------------------------------------------------
    let seam_rows: Vec<Row> = r
        .seams
        .iter()
        .map(|s| {
            let d = &s.reading;
            let (status, reason) = match d.status {
                SeamStatus::Within => (t.t("seams.status.within"), String::new()),
                SeamStatus::OutOfTolerance => (t.t("seams.status.out"), String::new()),
                SeamStatus::NotComputable { side, reason } => {
                    (t.t("seams.status.na"), side_reason(&t, side, reason.key()))
                }
            };
            Row {
                cells: vec![
                    Cell::Num(d.number.to_string()),
                    tx(s.piece_a.clone().unwrap_or_default()),
                    tx(ids(&s.edges_a)),
                    tx(s.piece_b.clone().unwrap_or_default()),
                    tx(ids(&s.edges_b)),
                    nm(d.length_a_mm),
                    nm(d.length_b_mm),
                    nm(d.gap_mm),
                    nm(d.embu_measured_mm),
                    nm(Some(d.embu_expected_mm)),
                    nm(d.deviation_mm),
                    nm(Some(d.tolerance_mm)),
                    tx(status),
                    tx(reason),
                ],
                note: None,
            }
        })
        .collect();
    let seams = Section {
        code: "couture",
        title: t.a("report.sec.seams", &[("count", &r.seams.len().to_string())]),
        header: col(&[
            "report.col.pair",
            "report.col.piece_a",
            "report.col.edges_a",
            "report.col.piece_b",
            "report.col.edges_b",
            "report.col.len_a",
            "report.col.len_b",
            "report.col.gap",
            "report.col.embu_measured",
            "report.col.embu_expected",
            "report.col.embu_dev",
            "report.col.tol_mm",
            "report.col.status",
            "report.col.reason",
        ]),
        rows: seam_rows,
        pdf: vec![
            pdf_col("report.col.pair", 11.0, &[0]),
            pdf_col("report.col.piece_a", 26.0, &[1, 2]),
            pdf_col("report.col.piece_b", 26.0, &[3, 4]),
            pdf_col("report.col.len_a", 13.0, &[5]),
            pdf_col("report.col.len_b", 13.0, &[6]),
            pdf_col("report.col.gap", 11.0, &[7]),
            pdf_col("report.col.embu_measured", 14.0, &[8]),
            pdf_col("report.col.embu_expected", 14.0, &[9]),
            pdf_col("report.col.tol_mm", 12.0, &[11]),
            pdf_col("report.col.status", 22.0, &[12]),
            pdf_col("report.col.reason", 18.0, &[13]),
        ],
        empty: t.t("report.empty"),
    };

    // --- Crans -----------------------------------------------------------------------------
    let mut notch_rows = Vec::new();
    for c in &r.notch_checks {
        if let Some((side, reason)) = c.not_computable {
            notch_rows.push(Row {
                cells: vec![
                    Cell::Num(c.number.to_string()),
                    tx(letter(side)),
                    tx(""),
                    nm(None),
                    nm(None),
                    nm(None),
                    nm(None),
                    nm(Some(c.tolerance_mm)),
                    tx(t.t("report.notch.na")),
                    tx(side_reason(&t, side, reason.key())),
                ],
                note: None,
            });
            continue;
        }
        for it in &c.items {
            let status = match it.status {
                NotchMatch::Matched => "report.notch.matched",
                NotchMatch::Shifted => "report.notch.shifted",
                NotchMatch::Missing => "report.notch.missing",
            };
            notch_rows.push(Row {
                cells: vec![
                    Cell::Num(c.number.to_string()),
                    tx(letter(it.side)),
                    Cell::Num(it.notch.0.to_string()),
                    nm(Some(it.position_mm)),
                    nm(Some(it.expected_mm)),
                    nm(it.found_mm),
                    nm(it.gap_mm),
                    nm(Some(c.tolerance_mm)),
                    tx(t.t(status)),
                    tx(""),
                ],
                note: None,
            });
        }
    }
    let notches = Section {
        code: "crans",
        title: t.a(
            "report.sec.notches",
            &[("count", &notch_rows.len().to_string())],
        ),
        header: col(&[
            "report.col.pair",
            "report.col.side",
            "report.col.notch",
            "report.col.position",
            "report.col.expected",
            "report.col.found",
            "report.col.gap",
            "report.col.tol_mm",
            "report.col.status",
            "report.col.reason",
        ]),
        pdf: vec![
            pdf_col("report.col.pair", 11.0, &[0]),
            pdf_col("report.col.side", 10.0, &[1]),
            pdf_col("report.col.notch", 12.0, &[2]),
            pdf_col("report.col.position", 20.0, &[3]),
            pdf_col("report.col.expected", 20.0, &[4]),
            pdf_col("report.col.found", 20.0, &[5]),
            pdf_col("report.col.gap", 17.0, &[6]),
            pdf_col("report.col.tol_mm", 14.0, &[7]),
            pdf_col("report.col.status", 22.0, &[8]),
            pdf_col("report.col.reason", 34.0, &[9]),
        ],
        rows: notch_rows,
        empty: t.t("report.empty"),
    };

    // --- Raccords --------------------------------------------------------------------------
    let junction_rows: Vec<Row> = r
        .junctions
        .iter()
        .map(|j| {
            let dev = j.deviation_deg.unwrap_or(0.0);
            let (status, reason) = match j.status {
                JunctionStatus::Smooth => (t.t("seams.join.status.smooth"), String::new()),
                JunctionStatus::Broken => (
                    t.t(if dev > 0.0 {
                        "seams.join.status.bump"
                    } else {
                        "seams.join.status.hollow"
                    }),
                    String::new(),
                ),
                JunctionStatus::NotComputable { side, reason } => (
                    t.t("seams.join.status.na"),
                    side_reason(&t, side, reason.key()),
                ),
            };
            Row {
                cells: vec![
                    Cell::Num(j.number.to_string()),
                    tx(end_text(&t, j.end)),
                    tx(end_text(&t, j.partner_end)),
                    nm(j.angle_a_deg),
                    nm(j.angle_b_deg),
                    nm(j.sum_deg),
                    nm(j.deviation_deg),
                    nm(Some(j.tolerance_deg)),
                    tx(status),
                    tx(reason),
                ],
                note: None,
            }
        })
        .collect();
    let junctions = Section {
        code: "raccord",
        title: t.a(
            "report.sec.junctions",
            &[("count", &junction_rows.len().to_string())],
        ),
        header: col(&[
            "report.col.pair",
            "report.col.end_a",
            "report.col.end_b",
            "report.col.angle_a",
            "report.col.angle_b",
            "report.col.sum",
            "report.col.dev",
            "report.col.tol_deg",
            "report.col.status",
            "report.col.reason",
        ]),
        pdf: vec![
            pdf_col("report.col.pair", 11.0, &[0]),
            pdf_col("report.col.end_a", 15.0, &[1]),
            pdf_col("report.col.end_b", 15.0, &[2]),
            pdf_col("report.col.angle_a", 17.0, &[3]),
            pdf_col("report.col.angle_b", 17.0, &[4]),
            pdf_col("report.col.sum", 17.0, &[5]),
            pdf_col("report.col.dev", 17.0, &[6]),
            pdf_col("report.col.tol_deg", 14.0, &[7]),
            pdf_col("report.col.status", 28.0, &[8]),
            pdf_col("report.col.reason", 29.0, &[9]),
        ],
        rows: junction_rows,
        empty: t.t("report.empty"),
    };

    // --- Bords sur le pli (PH-5b) : section présente seulement si une pièce en déclare, pour que
    // le rapport d'un projet sans pliure reste identique à l'octet.
    let fold_rows: Vec<Row> = r
        .pieces
        .iter()
        .filter(|p| p.fold_edges > 0)
        .map(|p| Row {
            cells: vec![
                tx(p.sheet.clone().unwrap_or_default()),
                tx(p.name.clone()),
                Cell::Num(int(p.fold_edges)),
                nm(Some(p.fold_length_mm)),
            ],
            note: None,
        })
        .collect();
    let mut out = vec![resume, anomalies, pieces];
    if !fold_rows.is_empty() {
        out.push(Section {
            code: "pli",
            title: t.a(
                "report.sec.folds",
                &[("count", &fold_rows.len().to_string())],
            ),
            header: col(&[
                "report.col.sheet",
                "report.col.name",
                "report.col.fold_edges",
                "report.col.fold_length",
            ]),
            pdf: vec![
                pdf_col("report.col.sheet", 40.0, &[0]),
                pdf_col("report.col.name", 70.0, &[1]),
                pdf_col("report.col.fold_edges", 30.0, &[2]),
                pdf_col("report.col.fold_length", 40.0, &[3]),
            ],
            rows: fold_rows,
            empty: t.t("report.empty"),
        });
    }
    out.extend([seams, notches, junctions]);
    // --- Mesures finies (US-10.4) : section présente seulement s'il y en a, pour que le rapport
    // d'un projet sans mesure finie reste identique à l'octet.
    if !r.finished.is_empty() {
        let rows: Vec<Row> = r
            .finished
            .iter()
            .map(|f| {
                let unit = match f.unit {
                    MeasurementUnit::Mm => "mm",
                    MeasurementUnit::Deg => "deg",
                };
                let status = match f.status {
                    FinishedStatus::Conforming => "report.fin.status.ok",
                    FinishedStatus::OutOfTolerance => "report.fin.status.out",
                    FinishedStatus::NotComparable => "report.fin.status.na",
                };
                let reason = match f.reason {
                    None => String::new(),
                    Some(FinishedReason::SourceDegenerate) => t.t("report.fin.reason.source"),
                    Some(FinishedReason::NoReferenceValue) => t.t("report.fin.reason.no_reference"),
                    Some(FinishedReason::UnitMismatch) => t.t("report.fin.reason.unit"),
                    Some(FinishedReason::AlterationOutOfRange) => {
                        t.t("report.fin.reason.alteration")
                    }
                    Some(FinishedReason::DeadMeasurement) => t.t("report.fin.reason.dead"),
                };
                Row {
                    cells: vec![
                        tx(f.name.clone()),
                        tx(format!("{} ({unit})", f.measurement_name)),
                        tx(f.size_label.clone().unwrap_or_default()),
                        nm(f.measured),
                        nm(f.reference),
                        nm(Some(f.alteration_delta).filter(|d| *d != 0.0)),
                        nm(f.gap),
                        nm(Some(f.tolerance_minus)),
                        nm(Some(f.tolerance_plus)),
                        tx(t.t(status)),
                        tx(reason),
                    ],
                    note: None,
                }
            })
            .collect();
        out.push(Section {
            code: "mesure_finie",
            title: t.a("report.sec.finished", &[("count", &rows.len().to_string())]),
            header: col(&[
                "report.col.fin_name",
                "report.col.fin_measure",
                "report.col.fin_size",
                "report.col.fin_measured",
                "report.col.fin_reference",
                "report.col.fin_alteration",
                "report.col.fin_gap",
                "report.col.fin_tol_minus",
                "report.col.fin_tol_plus",
                "report.col.status",
                "report.col.reason",
            ]),
            pdf: vec![
                pdf_col("report.col.fin_name", 24.0, &[0]),
                pdf_col("report.col.fin_measure", 24.0, &[1]),
                pdf_col("report.col.fin_size", 10.0, &[2]),
                pdf_col("report.col.fin_measured", 14.0, &[3]),
                pdf_col("report.col.fin_reference", 14.0, &[4]),
                pdf_col("report.col.fin_alteration", 13.0, &[5]),
                pdf_col("report.col.fin_gap", 13.0, &[6]),
                pdf_col("report.col.fin_tol_minus", 9.0, &[7]),
                pdf_col("report.col.fin_tol_plus", 9.0, &[8]),
                pdf_col("report.col.status", 22.0, &[9]),
                pdf_col("report.col.reason", 28.0, &[10]),
            ],
            rows,
            empty: t.t("report.empty"),
        });
    }
    // --- Trous de piqure (US-08.6) : section presente seulement s'il y a un couple declare, pour
    // que le rapport d'un projet sans couple reste identique a l'octet. Une ligne de synthese par
    // couple, puis une ligne par trou decale, manquant ou en trop (avec sa position).
    if !r.stitch_checks.is_empty() {
        let name_of = |id: pf_core::PieceId| {
            r.pieces
                .iter()
                .find(|p| p.id == id)
                .map(|p| p.name.clone())
                .unwrap_or_default()
        };
        let mut rows: Vec<Row> = Vec::new();
        for c in &r.stitch_checks {
            let head = |hole: Option<u32>,
                        status: String,
                        x: Option<f64>,
                        y: Option<f64>,
                        gap: Option<f64>| {
                Row {
                    cells: vec![
                        tx(c.number.to_string()),
                        tx(name_of(c.a.piece)),
                        tx(name_of(c.b.piece)),
                        tx(c.count_a.to_string()),
                        tx(c.count_b.to_string()),
                        nm(c.not_computable.is_none().then_some(c.mean_step_a)),
                        nm(c.not_computable.is_none().then_some(c.mean_step_b)),
                        nm(c.not_computable.is_none().then_some(c.step_gap_mm)),
                        tx(hole.map(|h| h.to_string()).unwrap_or_default()),
                        tx(status),
                        nm(x),
                        nm(y),
                        nm(gap),
                    ],
                    note: None,
                }
            };
            if c.not_computable.is_some() {
                rows.push(head(None, t.t("report.stitch.status.na"), None, None, None));
                continue;
            }
            let summary = if c.counts_equal() {
                "report.stitch.status.same"
            } else {
                "report.stitch.status.different"
            };
            rows.push(head(None, t.t(summary), None, None, None));
            for it in &c.items {
                let (key, hole) = match it.status {
                    StitchMatch::Matched => continue,
                    StitchMatch::Shifted => ("report.stitch.status.shifted", it.a),
                    StitchMatch::Missing(SeamWhich::B) => ("report.stitch.status.missing_b", it.a),
                    StitchMatch::Missing(SeamWhich::A) => ("report.stitch.status.extra_b", it.b),
                };
                rows.push(head(
                    Some(it.number),
                    t.t(key),
                    hole.map(|h| h.position.x),
                    hole.map(|h| h.position.y),
                    it.gap_mm,
                ));
            }
        }
        out.push(Section {
            code: "trous_piqure",
            title: t.a(
                "report.sec.stitch",
                &[("count", &r.stitch_checks.len().to_string())],
            ),
            header: col(&[
                "report.col.st_pair",
                "report.col.st_piece_a",
                "report.col.st_piece_b",
                "report.col.st_count_a",
                "report.col.st_count_b",
                "report.col.st_step_a",
                "report.col.st_step_b",
                "report.col.st_step_gap",
                "report.col.st_hole",
                "report.col.status",
                "report.col.st_x",
                "report.col.st_y",
                "report.col.st_gap",
            ]),
            pdf: vec![
                pdf_col("report.col.st_pair", 9.0, &[0]),
                pdf_col("report.col.st_piece_a", 20.0, &[1]),
                pdf_col("report.col.st_piece_b", 20.0, &[2]),
                pdf_col("report.col.st_count_a", 10.0, &[3]),
                pdf_col("report.col.st_count_b", 10.0, &[4]),
                pdf_col("report.col.st_step_a", 13.0, &[5]),
                pdf_col("report.col.st_step_b", 13.0, &[6]),
                pdf_col("report.col.st_step_gap", 13.0, &[7]),
                pdf_col("report.col.st_hole", 9.0, &[8]),
                pdf_col("report.col.status", 28.0, &[9]),
                pdf_col("report.col.st_x", 13.0, &[10]),
                pdf_col("report.col.st_y", 13.0, &[11]),
                pdf_col("report.col.st_gap", 13.0, &[12]),
            ],
            rows,
            empty: t.t("report.empty"),
        });
    }
    out
}

// ---------------------------------------------------------------------------
// CSV
// ---------------------------------------------------------------------------

/// Champ texte : apostrophe devant une formule possible, guillemets si nécessaire.
pub fn csv_text(s: &str) -> String {
    let mut field = String::with_capacity(s.len() + 2);
    if s.starts_with(['=', '+', '-', '@', '\t', '\r']) {
        field.push('\'');
    }
    field.push_str(s);
    csv_quote(&field)
}

/// Nombre CSV : 2 décimales, point décimal, champ vide s'il est absent ou non fini.
pub fn csv_number(v: Option<f64>) -> String {
    num(v)
}

fn csv_quote(s: &str) -> String {
    if s.contains([';', '"', '\r', '\n']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

pub(crate) fn csv_bytes(sections: &[Section]) -> Vec<u8> {
    let mut out = String::from("\u{FEFF}");
    for s in sections {
        let mut head = vec![format!("#{}", s.code)];
        head.extend(s.header.iter().map(|h| csv_text(h)));
        out.push_str(&head.join(";"));
        out.push_str("\r\n");
        for row in &s.rows {
            let mut line = vec![s.code.to_string()];
            line.extend(row.cells.iter().map(|c| match c {
                Cell::Text(t) => csv_text(t),
                Cell::Num(n) => n.clone(),
            }));
            out.push_str(&line.join(";"));
            out.push_str("\r\n");
        }
    }
    out.into_bytes()
}

// ---------------------------------------------------------------------------
// PDF
// ---------------------------------------------------------------------------

pub(crate) const PAGE_W_MM: f64 = 210.0;
const PAGE_H_MM: f64 = 297.0;
pub(crate) const MARGIN_MM: f64 = 15.0;
pub(crate) const BODY_PT: f64 = 7.5;
const HEAD_PT: f64 = 10.0;
const TITLE_PT: f64 = 15.0;
const FOOT_PT: f64 = 7.0;
/// Interligne du corps (mm).
const LINE_MM: f64 = 3.5;
/// Marge intérieure des cellules (mm) de chaque côté.
pub(crate) const PAD_MM: f64 = 1.0;
/// Bas de la zone de texte (le pied de page est au-dessous).
const BODY_BOTTOM_MM: f64 = 22.0;
const FOOT_BASELINE_MM: f64 = 12.0;
const GRAY_RULE: f64 = 0.75;

fn pt_mm(pt: f64) -> f64 {
    pt * 25.4 / 72.0
}

/// Largeur (mm) d'un caractère tel qu'il est écrit dans le PDF (« ? » s'il est absent).
fn char_mm(ch: char, size_pt: f64) -> f64 {
    let f = embedded();
    f64::from(f.advance_of_cid(f.cid(ch))) / f64::from(f.units_per_em()) * pt_mm(size_pt)
}

pub(crate) fn width_mm(text: &str, size_pt: f64) -> f64 {
    text.chars().map(|c| char_mm(c, size_pt)).sum()
}

/// Texte d'une cellule du PDF : pas de caractère de contrôle (ils n'ont pas de glyphe).
pub(crate) fn clean(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

/// Coupe `text` en lignes qui tiennent dans `max_w` mm (largeurs réelles de la police) : aux
/// espaces, puis, pour un mot plus large que la ligne, entre deux caractères. Jamais de ligne
/// vide en plus ; un texte vide donne une seule ligne vide.
fn wrap(text: &str, max_w: f64, size_pt: f64) -> Vec<String> {
    let max_w = max_w - 1e-6;
    let text = clean(text);
    let mut lines: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut cur_w = 0.0;
    let space = char_mm(' ', size_pt);
    for word in text.split_whitespace() {
        let ww = width_mm(word, size_pt);
        if !cur.is_empty() && cur_w + space + ww <= max_w {
            cur.push(' ');
            cur.push_str(word);
            cur_w += space + ww;
            continue;
        }
        if !cur.is_empty() {
            lines.push(std::mem::take(&mut cur));
            cur_w = 0.0;
        }
        if ww <= max_w {
            cur.push_str(word);
            cur_w = ww;
            continue;
        }
        for ch in word.chars() {
            let cw = char_mm(ch, size_pt);
            if !cur.is_empty() && cur_w + cw > max_w {
                lines.push(std::mem::take(&mut cur));
                cur_w = 0.0;
            }
            cur.push(ch);
            cur_w += cw;
        }
    }
    if !cur.is_empty() || lines.is_empty() {
        lines.push(cur);
    }
    lines
}

enum Op {
    Text {
        x: f64,
        y: f64,
        pt: f64,
        text: String,
    },
    Rule {
        y: f64,
    },
    /// Polyligne (US-11.3, vignettes) : points en mm de page, gris et épaisseur du trait.
    Poly {
        pts: Vec<pf_geometry::PointMm>,
        gray: f64,
        width: f64,
    },
}

/// Bloc graphique d'un PDF de rapport (US-11.3) : titre, hauteur réservée et dessin en mm
/// **relatifs au coin haut-gauche du bloc** (x vers la droite, y vers le bas, à partir de la marge).
pub(crate) struct GraphicBlock {
    pub(crate) title: String,
    pub(crate) height: f64,
    pub(crate) polys: Vec<(Vec<pf_geometry::PointMm>, f64)>,
    pub(crate) labels: Vec<(f64, f64, f64, String)>,
}

pub(crate) enum Item<'a> {
    Table(&'a Section),
    Graphic(GraphicBlock),
}

/// Une ligne visuelle d'un tableau : textes placés (x, texte).
struct VLine {
    parts: Vec<(f64, String)>,
}

struct Pager {
    pages: Vec<Vec<Op>>,
    /// Haut de la prochaine ligne, en mm depuis le bas de la page.
    y: f64,
}

impl Pager {
    fn new() -> Pager {
        Pager {
            pages: vec![Vec::new()],
            y: PAGE_H_MM - MARGIN_MM,
        }
    }

    fn remaining(&self) -> f64 {
        self.y - BODY_BOTTOM_MM
    }

    fn body_height() -> f64 {
        PAGE_H_MM - MARGIN_MM - BODY_BOTTOM_MM
    }

    fn new_page(&mut self, head: &str) {
        self.pages.push(Vec::new());
        self.y = PAGE_H_MM - MARGIN_MM;
        self.text(MARGIN_MM, 8.0, head);
        self.y -= LINE_MM * 1.6;
    }

    fn ops(&mut self) -> &mut Vec<Op> {
        self.pages.last_mut().expect("au moins une page")
    }

    /// Une ligne de texte de `pt` points dont le haut est `y` (la ligne occupe `LINE_MM`).
    fn text_at(&mut self, x: f64, pt: f64, text: &str) {
        let y = self.y - pt_mm(pt);
        self.ops().push(Op::Text {
            x,
            y,
            pt,
            text: text.to_string(),
        });
    }

    fn text(&mut self, x: f64, pt: f64, text: &str) {
        self.text_at(x, pt, text);
    }

    fn rule(&mut self) {
        let y = self.y;
        self.ops().push(Op::Rule { y });
    }

    fn vline(&mut self, line: &VLine) {
        for (x, t) in &line.parts {
            self.text_at(*x, BODY_PT, t);
        }
        self.y -= LINE_MM;
    }
}

fn col_x(cols: &[PdfCol]) -> Vec<f64> {
    let mut x = MARGIN_MM;
    cols.iter()
        .map(|c| {
            let at = x;
            x += c.width;
            at
        })
        .collect()
}

/// Lignes visuelles d'une rangée (cellules coupées à la largeur de leur colonne) + remarque.
fn row_lines(cols: &[PdfCol], xs: &[f64], cells: &[String], note: Option<&str>) -> Vec<VLine> {
    let wrapped: Vec<Vec<String>> = cols
        .iter()
        .zip(cells)
        .map(|(c, t)| wrap(t, c.width - 2.0 * PAD_MM, BODY_PT))
        .collect();
    let n = wrapped.iter().map(Vec::len).max().unwrap_or(1);
    let mut lines: Vec<VLine> = (0..n)
        .map(|i| VLine {
            parts: wrapped
                .iter()
                .zip(xs)
                .filter_map(|(w, x)| {
                    w.get(i)
                        .filter(|t| !t.is_empty())
                        .map(|t| (*x + PAD_MM, t.clone()))
                })
                .collect(),
        })
        .collect();
    if let Some(note) = note {
        let total: f64 = cols.iter().map(|c| c.width).sum();
        let indent = 4.0;
        for t in wrap(note, total - indent - PAD_MM, BODY_PT) {
            lines.push(VLine {
                parts: vec![(MARGIN_MM + indent, t)],
            });
        }
    }
    lines
}

fn pdf_bytes(report: &ControlReport, sections: &[Section], lang: Lang) -> (Vec<u8>, usize) {
    let t = T(lang);
    let frame = PdfFrame {
        title: t.t("report.title"),
        continued: t.t("report.continued"),
        footer: t.t("report.footer"),
        doc_title: format!("{} - {}", clean(&report.project_name), t.t("report.title")),
    };
    pdf_document(&frame, sections, lang)
}

/// Textes qui encadrent un PDF de tableaux : titre de la première page, en-tête des pages de
/// suite, pied de page et titre du document.
pub(crate) struct PdfFrame {
    pub(crate) title: String,
    pub(crate) continued: String,
    pub(crate) footer: String,
    pub(crate) doc_title: String,
}

/// PDF A4 portrait de sections de tableaux (mise en page commune aux rapports) : octets et
/// nombre de pages.
pub(crate) fn pdf_document(frame: &PdfFrame, sections: &[Section], lang: Lang) -> (Vec<u8>, usize) {
    let items: Vec<Item> = sections.iter().map(Item::Table).collect();
    pdf_document_items(frame, &items, lang)
}

/// Comme [`pdf_document`], avec des blocs graphiques entre les tableaux.
pub(crate) fn pdf_document_items(frame: &PdfFrame, items: &[Item], lang: Lang) -> (Vec<u8>, usize) {
    let t = T(lang);
    let head = frame.continued.clone();
    let mut p = Pager::new();
    p.text(MARGIN_MM, TITLE_PT, &frame.title);
    p.y -= pt_mm(TITLE_PT) * 1.6;
    for item in items {
        let s = match item {
            Item::Table(s) => *s,
            Item::Graphic(g) => {
                let title_h = pt_mm(HEAD_PT) * 1.5 + 1.0;
                if p.remaining() < title_h + g.height {
                    p.new_page(&head);
                }
                p.y -= 2.0;
                p.text(MARGIN_MM, HEAD_PT, &clean(&g.title));
                p.y -= title_h;
                let top = p.y;
                for (pts, width) in &g.polys {
                    let pts = pts
                        .iter()
                        .map(|q| pf_geometry::PointMm::new(MARGIN_MM + q.x, top - q.y))
                        .collect();
                    p.ops().push(Op::Poly {
                        pts,
                        gray: 0.0,
                        width: *width,
                    });
                }
                for (x, y, pt, text) in &g.labels {
                    p.ops().push(Op::Text {
                        x: MARGIN_MM + x,
                        y: top - y,
                        pt: *pt,
                        text: clean(text),
                    });
                }
                p.y -= g.height;
                continue;
            }
        };
        let cols = &s.pdf;
        let xs = col_x(cols);
        let labels: Vec<String> = cols
            .iter()
            .map(|c| {
                if c.literal {
                    c.label.clone()
                } else {
                    t.t(&c.label)
                }
            })
            .collect();
        let header_lines = row_lines(cols, &xs, &labels, None);
        let header_h = header_lines.len() as f64 * LINE_MM + 1.0;
        let title_h = pt_mm(HEAD_PT) * 1.5 + 1.0;
        let first_rows_h = if s.rows.is_empty() {
            LINE_MM
        } else {
            2.0 * LINE_MM
        };
        if p.remaining() < title_h + header_h + first_rows_h {
            p.new_page(&head);
        }
        p.y -= 2.0;
        p.text(MARGIN_MM, HEAD_PT, &clean(&s.title));
        p.y -= title_h;
        if s.rows.is_empty() {
            let mut shown = false;
            for l in wrap(&s.empty, 180.0, BODY_PT) {
                p.text(MARGIN_MM + PAD_MM, BODY_PT, &l);
                p.y -= LINE_MM;
                shown = true;
            }
            debug_assert!(shown || s.empty.is_empty());
            continue;
        }
        let draw_header = |p: &mut Pager| {
            for l in &header_lines {
                p.vline(l);
            }
            p.rule();
            p.y -= 1.0;
        };
        draw_header(&mut p);
        for row in &s.rows {
            let cells: Vec<String> = cols
                .iter()
                .map(|c| {
                    c.from
                        .iter()
                        .map(|i| row.cells[*i].text().to_string())
                        .filter(|x| !x.is_empty())
                        .collect::<Vec<_>>()
                        .join(" / ")
                })
                .collect();
            let lines = row_lines(cols, &xs, &cells, row.note.as_deref());
            let height = lines.len() as f64 * LINE_MM + 1.0;
            if height > p.remaining() && height <= Pager::body_height() - header_h {
                p.new_page(&head);
                draw_header(&mut p);
            }
            for l in &lines {
                if p.remaining() < LINE_MM {
                    p.new_page(&head);
                    draw_header(&mut p);
                }
                p.vline(l);
            }
            p.rule();
            p.y -= 1.0;
        }
    }
    let total = p.pages.len();
    let footer = frame.footer.clone();
    let pages: Vec<PdfPage> = p
        .pages
        .iter()
        .enumerate()
        .map(|(i, ops)| {
            let mut c = Content::new();
            for op in ops {
                match op {
                    Op::Text { x, y, pt, text } => c.text(*x, *y, *pt, text),
                    Op::Poly { pts, gray, width } => {
                        c.stroke(*gray, *width, &[]);
                        for w in pts.windows(2) {
                            c.line(w[0], w[1]);
                        }
                    }
                    Op::Rule { y } => {
                        c.stroke(GRAY_RULE, 0.15, &[]);
                        c.line(
                            pf_geometry::PointMm::new(MARGIN_MM, *y),
                            pf_geometry::PointMm::new(PAGE_W_MM - MARGIN_MM, *y),
                        );
                    }
                }
            }
            let page_no = t.a(
                "report.page",
                &[("n", &(i + 1).to_string()), ("total", &total.to_string())],
            );
            c.text(
                MARGIN_MM,
                FOOT_BASELINE_MM,
                FOOT_PT,
                &format!("{footer} - {page_no}"),
            );
            PdfPage {
                content: c.0,
                w_mm: PAGE_W_MM,
                h_mm: PAGE_H_MM,
            }
        })
        .collect();
    (assemble(&pages, &frame.doc_title), total)
}
