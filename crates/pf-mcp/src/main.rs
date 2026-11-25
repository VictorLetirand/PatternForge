//! Binaire `pf-mcp-server --project <chemin.pforge.json>` : boucle stdio.

use pf_mcp::Server;
use std::io::{self, BufReader};
use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (path, allow_write) = match args.as_slice() {
        [flag, path] if flag == "--project" => (PathBuf::from(path), false),
        [flag, path, w] | [w, flag, path] if flag == "--project" && w == "--allow-write" => {
            (PathBuf::from(path), true)
        }
        _ => {
            eprintln!("usage : pf-mcp-server --project <chemin.pforge.json> [--allow-write]");
            return ExitCode::from(2);
        }
    };
    let mode = if allow_write {
        "ÉCRITURE ACTIVÉE"
    } else {
        "lecture seule"
    };
    eprintln!("pf-mcp-server : {mode}, projet {}", path.display());
    let mut server = if allow_write {
        Server::with_write(path)
    } else {
        Server::new(path)
    };
    let stdin = BufReader::new(io::stdin().lock());
    let stdout = io::stdout().lock();
    match server.run(stdin, stdout) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("pf-mcp-server : erreur d'entrée/sortie : {e}");
            ExitCode::FAILURE
        }
    }
}
