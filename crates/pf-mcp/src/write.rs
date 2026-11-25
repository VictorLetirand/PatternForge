//! Outils d'écriture (activés par `--allow-write`) : prévisualisation,
//! application avec contrôle de révision, annulation via un journal.
//!
//! Le journal `<projet>.mcp-journal.jsonl` contient une ligne JSON par
//! mutation (ou annulation) ; chaque mutation embarque l'instantané complet du
//! fichier d'avant, ce qui permet de restaurer octet par octet.

use pf_core::{persist, Command, CoreError, Project};
use serde_json::{json, Map, Value};
use std::fs;
use std::io::Write;
use std::path::{Component, Path, PathBuf};

const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;

/// Taille au-delà de laquelle le journal est tourné (`PF_MCP_JOURNAL_MAX_BYTES` la remplace).
const JOURNAL_MAX_BYTES: u64 = 8 * 1024 * 1024;
/// Journal courant + fichiers tournés `.1` à `.4` : 5 fichiers au plus.
const JOURNAL_ROTATED: u32 = 4;
/// Âge (secondes) au-delà duquel un verrou d'écriture non libéré est considéré abandonné.
const WRITE_LOCK_STALE_SECS: u64 = 45;

pub(crate) const WRITE_TOOLS: [&str; 6] = [
    "preview_command",
    "apply_command",
    "undo_last",
    "extract_piece",
    "apply_seam_allowance",
    "export",
];

pub(crate) fn is_write_tool(name: &str) -> bool {
    WRITE_TOOLS.contains(&name)
}

/// Erreur métier d'un outil (renvoyée avec `isError: true`).
pub(crate) struct ToolError(pub String);

impl From<CoreError> for ToolError {
    fn from(e: CoreError) -> Self {
        ToolError(e.to_string())
    }
}

/// Contenu lu du fichier projet, sa révision et le projet validé.
pub(crate) struct State {
    pub text: String,
    pub revision: String,
    pub project: Project,
}

/// Lit le fichier une seule fois : la révision et le projet décrivent
/// exactement les mêmes octets.
pub(crate) fn read_state(path: &Path) -> Result<State, CoreError> {
    let io = |e: std::io::Error| CoreError::Io(format!("{} : {e}", path.display()));
    let meta = fs::metadata(path).map_err(io)?;
    if meta.len() > MAX_FILE_BYTES {
        return Err(CoreError::Malformed(
            "fichier trop volumineux pour un projet".into(),
        ));
    }
    let bytes = fs::read(path).map_err(io)?;
    let revision = revision_of(&bytes);
    let text = String::from_utf8(bytes)
        .map_err(|e| CoreError::Malformed(format!("fichier non UTF-8 : {e}")))?;
    let project = persist::from_json(text.trim_start_matches('\u{feff}'))?;
    Ok(State {
        text,
        revision,
        project,
    })
}

pub(crate) fn revision_of(bytes: &[u8]) -> String {
    format!("sha256:{}", sha256_hex(bytes))
}

