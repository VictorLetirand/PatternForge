//! US-09.8 — campagne adverse de l'export gradé : 160 séquences à graine fixe. Chaque séquence
//! tire un polygone (3 à 6 sommets), une gamme (2 à 6 tailles, base au hasard, libellés à
//! caractères parfois interdits dans un nom de fichier) et des écarts entiers par (sommet,
//! taille). L'**oracle** recalcule tout à la main sur « position de base + écart » : segments de la
//! scène, coordonnées SVG, périmètre, nid translaté de `base(ancre) − taille(ancre)`, longueur de
//! trait des motifs, noms de fichiers. Il ne lit ni `graded_project`, ni `size_nest`, ni les
//! constantes de l'écrivain.

mod common;

use common::contenu::{read_svg, Group};
use common::Rng;
use pf_core::*;
use pf_export::size_export::*;
use pf_export::{pdf, svg, OutputContent, PageSetup, PathGeom, Scene, SceneOptions};
use pf_geometry::PointMm;

const SEQUENCES: u64 = 160;
const TOL: f64 = 1e-6;
/// Le SVG écrit 4 décimales au plus.
const SVG_TOL: f64 = 2e-4;
const LABELS: [&str; 9] = ["XS", "S", "M", "L", "XL", "XXL", "3/4", "5.5", "A:B"];
/// Motifs attendus, par rang parmi les tailles autres que la base (écrits à la main).
const PATTERNS: [&[f64]; 6] = [
    &[8.0, 5.0],
    &[1.5, 3.0],
    &[10.0, 3.0, 1.5, 3.0],
    &[18.0, 5.0],
    &[10.0, 3.0, 1.5, 3.0, 1.5, 3.0],
    &[3.0, 2.5],
];

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

fn dist(a: PointMm, b: PointMm) -> f64 {
    (a.x - b.x).hypot(a.y - b.y)
}

fn on_len(len: f64, pattern: &[f64]) -> f64 {
    if pattern.is_empty() {
        return len;
    }
    let period: f64 = pattern.iter().sum();
    let per_period: f64 = pattern.iter().step_by(2).sum();
    let full = (len / period).floor();
    let mut rest = len - full * period;
    let mut on = full * per_period;
    for (i, e) in pattern.iter().enumerate() {
        let part = rest.min(*e);
        if i % 2 == 0 {
            on += part;
        }
        rest -= part;
        if rest <= 0.0 {
            break;
        }
    }
    on
}

fn dist_to_segment(p: PointMm, a: PointMm, b: PointMm) -> f64 {
    let (vx, vy) = (b.x - a.x, b.y - a.y);
    let t = (((p.x - a.x) * vx + (p.y - a.y) * vy) / (vx * vx + vy * vy)).clamp(0.0, 1.0);
    (p.x - (a.x + t * vx)).hypot(p.y - (a.y + t * vy))
}

fn oracle_sanitize(label: &str) -> String {
    let s: String = label
        .trim()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if s.is_empty() {
        "x".into()
    } else {
        s
    }
}

struct Seq {
    p: Project,
    sheet: SheetId,
    /// Positions de base des sommets.
    base: Vec<PointMm>,
    corners: Vec<PointId>,
    /// Tailles de la gamme, dans l'ordre, avec libellé.
    sizes: Vec<(SizeId, String)>,
    base_index: usize,
    /// Écart (dx ; dy) de chaque sommet par taille (nul pour la base).
    delta: Vec<Vec<(f64, f64)>>,
}

