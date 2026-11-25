//! US-04.6 — séparer une pièce par une ligne, assembler deux pièces le long de
//! deux bords (décisions 135 à 146).
//!
//! Les deux opérations sont des **commandes atomiques** (`Command::SplitPiece`,
//! `Command::JoinPieces`) : une seule étape annulable, refus sans trace. Le résultat
//! est toujours fait de **pièces détachées** qui vivent chacune sur sa propre feuille
//! (copie de la géométrie, comme l'extraction détachée) : la construction des feuilles
//! d'origine n'est jamais modifiée, et la pièce d'origine est remplacée dans la liste
//! des pièces (les feuilles d'origine restent). Aucun champ de modèle n'est ajouté :
//! le schéma 3 est inchangé.
//!
//! Les marges de couture (E05) n'existent pas encore : rien n'est recalculé ni annoncé,
//! le recalcul sera branché par E05.

use crate::command::Outcome;
use crate::contour::contour_from_entities;
use crate::error::CoreError;
use crate::model::*;
use crate::offset::trace_of;
use crate::piece_buttons::{ButtonElement, ButtonGroup};
use crate::piece_grain::{ExtraGrainLine, GrainDirection, GrainSense, PieceGrain};
use crate::piece_marking::{MarkingAnchor, MarkingShape, PieceMarking};
use crate::sheet_copy::unique_name;
use pf_geometry::region::{trace_geometry, EdgeGeom};
use pf_geometry::split::{
    chain_length_mm, chain_signed_area_mm2, cut_chain_by_line, edge_sample, max_deviation_mm, Rigid,
};
use pf_geometry::{distance_mm, Lasso, PointMm, ARC_RADIUS_TOLERANCE_MM};
use std::collections::HashSet;

/// Tolérance d'écart de longueur proposée par défaut à l'assemblage (mm).
pub const DEFAULT_JOIN_TOLERANCE_MM: f64 = 0.5;
/// Plus grande tolérance acceptée (mm) : au-delà, ce n'est plus un assemblage.
pub const MAX_JOIN_TOLERANCE_MM: f64 = 10.0;
/// Deux extrémités plus proches que cela (mm) sont confondues.
const COINCIDENT_MM: f64 = 1e-6;
/// Pas d'échantillonnage des courbes pour l'aperçu et les tests d'appartenance.
const SAMPLES: usize = 48;

/// Arête de la chaîne d'une pièce (géométrie élémentaire dans le sens du parcours).
#[derive(Debug, Clone)]
pub(crate) struct ChainEdge {
    pub(crate) geom: EdgeGeom,
    pub(crate) role: PathRole,
    /// Tracé de la pièce dont elle provient.
    pub(crate) source: EntityId,
}

/// Ligne copiable (un seul tracé : segment, Bézier ou arc).
#[derive(Debug, Clone)]
struct CarriedLine {
    role: PathRole,
    geom: EdgeGeom,
}

/// Droit-fil et éléments internes à reporter sur les nouvelles pièces.
#[derive(Debug, Default)]
pub(crate) struct Carry {
    grain: Option<CarriedLine>,
    /// Réglages avancés du droit-fil (US-06.5), liés à `grain` : sens, tolérance et lignes
    /// supplémentaires. Les directions sont figées à la flèche calculée (aucune référence).
    grain_set: Option<CarriedGrain>,
    lines: Vec<(InternalKind, CarriedLine)>,
    marks: Vec<(InternalKind, PointMm)>,
    /// Marquages (US-06.3) : forme et position résolue ; reportés sur un point libre.
    markings: Vec<(MarkingShape, PointMm)>,
    /// Groupes de boutons (US-06.4) : paramètres et positions des deux points ; reportés
    /// sur deux points libres.
    buttons: Vec<CarriedButtons>,
}

impl Carry {
    /// Transfert de pince (US-07.2) : ce qui est entièrement dans `region` (partie mobile)
    /// tourne autour de `apex` ; ce qui ne la touche pas reste en place ; ce qui la traverse
    /// n'est pas repris (remarque dans `notes`, jamais perdu en silence).
    pub(crate) fn turned_in(
        self,
        region: &Lasso,
        (apex, cos, sin): (PointMm, f64, f64),
        piece_name: &str,
        notes: &mut Vec<String>,
    ) -> Carry {
        self.moved_in(
            region,
            &|p| pf_geometry::dart::rotate_point(p, apex, cos, sin),
            &|g| pf_geometry::dart::rotate_edge(g, apex, cos, sin),
            "ligne de pivot",
            piece_name,
            notes,
        )
    }

    /// Plis (US-07.3) : comme `turned_in` avec un déplacement quelconque de la partie `region`
    /// (`map_pt` / `map_edge`) ; `line_name` nomme la ligne qui sépare les parties dans les
    /// remarques.
    pub(crate) fn moved_in(
        self,
        region: &Lasso,
        map_pt: &dyn Fn(PointMm) -> PointMm,
        map_edge: &dyn Fn(&EdgeGeom) -> EdgeGeom,
        line_name: &str,
        piece_name: &str,
        notes: &mut Vec<String>,
    ) -> Carry {
        #[derive(PartialEq)]
        enum Zone {
            Fixed,
            Moved,
            Cut,
        }
        let zone = |pts: &[PointMm]| {
            if region.contains_polyline(pts) {
                Zone::Moved
            } else if region.touches_polyline(pts) {
                Zone::Cut
            } else {
                Zone::Fixed
            }
        };
        let turn = |l: &CarriedLine, z: &Zone| CarriedLine {
            role: l.role,
            geom: if *z == Zone::Moved {
                map_edge(&l.geom)
            } else {
                l.geom
            },
        };
        let turn_pt = |p: PointMm, z: &Zone| {
            if *z == Zone::Moved {
                map_pt(p)
            } else {
                p
            }
        };
        let mut out = Carry::default();
        let mut lost = 0usize;
        if let Some(g) = &self.grain {
            let z = zone(&dense_points(&g.geom));
            if z == Zone::Cut {
                notes.push(format!(
                    "Le droit-fil de la pièce « {piece_name} » traverse la {line_name} : il \
                     n'est pas repris (droit-fil manquant)."
                ));
            } else {
                out.grain = Some(turn(g, &z));
                out.grain_set = self.grain_set.as_ref().map(|cg| CarriedGrain {
                    sense: cg.sense,
                    tolerance: cg.tolerance,
                    extra: cg
                        .extra
                        .iter()
                        .filter_map(|(l, s)| {
                            let z = zone(&dense_points(&l.geom));
                            if z == Zone::Cut {
                                lost += 1;
                                None
                            } else {
                                Some((turn(l, &z), *s))
                            }
                        })
                        .collect(),
                });
            }
        }
        for (kind, l) in &self.lines {
            let z = zone(&dense_points(&l.geom));
            if z == Zone::Cut {
                lost += 1;
            } else {
                out.lines.push((*kind, turn(l, &z)));
            }
        }
        for (kind, at) in &self.marks {
            let z = zone(&[*at]);
            out.marks.push((*kind, turn_pt(*at, &z)));
        }
        for (shape, at) in &self.markings {
            let z = zone(&[*at]);
            // Une fente de poinçon (PH-4b) tourne avec la partie déplacée : la rotation est lue
            // sur l'image d'un vecteur unitaire.
            let shape = if z == Zone::Moved && matches!(shape, MarkingShape::Slot { .. }) {
                let (a, b) = (map_pt(*at), map_pt(PointMm::new(at.x + 1.0, at.y)));
                shape.rotated((b.y - a.y).atan2(b.x - a.x).to_degrees())
            } else {
                *shape
            };
            out.markings.push((shape, turn_pt(*at, &z)));
        }
        for c in &self.buttons {
            let z = zone(&[c.from, c.to]);
            if z == Zone::Cut {
                lost += 1;
            } else {
                out.buttons.push(CarriedButtons {
                    from: turn_pt(c.from, &z),
                    to: turn_pt(c.to, &z),
                    ..*c
                });
            }
        }
        if lost > 0 {
            notes.push(format!(
                "{lost} élément(s) de la pièce « {piece_name} » traversent la {line_name} : \
                 ils ne sont pas repris."
            ));
        }
        out
    }
}

