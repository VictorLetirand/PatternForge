//! Pinces de pièce (US-07.1, décision 297).
//!
//! Une **pince** est une définition stockée dans `Piece.darts` : un **sommet** (point de la
//! construction), un **point d'axe** (direction du sommet vers la base), une **longueur**
//! (sommet → milieu de la base, sur l'axe), une **ouverture** (largeur de la base) et la
//! **jambe qui reste fixe** quand on ferme la pince (gauche ou droite de l'axe). Les pieds, les
//! jambes (égales par construction), la fermeture et le capuchon sont **calculés à la lecture**
//! (`pf_geometry::dart`), jamais stockés : ce sont des mesures observées, jamais des contraintes.
//!
//! Les points d'ancrage peuvent bouger : la pince suit. Si la géométrie devient impossible
//! (sommet hors de la pièce, pieds hors du contour…), la pince est **signalée**
//! (`PieceIssue::DartInvalid`), jamais refusée ni perdue. Un point supprimé retire la pince avec
//! une remarque. La pince ne modifie jamais le contour ni la construction.

use crate::command::{Outcome, PointRef};
use crate::error::CoreError;
use crate::model::*;
use crate::piece_marking::sheet_with_points;
use pf_geometry::dart::{
    close_dart, dart_shape, distance_to_chain_mm, leg_crossing_contour, ClosedLine, DartShape,
    DART_FOOT_TOLERANCE_MM, DART_MAX_LENGTH_MM, DART_MAX_OPENING_MM, DART_MIN_LENGTH_MM,
    DART_MIN_OPENING_MM,
};
use pf_geometry::region::EdgeGeom;
use pf_geometry::split::edge_trace;
use pf_geometry::Side;
use serde::{Deserialize, Serialize};

/// Nombre maximal de pinces par pièce.
pub const DART_MAX_PER_PIECE: usize = 100;

/// Pince d'une pièce (définition stockée).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PieceDart {
    pub id: DartId,
    /// Sommet (point de la feuille de la pièce, intérieur au contour).
    pub apex: PointId,
    /// Point qui donne la direction de l'axe (du sommet vers la base).
    pub toward: PointId,
    /// Distance du sommet au milieu de la base, sur l'axe (mm).
    pub length_mm: f64,
    /// Largeur de la base (mm).
    pub opening_mm: f64,
    /// Jambe qui reste en place à la fermeture : `left` ou `right` de l'axe.
    pub fixed: Side,
    /// Coupe « pince fermée » (PH-5a) : la marge au droit de la bouche est tracée pince fermée
    /// puis rouverte (`Project::piece_cut_contour`). Omis quand faux ; jamais un refus.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub cut_closed: bool,
}

impl PieceDart {
    /// Points de la construction dont dépend la pince.
    pub fn points(&self) -> [PointId; 2] {
        [self.apex, self.toward]
    }
}

/// Résultat complet du calcul d'une pince : forme et fermeture.
#[derive(Debug, Clone, PartialEq)]
pub struct DartCalc {
    pub shape: DartShape,
    /// Tracé du contour qui porte la base.
    pub carrier: EntityId,
    /// Fermeture de la ligne qui porte la base.
    pub closed: ClosedLine,
}

/// Une pince et son calcul actuel.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedDart {
    pub dart: PieceDart,
    /// `Err(raison)` : la pince n'est pas calculable aujourd'hui (signalée, jamais refusée).
    pub calc: Result<DartCalc, String>,
}

fn bad(msg: impl Into<String>) -> CoreError {
    CoreError::Invalid(msg.into())
}

