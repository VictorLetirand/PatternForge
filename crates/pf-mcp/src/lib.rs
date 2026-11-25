//! Serveur MCP local de PatternForge, **lecture seule par défaut**.
//!
//! JSON-RPC 2.0 sur stdio, un message JSON par ligne. Le serveur ne touche que
//! le fichier projet passé au lancement (relu à chaque appel). Il n'écrit que
//! si `--allow-write` est donné (module `write`).

mod commands;
mod tools;
mod write;

pub use commands::command_names;

use serde_json::{json, Map, Value};
use std::io::{self, BufRead, Read, Write};
use std::path::PathBuf;

/// Taille maximale d'une ligne d'entrée (1 Mio).
pub const MAX_LINE_BYTES: usize = 1024 * 1024;
/// Version de protocole préférée.
pub const LATEST_PROTOCOL: &str = "2025-06-18";
const SUPPORTED_PROTOCOLS: [&str; 3] = ["2024-11-05", "2025-03-26", "2025-06-18"];

pub(crate) const PARSE_ERROR: i64 = -32700;
pub(crate) const INVALID_REQUEST: i64 = -32600;
pub(crate) const METHOD_NOT_FOUND: i64 = -32601;
pub(crate) const INVALID_PARAMS: i64 = -32602;
pub(crate) const INTERNAL_ERROR: i64 = -32603;
pub(crate) const RESOURCE_NOT_FOUND: i64 = -32002;

/// Erreur de protocole JSON-RPC (distincte des erreurs métier d'outil).
#[derive(Debug)]
pub(crate) struct RpcError {
    pub code: i64,
    pub message: String,
}

impl RpcError {
    pub(crate) fn new(code: i64, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

fn response_ok(id: &Value, result: Value) -> String {
    serialize(&json!({"jsonrpc": "2.0", "id": id, "result": result}))
}

fn response_err(id: &Value, code: i64, message: &str) -> String {
    serialize(&json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {"code": code, "message": message}
    }))
}

fn serialize(v: &Value) -> String {
    serde_json::to_string(v).unwrap_or_else(|_| {
        r#"{"jsonrpc":"2.0","id":null,"error":{"code":-32603,"message":"sérialisation impossible"}}"#
            .to_string()
    })
}

/// Serveur lié à un unique fichier projet.
#[derive(Debug, Clone)]
pub struct Server {
    project_path: PathBuf,
    allow_write: bool,
}

impl Server {
    pub fn new(project_path: PathBuf) -> Self {
        Self {
            project_path,
            allow_write: false,
        }
    }

    /// Serveur avec outils d'écriture (`--allow-write`).
    pub fn with_write(project_path: PathBuf) -> Self {
        Self {
            project_path,
            allow_write: true,
        }
    }

