//! US-03.3 — **longueur développée imposée** à une courbe de Bézier (premier sous-ensemble : 03.3a).
//!
//! Une contrainte = `{ courbe, longueur cible, inconnue }` dans `Sheet.length_constraints`. L'inconnue
//! est **une seule** grandeur, au choix :
//! - `Handle { point }` : la **longueur de la poignée** attachée à cette extrémité (sa direction ne
//!   change pas) ;
//! - `Slide { point, angle_deg }` : cette extrémité (point **libre**) se déplace **le long d'une
//!   droite** de direction `angle_deg` (anti-horaire depuis +X) passant par sa position courante.
//!
//! La valeur de l'inconnue reste **écrite** dans le modèle (poignée ou coordonnées du point libre),
//! comme les constructions maintenues (US-03.2) : un lecteur qui ignore la contrainte voit la bonne
//! géométrie, retirer la contrainte garde la géométrie courante.
//!
//! La longueur développée d'une Bézier est une fonction **convexe** de l'inconnue (sa dérivée est
//! affine en l'inconnue, la norme d'une fonction affine est convexe) : le solveur
//! (`pf_geometry::solve`, ADR-B : une inconnue, bissection / Brent, aucun solveur global) localise le
//! minimum, prend la **branche monotone qui contient la position courante** et refuse, avec les bornes,
//! une cible hors de la plage. Une contrainte non résolue ne modifie rien : la commande entière est
//! refusée sans trace.
//!
//! Règles par opération (`Project::sync_length`, appelée par `Project::apply`) :
//! - parent déplacé, variable ou formule changée, transformation : la contrainte est **re-résolue**
//!   dans la même étape, dans l'ordre des dépendances (cycle = refus) ; impossible = commande refusée ;
//! - inconnue modifiée directement (poignée glissée, extrémité déplacée) : la contrainte l'emporte et
//!   ramène la longueur à la cible (désactivez-la pour déplacer librement) ;
//! - courbe supprimée, extrémités changées (couper, prolonger, fusionner), point ajusté redéfini ou
//!   devenu point d'arc / de construction maintenue : contrainte **retirée avec remarque** ;
//! - copie de feuille / import : contrainte conservée (identifiants remappés) ; presse-papiers :
//!   copie sans contrainte, avec remarque ; dupliquer : la copie n'a pas de contrainte ;
//! - retourner un tracé garde la contrainte (elle désigne le point, pas « début / fin »).
//!
//! Invariant (ouverture et après chaque commande) : structure valide, longueur réelle = cible à
//! 1e-6 mm. Rejeté : contrainte sur courbe lisse, sur segment, arc, cercle (autre mécanisme).
//!
//! **Égalité de longueurs (US-03.4a)** : la cible peut être « longueur d'un **autre tracé** de la
//! feuille + décalage signé » (`reference`, ex. tête de manche = emmanchure + embu). Même mécanisme :
//! la valeur calculée reste écrite dans `length_mm` (un lecteur qui ignore le lien voit une longueur
//! imposée ordinaire), recalculée à chaque étape **après** la référence (ordre topologique : la
//! référence peut elle-même être imposée, en chaîne ; cycle = refus). Référence : tout tracé dont la
//! longueur est déjà calculée (segment, Bézier, courbe lisse, arc, cercle). Cible <= 0 ou hors plage =
//! commande refusée sans trace ; référence supprimée, coupée, prolongée ou fusionnée = contrainte
//! retirée avec remarque ; copie de feuille : référence remappée ; presse-papiers : sans contrainte.
//!
//! **Cibles étendues (US-03.4b)** : l'égalité peut aussi viser (1) un tracé **d'une autre feuille**,
//! (2) la **somme** de plusieurs tracés (de feuilles quelconques), (3) un décalage **maintenu par une
//! formule** sur les variables. Format : `reference` (premier tracé, décalage évalué) reste tel
//! quel ; l'objet optionnel `link` (omis quand l'égalité est simple : même feuille, un seul tracé,
//! décalage en nombre) porte la feuille du premier tracé, les tracés suivants et la formule du
//! décalage. Un ancien fichier se relit et se réécrit à l'identique. L'ordre de résolution est
//! **global** (toutes feuilles) : un cycle, même entre deux feuilles, est refusé. Tracé, feuille ou
//! extrémités de référence supprimés / changés = contrainte retirée avec remarque.

use crate::command::Outcome;
use crate::error::CoreError;
use crate::expr;
use crate::input::format_number;
use crate::model::{
    EntityId, EntityKind, OffsetMm, PointDefinition, PointId, Project, Sheet, SheetId,
};
use pf_geometry::solve::{solve_convex, solve_nearest, SolveError, SOLVE_TOLERANCE_MM};
use pf_geometry::{CubicBezier, GeometryError, PointMm, VectorMm};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Précision garantie sur la longueur développée (mm) : critère d'acceptation (CA 1).
pub const LENGTH_TOLERANCE_MM: f64 = SOLVE_TOLERANCE_MM;
/// Plus courte poignée ajustable (mm) : en dessous, sa direction est indéterminée.
pub const MIN_HANDLE_MM: f64 = 1e-3;
/// Plus grande longueur imposée acceptée (mm).
pub const MAX_IMPOSED_LENGTH_MM: f64 = 1.0e6;
/// Écart en dessous duquel la contrainte est déjà satisfaite (rien n'est recalculé ni écrit).
const SETTLED_MM: f64 = 1e-8;
/// Tolérance de la quadrature de la longueur (plus fine que l'affichage : 1e-6).
const QUADRATURE_MM: f64 = 1e-9;
/// Étendue de la recherche, en multiples de `cible + corde + 1`.
const SEARCH_SPAN: f64 = 20.0;

/// Ce qui s'ajuste pour atteindre la longueur imposée.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum LengthAdjust {
    /// Longueur de la poignée attachée à cette extrémité (direction inchangée).
    Handle { point: PointId },
    /// Extrémité libre déplacée le long d'une droite de direction `angle_deg` (degrés, anti-horaire
    /// depuis +X) passant par sa position courante.
    Slide { point: PointId, angle_deg: f64 },
}

impl LengthAdjust {
    /// Extrémité concernée.
    pub fn point(self) -> PointId {
        match self {
            LengthAdjust::Handle { point } | LengthAdjust::Slide { point, .. } => point,
        }
    }
}

/// Cible « égale à la longueur d'un autre tracé + décalage » (US-03.4a).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LengthRef {
    /// Tracé de référence (même feuille ; jamais la courbe contrainte elle-même).
    pub entity: EntityId,
    /// Décalage signé (mm) ajouté à la longueur de la référence (embu : positif).
    pub offset_mm: f64,
}

/// Un tracé de la somme d'une égalité de longueurs (US-03.4b) ; `sheet` = `None` : même feuille que
/// la courbe contrainte.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LengthTerm {
    pub entity: EntityId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sheet: Option<SheetId>,
}

/// Extension d'une égalité de longueurs (US-03.4b), omise tant que l'égalité est simple. Jamais
/// vide : `reference` est présente et au moins un champ est renseigné.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LengthLink {
    /// Feuille du **premier** tracé (`reference.entity`) ; `None` = feuille de la courbe.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sheet: Option<SheetId>,
    /// Tracés ajoutés à la somme (le premier est `reference`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub more: Vec<LengthTerm>,
    /// Décalage **maintenu par une formule** : `reference.offset_mm` en est la valeur évaluée,
    /// laissée écrite.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset_expr: Option<String>,
}

/// Égalité de longueurs demandée (commande `SetCurveLengthEquality`) : somme des `terms` + décalage
/// (`offset_expr` l'emporte sur `offset_mm`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LengthEquality {
    pub terms: Vec<LengthTerm>,
    pub offset_mm: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset_expr: Option<String>,
}

