//! PH-2b : fenêtres d'outil rangées dans un dock, Échap, mode plan sans fiche, boîtes modales,
//! valeurs longues, grille des boutons, remarques repliées. Tests egui_kittest.

use crate::app::App;
use crate::dock::ToolWin;
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
    h.run_steps(6);
    h
}

fn panel_rect(h: &Harness<'_, App>, id: &str) -> Option<egui::Rect> {
    egui::containers::panel::PanelState::load(&h.ctx, egui::Id::new(id)).map(|s| s.rect)
}

fn area_rect(h: &Harness<'_, App>, win: ToolWin) -> Option<egui::Rect> {
    h.ctx.memory(|m| m.area_rect(win.id()))
}

// ----- CA1 : fenêtres d'outil rangées dans le dock -------------------------------------------

/// Étapes de capture qui ouvrent au moins une fenêtre d'outil.
const TOOL_STAGES: &[&str] = &[
    "pli",
    "pli-creux",
    "pince",
    "ampleur",
    "decoupe",
    "raccords",
    "coutures",
    "transformer",
    "alignement",
    "assembler",
    "deplier",
    "construction-rapide",
    "rapport",
    "tailles",
    "gradation",
    "gradation-report",
    "mesures-tailles",
    "mesures-export",
    "nomenclature",
    "variantes",
    "versions",
    "nid-tailles",
    "controle-forme",
    "export-gradue",
];

#[test]
fn aucune_fenetre_d_outil_ne_recouvre_le_travail_ni_l_inspecteur_ni_les_feuilles() {
    for size in [[1280.0_f32, 800.0], [1600.0, 1000.0]] {
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(size[0], size[1]));
        let mut windows_seen = 0;
        for stage in TOOL_STAGES {
            let h = harness(Some(stage), size);
            let open: Vec<ToolWin> = h.state().dock.open().to_vec();
            assert!(!open.is_empty(), "{stage} : aucune fenêtre d'outil ouverte");
            // Zones protégées : relevées sur la mise en page egui (panneaux, canevas), pas sur
            // le code d'ancrage.
            let protected = [
                (
                    "feuilles",
                    panel_rect(
                        &h,
                        if h.state().sheets_folded(&h.ctx) {
                            "sheets_folded"
                        } else {
                            "sheets"
                        },
                    )
                    .expect("panneau des feuilles"),
                ),
                (
                    "inspecteur",
                    panel_rect(&h, "inspector").expect("inspecteur"),
                ),
                (
                    "barre d'outils",
                    panel_rect(&h, "tools").expect("barre d'outils"),
                ),
                ("canevas", h.state().canvas_rect),
            ];
            for win in open {
                let rect = area_rect(&h, win)
                    .unwrap_or_else(|| panic!("{stage} : fenêtre {win:?} sans rectangle"));
                windows_seen += 1;
                assert!(
                    screen.contains_rect(rect),
                    "{size:?} {stage} {win:?} : hors écran {rect:?}"
                );
                for (name, zone) in protected {
                    assert!(
                        !rect.intersects(zone),
                        "{size:?} {stage} {win:?} {rect:?} recouvre {name} {zone:?}"
                    );
                }
            }
            // Le canevas se rétrécit de la colonne du dock mais reste là : 400 points au moins
            // (PH-2c : panneau des feuilles replié en bande étroite quand il le faut).
            let least = 400.0;
            assert!(
                h.state().canvas_rect.width() >= least,
                "{size:?} {stage} : canevas de {} px",
                h.state().canvas_rect.width()
            );
        }
        assert!(windows_seen >= TOOL_STAGES.len(), "{size:?}");
    }
}

