//! Les neuf outils d'origine gardent mêmes noms et mêmes réponses, à l'octet.
//!
//! `tests/golden/existing_tools.jsonl` a été produit par le serveur de `main` (avant le lot K) avec
//! `PF_MCP_GOLDEN_OUT=<fichier> cargo test -p pf-mcp --test existing_tools_bytes` exécuté sur ce
//! même fichier de test. Le test compare ensuite le serveur courant à cette référence.

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

const ORIGINAL: [&str; 9] = [
    "list_sheets",
    "list_entities",
    "measure_entity",
    "measure_angle",
    "measure_contours",
    "validate_project",
    "preview_command",
    "apply_command",
    "undo_last",
];

struct Session {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    dir: String,
    out: Vec<String>,
}

impl Session {
    fn ask(&mut self, request: Value) -> Value {
        writeln!(self.stdin, "{request}").expect("écriture");
        let mut line = String::new();
        self.stdout.read_line(&mut line).expect("lecture");
        let line = line.trim_end().replace(&self.dir, "<DIR>");
        self.out.push(line.clone());
        serde_json::from_str(&line).expect("réponse JSON")
    }

    fn call(&mut self, id: u64, name: &str, args: Value) -> Value {
        let r = self.ask(
            json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":name,"arguments":args}}),
        );
        r["result"]["structuredContent"].clone()
    }
}

fn transcript() -> Vec<String> {
    let dir = std::env::temp_dir().join(format!("pf-mcp-golden-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("dossier");
    let path = dir.join("demo.pforge.json");
    pf_core::persist::save(&path, &pf_core::demo_project().expect("démo")).expect("projet");
    let mut child = Command::new(env!("CARGO_BIN_EXE_pf-mcp-server"))
        .arg("--project")
        .arg(&path)
        .arg("--allow-write")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("lancement");
    let stdin = child.stdin.take().expect("stdin");
    let stdout = BufReader::new(child.stdout.take().expect("stdout"));
    let mut s = Session {
        child,
        stdin,
        stdout,
        dir: dir.to_string_lossy().into_owned(),
        out: Vec::new(),
    };
    s.ask(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}));
    let list = s.ask(json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}));
    // Seules les définitions d'origine sont comparées : le lot K en ajoute d'autres.
    let originals: Vec<Value> = list["result"]["tools"]
        .as_array()
        .expect("outils")
        .iter()
        .filter(|t| ORIGINAL.contains(&t["name"].as_str().unwrap_or("")))
        .cloned()
        .collect();
    s.out.pop();
    s.out.push(serde_json::to_string(&originals).expect("json"));

    let sheets = s.call(3, "list_sheets", json!({}));
    let revision = sheets["revision"].as_str().expect("révision").to_string();
    let sheet = sheets["sheets"][0]["id"].as_u64().expect("feuille");
    let ents = s.call(4, "list_entities", json!({"sheet_id": sheet}));
    let ids: Vec<u64> = ents["paths"]
        .as_array()
        .expect("tracés")
        .iter()
        .filter_map(|p| p["id"].as_u64())
        .collect();
    s.call(
        5,
        "measure_entity",
        json!({"sheet_id": sheet, "entity_id": ids[0]}),
    );
    s.call(
        6,
        "measure_angle",
        json!({"sheet_id": sheet, "first_entity_id": ids[0], "second_entity_id": ids[1]}),
    );
    s.call(7, "measure_contours", json!({"sheet_id": sheet}));
    s.call(8, "list_entities", json!({"sheet_id": 999_999}));
    s.call(9, "validate_project", json!({}));
    s.ask(json!({"jsonrpc":"2.0","id":10,"method":"resources/list"}));
    s.ask(json!({"jsonrpc":"2.0","id":11,"method":"resources/read","params":{"uri":"patterncraft://project/summary"}}));
    s.ask(json!({"jsonrpc":"2.0","id":12,"method":"tools/call","params":{"name":"inconnu","arguments":{}}}));
    s.ask(json!({"jsonrpc":"2.0","id":13,"method":"tools/call","params":{"name":"list_entities","arguments":{}}}));
    let rename = json!({"type":"rename_project","name":"Golden"});
    s.call(14, "preview_command", json!({"command": rename}));
    s.call(
        15,
        "apply_command",
        json!({"command": rename, "expected_revision": "sha256:0"}),
    );
    s.call(16, "apply_command", json!({"command": rename}));
    s.call(
        17,
        "apply_command",
        json!({"command": {"type":"zzz"}, "expected_revision": revision}),
    );
    s.call(
        18,
        "apply_command",
        json!({"command": rename, "expected_revision": revision}),
    );
    s.call(19, "list_sheets", json!({}));
    s.call(20, "undo_last", json!({}));
    s.call(21, "undo_last", json!({}));
    s.call(22, "validate_project", json!({}));
    drop(s.stdin);
    s.child.wait().expect("fin");
    let _ = std::fs::remove_dir_all(&dir);
    s.out
}

#[test]
fn original_tools_keep_their_bytes() {
    let now = transcript();
    if let Ok(target) = std::env::var("PF_MCP_GOLDEN_OUT") {
        std::fs::write(target, now.join("\n") + "\n").expect("écriture de la référence");
        return;
    }
    let golden = include_str!("golden/existing_tools.jsonl");
    let expected: Vec<&str> = golden.lines().collect();
    assert_eq!(now.len(), expected.len());
    for (i, (a, b)) in now.iter().zip(&expected).enumerate() {
        assert_eq!(a, b, "réponse {i} différente de la référence de main");
    }
}
