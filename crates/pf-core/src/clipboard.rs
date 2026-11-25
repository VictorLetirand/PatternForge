//! Presse-papiers (US-01.6) : un [`Fragment`] est une copie autonome d'objets
//! d'une feuille, sérialisable en JSON versionné (collage entre feuilles,
//! projets et instances). Les identifiants du fragment ne servent qu'à relier
//! ses objets entre eux : ils sont tous régénérés au collage.
//!
//! Règles de copie : les constructions internes à la sélection sont conservées
//! (points construits dont le tracé parent est copié, crans, cotations dont les
//! deux points sont copiés, groupes entièrement copiés) ; un parent externe est
//! figé (le point devient libre, avec remarque). Le calque n'est pas copié :
//! le collage range les objets dans le calque demandé.

use crate::command::Outcome;
use crate::error::CoreError;
use crate::model::*;
use crate::sheet_copy::{rewrite_content, unique_name, IdMap};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashSet};

/// Marqueur de format du presse-papiers.
pub const FRAGMENT_FORMAT: &str = "patternforge-clipboard";
/// Version du format du fragment.
pub const FRAGMENT_VERSION: u32 = 1;
/// Décalage (mm, sur x et y) appliqué à chaque collage qui tomberait sur des
/// points existants de la feuille.
pub const PASTE_OFFSET_MM: f64 = 5.0;
/// Tolérance de coïncidence de deux points (mm).
const SAME_SPOT_MM: f64 = 1e-6;

/// Copie autonome d'objets d'une feuille.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fragment {
    pub format: String,
    pub version: u32,
    pub points: Vec<Point>,
    pub entities: Vec<Entity>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notches: Vec<Notch>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dimensions: Vec<Dimension>,
    /// Cotes complètes (US-06.7) dont toutes les cibles sont copiées.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub full_dimensions: Vec<crate::full_dimension::FullDimension>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub groups: Vec<Group>,
    /// Lignes d'aide copiées (US-02.8) ; leur calque n'est pas copié.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub guides: Vec<Guide>,
    /// Remarques de la copie (parents figés…), affichées au collage.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

