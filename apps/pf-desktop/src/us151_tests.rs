//! US-15.1 / US-15.2 : fenêtre « Plan de coupe » (egui, français et anglais). Chaque geste est une
//! commande = un pas d'annulation ; un glisser continu n'en produit qu'un. Les valeurs sont celles
//! de l'étape `plan-coupe` (écrites à la main dans `cutting_stage_project`).

use crate::app::App;
use crate::dock::ToolWin;
use crate::Launch;
use eframe::egui::{self, Event, Key, Modifiers, PointerButton, Pos2};
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use pf_core::{CuttingPlan, Project};
use pf_i18n::{tr, tr_args, Lang};

fn harness() -> Harness<'static, App> {
    let launch = Launch {
        demo: true,
        stage: Some("plan-coupe".into()),
        ..Launch::default()
    };
    let app = App::with_context(&egui::Context::default(), launch);
    let mut h = Harness::builder()
        .with_size(egui::vec2(1600.0, 1000.0))
        .build_state(|ctx, app: &mut App| app.ui(ctx), app);
    h.run_steps(6);
    h
}

fn project(h: &Harness<'_, App>) -> Project {
    h.state().editor.project().clone()
}

fn plan(h: &Harness<'_, App>) -> CuttingPlan {
    h.state().editor.project().cutting_plans[0].clone()
}

fn undo(h: &mut Harness<'_, App>) {
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run_steps(3);
}

fn pointer(h: &mut Harness<'_, App>, pos: Pos2, pressed: Option<bool>) {
    h.input_mut().events.push(Event::PointerMoved(pos));
    if let Some(pressed) = pressed {
        let modifiers = h.input().modifiers;
        h.input_mut().events.push(Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers,
        });
    }
    h.step();
}

/// Glisser-déposer continu : appui en `from`, déplacement en six pas, relâchement en `to`.
fn drag(h: &mut Harness<'_, App>, from: Pos2, to: Pos2) {
    pointer(h, from, None);
    pointer(h, from, Some(true));
    for i in 1..=6 {
        let t = i as f32 / 6.0;
        pointer(h, from + (to - from) * t, None);
    }
    pointer(h, to, Some(false));
    h.run_steps(3);
}

fn screen(h: &Harness<'_, App>, x: f64, y: f64) -> Pos2 {
    h.state()
        .cutting
        .as_ref()
        .and_then(|d| d.screen_pos(x, y))
        .expect("canevas du plan de coupe")
}

fn type_in(h: &mut Harness<'_, App>, label: &str, text: &str) {
    h.get_by_label(label).focus();
    h.run_steps(2);
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.run_steps(1);
    h.input_mut().events.push(Event::Text(text.to_string()));
    h.step();
    h.key_press(Key::Enter);
    h.run_steps(3);
}

fn field(key: &str) -> String {
    format!("{} ({})", tr(key), tr("a11y.field_suffix"))
}

fn both_languages(f: impl Fn()) {
    for lang in [Lang::Fr, Lang::En] {
        pf_i18n::set_lang(lang);
        f();
    }
    pf_i18n::set_lang(Lang::Fr);
}

#[test]
fn la_fenetre_est_dans_le_dock_et_dit_les_defauts_en_toutes_lettres() {
    both_languages(|| {
        let h = harness();
        h.get_by_label(&tr("cutting.title"));
        let win = h
            .ctx
            .memory(|m| m.area_rect(ToolWin::Cutting.id()))
            .expect("fenêtre Plan de coupe");
        assert!(
            h.state().dock.rect.contains_rect(win),
            "{win:?} hors du dock"
        );
        assert!(!win.intersects(h.state().canvas_rect));
        // Le plan est invalide : la phrase le dit, les défauts sont listés, sans couleur seule.
        h.get_by_label(&tr("cutting.diag.invalid"));
        let p = project(&h);
        let pl = &p.cutting_plans[0];
        let a = p.analyze_cutting_plan(pl.id).unwrap();
        assert_eq!(a.overlaps.len(), 1);
        assert_eq!(a.overflows.len(), 1);
        h.get_all_by_label_contains("✗")
            .next()
            .expect("défaut listé avec une marque");
        // La pose qui chevauche porte le mot du défaut dans la liste des poses.
        assert!(
            h.get_all_by_label_contains(&tr("cutting.pattern.overlap"))
                .count()
                >= 2
        );
        assert!(
            h.get_all_by_label_contains(&tr("cutting.pattern.overflow"))
                .count()
                >= 1
        );
    });
}

