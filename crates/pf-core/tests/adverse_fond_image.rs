//! US-14.1 — campagne adverse de l'image de fond : 160 séquences à graine fixe (graine = numéro de
//! séquence), oracle **indépendant** écrit ici (modèle d'état + cartes pixels -> mm générées par le
//! test, jamais relues du code testé), compteurs de couverture **assertés** à la fin.
//!
//! Chaque séquence enchaîne des importations de fichiers (PNG gris / RVB, JPEG, tronqués, vides,
//! non-images), des commandes valides et dégénérées (points confondus, alignés, hors image, sens
//! horaire, distance nulle / négative / non finie, chemin `..`, absolu, antislash, empreinte mal
//! formée, dimensions nulles ou démesurées), des annulations / rétablissements **exacts à l'octet**,
//! le verrouillage, le reliage, et des fichiers modifiés ou supprimés après coup.

mod fond_support;
use fond_support::*;

use pf_core::background_image as bi;
use pf_core::persist;
use pf_core::*;
use pf_geometry::PointMm;
use std::collections::BTreeMap;

const SEQUENCES: u64 = 160;
const IMG_W: u32 = 64;
const IMG_H: u32 = 48;

fn pp(x: f64, y: f64) -> PlanePoint {
    PlanePoint { x, y }
}

// ----- oracle : modèle d'état ----------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
enum MCal {
    Two { p: [(f64, f64); 2], d: f64 },
    Four { p: [(f64, f64); 4], w: f64, h: f64 },
}

#[derive(Clone, Debug, PartialEq)]
struct Model {
    image: String,
    sha: String,
    w: u32,
    h: u32,
    cal: MCal,
    opacity: f64,
    visible: bool,
    locked: bool,
}

fn inside(p: (f64, f64), w: u32, h: u32) -> bool {
    p.0.is_finite()
        && p.1.is_finite()
        && p.0 >= 0.0
        && p.1 >= 0.0
        && p.0 <= f64::from(w)
        && p.1 <= f64::from(h)
}

/// Quadrilatère convexe parcouru dans le sens anti-horaire vu avec `y` vers le haut (oracle
/// indépendant : aire signée et produits vectoriels, `y` image inversé).
fn oracle_ccw_convex(p: &[(f64, f64); 4]) -> bool {
    let q: Vec<(f64, f64)> = p.iter().map(|v| (v.0, -v.1)).collect();
    let cross = |a: (f64, f64), b: (f64, f64), c: (f64, f64)| {
        (b.0 - a.0) * (c.1 - b.1) - (b.1 - a.1) * (c.0 - b.0)
    };
    (0..4).all(|i| cross(q[i], q[(i + 1) % 4], q[(i + 2) % 4]) > 1e-6)
}

fn distinct(p: &[(f64, f64)]) -> bool {
    (0..p.len()).all(|i| (0..i).all(|j| p[i] != p[j]))
}

fn oracle_cal_ok(c: &MCal, w: u32, h: u32) -> bool {
    match c {
        MCal::Two { p, d } => {
            p.iter().all(|q| inside(*q, w, h))
                && distinct(p)
                && d.is_finite()
                && *d > 0.0
                && (p[0].0 - p[1].0).hypot(p[0].1 - p[1].1) >= 1e-6
        }
        MCal::Four { p, w: cw, h: ch } => {
            p.iter().all(|q| inside(*q, w, h))
                && distinct(p)
                && cw.is_finite()
                && *cw > 0.0
                && ch.is_finite()
                && *ch > 0.0
                && oracle_ccw_convex(p)
        }
    }
}

fn spec_of(c: &MCal) -> CalibrationSpec {
    match c {
        MCal::Two { p, d } => CalibrationSpec::TwoPoints {
            points_px: [pp(p[0].0, p[0].1), pp(p[1].0, p[1].1)],
            known_distance_mm: *d,
        },
        MCal::Four { p, w, h } => CalibrationSpec::FourPoints {
            points_px: [
                pp(p[0].0, p[0].1),
                pp(p[1].0, p[1].1),
                pp(p[2].0, p[2].1),
                pp(p[3].0, p[3].1),
            ],
            known_width_mm: *w,
            known_height_mm: *h,
        },
    }
}

