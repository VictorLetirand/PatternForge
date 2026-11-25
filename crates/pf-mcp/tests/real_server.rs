//! Preuves de bout en bout du lot K : on lance le **vrai binaire** `pf-mcp-server` et on dialogue
//! en JSON-RPC sur stdio, avec un projet temporaire.

use pf_core::{file_guard, persist, Document, ExternalChange, SaveError};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

struct Rpc {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next: u64,
}

impl Rpc {
    fn start(path: &Path, write: bool, env: &[(&str, &str)]) -> Rpc {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_pf-mcp-server"));
        cmd.arg("--project").arg(path);
        if write {
            cmd.arg("--allow-write");
        }
        for (k, v) in env {
            cmd.env(k, v);
        }
        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("lancement du serveur");
        let stdin = child.stdin.take().expect("stdin");
        let stdout = BufReader::new(child.stdout.take().expect("stdout"));
        Rpc {
            child,
            stdin,
            stdout,
            next: 1,
        }
    }

    fn raw(&mut self, line: &str) -> Value {
        writeln!(self.stdin, "{line}").expect("écriture");
        let mut out = String::new();
        self.stdout.read_line(&mut out).expect("lecture");
        serde_json::from_str(out.trim_end()).expect("réponse JSON")
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        self.next += 1;
        let id = self.next;
        let r =
            self.raw(&json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}).to_string());
        assert_eq!(r["id"], id);
        r
    }

    /// Appel d'outil : renvoie `(isError, structuredContent)`.
    fn tool(&mut self, name: &str, args: Value) -> (bool, Value) {
        let r = self.request("tools/call", json!({"name": name, "arguments": args}));
        assert!(r.get("error").is_none(), "erreur de protocole : {r}");
        (
            r["result"]["isError"].as_bool().expect("isError"),
            r["result"]["structuredContent"].clone(),
        )
    }

    fn ok(&mut self, name: &str, args: Value) -> Value {
        let (err, v) = self.tool(name, args);
        assert!(!err, "{name} refusé : {v}");
        v
    }

    fn refused(&mut self, name: &str, args: Value) -> String {
        let (err, v) = self.tool(name, args);
        assert!(err, "{name} aurait dû être refusé : {v}");
        v["error"].as_str().expect("message").to_string()
    }

    fn revision(&mut self) -> String {
        self.ok("list_sheets", json!({}))["revision"]
            .as_str()
            .expect("révision")
            .to_string()
    }

    fn finish(mut self) {
        drop(self.stdin);
        let _ = self.child.wait();
    }
}

