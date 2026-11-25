//! Unités d'affichage et de saisie des longueurs (US-16.5) : mm, cm, pouces fractionnaires.
//!
//! Le modèle reste en **millimètres `f64`** : ce module ne fait que convertir à l'affichage et à
//! la saisie. Un texte arrondi (fraction de pouce, centimètres à 3 décimales) n'est **jamais**
//! renvoyé dans le modèle : seule une saisie explicite passe par [`parse_length_with`]. Les
//! exports (PDF, SVG, DXF, fichier projet) ne lisent pas ce réglage et restent en mm.
//!
//! Le réglage courant est propre à chaque fil d'exécution (comme le séparateur décimal) ; par
//! défaut mm, précision 1/16 po.

use crate::input::format_number;
use std::cell::Cell;

/// Un pouce, en millimètres (définition internationale, exacte).
pub const MM_PER_INCH: f64 = 25.4;
/// Dénominateurs de pouce proposés (précision d'affichage des fractions).
pub const INCH_DENOMINATORS: [u32; 6] = [2, 4, 8, 16, 32, 64];
/// Précision par défaut : 1/16 de pouce.
pub const DEFAULT_INCH_DENOMINATOR: u32 = 16;
/// Écart (mm) au-delà duquel une fraction affichée est signalée comme arrondie (`≈`).
const EXACT_MM: f64 = 1e-6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LengthUnit {
    Mm,
    Cm,
    In,
}

impl LengthUnit {
    pub const ALL: [LengthUnit; 3] = [LengthUnit::Mm, LengthUnit::Cm, LengthUnit::In];

    pub fn code(self) -> &'static str {
        match self {
            LengthUnit::Mm => "mm",
            LengthUnit::Cm => "cm",
            LengthUnit::In => "in",
        }
    }

    pub fn from_code(code: &str) -> Option<LengthUnit> {
        LengthUnit::ALL.into_iter().find(|u| u.code() == code)
    }

    /// Millimètres dans une unité.
    pub fn mm_per_unit(self) -> f64 {
        match self {
            LengthUnit::Mm => 1.0,
            LengthUnit::Cm => 10.0,
            LengthUnit::In => MM_PER_INCH,
        }
    }

    /// Suffixe d'affichage (`mm`, `cm`, `"`).
    pub fn suffix(self) -> &'static str {
        match self {
            LengthUnit::Mm => "mm",
            LengthUnit::Cm => "cm",
            LengthUnit::In => "\"",
        }
    }

    pub fn from_mm(self, mm: f64) -> f64 {
        mm / self.mm_per_unit()
    }

    pub fn to_mm(self, value: f64) -> f64 {
        value * self.mm_per_unit()
    }
}

/// Réglage d'affichage : unité et précision des fractions de pouce.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DisplayUnits {
    pub unit: LengthUnit,
    pub inch_denominator: u32,
}

impl Default for DisplayUnits {
    fn default() -> Self {
        DisplayUnits {
            unit: LengthUnit::Mm,
            inch_denominator: DEFAULT_INCH_DENOMINATOR,
        }
    }
}

impl DisplayUnits {
    /// Dénominateur ramené à une valeur proposée (sinon 1/16).
    pub fn sanitized(self) -> DisplayUnits {
        let d = if INCH_DENOMINATORS.contains(&self.inch_denominator) {
            self.inch_denominator
        } else {
            DEFAULT_INCH_DENOMINATOR
        };
        DisplayUnits {
            unit: self.unit,
            inch_denominator: d,
        }
    }
}

thread_local! {
    static CURRENT: Cell<DisplayUnits> = const {
        Cell::new(DisplayUnits { unit: LengthUnit::Mm, inch_denominator: DEFAULT_INCH_DENOMINATOR })
    };
}

/// Règle l'unité d'affichage du fil courant.
pub fn set_display_units(units: DisplayUnits) {
    CURRENT.with(|c| c.set(units.sanitized()));
}

pub fn display_units() -> DisplayUnits {
    CURRENT.with(Cell::get)
}

