//! US-04.6 — coupe d'une chaîne fermée par une droite, déplacement rigide et
//! écart de forme entre deux bords (séparer / assembler des pièces).
//!
//! Tout est en mm, sans état : les fonctions reçoivent des chaînes d'arêtes
//! élémentaires ([`EdgeGeom`] : segment, Bézier, arc) et rendent d'autres chaînes.
//! La coupe est **exacte** : les points de coupe viennent de [`crate::intersect`]
//! (segment, arc, Bézier, tangence reconnue à 1e-9 mm), les arêtes sont subdivisées
//! par de Casteljau (Bézier) ou par balayage (arc), sans approximation. La somme des
//! aires des deux moitiés est celle de la chaîne (formule de Green).
//!
//! La droite de coupe est **infinie** (définie par deux points) ; la chaîne doit
//! être coupée en exactement deux points distincts (à 1e-6 mm), sinon une raison.

use crate::region::{sub_cubic, trace_nearest_param, EdgeGeom, JOIN_TOLERANCE_MM};
use crate::{
    distance_mm, intersect, Arc, Bbox, CubicBezier, GeometryError, PointMm, Trace, EPSILON_MM,
};
use std::fmt;

/// Une aire de moitié inférieure (mm²) n'en est pas une.
const MIN_HALF_AREA_MM2: f64 = 1e-9;
/// Distance (mm) à la droite sous laquelle une moitié est dite « sur la droite ».
const SIDE_TOLERANCE_MM: f64 = 1e-9;
/// Écart maximal (mm) entre la fin d'une arête et le début de la suivante.
const CLOSED_GAP_MM: f64 = 1e-4;
/// Écart relatif toléré entre l'aire de la chaîne et la somme des aires des moitiés.
const AREA_RELATIVE_TOLERANCE: f64 = 1e-9;

/// Pourquoi une chaîne n'est pas coupée en deux.
#[derive(Debug, Clone, PartialEq)]
pub enum CutError {
    /// Les deux points de la droite sont confondus (ou non finis).
    DegenerateLine,
    /// La droite ne rencontre pas la chaîne.
    NoCrossing,
    /// La droite ne touche la chaîne qu'en un point, ou la frôle (tangente).
    Touches,
    /// La droite coupe la chaîne en plus de deux points (forme concave).
    TooManyCrossings { count: usize },
    /// La droite longe une arête sur une longueur non nulle.
    AlongEdge,
    /// Une moitié a une aire nulle.
    TinyHalf,
    /// La ligne de découpe passe hors de la chaîne entre ses deux traversées.
    OutsidePiece,
    /// La ligne de découpe se recoupe elle-même.
    SelfCrossing,
    /// Chaîne vide, non fermée ou illisible (raison).
    BadChain(String),
}

impl fmt::Display for CutError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CutError::DegenerateLine => write!(
                f,
                "la ligne de coupe est dégénérée : ses deux points sont confondus"
            ),
            CutError::NoCrossing => write!(f, "la ligne de coupe ne traverse pas le contour"),
            CutError::Touches => write!(
                f,
                "la ligne de coupe ne fait que toucher le contour (un seul point de contact ou \
                 tangente) : elle ne sépare pas la pièce"
            ),
            CutError::TooManyCrossings { count } => write!(
                f,
                "la ligne de coupe traverse le contour en {count} points : seule une coupe en \
                 exactement deux points (deux morceaux) est prise en charge"
            ),
            CutError::AlongEdge => write!(
                f,
                "la ligne de coupe longe un bord du contour : elle ne sépare pas la pièce"
            ),
            CutError::TinyHalf => write!(f, "la coupe donnerait un morceau d'aire nulle"),
            CutError::OutsidePiece => write!(
                f,
                "la ligne de découpe passe hors de la pièce entre ses deux traversées du contour"
            ),
            CutError::SelfCrossing => write!(f, "la ligne de découpe se recoupe elle-même"),
            CutError::BadChain(why) => write!(f, "contour inutilisable pour la coupe ({why})"),
        }
    }
}

impl std::error::Error for CutError {}

/// Arête d'une moitié et arête de la chaîne d'origine dont elle provient.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CutEdge {
    pub geom: EdgeGeom,
    /// Indice de l'arête de la chaîne d'origine ; `None` pour l'arête de coupe.
    pub source: Option<usize>,
}