/// Carte mm -> pixels générée par le test (perspective douce), qui sert d'oracle de vérité terrain.
#[derive(Clone, Copy)]
struct Truth {
    a: f64,
    b: f64,
    c: f64,
    d: f64,
    e: f64,
    f: f64,
    g: f64,
    h: f64,
}

impl Truth {
    fn to_px(self, m: (f64, f64)) -> (f64, f64) {
        let den = self.g * m.0 + self.h * m.1 + 1.0;
        (
            (self.a * m.0 + self.b * m.1 + self.c) / den,
            (self.d * m.0 + self.e * m.1 + self.f) / den,
        )
    }
}

type TruthQuad = (Truth, f64, f64, [(f64, f64); 4]);

/// Rectangle `W × H` mm vu en perspective dans l'image, ou `None` si l'échantillon ne convient pas.
fn truth_quad(g: &mut Lcg) -> Option<TruthQuad> {
    let (w, h) = (g.range(30.0, 90.0), g.range(20.0, 60.0));
    let s = g.range(0.25, 0.45);
    let t = Truth {
        a: s * g.range(0.9, 1.1),
        b: g.range(-0.05, 0.05),
        c: g.range(8.0, 20.0),
        d: g.range(-0.05, 0.05),
        e: -s * g.range(0.9, 1.1),
        f: g.range(36.0, 46.0),
        g: g.range(0.0, 0.002),
        h: g.range(0.0, 0.002),
    };
    let corners = [(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)];
    let p = [
        t.to_px(corners[0]),
        t.to_px(corners[1]),
        t.to_px(corners[2]),
        t.to_px(corners[3]),
    ];
    (p.iter().all(|q| inside(*q, IMG_W, IMG_H))
        && oracle_ccw_convex(&p)
        && (p[0].0 - p[2].0).abs() > 5.0)
        .then_some((t, w, h, p))
}

// ----- compteurs de couverture ---------------------------------------------------------------

