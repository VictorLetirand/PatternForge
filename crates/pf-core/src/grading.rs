//! US-09.2a — règle de gradation par point : écart cumulé par (point, taille) (décision 325).
//!
//! `Sheet.gradings` est une liste d'écarts [`PointGrading`] `{ point, size, dx_mm, dy_mm }` :
//! la position d'un **point libre** dans la taille `size` est sa position de base (celle qui est
//! écrite dans sa définition) plus l'écart, exprimé dans le repère de la feuille (axes X vers la
//! droite, Y vers le haut). L'écart est **cumulé** : il part de la taille de base, jamais de la
//! taille voisine. Aucune géométrie graduée n'est stockée (ADR-C) ; `Project::evaluate_sheet`
//! la calcule à la lecture.
//!
//! Conventions (à faire valider par une modéliste) :
//! - la taille de base de la gamme n'a **jamais** d'écart (0 implicite) ; une entrée `(0 ; 0)` n'est
//!   jamais stockée (retirer = poser zéro) ;
//! - une taille sans entrée a l'écart nul : la géométrie de base ;
//! - plusieurs gammes : un point peut recevoir des écarts pour les tailles de n'importe quelle
//!   gamme, l'évaluation prend **une** taille (une taille = une gamme, aucune combinaison
//!   stature × tour) ; la position de base est commune à toutes les gammes ;
//! - seuls les points **libres** se gradent ; un point construit (polaire, décalé, proportion…)
//!   est recalculé depuis ses parents gradués par le mécanisme ordinaire de la feuille ;
//! - les liens maintenus, formules et longueurs imposées ne sont **pas** réévalués par taille
//!   (US-09.5 / 09.7) : un écart sur un point suivi par un lien est refusé à la pose ; un point
//!   déjà gradué qui devient suivi est signalé par le diagnostic ;
//! - changer la base re-exprime les écarts (`écart'(s) = écart(s) − écart(nouvelle base)`) et
//!   déplace la position écrite des points concernés : toutes les positions évaluées restent
//!   identiques. Refusé quand cela ne peut pas rester vrai (plusieurs gammes, liens maintenus
//!   ou longueurs imposées sur la feuille) ;
//! - retirer une taille retire ses écarts dans la même étape, jamais ceux des autres tailles ;
//!   supprimer un point (ou le rendre non libre) retire ses écarts, avec remarque ;
//! - le « repère de gradation orientable » et les écarts par intervalle sont reportés ;
//! - US-09.3a, report : `CopyPointGrading` remplace **tous** les écarts des cibles par ceux de la
//!   source (modes identique / symétrie X / symétrie Y / inversion) ; `InterpolatePointGrading`
//!   donne à chaque point `écart = (1−t)·écart(A) + t·écart(B)` par taille (écart absent = 0),
//!   `t` = projection orthogonale de sa position de **base** sur A→B, **bornée à [0 ; 1]**
//!   (convention d'atelier à faire valider par une modéliste) ; valeurs sous 1e-9 mm ramenées à 0.
//!   La source / les extrémités ne sont que lues (un calque verrouillé ou un lien ne les gênent
//!   pas) ; chaque **cible** est refusée comme par `SetPointGrading`, y compris quand les écarts
//!   produits sont nuls.

use crate::command::{Command, Outcome};
use crate::error::CoreError;
use crate::model::*;
use pf_geometry::PointMm;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// Borne d'un écart (mm), en valeur absolue.
pub const MAX_GRADING_MM: f64 = 1_000_000.0;
/// Écart de recoupement des positions évaluées avant / après un changement de base.
pub const GRADING_TOLERANCE_MM: f64 = 1e-9;

/// Écart cumulé d'un point libre pour une taille, en mm, dans le repère de la feuille.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PointGrading {
    pub point: PointId,
    pub size: SizeId,
    pub dx_mm: f64,
    pub dy_mm: f64,
}

