//! Zones de parage, de collage et de découpe interne (US-06.10, maroquinerie) : **surfaces**
//! marquées sur une pièce.
//!
//! Une zone ([`PieceZone`]) est une **bande** de largeur donnée le long d'un bord du contour
//! (liée à ce bord : la surface est **recalculée à chaque lecture**, jamais stockée) ou un
//! **polygone libre** (sommets stockés en mm). La bande est entre le bord et son décalage de
//! `width_mm` vers l'intérieur de la pièce (`pf_geometry::offset_chain`, coins en pointe), fermée
//! par deux segments perpendiculaires au bord à chaque extrémité ; elle suit donc le bord dans la
//! même étape annulable que la modification qui le change. Un déplacement qui rendrait la surface
//! invalide (décalage hors de la pièce, bord qui ne se suit plus, polygone qui se recoupe) **refuse
//! la commande de la pièce mère** : rien n'est stocké qui puisse devenir obsolète.
//!
//! Sont refusés sans trace : largeur hors de 0,5 à 100 mm, bord qui est le contour entier (une
//! bande est le long d'un **bord**), surface qui se recoupe, d'aire inférieure à 0,1 mm², ou qui
//! sort de la pièce (test par échantillonnage du contour : le décalage doit être dans la pièce).
//! Tout est en mm.

use crate::command::Outcome;
use crate::error::CoreError;
use crate::model::*;
use crate::offset::trace_of;
use crate::seam_pair::SeamSide;
use crate::stitch_line::{trace_at, trace_len};
use pf_geometry::{
    is_simple_polygon, offset_chain, quick::polygon_area_mm2, OffsetOptions, PointMm, Trace,
};
use serde::{Deserialize, Serialize};

/// Plus petite largeur de bande (mm) : en dessous, le décalage serait dans l'erreur
/// d'échantillonnage du contour (test « dans la pièce »).
pub const ZONE_MIN_WIDTH_MM: f64 = 0.5;
/// Plus grande largeur de bande (mm).
pub const ZONE_MAX_WIDTH_MM: f64 = 100.0;
/// Nombre maximal de zones par pièce.
pub const ZONE_MAX_PER_PIECE: usize = 50;
/// Nombre maximal de sommets d'un polygone libre.
pub const ZONE_MAX_POINTS: usize = 200;
/// Plus petite aire d'une zone (mm²).
pub const ZONE_MIN_AREA_MM2: f64 = 0.1;
/// Pas d'échantillonnage d'un tracé courbe (mm).
const SAMPLE_MM: f64 = 1.0;
/// Écart maximal toléré entre deux morceaux du trajet décalé (mm).
const GAP_MM: f64 = 1e-4;

/// Nature de la zone : le motif et l'étiquette d'export en dépendent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ZoneKind {
    /// Parage : amincissement du cuir.
    Skive,
    /// Collage.
    Glue,
    /// Découpe interne.
    InnerCut,
}

impl ZoneKind {
    pub const ALL: [ZoneKind; 3] = [ZoneKind::Skive, ZoneKind::Glue, ZoneKind::InnerCut];

    /// Identifiant stable, indépendant de la langue.
    pub fn key(self) -> &'static str {
        match self {
            ZoneKind::Skive => "skive",
            ZoneKind::Glue => "glue",
            ZoneKind::InnerCut => "inner_cut",
        }
    }
}

/// Forme de la zone (clé `type`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ZoneShape {
    /// Bande le long d'un bord : tracés **consécutifs** du contour (jamais le contour entier) et
    /// largeur vers l'intérieur (mm).
    Band { edges: Vec<EntityId>, width_mm: f64 },
    /// Polygone libre : sommets en mm (au moins 3, simple, dans la pièce).
    Polygon { points: Vec<PointMm> },
}

/// Paramètres d'une zone (création et remplacement).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ZoneSpec {
    pub kind: ZoneKind,
    pub shape: ZoneShape,
}

/// Zone stockée dans [`Piece::zones`] : sa **définition** seulement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PieceZone {
    pub id: ZoneId,
    pub kind: ZoneKind,
    pub shape: ZoneShape,
}

impl PieceZone {
    /// Paramètres de la zone (sans identifiant).
    pub fn spec(&self) -> ZoneSpec {
        ZoneSpec {
            kind: self.kind,
            shape: self.shape.clone(),
        }
    }
}

