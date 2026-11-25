//! E14 (US-14.1) — calibration de l'image de fond et commandes `*Background`.
//!
//! Le stockage (`Sheet.background`, schéma 6) est dans `sheet_background.rs` ; ce module ajoute le
//! calcul et les commandes. **Conventions (à faire valider par la modéliste, voir le suivi) :**
//!
//! - repère de l'image : pixels, origine en haut à gauche, `y` vers le bas (comme le fichier) ;
//!   repère du plan : mm, `y` vers le haut ;
//! - **deux points** : `p1`, `p2` sont à `known_distance_mm` l'un de l'autre ; échelle
//!   `s = D / |p2 - p1|` (mm par pixel, **même échelle en x et en y**, aucune rotation, aucune
//!   perspective) ; `p1` est posé à l'origine du plan, `mm = (s (x - x1), -s (y - y1))` ;
//! - **quatre points** : les coins d'un rectangle réel `L × H`, cliqués dans l'ordre **anti-horaire
//!   tel qu'on voit l'image** (bas-gauche, bas-droite, haut-droite, haut-gauche) ; ils sont posés sur
//!   `(0 ; 0)`, `(L ; 0)`, `(L ; H)`, `(0 ; H)` par l'**homographie** (correction de perspective) ;
//!   l'homographie est **recalculée à chaque lecture** (`calibration_map`), jamais stockée ;
//! - **résidu** : écart, **observé** à la calibration, entre la position de chaque point calibré
//!   après transformation et sa position réelle attendue ; `residual_mm` stocké = RMS des écarts par
//!   point. Avec le **minimum** de points (2 ou 4) la transformation passe exactement par eux : le
//!   résidu est de l'ordre de l'erreur numérique (≈ 0) et **ne mesure pas l'erreur de pointage** ni la
//!   déformation de l'objectif ; il est affiché tel quel, jamais comme une garantie de précision ;
//! - le fond n'est **pas** de la géométrie : jamais exporté (PDF, SVG, DXF, HPGL inchangés), jamais
//!   lié à un point du modèle. Verrouillé : calibration, reliage et retrait refusés (l'affichage
//!   reste réglable pour le déverrouiller).

use crate::error::CoreError;
use crate::model::*;
use crate::sheet_background::{
    unsafe_image_path, Background, Calibration, PlanePoint, MAX_BACKGROUND_PX,
};
use pf_geometry::{Homography, PointMm};
use serde::{Deserialize, Serialize};

/// Opacité d'un fond nouvellement posé.
pub const DEFAULT_BACKGROUND_OPACITY: f64 = 0.5;

/// Calibration demandée (sans résidu : il est calculé par la commande).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CalibrationSpec {
    TwoPoints {
        points_px: [PlanePoint; 2],
        known_distance_mm: f64,
    },
    FourPoints {
        points_px: [PlanePoint; 4],
        known_width_mm: f64,
        known_height_mm: f64,
    },
}

/// Transformation pixels de l'image ↔ mm du plan, déduite d'une calibration.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BackgroundMap {
    /// Échelle uniforme (mm par pixel) et point d'origine (pixel posé en `(0 ; 0)`).
    Similarity { origin: PlanePoint, mm_per_px: f64 },
    /// Homographie pixels -> mm.
    Projective(Homography),
}

/// Résidus d'une calibration, recalculés à la lecture.
#[derive(Debug, Clone, PartialEq)]
pub struct Residuals {
    /// Écart (mm) pour chaque point calibré, dans l'ordre de la calibration.
    pub per_point_mm: Vec<f64>,
    /// Racine de la moyenne des carrés de ces écarts (mm).
    pub rms_mm: f64,
}

fn invalid(msg: impl Into<String>) -> CoreError {
    CoreError::Invalid(msg.into())
}

fn px_dist(a: PlanePoint, b: PlanePoint) -> f64 {
    ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt()
}

fn flipped(p: PlanePoint) -> PointMm {
    PointMm::new(p.x, -p.y)
}

/// Les quatre coins, vus avec `y` vers le haut, forment un quadrilatère convexe parcouru dans le
/// sens anti-horaire (chaque virage à gauche).
fn is_convex_ccw(p: &[PlanePoint; 4]) -> bool {
    let q: Vec<PointMm> = p.iter().map(|v| flipped(*v)).collect();
    (0..4).all(|i| {
        let (a, b, c) = (q[i], q[(i + 1) % 4], q[(i + 2) % 4]);
        let (ux, uy, vx, vy) = (b.x - a.x, b.y - a.y, c.x - b.x, c.y - b.y);
        let cross = ux * vy - uy * vx;
        let scale = (ux.hypot(uy) * vx.hypot(vy)).max(1e-300);
        cross > 1e-9 * scale
    })
}

