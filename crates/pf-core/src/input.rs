//! Saisie et affichage des nombres à la française.

use pf_geometry::PointMm;

/// Analyse une longueur saisie et la renvoie en millimètres.
///
/// Formes acceptées : `120`, `120,5`, `120.5`, `120 mm`, `12 cm`, `1,2 m`, `1 in`, `3/8"`.
/// Sans unité, la valeur est en millimètres (voir `units::parse_length_display` pour l'unité
/// d'affichage choisie).
pub fn parse_length_mm(text: &str) -> Result<f64, String> {
    crate::units::parse_length_with(text, crate::units::LengthUnit::Mm)
}

/// Analyse un angle en degrés : `90`, `90°`, `-45,5`.
pub fn parse_angle_deg(text: &str) -> Result<f64, String> {
    let t: String = text.trim().chars().filter(|c| !c.is_whitespace()).collect();
    let t = t.strip_suffix('°').unwrap_or(&t);
    if t.is_empty() {
        return Err("valeur vide".into());
    }
    parse_number(t)
}

/// Analyse une proportion saisie comme **fraction** (`0,25`, `1`) : un nombre sans unité.
pub fn parse_fraction(text: &str) -> Result<f64, String> {
    let t: String = text.trim().chars().filter(|c| !c.is_whitespace()).collect();
    if t.is_empty() {
        return Err("valeur vide".into());
    }
    parse_number(&t)
}

/// Coordonnée saisie au clavier (US-16.2), déjà résolue en un point du plan.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CoordInput {
    /// `x;y` : position absolue dans le repère du modèle.
    Absolute { point: PointMm },
    /// `@dx;dy` : décalage par rapport à l'origine donnée.
    Relative {
        dx_mm: f64,
        dy_mm: f64,
        point: PointMm,
    },
    /// `@L<A` : longueur et angle (degrés, sens trigonométrique, Y vers le haut)
    /// par rapport à l'origine donnée.
    Polar {
        length_mm: f64,
        angle_deg: f64,
        point: PointMm,
    },
}

impl CoordInput {
    /// Point désigné par la saisie.
    pub fn point(&self) -> PointMm {
        match *self {
            CoordInput::Absolute { point }
            | CoordInput::Relative { point, .. }
            | CoordInput::Polar { point, .. } => point,
        }
    }
}

/// Vrai si le texte a la forme d'une coordonnée plutôt que d'un nombre :
/// préfixe `@`, `;` ou `<`. La virgule seule n'en fait pas une (c'est aussi la
/// virgule décimale) : voir [`parse_coordinate`].
pub fn is_coordinate_text(text: &str) -> bool {
    let t = text.trim();
    t.starts_with('@') || t.contains([';', '<'])
}

/// Analyse une coordonnée saisie, avec le séparateur décimal courant.
///
/// Formes : `x;y` (absolue) ; `@dx;dy` (relative à `origin`) ; `@L<A` (polaire
/// relative : longueur `L` > 0, angle `A` en degrés). Chaque valeur accepte les
/// formes de [`parse_length_mm`] (unités comprises) ou [`parse_angle_deg`].
/// Séparateur de `x` et `y` : `;` toujours ; la virgule aussi quand le
/// séparateur décimal est le point (`10.5,20`). Avec la virgule décimale, `10,5`
/// est un nombre, jamais deux : la virgule ne sépare alors rien.
/// Le point obtenu doit rester dans l'espace de travail (±1 000 m).
pub fn parse_coordinate(text: &str, origin: PointMm) -> Result<CoordInput, String> {
    parse_coordinate_with(text, origin, decimal_separator())
}