impl Fragment {
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("un fragment est toujours sérialisable")
    }

    /// Vrai si le texte ressemble à un fragment (le texte quelconque du
    /// presse-papiers système est alors ignoré sans bruit).
    pub fn looks_like(text: &str) -> bool {
        let t = text.trim_start();
        t.starts_with('{') && t.contains(FRAGMENT_FORMAT)
    }

    /// Lit et valide un fragment ; toute incohérence est refusée.
    pub fn from_json(text: &str) -> Result<Fragment, CoreError> {
        crate::persist::reject_duplicate_keys(text)
            .map_err(|e| CoreError::Invalid(format!("presse-papiers illisible : {e}")))?;
        let fragment: Fragment = serde_json::from_str(text)
            .map_err(|e| CoreError::Invalid(format!("presse-papiers illisible : {e}")))?;
        fragment.validate()?;
        Ok(fragment)
    }

    /// Vérifie le format, l'unicité des identifiants et toutes les références.
    pub fn validate(&self) -> Result<(), CoreError> {
        let bad = |msg: String| CoreError::Invalid(format!("presse-papiers invalide : {msg}"));
        if self.format != FRAGMENT_FORMAT {
            return Err(bad("ce n'est pas un fragment PatternForge".into()));
        }
        if self.version != FRAGMENT_VERSION {
            return Err(bad(format!(
                "version {} non prise en charge (attendue : {FRAGMENT_VERSION})",
                self.version
            )));
        }
        if self.points.is_empty() && self.entities.is_empty() && self.guides.is_empty() {
            return Err(bad("aucun objet".into()));
        }
        let mut seen = HashSet::new();
        let ids = self
            .points
            .iter()
            .map(|p| p.id.0)
            .chain(self.entities.iter().map(|e| e.id.0))
            .chain(self.notches.iter().map(|n| n.id.0))
            .chain(self.dimensions.iter().map(|d| d.id.0))
            .chain(self.full_dimensions.iter().map(|d| d.id.0))
            .chain(self.groups.iter().map(|g| g.id.0))
            .chain(self.guides.iter().map(|g| g.id.0));
        for id in ids {
            if !seen.insert(id) {
                return Err(CoreError::DuplicateId(id));
            }
        }
        let points: HashSet<PointId> = self.points.iter().map(|p| p.id).collect();
        let entities: HashSet<EntityId> = self.entities.iter().map(|e| e.id).collect();
        let dead = |what: &str, id: u64| {
            CoreError::DeadReference(format!("{what} référence un objet absent (id {id})"))
        };
        for p in &self.points {
            if p.layer.is_some() {
                return Err(bad("un point porte un calque".into()));
            }
            for d in p.definition.dependencies() {
                if !entities.contains(&d) {
                    return Err(dead("un point construit", d.0));
                }
            }
            for d in p.definition.point_dependencies() {
                if !points.contains(&d) {
                    return Err(dead("un point construit", d.0));
                }
            }
            if let Some(g) = p.definition.axis_guide() {
                if !self.guides.iter().any(|o| o.id == g) {
                    return Err(dead("un point miroir", g.0));
                }
            }
        }
        for e in &self.entities {
            if e.layer.is_some() {
                return Err(bad("un tracé porte un calque".into()));
            }
            for p in e.kind.all_points() {
                if !points.contains(&p) {
                    return Err(dead("un tracé", p.0));
                }
            }
            if let EntityKind::Segment {
                construction:
                    Some(Construction::Perpendicular { reference, .. })
                    | Some(Construction::Parallel { reference, .. }),
                ..
            } = e.kind
            {
                if !entities.contains(&reference) {
                    return Err(dead("une construction", reference.0));
                }
            }
        }
        for g in &self.guides {
            g.validate()?;
            if g.layer.is_some() {
                return Err(bad("une ligne d'aide porte un calque".into()));
            }
        }
        for n in &self.notches {
            if !entities.contains(&n.entity) {
                return Err(dead("un cran", n.entity.0));
            }
            if let Some(p) = n.anchor.and_then(|a| a.point()) {
                if !points.contains(&p) {
                    return Err(dead("un cran ancré", p.0));
                }
            }
            if let Some(NotchAnchor::Carried { entity, .. }) = n.anchor {
                if !entities.contains(&entity) {
                    return Err(dead("un cran ancré", entity.0));
                }
            }
        }
        for d in &self.dimensions {
            for p in [d.from, d.to] {
                if !points.contains(&p) {
                    return Err(dead("une cotation", p.0));
                }
            }
        }
        for d in &self.full_dimensions {
            d.kind.validate_static()?;
            for p in d.kind.points() {
                if !points.contains(&p) {
                    return Err(dead("une cote", p.0));
                }
            }
            if let Some(e) = d.kind.entity() {
                if !entities.contains(&e) {
                    return Err(dead("une cote", e.0));
                }
            }
            crate::full_dimension::normalize_label(d.label.as_deref())?;
            crate::full_dimension::check_text_shift(d.text_shift_mm)?;
        }
        let mut grouped = HashSet::new();
        for g in &self.groups {
            for m in &g.members {
                if !entities.contains(m) {
                    return Err(dead("un groupe", m.0));
                }
                if !grouped.insert(*m) {
                    return Err(bad(format!("le tracé {} est dans deux groupes", m.0)));
                }
            }
        }
        Ok(())
    }

    /// Les objets du fragment, présentés comme une feuille (pour réutiliser
    /// la réécriture d'identifiants des copies de feuille).
    pub(crate) fn as_sheet(&self) -> Sheet {
        Sheet {
            id: SheetId(0),
            name: String::new(),
            points: self.points.clone(),
            entities: self.entities.clone(),
            notches: self.notches.clone(),
            dimensions: self.dimensions.clone(),
            full_dimensions: self.full_dimensions.clone(),
            layers: Vec::new(),
            groups: self.groups.clone(),
            guides: self.guides.clone(),
            topstitches: Vec::new(),
            formulas: Vec::new(),
            length_constraints: Vec::new(),
            gradings: Vec::new(),
            background: None,
            extra: BTreeMap::new(),
        }
    }
}

impl Project {
    /// Copie les objets sélectionnés d'une feuille dans un fragment ; ce
    /// projet n'est pas modifié. Les extrémités des tracés copiés sont
    /// incluses ; les crans, cotations et groupes internes à la copie aussi.
    pub fn copy_fragment(
        &self,
        sheet: SheetId,
        points: &[PointId],
        entities: &[EntityId],
        notches: &[NotchId],
        dimensions: &[DimensionId],
    ) -> Result<Fragment, CoreError> {
        self.copy_fragment_with_guides(sheet, points, entities, notches, dimensions, &[])
    }

