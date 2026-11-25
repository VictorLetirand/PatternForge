//! US-15.5 « Cuir » (tranche 140, egui, français et anglais) : étape `cuir-peau` (plan « Peau »,
//! contour de 5 sommets, trou 40 x 40, cicatrice, zones de qualité 1 et 3) avec un Devant posé
//! sur le trou et une Poche qui sort du coin ; valeurs **écrites à la main** :
//! - Devant 400 x 500 en (0 ; 0) : recoupe le trou (40 x 40) = 1 600 mm2 ; tout en zone 1
//!   (200 000 mm2 = 2 000 cm2) ;
//! - Poche 120 x 150 en (1350 ; 650) : dedans = intégrale de (50 + 2t/7) pour t de 0 à 50 =
//!   2 500 + 357,142857 = 2 857,142857 mm2, donc 18 000 - 2 857,142857 = 15 142,857143 mm2 dehors
//!   (15 142,9 affiché) ; en zone 3 : 50 x 50 = 2 500 mm2 = 25 cm2.

use crate::app::App;
use crate::cutting_hide::{HideAct, Target};
use crate::Launch;
use eframe::egui;
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use pf_core::{Command, DefectKind, PlanePoint, Project};
use pf_i18n::{tr, tr_args, Lang};

fn harness(stage: &str) -> Harness<'static, App> {
    let launch = Launch {
        demo: true,
        stage: Some(stage.into()),
        ..Launch::default()
    };
    let app = App::with_context(&egui::Context::default(), launch);
    let mut h = Harness::builder()
        .with_size(egui::vec2(1600.0, 1400.0))
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
fn peau_signale_defaut_debord_et_qualite_avec_les_valeurs_a_la_main() {
    both_languages(|| {
        let h = harness("cuir-peau");
        let p = project(&h);
        let plan = p.cutting_plans.last().unwrap();
        let hole = plan.hide.as_ref().unwrap().defects[0].id;
        // Les lignes de contrôle sont préfixées de « ✗ » par la section Contrôles.
        h.get_by_label(&format!(
            "✗ {}",
            tr_args(
                "cutting.hide.diag.hit",
                &[
                    ("name", "Devant #1"),
                    ("id", &hole.0.to_string()),
                    ("kind", &tr("cutting.hide.kind.hole")),
                    ("area", "1600"),
                ],
            )
        ));
        h.get_by_label(&format!(
            "✗ {}",
            tr_args(
                "cutting.hide.diag.overflow",
                &[("name", "Poche #2"), ("area", "15142.9")],
            )
        ));
        // Qualité des poses : pire grade et aires par grade (cm2).
        h.get_by_label(&tr_args(
            "cutting.hide.quality.row",
            &[
                ("name", "Devant #1"),
                ("worst", "1"),
                ("a1", "2000"),
                ("a2", "0"),
                ("a3", "0"),
            ],
        ));
        h.get_by_label(&tr_args(
            "cutting.hide.quality.row",
            &[
                ("name", "Poche #2"),
                ("worst", "3"),
                ("a1", "0"),
                ("a2", "0"),
                ("a3", "25"),
            ],
        ));
        // Plan invalide : aucun chiffre de métrage cuir.
        h.get_by_label(&tr("cutting.hide.yield.invalid"));
        // Rien n'a été modifié par la lecture.
        assert_eq!(project(&h), p);
    });
}

#[test]
fn saisie_par_points_defaut_puis_retrait_en_un_pas_annulable() {
    both_languages(|| {
        let mut h = harness("cuir-peau");
        let before = project(&h);
        let plan = before.cutting_plans.last().unwrap().id;
        // Tracé par points saisis (3 sommets) d'un défaut « tache », terminé par « Finish ».
        let mut dialog = h.state_mut().cutting.take().unwrap();
        dialog.hide.target = Target::Defect;
        dialog.hide.kind = DefectKind::Stain;
        dialog.hide.drawing = true;
        dialog.hide.points = vec![(900.0, 200.0), (960.0, 200.0), (930.0, 260.0)];
        let res = h.state_mut().hide_act(&mut dialog, HideAct::Finish);
        assert_eq!(res, Ok(()));
        assert!(!dialog.hide.drawing && dialog.hide.points.is_empty());
        h.state_mut().cutting = Some(dialog);
        h.run_steps(3);
        let after = project(&h);
        let hide = after.cutting_plan(plan).unwrap().hide.clone().unwrap();
        assert_eq!(hide.defects.len(), 3);
        assert_eq!(hide.defects[2].kind, DefectKind::Stain);
        // Un seul pas d'annulation.
        h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
        h.run_steps(3);
        assert_eq!(project(&h), before);
        // Refus nommé sans trace : polygone qui sort du contour.
        let mut dialog = h.state_mut().cutting.take().unwrap();
        dialog.hide.target = Target::Defect;
        dialog.hide.points = vec![(1390.0, 10.0), (1500.0, 10.0), (1500.0, 40.0)];
        let res = h.state_mut().hide_act(&mut dialog, HideAct::Finish);
        assert!(res.is_err(), "{res:?}");
        h.state_mut().cutting = Some(dialog);
        assert_eq!(project(&h), before);
        // Moins de 3 points : refus propre.
        let mut dialog = h.state_mut().cutting.take().unwrap();
        dialog.hide.points = vec![(1.0, 1.0)];
        let res = h.state_mut().hide_act(&mut dialog, HideAct::Finish);
        assert_eq!(res, Err(tr("cutting.hide.err.points")));
        h.state_mut().cutting = Some(dialog);
    });
}

#[test]
fn proposition_sur_peau_relue_et_appliquee_en_une_etape() {
    both_languages(|| {
        let mut h = harness("cuir-peau-auto");
        let before = project(&h);
        let plan = before.cutting_plans.last().unwrap().id;
        assert!(before.cutting_plan(plan).unwrap().placements.is_empty());
        let out = h.state().cutting.as_ref().unwrap().auto_out.clone();
        match out {
            Some(Ok(_)) => {
                h.get_by_label(&tr("cutting.auto.apply")).click();
                h.run_steps(3);
                let p = project(&h);
                let an = p.analyze_cutting_plan(plan).unwrap();
                assert!(an.is_valid(), "{an:?}");
                assert!(an.defect_hits.is_empty() && an.hide_overflows.is_empty());
                h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
                h.run_steps(3);
                assert_eq!(project(&h), before);
            }
            other => panic!("proposition attendue : {other:?}"),
        }
    });
}

#[test]
fn commande_directe_contour_depuis_l_interface_est_refusee_sans_trace() {
    let mut h = harness("cuir-peau");
    let before = project(&h);
    let plan = before.cutting_plans.last().unwrap().id;
    let bow = vec![
        PlanePoint { x: 0.0, y: 0.0 },
        PlanePoint { x: 10.0, y: 10.0 },
        PlanePoint { x: 10.0, y: 0.0 },
        PlanePoint { x: 0.0, y: 10.0 },
    ];
    let mut dialog = h.state_mut().cutting.take().unwrap();
    let res = h.state_mut().hide_act(
        &mut dialog,
        HideAct::Cmd(Command::SetHideContour { plan, contour: bow }),
    );
    assert!(res.is_err());
    h.state_mut().cutting = Some(dialog);
    assert_eq!(project(&h), before);
}
