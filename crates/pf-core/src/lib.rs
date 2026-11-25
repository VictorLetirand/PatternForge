//! PatternForge — cœur documentaire : projet, feuilles, commandes d'édition,
//! mesures observées et persistance. Aucune dépendance à l'interface.

pub mod align;
pub mod alteration;
pub mod background_image;
pub mod background_ops;
pub mod bom;
pub mod clipboard;
pub mod command;
pub mod contour;
pub mod control_report;
pub mod cutting_auto;
pub mod cutting_layout;
pub mod cutting_plan;
pub mod cutting_yield;
pub mod dependency;
pub mod derived;
pub mod diagnose;
pub mod diagnostic;
pub mod dimension;
pub mod document;
pub mod error;
pub mod error_log;
pub mod expr;
pub mod file_guard;
pub mod finished_measure;
pub mod formula;
pub mod full_dimension;
pub mod grading;
pub mod grading_derived;
pub mod grading_rule;
pub mod guide;
pub mod hide;
pub mod hide_layout;
pub mod hide_ops;
pub mod import_aama;
pub mod import_geometry;
pub mod input;
pub mod kept;
pub mod length_constraint;
pub mod library;
pub mod library_store;
pub mod measure;
pub mod measurement;
pub mod measurement_csv;
pub mod measurement_ops;
pub mod model;
pub mod naming;
pub mod notch;
pub mod notch_check;
pub mod offset;
pub mod persist;
pub mod piece;
pub mod piece_buttons;
pub mod piece_cut;
pub mod piece_dart;
mod piece_dart_transfer;
pub mod piece_flare;
pub mod piece_fold;
pub mod piece_grain;
pub mod piece_internal;
pub mod piece_marking;
pub mod piece_marking_run;
pub mod piece_no_grain;
pub mod piece_plan;
pub mod piece_pleat;
pub mod piece_seam;
pub mod piece_split_join;
pub mod piece_unfold;
pub mod piece_zone;
pub mod recovery;
mod relative;
pub mod seam_junction;
pub mod seam_march;
pub mod seam_pair;
pub mod shape_control;
pub mod sheet_background;
mod sheet_copy;
mod sheet_index;
pub mod size;
pub mod size_measures;
pub mod size_nest;
pub mod size_values;
pub mod stitch_line;
pub mod stitch_pair;
pub mod topstitch;
pub mod units;
pub mod variable;
pub mod variant;
pub mod version_diff;
pub mod versions;

