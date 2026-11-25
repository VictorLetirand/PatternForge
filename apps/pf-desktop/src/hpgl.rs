//! Export pour traceur HPGL / PLT (US-12.5) : le tracé est calculé à l'ouverture de la fenêtre
//! (lecture seule, aucune `Command`), le rapport est affiché **avant** l'écriture (table
//! plume → type de trait, taille, longueur à vide) et le fichier n'est écrit qu'après
//! « Enregistrer le HPGL… ». Un refus ou un échec ne laisse aucun fichier. Même sélection de
//! contenu et même mise en page que le PDF et le SVG. Case « Découper en pages » (PH-5e) : un
//! fichier par page (`nom-p1`, `nom-p2`…), mêmes pages que le PDF ; cochée d'office quand la feuille
//! ne tient pas sur une page (au lieu du refus d'avant).

use crate::app::App;
use crate::palette::pal;
use crate::print_setup::mm;
use eframe::egui::{self, RichText};
use pf_export::hpgl::{
    export_hpgl, export_hpgl_pages, HpglError, HpglOptions, HpglPen, HpglPlot, HpglReport,
    HpglTiles, PenUse,
};
use pf_export::{PrintScale, Scene};
use pf_i18n::{tr, tr_args};
use std::path::{Path, PathBuf};

/// Fenêtre « Exporter pour traceur » : le tracé déjà calculé (état d'interface, jamais enregistré).
pub struct HpglDialog {
    /// Fichier unique ; absent quand la feuille ne tient pas sur une page.
    pub plot: Option<HpglPlot>,
    /// HPGL découpé en pages, calculé quand la case est cochée.
    pub tiles: Option<HpglTiles>,
    /// Case « Découper en pages ».
    pub tile: bool,
    /// Refus de l'export en un fichier (feuille trop grande), montré quand la case est décochée.
    pub refusal: Option<String>,
    /// Scène exportée (pour recalculer en cochant la case).
    pub scene: Scene,
    /// Contours de coupe non calculables, donc non tracés (US-05.4) ; vide si tout est tracé.
    pub warnings: String,
}

/// Ce que la fenêtre affiche : le fichier unique, ou le total des pages (plumes additionnées).
struct Shown {
    strokes: usize,
    down: f64,
    travel: f64,
    natural: f64,
    pens: Vec<PenUse>,
    texts: usize,
    points: usize,
    dropped: usize,
    scale: PrintScale,
}

fn shown_plot(r: &HpglReport) -> Shown {
    Shown {
        strokes: r.strokes,
        down: r.pen_down_mm,
        travel: r.travel_mm,
        natural: r.natural_travel_mm,
        pens: r.pens.clone(),
        texts: r.texts_omitted,
        points: r.points_omitted,
        dropped: r.degenerate_dropped,
        scale: r.scale,
    }
}

fn shown_tiles(t: &HpglTiles) -> Shown {
    let first = &t.pages[0].report;
    let mut s = Shown {
        strokes: 0,
        down: 0.0,
        travel: 0.0,
        natural: 0.0,
        pens: Vec::new(),
        texts: first.texts_omitted,
        points: first.points_omitted,
        dropped: 0,
        scale: first.scale,
    };
    for p in &t.pages {
        let r = &p.report;
        s.strokes += r.strokes;
        s.down += r.pen_down_mm;
        s.travel += r.travel_mm;
        s.natural += r.natural_travel_mm;
        s.dropped += r.degenerate_dropped;
    }
    for pen in HpglPen::ALL {
        let used = t
            .pages
            .iter()
            .flat_map(|p| &p.report.pens)
            .filter(|u| u.pen == pen);
        let (strokes, length_mm) = used.fold((0, 0.0), |a, u| (a.0 + u.strokes, a.1 + u.length_mm));
        if strokes > 0 {
            s.pens.push(PenUse {
                pen,
                strokes,
                length_mm,
            });
        }
    }
    s
}

/// Noms des fichiers d'un export en `n` pages : `nom-p1.ext`, `nom-p2.ext`… à côté de `path`
/// (une seule page : `path` lui-même).
pub fn hpgl_page_paths(path: &Path, n: usize) -> Vec<PathBuf> {
    if n <= 1 {
        return vec![path.to_path_buf()];
    }
    let stem = path.file_stem().map_or_else(
        || "patron".to_string(),
        |s| s.to_string_lossy().into_owned(),
    );
    let ext = path
        .extension()
        .map_or_else(|| "plt".to_string(), |e| e.to_string_lossy().into_owned());
    (1..=n)
        .map(|k| path.with_file_name(format!("{stem}-p{k}.{ext}")))
        .collect()
}

