//! US-12.6 : dimensions et résolution des exports PNG / JPEG, valeurs calculées à la main.

use pf_core::*;
use pf_export::raster::{self, MAX_DPI, MIN_DPI};
use pf_export::{ExportError, Scene};
use pf_geometry::PointMm;

fn scene(side: f64) -> Scene {
    let mut p = Project::new("R");
    let sheet = p.sheets[0].id;
    p.apply(Command::AddPolygon {
        sheet,
        vertices: [(0.0, 0.0), (side, 0.0), (side, side), (0.0, side)]
            .iter()
            .map(|&(x, y)| PointRef::New(PointMm::new(x, y)))
            .collect(),
        closed: true,
        role: PathRole::Construction,
        layer: None,
    })
    .unwrap();
    Scene::from_sheet(&p, sheet).unwrap()
}

#[test]
fn mm_vers_pixels_arrondi_a_la_main() {
    // 100 / 25,4 x 300 = 1181,10 ; 25,4 mm = 1 pouce = dpi pixels ; 12,7 mm a 36 dpi = 18.
    assert_eq!(raster::mm_to_px(100.0, 300), 1181);
    assert_eq!(raster::mm_to_px(25.4, 36), 36);
    assert_eq!(raster::mm_to_px(25.4, 1200), 1200);
    assert_eq!(raster::mm_to_px(12.7, 36), 18);
    // 0,5 px arrondi au supérieur : 10 mm a 36 dpi = 14,17 -> 14 ; 10 mm a 72 dpi = 28,35 -> 28.
    assert_eq!(raster::mm_to_px(10.0, 36), 14);
    assert_eq!(raster::mm_to_px(10.0, 72), 28);
}

#[test]
fn taille_image_marge_comprise() {
    // 80 mm + 2 x 10 mm = 100 mm.
    let s = scene(80.0);
    assert_eq!(raster::raster_size(&s, 300).unwrap(), (1181, 1181));
    // 100 mm a 600 dpi = 2362,2 -> 2362 ; a 36 dpi = 141,7 -> 142.
    assert_eq!(raster::raster_size(&s, 600).unwrap(), (2362, 2362));
    assert_eq!(raster::raster_size(&s, 36).unwrap(), (142, 142));
}

#[test]
fn resolution_hors_bornes_refusee() {
    let s = scene(80.0);
    for dpi in [0, MIN_DPI - 1, MAX_DPI + 1, 100_000] {
        assert!(
            matches!(raster::export_png(&s, dpi), Err(ExportError::Io(_))),
            "{dpi}"
        );
        assert!(raster::export_jpeg(&s, dpi).is_err());
    }
    assert!(raster::export_png(&s, MIN_DPI).is_ok());
}

#[test]
fn png_signature_et_pixels_des_traits() {
    let s = scene(80.0);
    let bytes = raster::export_png(&s, 72).unwrap();
    assert_eq!(
        &bytes[..8],
        &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]
    );
    let r = raster::rasterize(&s, 72).unwrap();
    // 100 mm a 72 dpi = 283 px ; trait gauche a 10 mm = 28,35 px de large 0,25 mm = 0,7 px -> 1 px.
    assert_eq!((r.width, r.height), (283, 283));
    assert!(r.gray[141 * 283 + 28] < 128);
    assert_eq!(r.gray[141 * 283 + 141], 255);
    // Une image sans trait n'existe pas ici : le bord extérieur est blanc.
    assert!(r.gray[..283].iter().all(|v| *v == 255));
}
