//! US-08.3 — angle intérieur d'une pièce à un sommet de son contour (raccord de couture).
//! Maths pures en mm / degrés.

use crate::{ccw_angle_deg, VectorMm};

/// Un angle à moins de cette valeur (degrés) de 0° ou de 360° est une pointe (cusp).
pub const CUSP_TOLERANCE_DEG: f64 = 1e-9;

/// Angle **intérieur** de la pièce en un sommet, en degrés dans `]0 ; 360[` (> 180 = sommet
/// rentrant). `incoming` est la tangente de fin de l'arête qui arrive au sommet, `outgoing` la
/// tangente de début de l'arête qui en part, toutes deux dans le sens de parcours du contour ;
/// `clockwise` dit si le contour tourne dans le sens horaire. `None` si une tangente est
/// indéfinie (nulle, non finie) ou si les deux arêtes se replient l'une sur l'autre (pointe).
pub fn vertex_interior_angle_deg(
    incoming: VectorMm,
    outgoing: VectorMm,
    clockwise: bool,
) -> Option<f64> {
    let back = VectorMm::new(-incoming.x, -incoming.y);
    let deg = if clockwise {
        ccw_angle_deg(back, outgoing)
    } else {
        ccw_angle_deg(outgoing, back)
    }
    .ok()?;
    if deg <= CUSP_TOLERANCE_DEG || deg >= 360.0 - CUSP_TOLERANCE_DEG {
        return None;
    }
    Some(deg)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(x: f64, y: f64) -> VectorMm {
        VectorMm::new(x, y)
    }

    #[test]
    fn carre_anti_horaire_90_degres() {
        // (0,0) -> (1,0) -> (1,1) : arrivée (1,0), départ (0,1).
        let a = vertex_interior_angle_deg(v(1.0, 0.0), v(0.0, 1.0), false).unwrap();
        assert!((a - 90.0).abs() < 1e-12);
    }

    #[test]
    fn carre_horaire_90_degres() {
        // (0,0) -> (0,1) -> (1,1) : arrivée (0,1), départ (1,0), contour horaire.
        let a = vertex_interior_angle_deg(v(0.0, 1.0), v(1.0, 0.0), true).unwrap();
        assert!((a - 90.0).abs() < 1e-12);
    }

    #[test]
    fn sommet_rentrant_270_degres() {
        // Virage à droite sur un contour anti-horaire : l'intérieur fait 270°.
        let a = vertex_interior_angle_deg(v(1.0, 0.0), v(0.0, -1.0), false).unwrap();
        assert!((a - 270.0).abs() < 1e-12);
    }

    #[test]
    fn alignes_180_et_pointe_ou_nul_refuses() {
        let a = vertex_interior_angle_deg(v(1.0, 0.0), v(1.0, 0.0), false).unwrap();
        assert!((a - 180.0).abs() < 1e-12);
        assert!(vertex_interior_angle_deg(v(1.0, 0.0), v(-1.0, 0.0), false).is_none());
        assert!(vertex_interior_angle_deg(v(0.0, 0.0), v(1.0, 0.0), false).is_none());
        assert!(vertex_interior_angle_deg(v(f64::NAN, 0.0), v(1.0, 0.0), true).is_none());
    }
}