#[derive(Default)]
struct Cov(BTreeMap<&'static str, usize>);

impl Cov {
    fn hit(&mut self, k: &'static str) {
        *self.0.entry(k).or_insert(0) += 1;
    }
    fn n(&self, k: &str) -> usize {
        self.0.get(k).copied().unwrap_or(0)
    }
}

// ----- une séquence --------------------------------------------------------------------------

struct Img {
    rel: String,
    sha: String,
}

fn run_sequence(seed: u64, cov: &mut Cov) {
    let mut g = Lcg(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0xA5A5);
    let dir = tmp("adverse_fond", &format!("s{seed}"));
    let file = dir.join("proj.pforge.json");
    let mut doc = Document::new(Project::new("Adverse"));
    let sheet = doc.project().sheets[0].id;
    persist::save(&file, doc.project()).unwrap();

    // Jeu d'images générées : PNG gris, PNG RVB, JPEG (même taille), + un PNG d'une autre taille.
    let sources = [
        ("a.png", png_gray(IMG_W, IMG_H, checker(4))),
        (
            "b.png",
            png_rgb(IMG_W, IMG_H, |x, y| [(x * 4) as u8, (y * 5) as u8, 90]),
        ),
        ("c.jpg", jpeg_gray(IMG_W as u16, IMG_H as u16, noise(seed))),
        ("d.png", png_gray(IMG_W + 8, IMG_H, rect(10, 10, 40, 30))),
    ];
    let mut imgs: Vec<(Img, u32, u32)> = Vec::new();
    for (name, bytes) in &sources {
        std::fs::write(dir.join(name), bytes).unwrap();
        let im = bi::import_image(&file, &dir.join(name)).expect("import valide");
        cov.hit(if name.ends_with(".jpg") {
            "import_jpeg"
        } else {
            "import_png"
        });
        // L'empreinte est celle des octets de la copie (relue ici, indépendamment).
        let copy = std::fs::read(dir.join(&im.relative)).unwrap();
        assert_eq!(copy, *bytes, "copie fidèle");
        assert_eq!(im.sha256.len(), 64);
        imgs.push((
            Img {
                rel: im.relative.clone(),
                sha: im.sha256.clone(),
            },
            im.width_px,
            im.height_px,
        ));
    }
    // Fichiers non importables : refus propre, aucun fichier ne reste dans le dossier des images.
    let before_count = std::fs::read_dir(dir.join("proj.assets")).unwrap().count();
    let png = &sources[0].1;
    for (name, bytes, key) in [
        (
            "tronque.png",
            png[..png.len() / 3].to_vec(),
            "import_tronque",
        ),
        ("texte.png", b"pas une image".to_vec(), "import_non_image"),
        ("vide.jpg", Vec::new(), "import_vide"),
        (
            "jpeg_tronque.jpg",
            sources[2].1[..40].to_vec(),
            "import_jpeg_tronque",
        ),
    ] {
        std::fs::write(dir.join(name), &bytes).unwrap();
        assert!(bi::import_image(&file, &dir.join(name)).is_err(), "{name}");
        cov.hit(key);
    }
    assert!(bi::import_image(&file, &dir.join("absent.png")).is_err());
    cov.hit("import_absent");
    assert_eq!(
        std::fs::read_dir(dir.join("proj.assets")).unwrap().count(),
        before_count
    );

    let mut model: Option<Model> = None;
    let steps = 10 + g.below(14);
    for _ in 0..steps {
        let before_json = persist::to_json(doc.project()).unwrap();
        let op = g.below(11);
        let (cmd, expect_ok, next): (Command, bool, Option<Option<Model>>) = match op {
            // Pose valide à deux points.
            0 | 1 => {
                let k = g.below(3) as usize; // images de taille 64 × 48
                let (im, w, h) = &imgs[k];
                let p1 = (g.range(1.0, 30.0).round(), g.range(1.0, 40.0).round());
                let p2 = (p1.0 + g.range(5.0, 30.0).round(), p1.1 + g.below(3) as f64);
                let d = g.range(10.0, 400.0).round();
                let cal = MCal::Two { p: [p1, p2], d };
                let ok = model.is_none() && oracle_cal_ok(&cal, *w, *h);
                let m = Model {
                    image: im.rel.clone(),
                    sha: im.sha.clone(),
                    w: *w,
                    h: *h,
                    cal: cal.clone(),
                    opacity: 0.5,
                    visible: true,
                    locked: false,
                };
                (
                    Command::SetBackground {
                        sheet,
                        image: im.rel.clone(),
                        sha256: im.sha.clone(),
                        width_px: *w,
                        height_px: *h,
                        calibration: spec_of(&cal),
                    },
                    ok,
                    ok.then_some(Some(m)),
                )
            }
            // Pose valide à quatre points (perspective de vérité terrain connue).
            2 => {
                let mut found = None;
                for _ in 0..60 {
                    if let Some(q) = truth_quad(&mut g) {
                        found = Some(q);
                        break;
                    }
                }
                let Some((truth, w_mm, h_mm, p)) = found else {
                    continue;
                };
                let (im, w, h) = &imgs[g.below(3) as usize];
                let cal = MCal::Four {
                    p,
                    w: w_mm,
                    h: h_mm,
                };
                let ok = model.is_none() && oracle_cal_ok(&cal, *w, *h);
                let m = Model {
                    image: im.rel.clone(),
                    sha: im.sha.clone(),
                    w: *w,
                    h: *h,
                    cal: cal.clone(),
                    opacity: 0.5,
                    visible: true,
                    locked: false,
                };
                let c = Command::SetBackground {
                    sheet,
                    image: im.rel.clone(),
                    sha256: im.sha.clone(),
                    width_px: *w,
                    height_px: *h,
                    calibration: spec_of(&cal),
                };
                if ok {
                    // Le résultat est vérifié contre la vérité terrain après application (voir plus bas).
                    cov.hit("truth_four_pending");
                    let out = doc.execute(c).expect("pose à quatre points valide");
                    let _ = out;
                    let b = doc.project().sheets[0].background.clone().unwrap();
                    let map = b.map().unwrap();
                    for _ in 0..12 {
                        let m_mm = (g.range(0.0, w_mm), g.range(0.0, h_mm));
                        let px = truth.to_px(m_mm);
                        let back = map.to_mm(PointMm::new(px.0, px.1)).unwrap();
                        let tol = 1e-6 * (1.0 + w_mm.max(h_mm));
                        assert!(
                            (back.x - m_mm.0).abs() < tol && (back.y - m_mm.1).abs() < tol,
                            "perspective : {back:?} vs {m_mm:?}"
                        );
                    }
                    let r = b.residuals().unwrap();
                    assert!(r.rms_mm < 1e-6 && r.per_point_mm.len() == 4);
                    cov.hit("four_points_truth_ok");
                    model = Some(m);
                    // Annulation exacte à l'octet.
                    assert!(doc.undo());
                    assert_eq!(persist::to_json(doc.project()).unwrap(), before_json);
                    assert!(doc.redo());
                    cov.hit("undo_redo_exact");
                    continue;
                }
                (c, ok, None)
            }
            // Pose dégénérée (une des familles de refus), sur un projet sans fond.
            3 => {
                let (im, w, h) = &imgs[g.below(3) as usize];
                let family = g.below(14);
                let (mut image, mut sha, mut ww, mut hh) = (im.rel.clone(), im.sha.clone(), *w, *h);
                let mut cal = MCal::Two {
                    p: [(5.0, 5.0), (25.0, 5.0)],
                    d: 50.0,
                };
                let key: &'static str = match family {
                    0 => {
                        cal = MCal::Two {
                            p: [(5.0, 5.0), (5.0, 5.0)],
                            d: 50.0,
                        };
                        "refus_points_confondus"
                    }
                    1 => {
                        cal = MCal::Two {
                            p: [(5.0, 5.0), (25.0, 5.0)],
                            d: 0.0,
                        };
                        "refus_distance_nulle"
                    }
                    2 => {
                        cal = MCal::Two {
                            p: [(5.0, 5.0), (25.0, 5.0)],
                            d: -3.0,
                        };
                        "refus_distance_negative"
                    }
                    3 => {
                        cal = MCal::Two {
                            p: [(5.0, 5.0), (25.0, 5.0)],
                            d: f64::NAN,
                        };
                        "refus_distance_nan"
                    }
                    4 => {
                        cal = MCal::Two {
                            p: [(5.0, 5.0), (25.0, 5.0)],
                            d: f64::INFINITY,
                        };
                        "refus_distance_infinie"
                    }
                    5 => {
                        cal = MCal::Two {
                            p: [(5.0, 5.0), (1000.0, 5.0)],
                            d: 50.0,
                        };
                        "refus_hors_image"
                    }
                    6 => {
                        cal = MCal::Four {
                            p: [(0.0, 0.0), (10.0, 0.0), (20.0, 0.0), (30.0, 0.0)],
                            w: 10.0,
                            h: 10.0,
                        };
                        "refus_alignes"
                    }
                    7 => {
                        cal = MCal::Four {
                            p: [(10.0, 40.0), (10.0, 5.0), (50.0, 5.0), (50.0, 40.0)],
                            w: 40.0,
                            h: 30.0,
                        };
                        "refus_sens_horaire"
                    }
                    8 => {
                        cal = MCal::Four {
                            p: [(10.0, 40.0), (50.0, 5.0), (50.0, 40.0), (10.0, 5.0)],
                            w: 40.0,
                            h: 30.0,
                        };
                        "refus_croise"
                    }
                    9 => {
                        image = "../evil.png".into();
                        "refus_chemin_parent"
                    }
                    10 => {
                        image = if g.below(2) == 0 {
                            "C:/evil.png".into()
                        } else {
                            "/evil.png".into()
                        };
                        "refus_chemin_absolu"
                    }
                    11 => {
                        image = "a\\b.png".into();
                        "refus_antislash"
                    }
                    12 => {
                        sha = if g.below(2) == 0 {
                            sha.to_uppercase()
                        } else {
                            sha[..63].to_string()
                        };
                        "refus_empreinte_forme"
                    }
                    _ => {
                        if g.below(2) == 0 {
                            ww = 0
                        } else {
                            hh = 100_001
                        };
                        "refus_dimensions"
                    }
                };
                if matches!(family, 5 | 9..=13) && key == "refus_hors_image" {
                    // rien : image valide
                }
                (
                    Command::SetBackground {
                        sheet,
                        image,
                        sha256: sha,
                        width_px: ww,
                        height_px: hh,
                        calibration: spec_of(&cal),
                    },
                    false,
                    {
                        cov.hit(key);
                        None
                    },
                )
            }
            // Recalibrage (valide ou dégénéré).
            4 | 5 => {
                let valid = g.below(2) == 0;
                let (w, h) = model.as_ref().map_or((IMG_W, IMG_H), |m| (m.w, m.h));
                let cal = if valid {
                    let p1 = (g.range(1.0, 20.0).round(), g.range(1.0, 20.0).round());
                    MCal::Two {
                        p: [p1, (p1.0 + 17.0, p1.1 + 3.0)],
                        d: g.range(5.0, 300.0).round(),
                    }
                } else {
                    MCal::Two {
                        p: [(3.0, 3.0), (3.0, 3.0)],
                        d: 10.0,
                    }
                };
                let ok = model.as_ref().is_some_and(|m| !m.locked) && oracle_cal_ok(&cal, w, h);
                let next = ok.then(|| {
                    let mut m = model.clone().unwrap();
                    m.cal = cal.clone();
                    Some(m)
                });
                if model.is_none() {
                    cov.hit("calibrer_sans_fond");
                }
                (
                    Command::CalibrateBackground {
                        sheet,
                        calibration: spec_of(&cal),
                    },
                    ok,
                    next,
                )
            }
            // Affichage (opacité valide ou hors bornes, visibilité, verrou).
            6 | 7 => {
                let bad = g.below(5) == 0;
                let opacity = if bad {
                    [-0.01, 1.01, f64::NAN, f64::INFINITY, 2.0][g.below(5) as usize]
                } else {
                    (g.range(0.0, 1.0) * 100.0).round() / 100.0
                };
                let (visible, locked) = (g.below(2) == 0, g.below(3) == 0);
                let ok = model.is_some() && !bad;
                let next = ok.then(|| {
                    let mut m = model.clone().unwrap();
                    m.opacity = opacity;
                    m.visible = visible;
                    m.locked = locked;
                    Some(m)
                });
                if bad {
                    cov.hit("refus_opacite");
                }
                (
                    Command::SetBackgroundDisplay {
                        sheet,
                        opacity,
                        visible,
                        locked,
                    },
                    ok,
                    next,
                )
            }
            // Reliage : même taille (valide si déverrouillé) ou taille différente (refus).
            8 => {
                let different = g.below(3) == 0;
                let (im, w, h) = if different {
                    &imgs[3]
                } else {
                    &imgs[g.below(3) as usize]
                };
                let ok = model
                    .as_ref()
                    .is_some_and(|m| !m.locked && (m.w, m.h) == (*w, *h));
                let next = ok.then(|| {
                    let mut m = model.clone().unwrap();
                    m.image = im.rel.clone();
                    m.sha = im.sha.clone();
                    Some(m)
                });
                if different && model.is_some() {
                    cov.hit("reliage_dimensions_differentes");
                }
                (
                    Command::RelinkBackground {
                        sheet,
                        image: im.rel.clone(),
                        sha256: im.sha.clone(),
                        width_px: *w,
                        height_px: *h,
                    },
                    ok,
                    next,
                )
            }
            // Retrait.
            9 => {
                let ok = model.as_ref().is_some_and(|m| !m.locked);
                (Command::RemoveBackground { sheet }, ok, ok.then_some(None))
            }
            // Feuille inconnue.
            _ => {
                cov.hit("refus_feuille_inconnue");
                (
                    Command::SetBackgroundDisplay {
                        sheet: SheetId(987_654),
                        opacity: 0.5,
                        visible: true,
                        locked: false,
                    },
                    false,
                    None,
                )
            }
        };
        let mut was_locked_refusal = false;
        if !expect_ok {
            if let Some(m) = &model {
                if m.locked
                    && matches!(
                        cmd,
                        Command::CalibrateBackground { .. }
                            | Command::RelinkBackground { .. }
                            | Command::RemoveBackground { .. }
                    )
                {
                    was_locked_refusal = true;
                }
            }
        }
        let result = doc.execute(cmd);
        assert_eq!(
            result.is_ok(),
            expect_ok,
            "séquence {seed} : verdict de l'oracle"
        );
        if expect_ok {
            model = next.expect("état attendu");
            cov.hit("commande_acceptee");
            // Annulation exacte à l'octet, puis rétablissement identique.
            let after_json = persist::to_json(doc.project()).unwrap();
            if after_json == before_json {
                // Commande acceptée sans effet : aucune étape d'historique (rien à annuler).
                cov.hit("sans_effet");
            } else {
                assert!(doc.undo());
                assert_eq!(
                    persist::to_json(doc.project()).unwrap(),
                    before_json,
                    "annulation exacte"
                );
                assert!(doc.redo());
                assert_eq!(
                    persist::to_json(doc.project()).unwrap(),
                    after_json,
                    "rétablissement exact"
                );
                cov.hit("undo_redo_exact");
            }
        } else {
            // Refus sans trace : octets inchangés, aucune étape ajoutée.
            assert_eq!(
                persist::to_json(doc.project()).unwrap(),
                before_json,
                "refus sans trace"
            );
            cov.hit("refus_sans_trace");
            if was_locked_refusal {
                cov.hit("refus_verrouille");
            }
        }
        // Le projet réel est conforme au modèle de l'oracle.
        check_model(&doc, &model, cov);
    }

    // Aller-retour du fichier et validation ; un fichier modifié à la main est refusé proprement.
    let json = persist::to_json(doc.project()).unwrap();
    let back = persist::from_json(&json).unwrap();
    assert_eq!(back, *doc.project());
    persist::save(&file, doc.project()).unwrap();
    assert_eq!(persist::load(&file).unwrap(), *doc.project());
    if let Some(m) = &model {
        for (from, to, key) in [
            (
                format!("\"image\": \"{}\"", m.image),
                "\"image\": \"../x.png\"".to_string(),
                "json_chemin_parent",
            ),
            (
                format!("\"sha256\": \"{}\"", m.sha),
                format!("\"sha256\": \"{}\"", m.sha.to_uppercase()),
                "json_empreinte_majuscules",
            ),
            (
                "\"opacity\"".to_string(),
                "\"opacity\": 3.0, \"x_opacity\"".to_string(),
                "json_opacite",
            ),
        ] {
            let pretty = serde_json::to_string_pretty(
                &serde_json::from_str::<serde_json::Value>(&json).unwrap(),
            )
            .unwrap();
            let bad = pretty.replace(&from, &to);
            if bad != pretty {
                assert!(persist::from_json(&bad).is_err(), "{key}");
                cov.hit(key);
            }
        }
        // Fichier image : statut ok, puis modifié, puis supprimé, puis rétabli.
        let b = doc.project().sheets[0].background.clone().unwrap();
        let path = dir.join(&b.image);
        let original = std::fs::read(&path).unwrap();
        let st = |b: &Background| bi::status(&dir, b);
        if b.sha256 == m.sha && st(&b) == bi::ImageStatus::Ok {
            cov.hit("statut_ok");
        }
        let mut changed = original.clone();
        changed.push(0);
        std::fs::write(&path, &changed).unwrap();
        assert_eq!(st(&b), bi::ImageStatus::Modified);
        cov.hit("statut_modifie");
        assert!(bi::load(&dir, &b).is_err());
        std::fs::remove_file(&path).unwrap();
        assert_eq!(st(&b), bi::ImageStatus::Missing);
        cov.hit("statut_manquant");
        // Le projet s'ouvre toujours, sans perte, même sans le fichier.
        assert_eq!(persist::load(&file).unwrap(), *doc.project());
        cov.hit("ouverture_sans_image");
        std::fs::write(&path, &original).unwrap();
        assert_eq!(st(&b), bi::ImageStatus::Ok);
        cov.hit("statut_retabli");
        // Empreinte fausse posée par un appelant : le cœur la stocke, le statut la dénonce.
        let mut forged = doc.project().clone();
        forged.sheets[0].background.as_mut().unwrap().sha256 = "0".repeat(64);
        assert_eq!(
            st(forged.sheets[0].background.as_ref().unwrap()),
            bi::ImageStatus::Modified
        );
        cov.hit("empreinte_fausse");
    }
}