/// Résultat de la coupe d'une chaîne par une droite.
#[derive(Debug, Clone, PartialEq)]
pub struct ChainCut {
    /// Moitié à **gauche** de la droite orientée `a → b` (arêtes dans le sens de la
    /// chaîne d'origine, arête de coupe en dernier).
    pub left: Vec<CutEdge>,
    /// Moitié à droite.
    pub right: Vec<CutEdge>,
    /// Points de coupe, dans l'ordre le long de `a → b`.
    pub points: [PointMm; 2],
}

/// Arête → tracé borné (pour `intersect`, distances).
pub fn edge_trace(e: &EdgeGeom) -> Trace {
    match e {
        EdgeGeom::Line(a, b) => Trace::Line(*a, *b),
        EdgeGeom::Cubic(c) => Trace::Cubic(*c),
        EdgeGeom::Arc(a) => Trace::Arc(*a),
    }
}

/// Portion `[t0 ; t1]` d'une arête (`t` : fraction de la longueur pour un segment,
/// paramètre de la Bézier, fraction du balayage pour un arc). Exacte.
pub fn edge_portion(e: &EdgeGeom, t0: f64, t1: f64) -> EdgeGeom {
    if t0 <= 0.0 && t1 >= 1.0 {
        return *e;
    }
    match e {
        EdgeGeom::Line(a, b) => EdgeGeom::Line(
            if t0 <= 0.0 { *a } else { a.lerp(*b, t0) },
            if t1 >= 1.0 { *b } else { a.lerp(*b, t1) },
        ),
        EdgeGeom::Cubic(c) => EdgeGeom::Cubic(sub_cubic(c, t0, t1)),
        EdgeGeom::Arc(a) => EdgeGeom::Arc(Arc {
            center: a.center,
            radius_mm: a.radius_mm,
            start_rad: a.start_rad + a.sweep_rad * t0,
            sweep_rad: a.sweep_rad * (t1 - t0),
        }),
    }
}

/// Polyligne de `n` pas par courbe (segment : ses deux extrémités).
pub fn edge_sample(e: &EdgeGeom, n: usize) -> Vec<PointMm> {
    match e {
        EdgeGeom::Line(a, b) => vec![*a, *b],
        EdgeGeom::Cubic(c) => c.sample(n),
        EdgeGeom::Arc(a) => a.sample(n),
    }
}

/// Point au milieu du paramètre d'une arête.
pub fn edge_mid(e: &EdgeGeom) -> PointMm {
    match e {
        EdgeGeom::Line(a, b) => a.lerp(*b, 0.5),
        EdgeGeom::Cubic(c) => c.point_at(0.5),
        EdgeGeom::Arc(a) => {
            let ang = a.start_rad + 0.5 * a.sweep_rad;
            PointMm::new(
                a.center.x + a.radius_mm * ang.cos(),
                a.center.y + a.radius_mm * ang.sin(),
            )
        }
    }
}

fn edge_bbox(e: &EdgeGeom) -> Bbox {
    match e {
        EdgeGeom::Line(a, b) => {
            let mut bb = Bbox::from_point(*a);
            bb.include(*b);
            bb
        }
        EdgeGeom::Cubic(c) => c.bbox(),
        EdgeGeom::Arc(a) => a.bbox(),
    }
}

/// Aire signée (Green) d'une chaîne fermée : positive si elle est parcourue en sens
/// anti-horaire.
pub fn chain_signed_area_mm2(edges: &[EdgeGeom]) -> f64 {
    let Some(first) = edges.first() else {
        return 0.0;
    };
    let origin = first.start();
    edges.iter().map(|e| e.signed_area_mm2(origin)).sum()
}

/// Longueur développée d'une suite d'arêtes (mm).
pub fn chain_length_mm(edges: &[EdgeGeom]) -> f64 {
    edges.iter().map(EdgeGeom::length_mm).sum()
}

enum Loc {
    /// Début de l'arête d'indice donné.
    Vertex(usize),
    /// À l'intérieur de l'arête, au paramètre donné.
    Inside(usize, f64),
}

struct RawHit {
    edge: usize,
    t: f64,
    point: PointMm,
}

