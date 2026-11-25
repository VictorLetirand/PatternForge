//! Import de géométrie de construction (US-12.3) : `Command::ImportGeometry` ajoute à une
//! feuille des points, segments, cercles, arcs et courbes par points de passage, **en une seule
//! étape annulable**. Aucun format de fichier n'est connu ici (le lecteur DXF vit dans
//! `pf-export`) : la commande reçoit des objets en mm, ordinaires, sans pièce ni contrainte.
//!
//! Règles :
//!
//! - tout objet est de la géométrie de construction ordinaire (rôle `Construction`, points
//!   libres) ; les points confondus (à `MERGE_TOLERANCE_MM` près) entre eux ou avec un point déjà
//!   présent sont **fusionnés**, comme pour toutes les commandes de dessin ;
//! - un calque est donné par son nom : il est créé s'il n'existe pas, réutilisé sinon (refus si
//!   un calque réutilisé est verrouillé : c'est `Project::apply`) ; `None` = calque par défaut ;
//! - un coordonnée non finie ou hors de l'espace de travail, une liste vide ou trop longue, un
//!   nom de calque invalide : **refus** de toute la commande (rien n'est modifié) ;
//! - un objet qui devient impossible une fois ses points fusionnés (segment nul, arc dont les
//!   extrémités ne sont plus cohérentes, courbe dont un point revient) ou déjà présent (segment
//!   identique) n'est pas créé et **compté dans les remarques** de l'issue : jamais en silence.

use crate::command::{Outcome, MERGE_TOLERANCE_MM};
use crate::error::CoreError;
use crate::model::*;
use pf_geometry::PointMm;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Nombre maximal d'objets d'un import. Borne le coût de l'import et de la validation du projet
/// (indexée : environ linéaire) : au-delà, l'import est refusé avec un message, jamais tronqué.
pub const MAX_IMPORT_OBJECTS: usize = 200_000;
/// Nombre maximal de calques créés par un import.
pub const MAX_IMPORT_LAYERS: usize = 500;

/// Un objet à importer, en mm.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ImportItem {
    /// Un point libre.
    Point {
        at: PointMm,
    },
    Segment {
        from: PointMm,
        to: PointMm,
    },
    /// Cercle entier (point de départ à 0°, sens anti-horaire).
    Circle {
        center: PointMm,
        radius_mm: f64,
    },
    /// Arc de cercle de `start` à `end` autour de `center` ; `ccw` = sens anti-horaire.
    Arc {
        center: PointMm,
        start: PointMm,
        end: PointMm,
        ccw: bool,
    },
    /// Courbe lisse (Catmull-Rom) par ces points de passage.
    Spline {
        points: Vec<PointMm>,
    },
    /// Bézier cubique : extrémités et deux points de contrôle (absolus, mm) ; les poignées du
    /// modèle en sont déduites (US-12.4).
    Bezier {
        start: PointMm,
        c1: PointMm,
        c2: PointMm,
        end: PointMm,
    },
}

/// Un objet à importer et son calque (nom ; `None` = calque par défaut).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImportObject {
    pub item: ImportItem,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<String>,
}

/// Index spatial des points de la feuille : fusion en temps constant (une recherche linéaire
/// rendrait un import de plusieurs milliers d'objets quadratique).
struct PointIndex {
    cells: HashMap<(i64, i64), Vec<PointId>>,
    positions: HashMap<PointId, PointMm>,
}

const CELL_MM: f64 = 4.0 * MERGE_TOLERANCE_MM;

impl PointIndex {
    fn cell(p: PointMm) -> (i64, i64) {
        (
            (p.x / CELL_MM).floor() as i64,
            (p.y / CELL_MM).floor() as i64,
        )
    }

    fn insert(&mut self, id: PointId, p: PointMm) {
        self.cells.entry(Self::cell(p)).or_default().push(id);
        self.positions.insert(id, p);
    }

    fn remove(&mut self, id: PointId) {
        if let Some(p) = self.positions.remove(&id) {
            if let Some(v) = self.cells.get_mut(&Self::cell(p)) {
                v.retain(|x| *x != id);
            }
        }
    }

