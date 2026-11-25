//! Import de pièces d'un DXF de type AAMA / ASTM (US-12.2) : `Command::ImportAamaPieces` crée,
//! **en une seule étape annulable**, une pièce **détachée** par entrée, sur sa propre feuille :
//! contour (segments ordinaires), crans posés sur le contour et droit-fil. Aucun format de
//! fichier n'est connu ici (le lecteur vit dans `pf-export`) : la commande reçoit des pièces en
//! mm.
//!
//! Règles :
//!
//! - contour = polygone fermé d'au moins trois sommets distincts, sans doublon consécutif
//!   (le lecteur les retire et les compte) ; chaque côté est un segment de rôle coupe ou couture ;
//! - un cran est donné par son pied et sa pointe ; [`locate_notch`] (partagée avec le lecteur,
//!   donc un seul critère) trouve le côté du contour qui porte le pied à
//!   [`NOTCH_FOOT_TOLERANCE_MM`] près, la distance depuis le départ du côté et le côté
//!   (gauche / droite) de la pointe ; un cran qui ne tient à aucun côté **refuse** la commande ;
//! - droit-fil = un segment de rôle droit-fil de longueur non nulle ;
//! - valeur non finie ou hors de l'espace de travail, liste vide ou trop longue : **refus** de
//!   toute la commande, rien n'est modifié (`Project::apply` est atomique).

use crate::command::Outcome;
use crate::error::CoreError;
use crate::model::*;
use pf_geometry::{PointMm, Side};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Nombre maximal de pièces d'un import.
pub const MAX_AAMA_PIECES: usize = 500;
/// Nombre maximal de sommets d'un contour importé (borne le coût de la validation).
pub const MAX_AAMA_VERTICES: usize = 20_000;
/// Nombre maximal de crans par pièce.
pub const MAX_AAMA_NOTCHES: usize = 2_000;
/// Distance maximale (mm) du pied d'un cran au contour.
pub const NOTCH_FOOT_TOLERANCE_MM: f64 = 0.05;
/// Longueur minimale (mm) d'un cran, d'un côté ou du droit-fil.
const MIN_LENGTH_MM: f64 = 1e-6;
const MAX_TEXT_CHARS: usize = 200;

/// Un cran lu : pied (sur le bord) et pointe.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AamaNotch {
    pub foot: PointMm,
    pub tip: PointMm,
}

/// Une pièce à importer, en mm (repère du fichier).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AamaPiece {
    pub name: String,
    #[serde(default)]
    pub code: Option<String>,
    pub quantity: u32,
    #[serde(default)]
    pub material: Option<String>,
    #[serde(default)]
    pub mirrored_pair: bool,
    /// Sommets du contour, sans répéter le premier à la fin.
    pub outline: Vec<PointMm>,
    /// Vrai : le contour vient de la ligne de couture (rôle couture), sinon de la coupe.
    #[serde(default)]
    pub sewing: bool,
    #[serde(default)]
    pub notches: Vec<AamaNotch>,
    #[serde(default)]
    pub grain: Option<(PointMm, PointMm)>,
}

/// Position d'un cran sur un contour.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LocatedNotch {
    /// Indice du côté (de `outline[edge]` à `outline[edge + 1]`, le dernier se referme).
    pub edge: usize,
    pub distance_mm: f64,
    pub side: Side,
    pub length_mm: f64,
}

fn dist(a: PointMm, b: PointMm) -> f64 {
    pf_geometry::distance_mm(a, b)
}

/// Distance du point `p` au côté `a`-`b` et abscisse de sa projection (bornée au côté).
fn project(p: PointMm, a: PointMm, b: PointMm) -> (f64, f64) {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let len2 = dx * dx + dy * dy;
    if len2 <= 0.0 {
        return (dist(p, a), 0.0);
    }
    let t = (((p.x - a.x) * dx + (p.y - a.y) * dy) / len2).clamp(0.0, 1.0);
    let q = PointMm::new(a.x + t * dx, a.y + t * dy);
    (dist(p, q), t * len2.sqrt())
}

