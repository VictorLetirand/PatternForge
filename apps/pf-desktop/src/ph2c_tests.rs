//! PH-2c : agencement des étiquettes, largeur du canevas à 1280 px, fenêtres du dock
//! défilantes, défilement « Coins ». Tests egui_kittest.

use crate::app::App;
use crate::dock::{SheetsFold, ToolWin};
use crate::labels::{self, Kind, Placed, Request};
use crate::Launch;
use eframe::egui::{self, Pos2, Rect};
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;
use pf_core::Command;
use pf_geometry::PointMm;

fn harness(stage: Option<&str>, size: [f32; 2]) -> Harness<'static, App> {
    let launch = Launch {
        demo: true,
        stage: stage.map(Into::into),
        ..Launch::default()
    };
    build(launch, size)
}

/// Projet vide (pas de démonstration).
fn empty_harness(size: [f32; 2]) -> Harness<'static, App> {
    build(Launch::default(), size)
}

fn build(launch: Launch, size: [f32; 2]) -> Harness<'static, App> {
    let app = App::with_context(&egui::Context::default(), launch);
    let mut h = Harness::builder()
        .with_size(egui::vec2(size[0], size[1]))
        .build_state(|ctx, app: &mut App| app.ui(ctx), app);
    h.run_steps(6);
    h
}

fn panel_rect(h: &Harness<'_, App>, id: &str) -> Option<Rect> {
    egui::containers::panel::PanelState::load(&h.ctx, egui::Id::new(id)).map(|s| s.rect)
}

fn area_rect(h: &Harness<'_, App>, win: ToolWin) -> Option<Rect> {
    h.ctx.memory(|m| m.area_rect(win.id()))
}

fn flat_shapes(shape: &egui::Shape, out: &mut Vec<egui::Shape>) {
    match shape {
        egui::Shape::Vec(v) => v.iter().for_each(|s| flat_shapes(s, out)),
        other => out.push(other.clone()),
    }
}

/// Textes peints à la dernière image : (texte, rectangle).
fn painted_texts(h: &Harness<'_, App>) -> Vec<(String, Rect)> {
    let mut shapes = Vec::new();
    for clipped in &h.output().shapes {
        flat_shapes(&clipped.shape, &mut shapes);
    }
    shapes
        .iter()
        .filter_map(|s| match s {
            egui::Shape::Text(t) => Some((
                t.galley.text().to_string(),
                Rect::from_min_size(t.pos, t.galley.size()),
            )),
            _ => None,
        })
        .collect()
}

// ===== CA1 : agencement des étiquettes ===========================================================

/// Générateur congruentiel à graine fixe (aucune dépendance, séquence reproductible).
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (self.next() % 10_000) as f32 / 10_000.0 * (hi - lo)
    }
}

/// Rectangle en (x0, y0, x1, y1), pour l'oracle (pas de méthode d'egui).
type Box4 = (f32, f32, f32, f32);

fn b4(r: Rect) -> Box4 {
    (r.min.x, r.min.y, r.max.x, r.max.y)
}

/// Recouvrement d'aire strictement positive, recalculé ici (indépendant de `labels`).
fn overlap_area(a: Box4, b: Box4) -> f32 {
    let w = a.2.min(b.2) - a.0.max(b.0);
    let h = a.3.min(b.3) - a.1.max(b.1);
    if w > 0.0 && h > 0.0 {
        w * h
    } else {
        0.0
    }
}

fn rank(kind: Kind) -> u8 {
    match kind {
        Kind::Selected => 0,
        Kind::Dimension => 1,
        Kind::Point => 2,
        Kind::Path => 3,
    }
}

/// Une séquence : demandes, obstacles, zone visible.
struct Seq {
    requests: Vec<Request>,
    obstacles: Vec<Rect>,
    clip: Rect,
}

fn sequence(seed: u64) -> Seq {
    let mut rng = Lcg(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let clip = Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(400.0, 300.0));
    let n = 2 + (rng.next() % 38) as usize;
    let requests = (0..n)
        .map(|_| {
            let (w, h) = (rng.range(14.0, 60.0), rng.range(12.0, 20.0));
            // Zone dense (160 x 100) pour provoquer des recouvrements.
            let (x, y) = (rng.range(100.0, 260.0), rng.range(80.0, 180.0));
            let kind = [Kind::Selected, Kind::Dimension, Kind::Point, Kind::Path]
                [(rng.next() % 4) as usize];
            Request {
                rect: Rect::from_min_size(Pos2::new(x, y), egui::vec2(w, h)),
                kind,
            }
        })
        .collect();
    let obstacles = (0..(rng.next() % 6))
        .map(|_| {
            let (x, y) = (rng.range(100.0, 260.0), rng.range(80.0, 180.0));
            Rect::from_min_size(Pos2::new(x, y), egui::vec2(12.0, 12.0))
        })
        .collect();
    Seq {
        requests,
        obstacles,
        clip,
    }
}

