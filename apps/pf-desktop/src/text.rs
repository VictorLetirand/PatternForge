//! Textes dérivés du modèle, traduits dans la langue courante : noms de
//! tracés, rôles, et erreurs du cœur précédées d'un préfixe traduit.
//!
//! Limite assumée : le corps des messages d'erreur de `pf-core`, `pf-geometry`
//! et `pf-export` reste en français ; seul le préfixe est traduit.

use crate::editor::capitalize;
use pf_core::{CoreError, EntityId, EntityKind, PathRole, PointRole, Sheet};
use pf_geometry::GeometryError;
use pf_i18n::{tr, tr_args};

/// Texte d'un nombre affiché dans un champ à glisser : séparateur décimal de la langue courante
/// (un seul formateur, `format_number`). Affichage seulement : jamais écrit dans un fichier.
pub fn drag_text(value: f64, max_decimals: usize) -> String {
    pf_core::input::format_number(value, max_decimals)
}

/// Champs à glisser : affichage et saisie avec le séparateur décimal de la langue (« 42,0 mm »).
pub trait Localized {
    fn localized(self) -> Self;
}

impl Localized for eframe::egui::DragValue<'_> {
    fn localized(self) -> Self {
        self.custom_formatter(|n, decimals| drag_text(n, *decimals.end()))
            .custom_parser(|s| s.trim().replace(',', ".").parse::<f64>().ok())
    }
}

/// Message d'erreur du cœur, avec majuscule et préfixe traduit. Les erreurs `CoreError` (et
/// `SaveError::Core`) passent par leur code stable (US-19.1) ; les autres types gardent leur texte.
pub fn core_error<E: std::fmt::Display + std::any::Any>(error: &E) -> String {
    let any = error as &dyn std::any::Any;
    let detail = if let Some(e) = any.downcast_ref::<CoreError>() {
        localized_core(e)
    } else if let Some(pf_core::SaveError::Core(e)) = any.downcast_ref::<pf_core::SaveError>() {
        localized_core(e)
    } else {
        error.to_string()
    };
    tr_args("error.core", &[("detail", &capitalize(&detail))])
}

