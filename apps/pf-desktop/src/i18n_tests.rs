//! Tests de l'internationalisation de l'interface : clés utilisées présentes
//! dans les deux catalogues, aucun texte français en dur, bascule de langue
//! sur la vraie boucle egui et parcours complet de l'interface en anglais.

use crate::app::{App, Pending};
use crate::editor::Tool;
use crate::Launch;
use eframe::egui;
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use pf_geometry::PointMm;
use pf_i18n::{set_lang, Lang};
use std::collections::BTreeSet;
use std::path::PathBuf;

// ----- lecture des sources ---------------------------------------------------

/// Sources de l'application hors tests : tous les `.rs` de `src/` sauf les
/// modules de tests, coupés avant leur module `#[cfg(test)] mod tests`.
/// (Lecture du dossier plutôt qu'une liste figée : un nouveau fichier source
/// est contrôlé automatiquement.)
fn app_sources() -> Vec<(String, String)> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        panic!("dossier des sources illisible : {}", dir.display());
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if path.extension().is_none_or(|e| e != "rs")
            || name == "i18n_tests.rs"
            || name == "ui_tests.rs"
            || name == "ph2_tests.rs"
            || name == "ph2b_tests.rs"
            || name == "ph2c_tests.rs"
            || name == "ph6_tests.rs"
            || name == "us114_tests.rs"
            || name == "us115_tests.rs"
            || name == "us172_tests.rs"
            || name == "us136_tests.rs"
            || name == "us136_adverse_tests.rs"
            || name == "us163_tests.rs"
            || name == "us165_tests.rs"
            || name == "us165b_tests.rs"
            || name == "us168_tests.rs"
            || name == "us151_tests.rs"
            || name == "us154_tests.rs"
            || name == "us141_tests.rs"
            || name == "us142_tests.rs"
        {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            panic!("source illisible : {}", path.display());
        };
        let text = text.replace("\r\n", "\n");
        let code = match text.find("#[cfg(test)]\nmod tests {") {
            Some(i) => text[..i].to_string(),
            None => text,
        };
        files.push((name, code));
    }
    files.sort();
    files
}

/// Texte d'une ligne sans son commentaire de fin (`// …`).
fn code_part(line: &str) -> &str {
    match line.find("//") {
        Some(i) => &line[..i],
        None => line,
    }
}

/// Clés passées en littéral à `tr("…")` / `tr_args("…"`, retours à la ligne permis.
fn used_keys(code: &str) -> BTreeSet<String> {
    let mut keys = BTreeSet::new();
    let no_comments: String = code.lines().map(code_part).collect::<Vec<_>>().join("\n");
    for call in ["tr(", "tr_args("] {
        let mut from = 0;
        while let Some(found) = no_comments[from..].find(call) {
            let at = from + found;
            from = at + call.len();
            let before = no_comments[..at].chars().next_back();
            if before.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                continue; // `str(`, `my_tr(`…
            }
            let rest = no_comments[from..].trim_start();
            if let Some(after_quote) = rest.strip_prefix('"') {
                if let Some(end) = after_quote.find('"') {
                    keys.insert(after_quote[..end].to_string());
                }
            }
        }
    }
    keys
}

#[test]
fn toutes_les_cles_utilisees_existent_dans_les_deux_catalogues() {
    let mut used = BTreeSet::new();
    for (_, code) in app_sources() {
        used.extend(used_keys(&code));
    }
    assert!(
        used.len() > 100,
        "scan des clés défaillant : {}",
        used.len()
    );
    for lang in Lang::ALL {
        let catalog: BTreeSet<&str> = lang.catalog().iter().map(|(k, _)| *k).collect();
        let missing: Vec<_> = used
            .iter()
            .filter(|k| !catalog.contains(k.as_str()))
            .collect();
        assert!(
            missing.is_empty(),
            "clés utilisées absentes du catalogue {} : {missing:?}",
            lang.code()
        );
    }
    // Information seulement : clés de catalogue jamais utilisées.
    let unused: Vec<_> = Lang::Fr
        .catalog()
        .iter()
        .map(|(k, _)| *k)
        .filter(|k| !used.contains(*k))
        .collect();
    if !unused.is_empty() {
        eprintln!("clés de catalogue jamais utilisées : {unused:?}");
    }
}

