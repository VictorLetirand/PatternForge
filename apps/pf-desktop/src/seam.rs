//! Section « Marge de couture » de l'inspecteur d'une pièce (US-05.1), **repliée par défaut**
//! (la fiche de pièce ne s'allonge pas). Elle ne calcule rien : le contour de coupe, son
//! périmètre et son aire viennent de `Project::piece_cut_contour` (calculé à chaque image,
//! jamais stocké), et chaque modification est **une** commande `SetSeamAllowance` (une étape
//! annulable, refus sans trace). Convention non validée par une modéliste ; pas d'export.

use crate::app::App;
use crate::palette::pal;
use crate::panels::{buffered_edit, category_label};
use eframe::egui::{self, RichText};
use pf_core::input::{format_mm, format_number};
use pf_core::units::{field_text, len_number, parse_length_display};
use pf_core::{
    Command, CornerAllowance, CornerType, EdgeAllowance, EntityId, SeamAllowance, MAX_CORNER_MM,
    MAX_SEAM_MM,
};
use pf_i18n::{tr, tr_args};

/// Valeur saisie → millimètres, bornée comme la commande (message de l'inspecteur sinon).
fn parse_margin(text: &str) -> Option<f64> {
    parse_length_display(text)
        .ok()
        .filter(|v| v.is_finite() && (0.0..=MAX_SEAM_MM).contains(v))
}

/// Longueur de coin (pointe maximale, profondeur) : bornée comme la commande.
fn parse_corner_mm(text: &str) -> Option<f64> {
    parse_length_display(text)
        .ok()
        .filter(|v| v.is_finite() && (0.0..=MAX_CORNER_MM).contains(v))
}

/// Les types de coins proposés, dans l'ordre d'affichage (`None` = onglet, le défaut).
fn corner_choices() -> [Option<CornerType>; 8] {
    [
        None,
        Some(CornerType::PerpendicularPrevious {}),
        Some(CornerType::PerpendicularNext {}),
        Some(CornerType::Symmetric {}),
        Some(CornerType::Truncated { depth_mm: 2.0 }),
        Some(CornerType::Stepped {}),
        Some(CornerType::HemFoldNext {}),
        Some(CornerType::HemFoldPrevious {}),
    ]
}

/// Libellé et info-bulle d'un type de coin (clés en littéral : le test des clés les voit).
fn corner_label(c: Option<CornerType>) -> (String, String) {
    match c {
        None => (
            tr("piece.seam.corner.mitre"),
            tr("piece.seam.corner.mitre.hover"),
        ),
        Some(CornerType::PerpendicularPrevious {}) => (
            tr("piece.seam.corner.perp_prev"),
            tr("piece.seam.corner.perp_prev.hover"),
        ),
        Some(CornerType::PerpendicularNext {}) => (
            tr("piece.seam.corner.perp_next"),
            tr("piece.seam.corner.perp_next.hover"),
        ),
        Some(CornerType::Symmetric {}) => (
            tr("piece.seam.corner.symmetric"),
            tr("piece.seam.corner.symmetric.hover"),
        ),
        Some(CornerType::Truncated { .. }) => (
            tr("piece.seam.corner.truncated"),
            tr("piece.seam.corner.truncated.hover"),
        ),
        Some(CornerType::Stepped {}) => (
            tr("piece.seam.corner.stepped"),
            tr("piece.seam.corner.stepped.hover"),
        ),
        Some(CornerType::HemFoldNext {}) => (
            tr("piece.seam.corner.hem_next"),
            tr("piece.seam.corner.hem_next.hover"),
        ),
        Some(CornerType::HemFoldPrevious {}) => (
            tr("piece.seam.corner.hem_prev"),
            tr("piece.seam.corner.hem_prev.hover"),
        ),
    }
}

/// Même type, quels que soient ses paramètres.
fn same_kind(a: Option<CornerType>, b: Option<CornerType>) -> bool {
    fn id(c: Option<CornerType>) -> u8 {
        match c {
            None => 0,
            Some(CornerType::PerpendicularPrevious {}) => 1,
            Some(CornerType::PerpendicularNext {}) => 2,
            Some(CornerType::Symmetric {}) => 3,
            Some(CornerType::Truncated { .. }) => 4,
            Some(CornerType::Stepped {}) => 5,
            Some(CornerType::HemFoldNext {}) => 6,
            Some(CornerType::HemFoldPrevious {}) => 7,
        }
    }
    id(a) == id(b)
}

