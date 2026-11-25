//! Jetons sémantiques du design system. Aucun composant ne code de couleur en
//! dur : il lit ces jetons (via [`crate::theme::tokens`]).

use egui::Color32;

/// Construit une couleur opaque depuis `0xRRGGBB`.
const fn rgb(hex: u32) -> Color32 {
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

/// Échelle d'espacement fixe (points logiques).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spacing {
    pub xs: f32,
    pub s: f32,
    pub m: f32,
    pub l: f32,
    pub xl: f32,
}

/// Rayons de coins (points logiques).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Radii {
    pub s: u8,
    pub m: u8,
    pub l: u8,
}

/// Tailles de texte (points logiques).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextSizes {
    pub caption: f32,
    pub body: f32,
    pub mono: f32,
    pub heading: f32,
}

/// Jeu complet de jetons.
#[derive(Clone, Debug, PartialEq)]
pub struct Tokens {
    /// Thème sombre (pilote la base `egui::Visuals`).
    pub dark: bool,
    pub surface_panel: Color32,
    pub surface_canvas: Color32,
    pub surface_raised: Color32,
    pub border: Color32,
    pub border_strong: Color32,
    pub text_primary: Color32,
    pub text_secondary: Color32,
    pub text_disabled: Color32,
    /// Bleu d'accent (bordures, repères, sélection).
    pub action: Color32,
    /// Fond des boutons pleins portant du texte (contraste ≥ 4,5:1 avec `action_text`).
    pub action_strong: Color32,
    pub action_hover: Color32,
    pub action_pressed: Color32,
    pub action_text: Color32,
    pub selection_bg: Color32,
    pub focus_ring: Color32,
    pub danger: Color32,
    pub warning: Color32,
    pub success: Color32,
    pub role_construction: Color32,
    pub role_cutting: Color32,
    pub role_sewing: Color32,
    pub role_grainline: Color32,
    pub role_fold: Color32,
    pub role_axis: Color32,
    pub space: Spacing,
    pub radius: Radii,
    pub text: TextSizes,
}

const SPACING: Spacing = Spacing {
    xs: 2.0,
    s: 4.0,
    m: 8.0,
    l: 12.0,
    xl: 16.0,
};
const RADII: Radii = Radii { s: 2, m: 4, l: 6 };
const TEXT: TextSizes = TextSizes {
    caption: 11.0,
    body: 13.0,
    mono: 12.5,
    heading: 15.0,
};

impl Tokens {
    /// Thème de référence « Precision Light ».
    pub fn precision_light() -> Self {
        Self {
            dark: false,
            surface_panel: rgb(0xF7F9FC),
            surface_canvas: rgb(0xFCFDFE),
            surface_raised: rgb(0xFFFFFF),
            border: rgb(0xD5DCE6),
            border_strong: rgb(0xA3B1C4),
            text_primary: rgb(0x14263D),
            text_secondary: rgb(0x4A5B70),
            text_disabled: rgb(0x8A97A8),
            action: rgb(0x307AD6),
            action_strong: rgb(0x2B6CC0),
            action_hover: rgb(0x245CA6),
            action_pressed: rgb(0x1D4C8A),
            action_text: rgb(0xFFFFFF),
            selection_bg: rgb(0xEAF3FD),
            focus_ring: rgb(0x1E5FBF),
            danger: rgb(0xB42318),
            warning: rgb(0x8A5A00),
            success: rgb(0x17713A),
            role_construction: rgb(0x5B8FD0),
            role_cutting: rgb(0x14263D),
            role_sewing: rgb(0xD9480F),
            role_grainline: rgb(0x2F8F5B),
            role_fold: rgb(0x7B3FB5),
            role_axis: rgb(0x6B7B8F),
            space: SPACING,
            radius: RADII,
            text: TEXT,
        }
    }

    /// Variante sombre, expérimentale : mêmes rôles, valeurs non validées visuellement.
    pub fn precision_dark() -> Self {
        Self {
            dark: true,
            surface_panel: rgb(0x151C26),
            surface_canvas: rgb(0x0F141B),
            surface_raised: rgb(0x1D2633),
            border: rgb(0x2B3645),
            border_strong: rgb(0x52627A),
            text_primary: rgb(0xE6EDF6),
            text_secondary: rgb(0xA9B6C7),
            text_disabled: rgb(0x66758A),
            action: rgb(0x5B9BEA),
            action_strong: rgb(0x2B6CC0),
            action_hover: rgb(0x245CA6),
            action_pressed: rgb(0x1D4C8A),
            action_text: rgb(0xFFFFFF),
            selection_bg: rgb(0x1E3350),
            focus_ring: rgb(0x8DBBF5),
            danger: rgb(0xFF8A80),
            warning: rgb(0xF2B84B),
            success: rgb(0x5FD08A),
            role_construction: rgb(0x7FAAE0),
            role_cutting: rgb(0xE6EDF6),
            role_sewing: rgb(0xFF8A5C),
            role_grainline: rgb(0x5FD08A),
            role_fold: rgb(0xB58AE6),
            role_axis: rgb(0x8C9BB0),
            space: SPACING,
            radius: RADII,
            text: TEXT,
        }
    }

