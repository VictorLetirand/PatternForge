//! Contour de coupe : décalage du contour de couture par une marge par bord (US-05.1).
//!
//! `seam_contour(chain, margins)` reçoit le contour de **couture** (chaîne **fermée**,
//! orientée **anti-horaire**, en arêtes élémentaires) et une marge `(début, fin)` par arête.
//! Le contour de coupe est décalé vers l'**extérieur** (à droite du sens de parcours) ; il est
//! calculé à la demande, jamais stocké.
//!
//! # Méthode
//!
//! - **Marge uniforme** (même valeur partout, début = fin) : `offset_chain` fait tout le travail
//!   (arcs, Béziers, coins extérieurs en onglet, coins rentrants coupés à leur intersection,
//!   parties retournées éliminées). Marge nulle partout : copie exacte du contour de couture.
//! - **Marges différentes** : chaque arête est décalée seule. Un segment relie ses deux marges
//!   (la droite décalée est inclinée si début ≠ fin) ; un arc, une Bézier ou une courbe lisse
//!   ont une marge **constante** (sinon refus : « marge progressive : segments seulement »),
//!   décalée par la formule de l'arc (rayon ± marge) ou par `offset_chain` sur l'arête seule.
//!   Aux sommets : coin **extérieur** (virage à gauche) = prolongement des deux décalages
//!   jusqu'à leur intersection (onglet), repli en **biseau** si la pointe dépasse
//!   [`SEAM_MITER_LIMIT`] × la plus grande marge adjacente ; coin **rentrant** = les deux
//!   décalages sont coupés à leur intersection ; deux marges différentes à un sommet tangent :
//!   un petit segment les relie (une « marche »).
//! - Le résultat est contrôlé : continu, fermé, fini, sans auto-intersection. Sinon erreur
//!   expliquée (jamais un contour faux).
//!
//! # Types de coins (US-05.2)
//!
//! `seam_contour_with` accepte un [`CornerKind`] par **sommet** (sommet `j` = début de l'arête `j`,
//! entre l'arête `j − 1` et l'arête `j`) et une longueur maximale de pointe. Un type de coin ne
//! s'applique qu'à un coin **extérieur** (virage à gauche) ; sur un sommet lisse ou rentrant, le
//! calcul échoue avec une explication. Notations : `V` sommet, `t1` / `t2` directions de couture
//! avant / après, `n1` / `n2` normales extérieures (à droite), `a` / `b` marges avant / après,
//! `E = V + a·n1` bout du décalage précédent `L1`, `S = V + b·n2` début du décalage suivant `L2`.
//!
//! - `Mitre` : prolongement de `L1` et `L2` jusqu'à leur intersection ; biseau `E`–`S` si la pointe
//!   est à plus de `max_tip_mm` de `V` (défaut : 4 × la plus grande marge adjacente).
//! - `PerpendicularPrevious` : `L1` est prolongée jusqu'au pied de la perpendiculaire à la
//!   couture précédente passant par `S`, puis on rejoint `S` par cette perpendiculaire.
//! - `PerpendicularNext` : `L2` est prolongée en arrière jusqu'au pied de la perpendiculaire à la
//!   couture suivante passant par `E` ; on rejoint `E` par cette perpendiculaire.
//! - `Symmetric` (retour de couture) : coupe perpendiculaire à la bissectrice, passant par celui de
//!   `E` et `S` le plus proche de la pointe (à marges égales : le segment `E`–`S`).
//! - `Truncated` : pointe de l'onglet coupée par une perpendiculaire à la bissectrice, à
//!   `depth_mm` de la pointe (0 = onglet pur, sans limite de pointe).
//! - `Stepped` (en marche) : `E` → `M` → `S` avec `M = V + a·n1 + b·n2` (chaque palier est
//!   perpendiculaire à une couture).
//! - `HemFoldNext` / `HemFoldPrevious` (ourlet rabattu) : la couture suivante (resp. précédente)
//!   est la ligne de pli ; la marge de l'autre bord est le **miroir** de sa droite par rapport à
//!   la ligne de pli, de la ligne de pli jusqu'à la marge de l'ourlet.
//!
//! Limites : coins arrondis, ourlets complets… (US-05.3) ne sont pas traités ;
//! un coin rentrant dont une arête est trop courte pour la marge échoue (message) ; un contour
//! dont le décalage donnerait plusieurs boucles échoue.

use crate::region::EdgeGeom;
use crate::split::{chain_signed_area_mm2, edge_portion, edge_sample, edge_trace};
use crate::{
    distance_mm, intersect, offset_chain, GeometryError, OffsetOptions, PointMm, Result, Trace,
    VectorMm, MAX_COORD_MM,
};