    /// Comme [`Project::copy_fragment`], avec des lignes d'aide en plus.
    pub fn copy_fragment_with_guides(
        &self,
        sheet: SheetId,
        points: &[PointId],
        entities: &[EntityId],
        notches: &[NotchId],
        dimensions: &[DimensionId],
        guides: &[GuideId],
    ) -> Result<Fragment, CoreError> {
        let s = self.sheet(sheet)?;
        let mut out_guides: Vec<Guide> = Vec::new();
        for &g in guides {
            let mut ng = s.guide(g)?.clone();
            ng.layer = None;
            if !out_guides.iter().any(|o| o.id == g) {
                out_guides.push(ng);
            }
        }
        let mut ent_set: BTreeSet<EntityId> = BTreeSet::new();
        for &e in entities {
            s.entity(e)?;
            ent_set.insert(e);
        }
        let mut pt_set: BTreeSet<PointId> = BTreeSet::new();
        for &p in points {
            s.point(p)?;
            pt_set.insert(p);
        }
        for &e in &ent_set {
            pt_set.extend(s.entity_all_points(e)?);
        }
        let mut notes = Vec::new();

        let mut out_points = Vec::new();
        for p in s.points.iter().filter(|p| pt_set.contains(&p.id)) {
            let mut np = p.clone();
            np.layer = None;
            if !p
                .definition
                .dependencies()
                .iter()
                .all(|d| ent_set.contains(d))
                || !p
                    .definition
                    .point_dependencies()
                    .iter()
                    .all(|d| pt_set.contains(d))
                || p.definition
                    .axis_guide()
                    .is_some_and(|g| !out_guides.iter().any(|o| o.id == g))
            {
                let pos = s.position(p.id)?;
                np.definition = PointDefinition::Free { x: pos.x, y: pos.y };
                notes.push(format!(
                    "Le point {} dépendait d'un tracé non copié : il est libre dans la copie.",
                    p.label
                ));
            }
            out_points.push(np);
        }

        // Formules maintenues (US-03.1b) : non reprises, la valeur est copiée en nombre figé.
        let frozen = s
            .formulas
            .iter()
            .filter(|f| pt_set.contains(&f.point))
            .count();
        if frozen > 0 {
            notes.push(format!(
                "{frozen} paramètre(s) maintenu(s) par une formule sont copiés avec leur valeur \
                 figée : la formule n'est pas reprise."
            ));
        }

        // Longueurs imposées (US-03.3) : non reprises, la géométrie est copiée telle quelle.
        let imposed = s
            .length_constraints
            .iter()
            .filter(|c| ent_set.contains(&c.curve))
            .count();
        if imposed > 0 {
            notes.push(format!(
                "{imposed} courbe(s) à longueur imposée sont copiées sans la contrainte : la                  géométrie est conservée."
            ));
        }

        // Écarts de gradation (US-09.2a) : non repris, la position de base est copiée.
        let graded = s
            .gradings
            .iter()
            .filter(|g| pt_set.contains(&g.point))
            .map(|g| g.point)
            .collect::<std::collections::BTreeSet<_>>()
            .len();
        if graded > 0 {
            notes.push(format!(
                "{graded} point(s) gradué(s) sont copiés à leur position de base : les écarts de \
                 gradation ne sont pas repris."
            ));
        }

        let mut lost_provenance = 0;
        let mut unkept = 0;
        let mut out_entities = Vec::new();
        for e in s.entities.iter().filter(|e| ent_set.contains(&e.id)) {
            let mut ne = e.clone();
            ne.layer = None;
            // Constructions maintenues (US-03.2) : la copie n'est pas maintenue.
            if unkeep(&mut ne) {
                unkept += 1;
            }
            if let EntityKind::Segment {
                construction:
                    Some(Construction::Perpendicular { reference, .. })
                    | Some(Construction::Parallel { reference, .. }),
                ..
            } = e.kind
            {
                if !ent_set.contains(&reference) {
                    if let EntityKind::Segment { construction, .. } = &mut ne.kind {
                        *construction = None;
                    }
                    lost_provenance += 1;
                }
            }
            out_entities.push(ne);
        }
        if unkept > 0 {
            notes.push(format!(
                "{unkept} construction(s) maintenue(s) sont copiées sans être maintenues : la \
                 copie garde les positions."
            ));
        }
        if lost_provenance > 0 {
            notes.push(format!(
                "{lost_provenance} segment(s) perdent la mémoire de leur construction \
                 (perpendiculaire ou parallèle à un tracé non copié)."
            ));
        }

        // Un cran ancré à un point ou à un tracé qui n'est pas copié devient, dans la
        // copie, un cran à distance fixe (US-06.2) ; les autres gardent leur ancrage.
        let mut out_notches: Vec<Notch> = Vec::new();
        let mut fixed_anchors = 0;
        for n in s.notches.iter().filter(|n| ent_set.contains(&n.entity)) {
            let self_contained = match n.anchor {
                Some(NotchAnchor::Point { point }) => pt_set.contains(&point),
                Some(NotchAnchor::Carried { entity, point, .. }) => {
                    pt_set.contains(&point) && ent_set.contains(&entity)
                }
                _ => true,
            };
            if self_contained {
                out_notches.push(n.clone());
            } else {
                out_notches.push(s.notch_fixed(n)?);
                fixed_anchors += 1;
            }
        }
        if fixed_anchors > 0 {
            notes.push(format!(
                "{fixed_anchors} cran(s) ancré(s) à un objet non copié sont copiés à distance \
                 fixe."
            ));
        }
        // Un cran en coin dépend du contour d'une pièce, que le presse-papiers ne copie pas.
        let corners = out_notches.iter().filter(|n| n.corner).count();
        if corners > 0 {
            for n in out_notches.iter_mut() {
                n.corner = false;
            }
            notes.push(format!(
                "{corners} cran(s) en coin sont copiés comme crans ordinaires : la pièce dont ils \
                 suivent le sommet n'est pas copiée."
            ));
        }
        let out_dimensions: Vec<Dimension> = s
            .dimensions
            .iter()
            .filter(|d| pt_set.contains(&d.from) && pt_set.contains(&d.to))
            .cloned()
            .collect();
        // Une cote complète est copiée avec ses cibles ; si une cible seulement est copiée,
        // la cote ne l'est pas (remarque, pas de perte silencieuse).
        let covered = |d: &crate::full_dimension::FullDimension| {
            d.kind.points().iter().all(|p| pt_set.contains(p))
                && d.kind.entity().is_none_or(|e| ent_set.contains(&e))
        };
        let touched = |d: &crate::full_dimension::FullDimension| {
            d.kind.points().iter().any(|p| pt_set.contains(p))
                || d.kind.entity().is_some_and(|e| ent_set.contains(&e))
        };
        let out_full_dimensions: Vec<crate::full_dimension::FullDimension> = s
            .full_dimensions
            .iter()
            .filter(|d| covered(d))
            .cloned()
            .collect();
        let skipped_full = s
            .full_dimensions
            .iter()
            .filter(|d| touched(d) && !covered(d))
            .count();
        if skipped_full > 0 {
            notes.push(format!(
                "{skipped_full} cote(s) non copiée(s) : un des objets qu'elles mesurent ne l'est                  pas."
            ));
        }
        let out_groups: Vec<Group> = s
            .groups
            .iter()
            .filter(|g| g.members.iter().all(|m| ent_set.contains(m)))
            .cloned()
            .collect();

        let ignored_notches = notches
            .iter()
            .filter(|n| !out_notches.iter().any(|o| o.id == **n))
            .count();
        let ignored_dims = dimensions
            .iter()
            .filter(|d| !out_dimensions.iter().any(|o| o.id == **d))
            .count();
        if ignored_notches > 0 {
            notes.push(format!(
                "{ignored_notches} cran(s) sélectionné(s) non copié(s) : leur tracé ne l'est pas."
            ));
        }
        if ignored_dims > 0 {
            notes.push(format!(
                "{ignored_dims} cotation(s) sélectionnée(s) non copiée(s) : un de leurs points ne l'est pas."
            ));
        }

        let fragment = Fragment {
            format: FRAGMENT_FORMAT.to_string(),
            version: FRAGMENT_VERSION,
            points: out_points,
            entities: out_entities,
            notches: out_notches,
            dimensions: out_dimensions,
            full_dimensions: out_full_dimensions,
            groups: out_groups,
            guides: out_guides,
            notes,
        };
        if fragment.points.is_empty() && fragment.entities.is_empty() && fragment.guides.is_empty()
        {
            return Err(CoreError::Invalid(
                "rien à copier : sélectionnez des points ou des tracés".into(),
            ));
        }
        fragment.validate()?;
        Ok(fragment)
    }

