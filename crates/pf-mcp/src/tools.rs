//! Outils et ressources exposés (lecture seule ; écriture via `write`).

use crate::commands;
use crate::write;
use crate::{RpcError, INTERNAL_ERROR, INVALID_PARAMS, RESOURCE_NOT_FOUND};
use pf_core::diagnose::{classify, file_schema_version};
use pf_core::{
    measure, persist, CoreError, Entity, EntityId, EntityKind, Point, PointId, Project, Sheet,
    SheetId, SizeId,
};
use serde_json::{json, Map, Value};
use std::path::Path;

pub(crate) const SUMMARY_URI: &str = "patterncraft://project/summary";

const UNIT: &str = "mm";
const FRAME: &str = "X droite, Y haut, angles en degrés anti-horaires";

pub(crate) fn definitions(allow_write: bool) -> Value {
    let sheet =
        json!({"type": "integer", "minimum": 0, "description": "Identifiant de la feuille"});
    let entity = |d: &str| json!({"type": "integer", "minimum": 0, "description": d});
    let tool = |name: &str, description: &str, props: Value, required: Value| {
        json!({
            "name": name,
            "description": description,
            "inputSchema": {
                "type": "object",
                "properties": props,
                "required": required,
                "additionalProperties": false
            },
            "annotations": {"readOnlyHint": true}
        })
    };
    let mut tools = json!([
        tool(
            "list_sheets",
            "Liste les feuilles du projet (id, nom, nombre de points et de tracés, pièce) et la révision du fichier.",
            json!({}),
            json!([])
        ),
        tool(
            "list_entities",
            "Liste les points (position résolue en mm), les tracés (champ layer : id du calque, null = calque par défaut), les crans, cotations et calques d'une feuille.",
            json!({"sheet_id": sheet}),
            json!(["sheet_id"])
        ),
        tool(
            "measure_entity",
            "Mesure un tracé : segment (longueur mm, direction deg) ou courbe (longueur développée mm, corde mm).",
            json!({"sheet_id": sheet, "entity_id": entity("Identifiant du tracé")}),
            json!(["sheet_id", "entity_id"])
        ),
        tool(
            "measure_angle",
            "Angle intérieur (0 à 180 deg) entre deux segments.",
            json!({
                "sheet_id": sheet,
                "first_entity_id": entity("Identifiant du premier segment"),
                "second_entity_id": entity("Identifiant du second segment")
            }),
            json!(["sheet_id", "first_entity_id", "second_entity_id"])
        ),
        tool(
            "measure_contours",
            "Contours fermés d'une feuille : tracés, périmètre (mm), aire (mm²), auto-intersection.",
            json!({"sheet_id": sheet}),
            json!(["sheet_id"])
        ),
        tool(
            "validate_project",
            "Ouvre et valide le projet (références, identifiants, géométries) et renvoie sa révision.",
            json!({}),
            json!([])
        ),
    ]);
    if let Some(list) = tools.as_array_mut() {
        list.push(commands::definition());
        list.push(tool(
            "grade_size",
            "Lecture d'une taille dérivée (gradation) : points, tracés, crans, cotations, calques de la feuille dans la taille demandée, en mm. Ne modifie rien ; sans size_id : taille de base.",
            json!({
                "sheet_id": sheet,
                "size_id": {"type": "integer", "minimum": 0, "description": "Identifiant de la taille (voir list_sheets / gammes du projet)"}
            }),
            json!(["sheet_id"]),
        ));
        list.push(tool(
            "validate",
            "Valide le projet avec le chargeur de l'application et renvoie révision, version de schéma, effectifs, diagnostic localisé en cas d'échec et présence d'un verrou de l'application. Ne modifie rien.",
            json!({}),
            json!([]),
        ));
    }
    if allow_write {
        if let (Some(list), Some(extra)) = (tools.as_array_mut(), write::definitions().as_array()) {
            list.extend(extra.iter().cloned());
        }
    }
    tools
}

pub(crate) fn resources_list() -> Value {
    json!({"resources": [{
        "uri": SUMMARY_URI,
        "name": "Résumé du projet",
        "description": "Nom, version de schéma, unités, repère et feuilles du projet ouvert.",
        "mimeType": "application/json"
    }]})
}

