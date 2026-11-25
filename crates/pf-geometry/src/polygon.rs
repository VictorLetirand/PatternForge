//! Polygone libre (lasso de sélection, US-16.1) : appartenance d'un point et
//! d'une polyligne échantillonnée.
//!
//! Règle de remplissage **pair-impair** : un point est dedans si un rayon qui
//! en part coupe le contour un nombre impair de fois. Un polygone qui se
//! recoupe a donc des zones « trouées » (une boucle en huit a deux lobes
//! pleins). Un point exactement sur un côté est dedans ; au-delà des cas
//! exacts (côtés horizontaux, verticaux, sommets), l'arrondi des flottants
//! décide pour un point à quelques ulp d'un côté oblique.
//!
//! Aucun calcul ne panique : toute valeur non finie donne « dehors ».

use crate::{Bbox, PointMm, EPSILON_MM};

fn is_finite(p: PointMm) -> bool {
    p.x.is_finite() && p.y.is_finite()
}

/// Aire signée doublée du triangle (a, b, c) : > 0 si c est à gauche de a→b.
fn orient(a: PointMm, b: PointMm, c: PointMm) -> f64 {
    (b - a).cross(c - a)
}

/// `p` est-il exactement sur le segment [a, b] ?
fn on_segment(p: PointMm, a: PointMm, b: PointMm) -> bool {
    orient(a, b, p) == 0.0
        && p.x >= a.x.min(b.x)
        && p.x <= a.x.max(b.x)
        && p.y >= a.y.min(b.y)
        && p.y <= a.y.max(b.y)
}

/// Les segments [a, b] et [c, d] se **traversent** (croisement strict : les
/// extrémités de chacun sont de part et d'autre de l'autre).
fn segments_cross(a: PointMm, b: PointMm, c: PointMm, d: PointMm) -> bool {
    let (o1, o2) = (orient(a, b, c), orient(a, b, d));
    let (o3, o4) = (orient(c, d, a), orient(c, d, b));
    ((o1 > 0.0 && o2 < 0.0) || (o1 < 0.0 && o2 > 0.0))
        && ((o3 > 0.0 && o4 < 0.0) || (o3 < 0.0 && o4 > 0.0))
}

/// Les segments se croisent ou se touchent (extrémité sur l'autre segment comprise).
fn segments_touch(a: PointMm, b: PointMm, c: PointMm, d: PointMm) -> bool {
    segments_cross(a, b, c, d)
        || on_segment(c, a, b)
        || on_segment(d, a, b)
        || on_segment(a, c, d)
        || on_segment(b, c, d)
}

/// `p` est-il dans le polygone (règle pair-impair, bord compris) ? Un polygone
/// de moins de 3 sommets ou contenant une valeur non finie ne contient rien.
/// Ne vérifie pas la dégénérescence (voir [`Lasso::new`]).
pub fn point_in_polygon(p: PointMm, polygon: &[PointMm]) -> bool {
    let n = polygon.len();
    if n < 3 || !is_finite(p) || polygon.iter().any(|v| !is_finite(*v)) {
        return false;
    }
    let mut inside = false;
    for i in 0..n {
        let (a, b) = (polygon[i], polygon[(i + 1) % n]);
        if on_segment(p, a, b) {
            return true;
        }
        // Le côté enjambe-t-il l'horizontale de p, et à droite de p ?
        if (a.y > p.y) != (b.y > p.y) {
            let x = a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x);
            if p.x < x {
                inside = !inside;
            }
        }
    }
    inside
}

/// Le polygone fermé est-il **simple** (US-06.10) : au moins 3 sommets finis, aucun sommet
/// consécutif en double, aucun côté qui en croise ou touche un autre (hors sommet commun de deux
/// côtés voisins), aucun retour en arrière sur un même côté (pointe d'épaisseur nulle).
pub fn is_simple_polygon(polygon: &[PointMm]) -> bool {
    let n = polygon.len();
    if n < 3 || polygon.iter().any(|v| !is_finite(*v)) {
        return false;
    }
    for i in 0..n {
        let (a, b, c) = (polygon[i], polygon[(i + 1) % n], polygon[(i + 2) % n]);
        if a == b {
            return false;
        }
        // Pointe : c revient sur le côté a-b.
        if orient(a, b, c) == 0.0 && (b - a).dot(c - b) < 0.0 {
            return false;
        }
    }
    for i in 0..n {
        let (a, b) = (polygon[i], polygon[(i + 1) % n]);
        for j in (i + 1)..n {
            if j == i + 1 || (i == 0 && j == n - 1) {
                continue;
            }
            let (c, d) = (polygon[j], polygon[(j + 1) % n]);
            if segments_touch(a, b, c, d) {
                return false;
            }
        }
    }
    true
}