fn build(seed: u64) -> Seq {
    let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    for _ in 0..3 {
        rng.next();
    }
    let n = 3 + rng.below(4) as usize;
    let base: Vec<PointMm> = (0..n)
        .map(|i| {
            let angle = (i as f64 + rng.range(-0.15, 0.15)) / n as f64 * std::f64::consts::TAU;
            let r = rng.range(150.0, 300.0);
            pt(500.0 + r * angle.cos(), 500.0 + r * angle.sin())
        })
        .collect();
    let k = 2 + rng.below(5) as usize;
    let mut pool: Vec<&str> = LABELS.to_vec();
    let mut labels: Vec<String> = Vec::new();
    for _ in 0..k {
        let i = rng.below(pool.len() as u64) as usize;
        labels.push(pool.remove(i).to_string());
    }
    let base_index = rng.below(k as u64) as usize;
    let mut p = Project::new("Campagne gradée");
    let sheet = p.sheets[0].id;
    p.apply(Command::AddSizeRange {
        name: "Gamme".into(),
        kind: SizeKind::Alphanumeric,
        labels: labels.clone(),
        base: base_index,
    })
    .unwrap();
    let sizes: Vec<(SizeId, String)> = p.sizes[0]
        .sizes
        .iter()
        .map(|s| (s.id, s.label.clone()))
        .collect();
    let polygon = p
        .apply(Command::AddPolygon {
            sheet,
            vertices: base.iter().map(|b| PointRef::New(*b)).collect(),
            closed: true,
            role: PathRole::Construction,
            layer: None,
        })
        .unwrap();
    let corners = polygon.points.clone();
    assert_eq!(corners.len(), n);
    let mut delta = vec![vec![(0.0, 0.0); n]; k];
    for (si, (size, _)) in sizes.iter().enumerate() {
        if si == base_index {
            continue;
        }
        for (ci, corner) in corners.iter().enumerate() {
            if rng.unit() < 0.3 {
                continue;
            }
            let (dx, dy) = loop {
                let d = (
                    rng.range(-25.0, 25.0).round(),
                    rng.range(-25.0, 25.0).round(),
                );
                if d != (0.0, 0.0) {
                    break d;
                }
            };
            p.apply(Command::SetPointGrading {
                sheet,
                point: *corner,
                size: *size,
                dx_mm: dx,
                dy_mm: dy,
            })
            .unwrap();
            delta[si][ci] = (dx, dy);
        }
    }
    Seq {
        p,
        sheet,
        base,
        corners,
        sizes,
        base_index,
        delta,
    }
}

impl Seq {
    /// Position du sommet `ci` dans la taille `si` : base + écart (oracle).
    fn at(&self, si: usize, ci: usize) -> PointMm {
        let b = self.base[ci];
        pt(b.x + self.delta[si][ci].0, b.y + self.delta[si][ci].1)
    }

    fn edges(&self, si: usize, shift: (f64, f64)) -> Vec<(PointMm, PointMm)> {
        let n = self.base.len();
        (0..n)
            .map(|i| {
                let (a, b) = (self.at(si, i), self.at(si, (i + 1) % n));
                (
                    pt(a.x + shift.0, a.y + shift.1),
                    pt(b.x + shift.0, b.y + shift.1),
                )
            })
            .collect()
    }
}

fn lines_of(scene: &Scene) -> Vec<(PointMm, PointMm)> {
    scene
        .paths
        .iter()
        .filter_map(|p| match p.geom {
            PathGeom::Line(a, b) => Some((a, b)),
            _ => None,
        })
        .collect()
}

fn same_pair(x: (PointMm, PointMm), y: (PointMm, PointMm)) -> bool {
    let eq = |a: PointMm, b: PointMm| dist(a, b) < 1e-9;
    (eq(x.0, y.0) && eq(x.1, y.1)) || (eq(x.0, y.1) && eq(x.1, y.0))
}

fn scene_of(s: &Seq, size: Option<SizeId>) -> Scene {
    size_scene(
        &s.p,
        size,
        s.sheet,
        &OutputContent::default(),
        SceneOptions::default(),
    )
    .unwrap()
}

