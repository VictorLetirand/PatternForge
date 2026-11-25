//! Couverture des commandes du modèle par le serveur MCP (US-18.1).
//!
//! `classify` est un `match` **sans joker** sur `pf_core::Command` : ajouter une variante casse la
//! compilation de ce fichier tant qu'elle n'est pas classée ici (exposée par un outil, ou exclue avec
//! une raison écrite). Les autres tests recoupent cette table avec le catalogue de `list_commands`
//! et avec la liste que `serde` donne pour `Command`.

use pf_core::Command;
use serde_json::{json, Value};

#[allow(dead_code)]
enum Class {
    /// Acceptée par `apply_command` / `preview_command` (JSON serde) ; pas d'outil dédié.
    Generic,
    /// Acceptée aussi par un outil de haut niveau qui la compose.
    Dedicated(&'static str),
    /// Non exposée, avec la raison (aucune aujourd'hui : `apply_command` prend toute variante).
    Excluded(&'static str),
}

fn classify(c: &Command) -> (&'static str, Class) {
    match c {
        Command::RenameProject { .. } => ("rename_project", Class::Generic),
        Command::CreateSheet { .. } => ("create_sheet", Class::Generic),
        Command::RenameSheet { .. } => ("rename_sheet", Class::Generic),
        Command::DeleteSheet { .. } => ("delete_sheet", Class::Generic),
        Command::MoveSheet { .. } => ("move_sheet", Class::Generic),
        Command::DuplicateSheet { .. } => ("duplicate_sheet", Class::Generic),
        Command::ImportSheet { .. } => ("import_sheet", Class::Generic),
        Command::AddPiece { .. } => ("add_piece", Class::Generic),
        Command::SetPieceInfo { .. } => ("set_piece_info", Class::Generic),
        Command::SetPieceCard { .. } => ("set_piece_card", Class::Generic),
        Command::DeletePiece { .. } => ("delete_piece", Class::Generic),
        Command::SetPiecePlacement { .. } => ("set_piece_placement", Class::Generic),
        Command::SetPieceOutline { .. } => ("set_piece_outline", Class::Generic),
        Command::SetPieceGrain { .. } => ("set_piece_grain", Class::Generic),
        Command::SetSeamAllowance { .. } => (
            "set_seam_allowance",
            Class::Dedicated("apply_seam_allowance"),
        ),
        Command::SetNetCut { .. } => ("set_net_cut", Class::Generic),
        Command::SetSeamDefaults { .. } => ("set_seam_defaults", Class::Generic),
        Command::AddPieceInternal { .. } => ("add_piece_internal", Class::Generic),
        Command::RemovePieceInternal { .. } => ("remove_piece_internal", Class::Generic),
        Command::SetPieceInternalKind { .. } => ("set_piece_internal_kind", Class::Generic),
        Command::ImportPieceInternals { .. } => ("import_piece_internals", Class::Generic),
        Command::AddPieceMarking { .. } => ("add_piece_marking", Class::Generic),
        Command::AddPieceMarkingRun { .. } => ("add_piece_marking_run", Class::Generic),
        Command::AddStitchLine { .. } => ("add_stitch_line", Class::Generic),
        Command::SetStitchLine { .. } => ("set_stitch_line", Class::Generic),
        Command::RemoveStitchLine { .. } => ("remove_stitch_line", Class::Generic),
        Command::MergeStitchLines { .. } => ("merge_stitch_lines", Class::Generic),
        Command::PairStitchLines { .. } => ("pair_stitch_lines", Class::Generic),
        Command::UnpairStitchLines { .. } => ("unpair_stitch_lines", Class::Generic),
        Command::CarryStitchLine { .. } => ("carry_stitch_line", Class::Generic),
        Command::AddPieceZone { .. } => ("add_piece_zone", Class::Generic),
        Command::SetPieceZone { .. } => ("set_piece_zone", Class::Generic),
        Command::RemovePieceZone { .. } => ("remove_piece_zone", Class::Generic),
        Command::RemovePieceMarking { .. } => ("remove_piece_marking", Class::Generic),
        Command::SetPieceMarking { .. } => ("set_piece_marking", Class::Generic),
        Command::AddPieceDart { .. } => ("add_piece_dart", Class::Generic),
        Command::AddSeamPair { .. } => ("add_seam_pair", Class::Generic),
        Command::SetSeamPair { .. } => ("set_seam_pair", Class::Generic),
        Command::RemoveSeamPair { .. } => ("remove_seam_pair", Class::Generic),
        Command::MarchNotches { .. } => ("march_notches", Class::Generic),
        Command::RemovePieceDart { .. } => ("remove_piece_dart", Class::Generic),
        Command::SetPieceDart { .. } => ("set_piece_dart", Class::Generic),
        Command::SetPieceDartCutClosed { .. } => ("set_piece_dart_cut_closed", Class::Generic),
        Command::SetPieceFold { .. } => ("set_piece_fold", Class::Generic),
        Command::SetPieceNoGrain { .. } => ("set_piece_no_grain", Class::Generic),
        Command::SetBackground { .. } => ("set_background", Class::Generic),
        Command::CalibrateBackground { .. } => ("calibrate_background", Class::Generic),
        Command::SetBackgroundDisplay { .. } => ("set_background_display", Class::Generic),
        Command::RelinkBackground { .. } => ("relink_background", Class::Generic),
        Command::RemoveBackground { .. } => ("remove_background", Class::Generic),
        Command::AddVariable { .. } => ("add_variable", Class::Generic),
        Command::SetVariableExpr { .. } => ("set_variable_expr", Class::Generic),
        Command::SetVariableUnit { .. } => ("set_variable_unit", Class::Generic),
        Command::RenameVariable { .. } => ("rename_variable", Class::Generic),
        Command::RemoveVariable { .. } => ("remove_variable", Class::Generic),
        Command::SetVariableSizeExpr { .. } => ("set_variable_size_expr", Class::Generic),
        Command::ClearVariableSizeExpr { .. } => ("clear_variable_size_expr", Class::Generic),
        Command::AddMeasurement { .. } => ("add_measurement", Class::Generic),
        Command::RenameMeasurement { .. } => ("rename_measurement", Class::Generic),
        Command::RemoveMeasurement { .. } => ("remove_measurement", Class::Generic),
        Command::SetMeasurementValue { .. } => ("set_measurement_value", Class::Generic),
        Command::ClearMeasurementValue { .. } => ("clear_measurement_value", Class::Generic),
        Command::ImportMeasurements { .. } => ("import_measurements", Class::Generic),
        Command::AddClient { .. } => ("add_client", Class::Generic),
        Command::RenameClient { .. } => ("rename_client", Class::Generic),
        Command::RemoveClient { .. } => ("remove_client", Class::Generic),
        Command::SetClientValue { .. } => ("set_client_value", Class::Generic),
        Command::ClearClientValue { .. } => ("clear_client_value", Class::Generic),
        Command::GenerateClientSize { .. } => ("generate_client_size", Class::Generic),
        Command::AddAlteration { .. } => ("add_alteration", Class::Generic),
        Command::RenameAlteration { .. } => ("rename_alteration", Class::Generic),
        Command::RemoveAlteration { .. } => ("remove_alteration", Class::Generic),
        Command::SetAlterationTarget { .. } => ("set_alteration_target", Class::Generic),
        Command::SetAlterationDelta { .. } => ("set_alteration_delta", Class::Generic),
        Command::AddFinishedMeasure { .. } => ("add_finished_measure", Class::Generic),
        Command::SetFinishedMeasure { .. } => ("set_finished_measure", Class::Generic),
        Command::RemoveFinishedMeasure { .. } => ("remove_finished_measure", Class::Generic),
        Command::AddCuttingPlan { .. } => ("add_cutting_plan", Class::Generic),
        Command::AddVariant { .. } => ("add_variant", Class::Generic),
        Command::AddNamingRule { .. } => ("add_naming_rule", Class::Generic),
        Command::UpdateNamingRule { .. } => ("update_naming_rule", Class::Generic),
        Command::RemoveNamingRule { .. } => ("remove_naming_rule", Class::Generic),
        Command::ApplyNamingRule { .. } => ("apply_naming_rule", Class::Generic),
        Command::RenameVariant { .. } => ("rename_variant", Class::Generic),
        Command::RemoveVariant { .. } => ("remove_variant", Class::Generic),
        Command::DuplicateVariant { .. } => ("duplicate_variant", Class::Generic),
        Command::AddVariantEntry { .. } => ("add_variant_entry", Class::Generic),
        Command::UpdateVariantEntry { .. } => ("update_variant_entry", Class::Generic),
        Command::RemoveVariantEntry { .. } => ("remove_variant_entry", Class::Generic),
        Command::AddVariantOption { .. } => ("add_variant_option", Class::Generic),
        Command::RemoveVariantOption { .. } => ("remove_variant_option", Class::Generic),
        Command::RenameVariantOption { .. } => ("rename_variant_option", Class::Generic),
        Command::MoveVariantOption { .. } => ("move_variant_option", Class::Generic),
        Command::SetVariantOptionActive { .. } => ("set_variant_option_active", Class::Generic),
        Command::UpdateCuttingPlan { .. } => ("update_cutting_plan", Class::Generic),
        Command::RemoveCuttingPlan { .. } => ("remove_cutting_plan", Class::Generic),
        Command::AddPlacement { .. } => ("add_placement", Class::Generic),
        Command::UpdatePlacement { .. } => ("update_placement", Class::Generic),
        Command::RemovePlacement { .. } => ("remove_placement", Class::Generic),
        Command::SetHideContour { .. } => ("set_hide_contour", Class::Generic),
        Command::RemoveHide { .. } => ("remove_hide", Class::Generic),
        Command::AddHideDefect { .. } => ("add_hide_defect", Class::Generic),
        Command::UpdateHideDefect { .. } => ("update_hide_defect", Class::Generic),
        Command::RemoveHideDefect { .. } => ("remove_hide_defect", Class::Generic),
        Command::AddQualityZone { .. } => ("add_quality_zone", Class::Generic),
        Command::UpdateQualityZone { .. } => ("update_quality_zone", Class::Generic),
        Command::RemoveQualityZone { .. } => ("remove_quality_zone", Class::Generic),
        Command::AddSizeRange { .. } => ("add_size_range", Class::Generic),
        Command::RenameSizeRange { .. } => ("rename_size_range", Class::Generic),
        Command::RemoveSizeRange { .. } => ("remove_size_range", Class::Generic),
        Command::AddSize { .. } => ("add_size", Class::Generic),
        Command::RemoveSize { .. } => ("remove_size", Class::Generic),
        Command::RenameSize { .. } => ("rename_size", Class::Generic),
        Command::MoveSize { .. } => ("move_size", Class::Generic),
        Command::SetSizeBreak { .. } => ("set_size_break", Class::Generic),
        Command::SetBaseSize { .. } => ("set_base_size", Class::Generic),
        Command::SetPointGrading { .. } => ("set_point_grading", Class::Generic),
        Command::ClearPointGrading { .. } => ("clear_point_grading", Class::Generic),
        Command::CopyPointGrading { .. } => ("copy_point_grading", Class::Generic),
        Command::InterpolatePointGrading { .. } => ("interpolate_point_grading", Class::Generic),
        Command::SetParameterExpr { .. } => ("set_parameter_expr", Class::Generic),
        Command::SetConstructionKept { .. } => ("set_construction_kept", Class::Generic),
        Command::SetCurveLengthConstraint { .. } => ("set_curve_length_constraint", Class::Generic),
        Command::SetCurveLengthExpr { .. } => ("set_curve_length_expr", Class::Generic),
        Command::SetCurveLengthEquality { .. } => ("set_curve_length_equality", Class::Generic),
        Command::TransferPieceDart { .. } => ("transfer_piece_dart", Class::Generic),
        Command::AddPiecePleats { .. } => ("add_piece_pleats", Class::Generic),
        Command::AddPieceFlare { .. } => ("add_piece_flare", Class::Generic),
        Command::AddButtonGroup { .. } => ("add_button_group", Class::Generic),
        Command::SetButtonGroup { .. } => ("set_button_group", Class::Generic),
        Command::RemoveButtonGroup { .. } => ("remove_button_group", Class::Generic),
        Command::CreateDerivedPiece { .. } => ("create_derived_piece", Class::Generic),
        Command::BreakDerivedLink { .. } => ("break_derived_link", Class::Generic),
        Command::UnfoldPiece { .. } => ("unfold_piece", Class::Generic),
        Command::ExtractPiece { .. } => ("extract_piece", Class::Dedicated("extract_piece")),
        Command::SplitPiece { .. } => ("split_piece", Class::Generic),
        Command::CutPiece { .. } => ("cut_piece", Class::Generic),
        Command::JoinPieces { .. } => ("join_pieces", Class::Generic),
        Command::AddPoint { .. } => ("add_point", Class::Generic),
        Command::AddPointAt { .. } => ("add_point_at", Class::Generic),
        Command::AddPointWithFormulas { .. } => ("add_point_with_formulas", Class::Generic),
        Command::AddSegment { .. } => ("add_segment", Class::Generic),
        Command::AddSegmentPolar { .. } => ("add_segment_polar", Class::Generic),
        Command::AddPerpendicular { .. } => ("add_perpendicular", Class::Generic),
        Command::AddRectangle { .. } => ("add_rectangle", Class::Generic),
        Command::AddConstructedRectangle { .. } => ("add_constructed_rectangle", Class::Generic),
        Command::AddPolygon { .. } => ("add_polygon", Class::Generic),
        Command::AddOrientedRectangle { .. } => ("add_oriented_rectangle", Class::Generic),
        Command::AddRoundedRectangle { .. } => ("add_rounded_rectangle", Class::Generic),
        Command::AddParallel { .. } => ("add_parallel", Class::Generic),
        Command::AddCircle { .. } => ("add_circle", Class::Generic),
        Command::AddCircleThreePoints { .. } => ("add_circle_three_points", Class::Generic),
        Command::AddCircleTwoPointsRadius { .. } => {
            ("add_circle_two_points_radius", Class::Generic)
        }
        Command::AddArc { .. } => ("add_arc", Class::Generic),
        Command::AddArcThreePoints { .. } => ("add_arc_three_points", Class::Generic),
        Command::AddArcTwoPointsRadius { .. } => ("add_arc_two_points_radius", Class::Generic),
        Command::SetCircleRadius { .. } => ("set_circle_radius", Class::Generic),
        Command::FilletPaths { .. } => ("fillet_paths", Class::Generic),
        Command::ChamferPaths { .. } => ("chamfer_paths", Class::Generic),
        Command::AddTangentLine { .. } => ("add_tangent_line", Class::Generic),
        Command::AddTangentAt { .. } => ("add_tangent_at", Class::Generic),
        Command::Transform { .. } => ("transform", Class::Generic),
        Command::Duplicate { .. } => ("duplicate", Class::Generic),
        Command::AddPointOnSegment { .. } => ("add_point_on_segment", Class::Generic),
        Command::AddNotch { .. } => ("add_notch", Class::Generic),
        Command::SetNotch { .. } => ("set_notch", Class::Generic),
        Command::DeleteNotch { .. } => ("delete_notch", Class::Generic),
        Command::AddAnchoredNotch { .. } => ("add_anchored_notch", Class::Generic),
        Command::SetNotchAnchor { .. } => ("set_notch_anchor", Class::Generic),
        Command::SetNotchShape { .. } => ("set_notch_shape", Class::Generic),
        Command::SetNotchCorner { .. } => ("set_notch_corner", Class::Generic),
        Command::SetNotchProportional { .. } => ("set_notch_proportional", Class::Generic),
        Command::AddDimension { .. } => ("add_dimension", Class::Generic),
        Command::SetDimensionOffset { .. } => ("set_dimension_offset", Class::Generic),
        Command::DeleteDimension { .. } => ("delete_dimension", Class::Generic),
        Command::AddFullDimension { .. } => ("add_full_dimension", Class::Generic),
        Command::SetFullDimensionPlacement { .. } => {
            ("set_full_dimension_placement", Class::Generic)
        }
        Command::SetFullDimensionLabel { .. } => ("set_full_dimension_label", Class::Generic),
        Command::SetFullDimensionTextShift { .. } => {
            ("set_full_dimension_text_shift", Class::Generic)
        }
        Command::DeleteFullDimension { .. } => ("delete_full_dimension", Class::Generic),
        Command::AddGuide { .. } => ("add_guide", Class::Generic),
        Command::MoveGuide { .. } => ("move_guide", Class::Generic),
        Command::SetGuideAxis { .. } => ("set_guide_axis", Class::Generic),
        Command::DeleteGuide { .. } => ("delete_guide", Class::Generic),
        Command::OffsetPaths { .. } => ("offset_paths", Class::Generic),
        Command::LinkMirror { .. } => ("link_mirror", Class::Generic),
        Command::UnfoldMirror { .. } => ("unfold_mirror", Class::Generic),
        Command::Batch { .. } => ("batch", Class::Generic),
        Command::AddCurve { .. } => ("add_curve", Class::Generic),
        Command::AddSpline { .. } => ("add_spline", Class::Generic),
        Command::AddSplinePoint { .. } => ("add_spline_point", Class::Generic),
        Command::RemoveSplinePoint { .. } => ("remove_spline_point", Class::Generic),
        Command::SetSplineNode { .. } => ("set_spline_node", Class::Generic),
        Command::CurveToSpline { .. } => ("curve_to_spline", Class::Generic),
        Command::SplineToCurve { .. } => ("spline_to_curve", Class::Generic),
        Command::MovePoint { .. } => ("move_point", Class::Generic),
        Command::SetPointDistance { .. } => ("set_point_distance", Class::Generic),
        Command::SetPointParameter { .. } => ("set_point_parameter", Class::Generic),
        Command::SetPointValues { .. } => ("set_point_values", Class::Generic),
        Command::DivideTrace { .. } => ("divide_trace", Class::Generic),
        Command::ExtendPath { .. } => ("extend_path", Class::Generic),
        Command::TrimPathTo { .. } => ("trim_path_to", Class::Generic),
        Command::SplitPath { .. } => ("split_path", Class::Generic),
        Command::MergePaths { .. } => ("merge_paths", Class::Generic),
        Command::ReversePath { .. } => ("reverse_path", Class::Generic),
        Command::MoveCurveHandle { .. } => ("move_curve_handle", Class::Generic),
        Command::SetPointLabel { .. } => ("set_point_label", Class::Generic),
        Command::SetPointRole { .. } => ("set_point_role", Class::Generic),
        Command::SetPathRole { .. } => ("set_path_role", Class::Generic),
        Command::AddTopstitch { .. } => ("add_topstitch", Class::Generic),
        Command::SetTopstitch { .. } => ("set_topstitch", Class::Generic),
        Command::BreakTopstitchLink { .. } => ("break_topstitch_link", Class::Generic),
        Command::Delete { .. } => ("delete", Class::Generic),
        Command::AddLayer { .. } => ("add_layer", Class::Generic),
        Command::RenameLayer { .. } => ("rename_layer", Class::Generic),
        Command::SetLayerVisible { .. } => ("set_layer_visible", Class::Generic),
        Command::SetLayerLocked { .. } => ("set_layer_locked", Class::Generic),
        Command::MoveLayer { .. } => ("move_layer", Class::Generic),
        Command::DeleteLayer { .. } => ("delete_layer", Class::Generic),
        Command::SetObjectsLayer { .. } => ("set_objects_layer", Class::Generic),
        Command::SetEntityName { .. } => ("set_entity_name", Class::Generic),
        Command::CreateGroup { .. } => ("create_group", Class::Generic),
        Command::Ungroup { .. } => ("ungroup", Class::Generic),
        Command::PasteFragment { .. } => ("paste_fragment", Class::Generic),
        Command::InsertLibrary { .. } => ("insert_library", Class::Generic),
        Command::ImportGeometry { .. } => ("import_geometry", Class::Generic),
        Command::ImportGeometryNewSheet { .. } => ("import_geometry_new_sheet", Class::Generic),
        Command::ImportAamaPieces { .. } => ("import_aama_pieces", Class::Generic),
        Command::InLayer { .. } => ("in_layer", Class::Generic),
    }
}

fn server() -> (std::path::PathBuf, pf_mcp::Server) {
    use std::sync::atomic::{AtomicU32, Ordering};
    static N: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        "pf-mcp-cov-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).expect("dossier");
    let path = dir.join("demo.pforge.json");
    pf_core::persist::save(&path, &pf_core::demo_project().expect("démo")).expect("projet");
    (dir, pf_mcp::Server::with_write(path))
}

fn call(s: &mut pf_mcp::Server, method: &str, params: Value) -> Value {
    let req = json!({"jsonrpc":"2.0","id":1,"method":method,"params":params});
    let line = s.handle_line(&req.to_string()).expect("réponse");
    serde_json::from_str(&line).expect("JSON")
}

fn declared() -> Vec<(String, String)> {
    let src = include_str!("command_coverage.rs");
    let start = src.find("match c {").expect("match de classify");
    let end = src.find("fn server()").expect("fin de classify");
    src[start..end]
        .split("Command::")
        .skip(1)
        .map(|chunk| {
            let after = chunk.split("=>").nth(1).expect("nom");
            let name = after
                .split('"')
                .nth(1)
                .expect("nom de variante")
                .to_string();
            let class = match chunk.split("Class::Dedicated(\"").nth(1) {
                Some(t) => t.split('"').next().expect("outil").to_string(),
                None => String::new(),
            };
            (name, class)
        })
        .collect()
}

fn sorted(mut v: Vec<String>) -> Vec<String> {
    v.sort();
    v
}

#[test]
fn every_variant_is_classified_and_listed() {
    let declared = sorted(declared().into_iter().map(|(n, _)| n).collect());
    let catalog = sorted(
        pf_mcp::command_names()
            .iter()
            .map(|s| s.to_string())
            .collect(),
    );
    assert_eq!(declared.len(), 229, "nombre de variantes de Command");
    assert_eq!(declared, catalog);
    let mut dedup = declared.clone();
    dedup.dedup();
    assert_eq!(dedup.len(), declared.len(), "aucun doublon");
}

#[test]
fn serde_agrees_with_the_catalog() {
    let err = serde_json::from_value::<Command>(json!({"type": "__inconnue__"}))
        .expect_err("variante inconnue")
        .to_string();
    let expected = err.split("expected one of").nth(1).expect("liste serde");
    let from_serde: Vec<String> = expected
        .split('`')
        .skip(1)
        .step_by(2)
        .map(str::to_string)
        .collect();
    let catalog = pf_mcp::command_names()
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(sorted(from_serde), sorted(catalog));
}

#[test]
fn every_command_name_is_accepted_by_the_generic_tool() {
    let (dir, mut s) = server();
    for name in pf_mcp::command_names() {
        let r = call(
            &mut s,
            "tools/call",
            json!({"name": "preview_command", "arguments": {"command": {"type": name}}}),
        );
        let text = r["result"]["structuredContent"].to_string();
        assert!(
            !text.contains("unknown variant"),
            "{name} inconnue du serveur : {text}"
        );
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn dedicated_tools_exist_and_are_reported() {
    let (dir, mut s) = server();
    let list = call(&mut s, "tools/list", json!({}));
    let tools: Vec<&str> = list["result"]["tools"]
        .as_array()
        .expect("outils")
        .iter()
        .filter_map(|t| t["name"].as_str())
        .collect();
    let cmds = call(
        &mut s,
        "tools/call",
        json!({"name": "list_commands", "arguments": {}}),
    );
    let cmds = &cmds["result"]["structuredContent"];
    let dedicated: Vec<(String, String)> = declared()
        .into_iter()
        .filter(|(_, t)| !t.is_empty())
        .collect();
    assert_eq!(dedicated.len(), 2);
    for (name, tool) in dedicated {
        assert!(tools.contains(&tool.as_str()), "outil {tool} absent");
        let entry = cmds["commands"]
            .as_array()
            .expect("commandes")
            .iter()
            .find(|c| c["name"] == name)
            .expect("variante listée");
        assert_eq!(entry["dedicated_tool"], tool);
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn list_commands_schema_marks_required_fields() {
    let (dir, mut s) = server();
    let r = call(
        &mut s,
        "tools/call",
        json!({"name": "list_commands", "arguments": {}}),
    );
    let out = &r["result"]["structuredContent"];
    assert_eq!(out["count"], 229);
    let find = |n: &str| -> Value {
        out["commands"]
            .as_array()
            .expect("commandes")
            .iter()
            .find(|c| c["name"] == n)
            .expect("variante")["schema"]
            .clone()
    };
    // rename_project { name: String }
    assert_eq!(find("rename_project")["required"], json!(["type", "name"]));
    // set_piece_info { piece, name, quantity, material: Option, mirrored_pair }
    assert_eq!(
        find("set_piece_info")["required"],
        json!(["type", "piece", "name", "quantity", "mirrored_pair"])
    );
    assert_eq!(
        find("set_piece_info")["properties"]["quantity"]["type"],
        "integer"
    );
    assert_eq!(
        find("set_piece_info")["properties"]["name"]["type"],
        "string"
    );
    assert_eq!(
        find("set_piece_info")["properties"]["material"]["nullable"],
        true
    );
    // import_sheet { sheet: Box<Sheet>, #[serde(default)] pieces }
    assert_eq!(find("import_sheet")["required"], json!(["type", "sheet"]));
    assert_eq!(
        find("rename_project")["properties"]["type"]["const"],
        "rename_project"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn classify_names_match_serde_names() {
    let c = Command::RenameProject { name: "x".into() };
    let (name, class) = classify(&c);
    assert_eq!(name, "rename_project");
    assert!(matches!(class, Class::Generic));
    let value = serde_json::to_value(&c).expect("JSON");
    assert_eq!(value["type"], name);
    assert!(!matches!(class, Class::Dedicated(_) | Class::Excluded(_)));
}
