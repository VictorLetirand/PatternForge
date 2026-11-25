//! US-16.5 - vues : unité d'affichage, grille et règles paramétrables, vues nommées.
//!
//! Ce sont des **réglages de l'application** (clé `views` de `settings.json`), jamais une donnée
//! du projet : aucune `Command` pour les réglages, rien dans le fichier `.pforge.json` (schéma 5
//! inchangé). Le modèle reste en mm ; l'unité ne sert qu'à l'affichage et à la saisie. Le fichier
//! est relu sans jamais paniquer : une clé illisible retombe sur sa valeur par défaut et le
//! problème est signalé (`Issue`).
//!
//! Une **vue nommée** retient le cadrage, le zoom, la feuille et la visibilité des calques par
//! leur nom. La rétablir change la vue et la feuille active ; la visibilité des calques, elle,
//! est une donnée du projet : elle passe par `SetLayerVisible`, en **une seule étape annulable**.

use crate::app::App;
use crate::view::{MAX_ZOOM, MIN_ZOOM};
use eframe::egui::{self, RichText};
use pf_core::units::{
    display_units, format_field, format_length, inches_to_fraction, parse_length_with,
    set_display_units, DisplayUnits, LengthUnit, INCH_DENOMINATORS, MM_PER_INCH,
};
use pf_core::{Command, LayerId, SheetId};
use pf_geometry::PointMm;
use pf_i18n::{settings, tr, tr_args};
use serde_json::{json, Map, Value};
use std::path::Path;

pub const SETTINGS_KEY: &str = "views";
pub const VERSION: u64 = 1;
pub const MAX_NAMED_VIEWS: usize = 50;
pub const NAME_MAX_CHARS: usize = 40;
pub const MAX_LAYERS_PER_VIEW: usize = 200;
pub const GRID_STEP_MIN_MM: f64 = 0.5;
pub const GRID_STEP_MAX_MM: f64 = 10_000.0;
pub const SUBDIVISIONS_MIN: u32 = 1;
pub const SUBDIVISIONS_MAX: u32 = 20;

/// Grille du plan : `step_mm` = écart entre repères (traits forts), divisé en `subdivisions`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GridSettings {
    pub visible: bool,
    pub step_mm: f64,
    pub subdivisions: u32,
}

impl GridSettings {
    /// Grille par défaut d'une unité : 50 mm en 5 (mm, cm : repère tous les 5 cm, trait tous les
    /// cm), 1 po en 4 (trait tous les 1/4 po).
    pub fn default_for(unit: LengthUnit) -> GridSettings {
        match unit {
            LengthUnit::Mm | LengthUnit::Cm => GridSettings {
                visible: true,
                step_mm: 50.0,
                subdivisions: 5,
            },
            LengthUnit::In => GridSettings {
                visible: true,
                step_mm: MM_PER_INCH,
                subdivisions: 4,
            },
        }
    }

    /// Valeurs ramenées dans les bornes (jamais de pas nul, négatif ou infini).
    pub fn clamped(self) -> GridSettings {
        GridSettings {
            visible: self.visible,
            step_mm: if self.step_mm.is_finite() {
                self.step_mm.clamp(GRID_STEP_MIN_MM, GRID_STEP_MAX_MM)
            } else {
                50.0
            },
            subdivisions: self.subdivisions.clamp(SUBDIVISIONS_MIN, SUBDIVISIONS_MAX),
        }
    }

    /// Pas fin et pas fort affichés à ce zoom (px par mm) : le pas fin est élargi (×5 puis ×2)
    /// tant qu'il serre moins de 7 px, comme avant la grille paramétrable.
    pub fn levels(&self, zoom: f64) -> (f64, f64) {
        let g = self.clamped();
        let subdivisions = f64::from(g.subdivisions);
        let mut minor = g.step_mm / subdivisions;
        for i in 0..5 {
            if minor * zoom >= 7.0 {
                break;
            }
            minor *= if i % 2 == 0 { 5.0 } else { 2.0 };
        }
        (minor, minor * subdivisions)
    }
}

/// Vue nommée : cadrage, zoom, feuille et visibilité des calques (par nom).
#[derive(Debug, Clone, PartialEq)]
pub struct NamedView {
    pub name: String,
    pub sheet_id: u64,
    pub sheet_name: String,
    pub center: PointMm,
    pub zoom: f64,
    pub layers: Vec<(String, bool)>,
}

/// Problème rencontré à la lecture ou à une action sur les vues (clé `views.issue.<kind>`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    pub kind: &'static str,
    pub detail: String,
}

