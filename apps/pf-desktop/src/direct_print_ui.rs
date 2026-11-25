//! US-13.6 : fenêtre « Imprimer… » du dock (menu Fichier). Aucun calcul ici : le PDF est celui
//! de « Exporter PDF » (échelle 100 %, étalon compris) et `direct_print` l'envoie au spouleur.

use crate::app::App;
use crate::direct_print::{self, PrintError, PrinterInfo};
use crate::dock::ToolWin;
use crate::print_setup::setup_label;
use eframe::egui::{self, RichText};
use pf_export::pdf;
use pf_i18n::{tr, tr_args};
use pf_ui::components::{button, ButtonKind};
use std::sync::atomic::AtomicBool;

/// État de la fenêtre (interface seulement, jamais enregistré).
#[derive(Default)]
pub struct DirectPrintDialog {
    pub printers: Vec<PrinterInfo>,
    pub list_error: Option<String>,
    pub selected: Option<String>,
    pub copies: u32,
    pub all_sheets: bool,
    pub note: Option<(String, bool)>,
}

pub fn print_error(e: &PrintError) -> String {
    match e {
        PrintError::NoPrinter => tr("dprint.err.no_printer"),
        PrintError::UnknownPrinter(n) => tr_args("dprint.err.unknown", &[("name", n)]),
        PrintError::BadCopies(n) => tr_args("dprint.err.copies", &[("n", &n.to_string())]),
        PrintError::Cancelled => tr("dprint.err.cancelled"),
        PrintError::Io(d) => tr_args("dprint.err.io", &[("detail", d)]),
        PrintError::Spooler(d) => tr_args("dprint.err.spooler", &[("detail", d)]),
    }
}

impl App {
    pub fn open_direct_print(&mut self) {
        if self.direct_print.is_none() {
            let _ = direct_print::sweep_stale(&self.print_temp_dir);
            self.direct_print = Some(DirectPrintDialog {
                copies: 1,
                ..DirectPrintDialog::default()
            });
        }
        self.direct_print_refresh();
    }

    pub(crate) fn direct_print_refresh(&mut self) {
        let result = self.spooler.printers();
        let Some(d) = self.direct_print.as_mut() else {
            return;
        };
        match result {
            Ok(list) => {
                d.list_error = list.is_empty().then(|| tr("dprint.err.no_printer"));
                if !d
                    .selected
                    .as_ref()
                    .is_some_and(|s| list.iter().any(|p| &p.name == s))
                {
                    d.selected = direct_print::choose_printer(&list, None).ok();
                }
                d.printers = list;
            }
            Err(e) => {
                d.printers.clear();
                d.selected = None;
                d.list_error = Some(print_error(&e));
            }
        }
    }

    /// Génère le PDF d'export (mêmes octets que « Exporter PDF ») et l'envoie au spouleur.
    pub(crate) fn direct_print_send(&mut self) {
        let Some(d) = self.direct_print.as_ref() else {
            return;
        };
        let (all, copies, printer) = (d.all_sheets, d.copies, d.selected.clone());
        let outcome: Result<String, String> = (|| {
            let (scenes, _) = self.pdf_scenes(all)?;
            let out = pdf::export_pdf_lang(
                &scenes,
                &self.page_setup,
                &self.output_content,
                pf_i18n::lang(),
            )
            .map_err(|e| crate::print_setup::export_error(&e))?;
            let report = direct_print::print_pdf(
                self.spooler.as_ref(),
                &out.bytes,
                printer.as_deref(),
                copies,
                &self.print_temp_dir,
                &AtomicBool::new(false),
            )
            .map_err(|e| print_error(&e))?;
            let key = if report.temp_removed {
                "dprint.ok"
            } else {
                "dprint.ok_kept"
            };
            Ok(tr_args(
                key,
                &[
                    ("printer", &report.printer),
                    ("pages", &out.page_count.to_string()),
                    ("copies", &report.copies.to_string()),
                ],
            ))
        })();
        if let Some(d) = self.direct_print.as_mut() {
            d.note = Some(match outcome {
                Ok(t) => (t, false),
                Err(t) => (t, true),
            });
        }
    }

