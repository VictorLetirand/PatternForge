//! PH-5c — « Rempli 8 mm » et « Coupe nette » écrits dans les quatre exports (PDF, SVG, DXF, HPGL).
//! Lecteurs et fixtures propres à ce fichier (`lecteurs_libelles/`), indépendants de `common/`.
//! Valeurs calculées **à la main** (mm, Y vers le haut).
//!
//! Pièce de base « Carte » : rectangle 100 × 60 en (10 ; 20), tracés de rôle couture, marge de
//! couture 5 mm. Constantes de lecture : corps du libellé de bord 3,5 mm, largeur approchée 0,62 em
//! par caractère (demi-largeur de « Rempli 8 mm », 11 caractères : 11 × 3,5 × 0,62 / 2 = 11,935),
//! écart 1,5 mm entre le bord et le libellé, ligne de base 0,35 × 3,5 = 1,225 mm sous le centre.
//!
//! Libellé sur le bord bas (y = 20) : intérieur vers le haut, centre à 1,5 + 3,5 / 2 = 3,25 mm du
//! bord : (60 ; 23,25), ligne de base y = 22,025. Bord haut (y = 80) : (60 ; 76,75). Bord gauche
//! (x = 10, intérieur vers la droite) : centre x = 10 + 1,5 + demi-largeur, y = 50 ; bord droit :
//! x = 110 − 1,5 − demi-largeur.
//!
//! Étiquette de pièce : centre du bloc = milieu de l'emprise du contour, (60 ; 50) ; ligne `i` sur
//! `n` : ligne de base y = 50 + ((n − 1) / 2 − i) × 5 − 1,4 (pas 5 mm, corps 4 mm).
//!
//! Honnêteté : aucune ouverture dans un logiciel tiers, aucune impression, aucun traçage réels.

mod lecteurs_libelles;

use lecteurs_libelles::*;
use pf_core::{Command, FoldAllowance, PathRole, PieceId, Project, SeamAllowance, SheetId};
use pf_export::dxf::export_dxf_with;
use pf_export::hpgl::export_hpgl;
use pf_export::pdf::{export_pdf, export_pdf_with, plan_tiles_with};
use pf_export::svg::export_svg;
use pf_export::{
    fold_over_value_text, AssemblyOptions, OutputCategory as C, OutputContent, PageFormat,
    PageSetup, PieceExportMode, Scene, SceneOptions,
};
use pf_i18n::Lang;

struct Fx {
    p: Project,
    s: SheetId,
    piece: PieceId,
    edges: Edges,
}

/// Sommets de « Carte » : la construction du rectangle pose un point sur chacun ; ces points sont
/// les repères qui donnent le passage modèle → SVG ou page.
const CORNERS: [(f64, f64); 4] = [(10.0, 20.0), (110.0, 20.0), (110.0, 80.0), (10.0, 80.0)];

/// « Carte » 100 × 60 en (10 ; 20), couture.
fn base() -> Fx {
    let mut p = Project::new("Libelles");
    let s = p.sheets[0].id;
    let (piece, edges) = rect(
        &mut p,
        s,
        (10.0, 20.0),
        (100.0, 60.0),
        "Carte",
        PathRole::SewingContour,
    );
    Fx { p, s, piece, edges }
}

fn set_folds(fx: &mut Fx, folds: &[(pf_core::EntityId, f64)]) {
    fx.p.apply(Command::SetSeamAllowance {
        piece: fx.piece,
        seam: Some(SeamAllowance {
            default_mm: 5.0,
            folds: folds
                .iter()
                .map(|(entity, value_mm)| FoldAllowance {
                    entity: *entity,
                    value_mm: *value_mm,
                    notch_step_mm: None,
                    notch_count: None,
                })
                .collect(),
            ..Default::default()
        }),
    })
    .unwrap();
}

fn set_fiche(fx: &mut Fx) {
    fx.p.apply(Command::SetPieceCard {
        piece: fx.piece,
        name: "Carte".into(),
        code: Some("C1".into()),
        quantity: 1,
        mirrored_pair: false,
        material: None,
        category: None,
        comment: None,
    })
    .unwrap();
}

fn net_cut(fx: &mut Fx) {
    fx.p.apply(Command::SetNetCut {
        piece: fx.piece,
        net_cut: true,
    })
    .unwrap();
}

fn scene_of(fx: &Fx, lang: Lang, content: OutputContent) -> Scene {
    content
        .scene(
            &fx.p,
            fx.s,
            SceneOptions {
                lang,
                ..SceneOptions::default()
            },
        )
        .unwrap()
}