    /// Traite une ligne ; renvoie la réponse sérialisée, ou `None` pour une
    /// ligne vide ou une notification.
    pub fn handle_line(&mut self, line: &str) -> Option<String> {
        if line.len() > MAX_LINE_BYTES {
            return Some(line_too_long_response());
        }
        let line = line.trim();
        if line.is_empty() {
            return None;
        }
        let value: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => {
                return Some(response_err(
                    &Value::Null,
                    PARSE_ERROR,
                    &format!("JSON invalide : {e}"),
                ))
            }
        };
        let Some(obj) = value.as_object() else {
            return Some(response_err(
                &Value::Null,
                INVALID_REQUEST,
                "requête invalide : objet JSON attendu (les lots ne sont pas gérés)",
            ));
        };
        let method = obj.get("method").and_then(Value::as_str);
        let Some(id) = obj.get("id") else {
            // Notification : jamais de réponse.
            return match method {
                Some(_) => None,
                None => Some(response_err(
                    &Value::Null,
                    INVALID_REQUEST,
                    "requête invalide : « method » absent",
                )),
            };
        };
        if !(id.is_null() || id.is_string() || id.is_number()) {
            return Some(response_err(
                &Value::Null,
                INVALID_REQUEST,
                "requête invalide : « id » doit être une chaîne, un nombre ou null",
            ));
        }
        if obj.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
            return Some(response_err(
                id,
                INVALID_REQUEST,
                "requête invalide : « jsonrpc » doit valoir \"2.0\"",
            ));
        }
        let Some(method) = method else {
            return Some(response_err(
                id,
                INVALID_REQUEST,
                "requête invalide : « method » doit être une chaîne",
            ));
        };
        let params = obj.get("params");
        Some(match self.dispatch(method, params) {
            Ok(result) => response_ok(id, result),
            Err(e) => response_err(id, e.code, &e.message),
        })
    }

    fn dispatch(&self, method: &str, params: Option<&Value>) -> Result<Value, RpcError> {
        match method {
            "initialize" => initialize(params),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({"tools": tools::definitions(self.allow_write)})),
            "tools/call" => {
                let p = params_object(params)?;
                let name = p
                    .get("name")
                    .and_then(Value::as_str)
                    .ok_or_else(|| RpcError::new(INVALID_PARAMS, "« name » (chaîne) requis"))?;
                let empty = Map::new();
                let args = match p.get("arguments") {
                    None | Some(Value::Null) => &empty,
                    Some(Value::Object(m)) => m,
                    Some(_) => {
                        return Err(RpcError::new(
                            INVALID_PARAMS,
                            "« arguments » doit être un objet",
                        ))
                    }
                };
                // Filet de sécurité : un panic du cœur devient une erreur typée, jamais un arrêt.
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    tools::call(&self.project_path, self.allow_write, name, args)
                }))
                .unwrap_or_else(|_| {
                    Err(RpcError::new(
                        INTERNAL_ERROR,
                        "erreur interne : l'outil a échoué (rien n'a été renvoyé)",
                    ))
                })
            }
            "resources/list" => Ok(tools::resources_list()),
            "resources/read" => {
                let p = params_object(params)?;
                let uri = p
                    .get("uri")
                    .and_then(Value::as_str)
                    .ok_or_else(|| RpcError::new(INVALID_PARAMS, "« uri » (chaîne) requis"))?;
                tools::resources_read(&self.project_path, uri)
            }
            other => Err(RpcError::new(
                METHOD_NOT_FOUND,
                format!("méthode inconnue : {other}"),
            )),
        }
    }

    /// Boucle de service : une requête par ligne en entrée, une réponse par
    /// ligne en sortie. Retourne à la fin du flux d'entrée.
    pub fn run<R: BufRead, W: Write>(&mut self, mut input: R, mut output: W) -> io::Result<()> {
        loop {
            let mut buf = Vec::new();
            let n = input
                .by_ref()
                .take(MAX_LINE_BYTES as u64 + 1)
                .read_until(b'\n', &mut buf)?;
            if n == 0 {
                return Ok(());
            }
            let reply = if buf.len() > MAX_LINE_BYTES && buf.last() != Some(&b'\n') {
                skip_line(&mut input)?;
                Some(line_too_long_response())
            } else {
                self.handle_line(&String::from_utf8_lossy(&buf))
            };
            if let Some(r) = reply {
                output.write_all(r.as_bytes())?;
                output.write_all(b"\n")?;
                output.flush()?;
            }
        }
    }
}

fn line_too_long_response() -> String {
    response_err(
        &Value::Null,
        INVALID_REQUEST,
        &format!("ligne trop longue (maximum {MAX_LINE_BYTES} octets)"),
    )
}

/// Jette la fin de la ligne courante sans la mémoriser.
fn skip_line<R: BufRead>(input: &mut R) -> io::Result<()> {
    loop {
        let chunk = input.fill_buf()?;
        if chunk.is_empty() {
            return Ok(());
        }
        match chunk.iter().position(|&b| b == b'\n') {
            Some(pos) => {
                input.consume(pos + 1);
                return Ok(());
            }
            None => {
                let len = chunk.len();
                input.consume(len);
            }
        }
    }
}

fn params_object(params: Option<&Value>) -> Result<&Map<String, Value>, RpcError> {
    match params {
        Some(Value::Object(m)) => Ok(m),
        _ => Err(RpcError::new(
            INVALID_PARAMS,
            "« params » doit être un objet",
        )),
    }
}

fn initialize(params: Option<&Value>) -> Result<Value, RpcError> {
    let p = params_object(params)?;
    let requested = match p.get("protocolVersion") {
        None => None,
        Some(Value::String(s)) => Some(s.as_str()),
        Some(_) => {
            return Err(RpcError::new(
                INVALID_PARAMS,
                "« protocolVersion » doit être une chaîne",
            ))
        }
    };
    let version = match requested {
        Some(v) if SUPPORTED_PROTOCOLS.contains(&v) => v,
        _ => LATEST_PROTOCOL,
    };
    Ok(json!({
        "protocolVersion": version,
        "capabilities": {"tools": {}, "resources": {}},
        "serverInfo": {"name": "patterncraft", "version": env!("CARGO_PKG_VERSION")}
    }))
}
