//! US-10.1 — import et export CSV du barème de mesures (lecture et écriture de texte seulement ;
//! la modification du projet passe par `Command::ImportMeasurements`).
//!
//! Format (documenté dans le contrat d'interface) : une ligne d'en-tête `mesure;unite;<taille>…`
//! (les étiquettes de tailles d'**une** gamme), puis une ligne par mesure : nom, unité (`mm` ou
//! `deg`), une cellule par taille (vide = non renseignée). Séparateur de champs, séparateur décimal
//! et encodage sont des réglages **explicites** ([`CsvFormat`]) : rien n'est deviné. Champs entre
//! guillemets doublés (`""`) quand ils contiennent le séparateur, un guillemet ou une espace en
//! bordure ; fins de ligne CRLF à l'écriture, CRLF ou LF à la lecture.
//!
//! La lecture est **tout ou rien** : la moindre ligne en erreur rejette le fichier entier, et toutes
//! les erreurs sont rapportées avec leur numéro de ligne (et de colonne). Rien n'est ignoré en
//! silence, hormis les lignes totalement vides (comptées dans [`MeasurementCsv::blank_lines`]).

use crate::grading_rule::check_stored_name;
use crate::measurement::{MeasurementUnit, MAX_MEASUREMENTS, MAX_MEASUREMENT_VALUE};
use crate::measurement_ops::{MeasurementImportCell, MeasurementImportRow};
use crate::model::*;
use std::collections::HashMap;

/// Encodage du fichier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CsvEncoding {
    /// UTF-8 sans marque d'ordre des octets (à la lecture, une marque présente est retirée).
    Utf8,
    /// UTF-8 avec marque d'ordre des octets (reconnu par les tableurs Windows).
    Utf8Bom,
    /// Windows-1252 : un caractère hors de la table est une erreur d'écriture, jamais un `?`.
    Windows1252,
}

/// Réglages explicites du fichier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CsvFormat {
    /// `;`, `,` ou tabulation.
    pub separator: char,
    /// `.` ou `,` ; différent du séparateur.
    pub decimal: char,
    pub encoding: CsvEncoding,
}

impl Default for CsvFormat {
    /// `;`, virgule décimale, UTF-8 avec marque : l'ouverture attendue dans un tableur français.
    fn default() -> Self {
        CsvFormat {
            separator: ';',
            decimal: ',',
            encoding: CsvEncoding::Utf8Bom,
        }
    }
}

impl CsvFormat {
    fn check(&self) -> Result<(), String> {
        if ![';', ',', '\t'].contains(&self.separator) {
            return Err("séparateur de champs : « ; », « , » ou tabulation".into());
        }
        if !['.', ','].contains(&self.decimal) {
            return Err("séparateur décimal : « . » ou « , »".into());
        }
        if self.separator == self.decimal {
            return Err(format!(
                "le séparateur de champs et le séparateur décimal sont identiques (« {} »)",
                self.separator
            ));
        }
        Ok(())
    }
}

/// Erreur de lecture : ligne (depuis 1, 0 = fichier entier) et colonne (depuis 1) si connue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CsvLineError {
    pub line: usize,
    pub column: Option<usize>,
    pub message: String,
}

impl std::fmt::Display for CsvLineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match (self.line, self.column) {
            (0, _) => write!(f, "{}", self.message),
            (l, None) => write!(f, "ligne {l} : {}", self.message),
            (l, Some(c)) => write!(f, "ligne {l}, colonne {c} : {}", self.message),
        }
    }
}

/// Fichier lu sans erreur : lignes prêtes pour `Command::ImportMeasurements`.
#[derive(Debug, Clone, PartialEq)]
pub struct MeasurementCsv {
    pub rows: Vec<MeasurementImportRow>,
    /// Lignes totalement vides rencontrées (comptées, pas interprétées).
    pub blank_lines: usize,
}

/// Fichier écrit.
#[derive(Debug, Clone, PartialEq)]
pub struct MeasurementCsvExport {
    pub bytes: Vec<u8>,
    pub measurements: usize,
    /// Valeurs du barème d'autres gammes, absentes du fichier (une gamme par fichier).
    pub omitted_values: usize,
}

const WIN1252_HIGH: [u32; 32] = [
    0x20AC, 0, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021, 0x02C6, 0x2030, 0x0160, 0x2039,
    0x0152, 0, 0x017D, 0, 0, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022, 0x2013, 0x2014, 0x02DC,
    0x2122, 0x0161, 0x203A, 0x0153, 0, 0x017E, 0x0178,
];

