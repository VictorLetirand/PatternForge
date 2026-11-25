//! Ampleur par ouverture en éventail (US-07.4, décision 300).
//!
//! `Command::AddPieceFlare` coupe une pièce selon N lignes **perpendiculaires à un bord fixe** (un
//! côté droit du contour) et fait tourner chaque morceau autour du **pivot** de sa ligne, situé sur
//! le bord fixe, pour écarter le bord opposé de la valeur d'**ouverture** de la ligne (corde entre
//! les deux copies de l'extrémité haute). Les morceaux se suivent : le morceau d'une ligne entraîne
//! ceux qui sont au-delà. Le vide ouvert est rempli (la pièce élargie contient le coin de tissu
//! ajouté), si bien que : la longueur du **bord fixe est conservée** (somme des morceaux, le pivot
//! reste commun), chaque morceau garde sa forme (longueurs de ligne de coupe conservées), le
//! périmètre gagne la somme des ouvertures et l'aire `Σ ½ L² sin θ`.
//!
//! La pièce est **reconstruite sur une feuille dédiée** (même identifiant, construction d'origine
//! intacte, comme US-07.2 et 07.3). Seule la **définition** de chaque ligne (pivot et les deux
//! extrémités hautes) est stockée dans `Piece.flares` ; ouverture, angle et longueur se déduisent de
//! ces trois points. Convention d'atelier (non validée) : pivot au pied de la ligne sur le bord
//! fixe, lignes perpendiculaires au bord, ouverture mesurée en haut, valeur « bas » nulle.
//!
//! Non repris, refusé sans trace : pièce dérivée liée, pièce portant des pinces, des plis ou déjà
//! une ampleur (une seule opération par pièce).

use crate::command::Outcome;
use crate::error::CoreError;
use crate::model::*;
use crate::piece_split_join::geoms_of;
use pf_geometry::dart::rotate_point;
use pf_geometry::flare::{chain_self_touches, fan_chain};
use pf_geometry::pleat::merge_collinear;
use pf_geometry::region::EdgeGeom;
use pf_geometry::split::{chain_length_mm, chain_signed_area_mm2, edge_sample};
use pf_geometry::{distance_mm, Lasso, PointMm, MAX_COORD_MM};
use serde::{Deserialize, Serialize};

/// Nombre maximal de lignes d'une ampleur (et de lignes stockées par pièce).
pub const FLARE_MAX_LINES: usize = 50;
/// Plus petite ouverture d'une ligne (mm).
pub const FLARE_MIN_OPENING_MM: f64 = 0.01;
/// Plus grande ouverture d'une ligne (mm).
pub const FLARE_MAX_OPENING_MM: f64 = 2000.0;
/// Distance minimale (mm) d'une ligne aux extrémités du bord fixe et entre deux lignes.
pub const FLARE_MIN_SPACING_MM: f64 = 1e-3;
/// Tolérance de comparaison des longueurs et des aires (mm, relative pour les aires).
const CHECK_TOL: f64 = 1e-6;

/// Ligne demandée : position le long du bord fixe (mm depuis `fixed_from`) et ouverture en haut.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlareLineSpec {
    pub at_mm: f64,
    pub opening_mm: f64,
}

/// Ligne d'ampleur d'une pièce (définition stockée) : pivot et extrémités hautes des deux lèvres
/// de l'ouverture, dans le repère de la feuille de la pièce.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PieceFlare {
    pub id: FlareId,
    pub hinge_x_mm: f64,
    pub hinge_y_mm: f64,
    /// Extrémité haute de la lèvre qui est restée en place.
    pub stay_x_mm: f64,
    pub stay_y_mm: f64,
    /// Extrémité haute de la lèvre qui a tourné.
    pub moved_x_mm: f64,
    pub moved_y_mm: f64,
}

