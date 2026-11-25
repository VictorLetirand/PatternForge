//! PH-4a — construction rapide : polygone, rectangle orienté, rectangle à coins arrondis.
//! Géométrie ordinaire (points, segments, arcs existants) : aucun type ni champ nouveau,
//! schéma 3 inchangé. Les maths sont dans `pf_geometry::quick`.

use super::*;
use pf_geometry::quick::{rounded_rectangle_layout, validate_polygon, RoundedItem};

/// Nom du calque (masqué) qui reçoit les centres d'arcs d'un rectangle arrondi.
pub const CENTERS_LAYER_NAME: &str = "Centres d'arcs";

impl Project {
    /// Range dans `layer` et donne `role` aux points et tracés créés depuis les marques.
    fn style_created(
        &mut self,
        sheet: SheetId,
        marks: (usize, usize),
        role: PathRole,
        layer: Option<LayerId>,
        out: &Outcome,
    ) -> Result<(), CoreError> {
        let s = self.sheet_mut(sheet)?;
        for id in &out.points[marks.0..] {
            if let Some(p) = s.points.iter_mut().find(|p| p.id == *id) {
                p.layer = layer;
            }
        }
        for id in &out.entities[marks.1..] {
            if let Some(e) = s.entities.iter_mut().find(|e| e.id == *id) {
                e.role = role;
                e.layer = layer;
            }
        }
        Ok(())
    }

    pub(super) fn add_polygon(
        &mut self,
        sheet: SheetId,
        vertices: &[PointRef],
        closed: bool,
        role: PathRole,
        layer: Option<LayerId>,
        out: &mut Outcome,
    ) -> Result<(), CoreError> {
        if let Some(l) = layer {
            self.sheet(sheet)?.layer(l)?;
        }
        // Le nombre de sommets est vérifié avant toute résolution (coût borné).
        if vertices.len() > pf_geometry::quick::MAX_POLYGON_VERTICES {
            return Err(CoreError::Invalid(format!(
                "un polygone a au plus {} sommets ({} donnés)",
                pf_geometry::quick::MAX_POLYGON_VERTICES,
                vertices.len()
            )));
        }
        let marks = (out.points.len(), out.entities.len());
        let mut ids = Vec::with_capacity(vertices.len());
        for v in vertices {
            ids.push(self.resolve_ref(sheet, *v, out)?);
        }
        let s = self.sheet(sheet)?;
        let positions: Vec<PointMm> = ids
            .iter()
            .map(|id| s.position(*id))
            .collect::<Result<_, _>>()?;
        validate_polygon(&positions, closed)?;
        for w in ids.windows(2) {
            if w[0] == w[1] {
                return Err(pf_geometry::GeometryError::ZeroLengthSegment.into());
            }
        }
        let n = ids.len();
        let sides = if closed { n } else { n - 1 };
        for i in 0..sides {
            let (a, b) = (ids[i], ids[(i + 1) % n]);
            self.add_segment(sheet, a, b, None, out)?;
        }
        self.style_created(sheet, marks, role, layer, out)
    }

    /// Calque des centres d'arcs : celui qui porte déjà ce nom, sinon un calque créé (masqué).
    fn centers_layer(&mut self, sheet: SheetId, out: &mut Outcome) -> Result<LayerId, CoreError> {
        self.hidden_layer(sheet, CENTERS_LAYER_NAME, out)
    }

    /// Calque de ce nom : celui qui existe déjà, sinon un calque créé **masqué** (PH-4b : points
    /// d'ancrage des perçages au pas).
    pub(crate) fn hidden_layer(
        &mut self,
        sheet: SheetId,
        name: &str,
        out: &mut Outcome,
    ) -> Result<LayerId, CoreError> {
        let existing = self
            .sheet(sheet)?
            .layers
            .iter()
            .find(|l| l.name == name)
            .map(|l| l.id);
        if let Some(id) = existing {
            return Ok(id);
        }
        let id = LayerId(self.alloc_id());
        self.sheet_mut(sheet)?.layers.push(Layer {
            id,
            name: name.to_string(),
            visible: false,
            locked: false,
        });
        out.layers.push(id);
        Ok(id)
    }

    pub(super) fn add_rounded_rectangle(
        &mut self,
        sheet: SheetId,
        rect: &pf_geometry::RectSpec,
        radius_mm: f64,
        hide_centers: bool,
        style: (PathRole, Option<LayerId>),
        out: &mut Outcome,
    ) -> Result<(), CoreError> {
        let (role, layer) = style;
        if let Some(l) = layer {
            self.sheet(sheet)?.layer(l)?;
        }
        let lay = rounded_rectangle_layout(rect, radius_mm)?;
        let marks = (out.points.len(), out.entities.len());
        let centers_layer = if hide_centers {
            Some(self.centers_layer(sheet, out)?)
        } else {
            layer
        };
        let mut ids = Vec::with_capacity(lay.points.len());
        for (i, p) in lay.points.iter().enumerate() {
            let target = if lay.is_center[i] {
                centers_layer
            } else {
                layer
            };
            ids.push(self.tangency_point(sheet, *p, target, out)?);
        }
        for item in &lay.items {
            match *item {
                RoundedItem::Line { from, to } => {
                    self.add_segment(sheet, ids[from], ids[to], None, out)?
                }
                RoundedItem::Arc { center, start, end } => {
                    self.add_arc_entity(sheet, ids[center], ids[start], ids[end], true, out)?
                }
            }
        }
        // Rôle et calque des tracés ; les points ont déjà leur calque (centres à part).
        let s = self.sheet_mut(sheet)?;
        for id in &out.entities[marks.1..] {
            if let Some(e) = s.entities.iter_mut().find(|e| e.id == *id) {
                e.role = role;
                e.layer = layer;
            }
        }
        if hide_centers {
            let shown = lay.center_count();
            if let Some(l) = centers_layer {
                let name = self.sheet(sheet)?.layer_name(Some(l));
                let state = if self.sheet(sheet)?.layer_visible(Some(l)) {
                    "visible"
                } else {
                    "masqué"
                };
                // Le texte nomme toujours l'état du calque : jamais de masquage silencieux.
                out.notes.push(format!(
                    "{shown} centre(s) d'arc dans le calque {state} « {name} »."
                ));
            }
        }
        Ok(())
    }
}
