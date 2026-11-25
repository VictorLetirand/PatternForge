//! US-16.8 : accessibilité (contraste, focus visible, échelle, noms accessibles, clavier).
//!
//! Les valeurs de contraste sont recalculées avec une formule écrite ici ; ce qui exige un vrai
//! lecteur d'écran (annonces NVDA, ordre de lecture) est **[HUMAIN]** et n'est pas affirmé.

use crate::app::App;
use crate::Launch;
use eframe::egui::{self, accesskit::Role, Key, Modifiers};
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;

fn launch_stage(stage: &str) -> Launch {
    Launch {
        demo: true,
        stage: (!stage.is_empty()).then(|| stage.to_string()),
        ..Launch::default()
    }
}

/// Harnais à l'échelle `pct` : fenêtre de 1280 x 800 pixels natifs ; egui en tire (1280 / z) x
/// (800 / z) points une fois le facteur de zoom appliqué.
fn harness_scaled(pct: u32, stage: &str) -> Harness<'static, App> {
    let mut app = App::with_context(&egui::Context::default(), launch_stage(stage));
    app.prefs.ui_scale_pct = pct;
    let mut h = Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .build_state(|ctx, app: &mut App| app.ui(ctx), app);
    h.run_steps(8);
    h
}

fn is_interactive(role: Role) -> bool {
    matches!(
        role,
        Role::Button
            | Role::CheckBox
            | Role::RadioButton
            | Role::TextInput
            | Role::MultilineTextInput
            | Role::ComboBox
            | Role::Slider
            | Role::SpinButton
            | Role::MenuItem
            | Role::Tab
            | Role::Link
            | Role::Switch
    )
}

// ----- contraste ------------------------------------------------------------------------------

