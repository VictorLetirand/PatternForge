//! US-14.2 — décalque assisté : outil de tracé de points sur l'image de fond, accrochage
//! **optionnel** aux contours de l'image, lissage de la suite de points en Béziers.
//!
//! Règles (décisions de la tranche 139) :
//!
//! - les points cliqués sont des **positions en mm** d'une liste d'interface (`TraceState.points`) ;
//!   rien n'est écrit dans le projet avant « Appliquer » ; le résultat est de la géométrie
//!   **ordinaire** (`Command::ImportGeometry`, une seule étape annulable), jamais liée à l'image ;
//! - l'accrochage aux contours est une aide : la carte de contours (Sobel + seuil réglable) est
//!   calculée **à la demande**, dans un **fil séparé annulable**, relue seulement si l'empreinte de
//!   l'image n'a pas changé ; sans carte ou sans contour dans le rayon, le clic suit l'accrochage
//!   ordinaire de l'éditeur ;
//! - le lissage (`pf_geometry::curve_fit`) montre un aperçu des courbes avant « Appliquer » ; la
//!   tolérance et l'angle de coin sont réglables ; aucune longueur ni forme n'est imposée.

use crate::editor::Editor;
use pf_core::background_image as bi;
use pf_core::{Command, ImportItem, ImportObject, Outcome};
use pf_geometry::curve_fit::{fit_polyline, Fit};
use pf_geometry::edges::{sobel, EdgeMap};
use pf_geometry::PointMm;
use pf_i18n::{tr, tr_args};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};

/// Seuil d'amplitude de contour par défaut (niveaux de gris par pixel, 1 à 255).
pub const DEFAULT_THRESHOLD: u8 = 64;
/// Tolérance de lissage par défaut (mm).
pub const DEFAULT_TOLERANCE_MM: f64 = 0.5;
/// Virage au-delà duquel un sommet est un coin (degrés) par défaut.
pub const DEFAULT_CORNER_DEG: f64 = 45.0;
/// Nombre maximal de points d'un décalque (borne du lissage).
pub const MAX_TRACE_POINTS: usize = 5_000;
/// Rayon de recherche d'un contour autour du clic : ce multiple du rayon de saisie des objets.
pub const SNAP_RADIUS_FACTOR: f64 = 2.0;

type EdgeResult = Result<Option<EdgeMap>, String>;

/// Calcul de contours en cours (fil séparé).
pub struct EdgeJob {
    cancel: Arc<AtomicBool>,
    rx: mpsc::Receiver<EdgeResult>,
    key: String,
}

/// Carte de contours calculée pour l'image d'empreinte `key`.
pub struct EdgeCache {
    pub key: String,
    pub map: Arc<EdgeMap>,
}

/// État d'interface du décalque (jamais enregistré).
pub struct TraceState {
    pub points: Vec<PointMm>,
    /// Accrochage aux contours de l'image (désactivé par défaut).
    pub snap_edges: bool,
    pub threshold: u8,
    pub tolerance_text: String,
    pub corner_text: String,
    pub edges: Option<EdgeCache>,
    pub job: Option<EdgeJob>,
    pub error: Option<String>,
}

impl Default for TraceState {
    fn default() -> Self {
        TraceState {
            points: Vec::new(),
            snap_edges: false,
            threshold: DEFAULT_THRESHOLD,
            tolerance_text: format!("{DEFAULT_TOLERANCE_MM} mm"),
            corner_text: format!("{DEFAULT_CORNER_DEG}"),
            edges: None,
            job: None,
            error: None,
        }
    }
}

impl TraceState {
    /// Calcul en cours ?
    pub fn computing(&self) -> bool {
        self.job.is_some()
    }

    /// Demande l'arrêt du calcul en cours ; le résultat est jeté.
    pub fn cancel_edges(&mut self) {
        if let Some(job) = self.job.take() {
            job.cancel.store(true, Ordering::Relaxed);
        }
    }

    /// Lance le calcul de contours de l'image `bg` (fichier cherché dans `dir`) dans un fil
    /// séparé. Un calcul déjà en cours est annulé d'abord.
    pub fn start_edges(&mut self, dir: PathBuf, bg: pf_core::Background) {
        self.cancel_edges();
        self.error = None;
        let cancel = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();
        let key = bg.sha256.clone();
        let flag = Arc::clone(&cancel);
        std::thread::spawn(move || {
            let result = bi::load(&dir, &bg)
                .and_then(|img| sobel(&img.gray(), img.width as usize, img.height as usize, &flag));
            // Le destinataire a pu disparaître (annulation) : rien à signaler.
            let _ = tx.send(result);
        });
        self.job = Some(EdgeJob { cancel, rx, key });
    }

    /// Relit le résultat du calcul s'il est prêt ; vrai si l'état a changé (réaffichage).
    pub fn poll_edges(&mut self) -> bool {
        let Some(job) = &self.job else {
            return false;
        };
        match job.rx.try_recv() {
            Ok(result) => {
                let key = job.key.clone();
                self.job = None;
                match result {
                    Ok(Some(map)) => {
                        self.edges = Some(EdgeCache {
                            key,
                            map: Arc::new(map),
                        })
                    }
                    Ok(None) => {}
                    Err(e) => self.error = Some(e),
                }
                true
            }
            Err(mpsc::TryRecvError::Empty) => false,
            Err(mpsc::TryRecvError::Disconnected) => {
                self.job = None;
                self.error = Some(tr("trace.err.thread"));
                true
            }
        }
    }
}

