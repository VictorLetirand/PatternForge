//! Catalogue jouable du design system.
//!
//! Usage : `pf-ui-lab [--page N] [--dark] [--toasts] [--dialog] [--screenshot image.png]`
//! (`N` de 0 à 4 ; `--screenshot` enregistre une capture puis quitte).

use pf_ui::components::ToastLevel;
use pf_ui::lab::{ui_lab, LabPage, LabState};
use pf_ui::strings::lab;
use std::path::PathBuf;

struct LabApp {
    state: LabState,
    screenshot: Option<PathBuf>,
    frame: u32,
}

impl eframe::App for LabApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ui_lab(ctx, &mut self.state);
        self.frame += 1;
        let Some(path) = self.screenshot.clone() else {
            return;
        };
        ctx.request_repaint();
        if self.frame == 8 {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
        }
        let image = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(image) = image {
            let [w, h] = image.size;
            if let Err(e) = image::save_buffer(
                &path,
                image.as_raw(),
                w as u32,
                h as u32,
                image::ExtendedColorType::Rgba8,
            ) {
                eprintln!("capture impossible : {e}");
            }
            self.screenshot = None;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

fn main() -> eframe::Result {
    let mut state = LabState::default();
    let mut screenshot = None;
    let (mut toasts, mut dialog) = (false, false);
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--screenshot" => screenshot = args.next().map(PathBuf::from),
            "--page" => {
                if let Some(p) = args.next().and_then(|n| n.parse::<usize>().ok()) {
                    state.page = LabPage::ALL[p.min(LabPage::ALL.len() - 1)];
                }
            }
            "--dark" => state.dark = true,
            "--toasts" => toasts = true,
            "--dialog" => dialog = true,
            _ => {}
        }
    }
    if toasts {
        for (level, text) in [
            (ToastLevel::Info, lab::TOAST_INFO),
            (ToastLevel::Success, lab::TOAST_SUCCESS),
            (ToastLevel::Warning, lab::TOAST_WARNING),
            (ToastLevel::Error, lab::TOAST_ERROR),
        ] {
            state.toasts.push_with_ttl(level, text, 0.0, 600.0);
        }
    }
    state.dialog_open = dialog;
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("PatternForge — UI Lab")
            .with_inner_size([1000.0, 640.0])
            .with_min_inner_size([900.0, 560.0]),
        ..Default::default()
    };
    eframe::run_native(
        "pf-ui-lab",
        options,
        Box::new(move |_cc| {
            Ok(Box::new(LabApp {
                state,
                screenshot,
                frame: 0,
            }))
        }),
    )
}
