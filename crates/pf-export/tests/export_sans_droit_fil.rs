//! PH-5d — « Sans droit-fil » / « No grain » : étiquette de pièce des quatre exports, rapport DXF
//! (`MissingGrain`), rapport de contrôle (CSV, PDF) et scénario porte-cartes rejoué. Lecteurs et
//! fixtures de `lecteurs_libelles/` (indépendants de `common/`), valeurs écrites **à la main**
//! (mm, Y vers le haut).
//!
//! Pièce « Carte » : rectangle 100 × 60 en (10 ; 20), centre du bloc d'étiquette (60 ; 50) ; ligne
//! `i` sur `n` : ligne de base y = 50 + ((n − 1) / 2 − i) × 5 − 1,4 (pas 5 mm, corps 4 mm).
//! - sans fiche : lignes « Carte » puis la mention (n = 2) : y = 51,1 et 46,1 ;
//! - avec fiche (code C1) : « Carte », « Code : C1 », « Quantité : 1 », mention (n = 4, i = 3) :
//!   y = 41,1 ;
//! - en coupe nette avec fiche : mention de coupe nette puis « Sans droit-fil » (n = 5, i = 4) :
//!   y = 50 + (2 − 4) × 5 − 1,4 = 38,6.
//!
//! Honnêteté : aucune ouverture dans un logiciel tiers, aucune impression, aucun traçage réels.

#[path = "scenario_porte_cartes/construction.rs"]
mod construction;
mod lecteurs_libelles;

use lecteurs_libelles::*;
use pf_core::{Command, PathRole, PieceId, Project, SheetId};
use pf_export::dxf::{export_dxf_with, DxfIssueKind};
use pf_export::hpgl::export_hpgl;
use pf_export::pdf::export_pdf;
use pf_export::report::{export_control_report, ReportFormat};
use pf_export::svg::export_svg;
use pf_export::{
    OutputCategory as C, OutputContent, PageFormat, PageSetup, PieceExportMode, Scene, SceneOptions,
};
use pf_i18n::Lang;

struct Fx {
    p: Project,
    s: SheetId,
    piece: PieceId,
}

const CORNERS: [(f64, f64); 4] = [(10.0, 20.0), (110.0, 20.0), (110.0, 80.0), (10.0, 80.0)];

fn base() -> Fx {
    let mut p = Project::new("Etiquettes");
    let s = p.sheets[0].id;
    let (piece, _) = rect(
        &mut p,
        s,
        (10.0, 20.0),
        (100.0, 60.0),
        "Carte",
        PathRole::SewingContour,
    );
    Fx { p, s, piece }
}

