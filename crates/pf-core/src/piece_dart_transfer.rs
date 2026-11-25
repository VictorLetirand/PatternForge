//! Transfert d'une pince (US-07.2, décision 298).
//!
//! `Command::TransferPieceDart` pivote la pince d'une pièce autour de son **sommet** : la portion
//! du contour comprise entre la base d'origine et la **cible** (un point d'un autre bord : tracé
//! et distance depuis son début) tourne autour du sommet d'un angle égal à `part` fois l'angle
//! d'ouverture ; la base d'origine se ferme (en partie si `part < 1`), une base s'ouvre à la
//! cible. La somme des angles de pince est conservée, les bords hors pince gardent leur longueur.
//!
//! Le calcul est dans `pf_geometry::dart::transfer_dart`. Ici, **la pièce est reconstruite** sur
//! une feuille dédiée (comme Séparer, US-04.6) : la construction d'origine n'est jamais modifiée
//! (sûr, annulable en une étape), la pièce garde son identifiant, ses pinces sont recréées sur les
//! nouveaux points. Les pièces dérivées et les surpiqûres liées à la pièce sont régénérées dans la
//! même étape par `Project::apply` (obsolètes, jamais un refus). Le droit-fil, les éléments
//! internes, marquages et boutons suivent la partie qu'ils occupent (tournés ou non) ; ceux qui
//! traversent la ligne de pivot sont retirés avec une remarque.

use crate::command::Outcome;
use crate::error::CoreError;
use crate::model::*;
use crate::offset::trace_of;
use crate::piece_dart::PieceDart;
use crate::piece_split_join::{geoms_of, ChainEdge};
use pf_geometry::dart::transfer_dart;
use pf_geometry::region::{trace_geometry, trace_nearest_param, EdgeGeom};
use pf_geometry::split::{edge_portion, edge_sample, edge_trace};
use pf_geometry::{distance_mm, Lasso, PointMm, Side};

fn invalid(msg: impl Into<String>) -> CoreError {
    CoreError::Invalid(msg.into())
}

/// Côté (gauche ou droite de l'axe sommet → milieu de base) où se trouve `foot`.
fn side_of(apex: PointMm, mid: PointMm, foot: PointMm) -> Side {
    if (mid - apex).cross(foot - apex) > 0.0 {
        Side::Left
    } else {
        Side::Right
    }
}

fn middle(a: PointMm, b: PointMm) -> PointMm {
    PointMm::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0)
}

impl Project {
    /// Tracé du contour de la pièce le plus proche de `near` et distance de son début à ce point
    /// (dans le sens du tracé) : aide à la saisie de la cible d'un transfert. `None` si le contour
    /// est illisible. Lecture pure.
    pub fn dart_target_at(&self, piece: PieceId, near: PointMm) -> Option<(EntityId, f64)> {
        let p = self.piece(piece).ok()?;
        let sheet = self.piece_sheet(piece).ok()?;
        let mut best: Option<(EntityId, f64, f64)> = None;
        for e in &p.outline {
            let geoms = trace_geometry(&trace_of(sheet.shape(e.entity).ok()?, false));
            let mut before = 0.0;
            for g in &geoms {
                let (t, dist) = trace_nearest_param(&edge_trace(g), near);
                if best.is_none_or(|b| dist < b.2) {
                    best = Some((e.entity, before + edge_portion(g, 0.0, t).length_mm(), dist));
                }
                before += g.length_mm();
            }
        }
        best.map(|(e, d, _)| (e, d))
    }