/// Réglages du droit-fil à reporter (US-06.5).
#[derive(Debug, Clone)]
struct CarriedGrain {
    sense: GrainSense,
    tolerance: Option<f64>,
    extra: Vec<(CarriedLine, GrainSense)>,
}

/// Groupe de boutons à reporter : tout sauf les identifiants, avec les positions résolues
/// des deux points.
#[derive(Debug, Clone, Copy)]
struct CarriedButtons {
    count: u32,
    element: ButtonElement,
    edge_margin_mm: f64,
    from: PointMm,
    to: PointMm,
}

/// Droit-fil, éléments internes, marquages et groupes de boutons reportés sur une pièce
/// résultat.
pub(crate) type Carried = (
    Option<EntityId>,
    Vec<InternalElement>,
    Vec<PieceMarking>,
    Vec<ButtonGroup>,
    Option<PieceGrain>,
);

/// Aperçu d'une moitié.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HalfInfo {
    pub area_mm2: f64,
    pub perimeter_mm: f64,
}

/// Aperçu d'une séparation : points de coupe et mesures **observées** des deux moitiés.
#[derive(Debug, Clone, PartialEq)]
pub struct SplitPreview {
    /// Points où la droite traverse le contour, dans l'ordre le long de `a → b`.
    pub points: [PointMm; 2],
    /// Moitié à gauche de la droite orientée `a → b` (future pièce « 1/2 »).
    pub left: HalfInfo,
    /// Moitié à droite (future pièce « 2/2 »).
    pub right: HalfInfo,
}

/// Aperçu d'un assemblage. Les longueurs et l'écart sont **observés** ; rien n'est masqué.
#[derive(Debug, Clone, PartialEq)]
pub struct JoinPreview {
    pub first_length_mm: f64,
    pub second_length_mm: f64,
    /// Écart absolu entre les deux longueurs de bord (mm).
    pub gap_mm: f64,
    /// Écart de forme maximal entre les deux bords une fois superposés (mm).
    pub deviation_mm: f64,
    pub tolerance_mm: f64,
    /// Rotation appliquée à la seconde pièce (degrés).
    pub rotation_deg: f64,
    /// Contour de la seconde pièce après déplacement (polyligne fermée, repère de la première).
    pub moved_second: Vec<PointMm>,
    /// Aire et périmètre de la pièce assemblée, si l'assemblage est possible.
    pub result: Option<HalfInfo>,
    /// Raison pour laquelle la commande serait refusée ; `None` si elle réussirait.
    pub refusal: Option<String>,
}

/// Mesures d'un assemblage possible, avant tout contrôle de tolérance.
struct JoinMeasure {
    chain_a: Vec<ChainEdge>,
    chain_b: Vec<ChainEdge>,
    /// Bord choisi dans `chain_a` : début et nombre d'arêtes.
    range_a: (usize, usize),
    range_b: (usize, usize),
    rigid: Rigid,
    len_a: f64,
    len_b: f64,
    deviation_mm: f64,
    /// Début et fin du bord de A (sens du parcours de A).
    a_start: PointMm,
    a_end: PointMm,
}

impl JoinMeasure {
    fn gap_mm(&self) -> f64 {
        (self.len_a - self.len_b).abs()
    }

    fn check(&self, tolerance_mm: f64) -> Result<(), CoreError> {
        if self.gap_mm() > tolerance_mm + 1e-12 {
            return Err(CoreError::Invalid(format!(
                "les deux bords n'ont pas la même longueur : {:.3} mm et {:.3} mm, soit un écart de \
                 {:.3} mm (tolérance {:.3} mm)",
                self.len_a,
                self.len_b,
                self.gap_mm(),
                tolerance_mm
            )));
        }
        if self.deviation_mm > tolerance_mm + 1e-9 {
            return Err(CoreError::Invalid(format!(
                "les deux bords n'ont pas la même forme : une fois superposés, ils s'écartent de \
                 {:.3} mm au plus (tolérance {:.3} mm)",
                self.deviation_mm, tolerance_mm
            )));
        }
        Ok(())
    }
}

fn invalid(msg: impl Into<String>) -> CoreError {
    CoreError::Invalid(msg.into())
}

/// `base` + `suffix`, `base` raccourci pour tenir dans [`MAX_NAME_LEN`].
pub(crate) fn suffixed(base: &str, suffix: &str) -> String {
    let keep = MAX_NAME_LEN.saturating_sub(suffix.chars().count());
    let head: String = base.chars().take(keep).collect();
    format!("{}{suffix}", head.trim_end())
}

pub(crate) fn geoms_of(edges: &[ChainEdge]) -> Vec<EdgeGeom> {
    edges.iter().map(|e| e.geom).collect()
}

fn polyline_of(edges: &[EdgeGeom]) -> Vec<PointMm> {
    let mut pts = Vec::new();
    for e in edges {
        let s = edge_sample(e, SAMPLES);
        pts.extend(s.iter().take(s.len().saturating_sub(1)).copied());
    }
    pts
}

/// Points denses d'une ligne (pour juger de son appartenance à une moitié).
fn dense_points(g: &EdgeGeom) -> Vec<PointMm> {
    match g {
        EdgeGeom::Line(a, b) => (0..=16).map(|k| a.lerp(*b, k as f64 / 16.0)).collect(),
        other => edge_sample(other, 64),
    }
}