pub(crate) fn definitions() -> Value {
    let command = json!({
        "type": "object",
        "description": "Command JSON de pf-core, ex. {\"type\":\"rename_project\",\"name\":\"Veste\"}",
        "required": ["type"]
    });
    let tool = |name: &str, description: &str, props: Value, required: Value, read_only: bool| {
        json!({
            "name": name,
            "description": description,
            "inputSchema": {
                "type": "object",
                "properties": props,
                "required": required,
                "additionalProperties": false
            },
            "annotations": {"readOnlyHint": read_only, "destructiveHint": !read_only}
        })
    };
    let sheet =
        json!({"type": "integer", "minimum": 0, "description": "Identifiant de la feuille"});
    let revision = json!({"type": "string", "description": "Révision lue via list_sheets ou validate_project"});
    json!([
        tool(
            "preview_command",
            "Applique la commande sur une copie en mémoire et renvoie résultat, notes et validation. N'écrit rien sur disque.",
            json!({"command": command}),
            json!(["command"]),
            true
        ),
        tool(
            "apply_command",
            "Applique la commande et enregistre le projet (écriture atomique, journalisée). Refusé si expected_revision ne correspond plus au fichier.",
            json!({
                "command": command,
                "expected_revision": {"type": "string", "description": "Révision lue via list_sheets ou validate_project"}
            }),
            json!(["command", "expected_revision"]),
            false
        ),
        tool(
            "undo_last",
            "Annule la dernière mutation faite par ce serveur (restaure le fichier d'avant). Refusé si le fichier a changé depuis.",
            json!({}),
            json!([]),
            false
        ),
        tool(
            "extract_piece",
            "Extrait une pièce (Command extract_piece) : source = point dans une région fermée (x_mm, y_mm) ou liste de tracés (entity_ids). Même contrôle de révision et même journal que apply_command.",
            json!({
                "sheet_id": sheet,
                "x_mm": {"type": "number", "description": "Point dans la région (avec y_mm)"},
                "y_mm": {"type": "number"},
                "entity_ids": {"type": "array", "items": {"type": "integer", "minimum": 0}, "description": "Chaîne fermée de tracés (à la place de x_mm / y_mm)"},
                "name": {"type": "string"},
                "detached": {"type": "boolean", "description": "Pièce détachée de la construction (défaut false)"},
                "expected_revision": revision
            }),
            json!(["sheet_id", "name", "expected_revision"]),
            false
        ),
        tool(
            "apply_seam_allowance",
            "Pose une marge de couture uniforme (Command set_seam_allowance, default_mm de 0 à 1000) sur une pièce. Même contrôle de révision et même journal que apply_command.",
            json!({
                "piece_id": {"type": "integer", "minimum": 0},
                "default_mm": {"type": "number", "minimum": 0, "maximum": 1000},
                "expected_revision": revision
            }),
            json!(["piece_id", "default_mm", "expected_revision"]),
            false
        ),
        tool(
            "export",
            "Exporte le projet (pdf, svg, dxf, hpgl, png, jpeg) par les fonctions de la ligne de commande, vers un chemin relatif au dossier du projet (jamais en dehors, jamais un fichier .json). N'écrase rien sauf overwrite=true.",
            json!({
                "format": {"type": "string", "enum": ["pdf", "svg", "dxf", "hpgl", "png", "jpeg"]},
                "path": {"type": "string", "description": "Fichier ou dossier de sortie, relatif au dossier du projet"},
                "size": {"type": "string", "description": "Nom d'une taille (défaut : taille de base)"},
                "all_sizes": {"type": "boolean"},
                "dpi": {"type": "integer", "description": "png / jpeg seulement (36 à 1200)"},
                "lang": {"type": "string", "enum": ["fr", "en"]},
                "overwrite": {"type": "boolean"}
            }),
            json!(["format", "path"]),
            false
        ),
    ])
}

fn parse_command(args: &Map<String, Value>) -> Result<(Command, Value), ToolError> {
    let raw = args
        .get("command")
        .ok_or_else(|| ToolError("argument « command » requis".into()))?;
    let value = match raw {
        Value::String(s) => serde_json::from_str::<Value>(s)
            .map_err(|e| ToolError(format!("commande : JSON invalide ({e})")))?,
        other => other.clone(),
    };
    if !value.is_object() {
        return Err(ToolError("commande : objet JSON attendu".into()));
    }
    let command = serde_json::from_value::<Command>(value.clone())
        .map_err(|e| ToolError(format!("commande invalide ou inconnue : {e}")))?;
    Ok((command, value))
}