impl PieceFlare {
    pub fn hinge(&self) -> PointMm {
        PointMm::new(self.hinge_x_mm, self.hinge_y_mm)
    }
    pub fn stay_end(&self) -> PointMm {
        PointMm::new(self.stay_x_mm, self.stay_y_mm)
    }
    pub fn moved_end(&self) -> PointMm {
        PointMm::new(self.moved_x_mm, self.moved_y_mm)
    }
    /// Ouverture en haut (corde entre les deux extrémités, mm).
    pub fn opening_mm(&self) -> f64 {
        distance_mm(self.stay_end(), self.moved_end())
    }
    /// Longueur de la ligne de coupe (mm).
    pub fn length_mm(&self) -> f64 {
        distance_mm(self.hinge(), self.stay_end())
    }
    /// Angle d'ouverture (degrés).
    pub fn angle_deg(&self) -> f64 {
        2.0 * (self.opening_mm() / (2.0 * self.length_mm()))
            .min(1.0)
            .asin()
            .to_degrees()
    }
}

/// Côté droit du contour proposé comme bord fixe.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FlareSide {
    pub from: PointMm,
    pub to: PointMm,
    pub length_mm: f64,
}

/// Une ligne de l'aperçu.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FlareLineInfo {
    pub at_mm: f64,
    pub opening_mm: f64,
    pub length_mm: f64,
    pub angle_deg: f64,
}

/// Aperçu d'une ampleur (mesures **observées** sur le résultat de la commande rejouée).
#[derive(Debug, Clone, PartialEq)]
pub struct FlarePreview {
    pub fixed_before_mm: f64,
    pub fixed_after_mm: f64,
    /// Somme des ouvertures : ce que gagne le bord ouvert.
    pub open_gain_mm: f64,
    pub perimeter_before_mm: f64,
    pub perimeter_after_mm: f64,
    pub area_before_mm2: f64,
    pub area_after_mm2: f64,
    /// Lignes par position croissante.
    pub lines: Vec<FlareLineInfo>,
}

/// Positions à répartition égale de `n` lignes sur un bord de `length_mm` : `k × L / (n + 1)`.
pub fn flare_even_positions(length_mm: f64, n: usize) -> Vec<f64> {
    (1..=n)
        .map(|k| length_mm * k as f64 / (n as f64 + 1.0))
        .collect()
}

fn invalid(msg: impl Into<String>) -> CoreError {
    CoreError::Invalid(msg.into())
}

struct Computed {
    orig: Piece,
    index: usize,
    chain: Vec<(EdgeGeom, PathRole)>,
    /// Lignes par position croissante : pivot, extrémité fixe, extrémité tournée.
    flares: Vec<[PointMm; 3]>,
    carry: crate::piece_split_join::Carry,
    notes: Vec<String>,
    preview: FlarePreview,
}

impl Project {
    /// Validation structurelle des lignes d'ampleur stockées d'une pièce.
    pub(crate) fn validate_flares(piece: &Piece) -> Result<(), CoreError> {
        if piece.flares.len() > FLARE_MAX_LINES {
            return Err(invalid(format!(
                "la pièce « {} » porte plus de {FLARE_MAX_LINES} lignes d'ampleur",
                piece.name
            )));
        }
        for f in &piece.flares {
            let coords = [
                f.hinge_x_mm,
                f.hinge_y_mm,
                f.stay_x_mm,
                f.stay_y_mm,
                f.moved_x_mm,
                f.moved_y_mm,
            ];
            if coords
                .iter()
                .any(|v| !v.is_finite() || v.abs() > MAX_COORD_MM)
            {
                return Err(invalid(format!(
                    "une ligne d'ampleur de la pièce « {} » a une coordonnée non finie ou hors de \
                     l'espace de travail",
                    piece.name
                )));
            }
            let (l1, l2) = (
                distance_mm(f.hinge(), f.stay_end()),
                distance_mm(f.hinge(), f.moved_end()),
            );
            if l1 <= pf_geometry::EPSILON_MM
                || (l1 - l2).abs() > CHECK_TOL
                || f.opening_mm() <= pf_geometry::EPSILON_MM
            {
                return Err(invalid(format!(
                    "une ligne d'ampleur de la pièce « {} » est dégénérée : ses deux lèvres doivent \
                     partir du pivot avec la même longueur et être écartées",
                    piece.name
                )));
            }
        }
        Ok(())
    }

