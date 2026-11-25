//! Plis (US-07.3, décision 299).
//!
//! `Command::AddPiecePleats` pose un ou plusieurs plis de même profondeur : le contour de la pièce
//! est **écarté** le long de chaque ligne de pli de la largeur du pli (convention d'atelier non
//! validée : un pli **plat** de profondeur `p` consomme `2p` de tissu, un pli **creux** (deux plis
//! plats dos à dos) `4p`). La pièce est **reconstruite sur une feuille dédiée** (même identifiant,
//! construction d'origine intacte, comme US-07.2) ; la **définition** de chaque pli est stockée
//! dans `Piece.pleats` (ligne en mm dans le repère de la feuille, profondeur, sens, genre).
//!
//! Le contour stocké est donc le contour **déplié** (avec la bande de chaque pli) : la marge de
//! couture (E05) se calcule dessus, sans rien changer. La **pièce repliée** (`piece_folded`) est
//! calculée à la lecture en retirant la bande de chaque pli, dans l'ordre inverse de la pose ; elle
//! est égale à la pièce d'avant les plis à 1e-6 mm près (critère d'acceptation).
//!
//! Les repères (lignes de pli et de contre-pli, flèches de sens) sont calculés à la lecture
//! (`piece_pleat_marks`), jamais exportés. Les lignes sont des coordonnées : si le contour est
//! modifié ensuite, la pièce repliée peut devenir « non calculable » (jamais un refus).

use crate::command::Outcome;
use crate::error::CoreError;
use crate::model::*;
use pf_geometry::pleat::{
    fold_chain, merge_collinear, pleat_normal, spread_chain, translate_edge, PLEAT_MAX_WIDTH_MM,
};
use pf_geometry::region::EdgeGeom;
use pf_geometry::split::{chain_length_mm, chain_signed_area_mm2, cut_chain_by_line, edge_sample};
use pf_geometry::{distance_mm, Lasso, PointMm, Side, VectorMm, EPSILON_MM, MAX_COORD_MM};
use serde::{Deserialize, Serialize};

/// Plus petite profondeur de pli (mm).
pub const PLEAT_MIN_DEPTH_MM: f64 = 0.1;
/// Plus grande profondeur de pli (mm).
pub const PLEAT_MAX_DEPTH_MM: f64 = 500.0;
/// Nombre maximal de plis posés par une commande.
pub const PLEAT_MAX_COUNT: u32 = 50;
/// Nombre maximal de plis par pièce.
pub const PLEAT_MAX_PER_PIECE: usize = 100;
/// Jeu minimal (mm) entre deux bandes de plis répartis.
const MIN_GAP_MM: f64 = 1e-6;
/// Tolérance de comparaison de positions le long de la normale (mm).
const POS_TOL_MM: f64 = 1e-9;

/// Genre de pli.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PleatKind {
    /// Pli plat : bande de `2 × profondeur`.
    Flat,
    /// Pli creux : deux plis plats dos à dos, bande de `4 × profondeur`.
    Hollow,
}

impl PleatKind {
    /// Largeur de la bande de tissu consommée (mm).
    pub fn width_mm(self, depth_mm: f64) -> f64 {
        match self {
            PleatKind::Flat => 2.0 * depth_mm,
            PleatKind::Hollow => 4.0 * depth_mm,
        }
    }
}

/// Pli d'une pièce (définition stockée) : la droite `from → to` est le **bord de départ** de la
/// bande, qui s'étend du côté `sense` sur `width_mm()`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PiecePleat {
    pub id: PleatId,
    pub kind: PleatKind,
    pub from_x_mm: f64,
    pub from_y_mm: f64,
    pub to_x_mm: f64,
    pub to_y_mm: f64,
    /// Profondeur du pli (mm).
    pub depth_mm: f64,
    /// Côté de la ligne où s'étend la bande : `left` ou `right` de `from → to`.
    pub sense: Side,
}

impl PiecePleat {
    pub fn start(&self) -> PointMm {
        PointMm::new(self.from_x_mm, self.from_y_mm)
    }

    pub fn end(&self) -> PointMm {
        PointMm::new(self.to_x_mm, self.to_y_mm)
    }

