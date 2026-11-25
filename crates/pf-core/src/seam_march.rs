//! US-08.2 — marche de pièces : reporter les crans d'un côté d'un couple apparié (US-08.1) sur
//! l'autre côté, à la même abscisse curviligne **sur la ligne de couture**.
//!
//! Conventions d'atelier (non validées) :
//! - abscisse = longueur développée depuis le **début** de la suite de tracés du côté (ordre du
//!   contour, sens de chaque tracé respecté) ;
//! - **embu réparti proportionnellement** : abscisse cible = abscisse source × (longueur cible /
//!   longueur source) ; l'« écart d'abscisse » est cible − source dans le repère de couture ;
//! - **sens de couture** : endroit contre endroit, deux contours de même sens de rotation sont
//!   cousus en sens **opposés** (début de A contre fin de B) ; de sens différents, dans le même
//!   sens ;
//! - le cran reporté garde le côté de la pièce (intérieur / extérieur) du cran source, forme,
//!   largeur, longueur, style et `on_cut` copiés ; position **fixe** (jamais ancrée) ;
//! - les crans créés sont de vrais crans, **non liés** au côté source ; la correspondance n'est
//!   pas mémorisée (mesure ≠ contrainte) ; un cran déjà présent à ±0,01 mm n'est pas doublé.

use crate::command::Outcome;
use crate::error::CoreError;
use crate::model::*;
use crate::seam_pair::{SeamSide, SeamWhich};
use pf_geometry::{PointMm, Side};

/// Tolérance de détection d'un cran déjà présent (mm).
pub const MARCH_DUPLICATE_TOLERANCE_MM: f64 = 0.01;

/// Un cran reporté (lecture ; rien n'est modifié).
#[derive(Debug, Clone, PartialEq)]
pub struct MarchedNotch {
    pub source: NotchId,
    /// Abscisse du cran source (mm depuis le début de son côté).
    pub source_mm: f64,
    /// Abscisse du cran reporté (mm depuis le début de son côté, dans le sens du contour).
    pub target_mm: f64,
    /// Abscisse reportée − source, dans le repère de couture (négatif quand la cible est plus
    /// courte) ; nul sans embu.
    pub gap_mm: f64,
    /// Tracé d'accueil et distance depuis son départ.
    pub entity: EntityId,
    pub distance_mm: f64,
    pub side: Side,
    /// Point du bord où le cran est posé (feuille de la pièce cible).
    pub point: PointMm,
    /// Un cran existe déjà à cette position (ou vient d'être reporté) : il ne sera pas créé.
    pub duplicate: bool,
}

/// Aperçu d'une marche.
#[derive(Debug, Clone, PartialEq)]
pub struct MarchPreview {
    pub from: SeamWhich,
    pub from_length_mm: f64,
    pub to_length_mm: f64,
    /// Côtés cousus en sens opposés (début contre fin).
    pub opposite: bool,
    pub items: Vec<MarchedNotch>,
}

pub(crate) struct Frame {
    pub(crate) sheet: SheetId,
    /// (tracé, inversé dans le contour, longueur, abscisse de départ)
    pub(crate) edges: Vec<(EntityId, bool, f64, f64)>,
    pub(crate) total: f64,
    pub(crate) clockwise: bool,
    pub(crate) whole: bool,
}

/// Abscisse de couture `(proportionnelle, finale)` du cran d'abscisse `s` d'un côté de longueur `ts`
/// reporté sur un côté de longueur `tt` : embu réparti, puis sens de couture. Partagée avec le
/// contrôle des crans (US-08.4).
pub(crate) fn march_target(s: f64, ts: f64, tt: f64, opposite: bool) -> (f64, f64) {
    let sewn = s * tt / ts;
    (
        sewn,
        (if opposite { tt - sewn } else { sewn }).clamp(0.0, tt),
    )
}

/// Deux côtés sont cousus en sens **opposés** (début de l'un contre fin de l'autre) quand leurs
/// contours ont le même sens de rotation. Partagée avec le contrôle des crans (US-08.4) et le
/// contrôle des raccords (US-08.3).
pub(crate) fn sewn_opposite(a: &Frame, b: &Frame) -> bool {
    a.clockwise == b.clockwise
}