/// Luminance relative WCAG 2.x, formule écrite ici (indépendante de `pf_ui::tokens`).
fn luminance(c: egui::Color32) -> f64 {
    let lin = |v: u8| {
        let s = f64::from(v) / 255.0;
        if s <= 0.03928 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(c.r()) + 0.7152 * lin(c.g()) + 0.0722 * lin(c.b())
}

fn ratio(a: egui::Color32, b: egui::Color32) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// Superpose une couleur translucide sur un fond opaque.
fn over(fg: egui::Color32, bg: egui::Color32) -> egui::Color32 {
    let a = f32::from(fg.a()) / 255.0;
    let mix = |f: u8, b: u8| (f32::from(f) * a + f32::from(b) * (1.0 - a)).round() as u8;
    egui::Color32::from_rgb(
        mix(fg.r(), bg.r()),
        mix(fg.g(), bg.g()),
        mix(fg.b(), bg.b()),
    )
}

#[test]
fn la_formule_de_contraste_est_juste_sur_des_valeurs_calculees_a_la_main() {
    // Noir sur blanc : (1,0 + 0,05) / (0 + 0,05) = 21.
    assert!((ratio(egui::Color32::WHITE, egui::Color32::BLACK) - 21.0).abs() < 1e-9);
    // Gris #767676 sur blanc : canal 118/255 = 0,4627 ; ((0,4627+0,055)/1,055)^2,4 = 0,18115 ;
    // 1,05 / (0,18115 + 0,05) = 4,5424.
    let grey = egui::Color32::from_rgb(0x76, 0x76, 0x76);
    assert!((ratio(egui::Color32::WHITE, grey) - 4.5424).abs() < 0.001);
    // #777777 est juste en dessous de 4,5 (4,48) : la barre se joue à un cran.
    assert!(
        ratio(
            egui::Color32::WHITE,
            egui::Color32::from_rgb(0x77, 0x77, 0x77)
        ) < 4.5
    );
    // Texte principal clair #14263D sur blanc : luminance 0,018724 (calcul à la main), donc
    // 1,05 / 0,068724 = 15,28.
    let ink = egui::Color32::from_rgb(0x14, 0x26, 0x3D);
    assert!((ratio(egui::Color32::WHITE, ink) - 15.28).abs() < 0.02);
}

/// Paires texte / fond de toutes les familles de composants, pour un thème.
fn text_pairs(dark: bool) -> Vec<(&'static str, egui::Color32, egui::Color32)> {
    crate::palette::set_dark(dark);
    let t = if dark {
        pf_ui::tokens::Tokens::precision_dark()
    } else {
        pf_ui::tokens::Tokens::precision_light()
    };
    let pal = crate::palette::pal();
    let hover_fill = t.selection_bg;
    let ctx = egui::Context::default();
    pf_ui::theme::apply(&ctx, &t);
    let link = ctx.style().visuals.hyperlink_color;
    vec![
        ("panneau : texte / panneau", t.text_primary, t.surface_panel),
        (
            "panneau : texte / fenetre",
            t.text_primary,
            t.surface_raised,
        ),
        (
            "panneau : secondaire / panneau",
            t.text_secondary,
            t.surface_panel,
        ),
        (
            "panneau : secondaire / fenetre",
            t.text_secondary,
            t.surface_raised,
        ),
        ("bouton survole : texte", t.text_primary, hover_fill),
        ("bouton survole : secondaire", t.text_secondary, hover_fill),
        ("bouton plein : repos", t.action_text, t.action_strong),
        ("bouton plein : survol", t.action_text, t.action_hover),
        ("bouton plein : appui", t.action_text, t.action_pressed),
        ("lien : fenetre", link, t.surface_raised),
        ("lien : panneau", link, t.surface_panel),
        ("danger : fenetre", t.danger, t.surface_raised),
        ("danger : panneau", t.danger, t.surface_panel),
        ("avertissement : fenetre", t.warning, t.surface_raised),
        ("avertissement : panneau", t.warning, t.surface_panel),
        ("succes : fenetre", t.success, t.surface_raised),
        ("succes : panneau", t.success, t.surface_panel),
        ("aide : discret / panneau", pal.muted, t.surface_panel),
        ("aide : discret / fenetre", pal.muted, t.surface_raised),
        ("aide : erreur / panneau", pal.error, t.surface_panel),
        ("aide : erreur / fenetre", pal.error, t.surface_raised),
        ("aide : accent / panneau", pal.accent_text, t.surface_panel),
        ("canevas : encre", pal.ink, t.surface_canvas),
        ("canevas : graduations", pal.grid_text, t.surface_canvas),
        (
            "canevas : etiquette",
            pal.ink,
            over(pal.label_bg, t.surface_canvas),
        ),
    ]
}

#[test]
fn contraste_texte_fond_d_au_moins_4_5_dans_les_deux_themes() {
    let mut failures = Vec::new();
    let mut checked = 0;
    for dark in [false, true] {
        for (name, fg, bg) in text_pairs(dark) {
            checked += 1;
            let r = ratio(fg, bg);
            if r < 4.5 {
                failures.push(format!(
                    "{} {name} : {r:.2}",
                    if dark { "sombre" } else { "clair" }
                ));
            }
        }
    }
    crate::palette::set_dark(false);
    assert!(failures.is_empty(), "contraste < 4,5 : {failures:#?}");
    assert_eq!(checked, 50, "25 paires par theme");
}

#[test]
fn le_texte_desactive_est_la_seule_exception_declaree() {
    // WCAG 1.4.3 exempte les composants inactifs : on ne l'exige pas, on le mesure seulement.
    let t = pf_ui::tokens::Tokens::precision_light();
    assert!(ratio(t.text_disabled, t.surface_panel) < 4.5);
}

// ----- focus visible --------------------------------------------------------------------------

#[test]
fn l_anneau_de_focus_fait_2_points_au_moins_et_contraste_3_pour_1() {
    for dark in [false, true] {
        let t = if dark {
            pf_ui::tokens::Tokens::precision_dark()
        } else {
            pf_ui::tokens::Tokens::precision_light()
        };
        for bg in [t.surface_panel, t.surface_raised, t.surface_canvas] {
            assert!(ratio(t.focus_ring, bg) >= 3.0, "dark={dark}");
        }
    }
    const { assert!(pf_ui::theme::FOCUS_RING_WIDTH >= 2.0) };
}

#[test]
fn le_widget_qui_a_le_focus_clavier_recoit_un_anneau_de_2_points() {
    let mut h = harness_scaled(100, "");
    assert!(
        pf_ui::theme::focus_ring(&h.ctx).is_none(),
        "rien n'a le focus"
    );
    for _ in 0..3 {
        h.key_press(Key::Tab);
        h.run_steps(2);
    }
    let focused = h
        .ctx
        .memory(|m| m.focused())
        .expect("Tab donne le focus à un widget");
    let (rect, stroke) = pf_ui::theme::focus_ring(&h.ctx).expect("anneau");
    assert!(stroke.width >= 2.0, "{}", stroke.width);
    let widget = h
        .ctx
        .read_response(focused)
        .expect("réponse du widget")
        .interact_rect;
    assert_eq!(rect, widget.expand(1.0));
}

// ----- échelle 100 à 200 % --------------------------------------------------------------------

fn panel(h: &Harness<'static, App>, id: &str) -> egui::Rect {
    egui::containers::panel::PanelState::load(&h.ctx, egui::Id::new(id))
        .map(|s| s.rect)
        .unwrap_or_else(|| panic!("panneau {id} absent"))
}

/// Rectangles des fenêtres flottantes ouvertes (zones d'ordre `Middle`).
fn floating(h: &Harness<'static, App>) -> Vec<(egui::LayerId, egui::Rect)> {
    h.ctx.memory(|m| {
        m.areas()
            .visible_layer_ids()
            .into_iter()
            .filter(|l| l.order == egui::Order::Middle)
            .filter_map(|l| m.area_rect(l.id).map(|r| (l, r)))
            .collect()
    })
}

/// Défauts d'échelle : barres fixes (menu, outils, état) dans la fenêtre, contrôles de ces barres
/// dedans, fenêtres flottantes dedans. Le contenu des panneaux latéraux et des fenêtres défile : il
/// n'est pas mesuré ici (sa portée au défilement est celle d'egui).
fn scale_problems(pct: u32, stage: &str) -> Vec<String> {
    let zoom = pct as f32 / 100.0;
    let screen =
        egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0 / zoom, 800.0 / zoom));
    let h = harness_scaled(pct, stage);
    let mut out = Vec::new();
    if (h.ctx.pixels_per_point() - zoom).abs() > 1e-4 {
        out.push(format!(
            "pixels_per_point {} au lieu de {zoom}",
            h.ctx.pixels_per_point()
        ));
    }
    let status = panel(&h, "status");
    if !screen.expand(0.5).contains_rect(status) {
        out.push(format!("barre d'état {status:?} hors de {screen:?}"));
    }
    let bands: Vec<(&str, egui::Rect)> = ["menu", "tools"]
        .into_iter()
        .map(|id| (id, panel(&h, id)))
        .collect();
    for (id, r) in &bands {
        if !screen.expand(0.5).contains_rect(*r) {
            out.push(format!("barre {id} {r:?} hors de la fenêtre {screen:?}"));
        }
    }
    let windows: Vec<egui::Rect> = floating(&h).into_iter().map(|(_, r)| r).collect();
    for node in h.root().children_recursive() {
        let ak = node.accesskit_node();
        if !is_interactive(ak.role()) {
            continue;
        }
        // Les rectangles AccessKit sont en pixels : retour aux points.
        let px = node.rect();
        let r = egui::Rect::from_min_max(px.min / zoom, px.max / zoom);
        // Les contrôles d'une fenêtre flottante (titre, fermeture) ne sont pas ceux d'une barre.
        if windows.iter().any(|w| w.contains(r.center())) {
            continue;
        }
        if let Some((id, band)) = bands.iter().find(|(_, b)| b.contains(r.center())) {
            if !band.expand(0.5).contains_rect(r) || !screen.expand(0.5).contains_rect(r) {
                out.push(format!("{id} : {:?} {:?} sort de {band:?}", ak.label(), r));
            }
        }
    }
    for (layer, rect) in floating(&h) {
        if !screen.expand(0.5).contains_rect(rect) {
            out.push(format!("fenêtre {layer:?} {rect:?} sort de {screen:?}"));
        }
    }
    out
}