#[test]
fn le_scanner_de_cles_gere_les_appels_sur_plusieurs_lignes() {
    let code = "let a = tr(\"a.un\");\nlet b = tr_args(\n    \"b.deux\",\n    &[]);\n// tr(\"c.trois\")\nlet d = str(\"d.quatre\");";
    let keys = used_keys(code);
    assert_eq!(
        keys.into_iter().collect::<Vec<_>>(),
        vec!["a.un".to_string(), "b.deux".to_string()]
    );
}

#[test]
fn aucun_texte_francais_connu_ecrit_en_dur() {
    const SAMPLES: &[&str] = &[
        "Fichier",
        "FEUILLES DU PROJET",
        "Sélection",
        "Annuler",
        "Rétablir",
        "Nouveau projet",
        "Enregistrer",
        "Supprimer",
        "INSPECTEUR",
        "Longueur",
        "Périmètre",
        "Échap",
        "Cadrer",
        "Quitter",
        "Affichage",
        "Perpendiculaire",
        "Mesurer",
        "Courbe",
        "Feuille",
        "Raccourcis",
        "Modifications non enregistrées",
        "TRACÉ PRÉCIS",
        "Impossible",
        "tracé(s)",
        "Parallèle",
        "Droit-fil",
        "Transformer",
        "Dupliquer",
        "Appliquer",
        "Pivoter",
        "Symétrie",
        "Posé sur",
        "Crans",
        "Cran double",
        "Largeur",
    ];
    let mut offenders = Vec::new();
    for (file, code) in app_sources() {
        for (n, line) in code.lines().enumerate() {
            let code = code_part(line);
            for sample in SAMPLES {
                if code.contains(*sample) {
                    offenders.push(format!("{file}:{} contient « {sample} »", n + 1));
                }
            }
            // Filet plus large : un littéral avec une lettre accentuée française.
            if code.contains('"') && code.chars().any(|c| "éèêëàâùûçôîïœÉÈÀÇ".contains(c))
            {
                offenders.push(format!(
                    "{file}:{} littéral accentué : {}",
                    n + 1,
                    code.trim()
                ));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "textes français en dur :\n{}",
        offenders.join("\n")
    );
}

// ----- interface réelle --------------------------------------------------------

fn harness_with(launch: Launch) -> Harness<'static, App> {
    let app = App::with_context(&egui::Context::default(), launch);
    let mut h = Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .build_state(|ctx, app: &mut App| app.ui(ctx), app);
    h.run_steps(4);
    h
}

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

#[test]
fn le_titre_de_section_suit_la_langue() {
    set_lang(Lang::Fr);
    let mut h = harness_with(Launch::default());
    h.get_by_label("FEUILLES DU PROJET");
    assert!(h.query_by_label("PROJECT SHEETS").is_none());

    set_lang(Lang::En);
    h.run_steps(3);
    h.get_by_label("PROJECT SHEETS");
    assert!(
        h.query_by_label("FEUILLES DU PROJET").is_none(),
        "l'ancien libellé français doit avoir disparu"
    );
    h.get_by_label("INSPECTOR");

    set_lang(Lang::Fr);
    h.run_steps(3);
    h.get_by_label("FEUILLES DU PROJET");
    assert!(h.query_by_label("PROJECT SHEETS").is_none());
    h.get_by_label("INSPECTEUR");
}

#[test]
fn le_lancement_en_anglais_traduit_aussi_les_valeurs_par_defaut() {
    let h = harness_with(Launch {
        lang: Some(Lang::En),
        ..Launch::default()
    });
    assert_eq!(pf_i18n::lang(), Lang::En);
    assert_eq!(h.state().editor.project().name, "New project");
    h.get_by_label("PROJECT SHEETS");
    assert_eq!(pf_core::input::decimal_separator(), '.');
}

#[test]
fn le_menu_langue_bascule_toute_l_interface() {
    set_lang(Lang::Fr);
    let mut h = harness_with(Launch::default());
    h.get_by_label("Affichage").click();
    h.run_steps(3);
    h.get_by_label_contains("Langue").click();
    h.run_steps(3);
    h.get_by_label("English").click();
    h.run_steps(4);
    assert_eq!(pf_i18n::lang(), Lang::En);
    h.get_by_label("PROJECT SHEETS");
    assert_eq!(pf_core::input::decimal_separator(), '.');
    set_lang(Lang::Fr);
}

#[test]
fn le_choix_de_langue_est_memorise_hors_projet() {
    let dir = std::env::temp_dir().join(format!("pf-desktop-i18n-{}", std::process::id()));
    let file = dir.join("sous-dossier").join("settings.json");
    let mut h = harness_with(Launch::default());
    h.state_mut().settings_path = Some(file.clone());
    h.state_mut().set_language(Lang::En);
    assert_eq!(pf_i18n::settings::load_language(&file), Lang::En);
    assert_eq!(h.state().editor.status.text, "Language: English.");
    h.state_mut().set_language(Lang::Fr);
    assert_eq!(pf_i18n::settings::load_language(&file), Lang::Fr);
    assert_eq!(h.state().editor.status.text, "Langue : français.");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn une_ecriture_impossible_du_reglage_est_signalee_sans_panique() {
    let dir = std::env::temp_dir().join(format!("pf-desktop-i18n-ko-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    assert!(std::fs::create_dir_all(&dir).is_ok());
    // Le « fichier » de réglages est un dossier existant : l'écriture échoue.
    let blocked = dir.join("settings.json");
    assert!(std::fs::create_dir_all(&blocked).is_ok());
    let mut h = harness_with(Launch::default());
    h.state_mut().settings_path = Some(blocked);
    h.state_mut().set_language(Lang::En);
    assert!(
        h.state().editor.status.error,
        "{}",
        h.state().editor.status.text
    );
    assert_eq!(pf_i18n::lang(), Lang::En, "la langue change malgré l'échec");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Construit un tracé riche : segments, perpendiculaire, points construits
/// (distance, milieu, intersection), courbe et contour fermé.
fn populate(h: &mut Harness<'static, App>) {
    const TOL: f64 = 2.0;
    let ed = &mut h.state_mut().editor;
    ed.snap_to_paths = false;
    ed.set_tool(Tool::Segment);
    ed.click(pt(0.0, 0.0), TOL, false);
    ed.length_text = "120".into();
    ed.angle_text = "0".into();
    ed.confirm();
    ed.set_tool(Tool::Perpendicular);
    ed.click(pt(60.0, 0.3), TOL, false);
    ed.click(pt(120.0, 0.0), TOL, false);
    ed.pointer_moved(pt(130.0, 30.0));
    ed.length_text = "80".into();
    ed.confirm();
    ed.set_tool(Tool::Segment);
    ed.click(pt(40.0, -30.0), TOL, false);
    ed.click(pt(40.0, 50.0), TOL, false);
    ed.snap_to_paths = true;
    ed.set_tool(Tool::Point);
    ed.click(pt(40.2, 0.3), TOL, false); // intersection
    ed.click(pt(60.2, 0.3), TOL, false); // milieu
    ed.click(pt(87.4, 0.8), TOL, false); // sur le tracé
    ed.set_tool(Tool::PointOnSegment);
    ed.click(pt(80.0, 0.3), TOL, false);
    ed.length_text = "42".into();
    ed.confirm();
    ed.set_tool(Tool::Curve);
    ed.click(pt(0.0, 100.0), TOL, false);
    ed.click(pt(100.0, 100.0), TOL, false);
    ed.click(pt(150.0, 100.0), TOL, false);
    ed.set_tool(Tool::Spline);
    for (x, y) in [(0.0, 150.0), (50.0, 170.0), (100.0, 140.0)] {
        ed.click(pt(x, y), TOL, false);
    }
    ed.confirm();
    ed.set_tool(Tool::Notch);
    ed.click(pt(30.0, 0.2), TOL, false);
    ed.length_text = "30".into();
    ed.confirm();
    ed.set_tool(Tool::Dimension);
    ed.click(pt(0.0, 0.0), TOL, false);
    ed.click(pt(120.0, 0.0), TOL, false);
    ed.set_tool(Tool::Select);
}

#[test]
fn parcours_de_l_interface_en_anglais_sans_panique() {
    set_lang(Lang::En);
    let mut h = harness_with(Launch {
        lang: Some(Lang::En),
        ..Launch::default()
    });
    populate(&mut h);

    // Un tour de chaque outil, y compris au milieu d'un geste (palette ouverte).
    assert_eq!(Tool::ALL.len(), 26);
    for tool in Tool::ALL {
        h.state_mut().editor.set_tool(tool);
        h.run_steps(2);
        h.get_by_label_contains("Tool: ");
    }
    let gestures: [(Tool, &[(f64, f64)]); 14] = [
        (Tool::Offset, &[(30.0, 0.0)]),
        (Tool::Spline, &[(10.0, 10.0), (40.0, 60.0)]),
        (Tool::Segment, &[(10.0, 10.0)]),
        (Tool::Rectangle, &[(150.0, 10.0)]),
        (Tool::Parallel, &[(60.0, 0.0)]),
        (Tool::Notch, &[(30.0, 0.0)]),
        (Tool::Grainline, &[(10.0, 10.0)]),
        (Tool::Perpendicular, &[(60.0, 0.0)]),
        (Tool::Perpendicular, &[(60.0, 0.0), (30.0, 0.0)]),
        (Tool::PointOnSegment, &[(80.0, 0.0)]),
        (Tool::Measure, &[(0.0, 0.0)]),
        (Tool::Dimension, &[(0.0, 0.0)]),
        (Tool::Fillet, &[(30.0, 0.0)]),
        (Tool::Tangent, &[(10.0, 40.0)]),
    ];
    for (tool, clicks) in gestures {
        let ed = &mut h.state_mut().editor;
        ed.set_tool(tool);
        for (x, y) in clicks {
            ed.click(pt(*x, *y), 2.0, false);
        }
        ed.pointer_moved(pt(20.0, 25.0));
        h.run_steps(3);
        h.state_mut().editor.escape();
        h.run_steps(2);
    }
    h.state_mut().editor.set_tool(Tool::Select);

    // Inspecteur d'une cotation en anglais.
    let dim = h.state().editor.sheet().dimensions[0].id;
    h.state_mut().editor.selection.dimensions = vec![dim];
    h.run_steps(3);
    h.get_by_label("Measured value");

    // Inspecteur : feuille (sans sélection), chaque point, chaque tracé.
    h.state_mut().editor.selection.clear();
    h.run_steps(3);
    h.get_by_label("Overall size");
    let (points, entities): (Vec<_>, Vec<_>) = {
        let s = h.state().editor.sheet();
        (
            s.points.iter().map(|p| p.id).collect(),
            s.entities.iter().map(|e| e.id).collect(),
        )
    };
    assert!(points.len() >= 6 && entities.len() >= 4);
    for p in &points {
        let sel = &mut h.state_mut().editor.selection;
        sel.clear();
        sel.points.push(*p);
        h.run_steps(3);
    }
    for e in &entities {
        let sel = &mut h.state_mut().editor.selection;
        sel.clear();
        sel.entities.push(*e);
        h.run_steps(3);
    }
    // Inspecteur d'un cran.
    let notches: Vec<_> = h
        .state()
        .editor
        .sheet()
        .notches
        .iter()
        .map(|n| n.id)
        .collect();
    assert!(!notches.is_empty(), "le cran de la scène doit exister");
    let sel = &mut h.state_mut().editor.selection;
    sel.clear();
    sel.notches = vec![notches[0]];
    h.run_steps(3);
    h.get_by_label("Placed on");
    h.get_by_label("POSITION AND SHAPE");
    // Fenêtre Transformer, dans chacun de ses trois modes.
    let sel = &mut h.state_mut().editor.selection;
    sel.clear();
    sel.entities.extend(entities.iter().take(2));
    h.state_mut().open_transform(true);
    for kind in [
        crate::transform::Kind::Translate,
        crate::transform::Kind::Rotate,
        crate::transform::Kind::Mirror,
    ] {
        if let Some(t) = h.state_mut().transform.as_mut() {
            t.kind = kind;
        }
        h.run_steps(3);
        h.get_by_label("Transform selection");
        h.get_by_label("Copy (keep the original)");
        h.get_by_label("Apply");
    }
    h.get_by_label("Axis direction");
    h.state_mut().transform = None;
    h.run_steps(2);
    // Menu Édition : entrées de transformation en anglais.
    h.state_mut().editor.selection.clear();
    h.run_steps(2);
    // Deux points, deux tracés, plusieurs éléments.
    let sel = &mut h.state_mut().editor.selection;
    sel.clear();
    sel.points.extend(points.iter().take(2));
    h.run_steps(3);
    h.get_by_label("Distance");
    let sel = &mut h.state_mut().editor.selection;
    sel.clear();
    sel.entities.extend(entities.iter().take(2));
    h.run_steps(3);
    h.get_by_label("Angle between the two");
    let sel = &mut h.state_mut().editor.selection;
    sel.clear();
    sel.entities.extend(entities.iter().take(3));
    sel.points.extend(points.iter().take(2));
    h.run_steps(3);
    h.get_by_label("Total length");

    // Dialogues, aide, erreur.
    h.state_mut().editor.selection.clear();
    h.state_mut().show_help = true;
    h.state_mut().error_dialog = Some("Message".into());
    h.run_steps(3);
    h.get_by_label("Shortcuts and limitations");
    h.get_by_label("Close");
    h.state_mut().error_dialog = None;
    h.state_mut().show_help = false;
    h.state_mut().pending = Some(Pending::Quit);
    h.run_steps(3);
    h.get_by_label("Unsaved changes");
    h.state_mut().pending = None;
    h.run_steps(2);

    // Rien de la liste française connue ne doit rester affiché.
    for french in ["FEUILLES DU PROJET", "INSPECTEUR", "Fichier"] {
        assert!(
            h.query_by_label(french).is_none(),
            "{french} encore affiché"
        );
    }
    set_lang(Lang::Fr);
}

#[test]
fn separateur_decimal_et_statut_suivent_la_langue() {
    set_lang(Lang::Fr);
    let mut h = harness_with(Launch::default());
    populate(&mut h);
    let first = h.state().editor.sheet().entities[0].id;
    h.state_mut().editor.selection.entities = vec![first];
    h.run_steps(3);
    h.get_by_label("120,0 mm");
    set_lang(Lang::En);
    h.run_steps(3);
    h.get_by_label("120.0 mm");
    assert!(h.query_by_label("120,0 mm").is_none());
    set_lang(Lang::Fr);
}

#[test]
fn contours_fermes_en_anglais() {
    set_lang(Lang::En);
    let launch = Launch {
        demo: true,
        stage: Some("accrochage".into()),
        lang: Some(Lang::En),
        ..Launch::default()
    };
    let mut h = harness_with(launch);
    h.run_steps(2);
    h.get_by_label("CLOSED CONTOURS (READ-ONLY)");
    h.get_by_label("Contour 1 — 3 paths, Construction");
    h.get_by_label("Perimeter");
    h.get_by_label("Area");
    set_lang(Lang::Fr);
}