impl Issue {
    fn new(kind: &'static str, detail: impl Into<String>) -> Issue {
        Issue {
            kind,
            detail: detail.into(),
        }
    }

    pub fn text(&self) -> String {
        tr_args(
            &format!("views.issue.{}", self.kind),
            &[("detail", &self.detail)],
        )
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ViewSettings {
    pub units: DisplayUnits,
    pub grid: GridSettings,
    pub rulers: bool,
    pub named: Vec<NamedView>,
}

impl Default for ViewSettings {
    fn default() -> Self {
        ViewSettings {
            units: DisplayUnits::default(),
            grid: GridSettings::default_for(LengthUnit::Mm),
            rulers: false,
            named: Vec::new(),
        }
    }
}

/// Nom de vue nettoyé : espaces de bord retirés, 1 à 40 caractères, sans caractère de contrôle.
pub fn clean_name(name: &str) -> Result<String, Issue> {
    let name = name.trim();
    if name.is_empty() {
        return Err(Issue::new("name_empty", ""));
    }
    if name.chars().count() > NAME_MAX_CHARS {
        return Err(Issue::new("name_long", NAME_MAX_CHARS.to_string()));
    }
    if name.chars().any(char::is_control) {
        return Err(Issue::new("name_control", name));
    }
    Ok(name.to_string())
}

impl ViewSettings {
    /// Lit la valeur de la clé `views`. Ne panique jamais : une valeur illisible est remplacée
    /// par son défaut et listée dans les problèmes ; une vue invalide est écartée.
    pub fn from_json(value: &Value) -> (ViewSettings, Vec<Issue>) {
        let mut out = ViewSettings::default();
        let mut issues = Vec::new();
        let Some(map) = value.as_object() else {
            issues.push(Issue::new("not_object", ""));
            return (out, issues);
        };
        if map.get("version").and_then(Value::as_u64).unwrap_or(1) > VERSION {
            issues.push(Issue::new("newer", ""));
            return (out, issues);
        }
        if let Some(v) = map.get("unit") {
            match v.as_str().and_then(LengthUnit::from_code) {
                Some(u) => out.units.unit = u,
                None => issues.push(Issue::new("unit", v.to_string())),
            }
        }
        if let Some(v) = map.get("inch_denominator") {
            match v
                .as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .filter(|n| INCH_DENOMINATORS.contains(n))
            {
                Some(d) => out.units.inch_denominator = d,
                None => issues.push(Issue::new("inch_denominator", v.to_string())),
            }
        }
        out.grid = GridSettings::default_for(out.units.unit);
        if let Some(g) = map.get("grid") {
            match g.as_object() {
                None => issues.push(Issue::new("grid", g.to_string())),
                Some(g) => {
                    if let Some(v) = g.get("visible") {
                        match v.as_bool() {
                            Some(b) => out.grid.visible = b,
                            None => issues.push(Issue::new("grid", v.to_string())),
                        }
                    }
                    if let Some(v) = g.get("step_mm") {
                        match v
                            .as_f64()
                            .filter(|s| (GRID_STEP_MIN_MM..=GRID_STEP_MAX_MM).contains(s))
                        {
                            Some(s) => out.grid.step_mm = s,
                            None => issues.push(Issue::new("grid", v.to_string())),
                        }
                    }
                    if let Some(v) = g.get("subdivisions") {
                        match v
                            .as_u64()
                            .and_then(|n| u32::try_from(n).ok())
                            .filter(|n| (SUBDIVISIONS_MIN..=SUBDIVISIONS_MAX).contains(n))
                        {
                            Some(n) => out.grid.subdivisions = n,
                            None => issues.push(Issue::new("grid", v.to_string())),
                        }
                    }
                }
            }
        }
        if let Some(v) = map.get("rulers") {
            match v.as_bool() {
                Some(b) => out.rulers = b,
                None => issues.push(Issue::new("rulers", v.to_string())),
            }
        }
        match map.get("named") {
            None => {}
            Some(Value::Array(items)) => {
                for (i, item) in items.iter().enumerate() {
                    if out.named.len() >= MAX_NAMED_VIEWS {
                        issues.push(Issue::new("too_many", MAX_NAMED_VIEWS.to_string()));
                        break;
                    }
                    match NamedView::from_json(item) {
                        Ok(view) if out.find(&view.name).is_none() => out.named.push(view),
                        Ok(view) => issues.push(Issue::new("view_duplicate", view.name)),
                        Err(()) => issues.push(Issue::new("view_invalid", (i + 1).to_string())),
                    }
                }
            }
            Some(other) => issues.push(Issue::new("named", other.to_string())),
        }
        (out, issues)
    }

    pub fn to_json(&self) -> Value {
        let named: Vec<Value> = self.named.iter().map(NamedView::to_json).collect();
        json!({
            "version": VERSION,
            "unit": self.units.unit.code(),
            "inch_denominator": self.units.inch_denominator,
            "grid": {
                "visible": self.grid.visible,
                "step_mm": self.grid.step_mm,
                "subdivisions": self.grid.subdivisions,
            },
            "rulers": self.rulers,
            "named": named,
        })
    }

    /// Position d'une vue par son nom (casse ignorée).
    pub fn find(&self, name: &str) -> Option<usize> {
        let wanted = name.trim().to_lowercase();
        self.named
            .iter()
            .position(|v| v.name.to_lowercase() == wanted)
    }

    /// Change l'unité d'affichage. Si la grille était celle par défaut de l'ancienne unité, elle
    /// passe à celle de la nouvelle ; une grille réglée à la main est conservée.
    pub fn set_unit(&mut self, unit: LengthUnit) {
        if unit == self.units.unit {
            return;
        }
        let was_default = GridSettings {
            visible: self.grid.visible,
            ..GridSettings::default_for(self.units.unit)
        } == self.grid;
        self.units.unit = unit;
        if was_default {
            self.grid = GridSettings {
                visible: self.grid.visible,
                ..GridSettings::default_for(unit)
            };
        }
    }

    pub fn save_named(&mut self, view: NamedView) -> Result<(), Issue> {
        let name = clean_name(&view.name)?;
        if self.find(&name).is_some() {
            return Err(Issue::new("view_duplicate", name));
        }
        if self.named.len() >= MAX_NAMED_VIEWS {
            return Err(Issue::new("too_many", MAX_NAMED_VIEWS.to_string()));
        }
        self.named.push(NamedView { name, ..view });
        Ok(())
    }

    /// Remplace le cadrage d'une vue par celui donné ; le nom reste.
    pub fn update_named(&mut self, index: usize, view: NamedView) -> Result<(), Issue> {
        let slot = self
            .named
            .get_mut(index)
            .ok_or_else(|| Issue::new("view_missing", index.to_string()))?;
        let name = std::mem::take(&mut slot.name);
        *slot = NamedView { name, ..view };
        Ok(())
    }

    pub fn rename_named(&mut self, index: usize, new_name: &str) -> Result<(), Issue> {
        let name = clean_name(new_name)?;
        if index >= self.named.len() {
            return Err(Issue::new("view_missing", index.to_string()));
        }
        if self.find(&name).is_some_and(|i| i != index) {
            return Err(Issue::new("view_duplicate", name));
        }
        self.named[index].name = name;
        Ok(())
    }

    pub fn delete_named(&mut self, index: usize) -> Result<NamedView, Issue> {
        if index >= self.named.len() {
            return Err(Issue::new("view_missing", index.to_string()));
        }
        Ok(self.named.remove(index))
    }
}

impl NamedView {
    fn from_json(value: &Value) -> Result<NamedView, ()> {
        let map = value.as_object().ok_or(())?;
        let name =
            clean_name(map.get("name").and_then(Value::as_str).ok_or(())?).map_err(|_| ())?;
        let sheet_id = map.get("sheet_id").and_then(Value::as_u64).ok_or(())?;
        let sheet_name = map
            .get("sheet_name")
            .and_then(Value::as_str)
            .ok_or(())?
            .to_string();
        if sheet_name.chars().count() > 200 {
            return Err(());
        }
        let number = |key: &str| {
            map.get(key)
                .and_then(Value::as_f64)
                .filter(|v| v.is_finite())
        };
        let center = PointMm::new(number("center_x").ok_or(())?, number("center_y").ok_or(())?);
        let zoom = number("zoom")
            .filter(|z| (MIN_ZOOM..=MAX_ZOOM).contains(z))
            .ok_or(())?;
        let mut layers = Vec::new();
        for item in map
            .get("layers")
            .and_then(Value::as_array)
            .ok_or(())?
            .iter()
        {
            let l = item.as_object().ok_or(())?;
            let n = l.get("name").and_then(Value::as_str).ok_or(())?;
            let v = l.get("visible").and_then(Value::as_bool).ok_or(())?;
            if n.chars().count() > 200 {
                return Err(());
            }
            layers.push((n.to_string(), v));
        }
        if layers.len() > MAX_LAYERS_PER_VIEW {
            return Err(());
        }
        Ok(NamedView {
            name,
            sheet_id,
            sheet_name,
            center,
            zoom,
            layers,
        })
    }

    fn to_json(&self) -> Value {
        let layers: Vec<Value> = self
            .layers
            .iter()
            .map(|(n, v)| json!({"name": n, "visible": v}))
            .collect();
        let mut m = Map::new();
        m.insert("name".into(), json!(self.name));
        m.insert("sheet_id".into(), json!(self.sheet_id));
        m.insert("sheet_name".into(), json!(self.sheet_name));
        m.insert("center_x".into(), json!(self.center.x));
        m.insert("center_y".into(), json!(self.center.y));
        m.insert("zoom".into(), json!(self.zoom));
        m.insert("layers".into(), Value::Array(layers));
        Value::Object(m)
    }
}

/// Lit les réglages d'affichage du fichier de réglages. Fichier absent ou sans la clé : défauts
/// sans message ; fichier illisible, ou clé invalide : défauts (ou valeur par valeur) et problème
/// listé. Ne modifie jamais le fichier.
pub fn load(path: &Path) -> (ViewSettings, Vec<Issue>) {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return (ViewSettings::default(), Vec::new())
        }
        Err(e) => {
            return (
                ViewSettings::default(),
                vec![Issue::new("unreadable", e.to_string())],
            )
        }
    };
    match serde_json::from_str::<Value>(&text) {
        Ok(Value::Object(mut map)) => match map.remove(SETTINGS_KEY) {
            Some(v) => ViewSettings::from_json(&v),
            None => (ViewSettings::default(), Vec::new()),
        },
        Ok(_) | Err(_) => (
            ViewSettings::default(),
            vec![Issue::new("file_corrupt", "")],
        ),
    }
}