fn gcd(a: u64, b: u64) -> u64 {
    if b == 0 {
        a
    } else {
        gcd(b, a % b)
    }
}

/// Pouces (positifs ou négatifs) en fraction au dénominateur donné : `1 1/16`, `3/8`, `0`.
/// Renvoie le texte et l'écart d'arrondi en pouces.
pub fn inches_to_fraction(inches: f64, denominator: u32) -> (String, f64) {
    let den = u64::from(denominator.max(1));
    let scaled = inches.abs() * den as f64;
    if !scaled.is_finite() || scaled > 1.0e12 {
        return (format_number(inches, 3), 0.0);
    }
    let n = scaled.round() as u64;
    let err = inches.abs() - n as f64 / den as f64;
    let (whole, rem) = (n / den, n % den);
    let g = gcd(rem, den).max(1);
    let text = match (whole, rem) {
        (0, 0) => "0".to_string(),
        (w, 0) => w.to_string(),
        (0, r) => format!("{}/{}", r / g, den / g),
        (w, r) => format!("{w} {}/{}", r / g, den / g),
    };
    let negative = inches < 0.0 && n > 0;
    (if negative { format!("-{text}") } else { text }, err)
}

/// Longueur affichée avec son unité (jamais renvoyée au modèle). En pouces, un `≈` signale une
/// valeur arrondie à la précision choisie.
pub fn format_length(mm: f64, units: DisplayUnits) -> String {
    if !mm.is_finite() {
        return "—".into();
    }
    let units = units.sanitized();
    match units.unit {
        LengthUnit::Mm => format!("{} mm", format_number(mm, 2)),
        LengthUnit::Cm => format!("{} cm", format_number(mm / 10.0, 3)),
        LengthUnit::In => {
            let (text, err_in) = inches_to_fraction(mm / MM_PER_INCH, units.inch_denominator);
            let approx = if (err_in * MM_PER_INCH).abs() > EXACT_MM {
                "≈ "
            } else {
                ""
            };
            format!("{approx}{text}\"")
        }
    }
}

/// Texte **exact** d'un champ de saisie (sans suffixe) : mm et cm en décimal, pouces en fraction
/// si la valeur en est une, sinon en pouces décimaux (6 décimales au plus). Sert à préremplir un
/// champ : le relire redonne la valeur du modèle à 1e-4 mm près en pouces, sans arrondi à la
/// fraction.
pub fn format_field(mm: f64, units: DisplayUnits) -> String {
    if !mm.is_finite() {
        return "—".into();
    }
    let units = units.sanitized();
    match units.unit {
        LengthUnit::Mm => format_number(mm, 3),
        LengthUnit::Cm => format_number(mm / 10.0, 4),
        LengthUnit::In => {
            let inches = mm / MM_PER_INCH;
            let (text, err_in) = inches_to_fraction(inches, units.inch_denominator);
            if (err_in * MM_PER_INCH).abs() <= 1e-9 {
                text
            } else {
                format_number(inches, 6)
            }
        }
    }
}

/// Analyse un nombre fractionnaire de pouces : `3/8`, `1 1/16`, `1-1/16`, `-1/2`.
fn parse_fraction_quantity(t: &str) -> Result<f64, String> {
    let bad = || format!("« {t} » n'est pas une fraction");
    let (negative, rest) = match t.strip_prefix('-') {
        Some(r) => (true, r.trim_start()),
        None => (false, t.strip_prefix('+').unwrap_or(t).trim_start()),
    };
    let (left, right) = rest.split_once('/').ok_or_else(bad)?;
    let digits = |s: &str| !s.is_empty() && s.len() <= 9 && s.chars().all(|c| c.is_ascii_digit());
    let (left, right) = (left.trim(), right.trim());
    if !digits(right) {
        return Err(bad());
    }
    let (whole, numerator) = match left.rfind([' ', '-']) {
        Some(i) => (left[..i].trim(), left[i + 1..].trim()),
        None => ("", left),
    };
    if !digits(numerator) || (!whole.is_empty() && !digits(whole)) {
        return Err(bad());
    }
    let den: f64 = right.parse().map_err(|_| bad())?;
    if den == 0.0 {
        return Err(format!("« {t} » : dénominateur nul"));
    }
    let whole: f64 = if whole.is_empty() {
        0.0
    } else {
        whole.parse().map_err(|_| bad())?
    };
    let numerator: f64 = numerator.parse().map_err(|_| bad())?;
    let value = whole + numerator / den;
    Ok(if negative { -value } else { value })
}

