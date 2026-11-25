//! Lignes d'aide (US-02.8) : droites infinies de la feuille, accrochables,
//! jamais exportées par défaut. Un guide n'est pas un tracé : il n'a ni point
//! ni rôle, et aucun point construit ne le référence (un point posé sur un
//! guide est un point libre à la position exacte — décision 68).

use crate::error::CoreError;
use crate::model::{EntityId, Guide, GuideId, Sheet};
use pf_geometry::{Bbox, PointMm, VectorMm, MAX_COORD_MM};

/// Deux guides dont les directions diffèrent de moins que cela (degrés) sont
/// parallèles : pas d'intersection.
const PARALLEL_DEG: f64 = 1e-9;

/// Angle d'une droite ramené dans `[0 ; 180[` (une droite n'a pas de sens).
/// `rem_euclid` peut rendre exactement 180 pour un petit angle négatif.
pub fn normalize_angle_deg(angle_deg: f64) -> f64 {
    let r = angle_deg.rem_euclid(180.0);
    if r >= 180.0 {
        0.0
    } else {
        r + 0.0
    }
}

impl Guide {
    /// Un point par lequel passe la droite.
    pub fn origin(&self) -> PointMm {
        PointMm::new(self.x_mm, self.y_mm)
    }

    /// Vecteur directeur unitaire.
    pub fn direction(&self) -> VectorMm {
        let (s, c) = pf_geometry::sin_cos_deg(self.angle_deg);
        VectorMm::new(c, s)
    }

    /// Valeurs finies et dans l'espace de travail.
    pub fn validate(&self) -> Result<(), CoreError> {
        if !self.angle_deg.is_finite() {
            return Err(CoreError::Invalid(
                "ligne d'aide : l'angle doit être un nombre fini".into(),
            ));
        }
        PointMm::new(self.x_mm, self.y_mm).validate()?;
        Ok(())
    }

    /// Distance (mm) d'un point à la droite infinie.
    pub fn distance_to(&self, p: PointMm) -> f64 {
        self.direction().cross(p - self.origin()).abs()
    }

    /// Pied de la perpendiculaire abaissée de `p` sur la droite.
    pub fn project(&self, p: PointMm) -> PointMm {
        let u = self.direction();
        self.origin() + u * u.dot(p - self.origin())
    }

    /// Partie de la droite contenue dans `bb` (extrémités comprises) ; `None`
    /// si elle ne la traverse pas.
    pub fn clip_to(&self, bb: &Bbox) -> Option<(PointMm, PointMm)> {
        let (o, d) = (self.origin(), self.direction());
        let (mut t0, mut t1) = (f64::NEG_INFINITY, f64::INFINITY);
        for (p, q, lo, hi) in [
            (o.x, d.x, bb.min.x, bb.max.x),
            (o.y, d.y, bb.min.y, bb.max.y),
        ] {
            if q.abs() < 1e-12 {
                if p < lo || p > hi {
                    return None;
                }
            } else {
                let (a, b) = ((lo - p) / q, (hi - p) / q);
                t0 = t0.max(a.min(b));
                t1 = t1.min(a.max(b));
            }
        }
        (t0 < t1).then(|| (o + d * t0, o + d * t1))
    }

    /// Segment qui représente la droite dans l'espace de travail (pour les
    /// intersections avec les tracés bornés).
    fn workspace_segment(&self) -> Option<(PointMm, PointMm)> {
        let m = MAX_COORD_MM;
        let bb = Bbox {
            min: PointMm::new(-m, -m),
            max: PointMm::new(m, m),
        };
        let clamp = |p: PointMm| PointMm::new(p.x.clamp(-m, m), p.y.clamp(-m, m));
        self.clip_to(&bb).map(|(a, b)| (clamp(a), clamp(b)))
    }

    /// Intersection avec un autre guide ; `None` s'ils sont parallèles.
    pub fn intersection(&self, other: &Guide) -> Option<PointMm> {
        let (u, v) = (self.direction(), other.direction());
        if u.cross(v).abs() <= PARALLEL_DEG.to_radians() {
            return None;
        }
        let t = (other.origin() - self.origin()).cross(v) / u.cross(v);
        (self.origin() + u * t).validate().ok()
    }
}

impl Sheet {
    pub fn guide(&self, id: GuideId) -> Result<&Guide, CoreError> {
        self.guides
            .iter()
            .find(|g| g.id == id)
            .ok_or(CoreError::UnknownGuide(id))
    }

    /// Un guide suit la visibilité de son calque (le masquage global est un
    /// réglage d'affichage, hors du modèle).
    pub fn guide_visible(&self, g: &Guide) -> bool {
        self.layer_visible(g.layer)
    }

    pub fn guide_locked(&self, g: &Guide) -> bool {
        self.layer_locked(g.layer)
    }

    /// Points où la droite coupe un tracé (bornés) ; vide si elle ne le coupe
    /// pas, est superposée à lui, ou si le tracé est dégénéré.
    pub fn guide_trace_hits(&self, guide: &Guide, entity: EntityId) -> Vec<PointMm> {
        let (Some((a, b)), Ok(shape)) = (guide.workspace_segment(), self.shape(entity)) else {
            return Vec::new();
        };
        pf_geometry::intersect(&pf_geometry::Trace::Line(a, b), &shape.to_trace())
            .map(|hits| hits.into_iter().map(|h| h.point).collect())
            .unwrap_or_default()
    }
}
