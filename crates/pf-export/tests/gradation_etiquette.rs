//! US-09.5a — l'étiquette de pièce et le contour de coupe d'une scène lue **dans une taille**
//! (projet évalué `graded_project`, lecture seule : l'export gradé est US-09.8). Rectangle
//! 400 × 600, écarts `S (−5 ; −10)`, `L (+5 ; +10)`, `XL (+10 ; +22)`, `XS (−9 ; −20)` : A fixe,
//! B `(dx ; 0)`, C `(dx ; dy)`, D `(0 ; dy)`. Valeurs calculées à la main.

use pf_core::*;
use pf_export::{PathGeom, Scene};
use pf_geometry::{distance_mm, PointMm};

const DELTAS: [(&str, f64, f64); 5] = [
    ("XS", -9.0, -20.0),
    ("S", -5.0, -10.0),
    ("M", 0.0, 0.0),
    ("L", 5.0, 10.0),
    ("XL", 10.0, 22.0),
];

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

fn size(p: &Project, label: &str) -> SizeId {
    p.sizes[0]
        .sizes
        .iter()
        .find(|s| s.label == label)
        .unwrap()
        .id
}

fn point_at(p: &Project, sheet: SheetId, at: PointMm) -> PointId {
    let s = p.sheet(sheet).unwrap();
    s.points
        .iter()
        .find(|q| distance_mm(s.position(q.id).unwrap(), at) < 1e-9)
        .unwrap()
        .id
}