pub(crate) fn edge_of_shape(shape: &Shape) -> Option<EdgeGeom> {
    match shape {
        Shape::Line(a, b) => Some(EdgeGeom::Line(*a, *b)),
        Shape::Cubic(c) => Some(EdgeGeom::Cubic(*c)),
        Shape::Arc(a) => Some(EdgeGeom::Arc(*a)),
        Shape::Circle(_) | Shape::Spline(_) => None,
    }
}

impl Project {
    /// Contour d'une pièce en arêtes élémentaires, **anti-horaire**. Refus expliqué :
    /// pièce inconnue, dérivée liée, sans contour, ouvert, qui se recoupe ou dégénéré.
    pub(crate) fn piece_chain(&self, piece: PieceId) -> Result<Vec<ChainEdge>, CoreError> {
        let p = self.piece(piece)?;
        let name = &p.name;
        if p.derived.is_some() {
            return Err(invalid(format!(
                "la pièce « {name} » est une pièce dérivée liée à sa mère : cassez le lien avant \
                 de la séparer ou de l'assembler"
            )));
        }
        self.piece_chain_any(piece)
    }

    /// Comme `piece_chain`, sans refuser une pièce dérivée (lecture : contour de coupe).
    pub(crate) fn piece_chain_any(&self, piece: PieceId) -> Result<Vec<ChainEdge>, CoreError> {
        let p = self.piece(piece)?;
        let name = &p.name;
        let c = self.piece_contour(piece)?;
        if !c.closed {
            return Err(invalid(format!(
                "le contour de la pièce « {name} » n'est pas fermé"
            )));
        }
        if c.self_intersecting {
            return Err(invalid(format!(
                "le contour de la pièce « {name} » se recoupe lui-même"
            )));
        }
        if !c.area_mm2.is_finite() || c.area_mm2 <= 1e-9 {
            return Err(invalid(format!(
                "le contour de la pièce « {name} » est dégénéré (aire nulle)"
            )));
        }
        let sheet = self.piece_sheet(piece)?;
        let mut edges = Vec::new();
        for e in &p.outline {
            let role = sheet.entity(e.entity)?.role;
            let trace = trace_of(sheet.shape(e.entity)?, e.reversed);
            for geom in trace_geometry(&trace) {
                edges.push(ChainEdge {
                    geom,
                    role,
                    source: e.entity,
                });
            }
        }
        if chain_signed_area_mm2(&geoms_of(&edges)) < 0.0 {
            edges.reverse();
            for e in &mut edges {
                e.geom = e.geom.reversed();
            }
        }
        Ok(edges)
    }

    /// Droit-fil et éléments internes d'une pièce, prêts à être copiés ; les objets qui
    /// ne le peuvent pas sont signalés dans `notes` (jamais perdus en silence).
    fn carry_of(&self, piece: PieceId, notes: &mut Vec<String>) -> Result<Carry, CoreError> {
        self.carry_of_with(piece, notes, true)
    }

    /// Comme `carry_of` ; `darts_note` = signaler que les pinces ne sont pas reprises.
    pub(crate) fn carry_of_with(
        &self,
        piece: PieceId,
        notes: &mut Vec<String>,
        darts_note: bool,
    ) -> Result<Carry, CoreError> {
        let p = self.piece(piece)?;
        let sheet = self.piece_sheet(piece)?;
        let mut carry = Carry::default();
        let line_of = |entity: EntityId| -> Option<CarriedLine> {
            let role = sheet.entity(entity).ok()?.role;
            let geom = edge_of_shape(&sheet.shape(entity).ok()?)?;
            Some(CarriedLine { role, geom })
        };
        if let Some(g) = p.grainline {
            let grain_lines = self.piece_grain_lines(piece);
            match p.grain.as_ref() {
                // Droit-fil avancé (US-06.5) : on reporte la flèche **calculée** ; la feuille
                // d'arrivée n'a pas le tracé de référence, la direction est donc figée.
                Some(set) => match grain_lines.iter().find(|l| l.main) {
                    Some(main) => {
                        let carried = |l: &crate::piece_grain::ResolvedGrainLine| {
                            let role = sheet
                                .entity(l.entity)
                                .map_or(PathRole::Grainline, |e| e.role);
                            CarriedLine {
                                role,
                                geom: EdgeGeom::Line(l.from, l.to),
                            }
                        };
                        carry.grain = Some(carried(main));
                        carry.grain_set = Some(CarriedGrain {
                            sense: main.spec.sense,
                            tolerance: set.rotation_tolerance_deg,
                            extra: grain_lines
                                .iter()
                                .filter(|l| !l.main)
                                .map(|l| (carried(l), l.spec.sense))
                                .collect(),
                        });
                        if grain_lines
                            .iter()
                            .any(|l| !l.spec.direction.is_drawn() || l.spec.reversed)
                        {
                            notes.push(format!(
                                "La direction du droit-fil de la pièce « {} » n'est plus liée à \
                                 un tracé de référence : elle est figée à sa valeur actuelle.",
                                p.name
                            ));
                        }
                    }
                    None => notes.push(format!(
                        "Le droit-fil de la pièce « {} » n'est pas repris : sa direction est \
                         illisible.",
                        p.name
                    )),
                },
                None => match line_of(g) {
                    Some(l) => carry.grain = Some(l),
                    None => notes.push(format!(
                        "Le droit-fil de la pièce « {} » n'est pas repris : seuls un segment, une \
                         courbe de Bézier ou un arc peuvent être copiés.",
                        p.name
                    )),
                },
            }
        }
        let (mut skipped_lines, mut skipped_notches) = (0usize, 0usize);
        for el in &p.internal {
            match el.target {
                InternalTarget::Path { entity } => match line_of(entity) {
                    Some(l) => carry.lines.push((el.kind, l)),
                    None => skipped_lines += 1,
                },
                InternalTarget::Point { point } => match sheet.position(point) {
                    Ok(at) => carry.marks.push((el.kind, at)),
                    Err(_) => skipped_lines += 1,
                },
                InternalTarget::Notch { .. } => skipped_notches += 1,
            }
        }
        if skipped_lines > 0 {
            notes.push(format!(
                "{skipped_lines} élément(s) interne(s) de la pièce « {} » ne sont pas repris : \
                 seuls les segments, courbes de Bézier, arcs et points peuvent être copiés.",
                p.name
            ));
        }
        if skipped_notches > 0 {
            notes.push(format!(
                "{skipped_notches} cran(s) interne(s) de la pièce « {} » ne sont pas repris.",
                p.name
            ));
        }
        // Lignes de piqûre (US-06.9) : liées au bord, jamais reprises sur les pièces recréées.
        if !p.stitch_lines.is_empty() {
            notes.push(format!(
                "Les {} ligne(s) de piqûre de la pièce « {} » ne sont pas reprises : à replacer                  sur la pièce obtenue.",
                p.stitch_lines.len(),
                p.name
            ));
        }
        // Zones (US-06.10) : liées au bord ou en coordonnées de la pièce d'origine, jamais reprises.
        if !p.zones.is_empty() {
            notes.push(format!(
                "Les {} zone(s) de la pièce « {} » ne sont pas reprises : à replacer sur la \
                 pièce obtenue.",
                p.zones.len(),
                p.name
            ));
        }
        // Bords sur le pli (PH-5b) : les tracés changent, la déclaration n'est pas reprise.
        if !p.on_fold.is_empty() {
            notes.push(format!(
                "{} bord(s) sur le pli de la pièce « {} » ne sont pas repris (les tracés ont \
                 changé) : à déclarer sur les pièces obtenues.",
                p.on_fold.len(),
                p.name
            ));
        }
        if darts_note && !p.darts.is_empty() {
            notes.push(format!(
                "{} pince(s) de la pièce « {} » ne sont pas reprises : à replacer sur les pièces \
                 obtenues.",
                p.darts.len(),
                p.name
            ));
        }
        let mut skipped_markings = 0usize;
        for m in &p.markings {
            match self.marking_position(p.link, &m.at) {
                Some(at) => carry.markings.push((m.shape, at)),
                None => skipped_markings += 1,
            }
        }
        if skipped_markings > 0 {
            notes.push(format!(
                "{skipped_markings} marquage(s) de la pièce « {} » ne sont pas repris : position \
                 illisible.",
                p.name
            ));
        }
        let mut skipped_buttons = 0usize;
        for g in &p.buttons {
            match self.button_ends(p.link, g) {
                Some((from, to)) => carry.buttons.push(CarriedButtons {
                    count: g.count,
                    element: g.element,
                    edge_margin_mm: g.edge_margin_mm,
                    from,
                    to,
                }),
                None => skipped_buttons += 1,
            }
        }
        if skipped_buttons > 0 {
            notes.push(format!(
                "{skipped_buttons} groupe(s) de boutons de la pièce « {} » ne sont pas repris : \
                 position illisible.",
                p.name
            ));
        }
        let outline: HashSet<EntityId> = p.outline.iter().map(|e| e.entity).collect();
        let on_contour = sheet
            .notches
            .iter()
            .filter(|n| outline.contains(&n.entity))
            .count();
        if on_contour > 0 {
            notes.push(format!(
                "{on_contour} cran(s) posé(s) sur le contour de la pièce « {} » ne sont pas \
                 repris (ils restent sur la feuille d'origine).",
                p.name
            ));
        }
        Ok(carry)
    }

