//! US-14.1 — image de fond calibrée : import (PNG et JPEG), fichier à côté du projet, empreinte,
//! calibration à deux et quatre points, résidu, commandes (une étape annulable, refus sans trace),
//! image manquante ou modifiée, exports inchangés. Valeurs écrites à la main ; images générées
//! par le code (`fond_support`).

mod fond_support;
use fond_support::*;

use pf_core::background_image::{self as bi, ImageStatus};
use pf_core::persist;
use pf_core::*;
use pf_geometry::PointMm;

fn pp(x: f64, y: f64) -> PlanePoint {
    PlanePoint { x, y }
}

fn two(a: PlanePoint, b: PlanePoint, d: f64) -> CalibrationSpec {
    CalibrationSpec::TwoPoints {
        points_px: [a, b],
        known_distance_mm: d,
    }
}

fn four(p: [PlanePoint; 4], w: f64, h: f64) -> CalibrationSpec {
    CalibrationSpec::FourPoints {
        points_px: p,
        known_width_mm: w,
        known_height_mm: h,
    }
}

/// Projet enregistré dans un dossier neuf + une image PNG 400 × 300 importée à côté.
struct Fx {
    dir: std::path::PathBuf,
    file: std::path::PathBuf,
    doc: Document,
    sheet: SheetId,
    img: bi::ImportedImage,
}

fn fixture(name: &str) -> Fx {
    let dir = tmp("fond_image", name);
    let file = dir.join("jupe.pforge.json");
    let project = Project::new("Jupe");
    let sheet = project.sheets[0].id;
    persist::save(&file, &project).unwrap();
    let src = dir.join("photo patron (1).png");
    std::fs::write(&src, png_gray(400, 300, checker(20))).unwrap();
    let img = bi::import_image(&file, &src).unwrap();
    Fx {
        dir,
        file,
        doc: Document::new(project),
        sheet,
        img,
    }
}

impl Fx {
    fn set(&mut self, cal: CalibrationSpec) -> Result<Outcome, CoreError> {
        self.doc.execute(Command::SetBackground {
            sheet: self.sheet,
            image: self.img.relative.clone(),
            sha256: self.img.sha256.clone(),
            width_px: self.img.width_px,
            height_px: self.img.height_px,
            calibration: cal,
        })
    }
    fn bg(&self) -> Option<Background> {
        self.doc.project().sheets[0].background.clone()
    }
}

#[test]
fn import_png_copie_a_cote_du_projet_avec_empreinte_et_dimensions() {
    let fx = fixture("import_png");
    assert_eq!((fx.img.width_px, fx.img.height_px), (400, 300));
    // Dossier voisin `<nom-projet>.assets/`, chemin relatif sûr, nom assaini.
    assert!(
        fx.img.relative.starts_with("jupe.assets/"),
        "{}",
        fx.img.relative
    );
    assert!(
        fx.img.relative.ends_with("-photo_patron__1_.png"),
        "{}",
        fx.img.relative
    );
    assert_eq!(
        pf_core::sheet_background::unsafe_image_path(&fx.img.relative),
        None
    );
    let copy = fx.dir.join(&fx.img.relative);
    let bytes = std::fs::read(&copy).unwrap();
    // L'empreinte stockée est celle du fichier copié, et le fichier copié est identique à l'original.
    assert_eq!(
        fx.img.sha256,
        pf_core::file_guard::revision_of(&bytes).trim_start_matches("sha256:")
    );
    assert_eq!(
        bytes,
        std::fs::read(fx.dir.join("photo patron (1).png")).unwrap()
    );
    assert!(fx.img.relative.contains(&fx.img.sha256[..12]));
    // Réimporter la même image réutilise la copie ; aucun fichier temporaire ne reste.
    let again = bi::import_image(&fx.file, &fx.dir.join("photo patron (1).png")).unwrap();
    assert_eq!(again, fx.img);
    let names: Vec<_> = std::fs::read_dir(fx.dir.join("jupe.assets"))
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(names.len(), 1, "{names:?}");
}