fn fill(key: &str, params: &[(&'static str, String)]) -> String {
    let args: Vec<(&str, &str)> = params.iter().map(|(k, v)| (*k, v.as_str())).collect();
    tr_args(key, &args)
}

/// Message d'une erreur du cœur dans la langue courante : clé `err.core.<code>` + paramètres.
pub fn localized_core(error: &CoreError) -> String {
    match error {
        CoreError::Geometry(g) => localized_geometry(g),
        other => fill(&format!("err.core.{}", other.code()), &other.params()),
    }
}

/// Message d'une erreur de géométrie dans la langue courante : clé `err.geo.<code>`.
pub fn localized_geometry(error: &GeometryError) -> String {
    fill(&format!("err.geo.{}", error.code()), &error.params())
}

/// Nom lisible d'un tracé (« Segment AB » / « Courbe AC »), préfixe traduit.
/// Les libellés de points viennent du modèle.
pub fn entity_name(sheet: &Sheet, id: EntityId) -> String {
    match sheet.entity(id).map(|e| e.kind.clone()) {
        Ok(EntityKind::Segment { from, to, .. }) => format!(
            "{} {}{}",
            tr("entity.segment"),
            sheet.label(from),
            sheet.label(to)
        ),
        Ok(EntityKind::Curve { start, end, .. }) => format!(
            "{} {}{}",
            tr("entity.curve"),
            sheet.label(start),
            sheet.label(end)
        ),
        Ok(EntityKind::Circle { center, .. }) => {
            format!("{} {}", tr("entity.circle"), sheet.label(center))
        }
        Ok(EntityKind::Arc {
            center, start, end, ..
        }) => format!(
            "{} {}{} ({} {})",
            tr("entity.arc"),
            sheet.label(start),
            sheet.label(end),
            tr("entity.arc.center"),
            sheet.label(center)
        ),
        Ok(EntityKind::Spline { nodes }) => format!(
            "{} {}…{}",
            tr("entity.spline"),
            nodes.first().map_or("?", |n| sheet.label(n.point)),
            nodes.last().map_or("?", |n| sheet.label(n.point))
        ),
        Err(_) => tr("entity.missing"),
    }
}

/// Libellé traduit du rôle d'un tracé.
pub fn role_label(role: PathRole) -> String {
    match role {
        PathRole::Construction => tr("role.construction"),
        PathRole::SewingContour => tr("role.sewing"),
        PathRole::CuttingContour => tr("role.cutting"),
        PathRole::Grainline => tr("role.grainline"),
        PathRole::Fold => tr("role.fold"),
        PathRole::AnnotationAxis => tr("role.axis"),
        PathRole::Topstitch => tr("role.topstitch"),
        PathRole::CenterFront => tr("role.center_front"),
        PathRole::CenterBack => tr("role.center_back"),
        PathRole::WaistLine => tr("role.waist_line"),
        PathRole::HipLine => tr("role.hip_line"),
        PathRole::BustLine => tr("role.bust_line"),
        PathRole::SkivingLine => tr("role.skiving_line"),
        PathRole::GlueLine => tr("role.glue_line"),
    }
}

/// Libellé traduit du type d'un élément interne de pièce (US-04.3).
pub fn internal_kind_label(kind: pf_core::InternalKind) -> String {
    use pf_core::InternalKind as K;
    match kind {
        K::Fold => tr("piece.internal.kind.fold"),
        K::Topstitch => tr("piece.internal.kind.topstitch"),
        K::PocketPosition => tr("piece.internal.kind.pocket_position"),
        K::Mark => tr("piece.internal.kind.mark"),
        K::Buttonhole => tr("piece.internal.kind.buttonhole"),
        K::Other => tr("piece.internal.kind.other"),
    }
}

/// Nom lisible d'une cible d'élément interne : tracé, étiquette du point, « cran sur … ».
pub fn internal_target_name(sheet: &Sheet, target: pf_core::InternalTarget) -> String {
    match target {
        pf_core::InternalTarget::Path { entity } => entity_name(sheet, entity),
        pf_core::InternalTarget::Point { point } => sheet.label(point).to_string(),
        pf_core::InternalTarget::Notch { notch } => {
            let host = sheet
                .notch(notch)
                .map(|n| entity_name(sheet, n.entity))
                .unwrap_or_default();
            tr_args("piece.internal.notch_name", &[("name", &host)])
        }
    }
}

/// Libellé traduit de la forme d'un marquage de pièce (US-06.3), dimension comprise.
pub fn marking_shape_label(shape: pf_core::MarkingShape) -> String {
    use pf_core::input::format_mm;
    match shape {
        pf_core::MarkingShape::Punch => tr("piece.marking.shape.punch"),
        pf_core::MarkingShape::Drill { diameter_mm } => tr_args(
            "piece.marking.shape.drill",
            &[("size", &format_mm(diameter_mm))],
        ),
        pf_core::MarkingShape::Cross { size_mm } => tr_args(
            "piece.marking.shape.cross",
            &[("size", &format_mm(size_mm))],
        ),
        pf_core::MarkingShape::Slot {
            length_mm,
            width_mm,
            ..
        } => tr_args(
            "piece.marking.shape.slot",
            &[
                ("length", &format_mm(length_mm)),
                ("width", &format_mm(width_mm)),
            ],
        ),
    }
}

/// Libellé traduit du motif d'un objet ignoré à l'import.
pub fn skip_reason_label(reason: pf_core::piece_internal::SkipReason) -> String {
    use pf_core::piece_internal::SkipReason as R;
    match reason {
        R::OutsideOutline => tr("piece.internal.skip.outside_outline"),
        R::OutlineEdge => tr("piece.internal.skip.outline_edge"),
        R::OtherPieceOutline => tr("piece.internal.skip.other_piece_outline"),
        R::ContourRole => tr("piece.internal.skip.contour_role"),
        R::Grainline => tr("piece.internal.skip.grainline"),
        R::OutlineNotch => tr("piece.internal.skip.outline_notch"),
        R::Unreadable => tr("piece.internal.skip.unreadable"),
    }
}

/// Libellé traduit du rôle d'un point.
pub fn point_role_label(role: PointRole) -> String {
    match role {
        PointRole::Construction => tr("point_role.construction"),
        PointRole::Reference => tr("point_role.reference"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pf_core::{demo_project, Project};
    use pf_i18n::{set_lang, Lang};

    #[test]
    fn noms_et_roles_identiques_au_coeur_en_francais() {
        let Ok(project) = demo_project() else {
            panic!("projet de démonstration");
        };
        let sheet = &project.sheets[0];
        for e in &sheet.entities {
            assert_eq!(entity_name(sheet, e.id), sheet.entity_name(e.id));
        }
        for role in PathRole::ALL {
            assert_eq!(role_label(role), role.label_fr());
        }
        assert_eq!(point_role_label(PointRole::Reference), "Repère");
    }

    #[test]
    fn prefixes_traduits_en_anglais() {
        let Ok(project) = demo_project() else {
            panic!("projet de démonstration");
        };
        let sheet = &project.sheets[0];
        set_lang(Lang::En);
        for e in &sheet.entities {
            let name = entity_name(sheet, e.id);
            assert!(
                name.starts_with("Segment ") || name.starts_with("Curve "),
                "{name}"
            );
        }
        assert_eq!(role_label(PathRole::Grainline), "Grainline");
        assert_eq!(core_error(&"boom"), "Error: Boom");
        // Tracé inconnu : pas de panique.
        let empty = Project::new("x");
        assert_eq!(
            entity_name(&empty.sheets[0], EntityId(9999)),
            "Path not found"
        );
    }
}

/// US-19.1 : erreurs du cœur à code stable, traduites par `pf-i18n`.
#[cfg(test)]
mod err_tests {
    use super::*;
    use pf_core::model::{
        DimensionId, FullDimensionId, GroupId, GuideId, LayerId, NotchId, PieceId, PointId, SheetId,
    };
    use pf_i18n::{set_lang, Lang};

    /// Une variante de chaque, avec le numéro de sa position : le `match` sans joker casse la
    /// compilation si on ajoute une variante sans l'ajouter ici.
    fn index(e: &CoreError) -> usize {
        match e {
            CoreError::Geometry(_) => 0,
            CoreError::UnknownSheet(_) => 1,
            CoreError::UnknownPoint(_) => 2,
            CoreError::UnknownEntity(_) => 3,
            CoreError::UnknownNotch(_) => 4,
            CoreError::UnknownDimension(_) => 5,
            CoreError::UnknownFullDimension(_) => 6,
            CoreError::UnknownPiece(_) => 7,
            CoreError::UnknownLayer(_) => 8,
            CoreError::UnknownGroup(_) => 9,
            CoreError::UnknownGuide(_) => 10,
            CoreError::LayerLocked(_) => 11,
            CoreError::NotASegment(_) => 12,
            CoreError::NotACurve(_) => 13,
            CoreError::DeadReference(_) => 14,
            CoreError::CyclicReference(_) => 15,
            CoreError::DuplicateId(_) => 16,
            CoreError::Invalid(_) => 17,
            CoreError::Malformed(_) => 18,
            CoreError::NotAPatternForgeFile => 19,
            CoreError::UnsupportedVersion { .. } => 20,
            CoreError::Io(_) => 21,
        }
    }

    fn geo_index(e: &GeometryError) -> usize {
        match e {
            GeometryError::NonFinite(_) => 0,
            GeometryError::OutOfRange(_) => 1,
            GeometryError::ZeroLengthSegment => 2,
            GeometryError::DegenerateCurve => 3,
            GeometryError::InvalidLength(_) => 4,
            GeometryError::DistanceOutOfSegment { .. } => 5,
            GeometryError::InvalidTolerance(_) => 6,
            GeometryError::ParallelLines => 7,
            GeometryError::ParameterOutOfRange(_) => 8,
            GeometryError::InvalidArc(_) => 9,
            GeometryError::InvalidSpline(_) => 10,
            GeometryError::OverlappingTraces => 11,
            GeometryError::InvalidConstruction(_) => 12,
        }
    }

    fn geo_samples() -> Vec<GeometryError> {
        vec![
            GeometryError::NonFinite("une coordonnée"),
            GeometryError::OutOfRange(2.5e6),
            GeometryError::ZeroLengthSegment,
            GeometryError::DegenerateCurve,
            GeometryError::InvalidLength(-3.5),
            GeometryError::DistanceOutOfSegment {
                distance_mm: 12.25,
                length_mm: 10.0,
            },
            GeometryError::InvalidTolerance(0.0),
            GeometryError::ParallelLines,
            GeometryError::ParameterOutOfRange(1.5),
            GeometryError::InvalidArc("arc impossible".into()),
            GeometryError::InvalidSpline("courbe impossible".into()),
            GeometryError::OverlappingTraces,
            GeometryError::InvalidConstruction("construction impossible".into()),
        ]
    }

    fn samples() -> Vec<CoreError> {
        vec![
            CoreError::Geometry(GeometryError::ParallelLines),
            CoreError::UnknownSheet(SheetId(7)),
            CoreError::UnknownPoint(PointId(8)),
            CoreError::UnknownEntity(pf_core::EntityId(9)),
            CoreError::UnknownNotch(NotchId(10)),
            CoreError::UnknownDimension(DimensionId(11)),
            CoreError::UnknownFullDimension(FullDimensionId(12)),
            CoreError::UnknownPiece(PieceId(13)),
            CoreError::UnknownLayer(LayerId(14)),
            CoreError::UnknownGroup(GroupId(15)),
            CoreError::UnknownGuide(GuideId(16)),
            CoreError::LayerLocked("calque verrouillé".into()),
            CoreError::NotASegment(pf_core::EntityId(17)),
            CoreError::NotACurve(pf_core::EntityId(18)),
            CoreError::DeadReference("le point A".into()),
            CoreError::CyclicReference(PointId(19)),
            CoreError::DuplicateId(20),
            CoreError::Invalid("valeur refusée".into()),
            CoreError::Malformed("accolade attendue".into()),
            CoreError::NotAPatternForgeFile,
            CoreError::UnsupportedVersion {
                found: 9,
                supported: 5,
            },
            CoreError::Io("accès refusé".into()),
        ]
    }

    #[test]
    fn chaque_variante_a_un_exemple_un_code_stable_et_une_cle_dans_les_deux_langues() {
        let list = samples();
        let mut seen: Vec<usize> = list.iter().map(index).collect();
        seen.sort_unstable();
        assert_eq!(
            seen,
            (0..22).collect::<Vec<_>>(),
            "une variante sans exemple"
        );
        let geo = geo_samples();
        let mut seen: Vec<usize> = geo.iter().map(geo_index).collect();
        seen.sort_unstable();
        assert_eq!(seen, (0..13).collect::<Vec<_>>());
        let mut codes: Vec<&str> = list.iter().map(CoreError::code).collect();
        codes.extend(geo.iter().map(GeometryError::code));
        for code in &codes {
            assert!(
                code.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
                "{code}"
            );
        }
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(codes.len(), 22 + 13, "codes en double");
        for lang in Lang::ALL {
            let cat = lang.catalog();
            for e in &list {
                let key = format!("err.core.{}", e.code());
                assert!(
                    cat.iter().any(|(k, _)| *k == key),
                    "{key} ({})",
                    lang.code()
                );
            }
            for e in &geo {
                let key = format!("err.geo.{}", e.code());
                assert!(
                    cat.iter().any(|(k, _)| *k == key),
                    "{key} ({})",
                    lang.code()
                );
            }
        }
    }

    #[test]
    fn les_codes_sont_figes() {
        // Valeurs écrites en dur : renommer un code est une rupture.
        assert_eq!(
            CoreError::NotAPatternForgeFile.code(),
            "not_a_patternforge_file"
        );
        assert_eq!(CoreError::DuplicateId(1).code(), "duplicate_id");
        assert_eq!(GeometryError::ParallelLines.code(), "parallel_lines");
        assert_eq!(
            GeometryError::DistanceOutOfSegment {
                distance_mm: 1.0,
                length_mm: 2.0
            }
            .code(),
            "distance_out_of_segment"
        );
    }

    #[test]
    fn en_francais_le_message_est_identique_a_l_octet_au_coeur() {
        set_lang(Lang::Fr);
        for e in samples() {
            assert_eq!(localized_core(&e), e.to_string(), "{}", e.code());
        }
        for g in geo_samples() {
            assert_eq!(localized_geometry(&g), g.to_string(), "{}", g.code());
            assert_eq!(
                localized_core(&CoreError::Geometry(g.clone())),
                g.to_string()
            );
        }
        // Échantillon écrit à la main.
        assert_eq!(
            localized_core(&CoreError::UnknownPiece(PieceId(13))),
            "pièce introuvable (id 13)"
        );
        assert_eq!(
            localized_geometry(&GeometryError::DistanceOutOfSegment {
                distance_mm: 12.25,
                length_mm: 10.0
            }),
            "distance 12.25 mm en dehors du segment (longueur 10 mm)"
        );
    }

    #[test]
    fn en_anglais_aucun_message_a_modele_francais() {
        set_lang(Lang::En);
        for e in samples() {
            let text = localized_core(&e);
            let detail_free = matches!(&e, CoreError::Geometry(_)) || e.has_free_text();
            if !detail_free {
                assert!(
                    text.chars().all(|c| c.is_ascii() || c == '±'),
                    "{}: {text}",
                    e.code()
                );
                assert_ne!(text, e.to_string(), "{}", e.code());
            }
        }
        for g in geo_samples() {
            let text = localized_geometry(&g);
            if !g.has_free_text() {
                assert!(
                    text.chars().all(|c| c.is_ascii() || c == '±'),
                    "{}: {text}",
                    g.code()
                );
                assert_ne!(text, g.to_string());
            }
        }
        // Variantes à texte libre : le modèle est anglais, seul le détail vient du cœur.
        assert_eq!(
            localized_core(&CoreError::Malformed("accolade attendue".into())),
            "unreadable project file: accolade attendue"
        );
        assert_eq!(
            localized_core(&CoreError::UnknownPiece(PieceId(13))),
            "piece not found (id 13)"
        );
        assert_eq!(
            core_error(&CoreError::NotAPatternForgeFile),
            "Error: This file is not a PatternForge project"
        );
        assert_eq!(
            core_error(&pf_core::SaveError::Core(CoreError::DuplicateId(4))),
            "Error: Duplicate identifier in the project (4)"
        );
        set_lang(Lang::Fr);
    }
}
