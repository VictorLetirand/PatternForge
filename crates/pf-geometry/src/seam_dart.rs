//! Contour de coupe « pince fermée » (PH-5a).
//!
//! En atelier, la marge au droit d'une pince débouchant sur un bord se trace **pince fermée** :
//! on plie la pince (les deux jambes se rejoignent), on trace la marge sur le bord ainsi plié,
//! puis on rouvre. Une fois la pince cousue, la ligne de coupe est alors continue.
//!
//! # Méthode (par pince, les autres restant ouvertes)
//!
//! 1. Le contour est coupé aux deux pieds de la pince. La **base** (entre les pieds) disparaît de
//!    la coupe : elle est cousue dans la pince.
//! 2. **Fermeture** : le côté mobile (le bord qui suit la base si le premier pied est le pied
//!    fixe, sinon celui qui la précède) tourne autour du sommet de l'angle de fermeture de
//!    `close_dart` (même rotation). Le sommet fermé entre les deux bords est traité par le moteur
//!    de coins de `seam_contour` (onglet, coupe d'un coin rentrant…) avec les marges des deux
//!    bords : on obtient la coupe du contour fermé.
//! 3. **Réouverture** : les morceaux de coupe du côté mobile subissent la rotation inverse. Les
//!    deux extrémités de coupe ne coïncident plus ; elles sont reliées par un segment droit (la
//!    « pointe de marge de pince », jamais de trou). Si le bord est droit et la marge uniforme,
//!    tous ces points restent sur la droite décalée : la coupe est géométriquement la même, avec
//!    deux sommets en plus à la bouche ; sur un bord courbe ou une marge qui varie, la coupe
//!    diffère.
//!
//! Le calcul est local à chaque bouche : une pince ne regarde que ses deux bords voisins. Les
//! pinces sont fermées une à une (jamais ensemble). Impossible (bouche sur un coin, bouches qui
//! se touchent, bord trop court pour la marge, coupe qui reviendrait sur elle-même…) : erreur
//! expliquée, l'appelant retombe sur la coupe ordinaire.

use super::*;
use crate::dart::{rot_edge, DartShape, DART_FOOT_TOLERANCE_MM};
use crate::region::trace_nearest_param;
use crate::Side;
use std::collections::HashSet;

/// Un pied plus proche que cela (fraction du paramètre) d'une extrémité d'arête est à son sommet.
const VERTEX_EPS: f64 = 1e-9;
/// Une arête plus courte que cela (mm), après découpe aux pieds, est refusée.
const MIN_EDGE_MM: f64 = 1e-6;

/// Pince à fermer pour le calcul de la coupe.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DartClosing {
    /// Forme calculée de la pince (sommet, pieds).
    pub shape: DartShape,
    /// Jambe qui reste en place à la fermeture.
    pub fixed: Side,
    /// Première arête (de la chaîne fermée) du tracé qui porte la base, et leur nombre : les deux
    /// pieds sont sur ces arêtes, la base est entre eux dans le sens du parcours.
    pub carrier_start: usize,
    pub carrier_len: usize,
}

fn fail(msg: impl Into<String>) -> GeometryError {
    GeometryError::InvalidConstruction(msg.into())
}

/// Pied situé sur la chaîne d'origine : arête globale et paramètre (0 = sommet).
#[derive(Clone, Copy)]
struct Foot {
    /// Rang le long du tracé porteur (pour l'ordre des pieds).
    off: usize,
    edge: usize,
    t: f64,
}

fn locate_foot(chain: &[EdgeGeom], d: &DartClosing, p: PointMm) -> Result<Foot> {
    let n = chain.len();
    let mut best = (0usize, 0.0f64, f64::INFINITY);
    for off in 0..d.carrier_len.min(n) {
        let i = (d.carrier_start + off) % n;
        let (t, dist) = trace_nearest_param(&edge_trace(&chain[i]), p);
        if dist < best.2 - 1e-12 {
            best = (off, t, dist);
        }
    }
    if best.2 > DART_FOOT_TOLERANCE_MM {
        return Err(fail(format!(
            "un pied de la pince est à {:.3} mm du bord qui la porte",
            best.2
        )));
    }
    let (mut off, mut t) = (best.0, best.1);
    if t >= 1.0 - VERTEX_EPS {
        off += 1;
        t = 0.0;
    } else if t <= VERTEX_EPS {
        t = 0.0;
    }
    Ok(Foot {
        off,
        edge: (d.carrier_start + off) % n,
        t,
    })
}