#[test]
fn import_jpeg_dimensions_et_pixels_proches() {
    let dir = tmp("fond_image", "jpeg");
    let file = dir.join("a.pforge.json");
    persist::save(&file, &Project::new("A")).unwrap();
    let src = dir.join("scan.jpeg");
    // Rectangle blanc 40 × 20 sur fond noir dans 100 × 60.
    std::fs::write(&src, jpeg_gray(100, 60, rect(30, 20, 70, 40))).unwrap();
    let img = bi::import_image(&file, &src).unwrap();
    assert_eq!((img.width_px, img.height_px), (100, 60));
    assert!(img.relative.ends_with(".jpg"), "{}", img.relative);
    let bytes = std::fs::read(dir.join(&img.relative)).unwrap();
    let dec = bi::decode(&bytes).unwrap();
    let gray = dec.gray();
    assert!(
        gray[30 * 100 + 50] > 230,
        "dedans : {}",
        gray[30 * 100 + 50]
    );
    assert!(gray[5 * 100 + 5] < 25, "dehors : {}", gray[5 * 100 + 5]);
}

#[test]
fn le_format_se_reconnait_a_l_entete_pas_a_l_extension() {
    let dir = tmp("fond_image", "entete");
    let file = dir.join("a.pforge.json");
    persist::save(&file, &Project::new("A")).unwrap();
    // PNG nommé .jpg, texte nommé .png, fichier vide, PNG tronqué.
    let png = png_gray(40, 40, checker(5));
    std::fs::write(dir.join("faux.jpg"), &png).unwrap();
    assert!(bi::import_image(&file, &dir.join("faux.jpg")).is_ok());
    std::fs::write(dir.join("texte.png"), b"ceci n'est pas une image").unwrap();
    assert!(bi::import_image(&file, &dir.join("texte.png"))
        .unwrap_err()
        .contains("ni un PNG ni un JPEG"));
    std::fs::write(dir.join("vide.png"), b"").unwrap();
    assert!(bi::import_image(&file, &dir.join("vide.png")).is_err());
    std::fs::write(dir.join("coupe.png"), &png[..png.len() / 2]).unwrap();
    let cut = std::fs::read(dir.join("coupe.png")).unwrap();
    assert!(bi::decode(&cut).is_err(), "un PNG tronqué ne se décode pas");
    assert!(bi::import_image(&file, &dir.join("absent.png")).is_err());
}

#[test]
fn projet_non_enregistre_ou_dossier_assets_sans_nom_refuse() {
    let dir = tmp("fond_image", "nonnomme");
    let src = dir.join("a.png");
    std::fs::write(&src, png_gray(10, 10, checker(2))).unwrap();
    assert!(bi::import_image(std::path::Path::new(""), &src).is_err());
}

#[test]
fn pose_deux_points_echelle_1_mm_par_pixel_residu_zero_une_etape() {
    let mut fx = fixture("deux_points");
    fx.set(two(pp(100.0, 100.0), pp(300.0, 100.0), 200.0))
        .unwrap();
    let b = fx.bg().unwrap();
    assert_eq!(b.image, fx.img.relative);
    assert_eq!((b.opacity, b.visible, b.locked), (0.5, true, false));
    let Calibration::TwoPoints {
        residual_mm,
        known_distance_mm,
        ..
    } = b.calibration
    else {
        panic!()
    };
    assert_eq!(known_distance_mm, 200.0);
    assert!(residual_mm < 1e-9);
    // mm ↔ px : échelle exacte 1 mm / px, p1 à l'origine, y du plan vers le haut.
    let m = b.map().unwrap();
    let q = m.to_mm(PointMm::new(300.0, 100.0)).unwrap();
    assert!((q.x - 200.0).abs() < 1e-9 && q.y.abs() < 1e-9);
    // Les coins de l'image : 400 × 300 px = 400 × 300 mm, haut-gauche à (-100 ; +100).
    let c = b.corners_mm().unwrap();
    assert!((c[0].x + 100.0).abs() < 1e-9 && (c[0].y - 100.0).abs() < 1e-9);
    assert!((c[2].x - 300.0).abs() < 1e-9 && (c[2].y + 200.0).abs() < 1e-9);
    let r = b.residuals().unwrap();
    assert_eq!(r.per_point_mm.len(), 2);
    // Une seule étape annulable.
    assert!(fx.doc.can_undo());
    assert!(fx.doc.undo());
    assert!(fx.bg().is_none());
    assert!(!fx.doc.can_undo());
}

