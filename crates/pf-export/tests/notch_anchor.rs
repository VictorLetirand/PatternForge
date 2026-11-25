//! US-06.2 — un cran ancré est exporté à sa **position effective** (calculée à la lecture),
//! exactement comme un cran à distance fixe posé au même endroit : SVG et PDF identiques à
//! l'octet. Les crans à distance fixe, eux, ne changent pas (fichiers de `examples/`).

use pf_core::{Command, EntityId, NotchAnchor, NotchStyle, PointId, PointRef, Project, SheetId};
use pf_export::pdf::export_pdf;
use pf_export::svg::export_svg;
use pf_export::Scene;
use pf_geometry::{distance_mm, PointMm, Side};

/// Fabrique d'ancrage d'un cas de test (le point libre n'est utile qu'à l'un d'eux).
type Make = fn(PointId) -> NotchAnchor;

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

/// Rectangle 120 × 60 mm ; renvoie le projet, la feuille, le bord du bas et un point libre.
fn rectangle() -> (Project, SheetId, EntityId, PointId) {
    let mut p = Project::new("Crans ancrés export");
    let s = p.sheets[0].id;
    let o = p
        .apply(Command::AddRectangle {
            sheet: s,
            corner: PointRef::New(pt(0.0, 0.0)),
            width_mm: 120.0,
            height_mm: 60.0,
        })
        .unwrap();
    let q = p
        .apply(Command::AddPoint {
            sheet: s,
            at: pt(35.0, 25.0),
        })
        .unwrap()
        .points[0];
    (p, s, o.entities[0], q)
}

fn fixed(p: &mut Project, s: SheetId, e: EntityId, distance_mm: f64) {
    p.apply(Command::AddNotch {
        sheet: s,
        entity: e,
        distance_mm,
        side: Side::Left,
        length_mm: 5.0,
        style: NotchStyle::Double,
    })
    .unwrap();
}

fn anchored(p: &mut Project, s: SheetId, e: EntityId, anchor: NotchAnchor) {
    p.apply(Command::AddAnchoredNotch {
        sheet: s,
        entity: e,
        anchor,
        side: Side::Left,
        length_mm: 5.0,
        style: NotchStyle::Double,
    })
    .unwrap();
}

fn exports(p: &Project, s: SheetId) -> (Vec<(PointMm, PointMm)>, String, Vec<u8>) {
    let scene = Scene::from_sheet(p, s).unwrap();
    let svg = export_svg(&scene).unwrap();
    let pdf = export_pdf(std::slice::from_ref(&scene)).unwrap().bytes;
    (scene.notch_marks, svg, pdf)
}

#[test]
fn un_cran_ancre_s_exporte_comme_un_cran_fixe_a_la_meme_position() {
    // Les trois ancrages donnent les mêmes octets que le cran fixe équivalent.
    let cases: [(Make, f64); 3] = [
        (|_| NotchAnchor::Midpoint {}, 60.0),
        (|_| NotchAnchor::Ratio { ratio: 0.25 }, 30.0),
        (|point| NotchAnchor::Point { point }, 35.0),
    ];
    for (make, at) in cases {
        let (mut a, sa, ea, qa) = rectangle();
        let (mut b, sb, eb, _) = rectangle();
        let anchor = make(qa);
        anchored(&mut a, sa, ea, anchor);
        fixed(&mut b, sb, eb, at);
        let (marks_a, svg_a, pdf_a) = exports(&a, sa);
        let (marks_b, svg_b, pdf_b) = exports(&b, sb);
        assert_eq!(marks_a.len(), 2, "cran double : deux traits");
        for (x, y) in marks_a.iter().zip(&marks_b) {
            assert!(distance_mm(x.0, y.0) < 1e-12 && distance_mm(x.1, y.1) < 1e-12);
        }
        assert_eq!(svg_a, svg_b, "SVG identique : {anchor:?}");
        assert_eq!(pdf_a, pdf_b, "PDF identique : {anchor:?}");
    }
}

#[test]
fn l_export_suit_le_deplacement_du_point_d_ancrage() {
    let (mut p, s, e, q) = rectangle();
    anchored(&mut p, s, e, NotchAnchor::Point { point: q });
    let (_, svg_before, _) = exports(&p, s);
    p.apply(Command::MovePoint {
        sheet: s,
        point: q,
        to: pt(90.0, 25.0),
    })
    .unwrap();
    let (marks, svg_after, _) = exports(&p, s);
    assert_ne!(svg_before, svg_after);
    // Cran double centré en 90 mm : traits à 88,5 et 91,5 mm, de 5 mm, sur le bord du bas.
    assert!(distance_mm(marks[0].0, pt(88.5, 0.0)) < 1e-9);
    assert!(distance_mm(marks[1].0, pt(91.5, 0.0)) < 1e-9);
    assert!(distance_mm(marks[0].1, pt(88.5, 5.0)) < 1e-9);
}
