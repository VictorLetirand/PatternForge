//! Couleurs propres au canevas et aux textes d'aide, pour les deux thèmes.
//!
//! Les couleurs de l'interface (panneaux, boutons, rôles de tracé) viennent des
//! jetons de `pf-ui` ; ce module ne porte que ce que les jetons ne couvrent pas
//! (grille, accrochage, aperçu, repères de mesure). Le thème courant est par fil
//! d'exécution, réglé une fois par image par [`set_dark`].

use eframe::egui::Color32;
use std::cell::Cell;

thread_local! {
    static DARK: Cell<bool> = const { Cell::new(false) };
}

pub fn set_dark(dark: bool) {
    DARK.with(|d| d.set(dark));
}

pub fn is_dark() -> bool {
    DARK.with(Cell::get)
}

#[derive(Clone, Copy)]
pub struct Palette {
    pub grid_minor: Color32,
    pub grid_major: Color32,
    pub grid_text: Color32,
    pub axis: Color32,
    pub select: Color32,
    pub preview: Color32,
    pub snap: Color32,
    pub ink: Color32,
    pub handle: Color32,
    /// Fond des étiquettes posées sur le canevas.
    pub label_bg: Color32,
    /// Remplissage clair des repères (poignées, contours de points).
    pub halo: Color32,
    pub point_constructed: Color32,
    pub point_free: Color32,
    pub angle: Color32,
    pub measure: Color32,
    /// Lignes d'aide (trait fin pointillé, distinct des tracés).
    pub guide: Color32,
    /// Éléments internes de pièce (US-04.3) : lignes et repères liés, jamais la couleur seule.
    pub internal: Color32,
    pub accent_text: Color32,
    pub muted: Color32,
    pub error: Color32,
}

/// Couleur d'une zone de parage, de collage ou de découpe interne (US-06.10) : elle s'ajoute au
/// motif, au tracé du contour et à l'étiquette, jamais seule.
pub fn zone_color(kind: pf_core::ZoneKind) -> Color32 {
    match (kind, is_dark()) {
        (pf_core::ZoneKind::Skive, false) => Color32::from_rgb(180, 83, 9),
        (pf_core::ZoneKind::Skive, true) => Color32::from_rgb(251, 146, 60),
        (pf_core::ZoneKind::Glue, false) => Color32::from_rgb(21, 128, 61),
        (pf_core::ZoneKind::Glue, true) => Color32::from_rgb(74, 222, 128),
        (pf_core::ZoneKind::InnerCut, false) => Color32::from_rgb(126, 34, 206),
        (pf_core::ZoneKind::InnerCut, true) => Color32::from_rgb(192, 132, 252),
    }
}

pub fn pal() -> Palette {
    if is_dark() {
        Palette {
            grid_minor: Color32::from_rgb(26, 33, 43),
            grid_major: Color32::from_rgb(40, 50, 64),
            grid_text: Color32::from_rgb(120, 134, 150),
            axis: Color32::from_rgb(90, 104, 122),
            select: Color32::from_rgb(245, 158, 11),
            preview: Color32::from_rgb(45, 212, 191),
            snap: Color32::from_rgb(251, 146, 60),
            ink: Color32::from_rgb(230, 237, 246),
            handle: Color32::from_rgb(255, 120, 100),
            label_bg: Color32::from_rgba_unmultiplied(21, 28, 38, 230),
            halo: Color32::from_rgb(15, 20, 27),
            point_constructed: Color32::from_rgb(245, 180, 80),
            point_free: Color32::from_rgb(45, 212, 191),
            angle: Color32::from_rgb(251, 170, 40),
            measure: Color32::from_rgb(244, 114, 182),
            guide: Color32::from_rgb(96, 165, 250),
            internal: Color32::from_rgb(52, 211, 153),
            accent_text: Color32::from_rgb(45, 212, 191),
            muted: Color32::from_rgb(169, 182, 199),
            error: Color32::from_rgb(255, 138, 128),
        }
    } else {
        Palette {
            grid_minor: Color32::from_rgb(236, 238, 240),
            grid_major: Color32::from_rgb(214, 219, 224),
            grid_text: Color32::from_rgb(95, 107, 122),
            axis: Color32::from_rgb(170, 180, 190),
            select: Color32::from_rgb(245, 158, 11),
            preview: Color32::from_rgb(13, 148, 136),
            snap: Color32::from_rgb(194, 65, 12),
            ink: Color32::from_rgb(17, 24, 39),
            handle: Color32::from_rgb(220, 90, 70),
            label_bg: Color32::from_rgba_unmultiplied(255, 255, 255, 230),
            halo: Color32::WHITE,
            point_constructed: Color32::from_rgb(234, 160, 60),
            point_free: Color32::from_rgb(15, 118, 110),
            angle: Color32::from_rgb(217, 119, 6),
            measure: Color32::from_rgb(190, 24, 93),
            guide: Color32::from_rgb(37, 99, 235),
            internal: Color32::from_rgb(4, 120, 87),
            accent_text: Color32::from_rgb(15, 118, 110),
            muted: Color32::from_rgb(0x4A, 0x5B, 0x70),
            error: Color32::from_rgb(0xB4, 0x23, 0x18),
        }
    }
}