/// Marge d'un morceau `[t0 ; t1]` d'une arête : interpolée pour un segment, constante sinon.
fn split_margin(g: &EdgeGeom, m: EdgeMargin, t0: f64, t1: f64) -> EdgeMargin {
    if !matches!(g, EdgeGeom::Line(..)) {
        return m;
    }
    let at = |t: f64| {
        if t <= 0.0 {
            m.start_mm
        } else if t >= 1.0 {
            m.end_mm
        } else {
            m.start_mm + (m.end_mm - m.start_mm) * t
        }
    };
    EdgeMargin {
        start_mm: at(t0),
        end_mm: at(t1),
    }
}

/// Fermeture calculée d'une pince : morceaux de coupe à la bouche, dans le repère d'origine.
struct Closing {
    /// Sommet (de la chaîne découpée) du premier pied, du second pied.
    a: usize,
    b: usize,
    prev_end: Option<(usize, f64)>,
    next_start: Option<(usize, f64)>,
    prev_pieces: Vec<EdgeGeom>,
    next_pieces: Vec<EdgeGeom>,
    /// Extrémité de la coupe côté arête précédente, et début de la coupe côté arête suivante.
    from: PointMm,
    to: PointMm,
}

/// Contour de coupe d'un contour de couture dont les pinces `darts` sont fermées à leur bouche
/// (voir la documentation du module). `chain`, `margins` et `options` : comme
/// [`seam_contour_with`], dont toutes les erreurs s'appliquent aussi ici. Erreur expliquée si une
/// pince n'est pas portée par un bord calculable, si une bouche est sur un coin ou touche une
/// autre bouche, si un bord est trop court pour la marge, ou si la coupe reviendrait sur elle-même.
pub fn seam_contour_closed_darts(
    chain: &[EdgeGeom],
    margins: &[EdgeMargin],
    options: &SeamOptions,
    darts: &[DartClosing],
) -> Result<Vec<Trace>> {
    let plain = seam_contour_with(chain, margins, options)?;
    let all_zero = margins.iter().all(|m| m.start_mm == 0.0 && m.end_mm == 0.0);
    if darts.is_empty() || all_zero {
        return Ok(plain);
    }
    let n = chain.len();
    // Pieds, dans l'ordre du parcours le long du tracé porteur.
    struct Placed {
        first: Foot,
        second: Foot,
        moving_second: bool,
    }
    let mut placed: Vec<Placed> = Vec::new();
    for d in darts {
        if d.carrier_len == 0 || d.carrier_start >= n {
            return Err(fail("le bord qui porte la pince est introuvable"));
        }
        let left = locate_foot(chain, d, d.shape.left_foot)?;
        let right = locate_foot(chain, d, d.shape.right_foot)?;
        let key = |f: &Foot| (f.off, f.t);
        let (first, second, first_side) = match key(&left).partial_cmp(&key(&right)) {
            Some(std::cmp::Ordering::Less) => (left, right, Side::Left),
            Some(std::cmp::Ordering::Greater) => (right, left, Side::Right),
            _ => return Err(fail("les deux pieds de la pince sont confondus")),
        };
        placed.push(Placed {
            first,
            second,
            moving_second: first_side == d.fixed,
        });
    }
    // Un pied sur un sommet n'est accepté que si le contour y est lisse (jamais un coin).
    for f in placed.iter().flat_map(|p| [p.first, p.second]) {
        if f.t == 0.0 && vertex_turn(chain, f.edge)? != VertexTurn::Smooth {
            return Err(fail(
                "la bouche de la pince est sur un coin du contour : la marge pince fermée n'y est \
                 pas définie",
            ));
        }
    }
    // Découpe de la chaîne aux pieds (marges interpolées sur un segment).
    let mut cuts: Vec<Vec<f64>> = vec![Vec::new(); n];
    for f in placed.iter().flat_map(|p| [p.first, p.second]) {
        if f.t > 0.0 {
            cuts[f.edge].push(f.t);
        }
    }
    let mut chain2: Vec<EdgeGeom> = Vec::new();
    let mut margins2: Vec<EdgeMargin> = Vec::new();
    let mut first_new = vec![0usize; n];
    for i in 0..n {
        first_new[i] = chain2.len();
        cuts[i].sort_by(f64::total_cmp);
        if cuts[i].windows(2).any(|w| w[1] - w[0] <= VERTEX_EPS) {
            return Err(fail("deux bouches de pince se touchent"));
        }
        let mut bounds = vec![0.0];
        bounds.extend(cuts[i].iter().copied());
        bounds.push(1.0);
        for w in bounds.windows(2) {
            let piece = edge_portion(&chain[i], w[0], w[1]);
            if piece.length_mm() <= MIN_EDGE_MM {
                return Err(fail(
                    "une bouche de pince est trop près d'un sommet ou d'une autre bouche",
                ));
            }
            chain2.push(piece);
            margins2.push(split_margin(&chain[i], margins[i], w[0], w[1]));
        }
    }
    let m = chain2.len();
    let vertex_of = |f: Foot| -> usize {
        if f.t == 0.0 {
            first_new[f.edge]
        } else {
            let k = cuts[f.edge]
                .iter()
                .position(|c| (*c - f.t).abs() <= VERTEX_EPS)
                .unwrap_or(0);
            first_new[f.edge] + k + 1
        }
    };
    let spans: Vec<(usize, usize)> = placed
        .iter()
        .map(|p| (vertex_of(p.first), vertex_of(p.second)))
        .collect();
    // Bouches distinctes, bases disjointes.
    let mut seen = HashSet::new();
    let mut in_base = vec![false; m];
    for &(a, b) in &spans {
        if !seen.insert(a) || !seen.insert(b) {
            return Err(fail("deux bouches de pince se touchent"));
        }
        let mut v = a;
        while v != b {
            if in_base[v] {
                return Err(fail("deux pinces se chevauchent sur le même bord"));
            }
            in_base[v] = true;
            v = (v + 1) % m;
        }
    }
    if in_base.iter().all(|b| *b) {
        return Err(fail("la base de la pince couvre tout le contour"));
    }
    // Sommets où le moteur ordinaire ne passe pas : de la bouche à la bouche (inclus).
    let mut skip = vec![false; m];
    for &(a, b) in &spans {
        let mut v = a;
        loop {
            skip[v] = true;
            if v == b {
                break;
            }
            v = (v + 1) % m;
        }
    }
    let mapped = SeamOptions {
        corners: options
            .corners
            .iter()
            .map(|c| CornerSpec {
                vertex: first_new[c.vertex],
                kind: c.kind,
            })
            .collect(),
        max_tip_mm: options.max_tip_mm,
    };
    let opts = OffsetOptions {
        miter_limit: SEAM_MITER_LIMIT,
        ..OffsetOptions::default()
    };
    let offs: Vec<Vec<EdgeGeom>> = chain2
        .iter()
        .zip(&margins2)
        .map(|(g, mg)| offset_edge(g, *mg, &opts))
        .collect::<Result<_>>()?;
    // Coins ordinaires.
    let mut joins: Vec<Option<Join>> = Vec::with_capacity(m);
    for j in 0..m {
        if skip[j] {
            joins.push(None);
            continue;
        }
        let p = (j + m - 1) % m;
        joins.push(Some(join(
            &chain2[p],
            &chain2[j],
            &offs[p],
            &offs[j],
            margins2[p].end_mm,
            margins2[j].start_mm,
            mapped
                .corners
                .iter()
                .find(|c| c.vertex == j)
                .map_or(CornerKind::Mitre, |c| c.kind),
            mapped.max_tip_mm,
        )?));
    }
    // Fermeture de chaque pince.
    let mut closings: Vec<Closing> = Vec::new();
    for (k, d) in darts.iter().enumerate() {
        let (a, b) = spans[k];
        let (fixed_foot, moving_foot) = (d.shape.foot(d.fixed), d.shape.foot(d.fixed.flipped()));
        let (va, vb) = (moving_foot - d.shape.apex, fixed_foot - d.shape.apex);
        let norm = va.norm() * vb.norm();
        if !norm.is_finite() || norm <= 0.0 {
            return Err(fail(
                "pince dégénérée : un pied est confondu avec le sommet",
            ));
        }
        let (cos, sin) = (va.dot(vb) / norm, va.cross(vb) / norm);
        let apex = d.shape.apex;
        let fwd = |g: &EdgeGeom| rot_edge(g, apex, cos, sin);
        let back = |g: &EdgeGeom| rot_edge(g, apex, cos, -sin);
        let moving_second = placed[k].moving_second;
        let (e1, e2) = ((a + m - 1) % m, b % m);
        let (prev, prev_off, next, next_off): (EdgeGeom, Vec<EdgeGeom>, EdgeGeom, Vec<EdgeGeom>) =
            if moving_second {
                (
                    chain2[e1],
                    offs[e1].clone(),
                    fwd(&chain2[e2]),
                    offs[e2].iter().map(fwd).collect(),
                )
            } else {
                (
                    fwd(&chain2[e1]),
                    offs[e1].iter().map(fwd).collect(),
                    chain2[e2],
                    offs[e2].clone(),
                )
            };
        let closed = join(
            &prev,
            &next,
            &prev_off,
            &next_off,
            margins2[e1].end_mm,
            margins2[e2].start_mm,
            CornerKind::Mitre,
            mapped.max_tip_mm,
        )
        .map_err(|e| fail(format!("pince fermée : {e}")))?;
        let (prev_end, next_start, pieces) = match closed {
            Join::None => (None, None, Vec::new()),
            Join::Corner {
                prev_end,
                next_start,
                pieces,
            } => (prev_end, next_start, pieces),
        };
        // Un morceau qui prolonge le décalage précédent reste avec lui, les autres suivent le
        // décalage suivant ; chacun revient dans le repère d'origine s'il est du côté mobile.
        let last_end = prev_off.last().map_or(PointMm::new(0.0, 0.0), |g| g.end());
        let (mut prev_pieces, mut next_pieces) = (Vec::new(), Vec::new());
        for (i, p) in pieces.iter().enumerate() {
            if i == 0 && distance_mm(p.start(), last_end) <= JOIN_MM {
                prev_pieces.push(*p);
            } else {
                next_pieces.push(*p);
            }
        }
        if moving_second {
            next_pieces = next_pieces.iter().map(back).collect();
        } else {
            prev_pieces = prev_pieces.iter().map(back).collect();
        }
        let end_of_prev = match prev_end {
            Some((i, t)) => edge_portion(&offs[e1][i], 0.0, t),
            None => *offs[e1].last().ok_or_else(|| fail("décalage vide"))?,
        };
        let start_of_next = match next_start {
            Some((i, t)) => edge_portion(&offs[e2][i], t, 1.0),
            None => *offs[e2].first().ok_or_else(|| fail("décalage vide"))?,
        };
        let from = prev_pieces.last().map_or(end_of_prev.end(), |g| g.end());
        let to = next_pieces
            .first()
            .map_or(start_of_next.start(), |g| g.start());
        // La coupe rouverte ne doit jamais revenir sur elle-même à la bouche.
        let gap = to - from;
        if gap.norm() > JOIN_MM {
            let u = gap.unit()?;
            let t_in = prev_pieces
                .last()
                .map_or_else(|| end_dir(&end_of_prev), end_dir)?;
            let t_out = next_pieces
                .first()
                .map_or_else(|| start_dir(&start_of_next), start_dir)?;
            if u.dot(t_in) < -1e-6 || u.dot(t_out) < -1e-6 {
                return Err(fail(
                    "pince fermée : la coupe rouverte reviendrait sur elle-même à la bouche",
                ));
            }
        }
        closings.push(Closing {
            a,
            b,
            prev_end,
            next_start,
            prev_pieces,
            next_pieces,
            from,
            to,
        });
    }
    // Assemblage, en partant de la bouche de la première pince (la coupe commence après sa
    // pointe) ; `breaks` = débuts de morceaux à ne pas fusionner (la pointe reste visible).
    let mut out: Vec<EdgeGeom> = Vec::new();
    let mut breaks: Vec<usize> = Vec::new();
    let i0 = spans[0].1 % m;
    for step in 0..m {
        let i = (i0 + step) % m;
        if in_base[i] {
            continue;
        }
        let (mut start, mut end) = (None, None);
        if let Some(c) = closings.iter().find(|c| c.b % m == i) {
            out.extend(c.prev_pieces.iter().copied());
            if distance_mm(c.from, c.to) > JOIN_MM {
                breaks.push(out.len());
                out.push(EdgeGeom::Line(c.from, c.to));
                breaks.push(out.len());
            }
            out.extend(c.next_pieces.iter().copied());
            start = c.next_start;
        } else if let Some(Join::Corner {
            next_start, pieces, ..
        }) = &joins[i]
        {
            out.extend(pieces.iter().copied());
            start = *next_start;
        }
        let nv = (i + 1) % m;
        if let Some(c) = closings.iter().find(|c| c.a == nv) {
            end = c.prev_end;
        } else if let Some(Join::Corner { prev_end, .. }) = &joins[nv] {
            end = *prev_end;
        }
        out.extend(slice(&offs[i], start, end).map_err(|e| fail(format!("pince fermée : {e}")))?);
    }
    // Fusion des segments alignés, sans jamais effacer la pointe de la bouche.
    let mut chunks: Vec<Vec<EdgeGeom>> = Vec::new();
    let mut from = 0;
    for &at in &breaks {
        chunks.push(out[from..at].to_vec());
        from = at;
    }
    chunks.push(out[from..].to_vec());
    if chunks.len() > 1 {
        let head = chunks.remove(0);
        if let Some(tail) = chunks.last_mut() {
            tail.extend(head);
        }
    }
    let mut edges: Vec<EdgeGeom> = Vec::new();
    for mut c in chunks {
        merge_collinear(&mut c);
        edges.extend(c);
    }
    let before = edges.len();
    edges.retain(|e| !matches!(e, EdgeGeom::Line(a, b) if distance_mm(*a, *b) < NEGLIGIBLE_MM));
    if edges.len() < before && edges.len() < 3 {
        return Err(fail("contour de coupe dégénéré"));
    }
    check_result(&edges)?;
    Ok(edges.iter().map(edge_trace).collect())
}