/// Coupe la chaîne fermée `chain` par la droite infinie qui passe par `a` et `b`.
///
/// La chaîne est parcourue dans son ordre ; les deux moitiés gardent ce sens et
/// se terminent par une arête de coupe (segment) qui rejoint les deux points de
/// coupe. Refus (voir [`CutError`]) : droite dégénérée, aucune traversée, un seul
/// contact, plus de deux traversées, droite qui longe un bord.
pub fn cut_chain_by_line(chain: &[EdgeGeom], a: PointMm, b: PointMm) -> Result<ChainCut, CutError> {
    let n = chain.len();
    if n == 0 {
        return Err(CutError::BadChain("contour vide".into()));
    }
    let dir = b - a;
    let len = dir.norm();
    if !dir.x.is_finite() || !dir.y.is_finite() || len <= EPSILON_MM {
        return Err(CutError::DegenerateLine);
    }
    for j in 0..n {
        if distance_mm(chain[j].end(), chain[(j + 1) % n].start()) > CLOSED_GAP_MM {
            return Err(CutError::BadChain("la chaîne n'est pas fermée".into()));
        }
    }
    let u = dir * (1.0 / len);
    // Droite assez longue pour traverser toute la chaîne.
    let mut bb = edge_bbox(&chain[0]);
    for e in chain {
        bb.union(edge_bbox(e));
    }
    let center = PointMm::new(0.5 * (bb.min.x + bb.max.x), 0.5 * (bb.min.y + bb.max.y));
    let reach = 2.0 * (distance_mm(bb.min, bb.max) + distance_mm(a, center)) + 1.0;
    let line = Trace::Line(a + u * -reach, a + u * reach);

    let mut hits: Vec<RawHit> = Vec::new();
    for (j, e) in chain.iter().enumerate() {
        match intersect(&line, &edge_trace(e)) {
            Ok(found) => hits.extend(found.into_iter().map(|h| RawHit {
                edge: j,
                t: h.t2,
                point: h.point,
            })),
            Err(GeometryError::OverlappingTraces) => return Err(CutError::AlongEdge),
            Err(err) => return Err(CutError::BadChain(err.to_string())),
        }
    }
    // Un sommet est trouvé par deux arêtes : on regroupe les points confondus.
    let mut clusters: Vec<Vec<usize>> = Vec::new();
    for (i, h) in hits.iter().enumerate() {
        match clusters
            .iter_mut()
            .find(|c| distance_mm(hits[c[0]].point, h.point) <= JOIN_TOLERANCE_MM)
        {
            Some(c) => c.push(i),
            None => clusters.push(vec![i]),
        }
    }
    match clusters.len() {
        0 => return Err(CutError::NoCrossing),
        1 => return Err(CutError::Touches),
        2 => {}
        count => return Err(CutError::TooManyCrossings { count }),
    }
    let locs: Vec<Loc> = clusters
        .iter()
        .map(|c| {
            for &i in c {
                let h = &hits[i];
                if distance_mm(chain[h.edge].start(), h.point) <= JOIN_TOLERANCE_MM {
                    return Loc::Vertex(h.edge);
                }
                if distance_mm(chain[h.edge].end(), h.point) <= JOIN_TOLERANCE_MM {
                    return Loc::Vertex((h.edge + 1) % n);
                }
            }
            let h = &hits[c[0]];
            Loc::Inside(h.edge, h.t)
        })
        .collect();

    // Subdivision aux points de coupe ; `marks` = indices des arêtes qui commencent à une coupe.
    let mut pieces: Vec<(EdgeGeom, usize)> = Vec::with_capacity(n + 2);
    let mut marks: Vec<usize> = Vec::new();
    for (j, e) in chain.iter().enumerate() {
        if locs.iter().any(|l| matches!(l, Loc::Vertex(k) if *k == j)) {
            marks.push(pieces.len());
        }
        let mut ts: Vec<f64> = locs
            .iter()
            .filter_map(|l| match l {
                Loc::Inside(i, t) if *i == j => Some(*t),
                _ => None,
            })
            .collect();
        ts.sort_by(f64::total_cmp);
        let mut t0 = 0.0;
        for t in ts {
            pieces.push((edge_portion(e, t0, t), j));
            marks.push(pieces.len());
            t0 = t;
        }
        pieces.push((edge_portion(e, t0, 1.0), j));
    }
    if marks.len() != 2 || marks[0] >= marks[1] || marks[1] >= pieces.len() {
        return Err(CutError::BadChain(
            "incohérence interne : deux coupes attendues".into(),
        ));
    }
    let (i, k) = (marks[0], marks[1]);
    let half = |range: Vec<(EdgeGeom, usize)>| -> Vec<CutEdge> {
        let mut out: Vec<CutEdge> = range
            .into_iter()
            .map(|(geom, src)| CutEdge {
                geom,
                source: Some(src),
            })
            .collect();
        if let (Some(first), Some(last)) = (out.first(), out.last()) {
            let close = EdgeGeom::Line(last.geom.end(), first.geom.start());
            out.push(CutEdge {
                geom: close,
                source: None,
            });
        }
        out
    };
    let first_half = half(pieces[i..k].to_vec());
    let mut second_range = pieces[k..].to_vec();
    second_range.extend_from_slice(&pieces[..i]);
    let second_half = half(second_range);

    // Côté de chaque moitié : le point le plus éloigné de la droite décide.
    let side = |h: &[CutEdge]| -> f64 {
        let mut best = 0.0_f64;
        for e in h.iter().filter(|e| e.source.is_some()) {
            for p in [e.geom.start(), edge_mid(&e.geom)] {
                let d = u.cross(p - a);
                if d.abs() > best.abs() {
                    best = d;
                }
            }
        }
        best
    };
    let (s1, s2) = (side(&first_half), side(&second_half));
    if s1.abs() <= SIDE_TOLERANCE_MM || s2.abs() <= SIDE_TOLERANCE_MM || (s1 > 0.0) == (s2 > 0.0) {
        return Err(CutError::Touches);
    }
    let (left, right) = if s1 > 0.0 {
        (first_half, second_half)
    } else {
        (second_half, first_half)
    };

    let geoms = |h: &[CutEdge]| -> Vec<EdgeGeom> { h.iter().map(|e| e.geom).collect() };
    let (area_l, area_r) = (
        chain_signed_area_mm2(&geoms(&left)),
        chain_signed_area_mm2(&geoms(&right)),
    );
    if area_l.abs() <= MIN_HALF_AREA_MM2 || area_r.abs() <= MIN_HALF_AREA_MM2 {
        return Err(CutError::TinyHalf);
    }
    let total = chain_signed_area_mm2(chain);
    if (area_l + area_r - total).abs() > AREA_RELATIVE_TOLERANCE * total.abs().max(1.0) {
        return Err(CutError::BadChain(
            "incohérence interne : la somme des aires des moitiés diffère de l'aire du contour"
                .into(),
        ));
    }

    let (p0, p1) = (pieces[i].0.start(), pieces[k].0.start());
    let points = if u.dot(p0 - a) <= u.dot(p1 - a) {
        [p0, p1]
    } else {
        [p1, p0]
    };
    Ok(ChainCut {
        left,
        right,
        points,
    })
}