/// Comme [`parse_coordinate`], avec un séparateur décimal explicite (`,` ou `.`).
pub fn parse_coordinate_with(
    text: &str,
    origin: PointMm,
    decimal: char,
) -> Result<CoordInput, String> {
    let t = text.trim();
    if t.is_empty() {
        return Err("valeur vide".into());
    }
    let (relative, body) = match t.strip_prefix('@') {
        Some(rest) => (true, rest.trim()),
        None => (false, t),
    };
    if body.is_empty() {
        return Err("coordonnée vide après « @ »".into());
    }
    let in_range = |p: PointMm| p.validate().map_err(|e| e.to_string());
    if let Some((l, a)) = body.split_once('<') {
        if !relative {
            return Err("coordonnée polaire sans « @ » : écrivez @longueur<angle".into());
        }
        if a.contains('<') {
            return Err("un seul « < » est permis : @longueur<angle".into());
        }
        let length_mm =
            crate::units::parse_length_display(l).map_err(|e| format!("longueur : {e}"))?;
        let angle_deg = parse_angle_deg(a).map_err(|e| format!("angle : {e}"))?;
        if length_mm <= 0.0 {
            return Err("la longueur d'une coordonnée polaire doit être positive".into());
        }
        let point = pf_geometry::point_from_polar(origin, length_mm, angle_deg)
            .map_err(|e| e.to_string())?;
        return Ok(CoordInput::Polar {
            length_mm,
            angle_deg,
            point,
        });
    }
    let separator = if body.contains(';') {
        ';'
    } else if decimal == '.' && body.contains(',') {
        ','
    } else {
        return Err(format!(
            "« {body} » : séparez x et y par « ; » (ou « , » si le séparateur décimal est le point)"
        ));
    };
    let parts: Vec<&str> = body.split(separator).collect();
    let [x, y] = parts.as_slice() else {
        return Err(format!(
            "deux valeurs attendues (x{separator}y), {} trouvées",
            parts.len()
        ));
    };
    let x = crate::units::parse_length_display(x).map_err(|e| format!("x : {e}"))?;
    let y = crate::units::parse_length_display(y).map_err(|e| format!("y : {e}"))?;
    if relative {
        let point = in_range(PointMm::new(origin.x + x, origin.y + y))?;
        Ok(CoordInput::Relative {
            dx_mm: x,
            dy_mm: y,
            point,
        })
    } else {
        Ok(CoordInput::Absolute {
            point: in_range(PointMm::new(x, y))?,
        })
    }
}

pub(crate) fn parse_number(t: &str) -> Result<f64, String> {
    let separators = t.chars().filter(|c| *c == ',' || *c == '.').count();
    let plain = t
        .chars()
        .all(|c| c.is_ascii_digit() || matches!(c, ',' | '.' | '-' | '+'));
    if separators > 1 || !plain {
        return Err(format!("« {t} » n'est pas un nombre"));
    }
    match t.replace(',', ".").parse::<f64>() {
        Ok(v) if v.is_finite() => Ok(v),
        _ => Err(format!("« {t} » n'est pas un nombre")),
    }
}

/// Analyse la saisie d'une division (US-02.6) : un entier seul (`5`) = parts
/// égales ; sinon une liste de proportions séparées par `:`, `;` ou des espaces,
/// chacune un nombre ou une fraction (`1:2:1`, `1/4;1/2;1/4`, `0,5 1,5`).
/// Seule la syntaxe est vérifiée ici ; les bornes le sont par `DivisionSpec::ratios`.
pub fn parse_division(text: &str) -> Result<crate::DivisionSpec, String> {
    let normalized = text.replace([':', ';'], " ");
    let items: Vec<&str> = normalized.split_whitespace().collect();
    match items.as_slice() {
        [] => Err("valeur vide".into()),
        [one] if !text.contains([':', ';']) => one
            .parse::<usize>()
            .map(|parts| crate::DivisionSpec::Equal { parts })
            .map_err(|_| format!("« {one} » n'est pas un nombre entier de parts")),
        items => {
            let values = items
                .iter()
                .map(|item| match item.split_once('/') {
                    Some((a, b)) => {
                        let (a, b) = (parse_number(a)?, parse_number(b)?);
                        if b == 0.0 {
                            Err(format!("« {item} » : division par zéro"))
                        } else {
                            Ok(a / b)
                        }
                    }
                    None => parse_number(item),
                })
                .collect::<Result<Vec<f64>, String>>()?;
            Ok(crate::DivisionSpec::Proportions { values })
        }
    }
}

/// Formate un nombre avec virgule décimale : au moins 1 décimale, au plus
/// `max_decimals`, sans zéros superflus (`120,0` ; `141,42`).
pub fn format_number(value: f64, max_decimals: usize) -> String {
    if !value.is_finite() {
        return "—".into();
    }
    let mut s = format!("{:.*}", max_decimals.max(1), value);
    while s.ends_with('0') && !s.ends_with(".0") {
        s.pop();
    }
    if s.trim_start_matches('-')
        .chars()
        .all(|c| c == '0' || c == '.')
    {
        s = s.trim_start_matches('-').to_string();
    }
    s.replace('.', &decimal_separator().to_string())
}

thread_local! {
    static DECIMAL_SEPARATOR: std::cell::Cell<char> = const { std::cell::Cell::new(',') };
}

/// Règle le séparateur décimal des affichages (par fil d'exécution ; défaut `,`).
/// La saisie accepte toujours `,` et `.`.
pub fn set_decimal_separator(separator: char) {
    DECIMAL_SEPARATOR.with(|c| c.set(separator));
}