/// Oracle indépendant : vérifie un agencement `placed` pour `seq`.
fn check(seq: &Seq, placed: &[Placed]) -> Result<(), String> {
    let n = seq.requests.len();
    if placed.len() != n {
        return Err(format!(
            "{} étiquettes en entrée, {} en sortie",
            n,
            placed.len()
        ));
    }
    // Ordre de pose recalculé : priorité puis ordre de dessin (toutes sont dans la zone).
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by_key(|&i| (rank(seq.requests[i].kind), i));
    let mut fixed: Vec<Box4> = seq.obstacles.iter().map(|o| b4(*o)).collect();
    for &i in &order {
        let (orig, now) = (b4(seq.requests[i].rect), b4(placed[i].rect));
        let size_ok = ((orig.2 - orig.0) - (now.2 - now.0)).abs() < 1e-3
            && ((orig.3 - orig.1) - (now.3 - now.1)).abs() < 1e-3;
        if !size_ok {
            return Err(format!("étiquette {i} : taille modifiée"));
        }
        let hits_orig = fixed
            .iter()
            .filter(|f| overlap_area(**f, orig) > 0.0)
            .count();
        let hits_now = fixed
            .iter()
            .filter(|f| overlap_area(**f, now) > 0.0)
            .count();
        let moved = (orig.0 - now.0).abs() > 1e-3 || (orig.1 - now.1).abs() > 1e-3;
        if hits_orig == 0 && moved {
            return Err(format!("étiquette {i} déplacée sans être gênée"));
        }
        if hits_orig > 0 && !placed[i].unresolved && hits_now != 0 {
            return Err(format!(
                "étiquette {i} gênée ({hits_orig}) puis déplacée sur {hits_now} voisine(s)"
            ));
        }
        if hits_orig > 0 && !placed[i].unresolved && !moved {
            return Err(format!("étiquette {i} gênée mais laissée sur place"));
        }
        if placed[i].unresolved && moved {
            return Err(format!("étiquette {i} non résolue mais déplacée"));
        }
        // Décalage borné : 3 couronnes de (taille + 2).
        let (dx, dy) = ((now.0 - orig.0).abs(), (now.1 - orig.1).abs());
        let (w, h) = (orig.2 - orig.0 + 2.0, orig.3 - orig.1 + 2.0);
        if dx > 3.0 * w + 1e-3 || dy > 3.0 * h + 1e-3 {
            return Err(format!("étiquette {i} : décalage ({dx}, {dy}) hors bornes"));
        }
        fixed.push(now);
    }
    Ok(())
}

#[test]
fn campagne_de_180_sequences_a_graine_fixe_contre_un_oracle_independant() {
    let mut moved_total = 0;
    let mut unresolved_total = 0;
    for seed in 0..180_u64 {
        let seq = sequence(seed);
        let out = labels::layout(&seq.requests, &seq.obstacles, seq.clip);
        check(&seq, &out.placed).unwrap_or_else(|e| panic!("séquence {seed} : {e}"));
        // Déterminisme.
        assert_eq!(
            out,
            labels::layout(&seq.requests, &seq.obstacles, seq.clip),
            "séquence {seed}"
        );
        assert_eq!(out.skipped, 0, "séquence {seed}");
        moved_total += out.placed.iter().filter(|p| p.moved).count();
        unresolved_total += out.placed.iter().filter(|p| p.unresolved).count();
    }
    // La campagne n'est pas triviale : beaucoup d'étiquettes gênées, déplacées.
    assert!(moved_total > 1000, "{moved_total} déplacées");
    println!("campagne : {moved_total} déplacées, {unresolved_total} non résolues");
}