fn roll() -> PageSetup {
    PageSetup {
        format: PageFormat::Roll { width_mm: 914.0 },
        ..PageSetup::default()
    }
}

fn hpgl_text(sc: &Scene) -> String {
    export_hpgl(sc, &roll()).unwrap().text
}

/// Plus petit point des traits des plumes 1 et 2 (unités HPGL).
fn hpgl_min(plot: &str) -> (i64, i64) {
    let pts: Vec<(i64, i64)> = [1, 2]
        .into_iter()
        .flat_map(|pen| hpgl_strokes(plot, pen))
        .flatten()
        .collect();
    (
        pts.iter().map(|p| p.0).min().unwrap(),
        pts.iter().map(|p| p.1).min().unwrap(),
    )
}

/// Plus petit point du plus long contour de la couche `layer` (15 : couture, 1 : coupe).
fn dxf_min(dxf: &str, layer: &str) -> (f64, f64) {
    let v = dxf_layer_vertices(dxf, layer);
    assert!(!v.is_empty(), "couche {layer} vide");
    (
        v.iter().map(|p| p.0).fold(f64::MAX, f64::min),
        v.iter().map(|p| p.1).fold(f64::MAX, f64::min),
    )
}

/// Demi-largeur (mm) d'un libellé de bord, formule de lecture : 0,62 em de 3,5 mm par caractère.
fn half(text: &str) -> f64 {
    text.chars().count() as f64 * 3.5 * 0.62 / 2.0
}

// ---------------------------------------------------------------------------------------------
// CA1 — rempli
// ---------------------------------------------------------------------------------------------

#[test]
fn svg_rempli_quatre_bords_en_francais() {
    let mut fx = base();
    let e = fx.edges;
    set_folds(
        &mut fx,
        &[
            (e.bottom, 8.0),
            (e.right, 6.5),
            (e.top, 12.125),
            (e.left, 0.25),
        ],
    );
    let svg = export_svg(&scene_of(&fx, Lang::Fr, OutputContent::default())).unwrap();
    let (dx, dy) = svg_frame(&svg, &CORNERS);
    let texts: Vec<SvgText> = svg_texts(&svg)
        .into_iter()
        .filter(|t| t.role == "libelle-rempli")
        .collect();
    assert_eq!(texts.len(), 4, "{texts:?}");
    // (texte, centre attendu) ; ligne de base 1,225 mm sous le centre.
    let want = [
        ("Rempli 8 mm", (60.0, 23.25)),
        ("Rempli 6,5 mm", (110.0 - 1.5 - half("Rempli 6,5 mm"), 50.0)),
        ("Rempli 12,125 mm", (60.0, 76.75)),
        (
            "Rempli 0,25 mm",
            (10.0 + 1.5 + half("Rempli 0,25 mm"), 50.0),
        ),
    ];
    for (text, (cx, cy)) in want {
        let t = texts
            .iter()
            .find(|t| t.text == text)
            .unwrap_or_else(|| panic!("« {text} » absent : {texts:?}"));
        assert!(near(t.x, cx + dx, 1e-3), "{text} : x {} / {}", t.x, cx + dx);
        assert!(
            near(t.y, dy - (cy - 1.225), 1e-3),
            "{text} : y {} / {}",
            t.y,
            dy - (cy - 1.225)
        );
    }
    // Valeurs numériques : 110 − 1,5 − 14,105 = 94,395 et 10 + 1,5 + 15,19 = 26,69.
    assert!(near(half("Rempli 6,5 mm"), 14.105, 1e-9) && near(half("Rempli 0,25 mm"), 15.19, 1e-9));
}

#[test]
fn svg_rempli_en_anglais() {
    let mut fx = base();
    let e = fx.edges;
    set_folds(&mut fx, &[(e.bottom, 8.0), (e.right, 6.5)]);
    let svg = export_svg(&scene_of(&fx, Lang::En, OutputContent::default())).unwrap();
    let (dx, dy) = svg_frame(&svg, &CORNERS);
    let texts: Vec<SvgText> = svg_texts(&svg)
        .into_iter()
        .filter(|t| t.role == "libelle-rempli")
        .collect();
    let all: Vec<&str> = texts.iter().map(|t| t.text.as_str()).collect();
    assert_eq!(all.len(), 2);
    assert!(
        all.contains(&"Fold-over 8 mm") && all.contains(&"Fold-over 6.5 mm"),
        "{all:?}"
    );
    assert!(!svg.contains("Rempli"));
    let bottom = texts.iter().find(|t| t.text == "Fold-over 8 mm").unwrap();
    assert!(near(bottom.x, 60.0 + dx, 1e-3) && near(bottom.y, dy - 22.025, 1e-3));
    assert_eq!(
        attr_of(&svg, "libelle-rempli", "data-piece").as_deref(),
        Some("Carte")
    );
}