    /// Transfère la pince `dart` de `piece` vers le point du tracé `edge` situé à `distance_mm`
    /// de son début ; `part` ∈ ]0 ; 1] = part de l'angle d'ouverture transférée (1 = totale).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn transfer_piece_dart(
        &mut self,
        piece: PieceId,
        dart: DartId,
        edge: EntityId,
        distance_mm: f64,
        part: f64,
        out: &mut Outcome,
    ) -> Result<(), CoreError> {
        let orig = self.piece(piece)?.clone();
        let index = self
            .pieces
            .iter()
            .position(|p| p.id == piece)
            .ok_or(CoreError::UnknownPiece(piece))?;
        if !part.is_finite() || part <= 0.0 || part > 1.0 {
            return Err(invalid(format!(
                "la part transférée ({part}) doit être un nombre fini supérieur à 0 et au plus 1"
            )));
        }
        let d = *orig
            .darts
            .iter()
            .find(|d| d.id == dart)
            .ok_or_else(|| invalid(format!("la pince {} n'existe pas", dart.0)))?;
        if orig.darts.len() != 1 {
            return Err(invalid(format!(
                "la pièce « {} » porte {} pinces : le transfert ne reprend pas les autres pinces, \
                 supprimez-les d'abord",
                orig.name,
                orig.darts.len()
            )));
        }
        let chain = self.piece_chain(piece)?;
        let calc = self
            .dart_calc(&orig, &d)
            .map_err(|why| invalid(format!("la pince ne peut pas être transférée : {why}")))?;
        // Cible : point du tracé à la distance donnée de son début.
        if !orig.outline.iter().any(|e| e.entity == edge) {
            return Err(invalid(format!(
                "le tracé {} n'est pas un bord du contour de la pièce « {} »",
                edge.0, orig.name
            )));
        }
        let shape = self.piece_sheet(piece)?.shape(edge)?;
        let len = shape.length_mm()?;
        if !distance_mm.is_finite() || distance_mm < 0.0 || distance_mm > len {
            return Err(invalid(format!(
                "la distance de la cible ({distance_mm} mm) doit être comprise entre 0 et la \
                 longueur du tracé ({len:.3} mm)"
            )));
        }
        let target = shape.point_at_length(distance_mm)?;
        // Contour tourné pour que la ligne qui porte la base soit en tête.
        let n = chain.len();
        let carrier = calc.carrier;
        let start = (0..n)
            .find(|&i| chain[i].source == carrier && chain[(i + n - 1) % n].source != carrier)
            .unwrap_or(0);
        let rotated: Vec<ChainEdge> = (0..n).map(|k| chain[(start + k) % n].clone()).collect();
        let line_len = rotated.iter().take_while(|e| e.source == carrier).count();
        let tr = transfer_dart(
            &geoms_of(&rotated),
            line_len,
            &calc.shape,
            d.fixed,
            target,
            part,
        )
        .map_err(|e| invalid(e.to_string()))?;

        // Reports (droit-fil, internes, marquages, boutons) : par région.
        let mut notes = Vec::new();
        let carry = self.carry_of_with(piece, &mut notes, false)?;
        let mut poly = vec![tr.apex];
        for g in &tr.moving_arc {
            poly.extend(edge_sample(g, 48));
        }
        let region = Lasso::new(&poly)
            .ok_or_else(|| invalid("la région à tourner est dégénérée : transfert impossible"))?;
        let carry = carry.turned_in(&region, (tr.apex, tr.cos, tr.sin), &orig.name, &mut notes);

        // Nouveau contour sur une feuille dédiée.
        let m = tr.edges.len();
        let edges: Vec<(EdgeGeom, PathRole)> = (0..m)
            .map(|i| {
                let role = match tr.edges[i].source {
                    Some(s) => rotated[s].role,
                    None => (1..=m)
                        .filter_map(|k| tr.edges[(i + m - k) % m].source)
                        .next()
                        .map_or_else(PathRole::default, |s| rotated[s].role),
                };
                (tr.edges[i].geom, role)
            })
            .collect();
        let joints: Vec<PointMm> = edges.iter().map(|(g, _)| g.start()).collect();
        let sid = self.new_result_sheet(&orig.name);
        let outline = self.build_closed_chain(sid, &edges, &joints)?;
        let (grainline, internal, markings, buttons, grain) = self.carry_into(sid, &carry, true)?;

        // Pinces : celle de la cible garde l'identifiant ; la pince qui reste (partiel) en reçoit un.
        let apex_id = self
            .add_free_point(sid, tr.apex)
            .map_err(CoreError::Invalid)?;
        let make = |this: &mut Project,
                    id: DartId,
                    fixed_foot: PointMm,
                    moving_foot: PointMm|
         -> Result<PieceDart, CoreError> {
            let mid = middle(fixed_foot, moving_foot);
            let toward = this.add_free_point(sid, mid).map_err(CoreError::Invalid)?;
            Ok(PieceDart {
                id,
                apex: apex_id,
                toward,
                length_mm: dist(tr.apex, mid),
                opening_mm: dist(fixed_foot, moving_foot),
                fixed: side_of(tr.apex, mid, fixed_foot),
                // Le réglage suit la pince transférée (et la pince restante d'un transfert partiel).
                cut_closed: d.cut_closed,
            })
        };
        let mut darts = vec![make(self, d.id, tr.new_fixed_foot, tr.new_moving_foot)?];
        let mut remaining_id = None;
        if let Some((f_fixed, f_moved)) = tr.remaining {
            let id = DartId(self.alloc_id());
            remaining_id = Some(id);
            darts.push(make(self, id, f_fixed, f_moved)?);
        }
        let mut seam_notes = Vec::new();
        let seam = crate::piece_seam::carried_seam(&orig, &mut seam_notes);
        notes.extend(seam_notes);
        let new_piece = Piece {
            outline,
            grainline,
            grain,
            seam,
            // Les tracés changent : la déclaration « sur le pli » n'est pas reprise (remarque).
            on_fold: Vec::new(),
            internal,
            markings,
            darts,
            buttons,
            link: PieceLink::Detached,
            ..orig.clone()
        };
        self.pieces[index] = new_piece;

        // Contrôles du résultat (refus expliqué : jamais un contour ou une pince invalides).
        self.piece_chain_any(piece).map_err(|e| {
            invalid(format!(
                "le transfert produit un contour inutilisable : {e}"
            ))
        })?;
        let p = self.piece(piece)?.clone();
        for (k, nd) in p.darts.iter().enumerate() {
            self.dart_calc(&p, nd).map_err(|why| {
                invalid(if k == 0 {
                    format!("la pince obtenue à la cible n'est pas valable : {why}")
                } else {
                    format!(
                        "la pince qui reste à l'emplacement d'origine n'est pas valable : {why}"
                    )
                })
            })?;
        }
        out.pieces.push(piece);
        out.sheet = Some(sid);
        out.darts.push(d.id);
        out.darts.extend(remaining_id);
        notes.insert(
            0,
            format!(
                "La pince de la pièce « {} » est transférée ({:.4}° de rotation) ; la pièce est \
                 reconstruite sur une nouvelle feuille, la construction d'origine n'est pas \
                 modifiée.",
                orig.name,
                tr.rotation_deg.abs()
            ),
        );
        out.notes.extend(notes);
        Ok(())
    }
}

fn dist(a: PointMm, b: PointMm) -> f64 {
    distance_mm(a, b)
}