/// Ce qui suit un point et l'empêche d'être gradué proprement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GradingFollower {
    /// Point suivi par une construction maintenue (US-03.2a).
    Kept,
    /// Extrémité déplacée par une longueur imposée (US-03.3a).
    Length,
}

/// Comment les écarts de la source sont transformés quand une règle est reportée (US-09.3a).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GradingCopyMode {
    /// Écarts recopiés tels quels.
    #[default]
    Same,
    /// Symétrie X : `dx → −dx`, `dy` inchangé.
    MirrorX,
    /// Symétrie Y : `dy → −dy`, `dx` inchangé.
    MirrorY,
    /// Inversion : `(dx ; dy) → (−dx ; −dy)`.
    Invert,
}

impl GradingCopyMode {
    /// Les quatre modes, dans l'ordre d'affichage.
    pub const ALL: [GradingCopyMode; 4] = [
        GradingCopyMode::Same,
        GradingCopyMode::MirrorX,
        GradingCopyMode::MirrorY,
        GradingCopyMode::Invert,
    ];

    /// Écart transformé (un zéro reste `+0.0`, jamais `−0.0`).
    pub fn apply(self, dx: f64, dy: f64) -> (f64, f64) {
        let neg = |v: f64| if v == 0.0 { 0.0 } else { -v };
        match self {
            GradingCopyMode::Same => (dx, dy),
            GradingCopyMode::MirrorX => (neg(dx), dy),
            GradingCopyMode::MirrorY => (dx, neg(dy)),
            GradingCopyMode::Invert => (neg(dx), neg(dy)),
        }
    }
}

/// Refus d'une commande de gradation (affichable tel quel, localisable par l'interface).
#[derive(Debug, Clone, PartialEq)]
pub enum GradingError {
    UnknownSheet(SheetId),
    UnknownPoint(PointId),
    UnknownSize(SizeId),
    /// La taille de base n'a pas d'écart (0 implicite).
    BaseSize {
        label: String,
    },
    NotFinite,
    OutOfBounds,
    /// Point construit : sa position vient de ses parents.
    NotFree {
        label: String,
    },
    Locked {
        label: String,
    },
    Followed {
        label: String,
        by: GradingFollower,
    },
    /// La position graduée sortirait de l'espace de travail.
    OutOfWorkspace {
        label: String,
    },
    /// Aucun point cible (report ou interpolation, US-09.3a).
    NoTargets,
    /// Le même point deux fois dans la liste des cibles.
    DuplicateTarget {
        label: String,
    },
    /// La cible est la source (report), ou une extrémité A / B (interpolation).
    TargetIsSource {
        label: String,
    },
    /// Interpolation entre deux points de même position de base (ou le même point).
    DegenerateSegment {
        from: String,
        to: String,
    },
    NotAGradingCommand,
}

impl fmt::Display for GradingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GradingError::UnknownSheet(id) => write!(f, "feuille inconnue (id {})", id.0),
            GradingError::UnknownPoint(id) => write!(f, "point inconnu (id {})", id.0),
            GradingError::UnknownSize(id) => write!(f, "taille inconnue (id {})", id.0),
            GradingError::BaseSize { label } => write!(
                f,
                "la taille « {label} » est la taille de base : elle n'a pas d'écart (écart nul implicite)"
            ),
            GradingError::NotFinite => write!(f, "un écart doit être un nombre fini"),
            GradingError::OutOfBounds => {
                write!(f, "un écart est limité à ±{} mm", MAX_GRADING_MM as i64)
            }
            GradingError::NotFree { label } => write!(
                f,
                "le point {label} est construit : sa position vient de ses parents, graduez-les plutôt"
            ),
            GradingError::Locked { label } => {
                write!(f, "le point {label} est sur un calque verrouillé")
            }
            GradingError::Followed { label, by } => match by {
                GradingFollower::Kept => write!(
                    f,
                    "le point {label} est suivi par une construction maintenue : \
                     elle n'est pas réévaluée par taille, gradation refusée"
                ),
                GradingFollower::Length => write!(
                    f,
                    "le point {label} est ajusté par une longueur imposée : \
                     elle n'est pas réévaluée par taille, gradation refusée"
                ),
            },
            GradingError::OutOfWorkspace { label } => write!(
                f,
                "la position graduée du point {label} sortirait de l'espace de travail"
            ),
            GradingError::NoTargets => write!(f, "aucun point cible"),
            GradingError::DuplicateTarget { label } => {
                write!(f, "le point {label} est cité deux fois parmi les cibles")
            }
            GradingError::TargetIsSource { label } => write!(
                f,
                "le point {label} est la source (ou une extrémité) : il ne peut pas être aussi cible"
            ),
            GradingError::DegenerateSegment { from, to } => write!(
                f,
                "les points {from} et {to} ont la même position de base : interpolation impossible"
            ),
            GradingError::NotAGradingCommand => write!(f, "commande étrangère à la gradation"),
        }
    }
}