/// Nom d'une plume d'après son type de trait.
pub fn pen_label(pen: HpglPen) -> String {
    match pen {
        HpglPen::Cutting => tr("hpgl.pen.cutting"),
        HpglPen::Sewing => tr("hpgl.pen.sewing"),
        HpglPen::Construction => tr("hpgl.pen.construction"),
        HpglPen::Grainline => tr("hpgl.pen.grainline"),
        HpglPen::Notches => tr("hpgl.pen.notches"),
        HpglPen::Marks => tr("hpgl.pen.marks"),
        HpglPen::Dimensions => tr("hpgl.pen.dimensions"),
        HpglPen::Other => tr("hpgl.pen.other"),
        HpglPen::Darts => tr("hpgl.pen.darts"),
    }
}

/// Message d'un refus d'export (aucun fichier écrit).
pub fn hpgl_error(e: &HpglError) -> String {
    match e {
        HpglError::Nothing => tr("error.hpgl.nothing"),
        HpglError::NotFinite => tr("error.hpgl.non_finite"),
        HpglError::OutOfRange { .. } => tr("error.hpgl.range"),
        HpglError::Page(p) => tr_args(
            "error.core",
            &[("detail", &crate::print_setup::page_error(p))],
        ),
        HpglError::Tiling(e) => crate::print_setup::export_error(e),
        HpglError::SheetTooSmall {
            content_w_mm,
            content_h_mm,
            zone_w_mm,
            zone_h_mm,
        } => tr_args(
            "error.hpgl.too_big",
            &[
                ("w", &mm(*content_w_mm)),
                ("h", &mm(*content_h_mm)),
                ("zw", &mm(*zone_w_mm)),
                ("zh", &mm(*zone_h_mm)),
            ],
        ),
    }
}

impl App {
    /// Menu Fichier › « Exporter pour traceur (HPGL)… » : calcule le tracé et ouvre la fenêtre
    /// de rapport ; un refus est affiché comme erreur, sans fenêtre ni fichier. Une feuille trop
    /// grande pour une page ouvre la fenêtre déjà découpée en pages (case cochée).
    pub fn open_hpgl_export(&mut self) -> bool {
        // US-09.8 : `export_scene` lit la taille exportée (la base par défaut).
        let scene = match self.export_scene(self.editor.sheet) {
            Ok(s) => s,
            Err(e) => {
                self.editor.error(crate::print_setup::export_error(&e));
                return false;
            }
        };
        let warnings = crate::app::scene_warnings_note(&scene);
        let setup = self.page_setup;
        match export_hpgl(&scene, &setup) {
            Ok(plot) => {
                self.hpgl_dialog = Some(HpglDialog {
                    plot: Some(plot),
                    tiles: None,
                    tile: false,
                    refusal: None,
                    scene,
                    warnings,
                });
                true
            }
            Err(e @ HpglError::SheetTooSmall { .. }) => {
                match export_hpgl_pages(&scene, &setup, HpglOptions::default()) {
                    Ok(tiles) => {
                        self.hpgl_dialog = Some(HpglDialog {
                            plot: None,
                            tiles: Some(tiles),
                            tile: true,
                            refusal: Some(hpgl_error(&e)),
                            scene,
                            warnings,
                        });
                        true
                    }
                    Err(e) => {
                        self.editor.error(hpgl_error(&e));
                        false
                    }
                }
            }
            Err(e) => {
                self.editor.error(hpgl_error(&e));
                false
            }
        }
    }

    /// Coche ou décoche « Découper en pages » ; un refus du découpage laisse la case décochée.
    fn hpgl_set_tile(&mut self, d: &mut HpglDialog, on: bool) {
        if !on {
            d.tile = false;
            return;
        }
        match export_hpgl_pages(&d.scene, &self.page_setup, HpglOptions::default()) {
            Ok(t) => {
                d.tiles = Some(t);
                d.tile = true;
            }
            Err(e) => {
                d.tile = false;
                self.editor.error(hpgl_error(&e));
            }
        }
    }

