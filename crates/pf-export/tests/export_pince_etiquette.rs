//! Export des pinces et de l'étiquette de pièce (tranche 69) : un test par critère, sur une pièce
//! rectangulaire de 100 × 50 mm (pince de 8 mm d'ouverture et de 20 mm de long sur le bord bas,
//! jambes de √(4² + 20²) = √416 mm). Valeurs attendues calculées à la main ; fichiers relus par les
//! lecteurs indépendants de `common/`.
//!
//! Honnêteté : aucune ouverture dans un logiciel tiers, aucune impression, aucun traçage réels.

mod common;

use common::coupe::{mm_pt, near, pt, read_pdf as read_strokes, read_svg, rect_project, MODES};
use common::dxf::{self as rd, Ent};
use common::hpgl::read_hpgl;
use pf_core::{Command, PathRole, PieceCategory, PieceId, PointRef, Project, SheetId};
use pf_export::dxf::{export_dxf, DxfIssueKind};
use pf_export::hpgl::export_hpgl;
use pf_export::pdf::export_pdf;
use pf_export::svg::export_svg;
use pf_export::{
    piece_label_lines, OutputCategory as C, OutputContent, PageSetup, Scene, SceneOptions,
};
use pf_geometry::Side;

/// Jambe : √(4² + 20²) = √416.
const LEG: f64 = 20.396_078_054_371_14;

fn a3() -> PageSetup {
    PageSetup {
        format: pf_export::PageFormat::A3,
        ..Default::default()
    }
}

/// Rectangle 100 × 50 « Dos » avec deux points d'ancrage ; pince si `dart`, fiche si `card`.
fn fixture(dart: bool, card: bool) -> (Project, SheetId, PieceId) {
    let (mut p, s, piece) = rect_project(None);
    let apex = p
        .apply(Command::AddPoint {
            sheet: s,
            at: pt(50.0, 20.0),
        })
        .unwrap()
        .points[0];
    let toward = p
        .apply(Command::AddPoint {
            sheet: s,
            at: pt(50.0, 0.0),
        })
        .unwrap()
        .points[0];
    if dart {
        p.apply(Command::AddPieceDart {
            piece,
            apex: PointRef::Existing(apex),
            toward: PointRef::Existing(toward),
            length_mm: 20.0,
            opening_mm: 8.0,
            fixed: Side::Left,
        })
        .unwrap();
    }
    if card {
        set_card(&mut p, piece, "Dos");
    }
    (p, s, piece)
}

fn set_card(p: &mut Project, piece: PieceId, name: &str) {
    p.apply(Command::SetPieceCard {
        piece,
        name: name.into(),
        code: Some("D-7".into()),
        quantity: 2,
        mirrored_pair: true,
        material: Some("Coton".into()),
        category: Some(PieceCategory::Lining),
        comment: Some("Sens du fil\n\n  deux fois  ".into()),
    })
    .unwrap();
}

fn scene_of(p: &Project, s: SheetId) -> Scene {
    Scene::from_sheet(p, s).unwrap()
}

/// Longueur des traits d'un rôle SVG.
fn svg_len(it: &common::coupe::SvgItem) -> f64 {
    (it.pts[1].0 - it.pts[0].0).hypot(it.pts[1].1 - it.pts[0].1)
}