impl std::error::Error for GradingError {}

impl From<GradingError> for CoreError {
    fn from(e: GradingError) -> CoreError {
        CoreError::Invalid(e.to_string())
    }
}

impl Sheet {
    /// Écart d'un point pour une taille, s'il y en a un.
    pub fn grading(&self, point: PointId, size: SizeId) -> Option<&PointGrading> {
        self.gradings
            .iter()
            .find(|g| g.point == point && g.size == size)
    }

    /// Écarts d'un point, dans l'ordre de stockage.
    pub fn gradings_of(&self, point: PointId) -> impl Iterator<Item = &PointGrading> {
        self.gradings.iter().filter(move |g| g.point == point)
    }

    /// Nombre de points qui portent au moins un écart.
    pub fn graded_point_count(&self) -> usize {
        self.gradings
            .iter()
            .map(|g| g.point)
            .collect::<BTreeSet<_>>()
            .len()
    }

    /// Ce qui dicte la position de ce point (liens non réévalués par taille), s'il y en a un.
    pub fn grading_follower(&self, point: PointId) -> Option<GradingFollower> {
        if self.kept_segment_of_point(point).is_some() {
            Some(GradingFollower::Kept)
        } else if self.length_constraint_moving_point(point).is_some() {
            Some(GradingFollower::Length)
        } else {
            None
        }
    }

    /// Remet les écarts dans l'ordre canonique (point, taille) : l'état ne dépend pas de l'ordre
    /// des commandes.
    fn sort_gradings(&mut self) {
        self.gradings.sort_by_key(|g| (g.point, g.size));
    }

    /// Copie de la feuille dont les points libres sont déplacés de leur écart pour `size`. Les
    /// points construits suivent par le calcul ordinaire ; les poignées de courbes et les
    /// tangentes, relatives à leurs extrémités, suivent aussi.
    fn graded(&self, size: SizeId) -> Sheet {
        let mut s = self.clone();
        for g in self.gradings.iter().filter(|g| g.size == size) {
            if let Some(p) = s.points.iter_mut().find(|p| p.id == g.point) {
                if let PointDefinition::Free { x, y } = p.definition {
                    p.definition = PointDefinition::Free {
                        x: x + g.dx_mm,
                        y: y + g.dy_mm,
                    };
                }
            }
        }
        s
    }

    /// Ce qui ne se calcule pas dans cette feuille : points (et tracés) en erreur, un message par
    /// objet. Vide pour une géométrie saine.
    pub fn evaluation_issues(&self) -> Vec<String> {
        let _cache = crate::sheet_index::IndexGuard::new(self);
        let mut out = Vec::new();
        for p in &self.points {
            if let Err(e) = self.position(p.id) {
                out.push(format!("{} : {e}", self.label(p.id)));
            }
        }
        for e in &self.entities {
            if let Err(err) = self.shape(e.id) {
                out.push(format!("{} : {err}", self.entity_name(e.id)));
            }
        }
        out
    }
}

/// Coordonnées d'un point libre (position de base), sinon `None`.
fn free_at(sheet: &Sheet, point: PointId) -> Option<(f64, f64)> {
    match sheet.point(point).ok()?.definition {
        PointDefinition::Free { x, y } => Some((x, y)),
        _ => None,
    }
}

