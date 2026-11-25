//! Table centrale des raccourcis, personnalisable (US-16.3).
//!
//! Elle liste **toutes** les commandes à raccourci : les outils (une lettre A à Z, une lettre ne
//! sert qu'à un outil) et les commandes de [`Cmd`] (Ctrl+…, touches de fonction, Suppr, Début).
//! Entrée (répéter / valider) et Échap (annuler / fermer) sont fixes : leur rôle dépend du
//! contexte. Les commandes de menu sans raccourci par défaut ne sont pas dans la table.

use crate::editor::Tool;
use eframe::egui::{Key, KeyboardShortcut, Modifiers};
use serde_json::{json, Map, Value};
use std::cell::RefCell;

/// Clé du réglage dans `settings.json`.
pub const SETTINGS_KEY: &str = "shortcuts";

/// Répéter la dernière commande (US-16.2, fixe, hors table des outils) : Entrée
/// sans modificateur, seulement quand l'outil Sélection est actif, qu'aucune
/// étape n'est en cours et qu'aucun champ ni dialogue ne reçoit le clavier.
/// Espace n'est pas retenu : il sert au panoramique (Espace + glisser).
pub const REPEAT_LAST: Key = Key::Enter;

/// Lettres autorisées.
pub const LETTERS: [(char, Key); 26] = [
    ('A', Key::A),
    ('B', Key::B),
    ('C', Key::C),
    ('D', Key::D),
    ('E', Key::E),
    ('F', Key::F),
    ('G', Key::G),
    ('H', Key::H),
    ('I', Key::I),
    ('J', Key::J),
    ('K', Key::K),
    ('L', Key::L),
    ('M', Key::M),
    ('N', Key::N),
    ('O', Key::O),
    ('P', Key::P),
    ('Q', Key::Q),
    ('R', Key::R),
    ('S', Key::S),
    ('T', Key::T),
    ('U', Key::U),
    ('V', Key::V),
    ('W', Key::W),
    ('X', Key::X),
    ('Y', Key::Y),
    ('Z', Key::Z),
];

/// Lettre affichable d'une touche autorisée.
pub fn letter_of(key: Key) -> Option<char> {
    LETTERS.iter().find(|(_, k)| *k == key).map(|(c, _)| *c)
}

fn key_of(letter: char) -> Option<Key> {
    LETTERS
        .iter()
        .find(|(c, _)| *c == letter.to_ascii_uppercase())
        .map(|(_, k)| *k)
}

/// Identifiant stable d'un outil dans le fichier de réglages.
pub fn tool_id(tool: Tool) -> &'static str {
    match tool {
        Tool::Select => "select",
        Tool::Point => "point",
        Tool::Segment => "segment",
        Tool::Rectangle => "rectangle",
        Tool::Perpendicular => "perpendicular",
        Tool::Parallel => "parallel",
        Tool::PointOnSegment => "point_on_segment",
        Tool::Notch => "notch",
        Tool::Grainline => "grainline",
        Tool::Curve => "curve",
        Tool::Spline => "spline",
        Tool::Circle => "circle",
        Tool::Arc => "arc",
        Tool::Fillet => "fillet",
        Tool::Chamfer => "chamfer",
        Tool::Tangent => "tangent",
        Tool::Extend => "extend",
        Tool::Split => "split",
        Tool::Merge => "merge",
        Tool::Offset => "offset",
        Tool::Measure => "measure",
        Tool::Dimension => "dimension",
        Tool::Guide => "guide",
        Tool::ExtractPiece => "extract_piece",
        Tool::Dart => "dart",
        Tool::Trace => "trace",
    }
}

fn default_letter(tool: Tool) -> char {
    match tool {
        Tool::Select => 'V',
        Tool::Point => 'O',
        Tool::Segment => 'S',
        Tool::Rectangle => 'R',
        Tool::Perpendicular => 'P',
        Tool::Parallel => 'L',
        Tool::PointOnSegment => 'D',
        Tool::Notch => 'N',
        Tool::Grainline => 'G',
        Tool::Curve => 'C',
        Tool::Spline => 'B',
        Tool::Circle => 'E',
        Tool::Arc => 'A',
        Tool::Fillet => 'F',
        Tool::Chamfer => 'H',
        Tool::Tangent => 'T',
        Tool::Extend => 'I',
        Tool::Split => 'K',
        Tool::Merge => 'J',
        Tool::Offset => 'X',
        Tool::Measure => 'M',
        Tool::Dimension => 'Q',
        Tool::Guide => 'U',
        Tool::ExtractPiece => 'Z',
        Tool::Dart => 'Y',
        // Aucun raccourci par defaut : les 26 lettres sont prises (menu ou barre d'outils).
        Tool::Trace => '\0',
    }
}