#[test]
fn l_oracle_rejette_un_agencement_qui_ne_detecte_aucun_recouvrement() {
    // Mutation simulée : « détection désactivée » = chaque étiquette reste à sa place. L'oracle
    // doit le voir sur la grande majorité des séquences (sinon il ne prouverait rien).
    let mut rejected = 0;
    for seed in 0..180_u64 {
        let seq = sequence(seed);
        let identity: Vec<Placed> = seq
            .requests
            .iter()
            .map(|q| Placed {
                rect: q.rect,
                moved: false,
                unresolved: false,
                treated: true,
            })
            .collect();
        if check(&seq, &identity).is_err() {
            rejected += 1;
        }
    }
    assert!(rejected >= 150, "{rejected} séquences sur 180 rejetées");
}

#[test]
fn la_selection_garde_sa_place_dans_toutes_les_sequences_sauf_gene_par_une_selection() {
    // Propriété de priorité, recalculée : une étiquette de sélection qui ne recouvre ni
    // obstacle ni autre sélection posée avant elle reste exactement à sa place.
    for seed in 0..180_u64 {
        let seq = sequence(seed);
        let out = labels::layout(&seq.requests, &seq.obstacles, seq.clip);
        for (i, q) in seq.requests.iter().enumerate() {
            if q.kind != Kind::Selected {
                continue;
            }
            let hindered = seq
                .obstacles
                .iter()
                .any(|o| overlap_area(b4(*o), b4(q.rect)) > 0.0)
                || (0..i).any(|j| {
                    seq.requests[j].kind == Kind::Selected
                        && overlap_area(b4(out.placed[j].rect), b4(q.rect)) > 0.0
                });
            if !hindered {
                assert_eq!(out.placed[i].rect, q.rect, "séquence {seed}, sélection {i}");
            }
        }
    }
}

/// Cinq points à 2 mm les uns des autres, zoom 3 px/mm : leurs noms se chevauchent d'origine.
fn five_close_points() -> Harness<'static, App> {
    let mut h = empty_harness([1280.0, 800.0]);
    let sheet = h.state().editor.sheet;
    for i in 0..5 {
        let done = h.state_mut().editor.exec(Command::AddPoint {
            sheet,
            at: PointMm::new(2.0 * f64::from(i), 0.0),
        });
        assert!(done.is_some());
    }
    let app = h.state_mut();
    app.fit_pending = false;
    app.view.center = PointMm::new(4.0, 0.0);
    app.view.zoom = 3.0;
    h.run_steps(4);
    h
}

#[test]
fn des_points_voisins_gardent_des_noms_lisibles_et_disjoints_dans_le_plan() {
    let h = five_close_points();
    let names: Vec<String> = h
        .state()
        .editor
        .sheet()
        .points
        .iter()
        .map(|p| p.label.clone())
        .collect();
    assert_eq!(names.len(), 5);
    let texts: Vec<(String, Rect)> = painted_texts(&h)
        .into_iter()
        .filter(|(t, _)| names.contains(t))
        .collect();
    assert_eq!(
        texts.len(),
        5,
        "les 5 noms sont peints (aucun supprimé) : {texts:?}"
    );
    for (i, (a, ra)) in texts.iter().enumerate() {
        for (b, rb) in &texts[i + 1..] {
            assert_eq!(
                overlap_area(b4(*ra), b4(*rb)),
                0.0,
                "{a} {ra:?} recouvre {b} {rb:?}"
            );
        }
    }
    let report = labels::last_report(&h.ctx);
    assert!(report.count >= 5 && report.moved >= 1, "{report:?}");
    assert_eq!(report.skipped, 0);
}

#[test]
fn l_agencement_ne_modifie_ni_le_projet_ni_l_historique() {
    let mut h = five_close_points();
    let project = h.state().editor.project().clone();
    let undo = h.state().editor.doc.can_undo();
    let dirty = h.state().editor.doc.is_dirty();
    h.run_steps(6);
    assert_eq!(*h.state().editor.project(), project);
    assert_eq!(h.state().editor.doc.can_undo(), undo);
    assert_eq!(h.state().editor.doc.is_dirty(), dirty);
}