/// L'extrémité d'un côté de longueur `ts` (`at_end` : sa fin, sinon son début) est cousue à la
/// **fin** de l'autre côté (longueur `tt`) : même calcul que le report d'un cran placé à
/// cette extrémité (`march_target`). US-08.3.
pub(crate) fn partner_is_end(at_end: bool, ts: f64, tt: f64, opposite: bool) -> bool {
    march_target(if at_end { ts } else { 0.0 }, ts, tt, opposite).1 > tt / 2.0
}

fn bad(msg: impl Into<String>) -> CoreError {
    CoreError::Invalid(msg.into())
}

impl Project {
    pub(crate) fn march_frame(&self, side: &SeamSide, what: &str) -> Result<Frame, CoreError> {
        self.seam_side_length(side).map_err(|_| {
            bad(format!(
                "{what} : côté non calculable (contour ouvert ou tracé illisible)"
            ))
        })?;
        let piece = self.piece(side.piece)?;
        let clockwise = self.piece_contour(side.piece)?.clockwise;
        let sheet = self.piece_sheet(side.piece)?;
        let mut edges = Vec::new();
        let mut at = 0.0;
        for e in &side.edges {
            let rev = piece
                .outline
                .iter()
                .find(|o| o.entity == *e)
                .map(|o| o.reversed)
                .ok_or_else(|| bad(format!("{what} : tracé hors du contour")))?;
            let len = sheet.path_length_mm(*e)?;
            edges.push((*e, rev, len, at));
            at += len;
        }
        Ok(Frame {
            sheet: sheet.id,
            edges,
            total: at,
            clockwise,
            whole: side.edges.len() == piece.outline.len(),
        })
    }

    /// Abscisse d'un cran d'un tracé du côté, ou `None` si le tracé n'en fait pas partie.
    pub(crate) fn march_abscissa(&self, f: &Frame, n: &Notch) -> Result<Option<f64>, CoreError> {
        let Some(&(_, rev, len, start)) = f.edges.iter().find(|e| e.0 == n.entity) else {
            return Ok(None);
        };
        let d = self.sheet(f.sheet)?.notch_distance_mm(n)?.clamp(0.0, len);
        Ok(Some(start + if rev { len - d } else { d }))
    }

