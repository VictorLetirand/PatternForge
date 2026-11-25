//! US-17.2 - fenêtre « Versions » du dock (menu Fichier) : instantanés nommés du projet, rangés
//! à côté du fichier (`pf_core::versions`), comparaison calculée par `pf_core::version_diff`
//! (lecture seule, rien n'est stocké dans le projet), restauration en **une** étape annulable
//! (l'état courant est d'abord instantané sous le nom « avant restauration »).
//!
//! Aucun calcul métier ici : la fenêtre affiche ce que renvoie le cœur. La vue visuelle
//! superpose les deux états d'une feuille : supprimé = trait **tireté** rouge avec une croix ×,
//! ajouté = trait **épais** vert avec un plus +, déplacé = **flèche**, inchangé = gris ; la
//! couleur n'est jamais la seule information (règle fondamentale 6). Les courbes y sont
//! dessinées par leur corde.

use crate::app::App;
use crate::dock::ToolWin;
use crate::safeguard::unix_now;
use crate::view::View;
use eframe::egui::{self, Pos2, RichText, Sense, Shape, Stroke, Vec2};
use pf_core::version_diff::{
    diff_projects, overlay, Overlay, OverlayKind, PieceChange, Presence, VersionDiff,
};
use pf_core::versions::{self, NameError, VersionError, VersionInfo};
use pf_core::{Project, SheetId};
use pf_geometry::{Bbox, PointMm};
use pf_i18n::{lang, tr, tr_args};
use pf_ui::components::{button, ButtonKind};
use std::path::PathBuf;

/// Lignes de détail affichées par catégorie avant « … et N autres ».
const MAX_ROWS: usize = 12;

pub struct CompareView {
    pub label_a: String,
    pub label_b: String,
    pub old: Project,
    pub new: Project,
    pub diff: VersionDiff,
    pub sheet: Option<SheetId>,
}

/// État de la fenêtre (interface seulement, jamais enregistré).
#[derive(Default)]
pub struct VersionsDialog {
    pub name: String,
    /// Une ou deux versions choisies (fichiers), la plus ancienne choisie sortant en premier.
    pub selected: Vec<String>,
    pub list: Vec<VersionInfo>,
    pub note: Option<(String, bool)>,
    pub confirm_delete: Option<String>,
    pub compare: Option<CompareView>,
}

enum Act {
    Create,
    Compare,
    Restore,
    AskDelete,
    ConfirmDelete(String),
    CancelDelete,
    Pick(String),
}

fn mm_text(v: f64) -> String {
    if pf_core::units::display_units().unit != pf_core::units::LengthUnit::Mm {
        return pf_core::units::len_number(v, 3);
    }
    let s = format!("{v:.3}");
    if lang().decimal_separator() == ',' {
        s.replace('.', ",")
    } else {
        s
    }
}

pub(crate) fn version_error_text(e: &VersionError) -> String {
    match e {
        VersionError::Name(NameError::Empty) => tr("verswin.err.name_empty"),
        VersionError::Name(NameError::TooLong) => tr("verswin.err.name_long"),
        VersionError::Name(NameError::ForbiddenChar(c)) => {
            tr_args("verswin.err.name_char", &[("char", &c.to_string())])
        }
        VersionError::Name(NameError::Reserved) => tr("verswin.err.name_reserved"),
        VersionError::Name(NameError::TrailingDot) => tr("verswin.err.name_dot"),
        VersionError::Locked => tr("verswin.err.locked"),
        VersionError::Unknown(n) => tr_args("verswin.err.unknown", &[("name", n)]),
        VersionError::Damaged(n) => tr_args("verswin.err.damaged", &[("name", n)]),
        VersionError::Io(d) | VersionError::Core(pf_core::CoreError::Io(d)) => {
            tr_args("verswin.err.io", &[("detail", d)])
        }
        VersionError::Core(c) => tr_args("verswin.err.io", &[("detail", &c.to_string())]),
    }
}

fn list_text(label_key: &str, items: &[String]) -> Option<String> {
    (!items.is_empty()).then(|| tr_args(label_key, &[("list", &items.join(", "))]))
}

