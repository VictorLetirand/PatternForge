//! US-16.5 - vues : unités d'affichage (mm, cm, pouces fractionnaires), grille et règles,
//! zoom sur la sélection, vues nommées. Une valeur à la main par critère, puis une campagne
//! adverse à graine fixe avec oracle indépendant et compteurs de couverture assertés.

use crate::app::App;
use crate::views::{self, GridSettings, NamedView, ViewSettings};
use crate::Launch;
use eframe::egui::{self, Key};
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use pf_core::units::{
    display_units, format_field, format_length, parse_length_with, set_display_units, DisplayUnits,
    LengthUnit, INCH_DENOMINATORS, MM_PER_INCH,
};
use pf_core::{Command, LayerId};
use pf_geometry::PointMm;
use serde_json::{json, Value};
use std::path::PathBuf;

/// Remet l'unité d'affichage du fil à mm en sortie de test.
struct UnitGuard;

impl Drop for UnitGuard {
    fn drop(&mut self) {
        set_display_units(DisplayUnits::default());
    }
}

fn inches(denominator: u32) -> DisplayUnits {
    DisplayUnits {
        unit: LengthUnit::In,
        inch_denominator: denominator,
    }
}

fn near(a: f64, b: f64, tol: f64) {
    assert!((a - b).abs() <= tol, "{a} != {b} (tolérance {tol})");
}

/// Dossier temporaire propre au fil d'exécution (les tests FR et EN tournent en parallèle).
fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "pf-us165-{}-{:?}-{tag}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("dossier temporaire");
    dir
}

fn app() -> App {
    App::with_context(
        &egui::Context::default(),
        Launch {
            demo: true,
            ..Launch::default()
        },
    )
}

fn harness_of(app: App) -> Harness<'static, App> {
    let mut h = Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .build_state(|ctx, app: &mut App| app.ui(ctx), app);
    h.run_steps(4);
    h
}

// ===== (a) unités d'affichage : valeurs à la main ================================================

#[test]
fn un_pouce_fait_25_4_mm_et_les_fractions_valent_leur_valeur() {
    near(parse_length_with("1", LengthUnit::In).unwrap(), 25.4, 1e-12);
    near(
        parse_length_with("3/8", LengthUnit::In).unwrap(),
        9.525,
        1e-12,
    );
    near(
        parse_length_with("1 1/16", LengthUnit::In).unwrap(),
        26.9875,
        1e-12,
    );
    near(
        parse_length_with("25,4 mm", LengthUnit::In).unwrap(),
        25.4,
        1e-12,
    );
    near(
        parse_length_with("2,54 cm", LengthUnit::In).unwrap(),
        25.4,
        1e-12,
    );
    assert_eq!(format_length(25.4, inches(16)), "1\"");
    assert_eq!(format_length(9.525, inches(16)), "3/8\"");
    assert_eq!(format_length(26.9875, inches(16)), "1 1/16\"");
}

#[test]
fn la_precision_choisie_arrondit_et_le_signale() {
    // 1 po 1/16 à la précision 1/8 : 1 1/8 (arrondi signalé) ; à 1/16 : exact.
    assert_eq!(format_length(26.9875, inches(8)), "≈ 1 1/8\"");
    assert_eq!(format_length(26.9875, inches(16)), "1 1/16\"");
    assert_eq!(format_length(26.9875, inches(64)), "1 1/16\"");
}

#[test]
fn le_texte_arrondi_n_est_jamais_renvoye_au_modele() {
    // Le texte d'un champ ne perd pas la valeur : 10 mm reste 10 mm même à 1/2 po.
    let _guard = UnitGuard;
    for d in INCH_DENOMINATORS {
        set_display_units(inches(d));
        let shown = format_length(10.0, display_units());
        assert!(shown.starts_with('≈'), "{shown}");
        let field = format_field(10.0, display_units());
        near(
            parse_length_with(&field, LengthUnit::In).unwrap(),
            10.0,
            1e-4,
        );
    }
}

#[test]
fn la_palette_lit_la_saisie_dans_l_unite_choisie() {
    let _guard = UnitGuard;
    let mut app = app();
    app.editor.length_text = "3/8".into();
    assert!(app.editor.typed_length().is_err(), "fraction refusée en mm");
    set_display_units(inches(16));
    near(app.editor.typed_length().unwrap().unwrap(), 9.525, 1e-12);
    app.editor.length_text = "1 1/16".into();
    near(app.editor.typed_length().unwrap().unwrap(), 26.9875, 1e-12);
    // Sans unité = pouces ; une unité explicite l'emporte.
    app.editor.length_text = "10 mm".into();
    near(app.editor.typed_length().unwrap().unwrap(), 10.0, 1e-12);
    app.editor.length_text = "2".into();
    near(app.editor.typed_length().unwrap().unwrap(), 50.8, 1e-12);
    set_display_units(DisplayUnits {
        unit: LengthUnit::Cm,
        inch_denominator: 16,
    });
    near(app.editor.typed_length().unwrap().unwrap(), 20.0, 1e-12);
}