/// Plus grande marge acceptée (mm).
pub const MAX_SEAM_MM: f64 = 1000.0;
/// Pointe maximale d'un coin extérieur : distance du sommet à la pointe ÷ la plus grande marge
/// adjacente ; au-delà, le coin est coupé en biseau.
pub const SEAM_MITER_LIMIT: f64 = 4.0;
/// Borne (mm) de la longueur maximale de pointe et de la profondeur d'un coin tronqué.
pub const MAX_CORNER_MM: f64 = 10_000.0;
/// Deux extrémités plus proches que cela (mm) sont confondues.
const JOIN_MM: f64 = 1e-9;
/// Un contour de coupe dont deux morceaux consécutifs s'écartent de plus que cela est refusé.
const GAP_MM: f64 = 1e-4;
/// Un segment de contour de coupe plus court que cela (mm) est un éperon de calcul : retiré.
const NEGLIGIBLE_MM: f64 = 1e-5;
/// Tolérance (fraction du paramètre) pour une intersection en bout d'arête.
const PARAM_EPS: f64 = 1e-7;

/// Marge d'une arête : au premier point (`start_mm`) et au dernier (`end_mm`) dans le sens du
/// contour de couture (anti-horaire). Début ≠ fin = marge progressive (segments seulement).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EdgeMargin {
    pub start_mm: f64,
    pub end_mm: f64,
}

impl EdgeMargin {
    pub fn uniform(mm: f64) -> Self {
        EdgeMargin {
            start_mm: mm,
            end_mm: mm,
        }
    }
}

/// Type de coin extérieur d'un sommet (voir la documentation du module).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CornerKind {
    Mitre,
    PerpendicularPrevious,
    PerpendicularNext,
    Symmetric,
    Truncated { depth_mm: f64 },
    Stepped,
    HemFoldNext,
    HemFoldPrevious,
}

/// Type de coin d'un sommet : `vertex` = début de l'arête `vertex` de la chaîne.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CornerSpec {
    pub vertex: usize,
    pub kind: CornerKind,
}

/// Réglages des coins : un type par sommet (les autres sont en onglet) et la longueur maximale
/// d'une pointe d'onglet (`None` = 4 × la plus grande marge adjacente).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SeamOptions {
    pub corners: Vec<CornerSpec>,
    pub max_tip_mm: Option<f64>,
}

/// Nature d'un sommet de la chaîne.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VertexTurn {
    /// Tangentes confondues (courbe lisse).
    Smooth,
    /// Virage à gauche : coin extérieur du contour anti-horaire.
    Convex,
    /// Virage à droite : coin rentrant.
    Reentrant,
    /// Demi-tour.
    Reversal,
}

fn turn_angle(prev: &EdgeGeom, next: &EdgeGeom) -> Result<f64> {
    let (t_in, t_out) = (end_dir(prev)?, start_dir(next)?);
    Ok(t_in.cross(t_out).atan2(t_in.dot(t_out)))
}

/// Nature du sommet `vertex` (début de l'arête `vertex`) d'une chaîne fermée.
pub fn vertex_turn(chain: &[EdgeGeom], vertex: usize) -> Result<VertexTurn> {
    let n = chain.len();
    if vertex >= n {
        return Err(err("sommet inexistant"));
    }
    let theta = turn_angle(&chain[(vertex + n - 1) % n], &chain[vertex])?;
    Ok(if theta.abs() > std::f64::consts::PI - 1e-6 {
        VertexTurn::Reversal
    } else if theta.abs() < 1e-9 {
        VertexTurn::Smooth
    } else if theta > 0.0 {
        VertexTurn::Convex
    } else {
        VertexTurn::Reentrant
    })
}

fn err(msg: &str) -> GeometryError {
    GeometryError::InvalidConstruction(msg.to_string())
}

/// Direction unitaire au début d'une arête élémentaire.
fn start_dir(g: &EdgeGeom) -> Result<VectorMm> {
    match g {
        EdgeGeom::Line(a, b) => (*b - *a).unit(),
        EdgeGeom::Arc(a) => Ok(a.point_at_length(0.0)?.1),
        EdgeGeom::Cubic(c) => c.unit_tangent_at(0.0),
    }
}

/// Direction unitaire à la fin d'une arête élémentaire.
fn end_dir(g: &EdgeGeom) -> Result<VectorMm> {
    match g {
        EdgeGeom::Line(a, b) => (*b - *a).unit(),
        EdgeGeom::Arc(a) => Ok(a.point_at_length(a.length_mm())?.1),
        EdgeGeom::Cubic(c) => c.unit_tangent_at(1.0),
    }
}