fn named_changes(items: &[pf_core::version_diff::NamedChange]) -> Vec<String> {
    items
        .iter()
        .map(|c| {
            let mark = match c.change {
                Presence::Added => '+',
                Presence::Removed => '-',
                Presence::Changed => '~',
            };
            format!("{mark} {}", c.name)
        })
        .collect()
}

fn piece_change_text(c: &PieceChange) -> String {
    let unset = tr("verswin.cmp.unset");
    let opt = |v: &Option<String>| v.clone().unwrap_or_else(|| unset.clone());
    match c {
        PieceChange::Name { from, to } => {
            tr_args("verswin.cmp.piece.name", &[("from", from), ("to", to)])
        }
        PieceChange::Material { from, to } => tr_args(
            "verswin.cmp.piece.material",
            &[("from", &opt(from)), ("to", &opt(to))],
        ),
        PieceChange::Seam => tr("verswin.cmp.piece.seam"),
    }
}

/// Lignes de texte de la comparaison (tout est traduit ; aucune information par la couleur).
pub(crate) fn diff_lines(d: &VersionDiff) -> Vec<String> {
    let mut out = Vec::new();
    let names = |v: &[(pf_core::SheetId, String)]| -> Vec<String> {
        v.iter().map(|(_, n)| format!("« {n} »")).collect()
    };
    out.extend(list_text(
        "verswin.cmp.sheets_added",
        &names(&d.sheets_added),
    ));
    out.extend(list_text(
        "verswin.cmp.sheets_removed",
        &names(&d.sheets_removed),
    ));
    for s in &d.sheets {
        out.push(tr_args("verswin.cmp.sheet", &[("name", &s.name)]));
        if let Some((a, b)) = &s.renamed {
            out.push(tr_args("verswin.cmp.renamed", &[("from", a), ("to", b)]));
        }
        out.push(tr_args(
            "verswin.cmp.points",
            &[
                ("added", &s.points_added.len().to_string()),
                ("removed", &s.points_removed.len().to_string()),
                ("moved", &s.points_moved.len().to_string()),
            ],
        ));
        for m in s.points_moved.iter().take(MAX_ROWS) {
            out.push(pf_i18n::tr_len(
                "verswin.cmp.move",
                &[("label", &m.label), ("dist", &mm_text(m.distance_mm))],
            ));
        }
        if s.points_moved.len() > MAX_ROWS {
            out.push(tr_args(
                "verswin.cmp.more",
                &[("count", &(s.points_moved.len() - MAX_ROWS).to_string())],
            ));
        }
        out.push(tr_args(
            "verswin.cmp.paths",
            &[
                ("added", &s.paths_added.len().to_string()),
                ("removed", &s.paths_removed.len().to_string()),
                ("changed", &s.paths_changed.len().to_string()),
            ],
        ));
    }
    if !(d.pieces_added.is_empty() && d.pieces_removed.is_empty() && d.pieces.is_empty()) {
        out.push(tr_args(
            "verswin.cmp.pieces",
            &[
                ("added", &d.pieces_added.len().to_string()),
                ("removed", &d.pieces_removed.len().to_string()),
                ("changed", &d.pieces.len().to_string()),
            ],
        ));
        for p in d.pieces.iter().take(MAX_ROWS) {
            let what: Vec<String> = p.changes.iter().map(piece_change_text).collect();
            out.push(tr_args(
                "verswin.cmp.piece",
                &[("name", &p.name), ("what", &what.join(" ; "))],
            ));
        }
    }
    out.extend(list_text("verswin.cmp.sizes", &named_changes(&d.sizes)));
    out.extend(list_text(
        "verswin.cmp.variables",
        &named_changes(&d.variables),
    ));
    out
}

impl App {
    /// Menu Fichier, « Versions… ».
    pub fn open_versions(&mut self) {
        if self.versions.is_none() {
            self.versions = Some(VersionsDialog::default());
        }
        self.versions_reload();
    }

    fn versions_project_path(&self) -> Option<PathBuf> {
        self.editor.doc.path.clone()
    }

    fn versions_note(&mut self, text: String, error: bool) {
        if let Some(d) = self.versions.as_mut() {
            d.note = Some((text, error));
        }
    }

