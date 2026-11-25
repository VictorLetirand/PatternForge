//! Crans : petits repères perpendiculaires au bord d'un tracé. Leur position
//! est une longueur mesurée le long du tracé depuis son départ ; leur
//! géométrie est recalculée à la demande (rien n'est stocké).

use crate::command::Outcome;
use crate::error::CoreError;
use crate::model::{
    EntityId, EntityKind, Notch, NotchAnchor, NotchForm, NotchId, NotchStyle, Piece, PieceId,
    PointId, Project, Sheet, SheetId,
};
use crate::piece_seam::CutContour;
use pf_geometry::{PointMm, Side, Trace, VectorMm, DEFAULT_LENGTH_TOLERANCE_MM};

/// Tolérance (mm) sur le dépassement du bord d'une position ancrée (flottants).
const ANCHOR_TOLERANCE_MM: f64 = 1e-9;

/// Longueur d'un cran créé sans précision (mm).
pub const DEFAULT_NOTCH_LENGTH_MM: f64 = 5.0;
/// Longueur maximale acceptée (mm).
pub const MAX_NOTCH_LENGTH_MM: f64 = 100.0;
/// Écart entre les deux traits d'un cran double, le long du bord (mm).
pub const DOUBLE_NOTCH_GAP_MM: f64 = 3.0;

impl Sheet {
    pub fn notch(&self, id: NotchId) -> Result<&Notch, CoreError> {
        self.notches
            .iter()
            .find(|n| n.id == id)
            .ok_or(CoreError::UnknownNotch(id))
    }

    /// Longueur d'un tracé en mm (développée pour une courbe).
    pub fn path_length_mm(&self, entity: EntityId) -> Result<f64, CoreError> {
        match self.entity(entity)?.kind {
            EntityKind::Segment { .. } => {
                let (a, b) = self.segment_ends(entity)?;
                Ok(pf_geometry::segment_length_mm(a, b)?)
            }
            EntityKind::Curve { .. } => Ok(self
                .curve_geometry(entity)?
                .length_mm(DEFAULT_LENGTH_TOLERANCE_MM)?),
            EntityKind::Arc { .. } => Ok(self.arc_geometry(entity)?.length_mm()),
            EntityKind::Circle { .. } => Ok(self.circle_geometry(entity)?.length_mm()),
            EntityKind::Spline { .. } => Ok(self.spline_geometry(entity)?.length_mm()?),
        }
    }

    /// Point d'un tracé à `distance_mm` de son départ, avec la tangente unitaire.
    pub fn path_point_at(
        &self,
        entity: EntityId,
        distance_mm: f64,
    ) -> Result<(PointMm, VectorMm), CoreError> {
        match self.entity(entity)?.kind {
            EntityKind::Segment { .. } => {
                let (a, b) = self.segment_ends(entity)?;
                let pos = pf_geometry::point_on_segment(a, b, distance_mm)?;
                Ok((pos, (b - a).unit()?))
            }
            EntityKind::Curve { .. } => {
                let c = self.curve_geometry(entity)?;
                let t = c.t_at_length_mm(distance_mm, DEFAULT_LENGTH_TOLERANCE_MM)?;
                Ok((c.point_at(t), c.unit_tangent_at(t)?))
            }
            EntityKind::Arc { .. } => {
                Ok(self.arc_geometry(entity)?.point_at_length(distance_mm)?)
            }
            EntityKind::Circle { .. } => {
                Ok(self.circle_geometry(entity)?.point_at_length(distance_mm)?)
            }
            EntityKind::Spline { .. } => {
                Ok(self.spline_geometry(entity)?.point_at_length(distance_mm)?)
            }
        }
    }

    /// Distance, le long du tracé depuis son départ, du point du tracé le plus proche de `p`.
    pub fn path_distance_of(&self, entity: EntityId, p: PointMm) -> Result<f64, CoreError> {
        match self.entity(entity)?.kind {
            EntityKind::Segment { .. } => {
                let (a, b) = self.segment_ends(entity)?;
                Ok(pf_geometry::project_on_segment(p, a, b)?.0)
            }
            EntityKind::Curve { .. } => {
                let c = self.curve_geometry(entity)?;
                let (t, _) = c.nearest_t(p);
                Ok(c.length_to_t_mm(t, DEFAULT_LENGTH_TOLERANCE_MM)?)
            }
            EntityKind::Arc { .. } => Ok(self.arc_geometry(entity)?.nearest_length_mm(p)),
            EntityKind::Circle { .. } => Ok(self.circle_geometry(entity)?.nearest_length_mm(p)),
            EntityKind::Spline { .. } => Ok(self.spline_geometry(entity)?.nearest_length_mm(p)?),
        }
    }