/// Décalage d'une arête seule (vers la droite de son sens), pièces dans l'ordre du parcours.
fn offset_edge(g: &EdgeGeom, m: EdgeMargin, opts: &OffsetOptions) -> Result<Vec<EdgeGeom>> {
    if let EdgeGeom::Line(a, b) = g {
        let u = (*b - *a).unit()?;
        let right = VectorMm::new(u.y, -u.x);
        return Ok(vec![EdgeGeom::Line(
            *a + right * m.start_mm,
            *b + right * m.end_mm,
        )]);
    }
    if m.start_mm != m.end_mm {
        return Err(err(
            "marge progressive : segments seulement (une courbe a une marge constante)",
        ));
    }
    let mm = m.start_mm;
    if mm == 0.0 {
        return Ok(vec![*g]);
    }
    match g {
        EdgeGeom::Arc(a) => {
            // À droite : s'éloigne du centre d'un arc anti-horaire, s'en rapproche sinon.
            let radius = a.radius_mm + mm * a.sweep_rad.signum();
            if radius <= 1e-6 {
                return Err(err(
                    "marge trop grande : elle dépasse le rayon d'un arc rentrant",
                ));
            }
            Ok(vec![EdgeGeom::Arc(crate::Arc {
                center: a.center,
                radius_mm: radius,
                start_rad: a.start_rad,
                sweep_rad: a.sweep_rad,
            })])
        }
        EdgeGeom::Cubic(c) => {
            let traces = offset_chain(&[Trace::Cubic(*c)], -mm, opts)?;
            let pieces: Vec<EdgeGeom> = traces
                .iter()
                .flat_map(crate::region::trace_geometry)
                .collect();
            if pieces
                .windows(2)
                .any(|w| distance_mm(w[0].end(), w[1].start()) > GAP_MM)
            {
                return Err(err(
                    "marge trop grande pour cette courbe (son décalage se replie)",
                ));
            }
            Ok(pieces)
        }
        EdgeGeom::Line(..) => unreachable!("traité plus haut"),
    }
}

/// Résultat du traitement d'un sommet.
enum Join {
    /// Les deux décalages se rejoignent seuls.
    None,
    /// Coin : couper le décalage précédent (`prev_end`) et / ou le suivant (`next_start`) à
    /// `(pièce, paramètre)`, puis insérer `pieces` entre les deux.
    Corner {
        prev_end: Option<(usize, f64)>,
        next_start: Option<(usize, f64)>,
        pieces: Vec<EdgeGeom>,
    },
}

/// Coin extérieur entre deux segments décalés : sommet = intersection des deux **droites**
/// décalées (le décalage précédent est prolongé ou raccourci, le suivant aussi). `None` si les
/// droites sont parallèles ou si le sommet tombe hors d'un des segments (trop court).
fn line_corner(last: &EdgeGeom, first: &EdgeGeom, kprev: usize) -> Option<(Join, PointMm)> {
    let (EdgeGeom::Line(p, q), EdgeGeom::Line(r, t)) = (last, first) else {
        return None;
    };
    let (u, w) = (*q - *p, *t - *r);
    let denom = u.cross(w);
    if denom.abs() <= 1e-12 * u.norm() * w.norm() {
        return None;
    }
    let d = *r - *p;
    // Paramètres du sommet sur chaque droite (fraction de la longueur du segment décalé).
    let (tp, tn) = (d.cross(w) / denom, d.cross(u) / denom);
    if tp <= PARAM_EPS || tn >= 1.0 - PARAM_EPS {
        return None;
    }
    let x = p.lerp(*q, tp);
    let (e, s) = (*q, *r);
    let mut pieces = Vec::new();
    if tp > 1.0 + PARAM_EPS {
        pieces.push(EdgeGeom::Line(e, x));
    }
    if tn < -PARAM_EPS {
        pieces.push(EdgeGeom::Line(x, s));
    }
    Some((
        Join::Corner {
            prev_end: (tp < 1.0 - PARAM_EPS).then_some((kprev, tp)),
            next_start: (tn > PARAM_EPS).then_some((0, tn)),
            pieces,
        },
        x,
    ))
}

