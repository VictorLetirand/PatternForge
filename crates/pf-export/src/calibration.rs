//! Étalonnage de l'imprimante (US-13.3) : facteurs de correction d'échelle X / Y et profils
//! nommés.
//!
//! **Sens du facteur.** Le facteur saisi est `taille attendue / taille mesurée` du carré étalon
//! imprimé : si le carré de 100 mm mesure 99,5 mm sur le papier, le facteur est `100 / 99,5`.
//! L'export **multiplie** la géométrie du patron par ce facteur (et non son inverse) : une
//! imprimante qui rétrécit de 0,5 % ramène alors le trait à sa vraie taille. Les facteurs
//! s'expriment dans les axes de la **page imprimée** (X = horizontal, Y = vertical du PDF) : un
//! profil mesuré en portrait vaut pour le portrait.
//!
//! Rien ici ne dit que l'imprimante est « calibrée » : le facteur n'est qu'un réglage saisi après
//! une mesure humaine ; son effet sur le papier se vérifie au réglet (AT-09).
//!
//! Les profils vivent dans les préférences de l'application, jamais dans le projet.

use std::fmt;

/// Plus petit facteur accepté (strictement positif, borné pour refuser une saisie absurde).
pub const SCALE_MIN: f64 = 0.90;
/// Plus grand facteur accepté.
pub const SCALE_MAX: f64 = 1.10;
/// Côté du carré étalon dont on mesure l'impression (mm).
pub const REFERENCE_MM: f64 = crate::CALIBRATION_MM;
/// Nombre maximal de profils enregistrés.
pub const MAX_PROFILES: usize = 16;
/// Nombre maximal de caractères d'un nom de profil.
pub const PROFILE_NAME_MAX_CHARS: usize = 40;

/// Axe d'un facteur.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    X,
    Y,
}

impl Axis {
    pub fn label(self) -> &'static str {
        match self {
            Axis::X => "X",
            Axis::Y => "Y",
        }
    }
}

/// Facteur ou mesure refusé. Aucun n'écrit quoi que ce soit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ScaleError {
    /// Un facteur n'est pas un nombre fini.
    NotFinite(Axis),
    /// Facteur hors de [0,90 ; 1,10].
    OutOfRange { axis: Axis, value: f64 },
    /// Longueur mesurée non finie ou négative ou nulle.
    BadMeasure { axis: Axis, value_mm: f64 },
}

impl fmt::Display for ScaleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFinite(a) => write!(
                f,
                "facteur {} : la valeur n'est pas un nombre fini",
                a.label()
            ),
            Self::OutOfRange { axis, value } => write!(
                f,
                "facteur {} : {} hors des bornes ({SCALE_MIN} à {SCALE_MAX})",
                axis.label(),
                value
            ),
            Self::BadMeasure { axis, value_mm } => write!(
                f,
                "mesure {} : {value_mm} mm n'est pas une longueur valide",
                axis.label()
            ),
        }
    }
}

impl std::error::Error for ScaleError {}

/// Correction d'échelle par axe. `1,0 / 1,0` (défaut) = aucune correction : le PDF est alors
/// identique à l'octet à l'export d'avant la story.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PrintScale {
    pub x: f64,
    pub y: f64,
}

impl Default for PrintScale {
    fn default() -> Self {
        PrintScale { x: 1.0, y: 1.0 }
    }
}

fn check_factor(axis: Axis, v: f64) -> Result<(), ScaleError> {
    if !v.is_finite() {
        Err(ScaleError::NotFinite(axis))
    } else if !(SCALE_MIN..=SCALE_MAX).contains(&v) {
        Err(ScaleError::OutOfRange { axis, value: v })
    } else {
        Ok(())
    }
}

impl PrintScale {
    /// Facteurs validés (finis, dans [0,90 ; 1,10]).
    pub fn new(x: f64, y: f64) -> Result<PrintScale, ScaleError> {
        let s = PrintScale { x, y };
        s.validate()?;
        Ok(s)
    }

    pub fn validate(&self) -> Result<(), ScaleError> {
        check_factor(Axis::X, self.x)?;
        check_factor(Axis::Y, self.y)
    }

    /// Aucune correction (`1,0` sur les deux axes).
    pub fn is_identity(&self) -> bool {
        self.x == 1.0 && self.y == 1.0
    }