/// Place un cran (pied, pointe) sur un contour fermé. `None` si les nombres ne sont pas finis,
/// si le cran est de longueur nulle ou si aucun côté ne porte son pied ou sa pointe à
/// [`NOTCH_FOOT_TOLERANCE_MM`] près (le plus proche l'emporte ; le pied est celui des deux bouts
/// qui touche le contour, à égalité le premier).
pub fn locate_notch(outline: &[PointMm], a: PointMm, b: PointMm) -> Option<LocatedNotch> {
    if outline.len() < 3 || ![a.x, a.y, b.x, b.y].iter().all(|v| v.is_finite()) {
        return None;
    }
    let length_mm = dist(a, b);
    if length_mm <= MIN_LENGTH_MM {
        return None;
    }
    let n = outline.len();
    let nearest = |p: PointMm| -> Option<(f64, usize, f64)> {
        (0..n)
            .map(|i| {
                let (d, along) = project(p, outline[i], outline[(i + 1) % n]);
                (d, i, along)
            })
            .filter(|(d, _, _)| *d <= NOTCH_FOOT_TOLERANCE_MM)
            .min_by(|x, y| x.0.total_cmp(&y.0).then(x.1.cmp(&y.1)))
    };
    let (foot, tip, (_, edge, distance_mm)) = match (nearest(a), nearest(b)) {
        (Some(fa), Some(fb)) if fb.0 < fa.0 => (b, a, fb),
        (Some(fa), _) => (a, b, fa),
        (None, Some(fb)) => (b, a, fb),
        (None, None) => return None,
    };
    let (p, q) = (outline[edge], outline[(edge + 1) % n]);
    let cross = (q.x - p.x) * (tip.y - foot.y) - (q.y - p.y) * (tip.x - foot.x);
    let side = if cross >= 0.0 {
        Side::Left
    } else {
        Side::Right
    };
    Some(LocatedNotch {
        edge,
        distance_mm,
        side,
        length_mm,
    })
}

fn check_text(s: &str, what: &str) -> Result<(), CoreError> {
    if s.chars().count() > MAX_TEXT_CHARS || s.chars().any(char::is_control) {
        return Err(CoreError::Invalid(format!(
            "import : {what} invalide (contrôle ou plus de {MAX_TEXT_CHARS} caractères)"
        )));
    }
    Ok(())
}

fn check_piece(p: &AamaPiece) -> Result<(), CoreError> {
    let bad = |m: String| CoreError::Invalid(format!("import « {} » : {m}", p.name));
    if p.name.trim().is_empty() {
        return Err(CoreError::Invalid("import : nom de pièce vide".into()));
    }
    check_text(&p.name, "le nom")?;
    for (v, w) in [(&p.code, "le code"), (&p.material, "la matière")] {
        if let Some(v) = v {
            check_text(v, w)?;
        }
    }
    if p.quantity == 0 || p.quantity > 100_000 {
        return Err(bad("quantité hors de 1 à 100000".into()));
    }
    let n = p.outline.len();
    if n < 3 {
        return Err(bad("un contour a au moins trois sommets".into()));
    }
    if n > MAX_AAMA_VERTICES {
        return Err(bad(format!(
            "{n} sommets, plus que la limite de {MAX_AAMA_VERTICES}"
        )));
    }
    for v in &p.outline {
        v.validate()?;
    }
    for i in 0..n {
        if dist(p.outline[i], p.outline[(i + 1) % n]) <= MIN_LENGTH_MM {
            return Err(bad(format!("côté {} de longueur nulle", i + 1)));
        }
    }
    if p.notches.len() > MAX_AAMA_NOTCHES {
        return Err(bad(format!("plus de {MAX_AAMA_NOTCHES} crans")));
    }
    for (k, c) in p.notches.iter().enumerate() {
        c.foot.validate()?;
        c.tip.validate()?;
        if locate_notch(&p.outline, c.foot, c.tip).is_none() {
            return Err(bad(format!("le cran {} ne touche pas le contour", k + 1)));
        }
    }
    if let Some((a, b)) = p.grain {
        a.validate()?;
        b.validate()?;
        if dist(a, b) <= MIN_LENGTH_MM {
            return Err(bad("droit-fil de longueur nulle".into()));
        }
    }
    Ok(())
}

fn label(i: usize) -> String {
    let c = (b'A' + (i % 26) as u8) as char;
    match i / 26 {
        0 => c.to_string(),
        r => format!("{c}{r}"),
    }
}