impl Project {
    /// Calcul d'une pince : forme, contour qui porte la base, fermeture et capuchon. Lecture
    /// pure. `Err(raison)` : points absents, valeurs hors bornes, sommet hors de la pièce, pieds
    /// hors du contour ou sur deux tracés, jambe qui traverse le contour.
    pub(crate) fn dart_calc(&self, piece: &Piece, dart: &PieceDart) -> Result<DartCalc, String> {
        let sheet = sheet_with_points(self, piece.link, &dart.points())
            .ok_or_else(|| "le sommet ou le point d'axe est absent de la feuille".to_string())?;
        let apex = sheet.position(dart.apex).map_err(|e| e.to_string())?;
        let toward = sheet.position(dart.toward).map_err(|e| e.to_string())?;
        let shape =
            dart_shape(apex, toward, dart.length_mm, dart.opening_mm).map_err(|e| e.to_string())?;
        let chain = self
            .piece_chain_any(piece.id)
            .map_err(|e| format!("contour inutilisable : {e}"))?;
        let geoms: Vec<EdgeGeom> = chain.iter().map(|c| c.geom).collect();
        let lasso = self.outline_lasso(piece.id).map_err(|e| e.to_string())?;
        if !lasso.contains_point(shape.apex)
            || distance_to_chain_mm(&geoms, shape.apex) <= DART_FOOT_TOLERANCE_MM
        {
            return Err("le sommet de la pince n'est pas à l'intérieur de la pièce".into());
        }
        // Tracés qui portent chaque pied (un pied posé sur un sommet appartient aux deux tracés
        // voisins : un tracé commun aux deux pieds porte la base).
        let carriers_of = |p| {
            let dists: Vec<f64> = geoms
                .iter()
                .map(|e| pf_geometry::region::trace_nearest_param(&edge_trace(e), p).1)
                .collect();
            let best = dists.iter().copied().fold(f64::INFINITY, f64::min);
            let mut on: Vec<EntityId> = Vec::new();
            for (i, d) in dists.iter().enumerate() {
                if *d <= DART_FOOT_TOLERANCE_MM && !on.contains(&chain[i].source) {
                    on.push(chain[i].source);
                }
            }
            (best, on)
        };
        let (dl, on_l) = carriers_of(shape.left_foot);
        let (dr, on_r) = carriers_of(shape.right_foot);
        for (side, dist) in [("gauche", dl), ("droit", dr)] {
            if dist > DART_FOOT_TOLERANCE_MM {
                return Err(format!(
                    "le pied {side} de la pince est à {dist:.3} mm du contour : la base doit être \
                     sur un bord (tolérance {DART_FOOT_TOLERANCE_MM} mm)"
                ));
            }
        }
        let Some(src_l) = on_l.iter().copied().find(|s| on_r.contains(s)) else {
            return Err(
                "les deux pieds de la pince sont sur deux tracés différents : la base doit \
                        être portée par un seul tracé du contour"
                    .into(),
            );
        };
        let line: Vec<EdgeGeom> = chain
            .iter()
            .filter(|c| c.source == src_l)
            .map(|c| c.geom)
            .collect();
        if let Some(side) = leg_crossing_contour(&geoms, &shape) {
            return Err(format!(
                "la jambe {} de la pince traverse le contour",
                if side == Side::Left {
                    "gauche"
                } else {
                    "droite"
                }
            ));
        }
        let closed = close_dart(&line, &shape, dart.fixed).map_err(|e| e.to_string())?;
        Ok(DartCalc {
            shape,
            carrier: src_l,
            closed,
        })
    }

