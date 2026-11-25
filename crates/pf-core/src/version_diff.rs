//! US-17.2 - comparaison de deux projets (versions). Module **pur** : aucune E/S, rien de stocke,
//! sortie triee et deterministe (par identifiant, puis par nom pour les variables).
//!
//! Conventions (a faire valider, non validees metier) :
//! - feuilles, points, traces, pieces, gammes appariees par **identifiant** ; variables par **nom** ;
//! - un point **deplace** = meme identifiant, position evaluee (`Sheet::position`) differente de plus
//!   de `MOVE_EPSILON_MM` ; la distance est la distance euclidienne en mm (jamais un cumul) ;
//!   un point non evaluable d'un cote est ignore (ni deplace ni ajoute) ;
//! - un trace **modifie** = meme identifiant, definition differente (extremites, role, nom...) ;
//! - une piece **changee** = nom, matiere ou marges de couture differents ;
//! - la superposition (`overlay`) dessine chaque trace par sa corde (extremites) : une courbe est
//!   donc approchee par une droite, un cercle par son centre.

use crate::model::*;
use pf_geometry::PointMm;
use std::collections::BTreeMap;

/// Deplacement minimal retenu (mm) : en dessous, le point est considere inchange.
pub const MOVE_EPSILON_MM: f64 = 1e-6;

