//! PH-5b (CA3) — un bord « sur le pli » est écrit comme pliure (trait propre + « SUR LE PLI » /
//! « ON FOLD »), ni couture ni coupe : PDF, SVG, DXF (couche 91) et HPGL (plume 8, libellé plume 7)
//! le relisent avec les lecteurs indépendants de `common/`. Valeurs calculées **à la main**.
//!
//! Pièce : rectangle 100 × 100 en (0 ; 0), contour de rôle couture, marge 10 mm, bord **gauche**
//! (x = 0, de (0 ; 100) à (0 ; 0) en anti-horaire) sur le pli.
//!
//! - ligne de pliure dans la coupe : de (0 ; 110) à (0 ; −10), longueur 120 ;
//! - coupe restante, suite ouverte : (0 ; −10) → (110 ; −10) → (110 ; 110) → (0 ; 110), 110 + 120 +
//!   110 = 340 mm (fermée elle ferait 460) ;
//! - couture dessinée : trois bords (bas, droite, haut), 300 mm ;
//! - libellé « SUR LE PLI » : 10 caractères × 3,5 mm × 0,62 = 21,7 mm, demi-largeur 10,85 ; intérieur à
//!   droite du bord (gauche de la pliure orientée vers le bas) : centre à 1,5 + 10,85 = 12,35 mm de la
//!   pliure, à mi-hauteur (y = 50) ; ligne de base à 0,35 × 3,5 = 1,225 mm sous le centre ;
//! - « ON FOLD » : 7 × 3,5 × 0,62 = 15,19 mm, demi-largeur 7,595 ; centre à x = 1,5 + 7,595 = 9,095.
//!
//! Honnêteté : aucune ouverture dans un logiciel tiers, aucune impression, aucun traçage réels.

mod common;

use common::coupe::{attr, mm_pt, near, pt, read_pdf, read_svg, same_set, MODES};
use common::dxf::{self as rd, Ent};
use common::dxf_check::placed;
use common::dxf_models::rect_piece;
use common::hpgl::read_hpgl;
use pf_core::{Command, EntityId, PathRole, PieceId, Project, SeamAllowance, SheetId};
use pf_export::dxf::export_dxf_with;
use pf_export::hpgl::export_hpgl;
use pf_export::pdf::export_pdf;
use pf_export::svg::export_svg;
use pf_export::{
    OutputCategory as C, OutputContent, PageSetup, PieceExportMode, Scene, SceneOptions,
};
use pf_i18n::Lang;

const FOLD: [(f64, f64); 2] = [(0.0, 110.0), (0.0, -10.0)];

struct Fx {
    p: Project,
    s: SheetId,
    piece: PieceId,
    left: EntityId,
}

fn fixture(margin: Option<f64>, fold: bool) -> Fx {
    let mut p = Project::new("Pliure");
    let s = p.sheets[0].id;
    let (piece, ents) = rect_piece(
        &mut p,
        s,
        (0.0, 0.0),
        (100.0, 100.0),
        "Demi-devant",
        PathRole::SewingContour,
        false,
    );
    if let Some(m) = margin {
        p.apply(Command::SetSeamAllowance {
            piece,
            seam: Some(SeamAllowance {
                default_mm: m,
                ..Default::default()
            }),
        })
        .unwrap();
    }
    let sheet = p.sheet(s).unwrap();
    let left = ents
        .into_iter()
        .find(|e| {
            matches!(sheet.shape(*e), Ok(pf_core::Shape::Line(a, b)) if a.x == 0.0 && b.x == 0.0)
        })
        .expect("bord gauche");
    if fold {
        p.apply(Command::SetPieceFold {
            piece,
            edge: left,
            on: true,
        })
        .unwrap();
    }
    Fx { p, s, piece, left }
}

fn scene(f: &Fx, mode: PieceExportMode, lang: Lang) -> Scene {
    Scene::from_sheet_with(
        &f.p,
        f.s,
        SceneOptions {
            piece_mode: mode,
            lang,
            ..SceneOptions::default()
        },
    )
    .unwrap()
}

fn geom_ends(g: &pf_export::PathGeom) -> Option<((f64, f64), (f64, f64))> {
    match g {
        pf_export::PathGeom::Line(a, b) => Some(((a.x, a.y), (b.x, b.y))),
        _ => None,
    }
}

