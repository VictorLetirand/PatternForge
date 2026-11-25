//! US-15.5 « Cuir » : exports du plan de coupe **avec peau** (SVG, PDF 1:1, CSV), relus par les
//! lecteurs indépendants de `tests/common`. Valeurs écrites à la main : peau rectangulaire
//! 400 x 200 (80 000 mm2), trou 40 x 40 en (100 ; 100) (1 600 mm2), zone de qualité 2 en
//! (0 ; 0)-(200 ; 200), carré 100 x 100 en (200 ; 20) (10 000 mm2) : surface utile 78 400 mm2,
//! pourcentage 100 x 10 000 / 78 400 = 12,755102 (12,76). Octets des exports sans peau : voir
//! `cutting_export.rs` et `examples/` (inchangés).

mod common;
use common::coupe::{read_pdf, read_svg};
use common::csv_reader::parse;
use common::pdf_text::readable;
use pf_core::cutting_plan::{CuttingPlanSpec, FabricFold};
use pf_core::*;
use pf_export::cutting_export::{cutting_csv, cutting_pdf, cutting_svg};
use pf_export::{PageFormat, PageSetup};
use pf_geometry::PointMm;
use pf_i18n::Lang;

fn pp(x: f64, y: f64) -> PlanePoint {
    PlanePoint { x, y }
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<PlanePoint> {
    vec![pp(x0, y0), pp(x1, y0), pp(x1, y1), pp(x0, y1)]
}

fn roll() -> PageSetup {
    PageSetup {
        format: PageFormat::Roll { width_mm: 600.0 },
        ..PageSetup::default()
    }
}

fn fixture(on_defect: bool) -> (Project, CuttingPlanId, HideDefectId) {
    let mut p = Project::new("Export cuir");
    let s = p.sheets[0].id;
    let r = p
        .apply(Command::AddRectangle {
            sheet: s,
            corner: PointRef::New(PointMm::new(300.0, 7.0)),
            width_mm: 100.0,
            height_mm: 100.0,
        })
        .unwrap();
    let piece = p
        .apply(Command::ExtractPiece {
            sheet: s,
            source: PieceSource::Paths {
                entities: r.entities,
            },
            name: "Carre".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    let plan = p
        .apply(Command::AddCuttingPlan {
            spec: CuttingPlanSpec {
                name: "Peau 40".into(),
                material: None,
                fabric_width_mm: 400.0,
                fold: FabricFold::Flat,
                selvedge_mm: 0.0,
                min_gap_mm: 0.0,
                grain_direction: None,
            },
        })
        .unwrap()
        .cutting_plans[0];
    p.apply(Command::SetHideContour {
        plan,
        contour: rect(0.0, 0.0, 400.0, 200.0),
    })
    .unwrap();
    let defect = p
        .apply(Command::AddHideDefect {
            plan,
            kind: DefectKind::Hole,
            polygon: rect(100.0, 100.0, 140.0, 140.0),
            note: Some("trou de piqure".into()),
        })
        .unwrap()
        .hide_defects[0];
    p.apply(Command::AddQualityZone {
        plan,
        grade: 2,
        polygon: rect(0.0, 0.0, 200.0, 200.0),
    })
    .unwrap();
    let (x, y) = if on_defect {
        (80.0, 80.0)
    } else {
        (200.0, 20.0)
    };
    p.apply(Command::AddPlacement {
        plan,
        piece,
        size: None,
        x_mm: x,
        y_mm: y,
        rotation_deg: 0.0,
        mirrored: false,
    })
    .unwrap();
    (p, plan, defect)
}

#[test]
fn svg_dessine_contour_defaut_hachure_zone_et_legende() {
    let (p, plan, defect) = fixture(false);
    let before = p.clone();
    let svg = String::from_utf8(cutting_svg(&p, plan, Lang::Fr).unwrap()).unwrap();
    assert_eq!(p, before, "lecture seule");
    // Libellés : peau, défaut (id et genre), zone (grade).
    assert!(svg.contains("peau"), "{svg}");
    assert!(svg.contains(&format!("D{} trou", defect.0)));
    assert!(svg.contains("qualite 2"));
    // Légende cuir à la main : 10 000 mm2 posés sur 78 400 mm2 utiles = 12,76 %.
    assert!(
        svg.contains("cuir 10000.00 mm2 sur 78400.00 mm2 utiles : 12.76 %"),
        "{svg}"
    );
    // La laize rectangulaire n'est plus dessinée : aucun libellé « laize ».
    assert!(!svg.contains("laize"));
    let doc = read_svg(&svg);
    // Contour de peau : 4 côtés de 400 / 200 mm en construction ; le trou (carré 40) = 4 côtés
    // tiretés (« couture ») plus des hachures croisées (45° et 135°).
    let sew = doc.with_role("couture");
    assert_eq!(sew.len(), 4, "contour du défaut");
    let cons = doc.with_role("construction");
    let long = cons
        .iter()
        .filter(|i| {
            let (a, b) = (i.pts[0], i.pts[1]);
            let l = ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt();
            (l - 400.0).abs() < 1e-6 || (l - 200.0).abs() < 1e-6
        })
        .count();
    assert!(long >= 4, "côtés du contour : {long}");
    // Hachures croisées du trou : des segments diagonaux (|dx| = |dy|) de pentes opposées ; les
    // côtés du contour et les tirets de zone sont, eux, parallèles aux axes.
    let slopes: Vec<f64> = cons
        .iter()
        .filter_map(|i| {
            let (a, b) = (i.pts[0], i.pts[1]);
            let (dx, dy) = (b.0 - a.0, b.1 - a.1);
            ((dx.abs() - dy.abs()).abs() < 1e-6 && dx.abs() > 1.0).then_some(dx * dy)
        })
        .collect();
    assert!(slopes.iter().any(|s| *s > 0.0), "hachure à 45°");
    assert!(slopes.iter().any(|s| *s < 0.0), "hachure à 135°");
}

#[test]
fn pdf_ascii_un_pour_un_avec_legende_cuir_en_deux_langues() {
    let (p, plan, defect) = fixture(false);
    let fr = cutting_pdf(&p, plan, &roll(), Lang::Fr).unwrap();
    assert!(fr.starts_with(b"%PDF"));
    assert!(!read_pdf(&fr).is_empty());
    let text = readable(&fr);
    assert!(
        text.contains("cuir 10000.00 mm2 sur 78400.00 mm2 utiles : 12.76 %"),
        "{text}"
    );
    assert!(text.contains(&format!("D{} trou", defect.0)));
    assert!(!text.contains("utiles : 12.76 ?"));
    let en = readable(&cutting_pdf(&p, plan, &roll(), Lang::En).unwrap());
    assert!(
        en.contains("leather 10000.00 mm2 of 78400.00 mm2 usable: 12.76 %"),
        "{en}"
    );
    assert!(en.contains(&format!("D{} hole", defect.0)));
}

#[test]
fn plan_invalide_sur_defaut_aucun_chiffre_cuir() {
    let (p, plan, _) = fixture(true);
    let svg = String::from_utf8(cutting_svg(&p, plan, Lang::Fr).unwrap()).unwrap();
    assert!(svg.contains("metrage cuir invalide"));
    assert!(!svg.contains("12.76") && !svg.contains("mm2 utiles"));
    let rows = parse(&cutting_csv(&p, Lang::Fr).unwrap());
    let head = rows.iter().position(|r| r[0] == "#peau").unwrap();
    let row = &rows[head + 1];
    // Plan, statut, contour, défauts, utile : lisibles ; posé et pourcentage : vides.
    assert_eq!(
        row[1..6],
        ["Peau 40", "invalide", "80000.00", "1600.00", "78400.00"]
    );
    assert!(row[6].is_empty() && row[7].is_empty(), "{row:?}");
    // Une pose touche le trou : 1 ; aucune hors peau.
    assert_eq!(row[8..10], ["1", "0"]);
}

#[test]
fn csv_sections_peau_valeurs_a_la_main_et_absentes_sans_peau() {
    let (p, plan, _) = fixture(false);
    let rows = parse(&cutting_csv(&p, Lang::Fr).unwrap());
    let head = rows.iter().position(|r| r[0] == "#peau").unwrap();
    assert_eq!(
        rows[head][1..],
        [
            "plan",
            "statut",
            "surface_peau_mm2",
            "surface_defauts_mm2",
            "surface_utile_mm2",
            "surface_posee_mm2",
            "pose_sur_utile_pct",
            "poses_sur_defaut",
            "poses_hors_peau"
        ]
    );
    assert_eq!(
        rows[head + 1][1..],
        ["Peau 40", "valide", "80000.00", "1600.00", "78400.00", "10000.00", "12.76", "0", "0"]
    );
    // Qualité de la pose : tout en zone 2 ? Le carré (200 ; 20)-(300 ; 120) est hors de la zone
    // (0 ; 0)-(200 ; 200) : aucune zone recoupée, cellules de grade vides.
    let poses = rows.iter().position(|r| r[0] == "#peau_poses").unwrap();
    assert_eq!(
        rows[poses + 1][1..],
        ["Peau 40", "Carre", "1", "0.00", "0.00", "", "0.00", "0.00", "0.00"]
    );
    // Sans peau : une seule section, aucun octet de plus.
    let mut q = p.clone();
    q.apply(Command::RemoveHide { plan }).unwrap();
    let rows = parse(&cutting_csv(&q, Lang::Fr).unwrap());
    assert!(rows
        .iter()
        .all(|r| r[0] != "#peau" && r[0] != "#peau_poses"));
    let en = parse(&cutting_csv(&p, Lang::En).unwrap());
    let head = en.iter().position(|r| r[0] == "#peau").unwrap();
    assert_eq!(en[head][3], "hide_area_mm2");
}

#[test]
fn legendes_cuir_ascii_dans_les_deux_langues() {
    for lang in [Lang::Fr, Lang::En] {
        for key in [
            "cutx.hide.contour",
            "cutx.hide.zone",
            "cutx.hide.defect",
            "cutx.hide.kind.hole",
            "cutx.hide.kind.scar",
            "cutx.hide.kind.stain",
            "cutx.hide.kind.thin",
            "cutx.hide.kind.other",
            "cutx.hide.yield",
            "cutx.hide.invalid",
            "cutx.csv.hide.plan",
            "cutx.csv.hide.g3",
        ] {
            let text = pf_i18n::lookup(lang, key);
            assert!(text.is_ascii() && text != key, "{lang:?} {key} : {text}");
        }
    }
}