/// Écarts `(taille, dx, dy)` à écrire pour un point (jamais nuls).
type GradingEntries = Vec<(SizeId, f64, f64)>;

fn nonzero(dx: f64, dy: f64) -> bool {
    dx != 0.0 || dy != 0.0
}

impl Project {
    fn size_label(&self, size: SizeId) -> Option<String> {
        self.find_size(size).map(|(r, i)| r.sizes[i].label.clone())
    }

    /// Feuille telle qu'elle est dans la taille `size` (`None` = taille de base = géométrie
    /// écrite). Lecture seule ; une taille inconnue est refusée.
    pub fn graded_sheet(&self, sheet: SheetId, size: Option<SizeId>) -> Result<Sheet, CoreError> {
        if let Some(id) = size {
            if self.find_size(id).is_none() {
                return Err(crate::size::SizeError::UnknownSize(id).into());
            }
        }
        let s = self.sheet(sheet)?;
        Ok(match size {
            Some(id) if s.gradings.iter().any(|g| g.size == id) => s.graded(id),
            _ => s.clone(),
        })
    }

    /// Projet dont **toutes** les feuilles sont dans la taille `size` : sert à l'affichage en
    /// lecture seule d'une taille (jamais enregistré, jamais modifié par une commande).
    pub fn graded_project(&self, size: Option<SizeId>) -> Result<Project, CoreError> {
        if let Some(id) = size {
            if self.find_size(id).is_none() {
                return Err(crate::size::SizeError::UnknownSize(id).into());
            }
        }
        let mut p = self.clone();
        if let Some(id) = size {
            // Valeurs de variables de la taille d'abord (paramètres des formules liées, US-09.7),
            // puis écarts des points libres : l'ordre est décrit dans `size_values`.
            let mut graded = p.apply_size_variable_values(id)?;
            for s in &mut p.sheets {
                if s.gradings.iter().any(|g| g.size == id) {
                    *s = s.graded(id);
                    graded = true;
                }
            }
            if graded {
                p.scale_proportional_notches(self);
                p.resync_graded(self);
            }
            // US-10.3 : les retouches s'appliquent en dernier, aux seules valeurs de barème de la
            // copie lue (jamais à la géométrie).
            p.apply_alterations_in_read_copy(id);
        }
        Ok(p)
    }

    /// US-09.5a : ce que `Project::apply` recalcule quand une géométrie change (pièces dérivées,
    /// surpiqûres liées) est recalculé d'après la géométrie graduée, par les mêmes fonctions.
    /// Un échec (par exemple une dérivée dont les propres points sont gradués) laisse le projet
    /// gradué tel quel, sans régénération : jamais une erreur de lecture.
    fn resync_graded(&mut self, base: &Project) {
        let snapshot = self.clone();
        let mut out = Outcome::default();
        if self.sync_derived(base, &mut out).is_err() {
            *self = snapshot;
        }
        let snapshot = self.clone();
        if self.sync_topstitches(base, &mut out).is_err() {
            *self = snapshot;
        }
    }

    /// Écarts attachés à une taille (feuille, point, écart).
    pub(crate) fn gradings_of_size(&self, size: SizeId) -> Vec<(SheetId, PointGrading)> {
        self.sheets
            .iter()
            .flat_map(|s| s.gradings.iter().map(move |g| (s.id, *g)))
            .filter(|(_, g)| g.size == size)
            .collect()
    }

    /// Retire les écarts de ces tailles dans toutes les feuilles ; remarque s'il y en avait.
    pub(crate) fn drop_size_gradings(&mut self, sizes: &BTreeSet<SizeId>, out: &mut Outcome) {
        let mut removed = 0usize;
        for s in &mut self.sheets {
            let before = s.gradings.len();
            s.gradings.retain(|g| !sizes.contains(&g.size));
            removed += before - s.gradings.len();
        }
        if removed > 0 {
            out.notes.push(format!(
                "{removed} écart(s) de gradation retiré(s) avec la taille supprimée ; \
                 les écarts des autres tailles sont conservés."
            ));
        }
    }