    /// Position **effective** d'un cran, en mm depuis le départ de son tracé : la distance
    /// stockée pour un cran à distance fixe, la position calculée d'après la construction
    /// pour un cran ancré (US-06.2).
    pub fn notch_distance_mm(&self, notch: &Notch) -> Result<f64, CoreError> {
        match &notch.anchor {
            None => Ok(notch.distance_mm),
            Some(anchor) => self.anchor_distance_mm(notch.entity, anchor),
        }
    }

    /// Distance, depuis le départ de `entity`, que donne `anchor` d'après l'état actuel de
    /// la feuille. Refus expliqué : proportion hors [0 ; 1], point ou tracé absent, report
    /// qui sort du bord.
    pub fn anchor_distance_mm(
        &self,
        entity: EntityId,
        anchor: &NotchAnchor,
    ) -> Result<f64, CoreError> {
        let len = self.path_length_mm(entity)?;
        let dead_point = |p: PointId| {
            self.point(p).map(|_| ()).map_err(|_| {
                CoreError::DeadReference(format!(
                    "un cran ancré référence un point absent (id {})",
                    p.0
                ))
            })
        };
        let d = match *anchor {
            NotchAnchor::Midpoint {} => len / 2.0,
            NotchAnchor::Ratio { ratio } => {
                if !ratio.is_finite() || !(0.0..=1.0).contains(&ratio) {
                    return Err(CoreError::Invalid(
                        "la proportion d'un cran ancré doit être un nombre compris entre 0 et 1"
                            .into(),
                    ));
                }
                ratio * len
            }
            NotchAnchor::Point { point } => {
                dead_point(point)?;
                self.path_distance_of(entity, self.position(point)?)?
            }
            NotchAnchor::Carried {
                entity: source,
                point,
                from_end,
            } => {
                dead_point(point)?;
                if self.entity(source).is_err() {
                    return Err(CoreError::DeadReference(format!(
                        "un cran ancré référence un tracé source absent (id {})",
                        source.0
                    )));
                }
                let carried = self.path_distance_of(source, self.position(point)?)?;
                if from_end {
                    len - carried
                } else {
                    carried
                }
            }
        };
        if !d.is_finite() || d < -ANCHOR_TOLERANCE_MM || d > len + ANCHOR_TOLERANCE_MM {
            return Err(CoreError::Invalid(format!(
                "cran ancré sur {} : la position reportée ({} mm) sort du bord (longueur {} mm)",
                self.entity_name(entity),
                crate::input::format_mm(d),
                crate::input::format_mm(len)
            )));
        }
        Ok(d.clamp(0.0, len))
    }

    /// Contrôle d'un cran ancré (validation du projet) : références vivantes et position
    /// calculable.
    pub(crate) fn check_notch_anchor(
        &self,
        notch: &Notch,
        anchor: &NotchAnchor,
    ) -> Result<(), CoreError> {
        self.anchor_distance_mm(notch.entity, anchor).map(|_| ())
    }

    /// Cran sans ancrage, à la position qu'il a maintenant (aucun changement visible).
    pub(crate) fn notch_fixed(&self, notch: &Notch) -> Result<Notch, CoreError> {
        Ok(Notch {
            distance_mm: self.notch_distance_mm(notch)?,
            anchor: None,
            ..notch.clone()
        })
    }

    /// Traits d'un cran **sur la couture** (le bord de son tracé), segment par segment. Un
    /// simple trait va du pied (sur le bord) à la pointe ; les autres formes (T, V, U, fente)
    /// sont décomposées en petits segments. Un cran « sur la coupe » (`on_cut`) n'a aucun trait
    /// ici : il se lit par `Project::notch_placed_marks` (la coupe dépend de la pièce).
    pub fn notch_marks(&self, notch: &Notch) -> Result<Vec<(PointMm, PointMm)>, CoreError> {
        if notch.on_cut {
            return Ok(Vec::new());
        }
        self.notch_seam_marks(notch)
    }