#[derive(Debug, Clone, PartialEq)]
pub struct PointEntry {
    pub id: PointId,
    pub label: String,
    pub position: PointMm,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PointMove {
    pub id: PointId,
    pub label: String,
    pub from: PointMm,
    pub to: PointMm,
    /// Distance de deplacement (mm).
    pub distance_mm: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PathEntry {
    pub id: EntityId,
    pub name: Option<String>,
}

/// Differences d'une feuille presente dans les deux projets.
#[derive(Debug, Clone, PartialEq)]
pub struct SheetDiff {
    pub sheet: SheetId,
    pub name: String,
    /// `(ancien, nouveau)` si la feuille est renommee.
    pub renamed: Option<(String, String)>,
    pub points_added: Vec<PointEntry>,
    pub points_removed: Vec<PointEntry>,
    pub points_moved: Vec<PointMove>,
    pub paths_added: Vec<PathEntry>,
    pub paths_removed: Vec<PathEntry>,
    pub paths_changed: Vec<PathEntry>,
}

impl SheetDiff {
    pub fn is_empty(&self) -> bool {
        self.renamed.is_none()
            && self.points_added.is_empty()
            && self.points_removed.is_empty()
            && self.points_moved.is_empty()
            && self.paths_added.is_empty()
            && self.paths_removed.is_empty()
            && self.paths_changed.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PieceChange {
    Name {
        from: String,
        to: String,
    },
    Material {
        from: Option<String>,
        to: Option<String>,
    },
    Seam,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PieceDiff {
    pub piece: PieceId,
    /// Nom dans le projet le plus recent (le nouveau).
    pub name: String,
    pub changes: Vec<PieceChange>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Presence {
    Added,
    Removed,
    Changed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamedChange {
    pub name: String,
    pub change: Presence,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct VersionDiff {
    pub sheets_added: Vec<(SheetId, String)>,
    pub sheets_removed: Vec<(SheetId, String)>,
    pub sheets: Vec<SheetDiff>,
    pub pieces_added: Vec<(PieceId, String)>,
    pub pieces_removed: Vec<(PieceId, String)>,
    pub pieces: Vec<PieceDiff>,
    pub sizes: Vec<NamedChange>,
    pub variables: Vec<NamedChange>,
}

impl VersionDiff {
    pub fn is_empty(&self) -> bool {
        self.sheets_added.is_empty()
            && self.sheets_removed.is_empty()
            && self.sheets.is_empty()
            && self.pieces_added.is_empty()
            && self.pieces_removed.is_empty()
            && self.pieces.is_empty()
            && self.sizes.is_empty()
            && self.variables.is_empty()
    }
}

fn positions(s: &Sheet) -> BTreeMap<u64, (&Point, Option<PointMm>)> {
    s.points
        .iter()
        .map(|p| (p.id.0, (p, s.position(p.id).ok())))
        .collect()
}

fn sheet_diff(old: &Sheet, new: &Sheet) -> SheetDiff {
    let mut d = SheetDiff {
        sheet: new.id,
        name: new.name.clone(),
        renamed: (old.name != new.name).then(|| (old.name.clone(), new.name.clone())),
        points_added: Vec::new(),
        points_removed: Vec::new(),
        points_moved: Vec::new(),
        paths_added: Vec::new(),
        paths_removed: Vec::new(),
        paths_changed: Vec::new(),
    };
    let (po, pn) = (positions(old), positions(new));
    for (id, (p, pos)) in &pn {
        match po.get(id) {
            None => {
                if let Some(position) = *pos {
                    d.points_added.push(PointEntry {
                        id: p.id,
                        label: p.label.clone(),
                        position,
                    });
                }
            }
            Some((_, before)) => {
                if let (Some(a), Some(b)) = (before, pos) {
                    let dist = ((b.x - a.x).powi(2) + (b.y - a.y).powi(2)).sqrt();
                    if dist > MOVE_EPSILON_MM {
                        d.points_moved.push(PointMove {
                            id: p.id,
                            label: p.label.clone(),
                            from: *a,
                            to: *b,
                            distance_mm: dist,
                        });
                    }
                }
            }
        }
    }
    for (id, (p, pos)) in &po {
        if !pn.contains_key(id) {
            if let Some(position) = *pos {
                d.points_removed.push(PointEntry {
                    id: p.id,
                    label: p.label.clone(),
                    position,
                });
            }
        }
    }
    let eo: BTreeMap<u64, &Entity> = old.entities.iter().map(|e| (e.id.0, e)).collect();
    let en: BTreeMap<u64, &Entity> = new.entities.iter().map(|e| (e.id.0, e)).collect();
    let entry = |e: &Entity| PathEntry {
        id: e.id,
        name: e.name.clone(),
    };
    for (id, e) in &en {
        match eo.get(id) {
            None => d.paths_added.push(entry(e)),
            Some(before) if *before != *e => d.paths_changed.push(entry(e)),
            Some(_) => {}
        }
    }
    for (id, e) in &eo {
        if !en.contains_key(id) {
            d.paths_removed.push(entry(e));
        }
    }
    d
}

fn piece_diff(old: &Piece, new: &Piece) -> Option<PieceDiff> {
    let mut changes = Vec::new();
    if old.name != new.name {
        changes.push(PieceChange::Name {
            from: old.name.clone(),
            to: new.name.clone(),
        });
    }
    if old.material != new.material {
        changes.push(PieceChange::Material {
            from: old.material.clone(),
            to: new.material.clone(),
        });
    }
    if old.seam != new.seam {
        changes.push(PieceChange::Seam);
    }
    (!changes.is_empty()).then(|| PieceDiff {
        piece: new.id,
        name: new.name.clone(),
        changes,
    })
}

/// Differences de `new` par rapport a `old` (`old` = la version la plus ancienne ou l'etat de depart).
pub fn diff_projects(old: &Project, new: &Project) -> VersionDiff {
    let mut out = VersionDiff::default();
    let so: BTreeMap<u64, &Sheet> = old.sheets.iter().map(|s| (s.id.0, s)).collect();
    let sn: BTreeMap<u64, &Sheet> = new.sheets.iter().map(|s| (s.id.0, s)).collect();
    for (id, s) in &sn {
        match so.get(id) {
            None => out.sheets_added.push((s.id, s.name.clone())),
            Some(before) => {
                let d = sheet_diff(before, s);
                if !d.is_empty() {
                    out.sheets.push(d);
                }
            }
        }
    }
    for (id, s) in &so {
        if !sn.contains_key(id) {
            out.sheets_removed.push((s.id, s.name.clone()));
        }
    }
    let po: BTreeMap<u64, &Piece> = old.pieces.iter().map(|p| (p.id.0, p)).collect();
    let pn: BTreeMap<u64, &Piece> = new.pieces.iter().map(|p| (p.id.0, p)).collect();
    for (id, p) in &pn {
        match po.get(id) {
            None => out.pieces_added.push((p.id, p.name.clone())),
            Some(before) => out.pieces.extend(piece_diff(before, p)),
        }
    }
    for (id, p) in &po {
        if !pn.contains_key(id) {
            out.pieces_removed.push((p.id, p.name.clone()));
        }
    }
    named(
        &mut out.sizes,
        old.sizes.iter().map(|r| (r.name.clone(), r)),
        new.sizes.iter().map(|r| (r.name.clone(), r)),
        true,
    );
    named(
        &mut out.variables,
        old.variables.iter().map(|v| (v.name.clone(), v)),
        new.variables.iter().map(|v| (v.name.clone(), v)),
        false,
    );
    out
}

/// Gammes (appariees par identifiant, d'ou `by_id`) ou variables (par nom) ajoutees / retirees /
/// changees. Pour les gammes, le nom affiche est celui de la plus recente.
fn named<'a, T: PartialEq + HasKey + 'a>(
    out: &mut Vec<NamedChange>,
    old: impl Iterator<Item = (String, &'a T)>,
    new: impl Iterator<Item = (String, &'a T)>,
    by_id: bool,
) {
    let key = |name: &str, t: &T| {
        if by_id {
            t.key()
        } else {
            name.to_string()
        }
    };
    let o: BTreeMap<String, (String, &T)> = old.map(|(n, t)| (key(&n, t), (n, t))).collect();
    let n: BTreeMap<String, (String, &T)> = new.map(|(n, t)| (key(&n, t), (n, t))).collect();
    for (k, (name, t)) in &n {
        match o.get(k) {
            None => out.push(NamedChange {
                name: name.clone(),
                change: Presence::Added,
            }),
            Some((_, before)) if *before != *t => out.push(NamedChange {
                name: name.clone(),
                change: Presence::Changed,
            }),
            Some(_) => {}
        }
    }
    for (k, (name, _)) in &o {
        if !n.contains_key(k) {
            out.push(NamedChange {
                name: name.clone(),
                change: Presence::Removed,
            });
        }
    }
}

trait HasKey {
    fn key(&self) -> String;
}

impl HasKey for crate::size::SizeRange {
    fn key(&self) -> String {
        format!("{:020}", self.id.0)
    }
}

impl HasKey for crate::variable::Variable {
    fn key(&self) -> String {
        self.name.clone()
    }
}

/// Role d'un element dans la superposition de deux etats d'une feuille.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayKind {
    /// Present dans les deux etats (dessine dans le nouvel etat).
    Kept,
    /// Present seulement dans l'ancien etat.
    Removed,
    /// Present seulement dans le nouvel etat.
    Added,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OverlayLine {
    pub kind: OverlayKind,
    pub from: PointMm,
    pub to: PointMm,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OverlayPoint {
    pub kind: OverlayKind,
    pub at: PointMm,
}

/// Superposition des deux etats d'une feuille (lecture seule, rien n'est stocke).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Overlay {
    pub lines: Vec<OverlayLine>,
    pub points: Vec<OverlayPoint>,
    /// Deplacements : fleche de `from` vers `to`.
    pub moves: Vec<PointMove>,
}

fn chord(s: &Sheet, e: &Entity) -> Option<(PointMm, PointMm)> {
    let (a, b) = s.entity_points(e.id).ok()?;
    Some((s.position(a).ok()?, s.position(b).ok()?))
}

/// Superposition de la feuille `sheet` entre `old` et `new` (la feuille absente d'un cote : tout
/// l'autre cote est « supprime » ou « ajoute »).
pub fn overlay(old: &Project, new: &Project, sheet: SheetId) -> Overlay {
    let so = old.sheet(sheet).ok();
    let sn = new.sheet(sheet).ok();
    let mut out = Overlay::default();
    let eo: BTreeMap<u64, &Entity> = so
        .map(|s| s.entities.iter().map(|e| (e.id.0, e)).collect())
        .unwrap_or_default();
    let en: BTreeMap<u64, &Entity> = sn
        .map(|s| s.entities.iter().map(|e| (e.id.0, e)).collect())
        .unwrap_or_default();
    if let Some(s) = sn {
        for (id, e) in &en {
            let kind = if eo.contains_key(id) {
                OverlayKind::Kept
            } else {
                OverlayKind::Added
            };
            if let Some((from, to)) = chord(s, e) {
                out.lines.push(OverlayLine { kind, from, to });
            }
        }
    }
    if let Some(s) = so {
        for (id, e) in &eo {
            if !en.contains_key(id) {
                if let Some((from, to)) = chord(s, e) {
                    out.lines.push(OverlayLine {
                        kind: OverlayKind::Removed,
                        from,
                        to,
                    });
                }
            }
        }
    }
    if let (Some(a), Some(b)) = (so, sn) {
        let d = sheet_diff(a, b);
        out.points
            .extend(d.points_added.iter().map(|p| OverlayPoint {
                kind: OverlayKind::Added,
                at: p.position,
            }));
        out.points
            .extend(d.points_removed.iter().map(|p| OverlayPoint {
                kind: OverlayKind::Removed,
                at: p.position,
            }));
        out.moves = d.points_moved;
    }
    out
}