    /// Retire les écarts dont le point a disparu ou n'est plus libre (dans la même étape que la
    /// commande), avec remarque.
    pub(crate) fn prune_dead_gradings(&mut self, out: &mut Outcome) {
        let mut removed = 0usize;
        for s in &mut self.sheets {
            if s.gradings.is_empty() {
                continue;
            }
            let before = s.gradings.len();
            let free: BTreeSet<PointId> = s
                .points
                .iter()
                .filter(|p| matches!(p.definition, PointDefinition::Free { .. }))
                .map(|p| p.id)
                .collect();
            s.gradings.retain(|g| free.contains(&g.point));
            removed += before - s.gradings.len();
        }
        if removed > 0 {
            out.notes.push(format!(
                "{removed} écart(s) de gradation retiré(s) : leur point a été supprimé ou n'est \
                 plus libre."
            ));
        }
    }

    /// Valide les écarts (ouverture et après chaque commande).
    pub(crate) fn validate_gradings(&self) -> Result<(), CoreError> {
        for sheet in &self.sheets {
            let mut seen = BTreeSet::new();
            for g in &sheet.gradings {
                let what = format!("feuille « {} », écart de gradation", sheet.name);
                let bad = |m: String| CoreError::Invalid(format!("{what} : {m}"));
                let Some(start) = free_at(sheet, g.point) else {
                    return Err(bad(if sheet.point(g.point).is_ok() {
                        format!(
                            "le point {} est construit : seul un point libre se gradue",
                            sheet.label(g.point)
                        )
                    } else {
                        format!("le point {} n'existe pas", g.point.0)
                    }));
                };
                let Some((range, i)) = self.find_size(g.size) else {
                    return Err(bad(format!("la taille {} n'existe pas", g.size.0)));
                };
                if range.base == g.size {
                    return Err(bad(format!(
                        "la taille « {} » est la taille de base : elle n'a pas d'écart",
                        range.sizes[i].label
                    )));
                }
                if !seen.insert((g.point, g.size)) {
                    return Err(bad(format!(
                        "deux écarts pour le point {} et la taille « {} »",
                        sheet.label(g.point),
                        range.sizes[i].label
                    )));
                }
                if !g.dx_mm.is_finite() || !g.dy_mm.is_finite() {
                    return Err(bad("écart non fini".into()));
                }
                if g.dx_mm.abs() > MAX_GRADING_MM || g.dy_mm.abs() > MAX_GRADING_MM {
                    return Err(bad(format!("écart hors de ±{} mm", MAX_GRADING_MM as i64)));
                }
                if !nonzero(g.dx_mm, g.dy_mm) {
                    return Err(bad(format!(
                        "écart nul stocké pour le point {} (un écart nul s'omet)",
                        sheet.label(g.point)
                    )));
                }
                if PointMm::new(start.0 + g.dx_mm, start.1 + g.dy_mm)
                    .validate()
                    .is_err()
                {
                    return Err(bad(format!(
                        "la position graduée du point {} sort de l'espace de travail",
                        sheet.label(g.point)
                    )));
                }
            }
        }
        Ok(())
    }

    /// Refus éventuel d'une commande de gradation, **typé** (l'interface le localise) ; ne
    /// modifie rien.
    pub fn check_grading_command(&self, command: &Command) -> Result<(), GradingError> {
        let mut draft = self.clone();
        draft.apply_grading_command(command.clone(), &mut Outcome::default())
    }

    pub(crate) fn apply_grading_command(
        &mut self,
        command: Command,
        _out: &mut Outcome,
    ) -> Result<(), GradingError> {
        match command {
            Command::SetPointGrading {
                sheet,
                point,
                size,
                dx_mm,
                dy_mm,
            } => self.set_point_grading(sheet, point, size, dx_mm, dy_mm),
            Command::ClearPointGrading { sheet, point } => self.clear_point_grading(sheet, point),
            Command::CopyPointGrading {
                sheet,
                source,
                targets,
                mode,
            } => self.copy_point_grading(sheet, source, &targets, mode),
            Command::InterpolatePointGrading {
                sheet,
                from,
                to,
                points,
            } => self.interpolate_point_grading(sheet, from, to, &points),
            _ => Err(GradingError::NotAGradingCommand),
        }
    }