/// Changements de visibilité à faire pour atteindre la visibilité voulue, et nombre de calques
/// de la vue absents de la feuille. Les calques sont appariés par nom (casse comprise).
pub fn layer_changes(
    wanted: &[(String, bool)],
    layers: &[(LayerId, String, bool)],
) -> (Vec<(LayerId, bool)>, usize) {
    let mut changes = Vec::new();
    let mut missing = 0;
    for (name, visible) in wanted {
        let mut found = false;
        for (id, n, current) in layers {
            if n == name {
                found = true;
                if current != visible {
                    changes.push((*id, *visible));
                }
            }
        }
        if !found {
            missing += 1;
        }
    }
    (changes, missing)
}

/// Feuille d'une vue : même numéro **et** même nom, sinon la première du même nom.
pub fn find_sheet(view: &NamedView, sheets: &[(SheetId, String)]) -> Option<SheetId> {
    sheets
        .iter()
        .find(|(id, n)| id.0 == view.sheet_id && *n == view.sheet_name)
        .or_else(|| sheets.iter().find(|(_, n)| *n == view.sheet_name))
        .map(|(id, _)| *id)
}

// ----- règles ---------------------------------------------------------------------------------

/// Graduations principales possibles d'une règle (mm), du plus fin au plus large.
pub fn ruler_candidates_mm(unit: LengthUnit) -> Vec<f64> {
    let mut out = Vec::new();
    match unit {
        LengthUnit::Mm | LengthUnit::Cm => {
            for exp in -2..=7 {
                for m in [1.0, 2.0, 5.0] {
                    out.push(unit.to_mm(m * 10f64.powi(exp)));
                }
            }
        }
        LengthUnit::In => {
            for d in [16.0, 8.0, 4.0, 2.0] {
                out.push(MM_PER_INCH / d);
            }
            for exp in 0..=4 {
                for m in [1.0, 2.0, 5.0] {
                    out.push(MM_PER_INCH * m * 10f64.powi(exp));
                }
            }
        }
    }
    out
}