/// Polygone de sélection validé : au moins 3 sommets distincts, finis, non
/// tous alignés. Les sommets consécutifs en double et le sommet de fermeture
/// répété sont retirés.
#[derive(Debug, Clone, PartialEq)]
pub struct Lasso {
    vertices: Vec<PointMm>,
    bbox: Bbox,
}

impl Lasso {
    /// `None` si le polygone est dégénéré : moins de 3 sommets distincts, valeur
    /// non finie, ou sommets tous alignés (aire nulle).
    pub fn new(polygon: &[PointMm]) -> Option<Lasso> {
        if polygon.iter().any(|v| !is_finite(*v)) {
            return None;
        }
        let mut vertices: Vec<PointMm> = Vec::with_capacity(polygon.len());
        for &v in polygon {
            if vertices.last() != Some(&v) {
                vertices.push(v);
            }
        }
        while vertices.len() > 1 && vertices.first() == vertices.last() {
            vertices.pop();
        }
        if vertices.len() < 3 {
            return None;
        }
        // Non aligné : un sommet est à plus de EPSILON_MM de la droite (v0, v1).
        let (v0, v1) = (vertices[0], vertices[1]);
        let d = v1 - v0;
        let reach = EPSILON_MM * d.norm();
        let spread = vertices.iter().any(|w| d.cross(*w - v0).abs() > reach);
        if !spread {
            return None;
        }
        let mut bbox = Bbox::from_point(v0);
        for v in &vertices {
            bbox.include(*v);
        }
        Some(Lasso { vertices, bbox })
    }

    pub fn vertices(&self) -> &[PointMm] {
        &self.vertices
    }

    /// Point dedans (bord compris).
    pub fn contains_point(&self, p: PointMm) -> bool {
        self.bbox.contains(p) && point_in_polygon(p, &self.vertices)
    }

