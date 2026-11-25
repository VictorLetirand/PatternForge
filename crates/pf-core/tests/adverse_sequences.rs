//! US-17.6 - séquences aléatoires de commandes (PRNG maison, graines fixes).

mod adverse_support;
use adverse_support::*;

macro_rules! campagne_test {
    ($name:ident, $seed:expr) => {
        #[test]
        fn $name() {
            let stats = campagne($seed, 60);
            report(stringify!($name), &stats);
            assert_eq!(stats.sequences, 60);
            assert_coverage(&stats);
        }
    };
}

campagne_test!(sequences_aleatoires_graines_1000, 1000);

campagne_test!(sequences_aleatoires_graines_2000, 2000);

campagne_test!(sequences_aleatoires_graines_3000, 3000);

campagne_test!(sequences_aleatoires_graines_4000, 4000);

const ARC_VARIANTS: [&str; 7] = [
    "AddCircle",
    "AddCircleThreePoints",
    "AddCircleTwoPointsRadius",
    "AddArc",
    "AddArcThreePoints",
    "AddArcTwoPointsRadius",
    "SetCircleRadius",
];

/// US-02.1 : séquences riches en cercles, arcs, déplacements, transformations,
/// copies et suppressions ; mêmes propriétés que les autres campagnes (jamais de
/// panic, état identique après refus, annuler/rétablir exacts, export sans NaN).
macro_rules! campagne_arcs_test {
    ($name:ident, $seed:expr) => {
        #[test]
        fn $name() {
            let stats = campagne_avec($seed, 60, true);
            report(stringify!($name), &stats);
            assert_eq!(stats.sequences, 60);
            for v in ARC_VARIANTS {
                assert!(stats.attempted.contains_key(v), "variant {v} jamais tenté");
            }
            for v in [
                "AddCircle",
                "AddArc",
                "AddArcThreePoints",
                "AddCircleThreePoints",
            ] {
                assert!(
                    stats.accepted.get(v).copied().unwrap_or(0) > 0,
                    "variant {v} jamais accepté : couverture insuffisante"
                );
            }
        }
    };
}

campagne_arcs_test!(sequences_cercles_et_arcs_graines_5000, 5000);

campagne_arcs_test!(sequences_cercles_et_arcs_graines_6000, 6000);

campagne_arcs_test!(sequences_cercles_et_arcs_graines_7000, 7000);

const SPLINE_VARIANTS: [&str; 6] = [
    "AddSpline",
    "AddSplinePoint",
    "RemoveSplinePoint",
    "SetSplineNode",
    "CurveToSpline",
    "SplineToCurve",
];

/// US-02.2 : séquences riches en courbes par points de passage mêlées aux
/// cercles, arcs, transformations, copies, calques et suppressions ; mêmes
/// propriétés que les autres campagnes.
macro_rules! campagne_splines_test {
    ($name:ident, $seed:expr) => {
        #[test]
        fn $name() {
            let stats = campagne_mode($seed, 60, true, true);
            report(stringify!($name), &stats);
            assert_eq!(stats.sequences, 60);
            for v in SPLINE_VARIANTS {
                assert!(stats.attempted.contains_key(v), "variant {v} jamais tenté");
            }
            for v in [
                "AddSpline",
                "AddSplinePoint",
                "RemoveSplinePoint",
                "SetSplineNode",
                "CurveToSpline",
                "SplineToCurve",
            ] {
                assert!(
                    stats.accepted.get(v).copied().unwrap_or(0) > 0,
                    "variant {v} jamais accepté : couverture insuffisante"
                );
            }
        }
    };
}

campagne_splines_test!(sequences_courbes_lisses_graines_8000, 8000);

campagne_splines_test!(sequences_courbes_lisses_graines_9000, 9000);

campagne_splines_test!(sequences_courbes_lisses_graines_10000, 10000);

const TANGENCY_VARIANTS: [&str; 4] = [
    "FilletPaths",
    "ChamferPaths",
    "AddTangentLine",
    "AddTangentAt",
];