    /// Traits d'un cran tel qu'il est posé sur le bord de couture, même s'il est « sur la
    /// coupe » (sert de repli quand le cran n'est dans aucune pièce).
    pub fn notch_seam_marks(&self, notch: &Notch) -> Result<Vec<(PointMm, PointMm)>, CoreError> {
        let len = self.path_length_mm(notch.entity)?;
        let at = self.notch_distance_mm(notch)?;
        let k = match notch.side {
            Side::Left => 1.0,
            Side::Right => -1.0,
        };
        let mut out = Vec::new();
        for o in notch_offsets(notch.style) {
            let d = (at + o).clamp(0.0, len);
            let (base, tangent) = self.path_point_at(notch.entity, d)?;
            out.extend(shape_marks(
                notch,
                base,
                tangent,
                tangent.rotated_left() * k,
            ));
        }
        Ok(out)
    }

    /// Tous les traits de crans de la feuille (ceux dont le calcul échoue sont ignorés).
    pub fn all_notch_marks(&self) -> Vec<(PointMm, PointMm)> {
        self.notches
            .iter()
            .filter_map(|n| self.notch_marks(n).ok())
            .flatten()
            .collect()
    }

    /// Nombre de tracés ayant le rôle « droit-fil ».
    pub fn grainline_count(&self) -> usize {
        self.entities
            .iter()
            .filter(|e| e.role == crate::model::PathRole::Grainline)
            .count()
    }
}

impl Sheet {
    /// Le cran ancré a-t-il encore tous les objets qu'il cite ?
    pub(crate) fn notch_anchor_alive(&self, notch: &Notch) -> bool {
        match &notch.anchor {
            None => true,
            Some(anchor) => {
                anchor.point().is_none_or(|p| self.point(p).is_ok())
                    && match anchor {
                        NotchAnchor::Carried { entity, .. } => self.entity(*entity).is_ok(),
                        _ => true,
                    }
            }
        }
    }
}

impl Project {
    /// Retire les crans ancrés dont un point ou un tracé cité a disparu pendant la
    /// commande, avec une remarque (même règle que les éléments internes et les
    /// marquages). Ne touche que les crans qui existaient, inchangés, avant la commande :
    /// une référence morte introduite par la commande elle-même reste refusée par la
    /// validation.
    pub(crate) fn prune_dead_notch_anchors(&mut self, before: &Project, out: &mut Outcome) {
        for sheet in &mut self.sheets {
            if sheet.notches.iter().all(|n| n.anchor.is_none()) {
                continue;
            }
            let old = before.sheet(sheet.id).ok();
            let dead: Vec<(NotchId, String)> = sheet
                .notches
                .iter()
                .filter(|n| {
                    n.anchor.is_some()
                        && old.is_some_and(|o| o.notch(n.id) == Ok(n))
                        && !sheet.notch_anchor_alive(n)
                })
                .map(|n| (n.id, sheet.entity_name(n.entity)))
                .collect();
            for (id, name) in dead {
                sheet.notches.retain(|n| n.id != id);
                out.notes.push(format!(
                    "Un cran de {name} était ancré à un point ou à un tracé supprimé : il est \
                     retiré."
                ));
            }
        }
    }

    /// Les crans ancrés qui ne suivraient pas l'opération (tracé coupé, fusionné, raccourci,
    /// prolongé, ou source d'un report) deviennent des crans à distance fixe, à la
    /// position qu'ils ont **avant** l'opération, avec une remarque : jamais de saut
    /// silencieux. À appeler avant de modifier la géométrie.
    pub(crate) fn release_notch_anchors(
        &mut self,
        sheet: SheetId,
        entities: &[EntityId],
        out: &mut Outcome,
    ) -> Result<(), CoreError> {
        let ids: Vec<NotchId> = self
            .sheet(sheet)?
            .notches
            .iter()
            .filter(|n| {
                entities.contains(&n.entity)
                    || matches!(n.anchor, Some(NotchAnchor::Carried { entity, .. })
                        if entities.contains(&entity))
            })
            .map(|n| n.id)
            .collect();
        self.release_notch_anchors_of(sheet, &ids, out)
    }

