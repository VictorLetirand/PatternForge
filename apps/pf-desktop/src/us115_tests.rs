//! US-11.5 (tranche 130) : fenêtre « Conventions de nommage… », tests egui FR et EN. Étape `nommage` :
//! pièces « P-001 », « P-002 », « Manche », règle « Regle P » (P, -, début 1, pas 1, largeur 3) et
//! règle « Regle F » (F, -, début 10, pas 10, largeur 2). Valeurs écrites à la main : prochain nom
//! de la règle des pièces P-003, aperçu du lot P-001 / P-002 / P-003, prochaine feuille F-10.

use crate::app::App;
use crate::dock::ToolWin;
use crate::Launch;
use eframe::egui::{self, Key, Modifiers};
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use pf_core::NamingTarget;

fn harness() -> Harness<'static, App> {
    let launch = Launch {
        demo: true,
        stage: Some("nommage".into()),
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
    rules: &'static str,
    next: &'static str,
    row: &'static str,
    apply: &'static str,
    remove: &'static str,
    add: &'static str,
    use_rule: &'static str,
}

fn names(h: &Harness<'_, App>) -> Vec<String> {
    h.state()
        .editor
        .project()
        .pieces
        .iter()
        .map(|p| p.name.clone())
        .collect()
}

fn walk(l: Labels) {
    let mut h = harness();
    h.get_by_label(l.window);
    h.get_by_label(l.rules);
    h.get_by_label(l.next);
    h.get_by_label(l.row);
    let win = h
        .ctx
        .memory(|m| m.area_rect(ToolWin::Naming.id()))
        .expect("fenêtre Conventions de nommage");
    assert!(
        h.state().dock.rect.contains_rect(win),
        "{win:?} hors du dock"
    );
    assert!(!win.intersects(h.state().canvas_rect));
    let before = h.state().editor.project().clone();
    assert_eq!(names(&h), ["P-001", "P-002", "Manche"]);

    // 1. Rien n'est appliqué d'office : la case est décochée, le nom par défaut est utilisé.
    assert!(!h.state().editor.piece_use_rule && !h.state().editor.sheet_use_rule);
    assert_eq!(
        h.state().editor.rule_name(NamingTarget::Piece, false),
        None,
        "case décochée = pas de nom de règle"
    );
    assert_eq!(
        h.state().editor.rule_name(NamingTarget::Piece, true),
        Some("P-003".to_string())
    );
    assert_eq!(
        h.state().editor.rule_name(NamingTarget::Sheet, true),
        Some("F-10".to_string())
    );

    // 2. Feuille créée avec la règle : « F-10 » ; une annulation, le prochain nom reste F-10.
    h.state_mut().editor.sheet_use_rule = true;
    h.state_mut().editor.add_sheet();
    h.run_steps(3);
    let p = h.state().editor.project();
    assert_eq!(p.sheets.last().unwrap().name, "F-10");
    assert_eq!(
        h.state().editor.rule_name(NamingTarget::Sheet, true),
        Some("F-20".to_string())
    );
    undo(&mut h);
    assert_eq!(*h.state().editor.project(), before);
    assert_eq!(
        h.state().editor.rule_name(NamingTarget::Sheet, true),
        Some("F-10".to_string()),
        "ni trou ni double après annulation"
    );

    // 3. Renommage en lot : aperçu affiché, un seul pas, annulation exacte.
    h.get_by_label(l.apply).click();
    h.run_steps(4);
    assert_eq!(names(&h), ["P-001", "P-002", "P-003"]);
    undo(&mut h);
    assert_eq!(*h.state().editor.project(), before, "une seule annulation");
    h.run_steps(3);

    // 4. Case « utiliser la règle » absente de l'état d'origine : cochée à la main seulement.
    h.state_mut()
        .editor
        .set_tool(crate::editor::Tool::ExtractPiece);
    h.run_steps(3);
    assert!(!h.state().editor.piece_use_rule);
    h.get_by_label(l.use_rule).click();
    h.run_steps(2);
    assert!(h.state().editor.piece_use_rule);
    h.get_by_label(l.use_rule);

    // 5. Suppression de la règle : les noms ne changent pas, une annulation la rend.
    h.get_by_label(l.remove).click();
    h.run_steps(4);
    assert_eq!(h.state().editor.project().naming_rules.len(), 1);
    assert_eq!(names(&h), ["P-001", "P-002", "Manche"]);
    undo(&mut h);
    assert_eq!(*h.state().editor.project(), before);
    h.run_steps(3);

    // 6. Ajout d'une règle depuis un formulaire vide (aucune règle choisie) : refus, projet identique.
    h.get_by_label(l.add).click();
    h.run_steps(3);
    assert_eq!(*h.state().editor.project(), before, "refus sans trace");
}

#[test]
fn us115_fr_conventions_de_nommage_fenetre_lot_annulation() {
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
    walk(Labels {
        window: "Conventions de nommage",
        rules: "Règles (2)",
        next: "Prochain nom : P-003",
        row: "Manche -> P-003",
        apply: "Renommer 3 objet(s)",
        remove: "Supprimer la règle",
        add: "Ajouter la règle",
        use_rule: "Utiliser la règle « Regle P »",
    });
}

#[test]
fn us115_en_naming_conventions_window_batch_undo() {
    pf_i18n::set_lang(pf_i18n::Lang::En);
    walk(Labels {
        window: "Naming conventions",
        rules: "Rules (2)",
        next: "Next name: P-003",
        row: "Manche -> P-003",
        apply: "Rename 3 object(s)",
        remove: "Delete the rule",
        add: "Add the rule",
        use_rule: "Use the rule “Regle P”",
    });
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
}