/// Refus d'une affectation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssignError {
    /// Touche hors A–Z.
    NotALetter,
    /// Lettre déjà utilisée par cet autre outil.
    Taken(Tool),
}

/// Lettre de chaque outil, dans l'ordre de `Tool::ALL`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolKeys {
    letters: Vec<(Tool, char)>,
}

impl Default for ToolKeys {
    fn default() -> Self {
        ToolKeys {
            letters: Tool::ALL.iter().map(|t| (*t, default_letter(*t))).collect(),
        }
    }
}

impl ToolKeys {
    pub fn letter(&self, tool: Tool) -> char {
        self.letters
            .iter()
            .find(|(t, _)| *t == tool)
            .map_or('?', |(_, c)| *c)
    }

    /// Outil associé à une touche, s'il y en a un.
    pub fn tool_for(&self, key: Key) -> Option<Tool> {
        let letter = letter_of(key)?;
        self.letters
            .iter()
            .find(|(_, c)| *c == letter)
            .map(|(t, _)| *t)
    }

    /// Affecte une touche à un outil ; refuse une lettre déjà prise.
    pub fn assign(&mut self, tool: Tool, key: Key) -> Result<(), AssignError> {
        let letter = letter_of(key).ok_or(AssignError::NotALetter)?;
        if let Some(other) = self.tool_for(key) {
            if other != tool {
                return Err(AssignError::Taken(other));
            }
        }
        if let Some(entry) = self.letters.iter_mut().find(|(t, _)| *t == tool) {
            entry.1 = letter;
        }
        Ok(())
    }

    /// Charge depuis le réglage : toute entrée invalide, inconnue ou en
    /// doublon est ignorée (l'outil garde sa lettre par défaut).
    pub fn from_json(value: &Value) -> ToolKeys {
        let mut keys = ToolKeys::default();
        let Some(map) = value.as_object() else {
            return keys;
        };
        // Deux passes : d'abord libérer les lettres par défaut des outils
        // redéfinis, sinon un échange (A↔B) serait refusé à tort.
        let unbound: Vec<Tool> = Tool::ALL
            .iter()
            .copied()
            .filter(|t| map.get(tool_id(*t)).is_some_and(Value::is_null))
            .collect();
        let wanted: Vec<(Tool, char)> = Tool::ALL
            .iter()
            .filter_map(|t| {
                let s = map.get(tool_id(*t))?.as_str()?;
                let mut chars = s.chars();
                let c = chars.next()?;
                if chars.next().is_some() {
                    return None;
                }
                key_of(c)?;
                Some((*t, c.to_ascii_uppercase()))
            })
            .collect();
        let mut used: Vec<char> = Vec::new();
        let valid: Vec<(Tool, char)> = wanted
            .into_iter()
            .filter(|(_, c)| {
                if used.contains(c) {
                    false
                } else {
                    used.push(*c);
                    true
                }
            })
            .collect();
        let redefined: Vec<Tool> = valid.iter().map(|(t, _)| *t).collect();
        // Les outils non redéfinis dont la lettre par défaut est reprise
        // perdent leur raccourci par défaut… sauf s'ils peuvent le garder.
        for (tool, c) in &valid {
            if let Some(entry) = keys.letters.iter_mut().find(|(t, _)| t == tool) {
                entry.1 = *c;
            }
        }
        // Conflit résiduel (outil non redéfini dont la lettre est reprise) :
        // la redéfinition explicite l'emporte, on retire l'ancienne.
        let mut seen: Vec<char> = Vec::new();
        for (tool, c) in &mut keys.letters {
            if redefined.contains(tool) {
                seen.push(*c);
            }
        }
        for (tool, c) in &mut keys.letters {
            if !redefined.contains(tool) && seen.contains(c) {
                *c = '\0';
            }
        }
        for tool in unbound {
            keys.unbind(tool);
        }
        keys
    }

    /// Sérialise uniquement ce qui diffère des valeurs par défaut.
    pub fn to_json(&self) -> Value {
        let mut map = Map::new();
        for (tool, c) in &self.letters {
            if *c == default_letter(*tool) {
                continue;
            }
            if *c == '\0' {
                map.insert(tool_id(*tool).to_string(), Value::Null);
            } else {
                map.insert(tool_id(*tool).to_string(), json!(c.to_string()));
            }
        }
        Value::Object(map)
    }

    /// Retire le raccourci d'un outil.
    pub fn unbind(&mut self, tool: Tool) {
        if let Some(entry) = self.letters.iter_mut().find(|(t, _)| *t == tool) {
            entry.1 = '\0';
        }
    }

    /// Texte du raccourci d'un outil (vide si le raccourci a été retiré).
    pub fn label(&self, tool: Tool) -> String {
        match self.letter(tool) {
            '\0' => String::new(),
            c => c.to_string(),
        }
    }
}