    /// Lignes d'ampleur de la pièce (lecture pure des définitions stockées).
    pub fn piece_flares(&self, piece: PieceId) -> Vec<PieceFlare> {
        self.piece(piece)
            .map(|p| p.flares.clone())
            .unwrap_or_default()
    }

    /// Côtés droits du contour (candidats au bord fixe), dans le sens anti-horaire. Vide pour une
    /// pièce inconnue, dérivée liée ou au contour inutilisable.
    pub fn piece_flare_sides(&self, piece: PieceId) -> Vec<FlareSide> {
        let Ok(chain) = self.piece_chain(piece) else {
            return Vec::new();
        };
        chain
            .iter()
            .filter_map(|e| match e.geom {
                EdgeGeom::Line(a, b) if distance_mm(a, b) > pf_geometry::EPSILON_MM => {
                    Some(FlareSide {
                        from: a,
                        to: b,
                        length_mm: distance_mm(a, b),
                    })
                }
                _ => None,
            })
            .collect()
    }

    /// Aperçu de `Command::AddPieceFlare` : même calcul, rien n'est modifié. Mêmes refus.
    pub fn flare_preview(
        &self,
        piece: PieceId,
        fixed_from: PointMm,
        fixed_to: PointMm,
        lines: &[FlareLineSpec],
    ) -> Result<FlarePreview, CoreError> {
        Ok(self
            .compute_flare(piece, fixed_from, fixed_to, lines)?
            .preview)
    }