fn from_1252(b: u8) -> Option<char> {
    match b {
        0x00..=0x7F | 0xA0..=0xFF => Some(b as char),
        _ => char::from_u32(WIN1252_HIGH[(b - 0x80) as usize]).filter(|c| *c != '\0'),
    }
}

fn to_1252(c: char) -> Option<u8> {
    let u = c as u32;
    if u < 0x80 || (0xA0..=0xFF).contains(&u) {
        return Some(u as u8);
    }
    WIN1252_HIGH
        .iter()
        .position(|x| *x == u && u != 0)
        .map(|i| 0x80 + i as u8)
}

fn unit_text(u: MeasurementUnit) -> &'static str {
    match u {
        MeasurementUnit::Mm => "mm",
        MeasurementUnit::Deg => "deg",
    }
}

fn quote(field: &str, sep: char) -> String {
    let needs = field.contains(sep)
        || field.contains('"')
        || field.contains(['\r', '\n'])
        || field != field.trim();
    if needs {
        format!("\"{}\"", field.replace('"', "\"\""))
    } else {
        field.to_string()
    }
}

/// Nombre écrit : la forme la plus courte qui se relit à l'identique, jamais d'exposant.
fn write_number(v: f64, decimal: char) -> String {
    let s = format!("{v}");
    if decimal == '.' {
        s
    } else {
        s.replace('.', &decimal.to_string())
    }
}

/// Écrit le barème des tailles de `range`. Erreur : gamme inconnue, format invalide, caractère
/// absent de l'encodage choisi.
pub fn export_measurement_csv(
    project: &Project,
    range: SizeRangeId,
    format: &CsvFormat,
) -> Result<MeasurementCsvExport, String> {
    format.check()?;
    let r = project.size_range(range).map_err(|e| e.to_string())?;
    let sep = format.separator;
    let mut text = String::new();
    let mut header = vec!["mesure".to_string(), "unite".to_string()];
    header.extend(r.sizes.iter().map(|s| quote(&s.label, sep)));
    text.push_str(&header.join(&sep.to_string()));
    text.push_str("\r\n");
    let mut omitted = 0usize;
    for m in &project.measurements {
        let mut fields = vec![quote(&m.name, sep), unit_text(m.unit).to_string()];
        for s in &r.sizes {
            fields.push(
                m.values
                    .iter()
                    .find(|v| v.size == s.id)
                    .map(|v| write_number(v.value, format.decimal))
                    .unwrap_or_default(),
            );
        }
        omitted += m
            .values
            .iter()
            .filter(|v| !r.sizes.iter().any(|s| s.id == v.size))
            .count();
        text.push_str(&fields.join(&sep.to_string()));
        text.push_str("\r\n");
    }
    let mut bytes = Vec::new();
    match format.encoding {
        CsvEncoding::Utf8 => bytes.extend_from_slice(text.as_bytes()),
        CsvEncoding::Utf8Bom => {
            bytes.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
            bytes.extend_from_slice(text.as_bytes());
        }
        CsvEncoding::Windows1252 => {
            for c in text.chars() {
                bytes.push(to_1252(c).ok_or_else(|| {
                    format!("le caractère « {c} » n'existe pas en Windows-1252 : choisissez UTF-8")
                })?);
            }
        }
    }
    Ok(MeasurementCsvExport {
        bytes,
        measurements: project.measurements.len(),
        omitted_values: omitted,
    })
}

fn decode(bytes: &[u8], enc: CsvEncoding) -> Result<String, CsvLineError> {
    let file_error = |m: String| CsvLineError {
        line: 0,
        column: None,
        message: m,
    };
    let has_bom = bytes.starts_with(&[0xEF, 0xBB, 0xBF]);
    match enc {
        CsvEncoding::Utf8 | CsvEncoding::Utf8Bom => {
            let body = if has_bom { &bytes[3..] } else { bytes };
            String::from_utf8(body.to_vec()).map_err(|e| {
                file_error(format!(
                    "octet invalide à la position {} : le fichier n'est pas en UTF-8 (essayez \
                     Windows-1252)",
                    e.utf8_error().valid_up_to() + if has_bom { 3 } else { 0 }
                ))
            })
        }
        CsvEncoding::Windows1252 => {
            if has_bom {
                return Err(file_error(
                    "marque UTF-8 détectée : le fichier est en UTF-8, pas en Windows-1252".into(),
                ));
            }
            let mut s = String::with_capacity(bytes.len());
            for (i, b) in bytes.iter().enumerate() {
                s.push(from_1252(*b).ok_or_else(|| {
                    file_error(format!(
                        "octet 0x{b:02X} indéfini en Windows-1252 à la position {i}"
                    ))
                })?);
            }
            Ok(s)
        }
    }
}