pub(crate) fn call(path: &Path, name: &str, args: &Map<String, Value>) -> Result<Value, ToolError> {
    match name {
        "preview_command" => preview(path, args),
        "apply_command" => apply(path, args),
        "undo_last" => undo(path),
        "extract_piece" => apply(path, &extract_piece_args(args)?),
        "apply_seam_allowance" => apply(path, &seam_allowance_args(args)?),
        "export" => export(path, args),
        other => Err(ToolError(format!("outil inconnu : {other}"))),
    }
}

fn uint_arg(args: &Map<String, Value>, key: &str) -> Result<u64, ToolError> {
    args.get(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| ToolError(format!("argument « {key} » : entier positif requis")))
}

fn finite_arg(args: &Map<String, Value>, key: &str) -> Result<f64, ToolError> {
    args.get(key)
        .and_then(Value::as_f64)
        .filter(|v| v.is_finite())
        .ok_or_else(|| ToolError(format!("argument « {key} » : nombre fini requis")))
}

fn with_revision(args: &Map<String, Value>, command: Value) -> Map<String, Value> {
    let mut out = Map::new();
    out.insert("command".into(), command);
    if let Some(r) = args.get("expected_revision") {
        out.insert("expected_revision".into(), r.clone());
    }
    out
}

/// `extract_piece` compose la commande `extract_piece` : aucun algorithme ici.
fn extract_piece_args(args: &Map<String, Value>) -> Result<Map<String, Value>, ToolError> {
    let sheet = uint_arg(args, "sheet_id")?;
    let name = args
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError("argument « name » (chaîne) requis".into()))?;
    let source = match (args.get("entity_ids"), args.get("x_mm"), args.get("y_mm")) {
        (Some(ids), None, None) => {
            let list = ids
                .as_array()
                .filter(|a| !a.is_empty() && a.iter().all(|v| v.as_u64().is_some()))
                .ok_or_else(|| {
                    ToolError("« entity_ids » : liste non vide d'entiers positifs requise".into())
                })?;
            json!({"type": "paths", "entities": list})
        }
        (None, Some(_), Some(_)) => {
            json!({"type": "click", "at": {"x": finite_arg(args, "x_mm")?, "y": finite_arg(args, "y_mm")?}})
        }
        _ => {
            return Err(ToolError(
                "source : fournir soit « entity_ids », soit « x_mm » et « y_mm »".into(),
            ))
        }
    };
    let detached = match args.get("detached") {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(_) => return Err(ToolError("argument « detached » : booléen requis".into())),
    };
    Ok(with_revision(
        args,
        json!({"type": "extract_piece", "sheet": sheet, "source": source, "name": name, "detached": detached}),
    ))
}

fn seam_allowance_args(args: &Map<String, Value>) -> Result<Map<String, Value>, ToolError> {
    let piece = uint_arg(args, "piece_id")?;
    let default_mm = finite_arg(args, "default_mm")?;
    Ok(with_revision(
        args,
        json!({"type": "set_seam_allowance", "piece": piece, "seam": {"default_mm": default_mm}}),
    ))
}

fn refused_path(why: &str) -> ToolError {
    ToolError(format!("chemin d'export refusé : {why}"))
}

/// Dossiers du projet qu'un export n'écrit jamais : images de fond et versions.
fn protected_name(name: &str) -> bool {
    let name = name.to_lowercase();
    name.ends_with(".assets") || name.ends_with(".versions")
}

fn forbidden_extension(path: &Path) -> bool {
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase());
    matches!(ext.as_deref(), Some("json" | "jsonl" | "lock"))
}