    /// Comme [`Project::release_notch_anchors`], pour des crans précis.
    pub(crate) fn release_notch_anchors_of(
        &mut self,
        sheet: SheetId,
        notches: &[NotchId],
        out: &mut Outcome,
    ) -> Result<(), CoreError> {
        let s = self.sheet(sheet)?;
        let mut fixed = Vec::new();
        for n in s
            .notches
            .iter()
            .filter(|n| notches.contains(&n.id) && n.anchor.is_some())
        {
            fixed.push((s.notch_fixed(n)?, s.entity_name(n.entity)));
        }
        if fixed.is_empty() {
            return Ok(());
        }
        let s = self.sheet_mut(sheet)?;
        for (nf, name) in fixed {
            out.notes.push(format!(
                "Le cran ancré de {name} devient un cran à distance fixe ({} mm) : le tracé \
                 change d'une façon que l'ancrage ne suit pas.",
                crate::input::format_mm(nf.distance_mm)
            ));
            if let Some(n) = s.notches.iter_mut().find(|n| n.id == nf.id) {
                *n = nf;
            }
        }
        Ok(())
    }
}

/// Refus de la combinaison cran en coin + cran sur la coupe (reportée, US-06.1b).
pub(crate) const CORNER_ON_CUT_REFUSED: &str =
    "cran en coin : la combinaison avec « sur la coupe » n'est pas prise en charge (reportée) : \
     un cran est soit en coin, soit sur la coupe";

/// Largeur d'un cran en T, V, U ou fente quand elle n'est pas précisée (mm).
pub const DEFAULT_NOTCH_WIDTH_MM: f64 = 3.0;
/// Largeur minimale et maximale acceptées (mm).
pub const MIN_NOTCH_WIDTH_MM: f64 = 0.1;
pub const MAX_NOTCH_WIDTH_MM: f64 = 100.0;
/// Segments du fond demi-circulaire d'un cran en U.
const U_ARC_SEGMENTS: usize = 12;
/// Recul (mm) du départ du rayon de projection, vers l'intérieur de la pièce : le rayon
/// traverse ainsi la coupe même quand elle passe exactement par le point de couture (marge 0).
const RAY_BACK_MM: f64 = 0.01;
/// Distance (mm) du test « côté intérieur » d'une pièce (polygone du contour de couture).
const SIDE_PROBE_MM: f64 = 0.5;

impl Notch {
    /// Largeur effective (mm) : celle qui est réglée, sinon la largeur par défaut.
    pub fn effective_width_mm(&self) -> f64 {
        self.width_mm.unwrap_or(DEFAULT_NOTCH_WIDTH_MM)
    }
}

/// Décalages (mm) des traits d'un cran le long du bord : un trait, ou deux pour un cran double.
fn notch_offsets(style: NotchStyle) -> &'static [f64] {
    match style {
        NotchStyle::Single => &[0.0],
        NotchStyle::Double => &[-DOUBLE_NOTCH_GAP_MM / 2.0, DOUBLE_NOTCH_GAP_MM / 2.0],
    }
}

/// Contrôle de la forme d'un cran (validation du projet et des commandes) : la largeur ne
/// s'applique qu'aux formes T, V, U et fente, elle est bornée, et un U est au plus aussi large
/// que le double de sa profondeur (le fond demi-circulaire doit tenir).
pub(crate) fn check_notch_shape(n: &Notch) -> Result<(), CoreError> {
    if n.corner && n.on_cut {
        return Err(CoreError::Invalid(CORNER_ON_CUT_REFUSED.into()));
    }
    if n.proportional && n.anchor.is_some() {
        return Err(CoreError::Invalid(
            "cran : « proportional » ne s'applique qu'à un cran à distance fixe (un cran ancré              suit déjà son bord)"
                .into(),
        ));
    }
    if let Some(w) = n.width_mm {
        if n.form.is_line() {
            return Err(CoreError::Invalid(
                "cran : la largeur ne s'applique qu'aux formes T, V, U et fente".into(),
            ));
        }
        if !w.is_finite() || !(MIN_NOTCH_WIDTH_MM..=MAX_NOTCH_WIDTH_MM).contains(&w) {
            return Err(CoreError::Invalid(format!(
                "cran : la largeur doit être comprise entre {MIN_NOTCH_WIDTH_MM} et \
                 {MAX_NOTCH_WIDTH_MM} mm"
            )));
        }
    }
    if n.form == NotchForm::U && n.effective_width_mm() > 2.0 * n.length_mm + 1e-9 {
        return Err(CoreError::Invalid(format!(
            "cran en U : la largeur ({}) ne peut pas dépasser le double de la profondeur ({})",
            crate::input::format_mm(n.effective_width_mm()),
            crate::input::format_mm(n.length_mm)
        )));
    }
    Ok(())
}