/// Extrémités d'une arête remplacées par `a` et `b` (écart de l'ordre de 1e-9 mm : les
/// points de coupe du contour et de la ligne sont rendus strictement identiques).
fn with_ends(e: &EdgeGeom, a: PointMm, b: PointMm) -> EdgeGeom {
    match e {
        EdgeGeom::Line(..) => EdgeGeom::Line(a, b),
        EdgeGeom::Cubic(c) => EdgeGeom::Cubic(CubicBezier {
            p0: a,
            c1: c.c1,
            c2: c.c2,
            p3: b,
        }),
        EdgeGeom::Arc(arc) => EdgeGeom::Arc(*arc),
    }
}

/// Deux segments `[a ; b]` et `[c ; d]` se croisent-ils strictement (hors extrémités) ?
fn segments_cross(a: PointMm, b: PointMm, c: PointMm, d: PointMm) -> bool {
    let o = |p: PointMm, q: PointMm, r: PointMm| (q - p).cross(r - p);
    let (d1, d2, d3, d4) = (o(a, b, c), o(a, b, d), o(c, d, a), o(c, d, b));
    d1 * d2 < 0.0 && d3 * d4 < 0.0
}

/// Coupe la chaîne fermée `chain` (anti-horaire) par une **ligne** `line` (une seule arête :
/// segment, Bézier ou arc) qui traverse le contour en exactement deux points ; seule la portion
/// de la ligne entre les deux traversées sert (une ligne qui dépasse du contour est donc
/// acceptée). Les deux moitiés gardent le sens de la chaîne et se terminent par l'arête de
/// coupe (`source: None`), **orientée du départ de la ligne vers son arrivée** (`P → Q`) dans
/// `left` (à gauche de `P → Q`) et **retournée** (`Q → P`) dans `right`. `points = [P, Q]`.
/// Refus : ligne dégénérée, aucune traversée, un seul contact, plus de deux traversées, ligne qui
/// longe un bord, qui se recoupe, qui passe hors de la chaîne, moitié d'aire nulle.
pub fn cut_chain_by_edge(chain: &[EdgeGeom], line: &EdgeGeom) -> Result<ChainCut, CutError> {
    let n = chain.len();
    if n == 0 {
        return Err(CutError::BadChain("contour vide".into()));
    }
    for j in 0..n {
        if distance_mm(chain[j].end(), chain[(j + 1) % n].start()) > CLOSED_GAP_MM {
            return Err(CutError::BadChain("la chaîne n'est pas fermée".into()));
        }
    }
    let llen = line.length_mm();
    if !llen.is_finite() || llen <= EPSILON_MM {
        return Err(CutError::DegenerateLine);
    }
    let ltrace = edge_trace(line);
    let mut hits: Vec<RawHit> = Vec::new();
    let mut hit_t1: Vec<f64> = Vec::new();
    for (j, e) in chain.iter().enumerate() {
        match intersect(&ltrace, &edge_trace(e)) {
            Ok(found) => {
                for h in found {
                    hit_t1.push(h.t1);
                    hits.push(RawHit {
                        edge: j,
                        t: h.t2,
                        point: h.point,
                    });
                }
            }
            Err(GeometryError::OverlappingTraces) => return Err(CutError::AlongEdge),
            Err(err) => return Err(CutError::BadChain(err.to_string())),
        }
    }
    let mut clusters: Vec<Vec<usize>> = Vec::new();
    for (i, h) in hits.iter().enumerate() {
        match clusters
            .iter_mut()
            .find(|c| distance_mm(hits[c[0]].point, h.point) <= JOIN_TOLERANCE_MM)
        {
            Some(c) => c.push(i),
            None => clusters.push(vec![i]),
        }
    }
    match clusters.len() {
        0 => return Err(CutError::NoCrossing),
        1 => return Err(CutError::Touches),
        2 => {}
        count => return Err(CutError::TooManyCrossings { count }),
    }
    // Ordre le long de la ligne : P (départ) puis Q.
    clusters.sort_by(|a, b| hit_t1[a[0]].total_cmp(&hit_t1[b[0]]));
    let locs: Vec<Loc> = clusters
        .iter()
        .map(|c| {
            for &i in c {
                let h = &hits[i];
                if distance_mm(chain[h.edge].start(), h.point) <= JOIN_TOLERANCE_MM {
                    return Loc::Vertex(h.edge);
                }
                if distance_mm(chain[h.edge].end(), h.point) <= JOIN_TOLERANCE_MM {
                    return Loc::Vertex((h.edge + 1) % n);
                }
            }
            let h = &hits[c[0]];
            Loc::Inside(h.edge, h.t)
        })
        .collect();
    let contour_point = |l: &Loc| match l {
        Loc::Vertex(k) => chain[*k].start(),
        Loc::Inside(j, t) => edge_portion(&chain[*j], 0.0, *t).end(),
    };
    let (p, q) = (contour_point(&locs[0]), contour_point(&locs[1]));
    let (t_p, t_q) = (hit_t1[clusters[0][0]], hit_t1[clusters[1][0]]);
    let cut = with_ends(&edge_portion(line, t_p, t_q), p, q);
    if distance_mm(p, q) <= EPSILON_MM || cut.length_mm() <= EPSILON_MM {
        return Err(CutError::DegenerateLine);
    }
    let poly = edge_sample(&cut, 64);
    for i in 0..poly.len().saturating_sub(1) {
        for j in i + 2..poly.len().saturating_sub(1) {
            if segments_cross(poly[i], poly[i + 1], poly[j], poly[j + 1]) {
                return Err(CutError::SelfCrossing);
            }
        }
    }

    let mut pieces: Vec<(EdgeGeom, usize)> = Vec::with_capacity(n + 2);
    let mut marks: Vec<usize> = Vec::new();
    for (j, e) in chain.iter().enumerate() {
        if locs.iter().any(|l| matches!(l, Loc::Vertex(k) if *k == j)) {
            marks.push(pieces.len());
        }
        let mut ts: Vec<f64> = locs
            .iter()
            .filter_map(|l| match l {
                Loc::Inside(i, t) if *i == j => Some(*t),
                _ => None,
            })
            .collect();
        ts.sort_by(f64::total_cmp);
        let mut t0 = 0.0;
        for t in ts {
            pieces.push((edge_portion(e, t0, t), j));
            marks.push(pieces.len());
            t0 = t;
        }
        pieces.push((edge_portion(e, t0, 1.0), j));
    }
    if marks.len() != 2 || marks[0] >= marks[1] || marks[1] >= pieces.len() {
        return Err(CutError::BadChain(
            "incohérence interne : deux coupes attendues".into(),
        ));
    }
    let (i, k) = (marks[0], marks[1]);
    // Chaque moitié se ferme par la ligne, dans le sens qui rejoint la fin de son contour à son
    // début : `P → Q` (moitié de gauche) ou `Q → P` (moitié de droite).
    let half = |range: Vec<(EdgeGeom, usize)>| -> Result<(Vec<CutEdge>, bool), CutError> {
        let mut out: Vec<CutEdge> = range
            .into_iter()
            .map(|(geom, src)| CutEdge {
                geom,
                source: Some(src),
            })
            .collect();
        let (Some(first), Some(last)) = (out.first(), out.last()) else {
            return Err(CutError::TinyHalf);
        };
        let (from, to) = (last.geom.end(), first.geom.start());
        let forward = distance_mm(from, p) <= distance_mm(from, q);
        if (forward && distance_mm(to, q) > 1e-4) || (!forward && distance_mm(to, p) > 1e-4) {
            return Err(CutError::BadChain(
                "incohérence interne : moitié non rejointe par la ligne".into(),
            ));
        }
        out.push(CutEdge {
            geom: if forward { cut } else { cut.reversed() },
            source: None,
        });
        Ok((out, forward))
    };
    let (first_half, first_forward) = half(pieces[i..k].to_vec())?;
    let mut second_range = pieces[k..].to_vec();
    second_range.extend_from_slice(&pieces[..i]);
    let (second_half, second_forward) = half(second_range)?;
    if first_forward == second_forward {
        return Err(CutError::BadChain(
            "incohérence interne : les deux moitiés se ferment dans le même sens".into(),
        ));
    }
    let (left, right) = if first_forward {
        (first_half, second_half)
    } else {
        (second_half, first_half)
    };
    let geoms = |h: &[CutEdge]| -> Vec<EdgeGeom> { h.iter().map(|e| e.geom).collect() };
    let (area_l, area_r) = (
        chain_signed_area_mm2(&geoms(&left)),
        chain_signed_area_mm2(&geoms(&right)),
    );
    if area_l < -MIN_HALF_AREA_MM2 || area_r < -MIN_HALF_AREA_MM2 {
        return Err(CutError::OutsidePiece);
    }
    if area_l <= MIN_HALF_AREA_MM2 || area_r <= MIN_HALF_AREA_MM2 {
        return Err(CutError::TinyHalf);
    }
    let total = chain_signed_area_mm2(chain);
    if (area_l + area_r - total).abs() > AREA_RELATIVE_TOLERANCE * total.abs().max(1.0) {
        return Err(CutError::BadChain(
            "incohérence interne : la somme des aires des moitiés diffère de l'aire du contour"
                .into(),
        ));
    }
    Ok(ChainCut {
        left,
        right,
        points: [p, q],
    })
}