const SCALE_STAGES: [&str; 6] = [
    "",
    "preferences",
    "aide",
    "journal-erreurs",
    "glossaire",
    "recherche",
];

/// Contrôles interactifs sans nom accessible (ni étiquette).
fn unnamed(h: &Harness<'static, App>) -> Vec<String> {
    h.root()
        .children_recursive()
        .filter_map(|node| {
            let ak = node.accesskit_node();
            (is_interactive(ak.role()) && ak.label().is_none_or(|l| l.trim().is_empty()))
                .then(|| format!("{:?} valeur={:?}", ak.role(), ak.value()))
        })
        .collect()
}

#[test]
fn l_interface_tient_dans_la_fenetre_de_1280_par_800_a_100_150_et_200_pour_cent() {
    for pct in [100u32, 150, 200] {
        for stage in SCALE_STAGES {
            let problems = scale_problems(pct, stage);
            assert!(problems.is_empty(), "{pct} % [{stage}] : {problems:#?}");
        }
    }
}

#[test]
fn l_echelle_se_regle_dans_les_preferences_et_reste_entre_100_et_200() {
    use crate::prefs::{Prefs, UI_SCALE_MAX_PCT, UI_SCALE_MIN_PCT};
    assert_eq!(Prefs::default().ui_scale_pct, 100);
    assert_eq!((UI_SCALE_MIN_PCT, UI_SCALE_MAX_PCT), (100, 200));
    let json =
        |n: serde_json::Value| Prefs::from_json_report(&serde_json::json!({ "ui_scale_pct": n }));
    assert_eq!(json(serde_json::json!(150)).0.ui_scale_pct, 150);
    for bad in [
        serde_json::json!(99),
        serde_json::json!(201),
        serde_json::json!("150"),
        serde_json::json!(150.5),
    ] {
        let (prefs, ignored) = json(bad);
        assert_eq!((prefs.ui_scale_pct, ignored), (100, 1));
    }
    // Aller-retour par le réglage enregistré.
    let mut p = Prefs::default();
    p.ui_scale_pct = 175;
    assert_eq!(Prefs::from_json(&p.to_json()).ui_scale_pct, 175);
    // Le facteur est appliqué à `pixels_per_point` : 150 % donne 1,5 (natif 1,0 en test).
    let h = harness_scaled(150, "");
    assert!((h.ctx.pixels_per_point() - 1.5).abs() < 1e-4);
}