/// Découpe une ligne en champs (guillemets doublés). Erreur : (colonne, message).
fn split_fields(line: &str, sep: char) -> Result<Vec<String>, (usize, String)> {
    let chars: Vec<char> = line.chars().collect();
    let mut fields = Vec::new();
    let mut i = 0;
    loop {
        let mut field = String::new();
        if chars.get(i) == Some(&'"') {
            i += 1;
            loop {
                match chars.get(i) {
                    None => {
                        return Err((
                            fields.len() + 1,
                            "guillemet ouvert jamais refermé (un saut de ligne dans un champ \
                             n'est pas pris en charge)"
                                .into(),
                        ))
                    }
                    Some('"') if chars.get(i + 1) == Some(&'"') => {
                        field.push('"');
                        i += 2;
                    }
                    Some('"') => {
                        i += 1;
                        break;
                    }
                    Some(c) => {
                        field.push(*c);
                        i += 1;
                    }
                }
            }
            match chars.get(i) {
                None => {}
                Some(c) if *c == sep => {}
                Some(_) => {
                    return Err((
                        fields.len() + 1,
                        "texte après le guillemet fermant du champ".into(),
                    ))
                }
            }
        } else {
            while let Some(c) = chars.get(i) {
                if *c == sep {
                    break;
                }
                if *c == '"' {
                    return Err((
                        fields.len() + 1,
                        "guillemet au milieu d'un champ non guillemeté".into(),
                    ));
                }
                field.push(*c);
                i += 1;
            }
        }
        fields.push(field);
        match chars.get(i) {
            None => return Ok(fields),
            Some(_) => i += 1,
        }
    }
}

/// Analyse stricte d'un nombre : chiffres, éventuellement un séparateur décimal et des chiffres.
fn parse_number(cell: &str, decimal: char) -> Result<f64, String> {
    let t = cell.trim();
    let other = if decimal == '.' { ',' } else { '.' };
    if t.starts_with('-') {
        return Err(format!("valeur négative « {t} » refusée"));
    }
    if t.contains(other) {
        return Err(format!(
            "« {t} » : le séparateur décimal choisi est « {decimal} » (« {other} » n'est pas \
             interprété)"
        ));
    }
    let (int, frac) = match t.split_once(decimal) {
        Some((i, f)) => (i, Some(f)),
        None => (t, None),
    };
    let digits = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit());
    if !digits(int) || frac.is_some_and(|f| !digits(f)) {
        return Err(format!("« {t} » n'est pas un nombre"));
    }
    let normalized = match frac {
        Some(f) => format!("{int}.{f}"),
        None => int.to_string(),
    };
    let v: f64 = normalized
        .parse()
        .map_err(|_| format!("« {t} » n'est pas un nombre"))?;
    if !v.is_finite() || v > MAX_MEASUREMENT_VALUE {
        return Err(format!(
            "« {t} » dépasse la borne ({MAX_MEASUREMENT_VALUE} au plus)"
        ));
    }
    Ok(v)
}

fn err(line: usize, column: Option<usize>, message: impl Into<String>) -> CsvLineError {
    CsvLineError {
        line,
        column,
        message: message.into(),
    }
}