pub use alteration::{AlterationIssue, AlterationReading, AlterationStatus, AlteredValue};
pub use background_ops::{
    build_calibration, calibration_map, calibration_residuals, BackgroundMap, CalibrationSpec,
    Residuals,
};
pub use bom::{Bom, BomLine, BomMaterial};
pub use clipboard::Fragment;
pub use command::{
    Command, CurveHandle, DivisionSpec, Outcome, PieceSource, PointRef, TangentAt,
    CENTERS_LAYER_NAME, MAX_DIVISION_PARTS,
};
pub use cutting_plan::{
    CuttingPlacement, CuttingPlan, CuttingPlanSpec, FabricFold, MAX_FABRIC_WIDTH_MM,
    MAX_MIN_GAP_MM, MAX_PLACEMENT_COORD_MM,
};
pub use dependency::{
    DeletionImpact, DependencyEdge, DependencyEntry, DependencyNature, DependencyObject,
    DependencyTree,
};
pub use derived::{DerivedInfo, DerivedKind, DerivedOp, DerivedSpec, DerivedState};
pub use diagnostic::{
    DiagnosticKind, DiagnosticObject, DiagnosticSeverity, LinkDiagnostic, LinkType,
};
pub use document::{Document, ExternalChange, SaveError};
pub use error::CoreError;
pub use finished_measure::{
    FinishedReading, FinishedReason, FinishedStatus, FinishedTable, FINISHED_EPSILON,
};
pub use formula::{ParamExpr, ParamFormula, ParamKind, ParamTarget, FORMULA_TOLERANCE};
pub use full_dimension::{FullDimension, FullDimensionKind};
pub use grading::{
    GradingCopyMode, GradingError, GradingFollower, PointGrading, GRADING_TOLERANCE_MM,
    MAX_GRADING_MM,
};
pub use grading_derived::{
    ContourDerived, DerivedError, DerivedLimit, NotchDerived, PieceDerived, SizeDerived,
};
pub use grading_rule::{GradingRule, GradingRuleDelta, MAX_GRADING_RULES};
pub use hide::{DefectKind, Hide, HideDefect, QualityZone};
pub use import_aama::{
    locate_notch, AamaNotch, AamaPiece, LocatedNotch, MAX_AAMA_NOTCHES, MAX_AAMA_PIECES,
    MAX_AAMA_VERTICES, NOTCH_FOOT_TOLERANCE_MM,
};
pub use import_geometry::{ImportItem, ImportObject, MAX_IMPORT_LAYERS, MAX_IMPORT_OBJECTS};
pub use kept::KEPT_TOLERANCE_MM;
pub use length_constraint::{
    CurveLength, LengthAdjust, LengthEquality, LengthLink, LengthRef, LengthSpec, LengthTerm,
    LENGTH_TOLERANCE_MM, MAX_IMPOSED_LENGTH_MM, MIN_HANDLE_MM,
};
pub use library::{LibraryContent, LibraryKind, VariableConflict, LIBRARY_FORMAT, LIBRARY_VERSION};
pub use library_store::{
    Library, LibraryEntry, LibraryError, LibraryListing, LibraryProblem, LIBRARY_EXT,
    MAX_LIBRARY_NAME_CHARS,
};
pub use measurement::{
    Alteration, Client, ClientValue, FinishedMeasure, FinishedSource, Measurement, MeasurementUnit,
    MeasurementValue,
};
pub use measurement_csv::{
    export_measurement_csv, read_measurement_csv, CsvEncoding, CsvFormat, CsvLineError,
    MeasurementCsv, MeasurementCsvExport,
};
pub use measurement_ops::{
    ClientSizeProposal, MeasurementImportCell, MeasurementImportRow, SizeCandidate, SizeGap,
    PROPOSAL_TIE_MM,
};
pub use model::*;
pub use naming::{
    NamingChange, NamingCounter, NamingRule, NamingRuleSpec, NamingTarget, MAX_NAMING_NUMBER,
    MAX_NAMING_PART_CHARS, MAX_NAMING_RULES, MAX_NAMING_SEPARATOR_CHARS, MAX_NAMING_START,
    MAX_NAMING_STEP, MAX_NAMING_WIDTH,
};
pub use notch_check::{
    NotchCheckItem, NotchCheckReading, NotchMatch, NOTCH_CHECK_TOLERANCE_MM, NOTCH_SHIFT_RANGE_MM,
};
pub use pf_geometry::hem::{
    DEFAULT_FOLD_NOTCH_STEP_MM, MAX_FOLD_NOTCHES, MAX_FOLD_NOTCH_STEP_MM, MIN_FOLD_NOTCH_STEP_MM,
};
pub use piece_buttons::{
    ButtonElement, ButtonGroup, ButtonShape, ButtonholeOrientation, ResolvedButton,
    ResolvedButtonGroup,
};
pub use piece_cut::{
    CutHalfInfo, CutLine, CutNotches, CutPreview, CUT_DEFAULT_NOTCH_COUNT, CUT_MAX_NOTCHES,
    CUT_STEP_MAX_MM, CUT_STEP_MIN_MM,
};
pub use piece_dart::{DartCalc, PieceDart, ResolvedDart, DART_MAX_PER_PIECE};
pub use piece_flare::{
    flare_even_positions, FlareLineInfo, FlareLineSpec, FlarePreview, FlareSide, PieceFlare,
    FLARE_MAX_LINES, FLARE_MAX_OPENING_MM, FLARE_MIN_OPENING_MM, FLARE_MIN_SPACING_MM,
};
pub use piece_fold::{FoldCut, FoldEdge, FoldLine, FoldSummary};
pub use piece_grain::{
    ExtraGrainLine, GrainDirection, GrainLineSpec, GrainRotation, GrainSense, PieceGrain,
    ResolvedGrainLine, GRAIN_MAX_EXTRA_LINES, GRAIN_MAX_ROTATION_TOLERANCE_DEG,
};
pub use piece_marking::{MarkingAnchor, MarkingShape, PieceMarking, ResolvedMarking};
pub use piece_marking_run::{
    MarkingRunLayout, MarkingRunSpec, RunForm, RunHole, RunSide, RunSpacing, MARKING_RUN_MAX_COUNT,
    MARKING_RUN_MAX_DISTANCE_MM, MARKING_RUN_MAX_STEP_MM, MARKING_RUN_MIN_SPACING_MM,
    MARKING_RUN_MIN_STEP_MM, MARKING_RUN_POINTS_LAYER_NAME,
};
pub use piece_plan::{Placement, PlanItem, PlanSkip};
pub use piece_pleat::{
    FoldedPiece, PiecePleat, PleatArrow, PleatKind, PleatLine, PleatLineKind, PleatMarks,
    ResolvedPleat, PLEAT_MAX_COUNT, PLEAT_MAX_DEPTH_MM, PLEAT_MAX_PER_PIECE, PLEAT_MIN_DEPTH_MM,
};
pub use piece_seam::{
    CornerAllowance, CornerType, CutContour, EdgeAllowance, FoldAllowance, FoldLabelAnchor,
    FoldNotch, HemAllowance, SeamAllowance, SeamDefaults, SeamVertex, FALLBACK_SEAM_MM,
    MAX_CORNER_MM, MAX_SEAM_MM,
};
pub use piece_split_join::{
    HalfInfo, JoinPreview, SplitPreview, DEFAULT_JOIN_TOLERANCE_MM, MAX_JOIN_TOLERANCE_MM,
};
pub use piece_unfold::{FoldCandidate, UnfoldPreview};
pub use piece_zone::{
    PieceZone, ResolvedZone, ZoneKind, ZoneShape, ZoneSpec, ZONE_MAX_PER_PIECE, ZONE_MAX_POINTS,
    ZONE_MAX_WIDTH_MM, ZONE_MIN_AREA_MM2, ZONE_MIN_WIDTH_MM,
};
pub use seam_junction::{
    AssembledJunction, AssembledView, JunctionEnd, JunctionIssue, JunctionReading, JunctionStatus,
    JUNCTION_DEFAULT_TOLERANCE_DEG, JUNCTION_MAX_TOLERANCE_DEG,
};
pub use seam_march::{MarchPreview, MarchedNotch, MARCH_DUPLICATE_TOLERANCE_MM};
pub use seam_pair::{
    SeamIssue, SeamPair, SeamPairReading, SeamSide, SeamStatus, SeamWhich,
    SEAM_DEFAULT_TOLERANCE_MM, SEAM_MAX_PAIRS, SEAM_MAX_VALUE_MM,
};
pub use shape_control::{
    curvature_step, CurvatureStep, CurveKind, CurveShape, CurveSizeShape, DegenerateReason,
    ShapeControl, ShapeControlError, ShapeMetrics, ShapeReading, ShapeSignal, ShapeSignalKind,
    CURVATURE_JUMP_RATIO,
};
pub use sheet_background::{Background, Calibration, PlanePoint};
pub use size::{
    EvaluatedSheet, Size, SizeDependent, SizeError, SizeKind, SizeRange, TextProblem,
    MAX_SIZES_PER_RANGE, MAX_SIZE_LABEL_CHARS, MAX_SIZE_RANGES,
};
pub use size_measures::{
    analyze_series, Irregularity, MeasureGroup, MeasureKind, MeasureStep, MeasureUnit,
    SeriesAnalysis, SizeMeasureRow, SizeMeasures, SizeMeasuresError, IRREGULARITY_MEDIAN_RATIO,
    IRREGULARITY_MIN_SIZES,
};
pub use size_nest::{NestAnchor, NestError, NestOutline, NestOutlineKind, NestSize, SizeNest};
pub use stitch_line::{
    ResolvedStitchLine, StitchHole, StitchLayout, StitchLine, StitchLineSpec, StitchPlaced,
    STITCH_MAX_HOLES, STITCH_MAX_LINES, STITCH_MAX_PITCH_MM, STITCH_MAX_SETBACK_MM,
    STITCH_MIN_PITCH_MM, STITCH_MIN_SETBACK_MM,
};
pub use stitch_pair::{
    StitchCheckItem, StitchCheckReading, StitchIssue, StitchMatch, StitchPair, StitchRef,
    STITCH_CHECK_TOLERANCE_MM,
};
pub use topstitch::{
    ChainKind, Topstitch, TopstitchSide, TopstitchSource, TopstitchState,
    MAX_TOPSTITCHES_PER_SHEET, MAX_TOPSTITCH_MM, MIN_TOPSTITCH_MM,
};
pub use variable::{
    check_variable_name, evaluate_variables, Variable, VariableError, VariableSizeExpr,
    VariableUnit, MAX_VARIABLES, MAX_VARIABLE_NAME_CHARS,
};
pub use variant::{
    Variant, VariantEntry, MAX_VARIANTS, MAX_VARIANT_ENTRIES, MAX_VARIANT_OPTIONS,
    MAX_VARIANT_QUANTITY,
};

