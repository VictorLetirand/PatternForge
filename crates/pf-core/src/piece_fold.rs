//! PH-5b — bords « sur le pli » d'une pièce (demi-patron tracé contre le milieu devant d'une jupe).
//!
//! `Piece.on_fold` liste des **tracés rectilignes du contour** (références stables, triées par
//! identifiant). Le rôle de tracé `PathRole::Fold` ne convient pas : il décrit une ligne dessinée
//! (pli interne, ligne de dépliage), il ne dit rien du contour et change le rôle de couture ou de
//! coupe du tracé. Ici la déclaration est une propriété de la **pièce**, annulable et refusable
//! (bord inconnu, non rectiligne, hors contour, pièce dérivée liée).
//!
//! Conventions (à faire valider par une modéliste, voir le suivi) :
//!
//! - un bord sur le pli a une **marge de couture nulle** : la marge réglée sur ce bord (surcharge,
//!   ourlet, rempli) est **ignorée tant que le bord est sur le pli** (jamais effacée : elle revient
//!   si on retire la déclaration) et un contrôle le signale ; le coin avec le bord voisin est
//!   traité par le moteur de coins existant, comme un bord de marge 0 ;
//! - dans le contour de coupe calculé, la portion portée par le bord n'est **pas un bord de coupe** :
//!   elle suit la pliure, prolongée jusqu'aux marges voisines (le contour reste fermé pour l'aire
//!   et le périmètre, mais les exports écrivent cette portion comme pliure, pas comme coupe) ;
//! - rien n'est stocké d'autre que la liste des tracés ; longueur et portion de coupe sont lues.

use crate::command::Outcome;
use crate::error::CoreError;
use crate::model::*;
use pf_geometry::{distance_mm, PointMm, Trace};

/// Bord déclaré sur le pli, lu sur la construction : extrémités dans le sens **anti-horaire** du
/// contour (l'intérieur de la pièce est à gauche de `from → to`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FoldEdge {
    pub entity: EntityId,
    pub from: PointMm,
    pub to: PointMm,
    pub length_mm: f64,
}

/// Résumé pour la fiche et le rapport : nombre de bords sur le pli et longueur cumulée.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct FoldSummary {
    pub count: usize,
    pub length_mm: f64,
}

/// Portion du contour de coupe calculé portée par un bord sur le pli : `traces[trace]` du
/// [`crate::piece_seam::CutContour`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FoldCut {
    pub entity: EntityId,
    pub trace: usize,
}

/// Ligne de pliure à écrire à l'export : la portion de coupe suivant la pliure quand elle existe,
/// sinon le bord lui-même.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FoldLine {
    pub entity: EntityId,
    pub from: PointMm,
    pub to: PointMm,
}

/// Un bord sur le pli dans la chaîne élémentaire du contour (pour repérer sa portion de coupe).
#[derive(Debug, Clone, Copy)]
pub(crate) struct FoldSpec {
    pub entity: EntityId,
    pub a: PointMm,
    pub b: PointMm,
    /// Le bord précédent (resp. suivant) du contour est un segment dans le prolongement.
    pub prev_collinear: bool,
    pub next_collinear: bool,
}

const ON_LINE_MM: f64 = 1e-6;
const MIN_PIECE_MM: f64 = 1e-6;

fn invalid(msg: impl Into<String>) -> CoreError {
    CoreError::Invalid(msg.into())
}

/// Sépare dans `traces` la portion portée par chaque bord sur le pli : la trace est découpée aux
/// bornes du bord (prolongé par les coins voisins), la partie du milieu est la pliure. Retourne les
/// nouvelles traces (même parcours, même géométrie) et les portions repérées.
pub(crate) fn split_fold_traces(
    traces: Vec<Trace>,
    specs: &[FoldSpec],
) -> (Vec<Trace>, Vec<FoldCut>) {
    let mut out: Vec<(Trace, Option<EntityId>)> = traces.into_iter().map(|t| (t, None)).collect();
    for spec in specs {
        let ab = spec.b - spec.a;
        let len = ab.norm();
        if len <= MIN_PIECE_MM {
            continue;
        }
        let u = pf_geometry::VectorMm::new(ab.x / len, ab.y / len);
        // Paramètre (mm depuis `a`) d'un point de la droite, ou `None` s'il en est écarté.
        let along = |p: PointMm| -> Option<f64> {
            let d = p - spec.a;
            (u.cross(d).abs() <= ON_LINE_MM).then(|| u.dot(d))
        };
        let mut best: Option<(usize, f64, f64, f64)> = None;
        for (i, (t, tag)) in out.iter().enumerate() {
            let (Trace::Line(p, q), None) = (t, tag) else {
                continue;
            };
            let (Some(sp), Some(sq)) = (along(*p), along(*q)) else {
                continue;
            };
            let overlap = sq.min(len) - sp.max(0.0);
            if sq > sp && overlap > MIN_PIECE_MM && best.is_none_or(|b| overlap > b.3) {
                best = Some((i, sp, sq, overlap));
            }
        }
        let Some((i, sp, sq, _)) = best else { continue };
        let Trace::Line(p, q) = out[i].0 else {
            continue;
        };
        let lo = if spec.prev_collinear { sp.max(0.0) } else { sp };
        let hi = if spec.next_collinear { sq.min(len) } else { sq };
        let at = |s: f64| spec.a + u * s;
        let lo_pt = if lo <= sp { p } else { at(lo) };
        let hi_pt = if hi >= sq { q } else { at(hi) };
        let mut pieces: Vec<(Trace, Option<EntityId>)> = Vec::new();
        if lo - sp > MIN_PIECE_MM {
            pieces.push((Trace::Line(p, lo_pt), None));
        }
        pieces.push((Trace::Line(lo_pt, hi_pt), Some(spec.entity)));
        if sq - hi > MIN_PIECE_MM {
            pieces.push((Trace::Line(hi_pt, q), None));
        }
        out.splice(i..=i, pieces);
    }
    let cuts = out
        .iter()
        .enumerate()
        .filter_map(|(trace, (_, tag))| tag.map(|entity| FoldCut { entity, trace }))
        .collect();
    (out.into_iter().map(|(t, _)| t).collect(), cuts)
}