/// US-02.3 : séquences riches en congés, chanfreins et tangentes mêlés aux
/// cercles, arcs, courbes lisses, transformations, copies, calques, groupes,
/// crans et suppressions ; mêmes propriétés que les autres campagnes (jamais de
/// panic, état identique après refus, annuler/rétablir exacts, export sans NaN).
macro_rules! campagne_tangence_test {
    ($name:ident, $seed:expr) => {
        #[test]
        fn $name() {
            let stats = campagne_complete($seed, 60, true, true, true, false);
            report(stringify!($name), &stats);
            assert_eq!(stats.sequences, 60);
            for v in TANGENCY_VARIANTS {
                assert!(stats.attempted.contains_key(v), "variant {v} jamais tenté");
                assert!(
                    stats.accepted.get(v).copied().unwrap_or(0) > 0,
                    "variant {v} jamais accepté : couverture insuffisante"
                );
            }
        }
    };
}

campagne_tangence_test!(sequences_tangence_graines_11000, 11000);

campagne_tangence_test!(sequences_tangence_graines_12000, 12000);

campagne_tangence_test!(sequences_tangence_graines_13000, 13000);

/// US-02.4 : séquences riches en intersections générales (courbes lisses, Béziers,
/// cercles, arcs, segments) mêlées aux déplacements, transformations, copies,
/// calques et suppressions. Mêmes propriétés : jamais de panic, état identique après
/// refus (dont « les tracés ne se coupent plus »), annuler/rétablir exacts, export sans NaN.
macro_rules! campagne_intersections_test {
    ($name:ident, $seed:expr) => {
        #[test]
        fn $name() {
            let stats = campagne_complete($seed, 60, true, true, false, true);
            report(stringify!($name), &stats);
            assert_eq!(stats.sequences, 60);
            assert!(
                stats.attempted.contains_key("TraceIntersection"),
                "intersection jamais tentée"
            );
            assert!(
                stats
                    .accepted
                    .get("TraceIntersection")
                    .copied()
                    .unwrap_or(0)
                    > 0,
                "intersection jamais acceptée : couverture insuffisante"
            );
        }
    };
}

campagne_intersections_test!(sequences_intersections_graines_14000, 14000);

campagne_intersections_test!(sequences_intersections_graines_15000, 15000);

campagne_intersections_test!(sequences_intersections_graines_16000, 16000);

/// US-02.5 : séquences riches en points relatifs (distance sur courbe, dx/dy,
/// milieu, projection, bissectrice, direction, compas) mêlés aux déplacements,
/// transformations, copies, calques, suppressions et paramètres. Mêmes
/// propriétés : jamais de panic, état identique après refus, annuler/rétablir
/// exacts, relecture idempotente, export sans NaN.
macro_rules! campagne_relatifs_test {
    ($name:ident, $seed:expr) => {
        #[test]
        fn $name() {
            let stats = campagne_relative($seed, 60, true, true, false, true, true);
            report(stringify!($name), &stats);
            assert_eq!(stats.sequences, 60);
            assert!(
                stats.attempted.contains_key("PointRelatif"),
                "point relatif jamais tenté"
            );
            assert!(
                stats.accepted.get("PointRelatif").copied().unwrap_or(0) > 0,
                "point relatif jamais accepté : couverture insuffisante"
            );
            assert!(
                stats.attempted.contains_key("SetPointValues"),
                "SetPointValues jamais tenté"
            );
        }
    };
}

campagne_relatifs_test!(sequences_points_relatifs_graines_17000, 17000);

campagne_relatifs_test!(sequences_points_relatifs_graines_18000, 18000);

campagne_relatifs_test!(sequences_points_relatifs_graines_19000, 19000);

/// US-02.6 : séquences riches en divisions (N parts, proportions, point de
/// départ de cercle) mêlées à tous les autres générateurs. Mêmes propriétés :
/// jamais de panic, état identique après refus, annuler/rétablir exacts,
/// relecture idempotente, export sans NaN.
macro_rules! campagne_divisions_test {
    ($name:ident, $seed:expr) => {
        #[test]
        fn $name() {
            let stats = campagne_divisions($seed, 60);
            report(stringify!($name), &stats);
            assert_eq!(stats.sequences, 60);
            assert!(
                stats.attempted.contains_key("DivideTrace"),
                "division jamais tentée"
            );
            assert!(
                stats.accepted.get("DivideTrace").copied().unwrap_or(0) > 0,
                "division jamais acceptée : couverture insuffisante"
            );
        }
    };
}

campagne_divisions_test!(sequences_divisions_graines_20000, 20000);

campagne_divisions_test!(sequences_divisions_graines_21000, 21000);

campagne_divisions_test!(sequences_divisions_graines_22000, 22000);

const GUIDE_VARIANTS: [&str; 4] = ["AddGuide", "MoveGuide", "DeleteGuide", "SetGuideAxis"];