/// Projet de capture `--etape coins` : un parallélogramme (coins aigus et obtus), marge de 12 mm,
/// trois sommets avec un type de coin différent, le tracé du sommet 1 sélectionné ; renvoie aussi
/// ce tracé.
pub(crate) fn demo_corners_project(
) -> Result<(pf_core::Project, pf_core::EntityId), pf_core::CoreError> {
    use pf_core::{PieceSource, PointRef};
    use pf_geometry::PointMm;
    let mut p = pf_core::Project::new("Types de coins");
    let sheet = p.sheets[0].id;
    let pts = [(0.0, 0.0), (100.0, 0.0), (160.0, 80.0), (60.0, 80.0)];
    let mut ids = Vec::new();
    for i in 0..pts.len() {
        let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
        ids.push(
            p.apply(Command::AddSegment {
                sheet,
                from: PointRef::New(PointMm::new(a.0, a.1)),
                to: PointRef::New(PointMm::new(b.0, b.1)),
            })?
            .entities[0],
        );
    }
    let piece = p
        .apply(Command::ExtractPiece {
            sheet,
            source: PieceSource::Paths {
                entities: ids.clone(),
            },
            name: "Coins".into(),
            detached: false,
        })?
        .pieces[0];
    // Le tracé i commence au sommet i (parallélogramme anti-horaire).
    p.apply(Command::SetSeamAllowance {
        piece,
        seam: Some(SeamAllowance {
            default_mm: 12.0,
            corners: vec![
                CornerAllowance {
                    entity: ids[0],
                    corner: CornerType::Truncated { depth_mm: 4.0 },
                },
                CornerAllowance {
                    entity: ids[1],
                    corner: CornerType::Stepped {},
                },
                CornerAllowance {
                    entity: ids[2],
                    corner: CornerType::Symmetric {},
                },
            ],
            ..Default::default()
        }),
    })?;
    Ok((p, ids[1]))
}

/// Projet de capture (`--etape marge` / `marge-progressive`) : la pièce d'exemple avec une marge
/// de 10 mm, plus, pour la progressive, son bord droit de 0 mm (bas) à 25 mm (haut).
pub(crate) fn demo_seam_project(progressive: bool) -> Result<pf_core::Project, pf_core::CoreError> {
    let mut p = pf_core::demo_piece_project()?;
    let piece = p.pieces[0].id;
    let mut edges = Vec::new();
    if progressive {
        let sheet = p.piece_sheet(piece)?;
        let right = p.pieces[0].outline.iter().find_map(|o| {
            let (a, b) = sheet.segment_ends(o.entity).ok()?;
            ((a.x - b.x).abs() < 1e-6 && a.x > 100.0).then_some(o.entity)
        });
        if let Some(entity) = right {
            edges.push(EdgeAllowance {
                entity,
                start_mm: 0.0,
                end_mm: 25.0,
            });
        }
    }
    p.apply(Command::SetSeamAllowance {
        piece,
        seam: Some(SeamAllowance {
            default_mm: 10.0,
            edges,
            ..Default::default()
        }),
    })?;
    Ok(p)
}

/// Projet de capture `--etape sur-le-pli` (PH-5b) : la pièce d'exemple (170 × 120) avec une marge de
/// 10 mm et son bord gauche (x = 0) déclaré sur le pli ; renvoie aussi ce bord (sélectionné).
pub(crate) fn demo_fold_project() -> Result<(pf_core::Project, EntityId), pf_core::CoreError> {
    let mut p = pf_core::demo_piece_project()?;
    let piece = p.pieces[0].id;
    p.apply(Command::SetSeamAllowance {
        piece,
        seam: Some(SeamAllowance {
            default_mm: 10.0,
            ..Default::default()
        }),
    })?;
    let left = {
        let sheet = p.piece_sheet(piece)?;
        p.pieces[0].outline.iter().find_map(|o| {
            let (a, b) = sheet.segment_ends(o.entity).ok()?;
            (a.x.abs() < 1e-6 && b.x.abs() < 1e-6).then_some(o.entity)
        })
    }
    .ok_or_else(|| pf_core::CoreError::Invalid("bord gauche introuvable".into()))?;
    p.apply(Command::SetPieceFold {
        piece,
        edge: left,
        on: true,
    })?;
    Ok((p, left))
}