impl Project {
    /// Entité du contour d'une pièce : tracé de sa feuille (`None` si absent).
    fn fold_entity_kind(&self, piece: &Piece, entity: EntityId) -> Option<EntityKind> {
        match piece.link {
            PieceLink::Linked { sheet } => self
                .sheet(sheet)
                .ok()
                .and_then(|s| s.entity(entity).ok())
                .map(|e| e.kind.clone()),
            PieceLink::Detached => self
                .sheets
                .iter()
                .find_map(|s| s.entity(entity).ok())
                .map(|e| e.kind.clone()),
        }
    }

    /// Bords déclarés sur le pli qui sont encore des tracés rectilignes du contour, dans l'ordre du
    /// contour. Vide si la pièce est inconnue ou n'en déclare aucun. Lecture pure.
    pub fn piece_fold_edges(&self, piece: PieceId) -> Vec<FoldEdge> {
        let Ok(p) = self.piece(piece) else {
            return Vec::new();
        };
        if p.on_fold.is_empty() {
            return Vec::new();
        }
        let Ok(sheet) = self.piece_sheet(piece) else {
            return Vec::new();
        };
        let clockwise = self.piece_contour(piece).is_ok_and(|c| c.clockwise);
        p.outline
            .iter()
            .filter(|e| p.on_fold.contains(&e.entity))
            .filter_map(|e| {
                let Ok(Shape::Line(a, b)) = sheet.shape(e.entity) else {
                    return None;
                };
                let forward = e.reversed == clockwise;
                let (from, to) = if forward { (a, b) } else { (b, a) };
                Some(FoldEdge {
                    entity: e.entity,
                    from,
                    to,
                    length_mm: distance_mm(from, to),
                })
            })
            .collect()
    }

    /// Nombre et longueur cumulée des bords sur le pli d'une pièce (fiche, rapport).
    pub fn piece_fold_summary(&self, piece: PieceId) -> FoldSummary {
        let edges = self.piece_fold_edges(piece);
        FoldSummary {
            count: edges.len(),
            length_mm: edges.iter().map(|e| e.length_mm).sum(),
        }
    }

    /// Lignes de pliure à exporter. Avec `with_cut` et une pièce à marge dont la coupe se calcule,
    /// la ligne suit la portion de coupe (prolongée jusqu'aux marges voisines) ; sinon, ou si la
    /// coupe n'a plus de portion sur ce bord, c'est le bord lui-même.
    pub fn piece_fold_lines(&self, piece: PieceId, with_cut: bool) -> Vec<FoldLine> {
        let edges = self.piece_fold_edges(piece);
        if edges.is_empty() {
            return Vec::new();
        }
        let cut = self
            .piece(piece)
            .ok()
            .filter(|p| with_cut && p.seam.is_some() && !p.net_cut)
            .and_then(|_| self.piece_cut_contour(piece).ok());
        edges
            .iter()
            .map(|e| {
                let from_cut = cut.as_ref().and_then(|c| {
                    let f = c.fold_cuts.iter().find(|f| f.entity == e.entity)?;
                    match c.traces.get(f.trace)? {
                        Trace::Line(a, b) => Some((*a, *b)),
                        _ => None,
                    }
                });
                let (from, to) = from_cut.unwrap_or((e.from, e.to));
                FoldLine {
                    entity: e.entity,
                    from,
                    to,
                }
            })
            .collect()
    }

    /// Bords déclarés qui ne sont plus dans le contour de la pièce (contrôle `FoldOutsideContour`).
    pub(crate) fn fold_outside_contour(&self, piece: &Piece) -> usize {
        piece
            .on_fold
            .iter()
            .filter(|e| !piece.outline.iter().any(|o| o.entity == **e))
            .count()
    }