fn attr_of(svg: &str, role: &str, name: &str) -> Option<String> {
    svg.lines()
        .map(str::trim)
        .find(|l| l.contains(&format!("data-role=\"{role}\"")))
        .and_then(|l| attr(l, name))
}

#[test]
fn pdf_rempli_noto_sans_position_dans_la_page() {
    let mut fx = base();
    let e = fx.edges;
    set_folds(&mut fx, &[(e.bottom, 8.0)]);
    let pdf = export_pdf(&[scene_of(&fx, Lang::Fr, OutputContent::default())]).unwrap();
    let texts = pdf_texts(&pdf.bytes);
    let label: Vec<&PdfText> = texts.iter().filter(|t| t.text == "Rempli 8 mm").collect();
    assert_eq!(label.len(), 1, "{texts:?}");
    let t = label[0];
    // Corps 3,5 mm = 9,9213 pt, texte horizontal.
    assert!(near(t.size_pt, 3.5 * 72.0 / 25.4, 0.01), "{}", t.size_pt);
    assert!(near(t.angle_deg, 0.0, 1e-9));
    let (px, py) = pdf_frame(&pdf.bytes, t.page, &CORNERS, 0);
    // La page est le modèle décalé de (px ; py), lu sur les croix des quatre sommets.
    let width = pdf_text_width_mm(&pdf.bytes, "Rempli 8 mm", 3.5 * 72.0 / 25.4);
    // Départ = centre − demi-largeur réelle de la police ; ligne de base 1,225 mm sous le centre.
    assert!(
        near(t.x_mm, px + 60.0 - width / 2.0, 0.02),
        "{} / {}",
        t.x_mm,
        px + 60.0 - width / 2.0
    );
    assert!(
        near(t.y_mm, py + 22.025, 0.02),
        "{} / {}",
        t.y_mm,
        py + 22.025
    );
    // Police de l'étiquette : toute la chaîne est décodée par la table `ToUnicode` du fichier.
    assert!(!t.text.contains('\u{FFFD}'));
}

#[test]
fn dxf_rempli_texte_sur_la_couche_de_l_etiquette() {
    let mut fx = base();
    let e = fx.edges;
    set_folds(&mut fx, &[(e.bottom, 8.0)]);
    let dxf = export_dxf_with(&fx.p, PieceExportMode::Both).unwrap().text;
    let texts = dxf_texts(&dxf);
    let label: Vec<&DxfText> = texts
        .iter()
        .filter(|t| t.text == "Fold-over 8 mm")
        .collect();
    assert_eq!(label.len(), 1, "{texts:?}");
    // Même couche (84) que les lignes « Piece Name: … » ; texte anglais quelle que soit la langue.
    let name = texts
        .iter()
        .find(|t| t.text == "Piece Name: Carte")
        .unwrap();
    assert_eq!(label[0].layer, name.layer);
    assert_eq!(label[0].layer, "84");
    // Couture (couche 15) : coin bas-gauche (10 ; 20). Centre (60 ; 23,25) ; texte de 14 caractères
    // × 4 × 0,6 = 33,6 mm : coin bas-gauche du texte (60 − 16,8 ; 23,25 − 2) = (43,2 ; 21,25).
    let (mx, my) = dxf_min(&dxf, "15");
    assert!(near(label[0].x, 43.2 - 10.0 + mx, 1e-3), "{}", label[0].x);
    assert!(near(label[0].y, 21.25 - 20.0 + my, 1e-3), "{}", label[0].y);
}

#[test]
fn hpgl_rempli_plume_des_etiquettes_en_ascii() {
    let mut fx = base();
    let e = fx.edges;
    set_folds(&mut fx, &[(e.bottom, 8.0)]);
    let plot = hpgl_text(&scene_of(&fx, Lang::Fr, OutputContent::default()));
    let labels = hpgl_labels(&plot);
    let label: Vec<&HpglLabel> = labels.iter().filter(|l| l.text == "Rempli 8 mm").collect();
    assert_eq!(label.len(), 1, "{labels:?}");
    assert_eq!(label[0].pen, 7);
    // Origine : coin bas-gauche de la coupe calculée, (10 − 5 ; 20 − 8) = (5 ; 12). Départ de la
    // ligne de base : (60 − 11,935 − 5 ; 22,025 − 12) × 40 = (1 722,6 ; 401,0).
    assert_eq!(hpgl_min(&plot), (0, 0));
    assert!((label[0].x - 1723).abs() <= 1, "{}", label[0].x);
    assert!((label[0].y - 401).abs() <= 1, "{}", label[0].y);
}

