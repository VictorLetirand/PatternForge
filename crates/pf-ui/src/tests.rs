//! Tests d'interface : la vraie boucle egui est exécutée hors écran.

use crate::components::{
    confirm_dialog, tool_button, DialogAction, NumericField, ToastLevel, Toasts, Unit,
};
use crate::lab::{ui_lab, LabPage, LabState};
use crate::theme;
use crate::tokens::Tokens;
use egui::accesskit::Role;
use egui::{Event, Key, Modifiers, PointerButton, Pos2};
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;

struct Fx {
    v: f64,
    changed: bool,
    committed: bool,
    error: Option<String>,
}

fn numeric_harness(v: f64) -> Harness<'static, Fx> {
    let fx = Fx {
        v,
        changed: false,
        committed: false,
        error: None,
    };
    let mut h = Harness::builder()
        .with_size(egui::vec2(400.0, 200.0))
        .build_state(
            |ctx, s: &mut Fx| {
                theme::apply(ctx, &Tokens::precision_light());
                egui::CentralPanel::default().show(ctx, |ui| {
                    let r = NumericField::new(&mut s.v, Unit::Mm)
                        .id_salt("f")
                        .step(1.0)
                        .range(0.0..=1000.0)
                        .show(ui);
                    s.changed |= r.changed;
                    s.committed |= r.committed;
                    s.error = r.error;
                    // Second widget focalisable : cible d'une perte de focus.
                    let _ = ui.button("Ailleurs");
                });
            },
            fx,
        );
    h.run_steps(3);
    h
}

fn focus_field(h: &mut Harness<'_, Fx>) {
    h.get_by_role(Role::TextInput).focus();
    h.run_steps(3);
}

fn type_replace(h: &mut Harness<'_, Fx>, text: &str) {
    focus_field(h);
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.run_steps(2);
    h.input_mut().events.push(Event::Text(text.to_string()));
    h.run_steps(2);
}

fn press(h: &mut Harness<'_, Fx>, key: Key) {
    h.key_press(key);
    h.run_steps(3);
}

fn field_text(h: &Harness<'_, Fx>) -> String {
    h.get_by_role(Role::TextInput).value().unwrap_or_default()
}

#[test]
fn champ_accepte_unite_et_virgule() {
    let mut h = numeric_harness(50.0);
    type_replace(&mut h, "12 cm");
    press(&mut h, Key::Enter);
    assert_eq!(h.state().v, 120.0);
    assert!(h.state().committed && h.state().changed);
    assert_eq!(field_text(&h), "120,0");

    let mut h = numeric_harness(50.0);
    type_replace(&mut h, "120,5");
    press(&mut h, Key::Enter);
    assert_eq!(h.state().v, 120.5);
}

#[test]
fn champ_refuse_texte_invalide_sans_perdre_la_saisie() {
    let mut h = numeric_harness(50.0);
    type_replace(&mut h, "abc");
    press(&mut h, Key::Enter);
    assert_eq!(h.state().v, 50.0);
    assert!(!h.state().committed);
    assert!(h.state().error.is_some());
    assert_eq!(field_text(&h), "abc");
    h.get_by_label_contains("pas un nombre");
}

#[test]
fn champ_refuse_hors_bornes() {
    let mut h = numeric_harness(50.0);
    type_replace(&mut h, "1,2 m");
    press(&mut h, Key::Enter);
    assert_eq!(h.state().v, 50.0);
    assert!(h
        .state()
        .error
        .as_deref()
        .is_some_and(|e| e.contains("hors bornes")));
    assert_eq!(field_text(&h), "1,2 m");
}

#[test]
fn champ_echap_annule() {
    let mut h = numeric_harness(50.0);
    type_replace(&mut h, "77");
    press(&mut h, Key::Escape);
    assert_eq!(h.state().v, 50.0);
    assert!(!h.state().committed);
    assert_eq!(field_text(&h), "50,0");
}

#[test]
fn champ_fleches_incrementent_et_bornent() {
    let mut h = numeric_harness(50.0);
    focus_field(&mut h);
    press(&mut h, Key::ArrowUp);
    assert_eq!(h.state().v, 51.0);
    assert!(h.state().committed);
    press(&mut h, Key::ArrowDown);
    press(&mut h, Key::ArrowDown);
    assert_eq!(h.state().v, 49.0);

    let mut h = numeric_harness(0.0);
    focus_field(&mut h);
    press(&mut h, Key::ArrowDown);
    assert_eq!(h.state().v, 0.0);
}

#[test]
fn champ_valide_a_la_perte_de_focus() {
    let mut h = numeric_harness(50.0);
    type_replace(&mut h, "75");
    assert_eq!(h.state().v, 50.0);
    h.get_by_label("Ailleurs").focus();
    h.run_steps(3);
    assert_eq!(h.state().v, 75.0);
    assert!(h.state().committed);
}