    /// Le point existant le plus proche à `MERGE_TOLERANCE_MM` près (à égalité : le plus petit
    /// identifiant, pour un résultat déterministe).
    fn find(&self, p: PointMm) -> Option<PointId> {
        let (cx, cy) = Self::cell(p);
        let mut best: Option<(f64, PointId)> = None;
        for dx in -1..=1 {
            for dy in -1..=1 {
                let Some(ids) = self.cells.get(&(cx + dx, cy + dy)) else {
                    continue;
                };
                for id in ids {
                    let d = pf_geometry::distance_mm(self.positions[id], p);
                    if d <= MERGE_TOLERANCE_MM && best.is_none_or(|(bd, bid)| (d, *id) < (bd, bid))
                    {
                        best = Some((d, *id));
                    }
                }
            }
        }
        best.map(|(_, id)| id)
    }
}

/// Étiquettes A…Z, A1…Z1… dans l'ordre de `Sheet::next_label`, sans le recalculer à chaque point.
struct Labels {
    used: HashSet<String>,
    next: usize,
}

impl Labels {
    fn take(&mut self) -> String {
        loop {
            let (round, c) = (self.next / 26, (b'A' + (self.next % 26) as u8) as char);
            self.next += 1;
            let label = if round == 0 {
                c.to_string()
            } else {
                format!("{c}{round}")
            };
            if !self.used.contains(&label) {
                self.used.insert(label.clone());
                return label;
            }
        }
    }
}

#[derive(Default)]
struct Skipped {
    /// Segments déjà présents (sur la feuille ou plus haut dans l'import) et points déjà là.
    duplicates: usize,
    /// Segments nuls, arcs dégénérés après fusion.
    degenerate: usize,
    /// Arcs ou courbes devenus incohérents après fusion des points.
    inconsistent: usize,
}

struct Builder {
    index: PointIndex,
    labels: Labels,
    new_points: Vec<Point>,
    new_entities: Vec<Entity>,
    segments: HashSet<(PointId, PointId)>,
}

impl Builder {
    /// Point existant ou nouveau point libre à cet endroit.
    fn point(&mut self, project: &mut Project, at: PointMm, layer: Option<LayerId>) -> PointId {
        if let Some(id) = self.index.find(at) {
            return id;
        }
        let id = PointId(project.alloc_id());
        self.index.insert(id, at);
        self.new_points.push(Point {
            id,
            label: self.labels.take(),
            definition: PointDefinition::Free { x: at.x, y: at.y },
            role: PointRole::Construction,
            layer,
        });
        id
    }

    fn rollback(&mut self, mark: usize) {
        while self.new_points.len() > mark {
            if let Some(p) = self.new_points.pop() {
                self.index.remove(p.id);
            }
        }
    }

    fn entity(
        &mut self,
        project: &mut Project,
        kind: EntityKind,
        layer: Option<LayerId>,
    ) -> EntityId {
        let id = EntityId(project.alloc_id());
        self.new_entities.push(Entity {
            id,
            role: PathRole::Construction,
            kind,
            layer,
            name: None,
        });
        id
    }

    fn pos(&self, id: PointId) -> PointMm {
        self.index.positions[&id]
    }
}

fn check_point(p: PointMm) -> Result<(), CoreError> {
    p.validate()?;
    Ok(())
}

fn check_item(item: &ImportItem) -> Result<(), CoreError> {
    match item {
        ImportItem::Point { at } => check_point(*at),
        ImportItem::Segment { from, to } => {
            check_point(*from)?;
            check_point(*to)
        }
        ImportItem::Circle { center, radius_mm } => {
            check_point(*center)?;
            pf_geometry::check_radius(*radius_mm)?;
            Ok(())
        }
        ImportItem::Arc {
            center, start, end, ..
        } => {
            check_point(*center)?;
            check_point(*start)?;
            check_point(*end)
        }
        ImportItem::Spline { points } => {
            if points.len() < 2 || points.len() > MAX_SPLINE_NODES {
                return Err(CoreError::Invalid(format!(
                    "import : une courbe par points de passage a 2 à {MAX_SPLINE_NODES} points \
                     ({} reçus)",
                    points.len()
                )));
            }
            points.iter().try_for_each(|p| check_point(*p))
        }
        ImportItem::Bezier { start, c1, c2, end } => [start, c1, c2, end]
            .into_iter()
            .try_for_each(|p| check_point(*p)),
    }
}

