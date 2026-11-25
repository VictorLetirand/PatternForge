//! Pièces (US-04.1) : région fermée sous le curseur, contour orienté, aire et
//! périmètre. Tout est **lu** et recalculé à la demande : rien n'est stocké dans
//! le projet (une pièce liée suit donc la construction).

use crate::contour::{contour_from_entities, measure_contour, Contour, ContourEdge};
use crate::derived::DerivedInfo;
use crate::error::CoreError;
use crate::model::{
    Entity, EntityId, Piece, PieceCategory, PieceEdge, PieceId, PieceLink, Project, Sheet, SheetId,
};
use crate::piece_internal::InternalCounts;
use pf_geometry::{find_region, PointMm, Region, RegionError, Trace};

/// Région fermée trouvée sur une feuille : le résultat géométrique et la
/// correspondance entre les indices de tracés qu'il utilise et les tracés de la feuille.
#[derive(Debug, Clone, PartialEq)]
pub struct SheetRegion {
    pub region: Region,
    /// `entities[i]` est le tracé d'indice `i` dans `region.edges[..].source`.
    pub entities: Vec<EntityId>,
}

impl Sheet {
    /// Tracés pris en compte pour détecter une région : ceux des calques
    /// visibles (un calque masqué ne délimite rien), dans l'ordre de la feuille.
    pub fn region_traces(&self) -> (Vec<EntityId>, Vec<Trace>) {
        let mut ids = Vec::new();
        let mut traces = Vec::new();
        for e in self
            .entities
            .iter()
            .filter(|e: &&Entity| self.layer_visible(e.layer))
        {
            if let Ok(shape) = self.shape(e.id) {
                ids.push(e.id);
                traces.push(shape.to_trace());
            }
        }
        (ids, traces)
    }

    /// Plus petite région fermée contenant `click` (voir `pf_geometry::find_region`).
    pub fn find_region_at(&self, click: PointMm) -> Result<SheetRegion, RegionError> {
        let (entities, traces) = self.region_traces();
        let region = find_region(&traces, click)?;
        Ok(SheetRegion { region, entities })
    }
}

/// Contour d'une pièce, mesuré : valeurs **observées**, recalculées à chaque appel.
#[derive(Debug, Clone, PartialEq)]
pub struct PieceContour {
    /// Arêtes dans l'ordre de la pièce.
    pub edges: Vec<PieceEdge>,
    pub perimeter_mm: f64,
    /// Aire en mm², toujours positive.
    pub area_mm2: f64,
    /// Sens de parcours du contour tel qu'enregistré (une pièce extraite est anti-horaire).
    pub clockwise: bool,
    /// Les tracés forment une chaîne fermée simple (points communs).
    pub closed: bool,
    /// Le contour se recoupe lui-même : l'aire n'a alors pas de sens métier.
    pub self_intersecting: bool,
}

impl Project {
    /// Pièces dont le contour est sur cette feuille : les pièces liées à la feuille et
    /// les pièces détachées dont la copie y est (ordre du projet).
    pub fn pieces_on(&self, sheet: SheetId) -> Vec<&Piece> {
        let here = self.sheet(sheet).ok();
        self.pieces
            .iter()
            .filter(|p| match p.link {
                PieceLink::Linked { sheet: s } => s == sheet,
                PieceLink::Detached => p
                    .outline
                    .first()
                    .is_some_and(|e| here.is_some_and(|s| s.entity(e.entity).is_ok())),
            })
            .collect()
    }

    /// Feuille qui porte les tracés du contour d'une pièce (la sienne si elle est
    /// liée ; pour une pièce détachée, celle de son premier tracé).
    pub fn piece_sheet(&self, piece: PieceId) -> Result<&Sheet, CoreError> {
        let p = self.piece(piece)?;
        match p.link {
            PieceLink::Linked { sheet } => self.sheet(sheet),
            PieceLink::Detached => p
                .outline
                .first()
                .and_then(|e| self.sheets.iter().find(|s| s.entity(e.entity).is_ok()))
                .ok_or_else(|| CoreError::Invalid("pièce détachée sans contour".into())),
        }
    }