fn rect_corners(w: f64, h: f64) -> [PointMm; 4] {
    [
        PointMm::new(0.0, 0.0),
        PointMm::new(w, 0.0),
        PointMm::new(w, h),
        PointMm::new(0.0, h),
    ]
}

impl BackgroundMap {
    /// Pixel de l'image -> mm du plan ; `None` si le point part à l'infini.
    pub fn to_mm(&self, px: PointMm) -> Option<PointMm> {
        match self {
            BackgroundMap::Similarity { origin, mm_per_px } => Some(PointMm::new(
                (px.x - origin.x) * mm_per_px,
                -(px.y - origin.y) * mm_per_px,
            )),
            BackgroundMap::Projective(h) => h.apply(px),
        }
    }

    /// mm du plan -> pixel de l'image (inverse de [`BackgroundMap::to_mm`]).
    pub fn to_px(&self, mm: PointMm) -> Option<PointMm> {
        match self {
            BackgroundMap::Similarity { origin, mm_per_px } => Some(PointMm::new(
                origin.x + mm.x / mm_per_px,
                origin.y - mm.y / mm_per_px,
            )),
            BackgroundMap::Projective(h) => h.inverse()?.apply(mm),
        }
    }
}

/// Transformation d'une calibration stockée ou demandée. Refus : points confondus, alignés, dans
/// un ordre qui ne forme pas un quadrilatère convexe anti-horaire, distance nulle ou non finie.
pub fn calibration_map(c: &Calibration) -> Result<BackgroundMap, CoreError> {
    match c {
        Calibration::TwoPoints {
            points_px,
            known_distance_mm,
            ..
        } => {
            let d = px_dist(points_px[0], points_px[1]);
            if !(d.is_finite() && d >= 1e-6) {
                return Err(invalid("calibration : les deux points sont confondus"));
            }
            if !(known_distance_mm.is_finite() && *known_distance_mm > 0.0) {
                return Err(invalid("calibration : la distance connue doit être > 0 mm"));
            }
            let s = known_distance_mm / d;
            if !(s.is_finite() && s > 0.0) {
                return Err(invalid("calibration : échelle non calculable"));
            }
            Ok(BackgroundMap::Similarity {
                origin: points_px[0],
                mm_per_px: s,
            })
        }
        Calibration::FourPoints {
            points_px,
            known_width_mm,
            known_height_mm,
            ..
        } => {
            for (v, what) in [(known_width_mm, "largeur"), (known_height_mm, "hauteur")] {
                if !(v.is_finite() && *v > 0.0) {
                    return Err(invalid(format!(
                        "calibration : la {what} connue doit être > 0 mm"
                    )));
                }
            }
            if !is_convex_ccw(points_px) {
                return Err(invalid(
                    "calibration : les quatre points doivent former un quadrilatère convexe, \
                     cliqués dans le sens anti-horaire (bas-gauche, bas-droite, haut-droite, haut-gauche)",
                ));
            }
            let src = points_px.map(|p| PointMm::new(p.x, p.y));
            let dst = rect_corners(*known_width_mm, *known_height_mm);
            let h = Homography::from_points(&src, &dst)
                .map_err(|e| invalid(format!("calibration : {e}")))?;
            Ok(BackgroundMap::Projective(h))
        }
    }
}

/// Résidus par point et RMS, recalculés depuis les points de la calibration (jamais lus du champ
/// `residual_mm`).
pub fn calibration_residuals(c: &Calibration) -> Result<Residuals, CoreError> {
    let map = calibration_map(c)?;
    let per_point_mm: Vec<f64> = match (c, map) {
        (
            Calibration::TwoPoints {
                points_px,
                known_distance_mm,
                ..
            },
            m,
        ) => {
            let a = m.to_mm(PointMm::new(points_px[0].x, points_px[0].y));
            let b = m.to_mm(PointMm::new(points_px[1].x, points_px[1].y));
            match (a, b) {
                (Some(a), Some(b)) => vec![
                    // Le premier point est l'origine : écart de position à (0 ; 0).
                    a.x.hypot(a.y),
                    // Le second doit être à la distance connue du premier.
                    (pf_geometry::distance_mm(a, b) - known_distance_mm).abs(),
                ],
                _ => return Err(invalid("calibration : transformation non finie")),
            }
        }
        (
            Calibration::FourPoints {
                points_px,
                known_width_mm,
                known_height_mm,
                ..
            },
            m,
        ) => {
            let targets = rect_corners(*known_width_mm, *known_height_mm);
            points_px
                .iter()
                .zip(targets)
                .map(|(p, t)| {
                    m.to_mm(PointMm::new(p.x, p.y))
                        .map(|q| pf_geometry::distance_mm(q, t))
                        .ok_or_else(|| invalid("calibration : transformation non finie"))
                })
                .collect::<Result<_, _>>()?
        }
    };
    let rms = (per_point_mm.iter().map(|e| e * e).sum::<f64>() / per_point_mm.len() as f64).sqrt();
    Ok(Residuals {
        per_point_mm,
        rms_mm: rms,
    })
}

