//! PH-6 : passe visuelle sur les écrans ajoutés depuis PH-2 / PH-5 (gradation, mesures, piqûre,
//! zones, journal, aide). Un test par défaut corrigé (FR et EN) et des invariants de géométrie de
//! fenêtre pour les écrans qui n'en avaient pas. Tests egui_kittest, sans comparaison de pixels.

use crate::app::App;
use crate::Launch;
use eframe::egui;
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use pf_i18n::{tr, Lang};

const SIZES: [[f32; 2]; 2] = [[1280.0, 800.0], [1600.0, 1000.0]];

fn harness(stage: &str, size: [f32; 2]) -> Harness<'static, App> {
    let launch = Launch {
        demo: true,
        stage: Some(stage.into()),
        ..Launch::default()
    };
    let app = App::with_context(&egui::Context::default(), launch);
    let mut h = Harness::builder()
        .with_size(egui::vec2(size[0], size[1]))
        .build_state(|ctx, app: &mut App| app.ui(ctx), app);
    h.run_steps(8);
    h
}

fn panel_rect(h: &Harness<'_, App>, id: &str) -> Option<egui::Rect> {
    egui::containers::panel::PanelState::load(&h.ctx, egui::Id::new(id)).map(|s| s.rect)
}

fn area(h: &Harness<'_, App>, id: egui::Id) -> Option<egui::Rect> {
    h.ctx.memory(|m| m.area_rect(id))
}

fn in_both_languages(mut check: impl FnMut(Lang)) {
    for lang in [Lang::Fr, Lang::En] {
        pf_i18n::set_lang(lang);
        check(lang);
    }
    pf_i18n::set_lang(Lang::Fr);
}

// ----- Défaut 1 : bloc ouvert par une étape, titre hors de la vue ----------------------------

/// Le défilement automatique visait le corps du bloc : son titre sortait de la vue et le chevron de
/// repli recouvrait la première ligne. Le titre doit être visible, sous le chevron (22 pt).
#[test]
fn ph6_bloc_ouvert_par_une_etape_garde_son_titre_visible_sous_le_chevron() {
    in_both_languages(|lang| {
        for size in SIZES {
            for (stage, key) in [
                ("piqure", "stitch.title"),
                ("controle-piqure", "stitch.title"),
                ("zones", "zone.title"),
                ("percages-au-pas", "piece.marking.run.title"),
            ] {
                let h = harness(stage, size);
                let panel = panel_rect(&h, "sheets").expect("panneau des feuilles");
                let text = tr(key);
                let text = text.trim_end_matches('\u{2026}');
                // Le panneau garde un nœud par fiche de pièce : on prend celui qui s'y trouve.
                let title = h
                    .query_all_by_label_contains(text)
                    .map(|n| n.rect())
                    .find(|r| r.min.x >= panel.min.x && r.max.x <= panel.max.x)
                    .unwrap_or_else(|| {
                        let all: Vec<_> = h
                            .query_all_by_label_contains(text)
                            .map(|n| n.rect())
                            .collect();
                        panic!("{lang:?} {stage} : titre « {text} » introuvable dans {panel:?} : {all:?}")
                    });
                // Chevron de repli : 22 pt dans l'angle haut droit du panneau (hors gouttière).
                let chevron = egui::Rect::from_min_size(
                    egui::pos2(panel.right() - 32.0, panel.top()),
                    egui::vec2(22.0, 22.0),
                );
                assert!(
                    title.min.y >= panel.top()
                        && title.max.y <= panel.bottom() - 40.0
                        && !title.intersects(chevron),
                    "{lang:?} {size:?} {stage} : titre hors zone ou sous le chevron {title:?} / {panel:?} ({:?})",
                    h.query_all_by_label_contains(text).map(|n| n.rect()).collect::<Vec<_>>()
                );
            }
        }
    });
}

// ----- Défaut 2 : fenêtres centrées plus hautes que l'espace libre ---------------------------

/// L'aide et la mise en page du PDF débordaient sur la barre d'état.
#[test]
fn ph6_aide_et_mise_en_page_ne_recouvrent_pas_la_barre_d_etat() {
    in_both_languages(|lang| {
        for size in SIZES {
            for (stage, id) in [
                ("aide", "help_window"),
                ("impression", "print_setup_window"),
            ] {
                let h = harness(stage, size);
                let status = panel_rect(&h, "status").expect("barre d'état");
                let rect = area(&h, egui::Id::new(id))
                    .unwrap_or_else(|| panic!("{lang:?} {stage} : fenêtre sans rectangle"));
                assert!(
                    rect.max.y <= status.top() + 0.5,
                    "{lang:?} {size:?} {stage} : la fenêtre {rect:?} recouvre la barre d'état {status:?}"
                );
            }
        }
    });
}

// ----- Défaut 3 : formulaire « Nouvelle mesure finie » ----------------------------------------