#[test]
fn plusieurs_fenetres_s_empilent_sans_se_recouvrir_et_restent_dans_le_dock() {
    for size in [[1280.0_f32, 800.0], [1600.0, 1000.0]] {
        let mut h = harness(Some("raccords"), size);
        // Coutures + Vue assemblée, puis Plis et Ampleur par-dessus.
        h.state_mut().open_pleats();
        h.state_mut().open_flare();
        h.run_steps(8);
        let open: Vec<ToolWin> = h.state().dock.open().to_vec();
        assert!(open.len() >= 4, "{open:?}");
        let dock = h.state().dock.rect;
        let rects: Vec<(ToolWin, egui::Rect)> = open
            .iter()
            .map(|w| (*w, area_rect(&h, *w).expect("rectangle")))
            .collect();
        for (win, rect) in &rects {
            assert!(
                dock.contains_rect(*rect),
                "{size:?} {win:?} {rect:?} sort du dock {dock:?}"
            );
        }
        for (i, (a, ra)) in rects.iter().enumerate() {
            for (b, rb) in &rects[i + 1..] {
                assert!(
                    !ra.intersects(*rb),
                    "{size:?} {a:?} {ra:?} recouvre {b:?} {rb:?}"
                );
            }
        }
    }
}

#[test]
fn le_dock_n_existe_que_tant_qu_une_fenetre_est_ouverte() {
    let mut h = harness(None, [1280.0, 800.0]);
    let free = h.state().canvas_rect.width();
    assert!(h.state().dock.open().is_empty());
    h.state_mut().open_pleats();
    h.run_steps(4);
    assert!(h.state().canvas_rect.width() < free, "colonne réservée");
    h.state_mut().editor.pleat.open = false;
    h.run_steps(4);
    assert!(h.state().dock.open().is_empty());
    assert_eq!(h.state().canvas_rect.width(), free, "colonne rendue");
}

// ----- CA1 : Échap -----------------------------------------------------------------------------

#[test]
fn echap_ferme_la_fenetre_la_plus_recente_sans_toucher_au_document() {
    let mut h = harness(Some("raccords"), [1600.0, 1000.0]);
    let before = h.state().editor.project().clone();
    let undo = h.state().editor.doc.can_undo();
    assert_eq!(h.state().dock.latest(), Some(ToolWin::JoinView));
    h.key_press(egui::Key::Escape);
    h.run_steps(4);
    assert!(!h.state().editor.seam.join_view, "Vue assemblée fermée");
    assert!(h.state().editor.seam.open, "Coutures reste ouverte");
    h.key_press(egui::Key::Escape);
    h.run_steps(4);
    assert!(!h.state().editor.seam.open, "Coutures fermée");
    assert!(h.state().dock.open().is_empty());
    assert_eq!(*h.state().editor.project(), before);
    assert_eq!(h.state().editor.doc.can_undo(), undo);
    assert!(!h.state().editor.doc.is_dirty());
}

#[test]
fn echap_annule_l_apercu_non_applique_d_une_fenetre() {
    let mut h = harness(Some("pli"), [1600.0, 1000.0]);
    let before = h.state().editor.project().clone();
    assert!(h.state().editor.pleat.open);
    h.key_press(egui::Key::Escape);
    h.run_steps(4);
    assert!(!h.state().editor.pleat.open);
    assert_eq!(h.state().editor.pleat.picking, 0);
    assert_eq!(*h.state().editor.project(), before);
    assert!(!h.state().editor.doc.is_dirty());
}

#[test]
fn echap_ne_ferme_pas_la_fenetre_quand_un_champ_texte_a_le_focus() {
    let mut h = harness(Some("pli"), [1600.0, 1000.0]);
    let field = h
        .get_all_by_role(egui::accesskit::Role::TextInput)
        .next()
        .expect("un champ de la fenêtre Plis");
    field.focus();
    h.run_steps(3);
    assert!(h.ctx.wants_keyboard_input(), "le champ a le focus");
    h.key_press(egui::Key::Escape);
    h.run_steps(3);
    assert!(
        h.state().editor.pleat.open,
        "pas de fermeture pendant la saisie"
    );
    assert!(!h.ctx.wants_keyboard_input(), "Échap rend le focus");
    // Une seconde pression, hors champ, ferme.
    h.key_press(egui::Key::Escape);
    h.run_steps(3);
    assert!(!h.state().editor.pleat.open);
}