#[test]
fn la_scene_separe_pliure_couture_et_coupe() {
    let f = fixture(Some(10.0), true);
    let sc = scene(&f, PieceExportMode::Both, Lang::Fr);
    // Une pliure, de (0 ; 110) à (0 ; −10), libellé français.
    assert_eq!(sc.folds.len(), 1);
    let fold = &sc.folds[0];
    assert_eq!(fold.label, "SUR LE PLI");
    assert!(near(fold.from.x, 0.0, 1e-9) && near(fold.from.y, 110.0, 1e-9));
    assert!(near(fold.to.x, 0.0, 1e-9) && near(fold.to.y, -10.0, 1e-9));
    // Libellé à 12,35 mm de la pliure, à mi-hauteur.
    let c = fold.text_center();
    assert!(near(c.x, 12.35, 1e-9) && near(c.y, 50.0, 1e-9), "{c:?}");
    // Couture : trois bords seulement (le bord sur le pli n'est pas écrit comme couture).
    let sewing: Vec<_> = sc
        .paths
        .iter()
        .filter(|p| p.role == PathRole::SewingContour)
        .collect();
    assert_eq!(sewing.len(), 3);
    assert!(sewing.iter().all(|p| {
        geom_ends(&p.geom).is_some_and(|(a, b)| !(near(a.0, 0.0, 1e-9) && near(b.0, 0.0, 1e-9)))
    }));
    // Coupe : une suite ouverte de trois bords.
    assert_eq!(sc.cut_contours.len(), 1);
    assert!(!sc.cut_contours[0].closed);
    assert_eq!(sc.cut_contours[0].edges.len(), 3);
    // Sans déclaration : aucune pliure, coupe fermée de quatre bords.
    let plain = scene(&fixture(Some(10.0), false), PieceExportMode::Both, Lang::Fr);
    assert!(plain.folds.is_empty());
    assert!(plain.cut_contours[0].closed);
    assert_eq!(plain.cut_contours[0].edges.len(), 4);
}

#[test]
fn svg_pliure_trait_propre_libelle_et_coupe_ouverte() {
    let f = fixture(Some(10.0), true);
    let svg = export_svg(&scene(&f, PieceExportMode::Both, Lang::Fr)).unwrap();
    let doc = read_svg(&svg);
    // Repère SVG : x' = x + 10, y' = 110 − y + 10 (emprise : x de 0 à 110, y de −10 à 110).
    let fold = doc.with_role("sur-le-pli");
    assert_eq!(fold.len(), 1);
    assert!(same_set(&fold[0].pts, &[(10.0, 10.0), (10.0, 130.0)], 1e-6));
    assert_eq!(fold[0].dash.as_deref(), Some("20 3 1 3"));
    assert_eq!(fold[0].width.as_deref(), Some("0.5"));
    // La coupe n'est pas fermée : pas de « Z », quatre sommets, pas de segment sur la pliure.
    let cut = doc.with_role("coupe-calculee");
    assert_eq!(cut.len(), 1);
    assert!(!cut[0].d.as_deref().unwrap().contains('Z'));
    assert!(same_set(
        &cut[0].pts,
        &[(10.0, 130.0), (120.0, 130.0), (120.0, 10.0), (10.0, 10.0)],
        1e-6
    ));
    // Couture : trois lignes.
    assert_eq!(doc.with_role("couture").len(), 3);
    // Libellé : texte centré à 12,35 + 10 = 22,35, ligne de base à 110 − 48,775 + 10 = 71,225.
    let text = svg
        .lines()
        .map(str::trim)
        .find(|l| l.starts_with("<text data-role=\"libelle-pli\""))
        .expect("libellé");
    assert!(text.ends_with(">SUR LE PLI</text>"), "{text}");
    let x: f64 = attr(text, "x").unwrap().parse().unwrap();
    let y: f64 = attr(text, "y").unwrap().parse().unwrap();
    assert!(near(x, 22.35, 1e-3) && near(y, 71.225, 1e-3), "{x} {y}");
    assert_eq!(attr(text, "data-piece").as_deref(), Some("Demi-devant"));
}

