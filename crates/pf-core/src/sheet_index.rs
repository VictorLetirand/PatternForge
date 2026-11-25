//! Index de recherche par identifiant et cache des positions, actifs le temps d'une lecture de
//! masse d'une feuille (validation, création de point, export).
//!
//! `Sheet::point`, `Sheet::entity` et `Sheet::layer` cherchent linéairement et `Sheet::position`
//! recalcule toute la chaîne des parents : sur une feuille entière, c'est quadratique ou pire.
//! Tant qu'une garde vit, la feuille est empruntée en lecture seule (la durée de vie de la garde
//! l'impose) : on construit donc **une fois** un index id -> position, et chaque position
//! calculée est retenue. Hors garde, aucun index n'existe et le comportement est celui de la
//! recherche linéaire. Les résultats sont identiques dans les deux cas.

use crate::error::CoreError;
use crate::model::{EntityId, LayerId, PointId, Sheet};
use pf_geometry::PointMm;
use std::cell::RefCell;
use std::collections::HashMap;
use std::marker::PhantomData;

struct Index {
    points: HashMap<PointId, usize>,
    entities: HashMap<EntityId, usize>,
    layers: HashMap<LayerId, usize>,
    positions: HashMap<PointId, Result<PointMm, CoreError>>,
}

thread_local! {
    /// Clé : adresse de la feuille, stable tant que la garde l'emprunte.
    static ACTIVE: RefCell<HashMap<usize, Index>> = RefCell::new(HashMap::new());
}

fn key(sheet: &Sheet) -> usize {
    std::ptr::from_ref(sheet) as usize
}

/// Garde : l'index et le cache de la feuille existent tant qu'elle vit. Une garde imbriquée sur
/// la même feuille réutilise ceux de la première.
pub(crate) struct IndexGuard<'a> {
    /// `Some` pour la garde qui a créé l'index (et le retirera).
    owned: Option<usize>,
    /// Emprunt de la feuille ; le pointeur brut empêche aussi de changer de fil.
    sheet: PhantomData<(&'a Sheet, *const ())>,
}

fn first_positions<K: std::hash::Hash + Eq + Copy>(
    keys: impl Iterator<Item = K>,
) -> HashMap<K, usize> {
    let mut map = HashMap::new();
    for (i, k) in keys.enumerate() {
        map.entry(k).or_insert(i);
    }
    map
}

impl<'a> IndexGuard<'a> {
    pub(crate) fn new(sheet: &'a Sheet) -> IndexGuard<'a> {
        let k = key(sheet);
        let owned = ACTIVE.with(|a| {
            if a.borrow().contains_key(&k) {
                return None;
            }
            let index = Index {
                points: first_positions(sheet.points.iter().map(|p| p.id)),
                entities: first_positions(sheet.entities.iter().map(|e| e.id)),
                layers: first_positions(sheet.layers.iter().map(|l| l.id)),
                positions: HashMap::new(),
            };
            a.borrow_mut().insert(k, index);
            Some(k)
        });
        IndexGuard {
            owned,
            sheet: PhantomData,
        }
    }
}

impl Drop for IndexGuard<'_> {
    fn drop(&mut self) {
        if let Some(k) = self.owned {
            ACTIVE.with(|a| {
                a.borrow_mut().remove(&k);
            });
        }
    }
}

/// `None` = pas d'index pour cette feuille.
fn with_index<T>(sheet: &Sheet, f: impl FnOnce(&mut Index) -> T) -> Option<T> {
    ACTIVE.with(|a| {
        let mut a = a.borrow_mut();
        if a.is_empty() {
            return None;
        }
        a.get_mut(&key(sheet)).map(f)
    })
}

/// `None` = pas d'index pour cette feuille (recherche linéaire) ;
/// `Some(None)` = absent ; `Some(Some(i))` = position.
pub(crate) fn point_position(sheet: &Sheet, id: PointId) -> Option<Option<usize>> {
    with_index(sheet, |i| i.points.get(&id).copied())
}

pub(crate) fn entity_position(sheet: &Sheet, id: EntityId) -> Option<Option<usize>> {
    with_index(sheet, |i| i.entities.get(&id).copied())
}

pub(crate) fn layer_position(sheet: &Sheet, id: LayerId) -> Option<Option<usize>> {
    with_index(sheet, |i| i.layers.get(&id).copied())
}

/// Résultat déjà calculé pour ce point, si un cache est actif.
pub(crate) fn cached_position(sheet: &Sheet, id: PointId) -> Option<Result<PointMm, CoreError>> {
    with_index(sheet, |i| i.positions.get(&id).cloned()).flatten()
}

/// Retient un résultat. Un refus pour **dépendance circulaire** n'est jamais retenu : le point
/// qu'il nomme dépend de celui par lequel on est entré dans le cycle.
pub(crate) fn store_position(sheet: &Sheet, id: PointId, result: &Result<PointMm, CoreError>) {
    if matches!(result, Err(CoreError::CyclicReference(_))) {
        return;
    }
    with_index(sheet, |i| {
        i.positions.insert(id, result.clone());
    });
}