/// Coin de type choisi (tout sauf l'onglet) entre deux décalages : calcule les deux points
/// `p1` (sur la droite de `L1`) et `p2` (sur celle de `L2`) reliés par le segment de fermeture.
#[allow(clippy::too_many_arguments)]
fn custom_corner(
    prev: &EdgeGeom,
    next: &EdgeGeom,
    prev_off: &[EdgeGeom],
    next_off: &[EdgeGeom],
    kind: CornerKind,
    a: f64,
    b: f64,
    theta: f64,
) -> Result<Join> {
    if theta.abs() < 1e-9 {
        return Err(err(
            "un type de coin ne s'applique pas à un sommet lisse (tangentes confondues)",
        ));
    }
    if theta < 0.0 {
        return Err(err(
            "un type de coin ne s'applique qu'à un coin saillant : ici le coin est rentrant",
        ));
    }
    let (Some(last), Some(first)) = (prev_off.last(), next_off.first()) else {
        return Err(err("décalage vide"));
    };
    let (v, e, s) = (prev.end(), last.end(), first.start());
    let (t1, t2) = (end_dir(prev)?, start_dir(next)?);
    let (u1, u2) = (end_dir(last)?, start_dir(first)?);
    let (n1, n2) = (VectorMm::new(t1.y, -t1.x), VectorMm::new(t2.y, -t2.x));
    let degenerate = || err("coin impossible : géométrie dégénérée");
    let (p1, p2) = match kind {
        CornerKind::Mitre => unreachable!("l'onglet est traité par `join`"),
        CornerKind::Stepped => {
            let m = e + (s - v);
            return Ok(Join::Corner {
                prev_end: None,
                next_start: None,
                pieces: vec![EdgeGeom::Line(e, m), EdgeGeom::Line(m, s)],
            });
        }
        CornerKind::PerpendicularPrevious => (e + u1 * (s - e).dot(u1), s),
        CornerKind::PerpendicularNext => (e, s + u2 * (e - s).dot(u2)),
        CornerKind::Symmetric | CornerKind::Truncated { .. } => {
            let w = VectorMm::new(n1.x + n2.x, n1.y + n2.y)
                .unit()
                .map_err(|_| degenerate())?;
            let (dw1, dw2) = (u1.dot(w), u2.dot(w));
            if dw1.abs() < 1e-12 || dw2.abs() < 1e-12 {
                return Err(degenerate());
            }
            let height = |p: PointMm| (p - v).dot(w);
            let h = match kind {
                CornerKind::Truncated { depth_mm } => {
                    let denom = u1.cross(u2);
                    if denom.abs() < 1e-12 {
                        return Err(degenerate());
                    }
                    let tip = e + u1 * ((s - e).cross(u2) / denom);
                    height(tip) - depth_mm
                }
                _ => height(e).max(height(s)),
            };
            (
                e + u1 * ((h - height(e)) / dw1),
                s + u2 * ((h - height(s)) / dw2),
            )
        }
        CornerKind::HemFoldNext => {
            let denom = u1.cross(t2);
            if denom.abs() < 1e-12 {
                return Err(degenerate());
            }
            let p1 = e + u1 * (-((e - v).cross(t2)) / denom);
            let k = 2.0 * u1.dot(t2);
            let mut d = VectorMm::new(t2.x * k - u1.x, t2.y * k - u1.y);
            if d.dot(n2) < 0.0 {
                d = d * -1.0;
            }
            if d.dot(n2) < 1e-9 {
                return Err(degenerate());
            }
            let p2 = p1 + d * (b / d.dot(n2));
            if (p2 - s).cross(u2).abs() > 1e-6 {
                return Err(err(
                    "ourlet rabattu : le bord suivant doit être un segment droit",
                ));
            }
            (p1, p2)
        }
        CornerKind::HemFoldPrevious => {
            let denom = u2.cross(t1);
            if denom.abs() < 1e-12 {
                return Err(degenerate());
            }
            let p2 = s + u2 * (-((s - v).cross(t1)) / denom);
            let k = 2.0 * u2.dot(t1);
            let mut d = VectorMm::new(t1.x * k - u2.x, t1.y * k - u2.y);
            if d.dot(n1) < 0.0 {
                d = d * -1.0;
            }
            if d.dot(n1) < 1e-9 {
                return Err(degenerate());
            }
            let p1 = p2 + d * (a / d.dot(n1));
            if (p1 - e).cross(u1).abs() > 1e-6 {
                return Err(err(
                    "ourlet rabattu : le bord précédent doit être un segment droit",
                ));
            }
            (p1, p2)
        }
    };
    // Points sur les droites des deux décalages : prolonger (ou, pour un segment, raccourcir).
    let (lambda, mu) = ((p1 - e).dot(u1), (p2 - s).dot(u2));
    let mut pieces = Vec::new();
    let mut prev_end = None;
    let mut next_start = None;
    if lambda > 1e-9 {
        pieces.push(EdgeGeom::Line(e, p1));
    } else if lambda < -1e-9 {
        let len = distance_mm(last.start(), e);
        let t = 1.0 + lambda / len;
        if !matches!(last, EdgeGeom::Line(..)) || t <= PARAM_EPS {
            return Err(err(
                "coin impossible : le bord précédent est trop court ou courbe pour être raccourci",
            ));
        }
        prev_end = Some((prev_off.len() - 1, t));
    }
    if distance_mm(p1, p2) > JOIN_MM {
        pieces.push(EdgeGeom::Line(p1, p2));
    }
    if mu < -1e-9 {
        pieces.push(EdgeGeom::Line(p2, s));
    } else if mu > 1e-9 {
        let len = distance_mm(s, first.end());
        let t = mu / len;
        if !matches!(first, EdgeGeom::Line(..)) || t >= 1.0 - PARAM_EPS {
            return Err(err(
                "coin impossible : le bord suivant est trop court ou courbe pour être raccourci",
            ));
        }
        next_start = Some((0, t));
    }
    Ok(Join::Corner {
        prev_end,
        next_start,
        pieces,
    })
}