/// Première graduation principale dont l'écart à l'écran atteint 60 px (la plus large sinon).
pub fn ruler_major_mm(unit: LengthUnit, zoom: f64) -> f64 {
    let candidates = ruler_candidates_mm(unit);
    candidates
        .iter()
        .copied()
        .find(|s| s * zoom >= 60.0)
        .or_else(|| candidates.last().copied())
        .unwrap_or(1.0)
}

/// Étiquette d'une graduation principale (sans suffixe d'unité).
pub fn ruler_label(value_mm: f64, major_mm: f64, units: DisplayUnits) -> String {
    match units.unit {
        LengthUnit::Mm => {
            pf_core::input::format_number(value_mm, if major_mm < 1.0 { 2 } else { 0 })
        }
        LengthUnit::Cm => {
            pf_core::input::format_number(value_mm / 10.0, if major_mm < 10.0 { 2 } else { 0 })
        }
        LengthUnit::In => inches_to_fraction(value_mm / MM_PER_INCH, 16).0,
    }
}

/// Coordonnée du curseur dans l'unité d'affichage, sans suffixe (1 décimale en mm, 2 en cm,
/// 3 en pouces décimaux : la barre d'état n'est pas un champ de saisie).
pub fn cursor_text(mm: f64) -> String {
    let unit = display_units().unit;
    let decimals = match unit {
        LengthUnit::Mm => 1,
        LengthUnit::Cm => 2,
        LengthUnit::In => 3,
    };
    pf_core::input::format_number(unit.from_mm(mm), decimals)
}

