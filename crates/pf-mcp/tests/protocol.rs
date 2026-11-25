//! Tests du protocole et des outils (sans stdio, via `Server::handle_line`).

use pf_mcp::Server;
use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;

/// Dossier temporaire supprimé en fin de test.
struct Tmp(PathBuf);

impl Tmp {
    fn new(tag: &str) -> Tmp {
        let dir = std::env::temp_dir().join(format!("pf-mcp-{}-{tag}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("création du dossier temporaire");
        Tmp(dir)
    }
    fn file(&self) -> PathBuf {
        self.0.join("demo.pforge.json")
    }
}

impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn demo_file(tag: &str) -> (Tmp, Server) {
    let tmp = Tmp::new(tag);
    let project = pf_core::demo_project().expect("projet de démo");
    pf_core::persist::save(&tmp.file(), &project).expect("enregistrement");
    let server = Server::new(tmp.file());
    (tmp, server)
}

fn send(server: &mut Server, msg: Value) -> Value {
    let line = serde_json::to_string(&msg).expect("sérialisation");
    let out = server.handle_line(&line).expect("une réponse attendue");
    serde_json::from_str(&out).expect("réponse JSON")
}

fn call(server: &mut Server, name: &str, args: Value) -> Value {
    let r = send(
        server,
        json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":name,"arguments":args}}),
    );
    assert!(r.get("error").is_none(), "erreur JSON-RPC : {r}");
    r["result"].clone()
}

fn first_sheet_id(server: &mut Server) -> u64 {
    let r = call(server, "list_sheets", json!({}));
    r["structuredContent"]["sheets"][0]["id"]
        .as_u64()
        .expect("id de feuille")
}

fn entity_id_by_name(server: &mut Server, sheet: u64, prefix: &str) -> u64 {
    let r = call(server, "list_entities", json!({"sheet_id": sheet}));
    r["structuredContent"]["paths"]
        .as_array()
        .expect("tracés")
        .iter()
        .find(|e| e["name"].as_str().is_some_and(|n| n.starts_with(prefix)))
        .and_then(|e| e["id"].as_u64())
        .expect("tracé trouvé")
}

fn approx(v: &Value, expected: f64) {
    let x = v.as_f64().expect("nombre");
    assert!((x - expected).abs() < 1e-6, "{x} != {expected}");
}

#[test]
fn initialize_handshake() {
    let (_t, mut s) = demo_file("init");
    let r = send(
        &mut s,
        json!({"jsonrpc":"2.0","id":1,"method":"initialize",
               "params":{"protocolVersion":"2024-11-05","capabilities":{},
                         "clientInfo":{"name":"t","version":"0"}}}),
    );
    assert_eq!(r["id"], 1);
    assert_eq!(r["result"]["protocolVersion"], "2024-11-05");
    assert_eq!(r["result"]["serverInfo"]["name"], "patterncraft");
    assert_eq!(
        r["result"]["serverInfo"]["version"],
        env!("CARGO_PKG_VERSION")
    );
    assert!(r["result"]["capabilities"]["tools"].is_object());
    assert!(r["result"]["capabilities"]["resources"].is_object());

    let r = send(
        &mut s,
        json!({"jsonrpc":"2.0","id":2,"method":"initialize","params":{"protocolVersion":"1999-01-01"}}),
    );
    assert_eq!(r["result"]["protocolVersion"], "2025-06-18");
    let r = send(
        &mut s,
        json!({"jsonrpc":"2.0","id":3,"method":"initialize","params":{"protocolVersion":"2025-03-26"}}),
    );
    assert_eq!(r["result"]["protocolVersion"], "2025-03-26");
    let r = send(&mut s, json!({"jsonrpc":"2.0","id":4,"method":"ping"}));
    assert_eq!(r["result"], json!({}));
}

#[test]
fn tools_list_has_only_read_only_tools_with_schemas() {
    let (_t, mut s) = demo_file("list");
    let r = send(
        &mut s,
        json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}),
    );
    let tools = r["result"]["tools"].as_array().expect("tools");
    let names: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();
    assert_eq!(
        names,
        [
            "list_sheets",
            "list_entities",
            "measure_entity",
            "measure_angle",
            "measure_contours",
            "validate_project",
            "list_commands",
            "grade_size",
            "validate"
        ]
    );
    for t in tools {
        assert_eq!(t["inputSchema"]["type"], "object");
        assert!(t["inputSchema"]["properties"].is_object());
        assert_eq!(t["annotations"]["readOnlyHint"], true);
    }
}