fn dxf_lines(d: &rd::Dxf, layer: &str) -> Vec<((f64, f64), (f64, f64))> {
    d.blocks
        .iter()
        .flat_map(|(_, e)| e)
        .filter_map(|e| match e {
            Ent::Line { layer: l, a, b } if l == layer => Some((*a, *b)),
            _ => None,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Pinces : quatre formats, positions et longueurs
// ---------------------------------------------------------------------------

#[test]
fn pince_jambes_svg_dxf_hpgl_pdf() {
    let (p, s, _) = fixture(true, false);
    let scene = scene_of(&p, s);
    assert_eq!(scene.darts.len(), 1);
    // SVG : bbox = rectangle (0..100 × 0..50) ; x = x + 10, y = 50 − y + 10. Pied « gauche » = à
    // gauche de l axe en regardant du sommet vers la base (ici vers le bas) : x = 54, puis 46.
    let doc = read_svg(&export_svg(&scene).unwrap());
    let legs = doc.with_role("pince");
    assert_eq!(legs.len(), 2);
    for (l, foot_x) in legs.iter().zip([54.0, 46.0]) {
        assert!(near(svg_len(l), LEG, 1e-3));
        assert!(near(l.pts[0].0, 60.0, 1e-3) && near(l.pts[0].1, 40.0, 1e-3));
        assert!(near(l.pts[1].0, foot_x + 10.0, 1e-3) && near(l.pts[1].1, 60.0, 1e-3));
    }
    // DXF : couche 90, deux lignes ; Y vers le haut ; placement automatique = translation.
    let d = rd::parse(&export_dxf(&p).unwrap().text).unwrap();
    let dl = dxf_lines(&d, "90");
    assert_eq!(dl.len(), 2);
    assert!(dl.iter().all(|(a, b)| near(rd::dist(*a, *b), LEG, 1e-3)));
    assert!(near(dl[0].0 .0, dl[1].0 .0, 1e-3) && near(dl[0].0 .1, dl[1].0 .1, 1e-3));
    assert!(near((dl[1].1 .0 - dl[0].1 .0).abs(), 8.0, 1e-3));
    assert!(near(dl[0].0 .1 - dl[0].1 .1, 20.0, 1e-3));
    // HPGL : plume 9, deux traits.
    let r = read_hpgl(&export_hpgl(&scene, &a3()).unwrap().text).unwrap();
    let h: Vec<_> = r.strokes.iter().filter(|s| s.pen == 9).collect();
    assert_eq!(h.len(), 2);
    assert!(h.iter().all(|s| near(s.length_mm(), LEG, 0.05)));
    // PDF : deux traits au pointillé propre (8 ; 1,5 ; 1,5 ; 1,5 ; 1,5 ; 1,5 mm).
    let strokes = read_strokes(&export_pdf(&[scene]).unwrap().bytes);
    let dash = [8.0, 1.5, 1.5, 1.5, 1.5, 1.5].map(mm_pt);
    let pdf: Vec<_> = strokes
        .iter()
        .filter(|s| {
            s.dash_pt.len() == 6 && s.dash_pt.iter().zip(&dash).all(|(a, b)| near(*a, *b, 1e-3))
        })
        .collect();
    assert_eq!(pdf.len(), 2);
    for l in pdf {
        let len_mm = common::coupe::stroke_len_pt(l) * 25.4 / 72.0;
        assert!(near(len_mm, LEG, 1e-2), "{len_mm}");
    }
}

#[test]
fn pince_identique_dans_les_trois_modes_de_sortie() {
    // La pince n'est pas un contour : elle ne dépend pas du mode couture / coupe.
    let (mut p, s, piece) = fixture(true, true);
    p.apply(Command::SetSeamAllowance {
        piece,
        seam: Some(pf_core::SeamAllowance {
            default_mm: 10.0,
            ..Default::default()
        }),
    })
    .unwrap();
    for mode in MODES {
        let scene = common::coupe::scene_with(&p, s, mode);
        assert_eq!(scene.darts.len(), 1, "{mode:?}");
        assert_eq!(scene.piece_labels.len(), 1, "{mode:?}");
        let doc = read_svg(&export_svg(&scene).unwrap());
        assert_eq!(doc.with_role("pince").len(), 2, "{mode:?}");
    }
}

#[test]
fn pince_motif_role_et_calque_sont_distincts_de_tous_les_autres() {
    // Pointillé : aucun rôle de tracé ni la coupe calculée n'a ce motif.
    for role in PathRole::ALL {
        assert_ne!(
            pf_export::stroke_style(role).dash_mm,
            pf_export::DART_DASH_MM
        );
    }
    assert_ne!(pf_export::COMPUTED_CUT_DASH_MM, pf_export::DART_DASH_MM);
    // Calque DXF : 90, absent des autres rôles. Plume HPGL : 9, absente des autres.
    let layers: Vec<u32> = pf_export::dxf::LAYERS.iter().map(|l| l.number).collect();
    assert_eq!(layers.iter().filter(|n| **n == 90).count(), 1);
    let pens: Vec<u8> = pf_export::hpgl::HpglPen::ALL
        .iter()
        .map(|p| p.number())
        .collect();
    assert_eq!(pens.iter().filter(|n| **n == 9).count(), 1);
}

#[test]
fn pince_non_calculable_omise_et_signalee() {
    let (mut p, s, piece) = fixture(true, false);
    // Le sommet sort de la pièce : pince non calculable (PieceIssue::DartInvalid), jamais refusée.
    let apex = p.piece(piece).unwrap().darts[0].apex;
    p.apply(Command::MovePoint {
        sheet: s,
        point: apex,
        to: pt(500.0, 500.0),
    })
    .unwrap();
    assert!(p
        .piece_issues(piece)
        .iter()
        .any(|i| format!("{i:?}").contains("DartInvalid")));
    let scene = scene_of(&p, s);
    assert!(scene.darts.is_empty());
    assert_eq!(scene.dart_warnings.len(), 1);
    let w = &scene.dart_warnings[0];
    assert!(w.piece == "Dos" && !w.reason.is_empty());
    assert!(w.to_string().contains("« Dos »") && w.to_string().contains("non exportée"));
    // Aucun fichier incomplet en silence : SVG et HPGL sortent sans jambe, le DXF le signale.
    assert!(!export_svg(&scene).unwrap().contains("pince"));
    let r = read_hpgl(&export_hpgl(&scene, &a3()).unwrap().text).unwrap();
    assert!(r.strokes.iter().all(|s| s.pen != 9));
    let out = export_dxf(&p).unwrap();
    let issue = out
        .report
        .issues
        .iter()
        .find(|i| i.kind == DxfIssueKind::DartUnreadable)
        .expect("avertissement DXF");
    assert!(issue.kind.is_omission() && issue.count == 1 && issue.piece == "Dos");
    assert!(
        !out.text.contains("\n90\n"),
        "couche 90 non déclarée sans jambe"
    );
    // Une pince calculable et une autre non : seule la première est écrite, l'autre signalée.
    let (mut p, s, piece) = fixture(true, false);
    let apex2 = p
        .apply(Command::AddPoint {
            sheet: s,
            at: pt(20.0, 20.0),
        })
        .unwrap()
        .points[0];
    let toward2 = p
        .apply(Command::AddPoint {
            sheet: s,
            at: pt(20.0, 0.0),
        })
        .unwrap()
        .points[0];
    p.apply(Command::AddPieceDart {
        piece,
        apex: PointRef::Existing(apex2),
        toward: PointRef::Existing(toward2),
        length_mm: 20.0,
        opening_mm: 8.0,
        fixed: Side::Right,
    })
    .unwrap();
    p.apply(Command::MovePoint {
        sheet: s,
        point: apex2,
        to: pt(-300.0, 20.0),
    })
    .unwrap();
    let scene = scene_of(&p, s);
    assert_eq!((scene.darts.len(), scene.dart_warnings.len()), (1, 1));
}

// ---------------------------------------------------------------------------
// Non-régression : sans pince et sans fiche, rien ne change ; les filtres retirent exactement
// ---------------------------------------------------------------------------

#[test]
fn sans_pince_ni_fiche_aucun_element_nouveau() {
    let (p, s, _) = fixture(false, false);
    let scene = scene_of(&p, s);
    assert!(scene.darts.is_empty() && scene.piece_labels.is_empty());
    let svg = export_svg(&scene).unwrap();
    assert!(!svg.contains("pince") && !svg.contains("etiquette-piece"));
    let hpgl = export_hpgl(&scene, &a3()).unwrap();
    assert!(!hpgl.text.contains("SP9") && !hpgl.text.contains("LB") && hpgl.report.labels == 0);
    let dxf = export_dxf(&p).unwrap();
    assert!(!dxf.text.contains("\n90\n"));
    assert!(dxf.report.layers.iter().all(|l| l.layer.number != 90));
    // La couche 90 n'est pas déclarée : la table n'a que ses onze couches d'avant.
    assert_eq!(dxf.text.matches("  0\nLAYER\n").count(), 1 + 11);
}

#[test]
fn filtres_pinces_et_etiquettes_retirent_exactement_leurs_elements() {
    let (with, s, _) = fixture(true, true);
    let (without, s2, _) = fixture(false, false);
    let full = scene_of(&with, s);
    let none = OutputContent::default()
        .without(C::Darts)
        .without(C::PieceLabels);
    let filtered = none.apply(&full).unwrap();
    // Les deux scènes (avec pince et fiche filtrées / sans pince ni fiche) s'exportent à l'octet.
    let base = scene_of(&without, s2);
    assert_eq!(filtered.darts.len() + filtered.piece_labels.len(), 0);
    assert_eq!(export_svg(&filtered).unwrap(), export_svg(&base).unwrap());
    assert_eq!(
        export_pdf(std::slice::from_ref(&filtered)).unwrap().bytes,
        export_pdf(std::slice::from_ref(&base)).unwrap().bytes
    );
    assert_eq!(
        export_hpgl(&filtered, &a3()).unwrap().text,
        export_hpgl(&base, &a3()).unwrap().text
    );
    // Chaque catégorie indépendamment.
    let no_darts = OutputContent::default()
        .without(C::Darts)
        .apply(&full)
        .unwrap();
    assert!(no_darts.darts.is_empty() && no_darts.piece_labels.len() == 1);
    let no_labels = OutputContent::default()
        .without(C::PieceLabels)
        .apply(&full)
        .unwrap();
    assert!(no_labels.piece_labels.is_empty() && no_labels.darts.len() == 1);
    // Décompte du récapitulatif : une pince, quatre lignes de fiche (nom, code, quantité,
    // matière, catégorie, deux lignes de commentaire = sept).
    assert_eq!(pf_export::content::count(&full, C::Darts), 1);
    assert_eq!(pf_export::content::count(&full, C::PieceLabels), 1);
    assert_eq!(full.piece_labels[0].lines.len(), 7);
    // Seules les pinces : l'export reste possible.
    let only = OutputContent::only(&[C::Darts]).apply(&full).unwrap();
    assert!(export_svg(&only).unwrap().contains("data-role=\"pince\""));
}

// ---------------------------------------------------------------------------
// Étiquette de pièce : champs, placement, texte vide, accents, calques masqués
// ---------------------------------------------------------------------------

#[test]
fn etiquette_champs_et_ordre() {
    let (p, s, piece) = fixture(false, true);
    let lines = piece_label_lines(p.piece(piece).unwrap());
    assert_eq!(
        lines,
        [
            "Dos",
            "Code : D-7",
            "Quantité : 2 (paire, ×2 en miroir)",
            "Matière : Coton",
            "Catégorie : Doublure",
            "Sens du fil",
            "deux fois"
        ]
    );
    assert_eq!(scene_of(&p, s).piece_labels[0].lines, lines);
}

#[test]
fn etiquette_placement_au_centre_de_l_emprise_sans_droit_fil() {
    let (p, s, _) = fixture(false, true);
    let scene = scene_of(&p, s);
    // Sans droit-fil : centre de l'emprise du contour = (50 ; 25).
    let l = &scene.piece_labels[0];
    assert!(near(l.center.x, 50.0, 1e-9) && near(l.center.y, 25.0, 1e-9));
    // SVG : bbox = rectangle ; sept lignes de 5 mm de pas, ligne de base = centre + (3 − i) × 5
    // − 1,4 ; en SVG y = 60 − y.
    let svg = export_svg(&scene).unwrap();
    let ys: Vec<f64> = svg
        .lines()
        .filter(|l| l.contains("etiquette-piece"))
        .map(|l| common::coupe::attr(l, "y").unwrap().parse::<f64>().unwrap())
        .collect();
    assert_eq!(ys.len(), 7);
    for (i, y) in ys.iter().enumerate() {
        assert!(
            near(*y, 60.0 - (25.0 + (3.0 - i as f64) * 5.0 - 1.4), 1e-3),
            "{i} {y}"
        );
    }
}

#[test]
fn etiquette_texte_vide_ou_fiche_non_renseignee_n_ecrit_rien() {
    let (mut p, s, piece) = fixture(false, false);
    // Nom seul : rien.
    assert!(scene_of(&p, s).piece_labels.is_empty());
    // Champs vides ou blancs : rien (un champ vide n'écrit pas de ligne).
    p.apply(Command::SetPieceCard {
        piece,
        name: "Dos".into(),
        code: Some("   ".into()),
        quantity: 1,
        mirrored_pair: false,
        material: Some(String::new()),
        category: None,
        comment: Some(" \n\t \n".into()),
    })
    .unwrap();
    assert!(piece_label_lines(p.piece(piece).unwrap()).is_empty());
    assert!(scene_of(&p, s).piece_labels.is_empty());
    // Une quantité différente de 1 suffit : nom + quantité seulement.
    p.apply(Command::SetPieceCard {
        piece,
        name: "Dos".into(),
        code: None,
        quantity: 3,
        mirrored_pair: false,
        material: None,
        category: None,
        comment: None,
    })
    .unwrap();
    assert_eq!(
        piece_label_lines(p.piece(piece).unwrap()),
        ["Dos", "Quantité : 3"]
    );
}

#[test]
fn etiquette_accents_svg_fideles_pdf_police_embarquee_hpgl_ascii_dxf_unicode() {
    let (mut p, s, piece) = fixture(false, false);
    // « é » et « Ž » sont dans la police embarquée du PDF (US-06.6) ; HPGL (ASCII) : « ? ».
    p.apply(Command::SetPieceCard {
        piece,
        name: "Écharpe Ž; test".into(),
        code: None,
        quantity: 2,
        mirrored_pair: false,
        material: None,
        category: None,
        comment: None,
    })
    .unwrap();
    let scene = scene_of(&p, s);
    assert!(export_svg(&scene)
        .unwrap()
        .contains(">Écharpe Ž; test</text>"));
    let pages = common::read_pdf(&export_pdf(std::slice::from_ref(&scene)).unwrap().bytes);
    assert!(pages
        .iter()
        .any(|pg| pg.strings.iter().any(|t| t == "Écharpe Ž; test")));
    let plot = export_hpgl(&scene, &a3()).unwrap();
    let r = read_hpgl(&plot.text).unwrap();
    // `;` terminerait l'instruction : remplacé par une virgule ; accents retirés ; « ? » sinon.
    assert_eq!(r.labels[0].text, "Echarpe ?, test");
    let dxf = export_dxf(&p).unwrap();
    assert!(dxf.text.contains("\\U+00C9charpe \\U+017D; test"));
}

#[test]
fn pince_et_etiquette_d_une_piece_masquee_ne_sont_pas_exportees() {
    let (mut p, s, piece) = fixture(true, true);
    let outline: Vec<_> = p
        .piece(piece)
        .unwrap()
        .outline
        .iter()
        .map(|e| e.entity)
        .collect();
    let layer = p
        .apply(Command::AddLayer {
            sheet: s,
            name: "Cachée".into(),
        })
        .unwrap()
        .layers[0];
    p.apply(Command::SetObjectsLayer {
        sheet: s,
        points: Vec::new(),
        entities: outline,
        layer: Some(layer),
    })
    .unwrap();
    p.apply(Command::SetLayerVisible {
        sheet: s,
        layer,
        visible: false,
    })
    .unwrap();
    let scene = Scene::from_sheet(&p, s).unwrap();
    assert!(scene.darts.is_empty() && scene.piece_labels.is_empty());
    // Avec les calques masqués demandés, la pièce, sa pince et son étiquette reviennent.
    let all = Scene::from_sheet_with(
        &p,
        s,
        SceneOptions {
            include_hidden_layers: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!((all.darts.len(), all.piece_labels.len()), (1, 1));
}

#[test]
fn hpgl_etiquette_dans_l_emprise_et_ordre_des_plumes() {
    let (p, s, _) = fixture(true, true);
    let scene = scene_of(&p, s);
    let plot = export_hpgl(&scene, &a3()).unwrap();
    let r = read_hpgl(&plot.text).unwrap();
    // Plumes sélectionnées une fois chacune, par numéro croissant (étiquette : plume 7).
    let mut sorted = r.selections.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(r.selections, sorted, "{:?}", r.selections);
    assert!(r.selections.contains(&7) && r.selections.contains(&9));
    // Aucune coordonnée négative : l'étiquette est dans l'emprise.
    assert!(r.labels.iter().all(|l| l.at.0 >= 0 && l.at.1 >= 0));
    assert_eq!(r.labels.len(), 7);
    assert_eq!(plot.report.labels, 7);
}