thread_local! {
    static CURRENT: RefCell<ToolKeys> = RefCell::new(ToolKeys::default());
    static CURRENT_CMDS: RefCell<CmdKeys> = RefCell::new(CmdKeys::default());
}

/// Table courante (par fil d'exécution, comme la langue).
pub fn current() -> ToolKeys {
    CURRENT.with(|c| c.borrow().clone())
}

pub fn set_current(keys: ToolKeys) {
    CURRENT.with(|c| *c.borrow_mut() = keys);
}

/// Raccourcis courants des commandes hors outils.
pub fn current_cmds() -> CmdKeys {
    CURRENT_CMDS.with(|c| c.borrow().clone())
}

pub fn set_current_cmds(keys: CmdKeys) {
    CURRENT_CMDS.with(|c| *c.borrow_mut() = keys);
}

// ---------------------------------------------------------------------------------------------
// US-16.3 : table centrale de **toutes** les commandes à raccourci (outils compris).
// ---------------------------------------------------------------------------------------------

/// Combinaison de touches : Ctrl (Commande sur Mac), Maj éventuelle, une touche.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Binding {
    pub ctrl: bool,
    pub shift: bool,
    pub key: Key,
}

/// Touches nommées permises pour une commande personnalisable.
const NAMED_KEYS: [(&str, Key); 21] = [
    ("F1", Key::F1),
    ("F2", Key::F2),
    ("F3", Key::F3),
    ("F4", Key::F4),
    ("F5", Key::F5),
    ("F6", Key::F6),
    ("F7", Key::F7),
    ("F8", Key::F8),
    ("F9", Key::F9),
    ("F10", Key::F10),
    ("F11", Key::F11),
    ("F12", Key::F12),
    ("Delete", Key::Delete),
    ("Home", Key::Home),
    ("End", Key::End),
    ("PageUp", Key::PageUp),
    ("PageDown", Key::PageDown),
    ("Enter", Key::Enter),
    ("Escape", Key::Escape),
    ("Tab", Key::Tab),
    ("Backspace", Key::Backspace),
];

const DIGITS: [(&str, Key); 10] = [
    ("0", Key::Num0),
    ("1", Key::Num1),
    ("2", Key::Num2),
    ("3", Key::Num3),
    ("4", Key::Num4),
    ("5", Key::Num5),
    ("6", Key::Num6),
    ("7", Key::Num7),
    ("8", Key::Num8),
    ("9", Key::Num9),
];

/// Nom stable d'une touche (fichier d'export) ; `None` si elle n'est pas gérée.
pub fn key_name(key: Key) -> Option<String> {
    if let Some(c) = letter_of(key) {
        return Some(c.to_string());
    }
    DIGITS
        .iter()
        .chain(NAMED_KEYS.iter())
        .find(|(_, k)| *k == key)
        .map(|(n, _)| (*n).to_string())
}

fn key_from_name(name: &str) -> Option<Key> {
    let mut chars = name.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        if c.is_ascii_uppercase() {
            return key_of(c);
        }
    }
    DIGITS
        .iter()
        .chain(NAMED_KEYS.iter())
        .find(|(n, _)| *n == name)
        .map(|(_, k)| *k)
}

/// Raison de refus d'une combinaison ou d'une affectation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindError {
    /// Texte de combinaison illisible.
    Unreadable,
    /// Lettre ou chiffre sans Ctrl : réservé à la saisie et aux outils.
    NeedsCtrl,
    /// Touche non gérée (réservée : Entrée, Échap, Tab, Retour arrière).
    Reserved,
    /// Déjà utilisée par cette commande.
    Conflict(Cmd),
    /// Commande fixe : non modifiable.
    Fixed(Cmd),
}

impl Binding {
    pub const fn ctrl(key: Key) -> Binding {
        Binding {
            ctrl: true,
            shift: false,
            key,
        }
    }

    pub const fn ctrl_shift(key: Key) -> Binding {
        Binding {
            ctrl: true,
            shift: true,
            key,
        }
    }

    pub const fn bare(key: Key) -> Binding {
        Binding {
            ctrl: false,
            shift: false,
            key,
        }
    }

    /// Texte stable, par exemple `Ctrl+Shift+S` ; jamais traduit (fichier d'export).
    pub fn text(self) -> String {
        let mut out = String::new();
        if self.ctrl {
            out.push_str("Ctrl+");
        }
        if self.shift {
            out.push_str("Shift+");
        }
        out.push_str(&key_name(self.key).unwrap_or_else(|| "?".into()));
        out
    }

    /// Texte affiché (Maj traduit).
    pub fn display(self) -> String {
        self.text().replace("Shift", &pf_i18n::tr("key.shift"))
    }