#[test]
fn champ_glisser_sur_l_unite_ajuste() {
    let mut h = numeric_harness(50.0);
    let center = h.get_by_label("mm").rect().center();
    h.input_mut().events.push(Event::PointerMoved(center));
    h.run_steps(2);
    let button = |pressed| Event::PointerButton {
        pos: center,
        button: PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    };
    h.input_mut().events.push(button(true));
    h.run_steps(2);
    for dx in [10.0, 20.0, 40.0] {
        h.input_mut()
            .events
            .push(Event::PointerMoved(Pos2::new(center.x + dx, center.y)));
        h.run_steps(2);
    }
    assert!(h.state().v > 50.0, "valeur après glisser : {}", h.state().v);
    h.state_mut().committed = false;
    h.input_mut().events.push(Event::PointerButton {
        pos: Pos2::new(center.x + 40.0, center.y),
        button: PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    h.run_steps(2);
    assert!(h.state().committed);
}

#[test]
fn tool_button_clic_et_libelle() {
    let mut h = Harness::builder()
        .with_size(egui::vec2(300.0, 200.0))
        .build_ui_state(
            |ui, clicks: &mut u32| {
                if tool_button(ui, "Segment", Some("S"), false).clicked() {
                    *clicks += 1;
                }
                ui.add_enabled_ui(false, |ui| {
                    if tool_button(ui, "Courbe", Some("C"), false).clicked() {
                        *clicks += 100;
                    }
                });
            },
            0u32,
        );
    h.run_steps(3);
    h.get_by_label("Segment").click();
    h.run_steps(3);
    assert_eq!(*h.state(), 1);
    h.get_by_label("Courbe").click();
    h.run_steps(3);
    assert_eq!(*h.state(), 1, "un bouton désactivé ne répond pas");
}

#[test]
fn toast_expire() {
    let mut toasts = Toasts::default();
    toasts.push(ToastLevel::Info, "a", 10.0);
    toasts.push(ToastLevel::Error, "b", 10.0);
    toasts.expire(13.9);
    assert_eq!(toasts.len(), 2);
    toasts.expire(14.0);
    assert_eq!(toasts.len(), 1);
    toasts.expire(19.9);
    assert_eq!(toasts.len(), 1);
    toasts.expire(20.0);
    assert!(toasts.is_empty());
}

#[test]
fn toast_affiche_le_niveau_en_texte() {
    let mut toasts = Toasts::default();
    toasts.push(ToastLevel::Error, "Référence manquante", 0.0);
    let mut h = Harness::builder()
        .with_size(egui::vec2(600.0, 300.0))
        .build_state(
            |ctx, t: &mut Toasts| {
                theme::apply(ctx, &Tokens::precision_light());
                t.show(ctx, 0.0);
            },
            toasts,
        );
    h.run_steps(3);
    h.get_by_label("Erreur");
    h.get_by_label("Référence manquante");
    h.get_by_label("Fermer").click();
    h.run_steps(3);
    assert!(h.state().is_empty());
}

#[test]
fn dialogue_renvoie_le_choix() {
    let make = || {
        Harness::builder()
            .with_size(egui::vec2(600.0, 400.0))
            .build_state(
                |ctx, choice: &mut Option<u8>| {
                    theme::apply(ctx, &Tokens::precision_light());
                    let actions = [
                        DialogAction::cancel("Annuler", 1u8),
                        DialogAction::danger("Supprimer", 2u8),
                    ];
                    if let Some(c) = confirm_dialog(ctx, "Titre", "Corps", &actions) {
                        *choice = Some(c);
                    }
                },
                None,
            )
    };
    let mut h = make();
    h.run_steps(3);
    h.get_by_label("Supprimer").click();
    h.run_steps(3);
    assert_eq!(*h.state(), Some(2));

    let mut h = make();
    h.run_steps(3);
    h.key_press(Key::Escape);
    h.run_steps(3);
    assert_eq!(*h.state(), Some(1), "Échap = action d'annulation");
}

#[test]
fn theme_memorise_les_jetons() {
    let ctx = egui::Context::default();
    assert!(!theme::tokens(&ctx).dark);
    theme::apply(&ctx, &Tokens::precision_dark());
    assert_eq!(theme::tokens(&ctx), Tokens::precision_dark());
}

#[test]
fn lab_s_affiche_sans_panique() {
    for size in [egui::vec2(900.0, 560.0), egui::vec2(2560.0, 1440.0)] {
        for dark in [false, true] {
            for page in LabPage::ALL {
                let mut state = LabState {
                    page,
                    dark,
                    ..LabState::default()
                };
                state.toasts.push(ToastLevel::Warning, "test", 0.0);
                state.dialog_open = page == LabPage::Messages;
                let mut h = Harness::builder()
                    .with_size(size)
                    .build_state(|ctx, s: &mut LabState| ui_lab(ctx, s), state);
                h.run_steps(4);
            }
        }
    }
}

#[test]
fn lab_navigation_par_bouton() {
    let mut h = Harness::builder()
        .with_size(egui::vec2(900.0, 560.0))
        .build_state(|ctx, s: &mut LabState| ui_lab(ctx, s), LabState::default());
    h.run_steps(3);
    h.get_by_label("Champs numériques").click();
    h.run_steps(3);
    assert_eq!(h.state().page, LabPage::Numeric);
}

/// US-16.8 : l'anneau de focus est peint (2 points, couleur du jeton) autour du seul widget qui a
/// le focus clavier.
#[test]
fn l_anneau_de_focus_est_peint_autour_du_widget_focalise() {
    let ctx = egui::Context::default();
    let tokens = Tokens::precision_light();
    theme::apply(&ctx, &tokens);
    let pass = |focus: bool| {
        let mut button = None;
        let out = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    Pos2::ZERO,
                    egui::vec2(400.0, 200.0),
                )),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let r = ui.button("OK");
                    if focus {
                        r.request_focus();
                    }
                    button = Some(r.rect);
                });
                theme::paint_focus_ring(ctx);
            },
        );
        (out, button.unwrap())
    };
    let rings = |out: &egui::FullOutput| {
        out.shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Rect(r)
                    if r.stroke.width >= 2.0 && r.stroke.color == tokens.focus_ring =>
                {
                    Some(r.rect)
                }
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    let (none, _) = pass(false);
    assert!(rings(&none).is_empty(), "pas de focus : pas d'anneau");
    pass(true);
    let (out, rect) = pass(false);
    assert_eq!(rings(&out), vec![rect.expand(1.0)]);
}
