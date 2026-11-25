//! US-06.8 — lignes spéciales à l'export : chaque rôle (surpiqûre, milieu devant / dos, lignes de
//! taille, de hanche, de poitrine, de parage, de collage) a une représentation **distincte sans la
//! couleur** (épaisseur et pointillé, mesurés dans le SVG et dans le PDF) et un attribut nommé
//! dans le SVG ; une surpiqûre liée est exportée comme les autres lignes de rôle surpiqûre ; sans
//! ligne spéciale, rien ne change à l'octet (légende de la page étalon comprise).

#[path = "common/pdf_text.rs"]
mod pdf_text;

use pf_core::{Command, PathRole, PointRef, Project, SheetId, TopstitchSide, TopstitchSource};
use pf_export::pdf::export_pdf;
use pf_export::svg::export_svg;
use pf_export::{stroke_style, PathGeom, Scene, MM_TO_PT};
use pf_geometry::PointMm;

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

/// Projet avec un segment horizontal de 80 mm du rôle demandé.
fn one_line(role: PathRole) -> (Project, SheetId) {
    let mut p = Project::new("ligne spéciale");
    let s = p.sheets[0].id;
    let e = p
        .apply(Command::AddSegment {
            sheet: s,
            from: PointRef::New(pt(0.0, 0.0)),
            to: PointRef::New(pt(80.0, 0.0)),
        })
        .unwrap()
        .entities[0];
    p.apply(Command::SetPathRole {
        sheet: s,
        entity: e,
        role,
    })
    .unwrap();
    (p, s)
}

fn svg_id(role: PathRole) -> &'static str {
    match role {
        PathRole::Construction => "construction",
        PathRole::SewingContour => "couture",
        PathRole::CuttingContour => "coupe",
        PathRole::Grainline => "droit-fil",
        PathRole::Fold => "pliure",
        PathRole::AnnotationAxis => "axe-repere",
        PathRole::Topstitch => "surpiqure",
        PathRole::CenterFront => "milieu-devant",
        PathRole::CenterBack => "milieu-dos",
        PathRole::WaistLine => "ligne-taille",
        PathRole::HipLine => "ligne-hanche",
        PathRole::BustLine => "ligne-poitrine",
        PathRole::SkivingLine => "ligne-parage",
        PathRole::GlueLine => "ligne-collage",
    }
}

fn num(v: f64) -> String {
    let mut s = format!("{v:.4}");
    while s.contains('.') && (s.ends_with('0') || s.ends_with('.')) {
        s.pop();
    }
    s
}

fn page_streams(bytes: &[u8]) -> Vec<String> {
    let text = pdf_text::readable(bytes);
    let mut pages = Vec::new();
    let mut rest = text.as_str();
    while let Some(start) = rest.find("stream\n") {
        let body = &rest[start + 7..];
        let end = body.find("endstream").unwrap();
        pages.push(body[..end].to_string());
        rest = &body[end + 9..];
    }
    pages
}

/// Épaisseurs (mm) et pointillés (mm) déclarés dans un flux : « g G w [d1 d2 …] 0 d ».
fn strokes(stream: &str) -> Vec<(f64, Vec<f64>)> {
    let mut out = Vec::new();
    for line in stream.lines() {
        let Some(open) = line.find('[') else { continue };
        let Some(close) = line.find(']') else {
            continue;
        };
        if !line.trim_end().ends_with("0 d") || !line[..open].contains(" w ") {
            continue;
        }
        let head: Vec<&str> = line[..open].split_whitespace().collect();
        let Some(w) = head.iter().rev().nth(1).and_then(|t| t.parse::<f64>().ok()) else {
            continue;
        };
        let dash: Vec<f64> = line[open + 1..close]
            .split_whitespace()
            .filter_map(|t| t.parse::<f64>().ok())
            .map(|v| v / MM_TO_PT)
            .collect();
        out.push((w / MM_TO_PT, dash));
    }
    out
}

fn same(a: &[f64], b: &[f64]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() < 2e-3)
}