/// Séparateur décimal d'affichage courant.
pub fn decimal_separator() -> char {
    DECIMAL_SEPARATOR.with(std::cell::Cell::get)
}

/// Longueur affichée dans l'unité d'affichage du fil courant (mm par défaut). Texte d'affichage
/// seulement : il n'est jamais relu comme valeur du modèle.
pub fn format_mm(value: f64) -> String {
    crate::units::format_length(value, crate::units::display_units())
}

pub fn format_deg(value: f64) -> String {
    format!("{}°", format_number(value, 2))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn longueurs_formes_francaises() {
        assert_eq!(parse_length_mm("120"), Ok(120.0));
        assert_eq!(parse_length_mm("120,5"), Ok(120.5));
        assert_eq!(parse_length_mm("120.5"), Ok(120.5));
        assert_eq!(parse_length_mm(" 120,5 mm "), Ok(120.5));
        assert_eq!(parse_length_mm("12 cm"), Ok(120.0));
        assert_eq!(parse_length_mm("12,5cm"), Ok(125.0));
        assert_eq!(parse_length_mm("1,2 m"), Ok(1200.0));
        assert_eq!(parse_length_mm("12\u{a0}CM"), Ok(120.0));
    }

    #[test]
    fn saisies_refusees() {
        for bad in [
            "",
            "  ",
            "abc",
            "12,5,3",
            "1.2.3",
            "12 pouces",
            "NaN",
            "inf",
            "1e3",
            "--5",
        ] {
            assert!(
                parse_length_mm(bad).is_err(),
                "{bad:?} aurait dû être refusé"
            );
        }
        assert!(parse_angle_deg("quatre-vingt-dix").is_err());
    }

    #[test]
    fn angles() {
        assert_eq!(parse_angle_deg("90"), Ok(90.0));
        assert_eq!(parse_angle_deg("90°"), Ok(90.0));
        assert_eq!(parse_angle_deg("-45,5 °"), Ok(-45.5));
    }

    fn pt(x: f64, y: f64) -> PointMm {
        PointMm::new(x, y)
    }

    fn near(p: PointMm, x: f64, y: f64) {
        assert!(
            (p.x - x).abs() <= 1e-9 && (p.y - y).abs() <= 1e-9,
            "attendu ({x}, {y}), obtenu ({}, {})",
            p.x,
            p.y
        );
    }

    fn coord(text: &str, origin: PointMm, decimal: char) -> Result<CoordInput, String> {
        parse_coordinate_with(text, origin, decimal)
    }

    #[test]
    fn us162_coordonnees_absolues_fr_et_en() {
        let o = pt(5.0, 5.0);
        // FR : virgule décimale, « ; » sépare ; l'origine ne compte pas.
        let c = coord("10;20", o, ',').unwrap();
        assert!(matches!(c, CoordInput::Absolute { .. }));
        near(c.point(), 10.0, 20.0);
        near(coord("10,5;20,25", o, ',').unwrap().point(), 10.5, 20.25);
        near(coord(" -3,5 ; 7 ", o, ',').unwrap().point(), -3.5, 7.0);
        near(coord("12 cm;3 mm", o, ',').unwrap().point(), 120.0, 3.0);
        // EN : point décimal, « , » ou « ; » séparent.
        near(coord("10.5,20.25", o, '.').unwrap().point(), 10.5, 20.25);
        near(coord("10.5;20.25", o, '.').unwrap().point(), 10.5, 20.25);
        near(coord("10,5", o, '.').unwrap().point(), 10.0, 5.0);
        // « ; » prime : la virgule reste décimale même en EN.
        near(coord("10,5;2", o, '.').unwrap().point(), 10.5, 2.0);
        // FR : « 10,5 » est un nombre, pas une coordonnée.
        assert!(coord("10,5", o, ',').is_err());
        assert!(coord("10.5,20", o, ',').is_err());
    }

    #[test]
    fn us162_coordonnees_relatives() {
        let o = pt(10.0, 20.0);
        let c = coord("@3;4", o, ',').unwrap();
        assert!(matches!(c, CoordInput::Relative { .. }));
        near(c.point(), 13.0, 24.0);
        near(coord("@-3,5;4", o, ',').unwrap().point(), 6.5, 24.0);
        near(coord("@ -2 ; 0", o, ',').unwrap().point(), 8.0, 20.0);
        near(coord("@-1.5,2.5", o, '.').unwrap().point(), 8.5, 22.5);
        // Déplacement nul : accepté par le parseur (la commande refusera le segment).
        near(coord("@0;0", o, ',').unwrap().point(), 10.0, 20.0);
    }

    #[test]
    fn us162_coordonnees_polaires_relatives() {
        let o = pt(1.0, 2.0);
        // Multiples de 90° : exacts.
        near(coord("@30<0", o, ',').unwrap().point(), 31.0, 2.0);
        near(coord("@10<90", o, ',').unwrap().point(), 1.0, 12.0);
        near(coord("@10<180", o, ',').unwrap().point(), -9.0, 2.0);
        near(coord("@10<270", o, ',').unwrap().point(), 1.0, -8.0);
        near(coord("@100<-90", o, ',').unwrap().point(), 1.0, -98.0);
        // 100 mm à 45° : 100 / racine de 2 = 70,710678118654752.
        let c = coord("@100<45", pt(0.0, 0.0), ',').unwrap();
        near(c.point(), 70.710_678_118_654_75, 70.710_678_118_654_75);
        // 100 mm à 30° : (86,602540378443865 ; 50).
        near(
            coord("@100<30", pt(0.0, 0.0), ',').unwrap().point(),
            86.602_540_378_443_86,
            50.0,
        );
        match coord("@2 cm<90°", o, ',').unwrap() {
            CoordInput::Polar {
                length_mm,
                angle_deg,
                point,
            } => {
                assert_eq!((length_mm, angle_deg), (20.0, 90.0));
                near(point, 1.0, 22.0);
            }
            other => panic!("polaire attendue : {other:?}"),
        }
        near(coord("@12,5<0", o, ',').unwrap().point(), 13.5, 2.0);
    }

    #[test]
    fn us162_coordonnees_refusees() {
        let o = pt(0.0, 0.0);
        let too_long = "9".repeat(400);
        let huge = format!("1{}", "0".repeat(300));
        let cases = [
            "",
            "   ",
            "@",
            "@ ",
            "abc",
            "10",
            "1;2;3",
            "1;",
            ";2",
            "1;;2",
            "NaN;1",
            "1;inf",
            "-inf;0",
            "1e300;1",
            "1e3;1",
            "@@1;2",
            "@0<45",
            "@-5<45",
            "@5<",
            "@<45",
            "5<45",
            "@1<2<3",
            "@1<abc",
            "@1;2<45",
            "１;２",
            "１２;3",
            "10 pouces;2",
            "1,2,3",
        ];
        for bad in cases {
            for decimal in [',', '.'] {
                // Refusées quel que soit le séparateur décimal.
                let r = coord(bad, o, decimal);
                assert!(
                    r.is_err(),
                    "{bad:?} ({decimal:?}) aurait dû être refusé : {r:?}"
                );
            }
        }
        // Valeur finie mais hors de l'espace de travail (1e300, 400 chiffres).
        for bad in [
            format!("{huge};0"),
            format!("0;{too_long}"),
            format!("@{huge}<10"),
            "@900000;0".to_string(),
        ] {
            let origin = if bad.starts_with("@9") {
                pt(500_000.0, 0.0)
            } else {
                o
            };
            assert!(coord(&bad, origin, ',').is_err(), "{:.20}… accepté", bad);
        }
    }

    #[test]
    fn us162_texte_de_coordonnee_reconnu() {
        for yes in ["@1;2", " @3<4", "1;2", "@5", "30<45"] {
            assert!(is_coordinate_text(yes), "{yes:?}");
        }
        for no in ["", "12", "12,5", "12.5 mm", "1:3", "-4"] {
            assert!(!is_coordinate_text(no), "{no:?}");
        }
    }

    #[test]
    fn us162_parse_coordinate_suit_le_separateur_courant() {
        let o = pt(0.0, 0.0);
        set_decimal_separator(',');
        assert!(parse_coordinate("1,5", o).is_err());
        near(parse_coordinate("1,5;2", o).unwrap().point(), 1.5, 2.0);
        set_decimal_separator('.');
        near(parse_coordinate("1.5,2", o).unwrap().point(), 1.5, 2.0);
        set_decimal_separator(',');
    }

    #[test]
    fn affichage() {
        assert_eq!(format_mm(120.0), "120,0 mm");
        assert_eq!(format_mm(141.421_356), "141,42 mm");
        assert_eq!(format_mm(42.5), "42,5 mm");
        assert_eq!(format_deg(90.0), "90,0°");
        assert_eq!(format_number(-0.000_01, 2), "0,0");
        assert_eq!(format_number(f64::NAN, 2), "—");
    }
}