#[test]
fn au_dela_de_400_etiquettes_la_borne_est_annoncee_au_pied_du_canevas() {
    let mut h = empty_harness([1600.0, 1000.0]);
    let sheet = h.state().editor.sheet;
    let batch: Vec<Command> = (0..425)
        .map(|i| Command::AddPoint {
            sheet,
            at: PointMm::new(10.0 * f64::from(i % 25), 10.0 * f64::from(i / 25)),
        })
        .collect();
    assert!(h
        .state_mut()
        .editor
        .exec(Command::Batch { commands: batch })
        .is_some());
    h.state_mut().fit_pending = true;
    h.run_steps(4);
    let report = labels::last_report(&h.ctx);
    assert_eq!(report.count, 425);
    assert_eq!(report.skipped, 25, "{report:?}");
    // Aucune étiquette perdue : 425 noms peints (400 agencés + 25 à leur place d'origine).
    let names: std::collections::HashSet<String> = h
        .state()
        .editor
        .sheet()
        .points
        .iter()
        .map(|p| p.label.clone())
        .collect();
    let texts = painted_texts(&h);
    let painted = texts.iter().filter(|(t, _)| names.contains(t)).count();
    assert!(painted >= 425, "{painted} noms peints");
    assert!(
        texts
            .iter()
            .any(|(t, _)| t.contains("25 étiquette(s) au-delà de 400")),
        "pied du canevas : {:?}",
        texts
            .iter()
            .filter(|(t, _)| t.contains("étiquette"))
            .collect::<Vec<_>>()
    );
}

#[test]
fn sans_depassement_aucune_mention_de_borne() {
    let h = five_close_points();
    assert!(painted_texts(&h)
        .iter()
        .all(|(t, _)| !t.contains("au-delà de 400")));
}

#[test]
fn le_texte_complet_d_une_cote_reste_accessible_en_info_bulle() {
    let mut h = harness(Some("coins"), [1280.0, 800.0]);
    // La cote de lecture du tracé sélectionné est une étiquette du plan.
    let canvas = h.state().canvas_rect;
    let rect = painted_texts(&h)
        .into_iter()
        .find(|(t, r)| t == "100,0 mm" && canvas.contains_rect(*r))
        .map(|(_, r)| r)
        .expect("cote peinte dans le plan");
    assert!(h.query_by_role(egui::accesskit::Role::Tooltip).is_none());
    h.input_mut()
        .events
        .push(egui::Event::PointerMoved(rect.center()));
    // Le délai d'info-bulle d'egui (0,5 s) s'écoule image par image.
    for _ in 0..90 {
        h.run_steps(1);
    }
    let report = labels::last_report(&h.ctx);
    assert_eq!(report.tip.as_deref(), Some("100,0 mm"), "{:?}", report.tip);
    // Et egui l'affiche une fois le délai écoulé : le texte « 100,0 mm » est peint deux fois
    // dans la zone du plan (l'étiquette et son info-bulle).
    let count = painted_texts(&h)
        .iter()
        .filter(|(t, r)| t == "100,0 mm" && r.intersects(canvas))
        .count();
    assert_eq!(count, 2, "étiquette + info-bulle");
}

// ===== CA2 : largeur du canevas ==================================================================

#[test]
fn a_1280_et_1600_le_canevas_garde_400_points_avec_coutures_et_vue_assemblee() {
    for (size, folded) in [([1280.0_f32, 800.0], true), ([1600.0, 1000.0], false)] {
        let h = harness(Some("raccords"), size);
        let open: Vec<ToolWin> = h.state().dock.open().to_vec();
        assert_eq!(open, vec![ToolWin::Seams, ToolWin::JoinView], "{size:?}");
        let canvas = h.state().canvas_rect.width();
        assert!(canvas >= 400.0, "{size:?} : canevas de {canvas} points");
        assert_eq!(h.state().sheets_folded(&h.ctx), folded, "{size:?}");
        // Largeur égale pour toutes les fenêtres du dock : plus de bord en escalier.
        let widths: Vec<f32> = open
            .iter()
            .map(|w| area_rect(&h, *w).expect("fenêtre").width())
            .collect();
        assert!(
            (widths[0] - widths[1]).abs() < 1.0,
            "{size:?} : largeurs {widths:?}"
        );
        // Les fenêtres tiennent dans le dock.
        let dock = h.state().dock.rect;
        for w in &open {
            assert!(
                dock.contains_rect(area_rect(&h, *w).unwrap()),
                "{size:?} {w:?}"
            );
        }
    }
}

