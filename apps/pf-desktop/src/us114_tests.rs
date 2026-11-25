//! US-11.4 (tranche 129) : fenêtre « Bibliothèque… », tests egui FR et EN. Étape `bibliotheque` :
//! projet « Rabat » (variable L = 50, repère à L du départ du premier côté), dossier temporaire
//! avec trois entrées valides (pièce, bloc, modèle) et un fichier abîmé (numéro 4).
//! Valeurs écrites à la main : l'insertion d'une pièce ajoute 1 feuille et 1 pièce ; L reste seule.

use crate::app::App;
use crate::dock::ToolWin;
use crate::Launch;
use eframe::egui::{self, Event, Key, Modifiers};
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use pf_core::Library;

fn harness() -> Harness<'static, App> {
    let launch = Launch {
        demo: true,
        stage: Some("bibliotheque".into()),
        ..Launch::default()
    };
    let app = App::with_context(&egui::Context::default(), launch);
    let mut h = Harness::builder()
        .with_size(egui::vec2(1280.0, 900.0))
        .build_state(|ctx, app: &mut App| app.ui(ctx), app);
    h.run_steps(6);
    h
}

fn undo(h: &mut Harness<'_, App>) {
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run_steps(2);
}

struct Labels {
    window: &'static str,
    entries: &'static str,
    problems: &'static str,
    row_piece: &'static str,
    row_model: &'static str,
    insert: &'static str,
    save: &'static str,
    name_field: &'static str,
    kind_model: &'static str,
    rename_vars: &'static str,
    rename: &'static str,
    rename_field: &'static str,
    delete: &'static str,
    inserted: &'static str,
    saved: &'static str,
    duplicate: &'static str,
    no_piece: &'static str,
    kind_piece: &'static str,
    conflict: &'static str,
    deleted: &'static str,
}