#[test]
fn les_coordonnees_saisies_suivent_l_unite() {
    let _guard = UnitGuard;
    set_display_units(inches(16));
    let c = pf_core::input::parse_coordinate("1;1/2", PointMm::ORIGIN).unwrap();
    near(c.point().x, 25.4, 1e-12);
    near(c.point().y, 12.7, 1e-12);
    set_display_units(DisplayUnits::default());
    let c = pf_core::input::parse_coordinate("1;1/2", PointMm::ORIGIN);
    assert!(c.is_err(), "fraction refusée en mm");
}

#[test]
fn le_modele_et_les_exports_ne_changent_pas_avec_l_unite() {
    let _guard = UnitGuard;
    let project = pf_core::demo_project().expect("démo");
    let sheet = project.sheets[0].id;
    let svg_mm =
        pf_export::svg::export_svg(&pf_export::Scene::from_sheet(&project, sheet).expect("scène"))
            .expect("svg");
    let json_mm = serde_json::to_string(&project).expect("json");
    for d in [8, 16] {
        set_display_units(inches(d));
        let svg = pf_export::svg::export_svg(
            &pf_export::Scene::from_sheet(&project, sheet).expect("scène"),
        )
        .expect("svg");
        assert_eq!(svg, svg_mm, "SVG identique à l'octet en pouces 1/{d}");
        assert_eq!(serde_json::to_string(&project).unwrap(), json_mm);
    }
}

#[test]
fn l_affichage_en_pouces_ne_modifie_pas_le_projet() {
    let _guard = UnitGuard;
    let mut app = app();
    app.view_settings.units = inches(16);
    app.view_settings.rulers = true;
    app.editor.show_rulers = true;
    let before = serde_json::to_string(app.editor.project()).expect("json");
    let mut h = harness_of(app);
    h.run_steps(6);
    assert_eq!(display_units(), inches(16), "l'unité est publiée à l'écran");
    let after = serde_json::to_string(h.state().editor.project()).expect("json");
    assert_eq!(before, after);
    assert!(!h.state().editor.doc.can_undo(), "aucune étape créée");
}

#[test]
fn le_champ_a_glisser_est_inchange_en_mm_et_suit_l_unite_sinon() {
    let _guard = UnitGuard;
    // En mm : aucun formateur ajouté (le texte reste celui d'avant).
    let mut v = 12.7;
    let ctx = egui::Context::default();
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add(views::length_drag(
                egui::DragValue::new(&mut v).suffix(" mm"),
            ));
        });
    });
    assert_eq!(v, 12.7);
    set_display_units(inches(16));
    let ctx = egui::Context::default();
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add(views::length_drag(
                egui::DragValue::new(&mut v).suffix(" mm"),
            ));
        });
    });
    assert_eq!(v, 12.7, "afficher ne modifie pas la valeur");
}

// ===== (b) grille et règles =======================================================================

#[test]
fn la_grille_par_defaut_reproduit_l_ancien_affichage() {
    let g = GridSettings::default_for(LengthUnit::Mm);
    assert_eq!((g.step_mm, g.subdivisions, g.visible), (50.0, 5, true));
    let z100 = crate::view::PX_PER_MM_AT_100;
    assert_eq!(g.levels(z100), (10.0, 50.0));
    // Zoom serré : 10 mm < 7 px, le pas fin devient 50 mm puis 100 mm (comme avant).
    assert_eq!(g.levels(0.5), (50.0, 250.0));
    assert_eq!(g.levels(0.2), (50.0, 250.0));
    assert_eq!(g.levels(0.1), (100.0, 500.0));
    assert_eq!(g.levels(0.01), (1000.0, 5000.0));
    assert_eq!(g.levels(0.001), (5000.0, 25_000.0));
}

#[test]
fn la_grille_est_parametrable() {
    let g = GridSettings {
        visible: true,
        step_mm: 25.4,
        subdivisions: 4,
    };
    let (minor, major) = g.levels(crate::view::PX_PER_MM_AT_100);
    near(minor, 6.35, 1e-12);
    near(major, 25.4, 1e-12);
    // Bornes : pas nul, négatif ou infini ramenés dans l'intervalle.
    let c = GridSettings {
        visible: true,
        step_mm: 0.0,
        subdivisions: 0,
    }
    .clamped();
    assert_eq!((c.step_mm, c.subdivisions), (views::GRID_STEP_MIN_MM, 1));
    let c = GridSettings {
        visible: true,
        step_mm: f64::INFINITY,
        subdivisions: 999,
    }
    .clamped();
    assert_eq!((c.step_mm, c.subdivisions), (50.0, views::SUBDIVISIONS_MAX));
}

#[test]
fn changer_d_unite_change_la_grille_par_defaut_seulement() {
    let mut vs = ViewSettings::default();
    vs.set_unit(LengthUnit::In);
    assert_eq!(vs.grid, GridSettings::default_for(LengthUnit::In));
    vs.set_unit(LengthUnit::Mm);
    assert_eq!(vs.grid, GridSettings::default_for(LengthUnit::Mm));
    vs.grid.step_mm = 20.0;
    vs.grid.visible = false;
    vs.set_unit(LengthUnit::In);
    assert_eq!(
        (vs.grid.step_mm, vs.grid.visible),
        (20.0, false),
        "grille réglée gardée"
    );
}