/// Zone calculée (lecture ; rien n'est stocké).
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedZone {
    pub id: ZoneId,
    pub kind: ZoneKind,
    /// Contour fermé de la surface (le dernier sommet n'est pas répété).
    pub polygon: Vec<PointMm>,
    pub area_mm2: f64,
    /// Largeur saisie d'une bande ; `None` pour un polygone libre.
    pub width_mm: Option<f64>,
}

fn bad(msg: impl Into<String>) -> CoreError {
    CoreError::Invalid(msg.into())
}

/// Points d'un tracé : deux pour un segment, échantillon à `SAMPLE_MM` sinon.
fn sample_trace(t: &Trace) -> Result<Vec<PointMm>, CoreError> {
    if let Trace::Line(a, b) = t {
        return Ok(vec![*a, *b]);
    }
    let len = trace_len(t)?;
    let n = ((len / SAMPLE_MM).ceil() as usize).clamp(8, 400);
    (0..=n)
        .map(|i| trace_at(t, len * i as f64 / n as f64).map(|(p, _)| p))
        .collect()
}

fn sample_chain(chain: &[Trace]) -> Result<Vec<PointMm>, CoreError> {
    let mut pts: Vec<PointMm> = Vec::new();
    for t in chain {
        for p in sample_trace(t)? {
            if pts
                .last()
                .is_none_or(|l| pf_geometry::distance_mm(*l, p) > 1e-9)
            {
                pts.push(p);
            }
        }
    }
    Ok(pts)
}

impl ZoneSpec {
    /// Contrôles qui ne dépendent pas de la géométrie.
    pub fn check_values(&self) -> Result<(), CoreError> {
        match &self.shape {
            ZoneShape::Band { edges, width_mm } => {
                if edges.is_empty() {
                    return Err(bad("une bande suit au moins un tracé du contour"));
                }
                if !width_mm.is_finite()
                    || !(ZONE_MIN_WIDTH_MM..=ZONE_MAX_WIDTH_MM).contains(width_mm)
                {
                    return Err(bad(format!(
                        "la largeur d'une bande doit être un nombre fini compris entre \
                         {ZONE_MIN_WIDTH_MM} et {ZONE_MAX_WIDTH_MM} mm"
                    )));
                }
            }
            ZoneShape::Polygon { points } => {
                if !(3..=ZONE_MAX_POINTS).contains(&points.len()) {
                    return Err(bad(format!(
                        "un polygone de zone a de 3 à {ZONE_MAX_POINTS} sommets"
                    )));
                }
                for p in points {
                    p.validate()?;
                }
                let n = points.len();
                if (0..n).any(|i| points[i] == points[(i + 1) % n]) {
                    return Err(bad(
                        "deux sommets consécutifs d'un polygone de zone sont confondus",
                    ));
                }
            }
        }
        Ok(())
    }
}

impl Project {
    /// Bande le long d'un bord : (surface fermée, trajet décalé).
    fn zone_band(
        &self,
        piece: PieceId,
        edges: &[EntityId],
        width: f64,
    ) -> Result<(Vec<PointMm>, Vec<PointMm>), CoreError> {
        let what = "zone : bord";
        let side = SeamSide {
            piece,
            edges: edges.to_vec(),
        };
        side.check(self, what)?;
        let frame = self.march_frame(&side, what)?;
        if frame.whole {
            return Err(bad(
                "une bande suit un bord, pas le contour entier : choisissez un bord ou un polygone",
            ));
        }
        let sheet = self.sheet(frame.sheet)?;
        let mut chain = Vec::with_capacity(frame.edges.len());
        for &(entity, rev, _, _) in &frame.edges {
            chain.push(trace_of(sheet.shape(entity)?, rev));
        }
        // L'intérieur est à gauche d'un contour anti-horaire.
        let d = if frame.clockwise { -width } else { width };
        let inner = offset_chain(&chain, d, &OffsetOptions::default()).map_err(|e| {
            bad(format!(
                "la largeur ne donne pas de bande exploitable ({e})"
            ))
        })?;
        if inner.is_empty() {
            return Err(bad("la largeur ne donne pas de bande exploitable"));
        }
        let mut ends: Vec<(PointMm, PointMm)> = Vec::new();
        for t in &inner {
            let len = trace_len(t)?;
            ends.push((trace_at(t, 0.0)?.0, trace_at(t, len)?.0));
        }
        for w in ends.windows(2) {
            if pf_geometry::distance_mm(w[0].1, w[1].0) > GAP_MM {
                return Err(bad(
                    "la largeur sépare la bande en plusieurs morceaux (forme trop étroite)",
                ));
            }
        }
        let outer_pts = sample_chain(&chain)?;
        let inner_pts = sample_chain(&inner)?;
        let mut polygon = outer_pts;
        polygon.extend(inner_pts.iter().rev().copied());
        Ok((polygon, inner_pts))
    }