    fn edges(&self) -> impl Iterator<Item = (PointMm, PointMm)> + '_ {
        let n = self.vertices.len();
        (0..n).map(move |i| (self.vertices[i], self.vertices[(i + 1) % n]))
    }

    /// Polyligne entièrement dedans : tous ses points sont dedans **et** aucun
    /// de ses pas ne traverse le contour (un tracé qui sort puis rentre entre
    /// deux échantillons est donc refusé). Vide ou non finie : `false`.
    pub fn contains_polyline(&self, points: &[PointMm]) -> bool {
        if points.is_empty() || !points.iter().all(|p| self.contains_point(*p)) {
            return false;
        }
        points.windows(2).all(|w| {
            let (a, b) = (w[0], w[1]);
            !self.edges().any(|(c, d)| segments_cross(a, b, c, d))
        })
    }

    /// Polyligne touchée : un point dedans, ou un pas qui croise ou touche le
    /// contour (mode « touche » du lasso).
    pub fn touches_polyline(&self, points: &[PointMm]) -> bool {
        if points.iter().any(|p| self.contains_point(*p)) {
            return true;
        }
        points.windows(2).any(|w| {
            let (a, b) = (w[0], w[1]);
            is_finite(a) && is_finite(b) && self.edges().any(|(c, d)| segments_touch(a, b, c, d))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: f64, y: f64) -> PointMm {
        PointMm::new(x, y)
    }

    /// Carré [0, 10] x [0, 10].
    fn square() -> Vec<PointMm> {
        vec![p(0.0, 0.0), p(10.0, 0.0), p(10.0, 10.0), p(0.0, 10.0)]
    }

    /// Fer à cheval en U (concave) : bras [0,3] et [7,10], fond [0,10] x [0,3], haut à y = 10.
    fn u_shape() -> Vec<PointMm> {
        vec![
            p(0.0, 0.0),
            p(10.0, 0.0),
            p(10.0, 10.0),
            p(7.0, 10.0),
            p(7.0, 3.0),
            p(3.0, 3.0),
            p(3.0, 10.0),
            p(0.0, 10.0),
        ]
    }

    #[test]
    fn convexe_dedans_dehors_et_bord() {
        let s = square();
        assert!(point_in_polygon(p(5.0, 5.0), &s));
        assert!(point_in_polygon(p(0.1, 9.9), &s));
        assert!(!point_in_polygon(p(-0.1, 5.0), &s));
        assert!(!point_in_polygon(p(5.0, 10.1), &s));
        assert!(!point_in_polygon(p(20.0, 20.0), &s));
        // Bord exact (côtés et sommets) : dedans.
        assert!(point_in_polygon(p(0.0, 5.0), &s));
        assert!(point_in_polygon(p(10.0, 10.0), &s));
        assert!(point_in_polygon(p(5.0, 0.0), &s));
    }

    #[test]
    fn concave_la_cavite_est_dehors() {
        let u = u_shape();
        assert!(point_in_polygon(p(1.5, 8.0), &u), "bras gauche");
        assert!(point_in_polygon(p(8.5, 8.0), &u), "bras droit");
        assert!(point_in_polygon(p(5.0, 1.5), &u), "fond");
        assert!(!point_in_polygon(p(5.0, 8.0), &u), "cavité entre les bras");
        assert!(
            !point_in_polygon(p(5.0, 3.5), &u),
            "juste au-dessus du fond"
        );
    }

    #[test]
    fn sens_horaire_ou_antihoraire_equivalents() {
        let mut s = square();
        s.reverse();
        assert!(point_in_polygon(p(5.0, 5.0), &s));
        assert!(!point_in_polygon(p(15.0, 5.0), &s));
    }

    #[test]
    fn auto_intersection_pair_impair() {
        // Papillon : (0,0) (10,10) (10,0) (0,10) — se recoupe en (5,5).
        // Lobes pleins à gauche et à droite, triangles haut et bas vides.
        let bow = vec![p(0.0, 0.0), p(10.0, 10.0), p(10.0, 0.0), p(0.0, 10.0)];
        assert!(point_in_polygon(p(8.0, 5.0), &bow), "lobe droit");
        assert!(point_in_polygon(p(2.0, 5.0), &bow), "lobe gauche");
        assert!(!point_in_polygon(p(5.0, 8.0), &bow), "triangle du haut");
        assert!(!point_in_polygon(p(5.0, 2.0), &bow), "triangle du bas");
        // Pentagramme : le pentagone central est parcouru deux fois (nombre
        // d'enroulements 2, pair) donc dehors ; les pointes ne le sont qu'une fois.
        let star: Vec<PointMm> = (0..5)
            .map(|k| {
                let a = (90.0 + 144.0 * f64::from(k)).to_radians();
                p(10.0 * a.cos(), 10.0 * a.sin())
            })
            .collect();
        assert!(!point_in_polygon(p(0.0, 0.0), &star), "centre : dehors");
        assert!(point_in_polygon(p(0.0, 8.0), &star), "pointe : dedans");
    }

    #[test]
    fn sommets_en_double_et_fermeture_repetee() {
        let mut s = square();
        s.insert(1, p(10.0, 0.0)); // doublon consécutif
        s.push(p(0.0, 0.0)); // fermeture répétée
        s.push(p(0.0, 0.0));
        let l = Lasso::new(&s).expect("carré valide malgré les doublons");
        assert_eq!(l.vertices().len(), 4);
        assert!(l.contains_point(p(5.0, 5.0)));
        assert!(!l.contains_point(p(11.0, 5.0)));
        // Doublons seuls : pas de polygone.
        assert!(Lasso::new(&[p(1.0, 1.0), p(1.0, 1.0), p(1.0, 1.0)]).is_none());
    }

    #[test]
    fn polygone_degenere_refuse() {
        assert!(Lasso::new(&[]).is_none());
        assert!(Lasso::new(&[p(0.0, 0.0)]).is_none());
        assert!(Lasso::new(&[p(0.0, 0.0), p(5.0, 5.0)]).is_none());
        // Trois points alignés : aire nulle.
        assert!(Lasso::new(&[p(0.0, 0.0), p(5.0, 5.0), p(10.0, 10.0)]).is_none());
        // Aller-retour sur une droite.
        assert!(Lasso::new(&[p(0.0, 0.0), p(10.0, 0.0), p(5.0, 0.0), p(0.0, 0.0)]).is_none());
        // Un vrai triangle fin reste valide.
        assert!(Lasso::new(&[p(0.0, 0.0), p(10.0, 0.0), p(5.0, 0.001)]).is_some());
        // point_in_polygon seul : moins de 3 sommets ne contient rien.
        assert!(!point_in_polygon(p(0.0, 0.0), &[p(0.0, 0.0), p(1.0, 1.0)]));
    }

    #[test]
    fn valeurs_extremes_et_non_finies_sans_panique() {
        let nan = f64::NAN;
        let inf = f64::INFINITY;
        assert!(Lasso::new(&[p(0.0, 0.0), p(nan, 1.0), p(5.0, 5.0)]).is_none());
        assert!(Lasso::new(&[p(0.0, 0.0), p(inf, 1.0), p(5.0, 5.0)]).is_none());
        assert!(!point_in_polygon(p(nan, 5.0), &square()));
        assert!(!point_in_polygon(p(5.0, inf), &square()));
        let bad = vec![p(0.0, 0.0), p(10.0, nan), p(10.0, 10.0)];
        assert!(!point_in_polygon(p(5.0, 5.0), &bad));
        let l = Lasso::new(&square()).unwrap();
        assert!(!l.contains_point(p(nan, nan)));
        assert!(!l.contains_polyline(&[p(5.0, 5.0), p(nan, 5.0)]));
        assert!(!l.touches_polyline(&[p(nan, 5.0), p(5.0, inf)]));
        // Très grandes valeurs : pas de panique, résultat cohérent.
        let big = 1.0e300;
        let huge = vec![p(-big, -big), p(big, -big), p(big, big), p(-big, big)];
        let _ = point_in_polygon(p(0.0, 0.0), &huge);
        let _ = Lasso::new(&huge).map(|l| l.contains_point(p(1.0, 1.0)));
        // Coordonnées au maximum de l'espace de travail.
        let m = crate::MAX_COORD_MM;
        let wide = vec![p(-m, -m), p(m, -m), p(m, m), p(-m, m)];
        let l = Lasso::new(&wide).unwrap();
        assert!(l.contains_point(p(0.0, 0.0)));
        assert!(!l.contains_point(p(2.0 * m, 0.0)));
    }

    #[test]
    fn polyligne_dedans_et_sortie_par_intersection_d_arete() {
        let u = Lasso::new(&u_shape()).unwrap();
        // Dans le bras gauche : dedans.
        assert!(u.contains_polyline(&[p(1.0, 1.0), p(1.0, 9.0)]));
        // Les deux extrémités dans les bras, mais le segment traverse la cavité.
        let across = [p(1.5, 8.0), p(8.5, 8.0)];
        assert!(u.contains_point(across[0]) && u.contains_point(across[1]));
        assert!(!u.contains_polyline(&across), "sortie par arête");
        // Passer par le fond reste dedans.
        assert!(u.contains_polyline(&[p(1.5, 8.0), p(1.5, 1.5), p(8.5, 1.5), p(8.5, 8.0)]));
        // Une extrémité dehors.
        assert!(!u.contains_polyline(&[p(1.0, 1.0), p(20.0, 1.0)]));
        assert!(!u.contains_polyline(&[]));
        // Point seul dedans.
        assert!(u.contains_polyline(&[p(1.0, 1.0)]));
    }

    #[test]
    fn mode_touche() {
        let l = Lasso::new(&square()).unwrap();
        // Traverse le carré de part en part, extrémités dehors.
        let through = [p(-5.0, 5.0), p(15.0, 5.0)];
        assert!(!l.contains_polyline(&through));
        assert!(l.touches_polyline(&through));
        // Entièrement dehors.
        assert!(!l.touches_polyline(&[p(-5.0, 20.0), p(15.0, 20.0)]));
        // Effleure exactement le bord.
        assert!(l.touches_polyline(&[p(-5.0, 10.0), p(15.0, 10.0)]));
        // Dedans.
        assert!(l.touches_polyline(&[p(2.0, 2.0), p(3.0, 3.0)]));
        assert!(!l.touches_polyline(&[]));
    }

    #[test]
    fn croisement_strict_et_contact() {
        // Croix : se traversent.
        assert!(segments_cross(
            p(0.0, 0.0),
            p(10.0, 10.0),
            p(0.0, 10.0),
            p(10.0, 0.0)
        ));
        // Contact en T : pas une traversée stricte, mais un contact.
        let (a, b, c, d) = (p(0.0, 0.0), p(10.0, 0.0), p(5.0, 0.0), p(5.0, 5.0));
        assert!(!segments_cross(a, b, c, d));
        assert!(segments_touch(a, b, c, d));
        // Parallèles disjoints.
        assert!(!segments_touch(
            p(0.0, 0.0),
            p(10.0, 0.0),
            p(0.0, 1.0),
            p(10.0, 1.0)
        ));
        // Colinéaires qui se recouvrent : contact.
        assert!(segments_touch(
            p(0.0, 0.0),
            p(10.0, 0.0),
            p(5.0, 0.0),
            p(15.0, 0.0)
        ));
        // Colinéaires disjoints.
        assert!(!segments_touch(
            p(0.0, 0.0),
            p(1.0, 0.0),
            p(5.0, 0.0),
            p(15.0, 0.0)
        ));
    }
}