/// Pièce d'exemple : rectangle de coupe 170 × 120 mm avec crans (simples et
/// double, côté extérieur), deux cotations, droit-fil et fiche de pièce.
pub fn demo_piece_project() -> Result<Project, CoreError> {
    use pf_geometry::{PointMm, Side};
    let mut p = Project::new("Pièce avec crans et droit-fil");
    let sheet = p.sheets[0].id;
    p.apply(Command::RenameSheet {
        sheet,
        name: "Rabat".into(),
    })?;
    let rect = p.apply(Command::AddRectangle {
        sheet,
        corner: PointRef::New(PointMm::new(0.0, 0.0)),
        width_mm: 170.0,
        height_mm: 120.0,
    })?;
    for &entity in &rect.entities {
        p.apply(Command::SetPathRole {
            sheet,
            entity,
            role: PathRole::CuttingContour,
        })?;
    }
    // Le rectangle est tracé dans le sens anti-horaire : « droite » = extérieur.
    let notch = |entity, distance_mm, style| Command::AddNotch {
        sheet,
        entity,
        distance_mm,
        side: Side::Right,
        length_mm: 5.0,
        style,
    };
    p.apply(notch(rect.entities[0], 40.0, NotchStyle::Single))?;
    p.apply(notch(rect.entities[0], 125.0, NotchStyle::Double))?;
    p.apply(notch(rect.entities[3], 60.0, NotchStyle::Single))?;
    // Cotations (mesures affichées) : largeur sous le rabat, hauteur à l'intérieur.
    for (from, to, offset_mm) in [(0, 1, -15.0), (1, 2, 15.0)] {
        p.apply(Command::AddDimension {
            sheet,
            from: rect.points[from],
            to: rect.points[to],
            offset_mm,
        })?;
    }
    let grain = p.apply(Command::AddSegment {
        sheet,
        from: PointRef::New(PointMm::new(85.0, 20.0)),
        to: PointRef::New(PointMm::new(85.0, 100.0)),
    })?;
    p.apply(Command::SetPathRole {
        sheet,
        entity: grain.entities[0],
        role: PathRole::Grainline,
    })?;
    let piece = p.apply(Command::AddPiece {
        sheet,
        name: "Rabat".into(),
        quantity: 2,
        material: Some("Cuir".into()),
        mirrored_pair: false,
    })?;
    let outline = contour::closed_contours(p.sheet(sheet)?)
        .into_iter()
        .next()
        .map(|c| {
            c.edges
                .iter()
                .map(|e| PieceEdge {
                    entity: e.entity,
                    reversed: e.reversed,
                })
                .collect()
        })
        .unwrap_or_default();
    p.apply(Command::SetPieceOutline {
        piece: piece.pieces[0],
        outline,
        grainline: Some(grain.entities[0]),
    })?;
    Ok(p)
}