pub(crate) fn resources_read(path: &Path, uri: &str) -> Result<Value, RpcError> {
    if uri != SUMMARY_URI {
        return Err(RpcError::new(
            RESOURCE_NOT_FOUND,
            format!("ressource inconnue : {uri}"),
        ));
    }
    let project = persist::load(path)
        .map_err(|e| RpcError::new(INTERNAL_ERROR, format!("projet illisible : {e}")))?;
    let text = serde_json::to_string(&summary(&project))
        .map_err(|e| RpcError::new(INTERNAL_ERROR, e.to_string()))?;
    Ok(json!({"contents": [{
        "uri": SUMMARY_URI,
        "mimeType": "application/json",
        "text": text
    }]}))
}

fn pieces_json(project: &Project, sheet: &Sheet) -> Value {
    project
        .pieces_of(sheet.id)
        .map(|p| {
            json!({
                "id": p.id.0,
                "name": p.name,
                "code": p.code,
                "quantity": p.quantity,
                "material": p.material,
                "category": p.category.map(|c| c.key()),
                "comment": p.comment,
                "mirrored_pair": p.mirrored_pair,
                "outline": p.outline.iter()
                    .map(|e| json!({"entity": e.entity.0, "reversed": e.reversed}))
                    .collect::<Vec<_>>(),
                "grainline": p.grainline.map(|g| g.0),
            })
        })
        .collect()
}

fn sheets_json(project: &Project) -> Value {
    project
        .sheets
        .iter()
        .map(|s| {
            json!({
                "id": s.id.0,
                "name": s.name,
                "point_count": s.points.len(),
                "path_count": s.entities.len(),
                "pieces": pieces_json(project, s)
            })
        })
        .collect()
}

fn summary(project: &Project) -> Value {
    json!({
        "project_name": project.name,
        "schema_version": project.schema_version,
        "unit": UNIT,
        "frame": FRAME,
        "sheets": sheets_json(project)
    })
}

/// Résultat d'un outil : erreur métier (`isError`) ou erreur de protocole.
enum Failure {
    Tool(String),
    Rpc(RpcError),
}

impl From<CoreError> for Failure {
    fn from(e: CoreError) -> Self {
        Failure::Tool(e.to_string())
    }
}

impl From<RpcError> for Failure {
    fn from(e: RpcError) -> Self {
        Failure::Rpc(e)
    }
}

fn tool_result(structured: Value, is_error: bool) -> Value {
    let text = serde_json::to_string(&structured).unwrap_or_default();
    json!({
        "content": [{"type": "text", "text": text}],
        "structuredContent": structured,
        "isError": is_error
    })
}

pub(crate) fn call(
    path: &Path,
    allow_write: bool,
    name: &str,
    args: &Map<String, Value>,
) -> Result<Value, RpcError> {
    if write::is_write_tool(name) {
        if !allow_write {
            return Ok(tool_result(
                json!({"error": format!("outil d'écriture « {name} » refusé : serveur en lecture seule (relancer avec --allow-write)")}),
                true,
            ));
        }
        return Ok(match write::call(path, name, args) {
            Ok(v) => tool_result(v, false),
            Err(write::ToolError(msg)) => tool_result(json!({"error": msg}), true),
        });
    }
    let outcome = match name {
        "list_sheets" => list_sheets(path),
        "list_entities" => list_entities(path, args),
        "measure_entity" => measure_entity(path, args),
        "measure_angle" => measure_angle(path, args),
        "measure_contours" => measure_contours(path, args),
        "list_commands" => Ok(commands::list()),
        "grade_size" => grade_size(path, args),
        "validate" => validate(path),
        "validate_project" => {
            return Ok(match write::read_state(path) {
                Ok(s) => tool_result(json!({"valid": true, "revision": s.revision}), false),
                Err(e) => tool_result(json!({"valid": false, "error": e.to_string()}), false),
            })
        }
        other => {
            return Err(RpcError::new(
                INVALID_PARAMS,
                format!("outil inconnu : {other}"),
            ))
        }
    };
    match outcome {
        Ok(v) => Ok(tool_result(v, false)),
        Err(Failure::Tool(msg)) => Ok(tool_result(json!({"error": msg}), true)),
        Err(Failure::Rpc(e)) => Err(e),
    }
}

fn id_arg(args: &Map<String, Value>, key: &str) -> Result<u64, RpcError> {
    args.get(key).and_then(Value::as_u64).ok_or_else(|| {
        RpcError::new(
            INVALID_PARAMS,
            format!("argument « {key} » : entier positif requis"),
        )
    })
}

