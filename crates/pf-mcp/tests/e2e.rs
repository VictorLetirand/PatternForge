//! Test de bout en bout : lance réellement `pf-mcp-server` et dialogue par stdio.

use serde_json::{json, Value};
use std::io::Write;
use std::process::{Command, Stdio};

#[test]
fn binary_speaks_json_rpc_over_stdio() {
    let dir = std::env::temp_dir().join(format!("pf-mcp-e2e-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("dossier");
    let path = dir.join("demo.pforge.json");
    let project = pf_core::demo_project().expect("démo");
    pf_core::persist::save(&path, &project).expect("enregistrement");
    let before = std::fs::read(&path).expect("lecture");

    let mut child = Command::new(env!("CARGO_BIN_EXE_pf-mcp-server"))
        .arg("--project")
        .arg(&path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("lancement du serveur");

    let requests = [
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}),
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
        json!({"jsonrpc":"2.0","id":"x","method":"tools/call","params":{"name":"list_sheets","arguments":{}}}),
    ];
    {
        let mut stdin = child.stdin.take().expect("stdin");
        for r in &requests {
            writeln!(stdin, "{r}").expect("écriture");
        }
        writeln!(stdin, "pas du json").expect("écriture");
        // Fermeture de stdin en sortie de bloc : le serveur s'arrête.
    }
    let out = child.wait_with_output().expect("fin du serveur");
    assert!(out.status.success());
    let text = String::from_utf8(out.stdout).expect("UTF-8");
    let replies: Vec<Value> = text
        .lines()
        .map(|l| serde_json::from_str(l).expect("ligne JSON"))
        .collect();
    // La notification ne produit aucune réponse : 4 réponses attendues.
    assert_eq!(replies.len(), 4);
    assert_eq!(replies[0]["id"], 1);
    assert_eq!(replies[0]["result"]["serverInfo"]["name"], "patterncraft");
    assert_eq!(
        replies[1]["result"]["tools"].as_array().map(Vec::len),
        Some(9)
    );
    assert_eq!(replies[2]["id"], "x");
    assert_eq!(replies[2]["result"]["isError"], false);
    assert_eq!(replies[3]["error"]["code"], -32700);

    assert_eq!(std::fs::read(&path).expect("relecture"), before);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn binary_refuses_missing_arguments() {
    let out = Command::new(env!("CARGO_BIN_EXE_pf-mcp-server"))
        .output()
        .expect("lancement");
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
}