/// Polylignes d'un cran dans son repère : `foot` = pied (sur le bord), `along` = axe de la
/// largeur (unitaire, le long du bord), `into` = direction de la profondeur (unitaire, du pied
/// vers la pointe). `depth` et `width` en mm.
pub fn notch_shape_polylines(
    form: NotchForm,
    foot: PointMm,
    along: VectorMm,
    into: VectorMm,
    depth: f64,
    width: f64,
) -> Vec<Vec<PointMm>> {
    let at = |a: f64, b: f64| foot + along * a + into * b;
    let h = width / 2.0;
    match form {
        NotchForm::Line => vec![vec![foot, foot + into * depth]],
        NotchForm::T => vec![
            vec![foot, foot + into * depth],
            vec![at(-h, depth), at(h, depth)],
        ],
        NotchForm::V => vec![vec![at(-h, 0.0), at(0.0, depth), at(h, 0.0)]],
        NotchForm::U => {
            let c = depth - h;
            let mut line = vec![at(-h, 0.0)];
            for k in 0..=U_ARC_SEGMENTS {
                let theta = std::f64::consts::PI * (1.0 - k as f64 / U_ARC_SEGMENTS as f64);
                line.push(at(h * theta.cos(), c + h * theta.sin()));
            }
            line.push(at(h, 0.0));
            vec![line]
        }
        NotchForm::Slot => vec![vec![at(-h, 0.0), at(-h, depth), at(h, depth), at(h, 0.0)]],
    }
}

/// Segments d'un cran (voir [`notch_shape_polylines`]), pris sur sa forme et ses dimensions.
fn shape_marks(
    notch: &Notch,
    foot: PointMm,
    along: VectorMm,
    into: VectorMm,
) -> Vec<(PointMm, PointMm)> {
    notch_shape_polylines(
        notch.form,
        foot,
        along,
        into,
        notch.length_mm,
        notch.effective_width_mm(),
    )
    .iter()
    .flat_map(|line| line.windows(2).map(|w| (w[0], w[1])))
    .collect()
}

/// Écart (mm) en deçà duquel une position est « au bout » de son tracé et deux points sont
/// confondus (cran en coin).
const CORNER_TOL_MM: f64 = 1e-6;

/// Sommet d'un cran en coin et direction de son trait (US-06.1b), **lus** d'après le contour de
/// la pièce : rien n'est stocké hors de la définition (`Notch::corner`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NotchCorner {
    pub piece: PieceId,
    /// Sommet du contour (pied du cran).
    pub vertex: PointMm,
    /// Bissectrice **unitaire** de l'angle du contour au sommet, vers l'extérieur de la pièce
    /// (somme des normales extérieures des deux tracés adjacents : sommet saillant, rentrant ou
    /// lisse).
    pub outward: VectorMm,
    /// Direction unitaire du trait, du sommet vers la pointe : `outward` si le côté du cran est
    /// l'extérieur de la pièce, son opposée si c'est l'intérieur (même convention de côté qu'un
    /// cran ordinaire).
    pub direction: VectorMm,
    /// Angle **intérieur** du contour au sommet (degrés, 180 = sommet lisse).
    pub interior_angle_deg: f64,
}

impl Project {
    /// Sommet et direction d'un cran en coin (US-06.1b). Le cran doit être posé au début ou à la
    /// fin de son tracé, dans le contour d'une pièce ; la direction est la bissectrice de l'angle
    /// formé, au sommet, par le tracé et son voisin dans le contour. Erreur expliquée (jamais un
    /// panic) si le cran n'est pas à un sommet identifiable ; l'affichage et les exports se
    /// replient alors sur la normale du tracé (`PieceIssue::CornerNotchesFailed`).
    pub fn notch_corner(&self, sheet: SheetId, notch: NotchId) -> Result<NotchCorner, CoreError> {
        let s = self.sheet(sheet)?;
        self.corner_of(s, s.notch(notch)?)
    }