/// Projet de capture `--etape coupe-nette` : la pièce d'exemple en cuir déclarée en coupe nette.
pub(crate) fn demo_net_cut_project() -> Result<pf_core::Project, pf_core::CoreError> {
    let mut p = pf_core::demo_piece_project()?;
    let piece = p.pieces[0].id;
    p.apply(Command::SetNetCut {
        piece,
        net_cut: true,
    })?;
    Ok(p)
}

/// Projet de capture `--etape matieres` : la pièce d'exemple en cuir, sans marge, avec un jeu du
/// projet où le cuir vaut 7 mm.
pub(crate) fn demo_materials_project() -> Result<pf_core::Project, pf_core::CoreError> {
    let mut p = pf_core::demo_piece_project()?;
    let c = p.pieces[0].clone();
    p.apply(Command::SetPieceCard {
        piece: c.id,
        name: c.name,
        code: c.code,
        quantity: c.quantity,
        mirrored_pair: c.mirrored_pair,
        material: c.material,
        category: Some(pf_core::PieceCategory::Leather),
        comment: c.comment,
    })?;
    p.apply(Command::SetSeamDefaults {
        defaults: Some(pf_core::SeamDefaults {
            leather_mm: 7.0,
            ..pf_core::SeamDefaults::builtin()
        }),
    })?;
    Ok(p)
}

impl App {
    /// Types de coins (US-05.2) : liste des coins choisis, choix du type du coin qui commence au
    /// tracé sélectionné, pointe maximale. Le type est toujours écrit en toutes lettres (jamais la
    /// couleur seule) ; le contour de coupe calculé, dessiné sur le canevas, sert d'aperçu.
    #[allow(clippy::too_many_arguments)]
    fn corner_block(
        &mut self,
        ui: &mut egui::Ui,
        piece: &pf_core::Piece,
        current: &SeamAllowance,
        next: &mut SeamAllowance,
        vertices: &[pf_core::SeamVertex],
        name_of: &dyn Fn(EntityId) -> String,
        key: u64,
    ) {
        let title = crate::a11y::label(
            ui,
            RichText::new(tr_args(
                "piece.seam.corners",
                &[("count", &current.corners.len().to_string())],
            ))
            .small()
            .strong()
            .color(pal().muted),
        );
        if self.corners_focus {
            // egui aligne le haut du titre moins un interligne sur le haut de la vue : le bord du
            // widget précédent affleure et laisse un liseré rogné. On vise 2 points plus bas
            // (PH-2c, défaut 14) : la ligne précédente sort entièrement de la vue.
            let top = title.rect.min + egui::vec2(0.0, 2.0);
            ui.scroll_to_rect(
                egui::Rect::from_min_size(top, title.rect.size()),
                Some(egui::Align::TOP),
            );
        }
        if current.corners.is_empty() {
            crate::a11y::label(
                ui,
                RichText::new(tr("piece.seam.corner.none"))
                    .small()
                    .color(pal().muted),
            );
        }
        let mut remove = None;
        for (i, c) in current.corners.iter().enumerate() {
            ui.horizontal_wrapped(|ui| {
                let (label, hover) = corner_label(Some(c.corner));
                crate::a11y::label(
                    ui,
                    RichText::new(format!("{} : {label}", name_of(c.entity))).small(),
                )
                .on_hover_text(hover);
                if let CornerType::Truncated { depth_mm } = c.corner {
                    crate::a11y::label(ui, RichText::new(tr("piece.seam.corner.depth")).small())
                        .on_hover_text(tr("piece.seam.corner.depth.hover"));
                    let k = key | (1 << 40) | ((i as u64 + 1) << 20);
                    if let Some(text) =
                        buffered_edit(ui, &mut self.text_buf, k, &field_text(depth_mm), 48.0)
                    {
                        match parse_corner_mm(&text) {
                            Some(v) => {
                                next.corners[i].corner = CornerType::Truncated { depth_mm: v }
                            }
                            None => self.editor.error(tr("piece.seam.corner.invalid")),
                        }
                    }
                }
                if ui.small_button(tr("piece.seam.corner.remove")).clicked() {
                    remove = Some(i);
                }
            });
        }
        if let Some(i) = remove {
            next.corners.remove(i);
        }
        // Coin qui commence au tracé sélectionné : choix du type.
        let selected = match self.editor.selection.entities.as_slice() {
            [e] if piece.outline.iter().any(|o| o.entity == *e) => Some(*e),
            _ => None,
        };
        match selected {
            None => {
                crate::a11y::label(
                    ui,
                    RichText::new(tr("piece.seam.corner.select"))
                        .small()
                        .color(pal().muted),
                );
            }
            Some(e) => match vertices.iter().find(|v| v.entity == e) {
                None => {
                    crate::a11y::label(
                        ui,
                        RichText::new(tr("piece.seam.corner.no_vertex"))
                            .small()
                            .color(pal().muted),
                    );
                }
                Some(v) if !v.convex => {
                    crate::a11y::label(
                        ui,
                        RichText::new(tr_args(
                            "piece.seam.corner.reentrant",
                            &[("name", &name_of(e))],
                        ))
                        .small()
                        .color(pal().muted),
                    );
                }
                Some(v) => {
                    let point = format!("{} ; {}", format_mm(v.point.x), format_mm(v.point.y));
                    crate::a11y::label(
                        ui,
                        RichText::new(tr_args(
                            "piece.seam.corner.at",
                            &[("name", &name_of(e)), ("point", &point)],
                        ))
                        .small(),
                    );
                    let chosen = current
                        .corners
                        .iter()
                        .find(|c| c.entity == e)
                        .map(|c| c.corner);
                    ui.horizontal_wrapped(|ui| {
                        for choice in corner_choices() {
                            let (label, hover) = corner_label(choice);
                            let on = same_kind(chosen, choice);
                            if ui
                                .selectable_label(on, RichText::new(label).small())
                                .on_hover_text(hover)
                                .clicked()
                                && !on
                            {
                                next.corners.retain(|c| c.entity != e);
                                if let Some(t) = choice {
                                    next.corners.push(CornerAllowance {
                                        entity: e,
                                        corner: t,
                                    });
                                }
                            }
                        }
                    });
                }
            },
        }
        // Longueur maximale d'une pointe d'onglet (vide = 4 × la plus grande marge adjacente).
        ui.horizontal(|ui| {
            crate::a11y::label(ui, RichText::new(tr("piece.seam.corner.max")).small())
                .on_hover_text(tr("piece.seam.corner.max.hover"));
            let shown = current.max_corner_mm.map(field_text).unwrap_or_default();
            if let Some(text) = buffered_edit(ui, &mut self.text_buf, key | (2 << 40), &shown, 56.0)
            {
                if text.trim().is_empty() {
                    next.max_corner_mm = None;
                } else {
                    match parse_corner_mm(&text) {
                        Some(v) => next.max_corner_mm = Some(v),
                        None => self.editor.error(tr("piece.seam.corner.invalid")),
                    }
                }
            }
        });
    }