/// US-02.8 : séquences mêlant lignes d'aide (création, déplacement, axe,
/// suppression), copies de feuille, presse-papiers et tous les autres
/// générateurs. Mêmes propriétés : pas de panic, état identique après refus,
/// annuler/rétablir exacts, relecture idempotente.
macro_rules! campagne_guides_test {
    ($name:ident, $seed:expr) => {
        #[test]
        fn $name() {
            let stats = campagne_guides($seed, 60);
            report(stringify!($name), &stats);
            assert_eq!(stats.sequences, 60);
            for v in GUIDE_VARIANTS {
                assert!(stats.attempted.contains_key(v), "variant {v} jamais tenté");
                assert!(
                    stats.accepted.get(v).copied().unwrap_or(0) > 0,
                    "variant {v} jamais accepté : couverture insuffisante"
                );
            }
        }
    };
}

campagne_guides_test!(sequences_lignes_d_aide_graines_26000, 26000);

campagne_guides_test!(sequences_lignes_d_aide_graines_27000, 27000);

/// US-02.10 : séquences riches en décalages de contour (chaînes détectées ou
/// quelconques, distances extrêmes, coins pointe/arrondi) mêlées à tous les
/// autres générateurs. Mêmes propriétés : pas de panic, état identique après
/// refus, annuler/rétablir exacts, relecture idempotente.
macro_rules! campagne_decalages_test {
    ($name:ident, $seed:expr) => {
        #[test]
        fn $name() {
            let stats = campagne_offsets($seed, 40);
            report(stringify!($name), &stats);
            assert_eq!(stats.sequences, 40);
            assert!(
                stats.attempted.contains_key("OffsetPaths"),
                "décalage jamais tenté"
            );
            assert!(
                stats.accepted.get("OffsetPaths").copied().unwrap_or(0) > 0,
                "décalage jamais accepté : couverture insuffisante"
            );
        }
    };
}

campagne_decalages_test!(sequences_decalages_graines_28000, 28000);

campagne_decalages_test!(sequences_decalages_graines_29000, 29000);

const PATH_VARIANTS: [&str; 5] = [
    "ExtendPath",
    "TrimPathTo",
    "SplitPath",
    "MergePaths",
    "ReversePath",
];

/// US-02.7 : séquences riches en prolongements, raccourcissements, coupes,
/// fusions et inversions mêlés à tous les autres générateurs. Mêmes propriétés :
/// jamais de panic, état identique après refus, annuler/rétablir exacts,
/// relecture idempotente, export sans NaN.
macro_rules! campagne_chemins_test {
    ($name:ident, $seed:expr) => {
        #[test]
        fn $name() {
            let stats = campagne_paths($seed, 60);
            report(stringify!($name), &stats);
            assert_eq!(stats.sequences, 60);
            for v in PATH_VARIANTS {
                assert!(stats.attempted.contains_key(v), "variant {v} jamais tenté");
                assert!(
                    stats.accepted.get(v).copied().unwrap_or(0) > 0,
                    "variant {v} jamais accepté : couverture insuffisante"
                );
            }
        }
    };
}

campagne_chemins_test!(sequences_modifier_un_trace_graines_23000, 23000);

campagne_chemins_test!(sequences_modifier_un_trace_graines_24000, 24000);

campagne_chemins_test!(sequences_modifier_un_trace_graines_25000, 25000);

/// US-02.11 : séquences mêlant symétries liées, dépliage, lignes d'aide, copies
/// de feuille, presse-papiers et tous les autres générateurs. Mêmes propriétés :
/// pas de panic, état identique après refus, annuler/rétablir exacts, relecture
/// idempotente.
macro_rules! campagne_mirrors_test {
    ($name:ident, $seed:expr) => {
        #[test]
        fn $name() {
            let stats = campagne_mirrors($seed, 60);
            report(stringify!($name), &stats);
            assert_eq!(stats.sequences, 60);
            assert!(
                stats.attempted.contains_key("LinkMirror"),
                "symétrie jamais tentée"
            );
            assert!(
                stats.accepted.get("LinkMirror").copied().unwrap_or(0) > 0,
                "symétrie jamais acceptée : couverture insuffisante"
            );
        }
    };
}

campagne_mirrors_test!(sequences_symetries_liees_graines_30000, 30000);

campagne_mirrors_test!(sequences_symetries_liees_graines_31000, 31000);