    /// Colle un fragment dans une feuille : identifiants régénérés, objets
    /// rangés dans `layer`, décalés de `PASTE_OFFSET_MM` autant de fois qu'il
    /// faut pour ne pas tomber sur des points existants.
    pub(crate) fn paste_fragment(
        &mut self,
        sheet: SheetId,
        fragment: &Fragment,
        layer: Option<LayerId>,
        out: &mut Outcome,
    ) -> Result<(), CoreError> {
        fragment.validate()?;
        let steps = {
            let target = self.sheet(sheet)?;
            if let Some(l) = layer {
                target.layer(l)?;
            }
            if target.layer_locked(layer) {
                return Err(CoreError::LayerLocked(format!(
                    "coller : le calque « {} » est verrouillé (déverrouillez-le ou choisissez \
                     un autre calque actif)",
                    target.layer_name(layer)
                )));
            }
            paste_steps(target, fragment)
        };
        let shift = steps as f64 * PASTE_OFFSET_MM;

        let shell = fragment.as_sheet();
        let mut map = IdMap(BTreeMap::new());
        self.fresh_ids(&shell, &mut map);
        let content = rewrite_content(&map, &shell)?;

        let target = self.sheet_mut(sheet)?;
        let mut names: HashSet<String> = target
            .entities
            .iter()
            .filter_map(|e| e.name.clone())
            .collect();
        for mut p in content.points {
            p.layer = layer;
            if let PointDefinition::Free { x, y } = p.definition {
                p.definition = PointDefinition::Free {
                    x: x + shift,
                    y: y + shift,
                };
            }
            if target.points.iter().any(|q| q.label == p.label) {
                p.label = target.next_label();
            }
            out.points.push(p.id);
            target.points.push(p);
        }
        let mut unkept = 0;
        for mut e in content.entities {
            e.layer = layer;
            if unkeep(&mut e) {
                unkept += 1;
            }
            if let Some(n) = e.name.take() {
                let unique = unique_name(&n, &names);
                names.insert(unique.clone());
                e.name = Some(unique);
            }
            out.entities.push(e.id);
            target.entities.push(e);
        }
        out.notches = content.notches.iter().map(|n| n.id).collect();
        target.notches.extend(content.notches);
        out.dimensions = content.dimensions.iter().map(|d| d.id).collect();
        target.dimensions.extend(content.dimensions);
        out.full_dimensions = content.full_dimensions.iter().map(|d| d.id).collect();
        target.full_dimensions.extend(content.full_dimensions);
        out.groups = content.groups.iter().map(|g| g.id).collect();
        target.groups.extend(content.groups);
        for mut g in content.guides {
            g.layer = layer;
            g.x_mm += shift;
            g.y_mm += shift;
            out.guides.push(g.id);
            target.guides.push(g);
        }
        out.sheet = Some(sheet);
        out.notes.extend(fragment.notes.iter().cloned());
        if unkept > 0 {
            out.notes.push(format!(
                "{unkept} construction(s) maintenue(s) du fragment sont collées sans être \
                 maintenues."
            ));
        }
        Ok(())
    }
}