/// Contrôle le chemin **réel** : liens, jonctions et noms courts de la partie existante sont
/// résolus ; elle doit rester dans le dossier du projet, hors des dossiers protégés.
fn check_resolved(dir: &Path, relative: &Path) -> Result<(), ToolError> {
    let io = |e: std::io::Error| refused_path(&e.to_string());
    let base = fs::canonicalize(dir).map_err(io)?;
    let mut real = base.clone();
    let mut exists = true;
    for c in relative.components() {
        let Component::Normal(part) = c else { continue };
        real.push(part);
        if !exists {
            continue;
        }
        match fs::symlink_metadata(&real) {
            Ok(_) => {
                real = fs::canonicalize(&real)
                    .map_err(|_| refused_path("lien ou jonction sans destination"))?;
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => exists = false,
            Err(e) => return Err(io(e)),
        }
    }
    let inside = real
        .strip_prefix(&base)
        .map_err(|_| refused_path("un lien ou une jonction mène en dehors du dossier du projet"))?;
    if inside
        .components()
        .any(|c| protected_name(&c.as_os_str().to_string_lossy()))
    {
        return Err(refused_path(
            "les dossiers .assets et .versions du projet sont réservés",
        ));
    }
    if forbidden_extension(inside) {
        return Err(refused_path(
            "un fichier .json, .jsonl ou .lock ne peut pas être une sortie",
        ));
    }
    Ok(())
}

/// Chemin de sortie : relatif au dossier du projet, sans `..`, jamais un fichier de projet, ni
/// flux NTFS, ni sortie du dossier par un lien, ni écriture dans `.assets` ou `.versions`.
fn export_target(project: &Path, raw: &str) -> Result<PathBuf, ToolError> {
    let bad = refused_path;
    if raw.trim().is_empty() || raw.contains('\0') {
        return Err(bad("chemin vide ou invalide"));
    }
    let dir = project
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    let given = Path::new(raw);
    let relative = if given.is_absolute() {
        let base = fs::canonicalize(&dir).map_err(|e| bad(&e.to_string()))?;
        given
            .strip_prefix(&base)
            .or_else(|_| given.strip_prefix(&dir))
            .map_err(|_| bad("en dehors du dossier du projet"))?
            .to_path_buf()
    } else {
        given.to_path_buf()
    };
    for c in relative.components() {
        match c {
            Component::Normal(part) => {
                let name = part.to_string_lossy();
                if name.contains(':') {
                    return Err(bad("« : » interdit (flux NTFS)"));
                }
                // Windows retire ces fins de nom : « p.json. » désignerait « p.json ».
                if name.ends_with('.') || name.ends_with(' ') {
                    return Err(bad("un nom ne peut pas finir par un point ou une espace"));
                }
                if protected_name(&name) {
                    return Err(bad("les dossiers .assets et .versions sont réservés"));
                }
                let stem = name
                    .split('.')
                    .next()
                    .unwrap_or_default()
                    .trim_end()
                    .to_uppercase();
                let device = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
                    || ((stem.starts_with("COM") || stem.starts_with("LPT"))
                        && stem.len() == 4
                        && stem.ends_with(|d: char| d.is_ascii_digit()));
                if device {
                    return Err(bad("nom réservé du système (CON, NUL, COM1…)"));
                }
            }
            Component::CurDir => {}
            _ => return Err(bad("« .. », racine ou lecteur interdits")),
        }
    }
    if relative.as_os_str().is_empty() || relative.components().all(|c| c == Component::CurDir) {
        return Err(bad(
            "le dossier du projet lui-même n'est pas un fichier de sortie",
        ));
    }
    if forbidden_extension(&relative) {
        return Err(bad(
            "un fichier .json, .jsonl ou .lock ne peut pas être une sortie",
        ));
    }
    check_resolved(&dir, &relative)?;
    Ok(dir.join(relative))
}

fn export(path: &Path, args: &Map<String, Value>) -> Result<Value, ToolError> {
    let str_arg = |key: &str| -> Result<Option<String>, ToolError> {
        match args.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(s)) => Ok(Some(s.clone())),
            Some(_) => Err(ToolError(format!("argument « {key} » : chaîne requise"))),
        }
    };
    let bool_arg = |key: &str| -> Result<bool, ToolError> {
        match args.get(key) {
            None | Some(Value::Null) => Ok(false),
            Some(Value::Bool(b)) => Ok(*b),
            Some(_) => Err(ToolError(format!("argument « {key} » : booléen requis"))),
        }
    };
    let format =
        str_arg("format")?.ok_or_else(|| ToolError("argument « format » requis".into()))?;
    if !["pdf", "svg", "dxf", "hpgl", "png", "jpeg"].contains(&format.as_str()) {
        return Err(ToolError(format!(
            "format inconnu : {format} (pdf, svg, dxf, hpgl, png, jpeg)"
        )));
    }
    let raw = str_arg("path")?.ok_or_else(|| ToolError("argument « path » requis".into()))?;
    let target = export_target(path, &raw)?;
    let mut cli: Vec<String> = vec![
        "exporter".into(),
        path.to_string_lossy().into_owned(),
        "--format".into(),
        format,
        "--sortie".into(),
        target.to_string_lossy().into_owned(),
    ];
    let size = str_arg("size")?;
    if let Some(s) = size {
        cli.extend(["--taille".into(), s]);
    }
    if bool_arg("all_sizes")? {
        cli.push("--tailles-toutes".into());
    }
    match args.get("dpi") {
        None | Some(Value::Null) => {}
        Some(v) => {
            let d = v
                .as_u64()
                .ok_or_else(|| ToolError("argument « dpi » : entier positif requis".into()))?;
            cli.extend(["--dpi".into(), d.to_string()]);
        }
    }
    if let Some(l) = str_arg("lang")? {
        cli.extend(["--langue".into(), l]);
    }
    if bool_arg("overwrite")? {
        cli.push("--ecraser".into());
    }
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = pf_cli::run(&cli, &mut out, &mut err);
    let text = |b: &[u8]| String::from_utf8_lossy(b).trim().to_string();
    if code != 0 {
        return Err(ToolError(format!(
            "export refusé (code {code}) : {}",
            text(&err)
        )));
    }
    Ok(json!({
        "exported": true,
        "exit_code": code,
        "output": text(&out).lines().collect::<Vec<_>>()
    }))
}