fn no_grain(fx: &mut Fx) {
    fx.p.apply(Command::SetPieceNoGrain {
        piece: fx.piece,
        no_grain: true,
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

/// « Carte » avec fiche et déclarée sans droit-fil.
fn declared() -> Fx {
    let mut fx = base();
    set_fiche(&mut fx);
    no_grain(&mut fx);
    fx
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

fn svg_label_lines(svg: &str) -> Vec<SvgText> {
    svg_texts(svg)
        .into_iter()
        .filter(|t| t.role == "etiquette-piece")
        .collect()
}

fn dxf_min(dxf: &str, layer: &str) -> (f64, f64) {
    let v = dxf_layer_vertices(dxf, layer);
    assert!(!v.is_empty(), "couche {layer} vide");
    (
        v.iter().map(|p| p.0).fold(f64::MAX, f64::min),
        v.iter().map(|p| p.1).fold(f64::MAX, f64::min),
    )
}

// ---------------------------------------------------------------------------------------------
// Étiquette de pièce
// ---------------------------------------------------------------------------------------------

#[test]
fn svg_derniere_ligne_de_l_etiquette_fr_et_en() {
    let fx = declared();
    for (lang, mention) in [(Lang::Fr, "Sans droit-fil"), (Lang::En, "No grain")] {
        let svg = export_svg(&scene_of(&fx, lang, OutputContent::default())).unwrap();
        let (dx, dy) = svg_frame(&svg, &CORNERS);
        let lines = svg_label_lines(&svg);
        let texts: Vec<&str> = lines.iter().map(|t| t.text.as_str()).collect();
        assert_eq!(texts, ["Carte", "Code : C1", "Quantité : 1", mention]);
        assert!(near(lines[3].x, 60.0 + dx, 1e-3), "{}", lines[3].x);
        assert!(near(lines[3].y, dy - 41.1, 1e-3), "{}", lines[3].y);
    }
}

#[test]
fn sans_fiche_le_nom_puis_la_mention_et_sans_declaration_aucune_trace() {
    let mut fx = base();
    no_grain(&mut fx);
    let sc = scene_of(&fx, Lang::Fr, OutputContent::default());
    assert_eq!(sc.piece_labels.len(), 1);
    assert_eq!(sc.piece_labels[0].lines, ["Carte", "Sans droit-fil"]);
    let svg = export_svg(&sc).unwrap();
    let (dx, dy) = svg_frame(&svg, &CORNERS);
    let lines = svg_label_lines(&svg);
    assert!(near(lines[0].y, dy - 51.1, 1e-3) && near(lines[1].y, dy - 46.1, 1e-3));
    assert!(near(lines[1].x, 60.0 + dx, 1e-3));
    // Sans déclaration : ni étiquette ni mention, dans aucun des formats textuels.
    let plain = base();
    let sc = scene_of(&plain, Lang::Fr, OutputContent::default());
    assert!(sc.piece_labels.is_empty());
    let svg = export_svg(&sc).unwrap();
    assert!(!svg.contains("Sans droit-fil") && !svg.contains("etiquette-piece"));
    let dxf = export_dxf_with(&plain.p, PieceExportMode::Both)
        .unwrap()
        .text;
    assert!(!dxf.contains("No grain"));
    // Fiche renseignée sans déclaration : étiquette ordinaire.
    let mut fiche = base();
    set_fiche(&mut fiche);
    let sc = scene_of(&fiche, Lang::Fr, OutputContent::default());
    assert_eq!(
        sc.piece_labels[0].lines,
        ["Carte", "Code : C1", "Quantité : 1"]
    );
}

#[test]
fn coupe_nette_puis_sans_droit_fil_dans_cet_ordre() {
    let mut fx = declared();
    net_cut(&mut fx);
    let svg = export_svg(&scene_of(&fx, Lang::Fr, OutputContent::default())).unwrap();
    let (dx, dy) = svg_frame(&svg, &CORNERS);
    let lines = svg_label_lines(&svg);
    let texts: Vec<&str> = lines.iter().map(|t| t.text.as_str()).collect();
    assert_eq!(
        texts,
        [
            "Carte",
            "Code : C1",
            "Quantité : 1",
            "Coupe nette",
            "Sans droit-fil"
        ]
    );
    assert!(near(lines[4].y, dy - 38.6, 1e-3), "{}", lines[4].y);
    assert!(near(lines[4].x, 60.0 + dx, 1e-3));
    // Coupe nette seule : inchangé (aucune mention de droit-fil).
    let mut only = base();
    set_fiche(&mut only);
    net_cut(&mut only);
    let sc = scene_of(&only, Lang::Fr, OutputContent::default());
    assert_eq!(
        sc.piece_labels[0].lines,
        ["Carte", "Code : C1", "Quantité : 1", "Coupe nette"]
    );
}

#[test]
fn pdf_hpgl_dxf_mention() {
    let fx = declared();
    // PDF : Noto Sans 4 mm, centrée sur x = 60, ligne de base y = 41,1.
    let pdf = export_pdf(&[scene_of(&fx, Lang::Fr, OutputContent::default())]).unwrap();
    let texts = pdf_texts(&pdf.bytes);
    let t = texts
        .iter()
        .find(|t| t.text == "Sans droit-fil")
        .expect("mention PDF");
    let (px, py) = pdf_frame(&pdf.bytes, t.page, &CORNERS, 0);
    let width = pdf_text_width_mm(&pdf.bytes, "Sans droit-fil", 4.0 * 72.0 / 25.4);
    assert!(near(t.x_mm, px + 60.0 - width / 2.0, 0.02), "{}", t.x_mm);
    assert!(near(t.y_mm, py + 41.1, 0.02), "{}", t.y_mm);
    // HPGL : plume 7, ASCII ; 14 caractères × 2,2 mm : départ x = (60 − 15,4 − 10) × 40 = 1 384,
    // y = (41,1 − 20) × 40 = 844 (origine = coin bas-gauche du contour).
    let plot = export_hpgl(&scene_of(&fx, Lang::Fr, OutputContent::default()), &roll())
        .unwrap()
        .text;
    let l = hpgl_labels(&plot)
        .into_iter()
        .find(|l| l.text == "Sans droit-fil")
        .expect("mention HPGL");
    assert_eq!(l.pen, 7);
    assert!((l.x - 1384).abs() <= 1 && (l.y - 844).abs() <= 1, "{l:?}");
    // DXF : anglais, couche 84, dernière ligne du bloc ; n = 4, k = 3 : y = 50 + 9 − 18 − 2 = 39 ;
    // « No grain » 8 × 4 × 0,6 = 19,2 mm : x = 60 − 9,6 = 50,4.
    let dxf = export_dxf_with(&fx.p, PieceExportMode::Both).unwrap().text;
    let all = dxf_texts(&dxf);
    let m = all
        .iter()
        .find(|t| t.text == "No grain")
        .expect("mention DXF");
    assert_eq!(m.layer, "84");
    let (mx, my) = dxf_min(&dxf, "15");
    assert!(
        near(m.x, 50.4 - 10.0 + mx, 1e-3) && near(m.y, 39.0 - 20.0 + my, 1e-3),
        "{m:?}"
    );
    let order: Vec<&str> = all.iter().map(|t| t.text.as_str()).collect();
    assert_eq!(
        order,
        ["Piece Name: Carte", "Code: C1", "Quantity: 1", "No grain"]
    );
}

#[test]
fn filtre_de_contenu_etiquettes_de_piece_retire_la_mention() {
    let fx = declared();
    let sc = scene_of(
        &fx,
        Lang::Fr,
        OutputContent::default().without(C::PieceLabels),
    );
    assert!(sc.piece_labels.is_empty());
    assert!(!export_svg(&sc).unwrap().contains("Sans droit-fil"));
    assert!(export_pdf(std::slice::from_ref(&sc))
        .map(|p| pdf_texts(&p.bytes)
            .iter()
            .all(|t| !t.text.contains("droit-fil")))
        .unwrap());
    // Une autre catégorie exclue ne touche pas la mention.
    let sc = scene_of(
        &fx,
        Lang::Fr,
        OutputContent::default().without(C::SpecialLines),
    );
    assert!(export_svg(&sc).unwrap().contains(">Sans droit-fil<"));
}

// ---------------------------------------------------------------------------------------------
// Rapport DXF et rapport de contrôle
// ---------------------------------------------------------------------------------------------

fn dxf_missing_grain(p: &Project) -> Vec<(String, usize)> {
    export_dxf_with(p, PieceExportMode::Both)
        .unwrap()
        .report
        .issues
        .iter()
        .filter(|i| i.kind == DxfIssueKind::MissingGrain)
        .map(|i| (i.piece.clone(), i.count))
        .collect()
}

#[test]
fn rapport_dxf_plus_de_missing_grain_pour_la_piece_declaree() {
    let mut fx = base();
    assert_eq!(dxf_missing_grain(&fx.p), [("Carte".to_string(), 1)]);
    no_grain(&mut fx);
    assert!(dxf_missing_grain(&fx.p).is_empty());
    // Deux pièces : seule celle qui n'est pas déclarée reste signalée.
    let mut p = Project::new("Deux");
    let s = p.sheets[0].id;
    let (a, _) = rect(
        &mut p,
        s,
        (0.0, 0.0),
        (50.0, 50.0),
        "A",
        PathRole::SewingContour,
    );
    rect(
        &mut p,
        s,
        (100.0, 0.0),
        (50.0, 50.0),
        "B",
        PathRole::SewingContour,
    );
    p.apply(Command::SetPieceNoGrain {
        piece: a,
        no_grain: true,
    })
    .unwrap();
    assert_eq!(dxf_missing_grain(&p), [("B".to_string(), 1)]);
}

fn anomaly_rows(rows: &[Vec<String>]) -> Vec<&Vec<String>> {
    rows.iter().filter(|r| r[0] == "anomalie").collect()
}

#[test]
fn rapport_de_controle_csv_et_pdf_sans_anomalie_de_droit_fil_pour_la_piece_declaree() {
    let mut p = Project::new("Rapport");
    let s = p.sheets[0].id;
    let (a, _) = rect(
        &mut p,
        s,
        (0.0, 0.0),
        (50.0, 50.0),
        "Alpha",
        PathRole::SewingContour,
    );
    rect(
        &mut p,
        s,
        (100.0, 0.0),
        (50.0, 50.0),
        "Beta",
        PathRole::SewingContour,
    );
    let csv =
        |p: &Project, lang| csv_rows(&export_control_report(p, ReportFormat::Csv, lang).bytes);
    // Sans déclaration : deux anomalies de pièce (Alpha et Beta), « droit-fil » dans le texte.
    let rows = csv(&p, Lang::Fr);
    let before: Vec<String> = anomaly_rows(&rows).iter().map(|r| r.join("|")).collect();
    assert_eq!(before.len(), 2, "{before:?}");
    assert!(before.iter().all(|r| r.contains("droit-fil")), "{before:?}");
    assert!(
        before[0].contains("Alpha") && before[1].contains("Beta"),
        "{before:?}"
    );
    p.apply(Command::SetPieceNoGrain {
        piece: a,
        no_grain: true,
    })
    .unwrap();
    for lang in [Lang::Fr, Lang::En] {
        let rows = csv(&p, lang);
        let after: Vec<String> = anomaly_rows(&rows).iter().map(|r| r.join("|")).collect();
        assert_eq!(after.len(), 1, "{lang:?} {after:?}");
        assert!(
            after[0].contains("Beta") && !after[0].contains("Alpha"),
            "{after:?}"
        );
    }
    // PDF : le nom de la pièce déclarée n'apparaît plus dans la liste des anomalies.
    for (lang, word) in [(Lang::Fr, "droit-fil"), (Lang::En, "grain")] {
        let pdf = export_control_report(&p, ReportFormat::Pdf, lang);
        let texts = pdf_texts(&pdf.bytes);
        let n = texts
            .iter()
            .filter(|t| t.text.to_lowercase().contains(word))
            .count();
        let before_pdf = {
            let mut q = p.clone();
            q.apply(Command::SetPieceNoGrain {
                piece: a,
                no_grain: false,
            })
            .unwrap();
            let pdf = export_control_report(&q, ReportFormat::Pdf, lang);
            pdf_texts(&pdf.bytes)
                .iter()
                .filter(|t| t.text.to_lowercase().contains(word))
                .count()
        };
        assert!(n < before_pdf, "{lang:?} : {n} >= {before_pdf}");
    }
}

// ---------------------------------------------------------------------------------------------
// Scénario porte-cartes rejoué, puis déclaré (les exemples ne sont pas migrés)
// ---------------------------------------------------------------------------------------------

#[test]
fn scenario_porte_cartes_rejoue_puis_declare_sans_droit_fil() {
    let mut pc = construction::build().expect("scénario sans refus");
    let (body, pocket, flap) = (pc.body, pc.pocket, pc.flap);
    // Avant : trois pièces en cuir, trois avertissements et trois remarques DXF.
    for id in [body, pocket, flap] {
        assert_eq!(pc.project.piece_issues(id).len(), 1);
    }
    assert_eq!(dxf_missing_grain(&pc.project).len(), 3);
    for id in [body, pocket, flap] {
        pc.project
            .apply(Command::SetPieceNoGrain {
                piece: id,
                no_grain: true,
            })
            .unwrap();
    }
    // Après : plus aucun avertissement, plus de remarque DXF.
    for id in [body, pocket, flap] {
        assert!(pc.project.piece_issues(id).is_empty());
    }
    assert!(dxf_missing_grain(&pc.project).is_empty());
    // Étiquettes : la mention est la dernière ligne de chacune des trois pièces (le fond et la
    // patte, en coupe nette, la portent après « Coupe nette »).
    let sc = OutputContent::default()
        .scene(&pc.project, pc.sheet, SceneOptions::default())
        .unwrap();
    assert_eq!(sc.piece_labels.len(), 3);
    for l in &sc.piece_labels {
        assert_eq!(l.lines.last().unwrap(), "Sans droit-fil", "{}", l.piece);
        let n = l.lines.len();
        let net = l.piece != "Poche";
        assert_eq!(n, if net { 7 } else { 6 }, "{}", l.piece);
        if net {
            assert_eq!(l.lines[n - 2], "Coupe nette");
        }
    }
    // Les exemples livrés (sans déclaration) gardent leurs octets : le projet non déclaré
    // se réécrit à l'identique du fichier d'exemple.
    let fresh = construction::build().unwrap();
    let on_disk = std::fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/porte-cartes.pforge"),
    )
    .unwrap();
    assert_eq!(
        pf_core::persist::to_json(&fresh.project)
            .unwrap()
            .replace("\r\n", "\n"),
        on_disk.replace("\r\n", "\n")
    );
}