/// Champ à glisser pour une longueur **en mm** : dans l'unité d'affichage (mm = inchangé). Le
/// texte d'édition est exact (pouces décimaux si la valeur n'est pas une fraction), de sorte
/// qu'une édition sans changement ne réinjecte aucun arrondi de fraction.
pub fn length_drag(drag: egui::DragValue<'_>) -> egui::DragValue<'_> {
    let units = display_units();
    if units.unit == LengthUnit::Mm {
        return drag;
    }
    drag.custom_formatter(move |mm, _| format_field(mm, units))
        .custom_parser(move |s| parse_length_with(s, units.unit).ok())
        .suffix(format!(" {}", units.unit.suffix()))
}

// ----- application -----------------------------------------------------------------------------

/// Publie l'unité d'affichage au fil courant : conversions des champs de longueur **et** repère
/// « (mm) » des libellés (`pf_i18n::set_length_unit_word`). Point d'entrée unique.
pub fn publish_units(units: DisplayUnits) {
    set_display_units(units);
    pf_i18n::set_length_unit_word(Some(display_units().unit.code()));
}

impl App {
    /// Réécrit dans la nouvelle unité les champs de longueur des formulaires en cours (texte
    /// relu dans l'ancienne unité, puis écrit exactement) ; une saisie illisible ou une formule
    /// est laissée telle quelle. Les modifications en cours dans un champ sont abandonnées (elles
    /// se relisent du modèle). Rien n'est écrit dans le projet.
    pub(crate) fn convert_form_lengths(&mut self, from: DisplayUnits, to: DisplayUnits) {
        use pf_core::units::convert_length_text;
        let one = |t: &str| convert_length_text(t, from, to);
        let list = |t: &str| {
            t.split(';')
                .map(|p| one(p.trim()))
                .collect::<Vec<_>>()
                .join("; ")
        };
        self.text_buf = None;
        self.topstitch_distance = one(&self.topstitch_distance);
        if let Some(f) = &mut self.button_form {
            for t in [&mut f.size, &mut f.length, &mut f.width, &mut f.margin] {
                *t = one(t);
            }
        }
        if let Some(f) = &mut self.pitch_form {
            for t in [
                &mut f.step,
                &mut f.distance,
                &mut f.start,
                &mut f.end,
                &mut f.diameter,
                &mut f.length,
                &mut f.width,
            ] {
                *t = one(t);
            }
        }
        if let Some(f) = &mut self.stitch_form {
            for t in [
                &mut f.pitch,
                &mut f.setback,
                &mut f.diameter,
                &mut f.length,
                &mut f.width,
            ] {
                *t = one(t);
            }
        }
        if let Some(f) = &mut self.zone_form {
            f.width = one(&f.width);
        }
        if let Some(d) = &mut self.background {
            for t in [&mut d.dist_text, &mut d.width_text, &mut d.height_text] {
                *t = one(t);
            }
        }
        if let Some(f) = &mut self.imposed_form {
            f.convert_lengths(&one);
        }
        if let Some(f) = &mut self.marking_form {
            f.size = one(&f.size);
            f.setback = one(&f.setback);
        }
        if let Some(f) = &mut self.derived_form {
            f.distance = one(&f.distance);
        }
        if let Some(d) = &mut self.print_dialog {
            for t in [
                &mut d.width,
                &mut d.height,
                &mut d.roll_width,
                &mut d.overlap,
            ] {
                *t = one(t);
            }
            for t in &mut d.margins {
                *t = one(t);
            }
        }
        let e = &mut self.editor;
        e.length_text = one(&e.length_text);
        e.trace.tolerance_text = one(&e.trace.tolerance_text);
        for t in [&mut e.pleat.depth, &mut e.pleat.span] {
            *t = one(t);
        }
        e.flare.positions = list(&e.flare.positions);
        e.flare.opening = list(&e.flare.opening);
        for t in [
            &mut e.cutline.ax,
            &mut e.cutline.ay,
            &mut e.cutline.bx,
            &mut e.cutline.by,
            &mut e.cutline.step,
        ] {
            *t = one(t);
        }
        for t in [
            &mut e.quick.x,
            &mut e.quick.y,
            &mut e.quick.width,
            &mut e.quick.height,
            &mut e.quick.radius,
        ] {
            *t = one(t);
        }
        e.quick.vertices = e
            .quick
            .vertices
            .split('\n')
            .map(list)
            .collect::<Vec<_>>()
            .join("\n");
        for t in [
            &mut e.seam.embu,
            &mut e.seam.tol,
            &mut e.seam.sel_embu,
            &mut e.seam.sel_tol,
        ] {
            *t = one(t);
        }
    }

    /// Publie l'unité d'affichage au fil d'exécution de l'interface et recopie les règles.
    pub(crate) fn apply_view_settings(&mut self) {
        let before = display_units();
        publish_units(self.view_settings.units);
        if display_units() != before {
            self.convert_form_lengths(before, display_units());
        }
        if self.editor.show_rulers != self.view_settings.rulers {
            self.view_settings.rulers = self.editor.show_rulers;
            self.save_view_settings();
        }
    }

    /// Enregistre les réglages d'affichage (écriture atomique) ; un échec est signalé.
    pub(crate) fn save_view_settings(&mut self) {
        let Some(path) = self.settings_path.clone() else {
            return;
        };
        if let Err(e) = settings::save_value(&path, SETTINGS_KEY, self.view_settings.to_json()) {
            self.editor
                .error(tr_args("views.save_error", &[("error", &e.to_string())]));
        }
    }

    /// Boîte englobante de la sélection de la feuille active (points, tracés).
    pub(crate) fn selection_bbox(&self) -> Option<pf_geometry::Bbox> {
        let sheet = self.editor.sheet();
        let sel = &self.editor.selection;
        let mut bb: Option<pf_geometry::Bbox> = None;
        let mut add = |p: PointMm| match bb.as_mut() {
            Some(b) => b.include(p),
            None => bb = Some(pf_geometry::Bbox::from_point(p)),
        };
        for id in &sel.points {
            if let Ok(p) = sheet.position(*id) {
                add(p);
            }
        }
        for id in &sel.entities {
            if let Ok(shape) = sheet.shape(*id) {
                let b = shape.bbox();
                add(b.min);
                add(b.max);
            }
        }
        bb
    }

    /// Cadre la sélection (appliqué à la prochaine image, quand la taille du canevas est connue).
    pub(crate) fn zoom_to_selection(&mut self) {
        match self.selection_bbox() {
            Some(bb) => self.zoom_target = Some(bb),
            None => self.editor.info(tr("views.zoom_selection.empty")),
        }
    }

    /// Capture le cadrage courant sous un nom.
    pub(crate) fn capture_view(&self, name: &str) -> NamedView {
        let sheet = self.editor.sheet();
        NamedView {
            name: name.to_string(),
            sheet_id: sheet.id.0,
            sheet_name: sheet.name.clone(),
            center: self.view.center,
            zoom: self.view.zoom,
            layers: sheet
                .layers
                .iter()
                .take(MAX_LAYERS_PER_VIEW)
                .map(|l| (l.name.clone(), l.visible))
                .collect(),
        }
    }

    /// Rétablit une vue nommée : feuille, calques (une étape annulable), cadrage et zoom. Refus
    /// sans rien changer si la feuille n'existe plus.
    pub(crate) fn restore_view(&mut self, index: usize) -> Result<(), Issue> {
        let view = self
            .view_settings
            .named
            .get(index)
            .cloned()
            .ok_or_else(|| Issue::new("view_missing", index.to_string()))?;
        let sheets: Vec<(SheetId, String)> = self
            .editor
            .project()
            .sheets
            .iter()
            .map(|s| (s.id, s.name.clone()))
            .collect();
        let sheet = find_sheet(&view, &sheets)
            .ok_or_else(|| Issue::new("sheet_missing", view.sheet_name.clone()))?;
        self.editor.set_sheet(sheet);
        let layers: Vec<(LayerId, String, bool)> = self
            .editor
            .sheet()
            .layers
            .iter()
            .map(|l| (l.id, l.name.clone(), l.visible))
            .collect();
        let (changes, missing) = layer_changes(&view.layers, &layers);
        if !changes.is_empty() {
            self.editor.doc.end_group();
            self.editor.doc.begin_group();
            for (layer, visible) in changes {
                let cmd = Command::SetLayerVisible {
                    sheet,
                    layer,
                    visible,
                };
                if let Err(e) = self.editor.doc.execute(cmd) {
                    self.editor.error(crate::text::core_error(&e));
                }
            }
            self.editor.doc.end_group();
            self.editor.refresh_display();
        }
        self.view.center = view.center;
        self.view.zoom = view.zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        self.fit_pending = false;
        self.zoom_target = None;
        let text = if missing > 0 {
            tr_args(
                "views.restored_missing",
                &[("name", &view.name), ("count", &missing.to_string())],
            )
        } else {
            tr_args("views.restored", &[("name", &view.name)])
        };
        self.editor.info(text);
        Ok(())
    }

    fn view_issue(&mut self, issue: Issue) {
        let text = issue.text();
        self.editor.error(text.clone());
        self.view_issue = Some(text);
    }

    /// Fenêtre « Vues, unités et grille ».
    pub fn views_window(&mut self, ctx: &egui::Context) {
        if !self.show_views {
            return;
        }
        let mut open = true;
        let mut vs = self.view_settings.clone();
        let muted = crate::palette::pal().muted;
        let mut action: Option<ViewAction> = None;
        egui::Window::new(tr("views.title"))
            .open(&mut open)
            .default_width(440.0)
            .show(ctx, |ui| {
                let room = crate::panels::window_room(ctx) - 70.0;
                egui::ScrollArea::vertical()
                    .id_salt("views_scroll")
                    .max_height(room)
                    .auto_shrink([true, true])
                    .show(ui, |ui| {
                        crate::a11y::label(
                            ui,
                            RichText::new(tr("views.note")).small().color(muted),
                        );
                        if let Some(text) = &self.view_issue {
                            crate::a11y::label(
                                ui,
                                RichText::new(text).color(crate::palette::pal().error),
                            );
                        }
                        ui.separator();
                        views_units_ui(ui, &mut vs);
                        ui.separator();
                        views_grid_ui(ui, &mut vs);
                        ui.separator();
                        action = views_named_ui(
                            ui,
                            &vs,
                            &mut self.view_name_draft,
                            &mut self.view_rename,
                            self.plan.is_none(),
                        );
                    });
            });
        if !open {
            self.show_views = false;
        }
        if vs != self.view_settings {
            let unit_changed = vs.units != self.view_settings.units;
            self.editor.show_rulers = vs.rulers;
            self.view_settings = vs;
            if unit_changed {
                // Un texte mémorisé a été saisi dans l'ancienne unité : ne pas le rejouer.
                self.editor.repeat_memo = None;
            }
            self.save_view_settings();
        }
        if let Some(action) = action {
            self.run_view_action(action);
        }
    }

    pub(crate) fn run_view_action(&mut self, action: ViewAction) {
        let result = match action {
            ViewAction::Save(name) => {
                let view = self.capture_view(&name);
                let r = self.view_settings.save_named(view);
                if r.is_ok() {
                    self.view_name_draft.clear();
                }
                r
            }
            ViewAction::Update(i) => {
                let name = self
                    .view_settings
                    .named
                    .get(i)
                    .map(|v| v.name.clone())
                    .unwrap_or_default();
                let view = self.capture_view(&name);
                self.view_settings.update_named(i, view)
            }
            ViewAction::Rename(i, name) => {
                let r = self.view_settings.rename_named(i, &name);
                if r.is_ok() {
                    self.view_rename = None;
                }
                r
            }
            ViewAction::Delete(i) => self.view_settings.delete_named(i).map(|_| ()),
            ViewAction::Restore(i) => {
                let r = self.restore_view(i);
                if r.is_ok() {
                    self.view_issue = None;
                }
                return match r {
                    Ok(()) => {}
                    Err(issue) => self.view_issue(issue),
                };
            }
        };
        match result {
            Ok(()) => {
                self.view_issue = None;
                self.save_view_settings();
            }
            Err(issue) => self.view_issue(issue),
        }
    }
}

pub(crate) enum ViewAction {
    Save(String),
    Update(usize),
    Rename(usize, String),
    Delete(usize),
    Restore(usize),
}

fn unit_name(unit: LengthUnit) -> String {
    tr(match unit {
        LengthUnit::Mm => "views.unit.mm",
        LengthUnit::Cm => "views.unit.cm",
        LengthUnit::In => "views.unit.in",
    })
}

fn views_units_ui(ui: &mut egui::Ui, vs: &mut ViewSettings) {
    use crate::a11y::ComboExt;
    egui::Grid::new("views-units")
        .num_columns(2)
        .show(ui, |ui| {
            crate::a11y::label(ui, tr("views.unit"));
            let mut unit = vs.units.unit;
            egui::ComboBox::from_id_salt("views-unit")
                .selected_text(unit_name(unit))
                .show_ui_named(ui, |ui| {
                    for u in LengthUnit::ALL {
                        ui.selectable_value(&mut unit, u, unit_name(u));
                    }
                });
            vs.set_unit(unit);
            ui.end_row();
            crate::a11y::label(ui, tr("views.precision"));
            let mut denominator = vs.units.inch_denominator;
            ui.add_enabled_ui(vs.units.unit == LengthUnit::In, |ui| {
                egui::ComboBox::from_id_salt("views-precision")
                    .selected_text(format!("1/{denominator}\""))
                    .show_ui_named(ui, |ui| {
                        for d in INCH_DENOMINATORS {
                            ui.selectable_value(&mut denominator, d, format!("1/{d}\""));
                        }
                    });
            });
            vs.units.inch_denominator = denominator;
            ui.end_row();
        });
    crate::a11y::label(
        ui,
        RichText::new(tr_args(
            "views.sample",
            &[
                ("a", &format_length(25.4, vs.units)),
                ("b", &format_length(26.9875, vs.units)),
            ],
        ))
        .small()
        .color(crate::palette::pal().muted),
    );
}

fn views_grid_ui(ui: &mut egui::Ui, vs: &mut ViewSettings) {
    ui.checkbox(&mut vs.grid.visible, tr("views.grid.visible"));
    ui.checkbox(&mut vs.rulers, tr("menu.view.rulers"));
    egui::Grid::new("views-grid").num_columns(2).show(ui, |ui| {
        crate::a11y::label(ui, tr("views.grid.step"));
        let mut step = vs.grid.step_mm;
        crate::a11y::add_drag(
            ui,
            length_drag(
                egui::DragValue::new(&mut step)
                    .range(GRID_STEP_MIN_MM..=GRID_STEP_MAX_MM)
                    .speed(0.5)
                    .max_decimals(2)
                    .suffix(" mm"),
            ),
        );
        vs.grid.step_mm = step;
        ui.end_row();
        crate::a11y::label(ui, tr("views.grid.subdivisions"));
        let mut n = vs.grid.subdivisions;
        crate::a11y::add_drag(
            ui,
            egui::DragValue::new(&mut n)
                .range(SUBDIVISIONS_MIN..=SUBDIVISIONS_MAX)
                .speed(0.1),
        );
        vs.grid.subdivisions = n;
        ui.end_row();
    });
    if ui.button(tr("views.grid.reset")).clicked() {
        vs.grid = GridSettings {
            visible: vs.grid.visible,
            ..GridSettings::default_for(vs.units.unit)
        };
    }
}

fn views_named_ui(
    ui: &mut egui::Ui,
    vs: &ViewSettings,
    draft: &mut String,
    rename: &mut Option<(usize, String)>,
    can_restore: bool,
) -> Option<ViewAction> {
    let mut action = None;
    crate::a11y::label(ui, RichText::new(tr("views.named")).strong());
    ui.horizontal(|ui| {
        crate::a11y::label(ui, tr("views.name"));
        crate::a11y::add_text(
            ui,
            egui::TextEdit::singleline(draft)
                .desired_width(160.0)
                .char_limit(NAME_MAX_CHARS * 2),
        );
        if ui.button(tr("views.save")).clicked() {
            action = Some(ViewAction::Save(draft.clone()));
        }
    });
    if vs.named.is_empty() {
        crate::a11y::label(
            ui,
            RichText::new(tr("views.none"))
                .small()
                .color(crate::palette::pal().muted),
        );
    }
    for (i, view) in vs.named.iter().enumerate() {
        ui.horizontal(|ui| {
            let editing = rename.as_ref().is_some_and(|(k, _)| *k == i);
            if editing {
                if let Some((_, text)) = rename.as_mut() {
                    crate::a11y::label(ui, tr("views.rename_to"));
                    crate::a11y::add_text(
                        ui,
                        egui::TextEdit::singleline(text).desired_width(120.0),
                    );
                    if ui.button(tr("views.rename_ok")).clicked() {
                        action = Some(ViewAction::Rename(i, text.clone()));
                    }
                }
                if ui.button(tr("views.rename_cancel")).clicked() {
                    *rename = None;
                }
            } else {
                crate::a11y::label(ui, &view.name);
                if ui
                    .add_enabled(can_restore, egui::Button::new(tr("views.restore")))
                    .clicked()
                {
                    action = Some(ViewAction::Restore(i));
                }
                if ui.button(tr("views.update")).clicked() {
                    action = Some(ViewAction::Update(i));
                }
                if ui.button(tr("views.rename")).clicked() {
                    *rename = Some((i, view.name.clone()));
                }
                if ui.button(tr("views.delete")).clicked() {
                    action = Some(ViewAction::Delete(i));
                }
            }
        });
    }
    action
}