#[test]
fn chaque_role_a_une_epaisseur_et_un_pointille_distincts() {
    // Distincts deux à deux : le rôle ne repose jamais sur la couleur (l'export est en noir).
    let styles: Vec<(PathRole, f64, Vec<f64>)> = PathRole::ALL
        .iter()
        .map(|&r| {
            let st = stroke_style(r);
            (r, st.width_mm, st.dash_mm.to_vec())
        })
        .collect();
    assert_eq!(styles.len(), 14);
    for (i, a) in styles.iter().enumerate() {
        for b in &styles[i + 1..] {
            assert!(
                a.1 != b.1 || a.2 != b.2,
                "{:?} et {:?} ont le même trait ({} mm, {:?})",
                a.0,
                b.0,
                a.1,
                a.2
            );
        }
    }
    // Les huit lignes spéciales ont un pointillé d'un motif différent de tous les autres rôles
    // (pas seulement une autre épaisseur) et un nombre pair de valeurs (trait / intervalle).
    for r in PathRole::SPECIAL {
        let st = stroke_style(r);
        assert!(
            !st.dash_mm.is_empty(),
            "{r:?} : un rôle spécial est pointillé"
        );
        assert_eq!(st.dash_mm.len() % 2, 0, "{r:?}");
        for other in PathRole::ALL.iter().filter(|o| **o != r) {
            assert_ne!(
                stroke_style(*other).dash_mm,
                st.dash_mm,
                "{r:?} et {other:?} ont le même pointillé"
            );
        }
    }
    // Les six rôles d'avant US-06.8 gardent exactement leur trait.
    let base = [
        (PathRole::Construction, 0.25, vec![]),
        (PathRole::SewingContour, 0.35, vec![3.0, 2.0]),
        (PathRole::CuttingContour, 0.6, vec![]),
        (PathRole::Grainline, 0.35, vec![10.0, 1.5, 1.5, 1.5]),
        (PathRole::Fold, 0.35, vec![6.0, 2.0, 1.0, 2.0]),
        (PathRole::AnnotationAxis, 0.25, vec![0.6, 1.4]),
    ];
    for (r, w, d) in base {
        let st = stroke_style(r);
        assert_eq!((st.width_mm, st.dash_mm.to_vec()), (w, d), "{r:?}");
    }
}

#[test]
fn svg_chaque_role_a_son_attribut_nomme_epaisseur_et_pointille() {
    let mut ids: Vec<&str> = PathRole::ALL.iter().map(|r| svg_id(*r)).collect();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), 14, "noms d'attribut distincts");
    for role in PathRole::SPECIAL {
        let (p, s) = one_line(role);
        let svg = export_svg(&Scene::from_sheet(&p, s).unwrap()).unwrap();
        let st = stroke_style(role);
        let dash: Vec<String> = st.dash_mm.iter().map(|v| num(*v)).collect();
        let line = svg
            .lines()
            .find(|l| l.contains("<line") && l.contains(&format!("data-role=\"{}\"", svg_id(role))))
            .unwrap_or_else(|| panic!("{role:?} : aucun trait nommé dans le SVG"));
        assert!(
            line.contains(&format!("stroke-width=\"{}\"", num(st.width_mm))),
            "{role:?} : {line}"
        );
        assert!(
            line.contains(&format!("stroke-dasharray=\"{}\"", dash.join(" "))),
            "{role:?} : {line}"
        );
        // Longueur du trait : 80 mm exactement (1 unité = 1 mm).
        let attr = |name: &str| -> f64 {
            let key = format!("{name}=\"");
            let i = line.find(&key).unwrap() + key.len();
            line[i..].split('"').next().unwrap().parse().unwrap()
        };
        let len = (attr("x2") - attr("x1")).hypot(attr("y2") - attr("y1"));
        assert!((len - 80.0).abs() < 1e-3, "{role:?} : {len}");
        // Aucun autre rôle dans ce fichier.
        for other in PathRole::ALL.iter().filter(|o| **o != role) {
            assert!(
                !svg.contains(&format!("data-role=\"{}\"", svg_id(*other))),
                "{role:?} : {other:?} présent"
            );
        }
    }
}

#[test]
fn pdf_chaque_role_est_dessine_avec_son_trait_mesure_et_sa_legende() {
    for role in PathRole::SPECIAL {
        let (p, s) = one_line(role);
        let scene = Scene::from_sheet(&p, s).unwrap();
        let pdf = export_pdf(std::slice::from_ref(&scene)).unwrap();
        let pages = page_streams(&pdf.bytes);
        assert!(pages.len() >= 2, "étalon + au moins une page de tracé");
        let st = stroke_style(role);
        // Page de tracé : le trait est déclaré avec l'épaisseur et le pointillé du rôle (mm
        // relus depuis les pt, 72 / 25,4), suivi du segment de 80 mm.
        let tile = &pages[1];
        let declared = strokes(tile);
        assert!(
            declared
                .iter()
                .any(|(w, d)| (w - st.width_mm).abs() < 2e-3 && same(d, st.dash_mm)),
            "{role:?} : trait absent de la page de tracé ({declared:?})"
        );
        // Page étalon : la légende décrit la ligne (le rôle se lit sans couleur).
        let legend = &pages[0];
        assert!(
            strokes(legend)
                .iter()
                .any(|(w, d)| (w - st.width_mm).abs() < 2e-3 && same(d, st.dash_mm)),
            "{role:?} : échantillon absent de la légende"
        );
    }
    // Les fragments ASCII des libellés de la légende.
    let want = [
        (PathRole::Topstitch, "Surpiq"),
        (PathRole::CenterFront, "Milieu devant"),
        (PathRole::CenterBack, "Milieu dos"),
        (PathRole::WaistLine, "Ligne de taille"),
        (PathRole::HipLine, "Ligne de hanche"),
        (PathRole::BustLine, "Ligne de poitrine"),
        (PathRole::SkivingLine, "Ligne de parage"),
        (PathRole::GlueLine, "Ligne de collage"),
    ];
    for (role, text) in want {
        let (p, s) = one_line(role);
        let pdf = export_pdf(&[Scene::from_sheet(&p, s).unwrap()]).unwrap();
        let all = pdf_text::readable(&pdf.bytes);
        assert!(all.contains(text), "{role:?} : légende « {text} » absente");
        // Seule la légende du rôle utilisé est ajoutée.
        for (other, other_text) in want {
            if other != role {
                assert!(
                    !all.contains(other_text),
                    "{role:?} : légende de {other:?} ajoutée à tort"
                );
            }
        }
    }
}

