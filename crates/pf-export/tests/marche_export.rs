//! US-08.2 — les crans reportés par la marche de pièces sont de vrais crans : ils sont écrits dans
//! le SVG et le DXF (lecteurs indépendants de `common/`). Pièce A 200 × 50 en (0 ; 0), pièce B
//! 195 × 50 en (300 ; 0) (embu 5), couple A / B ; crans de A à 50, 100 et 150 mm du coin (0 ; 0),
//! à gauche du sens du contour (anti-horaire : vers l'intérieur, +y), longueur 5 mm.
//! Valeurs attendues **calculées à la main** : abscisse reportée = 50, 100, 150 × 195 / 200 =
//! 48,75 / 97,5 / 146,25 depuis le début de B ; sens de couture opposé (deux contours
//! anti-horaires) : x = 300 + 195 − 48,75 = 446,25 ; 300 + 195 − 97,5 = 397,5 ; 300 + 195 −
//! 146,25 = 348,75, pointes en y = 5. Rien ici ne prouve l'ouverture dans un logiciel tiers.

mod common;

use common::coupe::{attr, same_set};
use common::dxf::{self as rd, Ent};
use common::dxf_check::placed;
use common::dxf_models::rect_piece;
use pf_core::{Command, NotchStyle, PathRole, PieceId, Project, SeamWhich, SheetId};
use pf_export::dxf::export_dxf_with;
use pf_export::svg::export_svg;
use pf_export::{PieceExportMode, Scene, SceneOptions};
use pf_geometry::{PointMm, Side};

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

fn project(march: bool) -> (Project, SheetId, PieceId, PieceId) {
    let mut p = Project::new("Marche export");
    let s = p.sheets[0].id;
    let (a, ea) = rect_piece(
        &mut p,
        s,
        (0.0, 0.0),
        (200.0, 50.0),
        "A",
        PathRole::SewingContour,
        false,
    );
    let (b, eb) = rect_piece(
        &mut p,
        s,
        (300.0, 0.0),
        (195.0, 50.0),
        "B",
        PathRole::SewingContour,
        false,
    );
    let sh = p.sheet(s).unwrap();
    let bottom = |edges: &[pf_core::EntityId], x0: f64, x1: f64| {
        *edges
            .iter()
            .find(|e| {
                let (u, v) = sh.segment_ends(**e).unwrap();
                u.y == 0.0 && v.y == 0.0 && u.x.min(v.x) == x0 && u.x.max(v.x) == x1
            })
            .unwrap()
    };
    let (ea_bottom, eb_bottom) = (bottom(&ea, 0.0, 200.0), bottom(&eb, 300.0, 495.0));
    let (ua, _) = sh.segment_ends(ea_bottom).unwrap();
    let from_start = ua.x == 0.0;
    for d in [50.0, 100.0, 150.0] {
        p.apply(Command::AddNotch {
            sheet: s,
            entity: ea_bottom,
            // Le côté se lit dans le sens du tracé : « gauche du contour » = gauche si le tracé
            // va de x = 0 vers x = 200, droite sinon.
            distance_mm: if from_start { d } else { 200.0 - d },
            side: if from_start { Side::Left } else { Side::Right },
            length_mm: 5.0,
            style: NotchStyle::Single,
        })
        .unwrap();
    }
    let sa = p.seam_side_range(a, ea_bottom, 1).unwrap();
    let sb = p.seam_side_range(b, eb_bottom, 1).unwrap();
    let pair = p
        .apply(Command::AddSeamPair {
            a: sa,
            b: sb,
            embu_mm: Some(5.0),
            tolerance_mm: None,
        })
        .unwrap()
        .seam_pairs[0];
    if march {
        let o = p
            .apply(Command::MarchNotches {
                pair,
                from: SeamWhich::A,
                which: None,
            })
            .unwrap();
        assert_eq!(o.notches.len(), 3);
    }
    (p, s, a, b)
}

/// Traits du groupe des crans du SVG : ((x1, y1), (x2, y2)).
fn svg_notch_lines(svg: &str) -> Vec<((f64, f64), (f64, f64))> {
    let (mut inside, mut out) = (false, Vec::new());
    for raw in svg.lines() {
        let l = raw.trim();
        if l.starts_with("<g ") {
            inside = l.contains("data-role=\"cran\"");
        } else if l.starts_with("</g>") {
            inside = false;
        } else if inside && l.starts_with("<line") {
            let n = |k: &str| attr(l, k).unwrap().parse::<f64>().unwrap();
            out.push(((n("x1"), n("y1")), (n("x2"), n("y2"))));
        }
    }
    out
}

#[test]
fn svg_les_crans_reportes_sont_ecrits_aux_positions_calculees_a_la_main() {
    let (p0, s, _, _) = project(false);
    let scene = |p: &Project| Scene::from_sheet_with(p, s, SceneOptions::default()).unwrap();
    let before = svg_notch_lines(&export_svg(&scene(&p0)).unwrap());
    assert_eq!(before.len(), 3, "crans de A seuls : {before:?}");
    let (p, s, _, _) = project(true);
    let after = svg_notch_lines(&export_svg(&scene(&p)).unwrap());
    assert_eq!(after.len(), 6, "{after:?}");
    // Emprise 495 × 50, marge blanche de 10 : x' = x + 10, y' = 60 - y.
    // A : pieds en (60 ; 60) (110 ; 60) (160 ; 60), pointes à y' = 55.
    // B : pieds en x = 446,25 / 397,5 / 348,75 → x' = 456,25 / 407,5 / 358,75 ; pointes à y' = 55.
    let ends: Vec<(f64, f64)> = after.iter().flat_map(|m| [m.0, m.1]).collect();
    let want: Vec<(f64, f64)> = [60.0, 110.0, 160.0, 456.25, 407.5, 358.75]
        .iter()
        .flat_map(|x| [(*x, 60.0), (*x, 55.0)])
        .collect();
    assert!(same_set(&ends, &want, 1e-3), "{ends:?}");
    let _ = s;
}

#[test]
fn dxf_les_crans_reportes_sont_sur_la_couche_des_crans() {
    let (p, _, a, b) = project(true);
    let out = export_dxf_with(&p, PieceExportMode::Both).unwrap();
    let dxf = rd::parse(&out.text).unwrap();
    let marks: Vec<((f64, f64), (f64, f64))> = dxf
        .blocks
        .iter()
        .flat_map(|(_, e)| e)
        .filter_map(|e| match e {
            Ent::Line { layer, a, b } if layer == "4" => Some((*a, *b)),
            _ => None,
        })
        .collect();
    assert_eq!(marks.len(), 6, "{marks:?}");
    let got: Vec<(f64, f64)> = marks.iter().flat_map(|m| [m.0, m.1]).collect();
    let (pa, pb) = (placed(&p, a), placed(&p, b));
    let mut want: Vec<(f64, f64)> = Vec::new();
    for x in [50.0, 100.0, 150.0] {
        want.extend([pa(pt(x, 0.0)), pa(pt(x, 5.0))]);
    }
    for x in [446.25, 397.5, 348.75] {
        want.extend([pb(pt(x, 0.0)), pb(pt(x, 5.0))]);
    }
    assert!(same_set(&got, &want, 1e-4), "{got:?} / {want:?}");
    assert_eq!(out.report.omissions().count(), 0);
}