fn load(path: &Path) -> Result<Project, Failure> {
    persist::load(path).map_err(Failure::from)
}

fn list_sheets(path: &Path) -> Result<Value, Failure> {
    let state = write::read_state(path)?;
    Ok(json!({"unit": UNIT, "revision": state.revision, "sheets": sheets_json(&state.project)}))
}

fn point_json(sheet: &Sheet, p: &Point) -> Result<Value, Failure> {
    let pos = sheet.position(p.id)?;
    // Le nom du type vient de la sérialisation du modèle : toute nouvelle
    // définition de point est exposée sans modifier ce serveur.
    let definition = serde_json::to_value(p.definition)
        .ok()
        .and_then(|v| v.get("type").cloned())
        .unwrap_or(Value::Null);
    Ok(json!({
        "id": p.id.0,
        "label": p.label,
        "x_mm": pos.x,
        "y_mm": pos.y,
        "definition_type": definition,
        "role": serde_json::to_value(p.role).unwrap_or(Value::Null),
        "layer": p.layer.map(|l| l.0)
    }))
}

fn entity_json(sheet: &Sheet, e: &Entity) -> Value {
    let (kind, start, end) = match &e.kind {
        &EntityKind::Segment { from, to, .. } => ("segment", from, to),
        &EntityKind::Curve { start, end, .. } => ("curve", start, end),
        &EntityKind::Arc { start, end, .. } => ("arc", start, end),
        &EntityKind::Circle { center, .. } => ("circle", center, center),
        EntityKind::Spline { nodes } => match (nodes.first(), nodes.last()) {
            (Some(a), Some(b)) => ("spline", a.point, b.point),
            _ => ("spline", PointId(0), PointId(0)),
        },
    };
    let mut v = json!({
        "id": e.id.0,
        "name": sheet.entity_name(e.id),
        "type": kind,
        "role": serde_json::to_value(e.role).unwrap_or(Value::Null),
        "start_point_id": start.0,
        "end_point_id": end.0,
        "layer": e.layer.map(|l| l.0)
    });
    match &e.kind {
        &EntityKind::Arc { center, ccw, .. } => {
            v["center_point_id"] = json!(center.0);
            v["ccw"] = json!(ccw);
        }
        &EntityKind::Circle {
            center,
            radius_mm,
            ccw,
            ..
        } => {
            v["center_point_id"] = json!(center.0);
            v["radius_mm"] = json!(radius_mm);
            v["ccw"] = json!(ccw);
        }
        EntityKind::Spline { nodes } => {
            // Tangentes effectives (saisies ou calculées) : vecteurs d'Hermite en mm.
            let resolved = sheet.spline_nodes(e.id).unwrap_or_default();
            v["nodes"] = json!(nodes
                .iter()
                .zip(&resolved)
                .map(|(n, r)| json!({
                    "point_id": n.point.0,
                    "corner": n.corner,
                    "tangent_in": {"dx_mm": r.tangent_in.x, "dy_mm": r.tangent_in.y,
                                   "explicit": r.tangent_in_explicit},
                    "tangent_out": {"dx_mm": r.tangent_out.x, "dy_mm": r.tangent_out.y,
                                    "explicit": r.tangent_out_explicit}
                }))
                .collect::<Vec<_>>());
        }
        _ => {}
    }
    v
}

fn list_entities(path: &Path, args: &Map<String, Value>) -> Result<Value, Failure> {
    let sheet_id = id_arg(args, "sheet_id")?;
    let project = load(path)?;
    let sheet = project.sheet(SheetId(sheet_id))?;
    entities_json(sheet_id, sheet)
}

fn grade_size(path: &Path, args: &Map<String, Value>) -> Result<Value, Failure> {
    let sheet_id = id_arg(args, "sheet_id")?;
    let size = match args.get("size_id") {
        None | Some(Value::Null) => None,
        Some(v) => Some(SizeId(v.as_u64().ok_or_else(|| {
            RpcError::new(
                INVALID_PARAMS,
                "argument « size_id » : entier positif requis",
            )
        })?)),
    };
    let project = load(path)?;
    let graded = project.graded_sheet(SheetId(sheet_id), size)?;
    let label = size
        .and_then(|id| project.find_size(id))
        .map(|(range, i)| range.sizes[i].label.clone());
    let mut out = entities_json(sheet_id, &graded)?;
    if let Some(obj) = out.as_object_mut() {
        obj.insert("size_id".into(), json!(size.map(|s| s.0)));
        obj.insert("size_label".into(), json!(label));
    }
    Ok(out)
}