#[test]
fn echap_ferme_aussi_les_fenetres_a_etat_dialogue() {
    for (stage, win) in [
        ("deplier", ToolWin::Unfold),
        ("transformer", ToolWin::Transform),
        ("rapport", ToolWin::Report),
        ("pince", ToolWin::Dart),
    ] {
        let mut h = harness(Some(stage), [1280.0, 800.0]);
        assert!(h.state().dock.open().contains(&win), "{stage}");
        h.key_press(egui::Key::Escape);
        h.run_steps(4);
        assert!(!h.state().dock.open().contains(&win), "{stage} : {win:?}");
    }
}

// ----- CA2 : mode plan ---------------------------------------------------------------------------

#[test]
fn le_mode_plan_masque_les_feuilles_et_rend_exactement_l_etat_precedent() {
    let mut h = harness(Some("fiche-riche"), [1600.0, 1000.0]);
    // État à préserver : largeur du panneau, section dépliée.
    h.ctx.data_mut(|d| {
        d.insert_persisted(
            egui::Id::new("sheets"),
            egui::containers::panel::PanelState {
                rect: egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(255.0, 900.0)),
            },
        )
    });
    h.run_steps(3);
    h.get_by_label("Perçages et pointages").click();
    h.run_steps(3);
    h.get_by_label("Ajouter un marquage…");
    let width = panel_rect(&h, "sheets").expect("panneau").width();
    assert!((width - 255.0).abs() < 1.0, "largeur imposée : {width}");
    let project = h.state().editor.project().clone();
    let undo = h.state().editor.doc.can_undo();

    h.state_mut().toggle_plan();
    h.run_steps(4);
    assert!(h.state().plan.is_some());
    assert!(
        h.query_by_label("FEUILLES DU PROJET").is_none(),
        "feuilles masquées en mode plan"
    );
    assert!(
        h.query_by_label("Ajouter un marquage…").is_none(),
        "fiche masquée en mode plan"
    );
    // Aucune donnée modifiée par l'aller.
    assert_eq!(*h.state().editor.project(), project);

    h.get_by_label("Retour à la feuille").click();
    h.run_steps(4);
    assert!(h.state().plan.is_none());
    h.get_by_label("FEUILLES DU PROJET");
    h.get_by_label("Ajouter un marquage…");
    let back = panel_rect(&h, "sheets").expect("panneau").width();
    assert!(
        (back - width).abs() < 1.0,
        "largeur rétablie : {back} / {width}"
    );
    assert_eq!(*h.state().editor.project(), project);
    assert_eq!(h.state().editor.doc.can_undo(), undo);
    assert!(!h.state().editor.doc.is_dirty());
}

// ----- CA3 : boîtes modales ----------------------------------------------------------------------

/// Formes peintes (récursif), pour lire tailles de police et couleurs réellement dessinées.
fn flat_shapes(shape: &egui::Shape, out: &mut Vec<egui::Shape>) {
    match shape {
        egui::Shape::Vec(v) => v.iter().for_each(|s| flat_shapes(s, out)),
        other => out.push(other.clone()),
    }
}

fn painted(h: &Harness<'_, App>) -> Vec<egui::Shape> {
    let mut out = Vec::new();
    for clipped in &h.output().shapes {
        flat_shapes(&clipped.shape, &mut out);
    }
    out
}

/// Taille et couleur du texte peint `text`.
fn text_style(h: &Harness<'_, App>, text: &str) -> Option<(f32, egui::Color32)> {
    painted(h).iter().find_map(|s| match s {
        egui::Shape::Text(t) if t.galley.text() == text => {
            let f = &t.galley.job.sections.first()?.format;
            Some((f.font_id.size, f.color))
        }
        _ => None,
    })
}