    /// Point libre lu (source, extrémité) : inconnu ou construit = refus. Un calque verrouillé ou
    /// un lien ne gênent pas : le point n'est que **lu**.
    fn grading_reference(&self, sheet: SheetId, point: PointId) -> Result<(), GradingError> {
        let s = self
            .sheet(sheet)
            .map_err(|_| GradingError::UnknownSheet(sheet))?;
        s.point(point)
            .map_err(|_| GradingError::UnknownPoint(point))?;
        if free_at(s, point).is_none() {
            return Err(GradingError::NotFree {
                label: s.label(point).to_string(),
            });
        }
        Ok(())
    }

    /// Vérifie puis écrit les écarts de **remplacement** de chaque cible (tout ou rien) : les
    /// écarts existants de la cible disparaissent, `plan` donne les nouveaux (jamais nuls). Les
    /// cibles sont refusées comme par `SetPointGrading`, **même quand `plan` est vide** (plus
    /// strict : le report vers un point construit, verrouillé ou suivi n'a aucun sens).
    fn replace_gradings(
        &mut self,
        sheet: SheetId,
        reserved: &[PointId],
        plan: Vec<(PointId, GradingEntries)>,
    ) -> Result<(), GradingError> {
        let s = self
            .sheet(sheet)
            .map_err(|_| GradingError::UnknownSheet(sheet))?;
        let mut seen = BTreeSet::new();
        for (point, entries) in &plan {
            s.point(*point)
                .map_err(|_| GradingError::UnknownPoint(*point))?;
            let label = s.label(*point).to_string();
            if !seen.insert(*point) {
                return Err(GradingError::DuplicateTarget { label });
            }
            if reserved.contains(point) {
                return Err(GradingError::TargetIsSource { label });
            }
            let Some((x, y)) = free_at(s, *point) else {
                return Err(GradingError::NotFree { label });
            };
            if s.point_locked(*point) {
                return Err(GradingError::Locked { label });
            }
            if let Some(by) = s.grading_follower(*point) {
                return Err(GradingError::Followed { label, by });
            }
            for &(_, dx, dy) in entries {
                if !dx.is_finite() || !dy.is_finite() {
                    return Err(GradingError::NotFinite);
                }
                if dx.abs() > MAX_GRADING_MM || dy.abs() > MAX_GRADING_MM {
                    return Err(GradingError::OutOfBounds);
                }
                if PointMm::new(x + dx, y + dy).validate().is_err() {
                    return Err(GradingError::OutOfWorkspace { label });
                }
            }
        }
        let s = self
            .sheet_mut(sheet)
            .map_err(|_| GradingError::UnknownSheet(sheet))?;
        for (point, entries) in plan {
            s.gradings.retain(|g| g.point != point);
            for (size, dx, dy) in entries {
                s.gradings.push(PointGrading {
                    point,
                    size,
                    dx_mm: dx,
                    dy_mm: dy,
                });
            }
        }
        s.sort_gradings();
        Ok(())
    }

    fn copy_point_grading(
        &mut self,
        sheet: SheetId,
        source: PointId,
        targets: &[PointId],
        mode: GradingCopyMode,
    ) -> Result<(), GradingError> {
        self.grading_reference(sheet, source)?;
        if targets.is_empty() {
            return Err(GradingError::NoTargets);
        }
        let s = self
            .sheet(sheet)
            .map_err(|_| GradingError::UnknownSheet(sheet))?;
        let rule: Vec<(SizeId, f64, f64)> = s
            .gradings_of(source)
            .map(|g| {
                let (dx, dy) = mode.apply(g.dx_mm, g.dy_mm);
                (g.size, dx, dy)
            })
            .collect();
        let plan = targets.iter().map(|t| (*t, rule.clone())).collect();
        self.replace_gradings(sheet, &[source], plan)
    }