impl Project {
    /// Crée une feuille puis y importe les objets, dans la même étape (US-12.4).
    pub(crate) fn import_geometry_new_sheet(
        &mut self,
        name: &str,
        objects: Vec<ImportObject>,
        out: &mut Outcome,
    ) -> Result<(), CoreError> {
        check_name(name, "le nom de la feuille")?;
        let id = SheetId(self.alloc_id());
        self.sheets.push(Sheet::new(id, name.trim()));
        self.import_geometry(id, objects, out)
    }

    pub(crate) fn import_geometry(
        &mut self,
        sheet: SheetId,
        objects: Vec<ImportObject>,
        out: &mut Outcome,
    ) -> Result<(), CoreError> {
        if objects.is_empty() {
            return Err(CoreError::Invalid("import : rien à importer".into()));
        }
        if objects.len() > MAX_IMPORT_OBJECTS {
            return Err(CoreError::Invalid(format!(
                "import : {} objets, plus que la limite de {MAX_IMPORT_OBJECTS} par import \
                 (découpez le fichier)",
                objects.len()
            )));
        }
        for o in &objects {
            check_item(&o.item)?;
        }

        // Calques : par nom, dans l'ordre de première apparition.
        let mut layer_of: HashMap<String, LayerId> = HashMap::new();
        let mut new_layers: Vec<Layer> = Vec::new();
        {
            let s = self.sheet(sheet)?;
            for l in &s.layers {
                layer_of.insert(l.name.clone(), l.id);
            }
        }
        let mut wanted: Vec<&str> = Vec::new();
        for o in &objects {
            if let Some(name) = o.layer.as_deref() {
                let name = name.trim();
                if !layer_of.contains_key(name) && !wanted.contains(&name) {
                    wanted.push(name);
                }
            }
        }
        if wanted.len() > MAX_IMPORT_LAYERS {
            return Err(CoreError::Invalid(format!(
                "import : {} calques à créer, plus que la limite de {MAX_IMPORT_LAYERS}",
                wanted.len()
            )));
        }
        for name in wanted {
            check_name(name, "le nom du calque")?;
            let id = LayerId(self.alloc_id());
            layer_of.insert(name.to_string(), id);
            new_layers.push(Layer {
                id,
                name: name.to_string(),
                visible: true,
                locked: false,
            });
        }

        let mut b = {
            let s = self.sheet(sheet)?;
            let mut index = PointIndex {
                cells: HashMap::new(),
                positions: HashMap::new(),
            };
            for p in &s.points {
                let pos = match p.definition {
                    PointDefinition::Free { x, y } => PointMm::new(x, y),
                    _ => s.position(p.id)?,
                };
                index.insert(p.id, pos);
            }
            let mut segments = HashSet::new();
            for e in &s.entities {
                if let EntityKind::Segment { from, to, .. } = e.kind {
                    segments.insert((from.min(to), from.max(to)));
                }
            }
            Builder {
                index,
                labels: Labels {
                    used: s.points.iter().map(|p| p.label.clone()).collect(),
                    next: 0,
                },
                new_points: Vec::new(),
                new_entities: Vec::new(),
                segments,
            }
        };

        let mut skipped = Skipped::default();
        for o in objects {
            let layer = o
                .layer
                .as_deref()
                .map(str::trim)
                .and_then(|n| layer_of.get(n).copied());
            let mark = b.new_points.len();
            let entities_mark = b.new_entities.len();
            let created = self.import_one(&mut b, &o.item, layer, &mut skipped);
            if !created {
                b.rollback(mark);
                b.new_entities.truncate(entities_mark);
            }
        }

        let s = self.sheet_mut(sheet)?;
        s.layers.extend(new_layers.iter().cloned());
        out.layers.extend(new_layers.iter().map(|l| l.id));
        out.points.extend(b.new_points.iter().map(|p| p.id));
        out.entities.extend(b.new_entities.iter().map(|e| e.id));
        s.points.extend(b.new_points);
        s.entities.extend(b.new_entities);
        out.sheet = Some(sheet);
        if skipped.duplicates > 0 {
            out.notes.push(format!(
                "Import : {} objet(s) déjà présent(s) non recréé(s).",
                skipped.duplicates
            ));
        }
        if skipped.degenerate > 0 {
            out.notes.push(format!(
                "Import : {} objet(s) de longueur nulle après fusion des points confondus non \
                 créé(s).",
                skipped.degenerate
            ));
        }
        if skipped.inconsistent > 0 {
            out.notes.push(format!(
                "Import : {} arc(s) ou courbe(s) devenu(s) incohérent(s) après fusion des points \
                 confondus non créé(s).",
                skipped.inconsistent
            ));
        }
        Ok(())
    }