#[test]
fn les_boites_modales_ont_un_titre_fort_et_un_bouton_principal_accentue() {
    for (stage, title, primary) in [
        ("erreur", "PatternForge", "Fermer"),
        (
            "enregistrer-avant-quitter",
            "Modifications non enregistrées",
            "Enregistrer",
        ),
        (
            "import-feuille",
            "Importer une feuille",
            "(première feuille)",
        ),
        (
            "recuperation",
            "Récupération après un arrêt inattendu",
            "Récupérer",
        ),
    ] {
        let mut h = harness(Some(stage), [1280.0, 800.0]);
        h.run_steps(3);
        let accent = pf_ui::theme::tokens(&h.ctx).action_strong;
        // Import : le bouton principal est la première feuille de la liste.
        let first_sheet = h
            .state()
            .import
            .as_ref()
            .and_then(|d| d.source.sheets.first().map(|s| s.name.clone()));
        let primary = first_sheet.as_deref().unwrap_or(primary);
        let (size, color) = text_style(&h, title)
            .unwrap_or_else(|| panic!("{stage} : titre « {title} » non peint"));
        assert!(size >= 20.0, "{stage} : titre de {size} px");
        assert_eq!(
            color,
            crate::palette::pal().accent_text,
            "{stage} : titre accentué"
        );
        let rect = h.get_all_by_label(primary).next().expect("bouton").rect();
        // Bouton principal : fond plein de la couleur d'action du thème.
        let filled = painted(&h).iter().any(|s| match s {
            egui::Shape::Rect(r) => r.fill == accent && r.rect.intersects(rect),
            _ => false,
        });
        assert!(filled, "{stage} : « {primary} » non accentué");
    }
}

#[test]
fn entree_valide_le_bouton_principal_et_echap_annule_sauf_si_on_perdrait_des_donnees() {
    // Erreur : Entrée ferme.
    let mut h = harness(Some("erreur"), [1280.0, 800.0]);
    assert!(h.state().error_dialog.is_some());
    h.key_press(egui::Key::Enter);
    h.run_steps(3);
    assert!(h.state().error_dialog.is_none(), "Entrée ferme l'erreur");
    // Erreur : Échap ferme aussi.
    let mut h = harness(Some("erreur"), [1280.0, 800.0]);
    h.key_press(egui::Key::Escape);
    h.run_steps(3);
    assert!(h.state().error_dialog.is_none(), "Échap ferme l'erreur");

    // Import : Échap annule sans importer ; Entrée importe la première feuille.
    let mut h = harness(Some("import-feuille"), [1280.0, 800.0]);
    let sheets = h.state().editor.project().sheets.len();
    h.key_press(egui::Key::Escape);
    h.run_steps(3);
    assert!(h.state().import.is_none());
    assert_eq!(h.state().editor.project().sheets.len(), sheets);
    let mut h = harness(Some("import-feuille"), [1280.0, 800.0]);
    h.key_press(egui::Key::Enter);
    h.run_steps(3);
    assert!(h.state().import.is_none());
    assert!(
        h.state().editor.project().sheets.len() > sheets,
        "feuille importée"
    );

    // Enregistrer avant de quitter : Échap = annuler la fermeture, rien n'est perdu.
    let mut h = harness(Some("enregistrer-avant-quitter"), [1280.0, 800.0]);
    let project = h.state().editor.project().clone();
    assert!(h.state().pending.is_some());
    h.key_press(egui::Key::Escape);
    h.run_steps(3);
    assert!(h.state().pending.is_none(), "fermeture annulée");
    assert_eq!(*h.state().editor.project(), project);

    // Récupération : Échap ne fait rien (Ignorer supprimerait la copie) ; Entrée récupère.
    let mut h = harness(Some("recuperation"), [1280.0, 800.0]);
    h.key_press(egui::Key::Escape);
    h.run_steps(3);
    assert!(
        h.state().recovery_prompt.is_some(),
        "Échap ne supprime rien"
    );
    h.key_press(egui::Key::Enter);
    h.run_steps(3);
    assert!(h.state().recovery_prompt.is_none(), "Entrée récupère");
}

// ----- CA4 : valeurs longues ---------------------------------------------------------------------

use egui_kittest::kittest::NodeT;

/// Étiquettes (texte, rectangle) de tous les nœuds nommés de l'interface.
fn named_nodes(h: &Harness<'_, App>) -> Vec<(String, egui::Rect)> {
    h.root()
        .children_recursive()
        .filter_map(|n| {
            let label = n.accesskit_node().label()?;
            Some((label.to_string(), n.rect()))
        })
        .collect()
}

