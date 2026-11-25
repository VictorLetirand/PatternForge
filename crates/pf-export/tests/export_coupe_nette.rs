//! US-05.6 — export d'une pièce « coupe nette » (contour de couture = contour de coupe, déclaré) :
//! SVG, PDF, DXF et HPGL. Rectangle 100 × 50 mm : périmètre 300 mm, aire 5 000 mm² (calculés à la
//! main). Règle : le contour est écrit **une seule fois**, comme coupe en « coupe seule » et
//! « les deux » (SVG / PDF : trait de coupe plein ; DXF : couche 1 ; HPGL : plume 1), comme couture
//! en « couture seule » (couche 15 / plume 2), quel que soit le rôle des tracés dessinés. Les
//! lecteurs (SVG, PDF, DXF, HPGL) sont ceux, indépendants des écrivains, de `common/`.
//!
//! Honnêteté : rien ici ne prouve l'ouverture dans un logiciel tiers ni un tracé réel.

mod common;

use common::coupe::*;
use common::dxf::{self as rd, Ent};
use common::dxf_models::rect_piece;
use common::hpgl::read_hpgl;
use pf_core::{Command, PathRole, PieceId, Project, SheetId};
use pf_export::dxf::{export_dxf_with, DxfIssueKind};
use pf_export::hpgl::export_hpgl;
use pf_export::pdf::export_pdf;
use pf_export::svg::export_svg;
use pf_export::{PageSetup, PieceExportMode};

/// Rectangle 100 × 50 dont les tracés ont le rôle `role`, déclaré en coupe nette.
fn net_rect(role: PathRole) -> (Project, SheetId, PieceId) {
    let mut p = Project::new("Coupe nette");
    let s = p.sheets[0].id;
    let (piece, _) = rect_piece(&mut p, s, (0.0, 0.0), (100.0, 50.0), "Dos", role, false);
    p.apply(Command::SetNetCut {
        piece,
        net_cut: true,
    })
    .unwrap();
    (p, s, piece)
}

const ROLES: [PathRole; 2] = [PathRole::SewingContour, PathRole::CuttingContour];

fn polys(dxf: &rd::Dxf, layer: &str) -> Vec<Vec<(f64, f64)>> {
    dxf.blocks
        .iter()
        .flat_map(|(_, ents)| ents)
        .filter_map(|e| match e {
            Ent::Polyline {
                layer: l,
                pts,
                closed,
                ..
            } if l == layer => {
                assert!(*closed);
                Some(pts.clone())
            }
            _ => None,
        })
        .collect()
}

fn seg_len(pts: &[(f64, f64)]) -> f64 {
    pts.windows(2)
        .map(|w| (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1))
        .sum()
}

#[test]
fn la_scene_ne_calcule_aucune_coupe_et_ne_dedouble_rien() {
    for role in ROLES {
        let (p, s, _) = net_rect(role);
        for mode in MODES {
            let scene = scene_with(&p, s, mode);
            assert!(scene.cut_contours.is_empty() && scene.cut_warnings.is_empty());
            assert_eq!(scene.paths.len(), 4, "quatre côtés, une seule fois");
            let want = if mode.includes_cut() {
                PathRole::CuttingContour
            } else {
                PathRole::SewingContour
            };
            assert!(
                scene.paths.iter().all(|q| q.role == want),
                "{mode:?} {role:?}"
            );
        }
    }
}

#[test]
fn svg_une_seule_fois_coupe_ou_couture() {
    for role in ROLES {
        let (p, s, _) = net_rect(role);
        let by_mode = |m| read_svg(&export_svg(&scene_with(&p, s, m)).unwrap());
        for mode in [PieceExportMode::Both, PieceExportMode::CutOnly] {
            let doc = by_mode(mode);
            assert!(doc.with_role("couture").is_empty(), "{mode:?}");
            assert!(doc.with_role("coupe-calculee").is_empty());
            let cut = doc.with_role("coupe");
            assert_eq!(cut.len(), 4, "{mode:?} {role:?}");
            let total: f64 = cut.iter().map(|i| seg_len(&i.pts)).sum();
            assert!((total - 300.0).abs() < 1e-9, "périmètre {total}");
            assert!(cut.iter().all(|i| i.dash.is_none()), "trait de coupe plein");
        }
        // Les deux modes qui écrivent la coupe sont identiques à l'octet.
        assert_eq!(
            export_svg(&scene_with(&p, s, PieceExportMode::Both)).unwrap(),
            export_svg(&scene_with(&p, s, PieceExportMode::CutOnly)).unwrap()
        );
        let doc = by_mode(PieceExportMode::SewingOnly);
        assert!(doc.with_role("coupe").is_empty());
        let sewing = doc.with_role("couture");
        assert_eq!(sewing.len(), 4);
        let total: f64 = sewing.iter().map(|i| seg_len(&i.pts)).sum();
        assert!((total - 300.0).abs() < 1e-9);
        assert_eq!(sewing[0].dash.as_deref(), Some("3 2"));
    }
}

/// Même rectangle, sans déclaration, tracés de rôle `role` : référence « écrit une seule fois ».
fn plain_rect(role: PathRole) -> (Project, SheetId) {
    let mut p = Project::new("Coupe nette");
    let s = p.sheets[0].id;
    rect_piece(&mut p, s, (0.0, 0.0), (100.0, 50.0), "Dos", role, false);
    (p, s)
}