#[test]
fn resource_summary() {
    let (_t, mut s) = demo_file("res");
    let r = send(
        &mut s,
        json!({"jsonrpc":"2.0","id":1,"method":"resources/list"}),
    );
    assert_eq!(
        r["result"]["resources"][0]["uri"],
        "patterncraft://project/summary"
    );
    let r = send(
        &mut s,
        json!({"jsonrpc":"2.0","id":2,"method":"resources/read",
               "params":{"uri":"patterncraft://project/summary"}}),
    );
    let text = r["result"]["contents"][0]["text"].as_str().expect("texte");
    let v: Value = serde_json::from_str(text).expect("JSON");
    assert_eq!(v["project_name"], "Exercice de géométrie");
    assert_eq!(v["schema_version"], pf_core::SCHEMA_VERSION);
    assert_eq!(v["unit"], "mm");
    assert_eq!(v["sheets"].as_array().map(Vec::len), Some(3));
    assert_eq!(v["sheets"][0]["name"], "Devant");
    assert_eq!(v["sheets"][0]["path_count"], 3);

    let r = send(
        &mut s,
        json!({"jsonrpc":"2.0","id":3,"method":"resources/read","params":{"uri":"file:///etc/passwd"}}),
    );
    assert_eq!(r["error"]["code"], -32002);
    let r = send(
        &mut s,
        json!({"jsonrpc":"2.0","id":4,"method":"resources/read","params":{"uri":5}}),
    );
    assert_eq!(r["error"]["code"], -32602);
}

#[test]
fn tools_on_demo_project() {
    let (_t, mut s) = demo_file("tools");
    let r = call(&mut s, "list_sheets", json!({}));
    assert_eq!(r["isError"], false);
    assert_eq!(
        r["structuredContent"]["sheets"].as_array().map(Vec::len),
        Some(3)
    );
    let text = r["content"][0]["text"].as_str().expect("texte");
    assert_eq!(
        serde_json::from_str::<Value>(text).expect("JSON"),
        r["structuredContent"]
    );

    let sheet = first_sheet_id(&mut s);
    let r = call(&mut s, "list_entities", json!({"sheet_id": sheet}));
    let sc = &r["structuredContent"];
    let a = sc["points"]
        .as_array()
        .expect("points")
        .iter()
        .find(|p| p["label"] == "A")
        .expect("point A");
    approx(&a["x_mm"], 0.0);
    approx(&a["y_mm"], 0.0);
    assert_eq!(a["definition_type"], "free");
    assert!(sc["points"]
        .as_array()
        .expect("points")
        .iter()
        .any(|p| p["label"] == "P" && p["definition_type"] == "on_segment"));
    assert_eq!(sc["paths"].as_array().map(Vec::len), Some(3));

    let ab = entity_id_by_name(&mut s, sheet, "Segment AB");
    let bc = entity_id_by_name(&mut s, sheet, "Segment BC");
    let curve = entity_id_by_name(&mut s, sheet, "Courbe");

    let r = call(
        &mut s,
        "measure_entity",
        json!({"sheet_id": sheet, "entity_id": ab}),
    );
    approx(&r["structuredContent"]["length_mm"], 120.0);
    approx(&r["structuredContent"]["direction_deg"], 0.0);
    let r = call(
        &mut s,
        "measure_entity",
        json!({"sheet_id": sheet, "entity_id": bc}),
    );
    approx(&r["structuredContent"]["length_mm"], 80.0);
    let r = call(
        &mut s,
        "measure_entity",
        json!({"sheet_id": sheet, "entity_id": curve}),
    );
    let len = r["structuredContent"]["length_mm"]
        .as_f64()
        .expect("longueur");
    let chord = r["structuredContent"]["chord_mm"].as_f64().expect("corde");
    assert!(len > chord, "développée {len} <= corde {chord}");

    let r = call(
        &mut s,
        "measure_angle",
        json!({"sheet_id": sheet, "first_entity_id": ab, "second_entity_id": bc}),
    );
    approx(&r["structuredContent"]["angle_deg"], 90.0);

    let r = call(&mut s, "validate_project", json!({}));
    assert_eq!(r["structuredContent"]["valid"], true);
    assert!(r["structuredContent"]["revision"]
        .as_str()
        .is_some_and(|v| v.starts_with("sha256:")));
    assert_eq!(r["isError"], false);
}