    /// Bords sur le pli (PH-5b) : case « Sur le pli » pour le bord rectiligne sélectionné (une
    /// commande `SetPieceFold` par action, refus affiché), liste des bords déclarés avec
    /// « Retirer », nombre et longueur observés. Le rôle est écrit en toutes lettres (jamais la
    /// couleur seule) ; le canevas dessine le bord avec le libellé « SUR LE PLI ».
    fn fold_block(
        &mut self,
        ui: &mut egui::Ui,
        piece: &pf_core::Piece,
        name_of: &dyn Fn(EntityId) -> String,
    ) {
        crate::a11y::label(
            ui,
            RichText::new(tr_args(
                "onfold.title",
                &[("count", &piece.on_fold.len().to_string())],
            ))
            .small()
            .strong()
            .color(pal().muted),
        );
        let selected = match self.editor.selection.entities.as_slice() {
            [e] if piece.outline.iter().any(|o| o.entity == *e) => Some(*e),
            _ => None,
        };
        match selected {
            None => {
                crate::a11y::label(
                    ui,
                    RichText::new(tr("onfold.select"))
                        .small()
                        .color(pal().muted),
                );
            }
            Some(e) => {
                let straight = self
                    .editor
                    .project()
                    .piece_sheet(piece.id)
                    .ok()
                    .and_then(|s| s.entity(e).ok())
                    .is_some_and(|x| matches!(x.kind, pf_core::EntityKind::Segment { .. }));
                let declared = piece.on_fold.contains(&e);
                if straight || declared {
                    let mut on = declared;
                    if ui
                        .checkbox(&mut on, tr("onfold.toggle"))
                        .on_hover_text(tr("onfold.toggle.hover"))
                        .changed()
                    {
                        self.editor.exec(Command::SetPieceFold {
                            piece: piece.id,
                            edge: e,
                            on,
                        });
                    }
                } else {
                    crate::a11y::label(
                        ui,
                        RichText::new(tr("onfold.not_straight"))
                            .small()
                            .color(pal().muted),
                    );
                }
            }
        }
        let mut remove = None;
        for e in &piece.on_fold {
            ui.horizontal(|ui| {
                crate::a11y::label(ui, RichText::new(name_of(*e)).small());
                if ui.small_button(tr("onfold.remove")).clicked() {
                    remove = Some(*e);
                }
            });
        }
        if let Some(edge) = remove {
            self.editor.exec(Command::SetPieceFold {
                piece: piece.id,
                edge,
                on: false,
            });
        }
        let summary = self.editor.project().piece_fold_summary(piece.id);
        if summary.count > 0 {
            crate::a11y::label(
                ui,
                RichText::new(tr_args(
                    "onfold.summary",
                    &[
                        ("count", &summary.count.to_string()),
                        ("length", &format_mm(summary.length_mm)),
                    ],
                ))
                .small()
                .color(pal().muted),
            );
            crate::a11y::label(
                ui,
                RichText::new(tr("onfold.note")).small().color(pal().muted),
            );
        }
    }