impl Background {
    /// Transformation pixels ↔ mm de ce fond (recalculée à chaque appel).
    pub fn map(&self) -> Result<BackgroundMap, CoreError> {
        calibration_map(&self.calibration)
    }

    /// Résidus recalculés de la calibration.
    pub fn residuals(&self) -> Result<Residuals, CoreError> {
        calibration_residuals(&self.calibration)
    }

    /// Les quatre coins de l'image, en mm du plan (haut-gauche, haut-droite, bas-droite,
    /// bas-gauche de l'image).
    pub fn corners_mm(&self) -> Result<[PointMm; 4], CoreError> {
        let m = self.map()?;
        let (w, h) = (f64::from(self.width_px), f64::from(self.height_px));
        let at = |x: f64, y: f64| {
            m.to_mm(PointMm::new(x, y))
                .ok_or_else(|| invalid("image : coin rejeté à l'infini par la perspective"))
        };
        Ok([at(0.0, 0.0)?, at(w, 0.0)?, at(w, h)?, at(0.0, h)?])
    }
}

/// Construit la calibration stockée (résidu RMS compris) d'une demande, pour une image de
/// `width_px × height_px`. Refus : points hors de l'image, voir [`calibration_map`].
pub fn build_calibration(
    spec: &CalibrationSpec,
    width_px: u32,
    height_px: u32,
) -> Result<Calibration, CoreError> {
    let (w, h) = (f64::from(width_px), f64::from(height_px));
    let pts: Vec<PlanePoint> = match spec {
        CalibrationSpec::TwoPoints { points_px, .. } => points_px.to_vec(),
        CalibrationSpec::FourPoints { points_px, .. } => points_px.to_vec(),
    };
    for (i, p) in pts.iter().enumerate() {
        if !p.x.is_finite() || !p.y.is_finite() {
            return Err(invalid(format!(
                "calibration : le point {} n'est pas fini",
                i + 1
            )));
        }
        if p.x < 0.0 || p.y < 0.0 || p.x > w || p.y > h {
            return Err(invalid(format!(
                "calibration : le point {} est hors de l'image ({w} × {h} px)",
                i + 1
            )));
        }
        if pts[..i].iter().any(|q| q.x == p.x && q.y == p.y) {
            return Err(invalid(format!(
                "calibration : le point {} est confondu avec un autre",
                i + 1
            )));
        }
    }
    let mut c = match spec {
        CalibrationSpec::TwoPoints {
            points_px,
            known_distance_mm,
        } => Calibration::TwoPoints {
            points_px: *points_px,
            known_distance_mm: *known_distance_mm,
            residual_mm: 0.0,
        },
        CalibrationSpec::FourPoints {
            points_px,
            known_width_mm,
            known_height_mm,
        } => Calibration::FourPoints {
            points_px: *points_px,
            known_width_mm: *known_width_mm,
            known_height_mm: *known_height_mm,
            residual_mm: 0.0,
        },
    };
    let rms = calibration_residuals(&c)?.rms_mm;
    if !rms.is_finite() {
        return Err(invalid("calibration : résidu non fini"));
    }
    match &mut c {
        Calibration::TwoPoints { residual_mm, .. }
        | Calibration::FourPoints { residual_mm, .. } => *residual_mm = rms,
    }
    Ok(c)
}

fn check_image_ref(
    image: &str,
    sha256: &str,
    width_px: u32,
    height_px: u32,
) -> Result<(), CoreError> {
    if let Some(why) = unsafe_image_path(image) {
        return Err(invalid(format!("image de fond : chemin refusé ({why})")));
    }
    if sha256.len() != 64
        || !sha256
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(invalid(
            "image de fond : l'empreinte doit avoir 64 chiffres hexadécimaux minuscules",
        ));
    }
    for (v, name) in [(width_px, "largeur"), (height_px, "hauteur")] {
        if v == 0 || v > MAX_BACKGROUND_PX {
            return Err(invalid(format!(
                "image de fond : {name} hors de [1 ; {MAX_BACKGROUND_PX}] px"
            )));
        }
    }
    Ok(())
}