    /// Nouvelle feuille dédiée à une pièce résultat.
    pub(crate) fn new_result_sheet(&mut self, piece_name: &str) -> SheetId {
        let taken: HashSet<String> = self.sheets.iter().map(|s| s.name.clone()).collect();
        let base: String = piece_name.chars().take(MAX_NAME_LEN - 8).collect();
        let sid = SheetId(self.alloc_id());
        self.sheets
            .push(Sheet::new(sid, &unique_name(base.trim_end(), &taken)));
        sid
    }

    fn edge_kind(
        &mut self,
        sheet: SheetId,
        g: &EdgeGeom,
        start: PointId,
        end: PointId,
    ) -> Result<EntityKind, CoreError> {
        Ok(match g {
            EdgeGeom::Line(..) => EntityKind::Segment {
                from: start,
                to: end,
                construction: None,
            },
            EdgeGeom::Cubic(c) => EntityKind::Curve {
                start,
                handle_start: OffsetMm::from_vector(c.c1 - c.p0),
                handle_end: OffsetMm::from_vector(c.c2 - c.p3),
                end,
            },
            EdgeGeom::Arc(a) => EntityKind::Arc {
                center: self
                    .add_free_point(sheet, a.center)
                    .map_err(CoreError::Invalid)?,
                start,
                end,
                ccw: a.sweep_rad > 0.0,
            },
        })
    }

    /// Crée une chaîne fermée de tracés (points libres, jamais fusionnés) : l'arête `j`
    /// va du point `joints[j]` au point `joints[j + 1]`.
    pub(crate) fn build_closed_chain(
        &mut self,
        sheet: SheetId,
        edges: &[(EdgeGeom, PathRole)],
        joints: &[PointMm],
    ) -> Result<Vec<PieceEdge>, CoreError> {
        let n = edges.len();
        let mut ids = Vec::with_capacity(n);
        for p in joints {
            ids.push(self.add_free_point(sheet, *p).map_err(CoreError::Invalid)?);
        }
        let mut outline = Vec::with_capacity(n);
        for (j, (g, role)) in edges.iter().enumerate() {
            let kind = self.edge_kind(sheet, g, ids[j], ids[(j + 1) % n])?;
            let entity = self
                .add_entity(sheet, *role, kind)
                .map_err(CoreError::Invalid)?;
            outline.push(PieceEdge {
                entity,
                reversed: false,
            });
        }
        Ok(outline)
    }

    /// Copie une ligne (droit-fil, ligne interne) dans une feuille : points neufs.
    fn copy_line(&mut self, sheet: SheetId, line: &CarriedLine) -> Result<EntityId, CoreError> {
        let a = self
            .add_free_point(sheet, line.geom.start())
            .map_err(CoreError::Invalid)?;
        let b = self
            .add_free_point(sheet, line.geom.end())
            .map_err(CoreError::Invalid)?;
        let kind = self.edge_kind(sheet, &line.geom, a, b)?;
        self.add_entity(sheet, line.role, kind)
            .map_err(CoreError::Invalid)
    }