#[test]
fn quatre_points_rectangle_100_par_50_en_perspective() {
    let mut fx = fixture("quatre_points");
    // Bas-gauche, bas-droite, haut-droite, haut-gauche (pixels, y vers le bas) d'un rectangle
    // 100 × 50 mm vu en perspective ; la homographie qui les envoie sur les coins est connue.
    let px = [
        pp(60.0, 280.0),
        pp(380.0, 290.0),
        pp(340.0, 60.0),
        pp(90.0, 40.0),
    ];
    fx.set(four(px, 100.0, 50.0)).unwrap();
    let b = fx.bg().unwrap();
    let r = b.residuals().unwrap();
    assert_eq!(r.per_point_mm.len(), 4);
    assert!(r.rms_mm < 1e-6, "{}", r.rms_mm);
    assert!(r.per_point_mm.iter().all(|e| *e < 1e-6));
    let m = b.map().unwrap();
    for (p, t) in px
        .iter()
        .zip([(0.0, 0.0), (100.0, 0.0), (100.0, 50.0), (0.0, 50.0)])
    {
        let q = m.to_mm(PointMm::new(p.x, p.y)).unwrap();
        assert!(
            (q.x - t.0).abs() < 1e-6 && (q.y - t.1).abs() < 1e-6,
            "{q:?} vs {t:?}"
        );
        let back = m.to_px(q).unwrap();
        assert!((back.x - p.x).abs() < 1e-6 && (back.y - p.y).abs() < 1e-6);
    }
    // La perspective n'est pas stockée : le fichier ne porte que les quatre points et le rectangle.
    let json = persist::to_json(fx.doc.project()).unwrap();
    assert!(json.contains("\"four_points\"") && !json.contains("homograph"));
}

#[test]
fn recalibrer_ajuster_affichage_verrouiller_et_retirer() {
    let mut fx = fixture("cycle");
    fx.set(two(pp(100.0, 100.0), pp(300.0, 100.0), 200.0))
        .unwrap();
    // Recalibrer : 200 px = 100 mm -> 0,5 mm/px.
    fx.doc
        .execute(Command::CalibrateBackground {
            sheet: fx.sheet,
            calibration: two(pp(100.0, 100.0), pp(300.0, 100.0), 100.0),
        })
        .unwrap();
    let q = fx
        .bg()
        .unwrap()
        .map()
        .unwrap()
        .to_mm(PointMm::new(300.0, 100.0))
        .unwrap();
    assert!((q.x - 100.0).abs() < 1e-9);
    // Affichage.
    fx.doc
        .execute(Command::SetBackgroundDisplay {
            sheet: fx.sheet,
            opacity: 0.25,
            visible: false,
            locked: true,
        })
        .unwrap();
    let b = fx.bg().unwrap();
    assert_eq!((b.opacity, b.visible, b.locked), (0.25, false, true));
    // Verrouillé : recalibrer, relier, retirer refusés sans trace ; l'affichage reste réglable.
    let before = fx.doc.project().clone();
    let undo_len = fx.doc.can_undo();
    for c in [
        Command::CalibrateBackground {
            sheet: fx.sheet,
            calibration: two(pp(1.0, 1.0), pp(9.0, 1.0), 8.0),
        },
        Command::RemoveBackground { sheet: fx.sheet },
        Command::RelinkBackground {
            sheet: fx.sheet,
            image: fx.img.relative.clone(),
            sha256: fx.img.sha256.clone(),
            width_px: 400,
            height_px: 300,
        },
    ] {
        let e = fx.doc.execute(c).unwrap_err().to_string();
        assert!(e.contains("verrouillée"), "{e}");
    }
    assert_eq!(*fx.doc.project(), before);
    assert_eq!(fx.doc.can_undo(), undo_len);
    // Sans effet : aucune étape.
    let steps = |d: &mut Document| {
        let mut n = 0;
        let snapshot = d.project().clone();
        while d.undo() {
            n += 1;
        }
        while d.redo() {}
        assert_eq!(*d.project(), snapshot);
        n
    };
    let n0 = steps(&mut fx.doc);
    fx.doc
        .execute(Command::SetBackgroundDisplay {
            sheet: fx.sheet,
            opacity: 0.25,
            visible: false,
            locked: true,
        })
        .unwrap();
    assert_eq!(steps(&mut fx.doc), n0);
    fx.doc
        .execute(Command::SetBackgroundDisplay {
            sheet: fx.sheet,
            opacity: 0.25,
            visible: false,
            locked: false,
        })
        .unwrap();
    fx.doc
        .execute(Command::RemoveBackground { sheet: fx.sheet })
        .unwrap();
    assert!(fx.bg().is_none());
    // Le fichier image n'est jamais supprimé.
    assert!(fx.dir.join(&fx.img.relative).exists());
}