#[test]
fn unknown_sheet_or_entity_is_tool_error() {
    let (_t, mut s) = demo_file("unknown");
    let sheet = first_sheet_id(&mut s);
    let ab = entity_id_by_name(&mut s, sheet, "Segment AB");
    let curve = entity_id_by_name(&mut s, sheet, "Courbe");
    for (name, args) in [
        ("list_entities", json!({"sheet_id": 99999})),
        (
            "measure_entity",
            json!({"sheet_id": 99999, "entity_id": ab}),
        ),
        (
            "measure_entity",
            json!({"sheet_id": sheet, "entity_id": 99999}),
        ),
        (
            "measure_angle",
            json!({"sheet_id": sheet, "first_entity_id": ab, "second_entity_id": 99999}),
        ),
        (
            "measure_angle",
            json!({"sheet_id": sheet, "first_entity_id": ab, "second_entity_id": curve}),
        ),
    ] {
        let r = call(&mut s, name, args);
        assert_eq!(r["isError"], true, "{name}");
        assert!(r["structuredContent"]["error"].is_string());
        assert!(r["content"][0]["text"].is_string());
    }
    let r = call(
        &mut s,
        "measure_entity",
        json!({"sheet_id": 99999, "entity_id": ab}),
    );
    assert_eq!(
        r["structuredContent"]["error"],
        "feuille introuvable (id 99999)"
    );
}

#[test]
fn bad_tool_arguments_are_invalid_params() {
    let (_t, mut s) = demo_file("badargs");
    for (name, args) in [
        ("list_entities", json!({})),
        ("list_entities", json!({"sheet_id": "1"})),
        ("list_entities", json!({"sheet_id": -1})),
        ("measure_entity", json!({"sheet_id": 1})),
    ] {
        let r = send(
            &mut s,
            json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":name,"arguments":args}}),
        );
        assert_eq!(r["error"]["code"], -32602, "{name} {args}");
    }
    let r = send(
        &mut s,
        json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"rm_rf","arguments":{}}}),
    );
    assert_eq!(r["error"]["code"], -32602);
}

#[test]
fn corrupted_or_missing_project_is_tool_error() {
    let (t, mut s) = demo_file("corrupt");
    fs::write(t.file(), b"{ ceci n'est pas du json").expect("écriture");
    let r = call(&mut s, "list_sheets", json!({}));
    assert_eq!(r["isError"], true);
    assert!(r["structuredContent"]["error"].is_string());
    let r = call(&mut s, "list_entities", json!({"sheet_id": 1}));
    assert_eq!(r["isError"], true);
    let r = call(&mut s, "validate_project", json!({}));
    assert_eq!(r["structuredContent"]["valid"], false);
    assert!(r["structuredContent"]["error"].is_string());
    let r = send(
        &mut s,
        json!({"jsonrpc":"2.0","id":1,"method":"resources/read","params":{"uri":"patterncraft://project/summary"}}),
    );
    assert!(r["error"].is_object());

    fs::remove_file(t.file()).expect("suppression");
    let r = call(&mut s, "list_sheets", json!({}));
    assert_eq!(r["isError"], true);
}

#[test]
fn project_is_reread_on_each_call() {
    let (t, mut s) = demo_file("reread");
    let mut project = pf_core::persist::load(&t.file()).expect("ouverture");
    project.name = "Autre nom".into();
    pf_core::persist::save(&t.file(), &project).expect("enregistrement");
    let r = send(
        &mut s,
        json!({"jsonrpc":"2.0","id":1,"method":"resources/read","params":{"uri":"patterncraft://project/summary"}}),
    );
    let text = r["result"]["contents"][0]["text"].as_str().expect("texte");
    assert!(text.contains("Autre nom"));
}