// ----- noms accessibles -----------------------------------------------------------------------

/// Dette connue (contrôles de tableaux sans étiquette de ligne : barème, cotes, repères, cran,
/// variables, valeurs par taille…) : le compte par écran ne doit **jamais augmenter** ; une baisse
/// oblige à mettre ce tableau à jour. Tous les autres écrans : zéro.
const UNNAMED_BASELINE: [(&str, usize); 10] = [
    ("cran-coupe", 3),
    ("cran-coin", 2),
    ("taille-valeurs", 2),
    ("bareme", 10),
    ("cote", 1),
    ("repere", 3),
    ("transformer", 2),
    ("assembler", 1),
    ("variables", 1),
    ("longueur-lisse", 4),
];

#[test]
fn aucun_controle_interactif_sans_nom_accessible_au_dela_de_la_dette_declaree() {
    let mut debt = 0;
    for stage in crate::stages::STAGES {
        let h = harness_scaled(100, stage);
        let n = unnamed(&h);
        let allowed = UNNAMED_BASELINE
            .iter()
            .find(|(s, _)| s == stage)
            .map_or(0, |(_, c)| *c);
        debt += n.len();
        assert_eq!(n.len(), allowed, "{stage} : {n:?}");
    }
    assert_eq!(debt, UNNAMED_BASELINE.iter().map(|(_, c)| c).sum::<usize>());
}