    /// Lit `Ctrl+Shift+S`, `F2`, `Delete`… (casse des modificateurs libre, touche exacte).
    pub fn parse(text: &str) -> Result<Binding, BindError> {
        let parts: Vec<&str> = text.split('+').collect();
        let Some((key_text, mods)) = parts.split_last() else {
            return Err(BindError::Unreadable);
        };
        let (mut ctrl, mut shift) = (false, false);
        for m in mods {
            match m.to_ascii_lowercase().as_str() {
                "ctrl" if !ctrl => ctrl = true,
                "shift" if !shift => shift = true,
                _ => return Err(BindError::Unreadable),
            }
        }
        let key = key_from_name(key_text).ok_or(BindError::Unreadable)?;
        Ok(Binding { ctrl, shift, key })
    }

    /// Permise pour une commande personnalisable ?
    pub fn allowed(self) -> Result<(), BindError> {
        if RESERVED_KEYS.contains(&self.key) {
            return Err(BindError::Reserved);
        }
        let typing = letter_of(self.key).is_some() || DIGITS.iter().any(|(_, k)| *k == self.key);
        if typing && !self.ctrl {
            return Err(BindError::NeedsCtrl);
        }
        Ok(())
    }

    /// Touches pressées cette image ? Sans Ctrl ni Maj : touche seule, aucun modificateur,
    /// non consommée ; sinon le raccourci est consommé (comme avant le passage par la table).
    pub fn pressed(self, ctx: &eframe::egui::Context) -> bool {
        if !self.ctrl && !self.shift {
            return ctx.input(|i| i.key_pressed(self.key) && i.modifiers.is_none());
        }
        let mut mods = Modifiers::NONE;
        if self.ctrl {
            mods = mods.plus(Modifiers::COMMAND);
        }
        if self.shift {
            mods = mods.plus(Modifiers::SHIFT);
        }
        ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(mods, self.key)))
    }
}

/// Touches jamais attribuables : validation, annulation, navigation, effacement.
const RESERVED_KEYS: [Key; 4] = [Key::Enter, Key::Escape, Key::Tab, Key::Backspace];

/// Quand une commande est active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Guard {
    /// Toujours (même pendant une saisie).
    Always,
    /// Hors saisie de texte, sans boîte « enregistrer ? » ni plan de pièces.
    Canvas,
    /// Touche nue : hors saisie de texte et boîte modale ouverte.
    Bare,
}

macro_rules! commands {
    ($($variant:ident, $id:literal, $default:expr, $fixed:literal, $guard:ident;)*) => {
        /// Commande de l'interface hors outils (les outils sont dans [`ToolKeys`]).
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum Cmd { $($variant),* }

        impl Cmd {
            /// Toutes les commandes de la table, dans l'ordre d'affichage.
            pub const ALL: &'static [Cmd] = &[$(Cmd::$variant),*];

            /// Identifiant stable (`snake_case` pointé), jamais renommé.
            pub fn id(self) -> &'static str {
                match self { $(Cmd::$variant => $id),* }
            }

            /// Raccourci par défaut (`None` : commande sans raccourci par défaut).
            pub fn default_binding(self) -> Option<Binding> {
                match self { $(Cmd::$variant => $default),* }
            }

            /// Non modifiable (Entrée et Échap ont des rôles contextuels).
            pub fn fixed(self) -> bool {
                match self { $(Cmd::$variant => $fixed),* }
            }

            pub fn guard(self) -> Guard {
                match self { $(Cmd::$variant => Guard::$guard),* }
            }
        }
    };
}

commands! {
    New, "file.new", Some(Binding::ctrl(Key::N)), false, Always;
    Open, "file.open", Some(Binding::ctrl(Key::O)), false, Always;
    Save, "file.save", Some(Binding::ctrl(Key::S)), false, Always;
    SaveAs, "file.save_as", Some(Binding::ctrl_shift(Key::S)), false, Always;
    Undo, "edit.undo", Some(Binding::ctrl(Key::Z)), false, Always;
    Redo, "edit.redo", Some(Binding::ctrl(Key::Y)), false, Always;
    Find, "edit.find", Some(Binding::ctrl(Key::F)), false, Always;
    Group, "edit.group", Some(Binding::ctrl(Key::G)), false, Always;
    Ungroup, "edit.ungroup", Some(Binding::ctrl_shift(Key::G)), false, Always;
    Cut, "edit.cut", Some(Binding::ctrl(Key::X)), false, Canvas;
    Copy, "edit.copy", Some(Binding::ctrl(Key::C)), false, Canvas;
    Paste, "edit.paste", Some(Binding::ctrl(Key::V)), false, Canvas;
    SelectAll, "select.all", Some(Binding::ctrl(Key::A)), false, Canvas;
    SelectChain, "select.chain", Some(Binding::ctrl_shift(Key::A)), false, Canvas;
    InvertSelection, "select.invert", Some(Binding::ctrl(Key::I)), false, Canvas;
    ToggleLasso, "select.lasso", Some(Binding::ctrl(Key::L)), false, Canvas;
    Delete, "edit.delete", Some(Binding::bare(Key::Delete)), false, Bare;
    FitView, "view.fit", Some(Binding::bare(Key::Home)), false, Bare;
    RenameSheet, "sheet.rename", Some(Binding::bare(Key::F2)), false, Bare;
    Help, "help.shortcuts", Some(Binding::bare(Key::F1)), false, Bare;
    RepeatLast, "edit.repeat_last", Some(Binding::bare(Key::Enter)), true, Bare;
    CancelClose, "edit.cancel", Some(Binding::bare(Key::Escape)), true, Bare;
    ZoomSelection, "view.zoom_selection", Some(Binding::bare(Key::End)), false, Bare;
}

