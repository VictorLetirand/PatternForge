//! PH-2 « Interface lisible » : barre d'outils regroupée sur une rangée, sections repliables,
//! fenêtre de mise en page atteignable, symboles présents dans la police, nombres localisés.
//! Tests egui_kittest : la vraie boucle egui, hors écran.

use crate::app::App;
use crate::editor::Tool;
use crate::panels::ToolFamily;
use crate::Launch;
use eframe::egui;
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;

fn harness(stage: Option<&str>, size: [f32; 2]) -> Harness<'static, App> {
    let launch = Launch {
        demo: true,
        stage: stage.map(Into::into),
        ..Launch::default()
    };
    let app = App::with_context(&egui::Context::default(), launch);
    let mut h = Harness::builder()
        .with_size(egui::vec2(size[0], size[1]))
        .build_state(|ctx, app: &mut App| app.ui(ctx), app);
    h.run_steps(4);
    h
}

fn tools_panel_height(h: &Harness<'_, App>) -> f32 {
    egui::containers::panel::PanelState::load(&h.ctx, egui::Id::new("tools"))
        .expect("panneau des outils")
        .rect
        .height()
}

/// Outils atteignables depuis la barre : Sélection seule, puis chaque famille.
fn toolbar_tools() -> Vec<Tool> {
    let mut all = vec![Tool::Select];
    for family in ToolFamily::ALL {
        all.extend_from_slice(family.tools());
    }
    all
}

#[test]
fn le_regroupement_ne_perd_ni_ne_duplique_aucun_outil() {
    let grouped = toolbar_tools();
    assert_eq!(grouped.len(), Tool::ALL.len());
    for tool in Tool::ALL {
        assert_eq!(
            grouped.iter().filter(|t| **t == tool).count(),
            1,
            "{tool:?} doit être dans exactement une famille"
        );
    }
}

#[test]
fn la_barre_d_outils_tient_sur_une_rangee_a_1600_et_a_1280() {
    // Une rangée : 28 px de bouton + marges ; deux rangées dépasseraient 70 px.
    for size in [[1600.0, 1000.0], [1280.0, 800.0]] {
        let h = harness(None, size);
        let height = tools_panel_height(&h);
        assert!(
            height < 56.0,
            "{size:?} : barre de {height} px (plus d'une rangée)"
        );
    }
    // Même avec un outil à modes (Point) actif, en français comme en anglais.
    for lang in [pf_i18n::Lang::Fr, pf_i18n::Lang::En] {
        pf_i18n::set_lang(lang);
        let mut h = harness(None, [1280.0, 800.0]);
        h.state_mut().editor.set_tool(Tool::Point);
        h.run_steps(3);
        let height = tools_panel_height(&h);
        assert!(
            height < 56.0,
            "{lang:?} : barre de {height} px avec les modes du point"
        );
    }
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
}

#[test]
fn annuler_retablir_et_cadrer_restent_au_meme_endroit() {
    let mut h = harness(None, [1600.0, 1000.0]);
    let fit = |h: &Harness<'_, App>| h.get_by_label("Cadrer").rect();
    let before = fit(&h);
    h.state_mut().editor.set_tool(Tool::Circle);
    h.run_steps(3);
    assert_eq!(
        before,
        fit(&h),
        "Cadrer ne bouge pas quand les modes changent"
    );
    assert!(before.right() > 1500.0, "à droite de la barre");
    h.get_by_label("Annuler");
    h.get_by_label("Rétablir");
}

#[test]
fn chaque_outil_est_atteignable_depuis_la_barre() {
    let mut h = harness(None, [1280.0, 800.0]);
    for tool in Tool::ALL {
        let label = tool.label();
        if tool != Tool::Select {
            let family = ToolFamily::ALL
                .into_iter()
                .find(|f| f.tools().contains(&tool))
                .expect("famille");
            h.get_all_by_role_and_label(egui::accesskit::Role::Button, &family.label())
                .next()
                .expect("bouton de famille")
                .click();
            h.run_steps(3);
        }
        h.get_all_by_role_and_label(egui::accesskit::Role::Button, &label)
            .last()
            .unwrap_or_else(|| panic!("bouton de l'outil {label}"))
            .click();
        h.run_steps(3);
        assert_eq!(h.state().editor.tool, tool, "outil {label}");
    }
}