    fn interpolate_point_grading(
        &mut self,
        sheet: SheetId,
        from: PointId,
        to: PointId,
        points: &[PointId],
    ) -> Result<(), GradingError> {
        self.grading_reference(sheet, from)?;
        self.grading_reference(sheet, to)?;
        if points.is_empty() {
            return Err(GradingError::NoTargets);
        }
        let s = self
            .sheet(sheet)
            .map_err(|_| GradingError::UnknownSheet(sheet))?;
        let (ax, ay) = free_at(s, from).expect("extrémité libre");
        let (bx, by) = free_at(s, to).expect("extrémité libre");
        let (ux, uy) = (bx - ax, by - ay);
        let len2 = ux * ux + uy * uy;
        if from == to || len2.sqrt() < GRADING_TOLERANCE_MM {
            return Err(GradingError::DegenerateSegment {
                from: s.label(from).to_string(),
                to: s.label(to).to_string(),
            });
        }
        let value = |point: PointId, size: SizeId| {
            s.grading(point, size)
                .map_or((0.0, 0.0), |g| (g.dx_mm, g.dy_mm))
        };
        let sizes: BTreeSet<SizeId> = s
            .gradings_of(from)
            .chain(s.gradings_of(to))
            .map(|g| g.size)
            .collect();
        let mut plan = Vec::with_capacity(points.len());
        for &p in points {
            // Un point inconnu ou construit n'a pas de position de base : le laisser à la
            // vérification commune (refus typé).
            let Some((px, py)) = free_at(s, p) else {
                plan.push((p, Vec::new()));
                continue;
            };
            let t = (((px - ax) * ux + (py - ay) * uy) / len2).clamp(0.0, 1.0);
            let snap = |v: f64| {
                if v.abs() < GRADING_TOLERANCE_MM {
                    0.0
                } else {
                    v
                }
            };
            let entries = sizes
                .iter()
                .filter_map(|&size| {
                    let (a, b) = (value(from, size), value(to, size));
                    let dx = snap((1.0 - t) * a.0 + t * b.0);
                    let dy = snap((1.0 - t) * a.1 + t * b.1);
                    nonzero(dx, dy).then_some((size, dx, dy))
                })
                .collect();
            plan.push((p, entries));
        }
        self.replace_gradings(sheet, &[from, to], plan)
    }

    fn set_point_grading(
        &mut self,
        sheet: SheetId,
        point: PointId,
        size: SizeId,
        dx: f64,
        dy: f64,
    ) -> Result<(), GradingError> {
        let s = self
            .sheet(sheet)
            .map_err(|_| GradingError::UnknownSheet(sheet))?;
        s.point(point)
            .map_err(|_| GradingError::UnknownPoint(point))?;
        let (range, i) = self
            .find_size(size)
            .ok_or(GradingError::UnknownSize(size))?;
        if range.base == size {
            return Err(GradingError::BaseSize {
                label: range.sizes[i].label.clone(),
            });
        }
        if !dx.is_finite() || !dy.is_finite() {
            return Err(GradingError::NotFinite);
        }
        if dx.abs() > MAX_GRADING_MM || dy.abs() > MAX_GRADING_MM {
            return Err(GradingError::OutOfBounds);
        }
        let label = s.label(point).to_string();
        // Poser un écart nul retire l'entrée : toujours permis (c'est ainsi qu'un conflit se lève).
        if nonzero(dx, dy) {
            let Some((x, y)) = free_at(s, point) else {
                return Err(GradingError::NotFree { label });
            };
            if s.point_locked(point) {
                return Err(GradingError::Locked { label });
            }
            if let Some(by) = s.grading_follower(point) {
                return Err(GradingError::Followed { label, by });
            }
            if PointMm::new(x + dx, y + dy).validate().is_err() {
                return Err(GradingError::OutOfWorkspace { label });
            }
        }
        let s = self
            .sheet_mut(sheet)
            .map_err(|_| GradingError::UnknownSheet(sheet))?;
        s.gradings.retain(|g| !(g.point == point && g.size == size));
        if nonzero(dx, dy) {
            s.gradings.push(PointGrading {
                point,
                size,
                dx_mm: dx,
                dy_mm: dy,
            });
        }
        s.sort_gradings();
        Ok(())
    }