fn walk(l: Labels) {
    let mut h = harness();
    // La fenêtre est dans le dock, sans recouvrir le canevas.
    h.get_by_label(l.window);
    h.get_by_label(l.entries);
    h.get_by_label(l.problems);
    h.get_by_label(l.row_piece);
    let win = h
        .ctx
        .memory(|m| m.area_rect(ToolWin::Library.id()))
        .expect("fenêtre Bibliothèque");
    assert!(
        h.state().dock.rect.contains_rect(win),
        "{win:?} hors du dock"
    );
    assert!(!win.intersects(h.state().canvas_rect));
    let dir = h.state().library_dir();
    let before = h.state().editor.project().clone();
    assert_eq!((before.sheets.len(), before.pieces.len()), (1, 1));

    // 1. Insérer la pièce « Rabat cuir » : une étape, L (même valeur) reste seule ; une annulation.
    h.get_by_label(l.insert).click();
    h.run_steps(4);
    let p = h.state().editor.project().clone();
    assert_eq!((p.sheets.len(), p.pieces.len()), (2, 2));
    assert_eq!(p.variables.len(), 1);
    h.get_by_label_contains(l.inserted);
    undo(&mut h);
    assert_eq!(*h.state().editor.project(), before, "une seule annulation");
    h.run_steps(3);

    // 2. Conflit de variable : refus nommé, projet identique, puis case « renommer » : L_2.
    h.state_mut()
        .editor
        .exec(pf_core::Command::SetVariableExpr {
            name: "L".into(),
            expr: "60".into(),
        })
        .expect("L = 60");
    let conflicted = h.state().editor.project().clone();
    h.run_steps(3);
    h.get_by_label(l.insert).click();
    h.run_steps(4);
    h.get_by_label_contains(l.conflict);
    assert_eq!(*h.state().editor.project(), conflicted, "refus sans trace");
    h.get_by_label(l.rename_vars).click();
    h.run_steps(3);
    h.get_by_label(l.insert).click();
    h.run_steps(4);
    let p = h.state().editor.project().clone();
    assert_eq!((p.sheets.len(), p.pieces.len()), (2, 2));
    let names: Vec<_> = p.variables.iter().map(|v| v.name.as_str()).collect();
    assert_eq!(names, ["L", "L_2"]);
    assert_eq!(
        p.variables[0].expr, "60",
        "la variable du projet est intacte"
    );
    assert_eq!(p.variables[1].expr, "50");
    undo(&mut h);
    undo(&mut h);
    assert_eq!(*h.state().editor.project(), before);
    h.run_steps(3);

    // 3. Enregistrer : pièce non choisie = refus nommé, rien d'écrit.
    h.get_by_label(l.save).click();
    h.run_steps(3);
    h.get_by_label(l.no_piece);
    assert_eq!(Library::new(&dir).list().unwrap().entries.len(), 3);
    // Modèle : nom saisi au clavier, doublon (casse ignorée) refusé, puis nom libre accepté.
    h.get_by_label(l.kind_model).click();
    h.run_steps(3);
    let field = h.get_by_role_and_label(egui::accesskit::Role::TextInput, l.name_field);
    field.focus();
    h.run_steps(2);
    h.input_mut()
        .events
        .push(Event::Text("MODELE RABAT".to_string()));
    h.run_steps(3);
    h.get_by_label(l.save).click();
    h.run_steps(3);
    h.get_by_label_contains(l.duplicate);
    assert_eq!(Library::new(&dir).list().unwrap().entries.len(), 3);
    h.get_by_role_and_label(egui::accesskit::Role::TextInput, l.name_field)
        .focus();
    h.run_steps(2);
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.run_steps(1);
    h.input_mut()
        .events
        .push(Event::Text("Copie du modèle".to_string()));
    h.run_steps(3);
    h.get_by_label(l.save).click();
    h.run_steps(3);
    h.get_by_label_contains(l.saved);
    let listing = Library::new(&dir).list().unwrap();
    // Le numéro 4 est réservé par le fichier abîmé : la nouvelle entrée est la 5.
    assert_eq!(
        listing.entries.iter().map(|e| e.id).collect::<Vec<_>>(),
        [1, 2, 3, 5]
    );
    assert_eq!(listing.problems.len(), 1);
    assert_eq!(
        *h.state().editor.project(),
        before,
        "enregistrer ne modifie pas le projet"
    );

    // 4. Renommer puis supprimer l'entrée sélectionnée (la nouvelle).
    let field = h.get_by_role_and_label(egui::accesskit::Role::TextInput, l.rename_field);
    field.focus();
    h.run_steps(2);
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.run_steps(1);
    h.input_mut()
        .events
        .push(Event::Text("Modèle bis".to_string()));
    h.run_steps(3);
    h.get_by_label(l.rename).click();
    h.run_steps(3);
    assert_eq!(Library::new(&dir).load(5).unwrap().name, "Modèle bis");
    h.get_by_label(l.delete).click();
    h.run_steps(3);
    h.get_by_label_contains(l.deleted);
    assert!(!dir.join("lib-000005.pforge-lib.json").exists());
    // Le fichier abîmé n'a jamais été touché.
    assert_eq!(
        std::fs::read(dir.join("lib-000004.pforge-lib.json")).unwrap(),
        b"{\"format\": \"patternforge-library\""
    );
    let _ = (l.row_model, l.kind_piece);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn us114_fr_bibliotheque_fenetre_inserer_conflit_enregistrer_renommer_supprimer() {
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
    walk(Labels {
        window: "Bibliothèque",
        entries: "Entrées (3)",
        problems: "Fichiers illisibles (1)",
        row_piece: "Rabat cuir — Pièce",
        row_model: "Modele rabat — Modèle",
        insert: "Insérer",
        save: "Enregistrer la sélection",
        name_field: "Nom de l'entrée (champ)",
        kind_model: "Modèle",
        rename_vars: "Renommer les variables en conflit (nom_2…)",
        rename: "Renommer",
        rename_field: "Nouveau nom (champ)",
        delete: "Supprimer",
        inserted: "insérée (une étape annulable)",
        saved: "Entrée « Copie du modèle » enregistrée.",
        duplicate: "existe déjà",
        no_piece: "Choisissez la pièce à enregistrer.",
        kind_piece: "Pièce",
        conflict: "existe déjà avec une autre valeur ou une autre unité",
        deleted: "supprimée",
    });
}

#[test]
fn us114_en_library_window_insert_conflict_save_rename_delete() {
    pf_i18n::set_lang(pf_i18n::Lang::En);
    walk(Labels {
        window: "Library",
        entries: "Entries (3)",
        problems: "Unreadable files (1)",
        row_piece: "Rabat cuir — Piece",
        row_model: "Modele rabat — Model",
        insert: "Insert",
        save: "Save the selection",
        name_field: "Entry name (field)",
        kind_model: "Model",
        rename_vars: "Rename conflicting variables (name_2…)",
        rename: "Rename",
        rename_field: "New name (field)",
        delete: "Delete",
        inserted: "inserted (one undoable step)",
        saved: "Entry “Copie du modèle” saved.",
        duplicate: "already exists",
        no_piece: "Choose the piece to save.",
        kind_piece: "Piece",
        conflict: "already exists with another value or unit",
        deleted: "deleted",
    });
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
}