/// Lit un fichier de barème pour la gamme `range`. Tout ou rien : `Err` liste **toutes** les
/// erreurs (réglage invalide, encodage, en-tête, champs, nombres, unités, doublons), le projet
/// n'est jamais modifié.
pub fn read_measurement_csv(
    project: &Project,
    range: SizeRangeId,
    bytes: &[u8],
    format: &CsvFormat,
) -> Result<MeasurementCsv, Vec<CsvLineError>> {
    format.check().map_err(|m| vec![err(0, None, m)])?;
    let r = project
        .size_range(range)
        .map_err(|e| vec![err(0, None, e.to_string())])?;
    let text = decode(bytes, format.encoding).map_err(|e| vec![e])?;
    let sep = format.separator;
    let mut lines: Vec<(usize, &str)> = Vec::new();
    let mut blank = 0usize;
    let mut all: Vec<&str> = text.split('\n').collect();
    if all.last() == Some(&"") {
        all.pop();
    }
    for (i, raw) in all.iter().enumerate() {
        let l = raw.strip_suffix('\r').unwrap_or(raw);
        if l.is_empty() {
            blank += 1;
        } else {
            lines.push((i + 1, l));
        }
    }
    let Some((hline, header)) = lines.first().copied() else {
        return Err(vec![err(0, None, "fichier vide (aucun en-tête)")]);
    };
    let head = split_fields(header, sep).map_err(|(c, m)| vec![err(hline, Some(c), m)])?;
    let mut errors = Vec::new();
    let lower = |s: &str| s.trim().to_lowercase();
    if head.len() < 2
        || !["mesure", "measure"].contains(&lower(&head[0]).as_str())
        || !["unite", "unité", "unit"].contains(&lower(&head[1]).as_str())
    {
        errors.push(err(
            hline,
            None,
            "en-tête attendu : « mesure », « unite », puis les tailles",
        ));
    }
    let mut columns: Vec<Option<SizeId>> = Vec::new();
    for (k, cell) in head.iter().enumerate().skip(2) {
        let label = cell.trim();
        match r.sizes.iter().find(|s| s.label == label) {
            None => {
                errors.push(err(
                    hline,
                    Some(k + 1),
                    format!("taille « {label} » absente de la gamme « {} »", r.name),
                ));
                columns.push(None);
            }
            Some(s) if columns.contains(&Some(s.id)) => {
                errors.push(err(
                    hline,
                    Some(k + 1),
                    format!("taille « {label} » en double dans l'en-tête"),
                ));
                columns.push(None);
            }
            Some(s) => columns.push(Some(s.id)),
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    let mut rows: Vec<MeasurementImportRow> = Vec::new();
    let mut first_line: HashMap<String, usize> = HashMap::new();
    for (ln, raw) in lines.iter().skip(1) {
        let ln = *ln;
        let fields = match split_fields(raw, sep) {
            Ok(f) => f,
            Err((c, m)) => {
                errors.push(err(ln, Some(c), m));
                continue;
            }
        };
        if fields.len() != head.len() {
            errors.push(err(
                ln,
                None,
                format!("{} champ(s) au lieu de {}", fields.len(), head.len()),
            ));
            continue;
        }
        let name = fields[0].trim().to_string();
        let mut bad_row = false;
        if let Err(e) = check_stored_name(&name, "le nom de la mesure") {
            errors.push(err(ln, Some(1), e.to_string()));
            bad_row = true;
        } else if let Some(first) = first_line.get(&name.to_lowercase()) {
            errors.push(err(
                ln,
                Some(1),
                format!("la mesure « {name} » est déjà donnée ligne {first} (casse ignorée)"),
            ));
            bad_row = true;
        } else {
            first_line.insert(name.to_lowercase(), ln);
        }
        let unit = match lower(&fields[1]).as_str() {
            "mm" => Some(MeasurementUnit::Mm),
            "deg" => Some(MeasurementUnit::Deg),
            other => {
                errors.push(err(
                    ln,
                    Some(2),
                    format!("unité « {other} » inconnue (mm ou deg)"),
                ));
                bad_row = true;
                None
            }
        };
        if let (Some(u), false) = (
            unit,
            project
                .measurements
                .iter()
                .find(|m| m.name.to_lowercase() == name.to_lowercase())
                .is_none_or(|m| Some(m.unit) == unit),
        ) {
            errors.push(err(
                ln,
                Some(2),
                format!(
                    "la mesure « {name} » existe dans le projet avec une autre unité que {}",
                    unit_text(u)
                ),
            ));
            bad_row = true;
        }
        let mut cells = Vec::new();
        for (k, cell) in fields.iter().enumerate().skip(2) {
            let Some(size) = columns[k - 2] else { continue };
            if cell.trim().is_empty() {
                cells.push(MeasurementImportCell { size, value: None });
                continue;
            }
            match parse_number(cell, format.decimal) {
                Ok(v) => cells.push(MeasurementImportCell {
                    size,
                    value: Some(v),
                }),
                Err(m) => {
                    errors.push(err(ln, Some(k + 1), m));
                    bad_row = true;
                }
            }
        }
        if let (Some(unit), false) = (unit, bad_row) {
            rows.push(MeasurementImportRow { name, unit, cells });
        }
    }
    let new_names = rows
        .iter()
        .filter(|row| {
            !project
                .measurements
                .iter()
                .any(|m| m.name.to_lowercase() == row.name.to_lowercase())
        })
        .count();
    if errors.is_empty() && project.measurements.len() + new_names > MAX_MEASUREMENTS {
        errors.push(err(
            0,
            None,
            format!("trop de mesures : {MAX_MEASUREMENTS} au plus"),
        ));
    }
    if errors.is_empty() && rows.is_empty() {
        errors.push(err(0, None, "aucune ligne de mesure après l'en-tête"));
    }
    if errors.is_empty() {
        Ok(MeasurementCsv {
            rows,
            blank_lines: blank,
        })
    } else {
        Err(errors)
    }
}