    pub(crate) fn corner_of(&self, s: &Sheet, n: &Notch) -> Result<NotchCorner, CoreError> {
        let name = s.entity_name(n.entity);
        let fail = |why: &str| CoreError::Invalid(format!("cran en coin sur {name} : {why}"));
        let piece = self
            .notch_piece(s.id, n)
            .ok_or_else(|| fail("le tracé n'est dans le contour d'aucune pièce de la feuille"))?;
        let len = s.path_length_mm(n.entity)?;
        let at = s.notch_distance_mm(n)?;
        let at_start = at <= CORNER_TOL_MM;
        let at_end = at >= len - CORNER_TOL_MM;
        if at_start == at_end {
            return Err(fail(
                "le cran n'est pas à une extrémité de son tracé (ce n'est pas un sommet)",
            ));
        }
        let chain = self
            .piece_chain_any(piece.id)
            .map_err(|e| fail(&e.to_string()))?;
        let count = chain.len();
        let is = |i: usize| chain[i].source == n.entity;
        let starts: Vec<usize> = (0..count)
            .filter(|&i| is(i) && !is((i + count - 1) % count))
            .collect();
        let ends: Vec<usize> = (0..count)
            .filter(|&i| is(i) && !is((i + 1) % count))
            .collect();
        let ([i0], [i1]) = (starts.as_slice(), ends.as_slice()) else {
            return Err(fail(
                "le tracé n'est pas une suite continue du contour entre deux sommets",
            ));
        };
        let (i0, i1) = (*i0, *i1);
        let near = |a: PointMm, b: PointMm| (a - b).norm() <= CORNER_TOL_MM;
        let path_start = s.path_point_at(n.entity, 0.0)?.0;
        // Sens du tracé dans le contour (anti-horaire) : même sens que le tracé ou inverse.
        let forward = near(chain[i0].geom.start(), path_start);
        if !forward && !near(chain[i1].geom.end(), path_start) {
            return Err(fail(
                "le tracé ne se retrouve pas dans le contour de la pièce",
            ));
        }
        let at_block_start = forward == at_start;
        let (incoming, outgoing) = if at_block_start {
            (&chain[(i0 + count - 1) % count], &chain[i0])
        } else {
            (&chain[i1], &chain[(i1 + 1) % count])
        };
        let vertex = outgoing.geom.start();
        if !near(incoming.geom.end(), vertex) {
            return Err(fail(
                "les deux tracés adjacents ne se rejoignent pas au sommet",
            ));
        }
        let (Some(d_in), Some(d_out)) =
            (incoming.geom.end_tangent(), outgoing.geom.start_tangent())
        else {
            return Err(fail("un des tracés adjacents est dégénéré"));
        };
        // Contour anti-horaire : l'extérieur est à droite du parcours. Bissectrice extérieure =
        // somme des deux normales extérieures (opposé de celle des normales gauches).
        let (a, b) = (d_in.rotated_left(), d_out.rotated_left());
        let outward = VectorMm::new(-(a.x + b.x), -(a.y + b.y))
            .unit()
            .map_err(|_| fail("angle nul : les deux tracés reviennent l'un sur l'autre"))?;
        // Le côté gauche d'un tracé parcouru dans le sens du contour (anti-horaire) est l'intérieur.
        let side_inward = (n.side == Side::Left) == forward;
        let direction = if side_inward { outward * -1.0 } else { outward };
        let turn = d_in.cross(d_out).atan2(d_in.dot(d_out));
        Ok(NotchCorner {
            piece: piece.id,
            vertex,
            outward,
            direction,
            interior_angle_deg: 180.0 - turn.to_degrees(),
        })
    }

    /// Traits d'un cran en coin : la forme du cran posée au sommet, dirigée selon la bissectrice.
    /// Repli sur les traits d'un cran ordinaire (normale du tracé) si le sommet n'est pas
    /// identifiable : jamais un refus (voir `PieceIssue::CornerNotchesFailed`).
    fn corner_marks(&self, s: &Sheet, n: &Notch) -> Result<Vec<(PointMm, PointMm)>, CoreError> {
        let Ok(c) = self.corner_of(s, n) else {
            return s.notch_seam_marks(n);
        };
        let k = match n.side {
            Side::Left => 1.0,
            Side::Right => -1.0,
        };
        // Même repère qu'un cran ordinaire : `into = gauche(along) * k`.
        let along = c.direction.rotated_left() * -k;
        let mut out = Vec::new();
        for o in notch_offsets(n.style) {
            out.extend(shape_marks(n, c.vertex + along * *o, along, c.direction));
        }
        Ok(out)
    }

    /// Première raison pour laquelle un cran en coin de la pièce n'a pas de sommet identifiable
    /// (contrôle `PieceIssue::CornerNotchesFailed`), ou `None`.
    pub(crate) fn piece_corner_notch_failure(&self, piece: PieceId) -> Option<String> {
        let sheet = self.piece_sheet(piece).ok()?;
        sheet
            .notches
            .iter()
            .filter(|n| n.corner && self.notch_piece(sheet.id, n).is_some_and(|p| p.id == piece))
            .find_map(|n| self.corner_of(sheet, n).err().map(|e| e.to_string()))
    }
}