    /// Reporte droit-fil et éléments internes de `carry` dans la pièce `piece` de la
    /// feuille `sheet`.
    pub(crate) fn carry_into(
        &mut self,
        sheet: SheetId,
        carry: &Carry,
        grain: bool,
    ) -> Result<Carried, CoreError> {
        let mut grainline = None;
        let mut grain_set = None;
        if grain {
            if let Some(g) = &carry.grain {
                grainline = Some(self.copy_line(sheet, g)?);
                if let Some(cg) = &carry.grain_set {
                    let mut extra = Vec::new();
                    for (line, sense) in &cg.extra {
                        extra.push(ExtraGrainLine {
                            entity: self.copy_line(sheet, line)?,
                            direction: GrainDirection::Drawn {},
                            sense: *sense,
                            reversed: false,
                        });
                    }
                    let set = PieceGrain {
                        direction: GrainDirection::Drawn {},
                        sense: cg.sense,
                        reversed: false,
                        rotation_tolerance_deg: cg.tolerance,
                        extra,
                    };
                    grain_set = (!set.is_default()).then_some(set);
                }
            }
        }
        let mut internal = Vec::new();
        for (kind, line) in &carry.lines {
            let entity = self.copy_line(sheet, line)?;
            internal.push(InternalElement {
                target: InternalTarget::Path { entity },
                kind: *kind,
            });
        }
        for (kind, at) in &carry.marks {
            let point = self
                .add_free_point(sheet, *at)
                .map_err(CoreError::Invalid)?;
            internal.push(InternalElement {
                target: InternalTarget::Point { point },
                kind: *kind,
            });
        }
        let mut markings = Vec::new();
        for (shape, at) in &carry.markings {
            let point = self
                .add_free_point(sheet, *at)
                .map_err(CoreError::Invalid)?;
            markings.push(PieceMarking {
                id: MarkingId(self.alloc_id()),
                at: MarkingAnchor::Point { point },
                shape: *shape,
            });
        }
        let mut buttons = Vec::new();
        for c in &carry.buttons {
            let from = self
                .add_free_point(sheet, c.from)
                .map_err(CoreError::Invalid)?;
            let to = self
                .add_free_point(sheet, c.to)
                .map_err(CoreError::Invalid)?;
            buttons.push(ButtonGroup {
                id: ButtonGroupId(self.alloc_id()),
                from,
                to,
                count: c.count,
                element: c.element,
                edge_margin_mm: c.edge_margin_mm,
            });
        }
        Ok((grainline, internal, markings, buttons, grain_set))
    }

    // ------------------------------------------------------------------ séparer

    /// Aperçu d'une séparation (lecture seule) : points de coupe, aire et périmètre
    /// **observés** des deux moitiés. Erreur expliquée si la commande serait refusée
    /// pour une raison de géométrie ou de pièce.
    pub fn split_preview(
        &self,
        piece: PieceId,
        a: PointMm,
        b: PointMm,
    ) -> Result<SplitPreview, CoreError> {
        a.validate()?;
        b.validate()?;
        let chain = self.piece_chain(piece)?;
        let cut = cut_chain_by_line(&geoms_of(&chain), a, b).map_err(|e| invalid(e.to_string()))?;
        let info = |h: &[pf_geometry::split::CutEdge]| {
            let g: Vec<EdgeGeom> = h.iter().map(|e| e.geom).collect();
            HalfInfo {
                area_mm2: chain_signed_area_mm2(&g).abs(),
                perimeter_mm: chain_length_mm(&g),
            }
        };
        Ok(SplitPreview {
            points: cut.points,
            left: info(&cut.left),
            right: info(&cut.right),
        })
    }

    pub(crate) fn split_piece(
        &mut self,
        piece: PieceId,
        a: PointMm,
        b: PointMm,
        out: &mut Outcome,
    ) -> Result<(), CoreError> {
        a.validate()?;
        b.validate()?;
        self.piece(piece)?;
        let chain = self.piece_chain(piece)?;
        let cut = cut_chain_by_line(&geoms_of(&chain), a, b).map_err(|e| invalid(e.to_string()))?;
        self.split_with_cut(piece, &chain, cut, None, out)
    }