#[test]
fn svg_en_anglais() {
    let f = fixture(Some(10.0), true);
    let svg = export_svg(&scene(&f, PieceExportMode::Both, Lang::En)).unwrap();
    assert!(svg.contains(">ON FOLD</text>") && !svg.contains("SUR LE PLI"));
    let text = svg
        .lines()
        .map(str::trim)
        .find(|l| l.starts_with("<text data-role=\"libelle-pli\""))
        .unwrap();
    // Centre à 9,095 mm de la pliure : x' = 19,095.
    let x: f64 = attr(text, "x").unwrap().parse().unwrap();
    assert!(near(x, 19.095, 1e-3), "{x}");
}

#[test]
fn pdf_pliure_trait_propre_libelle_lisible_et_coupe_ouverte() {
    let f = fixture(Some(10.0), true);
    let pdf = export_pdf(&[scene(&f, PieceExportMode::Both, Lang::Fr)]).unwrap();
    let strokes = read_pdf(&pdf.bytes);
    // Pointillé 20 / 3 / 1 / 3 mm, épaisseur 0,5 mm, en points.
    let dash: Vec<f64> = [20.0, 3.0, 1.0, 3.0].iter().map(|v| mm_pt(*v)).collect();
    let fold: Vec<_> = strokes
        .iter()
        .filter(|s| {
            s.dash_pt.len() == 4 && s.dash_pt.iter().zip(&dash).all(|(a, b)| near(*a, *b, 0.02))
        })
        .collect();
    assert_eq!(fold.len(), 1, "{strokes:?}");
    assert!(near(fold[0].width_pt, mm_pt(0.5), 0.02));
    let (a, b) = (fold[0].pts[0], fold[0].pts[1]);
    assert!(near(a.0, b.0, 1e-6), "pliure verticale");
    assert!(near((a.1 - b.1).abs(), mm_pt(120.0), 0.05), "longueur 120");
    // La coupe est une suite ouverte (pas de « h »), quatre sommets.
    let cut: Vec<_> = strokes
        .iter()
        .filter(|s| s.pts.len() == 4 && !s.closed && s.dash_pt.len() == 4 && s.width_pt > 1.0)
        .collect();
    assert_eq!(cut.len(), 1, "{strokes:?}");
    assert!(
        !strokes.iter().any(|s| s.closed && s.pts.len() == 4),
        "aucune coupe fermée"
    );
    // Libellé : texte lisible, abscisse de départ = pliure + 12,35 − demi-largeur réelle ; ligne de
    // base 1,225 mm sous le milieu de la pliure.
    let readable = common::pdf_text::readable(&pdf.bytes);
    let op = readable
        .lines()
        .find(|l| l.ends_with("(SUR LE PLI) Tj ET"))
        .expect("libellé");
    let t: Vec<&str> = op.split_whitespace().collect();
    let td = t.iter().position(|x| *x == "Td").unwrap();
    let size_pt: f64 = t[t.iter().position(|x| *x == "Tf").unwrap() - 1]
        .parse()
        .unwrap();
    assert!(near(size_pt, 3.5 * 72.0 / 25.4, 0.01), "corps {size_pt}");
    let (x, y): (f64, f64) = (t[td - 2].parse().unwrap(), t[td - 1].parse().unwrap());
    let half = common::pdf_text::repo_font().width_mm("SUR LE PLI", size_pt) / 2.0;
    let mid_y = (a.1 + b.1) / 2.0;
    assert!(
        near(x, a.0 + mm_pt(12.35 - half), 0.05),
        "x {x} vs pliure {} + {}",
        a.0,
        mm_pt(12.35 - half)
    );
    assert!(near(y, mid_y - mm_pt(1.225), 0.05), "y {y} vs {mid_y}");
}

#[test]
fn pdf_en_anglais() {
    let f = fixture(Some(10.0), true);
    let sc = scene(&f, PieceExportMode::Both, Lang::En);
    let pdf = export_pdf(&[sc]).unwrap();
    let readable = common::pdf_text::readable(&pdf.bytes);
    assert!(readable.contains("(ON FOLD) Tj ET"));
    assert!(!readable.contains("(SUR LE PLI)"));
}