#[test]
fn sans_repli_a_1280_le_canevas_passe_sous_400_c_est_ce_que_le_repli_evite() {
    let mut h = harness(Some("raccords"), [1280.0, 800.0]);
    h.state_mut().sheets_fold = SheetsFold::Open;
    h.run_steps(6);
    let sheets = panel_rect(&h, "sheets").expect("panneau déployé");
    assert!(
        sheets.width() <= 262.0,
        "panneau de {} points",
        sheets.width()
    );
    assert!(h.state().canvas_rect.width() < 400.0);
}

#[test]
fn le_chevron_replie_et_deploie_le_panneau_des_feuilles() {
    let mut h = harness(Some("raccords"), [1280.0, 800.0]);
    // Replié automatiquement (dock ouvert) : « Déployer » le rend pour la session.
    assert!(h.state().sheets_folded(&h.ctx));
    h.get_by_label("Déployer le panneau des feuilles").click();
    h.run_steps(6);
    assert_eq!(h.state().sheets_fold, SheetsFold::Open);
    assert!(!h.state().sheets_folded(&h.ctx));
    // « Réduire » le replie, et ce choix tient même sans dock.
    h.get_by_label("Réduire le panneau des feuilles").click();
    h.run_steps(6);
    assert_eq!(h.state().sheets_fold, SheetsFold::Folded);
    h.state_mut().editor.seam.open = false;
    h.run_steps(6);
    assert!(h.state().dock.open().is_empty());
    assert!(h.state().sheets_folded(&h.ctx));
    assert!(h.state().canvas_rect.width() > 900.0);
    // Aucune donnée touchée par ces boutons.
    assert!(!h.state().editor.doc.is_dirty());
}

#[test]
fn le_repli_automatique_cesse_quand_le_dock_se_ferme() {
    let mut h = harness(Some("raccords"), [1280.0, 800.0]);
    assert!(h.state().sheets_folded(&h.ctx));
    h.state_mut().editor.seam.open = false;
    h.run_steps(6);
    assert!(!h.state().sheets_folded(&h.ctx));
    assert_eq!(h.state().sheets_fold, SheetsFold::Auto);
}