fn validate(path: &Path) -> Result<Value, Failure> {
    let app_lock = pf_core::file_guard::lock_path(path).exists();
    let state = match write::read_state(path) {
        Ok(s) => s,
        Err(e) => {
            let text = std::fs::read_to_string(path).ok();
            let text = text.as_deref().map(|t| t.trim_start_matches('\u{feff}'));
            let f = classify(&e, text, text.and_then(file_schema_version));
            return Ok(json!({
                "valid": false,
                "kind": f.kind,
                "error": f.message,
                "line": f.line,
                "column": f.column,
                "application_lock": app_lock
            }));
        }
    };
    Ok(json!({
        "valid": true,
        "revision": state.revision,
        "schema_version": state.project.schema_version,
        "sheets": state.project.sheets.len(),
        "pieces": state.project.pieces.len(),
        "application_lock": app_lock
    }))
}

fn entities_json(sheet_id: u64, sheet: &Sheet) -> Result<Value, Failure> {
    let points = sheet
        .points
        .iter()
        .map(|p| point_json(sheet, p))
        .collect::<Result<Vec<_>, _>>()?;
    let paths: Vec<Value> = sheet
        .entities
        .iter()
        .map(|e| entity_json(sheet, e))
        .collect();
    let notches: Vec<Value> = sheet
        .notches
        .iter()
        .map(|n| {
            // Position effective (calculée pour un cran ancré, US-06.2) ; `null` si illisible.
            let mut v = json!({
                "id": n.id.0,
                "entity_id": n.entity.0,
                "distance_mm": sheet.notch_distance_mm(n).ok(),
                "length_mm": n.length_mm,
                "side": serde_json::to_value(n.side).unwrap_or(Value::Null),
                "style": serde_json::to_value(n.style).unwrap_or(Value::Null)
            });
            if let (Some(anchor), Some(obj)) = (n.anchor, v.as_object_mut()) {
                obj.insert(
                    "anchor".into(),
                    serde_json::to_value(anchor).unwrap_or(Value::Null),
                );
            }
            v
        })
        .collect();
    // Cotations : mesures affichées (valeur recalculée), jamais des contraintes.
    let dimensions: Vec<Value> = sheet
        .dimensions
        .iter()
        .map(|d| {
            json!({
                "id": d.id.0,
                "from_point_id": d.from.0,
                "to_point_id": d.to.0,
                "offset_mm": d.offset_mm,
                "value_mm": sheet.dimension_geometry(d.id).map(|g| g.value_mm).ok()
            })
        })
        .collect();
    // Lignes d'aide : droites infinies, jamais exportées par défaut (lecture seule).
    let guides: Vec<Value> = sheet
        .guides
        .iter()
        .map(|g| {
            json!({
                "id": g.id.0,
                "x_mm": g.x_mm,
                "y_mm": g.y_mm,
                "angle_deg": g.angle_deg,
                "axis": g.axis,
                "layer_id": g.layer.map(|l| l.0)
            })
        })
        .collect();
    let layers: Vec<Value> = sheet
        .layers
        .iter()
        .map(|l| json!({"id": l.id.0, "name": l.name, "visible": l.visible, "locked": l.locked}))
        .collect();
    let mut out = json!({
        "sheet_id": sheet_id,
        "unit": UNIT,
        "points": points,
        "paths": paths,
        "notches": notches,
        "dimensions": dimensions,
        "guides": guides,
        "layers": layers
    });
    // Cotes complètes (US-06.7) : mesures **observées** recalculées à la lecture, jamais des
    // contraintes ; clé absente quand la feuille n'en a pas (sorties existantes inchangées).
    if !sheet.full_dimensions.is_empty() {
        let full: Vec<Value> = sheet
            .full_dimensions
            .iter()
            .map(|d| {
                let mut v = json!({
                    "id": d.id.0,
                    "type": d.kind.key(),
                    "kind": serde_json::to_value(d.kind).unwrap_or(Value::Null),
                    "label": d.label,
                    "text_shift_mm": d.text_shift_mm,
                    "observed": true,
                });
                if let Some(obj) = v.as_object_mut() {
                    match sheet.full_dimension_geometry(d.id) {
                        Ok(g) => {
                            obj.insert("value".into(), json!(g.value));
                            obj.insert(
                                "unit".into(),
                                json!(if g.kind.key() == "angle" { "deg" } else { UNIT }),
                            );
                            obj.insert("text".into(), json!(g.text('.')));
                            obj.insert("text_angle_deg".into(), json!(g.text_angle_deg));
                        }
                        Err(e) => {
                            obj.insert("value".into(), Value::Null);
                            obj.insert("error".into(), json!(e.to_string()));
                        }
                    }
                }
                v
            })
            .collect();
        if let Some(obj) = out.as_object_mut() {
            obj.insert("full_dimensions".into(), Value::Array(full));
        }
    }
    Ok(out)
}