#[test]
fn refus_sans_trace_poses_invalides() {
    let mut fx = fixture("refus");
    let good = two(pp(100.0, 100.0), pp(300.0, 100.0), 200.0);
    let base = fx.doc.project().clone();
    let try_set = |fx: &mut Fx, image: &str, sha: &str, w: u32, h: u32, cal: CalibrationSpec| {
        let e = fx.doc.execute(Command::SetBackground {
            sheet: fx.sheet,
            image: image.into(),
            sha256: sha.into(),
            width_px: w,
            height_px: h,
            calibration: cal,
        });
        assert!(e.is_err(), "{image} {sha} {w}x{h}");
        assert_eq!(*fx.doc.project(), base);
        assert!(!fx.doc.can_undo());
    };
    let (img, sha) = (fx.img.relative.clone(), fx.img.sha256.clone());
    try_set(&mut fx, "../photo.png", &sha, 400, 300, good.clone());
    try_set(&mut fx, "C:/photo.png", &sha, 400, 300, good.clone());
    try_set(&mut fx, "/abs/photo.png", &sha, 400, 300, good.clone());
    try_set(&mut fx, "a\\b.png", &sha, 400, 300, good.clone());
    try_set(&mut fx, &img, "abc", 400, 300, good.clone());
    try_set(&mut fx, &img, &sha.to_uppercase(), 400, 300, good.clone());
    try_set(&mut fx, &img, &sha, 0, 300, good.clone());
    try_set(&mut fx, &img, &sha, 400, 0, good.clone());
    try_set(&mut fx, &img, &sha, 200_000, 300, good.clone());
    try_set(
        &mut fx,
        &img,
        &sha,
        400,
        300,
        two(pp(1.0, 1.0), pp(1.0, 1.0), 10.0),
    );
    try_set(
        &mut fx,
        &img,
        &sha,
        400,
        300,
        two(pp(1.0, 1.0), pp(5.0, 1.0), 0.0),
    );
    try_set(
        &mut fx,
        &img,
        &sha,
        400,
        300,
        two(pp(1.0, 1.0), pp(5.0, 1.0), f64::NAN),
    );
    try_set(
        &mut fx,
        &img,
        &sha,
        400,
        300,
        two(pp(1.0, 1.0), pp(5.0, 1.0), f64::INFINITY),
    );
    try_set(
        &mut fx,
        &img,
        &sha,
        400,
        300,
        two(pp(1.0, 1.0), pp(500.0, 1.0), 10.0),
    );
    try_set(
        &mut fx,
        &img,
        &sha,
        400,
        300,
        two(pp(-1.0, 1.0), pp(5.0, 1.0), 10.0),
    );
    try_set(
        &mut fx,
        &img,
        &sha,
        400,
        300,
        two(pp(f64::NAN, 1.0), pp(5.0, 1.0), 10.0),
    );
    try_set(
        &mut fx,
        &img,
        &sha,
        400,
        300,
        four(
            [pp(0.0, 0.0), pp(10.0, 0.0), pp(20.0, 0.0), pp(30.0, 0.0)],
            100.0,
            50.0,
        ),
    );
    // Sens horaire (image en miroir) refusé.
    try_set(
        &mut fx,
        &img,
        &sha,
        400,
        300,
        four(
            [
                pp(50.0, 250.0),
                pp(50.0, 50.0),
                pp(350.0, 50.0),
                pp(350.0, 250.0),
            ],
            100.0,
            50.0,
        ),
    );
    // Feuille inconnue, puis double pose.
    assert!(fx
        .doc
        .execute(Command::SetBackground {
            sheet: SheetId(9999),
            image: img.clone(),
            sha256: sha.clone(),
            width_px: 400,
            height_px: 300,
            calibration: good.clone(),
        })
        .is_err());
    fx.set(good.clone()).unwrap();
    let e = fx.set(good).unwrap_err().to_string();
    assert!(e.contains("déjà une image de fond"), "{e}");
    // Recalibration, reliage, affichage sans fond ailleurs : refus nommés.
    let s2 = fx
        .doc
        .execute(Command::CreateSheet { name: "B".into() })
        .unwrap();
    let _ = s2;
    let other = fx.doc.project().sheets[1].id;
    for c in [
        Command::CalibrateBackground {
            sheet: other,
            calibration: two(pp(1.0, 1.0), pp(9.0, 1.0), 8.0),
        },
        Command::RemoveBackground { sheet: other },
        Command::SetBackgroundDisplay {
            sheet: other,
            opacity: 0.5,
            visible: true,
            locked: false,
        },
    ] {
        assert!(fx
            .doc
            .execute(c)
            .unwrap_err()
            .to_string()
            .contains("pas d'image de fond"));
    }
    for o in [-0.1, 1.1, f64::NAN, f64::INFINITY] {
        assert!(fx
            .doc
            .execute(Command::SetBackgroundDisplay {
                sheet: fx.sheet,
                opacity: o,
                visible: true,
                locked: false
            })
            .is_err());
    }
}

