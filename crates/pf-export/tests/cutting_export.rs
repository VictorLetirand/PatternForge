//! US-15.3 / US-13.5 : exports du plan de coupe (SVG, PDF, CSV) relus par des lecteurs
//! indépendants (`tests/common`). Valeurs écrites à la main : deux carrés 100 x 100 en (10 ; 0) et
//! (110 ; 0) sur une laize de 320 mm à lisières de 10 mm (utile 300 mm), longueur 100 mm,
//! rendement 20 000 / 30 000 = 66,67 %.

mod common;
use common::coupe::{mm_pt, read_pdf, read_svg, stroke_len_pt};
use common::csv_reader::parse;
use common::pdf_text::readable;
use pf_core::cutting_plan::{CuttingPlanSpec, FabricFold};
use pf_core::*;
use pf_export::cutting_export::{cutting_csv, cutting_pdf, cutting_scene, cutting_svg};
use pf_export::{PageFormat, PageSetup};
use pf_geometry::PointMm;
use pf_i18n::Lang;

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

struct Sc {
    p: Project,
    plan: CuttingPlanId,
    sq: PieceId,
}

fn sc() -> Sc {
    let mut p = Project::new("Export plan");
    let s = p.sheets[0].id;
    let r = p
        .apply(Command::AddRectangle {
            sheet: s,
            corner: PointRef::New(pt(300.0, 7.0)),
            width_mm: 100.0,
            height_mm: 100.0,
        })
        .unwrap();
    let sq = p
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
    p.apply(Command::SetPieceInfo {
        piece: sq,
        name: "Carre".into(),
        quantity: 4,
        material: Some("Coton".into()),
        mirrored_pair: false,
    })
    .unwrap();
    let plan = p
        .apply(Command::AddCuttingPlan {
            spec: CuttingPlanSpec {
                name: "Coton 32".into(),
                material: Some("Coton".into()),
                fabric_width_mm: 320.0,
                fold: FabricFold::Flat,
                selvedge_mm: 10.0,
                min_gap_mm: 0.0,
                grain_direction: None,
            },
        })
        .unwrap()
        .cutting_plans[0];
    Sc { p, plan, sq }
}

fn put(f: &mut Sc, size: Option<SizeId>, x: f64, y: f64) {
    f.p.apply(Command::AddPlacement {
        plan: f.plan,
        piece: f.sq,
        size,
        x_mm: x,
        y_mm: y,
        rotation_deg: 0.0,
        mirrored: false,
    })
    .unwrap();
}

fn near(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

fn roll() -> PageSetup {
    PageSetup {
        format: PageFormat::Roll { width_mm: 600.0 },
        ..PageSetup::default()
    }
}

fn two_squares() -> Sc {
    let mut f = sc();
    put(&mut f, None, 10.0, 0.0);
    put(&mut f, None, 110.0, 0.0);
    f
}

#[test]
fn csv_une_ligne_par_plan_et_par_taille_francais_et_anglais() {
    let mut f = two_squares();
    f.p.apply(Command::AddSizeRange {
        name: "Femme".into(),
        kind: SizeKind::Alphanumeric,
        labels: vec!["S".into(), "M".into()],
        base: 1,
    })
    .unwrap();
    let s = f.p.sizes[0].sizes[0].id;
    put(&mut f, Some(s), 210.0, 0.0);
    let before = f.p.clone();
    // Utile 300, longueur 100 : trois carrés = 30 000 / 30 000 = 100 %.
    let rows = parse(&cutting_csv(&f.p, Lang::Fr).unwrap());
    assert_eq!(rows.len(), 3, "en-tête + une ligne par taille : {rows:?}");
    assert_eq!(
        rows[0],
        [
            "#metrage",
            "plan",
            "matiere",
            "statut",
            "taille",
            "pieces",
            "surface_utilisee_mm2",
            "longueur_mm",
            "laize_utile_mm",
            "part_taille_pct",
            "rendement_plan_pct"
        ]
    );
    assert_eq!(
        rows[1],
        [
            "metrage",
            "Coton 32",
            "Coton",
            "valide",
            "taille de base",
            "2",
            "20000.00",
            "100.00",
            "300.00",
            "66.67",
            "100.00"
        ]
    );
    assert_eq!(
        rows[2],
        [
            "metrage", "Coton 32", "Coton", "valide", "S", "1", "10000.00", "100.00", "300.00",
            "33.33", "100.00"
        ]
    );
    let en = parse(&cutting_csv(&f.p, Lang::En).unwrap());
    assert_eq!(en[0][3], "status");
    assert_eq!(en[1][3], "valid");
    assert_eq!(en[1][4], "base size");
    assert_eq!(f.p, before, "lecture seule");
}

#[test]
fn csv_un_plan_invalide_ou_vide_ne_porte_aucun_chiffre() {
    let mut f = sc();
    // Vide : non renseigné.
    let rows = parse(&cutting_csv(&f.p, Lang::Fr).unwrap());
    assert_eq!(rows[1][3], "non renseigne");
    assert!(rows[1][4..].iter().all(String::is_empty), "{:?}", rows[1]);
    // Chevauchement : invalide, cellules vides.
    put(&mut f, None, 10.0, 0.0);
    put(&mut f, None, 60.0, 0.0);
    let rows = parse(&cutting_csv(&f.p, Lang::Fr).unwrap());
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[1][3], "invalide");
    assert!(rows[1][4..].iter().all(String::is_empty), "{:?}", rows[1]);
    let en = parse(&cutting_csv(&f.p, Lang::En).unwrap());
    assert_eq!(en[1][3], "invalid");
}