    /// Lecture : crans qui seraient reportés de `from` vers l'autre côté du couple, sans rien
    /// modifier. Refus : couple inconnu, côté non calculable, cran hors du côté source, aucun
    /// cran sur le côté source.
    pub fn march_preview(
        &self,
        pair: SeamPairId,
        from: SeamWhich,
        which: Option<&[NotchId]>,
    ) -> Result<MarchPreview, CoreError> {
        let p = self
            .seams
            .iter()
            .find(|s| s.id == pair)
            .ok_or_else(|| bad("couture appariée inconnue"))?;
        let (src, dst) = match from {
            SeamWhich::A => (&p.a, &p.b),
            SeamWhich::B => (&p.b, &p.a),
        };
        let fs = self.march_frame(src, "côté source")?;
        let ft = self.march_frame(dst, "côté cible")?;
        let opposite = sewn_opposite(&fs, &ft);
        let src_sheet = self.sheet(fs.sheet)?;
        let dst_sheet = self.sheet(ft.sheet)?;

        let mut picked: Vec<(f64, &Notch)> = Vec::new();
        match which {
            None => {
                for n in &src_sheet.notches {
                    if let Some(s) = self.march_abscissa(&fs, n)? {
                        picked.push((s, n));
                    }
                }
            }
            Some(ids) => {
                for id in ids {
                    let n = src_sheet
                        .notches
                        .iter()
                        .find(|n| n.id == *id)
                        .ok_or_else(|| bad("cran hors du côté source"))?;
                    let s = self
                        .march_abscissa(&fs, n)?
                        .ok_or_else(|| bad("cran hors du côté source"))?;
                    if !picked.iter().any(|(_, m)| m.id == n.id) {
                        picked.push((s, n));
                    }
                }
            }
        }
        if picked.is_empty() {
            return Err(bad("aucun cran à reporter sur ce côté"));
        }
        picked.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.id.0.cmp(&b.1.id.0)));

        let existing: Vec<f64> = dst_sheet
            .notches
            .iter()
            .filter_map(|n| self.march_abscissa(&ft, n).transpose())
            .collect::<Result<_, _>>()?;
        let tol = MARCH_DUPLICATE_TOLERANCE_MM;
        let mut taken: Vec<f64> = Vec::new();
        let mut items = Vec::new();
        for (s, n) in picked {
            let (sewn, pos) = march_target(s, fs.total, ft.total, opposite);
            let k = ft
                .edges
                .iter()
                .position(|e| pos <= e.3 + e.2 + 1e-12)
                .unwrap_or(ft.edges.len() - 1);
            let (entity, rev, len, start) = ft.edges[k];
            let along = (pos - start).clamp(0.0, len);
            let distance = if rev { len - along } else { along };
            // Côté de la pièce : la gauche du contour est l'intérieur s'il est anti-horaire.
            let src_rev = fs
                .edges
                .iter()
                .find(|e| e.0 == n.entity)
                .is_some_and(|e| e.1);
            let left_in_outline = (n.side == Side::Left) != src_rev;
            let inside = left_in_outline != fs.clockwise;
            let target_left = inside != ft.clockwise;
            let side = if target_left != rev {
                Side::Left
            } else {
                Side::Right
            };
            let near = |e: f64| {
                (e - pos).abs() <= tol || (ft.whole && (ft.total - (e - pos).abs()) <= tol)
            };
            let duplicate = existing.iter().chain(taken.iter()).any(|e| near(*e));
            if !duplicate {
                taken.push(pos);
            }
            let (point, _) = dst_sheet.path_point_at(entity, distance)?;
            items.push(MarchedNotch {
                source: n.id,
                source_mm: s,
                target_mm: pos,
                gap_mm: sewn - s,
                entity,
                distance_mm: distance,
                side,
                point,
                duplicate,
            });
        }
        Ok(MarchPreview {
            from,
            from_length_mm: fs.total,
            to_length_mm: ft.total,
            opposite,
            items,
        })
    }

    /// Crée les crans reportés (une étape). Refus sans trace : voir [`Self::march_preview`] ;
    /// aussi quand tous les crans à reporter existent déjà. Doublons évités : remarque.
    pub(crate) fn march_notches(
        &mut self,
        pair: SeamPairId,
        from: SeamWhich,
        which: Option<Vec<NotchId>>,
        out: &mut Outcome,
    ) -> Result<(), CoreError> {
        let prev = self.march_preview(pair, from, which.as_deref())?;
        let s = self
            .seams
            .iter()
            .find(|s| s.id == pair)
            .ok_or_else(|| bad("couture appariée inconnue"))?;
        let (src, dst) = match from {
            SeamWhich::A => (s.a.piece, s.b.piece),
            SeamWhich::B => (s.b.piece, s.a.piece),
        };
        let src_sheet = self.piece_sheet(src)?.id;
        let dst_sheet = self.piece_sheet(dst)?.id;
        let fresh: Vec<&MarchedNotch> = prev.items.iter().filter(|i| !i.duplicate).collect();
        if fresh.is_empty() {
            return Err(bad(
                "tous les crans de ce côté existent déjà sur l'autre côté : rien à reporter",
            ));
        }
        let mut sources = Vec::new();
        for it in &fresh {
            sources.push(self.sheet(src_sheet)?.notch(it.source)?.clone());
        }
        let mut made = Vec::new();
        for (it, src_notch) in fresh.iter().zip(sources) {
            let id = NotchId(self.alloc_id());
            made.push(Notch {
                id,
                entity: it.entity,
                distance_mm: it.distance_mm,
                side: it.side,
                anchor: None,
                corner: false,
                ..src_notch
            });
        }
        let ids: Vec<NotchId> = made.iter().map(|n| n.id).collect();
        self.sheet_mut(dst_sheet)?.notches.extend(made);
        out.notches.extend(ids);
        let skipped = prev.items.len() - fresh.len();
        if skipped > 0 {
            out.notes.push(format!(
                "{skipped} cran(s) déjà présent(s) à la position reportée : pas de doublon."
            ));
        }
        Ok(())
    }
}