#[test]
fn image_manquante_ou_modifiee_signalee_sans_perte_et_reliage_explicite() {
    let mut fx = fixture("statut");
    fx.set(two(pp(100.0, 100.0), pp(300.0, 100.0), 200.0))
        .unwrap();
    persist::save(&fx.file, fx.doc.project()).unwrap();
    let b = fx.bg().unwrap();
    assert_eq!(bi::status(&fx.dir, &b), ImageStatus::Ok);
    let copy = fx.dir.join(&b.image);
    // Image modifiée : même taille, autre contenu.
    std::fs::write(&copy, png_gray(400, 300, noise(7))).unwrap();
    assert_eq!(bi::status(&fx.dir, &b), ImageStatus::Modified);
    assert!(bi::load(&fx.dir, &b).unwrap_err().contains("modifiée"));
    // Le projet s'ouvre quand même, fond intact, rien n'est perdu.
    let reopened = persist::load(&fx.file).unwrap();
    assert_eq!(reopened.sheets[0].background, Some(b.clone()));
    // Image introuvable.
    std::fs::remove_file(&copy).unwrap();
    assert_eq!(bi::status(&fx.dir, &b), ImageStatus::Missing);
    assert!(bi::load(&fx.dir, &b).is_err());
    // Dossier à la place du fichier : jamais de panic.
    std::fs::create_dir_all(&copy).unwrap();
    assert_eq!(bi::status(&fx.dir, &b), ImageStatus::Missing);
    std::fs::remove_dir_all(&copy).unwrap();
    // Reliage explicite à un autre fichier de mêmes dimensions.
    let other_src = fx.dir.join("autre.png");
    std::fs::write(&other_src, png_gray(400, 300, noise(9))).unwrap();
    let other = bi::import_image(&fx.file, &other_src).unwrap();
    let cal_before = fx.bg().unwrap().calibration;
    fx.doc
        .execute(Command::RelinkBackground {
            sheet: fx.sheet,
            image: other.relative.clone(),
            sha256: other.sha256.clone(),
            width_px: 400,
            height_px: 300,
        })
        .unwrap();
    let b2 = fx.bg().unwrap();
    assert_eq!(b2.calibration, cal_before);
    assert_eq!(bi::status(&fx.dir, &b2), ImageStatus::Ok);
    assert!(bi::load(&fx.dir, &b2).is_ok());
    // Dimensions différentes : refus, fond intact.
    let big_src = fx.dir.join("grand.png");
    std::fs::write(&big_src, png_gray(500, 300, noise(3))).unwrap();
    let big = bi::import_image(&fx.file, &big_src).unwrap();
    let e = fx
        .doc
        .execute(Command::RelinkBackground {
            sheet: fx.sheet,
            image: big.relative,
            sha256: big.sha256,
            width_px: 500,
            height_px: 300,
        })
        .unwrap_err()
        .to_string();
    assert!(e.contains("500 × 300"), "{e}");
    assert_eq!(fx.bg().unwrap(), b2);
}