#[test]
fn valeur_au_plus_court_et_separateur_de_la_langue() {
    let cases: [(f64, &str, &str); 8] = [
        (8.0, "8", "8"),
        (6.5, "6,5", "6.5"),
        (0.25, "0,25", "0.25"),
        (12.125, "12,125", "12.125"),
        (7.0004, "7", "7"),
        (1000.0, "1000", "1000"),
        (10.0, "10", "10"),
        (0.1 + 0.2, "0,3", "0.3"),
    ];
    for (v, fr, en) in cases {
        assert_eq!(fold_over_value_text(v, Lang::Fr), fr, "{v}");
        assert_eq!(fold_over_value_text(v, Lang::En), en, "{v}");
    }
}

#[test]
fn pas_de_libelle_sans_rempli_a_valeur_non_nulle_ni_sur_un_bord_sur_le_pli() {
    // Aucun rempli : aucune trace dans les quatre formats.
    let fx = base();
    let sc = scene_of(&fx, Lang::Fr, OutputContent::default());
    assert!(sc.fold_overs.is_empty());
    let svg = export_svg(&sc).unwrap();
    assert!(!svg.contains("libelle-rempli") && !svg.contains("Rempli"));
    // Rempli de valeur nulle : pas de libellé.
    let mut zero = base();
    let e = zero.edges;
    set_folds(&mut zero, &[(e.bottom, 0.0)]);
    assert!(scene_of(&zero, Lang::Fr, OutputContent::default())
        .fold_overs
        .is_empty());
    let dxf = export_dxf_with(&zero.p, PieceExportMode::Both)
        .unwrap()
        .text;
    assert!(dxf_texts(&dxf)
        .iter()
        .all(|t| !t.text.contains("Fold-over")));
    // Rempli sur un bord déclaré sur le pli : la marge est ignorée, donc aucun libellé « Rempli ».
    let mut on_fold = base();
    let e = on_fold.edges;
    set_folds(&mut on_fold, &[(e.bottom, 8.0)]);
    on_fold
        .p
        .apply(Command::SetPieceFold {
            piece: on_fold.piece,
            edge: e.bottom,
            on: true,
        })
        .unwrap();
    let sc = scene_of(&on_fold, Lang::Fr, OutputContent::default());
    assert!(sc.fold_overs.is_empty());
    assert_eq!(sc.folds.len(), 1);
}

// ---------------------------------------------------------------------------------------------
// CA2 — coupe nette
// ---------------------------------------------------------------------------------------------

/// « Carte » en coupe nette avec fiche (code C1) : lignes Carte, Code : C1, Quantité : 1, mention
/// (n = 4) ; ligne de base de la mention : y = 50 + (1,5 − 3) × 5 − 1,4 = 41,1.
fn net_cut_with_fiche() -> Fx {
    let mut fx = base();
    net_cut(&mut fx);
    set_fiche(&mut fx);
    fx
}

#[test]
fn svg_coupe_nette_derniere_ligne_de_l_etiquette_fr_et_en() {
    let fx = net_cut_with_fiche();
    for (lang, mention) in [(Lang::Fr, "Coupe nette"), (Lang::En, "Net cut")] {
        let svg = export_svg(&scene_of(&fx, lang, OutputContent::default())).unwrap();
        let (dx, dy) = svg_frame(&svg, &CORNERS);
        let lines: Vec<SvgText> = svg_texts(&svg)
            .into_iter()
            .filter(|t| t.role == "etiquette-piece")
            .collect();
        let texts: Vec<&str> = lines.iter().map(|t| t.text.as_str()).collect();
        assert_eq!(texts, ["Carte", "Code : C1", "Quantité : 1", mention]);
        let last = &lines[3];
        assert!(near(last.x, 60.0 + dx, 1e-3), "{}", last.x);
        assert!(near(last.y, dy - 41.1, 1e-3), "{}", last.y);
    }
}

