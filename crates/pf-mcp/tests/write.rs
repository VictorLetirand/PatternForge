//! Outils d'écriture : lecture seule par défaut, preview, apply, undo.

use pf_mcp::Server;
use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;

struct Tmp(PathBuf);

impl Tmp {
    fn new(tag: &str) -> Tmp {
        let dir = std::env::temp_dir().join(format!("pf-mcp-w-{}-{tag}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("dossier temporaire");
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

fn setup(tag: &str, write: bool) -> (Tmp, Server) {
    let tmp = Tmp::new(tag);
    let project = pf_core::demo_project().expect("démo");
    pf_core::persist::save(&tmp.file(), &project).expect("enregistrement");
    let server = if write {
        Server::with_write(tmp.file())
    } else {
        Server::new(tmp.file())
    };
    (tmp, server)
}

fn call(server: &mut Server, name: &str, args: Value) -> Value {
    let line = json!({"jsonrpc":"2.0","id":1,"method":"tools/call",
        "params":{"name":name,"arguments":args}})
    .to_string();
    let out = server.handle_line(&line).expect("réponse");
    let r: Value = serde_json::from_str(&out).expect("JSON");
    assert!(r.get("error").is_none(), "erreur JSON-RPC : {r}");
    r["result"].clone()
}

fn ok(r: &Value) -> &Value {
    assert_eq!(r["isError"], false, "{r}");
    &r["structuredContent"]
}

fn err_msg(r: &Value) -> String {
    assert_eq!(r["isError"], true, "{r}");
    r["structuredContent"]["error"]
        .as_str()
        .expect("message")
        .to_string()
}

fn revision(server: &mut Server) -> String {
    let r = call(server, "list_sheets", json!({}));
    ok(&r)["revision"].as_str().expect("revision").to_string()
}

fn rename(name: &str) -> Value {
    json!({"type": "rename_project", "name": name})
}

#[test]
fn read_only_by_default() {
    let (t, mut s) = setup("ro", false);
    let before = fs::read(t.file()).unwrap();
    let list = s
        .handle_line(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#)
        .unwrap();
    let list: Value = serde_json::from_str(&list).unwrap();
    let names: Vec<&str> = list["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|t| t["name"].as_str())
        .collect();
    assert_eq!(names.len(), 9);
    for w in ["preview_command", "apply_command", "undo_last"] {
        assert!(!names.contains(&w));
        let r = call(
            &mut s,
            w,
            json!({"command": rename("X"), "expected_revision": "sha256:0"}),
        );
        assert!(err_msg(&r).contains("--allow-write"));
    }
    assert_eq!(fs::read(t.file()).unwrap(), before);
    assert!(!t.0.join("demo.pforge.json.mcp-journal.jsonl").exists());
}

#[test]
fn write_tools_listed_when_enabled() {
    let (_t, mut s) = setup("list", true);
    let list = s
        .handle_line(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#)
        .unwrap();
    let list: Value = serde_json::from_str(&list).unwrap();
    assert_eq!(list["result"]["tools"].as_array().unwrap().len(), 15);
}

#[test]
fn revision_is_content_hash() {
    let (t, mut s) = setup("rev", true);
    let rev = revision(&mut s);
    assert!(rev.starts_with("sha256:") && rev.len() == 7 + 64);
    let v = call(&mut s, "validate_project", json!({}));
    assert_eq!(ok(&v)["revision"], rev);
    // Même contenu, même révision ; contenu différent, révision différente.
    assert_eq!(revision(&mut s), rev);
    let mut bytes = fs::read(t.file()).unwrap();
    bytes.extend_from_slice(b"\n");
    fs::write(t.file(), bytes).unwrap();
    assert_ne!(revision(&mut s), rev);
}

#[test]
fn preview_has_no_disk_effect() {
    let (t, mut s) = setup("preview", true);
    let before = fs::read(t.file()).unwrap();
    let rev = revision(&mut s);
    let r = call(
        &mut s,
        "preview_command",
        json!({"command": rename("Aperçu")}),
    );
    let out = ok(&r);
    assert_eq!(out["applied"], false);
    assert_eq!(out["revision"], rev);
    assert_eq!(out["validation"]["valid"], true);
    assert!(out["outcome"].is_object() && out["notes"].is_array());
    assert_ne!(out["revision_after"], rev);
    assert_eq!(fs::read(t.file()).unwrap(), before);
    assert!(!t.0.join("demo.pforge.json.mcp-journal.jsonl").exists());
}

#[test]
fn apply_then_file_is_valid_and_journaled() {
    let (t, mut s) = setup("apply", true);
    let rev = revision(&mut s);
    let r = call(
        &mut s,
        "apply_command",
        json!({"command": rename("Renommé"), "expected_revision": rev}),
    );
    let out = ok(&r);
    assert_eq!(out["applied"], true);
    let new_rev = out["revision"].as_str().unwrap().to_string();
    assert_ne!(new_rev, rev);
    let project = pf_core::persist::load(&t.file()).expect("fichier valide");
    assert_eq!(project.name, "Renommé");
    assert_eq!(revision(&mut s), new_rev);
    let journal = fs::read_to_string(t.0.join("demo.pforge.json.mcp-journal.jsonl")).unwrap();
    let lines: Vec<Value> = journal
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0]["revision_before"], rev);
    assert_eq!(lines[0]["revision_after"], new_rev);
    assert_eq!(lines[0]["command"], rename("Renommé"));
    assert!(lines[0]["notes"].is_array());
    // Aucun fichier temporaire résiduel.
    let names: Vec<String> = fs::read_dir(&t.0)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names.len(), 2, "{names:?}");
}

#[test]
fn apply_requires_and_checks_revision() {
    let (t, mut s) = setup("stale", true);
    let before = fs::read(t.file()).unwrap();
    let r = call(&mut s, "apply_command", json!({"command": rename("X")}));
    assert!(err_msg(&r).contains("expected_revision"));
    let r = call(
        &mut s,
        "apply_command",
        json!({"command": rename("X"), "expected_revision": "sha256:deadbeef"}),
    );
    assert!(err_msg(&r).contains("révision périmée"));
    assert_eq!(fs::read(t.file()).unwrap(), before);
    assert!(!t.0.join("demo.pforge.json.mcp-journal.jsonl").exists());

    // Le fichier change entre la lecture de la révision et l'application.
    let rev = revision(&mut s);
    let mut p = pf_core::persist::load(&t.file()).unwrap();
    p.apply(serde_json::from_value(rename("Modifié ailleurs")).unwrap())
        .unwrap();
    pf_core::persist::save(&t.file(), &p).unwrap();
    let external = fs::read(t.file()).unwrap();
    let r = call(
        &mut s,
        "apply_command",
        json!({"command": rename("X"), "expected_revision": rev}),
    );
    assert!(err_msg(&r).contains("révision périmée"));
    assert_eq!(fs::read(t.file()).unwrap(), external);
}

#[test]
fn invalid_commands_refused_without_change() {
    let (t, mut s) = setup("invalid", true);
    let before = fs::read(t.file()).unwrap();
    let rev = revision(&mut s);
    let bad = [
        json!({"type": "delete_sheet", "sheet": 987654}),
        json!({"type": "commande_inconnue"}),
        json!("{pas du json"),
        json!([1, 2]),
    ];
    for command in bad {
        let r = call(
            &mut s,
            "apply_command",
            json!({"command": command, "expected_revision": rev}),
        );
        assert!(!err_msg(&r).is_empty());
        let r = call(&mut s, "preview_command", json!({"command": command}));
        assert!(!err_msg(&r).is_empty());
    }
    assert_eq!(fs::read(t.file()).unwrap(), before);
    assert!(!t.0.join("demo.pforge.json.mcp-journal.jsonl").exists());
}

#[test]
fn undo_restores_byte_for_byte() {
    let (t, mut s) = setup("undo", true);
    let original = fs::read(t.file()).unwrap();
    let rev0 = revision(&mut s);
    let r = call(
        &mut s,
        "apply_command",
        json!({"command": rename("Un"), "expected_revision": rev0}),
    );
    let rev1 = ok(&r)["revision"].as_str().unwrap().to_string();
    let after_first = fs::read(t.file()).unwrap();
    let r = call(
        &mut s,
        "apply_command",
        json!({"command": rename("Deux"), "expected_revision": rev1}),
    );
    assert_eq!(r["isError"], false);

    let r = call(&mut s, "undo_last", json!({}));
    assert_eq!(ok(&r)["revision"], rev1);
    assert_eq!(fs::read(t.file()).unwrap(), after_first);
    let r = call(&mut s, "undo_last", json!({}));
    assert_eq!(ok(&r)["revision"], rev0);
    assert_eq!(fs::read(t.file()).unwrap(), original);
    // Plus rien à annuler.
    let r = call(&mut s, "undo_last", json!({}));
    assert!(err_msg(&r).contains("rien à annuler"));
    assert_eq!(fs::read(t.file()).unwrap(), original);
}

#[test]
fn undo_without_history_refused() {
    let (t, mut s) = setup("nohist", true);
    let before = fs::read(t.file()).unwrap();
    let r = call(&mut s, "undo_last", json!({}));
    assert!(err_msg(&r).contains("rien à annuler"));
    assert_eq!(fs::read(t.file()).unwrap(), before);
}

#[test]
fn undo_refused_if_file_changed_externally() {
    let (t, mut s) = setup("undo-ext", true);
    let rev = revision(&mut s);
    call(
        &mut s,
        "apply_command",
        json!({"command": rename("Un"), "expected_revision": rev}),
    );
    let mut p = pf_core::persist::load(&t.file()).unwrap();
    p.apply(serde_json::from_value(rename("Externe")).unwrap())
        .unwrap();
    pf_core::persist::save(&t.file(), &p).unwrap();
    let external = fs::read(t.file()).unwrap();
    let r = call(&mut s, "undo_last", json!({}));
    assert!(err_msg(&r).contains("a changé"));
    assert_eq!(fs::read(t.file()).unwrap(), external);
}