/// Retire l'état « maintenu » d'un segment (US-03.2) ; vrai s'il l'était.
fn unkeep(e: &mut Entity) -> bool {
    match &mut e.kind {
        EntityKind::Segment {
            construction: Some(c),
            ..
        } if c.is_kept() => {
            *c = c.with_kept(false);
            true
        }
        _ => false,
    }
}

/// Nombre de décalages nécessaires pour que aucun point libre du fragment ne
/// tombe sur un point existant de la feuille (0 = collage à l'identique).
fn paste_steps(sheet: &Sheet, fragment: &Fragment) -> u32 {
    let existing: Vec<(f64, f64)> = sheet
        .points
        .iter()
        .filter_map(|p| sheet.position(p.id).ok())
        .map(|p| (p.x, p.y))
        .collect();
    let free: Vec<(f64, f64)> = fragment
        .points
        .iter()
        .filter_map(|p| match p.definition {
            PointDefinition::Free { x, y } => Some((x, y)),
            _ => None,
        })
        .collect();
    let collides = |k: u32| {
        let d = f64::from(k) * PASTE_OFFSET_MM;
        free.iter().any(|(x, y)| {
            existing.iter().any(|(ex, ey)| {
                (ex - (x + d)).abs() <= SAME_SPOT_MM && (ey - (y + d)).abs() <= SAME_SPOT_MM
            })
        })
    };
    (0..10_000).find(|&k| !collides(k)).unwrap_or(0)
}