    /// Largeur ajoutée au contour par ce pli (mm).
    pub fn width_mm(&self) -> f64 {
        self.kind.width_mm(self.depth_mm)
    }

    fn moved_by(&self, v: VectorMm) -> PiecePleat {
        PiecePleat {
            from_x_mm: self.from_x_mm + v.x,
            from_y_mm: self.from_y_mm + v.y,
            to_x_mm: self.to_x_mm + v.x,
            to_y_mm: self.to_y_mm + v.y,
            ..*self
        }
    }

    /// Positions des lignes repères le long de la normale (mm depuis le bord de départ).
    fn line_offsets(&self) -> Vec<(PleatLineKind, f64)> {
        let p = self.depth_mm;
        match self.kind {
            PleatKind::Flat => vec![(PleatLineKind::Fold, 0.0), (PleatLineKind::Counter, p)],
            PleatKind::Hollow => vec![
                (PleatLineKind::Fold, 0.0),
                (PleatLineKind::Counter, p),
                (PleatLineKind::Counter, 3.0 * p),
                (PleatLineKind::Fold, 4.0 * p),
            ],
        }
    }

    /// Flèches de sens : (départ, arrivée) le long de la normale.
    fn arrow_offsets(&self) -> Vec<(f64, f64)> {
        let p = self.depth_mm;
        match self.kind {
            PleatKind::Flat => vec![(0.0, p)],
            PleatKind::Hollow => vec![(0.0, p), (4.0 * p, 3.0 * p)],
        }
    }
}

/// Nature d'une ligne repère.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PleatLineKind {
    /// Ligne de pli.
    Fold,
    /// Ligne de contre-pli.
    Counter,
}

/// Ligne repère d'un pli, de bord à bord du contour.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PleatLine {
    pub kind: PleatLineKind,
    pub from: PointMm,
    pub to: PointMm,
}

/// Flèche de sens d'un pli (du pli vers le contre-pli).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PleatArrow {
    pub from: PointMm,
    pub to: PointMm,
}

/// Repères calculés d'un pli.
#[derive(Debug, Clone, PartialEq)]
pub struct PleatMarks {
    pub lines: Vec<PleatLine>,
    pub arrows: Vec<PleatArrow>,
}

/// Un pli et ses repères actuels.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedPleat {
    pub pleat: PiecePleat,
    /// `Err(raison)` : repères non calculables aujourd'hui (signalé, jamais refusé).
    pub marks: Result<PleatMarks, String>,
}

/// Pièce repliée : contour d'avant les plis (aperçu, la pièce n'est pas modifiée).
#[derive(Debug, Clone, PartialEq)]
pub struct FoldedPiece {
    /// Contour anti-horaire, segments alignés fusionnés.
    pub edges: Vec<EdgeGeom>,
    pub area_mm2: f64,
    pub perimeter_mm: f64,
    /// Largeur totale retirée (somme des largeurs des plis, mm).
    pub removed_mm: f64,
}

fn invalid(msg: impl Into<String>) -> CoreError {
    CoreError::Invalid(msg.into())
}

/// Position d'un pli existant par rapport à la bande `[0 ; width]` d'un nouveau pli.
enum Rel {
    Before,
    After,
}

fn classify(
    existing: &PiecePleat,
    a: PointMm,
    b: PointMm,
    side: Side,
    width: f64,
) -> Result<Rel, String> {
    let u = (b - a).unit().map_err(|e| e.to_string())?;
    let ue = (existing.end() - existing.start())
        .unit()
        .map_err(|e| e.to_string())?;
    if u.cross(ue).abs() > 1e-9 {
        return Err("les lignes de pli ne sont pas parallèles : les plis se croisent".into());
    }
    let d = pleat_normal(a, b, side).map_err(|e| e.to_string())?;
    let de = pleat_normal(existing.start(), existing.end(), existing.sense)
        .map_err(|e| e.to_string())?;
    let s = d.dot(existing.start() - a);
    let we = existing.width_mm();
    let (lo, hi) = if d.dot(de) > 0.0 {
        (s, s + we)
    } else {
        (s - we, s)
    };
    if lo >= width - POS_TOL_MM {
        Ok(Rel::After)
    } else if hi <= POS_TOL_MM {
        Ok(Rel::Before)
    } else {
        Err("deux plis se chevauchent".into())
    }
}