#[test]
fn pdf_hpgl_dxf_coupe_nette() {
    let fx = net_cut_with_fiche();
    // PDF.
    let pdf = export_pdf(&[scene_of(&fx, Lang::Fr, OutputContent::default())]).unwrap();
    let texts = pdf_texts(&pdf.bytes);
    let t = texts
        .iter()
        .find(|t| t.text == "Coupe nette")
        .expect("mention PDF");
    let (px, py) = pdf_frame(&pdf.bytes, t.page, &CORNERS, 0);
    let width = pdf_text_width_mm(&pdf.bytes, "Coupe nette", 4.0 * 72.0 / 25.4);
    assert!(near(t.x_mm, px + 60.0 - width / 2.0, 0.02), "{}", t.x_mm);
    assert!(near(t.y_mm, py + 41.1, 0.02), "{}", t.y_mm);
    // HPGL : plume 7, ASCII ; origine = coin bas-gauche de la coupe nette (10 ; 20) ; départ de la
    // ligne de base : (60 − 11 × 2,2 / 2 − 10 ; 41,1 − 20) × 40 = (1 516 ; 844).
    let plot = hpgl_text(&scene_of(&fx, Lang::Fr, OutputContent::default()));
    let l = hpgl_labels(&plot)
        .into_iter()
        .find(|l| l.text == "Coupe nette")
        .expect("mention HPGL");
    assert_eq!(l.pen, 7);
    assert_eq!(hpgl_min(&plot), (0, 0));
    assert!((l.x - 1516).abs() <= 1 && (l.y - 844).abs() <= 1, "{l:?}");
    // DXF : anglais, couche 84, dernière ligne du bloc ; n = 4, k = 3 : y = 50 + 9 − 18 − 2 = 39 ;
    // « Net cut » 7 × 4 × 0,6 = 16,8 mm : x = 60 − 8,4 = 51,6.
    let dxf = export_dxf_with(&fx.p, PieceExportMode::Both).unwrap().text;
    let all = dxf_texts(&dxf);
    let m = all
        .iter()
        .find(|t| t.text == "Net cut")
        .expect("mention DXF");
    assert_eq!(m.layer, "84");
    // Coupe nette : contour de coupe (couche 1) ou, selon le rôle des tracés, de couture (15).
    let (mx, my) = if dxf_layer_vertices(&dxf, "1").is_empty() {
        dxf_min(&dxf, "15")
    } else {
        dxf_min(&dxf, "1")
    };
    assert!(
        near(m.x, 51.6 - 10.0 + mx, 1e-3) && near(m.y, 39.0 - 20.0 + my, 1e-3),
        "{m:?}"
    );
    let order: Vec<&str> = all.iter().map(|t| t.text.as_str()).collect();
    assert_eq!(
        order,
        ["Piece Name: Carte", "Code: C1", "Quantity: 1", "Net cut"]
    );
}

#[test]
fn coupe_nette_sans_fiche_nom_puis_mention_et_sans_declaration_aucune_trace() {
    // Sans fiche : l'étiquette n'existe pas ; en coupe nette elle porte le nom puis la mention.
    let mut fx = base();
    net_cut(&mut fx);
    let sc = scene_of(&fx, Lang::Fr, OutputContent::default());
    assert_eq!(sc.piece_labels.len(), 1);
    assert_eq!(sc.piece_labels[0].lines, ["Carte", "Coupe nette"]);
    // Deux lignes : y = 50 + 2,5 − 1,4 = 51,1 et 50 − 2,5 − 1,4 = 46,1.
    let svg = export_svg(&sc).unwrap();
    let (dx, dy) = svg_frame(&svg, &CORNERS);
    let lines: Vec<SvgText> = svg_texts(&svg)
        .into_iter()
        .filter(|t| t.role == "etiquette-piece")
        .collect();
    assert!(near(lines[0].y, dy - 51.1, 1e-3) && near(lines[1].y, dy - 46.1, 1e-3));
    assert!(near(lines[1].x, 60.0 + dx, 1e-3));
    // Sans déclaration : ni étiquette, ni mention, nulle part.
    let plain = base();
    let sc = scene_of(&plain, Lang::Fr, OutputContent::default());
    assert!(sc.piece_labels.is_empty() && sc.fold_overs.is_empty());
    let svg = export_svg(&sc).unwrap();
    assert!(!svg.contains("Coupe nette") && !svg.contains("etiquette-piece"));
    let dxf = export_dxf_with(&plain.p, PieceExportMode::Both)
        .unwrap()
        .text;
    assert!(!dxf.contains("Net cut"));
    // Une fiche renseignée sans coupe nette : étiquette ordinaire, sans mention.
    let mut fiche = base();
    set_fiche(&mut fiche);
    let sc = scene_of(&fiche, Lang::Fr, OutputContent::default());
    assert_eq!(
        sc.piece_labels[0].lines,
        ["Carte", "Code : C1", "Quantité : 1"]
    );
}