    /// Facteurs déduits des longueurs **mesurées** (mm, arrondies au centième) du carré étalon
    /// de 100 mm imprimé : `100 / mesure`, arrondi à 6 décimales (valeur stable à l'écriture
    /// des préférences). Refuse une mesure non finie ou ≤ 0, et un facteur hors bornes.
    pub fn from_measures(measured_x_mm: f64, measured_y_mm: f64) -> Result<PrintScale, ScaleError> {
        let factor = |axis: Axis, measured: f64| -> Result<f64, ScaleError> {
            if !measured.is_finite() || measured <= 0.0 {
                return Err(ScaleError::BadMeasure {
                    axis,
                    value_mm: measured,
                });
            }
            let m = (measured * 100.0).round() / 100.0;
            if m <= 0.0 {
                return Err(ScaleError::BadMeasure {
                    axis,
                    value_mm: measured,
                });
            }
            let f = (REFERENCE_MM / m * 1e6).round() / 1e6;
            check_factor(axis, f)?;
            Ok(f)
        };
        Ok(PrintScale {
            x: factor(Axis::X, measured_x_mm)?,
            y: factor(Axis::Y, measured_y_mm)?,
        })
    }
}

/// Nom ou liste de profils refusé.
#[derive(Debug, Clone, PartialEq)]
pub enum ProfileError {
    NameEmpty,
    NameTooLong {
        max: usize,
    },
    NameControlChar,
    /// Un autre profil porte déjà ce nom (casse ignorée).
    Duplicate,
    TooMany {
        max: usize,
    },
    /// Indice de profil inexistant.
    Unknown,
    Scale(ScaleError),
}

impl fmt::Display for ProfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NameEmpty => write!(f, "le nom du profil est vide"),
            Self::NameTooLong { max } => {
                write!(f, "le nom du profil dépasse {max} caractères")
            }
            Self::NameControlChar => {
                write!(f, "le nom du profil contient un caractère de contrôle")
            }
            Self::Duplicate => write!(f, "un profil porte déjà ce nom"),
            Self::TooMany { max } => write!(f, "au plus {max} profils"),
            Self::Unknown => write!(f, "profil inconnu"),
            Self::Scale(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ProfileError {}

impl From<ScaleError> for ProfileError {
    fn from(e: ScaleError) -> Self {
        ProfileError::Scale(e)
    }
}

/// Nom nettoyé (espaces de bord retirés) et validé : 1 à 40 caractères, aucun caractère de
/// contrôle.
pub fn clean_profile_name(name: &str) -> Result<String, ProfileError> {
    let n = name.trim();
    if n.is_empty() {
        return Err(ProfileError::NameEmpty);
    }
    if n.chars().any(char::is_control) {
        return Err(ProfileError::NameControlChar);
    }
    if n.chars().count() > PROFILE_NAME_MAX_CHARS {
        return Err(ProfileError::NameTooLong {
            max: PROFILE_NAME_MAX_CHARS,
        });
    }
    Ok(n.to_string())
}

#[derive(Debug, Clone, PartialEq)]
pub struct PrinterProfile {
    pub name: String,
    pub scale: PrintScale,
}

/// Profils d'imprimante nommés (16 au plus) et profil actif. Chaque opération est tout ou
/// rien : un refus laisse la liste inchangée.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PrinterProfiles {
    list: Vec<PrinterProfile>,
    active: Option<usize>,
}

impl PrinterProfiles {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn list(&self) -> &[PrinterProfile] {
        &self.list
    }

    pub fn active_index(&self) -> Option<usize> {
        self.active
    }

    pub fn active(&self) -> Option<&PrinterProfile> {
        self.active.and_then(|i| self.list.get(i))
    }

    /// Correction du profil actif ; aucune correction sans profil actif.
    pub fn active_scale(&self) -> PrintScale {
        self.active().map(|p| p.scale).unwrap_or_default()
    }

    fn taken(&self, name: &str, except: Option<usize>) -> bool {
        let key = name.to_lowercase();
        self.list
            .iter()
            .enumerate()
            .any(|(i, p)| Some(i) != except && p.name.to_lowercase() == key)
    }

    /// Ajoute un profil (sans l'activer) ; rend son indice.
    pub fn add(&mut self, name: &str, scale: PrintScale) -> Result<usize, ProfileError> {
        let name = clean_profile_name(name)?;
        scale.validate()?;
        if self.list.len() >= MAX_PROFILES {
            return Err(ProfileError::TooMany { max: MAX_PROFILES });
        }
        if self.taken(&name, None) {
            return Err(ProfileError::Duplicate);
        }
        self.list.push(PrinterProfile { name, scale });
        Ok(self.list.len() - 1)
    }

