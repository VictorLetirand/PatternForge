//! Tests de bout en bout de l'interface : la vraie boucle egui est exécutée
//! hors écran, pilotée par des événements souris et clavier simulés. Ils
//! prouvent le câblage canevas → éditeur → modèle, pas seulement la logique.

use crate::app::App;
use crate::editor::{Tool, ToolState};
use crate::Launch;
use eframe::egui::{self, Event, Key, Modifiers, PointerButton, Pos2};
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;
use pf_core::measure::{angle_between_segments, entity_length};
use pf_geometry::PointMm;

fn harness() -> Harness<'static, App> {
    let app = App::with_context(&egui::Context::default(), Launch::default());
    let mut h = Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .build_state(|ctx, app: &mut App| app.ui(ctx), app);
    h.run();
    h
}

/// Déplie les sections « Notes » (paragraphes d'aide des fenêtres d'export, repliés : PH-2b).
fn open_notes(h: &mut Harness<'_, App>) {
    let count = h.get_all_by_label("Notes").count();
    for i in 0..count {
        h.get_all_by_label("Notes").nth(i).expect("Notes").click();
        h.run_steps(3);
    }
}

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

/// Déplie une section repliable de la fiche de pièce ou de l'inspecteur (repliées par défaut
/// depuis PH-2, sauf celle liée à la sélection). `key` : clé i18n du titre ; à n'appeler que sur
/// une section repliée (un second clic la replierait).
fn open_fold(h: &mut Harness<'_, App>, key: &str) {
    h.get_by_label(&pf_i18n::tr(key)).click();
    h.run_steps(3);
}

/// Position écran d'un point du monde, d'après la vue courante.
fn screen(h: &Harness<'_, App>, world: PointMm) -> Pos2 {
    let app = h.state();
    let c = app.canvas_rect.center();
    let (x, y) = app.view.to_screen(world, (f64::from(c.x), f64::from(c.y)));
    Pos2::new(x as f32, y as f32)
}

fn move_to(h: &mut Harness<'_, App>, world: PointMm) {
    let pos = screen(h, world);
    h.input_mut().events.push(Event::PointerMoved(pos));
    h.step();
}

fn button(h: &mut Harness<'_, App>, world: PointMm, pressed: bool) {
    let pos = screen(h, world);
    let modifiers = h.input().modifiers;
    h.input_mut().events.push(Event::PointerButton {
        pos,
        button: PointerButton::Primary,
        pressed,
        modifiers,
    });
    h.step();
}

fn click(h: &mut Harness<'_, App>, world: PointMm) {
    move_to(h, world);
    button(h, world, true);
    button(h, world, false);
    h.step();
}

fn type_text(h: &mut Harness<'_, App>, text: &str) {
    h.input_mut().events.push(Event::Text(text.to_string()));
    h.step();
}

fn key(h: &mut Harness<'_, App>, key: Key) {
    h.key_press(key);
    h.step();
    h.step();
}

fn set_shift(h: &mut Harness<'_, App>, on: bool) {
    h.input_mut().modifiers = if on {
        Modifiers::SHIFT
    } else {
        Modifiers::NONE
    };
    h.step();
}

/// Trace AB = 120 mm à 0° puis BC ⟂ 80 mm, au clavier et à la souris.
fn draw_reference(h: &mut Harness<'_, App>) {
    key(h, Key::S);
    assert_eq!(h.state().editor.tool, Tool::Segment);
    click(h, pt(0.0, 0.0));
    assert!(matches!(
        h.state().editor.state,
        ToolState::SegmentFrom { .. }
    ));
    move_to(h, pt(55.0, 23.0)); // la souris traîne ailleurs : la saisie prime
    h.step();
    type_text(h, "120");
    key(h, Key::Tab);
    type_text(h, "0");
    assert_eq!(
        h.state().editor.length_text,
        "120",
        "la palette doit avoir le focus clavier"
    );
    assert_eq!(
        h.state().editor.angle_text,
        "0",
        "Tab doit passer au champ Angle"
    );
    key(h, Key::Enter);
    assert_eq!(
        h.state().editor.sheet().entities.len(),
        1,
        "{}",
        h.state().editor.status.text
    );

    key(h, Key::P);
    assert_eq!(h.state().editor.tool, Tool::Perpendicular);
    click(h, pt(60.0, 0.0)); // référence AB
    click(h, pt(120.0, 0.0)); // départ : accroche B
    assert!(matches!(h.state().editor.state, ToolState::PerpFrom { .. }));
    move_to(h, pt(128.0, 35.0)); // côté gauche
    h.step();
    type_text(h, "80");
    key(h, Key::Enter);
    assert_eq!(
        h.state().editor.sheet().entities.len(),
        2,
        "{}",
        h.state().editor.status.text
    );
}

fn check_reference(h: &Harness<'_, App>) {
    let s = h.state().editor.sheet();
    let (ab, bc) = (s.entities[0].id, s.entities[1].id);
    assert_eq!(entity_length(s, ab).unwrap().value, 120.0);
    assert_eq!(entity_length(s, bc).unwrap().value, 80.0);
    assert_eq!(angle_between_segments(s, ab, bc).unwrap().value, 90.0);
    assert_eq!(s.points.len(), 3);
}

#[test]
fn demo_at01_at06_au_clavier_et_a_la_souris() {
    let mut h = harness();
    draw_reference(&mut h);
    check_reference(&h);

    // Sélection AB puis Maj+clic BC : l'inspecteur affiche l'angle mesuré.
    key(&mut h, Key::V);
    click(&mut h, pt(60.0, 0.0));
    assert_eq!(h.state().editor.selection.entities.len(), 1);
    h.get_by_label("Segment AB");
    set_shift(&mut h, true);
    click(&mut h, pt(120.0, 40.0));
    set_shift(&mut h, false);
    assert_eq!(h.state().editor.selection.entities.len(), 2);
    h.run();
    h.get_by_label("90,0°");
    h.get_by_label("Angle entre les deux");
}

#[test]
fn at08_zoom_molette_sans_effet_sur_les_mesures() {
    let mut h = harness();
    draw_reference(&mut h);
    let before = h.state().editor.project().clone();
    for (delta, expect_bigger) in [(240.0, true), (-600.0, false)] {
        let zoom = h.state().view.zoom;
        move_to(&mut h, pt(60.0, 40.0));
        h.input_mut().events.push(Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, delta),
            modifiers: Modifiers::NONE,
        });
        h.run_steps(12); // le zoom à la molette est lissé sur quelques images
        let now = h.state().view.zoom;
        assert_eq!(now > zoom, expect_bigger, "zoom {zoom} → {now}");
        assert_eq!(
            h.state().editor.project(),
            &before,
            "le zoom ne modifie jamais le modèle"
        );
        check_reference(&h);
        // La visée suit le zoom : on sélectionne toujours AB en cliquant dessus.
        key(&mut h, Key::V);
        click(&mut h, pt(50.0, 100.0));
        click(&mut h, pt(60.0, 0.0));
        assert_eq!(h.state().editor.selection.entities.len(), 1);
    }
}

#[test]
fn at13_echap_pendant_la_saisie() {
    let mut h = harness();
    let before = h.state().editor.project().clone();
    key(&mut h, Key::S);
    click(&mut h, pt(10.0, 10.0));
    h.step();
    type_text(&mut h, "75");
    assert_eq!(h.state().editor.length_text, "75");
    key(&mut h, Key::Escape);
    assert_eq!(h.state().editor.state, ToolState::Idle);
    assert_eq!(h.state().editor.project(), &before);
    assert!(!h.state().editor.doc.is_dirty());
}

#[test]
fn at14_glisser_une_poignee_de_courbe() {
    let mut h = harness();
    key(&mut h, Key::C);
    click(&mut h, pt(0.0, 0.0));
    click(&mut h, pt(100.0, 0.0));
    let (curve, c1, before) = {
        let s = h.state().editor.sheet();
        let id = s.entities[0].id;
        (
            id,
            s.curve_geometry(id).unwrap().c1,
            entity_length(s, id).unwrap().value,
        )
    };
    assert_eq!(
        h.state().editor.tool,
        Tool::Select,
        "retour à la sélection après la courbe"
    );
    h.run();
    h.get_by_label("Longueur développée");

    // Glisser la poignée de départ de 30 mm vers le haut.
    move_to(&mut h, c1);
    button(&mut h, c1, true);
    for step in 1..=6 {
        move_to(&mut h, pt(c1.x, c1.y + 5.0 * f64::from(step)));
    }
    let target = pt(c1.x, c1.y + 30.0);
    button(&mut h, target, false);
    h.run();
    let s = h.state().editor.sheet();
    let moved = s.curve_geometry(curve).unwrap().c1;
    assert!(
        (moved.y - (c1.y + 30.0)).abs() <= 0.5,
        "poignée à {moved:?}"
    );
    let after = entity_length(s, curve).unwrap().value;
    assert!(after > before + 1.0, "longueur {before} → {after}");
    assert_eq!(
        pf_core::measure::curve_chord(s, curve).unwrap().value,
        100.0
    );

    // Ctrl+Z annule tout le glisser d'un coup.
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run();
    let s = h.state().editor.sheet();
    assert_eq!(entity_length(s, curve).unwrap().value, before);
}

#[test]
fn at12_fenetre_de_selection_et_suppression() {
    let mut h = harness();
    draw_reference(&mut h);
    key(&mut h, Key::V);
    click(&mut h, pt(50.0, 100.0));
    // Fenêtre autour de BC seulement.
    let (a, b) = (pt(110.0, -10.0), pt(130.0, 90.0));
    move_to(&mut h, a);
    button(&mut h, a, true);
    move_to(&mut h, pt(120.0, 40.0));
    move_to(&mut h, b);
    button(&mut h, b, false);
    h.run();
    let sel = h.state().editor.selection.clone();
    assert_eq!((sel.entities.len(), sel.points.len()), (1, 2), "{sel:?}");

    // On ne garde que le segment, puis Suppr.
    let bc = sel.entities[0];
    h.state_mut().editor.selection.points.clear();
    key(&mut h, Key::Delete);
    let s = h.state().editor.sheet();
    assert!(s.entity(bc).is_err(), "{}", h.state().editor.status.text);
    assert_eq!(s.entities.len(), 1);
    assert_eq!(entity_length(s, s.entities[0].id).unwrap().value, 120.0);
}

#[test]
fn feuilles_creation_depuis_le_panneau() {
    let mut h = harness();
    for _ in 0..7 {
        h.get_by_label("+ Nouvelle feuille").click();
        h.run();
        key(&mut h, Key::Enter); // valide le nom proposé
    }
    assert_eq!(h.state().editor.project().sheets.len(), 8);
    h.get_by_label("Feuille 8");
    // Changer de feuille par un clic dans la liste.
    h.get_by_label("Feuille 3").click();
    h.run();
    assert_eq!(h.state().editor.sheet().name, "Feuille 3");
}

#[test]
fn feuille_dupliquee_et_importee_depuis_un_projet() {
    let mut h = harness();
    draw_reference(&mut h);
    h.get_by_label("Dupliquer").click();
    h.run();
    let p = h.state().editor.project();
    assert_eq!(p.sheets.len(), 2);
    assert_eq!(p.sheets[1].name, "Feuille 1 (copie)");
    assert_eq!(h.state().editor.sheet().name, "Feuille 1 (copie)");
    assert_eq!(h.state().editor.sheet().entities.len(), 2);
    // Une seule étape d'historique : Ctrl+Z retire la copie.
    h.state_mut().editor.undo();
    assert_eq!(h.state().editor.project().sheets.len(), 1);

    // Import depuis un projet v1 de la suite de tests du modèle.
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../crates/pf-core/tests/fixtures/v1-exercice.pforge.json");
    h.state_mut().begin_import(&path);
    h.run();
    let name = h.state().import.as_ref().unwrap().source.sheets[0]
        .name
        .clone();
    h.get_by_label(&name).click();
    h.run();
    let p = h.state().editor.project();
    assert!(h.state().import.is_none());
    assert_eq!(p.sheets.len(), 2);
    assert_eq!(p.sheets[1].name, name);
    assert_eq!(h.state().editor.sheet().id, p.sheets[1].id);
    p.validate().unwrap();
}

#[test]
fn accrochage_milieu_et_intersection_par_l_interface() {
    let mut h = harness();
    draw_reference(&mut h); // AB (0,0)→(120,0), BC (120,0)→(120,80)
    let ab = h.state().editor.sheet().entities[0].id;

    // Outil Point (touche O) : clic près du milieu de AB.
    key(&mut h, Key::O);
    assert_eq!(h.state().editor.tool, Tool::Point);
    move_to(&mut h, pt(60.6, 0.8));
    let snap = h
        .state()
        .editor
        .hover_snap()
        .expect("un accrochage doit être signalé");
    assert_eq!(snap.kind, crate::editor::SnapKind::Midpoint);
    click(&mut h, pt(60.6, 0.8));
    let m = h.state().editor.selection.points[0];
    let s = h.state().editor.sheet();
    assert_eq!(s.position(m).unwrap(), pt(60.0, 0.0));
    assert_eq!(
        s.point(m).unwrap().definition,
        pf_core::PointDefinition::OnSegmentRatio {
            segment: ab,
            ratio: 0.5
        }
    );
    h.run();
    h.get_by_label("CONSTRUCTION (PROPORTION DU SEGMENT)");

    // Segment partant de ce milieu, tracé à la souris jusqu'à croiser BC : son
    // extrémité s'accroche sur BC et reste liée à BC.
    key(&mut h, Key::S);
    click(&mut h, pt(60.0, 0.0));
    click(&mut h, pt(120.3, 40.2));
    let s = h.state().editor.sheet();
    let seg = s.entities.last().unwrap().id;
    let (_, to) = s.segment_point_ids(seg).unwrap();
    assert_eq!(s.position(to).unwrap(), pt(120.0, 40.0));
    assert!(!s.point(to).unwrap().definition.is_free());
}

#[test]
fn inspecteur_affiche_les_contours_fermes() {
    let launch = Launch {
        demo: true,
        stage: Some("accrochage".into()), // aucune sélection : inspecteur de feuille
        ..Launch::default()
    };
    let app = App::with_context(&egui::Context::default(), launch);
    let mut h = Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .build_state(|ctx, app: &mut App| app.ui(ctx), app);
    h.run_steps(4);
    h.get_by_label("CONTOURS FERMÉS (LECTURE SEULE)");
    h.get_by_label("Contour 1 — 3 tracés, Construction");
    h.get_by_label("Périmètre");
    h.get_by_label("Aire");
}

#[test]
fn rectangle_au_clavier_et_transformation_par_la_fenetre() {
    let mut h = harness();
    key(&mut h, Key::R);
    assert_eq!(h.state().editor.tool, Tool::Rectangle);
    click(&mut h, pt(0.0, 0.0));
    h.step();
    type_text(&mut h, "100");
    key(&mut h, Key::Tab);
    type_text(&mut h, "60");
    key(&mut h, Key::Enter);
    assert_eq!(
        h.state().editor.sheet().entities.len(),
        4,
        "{}",
        h.state().editor.status.text
    );
    assert_eq!(h.state().editor.selection.entities.len(), 4);

    // Menu Édition › Dupliquer : la fenêtre s'ouvre centrée sur la sélection.
    h.state_mut().open_transform(true);
    h.run_steps(3);
    h.get_by_label("Transformer la sélection");
    let (cx, cy) = {
        let d = h.state().transform.as_ref().expect("fenêtre ouverte");
        (d.cx, d.cy)
    };
    assert_eq!((cx, cy), (50.0, 30.0));
    // Réglage par programme du déplacement, puis clic sur « Appliquer ».
    h.state_mut().transform.as_mut().unwrap().dx = 150.0;
    h.state_mut().transform.as_mut().unwrap().dy = 0.0;
    h.get_by_label("Appliquer").click();
    h.run_steps(3);
    let s = h.state().editor.sheet();
    assert_eq!(s.entities.len(), 8, "{}", h.state().editor.status.text);
    assert!(
        h.state().transform.is_none(),
        "la fenêtre se ferme après succès"
    );
    assert!(s
        .points
        .iter()
        .any(|p| s.position(p.id).unwrap() == pt(250.0, 60.0)));
    assert_eq!(pf_core::contour::closed_contours(s).len(), 2);
}

#[test]
fn cran_au_clavier_et_inspecteur() {
    let mut h = harness();
    draw_reference(&mut h);
    key(&mut h, Key::N);
    assert_eq!(h.state().editor.tool, Tool::Notch);
    click(&mut h, pt(70.0, 0.4));
    assert!(matches!(h.state().editor.state, ToolState::NotchOn { .. }));
    move_to(&mut h, pt(70.0, -25.0));
    type_text(&mut h, "45");
    key(&mut h, Key::Enter);
    assert_eq!(
        h.state().editor.sheet().notches.len(),
        1,
        "{}",
        h.state().editor.status.text
    );

    // La sélection du cran affiche son inspecteur ; le champ de distance est éditable.
    key(&mut h, Key::V);
    h.run_steps(3);
    h.get_by_label("POSITION ET FORME");
    h.state_mut().editor.selection.clear();
    click(&mut h, pt(45.0, -5.0));
    assert_eq!(h.state().editor.selection.notches.len(), 1);
    h.run_steps(3);
    h.get_by_label("Posé sur");
    key(&mut h, Key::Delete);
    assert!(h.state().editor.sheet().notches.is_empty());
}

#[test]
fn cotation_au_clavier_selection_et_suppression() {
    let mut h = harness();
    draw_reference(&mut h);
    key(&mut h, Key::Q);
    assert_eq!(h.state().editor.tool, Tool::Dimension);
    click(&mut h, pt(0.0, 0.0));
    assert!(matches!(
        h.state().editor.state,
        ToolState::DimensionFrom { .. }
    ));
    click(&mut h, pt(120.0, 0.0));
    let s = h.state().editor.sheet();
    assert_eq!(s.dimensions.len(), 1, "{}", h.state().editor.status.text);
    let d = s.dimensions[0].id;
    assert_eq!(s.dimension_geometry(d).unwrap().value_mm, 120.0);
    assert_eq!(s.dimensions[0].offset_mm.abs(), 10.0);

    // Sélection sur la ligne de cote, inspecteur, puis Suppr.
    key(&mut h, Key::V);
    h.state_mut().editor.selection.clear();
    let y = h
        .state()
        .editor
        .sheet()
        .dimension_geometry(d)
        .unwrap()
        .from_offset
        .y;
    click(&mut h, pt(60.0, y));
    assert_eq!(h.state().editor.selection.dimensions, vec![d]);
    h.run_steps(3);
    h.get_by_label("Valeur mesurée");
    key(&mut h, Key::Delete);
    assert!(h.state().editor.sheet().dimensions.is_empty());
    assert_eq!(h.state().editor.sheet().points.len(), 3);
}

#[test]
fn droit_fil_au_clavier() {
    let mut h = harness();
    key(&mut h, Key::G);
    assert_eq!(h.state().editor.tool, Tool::Grainline);
    click(&mut h, pt(0.0, 0.0));
    h.step();
    type_text(&mut h, "90");
    key(&mut h, Key::Tab);
    type_text(&mut h, "90");
    key(&mut h, Key::Enter);
    assert_eq!(
        h.state().editor.sheet().grainline_count(),
        1,
        "{}",
        h.state().editor.status.text
    );
    key(&mut h, Key::Escape);
    h.state_mut().editor.selection.clear();
    h.run_steps(3);
    h.get_by_label("1 tracé(s)");
}

#[test]
fn raccourci_d_outil_personnalise_via_l_aide() {
    let mut h = harness();
    key(&mut h, Key::F1);
    assert!(h.state().show_help);
    h.state_mut().shortcut_capture = Some(Tool::Notch);
    key(&mut h, Key::W);
    assert_eq!(h.state().shortcut_capture, None);
    assert_eq!(crate::shortcuts::current().letter(Tool::Notch), 'W');
    // L'ancienne lettre n'active plus rien, la nouvelle active l'outil.
    h.state_mut().show_help = false;
    h.run_steps(2);
    key(&mut h, Key::N);
    assert_ne!(h.state().editor.tool, Tool::Notch);
    key(&mut h, Key::W);
    assert_eq!(h.state().editor.tool, Tool::Notch);
    // Lettre déjà prise : refus avec message, table inchangée.
    h.state_mut().show_help = true;
    h.state_mut().shortcut_capture = Some(Tool::Segment);
    key(&mut h, Key::W);
    assert!(h.state().shortcut_error.is_some());
    assert_eq!(crate::shortcuts::current().letter(Tool::Segment), 'S');
    h.state_mut().reset_shortcuts();
    assert_eq!(crate::shortcuts::current().letter(Tool::Notch), 'N');
}

#[test]
fn theme_sombre_applique_aux_jetons_et_a_la_palette() {
    let mut h = harness();
    h.state_mut().set_dark(true);
    h.run_steps(3);
    assert!(crate::palette::is_dark());
    let t = pf_ui::theme::tokens(&h.ctx);
    assert!(t.dark);
    assert_eq!(t, pf_ui::tokens::Tokens::precision_dark());
    h.state_mut().set_dark(false);
    h.run_steps(3);
    assert!(!crate::palette::is_dark());
}

#[test]
fn calques_masquer_un_calque_retire_ses_objets_de_la_selection_souris() {
    let mut h = harness();
    draw_reference(&mut h);
    key(&mut h, Key::V);
    // Sélectionne AB, crée un calque (actif) et y range la sélection.
    click(&mut h, pt(60.0, 0.0));
    assert_eq!(h.state().editor.selection.entities.len(), 1);
    let ab = h.state().editor.selection.entities[0];
    h.get_by_label("+ Calque").click();
    h.run();
    let layer = h.state().editor.sheet().layers[0].id;
    assert_eq!(h.state().editor.active_layer, Some(layer));
    h.get_by_label("Déplacer la sélection ici").click();
    h.run();
    assert_eq!(
        h.state().editor.sheet().entity(ab).unwrap().layer,
        Some(layer)
    );

    // Visible : un clic sur AB le sélectionne.
    h.state_mut().editor.selection.clear();
    click(&mut h, pt(60.0, 0.0));
    assert_eq!(h.state().editor.selection.entities, vec![ab]);
    // Masquer par le panneau : sortie de la sélection, et plus de sélection à la souris.
    h.get_by_label("Voir").click();
    h.run();
    assert!(!h.state().editor.sheet().layers[0].visible);
    assert!(
        h.state().editor.selection.entities.is_empty(),
        "un objet masqué quitte la sélection"
    );
    click(&mut h, pt(50.0, 100.0));
    click(&mut h, pt(60.0, 0.0));
    assert!(h.state().editor.selection.entities.is_empty());
    // BC (calque par défaut) reste sélectionnable.
    click(&mut h, pt(120.0, 40.0));
    assert_eq!(h.state().editor.selection.entities.len(), 1);
    // Réafficher : AB redevient sélectionnable.
    h.get_by_label("Voir").click();
    h.run();
    click(&mut h, pt(50.0, 100.0));
    click(&mut h, pt(60.0, 0.0));
    assert_eq!(h.state().editor.selection.entities, vec![ab]);
}

#[test]
fn calques_verrouiller_par_le_panneau_refuse_la_suppression() {
    let mut h = harness();
    draw_reference(&mut h);
    key(&mut h, Key::V);
    click(&mut h, pt(60.0, 0.0));
    let ab = h.state().editor.selection.entities[0];
    h.get_by_label("+ Calque").click();
    h.run();
    h.get_by_label("Déplacer la sélection ici").click();
    h.run();
    h.get_by_label("Verr.").click();
    h.run();
    assert!(h.state().editor.sheet().layers[0].locked);
    key(&mut h, Key::Delete);
    assert!(h.state().editor.sheet().entity(ab).is_ok());
    assert!(h.state().editor.status.error);
}

#[test]
fn groupes_ctrl_g_alt_clic_et_transformation_du_groupe_par_la_fenetre() {
    let mut h = harness();
    draw_reference(&mut h);
    key(&mut h, Key::V);
    // Sélectionne AB et BC (Maj+clic), puis Ctrl+G.
    click(&mut h, pt(60.0, 0.0));
    set_shift(&mut h, true);
    click(&mut h, pt(120.0, 40.0));
    set_shift(&mut h, false);
    assert_eq!(h.state().editor.selection.entities.len(), 2);
    h.key_press_modifiers(Modifiers::COMMAND, Key::G);
    h.run_steps(3);
    assert_eq!(h.state().editor.sheet().groups.len(), 1);
    let (ab, bc) = {
        let g = &h.state().editor.sheet().groups[0];
        (g.members[0], g.members[1])
    };

    // Clic sur un membre : tout le groupe. Alt+clic : le membre seul.
    click(&mut h, pt(30.0, 50.0));
    assert!(h.state().editor.selection.is_empty());
    click(&mut h, pt(60.0, 0.0));
    assert_eq!(h.state().editor.selection.entities.len(), 2);
    click(&mut h, pt(30.0, 50.0));
    h.input_mut().modifiers = Modifiers::ALT;
    h.step();
    click(&mut h, pt(60.0, 0.0));
    h.input_mut().modifiers = Modifiers::NONE;
    h.step();
    assert_eq!(h.state().editor.selection.entities, vec![ab]);

    // Transformation du groupe par la fenêtre : une seule étape annulable.
    click(&mut h, pt(30.0, 50.0));
    click(&mut h, pt(60.0, 0.0));
    let before = h.state().editor.project().clone();
    h.state_mut().open_transform(false);
    h.run_steps(3);
    h.state_mut().transform.as_mut().unwrap().dx = 20.0;
    h.state_mut().transform.as_mut().unwrap().dy = 0.0;
    h.get_by_label("Appliquer").click();
    h.run_steps(3);
    let s = h.state().editor.sheet();
    assert_eq!(s.segment_ends(ab).unwrap().0, pt(20.0, 0.0));
    assert_eq!(s.segment_ends(bc).unwrap().1, pt(140.0, 80.0));
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run_steps(3);
    assert_eq!(*h.state().editor.project(), before);

    // Ctrl+Maj+G dégroupe.
    h.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::G);
    h.run_steps(3);
    assert!(h.state().editor.sheet().groups.is_empty());
}

#[test]
fn recherche_ctrl_f_saisie_clic_selectionne_et_cadre_l_objet() {
    let mut h = harness();
    draw_reference(&mut h);
    key(&mut h, Key::V);
    h.state_mut().editor.selection.clear();
    // La vue est volontairement décadrée : le clic sur un résultat doit la ramener.
    h.state_mut().view.center = pt(5000.0, 5000.0);
    h.run_steps(2);

    h.key_press_modifiers(Modifiers::COMMAND, Key::F);
    h.run_steps(3);
    assert!(h.state().search.is_some(), "Ctrl+F ouvre la recherche");
    type_text(&mut h, "segment bc");
    h.run_steps(2);
    assert_eq!(h.state().search.as_ref().unwrap().query, "segment bc");

    h.get_by_label("Segment BC · Construction · Calque par défaut")
        .click();
    h.run_steps(3);

    let bc = h.state().editor.sheet().entities[1].id;
    assert_eq!(h.state().editor.selection.entities, vec![bc]);
    let (a, b) = h.state().editor.sheet().segment_ends(bc).unwrap();
    let rect = h.state().canvas_rect;
    for p in [a, b] {
        let s = screen(&h, p);
        assert!(rect.contains(s), "{p:?} hors de la vue : {s:?}");
    }
}

#[test]
fn presse_papiers_ctrl_c_puis_ctrl_v_colle_decale_et_selectionne() {
    let mut h = harness();
    draw_reference(&mut h);
    key(&mut h, Key::V);
    let sheet = h.state().editor.sheet().clone();
    let (points, entities) = (sheet.points.len(), sheet.entities.len());
    h.state_mut().editor.selection.entities = sheet.entities.iter().map(|e| e.id).collect();
    h.run_steps(2);

    h.key_press_modifiers(Modifiers::COMMAND, Key::C);
    h.run_steps(3);
    assert!(
        h.state().editor.clipboard.is_some(),
        "Ctrl+C remplit le presse-papiers"
    );
    assert_eq!(
        h.state().editor.sheet().entities.len(),
        entities,
        "copier ne modifie rien"
    );

    h.key_press_modifiers(Modifiers::COMMAND, Key::V);
    h.run_steps(3);
    let after = h.state().editor.sheet().clone();
    assert_eq!(after.entities.len(), entities * 2);
    assert_eq!(after.points.len(), points * 2);
    assert_eq!(h.state().editor.selection.entities.len(), entities);
    assert!(h
        .state()
        .editor
        .selection
        .entities
        .iter()
        .all(|e| !sheet.entities.iter().any(|o| o.id == *e)));
    h.state().editor.project().validate().unwrap();

    // Une seule étape d'historique pour le collage.
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run_steps(3);
    assert_eq!(h.state().editor.sheet().entities.len(), entities);
}

#[test]
fn us021_cercle_trace_par_saisie_du_rayon() {
    let mut h = harness();
    key(&mut h, Key::E);
    assert_eq!(h.state().editor.tool, Tool::Circle);
    click(&mut h, pt(20.0, 10.0));
    assert!(matches!(
        h.state().editor.state,
        ToolState::CircleCenter { .. }
    ));
    move_to(&mut h, pt(70.0, 60.0)); // la souris traîne ailleurs : la saisie prime
    type_text(&mut h, "35");
    assert_eq!(
        h.state().editor.length_text,
        "35",
        "la palette doit avoir le focus clavier"
    );
    key(&mut h, Key::Enter);
    let s = h.state().editor.sheet();
    assert_eq!(s.entities.len(), 1, "{}", h.state().editor.status.text);
    let c = s.circle_geometry(s.entities[0].id).unwrap();
    assert_eq!(
        (c.circle.center, c.circle.radius_mm),
        (pt(20.0, 10.0), 35.0)
    );
    let len = entity_length(s, s.entities[0].id).unwrap().value;
    assert!((len - 70.0 * std::f64::consts::PI).abs() < 1e-9);
    // L'inspecteur annonce le rayon comme paramètre et la circonférence comme mesure.
    h.run();
    h.get_by_label("Paramètre de création");
    h.get_by_label("Circonférence");
    // Annuler d'un seul coup : plus de cercle.
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run_steps(3);
    assert!(h.state().editor.sheet().entities.is_empty());
}

#[test]
fn us022_courbe_lisse_clics_entree_inspecteur_et_annulation() {
    let mut h = harness();
    key(&mut h, Key::B);
    assert_eq!(h.state().editor.tool, Tool::Spline);
    // Échap annule la pose : rien n'est créé.
    click(&mut h, pt(0.0, 0.0));
    click(&mut h, pt(40.0, 30.0));
    assert_eq!(h.state().editor.state, ToolState::SplineNext);
    key(&mut h, Key::Escape);
    assert!(h.state().editor.sheet().entities.is_empty());
    assert!(h.state().editor.spline_points.is_empty());
    assert_eq!(h.state().editor.tool, Tool::Spline);
    // Trois clics puis Entrée : une courbe à trois points de passage.
    for p in [pt(0.0, 0.0), pt(40.0, 30.0), pt(90.0, -10.0)] {
        click(&mut h, p);
    }
    key(&mut h, Key::Enter);
    let s = h.state().editor.sheet();
    assert_eq!(s.entities.len(), 1, "{}", h.state().editor.status.text);
    let id = s.entities[0].id;
    let nodes = s.spline_nodes(id).unwrap();
    assert_eq!(nodes.len(), 3);
    assert_eq!(nodes[1].position, pt(40.0, 30.0));
    assert_eq!(h.state().editor.tool, Tool::Select);
    assert_eq!(h.state().editor.selection.entities, vec![id]);
    // L'inspecteur annonce longueur totale et tronçons (mesures observées).
    h.run();
    h.get_by_label("Longueur développée");
    h.get_by_label("Tronçon A -> B");
    h.get_by_label("Tronçon B -> C");
    // Point anguleux : la case de l'inspecteur casse la tangente du point B.
    h.get_by_label("Anguleux").click();
    h.run_steps(3);
    let corner = h.state().editor.sheet().spline_nodes(id).unwrap()[1].corner;
    assert!(corner, "{}", h.state().editor.status.text);
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run_steps(3);
    assert!(!h.state().editor.sheet().spline_nodes(id).unwrap()[1].corner);
    // Retirer un point : deux points restent, plus aucun bouton « Retirer ».
    h.get_all_by_label("Retirer").next().unwrap().click();
    h.run_steps(3);
    assert_eq!(h.state().editor.sheet().spline_nodes(id).unwrap().len(), 2);
    assert!(h.query_all_by_label("Retirer").next().is_none());
    h.state().editor.project().validate().unwrap();
}

fn two_segments_from_origin(h: &mut Harness<'_, App>) {
    let ed = &mut h.state_mut().editor;
    let sheet = ed.sheet;
    for to in [pt(100.0, 0.0), pt(0.0, 100.0)] {
        ed.exec(pf_core::Command::AddSegment {
            sheet,
            from: pf_core::PointRef::New(pt(0.0, 0.0)),
            to: pf_core::PointRef::New(to),
        })
        .unwrap();
    }
    h.run_steps(2);
}

#[test]
fn us023_conge_par_clics_saisie_du_rayon_refus_et_annulation() {
    let mut h = harness();
    two_segments_from_origin(&mut h);
    key(&mut h, Key::F);
    assert_eq!(h.state().editor.tool, Tool::Fillet);
    click(&mut h, pt(50.0, 0.0));
    assert!(matches!(
        h.state().editor.state,
        ToolState::JoinFirst { .. }
    ));
    click(&mut h, pt(0.0, 50.0));
    assert!(matches!(
        h.state().editor.state,
        ToolState::JoinReady { .. }
    ));
    // Entrée sans rayon : rien n'est créé, la raison est affichée.
    key(&mut h, Key::Enter);
    assert_eq!(h.state().editor.sheet().entities.len(), 2);
    assert!(h.state().editor.status.error);
    // Rayon trop grand : refusé, projet inchangé.
    type_text(&mut h, "500");
    key(&mut h, Key::Enter);
    assert_eq!(h.state().editor.sheet().entities.len(), 2);
    assert!(h.state().editor.status.error);
    // Rayon valide : un arc tangent est créé, les segments sont raccourcis.
    h.state_mut().editor.length_text = "10".into();
    key(&mut h, Key::Enter);
    let s = h.state().editor.sheet();
    assert_eq!(s.entities.len(), 3, "{}", h.state().editor.status.text);
    let arc = s.entities[2].id;
    assert!((s.arc_geometry(arc).unwrap().radius_mm - 10.0).abs() < 1e-9);
    assert!((entity_length(s, s.entities[0].id).unwrap().value - 90.0).abs() < 1e-9);
    assert_eq!(h.state().editor.selection.entities, vec![arc]);
    h.state().editor.project().validate().unwrap();
    // Une seule étape d'historique.
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run_steps(3);
    let s = h.state().editor.sheet();
    assert_eq!(s.entities.len(), 2);
    assert!((entity_length(s, s.entities[0].id).unwrap().value - 100.0).abs() < 1e-9);
}

#[test]
fn us023_chanfrein_et_tangente_depuis_un_point() {
    let mut h = harness();
    two_segments_from_origin(&mut h);
    key(&mut h, Key::H);
    assert_eq!(h.state().editor.tool, Tool::Chamfer);
    click(&mut h, pt(50.0, 0.0));
    click(&mut h, pt(0.0, 50.0));
    type_text(&mut h, "10");
    key(&mut h, Key::Enter);
    let s = h.state().editor.sheet();
    assert_eq!(s.entities.len(), 3, "{}", h.state().editor.status.text);
    let len = entity_length(s, s.entities[2].id).unwrap().value;
    assert!((len - 10.0 * 2.0_f64.sqrt()).abs() < 1e-9, "{len}");

    // Tangente depuis un point libre à un cercle ; le côté du clic choisit la solution.
    let sheet = h.state().editor.sheet;
    let circle = h
        .state_mut()
        .editor
        .exec(pf_core::Command::AddCircle {
            sheet,
            center: pf_core::PointRef::New(pt(80.0, 80.0)),
            radius_mm: 25.0,
        })
        .unwrap()
        .entities[0];
    h.run_steps(2);
    key(&mut h, Key::T);
    assert_eq!(h.state().editor.tool, Tool::Tangent);
    click(&mut h, pt(140.0, 80.0));
    assert!(matches!(
        h.state().editor.state,
        ToolState::TangentFrom { .. }
    ));
    click(&mut h, pt(80.0 + 25.0 * 0.5, 80.0 + 25.0 * 0.866));
    let s = h.state().editor.sheet();
    assert_eq!(s.entities.len(), 5, "{}", h.state().editor.status.text);
    let (a, b) = s.segment_ends(s.entities[4].id).unwrap();
    assert!(b.y > 80.0 && (pf_geometry::distance_mm(b, pt(80.0, 80.0)) - 25.0).abs() < 1e-9);
    assert!(((b - pt(80.0, 80.0)).dot(b - a)).abs() < 1e-9);
    assert!(s.entity(circle).is_ok());
}

#[test]
fn us024_point_d_intersection_courbe_segment_accrochage_suivi_et_annulation() {
    let mut h = harness();
    let (curve, line);
    {
        let ed = &mut h.state_mut().editor;
        ed.snap_to_paths = true;
        ed.grid_snap_mm = None;
        let sheet = ed.sheet;
        // Arche : y(t) = 300 t (1 − t), sommet (50 ; 75).
        curve = ed
            .exec(pf_core::Command::AddCurve {
                sheet,
                start: pf_core::PointRef::New(pt(0.0, 0.0)),
                c1: pt(0.0, 100.0),
                c2: pt(100.0, 100.0),
                end: pf_core::PointRef::New(pt(100.0, 0.0)),
            })
            .unwrap()
            .entities[0];
        line = ed
            .exec(pf_core::Command::AddSegment {
                sheet,
                from: pf_core::PointRef::New(pt(-50.0, 30.0)),
                to: pf_core::PointRef::New(pt(150.0, 30.0)),
            })
            .unwrap()
            .entities[0];
    }
    h.run_steps(2);
    // Deux solutions : le clic choisit celle de droite.
    let t = (1.0 + (1.0_f64 - 4.0 * 30.0 / 300.0).sqrt()) / 2.0;
    let target = pt(300.0 * (1.0 - t) * t * t + 100.0 * t * t * t, 30.0);
    key(&mut h, Key::O);
    assert_eq!(h.state().editor.tool, Tool::Point);
    click(&mut h, target);
    let ed = &h.state().editor;
    let id = *ed.selection.points.first().expect(&ed.status.text);
    let s = ed.sheet();
    assert!(matches!(
        s.point(id).unwrap().definition,
        pf_core::PointDefinition::TraceIntersection { .. }
    ));
    assert!(pf_geometry::distance_mm(s.position(id).unwrap(), target) < 1e-9);
    // L'inspecteur annonce un point construit qui suit les tracés.
    h.run_steps(2);
    h.get_by_label("Intersection de deux tracés");
    // Le point suit : on déplace la droite, il est recalculé ; si l'intersection
    // disparaît, la commande est refusée et le message est affiché.
    let (a, _) = h.state().editor.sheet().segment_ends(line).unwrap();
    let start = h.state().editor.sheet().segment_point_ids(line).unwrap().0;
    let before = h.state().editor.sheet().position(id).unwrap();
    let sheet0 = h.state().editor.sheet;
    assert!(h
        .state_mut()
        .editor
        .exec(pf_core::Command::MovePoint {
            sheet: sheet0,
            point: start,
            to: pt(a.x, 40.0),
        })
        .is_some());
    let moved = h.state().editor.sheet().position(id).unwrap();
    assert!(moved != before && moved.y > 30.0 && moved.y < 40.0 + 1e-9);
    let ends = h.state().editor.sheet().segment_point_ids(line).unwrap();
    let sheet = h.state().editor.sheet;
    assert!(h
        .state_mut()
        .editor
        .exec(pf_core::Command::MovePoint {
            sheet,
            point: ends.1,
            to: pt(150.0, 400.0),
        })
        .is_none());
    assert!(h.state().editor.status.error);
    assert_eq!(h.state().editor.sheet().position(id).unwrap(), moved);
    assert!(h.state().editor.sheet().entity(curve).is_ok());
    h.state().editor.project().validate().unwrap();
    // Annuler : d'abord le déplacement (le point revient), puis la création du point.
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run_steps(3);
    assert_eq!(h.state().editor.sheet().position(id).unwrap(), before);
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run_steps(3);
    assert!(h.state().editor.sheet().point(id).is_err());
}

#[test]
fn us025_points_relatifs_milieu_et_compas_par_la_souris_et_la_palette() {
    let mut h = harness();
    let (a, b);
    {
        let ed = &mut h.state_mut().editor;
        ed.snap_to_paths = true;
        ed.grid_snap_mm = None;
        let sheet = ed.sheet;
        for (x, y) in [(0.0, 0.0), (60.0, 0.0)] {
            ed.exec(pf_core::Command::AddPoint {
                sheet,
                at: pt(x, y),
            })
            .unwrap();
        }
        let s = ed.sheet();
        a = s.points[0].id;
        b = s.points[1].id;
    }
    h.run_steps(2);
    key(&mut h, Key::O);
    assert_eq!(h.state().editor.tool, Tool::Point);
    // Sous-mode Milieu : deux clics sur des points existants, point maintenu.
    h.get_by_label("Milieu").click();
    h.run_steps(2);
    click(&mut h, pt(0.0, 0.0));
    click(&mut h, pt(60.0, 0.0));
    let ed = &h.state().editor;
    let mid = *ed.selection.points.first().expect(&ed.status.text);
    assert!(matches!(
        ed.sheet().point(mid).unwrap().definition,
        pf_core::PointDefinition::Midpoint { .. }
    ));
    assert_eq!(ed.sheet().position(mid).unwrap(), pt(30.0, 0.0));
    // Sous-mode Compas : centres A et B, rayons saisis, clic près de la solution du haut.
    h.get_by_label("Compas").click();
    h.run_steps(2);
    click(&mut h, pt(0.0, 0.0));
    click(&mut h, pt(60.0, 0.0));
    assert_eq!(h.state().editor.state, ToolState::RelativePick);
    h.run_steps(2);
    type_text(&mut h, "50");
    key(&mut h, Key::Tab);
    type_text(&mut h, "50");
    click(&mut h, pt(30.0, 35.0));
    let ed = &h.state().editor;
    let id = *ed.selection.points.first().expect(&ed.status.text);
    let s = ed.sheet();
    assert!(matches!(
        s.point(id).unwrap().definition,
        pf_core::PointDefinition::Compass { .. }
    ));
    assert!(pf_geometry::distance_mm(s.position(id).unwrap(), pt(30.0, 40.0)) < 1e-9);
    // L'inspecteur annonce un point maintenu ; déplacer un parent le recalcule.
    h.run_steps(2);
    h.get_by_label("Point d'intersection de deux arcs de compas");
    let sheet = h.state().editor.sheet;
    assert!(h
        .state_mut()
        .editor
        .exec(pf_core::Command::MovePoint {
            sheet,
            point: b,
            to: pt(80.0, 0.0),
        })
        .is_some());
    let moved = h.state().editor.sheet().position(id).unwrap();
    assert!(pf_geometry::distance_mm(moved, pt(40.0, 30.0)) < 1e-9);
    // Parent éloigné au point que les arcs ne se coupent plus : refus, rien ne bouge.
    assert!(h
        .state_mut()
        .editor
        .exec(pf_core::Command::MovePoint {
            sheet,
            point: b,
            to: pt(200.0, 0.0),
        })
        .is_none());
    assert!(h.state().editor.status.error);
    assert_eq!(h.state().editor.sheet().position(id).unwrap(), moved);
    let _ = a;
}

#[test]
fn us026_division_saisie_des_parts_une_seule_etape_annulable() {
    let mut h = harness();
    let line;
    {
        let ed = &mut h.state_mut().editor;
        ed.snap_to_paths = true;
        ed.grid_snap_mm = None;
        let sheet = ed.sheet;
        let out = ed
            .exec(pf_core::Command::AddSegment {
                sheet,
                from: pf_core::PointRef::New(pt(0.0, 0.0)),
                to: pf_core::PointRef::New(pt(100.0, 0.0)),
            })
            .unwrap();
        line = out.entities[0];
    }
    let before = h.state().editor.sheet().points.len();
    h.run_steps(2);
    key(&mut h, Key::O);
    h.get_by_label("Division").click();
    h.run_steps(2);
    click(&mut h, pt(50.0, 0.0));
    assert_eq!(h.state().editor.state, ToolState::RelativePick);
    h.run_steps(2);
    // Saisie invalide : message, rien créé, saisie conservée.
    type_text(&mut h, "1");
    key(&mut h, Key::Enter);
    h.run_steps(2);
    assert!(h.state().editor.status.error);
    assert_eq!(h.state().editor.sheet().points.len(), before);
    // Proportions 1:3 puis Entrée : un point à 25 %.
    h.state_mut().editor.length_text = "1:3".into();
    key(&mut h, Key::Enter);
    h.run_steps(2);
    let ed = &h.state().editor;
    assert_eq!(ed.sheet().points.len(), before + 1, "{}", ed.status.text);
    let id = ed.selection.points[0];
    assert!(matches!(
        ed.sheet().point(id).unwrap().definition,
        pf_core::PointDefinition::Division { trace, .. } if trace == line
    ));
    assert!(pf_geometry::distance_mm(ed.sheet().position(id).unwrap(), pt(25.0, 0.0)) < 1e-9);
    // Une seule étape d'historique : un Annuler retire le point.
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run_steps(3);
    assert_eq!(h.state().editor.sheet().points.len(), before);
}

#[test]
fn us027_prolonger_couper_fusionner_inverser_par_l_interface() {
    let mut h = harness();
    let (a, wall);
    {
        let ed = &mut h.state_mut().editor;
        ed.grid_snap_mm = None;
        let sheet = ed.sheet;
        let mut add = |from: PointMm, to: PointMm| {
            ed.exec(pf_core::Command::AddSegment {
                sheet,
                from: pf_core::PointRef::New(from),
                to: pf_core::PointRef::New(to),
            })
            .unwrap()
            .entities[0]
        };
        a = add(pt(0.0, 0.0), pt(60.0, 0.0));
        wall = add(pt(100.0, -30.0), pt(100.0, 30.0));
    }
    h.run_steps(2);
    let length =
        |h: &Harness<'_, App>, e| entity_length(h.state().editor.sheet(), e).unwrap().value;

    // Prolonger d'une valeur saisie : une seule étape annulable.
    key(&mut h, Key::I);
    assert_eq!(h.state().editor.tool, Tool::Extend);
    click(&mut h, pt(55.0, 0.0));
    assert!(matches!(
        h.state().editor.state,
        ToolState::ExtendFrom { entity, .. } if entity == a
    ));
    // Entrée sans valeur : refus affiché, rien ne change.
    key(&mut h, Key::Enter);
    assert!(h.state().editor.status.error);
    assert!((length(&h, a) - 60.0).abs() < 1e-9);
    h.state_mut().editor.length_text = "20".into();
    key(&mut h, Key::Enter);
    assert!(
        (length(&h, a) - 80.0).abs() < 1e-9,
        "{}",
        h.state().editor.status.text
    );
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run_steps(3);
    assert!((length(&h, a) - 60.0).abs() < 1e-9);

    // Prolonger jusqu'à un autre tracé.
    key(&mut h, Key::I);
    click(&mut h, pt(55.0, 0.0));
    click(&mut h, pt(100.0, 10.0));
    assert!(
        (length(&h, a) - 100.0).abs() < 1e-6,
        "{}",
        h.state().editor.status.text
    );
    assert!(!h.state().editor.status.error);

    // Couper : deux tracés, longueur totale conservée, une seule étape.
    key(&mut h, Key::K);
    assert_eq!(h.state().editor.tool, Tool::Split);
    click(&mut h, pt(50.0, 0.0));
    let s = h.state().editor.sheet();
    assert_eq!(s.entities.len(), 3, "{}", h.state().editor.status.text);
    let second = s.entities[1].id;
    assert!((length(&h, a) + length(&h, second) - 100.0).abs() < 1e-9);
    // Clic dans le vide : refus affiché, rien ne change.
    click(&mut h, pt(50.0, 80.0));
    assert!(h.state().editor.status.error);
    assert_eq!(h.state().editor.sheet().entities.len(), 3);

    // Fusionner les deux tronçons ; refus si on reclique le même tracé.
    key(&mut h, Key::J);
    assert_eq!(h.state().editor.tool, Tool::Merge);
    click(&mut h, pt(20.0, 0.0));
    click(&mut h, pt(20.0, 0.0));
    assert!(h.state().editor.status.error);
    click(&mut h, pt(80.0, 0.0));
    let s = h.state().editor.sheet();
    assert_eq!(s.entities.len(), 2, "{}", h.state().editor.status.text);
    assert!((length(&h, a) - 100.0).abs() < 1e-9);
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run_steps(3);
    assert_eq!(h.state().editor.sheet().entities.len(), 3);
    h.key_press_modifiers(Modifiers::COMMAND, Key::Y);
    h.run_steps(3);
    assert_eq!(h.state().editor.sheet().entities.len(), 2);

    // Inverser le sens de la sélection (menu Édition).
    h.state_mut().editor.set_tool(Tool::Select);
    h.state_mut().editor.selection.entities = vec![a];
    h.run_steps(2);
    h.get_by_label("Édition").click();
    h.run_steps(2);
    h.get_by_label("Inverser le sens du tracé").click();
    h.run_steps(3);
    let s = h.state().editor.sheet();
    let (start, _) = s.path_point_at(a, 0.0).unwrap();
    assert!(
        pf_geometry::distance_mm(start, pt(100.0, 0.0)) < 1e-6,
        "{start:?}"
    );
    assert!(!h.state().editor.status.error);
    h.state().editor.project().validate().unwrap();
    let _ = wall;
}

#[test]
fn us0210_decalage_de_contour_par_l_interface() {
    use pf_core::contour::{contour_from_entities, measure_contour};
    let mut h = harness();
    let rect;
    {
        let ed = &mut h.state_mut().editor;
        ed.grid_snap_mm = None;
        let sheet = ed.sheet;
        rect = ed
            .exec(pf_core::Command::AddRectangle {
                sheet,
                corner: pf_core::PointRef::New(pt(0.0, 0.0)),
                width_mm: 100.0,
                height_mm: 50.0,
            })
            .unwrap()
            .entities;
    }
    h.run_steps(2);
    let count = |h: &Harness<'_, App>| h.state().editor.sheet().entities.len();
    let perimeter = |h: &Harness<'_, App>, ids: &[pf_core::EntityId]| {
        let s = h.state().editor.sheet();
        let c = contour_from_entities(s, ids).unwrap();
        measure_contour(s, &c).unwrap().perimeter_mm
    };

    // Le raccourci X active l'outil ; un clic dans le vide est refusé.
    key(&mut h, Key::X);
    assert_eq!(h.state().editor.tool, Tool::Offset);
    click(&mut h, pt(50.0, 25.0));
    assert!(h.state().editor.status.error);
    assert_eq!(count(&h), 4);

    // Clic sur un côté : toute la chaîne fermée est choisie.
    click(&mut h, pt(50.0, 0.0));
    assert_eq!(h.state().editor.state, ToolState::OffsetReady);
    assert_eq!(h.state().editor.offset_paths.len(), 4);
    // Une distance nulle ou négative saisie est refusée, rien ne change.
    move_to(&mut h, pt(50.0, -20.0));
    h.state_mut().editor.length_text = "0".into();
    key(&mut h, Key::Enter);
    assert!(h.state().editor.status.error);
    assert_eq!(count(&h), 4);

    // Distance saisie, côté de la souris (à l'extérieur) : rectangle exact 110 × 60.
    h.state_mut().editor.length_text = "5".into();
    key(&mut h, Key::Enter);
    assert!(
        !h.state().editor.status.error,
        "{}",
        h.state().editor.status.text
    );
    assert_eq!(count(&h), 8);
    let made = h.state().editor.selection.entities.clone();
    assert_eq!(made.len(), 4);
    assert!((perimeter(&h, &made) - 340.0).abs() < 1e-9);
    // Rôle par défaut : construction, calque actif (aucun ici).
    let s = h.state().editor.sheet();
    assert!(made
        .iter()
        .all(|&e| s.entity(e).unwrap().role == pf_core::PathRole::Construction));

    // Un seul Annuler.
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run_steps(3);
    assert_eq!(count(&h), 4);

    // Côté inversé : la souris est à l'extérieur, le décalage va à l'intérieur
    // (92 × 42, coins intérieurs coupés, aucun arc malgré l'option « arrondi »).
    key(&mut h, Key::X);
    click(&mut h, pt(50.0, 0.0));
    move_to(&mut h, pt(50.0, -20.0));
    h.state_mut().editor.flip_offset();
    h.state_mut()
        .editor
        .set_offset_corner(pf_geometry::CornerStyle::Round);
    h.state_mut().editor.length_text = "4".into();
    key(&mut h, Key::Enter);
    assert_eq!(count(&h), 8, "{}", h.state().editor.status.text);
    let made = h.state().editor.selection.entities.clone();
    assert!((perimeter(&h, &made) - 268.0).abs() < 1e-9);
    h.state().editor.project().validate().unwrap();
    // L'original est intact.
    for e in rect {
        assert!(h.state().editor.sheet().entity(e).is_ok());
    }
}

#[test]
fn us028_symetrie_propose_l_axe_du_guide_selectionne() {
    use crate::transform::Kind;
    let mut h = harness();
    draw_reference(&mut h);
    let ed = &mut h.state_mut().editor;
    ed.set_tool(Tool::Guide);
    ed.set_guide_mode(crate::editor::GuideMode::Vertical);
    ed.length_text = "150".into();
    ed.confirm();
    let guide = ed.sheet().guides[0].id;
    let segment = ed.sheet().entities[0].id;
    let before = ed.sheet().entities.len();
    ed.set_tool(Tool::Select);
    ed.selection.entities = vec![segment];
    ed.selection.guides = vec![guide];
    h.state_mut().open_transform(true);
    let dialog = h.state().transform.clone().expect("fenêtre ouverte");
    assert_eq!(dialog.kind, Kind::Mirror);
    assert_eq!((dialog.cx, dialog.angle), (150.0, 90.0));
    h.run_steps(3);
    h.get_by_label("Axe d'une ligne d'aide");
    h.get_by_label("Appliquer").click();
    h.run_steps(3);
    let s = h.state().editor.sheet();
    assert_eq!(
        s.entities.len(),
        before + 1,
        "{}",
        h.state().editor.status.text
    );
    let (a, b) = s.segment_ends(s.entities.last().unwrap().id).unwrap();
    assert_eq!((a.x.min(b.x), a.x.max(b.x)), (180.0, 300.0));
    h.state().editor.project().validate().unwrap();
}

#[test]
fn us028_ligne_d_aide_clavier_accrochage_masquage_et_suppression() {
    let mut h = harness();
    draw_reference(&mut h);

    // Raccourci U : outil Ligne d'aide, horizontale par défaut ; un clic la crée.
    key(&mut h, Key::U);
    assert_eq!(h.state().editor.tool, Tool::Guide);
    click(&mut h, pt(40.0, 30.0));
    let s = h.state().editor.sheet();
    assert_eq!(s.guides.len(), 1, "{}", h.state().editor.status.text);
    assert_eq!((s.guides[0].y_mm, s.guides[0].angle_deg), (30.0, 0.0));
    assert_eq!(h.state().editor.selection.guides.len(), 1);
    // Création sans toucher aux tracés ni aux points.
    assert_eq!(s.entities.len(), 2);

    // Le curseur près de la ligne propose l'accrochage « sur ligne d'aide »,
    // et l'intersection avec le segment de référence (y = 0) est trouvée par
    // une verticale.
    move_to(&mut h, pt(70.0, 30.2));
    let snap = h.state().editor.hover_snap().expect("accrochage");
    assert_eq!(snap.kind, crate::editor::SnapKind::OnGuide);
    assert_eq!(snap.pos.y, 30.0);
    h.state_mut()
        .editor
        .set_guide_mode(crate::editor::GuideMode::Vertical);
    click(&mut h, pt(60.0, 50.0));
    assert_eq!(h.state().editor.sheet().guides.len(), 2);
    move_to(&mut h, pt(60.2, 30.2));
    let snap = h.state().editor.hover_snap().expect("accrochage");
    assert_eq!(snap.kind, crate::editor::SnapKind::Intersection);
    assert!(pf_geometry::distance_mm(snap.pos, pt(60.0, 30.0)) < 1e-9);

    // Sélection au clic, inspecteur, puis suppression au clavier.
    h.state_mut().editor.set_tool(Tool::Select);
    h.state_mut().editor.selection.clear();
    click(&mut h, pt(10.0, 30.0));
    assert_eq!(h.state().editor.selection.guides.len(), 1);
    h.run_steps(3);
    h.get_by_label("Axe de symétrie");
    key(&mut h, Key::Delete);
    assert_eq!(h.state().editor.sheet().guides.len(), 1);
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run_steps(3);
    assert_eq!(h.state().editor.sheet().guides.len(), 2);

    // Affichage > Lignes d'aide masque (et désactive l'accrochage et la sélection).
    h.get_by_label("Affichage").click();
    h.run_steps(2);
    h.get_by_label("Lignes d'aide").click();
    h.run_steps(3);
    assert!(!h.state().editor.show_guides);
    h.state_mut().editor.selection.clear();
    click(&mut h, pt(10.0, 30.0));
    assert!(h.state().editor.selection.guides.is_empty());
    move_to(&mut h, pt(70.0, 30.2));
    assert!(h.state().editor.hover_snap().is_none());
    assert_eq!(
        h.state().editor.sheet().guides.len(),
        2,
        "masquer ne supprime rien"
    );
    h.state().editor.project().validate().unwrap();
}

#[test]
fn us029_menu_edition_aligner_a_gauche_en_une_etape_annulable() {
    let mut h = harness();
    let ed = &mut h.state_mut().editor;
    let sheet = ed.sheet;
    let ids: Vec<_> = [(10.0, 5.0), (30.0, 25.0), (-4.0, 12.5)]
        .iter()
        .map(|&(x, y)| {
            ed.exec(pf_core::Command::AddPoint {
                sheet,
                at: pt(x, y),
            })
            .unwrap()
            .points[0]
        })
        .collect();
    ed.selection.points = ids.clone();
    h.get_by_label("Édition").click();
    h.run_steps(2);
    h.get_by_label_contains("Aligner").click();
    h.run_steps(2);
    h.get_by_label("À gauche").click();
    h.run_steps(3);
    let ed = &h.state().editor;
    assert!(!ed.status.error, "{}", ed.status.text);
    for &i in &ids {
        let p = ed.sheet().position(i).unwrap();
        assert!((p.x + 4.0).abs() < 1e-9, "{p:?}");
    }
    // Une seule étape d'historique : un annuler rétablit l'état précédent.
    h.state_mut().editor.undo();
    let after_undo = h.state().editor.sheet().position(ids[0]).unwrap();
    assert_eq!(after_undo, pt(10.0, 5.0));
    assert_eq!(
        h.state().editor.sheet().position(ids[1]).unwrap(),
        pt(30.0, 25.0)
    );
}

#[test]
fn us029_mise_a_l_horizontale_par_la_fenetre_et_refus_sans_selection() {
    let mut h = harness();
    let ed = &mut h.state_mut().editor;
    let sheet = ed.sheet;
    let e = ed
        .exec(pf_core::Command::AddSegment {
            sheet,
            from: pf_core::PointRef::New(pt(0.0, 0.0)),
            to: pf_core::PointRef::New(pt(30.0, 40.0)),
        })
        .unwrap()
        .entities[0];
    h.state_mut().open_level(true);
    assert!(h.state().level.is_none(), "rien de sélectionné : refus");
    h.state_mut().editor.selection.entities = vec![e];
    h.state_mut().open_level(true);
    h.run_steps(3);
    h.get_by_label("Appliquer").click();
    h.run_steps(3);
    let s = h.state().editor.sheet();
    let (pa, pb) = s.segment_ends(e).unwrap();
    assert!((pa.y - pb.y).abs() < 1e-9, "{pa:?} {pb:?}");
    assert!((pf_geometry::distance_mm(pa, pb) - 50.0).abs() < 1e-9);
    assert!(h.state().level.is_none());
}

#[test]
fn us0211_menu_edition_symetrie_liee_puis_deplier_en_une_etape_annulable() {
    let mut h = harness();
    let ed = &mut h.state_mut().editor;
    let sheet = ed.sheet;
    let guide = ed
        .exec(pf_core::Command::AddGuide {
            sheet,
            x_mm: 0.0,
            y_mm: 0.0,
            angle_deg: 90.0,
            axis: true,
            layer: None,
        })
        .unwrap()
        .guides[0];
    let src = ed
        .exec(pf_core::Command::AddPoint {
            sheet,
            at: pt(30.0, 12.0),
        })
        .unwrap()
        .points[0];
    // Sans axe sélectionné : refus affiché, aucun point créé.
    ed.selection.points = vec![src];
    h.get_by_label("Édition").click();
    h.run_steps(2);
    h.get_by_label("Symétrie liée…").click();
    h.run_steps(3);
    assert!(h.state().editor.status.error);
    assert_eq!(h.state().editor.sheet().points.len(), 1);
    // Avec l'axe sélectionné : un point miroir maintenu.
    h.state_mut().editor.selection.guides = vec![guide];
    h.get_by_label("Édition").click();
    h.run_steps(2);
    h.get_by_label("Symétrie liée…").click();
    h.run_steps(3);
    let ed = &h.state().editor;
    assert!(!ed.status.error, "{}", ed.status.text);
    let mirror = ed
        .sheet()
        .points
        .iter()
        .find(|p| p.definition.is_mirror())
        .expect("point miroir")
        .id;
    assert_eq!(ed.sheet().position(mirror).unwrap(), pt(-30.0, 12.0));
    // Déplier : le point devient libre ; un annuler rétablit la symétrie.
    h.get_by_label("Édition").click();
    h.run_steps(2);
    h.get_by_label("Déplier la symétrie").click();
    h.run_steps(3);
    assert!(h
        .state()
        .editor
        .sheet()
        .point(mirror)
        .unwrap()
        .definition
        .is_free());
    h.state_mut().editor.undo();
    assert!(h
        .state()
        .editor
        .sheet()
        .point(mirror)
        .unwrap()
        .definition
        .is_mirror());
    h.state().editor.project().validate().unwrap();
}

// ----- US-16.1 : sélection avancée ---------------------------------------------

/// Deux segments côte à côte sur la feuille (hors souris).
fn us161_two_segments(h: &mut Harness<'_, App>) -> (pf_core::EntityId, pf_core::EntityId) {
    let ed = &mut h.state_mut().editor;
    let sheet = ed.sheet;
    let mut add = |a: PointMm, b: PointMm| {
        ed.exec(pf_core::Command::AddSegment {
            sheet,
            from: pf_core::PointRef::New(a),
            to: pf_core::PointRef::New(b),
        })
        .unwrap()
        .entities[0]
    };
    let near = add(pt(20.0, 20.0), pt(60.0, 20.0));
    let far = add(pt(300.0, 300.0), pt(340.0, 300.0));
    (near, far)
}

#[test]
fn us161_ctrl_a_ctrl_i_et_projet_non_modifie() {
    let mut h = harness();
    let (near, far) = us161_two_segments(&mut h);
    let dirty = h.state().editor.doc.is_dirty();
    let could_undo = h.state().editor.doc.can_undo();
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.run_steps(3);
    // Deux tracés et leurs quatre extrémités.
    assert_eq!(h.state().editor.selection.len(), 6);
    h.state_mut().editor.selection.points.clear();
    h.key_press_modifiers(Modifiers::COMMAND, Key::I);
    h.run_steps(3);
    // Inversion : les quatre points reviennent, les deux tracés (sélectionnés) sortent.
    let sel = &h.state().editor.selection;
    assert_eq!(sel.entities.len(), 0);
    assert_eq!(sel.points.len(), 4);
    assert!(!sel.entities.contains(&near) && !sel.entities.contains(&far));
    assert_eq!(
        h.state().editor.doc.is_dirty(),
        dirty,
        "Ctrl+A / Ctrl+I ne salissent pas le projet"
    );
    assert_eq!(h.state().editor.doc.can_undo(), could_undo);
}

#[test]
fn us161_menu_edition_selection_tout_selectionner_et_filtre() {
    let mut h = harness();
    let (near, _) = us161_two_segments(&mut h);
    {
        let ed = &mut h.state_mut().editor;
        let sheet = ed.sheet;
        let (a, b) = ed.sheet().entity_points(near).unwrap();
        ed.exec(pf_core::Command::AddDimension {
            sheet,
            from: a,
            to: b,
            offset_mm: 10.0,
        })
        .unwrap();
    }
    h.get_by_label("Édition").click();
    h.run_steps(2);
    h.get_by_label("Sélection ⏵").click();
    h.run_steps(2);
    h.get_by_label_contains("Tout sélectionner").click();
    h.run_steps(3);
    // Deux tracés, quatre extrémités, une cotation.
    assert_eq!(h.state().editor.selection.len(), 7);
    // Filtre : décocher « Cotations » désélectionne la cotation et garde le reste.
    h.get_by_label("Édition").click();
    h.run_steps(2);
    h.get_by_label("Sélection ⏵").click();
    h.run_steps(2);
    h.get_by_label("Filtres de sélection ⏵").click();
    h.run_steps(2);
    h.get_by_label("Cotations").click();
    h.run_steps(3);
    let ed = &h.state().editor;
    assert!(!ed.select_filter.dimensions);
    assert!(ed.selection.dimensions.is_empty());
    assert_eq!(ed.selection.len(), 6);
    assert!(!ed.status.error, "{}", ed.status.text);
}

#[test]
fn us161_lasso_a_la_souris_entierement_dedans_seulement() {
    let mut h = harness();
    let (near, far) = us161_two_segments(&mut h);
    // Ctrl+L : mode lasso.
    h.key_press_modifiers(Modifiers::COMMAND, Key::L);
    h.run_steps(2);
    assert!(h.state().editor.lasso_mode);
    // Glisser en quadrilatère autour du premier segment seulement.
    move_to(&mut h, pt(0.0, 0.0));
    button(&mut h, pt(0.0, 0.0), true);
    for corner in [
        pt(100.0, 0.0),
        pt(100.0, 60.0),
        pt(0.0, 60.0),
        pt(2.0, 30.0),
    ] {
        move_to(&mut h, corner);
    }
    assert!(h.state().lasso.is_some(), "le lasso est en cours de tracé");
    button(&mut h, pt(2.0, 30.0), false);
    h.run_steps(2);
    assert!(h.state().lasso.is_none());
    let sel = &h.state().editor.selection;
    assert!(sel.entities.contains(&near), "{sel:?}");
    assert!(!sel.entities.contains(&far));
    assert_eq!(sel.points.len(), 2);
    // Retour à la fenêtre rectangulaire.
    h.key_press_modifiers(Modifiers::COMMAND, Key::L);
    h.run_steps(2);
    assert!(!h.state().editor.lasso_mode);
}

// ----- US-16.2 : saisie directe -------------------------------------------------

#[test]
fn us162_bulle_pres_du_curseur_segment_tab_entree_et_mesure_observee() {
    let mut h = harness();
    key(&mut h, Key::S);
    // Avant le premier point : un champ « Position » qui suit la souris.
    move_to(&mut h, pt(5.0, 5.0));
    h.step();
    h.get_by_label_contains("Position (x ; y)");
    click(&mut h, pt(0.0, 0.0));
    move_to(&mut h, pt(55.0, 23.0));
    h.step();
    // Rien de saisi : longueur et angle sont des mesures observées.
    assert_eq!(h.query_all_by_label_contains("· mesuré").count(), 2);
    assert!(h.query_all_by_label_contains("· saisi").next().is_none());
    type_text(&mut h, "120");
    key(&mut h, Key::Tab);
    type_text(&mut h, "0");
    assert_eq!(h.state().editor.length_text, "120");
    assert_eq!(h.state().editor.angle_text, "0");
    assert_eq!(h.state().editor.entry_field, 1, "Tab suit le champ actif");
    h.step();
    // Les mêmes champs vus dans la bulle : valeurs saisies, libellé « saisi ».
    assert_eq!(h.query_all_by_label_contains("· saisi").count(), 2);
    h.get_by_label_contains(" 120 ");
    key(&mut h, Key::Enter);
    let ed = &h.state().editor;
    assert_eq!(ed.sheet().entities.len(), 1, "{}", ed.status.text);
    let id = ed.sheet().entities[0].id;
    assert!((entity_length(ed.sheet(), id).unwrap().value - 120.0).abs() < 1e-9);
    // Étape suivante : la bulle redemande une position.
    assert!(h.query_all_by_label_contains("· saisi").next().is_none());
    h.get_by_label_contains("Position (x ; y)");
}

#[test]
fn us162_maj_et_souris_verrouillent_l_angle_a_15_degres() {
    let mut h = harness();
    key(&mut h, Key::S);
    click(&mut h, pt(0.0, 0.0));
    set_shift(&mut h, true);
    move_to(&mut h, pt(100.0, 30.0)); // 16,7° à la souris
    h.step();
    h.get_by_label_contains("Angle verrouillé par pas de 15");
    click(&mut h, pt(100.0, 30.0));
    set_shift(&mut h, false);
    let ed = &h.state().editor;
    assert_eq!(ed.sheet().entities.len(), 1, "{}", ed.status.text);
    let id = ed.sheet().entities[0].id;
    let dir = pf_core::measure::segment_direction_deg(ed.sheet(), id).unwrap();
    assert!((dir - 15.0).abs() < 1e-9, "{dir}");
    let len = entity_length(ed.sheet(), id).unwrap().value;
    assert!((len - 100.0f64.hypot(30.0)).abs() < 1e-9, "{len}");
    // Maj relâchée : plus de mention du verrou.
    click(&mut h, pt(0.0, 50.0));
    move_to(&mut h, pt(100.0, 80.0));
    h.step();
    assert!(h
        .query_all_by_label_contains("Angle verrouillé")
        .next()
        .is_none());
}

#[test]
fn us162_coordonnees_au_clavier_absolue_puis_polaire_relative() {
    let mut h = harness();
    key(&mut h, Key::S);
    move_to(&mut h, pt(5.0, 5.0));
    // Aucun champ n'a le focus : les touches vont à la bulle (position).
    type_text(&mut h, "20;10");
    assert_eq!(h.state().editor.position_text, "20;10");
    h.step();
    h.get_by_label_contains("20;10");
    let untouched = h.state().editor.sheet().entities.len();
    key(&mut h, Key::Enter);
    assert!(matches!(
        h.state().editor.state,
        ToolState::SegmentFrom { .. }
    ));
    assert_eq!(h.state().editor.sheet().entities.len(), untouched);
    // Polaire relative au départ posé : 30 mm à 45°.
    type_text(&mut h, "@30<45");
    assert_eq!(h.state().editor.length_text, "@30<45");
    key(&mut h, Key::Enter);
    let ed = &h.state().editor;
    assert_eq!(ed.sheet().entities.len(), 1, "{}", ed.status.text);
    let (a, b) = ed.sheet().segment_ends(ed.sheet().entities[0].id).unwrap();
    let k = 30.0 / 2f64.sqrt();
    assert!((a.x - 20.0).abs() < 1e-9 && (a.y - 10.0).abs() < 1e-9);
    assert!((b.x - (20.0 + k)).abs() < 1e-9 && (b.y - (10.0 + k)).abs() < 1e-9);
    // Une seule étape d'annulation.
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run_steps(2);
    assert!(h.state().editor.sheet().entities.is_empty());
    // Saisie refusée : message, rien ne change.
    key(&mut h, Key::S);
    type_text(&mut h, "1;2;3");
    key(&mut h, Key::Enter);
    assert!(h.state().editor.status.error);
    assert!(h.state().editor.sheet().entities.is_empty());
    assert_eq!(h.state().editor.state, ToolState::Idle);
}

#[test]
fn us162_repeter_la_derniere_commande_au_raccourci_et_par_le_menu() {
    // Aucun outil mémorisé : entrée de menu grisée, Entrée sans effet.
    let mut h = harness();
    key(&mut h, Key::Enter);
    assert_eq!(h.state().editor.tool, Tool::Select);
    h.get_by_label("Édition").click();
    h.run_steps(2);
    assert!(h
        .get_by_label_contains("Répéter la dernière commande")
        .accesskit_node()
        .is_disabled());

    let mut h = harness();
    key(&mut h, Key::S);
    click(&mut h, pt(0.0, 0.0));
    type_text(&mut h, "50");
    key(&mut h, Key::Tab);
    type_text(&mut h, "30");
    key(&mut h, Key::Enter);
    assert_eq!(h.state().editor.sheet().entities.len(), 1);
    // Entrée sans outil de création : l'outil revient avec ses valeurs.
    key(&mut h, Key::V);
    assert_eq!(h.state().editor.tool, Tool::Select);
    key(&mut h, Key::Enter);
    {
        let ed = &h.state().editor;
        assert_eq!(ed.tool, Tool::Segment);
        assert_eq!(
            (ed.length_text.as_str(), ed.angle_text.as_str()),
            ("50", "30")
        );
        assert_eq!(ed.sheet().entities.len(), 1, "aucune commande rejouée");
    }
    // Un clic de départ puis Entrée : second segment identique.
    click(&mut h, pt(0.0, 100.0));
    key(&mut h, Key::Enter);
    assert_eq!(h.state().editor.sheet().entities.len(), 2);
    // Par le menu Édition, avec le raccourci affiché.
    key(&mut h, Key::V);
    h.get_by_label("Édition").click();
    h.run_steps(2);
    let item = h.get_by_label_contains("Répéter la dernière commande");
    assert!(!item.accesskit_node().is_disabled());
    item.click();
    h.run_steps(3);
    assert_eq!(h.state().editor.tool, Tool::Segment);
    assert_eq!(h.state().editor.sheet().entities.len(), 2);
}

/// Deux rectangles de 100 × 100 qui se croisent sans point commun (zone commune 50 × 50).
fn draw_crossing_rectangles(h: &mut Harness<'_, App>) {
    let ed = &mut h.state_mut().editor;
    ed.grid_snap_mm = None;
    ed.snap_to_paths = false;
    let sheet = ed.sheet;
    for corner in [pt(0.0, 0.0), pt(50.0, 50.0)] {
        ed.exec(pf_core::Command::AddRectangle {
            sheet,
            corner: pf_core::PointRef::New(corner),
            width_mm: 100.0,
            height_mm: 100.0,
        })
        .unwrap();
    }
    h.run_steps(2);
}

#[test]
fn us041_clic_dans_une_zone_fermee_cree_une_piece() {
    let mut h = harness();
    draw_crossing_rectangles(&mut h);
    key(&mut h, Key::Z);
    assert_eq!(h.state().editor.tool, Tool::ExtractPiece);
    // Clic dans la zone commune : une pièce de 25 cm² (mesure observée), tracés coupés.
    click(&mut h, pt(75.0, 75.0));
    let ed = &h.state().editor;
    assert!(!ed.status.error, "{}", ed.status.text);
    assert_eq!(ed.project().pieces.len(), 1);
    assert_eq!(ed.project().pieces[0].name, "Pièce 1");
    assert_eq!(ed.sheet().entities.len(), 12);
    assert!(ed.status.text.contains("25,0 cm²"), "{}", ed.status.text);
    assert_eq!(
        ed.selection.entities.len(),
        4,
        "le contour est mis en évidence"
    );
    // Inspecteur de la feuille : la pièce, ses mesures observées et son lien.
    h.state_mut().editor.selection.clear();
    h.run_steps(3);
    h.get_by_label_contains("MESURES OBSERVÉES");
    h.get_by_label("25,0 cm²");
    h.get_by_label_contains("anti-horaire");
    h.get_by_label_contains("Liée");
    // Deuxième clic dans une zone non commune : la pièce en L, 75 cm².
    click(&mut h, pt(20.0, 20.0));
    let ed = &h.state().editor;
    assert_eq!(ed.project().pieces.len(), 2);
    assert_eq!(ed.project().pieces[1].name, "Pièce 2");
    assert!(ed.status.text.contains("75,0 cm²"), "{}", ed.status.text);
    ed.project().validate().unwrap();
}

#[test]
fn us041_clic_dans_une_zone_ouverte_affiche_l_erreur() {
    let mut h = harness();
    {
        let ed = &mut h.state_mut().editor;
        ed.grid_snap_mm = None;
        let sheet = ed.sheet;
        for (a, b) in [
            (pt(0.0, 0.0), pt(100.0, 0.0)),
            (pt(100.0, 0.0), pt(100.0, 100.0)),
            (pt(100.0, 100.0), pt(0.0, 100.0)),
            (pt(0.0, 100.0), pt(0.0, 50.25)),
            (pt(0.0, 49.75), pt(0.0, 0.0)),
        ] {
            ed.exec(pf_core::Command::AddSegment {
                sheet,
                from: pf_core::PointRef::New(a),
                to: pf_core::PointRef::New(b),
            })
            .unwrap();
        }
    }
    h.run_steps(2);
    let before = h.state().editor.project().clone();
    key(&mut h, Key::Z);
    click(&mut h, pt(50.0, 50.0));
    let ed = &h.state().editor;
    assert!(ed.status.error);
    assert!(
        ed.status.text.contains("ouverture de 0,5 mm"),
        "{}",
        ed.status.text
    );
    let gap = ed.piece_gap.expect("extrémités à surligner");
    assert!((gap.gap_mm - 0.5).abs() < 1e-9);
    assert!((gap.from.x).abs() < 1e-9 && (gap.to.x).abs() < 1e-9);
    assert_eq!(ed.project(), &before, "refus sans trace");
    h.step();
    h.get_by_label_contains("Région non fermée");
    // Hors de toute région : autre message, plus d'ouverture surlignée.
    click(&mut h, pt(130.0, 20.0));
    let ed = &h.state().editor;
    assert!(ed.status.error && ed.piece_gap.is_none());
    assert!(
        ed.status.text.contains("Aucune région fermée"),
        "{}",
        ed.status.text
    );
    assert!(ed.project().pieces.is_empty());
}

#[test]
fn us041_annulation_en_une_etape_et_copie_detachee() {
    let mut h = harness();
    draw_crossing_rectangles(&mut h);
    let before = h.state().editor.project().clone();
    key(&mut h, Key::Z);
    click(&mut h, pt(75.0, 75.0));
    assert_eq!(h.state().editor.project().pieces.len(), 1);
    assert_eq!(h.state().editor.sheet().entities.len(), 12);
    // Un seul Ctrl+Z défait les coupes et la pièce.
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run_steps(2);
    assert_eq!(h.state().editor.project(), &before);
    // Détachée : une feuille dédiée, la construction reste intacte.
    h.state_mut().editor.piece_detached = true;
    click(&mut h, pt(75.0, 75.0));
    let ed = &h.state().editor;
    assert!(!ed.status.error, "{}", ed.status.text);
    assert_eq!(ed.project().sheets.len(), 2);
    assert_eq!(
        ed.sheet().entities.len(),
        8,
        "la construction n'est pas coupée"
    );
    assert_eq!(ed.project().pieces[0].link, pf_core::PieceLink::Detached);
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run_steps(2);
    assert_eq!(h.state().editor.project(), &before);
}

#[test]
fn us041_creer_une_piece_depuis_la_selection() {
    let mut h = harness();
    {
        let ed = &mut h.state_mut().editor;
        let sheet = ed.sheet;
        let out = ed
            .exec(pf_core::Command::AddRectangle {
                sheet,
                corner: pf_core::PointRef::New(pt(0.0, 0.0)),
                width_mm: 60.0,
                height_mm: 40.0,
            })
            .unwrap();
        // Trois côtés seulement : pas un contour fermé, l'entrée de menu est grisée.
        ed.selection.entities = out.entities[..3].to_vec();
    }
    h.run_steps(2);
    h.get_by_label("Édition").click();
    h.run_steps(2);
    assert!(h
        .get_by_label_contains("Créer une pièce depuis la sélection")
        .accesskit_node()
        .is_disabled());
    key(&mut h, Key::Escape);
    // Les quatre côtés : contour fermé, la pièce se crée depuis le menu.
    {
        let ed = &mut h.state_mut().editor;
        ed.selection.entities = ed.sheet().entities.iter().map(|e| e.id).collect();
    }
    h.run_steps(2);
    h.get_by_label("Édition").click();
    h.run_steps(2);
    // Le menu et l'inspecteur proposent la même action : l'un ou l'autre convient.
    h.query_all_by_label_contains("Créer une pièce depuis la sélection")
        .next()
        .expect("action proposée")
        .click();
    h.run_steps(3);
    let ed = &h.state().editor;
    assert!(!ed.status.error, "{}", ed.status.text);
    assert_eq!(ed.project().pieces.len(), 1);
    assert!(ed.status.text.contains("24,0 cm²"), "{}", ed.status.text);
    assert_eq!(ed.sheet().entities.len(), 4, "aucun tracé coupé");
}

// ----- US-04.2 : fiche de pièce dans l'inspecteur ---------------------------------

/// Rectangle de 100 × 60 mm et pièce liée « Devant » extraite de sa région ;
/// rien n'est sélectionné, l'inspecteur de la feuille montre la fiche.
fn piece_harness() -> Harness<'static, App> {
    let mut h = harness();
    {
        let ed = &mut h.state_mut().editor;
        ed.grid_snap_mm = None;
        let sheet = ed.sheet;
        ed.exec(pf_core::Command::AddRectangle {
            sheet,
            corner: pf_core::PointRef::New(pt(0.0, 0.0)),
            width_mm: 100.0,
            height_mm: 60.0,
        })
        .unwrap();
        ed.exec(pf_core::Command::ExtractPiece {
            sheet,
            source: pf_core::PieceSource::Click { at: pt(50.0, 30.0) },
            name: "Devant".into(),
            detached: false,
        })
        .unwrap();
        ed.selection.clear();
    }
    h.run_steps(3);
    h
}

/// Saisit `text` dans le champ de texte à une ligne n° `index` du panneau (ordre d'affichage :
/// nom, code, quantité, matière…), le valide par Entrée.
fn type_in_field(h: &mut Harness<'_, App>, index: usize, text: &str) {
    {
        let fields: Vec<_> = h
            .get_all_by_role(egui::accesskit::Role::TextInput)
            .collect();
        let name = fields
            .iter()
            .position(|f| f.value().as_deref() == Some("Devant"))
            .expect("champ « Nom » de la pièce");
        fields[name + index].focus();
    }
    h.run_steps(2);
    // Remplace le contenu : tout sélectionner puis saisir.
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.run_steps(1);
    type_text(h, text);
    key(h, Key::Enter);
    h.run_steps(2);
}

fn piece_of(h: &Harness<'_, App>) -> pf_core::Piece {
    h.state().editor.project().pieces[0].clone()
}

fn undo(h: &mut Harness<'_, App>) {
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run_steps(2);
}

#[test]
fn us042_editer_le_code_et_la_categorie_dans_l_inspecteur() {
    let mut h = piece_harness();
    h.get_by_label("Code");
    h.get_by_label("Catégorie");
    h.get_by_label("Commentaire");
    h.get_by_label("Paire (×2 en miroir)");
    // Code : champ de texte (nom, code, quantité, matière dans l'ordre d'affichage).
    type_in_field(&mut h, 1, "DV-01");
    assert!(
        !h.state().editor.status.error,
        "{}",
        h.state().editor.status.text
    );
    assert_eq!(piece_of(&h).code.as_deref(), Some("DV-01"));
    // Catégorie : liste déroulante.
    h.get_by_role(egui::accesskit::Role::ComboBox).click();
    h.run_steps(3);
    h.get_by_label("Doublure").click();
    h.run_steps(3);
    assert_eq!(piece_of(&h).category, Some(pf_core::PieceCategory::Lining));
    assert!(
        !h.state().editor.status.error,
        "{}",
        h.state().editor.status.text
    );
    // Commentaire sur plusieurs lignes, validé à la perte du focus.
    {
        let area = h
            .get_all_by_role(egui::accesskit::Role::MultilineTextInput)
            .next()
            .expect("commentaire");
        area.focus();
    }
    h.run_steps(2);
    type_text(&mut h, "Couper\ndouble");
    h.run_steps(2);
    key(&mut h, Key::Tab);
    h.run_steps(2);
    assert_eq!(piece_of(&h).comment.as_deref(), Some("Couper\ndouble"));
    // Paire : une case, une commande.
    h.get_by_label("Paire (×2 en miroir)").click();
    h.run_steps(3);
    assert!(piece_of(&h).mirrored_pair);
    h.state().editor.project().validate().unwrap();
}

#[test]
fn us042_un_champ_valide_est_une_seule_etape_d_annulation() {
    let mut h = piece_harness();
    type_in_field(&mut h, 1, "DV-01");
    type_in_field(&mut h, 3, "Coton");
    assert_eq!(piece_of(&h).code.as_deref(), Some("DV-01"));
    assert_eq!(piece_of(&h).material.as_deref(), Some("Coton"));
    // Un Ctrl+Z défait la matière seulement (pas une étape par frappe).
    undo(&mut h);
    assert_eq!(piece_of(&h).material, None);
    assert_eq!(piece_of(&h).code.as_deref(), Some("DV-01"));
    // Quantité : « 3 » validé = une étape.
    type_in_field(&mut h, 2, "3");
    assert_eq!(piece_of(&h).quantity, 3);
    undo(&mut h);
    assert_eq!(piece_of(&h).quantity, 1);
    assert_eq!(piece_of(&h).code.as_deref(), Some("DV-01"));
    // Le code : un seul undo, retour à l'état sans code.
    undo(&mut h);
    assert_eq!(piece_of(&h).code, None);
    // La case paire : un clic, une étape.
    h.get_by_label("Paire (×2 en miroir)").click();
    h.run_steps(3);
    assert!(piece_of(&h).mirrored_pair);
    undo(&mut h);
    assert!(!piece_of(&h).mirrored_pair);
}

#[test]
fn us042_avertissement_droit_fil_visible_puis_disparait_quand_assigne() {
    let mut h = piece_harness();
    // Avertissement écrit en toutes lettres (pas seulement une couleur).
    h.get_by_label_contains("Avertissement : droit-fil manquant");
    h.get_by_label_contains("1 avec avertissement");
    assert!(h.state().editor.project().validate().is_ok());
    // Sans tracé de droit-fil sélectionné, l'assignation est grisée.
    assert!(h
        .get_by_label("Assigner le droit-fil sélectionné")
        .accesskit_node()
        .is_disabled());
    // Tracer un droit-fil, le sélectionner, l'assigner.
    let grain = {
        let ed = &mut h.state_mut().editor;
        let sheet = ed.sheet;
        let e = ed
            .exec(pf_core::Command::AddSegment {
                sheet,
                from: pf_core::PointRef::New(pt(10.0, 10.0)),
                to: pf_core::PointRef::New(pt(10.0, 50.0)),
            })
            .unwrap()
            .entities[0];
        ed.exec(pf_core::Command::SetPathRole {
            sheet,
            entity: e,
            role: pf_core::PathRole::Grainline,
        })
        .unwrap();
        ed.selection.entities = vec![e];
        e
    };
    h.run_steps(3);
    h.get_by_label_contains("Avertissement : droit-fil manquant");
    let button = h.get_by_label("Assigner le droit-fil sélectionné");
    assert!(!button.accesskit_node().is_disabled());
    button.click();
    h.run_steps(3);
    assert_eq!(piece_of(&h).grainline, Some(grain));
    assert!(
        h.query_by_label_contains("droit-fil manquant").is_none(),
        "l'avertissement doit disparaître"
    );
    h.get_by_label_contains("0 avec avertissement");
    // L'assignation est une étape annulable : l'avertissement revient.
    undo(&mut h);
    assert_eq!(piece_of(&h).grainline, None);
    h.get_by_label_contains("Avertissement : droit-fil manquant");
}

#[test]
fn us042_saisie_invalide_refusee_avec_message_valeur_conservee() {
    let mut h = piece_harness();
    let before = h.state().editor.project().clone();
    // Quantité non numérique, nulle, négative, décimale : message, rien ne change.
    for bad in ["abc", "0", "-2", "1,5", " "] {
        h.state_mut().editor.info("neutre");
        type_in_field(&mut h, 2, bad);
        let ed = &h.state().editor;
        assert!(ed.status.error, "« {bad} » aurait dû être refusé");
        assert!(
            ed.status.text.contains("entier supérieur ou égal à 1"),
            "{}",
            ed.status.text
        );
        assert_eq!(ed.project(), &before, "« {bad} » : projet modifié");
    }
    // Code trop long : refus du cœur, message affiché, valeur précédente conservée.
    h.state_mut().editor.info("neutre");
    type_in_field(&mut h, 1, &"c".repeat(121));
    let ed = &h.state().editor;
    assert!(ed.status.error);
    assert!(ed.status.text.contains("dépasse 120"), "{}", ed.status.text);
    assert_eq!(ed.project(), &before);
    // Le champ réaffiche la valeur conservée (la quantité vaut toujours 1).
    h.run_steps(2);
    assert!(h
        .get_all_by_role(egui::accesskit::Role::TextInput)
        .any(|f| f.value().as_deref() == Some("1")));
    // Un nom vide est refusé aussi.
    h.state_mut().editor.info("neutre");
    type_in_field(&mut h, 0, "   ");
    assert!(h.state().editor.status.error);
    assert_eq!(h.state().editor.project(), &before);
    // Une saisie valide ensuite passe, et l'erreur disparaît du modèle (rien n'a été écrit avant).
    type_in_field(&mut h, 1, "OK-1");
    assert_eq!(piece_of(&h).code.as_deref(), Some("OK-1"));
}

#[test]
fn us042_l_inspecteur_fonctionne_en_anglais() {
    let mut h = piece_harness();
    pf_i18n::set_lang(pf_i18n::Lang::En);
    h.run_steps(3);
    h.get_by_label("Category");
    h.get_by_label("Comment");
    h.get_by_label("Pair (×2 mirrored)");
    h.get_by_label_contains("Warning: grainline missing");
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
}

// ----- US-04.3 : éléments internes ------------------------------------------------

/// `piece_harness` plus, dans le contour de 100 × 60 mm : un pli (rôle pliure) et un
/// point libre ; et un tracé qui sort du contour. Rien n'est sélectionné.
fn internals_harness() -> (Harness<'static, App>, pf_core::EntityId) {
    let mut h = piece_harness();
    // Fenêtre haute : l'inspecteur défile, et un clic hors fenêtre n'atteint rien.
    h.set_size(egui::vec2(1280.0, 1600.0));
    let fold = {
        let ed = &mut h.state_mut().editor;
        let sheet = ed.sheet;
        let fold = ed
            .exec(pf_core::Command::AddSegment {
                sheet,
                from: pf_core::PointRef::New(pt(20.0, 10.0)),
                to: pf_core::PointRef::New(pt(20.0, 50.0)),
            })
            .unwrap()
            .entities[0];
        ed.exec(pf_core::Command::SetPathRole {
            sheet,
            entity: fold,
            role: pf_core::PathRole::Fold,
        })
        .unwrap();
        ed.exec(pf_core::Command::AddPoint {
            sheet,
            at: pt(50.0, 30.0),
        })
        .unwrap();
        ed.exec(pf_core::Command::AddSegment {
            sheet,
            from: pf_core::PointRef::New(pt(80.0, 20.0)),
            to: pf_core::PointRef::New(pt(150.0, 20.0)),
        })
        .unwrap();
        ed.selection.clear();
        fold
    };
    h.run_steps(3);
    open_fold(&mut h, "piece.internal.title");
    (h, fold)
}

fn internals_of(h: &Harness<'_, App>) -> Vec<pf_core::InternalElement> {
    h.state().editor.project().pieces[0].internal.clone()
}

#[test]
fn us043_importer_affiche_le_rapport_et_annule_en_une_etape() {
    let (mut h, fold) = internals_harness();
    h.get_by_label("Éléments internes");
    h.get_by_label("Aucun élément interne.");
    h.get_by_label("Importer depuis la construction").click();
    h.run_steps(3);
    // Pli (type déduit du rôle) et point libre ; le tracé qui sort est ignoré.
    let internal = internals_of(&h);
    assert_eq!(internal.len(), 2, "{internal:?}");
    assert_eq!(
        internal[0],
        pf_core::InternalElement {
            target: pf_core::InternalTarget::Path { entity: fold },
            kind: pf_core::InternalKind::Fold
        }
    );
    assert!(
        !h.state().editor.status.error,
        "{}",
        h.state().editor.status.text
    );
    h.get_by_label_contains("Dernier import : 2 ajouté(s), 5 ignoré(s), 0 déjà présent(s).");
    h.get_by_label_contains("— pas entièrement dans le contour");
    assert_eq!(h.get_all_by_label_contains("— tracé du contour").count(), 4);
    h.get_by_label("1 ligne(s) interne(s), 1 repère(s) (liés à la construction).");
    // Relancé : rien de plus (aucune étape), rapport mis à jour.
    h.get_by_label("Importer depuis la construction").click();
    h.run_steps(3);
    assert_eq!(internals_of(&h).len(), 2);
    h.get_by_label_contains("Dernier import : 0 ajouté(s), 5 ignoré(s), 2 déjà présent(s).");
    // Une seule annulation défait tout l'import.
    undo(&mut h);
    assert!(internals_of(&h).is_empty());
    h.state().editor.project().validate().unwrap();
}

#[test]
fn us043_changer_le_type_et_retirer() {
    let (mut h, fold) = internals_harness();
    let piece = piece_of(&h).id;
    h.state_mut()
        .editor
        .exec(pf_core::Command::AddPieceInternal {
            piece,
            target: pf_core::InternalTarget::Path { entity: fold },
            kind: pf_core::InternalKind::Fold,
        })
        .unwrap();
    h.run_steps(3);
    // Liste déroulante du type : (catégorie de la pièce, puis type de l'élément).
    h.get_all_by_role(egui::accesskit::Role::ComboBox)
        .nth(1)
        .expect("type de l'élément interne")
        .click();
    h.run_steps(3);
    h.get_by_label("Surpiqûre").click();
    h.run_steps(3);
    assert_eq!(internals_of(&h)[0].kind, pf_core::InternalKind::Topstitch);
    assert!(!h.state().editor.status.error);
    // Une étape : annuler rend le pli.
    undo(&mut h);
    assert_eq!(internals_of(&h)[0].kind, pf_core::InternalKind::Fold);
    // Retrait : l'élément disparaît, la construction reste.
    let objects = h.state().editor.sheet().entities.len();
    h.get_by_label("Retirer").click();
    h.run_steps(3);
    assert!(internals_of(&h).is_empty());
    assert_eq!(h.state().editor.sheet().entities.len(), objects);
    h.get_by_label("Aucun élément interne.");
    undo(&mut h);
    assert_eq!(internals_of(&h).len(), 1);
}

#[test]
fn us043_ajouter_la_selection_grise_si_invalide_et_annulable() {
    let (mut h, fold) = internals_harness();
    let add = "Ajouter la sélection comme élément interne";
    // Rien de sélectionné : grisé.
    assert!(h.get_by_label(add).accesskit_node().is_disabled());
    // Un tracé du contour : grisé (il ne peut pas être aussi interne).
    let edge = piece_of(&h).outline[0].entity;
    h.state_mut().editor.selection.entities = vec![edge];
    h.run_steps(3);
    assert!(h.get_by_label(add).accesskit_node().is_disabled());
    // Le pli : possible ; une étape annulable.
    h.state_mut().editor.selection.entities = vec![fold];
    h.run_steps(3);
    assert!(!h.get_by_label(add).accesskit_node().is_disabled());
    h.get_by_label(add).click();
    h.run_steps(3);
    let internal = internals_of(&h);
    assert_eq!(internal.len(), 1);
    assert_eq!(internal[0].kind, pf_core::InternalKind::Fold);
    h.get_by_label_contains("1 ligne(s) interne(s), 0 repère(s)");
    // Déjà interne : grisé de nouveau.
    h.run_steps(2);
    assert!(h.get_by_label(add).accesskit_node().is_disabled());
    undo(&mut h);
    assert!(internals_of(&h).is_empty());
    h.state().editor.project().validate().unwrap();
}

#[test]
fn us043_la_section_et_le_rapport_sont_visibles_en_anglais() {
    let (mut h, _) = internals_harness();
    pf_i18n::set_lang(pf_i18n::Lang::En);
    h.run_steps(3);
    h.get_by_label("Internal elements");
    h.get_by_label("No internal elements.");
    h.get_by_label("Add the selection as internal element");
    h.get_by_label("Import from construction").click();
    h.run_steps(3);
    h.get_by_label_contains("Last import: 2 added, 5 skipped, 0 already present.");
    h.get_by_label_contains("— not entirely inside the outline");
    assert_eq!(h.get_all_by_label_contains("— outline path").count(), 4);
    h.get_by_label("1 internal line(s), 1 mark(s) (linked to the construction).");
    assert_eq!(h.get_all_by_label("Remove").count(), 2);
    // Le point libre sort du contour quand on le déplace : avertissement en anglais.
    let point = h
        .state()
        .editor
        .sheet()
        .points
        .iter()
        .find(|p| !h.state().editor.sheet().point_is_used(p.id))
        .map(|p| p.id)
        .unwrap();
    {
        let ed = &mut h.state_mut().editor;
        let sheet = ed.sheet;
        ed.exec(pf_core::Command::MovePoint {
            sheet,
            point,
            to: pt(300.0, 300.0),
        })
        .unwrap();
    }
    h.run_steps(3);
    h.get_by_label_contains("1 internal element(s) are no longer entirely inside the outline");
    h.get_by_label_contains("outside the outline");
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
    h.run_steps(3);
    h.get_by_label_contains("ne sont plus entièrement dans le contour");
    assert!(h.state().editor.project().validate().is_ok());
}

// ----- US-04.4 : pièces dérivées --------------------------------------------------

/// Aire d'un polygone convexe décalé de `d` (positif = extérieur), calculée à part :
/// chaque arête est déplacée le long de sa normale, les sommets sont les
/// intersections des droites consécutives.
fn offset_polygon_area(poly: &[(f64, f64)], d: f64) -> f64 {
    let n = poly.len();
    let line = |i: usize| {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let l = dx.hypot(dy);
        ((a.0 + d * dy / l, a.1 - d * dx / l), (dx, dy))
    };
    let v: Vec<(f64, f64)> = (0..n)
        .map(|i| {
            let ((p0, d0), (p1, d1)) = (line((i + n - 1) % n), line(i));
            let t = ((p1.0 - p0.0) * d1.1 - (p1.1 - p0.1) * d1.0) / (d0.0 * d1.1 - d0.1 * d1.0);
            (p0.0 + t * d0.0, p0.1 + t * d0.1)
        })
        .collect();
    (0..n)
        .map(|i| 0.5 * (v[i].0 * v[(i + 1) % n].1 - v[(i + 1) % n].0 * v[i].1))
        .sum::<f64>()
        .abs()
}

/// Texte d'aire tel que l'inspecteur l'écrit (cm², séparateur décimal courant).
fn cm2(area_mm2: f64) -> String {
    format!("{} cm²", pf_core::input::format_number(area_mm2 / 100.0, 2))
}

/// `piece_harness` (rectangle 100 × 60 mm, pièce « Devant ») dans une fenêtre haute.
fn derived_harness() -> Harness<'static, App> {
    let mut h = piece_harness();
    h.set_size(egui::vec2(1280.0, 1600.0));
    h.run_steps(3);
    open_fold(&mut h, "piece.derived.title");
    h
}

/// Crée la dérivée (décalage `d`) de la pièce « Devant » par une commande.
fn make_derived(h: &mut Harness<'_, App>, d: f64) -> (pf_core::PieceId, pf_core::SheetId) {
    let mother = piece_of(h).id;
    let out = h
        .state_mut()
        .editor
        .exec(pf_core::Command::CreateDerivedPiece {
            mother,
            kind: pf_core::DerivedKind::Lining,
            operation: pf_core::DerivedOp::Offset { distance_mm: d },
            name: None,
        })
        .unwrap();
    h.run_steps(3);
    (out.pieces[0], out.sheet.unwrap())
}

fn move_corner(h: &mut Harness<'_, App>, from: (f64, f64), to: (f64, f64)) {
    let ed = &mut h.state_mut().editor;
    let sheet = ed.sheet;
    let point = ed
        .project()
        .sheet(sheet)
        .unwrap()
        .points
        .iter()
        .find(|p| {
            let v = ed.project().sheet(sheet).unwrap().position(p.id).unwrap();
            (v.x - from.0).abs() < 1e-9 && (v.y - from.1).abs() < 1e-9
        })
        .map(|p| p.id)
        .unwrap();
    ed.exec(pf_core::Command::MovePoint {
        sheet,
        point,
        to: pt(to.0, to.1),
    })
    .unwrap();
    h.run_steps(3);
}

#[test]
fn us044_creer_une_derivee_depuis_l_inspecteur() {
    let mut h = derived_harness();
    h.get_by_label("Créer une pièce dérivée…").click();
    h.run_steps(3);
    // Formulaire : type, décalage (champ « -10 » par défaut), nom (indication).
    h.get_by_label("Décalage (mm)");
    h.get_by_label_contains("pas de marge de couture");
    {
        let field = h
            .get_all_by_role(egui::accesskit::Role::TextInput)
            .find(|f| f.value().as_deref() == Some("-10"))
            .expect("champ du décalage");
        field.focus();
    }
    h.run_steps(2);
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.run_steps(1);
    type_text(&mut h, "-5");
    h.run_steps(2);
    // Type : liste déroulante (la 2e : la catégorie de la pièce est la 1re).
    h.get_all_by_role(egui::accesskit::Role::ComboBox)
        .nth(1)
        .expect("type de la dérivée")
        .click();
    h.run_steps(3);
    h.get_by_label("Entoilage").click();
    h.run_steps(3);
    h.get_by_label("Créer la pièce dérivée").click();
    h.run_steps(3);
    assert!(
        !h.state().editor.status.error,
        "{}",
        h.state().editor.status.text
    );
    let project = h.state().editor.project().clone();
    assert_eq!(project.pieces.len(), 2);
    assert_eq!(project.sheets.len(), 2, "feuille dédiée créée");
    let d = &project.pieces[1];
    let spec = d.derived.as_ref().expect("pièce dérivée");
    assert_eq!(spec.kind, pf_core::DerivedKind::Interfacing);
    assert_eq!(
        spec.operation,
        pf_core::DerivedOp::Offset { distance_mm: -5.0 }
    );
    assert_eq!(d.name, "Devant – entoilage");
    assert_eq!(d.category, Some(pf_core::PieceCategory::Interfacing));
    // 100 × 60 décalé de −5 : 90 × 50.
    assert!((project.piece_contour(d.id).unwrap().area_mm2 - 4500.0).abs() < 1e-6);
    // La fiche de la mère liste la dérivée avec son aire observée ; le formulaire est fermé.
    h.get_by_label_contains(&format!(
        "Devant – entoilage — entoilage, -5,0 mm — {}",
        cm2(4500.0)
    ));
    assert!(h.query_by_label("Créer la pièce dérivée").is_none());
    h.get_by_label("Créer une pièce dérivée…");
    // Une seule annulation défait tout : pièce, feuille et géométrie.
    undo(&mut h);
    assert_eq!(h.state().editor.project().pieces.len(), 1);
    assert_eq!(h.state().editor.project().sheets.len(), 1);
    h.state().editor.project().validate().unwrap();
}

#[test]
fn us044_decalage_invalide_message_sans_creation() {
    let mut h = derived_harness();
    h.get_by_label("Créer une pièce dérivée…").click();
    h.run_steps(3);
    let field = h
        .get_all_by_role(egui::accesskit::Role::TextInput)
        .find(|f| f.value().as_deref() == Some("-10"))
        .expect("champ du décalage");
    field.focus();
    h.run_steps(2);
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.run_steps(1);
    type_text(&mut h, "abc");
    h.run_steps(2);
    h.get_by_label("Créer la pièce dérivée").click();
    h.run_steps(3);
    assert!(h.state().editor.status.error);
    assert!(h.state().editor.status.text.contains("Décalage invalide"));
    assert_eq!(h.state().editor.project().pieces.len(), 1);
    // Un décalage que le cœur refuse (la forme disparaît) : message, rien de créé.
    let field = h
        .get_all_by_role(egui::accesskit::Role::TextInput)
        .find(|f| f.value().as_deref() == Some("abc"))
        .expect("champ du décalage");
    field.focus();
    h.run_steps(2);
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.run_steps(1);
    type_text(&mut h, "-40");
    h.run_steps(2);
    h.get_by_label("Créer la pièce dérivée").click();
    h.run_steps(3);
    assert!(h.state().editor.status.error);
    assert_eq!(h.state().editor.project().pieces.len(), 1);
    assert_eq!(h.state().editor.project().sheets.len(), 1);
    // Annuler le formulaire le ferme.
    h.get_by_label("Annuler la création").click();
    h.run_steps(3);
    h.get_by_label("Créer une pièce dérivée…");
}

#[test]
fn us044_deplacer_un_point_de_la_mere_change_l_aire_de_la_derivee() {
    let mut h = derived_harness();
    let (d, _) = make_derived(&mut h, -10.0);
    // 100 × 60 décalé de −10 : 80 × 40.
    h.get_by_label_contains(&format!("doublure, -10,0 mm — {}", cm2(3200.0)));
    // Trapèze : le coin (100 ; 60) passe en (60 ; 60).
    move_corner(&mut h, (100.0, 60.0), (60.0, 60.0));
    let expected = offset_polygon_area(
        &[(0.0, 0.0), (100.0, 0.0), (60.0, 60.0), (0.0, 60.0)],
        -10.0,
    );
    let area = h
        .state()
        .editor
        .project()
        .piece_contour(d)
        .unwrap()
        .area_mm2;
    assert!((area - expected).abs() < 1e-6, "{area} ≠ {expected}");
    assert!((expected - 3200.0).abs() > 100.0, "l'aire a bien changé");
    h.get_by_label_contains(&format!("doublure, -10,0 mm — {}", cm2(expected)));
    assert!(h.query_by_label_contains(&cm2(3200.0)).is_none());
    // Annuler la modification de la mère annule la mise à jour : une seule étape.
    undo(&mut h);
    assert!(
        (h.state()
            .editor
            .project()
            .piece_contour(d)
            .unwrap()
            .area_mm2
            - 3200.0)
            .abs()
            < 1e-6
    );
    h.get_by_label_contains(&format!("doublure, -10,0 mm — {}", cm2(3200.0)));
    h.state().editor.project().validate().unwrap();
}

#[test]
fn us044_inspecteur_de_la_derivee_etat_obsolete_et_casser_le_lien() {
    let mut h = derived_harness();
    let (d, ds) = make_derived(&mut h, -10.0);
    let mother_sheet = h.state().editor.sheet;
    // Feuille de la dérivée : lien, état à jour, bouton.
    h.state_mut().editor.sheet = ds;
    h.state_mut().editor.selection.clear();
    h.run_steps(3);
    h.get_by_label_contains("Dérivée de « Devant » (doublure, -10,0 mm)");
    h.get_by_label_contains("État : à jour");
    h.get_by_label_contains("cassez le lien pour la modifier");
    // La mère se recoupe : la dérivée devient obsolète, avec libellé, ⚠ et raison.
    h.state_mut().editor.sheet = mother_sheet;
    move_corner(&mut h, (100.0, 60.0), (-50.0, 60.0));
    h.state_mut().editor.sheet = ds;
    h.run_steps(3);
    h.get_by_label_contains("État : ⚠ obsolète — ");
    assert!(h.get_all_by_label_contains("se recoupe").count() >= 1);
    h.get_by_label_contains("pièce dérivée obsolète");
    assert!(h.state().editor.project().validate().is_ok());
    // Casser le lien : une commande, annulable.
    let before = h.state().editor.project().clone();
    h.get_by_label("Casser le lien").click();
    h.run_steps(3);
    assert!(h
        .state()
        .editor
        .project()
        .piece(d)
        .unwrap()
        .derived
        .is_none());
    assert!(h.query_by_label_contains("Dérivée de").is_none());
    assert!(h.query_by_label_contains("État : ").is_none());
    h.get_by_label("Créer une pièce dérivée…");
    undo(&mut h);
    assert_eq!(*h.state().editor.project(), before);
    h.get_by_label_contains("État : ⚠ obsolète — ");
}

#[test]
fn us044_la_derivee_ne_suit_plus_apres_avoir_casse_le_lien() {
    let mut h = derived_harness();
    let (d, ds) = make_derived(&mut h, -10.0);
    let mother_sheet = h.state().editor.sheet;
    h.state_mut().editor.sheet = ds;
    h.run_steps(3);
    h.get_by_label("Casser le lien").click();
    h.run_steps(3);
    h.state_mut().editor.sheet = mother_sheet;
    move_corner(&mut h, (100.0, 60.0), (60.0, 60.0));
    let area = h
        .state()
        .editor
        .project()
        .piece_contour(d)
        .unwrap()
        .area_mm2;
    assert!(
        (area - 3200.0).abs() < 1e-6,
        "la dérivée ne bouge plus : {area}"
    );
}

#[test]
fn us044_l_inspecteur_des_pieces_derivees_fonctionne_en_anglais() {
    let mut h = derived_harness();
    let (_, ds) = make_derived(&mut h, -10.0);
    let mother_sheet = h.state().editor.sheet;
    pf_i18n::set_lang(pf_i18n::Lang::En);
    h.run_steps(3);
    h.get_by_label("Create a derived piece…");
    h.get_by_label("Derived pieces");
    h.get_by_label_contains("— lining, -10");
    h.state_mut().editor.sheet = ds;
    h.run_steps(3);
    h.get_by_label_contains("Derived from \"Devant\" (lining,");
    h.get_by_label_contains("Status: up to date");
    h.get_by_label("Break the link");
    h.get_by_label_contains("break the link to edit it");
    // Obsolète, en anglais.
    h.state_mut().editor.sheet = mother_sheet;
    move_corner(&mut h, (100.0, 60.0), (-50.0, 60.0));
    h.state_mut().editor.sheet = ds;
    h.run_steps(3);
    h.get_by_label_contains("Status: ⚠ out of date — ");
    h.get_by_label_contains("derived piece out of date");
    // Formulaire en anglais.
    h.state_mut().editor.sheet = mother_sheet;
    h.run_steps(3);
    h.get_by_label("Create a derived piece…").click();
    h.run_steps(3);
    h.get_by_label("Offset (mm)");
    h.get_by_label("Create the derived piece");
    h.get_by_label("Cancel creation");
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
    h.run_steps(3);
    h.get_by_label("Décalage (mm)");
}

// ----- US-04.5 : plan de pièces ---------------------------------------------------

/// Trois pièces liées : « Devant » (100 × 60, 60 cm²), « Dos » (80 × 50, 40 cm²) et
/// « Manche » (60 × 40, 24 cm²), l'ordre alphabétique est l'inverse de l'ordre des aires.
fn plan_harness() -> Harness<'static, App> {
    let mut h = harness();
    {
        let ed = &mut h.state_mut().editor;
        ed.grid_snap_mm = None;
        let sheet = ed.sheet;
        for (name, x, w, hgt) in [
            ("Devant", 0.0, 100.0, 60.0),
            ("Dos", 200.0, 80.0, 50.0),
            ("Manche", 400.0, 60.0, 40.0),
        ] {
            ed.exec(pf_core::Command::AddRectangle {
                sheet,
                corner: pf_core::PointRef::New(pt(x, 0.0)),
                width_mm: w,
                height_mm: hgt,
            })
            .unwrap();
            ed.exec(pf_core::Command::ExtractPiece {
                sheet,
                source: pf_core::PieceSource::Click {
                    at: pt(x + w / 2.0, hgt / 2.0),
                },
                name: name.into(),
                detached: false,
            })
            .unwrap();
        }
        ed.selection.clear();
    }
    h.run_steps(3);
    h
}

fn open_plan(h: &mut Harness<'_, App>) {
    h.get_by_label("Affichage").click();
    h.run_steps(3);
    h.get_by_label("Plan de pièces").click();
    h.run_steps(4);
}

fn plan_item(h: &Harness<'_, App>, name: &str) -> pf_core::PlanItem {
    h.state()
        .plan
        .as_ref()
        .expect("plan ouvert")
        .snapshot
        .items
        .iter()
        .find(|i| i.name == name)
        .unwrap_or_else(|| panic!("pièce {name}"))
        .clone()
}

fn plan_centre(h: &Harness<'_, App>, name: &str) -> PointMm {
    let bb = plan_item(h, name).bbox.expect("pièce dessinée");
    pt((bb.min.x + bb.max.x) / 2.0, (bb.min.y + bb.max.y) / 2.0)
}

/// Bouton d'en-tête de la liste (le libellé porte « ▲ » ou « ▼ » sur la colonne triée) :
/// les mêmes mots existent ailleurs (fiche de pièce), d'où le filtre sur le rôle.
fn plan_header<'a>(h: &'a Harness<'_, App>, text: &str) -> egui_kittest::Node<'a> {
    h.get_all_by_role(egui::accesskit::Role::Button)
        .find(|n| {
            n.accesskit_node()
                .label()
                .is_some_and(|l| l == text || l.starts_with(&format!("{text} ")))
        })
        .unwrap_or_else(|| panic!("en-tête {text}"))
}

fn plan_names(h: &Harness<'_, App>) -> Vec<String> {
    h.state()
        .plan_sorted_rows()
        .into_iter()
        .map(|r| r.name)
        .collect()
}

/// Octets du projet hors du champ `placement` des pièces.
fn without_placements(h: &Harness<'_, App>) -> String {
    let mut v = serde_json::to_value(h.state().editor.project()).unwrap();
    for piece in v["pieces"].as_array_mut().unwrap() {
        piece.as_object_mut().unwrap().remove("placement");
    }
    v.to_string()
}

fn placement_of(h: &Harness<'_, App>, name: &str) -> Option<pf_core::Placement> {
    h.state()
        .editor
        .project()
        .pieces
        .iter()
        .find(|p| p.name == name)
        .unwrap()
        .placement
}

/// Glisse la pièce d'un point du plan à un autre (souris), par petites étapes.
fn drag_in_plan(h: &mut Harness<'_, App>, from: PointMm, to: PointMm, release: bool) {
    move_to(h, from);
    button(h, from, true);
    for k in 1..=4 {
        let t = f64::from(k) / 4.0;
        move_to(
            h,
            pt(from.x + (to.x - from.x) * t, from.y + (to.y - from.y) * t),
        );
    }
    if release {
        button(h, to, false);
        h.step();
    }
}

#[test]
fn us045_ouvrir_le_plan_liste_et_dessine_toutes_les_pieces_sans_rien_ecrire() {
    let mut h = plan_harness();
    let before = serde_json::to_string(h.state().editor.project()).unwrap();
    open_plan(&mut h);
    assert!(h.state().plan.is_some());
    h.get_by_label_contains("Mise en page : déplacer ou tourner une pièce ne modifie pas");
    h.get_by_label("Retour à la feuille");
    h.get_by_label("Tout voir");
    h.get_by_label("Liste des pièces");
    // Les trois pièces sont dans la liste, dessinées, placées automatiquement.
    for name in ["Devant", "Dos", "Manche"] {
        h.get_by_label(name);
        let item = plan_item(&h, name);
        assert!(!item.manual && item.skipped.is_none() && item.outline.len() >= 4);
    }
    h.get_by_label("3 pièce(s), 0 placée(s) à la main.");
    assert!(h
        .state()
        .editor
        .project()
        .pieces
        .iter()
        .all(|p| p.placement.is_none()));
    // Ouvrir le plan n'a rien écrit.
    assert_eq!(
        serde_json::to_string(h.state().editor.project()).unwrap(),
        before
    );
    // Placement automatique sans chevauchement, aucun avertissement de recouvrement.
    assert!(h
        .state()
        .plan
        .as_ref()
        .unwrap()
        .snapshot
        .overlaps
        .is_empty());
    // « Retour à la feuille » rend la feuille et sa vue.
    h.get_by_label("Retour à la feuille").click();
    h.run_steps(3);
    assert!(h.state().plan.is_none());
    h.get_by_label_contains("pièce(s) sur cette feuille");
}

#[test]
fn us045_liste_triable_un_clic_sur_l_en_tete_inverse_l_ordre() {
    let mut h = plan_harness();
    open_plan(&mut h);
    // Tri par défaut : nom croissant.
    assert_eq!(plan_names(&h), ["Devant", "Dos", "Manche"]);
    let y = |h: &Harness<'_, App>, name: &str| h.get_by_label(name).rect().min.y;
    assert!(y(&h, "Devant") < y(&h, "Dos") && y(&h, "Dos") < y(&h, "Manche"));
    // Aire croissante : ordre opposé à l'ordre alphabétique.
    plan_header(&h, "Aire (cm²)").click();
    h.run_steps(3);
    assert_eq!(plan_names(&h), ["Manche", "Dos", "Devant"]);
    assert!(y(&h, "Manche") < y(&h, "Dos") && y(&h, "Dos") < y(&h, "Devant"));
    h.get_by_label("Aire (cm²) ^");
    // Deuxième clic : décroissant (deux tris opposés).
    plan_header(&h, "Aire (cm²)").click();
    h.run_steps(3);
    assert_eq!(plan_names(&h), ["Devant", "Dos", "Manche"]);
    assert!(y(&h, "Devant") < y(&h, "Manche"));
    h.get_by_label("Aire (cm²) v");
    // Autres colonnes : le tri repart en croissant.
    plan_header(&h, "Qté").click();
    h.run_steps(3);
    h.get_by_label("Qté ^");
    plan_header(&h, "Périmètre").click();
    h.run_steps(3);
    assert_eq!(plan_names(&h), ["Manche", "Dos", "Devant"]);
    // Le tri est un état d'interface : le projet n'a pas changé.
    assert!(h
        .state()
        .editor
        .project()
        .pieces
        .iter()
        .all(|p| p.placement.is_none()));
    // Clic sur une ligne : sélection (et cadrage).
    h.get_by_label("Dos").click();
    h.run_steps(3);
    let selected = h.state().plan.as_ref().unwrap().selected;
    let dos = h
        .state()
        .editor
        .project()
        .pieces
        .iter()
        .find(|p| p.name == "Dos")
        .unwrap()
        .id;
    assert_eq!(selected, Some(dos));
    h.get_by_label("Pièce sélectionnée : Dos");
}

#[test]
fn us045_glisser_une_piece_est_une_etape_annulable_et_la_construction_ne_change_pas() {
    let mut h = plan_harness();
    open_plan(&mut h);
    let before_all = serde_json::to_string(h.state().editor.project()).unwrap();
    let construction = without_placements(&h);
    let start = plan_centre(&h, "Devant");
    let target = pt(start.x + 300.0, start.y - 150.0);
    drag_in_plan(&mut h, start, target, true);
    h.run_steps(3);
    let p = placement_of(&h, "Devant").expect("placement manuel enregistré");
    let moved = plan_centre(&h, "Devant");
    assert!(
        (moved.x - target.x).abs() < 1.5 && (moved.y - target.y).abs() < 1.5,
        "la pièce suit la souris : {moved:?} vers {target:?}"
    );
    assert!((p.x_mm - moved.x).abs() < 1e-6 && p.rotation_deg == 0.0);
    // Construction, fiches, aire, périmètre : inchangés à l'octet près hors `placement`.
    assert_eq!(without_placements(&h), construction);
    let item = plan_item(&h, "Devant");
    assert!(item.manual);
    assert!((item.area_mm2.unwrap() - 6000.0).abs() < 1e-9);
    assert!((item.perimeter_mm.unwrap() - 320.0).abs() < 1e-9);
    // Les autres pièces n'ont pas bougé.
    assert!(placement_of(&h, "Dos").is_none() && placement_of(&h, "Manche").is_none());
    h.get_by_label("3 pièce(s), 1 placée(s) à la main.");
    // Un seul Ctrl+Z défait tout le glisser.
    undo(&mut h);
    assert_eq!(
        serde_json::to_string(h.state().editor.project()).unwrap(),
        before_all
    );
    assert!(placement_of(&h, "Devant").is_none());
    // Rétablir : le même placement exactement.
    h.key_press_modifiers(Modifiers::COMMAND, Key::Y);
    h.run_steps(3);
    assert_eq!(placement_of(&h, "Devant"), Some(p));
    h.state().editor.project().validate().unwrap();
}

#[test]
fn us045_echap_annule_un_glisser_en_cours_sans_trace() {
    let mut h = plan_harness();
    open_plan(&mut h);
    let before = serde_json::to_string(h.state().editor.project()).unwrap();
    let start = plan_centre(&h, "Dos");
    drag_in_plan(&mut h, start, pt(start.x + 120.0, start.y + 80.0), false);
    // En cours de glisser : la pièce a bougé à l'écran et dans le projet.
    assert!(placement_of(&h, "Dos").is_some());
    key(&mut h, Key::Escape);
    assert_eq!(
        serde_json::to_string(h.state().editor.project()).unwrap(),
        before,
        "Échap annule sans trace"
    );
    button(&mut h, pt(start.x + 120.0, start.y + 80.0), false);
    h.run_steps(3);
    assert_eq!(
        serde_json::to_string(h.state().editor.project()).unwrap(),
        before
    );
    assert!(h.state().plan.as_ref().unwrap().drag.is_none());
    // Aucune étape d'annulation n'a été créée : un undo défait la dernière extraction.
    undo(&mut h);
    assert_eq!(h.state().editor.project().pieces.len(), 2);
}

#[test]
fn us045_clic_droit_annule_un_glisser_en_cours_sans_trace() {
    let mut h = plan_harness();
    open_plan(&mut h);
    let before = serde_json::to_string(h.state().editor.project()).unwrap();
    let start = plan_centre(&h, "Manche");
    drag_in_plan(&mut h, start, pt(start.x - 90.0, start.y + 60.0), false);
    assert!(placement_of(&h, "Manche").is_some());
    let pos = screen(&h, start);
    for pressed in [true, false] {
        h.input_mut().events.push(Event::PointerButton {
            pos,
            button: PointerButton::Secondary,
            pressed,
            modifiers: Modifiers::NONE,
        });
        h.step();
    }
    h.run_steps(2);
    assert_eq!(
        serde_json::to_string(h.state().editor.project()).unwrap(),
        before
    );
}

#[test]
fn us045_rotation_par_champ_et_par_boutons_une_etape_chacune() {
    let mut h = plan_harness();
    open_plan(&mut h);
    h.get_by_label("Dos").click();
    h.run_steps(3);
    h.get_by_label("Placement : automatique (calculé, non enregistré)");
    let construction = without_placements(&h);
    let area = plan_item(&h, "Dos").area_mm2.unwrap();
    // Champ « Rotation » : dernier champ de texte du panneau (X, Y, rotation).
    {
        let fields: Vec<_> = h
            .get_all_by_role(egui::accesskit::Role::TextInput)
            .collect();
        fields.last().expect("champ rotation").focus();
    }
    h.run_steps(2);
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.run_steps(1);
    type_text(&mut h, "30");
    key(&mut h, Key::Enter);
    h.run_steps(3);
    let p = placement_of(&h, "Dos").expect("placement manuel");
    assert_eq!(p.rotation_deg, 30.0);
    h.get_by_label("Placement : manuel (enregistré dans le projet)");
    // Boutons : +90° puis −15°, une étape chacun.
    h.get_by_label("+90°").click();
    h.run_steps(3);
    assert_eq!(placement_of(&h, "Dos").unwrap().rotation_deg, 120.0);
    h.get_by_label("−15°").click();
    h.run_steps(3);
    assert_eq!(placement_of(&h, "Dos").unwrap().rotation_deg, 105.0);
    h.get_by_label("−90°").click();
    h.run_steps(3);
    h.get_by_label("−90°").click();
    h.run_steps(3);
    assert_eq!(
        placement_of(&h, "Dos").unwrap().rotation_deg,
        285.0,
        "normalisée dans [0 ; 360["
    );
    // Aire, périmètre, construction inchangés ; la boîte du plan, elle, a tourné.
    assert_eq!(without_placements(&h), construction);
    assert!((plan_item(&h, "Dos").area_mm2.unwrap() - area).abs() < 1e-9);
    // Chaque saisie et chaque bouton = une étape : on remonte 285 → 15 → 105 → 120 → 30,
    // puis une dernière étape ramène au placement automatique.
    for expected in [Some(15.0), Some(105.0), Some(120.0), Some(30.0), None] {
        undo(&mut h);
        assert_eq!(placement_of(&h, "Dos").map(|p| p.rotation_deg), expected);
    }
    h.state().editor.project().validate().unwrap();
}

#[test]
fn us045_replacer_automatiquement_remet_le_placement_calcule_en_une_etape() {
    let mut h = plan_harness();
    open_plan(&mut h);
    let auto_centre = plan_centre(&h, "Devant");
    let start = auto_centre;
    drag_in_plan(&mut h, start, pt(start.x + 500.0, start.y + 200.0), true);
    h.run_steps(3);
    assert!(placement_of(&h, "Devant").is_some());
    let manual = placement_of(&h, "Devant").unwrap();
    h.get_by_label("Replacer automatiquement").click();
    h.run_steps(3);
    assert!(placement_of(&h, "Devant").is_none());
    let back = plan_centre(&h, "Devant");
    assert!((back.x - auto_centre.x).abs() < 1e-9 && (back.y - auto_centre.y).abs() < 1e-9);
    assert!(!plan_item(&h, "Devant").manual);
    // Le bouton est désactivé tant que la pièce est en placement automatique.
    // Une seule étape : un undo revient au placement manuel.
    undo(&mut h);
    assert_eq!(placement_of(&h, "Devant"), Some(manual));
}

#[test]
fn us045_avertissement_de_piece_visible_en_texte_et_symbole() {
    let mut h = plan_harness();
    open_plan(&mut h);
    // Aucun droit-fil : « ⚠ » dans la liste (symbole + nombre), jamais la couleur seule.
    assert!(h.get_all_by_label_contains("⚠").count() >= 3);
    h.get_by_label("Dos").click();
    h.run_steps(3);
    // Le panneau du plan le dit (la fiche de la feuille est masquée en mode plan, PH-2b).
    assert!(
        h.get_all_by_label_contains("⚠ Avertissement : droit-fil manquant")
            .count()
            >= 1
    );
    // Deux pièces au même endroit : l'information de recouvrement des boîtes s'affiche.
    let p = placement_of(&h, "Dos");
    assert!(p.is_none());
    let target = {
        let b = plan_centre(&h, "Devant");
        pt(b.x, b.y)
    };
    let start = plan_centre(&h, "Dos");
    drag_in_plan(&mut h, start, target, true);
    h.run_steps(3);
    assert!(!h
        .state()
        .plan
        .as_ref()
        .unwrap()
        .snapshot
        .overlaps
        .is_empty());
    h.get_by_label_contains("boîtes englobantes se recouvrent");
    h.get_by_label_contains("Ce n'est pas une détection exacte des recouvrements.");
    // L'information n'a rien refusé : la pièce est bien placée là.
    assert!(placement_of(&h, "Dos").is_some());
}

#[test]
fn us045_les_raccourcis_de_la_feuille_sont_inactifs_dans_le_plan() {
    let mut h = plan_harness();
    let before_entities = h.state().editor.sheet().entities.len();
    {
        let ed = &mut h.state_mut().editor;
        ed.selection.entities = ed.sheet().entities.iter().map(|e| e.id).collect();
    }
    open_plan(&mut h);
    key(&mut h, Key::Delete);
    key(&mut h, Key::S);
    assert_eq!(h.state().editor.sheet().entities.len(), before_entities);
    assert_eq!(h.state().editor.tool, Tool::Select);
    // Échap désélectionne la pièce, sans quitter le plan.
    h.get_by_label("Devant").click();
    h.run_steps(3);
    assert!(h.state().plan.as_ref().unwrap().selected.is_some());
    key(&mut h, Key::Escape);
    assert!(h.state().plan.as_ref().unwrap().selected.is_none());
    assert!(h.state().plan.is_some());
}

#[test]
fn us045_le_plan_fonctionne_en_anglais() {
    let mut h = plan_harness();
    pf_i18n::set_lang(pf_i18n::Lang::En);
    h.run_steps(3);
    h.get_by_label("View").click();
    h.run_steps(3);
    h.get_by_label("Pieces plan").click();
    h.run_steps(4);
    h.get_by_label_contains("Layout: moving or rotating a piece does not change the construction.");
    h.get_by_label("Back to the sheet");
    h.get_by_label("Fit all");
    h.get_by_label("Pieces list");
    h.get_by_label("3 piece(s), 0 placed by hand.");
    h.get_by_label_contains("Area (cm²)").click();
    h.run_steps(3);
    h.get_by_label("Area (cm²) ^");
    h.get_by_label("Placed by hand");
    h.get_by_label("Dos").click();
    h.run_steps(3);
    h.get_by_label("Selected piece: Dos");
    h.get_by_label("Placement: automatic (computed, not saved)");
    h.get_by_label("Reset to automatic placement");
    h.get_by_label("Center X (mm)");
    h.get_by_label("Rotation (°)");
    assert!(
        h.get_all_by_label_contains("⚠ Warning: grainline missing")
            .count()
            >= 1
    );
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
    h.run_steps(3);
    h.get_by_label("Retour à la feuille");
}

// ----- US-04.6 : séparer / assembler -------------------------------------------

/// Tracé du contour de `piece` qui relie `a` et `b` (dans un sens ou l'autre).
fn edge_at(
    h: &Harness<'_, App>,
    piece: pf_core::PieceId,
    a: PointMm,
    b: PointMm,
) -> pf_core::EntityId {
    let project = h.state().editor.project();
    let sheet = project.piece_sheet(piece).unwrap();
    let near = |p: PointMm, q: PointMm| pf_geometry::distance_mm(p, q) < 1e-6;
    project
        .piece(piece)
        .unwrap()
        .outline
        .iter()
        .find(|e| {
            let (s, t) = sheet.segment_ends(e.entity).unwrap();
            (near(s, a) && near(t, b)) || (near(s, b) && near(t, a))
        })
        .expect("bord")
        .entity
}

/// Deuxième pièce « Fond » : rectangle `w × 30` en (200, 0), extrait par un clic.
fn add_second_piece(h: &mut Harness<'_, App>, w: f64) -> pf_core::PieceId {
    let ed = &mut h.state_mut().editor;
    let sheet = ed.sheet;
    ed.exec(pf_core::Command::AddRectangle {
        sheet,
        corner: pf_core::PointRef::New(pt(200.0, 0.0)),
        width_mm: w,
        height_mm: 30.0,
    })
    .unwrap();
    let id = ed
        .exec(pf_core::Command::ExtractPiece {
            sheet,
            source: pf_core::PieceSource::Click {
                at: pt(200.0 + w / 2.0, 15.0),
            },
            name: "Fond".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    h.run_steps(3);
    id
}

#[test]
fn us046_separer_par_deux_clics_apercu_puis_une_etape_annulable() {
    let mut h = derived_harness();
    let devant = piece_of(&h).id;
    h.get_by_label("Séparer…").click();
    h.run_steps(3);
    assert_eq!(
        h.state().editor.split_job.map(|j| j.piece),
        Some(devant),
        "ligne de coupe en attente"
    );
    // Premier point posé, puis aperçu sous le curseur : aires observées des deux moitiés.
    click(&mut h, pt(30.0, 5.0));
    assert_eq!(
        h.state().editor.project().pieces.len(),
        1,
        "rien avant le 2e clic"
    );
    move_to(&mut h, pt(30.0, 55.0));
    h.run_steps(2);
    let draft = h.state().editor.split_draft().expect("aperçu de la coupe");
    let pre = draft.preview.expect("coupe possible");
    // Le clic passe par des pixels : la droite est à 30 mm près d'un dixième de pixel.
    assert!(
        (pre.left.area_mm2 - 1800.0).abs() < 30.0 && (pre.right.area_mm2 - 4200.0).abs() < 30.0,
        "{pre:?}"
    );
    assert!((pre.left.area_mm2 + pre.right.area_mm2 - 6000.0).abs() < 1e-6);
    // Le canevas dessine l'aperçu sans paniquer ; le texte double la couleur.
    h.run_steps(2);
    click(&mut h, pt(30.0, 55.0));
    let ed = &h.state().editor;
    assert!(!ed.status.error, "{}", ed.status.text);
    assert!(ed.split_job.is_none());
    let names: Vec<&str> = ed
        .project()
        .pieces
        .iter()
        .map(|p| p.name.as_str())
        .collect();
    assert_eq!(names, ["Devant (1/2)", "Devant (2/2)"]);
    assert!(
        ed.status.text.contains("Devant (1/2)") && ed.status.text.contains("cm²"),
        "{}",
        ed.status.text
    );
    ed.project().validate().unwrap();
    // Une seule annulation rend la pièce d'origine.
    undo(&mut h);
    let ed = &h.state().editor;
    assert_eq!(ed.project().pieces.len(), 1);
    assert_eq!(ed.project().pieces[0].name, "Devant");
    assert_eq!(ed.project().sheets.len(), 1);
}

#[test]
fn us046_separer_une_ligne_qui_ne_coupe_pas_affiche_l_erreur_sans_trace() {
    let mut h = derived_harness();
    let before = h.state().editor.project().clone();
    let devant = piece_of(&h).id;
    h.state_mut().editor.begin_split(devant);
    h.run_steps(2);
    {
        let ed = &mut h.state_mut().editor;
        ed.click(pt(10.0, 100.0), 1.0, false);
        ed.click(pt(50.0, 100.0), 1.0, false);
        assert!(ed.status.error);
        assert!(
            ed.status.text.contains("ne traverse pas"),
            "{}",
            ed.status.text
        );
        assert_eq!(*ed.project(), before, "refus sans trace");
        assert_eq!(
            ed.split_job.map(|j| j.first),
            Some(None),
            "on repart du premier point"
        );
        // Deux points confondus : message propre, pas de commande.
        ed.click(pt(10.0, 10.0), 1.0, false);
        ed.click(pt(10.0, 10.0), 1.0, false);
        assert!(
            ed.status.error && ed.status.text.contains("confondus"),
            "{}",
            ed.status.text
        );
        assert_eq!(*ed.project(), before);
    }
    h.run_steps(3);
    // Échap abandonne la séparation.
    key(&mut h, Key::Escape);
    assert!(h.state().editor.split_job.is_none());
    assert_eq!(*h.state().editor.project(), before);
}

#[test]
fn us046_assembler_ecart_affiche_refus_au_dela_de_la_tolerance_puis_une_etape() {
    let mut h = derived_harness();
    let devant = piece_of(&h).id;
    let fond = add_second_piece(&mut h, 100.3);
    let ea = edge_at(&h, devant, pt(100.0, 60.0), pt(0.0, 60.0));
    let eb = edge_at(&h, fond, pt(200.0, 0.0), pt(300.3, 0.0));
    h.state_mut().open_assemble(Some(devant));
    {
        let d = h.state_mut().assemble.as_mut().expect("fenêtre ouverte");
        d.first_edge = Some(ea);
        d.second = Some(fond);
        d.second_edge = Some(eb);
        d.tolerance_mm = 0.1;
    }
    h.run_steps(4);
    // Écart de 0,3 mm affiché, avec la tolérance ; assemblage refusé.
    h.get_by_label_contains("Écart de longueur : 0,3 mm (tolérance 0,1 mm)");
    h.get_by_label_contains("Assemblage impossible");
    h.get_by_label_contains("Longueur des bords : 100,0 mm et 100,3 mm.");
    assert_eq!(h.state().editor.project().pieces.len(), 2);
    // Tolérance suffisante : plus de refus, aire du résultat annoncée.
    h.state_mut().assemble.as_mut().unwrap().tolerance_mm = 0.5;
    h.run_steps(4);
    assert!(h.query_by_label_contains("Assemblage impossible").is_none());
    h.get_by_label_contains("Pièce assemblée : aire");
    h.get_by_label("Assembler").click();
    h.run_steps(4);
    {
        let ed = &h.state().editor;
        assert!(!ed.status.error, "{}", ed.status.text);
        assert_eq!(ed.project().pieces.len(), 1);
        assert_eq!(ed.project().pieces[0].name, "Devant + Fond");
        assert!(
            ed.status.text.contains("Écart de longueur"),
            "{}",
            ed.status.text
        );
        ed.project().validate().unwrap();
    }
    assert!(h.state().assemble.is_none(), "fenêtre fermée");
    // Une seule annulation rend les deux pièces.
    undo(&mut h);
    assert_eq!(h.state().editor.project().pieces.len(), 2);
}

#[test]
fn us046_separer_et_assembler_fonctionnent_en_anglais() {
    let mut h = derived_harness();
    pf_i18n::set_lang(pf_i18n::Lang::En);
    h.run_steps(3);
    let devant = piece_of(&h).id;
    h.get_by_label("Split…");
    h.get_by_label("Join…");
    h.state_mut().editor.begin_split(devant);
    h.run_steps(2);
    assert!(h
        .state()
        .editor
        .status
        .text
        .contains("first point of the cutting line"));
    h.state_mut().editor.escape();
    let fond = add_second_piece(&mut h, 100.0);
    // Menu Édition : les deux entrées, sans raccourci.
    h.get_by_label("Edit").click();
    h.run_steps(3);
    h.get_by_label_contains("Split a piece along a line");
    h.get_by_label_contains("Join two pieces…");
    h.key_press(Key::Escape);
    h.run_steps(3);
    let ea = edge_at(&h, devant, pt(100.0, 60.0), pt(0.0, 60.0));
    let eb = edge_at(&h, fond, pt(200.0, 0.0), pt(300.0, 0.0));
    h.state_mut().open_assemble(Some(devant));
    {
        let d = h.state_mut().assemble.as_mut().unwrap();
        d.first_edge = Some(ea);
        d.second = Some(fond);
        d.second_edge = Some(eb);
    }
    h.run_steps(4);
    h.get_by_label("Join two pieces");
    h.get_by_label_contains("Length gap:");
    h.get_by_label_contains("Joined piece: area");
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
    h.run_steps(3);
    h.get_by_label_contains("Écart de longueur");
}

// ----- US-04.7 : déplier -------------------------------------------------------

#[test]
fn us047_deplier_apercu_mesures_puis_une_etape_annulable() {
    let mut h = derived_harness();
    let devant = piece_of(&h).id;
    let left = edge_at(&h, devant, pt(0.0, 60.0), pt(0.0, 0.0));
    h.get_by_label("Déplier…").click();
    h.run_steps(3);
    assert!(
        h.state().unfold.is_some(),
        "fenêtre ouverte depuis la fiche"
    );
    h.state_mut().unfold.as_mut().unwrap().fold = Some(left);
    h.run_steps(4);
    // Mesures observées de la moitié et de la pièce entière (calculées à la main :
    // 100 × 60 = 60 cm², pliure 60 mm ; entière 200 × 60 = 120 cm², 2 × (320 − 60) = 520 mm).
    h.get_by_label_contains("Moitié : aire 60,0 cm², périmètre 320,0 mm, pliure 60,0 mm");
    h.get_by_label_contains("« Devant – dépliée » : aire 120,0 cm², périmètre 520,0 mm");
    h.get_by_label_contains("reste liée à la source");
    h.get_by_label_contains("Trait plein : la moitié.");
    assert_eq!(
        h.state().editor.project().pieces.len(),
        1,
        "aperçu sans trace"
    );
    h.get_by_label("Déplier").click();
    h.run_steps(4);
    {
        let ed = &h.state().editor;
        assert!(!ed.status.error, "{}", ed.status.text);
        assert!(
            ed.status.text.contains("Devant – dépliée") && ed.status.text.contains("120,0 cm²"),
            "{}",
            ed.status.text
        );
        assert_eq!(ed.project().pieces.len(), 2);
        let u = &ed.project().pieces[1];
        assert_eq!(
            u.derived.as_ref().map(|d| d.operation),
            Some(pf_core::DerivedOp::Unfold { fold: left })
        );
        assert!((ed.project().piece_contour(u.id).unwrap().area_mm2 - 12_000.0).abs() < 1e-6);
        ed.project().validate().unwrap();
    }
    assert!(h.state().unfold.is_none(), "fenêtre fermée");
    // Une seule annulation retire la pièce dépliée et sa feuille.
    undo(&mut h);
    let ed = &h.state().editor;
    assert_eq!(ed.project().pieces.len(), 1);
    assert_eq!(ed.project().sheets.len(), 1);
}

#[test]
fn us047_pliure_deja_depliee_affiche_le_refus_sans_trace() {
    let mut h = derived_harness();
    let devant = piece_of(&h).id;
    let left = edge_at(&h, devant, pt(0.0, 60.0), pt(0.0, 0.0));
    h.state_mut()
        .editor
        .exec(pf_core::Command::UnfoldPiece {
            piece: devant,
            fold: left,
            name: None,
        })
        .unwrap();
    h.run_steps(3);
    let before = h.state().editor.project().clone();
    h.state_mut().open_unfold(Some(devant));
    h.state_mut().unfold.as_mut().unwrap().fold = Some(left);
    h.run_steps(4);
    h.get_by_label_contains("Dépliage impossible");
    h.get_by_label_contains("déjà dépliée");
    assert_eq!(*h.state().editor.project(), before, "aperçu sans trace");
    // Le bouton est inactif : cliquer ne crée rien.
    h.get_by_label("Déplier").click();
    h.run_steps(3);
    assert_eq!(*h.state().editor.project(), before);
    assert!(h.state().unfold.is_some(), "la fenêtre reste ouverte");
    // Une pliure non dépliée (le bord du bas) est acceptée : l'aperçu l'annonce.
    let bottom = edge_at(&h, devant, pt(0.0, 0.0), pt(100.0, 0.0));
    h.state_mut().unfold.as_mut().unwrap().fold = Some(bottom);
    h.run_steps(4);
    assert!(h.query_by_label_contains("Dépliage impossible").is_none());
    h.get_by_label_contains("aire 120,0 cm², périmètre 440,0 mm");
}

#[test]
fn us047_menu_edition_et_fenetre_fonctionnent_en_anglais() {
    let mut h = derived_harness();
    pf_i18n::set_lang(pf_i18n::Lang::En);
    h.run_steps(3);
    let devant = piece_of(&h).id;
    h.get_by_label("Unfold…");
    h.get_by_label("Edit").click();
    h.run_steps(3);
    h.get_by_label_contains("Unfold a piece on its fold…")
        .click();
    h.run_steps(3);
    assert!(h.state().unfold.is_some());
    h.get_by_label("Unfold a piece on its fold");
    h.get_by_label_contains("Choose a piece and a segment");
    h.state_mut().unfold.as_mut().unwrap().piece = Some(devant);
    let left = edge_at(&h, devant, pt(0.0, 60.0), pt(0.0, 0.0));
    h.state_mut().unfold.as_mut().unwrap().fold = Some(left);
    h.run_steps(4);
    h.get_by_label_contains("Half: area 60.0 cm²");
    h.get_by_label_contains("\"Devant – dépliée\": area 120.0 cm²");
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
    h.run_steps(3);
    h.get_by_label_contains("Moitié : aire 60,0 cm²");
}

#[test]
fn us047_l_inspecteur_de_la_depliee_montre_le_lien_et_l_etat() {
    let mut h = derived_harness();
    let devant = piece_of(&h).id;
    let left = edge_at(&h, devant, pt(0.0, 60.0), pt(0.0, 0.0));
    let out = h
        .state_mut()
        .editor
        .exec(pf_core::Command::UnfoldPiece {
            piece: devant,
            fold: left,
            name: None,
        })
        .unwrap();
    let (u, sheet) = (out.pieces[0], out.sheet.unwrap());
    h.state_mut().editor.sheet = sheet;
    h.state_mut().editor.sync();
    h.run_steps(4);
    h.get_by_label_contains("Dépliée de « Devant »");
    h.get_by_label_contains("État : à jour");
    // La moitié liste sa pièce dépliée, sans distance de décalage.
    h.state_mut().editor.sheet = h.state().editor.project().sheets[0].id;
    h.state_mut().editor.sync();
    h.run_steps(4);
    h.get_by_label_contains("Devant – dépliée — dépliée — 120,0 cm²");
    let _ = u;
}

// ----- US-06.3 : perçages et pointages -----------------------------------------------

/// `piece_harness` (rectangle de 100 × 60 mm, pièce « Devant ») dans une fenêtre haute
/// (l'inspecteur défile) et quatre points libres : dedans (50 ; 30), dehors (150 ; 30),
/// pointe (10 ; 10) et direction (40 ; 50). Rien n'est sélectionné.
fn marks_harness() -> (Harness<'static, App>, [pf_core::PointId; 4]) {
    let mut h = piece_harness();
    h.set_size(egui::vec2(1280.0, 1600.0));
    let ids = {
        let ed = &mut h.state_mut().editor;
        let sheet = ed.sheet;
        let mut add = |x: f64, y: f64| {
            ed.exec(pf_core::Command::AddPoint {
                sheet,
                at: pt(x, y),
            })
            .unwrap()
            .points[0]
        };
        let ids = [
            add(50.0, 30.0),
            add(150.0, 30.0),
            add(10.0, 10.0),
            add(40.0, 50.0),
        ];
        ed.selection.clear();
        ids
    };
    h.run_steps(3);
    open_fold(&mut h, "piece.marking.title");
    open_fold(&mut h, "piece.buttons.title");
    (h, ids)
}

fn markings_of(h: &Harness<'_, App>) -> Vec<pf_core::PieceMarking> {
    h.state().editor.project().pieces[0].markings.clone()
}

fn select_points(h: &mut Harness<'_, App>, points: &[pf_core::PointId]) {
    h.state_mut().editor.selection.points = points.to_vec();
    h.run_steps(3);
}

/// Remplace le contenu du champ texte dont la valeur est `current` par `new`.
fn replace_field(h: &mut Harness<'_, App>, current: &str, new: &str) {
    {
        let field = h
            .get_all_by_role(egui::accesskit::Role::TextInput)
            .find(|f| f.value().as_deref() == Some(current))
            .unwrap_or_else(|| panic!("champ « {current} »"));
        field.focus();
    }
    h.run_steps(2);
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.run_steps(1);
    type_text(h, new);
    h.run_steps(2);
}

#[test]
fn us063_ajouter_un_percage_depuis_la_selection_une_etape_annulable() {
    let (mut h, [inside, ..]) = marks_harness();
    h.get_by_label("Perçages et pointages");
    h.get_by_label("Aucun marquage.");
    h.get_by_label("Ajouter un marquage…").click();
    h.run_steps(3);
    // Rien de sélectionné : l'ajout est grisé ; un seul point : possible.
    let add = "Ajouter le marquage";
    assert!(h.get_by_label(add).accesskit_node().is_disabled());
    h.get_by_label_contains("Sélectionnez 1 point de la feuille");
    select_points(&mut h, &[inside]);
    assert!(!h.get_by_label(add).accesskit_node().is_disabled());
    // Diamètre saisi au clavier (la valeur par défaut est 6).
    replace_field(&mut h, "6", "9");
    h.get_by_label(add).click();
    h.run_steps(3);
    assert!(
        !h.state().editor.status.error,
        "{}",
        h.state().editor.status.text
    );
    let marks = markings_of(&h);
    assert_eq!(marks.len(), 1, "{marks:?}");
    assert_eq!(marks[0].at, pf_core::MarkingAnchor::Point { point: inside });
    assert_eq!(
        marks[0].shape,
        pf_core::MarkingShape::Drill { diameter_mm: 9.0 }
    );
    h.get_by_label("1 marquage(s), exportés comme groupe distinct (liés à la construction).");
    h.get_by_label_contains("Perçage Ø 9,0 mm · point");
    // Une seule étape : annuler retire le marquage, la construction n'a pas changé.
    let points = h.state().editor.sheet().points.len();
    undo(&mut h);
    assert!(markings_of(&h).is_empty());
    assert_eq!(h.state().editor.sheet().points.len(), points);
    h.get_by_label("Aucun marquage.");
    h.state().editor.project().validate().unwrap();
}

#[test]
fn us063_pointage_croix_et_bout_de_pince_recule() {
    let (mut h, [inside, _, tip, toward]) = marks_harness();
    h.get_by_label("Ajouter un marquage…").click();
    h.run_steps(3);
    // Pointage sur un point : aucun champ de dimension.
    h.get_by_label("Pointage (point plein)").click();
    h.run_steps(3);
    select_points(&mut h, &[inside]);
    h.get_by_label("Ajouter le marquage").click();
    h.run_steps(3);
    assert_eq!(markings_of(&h)[0].shape, pf_core::MarkingShape::Punch);
    h.get_by_label_contains("Pointage · point");
    // Croix de 6 mm (valeur par défaut) sur un autre point.
    h.get_by_label("Croix").click();
    h.run_steps(3);
    select_points(&mut h, &[tip]);
    h.get_by_label("Ajouter le marquage").click();
    h.run_steps(3);
    assert_eq!(
        markings_of(&h)[1].shape,
        pf_core::MarkingShape::Cross { size_mm: 6.0 }
    );
    // Bout de pince reculé : deux points (pointe puis direction), recul 10 par défaut.
    h.get_by_label("Perçage (cercle)").click();
    h.run_steps(3);
    h.get_by_label("Bout de pince reculé").click();
    h.run_steps(3);
    h.get_by_label_contains("Sélectionnez 2 points");
    select_points(&mut h, &[tip]);
    assert!(
        h.get_by_label("Ajouter le marquage")
            .accesskit_node()
            .is_disabled(),
        "un seul point ne définit pas la direction"
    );
    select_points(&mut h, &[tip, toward]);
    h.get_by_label("Ajouter le marquage").click();
    h.run_steps(3);
    assert!(
        !h.state().editor.status.error,
        "{}",
        h.state().editor.status.text
    );
    let marks = markings_of(&h);
    assert_eq!(marks.len(), 3);
    assert_eq!(
        marks[2].at,
        pf_core::MarkingAnchor::DartTip {
            tip,
            toward,
            setback_mm: 10.0
        }
    );
    // (10 ; 10) + 10 · (30 ; 40) / 50 = (16 ; 18), calculé par le cœur.
    let at = h.state().editor.project().piece_markings(piece_of(&h).id)[2].at;
    assert!(
        pf_geometry::distance_mm(at, pt(16.0, 18.0)) < 1e-9,
        "{at:?}"
    );
    h.get_by_label_contains("bout de pince");
    // Trois ajouts = trois étapes.
    for n in [2, 1, 0] {
        undo(&mut h);
        assert_eq!(markings_of(&h).len(), n);
    }
}

#[test]
fn us063_modifier_la_dimension_et_le_recul_puis_retirer() {
    let (mut h, [inside, _, tip, toward]) = marks_harness();
    let piece = piece_of(&h).id;
    {
        let ed = &mut h.state_mut().editor;
        ed.exec(pf_core::Command::AddPieceMarking {
            piece,
            at: pf_core::MarkingAnchor::Point { point: inside },
            shape: pf_core::MarkingShape::Drill { diameter_mm: 6.0 },
        })
        .unwrap();
        ed.exec(pf_core::Command::AddPieceMarking {
            piece,
            at: pf_core::MarkingAnchor::DartTip {
                tip,
                toward,
                setback_mm: 10.0,
            },
            shape: pf_core::MarkingShape::Punch,
        })
        .unwrap();
    }
    h.run_steps(3);
    // Diamètre 6,0 → 4 : une étape.
    replace_field(&mut h, "6,0", "4");
    key(&mut h, Key::Enter);
    h.run_steps(3);
    assert_eq!(
        markings_of(&h)[0].shape,
        pf_core::MarkingShape::Drill { diameter_mm: 4.0 }
    );
    // Recul 10,0 → 25 : une étape ; le point calculé est (25 ; 30).
    replace_field(&mut h, "10,0", "25");
    key(&mut h, Key::Enter);
    h.run_steps(3);
    let at = h.state().editor.project().piece_markings(piece)[1].at;
    assert!(
        pf_geometry::distance_mm(at, pt(25.0, 30.0)) < 1e-9,
        "{at:?}"
    );
    // Valeur invalide : message, aucune trace.
    let before = markings_of(&h);
    replace_field(&mut h, "4,0", "abc");
    key(&mut h, Key::Enter);
    h.run_steps(3);
    assert!(h.state().editor.status.error);
    assert_eq!(markings_of(&h), before);
    // Valeur hors bornes : refus du cœur affiché, aucune trace.
    replace_field(&mut h, "4,0", "0");
    key(&mut h, Key::Enter);
    h.run_steps(3);
    assert!(h.state().editor.status.error);
    assert_eq!(markings_of(&h), before);
    // Retrait : le marquage disparaît, la construction reste.
    let points = h.state().editor.sheet().points.len();
    h.get_all_by_label("Retirer le marquage")
        .next()
        .expect("bouton de retrait")
        .click();
    h.run_steps(3);
    assert_eq!(markings_of(&h).len(), 1);
    assert_eq!(h.state().editor.sheet().points.len(), points);
    undo(&mut h);
    assert_eq!(markings_of(&h).len(), 2);
    // Retour en arrière sur les deux modifications : le diamètre d'origine revient.
    undo(&mut h);
    undo(&mut h);
    assert_eq!(
        markings_of(&h)[0].shape,
        pf_core::MarkingShape::Drill { diameter_mm: 6.0 }
    );
}

#[test]
fn us063_formulaire_refus_du_coeur_affiche_sans_trace() {
    let (mut h, [inside, ..]) = marks_harness();
    h.get_by_label("Ajouter un marquage…").click();
    h.run_steps(3);
    select_points(&mut h, &[inside]);
    replace_field(&mut h, "6", "0");
    h.get_by_label("Ajouter le marquage").click();
    h.run_steps(3);
    assert!(h.state().editor.status.error);
    assert!(markings_of(&h).is_empty());
    let steps = h.state().editor.project().clone();
    // Texte non numérique : message de saisie, aucun appel au cœur.
    replace_field(&mut h, "0", "abc");
    h.get_by_label("Ajouter le marquage").click();
    h.run_steps(3);
    assert!(h.state().editor.status.error);
    assert!(h.state().editor.status.text.contains("Dimension invalide"));
    assert!(markings_of(&h).is_empty());
    assert_eq!(*h.state().editor.project(), steps, "aucune trace");
    // Fermer le formulaire : il disparaît, la section reste.
    h.get_by_label("Fermer le formulaire").click();
    h.run_steps(3);
    h.get_by_label("Ajouter un marquage…");
}

#[test]
fn us063_marquage_hors_du_contour_signale_en_texte_et_symbole() {
    let (mut h, [_, outside, ..]) = marks_harness();
    let piece = piece_of(&h).id;
    h.state_mut()
        .editor
        .exec(pf_core::Command::AddPieceMarking {
            piece,
            at: pf_core::MarkingAnchor::Point { point: outside },
            shape: pf_core::MarkingShape::Cross { size_mm: 8.0 },
        })
        .unwrap();
    h.run_steps(3);
    h.get_by_label_contains("⚠ centre hors du contour");
    h.get_by_label_contains("1 marquage(s) ont leur centre hors du contour");
    assert!(!h.state().editor.status.error);
    h.state().editor.project().validate().unwrap();
}

#[test]
fn us063_la_section_fonctionne_en_anglais() {
    let (mut h, [inside, _, tip, toward]) = marks_harness();
    let piece = piece_of(&h).id;
    {
        let ed = &mut h.state_mut().editor;
        for (at, shape) in [
            (
                pf_core::MarkingAnchor::Point { point: inside },
                pf_core::MarkingShape::Drill { diameter_mm: 6.0 },
            ),
            (
                pf_core::MarkingAnchor::DartTip {
                    tip,
                    toward,
                    setback_mm: 10.0,
                },
                pf_core::MarkingShape::Cross { size_mm: 8.0 },
            ),
        ] {
            ed.exec(pf_core::Command::AddPieceMarking { piece, at, shape })
                .unwrap();
        }
    }
    pf_i18n::set_lang(pf_i18n::Lang::En);
    h.run_steps(4);
    h.get_by_label("Drill holes and punch marks");
    h.get_by_label("2 marking(s), exported as a separate group (linked to the construction).");
    h.get_by_label_contains("Drill hole Ø 6.0 mm · point");
    h.get_by_label_contains("Cross 8.0 mm · dart tip");
    assert_eq!(h.get_all_by_label("Remove the marking").count(), 2);
    h.get_by_label("Add a marking…").click();
    h.run_steps(3);
    h.get_by_label("Punch mark (solid dot)");
    h.get_by_label("Drill hole (circle)");
    h.get_by_label("Set-back dart tip").click();
    h.run_steps(3);
    h.get_by_label_contains("Select 2 points");
    h.get_by_label("Set back by (mm)");
    h.get_by_label("Close the form").click();
    h.run_steps(3);
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
}

// ----- US-06.4 : boutons et boutonnières --------------------------------------------------

fn groups_of(h: &Harness<'_, App>) -> Vec<pf_core::ButtonGroup> {
    h.state().editor.project().pieces[0].buttons.clone()
}

fn hole(
    length_mm: f64,
    width_mm: f64,
    orientation: pf_core::ButtonholeOrientation,
) -> pf_core::ButtonElement {
    pf_core::ButtonElement::Buttonhole {
        length_mm,
        width_mm,
        orientation,
    }
}

/// Ajoute un groupe à la pièce « Devant » par le cœur (pas par l'interface).
fn add_group(
    h: &mut Harness<'_, App>,
    ends: (pf_core::PointId, pf_core::PointId),
    count: u32,
    element: pf_core::ButtonElement,
    edge_margin_mm: f64,
) {
    let piece = piece_of(h).id;
    h.state_mut()
        .editor
        .exec(pf_core::Command::AddButtonGroup {
            piece,
            from: ends.0,
            to: ends.1,
            count,
            element,
            edge_margin_mm,
        })
        .unwrap();
    h.run_steps(3);
}

#[test]
fn us064_ajouter_des_boutons_depuis_la_selection_une_etape_annulable() {
    let (mut h, [_, _, tip, toward]) = marks_harness();
    h.get_by_label("Boutons et boutonnières");
    h.get_by_label("Aucun groupe.");
    h.get_by_label("Ajouter des boutons…").click();
    h.run_steps(3);
    // Rien de sélectionné ou un seul point : l'ajout est grisé.
    let add = "Ajouter le groupe";
    assert!(h.get_by_label(add).accesskit_node().is_disabled());
    select_points(&mut h, &[tip]);
    assert!(h.get_by_label(add).accesskit_node().is_disabled());
    select_points(&mut h, &[tip, toward]);
    assert!(!h.get_by_label(add).accesskit_node().is_disabled());
    // Nombre 5 → 4, recul 10 → 2, saisis au clavier ; diamètre 12 par défaut.
    replace_field(&mut h, "5", "4");
    replace_field(&mut h, "10", "2");
    h.get_by_label(add).click();
    h.run_steps(3);
    assert!(
        !h.state().editor.status.error,
        "{}",
        h.state().editor.status.text
    );
    let groups = groups_of(&h);
    assert_eq!(groups.len(), 1, "{groups:?}");
    assert_eq!(
        (groups[0].from, groups[0].to, groups[0].count),
        (tip, toward, 4)
    );
    assert_eq!(
        groups[0].element,
        pf_core::ButtonElement::Button { diameter_mm: 12.0 }
    );
    assert_eq!(groups[0].edge_margin_mm, 2.0);
    h.get_by_label("1 groupe(s), exportés comme groupe distinct (positions calculées d'après les deux points).");
    h.get_by_label_contains("4 bouton(s) Ø 12,0 mm");
    h.get_by_label_contains("espacement 16,67 mm");
    // Positions calculées par le cœur : de (10 ; 10) à (40 ; 50), quatre éléments à 50/3 mm.
    let centers: Vec<pf_geometry::PointMm> =
        h.state().editor.project().piece_buttons(piece_of(&h).id)[0]
            .elements
            .iter()
            .map(|e| e.center)
            .collect();
    for (c, want) in centers.iter().zip([
        (10.0, 10.0),
        (20.0, 23.333_333_333),
        (30.0, 36.666_666_667),
        (40.0, 50.0),
    ]) {
        assert!(
            pf_geometry::distance_mm(*c, pt(want.0, want.1)) < 1e-6,
            "{c:?} {want:?}"
        );
    }
    // Une seule étape : annuler retire le groupe, la construction n'a pas changé.
    let points = h.state().editor.sheet().points.len();
    undo(&mut h);
    assert!(groups_of(&h).is_empty());
    assert_eq!(h.state().editor.sheet().points.len(), points);
    h.get_by_label("Aucun groupe.");
    h.state().editor.project().validate().unwrap();
}

#[test]
fn us064_boutonnieres_formulaire_orientation_et_angle_libre() {
    let (mut h, [_, _, tip, toward]) = marks_harness();
    h.get_by_label("Ajouter des boutons…").click();
    h.run_steps(3);
    h.get_by_label("Boutonnières (fentes)").click();
    h.run_steps(3);
    select_points(&mut h, &[tip, toward]);
    // Longueur 20, largeur 3 par défaut ; orientation : angle libre −45° (normalisé en 315°).
    h.get_by_label("angle libre").click();
    h.run_steps(3);
    replace_field(&mut h, "0", "-45");
    h.get_by_label("Ajouter le groupe").click();
    h.run_steps(3);
    assert!(
        !h.state().editor.status.error,
        "{}",
        h.state().editor.status.text
    );
    let g = groups_of(&h);
    assert_eq!(g.len(), 1);
    assert_eq!(
        g[0].element,
        hole(
            20.0,
            3.0,
            pf_core::ButtonholeOrientation::Angle { degrees: 315.0 }
        )
    );
    h.get_by_label_contains("5 boutonnière(s) 20,0 mm × 3,0 mm (angle 315,0°)");
}

#[test]
fn us064_modifier_nombre_dimensions_orientation_et_recul_puis_retirer() {
    let (mut h, [_, _, tip, toward]) = marks_harness();
    add_group(
        &mut h,
        (tip, toward),
        3,
        hole(20.0, 4.0, pf_core::ButtonholeOrientation::Horizontal {}),
        0.0,
    );
    h.get_by_label_contains("3 boutonnière(s) 20,0 mm × 4,0 mm (horizontale)");
    // Nombre 3 → 4 : une étape.
    replace_field(&mut h, "3", "4");
    key(&mut h, Key::Enter);
    h.run_steps(3);
    assert_eq!(groups_of(&h)[0].count, 4);
    // Longueur 20,0 → 30 et largeur 4,0 → 5.
    replace_field(&mut h, "20,0", "30");
    key(&mut h, Key::Enter);
    h.run_steps(3);
    replace_field(&mut h, "4,0", "5");
    key(&mut h, Key::Enter);
    h.run_steps(3);
    assert_eq!(
        groups_of(&h)[0].element,
        hole(30.0, 5.0, pf_core::ButtonholeOrientation::Horizontal {})
    );
    // Orientation : verticale, puis perpendiculaire, puis angle libre (qui garde l'angle actuel).
    h.get_by_label("verticale").click();
    h.run_steps(3);
    assert_eq!(
        groups_of(&h)[0].element,
        hole(30.0, 5.0, pf_core::ButtonholeOrientation::Vertical {})
    );
    h.get_by_label("perpendiculaire à l'axe").click();
    h.run_steps(3);
    assert_eq!(
        groups_of(&h)[0].element,
        hole(30.0, 5.0, pf_core::ButtonholeOrientation::Perpendicular {})
    );
    let axis = h.state().editor.project().piece_buttons(piece_of(&h).id)[0].axis_deg;
    h.get_by_label("angle libre").click();
    h.run_steps(3);
    let stored = groups_of(&h)[0].element;
    let pf_core::ButtonElement::Buttonhole { orientation, .. } = stored else {
        panic!("boutonnière");
    };
    let pf_core::ButtonholeOrientation::Angle { degrees } = orientation else {
        panic!("angle libre : {orientation:?}");
    };
    assert!(
        (degrees - (axis + 90.0).rem_euclid(360.0)).abs() < 1e-9,
        "{degrees} / {axis}"
    );
    // Recul du bord 0,0 → 7 mm.
    replace_field(&mut h, "0,0", "7");
    key(&mut h, Key::Enter);
    h.run_steps(3);
    assert_eq!(groups_of(&h)[0].edge_margin_mm, 7.0);
    // Valeur non numérique : message, aucune trace.
    let before = h.state().editor.project().clone();
    replace_field(&mut h, "4", "abc");
    key(&mut h, Key::Enter);
    h.run_steps(3);
    assert!(h.state().editor.status.error);
    assert!(h.state().editor.status.text.contains("Nombre invalide"));
    assert_eq!(*h.state().editor.project(), before, "aucune trace");
    // Valeur hors bornes : refus du cœur affiché, aucune trace.
    replace_field(&mut h, "4", "0");
    key(&mut h, Key::Enter);
    h.run_steps(3);
    assert!(h.state().editor.status.error);
    assert_eq!(*h.state().editor.project(), before, "aucune trace");
    replace_field(&mut h, "4", "101");
    key(&mut h, Key::Enter);
    h.run_steps(3);
    assert!(h.state().editor.status.error);
    assert_eq!(*h.state().editor.project(), before, "aucune trace");
    // Retrait : le groupe disparaît, la construction reste ; annuler le rend.
    let points = h.state().editor.sheet().points.len();
    h.get_by_label("Retirer le groupe").click();
    h.run_steps(3);
    assert!(groups_of(&h).is_empty());
    assert_eq!(h.state().editor.sheet().points.len(), points);
    undo(&mut h);
    assert_eq!(groups_of(&h).len(), 1);
}

#[test]
fn us064_points_pris_dans_la_selection_pour_un_groupe_existant() {
    let (mut h, [inside, _, tip, toward]) = marks_harness();
    add_group(
        &mut h,
        (tip, toward),
        3,
        pf_core::ButtonElement::Button { diameter_mm: 8.0 },
        0.0,
    );
    // Aucun point sélectionné : le bouton est grisé.
    assert!(h
        .get_by_label("Points = sélection")
        .accesskit_node()
        .is_disabled());
    select_points(&mut h, &[inside, toward]);
    assert!(!h
        .get_by_label("Points = sélection")
        .accesskit_node()
        .is_disabled());
    h.get_by_label("Points = sélection").click();
    h.run_steps(3);
    let g = groups_of(&h);
    assert_eq!((g[0].from, g[0].to), (inside, toward));
    // (50 ; 30) → (40 ; 50) : le milieu est (45 ; 40).
    let mid = h.state().editor.project().piece_buttons(piece_of(&h).id)[0].elements[1].center;
    assert!(
        pf_geometry::distance_mm(mid, pt(45.0, 40.0)) < 1e-9,
        "{mid:?}"
    );
    // Deux fois le même point sélectionné n'est pas possible (la sélection est un ensemble).
    undo(&mut h);
    assert_eq!(groups_of(&h)[0].from, tip);
}

#[test]
fn us064_recul_du_bord_signale_en_texte_et_symbole_sans_refus() {
    let (mut h, [_, _, tip, toward]) = marks_harness();
    // Boutons Ø 12 de (10 ; 10) à (40 ; 50) : le premier est à 10 mm du bord gauche (x = 0) et du
    // bord bas (y = 0), soit 4 mm de recul réel pour un recul demandé de 20 mm.
    add_group(
        &mut h,
        (tip, toward),
        2,
        pf_core::ButtonElement::Button { diameter_mm: 12.0 },
        20.0,
    );
    h.get_by_label_contains("⚠ 2 élément(s) plus près du bord que le recul demandé");
    h.get_by_label_contains(
        "2 bouton(s) ou boutonnière(s) plus près du contour que le recul demandé",
    );
    assert!(!h.state().editor.status.error);
    let core = h
        .state()
        .editor
        .project()
        .piece_buttons_too_close(piece_of(&h).id);
    assert_eq!(core, 2);
    // Recul 20 → 3 : seul respecté désormais, l'avertissement disparaît.
    replace_field(&mut h, "20,0", "3");
    key(&mut h, Key::Enter);
    h.run_steps(3);
    assert_eq!(groups_of(&h)[0].edge_margin_mm, 3.0);
    assert!(h
        .query_by_label_contains("plus près du bord que le recul demandé")
        .is_none());
    assert!(h
        .query_by_label_contains("plus près du contour que le recul demandé")
        .is_none());
    h.state().editor.project().validate().unwrap();
}

#[test]
fn us064_la_section_et_le_dessin_fonctionnent_en_anglais() {
    let (mut h, [_, _, tip, toward]) = marks_harness();
    add_group(
        &mut h,
        (tip, toward),
        3,
        hole(20.0, 4.0, pf_core::ButtonholeOrientation::Perpendicular {}),
        30.0,
    );
    // Le dessin de la feuille (cercles, fentes, étiquettes, ⚠) s'exécute sans panique.
    h.run_steps(4);
    pf_i18n::set_lang(pf_i18n::Lang::En);
    h.run_steps(4);
    h.get_by_label("Buttons and buttonholes");
    h.get_by_label(
        "1 group(s), exported as a separate group (positions computed from the two points).",
    );
    h.get_by_label_contains("3 buttonhole(s) 20.0 mm × 4.0 mm (perpendicular to the axis)");
    h.get_by_label_contains("element(s) closer to the edge than the requested set-back");
    h.get_by_label("Remove the group");
    h.get_by_label("Add buttons…").click();
    h.run_steps(3);
    h.get_by_label("Buttons (circles)");
    h.get_by_label("Buttonholes (slots)").click();
    h.run_steps(3);
    h.get_by_label_contains("Select 2 points on the sheet");
    h.get_by_label("Add the group");
    h.get_by_label("Close the buttons form").click();
    h.run_steps(3);
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
}

// ----- US-06.2 : cran lié à un point (inspecteur du cran) ------------------------------------

/// Segment AB de (0 ; 0) à (120 ; 0), cran à distance fixe de 40 mm (sélectionné), points
/// libres P (70 ; 20) et Q (90 ; 20).
fn notch_anchor_harness() -> (
    Harness<'static, App>,
    pf_core::NotchId,
    pf_core::EntityId,
    [pf_core::PointId; 2],
) {
    let mut h = harness();
    h.set_size(egui::vec2(1280.0, 1600.0));
    let (notch, seg, pts) = {
        let ed = &mut h.state_mut().editor;
        let sheet = ed.sheet;
        // Les points libres d'abord : P est le premier point de la feuille (proposé par défaut).
        let add = |ed: &mut crate::editor::Editor, x: f64, y: f64| {
            ed.exec(pf_core::Command::AddPoint {
                sheet,
                at: pt(x, y),
            })
            .unwrap()
            .points[0]
        };
        let pts = [add(ed, 70.0, 20.0), add(ed, 90.0, 20.0)];
        let seg = ed
            .exec(pf_core::Command::AddSegment {
                sheet,
                from: pf_core::PointRef::New(pt(0.0, 0.0)),
                to: pf_core::PointRef::New(pt(120.0, 0.0)),
            })
            .unwrap()
            .entities[0];
        let notch = ed
            .exec(pf_core::Command::AddNotch {
                sheet,
                entity: seg,
                distance_mm: 40.0,
                side: pf_geometry::Side::Left,
                length_mm: 5.0,
                style: pf_core::NotchStyle::Single,
            })
            .unwrap()
            .notches[0];
        ed.selection.clear();
        ed.selection.notches = vec![notch];
        (notch, seg, pts)
    };
    h.run_steps(3);
    (h, notch, seg, pts)
}

fn the_notch(h: &Harness<'_, App>, id: pf_core::NotchId) -> pf_core::Notch {
    h.state().editor.sheet().notch(id).unwrap().clone()
}

/// Bouton de mode d'ancrage (le libellé peut aussi exister ailleurs : on prend le bouton).
fn anchor_mode<'a>(h: &'a Harness<'_, App>, label: &'a str) -> egui_kittest::Node<'a> {
    // Le panneau de l'inspecteur est le dernier de l'arbre : on prend le dernier bouton.
    h.get_all_by_label(label)
        .rfind(|n| n.accesskit_node().role() == egui::accesskit::Role::Button)
        .unwrap_or_else(|| panic!("bouton « {label} »"))
}

#[test]
fn us062_milieu_puis_proportion_une_etape_annulable_chacun() {
    let (mut h, notch, ..) = notch_anchor_harness();
    h.get_by_label("ANCRAGE");
    // À distance fixe, le champ de distance est saisissable.
    h.get_by_label("Distance du départ");
    h.get_by_label_contains("Distance fixe : le cran reste");
    assert_eq!(the_notch(&h, notch).anchor, None);
    anchor_mode(&h, "Milieu").click();
    h.run_steps(3);
    assert!(
        !h.state().editor.status.error,
        "{}",
        h.state().editor.status.text
    );
    assert_eq!(
        the_notch(&h, notch).anchor,
        Some(pf_core::NotchAnchor::Midpoint {})
    );
    // Position calculée affichée (60 mm), plus de champ de distance saisissable.
    h.get_by_label("Position calculée");
    assert!(h.get_all_by_value("60,0 mm").count() >= 1);
    assert_eq!(h.query_all_by_label("Distance du départ").count(), 0);
    // Proportion : la position actuelle est conservée (60 / 120 = 50 %).
    anchor_mode(&h, "Proportion").click();
    h.run_steps(3);
    assert_eq!(
        the_notch(&h, notch).anchor,
        Some(pf_core::NotchAnchor::Ratio { ratio: 0.5 })
    );
    h.get_by_label_contains("Le cran est à cette proportion");
    // Une étape par changement : annuler deux fois redonne le cran à distance fixe de 40 mm.
    undo(&mut h);
    assert_eq!(
        the_notch(&h, notch).anchor,
        Some(pf_core::NotchAnchor::Midpoint {})
    );
    undo(&mut h);
    let n = the_notch(&h, notch);
    assert_eq!((n.anchor, n.distance_mm), (None, 40.0));
    h.state().editor.project().validate().unwrap();
}

#[test]
fn us062_point_le_cran_suit_le_deplacement_puis_se_detache_a_sa_place() {
    let (mut h, notch, _, [p, _]) = notch_anchor_harness();
    anchor_mode(&h, "Point").click();
    h.run_steps(3);
    // Le premier point de la feuille est proposé ; la liste permet d'en choisir un autre.
    assert_eq!(
        the_notch(&h, notch).anchor,
        Some(pf_core::NotchAnchor::Point { point: p })
    );
    h.get_by_label_contains("Le cran est au pied de la perpendiculaire");
    {
        let ed = &mut h.state_mut().editor;
        let sheet = ed.sheet;
        ed.exec(pf_core::Command::MovePoint {
            sheet,
            point: p,
            to: pt(100.0, 33.0),
        })
        .unwrap();
    }
    h.run_steps(3);
    let sheet = h.state().editor.sheet();
    assert_eq!(
        sheet.notch_distance_mm(sheet.notch(notch).unwrap()),
        Ok(100.0)
    );
    assert!(h.get_all_by_value("100,0 mm").count() >= 1);
    // Détacher : le cran garde la position calculée, devenue distance fixe.
    anchor_mode(&h, "Distance fixe").click();
    h.run_steps(3);
    let n = the_notch(&h, notch);
    assert_eq!((n.anchor, n.distance_mm), (None, 100.0));
    h.state().editor.project().validate().unwrap();
}

#[test]
fn us062_suppression_du_point_retire_le_cran_et_le_dit() {
    let (mut h, notch, _, [p, _]) = notch_anchor_harness();
    anchor_mode(&h, "Point").click();
    h.run_steps(3);
    {
        let ed = &mut h.state_mut().editor;
        let sheet = ed.sheet;
        let out = ed
            .exec(pf_core::Command::Delete {
                sheet,
                points: vec![p],
                entities: vec![],
            })
            .unwrap();
        assert!(
            out.notes.iter().any(|n| n.contains("ancré")),
            "{:?}",
            out.notes
        );
    }
    h.run_steps(3);
    assert!(h.state().editor.sheet().notch(notch).is_err());
    assert!(!h.state().editor.status.error);
    assert!(
        h.state().editor.status.text.contains("ancré"),
        "{}",
        h.state().editor.status.text
    );
    h.state().editor.project().validate().unwrap();
}

#[test]
fn us062_report_refuse_par_le_coeur_sans_trace() {
    let mut h = harness();
    h.set_size(egui::vec2(1280.0, 1600.0));
    let notch = {
        let ed = &mut h.state_mut().editor;
        let sheet = ed.sheet;
        // Point libre d'abord (premier de la feuille, proposé par défaut), puis une source de
        // 200 mm (premier tracé, proposé par défaut) et le bord du cran, de 30 mm.
        ed.exec(pf_core::Command::AddPoint {
            sheet,
            at: pt(150.0, 80.0),
        })
        .unwrap();
        ed.exec(pf_core::Command::AddSegment {
            sheet,
            from: pf_core::PointRef::New(pt(0.0, 50.0)),
            to: pf_core::PointRef::New(pt(200.0, 50.0)),
        })
        .unwrap();
        let short = ed
            .exec(pf_core::Command::AddSegment {
                sheet,
                from: pf_core::PointRef::New(pt(0.0, 0.0)),
                to: pf_core::PointRef::New(pt(30.0, 0.0)),
            })
            .unwrap()
            .entities[0];
        let notch = ed
            .exec(pf_core::Command::AddNotch {
                sheet,
                entity: short,
                distance_mm: 10.0,
                side: pf_geometry::Side::Left,
                length_mm: 5.0,
                style: pf_core::NotchStyle::Single,
            })
            .unwrap()
            .notches[0];
        ed.selection.clear();
        ed.selection.notches = vec![notch];
        notch
    };
    h.run_steps(3);
    let before = h.state().editor.project().clone();
    anchor_mode(&h, "Report").click();
    h.run_steps(3);
    // Le point se projette à 150 mm de la source : hors du bord de 30 mm, refus expliqué.
    assert!(h.state().editor.status.error);
    assert!(
        h.state().editor.status.text.contains("sort du bord"),
        "{}",
        h.state().editor.status.text
    );
    assert_eq!(*h.state().editor.project(), before, "aucune trace");
    assert_eq!(the_notch(&h, notch).anchor, None);
}

#[test]
fn us062_l_inspecteur_fonctionne_en_anglais() {
    let (mut h, notch, ..) = notch_anchor_harness();
    pf_i18n::set_lang(pf_i18n::Lang::En);
    h.run_steps(4);
    h.get_by_label("ANCHOR");
    h.get_by_label_contains("Fixed distance: the notch stays");
    anchor_mode(&h, "Midpoint").click();
    h.run_steps(3);
    assert_eq!(
        the_notch(&h, notch).anchor,
        Some(pf_core::NotchAnchor::Midpoint {})
    );
    h.get_by_label("Computed position");
    h.get_by_label_contains("The notch sits at the middle of the path");
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
    h.run_steps(3);
    h.get_by_label("Position calculée");
}

// ----- US-06.1 : crans avancés (forme, largeur, sur la coupe) --------------------------------

/// Pièce « Devant » 100 × 60 avec 10 mm de marge et un cran de 5 mm, à 30 mm du coin (0 ; 0) sur
/// le bord du bas, sélectionné (inspecteur du cran affiché).
fn notch_shape_harness() -> (Harness<'static, App>, pf_core::NotchId) {
    let mut h = piece_harness();
    h.set_size(egui::vec2(1280.0, 1800.0));
    let notch = {
        let ed = &mut h.state_mut().editor;
        let sheet = ed.sheet;
        let piece = ed.project().pieces[0].id;
        ed.exec(pf_core::Command::SetSeamAllowance {
            piece,
            seam: Some(pf_core::SeamAllowance {
                default_mm: 10.0,
                ..Default::default()
            }),
        })
        .unwrap();
        let outline: Vec<pf_core::EntityId> = ed.project().pieces[0]
            .outline
            .iter()
            .map(|o| o.entity)
            .collect();
        let (edge, at) = outline
            .iter()
            .find_map(|&e| {
                let (a, b) = ed.sheet().segment_ends(e).unwrap();
                match (a.x, a.y, b.x, b.y) {
                    (x0, 0.0, x1, 0.0) if x0 == 0.0 && x1 == 100.0 => Some((e, 30.0)),
                    (x0, 0.0, x1, 0.0) if x0 == 100.0 && x1 == 0.0 => Some((e, 70.0)),
                    _ => None,
                }
            })
            .expect("bord du bas");
        let notch = ed
            .exec(pf_core::Command::AddNotch {
                sheet,
                entity: edge,
                distance_mm: at,
                side: pf_geometry::Side::Right,
                length_mm: 5.0,
                style: pf_core::NotchStyle::Single,
            })
            .unwrap()
            .notches[0];
        ed.selection.clear();
        ed.selection.notches = vec![notch];
        notch
    };
    h.run_steps(3);
    (h, notch)
}

#[test]
fn us061_forme_puis_sur_la_coupe_une_etape_annulable_chacune() {
    let (mut h, notch) = notch_shape_harness();
    h.get_by_label("Forme");
    assert_eq!(the_notch(&h, notch).form, pf_core::NotchForm::Line);
    // Forme V : une étape.
    anchor_mode(&h, "V").click();
    h.run_steps(3);
    assert!(
        !h.state().editor.status.error,
        "{}",
        h.state().editor.status.text
    );
    let n = the_notch(&h, notch);
    assert_eq!(
        (n.form, n.width_mm, n.on_cut),
        (pf_core::NotchForm::V, None, false)
    );
    h.get_by_label("Largeur du cran");
    // Sur la coupe : une autre étape ; la distance couture → coupe est la marge (10 mm).
    h.get_by_label_contains("Sur la coupe").click();
    h.run_steps(3);
    assert!(the_notch(&h, notch).on_cut);
    h.get_by_label("Écart couture / coupe");
    assert!(h.get_all_by_value("10,0 mm").count() >= 1);
    // Position lue d'après la marge : (30 ; -10) puis (30 ; -15) quand la marge change.
    let sheet = h.state().editor.sheet;
    let foot = |h: &Harness<'_, App>| {
        h.state()
            .editor
            .project()
            .notch_on_cut(sheet, notch)
            .unwrap()
            .foot
    };
    let f = foot(&h);
    assert!(
        (f.x - 30.0).abs() < 0.01 && (f.y + 10.0).abs() < 0.01,
        "{f:?}"
    );
    {
        let ed = &mut h.state_mut().editor;
        let piece = ed.project().pieces[0].id;
        ed.exec(pf_core::Command::SetSeamAllowance {
            piece,
            seam: Some(pf_core::SeamAllowance {
                default_mm: 15.0,
                ..Default::default()
            }),
        })
        .unwrap();
    }
    h.run_steps(3);
    let f = foot(&h);
    assert!(
        (f.x - 30.0).abs() < 0.01 && (f.y + 15.0).abs() < 0.01,
        "{f:?}"
    );
    assert!(h.get_all_by_value("15,0 mm").count() >= 1);
    // Annuler : la marge, puis « sur la coupe », puis la forme (une étape chacune).
    undo(&mut h);
    undo(&mut h);
    assert!(!the_notch(&h, notch).on_cut);
    assert_eq!(the_notch(&h, notch).form, pf_core::NotchForm::V);
    undo(&mut h);
    assert_eq!(the_notch(&h, notch).form, pf_core::NotchForm::Line);
    h.state().editor.project().validate().unwrap();
}

#[test]
fn us061_forme_refusee_par_le_coeur_sans_trace() {
    let (mut h, notch) = notch_shape_harness();
    {
        // Profondeur de 1 mm : un U de largeur par défaut (3 mm) ne tient pas.
        let ed = &mut h.state_mut().editor;
        let sheet = ed.sheet;
        let n = ed.sheet().notch(notch).unwrap().clone();
        ed.exec(pf_core::Command::SetNotch {
            sheet,
            notch,
            distance_mm: n.distance_mm,
            side: n.side,
            length_mm: 1.0,
            style: n.style,
        })
        .unwrap();
    }
    h.run_steps(3);
    let before = h.state().editor.project().clone();
    anchor_mode(&h, "U").click();
    h.run_steps(3);
    assert!(h.state().editor.status.error);
    assert!(
        h.state()
            .editor
            .status
            .text
            .contains("ran en U : la largeur (3,0 mm)"),
        "{}",
        h.state().editor.status.text
    );
    assert_eq!(*h.state().editor.project(), before, "aucune trace");
    assert_eq!(the_notch(&h, notch).form, pf_core::NotchForm::Line);
}

#[test]
fn us061_l_inspecteur_des_formes_fonctionne_en_anglais() {
    let (mut h, notch) = notch_shape_harness();
    pf_i18n::set_lang(pf_i18n::Lang::En);
    h.run_steps(4);
    h.get_by_label("Shape");
    anchor_mode(&h, "Slot").click();
    h.run_steps(3);
    assert_eq!(the_notch(&h, notch).form, pf_core::NotchForm::Slot);
    h.get_by_label("Notch width");
    h.get_by_label_contains("On the cutting line").click();
    h.run_steps(3);
    assert!(the_notch(&h, notch).on_cut);
    h.get_by_label("Seam / cut gap");
    h.get_by_label_contains("The notch is projected perpendicular to the seam");
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
    h.run_steps(3);
    h.get_by_label("Forme");
}

// ----- US-06.1b : cran en coin ---------------------------------------------------------------

/// Le cran de `notch_shape_harness` ramené au début de son bord (distance 0) : un sommet.
fn corner_harness(at_vertex: bool) -> (Harness<'static, App>, pf_core::NotchId) {
    let (mut h, notch) = notch_shape_harness();
    if at_vertex {
        let ed = &mut h.state_mut().editor;
        let sheet = ed.sheet;
        let n = ed.sheet().notch(notch).unwrap().clone();
        ed.exec(pf_core::Command::SetNotch {
            sheet,
            notch,
            distance_mm: 0.0,
            side: n.side,
            length_mm: n.length_mm,
            style: n.style,
        })
        .unwrap();
        h.run_steps(3);
    }
    (h, notch)
}

#[test]
fn us061b_cran_en_coin_une_etape_annulable_direction_a_45_degres() {
    let (mut h, notch) = corner_harness(true);
    let before = h.state().editor.project().clone();
    assert!(!the_notch(&h, notch).corner);
    let case = h.get_by_label_contains("Cran en coin (bissectrice)");
    assert!(!case.accesskit_node().is_disabled());
    case.click();
    h.run_steps(3);
    assert!(
        !h.state().editor.status.error,
        "{}",
        h.state().editor.status.text
    );
    assert!(the_notch(&h, notch).corner);
    h.get_by_label("Angle du sommet");
    assert!(h.get_all_by_value("90,0°").count() >= 1);
    // Direction : bissectrice à 45° (|dx| = |dy|), trait de 5 mm, au sommet du contour.
    let sheet = h.state().editor.sheet;
    let c = h
        .state()
        .editor
        .project()
        .notch_corner(sheet, notch)
        .unwrap();
    assert!((c.direction.x.abs() - std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-9);
    assert!((c.direction.y.abs() - std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-9);
    // Une seule annulation remet le projet d'avant, octet pour octet.
    undo(&mut h);
    assert_eq!(*h.state().editor.project(), before);
    assert!(!the_notch(&h, notch).corner);
    h.state().editor.project().validate().unwrap();
}

#[test]
fn us061b_case_desactivee_hors_sommet_avec_explication_et_refus_sans_trace() {
    let (mut h, notch) = corner_harness(false);
    let case = h.get_by_label_contains("Cran en coin (bissectrice)");
    assert!(case.accesskit_node().is_disabled());
    h.get_by_label_contains("Cran en coin indisponible");
    h.get_by_label_contains("pas à une extrémité");
    // Même par la commande : refus sans trace, erreur affichée.
    let before = h.state().editor.project().clone();
    {
        let ed = &mut h.state_mut().editor;
        let sheet = ed.sheet;
        assert!(ed
            .exec(pf_core::Command::SetNotchCorner {
                sheet,
                notch,
                corner: true,
            })
            .is_none());
    }
    h.run_steps(3);
    assert!(h.state().editor.status.error);
    assert_eq!(*h.state().editor.project(), before, "aucune trace");
}

#[test]
fn us061b_sur_la_coupe_est_desactive_pour_un_cran_en_coin() {
    let (mut h, notch) = corner_harness(true);
    h.get_by_label_contains("Cran en coin (bissectrice)")
        .click();
    h.run_steps(3);
    assert!(the_notch(&h, notch).corner);
    // Le libellé est aussi cité par la note du coin : on prend la case.
    let cut = h
        .get_all_by_label_contains("Sur la coupe")
        .find(|n| n.accesskit_node().role() == egui::accesskit::Role::CheckBox)
        .expect("case « Sur la coupe »");
    assert!(cut.accesskit_node().is_disabled());
    assert!(!the_notch(&h, notch).on_cut);
}

#[test]
fn us061b_l_inspecteur_du_coin_fonctionne_en_anglais() {
    let (mut h, notch) = corner_harness(true);
    pf_i18n::set_lang(pf_i18n::Lang::En);
    h.run_steps(4);
    h.get_by_label_contains("Corner notch (bisector)").click();
    h.run_steps(3);
    assert!(the_notch(&h, notch).corner);
    h.get_by_label("Vertex angle");
    h.get_by_label_contains("oriented along the bisector");
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
    h.run_steps(3);
    h.get_by_label("Angle du sommet");
}

// ----- US-06.5 : droit-fil avancé -----------------------------------------------------------

struct GrainIds {
    href: pf_core::EntityId,
    extra: pf_core::EntityId,
}

/// Pièce « Devant » 100 × 60 avec un droit-fil vertical (50 ; 10) → (50 ; 50), un tracé de
/// référence horizontal (0 ; 100) → (40 ; 100) et une ligne de droit-fil libre (10 ; 45) → (90 ; 45).
fn grain_harness() -> (Harness<'static, App>, GrainIds) {
    let mut h = piece_harness();
    h.set_size(egui::vec2(1280.0, 1800.0));
    let ids = {
        let ed = &mut h.state_mut().editor;
        let sheet = ed.sheet;
        let seg = |ed: &mut crate::editor::Editor,
                   a: (f64, f64),
                   b: (f64, f64),
                   role: pf_core::PathRole| {
            let e = ed
                .exec(pf_core::Command::AddSegment {
                    sheet,
                    from: pf_core::PointRef::New(pt(a.0, a.1)),
                    to: pf_core::PointRef::New(pt(b.0, b.1)),
                })
                .unwrap()
                .entities[0];
            ed.exec(pf_core::Command::SetPathRole {
                sheet,
                entity: e,
                role,
            })
            .unwrap();
            e
        };
        let main = seg(ed, (50.0, 10.0), (50.0, 50.0), pf_core::PathRole::Grainline);
        let href = seg(
            ed,
            (0.0, 100.0),
            (40.0, 100.0),
            pf_core::PathRole::Construction,
        );
        let extra = seg(ed, (10.0, 45.0), (90.0, 45.0), pf_core::PathRole::Grainline);
        let piece = ed.project().pieces[0].clone();
        ed.exec(pf_core::Command::SetPieceOutline {
            piece: piece.id,
            outline: piece.outline.clone(),
            grainline: Some(main),
        })
        .unwrap();
        ed.selection.clear();
        GrainIds { href, extra }
    };
    h.run_steps(3);
    open_fold(&mut h, "piece.grain.title");
    (h, ids)
}

fn grain_of(h: &Harness<'_, App>) -> Option<pf_core::PieceGrain> {
    h.state().editor.project().pieces[0].grain.clone()
}

fn select_entities(h: &mut Harness<'_, App>, entities: &[pf_core::EntityId]) {
    h.state_mut().editor.selection.entities = entities.to_vec();
    h.run_steps(3);
}

#[test]
fn us065_direction_parallele_a_un_trace_choisi_une_etape_annulable() {
    let (mut h, ids) = grain_harness();
    h.get_by_label("DROIT-FIL AVANCÉ");
    h.get_by_label_contains("Flèche : 90,0° depuis l'axe X");
    assert_eq!(grain_of(&h), None);
    select_entities(&mut h, &[ids.href]);
    h.get_by_label("Parallèle à…").click();
    h.run_steps(3);
    assert!(
        !h.state().editor.status.error,
        "{}",
        h.state().editor.status.text
    );
    assert_eq!(
        grain_of(&h).unwrap().direction,
        pf_core::GrainDirection::Parallel {
            reference: ids.href
        }
    );
    // La flèche est calculée par le cœur : horizontale, même longueur (40) et même milieu.
    h.get_by_label_contains("Flèche : 0,0° depuis l'axe X");
    let l = h
        .state()
        .editor
        .project()
        .piece_grain_lines(piece_of(&h).id)[0];
    assert!(pf_geometry::distance_mm(l.from, pt(30.0, 30.0)) < 1e-9);
    assert!(pf_geometry::distance_mm(l.to, pt(70.0, 30.0)) < 1e-9);
    // Perpendiculaire : même tracé de référence conservé.
    h.get_by_label("Perpendiculaire à…").click();
    h.run_steps(3);
    assert_eq!(
        grain_of(&h).unwrap().direction,
        pf_core::GrainDirection::Perpendicular {
            reference: ids.href
        }
    );
    h.get_by_label_contains("Flèche : 90,0° depuis l'axe X");
    // Biais ±45° : mesuré depuis l'axe X de la feuille.
    h.get_by_label("Biais +45°").click();
    h.run_steps(3);
    assert_eq!(
        grain_of(&h).unwrap().direction,
        pf_core::GrainDirection::Bias { angle_deg: 45.0 }
    );
    h.get_by_label_contains("Flèche : 45,0° depuis l'axe X");
    h.get_by_label("Biais −45°").click();
    h.run_steps(3);
    h.get_by_label_contains("Flèche : 315,0° depuis l'axe X");
    // Chaque clic = une étape : tout annuler redonne le droit-fil dessiné, sans réglage.
    for _ in 0..4 {
        undo(&mut h);
    }
    assert_eq!(grain_of(&h), None);
    h.get_by_label_contains("Flèche : 90,0° depuis l'axe X");
    h.state().editor.project().validate().unwrap();
}

#[test]
fn us065_trace_de_reference_choisi_dans_la_liste() {
    let (mut h, ids) = grain_harness();
    select_entities(&mut h, &[ids.href]);
    h.get_by_label("Parallèle à…").click();
    h.run_steps(3);
    h.get_by_label("Tracé de référence (la direction est celle de sa corde)");
    // Autre tracé de la liste : un côté vertical du rectangle de la pièce.
    let other = {
        let p = h.state().editor.project();
        let sheet = p.sheet(h.state().editor.sheet).unwrap();
        sheet
            .entities
            .iter()
            .find(|e| {
                e.id != ids.href
                    && e.role != pf_core::PathRole::Grainline
                    && sheet.entity_points(e.id).is_ok_and(|(a, b)| {
                        match (sheet.position(a), sheet.position(b)) {
                            (Ok(a), Ok(b)) => (a.x - b.x).abs() < 1e-9 && (a.y - b.y).abs() > 1.0,
                            _ => false,
                        }
                    })
            })
            .map(|e| (e.id, sheet.entity_name(e.id)))
            .expect("un côté vertical")
    };
    h.get_by_label(&other.1).click();
    h.run_steps(3);
    assert_eq!(
        grain_of(&h).unwrap().direction,
        pf_core::GrainDirection::Parallel { reference: other.0 }
    );
    h.get_by_label_contains("Flèche : 90,0° depuis l'axe X");
}

#[test]
fn us065_sens_inversion_et_tolerance_de_rotation() {
    let (mut h, _) = grain_harness();
    h.get_by_label("Poil").click();
    h.run_steps(3);
    assert_eq!(grain_of(&h).unwrap().sense, pf_core::GrainSense::Nap);
    h.get_by_label("Motif").click();
    h.run_steps(3);
    assert_eq!(grain_of(&h).unwrap().sense, pf_core::GrainSense::Pattern);
    h.get_by_label("Inverser la flèche").click();
    h.run_steps(3);
    assert!(grain_of(&h).unwrap().reversed);
    h.get_by_label_contains("Flèche : 270,0° depuis l'axe X");
    // Tolérance : saisie au clavier, validée par le cœur (0 à 180).
    replace_field(&mut h, "0,0", "20");
    key(&mut h, Key::Enter);
    h.run_steps(3);
    assert_eq!(grain_of(&h).unwrap().rotation_tolerance_deg, Some(20.0));
    // Valeur hors bornes : refus expliqué, rien n'est enregistré.
    let before = grain_of(&h);
    replace_field(&mut h, "20,0", "200");
    key(&mut h, Key::Enter);
    h.run_steps(3);
    assert!(h.state().editor.status.error);
    assert_eq!(grain_of(&h), before);
    // Texte non numérique : message propre.
    replace_field(&mut h, "20,0", "beaucoup");
    key(&mut h, Key::Enter);
    h.run_steps(3);
    assert!(h.state().editor.status.error);
    assert_eq!(grain_of(&h), before);
}

#[test]
fn us065_rotation_au_placement_avertissement_jamais_un_refus() {
    let (mut h, _) = grain_harness();
    let piece = piece_of(&h).id;
    // Tolérance 20°, poil : rotation 20° acceptée, 25° signalée, jamais refusée.
    h.state_mut()
        .editor
        .exec(pf_core::Command::SetPieceGrain {
            piece,
            grain: pf_core::PieceGrain {
                sense: pf_core::GrainSense::Nap,
                rotation_tolerance_deg: Some(20.0),
                ..pf_core::PieceGrain::default()
            },
        })
        .unwrap();
    let rotate = |h: &mut Harness<'_, App>, deg: f64| {
        h.state_mut()
            .editor
            .exec(pf_core::Command::SetPiecePlacement {
                piece,
                placement: Some(pf_core::Placement {
                    x_mm: 0.0,
                    y_mm: 0.0,
                    rotation_deg: deg,
                }),
            })
            .unwrap();
        h.run_steps(3);
    };
    rotate(&mut h, 20.0);
    h.get_by_label_contains(
        "Placement tourné de 20,0° : déviation du droit-fil 20,0°, tolérance 20,0°.",
    );
    assert!(h
        .query_by_label_contains("dévie le droit-fil au-delà")
        .is_none());
    rotate(&mut h, 25.0);
    h.get_by_label_contains(
        "Placement tourné de 25,0° : déviation du droit-fil 25,0°, tolérance 20,0°.",
    );
    h.get_by_label_contains("la rotation du placement dévie le droit-fil au-delà de la tolérance");
    assert!(!h.state().editor.status.error);
    h.state().editor.project().validate().unwrap();
    // Dans le plan de pièces, la même mesure est affichée pour la pièce choisie.
    open_plan(&mut h);
    h.get_all_by_label("Devant")
        .next()
        .expect("ligne de la liste")
        .click();
    h.run_steps(4);
    // Le panneau de gauche est masqué en mode plan : la mesure est dans l'inspecteur du plan.
    assert!(
        h.get_all_by_label_contains(
            "Placement tourné de 25,0° : déviation du droit-fil 25,0°, tolérance 20,0°."
        )
        .count()
            >= 1
    );
}

#[test]
fn us065_lignes_supplementaires_ajouter_regler_retirer() {
    let (mut h, ids) = grain_harness();
    h.get_by_label("Lignes supplémentaires (0/8)");
    h.get_by_label("Aucune ligne supplémentaire (rayures, raccords).");
    // Sans sélection valide, l'ajout est grisé.
    assert!(h
        .get_by_label("Ajouter la ligne sélectionnée")
        .accesskit_node()
        .is_disabled());
    // Un tracé qui n'a pas le rôle droit-fil n'est pas accepté.
    select_entities(&mut h, &[ids.href]);
    assert!(h
        .get_by_label("Ajouter la ligne sélectionnée")
        .accesskit_node()
        .is_disabled());
    select_entities(&mut h, &[ids.extra]);
    assert!(!h
        .get_by_label("Ajouter la ligne sélectionnée")
        .accesskit_node()
        .is_disabled());
    h.get_by_label("Ajouter la ligne sélectionnée").click();
    h.run_steps(3);
    assert!(
        !h.state().editor.status.error,
        "{}",
        h.state().editor.status.text
    );
    let g = grain_of(&h).unwrap();
    assert_eq!(g.extra.len(), 1);
    assert_eq!(g.extra[0].entity, ids.extra);
    h.get_by_label("Lignes supplémentaires (1/8)");
    // La ligne a ses propres réglages : le second « Poil » est celui de la ligne supplémentaire.
    let naps: Vec<_> = h.get_all_by_label("Poil").collect();
    assert_eq!(naps.len(), 2);
    naps[1].click();
    h.run_steps(3);
    let g = grain_of(&h).unwrap();
    assert_eq!(g.extra[0].sense, pf_core::GrainSense::Nap);
    assert_eq!(
        g.sense,
        pf_core::GrainSense::Unmarked,
        "la principale ne change pas"
    );
    // Biais de la ligne supplémentaire : calculé à partir de son propre tracé (longueur 80).
    h.get_all_by_label("Biais +45°")
        .nth(1)
        .expect("biais de la ligne supplémentaire")
        .click();
    h.run_steps(3);
    let lines = h
        .state()
        .editor
        .project()
        .piece_grain_lines(piece_of(&h).id);
    assert_eq!(lines.len(), 2);
    assert!((lines[1].length_mm - 80.0).abs() < 1e-9);
    assert!((lines[1].direction_deg - 45.0).abs() < 1e-9);
    // Une étape par action ; retirer la ligne puis tout annuler.
    h.get_by_label("Retirer cette ligne").click();
    h.run_steps(3);
    assert!(
        grain_of(&h).is_none(),
        "{:?} {}",
        grain_of(&h),
        h.state().editor.status.text
    );
    // Annuler le retrait redonne la ligne (« motif » du biais), puis les étapes précédentes.
    undo(&mut h);
    assert_eq!(grain_of(&h).unwrap().extra.len(), 1);
    for _ in 0..3 {
        undo(&mut h);
    }
    assert!(grain_of(&h).is_none());
    h.state().editor.project().validate().unwrap();
}

#[test]
fn us065_supprimer_le_trace_de_reference_ramene_la_direction_dessinee_avec_remarque() {
    let (mut h, ids) = grain_harness();
    select_entities(&mut h, &[ids.href]);
    h.get_by_label("Parallèle à…").click();
    h.run_steps(3);
    let sheet = h.state().editor.sheet;
    h.state_mut()
        .editor
        .exec(pf_core::Command::Delete {
            sheet,
            points: vec![],
            entities: vec![ids.href],
        })
        .unwrap();
    h.run_steps(3);
    assert_eq!(
        grain_of(&h),
        None,
        "plus aucun réglage : direction dessinée"
    );
    h.get_by_label_contains("Flèche : 90,0° depuis l'axe X");
    assert!(h.state().editor.status.text.contains("tracé supprimé"));
    h.state().editor.project().validate().unwrap();
}

#[test]
fn us065_la_section_et_les_libelles_fonctionnent_en_anglais() {
    let (mut h, ids) = grain_harness();
    select_entities(&mut h, &[ids.href]);
    pf_i18n::set_lang(pf_i18n::Lang::En);
    h.run_steps(4);
    h.get_by_label("ADVANCED GRAINLINE");
    h.get_by_label_contains("Arrow: 90.0° from the sheet X axis");
    h.get_by_label("Rotation tolerance");
    h.get_by_label("Extra lines (0/8)");
    h.get_by_label("Bias −45°").click();
    h.run_steps(3);
    h.get_by_label_contains("Arrow: 315.0° from the sheet X axis");
    h.get_by_label("Nap").click();
    h.run_steps(3);
    assert_eq!(grain_of(&h).unwrap().sense, pf_core::GrainSense::Nap);
    h.get_by_label("Reverse the arrow");
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
    h.run_steps(3);
    h.get_by_label("Inverser la flèche");
}

// ----- US-06.8 : lignes spéciales et surpiqûre liée -----------------------------------------

/// Pièce « Devant » 100 × 60 (quatre tracés de construction), fenêtre haute : la section de
/// l'inspecteur reste entièrement visible.
fn topstitch_harness() -> (Harness<'static, App>, Vec<pf_core::EntityId>) {
    let mut h = piece_harness();
    h.set_size(egui::vec2(1280.0, 1800.0));
    let edges: Vec<pf_core::EntityId> = h
        .state()
        .editor
        .sheet()
        .entities
        .iter()
        .map(|e| e.id)
        .collect();
    h.run_steps(2);
    (h, edges)
}

fn links_of(h: &Harness<'_, App>) -> Vec<pf_core::Topstitch> {
    h.state().editor.sheet().topstitches.clone()
}

fn bytes_of(h: &Harness<'_, App>) -> String {
    pf_core::persist::to_json(h.state().editor.project()).unwrap()
}

/// Boîte (xmin, ymin, xmax, ymax) des lignes générées d'une surpiqûre faite de segments.
fn generated_box(h: &Harness<'_, App>, t: &pf_core::Topstitch) -> (f64, f64, f64, f64) {
    let s = h.state().editor.sheet();
    let pts: Vec<PointMm> = t
        .paths
        .iter()
        .flat_map(|&e| {
            let (a, b) = s.segment_ends(e).unwrap();
            [a, b]
        })
        .collect();
    (
        pts.iter().map(|p| p.x).fold(f64::MAX, f64::min),
        pts.iter().map(|p| p.y).fold(f64::MAX, f64::min),
        pts.iter().map(|p| p.x).fold(f64::MIN, f64::max),
        pts.iter().map(|p| p.y).fold(f64::MIN, f64::max),
    )
}

#[test]
fn us068_creer_une_surpiqure_depuis_la_selection_une_etape_annulable() {
    let (mut h, edges) = topstitch_harness();
    assert!(links_of(&h).is_empty());
    select_entities(&mut h, &edges);
    h.get_by_label("SURPIQÛRE LIÉE");
    h.get_by_label("Distance au bord (mm)");
    // Bord fermé : intérieur / extérieur, jamais gauche / droite.
    assert!(h.query_by_label("Créer — Gauche").is_none());
    h.get_by_label("Créer — Intérieur").click();
    h.run_steps(3);
    assert!(
        !h.state().editor.status.error,
        "{}",
        h.state().editor.status.text
    );
    let links = links_of(&h);
    assert_eq!(links.len(), 1);
    let t = &links[0];
    assert_eq!(t.side, pf_core::TopstitchSide::Inside);
    assert!((t.distance_mm - 5.0).abs() < 1e-12);
    // Calculé par le cœur : rectangle de (100 − 10) × (60 − 10) mm.
    let (x0, y0, x1, y1) = generated_box(&h, t);
    assert!((x0 - 5.0).abs() < 1e-9 && (y0 - 5.0).abs() < 1e-9);
    assert!((x1 - 95.0).abs() < 1e-9 && (y1 - 55.0).abs() < 1e-9);
    // Une seule étape : un Ctrl+Z retire le lien et les lignes.
    undo(&mut h);
    assert!(links_of(&h).is_empty());
    assert_eq!(h.state().editor.sheet().entities.len(), 4);
    h.state().editor.project().validate().unwrap();
}

#[test]
fn us068_distance_invalide_et_refus_du_coeur_sans_trace() {
    let (mut h, edges) = topstitch_harness();
    select_entities(&mut h, &edges);
    let before = bytes_of(&h);
    for text in ["abc", "", "0", "-3", "1e9"] {
        h.state_mut().topstitch_distance = text.to_string();
        h.run_steps(2);
        h.get_by_label("Créer — Extérieur").click();
        h.run_steps(3);
        assert!(
            h.state().editor.status.error,
            "« {text} » : {}",
            h.state().editor.status.text
        );
        assert_eq!(bytes_of(&h), before, "« {text} » : refus sans trace");
    }
    // « 40 » : décalage dégénéré vers l'intérieur d'un bord de 60 mm de haut, refusé de même.
    h.state_mut().topstitch_distance = "40".into();
    h.run_steps(2);
    h.get_by_label("Créer — Intérieur").click();
    h.run_steps(3);
    assert!(h.state().editor.status.error);
    assert_eq!(bytes_of(&h), before);
    assert!(links_of(&h).is_empty());
}

#[test]
fn us068_regler_la_distance_et_le_cote_puis_casser_le_lien() {
    let (mut h, edges) = topstitch_harness();
    select_entities(&mut h, &edges);
    h.get_by_label("Créer — Intérieur").click();
    h.run_steps(3);
    let t = links_of(&h)[0].clone();
    // Sélection d'une ligne générée : l'état et les réglages de sa surpiqûre.
    select_entities(&mut h, &[t.paths[0]]);
    h.get_by_label("À jour");
    h.get_by_label_contains("Suit 4 tracé(s) de la feuille");
    replace_field(&mut h, "5,0", "8");
    key(&mut h, Key::Enter);
    h.run_steps(3);
    assert!(
        !h.state().editor.status.error,
        "{}",
        h.state().editor.status.text
    );
    let t = links_of(&h)[0].clone();
    assert!((t.distance_mm - 8.0).abs() < 1e-12);
    let (x0, _, x1, _) = generated_box(&h, &t);
    assert!((x0 - 8.0).abs() < 1e-9 && (x1 - 92.0).abs() < 1e-9);
    // Côté : « Extérieur » recalcule dans la même étape.
    h.get_by_label("Extérieur").click();
    h.run_steps(3);
    let t = links_of(&h)[0].clone();
    assert_eq!(t.side, pf_core::TopstitchSide::Outside);
    let (x0, _, x1, _) = generated_box(&h, &t);
    assert!((x0 + 8.0).abs() < 1e-9 && (x1 - 108.0).abs() < 1e-9);
    // Valeur refusée par le cœur : rien ne change.
    let before = bytes_of(&h);
    replace_field(&mut h, "8,0", "-2");
    key(&mut h, Key::Enter);
    h.run_steps(3);
    assert!(h.state().editor.status.error);
    assert_eq!(bytes_of(&h), before);
    // La ligne ne se modifie pas directement : le déplacement d'un de ses points est refusé.
    let sheet = h.state().editor.sheet;
    let point = h
        .state()
        .editor
        .sheet()
        .entity_points(t.paths[0])
        .unwrap()
        .0;
    assert!(h
        .state_mut()
        .editor
        .exec(pf_core::Command::MovePoint {
            sheet,
            point,
            to: pt(-30.0, -30.0),
        })
        .is_none());
    assert!(h.state().editor.status.text.contains("Cassez le lien"));
    assert_eq!(bytes_of(&h), before);
    // Casser le lien : les lignes restent, libres.
    h.get_by_label("Casser le lien").click();
    h.run_steps(3);
    assert!(links_of(&h).is_empty());
    assert_eq!(h.state().editor.sheet().entities.len(), 8);
    undo(&mut h);
    assert_eq!(links_of(&h).len(), 1);
}

#[test]
fn us068_obsolete_signale_en_texte_et_symbole_jamais_un_refus() {
    let (mut h, edges) = topstitch_harness();
    h.state_mut().topstitch_distance = "20".into();
    select_entities(&mut h, &edges);
    h.get_by_label("Créer — Intérieur").click();
    h.run_steps(3);
    let t = links_of(&h)[0].clone();
    let ok = generated_box(&h, &t);
    // Le bord se rétrécit à 30 mm de large (< 2 × 20) : la commande n'est pas refusée.
    let sheet = h.state().editor.sheet;
    let (top, bottom) = {
        let s = h.state().editor.sheet();
        let find = |x: f64, y: f64| {
            s.points
                .iter()
                .find(|p| {
                    s.position(p.id)
                        .is_ok_and(|q| (q.x - x).abs() < 1e-9 && (q.y - y).abs() < 1e-9)
                })
                .unwrap()
                .id
        };
        (find(100.0, 60.0), find(100.0, 0.0))
    };
    let out = h
        .state_mut()
        .editor
        .exec(pf_core::Command::Batch {
            commands: vec![
                pf_core::Command::MovePoint {
                    sheet,
                    point: bottom,
                    to: pt(30.0, 0.0),
                },
                pf_core::Command::MovePoint {
                    sheet,
                    point: top,
                    to: pt(30.0, 60.0),
                },
            ],
        })
        .expect("la commande de la mère n'est pas refusée");
    assert!(out.notes.iter().any(|n| n.contains("surpiqûre")));
    h.run_steps(3);
    let t = links_of(&h)[0].clone();
    assert!(t.stale.is_some());
    assert_eq!(generated_box(&h, &t), ok, "dernière géométrie conservée");
    // Texte : ligne de la feuille (rien de sélectionné) et section de la ligne sélectionnée.
    h.state_mut().editor.selection.clear();
    h.run_steps(3);
    h.get_by_label("Surpiqûres liées");
    h.get_by_label("1 (1 obsolète(s))");
    select_entities(&mut h, &[t.paths[0]]);
    h.get_by_label_contains("⚠ Obsolète");
    h.get_by_label_contains("Les lignes gardent leur dernière géométrie valide");
    // Le bord reprend sa largeur : à jour dans la même étape.
    let out = h
        .state_mut()
        .editor
        .exec(pf_core::Command::Batch {
            commands: vec![
                pf_core::Command::MovePoint {
                    sheet,
                    point: bottom,
                    to: pt(100.0, 0.0),
                },
                pf_core::Command::MovePoint {
                    sheet,
                    point: top,
                    to: pt(100.0, 60.0),
                },
            ],
        })
        .unwrap();
    assert_eq!(out.regenerated_topstitches.len(), 1);
    h.run_steps(3);
    assert!(links_of(&h)[0].stale.is_none());
    h.get_by_label("À jour");
}

#[test]
fn us068_bord_ouvert_gauche_droite_et_contour_de_piece() {
    let (mut h, edges) = topstitch_harness();
    // Un segment seul : bord ouvert, gauche / droite à partir de son point de départ.
    select_entities(&mut h, &edges[..1]);
    h.get_by_label_contains("Bord ouvert");
    assert!(h.query_by_label("Créer — Intérieur").is_none());
    h.get_by_label("Créer — Gauche").click();
    h.run_steps(3);
    let t = links_of(&h)[0].clone();
    assert_eq!(t.side, pf_core::TopstitchSide::Left);
    assert!(matches!(
        t.source,
        pf_core::TopstitchSource::Paths { from: Some(_), .. }
    ));
    // La ligne est à 5 mm du segment (distance mesurée par le cœur).
    let s = h.state().editor.sheet();
    let (a, b) = s.segment_ends(edges[0]).unwrap();
    let (c, d) = s.segment_ends(t.paths[0]).unwrap();
    let dist = |p: PointMm| pf_geometry::project_on_segment(p, a, b).unwrap().1;
    assert!((dist(c) - 5.0).abs() < 1e-6 && (dist(d) - 5.0).abs() < 1e-6);
    // La pièce : surpiqûre le long de son contour, depuis la fiche (rien de sélectionné).
    h.state_mut().editor.selection.clear();
    h.run_steps(3);
    h.get_by_label("Surpiqûre le long du contour");
    h.get_by_label("Aucune surpiqûre liée à ce contour.");
    h.get_by_label("Surpiqûre — Extérieur").click();
    h.run_steps(3);
    let links = links_of(&h);
    assert_eq!(links.len(), 2);
    let piece = h.state().editor.project().pieces[0].id;
    assert_eq!(
        links[1].source,
        pf_core::TopstitchSource::Piece { piece },
        "source = contour de la pièce"
    );
    assert_eq!(links[1].side, pf_core::TopstitchSide::Outside);
    h.get_by_label_contains("Extérieur à 5,0 mm");
    undo(&mut h);
    assert_eq!(links_of(&h).len(), 1);
    h.state().editor.project().validate().unwrap();
}

#[test]
fn us068_choix_du_role_dans_l_inspecteur_et_libelles_distincts() {
    let (mut h, edges) = topstitch_harness();
    select_entities(&mut h, &edges[..1]);
    // Dernière liste déroulante : celle du rôle, dans l'inspecteur (à droite).
    h.get_all_by_role(egui::accesskit::Role::ComboBox)
        .last()
        .expect("rôle du tracé")
        .click();
    h.run_steps(3);
    // Les quatorze rôles sont proposés ; chaque libellé est distinct.
    let mut labels: Vec<&str> = pf_core::PathRole::ALL
        .iter()
        .map(|r| r.label_fr())
        .collect();
    labels.sort();
    labels.dedup();
    assert_eq!(labels.len(), 14);
    for r in pf_core::PathRole::ALL {
        assert_eq!(crate::text::role_label(r), r.label_fr());
    }
    // La liste défile (quatorze rôles) : « Ligne de collage » est tout en bas.
    h.get_by_label("Ligne de collage").click_accesskit();
    h.run_steps(3);
    assert_eq!(
        h.state().editor.sheet().entity(edges[0]).unwrap().role,
        pf_core::PathRole::GlueLine
    );
    undo(&mut h);
    assert_eq!(
        h.state().editor.sheet().entity(edges[0]).unwrap().role,
        pf_core::PathRole::Construction
    );
}

#[test]
fn us068_rendu_ecran_distinct_pour_chaque_role_sans_la_couleur() {
    let t = pf_ui::tokens::Tokens::precision_light();
    let style = |r| {
        let (_, width, dash, gap) = crate::canvas::role_style(r, &t);
        (width.to_bits(), dash.to_vec(), gap.to_vec())
    };
    for (i, a) in pf_core::PathRole::ALL.iter().enumerate() {
        for b in &pf_core::PathRole::ALL[i + 1..] {
            assert_ne!(
                style(*a),
                style(*b),
                "{a:?} et {b:?} ont le même trait à l'écran"
            );
        }
    }
    // Les lignes spéciales sont toutes pointillées, d'un motif propre.
    for r in pf_core::PathRole::SPECIAL {
        assert!(!style(r).1.is_empty(), "{r:?}");
    }
}

#[test]
fn us068_la_section_et_les_libelles_fonctionnent_en_anglais() {
    let (mut h, edges) = topstitch_harness();
    select_entities(&mut h, &edges);
    pf_i18n::set_lang(pf_i18n::Lang::En);
    h.run_steps(4);
    h.get_by_label("LINKED TOPSTITCHING");
    h.get_by_label("Distance from edge (mm)");
    h.get_by_label("Create — Inside").click();
    h.run_steps(3);
    assert_eq!(links_of(&h).len(), 1);
    let t = links_of(&h)[0].clone();
    select_entities(&mut h, &[t.paths[0]]);
    h.get_by_label("Up to date");
    h.get_by_label("Break the link");
    assert_eq!(
        crate::text::role_label(pf_core::PathRole::CenterFront),
        "Center front"
    );
    assert_eq!(
        crate::text::role_label(pf_core::PathRole::SkivingLine),
        "Skiving line"
    );
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
    h.run_steps(3);
    h.get_by_label("Casser le lien");
}

// ----- US-06.7 : cotations complètes ---------------------------------------------------------

/// Pièce « Devant » 100 × 60, fenêtre haute : la section « Cotes » de l'inspecteur reste visible.
fn dims_harness() -> Harness<'static, App> {
    let mut h = piece_harness();
    h.set_size(egui::vec2(1280.0, 2200.0));
    h.run_steps(2);
    open_fold(&mut h, "full_dim.title");
    h
}

fn point_at(h: &Harness<'_, App>, x: f64, y: f64) -> pf_core::PointId {
    let s = h.state().editor.sheet();
    s.points
        .iter()
        .find(|p| {
            s.position(p.id)
                .is_ok_and(|q| pf_geometry::distance_mm(q, pt(x, y)) < 1e-9)
        })
        .unwrap_or_else(|| panic!("aucun point en ({x} ; {y})"))
        .id
}

fn segment_between(h: &Harness<'_, App>, a: PointMm, b: PointMm) -> pf_core::EntityId {
    let s = h.state().editor.sheet();
    s.entities
        .iter()
        .find(|e| match s.shape(e.id) {
            Ok(pf_core::Shape::Line(p, q)) => {
                (pf_geometry::distance_mm(p, a) < 1e-9 && pf_geometry::distance_mm(q, b) < 1e-9)
                    || (pf_geometry::distance_mm(p, b) < 1e-9
                        && pf_geometry::distance_mm(q, a) < 1e-9)
            }
            _ => false,
        })
        .expect("segment")
        .id
}

/// Sélection exacte (points et tracés), comme après des clics.
fn pick(h: &mut Harness<'_, App>, points: &[pf_core::PointId], entities: &[pf_core::EntityId]) {
    let sel = &mut h.state_mut().editor.selection;
    sel.clear();
    sel.points = points.to_vec();
    sel.entities = entities.to_vec();
    h.run_steps(3);
}

/// Bouton de création de la section « Cotes » (un libellé comme « Angle » existe ailleurs).
fn add_button<'a>(h: &'a mut Harness<'static, App>, label: &'a str) -> egui_kittest::Node<'a> {
    h.get_by_role_and_label(egui::accesskit::Role::Button, label)
}

fn cotes_of(h: &Harness<'_, App>) -> Vec<pf_core::FullDimension> {
    h.state().editor.sheet().full_dimensions.clone()
}

fn cote_value(h: &Harness<'_, App>, i: usize) -> f64 {
    let s = h.state().editor.sheet();
    s.full_dimension_value(s.full_dimensions[i].id).unwrap()
}

/// Formes de l'écran (aplaties) du dernier affichage.
fn flat_shapes(h: &Harness<'_, App>) -> Vec<egui::epaint::Shape> {
    fn walk(s: &egui::epaint::Shape, out: &mut Vec<egui::epaint::Shape>) {
        match s {
            egui::epaint::Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
            other => out.push(other.clone()),
        }
    }
    let mut out = Vec::new();
    for cs in &h.output().shapes {
        walk(&cs.shape, &mut out);
    }
    out
}

#[test]
fn us067_horizontale_et_verticale_depuis_deux_points_une_etape_annulable() {
    let mut h = dims_harness();
    h.get_by_label("COTES (MESURES)");
    h.get_by_label("Aucune cote sur cette feuille.");
    let (a, b) = (point_at(&h, 0.0, 0.0), point_at(&h, 100.0, 60.0));
    pick(&mut h, &[a, b], &[]);
    add_button(&mut h, "Horizontale").click();
    h.run_steps(3);
    assert!(
        !h.state().editor.status.error,
        "{}",
        h.state().editor.status.text
    );
    assert_eq!(cotes_of(&h).len(), 1);
    assert!((cote_value(&h, 0) - 100.0).abs() < 1e-9);
    // Valeur lisible dans l'inspecteur (virgule décimale), avec la valeur signée.
    h.get_by_label_contains("Horizontale : 100,00 mm (valeur signée 100,0)");
    // Une seule étape : annuler la retire.
    undo(&mut h);
    assert!(cotes_of(&h).is_empty());
    // Verticale du second au premier point : ΔY = −60 (signé), affichée 60,00 mm.
    pick(&mut h, &[b, a], &[]);
    add_button(&mut h, "Verticale").click();
    h.run_steps(3);
    assert!((cote_value(&h, 0) + 60.0).abs() < 1e-9);
    h.get_by_label_contains("Verticale : 60,00 mm (valeur signée -60,0)");
    h.state().editor.project().validate().unwrap();
}

#[test]
fn us067_longueur_angle_rayon_et_diametre_depuis_la_selection() {
    let mut h = dims_harness();
    let s = h.state().editor.sheet;
    // Boutons grisés tant que la sélection ne convient pas.
    for label in ["Longueur du tracé", "Angle", "Rayon", "Diamètre"] {
        add_button(&mut h, label);
    }
    let bottom = segment_between(&h, pt(0.0, 0.0), pt(100.0, 0.0));
    let left = segment_between(&h, pt(0.0, 0.0), pt(0.0, 60.0));
    // Longueur d'un tracé.
    pick(&mut h, &[], &[bottom]);
    add_button(&mut h, "Longueur du tracé").click();
    h.run_steps(3);
    assert!((cote_value(&h, 0) - 100.0).abs() < 1e-9);
    // Angle de deux segments qui se touchent : sommet = extrémité commune. De `bottom` (est)
    // à `left` (nord) autour de (0 ; 0), anti-horaire : 90°.
    pick(&mut h, &[], &[bottom, left]);
    add_button(&mut h, "Angle").click();
    h.run_steps(3);
    assert!(
        (cote_value(&h, 1) - 90.0).abs() < 1e-9,
        "{}",
        cote_value(&h, 1)
    );
    h.get_by_label_contains("Angle : 90,00°");
    // Dans l'autre ordre : 270° (sens anti-horaire du premier au second côté).
    pick(&mut h, &[], &[left, bottom]);
    add_button(&mut h, "Angle").click();
    h.run_steps(3);
    assert!(
        (cote_value(&h, 2) - 270.0).abs() < 1e-9,
        "{}",
        cote_value(&h, 2)
    );
    // Angle de trois points : sommet d'abord.
    let (o, e, n) = (
        point_at(&h, 0.0, 0.0),
        point_at(&h, 100.0, 0.0),
        point_at(&h, 0.0, 60.0),
    );
    pick(&mut h, &[o, n, e], &[]);
    add_button(&mut h, "Angle").click();
    h.run_steps(3);
    assert!((cote_value(&h, 3) - 270.0).abs() < 1e-9);
    // Cercle : rayon et diamètre.
    let circle = h
        .state_mut()
        .editor
        .exec(pf_core::Command::AddCircle {
            sheet: s,
            center: pf_core::PointRef::New(pt(300.0, 300.0)),
            radius_mm: 12.5,
        })
        .unwrap()
        .entities[0];
    pick(&mut h, &[], &[circle]);
    add_button(&mut h, "Rayon").click();
    h.run_steps(3);
    assert!((cote_value(&h, 4) - 12.5).abs() < 1e-9);
    add_button(&mut h, "Diamètre").click();
    h.run_steps(3);
    assert!((cote_value(&h, 5) - 25.0).abs() < 1e-9);
    h.get_by_label_contains("Rayon : R 12,50 mm");
    h.get_by_label_contains("Diamètre : Ø 25,00 mm");
    h.state().editor.project().validate().unwrap();
}

#[test]
fn us067_legende_position_du_texte_et_mise_en_page_editables() {
    let mut h = dims_harness();
    let (a, b) = (point_at(&h, 0.0, 0.0), point_at(&h, 100.0, 60.0));
    pick(&mut h, &[a, b], &[]);
    add_button(&mut h, "Horizontale").click();
    h.run_steps(3);
    let id = cotes_of(&h)[0].id;
    // Les trois champs de la cote se suivent : légende, décalage du texte, décalage de la ligne.
    let focus_legend = |h: &mut Harness<'_, App>| {
        let fields: Vec<_> = h
            .get_all_by_role(egui::accesskit::Role::TextInput)
            .collect();
        let i = (0..fields.len().saturating_sub(2))
            .find(|&i| {
                fields[i].value().as_deref() == Some("")
                    && fields[i + 1].value().as_deref() == Some("0,0")
                    && fields[i + 2].value().as_deref() == Some("10,0")
            })
            .expect("champ de la légende");
        fields[i].focus();
    };
    focus_legend(&mut h);
    h.run_steps(2);
    type_text(&mut h, "Épaule — côté œil");
    key(&mut h, Key::Enter);
    h.run_steps(3);
    assert_eq!(
        cotes_of(&h)[0].label.as_deref(),
        Some("Épaule — côté œil"),
        "{}",
        h.state().editor.status.text
    );
    h.get_by_label_contains("Épaule — côté œil : 100,00 mm");
    // Position du texte : −20 mm le long de la ligne ; ligne décalée de 10 à 15.
    replace_field(&mut h, "0,0", "-20");
    key(&mut h, Key::Enter);
    h.run_steps(3);
    replace_field(&mut h, "10,0", "15");
    key(&mut h, Key::Enter);
    h.run_steps(3);
    let c = cotes_of(&h)[0].clone();
    assert_eq!(c.text_shift_mm, -20.0);
    assert_eq!(c.kind.placement(), 15.0);
    let g = h
        .state()
        .editor
        .sheet()
        .full_dimension_geometry(id)
        .unwrap();
    assert!(pf_geometry::distance_mm(g.text_anchor, pt(30.0, 15.0)) < 1e-9);
    // Chaque réglage est une étape ; trois annulations ramènent la cote nue.
    for _ in 0..3 {
        undo(&mut h);
    }
    let c = cotes_of(&h)[0].clone();
    assert!(c.label.is_none() && c.text_shift_mm == 0.0 && c.kind.placement() == 10.0);
}

#[test]
fn us067_refus_du_coeur_et_saisie_invalide_sans_trace() {
    let mut h = dims_harness();
    let (a, b) = (point_at(&h, 0.0, 0.0), point_at(&h, 100.0, 60.0));
    pick(&mut h, &[a, b], &[]);
    add_button(&mut h, "Horizontale").click();
    h.run_steps(3);
    let before = bytes_of(&h);
    // Nombre invalide : message propre, rien n'est écrit.
    replace_field(&mut h, "0,0", "abc");
    key(&mut h, Key::Enter);
    h.run_steps(3);
    assert!(h.state().editor.status.error);
    assert_eq!(bytes_of(&h), before);
    // Décalage hors bornes : refus du cœur, sans trace.
    replace_field(&mut h, "10,0", "99999999");
    key(&mut h, Key::Enter);
    h.run_steps(3);
    assert!(
        h.state().editor.status.error,
        "{}",
        h.state().editor.status.text
    );
    assert_eq!(bytes_of(&h), before);
    // Un rayon sur un segment n'est pas proposé (bouton grisé) : sélectionner un segment
    // laisse « Rayon » et « Diamètre » inactifs.
    let bottom = segment_between(&h, pt(0.0, 0.0), pt(100.0, 0.0));
    pick(&mut h, &[], &[bottom]);
    for label in ["Rayon", "Diamètre"] {
        add_button(&mut h, label).click();
        h.run_steps(2);
    }
    assert_eq!(cotes_of(&h).len(), 1);
    assert_eq!(bytes_of(&h), before);
}

#[test]
fn us067_cible_supprimee_retire_la_cote_et_le_dit() {
    let mut h = dims_harness();
    let bottom = segment_between(&h, pt(0.0, 0.0), pt(100.0, 0.0));
    pick(&mut h, &[], &[bottom]);
    add_button(&mut h, "Longueur du tracé").click();
    h.run_steps(3);
    assert_eq!(cotes_of(&h).len(), 1);
    // Retirer la cote par le bouton : une étape annulable, la construction n'est pas touchée.
    h.get_by_label("Retirer la cote").click();
    h.run_steps(3);
    assert!(cotes_of(&h).is_empty());
    undo(&mut h);
    assert_eq!(cotes_of(&h).len(), 1);
    let (sheet, entity) = (h.state().editor.sheet, bottom);
    h.state_mut()
        .editor
        .exec(pf_core::Command::Delete {
            sheet,
            points: vec![],
            entities: vec![entity],
        })
        .unwrap();
    h.run_steps(3);
    assert!(cotes_of(&h).is_empty(), "plus de référence morte");
    assert!(
        h.state().editor.status.text.contains("cote"),
        "{}",
        h.state().editor.status.text
    );
    h.state().editor.project().validate().unwrap();
}

#[test]
fn us067_dessin_texte_oriente_jamais_a_l_envers() {
    let mut h = dims_harness();
    let s = h.state().editor.sheet;
    // Segment descendant (direction 135° : texte ramené à −45°) et horizontale (0°).
    let seg = h
        .state_mut()
        .editor
        .exec(pf_core::Command::AddSegment {
            sheet: s,
            from: pf_core::PointRef::New(pt(300.0, 100.0)),
            to: pf_core::PointRef::New(pt(240.0, 160.0)),
        })
        .unwrap()
        .entities[0];
    pick(&mut h, &[], &[seg]);
    add_button(&mut h, "Longueur du tracé").click();
    h.run_steps(3);
    let angles: Vec<f32> = flat_shapes(&h)
        .iter()
        .filter_map(|s| match s {
            egui::epaint::Shape::Text(t) if t.galley.text() == "84,85 mm" && t.angle != 0.0 => {
                Some(t.angle)
            }
            _ => None,
        })
        .collect();
    assert_eq!(angles.len(), 1, "texte de la cote introuvable à l'écran");
    // −45° du modèle (trigonométrique) = +45° à l'écran (Y vers le bas, sens horaire).
    assert!(
        (angles[0] - std::f32::consts::FRAC_PI_4).abs() < 1e-4,
        "{}",
        angles[0]
    );
    // Toutes les cotes affichées ont un angle d'écran dans [−90° ; 90°[ (jamais à l'envers).
    for t in flat_shapes(&h).iter().filter_map(|s| match s {
        egui::epaint::Shape::Text(t) => Some(t),
        _ => None,
    }) {
        assert!(t.angle > -std::f32::consts::FRAC_PI_2 - 1e-4);
        assert!(t.angle < std::f32::consts::FRAC_PI_2 + 1e-4);
    }
}

#[test]
fn us067_la_section_et_les_libelles_fonctionnent_en_anglais() {
    let mut h = dims_harness();
    let (a, b) = (point_at(&h, 0.0, 0.0), point_at(&h, 100.0, 60.0));
    pick(&mut h, &[a, b], &[]);
    pf_i18n::set_lang(pf_i18n::Lang::En);
    h.run_steps(4);
    h.get_by_label("DIMENSION LINES (MEASURES)");
    add_button(&mut h, "Horizontal").click();
    h.run_steps(3);
    assert_eq!(cotes_of(&h).len(), 1);
    h.get_by_label_contains("Horizontal: 100.00 mm (signed value 100.0)");
    h.get_by_label("Remove dimension");
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
    h.run_steps(3);
    h.get_by_label("Retirer la cote");
}

// ----- US-13.1 : formats de page, orientation, marges, rouleau ----------------------------

use crate::print_setup::{self, FormatKind};
use pf_export::{Margins, Orientation, PageFormat, PageSetup};

fn open_print_window(h: &mut Harness<'static, App>) {
    h.get_by_label("Fichier").click();
    h.run_steps(3);
    h.get_by_label("Mise en page du PDF…").click();
    h.run_steps(4);
    assert!(h.state().print_dialog.is_some());
}

fn print_apply(h: &mut Harness<'static, App>) {
    h.get_by_label("Appliquer la mise en page").click();
    h.run_steps(4);
}

#[test]
fn us131_le_menu_ouvre_la_fenetre_avec_le_reglage_par_defaut_et_les_dimensions() {
    let mut h = piece_harness();
    assert!(h.state().page_setup.is_default());
    open_print_window(&mut h);
    h.get_by_label("A4");
    h.get_by_label_contains("Page : 210 × 297 mm");
    h.get_by_label_contains("Zone imprimable : 190 × 277 mm");
    h.get_by_label_contains("Feuille courante : 2 page(s), étalon compris");
    h.get_by_label_contains("L'échelle réelle à l'impression n'est pas validée");
    assert_eq!(
        h.state().print_dialog.as_ref().unwrap().kind,
        FormatKind::A4
    );
}

#[test]
fn us131_choisir_a3_paysage_apercu_puis_appliquer_et_memoriser() {
    let dir = std::env::temp_dir().join(format!("pf-us131-a-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let file = dir.join("settings.json");
    let mut h = piece_harness();
    h.state_mut().settings_path = Some(file.clone());
    open_print_window(&mut h);
    h.get_by_label("A3").click();
    h.run_steps(3);
    h.get_by_label("Paysage").click();
    h.run_steps(3);
    h.get_by_label_contains("Page : 420 × 297 mm");
    h.get_by_label_contains("Zone imprimable : 400 × 277 mm");
    // Rien n'est appliqué avant le bouton.
    assert!(h.state().page_setup.is_default());
    print_apply(&mut h);
    let expect = PageSetup {
        format: PageFormat::A3,
        orientation: Orientation::Landscape,
        margins: Margins::default(),
        ..PageSetup::default()
    };
    assert_eq!(h.state().page_setup, expect);
    assert!(h.state().print_dialog.is_none(), "fenêtre fermée");
    assert!(!h.state().editor.status.error);
    assert!(
        h.state().editor.status.text.contains("A3 paysage"),
        "{}",
        h.state().editor.status.text
    );
    // L'inspecteur annonce le réglage.
    h.get_by_label("PDF A3 paysage 1:1");
    // Mémorisé hors projet, relu à l'identique.
    assert_eq!(print_setup::load_page_setup(&file), expect);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn us131_format_libre_champs_valides_refus_sans_effet() {
    let mut h = piece_harness();
    open_print_window(&mut h);
    h.get_by_label("Format libre").click();
    h.run_steps(3);
    // Valeur non numérique : message, rien d'appliqué.
    h.state_mut().print_dialog.as_mut().unwrap().width = "abc".into();
    h.run_steps(3);
    h.get_by_label_contains("Largeur (mm) : saisir un nombre.");
    h.get_by_label("Appliquer la mise en page").click();
    h.run_steps(3);
    assert!(h.state().page_setup.is_default());
    assert!(h.state().print_dialog.is_some(), "la fenêtre reste ouverte");
    // Hors bornes.
    for (w, hh) in [
        ("100", "300"),
        ("300", "5001"),
        ("300", "0"),
        ("-300", "300"),
    ] {
        {
            let d = h.state_mut().print_dialog.as_mut().unwrap();
            d.width = w.into();
            d.height = hh.into();
        }
        h.run_steps(3);
        h.get_by_label_contains("hors des bornes (190 à 5000 mm)");
        assert!(h.state().page_setup.is_default());
    }
    // Bornes incluses : 190 × 5000 accepté ; saisie réelle dans le champ « Hauteur ».
    {
        let d = h.state_mut().print_dialog.as_mut().unwrap();
        d.width = "190".into();
        d.height = "300".into();
    }
    h.run_steps(3);
    {
        let fields: Vec<_> = h
            .get_all_by_role(egui::accesskit::Role::TextInput)
            .collect();
        let at = fields
            .iter()
            .position(|f| f.value().as_deref() == Some("300"))
            .expect("champ Hauteur");
        fields[at].focus();
    }
    h.run_steps(2);
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.run_steps(1);
    type_text(&mut h, "5000");
    key(&mut h, Key::Enter);
    h.run_steps(3);
    assert_eq!(h.state().print_dialog.as_ref().unwrap().height, "5000");
    h.get_by_label_contains("Page : 190 × 5000 mm");
    print_apply(&mut h);
    assert_eq!(
        h.state().page_setup.format,
        PageFormat::Custom {
            width_mm: 190.0,
            height_mm: 5000.0
        }
    );
    h.get_by_label("PDF libre 190 × 5000 mm 1:1");
}

#[test]
fn us131_rouleau_orientation_sans_effet_longueur_et_bornes() {
    let mut h = piece_harness();
    open_print_window(&mut h);
    h.get_by_label("Rouleau").click();
    h.run_steps(3);
    h.get_by_label_contains("L'orientation est sans effet");
    h.get_by_label_contains("Rouleau : largeur 914 mm");
    h.get_by_label_contains("Largeur imprimable : 894 mm");
    // Pièce de 100 × 60 mm : emprise 110 × 70, longueur 70 + bandeau 8 + marges 20 = 98 mm.
    h.get_by_label_contains("page de 98 mm de long, 2 page(s)");
    h.state_mut().print_dialog.as_mut().unwrap().roll_width = "190".into();
    h.run_steps(3);
    h.get_by_label_contains("Largeur imprimable : 170 mm");
    h.state_mut().print_dialog.as_mut().unwrap().roll_width = "189".into();
    h.run_steps(3);
    h.get_by_label_contains("hors des bornes (190 à 5000 mm)");
    h.state_mut().print_dialog.as_mut().unwrap().roll_width = "914".into();
    h.run_steps(3);
    print_apply(&mut h);
    assert_eq!(
        h.state().page_setup.format,
        PageFormat::Roll { width_mm: 914.0 }
    );
    h.get_by_label("PDF rouleau 914 mm 1:1");
}

#[test]
fn us131_marges_invalides_ou_trop_grandes_refusees() {
    let mut h = piece_harness();
    open_print_window(&mut h);
    for (i, value, needle) in [
        (
            0,
            "-1",
            "Marge haute (mm) : -1 mm hors des bornes (0 à 100 mm)",
        ),
        (
            2,
            "101",
            "Marge gauche (mm) : 101 mm hors des bornes (0 à 100 mm)",
        ),
        (3, "x", "Marge droite (mm) : saisir un nombre."),
    ] {
        h.state_mut().print_dialog.as_mut().unwrap().margins[i] = value.into();
        h.run_steps(3);
        h.get_by_label_contains(needle);
        h.state_mut().print_dialog.as_mut().unwrap().margins[i] = "10".into();
    }
    // La somme des marges dépasse ce que la page permet : zone imprimable trop petite.
    {
        let d = h.state_mut().print_dialog.as_mut().unwrap();
        d.margins = ["10".into(), "10".into(), "60".into(), "60".into()];
    }
    h.run_steps(3);
    h.get_by_label_contains("Zone imprimable 90 × 277 mm trop petite");
    h.get_by_label("Appliquer la mise en page").click();
    h.run_steps(3);
    assert!(h.state().page_setup.is_default(), "refus sans trace");
    // Marges asymétriques valides : appliquées telles quelles, avec virgule décimale.
    {
        let d = h.state_mut().print_dialog.as_mut().unwrap();
        d.margins = ["7,5".into(), "12".into(), "20".into(), "5".into()];
    }
    h.run_steps(3);
    h.get_by_label_contains("Zone imprimable : 185 × 277,5 mm");
    print_apply(&mut h);
    assert_eq!(
        h.state().page_setup.margins,
        Margins {
            top_mm: 7.5,
            bottom_mm: 12.0,
            left_mm: 20.0,
            right_mm: 5.0
        }
    );
}

#[test]
fn us131_valeurs_par_defaut_et_fermeture_sans_appliquer() {
    let mut h = piece_harness();
    h.state_mut().page_setup = PageSetup {
        format: PageFormat::Letter,
        orientation: Orientation::Landscape,
        margins: Margins::uniform(0.0),
        ..PageSetup::default()
    };
    h.run_steps(2);
    open_print_window(&mut h);
    // La fenêtre reprend le réglage courant.
    assert_eq!(
        h.state().print_dialog.as_ref().unwrap().kind,
        FormatKind::Letter
    );
    h.get_by_label_contains("Page : 279,4 × 215,9 mm");
    h.get_by_label("Valeurs par défaut (A4, marges de 10 mm)")
        .click();
    h.run_steps(3);
    h.get_by_label_contains("Page : 210 × 297 mm");
    // Fermer sans appliquer ne change rien.
    h.state_mut().print_dialog = None;
    h.run_steps(2);
    assert_eq!(h.state().page_setup.format, PageFormat::Letter);
}

#[test]
fn us131_reglage_relu_a_l_identique_et_valeurs_invalides_ignorees() {
    let dir = std::env::temp_dir().join(format!("pf-us131-b-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("settings.json");
    let samples = [
        PageSetup::default(),
        PageSetup {
            format: PageFormat::A0,
            orientation: Orientation::Landscape,
            margins: Margins {
                top_mm: 1.5,
                bottom_mm: 2.0,
                left_mm: 3.25,
                right_mm: 0.0,
            },
            ..PageSetup::default()
        },
        PageSetup {
            format: PageFormat::Custom {
                width_mm: 333.3,
                height_mm: 777.7,
            },
            orientation: Orientation::Portrait,
            margins: Margins::uniform(12.0),
            ..PageSetup::default()
        },
        PageSetup {
            format: PageFormat::Roll { width_mm: 914.4 },
            orientation: Orientation::Landscape,
            margins: Margins::uniform(5.0),
            ..PageSetup::default()
        },
    ];
    let mut h = piece_harness();
    h.state_mut().settings_path = Some(file.clone());
    for s in samples {
        h.state_mut().apply_page_setup(s);
        assert_eq!(print_setup::load_page_setup(&file), s);
        assert_eq!(print_setup::from_json(&print_setup::to_json(&s)), s);
    }
    // Les autres clés du fichier sont conservées.
    pf_i18n::settings::save_language(&file, pf_i18n::Lang::En).unwrap();
    assert_eq!(print_setup::load_page_setup(&file), samples[3]);
    // Valeurs absentes, mal formées ou hors bornes : réglage par défaut, fichier intact.
    let ok_margins = r#""margins_mm":{"top":10,"bottom":10,"left":10,"right":10}"#;
    let cases = [
        String::new(),
        "pas du json".to_string(),
        "[]".to_string(),
        "{}".to_string(),
        r#"{"print_setup":null}"#.to_string(),
        r#"{"print_setup":{"format":"a4"}}"#.to_string(),
        format!(r#"{{"print_setup":{{"format":"zz","orientation":"portrait",{ok_margins}}}}}"#),
        format!(
            r#"{{"print_setup":{{"format":"custom","width_mm":10,"height_mm":300,"orientation":"portrait",{ok_margins}}}}}"#
        ),
        r#"{"print_setup":{"format":"a4","orientation":"portrait","margins_mm":{"top":-1,"bottom":10,"left":10,"right":10}}}"#.to_string(),
        r#"{"print_setup":{"format":"a4","orientation":"portrait","margins_mm":{"top":90,"bottom":90,"left":10,"right":10}}}"#.to_string(),
        format!(r#"{{"print_setup":{{"format":"a4","orientation":"sideways",{ok_margins}}}}}"#),
        format!(
            r#"{{"print_setup":{{"format":"roll","width_mm":"large","orientation":"portrait",{ok_margins}}}}}"#
        ),
    ];
    for content in cases {
        std::fs::write(&file, &content).unwrap();
        assert_eq!(
            print_setup::load_page_setup(&file),
            PageSetup::default(),
            "{content}"
        );
        assert_eq!(std::fs::read_to_string(&file).unwrap(), content);
    }
    assert_eq!(
        print_setup::load_page_setup(&dir.join("absent.json")),
        PageSetup::default()
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn us131_le_reglage_n_est_pas_dans_le_projet_ni_une_etape_annulable() {
    let mut h = piece_harness();
    let before = h.state().editor.project().clone();
    let undo_before = h.state().editor.doc.can_undo();
    h.state_mut().apply_page_setup(PageSetup {
        format: PageFormat::A3,
        orientation: Orientation::Landscape,
        margins: Margins::uniform(3.0),
        ..PageSetup::default()
    });
    assert_eq!(*h.state().editor.project(), before);
    assert_eq!(h.state().editor.doc.can_undo(), undo_before);
}

#[test]
fn us131_une_ecriture_impossible_est_signalee_et_le_reglage_reste_applique() {
    let dir = std::env::temp_dir().join(format!("pf-us131-c-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let blocked = dir.join("settings.json");
    std::fs::create_dir_all(&blocked).unwrap();
    let mut h = piece_harness();
    h.state_mut().settings_path = Some(blocked);
    h.state_mut().apply_page_setup(PageSetup {
        format: PageFormat::A0,
        ..PageSetup::default()
    });
    assert_eq!(h.state().page_setup.format, PageFormat::A0);
    assert!(h.state().editor.status.error);
    assert!(h.state().editor.status.text.contains("non enregistrée"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn us131_la_fenetre_fonctionne_en_anglais_avec_les_memes_refus() {
    let mut h = piece_harness();
    pf_i18n::set_lang(pf_i18n::Lang::En);
    h.run_steps(3);
    h.get_by_label("File").click();
    h.run_steps(3);
    h.get_by_label("PDF page setup…").click();
    h.run_steps(4);
    h.get_by_label_contains("Page: 210 × 297 mm");
    h.get_by_label_contains("Printable area: 190 × 277 mm");
    h.get_by_label_contains("Current sheet: 2 page(s), calibration page included");
    h.get_by_label_contains("The real printed scale is not validated");
    h.get_by_label("Landscape").click();
    h.run_steps(3);
    h.get_by_label_contains("Page: 297 × 210 mm");
    h.state_mut().print_dialog.as_mut().unwrap().margins[1] = "300".into();
    h.run_steps(3);
    h.get_by_label_contains("Bottom margin (mm): 300 mm is out of range (0 to 100 mm).");
    h.get_by_label("Apply page setup").click();
    h.run_steps(3);
    assert!(h.state().page_setup.is_default(), "refus sans trace");
    h.state_mut().print_dialog.as_mut().unwrap().margins[1] = "10".into();
    h.run_steps(3);
    h.get_by_label("Apply page setup").click();
    h.run_steps(4);
    h.get_by_label("PDF A4 landscape 1:1");
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
    h.run_steps(3);
    h.get_by_label("PDF A4 paysage 1:1");
}

#[test]
fn us131_chaque_refus_de_mise_en_page_a_un_message_traduit() {
    use pf_export::{ExportError, PageField, PageSetupError};
    let errors = [
        PageSetupError::NotFinite(PageField::Width),
        PageSetupError::SideOutOfRange {
            field: PageField::RollWidth,
            value_mm: 5.0,
        },
        PageSetupError::MarginOutOfRange {
            field: PageField::MarginRight,
            value_mm: -3.0,
        },
        PageSetupError::PrintableTooSmall {
            width_mm: 90.0,
            height_mm: 120.0,
        },
        PageSetupError::PrintableTooSmall {
            width_mm: 90.0,
            height_mm: f64::INFINITY,
        },
        PageSetupError::RollTooLong {
            length_mm: 25_000.0,
        },
        PageSetupError::ContentTooWide {
            content_mm: 900.0,
            printable_mm: 580.0,
        },
    ];
    for lang in [pf_i18n::Lang::Fr, pf_i18n::Lang::En] {
        pf_i18n::set_lang(lang);
        for e in &errors {
            let m = print_setup::page_error(e);
            assert!(
                !m.is_empty() && !m.contains('{') && !m.contains("print."),
                "{m}"
            );
            let full = print_setup::export_error(&ExportError::Page(e.clone()));
            assert!(full.contains(&m), "{full}");
        }
    }
    pf_i18n::set_lang(pf_i18n::Lang::En);
    let m = print_setup::page_error(&errors[6]);
    assert!(
        m.contains("900 mm wide") && m.contains("refused rather than shrunk"),
        "{m}"
    );
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
    let m = print_setup::page_error(&errors[6]);
    assert!(
        m.contains("900 mm de large") && m.contains("refusé plutôt que réduit"),
        "{m}"
    );
}

// ----- US-13.2 : assemblage de pages (recouvrement, repères, plan, économie de papier) ----

use pf_export::AssemblyOptions;

#[test]
fn us132_la_fenetre_expose_recouvrement_et_options_les_applique_hors_projet() {
    let dir = std::env::temp_dir().join(format!("pf-us132-a-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let file = dir.join("settings.json");
    let mut h = piece_harness();
    h.state_mut().settings_path = Some(file.clone());
    let project_before = h.state().editor.project().clone();
    let undo_before = h.state().editor.doc.can_undo();
    open_print_window(&mut h);
    h.get_by_label("Recouvrement (mm)");
    h.get_by_label_contains("Repères d'assemblage");
    h.get_by_label_contains("Plan d'assemblage");
    h.get_by_label_contains("Économiser le papier");
    // Défaut : 10 mm, rien de coché, 2 pages (étalon + pièce).
    {
        let d = h.state().print_dialog.as_ref().unwrap();
        assert_eq!(d.overlap, "10");
        assert!(!d.marks && !d.plan && !d.save_paper);
    }
    h.get_by_label_contains("Feuille courante : 2 page(s), étalon compris");
    // Le plan ajoute une page : le nombre estimé est affiché avant tout export.
    h.get_by_label_contains("Plan d'assemblage").click();
    h.run_steps(3);
    h.get_by_label_contains("Feuille courante : 3 page(s), étalon compris");
    h.get_by_label_contains("Repères d'assemblage").click();
    h.get_by_label_contains("Économiser le papier").click();
    h.run_steps(3);
    h.state_mut().print_dialog.as_mut().unwrap().overlap = "25".into();
    h.run_steps(3);
    // Rien n'est appliqué avant le bouton.
    assert!(h.state().page_setup.is_default());
    print_apply(&mut h);
    let expect = PageSetup {
        overlap_mm: 25.0,
        assembly: AssemblyOptions {
            marks: true,
            plan: true,
            save_paper: true,
        },
        ..PageSetup::default()
    };
    assert_eq!(h.state().page_setup, expect);
    assert!(!h.state().editor.status.error);
    // Mémorisé dans les préférences (clé print_setup), relu à l'identique ; le fichier du
    // projet et l'historique n'en savent rien.
    assert_eq!(print_setup::load_page_setup(&file), expect);
    let saved: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(saved["print_setup"]["overlap_mm"], 25.0);
    assert_eq!(saved["print_setup"]["assembly"]["plan"], true);
    assert_eq!(*h.state().editor.project(), project_before);
    assert_eq!(h.state().editor.doc.can_undo(), undo_before);
    // La fenêtre rouverte reprend le réglage appliqué.
    open_print_window(&mut h);
    {
        let d = h.state().print_dialog.as_ref().unwrap();
        assert_eq!(d.overlap, "25");
        assert!(d.marks && d.plan && d.save_paper);
    }
    // « Valeurs par défaut » remet recouvrement et options à zéro.
    h.get_by_label("Valeurs par défaut (A4, marges de 10 mm)")
        .click();
    h.run_steps(3);
    {
        let d = h.state().print_dialog.as_ref().unwrap();
        assert_eq!(d.overlap, "10");
        assert!(!d.marks && !d.plan && !d.save_paper);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn us132_l_apercu_annonce_pages_omises_avant_l_export() {
    let mut h = piece_harness();
    // Une longue diagonale : la feuille a des coins vides.
    {
        let ed = &mut h.state_mut().editor;
        let sheet = ed.sheet;
        ed.exec(pf_core::Command::AddSegment {
            sheet,
            from: pf_core::PointRef::New(pt(0.0, 0.0)),
            to: pf_core::PointRef::New(pt(900.0, 700.0)),
        })
        .unwrap();
    }
    h.run_steps(3);
    open_print_window(&mut h);
    let full = {
        let setup = h.state().print_dialog.as_ref().unwrap().to_setup().unwrap();
        let scene =
            pf_export::Scene::from_sheet(h.state().editor.project(), h.state().editor.sheet)
                .unwrap();
        pf_export::pdf::page_count_with(std::slice::from_ref(&scene), &setup).unwrap()
    };
    h.get_by_label_contains(&format!(
        "Feuille courante : {full} page(s), étalon compris"
    ));
    h.get_by_label_contains("Économiser le papier").click();
    h.run_steps(3);
    // Moins de pages, et le nombre de pages omises est dit.
    let (pages, omitted) = {
        let setup = h.state().print_dialog.as_ref().unwrap().to_setup().unwrap();
        let scene =
            pf_export::Scene::from_sheet(h.state().editor.project(), h.state().editor.sheet)
                .unwrap();
        let tiles = pf_export::pdf::plan_tiles_with(&scene, &setup).unwrap();
        (
            pf_export::pdf::page_count_with(std::slice::from_ref(&scene), &setup).unwrap(),
            tiles[0].cols * tiles[0].rows - tiles.len(),
        )
    };
    assert!(pages < full, "{pages} pages au lieu de {full}");
    assert!(omitted > 0);
    h.get_by_label_contains(&format!(
        "Feuille courante : {pages} page(s), étalon compris"
    ));
    h.get_by_label_contains(&format!("Pages vides omises : {omitted}"));
}

#[test]
fn us132_un_ancien_fichier_sans_ces_champs_se_charge_avec_les_defauts() {
    let dir = std::env::temp_dir().join(format!("pf-us132-b-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("settings.json");
    // Réglage écrit par la version 13.1 : ni recouvrement ni assemblage.
    std::fs::write(
        &file,
        r#"{"print_setup":{"format":"a3","orientation":"landscape","margins_mm":{"top":5,"bottom":6,"left":7,"right":8}}}"#,
    )
    .unwrap();
    let old = print_setup::load_page_setup(&file);
    assert_eq!(old.format, PageFormat::A3);
    assert_eq!(old.orientation, Orientation::Landscape);
    assert_eq!(old.margins.left_mm, 7.0);
    assert_eq!(old.overlap_mm, 10.0);
    assert_eq!(old.assembly, AssemblyOptions::default());
    // Assemblage partiel : les drapeaux absents valent faux.
    std::fs::write(
        &file,
        r#"{"print_setup":{"format":"a4","orientation":"portrait","margins_mm":{"top":10,"bottom":10,"left":10,"right":10},"overlap_mm":30,"assembly":{"plan":true}}}"#,
    )
    .unwrap();
    let s = print_setup::load_page_setup(&file);
    assert_eq!(s.overlap_mm, 30.0);
    assert!(s.assembly.plan && !s.assembly.marks && !s.assembly.save_paper);
    // Champs présents mais invalides : tout le réglage est ignoré, fichier intact.
    let ok = r#""format":"a4","orientation":"portrait","margins_mm":{"top":10,"bottom":10,"left":10,"right":10}"#;
    for tail in [
        r#""overlap_mm":"dix""#,
        r#""overlap_mm":51"#,
        r#""overlap_mm":-1"#,
        r#""overlap_mm":null"#,
        r#""assembly":"oui""#,
        r#""assembly":{"marks":"oui"}"#,
        r#""assembly":[true]"#,
    ] {
        let content = format!(r#"{{"print_setup":{{{ok},{tail}}}}}"#);
        std::fs::write(&file, &content).unwrap();
        assert_eq!(
            print_setup::load_page_setup(&file),
            PageSetup::default(),
            "{tail}"
        );
        assert_eq!(std::fs::read_to_string(&file).unwrap(), content);
    }
    // Aller-retour de réglages variés.
    for s in [
        PageSetup::default(),
        PageSetup {
            overlap_mm: 0.0,
            assembly: AssemblyOptions {
                marks: true,
                plan: false,
                save_paper: true,
            },
            ..PageSetup::default()
        },
        PageSetup {
            overlap_mm: 50.0,
            assembly: AssemblyOptions {
                marks: false,
                plan: true,
                save_paper: false,
            },
            format: PageFormat::Letter,
            ..PageSetup::default()
        },
    ] {
        assert_eq!(print_setup::from_json(&print_setup::to_json(&s)), s);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn us132_un_recouvrement_invalide_est_refuse_sans_trace() {
    let dir = std::env::temp_dir().join(format!("pf-us132-c-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let file = dir.join("settings.json");
    let mut h = piece_harness();
    h.state_mut().settings_path = Some(file.clone());
    // Sans préférences enregistrées d'abord : aucun fichier ne doit apparaître.
    open_print_window(&mut h);
    h.state_mut().print_dialog.as_mut().unwrap().overlap = "51".into();
    h.run_steps(3);
    h.get_by_label("Appliquer la mise en page").click();
    h.run_steps(3);
    assert!(h.state().page_setup.is_default() && !file.exists());
    // Avec un réglage déjà enregistré : le refus laisse réglage et fichier tels quels.
    h.state_mut().print_dialog = None;
    let saved = PageSetup {
        format: PageFormat::A3,
        ..PageSetup::default()
    };
    h.state_mut().apply_page_setup(saved);
    let file_before = std::fs::read_to_string(&file).unwrap();
    open_print_window(&mut h);
    for (value, needle) in [
        (
            "51",
            "Recouvrement (mm) : 51 mm hors des bornes (0 à 50 mm).",
        ),
        (
            "-1",
            "Recouvrement (mm) : -1 mm hors des bornes (0 à 50 mm).",
        ),
        ("abc", "Recouvrement (mm) : saisir un nombre."),
        ("", "Recouvrement (mm) : saisir un nombre."),
    ] {
        h.state_mut().print_dialog.as_mut().unwrap().overlap = value.into();
        h.run_steps(3);
        h.get_by_label_contains(needle);
        h.get_by_label("Appliquer la mise en page").click();
        h.run_steps(3);
        assert_eq!(h.state().page_setup, saved, "{value} : refus sans trace");
        assert!(h.state().print_dialog.is_some(), "la fenêtre reste ouverte");
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            file_before,
            "{value} : préférences inchangées"
        );
    }
    // Bornes incluses : 0 et 50 acceptés.
    for ok in ["0", "50"] {
        h.state_mut().print_dialog.as_mut().unwrap().overlap = ok.into();
        h.run_steps(3);
        assert!(
            h.state().print_dialog.as_ref().unwrap().to_setup().is_ok(),
            "{ok}"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn us132_la_fenetre_d_assemblage_fonctionne_en_anglais() {
    let mut h = piece_harness();
    pf_i18n::set_lang(pf_i18n::Lang::En);
    h.run_steps(3);
    h.get_by_label("File").click();
    h.run_steps(3);
    h.get_by_label("PDF page setup…").click();
    h.run_steps(4);
    h.get_by_label("Overlap (mm)");
    h.get_by_label("Page assembly");
    h.get_by_label_contains("Assembly marks");
    h.get_by_label_contains("Assembly plan");
    h.get_by_label_contains("Save paper");
    h.get_by_label_contains("The rotation exists only in the PDF");
    h.state_mut().print_dialog.as_mut().unwrap().overlap = "60".into();
    h.run_steps(3);
    h.get_by_label_contains("Overlap (mm): 60 mm is out of range (0 to 50 mm).");
    h.state_mut().print_dialog.as_mut().unwrap().overlap = "10".into();
    h.get_by_label_contains("Assembly plan").click();
    h.run_steps(3);
    h.get_by_label_contains("Current sheet: 3 page(s), calibration page included");
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
    h.run_steps(3);
    h.get_by_label("Recouvrement (mm)");
}

#[test]
fn us132_chaque_refus_de_recouvrement_a_un_message_traduit() {
    use pf_export::{ExportError, PageSetupError};
    let errors = [
        PageSetupError::NotFinite(pf_export::PageField::Overlap),
        PageSetupError::OverlapOutOfRange { value_mm: 60.0 },
        PageSetupError::OverlapTooLarge {
            overlap_mm: 50.0,
            window_mm: 140.0,
        },
    ];
    for lang in [pf_i18n::Lang::Fr, pf_i18n::Lang::En] {
        pf_i18n::set_lang(lang);
        for e in &errors {
            let m = print_setup::page_error(e);
            assert!(
                !m.is_empty() && !m.contains('{') && !m.contains("print."),
                "{m}"
            );
            assert!(print_setup::export_error(&ExportError::Page(e.clone())).contains(&m));
        }
    }
    pf_i18n::set_lang(pf_i18n::Lang::En);
    let m = print_setup::page_error(&errors[2]);
    assert!(
        m.contains("50 mm") && m.contains("140 mm") && m.contains("100 mm"),
        "{m}"
    );
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
    let m = print_setup::page_error(&errors[1]);
    assert!(m.contains("60 mm hors des bornes (0 à 50 mm)"), "{m}");
}

#[test]
fn us132_un_rouleau_garde_les_options_sans_page_de_plan() {
    let mut h = piece_harness();
    open_print_window(&mut h);
    h.get_by_label_contains("Plan d'assemblage").click();
    h.run_steps(3);
    h.get_by_label("Rouleau").click();
    h.run_steps(3);
    // Les options sont sans effet sur un rouleau : pas de page de plan dans le décompte.
    h.get_by_label_contains("page de 98 mm de long, 2 page(s)");
    print_apply(&mut h);
    assert_eq!(
        h.state().page_setup.format,
        PageFormat::Roll { width_mm: 914.0 }
    );
    assert!(h.state().page_setup.assembly.plan);
}

// ----- US-13.3 : étalonnage de l'imprimante (facteurs X / Y, profils) ------------------------

use pf_export::{PrintScale, PrinterProfiles};

fn open_calib_window(h: &mut Harness<'static, App>) {
    h.get_by_label("Étalonner l'imprimante…").click();
    h.run_steps(4);
    assert!(h.state().calib_dialog.is_some());
}

fn fill_calib(h: &mut Harness<'static, App>, name: &str, mx: &str, my: &str) {
    {
        let d = h.state_mut().calib_dialog.as_mut().unwrap();
        d.name = name.into();
        d.measure_x = mx.into();
        d.measure_y = my.into();
    }
    h.run_steps(3);
}

fn pages_for(h: &Harness<'static, App>, setup: &PageSetup) -> usize {
    let scene =
        pf_export::Scene::from_sheet(h.state().editor.project(), h.state().editor.sheet).unwrap();
    pf_export::pdf::page_count_with(std::slice::from_ref(&scene), setup).unwrap()
}

#[test]
fn us133_saisie_des_mesures_facteurs_calcules_profil_cree_active_puis_supprime() {
    let dir = std::env::temp_dir().join(format!("pf-us133-a-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let file = dir.join("settings.json");
    let mut h = piece_harness();
    h.state_mut().settings_path = Some(file.clone());
    // Un segment de 335 mm : 2 pages sans correction, 3 pages avec un facteur X de 1,0989.
    {
        let ed = &mut h.state_mut().editor;
        let sheet = ed.sheet;
        ed.exec(pf_core::Command::AddSegment {
            sheet,
            from: pf_core::PointRef::New(pt(0.0, 0.0)),
            to: pf_core::PointRef::New(pt(335.0, 0.0)),
        })
        .unwrap();
    }
    let project_before = h.state().editor.project().clone();
    h.run_steps(3);
    open_print_window(&mut h);
    h.get_by_label_contains("Profil d'imprimante actif : aucun");
    let plain = pages_for(&h, &PageSetup::default());
    h.get_by_label_contains(&format!(
        "Feuille courante : {plain} page(s), étalon compris"
    ));

    // L'assistant : mesures saisies, facteurs calculés et affichés avant tout enregistrement.
    open_calib_window(&mut h);
    h.get_by_label_contains("Aucun profil enregistré.");
    fill_calib(&mut h, "Bureau", "91", "100.25");
    h.get_by_label_contains("Facteur X : 100 / 91 = 1,098901");
    h.get_by_label_contains("Facteur Y : 100 / 100,25 = 0,997506");
    h.get_by_label_contains("Ce logiciel ne sait pas si l'imprimante est juste");
    assert!(h.state().printer_profiles.list().is_empty());
    assert!(!file.exists(), "rien n'est écrit avant « Enregistrer »");
    h.get_by_label("Enregistrer le profil").click();
    h.run_steps(4);
    {
        let book = &h.state().printer_profiles;
        assert_eq!(book.list().len(), 1);
        assert_eq!(book.list()[0].name, "Bureau");
        assert_eq!(
            book.list()[0].scale,
            PrintScale {
                x: 1.098901,
                y: 0.997506
            }
        );
        assert_eq!(book.active(), None, "créer n'active pas");
    }
    assert!(h.state().page_setup.is_default());
    h.get_by_label_contains("Bureau — X ×1,098901, Y ×0,997506");
    h.get_by_label_contains("Profils enregistrés (1/16)");
    assert!(h
        .state()
        .editor
        .status
        .text
        .contains("« Bureau » enregistré"));

    // Activation : la correction passe dans le réglage et dans l'aperçu du nombre de pages.
    h.get_by_label("Activer").click();
    h.run_steps(4);
    let scale = PrintScale {
        x: 1.098901,
        y: 0.997506,
    };
    assert_eq!(h.state().page_setup.scale, scale);
    h.get_by_label_contains("Profil d'imprimante actif : Bureau (X ×1,098901, Y ×0,997506)");
    h.get_by_label_contains("Correction d'échelle appliquée : X ×1,098901, Y ×0,997506");
    let corrected = pages_for(
        &h,
        &PageSetup {
            scale,
            ..PageSetup::default()
        },
    );
    assert!(
        corrected > plain,
        "la correction ajoute des pages : {corrected} contre {plain}"
    );
    h.get_by_label_contains(&format!(
        "Feuille courante : {corrected} page(s), étalon compris"
    ));
    // Préférences : profil et profil actif dans la clé print_setup, rien dans le projet.
    let saved: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(saved["print_setup"]["active_profile"], "Bureau");
    assert_eq!(saved["print_setup"]["profiles"][0]["x"], 1.098901);
    assert_eq!(*h.state().editor.project(), project_before);
    assert_eq!(print_setup::load_page_setup(&file).scale, scale);
    assert_eq!(
        print_setup::load_profiles(&file),
        h.state().printer_profiles
    );

    // « Appliquer » garde le profil actif ; l'étalon renforcé s'ajoute sans toucher au profil.
    h.state_mut().calib_dialog = None; // la fenêtre d'étalonnage recouvre celle de mise en page
    h.run_steps(3);
    h.get_by_label_contains("Étalon renforcé").click();
    h.run_steps(3);
    print_apply(&mut h);
    assert_eq!(h.state().page_setup.scale, scale);
    assert!(h.state().page_setup.control_marks);
    assert_eq!(print_setup::load_page_setup(&file), h.state().page_setup);
    // L'export réel porte la correction.
    let scene =
        pf_export::Scene::from_sheet(h.state().editor.project(), h.state().editor.sheet).unwrap();
    let out = pf_export::pdf::export_pdf_with(&[scene], &h.state().page_setup).unwrap();
    assert!(out.page_count >= corrected);
    let text = String::from_utf8(out.bytes).unwrap();
    // Texte écrit en CID hexadécimaux (police embarquée, US-06.6) : « correction d'échelle X × ».
    let font = pf_export::font::embedded();
    let wanted: String = "correction d'échelle X ×"
        .chars()
        .map(|c| format!("{:04X}", font.cid(c)))
        .collect();
    assert!(text.contains(&wanted), "{text}");

    // Désactivation, réactivation puis suppression.
    open_print_window(&mut h);
    open_calib_window(&mut h);
    h.get_by_label_contains("(actif)");
    h.get_by_label("Désactiver").click();
    h.run_steps(4);
    assert!(h.state().page_setup.scale.is_identity());
    assert_eq!(h.state().printer_profiles.active(), None);
    h.get_by_label("Activer").click();
    h.run_steps(4);
    h.get_all_by_label("Supprimer")
        .collect::<Vec<_>>()
        .last()
        .unwrap()
        .click();
    h.run_steps(4);
    assert!(h.state().printer_profiles.list().is_empty());
    assert!(h.state().page_setup.scale.is_identity());
    h.get_by_label_contains("Aucun profil enregistré.");
    let saved: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert!(saved["print_setup"].get("profiles").is_none());
    assert!(saved["print_setup"].get("active_profile").is_none());
    assert_eq!(*h.state().editor.project(), project_before);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn us133_modifier_et_renommer_un_profil_ecart_trop_grand_refuse_avec_un_message() {
    let dir = std::env::temp_dir().join(format!("pf-us133-b-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let file = dir.join("settings.json");
    let mut h = piece_harness();
    h.state_mut().settings_path = Some(file.clone());
    open_print_window(&mut h);
    open_calib_window(&mut h);
    // Écart de plus de 10 % : message clair, bouton sans effet, rien d'écrit.
    for (mx, my, expect) in [
        (
            "89",
            "100",
            "Facteur X : 1,123596 hors des bornes (0,9 à 1,1)",
        ),
        (
            "100",
            "111.2",
            "Facteur Y : 0,899281 hors des bornes (0,9 à 1,1)",
        ),
        ("abc", "100", "Mesure X : saisir un nombre."),
        ("100", "0", "Mesure Y : 0 mm n'est pas une longueur valide."),
        (
            "-5",
            "100",
            "Mesure X : -5 mm n'est pas une longueur valide.",
        ),
    ] {
        fill_calib(&mut h, "Test", mx, my);
        h.get_by_label_contains(expect);
        h.get_by_label("Enregistrer le profil").click();
        h.run_steps(3);
        assert!(h.state().printer_profiles.list().is_empty(), "{mx} {my}");
        assert!(!file.exists(), "{mx} {my} : aucun fichier écrit");
    }
    fill_calib(&mut h, "Test", "89", "100");
    h.get_by_label_contains("entre 90,91 et 111,11 mm");
    // Création puis modification (renommer et changer la mesure).
    fill_calib(&mut h, "Salon", "100", "99");
    h.get_by_label("Enregistrer le profil").click();
    h.run_steps(4);
    h.get_by_label("Modifier").click();
    h.run_steps(4);
    {
        let d = h.state().calib_dialog.as_ref().unwrap();
        assert_eq!((d.name.as_str(), d.editing), ("Salon", Some(0)));
        assert_eq!((d.measure_x.as_str(), d.measure_y.as_str()), ("100", "99"));
    }
    h.get_by_label("Mettre à jour le profil");
    fill_calib(&mut h, "Salon 2", "100.5", "99");
    h.get_by_label("Mettre à jour le profil").click();
    h.run_steps(4);
    let book = &h.state().printer_profiles;
    assert_eq!(book.list().len(), 1);
    assert_eq!(book.list()[0].name, "Salon 2");
    assert_eq!(book.list()[0].scale.x, 0.995025);
    assert_eq!(print_setup::load_profiles(&file), *book);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn us133_valeurs_invalides_refusees_preferences_identiques_octet_pour_octet() {
    let dir = std::env::temp_dir().join(format!("pf-us133-c-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let file = dir.join("settings.json");
    let mut h = piece_harness();
    h.state_mut().settings_path = Some(file.clone());
    let good = PrintScale::new(1.01, 0.99).unwrap();
    h.state_mut().profile_save("Bureau", good, None).unwrap();
    h.state_mut().profile_activate(Some(0)).unwrap();
    let bytes = std::fs::read(&file).unwrap();
    let (book, setup) = (h.state().printer_profiles.clone(), h.state().page_setup);
    let long = "x".repeat(41);
    let nan = PrintScale {
        x: f64::NAN,
        y: 1.0,
    };
    let zero = PrintScale { x: 1.0, y: 0.0 };
    let wide = PrintScale { x: 1.2, y: 1.0 };
    let inf = PrintScale {
        x: 1.0,
        y: f64::INFINITY,
    };
    let bad: Vec<(&str, PrintScale, Option<usize>)> = vec![
        ("", good, None),
        ("   ", good, None),
        ("bureau", good, None),
        (long.as_str(), good, None),
        ("a\u{7}b", good, None),
        ("Autre", nan, None),
        ("Autre", zero, None),
        ("Autre", wide, None),
        ("Autre", inf, Some(0)),
        ("Autre", good, Some(9)),
    ];
    for (name, scale, editing) in bad {
        let e = h
            .state_mut()
            .profile_save(name, scale, editing)
            .unwrap_err();
        assert!(!e.is_empty());
        assert_eq!(std::fs::read(&file).unwrap(), bytes, "{name:?}");
        assert_eq!(h.state().printer_profiles, book);
        assert_eq!(h.state().page_setup, setup);
    }
    assert!(h.state_mut().profile_activate(Some(5)).is_err());
    assert!(h.state_mut().profile_delete(5).is_err());
    assert_eq!(std::fs::read(&file).unwrap(), bytes);
    // 17e profil : refusé, fichier intact.
    for i in 1..16 {
        h.state_mut()
            .profile_save(&format!("P{i}"), good, None)
            .unwrap();
    }
    assert_eq!(h.state().printer_profiles.list().len(), 16);
    let full = std::fs::read(&file).unwrap();
    let e = h.state_mut().profile_save("P17", good, None).unwrap_err();
    assert!(e.contains("Au plus 16 profils"), "{e}");
    assert_eq!(std::fs::read(&file).unwrap(), full);
    // Un export refusé pour un facteur hors bornes est expliqué dans la langue de l'interface.
    let refused = PageSetup {
        scale: wide,
        ..PageSetup::default()
    };
    let e = refused.validate().unwrap_err();
    let m = print_setup::export_error(&pf_export::ExportError::Page(e));
    assert!(
        m.contains("Facteur X : 1,2 hors des bornes (0,9 à 1,1)"),
        "{m}"
    );
    // Préférences inaccessibles (un dossier, pas un fichier) : refus sans rien adopter.
    h.state_mut().settings_path = Some(dir.clone());
    let before = h.state().printer_profiles.clone();
    assert!(h.state_mut().profile_activate(None).is_err());
    assert_eq!(h.state().printer_profiles, before);
    assert!(!h.state().page_setup.scale.is_identity());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn us133_ancien_fichier_sans_profil_valeurs_invalides_ecartees_aller_retour() {
    let dir = std::env::temp_dir().join(format!("pf-us133-d-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("settings.json");
    // Fichier écrit par la 13.2 : ni profil ni étalon renforcé.
    let old = r#"{"print_setup":{"format":"a3","orientation":"landscape","margins_mm":{"top":5,"bottom":6,"left":7,"right":8},"overlap_mm":20,"assembly":{"plan":true}}}"#;
    std::fs::write(&file, old).unwrap();
    let s = print_setup::load_page_setup(&file);
    assert_eq!(s.format, PageFormat::A3);
    assert!(s.scale.is_identity() && !s.control_marks);
    assert_eq!(print_setup::load_profiles(&file), PrinterProfiles::new());
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        old,
        "lecture seule"
    );
    // Profils : invalides, en double ou hors bornes écartés ; actif retrouvé par son nom.
    let ok = r#""format":"a4","orientation":"portrait","margins_mm":{"top":10,"bottom":10,"left":10,"right":10}"#;
    let profiles = r#""profiles":[{"name":"A","x":1.01,"y":0.99},{"name":"a","x":1.0,"y":1.0},{"name":"B","x":2.0,"y":1.0},{"name":"","x":1.0,"y":1.0},{"name":"C","x":"1","y":1.0},{"x":1.0,"y":1.0},{"name":"D","x":0.95,"y":1.05}],"active_profile":"D","control_marks":true"#;
    std::fs::write(&file, format!(r#"{{"print_setup":{{{ok},{profiles}}}}}"#)).unwrap();
    let book = print_setup::load_profiles(&file);
    let names: Vec<&str> = book.list().iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["A", "D"]);
    let s = print_setup::load_page_setup(&file);
    assert_eq!(s.scale, PrintScale { x: 0.95, y: 1.05 });
    assert!(s.control_marks);
    // Profil actif inconnu : aucune correction. Étalon renforcé mal formé : réglage ignoré.
    std::fs::write(
        &file,
        format!(
            r#"{{"print_setup":{{{ok},"profiles":[{{"name":"A","x":1.01,"y":1.0}}],"active_profile":"Z"}}}}"#
        ),
    )
    .unwrap();
    assert!(print_setup::load_page_setup(&file).scale.is_identity());
    assert_eq!(print_setup::load_profiles(&file).list().len(), 1);
    std::fs::write(
        &file,
        format!(r#"{{"print_setup":{{{ok},"control_marks":"oui"}}}}"#),
    )
    .unwrap();
    assert_eq!(print_setup::load_page_setup(&file), PageSetup::default());
    // Aller-retour complet : profils + réglage.
    let mut book = PrinterProfiles::new();
    book.add("Bureau", PrintScale::from_measures(99.5, 100.25).unwrap())
        .unwrap();
    book.add("Atelier", PrintScale::new(0.95, 1.04).unwrap())
        .unwrap();
    book.set_active(Some(1)).unwrap();
    let setup = PageSetup {
        scale: book.active_scale(),
        control_marks: true,
        ..PageSetup::default()
    };
    let value = print_setup::settings_value(&setup, &book);
    pf_i18n::settings::save_value(&file, print_setup::SETTINGS_KEY, value).unwrap();
    assert_eq!(print_setup::load_page_setup(&file), setup);
    assert_eq!(print_setup::load_profiles(&file), book);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn us133_textes_en_anglais_et_aucune_promesse_de_calibration() {
    let mut h = piece_harness();
    pf_i18n::set_lang(pf_i18n::Lang::En);
    h.run_steps(3);
    h.get_by_label("File").click();
    h.run_steps(3);
    h.get_by_label("PDF page setup…").click();
    h.run_steps(4);
    assert!(h.state().print_dialog.is_some());
    h.get_by_label_contains("Active printer profile: none");
    h.get_by_label_contains("not validated");
    h.get_by_label("Calibrate printer…").click();
    h.run_steps(4);
    fill_calib(&mut h, "Home", "99.5", "99.5");
    h.get_by_label_contains("Factor X: 100 / 99.5 = 1.005025");
    h.get_by_label_contains("cannot know whether the printer is accurate");
    h.get_by_label("Save profile").click();
    h.run_steps(4);
    h.get_by_label_contains("Home — X ×1.005025, Y ×1.005025");
    // « Delete » existe aussi ailleurs dans l'interface : prendre le dernier (ligne du profil).
    h.get_all_by_label("Delete")
        .collect::<Vec<_>>()
        .last()
        .unwrap()
        .click();
    h.run_steps(4);
    h.get_by_label_contains("No saved profile.");
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
    h.run_steps(3);
}

// ----- US-13.4 : contenu de la sortie (calques, couture / coupe, cotes, aperçu fidèle) ---------

use pf_export::{OutputCategory, OutputContent};

/// Feuille de la pièce de test : rectangle de coupe + une diagonale de construction lointaine,
/// qui allonge la feuille de plusieurs pages.
fn content_harness() -> Harness<'static, App> {
    let mut h = piece_harness();
    {
        let ed = &mut h.state_mut().editor;
        let sheet = ed.sheet;
        let ids: Vec<_> = ed
            .project()
            .sheet(sheet)
            .unwrap()
            .entities
            .iter()
            .map(|e| e.id)
            .collect();
        for entity in ids {
            ed.exec(pf_core::Command::SetPathRole {
                sheet,
                entity,
                role: pf_core::PathRole::CuttingContour,
            })
            .unwrap();
        }
        ed.exec(pf_core::Command::AddSegment {
            sheet,
            from: pf_core::PointRef::New(pt(0.0, 0.0)),
            to: pf_core::PointRef::New(pt(900.0, 700.0)),
        })
        .unwrap();
    }
    h.run_steps(3);
    h
}

fn open_content_section(h: &mut Harness<'static, App>) {
    open_print_window(h);
    h.get_by_label("Contenu de la sortie").click();
    h.run_steps(3);
}

/// Avec la section ouverte, la fenêtre dépasse l'écran de test : le bouton est activé par
/// l'accessibilité plutôt que par un clic de souris.
fn print_apply_open(h: &mut Harness<'static, App>) {
    h.get_by_label("Appliquer la mise en page")
        .click_accesskit();
    h.run_steps(4);
}

fn pages_for_content(h: &Harness<'static, App>, content: &OutputContent) -> usize {
    let scene = content
        .scene(
            h.state().editor.project(),
            h.state().editor.sheet,
            pf_export::SceneOptions::default(),
        )
        .unwrap();
    pf_export::pdf::page_count_content(std::slice::from_ref(&scene), &h.state().page_setup, content)
        .unwrap()
}

#[test]
fn us134_la_fenetre_expose_le_contenu_de_la_sortie_et_l_applique_hors_projet() {
    let dir = std::env::temp_dir().join(format!("pf-us134-a-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let file = dir.join("settings.json");
    let mut h = piece_harness();
    h.state_mut().settings_path = Some(file.clone());
    let project_before = h.state().editor.project().clone();
    let undo_before = h.state().editor.doc.can_undo();
    open_print_window(&mut h);
    // Replié par défaut (la fenêtre garde sa hauteur) ; le contenu par défaut = tout inclus.
    assert!(h.state().output_content.is_default());
    h.get_by_label("Contenu de la sortie").click();
    h.run_steps(3);
    h.get_by_label_contains("Taille : la taille unique du projet");
    // Cases groupées, avec le nombre d'éléments de la feuille courante (rectangle : 4 tracés de
    // construction, 4 points nommés).
    h.get_by_label("Tracés de construction (4)");
    h.get_by_label("Points (repères) (4)");
    h.get_by_label("Noms des points (4)");
    h.get_by_label("Contours de coupe (0)");
    h.get_by_label("Contours de couture (0)");
    h.get_by_label("Cotes complètes (0)");
    h.get_by_label("Pinces (jambes) (0)");
    h.get_by_label("Étiquettes de pièce (0)");
    h.get_by_label("Cotations (mesures) (0)");
    h.get_by_label("Crans (0)");
    h.get_by_label("Couture seule (sans coupe)");
    h.get_by_label("Coupe seule (sans couture)");
    // Décocher la construction : rien n'est appliqué avant le bouton, le récapitulatif le dit.
    h.get_by_label("Tracés de construction (4)")
        .click_accesskit();
    h.run_steps(3);
    assert!(!h
        .state()
        .print_dialog
        .as_ref()
        .unwrap()
        .content
        .includes(OutputCategory::Construction));
    assert!(h.state().output_content.is_default());
    assert!(!file.exists());
    h.get_by_label_contains("Contenu retenu pour la feuille courante : 8 élément(s) sur 12.");
    h.get_by_label_contains("Liste de ce qui sera écrit, pas un rendu papier");
    // Sans les points, plus de noms de points : la remarque le dit.
    h.get_by_label("Points (repères) (4)").click_accesskit();
    h.run_steps(3);
    h.get_by_label_contains("Les noms de points ne sont écrits que si les points le sont.");
    print_apply_open(&mut h);
    let expect = OutputContent::default()
        .without(OutputCategory::Construction)
        .without(OutputCategory::Points);
    assert_eq!(h.state().output_content, expect);
    assert!(
        !h.state().editor.status.error,
        "{}",
        h.state().editor.status.text
    );
    // Préférence enregistrée, relue à l'identique ; le projet et l'historique n'en savent rien.
    assert_eq!(print_setup::load_output_content(&file), expect);
    let saved: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(
        saved["output_content"],
        serde_json::json!({"size": "project", "excluded": ["construction", "points"]})
    );
    assert_eq!(*h.state().editor.project(), project_before);
    assert_eq!(h.state().editor.doc.can_undo(), undo_before);
    // La fenêtre rouverte reprend le choix, et « Valeurs par défaut » le remet à zéro.
    open_print_window(&mut h);
    assert_eq!(h.state().print_dialog.as_ref().unwrap().content, expect);
    h.get_by_label("Valeurs par défaut (A4, marges de 10 mm)")
        .click_accesskit();
    h.run_steps(3);
    assert!(h
        .state()
        .print_dialog
        .as_ref()
        .unwrap()
        .content
        .is_default());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn us134_l_apercu_et_l_export_utilisent_exactement_la_meme_scene() {
    let mut h = content_harness();
    let default_pages = pages_for_content(&h, &OutputContent::default());
    open_content_section(&mut h);
    h.get_by_label_contains(&format!(
        "Feuille courante : {default_pages} page(s), étalon compris"
    ));
    // Sans la construction et sans les points, il ne reste que le rectangle : une page de patron.
    let content = OutputContent::default()
        .without(OutputCategory::Construction)
        .without(OutputCategory::Points);
    let pages = pages_for_content(&h, &content);
    assert!(
        pages < default_pages,
        "{pages} pages au lieu de {default_pages}"
    );
    h.state_mut().print_dialog.as_mut().unwrap().content = content;
    h.run_steps(3);
    h.get_by_label_contains(&format!(
        "Feuille courante : {pages} page(s), étalon compris"
    ));
    h.get_by_label_contains("Contenu retenu pour la feuille courante : 4 élément(s) sur 15.");
    print_apply_open(&mut h);
    assert_eq!(h.state().output_content, content);
    // L'inspecteur annonce le même nombre de pages.
    h.get_by_label_contains(&format!("{pages} page(s), étalon compris"));
    // Mêmes scènes et mêmes octets que l'export réel (PDF et SVG), construits ici sans passer
    // par les fonctions de l'application.
    let (scenes, skipped) = h.state().pdf_scenes(false).unwrap();
    assert_eq!(skipped, 0);
    let project = h.state().editor.project().clone();
    let sheet = h.state().editor.sheet;
    let full = pf_export::Scene::from_sheet(&project, sheet).unwrap();
    let by_hand = content.apply(&full).unwrap();
    assert_eq!(scenes, vec![by_hand.clone()]);
    let setup = h.state().page_setup;
    let real =
        pf_export::pdf::export_pdf_content(&scenes, &setup, &h.state().output_content).unwrap();
    let expected =
        pf_export::pdf::export_pdf_content(std::slice::from_ref(&by_hand), &setup, &content)
            .unwrap();
    assert_eq!(real.bytes, expected.bytes);
    assert_eq!(real.page_count, pages);
    let svg = pf_export::svg::export_svg(&h.state().export_scene(sheet).unwrap()).unwrap();
    assert_eq!(svg, pf_export::svg::export_svg(&by_hand).unwrap());
    // Le contenu par défaut redonne l'export d'avant, à l'octet.
    h.state_mut().output_content = OutputContent::default();
    let (default_scenes, _) = h.state().pdf_scenes(false).unwrap();
    assert_eq!(default_scenes, vec![full.clone()]);
    assert_eq!(
        pf_export::pdf::export_pdf_content(&default_scenes, &setup, &OutputContent::default())
            .unwrap()
            .bytes,
        pf_export::pdf::export_pdf_with(std::slice::from_ref(&full), &setup)
            .unwrap()
            .bytes
    );
    // Le récapitulatif de la fenêtre est celui du cœur : 4 contours de coupe + 1 tracé de
    // construction + 5 points + 5 noms = 15 éléments, 4 retenus.
    let summary = content.summary(&full);
    assert_eq!(summary.total_available(), 15);
    assert_eq!(summary.total_kept(), 4);
    // Les noms de points restent demandés alors que les points sont exclus : la remarque le dit.
    assert_eq!(
        summary.notes,
        vec![pf_export::ContentNote::LabelsWithoutPoints]
    );
}

#[test]
fn tranche69_les_cases_pinces_et_etiquettes_comptent_leurs_elements_et_filtrent() {
    let mut h = piece_harness();
    {
        // Pince de 8 mm d'ouverture et de 20 mm de long sur le bord bas, fiche renseignée.
        let ed = &mut h.state_mut().editor;
        let sheet = ed.sheet;
        let piece = ed.project().pieces[0].id;
        ed.exec(pf_core::Command::AddPieceDart {
            piece,
            apex: pf_core::PointRef::New(pt(50.0, 20.0)),
            toward: pf_core::PointRef::New(pt(50.0, 0.0)),
            length_mm: 20.0,
            opening_mm: 8.0,
            fixed: pf_geometry::Side::Left,
        })
        .unwrap();
        ed.exec(pf_core::Command::SetPieceCard {
            piece,
            name: "Devant".into(),
            code: Some("D-1".into()),
            quantity: 1,
            mirrored_pair: false,
            material: None,
            category: None,
            comment: None,
        })
        .unwrap();
        let _ = sheet;
    }
    h.run_steps(3);
    h.set_size(egui::vec2(1280.0, 1600.0));
    open_content_section(&mut h);
    // Une pince = un élément ; l'étiquette (nom, code, quantité) = un élément.
    h.get_by_label("Pinces (jambes) (1)");
    h.get_by_label("Étiquettes de pièce (1)");
    h.get_by_label("Pinces (jambes) (1)").click_accesskit();
    h.run_steps(3);
    let d = h.state().print_dialog.as_ref().unwrap();
    assert!(!d.content.includes(OutputCategory::Darts));
    assert!(d.content.includes(OutputCategory::PieceLabels));
}

#[test]
fn us134_remarques_couture_seule_coupe_seule_et_selection_vide() {
    let mut h = content_harness();
    // Fenêtre haute : la fenêtre de mise en page défile désormais, hors zone visible rien n'est lu.
    h.set_size(egui::vec2(1280.0, 1600.0));
    open_content_section(&mut h);
    // Coupe seule : la feuille a des contours de coupe, pas de remarque.
    h.get_by_label("Coupe seule (sans couture)").click();
    h.run_steps(3);
    assert!(h
        .query_all_by_label_contains("n'a aucun tracé de rôle coupe")
        .next()
        .is_none());
    // Couture seule : la feuille n'a aucun contour de couture, la remarque le dit, rien n'est
    // inventé et le reste (coupe exclue d'office) s'exporte encore.
    h.get_by_label("Couture seule (sans coupe)").click();
    h.run_steps(3);
    h.get_by_label_contains("Couture seule : cette feuille n'a aucun tracé de rôle couture.");
    // Tout décoché : refus annoncé en rouge dans l'aperçu.
    {
        let d = h.state_mut().print_dialog.as_mut().unwrap();
        d.content = OutputCategory::ALL
            .into_iter()
            .fold(OutputContent::default(), |c, cat| c.without(cat));
    }
    h.run_steps(3);
    h.get_by_label_contains("Ce choix ne laisse rien à exporter sur la feuille courante");
}

#[test]
fn us134_un_ancien_fichier_ou_une_valeur_douteuse_donne_tout_inclus_sans_toucher_au_fichier() {
    let dir = std::env::temp_dir().join(format!("pf-us134-b-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("settings.json");
    // Ancien fichier : réglage de page seulement, aucune clé de contenu.
    std::fs::write(
        &file,
        r#"{"print_setup":{"format":"a3","orientation":"landscape","margins_mm":{"top":5,"bottom":6,"left":7,"right":8}}}"#,
    )
    .unwrap();
    assert_eq!(
        print_setup::load_output_content(&file),
        OutputContent::default()
    );
    assert!(print_setup::load_output_content(&dir.join("absent.json")).is_default());
    // Valeur valide ; une clé inconnue en plus est tolérée (rétro et avant-compatible).
    let write = |value: &str| {
        std::fs::write(&file, format!(r#"{{"output_content":{value}}}"#)).unwrap();
        std::fs::read(&file).unwrap()
    };
    let before = write(r#"{"excluded":["notches","dimensions"],"size":"project","futur":1}"#);
    assert_eq!(
        print_setup::load_output_content(&file),
        OutputContent::default()
            .without(OutputCategory::Notches)
            .without(OutputCategory::Dimensions)
    );
    assert_eq!(
        std::fs::read(&file).unwrap(),
        before,
        "lire ne modifie jamais le fichier"
    );
    // Toute valeur douteuse : tout est inclus (jamais du contenu écarté sur une valeur illisible).
    for bad in [
        r#""oui""#,
        "null",
        "[]",
        "3",
        r#"{"excluded":"notches"}"#,
        r#"{"excluded":["inconnue"]}"#,
        r#"{"excluded":["notches",3]}"#,
        r#"{"excluded":null}"#,
        r#"{"size":"all"}"#,
        r#"{"size":3}"#,
        r#"{"excluded":["notches"],"size":"nid"}"#,
    ] {
        let bytes = write(bad);
        assert!(
            print_setup::load_output_content(&file).is_default(),
            "{bad} devrait donner tout inclus"
        );
        assert_eq!(std::fs::read(&file).unwrap(), bytes);
    }
    // Aller-retour de toutes les combinaisons d'une série.
    for mask in 0u32..(1 << OutputCategory::ALL.len()) {
        let mut c = OutputContent::default();
        for (i, cat) in OutputCategory::ALL.into_iter().enumerate() {
            c.set(cat, mask & (1 << i) == 0);
        }
        assert_eq!(
            print_setup::content_from_json(&print_setup::content_to_json(&c)),
            c
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn us134_enregistrer_le_contenu_garde_le_reglage_de_page_et_un_echec_n_adopte_rien() {
    let dir = std::env::temp_dir().join(format!("pf-us134-c-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("settings.json");
    let mut h = piece_harness();
    h.state_mut().settings_path = Some(file.clone());
    open_print_window(&mut h);
    h.get_by_label("A3").click();
    h.run_steps(3);
    print_apply(&mut h);
    let page_json_before: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    let content = OutputContent::default().without(OutputCategory::Notches);
    h.state_mut().apply_output_content(content);
    let after: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(after["print_setup"], page_json_before["print_setup"]);
    assert_eq!(h.state().output_content, content);
    // Écriture impossible (le « dossier » des réglages est un fichier) : rien n'est adopté.
    let blocker = dir.join("bloc");
    std::fs::write(&blocker, "x").unwrap();
    h.state_mut().settings_path = Some(blocker.join("settings.json"));
    h.state_mut()
        .apply_output_content(OutputContent::default().without(OutputCategory::Buttons));
    assert_eq!(
        h.state().output_content,
        content,
        "échec d'écriture : rien d'adopté"
    );
    assert!(h.state().editor.status.error);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn us134_un_export_sans_rien_a_ecrire_est_refuse_sans_aucune_trace() {
    let dir = std::env::temp_dir().join(format!("pf-us134-d-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("settings.json");
    std::fs::write(&file, r#"{"print_setup":{"format":"a4","orientation":"portrait","margins_mm":{"top":10,"bottom":10,"left":10,"right":10}}}"#).unwrap();
    let settings_before = std::fs::read(&file).unwrap();
    let mut h = piece_harness();
    h.state_mut().settings_path = Some(file.clone());
    let project_before = h.state().editor.project().clone();
    let undo_before = h.state().editor.doc.can_undo();
    h.state_mut().output_content = OutputCategory::ALL
        .into_iter()
        .fold(OutputContent::default(), |c, cat| c.without(cat));
    h.run_steps(3);
    let sheet = h.state().editor.sheet;
    assert!(matches!(
        h.state().export_scene(sheet),
        Err(pf_export::ExportError::NothingSelected(_))
    ));
    let err = h.state().pdf_scenes(false).unwrap_err();
    assert!(err.contains("ne laisse rien à exporter"), "{err}");
    // Par le menu : le refus a lieu avant tout dialogue de fichier.
    for entry in [
        "Exporter la feuille en PDF (1:1)…",
        "Exporter la feuille en SVG (mm)…",
        "Exporter toutes les feuilles en PDF…",
    ] {
        h.get_by_label("Fichier").click();
        h.run_steps(3);
        h.get_by_label(entry).click();
        h.run_steps(4);
        assert!(h.state().editor.status.error, "{entry}");
        assert!(
            h.state()
                .editor
                .status
                .text
                .contains("ne laisse rien à exporter"),
            "{entry} : {}",
            h.state().editor.status.text
        );
    }
    // L'inspecteur le dit aussi.
    h.get_by_label_contains("Le contenu de sortie choisi ne laisse rien à exporter.");
    // Le journal d'erreurs (US-17.5) ajoute sa clé : seules les préférences sont comparées.
    let mut after: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    after.as_object_mut().unwrap().remove("error_log");
    assert_eq!(
        after,
        serde_json::from_slice::<serde_json::Value>(&settings_before).unwrap(),
        "préférences inchangées"
    );
    assert_eq!(*h.state().editor.project(), project_before);
    assert_eq!(h.state().editor.doc.can_undo(), undo_before);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn us134_textes_en_anglais_et_aucune_promesse_de_rendu_papier() {
    let mut h = content_harness();
    h.state_mut().output_content = OutputContent::default().without(OutputCategory::Axes);
    pf_i18n::set_lang(pf_i18n::Lang::En);
    h.run_steps(3);
    h.get_by_label("File").click();
    h.run_steps(3);
    h.get_by_label("PDF page setup…").click();
    h.run_steps(4);
    // Contenu non défini par défaut : la section s'ouvre d'elle-même.
    h.get_by_label("Output content");
    h.get_by_label_contains("Size: the project's single size.");
    h.get_by_label("Construction lines (1)");
    h.get_by_label("Cutting outlines (4)");
    h.get_by_label("Reference axes and guide lines (0)");
    h.get_by_label("Point names (5)");
    h.get_by_label("Sewing only (no cutting)");
    h.get_by_label_contains("of what will be written, not a paper rendering");
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
    h.run_steps(3);
}

// ----- US-12.1 : export DXF de type AAMA / ASTM -----------------------------------------------

fn open_dxf_window(h: &mut Harness<'static, App>, label: &str) {
    let menu = if label.starts_with("Export pieces") {
        "File"
    } else {
        "Fichier"
    };
    h.get_by_label(menu).click();
    h.run_steps(3);
    h.get_by_label(label).click();
    h.run_steps(4);
}

#[test]
fn us121_le_menu_montre_le_rapport_puis_ecrit_le_fichier_sans_toucher_au_projet() {
    let mut h = piece_harness();
    let project_before = h.state().editor.project().clone();
    let undo_before = h.state().editor.doc.can_undo();
    open_dxf_window(&mut h, "Exporter les pièces en DXF de type AAMA / ASTM…");
    assert!(h.state().dxf_dialog.is_some());
    // Rapport avant écriture : pièces, couches, rien d'omis, remarques ; aucune promesse.
    h.get_by_label_contains("1 pièce(s) écrite(s), 2 couche(s) utilisée(s), 3 entité(s)");
    h.get_by_label_contains("Couche 1 — contour de pièce : 1 entité(s)");
    h.get_by_label_contains("Couche 84 — texte : 2 entité(s)");
    h.get_by_label("Rien n'a été omis.");
    h.get_by_label_contains("pas de droit-fil");
    open_notes(&mut h);
    h.get_by_label_contains("n'a pas été vérifiée");
    h.get_by_label_contains("Aucun fichier de règles de gradation");
    // Écriture (le chemin est injecté : la boîte système n'est pas pilotable ici).
    let dir = std::env::temp_dir().join(format!("pf-us121-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("devant.dxf");
    assert!(h.state_mut().write_dxf_to(&path));
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.starts_with("  0\nSECTION\n  2\nHEADER\n"));
    assert!(text.ends_with("  0\nEOF\n"));
    assert!(text.contains("$INSUNITS\n 70\n4\n"));
    assert!(text.contains("Piece Name: Devant"));
    assert!(h.state().editor.status.text.contains("DXF exporté"));
    assert!(!h.state().editor.status.error);
    h.get_by_label("Fermer sans enregistrer").click();
    h.run_steps(3);
    assert!(h.state().dxf_dialog.is_none());
    assert_eq!(
        *h.state().editor.project(),
        project_before,
        "projet inchangé"
    );
    assert_eq!(h.state().editor.doc.can_undo(), undo_before);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn us121_projet_sans_piece_refuse_sans_fenetre_ni_fichier() {
    let mut h = harness();
    let project_before = h.state().editor.project().clone();
    open_dxf_window(&mut h, "Exporter les pièces en DXF de type AAMA / ASTM…");
    assert!(h.state().dxf_dialog.is_none());
    assert!(h.state().editor.status.error);
    assert!(h
        .state()
        .editor
        .status
        .text
        .contains("Aucune pièce à exporter en DXF"));
    // Sans fenêtre, il n'y a rien à écrire : aucune écriture possible.
    let dir = std::env::temp_dir().join(format!("pf-us121-b-{}", std::process::id()));
    let path = dir.join("rien.dxf");
    assert!(!h.state_mut().write_dxf_to(&path));
    assert!(!path.exists());
    assert_eq!(*h.state().editor.project(), project_before);
}

#[test]
fn us121_omissions_signalees_et_textes_en_anglais() {
    let mut h = piece_harness();
    {
        // Une pièce-étiquette sans contour : écrite nulle part, mais signalée.
        let ed = &mut h.state_mut().editor;
        let sheet = ed.sheet;
        ed.exec(pf_core::Command::AddPiece {
            sheet,
            name: "Etiquette".into(),
            quantity: 1,
            material: None,
            mirrored_pair: false,
        })
        .unwrap();
    }
    h.run_steps(3);
    open_dxf_window(&mut h, "Exporter les pièces en DXF de type AAMA / ASTM…");
    h.get_by_label_contains("« Etiquette » : pièce sans contour, non écrite.");
    h.get_by_label("Non exporté");
    h.get_by_label("Fermer sans enregistrer").click();
    h.run_steps(3);
    assert!(h.state().dxf_dialog.is_none());
    pf_i18n::set_lang(pf_i18n::Lang::En);
    h.run_steps(3);
    open_dxf_window(&mut h, "Export pieces as AAMA / ASTM-type DXF…");
    h.get_by_label_contains("1 piece(s) written");
    h.get_by_label_contains("\"Etiquette\": piece without outline, not written.");
    open_notes(&mut h);
    h.get_by_label_contains("has not been verified");
    h.get_by_label("Close without saving").click();
    h.run_steps(3);
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
    h.run_steps(3);
}

// ----- US-12.5 : export pour traceur HPGL / PLT --------------------------------------------------

fn open_hpgl_window(h: &mut Harness<'static, App>, menu: &str, label: &str) {
    h.get_by_label(menu).click();
    h.run_steps(3);
    h.get_by_label(label).click();
    h.run_steps(4);
}

const HPGL_FR: &str = "Exporter pour traceur (HPGL)…";

#[test]
fn us125_le_menu_montre_la_table_des_plumes_puis_ecrit_le_fichier_sans_toucher_au_projet() {
    let mut h = piece_harness();
    let project_before = h.state().editor.project().clone();
    let undo_before = h.state().editor.doc.can_undo();
    open_hpgl_window(&mut h, "Fichier", HPGL_FR);
    assert!(h.state().hpgl_dialog.is_some());
    // Rapport avant écriture : taille, déplacements, table plume -> type de trait, honnêteté.
    h.get_by_label_contains("Taille du tracé : 100 × 60 mm");
    h.get_by_label_contains("plume levée (ordre naturel");
    h.get_by_label("Plumes utilisées");
    let first = h
        .state()
        .hpgl_dialog
        .as_ref()
        .unwrap()
        .plot
        .as_ref()
        .unwrap()
        .report
        .pens[0];
    let expected = format!(
        "Plume {} — {} :",
        first.pen.number(),
        crate::hpgl::pen_label(first.pen)
    );
    h.get_by_label_contains(&expected);
    open_notes(&mut h);
    h.get_by_label_contains("Non vérifié sur un vrai traceur");
    h.get_by_label_contains("repère(s) de point : non tracés");
    // Écriture (le chemin est injecté : la boîte système n'est pas pilotable ici).
    let dir = std::env::temp_dir().join(format!("pf-us125-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("devant.plt");
    assert!(h.state_mut().write_hpgl_to(&path));
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.starts_with("IN;\nPA;\nSP"));
    assert!(text.ends_with("PU;\nSP0;\n"));
    assert_eq!(
        text,
        h.state()
            .hpgl_dialog
            .as_ref()
            .unwrap()
            .plot
            .as_ref()
            .unwrap()
            .text,
        "le fichier est le tracé affiché"
    );
    assert!(h.state().editor.status.text.contains("HPGL exporté"));
    assert!(!h.state().editor.status.error);
    h.get_by_label("Fermer sans enregistrer le HPGL").click();
    h.run_steps(3);
    assert!(h.state().hpgl_dialog.is_none());
    assert_eq!(
        *h.state().editor.project(),
        project_before,
        "projet inchangé"
    );
    assert_eq!(h.state().editor.doc.can_undo(), undo_before);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn us125_feuille_vide_refuse_sans_fenetre_ni_fichier() {
    let mut h = harness();
    let project_before = h.state().editor.project().clone();
    open_hpgl_window(&mut h, "Fichier", HPGL_FR);
    assert!(h.state().hpgl_dialog.is_none());
    assert!(h.state().editor.status.error);
    // Sans fenêtre, il n'y a rien à écrire.
    let dir = std::env::temp_dir().join(format!("pf-us125-b-{}", std::process::id()));
    let path = dir.join("rien.plt");
    assert!(!h.state_mut().write_hpgl_to(&path));
    assert!(!path.exists());
    assert_eq!(*h.state().editor.project(), project_before);
}

#[test]
fn us125_trop_grand_pour_la_feuille_propose_le_rouleau_puis_le_rouleau_passe() {
    let mut h = piece_harness();
    {
        let ed = &mut h.state_mut().editor;
        let sheet = ed.sheet;
        ed.exec(pf_core::Command::AddRectangle {
            sheet,
            corner: pf_core::PointRef::New(pt(0.0, 100.0)),
            width_mm: 300.0,
            height_mm: 20.0,
        })
        .unwrap();
    }
    let project_before = h.state().editor.project().clone();
    // PH-5e : plus de refus, la fenêtre s'ouvre déjà découpée en pages ; le refus de l'export en
    // un seul fichier reste affiché quand on décoche la case.
    open_hpgl_window(&mut h, "Fichier", HPGL_FR);
    {
        let d = h.state().hpgl_dialog.as_ref().expect("fenêtre découpée");
        assert!(d.tile && d.plot.is_none());
        let msg = d.refusal.clone().expect("refus du fichier unique");
        assert!(msg.contains("choisir un rouleau"), "{msg}");
        assert!(msg.contains("300"), "{msg}");
    }
    h.get_by_label("Fermer sans enregistrer le HPGL").click();
    h.run_steps(3);
    // Le même tracé passe sur un rouleau (réglage de l'application, jamais du projet).
    h.state_mut().page_setup.format = pf_export::PageFormat::Roll { width_mm: 914.0 };
    open_hpgl_window(&mut h, "Fichier", HPGL_FR);
    assert!(h.state().hpgl_dialog.is_some());
    h.get_by_label_contains("rouleau, largeur imprimable 894 mm");
    h.get_by_label("Fermer sans enregistrer le HPGL").click();
    h.run_steps(3);
    assert_eq!(*h.state().editor.project(), project_before);
}

/// Pièce de démonstration plus un rectangle de 300 × 20 mm : 2 pages A4 (fenêtre 190 mm de large).
fn hpgl_too_big_harness() -> Harness<'static, App> {
    let mut h = piece_harness();
    let ed = &mut h.state_mut().editor;
    let sheet = ed.sheet;
    ed.exec(pf_core::Command::AddRectangle {
        sheet,
        corner: pf_core::PointRef::New(pt(0.0, 100.0)),
        width_mm: 300.0,
        height_mm: 20.0,
    })
    .unwrap();
    h
}

#[test]
fn ph5e_case_decouper_en_pages_ecrit_un_fichier_par_page() {
    let mut h = hpgl_too_big_harness();
    let project_before = h.state().editor.project().clone();
    open_hpgl_window(&mut h, "Fichier", HPGL_FR);
    h.get_by_label("Découper en pages");
    h.get_by_label_contains("2 page(s) de 190 × 269 mm");
    let dir = std::env::temp_dir().join(format!("pf-ph5e-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("jupe.plt");
    assert!(h.state_mut().write_hpgl_to(&path));
    assert!(!path.exists(), "pas de fichier sans suffixe de page");
    let pages = &h
        .state()
        .hpgl_dialog
        .as_ref()
        .unwrap()
        .tiles
        .as_ref()
        .unwrap()
        .pages;
    assert_eq!(pages.len(), 2);
    for (k, p) in pages.iter().enumerate() {
        let file = dir.join(format!("jupe-p{}.plt", k + 1));
        let text = std::fs::read_to_string(&file).unwrap();
        assert_eq!(text, p.text);
        assert!(text.contains(&format!("LB{}/2 ", k + 1)), "numéro de page");
    }
    assert!(h.state().editor.status.text.contains("2 page(s)"));
    // Décocher : le refus du fichier unique est montré, plus rien à enregistrer.
    h.get_by_label("Découper en pages").click();
    h.run_steps(3);
    assert!(!h.state().hpgl_dialog.as_ref().unwrap().tile);
    h.get_by_label_contains("ne tient pas sur une page");
    let other = dir.join("autre.plt");
    assert!(!h.state_mut().write_hpgl_to(&other));
    assert!(!other.exists());
    // Cocher à nouveau recalcule les pages.
    h.get_by_label("Découper en pages").click();
    h.run_steps(3);
    assert!(h.state().hpgl_dialog.as_ref().unwrap().tile);
    // Chemin impossible : échec signalé, aucun fichier.
    let bad = dir.join("absent").join("x.plt");
    assert!(!h.state_mut().write_hpgl_to(&bad));
    assert!(h.state().error_dialog.is_some());
    assert!(!dir.join("absent").exists());
    h.state_mut().error_dialog = None;
    h.get_by_label("Fermer sans enregistrer le HPGL").click();
    h.run_steps(3);
    assert_eq!(*h.state().editor.project(), project_before);
    let _ = std::fs::remove_dir_all(&dir);
    // Noms de fichiers : un seul = le nom choisi ; sinon -p1, -p2…
    let p = std::path::Path::new("d").join("jupe.plt");
    assert_eq!(crate::hpgl::hpgl_page_paths(&p, 1), vec![p.clone()]);
    assert_eq!(
        crate::hpgl::hpgl_page_paths(&p, 2),
        vec![
            std::path::Path::new("d").join("jupe-p1.plt"),
            std::path::Path::new("d").join("jupe-p2.plt")
        ]
    );
}

#[test]
fn ph5e_une_feuille_qui_tient_s_exporte_en_un_fichier_meme_case_cochee() {
    let mut h = piece_harness();
    open_hpgl_window(&mut h, "Fichier", HPGL_FR);
    assert!(!h.state().hpgl_dialog.as_ref().unwrap().tile);
    h.get_by_label("Découper en pages").click();
    h.run_steps(3);
    let d = h.state().hpgl_dialog.as_ref().unwrap();
    assert!(d.tile);
    // Une seule page : le même fichier que l'export non tuilé, sans numéro de page.
    let tiles = d.tiles.as_ref().unwrap();
    assert_eq!(tiles.pages.len(), 1);
    assert_eq!(tiles.pages[0].text, d.plot.as_ref().unwrap().text);
    h.get_by_label("Fermer sans enregistrer le HPGL").click();
    h.run_steps(3);
}

#[test]
fn ph5e_textes_en_anglais() {
    let mut h = hpgl_too_big_harness();
    pf_i18n::set_lang(pf_i18n::Lang::En);
    h.run_steps(3);
    open_hpgl_window(&mut h, "File", "Export for plotter (HPGL)…");
    h.get_by_label("Split into pages");
    h.get_by_label_contains("2 page(s) of 190 × 269 mm");
    h.get_by_label("Close without saving the HPGL").click();
    h.run_steps(3);
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
    h.run_steps(3);
}

#[test]
fn us125_textes_en_anglais_et_aucune_promesse_de_compatibilite() {
    let mut h = piece_harness();
    pf_i18n::set_lang(pf_i18n::Lang::En);
    h.run_steps(3);
    open_hpgl_window(&mut h, "File", "Export for plotter (HPGL)…");
    h.get_by_label("Pens used");
    open_notes(&mut h);
    h.get_by_label_contains("Not verified on a real plotter");
    h.get_by_label_contains("Plot size: 100 × 60 mm");
    h.get_by_label("Close without saving the HPGL").click();
    h.run_steps(3);
    for lang in [pf_i18n::Lang::En, pf_i18n::Lang::Fr] {
        pf_i18n::set_lang(lang);
        for key in [
            "hpgl.intro",
            "hpgl.title",
            "menu.file.export_hpgl",
            "export.note.hpgl",
        ] {
            let t = pf_i18n::tr(key).to_lowercase();
            assert!(!t.contains("compatib"), "{key} : {t}");
        }
    }
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
    h.run_steps(3);
}

// ----- US-12.3 : DXF générique import / export ---------------------------------------------------

/// DXF minimal : une LINE (0 ; 0)-(100 ; 50), un CIRCLE (200 ; 100) de rayon 10 sur la couche
/// donnée et un TEXT ignoré ; `insunits` ajoute `$INSUNITS` à l'en-tête.
fn dxf_text_fixture(insunits: Option<i32>, layer: &str) -> String {
    let mut s = String::from("  0\nSECTION\n  2\nHEADER\n");
    if let Some(u) = insunits {
        s.push_str(&format!("  9\n$INSUNITS\n 70\n{u}\n"));
    }
    s.push_str("  0\nENDSEC\n  0\nSECTION\n  2\nENTITIES\n");
    s.push_str(&format!(
        "  0\nLINE\n  8\n{layer}\n 10\n0\n 20\n0\n 11\n100\n 21\n50\n"
    ));
    s.push_str(&format!(
        "  0\nCIRCLE\n  8\n{layer}\n 10\n200\n 20\n100\n 40\n10\n"
    ));
    s.push_str("  0\nTEXT\n  8\nNotes\n 10\n1\n 20\n1\n  1\nbonjour\n");
    s.push_str("  0\nENDSEC\n  0\nEOF\n");
    s
}

fn dxf_temp(name: &str, bytes: &[u8]) -> (std::path::PathBuf, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("pf-us123-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("entree.dxf");
    std::fs::write(&path, bytes).unwrap();
    (dir, path)
}

#[test]
fn us123_import_apercu_suit_l_unite_une_etape_annulable() {
    let mut h = harness();
    let project_before = h.state().editor.project().clone();
    let (dir, path) = dxf_temp("a", dxf_text_fixture(None, "Coupe").as_bytes());
    // Menu : les entrées existent, sans raccourci.
    h.get_by_label("Fichier").click();
    h.run_steps(3);
    h.get_by_label("Importer un DXF…");
    h.get_by_label("Exporter la construction en DXF…");
    h.key_press(Key::Escape);
    h.run_steps(3);
    h.state_mut().load_dxf_import(&path);
    h.run_steps(4);
    assert!(h.state().dxf_import.is_some());
    // Aucune unité déclarée : l'utilisatrice doit choisir, aucun aperçu, rien n'est importable.
    h.get_by_label_contains("Le fichier ne déclare pas d'unité");
    h.get_by_label("Choisissez l'unité des coordonnées pour voir l'aperçu.");
    assert!(h
        .state_mut()
        .dxf_import
        .as_mut()
        .unwrap()
        .preview()
        .is_none());
    assert!(!h.state_mut().run_dxf_import(), "pas d'import sans unité");
    assert_eq!(*h.state().editor.project(), project_before);
    // Millimètre : boîte de (0 ; 0) à (210 ; 110), entités ignorées listées avec leur compte.
    h.get_by_label("Millimètre (mm)").click();
    h.run_steps(3);
    h.get_by_label_contains("Dimensions : 210,0 × 110,0 mm");
    h.get_by_label_contains("de 0,0 ; 0,0 à 210,0 ; 110,0");
    h.get_by_label("1 unité du fichier = 1,0 mm");
    h.get_by_label("2 objet(s) à créer");
    h.get_by_label_contains("CIRCLE 1, LINE 1");
    h.get_by_label_contains("TEXT : 1 — type non pris en charge");
    h.get_by_label_contains("Coupe -> Coupe : 2 objet(s)");
    // Pouce : l'aperçu est recalculé (25,4 mm exactement).
    h.get_by_label("Pouce (in, 25,4 mm)").click();
    h.run_steps(3);
    h.get_by_label_contains("Dimensions : 5334,0 × 2794,0 mm");
    h.get_by_label("1 unité du fichier = 25,4 mm");
    h.get_by_label("Centimètre (cm)").click();
    h.run_steps(3);
    h.get_by_label_contains("Dimensions : 2100,0 × 1100,0 mm");
    h.get_by_label("Millimètre (mm)").click();
    h.run_steps(3);
    // Import : une seule commande, un seul pas d'annulation.
    assert!(!h.state().editor.doc.can_undo());
    h.get_by_label("Importer dans la feuille").click();
    h.run_steps(4);
    assert!(h.state().dxf_import.is_none(), "fenêtre fermée");
    {
        let sheet = h.state().editor.sheet();
        assert_eq!(sheet.entities.len(), 2);
        assert_eq!(sheet.layers.len(), 1);
        assert_eq!(sheet.layers[0].name, "Coupe");
    }
    assert!(h
        .state()
        .editor
        .status
        .text
        .contains("2 tracé(s) importé(s) depuis entree.dxf"));
    assert_eq!(
        h.state().editor.selection.entities.len(),
        2,
        "objets sélectionnés"
    );
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run_steps(3);
    assert_eq!(
        *h.state().editor.project(),
        project_before,
        "un seul Ctrl+Z défait tout l'import"
    );
    assert!(!h.state().editor.doc.can_undo());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn us123_import_fichier_refuse_ou_commande_refusee_sans_trace() {
    let mut h = harness();
    let project_before = h.state().editor.project().clone();
    // Fichier tronqué, binaire, codes invalides : aucune fenêtre, erreur claire, projet intact.
    let truncated = dxf_text_fixture(Some(4), "0");
    let cut = &truncated[..truncated.len() / 2];
    for (name, bytes, expect) in [
        ("tronque", cut.as_bytes().to_vec(), "tronqué"),
        (
            "binaire",
            b"AutoCAD Binary DXF\r\n\x1a\0rest".to_vec(),
            "DXF binaire non pris en charge",
        ),
        (
            "code",
            b"  0\nSECTION\n  2\nENTITIES\nabc\nLINE\n".to_vec(),
            "Code de groupe invalide à la ligne 5",
        ),
    ] {
        let (dir, path) = dxf_temp(name, &bytes);
        h.state_mut().load_dxf_import(&path);
        h.run_steps(3);
        assert!(h.state().dxf_import.is_none(), "{name}");
        assert!(h.state().editor.status.error, "{name}");
        assert!(
            h.state().editor.status.text.contains(expect),
            "{name} : {}",
            h.state().editor.status.text
        );
        assert_eq!(*h.state().editor.project(), project_before, "{name}");
        let _ = std::fs::remove_dir_all(&dir);
    }
    // Fichier introuvable : boîte d'erreur, rien d'autre.
    h.state_mut()
        .load_dxf_import(std::path::Path::new("Z:/inexistant/rien.dxf"));
    h.run_steps(3);
    assert!(h.state().dxf_import.is_none());
    assert!(h.state().error_dialog.is_some());
    h.state_mut().error_dialog = None;
    // Commande refusée : le calque « Coupe » existe, verrouillé. Le projet reste identique à
    // l'octet, la fenêtre reste ouverte, aucune étape d'historique de plus.
    {
        let ed = &mut h.state_mut().editor;
        let sheet = ed.sheet;
        let layer = ed
            .exec(pf_core::Command::AddLayer {
                sheet,
                name: "Coupe".into(),
            })
            .unwrap()
            .layers[0];
        ed.exec(pf_core::Command::SetLayerLocked {
            sheet,
            layer,
            locked: true,
        })
        .unwrap();
    }
    h.run_steps(3);
    let before = h.state().editor.project().clone();
    let bytes_before = pf_core::persist::to_json(&before).unwrap();
    let (dir, path) = dxf_temp("verrou", dxf_text_fixture(Some(4), "Coupe").as_bytes());
    h.state_mut().load_dxf_import(&path);
    h.run_steps(4);
    assert!(h.state().dxf_import.is_some());
    h.get_by_label("Importer dans la feuille").click();
    h.run_steps(4);
    assert!(h.state().dxf_import.is_some(), "la fenêtre reste ouverte");
    assert!(h.state().editor.status.error);
    assert!(
        h.state().editor.status.text.contains("verrouillé"),
        "{}",
        h.state().editor.status.text
    );
    assert_eq!(*h.state().editor.project(), before);
    assert_eq!(
        pf_core::persist::to_json(h.state().editor.project()).unwrap(),
        bytes_before
    );
    h.get_by_label("Fermer sans importer").click();
    h.run_steps(3);
    assert!(h.state().dxf_import.is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn us123_export_construction_fenetre_fichier_et_refus() {
    let mut h = harness();
    // Feuille vide : refus, aucune fenêtre ni fichier.
    h.state_mut().open_construction_export();
    h.run_steps(3);
    assert!(h.state().dxf_construction.is_none());
    assert!(h.state().editor.status.error);
    assert!(h.state().editor.status.text.contains("Rien à exporter"));
    {
        let ed = &mut h.state_mut().editor;
        let sheet = ed.sheet;
        ed.exec(pf_core::Command::AddRectangle {
            sheet,
            corner: pf_core::PointRef::New(pt(0.0, 0.0)),
            width_mm: 100.0,
            height_mm: 60.0,
        })
        .unwrap();
        ed.exec(pf_core::Command::AddCircle {
            sheet,
            center: pf_core::PointRef::New(pt(50.0, 30.0)),
            radius_mm: 10.0,
        })
        .unwrap();
        ed.selection.clear();
    }
    h.run_steps(3);
    let project_before = h.state().editor.project().clone();
    let undo_before = h.state().editor.doc.can_undo();
    h.get_by_label("Fichier").click();
    h.run_steps(3);
    h.get_by_label("Exporter la construction en DXF…").click();
    h.run_steps(4);
    assert!(h.state().dxf_construction.is_some());
    // Rapport avant écriture : entités, couches, rien d'omis, remarque honnête.
    h.get_by_label_contains("5 entité(s) écrite(s) sur 1 couche(s)");
    h.get_by_label_contains("Entités DXF : CIRCLE 1, LINE 4");
    h.get_by_label("Couche « 0 » : 5 entité(s)");
    h.get_by_label("Aucun objet omis.");
    open_notes(&mut h);
    h.get_by_label_contains("n'a pas été vérifiée");
    let dir = std::env::temp_dir().join(format!("pf-us123-exp-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("construction.dxf");
    assert!(h.state_mut().write_construction_dxf_to(&path));
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.starts_with("  0\nSECTION\n  2\nHEADER\n"));
    assert!(text.ends_with("  0\nEOF\n"));
    assert!(text.contains("$INSUNITS\n 70\n4\n"));
    assert!(text.contains("CIRCLE") && !text.contains("INSERT"));
    assert!(h.state().editor.status.text.contains("DXF exporté"));
    h.get_by_label("Fermer sans écrire le fichier").click();
    h.run_steps(3);
    assert!(h.state().dxf_construction.is_none());
    assert_eq!(
        *h.state().editor.project(),
        project_before,
        "projet inchangé"
    );
    assert_eq!(h.state().editor.doc.can_undo(), undo_before);
    // Le fichier réexporté se relit : aller-retour par l'interface (les points fusionnent, les
    // segments identiques sont signalés, le cercle est recréé).
    h.state_mut().load_dxf_import(&path);
    h.run_steps(4);
    h.get_by_label("Importer dans la feuille").click();
    h.run_steps(4);
    assert_eq!(
        h.state().editor.sheet().entities.len(),
        6,
        "cercle de plus, segments fusionnés"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn us123_textes_en_anglais() {
    let mut h = harness();
    pf_i18n::set_lang(pf_i18n::Lang::En);
    h.run_steps(3);
    let (dir, path) = dxf_temp("en", dxf_text_fixture(Some(1), "Cut").as_bytes());
    h.get_by_label("File").click();
    h.run_steps(3);
    h.get_by_label("Import a DXF…");
    h.get_by_label("Export construction geometry as DXF…");
    h.key_press(Key::Escape);
    h.run_steps(3);
    h.state_mut().load_dxf_import(&path);
    h.run_steps(4);
    // $INSUNITS = 1 : l'unité déclarée est proposée d'office, le pouce.
    h.get_by_label("Unit declared by the file: inch");
    h.get_by_label_contains("Dimensions: 5334.0 × 2794.0 mm");
    h.get_by_label("1 file unit = 25.4 mm");
    h.get_by_label_contains("TEXT: 1 — unsupported type");
    open_notes(&mut h);
    h.get_by_label_contains("has not been tried on files from real software");
    h.get_by_label("Close without importing").click();
    h.run_steps(3);
    assert!(h.state().dxf_import.is_none());
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
    h.run_steps(3);
    let _ = std::fs::remove_dir_all(&dir);
}

// ----- US-05.1 : marge de couture ---------------------------------------------------

/// Saisit `text` dans le `nth`-ième (dans l'ordre d'affichage) champ de texte à une ligne dont la
/// valeur actuelle est `current`.
fn type_in_field_valued(h: &mut Harness<'_, App>, current: &str, nth: usize, text: &str) {
    {
        let fields: Vec<_> = h
            .get_all_by_role(egui::accesskit::Role::TextInput)
            .collect();
        fields
            .iter()
            .filter(|f| f.value().as_deref() == Some(current))
            .nth(nth)
            .unwrap_or_else(|| panic!("aucun champ n° {nth} « {current} »"))
            .focus();
    }
    h.run_steps(2);
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.run_steps(1);
    type_text(h, text);
    key(h, Key::Enter);
    h.run_steps(3);
}

#[test]
fn us051_marge_de_couture_creation_une_annulation_refus_et_anglais() {
    let mut h = piece_harness();
    // Fenêtre haute : l'inspecteur défile, et un clic hors fenêtre n'atteint rien.
    h.set_size(egui::vec2(1280.0, 1600.0));
    h.run_steps(3);
    // Repliée par défaut : l'en-tête seul, aucun champ.
    h.get_by_label("Marge de couture");
    assert!(
        h.query_by_label("Par défaut (mm)").is_none(),
        "repliée par défaut"
    );
    h.get_by_label("Marge de couture").click();
    h.run_steps(3);
    h.get_by_label("Par défaut (mm)");
    h.get_by_label("Aucune surcharge");
    assert!(piece_of(&h).seam.is_none());
    let project_before = h.state().editor.project().clone();
    // Création : « 10 » validé = une commande, contour de coupe 120 × 80.
    type_in_field_valued(&mut h, "0,0", 0, "10");
    assert!(
        !h.state().editor.status.error,
        "{}",
        h.state().editor.status.text
    );
    let seam = piece_of(&h).seam.expect("marge enregistrée");
    assert_eq!(seam.default_mm, 10.0);
    assert!(seam.edges.is_empty());
    let cut = h
        .state()
        .editor
        .project()
        .piece_cut_contour(piece_of(&h).id)
        .unwrap();
    assert!((cut.area_mm2 - 9600.0).abs() < 1e-6 && (cut.perimeter_mm - 400.0).abs() < 1e-6);
    // Mesures observées du contour calculé, affichées (96 cm²).
    h.get_by_label_contains("périmètre 400,0 mm");
    h.get_by_label_contains("aire 96,0 cm²");
    // Une seule annulation défait la marge : le projet est exactement celui d'avant la saisie.
    undo(&mut h);
    assert!(piece_of(&h).seam.is_none());
    assert_eq!(
        *h.state().editor.project(),
        project_before,
        "une seule étape"
    );
    // Refus : valeur hors bornes ou illisible, projet inchangé, message affiché.
    let before = h.state().editor.project().clone();
    type_in_field_valued(&mut h, "0,0", 0, "1001");
    assert!(h.state().editor.status.error);
    assert!(h.state().editor.status.text.contains("Marge invalide"));
    type_in_field_valued(&mut h, "0,0", 0, "abc");
    assert!(h.state().editor.status.error);
    assert_eq!(*h.state().editor.project(), before, "refus sans trace");
    // Surcharge du tracé sélectionné : progressive, début 10 / fin 20.
    type_in_field_valued(&mut h, "0,0", 0, "10");
    let piece = piece_of(&h);
    let right = {
        let sheet = h.state().editor.project().piece_sheet(piece.id).unwrap();
        piece
            .outline
            .iter()
            .find_map(|o| {
                let (a, b) = sheet.segment_ends(o.entity).ok()?;
                ((a.x - 100.0).abs() < 1e-9 && (b.x - 100.0).abs() < 1e-9).then_some(o.entity)
            })
            .expect("bord droit")
    };
    h.state_mut().editor.selection.entities = vec![right];
    h.run_steps(3);
    h.get_by_label("Surcharger le tracé sélectionné").click();
    h.run_steps(3);
    assert_eq!(piece_of(&h).seam.unwrap().edges.len(), 1);
    // Champs « 10,0 » dans l'ordre : défaut, début, fin. Fin = 20 : marge progressive 10 → 20.
    type_in_field_valued(&mut h, "10,0", 2, "20");
    let edges = piece_of(&h).seam.unwrap().edges;
    assert_eq!(edges.len(), 1);
    assert_eq!(
        (edges[0].start_mm, edges[0].end_mm),
        (10.0, 20.0),
        "{edges:?}"
    );
    assert_eq!(edges[0].entity, right);
    // « Aucune surcharge » : retire toutes les surcharges en une étape.
    h.get_by_label("Aucune surcharge").click();
    h.run_steps(3);
    assert!(piece_of(&h).seam.unwrap().edges.is_empty());
    h.state().editor.project().validate().unwrap();
    // Anglais.
    pf_i18n::set_lang(pf_i18n::Lang::En);
    h.run_steps(3);
    h.get_by_label("Seam allowance");
    h.get_by_label("Default (mm)");
    h.get_by_label("No override");
    h.get_by_label_contains("Computed cut contour (not stored)");
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
    h.run_steps(3);
}

// ----- US-05.5 : marges par matière -------------------------------------------------

fn set_piece_category(h: &mut Harness<'_, App>, category: Option<pf_core::PieceCategory>) {
    let c = piece_of(h);
    h.state_mut().editor.exec(pf_core::Command::SetPieceCard {
        piece: c.id,
        name: c.name,
        code: c.code,
        quantity: c.quantity,
        mirrored_pair: c.mirrored_pair,
        material: c.material,
        category,
        comment: c.comment,
    });
    h.run_steps(3);
}

#[test]
fn us055_marges_par_matiere_creation_une_annulation_refus_et_anglais() {
    use pf_core::{PieceCategory, SeamDefaults};
    let mut h = piece_harness();
    h.set_size(egui::vec2(1280.0, 1600.0));
    h.run_steps(3);
    set_piece_category(&mut h, Some(PieceCategory::Leather));
    h.get_by_label("Marge de couture").click();
    h.run_steps(3);
    // Sous-bloc replié par défaut : aucune valeur de catégorie affichée.
    h.get_by_label("Marges par matière");
    assert!(h.query_by_label("Valeurs d'atelier").is_none(), "replié");
    // Valeur d'atelier du cuir (6 mm) proposée ; la pièce n'a pas de marge.
    h.get_by_label("Appliquer la valeur de la catégorie (6,0 mm)");
    assert!(piece_of(&h).seam.is_none());
    let start = h.state().editor.project().clone();
    // Réglage du jeu : cuir 6 → 8, une commande, aucune marge posée n'est touchée.
    h.get_by_label("Marges par matière").click();
    h.run_steps(3);
    type_in_field_valued(&mut h, "6,0", 0, "8");
    assert!(
        !h.state().editor.status.error,
        "{}",
        h.state().editor.status.text
    );
    let p = h.state().editor.project().clone();
    assert_eq!(p.seam_defaults.unwrap().leather_mm, 8.0);
    assert_eq!(p.seam_defaults.unwrap().fabric_mm, 10.0);
    assert!(piece_of(&h).seam.is_none());
    // Appliquer la valeur de la catégorie : une commande, marge 8 mm (aire 116 × 76).
    h.get_by_label("Appliquer la valeur de la catégorie (8,0 mm)")
        .click();
    h.run_steps(3);
    assert_eq!(piece_of(&h).seam.unwrap().default_mm, 8.0);
    let cut = h
        .state()
        .editor
        .project()
        .piece_cut_contour(piece_of(&h).id)
        .unwrap();
    assert!((cut.area_mm2 - 116.0 * 76.0).abs() < 1e-6);
    // Une annulation défait la marge, une autre le jeu.
    undo(&mut h);
    assert!(piece_of(&h).seam.is_none());
    assert_eq!(
        h.state().editor.project().seam_defaults.unwrap().leather_mm,
        8.0
    );
    undo(&mut h);
    assert_eq!(*h.state().editor.project(), start, "deux étapes exactement");
    // Refus : hors bornes ou illisible, projet inchangé, message affiché.
    type_in_field_valued(&mut h, "6,0", 0, "1001");
    assert!(h.state().editor.status.error);
    assert!(h.state().editor.status.text.contains("Valeur invalide"));
    type_in_field_valued(&mut h, "6,0", 0, "abc");
    assert!(h.state().editor.status.error);
    assert_eq!(*h.state().editor.project(), start, "refus sans trace");
    assert_eq!(
        h.state().editor.project().seam_defaults,
        None::<SeamDefaults>
    );
    // Pièce sans catégorie : bouton désactivé, repli proposé, une seule étape.
    set_piece_category(&mut h, None);
    h.get_by_label_contains("Pièce sans catégorie (non défini)");
    h.get_by_label("Appliquer la valeur de la catégorie");
    let before = h.state().editor.project().clone();
    h.get_by_label("Créer la marge de repli (10,0 mm)").click();
    h.run_steps(3);
    assert_eq!(piece_of(&h).seam.unwrap().default_mm, 10.0);
    undo(&mut h);
    assert_eq!(*h.state().editor.project(), before);
    // Anglais.
    pf_i18n::set_lang(pf_i18n::Lang::En);
    h.run_steps(3);
    h.get_by_label("Allowances by material");
    h.get_by_label("Apply the category value");
    h.get_by_label("Create the fallback allowance (10.0 mm)");
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
    h.run_steps(3);
}

// ----- US-05.6 : pièce « coupe nette » ----------------------------------------------

#[test]
fn us056_coupe_nette_creation_une_annulation_refus_et_anglais() {
    let mut h = piece_harness();
    h.set_size(egui::vec2(1280.0, 1600.0));
    h.run_steps(3);
    h.get_by_label("Marge de couture").click();
    h.run_steps(3);
    let start = h.state().editor.project().clone();
    assert!(!piece_of(&h).net_cut);
    // Déclaration : une commande, indicateur écrit (jamais la couleur seule), marge masquée.
    h.get_by_label("Coupe nette (bord franc)").click();
    h.run_steps(3);
    assert!(
        !h.state().editor.status.error,
        "{}",
        h.state().editor.status.text
    );
    assert!(piece_of(&h).net_cut);
    // Pièce d'exemple 100 × 60 : périmètre 320 mm, aire 60 cm² (calculés à la main).
    h.get_by_label_contains("Coupe nette : contour de coupe = contour de couture");
    h.get_by_label_contains("aire 60");
    assert!(h.query_by_label("Défaut (mm)").is_none(), "marge masquée");
    let cut = h
        .state()
        .editor
        .project()
        .piece_cut_contour(piece_of(&h).id)
        .unwrap();
    assert!((cut.perimeter_mm - 320.0).abs() < 1e-9 && (cut.area_mm2 - 6000.0).abs() < 1e-9);
    // Une seule annulation rend le projet d'origine.
    undo(&mut h);
    assert!(!piece_of(&h).net_cut);
    assert_eq!(*h.state().editor.project(), start, "une étape exactement");
    // Refus : la pièce a une marge, la déclaration est refusée, projet identique, message affiché.
    type_in_field_valued(&mut h, "0,0", 0, "10");
    assert_eq!(piece_of(&h).seam.as_ref().unwrap().default_mm, 10.0);
    let before = h.state().editor.project().clone();
    h.get_by_label("Coupe nette (bord franc)").click();
    h.run_steps(3);
    assert!(h.state().editor.status.error);
    assert!(
        h.state()
            .editor
            .status
            .text
            .contains("retirez d'abord la marge"),
        "{}",
        h.state().editor.status.text
    );
    assert_eq!(*h.state().editor.project(), before, "refus sans trace");
    // Anglais.
    pf_i18n::set_lang(pf_i18n::Lang::En);
    h.run_steps(3);
    h.get_by_label("Net cut (raw edge)");
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
    h.run_steps(3);
}

// ----- US-05.2 : types de coins -----------------------------------------------------

#[test]
fn us052_types_de_coins_creation_une_annulation_refus_et_anglais() {
    use pf_core::CornerType;
    let mut h = piece_harness();
    h.set_size(egui::vec2(1280.0, 1600.0));
    h.run_steps(3);
    h.get_by_label("Marge de couture").click();
    h.run_steps(3);
    type_in_field_valued(&mut h, "0,0", 0, "10");
    assert_eq!(piece_of(&h).seam.unwrap().default_mm, 10.0);
    // Aucun type choisi : liste vide, invite à sélectionner un tracé.
    h.get_by_label("Coins (0 choisis)");
    h.get_by_label("Aucun type choisi : tous les coins sont en onglet.");
    h.get_by_label_contains("Sélectionnez un tracé du contour pour choisir le coin");
    // Tracé dont le début est un coin saillant : bord droit, qui commence en (100 ; 0).
    let right = {
        let piece = piece_of(&h);
        let sheet = h.state().editor.project().piece_sheet(piece.id).unwrap();
        piece
            .outline
            .iter()
            .find_map(|o| {
                let (a, b) = sheet.segment_ends(o.entity).ok()?;
                ((a.x - 100.0).abs() < 1e-9 && (b.x - 100.0).abs() < 1e-9).then_some(o.entity)
            })
            .expect("bord droit")
    };
    h.state_mut().editor.selection.entities = vec![right];
    h.run_steps(3);
    h.get_by_label_contains("Coin au début de");
    // Création : « En marche » = une commande, un seul type enregistré sur ce tracé.
    let before = h.state().editor.project().clone();
    h.get_by_label("En marche").click();
    h.run_steps(3);
    assert!(
        !h.state().editor.status.error,
        "{}",
        h.state().editor.status.text
    );
    let seam = piece_of(&h).seam.unwrap();
    assert_eq!(seam.corners.len(), 1);
    assert_eq!(seam.corners[0].entity, right);
    assert_eq!(seam.corners[0].corner, CornerType::Stepped {});
    h.get_by_label("Coins (1 choisis)");
    // Rectangle : un coin droit en marche = l'onglet (110 × 70 aux coins) ; le contour calculé suit.
    assert!(h
        .state()
        .editor
        .project()
        .piece_cut_contour(seam_piece_id(&h))
        .is_ok());
    // Une seule annulation défait le choix : le projet est exactement celui d'avant.
    undo(&mut h);
    assert_eq!(*h.state().editor.project(), before, "une seule étape");
    h.get_by_label("Coins (0 choisis)");
    // Tronqué : profondeur par défaut 2 mm, éditable ; valeur invalide refusée sans trace.
    h.get_by_label("Tronqué").click();
    h.run_steps(3);
    assert_eq!(
        piece_of(&h).seam.unwrap().corners[0].corner,
        CornerType::Truncated { depth_mm: 2.0 }
    );
    type_in_field_valued(&mut h, "2,0", 0, "3");
    assert_eq!(
        piece_of(&h).seam.unwrap().corners[0].corner,
        CornerType::Truncated { depth_mm: 3.0 }
    );
    let refused_before = h.state().editor.project().clone();
    type_in_field_valued(&mut h, "3,0", 0, "20000");
    assert!(h.state().editor.status.error);
    assert!(h.state().editor.status.text.contains("Valeur invalide"));
    type_in_field_valued(&mut h, "3,0", 0, "abc");
    assert!(h.state().editor.status.error);
    assert_eq!(
        *h.state().editor.project(),
        refused_before,
        "refus sans trace"
    );
    // Anglais.
    pf_i18n::set_lang(pf_i18n::Lang::En);
    h.run_steps(3);
    h.get_by_label("Corners (1 chosen)");
    h.get_by_label("Mitre");
    h.get_by_label("Stepped");
    h.get_by_label("Truncated");
    h.get_by_label("Maximum tip (mm)");
    h.get_by_label_contains("Corner at the start of");
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
    h.run_steps(3);
    // « Onglet » retire le type du sommet (une étape) ; la marge reste.
    h.get_by_label("Onglet").click();
    h.run_steps(3);
    let seam = piece_of(&h).seam.unwrap();
    assert!(seam.corners.is_empty() && seam.default_mm == 10.0);
    h.state().editor.project().validate().unwrap();
}

fn seam_piece_id(h: &Harness<'_, App>) -> pf_core::PieceId {
    piece_of(h).id
}

// ----- US-05.3 : ourlets et rempliés ------------------------------------------------

/// Saisit `text` dans le champ de texte situé juste à droite de l'étiquette `label` (même ligne).
fn type_in_field_after_label(h: &mut Harness<'_, App>, label: &str, text: &str) {
    {
        let lab = h.get_by_label(label).rect();
        let field = h
            .get_all_by_role(egui::accesskit::Role::TextInput)
            .filter(|f| {
                let r = f.rect();
                (r.center().y - lab.center().y).abs() < 6.0 && r.min.x >= lab.max.x - 1.0
            })
            .min_by(|a, b| a.rect().min.x.total_cmp(&b.rect().min.x))
            .unwrap_or_else(|| panic!("aucun champ après « {label} »"));
        field.focus();
    }
    h.run_steps(2);
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.run_steps(1);
    type_text(h, text);
    key(h, Key::Enter);
    h.run_steps(3);
}

/// Tracé du contour de la pièce du harnais qui va de `a` à `b` (dans un sens ou l'autre).
fn outline_edge(h: &Harness<'_, App>, a: PointMm, b: PointMm) -> pf_core::EntityId {
    let piece = piece_of(h);
    let sheet = h.state().editor.project().piece_sheet(piece.id).unwrap();
    piece
        .outline
        .iter()
        .find_map(|o| {
            let (s, t) = sheet.segment_ends(o.entity).ok()?;
            let same =
                |p: PointMm, q: PointMm| (p.x - q.x).abs() < 1e-9 && (p.y - q.y).abs() < 1e-9;
            ((same(s, a) && same(t, b)) || (same(s, b) && same(t, a))).then_some(o.entity)
        })
        .expect("bord du contour")
}

#[test]
fn us053_ourlet_et_rempli_creation_une_annulation_refus_et_anglais() {
    let mut h = piece_harness();
    h.set_size(egui::vec2(1280.0, 1600.0));
    h.run_steps(3);
    h.get_by_label("Marge de couture").click();
    h.run_steps(3);
    // Rien de sélectionné : les deux boutons d'ajout sont inactifs, listes vides.
    h.get_by_label("Ourlets (0)");
    h.get_by_label("Rempliés (0)");
    h.get_by_label_contains("Aucun ourlet");
    assert!(h
        .get_by_label("Ourlet sur le tracé sélectionné")
        .accesskit_node()
        .is_disabled());
    // Ourlet sur le bord du bas : une commande, hauteur proposée 20 mm.
    let bottom = outline_edge(&h, pt(0.0, 0.0), pt(100.0, 0.0));
    h.state_mut().editor.selection.entities = vec![bottom];
    h.run_steps(3);
    let before = h.state().editor.project().clone();
    h.get_by_label("Ourlet sur le tracé sélectionné").click();
    h.run_steps(3);
    assert!(
        !h.state().editor.status.error,
        "{}",
        h.state().editor.status.text
    );
    let seam = piece_of(&h).seam.expect("réglage enregistré");
    assert_eq!(seam.hems.len(), 1);
    assert_eq!(
        (seam.hems[0].entity, seam.hems[0].height_mm),
        (bottom, 20.0)
    );
    h.get_by_label("Ourlets (1)");
    // Marge nulle ailleurs, ourlet de 20 en bas : 100 × 80 (calcul à la main).
    let cut = h
        .state()
        .editor
        .project()
        .piece_cut_contour(seam_piece_id(&h))
        .unwrap();
    assert!((cut.area_mm2 - 8000.0).abs() < 1e-6 && (cut.perimeter_mm - 360.0).abs() < 1e-6);
    // Une seule annulation : le projet est exactement celui d'avant.
    undo(&mut h);
    assert_eq!(*h.state().editor.project(), before, "une seule étape");
    h.get_by_label("Ourlets (0)");
    h.get_by_label("Ourlet sur le tracé sélectionné").click();
    h.run_steps(3);
    // Refus : hauteur hors bornes ou illisible, projet inchangé, message affiché.
    let refused_before = h.state().editor.project().clone();
    type_in_field_valued(&mut h, "20,0", 0, "1001");
    assert!(h.state().editor.status.error);
    assert!(h.state().editor.status.text.contains("Hauteur invalide"));
    type_in_field_valued(&mut h, "20,0", 0, "abc");
    assert!(h.state().editor.status.error);
    assert_eq!(
        *h.state().editor.project(),
        refused_before,
        "refus sans trace"
    );
    // Modification valide : 30 mm.
    type_in_field_valued(&mut h, "20,0", 0, "30");
    assert_eq!(piece_of(&h).seam.unwrap().hems[0].height_mm, 30.0);
    // Rempli sur un bord droit : valeur 6 proposée, aucun cran, texte explicite.
    let right = outline_edge(&h, pt(100.0, 0.0), pt(100.0, 60.0));
    h.state_mut().editor.selection.entities = vec![right];
    h.run_steps(3);
    h.get_by_label("Rempli sur le tracé sélectionné").click();
    h.run_steps(3);
    let seam = piece_of(&h).seam.unwrap();
    assert_eq!((seam.folds.len(), seam.folds[0].value_mm), (1, 6.0));
    h.get_by_label("Rempliés (1)");
    h.get_by_label("bord droit : aucun cran");
    h.state().editor.project().validate().unwrap();
    // Anglais.
    pf_i18n::set_lang(pf_i18n::Lang::En);
    h.run_steps(3);
    h.get_by_label("Hems (1)");
    h.get_by_label("Leather folds (1)");
    h.get_by_label("straight edge: no notch");
    h.get_by_label("height (mm)");
    h.get_by_label("Hem on the selected path");
    h.get_by_label("Fold on the selected path");
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
    h.run_steps(3);
}

#[test]
fn us053_rempli_sur_courbe_crans_calcules_pas_et_nombre() {
    let mut h = harness();
    h.set_size(egui::vec2(1280.0, 1600.0));
    let (p, arc) = crate::hem::demo_hem_project(true).unwrap();
    h.state_mut().editor = crate::editor::Editor::new(pf_core::Document::new(p));
    h.state_mut().editor.selection.entities = vec![arc];
    h.run_steps(3);
    h.get_by_label("Marge de couture").click();
    h.run_steps(3);
    // Quartier de rayon 100 : pas par défaut 25 mm → 6 crans calculés, jamais stockés.
    h.get_by_label("6 cran(s) de rempli calculé(s), non enregistré(s)");
    let piece = piece_of(&h);
    assert_eq!(
        h.state()
            .editor
            .project()
            .piece_fold_notches(piece.id)
            .unwrap()
            .len(),
        6
    );
    // Pas de 100 mm : 1 cran (⌈157,08 / 100⌉ = 2 intervalles).
    type_in_field_after_label(&mut h, "pas des crans (mm)", "100");
    assert_eq!(
        piece_of(&h).seam.unwrap().folds[0].notch_step_mm,
        Some(100.0)
    );
    h.get_by_label("1 cran(s) de rempli calculé(s), non enregistré(s)");
    // Pas hors bornes : refus sans trace.
    let before = h.state().editor.project().clone();
    type_in_field_valued(&mut h, "100,0", 0, "0,5");
    assert!(h.state().editor.status.error);
    assert!(h.state().editor.status.text.contains("Pas invalide"));
    assert_eq!(*h.state().editor.project(), before, "refus sans trace");
    // Une annulation défait le pas.
    undo(&mut h);
    assert_eq!(piece_of(&h).seam.unwrap().folds[0].notch_step_mm, None);
    h.get_by_label("6 cran(s) de rempli calculé(s), non enregistré(s)");
    // Anglais.
    pf_i18n::set_lang(pf_i18n::Lang::En);
    h.run_steps(3);
    h.get_by_label("6 fold notch(es) computed, not stored");
    h.get_by_label("notch step (mm)");
    h.get_by_label("notch count");
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
    h.run_steps(3);
}

// ----- US-05.4 : affichage et export de la couture et de la coupe -----------------------------

#[test]
fn us054_boite_d_export_choix_du_mode_applique_hors_projet_et_aux_sorties_en_anglais() {
    let dir = std::env::temp_dir().join(format!("pf-us054-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let file = dir.join("settings.json");
    let mut h = piece_harness();
    h.state_mut().settings_path = Some(file.clone());
    // Marge de 10 mm : pièce 100 × 60, coupe 120 × 80 (périmètre 400 mm, calcul à la main).
    let piece = piece_of(&h).id;
    h.state_mut()
        .editor
        .exec(pf_core::Command::SetSeamAllowance {
            piece,
            seam: Some(pf_core::SeamAllowance {
                default_mm: 10.0,
                ..Default::default()
            }),
        })
        .unwrap();
    h.run_steps(3);
    let project_before = h.state().editor.project().clone();
    pf_i18n::set_lang(pf_i18n::Lang::En);
    h.run_steps(3);
    h.get_by_label("File").click();
    h.run_steps(3);
    h.get_by_label("PDF page setup…").click();
    h.run_steps(4);
    h.get_by_label("Output content").click();
    h.run_steps(3);
    h.get_by_label("Pieces with a seam allowance:");
    h.get_by_label("Sewing and cutting");
    h.get_by_label("Sewing only");
    h.get_by_label("Cutting only (computed)");
    h.get_by_label_contains("long-dash / short-dash line");
    // Défaut = les deux ; le choix n'est appliqué qu'avec le bouton.
    assert_eq!(
        h.state().output_content.piece_mode,
        pf_export::PieceExportMode::Both
    );
    h.get_by_label("Cutting only (computed)").click_accesskit();
    h.run_steps(3);
    assert_eq!(
        h.state().print_dialog.as_ref().unwrap().content.piece_mode,
        pf_export::PieceExportMode::CutOnly
    );
    assert_eq!(
        h.state().output_content.piece_mode,
        pf_export::PieceExportMode::Both
    );
    assert!(!file.exists());
    h.get_by_label("Apply page setup").click_accesskit();
    h.run_steps(4);
    assert_eq!(
        h.state().output_content.piece_mode,
        pf_export::PieceExportMode::CutOnly
    );
    // Préférence de l'application, relue à l'identique ; le projet n'en sait rien.
    assert_eq!(
        print_setup::load_output_content(&file).piece_mode,
        pf_export::PieceExportMode::CutOnly
    );
    let saved: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(saved["output_content"]["piece_mode"], "cut");
    assert_eq!(*h.state().editor.project(), project_before);
    // Les sorties suivent le choix : scène, HPGL (plume coupe seule) et DXF (couche 1 seule).
    let scene = h
        .state()
        .export_scene(h.state().editor.sheet)
        .expect("scène");
    assert_eq!(scene.cut_contours.len(), 1);
    assert!(scene.cut_warnings.is_empty());
    h.state_mut().open_hpgl_export();
    {
        let d = h.state().hpgl_dialog.as_ref().expect("fenêtre HPGL");
        assert!(d.warnings.is_empty());
        let pens: Vec<_> = d
            .plot
            .as_ref()
            .expect("fichier unique")
            .report
            .pens
            .iter()
            .map(|p| (p.pen, p.length_mm))
            .collect();
        let cut = pens
            .iter()
            .find(|(p, _)| *p == pf_export::hpgl::HpglPen::Cutting)
            .expect("plume coupe");
        assert!((cut.1 - 400.0).abs() < 0.1, "{pens:?}");
        assert!(!pens
            .iter()
            .any(|(p, _)| *p == pf_export::hpgl::HpglPen::Sewing));
    }
    h.state_mut().hpgl_dialog = None;
    h.state_mut().open_dxf_export();
    {
        let d = h.state().dxf_dialog.as_ref().expect("fenêtre DXF");
        let layers: Vec<u32> = d
            .export
            .report
            .layers
            .iter()
            .map(|l| l.layer.number)
            .collect();
        assert!(layers.contains(&1) && !layers.contains(&15), "{layers:?}");
    }
    h.state_mut().dxf_dialog = None;
    h.run_steps(2);
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
    h.run_steps(3);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn us054_avertissement_de_coupe_impossible_lisible_en_francais_et_en_anglais() {
    let w = pf_export::CutWarning {
        piece: "Dos".into(),
        reason: "marge impossible".into(),
        notches_only: false,
        cut_notches: false,
        sewing_kept: true,
    };
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
    let fr = crate::app::cut_warnings_note(std::slice::from_ref(&w));
    assert_eq!(
        fr,
        " ATTENTION : contour de coupe de « Dos » non exporté (marge impossible). \
         Le contour de couture est exporté à la place."
    );
    pf_i18n::set_lang(pf_i18n::Lang::En);
    let en = crate::app::cut_warnings_note(std::slice::from_ref(&w));
    assert_eq!(
        en,
        " WARNING: cutting contour of \"Dos\" not exported (marge impossible). \
         The sewing outline is exported instead."
    );
    let notches = pf_export::CutWarning {
        notches_only: true,
        cut_notches: false,
        sewing_kept: false,
        ..w
    };
    let en = crate::app::cut_warnings_note(&[notches]);
    assert!(en.contains("fold notches of \"Dos\" not exported"), "{en}");
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
    assert_eq!(crate::app::cut_warnings_note(&[]), "");
}

// ----- US-07.1 : pince ----------------------------------------------------------------------

fn darts_of(h: &Harness<'_, App>) -> Vec<pf_core::PieceDart> {
    h.state().editor.project().pieces[0].darts.clone()
}

/// Pose une pince : sommet (50 ; 40), bord bas visé près de (30 ; 1), ouverture saisie.
fn place_dart(h: &mut Harness<'_, App>, opening: &str) {
    key(h, Key::Y);
    assert_eq!(h.state().editor.tool, Tool::Dart);
    click(h, pt(50.0, 40.0));
    assert!(
        matches!(h.state().editor.state, ToolState::DartApex { .. }),
        "{}",
        h.state().editor.status.text
    );
    click(h, pt(30.0, 1.0));
    assert!(
        matches!(h.state().editor.state, ToolState::DartBase { .. }),
        "{}",
        h.state().editor.status.text
    );
    h.run_steps(2);
    type_text(h, opening);
    key(h, Key::Enter);
}

#[test]
fn us071_poser_une_pince_une_seule_annulation_et_controle_de_la_ligne() {
    let mut h = piece_harness();
    let before = h.state().editor.project().clone();
    place_dart(&mut h, "10");
    let darts = darts_of(&h);
    assert_eq!(darts.len(), 1, "{}", h.state().editor.status.text);
    assert!((darts[0].opening_mm - 10.0).abs() < 1e-9);
    assert!((darts[0].length_mm - 40.0).abs() < 1e-9);
    // Pince sélectionnée : fenêtre avec les mesures (ligne de 100 mm, 90 mm après pliage).
    h.run_steps(3);
    h.get_by_label("PINCE");
    h.get_by_label_contains("avant pliage");
    let piece = h.state().editor.project().pieces[0].id;
    let calc = h.state().editor.project().piece_darts(piece)[0]
        .calc
        .clone()
        .unwrap();
    assert!((calc.closed.before_mm - 100.0).abs() < 1e-9);
    assert!((calc.closed.after_mm - 90.0).abs() < 1e-9);
    // Une seule annulation défait la pince et les deux points créés avec elle.
    undo(&mut h);
    assert_eq!(*h.state().editor.project(), before);
}

#[test]
fn us071_refus_sans_trace_sommet_hors_piece_et_ouverture_nulle() {
    let mut h = piece_harness();
    let before = h.state().editor.project().clone();
    // Sommet hors de la pièce : message, aucun état d'outil, aucune trace.
    key(&mut h, Key::Y);
    click(&mut h, pt(50.0, 100.0));
    assert!(
        h.state().editor.status.error,
        "{} {:?}",
        h.state().editor.status.text,
        h.state().editor.state
    );
    assert_eq!(h.state().editor.state, ToolState::Idle);
    assert_eq!(*h.state().editor.project(), before);
    // Ouverture nulle : le cœur refuse, rien n'est créé (ni dart ni points).
    place_dart(&mut h, "0");
    assert!(
        h.state().editor.status.error,
        "{}",
        h.state().editor.status.text
    );
    assert_eq!(*h.state().editor.project(), before);
    assert!(matches!(h.state().editor.state, ToolState::DartBase { .. }));
    // Échap annule le geste.
    key(&mut h, Key::Escape);
    assert_eq!(h.state().editor.state, ToolState::Idle);
}

#[test]
fn us071_changer_de_cote_et_supprimer_une_pince_en_anglais() {
    pf_i18n::set_lang(pf_i18n::Lang::En);
    let mut h = piece_harness();
    place_dart(&mut h, "10");
    assert_eq!(darts_of(&h).len(), 1);
    assert_eq!(darts_of(&h)[0].fixed, pf_geometry::Side::Left);
    h.run_steps(3);
    h.get_by_label("DART");
    h.get_by_label_contains("Tool: Dart");
    h.get_by_label_contains("before folding");
    h.get_by_label("Switch side").click();
    h.run_steps(3);
    assert_eq!(darts_of(&h)[0].fixed, pf_geometry::Side::Right);
    undo(&mut h);
    assert_eq!(darts_of(&h)[0].fixed, pf_geometry::Side::Left);
    h.get_all_by_label("Delete").last().unwrap().click();
    h.run_steps(3);
    assert!(darts_of(&h).is_empty());
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
}

// ----- PH-5a : couper pince fermée -----------------------------------------------------------

fn cut_vertices(h: &Harness<'_, App>) -> Vec<PointMm> {
    let piece = h.state().editor.project().pieces[0].id;
    h.state()
        .editor
        .project()
        .piece_cut_contour(piece)
        .unwrap()
        .traces
        .iter()
        .flat_map(pf_geometry::region::trace_geometry)
        .map(|g| g.start())
        .collect()
}

fn set_margin(h: &mut Harness<'_, App>, mm: f64) {
    let piece = h.state().editor.project().pieces[0].id;
    h.state_mut()
        .editor
        .exec(pf_core::Command::SetSeamAllowance {
            piece,
            seam: Some(pf_core::SeamAllowance {
                default_mm: mm,
                ..Default::default()
            }),
        })
        .unwrap();
    h.run_steps(2);
}

#[test]
fn ph5a_cocher_couper_pince_fermee_change_la_coupe_et_s_annule() {
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
    let mut h = piece_harness();
    set_margin(&mut h, 10.0);
    place_dart(&mut h, "10");
    h.run_steps(3);
    // Rectangle 100 × 60, marge 10 : quatre coins ; pince (50 ; 40) → (50 ; 0), ouverture 10.
    assert_eq!(cut_vertices(&h).len(), 4);
    let before = h.state().editor.project().clone();
    h.get_by_label("Couper pince fermée").click();
    h.run_steps(3);
    assert!(darts_of(&h)[0].cut_closed);
    // Coupe recalculée à la main : pointe de marge J (43,75 ; −10) et J' (56,25 ; −10).
    let cut = cut_vertices(&h);
    assert_eq!(cut.len(), 6, "{cut:?}");
    for want in [pt(43.75, -10.0), pt(56.25, -10.0)] {
        assert!(
            cut.iter()
                .any(|q| (q.x - want.x).abs() < 1e-4 && (q.y - want.y).abs() < 1e-4),
            "{want:?} absent de {cut:?}"
        );
    }
    // Aucun échec affiché.
    assert!(h
        .query_all_by_label_contains("Coupe pince fermée impossible")
        .next()
        .is_none());
    // Une seule annulation : le projet est celui d'avant.
    undo(&mut h);
    assert_eq!(*h.state().editor.project(), before);
    assert_eq!(cut_vertices(&h).len(), 4);
    h.key_press_modifiers(Modifiers::COMMAND, Key::Y);
    h.run_steps(2);
    assert!(darts_of(&h)[0].cut_closed);
    assert_eq!(cut_vertices(&h).len(), 6);
}

#[test]
fn ph5a_bouche_sur_un_coin_echec_affiche_en_anglais_coupe_ordinaire_conservee() {
    pf_i18n::set_lang(pf_i18n::Lang::En);
    let mut h = piece_harness();
    set_margin(&mut h, 10.0);
    // Pince dont un pied est au coin (0 ; 0) : apex (5 ; 40), pieds (0 ; 0) et (10 ; 0).
    key(&mut h, Key::Y);
    click(&mut h, pt(5.0, 40.0));
    click(&mut h, pt(30.0, 1.0));
    h.run_steps(2);
    type_text(&mut h, "10");
    key(&mut h, Key::Enter);
    h.run_steps(3);
    assert_eq!(darts_of(&h).len(), 1, "{}", h.state().editor.status.text);
    h.get_by_label("Cut with the dart closed").click();
    h.run_steps(3);
    // Le réglage est accepté (jamais refusé pour un calcul impossible) ; la coupe est l'ordinaire.
    assert!(darts_of(&h)[0].cut_closed);
    assert_eq!(cut_vertices(&h).len(), 4);
    // Texte et symbole : « ⚠ Closed-dart cut not possible, ordinary cut kept: … ».
    h.get_by_label_contains("⚠ Closed-dart cut not possible, ordinary cut kept");
    let piece = h.state().editor.project().pieces[0].id;
    assert!(h
        .state()
        .editor
        .project()
        .piece_issues(piece)
        .iter()
        .any(|i| matches!(i, pf_core::piece::PieceIssue::CutDartFailed { .. })));
    // Décocher : plus d'échec, retour à l'état d'avant le réglage.
    h.get_by_label("Cut with the dart closed").click();
    h.run_steps(3);
    assert!(!darts_of(&h)[0].cut_closed);
    assert!(h
        .query_all_by_label_contains("Closed-dart cut not possible")
        .next()
        .is_none());
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
}

// ----- PH-5b : bord sur le pli ----------------------------------------------------------------

/// Bord gauche (x = 0) du contour de la première pièce.
fn left_edge(h: &Harness<'_, App>) -> pf_core::EntityId {
    let p = h.state().editor.project();
    let sheet = p.piece_sheet(p.pieces[0].id).unwrap();
    p.pieces[0]
        .outline
        .iter()
        .find_map(|o| {
            let (a, b) = sheet.segment_ends(o.entity).ok()?;
            (a.x.abs() < 1e-9 && b.x.abs() < 1e-9).then_some(o.entity)
        })
        .expect("bord gauche")
}

fn cut_measures(h: &Harness<'_, App>) -> (f64, f64) {
    let c = h
        .state()
        .editor
        .project()
        .piece_cut_contour(piece_of(h).id)
        .unwrap();
    (c.area_mm2, c.perimeter_mm)
}

#[test]
fn ph5b_cocher_sur_le_pli_supprime_la_marge_du_bord_et_s_annule() {
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
    let mut h = piece_harness();
    h.set_size(egui::vec2(1280.0, 1600.0));
    set_margin(&mut h, 10.0);
    open_fold(&mut h, "piece.seam.title");
    // Sans bord sélectionné : consigne écrite, aucune case.
    h.get_by_label_contains("Sélectionnez un bord rectiligne du contour");
    assert!(h.query_by_label("Sur le pli").is_none());
    // Rectangle 100 × 60, marge 10 : coupe 120 × 80 = 9600 mm², 400 mm.
    assert_eq!(cut_measures(&h), (9600.0, 400.0));
    let before = h.state().editor.project().clone();
    let left = left_edge(&h);
    h.state_mut().editor.selection.entities = vec![left];
    h.run_steps(3);
    h.get_by_label("Sur le pli").click();
    h.run_steps(3);
    assert!(
        !h.state().editor.status.error,
        "{}",
        h.state().editor.status.text
    );
    assert_eq!(piece_of(&h).on_fold, vec![left]);
    // Pliure à gauche : coupe [0 ; 110] × [−10 ; 70] = 110 × 80 = 8800 mm², périmètre 380 mm.
    let (area, per) = cut_measures(&h);
    assert!((area - 8800.0).abs() < 1e-6 && (per - 380.0).abs() < 1e-6);
    // Le rôle est écrit (jamais la couleur seule) : nombre et longueur observés (60 mm).
    h.get_by_label_contains("1 bord(s) sur le pli, 60");
    h.get_by_label_contains("Sur le pli (1)");
    // Une seule annulation rend le projet d'avant ; rétablir redéclare le bord.
    undo(&mut h);
    assert_eq!(*h.state().editor.project(), before);
    h.key_press_modifiers(Modifiers::COMMAND, Key::Y);
    h.run_steps(2);
    assert_eq!(piece_of(&h).on_fold, vec![left]);
    // Décocher : le bord redevient une coupe à 10 mm (une étape, retrait annulable).
    h.get_by_label("Sur le pli").click();
    h.run_steps(3);
    assert!(piece_of(&h).on_fold.is_empty());
    assert_eq!(cut_measures(&h), (9600.0, 400.0));
}

#[test]
fn ph5b_en_anglais_libelle_d_export_et_retrait() {
    pf_i18n::set_lang(pf_i18n::Lang::En);
    let mut h = piece_harness();
    h.set_size(egui::vec2(1280.0, 1600.0));
    set_margin(&mut h, 10.0);
    open_fold(&mut h, "piece.seam.title");
    h.get_by_label_contains("Select a straight outline edge");
    let left = left_edge(&h);
    h.state_mut().editor.selection.entities = vec![left];
    h.run_steps(3);
    h.get_by_label("On fold").click();
    h.run_steps(3);
    assert_eq!(piece_of(&h).on_fold, vec![left]);
    h.get_by_label_contains("1 edge(s) on fold, 60");
    h.get_by_label_contains("On fold (1)");
    // L'export écrit le libellé dans la langue de l'interface.
    let sheet = h.state().editor.sheet;
    let scene = h.state().export_scene(sheet).unwrap();
    assert_eq!(scene.folds.len(), 1);
    assert_eq!(scene.folds[0].label, "ON FOLD");
    let svg = pf_export::svg::export_svg(&scene).unwrap();
    assert!(svg.contains(">ON FOLD</text>") && !svg.contains("SUR LE PLI"));
    // « Remove » retire la déclaration (une étape).
    h.get_by_label("Remove").click();
    h.run_steps(3);
    assert!(piece_of(&h).on_fold.is_empty());
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
    let scene = h.state().export_scene(sheet).unwrap();
    assert!(scene.folds.is_empty());
}

// ----- PH-5d : pièce sans droit-fil ------------------------------------------------------------

/// Pose un droit-fil (segment de rôle droit-fil) sur la première pièce, par commandes.
fn give_grainline(h: &mut Harness<'_, App>) {
    let piece = piece_of(h);
    let ed = &mut h.state_mut().editor;
    let sheet = ed.sheet;
    let g = ed
        .exec(pf_core::Command::AddSegment {
            sheet,
            from: pf_core::PointRef::New(pt(50.0, 10.0)),
            to: pf_core::PointRef::New(pt(50.0, 50.0)),
        })
        .unwrap()
        .entities[0];
    ed.exec(pf_core::Command::SetPathRole {
        sheet,
        entity: g,
        role: pf_core::PathRole::Grainline,
    })
    .unwrap();
    ed.exec(pf_core::Command::SetPieceOutline {
        piece: piece.id,
        outline: piece.outline,
        grainline: Some(g),
    })
    .unwrap();
    ed.selection.clear();
    h.run_steps(3);
}

#[test]
fn ph5d_cocher_sans_droit_fil_retire_l_avertissement_et_s_annule() {
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
    let mut h = piece_harness();
    // Avant : l'avertissement est écrit, la case est proposée et décochée.
    h.get_by_label_contains("droit-fil manquant");
    let before = h.state().editor.project().clone();
    h.get_by_label("Sans droit-fil (cuir)").click();
    h.run_steps(3);
    assert!(
        !h.state().editor.status.error,
        "{}",
        h.state().editor.status.text
    );
    assert!(piece_of(&h).no_grain);
    assert!(h
        .query_all_by_label_contains("droit-fil manquant")
        .next()
        .is_none());
    // La case reste affichée (cochée) : l'état est un texte, pas une couleur.
    h.get_by_label("Sans droit-fil (cuir)");
    // Une seule annulation rend le projet d'avant et l'avertissement ; rétablir redéclare.
    undo(&mut h);
    assert_eq!(*h.state().editor.project(), before);
    h.get_by_label_contains("droit-fil manquant");
    h.key_press_modifiers(Modifiers::COMMAND, Key::Y);
    h.run_steps(2);
    assert!(piece_of(&h).no_grain);
    // Décocher : retour de l'avertissement (une étape).
    h.get_by_label("Sans droit-fil (cuir)").click();
    h.run_steps(3);
    assert!(!piece_of(&h).no_grain);
    h.get_by_label_contains("droit-fil manquant");
}

#[test]
fn ph5d_en_anglais_case_grisee_avec_droit_fil_et_mention_d_export() {
    pf_i18n::set_lang(pf_i18n::Lang::En);
    let mut h = piece_harness();
    h.get_by_label_contains("grainline missing");
    h.get_by_label("No grain (leather)").click();
    h.run_steps(3);
    assert!(piece_of(&h).no_grain);
    assert!(h
        .query_all_by_label_contains("grainline missing")
        .next()
        .is_none());
    // L'export écrit la mention dans la langue de l'interface.
    let sheet = h.state().editor.sheet;
    let scene = h.state().export_scene(sheet).unwrap();
    assert_eq!(scene.piece_labels.len(), 1);
    assert_eq!(
        scene.piece_labels[0].lines.last().map(String::as_str),
        Some("No grain")
    );
    // Décochée puis avec un droit-fil : la case est grisée (jamais refusée à la souris).
    h.get_by_label("No grain (leather)").click();
    h.run_steps(3);
    assert!(!piece_of(&h).no_grain);
    give_grainline(&mut h);
    assert!(piece_of(&h).grainline.is_some());
    assert!(h
        .get_by_label("No grain (leather)")
        .accesskit_node()
        .is_disabled());
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
}

// ----- US-07.2 : transférer la pince ---------------------------------------------------------

fn piece_edge_count(h: &Harness<'_, App>) -> usize {
    h.state().editor.project().pieces[0].outline.len()
}

#[test]
fn us072_transferer_la_pince_une_seule_annulation_et_refus_sans_trace() {
    let mut h = piece_harness();
    place_dart(&mut h, "10");
    h.run_steps(3);
    let with_dart = h.state().editor.project().clone();
    // Cible sur la base de la pince (bord bas, entre les pieds) : refus, rien ne change.
    h.get_by_label("Transférer la pince").click();
    h.run_steps(2);
    assert!(h.state().editor.dart_transfer.armed);
    click(&mut h, pt(50.0, 1.0));
    h.run_steps(2);
    assert!(
        h.state().editor.status.error,
        "{}",
        h.state().editor.status.text
    );
    assert_eq!(*h.state().editor.project(), with_dart);
    assert!(h.state().editor.dart_transfer.armed, "le geste reste armé");
    // Part saisie invalide en mode partiel : refus avant la commande, aucune trace.
    h.state_mut().editor.dart_transfer.partial = true;
    h.state_mut().editor.dart_transfer.percent = "0".into();
    click(&mut h, pt(30.0, 59.0));
    h.run_steps(2);
    assert!(h.state().editor.status.error);
    assert_eq!(*h.state().editor.project(), with_dart);
    // Transfert total vers le bord haut : la pince est à la cible, une seule annulation.
    h.state_mut().editor.dart_transfer.partial = false;
    click(&mut h, pt(30.0, 59.0));
    h.run_steps(3);
    assert!(
        !h.state().editor.status.error,
        "{}",
        h.state().editor.status.text
    );
    assert!(!h.state().editor.dart_transfer.armed);
    let darts = darts_of(&h);
    assert_eq!(darts.len(), 1);
    assert_eq!(piece_edge_count(&h), 7);
    // L'affichage suit la pièce reconstruite sur sa nouvelle feuille ; la fenêtre reste ouverte.
    let piece = h.state().editor.project().pieces[0].id;
    let sheet = h.state().editor.project().piece_sheet(piece).unwrap().id;
    assert_eq!(h.state().editor.sheet, sheet);
    h.get_by_label("PINCE");
    let calc = h.state().editor.project().piece_darts(piece)[0]
        .calc
        .clone()
        .expect("pince calculable à la cible");
    // Somme des angles conservée : 2·atan(5 / 40).
    assert!((calc.shape.angle_deg - 2.0 * (5.0f64 / 40.0).atan().to_degrees()).abs() < 1e-6);
    undo(&mut h);
    assert_eq!(*h.state().editor.project(), with_dart);
}

#[test]
fn us072_transfert_partiel_deux_pinces_en_anglais() {
    pf_i18n::set_lang(pf_i18n::Lang::En);
    let mut h = piece_harness();
    place_dart(&mut h, "10");
    h.run_steps(3);
    h.get_by_label("Partial transfer").click();
    h.run_steps(2);
    assert!(h.state().editor.dart_transfer.partial);
    h.get_by_label("Transfer the dart").click();
    h.run_steps(2);
    h.get_by_label_contains("click the piece edge");
    click(&mut h, pt(30.0, 59.0));
    h.run_steps(3);
    assert!(
        !h.state().editor.status.error,
        "{}",
        h.state().editor.status.text
    );
    assert_eq!(darts_of(&h).len(), 2);
    let piece = h.state().editor.project().pieces[0].id;
    let sum: f64 = h
        .state()
        .editor
        .project()
        .piece_darts(piece)
        .iter()
        .map(|r| r.calc.as_ref().unwrap().shape.angle_deg)
        .sum();
    assert!((sum - 2.0 * (5.0f64 / 40.0).atan().to_degrees()).abs() < 1e-6);
    h.get_by_label_contains("2 dart(s)");
    undo(&mut h);
    assert_eq!(darts_of(&h).len(), 1);
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
}

// ----- US-07.3 : plis ------------------------------------------------------------------------

fn pleats_of(h: &Harness<'_, App>) -> Vec<pf_core::PiecePleat> {
    h.state().editor.project().pieces[0].pleats.clone()
}

/// Ouvre la fenêtre « Plis » et pose la ligne `from` → `to` par deux clics sur le canevas.
fn open_pleats_with_line(h: &mut Harness<'_, App>, from: PointMm, to: PointMm) {
    h.state_mut().open_pleats();
    h.run_steps(3);
    let label = if pf_i18n::lang() == pf_i18n::Lang::En {
        "Place the line (2 clicks)"
    } else {
        "Poser la ligne (2 clics)"
    };
    h.get_by_label(label).click();
    h.run_steps(2);
    assert_eq!(h.state().editor.pleat.picking, 1);
    click(h, from);
    assert_eq!(h.state().editor.pleat.picking, 2);
    click(h, to);
    assert_eq!(h.state().editor.pleat.picking, 0);
    h.run_steps(2);
}

#[test]
fn us073_poser_un_pli_une_seule_annulation_et_apercu_replie() {
    let mut h = piece_harness();
    let before = h.state().editor.project().clone();
    open_pleats_with_line(&mut h, pt(50.0, -10.0), pt(50.0, 70.0));
    h.state_mut().editor.pleat.depth = "5".into();
    h.run_steps(2);
    // Aperçu : 10 mm ajoutés (pli plat de 5).
    h.get_by_label_contains("Largeur ajoutée : 10");
    assert_eq!(
        *h.state().editor.project(),
        before,
        "l'aperçu ne change rien"
    );
    h.get_by_label("Poser les plis").click();
    h.run_steps(3);
    assert!(
        !h.state().editor.status.error,
        "{}",
        h.state().editor.status.text
    );
    let pleats = pleats_of(&h);
    assert_eq!(pleats.len(), 1);
    assert!((pleats[0].width_mm() - 10.0).abs() < 1e-9);
    let piece = h.state().editor.project().pieces[0].id;
    let area = h
        .state()
        .editor
        .project()
        .piece_contour(piece)
        .unwrap()
        .area_mm2;
    assert!((area - 110.0 * 60.0).abs() < 1e-9);
    // La fenêtre propose la pièce repliée, égale à la pièce d'origine.
    h.run_steps(3);
    h.get_by_label_contains("1 pli(s) sur la pièce");
    h.get_by_label_contains("Pièce repliée : 10");
    let folded = h.state().editor.project().piece_folded(piece).unwrap();
    assert!((folded.area_mm2 - 100.0 * 60.0).abs() < 1e-6);
    // Une seule annulation défait la pose.
    undo(&mut h);
    assert_eq!(*h.state().editor.project(), before);
}

#[test]
fn us073_refus_sans_trace_ligne_hors_de_la_piece_et_valeurs_illisibles() {
    let mut h = piece_harness();
    let before = h.state().editor.project().clone();
    h.state_mut().open_pleats();
    h.state_mut().editor.pleat.from = Some(pt(150.0, -10.0));
    h.state_mut().editor.pleat.to = Some(pt(150.0, 70.0));
    h.run_steps(3);
    // Ligne hors de la pièce : l'aperçu donne la raison, le bouton est inactif.
    h.get_by_label_contains("Refus :");
    h.state_mut().editor.pleat_apply();
    assert!(h.state().editor.status.error);
    assert_eq!(*h.state().editor.project(), before);
    // Profondeur illisible : refus avant la commande, aucune trace.
    h.state_mut().editor.pleat.from = Some(pt(50.0, -10.0));
    h.state_mut().editor.pleat.to = Some(pt(50.0, 70.0));
    h.state_mut().editor.pleat.depth = "abc".into();
    h.state_mut().editor.pleat_apply();
    assert!(h.state().editor.status.error);
    assert_eq!(*h.state().editor.project(), before);
    // Échap annule la pose de la ligne.
    h.state_mut().editor.pleat.picking = 1;
    key(&mut h, Key::Escape);
    assert_eq!(h.state().editor.pleat.picking, 0);
}

#[test]
fn us073_plis_en_anglais() {
    pf_i18n::set_lang(pf_i18n::Lang::En);
    let mut h = piece_harness();
    open_pleats_with_line(&mut h, pt(50.0, -10.0), pt(50.0, 70.0));
    h.get_by_label("Hollow pleat").click();
    h.run_steps(2);
    h.state_mut().editor.pleat.depth = "5".into();
    h.run_steps(2);
    h.get_by_label_contains("Added width: 20");
    h.get_by_label("Place the pleats").click();
    h.run_steps(3);
    assert_eq!(pleats_of(&h).len(), 1);
    assert_eq!(pleats_of(&h)[0].kind, pf_core::PleatKind::Hollow);
    h.get_by_label_contains("1 pleat(s) on the piece");
    h.get_by_label_contains("Folded piece: 20");
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
}

// ----- US-07.4 : ampleur ---------------------------------------------------------------------

fn flares_of(h: &Harness<'_, App>) -> Vec<pf_core::PieceFlare> {
    h.state().editor.project().pieces[0].flares.clone()
}

#[test]
fn us074_ampleur_trois_lignes_une_seule_annulation_et_apercu_des_longueurs() {
    let mut h = piece_harness();
    let before = h.state().editor.project().clone();
    h.state_mut().open_flare();
    h.run_steps(3);
    // Aperçu : bord fixe de 100 mm conservé, bord ouvert + 90 mm (3 x 30).
    h.get_by_label_contains("Bord fixe : 100");
    h.get_by_label_contains("Bord ouvert : gagne 90");
    assert_eq!(
        *h.state().editor.project(),
        before,
        "l'aperçu ne change rien"
    );
    h.get_by_label("Ouvrir la pièce").click();
    h.run_steps(3);
    assert!(
        !h.state().editor.status.error,
        "{}",
        h.state().editor.status.text
    );
    let flares = flares_of(&h);
    assert_eq!(flares.len(), 3);
    assert!(flares
        .iter()
        .all(|f| (f.opening_mm() - 30.0).abs() < 1e-9 && (f.length_mm() - 60.0).abs() < 1e-9));
    let piece = h.state().editor.project().pieces[0].id;
    let c = h.state().editor.project().piece_contour(piece).unwrap();
    let theta = 2.0 * (30.0_f64 / 120.0).asin();
    assert!((c.area_mm2 - (6000.0 + 3.0 * 0.5 * 3600.0 * theta.sin())).abs() < 1e-6);
    assert!((c.perimeter_mm - (320.0 + 90.0)).abs() < 1e-6);
    h.run_steps(3);
    h.get_by_label_contains("3 ligne(s) d'ampleur sur la pièce");
    // Une seule annulation défait la pose.
    undo(&mut h);
    assert_eq!(*h.state().editor.project(), before);
}

#[test]
fn us074_refus_sans_trace_valeurs_illisibles_ouverture_trop_grande() {
    let mut h = piece_harness();
    let before = h.state().editor.project().clone();
    h.state_mut().open_flare();
    h.run_steps(3);
    // Valeur illisible : refus avant la commande, aucune trace.
    h.state_mut().editor.flare.opening = "abc".into();
    h.state_mut().editor.flare_apply();
    assert!(h.state().editor.status.error);
    assert_eq!(*h.state().editor.project(), before);
    // Ouverture trop grande pour des lignes de 60 mm : l'aperçu donne la raison, rien n'est posé.
    h.state_mut().editor.flare.opening = "200".into();
    h.run_steps(3);
    h.get_by_label_contains("Refus :");
    h.state_mut().editor.flare_apply();
    assert!(h.state().editor.status.error);
    assert_eq!(*h.state().editor.project(), before);
    // Nombre de lignes hors bornes.
    h.state_mut().editor.flare.opening = "10".into();
    h.state_mut().editor.flare.count = "51".into();
    h.state_mut().editor.flare_apply();
    assert!(h.state().editor.status.error);
    assert_eq!(*h.state().editor.project(), before);
}

#[test]
fn us074_ampleur_en_anglais() {
    pf_i18n::set_lang(pf_i18n::Lang::En);
    let mut h = piece_harness();
    h.state_mut().open_flare();
    h.run_steps(3);
    h.get_by_label_contains("Fixed edge: 100");
    h.get_by_label_contains("Open edge: gains 90");
    h.get_by_label("Open the piece").click();
    h.run_steps(3);
    assert_eq!(flares_of(&h).len(), 3);
    h.get_by_label_contains("3 flare line(s) on the piece");
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
}

// ----- US-07.5 : découpes et crans de repère -------------------------------------------------

fn cut_sheets_notches(h: &Harness<'_, App>) -> Vec<usize> {
    let p = h.state().editor.project();
    p.pieces
        .iter()
        .map(|pc| {
            let s = p.piece_sheet(pc.id).unwrap();
            let edge = pc.outline.last().unwrap().entity;
            s.notches.iter().filter(|n| n.entity == edge).count()
        })
        .collect()
}

#[test]
fn us075_decouper_une_seule_annulation_et_apercu_des_deux_morceaux() {
    let mut h = piece_harness();
    let before = h.state().editor.project().clone();
    h.state_mut().open_cutline();
    h.run_steps(3);
    // Droite proposée : verticale au milieu (x = 50) ; deux morceaux de 30 cm² (50 x 60 mm), bord de
    // découpe de 60 mm chacun, deux crans au tiers et aux deux tiers (20 et 40 mm du départ).
    h.get_by_label_contains(
        "Morceau 1/2 : aire 30,0 cm², périmètre 220,0 mm, bord de découpe 60,0 mm",
    );
    h.get_by_label_contains(
        "Morceau 2/2 : aire 30,0 cm², périmètre 220,0 mm, bord de découpe 60,0 mm",
    );
    h.get_by_label_contains("2 cran(s) par bord, à 20 ; 40 mm du départ");
    assert_eq!(
        *h.state().editor.project(),
        before,
        "l'aperçu ne change rien"
    );
    h.get_by_label("Découper la pièce").click();
    h.run_steps(3);
    assert!(
        !h.state().editor.status.error,
        "{}",
        h.state().editor.status.text
    );
    assert_eq!(h.state().editor.project().pieces.len(), 2);
    assert_eq!(cut_sheets_notches(&h), vec![2, 2]);
    // Une seule annulation défait la découpe et les quatre crans.
    undo(&mut h);
    assert_eq!(*h.state().editor.project(), before);
}

#[test]
fn us075_refus_sans_trace_valeurs_illisibles_ligne_hors_de_la_piece() {
    let mut h = piece_harness();
    let before = h.state().editor.project().clone();
    h.state_mut().open_cutline();
    h.run_steps(3);
    h.state_mut().editor.cutline.ax = "abc".into();
    h.state_mut().editor.cutline_apply();
    assert!(h.state().editor.status.error);
    assert_eq!(*h.state().editor.project(), before);
    // Ligne qui passe hors de la pièce : l'aperçu donne la raison, rien n'est posé.
    {
        let ui = &mut h.state_mut().editor.cutline;
        ui.ax = "300".into();
        ui.ay = "0".into();
        ui.bx = "300".into();
        ui.by = "60".into();
    }
    h.run_steps(3);
    h.get_by_label_contains("Refus :");
    h.state_mut().editor.cutline_apply();
    assert!(h.state().editor.status.error);
    assert_eq!(*h.state().editor.project(), before);
    // Nombre de crans hors bornes.
    {
        let ui = &mut h.state_mut().editor.cutline;
        ui.ax = "50".into();
        ui.ay = "-10".into();
        ui.bx = "50".into();
        ui.by = "70".into();
        ui.count = "51".into();
    }
    h.state_mut().editor.cutline_apply();
    assert!(h.state().editor.status.error);
    assert_eq!(*h.state().editor.project(), before);
}

#[test]
fn us075_decouper_en_anglais() {
    pf_i18n::set_lang(pf_i18n::Lang::En);
    let mut h = piece_harness();
    h.state_mut().open_cutline();
    h.run_steps(3);
    h.get_by_label_contains("Piece 1/2: area 30.0 cm², perimeter 220.0 mm, cut edge 60.0 mm");
    h.get_by_label_contains("2 notch(es) per edge, at 20 ; 40 mm from the start");
    h.get_by_label("Cut the piece").click();
    h.run_steps(3);
    assert_eq!(h.state().editor.project().pieces.len(), 2);
    assert_eq!(cut_sheets_notches(&h), vec![2, 2]);
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
}

// ----- US-08.1 : coutures appariées ------------------------------------------------------------

/// Deux pièces rectangulaires : « Devant » 100 x 60 en (0 ; 0) et « Dos » 80 x 60 en (300 ; 0).
fn seams_harness() -> Harness<'static, App> {
    let mut h = harness();
    {
        let ed = &mut h.state_mut().editor;
        ed.grid_snap_mm = None;
        let sheet = ed.sheet;
        for (x, w, name, at) in [(0.0, 100.0, "Devant", 50.0), (300.0, 80.0, "Dos", 340.0)] {
            ed.exec(pf_core::Command::AddRectangle {
                sheet,
                corner: pf_core::PointRef::New(pt(x, 0.0)),
                width_mm: w,
                height_mm: 60.0,
            })
            .unwrap();
            ed.exec(pf_core::Command::ExtractPiece {
                sheet,
                source: pf_core::PieceSource::Click { at: pt(at, 30.0) },
                name: name.into(),
                detached: false,
            })
            .unwrap();
        }
        ed.selection.clear();
    }
    h.run_steps(3);
    h
}

#[test]
fn us081_declarer_une_couture_une_seule_annulation_et_tableau() {
    let mut h = seams_harness();
    let before = h.state().editor.project().clone();
    h.state_mut().open_seams();
    h.run_steps(3);
    h.get_by_label("Aucun couple déclaré.");
    h.get_by_label("Déclarer la couture").click();
    h.run_steps(3);
    assert!(
        !h.state().editor.status.error,
        "{}",
        h.state().editor.status.text
    );
    assert_eq!(h.state().editor.project().seams.len(), 1);
    // Premier tracé de chaque pièce : 100 et 80 mm ; écart 20 ; embu mesuré 20, attendu 0 :
    // hors de la tolérance par défaut (1 mm), statut en texte (et icône dessinée).
    h.get_by_label("100,0 / 80,0");
    h.get_by_label("20,0");
    h.get_by_label("0,0 / 20,0");
    h.get_by_label("Hors tolérance");
    // Mise à jour en direct : l'embu attendu de 20 rend le couple conforme, sans rien recréer.
    h.state_mut().editor.seam.sel_embu = "20".into();
    h.state_mut().editor.seam_update();
    h.run_steps(3);
    h.get_by_label("Dans la tolérance");
    assert_eq!(h.state().editor.project().seams.len(), 1);
    // Retrait puis annulation : une étape chacune ; deux annulations ramènent le projet d'avant.
    h.state_mut().editor.seam_remove();
    h.run_steps(2);
    assert!(h.state().editor.project().seams.is_empty());
    undo(&mut h);
    assert_eq!(h.state().editor.project().seams.len(), 1);
    assert_eq!(h.state().editor.project().seams[0].embu_mm, Some(20.0));
    undo(&mut h);
    undo(&mut h);
    assert_eq!(*h.state().editor.project(), before);
}

#[test]
fn us081_refus_sans_trace_valeurs_hors_bornes_et_trace_deja_cousu() {
    let mut h = seams_harness();
    h.state_mut().open_seams();
    h.run_steps(3);
    let before = h.state().editor.project().clone();
    h.state_mut().editor.seam.embu = "5000".into();
    h.state_mut().editor.seam_declare();
    assert!(h.state().editor.status.error);
    assert_eq!(*h.state().editor.project(), before);
    h.state_mut().editor.seam.embu = "abc".into();
    h.state_mut().editor.seam_declare();
    assert!(h.state().editor.status.error);
    assert_eq!(*h.state().editor.project(), before);
    // Un même tracé ne peut être cousu deux fois.
    h.state_mut().editor.seam.embu = "20".into();
    h.state_mut().editor.seam_declare();
    assert_eq!(h.state().editor.project().seams.len(), 1);
    let one = h.state().editor.project().clone();
    h.state_mut().editor.seam_declare();
    assert!(h.state().editor.status.error);
    assert_eq!(*h.state().editor.project(), one);
}

#[test]
fn us081_coutures_en_anglais() {
    pf_i18n::set_lang(pf_i18n::Lang::En);
    let mut h = seams_harness();
    h.state_mut().open_seams();
    h.run_steps(3);
    h.get_by_label("No pair declared.");
    h.get_by_label("Declare the seam").click();
    h.run_steps(3);
    assert_eq!(h.state().editor.project().seams.len(), 1);
    h.get_by_label("100.0 / 80.0");
    h.get_by_label("Out of tolerance");
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
}

#[test]
fn us081_choisir_deux_bords_sur_le_plan_puis_echap() {
    let mut h = seams_harness();
    h.state_mut().open_seams();
    h.state_mut().editor.seam.picking = 1;
    // A : bord du haut de « Devant » (y = 60) ; B : bord du bas de « Dos » (y = 0).
    h.state_mut().editor.click(pt(50.0, 60.0), 2.0, false);
    assert_eq!(h.state().editor.seam.picking, 2);
    h.state_mut().editor.click(pt(340.0, 0.0), 2.0, false);
    assert_eq!(h.state().editor.seam.picking, 0);
    h.state_mut().editor.seam_declare();
    let p = h.state().editor.project();
    assert_eq!(p.seams.len(), 1, "{}", h.state().editor.status.text);
    let r = &p.seam_pair_report()[0];
    assert_eq!((r.length_a_mm, r.length_b_mm), (Some(100.0), Some(80.0)));
    // Clic dans le vide : refus, la sélection de bord reste en attente ; Échap l'annule.
    h.state_mut().editor.seam.picking = 1;
    h.state_mut().editor.click(pt(200.0, 200.0), 2.0, false);
    assert!(h.state().editor.status.error);
    assert_eq!(h.state().editor.seam.picking, 1);
    h.state_mut().editor.escape();
    assert_eq!(h.state().editor.seam.picking, 0);
}

// ----- US-08.2 : marche de pièces --------------------------------------------------------------

/// Couple « Devant » (100 mm) / « Dos » (80 mm) déclaré, choisi, avec deux crans sur le bord de A.
fn march_harness() -> (Harness<'static, App>, pf_core::SeamPair) {
    let mut h = seams_harness();
    h.state_mut().open_seams();
    h.run_steps(3);
    h.get_by_label("Déclarer la couture").click();
    h.run_steps(3);
    let pair = h.state().editor.project().seams[0].clone();
    {
        let ed = &mut h.state_mut().editor;
        let sheet = ed.sheet;
        for d in [25.0, 75.0] {
            ed.exec(pf_core::Command::AddNotch {
                sheet,
                entity: pair.a.edges[0],
                distance_mm: d,
                side: pf_geometry::Side::Left,
                length_mm: 5.0,
                style: pf_core::NotchStyle::Single,
            })
            .unwrap();
        }
        ed.seam.selected = Some(pair.id);
    }
    h.run_steps(3);
    (h, pair)
}

fn notches_on(h: &Harness<'_, App>, entity: pf_core::EntityId) -> usize {
    let ed = &h.state().editor;
    ed.project()
        .sheet(ed.sheet)
        .unwrap()
        .notches
        .iter()
        .filter(|n| n.entity == entity)
        .count()
}

#[test]
fn us082_reporter_les_crans_une_seule_annulation_et_refus() {
    let (mut h, pair) = march_harness();
    let before = h.state().editor.project().clone();
    // Aperçu en texte : 2 crans, sens de couture opposé (deux contours anti-horaires).
    h.get_by_label_contains("2 cran(s) sur le côté source");
    h.get_by_label("Reporter les crans").click();
    h.run_steps(3);
    assert!(
        !h.state().editor.status.error,
        "{}",
        h.state().editor.status.text
    );
    assert_eq!(notches_on(&h, pair.b.edges[0]), 2);
    // Une seule annulation ramène le projet d'avant.
    undo(&mut h);
    assert_eq!(*h.state().editor.project(), before);
    assert_eq!(notches_on(&h, pair.b.edges[0]), 0);
    // Reporté de nouveau ; rejoué : refus sans trace (tout est déjà présent).
    h.state_mut().editor.seam_march();
    let done = h.state().editor.project().clone();
    h.state_mut().editor.seam_march();
    assert!(h.state().editor.status.error);
    assert_eq!(*h.state().editor.project(), done);
    // Sens B vers A : les deux crans reportés sur B sont reportables sur A (déjà présents : 0 à poser).
    h.state_mut().editor.seam.march_from_b = true;
    h.run_steps(3);
    h.get_by_label_contains("2 cran(s) sur le côté source");
}

#[test]
fn us082_aucun_cran_a_reporter_refus_sans_trace() {
    let mut h = seams_harness();
    h.state_mut().open_seams();
    h.run_steps(3);
    h.get_by_label("Déclarer la couture").click();
    h.run_steps(3);
    let id = h.state().editor.project().seams[0].id;
    h.state_mut().editor.seam.selected = Some(id);
    h.run_steps(3);
    h.get_by_label_contains("Aucun cran à reporter dans ce sens");
    let before = h.state().editor.project().clone();
    h.state_mut().editor.seam_march();
    assert!(h.state().editor.status.error);
    assert_eq!(*h.state().editor.project(), before);
}

#[test]
fn us082_marche_en_anglais() {
    pf_i18n::set_lang(pf_i18n::Lang::En);
    let mut h = seams_harness();
    h.state_mut().open_seams();
    h.run_steps(3);
    h.get_by_label("Declare the seam").click();
    h.run_steps(3);
    let pair = h.state().editor.project().seams[0].clone();
    {
        let ed = &mut h.state_mut().editor;
        let sheet = ed.sheet;
        ed.exec(pf_core::Command::AddNotch {
            sheet,
            entity: pair.a.edges[0],
            distance_mm: 50.0,
            side: pf_geometry::Side::Left,
            length_mm: 5.0,
            style: pf_core::NotchStyle::Single,
        })
        .unwrap();
        ed.seam.selected = Some(pair.id);
    }
    h.run_steps(3);
    h.get_by_label_contains("1 notch(es) on the source side");
    h.get_by_label("Carry notches over").click();
    h.run_steps(3);
    assert_eq!(notches_on(&h, pair.b.edges[0]), 1);
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
}

// ----- US-08.4 : contrôle des crans ------------------------------------------------------------

/// Pose un cran sur le tracé donné, à `d` mm de son départ.
fn add_notch(h: &mut Harness<'_, App>, entity: pf_core::EntityId, d: f64) {
    let ed = &mut h.state_mut().editor;
    let sheet = ed.sheet;
    ed.exec(pf_core::Command::AddNotch {
        sheet,
        entity,
        distance_mm: d,
        side: pf_geometry::Side::Left,
        length_mm: 5.0,
        style: pf_core::NotchStyle::Single,
    })
    .unwrap();
}

#[test]
fn us084_section_repliee_puis_manquants_listes_et_reportes_en_une_annulation() {
    let (mut h, pair) = march_harness();
    // Repliée par défaut : aucune lecture affichée.
    assert!(!h.state().editor.seam.check_open);
    assert!(h.query_by_label_contains("manquant(s)").is_none());
    h.get_by_label("Contrôle des crans").click();
    h.run_steps(3);
    assert!(h.state().editor.seam.check_open);
    // A : 100 mm avec crans à 25 et 75 ; B : 80 mm sans cran ; sens opposé : attendus à 60 et 20.
    h.get_by_label_contains("2 cran(s) (2 sur A, 0 sur B), 2 manquant(s), 0 décalé(s)");
    h.get_by_label_contains("1. A : cran à 25,0 mm, aucun vis-à-vis (attendu à 60,0 mm");
    h.get_by_label_contains("2. A : cran à 75,0 mm, aucun vis-à-vis (attendu à 20,0 mm");
    let before = h.state().editor.project().clone();
    h.get_by_label("Reporter les manquants").click();
    h.run_steps(3);
    assert!(
        !h.state().editor.status.error,
        "{}",
        h.state().editor.status.text
    );
    assert_eq!(notches_on(&h, pair.b.edges[0]), 2);
    // Plus aucun manquant : chaque cran a son vis-à-vis (mise à jour en direct).
    h.get_by_label_contains("4 cran(s) (2 sur A, 2 sur B), 0 manquant(s), 0 décalé(s)");
    h.get_by_label("Chaque cran a son vis-à-vis.");
    // Une seule annulation ramène le projet d'avant.
    undo(&mut h);
    assert_eq!(*h.state().editor.project(), before);
    h.run_steps(3);
    h.get_by_label_contains("2 manquant(s)");
}

#[test]
fn us084_refus_propre_quand_rien_a_reporter_et_aucune_trace() {
    let (mut h, pair) = march_harness();
    h.state_mut().editor.seam_march();
    assert!(!h.state().editor.status.error);
    h.state_mut().editor.seam.check_open = true;
    h.run_steps(3);
    h.get_by_label("Chaque cran a son vis-à-vis.");
    let before = h.state().editor.project().clone();
    h.state_mut().editor.seam_march_missing();
    assert!(h.state().editor.status.error);
    assert_eq!(*h.state().editor.project(), before);
    assert_eq!(notches_on(&h, pair.b.edges[0]), 2);
}

#[test]
fn us084_decale_manquants_des_deux_cotes_en_une_seule_etape() {
    let (mut h, pair) = march_harness();
    // A = 100 mm, B = 80 mm (rapport 0,8, sens opposé) : le cran 25 de A attend 60 sur B, le cran 75
    // attend 20. B : un cran à 62 (écart 2 : décalé) et un cran à 5 (attendu 93,75 sur A : aucun
    // cran de A à moins de 10 mm : manquant) ; le cran 75 de A n'a rien près de 20 : manquant.
    add_notch(&mut h, pair.b.edges[0], 62.0);
    add_notch(&mut h, pair.b.edges[0], 5.0);
    h.state_mut().editor.seam.check_open = true;
    h.run_steps(3);
    let r = h.state().editor.project().notch_check_report().remove(0);
    assert_eq!(r.count_status(pf_core::NotchMatch::Shifted), 2);
    assert_eq!(r.missing(pf_core::SeamWhich::A).len(), 1);
    assert_eq!(r.missing(pf_core::SeamWhich::B).len(), 1);
    h.get_by_label_contains("manquant(s), 2 décalé(s)");
    h.get_by_label_contains("décalé de 2,0 mm (attendu 60,0, trouvé 62,0)");
    let before = h.state().editor.project().clone();
    h.get_by_label("Reporter les manquants").click();
    h.run_steps(3);
    assert!(!h.state().editor.status.error);
    let after = h.state().editor.project().notch_check_report().remove(0);
    assert_eq!(after.count_status(pf_core::NotchMatch::Missing), 0);
    // Deux côtés reportés, mais une seule annulation.
    undo(&mut h);
    assert_eq!(*h.state().editor.project(), before);
}

#[test]
fn us084_couple_non_calculable_dit_pourquoi() {
    let mut h = seams_harness();
    h.state_mut().open_seams();
    h.run_steps(3);
    h.get_by_label("Déclarer la couture").click();
    h.run_steps(3);
    let pair = h.state().editor.project().seams[0].clone();
    let piece = pair.b.piece;
    let mut keep: Vec<pf_core::PieceEdge> = h
        .state()
        .editor
        .project()
        .piece(piece)
        .unwrap()
        .outline
        .clone();
    let drop = keep
        .iter()
        .position(|e| e.entity != pair.b.edges[0])
        .unwrap();
    keep.remove(drop);
    h.state_mut()
        .editor
        .exec(pf_core::Command::SetPieceOutline {
            piece,
            outline: keep,
            grainline: None,
        })
        .unwrap();
    h.state_mut().editor.seam.selected = Some(pair.id);
    h.state_mut().editor.seam.check_open = true;
    h.run_steps(3);
    // Une fois sous le tableau, une fois dans la section « Contrôle des crans ».
    assert_eq!(
        h.query_all_by_label_contains("Couture 1, côté B : contour ouvert")
            .count(),
        2
    );
}

#[test]
fn us084_controle_des_crans_en_anglais() {
    pf_i18n::set_lang(pf_i18n::Lang::En);
    let mut h = seams_harness();
    h.state_mut().open_seams();
    h.run_steps(3);
    h.get_by_label("Declare the seam").click();
    h.run_steps(3);
    let pair = h.state().editor.project().seams[0].clone();
    for d in [25.0, 75.0] {
        add_notch(&mut h, pair.a.edges[0], d);
    }
    h.state_mut().editor.seam.selected = Some(pair.id);
    h.run_steps(3);
    h.get_by_label("Notch check").click();
    h.run_steps(3);
    h.get_by_label_contains("2 notch(es) (2 on A, 0 on B), 2 missing, 0 shifted");
    assert_eq!(h.query_all_by_label_contains("no counterpart").count(), 2);
    h.get_by_label("Carry over the missing ones").click();
    h.run_steps(3);
    assert_eq!(notches_on(&h, pair.b.edges[0]), 2);
    h.get_by_label("Every notch has its counterpart.");
    h.state_mut().editor.seam_march_missing();
    assert!(h.state().editor.status.error);
    assert!(h.state().editor.status.text.contains("no missing notch"));
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
}

// ----- US-08.3 : contrôle des raccords ---------------------------------------------------------

/// Projet de démonstration des raccords chargé dans l'application, fenêtre Coutures ouverte.
fn junction_harness() -> Harness<'static, App> {
    let mut h = harness();
    {
        let app = h.state_mut();
        app.editor = crate::editor::Editor::new(pf_core::Document::new(
            crate::seams::demo_junction_project().unwrap(),
        ));
        app.open_seams();
    }
    h.run_steps(3);
    h
}

#[test]
fn us083_section_repliee_puis_une_ligne_par_couple_et_extremite_statut_en_texte() {
    let mut h = junction_harness();
    // Repliée par défaut : aucune ligne de raccord, aucun bouton.
    assert!(!h.state().editor.seam.join_open);
    assert!(h.query_by_label("Cassé : creux").is_none());
    assert!(h.query_by_label("Vue assemblée").is_none());
    h.get_by_label("Raccords").click();
    h.run_steps(3);
    assert!(h.state().editor.seam.join_open);
    // Couple 1 : 100° + 70° = 170° (creux, −10°) ; couple 2 : 90° + 110° = 200° (bosse, +20°) ;
    // les deux fins : 90° + 90° = 180°. Quatre lignes, statut en texte.
    h.get_by_label("Cassé : creux");
    h.get_by_label("Cassé : bosse");
    assert_eq!(h.query_all_by_label("Lisse").count(), 2);
    assert_eq!(h.query_all_by_label("Début").count(), 2);
    assert_eq!(h.query_all_by_label("Fin").count(), 2);
    h.get_by_label("170,0°");
    h.get_by_label("-10,0°");
    h.get_by_label("200,0°");
    h.get_by_label("+20,0°");
    assert_eq!(h.query_all_by_label("180,0°").count(), 2);
    assert_eq!(h.query_all_by_label("0,0°").count(), 2);
    // Lecture seule : aucune étape ajoutée au document.
    assert!(!h.state().editor.doc.can_undo());
    // Bouton « Vue assemblée » inactif tant qu'aucun couple n'est choisi, puis fenêtre de dessin.
    let id = h.state().editor.project().seams[0].id;
    h.state_mut().editor.seam.selected = Some(id);
    h.run_steps(3);
    let before = h.state().editor.project().clone();
    h.get_by_label("Vue assemblée").click();
    h.run_steps(3);
    assert!(h.state().editor.seam.join_view);
    h.get_by_label("Vue assemblée : couture 1");
    h.get_by_label_contains("Début : 100,0° + 70,0° = 170,0°");
    h.get_by_label_contains("Écart à la fin de la couture : 0,0 mm");
    assert_eq!(*h.state().editor.project(), before);
    assert!(!h.state().editor.doc.can_undo());
}

#[test]
fn us083_clic_sur_une_ligne_choisit_le_couple_et_le_sommet() {
    let mut h = junction_harness();
    h.state_mut().editor.seam.join_open = true;
    h.run_steps(3);
    assert!(h.state().editor.seam.join_sel.is_none());
    let ids: Vec<_> = h
        .state()
        .editor
        .project()
        .seams
        .iter()
        .map(|s| s.id)
        .collect();
    // Les numéros de couple sont cliquables : le dernier « 2 » est celui d'une ligne de raccord.
    let twos: Vec<_> = h
        .query_all_by_role_and_label(egui::accesskit::Role::Button, "2")
        .collect();
    assert!(!twos.is_empty(), "numéro de couple cliquable");
    twos[twos.len() - 1].click();
    h.run_steps(3);
    assert_eq!(h.state().editor.seam.selected, Some(ids[1]));
    assert_eq!(
        h.state().editor.seam.join_sel.map(|s| s.0),
        Some(ids[1]),
        "sommet choisi"
    );
}

#[test]
fn us083_deplacer_un_point_met_a_jour_le_tableau_et_couple_non_calculable_dit_pourquoi() {
    let mut h = junction_harness();
    h.state_mut().editor.seam.join_open = true;
    h.run_steps(3);
    h.get_by_label("Cassé : creux");
    // Le sommet précédent de « Devant » (1,5 ; −17,4) passe en (100 ; −50) : le bord arrive à la
    // verticale, angle de 180° au lieu de 100° ; 180° + 70° = 250°. Mise à jour en direct.
    {
        let ed = &mut h.state_mut().editor;
        let sheet = ed.sheet;
        let sh = ed.project().sheet(sheet).unwrap();
        let id = sh
            .points
            .iter()
            .map(|p| p.id)
            .find(|id| {
                let q = sh.position(*id).unwrap();
                q.x > 1.0 && q.x < 2.0 && q.y < -17.0 && q.y > -18.0
            })
            .unwrap();
        ed.exec(pf_core::Command::MovePoint {
            sheet,
            point: id,
            to: pt(100.0, -50.0),
        })
        .unwrap();
    }
    h.run_steps(3);
    h.get_by_label("250,0°");
    assert!(h.query_by_label("Cassé : creux").is_none());
    // Contour ouvert (un côté non cité retiré) : « non calculable » avec la raison, sans refus.
    let (piece, keep) = {
        let p = h.state().editor.project();
        let b = p.seams[1].b.edges[0];
        let piece = p.seams[1].b.piece;
        let outline = &p.piece(piece).unwrap().outline;
        let drop = outline.iter().position(|e| e.entity != b).unwrap();
        let keep: Vec<pf_core::PieceEdge> = outline
            .iter()
            .copied()
            .enumerate()
            .filter(|(i, _)| *i != drop)
            .map(|x| x.1)
            .collect();
        (piece, keep)
    };
    h.state_mut()
        .editor
        .exec(pf_core::Command::SetPieceOutline {
            piece,
            outline: keep,
            grainline: None,
        })
        .unwrap();
    h.run_steps(3);
    h.get_by_label_contains("Raccord 2, côté B : contour ouvert");
    assert!(h.query_all_by_label("Non calculable").count() >= 2);
}

#[test]
fn us083_raccords_en_anglais() {
    pf_i18n::set_lang(pf_i18n::Lang::En);
    let mut h = junction_harness();
    assert!(h.query_by_label("Broken: hollow").is_none());
    h.get_by_label("Joins").click();
    h.run_steps(3);
    h.get_by_label("Broken: hollow");
    h.get_by_label("Broken: bump");
    assert_eq!(h.query_all_by_label("Smooth").count(), 2);
    h.get_by_label("170.0°");
    let id = h.state().editor.project().seams[0].id;
    h.state_mut().editor.seam.selected = Some(id);
    h.run_steps(3);
    h.get_by_label("Assembled view").click();
    h.run_steps(3);
    h.get_by_label("Assembled view: seam 1");
    h.get_by_label_contains("Start: 100.0° + 70.0° = 170.0°");
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
}

// ----- US-08.5 : rapport de contrôle -----------------------------------------------------------

fn open_report_window(h: &mut Harness<'static, App>, menu: &str, label: &str) {
    h.get_by_label(menu).click();
    h.run_steps(3);
    h.get_by_label(label).click();
    h.run_steps(4);
}

fn us085_scratch(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("pf-us085-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn us085_la_fenetre_resume_les_anomalies_puis_ecrit_pdf_et_csv_sans_toucher_au_projet() {
    let mut h = junction_harness();
    let project_before = h.state().editor.project().clone();
    let json_before = pf_core::persist::to_json(h.state().editor.project()).unwrap();
    let undo_before = h.state().editor.doc.can_undo();
    let report = project_before.control_report();
    let total = report.anomalies.len();
    assert!(
        total > 0,
        "le projet de démonstration a des raccords cassés"
    );
    open_report_window(&mut h, "Fichier", "Exporter le rapport de contrôle…");
    assert!(h.state().report_dialog.is_some());
    h.get_by_label("Rapport de contrôle");
    h.get_by_label_contains("Mesures observées sur le projet");
    h.get_by_label(&format!("Anomalies : {total}"));
    h.get_by_label("PDF (A4, pagination automatique)");
    // Ouvrir la fenêtre n'écrit rien et ne crée aucune étape d'annulation.
    assert_eq!(h.state().editor.doc.can_undo(), undo_before);
    // Fenêtre défilante de 600 points au plus, rangée dans le dock : ni sur le canevas ni sur
    // l'inspecteur (PH-2b).
    let win = h
        .ctx
        .memory(|m| m.area_rect(egui::Id::new("report_window")))
        .expect("fenêtre posée");
    assert!(win.height() <= 600.0 + 1.0, "hauteur {}", win.height());
    assert!(
        win.left() >= h.state().canvas_rect.right(),
        "recouvre le canevas"
    );
    let inspector = egui::containers::panel::PanelState::load(&h.ctx, egui::Id::new("inspector"))
        .expect("inspecteur")
        .rect;
    assert!(win.right() <= inspector.left(), "recouvre l'inspecteur");
    // PDF (format par défaut), chemin injecté.
    let dir = us085_scratch("ok");
    let pdf = dir.join("controle.pdf");
    assert!(h.state_mut().write_report_to(&pdf));
    let bytes = std::fs::read(&pdf).unwrap();
    assert!(bytes.starts_with(b"%PDF-1.4"));
    assert!(h.state().editor.status.text.contains("PDF exporté"));
    assert!(!h.state().editor.status.error);
    // CSV : choix du format dans la fenêtre.
    h.get_by_label_contains("CSV (tableur").click();
    h.run_steps(3);
    let csv = dir.join("controle.csv");
    assert!(h.state_mut().write_report_to(&csv));
    let text = std::fs::read_to_string(&csv).unwrap();
    assert!(text.starts_with('\u{FEFF}'));
    assert!(text.contains("#anomalie;"));
    assert!(text.contains("\r\nanomalie;Raccord cassé;"));
    assert!(h.state().editor.status.text.contains("CSV exporté"));
    h.get_by_label("Fermer sans exporter").click();
    h.run_steps(3);
    assert!(h.state().report_dialog.is_none());
    assert_eq!(
        *h.state().editor.project(),
        project_before,
        "projet inchangé"
    );
    assert_eq!(
        pf_core::persist::to_json(h.state().editor.project()).unwrap(),
        json_before
    );
    assert_eq!(h.state().editor.doc.can_undo(), undo_before);
    assert!(!h.state().editor.doc.is_dirty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn us085_destination_illisible_erreur_propre_sans_fichier_ni_changement() {
    let mut h = junction_harness();
    let project_before = h.state().editor.project().clone();
    let setup_before = h.state().page_setup;
    open_report_window(&mut h, "Fichier", "Exporter le rapport de contrôle…");
    let dir = us085_scratch("refus");
    let path = dir.join("absent").join("rapport.pdf");
    assert!(!h.state_mut().write_report_to(&path));
    assert!(!dir.join("absent").exists(), "aucun dossier créé");
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0, "aucun fichier");
    let msg = h.state().error_dialog.clone().expect("erreur affichée");
    assert!(msg.contains("Export impossible"), "{msg}");
    assert_eq!(*h.state().editor.project(), project_before);
    assert_eq!(h.state().page_setup, setup_before, "préférences inchangées");
    // Sans fenêtre, il n'y a rien à écrire.
    h.state_mut().report_dialog = None;
    assert!(!h.state_mut().write_report_to(&dir.join("rien.pdf")));
    assert!(!dir.join("rien.pdf").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn us085_fenetre_et_fichiers_en_anglais_sans_mot_de_validation() {
    pf_i18n::set_lang(pf_i18n::Lang::En);
    let mut h = junction_harness();
    open_report_window(&mut h, "File", "Export control report…");
    h.get_by_label("Control report");
    h.get_by_label_contains("Measurements observed on the project");
    h.get_by_label_contains("Anomalies: ");
    h.get_by_label("PDF (A4, automatic pagination)");
    let dir = us085_scratch("en");
    h.get_by_label_contains("CSV (spreadsheet").click();
    h.run_steps(3);
    let csv = dir.join("control.csv");
    assert!(h.state_mut().write_report_to(&csv));
    let text = std::fs::read_to_string(&csv).unwrap().to_lowercase();
    assert!(text.contains("broken junction"));
    let text = text.replace(
        "observed measurements, not validated by a pattern maker",
        "",
    );
    for w in [
        "validated",
        "compliant",
        "conforming",
        "ready to",
        "compatible",
    ] {
        assert!(!text.contains(w), "{w}");
    }
    h.get_by_label("Close without exporting").click();
    h.run_steps(3);
    assert!(h.state().report_dialog.is_none());
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn us085_projet_sans_couture_dit_aucune_anomalie() {
    let mut h = harness();
    open_report_window(&mut h, "Fichier", "Exporter le rapport de contrôle…");
    h.get_by_label("Anomalies : 0");
    h.get_by_label("Aucune anomalie.");
    h.get_by_label("Fermer sans exporter").click();
    h.run_steps(3);
}

// ----- PH-4a : construction rapide ------------------------------------------------------------

fn quick_harness() -> Harness<'static, App> {
    let mut h = harness();
    h.state_mut().open_quick();
    h.run_steps(3);
    h
}

#[test]
fn ph4a_rectangle_arrondi_saisi_une_seule_annulation_et_centres_masques() {
    let mut h = quick_harness();
    let before = h.state().editor.project().clone();
    {
        let ui = &mut h.state_mut().editor.quick;
        ui.kind = crate::quick::QuickKind::Rounded;
        (ui.x, ui.y, ui.width, ui.height, ui.radius) = (
            "10".into(),
            "20".into(),
            "100".into(),
            "60".into(),
            "5".into(),
        );
    }
    h.run_steps(3);
    // 2·(100 − 10) + 2·(60 − 10) + 2π·5 = 280 + 10π = 311,42 ; aire 6000 − (4 − π)·25 = 59,79 cm².
    h.get_by_label_contains("Dimensions : 100,0 mm × 60,0 mm.");
    h.get_by_label_contains("Périmètre : 311,42 mm.");
    h.get_by_label_contains("Aire : 59,79 cm².");
    h.get_by_label_contains("4 segment(s), 4 arc(s), 4 centre(s) d'arc.");
    assert_eq!(
        *h.state().editor.project(),
        before,
        "l'aperçu ne change rien"
    );
    h.get_by_label("Créer").click();
    h.run_steps(3);
    assert!(
        !h.state().editor.status.error,
        "{}",
        h.state().editor.status.text
    );
    let sheet = h.state().editor.sheet().clone();
    assert_eq!(sheet.entities.len(), 8, "{}", h.state().editor.status.text);
    // Les quatre centres sont dans le calque masqué : 8 points visibles sur 12.
    assert_eq!(sheet.points.len(), 12);
    assert_eq!(
        sheet
            .points
            .iter()
            .filter(|p| sheet.point_visible(p.id))
            .count(),
        8
    );
    // Une seule annulation défait tout (octets identiques).
    undo(&mut h);
    assert_eq!(*h.state().editor.project(), before);
}

#[test]
fn ph4a_polygone_et_rectangle_saisis() {
    let mut h = quick_harness();
    let before = h.state().editor.project().clone();
    {
        let ui = &mut h.state_mut().editor.quick;
        ui.kind = crate::quick::QuickKind::Polygon;
        ui.vertices = "0;0\n40;0\n50;30\n20;50\n-10;25\n".into();
    }
    h.run_steps(3);
    h.get_by_label_contains("Aire : 20,5 cm².");
    h.get_by_label_contains("5 segment(s), 0 arc(s), 0 centre(s) d'arc.");
    h.get_by_label("Créer").click();
    h.run_steps(3);
    assert_eq!(h.state().editor.sheet().entities.len(), 5);
    undo(&mut h);
    assert_eq!(*h.state().editor.project(), before);
    // Rectangle tourné de 90° et centré : dimensions saisies, 4 tracés.
    {
        let ui = &mut h.state_mut().editor.quick;
        ui.kind = crate::quick::QuickKind::Rectangle;
        (ui.x, ui.y, ui.width, ui.height, ui.angle, ui.centered) = (
            "0".into(),
            "0".into(),
            "100".into(),
            "50".into(),
            "90".into(),
            true,
        );
    }
    h.run_steps(3);
    h.get_by_label_contains("Périmètre : 300,0 mm.");
    h.get_by_label("Créer").click();
    h.run_steps(3);
    let sheet = h.state().editor.sheet();
    assert_eq!(sheet.entities.len(), 4);
    let xs: Vec<f64> = sheet
        .points
        .iter()
        .map(|p| sheet.position(p.id).unwrap().x)
        .collect();
    assert!(xs.iter().all(|x| (x.abs() - 25.0).abs() < 1e-9), "{xs:?}");
    undo(&mut h);
    assert_eq!(*h.state().editor.project(), before);
}

#[test]
fn ph4a_erreur_du_coeur_affichee_sans_effet() {
    let mut h = quick_harness();
    let before = h.state().editor.project().clone();
    {
        let ui = &mut h.state_mut().editor.quick;
        ui.kind = crate::quick::QuickKind::Rounded;
        (ui.width, ui.height, ui.radius) = ("100".into(), "60".into(), "40".into());
    }
    h.run_steps(3);
    // Rayon trop grand : l'aperçu donne la raison et le bouton est inactif.
    h.get_by_label_contains("Refus :");
    h.get_by_label_contains("ayon 40 mm trop grand");
    // Même par la commande directe : refus du cœur, message dans la fenêtre, rien n'est posé.
    h.state_mut().editor.quick_apply();
    h.run_steps(3);
    assert!(h.state().editor.status.error);
    assert!(h.state().editor.quick.error.is_some());
    assert_eq!(*h.state().editor.project(), before);
    // r = 0 : le message propose le rectangle simple.
    h.state_mut().editor.quick.radius = "0".into();
    h.run_steps(3);
    h.get_by_label_contains("rectangle simple");
    // Valeurs illisibles et sommet mal écrit.
    h.state_mut().editor.quick.radius = "abc".into();
    h.state_mut().editor.quick_apply();
    assert!(h.state().editor.status.error);
    h.state_mut().editor.quick.kind = crate::quick::QuickKind::Polygon;
    h.state_mut().editor.quick.vertices = "0;0\n10-10\n".into();
    h.state_mut().editor.quick_apply();
    assert!(h.state().editor.status.error);
    assert_eq!(*h.state().editor.project(), before);
}

#[test]
fn ph4a_construction_rapide_en_anglais() {
    pf_i18n::set_lang(pf_i18n::Lang::En);
    let mut h = quick_harness();
    h.get_by_label("Quick construction");
    h.get_by_label("Rounded rectangle");
    h.get_by_label_contains("Perimeter:");
    h.get_by_label_contains("4 segment(s), 4 arc(s), 4 arc center(s).");
    h.get_by_label("Create").click();
    h.run_steps(3);
    assert_eq!(h.state().editor.sheet().entities.len(), 8);
    h.get_by_label_contains("Quick construction: 8 path(s).");
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
}

// ----- PH-4b : perçages au pas le long d'un bord -----------------------------------------

/// `marks_harness` avec le bord bas du rectangle (0 ; 0) → (100 ; 0) sélectionné.
fn pitch_harness() -> (Harness<'static, App>, pf_core::PieceId, pf_core::EntityId) {
    let (mut h, _) = marks_harness();
    let piece = piece_of(&h).id;
    let bottom = edge_at(&h, piece, pt(0.0, 0.0), pt(100.0, 0.0));
    h.state_mut().editor.selection.entities = vec![bottom];
    h.run_steps(3);
    (h, piece, bottom)
}

#[test]
fn ph4b_bloc_replie_par_defaut_apercu_puis_creation_en_une_etape() {
    let (mut h, piece, _) = pitch_harness();
    // Replié par défaut : le titre est là, aucun champ du bloc.
    h.get_by_label("Perçages au pas…");
    assert!(h.query_by_label("Créer la rangée").is_none());
    assert!(h.query_by_label("Aperçu").is_none());
    open_fold(&mut h, "piece.marking.run.title");
    h.get_by_label_contains("Bord : 1 tracé(s) du contour.");
    let before = h.state().editor.project().clone();
    // Aperçu : pas 4 sur 100 mm = 26 perçages à 4 mm, rien n'est modifié.
    h.get_by_label("Aperçu").click();
    h.run_steps(3);
    assert_eq!(
        *h.state().editor.project(),
        before,
        "l'aperçu ne modifie rien"
    );
    h.get_by_label_contains(
        "26 perçage(s), espacement réel 4,0 mm (pas demandé 4 mm), bord de 100,0 mm.",
    );
    h.get_by_label_contains("1. centre x 0,0 mm, y 3,0 mm");
    let preview = h
        .state()
        .pitch_form
        .as_ref()
        .unwrap()
        .preview
        .clone()
        .unwrap();
    assert_eq!(preview.count, 26);
    // Création : les positions sont exactement celles de l'aperçu, en une seule étape.
    h.get_by_label("Créer la rangée").click();
    h.run_steps(3);
    assert!(
        !h.state().editor.status.error,
        "{}",
        h.state().editor.status.text
    );
    let positions: Vec<(f64, f64)> = h
        .state()
        .editor
        .project()
        .piece_markings(piece)
        .iter()
        .map(|r| (r.at.x, r.at.y))
        .collect();
    assert_eq!(positions, preview.positions);
    assert_eq!(positions.len(), 26);
    let layer = h
        .state()
        .editor
        .sheet()
        .layers
        .iter()
        .find(|l| l.name == pf_core::MARKING_RUN_POINTS_LAYER_NAME)
        .unwrap();
    assert!(!layer.visible, "points d'ancrage dans un calque masqué");
    h.get_by_label_contains("26 perçage(s) posé(s) en une étape.");
    // Une annulation suffit : le projet est celui d'avant (octets identiques).
    undo(&mut h);
    assert_eq!(*h.state().editor.project(), before);
    assert_eq!(
        pf_core::persist::to_json(h.state().editor.project()).unwrap(),
        pf_core::persist::to_json(&before).unwrap()
    );
}

#[test]
fn ph4b_le_nombre_la_fente_et_le_cote_exterieur() {
    let (mut h, piece, _) = pitch_harness();
    open_fold(&mut h, "piece.marking.run.title");
    h.get_by_label("Au nombre").click();
    h.run_steps(3);
    replace_field(&mut h, "10", "5");
    h.get_by_label("Fente de poinçon").click();
    h.run_steps(3);
    h.get_by_label("Côté extérieur").click();
    h.run_steps(3);
    h.get_by_label("Créer la rangée").click();
    h.run_steps(3);
    assert!(
        !h.state().editor.status.error,
        "{}",
        h.state().editor.status.text
    );
    let marks = h.state().editor.project().piece_markings(piece);
    assert_eq!(marks.len(), 5);
    for (i, m) in marks.iter().enumerate() {
        assert!((m.at.x - 25.0 * i as f64).abs() < 1e-9 && (m.at.y + 3.0).abs() < 1e-9);
        assert_eq!(
            m.marking.shape,
            pf_core::MarkingShape::Slot {
                length_mm: 4.0,
                width_mm: 1.0,
                angle_deg: 0.0
            }
        );
    }
    // Hors de la pièce : avertissement, jamais un refus.
    assert!(h
        .state()
        .editor
        .project()
        .piece_issues(piece)
        .contains(&pf_core::piece::PieceIssue::MarkingOutsideOutline { count: 5 }));
}

#[test]
fn ph4b_erreur_du_coeur_affichee_sans_effet_et_champ_illisible() {
    let (mut h, _, _) = pitch_harness();
    open_fold(&mut h, "piece.marking.run.title");
    let before = h.state().editor.project().clone();
    // Distance négative : refusée par le cœur, message affiché, projet inchangé.
    replace_field(&mut h, "3", "-5");
    h.get_by_label("Créer la rangée").click();
    h.run_steps(3);
    assert!(h.state().editor.status.error);
    assert!(
        h.state().editor.status.text.contains("distance"),
        "{}",
        h.state().editor.status.text
    );
    assert_eq!(*h.state().editor.project(), before);
    // L'aperçu refuse de même.
    h.get_by_label("Aperçu").click();
    h.run_steps(3);
    assert!(h.state().editor.status.error);
    assert!(h.state().pitch_form.as_ref().unwrap().preview.is_none());
    // Champ qui n'est pas un nombre.
    replace_field(&mut h, "-5", "abc");
    h.get_by_label("Créer la rangée").click();
    h.run_steps(3);
    assert!(h.state().editor.status.error);
    assert_eq!(*h.state().editor.project(), before);
    // Sans tracé du contour sélectionné : boutons grisés.
    h.state_mut().editor.selection.clear();
    h.run_steps(3);
    assert!(h
        .get_by_label("Créer la rangée")
        .accesskit_node()
        .is_disabled());
    assert_eq!(*h.state().editor.project(), before);
}

#[test]
fn ph4b_percages_au_pas_en_anglais() {
    pf_i18n::set_lang(pf_i18n::Lang::En);
    let (mut h, piece, _) = pitch_harness();
    h.get_by_label("Pitched holes…");
    assert!(h.query_by_label("Create row").is_none());
    open_fold(&mut h, "piece.marking.run.title");
    h.get_by_label_contains("Edge: 1 outline path(s).");
    h.get_by_label("By pitch");
    h.get_by_label("By count");
    h.get_by_label("Round hole");
    h.get_by_label("Punch slot");
    h.get_by_label("Inside");
    h.get_by_label("Outside");
    h.get_by_label("Requested pitch (mm)");
    for label in [
        "Distance from edge (mm)",
        "Start margin (mm)",
        "End margin (mm)",
    ] {
        assert!(h.get_all_by_label(label).next().is_some(), "{label}");
    }
    h.get_by_label("Preview").click();
    h.run_steps(3);
    h.get_by_label_contains(
        "26 hole(s), actual spacing 4.0 mm (requested pitch 4 mm), edge of 100.0 mm.",
    );
    h.get_by_label("Create row").click();
    h.run_steps(3);
    h.get_by_label_contains("26 hole(s) placed in one step.");
    assert_eq!(h.state().editor.project().piece_markings(piece).len(), 26);
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
}

// ===== US-03.1a : fenêtre « Variables » ==========================================================

/// Champs de saisie de la fenêtre « Variables », du haut vers le bas puis de gauche à droite.
fn var_type(h: &mut Harness<'_, App>, index: usize, text: &str) {
    {
        let dock = h.state().dock.rect;
        let mut fields: Vec<_> = h
            .get_all_by_role(egui::accesskit::Role::TextInput)
            .filter(|f| dock.contains(f.rect().center()))
            .collect();
        fields.sort_by(|a, b| {
            a.rect()
                .min
                .y
                .total_cmp(&b.rect().min.y)
                .then(a.rect().min.x.total_cmp(&b.rect().min.x))
        });
        fields[index].focus();
    }
    h.run_steps(2);
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.run_steps(1);
    type_text(h, text);
    key(h, Key::Enter);
    h.run_steps(3);
}

fn variables_exprs(h: &Harness<'_, App>) -> Vec<(String, String)> {
    h.state()
        .editor
        .project()
        .variables
        .iter()
        .map(|v| (v.name.clone(), v.expr.clone()))
        .collect()
}

#[test]
fn us031a_variables_fr_ajout_erreur_localisee_rien_modifie_et_virgule() {
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
    let mut h = harness();
    h.state_mut().open_variables();
    h.run_steps(4);
    // La fenêtre s'ouvre dans le dock : réservé, hors du canevas, sans le recouvrir.
    assert!(h
        .state()
        .dock
        .open()
        .contains(&crate::dock::ToolWin::Variables));
    let win = h
        .ctx
        .memory(|m| m.area_rect(crate::dock::ToolWin::Variables.id()))
        .expect("fenêtre Variables");
    let dock = h.state().dock.rect;
    assert!(dock.contains_rect(win), "{win:?} hors du dock {dock:?}");
    assert!(
        !win.intersects(h.state().canvas_rect),
        "{win:?} recouvre le canevas {:?}",
        h.state().canvas_rect
    );
    h.get_by_label("Aucune variable.");
    h.get_by_label_contains("Une variable se cite dans la formule d'un point en direction");
    // Ajout : nom, puis expression avec virgule décimale ; Entrée ajoute.
    var_type(&mut h, 0, "tour");
    var_type(&mut h, 1, "92,5/2");
    assert_eq!(
        variables_exprs(&h),
        vec![("tour".to_string(), "92,5/2".to_string())]
    );
    // Valeur calculée affichée avec la virgule de la langue.
    h.get_by_label("46,25 mm");
    // Ajout invalide : division par zéro, erreur localisée sous la ligne, rien n'est modifié.
    let before = h.state().editor.project().clone();
    var_type(&mut h, 2, "x");
    var_type(&mut h, 3, "tour/0");
    h.get_by_label("Division par zéro (caractère 5).");
    assert_eq!(*h.state().editor.project(), before);
    // Modification invalide d'une ligne existante : erreur de syntaxe à sa position.
    var_type(&mut h, 1, "2+");
    h.get_by_label("Erreur de syntaxe au caractère 3.");
    assert_eq!(*h.state().editor.project(), before);
    h.get_by_label("46,25 mm"); // la valeur affichée n'a pas bougé
                                // Nom refusé (accent), cycle, référence inconnue : rien n'est modifié.
    var_type(&mut h, 0, "é");
    h.get_by_label_contains("refusé : caractère « é » non permis");
    assert_eq!(*h.state().editor.project(), before);
    var_type(&mut h, 1, "tour+1");
    h.get_by_label("Dépendance circulaire : tour → tour.");
    assert_eq!(*h.state().editor.project(), before);
    var_type(&mut h, 1, "inconnue*2");
    h.get_by_label("Variable « inconnue » inconnue (caractère 1).");
    assert_eq!(*h.state().editor.project(), before);
    // Saisie valide : la valeur suit, une seule étape d'annulation.
    var_type(&mut h, 1, "10/4");
    assert_eq!(variables_exprs(&h)[0].1, "10/4");
    h.get_by_label("2,5 mm");
    // L'erreur de la ligne a disparu ; celle de la ligne d'ajout (champs inchangés) reste.
    assert!(h
        .query_by_label("Variable « inconnue » inconnue (caractère 1).")
        .is_none());
    h.get_by_label("Division par zéro (caractère 5).");
    undo(&mut h);
    assert_eq!(variables_exprs(&h)[0].1, "92,5/2");
    // Aucune mention d'un calcul maintenu ni d'une longueur imposée.
    for banned in ["maintenu", "imposée"] {
        assert!(h.query_by_label_contains(banned).is_none(), "{banned}");
    }
}

#[test]
fn us031a_variables_en_ajout_erreur_localisee_rien_modifie_et_point() {
    pf_i18n::set_lang(pf_i18n::Lang::En);
    let mut h = harness();
    h.state_mut().open_variables();
    h.run_steps(4);
    h.get_by_label("No variables yet.");
    h.get_by_label_contains("A variable can be cited in the formula of a direction point");
    var_type(&mut h, 0, "bust");
    var_type(&mut h, 1, "92,5/2");
    h.get_by_label("46.25 mm");
    let before = h.state().editor.project().clone();
    var_type(&mut h, 2, "y");
    var_type(&mut h, 3, "bust/0");
    h.get_by_label("Division by zero (character 5).");
    assert_eq!(*h.state().editor.project(), before);
    var_type(&mut h, 1, "sqrt(0-4)");
    h.get_by_label("Value out of range or too large (character 1).");
    assert_eq!(*h.state().editor.project(), before);
    var_type(&mut h, 1, "(1+2");
    h.get_by_label("Syntax error at character 1.");
    assert_eq!(*h.state().editor.project(), before);
    var_type(&mut h, 1, "min(3;1;2)");
    h.get_by_label("1.0 mm");
    // Suppression : refusée tant qu'une autre variable s'y réfère, listant les dépendants.
    var_type(&mut h, 2, "half");
    var_type(&mut h, 3, "bust/2");
    assert_eq!(h.state().editor.project().variables.len(), 2);
    let before = h.state().editor.project().clone();
    h.get_all_by_label("×")
        .next()
        .expect("bouton de suppression")
        .click();
    h.run_steps(3);
    h.get_by_label("\"bust\" is used by: half. Change them first.");
    assert_eq!(*h.state().editor.project(), before);
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
}

#[test]
fn us031a_echap_ferme_la_fenetre_sans_toucher_au_projet() {
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
    let mut h = harness();
    let before = h.state().editor.project().clone();
    h.state_mut().open_variables();
    h.run_steps(4);
    assert!(h.state().variables.is_some());
    key(&mut h, Key::Escape);
    h.run_steps(3);
    assert!(h.state().variables.is_none());
    assert_eq!(*h.state().editor.project(), before);
}

// ===== US-03.1b : valeurs maintenues par une formule =============================================

/// Champs de formule de l'inspecteur (colonne de droite), du haut vers le bas ; le premier champ
/// de l'inspecteur est l'étiquette du point, il n'est pas compté. Remplace le contenu et valide.
fn formula_type(h: &mut Harness<'_, App>, index: usize, text: &str) {
    {
        let right = h.state().canvas_rect.max.x;
        let mut fields: Vec<_> = h
            .get_all_by_role(egui::accesskit::Role::TextInput)
            .filter(|f| f.rect().min.x > right)
            .collect();
        fields.sort_by(|a, b| a.rect().min.y.total_cmp(&b.rect().min.y));
        fields[index + 1].focus();
    }
    h.run_steps(2);
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.run_steps(1);
    type_text(h, text);
    key(h, Key::Enter);
    h.run_steps(3);
}

/// Feuille avec les variables `L = 120` (mm) et `a = 30` (°), `A` (0 ; 0), `B` = A + (1 ; 0)
/// (point décalé) et `C` en direction de `A` (50 mm à 30°) ; le point `which` est sélectionné.
fn formula_harness(which: usize) -> (Harness<'static, App>, [pf_core::PointId; 3]) {
    let mut h = harness();
    let ids = {
        let ed = &mut h.state_mut().editor;
        ed.grid_snap_mm = None;
        let sheet = ed.sheet;
        for (name, unit, expr) in [
            ("L", pf_core::VariableUnit::Mm, "120"),
            ("a", pf_core::VariableUnit::Deg, "30"),
        ] {
            ed.exec(pf_core::Command::AddVariable {
                name: name.into(),
                unit,
                expr: expr.into(),
            })
            .unwrap();
        }
        let a = ed
            .exec(pf_core::Command::AddPoint {
                sheet,
                at: pt(0.0, 0.0),
            })
            .unwrap()
            .points[0];
        let mut derived = |definition| {
            ed.exec(pf_core::Command::AddPointAt {
                sheet,
                point: pf_core::PointRef::Derived(definition),
            })
            .unwrap()
            .points[0]
        };
        let b = derived(pf_core::PointDefinition::Offset {
            from: a,
            dx_mm: 1.0,
            dy_mm: 0.0,
        });
        let c = derived(pf_core::PointDefinition::Polar {
            from: a,
            distance_mm: 50.0,
            angle_deg: 30.0,
            reference: None,
        });
        ed.selection.clear();
        [a, b, c]
    };
    h.run_steps(3);
    select_points(&mut h, &[ids[which]]);
    (h, ids)
}

fn param_value(h: &Harness<'_, App>, point: pf_core::PointId, param: pf_core::ParamKind) -> f64 {
    let ed = &h.state().editor;
    ed.sheet().parameter_value(point, param).unwrap()
}

fn has_formula(h: &Harness<'_, App>, point: pf_core::PointId, param: pf_core::ParamKind) -> bool {
    let ed = &h.state().editor;
    ed.sheet().formula(point, param).is_some()
}

#[test]
fn us031b_formule_fr_saisie_maintenu_variable_erreur_localisee_delier() {
    use pf_core::ParamKind::{Dx, Dy};
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
    let (mut h, [_a, b, _c]) = formula_harness(1);
    // Rien de lié : le champ propose la saisie d'une formule, aucun « maintenu ».
    h.get_by_label("Formule · dx");
    h.get_by_label("Formule · dy");
    assert!(h.query_by_label("ƒ Maintenu").is_none());
    // Formule saisie avec une virgule décimale : 120 / 2 + 10 = 70 (point en (70 ; 0)).
    formula_type(&mut h, 0, "L/2+10");
    assert!(
        !h.state().editor.status.error,
        "{}",
        h.state().editor.status.text
    );
    assert!(has_formula(&h, b, Dx));
    assert_eq!(param_value(&h, b, Dx), 70.0);
    assert_eq!(h.state().editor.sheet().position(b).unwrap(), pt(70.0, 0.0));
    // Affichage visible : icône + texte « Maintenu », formule à côté de la valeur calculée.
    h.get_by_label("ƒ Maintenu");
    h.get_by_label("= L/2+10");
    h.get_by_label("70,0 mm");
    h.get_by_label("Délier");
    // Changer la variable déplace le point : 200 / 2 + 10 = 110, dans la même étape.
    let before = h.state().editor.project().clone();
    h.state_mut()
        .editor
        .exec(pf_core::Command::SetVariableExpr {
            name: "L".into(),
            expr: "200".into(),
        })
        .unwrap();
    h.run_steps(3);
    assert_eq!(param_value(&h, b, Dx), 110.0);
    assert_eq!(
        h.state().editor.sheet().position(b).unwrap(),
        pt(110.0, 0.0)
    );
    h.get_by_label("110,0 mm");
    undo(&mut h);
    assert_eq!(*h.state().editor.project(), before, "une seule étape");
    assert_eq!(h.state().editor.sheet().position(b).unwrap(), pt(70.0, 0.0));
    // Erreurs d'expression : message localisé (catégorie + position), rien n'est modifié.
    let before = h.state().editor.project().clone();
    formula_type(&mut h, 1, "L/0");
    h.get_by_label("Division par zéro (caractère 2).");
    assert_eq!(*h.state().editor.project(), before);
    formula_type(&mut h, 1, "2+");
    h.get_by_label("Erreur de syntaxe au caractère 3.");
    assert_eq!(*h.state().editor.project(), before);
    formula_type(&mut h, 1, "zz*2");
    h.get_by_label("Variable « zz » inconnue (caractère 1).");
    assert_eq!(*h.state().editor.project(), before);
    assert!(!has_formula(&h, b, Dy));
    // Délier garde la valeur courante en nombre figé.
    h.get_by_label("Délier").click();
    h.run_steps(3);
    assert!(!has_formula(&h, b, Dx));
    assert_eq!(param_value(&h, b, Dx), 70.0);
    assert!(h.query_by_label("ƒ Maintenu").is_none());
    undo(&mut h);
    assert!(has_formula(&h, b, Dx));
    // Un nombre simple saisi dans le champ d'un paramètre lié le fige (formule déliée).
    formula_type(&mut h, 0, "33");
    assert!(!has_formula(&h, b, Dx));
    assert_eq!(param_value(&h, b, Dx), 33.0);
    // Jamais « imposée » ni « contrainte » pour ce lien.
    for banned in ["imposée", "contrainte"] {
        assert!(h.query_by_label_contains(banned).is_none(), "{banned}");
    }
}

#[test]
fn us031b_formule_en_direction_angle_distance_et_point_decimal() {
    use pf_core::ParamKind::{Angle, Distance};
    pf_i18n::set_lang(pf_i18n::Lang::En);
    let (mut h, [_a, _b, c]) = formula_harness(2);
    h.get_by_label("Formula · Distance");
    h.get_by_label("Formula · Angle");
    // Refus du cœur (distance nulle : L - 120 = 0) : affiché sous le champ, rien n'est modifié.
    let before = h.state().editor.project().clone();
    formula_type(&mut h, 0, "L-120");
    assert!(h.state().editor.status.error);
    assert_eq!(*h.state().editor.project(), before);
    assert!(h.state().formula_error.is_some());
    // Distance min(L ; 80) / 2 = 40 ; angle 90 - a = 60°.
    formula_type(&mut h, 0, "min(L;80)/2");
    formula_type(&mut h, 1, "90-a");
    assert_eq!(param_value(&h, c, Distance), 40.0);
    assert_eq!(param_value(&h, c, Angle), 60.0);
    assert_eq!(h.get_all_by_label("ƒ Maintained").count(), 2);
    h.get_by_label("= min(L;80)/2");
    h.get_by_label("= 90-a");
    h.get_by_label("40.0 mm");
    h.get_by_label("60.0 °");
    let q = h.state().editor.sheet().position(c).unwrap();
    assert!((q.x - 20.0).abs() < 1e-9 && (q.y - 20.0 * 3f64.sqrt()).abs() < 1e-9);
    // a = 45 : le point se déplace à 45°, distance inchangée.
    h.state_mut()
        .editor
        .exec(pf_core::Command::SetVariableExpr {
            name: "a".into(),
            expr: "45".into(),
        })
        .unwrap();
    h.run_steps(3);
    assert_eq!(param_value(&h, c, Angle), 45.0);
    h.get_by_label("45.0 °");
    // Une variable devenue 0 qui rendrait la distance nulle est refusée : rien ne bouge.
    let before = h.state().editor.project().clone();
    let err = h
        .state_mut()
        .editor
        .exec(pf_core::Command::SetVariableExpr {
            name: "L".into(),
            expr: "0".into(),
        });
    assert!(err.is_none());
    assert_eq!(*h.state().editor.project(), before);
    assert!(h.state().editor.status.error);
    // Erreurs localisées en anglais ; rien n'est modifié.
    formula_type(&mut h, 0, "sqrt(0-4)");
    h.get_by_label("Value out of range or too large (character 1).");
    formula_type(&mut h, 0, "(1+2");
    h.get_by_label("Syntax error at character 1.");
    assert_eq!(*h.state().editor.project(), before);
    // Point décimal en anglais ; délier les deux garde 40 mm et 45°.
    formula_type(&mut h, 0, "12.5");
    assert_eq!(param_value(&h, c, Distance), 12.5);
    assert!(!has_formula(&h, c, Distance));
    h.get_by_label("Unlink").click();
    h.run_steps(3);
    assert!(!has_formula(&h, c, Angle));
    assert_eq!(param_value(&h, c, Angle), 45.0);
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
}

/// US-03.2 : A (0 ; 0), segment de 100 mm à 30° vers B, choisi dans l'inspecteur.
fn kept_harness() -> (
    Harness<'static, App>,
    pf_core::PointId,
    pf_core::PointId,
    pf_core::EntityId,
) {
    let mut h = harness();
    let (a, b, seg) = {
        let ed = &mut h.state_mut().editor;
        ed.grid_snap_mm = None;
        let sheet = ed.sheet;
        let a = ed
            .exec(pf_core::Command::AddPoint {
                sheet,
                at: pt(0.0, 0.0),
            })
            .unwrap()
            .points[0];
        let o = ed
            .exec(pf_core::Command::AddSegmentPolar {
                sheet,
                from: pf_core::PointRef::Existing(a),
                length_mm: 100.0,
                angle_deg: 30.0,
            })
            .unwrap();
        ed.selection.clear();
        ed.selection.entities.push(o.entities[0]);
        (a, o.points[0], o.entities[0])
    };
    h.run_steps(3);
    (h, a, b, seg)
}

fn kept_pos(h: &Harness<'_, App>, p: pf_core::PointId) -> PointMm {
    h.state().editor.sheet().position(p).unwrap()
}

fn kept_flag(h: &Harness<'_, App>, seg: pf_core::EntityId) -> bool {
    h.state().editor.sheet().is_kept_segment(seg)
}

/// Même parcours en français et en anglais : `(non maintenu, maintenu, case, délier)`.
fn kept_walk(
    h: &mut Harness<'_, App>,
    a: pf_core::PointId,
    b: pf_core::PointId,
    seg: pf_core::EntityId,
    labels: (&str, &str, &str, &str),
) {
    let (off, on, checkbox, unlink) = labels;
    let bx = 100.0 * 3f64.sqrt() / 2.0;
    // Non maintenu : texte explicite, case décochée, projet inchangé par l'affichage.
    h.get_by_label(off);
    h.get_by_label(checkbox);
    assert!(h.query_by_label(on).is_none());
    assert!(!kept_flag(h, seg));
    // Cocher : une seule commande, B ne bouge pas, l'indicateur apparaît (texte + icône dessinée).
    h.get_by_label(checkbox).click();
    h.run_steps(3);
    assert!(kept_flag(h, seg));
    h.get_by_label(on);
    h.get_by_label(unlink);
    assert!(h.query_by_label(off).is_none());
    let p = kept_pos(h, b);
    assert!((p.x - bx).abs() < 1e-9 && (p.y - 50.0).abs() < 1e-9);
    // Déplacer le point de départ déplace B dans la même étape : A (10 ; 20) -> B (96,6 ; 70).
    let before = h.state().editor.project().clone();
    let sheet = h.state().editor.sheet;
    h.state_mut()
        .editor
        .exec(pf_core::Command::MovePoint {
            sheet,
            point: a,
            to: pt(10.0, 20.0),
        })
        .unwrap();
    h.run_steps(3);
    let p = kept_pos(h, b);
    assert!((p.x - (10.0 + bx)).abs() < 1e-9 && (p.y - 70.0).abs() < 1e-9);
    undo(h);
    assert_eq!(*h.state().editor.project(), before, "une seule étape");
    // Délier garde les positions ; l'état redevient « non maintenu ».
    h.get_by_label(unlink).click();
    h.run_steps(3);
    assert!(!kept_flag(h, seg));
    h.get_by_label(off);
    assert!(h.query_by_label(on).is_none());
    let q = kept_pos(h, b);
    assert!((q.x - bx).abs() < 1e-9 && (q.y - 50.0).abs() < 1e-9);
    // Jamais « imposée » ni « contrainte » pour cette fonction.
    for banned in ["imposée", "contrainte", "imposed", "constraint"] {
        assert!(h.query_by_label_contains(banned).is_none(), "{banned}");
    }
}

#[test]
fn us032_construction_maintenue_fr_case_indicateur_delier() {
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
    let (mut h, a, b, seg) = kept_harness();
    kept_walk(
        &mut h,
        a,
        b,
        seg,
        (
            "Construction non maintenue",
            "Construction maintenue",
            "Conserver la construction",
            "Délier",
        ),
    );
    // Un déplacement direct du point suivi délie avec remarque (jamais de lien perdu sans trace).
    h.get_by_label("Conserver la construction").click();
    h.run_steps(3);
    assert!(kept_flag(&h, seg));
    let sheet = h.state().editor.sheet;
    h.state_mut()
        .editor
        .exec(pf_core::Command::MovePoint {
            sheet,
            point: b,
            to: pt(70.0, 80.0),
        })
        .unwrap();
    h.run_steps(3);
    assert!(!kept_flag(&h, seg));
    assert!(h.state().editor.status.text.contains("n'est plus maintenu"));
}

#[test]
fn us032_construction_maintenue_en_case_indicateur_delier() {
    pf_i18n::set_lang(pf_i18n::Lang::En);
    let (mut h, a, b, seg) = kept_harness();
    kept_walk(
        &mut h,
        a,
        b,
        seg,
        (
            "Construction not maintained",
            "Maintained construction",
            "Keep the construction",
            "Unlink",
        ),
    );
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
}

// ---------------------------------------------------------------------------------------------
// US-03.3 — longueur imposée (inspecteur d'une courbe de Bézier)
// ---------------------------------------------------------------------------------------------

/// Courbe de Bézier A (0 ; 0) → B (100 ; 0) sans poignée (longueur 100 mm), choisie, dans une
/// fenêtre assez haute pour que la section dépliée tienne.
fn imposed_harness() -> (
    Harness<'static, App>,
    pf_core::PointId,
    pf_core::PointId,
    pf_core::EntityId,
) {
    let app = App::with_context(&egui::Context::default(), Launch::default());
    let mut h = Harness::builder()
        .with_size(egui::vec2(1280.0, 1700.0))
        .build_state(|ctx, app: &mut App| app.ui(ctx), app);
    h.run();
    let (a, b, curve) = {
        let ed = &mut h.state_mut().editor;
        ed.grid_snap_mm = None;
        let sheet = ed.sheet;
        let o = ed
            .exec(pf_core::Command::AddCurve {
                sheet,
                start: pf_core::PointRef::New(pt(0.0, 0.0)),
                c1: pt(0.0, 0.0),
                c2: pt(100.0, 0.0),
                end: pf_core::PointRef::New(pt(100.0, 0.0)),
            })
            .unwrap();
        ed.selection.clear();
        ed.selection.entities.push(o.entities[0]);
        (o.points[0], o.points[1], o.entities[0])
    };
    h.run_steps(3);
    (h, a, b, curve)
}

fn imposed_of(h: &Harness<'_, App>, curve: pf_core::EntityId) -> Option<pf_core::CurveLength> {
    h.state().editor.sheet().length_constraint(curve).cloned()
}

/// Ordonnées des champs de texte de l'inspecteur situés sous l'en-tête de la section.
fn imposed_fields(h: &Harness<'_, App>, below_y: f32) -> Vec<f32> {
    let right = h.state().canvas_rect.max.x;
    let mut ys: Vec<f32> = h
        .get_all_by_role(egui::accesskit::Role::TextInput)
        .filter(|f| f.rect().min.x > right && f.rect().min.y > below_y)
        .map(|f| f.rect().min.y)
        .collect();
    ys.sort_by(|a, b| a.total_cmp(b));
    ys
}

fn imposed_type(h: &mut Harness<'_, App>, below_y: f32, index: usize, text: &str) {
    let y = imposed_fields(h, below_y)[index];
    let right = h.state().canvas_rect.max.x;
    h.get_all_by_role(egui::accesskit::Role::TextInput)
        .find(|f| f.rect().min.x > right && (f.rect().min.y - y).abs() < 0.5)
        .expect("champ")
        .focus();
    h.run_steps(2);
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.run_steps(1);
    type_text(h, text);
    key(h, Key::Enter);
    h.run_steps(3);
}

/// Même parcours en français et en anglais :
/// `(titre, actif, case, hors-contrainte, résolue, refus, désactiver)`.
fn imposed_walk(labels: (&str, &str, &str, &str, &str, &str, &str)) {
    let (title, active, checkbox, off_state, solved, refused, disable) = labels;
    let (mut h, a, b, curve) = imposed_harness();
    // Section repliée par défaut : seul l'en-tête est visible, rien n'est imposé.
    let header_y = h.get_by_label(title).rect().min.y;
    assert!(h.query_by_label(checkbox).is_none(), "section dépliée");
    assert!(imposed_of(&h, curve).is_none());
    h.get_by_label(title).click_accesskit();
    h.run_steps(3);
    h.get_by_label(off_state);
    h.get_by_label(checkbox);
    assert!(h.query_by_label(solved).is_none());
    // Cocher : la longueur actuelle (100 mm) devient la cible, B glisse le long de 0°.
    h.get_by_label(checkbox).click_accesskit();
    h.run_steps(3);
    let c = imposed_of(&h, curve).expect("contrainte posée");
    assert_eq!(c.length_mm, 100.0);
    assert!(matches!(
        c.adjust,
        pf_core::LengthAdjust::Slide { point, angle_deg } if point == b && angle_deg == 0.0
    ));
    h.get_by_label(solved);
    h.get_by_label(&format!("{title} · {active}"));
    assert!(h.query_by_label(off_state).is_none());
    // Cible 250 mm : B passe en (250 ; 0), état « résolue » (icône dessinée + texte).
    imposed_type(&mut h, header_y, 0, "250");
    assert_eq!(imposed_of(&h, curve).unwrap().length_mm, 250.0);
    let q = h.state().editor.sheet().position(b).unwrap();
    assert!((q.x - 250.0).abs() < 1e-6 && q.y.abs() < 1e-9, "{q:?}");
    h.get_by_label(solved);
    // Parent déplacé : la contrainte se re-résout dans la même étape annulable.
    let before = h.state().editor.project().clone();
    let sheet_id = h.state().editor.sheet;
    h.state_mut()
        .editor
        .exec(pf_core::Command::MovePoint {
            sheet: sheet_id,
            point: a,
            to: pt(0.0, 30.0),
        })
        .unwrap();
    h.run_steps(3);
    let q = h.state().editor.sheet().position(b).unwrap();
    assert!((q.x - 61600f64.sqrt()).abs() < 1e-6, "{q:?}");
    undo(&mut h);
    assert_eq!(*h.state().editor.project(), before, "une seule étape");
    // Direction verticale puis cible hors plage (la plus courte courbe possible fait 250 mm) :
    // refus expliqué sous le champ, rien ne change.
    imposed_type(&mut h, header_y, 1, "90");
    assert!(imposed_of(&h, curve).is_some());
    let before = h.state().editor.project().clone();
    imposed_type(&mut h, header_y, 0, "100");
    assert_eq!(*h.state().editor.project(), before, "refus sans trace");
    h.get_by_label_contains(refused);
    // Désactiver : la géométrie actuelle reste, la mention disparaît.
    h.get_by_label(disable).click_accesskit();
    h.run_steps(3);
    assert!(imposed_of(&h, curve).is_none());
    let q = h.state().editor.sheet().position(b).unwrap();
    assert!((q.x - 250.0).abs() < 1e-6, "{q:?}");
    h.get_by_label(off_state);
    assert!(h.query_by_label(solved).is_none());
}

#[test]
fn us033_longueur_imposee_fr_case_cible_etat_refus_desactiver() {
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
    imposed_walk((
        "Longueur imposée",
        "active",
        "Imposer la longueur développée",
        "Longueur non imposée (mesure seule)",
        "Longueur imposée résolue",
        "Refusé, rien n'a changé",
        "Désactiver",
    ));
}

#[test]
fn us033_longueur_imposee_en_case_cible_etat_refus_desactiver() {
    pf_i18n::set_lang(pf_i18n::Lang::En);
    imposed_walk((
        "Imposed length",
        "active",
        "Impose the developed length",
        "Length not imposed (measure only)",
        "Imposed length solved",
        "Refused, nothing changed",
        "Disable",
    ));
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
}

/// US-03.4 — égalité de longueurs, même parcours en français et en anglais :
/// `(titre, case, mode « égale », choisir, tracé de référence, refusé, résolue, désactiver)`.
fn equality_walk(labels: (&str, &str, &str, &str, &str, &str, &str, &str)) {
    let (title, checkbox, mode_ref, pick, ref_prefix, refused, solved, disable) = labels;
    let (mut h, a, b, curve) = imposed_harness();
    // Un segment de référence de 80 mm sous la courbe, que la sélection ne quitte pas.
    let (r0, r1, seg) = {
        let sheet = h.state().editor.sheet;
        let o = h
            .state_mut()
            .editor
            .exec(pf_core::Command::AddSegment {
                sheet,
                from: pf_core::PointRef::New(pt(0.0, -50.0)),
                to: pf_core::PointRef::New(pt(80.0, -50.0)),
            })
            .unwrap();
        let ed = &mut h.state_mut().editor;
        ed.selection.clear();
        ed.selection.entities.push(curve);
        (o.points[0], o.points[1], o.entities[0])
    };
    let _ = (a, r0);
    h.run_steps(3);
    let header_y = h.get_by_label(title).rect().min.y;
    h.get_by_label(title).click_accesskit();
    h.run_steps(3);
    // Mode « égale à un autre tracé » : aucun tracé choisi, rien n'est imposé.
    h.get_by_label(mode_ref).click_accesskit();
    h.run_steps(3);
    assert!(imposed_of(&h, curve).is_none());
    // Choisir dans le plan : le clic suivant (sur le segment) est intercepté, la sélection reste.
    h.get_by_label(pick).click_accesskit();
    h.run_steps(3);
    assert_eq!(h.state().editor.length_pick.waiting, Some(curve));
    h.state_mut()
        .editor
        .click_with(pt(40.0, -50.0), 2.0, false, false);
    h.run_steps(3);
    assert!(h.state().editor.length_pick.waiting.is_none());
    assert_eq!(h.state().editor.selection.entities, vec![curve]);
    h.get_by_label_contains(ref_prefix);
    // Décalage +12 puis case : la courbe devient « segment + 12 » = 92 mm, B glisse le long de 0°.
    imposed_type(&mut h, header_y, 0, "12");
    assert!(
        imposed_of(&h, curve).is_none(),
        "le décalage seul ne pose rien"
    );
    h.get_by_label(checkbox).click_accesskit();
    h.run_steps(3);
    let c = imposed_of(&h, curve).expect("contrainte posée");
    assert_eq!(
        c.reference,
        Some(pf_core::LengthRef {
            entity: seg,
            offset_mm: 12.0
        })
    );
    assert!((c.length_mm - 92.0).abs() < 1e-9, "{c:?}");
    let q = h.state().editor.sheet().position(b).unwrap();
    assert!((q.x - 92.0).abs() < 1e-6 && q.y.abs() < 1e-9, "{q:?}");
    h.get_by_label(solved);
    // Le segment s'allonge (parent déplacé) : la courbe suit dans la même étape annulable.
    let before = h.state().editor.project().clone();
    let sheet_id = h.state().editor.sheet;
    h.state_mut()
        .editor
        .exec(pf_core::Command::MovePoint {
            sheet: sheet_id,
            point: r1,
            to: pt(120.0, -50.0),
        })
        .unwrap();
    h.run_steps(3);
    let q = h.state().editor.sheet().position(b).unwrap();
    assert!((q.x - 132.0).abs() < 1e-6, "{q:?}");
    h.get_by_label(solved);
    undo(&mut h);
    assert_eq!(*h.state().editor.project(), before, "une seule étape");
    // Décalage -90 : la cible serait négative (80 - 90) : refus expliqué, rien ne change.
    let before = h.state().editor.project().clone();
    imposed_type(&mut h, header_y, 0, "-90");
    assert_eq!(*h.state().editor.project(), before, "refus sans trace");
    h.get_by_label_contains(refused);
    // Désactiver : la géométrie reste, la référence disparaît.
    h.get_by_label(disable).click_accesskit();
    h.run_steps(3);
    assert!(imposed_of(&h, curve).is_none());
    let q = h.state().editor.sheet().position(b).unwrap();
    assert!((q.x - 92.0).abs() < 1e-6, "{q:?}");
}

#[test]
fn us034_egalite_de_longueurs_fr_trace_de_reference_decalage_refus_desactiver() {
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
    equality_walk((
        "Longueur imposée",
        "Imposer la longueur développée",
        "Égale à un autre tracé",
        "Choisir dans le plan",
        "Tracé de référence :",
        "Refusé, rien n'a changé",
        "Longueur imposée résolue",
        "Désactiver",
    ));
}

#[test]
fn us034_egalite_de_longueurs_en_reference_path_offset_refusal_disable() {
    pf_i18n::set_lang(pf_i18n::Lang::En);
    equality_walk((
        "Imposed length",
        "Impose the developed length",
        "Equal to another path",
        "Pick in the plan",
        "Reference path:",
        "Refused, nothing changed",
        "Imposed length solved",
        "Disable",
    ));
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
}

/// US-03.4b — somme de tracés dont un d'une autre feuille, décalage par formule, même parcours en
/// français et en anglais :
/// `(titre, case, mode « égale », liste des autres feuilles, ajouter, somme, refusé, résolue, désactiver)`.
fn equality_sum_walk(labels: (&str, &str, &str, &str, &str, &str, &str, &str, &str)) {
    let (title, checkbox, mode_ref, _other, add, sum, refused, solved, disable) = labels;
    let (mut h, _a, b, curve) = imposed_harness();
    // Feuille 1 : courbe (100 mm) et bande de 30 mm. Feuille « Corsage » : segment de 60 mm.
    let sheet1 = h.state().editor.sheet;
    let (band, far_sheet, far, far_label) = {
        let ed = &mut h.state_mut().editor;
        let band = ed
            .exec(pf_core::Command::AddSegment {
                sheet: sheet1,
                from: pf_core::PointRef::New(pt(0.0, -50.0)),
                to: pf_core::PointRef::New(pt(30.0, -50.0)),
            })
            .unwrap()
            .entities[0];
        let far_sheet = ed
            .exec(pf_core::Command::CreateSheet {
                name: "Corsage".into(),
            })
            .unwrap()
            .sheet
            .unwrap();
        let far = ed
            .exec(pf_core::Command::AddSegment {
                sheet: far_sheet,
                from: pf_core::PointRef::New(pt(0.0, 0.0)),
                to: pf_core::PointRef::New(pt(60.0, 0.0)),
            })
            .unwrap()
            .entities[0];
        ed.exec(pf_core::Command::AddVariable {
            name: "embu".into(),
            unit: pf_core::VariableUnit::Mm,
            expr: "5".into(),
        })
        .unwrap();
        let far_label = format!(
            "Corsage · {}",
            crate::text::entity_name(ed.project().sheet(far_sheet).unwrap(), far)
        );
        ed.set_sheet(sheet1);
        ed.selection.clear();
        ed.selection.entities.push(curve);
        (band, far_sheet, far, far_label)
    };
    h.run_steps(3);
    let header_y = h.get_by_label(title).rect().min.y;
    h.get_by_label(title).click_accesskit();
    h.run_steps(3);
    h.get_by_label(mode_ref).click_accesskit();
    h.run_steps(3);
    // Tracé d'une autre feuille, choisi dans la liste : rien n'est imposé tant que la case est vide.
    // La liste est la première boîte déroulante sous l'en-tête de la section.
    let right = h.state().canvas_rect.max.x;
    let combo = h
        .get_all_by_role(egui::accesskit::Role::ComboBox)
        .filter(|f| f.rect().min.x > right && f.rect().min.y > header_y)
        .min_by(|a, b| a.rect().min.y.total_cmp(&b.rect().min.y))
        .expect("liste des autres feuilles");
    combo.click_accesskit();
    h.run_steps(3);
    h.get_by_label(&far_label).click_accesskit();
    h.run_steps(3);
    assert!(imposed_of(&h, curve).is_none());
    // Deuxième tracé, de la feuille : clic dans le plan après « Ajouter un tracé ».
    h.get_by_label(add).click_accesskit();
    h.run_steps(3);
    h.state_mut()
        .editor
        .click_with(pt(15.0, -50.0), 2.0, false, false);
    h.run_steps(3);
    h.get_by_label_contains(sum);
    // Décalage par formule, puis la case : 60 + 30 + embu (5) = 95, B glisse le long de 0°.
    imposed_type(&mut h, header_y, 0, "embu");
    assert!(imposed_of(&h, curve).is_none());
    h.get_by_label(checkbox).click_accesskit();
    h.run_steps(3);
    let c = imposed_of(&h, curve).expect("contrainte posée");
    assert_eq!(
        c.terms(),
        vec![
            pf_core::LengthTerm {
                entity: far,
                sheet: Some(far_sheet)
            },
            pf_core::LengthTerm {
                entity: band,
                sheet: None
            }
        ]
    );
    assert_eq!(c.offset_expr(), Some("embu"));
    assert!((c.length_mm - 95.0).abs() < 1e-9, "{c:?}");
    assert!((h.state().editor.sheet().position(b).unwrap().x - 95.0).abs() < 1e-6);
    h.get_by_label(solved);
    // La variable change : la courbe suit dans la même étape annulable.
    let before = h.state().editor.project().clone();
    h.state_mut()
        .editor
        .exec(pf_core::Command::SetVariableExpr {
            name: "embu".into(),
            expr: "7".into(),
        })
        .unwrap();
    h.run_steps(3);
    assert!((h.state().editor.sheet().position(b).unwrap().x - 97.0).abs() < 1e-6);
    undo(&mut h);
    assert_eq!(*h.state().editor.project(), before, "une seule étape");
    // Retirer le tracé de l'autre feuille (premier « × ») : 30 + 5 = 35 mm.
    let crosses: Vec<_> = h.get_all_by_label("×").collect();
    assert_eq!(crosses.len(), 2);
    crosses[0].click_accesskit();
    h.run_steps(3);
    let c = imposed_of(&h, curve).expect("contrainte gardée");
    assert_eq!(c.terms().len(), 1);
    assert!((c.length_mm - 35.0).abs() < 1e-9, "{c:?}");
    // Décalage -90 : la cible serait négative (30 - 90) : refus expliqué, rien ne change.
    let before = h.state().editor.project().clone();
    imposed_type(&mut h, header_y, 0, "-90");
    assert_eq!(*h.state().editor.project(), before, "refus sans trace");
    h.get_by_label_contains(refused);
    // Désactiver : la géométrie reste, l'égalité disparaît.
    h.get_by_label(disable).click_accesskit();
    h.run_steps(3);
    assert!(imposed_of(&h, curve).is_none());
    assert!((h.state().editor.sheet().position(b).unwrap().x - 35.0).abs() < 1e-6);
}

#[test]
fn us034b_somme_de_tracés_autre_feuille_formule_fr() {
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
    equality_sum_walk((
        "Longueur imposée",
        "Imposer la longueur développée",
        "Égale à un autre tracé",
        "Ajouter un tracé d'une autre feuille",
        "Ajouter un tracé",
        "Somme de 2 tracés",
        "Refusé, rien n'a changé",
        "Longueur imposée résolue",
        "Désactiver",
    ));
}

#[test]
fn us034b_sum_of_paths_other_sheet_formula_en() {
    pf_i18n::set_lang(pf_i18n::Lang::En);
    equality_sum_walk((
        "Imposed length",
        "Impose the developed length",
        "Equal to another path",
        "Add a path from another sheet",
        "Add a path",
        "Sum of 2 paths",
        "Refused, nothing changed",
        "Imposed length solved",
        "Disable",
    ));
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
}

#[test]
fn us033_la_mention_n_existe_que_pour_les_courbes_de_bezier_et_lisses() {
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
    let (mut h, _a, _b, _curve) = imposed_harness();
    // Un segment, un cercle : jamais « imposée » dans leur inspecteur (la courbe lisse l'a depuis
    // US-03.3b, voir `us033b_*`).
    let sheet = h.state().editor.sheet;
    let (seg, _spline, circle) = {
        let ed = &mut h.state_mut().editor;
        let seg = ed
            .exec(pf_core::Command::AddSegment {
                sheet,
                from: pf_core::PointRef::New(pt(0.0, 50.0)),
                to: pf_core::PointRef::New(pt(80.0, 50.0)),
            })
            .unwrap()
            .entities[0];
        let spline = ed
            .exec(pf_core::Command::AddSpline {
                sheet,
                points: vec![
                    pf_core::PointRef::New(pt(0.0, 90.0)),
                    pf_core::PointRef::New(pt(40.0, 120.0)),
                    pf_core::PointRef::New(pt(90.0, 90.0)),
                ],
            })
            .unwrap()
            .entities[0];
        let circle = ed
            .exec(pf_core::Command::AddCircle {
                sheet,
                center: pf_core::PointRef::New(pt(200.0, 50.0)),
                radius_mm: 20.0,
            })
            .unwrap()
            .entities[0];
        (seg, spline, circle)
    };
    for id in [seg, circle] {
        {
            let ed = &mut h.state_mut().editor;
            ed.selection.clear();
            ed.selection.entities.push(id);
        }
        h.run_steps(3);
        assert!(
            h.query_by_label_contains("Longueur imposée").is_none(),
            "mention pour un tracé qui n'est pas une courbe de Bézier ou lisse"
        );
        assert!(h.query_by_label_contains("Imposer la longueur").is_none());
    }
}

// ---------------------------------------------------------------------------------------------
// US-03.3b — longueur imposée d'une courbe lisse et cible par formule
// ---------------------------------------------------------------------------------------------

/// Courbe lisse A (0 ; 0) → B (100 ; 0), variable `w` = 100, sélectionnée.
fn smooth_imposed_harness() -> (Harness<'static, App>, pf_core::PointId, pf_core::EntityId) {
    let app = App::with_context(&egui::Context::default(), Launch::default());
    let mut h = Harness::builder()
        .with_size(egui::vec2(1280.0, 1700.0))
        .build_state(|ctx, app: &mut App| app.ui(ctx), app);
    h.run();
    let (b, curve) = {
        let ed = &mut h.state_mut().editor;
        ed.grid_snap_mm = None;
        ed.exec(pf_core::Command::AddVariable {
            name: "w".into(),
            unit: pf_core::VariableUnit::Mm,
            expr: "100".into(),
        })
        .unwrap();
        let sheet = ed.sheet;
        let o = ed
            .exec(pf_core::Command::AddSpline {
                sheet,
                points: vec![
                    pf_core::PointRef::New(pt(0.0, 0.0)),
                    pf_core::PointRef::New(pt(100.0, 0.0)),
                ],
            })
            .unwrap();
        ed.selection.clear();
        ed.selection.entities.push(o.entities[0]);
        (o.points[1], o.entities[0])
    };
    h.run_steps(3);
    (h, b, curve)
}

/// `(titre, case, mode formule, note courbe lisse, libellé poignée, refus, désactiver, valeur)`.
fn smooth_imposed_walk(labels: (&str, &str, &str, &str, &str, &str, &str, &str)) {
    let (title, checkbox, mode_formula, smooth_note, handle, refused, disable, value_line) = labels;
    let (mut h, b, curve) = smooth_imposed_harness();
    let header_y = h.get_by_label(title).rect().min.y;
    h.get_by_label(title).click_accesskit();
    h.run_steps(3);
    // Courbe lisse : la note dit qu'il n'y a pas de poignée, et la ligne « Poignée de » n'existe pas.
    h.get_by_label_contains(smooth_note);
    assert!(
        h.query_by_label(handle).is_none(),
        "poignée proposée pour une courbe lisse"
    );
    // Cocher : la longueur actuelle (100 mm) devient la cible, B glisse le long de 0° (défaut).
    h.get_by_label(checkbox).click_accesskit();
    h.run_steps(3);
    let c = imposed_of(&h, curve).expect("contrainte posée");
    assert_eq!(c.length_mm, 100.0);
    assert!(matches!(
        c.adjust,
        pf_core::LengthAdjust::Slide { point, angle_deg } if point == b && angle_deg == 0.0
    ));
    // Mode « Formule » : rien n'est envoyé tant que le champ est vide.
    let before = h.state().editor.project().clone();
    h.get_by_label(mode_formula).click_accesskit();
    h.run_steps(3);
    assert_eq!(
        *h.state().editor.project(),
        before,
        "formule vide : rien n'est envoyé"
    );
    // Saisie de la formule : une seule étape, B en (150 ; 0).
    imposed_type(&mut h, header_y, 0, "w*3/2");
    let c = imposed_of(&h, curve).unwrap();
    assert_eq!(c.expr.as_deref(), Some("w*3/2"));
    assert!((c.length_mm - 150.0).abs() < 1e-9);
    let q = h.state().editor.sheet().position(b).unwrap();
    assert!((q.x - 150.0).abs() < 1e-6 && q.y.abs() < 1e-9, "{q:?}");
    h.get_by_label_contains(value_line);
    undo(&mut h);
    assert_eq!(
        *h.state().editor.project(),
        before,
        "formule et contrainte : une seule étape"
    );
    // Rétablir puis changer la variable : la courbe suit dans la même étape.
    h.key_press_modifiers(Modifiers::COMMAND, Key::Y);
    h.run_steps(2);
    let after_formula = h.state().editor.project().clone();
    h.state_mut()
        .editor
        .exec(pf_core::Command::SetVariableExpr {
            name: "w".into(),
            expr: "120".into(),
        })
        .unwrap();
    h.run_steps(3);
    let q = h.state().editor.sheet().position(b).unwrap();
    assert!((q.x - 180.0).abs() < 1e-6, "{q:?}");
    undo(&mut h);
    assert_eq!(*h.state().editor.project(), after_formula);
    // Formule refusée (valeur négative) : message sous le champ, rien ne change.
    imposed_type(&mut h, header_y, 0, "0-w");
    assert_eq!(
        *h.state().editor.project(),
        after_formula,
        "refus sans trace"
    );
    h.get_by_label_contains(refused);
    // Désactiver : la contrainte et sa formule disparaissent, la géométrie reste.
    h.get_by_label(disable).click_accesskit();
    h.run_steps(3);
    assert!(imposed_of(&h, curve).is_none());
    let q = h.state().editor.sheet().position(b).unwrap();
    assert!((q.x - 150.0).abs() < 1e-6, "{q:?}");
}

#[test]
fn us033b_courbe_lisse_fr_pas_de_poignee_formule_une_etape_refus_desactiver() {
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
    smooth_imposed_walk((
        "Longueur imposée",
        "Imposer la longueur développée",
        "Formule",
        "Courbe lisse : pas de poignée",
        "Poignée de",
        "Refusé, rien n'a changé",
        "Désactiver",
        "Valeur de la formule",
    ));
}

#[test]
fn us033b_courbe_lisse_en_no_handle_formula_one_step_refusal_disable() {
    pf_i18n::set_lang(pf_i18n::Lang::En);
    smooth_imposed_walk((
        "Imposed length",
        "Impose the developed length",
        "Formula",
        "Smooth curve: no handle",
        "Handle of",
        "Refused, nothing changed",
        "Disable",
        "Formula value",
    ));
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
}

// ---------------------------------------------------------------------------------------------
// US-03.5 — fenêtre « Diagnostic » des liens
// ---------------------------------------------------------------------------------------------

/// Libellés d'un parcours du diagnostic, en français et en anglais.
struct DiagLabels {
    view_menu: &'static str,
    menu_item: &'static str,
    empty: &'static str,
    counts: &'static str,
    info_group: &'static str,
    warning_group: &'static str,
    error_group: &'static str,
    formula: &'static str,
    kept: &'static str,
    imposed: &'static str,
    equality: &'static str,
    locked: &'static str,
    unused: &'static str,
    vocabulary_ban: &'static str,
}

fn diagnostic_harness() -> Harness<'static, App> {
    let app = App::with_context(&egui::Context::default(), Launch::default());
    let mut h = Harness::builder()
        .with_size(egui::vec2(1280.0, 900.0))
        .build_state(|ctx, app: &mut App| app.ui(ctx), app);
    h.run();
    h
}

fn load_project(h: &mut Harness<'_, App>, project: pf_core::Project) {
    h.state_mut().editor = crate::editor::Editor::new(pf_core::Document::new(project));
    h.run_steps(3);
}

/// Ordonnées des lignes dont le libellé contient `text`, de haut en bas.
fn diag_rows(h: &Harness<'_, App>, text: &str) -> Vec<f32> {
    let mut ys: Vec<f32> = h
        .query_all_by_label_contains(text)
        .map(|n| n.rect().min.y)
        .collect();
    ys.sort_by(|a, b| a.total_cmp(b));
    ys
}

fn diag_click(h: &mut Harness<'_, App>, text: &str, nth: usize) {
    let mut nodes: Vec<_> = h.get_all_by_label_contains(text).collect();
    nodes.sort_by(|a, b| a.rect().min.y.total_cmp(&b.rect().min.y));
    nodes[nth].click_accesskit();
    h.run_steps(3);
}

fn diagnostic_walk(l: DiagLabels) {
    let mut h = diagnostic_harness();
    // Ouverture par le menu Affichage (aucun raccourci, aucune section dépliée ailleurs).
    h.get_by_label(l.view_menu).click_accesskit();
    h.run_steps(3);
    h.get_by_label(l.menu_item).click_accesskit();
    h.run_steps(3);
    assert!(h.state().diagnostic.is_some(), "fenêtre non ouverte");
    // Projet sans aucun lien : état vide clair, pas un faux « tout est bon ».
    h.get_by_label(l.empty);
    assert!(h.query_by_label_contains(l.counts).is_none());
    // Projet de démonstration des quatre mécanismes : 4 informations, 3 avertissements, 0 erreur.
    let project = crate::stages::diagnostic_project().unwrap();
    let (seg, formula_point, imposed_curve, equality_curve) = {
        let d = project.link_diagnostics();
        let primary = |link, kind| {
            d.iter()
                .find(|x| x.link == link && x.kind == kind)
                .and_then(|x| x.primary().cloned())
                .unwrap()
        };
        let pf_core::DiagnosticObject::Entity(seg) = primary(
            pf_core::LinkType::KeptConstruction,
            pf_core::DiagnosticKind::Active,
        ) else {
            panic!("segment attendu")
        };
        let pf_core::DiagnosticObject::Point(fp) =
            primary(pf_core::LinkType::Formula, pf_core::DiagnosticKind::Active)
        else {
            panic!("point attendu")
        };
        let pf_core::DiagnosticObject::Entity(ic) = primary(
            pf_core::LinkType::ImposedLength,
            pf_core::DiagnosticKind::Active,
        ) else {
            panic!("courbe attendue")
        };
        let pf_core::DiagnosticObject::Entity(ec) = primary(
            pf_core::LinkType::LengthEquality,
            pf_core::DiagnosticKind::Active,
        ) else {
            panic!("courbe attendue")
        };
        (seg, fp, ic, ec)
    };
    let sheet = project.sheets[0].id;
    load_project(&mut h, project);
    h.get_by_label(l.counts);
    h.get_by_label(l.info_group);
    h.get_by_label(l.warning_group);
    assert!(h.query_by_label_contains(l.error_group).is_none());
    // Une ligne par lien et par avertissement, groupes visibles.
    for text in [l.formula, l.kept, l.imposed, l.equality, l.unused] {
        assert!(!diag_rows(&h, text).is_empty(), "ligne absente : {text}");
    }
    assert_eq!(
        diag_rows(&h, l.unused).len(),
        2,
        "deux variables inutilisées"
    );
    assert_eq!(diag_rows(&h, l.locked).len(), 1);
    // Jamais « contrainte non satisfaite » pour des liens maintenus ou des formules.
    assert!(h.query_by_label_contains(l.vocabulary_ban).is_none());
    // Le canevas garde au moins 400 points à 1280 px avec la fenêtre ouverte.
    assert!(
        h.state().canvas_rect.width() >= 400.0,
        "canevas : {}",
        h.state().canvas_rect.width()
    );
    // Clic sur une ligne = sélection de l'objet.
    diag_click(&mut h, l.kept, 0);
    assert_eq!(h.state().editor.selection.entities, vec![seg]);
    assert!(h.state().editor.selection.points.is_empty());
    diag_click(&mut h, l.formula, 0);
    assert_eq!(h.state().editor.selection.points, vec![formula_point]);
    assert!(h.state().editor.selection.entities.is_empty());
    diag_click(&mut h, l.imposed, 0);
    assert_eq!(h.state().editor.selection.entities, vec![imposed_curve]);
    diag_click(&mut h, l.equality, 0);
    assert_eq!(h.state().editor.selection.entities, vec![equality_curve]);
    // Une variable ouvre la fenêtre « Variables » (rien à sélectionner dans le plan).
    assert!(h.state().variables.is_none());
    diag_click(&mut h, l.unused, 0);
    assert!(h.state().variables.is_some());
    // Rien n'a été écrit dans le projet par ces clics.
    assert!(!h.state().editor.doc.is_dirty());
    // Bascule de feuille : un lien sur une seconde feuille, la première étant active.
    let (second, second_segment) = {
        let ed = &mut h.state_mut().editor;
        let second = ed
            .exec(pf_core::Command::CreateSheet {
                name: "Second".into(),
            })
            .unwrap()
            .sheet
            .unwrap();
        let o = ed
            .exec(pf_core::Command::AddSegmentPolar {
                sheet: second,
                from: pf_core::PointRef::New(pt(0.0, 0.0)),
                length_mm: 40.0,
                angle_deg: 20.0,
            })
            .unwrap();
        ed.exec(pf_core::Command::SetConstructionKept {
            sheet: second,
            segment: o.entities[0],
            kept: true,
        })
        .unwrap();
        ed.set_sheet(sheet);
        (second, o.entities[0])
    };
    h.run_steps(3);
    assert_eq!(h.state().editor.sheet, sheet);
    assert_eq!(diag_rows(&h, l.kept).len(), 2);
    diag_click(&mut h, l.kept, 1);
    assert_eq!(h.state().editor.sheet, second, "feuille non changée");
    assert_eq!(h.state().editor.selection.entities, vec![second_segment]);
    // Fichier altéré : une formule dont la variable a disparu est une erreur, avec son groupe,
    // et n'est plus comptée comme lien actif.
    let mut broken = crate::stages::diagnostic_project().unwrap();
    broken.sheets[0].formulas[0].expr = "absente".into();
    load_project(&mut h, broken);
    h.get_by_label_contains(l.error_group);
    assert!(
        diag_rows(&h, l.formula).is_empty(),
        "lien actif et erreur à la fois"
    );
    // Échap ferme la plus récente (« Variables »), puis le diagnostic, sans toucher au projet.
    key(&mut h, Key::Escape);
    h.run_steps(3);
    assert!(h.state().variables.is_none() && h.state().diagnostic.is_some());
    key(&mut h, Key::Escape);
    h.run_steps(3);
    assert!(h.state().diagnostic.is_none());
}

#[test]
fn us035_diagnostic_fr_groupes_compteurs_vide_clic_selection_et_feuille() {
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
    diagnostic_walk(DiagLabels {
        view_menu: "Affichage",
        menu_item: "Diagnostic des liens",
        empty: "Aucun lien dans ce projet.",
        counts: "Erreurs : 0 · Avertissements : 3 · Informations : 4",
        info_group: "Informations : liens actifs (4)",
        warning_group: "Avertissements : liens fragiles (3)",
        error_group: "Erreurs : liens non résolus",
        formula: "formule « width/2+10 » = 70",
        kept: "construction maintenue, suit",
        imposed: "longueur imposée de 300",
        equality: "longueur imposée égale à",
        locked: "calque verrouillé",
        unused: "aucune formule ni autre variable ne la cite",
        vocabulary_ban: "non satisfaite",
    });
}

#[test]
fn us035_diagnostic_en_groups_counts_empty_click_selection_and_sheet() {
    pf_i18n::set_lang(pf_i18n::Lang::En);
    diagnostic_walk(DiagLabels {
        view_menu: "View",
        menu_item: "Link diagnostics",
        empty: "No links in this project.",
        counts: "Errors: 0 · Warnings: 3 · Information: 4",
        info_group: "Information: active links (4)",
        warning_group: "Warnings: fragile links (3)",
        error_group: "Errors: unresolved links",
        formula: "formula \"width/2+10\" = 70",
        kept: "kept construction, follows",
        imposed: "imposed length of 300",
        equality: "imposed length equal to",
        locked: "locked layer",
        unused: "no formula or other variable cites it",
        vocabulary_ban: "not satisfied",
    });
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
}

// ---------------------------------------------------------------------------------------------
// US-03.6 — fenêtre « Dépendances » et confirmation avant une suppression qui touche d'autres objets
// ---------------------------------------------------------------------------------------------

/// Libellés d'un parcours de la fenêtre « Dépendances », en français et en anglais.
struct DepLabels {
    view_menu: &'static str,
    menu_item: &'static str,
    empty: &'static str,
    parents_direct: &'static str,
    children_direct: &'static str,
    parents_all: &'static str,
    children_all: &'static str,
    all_levels: &'static str,
    hidden: &'static str,
    nature_construction: &'static str,
    nature_dimension: &'static str,
    deep: &'static str,
    delete_title: &'static str,
    delete_confirm: &'static str,
    unlinked_row: fn(&str, &str) -> String,
    sep: &'static str,
    banned: &'static str,
}

fn dependencies_walk(l: DepLabels) {
    use pf_core::DependencyObject as O;
    let mut h = diagnostic_harness();
    let (project, f) = crate::stages::dependances_project().unwrap();
    let sheet = project.sheets[0].id;
    let tree = project.dependency_tree(&O::Point(f), None).unwrap();
    let seg = tree
        .parents
        .iter()
        .find_map(|e| match e.object {
            O::Entity(id) if e.depth == 1 => Some(id),
            _ => None,
        })
        .expect("segment parent");
    let deep_child = tree
        .children
        .iter()
        .find_map(|e| match e.object {
            O::Point(id) if e.depth == 2 => Some(id),
            _ => None,
        })
        .expect("petit-enfant");
    let seg_name = crate::dependencies::object_name(&project, &O::Entity(seg));
    load_project(&mut h, project.clone());
    // Ouverture par le menu Affichage (aucun raccourci) : sans sélection, un état vide clair.
    h.get_by_label(l.view_menu).click_accesskit();
    h.run_steps(3);
    h.get_by_label(l.menu_item).click_accesskit();
    h.run_steps(3);
    assert!(h.state().dependencies.is_some(), "fenêtre non ouverte");
    h.get_by_label(l.empty);
    // Le point F choisi : compteurs directs, sections repliées par défaut.
    h.state_mut().editor.selection.points.push(f);
    h.run_steps(3);
    h.get_by_label(l.parents_direct);
    h.get_by_label(l.children_direct);
    assert!(
        h.query_by_label_contains(l.nature_construction).is_none(),
        "section dépliée par défaut"
    );
    assert!(
        h.state().canvas_rect.width() >= 400.0,
        "canevas : {}",
        h.state().canvas_rect.width()
    );
    // Dépliées : une ligne par lien direct, avec sa nature ; les indirects sont comptés, pas listés.
    h.get_by_label(l.parents_direct).click_accesskit();
    h.run_steps(3);
    h.get_by_label(l.children_direct).click_accesskit();
    h.run_steps(3);
    let seg_row = format!("{seg_name}{}{}", l.sep, l.nature_construction);
    assert_eq!(diag_rows(&h, &seg_row).len(), 1, "{seg_row}");
    assert!(!diag_rows(&h, l.nature_construction).is_empty());
    assert!(!diag_rows(&h, l.nature_dimension).is_empty());
    h.get_by_label_contains(l.hidden);
    assert!(h.query_by_label_contains(l.deep).is_none());
    // Tous les niveaux : transitifs avec leur profondeur.
    h.get_by_label(l.all_levels).click_accesskit();
    h.run_steps(3);
    h.get_by_label(l.parents_all);
    h.get_by_label(l.children_all);
    assert!(!diag_rows(&h, l.deep).is_empty(), "niveau 2 absent");
    assert!(h.query_by_label_contains(l.hidden).is_none());
    // Jamais « longueur imposée » ni « contrainte » pour ces liens.
    assert!(h.query_by_label_contains(l.banned).is_none());
    // Clic sur le segment parent = sélection de ce tracé.
    diag_click(&mut h, &seg_row, 0);
    assert_eq!(h.state().editor.selection.entities, vec![seg]);
    assert!(h.state().editor.selection.points.is_empty());
    assert_eq!(h.state().editor.sheet, sheet);
    // Clic sur un dépendant indirect (niveau 2) = sélection de ce point.
    h.state_mut().editor.selection.clear();
    h.state_mut().editor.selection.points.push(f);
    h.run_steps(3);
    let deep_name = crate::dependencies::object_name(&project, &O::Point(deep_child));
    let deep_row = format!("{deep_name}{}{}, {}", l.sep, l.nature_construction, l.deep);
    assert_eq!(diag_rows(&h, &deep_row).len(), 1, "{deep_row}");
    diag_click(&mut h, &deep_row, 0);
    assert_eq!(h.state().editor.selection.points, vec![deep_child]);
    // Rien n'a été écrit dans le projet par ces clics.
    assert!(!h.state().editor.doc.is_dirty());
    assert_eq!(*h.state().editor.project(), project);
    // Échap ferme la fenêtre sans toucher au projet.
    key(&mut h, Key::Escape);
    h.run_steps(3);
    assert!(h.state().dependencies.is_none());
    assert_eq!(*h.state().editor.project(), project);

    // ----- suppression avec dépendants : modale, Échap annule, confirmer = une étape ---------
    load_project(&mut h, project.clone());
    let impact = project.deletion_impact(sheet, &[O::Entity(seg)]);
    assert!(!impact.is_trivial() && impact.refused.is_none());
    let child = crate::dependencies::object_name(&project, &O::Point(f));
    let unlinked = (l.unlinked_row)(&child, &seg_name);
    h.state_mut().editor.selection.entities.push(seg);
    key(&mut h, Key::Delete);
    h.run_steps(3);
    h.get_by_label(l.delete_title);
    assert!(
        !diag_rows(&h, &unlinked).is_empty(),
        "conséquence non listée : {unlinked}"
    );
    assert!(h.state().pending_delete.is_some());
    assert_eq!(
        *h.state().editor.project(),
        project,
        "supprimé avant confirmation"
    );
    // Échap = annuler, rien n'est changé, la sélection reste.
    key(&mut h, Key::Escape);
    h.run_steps(3);
    assert!(h.state().pending_delete.is_none());
    assert!(h.query_by_label(l.delete_title).is_none());
    assert_eq!(*h.state().editor.project(), project);
    assert_eq!(h.state().editor.selection.entities, vec![seg]);
    assert!(!h.state().editor.doc.is_dirty());
    // Confirmer : une seule étape d'historique, annulable.
    key(&mut h, Key::Delete);
    h.run_steps(3);
    // Le bouton principal de la boîte (un autre « Supprimer », désactivé, existe dans le panneau).
    h.get_all_by_label(l.delete_confirm)
        .find(|n| !n.accesskit_node().is_disabled())
        .expect("bouton de confirmation")
        .click_accesskit();
    h.run_steps(3);
    assert!(h.state().pending_delete.is_none());
    {
        let p = h.state().editor.project();
        assert!(
            p.sheet(sheet).unwrap().entity(seg).is_err(),
            "tracé conservé"
        );
        assert!(p
            .sheet(sheet)
            .unwrap()
            .point(f)
            .unwrap()
            .definition
            .is_free());
    }
    assert!(h.state_mut().editor.doc.undo());
    assert_eq!(
        *h.state().editor.project(),
        project,
        "annulation en plusieurs étapes"
    );
    assert!(!h.state_mut().editor.doc.undo(), "plus d'une étape");

    // ----- suppression sans dépendant : immédiate, pas de modale -----------------------------
    assert!(project
        .deletion_impact(sheet, &[O::Point(deep_child)])
        .is_trivial());
    load_project(&mut h, project.clone());
    h.state_mut().editor.selection.points.push(deep_child);
    key(&mut h, Key::Delete);
    h.run_steps(3);
    assert!(h.state().pending_delete.is_none());
    assert!(h.query_by_label(l.delete_title).is_none());
    assert!(h
        .state()
        .editor
        .project()
        .sheet(sheet)
        .unwrap()
        .point(deep_child)
        .is_err());
}

#[test]
fn us036_dependances_fr_arbre_selection_modale_echap_et_une_etape() {
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
    dependencies_walk(DepLabels {
        view_menu: "Affichage",
        menu_item: "Dépendances",
        empty: "Aucun objet sélectionné.",
        parents_direct: "Parents (1)",
        children_direct: "Dépendants (2)",
        parents_all: "Parents (4)",
        children_all: "Dépendants (3)",
        all_levels: "Afficher tous les niveaux",
        hidden: "indirect(s) non affiché(s)",
        nature_construction: "lien de construction",
        nature_dimension: "cote liée",
        deep: "niveau 2",
        delete_title: "Confirmer la suppression",
        delete_confirm: "Supprimer",
        unlinked_row: |child, parent| {
            format!("{child} : lien de construction avec {parent} retiré")
        },
        sep: " : ",
        banned: "contrainte",
    });
}

// ===== US-09.1 : fenêtre « Tailles » ==============================================================

/// Libellés d'une langue pour le parcours de la fenêtre « Tailles ».
struct SzLabels {
    view_menu: &'static str,
    menu_item: &'static str,
    empty: &'static str,
    new_title: &'static str,
    create: &'static str,
    range_name: &'static str,
    header: &'static str,
    set_base: &'static str,
    break_: &'static str,
    base: &'static str,
    remove_range: &'static str,
    err_empty_name: &'static str,
    err_remove_base: &'static str,
    err_dup_label: &'static str,
    err_taken_label: &'static str,
    err_dup_range: &'static str,
    err_last_size: &'static str,
    down: &'static str,
}

/// Champ de texte du dock n° `index` (de haut en bas puis de gauche à droite ; négatif = depuis la
/// fin) : le focus puis tout sélectionner et saisir ; `enter` valide.
fn sz_type(h: &mut Harness<'_, App>, index: i32, text: &str, enter: bool) {
    {
        let dock = h.state().dock.rect;
        let mut fields: Vec<_> = h
            .get_all_by_role(egui::accesskit::Role::TextInput)
            .filter(|f| dock.contains(f.rect().center()))
            .collect();
        fields.sort_by(|a, b| {
            a.rect()
                .min
                .y
                .total_cmp(&b.rect().min.y)
                .then(a.rect().min.x.total_cmp(&b.rect().min.x))
        });
        let i = if index < 0 {
            fields.len() as i32 + index
        } else {
            index
        };
        fields[i as usize].focus();
    }
    h.run_steps(2);
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.run_steps(1);
    type_text(h, text);
    if enter {
        key(h, Key::Enter);
    }
    h.run_steps(3);
}

fn sz_labels(h: &Harness<'_, App>, range: usize) -> Vec<String> {
    h.state().editor.project().sizes[range]
        .sizes
        .iter()
        .map(|s| s.label.clone())
        .collect()
}

fn sz_base(h: &Harness<'_, App>, range: usize) -> String {
    h.state().editor.project().sizes[range]
        .base_size()
        .unwrap()
        .label
        .clone()
}

fn sz_click(h: &mut Harness<'_, App>, label: &str, nth: usize) {
    h.get_all_by_label(label)
        .nth(nth)
        .unwrap_or_else(|| panic!("aucun « {label} » n° {nth}"))
        .click_accesskit();
    h.run_steps(3);
}

fn sizes_walk(l: SzLabels) {
    let mut h = harness();
    // Ouverture par le menu Affichage (aucun raccourci) ; fenêtre dans le dock, canevas préservé.
    h.get_by_label(l.view_menu).click_accesskit();
    h.run_steps(3);
    h.get_by_label(l.menu_item).click_accesskit();
    h.run_steps(3);
    assert!(h.state().sizes.is_some(), "fenêtre non ouverte");
    assert!(h.state().dock.open().contains(&crate::dock::ToolWin::Sizes));
    let win = h
        .ctx
        .memory(|m| m.area_rect(crate::dock::ToolWin::Sizes.id()))
        .expect("fenêtre Tailles");
    let dock = h.state().dock.rect;
    assert!(dock.contains_rect(win), "{win:?} hors du dock {dock:?}");
    assert!(!win.intersects(h.state().canvas_rect));
    assert!(
        h.state().canvas_rect.width() >= 400.0,
        "canevas de {} pt",
        h.state().canvas_rect.width()
    );
    h.get_by_label(l.empty);
    // Aucune section dépliée par défaut : pas un seul champ de saisie.
    assert_eq!(
        h.query_all_by_role(egui::accesskit::Role::TextInput)
            .filter(|f| dock.contains(f.rect().center()))
            .count(),
        0,
        "section dépliée par défaut"
    );
    // Création refusée (nom vide) : message localisé, rien n'est modifié.
    h.get_by_label(l.new_title).click_accesskit();
    h.run_steps(3);
    sz_type(&mut h, 1, "XS S M L XL", false);
    h.get_by_label(l.create).click_accesskit();
    h.run_steps(3);
    h.get_by_label(l.err_empty_name);
    assert!(h.state().editor.project().sizes.is_empty());
    // Création : la première taille est la base, la gamme s'ouvre.
    sz_type(&mut h, 0, l.range_name, false);
    h.get_by_label(l.create).click_accesskit();
    h.run_steps(3);
    assert_eq!(sz_labels(&h, 0), ["XS", "S", "M", "L", "XL"]);
    assert_eq!(sz_base(&h, 0), "XS");
    assert_eq!(h.state().editor.project().sizes[0].name, l.range_name);
    h.get_by_label(l.header);
    assert!(h.query_by_label(l.err_empty_name).is_none());
    // Base : icône + texte « Base » sur une seule ligne ; la désigner ailleurs la déplace.
    assert_eq!(h.get_all_by_label(l.base).count(), 1);
    sz_click(&mut h, l.set_base, 1); // XS est la base : S est le premier bouton, M le deuxième
    assert_eq!(sz_base(&h, 0), "M");
    assert_eq!(h.get_all_by_label(l.base).count(), 1);
    // Rupture : marquer la taille S (deuxième ligne).
    sz_click(&mut h, l.break_, 1);
    assert!(h.state().editor.project().sizes[0].sizes[1].break_size);
    assert!(!h.state().editor.project().sizes[0].sizes[0].break_size);
    // Refus : retirer la base. Message localisé, projet identique.
    let before = h.state().editor.project().clone();
    sz_click(&mut h, "×", 2);
    h.get_by_label(l.err_remove_base);
    assert_eq!(*h.state().editor.project(), before);
    // Refus : libellé en double (casse ignorée) à l'ajout.
    sz_type(&mut h, 6, "xs", true);
    h.get_by_label(l.err_dup_label);
    assert_eq!(*h.state().editor.project(), before);
    // Ajout valide en fin de gamme : l'erreur disparaît.
    sz_type(&mut h, 6, "2XL", true);
    assert_eq!(sz_labels(&h, 0), ["XS", "S", "M", "L", "XL", "2XL"]);
    assert!(h.query_by_label(l.err_dup_label).is_none());
    let with_2xl = h.state().editor.project().clone();
    // Retrait de S : une seule étape annulable, la base et les autres tailles ne bougent pas.
    sz_click(&mut h, "×", 1);
    assert_eq!(sz_labels(&h, 0), ["XS", "M", "L", "XL", "2XL"]);
    assert_eq!(sz_base(&h, 0), "M");
    undo(&mut h);
    assert_eq!(*h.state().editor.project(), with_2xl);
    // Déplacement : XS descend d'un rang ; annuler.
    sz_click(&mut h, l.down, 0);
    assert_eq!(sz_labels(&h, 0), ["S", "XS", "M", "L", "XL", "2XL"]);
    undo(&mut h);
    assert_eq!(*h.state().editor.project(), with_2xl);
    // Renommage d'une taille, puis refus (libellé déjà pris).
    sz_type(&mut h, 1, "Petit", true);
    assert_eq!(sz_labels(&h, 0)[0], "Petit");
    let before = h.state().editor.project().clone();
    sz_type(&mut h, 2, "Petit", true);
    h.get_by_label(l.err_taken_label);
    assert_eq!(*h.state().editor.project(), before);
    // Renommage de la gamme.
    sz_type(&mut h, 0, "Dames", true);
    assert_eq!(h.state().editor.project().sizes[0].name, "Dames");
    // Gamme en double (casse ignorée) à la création : refus, rien n'est modifié.
    let before = h.state().editor.project().clone();
    sz_type(&mut h, -2, "dames", false);
    sz_type(&mut h, -1, "A B", false);
    h.get_by_label(l.create).click_accesskit();
    h.run_steps(3);
    h.get_by_label(l.err_dup_range);
    assert_eq!(*h.state().editor.project(), before);
    // Une seconde gamme, indépendante.
    sz_type(&mut h, -2, "Tour", false);
    h.get_by_label(l.create).click_accesskit();
    h.run_steps(3);
    assert_eq!(h.state().editor.project().sizes.len(), 2);
    assert_eq!(sz_labels(&h, 1), ["A", "B"]);
    assert_eq!(
        sz_labels(&h, 0)[0],
        "Petit",
        "la première gamme n'a pas bougé"
    );
    // Retirer la dernière taille d'une gamme : refusé (supprimer la gamme à la place).
    // Retrait de B (non base) puis de A (base : refusé, c'est aussi la dernière).
    let remove_count = h.get_all_by_label("×").count();
    sz_click(&mut h, "×", remove_count - 1); // B
    assert_eq!(sz_labels(&h, 1), ["A"]);
    sz_click(&mut h, "×", remove_count - 2); // A, seule taille de la gamme
    h.get_by_label(l.err_last_size);
    assert_eq!(sz_labels(&h, 1), ["A"]);
    // Suppression de la seconde gamme par son bouton : une seule étape, annulable.
    sz_click(&mut h, l.remove_range, 1);
    assert_eq!(h.state().editor.project().sizes.len(), 1);
    undo(&mut h);
    assert_eq!(h.state().editor.project().sizes.len(), 2);
    // Aucune mention d'un calcul gradé : rien n'est gradé dans cette story.
    assert!(h.query_by_label_contains("gradué").is_none());
}

#[test]
fn us091_sizes_fr_fenetre_ajout_retrait_base_rupture_et_refus_localises() {
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
    sizes_walk(SzLabels {
        view_menu: "Affichage",
        menu_item: "Tailles…",
        empty: "Aucune gamme de tailles.",
        new_title: "Nouvelle gamme",
        create: "Créer la gamme",
        range_name: "Femme",
        header: "Femme (alphanumérique, 5 tailles)",
        set_base: "Définir base",
        break_: "Rupture",
        base: "Base",
        remove_range: "Supprimer la gamme",
        err_empty_name: "Nom de gamme refusé : vide.",
        err_remove_base:
            "« M » est la taille de base : désignez d'abord une autre taille comme base.",
        err_dup_label:
            "La taille « xs » existe déjà dans cette gamme (majuscules et minuscules confondues).",
        err_taken_label:
            "La taille « Petit » existe déjà dans cette gamme (majuscules et minuscules confondues).",
        err_dup_range:
            "Une gamme « dames » existe déjà (majuscules et minuscules confondues).",
        err_last_size: "Une gamme garde au moins une taille : supprimez plutôt la gamme.",
        down: "Descendre la taille",
    });
}

#[test]
fn us091_sizes_en_window_add_remove_base_break_and_localized_refusals() {
    pf_i18n::set_lang(pf_i18n::Lang::En);
    sizes_walk(SzLabels {
        view_menu: "View",
        menu_item: "Sizes...",
        empty: "No size range yet.",
        new_title: "New range",
        create: "Create range",
        range_name: "Women",
        header: "Women (alphanumeric, 5 sizes)",
        set_base: "Set as base",
        break_: "Break",
        base: "Base",
        remove_range: "Delete range",
        err_empty_name: "Range name refused: empty.",
        err_remove_base: "\"M\" is the base size: pick another size as the base first.",
        err_dup_label:
            "Size \"xs\" already exists in this range (upper and lower case are the same).",
        err_taken_label:
            "Size \"Petit\" already exists in this range (upper and lower case are the same).",
        err_dup_range: "A range \"dames\" already exists (upper and lower case are the same).",
        err_last_size: "A range keeps at least one size: delete the range instead.",
        down: "Move size down",
    });
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
}

#[test]
fn us036_dependencies_en_tree_selection_modal_escape_and_one_step() {
    pf_i18n::set_lang(pf_i18n::Lang::En);
    dependencies_walk(DepLabels {
        view_menu: "View",
        menu_item: "Dependencies",
        empty: "No object selected.",
        parents_direct: "Parents (1)",
        children_direct: "Dependents (2)",
        parents_all: "Parents (4)",
        children_all: "Dependents (3)",
        all_levels: "Show all levels",
        hidden: "indirect object(s) not shown",
        nature_construction: "construction link",
        nature_dimension: "linked dimension",
        deep: "level 2",
        delete_title: "Confirm deletion",
        delete_confirm: "Delete",
        unlinked_row: |child, parent| format!("{child}: construction link with {parent} removed"),
        sep: ": ",
        banned: "constraint",
    });
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
}

// ===== US-09.2a : fenêtre « Gradation » et taille active ========================================

/// Libellés d'une langue pour le parcours de la fenêtre « Gradation ».
struct GrLabels {
    view_menu: &'static str,
    menu_item: &'static str,
    none_selected: &'static str,
    header: &'static str,
    count_one: &'static str,
    err_number: &'static str,
    err_bounds: &'static str,
    selector_base: &'static str,
    selector_xl: &'static str,
    base_item: &'static str,
    readonly: &'static str,
    banner: &'static str,
    clear: &'static str,
    export_note: &'static str,
    print_menu: &'static str,
    print_item: &'static str,
}

fn painted_strings(h: &Harness<'_, App>) -> Vec<String> {
    flat_shapes(h)
        .iter()
        .filter_map(|s| match s {
            egui::epaint::Shape::Text(t) => Some(t.galley.text().to_string()),
            _ => None,
        })
        .collect()
}

fn gr_position(
    h: &Harness<'_, App>,
    point: pf_core::PointId,
    size: Option<pf_core::SizeId>,
) -> (f64, f64) {
    let p = h.state().editor.project();
    let e = p.evaluate_sheet(h.state().editor.sheet, size).unwrap();
    let v = e.points.iter().find(|(i, _)| *i == point).unwrap().1;
    (v.x, v.y)
}

fn grading_walk(l: GrLabels) {
    use pf_core::{Command, Document, PointRef, SizeKind};
    let mut project = pf_core::Project::new("Grading");
    let sheet = project.sheets[0].id;
    let point = project
        .apply(Command::AddPoint {
            sheet,
            at: pt(100.0, 200.0),
        })
        .unwrap()
        .points[0];
    project
        .apply(Command::AddSizeRange {
            name: "Women".into(),
            kind: SizeKind::Alphanumeric,
            labels: ["XS", "S", "M", "L", "XL"].map(String::from).to_vec(),
            base: 2,
        })
        .unwrap();
    let sizes: Vec<_> = project.sizes[0].sizes.iter().map(|s| s.id).collect();
    let mut h = harness();
    h.state_mut().editor = crate::editor::Editor::new(Document::new(project));
    h.state_mut().fit_pending = true;
    h.run_steps(3);

    // Ouverture par le menu Affichage (aucun raccourci) ; fenêtre dans le dock, canevas préservé.
    h.get_by_label(l.view_menu).click_accesskit();
    h.run_steps(3);
    h.get_by_label(l.menu_item).click_accesskit();
    h.run_steps(3);
    assert!(h.state().grading.is_some(), "fenêtre non ouverte");
    assert!(h
        .state()
        .dock
        .open()
        .contains(&crate::dock::ToolWin::Grading));
    let win = h
        .ctx
        .memory(|m| m.area_rect(crate::dock::ToolWin::Grading.id()))
        .expect("fenêtre Gradation");
    let dock = h.state().dock.rect;
    assert!(dock.contains_rect(win), "{win:?} hors du dock {dock:?}");
    assert!(!win.intersects(h.state().canvas_rect));
    assert!(h.state().canvas_rect.width() >= 400.0);
    h.get_by_label(l.none_selected);
    // Sélecteur de taille active dans la barre d'état : « base » au départ.
    h.get_by_label(l.selector_base);

    // Point sélectionné : la gamme est repliée par défaut (aucun champ de saisie).
    h.state_mut().editor.selection.points.push(point);
    h.run_steps(3);
    h.get_by_label(l.header);
    let fields = |h: &Harness<'_, App>| {
        h.query_all_by_role(egui::accesskit::Role::TextInput)
            .filter(|f| dock.contains(f.rect().center()))
            .count()
    };
    assert_eq!(fields(&h), 0, "section dépliée par défaut");
    h.get_by_label(l.header).click_accesskit();
    h.run_steps(3);
    // XS, S, L, XL : 2 champs chacun ; la base M n'a pas de champ (écart nul, texte).
    assert_eq!(fields(&h), 8);

    // CA : cinq tailles saisies dans le tableau ; position en taille T = base + écart.
    let entries = [
        (0, -9.0, -20.0, 0),
        (1, -5.0, -10.0, 2),
        (3, 5.0, 10.0, 4),
        (4, 10.0, 22.0, 6),
    ];
    for (size_idx, dx, dy, first_field) in entries {
        sz_type(&mut h, first_field, &format!("{dx}"), true);
        sz_type(&mut h, first_field + 1, &format!("{dy}"), true);
        let g = h
            .state()
            .editor
            .project()
            .sheet(sheet)
            .unwrap()
            .grading(point, sizes[size_idx])
            .copied()
            .unwrap_or_else(|| panic!("écart {size_idx} absent"));
        assert_eq!((g.dx_mm, g.dy_mm), (dx, dy), "taille {size_idx}");
    }
    for (i, (dx, dy)) in [
        (-9.0, -20.0),
        (-5.0, -10.0),
        (0.0, 0.0),
        (5.0, 10.0),
        (10.0, 22.0),
    ]
    .into_iter()
    .enumerate()
    {
        let got = gr_position(&h, point, Some(sizes[i]));
        assert!(
            (got.0 - (100.0 + dx)).abs() < 1e-9 && (got.1 - (200.0 + dy)).abs() < 1e-9,
            "taille {i} : {got:?}"
        );
    }
    h.get_by_label(l.count_one);
    // La géométrie écrite n'a pas bougé.
    assert_eq!(gr_position(&h, point, None), (100.0, 200.0));

    // Refus : saisie non numérique et hors bornes, messages localisés, projet inchangé.
    let before = h.state().editor.project().clone();
    sz_type(&mut h, 0, "abc", true);
    h.get_by_label(l.err_number);
    assert_eq!(*h.state().editor.project(), before);
    sz_type(&mut h, 0, "2000000", true);
    h.get_by_label(l.err_bounds);
    assert_eq!(*h.state().editor.project(), before);

    // Taille active : le sélecteur de la barre d'état affiche XL (lecture seule hors base).
    h.get_by_label(l.selector_base).click_accesskit();
    h.run_steps(3);
    h.get_by_role_and_label(egui::accesskit::Role::Button, "XL")
        .click_accesskit();
    h.run_steps(4);
    assert_eq!(h.state().editor.active_size, Some(sizes[4]));
    h.get_by_label(l.selector_xl);
    assert!(
        painted_strings(&h).iter().any(|t| t == l.banner),
        "bandeau « {} » absent",
        l.banner
    );
    // Le plan montre la géométrie évaluée de XL : le point y est à (110 ; 222), jamais écrit.
    let shown = h.state().editor.sheet().position(point).unwrap();
    assert_eq!((shown.x, shown.y), (110.0, 222.0));
    assert_eq!(gr_position(&h, point, None), (100.0, 200.0));
    // Édition de géométrie désactivée, avec le message demandé ; rien n'est modifié.
    let before = h.state().editor.project().clone();
    key(&mut h, Key::S);
    assert_eq!(
        h.state().editor.tool,
        Tool::Select,
        "outil activé en lecture seule"
    );
    h.get_by_label_contains(l.readonly);
    assert!(h
        .state_mut()
        .editor
        .exec(pf_core::Command::AddPoint {
            sheet,
            at: pt(5.0, 5.0)
        })
        .is_none());
    h.run_steps(2);
    h.get_by_label_contains(l.readonly);
    assert!(!h
        .state_mut()
        .editor
        .begin_drag(crate::editor::Target::Point(point)));
    assert_eq!(*h.state().editor.project(), before);
    // La fenêtre de gradation reste éditable : L passe à (6 ; 10), en une étape annulable.
    sz_type(&mut h, 4, "6", true);
    let g = h
        .state()
        .editor
        .project()
        .sheet(sheet)
        .unwrap()
        .grading(point, sizes[3])
        .copied()
        .unwrap();
    assert_eq!((g.dx_mm, g.dy_mm), (6.0, 10.0));
    undo(&mut h);
    let g = h
        .state()
        .editor
        .project()
        .sheet(sheet)
        .unwrap()
        .grading(point, sizes[3])
        .copied()
        .unwrap();
    assert_eq!((g.dx_mm, g.dy_mm), (5.0, 10.0));
    // Mention d'export (US-09.8) : « taille M » (la base, choix par défaut) dans la fenêtre
    // d'export même quand une autre taille est affichée ; l'export ne suit pas l'affichage.
    h.get_by_label(l.print_menu).click_accesskit();
    h.run_steps(3);
    h.get_by_label(l.print_item).click_accesskit();
    h.run_steps(4);
    h.get_by_label(l.export_note);
    assert_eq!(
        h.state().export_scene(sheet).unwrap().points.len(),
        before.sheet(sheet).unwrap().points.len()
    );
    let scene_pos = h.state().export_scene(sheet).unwrap().points[0].pos;
    assert_eq!(
        (scene_pos.x, scene_pos.y),
        (100.0, 200.0),
        "export en taille de base"
    );
    h.state_mut().print_dialog = None;
    h.run_steps(2);

    // Retour à la taille de base : l'édition revient, le bandeau disparaît.
    h.get_by_label(l.selector_xl).click_accesskit();
    h.run_steps(3);
    h.get_by_label(l.base_item).click_accesskit();
    h.run_steps(4);
    assert_eq!(h.state().editor.active_size, None);
    assert!(h.state().editor.size_view().is_none());
    assert!(!painted_strings(&h).iter().any(|t| t == l.banner));
    assert!(h
        .state_mut()
        .editor
        .exec(pf_core::Command::AddPoint {
            sheet,
            at: pt(5.0, 5.0)
        })
        .is_some());
    let _ = PointRef::New(pt(0.0, 0.0));
    // Effacer la gradation du point : une étape, plus aucun écart.
    h.get_by_label(l.clear).click_accesskit();
    h.run_steps(3);
    assert!(h
        .state()
        .editor
        .project()
        .sheet(sheet)
        .unwrap()
        .gradings
        .is_empty());
    undo(&mut h);
    assert_eq!(
        h.state()
            .editor
            .project()
            .sheet(sheet)
            .unwrap()
            .gradings
            .len(),
        4
    );
}

#[test]
fn us092_gradation_fr_fenetre_tableau_taille_active_lecture_seule_et_retour() {
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
    grading_walk(GrLabels {
        view_menu: "Affichage",
        menu_item: "Gradation…",
        none_selected: "Sélectionnez un point libre dans le plan.",
        header: "Women (5 tailles)",
        count_one: "Points gradués dans cette feuille : 1",
        err_number: "« abc » n'est pas un nombre.",
        err_bounds: "Un écart est limité à ±1 000 000 mm.",
        selector_base: "Taille : base",
        selector_xl: "Taille : XL",
        base_item: "Taille de base (géométrie écrite)",
        readonly: "Revenir à la taille de base pour modifier.",
        banner: "Taille XL · lecture seule",
        clear: "Effacer la gradation du point",
        export_note: "Export : taille M",
        print_menu: "Fichier",
        print_item: "Mise en page du PDF…",
    });
}

#[test]
fn us092_grading_en_window_table_active_size_read_only_and_back() {
    pf_i18n::set_lang(pf_i18n::Lang::En);
    grading_walk(GrLabels {
        view_menu: "View",
        menu_item: "Grading...",
        none_selected: "Select a free point in the plan.",
        header: "Women (5 sizes)",
        count_one: "Graded points in this sheet: 1",
        err_number: "\"abc\" is not a number.",
        err_bounds: "An offset is limited to ±1,000,000 mm.",
        selector_base: "Size: base",
        selector_xl: "Size: XL",
        base_item: "Base size (written geometry)",
        readonly: "Go back to the base size to edit.",
        banner: "Size XL · read-only",
        clear: "Clear the point's grading",
        export_note: "Export: size M",
        print_menu: "File",
        print_item: "PDF page setup…",
    });
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
}

// ===== US-09.3a : report et interpolation de gradation ==========================================

/// Libellés d'une langue pour le parcours « Reporter la règle » / « Interpoler ».
struct RpLabels {
    report_title: &'static str,
    interp_title: &'static str,
    copy: &'static str,
    run: &'static str,
    mirror_x: &'static str,
    no_targets: &'static str,
    targets_two: fn(&str) -> String,
    done_copy: fn(&str) -> String,
    done_interp: fn(&str, &str) -> String,
    locked: fn(&str) -> String,
    readonly: &'static str,
}

fn rp_get(
    p: &pf_core::Project,
    sheet: pf_core::SheetId,
    point: pf_core::PointId,
    size: pf_core::SizeId,
) -> Option<(f64, f64)> {
    p.sheet(sheet)
        .unwrap()
        .grading(point, size)
        .map(|g| (g.dx_mm, g.dy_mm))
}

fn rp_close(got: Option<(f64, f64)>, want: Option<(f64, f64)>, what: &str) {
    match (got, want) {
        (None, None) => {}
        (Some(g), Some(w)) => assert!(
            (g.0 - w.0).abs() < 1e-9 && (g.1 - w.1).abs() < 1e-9,
            "{what} : {g:?} != {w:?}"
        ),
        (g, w) => panic!("{what} : {g:?} != {w:?}"),
    }
}

fn report_walk(l: RpLabels) {
    use pf_core::{Command, Document, GradingCopyMode, SizeKind};
    let mut project = pf_core::Project::new("Report");
    let sheet = project.sheets[0].id;
    project
        .apply(Command::AddSizeRange {
            name: "Women".into(),
            kind: SizeKind::Alphanumeric,
            labels: ["XS", "S", "M", "L", "XL"].map(String::from).to_vec(),
            base: 2,
        })
        .unwrap();
    let sizes: Vec<_> = project.sizes[0].sizes.iter().map(|s| s.id).collect();
    let (s_size, l_size, xl_size) = (sizes[1], sizes[3], sizes[4]);
    let add = |p: &mut pf_core::Project, x: f64, y: f64| {
        p.apply(Command::AddPoint {
            sheet,
            at: pt(x, y),
        })
        .unwrap()
        .points[0]
    };
    let a = add(&mut project, 0.0, 0.0);
    let b = add(&mut project, 100.0, 0.0);
    let c = add(&mut project, 25.0, 10.0);
    let d = add(&mut project, 60.0, -5.0);
    let e = add(&mut project, 300.0, 300.0);
    let f = add(&mut project, 400.0, 300.0);
    let g = add(&mut project, 700.0, 700.0);
    for (point, size, dx, dy) in [
        (a, s_size, -5.0, -10.0),
        (a, l_size, 5.0, 10.0),
        (b, s_size, -9.0, -20.0),
        (b, l_size, 9.0, 22.0),
    ] {
        project
            .apply(Command::SetPointGrading {
                sheet,
                point,
                size,
                dx_mm: dx,
                dy_mm: dy,
            })
            .unwrap();
    }
    // Un point sur un calque verrouillé : cible refusée.
    let layer = project
        .apply(Command::AddLayer {
            sheet,
            name: "Verrou".into(),
        })
        .unwrap()
        .layers[0];
    let locked = project
        .apply(Command::InLayer {
            sheet,
            layer,
            command: Box::new(Command::AddPoint {
                sheet,
                at: pt(900.0, 900.0),
            }),
        })
        .unwrap()
        .points[0];
    project
        .apply(Command::SetLayerLocked {
            sheet,
            layer,
            locked: true,
        })
        .unwrap();
    let _ = g;
    let name =
        |h: &Harness<'_, App>, p: pf_core::PointId| h.state().editor.sheet().label(p).to_string();
    let mut h = harness();
    h.state_mut().editor = crate::editor::Editor::new(Document::new(project));
    h.state_mut().fit_pending = true;
    h.run_steps(3);
    h.state_mut().open_grading();
    h.state_mut().editor.selection.points = vec![a];
    h.run_steps(3);
    let proj = |h: &Harness<'_, App>| h.state().editor.project().clone();

    // Sections repliées au départ : ni bouton ni mode.
    assert!(h.query_by_label(l.copy).is_none(), "report déplié");
    assert!(h.query_by_label(l.run).is_none(), "interpolation dépliée");
    h.get_by_label(l.report_title).click_accesskit();
    h.run_steps(3);
    // Sans cible : message et bouton désactivé.
    h.get_by_label_contains(l.no_targets);
    assert!(h.get_by_label(l.copy).accesskit_node().is_disabled());

    // Symétrie X vers deux points : le point affiché (a) reste la source.
    h.state_mut().editor.selection.points = vec![a, e, f];
    h.run_steps(3);
    h.get_by_label((l.targets_two)(&name(&h, a)).as_str());
    assert!(!h.get_by_label(l.copy).accesskit_node().is_disabled());
    h.get_by_label(l.mirror_x).click_accesskit();
    h.run_steps(3);
    let before = proj(&h);
    h.get_by_label(l.copy).click_accesskit();
    h.run_steps(3);
    let after = proj(&h);
    for target in [e, f] {
        rp_close(
            rp_get(&after, sheet, target, s_size),
            Some((5.0, -10.0)),
            "S",
        );
        rp_close(
            rp_get(&after, sheet, target, l_size),
            Some((-5.0, 10.0)),
            "L",
        );
        rp_close(rp_get(&after, sheet, target, xl_size), None, "XL");
    }
    assert_eq!(after.sheet(sheet).unwrap().gradings.len(), 8);
    // La source n'a pas bougé ; une seule étape annulable.
    rp_close(
        rp_get(&after, sheet, a, s_size),
        Some((-5.0, -10.0)),
        "source",
    );
    h.get_by_label((l.done_copy)(&name(&h, a)).as_str());
    undo(&mut h);
    assert_eq!(proj(&h), before, "annuler le report : une étape");

    // Refus affiché lisiblement (cible sur un calque verrouillé), projet inchangé.
    h.state_mut().editor.selection.points = vec![a, locked];
    h.run_steps(3);
    h.get_by_label(l.copy).click_accesskit();
    h.run_steps(3);
    h.get_by_label((l.locked)(&name(&h, locked)).as_str());
    assert_eq!(proj(&h), before, "refus avec trace");

    // Interpolation : c (t = 0,25) et d (t = 0,6) entre a et b.
    h.state_mut().editor.selection.points = vec![c, d];
    h.run_steps(3);
    h.get_by_label(l.interp_title).click_accesskit();
    h.run_steps(3);
    assert!(
        h.get_by_label(l.run).accesskit_node().is_disabled(),
        "A et B non choisis"
    );
    for (n, point) in [a, b].into_iter().enumerate() {
        h.get_all_by_role(egui::accesskit::Role::ComboBox)
            .nth(n)
            .expect("liste A / B")
            .click_accesskit();
        h.run_steps(3);
        h.get_by_role_and_label(egui::accesskit::Role::Button, &name(&h, point))
            .click_accesskit();
        h.run_steps(3);
    }
    assert_eq!(h.state().grading.as_ref().unwrap().from, Some(a));
    assert_eq!(h.state().grading.as_ref().unwrap().to, Some(b));
    assert!(!h.get_by_label(l.run).accesskit_node().is_disabled());
    h.get_by_label(l.run).click_accesskit();
    h.run_steps(3);
    let done = proj(&h);
    // t = 0,25 : S (−6 ; −12,5), L (6 ; 13) ; t = 0,6 : S (−7,4 ; −16), L (7,4 ; 17,2).
    rp_close(rp_get(&done, sheet, c, s_size), Some((-6.0, -12.5)), "c S");
    rp_close(rp_get(&done, sheet, c, l_size), Some((6.0, 13.0)), "c L");
    rp_close(rp_get(&done, sheet, d, s_size), Some((-7.4, -16.0)), "d S");
    rp_close(rp_get(&done, sheet, d, l_size), Some((7.4, 17.2)), "d L");
    rp_close(rp_get(&done, sheet, c, xl_size), None, "c XL");
    h.get_by_label((l.done_interp)(&name(&h, a), &name(&h, b)).as_str());
    undo(&mut h);
    assert_eq!(proj(&h), before, "annuler l'interpolation : une étape");

    // Hors taille de base : boutons désactivés avec le message existant, commandes refusées.
    h.state_mut().editor.selection.points = vec![a, e, f];
    h.state_mut().editor.set_active_size(Some(xl_size));
    h.run_steps(4);
    assert!(h.get_by_label(l.copy).accesskit_node().is_disabled());
    assert!(h.get_by_label(l.run).accesskit_node().is_disabled());
    assert!(h.get_all_by_label_contains(l.readonly).count() >= 2);
    assert!(h
        .state_mut()
        .editor
        .exec(Command::CopyPointGrading {
            sheet,
            source: a,
            targets: vec![e],
            mode: GradingCopyMode::Same,
        })
        .is_none());
    assert_eq!(proj(&h), before);
}

#[test]
fn us093_report_fr_copie_en_miroir_interpolation_refus_et_hors_base() {
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
    report_walk(RpLabels {
        report_title: "Reporter la règle",
        interp_title: "Interpoler entre deux points",
        copy: "Copier vers la sélection",
        run: "Interpoler",
        mirror_x: "Symétrie X (dx devient -dx)",
        no_targets: "Aucune cible : ajoutez des points libres",
        targets_two: |a| format!("Cibles : 2 point(s) sélectionné(s) en plus de {a}."),
        done_copy: |a| format!("Règle de {a} reportée vers 2 point(s)."),
        done_interp: |a, b| format!("2 point(s) interpolé(s) entre {a} et {b}."),
        locked: |p| format!("Le point {p} est sur un calque verrouillé."),
        readonly: "Revenir à la taille de base pour modifier.",
    });
}

#[test]
fn us093_report_en_mirror_copy_interpolation_refusal_and_read_only() {
    pf_i18n::set_lang(pf_i18n::Lang::En);
    report_walk(RpLabels {
        report_title: "Copy the rule",
        interp_title: "Interpolate between two points",
        copy: "Copy to the selection",
        run: "Interpolate",
        mirror_x: "Mirror X (dx becomes -dx)",
        no_targets: "No target: add free points",
        targets_two: |a| format!("Targets: 2 selected point(s) besides {a}."),
        done_copy: |a| format!("Rule of {a} copied to 2 point(s)."),
        done_interp: |a, b| format!("2 point(s) interpolated between {a} and {b}."),
        locked: |p| format!("Point {p} is on a locked layer."),
        readonly: "Go back to the base size to edit.",
    });
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
}

// ===== US-09.5a : géométrie dérivée gradée =======================================================

/// Libellés d'une langue pour le parcours « géométrie dérivée dans une taille ».
struct DvLabels {
    note_xl: &'static str,
    note_any: &'static str,
    readonly: &'static str,
}

fn derived_walk(l: DvLabels) {
    use pf_core::Document;
    let (project, corner, xl, piece) = crate::stages::gradation_derivee_project().unwrap();
    let sheet = project.sheets[0].id;
    let mut h = harness();
    // Fenêtre haute : l'inspecteur défile, les mesures de la pièce sont sous le pli.
    h.set_size(egui::vec2(1280.0, 1600.0));
    h.state_mut().editor = crate::editor::Editor::new(Document::new(project));
    h.state_mut().fit_pending = true;
    h.run_steps(4);

    // Taille de base : aucune mention de taille dans l'inspecteur, mesures de base.
    assert!(h.query_by_label_contains(l.note_any).is_none());
    assert!(h.get_all_by_label_contains(&cm2(240_000.0)).count() >= 1);

    // Taille XL : l'inspecteur grisé le dit, les mesures de la pièce sont celles de XL.
    h.state_mut().editor.set_active_size(Some(xl));
    h.run_steps(4);
    h.get_by_label(l.note_xl);
    assert!(h.get_all_by_label_contains(&cm2(408.0 * 612.0)).count() >= 1);
    assert!(h.query_by_label_contains(&cm2(240_000.0)).is_none());
    // Le plan évalué donne la coupe de XL (rectangle 408 × 612, marge 12 : 432 × 636) ; le
    // projet enregistré garde la base (400 × 600, marge 12 : 424 × 624).
    let view = h.state().editor.view_project().clone();
    let cut = view.piece_cut_contour(piece).unwrap();
    assert!((cut.area_mm2 - 432.0 * 636.0).abs() < 1e-6);
    assert!((cut.perimeter_mm - 2.0 * (432.0 + 636.0)).abs() < 1e-6);
    let base_cut = h.state().editor.project().piece_cut_contour(piece).unwrap();
    assert!((base_cut.area_mm2 - 424.0 * 624.0).abs() < 1e-6);
    // Le cran à distance fixe est à 150 mm de B (408 ; 0) sur le bord droit : (408 ; 150).
    let d = h
        .state()
        .editor
        .project()
        .derived_for_size(sheet, Some(xl))
        .unwrap();
    let foot = d.notches[0].marks.clone().unwrap().1[0].0;
    assert!((foot.x - 408.0).abs() < 1e-6 && (foot.y - 150.0).abs() < 1e-6);
    // Lecture seule : une édition de géométrie est refusée, le projet ne bouge pas.
    let before = h.state().editor.project().clone();
    assert!(h
        .state_mut()
        .editor
        .exec(pf_core::Command::MovePoint {
            sheet,
            point: corner,
            to: pt(1.0, 1.0),
        })
        .is_none());
    assert_eq!(*h.state().editor.project(), before);
    h.run_steps(2);
    h.get_by_label_contains(l.readonly);

    // Retour à la base : la mention disparaît.
    h.state_mut().editor.set_active_size(None);
    h.run_steps(4);
    assert!(h.query_by_label_contains(l.note_any).is_none());
}

#[test]
fn us095_derivee_fr_inspecteur_et_mesures_evalues_dans_la_taille_active() {
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
    derived_walk(DvLabels {
        note_xl: "Taille XL : position et mesures évaluées pour cette taille (lecture seule)",
        note_any: "évaluées pour cette taille",
        readonly: "Revenir à la taille de base pour modifier.",
    });
}

#[test]
fn us095_derived_en_inspector_and_measures_evaluated_in_the_active_size() {
    pf_i18n::set_lang(pf_i18n::Lang::En);
    derived_walk(DvLabels {
        note_xl: "Size XL: position and measures evaluated for this size (read-only)",
        note_any: "evaluated for this size",
        readonly: "Go back to the base size to edit.",
    });
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
}

// ===== US-09.6a : mesures par taille ==============================================================

/// Libellés d'une langue pour le parcours « Mesures par taille ».
struct SmLabels {
    view_menu: &'static str,
    menu_item: &'static str,
    title: &'static str,
    readonly: &'static str,
    summary: &'static str,
    pieces: &'static str,
    seams: &'static str,
    paths: &'static str,
    perimeter_row: &'static str,
    alert: &'static str,
    copy: &'static str,
    copied: &'static str,
    csv_head: &'static str,
    csv_row: &'static str,
    other_range: &'static str,
    close: &'static str,
}

fn size_measures_walk(l: SmLabels) {
    use pf_core::Document;
    let (project, range) = crate::stages::mesures_tailles_project().unwrap();
    let before = project.clone();
    let sheet = project.sheets[0].id;
    // Le repère est le dernier tracé de la feuille : ni côté de pièce, ni lié à une couture.
    let mark = project
        .sheet(sheet)
        .unwrap()
        .entities
        .last()
        .map(|e| e.id)
        .unwrap();
    let mark_name = crate::text::entity_name(project.sheet(sheet).unwrap(), mark);
    let mut h = harness();
    h.state_mut().editor = crate::editor::Editor::new(Document::new(project));
    h.state_mut().fit_pending = true;
    h.run_steps(4);

    // Ouverture par le menu Affichage (aucun raccourci) ; fenêtre dans le dock, canevas préservé.
    h.get_by_label(l.view_menu).click_accesskit();
    h.run_steps(3);
    h.get_by_label(l.menu_item).click_accesskit();
    h.run_steps(4);
    assert!(h.state().size_measures.is_some(), "fenêtre non ouverte");
    assert!(h
        .state()
        .dock
        .open()
        .contains(&crate::dock::ToolWin::SizeMeasures));
    let win = h
        .ctx
        .memory(|m| m.area_rect(crate::dock::ToolWin::SizeMeasures.id()))
        .expect("fenêtre Mesures par taille");
    let dock = h.state().dock.rect;
    assert!(dock.contains_rect(win), "{win:?} hors du dock {dock:?}");
    assert!(!win.intersects(h.state().canvas_rect));
    assert!(h.state().canvas_rect.width() >= 400.0);
    h.get_by_label(l.title);
    h.get_by_label(l.readonly);
    h.get_by_label(l.summary);

    // Aucune section dépliée au départ : les en-têtes sont là (avec le compte d'alertes), les
    // lignes non.
    h.get_by_label(l.pieces);
    h.get_by_label(l.seams);
    assert!(h.query_by_label_contains(l.perimeter_row).is_none());
    assert!(h.query_by_label_contains(l.alert).is_none());

    // Dépliage des tracés : l'alerte d'irrégularité est écrite (texte) en plus de l'icône dessinée.
    h.get_by_label(l.paths).click_accesskit();
    h.run_steps(3);
    h.get_by_label_contains(l.alert);
    // Clic sur une ligne = sélection de l'objet.
    h.get_by_label(&mark_name).click_accesskit();
    h.run_steps(3);
    assert_eq!(h.state().editor.selection.entities, vec![mark]);

    // Dépliage des pièces : un clic sur le périmètre sélectionne le contour de la pièce.
    h.get_by_label(l.pieces).click_accesskit();
    h.run_steps(3);
    h.get_by_label(l.perimeter_row).click_accesskit();
    h.run_steps(3);
    assert_eq!(h.state().editor.selection.entities.len(), 4);

    // Copie en CSV : texte dans le presse-papiers, confirmation écrite, aucun fichier.
    h.get_by_label(l.copy).click_accesskit();
    h.step();
    let copied = h.output().platform_output.commands.iter().any(|c| {
        matches!(c, egui::OutputCommand::CopyText(t) if t.starts_with(l.csv_head)
            && t.lines().count() == 1 + 17)
    });
    assert!(copied, "le texte CSV n'a pas été confié au presse-papiers");
    h.run_steps(3);
    h.get_by_label_contains(l.copied);
    let table = h.state().editor.project().size_measures(range).unwrap();
    let text = crate::size_measures::csv(h.state().editor.project(), &table);
    assert_eq!(text.lines().next().unwrap(), l.csv_head);
    assert!(
        text.lines().any(|x| x == l.csv_row),
        "ligne CSV absente : {text}"
    );
    assert_eq!(text.lines().count(), 1 + 17);

    // Une seconde gamme : un sélecteur apparaît, une gamme à la fois.
    let added = h
        .state_mut()
        .editor
        .exec(pf_core::Command::AddSizeRange {
            name: l.other_range.into(),
            kind: pf_core::SizeKind::Alphanumeric,
            labels: ["P", "M", "G"].map(String::from).to_vec(),
            base: 1,
        })
        .is_some();
    assert!(added);
    h.run_steps(4);
    h.get_by_label(l.other_range).click_accesskit();
    h.run_steps(4);
    let other = h.state().editor.project().sizes[1].id;
    assert_eq!(h.state().size_measures.as_ref().unwrap().range, Some(other));

    // Lecture seule : rien n'a changé dans le projet (hors la gamme ajoutée par le test).
    let mut expected = before;
    expected.sizes = h.state().editor.project().sizes.clone();
    expected.next_id = h.state().editor.project().next_id;
    assert!(*h.state().editor.project() == expected, "projet modifié");

    // Fermeture.
    h.get_by_label(l.close).click_accesskit();
    h.run_steps(3);
    assert!(h.state().size_measures.is_none());
}

#[test]
fn us096_fr_fenetre_mesures_par_taille_tableau_alerte_selection_et_csv() {
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
    size_measures_walk(SmLabels {
        view_menu: "Affichage",
        menu_item: "Mesures par taille…",
        title: "Mesures par taille",
        readonly: "Lecture seule : rien n'est modifié ni enregistré.",
        summary: "17 mesure(s) · 1 évolution(s) irrégulière(s).",
        pieces: "Pièces (5)",
        seams: "Coutures (3)",
        paths: "Tracés (9) · 1 alerte(s)",
        perimeter_row: "Périmètre · Devant",
        alert: "Irrégulier : pas à plus de 50 % de la médiane (S-M)",
        copy: "Copier en CSV",
        copied: "17 ligne(s) copiée(s) dans le presse-papiers",
        csv_head: "Mesure;Unité;XS;S;M;L;XL;Pas moyen;Alerte",
        csv_row: "Périmètre · Devant;mm;1960.00;1980.00;2000.00;2020.00;2040.00;20.00;",
        other_range: "Homme",
        close: "Fermer",
    });
}

#[test]
fn us096_en_measures_by_size_window_table_alert_selection_and_csv() {
    pf_i18n::set_lang(pf_i18n::Lang::En);
    size_measures_walk(SmLabels {
        view_menu: "View",
        menu_item: "Measures by size...",
        title: "Measures by size",
        readonly: "Read-only: nothing is modified or saved.",
        summary: "17 measure(s) · 1 irregular trend(s).",
        pieces: "Pieces (5)",
        seams: "Seams (3)",
        paths: "Paths (9) · 1 alert(s)",
        perimeter_row: "Perimeter · Devant",
        alert: "Irregular: step more than 50 % from the median (S-M)",
        copy: "Copy as CSV",
        copied: "17 row(s) copied to the clipboard",
        csv_head: "Measure;Unit;XS;S;M;L;XL;Mean step;Alert",
        csv_row: "Perimeter · Devant;mm;1960.00;1980.00;2000.00;2020.00;2040.00;20.00;",
        other_range: "Men",
        close: "Close",
    });
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
}

// ===== US-09.4a : nid de tailles ==================================================================

/// Libellés d'une langue pour le parcours « Nid de tailles ».
struct NvLabels {
    view_menu: &'static str,
    show_item: &'static str,
    menu_item: &'static str,
    title: &'static str,
    readonly: &'static str,
    show_box: &'static str,
    /// Motifs des tailles autres que la base (rotation).
    patterns: [&'static str; 4],
    base_pattern: &'static str,
    origin: &'static str,
    close: &'static str,
}

fn size_nest_walk(l: NvLabels) {
    use pf_core::{Document, NestAnchor};
    let (project, _anchor_c, range) = crate::stages::nid_tailles_project(5).unwrap();
    let before = project.clone();
    let sheet = project.sheets[0].id;
    // Points de la feuille par libellé (A, B, C… dans l'ordre du polygone).
    let point_b = project.sheet(sheet).unwrap().points[1].id;
    let b_label = project.sheet(sheet).unwrap().label(point_b).to_string();
    let mut h = harness();
    h.state_mut().editor = crate::editor::Editor::new(Document::new(project));
    h.state_mut().fit_pending = true;
    h.run_steps(4);
    assert!(
        !h.state().nest.enabled,
        "le nid n'est pas affiché au départ"
    );

    // Case du menu Affichage : affiche le nid sans ouvrir de fenêtre.
    h.get_by_label(l.view_menu).click_accesskit();
    h.run_steps(3);
    h.get_by_label(l.show_item).click_accesskit();
    h.run_steps(4);
    assert!(h.state().nest.enabled);
    assert!(h.state().size_nest.is_none());
    assert_eq!(h.state().nest.nest().map(|n| n.sizes.len()), Some(5));

    // Fenêtre par le menu Affichage (aucun raccourci) ; dans le dock, canevas préservé.
    h.get_by_label(l.view_menu).click_accesskit();
    h.run_steps(3);
    h.get_by_label(l.menu_item).click_accesskit();
    h.run_steps(4);
    assert!(h.state().size_nest.is_some(), "fenêtre non ouverte");
    assert!(h
        .state()
        .dock
        .open()
        .contains(&crate::dock::ToolWin::SizeNest));
    let win = h
        .ctx
        .memory(|m| m.area_rect(crate::dock::ToolWin::SizeNest.id()))
        .expect("fenêtre Nid de tailles");
    let dock = h.state().dock.rect;
    assert!(dock.contains_rect(win), "{win:?} hors du dock {dock:?}");
    assert!(!win.intersects(h.state().canvas_rect));
    assert!(h.state().canvas_rect.width() >= 400.0);
    h.get_by_label(l.title);
    h.get_by_label(l.readonly);

    // Le nid est calculé : cinq tailles, origine de la feuille par défaut.
    let nest = h.state().nest.nest().expect("nid calculé").clone();
    assert_eq!(nest.sizes.len(), 5);
    assert_eq!(nest.range, range);
    assert_eq!(nest.anchor, NestAnchor::Origin);

    // Légende en texte : nom de la taille + nom du motif ; la base en plein, marquée « base ».
    for text in l.patterns {
        h.get_by_label(text);
    }
    h.get_by_label(l.base_pattern);
    for label in ["34", "36", "38", "40", "42"] {
        h.get_by_label(label);
    }

    // Masquer une taille : décochée dans la légende, toujours dans le nid lu (affichage seul).
    let hidden_id = nest.sizes[1].size;
    h.get_by_label("36").click_accesskit();
    h.run_steps(3);
    assert_eq!(
        h.state().nest.hidden.iter().copied().collect::<Vec<_>>(),
        vec![hidden_id]
    );
    assert_eq!(h.state().nest.nest().unwrap().sizes.len(), 5);
    h.get_by_label("36").click_accesskit();
    h.run_steps(3);
    assert!(h.state().nest.hidden.is_empty());

    // Point d'empilement choisi dans la liste : B, hors de l'origine de la feuille.
    h.get_by_role(egui::accesskit::Role::ComboBox).click();
    h.run_steps(3);
    h.get_by_label(&b_label).click_accesskit();
    h.run_steps(8);
    assert_eq!(h.state().nest.anchor, Some(point_b));
    let nest = h.state().nest.nest().expect("nid recalculé").clone();
    assert_eq!(nest.anchor, NestAnchor::Point(point_b));
    assert_eq!(nest.anchor_position, pt(400.0, 0.0));
    // B bouge de 4 k mm : 42 est la base (k = 0), 34 est à k = −4, donc B = 384 ; ramené à 400.
    let first = &nest.sizes[0];
    assert_eq!(first.anchor_position, pt(384.0, 0.0));
    assert_eq!(first.shift, (16.0, 0.0));
    // Retour à l'origine de la feuille.
    if h.query_by_label(l.origin).is_none() {
        h.get_by_role(egui::accesskit::Role::ComboBox)
            .click_accesskit();
    }
    h.run_steps(3);
    h.get_by_label(l.origin).click_accesskit();
    h.run_steps(4);
    assert_eq!(h.state().nest.anchor, None);

    // La case de la fenêtre retire le nid du canevas ; la lecture seule n'a rien modifié.
    h.get_by_label(l.show_box).click_accesskit();
    h.run_steps(3);
    assert!(!h.state().nest.enabled);
    assert!(*h.state().editor.project() == before, "projet modifié");

    // Fermeture : plus de fenêtre, le nid (non affiché) n'est plus calculé.
    h.get_by_label(l.close).click_accesskit();
    h.run_steps(3);
    assert!(h.state().size_nest.is_none());
    assert!(h.state().nest.nest().is_none());
}

#[test]
fn us094_fr_nid_de_tailles_menu_legende_masquer_ancre_et_lecture_seule() {
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
    size_nest_walk(NvLabels {
        view_menu: "Affichage",
        show_item: "Afficher le nid de tailles",
        menu_item: "Nid de tailles…",
        title: "Nid de tailles",
        readonly: "Lecture seule : rien n'est modifié ni enregistré.",
        show_box: "Dessiner le nid sur le canevas",
        patterns: ["tirets", "pointillés", "tiret-point", "longs tirets"],
        base_pattern: "trait plein épais · base",
        origin: "Origine de la feuille",
        close: "Fermer",
    });
}

#[test]
fn us094_en_size_nest_menu_legend_hide_anchor_and_read_only() {
    pf_i18n::set_lang(pf_i18n::Lang::En);
    size_nest_walk(NvLabels {
        view_menu: "View",
        show_item: "Show size nest",
        menu_item: "Size nest...",
        title: "Size nest",
        readonly: "Read-only: nothing is modified or saved.",
        show_box: "Draw the nest on the canvas",
        patterns: ["dashes", "dots", "dash-dot", "long dashes"],
        base_pattern: "thick solid line · base",
        origin: "Sheet origin",
        close: "Close",
    });
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
}

#[test]
fn us094_vingt_tailles_legende_complete_et_etiquettes_toujours_presentes() {
    use pf_core::Document;
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
    let (project, anchor, range) = crate::stages::nid_tailles_project(20).unwrap();
    let before = project.clone();
    let mut h = harness();
    h.state_mut().editor = crate::editor::Editor::new(Document::new(project));
    h.state_mut().nest.enabled = true;
    h.state_mut().nest.range = Some(range);
    h.state_mut().nest.anchor = Some(anchor);
    h.state_mut().open_size_nest();
    h.state_mut().fit_pending = true;
    h.run_steps(5);
    let nest = h.state().nest.nest().expect("nid à vingt tailles").clone();
    assert_eq!(nest.sizes.len(), 20);
    // Les vingt tailles sont dans la légende (texte), chacune avec un motif nommé ; le plan
    // garde son espace.
    for i in 0..20 {
        h.get_by_label(&(34 + 2 * i).to_string());
    }
    assert_eq!(h.get_all_by_label("trait plein épais · base").count(), 1);
    assert!(h.get_all_by_label_contains("tirets").count() >= 6);
    assert!(h.state().canvas_rect.width() >= 400.0);
    // Chaque taille a un point d'étiquette (au plus haut de son contour) : rien n'est vide.
    assert!(nest.sizes.iter().all(|s| s.top_point().is_some()));
    // Le nid est une lecture : le projet n'a pas bougé.
    assert!(*h.state().editor.project() == before, "projet modifié");
}

// ===== US-09.9 : contrôle de forme ================================================================

/// Libellés d'une langue pour le parcours « Contrôle de forme ».
struct ScLabels {
    view_menu: &'static str,
    menu_item: &'static str,
    title: &'static str,
    readonly: &'static str,
    summary: &'static str,
    summary_clean: &'static str,
    verdict_ok: &'static str,
    verdict_three: &'static str,
    signals_header: &'static str,
    table_header: &'static str,
    cusp_text: &'static str,
    inflection_text: &'static str,
    col_cusps: &'static str,
    other_range: &'static str,
    no_curves: &'static str,
    close: &'static str,
}

fn shape_control_walk(l: ScLabels) {
    use pf_core::Document;
    let (project, _range) = crate::stages::controle_forme_project().unwrap();
    let before = project.clone();
    let sheet = project.sheets[0].id;
    let curves: Vec<pf_core::EntityId> = project
        .sheet(sheet)
        .unwrap()
        .entities
        .iter()
        .map(|e| e.id)
        .collect();
    assert_eq!(curves.len(), 3);
    let first_name = crate::text::entity_name(project.sheet(sheet).unwrap(), curves[0]);
    let mut h = harness();
    h.state_mut().editor = crate::editor::Editor::new(Document::new(project));
    h.state_mut().fit_pending = true;
    h.run_steps(4);

    // Ouverture par le menu Affichage (aucun raccourci) ; fenêtre dans le dock, canevas préservé.
    h.get_by_label(l.view_menu).click_accesskit();
    h.run_steps(3);
    h.get_by_label(l.menu_item).click_accesskit();
    h.run_steps(4);
    assert!(h.state().shape_control.is_some(), "fenêtre non ouverte");
    assert!(h
        .state()
        .dock
        .open()
        .contains(&crate::dock::ToolWin::ShapeControl));
    let win = h
        .ctx
        .memory(|m| m.area_rect(crate::dock::ToolWin::ShapeControl.id()))
        .expect("fenêtre Contrôle de forme");
    let dock = h.state().dock.rect;
    assert!(dock.contains_rect(win), "{win:?} hors du dock {dock:?}");
    assert!(!win.intersects(h.state().canvas_rect));
    assert!(h.state().canvas_rect.width() >= 400.0);
    h.get_by_label(l.title);
    h.get_by_label(l.readonly);
    h.get_by_label(l.summary);

    // Liste des courbes avec leur verdict (texte, en plus de l'icône dessinée) ; sections
    // repliées au départ : en-têtes présents, détails absents.
    h.get_by_label_contains(l.verdict_three);
    h.get_by_label_contains(l.verdict_ok);
    h.get_by_label(l.signals_header);
    h.get_by_label(l.table_header);
    assert!(h.query_by_label_contains(l.cusp_text).is_none());
    assert!(h.query_by_label(l.col_cusps).is_none());

    // Clic sur une courbe de la liste = sélection du tracé.
    h.get_by_label(&first_name).click_accesskit();
    h.run_steps(3);
    assert_eq!(h.state().editor.selection.entities, vec![curves[0]]);
    h.state_mut().editor.selection.clear();

    // Signalements : le texte de chaque type est écrit ; un clic sélectionne la courbe.
    h.get_by_label(l.signals_header).click_accesskit();
    h.run_steps(3);
    h.get_by_label_contains(l.inflection_text);
    h.get_by_label_contains(l.cusp_text).click_accesskit();
    h.run_steps(3);
    assert_eq!(h.state().editor.selection.entities, vec![curves[0]]);

    // Tableau courbe × taille : une table par courbe.
    h.get_by_label(l.table_header).click_accesskit();
    h.run_steps(3);
    assert_eq!(h.get_all_by_label(l.col_cusps).count(), 3);

    // Une seconde gamme (sans écart) : un sélecteur apparaît, plus aucun signalement.
    let added = h
        .state_mut()
        .editor
        .exec(pf_core::Command::AddSizeRange {
            name: l.other_range.into(),
            kind: pf_core::SizeKind::Alphanumeric,
            labels: ["P", "M", "G"].map(String::from).to_vec(),
            base: 1,
        })
        .is_some();
    assert!(added);
    h.run_steps(4);
    h.get_by_label(l.other_range).click_accesskit();
    h.run_steps(4);
    let other = h.state().editor.project().sizes[1].id;
    assert_eq!(h.state().shape_control.as_ref().unwrap().range, Some(other));
    h.get_by_label(l.summary_clean);

    // Lecture seule : rien n'a changé dans le projet (hors la gamme ajoutée par le test).
    let mut expected = before;
    expected.sizes = h.state().editor.project().sizes.clone();
    expected.next_id = h.state().editor.project().next_id;
    assert!(*h.state().editor.project() == expected, "projet modifié");

    // Fermeture.
    h.get_by_label(l.close).click_accesskit();
    h.run_steps(3);
    assert!(h.state().shape_control.is_none());

    // Aucune courbe : message clair.
    let mut bare = pf_core::Project::new("Sans courbe");
    bare.apply(pf_core::Command::AddSizeRange {
        name: "G".into(),
        kind: pf_core::SizeKind::Alphanumeric,
        labels: ["S", "M"].map(String::from).to_vec(),
        base: 0,
    })
    .unwrap();
    let mut h = harness();
    h.state_mut().editor = crate::editor::Editor::new(Document::new(bare));
    h.state_mut().open_shape_control();
    h.run_steps(4);
    h.get_by_label_contains(l.no_curves);
}

#[test]
fn us099_fr_controle_de_forme_liste_signalements_tableau_et_selection() {
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
    shape_control_walk(ScLabels {
        view_menu: "Affichage",
        menu_item: "Contrôle de forme…",
        title: "Contrôle de forme",
        readonly: "Lecture seule : rien n'est modifié ni enregistré.",
        summary: "3 courbe(s) · 2 signalée(s).",
        summary_clean: "3 courbe(s) · 0 signalée(s).",
        verdict_ok: "Aucun signalement",
        verdict_three: "3 signalement(s)",
        signals_header: "Signalements (5)",
        table_header: "Courbe × taille (3)",
        cusp_text: "XL : rebroussement introduit (1), absent en M",
        inflection_text: "XL : 1 inflexion(s) au lieu de 0 en M",
        col_cusps: "Rebr.",
        other_range: "Homme",
        no_curves: "Aucune courbe sur cette feuille",
        close: "Fermer",
    });
}

#[test]
fn us099_en_shape_check_list_flags_table_and_selection() {
    pf_i18n::set_lang(pf_i18n::Lang::En);
    shape_control_walk(ScLabels {
        view_menu: "View",
        menu_item: "Shape check...",
        title: "Shape check",
        readonly: "Read-only: nothing is modified or saved.",
        summary: "3 curve(s) · 2 flagged.",
        summary_clean: "3 curve(s) · 0 flagged.",
        verdict_ok: "No flag",
        verdict_three: "3 flag(s)",
        signals_header: "Flags (5)",
        table_header: "Curve × size (3)",
        cusp_text: "XL: cusp introduced (1), none in M",
        inflection_text: "XL: 1 inflection(s) instead of 0 in M",
        col_cusps: "Cusps",
        other_range: "Men",
        no_curves: "No curve on this sheet",
        close: "Close",
    });
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
}

// ===== US-09.8 : export gradué ====================================================================

/// Libellés d'une langue pour le parcours « Export gradué ».
struct EsLabels {
    file_menu: &'static str,
    print_item: &'static str,
    sizes_item: &'static str,
    view_menu: &'static str,
    nest_item: &'static str,
    mention_base: &'static str,
    mention_38: &'static str,
    mention_36: &'static str,
    base_text: &'static str,
    title_38: &'static str,
    count_three: &'static str,
    failed_one: &'static str,
    done_prefix: &'static str,
    nest_svg: &'static str,
    nest_pdf: &'static str,
    none_selected: &'static str,
    run: &'static str,
}

fn export_gradue_walk(l: EsLabels) {
    use pf_core::Document;
    use pf_export::size_export::SizeFormat;
    use std::path::PathBuf;
    let (project, _anchor, range) = crate::stages::nid_tailles_project(5).unwrap();
    let before = project.clone();
    let sheet = project.sheets[0].id;
    // Gamme 34 36 38 40 42, base 42 ; écart de B = 4 × (rang − base) : B(38) = (392 ; 0).
    let ids: Vec<pf_core::SizeId> = project
        .size_range(range)
        .unwrap()
        .sizes
        .iter()
        .map(|s| s.id)
        .collect();
    let (id36, id38, id42) = (ids[1], ids[2], ids[4]);
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!(
        "../../target/us098-export-{}",
        pf_i18n::lang().code()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut h = harness();
    h.state_mut().editor = crate::editor::Editor::new(Document::new(project));
    h.state_mut().fit_pending = true;
    h.run_steps(4);

    // Fenêtre de mise en page du PDF : la taille de base est exportée par défaut.
    h.get_by_label(l.file_menu).click_accesskit();
    h.run_steps(3);
    h.get_by_label(l.print_item).click_accesskit();
    h.run_steps(4);
    h.get_by_label(l.mention_base);
    assert_eq!(h.state().export_size, None);
    assert_eq!(h.state().effective_export_size(), Some(id42));
    h.get_by_label(l.base_text);
    // Le sélecteur liste toutes les tailles ; un clic choisit la taille 38.
    h.get_by_label("38").click_accesskit();
    h.run_steps(4);
    assert_eq!(h.state().export_size, Some(id38));
    h.get_by_label(l.mention_38);
    let scene = h.state().export_scene(sheet).unwrap();
    assert!(
        scene
            .points
            .iter()
            .any(|p| (p.pos.x - 392.0).abs() < 1e-9 && p.pos.y.abs() < 1e-9),
        "B de la taille 38 en (392 ; 0)"
    );
    assert!(
        scene.sheet_name.starts_with(l.title_38),
        "{}",
        scene.sheet_name
    );
    assert_eq!(h.state().export_stem("Patron"), "Patron-38");
    // Retour à la base : mêmes octets qu'avant, aucun nom de taille.
    h.get_by_label(l.base_text).click_accesskit();
    h.run_steps(4);
    assert_eq!(h.state().export_stem("Patron"), "Patron");
    let plain = pf_export::Scene::from_sheet(h.state().editor.project(), sheet).unwrap();
    assert_eq!(h.state().export_scene(sheet).unwrap(), plain);
    h.state_mut().print_dialog = None;
    h.run_steps(2);

    // Fenêtre HPGL : même sélecteur, le tracé est recalculé pour la taille choisie.
    h.state_mut().export_size = Some(id38);
    assert!(h.state_mut().open_hpgl_export());
    h.run_steps(4);
    h.get_by_label(l.mention_38);
    h.get_by_label("36").click_accesskit();
    h.run_steps(4);
    assert_eq!(h.state().export_size, Some(id36));
    h.get_by_label(l.mention_36);
    let d = h.state().hpgl_dialog.as_ref().expect("fenêtre HPGL");
    assert!(d.scene.sheet_name.contains("36"), "{}", d.scene.sheet_name);
    h.state_mut().hpgl_dialog = None;
    h.state_mut().export_size = None;
    h.run_steps(2);

    // Sélection de tailles : menu Fichier, cases à cocher, un fichier par taille.
    h.get_by_label(l.file_menu).click_accesskit();
    h.run_steps(3);
    h.get_by_label(l.sizes_item).click_accesskit();
    h.run_steps(4);
    assert!(h.state().export_sizes.is_some(), "fenêtre non ouverte");
    assert!(h
        .state()
        .dock
        .open()
        .contains(&crate::dock::ToolWin::ExportSizes));
    let win = h
        .ctx
        .memory(|m| m.area_rect(crate::dock::ToolWin::ExportSizes.id()))
        .expect("fenêtre Exporter des tailles");
    assert!(
        h.state().dock.rect.contains_rect(win),
        "{win:?} hors du dock"
    );
    assert!(!win.intersects(h.state().canvas_rect));
    h.get_by_label(l.none_selected);
    h.get_by_label(l.run);
    for label in ["34", "38", l.base_text] {
        h.get_by_label(label).click_accesskit();
        h.run_steps(3);
    }
    h.get_by_label(l.count_three);
    // Un dossier du nom du fichier de 38 : cette taille échoue, les autres sont écrites.
    std::fs::create_dir(dir.join("Size nest-38.pdf")).unwrap();
    assert!(!h.state_mut().export_sizes_to(&dir));
    h.run_steps(4);
    assert!(h.query_all_by_label_contains(l.done_prefix).count() >= 1);
    h.get_by_label(l.failed_one);
    assert!(h.query_all_by_label_contains("Size nest-38.pdf").count() >= 1);
    let mut found: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    found.sort();
    assert_eq!(
        found,
        ["Size nest-34.pdf", "Size nest-38.pdf", "Size nest-42.pdf"]
    );
    assert!(std::fs::read(dir.join("Size nest-34.pdf"))
        .unwrap()
        .starts_with(b"%PDF"));
    // 42 est la base : mêmes octets que l'export de base.
    let base_scenes = [h.state().export_scene(sheet).unwrap()];
    let (page_setup, content) = (h.state().page_setup, h.state().output_content);
    let in_lang = |lang| {
        pf_export::pdf::export_pdf_lang(&base_scenes, &page_setup, &content, lang)
            .unwrap()
            .bytes
    };
    let written = std::fs::read(dir.join("Size nest-42.pdf")).unwrap();
    // US-19.2 : les textes de l'export suivent la langue de l'interface.
    assert_eq!(written, in_lang(pf_i18n::lang()));
    assert_ne!(in_lang(pf_i18n::Lang::Fr), in_lang(pf_i18n::Lang::En));
    h.state_mut().export_sizes = None;
    h.run_steps(2);

    // Nid : tailles cochées de la légende seulement, SVG et PDF.
    h.get_by_label(l.view_menu).click_accesskit();
    h.run_steps(3);
    h.get_by_label(l.nest_item).click_accesskit();
    h.run_steps(4);
    h.get_by_label(l.nest_pdf);
    h.get_by_label(l.nest_svg);
    h.get_by_label("36").click_accesskit();
    h.run_steps(3);
    assert!(h.state().nest.hidden.contains(&id36));
    let svg_path = dir.join("nid.svg");
    assert!(h.state_mut().export_nest_to(SizeFormat::Svg, &svg_path));
    let text = std::fs::read_to_string(&svg_path).unwrap();
    for shown in ["34", "38", "40"] {
        assert!(text.contains(&format!(">{shown}</text>")), "{shown}");
    }
    assert!(!text.contains(">36</text>"), "taille masquée exportée");
    assert!(text.contains(&format!(">{}</text>", l.base_text)));
    let pdf_path = dir.join("nid.pdf");
    assert!(h.state_mut().export_nest_to(SizeFormat::Pdf, &pdf_path));
    assert!(std::fs::read(&pdf_path).unwrap().starts_with(b"%PDF"));
    // Aucun export n'a modifié le projet.
    assert!(*h.state().editor.project() == before, "projet modifié");
}

#[test]
fn us098_fr_export_gradue_selecteur_selection_de_tailles_et_nid() {
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
    export_gradue_walk(EsLabels {
        file_menu: "Fichier",
        print_item: "Mise en page du PDF…",
        sizes_item: "Exporter des tailles…",
        view_menu: "Affichage",
        nest_item: "Nid de tailles…",
        mention_base: "Export : taille 42",
        mention_38: "Export : taille 38",
        mention_36: "Export : taille 36",
        base_text: "42 (base)",
        title_38: "Taille 38 · ",
        count_three: "3 taille(s) cochée(s)",
        failed_one: "1 taille(s) non exportée(s) :",
        done_prefix: "2 fichier(s) écrit(s) dans",
        nest_svg: "Exporter le nid en SVG…",
        nest_pdf: "Exporter le nid en PDF…",
        none_selected: "0 taille(s) cochée(s)",
        run: "Exporter dans un dossier…",
    });
}

#[test]
fn us098_en_graded_export_selector_size_selection_and_nest() {
    pf_i18n::set_lang(pf_i18n::Lang::En);
    export_gradue_walk(EsLabels {
        file_menu: "File",
        print_item: "PDF page setup…",
        sizes_item: "Export sizes…",
        view_menu: "View",
        nest_item: "Size nest...",
        mention_base: "Export: size 42",
        mention_38: "Export: size 38",
        mention_36: "Export: size 36",
        base_text: "42 (base)",
        title_38: "Size 38 · ",
        count_three: "3 size(s) checked",
        failed_one: "1 size(s) not exported:",
        done_prefix: "2 file(s) written to",
        nest_svg: "Export the nest as SVG…",
        nest_pdf: "Export the nest as PDF…",
        none_selected: "0 size(s) checked",
        run: "Export to a folder…",
    });
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
}

// ===== US-09.8b : export des mesures par taille (CSV, PDF) ========================================

/// Libellés d'une langue pour le parcours « Exporter les mesures par taille ».
struct MxLabels {
    view_menu: &'static str,
    menu_item: &'static str,
    readonly: &'static str,
    copy: &'static str,
    export_csv: &'static str,
    export_pdf: &'static str,
    export_note: &'static str,
    csv_head: &'static str,
    csv_row: &'static str,
    csv_done: &'static str,
    pdf_done: &'static str,
    pdf_title: &'static str,
}

fn size_measures_export_walk(l: MxLabels) {
    use pf_core::Document;
    use pf_export::report::ReportFormat;
    use std::path::PathBuf;
    let (project, range) = crate::stages::mesures_tailles_project().unwrap();
    let before = pf_core::persist::to_json(&project).unwrap();
    let range_name = project.size_range(range).unwrap().name.clone();
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!(
        "../../target/us098b-mesures-{}",
        pf_i18n::lang().code()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut h = harness();
    h.state_mut().editor = crate::editor::Editor::new(Document::new(project));
    h.state_mut().fit_pending = true;
    h.run_steps(4);

    // Fenêtre « Mesures par taille » : « Copier en CSV » reste, deux boutons d'export, mention de
    // lecture seule et de ce que l'export écrit.
    h.get_by_label(l.view_menu).click_accesskit();
    h.run_steps(3);
    h.get_by_label(l.menu_item).click_accesskit();
    h.run_steps(4);
    h.get_by_label(l.readonly);
    h.get_by_label(l.copy);
    h.get_by_label(l.export_csv);
    h.get_by_label(l.export_pdf);
    h.get_by_label(l.export_note);
    let win = h
        .ctx
        .memory(|m| m.area_rect(crate::dock::ToolWin::SizeMeasures.id()))
        .expect("fenêtre Mesures par taille");
    assert!(
        h.state().dock.rect.contains_rect(win),
        "{win:?} hors du dock"
    );
    assert!(!win.intersects(h.state().canvas_rect));

    // Export CSV : seul le fichier choisi est écrit, avec l'en-tête et 17 lignes de mesures.
    let csv_path = dir.join("mesures.csv");
    assert!(h
        .state_mut()
        .write_size_measures_to(range, ReportFormat::Csv, &csv_path));
    h.run_steps(3);
    let bytes = std::fs::read(&csv_path).unwrap();
    let text = String::from_utf8(bytes.clone()).unwrap();
    assert!(text.starts_with('\u{FEFF}'));
    let lines: Vec<&str> = text.trim_start_matches('\u{FEFF}').lines().collect();
    assert!(lines.contains(&l.csv_head), "{text}");
    assert!(lines.contains(&l.csv_row), "{text}");
    // 9 lignes de synthèse (en-tête compris), 1 en-tête de tableau, 17 mesures.
    assert_eq!(lines.len(), 9 + 1 + 17);
    // Mêmes octets que l'export du module (aucune seconde mise en forme dans l'interface).
    let direct = pf_export::size_measures_export::export_size_measures(
        h.state().editor.project(),
        range,
        ReportFormat::Csv,
        pf_i18n::lang(),
    )
    .unwrap();
    assert_eq!(bytes, direct.bytes);
    let status = h.state().editor.status.text.clone();
    assert!(
        status.starts_with(&format!("{} ({}", l.csv_done, 17)) || status.contains(l.csv_done),
        "{status}"
    );
    assert!(status.contains(&range_name), "{status}");
    assert!(!h.state().editor.status.error);

    // Export PDF : un PDF, mêmes octets que l'export du module.
    let pdf_path = dir.join("mesures.pdf");
    assert!(h
        .state_mut()
        .write_size_measures_to(range, ReportFormat::Pdf, &pdf_path));
    let pdf = std::fs::read(&pdf_path).unwrap();
    assert!(pdf.starts_with(b"%PDF"));
    let direct = pf_export::size_measures_export::export_size_measures(
        h.state().editor.project(),
        range,
        ReportFormat::Pdf,
        pf_i18n::lang(),
    )
    .unwrap();
    assert_eq!(pdf, direct.bytes);
    assert!(h.state().editor.status.text.contains(l.pdf_done));
    let _ = l.pdf_title;

    // Seuls les deux fichiers choisis existent ; le projet n'a pas bougé d'un octet.
    let mut found: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    found.sort();
    assert_eq!(found, ["mesures.csv", "mesures.pdf"]);
    assert_eq!(
        pf_core::persist::to_json(h.state().editor.project()).unwrap(),
        before,
        "projet modifié"
    );

    // Destination illisible : erreur propre, aucun fichier, projet inchangé.
    assert!(h.state().error_dialog.is_none());
    assert!(!h.state_mut().write_size_measures_to(
        range,
        ReportFormat::Csv,
        &dir.join("absent").join("x.csv")
    ));
    assert!(h.state().error_dialog.is_some());
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 2);
    // Gamme disparue : refus aussi.
    h.state_mut().error_dialog = None;
    assert!(!h.state_mut().write_size_measures_to(
        pf_core::SizeRangeId(9999),
        ReportFormat::Csv,
        &dir.join("inconnue.csv")
    ));
    assert!(h.state().error_dialog.is_some());
    assert!(!dir.join("inconnue.csv").exists());
}

#[test]
fn us098b_fr_mesures_par_taille_export_csv_et_pdf() {
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
    size_measures_export_walk(MxLabels {
        view_menu: "Affichage",
        menu_item: "Mesures par taille…",
        readonly: "Lecture seule : rien n'est modifié ni enregistré.",
        copy: "Copier en CSV",
        export_csv: "Exporter en CSV…",
        export_pdf: "Exporter en PDF…",
        export_note: "Export : le projet n'est pas modifié, seul le fichier choisi est écrit.",
        csv_head: "#mesure;Mesure;Unité;XS;S;M;L;XL;Pas moyen;Pas moyen (%);Alerte",
        csv_row: "mesure;Périmètre · Devant;mm;1960.00;1980.00;2000.00;2020.00;2040.00;20.00;1.01;",
        csv_done: "CSV exporté (17 mesure(s), gamme",
        pdf_done: "PDF exporté (17 mesure(s), gamme",
        pdf_title: "Mesures par taille · ",
    });
}

#[test]
fn us098b_en_measures_by_size_export_csv_and_pdf() {
    pf_i18n::set_lang(pf_i18n::Lang::En);
    size_measures_export_walk(MxLabels {
        view_menu: "View",
        menu_item: "Measures by size...",
        readonly: "Read-only: nothing is modified or saved.",
        copy: "Copy as CSV",
        export_csv: "Export as CSV…",
        export_pdf: "Export as PDF…",
        export_note: "Export: the project is not modified, only the chosen file is written.",
        csv_head: "#mesure;Measure;Unit;XS;S;M;L;XL;Mean step;Mean step (%);Alert",
        csv_row: "mesure;Perimeter · Devant;mm;1960.00;1980.00;2000.00;2020.00;2040.00;20.00;1.01;",
        csv_done: "CSV exported (17 measure(s), range",
        pdf_done: "PDF exported (17 measure(s), range",
        pdf_title: "Measures by size · ",
    });
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
}

// ===== US-09.5c / US-09.7 : cran proportionnel et valeurs de variables par taille ===============

/// Étape `taille-valeurs` : rectangle gradué, cran proportionnel choisi, fenêtre « Variables » avec
/// la section « Valeurs par taille » dépliée sur la taille L (variables `ample` et `profond`).
fn size_values_harness() -> (Harness<'static, App>, pf_core::NotchId, pf_core::SizeId) {
    let launch = Launch {
        demo: true,
        stage: Some("taille-valeurs".into()),
        ..Launch::default()
    };
    let app = App::with_context(&egui::Context::default(), launch);
    let mut h = Harness::builder()
        .with_size(egui::vec2(1280.0, 900.0))
        .build_state(|ctx, app: &mut App| app.ui(ctx), app);
    h.run_steps(6);
    let p = h.state().editor.project();
    let notch = p.sheets[0].notches[0].id;
    let l = p
        .sizes
        .iter()
        .flat_map(|r| r.sizes.iter())
        .find(|s| s.label == "L")
        .unwrap()
        .id;
    (h, notch, l)
}

fn toggled(n: &egui_kittest::Node<'_>) -> bool {
    n.accesskit_node().toggled() == Some(egui::accesskit::Toggled::True)
}

#[test]
fn us095c_case_proportionnelle_une_etape_annulable_refus_ancre_et_anglais() {
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
    let (mut h, notch, _) = size_values_harness();
    assert!(the_notch(&h, notch).proportional);
    assert!(toggled(&h.get_by_label("Proportionnel à la longueur")));
    h.get_by_label_contains("garde la même fraction de la longueur");
    let before = h.state().editor.project().clone();
    // Décocher : une étape, le cran redevient à distance fixe.
    h.get_by_label("Proportionnel à la longueur").click();
    h.run_steps(3);
    assert!(!the_notch(&h, notch).proportional);
    assert!(!toggled(&h.get_by_label("Proportionnel à la longueur")));
    // Cocher de nouveau puis annuler deux fois : retour octet pour octet.
    h.get_by_label("Proportionnel à la longueur").click();
    h.run_steps(3);
    assert!(the_notch(&h, notch).proportional);
    undo(&mut h);
    assert!(!the_notch(&h, notch).proportional);
    undo(&mut h);
    assert_eq!(*h.state().editor.project(), before);
    // Un cran ancré : la case est grisée, avec l'explication ; la commande est refusée sans trace.
    {
        let ed = &mut h.state_mut().editor;
        let sheet = ed.sheet;
        ed.exec(pf_core::Command::SetNotchAnchor {
            sheet,
            notch,
            anchor: Some(pf_core::NotchAnchor::Midpoint {}),
        })
        .unwrap();
    }
    h.run_steps(3);
    assert!(!the_notch(&h, notch).proportional);
    assert!(h
        .get_by_label("Proportionnel à la longueur")
        .accesskit_node()
        .is_disabled());
    h.get_by_label_contains("le cran est ancré, il suit déjà son bord");
    let anchored = h.state().editor.project().clone();
    {
        let ed = &mut h.state_mut().editor;
        let sheet = ed.sheet;
        assert!(ed
            .exec(pf_core::Command::SetNotchProportional {
                sheet,
                notch,
                proportional: true,
            })
            .is_none());
    }
    h.run_steps(3);
    assert!(h.state().editor.status.error);
    assert_eq!(*h.state().editor.project(), anchored, "aucune trace");
    assert_eq!(
        the_notch(&h, notch).anchor,
        Some(pf_core::NotchAnchor::Midpoint {})
    );
    // Anglais.
    pf_i18n::set_lang(pf_i18n::Lang::En);
    h.run_steps(4);
    h.get_by_label("Proportional to length");
    h.get_by_label_contains("Unavailable: the notch is anchored");
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
}

#[test]
fn us095c_la_taille_active_hors_base_refuse_la_case_sans_trace() {
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
    let (mut h, notch, l) = size_values_harness();
    h.state_mut().editor.set_active_size(Some(l));
    h.run_steps(3);
    let before = h.state().editor.project().clone();
    let sheet = h.state().editor.sheet;
    assert!(h
        .state_mut()
        .editor
        .exec(pf_core::Command::SetNotchProportional {
            sheet,
            notch,
            proportional: false,
        })
        .is_none());
    assert_eq!(*h.state().editor.project(), before);
    assert!(the_notch(&h, notch).proportional);
}

#[test]
fn us097_valeur_par_taille_saisie_refus_localise_retrait_et_anglais() {
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
    let (mut h, _, l) = size_values_harness();
    // Section dépliée sur L : valeurs lues du cœur (ample 46, profond = 46 / 2 = 23).
    h.get_by_label("Valeurs par taille");
    h.get_by_label_contains("Dans la taille choisie, l'expression saisie remplace");
    h.get_by_label("46,0 mm");
    h.get_by_label("23,0 mm");
    let per_size = |h: &Harness<'_, App>, name: &str| -> Option<String> {
        h.state()
            .editor
            .project()
            .variable_size_expr(name, l)
            .map(String::from)
    };
    assert_eq!(per_size(&h, "ample").as_deref(), Some("46"));
    assert_eq!(per_size(&h, "profond"), None);
    // Champs du dock : ample (nom, expr), profond (nom, expr), ajout (nom, expr), puis les deux
    // champs « par taille » (ample, profond).
    let before = h.state().editor.project().clone();
    var_type(&mut h, 7, "10 + 5");
    assert_eq!(per_size(&h, "profond").as_deref(), Some("10 + 5"));
    h.get_by_label("15,0 mm");
    // Une étape annulable.
    undo(&mut h);
    assert_eq!(per_size(&h, "profond"), None);
    assert_eq!(*h.state().editor.project(), before);
    // Saisie refusée par le cœur (division par zéro dans la taille) : rien n'est modifié, la
    // raison, qui nomme la taille, reste sous la ligne.
    var_type(&mut h, 7, "1 / 0");
    assert_eq!(*h.state().editor.project(), before);
    assert!(h.get_all_by_label_contains("« L »").count() >= 1);
    // Retrait par la croix de la ligne « ample » (3e croix : deux lignes de la table avant).
    let crosses: Vec<_> = h.get_all_by_label("×").collect();
    assert!(crosses.len() >= 4, "{}", crosses.len());
    crosses[2].click();
    h.run_steps(3);
    assert_eq!(per_size(&h, "ample"), None);
    // ample = 40 (base) → profond = 20 dans L : la table de base et celle de L affichent 20,0.
    assert_eq!(h.get_all_by_label("20,0 mm").count(), 2);
    undo(&mut h);
    assert_eq!(per_size(&h, "ample").as_deref(), Some("46"));
    // Anglais.
    pf_i18n::set_lang(pf_i18n::Lang::En);
    h.run_steps(4);
    h.get_by_label("Values per size");
    h.get_by_label_contains("In the chosen size, the typed expression replaces");
    h.get_by_label("46.0 mm");
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
}

// ===== US-10.1 / US-10.2 : barème de mesures et fiches client ===================================

/// Étape `bareme` ou `fiche-client` : projet de démonstration (gamme XS à XL, base M ; Bust,
/// Back length, Shoulder angle ; fiche « Camille » sans angle d'épaule), fenêtre ouverte.
fn bareme_harness(stage: &str) -> Harness<'static, App> {
    let launch = Launch {
        demo: true,
        stage: Some(stage.into()),
        ..Launch::default()
    };
    let app = App::with_context(&egui::Context::default(), launch);
    let mut h = Harness::builder()
        .with_size(egui::vec2(1280.0, 900.0))
        .build_state(|ctx, app: &mut App| app.ui(ctx), app);
    h.run_steps(6);
    h
}

/// Vide un champ du dock (même repérage que `var_type`) puis valide : une case vidée efface.
fn var_clear(h: &mut Harness<'_, App>, index: usize) {
    {
        let dock = h.state().dock.rect;
        let mut fields: Vec<_> = h
            .get_all_by_role(egui::accesskit::Role::TextInput)
            .filter(|f| dock.contains(f.rect().center()))
            .collect();
        fields.sort_by(|a, b| {
            a.rect()
                .min
                .y
                .total_cmp(&b.rect().min.y)
                .then(a.rect().min.x.total_cmp(&b.rect().min.x))
        });
        fields[index].focus();
    }
    h.run_steps(2);
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.run_steps(1);
    key(h, Key::Backspace);
    key(h, Key::Enter);
    h.run_steps(3);
}

fn bareme_value(h: &Harness<'_, App>, measure: &str, size: &str) -> Option<f64> {
    let p = h.state().editor.project();
    let m = p.measurements.iter().find(|m| m.name == measure)?;
    let s = p
        .sizes
        .iter()
        .flat_map(|r| r.sizes.iter())
        .find(|s| s.label == size)?
        .id;
    m.values.iter().find(|v| v.size == s).map(|v| v.value)
}

#[test]
fn us101_fr_bareme_saisie_effacement_refus_ajout_suppression_une_annulation() {
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
    let mut h = bareme_harness("bareme");
    h.get_by_label("Nouvelle mesure :");
    h.get_by_label("Fichier CSV");
    assert_eq!(h.get_all_by_label("Suppr.").count(), 3);
    let before = h.state().editor.project().clone();
    assert_eq!(bareme_value(&h, "Bust", "XS"), Some(800.0));
    assert_eq!(bareme_value(&h, "Shoulder angle", "XS"), None);
    // Champs du dock : une ligne = nom + 5 tailles (XS S M L XL) ; la 3e ligne commence à 12.
    // Saisie dans une case vide : valeur définie (en degrés, virgule décimale), une annulation.
    var_type(&mut h, 13, "19,5");
    assert_eq!(bareme_value(&h, "Shoulder angle", "XS"), Some(19.5));
    assert!(!h.state().editor.status.error);
    undo(&mut h);
    assert_eq!(*h.state().editor.project(), before, "une seule annulation");
    // Case vidée : valeur effacée (non renseignée), une annulation.
    var_clear(&mut h, 1);
    assert_eq!(bareme_value(&h, "Bust", "XS"), None);
    assert_eq!(
        bareme_value(&h, "Bust", "S"),
        Some(840.0),
        "autres intactes"
    );
    undo(&mut h);
    assert_eq!(*h.state().editor.project(), before);
    // Valeur négative : refus du cœur, rien n'est modifié, la raison reste affichée.
    var_type(&mut h, 2, "-5");
    assert_eq!(*h.state().editor.project(), before);
    assert!(h.state().editor.status.error);
    assert!(h.get_all_by_label_contains("valeur négative").count() >= 1);
    // Texte qui n'est pas un nombre : message de l'interface, rien n'est envoyé.
    var_type(&mut h, 3, "abc");
    assert_eq!(*h.state().editor.project(), before);
    h.get_by_label("« abc » n'est pas un nombre.");
    // Ajout d'une mesure (unité mm par défaut), une annulation.
    var_type(&mut h, 18, "Tour de cou");
    h.get_by_label("Ajouter").click();
    h.run_steps(3);
    {
        let p = h.state().editor.project();
        assert_eq!(p.measurements.len(), 4);
        assert_eq!(p.measurements[3].name, "Tour de cou");
        assert_eq!(p.measurements[3].unit, pf_core::MeasurementUnit::Mm);
    }
    undo(&mut h);
    assert_eq!(*h.state().editor.project(), before);
    // Nom déjà pris (casse ignorée) : refus, rien n'est modifié.
    var_type(&mut h, 18, "bust");
    h.get_by_label("Ajouter").click();
    h.run_steps(3);
    assert_eq!(*h.state().editor.project(), before);
    assert!(h.get_all_by_label_contains("existe déjà").count() >= 1);
    // Renommage par le champ du nom.
    var_type(&mut h, 0, "Buste");
    assert_eq!(h.state().editor.project().measurements[0].name, "Buste");
    undo(&mut h);
    assert_eq!(*h.state().editor.project(), before);
    // Suppression de la première mesure, une annulation.
    h.get_all_by_label("Suppr.").next().expect("Suppr.").click();
    h.run_steps(3);
    assert_eq!(h.state().editor.project().measurements.len(), 2);
    undo(&mut h);
    assert_eq!(*h.state().editor.project(), before);
    // Anglais.
    pf_i18n::set_lang(pf_i18n::Lang::En);
    h.run_steps(4);
    h.get_by_label("New measurement:");
    h.get_by_label("CSV file");
    assert_eq!(h.get_all_by_label("Del.").count(), 3);
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
}

#[test]
fn us101_csv_export_et_import_depuis_la_fenetre() {
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
    let mut h = bareme_harness("bareme");
    let range = h.state().editor.project().sizes[0].id;
    let dir = std::env::temp_dir().join(format!("pf-us101-csv-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let fmt = pf_core::CsvFormat::default();
    let before = h.state().editor.project().clone();
    // Export : mêmes octets que le cœur, projet inchangé, confirmation localisée.
    let path = dir.join("bareme.csv");
    let note = h
        .state_mut()
        .bareme_export_to(range, &fmt, &path)
        .expect("export");
    assert!(
        note.contains("CSV exporté (3 mesure(s), gamme Women)"),
        "{note}"
    );
    let direct = pf_core::export_measurement_csv(&before, range, &fmt).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), direct.bytes);
    assert_eq!(*h.state().editor.project(), before);
    // Import valide : fusion (Bust XS mis à jour, Cou créé), une seule annulation.
    let good = dir.join("bon.csv");
    std::fs::write(
        &good,
        "\u{FEFF}mesure;unite;XS;S;M;L;XL\r\nBust;mm;810,5;;;;\r\nCou;mm;;;350;;\r\n",
    )
    .unwrap();
    let done = h
        .state_mut()
        .bareme_import_from(range, &fmt, false, &good)
        .expect("import");
    assert_eq!(
        done,
        "Import terminé : 1 mesure(s) créée(s), 1 mise(s) à jour."
    );
    assert_eq!(bareme_value(&h, "Bust", "XS"), Some(810.5));
    assert_eq!(
        bareme_value(&h, "Bust", "S"),
        Some(840.0),
        "cellule vide : conservée"
    );
    assert_eq!(bareme_value(&h, "Cou", "M"), Some(350.0));
    undo(&mut h);
    assert_eq!(*h.state().editor.project(), before);
    // Import refusé : lignes d'erreur avec ligne et colonne, aucun changement.
    let bad = dir.join("mauvais.csv");
    std::fs::write(&bad, "mesure;unite;XS\r\nBust;mm;abc\r\nX;cm;1\r\n").unwrap();
    let lines = h
        .state_mut()
        .bareme_import_from(range, &fmt, false, &bad)
        .expect_err("refusé");
    assert_eq!(lines[0], "Ligne 2, colonne 3 : « abc » n'est pas un nombre");
    assert!(lines[1].starts_with("Ligne 3, colonne 2 : "), "{lines:?}");
    assert_eq!(lines.len(), 2);
    assert_eq!(*h.state().editor.project(), before);
    // Fichier absent : message de lecture, rien n'est modifié.
    let lines = h
        .state_mut()
        .bareme_import_from(range, &fmt, false, &dir.join("absent.csv"))
        .expect_err("absent");
    assert!(lines[0].starts_with("Lecture impossible"), "{lines:?}");
    // Export vers une destination illisible : boîte d'erreur, aucun fichier.
    assert!(h.state().error_dialog.is_none());
    assert!(h
        .state_mut()
        .bareme_export_to(range, &fmt, &dir.join("absent").join("x.csv"))
        .is_none());
    assert!(h.state().error_dialog.is_some());
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 3);
    assert_eq!(*h.state().editor.project(), before);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn us102_fr_fiche_client_manquantes_proposition_jamais_appliquee_et_anglais() {
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
    let mut h = bareme_harness("fiche-client");
    assert!(h.get_all_by_label("Camille").count() >= 1);
    h.get_by_label("manquante");
    h.get_by_label_contains("1 mesure(s) manquante(s) : Shoulder angle");
    h.get_by_label("Taille la plus proche (information)");
    // Poitrine 905 et dos 405 : écarts XS 105 + 5, S 65 + 0, M 25 + 5, L 15 + 10, XL 55 + 15.
    h.get_by_label_contains("Taille la plus proche : L (écart total 25,0 mm");
    h.get_by_label("XS : écart total 110,0 mm");
    h.get_by_label("S : écart total 65,0 mm");
    h.get_by_label("M : écart total 30,0 mm");
    h.get_by_label("XL : écart total 70,0 mm");
    h.get_by_label("Bust : fiche 905,0 · taille 920,0 · écart -15,0");
    h.get_by_label("Back length : fiche 405,0 · taille 415,0 · écart -10,0");
    h.get_by_label_contains("Information seulement");
    let before = h.state().editor.project().clone();
    // Champs du dock : nouvelle fiche (0), nom de la fiche (1), puis Bust (2), Back (3), Shoulder (4).
    // L'angle renseigné : la mesure n'est plus manquante, le degré reste hors du total.
    var_type(&mut h, 4, "20,5");
    h.get_by_label("Aucune mesure manquante.");
    h.get_by_label_contains("Taille la plus proche : L (écart total 25,0 mm");
    undo(&mut h);
    assert_eq!(*h.state().editor.project(), before, "une seule annulation");
    // Valeur effacée : manquante, jamais devinée ; seule la longueur de dos compte (S : 0).
    var_clear(&mut h, 2);
    h.get_by_label_contains("2 mesure(s) manquante(s)");
    h.get_by_label_contains("Taille la plus proche : S (écart total 0,0 mm");
    undo(&mut h);
    assert_eq!(*h.state().editor.project(), before);
    // Refus : valeur négative.
    var_type(&mut h, 2, "-3");
    assert_eq!(*h.state().editor.project(), before);
    assert!(h.get_all_by_label_contains("valeur négative").count() >= 1);
    // Nouvelle fiche : sélectionnée, trois mesures manquantes ; une annulation.
    var_type(&mut h, 0, "Dana");
    h.get_by_label("Ajouter").click();
    h.run_steps(3);
    assert_eq!(h.state().editor.project().clients.len(), 2);
    h.get_by_label_contains("3 mesure(s) manquante(s)");
    h.get_by_label("Aucune taille comparable : une taille doit avoir une valeur pour chaque mesure renseignée de la fiche, dont au moins une en mm.");
    undo(&mut h);
    assert_eq!(*h.state().editor.project(), before);
    // Nom déjà pris (casse ignorée).
    var_type(&mut h, 0, "camille");
    h.get_by_label("Ajouter").click();
    h.run_steps(3);
    assert_eq!(*h.state().editor.project(), before);
    assert!(h.get_all_by_label_contains("existe déjà").count() >= 1);
    // Suppression de la fiche (la sélection est tombée avec l'annulation : on la rechoisit),
    // une annulation.
    h.get_by_label("Camille").click();
    h.run_steps(3);
    h.get_by_label("Supprimer la fiche").click();
    h.run_steps(3);
    assert!(h.state().editor.project().clients.is_empty());
    undo(&mut h);
    assert_eq!(*h.state().editor.project(), before);
    // La proposition n'a rien appliqué : valeurs de la fiche et du barème intactes.
    assert_eq!(h.state().editor.project().clients[0].values.len(), 2);
    // Anglais (la fiche est rechoisie : la suppression annulée avait fait tomber la sélection).
    h.get_by_label("Camille").click();
    h.run_steps(3);
    pf_i18n::set_lang(pf_i18n::Lang::En);
    h.run_steps(4);
    h.get_by_label("Closest size (information)");
    h.get_by_label_contains("Closest size: L (total gap 25.0 mm");
    h.get_by_label("missing");
    h.get_by_label_contains("1 missing measurement(s): Shoulder angle");
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
}

#[test]
fn us102_fr_generer_une_taille_depuis_la_fiche_rapport_confirmation_une_annulation() {
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
    let mut h = bareme_harness("fiche-client");
    h.get_by_label("Générer une taille sur-mesure");
    let before = h.state().editor.project().clone();
    // Champs du dock : fiche (0, 1), valeurs (2 à 4), nom de la taille à générer (5).
    var_type(&mut h, 5, "Camille mesure");
    h.get_by_label("Générer une taille…").click();
    h.run_steps(3);
    let p = h.state().editor.project().clone();
    let r = &p.sizes[0];
    assert_eq!(r.sizes.len(), 6);
    assert_eq!(r.sizes[5].label, "Camille mesure");
    let new = r.sizes[5].id;
    let col = |i: usize| {
        p.measurements[i]
            .values
            .iter()
            .find(|v| v.size == new)
            .map(|v| v.value)
    };
    // Bust 905 et dos 405 (fiche) ; angle d'épaule vide, jamais deviné (la taille L en a 21).
    assert_eq!((col(0), col(1), col(2)), (Some(905.0), Some(405.0), None));
    h.get_by_label_contains("Taille « Camille mesure » générée : 2 mesure(s) renseignée(s).");
    h.get_by_label_contains("1 mesure(s) laissée(s) vide(s) : Shoulder angle");
    // Même nom : confirmation explicite, rien ne change avant ; confirmée, colonne identique.
    var_type(&mut h, 5, "camille mesure");
    h.get_by_label("Générer une taille…").click();
    h.run_steps(3);
    h.get_by_label_contains("existe déjà");
    assert_eq!(*h.state().editor.project(), p);
    h.get_by_label("Remplacer la taille").click();
    h.run_steps(3);
    assert_eq!(*h.state().editor.project(), p, "colonne identique");
    // Une seule annulation pour toute la génération.
    undo(&mut h);
    assert_eq!(*h.state().editor.project(), before);
    // Nom vide : refus nommé, projet intact.
    h.get_by_label("Générer une taille…").click();
    h.run_steps(3);
    assert_eq!(*h.state().editor.project(), before);
    assert!(h.get_all_by_label_contains("vide").count() >= 1);
}

#[test]
fn us102_en_generer_une_taille_depuis_la_fiche_rapport_en_anglais() {
    pf_i18n::set_lang(pf_i18n::Lang::En);
    let h = bareme_harness("fiche-client-taille");
    h.get_by_label("Generate a made-to-measure size");
    h.get_by_label("Generate size…");
    h.get_by_label_contains("Size \"Camille\" generated: 2 measurement(s) filled in.");
    h.get_by_label_contains("1 measurement(s) left empty: Shoulder angle");
    let p = h.state().editor.project();
    assert_eq!(p.sizes[0].sizes.len(), 6);
    assert_eq!(p.sizes[0].sizes[5].label, "Camille");
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
}

// ===== US-10.3 / US-10.4 : retouches et mesures finies ==========================================

/// Valeurs de l'étape `retouches` (calculées à la main) : largeur mesurée 391 / 395 / 400 / 405 /
/// 410 contre le barème 390 / 395 / 402 / 405 / 415 retouché de −2 (388 / 393 / 400 / 403 / 413) :
/// écarts +3, +2, 0, +2, −3 (tolérances 2 / 3) ; hauteur 580 / 590 / 600 / 610 / 622 contre
/// (vide) / 590 / 600 / 610 / 620 : n/c en XS puis écarts 0, 0, 0, +2 (tolérances 1 / 1).
#[test]
fn us104_fr_tableau_de_controle_tolerances_refus_ajout_une_annulation() {
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
    let mut h = bareme_harness("retouches");
    h.get_by_label("Contrôle par taille");
    let before = h.state().editor.project().clone();
    assert_eq!(h.get_all_by_label("OK 0,0").count(), 4);
    h.get_by_label("n/c");
    assert_eq!(h.get_all_by_label("OK +2,0").count(), 2);
    h.get_by_label("OK +3,0");
    h.get_by_label("Hors -3,0");
    h.get_by_label("Hors +2,0");
    // Sans les retouches (réglage d'affichage : le projet ne change pas) : écarts +1, 0, −2, 0, −5.
    h.get_by_label("Avec les retouches").click();
    h.run_steps(3);
    h.get_by_label("OK +1,0");
    h.get_by_label("OK -2,0");
    h.get_by_label("Hors -5,0");
    assert_eq!(*h.state().editor.project(), before);
    h.get_by_label("Avec les retouches").click();
    h.run_steps(3);
    // Renommage (champs : 3 par mesure finie, puis le formulaire), une annulation.
    var_type(&mut h, 0, "Largeur finale");
    assert_eq!(
        h.state().editor.project().finished_measures[0].name,
        "Largeur finale"
    );
    undo(&mut h);
    assert_eq!(*h.state().editor.project(), before, "une seule annulation");
    // Tolérance + de la largeur : 0 → l'écart +3 de XS devient hors tolérance.
    var_type(&mut h, 2, "0");
    assert_eq!(
        h.state().editor.project().finished_measures[0].tolerance_plus,
        0.0
    );
    h.get_by_label("Hors +3,0");
    undo(&mut h);
    assert_eq!(*h.state().editor.project(), before);
    // Tolérance négative : refus du cœur, rien n'est modifié, la raison est affichée.
    var_type(&mut h, 2, "-5");
    assert_eq!(*h.state().editor.project(), before);
    assert!(h.get_all_by_label_contains("tolérance").count() >= 1);
    // Texte qui n'est pas un nombre : message de l'interface, rien n'est envoyé.
    var_type(&mut h, 2, "abc");
    assert_eq!(*h.state().editor.project(), before);
    h.get_by_label("« abc » n'est pas un nombre.");
    // Ajout : mesure « Side height », cote de 600 mm, tolérances 2 / 2, une annulation.
    let sheet_name = h.state().editor.project().sheets[0].name.clone();
    for (n, label) in [
        (0, "Side height (mm)".to_string()),
        (1, format!("Cote {sheet_name} : 600.00 mm")),
    ] {
        h.get_all_by_role(egui::accesskit::Role::ComboBox)
            .nth(n)
            .expect("liste")
            .click_accesskit();
        h.run_steps(3);
        h.get_by_role_and_label(egui::accesskit::Role::Button, &label)
            .click_accesskit();
        h.run_steps(3);
    }
    var_type(&mut h, 6, "Hauteur 2");
    var_type(&mut h, 7, "2");
    var_type(&mut h, 8, "2");
    h.get_by_label("Ajouter").click();
    h.run_steps(3);
    {
        let p = h.state().editor.project();
        assert_eq!(p.finished_measures.len(), 3);
        let f = &p.finished_measures[2];
        assert_eq!(f.name, "Hauteur 2");
        assert_eq!((f.tolerance_minus, f.tolerance_plus), (2.0, 2.0));
        assert_eq!(f.measurement, p.measurements[1].id);
    }
    undo(&mut h);
    assert_eq!(*h.state().editor.project(), before);
    // Nom déjà pris (casse ignorée) : refus, rien n'est modifié.
    var_type(&mut h, 6, "finished width");
    h.get_by_label("Ajouter").click();
    h.run_steps(3);
    assert_eq!(*h.state().editor.project(), before);
    assert!(h.get_all_by_label_contains("existe déjà").count() >= 1);
    // Suppression de la première mesure finie, une annulation.
    h.get_all_by_label("Supprimer")
        .next()
        .expect("Supprimer")
        .click();
    h.run_steps(3);
    assert_eq!(h.state().editor.project().finished_measures.len(), 1);
    undo(&mut h);
    assert_eq!(*h.state().editor.project(), before);
    // Anglais.
    pf_i18n::set_lang(pf_i18n::Lang::En);
    h.run_steps(4);
    h.get_by_label("Check by size");
    h.get_by_label("With alterations");
    h.get_by_label_contains("Out -3");
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
}

#[test]
fn us103_fr_retouches_delta_ajout_refus_suppression_une_annulation() {
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
    let mut h = bareme_harness("retouches");
    h.state_mut().finished.as_mut().unwrap().alt_open = true;
    h.run_steps(3);
    let before = h.state().editor.project().clone();
    assert_eq!(before.alterations.len(), 1);
    // Champs : retouche (nom, delta), nouvelle retouche (nom, delta), puis les mesures finies.
    var_type(&mut h, 1, "-3");
    assert_eq!(h.state().editor.project().alterations[0].delta, -3.0);
    // Écarts de la largeur avec −3 : référence 387, 392, 399, 402, 412 → XL = 410 − 412 = −2.
    h.get_by_label("OK -2,0");
    undo(&mut h);
    assert_eq!(*h.state().editor.project(), before, "une seule annulation");
    var_type(&mut h, 0, "Largeur moins deux");
    assert_eq!(
        h.state().editor.project().alterations[0].name,
        "Largeur moins deux"
    );
    undo(&mut h);
    assert_eq!(*h.state().editor.project(), before);
    // Sans cible choisie : message de l'interface, rien n'est envoyé.
    var_type(&mut h, 2, "Hauteur +1");
    var_type(&mut h, 3, "1,5");
    h.get_all_by_label("Ajouter")
        .next()
        .expect("Ajouter")
        .click();
    h.run_steps(3);
    assert_eq!(*h.state().editor.project(), before);
    h.get_by_label("Choisissez la mesure ciblée.");
    // Cible choisie dans la liste : ajout, une annulation.
    h.get_all_by_role(egui::accesskit::Role::ComboBox)
        .nth(1)
        .expect("liste de la nouvelle retouche")
        .click_accesskit();
    h.run_steps(3);
    h.get_by_role_and_label(egui::accesskit::Role::Button, "Side height (mm)")
        .click_accesskit();
    h.run_steps(3);
    h.get_all_by_label("Ajouter")
        .next()
        .expect("Ajouter")
        .click();
    h.run_steps(3);
    {
        let p = h.state().editor.project();
        assert_eq!(p.alterations.len(), 2);
        assert_eq!(p.alterations[1].name, "Hauteur +1");
        assert_eq!(p.alterations[1].delta, 1.5);
        assert_eq!(p.alterations[1].target, p.measurements[1].id);
    }
    undo(&mut h);
    assert_eq!(*h.state().editor.project(), before);
    // Delta qui n'est pas un nombre : message de l'interface.
    var_type(&mut h, 1, "abc");
    assert_eq!(*h.state().editor.project(), before);
    h.get_by_label("« abc » n'est pas un nombre.");
    // Suppression (la première liste « Supprimer » est celle de la retouche), une annulation.
    h.get_all_by_label("Supprimer")
        .next()
        .expect("Supprimer")
        .click();
    h.run_steps(3);
    assert!(h.state().editor.project().alterations.is_empty());
    undo(&mut h);
    assert_eq!(*h.state().editor.project(), before);
    // Anglais.
    pf_i18n::set_lang(pf_i18n::Lang::En);
    h.run_steps(4);
    h.get_by_label("Alterations (1)");
    assert_eq!(h.get_all_by_label("Delete").count(), 3);
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
}

// ---------------------------------------------------------------------------------------------
// Restes d'E03 (tranche 107) : rectangle maintenu à la création, formules sur les rayons du compas
// ---------------------------------------------------------------------------------------------

fn kept_count(h: &Harness<'_, App>) -> usize {
    h.state().editor.sheet().kept_segment_ids().len()
}

fn us032b_walk(h: &mut Harness<'_, App>, checkbox: &str) {
    key(h, Key::R);
    assert_eq!(h.state().editor.tool, Tool::Rectangle);
    // La case apparaît avec la palette (après le premier coin), décochée par défaut : un
    // rectangle ordinaire, aucune construction maintenue.
    click(h, pt(0.0, 0.0));
    h.run_steps(3);
    h.get_by_label(checkbox);
    assert!(!h.state().editor.rect_kept);
    type_text(h, "100");
    key(h, Key::Tab);
    type_text(h, "60");
    key(h, Key::Enter);
    assert_eq!(h.state().editor.sheet().entities.len(), 4);
    assert_eq!(kept_count(h), 0);
    // Cochée : le second rectangle est maintenu (deux côtés portent leur construction).
    click(h, pt(150.0, 100.0));
    h.run_steps(3);
    h.get_by_label(checkbox).click();
    h.run_steps(3);
    assert!(h.state().editor.rect_kept);
    let before = h.state().editor.project().clone();
    // Largeur et hauteur dans les deux champs de la palette (le focus est sur la case).
    for text in ["100", "60"] {
        {
            let mut fields: Vec<_> = h
                .get_all_by_role(egui::accesskit::Role::TextInput)
                .filter(|f| f.value().as_deref() == Some(""))
                .collect();
            fields.sort_by(|a, b| a.rect().min.y.total_cmp(&b.rect().min.y));
            fields[0].focus();
        }
        h.run_steps(2);
        type_text(h, text);
    }
    key(h, Key::Enter);
    assert_eq!(h.state().editor.sheet().entities.len(), 8);
    assert_eq!(kept_count(h), 2);
    // Une seule étape annulable pour tout le rectangle.
    undo(h);
    assert_eq!(*h.state().editor.project(), before);
}

#[test]
fn us032b_rectangle_maintenu_case_a_la_creation_fr() {
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
    let mut h = harness();
    h.state_mut().editor.grid_snap_mm = None;
    us032b_walk(&mut h, "Conserver la construction (rectangle maintenu)");
}

#[test]
fn us032b_rectangle_maintenu_case_a_la_creation_en_et_deplacement_du_coin() {
    pf_i18n::set_lang(pf_i18n::Lang::En);
    let mut h = harness();
    h.state_mut().editor.grid_snap_mm = None;
    us032b_walk(&mut h, "Keep the construction (kept rectangle)");
    // Un rectangle maintenu dont on déplace le coin : tout le rectangle suit.
    let sheet = h.state().editor.sheet;
    let o = h
        .state_mut()
        .editor
        .exec(pf_core::Command::AddConstructedRectangle {
            sheet,
            corner: pf_core::PointRef::New(pt(1000.0, 1000.0)),
            width_mm: 40.0,
            height_mm: 30.0,
            kept: true,
        })
        .unwrap();
    h.state_mut()
        .editor
        .exec(pf_core::Command::MovePoint {
            sheet,
            point: o.points[0],
            to: pt(1010.0, 1020.0),
        })
        .unwrap();
    let ed = h.state().editor.sheet();
    assert_eq!(ed.position(o.points[3]).unwrap(), pt(1050.0, 1050.0));
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
}

#[test]
fn us031_ca3_formule_sur_les_rayons_du_compas_en_et_fr() {
    use pf_core::ParamKind::{RadiusA, RadiusB};
    for (lang, hint, kept) in [
        (pf_i18n::Lang::Fr, "Formule · Rayon A", "ƒ Maintenu"),
        (pf_i18n::Lang::En, "Formula · Radius A", "ƒ Maintained"),
    ] {
        pf_i18n::set_lang(lang);
        let mut h = harness();
        let c = {
            let ed = &mut h.state_mut().editor;
            ed.grid_snap_mm = None;
            let sheet = ed.sheet;
            ed.exec(pf_core::Command::AddVariable {
                name: "r".into(),
                unit: pf_core::VariableUnit::Mm,
                expr: "60".into(),
            })
            .unwrap();
            let a = ed
                .exec(pf_core::Command::AddPoint {
                    sheet,
                    at: pt(0.0, 0.0),
                })
                .unwrap()
                .points[0];
            let b = ed
                .exec(pf_core::Command::AddPoint {
                    sheet,
                    at: pt(100.0, 0.0),
                })
                .unwrap()
                .points[0];
            let c = ed
                .exec(pf_core::Command::AddPointAt {
                    sheet,
                    point: pf_core::PointRef::Derived(pf_core::PointDefinition::Compass {
                        center_a: a,
                        center_b: b,
                        radius_a_mm: 50.0,
                        radius_b_mm: 80.0,
                        near: pf_core::AimMm { x: 36.0, y: 48.0 },
                    }),
                })
                .unwrap()
                .points[0];
            ed.selection.clear();
            c
        };
        h.run_steps(3);
        select_points(&mut h, &[c]);
        h.get_by_label(hint);
        assert!(h.query_by_label(kept).is_none());
        // r = 60, rayon B = 80 : (36 ; 48), calculé à la main.
        formula_type(&mut h, 0, "r");
        assert!(
            !h.state().editor.status.error,
            "{}",
            h.state().editor.status.text
        );
        assert_eq!(param_value(&h, c, RadiusA), 60.0);
        assert!(has_formula(&h, c, RadiusA) && !has_formula(&h, c, RadiusB));
        h.get_by_label(kept);
        let q = h.state().editor.sheet().position(c).unwrap();
        assert!((q.x - 36.0).abs() < 1e-9 && (q.y - 48.0).abs() < 1e-9);
        // r = 10 (10 + 80 < 100) : refusé, rien ne change.
        let before = h.state().editor.project().clone();
        assert!(h
            .state_mut()
            .editor
            .exec(pf_core::Command::SetVariableExpr {
                name: "r".into(),
                expr: "10".into(),
            })
            .is_none());
        assert_eq!(*h.state().editor.project(), before);
    }
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
}

/// Feuille avec `L = 250`, `p = 0,6`, `a = 30` ; segment S de (0 ; 0) à (1000 ; 0) ; points :
/// distance 10 mm sur S, proportion 0,1 sur S, milieu de S par division en deux.
fn formula_rest_harness() -> (Harness<'static, App>, [pf_core::PointId; 3]) {
    let mut h = harness();
    let ids = {
        let ed = &mut h.state_mut().editor;
        ed.grid_snap_mm = None;
        let sheet = ed.sheet;
        for (name, unit, expr) in [
            ("L", pf_core::VariableUnit::Mm, "250"),
            ("p", pf_core::VariableUnit::Mm, "0.6"),
            ("a", pf_core::VariableUnit::Deg, "30"),
        ] {
            ed.exec(pf_core::Command::AddVariable {
                name: name.into(),
                unit,
                expr: expr.into(),
            })
            .unwrap();
        }
        let seg = ed
            .exec(pf_core::Command::AddSegment {
                sheet,
                from: pf_core::PointRef::New(pt(0.0, 0.0)),
                to: pf_core::PointRef::New(pt(1000.0, 0.0)),
            })
            .unwrap()
            .entities[0];
        let mut derived = |definition| {
            ed.exec(pf_core::Command::AddPointAt {
                sheet,
                point: pf_core::PointRef::Derived(definition),
            })
            .unwrap()
            .points[0]
        };
        let ps = derived(pf_core::PointDefinition::OnSegment {
            segment: seg,
            distance_mm: 10.0,
            origin: pf_core::SegmentEnd::Start,
        });
        let pr = derived(pf_core::PointDefinition::OnSegmentRatio {
            segment: seg,
            ratio: 0.1,
        });
        let pd = ed
            .exec(pf_core::Command::DivideTrace {
                sheet,
                trace: seg,
                spec: pf_core::DivisionSpec::Equal { parts: 2 },
                start: None,
            })
            .unwrap()
            .points[0];
        ed.selection.clear();
        [ps, pr, pd]
    };
    h.run_steps(3);
    (h, ids)
}

#[test]
fn us031_ca3_restes_formules_segment_proportions_division_fr_en() {
    use pf_core::ParamKind::{DivisionRatio, SegmentDistance, SegmentRatio};
    for (lang, dist_hint, ratio_hint, div_hint, kept) in [
        (
            pf_i18n::Lang::Fr,
            "Formule · Distance sur le segment",
            "Formule · Proportion sur le segment",
            "Formule · Fraction de la division",
            "ƒ Maintenu",
        ),
        (
            pf_i18n::Lang::En,
            "Formula · Distance on segment",
            "Formula · Proportion on segment",
            "Formula · Division fraction",
            "ƒ Maintained",
        ),
    ] {
        pf_i18n::set_lang(lang);
        let (mut h, [ps, pr, pd]) = formula_rest_harness();
        // Distance sur un segment : 250 / 2 + 10 = 135 mm.
        select_points(&mut h, &[ps]);
        h.get_by_label(dist_hint);
        assert!(h.query_by_label(kept).is_none());
        formula_type(&mut h, 0, "L/2+10");
        assert!(
            !h.state().editor.status.error,
            "{}",
            h.state().editor.status.text
        );
        assert_eq!(param_value(&h, ps, SegmentDistance), 135.0);
        assert_eq!(
            h.state().editor.sheet().position(ps).unwrap(),
            pt(135.0, 0.0)
        );
        h.get_by_label(kept);
        h.get_by_label("= L/2+10");
        assert!(h.query_all_by_label_contains("135").next().is_some());
        // Proportion sur un segment : 0,6 / 2 = 0,3 -> 300 mm, affichée en pourcentage.
        select_points(&mut h, &[pr]);
        h.get_by_label(ratio_hint);
        formula_type(&mut h, 0, "p/2");
        assert_eq!(param_value(&h, pr, SegmentRatio), 0.3);
        assert_eq!(
            h.state().editor.sheet().position(pr).unwrap(),
            pt(300.0, 0.0)
        );
        assert!(h.query_all_by_label_contains("30").next().is_some());
        assert!(h.query_all_by_label_contains("%").next().is_some());
        // Fraction de division : 1 / 4 -> 250 mm ; erreur d'expression : rien ne change.
        select_points(&mut h, &[pd]);
        h.get_by_label(div_hint);
        let before = h.state().editor.project().clone();
        formula_type(&mut h, 0, "p*");
        assert_eq!(*h.state().editor.project(), before);
        assert!(h.state().formula_error.is_some());
        formula_type(&mut h, 0, "3/2");
        assert_eq!(*h.state().editor.project(), before, "hors plage refusé");
        formula_type(&mut h, 0, "1/4");
        assert_eq!(param_value(&h, pd, DivisionRatio), 0.25);
        assert_eq!(
            h.state().editor.sheet().position(pd).unwrap(),
            pt(250.0, 0.0)
        );
        // Une seule étape pour la variable : annuler la rétablit.
        let before = h.state().editor.project().clone();
        h.state_mut()
            .editor
            .exec(pf_core::Command::SetVariableExpr {
                name: "L".into(),
                expr: "300".into(),
            })
            .unwrap();
        assert_eq!(param_value(&h, ps, SegmentDistance), 160.0);
        undo(&mut h);
        assert_eq!(*h.state().editor.project(), before);
        // Un nombre saisi dans le champ d'un paramètre lié le fige (formule déliée).
        select_points(&mut h, &[pr]);
        formula_type(&mut h, 0, "0,4");
        assert!(!has_formula(&h, pr, SegmentRatio));
        assert_eq!(param_value(&h, pr, SegmentRatio), 0.4);
        assert_eq!(
            h.state().editor.sheet().position(pr).unwrap(),
            pt(400.0, 0.0)
        );
    }
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
}

#[test]
fn us031_ca3_restes_formule_a_la_creation_dans_la_palette_fr_en() {
    use pf_core::ParamKind::{Angle, Distance};
    for (lang, announce) in [
        (pf_i18n::Lang::Fr, "ƒ Maintenu : L/2 = 125,0 mm"),
        (pf_i18n::Lang::En, "ƒ Maintained: L/2 = 125.0 mm"),
    ] {
        pf_i18n::set_lang(lang);
        let (mut h, _) = formula_rest_harness();
        key(&mut h, Key::O);
        assert_eq!(h.state().editor.tool, Tool::Point);
        h.get_by_label("Direction").click();
        h.run_steps(2);
        click(&mut h, pt(0.0, 0.0));
        assert_eq!(h.state().editor.state, ToolState::RelativePick);
        h.run_steps(2);
        // Expression dans le champ distance et le champ angle : annoncées avant la création.
        type_text(&mut h, "L/2");
        key(&mut h, Key::Tab);
        type_text(&mut h, "a*2");
        h.run_steps(2);
        h.get_by_label(announce);
        h.get_by_label_contains("a*2 = 60");
        let before = h.state().editor.project().clone();
        key(&mut h, Key::Enter);
        assert!(
            !h.state().editor.status.error,
            "{}",
            h.state().editor.status.text
        );
        let id = *h
            .state()
            .editor
            .selection
            .points
            .first()
            .expect("point créé");
        // 250 / 2 = 125 mm à 2 x 30 = 60° de A (0 ; 0) : (62,5 ; 125 x sin 60° = 108,2531754730548).
        let q = h.state().editor.sheet().position(id).unwrap();
        assert!((q.x - 62.5).abs() < 1e-9 && (q.y - 108.253_175_473_054_8).abs() < 1e-9);
        assert_eq!(param_value(&h, id, Distance), 125.0);
        assert_eq!(param_value(&h, id, Angle), 60.0);
        assert!(has_formula(&h, id, Distance) && has_formula(&h, id, Angle));
        // Une seule étape annulable pour le point et ses deux liens.
        undo(&mut h);
        assert_eq!(*h.state().editor.project(), before);
        // Expression refusée (variable inconnue) : message, rien n'est créé, saisie conservée.
        click(&mut h, pt(0.0, 0.0));
        h.run_steps(2);
        type_text(&mut h, "zz+1");
        key(&mut h, Key::Enter);
        assert!(h.state().editor.status.error);
        assert_eq!(*h.state().editor.project(), before);
        assert_eq!(h.state().editor.state, ToolState::RelativePick);
    }
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
}

// ===== US-06.9 : lignes de piqûre ================================================================

#[test]
fn us069_bloc_replie_apercu_puis_creation_en_une_etape_et_annulation() {
    let (mut h, piece, _) = pitch_harness();
    h.get_by_label("Lignes de piqûre…");
    assert!(h.query_by_label("Créer la ligne").is_none());
    open_fold(&mut h, "stitch.title");
    h.get_by_label_contains("Bord : 1 tracé(s) du contour.");
    let before = h.state().editor.project().clone();
    // Aperçu : pas 4 sur 100 mm = 26 trous espacés de 4 mm, rien n'est modifié.
    h.get_by_label("Aperçu").click();
    h.run_steps(3);
    assert_eq!(*h.state().editor.project(), before);
    h.get_by_label_contains("26 trou(s), espacement réel 4,0 mm, trajet de 100,0 mm.");
    let preview = h
        .state()
        .stitch_form
        .as_ref()
        .unwrap()
        .preview
        .clone()
        .unwrap();
    assert_eq!(preview.count, 26);
    h.get_by_label("Créer la ligne").click();
    h.run_steps(3);
    assert!(
        !h.state().editor.status.error,
        "{}",
        h.state().editor.status.text
    );
    let line = h
        .state()
        .editor
        .project()
        .piece(piece)
        .unwrap()
        .stitch_lines[0]
        .id;
    let layout = h
        .state()
        .editor
        .project()
        .stitch_line_layout(piece, line)
        .unwrap();
    // Les positions créées sont celles de l'aperçu ; le retrait par défaut est 3 mm.
    assert_eq!(layout.holes.len(), 26);
    for (placed, (at, _)) in layout.holes.iter().zip(&preview.holes) {
        assert_eq!(placed.position, *at);
    }
    assert!((layout.holes[0].position.y - 3.0).abs() < 1e-9);
    h.get_by_label_contains("26 trou(s), espacement 4,0 mm");
    // Une seule annulation : le projet est celui d'avant, octets identiques.
    undo(&mut h);
    assert_eq!(
        pf_core::persist::to_json(h.state().editor.project()).unwrap(),
        pf_core::persist::to_json(&before).unwrap()
    );
}

#[test]
fn us069_retrait_hors_de_la_piece_refuse_sans_trace_et_champ_illisible() {
    let (mut h, _, _) = pitch_harness();
    open_fold(&mut h, "stitch.title");
    let before = h.state().editor.project().clone();
    // Retrait 80 mm sur une pièce de 70 mm : refusé par le cœur, message affiché.
    replace_field(&mut h, "3", "80");
    h.get_by_label("Créer la ligne").click();
    h.run_steps(3);
    assert!(h.state().editor.status.error);
    assert!(
        h.state().editor.status.text.contains("sortirait"),
        "{}",
        h.state().editor.status.text
    );
    assert_eq!(*h.state().editor.project(), before);
    h.get_by_label("Aperçu").click();
    h.run_steps(3);
    assert!(h.state().editor.status.error);
    assert!(h.state().stitch_form.as_ref().unwrap().preview.is_none());
    replace_field(&mut h, "80", "abc");
    h.get_by_label("Créer la ligne").click();
    h.run_steps(3);
    assert!(h.state().editor.status.error);
    assert_eq!(*h.state().editor.project(), before);
    // Sans tracé du contour sélectionné : boutons grisés.
    h.state_mut().editor.selection.clear();
    h.run_steps(3);
    assert!(h
        .get_by_label("Créer la ligne")
        .accesskit_node()
        .is_disabled());
}

#[test]
fn us069_forme_nombre_fixe_et_suppression() {
    let (mut h, piece, _) = pitch_harness();
    open_fold(&mut h, "stitch.title");
    h.get_by_label("Losange français").click();
    h.run_steps(3);
    h.get_by_label("Nombre fixé").click();
    h.run_steps(3);
    replace_field(&mut h, "10", "5");
    h.get_by_label("Créer la ligne").click();
    h.run_steps(3);
    assert!(
        !h.state().editor.status.error,
        "{}",
        h.state().editor.status.text
    );
    let p = h.state().editor.project().piece(piece).unwrap().clone();
    assert_eq!(p.stitch_lines.len(), 1);
    assert_eq!(
        p.stitch_lines[0].hole,
        pf_core::StitchHole::French {
            length_mm: 3.0,
            width_mm: 1.0
        }
    );
    assert_eq!(p.stitch_lines[0].count, Some(5));
    // Supprimer : une étape, annulable.
    let before = h.state().editor.project().clone();
    h.get_by_label("Supprimer la ligne").click();
    h.run_steps(3);
    assert!(h
        .state()
        .editor
        .project()
        .piece(piece)
        .unwrap()
        .stitch_lines
        .is_empty());
    undo(&mut h);
    assert_eq!(*h.state().editor.project(), before);
}

/// Deux pièces 100 × 60 (« Devant » en (0 ; 0), « Dos » en (200 ; 0)) ; une ligne de piqûre de pas
/// 4 mm sur le bas de « Devant » ; fiche de « Devant » ouverte (premier bloc « Lignes de piqûre »).
fn stitch_pair_harness() -> (
    Harness<'static, App>,
    pf_core::PieceId,
    pf_core::PieceId,
    pf_core::StitchLineId,
) {
    let (mut h, front, bottom) = pitch_harness();
    let back = {
        let ed = &mut h.state_mut().editor;
        let sheet = ed.sheet;
        ed.exec(pf_core::Command::AddRectangle {
            sheet,
            corner: pf_core::PointRef::New(pt(200.0, 0.0)),
            width_mm: 100.0,
            height_mm: 60.0,
        })
        .unwrap();
        ed.exec(pf_core::Command::ExtractPiece {
            sheet,
            source: pf_core::PieceSource::Click {
                at: pt(250.0, 30.0),
            },
            name: "Dos".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0]
    };
    let line = {
        let ed = &mut h.state_mut().editor;
        ed.exec(pf_core::Command::AddStitchLine {
            piece: front,
            line: pf_core::StitchLineSpec {
                edges: vec![bottom],
                hole: pf_core::StitchHole::Round { diameter_mm: 1.2 },
                pitch_mm: 4.0,
                setback_mm: 3.0,
                count: None,
                continues: None,
            },
        })
        .unwrap()
        .stitch_lines[0]
    };
    h.run_steps(3);
    h.get_all_by_label(&pf_i18n::tr("stitch.title"))
        .next()
        .unwrap()
        .click();
    h.run_steps(3);
    (h, front, back, line)
}

#[test]
fn us086_report_sur_la_piece_voisine_en_une_etape_et_lecture_du_controle() {
    let (mut h, front, back, line) = stitch_pair_harness();
    let before = h.state().editor.project().clone();
    h.get_by_label("Reporter…").click();
    h.run_steps(3);
    // Sans bord sélectionné sur « Dos » : le report est grisé.
    assert!(h
        .get_by_label_contains("Sur « Dos »")
        .accesskit_node()
        .is_disabled());
    let edge = edge_at(&h, back, pt(200.0, 0.0), pt(300.0, 0.0));
    h.state_mut().editor.selection.entities = vec![edge];
    h.run_steps(3);
    h.get_by_label_contains("Sur « Dos »").click();
    h.run_steps(3);
    assert!(
        !h.state().editor.status.error,
        "{}",
        h.state().editor.status.text
    );
    let project = h.state().editor.project().clone();
    assert_eq!(project.stitch_pairs.len(), 1);
    assert_eq!(project.piece(back).unwrap().stitch_lines.len(), 1);
    // 26 trous des deux côtés, pas moyen 4,0 / 4,0 : écrit par la lecture du couple.
    h.get_by_label_contains("Couple 1 : 26 trou(s) / 26 trou(s), pas moyen 4,0 mm / 4,0 mm");
    let r = &project.stitch_check_report()[0];
    assert_eq!(r.count_status(pf_core::StitchMatch::Matched), 26);
    // Une seule annulation : le projet est celui d'avant le report.
    undo(&mut h);
    assert_eq!(
        pf_core::persist::to_json(h.state().editor.project()).unwrap(),
        pf_core::persist::to_json(&before).unwrap()
    );
    let _ = (front, line);
}

#[test]
fn us086_apparier_deux_lignes_trou_manquant_liste_puis_rompre() {
    let (mut h, front, back, line) = stitch_pair_harness();
    // Ligne de « Dos » à 4 trous (x = 200, 233,3, 266,7, 300) ; celle de « Devant » en a 26.
    let edge = edge_at(&h, back, pt(200.0, 0.0), pt(300.0, 0.0));
    let other = h
        .state_mut()
        .editor
        .exec(pf_core::Command::AddStitchLine {
            piece: back,
            line: pf_core::StitchLineSpec {
                edges: vec![edge],
                hole: pf_core::StitchHole::Round { diameter_mm: 1.2 },
                pitch_mm: 4.0,
                setback_mm: 3.0,
                count: Some(24),
                continues: None,
            },
        })
        .unwrap()
        .stitch_lines[0];
    h.run_steps(3);
    h.get_by_label("Apparier…").click();
    h.run_steps(3);
    h.get_by_label_contains(&format!("Ligne {} de « Dos »", other.0))
        .click();
    h.run_steps(3);
    assert!(
        !h.state().editor.status.error,
        "{}",
        h.state().editor.status.text
    );
    let r = h.state().editor.project().stitch_check_report()[0].clone();
    assert_eq!((r.count_a, r.count_b), (26, 24));
    // Le contrôle est lu à l'écran : un trou sans vis-à-vis est listé avec sa position.
    h.get_by_label_contains("Couple 1 : 26 trou(s) / 24 trou(s)");
    assert!(r.unmatched().count() >= 1);
    let first = r.unmatched().next().unwrap();
    let hole = first.a.or(first.b).unwrap();
    let text = pf_i18n::tr_args(
        if first.a.is_some() {
            "stitch.pair.item.missing"
        } else {
            "stitch.pair.item.extra"
        },
        &[
            ("n", &first.number.to_string()),
            ("x", &pf_core::input::format_mm(hole.position.x)),
            ("y", &pf_core::input::format_mm(hole.position.y)),
            ("gap", &pf_core::input::format_mm(0.0)),
        ],
    );
    h.get_by_label_contains(&text);
    // Rompre le couple : les lignes restent, plus aucun couple.
    let before = h.state().editor.project().clone();
    h.get_by_label("Rompre le couple").click();
    h.run_steps(3);
    assert!(h.state().editor.project().stitch_pairs.is_empty());
    assert_eq!(
        h.state()
            .editor
            .project()
            .piece(front)
            .unwrap()
            .stitch_lines
            .len(),
        1
    );
    undo(&mut h);
    assert_eq!(*h.state().editor.project(), before);
    let _ = line;
}

#[test]
fn us086_controle_des_trous_en_anglais() {
    pf_i18n::set_lang(pf_i18n::Lang::En);
    let (mut h, _, back, _) = stitch_pair_harness();
    h.get_by_label("Carry over…").click();
    h.run_steps(3);
    let edge = edge_at(&h, back, pt(200.0, 0.0), pt(300.0, 0.0));
    h.state_mut().editor.selection.entities = vec![edge];
    h.run_steps(3);
    h.get_by_label_contains("To “Dos”").click();
    h.run_steps(3);
    h.get_by_label_contains("Pair 1: 26 hole(s) / 26 hole(s), mean pitch 4.0 mm / 4.0 mm");
    h.get_by_label("Break pair");
    assert_eq!(h.state().editor.project().stitch_pairs.len(), 1);
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
}

#[test]
fn us069_lignes_de_piqure_en_anglais() {
    pf_i18n::set_lang(pf_i18n::Lang::En);
    let (mut h, piece, _) = pitch_harness();
    h.get_by_label("Stitching lines…");
    open_fold(&mut h, "stitch.title");
    h.get_by_label_contains("Edge: 1 outline path(s).");
    h.get_by_label("French lozenge");
    h.get_by_label("Japanese oblique");
    h.get_by_label("Requested pitch (mm)");
    h.get_by_label("Preview").click();
    h.run_steps(3);
    h.get_by_label_contains("26 hole(s), actual spacing 4.0 mm, path of 100.0 mm.");
    h.get_by_label("Create line").click();
    h.run_steps(3);
    assert_eq!(
        h.state()
            .editor
            .project()
            .piece(piece)
            .unwrap()
            .stitch_lines
            .len(),
        1
    );
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
}

// ===== US-06.10 : zones de parage, de collage et de découpe interne ===========================

#[test]
fn us0610_bloc_replie_apercu_puis_creation_en_une_etape_et_annulation() {
    let (mut h, piece, _) = pitch_harness();
    h.get_by_label("Zones (parage, collage, découpe)…");
    assert!(h.query_by_label("Créer la zone").is_none());
    open_fold(&mut h, "zone.title");
    h.get_by_label_contains("Bande sur : 1 tracé(s) du contour.");
    let before = h.state().editor.project().clone();
    // Aperçu : bande de 5 mm sur un bord de 100 mm = 500 mm², rien n'est modifié.
    h.get_by_label("Aperçu de la zone").click();
    h.run_steps(3);
    assert_eq!(*h.state().editor.project(), before);
    h.get_by_label_contains("Parage : surface de 500,0 mm².");
    let preview = h
        .state()
        .zone_form
        .as_ref()
        .unwrap()
        .preview
        .clone()
        .unwrap();
    assert_eq!(preview.polygon.len(), 4);
    h.get_by_label("Créer la zone").click();
    h.run_steps(3);
    assert!(
        !h.state().editor.status.error,
        "{}",
        h.state().editor.status.text
    );
    let zone = h.state().editor.project().piece(piece).unwrap().zones[0].id;
    let z = h.state().editor.project().zone_layout(piece, zone).unwrap();
    assert_eq!(
        z.polygon, preview.polygon,
        "la zone créée est celle de l'aperçu"
    );
    assert!((z.area_mm2 - 500.0).abs() < 1e-9);
    assert_eq!(z.width_mm, Some(5.0));
    h.get_by_label_contains("Parage, bande de 5,0 mm, 500,0 mm².");
    // Une seule annulation : le projet est celui d'avant, octets identiques.
    undo(&mut h);
    assert_eq!(
        pf_core::persist::to_json(h.state().editor.project()).unwrap(),
        pf_core::persist::to_json(&before).unwrap()
    );
}

#[test]
fn us0610_largeur_hors_de_la_piece_refusee_sans_trace_et_champ_illisible() {
    let (mut h, _, _) = pitch_harness();
    open_fold(&mut h, "zone.title");
    let before = h.state().editor.project().clone();
    // 80 mm sur une pièce de 70 mm : refusé par le cœur, message affiché.
    replace_field(&mut h, "5", "80");
    h.get_by_label("Créer la zone").click();
    h.run_steps(3);
    assert!(h.state().editor.status.error);
    assert!(
        h.state().editor.status.text.contains("sortirait"),
        "{}",
        h.state().editor.status.text
    );
    assert_eq!(*h.state().editor.project(), before);
    h.get_by_label("Aperçu de la zone").click();
    h.run_steps(3);
    assert!(h.state().editor.status.error);
    assert!(h.state().zone_form.as_ref().unwrap().preview.is_none());
    replace_field(&mut h, "80", "abc");
    h.get_by_label("Créer la zone").click();
    h.run_steps(3);
    assert!(h.state().editor.status.error);
    assert_eq!(*h.state().editor.project(), before);
    // Sans tracé du contour sélectionné : boutons grisés.
    h.state_mut().editor.selection.clear();
    h.run_steps(3);
    assert!(h
        .get_by_label("Créer la zone")
        .accesskit_node()
        .is_disabled());
}

#[test]
fn us0610_polygone_libre_sur_les_points_selectionnes_et_suppression() {
    let (mut h, piece, _) = pitch_harness();
    open_fold(&mut h, "zone.title");
    h.get_by_label("Polygone libre").click();
    h.run_steps(3);
    h.get_by_label("Découpe interne").click();
    h.run_steps(3);
    // Moins de trois points : bouton grisé.
    assert!(h
        .get_by_label("Créer la zone")
        .accesskit_node()
        .is_disabled());
    // Les trois points libres (50 ; 30) (10 ; 10) (40 ; 50) : triangle d'aire
    // |50 × (10 − 50) + 10 × (50 − 30) + 40 × (30 − 10)| / 2 = 1000 / 2 = 500 mm².
    let ids: Vec<pf_core::PointId> = {
        let sheet = h.state().editor.sheet();
        [(50.0, 30.0), (10.0, 10.0), (40.0, 50.0)]
            .iter()
            .map(|&(x, y)| {
                sheet
                    .points
                    .iter()
                    .find(|p| {
                        sheet
                            .position(p.id)
                            .is_ok_and(|q| (q.x - x).abs() < 1e-9 && (q.y - y).abs() < 1e-9)
                    })
                    .unwrap()
                    .id
            })
            .collect()
    };
    select_points(&mut h, &ids);
    h.get_by_label_contains("Sommets : 3 point(s) sélectionné(s).");
    h.get_by_label("Créer la zone").click();
    h.run_steps(3);
    assert!(
        !h.state().editor.status.error,
        "{}",
        h.state().editor.status.text
    );
    let zones = h
        .state()
        .editor
        .project()
        .piece(piece)
        .unwrap()
        .zones
        .clone();
    assert_eq!(zones.len(), 1);
    assert_eq!(zones[0].kind, pf_core::ZoneKind::InnerCut);
    let z = h
        .state()
        .editor
        .project()
        .zone_layout(piece, zones[0].id)
        .unwrap();
    assert!((z.area_mm2 - 500.0).abs() < 1e-9, "{}", z.area_mm2);
    h.get_by_label_contains("Découpe interne, polygone de 3 sommets, 500,0 mm².");
    // Suppression : une étape, la zone disparaît.
    h.get_by_label("Supprimer la zone").click();
    h.run_steps(3);
    assert!(h
        .state()
        .editor
        .project()
        .piece(piece)
        .unwrap()
        .zones
        .is_empty());
}

#[test]
fn us0610_zones_en_anglais() {
    pf_i18n::set_lang(pf_i18n::Lang::En);
    let (mut h, piece, _) = pitch_harness();
    h.get_by_label("Zones (skiving, gluing, cut-out)…");
    open_fold(&mut h, "zone.title");
    h.get_by_label_contains("Band on: 1 outline path(s).");
    h.get_by_label("Skiving");
    h.get_by_label("Gluing");
    h.get_by_label("Inner cut-out");
    h.get_by_label("Band width (mm)");
    h.get_by_label("Zone preview").click();
    h.run_steps(3);
    h.get_by_label_contains("Skiving: area of 500.0 mm².");
    h.get_by_label("Create zone").click();
    h.run_steps(3);
    assert_eq!(
        h.state().editor.project().piece(piece).unwrap().zones.len(),
        1
    );
    h.get_by_label_contains("Skiving, band of 5.0 mm, 500.0 mm².");
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
}

// ----- US-12.2 : import de pièces d'un DXF de type AAMA / ASTM --------------------------------

fn aama_fixture() -> String {
    let mut s = String::new();
    s.push_str("  0\nSECTION\n  2\nHEADER\n  9\n$INSUNITS\n 70\n4\n  0\nENDSEC\n");
    s.push_str(
        "  0\nSECTION\n  2\nBLOCKS\n  0\nBLOCK\n  8\n0\n  2\nPIECE_1\n 70\n0\n 10\n0\n 20\n0\n",
    );
    s.push_str("  0\nPOLYLINE\n  8\n1\n 66\n1\n 70\n1\n");
    for (x, y) in [(0, 0), (100, 0), (100, 50), (0, 50)] {
        s.push_str(&format!("  0\nVERTEX\n  8\n1\n 10\n{x}\n 20\n{y}\n"));
    }
    s.push_str("  0\nSEQEND\n  8\n1\n");
    s.push_str("  0\nLINE\n  8\n4\n 10\n30\n 20\n0\n 11\n30\n 21\n5\n");
    s.push_str("  0\nLINE\n  8\n6\n 10\n20\n 20\n25\n 11\n80\n 21\n25\n");
    s.push_str("  0\nCIRCLE\n  8\n13\n 10\n50\n 20\n25\n 40\n3\n");
    s.push_str("  0\nTEXT\n  8\n84\n 10\n0\n 20\n0\n 40\n4\n  1\nPiece Name: Manche\n");
    s.push_str("  0\nENDBLK\n  8\n0\n  0\nENDSEC\n");
    s.push_str("  0\nSECTION\n  2\nENTITIES\n  0\nINSERT\n  8\n0\n  2\nPIECE_1\n 10\n0\n 20\n0\n");
    s.push_str("  0\nENDSEC\n  0\nEOF\n");
    s
}

#[test]
fn us122_import_aama_apercu_rapport_une_etape_annulable() {
    let mut h = harness();
    let project_before = h.state().editor.project().clone();
    let (dir, path) = dxf_temp("aama", aama_fixture().as_bytes());
    h.get_by_label("Fichier").click();
    h.run_steps(3);
    h.get_by_label_contains("Importer des pièces d'un DXF de type AAMA");
    h.key_press(Key::Escape);
    h.run_steps(3);
    h.state_mut().load_aama_import(&path);
    h.run_steps(4);
    assert!(h.state().aama_import.is_some());
    // Aperçu avant import : pièce, cran, droit-fil, et ce qui n'est pas importé (compté).
    h.get_by_label_contains("1 pièce(s) créée(s), 1 cran(s)");
    h.get_by_label_contains("Manche : 4 sommet(s), 1 cran(s), droit-fil : oui");
    h.get_by_label_contains("1 × CIRCLE");
    h.get_by_label_contains("n'est pas lu");
    assert_eq!(
        *h.state().editor.project(),
        project_before,
        "aperçu sans trace"
    );
    assert!(!h.state().editor.doc.can_undo());
    h.get_by_label("Importer les pièces").click();
    h.run_steps(4);
    assert!(h.state().aama_import.is_none(), "fenêtre fermée");
    assert_eq!(h.state().editor.project().pieces.len(), 1);
    assert_eq!(h.state().editor.project().pieces[0].name, "Manche");
    assert!(h.state_mut().editor.doc.undo(), "un pas d'annulation");
    assert!(!h.state().editor.doc.can_undo(), "un seul pas");
    assert_eq!(*h.state().editor.project(), project_before);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn us122_import_aama_fichier_tronque_refuse_sans_fenetre_ni_trace() {
    let mut h = harness();
    let project_before = h.state().editor.project().clone();
    let text = aama_fixture();
    let (dir, path) = dxf_temp("aama-tronque", &text.as_bytes()[..text.len() - 5]);
    h.state_mut().load_aama_import(&path);
    h.run_steps(3);
    assert!(h.state().aama_import.is_none());
    assert_eq!(*h.state().editor.project(), project_before);
    let _ = std::fs::remove_dir_all(dir);
}

// ===== US-11.2 / US-11.3 : nomenclature et fiche technique ========================================

struct BomLabels {
    file_menu: &'static str,
    menu_item: &'static str,
    intro: &'static str,
    readonly: &'static str,
    variants: &'static str,
    export_csv: &'static str,
    export_pdf: &'static str,
    export_tech: &'static str,
    export_note: &'static str,
    note_label: &'static str,
    unset: &'static str,
    csv_head: &'static str,
    csv_row: &'static str,
    csv_done: &'static str,
    tech_title: &'static str,
}

fn bom_walk(l: BomLabels) {
    use pf_core::{Command, Document};
    use pf_export::bom_export::{export_bom, export_tech_sheet};
    use pf_export::report::ReportFormat;
    use std::path::PathBuf;
    let (mut project, range) = crate::stages::mesures_tailles_project().unwrap();
    let first = project.pieces[0].id;
    let name = project.pieces[0].name.clone();
    project
        .apply(Command::SetPieceCard {
            piece: first,
            name,
            code: None,
            quantity: 2,
            mirrored_pair: true,
            material: Some("Cuir".into()),
            category: None,
            comment: Some("Coudre a plat".into()),
        })
        .unwrap();
    let before = pf_core::persist::to_json(&project).unwrap();
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!(
        "../../target/us112-nomenclature-{}",
        pf_i18n::lang().code()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut h = harness();
    h.state_mut().editor = crate::editor::Editor::new(Document::new(project));
    h.state_mut().fit_pending = true;
    h.run_steps(4);

    h.get_by_label(l.file_menu).click_accesskit();
    h.run_steps(3);
    h.get_by_label(l.menu_item).click_accesskit();
    h.run_steps(4);
    for label in [
        l.intro,
        l.readonly,
        l.variants,
        l.export_csv,
        l.export_pdf,
        l.export_tech,
        l.export_note,
        l.note_label,
    ] {
        h.get_by_label(label);
    }
    // Matière non renseignée : une ligne de pièce et une ligne de total au moins.
    assert!(h.get_all_by_label(l.unset).count() >= 2);
    let win = h
        .ctx
        .memory(|m| m.area_rect(crate::dock::ToolWin::Bom.id()))
        .expect("fenêtre Nomenclature");
    assert!(
        h.state().dock.rect.contains_rect(win),
        "{win:?} hors du dock"
    );
    assert!(!win.intersects(h.state().canvas_rect));

    // CSV : octets du module d'export, mention de lecture seule, rien d'autre d'écrit.
    let csv_path = dir.join("nomenclature.csv");
    let r = Some(range);
    assert!(h
        .state_mut()
        .write_bom_to(r, None, ReportFormat::Csv, None, &csv_path));
    let bytes = std::fs::read(&csv_path).unwrap();
    let text = String::from_utf8(bytes.clone()).unwrap();
    let lines: Vec<&str> = text.trim_start_matches('\u{FEFF}').lines().collect();
    assert!(lines.contains(&l.csv_head), "{text}");
    assert!(lines.iter().any(|x| x.starts_with(l.csv_row)), "{text}");
    let direct = export_bom(
        h.state().editor.project(),
        r,
        ReportFormat::Csv,
        pf_i18n::lang(),
    )
    .unwrap();
    assert_eq!(bytes, direct.bytes);
    assert!(h.state().editor.status.text.contains(l.csv_done));
    assert!(!h.state().editor.status.error);

    // PDF de nomenclature et fiche technique avec une note.
    let pdf_path = dir.join("nomenclature.pdf");
    assert!(h
        .state_mut()
        .write_bom_to(r, None, ReportFormat::Pdf, None, &pdf_path));
    let direct = export_bom(
        h.state().editor.project(),
        r,
        ReportFormat::Pdf,
        pf_i18n::lang(),
    )
    .unwrap();
    assert_eq!(std::fs::read(&pdf_path).unwrap(), direct.bytes);
    let tech_path = dir.join("fiche.pdf");
    assert!(h
        .state_mut()
        .write_bom_to(r, None, ReportFormat::Pdf, Some("Note ici"), &tech_path));
    let tech = std::fs::read(&tech_path).unwrap();
    let direct = export_tech_sheet(
        h.state().editor.project(),
        r,
        Some("Note ici"),
        pf_i18n::lang(),
    )
    .unwrap();
    assert_eq!(tech, direct.bytes);
    assert!(tech.starts_with(b"%PDF"));
    let _ = l.tech_title;

    let mut found: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    found.sort();
    assert_eq!(found, ["fiche.pdf", "nomenclature.csv", "nomenclature.pdf"]);
    assert_eq!(
        pf_core::persist::to_json(h.state().editor.project()).unwrap(),
        before,
        "projet modifié"
    );

    // Destination illisible et gamme disparue : refus propres, aucun fichier.
    assert!(h.state().error_dialog.is_none());
    assert!(!h.state_mut().write_bom_to(
        r,
        None,
        ReportFormat::Csv,
        None,
        &dir.join("absent").join("x.csv")
    ));
    assert!(h.state().error_dialog.is_some());
    h.state_mut().error_dialog = None;
    assert!(!h.state_mut().write_bom_to(
        Some(pf_core::SizeRangeId(9999)),
        None,
        ReportFormat::Csv,
        None,
        &dir.join("inconnue.csv")
    ));
    assert!(h.state().error_dialog.is_some());
    assert!(!dir.join("inconnue.csv").exists());
}

#[test]
fn us112_fr_nomenclature_et_fiche_technique() {
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
    bom_walk(BomLabels {
        file_menu: "Fichier",
        menu_item: "Nomenclature et fiche technique…",
        intro: "Pièces, matières, découpes et surface nette par taille, calculées à la lecture.",
        readonly: "Lecture seule : rien n'est enregistré dans le projet.",
        variants: "Par variante : tout le projet par défaut ; les variantes se définissent dans la fenêtre « Variantes… ».",
        export_csv: "Nomenclature CSV…",
        export_pdf: "Nomenclature PDF…",
        export_tech: "Fiche technique PDF…",
        export_note: "Export : le projet n'est pas modifié, seul le fichier choisi est écrit.",
        note_label: "Note de la fiche technique (non enregistrée)",
        unset: "non renseignée",
        csv_head: "#piece;Pièce;Code;Feuille;Matière;Catégorie;Quantité;Paire miroir;Découpes;Aire XS (mm²);Aire S (mm²);Aire M (mm²);Aire L (mm²);Aire XL (mm²)",
        csv_row: "piece;",
        csv_done: "CSV exporté",
        tech_title: "Fiche technique",
    });
}

#[test]
fn us112_en_bill_of_materials_and_technical_sheet() {
    pf_i18n::set_lang(pf_i18n::Lang::En);
    bom_walk(BomLabels {
        file_menu: "File",
        menu_item: "Bill of materials and technical sheet…",
        intro: "Pieces, materials, cuts and net surface by size, computed on reading.",
        readonly: "Read-only: nothing is saved in the project.",
        variants: "By variant: whole project by default; variants are defined in the “Variants…” window.",
        export_csv: "Bill of materials CSV…",
        export_pdf: "Bill of materials PDF…",
        export_tech: "Technical sheet PDF…",
        export_note: "Export: the project is not modified, only the chosen file is written.",
        note_label: "Technical sheet note (not saved)",
        unset: "not set",
        csv_head: "#piece;Piece;Code;Sheet;Material;Category;Quantity;Mirrored pair;Cuts;Area XS (mm²);Area S (mm²);Area M (mm²);Area L (mm²);Area XL (mm²)",
        csv_row: "piece;",
        csv_done: "CSV exported",
        tech_title: "Technical sheet",
    });
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
}

// ---------------------------------------------------------------------------
// US-11.1 variantes
// ---------------------------------------------------------------------------

struct VarLabels {
    window: &'static str,
    list: &'static str,
    options: &'static str,
    entries: &'static str,
    option_on: &'static str,
    option_off: &'static str,
    add: &'static str,
    duplicate: &'static str,
    remove: &'static str,
    copy_name: &'static str,
    exists: &'static str,
    qty_error: &'static str,
    qty_range: &'static str,
    bom_variant: &'static str,
    bom_all: &'static str,
}

fn variants_walk(l: VarLabels) {
    let mut h = bareme_harness("variantes");
    // La fenêtre est dans le dock, sans recouvrir le canevas.
    h.get_by_label(l.window);
    h.get_by_label(l.list);
    h.get_by_label(l.options);
    h.get_by_label(l.entries);
    let win = h
        .ctx
        .memory(|m| m.area_rect(crate::dock::ToolWin::Variants.id()))
        .expect("fenêtre Variantes");
    assert!(
        h.state().dock.rect.contains_rect(win),
        "{win:?} hors du dock"
    );
    assert!(!win.intersects(h.state().canvas_rect));
    let before = h.state().editor.project().clone();
    assert_eq!(before.variants.len(), 2);
    // 1. Désactiver l'option (l'état est dit en toutes lettres, jamais par la seule case) : une
    // commande, une annulation.
    h.get_by_label(l.option_on).click();
    h.run_steps(3);
    assert!(h.state().editor.project().variants[0]
        .options_active
        .is_empty());
    h.get_by_label(l.option_off);
    undo(&mut h);
    assert_eq!(*h.state().editor.project(), before, "une seule annulation");
    h.run_steps(3);
    // 2. Dupliquer : une copie sélectionnée, indépendante ; une annulation.
    h.get_all_by_label(l.duplicate)
        .next()
        .expect("Dupliquer")
        .click();
    h.run_steps(3);
    {
        let p = h.state().editor.project();
        assert_eq!(p.variants.len(), 3);
        assert_eq!(p.variants[2].name, l.copy_name);
        // Mêmes entrées (hors identifiants : la copie en reçoit de neufs, schéma 6).
        let strip = |v: &[pf_core::VariantEntry]| {
            v.iter()
                .map(|e| (e.piece, e.quantity, e.material.clone(), e.option.clone()))
                .collect::<Vec<_>>()
        };
        assert_eq!(strip(&p.variants[2].entries), strip(&p.variants[0].entries));
        assert_ne!(p.variants[2].id, p.variants[0].id);
    }
    undo(&mut h);
    assert_eq!(*h.state().editor.project(), before);
    h.run_steps(3);
    // 3. Nom déjà pris (casse ignorée) : refus du cœur, rien n'est modifié, raison affichée.
    // Champs du dock : 0 nom de la variante, 1 nouvelle variante, 2 nouvelle option, puis une
    // quantité et une matière par entrée, puis la quantité à ajouter.
    var_type(&mut h, 1, "gilet");
    h.get_all_by_label(l.add).next().expect("Ajouter").click();
    h.run_steps(3);
    assert_eq!(*h.state().editor.project(), before);
    assert!(h.get_all_by_label_contains(l.exists).count() >= 1);
    // 4. Quantité d'une entrée : valide = une étape annulable ; 0 refusé par le cœur ; « abc »
    // refusé par l'interface ; rien n'est modifié.
    var_type(&mut h, 3, "5");
    assert_eq!(
        h.state().editor.project().variants[0].entries[0].quantity,
        5
    );
    undo(&mut h);
    assert_eq!(*h.state().editor.project(), before);
    var_type(&mut h, 3, "0");
    assert_eq!(*h.state().editor.project(), before);
    assert!(h.get_all_by_label_contains(l.qty_range).count() >= 1);
    var_type(&mut h, 3, "abc");
    assert_eq!(*h.state().editor.project(), before);
    h.get_by_label(l.qty_error);
    // 5. Supprimer la première variante : une annulation.
    h.get_all_by_label(l.remove)
        .next()
        .expect("Supprimer")
        .click();
    h.run_steps(3);
    assert_eq!(h.state().editor.project().variants.len(), 1);
    undo(&mut h);
    assert_eq!(*h.state().editor.project(), before);
    h.run_steps(3);
    // 6. Nomenclature : sélecteur de variante (défaut = tout le projet, jamais suivi).
    h.state_mut().variants = None;
    h.state_mut().open_bom();
    h.run_steps(4);
    h.get_by_label(l.bom_variant);
    assert!(h.state().bom.as_ref().unwrap().variant.is_none());
    h.get_by_label(l.bom_all);
    h.get_by_role_and_label(egui::accesskit::Role::Button, "Chemise")
        .click_accesskit();
    h.run_steps(4);
    let chosen = h.state().bom.as_ref().unwrap().variant;
    assert_eq!(chosen, Some(before.variants[0].id));
    // L'option d'une entrée est écrite avec son nom, entre crochets.
    let devant = before.pieces[0].name.clone();
    h.get_by_label(&format!("{devant} [avec poche]"));
    assert_eq!(*h.state().editor.project(), before, "lecture seule");
    // Export CSV de la variante : octets du module d'export, projet inchangé.
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!(
        "../../target/us111-variantes-{}",
        pf_i18n::lang().code()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("nomenclature.csv");
    assert!(h.state_mut().write_bom_to(
        None,
        chosen,
        pf_export::report::ReportFormat::Csv,
        None,
        &path
    ));
    let direct = pf_export::bom_export::export_bom_variant(
        h.state().editor.project(),
        None,
        chosen,
        pf_export::report::ReportFormat::Csv,
        pf_i18n::lang(),
    )
    .unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), direct.bytes);
    assert_eq!(*h.state().editor.project(), before);
}

#[test]
fn us111_fr_variantes_fenetre_gestes_refus_une_annulation_et_nomenclature() {
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
    variants_walk(VarLabels {
        window: "Variantes",
        list: "Variantes (2)",
        options: "Options de « Chemise » (1)",
        entries: "Pièces de « Chemise » (2)",
        option_on: "avec poche : active",
        option_off: "avec poche : inactive",
        add: "Ajouter",
        duplicate: "Dupliquer",
        remove: "Supprimer",
        copy_name: "Chemise (copie)",
        exists: "existe déjà",
        qty_error: "Quantité invalide : « abc » (entier de 1 à 999).",
        qty_range: "entre 1 et 999",
        bom_variant: "Variante :",
        bom_all: "Tout le projet",
    });
}

#[test]
fn us111_en_variants_window_gestures_refusals_one_undo_and_bill_of_materials() {
    pf_i18n::set_lang(pf_i18n::Lang::En);
    variants_walk(VarLabels {
        window: "Variants",
        list: "Variants (2)",
        options: "Options of “Chemise” (1)",
        entries: "Pieces of “Chemise” (2)",
        option_on: "avec poche : active",
        option_off: "avec poche : inactive",
        add: "Add",
        duplicate: "Duplicate",
        remove: "Delete",
        copy_name: "Chemise (copy)",
        exists: "existe déjà",
        qty_error: "Invalid quantity: “abc” (integer from 1 to 999).",
        qty_range: "entre 1 et 999",
        bom_variant: "Variant:",
        bom_all: "Whole project",
    });
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
}

// ----- US-12.4 : import SVG et PDF vectoriel ---------------------------------------------------

const VIMP_SVG: &str = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"100mm\" height=\"50mm\" \
    viewBox=\"0 0 100 50\"><rect x=\"10\" y=\"5\" width=\"20\" height=\"10\"/><text x=\"1\" y=\"1\">t</text></svg>";

fn vimp_temp(name: &str, file: &str, bytes: &[u8]) -> (std::path::PathBuf, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("pf-us124-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(file);
    std::fs::write(&path, bytes).unwrap();
    (dir, path)
}

/// PDF minimal : un flux de contenu par page (72 pt = 25,4 mm).
fn vimp_pdf(pages: &[&str], trailer: &str) -> Vec<u8> {
    let kids: Vec<String> = (0..pages.len())
        .map(|i| format!("{} 0 R", 3 + 2 * i))
        .collect();
    let mut s = format!(
        "%PDF-1.4\n1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n2 0 obj\n<< /Type /Pages /Kids [{}] /Count {} >>\nendobj\n",
        kids.join(" "),
        pages.len()
    );
    for (i, c) in pages.iter().enumerate() {
        s += &format!(
            "{} 0 obj\n<< /Type /Page /Parent 2 0 R /Contents {} 0 R >>\nendobj\n{} 0 obj\n<< /Length {} >>\nstream\n{c}\nendstream\nendobj\n",
            3 + 2 * i,
            4 + 2 * i,
            4 + 2 * i,
            c.len()
        );
    }
    s += &format!("trailer\n<< /Size 9 /Root 1 0 R {trailer} >>\n%%EOF\n");
    s.into_bytes()
}

#[test]
fn us124_import_svg_apercu_unite_echelle_une_etape_annulable() {
    let mut h = harness();
    let before = h.state().editor.project().clone();
    let (dir, path) = vimp_temp("a", "entree.svg", VIMP_SVG.as_bytes());
    // Menu : l'entrée existe.
    h.get_by_label("Fichier").click();
    h.run_steps(3);
    h.get_by_label("Importer un SVG / PDF…");
    h.key_press(Key::Escape);
    h.run_steps(3);
    h.state_mut().load_vector_import(&path);
    h.run_steps(4);
    assert!(h.state().vector_import.is_some());
    // Rectangle 20 x 10 dans un viewBox 100 x 50 déclaré en mm : exactement 20 x 10 mm.
    h.get_by_label_contains("Dimensions : 20,0 × 10,0 mm");
    h.get_by_label("4 objet(s) à créer");
    h.get_by_label_contains("rect 1");
    h.get_by_label_contains("text : 1 - type non pris en charge");
    h.get_by_label_contains("Ellipses et arcs elliptiques : polylignes, écart maximal 0,05 mm.");
    h.get_by_label("Unité déclarée par le fichier (1 unité = 1,0 mm)");
    // Pouce : 20 in = 508 mm ; l'aperçu est recalculé.
    h.get_by_label("Pouce (in, 25,4 mm)").click();
    h.run_steps(3);
    h.get_by_label_contains("Dimensions : 508,0 × 254,0 mm");
    h.get_by_label("Unité déclarée par le fichier (1 unité = 1,0 mm)")
        .click();
    h.run_steps(3);
    // Échelle 50 % : la moitié ; échelle illisible : message, rien d'importable.
    h.state_mut().vector_import.as_mut().unwrap().scale_text = "50".into();
    h.run_steps(3);
    h.get_by_label_contains("Dimensions : 10,0 × 5,0 mm");
    h.state_mut().vector_import.as_mut().unwrap().scale_text = "abc".into();
    h.run_steps(3);
    h.get_by_label_contains("Échelle invalide");
    assert!(
        !h.state_mut().run_vector_import(),
        "pas d'import sans échelle"
    );
    assert_eq!(*h.state().editor.project(), before);
    h.state_mut().vector_import.as_mut().unwrap().scale_text = "100".into();
    h.run_steps(3);
    // Nouvelle feuille (défaut) : une seule commande, un seul pas d'annulation.
    assert!(!h.state().editor.doc.can_undo());
    let sheets = before.sheets.len();
    h.get_by_label("Importer").click();
    h.run_steps(4);
    assert!(h.state().vector_import.is_none(), "fenêtre fermée");
    assert_eq!(h.state().editor.project().sheets.len(), sheets + 1);
    assert_eq!(h.state().editor.sheet().name, "entree");
    assert_eq!(h.state().editor.sheet().entities.len(), 4);
    assert!(h
        .state()
        .editor
        .status
        .text
        .contains("4 objet(s) importé(s) depuis entree.svg"));
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run_steps(3);
    assert_eq!(
        *h.state().editor.project(),
        before,
        "un seul Ctrl+Z défait la feuille et l'import"
    );
    // Feuille existante : le contenu s'ajoute à la feuille choisie.
    h.state_mut().load_vector_import(&path);
    h.run_steps(4);
    h.get_by_label("Feuille existante").click();
    h.run_steps(3);
    let target = h.state().editor.sheet;
    h.get_by_label("Importer").click();
    h.run_steps(4);
    assert_eq!(h.state().editor.project().sheets.len(), sheets);
    assert_eq!(
        h.state()
            .editor
            .project()
            .sheet(target)
            .unwrap()
            .entities
            .len(),
        4
    );
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run_steps(3);
    assert_eq!(*h.state().editor.project(), before);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn us124_import_pdf_page_choisie_et_refus_sans_trace() {
    let mut h = harness();
    let before = h.state().editor.project().clone();
    let pdf = vimp_pdf(&["0 0 m 72 0 l S", "0 0 m 144 0 l S BT (x) Tj ET"], "");
    let (dir, path) = vimp_temp("p", "entree.pdf", &pdf);
    h.state_mut().load_vector_import(&path);
    h.run_steps(4);
    h.get_by_label("Fichier : entree.pdf - PDF vectoriel, 2 page(s)");
    h.get_by_label("Page (sur 2)");
    h.get_by_label_contains("Dimensions : 25,4 × 0,0 mm");
    h.state_mut().vector_import.as_mut().unwrap().page = 2;
    h.run_steps(3);
    h.get_by_label_contains("Dimensions : 50,8 × 0,0 mm");
    h.get_by_label_contains("text : 1 - type non pris en charge");
    assert!(h.state_mut().run_vector_import());
    assert_eq!(h.state().editor.sheet().entities.len(), 1);
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run_steps(3);
    assert_eq!(*h.state().editor.project(), before);
    // Refus clairs : aucune fenêtre, erreur affichée, projet intact.
    let enc = vimp_pdf(&["0 0 m 1 1 l S"], "/Encrypt 9 0 R");
    let cut = &pdf[..pdf.windows(7).position(|w| w == b"trailer").unwrap()];
    for (name, file, bytes, expect) in [
        (
            "enc",
            "chiffre.pdf",
            enc,
            "PDF chiffré : non pris en charge",
        ),
        ("cut", "coupe.pdf", cut.to_vec(), "PDF tronqué ou incomplet"),
        (
            "xml",
            "casse.svg",
            b"<svg><g><line x2=\"1\"/></svg>".to_vec(),
            "SVG mal formé",
        ),
        ("txt", "autre.svg", b"<html/>".to_vec(), "n'est pas un SVG"),
    ] {
        let (d2, p2) = vimp_temp(name, file, &bytes);
        h.state_mut().load_vector_import(&p2);
        h.run_steps(3);
        assert!(h.state().vector_import.is_none(), "{name}");
        assert!(h.state().editor.status.error, "{name}");
        assert!(
            h.state().editor.status.text.contains(expect),
            "{name} : {}",
            h.state().editor.status.text
        );
        assert_eq!(*h.state().editor.project(), before, "{name}");
        let _ = std::fs::remove_dir_all(&d2);
    }
    h.state_mut()
        .load_vector_import(std::path::Path::new("Z:/inexistant/rien.svg"));
    h.run_steps(3);
    assert!(h.state().vector_import.is_none());
    assert!(h.state().error_dialog.is_some());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn us124_fenetre_en_anglais() {
    pf_i18n::set_lang(pf_i18n::Lang::En);
    let mut h = harness();
    let (dir, path) = vimp_temp("en", "entree.svg", VIMP_SVG.as_bytes());
    h.get_by_label("File").click();
    h.run_steps(3);
    h.get_by_label("Import an SVG / PDF…");
    h.key_press(Key::Escape);
    h.run_steps(3);
    h.state_mut().load_vector_import(&path);
    h.run_steps(4);
    h.get_by_label_contains("Dimensions: 20.0 × 10.0 mm");
    h.get_by_label("4 object(s) to create");
    h.get_by_label_contains("text: 1 - unsupported type");
    h.get_by_label("Unit declared by the file (1 unit = 1.0 mm)");
    h.get_by_label("Inch (in, 25.4 mm)");
    h.get_by_label("New sheet");
    h.get_by_label("Elements not imported");
    h.state_mut().vector_import.as_mut().unwrap().scale_text = "abc".into();
    h.run_steps(3);
    h.get_by_label_contains("Invalid scale");
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn us124_echelle_saisie() {
    use crate::vector_import::parse_scale;
    assert_eq!(parse_scale("100"), Some(1.0));
    assert_eq!(parse_scale(" 50,5 % "), Some(0.505));
    assert_eq!(parse_scale("0.1"), Some(0.001));
    for bad in ["", "abc", "0", "-5", "0,05", "100001", "NaN", "inf"] {
        assert_eq!(parse_scale(bad), None, "{bad}");
    }
}

#[path = "us165b_tests.rs"]
mod us165b;