#[test]
fn campagne_export_gradue_160_sequences_oracle_independant() {
    for seed in 0..SEQUENCES {
        let s = build(seed);
        let before = persist::to_json(&s.p).unwrap();
        let n = s.base.len();
        let ctx = format!("séquence {seed} (n = {n}, tailles = {})", s.sizes.len());

        // --- une taille : segments, périmètre, coordonnées SVG ---------------------------------
        for (si, (id, label)) in s.sizes.iter().enumerate() {
            let want = s.edges(si, (0.0, 0.0));
            let scene = scene_of(&s, Some(*id));
            let got = lines_of(&scene);
            assert_eq!(got.len(), n, "{ctx} {label} : nombre de segments");
            for w in &want {
                assert!(
                    got.iter().any(|g| same_pair(*g, *w)),
                    "{ctx} {label} : segment {w:?} absent de {got:?}"
                );
            }
            let perimeter: f64 = want.iter().map(|(a, b)| dist(*a, *b)).sum();
            let exported: f64 = got.iter().map(|(a, b)| dist(*a, *b)).sum();
            assert!(
                (perimeter - exported).abs() < TOL,
                "{ctx} {label} : périmètre"
            );
            // SVG : (x − xmin + 10 ; ymax − y + 10) sur l'emprise des sommets de la taille.
            let xs: Vec<f64> = (0..n).map(|i| s.at(si, i).x).collect();
            let ys: Vec<f64> = (0..n).map(|i| s.at(si, i).y).collect();
            let (xmin, ymax) = (
                xs.iter().copied().fold(f64::MAX, f64::min),
                ys.iter().copied().fold(f64::MIN, f64::max),
            );
            let (xmax, ymin) = (
                xs.iter().copied().fold(f64::MIN, f64::max),
                ys.iter().copied().fold(f64::MAX, f64::min),
            );
            let text = svg::export_svg(&scene).unwrap();
            let doc = read_svg(&text);
            assert!(
                (doc.width - (xmax - xmin + 20.0)).abs() < SVG_TOL
                    && (doc.height - (ymax - ymin + 20.0)).abs() < SVG_TOL,
                "{ctx} {label} : emprise SVG"
            );
            let svg_lines: Vec<Vec<(f64, f64)>> = doc
                .elems
                .iter()
                .filter(|e| e.group == Group::Paths && e.tag == "line")
                .map(|e| e.pts.clone())
                .collect();
            assert_eq!(svg_lines.len(), n, "{ctx} {label}");
            for (a, b) in &want {
                let (ta, tb) = (
                    (a.x - xmin + 10.0, ymax - a.y + 10.0),
                    (b.x - xmin + 10.0, ymax - b.y + 10.0),
                );
                let near = |p: (f64, f64), q: (f64, f64)| {
                    (p.0 - q.0).abs() < SVG_TOL && (p.1 - q.1).abs() < SVG_TOL
                };
                assert!(
                    svg_lines.iter().any(|l| (near(l[0], ta) && near(l[1], tb))
                        || (near(l[0], tb) && near(l[1], ta))),
                    "{ctx} {label} : segment SVG {ta:?} → {tb:?}"
                );
            }
            // Titre : nom de la taille hors base, rien pour la base.
            let titled = text.contains(&format!("Taille {label} · "));
            assert_eq!(titled, si != s.base_index, "{ctx} {label} : titre");
            // Base : octets identiques à l'export d'avant (SVG et PDF).
            if si == s.base_index {
                let plain = Scene::from_sheet(&s.p, s.sheet).unwrap();
                assert_eq!(text, svg::export_svg(&plain).unwrap(), "{ctx}");
                let a = pdf::export_pdf_content(
                    &[scene],
                    &PageSetup::default(),
                    &OutputContent::default(),
                )
                .unwrap()
                .bytes;
                assert_eq!(a, pdf::export_pdf(&[plain]).unwrap().bytes, "{ctx}");
            }
            // Nom : base ou aucune taille = nom d'avant, sinon `-` et libellé assaini.
            let info = export_size(&s.p, *id).unwrap();
            let stem = size_file_stem("P", Some(&info));
            if si == s.base_index {
                assert_eq!(stem, "P", "{ctx}");
            } else {
                assert_eq!(stem, format!("P-{}", oracle_sanitize(label)), "{ctx}");
            }
        }

        // --- sélection : un fichier par taille, noms uniques, aucun séparateur -----------------
        let ids: Vec<SizeId> = s.sizes.iter().map(|x| x.0).collect();
        let files = size_files(
            &s.p,
            s.sheet,
            &ids,
            "P",
            SizeFormat::Svg,
            &PageSetup::default(),
            &OutputContent::default(),
            SceneOptions::default(),
        );
        assert_eq!(files.len(), ids.len(), "{ctx}");
        let mut lower: Vec<String> = files.iter().map(|f| f.file_name.to_lowercase()).collect();
        for (f, (_, label)) in files.iter().zip(&s.sizes) {
            assert!(f.bytes.is_ok(), "{ctx} {label}");
            assert!(
                !f.file_name
                    .contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|']),
                "{ctx} : {}",
                f.file_name
            );
            assert!(
                f.file_name
                    .starts_with(&format!("P-{}", oracle_sanitize(label))),
                "{ctx} : {} pour {label}",
                f.file_name
            );
        }
        lower.sort();
        lower.dedup();
        assert_eq!(lower.len(), files.len(), "{ctx} : noms non uniques");

        // --- nid : contours translatés de base(ancre) − taille(ancre) --------------------------
        let anchor = if seed % 5 == 0 {
            None
        } else {
            Some((seed as usize) % n)
        };
        let nest =
            s.p.size_nest(
                s.sheet,
                s.p.sizes[0].id,
                anchor.map_or(NestAnchor::Origin, |i| NestAnchor::Point(s.corners[i])),
            )
            .unwrap();
        let mut rank = 0;
        for (si, (id, label)) in s.sizes.iter().enumerate() {
            let is_base = si == s.base_index;
            let pattern: &[f64] = if is_base {
                &[]
            } else {
                rank += 1;
                PATTERNS[(rank - 1) % PATTERNS.len()]
            };
            let shift = match (anchor, is_base) {
                (Some(a), false) => {
                    let (b, t) = (s.at(s.base_index, a), s.at(si, a));
                    (b.x - t.x, b.y - t.y)
                }
                _ => (0.0, 0.0),
            };
            let want = s.edges(si, shift);
            let out = nest_scene(&s.p, &nest, &[*id], pf_i18n::Lang::Fr).unwrap();
            let all = lines_of(&out.scene);
            let contour = &all[..all.len() - 4];
            for (a, b) in contour {
                assert!(
                    want.iter().any(|e| dist_to_segment(*a, e.0, e.1) < TOL
                        && dist_to_segment(*b, e.0, e.1) < TOL),
                    "{ctx} nid {label} : tronçon {a:?} → {b:?} hors du contour attendu"
                );
            }
            let got: f64 = contour.iter().map(|(a, b)| dist(*a, *b)).sum();
            let on: f64 = want
                .iter()
                .map(|(a, b)| on_len(dist(*a, *b), pattern))
                .sum();
            assert!((got - on).abs() < TOL, "{ctx} nid {label} : {got} ≠ {on}");
            if is_base {
                assert_eq!(contour.len(), n, "{ctx} nid base en plein");
                for w in &want {
                    assert!(contour.iter().any(|g| same_pair(*g, *w)), "{ctx} nid base");
                }
            }
            // Étiquette de la taille (base marquée) et point d'empilement nommé.
            let tag = if is_base {
                format!("{label} (base)")
            } else {
                label.clone()
            };
            let labels: Vec<&str> = out.scene.points.iter().map(|p| p.label.as_str()).collect();
            assert!(
                labels.contains(&tag.as_str()),
                "{ctx} nid : {tag} dans {labels:?}"
            );
            assert!(labels.contains(&"Empilement"), "{ctx}");
        }

        // --- aucune écriture dans le projet ----------------------------------------------------
        assert_eq!(persist::to_json(&s.p).unwrap(), before, "{ctx}");
    }
}