    /// Écrit le HPGL calculé dans `path` (case cochée : une page par fichier, voir
    /// [`hpgl_page_paths`]). Renvoie `true` si le ou les fichiers sont écrits ; un échec
    /// d'écriture ouvre la boîte d'erreur.
    pub fn write_hpgl_to(&mut self, path: &Path) -> bool {
        let Some(d) = &self.hpgl_dialog else {
            return false;
        };
        if d.tile {
            return self.write_hpgl_pages_to(path);
        }
        let Some(plot) = &d.plot else {
            return false;
        };
        let report = &plot.report;
        let note = format!(
            "{}{}",
            tr_args(
                "export.note.hpgl",
                &[
                    ("strokes", &report.strokes.to_string()),
                    ("pens", &report.pens.len().to_string()),
                ],
            ),
            d.warnings
        );
        match std::fs::write(path, plot.bytes()) {
            Ok(()) => {
                self.editor.status.text = tr_args(
                    "export.done",
                    &[
                        ("what", "HPGL"),
                        ("note", &note),
                        ("path", &path.display().to_string()),
                    ],
                );
                self.editor.status.error = false;
                true
            }
            Err(e) => {
                self.error_dialog = Some(tr_args("export.failed", &[("detail", &e.to_string())]));
                false
            }
        }
    }

    /// Une page par fichier ; un échec d'écriture supprime les fichiers déjà écrits par cet appel.
    fn write_hpgl_pages_to(&mut self, path: &Path) -> bool {
        let Some(d) = self.hpgl_dialog.as_ref() else {
            return false;
        };
        let Some(tiles) = d.tiles.as_ref() else {
            return false;
        };
        let paths = hpgl_page_paths(path, tiles.pages.len());
        let strokes: usize = tiles.pages.iter().map(|p| p.report.strokes).sum();
        let mut written: Vec<&PathBuf> = Vec::new();
        let mut failure: Option<String> = None;
        for (p, file) in tiles.pages.iter().zip(&paths) {
            if let Err(e) = std::fs::write(file, p.bytes()) {
                failure = Some(e.to_string());
                break;
            }
            written.push(file);
        }
        if let Some(detail) = failure {
            for w in &written {
                let _ = std::fs::remove_file(w);
            }
            self.error_dialog = Some(tr_args("export.failed", &[("detail", &detail)]));
            return false;
        }
        let note = format!(
            "{}{}",
            tr_args(
                "export.note.hpgl.pages",
                &[
                    ("pages", &paths.len().to_string()),
                    ("strokes", &strokes.to_string()),
                ],
            ),
            d.warnings
        );
        let first = paths[0].display().to_string();
        self.editor.status.text = tr_args(
            "export.done",
            &[("what", "HPGL"), ("note", &note), ("path", &first)],
        );
        self.editor.status.error = false;
        true
    }

    fn save_hpgl_dialog(&mut self) -> bool {
        let stem = self.export_stem(&Self::safe_file_stem(&self.editor.sheet().name));
        let Some(path) = rfd::FileDialog::new()
            .add_filter("HPGL", &["plt", "hpgl"])
            .set_file_name(format!("{stem}.plt"))
            .save_file()
        else {
            return false;
        };
        self.write_hpgl_to(&path)
    }