// ---------------------------------------------------------------------------------------------
// CA3 — filtre de contenu et modes couture / coupe
// ---------------------------------------------------------------------------------------------

fn rempli_fx() -> Fx {
    let mut fx = base();
    let e = fx.edges;
    set_folds(&mut fx, &[(e.bottom, 8.0)]);
    fx
}

/// Position du libellé SVG dans le repère du modèle (indépendante de l'emprise).
fn svg_rel(svg: &str, role: &str, text: &str) -> Option<(f64, f64)> {
    let (dx, dy) = svg_frame(svg, &CORNERS);
    svg_texts(svg)
        .into_iter()
        .find(|t| t.role == role && t.text == text)
        .map(|t| (t.x - dx, dy - t.y))
}

#[test]
fn filtre_de_contenu_etiquettes_de_piece_retire_rempli_et_mention() {
    let no_labels = OutputContent::default().without(C::PieceLabels);
    // Rempli.
    let fx = rempli_fx();
    let sc = scene_of(&fx, Lang::Fr, no_labels);
    assert!(sc.fold_overs.is_empty());
    let svg = export_svg(&sc).unwrap();
    assert!(!svg.contains("libelle-rempli") && !svg.contains("Rempli"));
    let pdf = export_pdf(std::slice::from_ref(&sc)).unwrap();
    assert!(pdf_texts(&pdf.bytes)
        .iter()
        .all(|t| !t.text.contains("Rempli")));
    assert!(hpgl_labels(&hpgl_text(&sc)).is_empty());
    // Coupe nette.
    let fx = net_cut_with_fiche();
    let sc = scene_of(&fx, Lang::Fr, no_labels);
    assert!(sc.piece_labels.is_empty());
    assert!(!export_svg(&sc).unwrap().contains("Coupe nette"));
    assert!(hpgl_labels(&hpgl_text(&sc)).is_empty());
    // Une autre catégorie exclue (lignes spéciales) ne touche pas ces libellés.
    let other = OutputContent::default().without(C::SpecialLines);
    let svg = export_svg(&scene_of(&rempli_fx(), Lang::Fr, other)).unwrap();
    assert!(svg.contains(">Rempli 8 mm<"));
    let svg = export_svg(&scene_of(&net_cut_with_fiche(), Lang::Fr, other)).unwrap();
    assert!(svg.contains(">Coupe nette<"));
}

fn mode_content(mode: PieceExportMode) -> OutputContent {
    let mut c = OutputContent::default();
    c.piece_mode = mode;
    c
}

#[test]
fn modes_couture_seule_et_coupe_seule_ne_changent_pas_les_libelles() {
    // Convention : les libellés décrivent la pièce, pas un contour ; ils sont écrits quel que soit
    // le mode (aucun contour ne les porte) et au même endroit par rapport au repère.
    let fx = rempli_fx();
    let reference = svg_rel(
        &export_svg(&scene_of(&fx, Lang::Fr, OutputContent::default())).unwrap(),
        "libelle-rempli",
        "Rempli 8 mm",
    )
    .unwrap();
    assert!(
        near(reference.0, 60.0, 1e-3) && near(reference.1, 22.025, 1e-3),
        "{reference:?}"
    );
    let variants = [
        OutputContent::sewing_only(),
        OutputContent::cutting_only(),
        mode_content(PieceExportMode::SewingOnly),
        mode_content(PieceExportMode::CutOnly),
    ];
    for content in variants {
        let svg = export_svg(&scene_of(&fx, Lang::Fr, content)).unwrap();
        let got = svg_rel(&svg, "libelle-rempli", "Rempli 8 mm").expect("libellé écrit");
        assert!(
            near(got.0, reference.0, 1e-3) && near(got.1, reference.1, 1e-3),
            "{got:?}"
        );
    }
    let nc = net_cut_with_fiche();
    for content in variants {
        let svg = export_svg(&scene_of(&nc, Lang::Fr, content)).unwrap();
        assert!(svg_rel(&svg, "etiquette-piece", "Coupe nette").is_some());
    }
}

// ---------------------------------------------------------------------------------------------
// Pas de recouvrement d'une autre étiquette
// ---------------------------------------------------------------------------------------------

