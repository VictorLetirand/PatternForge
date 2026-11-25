//! US-13.6 (impression directe) et US-13.7 (projection) : tests egui FR et EN, faux spouleur.
//! Valeurs écrites à la main : écran de 640 mm sur 1280 px = 2 px/mm, 100 mm = 200 px.

use crate::app::App;
use crate::direct_print::{PrintError, PrintRequest, PrinterInfo, Spooler};
use crate::dock::ToolWin;
use crate::editor::Editor;
use crate::Launch;
use eframe::egui;
use eframe::egui::accesskit::Role;
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use pf_core::{Command, Document, PointRef, Project};
use pf_geometry::PointMm;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

type Jobs = Arc<Mutex<Vec<(PrintRequest, Vec<u8>)>>>;

struct Rec {
    list: Vec<PrinterInfo>,
    jobs: Jobs,
}

impl Spooler for Rec {
    fn printers(&self) -> Result<Vec<PrinterInfo>, PrintError> {
        Ok(self.list.clone())
    }
    fn submit(&self, job: &PrintRequest) -> Result<(), PrintError> {
        let bytes = std::fs::read(&job.file).map_err(|e| PrintError::Io(e.to_string()))?;
        self.jobs.lock().unwrap().push((job.clone(), bytes));
        Ok(())
    }
}

fn tmp(name: &str) -> PathBuf {
    let d = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target")
        .join(format!("us136-ui-{name}-{}", pf_i18n::lang().code()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d.canonicalize().unwrap()
}

fn harness() -> Harness<'static, App> {
    let app = App::with_context(&egui::Context::default(), Launch::default());
    let mut h = Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .build_state(|ctx, app: &mut App| app.ui(ctx), app);
    h.run();
    h
}

/// Projet à un segment de 100 mm : de quoi produire un PDF.
fn with_segment(h: &mut Harness<'static, App>) {
    let mut p = Project::new("Essai");
    let s = p.sheets[0].id;
    p.apply(Command::AddSegment {
        sheet: s,
        from: PointRef::New(PointMm::new(0.0, 0.0)),
        to: PointRef::New(PointMm::new(100.0, 0.0)),
    })
    .unwrap();
    h.state_mut().editor = Editor::new(Document::new(p));
    h.state_mut().fit_pending = true;
    h.run_steps(4);
}

struct Labels {
    file_menu: &'static str,
    print_item: &'static str,
    proj_item: &'static str,
    send: &'static str,
    start: &'static str,
    sent_part: &'static str,
    no_printer: &'static str,
    bad_width: &'static str,
}

fn walk_print(l: &Labels) {
    let dir = tmp("print");
    let mut h = harness();
    with_segment(&mut h);
    let jobs: Jobs = Arc::default();
    h.state_mut().spooler = Box::new(Rec {
        list: vec![
            PrinterInfo {
                name: "Alpha".into(),
                is_default: false,
            },
            PrinterInfo {
                name: "Beta".into(),
                is_default: true,
            },
        ],
        jobs: jobs.clone(),
    });
    h.state_mut().print_temp_dir = dir.clone();

    h.get_by_label(l.file_menu).click_accesskit();
    h.run_steps(3);
    h.get_by_label(&pf_i18n::tr("menu.file.print_setup"))
        .click_accesskit();
    h.run_steps(4);
    h.get_by_role_and_label(Role::Button, l.print_item)
        .click_accesskit();
    h.run_steps(4);
    let win = h
        .ctx
        .memory(|m| m.area_rect(ToolWin::Print.id()))
        .expect("fenêtre Imprimer");
    assert!(h.state().dock.rect.contains_rect(win));
    assert!(!win.intersects(h.state().canvas_rect));
    assert_eq!(
        h.state().direct_print.as_ref().unwrap().selected.as_deref(),
        Some("Beta"),
        "valeur par défaut du système"
    );

    h.get_by_role_and_label(Role::Button, l.send)
        .click_accesskit();
    h.run_steps(4);
    let sent = jobs.lock().unwrap();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].0.printer, "Beta");
    assert_eq!(sent[0].0.scale_percent, 100);
    let (scenes, _) = h.state().pdf_scenes(false).unwrap();
    let expected = pf_export::pdf::export_pdf_lang(
        &scenes,
        &h.state().page_setup,
        &h.state().output_content,
        pf_i18n::lang(),
    )
    .unwrap();
    assert_eq!(sent[0].1, expected.bytes, "mêmes octets que l'export PDF");
    drop(sent);
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0, "temporaire");
    h.get_by_label_contains(l.sent_part);

    // Aucune imprimante : refus nommé, rien d'envoyé.
    h.state_mut().spooler = Box::new(Rec {
        list: Vec::new(),
        jobs: jobs.clone(),
    });
    h.state_mut().direct_print_refresh();
    h.state_mut().direct_print.as_mut().unwrap().note = None;
    h.run_steps(3);
    h.get_by_label_contains(l.no_printer);
    h.state_mut().direct_print_send();
    h.run_steps(2);
    assert_eq!(jobs.lock().unwrap().len(), 1);
    let _ = std::fs::remove_dir_all(dir);
}