#[test]
fn pdf_une_seule_fois_en_points_un_pour_un() {
    for role in ROLES {
        let (p, s, _) = net_rect(role);
        // Sans l'étiquette : la mention « Coupe nette » (PH-5c) a ses propres tests.
        let pdf_of = |p: &Project, s, m| {
            let mut sc = scene_with(p, s, m);
            sc.piece_labels.clear();
            export_pdf(&[sc]).unwrap().bytes
        };
        // « Coupe seule » et « les deux » : exactement le PDF d'un rectangle de coupe ordinaire
        // (quatre traits pleins, une fois) ; « couture seule » : celui d'un rectangle de couture.
        let (cut, cs) = plain_rect(PathRole::CuttingContour);
        let (sewing, ss) = plain_rect(PathRole::SewingContour);
        for mode in [PieceExportMode::Both, PieceExportMode::CutOnly] {
            assert_eq!(
                pdf_of(&p, s, mode),
                pdf_of(&cut, cs, PieceExportMode::Both),
                "{mode:?} {role:?}"
            );
            let strokes = read_pdf(&pdf_of(&p, s, mode));
            assert!(
                !strokes
                    .iter()
                    .any(|t| t.dash_pt.len() == 2 || t.dash_pt.len() == 4),
                "ni pointillé de couture ni motif de coupe calculée"
            );
        }
        assert_eq!(
            pdf_of(&p, s, PieceExportMode::SewingOnly),
            pdf_of(&sewing, ss, PieceExportMode::Both)
        );
        let strokes = read_pdf(&pdf_of(&p, s, PieceExportMode::SewingOnly));
        let dashed: Vec<f64> = strokes
            .iter()
            .filter(|t| !t.closed && t.pts.len() == 2 && t.dash_pt.len() == 2)
            .map(stroke_len_pt)
            .collect();
        assert_eq!(dashed.len(), 4);
        assert!((dashed.iter().sum::<f64>() - mm_pt(300.0)).abs() < 5e-3);
    }
}

#[test]
fn dxf_couche_1_une_seule_fois_et_couche_15_en_couture_seule() {
    for role in ROLES {
        let (p, _, _) = net_rect(role);
        for mode in [PieceExportMode::Both, PieceExportMode::CutOnly] {
            let out = export_dxf_with(&p, mode).unwrap();
            let dxf = rd::parse(&out.text).unwrap();
            let cut = polys(&dxf, "1");
            assert_eq!(cut.len(), 1, "{mode:?} {role:?}");
            assert!(polys(&dxf, "15").is_empty(), "pas de doublon en couture");
            assert!((rd::area(&cut[0]).abs() - 5000.0).abs() < 1e-6);
            assert!((rd::perimeter(&cut[0], true) - 300.0).abs() < 1e-6);
            assert_eq!(cut[0].len(), 4);
            // Déclaré : ni « contour de coupe absent », ni « tracé qui n'est pas une coupe ».
            assert!(
                !out.report.issues.iter().any(|i| matches!(
                    i.kind,
                    DxfIssueKind::NoCutContour | DxfIssueKind::BoundaryNotCut
                )),
                "{:?}",
                out.report.issues
            );
        }
        let out = export_dxf_with(&p, PieceExportMode::SewingOnly).unwrap();
        let dxf = rd::parse(&out.text).unwrap();
        assert!(polys(&dxf, "1").is_empty());
        let sewing = polys(&dxf, "15");
        assert_eq!(sewing.len(), 1);
        assert!((rd::area(&sewing[0]).abs() - 5000.0).abs() < 1e-6);
        assert!(out
            .report
            .issues
            .iter()
            .any(|i| i.kind == DxfIssueKind::NoCutContour));
    }
}

#[test]
fn hpgl_plume_1_une_seule_fois_et_plume_2_en_couture_seule() {
    for role in ROLES {
        let (p, s, _) = net_rect(role);
        let pens = |m| {
            let text = export_hpgl(&scene_with(&p, s, m), &PageSetup::default())
                .unwrap()
                .text;
            let r = read_hpgl(&text).unwrap();
            let len = |pen: u8| -> f64 {
                r.strokes
                    .iter()
                    .filter(|x| x.pen == pen)
                    .map(|x| x.length_mm())
                    .sum()
            };
            (len(1), len(2))
        };
        for mode in [PieceExportMode::Both, PieceExportMode::CutOnly] {
            let (cut, sewing) = pens(mode);
            assert!((cut - 300.0).abs() < 0.05, "{mode:?} {role:?} coupe {cut}");
            assert_eq!(sewing, 0.0, "pas de doublon en couture");
        }
        let (cut, sewing) = pens(PieceExportMode::SewingOnly);
        assert_eq!(cut, 0.0);
        assert!((sewing - 300.0).abs() < 0.05, "couture {sewing}");
    }
}

#[test]
fn une_piece_sans_declaration_exporte_comme_avant() {
    // Sans déclaration, ni marge : le contour reste tel que dessiné (rôle couture), quel que soit
    // le mode (comportement d'US-05.4 inchangé).
    let (p, s, _) = rect_project(None);
    for mode in MODES {
        let scene = scene_with(&p, s, mode);
        assert!(scene
            .paths
            .iter()
            .all(|q| q.role == PathRole::SewingContour));
    }
}