fn fixture(with_grain: bool) -> (Project, SheetId, PieceId) {
    let mut p = Project::new("Étiquette gradée");
    let sheet = p.sheets[0].id;
    p.apply(Command::AddSizeRange {
        name: "Femme".into(),
        kind: SizeKind::Alphanumeric,
        labels: ["XS", "S", "M", "L", "XL"]
            .iter()
            .map(|s| s.to_string())
            .collect(),
        base: 2,
    })
    .unwrap();
    let corners = [
        pt(0.0, 0.0),
        pt(400.0, 0.0),
        pt(400.0, 600.0),
        pt(0.0, 600.0),
    ];
    let ids: Vec<EntityId> = (0..4)
        .map(|i| {
            p.apply(Command::AddSegment {
                sheet,
                from: PointRef::New(corners[i]),
                to: PointRef::New(corners[(i + 1) % 4]),
            })
            .unwrap()
            .entities[0]
        })
        .collect();
    let piece = p
        .apply(Command::ExtractPiece {
            sheet,
            source: PieceSource::Paths { entities: ids },
            name: "Devant".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    p.apply(Command::SetPieceCard {
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
    let (b, c, d) = (
        point_at(&p, sheet, corners[1]),
        point_at(&p, sheet, corners[2]),
        point_at(&p, sheet, corners[3]),
    );
    let grade = |p: &mut Project, point: PointId, label: &str, dx: f64, dy: f64| {
        let size = size(p, label);
        p.apply(Command::SetPointGrading {
            sheet,
            point,
            size,
            dx_mm: dx,
            dy_mm: dy,
        })
        .unwrap();
    };
    for (label, dx, dy) in DELTAS.into_iter().filter(|d| d.0 != "M") {
        grade(&mut p, b, label, dx, 0.0);
        grade(&mut p, c, label, dx, dy);
        grade(&mut p, d, label, 0.0, dy);
    }
    if with_grain {
        let g = p
            .apply(Command::AddSegment {
                sheet,
                from: PointRef::New(pt(200.0, 50.0)),
                to: PointRef::New(pt(200.0, 550.0)),
            })
            .unwrap()
            .entities[0];
        p.apply(Command::SetPathRole {
            sheet,
            entity: g,
            role: PathRole::Grainline,
        })
        .unwrap();
        let (lo, hi) = (
            point_at(&p, sheet, pt(200.0, 50.0)),
            point_at(&p, sheet, pt(200.0, 550.0)),
        );
        for (label, dx, dy) in DELTAS.into_iter().filter(|d| d.0 != "M") {
            grade(&mut p, lo, label, dx / 2.0, 0.0);
            grade(&mut p, hi, label, dx / 2.0, dy);
        }
        let outline = p.piece(piece).unwrap().outline.clone();
        p.apply(Command::SetPieceOutline {
            piece,
            outline,
            grainline: Some(g),
        })
        .unwrap();
    }
    (p, sheet, piece)
}

fn scene_in(p: &Project, sheet: SheetId, label: &str) -> Scene {
    let g = p.graded_project(Some(size(p, label))).unwrap();
    Scene::from_sheet(&g, sheet).unwrap()
}

#[test]
fn l_etiquette_sans_droit_fil_est_au_centre_de_l_emprise_de_chaque_taille() {
    let (p, sheet, _) = fixture(false);
    for (label, dx, dy) in DELTAS {
        let scene = scene_in(&p, sheet, label);
        assert_eq!(scene.piece_labels.len(), 1, "{label}");
        let l = &scene.piece_labels[0];
        // Centre de l'emprise du contour : (w / 2 ; h / 2) avec w = 400 + dx, h = 600 + dy.
        let want = pt((400.0 + dx) / 2.0, (600.0 + dy) / 2.0);
        assert!(
            distance_mm(l.center, want) < 1e-6,
            "{label} : {:?}",
            l.center
        );
        // Le texte lui-même ne dépend pas de la taille.
        assert_eq!(l.lines, ["Devant", "Code : D-1", "Quantité : 1"], "{label}");
    }
}

#[test]
fn l_etiquette_avec_droit_fil_longe_le_droit_fil_de_la_taille() {
    let (p, sheet, _) = fixture(true);
    for (label, dx, dy) in DELTAS {
        let scene = scene_in(&p, sheet, label);
        let l = &scene.piece_labels[0];
        // Droit-fil vertical x = 200 + dx / 2, de y = 50 à 550 + dy : milieu y = 300 + dy / 2 ;
        // texte à gauche du sens (vers −x), de la demi-largeur du bloc + 3 mm.
        let off = l.width_mm() / 2.0 + 3.0;
        let want = pt(200.0 + dx / 2.0 - off, 300.0 + dy / 2.0);
        assert!(
            distance_mm(l.center, want) < 1e-6,
            "{label} : {:?}",
            l.center
        );
    }
}

#[test]
fn le_contour_de_coupe_de_la_scene_suit_la_taille_avec_la_meme_marge() {
    let (mut p, sheet, piece) = fixture(false);
    p.apply(Command::SetSeamAllowance {
        piece,
        seam: Some(SeamAllowance {
            default_mm: 10.0,
            ..Default::default()
        }),
    })
    .unwrap();
    for (label, dx, dy) in DELTAS {
        let scene = scene_in(&p, sheet, label);
        assert_eq!(scene.cut_contours.len(), 1, "{label}");
        let c = &scene.cut_contours[0];
        let pts: Vec<PointMm> = c
            .edges
            .iter()
            .flat_map(|e| match e {
                PathGeom::Line(a, b) => [*a, *b],
                other => panic!("segment attendu : {other:?}"),
            })
            .collect();
        let xs = pts.iter().map(|q| q.x);
        let ys = pts.iter().map(|q| q.y);
        let (x0, x1) = (
            xs.clone().fold(f64::MAX, f64::min),
            xs.fold(f64::MIN, f64::max),
        );
        let (y0, y1) = (
            ys.clone().fold(f64::MAX, f64::min),
            ys.fold(f64::MIN, f64::max),
        );
        // Emprise de la coupe : (−10 ; −10) à (w + 10 ; h + 10).
        assert!(
            (x0 + 10.0).abs() < 1e-6 && (y0 + 10.0).abs() < 1e-6,
            "{label}"
        );
        assert!((x1 - (410.0 + dx)).abs() < 1e-6, "{label} : {x1}");
        assert!((y1 - (610.0 + dy)).abs() < 1e-6, "{label} : {y1}");
    }
}