/// Intersections entre deux pièces de décalage : `(paramètre sur a, paramètre sur b, point)`.
fn piece_hits(a: &EdgeGeom, b: &EdgeGeom) -> Result<Vec<(f64, f64, PointMm)>> {
    if let (EdgeGeom::Line(p, q), EdgeGeom::Line(r, s)) = (a, b) {
        let (u, v) = (*q - *p, *s - *r);
        let denom = u.cross(v);
        if denom.abs() <= 1e-12 * u.norm() * v.norm() {
            return Ok(Vec::new());
        }
        let w = *r - *p;
        let (t, k) = (w.cross(v) / denom, w.cross(u) / denom);
        let range = -PARAM_EPS..=1.0 + PARAM_EPS;
        if range.contains(&t) && range.contains(&k) {
            let (t, k) = (t.clamp(0.0, 1.0), k.clamp(0.0, 1.0));
            return Ok(vec![(t, k, p.lerp(*q, t))]);
        }
        return Ok(Vec::new());
    }
    Ok(intersect(&edge_trace(a), &edge_trace(b))?
        .into_iter()
        .map(|h| (h.t1, h.t2, h.point))
        .collect())
}

/// Traite le sommet entre l'arête `prev` (décalée en `prev_off`) et `next` (en `next_off`).
#[allow(clippy::too_many_arguments)]
fn join(
    prev: &EdgeGeom,
    next: &EdgeGeom,
    prev_off: &[EdgeGeom],
    next_off: &[EdgeGeom],
    margin_prev_end: f64,
    margin_next_start: f64,
    kind: CornerKind,
    max_tip: Option<f64>,
) -> Result<Join> {
    let (Some(last), Some(first)) = (prev_off.last(), next_off.first()) else {
        return Err(err("décalage vide"));
    };
    let (e, s) = (last.end(), first.start());
    if distance_mm(e, s) <= JOIN_MM {
        return Ok(Join::None);
    }
    let v = prev.end();
    let (t_in, t_out) = (end_dir(prev)?, start_dir(next)?);
    let theta = t_in.cross(t_out).atan2(t_in.dot(t_out));
    if theta.abs() > std::f64::consts::PI - 1e-6 {
        return Err(err(
            "pointe en demi-tour : la marge ne peut pas être calculée",
        ));
    }
    if kind != CornerKind::Mitre {
        return custom_corner(
            prev,
            next,
            prev_off,
            next_off,
            kind,
            margin_prev_end,
            margin_next_start,
            theta,
        );
    }
    let bevel = || Join::Corner {
        prev_end: None,
        next_start: None,
        pieces: vec![EdgeGeom::Line(e, s)],
    };
    if theta.abs() < 1e-9 {
        return Ok(bevel()); // sommet lisse, marges différentes : une marche
    }
    // Deux segments : le sommet est l'intersection des droites décalées (onglet d'un coin
    // extérieur, borné par la limite de pointe ; coupe d'un coin rentrant, jamais de biseau).
    let reach = max_tip.unwrap_or(margin_prev_end.max(margin_next_start) * SEAM_MITER_LIMIT);
    if let Some((corner, tip)) = line_corner(last, first, prev_off.len() - 1) {
        return Ok(if theta > 0.0 && distance_mm(tip, v) > reach {
            bevel()
        } else {
            corner
        });
    }
    if theta > 0.0 {
        // Courbe voisine : prolongement des tangentes des deux décalages.
        let (u_in, u_out) = (end_dir(last)?, start_dir(first)?);
        let denom = u_in.cross(u_out);
        if denom.abs() >= 1e-12 {
            let d = s - e;
            let (lambda, mu) = (d.cross(u_out) / denom, d.cross(u_in) / denom);
            if lambda >= -1e-9 && mu <= 1e-9 {
                let tip = e + u_in * lambda;
                if distance_mm(tip, v) > reach {
                    return Ok(bevel());
                }
                let mut pieces = Vec::new();
                for (p, q) in [(e, tip), (tip, s)] {
                    if distance_mm(p, q) > JOIN_MM {
                        pieces.push(EdgeGeom::Line(p, q));
                    }
                }
                return Ok(Join::Corner {
                    prev_end: None,
                    next_start: None,
                    pieces,
                });
            }
        }
        // Marges très différentes : les décalages se croisent avant leurs bouts (coupe), sinon biseau.
        return Ok(trim(prev_off, next_off, v)?.unwrap_or_else(bevel));
    }
    // Coin rentrant : les deux décalages se croisent ; on garde l'intersection la plus proche.
    trim(prev_off, next_off, v)?.ok_or_else(|| {
        err("coin rentrant : la marge est trop grande pour les bords adjacents (ils sont trop courts)")
    })
}