#[test]
fn glisser_une_piece_de_la_liste_pose_une_instance_en_un_pas() {
    both_languages(|| {
        let mut h = harness();
        let before = project(&h);
        let item = tr_args(
            "cutting.todo.row",
            &[("name", "Manche"), ("placed", "1"), ("expected", "2")],
        );
        let from = h.get_by_label(&item).rect().center();
        // Dépôt centré en (300 ; 800) mm : au-dessus de tout, dans la zone utile.
        let to = screen(&h, 300.0, 800.0);
        drag(&mut h, from, to);
        let after = plan(&h);
        assert_eq!(after.placements.len(), 6, "une pose de plus");
        let new = after.placements.last().unwrap();
        // Manche (300 de large, 420 de haut) centrée en (300 ; 800) : coin bas-gauche (150 ; 590).
        assert!((new.x_mm - 150.0).abs() <= 2.0, "x = {}", new.x_mm);
        assert!((new.y_mm - 590.0).abs() <= 2.0, "y = {}", new.y_mm);
        assert_eq!(new.rotation_deg, 0.0);
        undo(&mut h);
        assert_eq!(project(&h), before, "une seule annulation");
    });
}

#[test]
fn deplacer_une_piece_posee_est_un_seul_pas_a_la_fin_du_geste() {
    both_languages(|| {
        let mut h = harness();
        let before = project(&h);
        // Seconde Poche (débordante) : coin (1300 ; 100), 120 × 150 ; son centre (1360 ; 175).
        let from = screen(&h, 1360.0, 175.0);
        let to = screen(&h, 1060.0, 175.0);
        drag(&mut h, from, to);
        let moved = plan(&h).placements[4];
        assert!(
            (moved.x_mm - 1000.0).abs() <= 3.0,
            "1300 - 300 = 1000, obtenu {}",
            moved.x_mm
        );
        assert!((moved.y_mm - 100.0).abs() <= 3.0);
        // Un seul pas : une annulation rend le projet d'origine.
        undo(&mut h);
        assert_eq!(project(&h), before);
        // Rétablir ramène la position.
        h.key_press_modifiers(Modifiers::COMMAND, Key::Y);
        h.run_steps(3);
        assert!((plan(&h).placements[4].x_mm - moved.x_mm).abs() < 1e-9);
    });
}

#[test]
fn rotation_miroir_retrait_sont_des_commandes_annulables() {
    both_languages(|| {
        let mut h = harness();
        let before = project(&h);
        h.get_all_by_label_contains("Devant #1")
            .next()
            .expect("Devant #1")
            .click();
        h.run_steps(3);
        // Miroir : une commande.
        h.get_by_label(&tr("cutting.mirror")).click_accesskit();
        h.run_steps(3);
        assert!(plan(&h).placements[0].mirrored);
        undo(&mut h);
        assert_eq!(project(&h), before);
        h.run_steps(3);
        // Le Devant a un droit-fil et le plan un sens : le pas est de 180°, pas de 90°.
        h.get_by_label(&tr_args("cutting.rot_step", &[("deg", "180")]))
            .click_accesskit();
        h.run_steps(3);
        assert_eq!(plan(&h).placements[0].rotation_deg, 180.0);
        undo(&mut h);
        assert_eq!(project(&h), before);
        h.run_steps(3);
        // Retrait.
        h.get_by_label(&tr("cutting.delete")).click_accesskit();
        h.run_steps(3);
        assert_eq!(plan(&h).placements.len(), 4);
        undo(&mut h);
        assert_eq!(project(&h), before);
    });
}

#[test]
fn une_rotation_que_le_droit_fil_interdit_est_refusee_avec_son_message() {
    both_languages(|| {
        let mut h = harness();
        let before = project(&h);
        h.get_all_by_label_contains("Devant #1")
            .next()
            .expect("Devant #1")
            .click();
        h.run_steps(3);
        type_in(&mut h, &field("cutting.rot"), "90");
        assert_eq!(project(&h), before, "refus sans trace");
        h.get_by_label(&tr_args("cutting.err.rotation_grain", &[("dev", "90")]));
        // 180° est admis : une commande.
        type_in(&mut h, &field("cutting.rot"), "180");
        assert_eq!(plan(&h).placements[0].rotation_deg, 180.0);
        undo(&mut h);
        assert_eq!(project(&h), before);
    });
}