#[test]
fn les_lettres_des_raccourcis_sont_toujours_affichees_dans_les_menus_de_famille() {
    let mut h = harness(None, [1280.0, 800.0]);
    h.get_all_by_role_and_label(egui::accesskit::Role::Button, &ToolFamily::Draw.label())
        .next()
        .unwrap()
        .click();
    h.run_steps(3);
    // Le bouton d'outil porte son libellé ; la lettre est celle de la table des raccourcis.
    assert_eq!(Tool::Segment.shortcut(), "S");
    h.get_all_by_role_and_label(egui::accesskit::Role::Button, &Tool::Segment.label())
        .last()
        .expect("Segment");
}

#[test]
fn les_sections_de_la_fiche_sont_repliees_par_defaut_puis_depliables() {
    let mut h = harness(Some("fiche-riche"), [1600.0, 1000.0]);
    let inside = "Ajouter un marquage…";
    // Les titres sont là, le contenu non.
    h.get_by_label("Perçages et pointages");
    h.get_by_label("Boutons et boutonnières");
    assert!(
        h.query_by_label(inside).is_none(),
        "section repliée par défaut"
    );
    h.get_by_label("Perçages et pointages").click();
    h.run_steps(3);
    h.get_by_label(inside);
    // L'état replié est une préférence d'interface : rien n'est écrit dans le projet.
    assert!(
        !h.state().editor.doc.is_dirty(),
        "déplier ne modifie pas le projet"
    );
    // Se replie à nouveau.
    h.get_by_label("Perçages et pointages").click();
    h.run_steps(3);
    assert!(h.query_by_label(inside).is_none());
}

#[test]
fn la_section_liee_a_la_selection_s_ouvre_d_elle_meme() {
    let mut h = harness(Some("fiche-riche"), [1600.0, 1000.0]);
    let point = h.state().editor.sheet().points[0].id;
    assert!(h.query_by_label("Ajouter un marquage…").is_none());
    h.state_mut().editor.selection.points = vec![point];
    h.run_steps(4);
    h.get_by_label("Ajouter un marquage…");
}

#[test]
fn la_fenetre_de_mise_en_page_est_entierement_atteignable() {
    let mut h = harness(Some("impression"), [1280.0, 800.0]);
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 800.0));
    let apply = h.get_by_label("Appliquer la mise en page").rect();
    assert!(
        screen.contains_rect(apply),
        "Appliquer doit rester dans l'écran : {apply:?}"
    );
    // Contenu de la sortie déplié : la fenêtre défile, Appliquer reste visible.
    h.get_by_label("Contenu de la sortie").click();
    h.run_steps(4);
    let apply = h.get_by_label("Appliquer la mise en page").rect();
    assert!(
        screen.contains_rect(apply),
        "Appliquer reste visible, section dépliée : {apply:?}"
    );
}

#[test]
fn l_aide_tient_dans_l_ecran_sous_la_barre_d_outils() {
    let h = harness(Some("aide"), [1280.0, 800.0]);
    let title = h.get_by_label("Raccourcis et limites").rect();
    let top = h.get_by_label("Annuler").rect().max.y;
    assert!(
        title.min.y >= top,
        "sous le menu et la barre d'outils : {title:?}"
    );
    assert!(title.max.y < 800.0);
}

#[test]
fn aucun_catalogue_ne_contient_de_symbole_absent_de_la_police() {
    for lang in pf_i18n::Lang::ALL {
        for (key, value) in lang.catalog() {
            for glyph in ['→', '▲', '▼'] {
                assert!(
                    !value.contains(glyph),
                    "{lang:?} / {key} contient « {glyph} » (carré vide dans la police)"
                );
            }
        }
    }
}

#[test]
fn le_formateur_des_champs_a_glisser_suit_la_langue() {
    pf_core::input::set_decimal_separator(',');
    assert_eq!(crate::text::drag_text(42.0, 3), "42,0");
    assert_eq!(crate::text::drag_text(8.75, 3), "8,75");
    assert_eq!(crate::text::drag_text(-0.0, 3), "0,0");
    pf_core::input::set_decimal_separator('.');
    assert_eq!(crate::text::drag_text(42.0, 3), "42.0");
    pf_core::input::set_decimal_separator(',');
}