/// Intersection la plus proche de `v` entre deux décalages consécutifs, comme coupe des deux.
fn trim(prev_off: &[EdgeGeom], next_off: &[EdgeGeom], v: PointMm) -> Result<Option<Join>> {
    let mut best: Option<(f64, Join)> = None;
    for (k, a) in prev_off.iter().enumerate() {
        for (l, b) in next_off.iter().enumerate() {
            for (ta, tb, point) in piece_hits(a, b)? {
                let dist = distance_mm(point, v);
                if best.as_ref().is_none_or(|(d, _)| dist < *d) {
                    let corner = Join::Corner {
                        prev_end: Some((k, ta)),
                        next_start: Some((l, tb)),
                        pieces: Vec::new(),
                    };
                    best = Some((dist, corner));
                }
            }
        }
    }
    Ok(best.map(|(_, corner)| corner))
}

/// Partie d'un décalage entre deux coupes (`None` = bout de l'arête).
fn slice(
    pieces: &[EdgeGeom],
    start: Option<(usize, f64)>,
    end: Option<(usize, f64)>,
) -> Result<Vec<EdgeGeom>> {
    let (k0, t0) = start.unwrap_or((0, 0.0));
    let (k1, t1) = end.unwrap_or((pieces.len() - 1, 1.0));
    if k1 < k0 || (k1 == k0 && t1 <= t0 + 1e-9) {
        return Err(err(
            "marge trop grande : un bord disparaît dans le décalage (coins rentrants trop proches)",
        ));
    }
    let mut out = Vec::new();
    for (k, piece) in pieces.iter().enumerate().take(k1 + 1).skip(k0) {
        let a = if k == k0 { t0 } else { 0.0 };
        let b = if k == k1 { t1 } else { 1.0 };
        if b - a > 1e-9 {
            out.push(edge_portion(piece, a, b));
        }
    }
    Ok(out)
}

/// Fusionne les segments consécutifs alignés (le contour reste le même).
fn merge_collinear(edges: &mut Vec<EdgeGeom>) {
    let mergeable = |a: &EdgeGeom, b: &EdgeGeom| -> Option<EdgeGeom> {
        if let (EdgeGeom::Line(a0, a1), EdgeGeom::Line(b0, b1)) = (a, b) {
            let (u, v) = (*a1 - *a0, *b1 - *b0);
            if distance_mm(*a1, *b0) <= 1e-7
                && u.cross(v).abs() <= 1e-9 * u.norm() * v.norm()
                && u.dot(v) > 0.0
            {
                return Some(EdgeGeom::Line(*a0, *b1));
            }
        }
        None
    };
    let mut out: Vec<EdgeGeom> = Vec::new();
    for e in edges.drain(..) {
        if let Some(m) = out.last().and_then(|l| mergeable(l, &e)) {
            *out.last_mut().expect("non vide") = m;
        } else {
            out.push(e);
        }
    }
    while out.len() > 1 {
        match mergeable(&out[out.len() - 1], &out[0]) {
            Some(m) => {
                out[0] = m;
                out.pop();
            }
            None => break,
        }
    }
    *edges = out;
}