/// Déplacement rigide du plan (rotation puis translation, sans symétrie).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rigid {
    cos: f64,
    sin: f64,
    from: PointMm,
    to: PointMm,
}

impl Rigid {
    /// Identité.
    pub fn identity() -> Rigid {
        Rigid {
            cos: 1.0,
            sin: 0.0,
            from: PointMm::ORIGIN,
            to: PointMm::ORIGIN,
        }
    }

    /// Déplacement qui amène le **milieu** du segment `b0 → b1` sur le milieu de
    /// `t0 → t1` et tourne `b0 → b1` dans la direction de `t0 → t1`. Si les deux
    /// segments ont la même longueur, `b0` tombe sur `t0` et `b1` sur `t1`.
    /// Erreur si l'un des segments est de longueur nulle.
    pub fn between_segments(
        b0: PointMm,
        b1: PointMm,
        t0: PointMm,
        t1: PointMm,
    ) -> Result<Rigid, GeometryError> {
        let (vb, vt) = ((b1 - b0).unit()?, (t1 - t0).unit()?);
        Ok(Rigid {
            cos: vb.dot(vt),
            sin: vb.cross(vt),
            from: b0.lerp(b1, 0.5),
            to: t0.lerp(t1, 0.5),
        })
    }

    /// Déplacement qui amène `b0` exactement sur `t0` et tourne `b0 → b1` dans la direction de
    /// `t0 → t1` (US-08.3, vue assemblée). Erreur si l'un des segments est de longueur nulle.
    pub fn between_starts(
        b0: PointMm,
        b1: PointMm,
        t0: PointMm,
        t1: PointMm,
    ) -> Result<Rigid, GeometryError> {
        let (vb, vt) = ((b1 - b0).unit()?, (t1 - t0).unit()?);
        Ok(Rigid {
            cos: vb.dot(vt),
            sin: vb.cross(vt),
            from: b0,
            to: t0,
        })
    }