impl Cmd {
    pub fn from_id(id: &str) -> Option<Cmd> {
        Cmd::ALL.iter().copied().find(|c| c.id() == id)
    }

    /// Libellé dans la langue courante (clé `cmd.<id>`).
    pub fn label(self) -> String {
        match self {
            Cmd::New => pf_i18n::tr("cmd.file.new"),
            Cmd::Open => pf_i18n::tr("cmd.file.open"),
            Cmd::Save => pf_i18n::tr("cmd.file.save"),
            Cmd::SaveAs => pf_i18n::tr("cmd.file.save_as"),
            Cmd::Undo => pf_i18n::tr("cmd.edit.undo"),
            Cmd::Redo => pf_i18n::tr("cmd.edit.redo"),
            Cmd::Find => pf_i18n::tr("cmd.edit.find"),
            Cmd::Group => pf_i18n::tr("cmd.edit.group"),
            Cmd::Ungroup => pf_i18n::tr("cmd.edit.ungroup"),
            Cmd::Cut => pf_i18n::tr("cmd.edit.cut"),
            Cmd::Copy => pf_i18n::tr("cmd.edit.copy"),
            Cmd::Paste => pf_i18n::tr("cmd.edit.paste"),
            Cmd::SelectAll => pf_i18n::tr("cmd.select.all"),
            Cmd::SelectChain => pf_i18n::tr("cmd.select.chain"),
            Cmd::InvertSelection => pf_i18n::tr("cmd.select.invert"),
            Cmd::ToggleLasso => pf_i18n::tr("cmd.select.lasso"),
            Cmd::Delete => pf_i18n::tr("cmd.edit.delete"),
            Cmd::FitView => pf_i18n::tr("cmd.view.fit"),
            Cmd::ZoomSelection => pf_i18n::tr("cmd.view.zoom_selection"),
            Cmd::RenameSheet => pf_i18n::tr("cmd.sheet.rename"),
            Cmd::Help => pf_i18n::tr("cmd.help.shortcuts"),
            Cmd::RepeatLast => pf_i18n::tr("cmd.edit.repeat_last"),
            Cmd::CancelClose => pf_i18n::tr("cmd.edit.cancel"),
        }
    }
}

/// Cible d'une affectation : un outil (lettre seule) ou une commande.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Tool(Tool),
    Cmd(Cmd),
}

/// Raccourcis des commandes hors outils : `None` = retiré.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CmdKeys {
    slots: Vec<(Cmd, Option<Binding>)>,
}

impl Default for CmdKeys {
    fn default() -> Self {
        CmdKeys {
            slots: Cmd::ALL.iter().map(|c| (*c, c.default_binding())).collect(),
        }
    }
}

impl CmdKeys {
    pub fn get(&self, cmd: Cmd) -> Option<Binding> {
        self.slots
            .iter()
            .find(|(c, _)| *c == cmd)
            .and_then(|(_, b)| *b)
    }

    /// Commande qui porte déjà cette combinaison, y compris les combinaisons réservées
    /// (`Ctrl+Maj+Z` : second raccourci de « Rétablir »).
    pub fn owner(&self, binding: Binding) -> Option<Cmd> {
        if binding == REDO_ALIAS {
            return Some(Cmd::Redo);
        }
        self.slots
            .iter()
            .find(|(_, b)| *b == Some(binding))
            .map(|(c, _)| *c)
    }

    /// Affecte (ou retire, avec `None`) la combinaison d'une commande ; refus clair : nom de
    /// la commande en conflit, jamais d'écrasement silencieux.
    pub fn assign(&mut self, cmd: Cmd, binding: Option<Binding>) -> Result<(), BindError> {
        if cmd.fixed() {
            return Err(BindError::Fixed(cmd));
        }
        if let Some(b) = binding {
            b.allowed()?;
            if let Some(other) = self.owner(b) {
                if other != cmd {
                    return Err(BindError::Conflict(other));
                }
            }
        }
        if let Some(slot) = self.slots.iter_mut().find(|(c, _)| *c == cmd) {
            slot.1 = binding;
        }
        Ok(())
    }