impl Project {
    /// Validation structurelle des plis stockés d'une pièce (la géométrie n'est pas jugée).
    pub(crate) fn validate_pleats(piece: &Piece) -> Result<(), CoreError> {
        if piece.pleats.len() > PLEAT_MAX_PER_PIECE {
            return Err(invalid(format!(
                "la pièce « {} » porte plus de {PLEAT_MAX_PER_PIECE} plis",
                piece.name
            )));
        }
        for l in &piece.pleats {
            let coords = [l.from_x_mm, l.from_y_mm, l.to_x_mm, l.to_y_mm];
            if coords
                .iter()
                .any(|v| !v.is_finite() || v.abs() > MAX_COORD_MM)
            {
                return Err(invalid(format!(
                    "un pli de la pièce « {} » a une coordonnée non finie ou hors de l'espace de \
                     travail",
                    piece.name
                )));
            }
            if distance_mm(l.start(), l.end()) <= EPSILON_MM {
                return Err(invalid(format!(
                    "un pli de la pièce « {} » a ses deux points de ligne confondus",
                    piece.name
                )));
            }
            if !l.depth_mm.is_finite()
                || !(PLEAT_MIN_DEPTH_MM..=PLEAT_MAX_DEPTH_MM).contains(&l.depth_mm)
            {
                return Err(invalid(format!(
                    "la profondeur d'un pli de la pièce « {} » doit être un nombre fini compris \
                     entre {PLEAT_MIN_DEPTH_MM} et {PLEAT_MAX_DEPTH_MM} mm",
                    piece.name
                )));
            }
        }
        Ok(())
    }

    /// Repères (lignes de pli et de contre-pli, flèches de sens) de chaque pli de la pièce.
    /// Lecture pure ; liste vide pour une pièce inconnue ou sans pli.
    pub fn piece_pleat_marks(&self, piece: PieceId) -> Vec<ResolvedPleat> {
        let Ok(p) = self.piece(piece) else {
            return Vec::new();
        };
        if p.pleats.is_empty() {
            return Vec::new();
        }
        let chain = self.piece_chain_any(piece);
        p.pleats
            .iter()
            .map(|&pleat| ResolvedPleat {
                pleat,
                marks: match &chain {
                    Ok(c) => marks_of(&pleat, &crate::piece_split_join::geoms_of(c)),
                    Err(e) => Err(format!("contour inutilisable : {e}")),
                },
            })
            .collect()
    }

    /// Pièce repliée : le contour sans la bande de chaque pli (ordre inverse de la pose). Lecture
    /// pure, la pièce n'est pas modifiée. Refus expliqué : pièce inconnue, sans pli, contour
    /// inutilisable ou qui ne correspond plus aux plis.
    pub fn piece_folded(&self, piece: PieceId) -> Result<FoldedPiece, CoreError> {
        let p = self.piece(piece)?;
        if p.pleats.is_empty() {
            return Err(invalid(format!(
                "la pièce « {} » n'a aucun pli à replier",
                p.name
            )));
        }
        let chain = self.piece_chain_any(piece)?;
        let mut edges = crate::piece_split_join::geoms_of(&chain);
        let mut rest = p.pleats.clone();
        let mut removed = 0.0;
        while let Some(k) = rest.pop() {
            let w = k.width_mm();
            edges = fold_chain(&edges, k.start(), k.end(), k.sense, w)
                .map_err(|e| invalid(format!("la pièce ne peut pas être repliée : {e}")))?;
            removed += w;
            let back =
                pleat_normal(k.start(), k.end(), k.sense).map_err(|e| invalid(e.to_string()))? * -w;
            let mut next = Vec::with_capacity(rest.len());
            for j in rest {
                match classify(&j, k.start(), k.end(), k.sense, w)
                    .map_err(|e| invalid(format!("la pièce ne peut pas être repliée : {e}")))?
                {
                    Rel::After => next.push(j.moved_by(back)),
                    Rel::Before => next.push(j),
                }
            }
            rest = next;
        }
        let merged: Vec<EdgeGeom> = merge_collinear(edges.into_iter().map(|e| (e, ())).collect())
            .into_iter()
            .map(|(e, _)| e)
            .collect();
        Ok(FoldedPiece {
            area_mm2: chain_signed_area_mm2(&merged).abs(),
            perimeter_mm: chain_length_mm(&merged),
            edges: merged,
            removed_mm: removed,
        })
    }