    /// Angle de rotation (radians, `]-π ; π]`).
    pub fn angle_rad(&self) -> f64 {
        self.sin.atan2(self.cos)
    }

    pub fn apply(&self, p: PointMm) -> PointMm {
        let (dx, dy) = (p.x - self.from.x, p.y - self.from.y);
        PointMm::new(
            self.to.x + self.cos * dx - self.sin * dy,
            self.to.y + self.sin * dx + self.cos * dy,
        )
    }

    pub fn apply_edge(&self, e: &EdgeGeom) -> EdgeGeom {
        match e {
            EdgeGeom::Line(a, b) => EdgeGeom::Line(self.apply(*a), self.apply(*b)),
            EdgeGeom::Cubic(c) => EdgeGeom::Cubic(CubicBezier::new(
                self.apply(c.p0),
                self.apply(c.c1),
                self.apply(c.c2),
                self.apply(c.p3),
            )),
            EdgeGeom::Arc(a) => EdgeGeom::Arc(Arc {
                center: self.apply(a.center),
                radius_mm: a.radius_mm,
                start_rad: a.start_rad + self.angle_rad(),
                sweep_rad: a.sweep_rad,
            }),
        }
    }
}

/// Distance d'un point à une suite d'arêtes (mm).
fn distance_to_edges(p: PointMm, edges: &[EdgeGeom]) -> f64 {
    edges
        .iter()
        .map(|e| trace_nearest_param(&edge_trace(e), p).1)
        .fold(f64::INFINITY, f64::min)
}

/// Écart de forme entre deux bords (distance de Hausdorff échantillonnée, en mm) :
/// plus grande distance d'un point de l'un à l'autre, dans les deux sens. Deux bords
/// superposés donnent 0.
pub fn max_deviation_mm(a: &[EdgeGeom], b: &[EdgeGeom]) -> f64 {
    const STEPS: usize = 48;
    let sample = |edges: &[EdgeGeom]| -> Vec<PointMm> {
        let mut pts = Vec::new();
        for e in edges {
            pts.extend(edge_sample(e, STEPS));
            if let EdgeGeom::Line(p, q) = e {
                for k in 1..8 {
                    pts.push(p.lerp(*q, k as f64 / 8.0));
                }
            }
        }
        pts
    };
    let one_way = |from: &[EdgeGeom], to: &[EdgeGeom]| {
        sample(from)
            .into_iter()
            .map(|p| distance_to_edges(p, to))
            .fold(0.0_f64, f64::max)
    };
    one_way(a, b).max(one_way(b, a))
}

#[cfg(test)]
#[path = "split_tests.rs"]
mod tests;
