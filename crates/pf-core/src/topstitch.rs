//! US-06.8 — surpiqûre liée à un bord : une ligne de rôle « surpiqûre » **générée par
//! décalage** d'un bord (le contour d'une pièce ou une chaîne de tracés de la feuille) à une
//! distance donnée, **régénérée** quand le bord change (décisions 194 à 203).
//!
//! Le lien est une **dérivation recalculée**, pas un solveur : après chaque commande, les
//! surpiqûres dont le bord a changé sont recalculées dans la même étape atomique
//! (`Project::sync_topstitches`, appelée par `Project::apply`), avec les mêmes règles que les
//! pièces dérivées (US-04.4) : identifiants conservés quand la topologie du décalage ne change
//! pas, **obsolète plutôt que refus** quand le recalcul échoue à cause du bord (la ligne garde sa
//! dernière géométrie valide), lien cassable (les lignes deviennent des tracés libres).
//!
//! Distinction avec les éléments internes (US-04.3) : un élément interne est l'**appartenance**
//! d'un tracé existant à une pièce (type « surpiqûre » parmi d'autres) ; la surpiqûre liée est la
//! **génération** de ce tracé. Les deux se composent (une ligne générée peut être déclarée
//! élément interne d'une pièce) mais ne se confondent pas : aucun doublon de données.
//!
//! Géométrie : `pf_geometry::offset_chain` (distance exacte sur segments et arcs, 2e-5 mm sur
//! les courbes). Aucune marge de couture (E05) : c'est un décalage de mise en page d'une ligne.

use crate::command::Outcome;
use crate::contour::{contour_from_entities, measure_contour};
use crate::derived::{
    clean_reason, joint_at, trace_end, trace_start, Plan, CHAIN_GAP_MM, MAX_STALE_LEN,
};
use crate::error::CoreError;
use crate::model::*;
use crate::offset::{ordered_chain, trace_of, MAX_OFFSET_PATHS};
use pf_geometry::{distance_mm, offset_chain, OffsetOptions, PointMm, Trace};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Plus petite distance de surpiqûre acceptée (mm).
pub const MIN_TOPSTITCH_MM: f64 = 0.1;
/// Plus grande distance de surpiqûre acceptée (mm).
pub const MAX_TOPSTITCH_MM: f64 = 1000.0;
/// Nombre maximal de surpiqûres liées par feuille.
pub const MAX_TOPSTITCHES_PER_SHEET: usize = 200;

/// Côté du décalage. Bord **fermé** (contour de pièce, chaîne fermée, cercle) : `Inside` /
/// `Outside`, indépendants du sens d'enregistrement des tracés. Bord **ouvert** : `Left` /
/// `Right` du sens de parcours qui part du point `from` de la source (voir [`TopstitchSource`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TopstitchSide {
    Inside,
    Outside,
    Left,
    Right,
}

impl TopstitchSide {
    /// Identifiant stable, identique à la valeur écrite dans le fichier.
    pub fn key(self) -> &'static str {
        match self {
            TopstitchSide::Inside => "inside",
            TopstitchSide::Outside => "outside",
            TopstitchSide::Left => "left",
            TopstitchSide::Right => "right",
        }
    }

    /// Côté d'un bord fermé (`Inside` / `Outside`) ?
    pub fn is_closed_side(self) -> bool {
        matches!(self, TopstitchSide::Inside | TopstitchSide::Outside)
    }
}

/// Le bord que suit la surpiqûre.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum TopstitchSource {
    /// Le contour de la pièce (chaîne fermée simple, orientée à la lecture).
    Piece { piece: PieceId },
    /// Une chaîne de tracés de la feuille, bout à bout. `from` n'existe que pour une chaîne
    /// **ouverte** : l'extrémité d'où part le sens de parcours (celui du plus petit
    /// identifiant, tel qu'enregistré, au moment de la création) ; il fixe « gauche » et
    /// « droite » même si un tracé est ensuite inversé.
    Paths {
        paths: Vec<EntityId>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        from: Option<PointId>,
    },
}

impl TopstitchSource {
    /// Identifiant stable de la forme de source.
    pub fn key(&self) -> &'static str {
        match self {
            TopstitchSource::Piece { .. } => "piece",
            TopstitchSource::Paths { .. } => "paths",
        }
    }
}

/// Lien d'une surpiqûre avec son bord (champ `topstitches` de la feuille).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Topstitch {
    pub id: TopstitchId,
    pub source: TopstitchSource,
    /// Distance au bord, en mm, strictement positive (bornes `MIN_TOPSTITCH_MM` à
    /// `MAX_TOPSTITCH_MM`).
    pub distance_mm: f64,
    pub side: TopstitchSide,
    /// Tracés **générés**, dans l'ordre de la chaîne (jamais vide). Ce sont de vrais tracés
    /// de rôle surpiqûre, aux points libres.
    pub paths: Vec<EntityId>,
    /// Raison pour laquelle la dernière mise à jour a échoué ; les lignes gardent alors leur
    /// dernière géométrie valide. Absent = à jour.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stale: Option<String>,
}

/// État d'une surpiqûre liée.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TopstitchState {
    Current,
    Stale { reason: String },
}