#[test]
fn poser_la_selection_est_une_seule_etape_en_rangees() {
    both_languages(|| {
        let mut h = harness();
        let before = project(&h);
        let item = tr_args(
            "cutting.todo.row",
            &[("name", "Manche"), ("placed", "1"), ("expected", "2")],
        );
        h.get_by_label(&item).click();
        h.run_steps(3);
        h.get_by_label(&tr("cutting.pose_sel")).click();
        h.run_steps(3);
        let after = plan(&h);
        assert_eq!(after.placements.len(), 6);
        let new = after.placements.last().unwrap();
        // Au-dessus de tout (la Poche posée en (150 ; 400) monte à 400 + 150 = 550, écart 10) :
        // y = 560, à la lisière (x = 20).
        assert!((new.y_mm - 560.0).abs() < 1e-6, "y = {}", new.y_mm);
        assert!((new.x_mm - 20.0).abs() < 1e-6, "x = {}", new.x_mm);
        // La Manche est complète : elle quitte la liste des pièces à poser.
        assert!(h.query_by_label(&item).is_none());
        undo(&mut h);
        assert_eq!(project(&h), before);
    });
}

#[test]
fn rien_a_poser_est_dit_sans_modifier_le_projet() {
    both_languages(|| {
        let mut h = harness();
        let before = project(&h);
        h.get_by_label(&tr("cutting.pose_all")).click();
        h.run_steps(3);
        assert_eq!(project(&h), before);
        h.get_by_label(&tr("cutting.pose_nothing"));
    });
}

#[test]
fn creer_un_plan_modifier_ses_parametres_et_le_supprimer() {
    both_languages(|| {
        let mut h = harness();
        let before = project(&h);
        h.state_mut().cutting.as_mut().unwrap().params_open = true;
        h.get_by_label(&field("cutting.new")).focus();
        h.run_steps(2);
        h.input_mut().events.push(Event::Text("Lin".into()));
        h.step();
        h.get_by_label(&tr("cutting.add")).click();
        h.run_steps(3);
        assert_eq!(project(&h).cutting_plans.len(), 2);
        assert_eq!(project(&h).cutting_plans[1].name, "Lin");
        // Laize 1000 : une commande ; « abc » : refus du texte, projet inchangé.
        type_in(&mut h, &field("cutting.width"), "1000");
        assert_eq!(project(&h).cutting_plans[1].fabric_width_mm, 1000.0);
        let mid = project(&h);
        type_in(&mut h, &field("cutting.width"), "abc");
        assert_eq!(project(&h), mid);
        h.get_by_label(&tr_args("cutting.err.number", &[("text", "abc")]));
        // 2 lisières >= laize : refus du cœur, rien n'est modifié.
        type_in(&mut h, &field("cutting.selvedge"), "500");
        assert_eq!(project(&h), mid);
        // Supprimer le plan, puis tout annuler.
        h.get_by_label(&tr("cutting.remove")).click();
        h.run_steps(3);
        assert_eq!(project(&h).cutting_plans.len(), 1);
        for _ in 0..3 {
            undo(&mut h);
        }
        assert_eq!(project(&h), before);
    });
}

// ----- US-15.3 / US-13.5 : métrage et exports ----------------------------------------------------

fn metrage_harness() -> Harness<'static, App> {
    let launch = Launch {
        demo: true,
        stage: Some("plan-coupe-metrage".into()),
        ..Launch::default()
    };
    let app = App::with_context(&egui::Context::default(), launch);
    let mut h = Harness::builder()
        .with_size(egui::vec2(1600.0, 1000.0))
        .build_state(|ctx, app: &mut App| app.ui(ctx), app);
    h.run_steps(6);
    h
}

