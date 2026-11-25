//! US-17.4 - ouverture d'un gros projet **dans un fil séparé**, avec barre de progression et
//! annulation : l'interface continue de répondre pendant la lecture et l'analyse.
//!
//! Le fil lit le fichier par morceaux (progression réelle, annulation entre deux morceaux), puis
//! confie les octets à `persist::from_bytes` (même analyse et même validation que l'ouverture
//! synchrone). Cette analyse reste **synchrone dans le fil** : on ne peut pas l'interrompre ni la
//! mesurer, la barre devient alors indéterminée ; annuler pendant l'analyse jette le résultat (le
//! fil finit seul, sans effet). Sous `ASYNC_OPEN_BYTES`, l'ouverture reste synchrone (inchangée).
//!
//! Rien n'est modifié tant que le résultat n'est pas arrivé : le document courant reste intact et
//! utilisable, le verrou du fichier est gardé par la tâche et rendu à l'annulation.

use crate::app::{modal_title, App, MODAL_WIDTH};
use eframe::egui;
use pf_core::file_guard::FileLock;
use pf_core::{persist, CoreError, Document, Project};
use pf_i18n::{tr, tr_args};
use pf_ui::components::{button, ButtonKind};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::sync::Arc;
use std::time::Duration;

/// Taille de fichier à partir de laquelle l'ouverture se fait dans un fil (1 Mo).
pub(crate) const ASYNC_OPEN_BYTES: u64 = 1024 * 1024;
const CHUNK: usize = 1024 * 1024;

type Loaded = Result<(Project, String), CoreError>;

pub(crate) struct OpenJob {
    path: PathBuf,
    lock: Option<FileLock>,
    rx: Receiver<Loaded>,
    cancel: Arc<AtomicBool>,
    read: Arc<AtomicU64>,
    total: u64,
    analysing: Arc<AtomicBool>,
}

impl OpenJob {
    pub(crate) fn is_analysing(&self) -> bool {
        self.analysing.load(Ordering::Relaxed)
    }
}

fn load_in_thread(
    path: &Path,
    cancel: &AtomicBool,
    read: &AtomicU64,
    analysing: &AtomicBool,
    hold: Option<&AtomicBool>,
) -> Option<Loaded> {
    // Point d'arrêt des tests : le fil attend avant de commencer.
    while hold.is_some_and(|h| h.load(Ordering::Relaxed)) {
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
        std::thread::yield_now();
    }
    let origin = path.display().to_string();
    let io = |e: std::io::Error| CoreError::Io(format!("{origin} : {e}"));
    let mut file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) => return Some(Err(io(e))),
    };
    let total = match file.metadata() {
        Ok(m) => m.len(),
        Err(e) => return Some(Err(io(e))),
    };
    if total > persist::MAX_FILE_BYTES {
        return Some(Err(CoreError::Malformed(
            "fichier trop volumineux pour un projet".into(),
        )));
    }
    let mut bytes = Vec::with_capacity(total as usize);
    let mut buf = vec![0u8; CHUNK];
    loop {
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
        match file.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                bytes.extend_from_slice(&buf[..n]);
                read.store(bytes.len() as u64, Ordering::Relaxed);
            }
            Err(e) => return Some(Err(io(e))),
        }
    }
    analysing.store(true, Ordering::Relaxed);
    if cancel.load(Ordering::Relaxed) {
        return None;
    }
    Some(persist::from_bytes(bytes, &origin))
}

impl App {
    /// Lance l'ouverture de `path` dans un fil. `hold` (tests) retient le fil tant qu'il vaut vrai.
    pub(crate) fn start_open_job(
        &mut self,
        path: &Path,
        lock: Option<FileLock>,
        hold: Option<Arc<AtomicBool>>,
    ) {
        let total = std::fs::metadata(path).map_or(0, |m| m.len());
        let (tx, rx) = channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let read = Arc::new(AtomicU64::new(0));
        let analysing = Arc::new(AtomicBool::new(false));
        let (c, r, a, p) = (
            cancel.clone(),
            read.clone(),
            analysing.clone(),
            path.to_path_buf(),
        );
        std::thread::spawn(move || {
            if let Some(result) = load_in_thread(&p, &c, &r, &a, hold.as_deref()) {
                let _ = tx.send(result);
            }
        });
        self.open_job = Some(OpenJob {
            path: path.to_path_buf(),
            lock,
            rx,
            cancel,
            read,
            total,
            analysing,
        });
    }

    /// Annule l'ouverture en cours : le document courant est intact, le verrou est rendu.
    pub(crate) fn cancel_open_job(&mut self) {
        if let Some(job) = self.open_job.take() {
            job.cancel.store(true, Ordering::Relaxed);
            self.editor.status.text = tr_args(
                "openjob.cancelled",
                &[("path", &job.path.display().to_string())],
            );
            self.editor.status.error = false;
        }
    }

    /// À chaque image : récupère le résultat du fil s'il est arrivé.
    pub(crate) fn poll_open_job(&mut self, ctx: &egui::Context) {
        let Some(job) = &self.open_job else { return };
        let result = match job.rx.try_recv() {
            Ok(r) => Some(r),
            Err(TryRecvError::Empty) => {
                ctx.request_repaint_after(Duration::from_millis(50));
                None
            }
            Err(TryRecvError::Disconnected) => Some(Err(CoreError::Io(String::new()))),
        };
        let Some(result) = result else { return };
        let Some(job) = self.open_job.take() else {
            return;
        };
        match result {
            Ok((project, revision)) => {
                let doc = Document::opened(&job.path, project, revision);
                self.finish_open(&job.path, doc, job.lock);
            }
            Err(e) => self.open_failed(&job.path, &e),
        }
    }

    pub(crate) fn open_job_window(&mut self, ctx: &egui::Context) {
        let Some(job) = &self.open_job else { return };
        let (read, total, analysing) = (
            job.read.load(Ordering::Relaxed),
            job.total,
            job.is_analysing(),
        );
        let mut cancel = false;
        egui::Modal::new(egui::Id::new("open_job")).show(ctx, |ui| {
            ui.set_width(MODAL_WIDTH);
            modal_title(ui, &tr("openjob.title"));
            crate::a11y::label(
                ui,
                job.path
                    .file_name()
                    .map_or_else(String::new, |n| n.to_string_lossy().into_owned()),
            );
            let mb = |b: u64| format!("{:.1}", b as f64 / (1024.0 * 1024.0));
            if analysing {
                ui.add(
                    egui::ProgressBar::new(1.0)
                        .animate(true)
                        .text(tr("openjob.analysing")),
                );
            } else {
                let fraction = if total == 0 {
                    0.0
                } else {
                    (read as f64 / total as f64) as f32
                };
                ui.add(egui::ProgressBar::new(fraction).text(tr_args(
                    "openjob.reading",
                    &[("done", &mb(read)), ("total", &mb(total))],
                )));
            }
            ui.add_space(8.0);
            cancel = button(ui, &tr("openjob.cancel"), ButtonKind::Secondary).clicked();
        });
        if cancel || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.cancel_open_job();
        }
    }
}