    /// Relit la liste (index confronté aux fichiers par le cœur).
    pub(crate) fn versions_reload(&mut self) {
        let Some(path) = self.versions_project_path() else {
            if let Some(d) = self.versions.as_mut() {
                d.list.clear();
            }
            return;
        };
        match versions::list(&path) {
            Ok(l) => {
                if let Some(d) = self.versions.as_mut() {
                    d.selected
                        .retain(|f| l.versions.iter().any(|v| &v.file == f));
                    d.list = l.versions;
                    if l.index_rebuilt {
                        d.note = Some((tr("verswin.index_rebuilt"), false));
                    }
                }
            }
            Err(e) => self.versions_note(version_error_text(&e), true),
        }
    }

    /// Crée la version nommée `name` de l'état courant ; `true` si elle existe sur disque.
    pub(crate) fn create_version(&mut self, name: &str) -> bool {
        let Some(path) = self.versions_project_path() else {
            self.versions_note(tr("verswin.unsaved"), true);
            return false;
        };
        match versions::create(&path, self.editor.project(), name, unix_now()) {
            Ok(info) => {
                if let Some(d) = self.versions.as_mut() {
                    d.name.clear();
                }
                self.versions_reload();
                self.versions_note(tr_args("verswin.created", &[("name", &info.name)]), false);
                true
            }
            Err(e) => {
                self.versions_note(version_error_text(&e), true);
                false
            }
        }
    }

    /// Restaure `file` : instantané automatique de l'état courant d'abord (si celui-ci échoue,
    /// rien n'est restauré), puis remplacement en une étape annulable.
    pub(crate) fn restore_version_file(&mut self, file: &str) -> bool {
        let Some(path) = self.versions_project_path() else {
            self.versions_note(tr("verswin.unsaved"), true);
            return false;
        };
        let target = match versions::read(&path, file) {
            Ok(p) => p,
            Err(e) => {
                self.versions_note(version_error_text(&e), true);
                return false;
            }
        };
        let label = self
            .versions
            .as_ref()
            .and_then(|d| d.list.iter().find(|v| v.file == file))
            .map_or_else(|| file.to_string(), |v| v.name.clone());
        if let Err(e) = versions::create(
            &path,
            self.editor.project(),
            &tr("verswin.before_restore"),
            unix_now(),
        ) {
            self.versions_note(version_error_text(&e), true);
            return false;
        }
        if let Err(e) = self.editor.restore_version(target) {
            self.versions_note(version_error_text(&VersionError::Core(e)), true);
            return false;
        }
        if let Some(d) = self.versions.as_mut() {
            d.selected.clear();
            d.compare = None;
        }
        self.versions_reload();
        self.versions_note(tr_args("verswin.restored", &[("name", &label)]), false);
        self.editor.status.text = tr_args("verswin.restored", &[("name", &label)]);
        self.editor.status.error = false;
        true
    }

    /// Supprime `file` (la confirmation a déjà eu lieu).
    pub(crate) fn delete_version_file(&mut self, file: &str) -> bool {
        let Some(path) = self.versions_project_path() else {
            return false;
        };
        match versions::delete(&path, file, unix_now()) {
            Ok(()) => {
                if let Some(d) = self.versions.as_mut() {
                    d.selected.retain(|f| f != file);
                    d.compare = None;
                }
                self.versions_reload();
                self.versions_note(tr("verswin.deleted"), false);
                true
            }
            Err(e) => {
                self.versions_note(version_error_text(&e), true);
                false
            }
        }
    }