    /// Marges par matière (US-05.5) : « appliquer la valeur de la catégorie » (une commande
    /// `SetSeamAllowance`, qui garde les surcharges) et, dans un sous-bloc **replié**, le jeu du
    /// projet par catégorie (une commande `SetSeamDefaults` par valeur validée). Modifier le jeu ne
    /// touche aucune marge posée.
    fn material_block(
        &mut self,
        ui: &mut egui::Ui,
        piece: &pf_core::Piece,
        next: &mut SeamAllowance,
        key: u64,
    ) {
        let (defaults, fallback) = {
            let p = self.editor.project();
            (p.seam_defaults_effective(), p.seam_default_for(None))
        };
        match piece.category {
            Some(c) => {
                let value = len_number(defaults.get(c), 3);
                crate::a11y::label(
                    ui,
                    RichText::new(tr_args(
                        "piece.seam.mat.of",
                        &[("category", &category_label(Some(c)))],
                    ))
                    .small()
                    .color(pal().muted),
                );
                if ui
                    .button(pf_i18n::tr_len(
                        "piece.seam.mat.apply",
                        &[("value", &value)],
                    ))
                    .on_hover_text(tr("piece.seam.mat.apply.hover"))
                    .clicked()
                {
                    next.default_mm = defaults.get(c);
                }
            }
            None => {
                let value = len_number(fallback, 3);
                crate::a11y::label(
                    ui,
                    RichText::new(tr_args("piece.seam.mat.none", &[("value", &value)]))
                        .small()
                        .color(pal().muted),
                );
                ui.add_enabled(false, egui::Button::new(tr("piece.seam.mat.apply.none")))
                    .on_hover_text(tr("piece.seam.mat.apply.hover"));
                if piece.seam.is_none()
                    && ui
                        .button(pf_i18n::tr_len(
                            "piece.seam.mat.fallback",
                            &[("value", &value)],
                        ))
                        .clicked()
                {
                    next.default_mm = fallback;
                }
            }
        }
        let header = egui::CollapsingHeader::new(
            RichText::new(tr("piece.seam.mat.title"))
                .small()
                .strong()
                .color(pal().muted),
        )
        .id_salt(("seam-materials", piece.id.0))
        .default_open(false);
        let header = if self.materials_open {
            header.open(Some(true))
        } else {
            header
        };
        let focus = self.materials_open;
        let shown = header.show(ui, |ui| {
            let mut new = defaults;
            for (i, c) in pf_core::PieceCategory::ALL.into_iter().enumerate() {
                ui.horizontal(|ui| {
                    crate::a11y::label(ui, RichText::new(category_label(Some(c))).small());
                    let k = key | (3 << 40) | ((i as u64 + 1) << 20);
                    let shown = field_text(defaults.get(c));
                    if let Some(text) = buffered_edit(ui, &mut self.text_buf, k, &shown, 56.0) {
                        match parse_margin(&text) {
                            Some(v) => new.set(c, v),
                            None => self.editor.error(tr("piece.seam.mat.invalid")),
                        }
                    }
                });
            }
            if ui
                .add_enabled(
                    defaults != pf_core::SeamDefaults::builtin(),
                    egui::Button::new(tr("piece.seam.mat.reset")),
                )
                .clicked()
            {
                new = pf_core::SeamDefaults::builtin();
            }
            crate::a11y::label(
                ui,
                RichText::new(tr("piece.seam.mat.note"))
                    .small()
                    .color(pal().muted),
            );
            if new != defaults {
                self.editor.exec(Command::SetSeamDefaults {
                    defaults: Some(new),
                });
            }
        });
        if focus {
            shown.header_response.scroll_to_me(Some(egui::Align::TOP));
        }
    }