#[test]
fn les_noms_sont_les_etiquettes_voisines_dans_les_deux_langues() {
    for (lang, expect) in [
        (pf_i18n::Lang::Fr, "Longueur (champ)"),
        (pf_i18n::Lang::En, "Length (field)"),
    ] {
        pf_i18n::set_lang(lang);
        let h = harness_scaled(100, "segment");
        let names: Vec<String> = h
            .root()
            .children_recursive()
            .filter(|n| n.accesskit_node().role() == Role::TextInput)
            .filter_map(|n| n.accesskit_node().label())
            .collect();
        assert!(names.iter().any(|n| n == expect), "{lang:?} : {names:?}");
    }
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
    // Les boutons d'icône de la barre d'outils portent déjà un nom ; le curseur d'échelle aussi.
    let mut h = harness_scaled(100, "preferences");
    h.run_steps(2);
    assert!(h
        .query_all_by_label(&format!("{} (champ)", pf_i18n::tr("prefs.scale")))
        .any(|n| n.accesskit_node().role() == Role::SpinButton));
}

// ----- clavier --------------------------------------------------------------------------------

fn focused_id(h: &Harness<'static, App>) -> Option<egui::Id> {
    h.ctx.memory(|m| m.focused())
}

#[test]
fn tab_et_maj_tab_parcourent_les_controles_dans_les_deux_sens() {
    let mut h = harness_scaled(100, "");
    let mut forward = Vec::new();
    for _ in 0..12 {
        h.key_press(Key::Tab);
        h.run_steps(2);
        forward.push(focused_id(&h).expect("un widget a le focus"));
    }
    let mut distinct = forward.clone();
    distinct.sort_by_key(|id| id.value());
    distinct.dedup();
    assert!(
        distinct.len() >= 8,
        "seulement {} arrêts de focus",
        distinct.len()
    );
    // Maj+Tab revient au widget précédent.
    h.key_press_modifiers(Modifiers::SHIFT, Key::Tab);
    h.run_steps(2);
    assert_eq!(focused_id(&h), Some(forward[forward.len() - 2]));
}

#[test]
fn entree_et_espace_activent_le_bouton_qui_a_le_focus() {
    for key in [Key::Enter, Key::Space] {
        let mut h = harness_scaled(100, "preferences");
        h.state_mut().prefs.pick_radius_px = 12.0;
        h.run_steps(2);
        let reset = pf_i18n::tr("prefs.reset");
        h.get_by_label(&reset).focus();
        h.run_steps(2);
        assert!(h.get_by_label(&reset).is_focused());
        h.key_press(key);
        h.run_steps(3);
        assert_eq!(h.state().prefs, crate::prefs::Prefs::default(), "{key:?}");
    }
}

#[test]
fn echap_ferme_chaque_fenetre_flottante_sans_toucher_au_document() {
    for stage in [
        "preferences",
        "aide",
        "glossaire",
        "historique",
        "journal-erreurs",
        "impression",
        "dxf-export",
        "hpgl-export",
        "rapport",
        "recherche",
    ] {
        let mut h = harness_scaled(100, stage);
        let before = pf_core::persist::to_json(h.state().editor.project()).unwrap();
        let open = |h: &Harness<'static, App>| {
            let s = h.state();
            s.show_prefs
                || s.show_help
                || s.show_glossary
                || s.show_history
                || s.show_error_log
                || s.print_dialog.is_some()
                || s.dxf_dialog.is_some()
                || s.hpgl_dialog.is_some()
                || s.report_dialog.is_some()
                || s.search.is_some()
        };
        assert!(open(&h), "{stage} : aucune fenêtre ouverte");
        for _ in 0..4 {
            if !open(&h) {
                break;
            }
            h.key_press(Key::Escape);
            h.run_steps(3);
        }
        assert!(!open(&h), "{stage} : Échap ne ferme pas la fenêtre");
        assert_eq!(
            pf_core::persist::to_json(h.state().editor.project()).unwrap(),
            before,
            "{stage}"
        );
        assert!(!h.state().editor.doc.is_dirty(), "{stage}");
    }
}