    fn compute_flare(
        &self,
        piece: PieceId,
        fixed_from: PointMm,
        fixed_to: PointMm,
        lines: &[FlareLineSpec],
    ) -> Result<Computed, CoreError> {
        fixed_from.validate()?;
        fixed_to.validate()?;
        if lines.is_empty() || lines.len() > FLARE_MAX_LINES {
            return Err(invalid(format!(
                "le nombre de lignes ({}) doit être compris entre 1 et {FLARE_MAX_LINES}",
                lines.len()
            )));
        }
        for l in lines {
            if !l.opening_mm.is_finite()
                || !(FLARE_MIN_OPENING_MM..=FLARE_MAX_OPENING_MM).contains(&l.opening_mm)
            {
                return Err(invalid(format!(
                    "l'ouverture d'une ligne ({}) doit être un nombre fini compris entre \
                     {FLARE_MIN_OPENING_MM} et {FLARE_MAX_OPENING_MM} mm",
                    l.opening_mm
                )));
            }
            if !l.at_mm.is_finite() {
                return Err(invalid("la position d'une ligne n'est pas un nombre fini"));
            }
        }
        let orig = self.piece(piece)?.clone();
        let index = self
            .pieces
            .iter()
            .position(|p| p.id == piece)
            .ok_or(CoreError::UnknownPiece(piece))?;
        if orig.derived.is_some() {
            return Err(invalid(format!(
                "la pièce « {} » est une pièce dérivée liée à sa mère : cassez le lien avant d'y \
                 poser une ampleur",
                orig.name
            )));
        }
        if !orig.flares.is_empty() {
            return Err(invalid(format!(
                "la pièce « {} » porte déjà une ampleur : une seule opération par pièce",
                orig.name
            )));
        }
        if !orig.darts.is_empty() || !orig.pleats.is_empty() {
            return Err(invalid(format!(
                "la pièce « {} » porte des pinces ou des plis : une ampleur reconstruit la pièce \
                 sur une nouvelle feuille et ne les reprend pas, supprimez-les d'abord",
                orig.name
            )));
        }
        let chain = self.piece_chain(piece)?;
        // Bord fixe : un côté droit du contour.
        let tol = pf_geometry::flare::FLARE_HINGE_TOLERANCE_MM;
        let (fixed_index, a, b) = chain
            .iter()
            .enumerate()
            .find_map(|(i, e)| match e.geom {
                EdgeGeom::Line(s, t) => {
                    if distance_mm(s, fixed_from) <= tol && distance_mm(t, fixed_to) <= tol {
                        Some((i, s, t))
                    } else if distance_mm(t, fixed_from) <= tol && distance_mm(s, fixed_to) <= tol {
                        Some((i, t, s))
                    } else {
                        None
                    }
                }
                _ => None,
            })
            .ok_or_else(|| {
                invalid("le bord fixe doit être un côté droit du contour de la pièce")
            })?;
        let fixed_len = distance_mm(a, b);
        let dir = (b - a)
            .unit()
            .map_err(|_| invalid("le bord fixe est dégénéré"))?;
        let mut sorted: Vec<FlareLineSpec> = lines.to_vec();
        sorted.sort_by(|x, y| x.at_mm.total_cmp(&y.at_mm));
        for (i, l) in sorted.iter().enumerate() {
            if l.at_mm < FLARE_MIN_SPACING_MM || l.at_mm > fixed_len - FLARE_MIN_SPACING_MM {
                return Err(invalid(format!(
                    "la position d'une ligne ({} mm) doit être strictement à l'intérieur du bord \
                     fixe (de 0 à {fixed_len:.3} mm)",
                    l.at_mm
                )));
            }
            if i > 0 && l.at_mm - sorted[i - 1].at_mm < FLARE_MIN_SPACING_MM {
                return Err(invalid("deux lignes sont à la même position"));
            }
        }
        let area_before = chain_signed_area_mm2(&geoms_of(&chain));
        let perimeter_before = chain_length_mm(&geoms_of(&chain));
        let mut cur: Vec<(EdgeGeom, PathRole, bool)> = chain
            .iter()
            .enumerate()
            .map(|(i, e)| (e.geom, e.role, i == fixed_index))
            .collect();
        let mut notes = Vec::new();
        let mut carry = self.carry_of_with(piece, &mut notes, false)?;
        let mut flares: Vec<[PointMm; 3]> = Vec::with_capacity(sorted.len());
        let mut infos = Vec::with_capacity(sorted.len());
        let mut expected_area = 0.0;
        // La ligne la plus éloignée d'abord : les plus proches restent à leur place d'origine et
        // entraînent ensuite tout ce qui est au-delà.
        for l in sorted.iter().rev() {
            let hinge = a + dir * l.at_mm;
            let geoms: Vec<EdgeGeom> = cur.iter().map(|c| c.0).collect();
            let fan = fan_chain(&geoms, hinge, dir, l.opening_mm)
                .map_err(|e| invalid(format!("l'ampleur ne peut pas être posée : {e}")))?;
            expected_area += 0.5 * fan.length_mm * fan.length_mm * fan.angle_rad.sin();
            let (h, cos, sin) = (fan.hinge, fan.cos, fan.sin);
            let mut last = (PathRole::default(), false);
            let mut next: Vec<(EdgeGeom, PathRole, bool)> = Vec::with_capacity(fan.edges.len());
            for e in &fan.edges {
                match e.source {
                    Some(s) => {
                        last = (cur[s].1, cur[s].2);
                        next.push((e.geom, last.0, last.1));
                    }
                    None => next.push((e.geom, last.0, false)),
                }
            }
            cur = next;
            // Les lignes déjà posées (plus éloignées) sont au-delà : elles tournent aussi.
            for f in &mut flares {
                for q in f.iter_mut() {
                    *q = rotate_point(*q, h, cos, sin);
                }
            }
            flares.push([fan.hinge, fan.stay_end, fan.moved_end]);
            infos.push(FlareLineInfo {
                at_mm: l.at_mm,
                opening_mm: l.opening_mm,
                length_mm: fan.length_mm,
                angle_deg: fan.angle_rad.to_degrees(),
            });
            let mut poly = Vec::new();
            for g in &fan.moving {
                poly.extend(edge_sample(g, 48));
            }
            let region = Lasso::new(&poly)
                .ok_or_else(|| invalid("la partie à tourner est dégénérée : ampleur impossible"))?;
            carry = carry.moved_in(
                &region,
                &|p| rotate_point(p, h, cos, sin),
                &|g| pf_geometry::dart::rotate_edge(g, h, cos, sin),
                "ligne d'ampleur",
                &orig.name,
                &mut notes,
            );
        }
        flares.reverse();
        infos.reverse();
        let all: Vec<EdgeGeom> = cur.iter().map(|c| c.0).collect();
        if chain_self_touches(&all) {
            return Err(invalid(
                "l'ampleur ne peut pas être posée : les morceaux se chevaucheraient ou se \
                 toucheraient après l'ouverture (ouverture trop grande ou lignes trop proches)",
            ));
        }
        let area_after = chain_signed_area_mm2(&all);
        if (area_after - area_before - expected_area).abs() > CHECK_TOL * area_after.abs().max(1.0)
        {
            return Err(invalid(
                "incohérence interne : l'aire du contour ouvert ne correspond pas aux ouvertures",
            ));
        }
        let gain: f64 = sorted.iter().map(|l| l.opening_mm).sum();
        let perimeter_after = chain_length_mm(&all);
        if (perimeter_after - perimeter_before - gain).abs() > CHECK_TOL * perimeter_after.max(1.0)
        {
            return Err(invalid(
                "incohérence interne : le périmètre du contour ouvert ne correspond pas aux \
                 ouvertures",
            ));
        }
        let fixed_after: f64 = cur.iter().filter(|c| c.2).map(|c| c.0.length_mm()).sum();
        if (fixed_after - fixed_len).abs() > CHECK_TOL * fixed_len.max(1.0) {
            return Err(invalid(
                "incohérence interne : la longueur du bord fixe n'est pas conservée",
            ));
        }
        let merged = merge_collinear(
            cur.iter()
                .map(|c| (c.0, (c.1, c.2)))
                .collect::<Vec<(EdgeGeom, (PathRole, bool))>>(),
        );
        let chain_out: Vec<(EdgeGeom, PathRole)> =
            merged.into_iter().map(|(g, (r, _))| (g, r)).collect();
        Ok(Computed {
            orig,
            index,
            chain: chain_out,
            flares,
            carry,
            notes,
            preview: FlarePreview {
                fixed_before_mm: fixed_len,
                fixed_after_mm: fixed_after,
                open_gain_mm: gain,
                perimeter_before_mm: perimeter_before,
                perimeter_after_mm: perimeter_after,
                area_before_mm2: area_before,
                area_after_mm2: area_after,
                lines: infos,
            },
        })
    }