/// Compare le projet réel au modèle de l'oracle (champs et cartes pixels -> mm).
fn check_model(doc: &Document, model: &Option<Model>, cov: &mut Cov) {
    let real = doc.project().sheets[0].background.as_ref();
    match (real, model) {
        (None, None) => {}
        (Some(b), Some(m)) => {
            assert_eq!(
                (b.image.as_str(), b.sha256.as_str(), b.width_px, b.height_px),
                (m.image.as_str(), m.sha.as_str(), m.w, m.h)
            );
            assert_eq!(
                (b.opacity, b.visible, b.locked),
                (m.opacity, m.visible, m.locked)
            );
            let map = b.map().unwrap();
            match &m.cal {
                MCal::Two { p, d } => {
                    let dist = (p[1].0 - p[0].0).hypot(p[1].1 - p[0].1);
                    let s = d / dist;
                    for q in [(0.0, 0.0), (10.0, 7.0), (63.0, 47.0)] {
                        let want = ((q.0 - p[0].0) * s, -(q.1 - p[0].1) * s);
                        let got = map.to_mm(PointMm::new(q.0, q.1)).unwrap();
                        assert!(
                            (got.x - want.0).abs() <= 1e-9 * (1.0 + want.0.abs())
                                && (got.y - want.1).abs() <= 1e-9 * (1.0 + want.1.abs()),
                            "{got:?} vs {want:?}"
                        );
                    }
                    let r = b.residuals().unwrap();
                    assert!(r.rms_mm < 1e-9 && r.per_point_mm.len() == 2);
                    cov.hit("carte_deux_points_verifiee");
                }
                MCal::Four { p, w, h } => {
                    // Les quatre coins reviennent exactement sur les coins du rectangle.
                    for (q, t) in p.iter().zip([(0.0, 0.0), (*w, 0.0), (*w, *h), (0.0, *h)]) {
                        let got = map.to_mm(PointMm::new(q.0, q.1)).unwrap();
                        assert!((got.x - t.0).abs() < 1e-6 && (got.y - t.1).abs() < 1e-6);
                    }
                    cov.hit("carte_quatre_points_verifiee");
                }
            }
        }
        (r, m) => panic!("état réel {:?} ≠ modèle {:?}", r.is_some(), m.is_some()),
    }
}