#[test]
fn les_regles_graduent_selon_l_unite() {
    let z100 = crate::view::PX_PER_MM_AT_100;
    assert_eq!(views::ruler_major_mm(LengthUnit::Mm, z100), 20.0);
    near(views::ruler_major_mm(LengthUnit::Cm, z100), 20.0, 1e-9);
    near(views::ruler_major_mm(LengthUnit::In, z100), 25.4, 1e-9);
    assert_eq!(
        views::ruler_label(40.0, 20.0, DisplayUnits::default()),
        "40,0"
    );
    assert_eq!(
        views::ruler_label(
            40.0,
            20.0,
            DisplayUnits {
                unit: LengthUnit::Cm,
                inch_denominator: 16
            }
        ),
        "4,0"
    );
    assert_eq!(views::ruler_label(38.1, 25.4, inches(16)), "1 1/2");
    assert_eq!(views::ruler_label(-12.7, 25.4, inches(16)), "-1/2");
    // Jamais de pas nul : chaque unité propose des graduations strictement croissantes.
    for unit in LengthUnit::ALL {
        let c = views::ruler_candidates_mm(unit);
        assert!(c.windows(2).all(|w| w[0] < w[1] && w[0] > 0.0), "{unit:?}");
    }
}

// ===== (c) zoom sur sélection =====================================================================

#[test]
fn le_zoom_sur_la_selection_cadre_la_selection_sans_toucher_au_projet() {
    let mut app = app();
    let sheet = app.editor.sheet().id;
    let a = app.editor.exec(Command::AddPoint {
        sheet,
        at: PointMm::new(1000.0, 1000.0),
    });
    let b = app.editor.exec(Command::AddPoint {
        sheet,
        at: PointMm::new(1100.0, 1060.0),
    });
    let (a, b) = (a.unwrap().points[0], b.unwrap().points[0]);
    app.editor.selection.clear();
    app.editor.selection.points = vec![a, b];
    let before = serde_json::to_string(app.editor.project()).expect("json");
    let mut h = harness_of(app);
    h.state_mut().fit_pending = false;
    h.state_mut().zoom_to_selection();
    h.run_steps(3);
    let app = h.state();
    near(app.view.center.x, 1050.0, 1e-9);
    near(app.view.center.y, 1030.0, 1e-9);
    let rect = app.canvas_rect;
    // 100 x 60 mm tient dans 80 % de la zone.
    assert!(app.view.zoom * 100.0 <= f64::from(rect.width()) * 0.8 + 1e-6);
    assert!(app.view.zoom * 60.0 <= f64::from(rect.height()) * 0.8 + 1e-6);
    assert!(app.zoom_target.is_none());
    assert_eq!(serde_json::to_string(app.editor.project()).unwrap(), before);
}

#[test]
fn le_zoom_sur_une_selection_vide_ne_change_rien_et_le_dit() {
    let mut app = app();
    app.editor.selection.clear();
    let view = app.view;
    app.zoom_to_selection();
    assert!(app.zoom_target.is_none());
    assert_eq!(app.view, view);
    assert_eq!(
        app.editor.status.text,
        pf_i18n::tr("views.zoom_selection.empty")
    );
}

#[test]
fn le_raccourci_fin_declenche_le_zoom_sur_la_selection() {
    let cmd = crate::shortcuts::Cmd::from_id("view.zoom_selection").expect("commande");
    assert_eq!(cmd.id(), "view.zoom_selection");
    let mut app = app();
    let sheet = app.editor.sheet().id;
    let p = app
        .editor
        .exec(Command::AddPoint {
            sheet,
            at: PointMm::new(500.0, 500.0),
        })
        .unwrap()
        .points[0];
    app.editor.selection.clear();
    app.editor.selection.points = vec![p];
    let mut h = harness_of(app);
    h.state_mut().fit_pending = false;
    h.key_press(Key::End);
    h.run_steps(4);
    near(h.state().view.center.x, 500.0, 1e-9);
    near(h.state().view.center.y, 500.0, 1e-9);
}

// ===== (d) vues nommées ===========================================================================

fn view_named(name: &str) -> NamedView {
    NamedView {
        name: name.into(),
        sheet_id: 7,
        sheet_name: "Feuille".into(),
        center: PointMm::new(12.5, -3.25),
        zoom: 2.0,
        layers: vec![("Haut".into(), true), ("Bas".into(), false)],
    }
}