    /// Renomme et/ou change les facteurs d'un profil.
    pub fn update(
        &mut self,
        index: usize,
        name: &str,
        scale: PrintScale,
    ) -> Result<(), ProfileError> {
        if index >= self.list.len() {
            return Err(ProfileError::Unknown);
        }
        let name = clean_profile_name(name)?;
        scale.validate()?;
        if self.taken(&name, Some(index)) {
            return Err(ProfileError::Duplicate);
        }
        self.list[index] = PrinterProfile { name, scale };
        Ok(())
    }

    /// Supprime un profil ; le profil actif reste le même (ou aucun s'il est supprimé).
    pub fn remove(&mut self, index: usize) -> Result<PrinterProfile, ProfileError> {
        if index >= self.list.len() {
            return Err(ProfileError::Unknown);
        }
        let removed = self.list.remove(index);
        self.active = match self.active {
            Some(a) if a == index => None,
            Some(a) if a > index => Some(a - 1),
            other => other,
        };
        Ok(removed)
    }

    /// Choisit le profil actif (`None` = aucune correction).
    pub fn set_active(&mut self, index: Option<usize>) -> Result<(), ProfileError> {
        if index.is_some_and(|i| i >= self.list.len()) {
            return Err(ProfileError::Unknown);
        }
        self.active = index;
        Ok(())
    }

    /// Profils lus d'un fichier : un profil invalide ou en double est écarté (jamais appliqué),
    /// au plus 16 sont gardés ; `active_name` désigne le profil actif s'il existe encore.
    pub fn from_parts(items: Vec<(String, f64, f64)>, active_name: Option<&str>) -> Self {
        let mut book = PrinterProfiles::new();
        for (name, x, y) in items {
            if let Ok(scale) = PrintScale::new(x, y) {
                let _ = book.add(&name, scale);
            }
        }
        if let Some(wanted) = active_name {
            let key = wanted.trim().to_lowercase();
            book.active = book.list.iter().position(|p| p.name.to_lowercase() == key);
        }
        book
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn facteur_est_attendu_sur_mesure() {
        let s = PrintScale::from_measures(99.5, 100.25).unwrap();
        assert_eq!(s.x, 1.005025);
        assert_eq!(s.y, 0.997506);
        assert!(PrintScale::from_measures(100.0, 100.0)
            .unwrap()
            .is_identity());
    }

    #[test]
    fn bornes_et_valeurs_pieges() {
        assert!(PrintScale::new(0.90, 1.10).is_ok());
        for bad in [0.0, -1.0, 0.8999, 1.1001, f64::NAN, f64::INFINITY] {
            assert!(PrintScale::new(bad, 1.0).is_err(), "{bad}");
            assert!(PrintScale::new(1.0, bad).is_err(), "{bad}");
        }
        for bad in [0.0, -5.0, f64::NAN, 0.001, 1e9, 200.0, 50.0] {
            assert!(PrintScale::from_measures(bad, 100.0).is_err(), "{bad}");
        }
    }

    #[test]
    fn profils_nom_doublon_limite() {
        let mut b = PrinterProfiles::new();
        let one = PrintScale::default();
        assert_eq!(b.add(" Bureau ", one), Ok(0));
        assert_eq!(b.list()[0].name, "Bureau");
        assert_eq!(b.add("bureau", one), Err(ProfileError::Duplicate));
        assert_eq!(b.add("  ", one), Err(ProfileError::NameEmpty));
        assert_eq!(b.add("a\nb", one), Err(ProfileError::NameControlChar));
        assert!(matches!(
            b.add(&"x".repeat(41), one),
            Err(ProfileError::NameTooLong { .. })
        ));
        for i in 1..MAX_PROFILES {
            b.add(&format!("p{i}"), one).unwrap();
        }
        assert!(matches!(
            b.add("de trop", one),
            Err(ProfileError::TooMany { .. })
        ));
        assert_eq!(b.list().len(), MAX_PROFILES);
        b.set_active(Some(3)).unwrap();
        b.remove(1).unwrap();
        assert_eq!(b.active().unwrap().name, "p3");
        b.remove(2).unwrap();
        assert_eq!(b.active(), None);
        assert_eq!(b.active_scale(), PrintScale::default());
    }
}