    /// Remplace `piece` par les deux moitiés de `cut` (pièces détachées sur feuilles dédiées).
    /// `notches` (US-07.5) : crans de repère à poser sur les deux bords de découpe ; l'arête de
    /// coupe de `cut.left` doit alors aller de `P` à `Q` et celle de `cut.right` de `Q` à `P`.
    pub(crate) fn split_with_cut(
        &mut self,
        piece: PieceId,
        chain: &[ChainEdge],
        cut: pf_geometry::split::ChainCut,
        notches: Option<&crate::piece_cut::CutNotches>,
        out: &mut Outcome,
    ) -> Result<(), CoreError> {
        let orig = self.piece(piece)?.clone();
        let index = self
            .pieces
            .iter()
            .position(|p| p.id == piece)
            .ok_or(CoreError::UnknownPiece(piece))?;
        let mut notes = Vec::new();
        let carry = self.carry_of(piece, &mut notes)?;

        let halves = [&cut.left, &cut.right];
        let mut sheets = Vec::new();
        let mut outlines = Vec::new();
        let mut lassos = Vec::new();
        let names = [
            suffixed(&orig.name, " (1/2)"),
            suffixed(&orig.name, " (2/2)"),
        ];
        for (k, half) in halves.iter().enumerate() {
            let last_role = half
                .iter()
                .rev()
                .find_map(|e| e.source.map(|s| chain[s].role))
                .unwrap_or_default();
            let edges: Vec<(EdgeGeom, PathRole)> = half
                .iter()
                .map(|e| (e.geom, e.source.map_or(last_role, |s| chain[s].role)))
                .collect();
            let joints: Vec<PointMm> = edges.iter().map(|(g, _)| g.start()).collect();
            let sid = self.new_result_sheet(&names[k]);
            outlines.push(self.build_closed_chain(sid, &edges, &joints)?);
            if let Some(spec) = notches {
                // Dernière arête = arête de découpe ; `k == 1` : moitié de droite (retournée).
                let last = outlines[k].len() - 1;
                self.mark_cut_edge(sid, &mut outlines[k][last], k == 1, spec, out)?;
            }
            sheets.push(sid);
            let geoms: Vec<EdgeGeom> = edges.iter().map(|(g, _)| *g).collect();
            lassos.push(Lasso::new(&polyline_of(&geoms)));
        }

        // Droit-fil et éléments internes : à la moitié qui les contient entièrement.
        let which = |pts: &[PointMm]| {
            lassos
                .iter()
                .position(|l| l.as_ref().is_some_and(|l| l.contains_polyline(pts)))
        };
        let mut half_carry = [Carry::default(), Carry::default()];
        let mut lost = 0usize;
        let mut grain_lost = false;
        let mut grain_extra_lost = 0usize;
        if let Some(g) = &carry.grain {
            match which(&dense_points(&g.geom)) {
                Some(h) => {
                    half_carry[h].grain = Some(g.clone());
                    // Réglages du droit-fil (US-06.5) : avec la moitié qui garde la ligne
                    // principale ; une ligne supplémentaire de l'autre moitié est perdue.
                    if let Some(cg) = &carry.grain_set {
                        let mut set = CarriedGrain {
                            sense: cg.sense,
                            tolerance: cg.tolerance,
                            extra: Vec::new(),
                        };
                        for (l, sense) in &cg.extra {
                            if which(&dense_points(&l.geom)) == Some(h) {
                                set.extra.push((l.clone(), *sense));
                            } else {
                                grain_extra_lost += 1;
                            }
                        }
                        half_carry[h].grain_set = Some(set);
                    }
                }
                None => grain_lost = true,
            }
        }
        for (kind, l) in &carry.lines {
            match which(&dense_points(&l.geom)) {
                Some(h) => half_carry[h].lines.push((*kind, l.clone())),
                None => lost += 1,
            }
        }
        for (kind, at) in &carry.marks {
            match which(&[*at]) {
                Some(h) => half_carry[h].marks.push((*kind, *at)),
                None => lost += 1,
            }
        }
        // Marquages : un point, donc toujours dans une moitié ou sur la ligne de coupe.
        let mut markings_lost = 0usize;
        for (shape, at) in &carry.markings {
            match which(&[*at]) {
                Some(h) => half_carry[h].markings.push((*shape, *at)),
                None => markings_lost += 1,
            }
        }
        if markings_lost > 0 {
            notes.push(format!(
                "{markings_lost} marquage(s) de la pièce « {} » sont sur la ligne de coupe : ils \
                 ne sont pas repris.",
                orig.name
            ));
        }
        // Groupes de boutons : repris par la moitié qui contient entièrement l'axe de leurs
        // deux points ; sinon (ligne de coupe entre les deux points ou sur l'axe) remarque.
        let mut buttons_lost = 0usize;
        for c in &carry.buttons {
            match which(&[c.from, c.to]) {
                Some(h) => half_carry[h].buttons.push(*c),
                None => buttons_lost += 1,
            }
        }
        if buttons_lost > 0 {
            notes.push(format!(
                "{buttons_lost} groupe(s) de boutons de la pièce « {} » traversent la ligne de \
                 coupe ou la touchent : ils ne sont pas repris.",
                orig.name
            ));
        }
        if grain_lost {
            notes.push(format!(
                "Le droit-fil de la pièce « {} » traverse la ligne de coupe : aucune des deux \
                 pièces ne le garde (droit-fil manquant).",
                orig.name
            ));
        }
        if grain_extra_lost > 0 {
            notes.push(format!(
                "{grain_extra_lost} ligne(s) de droit-fil supplémentaire(s) de la pièce « {} » \
                 ne sont pas dans la moitié qui garde le droit-fil : elles ne sont pas reprises.",
                orig.name
            ));
        }
        if lost > 0 {
            notes.push(format!(
                "{lost} élément(s) interne(s) de la pièce « {} » traversent la ligne de coupe \
                 ou la touchent : ils ne sont pas repris.",
                orig.name
            ));
        }
        if orig.placement.is_some() {
            notes.push(format!(
                "Le placement manuel de la pièce « {} » n'est pas repris : les deux pièces sont \
                 placées automatiquement dans le plan.",
                orig.name
            ));
        }

        // Marge de couture (US-05.1) : la valeur par défaut est reportée sur les deux moitiés
        // (la ligne de coupe en reçoit une aussi) ; les surcharges par bord ne le sont pas.
        let half_seam = crate::piece_seam::carried_seam(&orig, &mut notes);
        let mut new_pieces = Vec::new();
        for k in 0..2 {
            let (grainline, internal, markings, buttons, grain) =
                self.carry_into(sheets[k], &half_carry[k], true)?;
            let id = PieceId(self.alloc_id());
            let code = orig
                .code
                .as_deref()
                .map(|c| suffixed(c, if k == 0 { "-1" } else { "-2" }));
            new_pieces.push(Piece {
                id,
                name: names[k].clone(),
                code,
                quantity: orig.quantity,
                material: orig.material.clone(),
                category: orig.category,
                comment: orig.comment.clone(),
                mirrored_pair: orig.mirrored_pair,
                outline: outlines[k].clone(),
                grainline,
                grain,
                seam: half_seam.clone(),
                net_cut: orig.net_cut,
                no_grain: orig.no_grain,
                internal,
                markings,
                buttons,
                extra: orig.extra.clone(),
                ..Piece::default()
            });
            out.pieces.push(id);
        }
        self.pieces.splice(index..=index, new_pieces);
        out.sheet = Some(sheets[0]);
        notes.insert(
            0,
            format!(
                "La pièce « {} » est remplacée par « {} » et « {} » ; la construction n'est pas \
                 modifiée.",
                orig.name, names[0], names[1]
            ),
        );
        out.notes.extend(notes);
        Ok(())
    }

    // ---------------------------------------------------------------- assembler

    /// Position du bord `edge` dans la chaîne : (début, nombre d'arêtes élémentaires).
    fn edge_range(
        &self,
        chain: &[ChainEdge],
        piece: PieceId,
        edge: EntityId,
    ) -> Result<(usize, usize), CoreError> {
        let idx: Vec<usize> = chain
            .iter()
            .enumerate()
            .filter(|(_, e)| e.source == edge)
            .map(|(i, _)| i)
            .collect();
        let name = &self.piece(piece)?.name;
        let (Some(&first), Some(&last)) = (idx.first(), idx.last()) else {
            return Err(invalid(format!(
                "le tracé {} n'est pas un bord du contour de la pièce « {name} »",
                edge.0
            )));
        };
        if last - first + 1 != idx.len() {
            return Err(invalid(format!(
                "incohérence interne : le bord {} de la pièce « {name} » n'est pas continu",
                edge.0
            )));
        }
        if idx.len() == chain.len() {
            return Err(invalid(format!(
                "la pièce « {name} » n'a qu'un seul bord : il ne peut pas servir de jonction"
            )));
        }
        Ok((first, idx.len()))
    }

    fn join_measure(
        &self,
        first: PieceId,
        first_edge: EntityId,
        second: PieceId,
        second_edge: EntityId,
    ) -> Result<JoinMeasure, CoreError> {
        if first == second {
            return Err(invalid(
                "choisissez deux pièces différentes : une pièce ne s'assemble pas à elle-même",
            ));
        }
        let chain_a = self.piece_chain(first)?;
        let chain_b = self.piece_chain(second)?;
        let range_a = self.edge_range(&chain_a, first, first_edge)?;
        let range_b = self.edge_range(&chain_b, second, second_edge)?;
        let a_geoms = geoms_of(&chain_a[range_a.0..range_a.0 + range_a.1]);
        let b_geoms = geoms_of(&chain_b[range_b.0..range_b.0 + range_b.1]);
        let (a_start, a_end) = (a_geoms[0].start(), a_geoms[a_geoms.len() - 1].end());
        let (b_start, b_end) = (b_geoms[0].start(), b_geoms[b_geoms.len() - 1].end());
        if distance_mm(a_start, a_end) <= COINCIDENT_MM
            || distance_mm(b_start, b_end) <= COINCIDENT_MM
        {
            return Err(invalid(
                "un des deux bords se referme sur lui-même : il ne peut pas servir de jonction",
            ));
        }
        // B_start → A_end et B_end → A_start : le bord de B est retourné en sens inverse.
        let rigid = Rigid::between_segments(b_start, b_end, a_end, a_start)
            .map_err(|e| invalid(e.to_string()))?;
        let moved: Vec<EdgeGeom> = b_geoms.iter().map(|g| rigid.apply_edge(g)).collect();
        Ok(JoinMeasure {
            len_a: chain_length_mm(&a_geoms),
            len_b: chain_length_mm(&b_geoms),
            deviation_mm: max_deviation_mm(&a_geoms, &moved),
            chain_a,
            chain_b,
            range_a,
            range_b,
            rigid,
            a_start,
            a_end,
        })
    }