    /// Valeurs différentes des valeurs par défaut, par identifiant (`null` = retiré).
    pub fn to_json(&self) -> Map<String, Value> {
        let mut map = Map::new();
        for (cmd, b) in &self.slots {
            if *b != cmd.default_binding() {
                map.insert(
                    cmd.id().to_string(),
                    b.map_or(Value::Null, |b| json!(b.text())),
                );
            }
        }
        map
    }

    /// Lecture **tolérante** des réglages de l'application : toute entrée invalide, inconnue ou
    /// en conflit est ignorée (la commande garde sa valeur par défaut). Compte les écartées.
    pub fn from_settings(value: &Value) -> (CmdKeys, usize) {
        let mut keys = CmdKeys::default();
        let Some(map) = value.as_object() else {
            return (keys, 0);
        };
        let mut ignored = 0;
        let mut wanted: Vec<(Cmd, Option<Binding>)> = Vec::new();
        for (id, v) in map {
            let Some(cmd) = Cmd::from_id(id) else {
                continue;
            };
            let binding = match v {
                Value::Null => Some(None),
                Value::String(s) => Binding::parse(s).ok().map(Some),
                _ => None,
            };
            match binding {
                Some(b) if !cmd.fixed() => wanted.push((cmd, b)),
                _ => ignored += 1,
            }
        }
        // Les cases visées sont libérées d'abord : un échange A↔B ne doit pas être refusé à tort.
        for (cmd, _) in &wanted {
            if let Some(slot) = keys.slots.iter_mut().find(|(c, _)| c == cmd) {
                slot.1 = None;
            }
        }
        for (cmd, b) in &wanted {
            if keys.assign(*cmd, *b).is_err() {
                ignored += 1;
                let d = cmd.default_binding();
                if d.is_some_and(|d| keys.owner(d).is_none()) {
                    let _ = keys.assign(*cmd, d);
                }
            }
        }
        (keys, ignored)
    }
}

/// Second raccourci de « Rétablir » (toujours actif, non modifiable).
pub const REDO_ALIAS: Binding = Binding::ctrl_shift(Key::Z);

/// Jeux prédéfinis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Preset {
    /// Les raccourcis par défaut.
    PatternForge,
    /// Seulement l'essentiel : fichier, annuler / rétablir, presse-papiers, quelques outils.
    Minimal,
}

impl Preset {
    pub const ALL: [Preset; 2] = [Preset::PatternForge, Preset::Minimal];

    pub fn label(self) -> String {
        match self {
            Preset::PatternForge => pf_i18n::tr("shortcuts.preset.patternforge"),
            Preset::Minimal => pf_i18n::tr("shortcuts.preset.minimal"),
        }
    }

    /// Outils conservés par le jeu « minimal ».
    const MINIMAL_TOOLS: [Tool; 6] = [
        Tool::Select,
        Tool::Point,
        Tool::Segment,
        Tool::Rectangle,
        Tool::Curve,
        Tool::Measure,
    ];
    const MINIMAL_CMDS: [Cmd; 11] = [
        Cmd::New,
        Cmd::Open,
        Cmd::Save,
        Cmd::SaveAs,
        Cmd::Undo,
        Cmd::Redo,
        Cmd::Cut,
        Cmd::Copy,
        Cmd::Paste,
        Cmd::Delete,
        Cmd::Help,
    ];

    pub fn keymap(self) -> Keymap {
        match self {
            Preset::PatternForge => Keymap::default(),
            Preset::Minimal => {
                let mut km = Keymap::default();
                for tool in Tool::ALL {
                    if !Preset::MINIMAL_TOOLS.contains(&tool) {
                        km.tools.unbind(tool);
                    }
                }
                for cmd in Cmd::ALL {
                    if !cmd.fixed() && !Preset::MINIMAL_CMDS.contains(cmd) {
                        let _ = km.cmds.assign(*cmd, None);
                    }
                }
                km
            }
        }
    }
}

/// Marque et version du fichier d'export des raccourcis.
pub const FILE_FORMAT: &str = "patternforge-shortcuts";
pub const FILE_VERSION: u64 = 1;

/// Refus d'un fichier de raccourcis (rien n'est changé).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileError {
    NotJson(String),
    NotAnObject,
    WrongFormat,
    BadVersion,
    UnknownField(String),
    UnknownTool(String),
    UnknownCommand(String),
    BadValue(String),
    Conflict { command: String, other: String },
}