fn preview(path: &Path, args: &Map<String, Value>) -> Result<Value, ToolError> {
    let (command, _) = parse_command(args)?;
    let state = read_state(path)?;
    let mut copy = state.project;
    let outcome = copy.apply(command)?;
    let after = persist::to_json(&copy)?;
    Ok(json!({
        "applied": false,
        "revision": state.revision,
        "revision_after": revision_of(after.as_bytes()),
        "outcome": serde_json::to_value(&outcome).unwrap_or(Value::Null),
        "notes": outcome.notes,
        "validation": {"valid": true}
    }))
}

fn journal_path(path: &Path) -> Result<PathBuf, ToolError> {
    let mut name = path
        .file_name()
        .ok_or_else(|| ToolError("chemin de projet invalide".into()))?
        .to_os_string();
    name.push(".mcp-journal.jsonl");
    Ok(path.with_file_name(name))
}

fn journal_io(p: &Path, e: std::io::Error) -> ToolError {
    ToolError(format!("journal {} : {e}", p.display()))
}

fn read_journal(jp: &Path) -> Result<Vec<Value>, ToolError> {
    let text = match fs::read_to_string(jp) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(journal_io(jp, e)),
    };
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            serde_json::from_str(l)
                .map_err(|e| ToolError(format!("journal {} corrompu : {e}", jp.display())))
        })
        .collect()
}

/// Ajoute une ligne ; renvoie la taille du journal avant l'ajout. Un journal trop gros est
/// d'abord tourné (`.1` à `.4`, le plus ancien disparaît) ; la dernière entrée est toujours dans
/// le journal courant, et `undo_last` lit toute la fenêtre.
fn append_journal(jp: &Path, entry: &Value) -> Result<u64, ToolError> {
    let mut line = serde_json::to_string(entry).unwrap_or_default();
    line.push('\n');
    rotate_journal(jp, line.len() as u64)?;
    let mut f = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(jp)
        .map_err(|e| journal_io(jp, e))?;
    let before = f.metadata().map_err(|e| journal_io(jp, e))?.len();
    let res = f
        .write_all(line.as_bytes())
        .and_then(|()| f.sync_all())
        .map_err(|e| journal_io(jp, e));
    if res.is_err() {
        let _ = f.set_len(before);
    }
    res.map(|()| before)
}