#[test]
fn dxf_couche_91_ligne_et_texte_coupe_ouverte() {
    let f = fixture(Some(10.0), true);
    let out = export_dxf_with(&f.p, PieceExportMode::Both).unwrap();
    let dxf = rd::parse(&out.text).unwrap();
    let place = placed(&f.p, f.piece);
    let ents: Vec<&Ent> = dxf.blocks.iter().flat_map(|(_, e)| e).collect();
    // Ligne de pliure sur la couche 91 : (0 ; 110) → (0 ; −10).
    let lines: Vec<_> = ents
        .iter()
        .filter_map(|e| match e {
            Ent::Line { layer, a, b } if layer == "91" => Some((*a, *b)),
            _ => None,
        })
        .collect();
    assert_eq!(lines.len(), 1);
    let (ea, eb) = (
        place(pt(FOLD[0].0, FOLD[0].1)),
        place(pt(FOLD[1].0, FOLD[1].1)),
    );
    assert!(near(lines[0].0 .0, ea.0, 1e-4) && near(lines[0].0 .1, ea.1, 1e-4));
    assert!(near(lines[0].1 .0, eb.0, 1e-4) && near(lines[0].1 .1, eb.1, 1e-4));
    // Texte sur la même couche, en anglais (le DXF écrit tout son texte en anglais).
    let texts: Vec<_> = ents
        .iter()
        .filter_map(|e| match e {
            Ent::Text {
                layer, at, text, ..
            } if layer == "91" => Some((*at, text.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(texts.len(), 1);
    assert_eq!(texts[0].1, "ON FOLD");
    // Pièce non tournée au plan : écarts relatifs à la pliure. Largeur DXF : 7 × 4 × 0,6 = 16,8 ;
    // centre à 9,095 de la pliure → départ à 0,695 ; ligne de base 2 mm sous le milieu.
    let o = place(pt(0.0, 0.0));
    let ux = place(pt(1.0, 0.0));
    assert!(near(ux.0 - o.0, 1.0, 1e-9) && near(ux.1 - o.1, 0.0, 1e-9));
    assert!(near(texts[0].0 .0, ea.0 + 0.695, 1e-3), "{:?}", texts[0].0);
    assert!(near(texts[0].0 .1, (ea.1 + eb.1) / 2.0 - 2.0, 1e-3));
    // Coupe (couche 1) : suite ouverte à 4 sommets ; couture (couche 15) : trois bords, ouverte.
    let polys = |layer: &str| -> Vec<(Vec<(f64, f64)>, bool)> {
        ents.iter()
            .filter_map(|e| match e {
                Ent::Polyline {
                    layer: l,
                    pts,
                    closed,
                } if l == layer => Some((pts.clone(), *closed)),
                _ => None,
            })
            .collect()
    };
    let cut = polys("1");
    assert_eq!(cut.len(), 1);
    assert!(!cut[0].1);
    let want: Vec<(f64, f64)> = [(0.0, -10.0), (110.0, -10.0), (110.0, 110.0), (0.0, 110.0)]
        .iter()
        .map(|&(x, y)| place(pt(x, y)))
        .collect();
    assert!(same_set(&cut[0].0, &want, 1e-4), "{:?}", cut[0].0);
    let sewing = polys("15");
    assert_eq!(sewing.len(), 1);
    assert!(!sewing[0].1);
    let want: Vec<(f64, f64)> = [(0.0, 0.0), (100.0, 0.0), (100.0, 100.0), (0.0, 100.0)]
        .iter()
        .map(|&(x, y)| place(pt(x, y)))
        .collect();
    assert!(same_set(&sewing[0].0, &want, 1e-4), "{:?}", sewing[0].0);
    // Le rapport compte la couche 91 ; la couche est déclarée dans le fichier.
    assert!(out.report.layers.iter().any(|l| l.layer.number == 91));
}

#[test]
fn dxf_sans_pliure_la_couche_91_n_est_pas_declaree() {
    let f = fixture(Some(10.0), false);
    let out = export_dxf_with(&f.p, PieceExportMode::Both).unwrap();
    assert!(!out.report.layers.iter().any(|l| l.layer.number == 91));
    let dxf = rd::parse(&out.text).unwrap();
    assert!(!dxf.layers.iter().any(|(n, _)| n == "91"));
}

#[test]
fn hpgl_pliure_plume_8_libelle_plume_7_coupe_ouverte() {
    let f = fixture(Some(10.0), true);
    let text = export_hpgl(
        &scene(&f, PieceExportMode::Both, Lang::Fr),
        &PageSetup::default(),
    )
    .unwrap()
    .text;
    let r = read_hpgl(&text).unwrap();
    let on_pen =
        |n: u8| -> Vec<&common::hpgl::Stroke> { r.strokes.iter().filter(|s| s.pen == n).collect() };
    // Pliure : un trait de 120 mm sur la plume 8 (« autres »).
    let fold = on_pen(8);
    assert_eq!(fold.len(), 1);
    assert!(near(fold[0].length_mm(), 120.0, 0.05));
    // Coupe : plume 1, 340 mm (ouverte), jamais 460.
    let cut = on_pen(1);
    assert_eq!(cut.len(), 1);
    assert!(
        near(cut[0].length_mm(), 340.0, 0.1),
        "{}",
        cut[0].length_mm()
    );
    // Couture : trois traits de 100 mm.
    let sewing = on_pen(2);
    assert_eq!(sewing.len(), 3);
    assert!(sewing.iter().all(|s| near(s.length_mm(), 100.0, 0.05)));
    // Libellé ASCII sur la plume 7, à droite de la pliure (12,35 − 10,85 = 1,5 mm de la ligne).
    let label = r
        .labels
        .iter()
        .find(|l| l.text == "SUR LE PLI")
        .expect("libellé");
    assert_eq!(label.pen, 7);
    let fold_x = fold[0].pts[0].0 as f64 / 40.0;
    assert!(
        near(label.at.0 as f64 / 40.0 - fold_x, 1.5, 0.05),
        "{label:?}"
    );
    assert!(text.is_ascii());
}

#[test]
fn hpgl_en_anglais() {
    let f = fixture(Some(10.0), true);
    let text = export_hpgl(
        &scene(&f, PieceExportMode::Both, Lang::En),
        &PageSetup::default(),
    )
    .unwrap()
    .text;
    assert!(read_hpgl(&text)
        .unwrap()
        .labels
        .iter()
        .any(|l| l.text == "ON FOLD"));
}

#[test]
fn modes_couture_seule_et_coupe_seule() {
    let f = fixture(Some(10.0), true);
    // Couture seule : pas de coupe calculée ; la pliure est le bord lui-même, de (0 ; 100) à (0 ; 0).
    let sew = scene(&f, PieceExportMode::SewingOnly, Lang::Fr);
    assert!(sew.cut_contours.is_empty());
    assert_eq!(sew.folds.len(), 1);
    assert!(near(sew.folds[0].from.y, 100.0, 1e-9) && near(sew.folds[0].to.y, 0.0, 1e-9));
    assert_eq!(
        sew.paths
            .iter()
            .filter(|p| p.role == PathRole::SewingContour)
            .count(),
        3
    );
    // Coupe seule : plus aucun tracé de couture ; pliure prolongée jusqu'aux marges.
    let cut = scene(&f, PieceExportMode::CutOnly, Lang::Fr);
    assert!(cut.paths.iter().all(|p| p.role != PathRole::SewingContour));
    assert!(near(cut.folds[0].from.y, 110.0, 1e-9) && near(cut.folds[0].to.y, -10.0, 1e-9));
    assert_eq!(cut.cut_contours[0].edges.len(), 3);
}

#[test]
fn piece_sans_marge_le_bord_sur_le_pli_n_est_plus_un_trait_de_contour() {
    let f = fixture(None, true);
    let sc = scene(&f, PieceExportMode::Both, Lang::Fr);
    assert!(sc.cut_contours.is_empty());
    assert_eq!(sc.folds.len(), 1);
    assert_eq!(
        sc.paths
            .iter()
            .filter(|p| p.role == PathRole::SewingContour)
            .count(),
        3
    );
    // DXF : couture ouverte à quatre sommets, pliure sur la couche 91.
    let dxf = rd::parse(&export_dxf_with(&f.p, PieceExportMode::Both).unwrap().text).unwrap();
    let ents: Vec<&Ent> = dxf.blocks.iter().flat_map(|(_, e)| e).collect();
    assert!(ents
        .iter()
        .any(|e| matches!(e, Ent::Polyline { layer, closed, pts } if layer == "15" && !closed && pts.len() == 4)));
    assert!(ents
        .iter()
        .any(|e| matches!(e, Ent::Line { layer, .. } if layer == "91")));
}

#[test]
fn filtre_de_contenu_pliure_dans_les_lignes_speciales() {
    let f = fixture(Some(10.0), true);
    let sc = scene(&f, PieceExportMode::Both, Lang::Fr);
    assert_eq!(pf_export::content::count(&sc, C::SpecialLines), 1);
    // Exclure « pliure et lignes spéciales » retire la pliure et son libellé, rien d'autre.
    let without = OutputContent::default().without(C::SpecialLines);
    let filtered = without.apply(&sc).unwrap();
    assert!(filtered.folds.is_empty());
    assert_eq!(filtered.paths, sc.paths);
    assert_eq!(filtered.cut_contours, sc.cut_contours);
    let svg = export_svg(&filtered).unwrap();
    assert!(!svg.contains("sur-le-pli") && !svg.contains("SUR LE PLI"));
    // Les autres catégories laissent la pliure ; garder seulement la coupe : plus de pliure.
    for cat in C::ALL {
        let kept = OutputContent::default().without(cat).apply(&sc).unwrap();
        assert_eq!(
            kept.folds.len(),
            usize::from(cat != C::SpecialLines),
            "{cat:?}"
        );
    }
    let only_cut = OutputContent::only(&[C::CuttingContour])
        .apply(&sc)
        .unwrap();
    assert!(only_cut.folds.is_empty());
    // Résumé du contenu : un élément disponible dans cette catégorie.
    let summary = OutputContent::default().summary(&sc);
    let row = summary
        .counts
        .iter()
        .find(|c| c.category == C::SpecialLines)
        .unwrap();
    assert_eq!((row.available, row.kept), (1, 1));
}

#[test]
fn sans_declaration_les_octets_sont_ceux_de_la_pliure_posee_puis_retiree() {
    let base = fixture(Some(10.0), false);
    let mut toggled = fixture(Some(10.0), false);
    toggled
        .p
        .apply(Command::SetPieceFold {
            piece: toggled.piece,
            edge: toggled.left,
            on: true,
        })
        .unwrap();
    toggled
        .p
        .apply(Command::SetPieceFold {
            piece: toggled.piece,
            edge: toggled.left,
            on: false,
        })
        .unwrap();
    for mode in MODES {
        let (a, b) = (
            scene(&base, mode, Lang::Fr),
            scene(&toggled, mode, Lang::Fr),
        );
        assert_eq!(a, b, "{mode:?}");
        assert_eq!(export_svg(&a).unwrap(), export_svg(&b).unwrap());
        assert_eq!(
            export_pdf(std::slice::from_ref(&a)).unwrap().bytes,
            export_pdf(std::slice::from_ref(&b)).unwrap().bytes
        );
        assert_eq!(
            export_dxf_with(&base.p, mode).unwrap().text,
            export_dxf_with(&toggled.p, mode).unwrap().text
        );
        assert_eq!(
            export_hpgl(&a, &PageSetup::default()).unwrap().text,
            export_hpgl(&b, &PageSetup::default()).unwrap().text
        );
    }
    // Déclarée : chaque format change.
    let folded = fixture(Some(10.0), true);
    let (a, b) = (
        scene(&base, PieceExportMode::Both, Lang::Fr),
        scene(&folded, PieceExportMode::Both, Lang::Fr),
    );
    assert_ne!(export_svg(&a).unwrap(), export_svg(&b).unwrap());
    assert_ne!(
        export_pdf(std::slice::from_ref(&a)).unwrap().bytes,
        export_pdf(std::slice::from_ref(&b)).unwrap().bytes
    );
    assert_ne!(
        export_dxf_with(&base.p, PieceExportMode::Both)
            .unwrap()
            .text,
        export_dxf_with(&folded.p, PieceExportMode::Both)
            .unwrap()
            .text
    );
    assert_ne!(
        export_hpgl(&a, &PageSetup::default()).unwrap().text,
        export_hpgl(&b, &PageSetup::default()).unwrap().text
    );
}