#[test]
fn enregistrer_renommer_supprimer_une_vue() {
    let mut vs = ViewSettings::default();
    vs.save_named(view_named("  Ensemble ")).unwrap();
    assert_eq!(vs.named[0].name, "Ensemble", "espaces de bord retirés");
    assert_eq!(
        vs.save_named(view_named("ensemble")).unwrap_err().kind,
        "view_duplicate"
    );
    assert_eq!(
        vs.save_named(view_named("   ")).unwrap_err().kind,
        "name_empty"
    );
    assert_eq!(
        vs.save_named(view_named(&"x".repeat(41))).unwrap_err().kind,
        "name_long"
    );
    assert_eq!(
        vs.save_named(view_named("a\u{7}b")).unwrap_err().kind,
        "name_control"
    );
    vs.save_named(view_named("Détail")).unwrap();
    assert_eq!(
        vs.rename_named(1, "ENSEMBLE").unwrap_err().kind,
        "view_duplicate"
    );
    vs.rename_named(1, "Poche").unwrap();
    vs.rename_named(0, "ensemble").unwrap(); // même vue, autre casse : permis
    let mut moved = view_named("ignoré");
    moved.zoom = 4.0;
    vs.update_named(1, moved).unwrap();
    assert_eq!(
        (vs.named[1].name.as_str(), vs.named[1].zoom),
        ("Poche", 4.0)
    );
    assert_eq!(vs.delete_named(0).unwrap().name, "ensemble");
    assert_eq!(vs.named.len(), 1);
    assert_eq!(vs.delete_named(5).unwrap_err().kind, "view_missing");
    assert_eq!(vs.rename_named(5, "x").unwrap_err().kind, "view_missing");
}

#[test]
fn au_plus_cinquante_vues() {
    let mut vs = ViewSettings::default();
    for i in 0..views::MAX_NAMED_VIEWS {
        vs.save_named(view_named(&format!("v{i}"))).unwrap();
    }
    assert_eq!(
        vs.save_named(view_named("de trop")).unwrap_err().kind,
        "too_many"
    );
    assert_eq!(vs.named.len(), 50);
}

fn two_layer_app() -> (App, [LayerId; 2]) {
    let mut app = app();
    let sheet = app.editor.sheet().id;
    let a = app
        .editor
        .exec(Command::AddLayer {
            sheet,
            name: "Haut".into(),
        })
        .unwrap()
        .layers[0];
    let b = app
        .editor
        .exec(Command::AddLayer {
            sheet,
            name: "Bas".into(),
        })
        .unwrap()
        .layers[0];
    app.editor.doc.end_group();
    (app, [a, b])
}

fn visible(app: &App, id: LayerId) -> bool {
    app.editor
        .sheet()
        .layers
        .iter()
        .find(|l| l.id == id)
        .unwrap()
        .visible
}

#[test]
fn restaurer_une_vue_rend_cadrage_zoom_feuille_et_calques_en_une_etape() {
    let (mut app, [haut, bas]) = two_layer_app();
    app.view.center = PointMm::new(250.0, 125.0);
    app.view.zoom = 3.0;
    let named = app.capture_view("Travail");
    app.view_settings.save_named(named).unwrap();
    // On change tout.
    let sheet = app.editor.sheet().id;
    for layer in [haut, bas] {
        app.editor
            .exec(Command::SetLayerVisible {
                sheet,
                layer,
                visible: false,
            })
            .unwrap();
    }
    app.view.center = PointMm::new(-5.0, 9.0);
    app.view.zoom = 0.5;
    app.editor.doc.end_group();
    assert!(!visible(&app, bas) && !visible(&app, haut));
    app.restore_view(0).unwrap();
    assert!(
        visible(&app, bas) && visible(&app, haut),
        "calques rétablis"
    );
    assert_eq!(
        (app.view.center, app.view.zoom),
        (PointMm::new(250.0, 125.0), 3.0)
    );
    // Une seule étape pour tous les calques.
    assert!(app.editor.doc.undo());
    assert!(
        !visible(&app, bas) && !visible(&app, haut),
        "les deux calques reviennent en une seule étape"
    );
}

#[test]
fn restaurer_signale_les_calques_disparus_et_refuse_une_feuille_absente() {
    let (mut app, [_, bas]) = two_layer_app();
    let mut named = app.capture_view("A");
    named.layers.push(("Fantôme".into(), true));
    app.view_settings.save_named(named).unwrap();
    app.restore_view(0).unwrap();
    assert_eq!(
        app.editor.status.text,
        pf_i18n::tr_args("views.restored_missing", &[("name", "A"), ("count", "1")])
    );
    // Feuille absente : refus sans rien changer (ni vue, ni calques).
    let mut gone = app.capture_view("B");
    gone.sheet_id = 9999;
    gone.sheet_name = "Disparue".into();
    app.view_settings.save_named(gone).unwrap();
    let (before, project) = (
        app.view,
        serde_json::to_string(app.editor.project()).unwrap(),
    );
    let sheet = app.editor.sheet().id;
    let _ = (bas, sheet);
    assert_eq!(app.restore_view(1).unwrap_err().kind, "sheet_missing");
    assert_eq!(app.view, before);
    assert_eq!(
        serde_json::to_string(app.editor.project()).unwrap(),
        project
    );
    assert_eq!(app.restore_view(9).unwrap_err().kind, "view_missing");
}

#[test]
fn appariement_des_calques_et_des_feuilles() {
    let layers = vec![
        (LayerId(1), "Haut".to_string(), true),
        (LayerId(2), "Bas".to_string(), true),
    ];
    let (changes, missing) = views::layer_changes(
        &[
            ("Bas".into(), false),
            ("Haut".into(), true),
            ("Nul".into(), false),
        ],
        &layers,
    );
    assert_eq!((changes, missing), (vec![(LayerId(2), false)], 1));
    let sheets = vec![
        (pf_core::SheetId(1), "A".to_string()),
        (pf_core::SheetId(2), "B".to_string()),
        (pf_core::SheetId(3), "B".to_string()),
    ];
    let mut v = view_named("v");
    v.sheet_name = "B".into();
    v.sheet_id = 3;
    assert_eq!(views::find_sheet(&v, &sheets), Some(pf_core::SheetId(3)));
    v.sheet_id = 99;
    assert_eq!(
        views::find_sheet(&v, &sheets),
        Some(pf_core::SheetId(2)),
        "repli sur le nom"
    );
    v.sheet_name = "Z".into();
    assert_eq!(views::find_sheet(&v, &sheets), None);
}

