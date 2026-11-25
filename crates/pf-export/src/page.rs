//! Formats de page, orientation et marges d'impression (US-13.1).
//!
//! Tout est en millimètres. Une `PageSetup` décrit seulement la **feuille de papier** et sa
//! zone non imprimable ; elle ne change jamais l'échelle (toujours 1:1) : un contenu plus
//! grand que la zone imprimable est découpé en pages avec recouvrement (ou refusé pour un
//! rouleau), jamais réduit. Le réglage par défaut (A4 portrait, marges de 10 mm, recouvrement
//! de 10 mm, aucune option d'assemblage) reproduit l'export d'avant ces stories, octet pour octet.
//!
//! Bornes : côté d'un format libre ou largeur de rouleau de 190 à 5000 mm ; marge de 0 à
//! 100 mm ; zone imprimable d'au moins 160 mm de large et (hors rouleau) 170 mm de haut, pour
//! que le carré étalon de 100 mm et ses consignes tiennent toujours dedans ; longueur d'un
//! rouleau de 20 000 mm au plus ; recouvrement de 0 à 50 mm (US-13.2), toujours au moins
//! 100 mm de pas utile entre deux pages.

use crate::calibration::{PrintScale, ScaleError};
use std::fmt;

/// Marge d'impression par défaut sur chaque bord (même valeur que `pdf::PAGE_MARGIN_MM`).
pub const DEFAULT_MARGIN_MM: f64 = 10.0;
/// Plus petit côté d'un format libre, plus petite largeur de rouleau.
pub const PAGE_SIDE_MIN_MM: f64 = 190.0;
/// Plus grand côté d'un format libre, plus grande largeur de rouleau.
pub const PAGE_SIDE_MAX_MM: f64 = 5000.0;
/// Plus grande marge acceptée sur un bord.
pub const MARGIN_MAX_MM: f64 = 100.0;
/// Zone imprimable minimale (largeur) : le carré étalon et ses consignes y tiennent.
pub const PRINTABLE_MIN_WIDTH_MM: f64 = 160.0;
/// Zone imprimable minimale (hauteur), hors rouleau.
pub const PRINTABLE_MIN_HEIGHT_MM: f64 = 170.0;
/// Plus grande longueur d'une page de rouleau ; au-delà, l'export est refusé.
pub const ROLL_LENGTH_MAX_MM: f64 = 20_000.0;
/// La longueur d'une page de rouleau est arrondie au multiple de cette valeur (au-dessus).
pub const ROLL_LENGTH_STEP_MM: f64 = 10.0;
/// Recouvrement par défaut entre deux pages voisines (US-13.2).
pub const DEFAULT_OVERLAP_MM: f64 = 10.0;
/// Plus grand recouvrement accepté.
pub const OVERLAP_MAX_MM: f64 = 50.0;
/// Pas utile minimal entre deux pages (fenêtre moins recouvrement) : jamais de pas nul ou
/// négatif, jamais de découpage en bandes minces.
pub const OVERLAP_MIN_STEP_MM: f64 = 100.0;

/// Format de papier. Les dimensions sont celles du format **portrait** ; l'orientation
/// paysage échange largeur et hauteur.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PageFormat {
    A4,
    A3,
    A0,
    Letter,
    /// Format libre : largeur × hauteur en mm (de 190 à 5000 mm chacune).
    Custom {
        width_mm: f64,
        height_mm: f64,
    },
    /// Rouleau de largeur donnée (de 190 à 5000 mm) : une seule page continue par feuille,
    /// dont la longueur est l'emprise du contenu arrondie à 10 mm au-dessus. L'orientation
    /// n'a aucun effet (la longueur est toujours dans le sens du rouleau).
    Roll {
        width_mm: f64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Orientation {
    Portrait,
    Landscape,
}

/// Marges imprimante : zone non imprimable sur chaque bord, en mm (de 0 à 100).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Margins {
    pub top_mm: f64,
    pub bottom_mm: f64,
    pub left_mm: f64,
    pub right_mm: f64,
}

impl Margins {
    pub const fn uniform(mm: f64) -> Margins {
        Margins {
            top_mm: mm,
            bottom_mm: mm,
            left_mm: mm,
            right_mm: mm,
        }
    }
}

impl Default for Margins {
    fn default() -> Self {
        Margins::uniform(DEFAULT_MARGIN_MM)
    }
}