fn journal_limit() -> u64 {
    std::env::var("PF_MCP_JOURNAL_MAX_BYTES")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(JOURNAL_MAX_BYTES)
}

fn rotated_path(jp: &Path, n: u32) -> PathBuf {
    let mut name = jp.as_os_str().to_owned();
    name.push(format!(".{n}"));
    PathBuf::from(name)
}

fn rotate_journal(jp: &Path, incoming_len: u64) -> Result<(), ToolError> {
    let current = fs::metadata(jp).map_or(0, |m| m.len());
    if current == 0 || current + incoming_len <= journal_limit() {
        return Ok(());
    }
    let _ = fs::remove_file(rotated_path(jp, JOURNAL_ROTATED));
    for n in (1..JOURNAL_ROTATED).rev() {
        let from = rotated_path(jp, n);
        if from.exists() {
            fs::rename(&from, rotated_path(jp, n + 1)).map_err(|e| journal_io(&from, e))?;
        }
    }
    fs::rename(jp, rotated_path(jp, 1)).map_err(|e| journal_io(jp, e))
}

/// Entrées de toute la fenêtre de journaux, de la plus ancienne à la plus récente : l'annulation
/// et la numérotation traversent les rotations.
fn read_journal_all(jp: &Path) -> Result<Vec<Value>, ToolError> {
    let mut all = Vec::new();
    for n in (1..=JOURNAL_ROTATED).rev() {
        all.extend(read_journal(&rotated_path(jp, n))?);
    }
    all.extend(read_journal(jp)?);
    Ok(all)
}

fn next_seq(entries: &[Value]) -> u64 {
    entries
        .iter()
        .filter_map(|e| e.get("seq").and_then(Value::as_u64))
        .max()
        .unwrap_or(0)
        + 1
}

/// Verrou consultatif d'écriture `<projet>.mcp-write.lock`, retiré à la destruction : deux
/// écritures MCP simultanées ne s'entrelacent pas. Il ne bloque pas l'application, qui a son
/// propre verrou et son propre contrôle de révision à l'enregistrement.
struct WriteLock(PathBuf);

impl WriteLock {
    fn acquire(project: &Path) -> Result<WriteLock, ToolError> {
        let mut name = project
            .file_name()
            .ok_or_else(|| ToolError("chemin de projet invalide".into()))?
            .to_os_string();
        name.push(".mcp-write.lock");
        let lock = project.with_file_name(name);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        for _ in 0..2 {
            match fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&lock)
            {
                Ok(mut f) => {
                    let _ = write!(f, "{} {now}", std::process::id());
                    return Ok(WriteLock(lock));
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    let held = fs::read_to_string(&lock).ok().and_then(|t| {
                        let mut p = t.split_whitespace();
                        let pid = p.next()?.parse::<u32>().ok()?;
                        let since = p.next()?.parse::<u64>().ok()?;
                        (now.saturating_sub(since) < WRITE_LOCK_STALE_SECS).then_some(pid)
                    });
                    if let Some(pid) = held {
                        return Err(ToolError(format!(
                            "verrou d'écriture tenu par le processus {pid} : une autre écriture est en cours ; rien n'a été écrit"
                        )));
                    }
                    let _ = fs::remove_file(&lock);
                }
                Err(e) => return Err(ToolError(format!("verrou d'écriture : {e}"))),
            }
        }
        Err(ToolError(
            "verrou d'écriture repris par une autre instance ; rien n'a été écrit".into(),
        ))
    }
}

impl Drop for WriteLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