    /// Couleurs nommées, dans l'ordre d'affichage du catalogue.
    pub fn colors(&self) -> Vec<(&'static str, Color32)> {
        vec![
            ("surface_panel", self.surface_panel),
            ("surface_canvas", self.surface_canvas),
            ("surface_raised", self.surface_raised),
            ("border", self.border),
            ("border_strong", self.border_strong),
            ("text_primary", self.text_primary),
            ("text_secondary", self.text_secondary),
            ("text_disabled", self.text_disabled),
            ("action", self.action),
            ("action_strong", self.action_strong),
            ("action_hover", self.action_hover),
            ("action_pressed", self.action_pressed),
            ("action_text", self.action_text),
            ("selection_bg", self.selection_bg),
            ("focus_ring", self.focus_ring),
            ("danger", self.danger),
            ("warning", self.warning),
            ("success", self.success),
            ("role_construction", self.role_construction),
            ("role_cutting", self.role_cutting),
            ("role_sewing", self.role_sewing),
            ("role_grainline", self.role_grainline),
            ("role_fold", self.role_fold),
            ("role_axis", self.role_axis),
        ]
    }
}

/// Luminance relative WCAG 2.x d'une couleur opaque.
pub fn relative_luminance(c: Color32) -> f64 {
    let lin = |v: u8| {
        let s = f64::from(v) / 255.0;
        if s <= 0.039_28 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(c.r()) + 0.7152 * lin(c.g()) + 0.0722 * lin(c.b())
}

/// Rapport de contraste WCAG entre deux couleurs opaques (1,0 à 21,0).
pub fn contrast_ratio(a: Color32, b: Color32) -> f64 {
    let (la, lb) = (relative_luminance(a), relative_luminance(b));
    let (hi, lo) = if la >= lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(t: &Tokens, name: &str) {
        let pairs = [
            ("text_primary/panel", t.text_primary, t.surface_panel),
            ("text_primary/canvas", t.text_primary, t.surface_canvas),
            ("text_primary/raised", t.text_primary, t.surface_raised),
            ("text_secondary/panel", t.text_secondary, t.surface_panel),
            ("text_secondary/canvas", t.text_secondary, t.surface_canvas),
            ("action_text/action_strong", t.action_text, t.action_strong),
            ("action_text/action_hover", t.action_text, t.action_hover),
            ("danger/raised", t.danger, t.surface_raised),
            ("warning/raised", t.warning, t.surface_raised),
            ("success/raised", t.success, t.surface_raised),
        ];
        for (label, fg, bg) in pairs {
            let ratio = contrast_ratio(fg, bg);
            assert!(ratio >= 4.5, "{name} {label} : contraste {ratio:.2} < 4,5");
        }
        // Le bleu d'accent imposé sert aux bordures/repères : seuil 3:1 (composants d'interface).
        assert!(contrast_ratio(t.action, t.surface_panel) >= 3.0, "{name}");
        assert!(
            contrast_ratio(t.focus_ring, t.surface_panel) >= 3.0,
            "{name}"
        );
    }

    #[test]
    fn contraste_theme_clair() {
        check(&Tokens::precision_light(), "clair");
    }

    #[test]
    fn contraste_theme_sombre() {
        check(&Tokens::precision_dark(), "sombre");
    }

    #[test]
    fn valeurs_imposees() {
        let t = Tokens::precision_light();
        assert_eq!(t.action, Color32::from_rgb(0x30, 0x7A, 0xD6));
        assert_eq!(t.selection_bg, Color32::from_rgb(0xEA, 0xF3, 0xFD));
        assert_eq!(t.surface_panel, Color32::from_rgb(0xF7, 0xF9, 0xFC));
        assert_eq!(t.surface_canvas, Color32::from_rgb(0xFC, 0xFD, 0xFE));
        assert_eq!(t.text_primary, Color32::from_rgb(0x14, 0x26, 0x3D));
    }

    #[test]
    fn echelle_d_espacement_fixe() {
        let s = Tokens::precision_light().space;
        assert_eq!([s.xs, s.s, s.m, s.l, s.xl], [2.0, 4.0, 8.0, 12.0, 16.0]);
    }

    #[test]
    fn contraste_blanc_noir() {
        let r = contrast_ratio(Color32::WHITE, Color32::BLACK);
        assert!((r - 21.0).abs() < 1e-9);
    }
}