#[test]
fn sans_ligne_speciale_la_page_etalon_et_les_traits_sont_inchanges() {
    let (p, s) = one_line(PathRole::Fold);
    let scene = Scene::from_sheet(&p, s).unwrap();
    let svg = export_svg(&scene).unwrap();
    assert!(svg.contains("data-role=\"pliure\""));
    for r in PathRole::SPECIAL {
        assert!(!svg.contains(&format!("data-role=\"{}\"", svg_id(r))));
    }
    let pdf = pdf_text::readable(&export_pdf(&[scene]).unwrap().bytes);
    for text in [
        "Surpiq",
        "Milieu devant",
        "Milieu dos",
        "Ligne de taille",
        "Ligne de hanche",
        "Ligne de poitrine",
        "Ligne de parage",
        "Ligne de collage",
    ] {
        assert!(!pdf.contains(text), "{text}");
    }
    // Les six lignes de légende historiques sont toujours là.
    for text in [
        "Construction",
        "Ligne de couture",
        "Ligne de coupe",
        "Droit-fil",
        "Pliure",
    ] {
        assert!(pdf.contains(text), "{text}");
    }
    // La page étalon seule reste identique à l'exemple livré.
    let shipped = std::fs::read(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/etalon-100mm.pdf"),
    )
    .unwrap();
    let fresh = pf_export::pdf::export_calibration_pdf().bytes;
    assert_eq!(shipped, fresh);
}

#[test]
fn une_surpiqure_liee_est_exportee_comme_une_ligne_de_surpiqure() {
    let mut p = Project::new("surpiqûre");
    let s = p.sheets[0].id;
    let r = p
        .apply(Command::AddRectangle {
            sheet: s,
            corner: PointRef::New(pt(0.0, 0.0)),
            width_mm: 100.0,
            height_mm: 60.0,
        })
        .unwrap();
    for &e in &r.entities {
        p.apply(Command::SetPathRole {
            sheet: s,
            entity: e,
            role: PathRole::CuttingContour,
        })
        .unwrap();
    }
    p.apply(Command::AddTopstitch {
        sheet: s,
        source: TopstitchSource::Paths {
            paths: r.entities.clone(),
            from: None,
        },
        distance_mm: 5.0,
        side: TopstitchSide::Inside,
    })
    .unwrap();
    let scene = Scene::from_sheet(&p, s).unwrap();
    let stitched: Vec<_> = scene
        .paths
        .iter()
        .filter(|x| x.role == PathRole::Topstitch)
        .collect();
    assert_eq!(stitched.len(), 4);
    // Lignes de 90 × 50 mm, exportées en mm à l'échelle 1:1.
    let mut lengths: Vec<f64> = stitched
        .iter()
        .map(|x| match x.geom {
            PathGeom::Line(a, b) => pf_geometry::distance_mm(a, b),
            _ => panic!("segment attendu"),
        })
        .collect();
    lengths.sort_by(f64::total_cmp);
    assert_eq!(
        lengths
            .iter()
            .map(|l| (l * 1e6).round() as i64)
            .collect::<Vec<_>>(),
        vec![50_000_000, 50_000_000, 90_000_000, 90_000_000]
    );
    let svg = export_svg(&scene).unwrap();
    assert_eq!(svg.matches("data-role=\"surpiqure\"").count(), 4);
    assert_eq!(svg.matches("data-role=\"coupe\"").count(), 4);
    assert_eq!(
        scene.roles_used(),
        vec![PathRole::CuttingContour, PathRole::Topstitch]
    );
    // PDF : les deux traits (coupe continu, surpiqûre pointillée) sont déclarés.
    let pdf = export_pdf(std::slice::from_ref(&scene)).unwrap();
    let pages = page_streams(&pdf.bytes);
    let declared = strokes(&pages[1]);
    let st = stroke_style(PathRole::Topstitch);
    assert!(declared
        .iter()
        .any(|(w, d)| (w - st.width_mm).abs() < 2e-3 && same(d, st.dash_mm)));
}