/// Refuse si le fichier n'a plus la révision lue (modification concurrente juste avant l'écriture).
fn ensure_unchanged(path: &Path, expected: &str) -> Result<(), ToolError> {
    let bytes = fs::read(path).map_err(|e| ToolError(format!("{} : {e}", path.display())))?;
    let now = revision_of(&bytes);
    if now == expected {
        Ok(())
    } else {
        Err(ToolError(format!(
            "modification concurrente détectée : révision lue {expected}, actuelle {now} ; rien n'a été écrit"
        )))
    }
}

fn truncate_journal(jp: &Path, len: u64) {
    if let Ok(f) = fs::OpenOptions::new().write(true).open(jp) {
        let _ = f.set_len(len);
    }
}

fn apply(path: &Path, args: &Map<String, Value>) -> Result<Value, ToolError> {
    let (command, command_json) = parse_command(args)?;
    let expected = args
        .get("expected_revision")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            ToolError(
                "argument « expected_revision » (chaîne) obligatoire pour apply_command".into(),
            )
        })?;
    let _lock = WriteLock::acquire(path)?;
    let state = read_state(path)?;
    if state.revision != expected {
        return Err(ToolError(format!(
            "révision périmée : attendue {expected}, actuelle {}. Relire le projet (list_sheets) puis recommencer ; rien n'a été écrit.",
            state.revision
        )));
    }
    let mut project = state.project;
    let outcome = project.apply(command)?;
    let after_json = persist::to_json(&project)?;
    let revision_after = revision_of(after_json.as_bytes());
    let jp = journal_path(path)?;
    let seq = next_seq(&read_journal_all(&jp)?);
    let entry = json!({
        "seq": seq,
        "kind": "mutation",
        "revision_before": state.revision,
        "revision_after": revision_after,
        "command": command_json,
        "notes": outcome.notes,
        "snapshot_before": state.text
    });
    ensure_unchanged(path, &state.revision)?;
    let journal_len = append_journal(&jp, &entry)?;
    if let Err(e) = persist::save(path, &project) {
        truncate_journal(&jp, journal_len);
        return Err(e.into());
    }
    let written = fs::read(path).map_err(|e| ToolError(format!("relecture : {e}")))?;
    if revision_of(&written) != revision_after {
        return Err(ToolError(
            "le fichier enregistré ne correspond pas à la révision prévue (modification concurrente ?) ; vérifier le fichier avant tout undo_last".into(),
        ));
    }
    Ok(json!({
        "applied": true,
        "revision_before": state.revision,
        "revision": revision_after,
        "outcome": serde_json::to_value(&outcome).unwrap_or(Value::Null),
        "notes": outcome.notes,
        "journal": jp.file_name().map(|n| n.to_string_lossy().into_owned())
    }))
}

/// Mutation la plus récente non annulée.
fn last_active_mutation(entries: &[Value]) -> Option<&Value> {
    let mut stack: Vec<&Value> = Vec::new();
    for e in entries {
        match e.get("kind").and_then(Value::as_str) {
            Some("mutation") => stack.push(e),
            Some("undo") => {
                stack.pop();
            }
            _ => {}
        }
    }
    stack.pop()
}