/// Options d'assemblage des pages (US-13.2), toutes désactivées par défaut : un export sans
/// option est identique à l'octet à l'export d'avant la story.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AssemblyOptions {
    /// Repères de recalage (croix) dans les bandes de recouvrement et pages voisines
    /// indiquées sur les bords.
    pub marks: bool,
    /// Page de plan d'assemblage (miniature, pas à l'échelle 1:1) après la page étalon.
    pub plan: bool,
    /// Omet les pages sans aucun contenu et tourne la feuille de 90°, 180° ou 270° (à l'export
    /// seulement) si cela réduit le nombre de pages sans enfreindre un droit-fil.
    pub save_paper: bool,
}

/// Réglages de sortie : format, orientation, marges, recouvrement, options d'assemblage et
/// étalonnage de l'imprimante (US-13.3).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageSetup {
    pub format: PageFormat,
    pub orientation: Orientation,
    pub margins: Margins,
    /// Recouvrement entre deux pages voisines, de 0 à 50 mm (sans effet sur un rouleau).
    pub overlap_mm: f64,
    pub assembly: AssemblyOptions,
    /// Correction d'échelle X / Y du profil d'imprimante actif (`1,0 / 1,0` = aucune).
    pub scale: PrintScale,
    /// « Étalon renforcé » : règles de contrôle en mm papier **non corrigés** sur la page étalon
    /// et repères de contrôle de 100 mm sur chaque page. Toujours présents quand une
    /// correction est active ; absents par défaut (PDF par défaut identique à l'octet).
    pub control_marks: bool,
}

impl Default for PageSetup {
    fn default() -> Self {
        PageSetup {
            format: PageFormat::A4,
            orientation: Orientation::Portrait,
            margins: Margins::default(),
            overlap_mm: DEFAULT_OVERLAP_MM,
            assembly: AssemblyOptions::default(),
            scale: PrintScale::default(),
            control_marks: false,
        }
    }
}

/// Champ d'un réglage de page, pour désigner celui qui est refusé.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageField {
    Width,
    Height,
    RollWidth,
    MarginTop,
    MarginBottom,
    MarginLeft,
    MarginRight,
    Overlap,
}

impl PageField {
    pub fn label(self) -> &'static str {
        match self {
            PageField::Width => "largeur de page",
            PageField::Height => "hauteur de page",
            PageField::RollWidth => "largeur du rouleau",
            PageField::MarginTop => "marge haute",
            PageField::MarginBottom => "marge basse",
            PageField::MarginLeft => "marge gauche",
            PageField::MarginRight => "marge droite",
            PageField::Overlap => "recouvrement",
        }
    }
}

/// Entrée de réglage invalide ou contenu impossible à placer. Aucune n'écrit quoi que ce soit.
#[derive(Debug, Clone, PartialEq)]
pub enum PageSetupError {
    /// Une valeur n'est pas un nombre fini.
    NotFinite(PageField),
    /// Côté ou largeur hors de [190 ; 5000] mm.
    SideOutOfRange { field: PageField, value_mm: f64 },
    /// Marge hors de [0 ; 100] mm.
    MarginOutOfRange { field: PageField, value_mm: f64 },
    /// Zone imprimable (page moins marges) trop petite ou négative.
    PrintableTooSmall { width_mm: f64, height_mm: f64 },
    /// Page de rouleau plus longue que le maximum.
    RollTooLong { length_mm: f64 },
    /// Contenu plus large que la zone imprimable du rouleau (jamais réduit).
    ContentTooWide { content_mm: f64, printable_mm: f64 },
    /// Recouvrement hors de [0 ; 50] mm.
    OverlapOutOfRange { value_mm: f64 },
    /// Recouvrement qui ne laisse pas assez de pas utile entre deux pages.
    OverlapTooLarge { overlap_mm: f64, window_mm: f64 },
    /// Facteur de correction d'échelle invalide (US-13.3).
    Scale(ScaleError),
}