    pub fn hpgl_window(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.hpgl_dialog.take() else {
            return;
        };
        let mut open = true;
        let mut save = false;
        let mut close = false;
        let mut toggle: Option<bool> = None;
        let shown = match (&d.plot, &d.tiles, d.tile) {
            (_, Some(t), true) => Some(shown_tiles(t)),
            (Some(p), _, _) => Some(shown_plot(&p.report)),
            _ => None,
        };
        let mut chosen_size = None;
        egui::Window::new(tr("hpgl.title"))
            .id(egui::Id::new("hpgl_window"))
            .open(&mut open)
            .collapsible(false)
            .default_width(520.0)
            .show(ctx, |ui| {
                // US-09.8 : taille exportée ; changer de taille recalcule le tracé.
                if let Some(size) = self.export_size_section(ui, true) {
                    chosen_size = Some(size);
                }
                crate::panels::note_paragraph(ui, "hpgl.intro", &tr("hpgl.intro"));
                if !d.warnings.is_empty() {
                    crate::a11y::label(ui, RichText::new(d.warnings.trim()).color(pal().error));
                }
                let mut tile = d.tile;
                if ui.checkbox(&mut tile, tr("hpgl.tile.check")).changed() {
                    toggle = Some(tile);
                }
                crate::panels::note_paragraph(ui, "hpgl.tile.hint", &tr("hpgl.tile.hint"));
                match (&d.plot, &d.tiles, d.tile) {
                    (_, Some(t), true) => {
                        let r = &t.pages[0].report;
                        crate::a11y::label(
                            ui,
                            tr_args(
                                "hpgl.tile.summary",
                                &[
                                    ("pages", &t.pages.len().to_string()),
                                    ("w", &mm(r.width_mm)),
                                    ("h", &mm(r.height_mm)),
                                    (
                                        "strokes",
                                        &t.pages
                                            .iter()
                                            .map(|p| p.report.strokes)
                                            .sum::<usize>()
                                            .to_string(),
                                    ),
                                ],
                            ),
                        );
                        if t.plan_ignored {
                            crate::a11y::label(
                                ui,
                                RichText::new(tr("hpgl.tile.plan_ignored")).small(),
                            );
                        }
                    }
                    (Some(p), _, _) => {
                        let r = &p.report;
                        let zone = match r.zone_height_mm {
                            Some(h) => tr_args(
                                "hpgl.zone.sheet",
                                &[("w", &mm(r.zone_width_mm)), ("h", &mm(h))],
                            ),
                            None => tr_args("hpgl.zone.roll", &[("w", &mm(r.zone_width_mm))]),
                        };
                        crate::a11y::label(
                            ui,
                            tr_args(
                                "hpgl.size",
                                &[
                                    ("w", &mm(r.width_mm)),
                                    ("h", &mm(r.height_mm)),
                                    ("zone", &zone),
                                    ("format", &crate::print_setup::setup_label(&self.page_setup)),
                                ],
                            ),
                        );
                    }
                    _ => {
                        crate::a11y::label(
                            ui,
                            RichText::new(tr("hpgl.tile.refused")).color(pal().error),
                        );
                        if let Some(msg) = &d.refusal {
                            crate::a11y::label(ui, RichText::new(msg).small().color(pal().error));
                        }
                    }
                }
                if let Some(s) = &shown {
                    crate::a11y::label(
                        ui,
                        tr_args(
                            "hpgl.summary",
                            &[
                                ("strokes", &s.strokes.to_string()),
                                ("down", &mm(s.down)),
                                ("travel", &mm(s.travel)),
                                ("natural", &mm(s.natural)),
                            ],
                        ),
                    );
                    egui::ScrollArea::vertical()
                        .max_height(260.0)
                        .id_salt("hpgl_report")
                        .show(ui, |ui| {
                            crate::a11y::label(ui, RichText::new(tr("hpgl.pens.title")).strong());
                            for p in &s.pens {
                                crate::a11y::label(
                                    ui,
                                    tr_args(
                                        "hpgl.pen.line",
                                        &[
                                            ("number", &p.pen.number().to_string()),
                                            ("name", &pen_label(p.pen)),
                                            ("count", &p.strokes.to_string()),
                                            ("length", &mm(p.length_mm)),
                                        ],
                                    ),
                                );
                            }
                            if !s.scale.is_identity() {
                                crate::a11y::label(
                                    ui,
                                    tr_args(
                                        "hpgl.scale",
                                        &[
                                            ("x", &s.scale.x.to_string()),
                                            ("y", &s.scale.y.to_string()),
                                        ],
                                    ),
                                );
                            }
                            crate::a11y::label(
                                ui,
                                RichText::new(tr("hpgl.omitted.title")).strong(),
                            );
                            crate::a11y::label(
                                ui,
                                tr_args(
                                    "hpgl.omitted",
                                    &[
                                        ("texts", &s.texts.to_string()),
                                        ("points", &s.points.to_string()),
                                    ],
                                ),
                            );
                            if s.dropped > 0 {
                                crate::a11y::label(
                                    ui,
                                    RichText::new(tr_args(
                                        "hpgl.dropped",
                                        &[("count", &s.dropped.to_string())],
                                    ))
                                    .color(pal().error),
                                );
                            }
                        });
                }
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(shown.is_some(), egui::Button::new(tr("hpgl.save")))
                        .clicked()
                    {
                        save = true;
                    }
                    if ui.button(tr("hpgl.close")).clicked() {
                        close = true;
                    }
                });
            });
        if let Some(on) = toggle {
            self.hpgl_set_tile(&mut d, on);
        }
        self.hpgl_dialog = Some(d);
        if let Some(size) = chosen_size {
            // Un refus (taille non exportable) laisse la fenêtre et le choix d'avant.
            let previous = self.export_size;
            self.export_size = Some(size);
            if !self.open_hpgl_export() {
                self.export_size = previous;
            }
        }
        if save && self.save_hpgl_dialog() {
            close = true;
        }
        if close || !open {
            self.hpgl_dialog = None;
        }
    }
}