fn project_dir(tag: &str) -> (PathBuf, PathBuf) {
    let dir = std::env::temp_dir().join(format!("pf-mcp-real-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("dossier");
    let path = dir.join("demo.pforge.json");
    persist::save(&path, &pf_core::demo_project().expect("démo")).expect("projet");
    (dir, path)
}

fn rename(name: &str) -> Value {
    json!({"type": "rename_project", "name": name})
}

fn point(out: &Value, id: u64) -> (f64, f64) {
    let p = out["points"]
        .as_array()
        .expect("points")
        .iter()
        .find(|p| p["id"] == id)
        .expect("point");
    (p["x_mm"].as_f64().unwrap(), p["y_mm"].as_f64().unwrap())
}

#[test]
fn every_tool_category_over_the_real_binary_with_undo() {
    let (dir, path) = project_dir("all");
    let original = std::fs::read(&path).unwrap();
    let mut s = Rpc::start(&path, true, &[]);

    // Découverte : 15 outils, 229 commandes.
    let list = s.request("tools/list", json!({}));
    assert_eq!(list["result"]["tools"].as_array().unwrap().len(), 15);
    let cmds = s.ok("list_commands", json!({}));
    assert_eq!(cmds["count"], 229);

    // validate : projet sain, sans verrou d'application.
    let v = s.ok("validate", json!({}));
    assert_eq!(v["valid"], true);
    assert_eq!(v["sheets"], 3);
    assert_eq!(v["application_lock"], false);
    let r0 = s.revision();
    assert_eq!(v["revision"], r0);
    assert_eq!(r0, file_guard::revision_of(&original));

    // Commande générique : gamme de tailles S, M, L (base M) puis écart de gradation du point C.
    let r1 = {
        let a = s.ok(
            "apply_command",
            json!({"command": {"type":"add_size_range","name":"Tailles","kind":"alphanumeric",
                    "labels":["S","M","L"],"base":1}, "expected_revision": r0}),
        );
        a["revision"].as_str().unwrap().to_string()
    };
    let size_l = {
        let p = persist::load(&path).unwrap();
        p.sizes[0].sizes[2].id.0
    };
    let r2 = s.ok(
        "apply_command",
        json!({"command": {"type":"set_point_grading","sheet":1,"point":7,"size":size_l,
                    "dx_mm":5.0,"dy_mm":3.0}, "expected_revision": r1}),
    )["revision"]
        .as_str()
        .unwrap()
        .to_string();

    // grade_size : lecture seule ; C = (120 ; 80) en base, (125 ; 83) en L (120 + 5, 80 + 3).
    let base = s.ok("grade_size", json!({"sheet_id": 1}));
    assert_eq!(point(&base, 7), (120.0, 80.0));
    assert_eq!(base["size_id"], Value::Null);
    let l = s.ok("grade_size", json!({"sheet_id": 1, "size_id": size_l}));
    assert_eq!(point(&l, 7), (125.0, 83.0));
    assert_eq!(l["size_label"], "L");
    assert_eq!(s.revision(), r2, "grade_size ne modifie rien");
    let msg = s.refused("grade_size", json!({"sheet_id": 1, "size_id": 987_654}));
    assert!(!msg.is_empty());
    s.refused("grade_size", json!({"sheet_id": 999}));

    // extract_piece : un clic dans la région fermée AB, BC, courbe AC.
    let before_refusals = std::fs::read(&path).unwrap();
    s.refused(
        "extract_piece",
        json!({"sheet_id": 1, "x_mm": 100.0, "y_mm": 10.0, "name": "P", "expected_revision": "sha256:0"}),
    );
    s.refused(
        "extract_piece",
        json!({"sheet_id": 1, "x_mm": 100.0, "name": "P", "expected_revision": r2}),
    );
    s.refused(
        "extract_piece",
        json!({"sheet_id": 1, "x_mm": 1.0, "y_mm": 1.0, "entity_ids": [6], "name": "P", "expected_revision": r2}),
    );
    s.refused(
        "extract_piece",
        json!({"sheet_id": 1, "x_mm": 5000.0, "y_mm": 5000.0, "name": "P", "expected_revision": r2}),
    );
    assert_eq!(
        std::fs::read(&path).unwrap(),
        before_refusals,
        "refus sans trace"
    );
    let e = s.ok(
        "extract_piece",
        json!({"sheet_id": 1, "x_mm": 100.0, "y_mm": 10.0, "name": "Devant", "expected_revision": r2}),
    );
    assert_eq!(e["applied"], true);
    let r3 = e["revision"].as_str().unwrap().to_string();
    let sheets = s.ok("list_sheets", json!({}));
    let piece = sheets["sheets"][0]["pieces"][0].clone();
    assert_eq!(piece["name"], "Devant");
    let piece_id = piece["id"].as_u64().unwrap();

    // apply_seam_allowance : marge de 10 mm ; refus d'une valeur hors bornes sans trace.
    s.refused(
        "apply_seam_allowance",
        json!({"piece_id": piece_id, "default_mm": 5000.0, "expected_revision": r3}),
    );
    assert_eq!(s.revision(), r3);
    let a = s.ok(
        "apply_seam_allowance",
        json!({"piece_id": piece_id, "default_mm": 10.0, "expected_revision": r3}),
    );
    let r4 = a["revision"].as_str().unwrap().to_string();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("\"default_mm\": 10"), "marge écrite");

    // export : chaque format, chemin relatif au dossier du projet.
    for (fmt, file, magic) in [
        ("svg", "out/demo.svg", &b"<?xml"[..]),
        ("pdf", "out/demo.pdf", &b"%PDF"[..]),
        ("png", "out/demo.png", &[0x89, b'P', b'N', b'G'][..]),
        ("jpeg", "out/demo.jpg", &[0xFF, 0xD8][..]),
    ] {
        let r = s.ok("export", json!({"format": fmt, "path": file}));
        assert_eq!(r["exported"], true, "{fmt}");
        let bytes = std::fs::read(dir.join(file)).expect("fichier exporté");
        assert!(
            bytes.starts_with(magic),
            "{fmt} : {:?}",
            &bytes[..8.min(bytes.len())]
        );
    }
    for fmt in ["dxf", "hpgl"] {
        s.ok(
            "export",
            json!({"format": fmt, "path": format!("out/{fmt}")}),
        );
    }
    // 100 mm à 300 ppp = 1181 px : largeur lue dans l'en-tête PNG (octets 16 à 19).
    s.ok(
        "export",
        json!({"format": "png", "path": "out/dpi.png", "dpi": 300}),
    );
    s.ok(
        "export",
        json!({"format": "png", "path": "out/dpi36.png", "dpi": 36}),
    );
    let big = std::fs::read(dir.join("out/dpi.png")).unwrap();
    let small = std::fs::read(dir.join("out/dpi36.png")).unwrap();
    let width = |b: &[u8]| u32::from_be_bytes([b[16], b[17], b[18], b[19]]);
    assert!(width(&big) > width(&small) * 8, "300 ppp ≈ 8,33 × 36 ppp");

    // Refus d'export : écrasement, évasion du dossier, fichier projet, format, dpi.
    let svg_before = std::fs::read(dir.join("out/demo.svg")).unwrap();
    s.refused("export", json!({"format": "svg", "path": "out/demo.svg"}));
    s.ok(
        "export",
        json!({"format": "svg", "path": "out/demo.svg", "overwrite": true}),
    );
    assert_eq!(std::fs::read(dir.join("out/demo.svg")).unwrap(), svg_before);
    let outside = path.parent().unwrap().parent().unwrap().join("evil.svg");
    let _ = std::fs::remove_file(&outside);
    for bad in [
        "../evil.svg",
        "..\\evil.svg",
        "a/../../evil.svg",
        "demo.pforge.json",
        "x.json",
        "",
        "NUL\0x",
        "con",
        "NUL.svg",
        "out/COM3",
    ] {
        s.refused("export", json!({"format": "svg", "path": bad}));
    }
    assert!(!outside.exists());
    let abs_outside =
        std::env::temp_dir().join(format!("pf-mcp-abs-evil-{}.svg", std::process::id()));
    let _ = std::fs::remove_file(&abs_outside);
    s.refused(
        "export",
        json!({"format": "svg", "path": abs_outside.to_string_lossy()}),
    );
    assert!(!abs_outside.exists());
    s.refused("export", json!({"format": "bmp", "path": "o.bmp"}));
    s.refused(
        "export",
        json!({"format": "svg", "path": "o.svg", "dpi": 300}),
    );
    s.refused(
        "export",
        json!({"format": "png", "path": "o.png", "dpi": 5}),
    );
    assert_eq!(s.revision(), r4, "aucun export ne touche le projet");

    // Annulation de bout en bout par l'API : quatre mutations, quatre annulations, octets d'origine.
    assert_eq!(s.ok("undo_last", json!({}))["revision"], r3);
    assert_eq!(s.ok("undo_last", json!({}))["revision"], r2);
    assert_eq!(s.ok("undo_last", json!({}))["revision"], r1);
    assert_eq!(s.ok("undo_last", json!({}))["revision"], r0);
    assert_eq!(std::fs::read(&path).unwrap(), original);
    s.refused("undo_last", json!({}));
    assert!(
        !dir.join("demo.pforge.json.mcp-write.lock").exists(),
        "verrou libéré"
    );
    s.finish();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn read_only_server_refuses_the_new_write_tools_and_keeps_reading() {
    let (dir, path) = project_dir("ro");
    let before = std::fs::read(&path).unwrap();
    let mut s = Rpc::start(&path, false, &[]);
    for (name, args) in [
        (
            "extract_piece",
            json!({"sheet_id":1,"x_mm":1.0,"y_mm":1.0,"name":"P","expected_revision":"sha256:0"}),
        ),
        (
            "apply_seam_allowance",
            json!({"piece_id":1,"default_mm":1.0,"expected_revision":"sha256:0"}),
        ),
        ("export", json!({"format":"svg","path":"o.svg"})),
    ] {
        assert!(s.refused(name, args).contains("--allow-write"), "{name}");
    }
    assert_eq!(s.ok("list_commands", json!({}))["count"], 229);
    assert_eq!(s.ok("validate", json!({}))["valid"], true);
    assert_eq!(s.ok("grade_size", json!({"sheet_id": 1}))["sheet_id"], 1);
    assert!(!dir.join("o.svg").exists());
    assert_eq!(std::fs::read(&path).unwrap(), before);
    s.finish();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn validate_reports_a_corrupted_project_without_touching_it() {
    let (dir, path) = project_dir("bad");
    std::fs::write(&path, "{ \"schema_version\": 5, \"name\": ").unwrap();
    let before = std::fs::read(&path).unwrap();
    let mut s = Rpc::start(&path, true, &[]);
    let v = s.ok("validate", json!({}));
    assert_eq!(v["valid"], false);
    assert!(v["error"].as_str().is_some_and(|m| !m.is_empty()));
    assert!(v["line"].is_number());
    s.refused(
        "apply_command",
        json!({"command": rename("X"), "expected_revision": "sha256:0"}),
    );
    s.refused("grade_size", json!({"sheet_id": 1}));
    assert_eq!(std::fs::read(&path).unwrap(), before);
    s.finish();
    let _ = std::fs::remove_dir_all(dir);
}

/// Dialogue avec l'application ouverte : ce que le test prouve, et seulement cela.
/// Le serveur écrit le fichier ; le document de l'application (`Document`, celui de
/// `pf-desktop`) détecte la modification externe et refuse d'écraser. L'affichage en direct dans
/// la fenêtre n'existe pas (l'interface ne surveille pas le fichier) : voir le contrat.
#[test]
fn the_open_application_detects_the_mcp_change_and_refuses_to_overwrite_it() {
    let (dir, path) = project_dir("app");
    let mut app = Document::open(&path).expect("ouverture par l'application");
    assert_eq!(app.external_change().unwrap(), ExternalChange::Unchanged);

    let mut s = Rpc::start(&path, true, &[]);
    let rev = s.revision();
    s.ok(
        "apply_command",
        json!({"command": rename("Par le MCP"), "expected_revision": rev}),
    );
    let mcp_bytes = std::fs::read(&path).unwrap();

    assert_eq!(app.external_change().unwrap(), ExternalChange::Modified);
    app.execute(pf_core::Command::RenameProject {
        name: "Local".into(),
    })
    .expect("modification locale");
    match app.save_checked(&path) {
        Err(SaveError::External(ExternalChange::Modified)) => {}
        other => panic!("l'enregistrement aurait dû être refusé : {other:?}"),
    }
    assert_eq!(std::fs::read(&path).unwrap(), mcp_bytes, "rien d'écrasé");

    // Après rechargement par l'utilisateur, la modification MCP est visible.
    let reopened = Document::open(&path).expect("réouverture");
    assert_eq!(reopened.project().name, "Par le MCP");
    assert_eq!(
        reopened.external_change().unwrap(),
        ExternalChange::Unchanged
    );

    // L'annulation MCP rend le fichier d'origine, que l'application détecte à nouveau.
    s.ok("undo_last", json!({}));
    assert_eq!(
        reopened.external_change().unwrap(),
        ExternalChange::Modified
    );
    assert_eq!(
        Document::open(&path).unwrap().project().name,
        pf_core::demo_project().unwrap().name
    );
    s.finish();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn write_lock_is_advisory_named_and_released() {
    let (dir, path) = project_dir("lock");
    let lock = dir.join("demo.pforge.json.mcp-write.lock");
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let mut s = Rpc::start(&path, true, &[]);
    let rev = s.revision();
    let before = std::fs::read(&path).unwrap();

    // Verrou récent tenu par un autre processus : refus nommé, rien d'écrit, verrou intact.
    std::fs::write(&lock, format!("424242 {now}")).unwrap();
    let msg = s.refused(
        "apply_command",
        json!({"command": rename("X"), "expected_revision": rev}),
    );
    assert!(
        msg.contains("verrou d'écriture") && msg.contains("424242"),
        "{msg}"
    );
    let msg = s.refused("undo_last", json!({}));
    assert!(msg.contains("verrou d'écriture"), "{msg}");
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert_eq!(
        std::fs::read_to_string(&lock).unwrap(),
        format!("424242 {now}")
    );

    // Verrou abandonné (plus de 45 s) : repris, puis retiré.
    std::fs::write(&lock, format!("424242 {}", now - 120)).unwrap();
    s.ok(
        "apply_command",
        json!({"command": rename("Y"), "expected_revision": rev}),
    );
    assert!(!lock.exists());
    s.finish();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn journal_rotates_to_five_files_and_never_loses_the_last_entry() {
    let (dir, path) = project_dir("rot");
    let size = std::fs::metadata(&path).unwrap().len();
    // Une entrée pèse environ un projet : on tourne toutes les deux ou trois mutations.
    let limit = (size * 3).to_string();
    let mut s = Rpc::start(&path, true, &[("PF_MCP_JOURNAL_MAX_BYTES", limit.as_str())]);
    let journal = dir.join("demo.pforge.json.mcp-journal.jsonl");
    let mut revisions = vec![s.revision()];
    for n in 1..=14 {
        let rev = revisions.last().unwrap().clone();
        let r = s.ok(
            "apply_command",
            json!({"command": rename(&format!("v{n}")), "expected_revision": rev}),
        );
        revisions.push(r["revision"].as_str().unwrap().to_string());
        let last: Value = serde_json::from_str(
            std::fs::read_to_string(&journal)
                .unwrap()
                .lines()
                .last()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            last["seq"], n,
            "la dernière entrée est toujours dans le journal courant"
        );
        assert_eq!(last["revision_after"], *revisions.last().unwrap());
    }
    let mut files: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.contains("mcp-journal"))
        .collect();
    files.sort();
    assert_eq!(
        files,
        [
            "demo.pforge.json.mcp-journal.jsonl",
            "demo.pforge.json.mcp-journal.jsonl.1",
            "demo.pforge.json.mcp-journal.jsonl.2",
            "demo.pforge.json.mcp-journal.jsonl.3",
            "demo.pforge.json.mcp-journal.jsonl.4"
        ]
    );
    // L'annulation fonctionne après rotation, plusieurs fois de suite (elle franchit au moins une
    // frontière de rotation) : chaque pas revient à la révision précédente, octet par octet.
    for k in (11..=13).rev() {
        let undone = s.ok("undo_last", json!({}));
        assert_eq!(undone["revision"], revisions[k]);
        assert_eq!(
            file_guard::file_revision(&path).unwrap().unwrap(),
            revisions[k]
        );
    }
    s.finish();
    let _ = std::fs::remove_dir_all(dir);
}
