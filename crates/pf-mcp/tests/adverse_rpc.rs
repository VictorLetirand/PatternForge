//! Campagne adverse du serveur MCP (US-18.1) : 500 requêtes JSON-RPC à graine fixe.
//!
//! Oracle indépendant du serveur : forme JSON-RPC écrite ici, empreinte du fichier recalculée par
//! `pf_core::file_guard` (autre implémentation de SHA-256 que celle de `pf-mcp`), état du disque
//! comparé octet par octet avant / après. Contrat vérifié : aucun panic (jamais le code -32603),
//! toute erreur est typée, un refus ne laisse aucune trace.

use pf_core::{file_guard, persist};
use serde_json::{json, Map, Value};
use std::path::Path;

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }
}

const TOOLS: [&str; 15] = [
    "list_sheets",
    "list_entities",
    "measure_entity",
    "measure_angle",
    "measure_contours",
    "validate_project",
    "list_commands",
    "grade_size",
    "validate",
    "preview_command",
    "apply_command",
    "undo_last",
    "extract_piece",
    "apply_seam_allowance",
    "export",
];

const NASTY_PATHS: [&str; 14] = [
    "../x.svg",
    "..\\x.svg",
    "a/../../x.svg",
    "C:\\Windows\\x.svg",
    "/etc/x.svg",
    "\\\\srv\\share\\x.svg",
    "demo.pforge.json",
    "demo.pforge.json.mcp-journal.jsonl",
    "x.lock",
    "",
    "   ",
    "con",
    "a\0b.svg",
    ".",
];

fn junk(r: &mut Rng, depth: u32) -> Value {
    match r.below(if depth > 2 { 9 } else { 11 }) {
        0 => Value::Null,
        1 => json!(true),
        2 => json!(u64::MAX),
        3 => json!(-1),
        4 => json!(1.5),
        5 => json!(1e308),
        6 => json!("NaN"),
        7 => json!("é\u{1F600}\u{0}"),
        8 => json!("x".repeat(r.below(5000))),
        9 => Value::Array((0..r.below(4)).map(|_| junk(r, depth + 1)).collect()),
        _ => {
            let mut m = Map::new();
            for i in 0..r.below(4) {
                m.insert(format!("k{i}"), junk(r, depth + 1));
            }
            Value::Object(m)
        }
    }
}

fn args_for(r: &mut Rng, tool: &str, revision: &str) -> Value {
    let mut m = Map::new();
    let keys: &[&str] = match tool {
        "list_entities" | "measure_contours" => &["sheet_id"],
        "measure_entity" => &["sheet_id", "entity_id"],
        "measure_angle" => &["sheet_id", "first_entity_id", "second_entity_id"],
        "grade_size" => &["sheet_id", "size_id"],
        "extract_piece" => &["sheet_id", "x_mm", "y_mm", "entity_ids", "name", "detached"],
        "apply_seam_allowance" => &["piece_id", "default_mm"],
        "export" => &[
            "format",
            "path",
            "size",
            "all_sizes",
            "dpi",
            "lang",
            "overwrite",
        ],
        _ => &[],
    };
    for k in keys {
        if r.below(5) > 0 {
            m.insert(
                (*k).to_string(),
                if r.below(2) == 0 {
                    json!(r.below(4))
                } else {
                    junk(r, 0)
                },
            );
        }
    }
    if matches!(tool, "extract_piece" | "apply_seam_allowance") {
        m.insert(
            "expected_revision".into(),
            if r.below(3) == 0 {
                junk(r, 0)
            } else {
                json!(revision)
            },
        );
    }
    Value::Object(m)
}

fn command_json(r: &mut Rng, names: &[&str]) -> Value {
    let mut m = Map::new();
    m.insert("type".into(), json!(r.pick(names)));
    for i in 0..r.below(5) {
        let key = *r.pick(&["name", "sheet", "piece", "x", "size", "at", "label", "k"]);
        m.insert(
            if r.below(4) == 0 {
                format!("zz{i}")
            } else {
                key.to_string()
            },
            junk(r, 1),
        );
    }
    Value::Object(m)
}

fn request(id: &Value, method: &str, params: Value) -> String {
    json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}).to_string()
}