#[test]
fn protocol_errors() {
    let (_t, mut s) = demo_file("proto");
    let parse = |out: Option<String>| -> Value {
        serde_json::from_str(&out.expect("réponse")).expect("JSON")
    };

    let r = parse(s.handle_line("{pas du json"));
    assert_eq!(r["error"]["code"], -32700);
    assert_eq!(r["id"], Value::Null);

    let r = send(&mut s, json!({"jsonrpc":"2.0","id":7,"method":"nope"}));
    assert_eq!(r["error"]["code"], -32601);
    assert_eq!(r["id"], 7);

    for params in [json!("texte"), json!(42), json!([1])] {
        let r = send(
            &mut s,
            json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":params}),
        );
        assert_eq!(r["error"]["code"], -32602);
    }
    let r = send(
        &mut s,
        json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":5}}),
    );
    assert_eq!(r["error"]["code"], -32602);
    let r = send(
        &mut s,
        json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"list_sheets","arguments":[1]}}),
    );
    assert_eq!(r["error"]["code"], -32602);
    let r = send(
        &mut s,
        json!({"jsonrpc":"2.0","id":1,"method":"initialize"}),
    );
    assert_eq!(r["error"]["code"], -32602);

    // Requêtes structurellement invalides.
    for bad in [
        "[]",
        "42",
        "\"x\"",
        "null",
        "[{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}]",
    ] {
        let r = parse(s.handle_line(bad));
        assert_eq!(r["error"]["code"], -32600, "{bad}");
    }
    let r = parse(s.handle_line(r#"{"jsonrpc":"2.0","id":1}"#));
    assert_eq!(r["error"]["code"], -32600);
    let r = parse(s.handle_line(r#"{"jsonrpc":"2.0","id":1,"method":5}"#));
    assert_eq!(r["error"]["code"], -32600);
    let r = parse(s.handle_line(r#"{"id":1,"method":"ping"}"#));
    assert_eq!(r["error"]["code"], -32600);
    let r = parse(s.handle_line(r#"{"jsonrpc":"2.0","id":{"a":1},"method":"ping"}"#));
    assert_eq!(r["error"]["code"], -32600);
    assert_eq!(r["id"], Value::Null);
}

#[test]
fn id_types_are_echoed() {
    let (_t, mut s) = demo_file("ids");
    for id in [json!(null), json!("abc"), json!(42), json!(1.5), json!("")] {
        let r = send(&mut s, json!({"jsonrpc":"2.0","id":id,"method":"ping"}));
        assert_eq!(r["id"], id);
        assert_eq!(r["result"], json!({}));
    }
}

#[test]
fn notifications_and_blank_lines_produce_no_output() {
    let (_t, mut s) = demo_file("notif");
    assert!(s
        .handle_line(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#)
        .is_none());
    assert!(s
        .handle_line(r#"{"jsonrpc":"2.0","method":"inconnue"}"#)
        .is_none());
    assert!(s
        .handle_line(r#"{"jsonrpc":"2.0","method":"tools/call","params":5}"#)
        .is_none());
    assert!(s.handle_line("").is_none());
    assert!(s.handle_line("   \t\r").is_none());
}

#[test]
fn very_long_line_is_rejected_without_panic() {
    let (_t, mut s) = demo_file("long");
    let long = format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"ping","x":"{}"}}"#,
        "a".repeat(pf_mcp::MAX_LINE_BYTES)
    );
    let out = s.handle_line(&long).expect("réponse");
    let r: Value = serde_json::from_str(&out).expect("JSON");
    assert_eq!(r["error"]["code"], -32600);

    // Via la boucle : la ligne trop longue est ignorée, la suivante est servie.
    let mut input = long.into_bytes();
    input.extend_from_slice(b"\n{\"jsonrpc\":\"2.0\",\"id\":9,\"method\":\"ping\"}\n");
    let mut out = Vec::new();
    s.run(&input[..], &mut out).expect("boucle");
    let text = String::from_utf8(out).expect("UTF-8");
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 2);
    assert!(lines[0].contains("-32600"));
    assert!(lines[1].contains(r#""id":9"#));
}

#[test]
fn arbitrary_garbage_never_panics() {
    let (_t, mut s) = demo_file("garbage");
    let samples = [
        "\u{0}",
        "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/call\",\"params\":{\"name\":\"list_entities\",\"arguments\":{\"sheet_id\":1e999}}}",
        "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/call\",\"params\":{\"name\":\"list_entities\",\"arguments\":{\"sheet_id\":18446744073709551615}}}",
        "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/call\",\"params\":null}",
        "{{{{{{{{",
        "\u{feff}{}",
    ];
    for g in samples {
        let _ = s.handle_line(g);
    }
}

#[test]
fn project_file_is_never_modified() {
    let (t, mut s) = demo_file("readonly");
    let before = fs::read(t.file()).expect("lecture");
    let sheet = first_sheet_id(&mut s);
    let ab = entity_id_by_name(&mut s, sheet, "Segment AB");
    let bc = entity_id_by_name(&mut s, sheet, "Segment BC");
    let curve = entity_id_by_name(&mut s, sheet, "Courbe");
    let _ = call(&mut s, "list_entities", json!({"sheet_id": sheet}));
    let _ = call(
        &mut s,
        "measure_entity",
        json!({"sheet_id": sheet, "entity_id": curve}),
    );
    let _ = call(
        &mut s,
        "measure_angle",
        json!({"sheet_id": sheet, "first_entity_id": ab, "second_entity_id": bc}),
    );
    let _ = call(&mut s, "validate_project", json!({}));
    let _ = call(&mut s, "list_entities", json!({"sheet_id": 123456}));
    let _ = s.handle_line("n'importe quoi");
    let _ = send(
        &mut s,
        json!({"jsonrpc":"2.0","id":1,"method":"resources/read","params":{"uri":"patterncraft://project/summary"}}),
    );
    assert_eq!(fs::read(t.file()).expect("relecture"), before);
    let names: Vec<_> = fs::read_dir(&t.0)
        .expect("dossier")
        .filter_map(Result::ok)
        .collect();
    assert_eq!(names.len(), 1, "aucun fichier supplémentaire attendu");
}

#[test]
fn measure_contours_on_demo_project() {
    let (_tmp, mut server) = demo_file("contours");
    let sheet = first_sheet_id(&mut server);
    let r = call(&mut server, "measure_contours", json!({"sheet_id": sheet}));
    assert_eq!(r["isError"], false);
    let contours = r["structuredContent"]["contours"]
        .as_array()
        .expect("contours");
    assert_eq!(contours.len(), 1);
    assert_eq!(contours[0]["entity_ids"].as_array().map(Vec::len), Some(3));
    assert_eq!(contours[0]["self_intersecting"], false);
    let perimeter = contours[0]["perimeter_mm"].as_f64().expect("périmètre");
    assert!(perimeter > 364.0 && perimeter < 367.0, "{perimeter}");
    assert!(contours[0]["area_mm2"].as_f64().expect("aire") > 4800.0);
    // Feuille vide : aucun contour, pas une erreur.
    let sheets = call(&mut server, "list_sheets", json!({}));
    let empty = sheets["structuredContent"]["sheets"][1]["id"]
        .as_u64()
        .expect("id");
    let r = call(&mut server, "measure_contours", json!({"sheet_id": empty}));
    assert_eq!(
        r["structuredContent"]["contours"].as_array().map(Vec::len),
        Some(0)
    );
    let r = call(&mut server, "measure_contours", json!({"sheet_id": 987654}));
    assert_eq!(r["isError"], true);
}

#[test]
fn list_entities_exposes_guides_read_only() {
    let tmp = Tmp::new("guides");
    let mut project = pf_core::demo_piece_project().expect("projet pièce");
    let sheet_id = project.sheets[0].id;
    project
        .apply(pf_core::Command::AddGuide {
            sheet: sheet_id,
            x_mm: 12.0,
            y_mm: 34.0,
            angle_deg: 45.0,
            axis: true,
            layer: None,
        })
        .expect("guide");
    pf_core::persist::save(&tmp.file(), &project).expect("enregistrement");
    let mut server = Server::new(tmp.file());
    let sheet = first_sheet_id(&mut server);
    let r = call(&mut server, "list_entities", json!({"sheet_id": sheet}));
    let g = &r["structuredContent"]["guides"];
    assert_eq!(g.as_array().map(Vec::len), Some(1));
    assert_eq!(g[0]["x_mm"], 12.0);
    assert_eq!(g[0]["y_mm"], 34.0);
    assert_eq!(g[0]["angle_deg"], 45.0);
    assert_eq!(g[0]["axis"], true);
}

#[test]
fn list_entities_exposes_notches_and_grainline_role() {
    let tmp = Tmp::new("notches");
    let project = pf_core::demo_piece_project().expect("projet pièce");
    pf_core::persist::save(&tmp.file(), &project).expect("enregistrement");
    let mut server = Server::new(tmp.file());
    let sheet = first_sheet_id(&mut server);
    let r = call(&mut server, "list_entities", json!({"sheet_id": sheet}));
    let s = &r["structuredContent"];
    assert_eq!(s["notches"].as_array().map(Vec::len), Some(3));
    assert_eq!(s["notches"][0]["side"], "right");
    // Deux cotations en lecture seule : valeurs mesurées de la pièce 170 x 120.
    assert_eq!(s["dimensions"].as_array().map(Vec::len), Some(2));
    assert_eq!(s["dimensions"][0]["value_mm"], 170.0);
    assert_eq!(s["dimensions"][1]["value_mm"], 120.0);
    assert!(s["notches"]
        .as_array()
        .expect("crans")
        .iter()
        .any(|n| n["style"] == "double"));
    let roles: Vec<&str> = s["paths"]
        .as_array()
        .expect("tracés")
        .iter()
        .filter_map(|p| p["role"].as_str())
        .collect();
    assert_eq!(roles.iter().filter(|r| **r == "grainline").count(), 1);
    assert_eq!(roles.iter().filter(|r| **r == "cutting_contour").count(), 4);
}

#[test]
fn list_entities_expose_la_position_effective_et_l_ancrage_d_un_cran() {
    use pf_core::Command;
    let tmp = Tmp::new("crans-ancres");
    let mut project = pf_core::Project::new("crans ancrés");
    let sheet = project.sheets[0].id;
    // Commandes écrites en JSON : ce test ne dépend pas de `pf-geometry`.
    let run = |project: &mut pf_core::Project, v: Value| {
        let command: Command = serde_json::from_value(v).expect("commande");
        project.apply(command).expect("commande appliquée")
    };
    let seg = run(
        &mut project,
        json!({"type": "add_segment", "sheet": sheet.0,
               "from": {"new": {"x": 0.0, "y": 0.0}}, "to": {"new": {"x": 120.0, "y": 0.0}}}),
    )
    .entities[0];
    for anchor in [
        json!({"type": "midpoint"}),
        json!({"type": "ratio", "ratio": 0.25}),
    ] {
        run(
            &mut project,
            json!({"type": "add_anchored_notch", "sheet": sheet.0, "entity": seg.0,
                   "anchor": anchor, "side": "left", "length_mm": 5.0, "style": "single"}),
        );
    }
    pf_core::persist::save(&tmp.file(), &project).expect("enregistrement");
    let mut server = Server::new(tmp.file());
    let sheet = first_sheet_id(&mut server);
    let r = call(&mut server, "list_entities", json!({"sheet_id": sheet}));
    let n = &r["structuredContent"]["notches"];
    // Position calculée (pas le 0 stocké) et ancrage lisible ; rien d'ajouté aux crans fixes.
    assert_eq!(n[0]["distance_mm"], 60.0);
    assert_eq!(n[0]["anchor"], json!({"type": "midpoint"}));
    assert_eq!(n[1]["distance_mm"], 30.0);
    assert_eq!(n[1]["anchor"], json!({"type": "ratio", "ratio": 0.25}));
}

#[test]
fn list_entities_expose_le_calque_des_objets() {
    use pf_core::{Command, PointRef};
    let tmp = Tmp::new("calques");
    let mut project = pf_core::Project::new("calques");
    let sheet = project.sheets[0].id;
    let layer = project
        .apply(Command::AddLayer {
            sheet,
            name: "Détails".into(),
        })
        .expect("calque")
        .layers[0];
    let at = |x: f64| -> PointRef {
        serde_json::from_value(json!({"new": {"x": x, "y": 0.0}})).expect("point")
    };
    project
        .apply(Command::InLayer {
            sheet,
            layer,
            command: Box::new(Command::AddSegment {
                sheet,
                from: at(0.0),
                to: at(10.0),
            }),
        })
        .expect("segment dans le calque");
    project
        .apply(Command::AddSegment {
            sheet,
            from: at(0.0),
            to: at(20.0),
        })
        .expect("segment par défaut");
    pf_core::persist::save(&tmp.file(), &project).expect("enregistrement");
    let mut s = Server::new(tmp.file());
    let r = call(&mut s, "list_entities", json!({"sheet_id": sheet.0}));
    let sc = &r["structuredContent"];
    let layers: Vec<Option<u64>> = sc["paths"]
        .as_array()
        .expect("tracés")
        .iter()
        .map(|p| p["layer"].as_u64())
        .collect();
    assert_eq!(layers, vec![Some(layer.0), None]);
    assert!(sc["points"][0]["layer"].is_u64());
    assert_eq!(sc["layers"][0]["name"], "Détails");
    assert_eq!(sc["layers"][0]["visible"], true);
}

#[test]
fn courbe_par_points_de_passage_lisible_et_mesuree() {
    use pf_core::{Command, OffsetMm, PointRef, Project};
    let tmp = Tmp::new("spline");
    let mut project = Project::new("Courbes");
    let sheet = project.sheets[0].id;
    let points: Vec<PointRef> = [(0.0, 0.0), (30.0, 20.0), (60.0, -5.0)]
        .iter()
        .map(|&(x, y)| serde_json::from_value(json!({"new": {"x": x, "y": y}})).expect("point"))
        .collect();
    let id = project
        .apply(Command::AddSpline { sheet, points })
        .expect("courbe")
        .entities[0];
    project
        .apply(Command::SetSplineNode {
            sheet,
            spline: id,
            index: 1,
            corner: true,
            tangent: Some(OffsetMm { dx: 9.0, dy: 2.0 }),
            tangent_in: None,
        })
        .expect("point anguleux");
    pf_core::persist::save(&tmp.file(), &project).expect("enregistrement");
    let mut s = Server::new(tmp.file());
    let r = call(&mut s, "list_entities", json!({"sheet_id": sheet.0}));
    let path = &r["structuredContent"]["paths"][0];
    assert_eq!(path["type"], "spline");
    let nodes = path["nodes"].as_array().expect("points de passage");
    assert_eq!(nodes.len(), 3);
    assert_eq!(nodes[1]["corner"], true);
    assert_eq!(nodes[1]["tangent_out"]["explicit"], true);
    assert_eq!(nodes[1]["tangent_out"]["dx_mm"], 9.0);
    assert_eq!(nodes[1]["tangent_in"]["explicit"], false);
    let r = call(
        &mut s,
        "measure_entity",
        json!({"sheet_id": sheet.0, "entity_id": id.0}),
    );
    let m = &r["structuredContent"];
    assert_eq!(m["type"], "spline");
    let parts: Vec<f64> = m["segment_lengths_mm"]
        .as_array()
        .expect("tronçons")
        .iter()
        .map(|v| v.as_f64().expect("nombre"))
        .collect();
    assert_eq!(parts.len(), 2);
    assert!((parts.iter().sum::<f64>() - m["length_mm"].as_f64().unwrap()).abs() < 1e-9);
    assert!(m["chord_mm"].as_f64().unwrap() > 0.0);
}

#[test]
fn list_sheets_expose_la_fiche_de_piece_en_lecture_seule() {
    use pf_core::{Command, PieceCategory};
    let tmp = Tmp::new("fiche");
    let mut project = pf_core::Project::new("fiche");
    let sheet = project.sheets[0].id;
    let piece = project
        .apply(Command::AddPiece {
            sheet,
            name: "Dos".into(),
            quantity: 1,
            material: None,
            mirrored_pair: false,
        })
        .expect("pièce")
        .pieces[0];
    project
        .apply(Command::SetPieceCard {
            piece,
            name: "Dos".into(),
            code: Some("DS-02".into()),
            quantity: 2,
            mirrored_pair: true,
            material: Some("Coton".into()),
            category: Some(PieceCategory::Interfacing),
            comment: Some("ligne 1\nligne 2".into()),
        })
        .expect("fiche");
    pf_core::persist::save(&tmp.file(), &project).expect("enregistrement");
    let mut server = Server::new(tmp.file());
    let r = call(&mut server, "list_sheets", json!({}));
    let p = &r["structuredContent"]["sheets"][0]["pieces"][0];
    assert_eq!(p["code"], "DS-02");
    assert_eq!(p["category"], "interfacing");
    assert_eq!(p["comment"], "ligne 1\nligne 2");
    assert_eq!(p["quantity"], 2);
    assert_eq!(p["mirrored_pair"], true);
    assert_eq!(p["material"], "Coton");
    // Une pièce sans fiche renseignée : valeurs nulles, jamais de clé manquante.
    let mut bare = pf_core::Project::new("nue");
    let s = bare.sheets[0].id;
    bare.apply(Command::AddPiece {
        sheet: s,
        name: "X".into(),
        quantity: 1,
        material: None,
        mirrored_pair: false,
    })
    .expect("pièce");
    pf_core::persist::save(&tmp.file(), &bare).expect("enregistrement");
    let r = call(&mut server, "list_sheets", json!({}));
    let p = &r["structuredContent"]["sheets"][0]["pieces"][0];
    assert!(p["code"].is_null() && p["category"].is_null() && p["comment"].is_null());
}

#[test]
fn list_entities_expose_les_huit_roles_de_lignes_speciales() {
    use pf_core::Command;
    let tmp = Tmp::new("lignes-speciales");
    let mut project = pf_core::Project::new("lignes spéciales");
    let sheet = project.sheets[0].id;
    let roles = [
        "topstitch",
        "center_front",
        "center_back",
        "waist_line",
        "hip_line",
        "bust_line",
        "skiving_line",
        "glue_line",
    ];
    for (i, role) in roles.iter().enumerate() {
        // Commandes écrites en JSON : un rôle inconnu serait refusé à la désérialisation.
        let y = 10.0 * i as f64;
        let add: Command = serde_json::from_value(json!({
            "type": "add_segment", "sheet": sheet.0,
            "from": {"new": {"x": 0.0, "y": y}}, "to": {"new": {"x": 50.0, "y": y}}}))
        .expect("commande");
        let entity = project.apply(add).expect("segment").entities[0];
        let set: Command = serde_json::from_value(json!({
            "type": "set_path_role", "sheet": sheet.0, "entity": entity.0, "role": role}))
        .expect("commande de rôle");
        project.apply(set).expect("rôle");
    }
    let bad = serde_json::from_value::<Command>(json!({
        "type": "set_path_role", "sheet": sheet.0, "entity": 1, "role": "centre_devant"}));
    assert!(bad.is_err(), "un rôle inconnu reste refusé");
    pf_core::persist::save(&tmp.file(), &project).expect("enregistrement");
    let mut server = Server::new(tmp.file());
    let id = first_sheet_id(&mut server);
    let r = call(&mut server, "list_entities", json!({"sheet_id": id}));
    let seen: Vec<&str> = r["structuredContent"]["paths"]
        .as_array()
        .expect("tracés")
        .iter()
        .filter_map(|p| p["role"].as_str())
        .collect();
    assert_eq!(seen, roles);
}

/// US-06.7 — les cotes complètes sont exposées en lecture seule, valeur recalculée à chaque
/// lecture ; la clé est absente quand la feuille n'en a pas (sorties existantes inchangées).
#[test]
fn list_entities_expose_les_cotes_completes_en_lecture_seule() {
    use pf_core::Command;
    let tmp = Tmp::new("cotes-completes");
    let mut project = pf_core::Project::new("cotes");
    let sheet = project.sheets[0].id;
    let run = |project: &mut pf_core::Project, v: Value| {
        let command: Command = serde_json::from_value(v).expect("commande");
        project.apply(command).expect("commande appliquée")
    };
    let o = run(
        &mut project,
        json!({"type": "add_segment", "sheet": sheet.0,
               "from": {"new": {"x": 0.0, "y": 0.0}}, "to": {"new": {"x": 30.0, "y": 40.0}}}),
    );
    let (seg, a, b) = (o.entities[0], o.points[0], o.points[1]);
    pf_core::persist::save(&tmp.file(), &project).expect("enregistrement");
    let mut server = Server::new(tmp.file());
    let sheet_id = first_sheet_id(&mut server);
    let r = call(&mut server, "list_entities", json!({"sheet_id": sheet_id}));
    assert!(r["structuredContent"].get("full_dimensions").is_none());
    run(
        &mut project,
        json!({"type": "add_full_dimension", "sheet": sheet.0, "label": "Côté",
               "kind": {"type": "path_length", "entity": seg.0, "offset_mm": 5.0}}),
    );
    run(
        &mut project,
        json!({"type": "add_full_dimension", "sheet": sheet.0,
               "kind": {"type": "horizontal", "from": a.0, "to": b.0, "offset_mm": 2.0}}),
    );
    pf_core::persist::save(&tmp.file(), &project).expect("enregistrement");
    let r = call(&mut server, "list_entities", json!({"sheet_id": sheet_id}));
    let full = &r["structuredContent"]["full_dimensions"];
    assert_eq!(full.as_array().map(Vec::len), Some(2));
    assert_eq!(full[0]["type"], "path_length");
    assert_eq!(full[0]["value"], 50.0);
    assert_eq!(full[0]["unit"], "mm");
    assert_eq!(full[0]["label"], "Côté");
    assert_eq!(full[0]["text"], "Côté : 50.00 mm");
    assert_eq!(full[0]["observed"], true);
    assert_eq!(full[1]["type"], "horizontal");
    assert_eq!(full[1]["value"], 30.0);
    assert_eq!(full[1]["kind"]["from"], a.0);
    // Lecture seule : la lecture ne modifie pas le fichier.
    let bytes = fs::read(tmp.file()).expect("fichier");
    let r2 = call(&mut server, "list_entities", json!({"sheet_id": sheet_id}));
    assert_eq!(r2["structuredContent"], r["structuredContent"]);
    assert_eq!(fs::read(tmp.file()).expect("fichier"), bytes);
    // Le déplacement d'un point est suivi à la lecture suivante.
    run(
        &mut project,
        json!({"type": "move_point", "sheet": sheet.0, "point": b.0, "to": {"x": 60.0, "y": 80.0}}),
    );
    pf_core::persist::save(&tmp.file(), &project).expect("enregistrement");
    let r3 = call(&mut server, "list_entities", json!({"sheet_id": sheet_id}));
    assert_eq!(
        r3["structuredContent"]["full_dimensions"][0]["value"],
        100.0
    );
}