    /// Bords sur le pli dont une marge est réglée (ignorée : contrôle `FoldMarginIgnored`).
    pub(crate) fn fold_margin_ignored(&self, piece: &Piece) -> usize {
        let Some(seam) = &piece.seam else { return 0 };
        piece
            .on_fold
            .iter()
            .filter(|e| piece.outline.iter().any(|o| o.entity == **e))
            .filter(|e| {
                seam.edges.iter().any(|x| x.entity == **e)
                    || seam.hems.iter().any(|x| x.entity == **e)
                    || seam.folds.iter().any(|x| x.entity == **e)
            })
            .count()
    }

    /// Validation d'une pièce chargée ou modifiée : pas de doublon, tracés existants de la feuille
    /// de la pièce et rectilignes. Que le tracé soit toujours dans le contour n'est **pas** exigé
    /// (un contour modifié laisse la déclaration, signalée par un contrôle, jamais un refus).
    pub(crate) fn validate_on_fold(&self, piece: &Piece) -> Result<(), CoreError> {
        let mut seen = std::collections::HashSet::new();
        for e in &piece.on_fold {
            let Some(kind) = self.fold_entity_kind(piece, *e) else {
                return Err(CoreError::DeadReference(format!(
                    "la pièce « {} » déclare sur le pli le tracé {}, absent de sa feuille",
                    piece.name, e.0
                )));
            };
            if !matches!(kind, EntityKind::Segment { .. }) {
                return Err(invalid(format!(
                    "la pièce « {} » déclare sur le pli le tracé {}, qui n'est pas un segment \
                     rectiligne",
                    piece.name, e.0
                )));
            }
            if !seen.insert(*e) {
                return Err(invalid(format!(
                    "la pièce « {} » déclare deux fois le tracé {} sur le pli",
                    piece.name, e.0
                )));
            }
        }
        Ok(())
    }

    /// `SetPieceFold` : déclare ou retire un bord sur le pli. Sans effet (aucune étape) si rien ne
    /// change. Refus expliqué, sans trace : pièce inconnue, pièce dérivée liée, tracé absent de la
    /// feuille de la pièce ; à la pose, tracé hors du contour ou non rectiligne.
    pub(crate) fn set_piece_fold(
        &mut self,
        piece: PieceId,
        edge: EntityId,
        on: bool,
    ) -> Result<(), CoreError> {
        let p = self.piece(piece)?;
        if p.derived.is_some() {
            return Err(invalid(format!(
                "la pièce « {} » est une pièce dérivée liée à sa mère : cassez le lien avant de \
                 déclarer un bord sur le pli",
                p.name
            )));
        }
        let Some(kind) = self.fold_entity_kind(p, edge) else {
            return Err(CoreError::DeadReference(format!(
                "le tracé {} n'existe pas sur la feuille de la pièce « {} »",
                edge.0, p.name
            )));
        };
        let declared = p.on_fold.contains(&edge);
        if on == declared {
            return Ok(());
        }
        if on {
            if !p.outline.iter().any(|o| o.entity == edge) {
                return Err(invalid(format!(
                    "le tracé {} n'est pas dans le contour de la pièce « {} » : seul un bord du \
                     contour peut être sur le pli",
                    edge.0, p.name
                )));
            }
            if !matches!(kind, EntityKind::Segment { .. }) {
                return Err(invalid(format!(
                    "le bord sur le pli de la pièce « {} » doit être un segment rectiligne (ni \
                     courbe, ni arc, ni cercle, ni courbe lisse)",
                    p.name
                )));
            }
        }
        let p = self.piece_mut(piece)?;
        if on {
            p.on_fold.push(edge);
            p.on_fold.sort();
        } else {
            p.on_fold.retain(|e| *e != edge);
        }
        Ok(())
    }

    /// Après une commande : un tracé déclaré sur le pli qui n'existe plus est retiré **avec une
    /// remarque** (jamais une référence morte). Ne touche que ce qui existait avant la commande :
    /// une référence morte introduite par la commande elle-même reste refusée par la validation.
    pub(crate) fn prune_dead_fold(&mut self, before: &Project, out: &mut Outcome) {
        if self.pieces.iter().all(|p| p.on_fold.is_empty()) {
            return;
        }
        let dead: Vec<(PieceId, Vec<EntityId>)> = self
            .pieces
            .iter()
            .filter(|p| !p.on_fold.is_empty())
            .map(|p| {
                let old = before.pieces.iter().find(|o| o.id == p.id);
                let gone = p
                    .on_fold
                    .iter()
                    .copied()
                    .filter(|e| {
                        old.is_some_and(|o| o.on_fold.contains(e))
                            && self.fold_entity_kind(p, *e).is_none()
                    })
                    .collect();
                (p.id, gone)
            })
            .filter(|(_, gone): &(PieceId, Vec<EntityId>)| !gone.is_empty())
            .collect();
        for (id, gone) in dead {
            if let Ok(p) = self.piece_mut(id) {
                p.on_fold.retain(|e| !gone.contains(e));
                for _ in &gone {
                    out.notes.push(format!(
                        "Un bord de la pièce « {} » déclaré sur le pli référençait un tracé \
                         supprimé : la déclaration est retirée.",
                        p.name
                    ));
                }
            }
        }
    }
}