    /// Pose l'ampleur (voir `Command::AddPieceFlare`).
    pub(crate) fn add_piece_flare(
        &mut self,
        piece: PieceId,
        fixed_from: PointMm,
        fixed_to: PointMm,
        lines: &[FlareLineSpec],
        out: &mut Outcome,
    ) -> Result<(), CoreError> {
        let c = self.compute_flare(piece, fixed_from, fixed_to, lines)?;
        let Computed {
            orig,
            index,
            chain,
            flares,
            carry,
            mut notes,
            preview,
        } = c;
        let defs: Vec<PieceFlare> = flares
            .iter()
            .map(|f| PieceFlare {
                id: FlareId(self.alloc_id()),
                hinge_x_mm: f[0].x,
                hinge_y_mm: f[0].y,
                stay_x_mm: f[1].x,
                stay_y_mm: f[1].y,
                moved_x_mm: f[2].x,
                moved_y_mm: f[2].y,
            })
            .collect();
        let joints: Vec<PointMm> = chain.iter().map(|(g, _)| g.start()).collect();
        let sid = self.new_result_sheet(&orig.name);
        let outline = self.build_closed_chain(sid, &chain, &joints)?;
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
            flares: defs.clone(),
            link: PieceLink::Detached,
            ..orig.clone()
        };
        self.pieces[index] = new_piece;
        self.piece_chain_any(piece)
            .map_err(|e| invalid(format!("l'ampleur produit un contour inutilisable : {e}")))?;
        out.pieces.push(piece);
        out.sheet = Some(sid);
        out.flares.extend(defs.iter().map(|d| d.id));
        notes.dedup();
        notes.insert(
            0,
            format!(
                "{} ligne(s) d'ampleur posée(s) sur la pièce « {} » (bord fixe {:.3} mm conservé, \
                 bord ouvert + {:.3} mm) ; la pièce est reconstruite sur une nouvelle feuille, la \
                 construction d'origine n'est pas modifiée.",
                defs.len(),
                orig.name,
                preview.fixed_after_mm,
                preview.open_gain_mm
            ),
        );
        out.notes.extend(notes);
        Ok(())
    }
}