impl fmt::Display for PageSetupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFinite(field) => {
                write!(f, "{} : la valeur n'est pas un nombre fini", field.label())
            }
            Self::SideOutOfRange { field, value_mm } => write!(
                f,
                "{} : {} mm hors des bornes ({PAGE_SIDE_MIN_MM} à {PAGE_SIDE_MAX_MM} mm)",
                field.label(),
                crate::num(*value_mm)
            ),
            Self::MarginOutOfRange { field, value_mm } => write!(
                f,
                "{} : {} mm hors des bornes (0 à {MARGIN_MAX_MM} mm)",
                field.label(),
                crate::num(*value_mm)
            ),
            Self::PrintableTooSmall {
                width_mm,
                height_mm,
            } => write!(
                f,
                "zone imprimable {} × {} mm trop petite (minimum {PRINTABLE_MIN_WIDTH_MM} × \
                 {PRINTABLE_MIN_HEIGHT_MM} mm) : réduire les marges ou agrandir la page",
                crate::num(*width_mm),
                crate::num(*height_mm)
            ),
            Self::RollTooLong { length_mm } => write!(
                f,
                "le rouleau demanderait {} mm de long (maximum {ROLL_LENGTH_MAX_MM} mm) : \
                 export refusé plutôt que tronqué",
                crate::num(*length_mm)
            ),
            Self::ContentTooWide {
                content_mm,
                printable_mm,
            } => write!(
                f,
                "le contenu fait {} mm de large, le rouleau n'en offre que {} mm : export \
                 refusé plutôt que réduit",
                crate::num(*content_mm),
                crate::num(*printable_mm)
            ),
            Self::OverlapOutOfRange { value_mm } => write!(
                f,
                "recouvrement : {} mm hors des bornes (0 à {OVERLAP_MAX_MM} mm)",
                crate::num(*value_mm)
            ),
            Self::OverlapTooLarge {
                overlap_mm,
                window_mm,
            } => write!(
                f,
                "recouvrement de {} mm trop grand pour une fenêtre de {} mm : il doit rester au \
                 moins {OVERLAP_MIN_STEP_MM} mm de pas utile entre deux pages",
                crate::num(*overlap_mm),
                crate::num(*window_mm)
            ),
            Self::Scale(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for PageSetupError {}

impl From<ScaleError> for PageSetupError {
    fn from(e: ScaleError) -> Self {
        PageSetupError::Scale(e)
    }
}

/// Géométrie d'une page de sortie : taille, marges et fenêtre de patron (zone imprimable
/// moins le bandeau de titre).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageLayout {
    pub page_w_mm: f64,
    pub page_h_mm: f64,
    pub margins: Margins,
    pub window_w_mm: f64,
    pub window_h_mm: f64,
}

impl PageFormat {
    /// Dimensions portrait (largeur, hauteur) ; `None` pour un rouleau.
    pub fn portrait_mm(&self) -> Option<(f64, f64)> {
        match *self {
            PageFormat::A4 => Some((210.0, 297.0)),
            PageFormat::A3 => Some((297.0, 420.0)),
            PageFormat::A0 => Some((841.0, 1189.0)),
            PageFormat::Letter => Some((215.9, 279.4)),
            PageFormat::Custom {
                width_mm,
                height_mm,
            } => Some((width_mm, height_mm)),
            PageFormat::Roll { .. } => None,
        }
    }

    pub fn is_roll(&self) -> bool {
        matches!(self, PageFormat::Roll { .. })
    }

    /// Libellé court : « A4 », « Letter », « libre 500 × 300 mm », « rouleau 914 mm ».
    pub fn label(&self) -> String {
        self.label_lang(pf_i18n::Lang::Fr)
    }

    /// Libellé court dans la langue `lang` (US-19.2) ; en français, celui de [`PageFormat::label`].
    pub fn label_lang(&self, lang: pf_i18n::Lang) -> String {
        let t = |key: &str, args: &[(&str, String)]| {
            let mut s = pf_i18n::lookup(lang, key);
            for (n, v) in args {
                s = s.replace(&format!("{{{n}}}"), v);
            }
            s
        };
        match *self {
            PageFormat::A4 => "A4".into(),
            PageFormat::A3 => "A3".into(),
            PageFormat::A0 => "A0".into(),
            PageFormat::Letter => "Letter".into(),
            PageFormat::Custom {
                width_mm,
                height_mm,
            } => t(
                "pdfx.format.custom",
                &[("w", crate::num(width_mm)), ("h", crate::num(height_mm))],
            ),
            PageFormat::Roll { width_mm } => t("pdfx.format.roll", &[("w", crate::num(width_mm))]),
        }
    }
}

fn finite(field: PageField, v: f64) -> Result<(), PageSetupError> {
    if v.is_finite() {
        Ok(())
    } else {
        Err(PageSetupError::NotFinite(field))
    }
}

fn side(field: PageField, v: f64) -> Result<(), PageSetupError> {
    finite(field, v)?;
    if (PAGE_SIDE_MIN_MM..=PAGE_SIDE_MAX_MM).contains(&v) {
        Ok(())
    } else {
        Err(PageSetupError::SideOutOfRange { field, value_mm: v })
    }
}

fn margin(field: PageField, v: f64) -> Result<(), PageSetupError> {
    finite(field, v)?;
    if (0.0..=MARGIN_MAX_MM).contains(&v) {
        Ok(())
    } else {
        Err(PageSetupError::MarginOutOfRange { field, value_mm: v })
    }
}