impl Project {
    pub(crate) fn import_aama_pieces(
        &mut self,
        pieces: Vec<AamaPiece>,
        out: &mut Outcome,
    ) -> Result<(), CoreError> {
        if pieces.is_empty() {
            return Err(CoreError::Invalid(
                "import : aucune pièce à importer".into(),
            ));
        }
        if pieces.len() > MAX_AAMA_PIECES {
            return Err(CoreError::Invalid(format!(
                "import : {} pièces, plus que la limite de {MAX_AAMA_PIECES} par import",
                pieces.len()
            )));
        }
        for p in &pieces {
            check_piece(p)?;
        }
        let mut taken: HashSet<String> = self.sheets.iter().map(|s| s.name.clone()).collect();
        for p in pieces {
            let mut sheet_name = p.name.trim().to_string();
            let mut k = 1;
            while taken.contains(&sheet_name) {
                k += 1;
                sheet_name = format!("{} ({k})", p.name.trim());
            }
            taken.insert(sheet_name.clone());
            let sid = SheetId(self.alloc_id());
            let mut sheet = Sheet::new(sid, &sheet_name);
            let n = p.outline.len();
            let mut point_ids = Vec::with_capacity(n);
            for (i, v) in p.outline.iter().enumerate() {
                let id = PointId(self.alloc_id());
                sheet.points.push(Point {
                    id,
                    label: label(i),
                    definition: PointDefinition::Free { x: v.x, y: v.y },
                    role: PointRole::Construction,
                    layer: None,
                });
                point_ids.push(id);
                out.points.push(id);
            }
            let role = if p.sewing {
                PathRole::SewingContour
            } else {
                PathRole::CuttingContour
            };
            let mut edges = Vec::with_capacity(n);
            let mut edge_ids = Vec::with_capacity(n);
            for i in 0..n {
                let id = EntityId(self.alloc_id());
                sheet.entities.push(Entity {
                    id,
                    role,
                    layer: None,
                    name: None,
                    kind: EntityKind::Segment {
                        from: point_ids[i],
                        to: point_ids[(i + 1) % n],
                        construction: None,
                    },
                });
                edges.push(PieceEdge {
                    entity: id,
                    reversed: false,
                });
                edge_ids.push(id);
                out.entities.push(id);
            }
            for c in &p.notches {
                let Some(l) = locate_notch(&p.outline, c.foot, c.tip) else {
                    continue;
                };
                let id = NotchId(self.alloc_id());
                sheet.notches.push(Notch {
                    id,
                    entity: edge_ids[l.edge],
                    distance_mm: l.distance_mm,
                    side: l.side,
                    length_mm: l.length_mm,
                    style: NotchStyle::Single,
                    anchor: None,
                    form: NotchForm::Line,
                    width_mm: None,
                    on_cut: false,
                    corner: false,
                    proportional: false,
                });
                out.notches.push(id);
            }
            let mut grainline = None;
            if let Some((a, b)) = p.grain {
                let mut ends = [PointId(0); 2];
                for (slot, at) in ends.iter_mut().zip([a, b]) {
                    let id = PointId(self.alloc_id());
                    sheet.points.push(Point {
                        id,
                        label: label(sheet.points.len()),
                        definition: PointDefinition::Free { x: at.x, y: at.y },
                        role: PointRole::Construction,
                        layer: None,
                    });
                    out.points.push(id);
                    *slot = id;
                }
                let id = EntityId(self.alloc_id());
                sheet.entities.push(Entity {
                    id,
                    role: PathRole::Grainline,
                    layer: None,
                    name: None,
                    kind: EntityKind::Segment {
                        from: ends[0],
                        to: ends[1],
                        construction: None,
                    },
                });
                out.entities.push(id);
                grainline = Some(id);
            }
            self.sheets.push(sheet);
            let pid = PieceId(self.alloc_id());
            self.pieces.push(Piece {
                id: pid,
                name: p.name.trim().to_string(),
                code: p.code.filter(|c| !c.trim().is_empty()),
                quantity: p.quantity,
                material: p.material.filter(|m| !m.trim().is_empty()),
                mirrored_pair: p.mirrored_pair,
                outline: edges,
                grainline,
                ..Piece::default()
            });
            out.pieces.push(pid);
            out.sheet = Some(sid);
        }
        Ok(())
    }
}