    /// Surface d'une zone (lecture pure) avec tous les contrôles : refus expliqué si le bord ne la
    /// porte plus, si la surface se recoupe, est trop petite ou sort de la pièce.
    fn zone_polygon(&self, piece: PieceId, spec: &ZoneSpec) -> Result<Vec<PointMm>, CoreError> {
        spec.check_values()?;
        let lasso = self.outline_lasso(piece)?;
        let (mut polygon, inside): (Vec<PointMm>, Vec<PointMm>) = match &spec.shape {
            ZoneShape::Band { edges, width_mm } => {
                let (polygon, inner) = self.zone_band(piece, edges, *width_mm)?;
                (polygon, inner)
            }
            ZoneShape::Polygon { points } => {
                let mut ring = points.clone();
                ring.push(points[0]);
                (points.clone(), ring)
            }
        };
        // Sommets consécutifs confondus retirés (fermeture comprise).
        polygon.dedup_by(|b, a| pf_geometry::distance_mm(*a, *b) <= 1e-9);
        while polygon.len() > 1
            && pf_geometry::distance_mm(polygon[0], polygon[polygon.len() - 1]) <= 1e-9
        {
            polygon.pop();
        }
        if !is_simple_polygon(&polygon) {
            return Err(bad("la surface de la zone se recoupe ou est dégénérée"));
        }
        let area = polygon_area_mm2(&polygon);
        if !area.is_finite() || area < ZONE_MIN_AREA_MM2 {
            return Err(bad(format!(
                "la surface de la zone est trop petite (moins de {ZONE_MIN_AREA_MM2} mm²)"
            )));
        }
        if !lasso.contains_polyline(&inside) {
            return Err(bad("la zone sortirait de la pièce"));
        }
        Ok(polygon)
    }

    fn zone_ref(&self, piece: PieceId, zone: ZoneId) -> Result<&PieceZone, CoreError> {
        let p = self.piece(piece)?;
        p.zones.iter().find(|z| z.id == zone).ok_or_else(|| {
            bad(format!(
                "la zone {} n'existe pas sur la pièce « {} »",
                zone.0, p.name
            ))
        })
    }

    /// Surface d'une zone de la pièce (lecture pure ; refus expliqué si elle n'est plus valable).
    pub fn zone_layout(&self, piece: PieceId, zone: ZoneId) -> Result<ResolvedZone, CoreError> {
        let z = self.zone_ref(piece, zone)?;
        let polygon = self.zone_polygon(piece, &z.spec())?;
        Ok(ResolvedZone {
            id: z.id,
            kind: z.kind,
            area_mm2: polygon_area_mm2(&polygon),
            width_mm: match &z.shape {
                ZoneShape::Band { width_mm, .. } => Some(*width_mm),
                ZoneShape::Polygon { .. } => None,
            },
            polygon,
        })
    }

    /// Zones d'une pièce avec leur surface (lecture pure) ; une zone qui n'est plus calculable
    /// est omise (impossible dans un projet valide).
    pub fn piece_zones(&self, piece: PieceId) -> Vec<ResolvedZone> {
        let Ok(p) = self.piece(piece) else {
            return Vec::new();
        };
        p.zones
            .iter()
            .filter_map(|z| self.zone_layout(piece, z.id).ok())
            .collect()
    }

    /// Zones de toutes les pièces dont le contour est sur la feuille (export et affichage).
    pub fn sheet_zones(&self, sheet: SheetId) -> Vec<ResolvedZone> {
        self.pieces_on(sheet)
            .into_iter()
            .flat_map(|p| self.piece_zones(p.id))
            .collect()
    }

    /// Aperçu en **lecture seule** d'une zone qui serait créée : mêmes refus que
    /// [`Command::AddPieceZone`](crate::Command::AddPieceZone) (la commande est rejouée sur une
    /// copie du projet).
    pub fn zone_preview(&self, piece: PieceId, spec: &ZoneSpec) -> Result<ResolvedZone, CoreError> {
        let mut copy = self.clone();
        let out = copy.apply(crate::Command::AddPieceZone {
            piece,
            zone: spec.clone(),
        })?;
        let id = *out.zones.first().ok_or_else(|| bad("aperçu impossible"))?;
        copy.zone_layout(piece, id)
    }