#[test]
fn une_valeur_qui_ne_tient_pas_passe_a_la_ligne_et_garde_son_info_bulle() {
    let long = "1 (0 obsolète(s)) et une suite de remarques bien trop longue pour la ligne";
    let mut h = Harness::builder()
        .with_size(egui::vec2(400.0, 300.0))
        .build_ui(move |ui| {
            ui.set_max_width(230.0);
            pf_ui::components::property_row(ui, "Surpiqûres liées", long);
            pf_ui::components::property_row(ui, "Longueur", "72,8 mm");
        });
    h.run_steps(3);
    let label = h.get_by_label("Surpiqûres liées").rect();
    let value = h.get_by_label(long).rect();
    // La valeur est sous son libellé et reste dans la largeur de 230 points.
    assert!(value.top() >= label.bottom() - 0.5, "{label:?} / {value:?}");
    assert!(value.right() <= 232.0, "déborde : {value:?}");
    // Valeur courte : sur la même ligne que son libellé, alignée à droite.
    let short_label = h.get_by_label("Longueur").rect();
    let short_value = h.get_by_label("72,8 mm").rect();
    assert!((short_label.center().y - short_value.center().y).abs() < 2.0);
    assert!(short_value.left() > short_label.right());
    // Info-bulle : la valeur complète est reprise au survol.
    h.get_by_label(long).hover();
    h.run_steps(8);
    assert!(
        h.get_all_by_label(long).count() >= 2,
        "info-bulle de la valeur complète"
    );
}

#[test]
fn les_valeurs_de_l_inspecteur_restent_dans_le_panneau_a_1280() {
    for lang in [pf_i18n::Lang::Fr, pf_i18n::Lang::En] {
        pf_i18n::set_lang(lang);
        for stage in [
            "feuille",
            "trait",
            "cote",
            "cran-coupe",
            "fiche-riche",
            "point-construit",
        ] {
            let h = harness(Some(stage), [1280.0, 800.0]);
            let inspector = panel_rect(&h, "inspector").expect("inspecteur");
            for (text, rect) in named_nodes(&h) {
                let inside = rect.left() >= inspector.left() && rect.top() < inspector.bottom();
                if inside && rect.width() > 0.0 && rect.top() >= inspector.top() {
                    assert!(
                        rect.right() <= inspector.right() + 1.0,
                        "{lang:?} {stage} : « {text} » {rect:?} dépasse l'inspecteur {inspector:?}"
                    );
                }
            }
        }
    }
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
}

// ----- CA5 : groupe « Boutons », fenêtre Déplier ----------------------------------------------

#[test]
fn les_parametres_d_un_groupe_de_boutons_sont_en_grille_etiquette_champ() {
    let mut h = harness(Some("fiche-riche"), [1600.0, 1000.0]);
    // Aucune section dépliée d'office : on déplie « Boutons et boutonnières ».
    assert!(h.query_by_label("Recul du bord (mm)").is_none());
    h.get_by_label("Boutons et boutonnières").click();
    h.run_steps(4);
    let labels: Vec<egui::Rect> = ["Nombre", "Recul du bord (mm)"]
        .iter()
        .map(|l| h.get_all_by_label(l).next().expect("étiquette").rect())
        .collect();
    // Même colonne d'étiquettes (bord gauche commun), champs alignés dans une seconde colonne.
    assert!(
        (labels[0].left() - labels[1].left()).abs() < 1.0,
        "{labels:?}"
    );
    let fields: Vec<egui::Rect> = h
        .get_all_by_role(egui::accesskit::Role::TextInput)
        .map(|n| n.rect())
        .filter(|r| {
            labels
                .iter()
                .any(|l| (l.center().y - r.center().y).abs() < 4.0 && r.left() > l.right())
        })
        .collect();
    assert_eq!(
        fields.len(),
        2,
        "un champ par étiquette, sur sa ligne : {fields:?}"
    );
    assert!(
        (fields[0].left() - fields[1].left()).abs() < 1.0,
        "colonne des champs : {fields:?}"
    );
}

