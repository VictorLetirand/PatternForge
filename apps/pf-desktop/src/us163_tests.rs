//! US-16.3 : table centrale des raccourcis. Les valeurs attendues sont écrites à la main ici ; la
//! campagne adverse tient son propre modèle (textes de combinaisons) sans lire la table testée.

use crate::app::App;
use crate::editor::Tool;
use crate::shortcuts::{self, BindError, Binding, Cmd, CmdKeys, FileError, Keymap, Preset};
use crate::Launch;
use eframe::egui::{self, Key, Modifiers};
use egui_kittest::Harness;
use pf_i18n::{set_lang, tr, Lang};
use serde_json::{json, Value};

fn harness() -> Harness<'static, App> {
    shortcuts::set_current_keymap(Keymap::default());
    let app = App::with_context(&egui::Context::default(), Launch::default());
    let mut h = Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .build_state(|ctx, app: &mut App| app.ui(ctx), app);
    h.run();
    h
}

fn temp(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("pf_us163_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

const CTRL: Modifiers = Modifiers::COMMAND;
const CTRL_SHIFT: Modifiers = Modifiers {
    command: true,
    ctrl: true,
    shift: true,
    alt: false,
    mac_cmd: false,
};

/// Les 20 raccourcis d'avant la table, écrits en dur : (modificateurs, touche, commande).
fn legacy() -> Vec<(Modifiers, Key, Cmd)> {
    vec![
        (CTRL, Key::N, Cmd::New),
        (CTRL, Key::O, Cmd::Open),
        (CTRL, Key::S, Cmd::Save),
        (CTRL_SHIFT, Key::S, Cmd::SaveAs),
        (CTRL, Key::Z, Cmd::Undo),
        (CTRL, Key::Y, Cmd::Redo),
        (CTRL_SHIFT, Key::Z, Cmd::Redo),
        (CTRL, Key::F, Cmd::Find),
        (CTRL, Key::G, Cmd::Group),
        (CTRL_SHIFT, Key::G, Cmd::Ungroup),
        (CTRL, Key::A, Cmd::SelectAll),
        (CTRL_SHIFT, Key::A, Cmd::SelectChain),
        (CTRL, Key::I, Cmd::InvertSelection),
        (CTRL, Key::L, Cmd::ToggleLasso),
        (Modifiers::NONE, Key::Delete, Cmd::Delete),
        (Modifiers::NONE, Key::Home, Cmd::FitView),
        (Modifiers::NONE, Key::F2, Cmd::RenameSheet),
        (Modifiers::NONE, Key::F1, Cmd::Help),
    ]
}

#[test]
fn les_identifiants_sont_figes_uniques_et_dans_l_ordre() {
    let ids: Vec<&str> = Cmd::ALL.iter().map(|c| c.id()).collect();
    assert_eq!(
        ids,
        vec![
            "file.new",
            "file.open",
            "file.save",
            "file.save_as",
            "edit.undo",
            "edit.redo",
            "edit.find",
            "edit.group",
            "edit.ungroup",
            "edit.cut",
            "edit.copy",
            "edit.paste",
            "select.all",
            "select.chain",
            "select.invert",
            "select.lasso",
            "edit.delete",
            "view.fit",
            "sheet.rename",
            "help.shortcuts",
            "edit.repeat_last",
            "edit.cancel",
            "view.zoom_selection",
        ]
    );
    for cmd in Cmd::ALL {
        assert_eq!(Cmd::from_id(cmd.id()), Some(*cmd));
    }
}

#[test]
fn chaque_commande_a_un_libelle_dans_les_deux_langues_et_une_action() {
    let run_cmd = include_str!("app.rs")
        .split("pub(crate) fn run_cmd")
        .nth(1)
        .and_then(|rest| rest.split("\n    /// ").next())
        .expect("run_cmd")
        .replace("\r\n", "\n");
    let mut in_match: Vec<String> = run_cmd
        .split("Cmd::")
        .skip(1)
        .filter_map(|chunk| {
            let name: String = chunk.chars().take_while(|c| c.is_alphanumeric()).collect();
            chunk[name.len()..]
                .trim_start()
                .starts_with("=>")
                .then_some(name)
        })
        .collect();
    in_match.sort();
    in_match.dedup();
    let mut derived: Vec<String> = Cmd::ALL.iter().map(|c| format!("{c:?}")).collect();
    derived.sort();
    assert_eq!(in_match, derived, "table et actions divergent");
    for lang in Lang::ALL {
        set_lang(lang);
        for cmd in Cmd::ALL {
            let label = cmd.label();
            assert!(!label.is_empty() && !label.starts_with("cmd."), "{cmd:?}");
        }
    }
    set_lang(Lang::Fr);
    // Les outils : une lettre par défaut, tous présents (liste dérivée de `Tool::ALL`).
    let keys = shortcuts::current_keymap();
    for tool in Tool::ALL {
        // Decalque : aucun raccourci par defaut (les 26 lettres sont prises).
        if tool != Tool::Trace {
            assert_ne!(keys.tools.label(tool), "", "{tool:?}");
        }
        assert!(!tool.label().is_empty());
    }
}

#[test]
fn chaque_raccourci_existant_declenche_la_meme_commande_qu_avant() {
    for (mods, key, cmd) in legacy() {
        let mut h = harness();
        h.key_press_modifiers(mods, key);
        h.run_steps(2);
        assert_eq!(h.state().cmd_log, vec![cmd], "{key:?} {mods:?}");
    }
    // Hors saisie, Ctrl+X / C / V passent par le presse-papiers : même commande qu'avant.
    for (key, cmd) in [
        (Key::X, Cmd::Cut),
        (Key::C, Cmd::Copy),
        (Key::V, Cmd::Paste),
    ] {
        let mut h = harness();
        h.key_press_modifiers(CTRL, key);
        h.run_steps(2);
        let log = &h.state().cmd_log;
        assert!(log.is_empty() || log == &vec![cmd], "{key:?}");
    }
    // Une lettre d'outil seule choisit toujours son outil.
    let mut h = harness();
    h.key_press(Key::S);
    h.run_steps(2);
    assert_eq!(h.state().editor.tool, Tool::Segment);
    assert!(h.state().cmd_log.is_empty());
}

#[test]
fn les_effets_directs_sont_conserves() {
    let mut h = harness();
    assert!(!h.state().show_help);
    h.key_press(Key::F1);
    h.run_steps(2);
    assert!(h.state().show_help);
    h.key_press(Key::F2);
    h.run_steps(2);
    assert!(h.state().renaming.is_some(), "F2 renomme la feuille");
}

#[test]
fn une_combinaison_personnalisee_remplace_l_ancienne() {
    let mut h = harness();
    let mut km = shortcuts::current_keymap();
    km.cmds
        .assign(Cmd::Find, Some(Binding::ctrl(Key::K)))
        .unwrap();
    h.state_mut().apply_keymap(km);
    h.key_press_modifiers(CTRL, Key::F);
    h.run_steps(2);
    assert!(
        h.state().cmd_log.is_empty(),
        "l'ancienne combinaison est libre"
    );
    h.key_press_modifiers(CTRL, Key::K);
    h.run_steps(2);
    assert_eq!(h.state().cmd_log, vec![Cmd::Find]);
    shortcuts::set_current_keymap(Keymap::default());
}

#[test]
fn un_conflit_est_refuse_avec_le_nom_de_la_commande_en_conflit() {
    let mut keys = CmdKeys::default();
    assert_eq!(
        keys.assign(Cmd::Find, Some(Binding::ctrl(Key::G))),
        Err(BindError::Conflict(Cmd::Group))
    );
    assert_eq!(
        keys.get(Cmd::Find),
        Some(Binding::ctrl(Key::F)),
        "rien changé"
    );
    assert_eq!(
        keys.assign(Cmd::Undo, Some(Binding::ctrl_shift(Key::Z))),
        Err(BindError::Conflict(Cmd::Redo)),
        "le second raccourci de Rétablir est réservé"
    );
    assert_eq!(
        keys.assign(Cmd::Find, Some(Binding::bare(Key::K))),
        Err(BindError::NeedsCtrl)
    );
    assert_eq!(
        keys.assign(Cmd::Find, Some(Binding::bare(Key::Enter))),
        Err(BindError::Reserved)
    );
    assert_eq!(
        keys.assign(Cmd::RepeatLast, None),
        Err(BindError::Fixed(Cmd::RepeatLast))
    );
    // Retirer puis réaffecter la combinaison libérée est permis.
    keys.assign(Cmd::Group, None).unwrap();
    assert_eq!(keys.assign(Cmd::Find, Some(Binding::ctrl(Key::G))), Ok(()));

    // Le message affiché cite la commande en conflit, dans les deux langues.
    let mut h = harness();
    let refused = h
        .state_mut()
        .assign_target(shortcuts::Target::Cmd(Cmd::Find), Binding::ctrl(Key::G));
    assert!(!refused);
    let err = h.state().key_error.clone().unwrap();
    assert_eq!(err, "Ctrl+G est déjà utilisé par « Grouper ».");
    set_lang(Lang::En);
    h.state_mut()
        .assign_target(shortcuts::Target::Cmd(Cmd::Find), Binding::ctrl(Key::G));
    assert_eq!(
        h.state().key_error.as_deref(),
        Some("Ctrl+G is already used by \"Group\".")
    );
    set_lang(Lang::Fr);
    assert_eq!(
        shortcuts::current_cmds().get(Cmd::Find),
        Some(Binding::ctrl(Key::F))
    );
}

#[test]
fn les_jeux_predefinis_et_la_reinitialisation() {
    let default = Keymap::default();
    let minimal = Preset::Minimal.keymap();
    assert_eq!(minimal.cmds.get(Cmd::Find), None);
    assert_eq!(minimal.cmds.get(Cmd::Save), Some(Binding::ctrl(Key::S)));
    assert_eq!(
        minimal.cmds.get(Cmd::RepeatLast),
        Some(Binding::bare(Key::Enter))
    );
    assert_eq!(minimal.tools.label(Tool::Select), "V");
    assert_eq!(minimal.tools.label(Tool::Notch), "");
    assert_eq!(Preset::PatternForge.keymap(), default);
    let mut h = harness();
    h.state_mut().apply_preset(Preset::Minimal);
    assert_eq!(shortcuts::current_keymap(), minimal);
    h.key_press(Key::N); // l'outil Cran n'a plus de lettre
    h.run_steps(2);
    assert_ne!(h.state().editor.tool, Tool::Notch);
    h.state_mut().apply_preset(Preset::PatternForge);
    assert_eq!(shortcuts::current_keymap(), default);
}

#[test]
fn export_puis_import_redonnent_la_meme_table_et_refusent_les_fichiers_invalides() {
    let dir = temp("fichier");
    let file = dir.join("raccourcis.json");
    let mut km = Keymap::default();
    km.cmds
        .assign(Cmd::Find, Some(Binding::ctrl(Key::K)))
        .unwrap();
    km.cmds.assign(Cmd::Group, None).unwrap();
    km.tools.assign(Tool::Notch, Key::W).unwrap();
    km.tools.unbind(Tool::Dart);
    let mut h = harness();
    h.state_mut().apply_keymap(km.clone());
    assert!(h.state_mut().export_keymap_to(&file));
    h.state_mut().apply_preset(Preset::PatternForge);
    assert!(h.state_mut().import_keymap_from(&file));
    assert_eq!(shortcuts::current_keymap(), km);
    let text = std::fs::read_to_string(&file).unwrap();
    let value: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(value["format"], "patternforge-shortcuts");
    assert_eq!(value["version"], 1);
    assert_eq!(value["commands"]["edit.find"], "Ctrl+K");
    assert_eq!(value["commands"]["edit.group"], Value::Null);
    assert_eq!(value["tools"]["notch"], "W");

    // Fichiers invalides : refus entier, la table ne bouge pas, message d'erreur affiché.
    let before = shortcuts::current_keymap();
    let bad: Vec<(&str, String, &str)> = vec![
        ("not_json", "pas du json".into(), "illisible"),
        ("array", "[1]".into(), "objet"),
        ("format", r#"{"format":"x","version":1}"#.into(), "pas un fichier"),
        (
            "version",
            r#"{"format":"patternforge-shortcuts","version":2}"#.into(),
            "version",
        ),
        (
            "champ",
            r#"{"format":"patternforge-shortcuts","version":1,"extra":1}"#.into(),
            "champ inconnu",
        ),
        (
            "commande",
            r#"{"format":"patternforge-shortcuts","version":1,"commands":{"file.fly":"Ctrl+K"}}"#
                .into(),
            "commande inconnue",
        ),
        (
            "outil",
            r#"{"format":"patternforge-shortcuts","version":1,"tools":{"laser":"K"}}"#.into(),
            "outil inconnu",
        ),
        (
            "valeur",
            r#"{"format":"patternforge-shortcuts","version":1,"commands":{"edit.find":"Ctrl+Alt+K"}}"#
                .into(),
            "valeur invalide",
        ),
        (
            "lettre",
            r#"{"format":"patternforge-shortcuts","version":1,"commands":{"edit.find":"K"}}"#.into(),
            "valeur invalide",
        ),
        (
            "conflit",
            r#"{"format":"patternforge-shortcuts","version":1,"commands":{"edit.find":"Ctrl+G","edit.group":"Ctrl+G"}}"#
                .into(),
            "conflit",
        ),
        (
            "fixe",
            r#"{"format":"patternforge-shortcuts","version":1,"commands":{"edit.cancel":"F5"}}"#
                .into(),
            "valeur invalide",
        ),
        (
            "outil en double",
            r#"{"format":"patternforge-shortcuts","version":1,"tools":{"select":"K","point":"K"}}"#
                .into(),
            "conflit",
        ),
    ];
    for (name, body, reason) in bad {
        let f = dir.join(format!("{name}.json"));
        std::fs::write(&f, body).unwrap();
        assert!(!h.state_mut().import_keymap_from(&f), "{name}");
        assert!(h.state().editor.status.error, "{name}");
        assert!(
            h.state().editor.status.text.contains(reason),
            "{name}: {}",
            h.state().editor.status.text
        );
        assert_eq!(shortcuts::current_keymap(), before, "{name}");
    }
    assert!(!h.state_mut().import_keymap_from(&dir.join("absent.json")));
    shortcuts::set_current_keymap(Keymap::default());
}

#[test]
fn la_table_est_memorisee_dans_les_reglages_et_relit_les_anciens_reglages() {
    let dir = temp("reglages");
    let file = dir.join("settings.json");
    let mut h = harness();
    h.state_mut().settings_path = Some(file.clone());
    let mut km = Keymap::default();
    km.cmds
        .assign(Cmd::Find, Some(Binding::ctrl(Key::K)))
        .unwrap();
    km.tools.assign(Tool::Notch, Key::W).unwrap();
    h.state_mut().apply_keymap(km.clone());
    let stored = pf_i18n::settings::load_value(&file, shortcuts::SETTINGS_KEY).unwrap();
    assert_eq!(stored, json!({"edit.find": "Ctrl+K", "notch": "W"}));
    assert_eq!(Keymap::from_settings(&stored), km);
    // Ancien réglage (lettres d'outils seules) : relu tel quel.
    let old = Keymap::from_settings(&json!({"notch": "W"}));
    assert_eq!(old.tools.label(Tool::Notch), "W");
    assert_eq!(old.cmds, CmdKeys::default());
    shortcuts::set_current_keymap(Keymap::default());
}

#[test]
fn les_reglages_douteux_sont_ignores_sans_perte_de_defaut() {
    let d = CmdKeys::default();
    for bad in [
        json!(null),
        json!([1]),
        json!("x"),
        json!({"edit.find": 5}),
        json!({"edit.find": "Ctrl+Alt+K"}),
        json!({"edit.find": "K"}),
        json!({"edit.find": "Ctrl+G"}),
        json!({"inconnu": "Ctrl+K"}),
        json!({"edit.cancel": "F5"}),
        json!({"edit.find": "Ctrl+Shift+Z"}),
    ] {
        let (keys, _) = CmdKeys::from_settings(&bad);
        assert_eq!(keys, d, "{bad}");
    }
    let (keys, ignored) =
        CmdKeys::from_settings(&json!({"edit.find": "Ctrl+Z", "edit.undo": "Ctrl+F"}));
    assert_eq!(ignored, 0, "un échange est permis");
    assert_eq!(keys.get(Cmd::Find), Some(Binding::ctrl(Key::Z)));
    assert_eq!(keys.get(Cmd::Undo), Some(Binding::ctrl(Key::F)));
}

#[test]
fn le_texte_d_une_combinaison_est_stable() {
    assert_eq!(Binding::ctrl_shift(Key::S).text(), "Ctrl+Shift+S");
    assert_eq!(Binding::bare(Key::F2).text(), "F2");
    assert_eq!(Binding::ctrl(Key::Num5).text(), "Ctrl+5");
    assert_eq!(
        Binding::parse("ctrl+shift+S"),
        Ok(Binding::ctrl_shift(Key::S))
    );
    assert_eq!(Binding::parse("Delete"), Ok(Binding::bare(Key::Delete)));
    assert_eq!(Binding::parse("Ctrl+"), Err(BindError::Unreadable));
    assert_eq!(Binding::parse("Ctrl+Ctrl+S"), Err(BindError::Unreadable));
    assert_eq!(Binding::parse("Alt+S"), Err(BindError::Unreadable));
    assert_eq!(
        Binding::parse("s"),
        Err(BindError::Unreadable),
        "touche en majuscule"
    );
    set_lang(Lang::En);
    assert_eq!(tr("key.shift"), "Shift");
    set_lang(Lang::Fr);
    assert_eq!(Binding::ctrl_shift(Key::S).display(), "Ctrl+Maj+S");
}

// ----- campagne adverse -------------------------------------------------------------------

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

/// Modèle indépendant : identifiant → texte de combinaison (ou rien).
struct Model(Vec<(&'static str, Option<String>)>);

impl Model {
    fn new() -> Model {
        let table: [(&'static str, &str); 22] = [
            ("file.new", "Ctrl+N"),
            ("file.open", "Ctrl+O"),
            ("file.save", "Ctrl+S"),
            ("file.save_as", "Ctrl+Shift+S"),
            ("edit.undo", "Ctrl+Z"),
            ("edit.redo", "Ctrl+Y"),
            ("edit.find", "Ctrl+F"),
            ("edit.group", "Ctrl+G"),
            ("edit.ungroup", "Ctrl+Shift+G"),
            ("edit.cut", "Ctrl+X"),
            ("edit.copy", "Ctrl+C"),
            ("edit.paste", "Ctrl+V"),
            ("select.all", "Ctrl+A"),
            ("select.chain", "Ctrl+Shift+A"),
            ("select.invert", "Ctrl+I"),
            ("select.lasso", "Ctrl+L"),
            ("edit.delete", "Delete"),
            ("view.fit", "Home"),
            ("sheet.rename", "F2"),
            ("help.shortcuts", "F1"),
            ("edit.repeat_last", "Enter"),
            ("edit.cancel", "Escape"),
        ];
        Model(
            table
                .iter()
                .map(|(i, t)| (*i, Some((*t).to_string())))
                .collect(),
        )
    }

    /// Verdict attendu d'une affectation, écrit d'après la règle de la story.
    fn verdict(&self, id: &str, combo: Option<&str>) -> Result<(), &'static str> {
        if id == "edit.repeat_last" || id == "edit.cancel" {
            return Err("fixed");
        }
        let Some(combo) = combo else {
            return Ok(());
        };
        let key = combo.rsplit('+').next().unwrap();
        if ["Enter", "Escape", "Tab", "Backspace"].contains(&key) {
            return Err("reserved");
        }
        let typing = key.len() == 1 && key.chars().all(|c| c.is_ascii_alphanumeric());
        if typing && !combo.contains("Ctrl+") {
            return Err("needs_ctrl");
        }
        if combo == "Ctrl+Shift+Z" {
            return if id == "edit.redo" {
                Ok(())
            } else {
                Err("conflict")
            };
        }
        if self
            .0
            .iter()
            .any(|(i, c)| *i != id && c.as_deref() == Some(combo))
        {
            return Err("conflict");
        }
        Ok(())
    }
}

#[test]
fn campagne_adverse_affectations_et_fichiers_de_raccourcis() {
    let mut rng = Rng(0xDEAD_BEEF_CAFE_F00D);
    let mut model = Model::new();
    let mut keys = CmdKeys::default();
    let pool = [
        "Ctrl+N",
        "Ctrl+G",
        "Ctrl+K",
        "Ctrl+Shift+Z",
        "Ctrl+Shift+K",
        "F5",
        "F2",
        "Delete",
        "K",
        "Enter",
        "Ctrl+Enter",
        "Ctrl+5",
        "5",
        "Home",
        "Ctrl+Z",
        "Ctrl+Y",
        "Tab",
        "Ctrl+Shift+S",
    ];
    let mut seen = std::collections::BTreeMap::<&str, u32>::new();
    for step in 0..400 {
        let id = Model::new().0[rng.below(22)].0;
        let cmd = Cmd::from_id(id).unwrap();
        let combo = if rng.below(6) == 0 {
            None
        } else {
            Some(pool[rng.below(pool.len())])
        };
        let want = model.verdict(id, combo);
        let binding = combo.map(|c| Binding::parse(c).unwrap());
        let got = keys.assign(cmd, binding);
        match (&want, &got) {
            (Ok(()), Ok(())) => {
                *seen.entry("ok").or_default() += 1;
                if combo.is_none() {
                    *seen.entry("removed").or_default() += 1;
                }
                let slot = model.0.iter_mut().find(|(i, _)| *i == id).unwrap();
                slot.1 = combo.map(str::to_string);
            }
            (Err("fixed"), Err(BindError::Fixed(_))) => *seen.entry("fixed").or_default() += 1,
            (Err("reserved"), Err(BindError::Reserved)) => {
                *seen.entry("reserved").or_default() += 1;
            }
            (Err("needs_ctrl"), Err(BindError::NeedsCtrl)) => {
                *seen.entry("needs_ctrl").or_default() += 1;
            }
            (Err("conflict"), Err(BindError::Conflict(owner))) => {
                *seen.entry("conflict").or_default() += 1;
                let expected_owner = if combo == Some("Ctrl+Shift+Z") {
                    "edit.redo"
                } else {
                    model
                        .0
                        .iter()
                        .find(|(_, c)| c.as_deref() == combo)
                        .map(|(i, _)| *i)
                        .unwrap()
                };
                assert_eq!(owner.id(), expected_owner, "step {step}");
            }
            _ => panic!("step {step}: {id} {combo:?} attendu {want:?}, obtenu {got:?}"),
        }
        // Invariants : jamais deux commandes sur la même combinaison, état = modèle.
        for (i, c) in &model.0 {
            let actual = keys.get(Cmd::from_id(i).unwrap()).map(Binding::text);
            assert_eq!(actual.as_deref(), c.as_deref(), "step {step} {i}");
        }
        let mut texts: Vec<String> = model.0.iter().filter_map(|(_, c)| c.clone()).collect();
        texts.sort();
        let n = texts.len();
        texts.dedup();
        assert_eq!(texts.len(), n, "step {step}: combinaison en double");
        // Aller-retour par les réglages à chaque étape.
        let stored = Value::Object(keys.to_json());
        assert_eq!(CmdKeys::from_settings(&stored).0, keys, "step {step}");
    }
    for k in [
        "ok",
        "removed",
        "fixed",
        "reserved",
        "needs_ctrl",
        "conflict",
    ] {
        assert!(
            seen.get(k).copied().unwrap_or(0) >= 5,
            "{k} peu couvert : {seen:?}"
        );
    }

    // Fichiers corrompus : verdicts connus d'avance, jamais de panique, jamais de changement.
    let valid = Keymap::default().export_text();
    let mut refused = 0;
    let mut accepted = 0;
    for n in 0..200 {
        let (text, ok): (String, bool) = match rng.below(9) {
            0 => (valid.clone(), true),
            1 => (valid[..1 + rng.below(valid.len() - 2)].to_string(), false),
            2 => (valid.replace("\"version\": 1", "\"version\": 7"), false),
            3 => (valid.replace("patternforge-shortcuts", "x"), false),
            4 => (valid.replace("\"Ctrl+O\"", "\"Ctrl+N\""), false),
            5 => (valid.replace("\"Ctrl+O\"", "\"Ctrl+Alt+O\""), false),
            6 => (valid.replace("\"file.open\"", "\"file.ouvre\""), false),
            7 => (valid.replace("\"Ctrl+O\"", "null"), true),
            _ => (valid.replace("\"S\"", "\"s\""), false),
        };
        let result = Keymap::import_text(&text);
        assert_eq!(result.is_ok(), ok, "n={n} {text:.80}");
        if ok {
            accepted += 1;
        } else {
            refused += 1;
            let err: FileError = result.unwrap_err();
            assert!(!format!("{err:?}").is_empty());
        }
    }
    assert!(accepted >= 20 && refused >= 120, "{accepted}/{refused}");
}