    /// Contour orienté, aire et périmètre d'une pièce, recalculés d'après la
    /// construction actuelle. Erreur expliquée si le contour est vide ou illisible.
    pub fn piece_contour(&self, piece: PieceId) -> Result<PieceContour, CoreError> {
        let p = self.piece(piece)?;
        if p.outline.is_empty() {
            return Err(CoreError::Invalid(format!(
                "la pièce « {} » n'a pas de contour",
                p.name
            )));
        }
        let sheet = self.piece_sheet(piece)?;
        let contour = Contour {
            edges: p
                .outline
                .iter()
                .map(|e| ContourEdge {
                    entity: e.entity,
                    reversed: e.reversed,
                })
                .collect(),
            points: Vec::new(),
            role: None,
        };
        let m = measure_contour(sheet, &contour)?;
        let ids: Vec<EntityId> = p.outline.iter().map(|e| e.entity).collect();
        Ok(PieceContour {
            edges: p.outline.clone(),
            perimeter_mm: m.perimeter_mm,
            area_mm2: m.area_mm2,
            clockwise: m.clockwise,
            closed: contour_from_entities(sheet, &ids).is_ok(),
            self_intersecting: m.self_intersecting,
        })
    }
}

/// Point d'attention sur une pièce (US-04.2). Ce n'est **pas** une erreur de
/// validation : la pièce reste valide et sauvegardable, l'interface l'affiche
/// comme avertissement. Les libellés traduits sont dans l'interface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PieceIssue {
    /// Le nom est vide (impossible via les commandes ; projet construit à la main).
    NoName,
    /// Aucun droit-fil n'est assigné (obligatoire pour une pièce de patron).
    MissingGrainline,
    /// Le droit-fil désigne un tracé qui n'existe pas sur la feuille de la pièce.
    GrainlineMissingEntity,
    /// Pièce-étiquette : aucun contour.
    EmptyOutline,
    /// Le contour existe mais ses tracés ne forment pas une chaîne fermée.
    OutlineOpen,
    /// Le contour existe mais ne peut pas être mesuré (tracé illisible ou dégénéré).
    OutlineUnreadable,
    /// Le contour se recoupe lui-même : l'aire n'a pas de sens métier.
    OutlineSelfIntersecting,
    /// Des éléments internes ne sont plus entièrement dans le contour (la
    /// construction a changé depuis l'import) : signalé, jamais refusé.
    InternalOutsideOutline { count: usize },
    /// Des marquages (US-06.3) ont leur centre hors du contour : signalé, jamais
    /// refusé (jugé sur le centre seulement, pas sur le cercle ou la croix entiers).
    MarkingOutsideOutline { count: usize },
    /// Des pinces (US-07.1) ne sont plus calculables (sommet hors de la pièce, pieds hors du
    /// contour…) : signalé, jamais refusé ; les raisons sont dans `Project::piece_darts`.
    DartInvalid { count: usize },
    /// Des boutons ou boutonnières (US-06.4) sont plus près du contour que le recul du
    /// bord demandé par leur groupe, ou le touchent, le traversent ou sortent de la pièce :
    /// signalé, jamais refusé ni corrigé. `count` compte les **éléments**.
    ButtonsTooCloseToEdge { count: usize },
    /// La rotation du placement manuel (plan de pièces, US-04.5) fait dévier le droit-fil de
    /// plus que la tolérance de rotation de la pièce (US-06.5, 0 si absente) : signalé, jamais
    /// refusé. Les valeurs sont dans `Project::piece_grain_rotation`.
    GrainRotationExceeded,
    /// Pièce dérivée obsolète (US-04.4) : la dernière mise à jour d'après la mère a
    /// échoué, la pièce garde sa dernière géométrie valide. Signalé, jamais refusé.
    DerivedStale { reason: String },
    /// Une autre pièce du projet porte exactement le même code (comparaison exacte,
    /// après rognage des espaces).
    DuplicateCode { code: String },
    /// La pièce a une marge de couture (US-05.1) mais son contour de coupe ne peut pas être
    /// calculé (contour ouvert, dégénéré, marge trop grande pour la forme…) : signalé, jamais un
    /// refus de la commande qui l'a causé.
    CutContourFailed { reason: String },
    /// La pièce a un rempli à crans (US-05.3) mais leurs positions ne peuvent pas être calculées
    /// (trop de crans, bord absent du contour…) : signalé, jamais un refus de commande.
    FoldNotchesFailed { reason: String },
    /// La pièce est déclarée en coupe nette (US-05.6) mais porte aussi une marge de couture
    /// (dont `hems` ourlets et `folds` rempliés) : les deux s'excluent. Impossible par les
    /// commandes et refusé à la lecture ; signalé pour un projet construit à la main.
    NetCutWithSeam { hems: usize, folds: usize },
    /// Un cran « sur la coupe » (US-06.1) de la pièce ne peut pas être projeté sur son contour de
    /// coupe (contour illisible, perpendiculaire sans intersection…) : signalé, jamais un refus de
    /// commande ; le cran n'est ni dessiné ni exporté tant que le calcul échoue.
    CutNotchesFailed { reason: String },
    /// Un cran en coin (US-06.1b) de la pièce n'est pas à un sommet identifiable de son contour
    /// (cran hors contour, tracés adjacents absents, angle nul…) : signalé, jamais un refus de
    /// commande ; le cran est alors dessiné et exporté comme un cran ordinaire (normale du tracé).
    CornerNotchesFailed { reason: String },
    /// Une pince de la pièce est réglée en coupe « pince fermée » (PH-5a) mais le calcul est
    /// impossible (pince non calculable, bouche sur un coin, bouches qui se touchent, bord trop
    /// court pour la marge…) : signalé, jamais un refus ; la coupe ordinaire est conservée.
    CutDartFailed { reason: String },
    /// Des bords déclarés « sur le pli » (PH-5b) ne sont plus dans le contour de la pièce (contour
    /// modifié) : la déclaration reste mais n'a plus d'effet ; signalé, jamais un refus.
    FoldOutsideContour { count: usize },
    /// Des bords sur le pli (PH-5b) portent aussi une marge réglée (surcharge, ourlet, rempli) : le
    /// réglage est **ignoré** tant que le bord est sur le pli (il n'est pas effacé).
    FoldMarginIgnored { count: usize },
}