/// Projet de démonstration : l'exercice de référence du cahier v0.3
/// (AB = 120 mm, BC ⟂ 80 mm, P à 42 mm, une courbe, trois feuilles).
pub fn demo_project() -> Result<Project, CoreError> {
    use pf_geometry::{PointMm, Side};
    let mut p = Project::new("Exercice de géométrie");
    let devant = p.sheets[0].id;
    p.apply(Command::RenameSheet {
        sheet: devant,
        name: "Devant".into(),
    })?;
    p.apply(Command::CreateSheet {
        name: "Fond".into(),
    })?;
    p.apply(Command::CreateSheet {
        name: "Anse".into(),
    })?;
    let ab = p.apply(Command::AddSegmentPolar {
        sheet: devant,
        from: PointRef::New(PointMm::new(0.0, 0.0)),
        length_mm: 120.0,
        angle_deg: 0.0,
    })?;
    let (a, b) = (ab.points[0], ab.points[1]);
    let bc = p.apply(Command::AddPerpendicular {
        sheet: devant,
        reference: ab.entities[0],
        from: PointRef::Existing(b),
        length_mm: 80.0,
        side: Side::Left,
    })?;
    let on_ab = p.apply(Command::AddPointOnSegment {
        sheet: devant,
        segment: ab.entities[0],
        distance_mm: 42.0,
        origin: SegmentEnd::Start,
    })?;
    p.apply(Command::SetPointLabel {
        sheet: devant,
        point: on_ab.points[0],
        label: "P".into(),
    })?;
    p.apply(Command::AddCurve {
        sheet: devant,
        start: PointRef::Existing(a),
        c1: PointMm::new(-5.0, 45.0),
        c2: PointMm::new(55.0, 95.0),
        end: PointRef::Existing(bc.points[0]),
    })?;
    Ok(p)
}
