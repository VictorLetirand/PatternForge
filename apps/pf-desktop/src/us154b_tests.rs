//! US-15.4, mode « compléter » (tranche 133, egui, français et anglais) : les placements existants
//! restent tels quels, seules les pièces manquantes sont posées, en une étape annulable ; refus
//! nommés. Valeurs de l'étape `plan-coupe-auto-completer` écrites à la main : plan du Coton 140
//! (laize utile 1360, écart 10) avec Devant 400 x 500 (paire), Manche (trapèze 300 x 420) et Poche
//! 120 x 150 ; la Poche est retirée. Placement actuel : 400 000 + 105 000 = 505 000 mm2 sur
//! 1360 x 500 = 74,26 % ; avec la Poche reposée (18 000 mm2, à droite de la Manche, en x = 1210) :
//! 523 000 / 680 000 = 76,91 %, longueur inchangée (500).

use crate::app::App;
use crate::Launch;
use eframe::egui;
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use pf_core::cutting_auto::AutoMode;
use pf_core::{Command, Project};
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

fn wait_for_result(h: &mut Harness<'_, App>) {
    for _ in 0..400 {
        h.step();
        if h.state().cutting.as_ref().unwrap().auto_out.is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    h.run_steps(2);
}

#[test]
fn completer_garde_l_existant_pose_le_manquant_en_une_etape() {
    both_languages(|| {
        let mut h = harness("plan-coupe-auto-completer");
        let before = project(&h);
        let kept = before.cutting_plans[0].placements.clone();
        assert_eq!(kept.len(), 3);
        h.get_by_label(&tr("cutting.auto.mode.complete.note"));
        h.get_by_label(&tr_args(
            "cutting.auto.current",
            &[("l", "500"), ("y", "74.26")],
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
        assert_eq!(&plan.placements[..3], &kept[..]);
        assert!(p.analyze_cutting_plan(plan.id).unwrap().is_valid());
        // Une seule étape annulable : retour exact à l'état d'avant.
        h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
        h.run_steps(3);
        assert_eq!(project(&h), before);
    });
}

#[test]
fn rien_a_completer_est_dit_sans_proposition_ni_modification() {
    both_languages(|| {
        let mut h = harness("plan-coupe-auto-completer");
        // Une première complétion appliquée : toutes les quantités sont alors posées.
        h.get_by_label(&tr("cutting.auto.apply")).click();
        h.run_steps(3);
        let before = project(&h);
        assert_eq!(before.cutting_plans[0].placements.len(), 4);
        h.get_by_label(&tr("cutting.auto.run")).click();
        h.run_steps(4);
        wait_for_result(&mut h);
        h.get_by_label(&tr("cutting.auto.err.nothing_to_complete"));
        assert!(h.query_by_label(&tr("cutting.auto.apply")).is_none());
        assert_eq!(project(&h), before);
        // Aucune commande exécutée : un seul pas d'annulation (celui de la complétion).
        h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
        h.run_steps(3);
        assert_eq!(project(&h).cutting_plans[0].placements.len(), 3);
    });
}

#[test]
fn existant_chevauchant_est_refuse_avec_les_chiffres() {
    both_languages(|| {
        let mut h = harness("plan-coupe-auto-completer");
        let plan = project(&h).cutting_plans[0].id;
        let d = project(&h).cutting_plans[0].placements[0];
        h.state_mut()
            .measure_exec(Command::AddPlacement {
                plan,
                piece: d.piece,
                size: d.size,
                x_mm: d.x_mm,
                y_mm: d.y_mm,
                rotation_deg: d.rotation_deg,
                mirrored: d.mirrored,
            })
            .unwrap();
        h.run_steps(3);
        let before = project(&h);
        h.get_by_label(&tr("cutting.auto.run")).click();
        h.run_steps(4);
        wait_for_result(&mut h);
        h.get_by_label(&tr_args(
            "cutting.auto.err.existing_invalid",
            &[
                ("overlaps", "1"),
                ("gaps", "0"),
                ("overflows", "0"),
                ("rotations", "0"),
                ("unreadable", "0"),
            ],
        ));
        assert!(h.query_by_label(&tr("cutting.auto.apply")).is_none());
        assert_eq!(project(&h), before);
    });
}

#[test]
fn choisir_le_mode_efface_la_proposition_et_n_est_pas_enregistre() {
    both_languages(|| {
        let mut h = harness("plan-coupe-auto");
        let before = project(&h);
        assert_eq!(
            h.state().cutting.as_ref().unwrap().auto_mode,
            AutoMode::ReplaceAll
        );
        h.get_by_label(&tr("cutting.auto.mode.complete")).click();
        h.run_steps(3);
        let d = h.state().cutting.as_ref().unwrap();
        assert_eq!(d.auto_mode, AutoMode::CompleteMissing);
        assert!(d.auto_out.is_none());
        h.get_by_label(&tr("cutting.auto.mode.complete.note"));
        h.get_by_label(&tr("cutting.auto.mode.replace")).click();
        h.run_steps(3);
        assert_eq!(
            h.state().cutting.as_ref().unwrap().auto_mode,
            AutoMode::ReplaceAll
        );
        assert!(h
            .query_by_label(&tr("cutting.auto.mode.complete.note"))
            .is_none());
        // État d'interface seulement : le projet n'a pas changé.
        assert_eq!(project(&h), before);
    });
}