    pub fn direct_print_window(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.direct_print.take() else {
            return;
        };
        let tokens = pf_ui::theme::tokens(ctx);
        let secondary = tokens.text_secondary;
        let format = setup_label(&self.page_setup);
        let m = self.page_setup.margins;
        let margins = tr_args(
            "dprint.margins",
            &[
                ("format", &format),
                ("top", &crate::print_setup::factor_text(m.top_mm)),
                ("right", &crate::print_setup::factor_text(m.right_mm)),
                ("bottom", &crate::print_setup::factor_text(m.bottom_mm)),
                ("left", &crate::print_setup::factor_text(m.left_mm)),
            ],
        );
        let (mut open, mut close, mut send, mut refresh) = (true, false, false, false);
        let slot = crate::dock::slot(ctx, &self.dock, ToolWin::Print);
        let width = self.dock.content_width(ToolWin::Print);
        egui::Window::new(tr("dprint.title"))
            .id(ToolWin::Print.id())
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .fixed_pos(slot.pos)
            .default_height(slot.max_height)
            .max_height(slot.max_height)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .max_height(slot.max_height)
                    .show(ui, |ui| {
                        ui.set_width(width);
                        crate::a11y::label(ui, RichText::new(tr("dprint.intro")).color(secondary));
                        ui.add_space(4.0);
                        crate::a11y::label(ui, tr("dprint.printer"));
                        let current = d
                            .selected
                            .clone()
                            .unwrap_or_else(|| tr("dprint.none_short"));
                        ui.horizontal(|ui| {
                            use crate::a11y::ComboExt;
                            egui::ComboBox::from_id_salt("dprint_printer")
                                .selected_text(current)
                                .width(width - 160.0)
                                .show_ui_named(ui, |ui| {
                                    for p in &d.printers {
                                        let mut text = p.name.clone();
                                        if p.is_default {
                                            text.push(' ');
                                            text.push_str(&tr("dprint.default_tag"));
                                        }
                                        if ui
                                            .selectable_label(
                                                d.selected.as_deref() == Some(p.name.as_str()),
                                                text,
                                            )
                                            .clicked()
                                        {
                                            d.selected = Some(p.name.clone());
                                        }
                                    }
                                });
                            refresh =
                                button(ui, &tr("dprint.refresh"), ButtonKind::Secondary).clicked();
                        });
                        if let Some(e) = &d.list_error {
                            crate::a11y::label(ui, RichText::new(e).color(tokens.danger));
                        }
                        ui.add_space(4.0);
                        crate::a11y::label(ui, tr("dprint.copies"));
                        crate::a11y::add_drag(
                            ui,
                            egui::DragValue::new(&mut d.copies).range(1..=direct_print::MAX_COPIES),
                        );
                        ui.horizontal(|ui| {
                            ui.radio_value(&mut d.all_sheets, false, tr("dprint.scope_one"));
                            ui.radio_value(&mut d.all_sheets, true, tr("dprint.scope_all"));
                        });
                        ui.add_space(4.0);
                        crate::a11y::label(ui, RichText::new(margins).strong());
                        crate::a11y::label(
                            ui,
                            RichText::new(tr("dprint.scale_fixed")).color(secondary),
                        );
                        crate::a11y::label(ui, RichText::new(tr("dprint.human")).color(secondary));
                        if let Some((text, err)) = &d.note {
                            let color = if *err { tokens.danger } else { secondary };
                            crate::a11y::label(ui, RichText::new(text).color(color));
                        }
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            let ready = d.selected.is_some();
                            ui.add_enabled_ui(ready, |ui| {
                                send =
                                    button(ui, &tr("dprint.send"), ButtonKind::Primary).clicked();
                            });
                            close =
                                button(ui, &tr("dialog.close"), ButtonKind::Secondary).clicked();
                        });
                    });
            });
        if open && !close {
            self.direct_print = Some(d);
        }
        if refresh {
            self.direct_print_refresh();
        }
        if send {
            self.direct_print_send();
        }
    }
}