/// Où un cran est exporté : sur le contour de couture, ou sur le contour de coupe calculé.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotchPlace {
    Seam,
    Cut,
}

/// Position **calculée** d'un cran sur le contour de coupe (US-06.1) : jamais stockée, elle suit
/// la marge de couture et la construction.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NotchOnCut {
    pub piece: PieceId,
    /// Point du cran sur la couture (pied de la perpendiculaire).
    pub seam_point: PointMm,
    /// Pied du cran sur la coupe : intersection de la perpendiculaire à la couture avec le
    /// contour de coupe.
    pub foot: PointMm,
    /// Tangente unitaire de la couture au point (axe de la largeur du cran).
    pub along: VectorMm,
    /// Direction unitaire de la couture vers la coupe (extérieur de la pièce).
    pub outward: VectorMm,
    /// Distance couture → coupe mesurée sur la perpendiculaire (mm).
    pub reach_mm: f64,
}

/// Contour de coupe et polygone de couture d'une pièce, calculés une fois pour les projections.
struct CutFrame {
    piece: PieceId,
    cut: CutContour,
    seam_polygon: Vec<PointMm>,
}

fn no_piece() -> CoreError {
    CoreError::Invalid(
        "cran sur la coupe : le tracé du cran n'est dans le contour d'aucune pièce de la feuille"
            .into(),
    )
}

impl Project {
    /// Pièce de la feuille dont le contour contient le tracé du cran (la première, dans l'ordre
    /// du projet), ou `None` : le cran ne peut pas être « sur la coupe ».
    pub fn notch_piece(&self, sheet: SheetId, notch: &Notch) -> Option<&Piece> {
        self.pieces_on(sheet)
            .into_iter()
            .find(|p| p.outline.iter().any(|o| o.entity == notch.entity))
    }

    fn cut_frame(&self, piece: PieceId) -> Result<CutFrame, CoreError> {
        let cut = self.piece_cut_contour(piece)?;
        let chain = self.piece_chain_any(piece)?;
        let mut seam_polygon = Vec::new();
        for e in &chain {
            let mut pts = pf_geometry::split::edge_sample(&e.geom, 256);
            pts.pop();
            seam_polygon.extend(pts);
        }
        Ok(CutFrame {
            piece,
            cut,
            seam_polygon,
        })
    }

    /// Projette le point du bord `entity` situé à `distance_mm` de son départ sur le contour de
    /// coupe, selon la perpendiculaire à la couture, vers l'extérieur de la pièce.
    fn project_on_cut(
        &self,
        sheet: &Sheet,
        frame: &CutFrame,
        entity: EntityId,
        distance_mm: f64,
    ) -> Result<NotchOnCut, CoreError> {
        let fail = |why: &str| {
            CoreError::Invalid(format!(
                "cran sur la coupe de {} : {why}",
                sheet.entity_name(entity)
            ))
        };
        let (p, tangent) = sheet.path_point_at(entity, distance_mm)?;
        let left = tangent.rotated_left();
        let back = left * -1.0;
        let inside = |dir: VectorMm| {
            pf_geometry::point_in_polygon(p + dir * SIDE_PROBE_MM, &frame.seam_polygon)
        };
        let outward = match (inside(left), inside(back)) {
            (true, false) => back,
            (false, true) => left,
            _ => {
                return Err(fail(
                    "le côté extérieur de la pièce n'est pas déterminable ici",
                ))
            }
        };
        let start = p + outward * -RAY_BACK_MM;
        let end = p + outward * (frame.cut.perimeter_mm + 2.0 * crate::piece_seam::MAX_SEAM_MM);
        let ray = Trace::Line(start, end);
        let mut best: Option<(f64, PointMm)> = None;
        for trace in &frame.cut.traces {
            let hits = pf_geometry::intersect(&ray, trace)
                .map_err(|e| fail(&format!("intersection impossible ({e})")))?;
            for h in hits {
                let s = (h.point - p).dot(outward);
                if s >= -1e-6 && best.is_none_or(|(b, _)| s < b) {
                    best = Some((s, h.point));
                }
            }
        }
        let Some((reach, foot)) = best else {
            return Err(fail(
                "la perpendiculaire à la couture ne rencontre pas le contour de coupe",
            ));
        };
        Ok(NotchOnCut {
            piece: frame.piece,
            seam_point: p,
            foot,
            along: tangent,
            outward,
            reach_mm: reach.max(0.0),
        })
    }