    /// Crée un objet ; renvoie faux (rien créé, remarque comptée) s'il est impossible.
    fn import_one(
        &mut self,
        b: &mut Builder,
        item: &ImportItem,
        layer: Option<LayerId>,
        skipped: &mut Skipped,
    ) -> bool {
        match item {
            ImportItem::Point { at } => {
                if b.index.find(*at).is_some() {
                    skipped.duplicates += 1;
                    return false;
                }
                b.point(self, *at, layer);
                true
            }
            ImportItem::Segment { from, to } => {
                let (a, c) = (b.point(self, *from, layer), b.point(self, *to, layer));
                if a == c {
                    skipped.degenerate += 1;
                    return false;
                }
                if !b.segments.insert((a.min(c), a.max(c))) {
                    skipped.duplicates += 1;
                    return false;
                }
                b.entity(
                    self,
                    EntityKind::Segment {
                        from: a,
                        to: c,
                        construction: None,
                    },
                    layer,
                );
                true
            }
            ImportItem::Circle { center, radius_mm } => {
                let c = b.point(self, *center, layer);
                let pos = b.pos(c);
                if pf_geometry::CircleTrace::new(pos, *radius_mm, 0.0, true).is_err() {
                    skipped.inconsistent += 1;
                    return false;
                }
                b.entity(
                    self,
                    EntityKind::Circle {
                        center: c,
                        radius_mm: *radius_mm,
                        start_deg: 0.0,
                        ccw: true,
                    },
                    layer,
                );
                true
            }
            ImportItem::Arc {
                center,
                start,
                end,
                ccw,
            } => {
                let (c, s, e) = (
                    b.point(self, *center, layer),
                    b.point(self, *start, layer),
                    b.point(self, *end, layer),
                );
                if c == s || c == e || s == e {
                    skipped.degenerate += 1;
                    return false;
                }
                if pf_geometry::Arc::from_center_points(b.pos(c), b.pos(s), b.pos(e), *ccw).is_err()
                {
                    skipped.inconsistent += 1;
                    return false;
                }
                b.entity(
                    self,
                    EntityKind::Arc {
                        center: c,
                        start: s,
                        end: e,
                        ccw: *ccw,
                    },
                    layer,
                );
                true
            }
            ImportItem::Spline { points } => {
                let ids: Vec<PointId> = points.iter().map(|p| b.point(self, *p, layer)).collect();
                let distinct: HashSet<PointId> = ids.iter().copied().collect();
                let positions: Vec<PointMm> = ids.iter().map(|id| b.pos(*id)).collect();
                if distinct.len() != ids.len()
                    || pf_geometry::Spline::through_points(&positions).is_err()
                {
                    skipped.inconsistent += 1;
                    return false;
                }
                b.entity(
                    self,
                    EntityKind::Spline {
                        nodes: ids.into_iter().map(SplineNode::new).collect(),
                    },
                    layer,
                );
                true
            }
            ImportItem::Bezier { start, c1, c2, end } => {
                let (s, e) = (b.point(self, *start, layer), b.point(self, *end, layer));
                let (ps, pe) = (b.pos(s), b.pos(e));
                if pf_geometry::CubicBezier::new(ps, *c1, *c2, pe)
                    .validate()
                    .is_err()
                {
                    skipped.degenerate += 1;
                    return false;
                }
                b.entity(
                    self,
                    EntityKind::Curve {
                        start: s,
                        handle_start: OffsetMm {
                            dx: c1.x - ps.x,
                            dy: c1.y - ps.y,
                        },
                        handle_end: OffsetMm {
                            dx: c2.x - pe.x,
                            dy: c2.y - pe.y,
                        },
                        end: e,
                    },
                    layer,
                );
                true
            }
        }
    }
}