/// Nature du bord : fermé (côtés intérieur / extérieur) ou ouvert (gauche / droite à partir de
/// `from`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChainKind {
    Closed,
    Open { from: PointId },
}

impl Topstitch {
    pub fn state(&self) -> TopstitchState {
        match &self.stale {
            None => TopstitchState::Current,
            Some(reason) => TopstitchState::Stale {
                reason: reason.clone(),
            },
        }
    }
}

impl Sheet {
    pub fn topstitch(&self, id: TopstitchId) -> Result<&Topstitch, CoreError> {
        self.topstitches
            .iter()
            .find(|t| t.id == id)
            .ok_or_else(|| CoreError::Invalid(format!("surpiqûre liée {} introuvable", id.0)))
    }

    fn topstitch_mut(&mut self, id: TopstitchId) -> Result<&mut Topstitch, CoreError> {
        self.topstitches
            .iter_mut()
            .find(|t| t.id == id)
            .ok_or_else(|| CoreError::Invalid(format!("surpiqûre liée {} introuvable", id.0)))
    }

    /// Surpiqûre liée dont ce tracé est une ligne **générée**, s'il y en a une.
    pub fn topstitch_of_path(&self, entity: EntityId) -> Option<&Topstitch> {
        self.topstitches.iter().find(|t| t.paths.contains(&entity))
    }
}

fn invalid(msg: impl Into<String>) -> CoreError {
    CoreError::Invalid(msg.into())
}

/// Distance dans les bornes, finie.
pub fn check_topstitch_distance(distance_mm: f64) -> Result<(), CoreError> {
    if !distance_mm.is_finite() || !(MIN_TOPSTITCH_MM..=MAX_TOPSTITCH_MM).contains(&distance_mm) {
        return Err(invalid(format!(
            "surpiqûre : la distance au bord doit être un nombre fini compris entre \
             {MIN_TOPSTITCH_MM} mm et {MAX_TOPSTITCH_MM} mm"
        )));
    }
    Ok(())
}