#[test]
fn la_fenetre_deplier_a_un_nom_pleine_largeur_et_des_remarques_repliees() {
    let mut h = harness(Some("deplier"), [1600.0, 1000.0]);
    let window = area_rect(&h, ToolWin::Unfold).expect("fenêtre");
    let name = h
        .get_all_by_role(egui::accesskit::Role::TextInput)
        .map(|n| n.rect())
        .find(|r| window.contains_rect(*r))
        .expect("champ Nom");
    assert!(
        name.width() >= window.width() - 40.0,
        "nom pleine largeur : {name:?} dans {window:?}"
    );
    // Remarques du cœur : un seul en-tête « n remarque(s) », texte replié.
    let notes = {
        let dialog = h.state().unfold.clone().expect("dialogue");
        let preview = h
            .state()
            .editor
            .project()
            .unfold_preview(dialog.piece.unwrap(), dialog.fold.unwrap(), None)
            .expect("aperçu");
        preview.notes
    };
    assert!(!notes.is_empty(), "l'exemple doit produire des remarques");
    let title = format!("{} remarque(s)", notes.len());
    let header = h.get_by_label(&title);
    let first = format!("Remarque : {}", notes[0]);
    let _ = first;
    let before = area_rect(&h, ToolWin::Unfold).unwrap().height();
    header.click();
    h.run_steps(4);
    let after = area_rect(&h, ToolWin::Unfold).unwrap().height();
    assert!(
        after > before,
        "les remarques se déplient : {before} -> {after}"
    );
}

#[test]
fn les_notes_longues_des_fenetres_d_export_sont_repliees_et_depliables() {
    for (stage, key) in [
        ("dxf-export", "dxf.intro"),
        ("hpgl-export", "hpgl.intro"),
        ("dxf-import", "dxfi.intro"),
        ("dxf-construction", "dxfc.intro"),
    ] {
        let mut h = harness(Some(stage), [1280.0, 800.0]);
        // Début du paragraphe : présent seulement une fois « Notes » déplié.
        let text = pf_i18n::tr(key);
        let start: String = text.chars().take(40).collect();
        assert!(
            h.get_all_by_label("Notes").count() >= 1,
            "{stage} : « Notes » absent"
        );
        assert!(
            h.query_by_label_contains(&start).is_none(),
            "{stage} : paragraphe visible alors qu'il devrait être replié"
        );
        h.get_all_by_label("Notes").next().unwrap().click();
        h.run_steps(4);
        h.get_by_label_contains(&start);
    }
}

#[test]
fn une_note_courte_reste_affichee_telle_quelle() {
    let mut h = Harness::builder()
        .with_size(egui::vec2(600.0, 200.0))
        .build_ui(|ui| {
            crate::panels::note_paragraph(ui, "court", "Un texte bref.");
            crate::panels::note_paragraph(ui, "long", &"mot ".repeat(200));
        });
    h.run_steps(3);
    h.get_by_label("Un texte bref.");
    assert_eq!(h.get_all_by_label("Notes").count(), 1);
    assert!(h.query_by_label_contains("mot mot mot").is_none());
}

#[test]
fn les_cles_ajoutees_existent_dans_les_deux_catalogues_avec_les_memes_marques() {
    for key in ["export.notes", "unfoldpiece.notes"] {
        let fr = pf_i18n::Lang::Fr
            .catalog()
            .iter()
            .filter(|(k, _)| *k == key)
            .count();
        let en = pf_i18n::Lang::En
            .catalog()
            .iter()
            .filter(|(k, _)| *k == key)
            .count();
        assert_eq!((fr, en), (1, 1), "{key} : une seule entrée par catalogue");
    }
    pf_i18n::set_lang(pf_i18n::Lang::En);
    assert_eq!(pf_i18n::tr("export.notes"), "Notes");
    assert_eq!(
        pf_i18n::tr_args("unfoldpiece.notes", &[("n", "6")]),
        "6 note(s)"
    );
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
    assert_eq!(
        pf_i18n::tr_args("unfoldpiece.notes", &[("n", "6")]),
        "6 remarque(s)"
    );
}

#[test]
fn une_touche_pressee_l_image_d_ouverture_ne_valide_pas_la_boite() {
    let mut h = harness(None, [1280.0, 800.0]);
    h.key_press(egui::Key::Enter);
    h.state_mut().error_dialog = Some("x".into());
    h.run_steps(1);
    assert!(
        h.state().error_dialog.is_some(),
        "image d'ouverture ignorée"
    );
}