    /// Aperçu d'un assemblage (lecture seule). Les longueurs, l'écart et le contour déplacé
    /// de la seconde pièce sont rendus même quand l'assemblage serait refusé (la raison est
    /// dans `refusal`). Erreur seulement si une pièce ou un bord est inutilisable.
    pub fn join_preview(
        &self,
        first: PieceId,
        first_edge: EntityId,
        second: PieceId,
        second_edge: EntityId,
        tolerance_mm: f64,
    ) -> Result<JoinPreview, CoreError> {
        check_join_tolerance(tolerance_mm)?;
        let m = self.join_measure(first, first_edge, second, second_edge)?;
        let moved_all: Vec<EdgeGeom> = m
            .chain_b
            .iter()
            .map(|e| m.rigid.apply_edge(&e.geom))
            .collect();
        let mut refusal = m.check(tolerance_mm).err().map(|e| e.to_string());
        let mut result = None;
        if refusal.is_none() {
            // Même chemin que la commande : tout refus (recouvrement, arcs…) est annoncé.
            let mut trial = self.clone();
            match trial.apply(crate::command::Command::JoinPieces {
                first,
                first_edge,
                second,
                second_edge,
                tolerance_mm,
            }) {
                Ok(o) => {
                    result = o.pieces.first().and_then(|id| {
                        trial.piece_contour(*id).ok().map(|c| HalfInfo {
                            area_mm2: c.area_mm2,
                            perimeter_mm: c.perimeter_mm,
                        })
                    })
                }
                Err(e) => refusal = Some(e.to_string()),
            }
        }
        Ok(JoinPreview {
            first_length_mm: m.len_a,
            second_length_mm: m.len_b,
            gap_mm: m.gap_mm(),
            deviation_mm: m.deviation_mm,
            tolerance_mm,
            rotation_deg: m.rigid.angle_rad().to_degrees(),
            moved_second: polyline_of(&moved_all),
            result,
            refusal,
        })
    }