/// Analyse une longueur saisie et la renvoie en millimètres ; sans unité, `default` s'applique.
///
/// Unités : `mm`, `cm`, `m`, `in`, `po`, `"`, `″`. Les fractions (`3/8`, `1 1/16`, `1-1/16`) ne
/// sont acceptées qu'en pouces. Refus : vide, texte, `NaN`, `inf`, exposant, dénominateur nul.
pub fn parse_length_with(text: &str, default: LengthUnit) -> Result<f64, String> {
    let normalized = text.replace(['\u{a0}', '\u{202f}'], " ").to_lowercase();
    let t = normalized.trim();
    if t.is_empty() {
        return Err("valeur vide".into());
    }
    let suffixes: [(&str, LengthUnit); 8] = [
        ("mm", LengthUnit::Mm),
        ("cm", LengthUnit::Cm),
        ("in", LengthUnit::In),
        ("po", LengthUnit::In),
        ("\"", LengthUnit::In),
        ("\u{2033}", LengthUnit::In),
        ("''", LengthUnit::In),
        ("m", LengthUnit::Mm),
    ];
    let (number, unit, factor_m) = suffixes
        .iter()
        .find_map(|(s, u)| {
            t.strip_suffix(s)
                .map(|n| (n.trim(), *u, if *s == "m" { 1000.0 } else { 1.0 }))
        })
        .unwrap_or((t, default, 1.0));
    let quantity = if number.contains('/') {
        if unit != LengthUnit::In {
            return Err(format!(
                "« {number} » : les fractions ne sont acceptées qu'en pouces"
            ));
        }
        parse_fraction_quantity(number)?
    } else {
        let compact: String = number.chars().filter(|c| !c.is_whitespace()).collect();
        if compact.is_empty() {
            return Err("valeur vide".into());
        }
        crate::input::parse_number(&compact)?
    };
    let mm = unit.to_mm(quantity) * factor_m;
    if mm.is_finite() {
        Ok(mm)
    } else {
        Err(format!("« {t} » n'est pas un nombre"))
    }
}

/// Longueur saisie, unité d'affichage courante par défaut.
pub fn parse_length_display(text: &str) -> Result<f64, String> {
    parse_length_with(text, display_units().unit)
}

/// Texte de préremplissage d'un champ de longueur : en mm, `format_number(mm, 3)` (comme avant) ;
/// en cm ou en pouces, le texte qui relu redonne **exactement** les mm du modèle (fraction ou
/// décimal) et, à défaut, la valeur en mm avec son unité explicite (`10 mm`) : relire un champ
/// non modifié ne dérive jamais. Jamais renvoyé au modèle sans saisie explicite.
pub fn field_text(mm: f64) -> String {
    exact_field(mm, display_units())
}

fn exact_field(mm: f64, units: DisplayUnits) -> String {
    let units = units.sanitized();
    let candidate = format_field(mm, units);
    if units.unit == LengthUnit::Mm || !mm.is_finite() {
        return candidate;
    }
    match parse_length_with(&candidate, units.unit) {
        Ok(back) if (back - mm).abs() <= 1e-12 * mm.abs().max(1.0) => candidate,
        _ => format!("{} mm", format_number(mm, 9)),
    }
}

/// Texte d'un champ de saisie relu dans l'ancienne unité puis réécrit dans la nouvelle (formulaires
/// en cours quand l'unité d'affichage change) ; un texte illisible ou vide est laissé tel quel.
/// Les mm relus sont conservés **exactement** (voir [`field_text`]).
pub fn convert_length_text(text: &str, from: DisplayUnits, to: DisplayUnits) -> String {
    match parse_length_with(text, from.unit) {
        Ok(mm) if to.unit == LengthUnit::Mm => {
            let candidate = format_number(mm, 3);
            match parse_length_with(&candidate, LengthUnit::Mm) {
                Ok(back) if (back - mm).abs() <= 1e-12 * mm.abs().max(1.0) => candidate,
                _ => format!("{} mm", format_number(mm, 9)),
            }
        }
        Ok(mm) => exact_field(mm, to),
        Err(_) => text.to_string(),
    }
}