fn undo(path: &Path) -> Result<Value, ToolError> {
    let _lock = WriteLock::acquire(path)?;
    let jp = journal_path(path)?;
    let entries = read_journal_all(&jp)?;
    let Some(m) = last_active_mutation(&entries) else {
        return Err(ToolError(
            "rien à annuler : aucune mutation journalisée par ce serveur".into(),
        ));
    };
    let field = |k: &str| m.get(k).and_then(Value::as_str).map(str::to_owned);
    let (Some(rev_before), Some(rev_after), Some(snapshot)) = (
        field("revision_before"),
        field("revision_after"),
        field("snapshot_before"),
    ) else {
        return Err(ToolError("entrée de journal incomplète".into()));
    };
    if revision_of(snapshot.as_bytes()) != rev_before {
        return Err(ToolError(
            "instantané du journal altéré (révision incohérente) ; annulation refusée".into(),
        ));
    }
    let current = fs::read(path).map_err(|e| ToolError(format!("{} : {e}", path.display())))?;
    let current_rev = revision_of(&current);
    if current_rev != rev_after {
        return Err(ToolError(format!(
            "le fichier a changé depuis la dernière mutation (révision {current_rev}, attendue {rev_after}) ; annulation refusée pour ne rien perdre"
        )));
    }
    // L'instantané doit rester un projet valide avant de remplacer le fichier.
    persist::from_json(snapshot.trim_start_matches('\u{feff}'))?;
    write_atomic(path, snapshot.as_bytes())?;
    let undoes = m.get("seq").cloned().unwrap_or(Value::Null);
    let entry = json!({
        "seq": next_seq(&entries),
        "kind": "undo",
        "undoes_seq": undoes,
        "revision_before": rev_after,
        "revision_after": rev_before,
        "notes": []
    });
    append_journal(&jp, &entry)?;
    Ok(json!({"undone": true, "undoes_seq": undoes, "revision": rev_before}))
}

/// Écriture octet par octet : fichier temporaire voisin, sync, renommage.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), ToolError> {
    let mut tmp_name = path
        .file_name()
        .ok_or_else(|| ToolError("chemin invalide".into()))?
        .to_os_string();
    tmp_name.push(format!(".undo{}", std::process::id()));
    let tmp = path.with_file_name(tmp_name);
    let run = || -> std::io::Result<()> {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        fs::rename(&tmp, path)
    };
    run().map_err(|e| {
        let _ = fs::remove_file(&tmp);
        ToolError(format!("{} : {e}", path.display()))
    })
}

// --- SHA-256 (FIPS 180-4), pour ne pas ajouter de dépendance ---------------

const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

pub(crate) fn sha256_hex(data: &[u8]) -> String {
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&((data.len() as u64) * 8).to_be_bytes());
    for block in msg.chunks_exact(64) {
        let mut w = [0u32; 64];
        for (i, c) in block.chunks_exact(4).enumerate() {
            w[i] = u32::from_be_bytes([c[0], c[1], c[2], c[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let mut v = h;
        for i in 0..64 {
            let s1 = v[4].rotate_right(6) ^ v[4].rotate_right(11) ^ v[4].rotate_right(25);
            let ch = (v[4] & v[5]) ^ (!v[4] & v[6]);
            let t1 = v[7]
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = v[0].rotate_right(2) ^ v[0].rotate_right(13) ^ v[0].rotate_right(22);
            let maj = (v[0] & v[1]) ^ (v[0] & v[2]) ^ (v[1] & v[2]);
            let t2 = s0.wrapping_add(maj);
            v = [
                t1.wrapping_add(t2),
                v[0],
                v[1],
                v[2],
                v[3].wrapping_add(t1),
                v[4],
                v[5],
                v[6],
            ];
        }
        for (a, b) in h.iter_mut().zip(v) {
            *a = a.wrapping_add(b);
        }
    }
    h.iter().map(|x| format!("{x:08x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::{ensure_unchanged, revision_of, sha256_hex};

    #[test]
    fn concurrent_modification_is_named_and_refused() {
        let dir = std::env::temp_dir().join(format!("pf-mcp-unit-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("p.json");
        std::fs::write(&file, b"un").unwrap();
        let rev = revision_of(b"un");
        assert!(ensure_unchanged(&file, &rev).is_ok());
        std::fs::write(&file, b"deux").unwrap();
        let msg = ensure_unchanged(&file, &rev)
            .err()
            .map(|e| e.0)
            .unwrap_or_default();
        assert!(msg.contains("modification concurrente"), "{msg}");
        assert!(msg.contains(&revision_of(b"deux")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sha256_known_vectors() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            sha256_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
    }
}