    /// Pose `count` plis (voir `Command::AddPiecePleats`).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn add_piece_pleats(
        &mut self,
        piece: PieceId,
        from: PointMm,
        to: PointMm,
        depth_mm: f64,
        kind: PleatKind,
        sense: Side,
        count: u32,
        span_mm: f64,
        out: &mut Outcome,
    ) -> Result<(), CoreError> {
        from.validate()?;
        to.validate()?;
        if !depth_mm.is_finite() || !(PLEAT_MIN_DEPTH_MM..=PLEAT_MAX_DEPTH_MM).contains(&depth_mm) {
            return Err(invalid(format!(
                "la profondeur d'un pli ({depth_mm}) doit être un nombre fini compris entre \
                 {PLEAT_MIN_DEPTH_MM} et {PLEAT_MAX_DEPTH_MM} mm"
            )));
        }
        if !(1..=PLEAT_MAX_COUNT).contains(&count) {
            return Err(invalid(format!(
                "le nombre de plis ({count}) doit être compris entre 1 et {PLEAT_MAX_COUNT}"
            )));
        }
        let width = kind.width_mm(depth_mm);
        debug_assert!(width <= PLEAT_MAX_WIDTH_MM);
        let d = pleat_normal(from, to, sense).map_err(|e| invalid(e.to_string()))?;
        let step = if count > 1 {
            if !span_mm.is_finite() || span_mm <= 0.0 || span_mm > MAX_COORD_MM {
                return Err(invalid(format!(
                    "l'étendue de la répartition ({span_mm} mm) doit être un nombre fini positif"
                )));
            }
            span_mm / f64::from(count - 1)
        } else {
            0.0
        };
        if count > 1 && step < width + MIN_GAP_MM {
            return Err(invalid(format!(
                "les plis se chevauchent : le pas ({step:.3} mm) doit dépasser la largeur d'un pli \
                 ({width:.3} mm)"
            )));
        }
        let orig = self.piece(piece)?.clone();
        let index = self
            .pieces
            .iter()
            .position(|p| p.id == piece)
            .ok_or(CoreError::UnknownPiece(piece))?;
        if orig.pleats.len() + count as usize > PLEAT_MAX_PER_PIECE {
            return Err(invalid(format!(
                "la pièce « {} » porterait plus de {PLEAT_MAX_PER_PIECE} plis",
                orig.name
            )));
        }
        if orig.derived.is_some() {
            return Err(invalid(format!(
                "la pièce « {} » est une pièce dérivée liée à sa mère : cassez le lien avant d'y                  poser un pli",
                orig.name
            )));
        }
        if !orig.darts.is_empty() {
            return Err(invalid(format!(
                "la pièce « {} » porte {} pince(s) : un pli reconstruit la pièce sur une nouvelle \
                 feuille et ne reprend pas les pinces, supprimez-les d'abord",
                orig.name,
                orig.darts.len()
            )));
        }
        let chain = self.piece_chain(piece)?;
        let area_before = chain_signed_area_mm2(&crate::piece_split_join::geoms_of(&chain));
        let mut cur: Vec<(EdgeGeom, PathRole)> = chain.iter().map(|e| (e.geom, e.role)).collect();
        let mut pleats = orig.pleats.clone();
        let mut notes = Vec::new();
        let mut carry = self.carry_of_with(piece, &mut notes, false)?;
        let shift = d * width;
        let mut added = Vec::new();
        let mut expected_gain = 0.0;
        // Le pli le plus éloigné d'abord : les plus proches restent à leur place d'origine.
        for k in (0..count).rev() {
            let off = d * (step * f64::from(k));
            let (a, b) = (from + off, to + off);
            let geoms: Vec<EdgeGeom> = cur.iter().map(|(g, _)| *g).collect();
            let sp = spread_chain(&geoms, a, b, sense, width)
                .map_err(|e| invalid(format!("le pli ne peut pas être posé : {e}")))?;
            expected_gain += width * sp.chord_mm;
            for j in &mut pleats {
                match classify(j, a, b, sense, width)
                    .map_err(|e| invalid(format!("le pli ne peut pas être posé : {e}")))?
                {
                    Rel::After => *j = j.moved_by(shift),
                    Rel::Before => {}
                }
            }
            let mut last_role = PathRole::default();
            let mut next: Vec<(EdgeGeom, PathRole)> = Vec::with_capacity(sp.edges.len());
            for e in &sp.edges {
                if let Some(s) = e.source {
                    last_role = cur[s].1;
                }
                next.push((e.geom, last_role));
            }
            cur = merge_collinear(next);
            let mut poly = Vec::new();
            for g in &sp.moving {
                poly.extend(edge_sample(g, 48));
            }
            let region = Lasso::new(&poly)
                .ok_or_else(|| invalid("la partie à écarter est dégénérée : pli impossible"))?;
            carry = carry.moved_in(
                &region,
                &|p| p + shift,
                &|g| translate_edge(g, shift),
                "ligne de pli",
                &orig.name,
                &mut notes,
            );
            let id = PleatId(self.alloc_id());
            pleats.push(PiecePleat {
                id,
                kind,
                from_x_mm: a.x,
                from_y_mm: a.y,
                to_x_mm: b.x,
                to_y_mm: b.y,
                depth_mm,
                sense,
            });
            added.push(id);
        }
        let area_after = chain_signed_area_mm2(&cur.iter().map(|(g, _)| *g).collect::<Vec<_>>());
        if (area_after - area_before - expected_gain).abs() > 1e-6 * area_after.abs().max(1.0) {
            return Err(invalid(
                "incohérence interne : l'aire du contour écarté ne correspond pas à la largeur \
                 ajoutée",
            ));
        }

        let joints: Vec<PointMm> = cur.iter().map(|(g, _)| g.start()).collect();
        let sid = self.new_result_sheet(&orig.name);
        let outline = self.build_closed_chain(sid, &cur, &joints)?;
        let (grainline, internal, markings, buttons, grain) = self.carry_into(sid, &carry, true)?;
        let mut seam_notes = Vec::new();
        let seam = crate::piece_seam::carried_seam(&orig, &mut seam_notes);
        notes.extend(seam_notes);
        let new_piece = Piece {
            outline,
            grainline,
            grain,
            seam,
            on_fold: Vec::new(),
            internal,
            markings,
            buttons,
            pleats,
            link: PieceLink::Detached,
            ..orig.clone()
        };
        self.pieces[index] = new_piece;
        self.piece_chain_any(piece)
            .map_err(|e| invalid(format!("le pli produit un contour inutilisable : {e}")))?;
        out.pieces.push(piece);
        out.sheet = Some(sid);
        out.pleats.extend(added);
        notes.dedup();
        notes.insert(
            0,
            format!(
                "{count} pli(s) posé(s) sur la pièce « {} » (largeur ajoutée {:.3} mm) ; la pièce \
                 est reconstruite sur une nouvelle feuille, la construction d'origine n'est pas \
                 modifiée.",
                orig.name,
                width * f64::from(count)
            ),
        );
        out.notes.extend(notes);
        Ok(())
    }
}

/// Repères d'un pli : chaque ligne va de bord à bord du contour.
fn marks_of(pleat: &PiecePleat, chain: &[EdgeGeom]) -> Result<PleatMarks, String> {
    let d = pleat_normal(pleat.start(), pleat.end(), pleat.sense).map_err(|e| e.to_string())?;
    let chord = |offset: f64| -> Result<[PointMm; 2], String> {
        let o = d * offset;
        cut_chain_by_line(chain, pleat.start() + o, pleat.end() + o)
            .map(|c| c.points)
            .map_err(|e| e.to_string())
    };
    let mut lines = Vec::new();
    for (kind, off) in pleat.line_offsets() {
        let [p, q] = chord(off)?;
        lines.push(PleatLine {
            kind,
            from: p,
            to: q,
        });
    }
    let mut arrows = Vec::new();
    for (s0, s1) in pleat.arrow_offsets() {
        let [p, q] = chord(s0)?;
        let mid = p.lerp(q, 0.5);
        arrows.push(PleatArrow {
            from: mid,
            to: mid + d * (s1 - s0),
        });
    }
    Ok(PleatMarks { lines, arrows })
}