/// Demande de contrainte (commande `SetCurveLengthConstraint`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LengthSpec {
    /// Longueur imposée (mm). **Ignorée** si `reference` est donnée : la cible est alors
    /// recalculée (longueur de la référence + décalage).
    pub length_mm: f64,
    pub adjust: LengthAdjust,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference: Option<LengthRef>,
}

/// Contrainte stockée dans `Sheet.length_constraints` : une par courbe.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CurveLength {
    pub curve: EntityId,
    /// Longueur imposée ; avec `reference`, valeur calculée (référence + décalage) laissée écrite.
    pub length_mm: f64,
    pub adjust: LengthAdjust,
    /// Cible égale à la longueur d'un autre tracé + décalage (US-03.4a), omise quand absente.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference: Option<LengthRef>,
    /// Cible **maintenue par une formule** (US-03.3b) : texte d'une expression sur les variables,
    /// `length_mm` en est la valeur évaluée, laissée écrite ; omise quand absente.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expr: Option<String>,
    /// Extension de l'égalité (US-03.4b : autre feuille, somme, décalage par formule), omise quand
    /// l'égalité est simple ou absente.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<LengthLink>,
}

impl CurveLength {
    /// Tracés dont la somme est la base de l'égalité (vide sans égalité).
    pub fn terms(&self) -> Vec<LengthTerm> {
        let Some(r) = self.reference else {
            return Vec::new();
        };
        let mut v = vec![LengthTerm {
            entity: r.entity,
            sheet: self.link.as_ref().and_then(|l| l.sheet),
        }];
        if let Some(l) = &self.link {
            v.extend(l.more.iter().copied());
        }
        v
    }

    /// Formule du décalage, s'il y en a une.
    pub fn offset_expr(&self) -> Option<&str> {
        self.link.as_ref().and_then(|l| l.offset_expr.as_deref())
    }

    /// Texte des formules (cible et décalage) qui citent des variables.
    pub fn formula_texts(&self) -> impl Iterator<Item = &str> {
        self.expr.as_deref().into_iter().chain(self.offset_expr())
    }
}

/// Longueur développée d'une Bézier ; une courbe réduite à un point mesure 0.
fn bezier_len(c: &CubicBezier) -> Result<f64, String> {
    match c.length_mm(QUADRATURE_MM) {
        Ok(v) => Ok(v),
        Err(GeometryError::DegenerateCurve) => Ok(0.0),
        Err(e) => Err(e.to_string()),
    }
}

/// Course permise `(arrière ; avant)` d'un point qui part de `origin` dans la direction `dir` sans
/// quitter l'espace de travail (la recherche n'évalue jamais une position invalide).
fn room(origin: PointMm, dir: VectorMm) -> (f64, f64) {
    let limit = pf_geometry::MAX_COORD_MM * 0.999;
    let along = |o: f64, d: f64| -> (f64, f64) {
        if d.abs() < 1e-12 {
            (f64::INFINITY, f64::INFINITY)
        } else if d > 0.0 {
            ((o + limit) / d, (limit - o) / d)
        } else {
            ((limit - o) / -d, (o + limit) / -d)
        }
    };
    let (bx, fx) = along(origin.x, dir.x);
    let (by, fy) = along(origin.y, dir.y);
    (bx.min(by).max(0.0), fx.min(fy).max(0.0))
}