#[test]
fn campagne_adverse_160_sequences_oracle_independant() {
    let mut cov = Cov::default();
    for seed in 1..=SEQUENCES {
        run_sequence(seed, &mut cov);
    }
    // Couverture : chaque famille de refus et chaque issue a été exercée plusieurs fois.
    for (k, min) in [
        ("import_png", 400),
        ("import_jpeg", 100),
        ("import_tronque", 100),
        ("import_non_image", 100),
        ("import_vide", 100),
        ("import_jpeg_tronque", 100),
        ("commande_acceptee", 300),
        ("undo_redo_exact", 300),
        ("refus_sans_trace", 300),
        ("four_points_truth_ok", 20),
        ("carte_deux_points_verifiee", 100),
        ("carte_quatre_points_verifiee", 20),
        ("refus_points_confondus", 3),
        ("refus_distance_nulle", 3),
        ("refus_distance_negative", 3),
        ("refus_distance_nan", 3),
        ("refus_distance_infinie", 3),
        ("refus_hors_image", 3),
        ("refus_alignes", 3),
        ("refus_sens_horaire", 3),
        ("refus_croise", 3),
        ("refus_chemin_parent", 3),
        ("refus_chemin_absolu", 3),
        ("refus_antislash", 3),
        ("refus_empreinte_forme", 3),
        ("refus_dimensions", 3),
        ("refus_opacite", 10),
        ("refus_verrouille", 10),
        ("refus_feuille_inconnue", 10),
        ("reliage_dimensions_differentes", 5),
        ("calibrer_sans_fond", 3),
        ("statut_ok", 60),
        ("statut_modifie", 60),
        ("statut_manquant", 60),
        ("statut_retabli", 60),
        ("ouverture_sans_image", 60),
        ("empreinte_fausse", 60),
        ("json_chemin_parent", 60),
        ("json_empreinte_majuscules", 60),
        ("json_opacite", 60),
    ] {
        assert!(
            cov.n(k) >= min,
            "couverture « {k} » : {} < {min} ({:?})",
            cov.n(k),
            cov.0
        );
    }
}

#[test]
fn image_declaree_trop_grande_refusee_avant_tout_decodage() {
    // 8 000 x 7 000 px = 56 mégapixels (> 50 Mpx) : l'entête suffit à refuser, sans décoder.
    let bytes = png_gray(8000, 7000, |_, _| 0);
    let err = bi::probe(&bytes).unwrap_err();
    assert!(err.contains("trop grande"), "{err}");
    assert!(bi::decode(&bytes).is_err());
    // Une image de 0 x 0 n'existe pas dans un fichier valide ; un entête PNG falsifié est refusé.
    let mut forged = png_gray(8, 8, |_, _| 0);
    forged[16..20].copy_from_slice(&0u32.to_be_bytes());
    assert!(bi::probe(&forged).is_err());
}