#[test]
fn le_libelle_glisse_le_long_du_bord_pour_ne_pas_recouvrir_l_etiquette_de_la_piece() {
    // Pièce 120 × 14 en (0 ; 0), fiche : « Carte », « Code : A1 », « Quantité : 1 » (3 lignes ;
    // « Quantité : 1 » : 12 × 4 × 0,55 = 26,4 mm de large). Bloc centré en (60 ; 7) : x de 46,8 à
    // 73,2, y de −0,5 à 14,5. « Rempli 8 mm » au milieu du bord bas : x de 48,065 à 71,935, y de
    // 1,5 à 5 : il recouvre le bloc. Pas de 3,5 mm le long du bord, d'abord vers +x : il faut
    // 60 + s − 11,935 ≥ 73,2, soit s ≥ 25,135 ; j = 7 (24,5) échoue, j = 8 (28) convient : centre
    // (88 ; 3,25), ligne de base y = 2,025.
    let mut p = Project::new("Glisse");
    let s = p.sheets[0].id;
    let (piece, e) = rect(
        &mut p,
        s,
        (0.0, 0.0),
        (120.0, 14.0),
        "Carte",
        PathRole::SewingContour,
    );
    p.apply(Command::SetPieceCard {
        piece,
        name: "Carte".into(),
        code: Some("A1".into()),
        quantity: 1,
        mirrored_pair: false,
        material: None,
        category: None,
        comment: None,
    })
    .unwrap();
    p.apply(Command::SetSeamAllowance {
        piece,
        seam: Some(SeamAllowance {
            default_mm: 2.0,
            folds: vec![FoldAllowance {
                entity: e.bottom,
                value_mm: 8.0,
                notch_step_mm: None,
                notch_count: None,
            }],
            ..Default::default()
        }),
    })
    .unwrap();
    let scene = Scene::from_sheet(&p, s).unwrap();
    let svg = export_svg(&scene).unwrap();
    let (dx, dy) = svg_frame(
        &svg,
        &[(0.0, 0.0), (120.0, 0.0), (120.0, 14.0), (0.0, 14.0)],
    );
    let l = svg_texts(&svg)
        .into_iter()
        .find(|t| t.role == "libelle-rempli")
        .unwrap();
    assert!(
        near(l.x, 88.0 + dx, 1e-3) && near(l.y, dy - 2.025, 1e-3),
        "{l:?}"
    );
}

// ---------------------------------------------------------------------------------------------
// Feuille tournée : le libellé du rempli reste horizontal, calculé dans le repère de la page
// ---------------------------------------------------------------------------------------------

#[test]
fn pdf_feuille_tournee_libelle_du_rempli_horizontal_dans_le_repere_de_la_page() {
    // Pièce 240 × 60 en (0 ; 0), marge 5, rempli 8 mm sur le bord bas. En portrait A4 (fenêtre
    // 190 × 277) elle demande 2 pages ; tournée d'un quart de tour (x, y) → (−y, x) elle n'en
    // demande qu'une. Le bord bas devient la verticale x = 0 de y = 0 à y = 240, intérieur à gauche
    // (normale intérieure (−1 ; 0)) : centre du libellé à 1,5 + 11,935 = 13,435 mm à gauche du bord,
    // à y = 120.
    let mut p = Project::new("Tourne");
    let s = p.sheets[0].id;
    let (piece, e) = rect(
        &mut p,
        s,
        (0.0, 0.0),
        (240.0, 60.0),
        "Carte",
        PathRole::SewingContour,
    );
    p.apply(Command::SetSeamAllowance {
        piece,
        seam: Some(SeamAllowance {
            default_mm: 5.0,
            folds: vec![FoldAllowance {
                entity: e.bottom,
                value_mm: 8.0,
                notch_step_mm: None,
                notch_count: None,
            }],
            ..Default::default()
        }),
    })
    .unwrap();
    let scene = Scene::from_sheet(&p, s).unwrap();
    let setup = PageSetup {
        overlap_mm: 10.0,
        assembly: AssemblyOptions {
            marks: false,
            plan: false,
            save_paper: true,
        },
        ..PageSetup::default()
    };
    let tiles = plan_tiles_with(&scene, &setup).unwrap();
    assert_eq!((tiles.len(), tiles[0].turns), (1, 1));
    let pdf = export_pdf_with(&[scene], &setup).unwrap();
    let texts = pdf_texts(&pdf.bytes);
    let t = texts
        .iter()
        .find(|t| t.text == "Rempli 8 mm")
        .expect("libellé");
    // Horizontal (pas de matrice de texte), jamais à l'envers.
    assert!(near(t.angle_deg, 0.0, 1e-9));
    let corners = [(0.0, 0.0), (240.0, 0.0), (240.0, 60.0), (0.0, 60.0)];
    let (px, py) = pdf_frame(&pdf.bytes, t.page, &corners, 1);
    let width = pdf_text_width_mm(&pdf.bytes, "Rempli 8 mm", 3.5 * 72.0 / 25.4);
    // Page = monde tourné décalé de (px ; py) : centre (−13,435 ; 120) → départ de la ligne de base.
    assert!(
        near(t.x_mm, px - 13.435 - width / 2.0, 0.02),
        "{} / {}",
        t.x_mm,
        px - 13.435 - width / 2.0
    );
    assert!(
        near(t.y_mm, py + 120.0 - 1.225, 0.02),
        "{} / {}",
        t.y_mm,
        py + 118.775
    );
}