#[test]
fn le_repli_choisi_est_memorise_dans_les_reglages_et_jamais_dans_le_projet() {
    let dir = std::env::temp_dir().join(format!("pf-ph2c-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("dossier");
    let path = dir.join("settings.json");
    let mut h = harness(None, [1280.0, 800.0]);
    h.state_mut().settings_path = Some(path.clone());
    let project = h.state().editor.project().clone();
    let ctx = h.ctx.clone();
    h.state_mut().toggle_sheets_fold(&ctx);
    assert_eq!(h.state().sheets_fold, SheetsFold::Folded);
    assert_eq!(
        pf_i18n::settings::load_value(&path, crate::dock::SHEETS_SETTING),
        Some(serde_json::json!("folded"))
    );
    h.state_mut().toggle_sheets_fold(&ctx);
    assert_eq!(
        pf_i18n::settings::load_value(&path, crate::dock::SHEETS_SETTING),
        Some(serde_json::json!("auto"))
    );
    assert_eq!(*h.state().editor.project(), project);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn le_panneau_des_feuilles_deploye_ne_depasse_pas_262_points_a_1280() {
    let mut checked = 0;
    for stage in crate::stages::STAGES.iter().copied() {
        let h = harness(Some(stage), [1280.0, 800.0]);
        if h.state().plan.is_some() || h.state().sheets_folded(&h.ctx) {
            continue;
        }
        let w = panel_rect(&h, "sheets").expect("panneau").width();
        assert!(w <= 262.0, "{stage} : panneau de {w} points");
        checked += 1;
    }
    assert!(checked >= 40, "{checked} étapes vérifiées");
}

// ===== CA3 : fenêtres du dock défilantes ==========================================================

#[test]
fn quatre_fenetres_empilees_ne_depassent_ni_le_dock_ni_l_ecran_a_1280() {
    let mut h = harness(Some("assembler"), [1280.0, 800.0]);
    // Transformer et Aligner exigent une sélection (points et un segment).
    let (points, entities): (Vec<_>, Vec<_>) = {
        let sheet = h.state().editor.sheet();
        (
            sheet.points.iter().map(|p| p.id).collect(),
            sheet.entities.iter().map(|e| e.id).collect(),
        )
    };
    h.state_mut().editor.selection.points = points;
    h.state_mut().editor.selection.entities = entities;
    h.state_mut().open_unfold(None);
    h.state_mut().open_transform(false);
    h.state_mut().open_level(true);
    h.run_steps(10);
    let open: Vec<ToolWin> = h.state().dock.open().to_vec();
    for w in [
        ToolWin::Assemble,
        ToolWin::Unfold,
        ToolWin::Transform,
        ToolWin::Level,
    ] {
        assert!(open.contains(&w), "{w:?} ouverte : {open:?}");
    }
    let screen = Rect::from_min_size(Pos2::ZERO, egui::vec2(1280.0, 800.0));
    let dock = h.state().dock.rect;
    let rects: Vec<(ToolWin, Rect)> = open
        .iter()
        .map(|w| (*w, area_rect(&h, *w).expect("rectangle")))
        .collect();
    for (w, r) in &rects {
        assert!(screen.contains_rect(*r), "{w:?} {r:?} hors écran");
        assert!(dock.contains_rect(*r), "{w:?} {r:?} hors du dock {dock:?}");
    }
    for (i, (a, ra)) in rects.iter().enumerate() {
        for (b, rb) in &rects[i + 1..] {
            assert!(!ra.intersects(*rb), "{a:?} {ra:?} recouvre {b:?} {rb:?}");
        }
    }
    // Le bas de la dernière ne dépasse pas le bas du dock.
    let last = rects.last().expect("fenêtre").1;
    assert!(last.bottom() <= dock.bottom() + 0.5, "{last:?} / {dock:?}");
}

#[test]
fn une_fenetre_courte_garde_sa_hauteur_de_contenu_malgre_le_defilement() {
    // Le défilement ne gonfle pas la fenêtre : Transformer seule reste nettement plus basse
    // que la hauteur du dock.
    let mut h = harness(None, [1280.0, 800.0]);
    h.state_mut().open_transform(false);
    h.run_steps(8);
    let r = area_rect(&h, ToolWin::Transform).expect("fenêtre");
    assert!(r.height() < h.state().dock.rect.height() * 0.8, "{r:?}");
}

// ===== CA4 : défilement « Coins » =================================================================

#[test]
fn le_defilement_coins_ne_rogne_aucune_ligne_en_haut_de_la_fiche() {
    for size in [[1280.0_f32, 800.0], [1600.0, 1000.0]] {
        let h = harness(Some("coins"), size);
        let panel = panel_rect(&h, "sheets").expect("panneau");
        // Haut de la zone défilante : celui du panneau plus sa marge (2 points).
        let top = panel.top() + 2.0;
        let title = h
            .query_by_label_contains("Coins (")
            .expect("titre du bloc Coins")
            .rect();
        assert!(
            title.min.y >= top - 0.5,
            "{size:?} : titre rogné {title:?} / {top}"
        );
        assert!(
            title.min.y <= top + 16.0,
            "{size:?} : titre loin du haut {title:?}"
        );
        // Aucun nœud de la fiche n'est coupé par le bord haut de la zone défilante.
        for n in h.root().children_recursive() {
            let r = n.rect();
            if r.min.x >= panel.min.x
                && r.max.x <= panel.max.x
                && r.height() > 1.0
                && r.height() < 60.0
            {
                assert!(
                    !(r.min.y < top - 0.5 && r.max.y > top + 0.5),
                    "{size:?} : ligne rognée en haut {:?} {:?}",
                    n.accesskit_node().label(),
                    r
                );
            }
        }
    }
}

// ===== Textes =====================================================================================

#[test]
fn les_cles_ajoutees_existent_en_fr_et_en_avec_les_memes_marques() {
    let keys = [
        ("canvas.labels.limit", &["count", "max"][..]),
        ("panel.sheets.fold", &[][..]),
        ("panel.sheets.unfold", &[][..]),
    ];
    for (key, marks) in keys {
        for lang in [pf_i18n::Lang::Fr, pf_i18n::Lang::En] {
            let found: Vec<&str> = lang
                .catalog()
                .iter()
                .filter(|(k, _)| *k == key)
                .map(|(_, v)| *v)
                .collect();
            assert_eq!(found.len(), 1, "{key} : une seule entrée");
            for m in marks {
                assert!(found[0].contains(&format!("{{{m}}}")), "{key} : {{{m}}}");
            }
            assert_eq!(found[0].matches('{').count(), marks.len(), "{key}");
        }
    }
    pf_i18n::set_lang(pf_i18n::Lang::En);
    assert_eq!(
        pf_i18n::tr("panel.sheets.fold"),
        "Collapse the sheets panel"
    );
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
}