    /// Pièce de la feuille dont le contour contient `at` (la plus petite si plusieurs) ; `None`
    /// si aucune. Sert à choisir la pièce d'une pince d'après le sommet cliqué.
    pub fn piece_containing(&self, sheet: SheetId, at: pf_geometry::PointMm) -> Option<PieceId> {
        self.pieces_on(sheet)
            .into_iter()
            .filter(|p| {
                self.outline_lasso(p.id)
                    .map(|l| l.contains_point(at))
                    .unwrap_or(false)
            })
            .filter_map(|p| self.piece_contour(p.id).ok().map(|c| (p.id, c.area_mm2)))
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(id, _)| id)
    }

    /// Pied de la perpendiculaire menée de `apex` au **bord droit** du contour le plus proche de
    /// `near` : point d'axe qui fait tomber les deux pieds d'une pince sur ce bord. `None` si le
    /// contour est illisible, si le bord le plus proche n'est pas un segment, ou si le pied de la
    /// perpendiculaire est hors du segment. Lecture pure, aide à la saisie d'une pince.
    pub fn dart_base_point(
        &self,
        piece: PieceId,
        apex: pf_geometry::PointMm,
        near: pf_geometry::PointMm,
    ) -> Option<pf_geometry::PointMm> {
        let chain = self.piece_chain_any(piece).ok()?;
        let (edge, _) = chain
            .iter()
            .map(|c| {
                (
                    c.geom,
                    pf_geometry::region::trace_nearest_param(&edge_trace(&c.geom), near).1,
                )
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))?;
        let EdgeGeom::Line(a, b) = edge else {
            return None;
        };
        let v = b - a;
        let len2 = v.dot(v);
        if !len2.is_finite() || len2 <= 0.0 {
            return None;
        }
        let t = (apex - a).dot(v) / len2;
        (0.0..=1.0).contains(&t).then(|| a.lerp(b, t))
    }

    /// Pinces d'une pièce avec leur calcul actuel (liste vide pour une pièce inconnue).
    pub fn piece_darts(&self, piece: PieceId) -> Vec<ResolvedDart> {
        let Ok(p) = self.piece(piece) else {
            return Vec::new();
        };
        p.darts
            .iter()
            .map(|&dart| ResolvedDart {
                dart,
                calc: self.dart_calc(p, &dart),
            })
            .collect()
    }

    /// Fermeture d'une pince (aperçu / contrôle, lecture pure : rien n'est modifié).
    pub fn piece_dart_close(&self, piece: PieceId, dart: DartId) -> Result<DartCalc, CoreError> {
        let p = self.piece(piece)?;
        let d = p.darts.iter().find(|d| d.id == dart).ok_or_else(|| {
            bad(format!(
                "la pince {} n'existe pas sur la pièce « {} »",
                dart.0, p.name
            ))
        })?;
        self.dart_calc(p, d).map_err(CoreError::Invalid)
    }

    /// Nombre de pinces de la pièce qui ne sont plus calculables (pour `PieceIssue`).
    pub(crate) fn piece_darts_invalid(&self, piece: PieceId) -> usize {
        self.piece_darts(piece)
            .iter()
            .filter(|r| r.calc.is_err())
            .count()
    }

    /// Validation structurelle d'une pièce chargée ou modifiée : nombre, bornes, références. La
    /// géométrie n'est **pas** jugée ici (un point déplacé ne doit pas refuser la commande).
    pub(crate) fn validate_darts(&self, piece: &Piece) -> Result<(), CoreError> {
        if piece.darts.len() > DART_MAX_PER_PIECE {
            return Err(bad(format!(
                "la pièce « {} » porte plus de {DART_MAX_PER_PIECE} pinces",
                piece.name
            )));
        }
        for d in &piece.darts {
            check_dart_values(d)?;
            if sheet_with_points(self, piece.link, &d.points()).is_none() {
                return Err(CoreError::DeadReference(format!(
                    "une pince de la pièce « {} » référence un point absent de la feuille de la \
                     pièce",
                    piece.name
                )));
            }
        }
        Ok(())
    }

    fn check_new_dart(&self, piece: &Piece, dart: &PieceDart) -> Result<(), CoreError> {
        check_dart_values(dart)?;
        if piece.derived.is_some() {
            return Err(bad(format!(
                "la pièce « {} » est une pièce dérivée liée à sa mère : cassez le lien avant d'y \
                 poser une pince",
                piece.name
            )));
        }
        for pt in dart.points() {
            if !self.sheets.iter().any(|s| s.point(pt).is_ok()) {
                return Err(CoreError::UnknownPoint(pt));
            }
        }
        if sheet_with_points(self, piece.link, &dart.points()).is_none() {
            return Err(CoreError::DeadReference(format!(
                "la pince de la pièce « {} » référence un point absent de la feuille de la pièce",
                piece.name
            )));
        }
        self.dart_calc(piece, dart).map(|_| ()).map_err(|why| {
            bad(format!(
                "pince de la pièce « {} » impossible : {why}",
                piece.name
            ))
        })
    }

    fn dart_index(&self, piece: PieceId, dart: DartId) -> Result<usize, CoreError> {
        let p = self.piece(piece)?;
        p.darts.iter().position(|d| d.id == dart).ok_or_else(|| {
            bad(format!(
                "la pince {} n'existe pas sur la pièce « {} »",
                dart.0, p.name
            ))
        })
    }

    /// Ajoute une pince (refus expliqué : pièce inconnue, dérivée liée, sans contour fermé,
    /// point absent ou d'une autre feuille, valeurs hors bornes, sommet hors de la pièce, pieds
    /// hors du contour, jambe qui traverse le contour, trop de pinces).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn add_piece_dart(
        &mut self,
        piece: PieceId,
        apex: PointRef,
        toward: PointRef,
        length_mm: f64,
        opening_mm: f64,
        fixed: Side,
        out: &mut Outcome,
    ) -> Result<(), CoreError> {
        let p = self.piece(piece)?;
        if p.darts.len() >= DART_MAX_PER_PIECE {
            return Err(bad(format!(
                "la pièce « {} » porte déjà {DART_MAX_PER_PIECE} pinces",
                p.name
            )));
        }
        if p.derived.is_some() {
            return Err(bad(format!(
                "la pièce « {} » est une pièce dérivée liée à sa mère : cassez le lien avant d'y \
                 poser une pince",
                p.name
            )));
        }
        let sheet = self.piece_sheet(piece)?.id;
        let apex = self.resolve_ref(sheet, apex, out)?;
        let toward = self.resolve_ref(sheet, toward, out)?;
        let id = DartId(self.next_id);
        let dart = PieceDart {
            id,
            apex,
            toward,
            length_mm,
            opening_mm,
            fixed,
            cut_closed: false,
        };
        let p = self.piece(piece)?;
        self.check_new_dart(p, &dart)?;
        let id = DartId(self.alloc_id());
        self.piece_mut(piece)?.darts.push(PieceDart { id, ..dart });
        out.darts.push(id);
        Ok(())
    }

    pub(crate) fn remove_piece_dart(
        &mut self,
        piece: PieceId,
        dart: DartId,
    ) -> Result<(), CoreError> {
        let i = self.dart_index(piece, dart)?;
        self.piece_mut(piece)?.darts.remove(i);
        Ok(())
    }

    /// Remplace les paramètres d'une pince (même identifiant, même rang ; mêmes contrôles que
    /// l'ajout).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn set_piece_dart(
        &mut self,
        piece: PieceId,
        dart: DartId,
        apex: PointId,
        toward: PointId,
        length_mm: f64,
        opening_mm: f64,
        fixed: Side,
    ) -> Result<(), CoreError> {
        let i = self.dart_index(piece, dart)?;
        let new = PieceDart {
            id: dart,
            apex,
            toward,
            length_mm,
            opening_mm,
            fixed,
            cut_closed: self.piece(piece)?.darts[i].cut_closed,
        };
        self.check_new_dart(self.piece(piece)?, &new)?;
        self.piece_mut(piece)?.darts[i] = new;
        Ok(())
    }

    /// Règle la coupe « pince fermée » d'une pince (PH-5a). Refus : pièce ou pince inconnue,
    /// pièce dérivée liée. Un calcul impossible n'est **pas** un refus (contrôle
    /// `PieceIssue::CutDartFailed`, coupe ordinaire conservée).
    pub(crate) fn set_piece_dart_cut_closed(
        &mut self,
        piece: PieceId,
        dart: DartId,
        closed: bool,
    ) -> Result<(), CoreError> {
        let i = self.dart_index(piece, dart)?;
        let p = self.piece(piece)?;
        if p.derived.is_some() {
            return Err(bad(format!(
                "la pièce « {} » est une pièce dérivée liée à sa mère : cassez le lien avant de \
                 régler sa pince",
                p.name
            )));
        }
        self.piece_mut(piece)?.darts[i].cut_closed = closed;
        Ok(())
    }

    /// Retire les pinces dont un point a disparu pendant la commande, avec une remarque. Ne
    /// touche que les pinces qui existaient avant la commande (une référence morte introduite
    /// par la commande elle-même reste refusée par la validation).
    pub(crate) fn prune_dead_darts(&mut self, before: &Project, out: &mut Outcome) {
        if self.pieces.iter().all(|p| p.darts.is_empty()) {
            return;
        }
        let dead: Vec<(PieceId, Vec<DartId>)> = self
            .pieces
            .iter()
            .filter(|p| !p.darts.is_empty())
            .map(|p| {
                let old = before.pieces.iter().find(|o| o.id == p.id);
                let gone = p
                    .darts
                    .iter()
                    .filter(|d| {
                        old.is_some_and(|o| o.darts.contains(d))
                            && sheet_with_points(self, p.link, &d.points()).is_none()
                    })
                    .map(|d| d.id)
                    .collect();
                (p.id, gone)
            })
            .filter(|(_, gone): &(PieceId, Vec<DartId>)| !gone.is_empty())
            .collect();
        for (id, gone) in dead {
            if let Ok(p) = self.piece_mut(id) {
                p.darts.retain(|d| !gone.contains(&d.id));
                for _ in &gone {
                    out.notes.push(format!(
                        "Une pince de la pièce « {} » référençait un point supprimé : elle est \
                         retirée.",
                        p.name
                    ));
                }
            }
        }
    }
}

/// Valeurs numériques d'une pince : finies et dans les bornes, ouverture ≤ longueur.
fn check_dart_values(d: &PieceDart) -> Result<(), CoreError> {
    if d.apex == d.toward {
        return Err(bad(
            "le sommet de la pince et son point d'axe doivent être deux points différents",
        ));
    }
    let l = d.length_mm;
    if !l.is_finite() || !(DART_MIN_LENGTH_MM..=DART_MAX_LENGTH_MM).contains(&l) {
        return Err(bad(format!(
            "la longueur d'une pince doit être un nombre fini compris entre {DART_MIN_LENGTH_MM} et \
             {DART_MAX_LENGTH_MM} mm"
        )));
    }
    let w = d.opening_mm;
    if !w.is_finite() || !(DART_MIN_OPENING_MM..=DART_MAX_OPENING_MM).contains(&w) || w > l {
        return Err(bad(format!(
            "l'ouverture d'une pince doit être un nombre fini compris entre {DART_MIN_OPENING_MM} \
             et {DART_MAX_OPENING_MM} mm, sans dépasser sa longueur"
        )));
    }
    Ok(())
}