/// Raccourcis complets : outils (lettre) et commandes.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Keymap {
    pub tools: ToolKeys,
    pub cmds: CmdKeys,
}

impl Keymap {
    /// Fichier complet, versionné, lisible (toutes les entrées, pas seulement les écarts).
    pub fn export_text(&self) -> String {
        let mut tools = Map::new();
        for tool in Tool::ALL {
            let letter = self.tools.letter(tool);
            tools.insert(
                tool_id(tool).to_string(),
                if letter == '\0' {
                    Value::Null
                } else {
                    json!(letter.to_string())
                },
            );
        }
        let mut cmds = Map::new();
        for cmd in Cmd::ALL {
            cmds.insert(
                cmd.id().to_string(),
                self.cmds.get(*cmd).map_or(Value::Null, |b| json!(b.text())),
            );
        }
        let doc = json!({
            "format": FILE_FORMAT,
            "version": FILE_VERSION,
            "tools": tools,
            "commands": cmds,
        });
        let mut text = serde_json::to_string_pretty(&doc).unwrap_or_default();
        text.push('\n');
        text
    }

    /// Lecture **stricte** : tout champ inconnu, valeur invalide ou conflit refuse le fichier
    /// entier (jamais de perte silencieuse). Les entrées absentes valent leur défaut.
    pub fn import_text(text: &str) -> Result<Keymap, FileError> {
        let value: Value =
            serde_json::from_str(text).map_err(|e| FileError::NotJson(e.to_string()))?;
        let Value::Object(map) = value else {
            return Err(FileError::NotAnObject);
        };
        if let Some(k) = map
            .keys()
            .find(|k| !["format", "version", "tools", "commands"].contains(&k.as_str()))
        {
            return Err(FileError::UnknownField(k.clone()));
        }
        if map.get("format").and_then(Value::as_str) != Some(FILE_FORMAT) {
            return Err(FileError::WrongFormat);
        }
        if map.get("version").and_then(Value::as_u64) != Some(FILE_VERSION) {
            return Err(FileError::BadVersion);
        }
        let mut km = Keymap::default();
        // Tout est d'abord retiré, puis ce que le fichier dit est posé : seul le fichier compte.
        for tool in Tool::ALL {
            km.tools.unbind(tool);
        }
        for cmd in Cmd::ALL {
            if !cmd.fixed() {
                let _ = km.cmds.assign(*cmd, None);
            }
        }
        let mut seen_tools = Vec::new();
        if let Some(tools) = map.get("tools") {
            let tools = tools
                .as_object()
                .ok_or_else(|| FileError::BadValue("tools".into()))?;
            for (id, v) in tools {
                let tool = Tool::ALL
                    .iter()
                    .copied()
                    .find(|t| tool_id(*t) == id)
                    .ok_or_else(|| FileError::UnknownTool(id.clone()))?;
                seen_tools.push(tool);
                match v {
                    Value::Null => {}
                    Value::String(s) => {
                        let mut chars = s.chars();
                        let key = match (chars.next(), chars.next()) {
                            (Some(c), None) if c.is_ascii_uppercase() => key_of(c),
                            _ => None,
                        }
                        .ok_or_else(|| FileError::BadValue(id.clone()))?;
                        km.tools.assign(tool, key).map_err(|e| match e {
                            AssignError::Taken(other) => FileError::Conflict {
                                command: tool_id(tool).into(),
                                other: tool_id(other).into(),
                            },
                            AssignError::NotALetter => FileError::BadValue(id.clone()),
                        })?;
                    }
                    _ => return Err(FileError::BadValue(id.clone())),
                }
            }
        }
        // Un outil absent du fichier garde sa lettre par défaut s'il peut (jamais de conflit).
        for tool in Tool::ALL {
            if !seen_tools.contains(&tool) {
                if let Some(key) = key_of(default_letter(tool)) {
                    km.tools
                        .assign(tool, key)
                        .map_err(|_| FileError::Conflict {
                            command: tool_id(tool).into(),
                            other: "tools".into(),
                        })?;
                }
            }
        }
        let mut seen_cmds = Vec::new();
        if let Some(cmds) = map.get("commands") {
            let cmds = cmds
                .as_object()
                .ok_or_else(|| FileError::BadValue("commands".into()))?;
            for (id, v) in cmds {
                let cmd = Cmd::from_id(id).ok_or_else(|| FileError::UnknownCommand(id.clone()))?;
                seen_cmds.push(cmd);
                let binding = match v {
                    Value::Null => None,
                    Value::String(s) => {
                        Some(Binding::parse(s).map_err(|_| FileError::BadValue(id.clone()))?)
                    }
                    _ => return Err(FileError::BadValue(id.clone())),
                };
                if cmd.fixed() {
                    if binding != cmd.default_binding() {
                        return Err(FileError::BadValue(id.clone()));
                    }
                    continue;
                }
                km.cmds.assign(cmd, binding).map_err(|e| match e {
                    BindError::Conflict(other) => FileError::Conflict {
                        command: id.clone(),
                        other: other.id().into(),
                    },
                    _ => FileError::BadValue(id.clone()),
                })?;
            }
        }
        for cmd in Cmd::ALL {
            if !cmd.fixed() && !seen_cmds.contains(cmd) {
                if let Some(d) = cmd.default_binding() {
                    if km.cmds.assign(*cmd, Some(d)).is_err() {
                        return Err(FileError::Conflict {
                            command: cmd.id().into(),
                            other: km.cmds.owner(d).map_or("?", Cmd::id).into(),
                        });
                    }
                }
            }
        }
        Ok(km)
    }