    /// Position **lue** d'un cran sur le contour de coupe de sa pièce (US-06.1) : le point de
    /// couture du cran (`Sheet::notch_distance_mm`) est projeté perpendiculairement à la couture,
    /// vers l'extérieur, sur le contour de coupe calculé (`piece_cut_contour`). Recalculée à chaque
    /// appel, elle suit donc la marge. Erreur expliquée (jamais un panic) si le tracé n'est dans
    /// aucune pièce, si le contour de coupe ne se calcule pas, ou si la perpendiculaire ne le
    /// rencontre pas.
    pub fn notch_on_cut(&self, sheet: SheetId, notch: NotchId) -> Result<NotchOnCut, CoreError> {
        let s = self.sheet(sheet)?;
        let n = s.notch(notch)?;
        let piece = self.notch_piece(sheet, n).ok_or_else(no_piece)?;
        let frame = self.cut_frame(piece.id)?;
        self.project_on_cut(s, &frame, n.entity, s.notch_distance_mm(n)?)
    }

    /// Traits d'un cran « sur la coupe » : la forme du cran posée sur le contour de coupe, du
    /// pied (sur la coupe) vers la couture (la pointe est à `length_mm` du pied, vers l'intérieur
    /// de la pièce). Un cran double donne deux traits, chacun projeté à son propre point.
    pub fn notch_on_cut_marks(
        &self,
        sheet: SheetId,
        notch: NotchId,
    ) -> Result<Vec<(PointMm, PointMm)>, CoreError> {
        let s = self.sheet(sheet)?;
        let n = s.notch(notch)?;
        let piece = self.notch_piece(sheet, n).ok_or_else(no_piece)?;
        let frame = self.cut_frame(piece.id)?;
        let len = s.path_length_mm(n.entity)?;
        let at = s.notch_distance_mm(n)?;
        let mut out = Vec::new();
        for o in notch_offsets(n.style) {
            let proj = self.project_on_cut(s, &frame, n.entity, (at + o).clamp(0.0, len))?;
            out.extend(shape_marks(n, proj.foot, proj.along, proj.outward * -1.0));
        }
        Ok(out)
    }

    /// Première raison pour laquelle un cran « sur la coupe » de la pièce ne se projette pas
    /// (contrôle `PieceIssue::CutNotchesFailed`), ou `None` si tous se projettent.
    pub(crate) fn piece_cut_notch_failure(&self, piece: PieceId) -> Option<String> {
        let sheet = self.piece_sheet(piece).ok()?;
        sheet
            .notches
            .iter()
            .filter(|n| n.on_cut && self.notch_piece(sheet.id, n).is_some_and(|p| p.id == piece))
            .find_map(|n| {
                self.notch_on_cut_marks(sheet.id, n.id)
                    .err()
                    .map(|e| e.to_string())
            })
    }

    /// Traits d'un cran là où il est exporté et dessiné : sur la couture pour un cran ordinaire
    /// (ou un cran « sur la coupe » qui n'est dans aucune pièce : repli) ; pour un cran « sur la
    /// coupe » d'une pièce, la projection sur la coupe, de lieu `Cut` si la pièce a une marge de
    /// couture (son contour de coupe est alors un élément à part), `Seam` sinon (la coupe est la
    /// couture).
    pub fn notch_placed_marks(
        &self,
        sheet: SheetId,
        notch: &Notch,
    ) -> Result<(NotchPlace, Vec<(PointMm, PointMm)>), CoreError> {
        let s = self.sheet(sheet)?;
        if !notch.on_cut {
            if notch.corner {
                return Ok((NotchPlace::Seam, self.corner_marks(s, notch)?));
            }
            return Ok((NotchPlace::Seam, s.notch_marks(notch)?));
        }
        let Some(piece) = self.notch_piece(sheet, notch) else {
            return Ok((NotchPlace::Seam, s.notch_seam_marks(notch)?));
        };
        let place = if piece.seam.is_some() && !piece.net_cut {
            NotchPlace::Cut
        } else {
            NotchPlace::Seam
        };
        Ok((place, self.notch_on_cut_marks(sheet, notch.id)?))
    }
}