    /// Compare une version à l'état courant, ou deux versions entre elles (la plus ancienne
    /// d'abord).
    pub(crate) fn compare_versions(&mut self, files: &[String]) -> bool {
        let Some(path) = self.versions_project_path() else {
            return false;
        };
        let mut files: Vec<String> = files.to_vec();
        files.sort();
        let label = |app: &App, f: &str| {
            app.versions
                .as_ref()
                .and_then(|d| d.list.iter().find(|v| v.file == f))
                .map_or_else(|| f.to_string(), |v| format!("{} ({})", v.name, v.created))
        };
        let (old, new, la, lb) = match files.as_slice() {
            [a] => match versions::read(&path, a) {
                Ok(p) => (
                    p,
                    self.editor.project().clone(),
                    label(self, a),
                    tr("verswin.cmp.current"),
                ),
                Err(e) => {
                    self.versions_note(version_error_text(&e), true);
                    return false;
                }
            },
            [a, b] => match (versions::read(&path, a), versions::read(&path, b)) {
                (Ok(x), Ok(y)) => (x, y, label(self, a), label(self, b)),
                (Err(e), _) | (_, Err(e)) => {
                    self.versions_note(version_error_text(&e), true);
                    return false;
                }
            },
            _ => return false,
        };
        let diff = diff_projects(&old, &new);
        let sheet = diff
            .sheets
            .first()
            .map(|s| s.sheet)
            .or_else(|| new.sheets.first().map(|s| s.id));
        if let Some(d) = self.versions.as_mut() {
            d.compare = Some(CompareView {
                label_a: la,
                label_b: lb,
                old,
                new,
                diff,
                sheet,
            });
        }
        true
    }