fn measure_entity(path: &Path, args: &Map<String, Value>) -> Result<Value, Failure> {
    let sheet_id = id_arg(args, "sheet_id")?;
    let entity_id = id_arg(args, "entity_id")?;
    let project = load(path)?;
    let sheet = project.sheet(SheetId(sheet_id))?;
    let id = EntityId(entity_id);
    let kind = sheet.entity(id)?.kind.clone();
    let name = sheet.entity_name(id);
    let length = measure::entity_length(sheet, id)?.value;
    Ok(match kind {
        EntityKind::Segment { .. } => json!({
            "entity_id": entity_id,
            "name": name,
            "type": "segment",
            "length_mm": length,
            "direction_deg": measure::segment_direction_deg(sheet, id)?
        }),
        EntityKind::Curve { .. } => json!({
            "entity_id": entity_id,
            "name": name,
            "type": "curve",
            "length_mm": length,
            "chord_mm": measure::curve_chord(sheet, id)?.value
        }),
        EntityKind::Arc { .. } => {
            let arc = sheet.arc_geometry(id)?;
            json!({
                "entity_id": entity_id,
                "name": name,
                "type": "arc",
                "length_mm": length,
                "radius_mm": arc.radius_mm,
                "sweep_deg": arc.sweep_deg().abs()
            })
        }
        EntityKind::Circle { .. } => json!({
            "entity_id": entity_id,
            "name": name,
            "type": "circle",
            "length_mm": length,
            "radius_mm": sheet.circle_geometry(id)?.circle.radius_mm
        }),
        EntityKind::Spline { .. } => {
            let (first, last) = sheet.entity_points(id)?;
            json!({
                "entity_id": entity_id,
                "name": name,
                "type": "spline",
                "length_mm": length,
                "segment_lengths_mm": measure::spline_segment_lengths(sheet, id)?,
                "chord_mm": measure::point_distance(sheet, first, last)?.value
            })
        }
    })
}

fn measure_angle(path: &Path, args: &Map<String, Value>) -> Result<Value, Failure> {
    let sheet_id = id_arg(args, "sheet_id")?;
    let first = id_arg(args, "first_entity_id")?;
    let second = id_arg(args, "second_entity_id")?;
    let project = load(path)?;
    let sheet = project.sheet(SheetId(sheet_id))?;
    let angle = measure::angle_between_segments(sheet, EntityId(first), EntityId(second))?;
    Ok(json!({
        "sheet_id": sheet_id,
        "first_entity_id": first,
        "second_entity_id": second,
        "angle_deg": angle.value
    }))
}

fn measure_contours(path: &Path, args: &Map<String, Value>) -> Result<Value, Failure> {
    let sheet_id = id_arg(args, "sheet_id")?;
    let project = load(path)?;
    let sheet = project.sheet(SheetId(sheet_id))?;
    let mut contours = Vec::new();
    for c in pf_core::contour::closed_contours(sheet) {
        let m = pf_core::contour::measure_contour(sheet, &c)?;
        contours.push(json!({
            "entity_ids": c.edges.iter().map(|e| e.entity.0).collect::<Vec<_>>(),
            "role": serde_json::to_value(c.role).unwrap_or(Value::Null),
            "perimeter_mm": m.perimeter_mm,
            "area_mm2": m.area_mm2,
            "clockwise": m.clockwise,
            "self_intersecting": m.self_intersecting
        }));
    }
    Ok(json!({"sheet_id": sheet_id, "unit": UNIT, "contours": contours}))
}