    fn clear_point_grading(&mut self, sheet: SheetId, point: PointId) -> Result<(), GradingError> {
        let s = self
            .sheet(sheet)
            .map_err(|_| GradingError::UnknownSheet(sheet))?;
        s.point(point)
            .map_err(|_| GradingError::UnknownPoint(point))?;
        let s = self
            .sheet_mut(sheet)
            .map_err(|_| GradingError::UnknownSheet(sheet))?;
        s.gradings.retain(|g| g.point != point);
        Ok(())
    }

    /// Re-exprime les écarts quand `new_base` devient la base de sa gamme (appelée avant que
    /// `base` ne change) : `écart'(s) = écart(s) − écart(nouvelle base)` pour chaque point qui
    /// porte un écart pour la nouvelle base, la position écrite devient la position évaluée de la
    /// nouvelle base. Toutes les positions évaluées restent identiques.
    pub(crate) fn rebase_gradings(
        &mut self,
        new_base: SizeId,
    ) -> Result<(), crate::size::SizeError> {
        use crate::size::SizeError;
        let (range, _) = self
            .find_size(new_base)
            .ok_or(SizeError::UnknownSize(new_base))?;
        let all_sizes: Vec<SizeId> = range.sizes.iter().map(|s| s.id).collect();
        let label = self.size_label(new_base).unwrap_or_default();
        let shifting: Vec<(usize, PointId)> = self
            .sheets
            .iter()
            .enumerate()
            .flat_map(|(si, s)| {
                s.gradings
                    .iter()
                    .filter(|g| g.size == new_base)
                    .map(move |g| (si, g.point))
            })
            .collect();
        if shifting.is_empty() {
            return Ok(());
        }
        if self.sizes.len() > 1 {
            return Err(SizeError::BaseShiftsOtherRanges { label });
        }
        for &(si, _) in &shifting {
            let s = &self.sheets[si];
            if !s.length_constraints.is_empty() || !s.kept_segment_ids().is_empty() {
                return Err(SizeError::BaseShiftsMaintained {
                    label,
                    sheet: s.name.clone(),
                });
            }
        }
        let shift: BTreeMap<(usize, PointId), (f64, f64)> = shifting
            .iter()
            .map(|&(si, p)| {
                let g = self.sheets[si]
                    .grading(p, new_base)
                    .copied()
                    .expect("écart de la nouvelle base");
                ((si, p), (g.dx_mm, g.dy_mm))
            })
            .collect();
        for (&(si, point), &(sx, sy)) in &shift {
            let s = &mut self.sheets[si];
            let old: BTreeMap<SizeId, (f64, f64)> = s
                .gradings_of(point)
                .map(|g| (g.size, (g.dx_mm, g.dy_mm)))
                .collect();
            s.gradings.retain(|g| g.point != point);
            if let Some(p) = s.points.iter_mut().find(|p| p.id == point) {
                if let PointDefinition::Free { x, y } = p.definition {
                    p.definition = PointDefinition::Free {
                        x: x + sx,
                        y: y + sy,
                    };
                }
            }
            for &size in &all_sizes {
                if size == new_base {
                    continue;
                }
                let (ex, ey) = old.get(&size).copied().unwrap_or((0.0, 0.0));
                let (nx, ny) = (ex - sx, ey - sy);
                if nonzero(nx, ny) {
                    s.gradings.push(PointGrading {
                        point,
                        size,
                        dx_mm: nx,
                        dy_mm: ny,
                    });
                }
            }
            s.sort_gradings();
        }
        Ok(())
    }
}