// ---------------------------------------------------------------------------------------------
// Dette PH-5b : libellé de pli sur une feuille tournée d'un quart de tour
// ---------------------------------------------------------------------------------------------

#[test]
fn pdf_feuille_tournee_libelle_de_pli_horizontal_dans_le_repere_de_la_page() {
    // Pièce 240 × 60 en (0 ; 0), sans marge, bord bas sur le pli : ligne de (0 ; 0) à (240 ; 0).
    // Tournée d'un quart de tour (x, y) → (−y, x), elle tient sur une page (portrait 190 × 277) ;
    // la pliure devient la verticale x = 0 de y = 0 à y = 240, l'intérieur est à gauche (normale
    // (−1 ; 0)). Libellé horizontal : centre à 1,5 + demi-largeur du bord de la pliure, soit
    // « SUR LE PLI » 10 × 3,5 × 0,62 / 2 = 10,85 → x = −12,35 ; « ON FOLD » 7 × 3,5 × 0,62 / 2 =
    // 7,595 → x = −9,095 ; à mi-longueur, y = 120 ; ligne de base 1,225 mm plus bas.
    let mut p = Project::new("Pli tourne");
    let s = p.sheets[0].id;
    let (piece, e) = rect(
        &mut p,
        s,
        (0.0, 0.0),
        (240.0, 60.0),
        "Dos",
        PathRole::SewingContour,
    );
    p.apply(Command::SetPieceFold {
        piece,
        edge: e.bottom,
        on: true,
    })
    .unwrap();
    let setup = PageSetup {
        overlap_mm: 10.0,
        assembly: AssemblyOptions {
            marks: false,
            plan: false,
            save_paper: true,
        },
        ..PageSetup::default()
    };
    let corners = [(0.0, 0.0), (240.0, 0.0), (240.0, 60.0), (0.0, 60.0)];
    for (lang, text, centre_x) in [
        (Lang::Fr, "SUR LE PLI", -12.35),
        (Lang::En, "ON FOLD", -9.095),
    ] {
        let scene = Scene::from_sheet_with(
            &p,
            s,
            SceneOptions {
                lang,
                ..SceneOptions::default()
            },
        )
        .unwrap();
        let tiles = plan_tiles_with(&scene, &setup).unwrap();
        assert_eq!((tiles.len(), tiles[0].turns), (1, 1));
        let pdf = export_pdf_with(&[scene], &setup).unwrap();
        let t = pdf_texts(&pdf.bytes)
            .into_iter()
            .find(|t| t.text == text)
            .unwrap_or_else(|| panic!("« {text} » absent"));
        assert!(near(t.angle_deg, 0.0, 1e-9), "horizontal : {}", t.angle_deg);
        let (px, py) = pdf_frame(&pdf.bytes, t.page, &corners, 1);
        let width = pdf_text_width_mm(&pdf.bytes, text, 3.5 * 72.0 / 25.4);
        assert!(
            near(t.x_mm, px + centre_x - width / 2.0, 0.02),
            "{text} : x {} / {}",
            t.x_mm,
            px + centre_x - width / 2.0
        );
        assert!(
            near(t.y_mm, py + 120.0 - 1.225, 0.02),
            "{text} : y {}",
            t.y_mm
        );
        // La pliure elle-même : verticale à x = px, de py à py + 240.
        let line = pdf_segments(&pdf.bytes).into_iter().find(|(pg, s)| {
            *pg == t.page
                && near(s[0], px, 0.01)
                && near(s[2], px, 0.01)
                && near((s[3] - s[1]).abs(), 240.0, 0.01)
        });
        assert!(line.is_some(), "pliure verticale absente");
    }
}