/// Bord fermé pris par le contour de ces tracés, orienté **anti-horaire**, avec son aire.
fn closed_ccw_chain(sheet: &Sheet, ids: &[EntityId]) -> Result<(Vec<Trace>, f64), String> {
    let contour = contour_from_entities(sheet, ids)
        .map_err(|e| format!("le bord n'est pas une chaîne fermée ({e})"))?;
    let m = measure_contour(sheet, &contour)
        .map_err(|e| format!("le contour du bord est illisible ({e})"))?;
    if m.self_intersecting {
        return Err("le contour du bord se recoupe lui-même".into());
    }
    if m.area_mm2.is_nan() || m.area_mm2 <= 1e-9 {
        return Err("le contour du bord est dégénéré (aire nulle)".into());
    }
    let mut edges = contour.edges.clone();
    if m.clockwise && edges.len() > 1 {
        edges.reverse();
        for e in &mut edges {
            e.reversed = !e.reversed;
        }
    }
    let traces = edges
        .iter()
        .map(|e| {
            sheet
                .shape(e.entity)
                .map(|s| trace_of(s, e.reversed))
                .map_err(|e| e.to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok((traces, m.area_mm2))
}

/// Bord prêt à être décalé.
struct SourceChain {
    /// Chaîne parcourue dans le sens utile (anti-horaire si fermée ; sens à partir de `from`
    /// si ouverte).
    traces: Vec<Trace>,
    closed: bool,
    /// Aire de la chaîne fermée (mm²).
    area_mm2: f64,
    /// Extrémité de départ d'une chaîne ouverte.
    start: Option<PointId>,
}

/// Point de départ et d'arrivée (identifiants) d'une chaîne ordonnée.
fn chain_ends(sheet: &Sheet, edges: &[(EntityId, bool)]) -> Result<(PointId, PointId), CoreError> {
    let (first, last) = (edges[0], edges[edges.len() - 1]);
    let (a, b) = sheet.entity_points(first.0)?;
    let start = if first.1 { b } else { a };
    let (c, d) = sheet.entity_points(last.0)?;
    let end = if last.1 { c } else { d };
    Ok((start, end))
}

impl Project {
    /// Bord d'une surpiqûre, vérifié pour le côté demandé.
    fn topstitch_chain(
        &self,
        sheet: &Sheet,
        source: &TopstitchSource,
        side: TopstitchSide,
    ) -> Result<SourceChain, String> {
        let chain = match source {
            TopstitchSource::Piece { piece } => {
                let p = self.piece(*piece).map_err(|e| e.to_string())?;
                let on = self.piece_sheet(*piece).map_err(|e| e.to_string())?;
                if on.id != sheet.id {
                    return Err(format!(
                        "la pièce « {} » n'est pas sur la feuille de la surpiqûre",
                        p.name
                    ));
                }
                if p.outline.is_empty() {
                    return Err(format!("la pièce « {} » n'a pas de contour", p.name));
                }
                let ids: Vec<EntityId> = p.outline.iter().map(|e| e.entity).collect();
                let (traces, area_mm2) = closed_ccw_chain(sheet, &ids)
                    .map_err(|e| format!("pièce « {} » : {e}", p.name))?;
                SourceChain {
                    traces,
                    closed: true,
                    area_mm2,
                    start: None,
                }
            }
            TopstitchSource::Paths { paths, from } => {
                let ordered = ordered_chain(sheet, paths).map_err(|e| e.to_string())?;
                if ordered.closed {
                    let (traces, area_mm2) = match ordered.circle_ccw {
                        Some(_) => {
                            let Some(Trace::Circle(c)) = ordered.traces.first() else {
                                return Err("cercle illisible".into());
                            };
                            (ordered.traces.clone(), c.area_mm2())
                        }
                        None => {
                            let ids: Vec<EntityId> = ordered.edges.iter().map(|e| e.0).collect();
                            closed_ccw_chain(sheet, &ids)?
                        }
                    };
                    SourceChain {
                        traces,
                        closed: true,
                        area_mm2,
                        start: None,
                    }
                } else {
                    let mut edges = ordered.edges.clone();
                    let (start, end) = chain_ends(sheet, &edges).map_err(|e| e.to_string())?;
                    let start = match *from {
                        None => start,
                        Some(f) if f == start => start,
                        Some(f) if f == end => {
                            edges.reverse();
                            for e in &mut edges {
                                e.1 = !e.1;
                            }
                            end
                        }
                        Some(_) => {
                            return Err(
                                "le point de départ du bord n'est plus une extrémité de la \
                                 chaîne"
                                    .into(),
                            )
                        }
                    };
                    let traces = edges
                        .iter()
                        .map(|&(id, rev)| {
                            sheet
                                .shape(id)
                                .map(|s| trace_of(s, rev))
                                .map_err(|e| e.to_string())
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    SourceChain {
                        traces,
                        closed: false,
                        area_mm2: 0.0,
                        start: Some(start),
                    }
                }
            }
        };
        match (chain.closed, side.is_closed_side()) {
            (true, false) => {
                Err("ce bord est fermé : le côté se choisit entre intérieur et extérieur".into())
            }
            (false, true) => {
                Err("ce bord est ouvert : le côté se choisit entre gauche et droite".into())
            }
            _ => Ok(chain),
        }
    }

    /// Nature du bord (fermé, ou ouvert avec son extrémité de départ), pour choisir les
    /// côtés proposés.
    pub fn topstitch_chain_kind(
        &self,
        sheet: SheetId,
        source: &TopstitchSource,
    ) -> Result<ChainKind, CoreError> {
        let s = self.sheet(sheet)?;
        // Un côté arbitraire du bon genre : seul le genre du bord nous intéresse.
        let probe = |side| self.topstitch_chain(s, source, side);
        match probe(TopstitchSide::Inside) {
            Ok(_) => Ok(ChainKind::Closed),
            Err(_) => match probe(TopstitchSide::Left) {
                Ok(c) => c
                    .start
                    .map(|from| ChainKind::Open { from })
                    .ok_or_else(|| invalid("bord illisible")),
                Err(e) => Err(invalid(e)),
            },
        }
    }

    /// Lecture des surpiqûres liées d'une feuille.
    pub fn topstitches_of(&self, sheet: SheetId) -> Vec<Topstitch> {
        self.sheet(sheet)
            .map(|s| s.topstitches.clone())
            .unwrap_or_default()
    }
}

/// Décalage de `chain` à `distance_mm` du côté `side`, avec les contrôles de forme : une
/// raison si le résultat n'est pas une chaîne exploitable.
fn compute_traces(
    chain: &SourceChain,
    distance_mm: f64,
    side: TopstitchSide,
) -> Result<Vec<Trace>, String> {
    // Chaîne fermée anti-horaire ou ouverte à partir de `from` : l'intérieur et la gauche sont
    // du côté `+d`.
    let d = match side {
        TopstitchSide::Inside | TopstitchSide::Left => distance_mm,
        TopstitchSide::Outside | TopstitchSide::Right => -distance_mm,
    };
    let out =
        offset_chain(&chain.traces, d, &OffsetOptions::default()).map_err(|e| e.to_string())?;
    if matches!(out.as_slice(), [Trace::Circle(_)]) {
        return if chain.closed {
            Ok(out)
        } else {
            Err("le décalage ne donne pas une ligne exploitable".into())
        };
    }
    if out.is_empty()
        || out
            .iter()
            .any(|t| !matches!(t, Trace::Line(..) | Trace::Cubic(_) | Trace::Arc(_)))
    {
        return Err("le décalage ne donne pas une ligne exploitable".into());
    }
    let n = out.len();
    for j in 0..n - 1 {
        if distance_mm_between(&out[j], &out[j + 1]) > CHAIN_GAP_MM {
            return Err(
                "le décalage se sépare en plusieurs morceaux (la forme est trop étroite ou \
                 trop complexe pour cette distance)"
                    .into(),
            );
        }
    }
    if chain.closed && (n < 2 || distance_mm_between(&out[n - 1], &out[0]) > CHAIN_GAP_MM) {
        return Err(
            "le décalage ne donne pas un contour fermé d'un seul tenant (la forme se sépare \
             en plusieurs morceaux ou n'est plus fermée)"
                .into(),
        );
    }
    Ok(out)
}

fn distance_mm_between(a: &Trace, b: &Trace) -> f64 {
    distance_mm(trace_end(a), trace_start(b))
}

/// Sommet numéro `j` d'une chaîne décalée : fermée = `n` sommets (le dernier tracé finit au
/// premier) ; ouverte = `n + 1` sommets. Un arc reste exact : son sommet de fin est celui de
/// l'arc.
fn joint(traces: &[Trace], closed: bool, j: usize) -> PointMm {
    let n = traces.len();
    if closed {
        return joint_at(traces, j % n);
    }
    if j == 0 {
        trace_start(&traces[0])
    } else if j == n {
        trace_end(&traces[n - 1])
    } else {
        match &traces[j - 1] {
            Trace::Arc(a) => a.end_point(),
            _ => trace_start(&traces[j]),
        }
    }
}

/// Plan de mise à jour **sur place** des tracés générés : seulement si leur topologie est
/// exactement celle du décalage (même nombre, même type à chaque rang, points partagés
/// libres) ; `None` sinon.
fn in_place_plan(sheet: &Sheet, ids: &[EntityId], traces: &[Trace], closed: bool) -> Option<Plan> {
    if ids.len() != traces.len() {
        return None;
    }
    let free = |id: PointId| sheet.point(id).is_ok_and(|p| p.definition.is_free());
    let mut plan = Plan {
        points: Vec::new(),
        curves: Vec::new(),
        arcs: Vec::new(),
        circles: Vec::new(),
    };
    if let [Trace::Circle(c)] = traces {
        let EntityKind::Circle { center, .. } = sheet.entity(ids[0]).ok()?.kind else {
            return None;
        };
        if !free(center) {
            return None;
        }
        plan.points.push((center, c.center));
        plan.circles.push((ids[0], c.radius_mm));
        return Some(plan);
    }
    let n = traces.len();
    let mut starts = Vec::with_capacity(n);
    let mut ends = Vec::with_capacity(n);
    for (&id, trace) in ids.iter().zip(traces) {
        match (&sheet.entity(id).ok()?.kind, trace) {
            (EntityKind::Segment { from, to, .. }, Trace::Line(..)) => {
                starts.push(*from);
                ends.push(*to);
            }
            (EntityKind::Curve { start, end, .. }, Trace::Cubic(_)) => {
                starts.push(*start);
                ends.push(*end);
            }
            (
                EntityKind::Arc {
                    center, start, end, ..
                },
                Trace::Arc(a),
            ) => {
                if !free(*center) {
                    return None;
                }
                plan.points.push((*center, a.center));
                plan.arcs.push((id, a.sweep_rad > 0.0));
                starts.push(*start);
                ends.push(*end);
            }
            _ => return None,
        }
    }
    for j in 0..n {
        let next = if j + 1 < n {
            Some(j + 1)
        } else if closed {
            Some(0)
        } else {
            None
        };
        if let Some(next) = next {
            if ends[j] != starts[next] {
                return None;
            }
        }
        if !free(starts[j]) {
            return None;
        }
        plan.points.push((starts[j], joint(traces, closed, j)));
        if let Trace::Cubic(c) = &traces[j] {
            let end_index = if closed { (j + 1) % n } else { j + 1 };
            let (ps, pe) = (joint(traces, closed, j), joint(traces, closed, end_index));
            plan.curves.push((
                ids[j],
                OffsetMm::from_vector(c.c1 - ps),
                OffsetMm::from_vector(c.c2 - pe),
            ));
        }
    }
    if !closed {
        if !free(ends[n - 1]) {
            return None;
        }
        plan.points.push((ends[n - 1], joint(traces, false, n)));
    }
    Some(plan)
}

/// Signature de la géométrie d'un tracé généré : rôle et forme.
type OwnSignature = Vec<Option<(PathRole, Shape)>>;

impl Project {
    /// Crée les tracés d'un décalage (points libres, jamais fusionnés), de rôle surpiqûre.
    fn build_topstitch_geometry(
        &mut self,
        sheet: SheetId,
        traces: &[Trace],
        closed: bool,
    ) -> Result<Vec<EntityId>, String> {
        let role = PathRole::Topstitch;
        if let [Trace::Circle(c)] = traces {
            let center = self.add_free_point(sheet, c.center)?;
            let id = self.add_entity(
                sheet,
                role,
                EntityKind::Circle {
                    center,
                    radius_mm: c.radius_mm,
                    start_deg: 0.0,
                    ccw: true,
                },
            )?;
            return Ok(vec![id]);
        }
        let n = traces.len();
        let count = if closed { n } else { n + 1 };
        let mut joints = Vec::with_capacity(count);
        for j in 0..count {
            joints.push(self.add_free_point(sheet, joint(traces, closed, j))?);
        }
        let mut ids = Vec::with_capacity(n);
        for (j, t) in traces.iter().enumerate() {
            let end_index = if closed { (j + 1) % n } else { j + 1 };
            let (a, b) = (joints[j], joints[end_index]);
            let kind = match t {
                Trace::Line(..) => EntityKind::Segment {
                    from: a,
                    to: b,
                    construction: None,
                },
                Trace::Cubic(c) => EntityKind::Curve {
                    start: a,
                    handle_start: OffsetMm::from_vector(c.c1 - joint(traces, closed, j)),
                    handle_end: OffsetMm::from_vector(c.c2 - joint(traces, closed, end_index)),
                    end: b,
                },
                Trace::Arc(arc) => EntityKind::Arc {
                    center: self.add_free_point(sheet, arc.center)?,
                    start: a,
                    end: b,
                    ccw: arc.sweep_rad > 0.0,
                },
                _ => return Err("le décalage ne donne pas une ligne exploitable".into()),
            };
            ids.push(self.add_entity(sheet, role, kind)?);
        }
        Ok(ids)
    }

    /// Contrôle de la géométrie générée : feuille valide, ligne fermée simple et d'aire
    /// cohérente avec le côté choisi quand le bord est fermé.
    fn verify_topstitch(
        &self,
        sheet: SheetId,
        ids: &[EntityId],
        chain: &SourceChain,
        side: TopstitchSide,
    ) -> Result<(), String> {
        let s = self.sheet(sheet).map_err(|e| e.to_string())?;
        s.validate().map_err(|e| e.to_string())?;
        if !chain.closed {
            return Ok(());
        }
        let bad = |why: &str| format!("le décalage ne donne pas un contour valide ({why})");
        let contour = contour_from_entities(s, ids).map_err(|_| bad("contour non fermé"))?;
        let m = measure_contour(s, &contour).map_err(|e| bad(&e.to_string()))?;
        if m.self_intersecting {
            return Err(bad("le contour se recoupe"));
        }
        let coherent = m.area_mm2.is_finite()
            && m.area_mm2 > 1e-9
            && match side {
                TopstitchSide::Inside => m.area_mm2 < chain.area_mm2 + 1e-6,
                _ => m.area_mm2 > chain.area_mm2 - 1e-6,
            };
        if !coherent {
            return Err(bad("aire incohérente avec le côté et la distance"));
        }
        Ok(())
    }

    fn own_signature(&self, sheet: SheetId, link: &Topstitch) -> OwnSignature {
        let Ok(s) = self.sheet(sheet) else {
            return Vec::new();
        };
        link.paths
            .iter()
            .map(|&id| Some((s.entity(id).ok()?.role, s.shape(id).ok()?)))
            .collect()
    }

    fn source_signature(&self, sheet: SheetId, link: &Topstitch) -> Result<Vec<Trace>, String> {
        let s = self.sheet(sheet).map_err(|e| e.to_string())?;
        self.topstitch_chain(s, &link.source, link.side)
            .map(|c| c.traces)
    }

    /// Recalcule les lignes d'une surpiqûre d'après son bord actuel. Une erreur ne laisse pas
    /// de trace utilisable : l'appelant refuse la commande, ou restaure et marque obsolète.
    fn regenerate_topstitch(
        &mut self,
        sheet: SheetId,
        id: TopstitchId,
        out: &mut Outcome,
    ) -> Result<(), CoreError> {
        let s = self.sheet(sheet)?;
        let link = s.topstitch(id)?.clone();
        let chain = self
            .topstitch_chain(s, &link.source, link.side)
            .map_err(CoreError::Invalid)?;
        let traces =
            compute_traces(&chain, link.distance_mm, link.side).map_err(CoreError::Invalid)?;
        if let Some(&e) = link.paths.iter().find(|&&e| s.entity_locked(e)) {
            return Err(CoreError::LayerLocked(format!(
                "{} : le calque « {} » est verrouillé, la surpiqûre liée ne peut pas être \
                 recalculée (déverrouillez le calque d'abord)",
                s.entity_name(e),
                s.layer_name(s.entity(e)?.layer)
            )));
        }
        let plan = if link.paths.is_empty() {
            None
        } else {
            in_place_plan(s, &link.paths, &traces, chain.closed)
        };
        let new_paths = match plan {
            Some(plan) => {
                self.apply_plan(sheet, plan).map_err(invalid)?;
                link.paths.clone()
            }
            None => {
                let before = self.clone();
                if !link.paths.is_empty() {
                    let mark = out.notes.len();
                    self.delete(sheet, &[], &link.paths, out)?;
                    out.notes.truncate(mark);
                    out.notes.push(
                        "La géométrie d'une surpiqûre liée a changé de structure : ses tracés \
                         sont recréés (nouveaux identifiants)."
                            .to_string(),
                    );
                }
                let fresh = self
                    .build_topstitch_geometry(sheet, &traces, chain.closed)
                    .map_err(CoreError::Invalid)?;
                out.entities.extend(fresh.iter().copied());
                // Ce qui référençait les anciennes lignes est retiré avec remarque.
                self.sheet_mut(sheet)?.topstitch_mut(id)?.paths = fresh.clone();
                self.prune_dead_notch_anchors(&before, out);
                self.prune_dead_internals(&before, out);
                self.prune_dead_markings(&before, out);
                self.prune_dead_buttons(&before, out);
                self.prune_dead_grain(&before, out);
                fresh
            }
        };
        self.verify_topstitch(sheet, &new_paths, &chain, link.side)
            .map_err(CoreError::Invalid)?;
        let l = self.sheet_mut(sheet)?.topstitch_mut(id)?;
        l.paths = new_paths;
        l.stale = None;
        out.sheet = Some(sheet);
        Ok(())
    }

    /// Régénère une surpiqûre dont le bord a changé : en cas d'échec, tout revient à l'identique
    /// et la surpiqûre est marquée obsolète (jamais un refus de la commande qui a changé le bord).
    fn regenerate_topstitch_with_rollback(
        &mut self,
        sheet: SheetId,
        id: TopstitchId,
        out: &mut Outcome,
    ) {
        let snapshot = self.clone();
        let (notes, ents, pts) = (out.notes.len(), out.entities.len(), out.points.len());
        match self.regenerate_topstitch(sheet, id, out) {
            Ok(()) => out.regenerated_topstitches.push(id),
            Err(e) => {
                *self = snapshot;
                out.notes.truncate(notes);
                out.entities.truncate(ents);
                out.points.truncate(pts);
                let reason = clean_reason(&e.to_string());
                if let Ok(l) = self.sheet_mut(sheet).and_then(|s| s.topstitch_mut(id)) {
                    l.stale = Some(reason.clone());
                    out.notes.push(format!(
                        "Une surpiqûre liée n'a pas pu être mise à jour ({reason}) : elle garde \
                         sa dernière géométrie valide."
                    ));
                }
            }
        }
    }

    /// Un bord, une pièce ou une ligne générée supprimés par la commande cassent le lien dans la
    /// même étape, avec une remarque : les lignes qui restent deviennent des tracés libres.
    pub(crate) fn break_dead_topstitches(&mut self, before: &Project, out: &mut Outcome) {
        if self.sheets.iter().all(|s| s.topstitches.is_empty()) {
            return;
        }
        let mut dead: Vec<(SheetId, TopstitchId, &'static str)> = Vec::new();
        for s in &self.sheets {
            for t in &s.topstitches {
                let existed = before.sheet(s.id).is_ok_and(|b| b.topstitch(t.id).is_ok());
                if !existed {
                    continue;
                }
                let reason = if t.paths.iter().any(|&e| s.entity(e).is_err()) {
                    Some("une de ses lignes a été supprimée")
                } else {
                    match &t.source {
                        TopstitchSource::Piece { piece } if self.piece(*piece).is_err() => {
                            Some("la pièce dont elle suivait le contour n'existe plus")
                        }
                        // US-07.2 : une pièce reconstruite sur une feuille dédiée a quitté celle-ci.
                        TopstitchSource::Piece { piece }
                            if self.piece_sheet(*piece).is_ok_and(|ps| ps.id != s.id) =>
                        {
                            Some("la pièce dont elle suivait le contour est sur une autre feuille")
                        }
                        TopstitchSource::Paths { paths, .. }
                            if paths.iter().any(|&e| s.entity(e).is_err()) =>
                        {
                            Some("un tracé du bord a été supprimé")
                        }
                        _ => None,
                    }
                };
                if let Some(r) = reason {
                    dead.push((s.id, t.id, r));
                }
            }
        }
        for (sheet, id, reason) in dead {
            if let Ok(s) = self.sheet_mut(sheet) {
                s.topstitches.retain(|t| t.id != id);
                out.notes.push(format!(
                    "Une surpiqûre n'est plus liée à son bord : {reason}. Les lignes qui restent \
                     sont des tracés libres."
                ));
            }
        }
    }

    /// Après une commande : crée ou recalcule les surpiqûres liées (seule étape qui écrit la
    /// géométrie générée), et refuse de modifier directement une ligne générée.
    pub(crate) fn sync_topstitches(
        &mut self,
        before: &Project,
        out: &mut Outcome,
    ) -> Result<(), CoreError> {
        let todo: Vec<(SheetId, TopstitchId)> = self
            .sheets
            .iter()
            .flat_map(|s| s.topstitches.iter().map(move |t| (s.id, t.id)))
            .collect();
        for (sheet, id) in todo {
            let Ok(link) = self.sheet(sheet).and_then(|s| s.topstitch(id)).cloned() else {
                continue;
            };
            let old = before
                .sheet(sheet)
                .and_then(|s| s.topstitch(id))
                .ok()
                .cloned();
            let Some(old) = old else {
                // Créée par la commande : générée d'après l'état final (échec = refus).
                self.regenerate_topstitch(sheet, id, out)?;
                out.regenerated_topstitches.push(id);
                continue;
            };
            let same_source_kind = match (&old.source, &link.source) {
                (TopstitchSource::Piece { piece: a }, TopstitchSource::Piece { piece: b }) => {
                    a == b
                }
                (TopstitchSource::Paths { .. }, TopstitchSource::Paths { .. }) => true,
                _ => false,
            };
            if old.distance_mm != link.distance_mm || old.side != link.side || !same_source_kind {
                // Paramètres changés par la commande : échec = refus.
                self.regenerate_topstitch(sheet, id, out)?;
                out.regenerated_topstitches.push(id);
                continue;
            }
            let source_changed =
                before.source_signature(sheet, &old) != self.source_signature(sheet, &link);
            let own_changed = before.own_signature(sheet, &old) != self.own_signature(sheet, &link);
            if source_changed {
                // La régénération est la seule à écrire la géométrie d'une ligne liée (même si
                // le bord et la ligne changent ensemble, par exemple en déplaçant tout).
                self.regenerate_topstitch_with_rollback(sheet, id, out);
            } else if own_changed {
                return Err(invalid(
                    "cette ligne est une surpiqûre liée à un bord : sa géométrie est recalculée \
                     d'après lui. Cassez le lien avant de la modifier.",
                ));
            }
        }
        Ok(())
    }

    /// Crée le lien d'une surpiqûre (les lignes sont générées par `sync_topstitches`, dans la
    /// même commande : un échec refuse la commande entière).
    pub(crate) fn add_topstitch(
        &mut self,
        sheet: SheetId,
        source: TopstitchSource,
        distance_mm: f64,
        side: TopstitchSide,
        out: &mut Outcome,
    ) -> Result<(), CoreError> {
        check_topstitch_distance(distance_mm)?;
        let s = self.sheet(sheet)?;
        if s.topstitches.len() >= MAX_TOPSTITCHES_PER_SHEET {
            return Err(invalid(format!(
                "surpiqûre : au plus {MAX_TOPSTITCHES_PER_SHEET} surpiqûres liées par feuille"
            )));
        }
        let source = match source {
            TopstitchSource::Piece { piece } => {
                self.piece(piece)?;
                TopstitchSource::Piece { piece }
            }
            TopstitchSource::Paths { mut paths, from } => {
                paths.sort();
                paths.dedup();
                if paths.is_empty() {
                    return Err(invalid("surpiqûre : aucun tracé de bord indiqué"));
                }
                if paths.len() > MAX_OFFSET_PATHS {
                    return Err(invalid(format!(
                        "surpiqûre : trop de tracés (au plus {MAX_OFFSET_PATHS})"
                    )));
                }
                for &p in &paths {
                    s.entity(p)?;
                }
                TopstitchSource::Paths { paths, from }
            }
        };
        let generated: HashSet<EntityId> = s
            .topstitches
            .iter()
            .flat_map(|t| t.paths.iter().copied())
            .collect();
        let edge_ids: Vec<EntityId> = match &source {
            TopstitchSource::Piece { piece } => self
                .piece(*piece)?
                .outline
                .iter()
                .map(|e| e.entity)
                .collect(),
            TopstitchSource::Paths { paths, .. } => paths.clone(),
        };
        if edge_ids.iter().any(|e| generated.contains(e)) {
            return Err(invalid(
                "surpiqûre : le bord ne peut pas être une ligne de surpiqûre déjà liée à un \
                 autre bord",
            ));
        }
        // Le bord doit être lisible, et son genre doit convenir au côté demandé.
        let chain = self
            .topstitch_chain(s, &source, side)
            .map_err(CoreError::Invalid)?;
        let source = match source {
            TopstitchSource::Paths { paths, .. } => TopstitchSource::Paths {
                paths,
                from: if chain.closed { None } else { chain.start },
            },
            other => other,
        };
        if s.topstitches
            .iter()
            .any(|t| t.source == source && t.side == side && t.distance_mm == distance_mm)
        {
            return Err(invalid(
                "surpiqûre : ce bord a déjà une surpiqûre liée à cette distance et de ce côté",
            ));
        }
        let id = TopstitchId(self.alloc_id());
        self.sheet_mut(sheet)?.topstitches.push(Topstitch {
            id,
            source,
            distance_mm,
            side,
            paths: Vec::new(),
            stale: None,
        });
        out.topstitches.push(id);
        out.sheet = Some(sheet);
        Ok(())
    }

    /// Change la distance et le côté ; les lignes sont recalculées par `sync_topstitches`
    /// (un échec refuse la commande).
    pub(crate) fn set_topstitch(
        &mut self,
        sheet: SheetId,
        topstitch: TopstitchId,
        distance_mm: f64,
        side: TopstitchSide,
    ) -> Result<(), CoreError> {
        check_topstitch_distance(distance_mm)?;
        let s = self.sheet(sheet)?;
        let link = s.topstitch(topstitch)?.clone();
        self.topstitch_chain(s, &link.source, side)
            .map_err(CoreError::Invalid)?;
        let l = self.sheet_mut(sheet)?.topstitch_mut(topstitch)?;
        l.distance_mm = distance_mm;
        l.side = side;
        Ok(())
    }

    /// Casse le lien : les lignes deviennent des tracés libres (rôle surpiqûre conservé).
    pub(crate) fn break_topstitch_link(
        &mut self,
        sheet: SheetId,
        topstitch: TopstitchId,
        out: &mut Outcome,
    ) -> Result<(), CoreError> {
        let s = self.sheet_mut(sheet)?;
        let link = s.topstitch(topstitch)?.clone();
        s.topstitches.retain(|t| t.id != topstitch);
        out.notes.push(format!(
            "La surpiqûre n'est plus liée à son bord : ses {} tracé(s) sont maintenant libres.",
            link.paths.len()
        ));
        out.sheet = Some(sheet);
        Ok(())
    }

    /// Un tracé coupé en deux : le second tronçon prolonge le bord des surpiqûres qui le
    /// suivaient (sinon la ligne ne suivrait plus que la moitié du bord).
    pub(crate) fn topstitch_after_split(&mut self, sheet: SheetId, old: EntityId, new: EntityId) {
        if let Ok(s) = self.sheet_mut(sheet) {
            for t in &mut s.topstitches {
                if let TopstitchSource::Paths { paths, .. } = &mut t.source {
                    if paths.contains(&old) && !paths.contains(&new) {
                        paths.push(new);
                        paths.sort();
                    }
                }
            }
        }
    }

    /// Deux tracés fusionnés (`second` disparaît) : le bord suit `first`.
    pub(crate) fn topstitch_after_merge(
        &mut self,
        sheet: SheetId,
        first: EntityId,
        second: EntityId,
    ) {
        if let Ok(s) = self.sheet_mut(sheet) {
            for t in &mut s.topstitches {
                if let TopstitchSource::Paths { paths, .. } = &mut t.source {
                    if let Some(i) = paths.iter().position(|e| *e == second) {
                        if paths.contains(&first) {
                            paths.remove(i);
                        } else {
                            paths[i] = first;
                        }
                        paths.sort();
                    }
                }
            }
        }
    }

    /// Validation des surpiqûres liées (à l'ouverture et après chaque commande) : bornes,
    /// références vivantes, sur la feuille du lien, lignes générées distinctes et jamais un
    /// bord, rôle surpiqûre.
    pub(crate) fn validate_topstitches(&self) -> Result<(), CoreError> {
        for sheet in &self.sheets {
            if sheet.topstitches.is_empty() {
                continue;
            }
            if sheet.topstitches.len() > MAX_TOPSTITCHES_PER_SHEET {
                return Err(invalid(format!(
                    "la feuille « {} » a trop de surpiqûres liées (au plus \
                     {MAX_TOPSTITCHES_PER_SHEET})",
                    sheet.name
                )));
            }
            let mut generated: HashSet<EntityId> = HashSet::new();
            for t in &sheet.topstitches {
                check_topstitch_distance(t.distance_mm)?;
                if let Some(reason) = &t.stale {
                    if reason.trim().is_empty()
                        || reason.chars().count() > MAX_STALE_LEN
                        || reason.chars().any(|c| c.is_control())
                    {
                        return Err(invalid(
                            "une surpiqûre liée porte une raison d'obsolescence invalide",
                        ));
                    }
                }
                if t.paths.is_empty() {
                    return Err(invalid("une surpiqûre liée n'a aucune ligne générée"));
                }
                for &p in &t.paths {
                    let e = sheet.entity(p).map_err(|_| {
                        CoreError::DeadReference(format!(
                            "une surpiqûre liée référence une ligne absente (id {})",
                            p.0
                        ))
                    })?;
                    if e.role != PathRole::Topstitch {
                        return Err(invalid(format!(
                            "{} est une ligne de surpiqûre liée : son rôle doit rester « surpiqûre »",
                            sheet.entity_name(p)
                        )));
                    }
                    if !generated.insert(p) {
                        return Err(invalid(format!(
                            "le tracé {} est généré par deux surpiqûres liées",
                            p.0
                        )));
                    }
                }
                match &t.source {
                    TopstitchSource::Piece { piece } => {
                        let ok = self
                            .piece_sheet(*piece)
                            .map_err(|_| {
                                CoreError::DeadReference(format!(
                                    "une surpiqûre liée suit une pièce absente (id {})",
                                    piece.0
                                ))
                            })?
                            .id
                            == sheet.id;
                        if !ok {
                            return Err(invalid(
                                "une surpiqûre liée suit le contour d'une pièce d'une autre feuille",
                            ));
                        }
                        if !t.side.is_closed_side() {
                            return Err(invalid(
                                "une surpiqûre liée au contour d'une pièce se place à l'intérieur ou à l'extérieur",
                            ));
                        }
                    }
                    TopstitchSource::Paths { paths, from } => {
                        if paths.is_empty() || paths.len() > MAX_OFFSET_PATHS {
                            return Err(invalid("une surpiqûre liée a un bord vide ou trop long"));
                        }
                        let mut seen = HashSet::new();
                        for &p in paths {
                            if sheet.entity(p).is_err() {
                                return Err(CoreError::DeadReference(format!(
                                    "une surpiqûre liée suit un tracé absent (id {})",
                                    p.0
                                )));
                            }
                            if !seen.insert(p) {
                                return Err(invalid(
                                    "le bord d'une surpiqûre liée cite deux fois le même tracé",
                                ));
                            }
                        }
                        match (from, t.side.is_closed_side()) {
                            (None, true) => {}
                            (Some(f), false) => {
                                if sheet.point(*f).is_err() {
                                    return Err(CoreError::DeadReference(format!(
                                        "une surpiqûre liée part d'un point absent (id {})",
                                        f.0
                                    )));
                                }
                            }
                            _ => {
                                return Err(invalid(
                                    "une surpiqûre liée : le côté (intérieur / extérieur ou \
                                     gauche / droite) ne correspond pas à la forme du bord",
                                ))
                            }
                        }
                    }
                }
            }
            // Un bord n'est jamais une ligne générée (pas de surpiqûre de surpiqûre).
            for t in &sheet.topstitches {
                let edge: Vec<EntityId> = match &t.source {
                    TopstitchSource::Piece { piece } => self
                        .piece(*piece)
                        .map(|p| p.outline.iter().map(|e| e.entity).collect())
                        .unwrap_or_default(),
                    TopstitchSource::Paths { paths, .. } => paths.clone(),
                };
                if edge.iter().any(|e| generated.contains(e)) {
                    return Err(invalid(
                        "le bord d'une surpiqûre liée ne peut pas être une ligne de surpiqûre liée",
                    ));
                }
            }
        }
        Ok(())
    }
}