#[derive(Default, Debug)]
struct Cover {
    parse_err: u32,
    invalid_request: u32,
    method_not_found: u32,
    params_err: u32,
    tool_err: u32,
    tool_ok: u32,
    applied: u32,
    undone: u32,
    export_ok: u32,
    export_refused: u32,
    notifications: u32,
    oversize: u32,
}

fn disk(path: &Path) -> Vec<u8> {
    std::fs::read(path).expect("lecture du projet")
}

#[test]
fn five_hundred_random_requests_never_panic_and_every_error_is_typed() {
    let parent = std::env::temp_dir().join(format!("pf-mcp-adverse-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&parent);
    let dir = parent.join("p");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("demo.pforge.json");
    persist::save(&path, &pf_core::demo_project().unwrap()).unwrap();
    let mut server = pf_mcp::Server::with_write(path.clone());
    let names = pf_mcp::command_names();
    let mut r = Rng(0x9E37_79B9_7F4A_7C15);
    let mut cover = Cover::default();
    let allowed = [-32700, -32600, -32601, -32602, -32002];

    for n in 0..500u32 {
        let before = disk(&path);
        let revision = file_guard::revision_of(&before);
        let id = json!(n);
        let kind = r.below(14);
        let line: String = match kind {
            0 => {
                let valid = request(&id, "tools/list", json!({}));
                match r.below(5) {
                    0 => valid[..r.below(valid.len())].to_string(),
                    1 => "{".repeat(r.below(300) + 1),
                    2 => "[]".into(),
                    3 => format!("{{\"a\":{}{}", "[".repeat(300), "]".repeat(300)),
                    _ => {
                        (0..r.below(60))
                            .map(|_| char::from(32 + r.below(90) as u8))
                            .collect::<String>()
                            + "}}"
                    }
                }
            }
            1 => {
                json!({"jsonrpc": junk(&mut r, 2), "id": id, "method": junk(&mut r, 2)}).to_string()
            }
            2 => request(&id, &format!("m{}", r.below(1000)), junk(&mut r, 0)),
            3 => request(
                &id,
                "tools/call",
                match r.below(4) {
                    0 => json!({"arguments": {}}),
                    1 => json!({"name": junk(&mut r, 2)}),
                    2 => json!({"name": "list_sheets", "arguments": junk(&mut r, 2)}),
                    _ => junk(&mut r, 0),
                },
            ),
            4 | 5 => {
                let tool = *r.pick(&TOOLS);
                request(
                    &id,
                    "tools/call",
                    json!({"name": tool, "arguments": args_for(&mut r, tool, &revision)}),
                )
            }
            6 | 7 => {
                let cmd = if r.below(2) == 0 {
                    json!({"type": "rename_project", "name": format!("n{n}")})
                } else {
                    command_json(&mut r, &names)
                };
                let rev = if r.below(4) == 0 {
                    json!(junk(&mut r, 2))
                } else {
                    json!(revision)
                };
                let tool = if r.below(4) == 0 {
                    "preview_command"
                } else {
                    "apply_command"
                };
                request(
                    &id,
                    "tools/call",
                    json!({"name": tool, "arguments": {"command": cmd, "expected_revision": rev}}),
                )
            }
            8 => request(
                &id,
                "tools/call",
                json!({"name": "undo_last", "arguments": {}}),
            ),
            9 => {
                let p = if r.below(2) == 0 {
                    json!(r.pick(&NASTY_PATHS))
                } else {
                    json!(format!("out/f{}", r.below(6)))
                };
                let f = *r.pick(&["svg", "pdf", "png", "jpeg", "dxf", "hpgl", "bmp"]);
                let mut a = json!({"format": f, "path": p});
                if r.below(4) == 0 {
                    a["dpi"] = junk(&mut r, 2);
                }
                request(&id, "tools/call", json!({"name": "export", "arguments": a}))
            }
            10 => {
                cover.notifications += 1;
                let line =
                    json!({"jsonrpc":"2.0","method":"notifications/x","params":junk(&mut r, 1)})
                        .to_string();
                assert!(
                    server.handle_line(&line).is_none(),
                    "une notification n'a pas de réponse"
                );
                continue;
            }
            11 => request(&id, "resources/read", json!({"uri": junk(&mut r, 2)})),
            12 => request(
                &id,
                "tools/call",
                json!({"name": "apply_command", "arguments": {
                    "command": {"type": "rename_project", "name": "y".repeat(200_000)},
                    "expected_revision": revision}}),
            ),
            _ => {
                cover.oversize += 1;
                "x".repeat(1024 * 1024 + 10)
            }
        };

        if line.trim().is_empty() {
            assert!(
                server.handle_line(&line).is_none(),
                "ligne vide : pas de réponse"
            );
            continue;
        }
        let reply = server
            .handle_line(&line)
            .unwrap_or_else(|| panic!("réponse attendue ({kind}) : {line:.80}"));
        let v: Value = serde_json::from_str(&reply).expect("la réponse est du JSON");
        assert_eq!(v["jsonrpc"], "2.0");
        assert!(
            v.get("result").is_some() ^ v.get("error").is_some(),
            "result xor error : {reply:.200}"
        );
        let mut mutated = false;
        if let Some(e) = v.get("error") {
            let code = e["code"].as_i64().expect("code entier");
            assert!(
                allowed.contains(&code),
                "code {code} hors table (-32603 = panic ?) : {reply:.300}"
            );
            assert!(e["message"].as_str().is_some_and(|m| !m.is_empty()));
            match code {
                -32700 => cover.parse_err += 1,
                -32600 => cover.invalid_request += 1,
                -32601 => cover.method_not_found += 1,
                -32602 => cover.params_err += 1,
                _ => {}
            }
        } else if line.contains("\"tools/call\"") && v["result"].get("isError").is_some() {
            let res = &v["result"];
            let sc = &res["structuredContent"];
            assert!(sc.is_object(), "structuredContent objet : {reply:.200}");
            if res["isError"] == true {
                cover.tool_err += 1;
                assert!(
                    sc["error"].as_str().is_some_and(|m| !m.is_empty()),
                    "erreur typée : {reply:.200}"
                );
                if line.contains("\"export\"") {
                    cover.export_refused += 1;
                }
            } else {
                cover.tool_ok += 1;
                if sc["applied"] == true {
                    cover.applied += 1;
                    mutated = true;
                }
                if sc["undone"] == true {
                    cover.undone += 1;
                    mutated = true;
                }
                if sc["exported"] == true {
                    cover.export_ok += 1;
                }
            }
        }
        let after = disk(&path);
        if mutated {
            let rev = file_guard::revision_of(&after);
            assert_eq!(
                v["result"]["structuredContent"]["revision"], rev,
                "révision annoncée = empreinte du disque"
            );
            persist::load(&path).expect("le projet reste valide après chaque mutation");
        } else {
            assert_eq!(
                after, before,
                "requête {n} (genre {kind}) : un refus ne laisse aucune trace"
            );
        }
    }

    assert!(cover.parse_err >= 12, "{cover:?}");
    assert!(cover.invalid_request >= 20, "{cover:?}");
    assert!(cover.method_not_found >= 10, "{cover:?}");
    assert!(cover.params_err >= 20, "{cover:?}");
    assert!(cover.tool_err >= 80, "{cover:?}");
    assert!(cover.tool_ok >= 30, "{cover:?}");
    assert!(cover.applied >= 15, "{cover:?}");
    assert!(cover.undone >= 5, "{cover:?}");
    assert!(cover.export_ok >= 3, "{cover:?}");
    assert!(cover.export_refused >= 15, "{cover:?}");
    assert!(cover.notifications >= 10, "{cover:?}");
    assert!(cover.oversize >= 1, "{cover:?}");

    // Rien n'a été écrit hors du dossier du projet ni sous un nom inattendu.
    let siblings: Vec<_> = std::fs::read_dir(&parent).unwrap().collect();
    assert_eq!(siblings.len(), 1, "aucun fichier hors du dossier du projet");
    for e in std::fs::read_dir(&dir).unwrap() {
        let name = e.unwrap().file_name().to_string_lossy().into_owned();
        assert!(
            name == "demo.pforge.json"
                || name == "out"
                || name.starts_with("demo.pforge.json.mcp-journal"),
            "fichier inattendu : {name}"
        );
    }
    assert!(!dir.join("demo.pforge.json.mcp-write.lock").exists());
    let _ = std::fs::remove_dir_all(parent);
}