fn shapes_of(h: &Harness<'static, App>) -> Vec<egui::Shape> {
    fn flat(s: &egui::Shape, out: &mut Vec<egui::Shape>) {
        if let egui::Shape::Vec(v) = s {
            v.iter().for_each(|x| flat(x, out));
        } else {
            out.push(s.clone());
        }
    }
    let mut out = Vec::new();
    for c in &h.output().shapes {
        flat(&c.shape, &mut out);
    }
    out
}

/// Boîte (x min, x max) du carré étalon doré dessiné par la projection.
fn gold_square(h: &Harness<'static, App>) -> (f32, f32, f32, f32) {
    let gold = egui::Color32::from_rgb(255, 220, 0);
    for s in shapes_of(h) {
        if let egui::Shape::Path(p) = s {
            if p.stroke.color == egui::epaint::ColorMode::Solid(gold) && p.points.len() == 4 {
                let xs: Vec<f32> = p.points.iter().map(|q| q.x).collect();
                let ys: Vec<f32> = p.points.iter().map(|q| q.y).collect();
                let f = |v: &[f32]| {
                    (
                        v.iter().cloned().fold(f32::MAX, f32::min),
                        v.iter().cloned().fold(f32::MIN, f32::max),
                    )
                };
                let (x0, x1) = f(&xs);
                let (y0, y1) = f(&ys);
                return (x0, x1, y0, y1);
            }
        }
    }
    panic!("carré étalon non dessiné");
}

fn walk_projection(l: &Labels) {
    let mut h = harness();
    with_segment(&mut h);
    h.get_by_label(l.file_menu).click_accesskit();
    h.run_steps(3);
    h.get_by_label(&pf_i18n::tr("menu.file.print_setup"))
        .click_accesskit();
    h.run_steps(4);
    h.get_by_role_and_label(Role::Button, l.proj_item)
        .click_accesskit();
    h.run_steps(4);
    let win = h
        .ctx
        .memory(|m| m.area_rect(ToolWin::Projection.id()))
        .expect("fenêtre Projection");
    assert!(h.state().dock.rect.contains_rect(win));

    // Largeur hors bornes : refus affiché, pas de projection.
    h.state_mut()
        .projection
        .as_mut()
        .unwrap()
        .settings
        .screen_width_mm = f64::NAN;
    assert!(!h.state_mut().projection_arm());
    h.run_steps(3);
    assert!(!h.state().projection.as_ref().unwrap().active);
    h.get_by_label_contains(l.bad_width);

    // 640 mm sur 1280 px : 2 px/mm, carré de 100 mm = 200 px, à 10 mm (20 px) du bord.
    h.state_mut()
        .projection
        .as_mut()
        .unwrap()
        .settings
        .screen_width_mm = 640.0;
    h.get_by_role_and_label(Role::Button, l.start)
        .click_accesskit();
    h.run_steps(4);
    assert!(h.state().projection.as_ref().unwrap().active);
    let (x0, x1, _, _) = gold_square(&h);
    assert_eq!((x0, x1), (20.0, 220.0));
    let black_full = shapes_of(&h).iter().any(|s| {
        matches!(s, egui::Shape::Rect(r)
            if r.fill == egui::Color32::BLACK && r.rect.width() >= 1280.0)
    });
    assert!(black_full, "fond noir plein écran");

    // Miroir horizontal : x -> 1280 - x, le carré reste de 200 px.
    h.state_mut().projection.as_mut().unwrap().settings.mirror_h = true;
    h.run_steps(3);
    let (x0, x1, _, _) = gold_square(&h);
    assert_eq!((x0, x1), (1060.0, 1260.0));

    // Échap quitte.
    h.key_press(egui::Key::Escape);
    h.run_steps(3);
    assert!(!h.state().projection.as_ref().unwrap().active);
}

#[test]
fn us136_fr_impression_directe() {
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
    walk_print(&LABELS_FR);
}

#[test]
fn us136_en_direct_printing() {
    pf_i18n::set_lang(pf_i18n::Lang::En);
    walk_print(&LABELS_EN);
}

#[test]
fn us137_fr_projection() {
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
    walk_projection(&LABELS_FR);
}

#[test]
fn us137_en_projection() {
    pf_i18n::set_lang(pf_i18n::Lang::En);
    walk_projection(&LABELS_EN);
}

const LABELS_FR: Labels = Labels {
    file_menu: "Fichier",
    print_item: "Imprimer…",
    proj_item: "Projection…",
    send: "Imprimer",
    start: "Lancer la projection",
    sent_part: "Envoyé à",
    no_printer: "Aucune imprimante installée",
    bad_width: "Largeur d'écran invalide",
};

const LABELS_EN: Labels = Labels {
    file_menu: "File",
    print_item: "Print…",
    proj_item: "Projection…",
    send: "Print",
    start: "Start projection",
    sent_part: "Sent to",
    no_printer: "No printer installed",
    bad_width: "Invalid screen width",
};