impl PieceIssue {
    /// Identifiant stable (E12, MCP) ; ne change pas avec la langue.
    pub fn key(&self) -> &'static str {
        match self {
            PieceIssue::NoName => "no_name",
            PieceIssue::MissingGrainline => "missing_grainline",
            PieceIssue::GrainlineMissingEntity => "grainline_missing_entity",
            PieceIssue::EmptyOutline => "empty_outline",
            PieceIssue::OutlineOpen => "outline_open",
            PieceIssue::OutlineUnreadable => "outline_unreadable",
            PieceIssue::OutlineSelfIntersecting => "outline_self_intersecting",
            PieceIssue::InternalOutsideOutline { .. } => "internal_outside_outline",
            PieceIssue::MarkingOutsideOutline { .. } => "marking_outside_outline",
            PieceIssue::DartInvalid { .. } => "dart_invalid",
            PieceIssue::ButtonsTooCloseToEdge { .. } => "buttons_too_close_to_edge",
            PieceIssue::GrainRotationExceeded => "grain_rotation_exceeded",
            PieceIssue::DerivedStale { .. } => "derived_stale",
            PieceIssue::DuplicateCode { .. } => "duplicate_code",
            PieceIssue::CutContourFailed { .. } => "cut_contour_failed",
            PieceIssue::FoldNotchesFailed { .. } => "fold_notches_failed",
            PieceIssue::NetCutWithSeam { .. } => "net_cut_with_seam",
            PieceIssue::CutNotchesFailed { .. } => "cut_notches_failed",
            PieceIssue::CornerNotchesFailed { .. } => "corner_notches_failed",
            PieceIssue::CutDartFailed { .. } => "cut_dart_failed",
            PieceIssue::FoldOutsideContour { .. } => "fold_outside_contour",
            PieceIssue::FoldMarginIgnored { .. } => "fold_margin_ignored",
        }
    }
}