    pub(crate) fn join_pieces(
        &mut self,
        first: PieceId,
        first_edge: EntityId,
        second: PieceId,
        second_edge: EntityId,
        tolerance_mm: f64,
        out: &mut Outcome,
    ) -> Result<(), CoreError> {
        check_join_tolerance(tolerance_mm)?;
        let m = self.join_measure(first, first_edge, second, second_edge)?;
        m.check(tolerance_mm)?;
        let pa = self.piece(first)?.clone();
        let pb = self.piece(second)?.clone();

        // Contour fusionné : le contour de A après son bord, puis celui de B déplacé après le sien.
        let rotate = |chain: &[ChainEdge], (start, count): (usize, usize)| -> Vec<ChainEdge> {
            let n = chain.len();
            (0..n - count)
                .map(|k| chain[(start + count + k) % n].clone())
                .collect()
        };
        let a_after = rotate(&m.chain_a, m.range_a);
        let b_after = rotate(&m.chain_b, m.range_b);
        let mut edges: Vec<(EdgeGeom, PathRole)> =
            a_after.iter().map(|e| (e.geom, e.role)).collect();
        edges.extend(
            b_after
                .iter()
                .map(|e| (m.rigid.apply_edge(&e.geom), e.role)),
        );
        let n = edges.len();
        let mut joints: Vec<PointMm> = edges.iter().map(|(g, _)| g.start()).collect();
        // La jonction garde les coordonnées de A.
        let (v_ab, v_ba) = (a_after.len(), 0);
        joints[v_ab] = m.a_start;
        joints[v_ba] = m.a_end;
        for (v, j) in [(v_ab, m.a_start), (v_ba, m.a_end)] {
            let (prev, next) = (&edges[(v + n - 1) % n].0, &edges[v].0);
            let is_arc = |g: &EdgeGeom| matches!(g, EdgeGeom::Arc(_));
            if (is_arc(prev) && distance_mm(prev.end(), j) > ARC_RADIUS_TOLERANCE_MM)
                || (is_arc(next) && distance_mm(next.start(), j) > ARC_RADIUS_TOLERANCE_MM)
            {
                return Err(invalid(format!(
                    "un arc touche la jonction des pièces « {} » et « {} » : avec un écart de                      {:.3} mm, il ne peut pas être raccordé (un arc ne tolère que 0,0025 mm)",
                    pa.name,
                    pb.name,
                    m.gap_mm()
                )));
            }
        }

        let name: String = format!("{} + {}", pa.name, pb.name)
            .chars()
            .take(MAX_NAME_LEN)
            .collect();
        let sid = self.new_result_sheet(&name);
        let outline = self.build_closed_chain(sid, &edges, &joints)?;

        let mut notes = Vec::new();
        let ca = self.carry_of(first, &mut notes)?;
        let mut cb = self.carry_of(second, &mut notes)?;
        let move_line = |l: &mut CarriedLine| l.geom = m.rigid.apply_edge(&l.geom);
        if let Some(g) = cb.grain.as_mut() {
            move_line(g);
        }
        if let Some(set) = cb.grain_set.as_mut() {
            set.extra.iter_mut().for_each(|(l, _)| move_line(l));
        }
        cb.lines.iter_mut().for_each(|(_, l)| move_line(l));
        cb.marks
            .iter_mut()
            .for_each(|(_, p)| *p = m.rigid.apply(*p));
        let rotation_deg = m.rigid.angle_rad().to_degrees();
        cb.markings.iter_mut().for_each(|(shape, p)| {
            *p = m.rigid.apply(*p);
            *shape = shape.rotated(rotation_deg);
        });
        // Groupes de boutons : points déplacés ; l'orientation suit la rotation de la pièce
        // (« horizontale » devient un angle libre, l'orientation relative à l'axe ne change pas).
        for c in &mut cb.buttons {
            c.from = m.rigid.apply(c.from);
            c.to = m.rigid.apply(c.to);
            c.element = c.element.rotated(rotation_deg);
        }
        let keep_b_grain = ca.grain.is_none();
        if !keep_b_grain && cb.grain.is_some() {
            notes.push(format!(
                "Le droit-fil de la pièce « {} » n'est pas repris : celui de « {} » est conservé.",
                pb.name, pa.name
            ));
        }
        let (grain_a, mut internal, mut markings, mut buttons, set_a) =
            self.carry_into(sid, &ca, true)?;
        let (grain_b, internal_b, markings_b, buttons_b, set_b) =
            self.carry_into(sid, &cb, keep_b_grain)?;
        internal.extend(internal_b);
        markings.extend(markings_b);
        buttons.extend(buttons_b);
        let grainline = grain_a.or(grain_b);
        let grain = set_a.or(set_b);

        let mut differs = Vec::new();
        if pa.material != pb.material {
            differs.push("matière");
        }
        if pa.category != pb.category {
            differs.push("catégorie");
        }
        if pa.quantity != pb.quantity {
            differs.push("quantité");
        }
        if pa.mirrored_pair != pb.mirrored_pair {
            differs.push("paire en miroir");
        }
        if pa.code != pb.code {
            differs.push("code");
        }
        if !differs.is_empty() {
            notes.push(format!(
                "La fiche de la pièce « {} » est conservée ; celle de « {} » différait ({}).",
                pa.name,
                pb.name,
                differs.join(", ")
            ));
        }
        if pa.placement.is_some() || pb.placement.is_some() {
            notes.push(
                "Le placement manuel des pièces d'origine n'est pas repris : la pièce assemblée \
                 est placée automatiquement dans le plan."
                    .to_string(),
            );
        }

        // Marge de couture (US-05.1) : la valeur par défaut de la première pièce (à défaut, de la
        // seconde) ; les surcharges par bord ne sont pas reprises. Rien n'est perdu en silence.
        let mut joined_seam = crate::piece_seam::carried_seam(&pa, &mut notes);
        let seam_b = crate::piece_seam::carried_seam(&pb, &mut notes);
        if let (Some(a), Some(b)) = (&joined_seam, &seam_b) {
            if a.default_mm != b.default_mm {
                notes.push(format!(
                    "La marge par défaut de la pièce « {} » ({} mm) est conservée ; celle de « {} » \
                     ({} mm) différait.",
                    pa.name, a.default_mm, pb.name, b.default_mm
                ));
            }
        }
        if joined_seam.is_none() {
            joined_seam = seam_b;
        }
        // Coupe nette (US-05.6) : exclusive avec une marge ; la déclaration de la première pièce
        // l'emporte, sinon la marge reportée ; ce qui n'est pas reporté est dit.
        let net_cut = pa.net_cut;
        if net_cut {
            if joined_seam.is_some() {
                notes.push(format!(
                    "La pièce « {} » est en coupe nette : la marge de couture de « {} » n'est pas \
                     reprise.",
                    pa.name, pb.name
                ));
            }
            joined_seam = None;
        } else if pb.net_cut && joined_seam.is_none() {
            notes.push(format!(
                "La coupe nette de la pièce « {} » n'est pas reprise sur la pièce assemblée : à \
                 déclarer de nouveau.",
                pb.name
            ));
        } else if pb.net_cut {
            notes.push(format!(
                "La coupe nette de la pièce « {} » n'est pas reprise : la marge de couture de \
                 « {} » l'emporte.",
                pb.name, pa.name
            ));
        }
        // « Sans droit-fil » (PH-5d) : repris seulement si les deux pièces le déclarent ; sinon la
        // pièce assemblée n'est pas déclarée et la remarque le dit.
        let no_grain = pa.no_grain && pb.no_grain;
        if (pa.no_grain || pb.no_grain) && !no_grain {
            let (declared, other) = if pa.no_grain {
                (&pa.name, &pb.name)
            } else {
                (&pb.name, &pa.name)
            };
            notes.push(format!(
                "La pièce « {declared} » est déclarée sans droit-fil mais pas « {other} » : la \
                 déclaration n'est pas reprise sur la pièce assemblée."
            ));
        }
        let id = PieceId(self.alloc_id());
        let merged = Piece {
            id,
            name,
            code: pa.code.clone(),
            quantity: pa.quantity,
            material: pa.material.clone(),
            category: pa.category,
            comment: pa.comment.clone(),
            mirrored_pair: pa.mirrored_pair,
            outline,
            grainline,
            grain,
            seam: joined_seam,
            net_cut,
            no_grain,
            internal,
            markings,
            buttons,
            extra: pa.extra.clone(),
            ..Piece::default()
        };
        // Le contour fusionné doit être une chaîne fermée qui ne se recoupe pas.
        let ids: Vec<EntityId> = merged.outline.iter().map(|e| e.entity).collect();
        contour_from_entities(self.sheet(sid)?, &ids).map_err(|e| {
            invalid(format!(
                "le contour assemblé n'est pas une chaîne fermée ({e})"
            ))
        })?;
        self.pieces.retain(|p| p.id != second);
        let at = self
            .pieces
            .iter()
            .position(|p| p.id == first)
            .ok_or(CoreError::UnknownPiece(first))?;
        self.pieces[at] = merged;
        let c = self.piece_contour(id)?;
        if c.self_intersecting {
            return Err(invalid(
                "les deux pièces se recouvrent une fois assemblées : leurs contours se croisent",
            ));
        }
        if !c.area_mm2.is_finite() || c.area_mm2 <= 1e-9 {
            return Err(invalid("l'assemblage donnerait un contour d'aire nulle"));
        }
        out.pieces.push(id);
        out.sheet = Some(sid);
        out.notes.push(format!(
            "Écart de longueur entre les deux bords : {:.3} mm (tolérance {:.3} mm) ; écart de forme \
             au plus {:.3} mm.",
            m.gap_mm(),
            tolerance_mm,
            m.deviation_mm
        ));
        out.notes.push(format!(
            "Les pièces « {} » et « {} » sont remplacées par « {} » ; la construction n'est pas \
             modifiée.",
            pa.name,
            pb.name,
            self.piece(id)?.name
        ));
        out.notes.extend(notes);
        Ok(())
    }
}

fn check_join_tolerance(tolerance_mm: f64) -> Result<(), CoreError> {
    if !tolerance_mm.is_finite() || !(0.0..=MAX_JOIN_TOLERANCE_MM).contains(&tolerance_mm) {
        return Err(invalid(format!(
            "la tolérance d'assemblage doit être un nombre fini compris entre 0 et \
             {MAX_JOIN_TOLERANCE_MM} mm"
        )));
    }
    Ok(())
}