/// « Tol. + » finissait seul au bout de la 1re rangée et son champ au début de la 2e.
#[test]
fn ph6_nouvelle_mesure_finie_garde_chaque_tolerance_avec_son_champ() {
    in_both_languages(|lang| {
        for size in SIZES {
            let h = harness("retouches", size);
            for key in ["finished.fin.tol_minus", "finished.fin.tol_plus"] {
                let text = tr(key);
                // Le formulaire de création est le dernier de la fenêtre (sous les mesures existantes).
                let label = h
                    .query_all_by_label(&text)
                    .map(|n| n.rect())
                    .max_by(|a, b| a.min.y.total_cmp(&b.min.y))
                    .unwrap_or_else(|| panic!("{lang:?} : « {text} » introuvable"));
                let field = h
                    .query_all_by_role(egui::accesskit::Role::TextInput)
                    .map(|n| n.rect())
                    .find(|r| {
                        (r.center().y - label.center().y).abs() < 10.0
                            && r.min.x >= label.max.x - 1.0
                            && r.min.x - label.max.x < 30.0
                    });
                assert!(
                    field.is_some(),
                    "{lang:?} {size:?} : « {text} » {label:?} sans son champ sur la même rangée"
                );
            }
        }
    });
}

// ----- Défaut 4 : journal d'erreurs ------------------------------------------------------------

/// Le journal affichait les types bruts « [status] », « [conflict] », « [dialog] » : ils sont
/// traduits (le fichier exporté garde les types techniques).
#[test]
fn ph6_journal_d_erreurs_affiche_des_types_traduits() {
    in_both_languages(|lang| {
        let h = harness("journal-erreurs", [1280.0, 800.0]);
        for raw in ["[status]", "[conflict]", "[dialog]", "[lock]"] {
            assert!(
                h.query_by_label_contains(raw).is_none(),
                "{lang:?} : type brut {raw} affiché"
            );
        }
        for key in [
            "errlog.kind.status",
            "errlog.kind.conflict",
            "errlog.kind.dialog",
        ] {
            let text = tr(key);
            assert!(
                h.query_by_label_contains(&text).is_some(),
                "{lang:?} : « {text} » absent du journal"
            );
        }
    });
}

#[test]
fn ph6_les_cles_ajoutees_existent_en_fr_et_en() {
    for key in [
        "errlog.kind.status",
        "errlog.kind.conflict",
        "errlog.kind.dialog",
        "errlog.kind.lock",
    ] {
        for lang in [Lang::Fr, Lang::En] {
            let found = lang.catalog().iter().filter(|(k, _)| *k == key).count();
            assert_eq!(found, 1, "{lang:?} {key}");
        }
    }
}

// ----- Invariants de géométrie des écrans sans test jusque-là -----------------------------------

/// Fenêtres du dock des écrans de barème, fiches, retouches, variables, espace de travail :
/// dans l'écran, sans recouvrir feuilles, inspecteur, barre d'outils ni canevas.
#[test]
fn ph6_les_fenetres_du_dock_des_nouveaux_ecrans_ne_recouvrent_rien() {
    for size in SIZES {
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(size[0], size[1]));
        for stage in [
            "bareme",
            "fiche-client",
            "fiche-client-taille",
            "retouches",
            "variables",
            "taille-valeurs",
            "diagnostic",
            "dependances",
            "espace-gradation",
            "nomenclature",
            "variantes",
        ] {
            let h = harness(stage, size);
            let open = h.state().dock.open().to_vec();
            assert!(!open.is_empty(), "{stage} : aucune fenêtre ouverte");
            let sheets = if h.state().sheets_folded(&h.ctx) {
                "sheets_folded"
            } else {
                "sheets"
            };
            let protected = [
                ("feuilles", panel_rect(&h, sheets).expect("feuilles")),
                (
                    "inspecteur",
                    panel_rect(&h, "inspector").expect("inspecteur"),
                ),
                ("barre d'outils", panel_rect(&h, "tools").expect("outils")),
                ("barre d'état", panel_rect(&h, "status").expect("état")),
                ("canevas", h.state().canvas_rect),
            ];
            for win in open {
                let rect = area(&h, win.id()).expect("rectangle");
                assert!(
                    screen.contains_rect(rect),
                    "{size:?} {stage} {win:?} hors écran"
                );
                for (name, zone) in protected {
                    assert!(
                        !rect.intersects(zone),
                        "{size:?} {stage} {win:?} {rect:?} recouvre {name} {zone:?}"
                    );
                }
            }
            assert!(
                h.state().canvas_rect.width() >= 400.0,
                "{size:?} {stage} : canevas de {} pt",
                h.state().canvas_rect.width()
            );
        }
    }
}

/// Fenêtres flottantes (historique, préférences, import AAMA, journal) : entièrement dans l'écran,
/// sous la barre d'outils, hors de l'inspecteur et de la barre d'état.
#[test]
fn ph6_les_fenetres_flottantes_restent_dans_la_zone_de_travail() {
    for size in SIZES {
        for (stage, id) in [
            ("historique", egui::Id::new(tr("history.title"))),
            ("preferences", egui::Id::new(tr("prefs.title"))),
            ("import-aama", egui::Id::new("aama_import_window")),
            ("journal-erreurs", egui::Id::new(tr("errlog.title"))),
        ] {
            let h = harness(stage, size);
            let rect = area(&h, id).unwrap_or_else(|| panic!("{stage} : fenêtre introuvable"));
            let tools = panel_rect(&h, "tools").expect("outils");
            let inspector = panel_rect(&h, "inspector").expect("inspecteur");
            let status = panel_rect(&h, "status").expect("état");
            assert!(
                rect.top() >= tools.bottom() - 0.5,
                "{size:?} {stage} {rect:?}"
            );
            assert!(
                rect.bottom() <= status.top() + 0.5,
                "{size:?} {stage} {rect:?}"
            );
            assert!(!rect.intersects(inspector), "{size:?} {stage} {rect:?}");
            assert!(rect.left() >= 0.0, "{size:?} {stage} {rect:?}");
        }
    }
}