    pub fn versions_window(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.versions.take() else {
            return;
        };
        let tokens = pf_ui::theme::tokens(ctx);
        let secondary = tokens.text_secondary;
        let has_path = self.versions_project_path().is_some();
        let dir = self
            .versions_project_path()
            .map(|p| {
                versions::versions_dir(&p)
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default()
            })
            .unwrap_or_default();
        let mut open = true;
        let mut close = false;
        let mut act: Option<Act> = None;
        let slot = crate::dock::slot(ctx, &self.dock, ToolWin::Versions);
        let width = self.dock.content_width(ToolWin::Versions);
        egui::Window::new(tr("verswin.title"))
            .id(ToolWin::Versions.id())
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
                        crate::a11y::label(
                            ui,
                            RichText::new(tr_args("verswin.intro", &[("dir", &dir)]))
                                .color(secondary),
                        );
                        if !has_path {
                            crate::a11y::label(ui, RichText::new(tr("verswin.unsaved")).strong());
                        } else {
                            ui.add_space(4.0);
                            crate::a11y::label(ui, tr("verswin.name_label"));
                            ui.horizontal(|ui| {
                                crate::a11y::add_text(
                                    ui,
                                    egui::TextEdit::singleline(&mut d.name)
                                        .desired_width(width - 150.0)
                                        .char_limit(versions::MAX_NAME_CHARS + 10),
                                );
                                if button(ui, &tr("verswin.create"), ButtonKind::Primary).clicked()
                                {
                                    act = Some(Act::Create);
                                }
                            });
                        }
                        if let Some((text, err)) = &d.note {
                            let color = if *err { tokens.danger } else { secondary };
                            crate::a11y::label(ui, RichText::new(text).color(color));
                        }
                        ui.add_space(6.0);
                        crate::a11y::label(
                            ui,
                            RichText::new(tr_args(
                                "verswin.list",
                                &[("count", &d.list.len().to_string())],
                            ))
                            .strong(),
                        );
                        if d.list.is_empty() {
                            crate::a11y::label(
                                ui,
                                RichText::new(tr("verswin.empty")).color(secondary),
                            );
                        } else {
                            crate::a11y::label(
                                ui,
                                RichText::new(tr("verswin.select_hint")).color(secondary),
                            );
                        }
                        for v in &d.list {
                            let mut text = tr_args(
                                "verswin.row",
                                &[
                                    ("name", &v.name),
                                    ("date", &v.created),
                                    ("kb", &(v.size.div_ceil(1024)).to_string()),
                                ],
                            );
                            if v.damaged {
                                text.push(' ');
                                text.push_str(&tr("verswin.damaged"));
                            }
                            let on = d.selected.contains(&v.file);
                            if ui.selectable_label(on, text).clicked() {
                                act = Some(Act::Pick(v.file.clone()));
                            }
                        }
                        ui.add_space(4.0);
                        let n = d.selected.len();
                        ui.horizontal_wrapped(|ui| {
                            if ui
                                .add_enabled(n >= 1, egui::Button::new(tr("verswin.compare")))
                                .clicked()
                            {
                                act = Some(Act::Compare);
                            }
                            if ui
                                .add_enabled(n == 1, egui::Button::new(tr("verswin.restore")))
                                .clicked()
                            {
                                act = Some(Act::Restore);
                            }
                            if ui
                                .add_enabled(n == 1, egui::Button::new(tr("verswin.delete")))
                                .clicked()
                            {
                                act = Some(Act::AskDelete);
                            }
                        });
                        crate::a11y::label(
                            ui,
                            RichText::new(tr_args(
                                "verswin.restore_note",
                                &[("name", &tr("verswin.before_restore"))],
                            ))
                            .color(secondary),
                        );
                        if let Some(file) = d.confirm_delete.clone() {
                            let name = d
                                .list
                                .iter()
                                .find(|v| v.file == file)
                                .map_or(file.clone(), |v| v.name.clone());
                            ui.add_space(4.0);
                            crate::a11y::label(
                                ui,
                                RichText::new(tr_args(
                                    "verswin.confirm_delete",
                                    &[("name", &name)],
                                ))
                                .strong(),
                            );
                            ui.horizontal(|ui| {
                                if button(ui, &tr("verswin.delete_yes"), ButtonKind::Primary)
                                    .clicked()
                                {
                                    act = Some(Act::ConfirmDelete(file.clone()));
                                }
                                if button(ui, &tr("dialog.cancel"), ButtonKind::Secondary).clicked()
                                {
                                    act = Some(Act::CancelDelete);
                                }
                            });
                        }
                        if let Some(c) = d.compare.as_mut() {
                            draw_compare(ui, c, secondary, &tokens);
                        }
                        ui.add_space(6.0);
                        close = button(ui, &tr("dialog.close"), ButtonKind::Secondary).clicked();
                    });
            });
        let selected = d.selected.clone();
        if open && !close {
            self.versions = Some(d);
        }
        match act {
            Some(Act::Create) => {
                let name = self
                    .versions
                    .as_ref()
                    .map(|d| d.name.clone())
                    .unwrap_or_default();
                self.create_version(&name);
            }
            Some(Act::Compare) => {
                self.compare_versions(&selected);
            }
            Some(Act::Restore) => {
                if let Some(f) = selected.first() {
                    self.restore_version_file(f);
                }
            }
            Some(Act::AskDelete) => {
                if let Some(d) = self.versions.as_mut() {
                    d.confirm_delete = selected.first().cloned();
                }
            }
            Some(Act::ConfirmDelete(f)) => {
                if let Some(d) = self.versions.as_mut() {
                    d.confirm_delete = None;
                }
                self.delete_version_file(&f);
            }
            Some(Act::CancelDelete) => {
                if let Some(d) = self.versions.as_mut() {
                    d.confirm_delete = None;
                }
            }
            Some(Act::Pick(f)) => {
                if let Some(d) = self.versions.as_mut() {
                    if let Some(i) = d.selected.iter().position(|x| *x == f) {
                        d.selected.remove(i);
                    } else {
                        d.selected.push(f);
                        if d.selected.len() > 2 {
                            d.selected.remove(0);
                        }
                    }
                }
            }
            None => {}
        }
    }
}

fn draw_compare(
    ui: &mut egui::Ui,
    c: &mut CompareView,
    secondary: egui::Color32,
    tokens: &pf_ui::tokens::Tokens,
) {
    ui.add_space(6.0);
    ui.separator();
    crate::a11y::label(
        ui,
        RichText::new(tr_args(
            "verswin.cmp.title",
            &[("a", &c.label_a), ("b", &c.label_b)],
        ))
        .strong(),
    );
    if c.diff.is_empty() {
        crate::a11y::label(ui, RichText::new(tr("verswin.cmp.none")).color(secondary));
        return;
    }
    let candidates: Vec<(SheetId, String)> = c
        .new
        .sheets
        .iter()
        .chain(c.old.sheets.iter())
        .map(|s| (s.id, s.name.clone()))
        .fold(Vec::new(), |mut acc, x| {
            if !acc.iter().any(|(id, _)| *id == x.0) {
                acc.push(x);
            }
            acc
        });
    if candidates.len() > 1 {
        ui.horizontal_wrapped(|ui| {
            crate::a11y::label(ui, tr("verswin.sheet_pick"));
            for (id, name) in &candidates {
                if ui.selectable_label(c.sheet == Some(*id), name).clicked() {
                    c.sheet = Some(*id);
                }
            }
        });
    }
    if let Some(sheet) = c.sheet {
        let ov = overlay(&c.old, &c.new, sheet);
        paint_overlay(ui, &ov, tokens);
    }
    crate::a11y::label(ui, RichText::new(tr("verswin.legend")).color(secondary));
    for line in diff_lines(&c.diff) {
        crate::a11y::label(ui, line);
    }
}