/// Décalage arête par arête (marges différentes).
fn per_edge(
    chain: &[EdgeGeom],
    margins: &[EdgeMargin],
    opts: &OffsetOptions,
    options: &SeamOptions,
) -> Result<Vec<EdgeGeom>> {
    let n = chain.len();
    let offs: Vec<Vec<EdgeGeom>> = chain
        .iter()
        .zip(margins)
        .map(|(g, m)| offset_edge(g, *m, opts))
        .collect::<Result<_>>()?;
    // Sommet j = entre l'arête j−1 et l'arête j.
    let mut joins = Vec::with_capacity(n);
    for j in 0..n {
        let p = (j + n - 1) % n;
        joins.push(join(
            &chain[p],
            &chain[j],
            &offs[p],
            &offs[j],
            margins[p].end_mm,
            margins[j].start_mm,
            options
                .corners
                .iter()
                .find(|c| c.vertex == j)
                .map_or(CornerKind::Mitre, |c| c.kind),
            options.max_tip_mm,
        )?);
    }
    let mut out = Vec::new();
    for i in 0..n {
        let (mut start, mut end) = (None, None);
        if let Join::Corner {
            next_start, pieces, ..
        } = &joins[i]
        {
            out.extend(pieces.iter().copied());
            start = *next_start;
        }
        if let Join::Corner { prev_end, .. } = &joins[(i + 1) % n] {
            end = *prev_end;
        }
        out.extend(slice(&offs[i], start, end)?);
    }
    Ok(out)
}

/// Contrôle du résultat : fini, dans l'espace de travail, continu et fermé, sans croisement.
fn check_result(edges: &[EdgeGeom]) -> Result<()> {
    if edges.len() < 2 {
        return Err(err("contour de coupe dégénéré"));
    }
    for e in edges {
        let pts = edge_sample(e, 8);
        if pts.iter().any(|p| {
            !p.x.is_finite() || !p.y.is_finite() || p.x.abs().max(p.y.abs()) > MAX_COORD_MM
        }) {
            return Err(err("le contour de coupe sort de l'espace de travail"));
        }
    }
    for (i, e) in edges.iter().enumerate() {
        if distance_mm(e.end(), edges[(i + 1) % edges.len()].start()) > GAP_MM {
            return Err(err(
                "le contour de coupe n'est pas une boucle fermée (la marge sépare le contour en \
                 plusieurs morceaux)",
            ));
        }
    }
    if crosses_itself(edges) {
        return Err(err(
            "le contour de coupe se recoupe lui-même (marge trop grande pour cette forme)",
        ));
    }
    Ok(())
}

/// Deux morceaux non adjacents du contour se croisent-ils en leur intérieur ?
fn crosses_itself(edges: &[EdgeGeom]) -> bool {
    let polys: Vec<Vec<PointMm>> = edges.iter().map(|e| edge_sample(e, 48)).collect();
    let boxes: Vec<[f64; 4]> = polys
        .iter()
        .map(|pl| {
            pl.iter()
                .fold([f64::MAX, f64::MAX, f64::MIN, f64::MIN], |b, q| {
                    [b[0].min(q.x), b[1].min(q.y), b[2].max(q.x), b[3].max(q.y)]
                })
        })
        .collect();
    let crosses = |a: PointMm, b: PointMm, c: PointMm, d: PointMm| {
        let (u, v) = (b - a, d - c);
        let denom = u.cross(v);
        if denom.abs() < 1e-12 {
            return false;
        }
        let t = (c - a).cross(v) / denom;
        let s = (c - a).cross(u) / denom;
        let eps = 1e-9;
        t > eps && t < 1.0 - eps && s > eps && s < 1.0 - eps
    };
    let n = polys.len();
    for i in 0..n {
        for j in i..n {
            if i != j {
                let (p, q) = (boxes[i], boxes[j]);
                if p[0] > q[2] + 1e-6
                    || q[0] > p[2] + 1e-6
                    || p[1] > q[3] + 1e-6
                    || q[1] > p[3] + 1e-6
                {
                    continue;
                }
            }
            for (ia, wa) in polys[i].windows(2).enumerate() {
                for (ib, wb) in polys[j].windows(2).enumerate() {
                    if i == j && ib <= ia + 1 {
                        continue;
                    }
                    if crosses(wa[0], wa[1], wb[0], wb[1]) {
                        return true;
                    }
                }
            }
        }
    }
    false
}

/// Contour de coupe d'un contour de couture. Voir la documentation du module.
///
/// `chain` : arêtes élémentaires d'une chaîne fermée anti-horaire ; `margins` : une marge par
/// arête (mêmes indices). Erreurs expliquées : marge négative, non finie ou > [`MAX_SEAM_MM`],
/// chaîne ouverte ou horaire, marge progressive sur une courbe, décalage impossible.
pub fn seam_contour(chain: &[EdgeGeom], margins: &[EdgeMargin]) -> Result<Vec<Trace>> {
    seam_contour_with(chain, margins, &SeamOptions::default())
}

