//! US-12.4 - Import SVG et PDF vectoriel : un test par critere d'acceptation.
//!
//! Les SVG et PDF sont **ecrits a la main** (petits, un par fonctionnalite) ; les valeurs
//! attendues sont calculees a la main dans les commentaires (rectangle 20 x 10, translate puis
//! scale, quart de cercle de rayon 10...), jamais recopiees de la sortie du code teste.

use pf_core::{persist, Command, Document, EntityKind, ImportItem, Project, MAX_IMPORT_OBJECTS};
use pf_export::pdf_import::PdfDocument;
use pf_export::svg_import::{SvgDocument, SvgUnit, SvgUnitChoice};
use pf_export::vector_import::{
    IgnoreReason, NoteKind, VectorBuild, VectorImportError, MAX_FILE_BYTES,
};
use pf_geometry::PointMm;

const NS: &str = "xmlns=\"http://www.w3.org/2000/svg\"";

/// SVG de 100 x 50 mm, `viewBox` 0 0 100 50 : une unite utilisateur = 1 mm.
fn svg(body: &str) -> String {
    format!("<svg {NS} width=\"100mm\" height=\"50mm\" viewBox=\"0 0 100 50\">{body}</svg>")
}

fn build_svg(text: &str) -> VectorBuild {
    SvgDocument::parse(text.as_bytes())
        .unwrap()
        .build_choice(SvgUnitChoice::Declared, 1.0)
        .unwrap()
}

fn items(b: &VectorBuild) -> Vec<ImportItem> {
    b.objects.iter().map(|o| o.item.clone()).collect()
}