fn bbox_of(ov: &Overlay) -> Option<Bbox> {
    let mut bb: Option<Bbox> = None;
    let mut add = |p: PointMm| match bb.as_mut() {
        Some(b) => b.include(p),
        None => bb = Some(Bbox::from_point(p)),
    };
    for l in &ov.lines {
        add(l.from);
        add(l.to);
    }
    for p in &ov.points {
        add(p.at);
    }
    for m in &ov.moves {
        add(m.from);
        add(m.to);
    }
    bb
}

fn paint_overlay(ui: &mut egui::Ui, ov: &Overlay, t: &pf_ui::tokens::Tokens) {
    let width = ui.available_width().clamp(200.0, 440.0);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 200.0), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 2.0, t.surface_canvas);
    painter.rect_stroke(
        rect,
        2.0,
        Stroke::new(1.0_f32, t.border),
        egui::StrokeKind::Inside,
    );
    let Some(bb) = bbox_of(ov) else {
        return;
    };
    let mut view = View::default();
    view.fit(
        Some(bb),
        (f64::from(rect.width()), f64::from(rect.height())),
    );
    let c = (f64::from(rect.center().x), f64::from(rect.center().y));
    let pos = |p: PointMm| {
        let s = view.to_screen(p, c);
        Pos2::new(s.0 as f32, s.1 as f32)
    };
    let cross = |at: Pos2, stroke: Stroke, diagonal: bool| {
        let r = 4.0;
        let (a, b) = if diagonal {
            (Vec2::new(r, r), Vec2::new(r, -r))
        } else {
            (Vec2::new(r, 0.0), Vec2::new(0.0, r))
        };
        painter.line_segment([at - a, at + a], stroke);
        painter.line_segment([at - b, at + b], stroke);
    };
    // Inchangé : gris, trait fin continu.
    for l in ov.lines.iter().filter(|l| l.kind == OverlayKind::Kept) {
        painter.line_segment(
            [pos(l.from), pos(l.to)],
            Stroke::new(1.0_f32, t.text_secondary),
        );
    }
    for l in ov.lines.iter().filter(|l| l.kind == OverlayKind::Removed) {
        let stroke = Stroke::new(1.5_f32, t.danger);
        painter.extend(Shape::dashed_line(
            &[pos(l.from), pos(l.to)],
            stroke,
            6.0,
            4.0,
        ));
        cross(pos(l.from).lerp(pos(l.to), 0.5), stroke, true);
    }
    for l in ov.lines.iter().filter(|l| l.kind == OverlayKind::Added) {
        let stroke = Stroke::new(3.0_f32, t.success);
        painter.line_segment([pos(l.from), pos(l.to)], stroke);
        cross(
            pos(l.from).lerp(pos(l.to), 0.5),
            Stroke::new(1.5_f32, t.text_primary),
            false,
        );
    }
    for p in &ov.points {
        match p.kind {
            OverlayKind::Removed => cross(pos(p.at), Stroke::new(1.5_f32, t.danger), true),
            OverlayKind::Added => cross(pos(p.at), Stroke::new(2.0_f32, t.success), false),
            OverlayKind::Kept => {}
        }
    }
    for m in &ov.moves {
        let (a, b) = (pos(m.from), pos(m.to));
        painter.circle_stroke(a, 3.0, Stroke::new(1.0_f32, t.warning));
        painter.arrow(a, b - a, Stroke::new(2.0_f32, t.warning));
    }
}