/// Le pas entre deux pages (plus petite fenêtre moins recouvrement) reste d'au moins
/// [`OVERLAP_MIN_STEP_MM`] : jamais de pas nul ou négatif dans le découpage.
pub(crate) fn check_overlap_step(
    overlap_mm: f64,
    window_w_mm: f64,
    window_h_mm: f64,
) -> Result<(), PageSetupError> {
    let window = window_w_mm.min(window_h_mm);
    if window - overlap_mm < OVERLAP_MIN_STEP_MM {
        Err(PageSetupError::OverlapTooLarge {
            overlap_mm,
            window_mm: window,
        })
    } else {
        Ok(())
    }
}

impl PageSetup {
    /// Contrôle toutes les bornes. Un réglage invalide n'est jamais appliqué.
    pub fn validate(&self) -> Result<(), PageSetupError> {
        self.scale.validate()?;
        match self.format {
            PageFormat::Custom {
                width_mm,
                height_mm,
            } => {
                side(PageField::Width, width_mm)?;
                side(PageField::Height, height_mm)?;
            }
            PageFormat::Roll { width_mm } => side(PageField::RollWidth, width_mm)?,
            _ => {}
        }
        let m = self.margins;
        margin(PageField::MarginTop, m.top_mm)?;
        margin(PageField::MarginBottom, m.bottom_mm)?;
        margin(PageField::MarginLeft, m.left_mm)?;
        margin(PageField::MarginRight, m.right_mm)?;
        finite(PageField::Overlap, self.overlap_mm)?;
        if !(0.0..=OVERLAP_MAX_MM).contains(&self.overlap_mm) {
            return Err(PageSetupError::OverlapOutOfRange {
                value_mm: self.overlap_mm,
            });
        }
        let width = self.printable_width_mm();
        match self.printable_height_mm() {
            Some(height) => {
                if width < PRINTABLE_MIN_WIDTH_MM || height < PRINTABLE_MIN_HEIGHT_MM {
                    return Err(PageSetupError::PrintableTooSmall {
                        width_mm: width,
                        height_mm: height,
                    });
                }
                check_overlap_step(self.overlap_mm, width, height - crate::pdf::HEADER_MM)?;
            }
            None => {
                if width < PRINTABLE_MIN_WIDTH_MM {
                    return Err(PageSetupError::PrintableTooSmall {
                        width_mm: width,
                        height_mm: f64::INFINITY,
                    });
                }
            }
        }
        Ok(())
    }

    /// Repères de contrôle (règles non corrigées) : option cochée ou correction active.
    pub fn shows_control_marks(&self) -> bool {
        self.control_marks || !self.scale.is_identity()
    }

    /// Réglage par défaut : A4 portrait, marges de 10 mm (export historique).
    pub fn is_default(&self) -> bool {
        *self == PageSetup::default()
    }

    fn landscape(&self) -> bool {
        self.orientation == Orientation::Landscape && !self.format.is_roll()
    }

    /// Largeur de la page (mm), orientation appliquée ; largeur du rouleau pour un rouleau.
    pub fn page_width_mm(&self) -> f64 {
        match self.format {
            PageFormat::Roll { width_mm } => width_mm,
            f => {
                let (w, h) = f.portrait_mm().unwrap_or((0.0, 0.0));
                if self.landscape() {
                    h
                } else {
                    w
                }
            }
        }
    }

    /// Hauteur de la page (mm), orientation appliquée ; `None` pour un rouleau (longueur
    /// fixée par le contenu de chaque feuille).
    pub fn page_height_mm(&self) -> Option<f64> {
        let (w, h) = self.format.portrait_mm()?;
        Some(if self.landscape() { w } else { h })
    }

    /// Largeur de la zone imprimable (page moins marges gauche et droite), en mm.
    pub fn printable_width_mm(&self) -> f64 {
        self.page_width_mm() - self.margins.left_mm - self.margins.right_mm
    }

    /// Hauteur de la zone imprimable, en mm ; `None` pour un rouleau.
    pub fn printable_height_mm(&self) -> Option<f64> {
        Some(self.page_height_mm()? - self.margins.top_mm - self.margins.bottom_mm)
    }

    /// Libellé du format et de l'orientation : « A4 », « A3 paysage », « rouleau 914 mm ».
    pub fn describe(&self) -> String {
        self.describe_lang(pf_i18n::Lang::Fr)
    }