    /// Contrôle des zones d'une pièce (ouverture et après chaque commande) : nombre, valeurs,
    /// puis calcul complet de chaque surface.
    pub(crate) fn validate_zones(&self, piece: &Piece) -> Result<(), CoreError> {
        if piece.zones.is_empty() {
            return Ok(());
        }
        let name = &piece.name;
        if piece.zones.len() > ZONE_MAX_PER_PIECE {
            return Err(bad(format!(
                "la pièce « {name} » porte plus de {ZONE_MAX_PER_PIECE} zones"
            )));
        }
        for (i, z) in piece.zones.iter().enumerate() {
            if piece.zones[..i].iter().any(|o| o.id == z.id) {
                return Err(bad(format!(
                    "la pièce « {name} » porte deux fois la zone {}",
                    z.id.0
                )));
            }
        }
        for z in &piece.zones {
            self.zone_polygon(piece.id, &z.spec())
                .map_err(|e| bad(format!("zone {} de la pièce « {name} » : {e}", z.id.0)))?;
        }
        Ok(())
    }

    pub(crate) fn add_piece_zone(
        &mut self,
        piece: PieceId,
        spec: ZoneSpec,
        out: &mut Outcome,
    ) -> Result<(), CoreError> {
        spec.check_values()?;
        let p = self.piece(piece)?;
        if p.zones.len() >= ZONE_MAX_PER_PIECE {
            return Err(bad(format!(
                "la pièce « {} » porte déjà {ZONE_MAX_PER_PIECE} zones",
                p.name
            )));
        }
        self.zone_polygon(piece, &spec)?;
        let id = ZoneId(self.alloc_id());
        self.piece_mut(piece)?.zones.push(PieceZone {
            id,
            kind: spec.kind,
            shape: spec.shape,
        });
        out.zones.push(id);
        Ok(())
    }

    pub(crate) fn set_piece_zone(
        &mut self,
        piece: PieceId,
        zone: ZoneId,
        spec: ZoneSpec,
    ) -> Result<(), CoreError> {
        spec.check_values()?;
        self.zone_ref(piece, zone)?;
        self.zone_polygon(piece, &spec)?;
        let p = self.piece_mut(piece)?;
        if let Some(z) = p.zones.iter_mut().find(|z| z.id == zone) {
            z.kind = spec.kind;
            z.shape = spec.shape;
        }
        Ok(())
    }

    pub(crate) fn remove_piece_zone(
        &mut self,
        piece: PieceId,
        zone: ZoneId,
    ) -> Result<(), CoreError> {
        self.zone_ref(piece, zone)?;
        self.piece_mut(piece)?.zones.retain(|z| z.id != zone);
        Ok(())
    }

    /// Retire les bandes dont un tracé n'est plus dans le contour (tracé supprimé, coupé ou
    /// contour réécrit), avec remarque. Ne touche que les zones qui existaient avant la commande.
    pub(crate) fn prune_dead_zones(&mut self, before: &Project, out: &mut Outcome) {
        if self.pieces.iter().all(|p| p.zones.is_empty()) {
            return;
        }
        for pi in 0..self.pieces.len() {
            let p = &self.pieces[pi];
            if p.zones.is_empty() {
                continue;
            }
            let old = before.pieces.iter().find(|o| o.id == p.id);
            let gone: Vec<ZoneId> = p
                .zones
                .iter()
                .filter(|z| match &z.shape {
                    ZoneShape::Band { edges, .. } => {
                        old.is_some_and(|o| o.zones.contains(z))
                            && edges
                                .iter()
                                .any(|e| !p.outline.iter().any(|o| o.entity == *e))
                    }
                    ZoneShape::Polygon { .. } => false,
                })
                .map(|z| z.id)
                .collect();
            if gone.is_empty() {
                continue;
            }
            let p = &mut self.pieces[pi];
            p.zones.retain(|z| !gone.contains(&z.id));
            for g in &gone {
                out.notes.push(format!(
                    "La zone {} de la pièce « {} » dépendait d'un bord qui n'est plus dans le \
                     contour : elle est retirée.",
                    g.0, p.name
                ));
            }
        }
    }
}