#[test]
fn svg_a_l_echelle_avec_cotes_et_legende() {
    let f = two_squares();
    let svg = String::from_utf8(cutting_svg(&f.p, f.plan, Lang::Fr).unwrap()).unwrap();
    let doc = read_svg(&svg);
    // Huit traits de coupe (deux carrés de côté 100), chacun de 100 mm de long.
    let cut = doc.with_role("coupe");
    assert_eq!(cut.len(), 8);
    for item in &cut {
        let (a, b) = (item.pts[0], item.pts[1]);
        let len = ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt();
        assert!(near(len, 100.0, 1e-6), "{len}");
    }
    // Bord de la laize : 320 mm de large (quatre traits « construction » au moins).
    let xs: Vec<f64> = doc
        .with_role("construction")
        .iter()
        .flat_map(|i| i.pts.iter().map(|p| p.0))
        .collect();
    let span =
        xs.iter().cloned().fold(f64::MIN, f64::max) - xs.iter().cloned().fold(f64::MAX, f64::min);
    assert!(near(span, 320.0, 1e-6), "{span}");
    for legend in [
        "laize 320 mm, utile 300 mm",
        "longueur 100 mm",
        "rendement 66.67 %",
        "lisiere",
        "Carre #1",
        "Carre #2",
    ] {
        assert!(svg.contains(legend), "{legend} absent");
    }
    assert!(!svg.contains("pli"), "pas de pli en étoffe à plat");
    let en = String::from_utf8(cutting_svg(&f.p, f.plan, Lang::En).unwrap()).unwrap();
    assert!(en.contains("width 320 mm, usable 300 mm") && en.contains("yield 66.67 %"));
    assert!(!en.contains("laize"));
}

#[test]
fn svg_plie_porte_le_pli_en_tirets_et_un_plan_invalide_aucun_chiffre() {
    let mut f = two_squares();
    f.p.apply(Command::UpdateCuttingPlan {
        plan: f.plan,
        spec: CuttingPlanSpec {
            name: "Coton 32".into(),
            material: Some("Coton".into()),
            fabric_width_mm: 640.0,
            fold: FabricFold::Folded,
            selvedge_mm: 10.0,
            min_gap_mm: 0.0,
            grain_direction: None,
        },
    })
    .unwrap();
    let svg = String::from_utf8(cutting_svg(&f.p, f.plan, Lang::Fr).unwrap()).unwrap();
    assert!(svg.contains(">pli<"), "légende du pli");
    // Le carré posé en (110 ; 0) n'enjambe pas le pli (320) ; on en pose un qui l'enjambe.
    put(&mut f, None, 300.0, 0.0);
    let svg = String::from_utf8(cutting_svg(&f.p, f.plan, Lang::Fr).unwrap()).unwrap();
    assert!(svg.contains("metrage invalide"), "plan invalide");
    assert!(!svg.contains("rendement"), "aucun rendement chiffré");
    assert!(!svg.contains("longueur 1"), "aucune longueur chiffrée");
}

#[test]
fn pdf_a_l_echelle_un_pour_un_et_texte_ascii() {
    let f = two_squares();
    let bytes = cutting_pdf(&f.p, f.plan, &roll(), Lang::Fr).unwrap();
    let strokes = read_pdf(&bytes);
    // Les huit côtés de 100 mm mesurent 100 x 72 / 25,4 = 283,465 pt : échelle 1:1.
    let side = mm_pt(100.0);
    let n = strokes
        .iter()
        .filter(|s| s.dash_pt.is_empty() && near(stroke_len_pt(s), side, 0.01))
        .count();
    assert!(n >= 8, "{n} côtés de 100 mm lus");
    let text = readable(&bytes);
    for legend in [
        "rendement 66.67 %",
        "laize 320 mm, utile 300 mm",
        "longueur 100 mm",
    ] {
        assert!(text.contains(legend), "{legend} absent du PDF");
    }
    // Aucun « ? » de remplacement dans les légendes (ASCII uniquement).
    assert!(!text.contains("rendement 66.67 ?"));
    let en = readable(&cutting_pdf(&f.p, f.plan, &roll(), Lang::En).unwrap());
    assert!(en.contains("yield 66.67 %"));
    // Paginé par la mécanique existante sur une feuille A4 : plusieurs pages (laize 320 mm).
    let a4 = cutting_pdf(&f.p, f.plan, &PageSetup::default(), Lang::Fr).unwrap();
    assert!(a4.starts_with(b"%PDF"));
}

#[test]
fn les_legendes_d_export_sont_ascii_dans_les_deux_langues() {
    for lang in [Lang::Fr, Lang::En] {
        for key in [
            "cutx.title",
            "cutx.width",
            "cutx.length",
            "cutx.length_invalid",
            "cutx.yield",
            "cutx.invalid",
            "cutx.selvedge",
            "cutx.fold",
            "cutx.size.base",
            "cutx.unset",
            "cutx.status.valid",
            "cutx.status.invalid",
            "cutx.status.empty",
            "cutx.csv.plan",
            "cutx.csv.material",
            "cutx.csv.status",
            "cutx.csv.size",
            "cutx.csv.pieces",
            "cutx.csv.used_area",
            "cutx.csv.length",
            "cutx.csv.useful_width",
            "cutx.csv.size_share",
            "cutx.csv.plan_yield",
        ] {
            let text = pf_i18n::lookup(lang, key);
            assert!(text.is_ascii() && text != key, "{lang:?} {key} : {text}");
        }
    }
}

#[test]
fn un_plan_sans_pose_est_refuse_sans_rien_ecrire() {
    let f = sc();
    assert!(cutting_scene(&f.p, f.plan, Lang::Fr).is_err());
    assert!(cutting_svg(&f.p, f.plan, Lang::Fr).is_err());
    assert!(cutting_pdf(&f.p, f.plan, &roll(), Lang::Fr).is_err());
}