    /// Comme [`PageSetup::describe`], dans la langue `lang` (US-19.2).
    pub fn describe_lang(&self, lang: pf_i18n::Lang) -> String {
        let label = self.format.label_lang(lang);
        if self.landscape() {
            pf_i18n::lookup(lang, "pdfx.format.landscape").replace("{label}", &label)
        } else {
            label
        }
    }

    /// Géométrie d'une page. `roll_content_height_mm` n'est lu que pour un rouleau (emprise
    /// du contenu, bandeau non compris). Suppose un réglage validé.
    pub fn layout(&self, roll_content_height_mm: f64) -> PageLayout {
        let m = self.margins;
        let page_w = self.page_width_mm();
        let window_w = page_w - m.left_mm - m.right_mm;
        let (page_h, window_h) = match self.page_height_mm() {
            Some(h) => (h, h - m.top_mm - m.bottom_mm - crate::pdf::HEADER_MM),
            None => {
                let mut wh =
                    (roll_content_height_mm / ROLL_LENGTH_STEP_MM).ceil() * ROLL_LENGTH_STEP_MM;
                if wh < roll_content_height_mm {
                    wh += ROLL_LENGTH_STEP_MM;
                }
                (wh + m.top_mm + m.bottom_mm + crate::pdf::HEADER_MM, wh)
            }
        };
        PageLayout {
            page_w_mm: page_w,
            page_h_mm: page_h,
            margins: m,
            window_w_mm: window_w,
            window_h_mm: window_h,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaut_est_a4_portrait_marges_10() {
        let s = PageSetup::default();
        assert!(s.is_default());
        assert_eq!(s.page_width_mm(), 210.0);
        assert_eq!(s.page_height_mm(), Some(297.0));
        let l = s.layout(0.0);
        assert_eq!(l.window_w_mm, crate::pdf::WINDOW_W_MM);
        assert_eq!(l.window_h_mm, crate::pdf::WINDOW_H_MM);
        assert!(s.validate().is_ok());
    }

    #[test]
    fn paysage_echange_et_rouleau_ignore_l_orientation() {
        let mut s = PageSetup {
            format: PageFormat::A3,
            orientation: Orientation::Landscape,
            ..PageSetup::default()
        };
        assert_eq!(
            (s.page_width_mm(), s.page_height_mm()),
            (420.0, Some(297.0))
        );
        s.format = PageFormat::Roll { width_mm: 914.0 };
        assert_eq!((s.page_width_mm(), s.page_height_mm()), (914.0, None));
        s.orientation = Orientation::Portrait;
        assert_eq!(s.page_width_mm(), 914.0);
    }

    #[test]
    fn bornes() {
        let ok = |s: PageSetup| s.validate().is_ok();
        let custom = |w, h| PageSetup {
            format: PageFormat::Custom {
                width_mm: w,
                height_mm: h,
            },
            ..PageSetup::default()
        };
        assert!(ok(custom(190.0, 190.0)));
        assert!(ok(custom(5000.0, 5000.0)));
        assert!(!ok(custom(189.9, 300.0)));
        assert!(!ok(custom(300.0, 5000.1)));
        assert!(!ok(custom(f64::NAN, 300.0)));
        assert!(!ok(custom(300.0, f64::INFINITY)));
        assert!(!ok(custom(-300.0, 300.0)));
        assert!(!ok(custom(0.0, 300.0)));
    }

    #[test]
    fn recouvrement_bornes_et_pas_utile() {
        let with = |mm: f64| PageSetup {
            overlap_mm: mm,
            ..PageSetup::default()
        };
        for ok in [0.0, 10.0, 50.0] {
            assert!(with(ok).validate().is_ok(), "{ok}");
        }
        for (bad, nan) in [(-0.1, false), (50.01, false), (f64::NAN, true)] {
            let err = with(bad).validate().unwrap_err();
            if nan {
                assert_eq!(err, PageSetupError::NotFinite(PageField::Overlap));
            } else {
                assert!(matches!(err, PageSetupError::OverlapOutOfRange { .. }));
            }
        }
        assert!(with(f64::INFINITY).validate().is_err());
        // Le pas utile ne peut jamais être nul ou négatif, même hors des bornes habituelles.
        assert!(check_overlap_step(50.0, 160.0, 162.0).is_ok());
        assert!(matches!(
            check_overlap_step(50.0, 140.0, 400.0),
            Err(PageSetupError::OverlapTooLarge { .. })
        ));
        assert!(check_overlap_step(10.0, 100.0, 1000.0).is_err());
        assert!(check_overlap_step(0.0, 99.0, 99.0).is_err());
    }
}