// ===== réglages : fichier hors projet =============================================================

fn sample_settings() -> ViewSettings {
    let mut vs = ViewSettings {
        units: inches(32),
        grid: GridSettings {
            visible: false,
            step_mm: 12.5,
            subdivisions: 3,
        },
        rulers: true,
        named: Vec::new(),
    };
    vs.save_named(view_named("Un")).unwrap();
    vs.save_named(view_named("Deux")).unwrap();
    vs
}

#[test]
fn les_reglages_font_l_aller_retour_sans_perte_et_gardent_les_autres_cles() {
    let dir = temp_dir("aller-retour");
    let path = dir.join("settings.json");
    pf_i18n::settings::save_language(&path, pf_i18n::Lang::En).unwrap();
    let vs = sample_settings();
    pf_i18n::settings::save_value(&path, views::SETTINGS_KEY, vs.to_json()).unwrap();
    let (back, issues) = views::load(&path);
    assert_eq!(back, vs);
    assert!(issues.is_empty(), "{issues:?}");
    assert_eq!(pf_i18n::settings::load_language(&path), pf_i18n::Lang::En);
    assert!(
        !dir.join("settings.json.tmp").exists(),
        "écriture atomique : pas de reste"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn fichier_absent_sans_cle_ou_corrompu_donne_les_defauts_sans_panique_ni_ecriture() {
    let dir = temp_dir("corrompu");
    let path = dir.join("settings.json");
    let (vs, issues) = views::load(&path);
    assert_eq!(
        (vs, issues.len()),
        (ViewSettings::default(), 0),
        "absent : silencieux"
    );
    for (content, expect_issue) in [
        ("", true),
        ("pas du json {", true),
        ("[1,2]", true),
        ("\u{0}\u{1}", true),
        ("{}", false),
        (r#"{"language":"fr"}"#, false),
        (r#"{"views":42}"#, true),
        (r#"{"views":null}"#, true),
        (r#"{"views":{"unit":"furlong"}}"#, true),
        (r#"{"views":{"version":99}}"#, true),
    ] {
        std::fs::write(&path, content).unwrap();
        let (vs, issues) = views::load(&path);
        assert_eq!(vs, ViewSettings::default(), "{content:?}");
        assert_eq!(!issues.is_empty(), expect_issue, "{content:?}");
        for i in &issues {
            assert!(!i.text().is_empty());
        }
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            content,
            "jamais réécrit à la lecture"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn une_valeur_invalide_garde_le_reste_et_est_signalee() {
    let value = json!({
        "version": 1,
        "unit": "in",
        "inch_denominator": 7,
        "grid": {"visible": true, "step_mm": -3.0, "subdivisions": 0},
        "rulers": "oui",
        "named": [
            {"name": "ok", "sheet_id": 1, "sheet_name": "F", "center_x": 0.0, "center_y": 0.0, "zoom": 2.0, "layers": []},
            {"name": "", "sheet_id": 1, "sheet_name": "F", "center_x": 0.0, "center_y": 0.0, "zoom": 2.0, "layers": []},
            {"name": "zoom fou", "sheet_id": 1, "sheet_name": "F", "center_x": 0.0, "center_y": 0.0, "zoom": 1e9, "layers": []},
            {"name": "OK", "sheet_id": 1, "sheet_name": "F", "center_x": 0.0, "center_y": 0.0, "zoom": 2.0, "layers": []},
            "pas un objet"
        ]
    });
    let (vs, issues) = ViewSettings::from_json(&value);
    assert_eq!(vs.units, inches(16), "unité gardée, précision par défaut");
    assert_eq!(vs.grid, GridSettings::default_for(LengthUnit::In));
    assert!(!vs.rulers);
    assert_eq!(vs.named.len(), 1);
    let kinds: Vec<&str> = issues.iter().map(|i| i.kind).collect();
    for k in [
        "inch_denominator",
        "grid",
        "rulers",
        "view_invalid",
        "view_duplicate",
    ] {
        assert!(kinds.contains(&k), "{k} absent de {kinds:?}");
    }
}

// ===== campagne adverse (graine fixe) =============================================================

/// Générateur xorshift64* : reproductible, sans dépendance.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
    fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (self.next() >> 11) as f64 / (1u64 << 53) as f64 * (hi - lo)
    }
}

/// Oracle indépendant : numérateur entier le plus proche de `inches * den`, cherché au voisinage
/// plutôt que calculé par `round`.
fn nearest_numerator(inches: f64, den: u32) -> i64 {
    let guess = (inches * f64::from(den)).floor() as i64;
    (guess - 2..=guess + 2)
        .min_by(|a, b| {
            let da = (inches - *a as f64 / f64::from(den)).abs();
            let db = (inches - *b as f64 / f64::from(den)).abs();
            da.partial_cmp(&db).unwrap()
        })
        .unwrap()
}

#[derive(Default, Debug)]
struct Coverage {
    exact_fraction: u32,
    decimal: u32,
    negative: u32,
    tiny: u32,
    large: u32,
    per_unit: [u32; 3],
    per_den: [u32; 6],
    invalid_refused: u32,
    garbage: u32,
    approx_flagged: u32,
}

#[test]
fn campagne_aller_retour_mm_unite_mm_et_saisies_invalides() {
    let mut rng = Rng(0x165_0001);
    let mut cov = Coverage::default();
    for _ in 0..240 {
        let den = INCH_DENOMINATORS[rng.below(6)];
        let unit = LengthUnit::ALL[rng.below(3)];
        let units = DisplayUnits {
            unit,
            inch_denominator: den,
        };
        cov.per_unit[LengthUnit::ALL.iter().position(|u| *u == unit).unwrap()] += 1;
        cov.per_den[INCH_DENOMINATORS.iter().position(|d| *d == den).unwrap()] += 1;
        // Valeur du modèle : fraction exacte de pouce, décimal quelconque, négative, minuscule,
        // énorme.
        let (mm, kind) = match rng.below(5) {
            0 => {
                let n = rng.below(2000) as i64 - 500;
                (n as f64 * 254.0 / (10.0 * f64::from(den)), 0)
            }
            1 => (rng.range(0.0, 3000.0), 1),
            2 => (-rng.range(0.001, 800.0), 2),
            3 => (rng.range(0.0, 0.002), 3),
            _ => (rng.range(1.0e5, 1.0e7), 4),
        };
        match kind {
            0 => cov.exact_fraction += 1,
            1 => cov.decimal += 1,
            2 => cov.negative += 1,
            3 => cov.tiny += 1,
            _ => cov.large += 1,
        }
        // 1. Champ exact : relu, il redonne le modèle (tolérance de l'unité), jamais plus loin.
        let field = format_field(mm, units);
        let back = parse_length_with(&field, unit).expect("le champ exact se relit");
        let tol = match unit {
            LengthUnit::Mm | LengthUnit::Cm => 6e-4,
            LengthUnit::In => 3e-5 * mm.abs().max(1.0),
        };
        assert!(
            (back - mm).abs() <= tol,
            "{unit:?} 1/{den}: {mm} -> {field:?} -> {back}"
        );
        // Une fraction exacte revient exactement (à l'epsilon flottant près).
        if kind == 0 && unit == LengthUnit::In {
            assert!((back - mm).abs() < 1e-9, "fraction exacte {mm} -> {back}");
        }
        // 2. Texte d'affichage : en pouces, c'est la fraction la plus proche (oracle).
        if unit == LengthUnit::In {
            let shown = format_length(mm, units);
            let inches = mm / MM_PER_INCH;
            let n = nearest_numerator(inches.abs(), den);
            let text = shown.trim_start_matches("≈ ").trim_end_matches('"');
            let reparsed = parse_length_with(text, LengthUnit::In).expect("fraction relue");
            let expected = n as f64 * MM_PER_INCH / f64::from(den);
            let expected = if inches < 0.0 && n > 0 {
                -expected
            } else {
                expected
            };
            assert!(
                (reparsed - expected).abs() < 1e-7 * expected.abs().max(1.0),
                "{shown} vs {expected}"
            );
            if shown.starts_with('≈') {
                cov.approx_flagged += 1;
                assert!(
                    (expected - mm).abs() > 1e-7 * 0.5,
                    "≈ sur une valeur exacte : {shown}"
                );
            }
        }
        // 3. Saisies invalides dérivées d'une saisie valide : toujours refusées, jamais de panique.
        let valid = format_field(mm.abs() + 1.0, units);
        let bad = match rng.below(6) {
            0 => format!("{valid}/0"),
            1 => format!("{valid}//2"),
            2 => format!("{valid} mm mm"),
            3 => format!("{valid}e3x"),
            4 => format!("x{valid}"),
            _ => format!("{valid},,5"),
        };
        if parse_length_with(&bad, unit).is_err() {
            cov.invalid_refused += 1;
        } else {
            panic!("{bad:?} aurait dû être refusé ({unit:?})");
        }
        // 4. Bruit : jamais de panique, résultat fini s'il est accepté.
        let alphabet: Vec<char> = "0123456789/ -+.,\"'mcinpoe\u{a0}x²".chars().collect();
        let len = rng.below(12);
        let noise: String = (0..len)
            .map(|_| alphabet[rng.below(alphabet.len())])
            .collect();
        if let Ok(v) = parse_length_with(&noise, unit) {
            assert!(v.is_finite(), "{noise:?} -> {v}");
        }
        cov.garbage += 1;
    }
    // Compteurs de couverture : chaque famille a réellement été exercée.
    assert!(
        cov.exact_fraction >= 30 && cov.decimal >= 30 && cov.negative >= 30,
        "{cov:?}"
    );
    assert!(cov.tiny >= 30 && cov.large >= 30, "{cov:?}");
    assert!(cov.per_unit.iter().all(|c| *c >= 40), "{cov:?}");
    assert!(cov.per_den.iter().all(|c| *c >= 15), "{cov:?}");
    assert!(cov.invalid_refused == 240 && cov.garbage == 240, "{cov:?}");
    assert!(cov.approx_flagged >= 20, "{cov:?}");
}

#[test]
fn campagne_fichier_de_reglages_corrompu() {
    let dir = temp_dir("campagne");
    let path = dir.join("settings.json");
    let mut rng = Rng(0x165_0002);
    let base =
        serde_json::to_string(&json!({"language": "en", "views": sample_settings().to_json()}))
            .unwrap();
    let (mut truncated, mut retyped, mut out_of_range, mut valid, mut flagged) =
        (0u32, 0u32, 0u32, 0u32, 0u32);
    for i in 0..180 {
        let mut text = base.clone();
        let mut mutated = true;
        match rng.below(4) {
            0 => {
                // Fichier tronqué à un octet quelconque (frontière de caractère).
                let mut cut = rng.below(text.len());
                while !text.is_char_boundary(cut) {
                    cut -= 1;
                }
                text.truncate(cut);
                truncated += 1;
            }
            1 => {
                // Un champ remplacé par un autre type.
                let mut v: Value = serde_json::from_str(&base).unwrap();
                let keys = ["unit", "inch_denominator", "grid", "rulers", "named"];
                let key = keys[rng.below(keys.len())];
                let wrong = [
                    json!(null),
                    json!("x"),
                    json!([1]),
                    json!(-1),
                    json!(true),
                    json!({"a": 1}),
                ];
                v["views"][key] = wrong[rng.below(wrong.len())].clone();
                text = v.to_string();
                retyped += 1;
            }
            2 => {
                // Nombres hors bornes dans la grille et le zoom d'une vue.
                let mut v: Value = serde_json::from_str(&base).unwrap();
                v["views"]["grid"]["step_mm"] = json!([-1.0, 0.0, 1e12, 1e-9][rng.below(4)]);
                v["views"]["grid"]["subdivisions"] =
                    json!([0, 21, 4_000_000_000u64, 1][rng.below(4)]);
                v["views"]["named"][0]["zoom"] = json!([0.0, -1.0, 1e300, 5.0][rng.below(4)]);
                text = v.to_string();
                out_of_range += 1;
            }
            _ => {
                mutated = false;
                valid += 1;
            }
        }
        std::fs::write(&path, &text).unwrap();
        let (vs, issues) = views::load(&path);
        // Invariants, quoi qu'on ait lu.
        assert!(
            (views::GRID_STEP_MIN_MM..=views::GRID_STEP_MAX_MM).contains(&vs.grid.step_mm),
            "{i}: {text}"
        );
        assert!((views::SUBDIVISIONS_MIN..=views::SUBDIVISIONS_MAX).contains(&vs.grid.subdivisions));
        assert!(vs.named.len() <= views::MAX_NAMED_VIEWS);
        assert!(INCH_DENOMINATORS.contains(&vs.units.inch_denominator));
        let mut seen = std::collections::HashSet::new();
        for v in &vs.named {
            assert!((crate::view::MIN_ZOOM..=crate::view::MAX_ZOOM).contains(&v.zoom));
            assert!(seen.insert(v.name.to_lowercase()), "doublon {}", v.name);
        }
        if !mutated {
            assert_eq!(vs, sample_settings());
            assert!(issues.is_empty());
        } else if !issues.is_empty() {
            flagged += 1;
        }
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            text,
            "lecture seule"
        );
    }
    assert!(
        truncated >= 25 && retyped >= 25 && out_of_range >= 25 && valid >= 25,
        "{truncated} {retyped} {out_of_range} {valid}"
    );
    assert!(
        flagged >= 60,
        "les fichiers abîmés sont signalés : {flagged}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn campagne_operations_sur_les_vues_nommees() {
    let mut rng = Rng(0x165_0003);
    let mut vs = ViewSettings::default();
    let mut oracle: Vec<String> = Vec::new();
    let (mut saved, mut refused, mut renamed, mut deleted, mut updated) =
        (0u32, 0u32, 0u32, 0u32, 0u32);
    let names = [
        "Ensemble", "ensemble", "Détail", "  Poche ", "poche", "", "x", "Tête",
    ];
    for _ in 0..400 {
        let name = names[rng.below(names.len())];
        let idx = rng.below(6);
        match rng.below(4) {
            0 => {
                let ok = {
                    let n = name.trim();
                    !n.is_empty()
                        && !oracle.iter().any(|o| o.to_lowercase() == n.to_lowercase())
                        && oracle.len() < views::MAX_NAMED_VIEWS
                };
                let r = vs.save_named(view_named(name));
                assert_eq!(r.is_ok(), ok, "save {name:?} sur {oracle:?}");
                if ok {
                    oracle.push(name.trim().to_string());
                    saved += 1;
                } else {
                    refused += 1;
                }
            }
            1 => {
                let n = name.trim();
                let ok = idx < oracle.len()
                    && !n.is_empty()
                    && !oracle
                        .iter()
                        .enumerate()
                        .any(|(k, o)| k != idx && o.to_lowercase() == n.to_lowercase());
                let r = vs.rename_named(idx, name);
                assert_eq!(r.is_ok(), ok, "rename {idx} {name:?} sur {oracle:?}");
                if ok {
                    oracle[idx] = n.to_string();
                    renamed += 1;
                } else {
                    refused += 1;
                }
            }
            2 => {
                let r = vs.delete_named(idx);
                assert_eq!(r.is_ok(), idx < oracle.len());
                if r.is_ok() {
                    oracle.remove(idx);
                    deleted += 1;
                } else {
                    refused += 1;
                }
            }
            _ => {
                let mut v = view_named("autre");
                v.zoom = 1.0 + rng.range(0.0, 3.0);
                let z = v.zoom;
                let r = vs.update_named(idx, v);
                assert_eq!(r.is_ok(), idx < oracle.len());
                if r.is_ok() {
                    assert_eq!(vs.named[idx].zoom, z);
                    updated += 1;
                }
            }
        }
        let got: Vec<&str> = vs.named.iter().map(|v| v.name.as_str()).collect();
        assert_eq!(got, oracle.iter().map(String::as_str).collect::<Vec<_>>());
        // Le fichier relu est identique : aucune perte silencieuse.
        let (back, issues) = ViewSettings::from_json(&vs.to_json());
        assert!(issues.is_empty(), "{issues:?}");
        assert_eq!(back, vs);
    }
    assert!(
        saved >= 20 && refused >= 40 && renamed >= 10 && deleted >= 10 && updated >= 10,
        "{saved} {refused} {renamed} {deleted} {updated}"
    );
}

// ===== écran : FR + EN ============================================================================

fn stage_harness(lang: pf_i18n::Lang) -> Harness<'static, App> {
    let launch = Launch {
        demo: true,
        stage: Some("vues".into()),
        lang: Some(lang),
        ..Launch::default()
    };
    let app = App::with_context(&egui::Context::default(), launch);
    let mut h = Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .build_state(|ctx, app: &mut App| app.ui(ctx), app);
    h.run_steps(6);
    h
}

#[test]
fn la_fenetre_des_vues_s_affiche_en_francais() {
    fenetre_des_vues(pf_i18n::Lang::Fr);
}

#[test]
fn la_fenetre_des_vues_s_affiche_en_anglais() {
    fenetre_des_vues(pf_i18n::Lang::En);
}

fn fenetre_des_vues(lang: pf_i18n::Lang) {
    let _guard = UnitGuard;
    {
        let mut h = stage_harness(lang);
        for key in [
            "views.title",
            "views.unit",
            "views.precision",
            "views.grid.visible",
            "views.grid.step",
            "views.grid.subdivisions",
            "views.named",
            "views.save",
        ] {
            let label = pf_i18n::lookup(lang, key);
            assert!(
                h.query_all_by_label_contains(&label).count() > 0,
                "{lang:?} : « {label} » absent de la fenêtre"
            );
        }
        // Deux vues enregistrées, chacune avec ses boutons.
        assert_eq!(
            h.get_all_by_label(&pf_i18n::lookup(lang, "views.restore"))
                .count(),
            2
        );
        assert!(
            h.get_all_by_label(&pf_i18n::lookup(lang, "views.delete"))
                .count()
                >= 2
        );
        // Les unités sont celles de l'étape (pouces) : la grille affichée l'est aussi.
        assert_eq!(display_units(), inches(16));
        // Supprimer une vue depuis l'écran la retire des réglages.
        h.state_mut().run_view_action(views::ViewAction::Delete(0));
        h.run_steps(4);
        assert_eq!(h.state().view_settings.named.len(), 1, "{lang:?}");
        // Le français n'est pas figé dans le fichier d'une autre langue.
        let title_other = pf_i18n::lookup(
            if lang == pf_i18n::Lang::Fr {
                pf_i18n::Lang::En
            } else {
                pf_i18n::Lang::Fr
            },
            "views.title",
        );
        assert_ne!(title_other, pf_i18n::lookup(lang, "views.title"));
    }
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
}

#[test]
fn chaque_cle_views_existe_dans_les_deux_langues() {
    let source = include_str!("views.rs");
    let mut keys = std::collections::BTreeSet::new();
    for part in source.split('"') {
        if part.starts_with("views.") && !part.contains(' ') && !part.contains('{') {
            keys.insert(part.to_string());
        }
    }
    for kind in [
        "not_object",
        "newer",
        "unit",
        "inch_denominator",
        "grid",
        "rulers",
        "named",
        "view_invalid",
        "view_duplicate",
        "too_many",
        "name_empty",
        "name_long",
        "name_control",
        "view_missing",
        "sheet_missing",
        "unreadable",
        "file_corrupt",
    ] {
        keys.insert(format!("views.issue.{kind}"));
    }
    assert!(keys.len() >= 40, "{}", keys.len());
    for key in keys {
        for lang in [pf_i18n::Lang::Fr, pf_i18n::Lang::En] {
            let text = pf_i18n::lookup(lang, &key);
            assert!(
                !text.is_empty() && text != key,
                "{lang:?} : clé {key} manquante"
            );
        }
    }
}