#[test]
fn l_image_n_est_jamais_dans_le_json_et_le_fichier_reste_petit() {
    let mut fx = fixture("json");
    fx.set(two(pp(100.0, 100.0), pp(300.0, 100.0), 200.0))
        .unwrap();
    let json = persist::to_json(fx.doc.project()).unwrap();
    let image_bytes = std::fs::metadata(fx.dir.join(&fx.img.relative))
        .unwrap()
        .len() as usize;
    assert!(
        json.len() < 4000 && json.len() < image_bytes + 4000,
        "{}",
        json.len()
    );
    assert!(json.contains(&fx.img.relative) && json.contains(&fx.img.sha256));
    assert!(!json.contains("base64") && !json.contains("iVBOR"));
    // Aller-retour exact, schéma toujours 6.
    let back = persist::from_json(&json).unwrap();
    assert_eq!(back, *fx.doc.project());
    assert_eq!(SCHEMA_VERSION, 6);
}

#[test]
fn les_exports_sont_inchanges_octet_pour_octet_avec_un_fond() {
    let mut fx = fixture("exports");
    fx.doc
        .execute(Command::AddRectangle {
            sheet: fx.sheet,
            corner: PointRef::New(PointMm::new(0.0, 0.0)),
            width_mm: 100.0,
            height_mm: 60.0,
        })
        .unwrap();
    fx.doc
        .execute(Command::ExtractPiece {
            sheet: fx.sheet,
            source: PieceSource::Click {
                at: PointMm::new(50.0, 30.0),
            },
            name: "Devant".into(),
            detached: false,
        })
        .unwrap();
    let sheet = fx.sheet;
    let render = |p: &Project| {
        let scene = pf_export::Scene::from_sheet(p, sheet).unwrap();
        (
            pf_export::svg::export_svg(&scene).unwrap(),
            pf_export::pdf::export_pdf(std::slice::from_ref(&scene))
                .unwrap()
                .bytes,
            pf_export::dxf::export_dxf(p).unwrap().text,
            format!("{:?}", scene.paths.len()),
        )
    };
    let without = render(fx.doc.project());
    fx.set(two(pp(100.0, 100.0), pp(300.0, 100.0), 200.0))
        .unwrap();
    let with = render(fx.doc.project());
    assert_eq!(without, with);
}

#[test]
fn pf_validate_couvre_le_fond_et_signale_image_manquante_ou_modifiee() {
    let mut fx = fixture("validate");
    fx.set(two(pp(100.0, 100.0), pp(300.0, 100.0), 200.0))
        .unwrap();
    persist::save(&fx.file, fx.doc.project()).unwrap();
    let run = |json: bool| {
        let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_pf-validate"));
        cmd.arg(&fx.file);
        if json {
            cmd.arg("--json");
        }
        let out = cmd.output().unwrap();
        (
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stdout).into_owned(),
        )
    };
    // Image présente : valide (0), état et résidu cités.
    let (code, out) = run(false);
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains("Image de fond") && out.contains("empreinte conforme"),
        "{out}"
    );
    let (code, json) = run(true);
    assert_eq!(code, 0);
    let v: serde_json::Value = serde_json::from_str(json.trim()).unwrap();
    assert_eq!(v["backgrounds"][0]["status"], "ok");
    assert_eq!(v["backgrounds"][0]["residual_mm"], 0.0);
    // Image modifiée : toujours valide (0), signalée.
    let copy = fx.dir.join(&fx.img.relative);
    std::fs::write(&copy, png_gray(400, 300, noise(5))).unwrap();
    let (code, out) = run(false);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("image modifiée"), "{out}");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(run(true).1.trim()).unwrap()["backgrounds"][0]
            ["status"],
        "modified"
    );
    // Image introuvable : toujours valide (0), signalée.
    std::fs::remove_file(&copy).unwrap();
    let (code, out) = run(false);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("image introuvable"), "{out}");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(run(true).1.trim()).unwrap()["backgrounds"][0]
            ["status"],
        "missing"
    );
    // Sans fond : la sortie JSON d'avant, sans clé « backgrounds ».
    persist::save(&fx.file, &Project::new("Sans fond")).unwrap();
    let v: serde_json::Value = serde_json::from_str(run(true).1.trim()).unwrap();
    assert!(v.get("backgrounds").is_none());
}
