//! US-03.2b — rectangle créé avec ses constructions mémorisées, optionnellement **maintenu**.
//!
//! Aucun type ni champ nouveau (schéma 5 inchangé) : le rectangle est fait de quatre segments
//! ordinaires dont deux portent une construction d'US-03.2a. Le côté de départ `p0 → p1` est un
//! segment longueur / angle (`Construction::Polar`) ; le côté opposé `p3 → p2` est une parallèle
//! à ce côté, à la distance de la hauteur (`Construction::Parallel`) ; les deux côtés restants sont
//! ordinaires. Avec `kept`, déplacer le coin `p0` déplace `p1`, puis `p3` et `p2` (même étape,
//! mêmes règles de déliage que les constructions maintenues).
//!
//! Les positions sont celles d'`AddRectangle` (`p1 = p0 + (w ; 0)`, `p2 = p0 + (w ; h)`,
//! `p3 = p0 + (0 ; h)`) ; `width_mm` et `height_mm` sont signés, jamais nuls.

use super::*;

impl Project {
    pub(super) fn add_constructed_rectangle(
        &mut self,
        sheet: SheetId,
        corner: PointRef,
        (width_mm, height_mm): (f64, f64),
        kept: bool,
        out: &mut Outcome,
    ) -> Result<(), CoreError> {
        for v in [width_mm, height_mm] {
            if !v.is_finite() || v.abs() <= EPSILON_MM {
                return Err(pf_geometry::GeometryError::InvalidLength(v).into());
            }
        }
        let p0_id = self.resolve_ref(sheet, corner, out)?;
        let p0 = self.sheet(sheet)?.position(p0_id)?;
        let angle_deg = if width_mm > 0.0 { 0.0 } else { 180.0 };
        let p1_pos = pf_geometry::point_from_polar(p0, width_mm.abs(), angle_deg)?;
        let normal = (p1_pos - p0).unit()?.rotated_left();
        // Gauche du côté de départ = +Y quand la largeur est positive, −Y sinon.
        let side = if (height_mm > 0.0) == (width_mm > 0.0) {
            Side::Left
        } else {
            Side::Right
        };
        let k = match side {
            Side::Left => height_mm.abs(),
            Side::Right => -height_mm.abs(),
        };
        let p3_pos = (p0 + normal * k).validate()?;
        let p2_pos = (p1_pos + normal * k).validate()?;
        let p1 = self.resolve_ref(sheet, PointRef::New(p1_pos), out)?;
        let p3 = self.resolve_ref(sheet, PointRef::New(p3_pos), out)?;
        let p2 = self.resolve_ref(sheet, PointRef::New(p2_pos), out)?;
        let base = Construction::Polar {
            length_mm: width_mm.abs(),
            angle_deg,
            kept,
        };
        self.add_segment(sheet, p0_id, p1, Some(base), out)?;
        let reference = *out.entities.last().ok_or(CoreError::Invalid(
            "rectangle : côté de départ introuvable".into(),
        ))?;
        self.add_segment(sheet, p1, p2, None, out)?;
        let opposite = Construction::Parallel {
            reference,
            distance_mm: height_mm.abs(),
            side,
            kept,
        };
        self.add_segment(sheet, p3, p2, Some(opposite), out)?;
        self.add_segment(sheet, p3, p0_id, None, out)?;
        Ok(())
    }
}