impl Project {
    fn background_of(&mut self, sheet: SheetId) -> Result<&mut Background, CoreError> {
        let name = self.sheet(sheet)?.name.clone();
        self.sheet_mut(sheet)?
            .background
            .as_mut()
            .ok_or_else(|| invalid(format!("la feuille « {name} » n'a pas d'image de fond")))
    }

    fn unlocked_background(&mut self, sheet: SheetId) -> Result<&mut Background, CoreError> {
        let b = self.background_of(sheet)?;
        if b.locked {
            return Err(invalid(
                "l'image de fond est verrouillée : déverrouillez-la d'abord",
            ));
        }
        Ok(b)
    }

    /// `SetBackground` : pose l'image de fond (déjà copiée à côté du projet) calibrée. Refus : feuille
    /// inconnue, fond déjà posé, référence ou calibration invalide.
    pub(crate) fn set_background(
        &mut self,
        sheet: SheetId,
        image: String,
        sha256: String,
        width_px: u32,
        height_px: u32,
        calibration: CalibrationSpec,
    ) -> Result<(), CoreError> {
        let s = self.sheet(sheet)?;
        if s.background.is_some() {
            return Err(invalid(format!(
                "la feuille « {} » a déjà une image de fond : retirez-la ou reliez-la",
                s.name
            )));
        }
        check_image_ref(&image, &sha256, width_px, height_px)?;
        let calibration = build_calibration(&calibration, width_px, height_px)?;
        self.sheet_mut(sheet)?.background = Some(Background {
            image,
            sha256,
            width_px,
            height_px,
            calibration,
            opacity: DEFAULT_BACKGROUND_OPACITY,
            visible: true,
            locked: false,
        });
        Ok(())
    }

    /// `CalibrateBackground` : remplace la calibration. Refus : pas de fond, fond verrouillé,
    /// calibration invalide.
    pub(crate) fn calibrate_background(
        &mut self,
        sheet: SheetId,
        calibration: CalibrationSpec,
    ) -> Result<(), CoreError> {
        let b = self.unlocked_background(sheet)?;
        let calibration = build_calibration(&calibration, b.width_px, b.height_px)?;
        self.background_of(sheet)?.calibration = calibration;
        Ok(())
    }

    /// `SetBackgroundDisplay` : opacité, visibilité, verrou. Jamais refusé par le verrou (c'est le
    /// moyen de le lever). Sans effet (aucune étape) si rien ne change.
    pub(crate) fn set_background_display(
        &mut self,
        sheet: SheetId,
        opacity: f64,
        visible: bool,
        locked: bool,
    ) -> Result<(), CoreError> {
        if !opacity.is_finite() || !(0.0..=1.0).contains(&opacity) {
            return Err(invalid(format!(
                "l'opacité doit être comprise entre 0 et 1 (reçu {opacity})"
            )));
        }
        let b = self.background_of(sheet)?;
        b.opacity = opacity;
        b.visible = visible;
        b.locked = locked;
        Ok(())
    }

    /// `RelinkBackground` : pointe le fond vers un autre fichier (ou le même, modifié) **de mêmes
    /// dimensions** ; la calibration est conservée telle quelle (ses points sont des pixels), le
    /// résidu reste celui de la calibration. Refus : pas de fond, fond verrouillé, référence
    /// invalide, dimensions différentes (recalibrez : retirez puis reposez le fond).
    pub(crate) fn relink_background(
        &mut self,
        sheet: SheetId,
        image: String,
        sha256: String,
        width_px: u32,
        height_px: u32,
    ) -> Result<(), CoreError> {
        let b = self.unlocked_background(sheet)?;
        check_image_ref(&image, &sha256, width_px, height_px)?;
        if (b.width_px, b.height_px) != (width_px, height_px) {
            return Err(invalid(format!(
                "la nouvelle image fait {width_px} × {height_px} px, la calibration porte sur \
                 {} × {} px : retirez le fond puis reposez-le pour recalibrer",
                b.width_px, b.height_px
            )));
        }
        b.image = image;
        b.sha256 = sha256;
        Ok(())
    }