    /// Marge de couture d'une pièce à contour (pas d'une dérivée liée : la commande la refuse).
    pub(crate) fn piece_seam_section(&mut self, ui: &mut egui::Ui, piece: &pf_core::Piece) {
        if piece.outline.is_empty() || piece.derived.is_some() {
            return;
        }
        let header = egui::CollapsingHeader::new(
            RichText::new(tr("piece.seam.title"))
                .small()
                .strong()
                .color(pal().muted),
        )
        .id_salt(("seam-section", piece.id.0))
        .default_open(false);
        // Capture : `--etape marge…` déplie la section (état d'interface, jamais enregistré).
        let header = if self.seam_open {
            header.open(Some(true))
        } else {
            header
        };
        header.show(ui, |ui| self.piece_seam_body(ui, piece));
    }

    fn piece_seam_body(&mut self, ui: &mut egui::Ui, piece: &pf_core::Piece) {
        let current = piece.seam.clone().unwrap_or_default();
        let mut next = current.clone();
        let key = (piece.id.0 << 8) | 0x90;
        let (names, cut, vertices) = {
            let project = self.editor.project();
            let names: Vec<(EntityId, String)> = project
                .piece_sheet(piece.id)
                .map(|s| {
                    piece
                        .outline
                        .iter()
                        .map(|e| (e.entity, s.entity_name(e.entity)))
                        .collect()
                })
                .unwrap_or_default();
            let cut = piece
                .seam
                .as_ref()
                .map(|_| self.editor.view_project().piece_cut_contour(piece.id));
            let vertices = project.seam_vertices(piece.id).unwrap_or_default();
            (names, cut, vertices)
        };
        let name_of = |id: EntityId| {
            names
                .iter()
                .find(|n| n.0 == id)
                .map(|n| n.1.clone())
                .unwrap_or_default()
        };
        // Coupe nette (US-05.6) : une commande par bascule ; exclusive avec une marge (refus
        // expliqué par la commande). Déclarée, elle remplace les réglages de marge à l'écran.
        let mut net = piece.net_cut;
        if ui
            .checkbox(&mut net, tr("piece.netcut.toggle"))
            .on_hover_text(tr("piece.netcut.hover"))
            .changed()
        {
            self.editor.exec(Command::SetNetCut {
                piece: piece.id,
                net_cut: net,
            });
            return;
        }
        self.fold_block(ui, piece, &name_of);
        if piece.net_cut {
            let text = match self.editor.view_project().piece_cut_contour(piece.id) {
                Ok(c) => tr_args(
                    "piece.netcut.on",
                    &[
                        ("perimeter", &format_mm(c.perimeter_mm)),
                        ("area", &format_number(c.area_mm2 / 100.0, 2)),
                    ],
                ),
                Err(e) => tr_args("piece.seam.failed", &[("reason", &e.to_string())]),
            };
            crate::a11y::label(ui, RichText::new(text).small().color(pal().muted));
            return;
        }
        ui.horizontal(|ui| {
            crate::a11y::label(ui, RichText::new(tr("piece.seam.default")).small())
                .on_hover_text(tr("piece.seam.default.hover"));
            let shown = field_text(current.default_mm);
            if let Some(text) = buffered_edit(ui, &mut self.text_buf, key, &shown, 56.0) {
                match parse_margin(&text) {
                    Some(v) => next.default_mm = v,
                    None => self.editor.error(tr("piece.seam.invalid")),
                }
            }
        });
        if piece.seam.is_none() {
            let margin = self.prefs.default_margin_mm;
            if ui
                .button(pf_i18n::tr_len(
                    "prefs.seam.apply",
                    &[("value", &len_number(margin, 3))],
                ))
                .on_hover_text(tr("prefs.seam.apply.hover"))
                .clicked()
            {
                self.editor.exec(Command::SetSeamAllowance {
                    piece: piece.id,
                    seam: Some(SeamAllowance {
                        default_mm: margin,
                        ..Default::default()
                    }),
                });
                return;
            }
        }
        self.material_block(ui, piece, &mut next, key);
        // Mesures observées du contour de coupe calculé, ou la raison de l'échec.
        match &cut {
            Some(Ok(c)) => {
                crate::a11y::label(
                    ui,
                    RichText::new(tr_args(
                        "piece.seam.observed",
                        &[
                            ("perimeter", &format_mm(c.perimeter_mm)),
                            ("area", &format_number(c.area_mm2 / 100.0, 2)),
                        ],
                    ))
                    .small()
                    .color(pal().muted),
                );
            }
            Some(Err(e)) => {
                crate::a11y::label(
                    ui,
                    RichText::new(tr_args("piece.seam.failed", &[("reason", &e.to_string())]))
                        .small()
                        .color(ui.visuals().warn_fg_color),
                );
            }
            None => {}
        }
        crate::a11y::label(
            ui,
            RichText::new(tr_args(
                "piece.seam.edges",
                &[("count", &current.edges.len().to_string())],
            ))
            .small()
            .color(pal().muted),
        );
        if current.edges.is_empty() {
            crate::a11y::label(
                ui,
                RichText::new(tr("piece.seam.edges.none"))
                    .small()
                    .color(pal().muted),
            );
        }
        let mut remove = None;
        for (i, edge) in current.edges.iter().enumerate() {
            ui.horizontal_wrapped(|ui| {
                crate::a11y::label(ui, RichText::new(name_of(edge.entity)).small());
                for (n, (label, value)) in [
                    (tr("piece.seam.edge.start"), edge.start_mm),
                    (tr("piece.seam.edge.end"), edge.end_mm),
                ]
                .into_iter()
                .enumerate()
                {
                    crate::a11y::label(ui, RichText::new(label).small())
                        .on_hover_text(tr("piece.seam.edge.hover"));
                    let k = key | ((i as u64 + 1) << 16) | ((n as u64 + 1) << 12);
                    let shown = field_text(value);
                    if let Some(text) = buffered_edit(ui, &mut self.text_buf, k, &shown, 48.0) {
                        match parse_margin(&text) {
                            Some(v) if n == 0 => next.edges[i].start_mm = v,
                            Some(v) => next.edges[i].end_mm = v,
                            None => self.editor.error(tr("piece.seam.invalid")),
                        }
                    }
                }
                if ui.small_button(tr("piece.seam.edge.remove")).clicked() {
                    remove = Some(i);
                }
            });
        }
        if let Some(i) = remove {
            next.edges.remove(i);
        }
        // Tracé sélectionné du contour et pas encore surchargé : une surcharge à la marge par défaut.
        let candidate = match self.editor.selection.entities.as_slice() {
            [e] if piece.outline.iter().any(|o| o.entity == *e)
                && !current.edges.iter().any(|x| x.entity == *e) =>
            {
                Some(*e)
            }
            _ => None,
        };
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(candidate.is_some(), egui::Button::new(tr("piece.seam.add")))
                .on_hover_text(tr("piece.seam.add.hover"))
                .clicked()
            {
                if let Some(entity) = candidate {
                    next.edges.push(EdgeAllowance {
                        entity,
                        start_mm: current.default_mm,
                        end_mm: current.default_mm,
                    });
                }
            }
            if ui
                .add_enabled(
                    !current.edges.is_empty(),
                    egui::Button::new(tr("piece.seam.clear")),
                )
                .clicked()
            {
                next.edges.clear();
            }
        });
        self.corner_block(ui, piece, &current, &mut next, &vertices, &name_of, key);
        self.hem_fold_block(ui, piece, &current, &mut next, &name_of, key);
        crate::a11y::label(
            ui,
            RichText::new(tr("piece.seam.note"))
                .small()
                .color(pal().muted),
        );
        if next != current {
            let seam = (!next.is_default()).then_some(next);
            if seam != piece.seam {
                self.editor.exec(Command::SetSeamAllowance {
                    piece: piece.id,
                    seam,
                });
            }
        }
    }
}