/// Longueur développée d'une Bézier ou d'une courbe lisse (somme de ses tronçons), à la précision
/// de la quadrature du solveur ; `Err` = message déjà lisible.
fn developed(sheet: &Sheet, curve: EntityId) -> Result<f64, String> {
    match sheet.entity(curve).map_err(|e| e.to_string())?.kind {
        EntityKind::Spline { .. } => {
            let spline = sheet.spline_geometry(curve).map_err(|e| e.to_string())?;
            let mut total = 0.0;
            for seg in spline.segments() {
                total += bezier_len(seg)?;
            }
            Ok(total)
        }
        _ => {
            let g = sheet.curve_geometry(curve).map_err(|e| e.to_string())?;
            bezier_len(&g)
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Node {
    Point(PointId),
    Entity(EntityId),
}

fn mm(x: f64) -> String {
    format_number(x, 3)
}

/// Nombre signé (`+12`, `-3`) pour un décalage.
fn signed_mm(x: f64) -> String {
    if x >= 0.0 {
        format!("+{}", mm(x))
    } else {
        mm(x)
    }
}

impl Sheet {
    /// Contrainte de longueur de cette courbe, s'il y en a une.
    pub fn length_constraint(&self, curve: EntityId) -> Option<&CurveLength> {
        self.length_constraints.iter().find(|c| c.curve == curve)
    }

    /// Contrainte dont l'extrémité **déplacée** (`Slide`) est ce point.
    pub fn length_constraint_moving_point(&self, point: PointId) -> Option<&CurveLength> {
        self.length_constraints
            .iter()
            .find(|c| matches!(c.adjust, LengthAdjust::Slide { point: p, .. } if p == point))
    }

    /// Longueur développée courante d'une courbe de Bézier ou d'une courbe lisse, avec la précision
    /// du solveur.
    pub fn curve_developed_mm(&self, curve: EntityId) -> Result<f64, CoreError> {
        developed(self, curve)
            .map_err(|m| CoreError::Invalid(format!("{} : {m}", self.entity_name(curve))))
    }

    fn is_arc_point(&self, point: PointId) -> bool {
        self.entities.iter().any(|e| {
            matches!(e.kind, EntityKind::Arc { center, start, end, .. }
                if center == point || start == point || end == point)
        })
    }

    /// Structure d'une contrainte : courbe de Bézier existante, cible valide, extrémité de la courbe,
    /// point déplacé libre et non suivi par une autre construction.
    pub(crate) fn check_length_structure(
        &self,
        proj: &Project,
        c: &CurveLength,
    ) -> Result<(), CoreError> {
        let name = self.entity_name(c.curve);
        let bad = |m: &str| CoreError::Invalid(format!("{name} (longueur imposée) : {m}"));
        let (start, end, smooth) = match &self.entity(c.curve)?.kind {
            EntityKind::Curve { start, end, .. } => (*start, *end, false),
            EntityKind::Spline { nodes } => match (nodes.first(), nodes.last()) {
                (Some(a), Some(b)) => (a.point, b.point, true),
                _ => return Err(bad("courbe lisse sans point de passage")),
            },
            _ => {
                return Err(bad(
                    "seule une courbe de Bézier ou une courbe lisse peut avoir une longueur \
                     imposée (pas un segment, un arc ni un cercle)",
                ))
            }
        };
        if smooth && matches!(c.adjust, LengthAdjust::Handle { .. }) {
            return Err(bad(
                "une courbe lisse n'a pas de poignée à ajuster : choisissez une extrémité qui \
                 glisse le long d'une direction",
            ));
        }
        if c.reference.is_some() && c.expr.is_some() {
            return Err(bad(
                "la cible est une formule ou la longueur d'un autre tracé, pas les deux",
            ));
        }
        proj.check_length_link(self, c)?;
        if !(c.length_mm.is_finite() && c.length_mm > 0.0 && c.length_mm <= MAX_IMPOSED_LENGTH_MM) {
            return Err(bad(&format!(
                "la longueur imposée doit être un nombre de mm strictement positif, au plus {}",
                mm(MAX_IMPOSED_LENGTH_MM)
            )));
        }
        if start == end {
            return Err(bad("les deux extrémités de la courbe sont le même point"));
        }
        let point = c.adjust.point();
        if point != start && point != end {
            return Err(bad("le point ajusté n'est pas une extrémité de la courbe"));
        }
        if let LengthAdjust::Slide { angle_deg, .. } = c.adjust {
            if !angle_deg.is_finite() {
                return Err(bad("la direction de déplacement n'est pas un nombre fini"));
            }
            let p = self.point(point)?;
            if !p.definition.is_free() {
                return Err(bad(&format!(
                    "le point {} est construit à partir d'autres tracés : seule une extrémité \
                     libre peut être déplacée",
                    p.label
                )));
            }
            if self.is_arc_point(point) {
                return Err(bad(&format!(
                    "le point {} est le centre ou une extrémité d'un arc : il ne peut pas être \
                     déplacé par une longueur imposée",
                    p.label
                )));
            }
            if let Some(seg) = self.kept_segment_of_point(point) {
                return Err(bad(&format!(
                    "le point {} est écrit par la construction maintenue de {}",
                    p.label,
                    self.entity_name(seg)
                )));
            }
        }
        Ok(())
    }

    /// Longueur d'un tracé de référence (mm) : Bézier à la précision du solveur, autres tracés par
    /// la mesure observée (`measure::entity_length`).
    pub fn reference_length_mm(&self, entity: EntityId) -> Result<f64, CoreError> {
        match self.entity(entity)?.kind {
            EntityKind::Curve { .. } => self.curve_developed_mm(entity),
            _ => Ok(crate::measure::entity_length(self, entity)?.value),
        }
    }

    /// Points et tracés dont dépend la géométrie de la courbe.
    fn length_reads(&self, curve: EntityId) -> HashSet<Node> {
        let mut seen = HashSet::new();
        let mut stack = vec![Node::Entity(curve)];
        while let Some(n) = stack.pop() {
            if !seen.insert(n) {
                continue;
            }
            match n {
                Node::Entity(e) => {
                    if let Ok(ent) = self.entity(e) {
                        stack.extend(ent.kind.all_points().into_iter().map(Node::Point));
                    }
                }
                Node::Point(p) => {
                    if let Ok(pt) = self.point(p) {
                        stack.extend(
                            pt.definition
                                .point_dependencies()
                                .into_iter()
                                .map(Node::Point),
                        );
                        stack.extend(pt.definition.dependencies().into_iter().map(Node::Entity));
                    }
                }
            }
        }
        seen
    }

    /// `desc` : description de l'égalité de longueurs (calculée par le projet : feuilles, somme).
    fn explain_length_error(
        &self,
        c: &CurveLength,
        desc: Option<&str>,
        e: SolveError,
    ) -> CoreError {
        let name = self.entity_name(c.curve);
        let target = match (desc, &c.expr) {
            (Some(d), _) => format!("{} mm (égale à {d})", mm(c.length_mm)),
            (None, Some(text)) => format!("{} mm (formule « {text} »)", mm(c.length_mm)),
            (None, None) => format!("{} mm", mm(c.length_mm)),
        };
        let (how, span) = match c.adjust {
            LengthAdjust::Handle { point } => (
                format!("en ajustant la poignée du point {}", self.label(point)),
                "poignée de",
            ),
            LengthAdjust::Slide { point, angle_deg } => (
                format!(
                    "en déplaçant le point {} suivant la direction {}°",
                    self.label(point),
                    mm(angle_deg)
                ),
                "déplacement de",
            ),
        };
        let m = match e {
            SolveError::OutOfRange {
                min, max, lo, hi, ..
            } => format!(
                "la longueur imposée de {target} n'a pas de solution {how} : sur la plage \
                 explorée ({span} {} à {} mm), la longueur développée va de {} à {} mm",
                mm(lo),
                mm(hi),
                mm(min),
                mm(max)
            ),
            SolveError::NotMonotone { .. } => format!(
                "la longueur imposée de {target} a plusieurs solutions {how} : la longueur \
                 n'est pas monotone sur la plage explorée (changez de direction ou d'inconnue)"
            ),
            SolveError::NoConvergence { residual } => format!(
                "la longueur imposée de {target} n'est pas atteinte {how} (écart restant {} mm)",
                format_number(residual, 9)
            ),
            other => {
                format!("la longueur imposée de {target} ne peut pas être résolue : {other}")
            }
        };
        CoreError::Invalid(format!("{name} : {m}"))
    }

    /// Résout une contrainte : écrit l'inconnue si la longueur courante s'écarte de la cible.
    /// `Ok(true)` si quelque chose a été écrit ; erreur expliquée sinon (rien n'est écrit).
    pub(crate) fn solve_length(
        &mut self,
        c: &CurveLength,
        desc: Option<&str>,
    ) -> Result<bool, CoreError> {
        let name = self.entity_name(c.curve);
        let current =
            developed(self, c.curve).map_err(|m| CoreError::Invalid(format!("{name} : {m}")))?;
        if (current - c.length_mm).abs() <= SETTLED_MM {
            return Ok(false);
        }
        let smooth = matches!(self.entity(c.curve)?.kind, EntityKind::Spline { .. });
        let (first, last) = self.entity_points(c.curve)?;
        let (pa, pb) = (self.position(first)?, self.position(last)?);
        let chord = (pa.x - pb.x).hypot(pa.y - pb.y);
        let span = SEARCH_SPAN * (c.length_mm + chord + 1.0);
        match c.adjust {
            LengthAdjust::Handle { point } => {
                let geom = self.curve_geometry(c.curve)?;
                let EntityKind::Curve {
                    start,
                    handle_start,
                    handle_end,
                    ..
                } = self.entity(c.curve)?.kind
                else {
                    return Err(CoreError::NotACurve(c.curve));
                };
                let at_start = point == start;
                let v = if at_start { handle_start } else { handle_end }.vector();
                let s0 = v.norm();
                if s0 < MIN_HANDLE_MM {
                    return Err(CoreError::Invalid(format!(
                        "{name} : la poignée du point {} est de longueur nulle, sa direction est \
                         indéterminée : la longueur imposée ne peut pas l'ajuster",
                        self.label(point)
                    )));
                }
                let u = v * (1.0 / s0);
                let mut f = |s: f64| {
                    let mut g = geom;
                    if at_start {
                        g.c1 = g.p0 + u * s;
                    } else {
                        g.c2 = g.p3 + u * s;
                    }
                    bezier_len(&g)
                };
                let hi = (span + s0).min(room(geom.p0, u).1.min(room(geom.p3, u).1));
                let s = solve_convex(
                    &mut f,
                    s0,
                    MIN_HANDLE_MM,
                    hi,
                    c.length_mm,
                    LENGTH_TOLERANCE_MM,
                )
                .map_err(|e| self.explain_length_error(c, desc, e))?;
                let offset = OffsetMm::from_vector(u * s);
                let entity = self.entity_mut(c.curve)?;
                if let EntityKind::Curve {
                    handle_start,
                    handle_end,
                    ..
                } = &mut entity.kind
                {
                    if at_start {
                        *handle_start = offset;
                    } else {
                        *handle_end = offset;
                    }
                }
            }
            LengthAdjust::Slide { point, angle_deg } => {
                let (sin, cos) = angle_deg.to_radians().sin_cos();
                let dir = VectorMm::new(cos, sin);
                let origin = self.position(point)?;
                let mut scratch = self.clone();
                let Some(idx) = scratch.points.iter().position(|p| p.id == point) else {
                    return Err(CoreError::UnknownPoint(point));
                };
                let curve = c.curve;
                let mut f = |s: f64| {
                    let at = origin + dir * s;
                    scratch.points[idx].definition = PointDefinition::Free { x: at.x, y: at.y };
                    developed(&scratch, curve)
                };
                let handle = match self.entity(c.curve)?.kind {
                    EntityKind::Curve {
                        start,
                        handle_start,
                        handle_end,
                        ..
                    } => if point == start {
                        handle_start
                    } else {
                        handle_end
                    }
                    .vector(),
                    EntityKind::Spline { .. } => VectorMm::new(0.0, 0.0),
                    _ => return Err(CoreError::NotACurve(c.curve)),
                };
                let (back_a, fwd_a) = room(origin, dir);
                let (back_b, fwd_b) = room(origin + handle, dir);
                let (lo, hi) = (-span.min(back_a).min(back_b), span.min(fwd_a).min(fwd_b));
                // Bézier : longueur convexe en l'inconnue (branche de la position courante) ;
                // courbe lisse : fonction quelconque, racine la plus proche de la position courante.
                let found = if smooth {
                    solve_nearest(&mut f, 0.0, lo, hi, c.length_mm, LENGTH_TOLERANCE_MM)
                } else {
                    solve_convex(&mut f, 0.0, lo, hi, c.length_mm, LENGTH_TOLERANCE_MM)
                };
                let s = found.map_err(|e| self.explain_length_error(c, desc, e))?;
                let at = (origin + dir * s).validate()?;
                self.write_free_point(point, at);
            }
        }
        let after = self.curve_developed_mm(c.curve)?;
        if (after - c.length_mm).abs() > LENGTH_TOLERANCE_MM {
            return Err(self.explain_length_error(
                c,
                desc,
                SolveError::NoConvergence {
                    residual: (after - c.length_mm).abs(),
                },
            ));
        }
        Ok(true)
    }

    /// Pourquoi une contrainte déjà stockée n'a plus de sens après une commande (`None` = valide).
    /// `proj` : projet courant (feuilles des références) ; `old` : projet d'avant la commande.
    fn length_obsolete_reason(
        &self,
        proj: &Project,
        c: &CurveLength,
        old: Option<&Project>,
    ) -> Option<String> {
        if self.entity(c.curve).is_err() {
            return Some("la courbe a été supprimée".into());
        }
        for t in c.terms() {
            match proj.term_sheet(self, t.sheet) {
                Err(_) => {
                    return Some("la feuille du tracé de référence a été supprimée".into());
                }
                Ok(s) if s.entity(t.entity).is_err() => {
                    return Some("le tracé de référence a été supprimé".into());
                }
                Ok(_) => {}
            }
        }
        if let Err(e) = self.check_length_structure(proj, c) {
            let m = e.to_string();
            return Some(
                m.split_once("(longueur imposée) : ")
                    .map_or(m.clone(), |(_, r)| r.to_string()),
            );
        }
        let ends =
            |s: &Sheet, e: EntityId| s.entity_points(e).ok().map(|(a, b)| (a.min(b), a.max(b)));
        fn of(p: &Project, sheet: SheetId) -> Option<&Sheet> {
            p.sheets.iter().find(|s| s.id == sheet)
        }
        let before = old
            .and_then(|o| of(o, self.id))
            .and_then(|s| ends(s, c.curve));
        if before.is_some() && before != ends(self, c.curve) {
            return Some(
                "les extrémités de la courbe ont changé (tracé coupé, prolongé ou fusionné)".into(),
            );
        }
        for t in c.terms() {
            let sid = t.sheet.unwrap_or(self.id);
            let before = old.and_then(|o| of(o, sid)).and_then(|s| ends(s, t.entity));
            let now = of(proj, sid).and_then(|s| ends(s, t.entity));
            if before.is_some() && before != now {
                return Some(
                    "les extrémités du tracé de référence ont changé (tracé coupé, prolongé ou \
                     fusionné)"
                        .into(),
                );
            }
        }
        None
    }
}

/// Désignation d'un tracé de référence ; la feuille est nommée quand ce n'est pas celle de la courbe.
fn term_label(home: &Sheet, found: Option<&Sheet>, entity: EntityId) -> String {
    match found {
        Some(s) if s.id != home.id => {
            format!("{} (feuille « {} »)", s.entity_name(entity), s.name)
        }
        Some(s) => s.entity_name(entity),
        None => format!("tracé {} d'une feuille absente", entity.0),
    }
}

/// Une contrainte vue du plan de résolution : feuille, rang dans la feuille.
type PlanItem = (usize, usize);

/// Cycle refusé : message expliqué et feuilles concernées.
type PlanError = (CoreError, Vec<SheetId>);

/// Ordre de résolution **global** (toutes feuilles) : une contrainte vient après celles qui
/// écrivent ce que lisent sa courbe **et** ses tracés de référence (chaînes A = B + 3, B = C + 2,
/// y compris d'une feuille à l'autre). Cycle = erreur expliquée et feuilles concernées.
fn plan_lengths(sheets: &[&Sheet]) -> Result<Vec<PlanItem>, PlanError> {
    let items: Vec<PlanItem> = sheets
        .iter()
        .enumerate()
        .flat_map(|(si, s)| (0..s.length_constraints.len()).map(move |ci| (si, ci)))
        .collect();
    let n = items.len();
    let sheet_of = |id: SheetId| sheets.iter().find(|s| s.id == id).copied();
    let constraint = |i: usize| &sheets[items[i].0].length_constraints[items[i].1];
    let ref_reads: Vec<Option<HashSet<Node>>> = (0..n)
        .map(|i| {
            let (si, c) = (items[i].0, constraint(i));
            c.reference?;
            let mut set = HashSet::new();
            for t in c.terms() {
                let home = sheets[si];
                if let Some(s) = t.sheet.map_or(Some(home), sheet_of) {
                    set.extend(s.length_reads(t.entity));
                }
            }
            Some(set)
        })
        .collect();
    let reads: Vec<HashSet<Node>> = (0..n)
        .map(|i| {
            let mut r = sheets[items[i].0].length_reads(constraint(i).curve);
            if let Some(rr) = &ref_reads[i] {
                r.extend(rr.iter().copied());
            }
            r
        })
        .collect();
    let writes: Vec<Node> = (0..n)
        .map(|i| match constraint(i).adjust {
            LengthAdjust::Handle { .. } => Node::Entity(constraint(i).curve),
            LengthAdjust::Slide { point, .. } => Node::Point(point),
        })
        .collect();
    let names_of = |i: usize| -> String {
        let (si, c) = (items[i].0, constraint(i));
        c.terms()
            .iter()
            .map(|t| {
                let s = t.sheet.map_or(Some(sheets[si]), sheet_of);
                term_label(sheets[si], s, t.entity)
            })
            .collect::<Vec<_>>()
            .join(" + ")
    };
    for (j, rr) in ref_reads.iter().enumerate() {
        if rr.as_ref().is_some_and(|rr| rr.contains(&writes[j])) {
            let (si, c) = (items[j].0, constraint(j));
            return Err((
                CoreError::Invalid(format!(
                    "dépendance circulaire : la longueur imposée de {} est égale à celle de {}, \
                     qui dépend de ce que cette contrainte modifie",
                    sheets[si].entity_name(c.curve),
                    names_of(j)
                )),
                vec![sheets[si].id],
            ));
        }
    }
    let mut waiting = vec![0usize; n];
    let mut after: Vec<Vec<usize>> = vec![Vec::new(); n];
    for k in 0..n {
        for j in 0..n {
            if k != j && reads[j].contains(&writes[k]) {
                after[k].push(j);
                waiting[j] += 1;
            }
        }
    }
    let mut order = Vec::with_capacity(n);
    let mut ready: Vec<usize> = (0..n).filter(|&i| waiting[i] == 0).collect();
    while let Some(i) = ready.first().copied() {
        ready.remove(0);
        order.push(i);
        for &j in &after[i] {
            waiting[j] -= 1;
            if waiting[j] == 0 {
                ready.push(j);
                ready.sort_unstable();
            }
        }
    }
    if order.len() != n {
        let stuck: Vec<usize> = (0..n).filter(|i| !order.contains(i)).collect();
        let names: Vec<String> = stuck
            .iter()
            .map(|&i| sheets[items[i].0].entity_name(constraint(i).curve))
            .collect();
        let mut involved: Vec<SheetId> = stuck.iter().map(|&i| sheets[items[i].0].id).collect();
        involved.dedup();
        return Err((
            CoreError::Invalid(format!(
                "dépendance circulaire entre longueurs imposées : {}",
                names.join(", ")
            )),
            involved,
        ));
    }
    Ok(order.into_iter().map(|i| items[i]).collect())
}

/// Cible calculée d'une égalité de longueurs.
pub(crate) struct LinkTarget {
    /// Désignation de la somme (`A + B`).
    pub text: String,
    /// Somme des longueurs des tracés de référence (mm).
    pub base: f64,
    /// Décalage évalué (mm).
    pub offset: f64,
    /// `base + offset`.
    pub target: f64,
    /// Description pour les messages du solveur (`A + B = 120 mm +3 mm`).
    pub desc: String,
}

impl Project {
    /// Feuille d'un tracé de référence : celle de la courbe (`home`) quand `sheet` est `None`.
    pub(crate) fn term_sheet<'a>(
        &'a self,
        home: &'a Sheet,
        sheet: Option<SheetId>,
    ) -> Result<&'a Sheet, CoreError> {
        match sheet {
            None => Ok(home),
            Some(id) if id == home.id => Ok(home),
            Some(id) => self.sheet(id),
        }
    }

    /// Ordre de résolution des longueurs imposées de **toutes** les feuilles ; cycle = erreur
    /// expliquée avec les feuilles concernées.
    pub(crate) fn length_plan(&self) -> Result<Vec<PlanItem>, PlanError> {
        let sheets: Vec<&Sheet> = self.sheets.iter().collect();
        plan_lengths(&sheets)
    }

    /// Structure de l'égalité de longueurs de `c` (tracés de référence, décalage, extension) ;
    /// `home` est la feuille de la courbe.
    pub(crate) fn check_length_link(&self, home: &Sheet, c: &CurveLength) -> Result<(), CoreError> {
        let name = home.entity_name(c.curve);
        let bad = |m: &str| CoreError::Invalid(format!("{name} (longueur imposée) : {m}"));
        let Some(r) = c.reference else {
            if c.link.is_some() {
                return Err(bad("une extension d'égalité sans tracé de référence"));
            }
            return Ok(());
        };
        if let Some(l) = &c.link {
            if l.sheet.is_none() && l.more.is_empty() && l.offset_expr.is_none() {
                return Err(bad("extension d'égalité vide (à omettre)"));
            }
            if l.sheet == Some(home.id) || l.more.iter().any(|t| t.sheet == Some(home.id)) {
                return Err(bad(
                    "la feuille de la courbe ne s'indique pas : omettez-la pour un tracé de la \
                     même feuille",
                ));
            }
            if l.offset_expr
                .as_deref()
                .is_some_and(|t| t.trim().is_empty())
            {
                return Err(bad("la formule du décalage est vide"));
            }
        }
        let mut seen = HashSet::new();
        for t in c.terms() {
            if t.entity == c.curve {
                return Err(bad(
                    "la longueur ne peut pas être égale à celle du même tracé (référence à \
                     soi-même)",
                ));
            }
            let sheet = self
                .term_sheet(home, t.sheet)
                .map_err(|_| bad("la feuille du tracé de référence n'existe pas dans ce projet"))?;
            if sheet.entity(t.entity).is_err() {
                return Err(if sheet.id == home.id {
                    bad("le tracé de référence n'existe pas sur cette feuille")
                } else {
                    bad(&format!(
                        "le tracé de référence n'existe pas sur la feuille « {} »",
                        sheet.name
                    ))
                });
            }
            if !seen.insert(t.entity) {
                return Err(bad(
                    "un même tracé ne peut figurer qu'une fois dans la somme",
                ));
            }
        }
        if !(r.offset_mm.is_finite() && r.offset_mm.abs() <= MAX_IMPOSED_LENGTH_MM) {
            return Err(bad(&format!(
                "le décalage doit être un nombre de mm d'au plus {} en valeur absolue",
                mm(MAX_IMPOSED_LENGTH_MM)
            )));
        }
        Ok(())
    }

    /// Valeur d'une formule de décalage : finie, au plus `MAX_IMPOSED_LENGTH_MM` en valeur absolue.
    fn length_offset_value(
        values: &HashMap<String, f64>,
        name: &str,
        text: &str,
    ) -> Result<f64, CoreError> {
        let v = crate::formula::eval_text(values, text).map_err(|e| {
            CoreError::Invalid(format!(
                "{name} : la formule « {text} » du décalage de la longueur imposée ne peut pas \
                 être calculée : {e}"
            ))
        })?;
        if !(v.is_finite() && v.abs() <= MAX_IMPOSED_LENGTH_MM) {
            return Err(CoreError::Invalid(format!(
                "{name} : la formule « {text} » donne un décalage de {} mm ; il doit être un \
                 nombre de mm d'au plus {} en valeur absolue",
                mm(v),
                mm(MAX_IMPOSED_LENGTH_MM)
            )));
        }
        Ok(v)
    }

    /// Cible d'une égalité de longueurs : somme des longueurs des tracés de référence + décalage
    /// (évalué si c'est une formule). Cible <= 0 ou trop grande, tracé non mesurable (dégénéré) =
    /// erreur expliquée.
    pub(crate) fn link_target(
        &self,
        home: &Sheet,
        c: &CurveLength,
    ) -> Result<LinkTarget, CoreError> {
        self.check_length_link(home, c)?;
        let Some(r) = c.reference else {
            return Err(CoreError::Invalid(format!(
                "{} : aucune égalité de longueurs",
                home.entity_name(c.curve)
            )));
        };
        let name = home.entity_name(c.curve);
        let mut names = Vec::new();
        let mut base = 0.0;
        for t in c.terms() {
            let sheet = self.term_sheet(home, t.sheet)?;
            let label = term_label(home, Some(sheet), t.entity);
            base += sheet.reference_length_mm(t.entity).map_err(|e| {
                CoreError::Invalid(format!(
                    "{name} : la longueur de {label} ne peut pas servir de référence : {e}"
                ))
            })?;
            names.push(label);
        }
        let text = names.join(" + ");
        let offset = match c.offset_expr() {
            Some(formula) => {
                Self::length_offset_value(&self.variable_value_map()?, &name, formula)?
            }
            None => r.offset_mm,
        };
        let target = base + offset;
        if !(target.is_finite() && target > 0.0 && target <= MAX_IMPOSED_LENGTH_MM) {
            return Err(CoreError::Invalid(format!(
                "{name} : la longueur imposée « égale à {text} {} mm » n'a pas de solution : \
                 {text} mesure {} mm et le décalage de {} mm donnerait {} mm ; la longueur \
                 imposée doit être comprise entre 0 mm (exclu) et {} mm, soit un décalage compris \
                 entre {} mm (exclu) et {} mm",
                signed_mm(offset),
                mm(base),
                signed_mm(offset),
                mm(target),
                mm(MAX_IMPOSED_LENGTH_MM),
                mm(-base),
                signed_mm(MAX_IMPOSED_LENGTH_MM - base),
            )));
        }
        let formula = c
            .offset_expr()
            .map_or(String::new(), |f| format!(" (décalage = formule « {f} »)"));
        let desc = format!("{text} = {} mm {} mm{formula}", mm(base), signed_mm(offset));
        Ok(LinkTarget {
            text,
            base,
            offset,
            target,
            desc,
        })
    }

    /// Contrainte d'égalité décrite par `eq` (forme canonique : la feuille de la courbe ne
    /// s'écrit pas, l'extension est omise tant que l'égalité est simple). La cible n'est pas
    /// calculée ici.
    fn equality_constraint(
        sheet: SheetId,
        curve: EntityId,
        adjust: LengthAdjust,
        length_mm: f64,
        eq: LengthEquality,
    ) -> Result<CurveLength, String> {
        let Some(first) = eq.terms.first().copied() else {
            return Err("l'égalité de longueurs demande au moins un tracé de référence".into());
        };
        let own = |s: Option<SheetId>| s.filter(|id| *id != sheet);
        let offset_expr = match eq.offset_expr.as_deref().map(str::trim) {
            Some("") => {
                return Err(
                    "formule du décalage vide (saisissez un nombre ou une expression)".into(),
                )
            }
            other => other.map(str::to_string),
        };
        let link = LengthLink {
            sheet: own(first.sheet),
            more: eq
                .terms
                .iter()
                .skip(1)
                .map(|t| LengthTerm {
                    entity: t.entity,
                    sheet: own(t.sheet),
                })
                .collect(),
            offset_expr,
        };
        let trivial = link.sheet.is_none() && link.more.is_empty() && link.offset_expr.is_none();
        Ok(CurveLength {
            curve,
            length_mm,
            adjust,
            reference: Some(LengthRef {
                entity: first.entity,
                offset_mm: eq.offset_mm,
            }),
            expr: None,
            link: (!trivial).then_some(link),
        })
    }

    /// Aperçu d'une égalité de longueurs **sans rien écrire** : `(somme des tracés, décalage évalué,
    /// cible)` en mm, ou l'erreur expliquée que la commande donnerait pour ce calcul.
    pub fn equality_preview(
        &self,
        sheet: SheetId,
        curve: EntityId,
        equality: &LengthEquality,
    ) -> Result<(f64, f64, f64), CoreError> {
        let home = self.sheet(sheet)?;
        let name = home.entity_name(curve);
        let c = Self::equality_constraint(
            sheet,
            curve,
            LengthAdjust::Handle { point: PointId(0) },
            1.0,
            equality.clone(),
        )
        .map_err(|e| CoreError::Invalid(format!("{name} : {e}")))?;
        let t = self.link_target(home, &c)?;
        Ok((t.base, t.offset, t.target))
    }

    /// `SetCurveLengthEquality` : fixe (`Some`) ou délie (`None`, la valeur courante est gardée) la
    /// cible d'une longueur imposée **existante** : somme de tracés (de toute feuille) + décalage
    /// (nombre ou formule). Remplace une formule de cible. La valeur calculée est écrite ; la
    /// résolution est faite par `sync_length`, même étape.
    pub(crate) fn set_curve_length_equality(
        &mut self,
        sheet: SheetId,
        curve: EntityId,
        equality: Option<LengthEquality>,
    ) -> Result<(), CoreError> {
        let home = self.sheet(sheet)?;
        home.entity(curve)?;
        let name = home.entity_name(curve);
        let Some(existing) = home.length_constraint(curve) else {
            return Err(CoreError::Invalid(format!(
                "{name} : aucune longueur imposée à lier à d'autres tracés (imposez d'abord la \
                 longueur de cette courbe)"
            )));
        };
        let Some(eq) = equality else {
            if existing.reference.is_some() {
                if let Some(c) = self
                    .sheet_mut(sheet)?
                    .length_constraints
                    .iter_mut()
                    .find(|c| c.curve == curve)
                {
                    c.reference = None;
                    c.link = None;
                }
            }
            return Ok(());
        };
        let mut c =
            Self::equality_constraint(sheet, curve, existing.adjust, existing.length_mm, eq)
                .map_err(|e| CoreError::Invalid(format!("{name} : {e}")))?;
        let t = self.link_target(home, &c)?;
        if let Some(r) = c.reference.as_mut() {
            r.offset_mm = t.offset;
        }
        c.length_mm = t.target;
        home.check_length_structure(self, &c)?;
        if let Some(o) = self
            .sheet_mut(sheet)?
            .length_constraints
            .iter_mut()
            .find(|o| o.curve == curve)
        {
            *o = c;
        }
        Ok(())
    }

    /// `SetCurveLengthConstraint` : pose / modifie (`Some`) ou retire (`None`) la longueur imposée
    /// d'une courbe. Retirer garde la géométrie courante. Poser enregistre la contrainte ; la
    /// résolution (écriture de l'inconnue) est faite par `sync_length` dans la même étape.
    pub(crate) fn set_curve_length_constraint(
        &mut self,
        sheet: SheetId,
        curve: EntityId,
        spec: Option<LengthSpec>,
    ) -> Result<(), CoreError> {
        let s = self.sheet(sheet)?;
        s.entity(curve)?;
        let Some(spec) = spec else {
            self.sheet_mut(sheet)?
                .length_constraints
                .retain(|c| c.curve != curve);
            return Ok(());
        };
        let mut c = CurveLength {
            curve,
            length_mm: spec.length_mm,
            adjust: spec.adjust,
            reference: spec.reference,
            // Une nouvelle demande remplace la contrainte : une formule n'y survit pas (elle se
            // relie par `SetCurveLengthExpr`), donc jamais une formule qui ment.
            expr: None,
            link: None,
        };
        if c.reference.is_some() {
            c.length_mm = self.link_target(s, &c)?.target;
        }
        s.check_length_structure(self, &c)?;
        if let LengthAdjust::Slide { point, .. } = c.adjust {
            if let Some(other) = s
                .length_constraints
                .iter()
                .find(|o| o.curve != curve && o.adjust.point() == point)
                .filter(|o| matches!(o.adjust, LengthAdjust::Slide { .. }))
            {
                return Err(CoreError::Invalid(format!(
                    "{} : le point {} est déjà déplacé par la longueur imposée de {}",
                    s.entity_name(curve),
                    s.label(point),
                    s.entity_name(other.curve)
                )));
            }
        }
        let s = self.sheet_mut(sheet)?;
        match s.length_constraints.iter_mut().find(|o| o.curve == curve) {
            Some(o) => *o = c,
            None => s.length_constraints.push(c),
        }
        Ok(())
    }

    /// Valeur d'une formule de longueur : expression évaluée sur les variables, strictement
    /// positive et au plus `MAX_IMPOSED_LENGTH_MM` (sinon erreur expliquée qui nomme la courbe).
    fn length_formula_value(
        values: &HashMap<String, f64>,
        name: &str,
        text: &str,
    ) -> Result<f64, CoreError> {
        let v = crate::formula::eval_text(values, text).map_err(|e| {
            CoreError::Invalid(format!(
                "{name} : la formule « {text} » de la longueur imposée ne peut pas être \
                 calculée : {e}"
            ))
        })?;
        if !(v.is_finite() && v > 0.0 && v <= MAX_IMPOSED_LENGTH_MM) {
            return Err(CoreError::Invalid(format!(
                "{name} : la formule « {text} » donne {} mm ; la longueur imposée doit être un \
                 nombre de mm strictement positif, au plus {}",
                mm(v),
                mm(MAX_IMPOSED_LENGTH_MM)
            )));
        }
        Ok(v)
    }

    /// `SetCurveLengthExpr` : lie (`Some`) la cible d'une longueur imposée **existante** à une
    /// formule sur les variables, ou la délie (`None`, la valeur courante est gardée). La valeur
    /// évaluée est écrite dans `length_mm` ; la résolution est faite par `sync_length`, même étape.
    /// Délier sans formule liée ne change rien (aucune étape).
    pub(crate) fn set_curve_length_expr(
        &mut self,
        sheet: SheetId,
        curve: EntityId,
        expr_text: Option<String>,
    ) -> Result<(), CoreError> {
        let s = self.sheet(sheet)?;
        s.entity(curve)?;
        let name = s.entity_name(curve);
        let Some(existing) = s.length_constraint(curve) else {
            return Err(CoreError::Invalid(format!(
                "{name} : aucune longueur imposée à lier à une formule (imposez d'abord la \
                 longueur de cette courbe)"
            )));
        };
        let Some(text) = expr_text else {
            if existing.expr.is_some() {
                if let Some(c) = self
                    .sheet_mut(sheet)?
                    .length_constraints
                    .iter_mut()
                    .find(|c| c.curve == curve)
                {
                    c.expr = None;
                }
            }
            return Ok(());
        };
        if existing.reference.is_some() {
            return Err(CoreError::Invalid(format!(
                "{name} : la longueur est déjà égale à celle d'un autre tracé ; retirez cette \
                 égalité avant de la lier à une formule (pour une formule sur le décalage de \
                 l'égalité, utilisez le décalage de l'égalité)"
            )));
        }
        let text = text.trim().to_string();
        if text.is_empty() {
            return Err(CoreError::Invalid(format!(
                "{name} : formule vide (saisissez un nombre ou une expression)"
            )));
        }
        let values = self.variable_value_map()?;
        let value = Self::length_formula_value(&values, &name, &text)?;
        if let Some(c) = self
            .sheet_mut(sheet)?
            .length_constraints
            .iter_mut()
            .find(|c| c.curve == curve)
        {
            c.length_mm = value;
            c.expr = Some(text);
        }
        Ok(())
    }

    /// Variables citées par les formules de longueur, cible ou décalage (désignations pour les
    /// messages) : tient `RemoveVariable` et le diagnostic au courant.
    pub fn length_formula_users(&self, name: &str) -> Vec<String> {
        let mut out = Vec::new();
        for s in &self.sheets {
            for c in &s.length_constraints {
                let cites = |t: &str| {
                    expr::parse(t)
                        .map(|e| e.references().iter().any(|r| r.name == name))
                        .unwrap_or(false)
                };
                if c.expr.as_deref().is_some_and(cites) {
                    out.push(format!("longueur imposée de {}", s.entity_name(c.curve)));
                }
                if c.offset_expr().is_some_and(cites) {
                    out.push(format!(
                        "décalage de la longueur imposée de {}",
                        s.entity_name(c.curve)
                    ));
                }
            }
        }
        out
    }

    /// Re-résout les longueurs imposées après une commande (même étape). `old` : projet d'avant.
    /// Retire avec remarque les contraintes qui n'ont plus de sens ; résout les autres dans l'ordre
    /// **global** des dépendances (toutes feuilles) ; impossible = erreur (la commande mère est
    /// refusée). Renvoie la désignation des courbes dont l'inconnue a été réécrite.
    pub(crate) fn sync_length(
        &mut self,
        old: &Project,
        out: &mut Outcome,
    ) -> Result<Vec<String>, CoreError> {
        if self.sheets.iter().all(|s| s.length_constraints.is_empty()) {
            return Ok(Vec::new());
        }
        let mut moved = Vec::new();
        let variables = self.variable_value_map();
        for si in 0..self.sheets.len() {
            if self.sheets[si].length_constraints.is_empty() {
                continue;
            }
            let old_sheet = old.sheets.iter().find(|s| s.id == self.sheets[si].id);
            let constraints = std::mem::take(&mut self.sheets[si].length_constraints);
            let mut live = Vec::with_capacity(constraints.len());
            let mut taken: HashSet<PointId> = HashSet::new();
            for c in constraints {
                let sheet = &self.sheets[si];
                let reason = sheet
                    .length_obsolete_reason(self, &c, Some(old))
                    .or_else(|| match c.adjust {
                        LengthAdjust::Slide { point, .. } if !taken.insert(point) => Some(
                            "son extrémité est déjà déplacée par une autre longueur imposée".into(),
                        ),
                        _ => None,
                    });
                if let Some(reason) = reason {
                    let name = old_sheet
                        .map_or_else(|| sheet.entity_name(c.curve), |o| o.entity_name(c.curve));
                    out.notes.push(format!(
                        "La longueur imposée de {name} est retirée : {reason} (la géométrie \
                         actuelle est conservée)."
                    ));
                } else {
                    live.push(c);
                }
            }
            self.sheets[si].length_constraints = live;
        }
        let plan = self.length_plan().map_err(|(e, _)| e)?;
        for (si, i) in plan {
            let mut c = self.sheets[si].length_constraints[i].clone();
            let mut desc = None;
            if c.reference.is_some() {
                // Les références sont déjà résolues (ordre du plan, toutes feuilles) : la cible
                // est relue d'elles.
                let t = self.link_target(&self.sheets[si], &c)?;
                c.length_mm = t.target;
                if c.offset_expr().is_some() {
                    if let Some(r) = c.reference.as_mut() {
                        r.offset_mm = t.offset;
                    }
                }
                desc = Some(t.desc);
                self.sheets[si].length_constraints[i] = c.clone();
            } else if let (Some(text), Ok(values)) = (&c.expr, &variables) {
                // Cible maintenue par une formule : réévaluée dans la même étape (table de
                // variables invalide : `validate` refuse ensuite).
                let name = self.sheets[si].entity_name(c.curve);
                c.length_mm = Self::length_formula_value(values, &name, text)?;
                self.sheets[si].length_constraints[i].length_mm = c.length_mm;
            }
            if self.sheets[si].solve_length(&c, desc.as_deref())? {
                moved.push(format!(
                    "longueur imposée de {}",
                    self.sheets[si].entity_name(c.curve)
                ));
            }
        }
        if !moved.is_empty() {
            // Un point déplacé ne doit pas être le parent d'une construction maintenue (ses
            // coordonnées écrites seraient périmées) : refus expliqué plutôt qu'un fichier invalide.
            self.validate_kept().map_err(|e| {
                CoreError::Invalid(format!(
                    "{e} (déplacé par {} : une construction maintenue dépend d'une extrémité \
                     ajustée par une longueur imposée)",
                    moved.join(", ")
                ))
            })?;
        }
        Ok(moved)
    }

    /// Validation à l'ouverture et après chaque commande : structure, unicité, ordre sans cycle,
    /// poignée non nulle et longueur réelle = cible à `LENGTH_TOLERANCE_MM`.
    pub(crate) fn validate_length_constraints(&self) -> Result<(), CoreError> {
        for sheet in &self.sheets {
            if let Some((_, e)) = sheet.length_problems(self).into_iter().next() {
                return Err(e);
            }
        }
        match self.length_formula_problems().into_iter().next() {
            Some((_, _, e)) => Err(e),
            None => Ok(()),
        }
    }

    /// Reprend dans `sheet` (copie importée) les formules de longueur qui s'évaluent dans **ce**
    /// projet avec la valeur écrite ; les autres sont déliées avec remarque (valeur gardée).
    pub(crate) fn retain_adoptable_length_formulas(&self, sheet: &mut Sheet, out: &mut Outcome) {
        let values = self.variable_value_map().unwrap_or_default();
        let mut dropped = 0;
        for c in &mut sheet.length_constraints {
            if let Some(text) = &c.expr {
                let ok = crate::formula::eval_text(&values, text)
                    .is_ok_and(|v| (v - c.length_mm).abs() <= crate::formula::FORMULA_TOLERANCE);
                if !ok {
                    c.expr = None;
                    dropped += 1;
                }
            }
            let offset = c.reference.map(|r| r.offset_mm);
            if let (Some(text), Some(offset)) = (c.offset_expr(), offset) {
                let ok = crate::formula::eval_text(&values, text)
                    .is_ok_and(|v| (v - offset).abs() <= crate::formula::FORMULA_TOLERANCE);
                if !ok {
                    c.drop_offset_expr();
                    dropped += 1;
                }
            }
        }
        if dropped > 0 {
            out.notes.push(format!(
                "{dropped} formule(s) de longueur imposée de la feuille « {} » ne sont pas \
                 reprises : leurs variables manquent ou ont une autre valeur dans ce projet (les \
                 longueurs sont conservées en nombres figés).",
                sheet.name
            ));
        }
    }

    /// Une feuille **importée d'un autre projet** n'a pas de feuille voisine ici : une égalité qui
    /// vise une autre feuille est déliée (la longueur reste un nombre imposé), avec remarque.
    pub(crate) fn drop_foreign_length_links(sheet: &mut Sheet, out: &mut Outcome) {
        let mut dropped = 0;
        for c in &mut sheet.length_constraints {
            if c.terms().iter().any(|t| t.sheet.is_some()) {
                c.reference = None;
                c.link = None;
                dropped += 1;
            }
        }
        if dropped > 0 {
            out.notes.push(format!(
                "{dropped} égalité(s) de longueurs de la feuille « {} » visent une autre feuille \
                 d'un autre projet : elles ne sont pas reprises (les longueurs sont conservées \
                 en nombres imposés).",
                sheet.name
            ));
        }
    }

    /// Problèmes des cibles par formule (feuille, courbe, erreur localisée) : formule non
    /// évaluable, valeur non conforme ou écart avec la valeur écrite (> 1e-9 mm). Vide si la table de
    /// variables est invalide (`validate` la refuse ailleurs). Couvre la formule de la cible et
    /// celle du décalage d'une égalité.
    pub(crate) fn length_formula_problems(&self) -> Vec<(SheetId, EntityId, CoreError)> {
        let mut out = Vec::new();
        if self.sheets.iter().all(|s| {
            s.length_constraints
                .iter()
                .all(|c| c.formula_texts().next().is_none())
        }) {
            return out;
        }
        let Ok(values) = self.variable_value_map() else {
            return out;
        };
        for sheet in &self.sheets {
            for c in &sheet.length_constraints {
                let name = format!(
                    "feuille « {} », longueur imposée, {}",
                    sheet.name,
                    sheet.entity_name(c.curve)
                );
                let mismatch = |what: &str, text: &str, written: f64, v: f64| {
                    if (v - written).abs() > crate::formula::FORMULA_TOLERANCE {
                        Err(CoreError::Invalid(format!(
                            "{name} : {what} ({} mm) ne correspond pas à la \
                             formule « {text} » ({} mm)",
                            format_number(written, 6),
                            format_number(v, 6)
                        )))
                    } else {
                        Ok(())
                    }
                };
                let mut problem = None;
                if let Some(text) = &c.expr {
                    problem = Self::length_formula_value(&values, &name, text)
                        .and_then(|v| mismatch("la longueur écrite", text, c.length_mm, v))
                        .err();
                }
                if let (None, Some(text), Some(r)) = (&problem, c.offset_expr(), c.reference) {
                    problem = Self::length_offset_value(&values, &name, text)
                        .and_then(|v| mismatch("le décalage écrit", text, r.offset_mm, v))
                        .err();
                }
                if let Some(e) = problem {
                    out.push((sheet.id, c.curve, e));
                }
            }
        }
        out
    }
}

impl CurveLength {
    /// Délie la formule du décalage (valeur écrite gardée) et rend l'extension canonique.
    pub(crate) fn drop_offset_expr(&mut self) {
        if let Some(l) = &mut self.link {
            l.offset_expr = None;
            if l.sheet.is_none() && l.more.is_empty() {
                self.link = None;
            }
        }
    }
}

impl Sheet {
    /// Tous les problèmes des longueurs imposées de la feuille (même contrôle que
    /// `validate_length_constraints`, qui garde le premier ; le diagnostic US-03.5 les garde
    /// tous), chacun avec la courbe en cause quand elle est attribuable (au plus un problème par
    /// contrainte ; le cycle n'est attribué à aucune). Erreurs déjà localisées. `proj` : le projet
    /// qui contient la feuille (feuilles des tracés de référence, ordre global).
    pub(crate) fn length_problems(&self, proj: &Project) -> Vec<(Option<EntityId>, CoreError)> {
        if self.length_constraints.is_empty() {
            return Vec::new();
        }
        let _index = crate::sheet_index::IndexGuard::new(self);
        let ctx = |e: CoreError| {
            CoreError::Invalid(format!("feuille « {} », longueur imposée : {e}", self.name))
        };
        let mut out = Vec::new();
        let mut curves = HashSet::new();
        let mut slid: HashMap<PointId, EntityId> = HashMap::new();
        for c in &self.length_constraints {
            if let Err(e) = self.length_constraint_problem(proj, c, &mut curves, &mut slid) {
                out.push((Some(c.curve), ctx(e)));
            }
        }
        if let Err((e, sheets)) = proj.length_plan() {
            if sheets.contains(&self.id) {
                out.push((None, ctx(e)));
            }
        }
        out
    }

    /// Premier problème d'une contrainte (structure, unicité, poignée, cible, longueur réelle).
    fn length_constraint_problem(
        &self,
        proj: &Project,
        c: &CurveLength,
        curves: &mut HashSet<EntityId>,
        slid: &mut HashMap<PointId, EntityId>,
    ) -> Result<(), CoreError> {
        let sheet = self;
        let name = sheet.entity_name(c.curve);
        if !curves.insert(c.curve) {
            return Err(CoreError::Invalid(format!(
                "{name} : deux longueurs imposées pour la même courbe"
            )));
        }
        sheet.check_length_structure(proj, c)?;
        if let LengthAdjust::Slide { point, .. } = c.adjust {
            if slid.insert(point, c.curve).is_some() {
                return Err(CoreError::Invalid(format!(
                    "{name} : le point {} est déplacé par deux longueurs imposées",
                    sheet.label(point)
                )));
            }
        }
        if let (
            LengthAdjust::Handle { point },
            Ok(EntityKind::Curve {
                start,
                handle_start,
                handle_end,
                ..
            }),
        ) = (c.adjust, sheet.entity(c.curve).map(|e| e.kind.clone()))
        {
            let h = if point == start {
                handle_start
            } else {
                handle_end
            };
            if h.vector().norm() < MIN_HANDLE_MM - 1e-9 {
                return Err(CoreError::Invalid(format!(
                    "{name} : la poignée du point {} est de longueur nulle",
                    sheet.label(point)
                )));
            }
        }
        if c.reference.is_some() {
            let t = proj.link_target(sheet, c)?;
            if (t.target - c.length_mm).abs() > LENGTH_TOLERANCE_MM {
                return Err(CoreError::Invalid(format!(
                    "{name} : la longueur imposée ({} mm) ne correspond pas à la longueur \
                     de {} ({} mm) {} mm",
                    format_number(c.length_mm, 6),
                    t.text,
                    format_number(t.base, 6),
                    signed_mm(t.offset)
                )));
            }
        }
        let len = sheet.curve_developed_mm(c.curve)?;
        if (len - c.length_mm).abs() > LENGTH_TOLERANCE_MM {
            return Err(CoreError::Invalid(format!(
                "{name} : la longueur écrite ({} mm) ne correspond pas à la longueur \
                 imposée ({} mm)",
                format_number(len, 6),
                format_number(c.length_mm, 6)
            )));
        }
        Ok(())
    }
}