/// Fiche d'une pièce, en lecture seule, pour les consommateurs (exports E12 à venir).
/// Les mesures sont **observées** (recalculées d'après la construction), jamais stockées.
#[derive(Debug, Clone, PartialEq)]
pub struct PieceCard {
    pub id: PieceId,
    pub name: String,
    pub code: Option<String>,
    pub quantity: u32,
    /// Coupée en paire (×2 en miroir).
    pub mirrored_pair: bool,
    pub material: Option<String>,
    pub category: Option<PieceCategory>,
    pub comment: Option<String>,
    /// Un droit-fil est assigné (sans préjuger qu'il existe encore : voir `issues`).
    pub has_grainline: bool,
    /// La pièce est déclarée « sans droit-fil » (PH-5d, cuir) : aucun avertissement d'absence.
    pub no_grain: bool,
    /// Aire en mm² ; `None` si la pièce n'a pas de contour mesurable.
    pub area_mm2: Option<f64>,
    /// Périmètre en mm ; `None` si la pièce n'a pas de contour mesurable.
    pub perimeter_mm: Option<f64>,
    pub issues: Vec<PieceIssue>,
    /// Nombre d'éléments internes par type (US-04.3).
    pub internal: InternalCounts,
    /// Nombre de marquages (pointages, perçages, croix) de la pièce (US-06.3).
    pub markings: usize,
    /// Dérivation (mère, type, paramètre, état) si la pièce est dérivée (US-04.4).
    pub derived: Option<DerivedInfo>,
}