impl Drop for TraceState {
    fn drop(&mut self) {
        self.cancel_edges();
    }
}

fn parse_positive(text: &str) -> Option<f64> {
    pf_core::units::parse_length_display(text.trim())
        .ok()
        .filter(|v| v.is_finite() && *v > 0.0)
}

impl Editor {
    /// Contour de l'image le plus proche du point `world`, en mm du plan (`None` : accrochage
    /// désactivé, pas de fond, pas de carte pour cette image, rien dans le rayon).
    pub fn trace_edge_near(&self, world: PointMm, tol_mm: f64) -> Option<PointMm> {
        let t = &self.trace;
        if !t.snap_edges {
            return None;
        }
        let bg = self.project().sheet(self.sheet).ok()?.background.as_ref()?;
        if !bg.visible {
            return None;
        }
        let cache = t.edges.as_ref().filter(|c| c.key == bg.sha256)?;
        let map = bg.map().ok()?;
        let here = map.to_px(world)?;
        // Rayon de recherche : `SNAP_RADIUS_FACTOR` fois le rayon de saisie, converti en pixels
        // d'image (au moins 1 pixel).
        let there = map.to_px(PointMm::new(world.x + tol_mm * SNAP_RADIUS_FACTOR, world.y))?;
        let radius = pf_geometry::distance_mm(here, there).max(1.0);
        let (ex, ey) =
            cache
                .map
                .nearest_edge(here.x, here.y, radius, f32::from(t.threshold.max(1)))?;
        map.to_mm(PointMm::new(ex, ey))
    }

    /// Clic de l'outil Décalque : ajoute un point (contour de l'image si l'accrochage est actif et
    /// qu'un contour est proche, sinon accrochage ordinaire). Un point confondu avec le dernier est
    /// refusé (message) : il annulerait une tangente du lissage.
    pub fn trace_click(&mut self, world: PointMm, tol_mm: f64) {
        if self.trace.points.len() >= MAX_TRACE_POINTS {
            self.error(tr_args(
                "trace.err.too_many",
                &[("max", &MAX_TRACE_POINTS.to_string())],
            ));
            return;
        }
        let at = self
            .trace_edge_near(world, tol_mm)
            .unwrap_or_else(|| self.snap_at(world, tol_mm).pos);
        if self.trace.points.last().is_some_and(|p| {
            pf_geometry::distance_mm(*p, at) <= pf_core::command::MERGE_TOLERANCE_MM
        }) {
            self.error(tr("trace.err.duplicate"));
            return;
        }
        self.trace.points.push(at);
        self.info(tr_args(
            "trace.status.points",
            &[("n", &self.trace.points.len().to_string())],
        ));
    }

    /// Retire le dernier point posé.
    pub fn trace_remove_last(&mut self) {
        self.trace.points.pop();
    }

    /// Efface tous les points du décalque (rien n'est écrit dans le projet).
    pub fn trace_clear(&mut self) {
        self.trace.points.clear();
    }

    /// Ajustement des points par des Béziers, aux réglages de la fenêtre. Lecture seule : c'est le
    /// même calcul que « Appliquer » (aperçu = résultat).
    pub fn trace_fit(&self) -> Result<Fit, String> {
        let tol = parse_positive(&self.trace.tolerance_text).ok_or_else(|| tr("trace.err.tol"))?;
        let corner: f64 = self
            .trace
            .corner_text
            .trim()
            .replace(',', ".")
            .parse()
            .map_err(|_| tr("trace.err.corner"))?;
        // 180 ou plus = aucun coin (courbe lisse d'un bout à l'autre).
        let corner = (corner < 180.0).then_some(corner);
        fit_polyline(&self.trace.points, tol, corner)
            .map_err(|e| tr_args("trace.err.fit", &[("why", &e.to_string())]))
    }

    /// Applique le décalque : suite de Béziers lissées (`smooth`) ou de segments droits. Une seule
    /// étape annulable ; les points restent affichés tant que l'utilisateur ne les efface pas.
    pub fn trace_apply(&mut self, smooth: bool) -> Option<Outcome> {
        if self.trace.points.len() < 2 {
            self.error(tr("trace.err.few"));
            return None;
        }
        let objects: Vec<ImportObject> = if smooth {
            match self.trace_fit() {
                Ok(fit) => fit
                    .curves
                    .iter()
                    .map(|c| ImportObject {
                        item: ImportItem::Bezier {
                            start: c.p0,
                            c1: c.c1,
                            c2: c.c2,
                            end: c.p3,
                        },
                        layer: None,
                    })
                    .collect(),
                Err(e) => {
                    self.error(e);
                    return None;
                }
            }
        } else {
            self.trace
                .points
                .windows(2)
                .map(|w| ImportObject {
                    item: ImportItem::Segment {
                        from: w[0],
                        to: w[1],
                    },
                    layer: None,
                })
                .collect()
        };
        let out = self.exec(Command::ImportGeometry {
            sheet: self.sheet,
            objects,
        })?;
        self.info(tr_args(
            "trace.status.applied",
            &[("n", &out.entities.len().to_string())],
        ));
        self.trace.points.clear();
        Some(out)
    }
}