fn near(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

fn close(p: PointMm, x: f64, y: f64) -> bool {
    near(p.x, x, 1e-9) && near(p.y, y, 1e-9)
}

fn seg_len(i: &ImportItem) -> f64 {
    match i {
        ImportItem::Segment { from, to } => (to.x - from.x).hypot(to.y - from.y),
        other => panic!("pas un segment : {other:?}"),
    }
}

/// Longueur d'un objet importe, calculee ici (Bezier par echantillonnage, arc par son angle).
fn length(i: &ImportItem) -> f64 {
    match i {
        ImportItem::Segment { .. } => seg_len(i),
        ImportItem::Circle { radius_mm, .. } => std::f64::consts::TAU * radius_mm,
        ImportItem::Arc {
            center,
            start,
            end,
            ccw,
        } => {
            let r = (start.x - center.x).hypot(start.y - center.y);
            let (a0, a1) = (
                (start.y - center.y).atan2(start.x - center.x),
                (end.y - center.y).atan2(end.x - center.x),
            );
            let sweep = if *ccw {
                (a1 - a0).rem_euclid(std::f64::consts::TAU)
            } else {
                (a0 - a1).rem_euclid(std::f64::consts::TAU)
            };
            r * sweep
        }
        ImportItem::Bezier { start, c1, c2, end } => {
            let at = |t: f64| {
                let u = 1.0 - t;
                (
                    u * u * u * start.x
                        + 3.0 * u * u * t * c1.x
                        + 3.0 * u * t * t * c2.x
                        + t * t * t * end.x,
                    u * u * u * start.y
                        + 3.0 * u * u * t * c1.y
                        + 3.0 * u * t * t * c2.y
                        + t * t * t * end.y,
                )
            };
            let n = 4000;
            (0..n)
                .map(|k| {
                    let (a, b) = (at(k as f64 / n as f64), at((k + 1) as f64 / n as f64));
                    (b.0 - a.0).hypot(b.1 - a.1)
                })
                .sum()
        }
        other => panic!("non prevu : {other:?}"),
    }
}

fn total(b: &VectorBuild) -> f64 {
    b.objects.iter().map(|o| length(&o.item)).sum()
}

// ==================================================================== SVG : formes et unites

#[test]
fn rect_20_par_10_dans_un_viewbox_100_par_50_en_mm_donne_20_par_10_mm() {
    let b = build_svg(&svg("<rect x=\"10\" y=\"5\" width=\"20\" height=\"10\"/>"));
    assert_eq!(b.mm_per_unit, 1.0);
    let bb = b.bbox_mm.unwrap();
    assert!(near(bb.width(), 20.0, 1e-9) && near(bb.height(), 10.0, 1e-9));
    // SVG y = 5..15 sur 50 : modele y = 50 - 15 .. 50 - 5 = 35..45 (axe Y inverse).
    assert!(
        close(bb.min, 10.0, 35.0) && close(bb.max, 30.0, 45.0),
        "{bb:?}"
    );
    assert_eq!(b.report.objects, 4);
    assert_eq!(b.report.imported["rect"], 1);
    assert!(b
        .objects
        .iter()
        .all(|o| matches!(o.item, ImportItem::Segment { .. })));
    assert!(near(total(&b), 60.0, 1e-9));
}

#[test]
fn translate_10_5_puis_scale_2_sur_un_rect_5_par_2_5() {
    // x' = 10 + 2 x ; y' = 5 + 2 y : le rect (0 0 5 2,5) devient (10 5) - (20 10).
    let b = build_svg(&svg(
        "<g transform=\"translate(10 5) scale(2)\"><rect width=\"5\" height=\"2.5\"/></g>",
    ));
    let bb = b.bbox_mm.unwrap();
    assert!(near(bb.width(), 10.0, 1e-9) && near(bb.height(), 5.0, 1e-9));
    assert!(near(bb.min.x, 10.0, 1e-9) && near(bb.max.x, 20.0, 1e-9));
    // y SVG 5..10 -> modele 40..45.
    assert!(
        near(bb.min.y, 40.0, 1e-9) && near(bb.max.y, 45.0, 1e-9),
        "{bb:?}"
    );
}

#[test]
fn quart_de_cercle_de_rayon_10_donne_un_arc_du_modele() {
    // M 10 0 A 10 10 0 0 1 20 10 : centre (10 ; 10), de haut en droite, sens horaire a l'ecran.
    let b = build_svg(&svg("<path d=\"M 10 0 A 10 10 0 0 1 20 10\"/>"));
    let it = items(&b);
    assert_eq!(it.len(), 1);
    let ImportItem::Arc {
        center,
        start,
        end,
        ccw,
    } = it[0].clone()
    else {
        panic!("pas un arc : {it:?}")
    };
    // Axe Y inverse : le centre (10 ; 10) devient (10 ; 40), le depart (10 ; 0) -> (10 ; 50).
    assert!(close(center, 10.0, 40.0), "{center:?}");
    assert!(close(start, 10.0, 50.0) && close(end, 20.0, 40.0));
    assert!(
        !ccw,
        "haut vers droite autour du centre = horaire dans le modele"
    );
    assert!(near(
        length(&it[0]),
        10.0 * std::f64::consts::FRAC_PI_2,
        1e-9
    ));
    assert_eq!(b.report.note(NoteKind::Flattened), 0);
}

#[test]
fn cercle_et_symetrie_sont_des_cercles_et_arcs_exacts() {
    let b = build_svg(&svg(
        "<g transform=\"scale(2)\"><circle cx=\"5\" cy=\"5\" r=\"3\"/></g>",
    ));
    let ImportItem::Circle { center, radius_mm } = items(&b)[0].clone() else {
        panic!()
    };
    assert!(near(radius_mm, 6.0, 1e-9));
    assert!(close(center, 10.0, 40.0));
    // Symetrie horizontale (scale(-1 1)) : un arc horaire devient antihoraire.
    let b = build_svg(&svg(
        "<g transform=\"matrix(-1 0 0 1 50 0)\"><path d=\"M 10 0 A 10 10 0 0 1 20 10\"/></g>",
    ));
    let ImportItem::Arc { ccw, .. } = items(&b)[0].clone() else {
        panic!()
    };
    assert!(ccw);
}

#[test]
fn ellipse_et_cercle_deforme_sont_des_polylignes_a_tolerance_declaree() {
    // Ellipse rx 10, ry 5 : perimetre de Ramanujan = pi (3(a+b) - sqrt((3a+b)(a+3b))).
    let b = build_svg(&svg("<ellipse cx=\"50\" cy=\"25\" rx=\"10\" ry=\"5\"/>"));
    assert_eq!(b.tolerance_mm, 0.05);
    assert_eq!(b.report.note(NoteKind::Flattened), 1);
    let (a, c) = (10.0f64, 5.0f64);
    let perimetre = std::f64::consts::PI * (3.0 * (a + c) - ((3.0 * a + c) * (a + 3.0 * c)).sqrt());
    let l = total(&b);
    assert!(
        l <= perimetre + 1e-3 && perimetre - l < 0.1,
        "{l} vs {perimetre}"
    );
    // Ecart : le milieu de chaque corde est a moins de 0,05 mm de l'ellipse (echantillonnee).
    let dense: Vec<(f64, f64)> = (0..40_000)
        .map(|k| {
            let t = std::f64::consts::TAU * k as f64 / 40_000.0;
            (50.0 + 10.0 * t.cos(), 25.0 + 5.0 * t.sin())
        })
        .collect();
    for o in &b.objects {
        let ImportItem::Segment { from, to } = &o.item else {
            panic!()
        };
        let m = ((from.x + to.x) / 2.0, (from.y + to.y) / 2.0);
        let d = dense
            .iter()
            .map(|p| (p.0 - m.0).hypot(p.1 - m.1))
            .fold(f64::MAX, f64::min);
        assert!(d <= 0.05 + 1e-4, "{d}");
    }
    // Chaque sommet est sur l'ellipse (x - 50)^2 / 100 + (y - 25)^2 / 25 = 1.
    for o in &b.objects {
        let ImportItem::Segment { from, .. } = &o.item else {
            panic!()
        };
        let e = (from.x - 50.0).powi(2) / 100.0 + (from.y - 25.0).powi(2) / 25.0;
        assert!(near(e, 1.0, 1e-9), "{e}");
    }
    // Cercle r 5 sous scale(2 1) : meme ellipse (rx 10, ry 5).
    let b = build_svg(&svg(
        "<g transform=\"scale(2 1)\"><circle cx=\"25\" cy=\"25\" r=\"5\"/></g>",
    ));
    assert_eq!(b.report.note(NoteKind::Flattened), 1);
    assert!(total(&b) <= perimetre + 1e-3 && perimetre - total(&b) < 0.1);
}

#[test]
fn rectangle_a_coins_arrondis_perimetre() {
    // 20 x 10, rayon 2 : 2 (16 + 6) + 2 pi 2 = 44 + 4 pi.
    let b = build_svg(&svg(
        "<rect x=\"5\" y=\"5\" width=\"20\" height=\"10\" rx=\"2\"/>",
    ));
    let n_arcs = items(&b)
        .iter()
        .filter(|i| matches!(i, ImportItem::Arc { .. }))
        .count();
    assert_eq!(n_arcs, 4);
    assert!(near(total(&b), 44.0 + 4.0 * std::f64::consts::PI, 1e-9));
}

#[test]
fn chemin_relatif_absolu_h_v_z_et_implicites() {
    // m 10 10 h 20 v 10 h -20 z : rectangle 20 x 10 ferme (3 cotes + fermeture).
    let b = build_svg(&svg("<path d=\"m10 10h20v10h-20z\"/>"));
    assert_eq!(b.report.objects, 4);
    assert!(near(total(&b), 60.0, 1e-9));
    // Coordonnees implicites apres M : L, et nombres colles "1.5.5-2".
    let b = build_svg(&svg("<path d=\"M0 0 10 0 10 10\"/>"));
    assert_eq!(b.report.objects, 2);
    let b = build_svg(&svg("<path d=\"M0,0 L1.5.5-2 3\"/>"));
    let it = items(&b);
    let ImportItem::Segment { to, .. } = &it[0] else {
        panic!()
    };
    // 1.5 puis .5 puis -2 : 1er point (1,5 ; 0,5), 2e point (-2 ; 3).
    assert!(close(*to, 1.5, 50.0 - 0.5), "{to:?}");
    assert_eq!(it.len(), 2);
}

#[test]
fn bezier_cubique_s_et_quadratique_elevee() {
    // C 10 0 10 10 20 10 puis S 30 20 40 20 : 2e controle = 2*(20,10) - (10,10) = (30 ; 10).
    let b = build_svg(&svg("<path d=\"M 0 0 C 10 0 10 10 20 10 S 30 20 40 20\"/>"));
    let it = items(&b);
    assert_eq!(it.len(), 2);
    let ImportItem::Bezier { c1, c2, end, .. } = &it[1] else {
        panic!()
    };
    assert!(close(*c1, 30.0, 40.0) && close(*c2, 30.0, 30.0) && close(*end, 40.0, 30.0));
    // Q 30 0 30 30 depuis (0 ; 0) : c1 = (20 ; 0), c2 = (30 ; 10).
    let b = build_svg(&svg("<path d=\"M 0 0 Q 30 0 30 30\"/>"));
    let ImportItem::Bezier { c1, c2, .. } = &items(&b)[0] else {
        panic!()
    };
    assert!(close(*c1, 20.0, 50.0) && close(*c2, 30.0, 40.0));
    assert_eq!(b.report.note(NoteKind::QuadraticRaised), 1);
}

#[test]
fn unites_fixes_et_declarees() {
    let line = |s: &str| format!("<svg {NS}{s}><line x2=\"1\"/></svg>");
    let len = |text: &str, c: SvgUnitChoice| {
        let b = SvgDocument::parse(text.as_bytes())
            .unwrap()
            .build_choice(c, 1.0)
            .unwrap();
        total(&b)
    };
    let plain = line("");
    // 1 unite = 1 pouce, 1 cm, 1 pt, 1 px.
    assert!(near(
        len(&plain, SvgUnitChoice::Fixed(SvgUnit::Inch)),
        25.4,
        1e-9
    ));
    assert!(near(
        len(&plain, SvgUnitChoice::Fixed(SvgUnit::Centimetre)),
        10.0,
        1e-9
    ));
    assert!(near(
        len(&plain, SvgUnitChoice::Fixed(SvgUnit::Point)),
        25.4 / 72.0,
        1e-9
    ));
    assert!(near(
        len(&plain, SvgUnitChoice::Fixed(SvgUnit::Pixel)),
        25.4 / 96.0,
        1e-9
    ));
    // Sans unite ni viewBox : le pixel a 96 dpi.
    assert!(near(
        len(&plain, SvgUnitChoice::Declared),
        25.4 / 96.0,
        1e-9
    ));
    // viewBox 0 0 200 100 sur 100 mm x 50 mm : 0,5 mm par unite.
    let declared = line(" width=\"100mm\" height=\"50mm\" viewBox=\"0 0 200 100\"");
    assert!(near(len(&declared, SvgUnitChoice::Declared), 0.5, 1e-9));
    // Largeur en cm et en pouces : 10 cm / 100 = 1 mm ; 2 in / 50.8 = 1 mm.
    let cm = line(" width=\"10cm\" viewBox=\"0 0 100 100\"");
    assert!(near(len(&cm, SvgUnitChoice::Declared), 1.0, 1e-9));
    let inch = line(" width=\"2in\" viewBox=\"0 0 50.8 50.8\"");
    assert!(near(len(&inch, SvgUnitChoice::Declared), 1.0, 1e-9));
    // Echelle utilisateur en plus.
    let b = SvgDocument::parse(declared.as_bytes())
        .unwrap()
        .build_choice(SvgUnitChoice::Declared, 2.0)
        .unwrap();
    assert!(near(total(&b), 1.0, 1e-9));
    // Unites sur les attributs d'une forme : 1 cm = 96 / 2,54 px ; au pixel par defaut = 10 mm.
    let b = SvgDocument::parse(format!("<svg {NS}><line x2=\"1cm\"/></svg>").as_bytes())
        .unwrap()
        .build_choice(SvgUnitChoice::Declared, 1.0)
        .unwrap();
    assert!(near(total(&b), 10.0, 1e-9));
}

#[test]
fn transformations_rotate_skew_matrix() {
    // rotate(90) : le segment (0 ; 0)-(10 ; 0) devient (0 ; 0)-(0 ; 10) dans le repere du fichier.
    let b = build_svg(&svg("<g transform=\"rotate(90)\"><line x2=\"10\"/></g>"));
    let ImportItem::Segment { to, .. } = &items(&b)[0] else {
        panic!()
    };
    // x' = 0, y' = 10 (SVG) -> modele y = 50 - 10 = 40.
    assert!(close(*to, 0.0, 40.0), "{to:?}");
    // skewX(45) : (0 ; 10) -> (10 ; 10).
    let b = build_svg(&svg("<g transform=\"skewX(45)\"><line y2=\"10\"/></g>"));
    let ImportItem::Segment { to, .. } = &items(&b)[0] else {
        panic!()
    };
    assert!(close(*to, 10.0, 40.0), "{to:?}");
    // matrix(2 0 0 2 1 1) imbrique dans translate(3 0) : x' = 3 + 1 + 2 x.
    let b = build_svg(&svg(
        "<g transform=\"translate(3 0)\"><g transform=\"matrix(2 0 0 2 1 1)\"><line x2=\"5\"/></g></g>",
    ));
    let ImportItem::Segment { from, to } = &items(&b)[0] else {
        panic!()
    };
    assert!(close(*from, 4.0, 49.0) && close(*to, 14.0, 49.0));
}

#[test]
fn polyligne_polygone_ligne() {
    let b = build_svg(&svg(
        "<polyline points=\"0,0 10,0 10,10\"/><polygon points=\"0 0 4 0 4 3\"/><line x1=\"0\" y1=\"0\" x2=\"3\" y2=\"4\"/>",
    ));
    // 2 + 3 (polygone ferme : 4 + 3 + 5) + 1 segments.
    assert_eq!(b.report.objects, 6);
    assert!(near(total(&b), 20.0 + 12.0 + 5.0, 1e-9));
}

// ============================================================== SVG : ignore, refus, bornes

#[test]
fn ignores_listes_jamais_perdus_en_silence() {
    let b = build_svg(&svg(
        "<defs><linearGradient id=\"g\"/></defs><text x=\"1\" y=\"2\">abc</text><image href=\"x\"/>\
         <use href=\"#a\"/><clipPath id=\"c\"><rect width=\"1\" height=\"1\"/></clipPath>\
         <rect style=\"fill:red\" width=\"2\" height=\"2\" clip-path=\"url(#c)\"/>\
         <g><title>t</title><line x2=\"4\"/></g>",
    ));
    assert_eq!(b.report.imported_total(), 2);
    for kind in ["defs", "text", "image", "use", "clipPath"] {
        assert_eq!(b.report.ignored_of(kind), 1, "{kind}");
    }
    assert_eq!(b.report.note(NoteKind::StyleIgnored), 1);
    assert_eq!(b.report.note(NoteKind::ClipIgnored), 1);
    // Ce qui est lu + ce qui est ignore = tout ce qui a ete rencontre.
    assert_eq!(
        b.report.read,
        b.report.imported_total() + b.report.ignored_total()
    );
}

#[test]
fn valeurs_illisibles_comptees_et_refus_xml() {
    for bad in [
        "<path d=\"M NaN 0 L 1 1\"/>",
        "<path d=\"M 0 0 L 1\"/>",
        "<path d=\"L 0 0\"/>",
        "<path d=\"M 0 0 X 1 1\"/>",
        "<rect width=\"abc\" height=\"2\"/>",
        "<g transform=\"rotate(\"><line x2=\"1\"/></g>",
        "<circle r=\"1e999\"/>",
    ] {
        let b = build_svg(&svg(bad));
        assert_eq!(b.report.objects, 0, "{bad}");
        assert_eq!(b.report.ignored_total(), 1, "{bad}");
        assert_eq!(b.report.ignored[0].reason, IgnoreReason::Invalid, "{bad}");
    }
    for bad in [
        "<svg xmlns=\"x\"><g><line x2=\"1\"/></svg>",
        "<svg><line x2=1/></svg>",
        "<svg><line x2=\"1\"></svg>",
        "<svg><line x2=\"1\"/>",
        "<svg><!-- jamais ferme </svg>",
        "<svg><g></h></svg>",
        "",
    ] {
        assert!(SvgDocument::parse(bad.as_bytes()).is_err(), "{bad}");
    }
    assert_eq!(
        SvgDocument::parse(b"<html><body/></html>").unwrap_err(),
        VectorImportError::NotSvg
    );
}

#[test]
fn degeneres_et_hors_espace_de_travail() {
    let b = build_svg(&svg(
        "<g transform=\"scale(0)\"><line x2=\"5\"/></g><rect width=\"0\" height=\"3\"/>\
         <g transform=\"matrix(1 2 2 4 0 0)\"><line x2=\"5\"/></g>",
    ));
    assert_eq!(b.report.objects, 0);
    assert_eq!(b.report.ignored_total(), 3);
    assert!(b
        .report
        .ignored
        .iter()
        .all(|i| i.reason == IgnoreReason::Degenerate));
    let b = build_svg(&svg("<line x2=\"1e9\"/><line x2=\"5\"/>"));
    assert_eq!(b.report.objects, 1);
    assert_eq!(b.report.ignored[0].reason, IgnoreReason::OutOfRange);
}

#[test]
fn bornes_objets_fichier_et_echelle() {
    // 200 002 sommets = 200 001 segments : un de plus que la limite.
    let mut pts = String::new();
    for i in 0..=(MAX_IMPORT_OBJECTS + 1) {
        pts.push_str(&format!("{},{} ", i % 90, (i / 90) % 40));
    }
    let doc = SvgDocument::parse(svg(&format!("<polyline points=\"{pts}\"/>")).as_bytes()).unwrap();
    assert!(matches!(
        doc.build_choice(SvgUnitChoice::Declared, 1.0),
        Err(VectorImportError::TooManyObjects { count }) if count == MAX_IMPORT_OBJECTS + 1
    ));
    let big = vec![b' '; MAX_FILE_BYTES + 1];
    assert!(matches!(
        SvgDocument::parse(&big),
        Err(VectorImportError::TooLarge { .. })
    ));
    assert!(matches!(
        PdfDocument::parse(&big),
        Err(VectorImportError::TooLarge { .. })
    ));
    let doc = SvgDocument::parse(svg("<line x2=\"1\"/>").as_bytes()).unwrap();
    for s in [0.0, -1.0, f64::NAN, f64::INFINITY, 1e9, 1e-9] {
        assert_eq!(
            doc.build_choice(SvgUnitChoice::Declared, s).unwrap_err(),
            VectorImportError::BadScale,
            "{s}"
        );
    }
}

#[test]
fn demi_cercle_arrondi_a_4_decimales_reste_un_demi_cercle() {
    // Rayon 10, extremites arrondies : corde 19,9999 -> demi-cercle (pi 10 = 31,4159...).
    let b = build_svg(&svg("<path d=\"M 10 25 A 10 10 0 0 1 29.9999 25\"/>"));
    assert!(
        near(total(&b), std::f64::consts::PI * 9.99995, 1e-6),
        "{}",
        total(&b)
    );
    assert_eq!(b.report.note(NoteKind::SemicircleSnapped), 1);
}

// ============================================================ une seule etape annulable

#[test]
fn une_commande_une_etape_et_nouvelle_feuille() {
    let b = build_svg(&svg(
        "<rect x=\"1\" y=\"1\" width=\"20\" height=\"10\"/><path d=\"M 0 0 C 5 0 5 5 10 5\"/>",
    ));
    let mut doc = Document::new(Project::new("t"));
    let sheet = doc.project().sheets[0].id;
    doc.execute(Command::ImportGeometry {
        sheet,
        objects: b.objects.clone(),
    })
    .unwrap();
    let s = doc.project().sheet(sheet).unwrap();
    assert!(s
        .entities
        .iter()
        .any(|e| matches!(e.kind, EntityKind::Curve { .. })));
    assert_eq!(s.entities.len(), 5);
    doc.undo();
    assert!(doc.project().sheet(sheet).unwrap().entities.is_empty());
    // Nouvelle feuille : une seule etape pour creer la feuille et importer.
    let before = persist::to_json(doc.project()).unwrap();
    doc.execute(Command::ImportGeometryNewSheet {
        name: "Import".into(),
        objects: b.objects.clone(),
    })
    .unwrap();
    assert_eq!(doc.project().sheets.len(), 2);
    assert_eq!(doc.project().sheets[1].entities.len(), 5);
    doc.undo();
    assert_eq!(persist::to_json(doc.project()).unwrap(), before);
    // Refus sans trace : nom vide.
    assert!(doc
        .execute(Command::ImportGeometryNewSheet {
            name: "  ".into(),
            objects: b.objects,
        })
        .is_err());
    assert_eq!(persist::to_json(doc.project()).unwrap(), before);
}