    /// `RemoveBackground` : retire la référence (le fichier image n'est jamais supprimé). Refus :
    /// pas de fond, fond verrouillé.
    pub(crate) fn remove_background(&mut self, sheet: SheetId) -> Result<(), CoreError> {
        self.unlocked_background(sheet)?;
        self.sheet_mut(sheet)?.background = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pp(x: f64, y: f64) -> PlanePoint {
        PlanePoint { x, y }
    }

    #[test]
    fn deux_points_echelle_un_mm_par_pixel_et_residu_nul() {
        let c = build_calibration(
            &CalibrationSpec::TwoPoints {
                points_px: [pp(100.0, 100.0), pp(300.0, 100.0)],
                known_distance_mm: 200.0,
            },
            1000,
            800,
        )
        .unwrap();
        let Calibration::TwoPoints { residual_mm, .. } = &c else {
            panic!()
        };
        assert!(*residual_mm < 1e-9);
        let m = calibration_map(&c).unwrap();
        let q = m.to_mm(PointMm::new(300.0, 100.0)).unwrap();
        assert!((q.x - 200.0).abs() < 1e-9 && q.y.abs() < 1e-9);
        // y du plan vers le haut : 50 px plus bas dans l'image = -50 mm.
        let q = m.to_mm(PointMm::new(100.0, 150.0)).unwrap();
        assert!(q.x.abs() < 1e-9 && (q.y + 50.0).abs() < 1e-9);
        let back = m.to_px(q).unwrap();
        assert!((back.x - 100.0).abs() < 1e-9 && (back.y - 150.0).abs() < 1e-9);
    }

    #[test]
    fn quatre_points_rectangle_100_par_50_en_perspective() {
        let c = build_calibration(
            &CalibrationSpec::FourPoints {
                // bas-gauche, bas-droite, haut-droite, haut-gauche (y image vers le bas).
                points_px: [
                    pp(60.0, 300.0),
                    pp(480.0, 360.0),
                    pp(520.0, 120.0),
                    pp(100.0, 80.0),
                ],
                known_width_mm: 100.0,
                known_height_mm: 50.0,
            },
            600,
            400,
        )
        .unwrap();
        let r = calibration_residuals(&c).unwrap();
        assert_eq!(r.per_point_mm.len(), 4);
        assert!(r.rms_mm < 1e-6 && r.per_point_mm.iter().all(|e| *e < 1e-6));
        let m = calibration_map(&c).unwrap();
        let q = m.to_mm(PointMm::new(520.0, 120.0)).unwrap();
        assert!((q.x - 100.0).abs() < 1e-6 && (q.y - 50.0).abs() < 1e-6);
    }

    #[test]
    fn refus_calibrations_degenerees() {
        let two = |a: PlanePoint, b: PlanePoint, d: f64| {
            build_calibration(
                &CalibrationSpec::TwoPoints {
                    points_px: [a, b],
                    known_distance_mm: d,
                },
                1000,
                800,
            )
        };
        assert!(two(pp(1.0, 1.0), pp(1.0, 1.0), 10.0).is_err());
        assert!(two(pp(1.0, 1.0), pp(5.0, 1.0), 0.0).is_err());
        assert!(two(pp(1.0, 1.0), pp(5.0, 1.0), f64::NAN).is_err());
        assert!(two(pp(1.0, 1.0), pp(5.0, 1.0), f64::INFINITY).is_err());
        assert!(two(pp(1.0, 1.0), pp(5000.0, 1.0), 10.0).is_err()); // hors de l'image
        assert!(two(pp(1.0, 1.0), pp(5.0, 1.0), 10.0).is_ok());
        let four = |p: [PlanePoint; 4]| {
            build_calibration(
                &CalibrationSpec::FourPoints {
                    points_px: p,
                    known_width_mm: 100.0,
                    known_height_mm: 50.0,
                },
                1000,
                800,
            )
        };
        // alignés
        assert!(four([pp(0.0, 0.0), pp(10.0, 0.0), pp(20.0, 0.0), pp(30.0, 0.0)]).is_err());
        // sens horaire (miroir)
        assert!(four([
            pp(100.0, 500.0),
            pp(100.0, 100.0),
            pp(500.0, 100.0),
            pp(500.0, 500.0)
        ])
        .is_err());
        // en croix (non convexe)
        assert!(four([
            pp(100.0, 500.0),
            pp(500.0, 100.0),
            pp(500.0, 500.0),
            pp(100.0, 100.0)
        ])
        .is_err());
        // correct
        assert!(four([
            pp(100.0, 500.0),
            pp(500.0, 500.0),
            pp(500.0, 100.0),
            pp(100.0, 100.0)
        ])
        .is_ok());
    }
}