    /// Valeurs écrites dans le réglage `shortcuts` : outils et commandes, seulement les écarts.
    pub fn to_settings(&self) -> Value {
        let mut map = match self.tools.to_json() {
            Value::Object(m) => m,
            _ => Map::new(),
        };
        map.extend(self.cmds.to_json());
        Value::Object(map)
    }

    /// Lecture tolérante du réglage `shortcuts`.
    pub fn from_settings(value: &Value) -> Keymap {
        Keymap {
            tools: ToolKeys::from_json(value),
            cmds: CmdKeys::from_settings(value).0,
        }
    }
}

/// Raccourcis courants complets (outils et commandes).
pub fn current_keymap() -> Keymap {
    Keymap {
        tools: current(),
        cmds: current_cmds(),
    }
}

pub fn set_current_keymap(km: Keymap) {
    set_current(km.tools);
    set_current_cmds(km.cmds);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_sans_doublon() {
        let k = ToolKeys::default();
        let mut seen = Vec::new();
        for t in Tool::ALL {
            let c = k.letter(t);
            if t == Tool::Trace {
                // Les 26 lettres sont prises : l'outil Decalque n'a aucun raccourci par defaut.
                assert_eq!(c, '\0');
                continue;
            }
            assert!(!seen.contains(&c), "doublon {c}");
            seen.push(c);
            assert_eq!(k.tool_for(key_of(c).unwrap()), Some(t));
        }
        assert_eq!(k.to_json(), json!({}));
    }

    #[test]
    fn affectation_et_conflit() {
        let mut k = ToolKeys::default();
        assert_eq!(k.assign(Tool::Notch, Key::W), Ok(()));
        assert_eq!(k.letter(Tool::Notch), 'W');
        assert_eq!(k.tool_for(Key::N), None);
        assert_eq!(
            k.assign(Tool::Segment, Key::W),
            Err(AssignError::Taken(Tool::Notch))
        );
        assert_eq!(
            k.assign(Tool::Segment, Key::Num1),
            Err(AssignError::NotALetter)
        );
        assert_eq!(k.letter(Tool::Segment), 'S');
        assert_eq!(
            k.assign(Tool::Notch, Key::W),
            Ok(()),
            "même lettre : accepté"
        );
    }

    #[test]
    fn aller_retour_json() {
        let mut k = ToolKeys::default();
        k.assign(Tool::Notch, Key::W).unwrap();
        k.assign(Tool::Dart, Key::N).unwrap();
        assert_eq!(ToolKeys::from_json(&k.to_json()), k);
    }

    #[test]
    fn json_invalide_ignore() {
        let d = ToolKeys::default();
        for v in [
            json!(null),
            json!([1]),
            json!("x"),
            json!({"notch": 5}),
            json!({"notch": "KK"}),
            json!({"notch": "1"}),
            json!({"inconnu": "K"}),
            json!({"notch": ""}),
        ] {
            assert_eq!(ToolKeys::from_json(&v), d, "{v}");
        }
    }

    #[test]
    fn doublon_dans_le_fichier_premier_gagne_sans_collision() {
        let k = ToolKeys::from_json(&json!({"select": "W", "point": "W"}));
        let mut seen = Vec::new();
        for t in Tool::ALL {
            let c = k.letter(t);
            if c != '\0' {
                assert!(!seen.contains(&c), "collision {c}");
                seen.push(c);
            }
        }
        assert_eq!(k.letter(Tool::Select), 'W');
    }

    #[test]
    fn reprise_d_une_lettre_par_defaut_retire_l_ancien() {
        // « segment » prend la lettre N, par défaut celle du cran.
        let k = ToolKeys::from_json(&json!({"segment": "N"}));
        assert_eq!(k.letter(Tool::Segment), 'N');
        assert_eq!(k.tool_for(Key::N), Some(Tool::Segment));
        assert_eq!(k.label(Tool::Notch), "");
    }
}