/// Comme [`len_text`] **sans suffixe** (le gabarit dit l'unité : voir `pf_i18n::tr_len`, qui
/// remplace « {x} mm » par l'unité courante) ; en pouces, la fraction, précédée de `≈ ` si elle
/// est arrondie.
pub fn len_number(mm: f64, decimals: usize) -> String {
    if !mm.is_finite() {
        return "—".into();
    }
    let units = display_units();
    match units.unit {
        LengthUnit::Mm => format_number(mm, decimals),
        LengthUnit::Cm => format_number(mm / 10.0, decimals + 1),
        LengthUnit::In => {
            let (text, err_in) = inches_to_fraction(mm / MM_PER_INCH, units.inch_denominator);
            let approx = if (err_in * MM_PER_INCH).abs() > EXACT_MM {
                "≈ "
            } else {
                ""
            };
            format!("{approx}{text}")
        }
    }
}

/// Deux longueurs (`x ; y`) dans l'unité d'affichage, sans suffixe.
pub fn len_pair(x_mm: f64, y_mm: f64, decimals: usize) -> String {
    format!(
        "{} ; {}",
        len_number(x_mm, decimals),
        len_number(y_mm, decimals)
    )
}

/// Longueur affichée avec son unité dans l'unité courante. En mm : exactement
/// `format!("{} mm", format_number(mm, decimals))` ; en cm une décimale de plus ; en pouces une
/// fraction (`≈` si arrondie, voir [`format_length`]).
pub fn len_text(mm: f64, decimals: usize) -> String {
    let units = display_units();
    match units.unit {
        LengthUnit::Mm => format!("{} mm", format_number(mm, decimals)),
        LengthUnit::Cm => format!("{} cm", format_number(mm / 10.0, decimals + 1)),
        LengthUnit::In => format_length(mm, units),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inch(denominator: u32) -> DisplayUnits {
        DisplayUnits {
            unit: LengthUnit::In,
            inch_denominator: denominator,
        }
    }

    fn near(got: Result<f64, String>, want: f64) {
        let got = got.expect("valeur valide");
        assert!((got - want).abs() < 1e-9, "{got} != {want}");
    }

    #[test]
    fn valeurs_a_la_main() {
        near(parse_length_with("1 in", LengthUnit::Mm), 25.4);
        near(parse_length_with("3/8", LengthUnit::In), 9.525);
        near(parse_length_with("1 1/16", LengthUnit::In), 26.9875);
        near(parse_length_with("1-1/16\"", LengthUnit::Mm), 26.9875);
        near(parse_length_with("-1/2 po", LengthUnit::Mm), -12.7);
        near(parse_length_with("2,5", LengthUnit::Cm), 25.0);
        near(parse_length_with("2,5", LengthUnit::In), 63.5);
        near(parse_length_with("120", LengthUnit::Mm), 120.0);
    }

    #[test]
    fn affichage_a_la_main() {
        let mm = DisplayUnits::default();
        assert_eq!(format_length(25.4, mm), "25,4 mm");
        let cm = DisplayUnits {
            unit: LengthUnit::Cm,
            ..mm
        };
        assert_eq!(format_length(25.4, cm), "2,54 cm");
        assert_eq!(format_length(25.4, inch(16)), "1\"");
        assert_eq!(format_length(9.525, inch(16)), "3/8\"");
        assert_eq!(format_length(26.9875, inch(16)), "1 1/16\"");
        assert_eq!(format_length(26.9875, inch(8)), "≈ 1 1/8\"");
        assert_eq!(format_length(-12.7, inch(8)), "-1/2\"");
        assert_eq!(format_length(0.0, inch(8)), "0\"");
        assert_eq!(format_length(f64::NAN, inch(8)), "—");
    }

    #[test]
    fn la_precision_change_la_fraction_affichee() {
        // 10 mm = 0,3937 po : 3/8 (0,375) à 1/8, 25/64 (0,3906) à 1/64.
        assert_eq!(format_length(10.0, inch(8)), "≈ 3/8\"");
        assert_eq!(format_length(10.0, inch(64)), "≈ 25/64\"");
        assert_eq!(format_length(10.0, inch(16)), "≈ 3/8\"");
    }

    #[test]
    fn champ_exact_aller_retour() {
        for denominator in INCH_DENOMINATORS {
            let u = inch(denominator);
            for mm in [0.0, 0.123, 9.525, 10.0, 26.9875, 123.456_789, -47.3] {
                let back = parse_length_with(&format_field(mm, u), LengthUnit::In).expect("relu");
                assert!((back - mm).abs() < 1e-4, "{mm} -> {back}");
            }
        }
        // Fraction exacte : retour exact.
        near(
            parse_length_with(&format_field(26.9875, inch(16)), LengthUnit::In),
            26.9875,
        );
        assert_eq!(format_field(26.9875, inch(16)), "1 1/16");
    }

    #[test]
    fn saisies_refusees() {
        for bad in [
            "",
            " ",
            "abc",
            "1/0",
            "1/",
            "/2",
            "1//2",
            "1/2/3",
            "1,5/2",
            "1 1,5/2",
            "1/2.5",
            "nan",
            "inf",
            "1e3",
            "--5",
            "1/2 mm",
            "3/8 cm",
            "1 in 1/2",
            "12 pouces",
        ] {
            assert!(
                parse_length_with(bad, LengthUnit::In).is_err(),
                "{bad:?} aurait dû être refusé"
            );
        }
    }

    #[test]
    fn le_reglage_est_propre_au_fil_et_assaini() {
        set_display_units(inch(32));
        assert_eq!(display_units(), inch(32));
        let other = std::thread::spawn(display_units).join().expect("fil");
        assert_eq!(other, DisplayUnits::default());
        set_display_units(DisplayUnits {
            unit: LengthUnit::In,
            inch_denominator: 7,
        });
        assert_eq!(display_units().inch_denominator, 16);
        set_display_units(DisplayUnits::default());
    }

    #[test]
    fn champs_exacts_conversion_et_nombres_sans_suffixe() {
        set_display_units(inch(16));
        assert_eq!(field_text(25.4), "1");
        assert_eq!(field_text(9.525), "3/8");
        // 10 mm n'est pas une fraction de 1/16 : mm explicite, relu à l'identique.
        assert_eq!(field_text(10.0), "10,0 mm");
        near(parse_length_display(&field_text(10.0)), 10.0);
        assert_eq!(len_number(10.0, 2), "≈ 3/8");
        assert_eq!(len_number(f64::NAN, 2), "—");
        assert_eq!(len_pair(25.4, 9.525, 2), "1 ; 3/8");
        set_display_units(DisplayUnits {
            unit: LengthUnit::Cm,
            inch_denominator: 16,
        });
        assert_eq!(field_text(125.0), "12,5");
        assert_eq!(len_number(125.0, 2), "12,5");
        assert_eq!(len_text(125.0, 2), "12,5 cm");
        set_display_units(DisplayUnits::default());
        assert_eq!(len_text(12.5, 3), "12,5 mm");
        assert_eq!(field_text(12.5), "12,5");
        let mm = DisplayUnits::default();
        // Conversion de formulaire : mm -> pouces -> mm sans dérive.
        let there = convert_length_text("100", mm, inch(16));
        assert_eq!(there, "100,0 mm");
        assert_eq!(convert_length_text(&there, inch(16), mm), "100,0");
        assert_eq!(convert_length_text("25,4", mm, inch(16)), "1");
        assert_eq!(convert_length_text("3/8", inch(16), mm), "9,525");
        assert_eq!(convert_length_text("abc", mm, inch(16)), "abc");
        assert_eq!(convert_length_text("", mm, inch(16)), "");
    }
}