impl Project {
    /// Points d'attention d'une pièce (liste vide si elle n'en a aucun, ou si
    /// l'identifiant est inconnu). Lecture pure ; ordre stable.
    pub fn piece_issues(&self, piece: PieceId) -> Vec<PieceIssue> {
        let Ok(p) = self.piece(piece) else {
            return Vec::new();
        };
        let mut issues = Vec::new();
        if p.name.trim().is_empty() {
            issues.push(PieceIssue::NoName);
        }
        match p.grainline {
            // Pièce déclarée sans droit-fil (PH-5d) : l'absence est voulue.
            None if p.no_grain => {}
            None => issues.push(PieceIssue::MissingGrainline),
            Some(g) => {
                let found = match p.link {
                    PieceLink::Linked { sheet } => {
                        self.sheet(sheet).is_ok_and(|s| s.entity(g).is_ok())
                    }
                    PieceLink::Detached => self.sheets.iter().any(|s| s.entity(g).is_ok()),
                };
                if !found {
                    issues.push(PieceIssue::GrainlineMissingEntity);
                }
            }
        }
        if p.outline.is_empty() {
            issues.push(PieceIssue::EmptyOutline);
        } else {
            match self.piece_contour(piece) {
                Ok(c) => {
                    if !c.closed {
                        issues.push(PieceIssue::OutlineOpen);
                    }
                    if c.self_intersecting {
                        issues.push(PieceIssue::OutlineSelfIntersecting);
                    }
                }
                Err(_) => issues.push(PieceIssue::OutlineUnreadable),
            }
        }
        let outside = self
            .piece_internals(piece)
            .iter()
            .filter(|r| r.inside_outline == Some(false))
            .count();
        if outside > 0 {
            issues.push(PieceIssue::InternalOutsideOutline { count: outside });
        }
        let marks_outside = self
            .piece_markings(piece)
            .iter()
            .filter(|r| r.inside_outline == Some(false))
            .count();
        if marks_outside > 0 {
            issues.push(PieceIssue::MarkingOutsideOutline {
                count: marks_outside,
            });
        }
        let darts_bad = self.piece_darts_invalid(piece);
        if darts_bad > 0 {
            issues.push(PieceIssue::DartInvalid { count: darts_bad });
        }
        let too_close = self.piece_buttons_too_close(piece);
        if too_close > 0 {
            issues.push(PieceIssue::ButtonsTooCloseToEdge { count: too_close });
        }
        if self
            .piece_grain_rotation(piece)
            .is_some_and(|r| r.exceeded())
        {
            issues.push(PieceIssue::GrainRotationExceeded);
        }
        if let Some(reason) = p.derived.as_ref().and_then(|d| d.stale.as_ref()) {
            issues.push(PieceIssue::DerivedStale {
                reason: reason.clone(),
            });
        }
        if let (true, Some(seam)) = (p.net_cut, p.seam.as_ref()) {
            issues.push(PieceIssue::NetCutWithSeam {
                hems: seam.hems.len(),
                folds: seam.folds.len(),
            });
        }
        if p.net_cut && p.seam.is_none() {
            if let Err(e) = self.piece_cut_contour(piece) {
                issues.push(PieceIssue::CutContourFailed {
                    reason: e.to_string(),
                });
            }
        }
        if p.seam.is_some() {
            if let Err(e) = self.piece_cut_contour(piece) {
                issues.push(PieceIssue::CutContourFailed {
                    reason: e.to_string(),
                });
            }
            if let Err(e) = self.piece_fold_notches(piece) {
                issues.push(PieceIssue::FoldNotchesFailed {
                    reason: e.to_string(),
                });
            }
        }
        if p.darts.iter().any(|d| d.cut_closed) {
            if let Some(reason) = self.piece_cut_dart_failure(piece) {
                issues.push(PieceIssue::CutDartFailed { reason });
            }
        }
        let outside_fold = self.fold_outside_contour(p);
        if outside_fold > 0 {
            issues.push(PieceIssue::FoldOutsideContour {
                count: outside_fold,
            });
        }
        let ignored_fold = self.fold_margin_ignored(p);
        if ignored_fold > 0 {
            issues.push(PieceIssue::FoldMarginIgnored {
                count: ignored_fold,
            });
        }
        if let Some(reason) = self.piece_cut_notch_failure(piece) {
            issues.push(PieceIssue::CutNotchesFailed { reason });
        }
        if let Some(reason) = self.piece_corner_notch_failure(piece) {
            issues.push(PieceIssue::CornerNotchesFailed { reason });
        }
        if let Some(code) = p.code.as_deref().map(str::trim).filter(|c| !c.is_empty()) {
            let twin = self
                .pieces
                .iter()
                .any(|o| o.id != p.id && o.code.as_deref().map(str::trim) == Some(code));
            if twin {
                issues.push(PieceIssue::DuplicateCode {
                    code: code.to_string(),
                });
            }
        }
        issues
    }

    /// Fiche en lecture seule d'une pièce (nom, code, quantité, paire, matière,
    /// catégorie, commentaire, droit-fil, aire, périmètre, points d'attention).
    pub fn piece_card(&self, piece: PieceId) -> Result<PieceCard, CoreError> {
        let p = self.piece(piece)?;
        let contour = if p.outline.is_empty() {
            None
        } else {
            self.piece_contour(piece).ok()
        };
        Ok(PieceCard {
            id: p.id,
            name: p.name.clone(),
            code: p.code.clone(),
            quantity: p.quantity,
            mirrored_pair: p.mirrored_pair,
            material: p.material.clone(),
            category: p.category,
            comment: p.comment.clone(),
            has_grainline: p.grainline.is_some(),
            no_grain: p.no_grain,
            area_mm2: contour.as_ref().map(|c| c.area_mm2),
            perimeter_mm: contour.as_ref().map(|c| c.perimeter_mm),
            issues: self.piece_issues(piece),
            internal: self.piece_internal_counts(piece),
            markings: p.markings.len(),
            derived: self.piece_derivation(piece),
        })
    }
}