fn tmp(name: &str) -> std::path::PathBuf {
    let d = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target")
        .join(format!("us153-ui-{name}-{}", pf_i18n::lang().code()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d.canonicalize().unwrap()
}

#[test]
fn le_metrage_d_un_plan_valide_est_lu_a_la_main() {
    both_languages(|| {
        let h = metrage_harness();
        h.get_by_label(&tr("cutting.yield"));
        // Devant x 2 (200 x 100 ... 400 x 500 = 200 000 mm² chacun), Manche (300 + 200) / 2 x 420 =
        // 105 000, Poche 120 x 150 = 18 000 : 523 000 mm² = 5230 cm². Longueur = 510 + 150 = 660.
        // Laize utile 1400 - 40 = 1360 ; rendement 523 000 / (1360 x 660) = 58,27 %.
        h.get_by_label("660 mm");
        h.get_by_label("1360 mm");
        h.get_by_label("5230 cm²");
        h.get_by_label("58.27 %");
        h.get_by_label(&tr_args(
            "cutting.yield.row",
            &[
                ("size", &tr("cutting.size.base")),
                ("pieces", "4"),
                ("area", "5230"),
                ("share", "58.27"),
            ],
        ));
        assert!(project(&h)
            .analyze_cutting_plan(plan(&h).id)
            .unwrap()
            .is_valid());
    });
}

#[test]
fn un_plan_invalide_n_affiche_aucun_chiffre_de_metrage() {
    both_languages(|| {
        let mut h = harness();
        h.state_mut().cutting.as_mut().unwrap().yield_open = true;
        h.run_steps(3);
        h.get_by_label(&tr("cutting.yield.invalid"));
        assert!(h.query_by_label(&tr("cutting.yield.percent")).is_none());
        assert!(h.query_by_label(&tr("cutting.yield.length")).is_none());
        // Corriger (retirer la Poche qui déborde et celle qui chevauche) : les chiffres reviennent.
        let ids: Vec<_> = plan(&h).placements.iter().map(|p| p.id).collect();
        let p4 = ids[3];
        let p5 = ids[4];
        let plan_id = plan(&h).id;
        for pl in [p4, p5] {
            h.state_mut()
                .editor
                .exec(pf_core::Command::RemovePlacement {
                    plan: plan_id,
                    placement: pl,
                })
                .unwrap();
        }
        h.run_steps(3);
        assert!(project(&h)
            .analyze_cutting_plan(plan(&h).id)
            .unwrap()
            .is_valid());
        h.get_by_label(&tr("cutting.yield.percent"));
    });
}

#[test]
fn un_plan_sans_pose_est_non_renseigne() {
    both_languages(|| {
        let mut h = harness();
        h.state_mut()
            .editor
            .exec(pf_core::Command::AddCuttingPlan {
                spec: pf_core::CuttingPlanSpec {
                    name: "Vide".into(),
                    material: None,
                    fabric_width_mm: 1000.0,
                    fold: pf_core::FabricFold::Flat,
                    selvedge_mm: 10.0,
                    min_gap_mm: 0.0,
                    grain_direction: None,
                },
            })
            .unwrap();
        let id = project(&h).cutting_plans[1].id;
        {
            let d = h.state_mut().cutting.as_mut().unwrap();
            d.selected = Some(id);
            d.yield_open = true;
        }
        h.run_steps(3);
        h.get_by_label(&tr("cutting.yield.empty"));
    });
}

#[test]
fn les_exports_ecrivent_les_octets_de_la_bibliotheque_sans_toucher_au_projet() {
    both_languages(|| {
        use crate::cutting::CutOut;
        let mut h = metrage_harness();
        for key in [
            "cutting.export.svg",
            "cutting.export.pdf",
            "cutting.export.csv",
        ] {
            h.get_by_label(&tr(key));
        }
        let before = project(&h);
        let id = plan(&h).id;
        let dir = tmp("export");
        let lang = pf_i18n::lang();
        let setup = h.state().page_setup;
        for (what, name) in [
            (CutOut::Svg, "plan.svg"),
            (CutOut::Pdf, "plan.pdf"),
            (CutOut::Csv, "metrage.csv"),
        ] {
            let path = dir.join(name);
            assert!(h.state_mut().write_cutting_to(id, what, &path));
            let want = match what {
                CutOut::Svg => pf_export::cutting_export::cutting_svg(&before, id, lang).unwrap(),
                CutOut::Pdf => {
                    pf_export::cutting_export::cutting_pdf(&before, id, &setup, lang).unwrap()
                }
                CutOut::Csv => pf_export::cutting_export::cutting_csv(&before, lang).unwrap(),
            };
            assert_eq!(
                std::fs::read(&path).unwrap(),
                want,
                "{name} : octets de la bibliothèque"
            );
        }
        assert_eq!(project(&h), before, "lecture seule");
        // Destination impossible : boîte d'erreur, aucun fichier.
        let bad = dir.join("absent").join("plan.svg");
        assert!(!h.state_mut().write_cutting_to(id, CutOut::Svg, &bad));
        assert!(h.state().error_dialog.is_some());
        assert!(!bad.exists());
    });
}
