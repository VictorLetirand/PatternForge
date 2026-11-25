//! US-15.4 : placement automatique dans la fenêtre « Plan de coupe » (egui, français et anglais) :
//! proposition calculée dans un fil séparé, aperçu avant validation (longueur et rendement
//! comparés), une seule étape annulable, refus clairs. Valeurs de l'étape `plan-coupe-auto` écrites
//! à la main : Devant 400 x 500 (paire : 1 + 1 en miroir), Manche 300 x 420, Poche 120 x 150 sur
//! une laize utile de 1360 mm, écart 10 : 400 + 10 + 400 + 10 + 300 + 10 + 120 = 1250 <= 1360, donc
//! une seule rangée de longueur 500 ; surface 523 000 mm2, rendement 523 000 / (1360 x 500) = 76,91 %.

use crate::app::App;
use crate::Launch;
use eframe::egui;
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use pf_core::Project;
use pf_i18n::{tr, tr_args, Lang};

fn harness(stage: &str) -> Harness<'static, App> {
    let launch = Launch {
        demo: true,
        stage: Some(stage.into()),
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

fn both_languages(f: impl Fn()) {
    for lang in [Lang::Fr, Lang::En] {
        pf_i18n::set_lang(lang);
        f();
    }
    pf_i18n::set_lang(Lang::Fr);
}

#[test]
fn la_proposition_est_comparee_puis_appliquee_en_une_etape() {
    both_languages(|| {
        let mut h = harness("plan-coupe-auto");
        let before = project(&h);
        h.get_by_label(&tr("cutting.auto.label"));
        h.get_by_label(&tr_args(
            "cutting.auto.current",
            &[("l", "660"), ("y", "58.27")],
        ));
        h.get_by_label(&tr_args(
            "cutting.auto.proposed",
            &[("l", "500"), ("y", "76.91")],
        ));
        // Rien n'est appliqué tant que l'utilisateur n'a pas validé.
        assert_eq!(project(&h), before);
        h.get_by_label(&tr("cutting.auto.apply")).click();
        h.run_steps(3);
        let p = project(&h);
        let plan = &p.cutting_plans[0];
        assert_eq!(plan.placements.len(), 4);
        assert!(p.analyze_cutting_plan(plan.id).unwrap().is_valid());
        let y = p.cutting_plan_yield(plan.id).unwrap().figures.unwrap();
        assert!((y.length_mm - 500.0).abs() < 1e-6, "{}", y.length_mm);
        assert!((y.yield_percent - 76.91).abs() < 0.01);
        assert!(h.state().cutting.as_ref().unwrap().auto_out.is_none());
        // Une seule étape annulable.
        h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
        h.run_steps(3);
        assert_eq!(project(&h), before);
    });
}

#[test]
fn ecarter_la_proposition_ne_change_rien() {
    both_languages(|| {
        let mut h = harness("plan-coupe-auto");
        let before = project(&h);
        h.get_by_label(&tr("cutting.auto.discard")).click();
        h.run_steps(3);
        assert!(h.query_by_label(&tr("cutting.auto.label")).is_none());
        assert_eq!(project(&h), before);
    });
}

#[test]
fn le_calcul_se_fait_dans_un_fil_separe_et_rend_la_meme_proposition() {
    both_languages(|| {
        let mut h = harness("plan-coupe-auto");
        h.get_by_label(&tr("cutting.auto.discard")).click();
        h.run_steps(3);
        h.get_by_label(&tr("cutting.auto.run")).click();
        for _ in 0..400 {
            h.step();
            if h.state().cutting.as_ref().unwrap().auto_out.is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        h.run_steps(2);
        h.get_by_label(&tr_args(
            "cutting.auto.proposed",
            &[("l", "500"), ("y", "76.91")],
        ));
        assert!(h.state().cutting.as_ref().unwrap().auto_job.is_none());
    });
}

#[test]
fn sans_piece_cochee_le_refus_est_dit_sans_modifier_le_projet() {
    both_languages(|| {
        let mut h = harness("plan-coupe-auto");
        let before = project(&h);
        h.state_mut().cutting.as_mut().unwrap().auto_all = false;
        h.get_by_label(&tr("cutting.auto.discard")).click();
        h.run_steps(3);
        h.get_by_label(&tr("cutting.auto.run")).click();
        h.run_steps(4);
        h.get_by_label(&tr("cutting.auto.err.nothing"));
        assert_eq!(project(&h), before);
    });
}

#[test]
fn laize_trop_etroite_liste_les_pieces_non_posables() {
    both_languages(|| {
        let mut h = harness("plan-coupe-auto");
        let id = project(&h).cutting_plans[0].id;
        // Laize 300, lisières 20 : utile 260 ; le Devant (400) ne tient pas, droit-fil vertical.
        let mut spec = pf_core::CuttingPlanSpec {
            name: "Coton 140".into(),
            material: None,
            fabric_width_mm: 300.0,
            fold: pf_core::FabricFold::Flat,
            selvedge_mm: 20.0,
            min_gap_mm: 10.0,
            grain_direction: Some(90.0),
        };
        spec.name = project(&h).cutting_plans[0].name.clone();
        h.state_mut()
            .measure_exec(pf_core::Command::UpdateCuttingPlan { plan: id, spec })
            .unwrap();
        h.run_steps(3);
        h.get_by_label(&tr("cutting.auto.run")).click();
        h.run_steps(4);
        for _ in 0..400 {
            if h.state().cutting.as_ref().unwrap().auto_out.is_some() {
                break;
            }
            h.step();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        h.run_steps(2);
        h.get_by_label(&tr("cutting.auto.err.unplaceable"));
        h.get_by_label(&tr_args(
            "cutting.auto.err.too_wide",
            &[("name", "Devant"), ("width", "400"), ("span", "260")],
        ));
    });
}
