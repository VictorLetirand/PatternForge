//! PatternForge — application de bureau.
//!
//! Usage : `patternforge [projet.pforge.json] [--demo] [--screenshot image.png] [--lang fr|en]`
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod a11y;
mod align;
mod app;
mod assemble;
mod assets_copy;
mod background;
mod bom;
mod buttons;
mod canvas;
mod cutline;
mod cutting;
mod cutting_hide;
mod dart;
mod dependencies;
mod diagnostic;
mod direct_print;
mod direct_print_ui;
mod dock;
mod dxf;
mod dxf_aama;
mod dxf_generic;
mod editor;
mod error_journal;
mod export_size;
mod extra_lang;
mod finished;
mod flare;
mod formulas;
mod full_dimensions;
mod glossary;
mod grading;
mod grain;
mod help;
mod hem;
mod history;
mod hpgl;
#[cfg(test)]
mod i18n_tests;
mod imposed;
mod junctions;
mod kept;
mod labels;
mod library;
mod marking_run;
mod measurements;
mod naming;
mod notch_anchor;
mod notch_shape;
mod open_job;
mod palette;
mod panels;
#[cfg(test)]
mod ph2_tests;
#[cfg(test)]
mod ph2b_tests;
#[cfg(test)]
mod ph2c_tests;
#[cfg(test)]
mod ph6_tests;
mod plan;
mod pleat;
mod prefs;
mod print_setup;
mod projection;
mod projection_ui;
mod quick;
#[cfg(test)]
mod recovery_tests;
mod report;
mod safeguard;
#[cfg(test)]
mod safeguard_tests;
mod seam;
mod seams;
mod search;
mod select_ops;
mod shape_control;
mod shortcuts;
mod shortcuts_ui;
mod size_measures;
mod size_nest;
mod sizes;
mod stages;
mod stitch;
mod text;
mod topstitch;
mod trace;
mod trace_ui;
mod transform;
#[cfg(test)]
mod ui_tests;
mod unfold;
#[cfg(test)]
mod us114_tests;
#[cfg(test)]
mod us115_tests;
#[cfg(test)]
mod us136_adverse_tests;
#[cfg(test)]
mod us136_tests;
#[cfg(test)]
mod us141_tests;
#[cfg(test)]
mod us142_tests;
#[cfg(test)]
mod us151_tests;
#[cfg(test)]
mod us154_tests;
#[cfg(test)]
mod us154b_tests;
#[cfg(test)]
mod us155_tests;
#[cfg(test)]
mod us163_tests;
#[cfg(test)]
mod us165_tests;
#[cfg(test)]
mod us168_tests;
#[cfg(test)]
mod us172_tests;
mod variables;
mod variants;
mod vector_import;
mod versions;
mod view;
mod views;
mod workspace;
mod zones;

use std::path::PathBuf;

/// Options de lancement.
#[derive(Debug, Default)]
pub struct Launch {
    /// Projet à ouvrir.
    pub file: Option<PathBuf>,
    /// Ouvre le projet de démonstration (exercice de référence).
    pub demo: bool,
    /// Enregistre une capture de la fenêtre puis quitte (vérification).
    pub screenshot: Option<PathBuf>,
    /// Avec `--demo` : place l'éditeur au milieu d'un geste (`segment`,
    /// `perpendiculaire`, `point`, `angle`) pour les captures de vérification.
    pub stage: Option<String>,
    /// Langue imposée pour cette session (`--lang fr|en`), sans modifier le
    /// réglage enregistré. Absent : réglage enregistré, sinon français.
    pub lang: Option<pf_i18n::Lang>,
    /// Thème sombre imposé pour cette session (`--theme dark`), sans modifier le réglage.
    pub dark: Option<bool>,
    /// Taille de la fenêtre en points (`--taille 1600x1000`), pour les captures.
    pub size: Option<[f32; 2]>,
    /// Décalage vertical imposé aux panneaux défilants (`--defile 600`), pour les captures.
    pub scroll: Option<f32>,
}

fn parse_args() -> Launch {
    let mut launch = Launch::default();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--demo" => launch.demo = true,
            "--screenshot" => launch.screenshot = args.next().map(PathBuf::from),
            "--etape" => launch.stage = args.next(),
            "--taille" => {
                launch.size = args.next().and_then(|v| {
                    let (w, h) = v.split_once('x')?;
                    Some([w.parse().ok()?, h.parse().ok()?])
                });
            }
            "--defile" => launch.scroll = args.next().and_then(|v| v.parse().ok()),
            "--liste-etapes" => {
                for stage in stages::STAGES {
                    println!("{stage}");
                }
                std::process::exit(0);
            }
            "--theme" => launch.dark = args.next().map(|v| v == "dark"),
            "--lang" => launch.lang = args.next().and_then(|v| pf_i18n::Lang::from_code(&v)),
            other if !other.starts_with("--") => launch.file = Some(PathBuf::from(other)),
            _ => {}
        }
    }
    launch
}

fn main() -> eframe::Result {
    let launch = parse_args();
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("PatternForge")
            .with_inner_size(launch.size.unwrap_or([1280.0, 800.0]))
            .with_min_inner_size([900.0, 560.0]),
        ..Default::default()
    };
    eframe::run_native(
        "PatternForge",
        options,
        Box::new(move |cc| Ok(Box::new(app::App::new(cc, launch)))),
    )
}