/// Comme [`seam_contour`], avec un type de coin par sommet et une longueur maximale de pointe
/// (voir la documentation du module). Erreur expliquée si un sommet est inexistant ou cité
/// deux fois, si une valeur sort de ses bornes, ou si un coin est impossible.
pub fn seam_contour_with(
    chain: &[EdgeGeom],
    margins: &[EdgeMargin],
    options: &SeamOptions,
) -> Result<Vec<Trace>> {
    if chain.is_empty() || chain.len() != margins.len() {
        return Err(err("contour de couture vide ou marges mal comptées"));
    }
    for m in margins {
        for v in [m.start_mm, m.end_mm] {
            if !v.is_finite() || !(0.0..=MAX_SEAM_MM).contains(&v) {
                return Err(err(&format!(
                    "une marge doit être un nombre fini compris entre 0 et {MAX_SEAM_MM} mm"
                )));
            }
        }
    }
    for (i, e) in chain.iter().enumerate() {
        if distance_mm(e.end(), chain[(i + 1) % chain.len()].start()) > 1e-6 {
            return Err(err("le contour de couture n'est pas fermé"));
        }
    }
    if let Some(l) = options.max_tip_mm {
        if !l.is_finite() || !(0.0..=MAX_CORNER_MM).contains(&l) {
            return Err(err(&format!(
                "la longueur maximale d'une pointe doit être comprise entre 0 et {MAX_CORNER_MM} mm"
            )));
        }
    }
    for (i, c) in options.corners.iter().enumerate() {
        if c.vertex >= chain.len() {
            return Err(err("un type de coin porte sur un sommet inexistant"));
        }
        if options.corners[..i].iter().any(|o| o.vertex == c.vertex) {
            return Err(err("un sommet a deux types de coin : un seul par sommet"));
        }
        if c.kind != CornerKind::Mitre {
            match vertex_turn(chain, c.vertex)? {
                VertexTurn::Smooth => {
                    return Err(err(
                        "un type de coin ne s'applique pas à un sommet lisse (tangentes confondues)",
                    ))
                }
                VertexTurn::Reentrant => {
                    return Err(err(
                        "un type de coin ne s'applique qu'à un coin saillant : ici le coin est rentrant",
                    ))
                }
                _ => {}
            }
        }
        if let CornerKind::Truncated { depth_mm } = c.kind {
            if !depth_mm.is_finite() || !(0.0..=MAX_CORNER_MM).contains(&depth_mm) {
                return Err(err(&format!(
                    "la profondeur d'un coin tronqué doit être comprise entre 0 et {MAX_CORNER_MM} mm"
                )));
            }
        }
    }
    if chain_signed_area_mm2(chain) <= 0.0 {
        return Err(err(
            "le contour de couture doit être anti-horaire et d'aire non nulle",
        ));
    }
    let opts = OffsetOptions {
        miter_limit: SEAM_MITER_LIMIT,
        ..OffsetOptions::default()
    };
    let first = margins[0];
    let uniform = margins
        .iter()
        .all(|m| m.start_mm == first.start_mm && m.end_mm == first.start_mm);
    if uniform && first.start_mm == 0.0 {
        return Ok(chain.iter().map(edge_trace).collect());
    }
    let mut edges: Vec<EdgeGeom> = if uniform && options.corners.is_empty() {
        let traces: Vec<Trace> = chain.iter().map(edge_trace).collect();
        let opts = OffsetOptions {
            miter_limit: options
                .max_tip_mm
                .map_or(SEAM_MITER_LIMIT, |l| (l / first.start_mm).max(1.0)),
            ..opts
        };
        offset_chain(&traces, -first.start_mm, &opts)?
            .iter()
            .flat_map(crate::region::trace_geometry)
            .collect()
    } else {
        per_edge(chain, margins, &opts, options)?
    };
    // `offset_chain` laisse parfois un éperon de quelques micromètres à un raccord : les segments
    // de moins de 1e-5 mm sont retirés (l'écart de raccord reste très en dessous de `GAP_MM`).
    let before = edges.len();
    edges.retain(|e| !matches!(e, EdgeGeom::Line(a, b) if distance_mm(*a, *b) < NEGLIGIBLE_MM));
    if edges.len() < before && edges.len() < 3 {
        return Err(err("contour de coupe dégénéré"));
    }
    merge_collinear(&mut edges);
    check_result(&edges)?;
    Ok(edges.iter().map(edge_trace).collect())
}

#[path = "seam_dart.rs"]
mod dart_closing;
pub use dart_closing::{seam_contour_closed_darts, DartClosing};

#[cfg(test)]
#[path = "seam_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "seam_corner_tests.rs"]
mod corner_tests;
